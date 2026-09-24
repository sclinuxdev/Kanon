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

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use kanon_core::ManagedHost;
use kanon_llm::tool_router::prost_struct_to_json;
use kanon_proto::v1::PluginMeta;
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
        .route(
            "/api/v1/plugins/:id/config",
            get(get_config).put(put_config),
        )
        .route("/api/v1/plugins/:id/restart", post(restart_plugin))
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
    pub status: &'static str,
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
}

/// A plugin instance with its declared capabilities.
#[derive(Debug, Serialize)]
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
    /// `running` when reported by the live process, `declared` when only statically known.
    pub status: &'static str,
    /// Statically declared commands.
    pub commands: Vec<CommandView>,
    /// Statically declared tools and their parameter schemas.
    pub tools: Vec<ToolView>,
}

/// A command declared by a plugin.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
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
        host_views.push(host_view(host));
        plugin_views.extend(plugin_views_for(host));
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
    Path(plugin_id): Path<String>,
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
    Path(plugin_id): Path<String>,
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
    Path(plugin_id): Path<String>,
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

/// Builds the host view for a supervised process.
fn host_view(host: &ManagedHost) -> HostView {
    HostView {
        host_id: host.host_id.clone(),
        status: "running",
        runtime: host
            .manifest()
            .map(|manifest| manifest.plugin.runtime.clone()),
        socket_path: host.socket_path.to_string_lossy().to_string(),
        priority: host.priority,
        plugin_ids: host.meta.iter().map(|meta| meta.id.clone()).collect(),
        restartable: host.launch_spec().is_some(),
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
        status: "running",
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
        status: "declared",
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
