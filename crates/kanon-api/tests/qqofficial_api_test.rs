//! Test coverage for QQ Official QR binding and credential polling endpoints.

mod common;

use axum::Router;
use axum::extract::Json;
use axum::http::Method;
use axum::routing::post;
use base64::Engine;
use common::{adapter_state, send_json};
use kanon_api::app;
use ring::aead::{AES_256_GCM, LessSafeKey, Nonce, UnboundKey};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Helper to encrypt secret with AES-256-GCM mimicking QQ OpenClaw backend.
fn encrypt_test_secret(secret: &str, bind_key_b64: &str) -> String {
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(bind_key_b64)
        .expect("decode bind key");
    let unbound = UnboundKey::new(&AES_256_GCM, &key_bytes).expect("unbound key");
    let key = LessSafeKey::new(unbound);

    let nonce_bytes = [7u8; 12];
    let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes).expect("nonce");

    let mut in_out = secret.as_bytes().to_vec();
    key.seal_in_place_append_tag(nonce, ring::aead::Aad::empty(), &mut in_out)
        .expect("seal in place");

    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&in_out);
    base64::engine::general_purpose::STANDARD.encode(combined)
}

#[tokio::test]
async fn qqofficial_qr_and_poll_flow_end_to_end() {
    // 1. Setup mock QQ auth server
    let mock_app = Router::new()
        .route(
            "/lite/create_bind_task",
            post(|Json(body): Json<Value>| async move {
                let key = body.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    Json(json!({
                        "retcode": 30001,
                        "msg": "key 不能为空"
                    }))
                } else {
                    Json(json!({
                        "retcode": 0,
                        "data": {
                            "task_id": "test-task-abc-123"
                        }
                    }))
                }
            }),
        )
        .route(
            "/lite/poll_bind_result",
            post(|Json(body): Json<Value>| async move {
                let task_id = body.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if task_id == "test-task-abc-123" {
                    Json(json!({
                        "retcode": 0,
                        "data": {
                            "status": 2,
                            "bot_appid": "102888999",
                            "bot_encrypt_secret": "REPLACE_ME_IN_HANDLER"
                        }
                    }))
                } else {
                    Json(json!({
                        "retcode": 0,
                        "data": {
                            "status": 1
                        }
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let bind_host = format!("127.0.0.1:{}", addr.port());

    tokio::spawn(async move {
        axum::serve(listener, mock_app).await.ok();
    });

    // 2. Setup Kanon API
    let dir = tempfile::tempdir().expect("temp dir");
    let (state, _ingest_rx, _adapter) = adapter_state(PathBuf::from(dir.path()), 8).await;
    let app: Router = app(state.clone());

    // 3. Request QR Code
    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/adapters/qqofficial/login/qr",
        Some(json!({
            "bind_host": &bind_host,
        })),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(body["task_id"], "test-task-abc-123");
    let bind_key = body["bind_key"]
        .as_str()
        .expect("bind_key string")
        .to_string();
    let qrcode_url = body["qrcode_url"].as_str().expect("qrcode_url string");
    assert!(qrcode_url.contains("test-task-abc-123"));

    // 4. Poll for credentials with mock encrypted secret
    let encrypted_secret = encrypt_test_secret("my_super_secret_token_123", &bind_key);

    // Setup updated mock that returns the dynamically encrypted secret for this bind_key
    let mock_poll_app = Router::new().route(
        "/lite/poll_bind_result",
        post({
            let enc = encrypted_secret.clone();
            move |Json(_body): Json<Value>| {
                let enc = enc.clone();
                async move {
                    Json(json!({
                        "retcode": 0,
                        "data": {
                            "status": 2,
                            "bot_appid": "102888999",
                            "bot_encrypt_secret": enc
                        }
                    }))
                }
            }
        }),
    );
    let poll_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("poll listener");
    let poll_addr = poll_listener.local_addr().expect("poll addr");
    let poll_bind_host = format!("127.0.0.1:{}", poll_addr.port());

    tokio::spawn(async move {
        axum::serve(poll_listener, mock_poll_app).await.ok();
    });

    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/v1/adapters/qqofficial/login/poll",
        Some(json!({
            "task_id": "test-task-abc-123",
            "bind_key": &bind_key,
            "bind_host": &poll_bind_host,
            "auto_save": true,
        })),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(body["status"], "created");
    assert_eq!(body["appid"], "102888999");
    assert_eq!(body["secret"], "my_super_secret_token_123");
    assert_eq!(body["saved"], true);

    // 5. Verify persistence in PluginConfigStore
    let stored = state
        .config_store()
        .load("org.kanon.adapter.qqofficial")
        .expect("load config");
    assert_eq!(stored["appid"], "102888999");
    assert_eq!(stored["secret"], "my_super_secret_token_123");
}
