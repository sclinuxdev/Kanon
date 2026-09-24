//! Health, metrics and routing-error coverage for the management gateway.

mod common;

use std::path::PathBuf;

use axum::Router;
use axum::http::Method;
use kanon_api::app;
use serde_json::Value;

use common::{empty_state, error_code, fixture_state, send_json, send_raw};

/// Health reports liveness, uptime and a memory footprint.
#[tokio::test]
async fn health_reports_liveness_and_resources() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = fixture_state(PathBuf::from(dir.path()), true).await;

    let app: Router = app(state);
    let (status, body) = send_json(&app, Method::GET, "/api/v1/health", None).await;

    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["llm_configured"], true);
    assert!(body["uptime_seconds"].as_u64().is_some());
    assert_eq!(body["plugins"]["hosts"], 1);
    assert_eq!(body["plugins"]["loaded"], 1);
    // The test binary runs on a real host, so the resident set size must be reported.
    assert!(
        body["memory"]["resident_bytes"].as_u64().unwrap_or(0) > 0,
        "resident memory must be sampled: {body}"
    );
}

/// Health stays `200` without a model provider, flagging chat as disabled instead of failing.
#[tokio::test]
async fn health_without_provider_reports_disabled_chat() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = empty_state(PathBuf::from(dir.path())).await;

    let app: Router = app(state);
    let (status, body) = send_json(&app, Method::GET, "/api/v1/health", None).await;

    assert_eq!(status, 200);
    assert_eq!(body["llm_configured"], false);
    assert_eq!(body["plugins"]["hosts"], 0);
}

/// Metrics are exported in Prometheus text exposition format.
#[tokio::test]
async fn metrics_render_prometheus_exposition() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = fixture_state(PathBuf::from(dir.path()), false).await;

    // Drive one observed pipeline stage so a counter becomes non-zero.
    state
        .observability()
        .events
        .publish(kanon_api::TraceEvent::SessionReset {
            session_id: "s-1".to_string(),
        });

    let app: Router = app(state);
    let (status, body, content_type) = send_raw(&app, Method::GET, "/api/v1/metrics").await;

    assert_eq!(status, 200);
    assert!(
        content_type.starts_with("text/plain; version=0.0.4"),
        "unexpected content type: {content_type}"
    );
    assert!(body.contains("# TYPE kanon_events_ingested_total counter"));
    assert!(body.contains("# TYPE kanon_uptime_seconds gauge"));
    assert!(body.contains("kanon_plugins_loaded 1"));
    assert!(body.contains("kanon_trace_events_total 1"));
    // Gauges must be rendered as numeric samples, never as placeholders.
    let uptime_line = body
        .lines()
        .find(|line| line.starts_with("kanon_uptime_seconds "))
        .expect("uptime sample");
    assert!(
        uptime_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u64>().ok())
            .is_some(),
        "uptime sample must be numeric: {uptime_line}"
    );
}

/// Unknown endpoints answer with the shared error envelope.
#[tokio::test]
async fn unknown_endpoint_returns_structured_404() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = empty_state(PathBuf::from(dir.path())).await;
    let app: Router = app(state);

    let (status, body): (_, Value) =
        send_json(&app, Method::GET, "/api/v1/does-not-exist", None).await;

    assert_eq!(status, 404);
    assert_eq!(error_code(&body), "not_found");
    assert!(body["error"]["message"].as_str().is_some());
}
