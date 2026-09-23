//! Sub-process plugin host supervisor and lifecycle manager.
//!
//! Responsible for spawning child plugin processes, managing per-host IPC endpoints
//! (`./run/host_<id>.sock`), verifying socket readiness, conducting initial
//! `GetPluginMeta` handshakes, and ensuring graceful process termination and cleanup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tonic::transport::Channel;

use kanon_proto::v1::message_pipeline_service_client::MessagePipelineServiceClient;
use kanon_proto::v1::plugin_host_service_client::PluginHostServiceClient;
use kanon_proto::v1::{
    CommandExecuteRequest, CommandExecuteResponse, GetPluginMetaRequest,
    PipelineEventRequest, PluginMeta, PreFilterResult, ToolCallRequest, ToolCallResponse,
};
use kanon_transport::{
    connect_ipc, core_socket_path, default_run_dir, host_socket_path,
};
use crate::manifest::PluginManifest;

/// Errors arising during supervisor operations.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// Standard I/O failure when launching child processes or inspecting filesystem sockets.
    #[error("I/O error in supervisor: {0}")]
    Io(#[from] std::io::Error),
    /// gRPC transport error when dialing host endpoint.
    #[error("Transport error connecting to host: {0}")]
    Transport(#[from] tonic::transport::Error),
    /// gRPC status error returned during RPC execution.
    #[error("RPC error during host communication: {0}")]
    Rpc(Box<tonic::Status>),
    /// Host failed to become ready within the allocated deadline.
    #[error("Host '{0}' timed out waiting for socket readiness")]
    Timeout(String),
    /// Child process exited prematurely before completing handshake.
    #[error("Host process '{host_id}' exited prematurely with exit status: {status}")]
    PrematureExit { host_id: String, status: String },
    /// Manifest parsing failure when spawning from static file.
    #[error("Failed to load plugin manifest: {0}")]
    Manifest(String),
    /// Requested host was not found in the supervisor registry.
    #[error("Host '{0}' not found in supervisor")]
    HostNotFound(String),
}

impl From<tonic::Status> for SupervisorError {
    fn from(status: tonic::Status) -> Self {
        Self::Rpc(Box::new(status))
    }
}

/// Represents a running child plugin host managed by the supervisor.
pub struct ManagedHost {
    /// Unique identifier for this host (e.g. `host_demo_rust`).
    pub host_id: String,
    /// Filesystem path to the dedicated IPC endpoint.
    pub socket_path: PathBuf,
    /// Sub-process child handle. Wrapped in a Mutex for exclusive wait/kill operations.
    child: Mutex<Option<Child>>,
    /// Client for host lifecycle operations (`PluginHostService`).
    host_client: Mutex<PluginHostServiceClient<Channel>>,
    /// Client for event and message dispatching (`MessagePipelineService`).
    pipeline_client: Mutex<MessagePipelineServiceClient<Channel>>,
    /// Cached metadata obtained during initial handshake.
    pub meta: Vec<PluginMeta>,
    /// Execution priority for pipeline scheduling (lower executes first, default 500).
    pub priority: i32,
}

impl ManagedHost {
    /// Creates a new `ManagedHost` with an established channel, primarily used in testing or direct registration.
    pub fn new(
        host_id: String,
        socket_path: PathBuf,
        channel: Channel,
        meta: Vec<PluginMeta>,
        priority: i32,
    ) -> Self {
        Self {
            host_id,
            socket_path,
            child: Mutex::new(None),
            host_client: Mutex::new(PluginHostServiceClient::new(channel.clone())),
            pipeline_client: Mutex::new(MessagePipelineServiceClient::new(channel)),
            meta,
            priority,
        }
    }
}

#[allow(clippy::result_large_err)]
impl ManagedHost {
    /// Dispatches an event through this host's pre-filter pipeline.
    pub async fn pre_filter(&self, req: PipelineEventRequest) -> Result<PreFilterResult, tonic::Status> {
        let mut client = self.pipeline_client.lock().await;
        let response = client.on_pre_filter(req).await?;
        Ok(response.into_inner())
    }

    /// Dispatches a slash command to this host for execution.
    pub async fn execute_command(
        &self,
        req: CommandExecuteRequest,
    ) -> Result<CommandExecuteResponse, tonic::Status> {
        let mut client = self.pipeline_client.lock().await;
        let response = client.on_execute_command(req).await?;
        Ok(response.into_inner())
    }

    /// Queries the host for fresh plugin metadata.
    pub async fn get_plugin_meta(&self) -> Result<Vec<PluginMeta>, tonic::Status> {
        let mut client = self.host_client.lock().await;
        let response = client.get_plugin_meta(GetPluginMetaRequest {}).await?;
        Ok(response.into_inner().plugins)
    }

    /// Dispatches a tool call to this host for execution via gRPC IPC.
    pub async fn on_call_tool(
        &self,
        req: ToolCallRequest,
    ) -> Result<ToolCallResponse, tonic::Status> {
        let mut client = self.pipeline_client.lock().await;
        let response = client.on_call_tool(req).await?;
        Ok(response.into_inner())
    }
}

#[tonic::async_trait]
impl kanon_llm::tool_router::ToolHost for ManagedHost {
    fn host_id(&self) -> &str {
        &self.host_id
    }

    fn plugin_metas(&self) -> &[PluginMeta] {
        &self.meta
    }

    async fn call_tool(
        &self,
        req: ToolCallRequest,
    ) -> Result<ToolCallResponse, tonic::Status> {
        self.on_call_tool(req).await
    }
}

/// Supervisor responsible for managing the lifecycle of out-of-process plugin hosts.
pub struct Supervisor {
    /// Base directory where IPC sockets and temporary state reside.
    run_dir: PathBuf,
    /// Path to the Kanon Core IPC server socket (`core.sock`).
    core_sock_path: PathBuf,
    /// Registry of active managed plugin host instances.
    hosts: Arc<RwLock<HashMap<String, Arc<ManagedHost>>>>,
}

impl Supervisor {
    /// Creates a new `Supervisor` instance.
    ///
    /// If `run_dir` is not specified, [`default_run_dir`] is used.
    /// If `core_sock_path` is not specified, [`core_socket_path`] within `run_dir` is used.
    pub fn new(run_dir: Option<PathBuf>, core_sock_path: Option<PathBuf>) -> Self {
        let run_dir = run_dir.unwrap_or_else(default_run_dir);
        let core_sock_path = core_sock_path.unwrap_or_else(|| core_socket_path(Some(&run_dir)));

        // Ensure the run directory exists for socket allocation.
        if !run_dir.exists() {
            let _ = std::fs::create_dir_all(&run_dir);
        }

        Self {
            run_dir,
            core_sock_path,
            hosts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns a reference to the active run directory.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Returns a reference to the Core socket path.
    pub fn core_sock_path(&self) -> &Path {
        &self.core_sock_path
    }

    /// Spawns a plugin host sub-process, waits for its IPC socket readiness,
    /// performs the initial `GetPluginMeta` handshake, and registers it with default priority (500).
    pub async fn spawn_plugin(
        &self,
        host_id: &str,
        executable_path: impl AsRef<Path>,
        args: &[&str],
    ) -> Result<Arc<ManagedHost>, SupervisorError> {
        self.spawn_plugin_with_priority(host_id, executable_path, args, 500).await
    }

    /// Spawns a plugin host sub-process with explicit execution priority,
    /// waits for its IPC socket readiness, performs the initial `GetPluginMeta`
    /// handshake, and registers it in the supervisor registry.
    pub async fn spawn_plugin_with_priority(
        &self,
        host_id: &str,
        executable_path: impl AsRef<Path>,
        args: &[&str],
        priority: i32,
    ) -> Result<Arc<ManagedHost>, SupervisorError> {
        let socket_path = host_socket_path(host_id, Some(&self.run_dir));

        // Clean up stale socket file if it exists prior to launching child.
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }

        tracing::info!(
            host_id = %host_id,
            priority = priority,
            executable = %executable_path.as_ref().display(),
            socket = %socket_path.display(),
            "Spawning plugin host process"
        );

        let mut cmd = Command::new(executable_path.as_ref());
        cmd.args(args)
            .env("KANON_HOST_ID", host_id)
            .env("KANON_HOST_SOCK", &socket_path)
            .env("KANON_CORE_SOCK", &self.core_sock_path)
            .env("KANON_RUN_DIR", &self.run_dir)
            .kill_on_drop(true);

        let mut child = cmd.spawn()?;

        // Wait for child process to bind the socket and become ready.
        // We poll every 50ms with a 5-second deadline.
        let channel = match self.wait_for_readiness(&mut child, host_id, &socket_path, Duration::from_secs(5)).await {
            Ok(ch) => ch,
            Err(e) => {
                // Terminate child process if readiness check fails to prevent orphan processes.
                let _ = child.kill().await;
                return Err(e);
            }
        };

        // Create gRPC clients for handshake and future message pipeline calls.
        let mut host_client = PluginHostServiceClient::new(channel.clone());
        let pipeline_client = MessagePipelineServiceClient::new(channel);

        // Perform initial GetPluginMeta handshake to verify contract compatibility
        // and discover static command/tool definitions.
        tracing::debug!(host_id = %host_id, "Conducting GetPluginMeta handshake");
        let meta_response = host_client
            .get_plugin_meta(GetPluginMetaRequest {})
            .await
            .map_err(|status| {
                tracing::error!(host_id = %host_id, error = %status, "Handshake failed");
                SupervisorError::from(status)
            })?;

        let plugins = meta_response.into_inner().plugins;
        tracing::info!(
            host_id = %host_id,
            plugin_count = plugins.len(),
            "Handshake completed successfully with plugin host"
        );

        let managed_host = Arc::new(ManagedHost {
            host_id: host_id.to_string(),
            socket_path: socket_path.clone(),
            child: Mutex::new(Some(child)),
            host_client: Mutex::new(host_client),
            pipeline_client: Mutex::new(pipeline_client),
            meta: plugins,
            priority,
        });

        self.hosts
            .write()
            .await
            .insert(host_id.to_string(), managed_host.clone());

        Ok(managed_host)
    }

    /// Spawns a plugin sub-process based on a `plugin.toml` manifest file.
    pub async fn spawn_from_manifest(
        &self,
        manifest_path: impl AsRef<Path>,
        executable_override: Option<&Path>,
    ) -> Result<Arc<ManagedHost>, SupervisorError> {
        let manifest = PluginManifest::load_from_file(manifest_path.as_ref())
            .map_err(|e| SupervisorError::Manifest(e.to_string()))?;

        let host_id = manifest.plugin.id.replace('.', "_");
        let exec_path = match executable_override {
            Some(path) => path.to_path_buf(),
            None => {
                let parent = manifest_path.as_ref().parent().unwrap_or_else(|| Path::new("."));
                parent.join(&manifest.plugin.entrypoint)
            }
        };

        let priority = manifest.plugin.priority.unwrap_or(500);
        self.spawn_plugin_with_priority(&host_id, exec_path, &[], priority).await
    }

    /// Directly registers an externally created or mocked `ManagedHost` (useful for unit tests).
    pub async fn register_managed_host(&self, host: Arc<ManagedHost>) {
        self.hosts
            .write()
            .await
            .insert(host.host_id.clone(), host);
    }

    /// Retrieves an active managed host by its identifier.
    pub async fn get_host(&self, host_id: &str) -> Option<Arc<ManagedHost>> {
        self.hosts.read().await.get(host_id).cloned()
    }

    /// Returns a list of all currently active managed plugin hosts.
    pub async fn get_all_hosts(&self) -> Vec<Arc<ManagedHost>> {
        self.hosts.read().await.values().cloned().collect()
    }

    /// Stops a specific managed host and cleans up its socket.
    pub async fn stop_host(&self, host_id: &str) -> Result<(), SupervisorError> {
        let host = {
            let mut hosts = self.hosts.write().await;
            hosts.remove(host_id)
        };

        if let Some(host) = host {
            let mut child_guard = host.child.lock().await;
            if let Some(mut child) = child_guard.take() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                tracing::info!(host_id = %host_id, "Host process terminated");
            }
            if host.socket_path.exists() {
                let _ = std::fs::remove_file(&host.socket_path);
                tracing::debug!(socket = %host.socket_path.display(), "Removed host socket");
            }
            Ok(())
        } else {
            Err(SupervisorError::HostNotFound(host_id.to_string()))
        }
    }

    /// Stops all managed host processes and cleans up runtime sockets.
    pub async fn stop_all(&self) -> Result<(), SupervisorError> {
        let host_ids: Vec<String> = self.hosts.read().await.keys().cloned().collect();
        for id in host_ids {
            let _ = self.stop_host(&id).await;
        }
        Ok(())
    }

    /// Polls the designated socket until the host server is ready to accept connections.
    async fn wait_for_readiness(
        &self,
        child: &mut Child,
        host_id: &str,
        socket_path: &Path,
        timeout: Duration,
    ) -> Result<Channel, SupervisorError> {
        let start = std::time::Instant::now();
        let interval = Duration::from_millis(50);

        while start.elapsed() < timeout {
            // First verify that the child process has not exited unexpectedly.
            if let Ok(Some(status)) = child.try_wait() {
                return Err(SupervisorError::PrematureExit {
                    host_id: host_id.to_string(),
                    status: status.to_string(),
                });
            }

            // Attempt to establish a test connection to the host socket.
            if socket_path.exists() {
                match connect_ipc(socket_path).await {
                    Ok(channel) => {
                        tracing::debug!(
                            host_id = %host_id,
                            elapsed_ms = start.elapsed().as_millis(),
                            "Socket ready and connection established"
                        );
                        return Ok(channel);
                    }
                    Err(_) => {
                        // Socket file exists but listener is not yet ready to accept connections.
                    }
                }
            }

            tokio::time::sleep(interval).await;
        }

        Err(SupervisorError::Timeout(host_id.to_string()))
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // As a safeguard against leftover socket files on unexpected drops:
        if self.core_sock_path.exists() {
            let _ = std::fs::remove_file(&self.core_sock_path);
        }
    }
}
