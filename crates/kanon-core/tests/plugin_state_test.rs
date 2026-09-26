//! Tests for the persisted plugin enable/disable state.

use kanon_core::PluginStateStore;

#[tokio::test]
async fn plugins_are_enabled_until_an_operator_says_otherwise() {
    let store = PluginStateStore::in_memory();

    // Absence means enabled: a freshly cloned plugin runs without an opt-in.
    assert!(store.is_enabled("org.kanon.plugin.fresh").await);
    assert!(store.disabled_ids().await.is_empty());

    assert!(
        store
            .set_enabled("org.kanon.plugin.fresh", false)
            .await
            .expect("disable")
    );
    assert!(!store.is_enabled("org.kanon.plugin.fresh").await);
    assert_eq!(
        store.disabled_ids().await,
        vec!["org.kanon.plugin.fresh".to_string()]
    );

    // Idempotence is reported so callers can skip stopping a host twice.
    assert!(
        !store
            .set_enabled("org.kanon.plugin.fresh", false)
            .await
            .expect("second disable")
    );
}

#[tokio::test]
async fn state_survives_a_restart_and_keeps_unrelated_keys() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plugins_state.json");

    // A document that already carries another component's setting.
    std::fs::write(&path, r#"{"version":1,"plugins":{},"future_setting":{"keep":true}}"#)
        .expect("seed document");

    let store = PluginStateStore::open(&path).await.expect("open store");
    store
        .set_enabled("org.kanon.plugin.weather", false)
        .await
        .expect("disable");

    let reopened = PluginStateStore::open(&path).await.expect("reopen store");
    assert!(!reopened.is_enabled("org.kanon.plugin.weather").await);
    assert!(reopened.is_enabled("org.kanon.plugin.other").await);

    let raw = std::fs::read_to_string(&path).expect("read document");
    assert!(
        raw.contains("future_setting"),
        "writing the plugin state must not discard sibling settings: {raw}"
    );
}

#[tokio::test]
async fn a_malformed_document_is_reported_instead_of_ignored() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plugins_state.json");
    std::fs::write(&path, "{ not json").expect("seed malformed document");

    // Silently enabling a plugin the operator disabled would be the worst possible recovery.
    let err = PluginStateStore::open(&path)
        .await
        .expect_err("malformed state must be rejected");
    assert!(err.contains("failed to parse"), "unexpected error: {err}");
}

#[tokio::test]
async fn an_in_memory_store_never_touches_the_filesystem() {
    let store = PluginStateStore::in_memory();
    assert!(store.path().is_none());

    store
        .set_enabled("org.kanon.plugin.x", false)
        .await
        .expect("disable without a path");
    assert!(!store.is_enabled("org.kanon.plugin.x").await);
}
