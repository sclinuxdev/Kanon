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

#[cfg(test)]
mod tests {
    use super::*;
    use kanon_proto::v1::message_pipeline_service_server::{
        MessagePipelineService, MessagePipelineServiceServer,
    };
    use kanon_proto::v1::pre_filter_result::Action;
    use kanon_proto::v1::{
        CommandExecuteRequest, CommandExecuteResponse, DeliverMessageRequest, DeliverMessageResponse,
        EventAck, EventNotification, PreFilterResult, TextSegment, ToolCallRequest, ToolCallResponse,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;
    use tonic::{Request, Response, Status};

    struct MockPipeline {
        action: i32,
        modified_text: String,
        delay: Option<Duration>,
        call_count: Arc<AtomicUsize>,
    }

    #[tonic::async_trait]
    impl MessagePipelineService for MockPipeline {
        async fn on_pre_filter(
            &self,
            _req: Request<PipelineEventRequest>,
        ) -> Result<Response<PreFilterResult>, Status> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            Ok(Response::new(PreFilterResult {
                action: self.action,
                modified_text: self.modified_text.clone(),
                reply_messages: if self.action == Action::Block as i32 {
                    vec![MessageSegment {
                        segment: Some(Segment::Text(TextSegment {
                            content: "Blocked by mock".to_string(),
                        })),
                    }]
                } else {
                    vec![]
                },
            }))
        }

        async fn on_execute_command(
            &self,
            _req: Request<CommandExecuteRequest>,
        ) -> Result<Response<CommandExecuteResponse>, Status> {
            Ok(Response::new(CommandExecuteResponse {
                success: true,
                replies: vec![],
                error_message: String::new(),
            }))
        }

        async fn on_call_tool(
            &self,
            _req: Request<ToolCallRequest>,
        ) -> Result<Response<ToolCallResponse>, Status> {
            Ok(Response::new(ToolCallResponse {
                call_id: String::new(),
                success: true,
                error_message: String::new(),
                payload: None,
            }))
        }

        async fn on_event(
            &self,
            _req: Request<EventNotification>,
        ) -> Result<Response<EventAck>, Status> {
            Ok(Response::new(EventAck { received: true }))
        }

        async fn on_deliver_message(
            &self,
            _req: Request<DeliverMessageRequest>,
        ) -> Result<Response<DeliverMessageResponse>, Status> {
            Ok(Response::new(DeliverMessageResponse {
                success: true,
                message_id: "deliv_1".to_string(),
                error_message: String::new(),
            }))
        }
    }

    async fn spawn_mock_host(
        host_id: &str,
        priority: i32,
        action: i32,
        modified_text: String,
        delay: Option<Duration>,
    ) -> (Arc<ManagedHost>, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral TCP");
        let local_addr = listener.local_addr().expect("local addr");
        let call_count = Arc::new(AtomicUsize::new(0));

        let mock_service = MockPipeline {
            action,
            modified_text,
            delay,
            call_count: call_count.clone(),
        };

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(MessagePipelineServiceServer::new(mock_service))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .expect("mock server failed");
        });

        let endpoint_str = format!("http://{local_addr}");
        let channel = tonic::transport::Channel::from_shared(endpoint_str)
            .expect("valid URI")
            .connect()
            .await
            .expect("channel connect");

        let managed_host = Arc::new(ManagedHost::new(
            host_id.to_string(),
            PathBuf::from(format!("/tmp/{host_id}.sock")),
            channel,
            vec![],
            priority,
        ));

        (managed_host, call_count)
    }

    fn make_test_event(text: &str) -> PipelineEventRequest {
        PipelineEventRequest {
            event_id: "evt_test".to_string(),
            platform: "test".to_string(),
            channel_id: "chan_test".to_string(),
            sender_id: "user_test".to_string(),
            raw_text: text.to_string(),
            segments: vec![MessageSegment {
                segment: Some(Segment::Text(TextSegment {
                    content: text.to_string(),
                })),
            }],
            metadata: None,
        }
    }

    #[tokio::test]
    async fn test_pre_filter_empty_hosts() {
        let evt = make_test_event("hello");
        let outcome = PreFilterChain::execute(evt.clone(), &[]).await;
        assert_eq!(outcome, PreFilterOutcome::Passed(evt));
    }

    #[tokio::test]
    async fn test_pre_filter_priority_order_and_modify() {
        // Host 2 has lower priority (200), Host 1 has higher priority (100)
        let (host2, count2) = spawn_mock_host("host2", 200, Action::Modify as i32, "final_banana".to_string(), None).await;
        let (host1, count1) = spawn_mock_host("host1", 100, Action::Modify as i32, "intermediate_apple".to_string(), None).await;

        let evt = make_test_event("initial_text");
        // Pass hosts in reverse order to verify that sorting strictly orders by priority
        let outcome = PreFilterChain::execute(evt, &[host2, host1]).await;

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);

        match outcome {
            PreFilterOutcome::Passed(res) => {
                assert_eq!(res.raw_text, "final_banana");
            }
            _ => panic!("Expected PreFilterOutcome::Passed"),
        }
    }

    #[tokio::test]
    async fn test_pre_filter_blocking_short_circuit() {
        let (host_block, count_block) = spawn_mock_host("host_block", 100, Action::Block as i32, String::new(), None).await;
        let (host_pass, count_pass) = spawn_mock_host("host_pass", 200, Action::Pass as i32, String::new(), None).await;

        let evt = make_test_event("test message");
        let outcome = PreFilterChain::execute(evt, &[host_block, host_pass]).await;

        assert_eq!(count_block.load(Ordering::SeqCst), 1);
        assert_eq!(count_pass.load(Ordering::SeqCst), 0, "Downstream filter must not be invoked after Block");

        match outcome {
            PreFilterOutcome::Blocked { host_id, reply_messages } => {
                assert_eq!(host_id, "host_block");
                assert_eq!(reply_messages.len(), 1);
            }
            _ => panic!("Expected PreFilterOutcome::Blocked"),
        }
    }

    #[tokio::test]
    async fn test_pre_filter_deadline_short_circuit() {
        // Host 1 sleeps 40ms, exceeding the 30ms total deadline
        let (host_slow, count_slow) = spawn_mock_host("host_slow", 50, Action::Pass as i32, String::new(), Some(Duration::from_millis(40))).await;
        let (host_fast, count_fast) = spawn_mock_host("host_fast", 100, Action::Pass as i32, String::new(), None).await;

        let evt = make_test_event("slow message");
        let outcome = PreFilterChain::execute(evt.clone(), &[host_slow, host_fast]).await;

        assert_eq!(count_slow.load(Ordering::SeqCst), 1);
        assert_eq!(count_fast.load(Ordering::SeqCst), 0, "Subsequent filter must be short-circuited due to 30ms deadline");

        // System must gracefully pass the event downstream when budget is exhausted
        assert_eq!(outcome, PreFilterOutcome::Passed(evt));
    }
}
