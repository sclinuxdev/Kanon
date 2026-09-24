//! Pipeline lifecycle observation hooks.
//!
//! The engine emits a compact, serializable stage record at every message lifecycle
//! transition (ingest -> pre-filter -> command routing -> LLM reasoning -> outbound).
//! Control planes (e.g. the `kanon-api` WebSocket event bus) subscribe through
//! [`PipelineObserver`] to render end-to-end trace timelines.
//!
//! # Why a synchronous trait
//! Observation is strictly fire-and-forget. A synchronous callback lets an implementation
//! hand the record to a non-blocking broadcast channel without ever introducing an `await`
//! point inside the hot message path, so a slow or disconnected console can never stall
//! inbound processing (the same reasoning that motivates the Fast-ACK ingest queue).

use serde::Serialize;

/// A single message lifecycle transition emitted by the pipeline engine.
///
/// Serialized with an internal `stage` tag so that WebSocket clients can dispatch on the
/// stage name directly, e.g. `{"stage":"pre_filter_blocked","event_id":"...","host_id":"..."}`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum PipelineStage {
    /// An inbound event left the Fast-ACK queue and started pipeline processing.
    Ingested {
        /// Unique identifier of the inbound event.
        event_id: String,
        /// Source platform identifier (e.g. `discord`).
        platform: String,
        /// Channel the message originated from.
        channel_id: String,
        /// User that authored the message.
        sender_id: String,
    },
    /// PreFilter interception chain evaluation started across the active hosts.
    PreFilterStarted {
        /// Unique identifier of the inbound event.
        event_id: String,
        /// Number of plugin hosts participating in the chain.
        host_count: usize,
    },
    /// The event was intercepted and blocked by a plugin pre-filter.
    PreFilterBlocked {
        /// Unique identifier of the inbound event.
        event_id: String,
        /// Host that blocked the event.
        host_id: String,
    },
    /// The event passed every pre-filter (or the remaining chain was short-circuited).
    PreFilterPassed {
        /// Unique identifier of the inbound event.
        event_id: String,
    },
    /// A slash command was matched and dispatched to a plugin host.
    CommandMatched {
        /// Unique identifier of the inbound event.
        event_id: String,
        /// Command name without the leading slash.
        command: String,
        /// Plugin that declared the command.
        plugin_id: String,
        /// Host process executing the command.
        host_id: String,
    },
    /// A slash command was parsed but no plugin declared it.
    CommandNotFound {
        /// Unique identifier of the inbound event.
        event_id: String,
        /// Command name without the leading slash.
        command: String,
    },
    /// LLM reasoning produced a conversational reply.
    LlmReplied {
        /// Unique identifier of the inbound event.
        event_id: String,
        /// Session key used for conversational memory.
        session_id: String,
        /// Character length of the generated reply.
        content_length: usize,
    },
    /// Outbound replies were enqueued towards a platform adapter.
    OutboundQueued {
        /// Unique identifier of the inbound event that produced the replies.
        event_id: String,
        /// Destination platform.
        platform: String,
        /// Destination channel.
        channel_id: String,
        /// Number of outbound message segments enqueued.
        segment_count: usize,
    },
}

impl PipelineStage {
    /// Returns the stable stage name used in serialized trace records.
    pub fn name(&self) -> &'static str {
        match self {
            PipelineStage::Ingested { .. } => "ingested",
            PipelineStage::PreFilterStarted { .. } => "pre_filter_started",
            PipelineStage::PreFilterBlocked { .. } => "pre_filter_blocked",
            PipelineStage::PreFilterPassed { .. } => "pre_filter_passed",
            PipelineStage::CommandMatched { .. } => "command_matched",
            PipelineStage::CommandNotFound { .. } => "command_not_found",
            PipelineStage::LlmReplied { .. } => "llm_replied",
            PipelineStage::OutboundQueued { .. } => "outbound_queued",
        }
    }

    /// Returns the event identifier this stage belongs to, when applicable.
    pub fn event_id(&self) -> &str {
        match self {
            PipelineStage::Ingested { event_id, .. }
            | PipelineStage::PreFilterStarted { event_id, .. }
            | PipelineStage::PreFilterBlocked { event_id, .. }
            | PipelineStage::PreFilterPassed { event_id }
            | PipelineStage::CommandMatched { event_id, .. }
            | PipelineStage::CommandNotFound { event_id, .. }
            | PipelineStage::LlmReplied { event_id, .. }
            | PipelineStage::OutboundQueued { event_id, .. } => event_id,
        }
    }
}

/// Receiver of pipeline lifecycle stage records.
///
/// Implementations must return promptly: they execute inline on the pipeline worker task.
pub trait PipelineObserver: Send + Sync {
    /// Invoked once per lifecycle transition.
    fn on_stage(&self, stage: &PipelineStage);
}
