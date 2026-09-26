//! Kanon Core microkernel server entrypoint.

use std::sync::Arc;
use kanon_core::ipc::{CoreApiService, CoreIpcServer, DEFAULT_INGEST_QUEUE_CAPACITY};
use kanon_core::pipeline::PipelineEngine;
use kanon_core::supervisor::Supervisor;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    tracing::info!("Starting Kanon Core Microkernel Engine...");

    let (event_tx, event_rx) = mpsc::channel(DEFAULT_INGEST_QUEUE_CAPACITY);
    let socket_path = kanon_transport::core_socket_path(None);

    // Initialize process supervisor and pipeline engine.
    let supervisor = Arc::new(Supervisor::new(None, Some(socket_path.clone())));
    let agent_slot = Arc::new(kanon_llm::AgentSlot::new());
    let engine_builder =
        PipelineEngine::new(supervisor.clone()).with_agent_slot(agent_slot.clone());

    match kanon_llm::provider_from_env() {
        Ok(Some((provider, model))) => {
            tracing::info!(model = %model, "LLM provider configured for core pipeline");
            let memory = Arc::new(kanon_llm::SlidingWindowMemory::new(40));
            let agent = Arc::new(
                kanon_llm::Agent::builder("kanon-core", provider)
                    .memory(memory)
                    .model(model)
                    .build(),
            );
            agent_slot.set(Some(agent));
        }
        Ok(None) => {
            tracing::info!(
                "KANON_LLM_BASE_URL is unset; conversational LLM routing is disabled"
            );
        }
        Err(e) => {
            return Err(format!("Failed to initialize LLM provider: {e}").into());
        }
    }

    let engine = Arc::new(engine_builder);
    let worker_handle = engine.clone().start_worker(event_rx);

    // Outbound replies are routed through the platform adapter registry. A bare microkernel has
    // no adapters registered, so deliveries fail explicitly with `UnknownPlatform` rather than
    // disappearing silently.
    let dispatcher_handle = engine.clone().start_outbound_dispatcher();

    let service = CoreApiService::new(event_tx)
        .with_supervisor(supervisor.clone())
        .with_outbound_sender(engine.outbound_sender())
        .with_agent_slot(agent_slot.clone());
    let server = CoreIpcServer::new(socket_path, service);

    server
        .run(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    worker_handle.abort();
    if let Some(handle) = dispatcher_handle {
        handle.abort();
    }
    let _ = supervisor.stop_all().await;

    tracing::info!("Kanon Core Engine shut down gracefully");
    Ok(())
}

