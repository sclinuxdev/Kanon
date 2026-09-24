//! Real-time observability substrate for the management gateway.
//!
//! Two cooperating pieces feed the WebSocket control plane:
//!
//! - [`logs`]: a `tracing` [`Layer`](tracing_subscriber::Layer) that captures every emitted
//!   event as a structured JSON record and broadcasts it to `/ws/v1/logs` subscribers;
//! - [`events`]: an [`EventBus`] carrying message lifecycle transitions to `/ws/v1/events`,
//!   fed both by the core pipeline ([`kanon_core::PipelineObserver`]) and by the LLM agent
//!   ([`kanon_llm::AgentHook`], so reasoning and tool calling stages join the same timeline).
//!
//! Both channels are `tokio::sync::broadcast` based: a slow or stalled console can never
//! block the microkernel. When a subscriber falls behind, the lag is reported to that
//! subscriber instead of silently truncating its view.

pub mod events;
pub mod logs;

use std::sync::Arc;

use crate::metrics::MetricsRegistry;
use events::EventBus;
use logs::LogBroadcaster;

pub use events::{EventBus as TraceEventBus, TraceEvent, TraceRecord};
pub use logs::{LogBroadcaster as ApiLogBroadcaster, LogLevel, LogRecord};

/// Default broadcast buffer depth for log and trace channels.
///
/// Sized so that a console experiencing a multi-second UI stall still recovers without
/// losing records, while memory remains bounded under a log storm.
pub const DEFAULT_BROADCAST_CAPACITY: usize = 1024;

/// Aggregate handle owning every real-time observability channel of the gateway.
#[derive(Clone)]
pub struct Observability {
    /// Shared metrics registry updated by both channels.
    pub metrics: Arc<MetricsRegistry>,
    /// Structured log broadcaster.
    pub logs: Arc<LogBroadcaster>,
    /// Lifecycle trace event bus.
    pub events: Arc<EventBus>,
}

impl Observability {
    /// Creates an observability hub with the default broadcast capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BROADCAST_CAPACITY)
    }

    /// Creates an observability hub with an explicit broadcast buffer depth.
    pub fn with_capacity(capacity: usize) -> Self {
        let metrics = Arc::new(MetricsRegistry::new());
        Self {
            logs: LogBroadcaster::new(capacity, metrics.clone()),
            events: EventBus::new(capacity, metrics.clone()),
            metrics,
        }
    }
}

impl Default for Observability {
    fn default() -> Self {
        Self::new()
    }
}
