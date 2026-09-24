//! Standalone Kanon node: microkernel engine plus management gateway.
//!
//! This binary is the composition root for a headless deployment. It starts the core IPC server
//! (`core.sock`), the pipeline worker, the process supervisor and the Axum management gateway in
//! one process, wiring the observability hub into both the `tracing` pipeline and the lifecycle
//! trace bus.
//!
//! # Environment
//! - `KANON_API_ADDR` — management gateway bind address (default `127.0.0.1:8080`).
//! - `KANON_LLM_BASE_URL` — model provider base URL; when unset, chat debugging is disabled and
//!   `/api/v1/chat/completions` answers `503` instead of inventing a fake provider.
//! - `KANON_LLM_API_KEY` — provider credential.
//! - `KANON_LLM_MODEL` — default model identifier.
//! - `KANON_LLM_PROTOCOL` — `openai` (default), `openai_responses` or `anthropic`.
//! - `RUST_LOG` — standard `tracing` filter directive.

use std::net::SocketAddr;
use std::sync::Arc;

use kanon_api::{ApiServer, ApiState, Observability, default_agent_config};
use kanon_core::ipc::{CoreApiService, CoreIpcServer, DEFAULT_INGEST_QUEUE_CAPACITY};
use kanon_core::pipeline::PipelineEngine;
use kanon_core::supervisor::Supervisor;
use kanon_llm::{
    AnthropicMessagesProvider, LlmProvider, OpenAiChatProvider, OpenAiResponsesProvider,
};
use tokio::sync::{mpsc, oneshot};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Default management gateway bind address (loopback only, never exposed by accident).
const DEFAULT_API_ADDR: &str = "127.0.0.1:8080";

/// Capacity of the outbound reply channel feeding platform adapters.
const OUTBOUND_QUEUE_CAPACITY: usize = 1024;

/// Fallible startup result type shared by the binary entrypoint helpers.
type StartupResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Configured model provider and the model identifier it should default to.
type ProviderSetup = (Arc<dyn LlmProvider>, String);

#[tokio::main]
async fn main() -> StartupResult<()> {
    let observability = Arc::new(Observability::new());
    init_tracing(observability.clone());

    let api_addr = resolve_api_addr()?;

    // --- Core microkernel -------------------------------------------------------------
    let (event_tx, event_rx) = mpsc::channel(DEFAULT_INGEST_QUEUE_CAPACITY);
    let service = CoreApiService::new(event_tx);
    let ipc_server = CoreIpcServer::with_default_path(service);

    let supervisor = Arc::new(Supervisor::new(
        None,
        Some(ipc_server.socket_path().to_path_buf()),
    ));

    // Outbound replies are drained by a task until a platform adapter subscribes; dropping them
    // silently would hide pipeline bugs, so each drop is logged with its routing key.
    let (outbound_tx, mut outbound_rx) = mpsc::channel(OUTBOUND_QUEUE_CAPACITY);
    let engine = Arc::new(
        PipelineEngine::new(supervisor.clone(), Some(outbound_tx))
            .with_observer(observability.events.clone()),
    );
    let pipeline_worker = engine.start_worker(event_rx);
    let outbound_drain = tokio::spawn(async move {
        while let Some(request) = outbound_rx.recv().await {
            tracing::warn!(
                platform = %request.platform,
                channel_id = %request.channel_id,
                segments = request.segments.len(),
                "No platform adapter registered; outbound message dropped"
            );
        }
    });

    // --- Management gateway -----------------------------------------------------------
    let mut builder =
        ApiState::builder(supervisor.clone()).with_observability(observability.clone());
    if let Some((provider, model)) = provider_from_env()? {
        tracing::info!(model = %model, "LLM provider configured for sandbox chat");
        builder = builder.with_llm_provider("kanon-core", provider, default_agent_config(model));
    } else {
        tracing::warn!(
            "KANON_LLM_BASE_URL is unset; chat completions are disabled on the management gateway"
        );
    }
    let state = builder.build();

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
    outbound_drain.abort();
    supervisor.stop_all().await?;

    tracing::info!("Kanon node shut down gracefully");
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

/// Builds a model provider from environment configuration.
///
/// Returns `Ok(None)` when no provider is configured; misconfiguration (unknown protocol) is an
/// explicit startup error rather than a silently disabled feature.
fn provider_from_env() -> StartupResult<Option<ProviderSetup>> {
    let Ok(base_url) = std::env::var("KANON_LLM_BASE_URL") else {
        return Ok(None);
    };

    let model = std::env::var("KANON_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let api_key = std::env::var("KANON_LLM_API_KEY").ok();
    let protocol = std::env::var("KANON_LLM_PROTOCOL").unwrap_or_else(|_| "openai".to_string());

    let provider: Arc<dyn LlmProvider> = match protocol.as_str() {
        "openai" | "openai_chat" => {
            Arc::new(OpenAiChatProvider::new(base_url, api_key, model.clone()))
        }
        "openai_responses" => Arc::new(
            OpenAiResponsesProvider::new(api_key.unwrap_or_default()).with_base_url(base_url),
        ),
        "anthropic" => Arc::new(AnthropicMessagesProvider::new(
            base_url,
            api_key,
            model.clone(),
        )),
        other => {
            return Err(format!(
                "Unsupported KANON_LLM_PROTOCOL '{other}'; expected openai, openai_responses or anthropic"
            )
            .into());
        }
    };

    Ok(Some((provider, model)))
}
