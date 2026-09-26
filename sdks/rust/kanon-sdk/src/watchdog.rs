//! Core-liveness watchdog: notices that the core a host registered with has exited.
//!
//! # Why this exists
//! A plugin host is a child of the core, but process parentage is not a liveness guarantee: a core
//! killed with `SIGKILL`, killed from a debugger, or whose parent terminal disappears leaves its
//! hosts running. Such an orphan keeps serving its platform (a QQ gateway socket, a Telegram
//! long-poller) even though nothing can answer — and once a *new* core starts, the orphan and the
//! freshly spawned host both serve the same platform, so every inbound message is handled twice:
//! two replies, two model calls, two bills, with nothing visibly broken in the console.
//!
//! The watchdog probes `BotApiService.Ping` on the core. Any answer proves the core is alive;
//! after [`CoreWatchdogConfig::max_failures`] consecutive failures the host is told to stop, which
//! unloads the plugin and closes its platform connection. Exiting is the correct outcome even
//! during a legitimate core restart: the new core spawns its own hosts from the plugin directory.

use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::context::CoreHandle;

/// Watchdog tuning.
#[derive(Debug, Clone, Copy)]
pub struct CoreWatchdogConfig {
    /// Delay between probes.
    pub interval: Duration,
    /// Per-probe deadline.
    pub timeout: Duration,
    /// Consecutive failed probes tolerated before the core is declared lost.
    ///
    /// Three failures at the default cadence mean roughly 45 seconds of silence, which a busy core
    /// never produces but a restart or a shutdown does.
    pub max_failures: u32,
}

impl Default for CoreWatchdogConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(15),
            timeout: Duration::from_secs(5),
            max_failures: 3,
        }
    }
}

/// Reason a host decided to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The embedding process asked the host to stop.
    Requested,
    /// The core stopped answering liveness probes.
    CoreLost,
}

/// Watches the core and resolves once it is considered lost.
///
/// Returned as a future so a host can `select!` it against its own shutdown signal: whichever
/// happens first stops the host, and the [`StopReason`] is reported to the caller for logging.
pub async fn watch_core(
    core: CoreHandle,
    config: CoreWatchdogConfig,
    mut shutdown: mpsc::Receiver<()>,
) -> StopReason {
    let mut consecutive_failures: u32 = 0;

    loop {
        tokio::select! {
            // The host is shutting down for its own reasons; the watchdog is no longer relevant.
            _ = shutdown.recv() => return StopReason::Requested,
            _ = tokio::time::sleep(config.interval) => {}
        }

        match tokio::time::timeout(config.timeout, core.ping()).await {
            Ok(Ok(())) => {
                consecutive_failures = 0;
            }
            Ok(Err(status)) => {
                consecutive_failures += 1;
                warn!(
                    failures = consecutive_failures,
                    max_failures = config.max_failures,
                    error = %status,
                    "Core liveness probe failed"
                );
            }
            Err(_) => {
                consecutive_failures += 1;
                warn!(
                    failures = consecutive_failures,
                    max_failures = config.max_failures,
                    timeout_ms = config.timeout.as_millis() as u64,
                    "Core liveness probe timed out"
                );
            }
        }

        if consecutive_failures >= config.max_failures {
            info!(
                failures = consecutive_failures,
                "Core unreachable; stopping this host so it cannot serve its platform without a core"
            );
            return StopReason::CoreLost;
        }
    }
}
