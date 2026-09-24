//! Plugin catalog, configuration and restart route coverage.

mod common;

use std::path::PathBuf;

use axum::Router;
use axum::http::Method;
use kanon_api::app;
use serde_json::json;

use common::{FIXTURE_HOST_ID, FIXTURE_PLUGIN_ID, error_code, fixture_state, send_json};

/// The catalog merges live host metadata with the static manifest declaration.
#[tokio::test]
async fn plugin_catalog_lists_hosts_and_plugins() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(&app, Method::GET, "/api/v1/plugins", None).await;

    assert_eq!(status, 200);
    assert_eq!(body["total"], 1);

    let host = &body["hosts"][0];
    assert_eq!(host["host_id"], FIXTURE_HOST_ID);
    assert_eq!(host["status"], "running");
    assert_eq!(host["runtime"], "rust");
    assert_eq!(host["priority"], 120);
    // Externally registered hosts carry no launch recipe, so they cannot be restarted.
    assert_eq!(host["restartable"], false);

    let plugin = &body["plugins"][0];
    assert_eq!(plugin["id"], FIXTURE_PLUGIN_ID);
    assert_eq!(plugin["version"], "2.1.0");
    assert_eq!(plugin["status"], "running");
    assert_eq!(plugin["commands"][0]["name"], "fixture");
    assert_eq!(plugin["tools"][0]["name"], "fixture_tool");
}

/// The catalog is empty (not an error) when no host is registered.
#[tokio::test]
async fn plugin_catalog_is_empty_without_hosts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(common::empty_state(PathBuf::from(dir.path())).await);

    let (status, body) = send_json(&app, Method::GET, "/api/v1/plugins", None).await;

    assert_eq!(status, 200);
    assert_eq!(body["total"], 0);
    assert_eq!(body["plugins"].as_array().map(Vec::len), Some(0));
}

/// Configuration reads expose the declared schema with defaults merged in.
#[tokio::test]
async fn plugin_config_returns_schema_and_defaults() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(
        &app,
        Method::GET,
        &format!("/api/v1/plugins/{FIXTURE_PLUGIN_ID}/config"),
        None,
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(body["plugin_id"], FIXTURE_PLUGIN_ID);
    assert_eq!(body["persisted"], false);
    assert_eq!(body["schema"]["required"][0], "api_key");
    assert_eq!(body["values"]["default_city"], "Beijing");
    assert_eq!(body["values"]["enable_cache"], true);
    // `api_key` has no default, so it must not be invented.
    assert!(body["values"].get("api_key").is_none());
}

/// Unknown plugins produce a structured `404`.
#[tokio::test]
async fn plugin_config_unknown_plugin_returns_404() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(
        &app,
        Method::GET,
        "/api/v1/plugins/org.kanon.plugin.missing/config",
        None,
    )
    .await;

    assert_eq!(status, 404);
    assert_eq!(error_code(&body), "not_found");
}

/// Schema violations are rejected before any host round trip happens.
#[tokio::test]
async fn plugin_config_rejects_schema_violations() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);
    let uri = format!("/api/v1/plugins/{FIXTURE_PLUGIN_ID}/config");

    let (status, body) = send_json(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "values": { "default_city": "Shanghai" } })),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(error_code(&body), "bad_request");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("api_key")),
        "error must name the missing property: {body}"
    );

    let (status, body) = send_json(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "values": { "api_key": "k", "enable_cache": "yes" } })),
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("enable_cache")),
        "error must name the mistyped property: {body}"
    );

    let (status, body) = send_json(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "values": { "api_key": "k", "surprise": 1 } })),
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("surprise")),
        "additionalProperties=false must reject unknown keys: {body}"
    );

    let (status, body) = send_json(&app, Method::PUT, &uri, Some(json!({ "values": 7 }))).await;
    assert_eq!(status, 400);
    assert!(body["error"]["message"].as_str().is_some());
}

/// A rejected configuration payload is never persisted to disk.
#[tokio::test]
async fn plugin_config_is_not_persisted_when_rejected() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config_dir = PathBuf::from(dir.path());
    let app: Router = app(fixture_state(config_dir.clone(), false).await);
    let uri = format!("/api/v1/plugins/{FIXTURE_PLUGIN_ID}/config");

    let (status, _) = send_json(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "values": { "api_key": "k", "unknown": true } })),
    )
    .await;
    assert_eq!(status, 400);

    assert!(
        !config_dir
            .join(FIXTURE_PLUGIN_ID)
            .join("config.json")
            .exists(),
        "validation failures must not leave a configuration file behind"
    );
}

/// A valid payload reaching an unreachable host surfaces as an upstream failure.
#[tokio::test]
async fn plugin_config_reports_upstream_failure() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config_dir = PathBuf::from(dir.path());
    let app: Router = app(fixture_state(config_dir.clone(), false).await);
    let uri = format!("/api/v1/plugins/{FIXTURE_PLUGIN_ID}/config");

    let (status, body) = send_json(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "values": { "api_key": "secret", "default_city": "Hangzhou" } })),
    )
    .await;

    // The fixture host listens on a closed port, so the reload RPC fails explicitly.
    assert_eq!(status, 502, "body: {body}");
    assert_eq!(error_code(&body), "upstream_error");

    // Persistence happens only after the host accepts the payload.
    assert!(
        !config_dir
            .join(FIXTURE_PLUGIN_ID)
            .join("config.json")
            .exists(),
        "a failed hot reload must not persist configuration"
    );
}

/// Restarting a host without a recorded launch recipe is a conflict, not a silent no-op.
#[tokio::test]
async fn plugin_restart_without_launch_spec_is_conflict() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(
        &app,
        Method::POST,
        &format!("/api/v1/plugins/{FIXTURE_PLUGIN_ID}/restart"),
        None,
    )
    .await;

    assert_eq!(status, 409);
    assert_eq!(error_code(&body), "conflict");
    assert_eq!(
        body["error"]["message"],
        format!(
            "Host '{FIXTURE_HOST_ID}' has no recorded launch specification and cannot be restarted"
        )
    );
}

/// Restarting an unknown plugin reports `404`.
#[tokio::test]
async fn plugin_restart_unknown_plugin_returns_404() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/plugins/org.kanon.plugin.missing/restart",
        None,
    )
    .await;

    assert_eq!(status, 404);
    assert_eq!(error_code(&body), "not_found");
}

/// Plugin identifiers that could escape the data sandbox are rejected outright.
#[tokio::test]
async fn plugin_config_rejects_path_traversal_identifier() {
    let dir = tempfile::tempdir().expect("temp dir");
    let app: Router = app(fixture_state(PathBuf::from(dir.path()), false).await);

    let (status, body) = send_json(
        &app,
        Method::GET,
        "/api/v1/plugins/..%2F..%2Fetc/config",
        None,
    )
    .await;

    // Either the router rejects the encoded path (404) or the store rejects the identifier (400);
    // both are explicit failures and neither may touch the filesystem outside the sandbox.
    assert!(
        status == 404 || status == 400,
        "expected explicit rejection, got {status}: {body}"
    );
}
