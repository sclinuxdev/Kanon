//! Shutdown signals shared by the node binaries.
//!
//! SIGTERM matters as much as Ctrl-C: service managers (`systemctl stop`), container runtimes
//! (`docker stop`) and a plain `kill` all send it. A node that dies without running its shutdown
//! path never stops its plugin hosts, and those hosts keep their platform connections open — the
//! fresh node then spawns *new* hosts, so two processes serve every message and the user sees
//! duplicate replies while nothing looks broken in the console.

/// Completes when the process is asked to stop, by SIGINT or (on Unix) SIGTERM.
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(err) => {
                // Without the handler the process still dies on SIGTERM, but silently: the hosts
                // would be orphaned again. Say so instead of pretending shutdown is covered.
                tracing::error!(error = %err, "Failed to install the SIGTERM handler");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("SIGINT received; draining background tasks");
            }
            _ = sigterm.recv() => {
                tracing::info!("SIGTERM received; draining background tasks");
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Shutdown signal received; draining background tasks");
    }
}
