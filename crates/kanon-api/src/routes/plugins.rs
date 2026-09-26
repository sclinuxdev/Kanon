//! Plugin lifecycle and configuration control routes.
//!
//! # Design notes
//! - The **running process** is authoritative for plugin metadata: `GetPluginMeta` results are
//!   cached on [`kanon_core::ManagedHost`] at handshake time.
//! - The **static manifest** is authoritative for declaration-time facts the process does not
//!   report over gRPC, namely the `[config_schema]` JSON Schema used to render console forms.
//! - Configuration updates are committed in the order *validate → hot reload → persist*: the
//!   plugin host judges the payload first, and only an accepted configuration is written to
//!   disk. A rejected reload therefore leaves both the running plugin and the persisted file
//!   untouched.

use std::path::Path;

use axum::Json;
use axum::Router;
use axum::extract::{FromRequest, Path as AxumPath, State};
use axum::routing::{get, post};
use kanon_core::{ManagedHost, PluginManifest, PluginScanner, SupervisorError};
use kanon_llm::tool_router::{json_to_prost_struct, prost_struct_to_json};
use kanon_proto::v1::PluginMeta;
use kanon_storage::PluginId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::observability::TraceEvent;
use crate::plugin_config::PluginConfigStore;
use crate::state::ApiState;

/// Registers all plugin management routes.
pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/plugins", get(list_plugins))
        .route("/api/v1/plugins/install", post(install_plugin))
        .route(
            "/api/v1/plugins/:id/config",
            get(get_config).put(put_config),
        )
        .route("/api/v1/plugins/:id/restart", post(restart_plugin))
        .route(
            "/api/v1/plugins/:id/tools/:tool_name",
            post(call_plugin_tool),
        )
}

/// Request body for installing a plugin from a local directory path.
#[derive(Debug, Deserialize)]
pub struct InstallPathRequest {
    /// Filesystem path to the local plugin directory.
    pub path: String,
}

/// Standard response payload returned after a plugin installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPluginResponse {
    /// Unique plugin identifier.
    pub plugin_id: String,
    /// Human-readable plugin name.
    pub name: String,
    /// Semantic version of the installed plugin.
    pub version: String,
    /// Runtime platform declared by the plugin.
    pub runtime: String,
    /// List of statically declared commands.
    pub commands: Vec<CommandView>,
    /// List of statically declared tools.
    pub tools: Vec<ToolView>,
    /// Lifecycle status after installation (`running` or `RuntimeUnavailable`).
    pub status: String,
    /// Informational or status message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Catalog of supervised hosts and their plugins.
#[derive(Debug, Serialize)]
pub struct PluginCatalog {
    /// Total number of plugin instances across every host.
    pub total: usize,
    /// Supervised host processes.
    pub hosts: Vec<HostView>,
    /// Flat plugin listing with owning host information.
    pub plugins: Vec<PluginView>,
}

/// A supervised plugin host process.
#[derive(Debug, Serialize)]
pub struct HostView {
    /// Host process identifier.
    pub host_id: String,
    /// Lifecycle status of the host process.
    pub status: String,
    /// Runtime language declared by the manifest, when known.
    pub runtime: Option<String>,
    /// IPC endpoint the host listens on.
    pub socket_path: String,
    /// Pipeline scheduling priority.
    pub priority: i32,
    /// Identifiers of the plugins served by this host.
    pub plugin_ids: Vec<String>,
    /// Whether the control plane can restart this host (a launch recipe is recorded).
    pub restartable: bool,
    /// Operating system process ID (PID) of the managed host child process, if running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Plugin instances hosted by this process.
    pub plugins: Vec<PluginView>,
}

/// A plugin instance with its declared capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginView {
    /// Plugin identifier (e.g. `org.kanon.plugin.weather`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Plugin version string.
    pub version: String,
    /// Author attribution.
    pub author: String,
    /// Short description.
    pub description: String,
    /// Owning host process identifier.
    pub host_id: String,
    /// Runtime language of the owning host.
    pub runtime: Option<String>,
    /// Scheduling priority inherited from the manifest.
    pub priority: i32,
    /// Lifecycle status (`running`, `declared`, or `RuntimeUnavailable`).
    pub status: String,
    /// Statically declared commands.
    pub commands: Vec<CommandView>,
    /// Statically declared tools and their parameter schemas.
    pub tools: Vec<ToolView>,
}

/// A command declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandView {
    /// Command trigger word without the leading slash.
    pub name: String,
    /// Help text.
    pub description: String,
    /// Usage syntax.
    pub usage: String,
    /// Dispatch priority.
    pub priority: i32,
}

/// A tool declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolView {
    /// Tool name exposed to the model.
    pub name: String,
    /// Description presented to the model.
    pub description: String,
    /// JSON Schema of the accepted parameters.
    pub parameters: Value,
}

/// Current configuration values plus the declaration schema.
#[derive(Debug, Serialize)]
pub struct PluginConfigView {
    /// Plugin identifier.
    pub plugin_id: String,
    /// Effective configuration values (schema defaults merged with persisted values).
    pub values: Value,
    /// Declared JSON Schema, or `null` when the manifest declares none.
    pub schema: Value,
    /// Whether the values came from a persisted file rather than schema defaults alone.
    pub persisted: bool,
    /// Currently applied configuration version token.
    pub version: u64,
}

/// Request body for `PUT /api/v1/plugins/:id/config`.
#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    /// Complete replacement configuration object.
    pub values: Value,
    /// Optional expected version for optimistic concurrency control (CAS).
    /// If provided and mismatched against in-memory state, yields HTTP 409 Conflict.
    #[serde(default)]
    pub version: Option<u64>,
}

/// Confirmation payload returned after a successful configuration update.
#[derive(Debug, Serialize)]
pub struct UpdateConfigResponse {
    /// Plugin identifier.
    pub plugin_id: String,
    /// Host process that acknowledged the reload.
    pub host_id: String,
    /// Effective configuration values now in force.
    pub values: Value,
    /// Always `true`: the field exists so console clients can assert on the outcome explicitly.
    pub reloaded: bool,
    /// Monotonically incremented configuration version token.
    pub version: u64,
}

/// Confirmation payload returned after a host restart.
#[derive(Debug, Serialize)]
pub struct RestartResponse {
    /// Host process that was restarted.
    pub host_id: String,
    /// Plugins reported by the freshly restarted host.
    pub plugins: Vec<PluginView>,
}

/// Lists every supervised host and plugin with static metadata.
async fn list_plugins(State(state): State<ApiState>) -> Json<PluginCatalog> {
    let hosts = state.supervisor().get_all_hosts().await;

    let mut host_views = Vec::with_capacity(hosts.len());
    let mut plugin_views = Vec::new();

    for host in &hosts {
        let p_views = plugin_views_for(host);
        let pid = host.pid().await;
        host_views.push(host_view(host, pid, p_views.clone()));
        plugin_views.extend(p_views);
    }

    // Include any plugins recorded as unavailable due to missing runtime environments
    let unavailable = state.supervisor().get_unavailable_plugins().await;
    for unavail in unavailable {
        let plugin_view = plugin_view_from_manifest_with_status(&unavail.manifest, &unavail.status);
        let host_id = format!("host_{}", unavail.manifest.plugin.id.replace('.', "_"));
        host_views.push(HostView {
            host_id: host_id.clone(),
            status: unavail.status.clone(),
            runtime: Some(unavail.manifest.plugin.runtime.clone()),
            socket_path: String::new(),
            priority: unavail.manifest.plugin.priority.unwrap_or(500),
            plugin_ids: vec![unavail.manifest.plugin.id.clone()],
            restartable: true,
            pid: None,
            plugins: vec![plugin_view.clone()],
        });
        plugin_views.push(plugin_view);
    }

    // Deterministic ordering keeps console tables stable across polls.
    host_views.sort_by(|a, b| a.host_id.cmp(&b.host_id));
    plugin_views.sort_by(|a, b| a.id.cmp(&b.id));

    Json(PluginCatalog {
        total: plugin_views.len(),
        hosts: host_views,
        plugins: plugin_views,
    })
}

/// Returns the current configuration and declaration schema for a plugin.
async fn get_config(
    State(state): State<ApiState>,
    AxumPath(plugin_id): AxumPath<String>,
) -> Result<Json<PluginConfigView>, ApiError> {
    let host = state
        .supervisor()
        .find_host_for_plugin(&plugin_id)
        .await
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Plugin '{plugin_id}' is not loaded by any active host"
            ))
        })?;

    let schema = host
        .manifest()
        .and_then(|manifest| manifest.config_schema.clone());

    let store = state.config_store();
    let stored = store.load(&plugin_id)?;
    let persisted = stored.as_object().is_some_and(|map| !map.is_empty());
    let values = PluginConfigStore::apply_defaults(schema.as_ref(), &stored);
    let version = state.supervisor().config_version(&plugin_id).await;

    Ok(Json(PluginConfigView {
        plugin_id,
        values,
        schema: schema.unwrap_or(Value::Null),
        persisted,
        version,
    }))
}

/// Validates, hot reloads and persists a plugin configuration.
async fn put_config(
    State(state): State<ApiState>,
    AxumPath(plugin_id): AxumPath<String>,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<Json<UpdateConfigResponse>, ApiError> {
    if !body.values.is_object() {
        return Err(ApiError::BadRequest(
            "Field 'values' must be a JSON object".to_string(),
        ));
    }

    let host = state
        .supervisor()
        .find_host_for_plugin(&plugin_id)
        .await
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Plugin '{plugin_id}' is not loaded by any active host"
            ))
        })?;

    let schema = host
        .manifest()
        .and_then(|manifest| manifest.config_schema.clone());

    PluginConfigStore::validate(schema.as_ref(), &body.values)
        .map_err(|reason| ApiError::BadRequest(format!("Invalid configuration: {reason}")))?;

    // Step 1: let the plugin host accept or reject the payload before anything becomes durable.
    // Optimistic concurrency control (CAS) verifies the version vector and rejects stale reloads.
    let applied_version = state
        .supervisor()
        .reload_plugin_config_cas(&plugin_id, &body.values, body.version)
        .await?;

    // Step 2: persist only an accepted configuration. `spawn_blocking` keeps small filesystem
    // writes off the async worker threads shared with request handling.
    let store = state.config_store().clone();
    let values = body.values.clone();
    let persist_id = plugin_id.clone();
    tokio::task::spawn_blocking(move || store.store(&persist_id, &values))
        .await
        .map_err(|err| ApiError::Internal(format!("Persistence task failed: {err}")))??;

    state
        .observability()
        .events
        .publish(TraceEvent::PluginConfigUpdated {
            plugin_id: plugin_id.clone(),
            host_id: host.host_id.clone(),
        });

    tracing::info!(
        plugin_id = %plugin_id,
        host_id = %host.host_id,
        version = applied_version,
        "Plugin configuration updated and hot reloaded"
    );

    Ok(Json(UpdateConfigResponse {
        plugin_id,
        host_id: host.host_id.clone(),
        values: body.values,
        reloaded: true,
        version: applied_version,
    }))
}

/// Restarts the host process that owns a plugin.
async fn restart_plugin(
    State(state): State<ApiState>,
    AxumPath(plugin_id): AxumPath<String>,
) -> Result<Json<RestartResponse>, ApiError> {
    let host = state
        .supervisor()
        .find_host_for_plugin(&plugin_id)
        .await
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Plugin '{plugin_id}' is not loaded by any active host"
            ))
        })?;

    let host_id = host.host_id.clone();
    let restarted = state.supervisor().restart_host(&host_id).await?;

    state
        .observability()
        .events
        .publish(TraceEvent::PluginRestarted {
            host_id: host_id.clone(),
        });

    tracing::info!(host_id = %host_id, plugin_id = %plugin_id, "Plugin host restarted by control plane");

    Ok(Json(RestartResponse {
        host_id,
        plugins: plugin_views_for(&restarted),
    }))
}

/// Request body for invoking a plugin tool.
#[derive(Debug, Deserialize, Default)]
pub struct CallPluginToolRequest {
    /// Arguments payload passed to the tool.
    #[serde(default)]
    pub arguments: Value,
}

/// Response returned after executing a plugin tool.
#[derive(Debug, Serialize)]
pub struct CallPluginToolResponse {
    /// Whether the tool execution reported success.
    pub success: bool,
    /// Tool result payload.
    pub result: Value,
    /// Error message when tool execution failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Invokes a declared tool on a plugin over gRPC IPC.
async fn call_plugin_tool(
    State(state): State<ApiState>,
    AxumPath((plugin_id, tool_name)): AxumPath<(String, String)>,
    Json(body): Json<CallPluginToolRequest>,
) -> Result<Json<CallPluginToolResponse>, ApiError> {
    let host = state
        .supervisor()
        .find_host_for_plugin(&plugin_id)
        .await
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Plugin '{plugin_id}' is not loaded by any active host"
            ))
        })?;

    let args_struct = json_to_prost_struct(&body.arguments).unwrap_or_default();
    let call_id = format!(
        "call-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    let req = kanon_proto::v1::ToolCallRequest {
        call_id,
        tool_name,
        session_id: "api-tool-call".to_string(),
        payload: Some(kanon_proto::v1::tool_call_request::Payload::StructuredArgs(
            args_struct,
        )),
    };

    let response = host
        .on_call_tool(req)
        .await
        .map_err(|status| ApiError::Internal(format!("Tool execution failed: {}", status.message())))?;

    let result = match response.payload {
        Some(kanon_proto::v1::tool_call_response::Payload::StructuredResult(res)) => {
            prost_struct_to_json(res)
        }
        _ => Value::Null,
    };

    Ok(Json(CallPluginToolResponse {
        success: response.success,
        result,
        error: if response.error_message.is_empty() {
            None
        } else {
            Some(response.error_message)
        },
    }))
}

/// Handles plugin installation from a local directory path or uploaded distribution archive.
async fn install_plugin(
    State(state): State<ApiState>,
    req: axum::extract::Request,
) -> Result<Json<InstallPluginResponse>, ApiError> {
    let content_type = req
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("multipart/form-data") {
        let mut multipart = axum::extract::Multipart::from_request(req, &state)
            .await
            .map_err(|err| ApiError::BadRequest(format!("Invalid multipart payload: {err}")))?;

        let mut archive_bytes: Option<Vec<u8>> = None;
        let mut path_str: Option<String> = None;

        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?
        {
            let name = field.name().unwrap_or("").to_string();
            let file_name = field.file_name().map(ToString::to_string);

            if name == "path" {
                let text = field.text().await.map_err(|e| ApiError::BadRequest(e.to_string()))?;
                if !text.trim().is_empty() {
                    path_str = Some(text.trim().to_string());
                }
            } else if name == "file"
                || file_name
                    .as_ref()
                    .is_some_and(|f| f.ends_with(".kpk") || f.ends_with(".zip"))
            {
                let bytes = field.bytes().await.map_err(|e| ApiError::BadRequest(e.to_string()))?;
                archive_bytes = Some(bytes.to_vec());
            }
        }

        if let Some(bytes) = archive_bytes {
            let response = install_from_archive(&state, &bytes).await?;
            Ok(Json(response))
        } else if let Some(path) = path_str {
            let response = install_from_path(&state, Path::new(&path)).await?;
            Ok(Json(response))
        } else {
            Err(ApiError::BadRequest(
                "Multipart form must contain either 'file' (.kpk/.zip) or 'path'".to_string(),
            ))
        }
    } else if content_type.is_empty() || content_type.contains("application/json") {
        let bytes = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
            .await
            .map_err(|e| ApiError::BadRequest(format!("Failed to read request body: {e}")))?;
        let payload: InstallPathRequest = serde_json::from_slice(&bytes)
            .map_err(|e| ApiError::BadRequest(format!("Invalid JSON request body: {e}")))?;
        let response = install_from_path(&state, Path::new(&payload.path)).await?;
        Ok(Json(response))
    } else {
        Err(ApiError::BadRequest(format!(
            "Unsupported Content-Type: '{content_type}'. Expected application/json or multipart/form-data"
        )))
    }
}

/// Installs a plugin from a local filesystem path.
async fn install_from_path(
    state: &ApiState,
    source_path: &Path,
) -> Result<InstallPluginResponse, ApiError> {
    if !source_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "Source path does not exist: {}",
            source_path.display()
        )));
    }

    let manifest_file = if source_path.is_file()
        && source_path
            .file_name()
            .is_some_and(|n| n == "plugin.toml")
    {
        source_path.to_path_buf()
    } else if source_path.is_dir() {
        let candidate = source_path.join("plugin.toml");
        if candidate.is_file() {
            candidate
        } else if let Some(found) = PluginScanner::find_manifest_in_dir(source_path) {
            found
        } else {
            return Err(ApiError::BadRequest(format!(
                "No plugin.toml found in directory: {}",
                source_path.display()
            )));
        }
    } else {
        return Err(ApiError::BadRequest(format!(
            "Source path is neither a directory nor a plugin.toml file: {}",
            source_path.display()
        )));
    };

    let manifest = PluginManifest::load_from_file(&manifest_file)
        .map_err(|e| ApiError::BadRequest(format!("Invalid plugin manifest: {e}")))?;

    PluginId::validate(&manifest.plugin.id)
        .map_err(|e| ApiError::BadRequest(format!("Invalid plugin identifier: {e}")))?;

    let plugin_source_dir = manifest_file.parent().unwrap_or(source_path);

    // Target installation directory: <plugins_root>/<plugin_id>
    let plugins_root = state.plugins_dir().to_path_buf();
    std::fs::create_dir_all(&plugins_root).map_err(|e| ApiError::Internal(e.to_string()))?;
    let dest_dir = plugins_root.join(&manifest.plugin.id);

    let is_same = if let (Ok(c1), Ok(c2)) = (plugin_source_dir.canonicalize(), dest_dir.canonicalize()) {
        c1 == c2
    } else {
        false
    };

    let host_id = manifest.plugin.id.replace('.', "_");
    // Terminate existing host if running
    let _ = state.supervisor().stop_host(&host_id).await;

    if !is_same {
        if dest_dir.exists() {
            let _ = std::fs::remove_dir_all(&dest_dir);
        }
        copy_dir_recursive(plugin_source_dir, &dest_dir).map_err(|e| {
            ApiError::Internal(format!("Failed to copy plugin to target directory: {e}"))
        })?;
    }

    ensure_entrypoint_executable(&dest_dir, &manifest.plugin.entrypoint);

    let target_manifest_path = dest_dir.join("plugin.toml");
    let spawn_result = state
        .supervisor()
        .spawn_from_manifest(&target_manifest_path, None)
        .await;

    match spawn_result {
        Ok(host) => {
            state.observability().events.publish(TraceEvent::PluginInstalled {
                plugin_id: manifest.plugin.id.clone(),
                host_id: host.host_id.clone(),
            });

            let p_views = plugin_views_for(&host);
            let (commands, tools) = p_views
                .into_iter()
                .find(|p| p.id == manifest.plugin.id)
                .map(|p| (p.commands, p.tools))
                .unwrap_or_else(|| {
                    let declared = plugin_view_from_manifest_with_status(&manifest, "running");
                    (declared.commands, declared.tools)
                });

            tracing::info!(
                plugin_id = %manifest.plugin.id,
                host_id = %host.host_id,
                "Plugin installed and host spawned successfully"
            );

            Ok(InstallPluginResponse {
                plugin_id: manifest.plugin.id.clone(),
                name: manifest.plugin.name.clone(),
                version: manifest.plugin.version.clone(),
                runtime: manifest.plugin.runtime.clone(),
                commands,
                tools,
                status: "running".to_string(),
                message: Some("Plugin installed and launched successfully".to_string()),
            })
        }
        Err(SupervisorError::RuntimeUnavailable { runtime, reason }) => {
            tracing::warn!(
                plugin_id = %manifest.plugin.id,
                runtime = %runtime,
                reason = %reason,
                "Plugin installed but runtime is unavailable"
            );

            let declared = plugin_view_from_manifest_with_status(&manifest, "RuntimeUnavailable");
            Ok(InstallPluginResponse {
                plugin_id: manifest.plugin.id.clone(),
                name: manifest.plugin.name.clone(),
                version: manifest.plugin.version.clone(),
                runtime,
                commands: declared.commands,
                tools: declared.tools,
                status: "RuntimeUnavailable".to_string(),
                message: Some(format!("Plugin installed but runtime is unavailable: {reason}")),
            })
        }
        Err(err) => Err(ApiError::BadRequest(format!(
            "Failed to launch plugin host: {err}"
        ))),
    }
}

/// Installs a plugin from raw archive bytes (.kpk or .zip).
async fn install_from_archive(
    state: &ApiState,
    archive_bytes: &[u8],
) -> Result<InstallPluginResponse, ApiError> {
    if archive_bytes.len() < 22 {
        return Err(ApiError::BadRequest(
            "Uploaded archive file is too small or corrupt".to_string(),
        ));
    }

    let temp_dir = tempfile::tempdir().map_err(|e| ApiError::Internal(e.to_string()))?;
    let cursor = std::io::Cursor::new(archive_bytes);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| ApiError::BadRequest(format!("Failed to parse ZIP archive: {e}")))?;

    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;

        let enclosed = match file.enclosed_name() {
            Some(p) => p.to_owned(),
            None => {
                return Err(ApiError::BadRequest(
                    "Archive contains forbidden or malicious relative path".to_string(),
                ));
            }
        };

        let out_path = temp_dir.path().join(&enclosed);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| ApiError::Internal(e.to_string()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| ApiError::Internal(e.to_string()))?;
            }
            let mut outfile =
                std::fs::File::create(&out_path).map_err(|e| ApiError::Internal(e.to_string()))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| ApiError::Internal(e.to_string()))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
                }
            }
        }
    }

    let source_dir = if temp_dir.path().join("plugin.toml").is_file() {
        temp_dir.path().to_path_buf()
    } else if let Some(manifest_path) = PluginScanner::find_manifest_in_dir(temp_dir.path()) {
        manifest_path.parent().unwrap_or(temp_dir.path()).to_path_buf()
    } else {
        return Err(ApiError::BadRequest(
            "Uploaded archive does not contain a valid plugin.toml file".to_string(),
        ));
    };

    install_from_path(state, &source_dir).await
}

/// Recursively copies directory contents, skipping transient development and cache directories.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "target"
            || name_str == ".venv"
            || name_str == "venv"
            || name_str == "__pycache__"
        {
            continue;
        }

        let src_child = entry.path();
        let dst_child = dst.join(file_name);

        if file_type.is_dir() {
            copy_dir_recursive(&src_child, &dst_child)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_child, &dst_child)?;
        }
    }
    Ok(())
}

/// Sets Unix executable permission bits (0o755) on the declared entrypoint binary if present.
fn ensure_entrypoint_executable(plugin_dir: &Path, entrypoint: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec_path = plugin_dir.join(entrypoint);
        if exec_path.is_file() {
            if let Ok(metadata) = std::fs::metadata(&exec_path) {
                let mut permissions = metadata.permissions();
                let mode = permissions.mode();
                permissions.set_mode(mode | 0o755);
                let _ = std::fs::set_permissions(&exec_path, permissions);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (plugin_dir, entrypoint);
    }
}

/// Builds the host view for a supervised process.
fn host_view(host: &ManagedHost, pid: Option<u32>, plugins: Vec<PluginView>) -> HostView {
    HostView {
        host_id: host.host_id.clone(),
        status: "running".to_string(),
        runtime: host
            .manifest()
            .map(|manifest| manifest.plugin.runtime.clone()),
        socket_path: host.socket_path.to_string_lossy().to_string(),
        priority: host.priority,
        plugin_ids: host.meta.iter().map(|meta| meta.id.clone()).collect(),
        restartable: host.launch_spec().is_some(),
        pid,
        plugins,
    }
}

/// Builds plugin views for a host, merging live metadata with the static manifest.
fn plugin_views_for(host: &ManagedHost) -> Vec<PluginView> {
    let manifest = host.manifest();

    let mut views: Vec<PluginView> = host
        .meta
        .iter()
        .map(|meta| plugin_view_from_meta(meta, host, manifest))
        .collect();

    // A host that declares a manifest but has not reported its plugin (or reports it later)
    // still appears in the catalog as `declared`, so consoles never hide a configured plugin.
    if let Some(manifest) = manifest
        && !views.iter().any(|view| view.id == manifest.plugin.id)
    {
        views.push(plugin_view_from_manifest(host, manifest));
    }

    views
}

/// Builds a plugin view from live process metadata.
fn plugin_view_from_meta(
    meta: &PluginMeta,
    host: &ManagedHost,
    manifest: Option<&kanon_core::PluginManifest>,
) -> PluginView {
    let fallback = manifest.filter(|m| m.plugin.id == meta.id);

    PluginView {
        id: meta.id.clone(),
        name: non_empty_or(meta.name.clone(), || {
            fallback.map(|m| m.plugin.name.clone()).unwrap_or_default()
        }),
        version: non_empty_or(meta.version.clone(), || {
            fallback
                .map(|m| m.plugin.version.clone())
                .unwrap_or_default()
        }),
        author: non_empty_or(meta.author.clone(), || {
            fallback
                .and_then(|m| m.plugin.author.clone())
                .unwrap_or_default()
        }),
        description: non_empty_or(meta.description.clone(), || {
            fallback
                .and_then(|m| m.plugin.description.clone())
                .unwrap_or_default()
        }),
        host_id: host.host_id.clone(),
        runtime: fallback.map(|m| m.plugin.runtime.clone()),
        priority: host.priority,
        status: "running".to_string(),
        commands: meta
            .commands
            .iter()
            .map(|command| CommandView {
                name: command.name.clone(),
                description: command.description.clone(),
                usage: command.usage.clone(),
                priority: command.priority,
            })
            .collect(),
        tools: meta
            .tools
            .iter()
            .map(|tool| ToolView {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool
                    .parameters
                    .clone()
                    .map(prost_struct_to_json)
                    .unwrap_or_else(|| json!({ "type": "object" })),
            })
            .collect(),
    }
}

/// Builds a plugin view from the static manifest alone.
fn plugin_view_from_manifest(
    host: &ManagedHost,
    manifest: &kanon_core::PluginManifest,
) -> PluginView {
    PluginView {
        id: manifest.plugin.id.clone(),
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        author: manifest.plugin.author.clone().unwrap_or_default(),
        description: manifest.plugin.description.clone().unwrap_or_default(),
        host_id: host.host_id.clone(),
        runtime: Some(manifest.plugin.runtime.clone()),
        priority: host.priority,
        status: "declared".to_string(),
        commands: manifest
            .commands
            .iter()
            .map(|command| CommandView {
                name: command.name.clone(),
                description: command.description.clone().unwrap_or_default(),
                usage: command.usage.clone().unwrap_or_default(),
                priority: command.priority.unwrap_or(500),
            })
            .collect(),
        tools: manifest
            .tools
            .iter()
            .map(|tool| ToolView {
                name: tool.name.clone(),
                description: tool.description.clone().unwrap_or_default(),
                parameters: tool
                    .parameters
                    .clone()
                    .unwrap_or_else(|| json!({ "type": "object" })),
            })
            .collect(),
    }
}

/// Builds a plugin view from a manifest with an explicit lifecycle status.
fn plugin_view_from_manifest_with_status(
    manifest: &kanon_core::PluginManifest,
    status: &str,
) -> PluginView {
    let host_id = format!("host_{}", manifest.plugin.id.replace('.', "_"));
    PluginView {
        id: manifest.plugin.id.clone(),
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        author: manifest.plugin.author.clone().unwrap_or_default(),
        description: manifest.plugin.description.clone().unwrap_or_default(),
        host_id,
        runtime: Some(manifest.plugin.runtime.clone()),
        priority: manifest.plugin.priority.unwrap_or(500),
        status: status.to_string(),
        commands: manifest
            .commands
            .iter()
            .map(|command| CommandView {
                name: command.name.clone(),
                description: command.description.clone().unwrap_or_default(),
                usage: command.usage.clone().unwrap_or_default(),
                priority: command.priority.unwrap_or(500),
            })
            .collect(),
        tools: manifest
            .tools
            .iter()
            .map(|tool| ToolView {
                name: tool.name.clone(),
                description: tool.description.clone().unwrap_or_default(),
                parameters: tool
                    .parameters
                    .clone()
                    .unwrap_or_else(|| json!({ "type": "object" })),
            })
            .collect(),
    }
}

/// Returns `value` when non-empty, otherwise the fallback.
fn non_empty_or(value: String, fallback: impl FnOnce() -> String) -> String {
    if value.is_empty() { fallback() } else { value }
}
