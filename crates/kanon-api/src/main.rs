//! Standalone Kanon node: microkernel engine plus management gateway.
//!
//! This binary is the composition root for a headless deployment. It starts the core IPC server
//! (`core.sock`), the pipeline worker, the process supervisor and the Axum management gateway in
//! one process, wiring the observability hub into both the `tracing` pipeline and the lifecycle
//! trace bus.
//!
//! # Environment
//! - `KANON_API_ADDR` — management gateway bind address (default `127.0.0.1:8080`).
//! - `KANON_WEBHOOK_PLATFORM` — platform id served by the bundled webhook adapter (default
//!   `webhook`); inbound messages POST to `/api/v1/adapters/<platform>/ingest`.
//! - `KANON_WEBHOOK_CALLBACK_URL` — outbound callback URL; when unset the adapter stays
//!   inbound-only and reports `connected: false` instead of pretending to deliver.
//! - `KANON_LLM_BASE_URL` — model provider base URL; when unset, chat debugging is disabled and
//!   `/api/v1/chat/completions` answers `503` instead of inventing a fake provider.
//! - `KANON_LLM_API_KEY` — provider credential.
//! - `KANON_LLM_MODEL` — default model identifier.
//! - `KANON_LLM_PROTOCOL` — `openai` (default), `openai_responses` or `anthropic`.
//! - `RUST_LOG` — standard `tracing` filter directive.

use std::net::SocketAddr;
use std::sync::Arc;

use kanon_api::{ApiServer, ApiState, Observability, WebhookAdapter, default_agent_config};
use kanon_core::EventIngress;
use kanon_core::ipc::{CoreApiService, CoreIpcServer, DEFAULT_INGEST_QUEUE_CAPACITY};
use kanon_core::pipeline::PipelineEngine;
use kanon_core::supervisor::Supervisor;
use tokio::sync::{mpsc, oneshot};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Default management gateway bind address (loopback only, never exposed by accident).
const DEFAULT_API_ADDR: &str = "127.0.0.1:8080";

/// Default platform identifier served by the bundled webhook adapter.
const DEFAULT_WEBHOOK_PLATFORM: &str = "webhook";

/// Fallible startup result type shared by the binary entrypoint helpers.
type StartupResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main]
async fn main() -> StartupResult<()> {
    let observability = Arc::new(Observability::new());
    init_tracing(observability.clone());

    let api_addr = resolve_api_addr()?;

    // --- Core microkernel & supervisor ------------------------------------------------
    let (event_tx, event_rx) = mpsc::channel(DEFAULT_INGEST_QUEUE_CAPACITY);
    let ingress = EventIngress::new(event_tx);
    let default_ipc = CoreIpcServer::with_default_path(CoreApiService::new(ingress.clone()));
    let socket_path = default_ipc.socket_path().to_path_buf();

    let supervisor = Arc::new(Supervisor::new(
        None,
        Some(socket_path.clone()),
    ));

    // Ensure ./plugins and ./data/plugins directories exist
    if let Err(err) = std::fs::create_dir_all("./plugins") {
        tracing::warn!(error = %err, "Failed to ensure ./plugins directory exists");
    }
    if let Err(err) = std::fs::create_dir_all("./data/plugins") {
        tracing::warn!(error = %err, "Failed to ensure ./data/plugins directory exists");
    }

    // Auto-discover and launch declared plugins from ./plugins directory
    load_plugins_from_directory(&supervisor, "./plugins").await;

    // --- Platform adapters ------------------------------------------------------------
    register_webhook_adapter(&supervisor).await?;
    for (platform, error) in supervisor.adapters().start_all(ingress.clone()).await {
        tracing::error!(platform = %platform, error = %error, "Adapter failed to start");
    }

    // --- Management gateway state & agent engine --------------------------------------
    let mut builder = ApiState::builder(supervisor.clone())
        .with_observability(observability.clone())
        .with_ingress(ingress.clone());
    if let Some((provider, model)) = kanon_llm::provider_from_env()? {
        tracing::info!(model = %model, "LLM provider configured for conversational pipeline and sandbox chat");
        builder = builder.with_llm_provider("kanon-core", provider, default_agent_config(model));
    } else {
        tracing::warn!(
            "KANON_LLM_BASE_URL is unset; chat completions and conversational LLM routing are disabled"
        );
    }
    let state = builder.build();

    // The pipeline never awaits platform I/O: replies are queued and an independent dispatcher
    // resolves the destination platform to a built-in adapter or a plugin host.
    let mut engine_builder = PipelineEngine::new(supervisor.clone())
        .with_observer(observability.events.clone());
    if let Some(agent) = state.agent() {
        let tool_router = Arc::new(kanon_llm::ToolRouter::from_arc(agent.clone()));
        engine_builder = engine_builder.with_tool_router(tool_router);
    }
    let engine = Arc::new(engine_builder);
    let pipeline_worker = engine.clone().start_worker(event_rx);
    let outbound_dispatcher = engine.clone().start_outbound_dispatcher();

    let mut service = CoreApiService::new(ingress.clone())
        .with_supervisor(supervisor.clone())
        .with_outbound_sender(engine.outbound_sender());
    if let Some(agent) = state.agent() {
        let gateway = Arc::new(kanon_llm::LlmGateway::new(
            agent.provider().clone(),
            agent.config().default_model.clone(),
        ));
        service = service.with_gateway(gateway);
    }
    let ipc_server = CoreIpcServer::new(socket_path, service);

    let api_server = ApiServer::bind(api_addr, state).await?;
    let bound_addr = api_server.local_addr();

    // --- Graceful shutdown ------------------------------------------------------------
    let (core_shutdown_tx, core_shutdown_rx) = oneshot::channel();
    let (api_shutdown_tx, api_shutdown_rx) = oneshot::channel();

    let core_task = tokio::spawn(async move {
        ipc_server
            .run(async move {
                let _ = core_shutdown_rx.await;
            })
            .await
    });
    let api_task = tokio::spawn(async move {
        api_server
            .run(async move {
                let _ = api_shutdown_rx.await;
            })
            .await
    });

    tracing::info!(address = %bound_addr, "Kanon node started");

    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received; draining background tasks");

    let _ = core_shutdown_tx.send(());
    let _ = api_shutdown_tx.send(());

    if let Err(err) = core_task.await? {
        tracing::error!(error = %err, "Core IPC server terminated with an error");
    }
    if let Err(err) = api_task.await? {
        tracing::error!(error = %err, "Management gateway terminated with an error");
    }

    pipeline_worker.abort();
    if let Some(handle) = outbound_dispatcher {
        handle.abort();
    }
    for (platform, error) in supervisor.adapters().stop_all().await {
        tracing::warn!(platform = %platform, error = %error, "Adapter failed to stop cleanly");
    }
    supervisor.stop_all().await?;

    tracing::info!("Kanon node shut down gracefully");
    Ok(())
}

/// Registers the bundled webhook adapter from environment configuration.
///
/// The adapter is always registered: inbound ingress works out of the box (a fresh node can be
/// driven by `curl`), while outbound delivery stays explicitly disabled until a callback URL is
/// provided, which the console reports as `connected: false`.
async fn register_webhook_adapter(supervisor: &Arc<Supervisor>) -> StartupResult<()> {
    let platform =
        std::env::var("KANON_WEBHOOK_PLATFORM").unwrap_or_else(|_| DEFAULT_WEBHOOK_PLATFORM.to_string());
    let callback_url = std::env::var("KANON_WEBHOOK_CALLBACK_URL")
        .ok()
        .filter(|url| !url.trim().is_empty());
    let secret = std::env::var("KANON_WEBHOOK_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let mut adapter = WebhookAdapter::new(platform.clone(), None, callback_url.clone())?;
    if let Some(sec) = secret {
        tracing::info!(platform = %platform, "Webhook adapter configured with HMAC-SHA256 signature verification");
        adapter = adapter.with_secret(sec);
    }

    supervisor
        .adapters()
        .register(Arc::new(adapter))
        .await?;

    if callback_url.is_some() {
        tracing::info!(platform = %platform, "Webhook adapter registered with outbound callback");
    } else {
        tracing::info!(
            platform = %platform,
            "Webhook adapter registered inbound-only (KANON_WEBHOOK_CALLBACK_URL is unset)"
        );
    }

    Ok(())
}

/// Installs the `tracing` subscriber with both console output and the log broadcast layer.
///
/// The WebSocket layer is attached to the same registry as the formatter, so `/ws/v1/logs`
/// observes exactly what operators see on stdout — no second, divergent logging path.
fn init_tracing(observability: Arc<Observability>) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(observability.logs.layer())
        .init();
}

/// Resolves the management gateway bind address from the environment.
fn resolve_api_addr() -> StartupResult<SocketAddr> {
    let raw = std::env::var("KANON_API_ADDR").unwrap_or_else(|_| DEFAULT_API_ADDR.to_string());
    raw.parse::<SocketAddr>()
        .map_err(|err| format!("Invalid KANON_API_ADDR '{raw}': {err}").into())
}

/// Discovers plugins in the specified directory and launches their host processes.
///
/// Missing runtime environments (e.g. Python / TypeScript) or individual manifest errors
/// degrade gracefully to ensure the core microkernel and API gateway remain operational.
async fn load_plugins_from_directory(supervisor: &Arc<Supervisor>, dir: impl AsRef<std::path::Path>) {
    let plugins = match kanon_core::PluginScanner::scan(dir.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(dir = %dir.as_ref().display(), error = %e, "Failed to scan plugins directory");
            return;
        }
    };

    tracing::info!(count = plugins.len(), "Discovered plugins during startup scan");

    for discovered in plugins {
        let plugin_id = discovered.manifest.plugin.id.clone();
        let runtime = discovered.manifest.plugin.runtime.clone();
        match supervisor.spawn_from_manifest(&discovered.manifest_path, None).await {
            Ok(host) => {
                tracing::info!(
                    plugin_id = %plugin_id,
                    host_id = %host.host_id,
                    runtime = %runtime,
                    "Plugin host launched and ready"
                );
            }
            Err(kanon_core::SupervisorError::RuntimeUnavailable { runtime, reason }) => {
                tracing::warn!(
                    plugin_id = %plugin_id,
                    runtime = %runtime,
                    reason = %reason,
                    "Plugin runtime is unavailable on this host; marked as RuntimeUnavailable"
                );
            }
            Err(err) => {
                tracing::warn!(
                    plugin_id = %plugin_id,
                    error = %err,
                    "Failed to launch plugin host; skipping gracefully without blocking startup"
                );
            }
        }
    }
}
