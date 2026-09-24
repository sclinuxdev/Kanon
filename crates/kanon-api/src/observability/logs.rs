//! Structured log capture and broadcast for `/ws/v1/logs`.
//!
//! The layer is intentionally decoupled from any concrete exporter: it converts each
//! `tracing` event into a JSON-serializable [`LogRecord`] and hands it to a bounded
//! broadcast channel. Console subscribers filter server-side by level and plugin id so
//! that a busy core never floods a narrow client with irrelevant records.

use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::metrics::MetricsRegistry;

/// Severity ordering used by both the record envelope and subscriber-side filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Fine-grained execution tracing.
    Trace,
    /// Developer-facing diagnostic detail.
    Debug,
    /// Normal operational milestones.
    Info,
    /// Recoverable degradation requiring attention.
    Warn,
    /// Faults that changed an operation's outcome.
    Error,
}

impl LogLevel {
    /// Parses a case-insensitive level name (e.g. `warn`, `WARN`).
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(LogLevel::Trace),
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warn" | "warning" => Some(LogLevel::Warn),
            "error" => Some(LogLevel::Error),
            _ => None,
        }
    }

    /// Maps a `tracing` level onto the API level.
    pub fn from_tracing(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        }
    }

    /// Returns the lowercase name used on the wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    /// Returns `true` when this level is at least as severe as `minimum`.
    pub fn passes(&self, minimum: LogLevel) -> bool {
        *self >= minimum
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single structured log record delivered to console subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    /// Originating tracing target (usually the module path).
    pub target: String,
    /// Severity of the record.
    pub level: LogLevel,
    /// Rendered human-readable message.
    pub message: String,
    /// Plugin identifier when the emitting code declared a `plugin_id` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Host process identifier when the emitting code declared a `host_id` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    /// Additional structured fields captured verbatim.
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// Unix timestamp in milliseconds at capture time.
    pub timestamp_ms: u64,
}

/// Bounded fan-out channel for structured log records.
pub struct LogBroadcaster {
    sender: broadcast::Sender<LogRecord>,
    metrics: Arc<MetricsRegistry>,
}

impl LogBroadcaster {
    /// Creates a broadcaster with the given buffer depth.
    pub fn new(capacity: usize, metrics: Arc<MetricsRegistry>) -> Arc<Self> {
        // `broadcast::channel` panics on a zero capacity; clamp defensively so a
        // misconfigured deployment cannot abort the process at startup.
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Arc::new(Self { sender, metrics })
    }

    /// Subscribes a new console client to the log stream.
    pub fn subscribe(&self) -> broadcast::Receiver<LogRecord> {
        self.sender.subscribe()
    }

    /// Number of clients currently subscribed to the log stream.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Publishes a record, counting it and tolerating the absence of subscribers.
    pub fn publish(&self, record: LogRecord) {
        MetricsRegistry::incr(&self.metrics.log_records);
        // `send` fails only when no receiver is alive; an idle control plane is not an error.
        let _ = self.sender.send(record);
    }

    /// Builds the `tracing` layer that feeds this broadcaster.
    ///
    /// The layer is cheap to clone: it only holds an `Arc` to the shared broadcaster.
    pub fn layer(self: &Arc<Self>) -> LogLayer {
        LogLayer {
            broadcaster: self.clone(),
        }
    }
}

/// `tracing` layer capturing every event as a [`LogRecord`].
#[derive(Clone)]
pub struct LogLayer {
    broadcaster: Arc<LogBroadcaster>,
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();

        let mut visitor = RecordVisitor::default();
        event.record(&mut visitor);

        let record = LogRecord {
            target: metadata.target().to_string(),
            level: LogLevel::from_tracing(metadata.level()),
            message: visitor.message.unwrap_or_default(),
            plugin_id: visitor.plugin_id,
            host_id: visitor.host_id,
            fields: visitor.fields,
            timestamp_ms: current_timestamp_ms(),
        };

        self.broadcaster.publish(record);
    }
}

/// Current Unix timestamp in milliseconds, saturating at the epoch on clock skew.
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Field visitor flattening `tracing` event fields into JSON values.
#[derive(Default)]
struct RecordVisitor {
    message: Option<String>,
    plugin_id: Option<String>,
    host_id: Option<String>,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl RecordVisitor {
    /// Stores a field, lifting well-known correlation keys for fast server-side filtering.
    fn insert(&mut self, field: &Field, value: serde_json::Value) {
        match field.name() {
            "message" => {
                self.message = value.as_str().map(str::to_string);
            }
            "plugin_id" => {
                self.plugin_id = value.as_str().map(str::to_string);
                self.fields.insert("plugin_id".to_string(), value);
            }
            "host_id" => {
                self.host_id = value.as_str().map(str::to_string);
                self.fields.insert("host_id".to_string(), value);
            }
            name => {
                self.fields.insert(name.to_string(), value);
            }
        }
    }
}

impl Visit for RecordVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, serde_json::Value::String(value.to_string()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, serde_json::Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, serde_json::Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, serde_json::Value::Number(value.into()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let json = serde_json::Number::from_f64(value);
        self.insert(
            field,
            json.map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
        );
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        // `tracing` renders the `message` field through `record_debug` with `format_args!`.
        // Formatting with `{:?}` on `format_args!` yields the plain message text, so the
        // common path produces clean strings while remaining lossless for other fields.
        self.insert(field, serde_json::Value::String(format!("{value:?}")));
    }
}
