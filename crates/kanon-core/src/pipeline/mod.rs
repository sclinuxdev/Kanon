//! Message pipeline and command/tool routing engine.
//!
//! Provides the central message processing pipeline for Kanon Core, including:
//! - [`PreFilterChain`]: Ordered PreFilter interception chain with priority scheduling and strict 30ms total deadline.
//! - [`CommandRouter`]: Slash command parser and router dispatching to plugin hosts.
//! - [`PipelineEngine`]: Asynchronous worker loop consuming ingested events and managing outbound dispatch.
//! - [`PipelineObserver`]: Fire-and-forget lifecycle observation hook for control-plane tracing.

pub mod command;
pub mod dead_letter;
pub mod engine;
pub mod observer;
pub mod pre_filter;

pub use command::{CommandRouter, MatchedCommand};
pub use dead_letter::{DEFAULT_DEAD_LETTER_DIR, DeadLetterRecord, DeadLetterWriter};
pub use engine::{
    DEFAULT_OUTBOUND_QUEUE_CAPACITY, DeliveryOutcome, NEW_SESSION_COMMAND, PipelineEngine,
    PipelineResult,
};
pub use observer::{PipelineObserver, PipelineStage};
pub use pre_filter::{
    PREFILTER_TOTAL_DEADLINE, PREFILTER_WARN_THRESHOLD, PreFilterChain, PreFilterOutcome,
};
