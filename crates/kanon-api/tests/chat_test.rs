//! Sandbox chat completion coverage (JSON and Server-Sent Events).

mod common;

use std::path::PathBuf;

use axum::Router;
use axum::http::Method;
use kanon_api::app;
use serde_json::json;

use common::{empty_state, error_code, fixture_state, send_json};

/// Non-streaming completions return the agent answer and register the session.
#[tokio::test]
async fn chat_completion_returns_json_answer() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = fixture_state(PathBuf::from(dir.path()), true).await;
    let app: Router = app(state.clone());

    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({ "session_id": "webui:demo", "message": "hello" })),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["session_id"], "webui:demo");
    assert_eq!(body["content"], "fixture reply");
    assert_eq!(body["turns"], 1);
    assert_eq!(body["finish_reason"], "stop");
    assert_eq!(body["executed_tools"].as_array().map(Vec::len), Some(0));

    // The turn is recorded in session metadata, so the console sees it immediately.
    let metadata = state
        .sessions()
        .get_metadata("webui:demo")
        .expect("session recorded");
    assert_eq!(metadata.turn_count, 1);
    assert!(metadata.total_tokens_used > 0);
}

/// The tool-enabled branch runs the same turn without registered hosts.
#[tokio::test]
async fn chat_completion_with_tools_flag_succeeds_without_hosts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let without_provider: Router = app(empty_state(PathBuf::from(dir.path())).await);

    // Without a configured provider the endpoint must refuse rather than fall back.
    let (status, body) = send_json(
        &without_provider,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({ "session_id": "webui:demo", "message": "hello", "tools": true })),
    )
    .await;

    assert_eq!(status, 503);
    assert_eq!(error_code(&body), "unavailable");

    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), true).await);
    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({ "session_id": "webui:demo", "message": "hello", "tools": true })),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["content"], "fixture reply");
}

/// Streaming completions emit SSE delta and terminal frames.
#[tokio::test]
async fn chat_completion_streams_server_sent_events() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), true).await);

    let (status, body, content_type) = common::send_raw_json(
        &app,
        "/api/v1/chat/completions",
        json!({ "session_id": "webui:stream", "message": "stream please", "stream": true }),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content type: {content_type}"
    );
    assert!(body.contains(r#""type":"delta""#), "body: {body}");
    assert!(body.contains("fixture reply"), "body: {body}");
    assert!(body.contains(r#""type":"done""#), "body: {body}");
    assert!(body.contains(r#""finish_reason":"stop""#), "body: {body}");
}

/// Chat requests are validated before the agent is invoked.
#[tokio::test]
async fn chat_completion_validates_request() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), true).await);

    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({ "session_id": "  ", "message": "hello" })),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(error_code(&body), "bad_request");

    let (status, _) = send_json(
        &app,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({ "session_id": "webui:demo", "message": "   " })),
    )
    .await;
    assert_eq!(status, 400);

    // An unknown persona override must not silently fall back to the default persona.
    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({ "session_id": "webui:demo", "message": "hi", "persona_id": "wizard" })),
    )
    .await;
    assert_eq!(status, 404);
    assert_eq!(error_code(&body), "not_found");
}

/// A persona override applies to the session before the turn runs.
#[tokio::test]
async fn chat_completion_applies_persona_override() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = fixture_state(PathBuf::from(dir.path()), true).await;
    let app: Router = app(state.clone());

    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/chat/completions",
        Some(json!({
            "session_id": "webui:persona",
            "message": "hello",
            "persona_id": "concise"
        })),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["persona_id"], "concise");
    assert_eq!(
        state.sessions().get_persona("webui:persona").as_deref(),
        Some("concise")
    );
}
