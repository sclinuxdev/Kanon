//! Platform adapter contract coverage: registration, routing, delivery and ingest semantics.
//!
//! The plugin half of the contract is verified against a real gRPC host served in-process, so the
//! exact `OnDeliverMessage` round trip the core performs is exercised rather than simulated.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kanon_core::adapter::{AdapterError, AdapterKind, EventIngress, IngestError};
use kanon_core::pipeline::PipelineEngine;
use kanon_core::supervisor::Supervisor;
use kanon_core::{ManagedHost, PluginManifest};
use kanon_proto::v1::message_segment::Segment;
use kanon_proto::v1::message_pipeline_service_server::{
    MessagePipelineService, MessagePipelineServiceServer,
};
use kanon_proto::v1::{
    CommandExecuteRequest, CommandExecuteResponse, DeliverMessageRequest, DeliverMessageResponse,
    EventAck, EventNotification, IngestEventRequest, MessageSegment, PipelineEventRequest,
    PluginMeta, PreFilterResult, TextSegment, ToolCallRequest, ToolCallResponse,
};
use kanon_transport::connect_ipc;
use tempfile::tempdir;
use tokio::sync::{Mutex, mpsc};

use common::ChannelAdapter;

/// Manifest declaring a platform adapter, as a plugin author would write it.
const ADAPTER_MANIFEST: &str = r#"
[plugin]
id = "org.kanon.plugin.adapter_fixture"
name = "Adapter Fixture"
version = "1.0.0"
runtime = "rust"
entrypoint = "target/debug/adapter_fixture"

[adapter]
platform = "fixture_platform"
display_name = "Fixture Platform"
"#;

/// Mock plugin host that records the outbound messages it receives.
#[derive(Default)]
struct RecordingHost {
    deliveries: Mutex<Vec<DeliverMessageRequest>>,
    /// When true the host rejects every delivery, mimicking a platform-side failure.
    reject_delivery: bool,
}

#[tonic::async_trait]
impl MessagePipelineService for RecordingHost {
    async fn on_pre_filter(
        &self,
        _request: tonic::Request<PipelineEventRequest>,
    ) -> Result<tonic::Response<PreFilterResult>, tonic::Status> {
        Ok(tonic::Response::new(PreFilterResult {
            action: kanon_proto::v1::pre_filter_result::Action::Pass as i32,
            modified_text: String::new(),
            reply_messages: vec![],
        }))
    }

    async fn on_execute_command(
        &self,
        _request: tonic::Request<CommandExecuteRequest>,
    ) -> Result<tonic::Response<CommandExecuteResponse>, tonic::Status> {
        Ok(tonic::Response::new(CommandExecuteResponse {
            success: true,
            replies: vec![],
            error_message: String::new(),
        }))
    }

    async fn on_call_tool(
        &self,
        _request: tonic::Request<ToolCallRequest>,
    ) -> Result<tonic::Response<ToolCallResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("no tools"))
    }

    async fn on_event(
        &self,
        _request: tonic::Request<EventNotification>,
    ) -> Result<tonic::Response<EventAck>, tonic::Status> {
        Ok(tonic::Response::new(EventAck { received: true }))
    }

    async fn on_deliver_message(
        &self,
        request: tonic::Request<DeliverMessageRequest>,
    ) -> Result<tonic::Response<DeliverMessageResponse>, tonic::Status> {
        let req = request.into_inner();

        if self.reject_delivery {
            return Ok(tonic::Response::new(DeliverMessageResponse {
                success: false,
                message_id: String::new(),
                error_message: "platform rejected the message".to_string(),
            }));
        }

        let message_id = format!("plugin-{}", req.channel_id);
        self.deliveries.lock().await.push(req);

        Ok(tonic::Response::new(DeliverMessageResponse {
            success: true,
            message_id,
            error_message: String::new(),
        }))
    }
}

/// Starts an in-process plugin host serving `MessagePipelineService` on a temporary socket.
async fn start_recording_host(
    socket_path: PathBuf,
    reject_delivery: bool,
) -> Arc<RecordingHost> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    let service = Arc::new(RecordingHost {
        deliveries: Mutex::new(Vec::new()),
        reject_delivery,
    });

    let listener = kanon_transport::IpcListener::bind(&socket_path).expect("host socket binds");
    let incoming = listener.incoming();
    let served = service.clone();

    tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(MessagePipelineServiceServer::new(RecordingHostServer { inner: served }))
            .serve_with_incoming(incoming)
            .await;
    });

    service
}

/// Tonic adapter around [`RecordingHost`] (the trait impl itself owns the state).
struct RecordingHostServer {
    inner: Arc<RecordingHost>,
}

#[tonic::async_trait]
impl MessagePipelineService for RecordingHostServer {
    async fn on_pre_filter(
        &self,
        request: tonic::Request<PipelineEventRequest>,
    ) -> Result<tonic::Response<PreFilterResult>, tonic::Status> {
        self.inner.on_pre_filter(request).await
    }

    async fn on_execute_command(
        &self,
        request: tonic::Request<CommandExecuteRequest>,
    ) -> Result<tonic::Response<CommandExecuteResponse>, tonic::Status> {
        self.inner.on_execute_command(request).await
    }

    async fn on_call_tool(
        &self,
        request: tonic::Request<ToolCallRequest>,
    ) -> Result<tonic::Response<ToolCallResponse>, tonic::Status> {
        self.inner.on_call_tool(request).await
    }

    async fn on_event(
        &self,
        request: tonic::Request<EventNotification>,
    ) -> Result<tonic::Response<EventAck>, tonic::Status> {
        self.inner.on_event(request).await
    }

    async fn on_deliver_message(
        &self,
        request: tonic::Request<DeliverMessageRequest>,
    ) -> Result<tonic::Response<DeliverMessageResponse>, tonic::Status> {
        self.inner.on_deliver_message(request).await
    }
}

/// Builds an outbound request for the fixture platform.
fn outbound_request(platform: &str) -> DeliverMessageRequest {
    DeliverMessageRequest {
        platform: platform.to_string(),
        channel_id: "chan-1".to_string(),
        recipient_id: "alice".to_string(),
        segments: vec![MessageSegment {
            segment: Some(Segment::Text(TextSegment {
                content: "hello".to_string(),
            })),
        }],
    }
}

/// Registers a plugin host whose manifest declares `fixture_platform`.
async fn register_adapter_host(supervisor: &Supervisor, run_dir: &std::path::Path) -> Arc<RecordingHost> {
    let socket_path = run_dir.join("host_fixture_adapter.sock");
    let recorded = start_recording_host(socket_path.clone(), false).await;

    let channel = connect_ipc(&socket_path)
        .await
        .expect("test host reachable");

    let manifest: PluginManifest = toml::from_str(ADAPTER_MANIFEST).expect("manifest parses");
    let host = Arc::new(
        ManagedHost::new(
            "org_kanon_plugin_adapter_fixture".to_string(),
            socket_path,
            channel,
            vec![PluginMeta {
                id: "org.kanon.plugin.adapter_fixture".to_string(),
                name: "Adapter Fixture".to_string(),
                version: "1.0.0".to_string(),
                ..PluginMeta::default()
            }],
            500,
        )
        .with_manifest(manifest),
    );

    supervisor.register_managed_host(host).await;
    recorded
}

/// Built-in adapters are registered, listed and resolved by platform.
#[tokio::test]
async fn builtin_adapter_registers_and_resolves() {
    let run_dir = tempdir().expect("temp dir");
    let supervisor = Arc::new(Supervisor::new(Some(run_dir.path().to_path_buf()), None));

    let (tx, _rx) = mpsc::channel(4);
    supervisor
        .adapters()
        .register(ChannelAdapter::shared("local_test", tx))
        .await
        .expect("registration succeeds");

    // Duplicate platform registration would make routing order-dependent, so it must fail.
    let (tx2, _rx2) = mpsc::channel(4);
    let error = supervisor
        .adapters()
        .register(ChannelAdapter::shared("local_test", tx2))
        .await
        .expect_err("duplicate platform rejected");
    assert!(matches!(error, AdapterError::DuplicatePlatform(p) if p == "local_test"));

    let route = supervisor
        .resolve_adapter("local_test")
        .await
        .expect("platform resolves");
    match route {
        kanon_core::AdapterRoute::Builtin(adapter) => {
            assert_eq!(adapter.platform(), "local_test");
            assert!(adapter.is_connected());
        }
        other => panic!("expected a built-in route, got {other:?}"),
    }

    let catalog = supervisor.adapter_catalog().await;
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].kind, AdapterKind::Builtin);
    assert_eq!(catalog[0].display_name, "Test channel adapter");
    assert!(catalog[0].host_id.is_none());
}

/// Unrouted platforms are reported explicitly instead of silently dropped.
#[tokio::test]
async fn unknown_platform_is_reported() {
    let run_dir = tempdir().expect("temp dir");
    let supervisor = Arc::new(Supervisor::new(Some(run_dir.path().to_path_buf()), None));
    let engine = PipelineEngine::new(supervisor);

    let error = engine
        .deliver_outbound(outbound_request("nowhere"))
        .await
        .expect_err("unknown platforms must fail");
    assert!(matches!(error, AdapterError::UnknownPlatform(p) if p == "nowhere"));
}

/// Outbound messages reach a built-in adapter through the engine.
#[tokio::test]
async fn outbound_is_routed_to_builtin_adapter() {
    let run_dir = tempdir().expect("temp dir");
    let supervisor = Arc::new(Supervisor::new(Some(run_dir.path().to_path_buf()), None));

    let (tx, mut rx) = mpsc::channel(4);
    supervisor
        .adapters()
        .register(ChannelAdapter::shared("local_test", tx))
        .await
        .expect("registration succeeds");

    let engine = PipelineEngine::new(supervisor);
    let outcome = engine
        .deliver_outbound(outbound_request("local_test"))
        .await
        .expect("delivery succeeds");

    assert_eq!(outcome.kind, AdapterKind::Builtin);
    assert_eq!(outcome.platform, "local_test");
    assert_eq!(outcome.message_id, "test-msg-chan-1");

    let delivered = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("delivery observed")
        .expect("channel open");
    assert_eq!(delivered.recipient_id, "alice");
    assert_eq!(delivered.segments.len(), 1);
}

/// Plugin-declared platforms are discovered from the manifest and delivered over gRPC.
#[tokio::test]
async fn outbound_is_routed_to_plugin_adapter_host() {
    let run_dir = tempdir().expect("temp dir");
    let supervisor = Arc::new(Supervisor::new(Some(run_dir.path().to_path_buf()), None));
    let recorded = register_adapter_host(&supervisor, run_dir.path()).await;

    // Discovery derives strictly from the manifest declaration.
    let catalog = supervisor.adapter_catalog().await;
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].platform, "fixture_platform");
    assert_eq!(catalog[0].kind, AdapterKind::Plugin);
    assert_eq!(catalog[0].display_name, "Fixture Platform");
    assert_eq!(catalog[0].host_id.as_deref(), Some("org_kanon_plugin_adapter_fixture"));
    assert_eq!(catalog[0].plugin_id.as_deref(), Some("org.kanon.plugin.adapter_fixture"));

    let engine = PipelineEngine::new(supervisor);
    let outcome = engine
        .deliver_outbound(outbound_request("fixture_platform"))
        .await
        .expect("plugin delivery succeeds");

    assert_eq!(outcome.kind, AdapterKind::Plugin);
    assert_eq!(outcome.platform, "fixture_platform");
    assert_eq!(outcome.message_id, "plugin-chan-1");

    let deliveries = recorded.deliveries.lock().await;
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].channel_id, "chan-1");
    assert_eq!(deliveries[0].recipient_id, "alice");
}

/// A plugin that refuses the message turns into an explicit delivery error.
#[tokio::test]
async fn plugin_rejection_becomes_adapter_error() {
    let run_dir = tempdir().expect("temp dir");
    let supervisor = Arc::new(Supervisor::new(Some(run_dir.path().to_path_buf()), None));

    let socket_path = run_dir.path().join("host_fixture_adapter.sock");
    start_recording_host(socket_path.clone(), true).await;
    let channel = connect_ipc(&socket_path)
        .await
        .expect("test host reachable");

    let manifest: PluginManifest = toml::from_str(ADAPTER_MANIFEST).expect("manifest parses");
    supervisor
        .register_managed_host(Arc::new(
            ManagedHost::new(
                "org_kanon_plugin_adapter_fixture".to_string(),
                socket_path,
                channel,
                vec![PluginMeta {
                    id: "org.kanon.plugin.adapter_fixture".to_string(),
                    ..PluginMeta::default()
                }],
                500,
            )
            .with_manifest(manifest),
        ))
        .await;

    let engine = PipelineEngine::new(supervisor);
    let error = engine
        .deliver_outbound(outbound_request("fixture_platform"))
        .await
        .expect_err("plugin rejection must surface");
    assert!(
        matches!(error, AdapterError::Delivery { ref reason, .. } if reason.contains("platform rejected the message")),
        "unexpected error: {error}"
    );
}

/// The ingest handle enforces Fast-ACK semantics: accepted, saturated, or closed.
#[tokio::test]
async fn ingest_handle_reports_backpressure_and_closure() {
    // Capacity 1: the second enqueue must report saturation instead of blocking.
    let (tx, mut rx) = mpsc::channel(1);
    let ingress = EventIngress::new(tx);
    assert_eq!(ingress.capacity(), 1);

    let event = |id: &str| IngestEventRequest {
        platform: "local_test".to_string(),
        event: Some(PipelineEventRequest {
            event_id: id.to_string(),
            platform: "local_test".to_string(),
            channel_id: "chan-1".to_string(),
            sender_id: "alice".to_string(),
            raw_text: "hello".to_string(),
            segments: vec![],
            metadata: None,
        }),
    };

    assert!(ingress.try_ingest(event("evt-1")).is_ok());
    assert_eq!(
        ingress.try_ingest(event("evt-2")),
        Err(IngestError::QueueFull)
    );

    // Draining frees capacity again.
    let drained = rx.recv().await.expect("event received");
    assert_eq!(drained.event.expect("payload").event_id, "evt-1");
    assert!(ingress.try_ingest(event("evt-3")).is_ok());

    // Closing the consumer surfaces as an explicit, non-retryable state.
    drop(rx);
    assert!(ingress.is_closed());
    assert_eq!(ingress.try_ingest(event("evt-4")), Err(IngestError::Closed));
    assert_eq!(ingress.ingest(event("evt-5")).await, Err(IngestError::Closed));
}
