//! Kanon Core microkernel server entrypoint.

use kanon_core::ipc::{CoreApiService, CoreIpcServer, DEFAULT_INGEST_QUEUE_CAPACITY};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    tracing::info!("Starting Kanon Core Microkernel Engine...");

    let (event_tx, _event_rx) = mpsc::channel(DEFAULT_INGEST_QUEUE_CAPACITY);
    let service = CoreApiService::new(event_tx);
    let server = CoreIpcServer::with_default_path(service);

    server
        .run(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    tracing::info!("Kanon Core Engine shut down gracefully");
    Ok(())
}

