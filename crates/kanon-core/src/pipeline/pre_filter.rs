//! PreFilter interception chain scheduler.
//!
//! Executes plugin `OnPreFilter` handlers in ascending priority order with strict
//! total deadline enforcement (30ms) and individual latency monitoring (5ms threshold).

use std::sync::Arc;
use std::time::{Duration, Instant};

use kanon_proto::v1::message_segment::Segment;
use kanon_proto::v1::{MessageSegment, PipelineEventRequest};

use crate::supervisor::ManagedHost;

/// Maximum allowable total duration across the entire PreFilter execution chain.
///
/// If cumulative execution time reaches or exceeds this budget, the microkernel
/// short-circuits the remaining filters to guarantee message pipeline throughput.
pub const PREFILTER_TOTAL_DEADLINE: Duration = Duration::from_millis(30);

/// Performance degradation warning threshold for a single plugin's PreFilter execution.
pub const PREFILTER_WARN_THRESHOLD: Duration = Duration::from_millis(5);

/// Outcome produced after evaluating the PreFilter chain across all active plugin hosts.
#[derive(Debug, Clone, PartialEq)]
pub enum PreFilterOutcome {
    /// Inbound event passed all filters (or remaining filters were short-circuited).
    Passed(PipelineEventRequest),
    /// Inbound event was explicitly blocked by a plugin pre-filter.
    Blocked {
        /// Identifier of the host that intercepted and blocked the event.
        host_id: String,
        /// Optional outbound reply messages emitted by the blocking filter.
        reply_messages: Vec<MessageSegment>,
    },
}

/// Scheduler executing the ordered chain of PreFilters across managed plugin hosts.
pub struct PreFilterChain;

impl PreFilterChain {
    /// Executes the pre-filter chain on the provided inbound event.
    ///
    /// # Execution Guarantees
    /// 1. **Priority Ordering**: Filters run in strictly ascending priority order
    ///    (lower numerical value executes earlier). Ties are broken deterministically by `host_id`.
    /// 2. **Strict 30ms Deadline**: The cumulative elapsed time across all filters is bounded by
    ///    `PREFILTER_TOTAL_DEADLINE`. If the deadline expires, subsequent filters are skipped and
    ///    the event proceeds downstream.
    /// 3. **Latency Warnings**: Any single plugin taking longer than `PREFILTER_WARN_THRESHOLD` (5ms)
    ///    triggers a performance warning log.
    /// 4. **Action Semantics**:
    ///    - `Action::Pass (0)`: Proceeds to the next filter in sequence.
    ///    - `Action::Block (1)`: Immediately halts the chain and returns [`PreFilterOutcome::Blocked`].
    ///    - `Action::Modify (2)`: Updates the event text and propagates modified text downstream.
    pub async fn execute(
        event: PipelineEventRequest,
        hosts: &[Arc<ManagedHost>],
    ) -> PreFilterOutcome {
        if hosts.is_empty() {
            return PreFilterOutcome::Passed(event);
        }

        // Sort hosts by priority ascending; break ties using host_id.
        let mut sorted_hosts: Vec<Arc<ManagedHost>> = hosts.to_vec();
        sorted_hosts.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.host_id.cmp(&b.host_id))
        });

        let mut current_event = event;
        let chain_start = Instant::now();
        let deadline = chain_start + PREFILTER_TOTAL_DEADLINE;

        for host in sorted_hosts {
            let now = Instant::now();
            if now >= deadline {
                tracing::warn!(
                    event_id = %current_event.event_id,
                    elapsed_ms = chain_start.elapsed().as_millis(),
                    "PreFilter total deadline (30ms) reached; short-circuiting remaining plugins"
                );
                break;
            }

            let remaining_budget = deadline - now;
            let filter_start = Instant::now();

            let filter_result = match tokio::time::timeout(
                remaining_budget,
                host.pre_filter(current_event.clone()),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(status)) => {
                    tracing::error!(
                        host_id = %host.host_id,
                        event_id = %current_event.event_id,
                        error = %status,
                        "Host PreFilter returned gRPC error; bypassing this filter"
                    );
                    continue;
                }
                Err(_timeout_elapsed) => {
                    tracing::warn!(
                        host_id = %host.host_id,
                        event_id = %current_event.event_id,
                        total_elapsed_ms = chain_start.elapsed().as_millis(),
                        "PreFilter call timed out; 30ms total budget expired. Short-circuiting remaining chain"
                    );
                    break;
                }
            };

            let filter_elapsed = filter_start.elapsed();
            if filter_elapsed > PREFILTER_WARN_THRESHOLD {
                tracing::warn!(
                    host_id = %host.host_id,
                    elapsed_ms = filter_elapsed.as_millis(),
                    "PreFilter execution exceeded 5ms latency threshold"
                );
            }

            match filter_result.action {
                // Action::Pass = 0
                0 => {
                    tracing::trace!(
                        host_id = %host.host_id,
                        event_id = %current_event.event_id,
                        "PreFilter passed"
                    );
                }
                // Action::Block = 1
                1 => {
                    tracing::info!(
                        host_id = %host.host_id,
                        event_id = %current_event.event_id,
                        reply_count = filter_result.reply_messages.len(),
                        "PreFilter blocked event"
                    );
                    return PreFilterOutcome::Blocked {
                        host_id: host.host_id.clone(),
                        reply_messages: filter_result.reply_messages,
                    };
                }
                // Action::Modify = 2
                2 => {
                    tracing::debug!(
                        host_id = %host.host_id,
                        event_id = %current_event.event_id,
                        old_text = %current_event.raw_text,
                        new_text = %filter_result.modified_text,
                        "PreFilter modified event text"
                    );
                    current_event.raw_text = filter_result.modified_text.clone();
                    // Also synchronize the modified text into the primary text segment if present.
                    for segment in &mut current_event.segments {
                        if let Some(Segment::Text(ref mut text)) = segment.segment {
                            text.content = filter_result.modified_text.clone();
                            break;
                        }
                    }
                }
                unknown => {
                    tracing::warn!(
                        host_id = %host.host_id,
                        action = unknown,
                        "Unknown PreFilter action enum value received; treating as Pass"
                    );
                }
            }
        }

        PreFilterOutcome::Passed(current_event)
    }
}

