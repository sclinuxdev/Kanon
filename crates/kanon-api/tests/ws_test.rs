//! WebSocket real-time channel coverage (`/ws/v1/logs` and `/ws/v1/events`).
//!
//! These tests exercise the channels over a real loopback listener: `tokio-tungstenite` acts as
//! the console client, so the upgrade handshake, framing, filtering and fan-out are verified end
//! to end rather than through internal calls.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use kanon_api::observability::{LogLevel, LogRecord};
use kanon_api::{ApiServer, Observability, TraceEvent};
use kanon_core::PipelineStage;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use common::{FIXTURE_PLUGIN_ID, fixture_state};

/// Upper bound for any single frame read, so a broken channel fails instead of hanging.
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// A connected console client.
type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Starts the gateway on an ephemeral loopback port and returns its address with the shared hub.
///
/// Handing back the observability hub lets a test publish on the exact channels the server
/// broadcasts from, which is what a live pipeline or agent would do in production.
async fn start_gateway(config_dir: PathBuf) -> (String, Arc<Observability>) {
    let state = fixture_state(config_dir, true).await;
    let hub = state.observability().clone();

    let server = ApiServer::bind("127.0.0.1:0".parse().expect("addr"), state)
        .await
        .expect("gateway binds");
    let port = server.local_addr().port();

    tokio::spawn(async move {
        // The test owns the process lifetime; the server serves until the runtime shuts down.
        let _ = server.run(std::future::pending::<()>()).await;
    });

    (format!("127.0.0.1:{port}"), hub)
}

/// Connects a console client, retrying until the listener accepts connections.
async fn connect(addr: &str, path: &str) -> Client {
    let url = format!("ws://{addr}{path}");

    for _ in 0..50 {
        match connect_async(&url).await {
            Ok((socket, _)) => return socket,
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }

    panic!("failed to connect to {url}");
}

/// Reads the next JSON frame, asserting it arrives within the timeout.
async fn next_json(client: &mut Client) -> Value {
    let message = tokio::time::timeout(FRAME_TIMEOUT, client.next())
        .await
        .expect("frame within timeout")
        .expect("stream still open")
        .expect("frame decodes");

    let text = match message {
        Message::Text(text) => text.to_string(),
        other => panic!("expected text frame, received {other:?}"),
    };

    serde_json::from_str(&text).expect("frame is JSON")
}

/// Builds a structured log record for publication.
fn log_record(level: LogLevel, plugin_id: Option<&str>, message: &str) -> LogRecord {
    LogRecord {
        target: "kanon_test".to_string(),
        level,
        message: message.to_string(),
        plugin_id: plugin_id.map(str::to_string),
        host_id: None,
        fields: serde_json::Map::new(),
        timestamp_ms: 0,
    }
}

/// The log channel announces its effective filter on connect.
#[tokio::test]
async fn log_channel_announces_default_filter() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, _hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/logs").await;
    let ready = next_json(&mut client).await;

    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["channel"], "logs");
    assert_eq!(ready["filter"]["level"], "info");
    assert!(ready["filter"]["plugin_id"].is_null());

    let _ = client.close(None).await;
}

/// Server-side publication reaches a subscribed console.
#[tokio::test]
async fn log_channel_delivers_published_records() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/logs").await;
    assert_eq!(next_json(&mut client).await["type"], "ready");

    hub.logs.publish(log_record(
        LogLevel::Warn,
        Some(FIXTURE_PLUGIN_ID),
        "disk almost full",
    ));

    let frame = next_json(&mut client).await;
    assert_eq!(frame["type"], "log");
    assert_eq!(frame["record"]["message"], "disk almost full");
    assert_eq!(frame["record"]["level"], "warn");
    assert_eq!(frame["record"]["plugin_id"], FIXTURE_PLUGIN_ID);

    let _ = client.close(None).await;
}

/// Query-string level filters suppress records below the threshold.
#[tokio::test]
async fn log_channel_honours_level_query_filter() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/logs?level=error").await;
    let ready = next_json(&mut client).await;
    assert_eq!(ready["filter"]["level"], "error");

    // The info record is filtered out server-side; only the error record must arrive.
    hub.logs
        .publish(log_record(LogLevel::Info, None, "ignored"));
    hub.logs
        .publish(log_record(LogLevel::Error, None, "delivered"));

    let frame = next_json(&mut client).await;
    assert_eq!(frame["record"]["message"], "delivered");
    assert_eq!(frame["record"]["level"], "error");

    let _ = client.close(None).await;
}

/// Clients can update filters at runtime with a `filter` control frame.
#[tokio::test]
async fn log_channel_accepts_filter_frames() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/logs").await;
    assert_eq!(next_json(&mut client).await["type"], "ready");

    client
        .send(Message::Text(
            serde_json::json!({
                "type": "filter",
                "level": "debug",
                "plugin_id": FIXTURE_PLUGIN_ID,
            })
            .to_string(),
        ))
        .await
        .expect("filter frame sent");

    let ack = next_json(&mut client).await;
    assert_eq!(ack["type"], "filtered");
    assert_eq!(ack["filter"]["level"], "debug");
    assert_eq!(ack["filter"]["plugin_id"], FIXTURE_PLUGIN_ID);

    // A debug record from another plugin is filtered out; the fixture plugin's record arrives.
    hub.logs.publish(log_record(
        LogLevel::Debug,
        Some("org.kanon.plugin.other"),
        "other plugin",
    ));
    hub.logs.publish(log_record(
        LogLevel::Debug,
        Some(FIXTURE_PLUGIN_ID),
        "fixture debug",
    ));

    let frame = next_json(&mut client).await;
    assert_eq!(frame["record"]["message"], "fixture debug");
    assert_eq!(frame["record"]["plugin_id"], FIXTURE_PLUGIN_ID);

    let _ = client.close(None).await;
}

/// An unparsable level is rejected during the HTTP upgrade.
#[tokio::test]
async fn log_channel_rejects_unknown_level() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, _hub) = start_gateway(PathBuf::from(dir.path())).await;

    let result = connect_async(format!("ws://{addr}/ws/v1/logs?level=verbose")).await;

    assert!(result.is_err(), "unknown log level must reject the upgrade");
}

/// The trace channel streams lifecycle events and honours kind filters.
#[tokio::test]
async fn event_channel_streams_lifecycle_stages() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/events?kind=ingested").await;
    let ready = next_json(&mut client).await;
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["channel"], "events");
    assert_eq!(ready["filter"]["kind"][0], "ingested");

    // A non-matching stage is dropped; the ingested stage is forwarded with its sequence number.
    hub.events.publish(TraceEvent::SessionReset {
        session_id: "webui:demo".to_string(),
    });
    hub.events
        .publish(TraceEvent::Pipeline(PipelineStage::Ingested {
            event_id: "evt-1".to_string(),
            platform: "discord".to_string(),
            channel_id: "chan-1".to_string(),
            sender_id: "user-1".to_string(),
        }));

    let frame = next_json(&mut client).await;
    assert_eq!(frame["type"], "trace");
    assert_eq!(frame["record"]["event"]["kind"], "pipeline");
    assert_eq!(frame["record"]["event"]["stage"], "ingested");
    assert_eq!(frame["record"]["event"]["event_id"], "evt-1");
    assert!(frame["record"]["seq"].as_u64().is_some());
    assert!(frame["record"]["timestamp_ms"].as_u64().is_some());

    let _ = client.close(None).await;
}

/// The coarse `pipeline` kind selects every lifecycle stage.
#[tokio::test]
async fn event_channel_accepts_coarse_pipeline_kind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/events?kind=pipeline,llm_request").await;
    let ready = next_json(&mut client).await;
    assert_eq!(ready["filter"]["kind"][0], "pipeline");
    assert_eq!(ready["filter"]["kind"][1], "llm_request");

    hub.events.publish(TraceEvent::SessionReset {
        session_id: "webui:demo".to_string(),
    });
    hub.events
        .publish(TraceEvent::Pipeline(PipelineStage::PreFilterBlocked {
            event_id: "evt-2".to_string(),
            host_id: "host-1".to_string(),
        }));

    let frame = next_json(&mut client).await;
    assert_eq!(frame["record"]["event"]["kind"], "pipeline");
    assert_eq!(frame["record"]["event"]["stage"], "pre_filter_blocked");
    assert_eq!(frame["record"]["event"]["host_id"], "host-1");

    let _ = client.close(None).await;
}

/// Session filters isolate a single conversation timeline.
#[tokio::test]
async fn event_channel_honours_session_filter() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (addr, hub) = start_gateway(PathBuf::from(dir.path())).await;

    let mut client = connect(&addr, "/ws/v1/events?session_id=webui:demo").await;
    let ready = next_json(&mut client).await;
    assert_eq!(ready["filter"]["session_id"], "webui:demo");

    hub.events.publish(TraceEvent::SessionReset {
        session_id: "other:session".to_string(),
    });
    hub.events.publish(TraceEvent::SessionReset {
        session_id: "webui:demo".to_string(),
    });

    let frame = next_json(&mut client).await;
    assert_eq!(frame["record"]["event"]["kind"], "session_reset");
    assert_eq!(frame["record"]["event"]["session_id"], "webui:demo");

    let _ = client.close(None).await;
}
