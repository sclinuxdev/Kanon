//! WebSocket real-time channels (`/ws/v1/logs` and `/ws/v1/events`).
//!
//! Both channels share the same contract:
//! 1. the client connects with optional query-string filters;
//! 2. the gateway emits a `ready` frame echoing the effective filter;
//! 3. every subsequent frame is a JSON record (`log` or `trace`);
//! 4. the client may send `{"type":"filter", ...}` at any time to narrow or widen the stream;
//! 5. if a client falls behind the bounded broadcast buffer, the gateway emits an explicit
//!    `{"type":"lagged","skipped":N}` frame — dropped records are always accounted for, never
//!    silently skipped.
//!
//! Each connection is handled by a single `tokio::select!` task that owns both socket halves.
//! That keeps filter state local (no locks) and guarantees the read and write halves are
//! dropped together when either side closes.

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;

use crate::metrics::MetricsRegistry;
use crate::observability::{LogLevel, LogRecord, TraceRecord};
use crate::state::ApiState;

/// Registers both real-time WebSocket channels.
pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/ws/v1/logs", get(logs_socket))
        .route("/ws/v1/events", get(events_socket))
}

/// Query-string filters accepted by `/ws/v1/logs`.
#[derive(Debug, Default, Deserialize)]
pub struct LogQuery {
    /// Minimum severity to deliver (`trace`, `debug`, `info`, `warn`, `error`).
    pub level: Option<String>,
    /// Restrict delivery to records tagged with this plugin identifier.
    pub plugin_id: Option<String>,
}

/// Query-string filters accepted by `/ws/v1/events`.
#[derive(Debug, Default, Deserialize)]
pub struct EventQuery {
    /// Restrict delivery to events belonging to this session key.
    pub session_id: Option<String>,
    /// Comma-separated list of event kinds to deliver (e.g. `ingested,llm_request`).
    pub kind: Option<String>,
}

/// Client-supplied filter update frame accepted on both channels.
#[derive(Debug, Deserialize)]
struct FilterFrame {
    /// Frame discriminator; must be `filter`.
    #[serde(rename = "type")]
    kind: String,
    /// Minimum severity (logs channel only).
    #[serde(default)]
    level: Option<String>,
    /// Plugin identifier restriction (logs channel only).
    #[serde(default)]
    plugin_id: Option<String>,
    /// Session restriction (events channel only).
    #[serde(default)]
    session_id: Option<String>,
    /// Event kind restriction (events channel only).
    #[serde(default)]
    kind_filter: Option<String>,
}

/// Effective server-side log filter.
#[derive(Debug, Clone)]
struct LogFilter {
    minimum_level: LogLevel,
    plugin_id: Option<String>,
}

impl LogFilter {
    /// Returns `true` when a record satisfies the filter.
    fn matches(&self, record: &LogRecord) -> bool {
        if !record.level.passes(self.minimum_level) {
            return false;
        }

        match self.plugin_id.as_deref() {
            Some(plugin_id) => record.plugin_id.as_deref() == Some(plugin_id),
            None => true,
        }
    }

    /// Serializes the effective filter for the `ready` frame and filter acknowledgements.
    fn describe(&self) -> Value {
        json!({
            "level": self.minimum_level.as_str(),
            "plugin_id": self.plugin_id,
        })
    }
}

/// Effective server-side trace filter.
#[derive(Debug, Clone, Default)]
struct EventFilter {
    session_id: Option<String>,
    kinds: Vec<String>,
}

impl EventFilter {
    /// Returns `true` when a record satisfies the filter.
    ///
    /// Kind entries match either the coarse event kind (`pipeline`, `llm_request`, ...) or the
    /// fine-grained pipeline stage (`ingested`, `pre_filter_blocked`, ...).
    fn matches(&self, record: &TraceRecord) -> bool {
        if !self.kinds.is_empty()
            && !self
                .kinds
                .iter()
                .any(|kind| record.event.matches_kind(kind))
        {
            return false;
        }

        match self.session_id.as_deref() {
            Some(session_id) => record.event.session_id() == Some(session_id),
            None => true,
        }
    }

    /// Serializes the effective filter.
    fn describe(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "kind": self.kinds,
        })
    }
}

/// Upgrades a request to the log streaming channel.
async fn logs_socket(
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    Query(query): Query<LogQuery>,
) -> Response {
    let filter = match parse_log_filter(query.level.as_deref(), query.plugin_id) {
        Ok(filter) => filter,
        Err(message) => return crate::error::ApiError::BadRequest(message).into_response(),
    };

    ws.on_upgrade(move |socket| handle_logs(socket, state, filter))
}

/// Upgrades a request to the lifecycle trace channel.
async fn events_socket(
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    Query(query): Query<EventQuery>,
) -> Response {
    ws.on_upgrade(move |socket| handle_events(socket, state, EventFilter::from_query(query)))
}

/// Streams structured log records until the client disconnects.
async fn handle_logs(socket: WebSocket, state: ApiState, initial_filter: LogFilter) {
    let metrics = state.observability().metrics.clone();
    let broadcaster = state.observability().logs.clone();

    // Subscribe before the handshake frame so records emitted during the handshake are queued
    // in the broadcast buffer rather than lost between subscription and first poll.
    let mut receiver = broadcaster.subscribe();
    let (mut sink, mut stream) = socket.split();
    metrics.ws_opened();

    let mut filter = initial_filter;
    if send_frame(
        &mut sink,
        json!({
            "type": "ready",
            "channel": "logs",
            "filter": filter.describe(),
        }),
    )
    .await
    .is_err()
    {
        metrics.ws_closed();
        return;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if apply_log_filter_frame(&text, &mut filter, &mut sink).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if sink.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            record = receiver.recv() => {
                match record {
                    Ok(record) => {
                        if !filter.matches(&record) {
                            continue;
                        }
                        let frame = json!({ "type": "log", "record": record });
                        if send_frame(&mut sink, frame).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        // Surface the lag explicitly: a console must know its timeline has a gap.
                        MetricsRegistry::add(&metrics.log_records_dropped, skipped);
                        let frame = json!({ "type": "lagged", "skipped": skipped });
                        if send_frame(&mut sink, frame).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    let _ = sink.close().await;
    metrics.ws_closed();
}

/// Streams lifecycle trace records until the client disconnects.
async fn handle_events(socket: WebSocket, state: ApiState, initial_filter: EventFilter) {
    let metrics = state.observability().metrics.clone();
    let bus = state.observability().events.clone();

    let mut receiver = bus.subscribe();
    let (mut sink, mut stream) = socket.split();
    metrics.ws_opened();

    let mut filter = initial_filter;
    if send_frame(
        &mut sink,
        json!({
            "type": "ready",
            "channel": "events",
            "filter": filter.describe(),
        }),
    )
    .await
    .is_err()
    {
        metrics.ws_closed();
        return;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if apply_event_filter_frame(&text, &mut filter, &mut sink).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if sink.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            record = receiver.recv() => {
                match record {
                    Ok(record) => {
                        if !filter.matches(&record) {
                            continue;
                        }
                        let frame = json!({ "type": "trace", "record": record });
                        if send_frame(&mut sink, frame).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        MetricsRegistry::add(&metrics.log_records_dropped, skipped);
                        let frame = json!({ "type": "lagged", "skipped": skipped });
                        if send_frame(&mut sink, frame).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    let _ = sink.close().await;
    metrics.ws_closed();
}

/// Applies a client `filter` frame to the log filter, acknowledging the change.
async fn apply_log_filter_frame(
    text: &str,
    filter: &mut LogFilter,
    sink: &mut SplitSink<WebSocket, Message>,
) -> Result<(), ()> {
    let Some(frame) = parse_filter_frame(text, "logs") else {
        return Ok(());
    };

    match parse_log_filter(frame.level.as_deref(), frame.plugin_id) {
        Ok(updated) => {
            *filter = updated;
            send_frame(
                sink,
                json!({ "type": "filtered", "filter": filter.describe() }),
            )
            .await
        }
        Err(message) => send_frame(sink, json!({ "type": "error", "message": message })).await,
    }
}

/// Applies a client `filter` frame to the trace filter, acknowledging the change.
async fn apply_event_filter_frame(
    text: &str,
    filter: &mut EventFilter,
    sink: &mut SplitSink<WebSocket, Message>,
) -> Result<(), ()> {
    let Some(frame) = parse_filter_frame(text, "events") else {
        return Ok(());
    };

    *filter = EventFilter {
        session_id: frame.session_id.filter(|value| !value.trim().is_empty()),
        kinds: parse_kinds(frame.kind_filter.as_deref()),
    };

    send_frame(
        sink,
        json!({ "type": "filtered", "filter": filter.describe() }),
    )
    .await
}

/// Parses an incoming text frame into a filter update, ignoring unrelated frames.
///
/// Malformed JSON and non-filter frames are ignored rather than fatal: consoles may send
/// application-level pings or future frame types this gateway version does not know yet.
fn parse_filter_frame(text: &str, expected_channel: &str) -> Option<FilterFrame> {
    let frame: FilterFrame = serde_json::from_str(text).ok()?;
    if frame.kind != "filter" {
        tracing::debug!(
            channel = expected_channel,
            frame_type = %frame.kind,
            "Ignoring unsupported WebSocket control frame"
        );
        return None;
    }
    Some(frame)
}

/// Serializes and transmits one JSON frame.
async fn send_frame(sink: &mut SplitSink<WebSocket, Message>, frame: Value) -> Result<(), ()> {
    sink.send(Message::Text(frame.to_string()))
        .await
        .map_err(|_| ())
}

/// Builds a log filter from query parameters.
fn parse_log_filter(level: Option<&str>, plugin_id: Option<String>) -> Result<LogFilter, String> {
    let minimum_level = match level.map(str::trim).filter(|value| !value.is_empty()) {
        Some(raw) => LogLevel::parse(raw).ok_or_else(|| {
            format!("Unknown log level '{raw}'; expected trace, debug, info, warn or error")
        })?,
        None => LogLevel::Info,
    };

    Ok(LogFilter {
        minimum_level,
        plugin_id: plugin_id.filter(|value| !value.trim().is_empty()),
    })
}

impl EventFilter {
    /// Builds a trace filter from query parameters.
    fn from_query(query: EventQuery) -> Self {
        Self {
            session_id: query.session_id.filter(|value| !value.trim().is_empty()),
            kinds: parse_kinds(query.kind.as_deref()),
        }
    }
}

/// Splits a comma-separated kind list into normalized entries.
fn parse_kinds(raw: Option<&str>) -> Vec<String> {
    raw.map(|value| {
        value
            .split(',')
            .map(|entry| entry.trim().to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect()
    })
    .unwrap_or_default()
}
