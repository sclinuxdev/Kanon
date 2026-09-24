//! Prometheus-compatible metrics exposition (`GET /api/v1/metrics`).

use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::routes::health::sample_gauges;
use crate::state::ApiState;

/// Content type mandated by the Prometheus text exposition format (`0.0.4`).
const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Registers the metrics endpoint.
pub fn routes() -> Router<ApiState> {
    Router::new().route("/api/v1/metrics", get(metrics))
}

/// Renders the registry as Prometheus text exposition.
///
/// Gauges are sampled from their owners on every scrape, while counters are read straight from
/// the shared atomics. The endpoint performs no aggregation work beyond formatting, so scraping
/// at a high frequency cannot measurably perturb pipeline throughput.
async fn metrics(State(state): State<ApiState>) -> Response {
    let gauges = sample_gauges(&state).await;
    let body = state.observability().metrics.render_prometheus(&gauges);

    ([(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)], body).into_response()
}
