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
    let service = CoreApiService::new(event_tx);
    let server = CoreIpcServer::with_default_path(service);

    // Initialize process supervisor and start the central pipeline worker loop.
    let supervisor = Arc::new(Supervisor::new(None, Some(server.socket_path().to_path_buf())));
    let engine = Arc::new(PipelineEngine::new(supervisor.clone(), None));
    let worker_handle = engine.start_worker(event_rx);

    server
        .run(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    worker_handle.abort();
    let _ = supervisor.stop_all().await;

    tracing::info!("Kanon Core Engine shut down gracefully");
    Ok(())
}

