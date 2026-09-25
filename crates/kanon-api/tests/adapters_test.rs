//! Platform adapter route coverage: catalog, Fast-ACK ingress and outbound delivery.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::Method;
use kanon_api::app;
use kanon_core::PlatformAdapter;
use kanon_proto::v1::message_segment::Segment;
use kanon_proto::v1::{DeliverMessageRequest, IngestEventRequest, MessageSegment, TextSegment};
use serde_json::json;

use common::{
    BUILTIN_PLATFORM, PLUGIN_PLATFORM, RecordingAdapter, adapter_state, empty_state, error_code,
    fixture_state, send_json,
};

/// The catalog merges built-in adapters with plugin manifest declarations.
#[tokio::test]
async fn adapter_catalog_lists_builtin_and_plugin_adapters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, _ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;
    let app: Router = app(state);

    let (status, body) = send_json(&app, Method::GET, "/api/v1/adapters", None).await;

    assert_eq!(status, 200);
    assert_eq!(body["total"], 2);

    let adapters = body["adapters"].as_array().expect("adapter array");
    let builtin = adapters
        .iter()
        .find(|adapter| adapter["platform"] == BUILTIN_PLATFORM)
        .expect("built-in adapter listed");
    assert_eq!(builtin["kind"], "builtin");
    assert_eq!(builtin["display_name"], "Built-in fixture adapter");
    assert_eq!(builtin["connected"], true);
    assert!(builtin["host_id"].is_null());

    let plugin = adapters
        .iter()
        .find(|adapter| adapter["platform"] == PLUGIN_PLATFORM)
        .expect("plugin adapter listed");
    assert_eq!(plugin["kind"], "plugin");
    assert_eq!(plugin["display_name"], "Fixture Platform");
    assert_eq!(plugin["connected"], true);
    assert_eq!(plugin["host_id"], common::FIXTURE_HOST_ID);
    assert_eq!(plugin["plugin_id"], common::FIXTURE_PLUGIN_ID);
}

/// A node without adapters reports an empty catalog rather than failing.
#[tokio::test]
async fn adapter_catalog_is_empty_without_adapters() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(empty_state(PathBuf::from(dir.path())).await);

    let (status, body) = send_json(&app, Method::GET, "/api/v1/adapters", None).await;

    assert_eq!(status, 200);
    assert_eq!(body["total"], 0);
    assert_eq!(body["adapters"].as_array().map(Vec::len), Some(0));
}

/// Inbound messages for a served platform are Fast-ACKed and enqueued verbatim.
#[tokio::test]
async fn ingest_accepts_and_enqueues_event() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, mut ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;
    let app: Router = app(state);

    let (status, body) = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/adapters/{BUILTIN_PLATFORM}/ingest"),
        Some(json!({
            "channel_id": "chan-1",
            "sender_id": "alice",
            "text": "hello kanon",
            "event_id": "evt-42",
            "metadata": { "update_id": 9001 }
        })),
    )
    .await;

    assert_eq!(status, 202, "body: {body}");
    assert_eq!(body["accepted"], true);
    assert_eq!(body["event_id"], "evt-42");
    assert_eq!(body["platform"], BUILTIN_PLATFORM);

    let queued: IngestEventRequest = tokio::time::timeout(Duration::from_secs(2), ingest_rx.recv())
        .await
        .expect("event enqueued")
        .expect("channel open");
    assert_eq!(queued.platform, BUILTIN_PLATFORM);
    let event = queued.event.expect("inner event");
    assert_eq!(event.event_id, "evt-42");
    assert_eq!(event.platform, BUILTIN_PLATFORM);
    assert_eq!(event.channel_id, "chan-1");
    assert_eq!(event.sender_id, "alice");
    assert_eq!(event.raw_text, "hello kanon");
    // Metadata round-trips into a `google.protobuf.Struct` (numbers are stored as doubles).
    let update_id = event
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.fields.get("update_id"))
        .and_then(|value| value.kind.as_ref())
        .and_then(|kind| match kind {
            kanon_proto::prost_types::value::Kind::NumberValue(number) => Some(*number),
            _ => None,
        });
    assert_eq!(update_id, Some(9001.0));
}

/// Plugin-declared platforms accept inbound messages too.
#[tokio::test]
async fn ingest_accepts_plugin_platform() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, mut ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;
    let app: Router = app(state);

    let (status, body) = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/adapters/{PLUGIN_PLATFORM}/ingest"),
        Some(json!({ "channel_id": "c", "sender_id": "u", "text": "hi" })),
    )
    .await;

    assert_eq!(status, 202, "body: {body}");
    // Without a caller-supplied id the gateway assigns one and reports it for trace correlation.
    assert!(
        body["event_id"].as_str().is_some_and(|id| !id.is_empty()),
        "event id must be generated: {body}"
    );

    let queued = ingest_rx.recv().await.expect("event enqueued");
    assert_eq!(queued.platform, PLUGIN_PLATFORM);
}

/// Messages for a platform no adapter serves are rejected: they could never be answered.
#[tokio::test]
async fn ingest_rejects_unknown_platform() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, _ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;
    let app: Router = app(state);

    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/adapters/nowhere/ingest",
        Some(json!({ "channel_id": "c", "sender_id": "u", "text": "hi" })),
    )
    .await;

    assert_eq!(status, 404);
    assert_eq!(error_code(&body), "not_found");
}

/// Request bodies are validated before anything is enqueued.
#[tokio::test]
async fn ingest_validates_request() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, mut ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;
    let app: Router = app(state);
    let uri = format!("/api/v1/adapters/{BUILTIN_PLATFORM}/ingest");

    for payload in [
        json!({ "channel_id": "", "sender_id": "u", "text": "hi" }),
        json!({ "channel_id": "c", "sender_id": "  ", "text": "hi" }),
        json!({ "channel_id": "c", "sender_id": "u", "text": "   " }),
    ] {
        let (status, body) = send_json(&app, Method::POST, &uri, Some(payload)).await;
        assert_eq!(status, 400, "body: {body}");
        assert_eq!(error_code(&body), "bad_request");
    }

    assert!(
        ingest_rx.try_recv().is_err(),
        "rejected requests must not enqueue events"
    );
}

/// A saturated ingest queue reports backpressure instead of blocking the caller.
#[tokio::test]
async fn ingest_reports_queue_saturation() {
    let dir = tempfile::tempdir().expect("temp dir");
    // Capacity 1 with no consumer: the second message must be refused explicitly.
    let (state, _ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 1).await;
    let app: Router = app(state);
    let uri = format!("/api/v1/adapters/{BUILTIN_PLATFORM}/ingest");
    let payload = json!({ "channel_id": "c", "sender_id": "u", "text": "hi" });

    let (first, _) = send_json(&app, Method::POST, &uri, Some(payload.clone())).await;
    assert_eq!(first, 202);

    let (second, body) = send_json(&app, Method::POST, &uri, Some(payload)).await;
    assert_eq!(second, 503);
    assert_eq!(error_code(&body), "unavailable");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("queue")),
        "error must name the saturated queue: {body}"
    );
}

/// A gateway without a pipeline attached refuses inbound messages explicitly.
#[tokio::test]
async fn ingest_without_pipeline_is_unavailable() {
    let dir = tempfile::tempdir().expect("temp dir");
    // `fixture_state` builds state without an ingest handle.
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/adapters/{PLUGIN_PLATFORM}/ingest"),
        Some(json!({ "channel_id": "c", "sender_id": "u", "text": "hi" })),
    )
    .await;

    assert_eq!(status, 503);
    assert_eq!(error_code(&body), "unavailable");
}

/// The bundled webhook adapter POSTs a faithful JSON payload to its callback URL.
#[tokio::test]
async fn webhook_adapter_posts_payload_to_callback() {
    use axum::extract::State;
    use axum::routing::post;

    /// Captured callback request body.
    #[derive(Clone)]
    struct Receiver {
        payloads: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    }

    async fn capture(
        State(receiver): State<Receiver>,
        axum::Json(payload): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        receiver.payloads.lock().await.push(payload);
        axum::Json(json!({ "message_id": "callback-1" }))
    }

    let receiver = Receiver {
        payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    };
    let callback_app = Router::new()
        .route("/callback", post(capture))
        .with_state(receiver.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("callback listener binds");
    let callback_url = format!("http://{}/callback", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, callback_app).await;
    });

    let adapter = kanon_api::WebhookAdapter::new(
        "webhook_test",
        Some("Webhook Test".to_string()),
        Some(callback_url),
    )
    .expect("adapter constructs");

    assert_eq!(adapter.platform(), "webhook_test");
    assert_eq!(adapter.display_name(), "Webhook Test");
    assert!(adapter.is_connected());

    let response = adapter
        .deliver(DeliverMessageRequest {
            platform: "webhook_test".to_string(),
            channel_id: "chan-9".to_string(),
            recipient_id: "alice".to_string(),
            segments: vec![
                MessageSegment {
                    segment: Some(Segment::Text(TextSegment {
                        content: "hello from kanon".to_string(),
                    })),
                },
                MessageSegment {
                    segment: Some(Segment::Mention(kanon_proto::v1::MentionSegment {
                        target_user_id: "bob".to_string(),
                        display_name: "Bob".to_string(),
                        is_all: false,
                    })),
                },
            ],
            event_id: "evt-api-adapter-1".to_string(),
        })
        .await
        .expect("delivery succeeds");

    // The callback's own message id is surfaced so the core can trace the platform receipt.
    assert!(response.success);
    assert_eq!(response.message_id, "callback-1");

    let payloads = receiver.payloads.lock().await;
    assert_eq!(payloads.len(), 1);
    let payload = &payloads[0];
    assert_eq!(payload["platform"], "webhook_test");
    assert_eq!(payload["channel_id"], "chan-9");
    assert_eq!(payload["recipient_id"], "alice");
    assert_eq!(payload["segments"][0]["kind"], "text");
    assert_eq!(payload["segments"][0]["text"], "hello from kanon");
    assert_eq!(payload["segments"][1]["kind"], "mention");
    assert_eq!(payload["segments"][1]["target_user_id"], "bob");
}

/// Callback failures and missing configuration are explicit, never silently successful.
#[tokio::test]
async fn webhook_adapter_reports_delivery_failures() {
    use axum::http::StatusCode;
    use axum::routing::post;

    async fn failing() -> (StatusCode, &'static str) {
        (StatusCode::INTERNAL_SERVER_ERROR, "platform is down")
    }

    let failing_app = Router::new().route("/callback", post(failing));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener binds");
    let callback_url = format!("http://{}/callback", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, failing_app).await;
    });

    let adapter = kanon_api::WebhookAdapter::new("webhook_test", None, Some(callback_url))
        .expect("adapter constructs");

    let error = adapter
        .deliver(test_request())
        .await
        .expect_err("HTTP 500 must fail the delivery");
    assert!(
        error.to_string().contains("500"),
        "error must carry the status: {error}"
    );

    // Inbound-only adapters report themselves as not connected and fail loudly on delivery.
    let inbound_only = kanon_api::WebhookAdapter::new("webhook_inbound", None, None)
        .expect("adapter constructs");
    assert!(!inbound_only.is_connected());
    let error = inbound_only
        .deliver(test_request())
        .await
        .expect_err("missing callback must fail the delivery");
    assert!(error.to_string().contains("no callback URL"), "error: {error}");

    // An empty platform identifier is a configuration error, not a usable adapter.
    assert!(kanon_api::WebhookAdapter::new("   ", None, None).is_err());
}

/// Minimal outbound request used by the webhook failure tests.
fn test_request() -> DeliverMessageRequest {
    DeliverMessageRequest {
        platform: "webhook_test".to_string(),
        channel_id: "chan-9".to_string(),
        recipient_id: "alice".to_string(),
        segments: vec![MessageSegment {
            segment: Some(Segment::Text(TextSegment {
                content: "hello".to_string(),
            })),
        }],
        event_id: "evt-test-request".to_string(),
    }
}

/// Built-in adapters are reachable from the pipeline through the registry.
#[tokio::test]
async fn builtin_adapter_delivery_is_recorded() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, _ingest_rx, adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;

    let response = state
        .supervisor()
        .adapters()
        .get(BUILTIN_PLATFORM)
        .await
        .expect("adapter registered")
        .deliver(test_request())
        .await
        .expect("delivery succeeds");

    assert!(response.success);
    let deliveries: Vec<DeliverMessageRequest> = RecordingAdapter::deliveries(&adapter);
    assert_eq!(deliveries.len(), 1);
}

/// Inbound webhook ingest enforces HMAC-SHA256 signature verification when a secret is configured.
#[tokio::test]
async fn test_webhook_inbound_hmac_sha256_signature_verification() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let dir = tempfile::tempdir().expect("temp dir");
    let (state, mut ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;

    // Register a webhook adapter with secret
    let secret = "top-secret-signing-key";
    let secured_adapter = kanon_api::WebhookAdapter::new("secured_webhook", None, None)
        .expect("adapter constructs")
        .with_secret(secret);
    state
        .supervisor()
        .adapters()
        .register(Arc::new(secured_adapter))
        .await
        .expect("register secured adapter");

    let app: Router = app(state);

    let payload = json!({
        "channel_id": "sec-chan",
        "sender_id": "bob",
        "text": "authenticated message",
    });
    let payload_str = payload.to_string();

    // 1. Missing signature header must be rejected with 401 Unauthorized
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/adapters/secured_webhook/ingest")
        .header("content-type", "application/json")
        .body(Body::from(payload_str.clone()))
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("execute request");
    assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.expect("bytes");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(body["error"]["code"], "unauthorized");

    // 2. Invalid signature header must be rejected with 401 Unauthorized
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/adapters/secured_webhook/ingest")
        .header("content-type", "application/json")
        .header("x-hub-signature-256", "sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        .body(Body::from(payload_str.clone()))
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("execute request");
    assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);

    // 3. Valid HMAC-SHA256 signature must be accepted with 202
    let valid_sig = kanon_api::adapters::webhook::sign_hmac_sha256(
        secret.as_bytes(),
        payload_str.as_bytes(),
    );
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/adapters/secured_webhook/ingest")
        .header("content-type", "application/json")
        .header("x-hub-signature-256", format!("sha256={valid_sig}"))
        .body(Body::from(payload_str.clone()))
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("execute request");
    assert_eq!(resp.status(), axum::http::StatusCode::ACCEPTED);

    let queued = ingest_rx.recv().await.expect("queued event");
    assert_eq!(queued.platform, "secured_webhook");
}

/// Outbound delivery retries on transient errors (503) with backoff and succeeds when endpoint recovers.
#[tokio::test]
async fn test_webhook_outbound_retry_and_backoff() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use axum::http::StatusCode;
    use axum::routing::post;

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_clone = attempts.clone();

    async fn flaky_callback(
        axum::extract::State(counter): axum::extract::State<Arc<AtomicUsize>>,
    ) -> (StatusCode, axum::Json<serde_json::Value>) {
        let count = counter.fetch_add(1, Ordering::SeqCst);
        if count < 2 {
            // Fail first 2 attempts with transient 503
            (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"error": "server busy"})))
        } else {
            // Succeed on 3rd attempt
            (StatusCode::OK, axum::Json(json!({"message_id": "recovered-msg-99"})))
        }
    }

    let callback_app = Router::new()
        .route("/callback", post(flaky_callback))
        .with_state(attempts_clone);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener binds");
    let callback_url = format!("http://{}/callback", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, callback_app).await;
    });

    let adapter = kanon_api::WebhookAdapter::new("flaky_test", None, Some(callback_url))
        .expect("adapter constructs")
        .with_retry_config(kanon_api::adapters::webhook::WebhookRetryConfig {
            max_retries: 2,
            initial_backoff: Duration::from_millis(10),
        });

    let resp = adapter
        .deliver(test_request())
        .await
        .expect("delivery succeeds on 3rd attempt");

    assert!(resp.success);
    assert_eq!(resp.message_id, "recovered-msg-99");
    assert_eq!(attempts.load(Ordering::SeqCst), 3, "expected 3 total delivery attempts");
}

/// Outbound delivery must NOT retry on permanent 4xx client errors (e.g. 400 Bad Request).
#[tokio::test]
async fn test_webhook_outbound_non_retryable_on_4xx_client_error() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use axum::http::StatusCode;
    use axum::routing::post;

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_clone = attempts.clone();

    async fn bad_request_callback(
        axum::extract::State(counter): axum::extract::State<Arc<AtomicUsize>>,
    ) -> (StatusCode, &'static str) {
        counter.fetch_add(1, Ordering::SeqCst);
        (StatusCode::BAD_REQUEST, "invalid payload schema")
    }

    let callback_app = Router::new()
        .route("/callback", post(bad_request_callback))
        .with_state(attempts_clone);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener binds");
    let callback_url = format!("http://{}/callback", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, callback_app).await;
    });

    let adapter = kanon_api::WebhookAdapter::new("client_err_test", None, Some(callback_url))
        .expect("adapter constructs")
        .with_retry_config(kanon_api::adapters::webhook::WebhookRetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(10),
        });

    let err = adapter
        .deliver(test_request())
        .await
        .expect_err("400 must fail immediately");

    assert!(err.to_string().contains("400"), "error: {err}");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "permanent 4xx error must never be retried"
    );
}

/// Outbound delivery attaches HMAC signature headers when a secret is configured.
#[tokio::test]
async fn test_webhook_outbound_signing_header() {
    use tokio::sync::Mutex;
    use axum::http::HeaderMap;
    use axum::routing::post;

    let captured_header = Arc::new(Mutex::new(None));
    let captured_clone = captured_header.clone();

    async fn signed_callback(
        headers: HeaderMap,
        axum::extract::State(captured): axum::extract::State<Arc<Mutex<Option<String>>>>,
    ) -> axum::Json<serde_json::Value> {
        let sig = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        *captured.lock().await = sig;
        axum::Json(json!({"message_id": "signed-1"}))
    }

    let callback_app = Router::new()
        .route("/callback", post(signed_callback))
        .with_state(captured_clone);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener binds");
    let callback_url = format!("http://{}/callback", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, callback_app).await;
    });

    let secret = "secret-outbound-key";
    let adapter = kanon_api::WebhookAdapter::new("signing_test", None, Some(callback_url))
        .expect("adapter constructs")
        .with_secret(secret);

    let resp = adapter.deliver(test_request()).await.expect("deliver succeeds");
    assert!(resp.success);

    let sig = captured_header.lock().await.clone().expect("signature header captured");
    assert!(sig.starts_with("sha256="), "expected sha256 prefix: {sig}");
}

/// Conversational messages ingested via adapter route through the LLM pipeline and deliver to the adapter.
#[tokio::test]
async fn pipeline_llm_conversational_turn_routes_to_outbound_delivery() {
    let dir = tempfile::tempdir().expect("temp dir");
    let supervisor = Arc::new(kanon_core::supervisor::Supervisor::new(
        Some(PathBuf::from(dir.path())),
        None,
    ));

    let adapter = Arc::new(RecordingAdapter::new());
    supervisor
        .adapters()
        .register(adapter.clone())
        .await
        .expect("built-in adapter registers");

    let (ingest_tx, ingest_rx) = tokio::sync::mpsc::channel(16);
    let mock_reply = "LLM conversational reply from kanon-core";
    let mock_provider = Arc::new(common::MockProvider::new(mock_reply));

    let state = kanon_api::ApiState::builder(supervisor.clone())
        .with_config_dir(PathBuf::from(dir.path()))
        .with_ingress(kanon_core::EventIngress::new(ingest_tx))
        .with_llm_provider(
            "kanon-core",
            mock_provider,
            kanon_api::default_agent_config("mock-model"),
        )
        .build();

    let agent = state.agent().expect("agent configured").clone();
    let engine = Arc::new(
        kanon_core::pipeline::PipelineEngine::new(supervisor.clone())
            .with_tool_router(Arc::new(kanon_llm::ToolRouter::from_arc(agent))),
    );
    let worker_handle = engine.clone().start_worker(ingest_rx);
    let dispatcher_handle = engine
        .clone()
        .start_outbound_dispatcher()
        .expect("dispatcher starts");

    let app: Router = app(state);

    let (status, body) = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/adapters/{BUILTIN_PLATFORM}/ingest"),
        Some(json!({
            "channel_id": "chan-llm-1",
            "sender_id": "user-42",
            "text": "Hello, how are you?",
            "event_id": "evt-llm-1",
        })),
    )
    .await;

    assert_eq!(status, 202);
    assert_eq!(body["accepted"], true);
    assert_eq!(body["event_id"], "evt-llm-1");

    // Wait for the pipeline worker to process the turn and dispatch the reply to the adapter
    let start = std::time::Instant::now();
    let mut deliveries = Vec::new();
    while start.elapsed() < Duration::from_secs(3) {
        deliveries = adapter.deliveries();
        if !deliveries.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(deliveries.len(), 1, "expected 1 outbound delivery to adapter");
    let delivery = &deliveries[0];
    assert_eq!(delivery.platform, BUILTIN_PLATFORM);
    assert_eq!(delivery.channel_id, "chan-llm-1");
    assert_eq!(delivery.recipient_id, "user-42");
    assert_eq!(delivery.event_id, "evt-llm-1");
    assert_eq!(delivery.segments.len(), 1);

    match &delivery.segments[0].segment {
        Some(Segment::Text(TextSegment { content })) => {
            assert_eq!(content, mock_reply);
        }
        other => panic!("expected Text segment, got {other:?}"),
    }

    worker_handle.abort();
    dispatcher_handle.abort();
}
