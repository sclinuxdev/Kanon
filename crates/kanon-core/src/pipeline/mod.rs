//! Message pipeline and command/tool routing engine.
//!
//! Provides the central message processing pipeline for Kanon Core, including:
//! - [`PreFilterChain`]: Ordered PreFilter interception chain with priority scheduling and strict 30ms total deadline.
//! - [`CommandRouter`]: Slash command parser and router dispatching to plugin hosts.
//! - [`PipelineEngine`]: Asynchronous worker loop consuming ingested events and managing outbound dispatch.

pub mod command;
pub mod engine;
pub mod pre_filter;

pub use command::{CommandRouter, MatchedCommand};
pub use engine::{PipelineEngine, PipelineResult};
pub use pre_filter::{
    PreFilterChain, PreFilterOutcome, PREFILTER_TOTAL_DEADLINE, PREFILTER_WARN_THRESHOLD,
};
