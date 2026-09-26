//! MCP (Model Context Protocol) servers as tool sources.
//!
//! # What this is
//! An MCP server exposes tools over JSON-RPC: over a child process' stdio (`npx`, `uvx`, any
//! script) or over HTTP. Rather than inventing a parallel tool path, a server is adapted to the
//! existing [`ToolHost`] contract, so its tools are aggregated, namespaced, circuit-broken and
//! routed exactly like plugin tools — the model cannot tell the difference.
//!
//! Tools are exposed as `mcp__<server>__<tool>`: the prefix keeps them recognisable in logs and
//! prevents an MCP tool from silently shadowing a plugin tool with the same name.
//!
//! # Lifecycle
//! Servers are connected lazily on first use and probed by [`McpPool::spawn_watchdog`]. A server
//! that stops answering is marked `reconnecting` (or `failed` once its budget is exhausted) and
//! its process is replaced on the next successful probe, so a crashed MCP server degrades to
//! "its tools are unavailable" instead of breaking every conversation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kanon_llm::tool_router::{ToolHost, json_to_prost_struct};
use kanon_proto::v1::{PluginMeta, ToolCallRequest, ToolCallResponse, ToolMeta};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};

use crate::instance::BotInstance;
use crate::toggle::{MCP_SECTION, ToggleStore};

/// Default MCP configuration path, relative to the node working directory.
pub const DEFAULT_MCP_CONFIG: &str = "./data/mcp.json";

/// Per-request deadline for an MCP call.
pub const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the MCP watchdog probes every configured server.
pub const MCP_WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);

/// Consecutive failed probes before a server is parked as failed.
pub const MCP_MAX_FAILURES: u32 = 3;

/// Failures raised while configuring or talking to MCP servers.
#[derive(Debug, Error)]
pub enum McpError {
    /// The server is not configured on this node.
    #[error("MCP server '{0}' is not configured")]
    NotFound(String),
    /// The configuration document could not be read or written.
    #[error("MCP configuration failed: {0}")]
    Config(String),
    /// The transport could not be established.
    #[error("MCP transport failed: {0}")]
    Transport(String),
    /// The server answered with a JSON-RPC error.
    #[error("MCP server '{server}' rejected '{method}': {message}")]
    Rpc {
        /// Server identifier.
        server: String,
        /// Method that failed.
        method: String,
        /// Error message reported by the server.
        message: String,
    },
}

/// How to reach an MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    /// Server runs as a child process speaking JSON-RPC on stdio.
    Stdio {
        /// Executable to run.
        command: String,
        /// Arguments passed to it.
        #[serde(default)]
        args: Vec<String>,
        /// Extra environment variables.
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// Server answers JSON-RPC over HTTP POST.
    Http {
        /// Endpoint URL.
        url: String,
        /// Extra headers (authorization, etc.).
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

/// One configured MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Stable identifier, also used in tool names.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Transport configuration.
    pub transport: McpTransport,
}

/// Document persisted at the MCP configuration path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct McpDocument {
    /// Schema version.
    #[serde(default = "default_version")]
    version: u32,
    /// Configured servers.
    #[serde(default)]
    servers: Vec<McpServerConfig>,
    /// Unrecognized keys are preserved verbatim.
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

/// Current MCP schema version.
fn default_version() -> u32 {
    1
}

/// Persisted MCP configuration.
#[derive(Debug)]
pub struct McpConfigStore {
    /// Path of the configuration document; `None` for an in-memory store.
    path: Option<PathBuf>,
    /// Configured servers by identifier.
    servers: RwLock<HashMap<String, McpServerConfig>>,
}

impl Default for McpConfigStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

impl McpConfigStore {
    /// Creates a configuration that lives only for this process.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            servers: RwLock::new(HashMap::new()),
        }
    }

    /// Opens (or creates) the configuration document.
    pub async fn open(path: impl Into<PathBuf>) -> Result<Self, McpError> {
        let path = path.into();
        let document = read_document(&path)?;
        let servers = document
            .map(|document| {
                document
                    .servers
                    .into_iter()
                    .map(|server| (server.id.clone(), server))
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            path: Some(path),
            servers: RwLock::new(servers),
        })
    }

    /// Lists configured servers, ordered by identifier.
    pub async fn list(&self) -> Vec<McpServerConfig> {
        let mut servers: Vec<McpServerConfig> =
            self.servers.read().await.values().cloned().collect();
        servers.sort_by(|a, b| a.id.cmp(&b.id));
        servers
    }

    /// Returns one server configuration.
    pub async fn get(&self, id: &str) -> Option<McpServerConfig> {
        self.servers.read().await.get(id).cloned()
    }

    /// Inserts or replaces a server configuration.
    pub async fn upsert(&self, config: McpServerConfig) -> Result<(), McpError> {
        validate_config(&config)?;
        let mut servers = self.servers.write().await;
        servers.insert(config.id.clone(), config);
        self.persist(&servers)
    }

    /// Removes a server configuration.
    pub async fn remove(&self, id: &str) -> Result<bool, McpError> {
        let mut servers = self.servers.write().await;
        let removed = servers.remove(id).is_some();
        if removed {
            self.persist(&servers)?;
        }
        Ok(removed)
    }

    /// Atomically writes the configuration with owner-only permissions.
    fn persist(&self, servers: &HashMap<String, McpServerConfig>) -> Result<(), McpError> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                McpError::Config(format!("failed to create {}: {err}", parent.display()))
            })?;
        }

        let mut document = read_document(path)?.unwrap_or_default();
        document.version = default_version();
        let mut ordered: Vec<McpServerConfig> = servers.values().cloned().collect();
        ordered.sort_by(|a, b| a.id.cmp(&b.id));
        document.servers = ordered;

        let payload = serde_json::to_string_pretty(&document)
            .map_err(|err| McpError::Config(format!("failed to serialize MCP config: {err}")))?;

        let temp_path = path.with_extension("json.tmp");
        std::fs::write(&temp_path, payload).map_err(|err| {
            McpError::Config(format!("failed to write {}: {err}", temp_path.display()))
        })?;

        // The document may carry server credentials in headers/env.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600)).map_err(
                |err| {
                    McpError::Config(format!("failed to restrict {}: {err}", temp_path.display()))
                },
            )?;
        }

        std::fs::rename(&temp_path, path).map_err(|err| {
            McpError::Config(format!(
                "failed to move {} into place at {}: {err}",
                temp_path.display(),
                path.display()
            ))
        })
    }
}

/// Validates a server description before it is stored.
fn validate_config(config: &McpServerConfig) -> Result<(), McpError> {
    let id = config.id.trim();
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(McpError::Config(format!(
            "invalid MCP server id '{}': use letters, digits, '-' or '_'",
            config.id
        )));
    }
    if config.name.trim().is_empty() {
        return Err(McpError::Config("server name must not be empty".into()));
    }
    match &config.transport {
        McpTransport::Stdio { command, .. } if command.trim().is_empty() => {
            Err(McpError::Config("stdio command must not be empty".into()))
        }
        McpTransport::Http { url, .. }
            if !url.starts_with("http://") && !url.starts_with("https://") =>
        {
            Err(McpError::Config(
                "HTTP MCP url must start with http:// or https://".into(),
            ))
        }
        _ => Ok(()),
    }
}

/// Reads the configuration document, returning `None` when it does not exist.
fn read_document(path: &Path) -> Result<Option<McpDocument>, McpError> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|err| McpError::Config(format!("failed to read {}: {err}", path.display())))?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|err| McpError::Config(format!("failed to parse {}: {err}", path.display())))
}

/// Live connection to one MCP server.
enum Connection {
    /// Child process speaking JSON-RPC on stdio.
    Stdio {
        /// Child process handle.
        ///
        /// Never read directly, but it must stay alive: dropping it closes the pipes and, with
        /// `kill_on_drop`, stops the server process, which is exactly what reconnecting needs.
        #[allow(dead_code)]
        child: Child,
        /// Line reader over the child's stdout.
        stdout: BufReader<tokio::process::ChildStdout>,
        /// Child's stdin for outgoing requests.
        stdin: tokio::process::ChildStdin,
    },
    /// HTTP endpoint.
    Http {
        /// Shared HTTP client.
        client: reqwest::Client,
        /// Endpoint URL.
        url: String,
        /// Extra headers.
        headers: HashMap<String, String>,
    },
}

/// Runtime health of one MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpHealth {
    /// `connected`, `connecting`, `reconnecting`, `failed` or `disabled`.
    pub state: String,
    /// Tools currently advertised by the server.
    pub tools: usize,
    /// Consecutive failed probes.
    pub failures: u32,
    /// Last observed error, when any.
    pub last_error: Option<String>,
}

impl Default for McpHealth {
    fn default() -> Self {
        Self {
            state: "connecting".to_string(),
            tools: 0,
            failures: 0,
            last_error: None,
        }
    }
}

/// One MCP server, adapted to the [`ToolHost`] contract.
pub struct McpServer {
    /// Static configuration.
    config: McpServerConfig,
    /// Host identifier used in tool metadata (`mcp_<server>`), stable for the server's lifetime.
    host_id: String,
    /// Live connection, absent until the first successful connect.
    connection: Mutex<Option<Connection>>,
    /// Synthetic plugin metadata advertised to the tool router.
    ///
    /// A synchronous lock: [`ToolHost::plugin_metas`] is a synchronous trait method, so the
    /// metadata must be readable without awaiting. Critical sections only clone a `Vec`.
    meta: std::sync::RwLock<Vec<PluginMeta>>,
    /// Health snapshot for the console.
    health: Mutex<McpHealth>,
    /// Monotonic JSON-RPC request id.
    next_request_id: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("id", &self.config.id)
            .finish_non_exhaustive()
    }
}

impl McpServer {
    /// Creates a server handle from its configuration.
    pub fn new(config: McpServerConfig) -> Self {
        let host_id = host_id(&config.id);
        Self {
            config,
            host_id,
            connection: Mutex::new(None),
            meta: std::sync::RwLock::new(Vec::new()),
            health: Mutex::new(McpHealth::default()),
            next_request_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Configuration of this server.
    pub fn config(&self) -> &McpServerConfig {
        &self.config
    }

    /// Current health snapshot.
    pub async fn health(&self) -> McpHealth {
        self.health.lock().await.clone()
    }

    /// Connects (if needed), performs the MCP handshake and refreshes the tool list.
    pub async fn connect(&self) -> Result<(), McpError> {
        // Fast path: already connected and no refresh requested.
        if self.connection.lock().await.is_some() {
            return Ok(());
        }

        let attempt = async {
            let connection = self.open_transport().await?;
            *self.connection.lock().await = Some(connection);

            if let Err(err) = self.handshake().await {
                // A server that cannot complete the handshake is unusable: drop the transport so
                // the next attempt starts clean instead of reusing a half-initialized process.
                *self.connection.lock().await = None;
                return Err(err);
            }

            self.refresh_tools().await
        }
        .await;

        // The health snapshot is the console's only view of this server, so it is updated on both
        // outcomes here rather than left at its initial value.
        match &attempt {
            Ok(()) => self.record_success().await,
            Err(err) => {
                self.record_failure(err).await;
            }
        }
        attempt
    }

    /// Records a live server in the health snapshot.
    async fn record_success(&self) {
        // Read the tool count before taking the health lock so two locks are never held at once.
        let tools = self.tool_count();
        let mut health = self.health.lock().await;
        health.state = "connected".to_string();
        health.failures = 0;
        health.last_error = None;
        health.tools = tools;
    }

    /// Records a failed attempt, preserving the concrete cause for the console.
    async fn record_failure(&self, err: &McpError) {
        let mut health = self.health.lock().await;
        health.failures += 1;
        health.state = if health.failures >= MCP_MAX_FAILURES {
            "failed".to_string()
        } else {
            "reconnecting".to_string()
        };
        health.last_error = Some(err.to_string());
        health.tools = 0;
    }

    /// Drops the live connection and marks the server as not currently connected.
    ///
    /// Used by the watchdog before reconnecting and by the console when a server is switched off;
    /// the connection is genuinely gone either way, so the health snapshot must say so.
    pub async fn disconnect(&self) {
        *self.connection.lock().await = None;
        let mut health = self.health.lock().await;
        health.state = "disconnected".to_string();
        health.tools = 0;
    }

    /// Opens the configured transport.
    async fn open_transport(&self) -> Result<Connection, McpError> {
        match &self.config.transport {
            McpTransport::Stdio { command, args, env } => {
                let mut cmd = Command::new(command);
                cmd.args(args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                for (key, value) in env {
                    cmd.env(key, value);
                }

                let mut child = cmd.spawn().map_err(|err| {
                    McpError::Transport(format!(
                        "failed to start MCP server '{}' ({}): {err}",
                        self.config.id, command
                    ))
                })?;

                let stdin = child
                    .stdin
                    .take()
                    .ok_or_else(|| McpError::Transport("child stdin unavailable".into()))?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| McpError::Transport("child stdout unavailable".into()))?;

                Ok(Connection::Stdio {
                    child,
                    stdout: BufReader::new(stdout),
                    stdin,
                })
            }
            McpTransport::Http { url, headers } => Ok(Connection::Http {
                client: reqwest::Client::new(),
                url: url.clone(),
                headers: headers.clone(),
            }),
        }
    }

    /// Performs the MCP `initialize` handshake.
    async fn handshake(&self) -> Result<(), McpError> {
        self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "kanon", "version": env!("CARGO_PKG_VERSION") }
            }),
        )
        .await?;

        // Fire-and-forget: the specification only requires the notification, not a response.
        let _ = self
            .notify("notifications/initialized", serde_json::json!({}))
            .await;
        Ok(())
    }

    /// Refreshes the advertised tool list and rebuilds the synthetic metadata.
    pub async fn refresh_tools(&self) -> Result<(), McpError> {
        let response = self.request("tools/list", serde_json::json!({})).await?;
        let tools = response
            .get("tools")
            .and_then(|tools| tools.as_array())
            .cloned()
            .unwrap_or_default();

        let mut metas = Vec::with_capacity(tools.len());
        for tool in &tools {
            let Some(name) = tool.get("name").and_then(|name| name.as_str()) else {
                continue;
            };
            let description = tool
                .get("description")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let parameters = tool
                .get("inputSchema")
                .cloned()
                .map(|schema| json_to_prost_struct(&schema).unwrap_or_default());

            metas.push(ToolMeta {
                name: qualified_tool_name(&self.config.id, name),
                description,
                parameters,
            });
        }

        *self
            .meta
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = vec![PluginMeta {
            id: host_id(&self.config.id),
            name: self.config.name.clone(),
            version: "mcp".to_string(),
            author: "MCP".to_string(),
            description: format!("Model Context Protocol server '{}'", self.config.name),
            commands: Vec::new(),
            tools: metas,
        }];

        Ok(())
    }

    /// Calls one tool by its *MCP* name (unqualified).
    pub async fn call(&self, tool: &str, arguments: serde_json::Value) -> Result<String, McpError> {
        self.connect().await?;

        let response = self
            .request(
                "tools/call",
                serde_json::json!({ "name": tool, "arguments": arguments }),
            )
            .await?;

        // MCP returns `content: [{type: "text", text: ...}]`; anything else is passed through as
        // JSON so the model still sees a result instead of an empty string.
        if let Some(items) = response.get("content").and_then(|value| value.as_array()) {
            let text: Vec<String> = items
                .iter()
                .filter_map(|item| item.get("text").and_then(|text| text.as_str()))
                .map(str::to_string)
                .collect();
            if !text.is_empty() {
                return Ok(text.join("\n"));
            }
        }

        Ok(response.to_string())
    }

    /// Sends one JSON-RPC request and waits for its response.
    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let id = self
            .next_request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut guard = self.connection.lock().await;
        let Some(connection) = guard.as_mut() else {
            return Err(McpError::Transport(format!(
                "MCP server '{}' is not connected",
                self.config.id
            )));
        };

        let response = match connection {
            Connection::Stdio { stdout, stdin, .. } => {
                let mut line = serde_json::to_vec(&payload).map_err(|err| {
                    McpError::Transport(format!("failed to encode request: {err}"))
                })?;
                line.push(b'\n');
                stdin.write_all(&line).await.map_err(|err| {
                    McpError::Transport(format!("failed to write request: {err}"))
                })?;
                stdin.flush().await.map_err(|err| {
                    McpError::Transport(format!("failed to flush request: {err}"))
                })?;

                // Read until the matching response id; notifications are ignored.
                let mut buffer = String::new();
                loop {
                    buffer.clear();
                    let read =
                        tokio::time::timeout(MCP_REQUEST_TIMEOUT, stdout.read_line(&mut buffer))
                            .await
                            .map_err(|_| {
                                McpError::Transport(format!(
                                    "MCP server '{}' did not answer '{method}' in time",
                                    self.config.id
                                ))
                            })?
                            .map_err(|err| {
                                McpError::Transport(format!("failed to read response: {err}"))
                            })?;

                    if read == 0 {
                        return Err(McpError::Transport(format!(
                            "MCP server '{}' closed its output while answering '{method}'",
                            self.config.id
                        )));
                    }

                    let trimmed = buffer.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(message) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                        // Servers may log to stdout; ignore anything that is not JSON-RPC.
                        continue;
                    };
                    if message.get("id").and_then(|value| value.as_u64()) != Some(id) {
                        continue;
                    }
                    break message;
                }
            }
            Connection::Http {
                client,
                url,
                headers,
            } => {
                let mut request = client.post(url.as_str()).json(&payload);
                for (key, value) in headers {
                    request = request.header(key.as_str(), value.as_str());
                }
                let response = tokio::time::timeout(MCP_REQUEST_TIMEOUT, request.send())
                    .await
                    .map_err(|_| {
                        McpError::Transport(format!(
                            "MCP server '{}' did not answer '{method}' in time",
                            self.config.id
                        ))
                    })?
                    .map_err(|err| McpError::Transport(format!("HTTP request failed: {err}")))?;

                let status = response.status();
                let body = response.text().await.map_err(|err| {
                    McpError::Transport(format!("failed to read HTTP response: {err}"))
                })?;

                if !status.is_success() {
                    return Err(McpError::Transport(format!(
                        "MCP server '{}' answered HTTP {status}",
                        self.config.id
                    )));
                }

                // Streamable HTTP servers may answer with an SSE stream; take the last data line.
                let json = body
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:"))
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .next_back()
                    .unwrap_or(body.trim());
                serde_json::from_str::<serde_json::Value>(json).map_err(|err| {
                    McpError::Transport(format!("failed to decode HTTP response: {err}"))
                })?
            }
        };

        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error");
            return Err(McpError::Rpc {
                server: self.config.id.clone(),
                method: method.to_string(),
                message: message.to_string(),
            });
        }

        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Sends one JSON-RPC notification (no response expected).
    async fn notify(&self, method: &str, params: serde_json::Value) -> Result<(), McpError> {
        let payload = serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let mut guard = self.connection.lock().await;
        match guard.as_mut() {
            Some(Connection::Stdio { stdin, .. }) => {
                let mut line = serde_json::to_vec(&payload).map_err(|err| {
                    McpError::Transport(format!("failed to encode notice: {err}"))
                })?;
                line.push(b'\n');
                stdin
                    .write_all(&line)
                    .await
                    .map_err(|err| McpError::Transport(format!("failed to write notice: {err}")))?;
                stdin
                    .flush()
                    .await
                    .map_err(|err| McpError::Transport(format!("failed to flush notice: {err}")))?;
            }
            Some(Connection::Http { .. }) | None => {}
        }
        Ok(())
    }
}

/// Host identifier used for MCP servers in tool metadata.
pub fn host_id(server_id: &str) -> String {
    format!("mcp_{server_id}")
}

/// Qualified tool name exposed to the model.
pub fn qualified_tool_name(server_id: &str, tool: &str) -> String {
    format!("mcp__{server_id}__{tool}")
}

/// Recovers the MCP tool name from its qualified form.
pub fn unqualified_tool_name(server_id: &str, qualified: &str) -> Option<String> {
    qualified
        .strip_prefix(&format!("mcp__{server_id}__"))
        .map(str::to_string)
}

#[async_trait]
impl ToolHost for McpServer {
    fn host_id(&self) -> &str {
        &self.host_id
    }

    fn plugin_metas(&self) -> Vec<PluginMeta> {
        self.meta
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    async fn call_tool(&self, req: ToolCallRequest) -> Result<ToolCallResponse, tonic::Status> {
        let Some(tool) = unqualified_tool_name(&self.config.id, &req.tool_name) else {
            return Ok(ToolCallResponse {
                call_id: req.call_id,
                success: false,
                error_message: format!(
                    "Tool '{}' does not belong to MCP server '{}'",
                    req.tool_name, self.config.id
                ),
                payload: None,
            });
        };

        let arguments = match req.payload {
            Some(kanon_proto::v1::tool_call_request::Payload::StructuredArgs(args)) => {
                kanon_llm::tool_router::prost_struct_to_json(args)
            }
            _ => serde_json::json!({}),
        };

        match self.call(&tool, arguments).await {
            Ok(text) => {
                let result = json_to_prost_struct(&serde_json::json!({ "content": text }))
                    .unwrap_or_default();
                Ok(ToolCallResponse {
                    call_id: req.call_id,
                    success: true,
                    error_message: String::new(),
                    payload: Some(
                        kanon_proto::v1::tool_call_response::Payload::StructuredResult(result),
                    ),
                })
            }
            Err(err) => {
                tracing::warn!(server = %self.config.id, tool = %tool, error = %err, "MCP tool call failed");
                Ok(ToolCallResponse {
                    call_id: req.call_id,
                    success: false,
                    error_message: err.to_string(),
                    payload: None,
                })
            }
        }
    }
}

/// Every configured MCP server, connected on demand.
#[derive(Debug, Default)]
pub struct McpPool {
    /// Servers by identifier.
    servers: RwLock<HashMap<String, Arc<McpServer>>>,
}

impl McpPool {
    /// Creates an empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds the pool from the configuration document.
    pub async fn sync_from_config(&self, config: &McpConfigStore) {
        let configured = config.list().await;
        let mut servers = self.servers.write().await;

        // Drop servers that were removed, keep the rest so live connections survive a config edit.
        servers.retain(|id, _| configured.iter().any(|server| &server.id == id));
        for server in configured {
            match servers.get(&server.id) {
                Some(existing) if existing.config() == &server => {}
                _ => {
                    servers.insert(server.id.clone(), Arc::new(McpServer::new(server)));
                }
            }
        }
    }

    /// Returns one server.
    pub async fn get(&self, id: &str) -> Option<Arc<McpServer>> {
        self.servers.read().await.get(id).cloned()
    }

    /// Lists servers with their health, ordered by identifier.
    pub async fn describe(&self) -> Vec<(McpServerConfig, McpHealth)> {
        let servers: Vec<Arc<McpServer>> = self.servers.read().await.values().cloned().collect();
        let mut described = Vec::with_capacity(servers.len());
        for server in servers {
            described.push((server.config().clone(), server.health().await));
        }
        described.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        described
    }

    /// Tool hosts this instance may use.
    ///
    /// Both switches are honoured: the node-wide toggle and the instance's own override. A server
    /// is only connected when an instance actually reaches it, so an unused server costs nothing.
    pub async fn hosts_for_instance(
        &self,
        toggles: &ToggleStore,
        instance: Option<&BotInstance>,
    ) -> Vec<Arc<dyn ToolHost>> {
        let servers: Vec<Arc<McpServer>> = self.servers.read().await.values().cloned().collect();

        let mut hosts: Vec<Arc<dyn ToolHost>> = Vec::new();
        for server in servers {
            let id = server.config().id.clone();
            let globally_enabled = toggles.is_enabled(MCP_SECTION, &id).await;
            if !globally_enabled {
                continue;
            }
            if let Some(instance) = instance
                && !instance.allows_mcp(&id, globally_enabled)
            {
                continue;
            }
            // Connecting here keeps the first token of latency on the pipeline worker instead of
            // making the model wait for a handshake mid-conversation.
            if let Err(err) = server.connect().await {
                tracing::warn!(server = %id, error = %err, "MCP server unavailable; its tools are skipped");
                continue;
            }
            hosts.push(server as Arc<dyn ToolHost>);
        }
        hosts
    }

    /// Starts the MCP watchdog: probes every enabled server and reconnects when it stops answering.
    pub fn spawn_watchdog(
        self: &Arc<Self>,
        config: Arc<McpConfigStore>,
        toggles: Arc<ToggleStore>,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                pool.sync_from_config(&config).await;

                let servers: Vec<Arc<McpServer>> =
                    pool.servers.read().await.values().cloned().collect();
                for server in servers {
                    // A server switched off in the console is not probed: it holds no connection
                    // worth keeping, and the toggle store is the single source of that decision.
                    if !toggles.is_enabled(MCP_SECTION, &server.config().id).await {
                        continue;
                    }

                    // `tools/list` doubles as the liveness probe: it proves the transport, the
                    // handshake and the server's own dispatch loop are all working.
                    match server.refresh_tools().await {
                        Ok(()) => server.record_success().await,
                        Err(err) => {
                            server.disconnect().await;
                            server.record_failure(&err).await;
                            let failures = server.health.lock().await.failures;
                            tracing::warn!(
                                server = %server.config().id,
                                failures,
                                error = %err,
                                "MCP server liveness probe failed; it will be reconnected on the next attempt"
                            );
                        }
                    }
                }
            }
        })
    }
}

/// Tool metadata listing is asynchronous; this helper exposes it to the pipeline.
impl McpServer {
    /// Synthetic plugin metadata describing this server's tools.
    pub fn plugin_meta(&self) -> Vec<PluginMeta> {
        self.meta
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Tool definitions currently advertised by this server.
    pub fn tool_count(&self) -> usize {
        self.plugin_meta()
            .first()
            .map(|meta| meta.tools.len())
            .unwrap_or(0)
    }
}
