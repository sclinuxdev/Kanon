//! Tests for the built-in MCP client: handshake, tool discovery, invocation and policy.
//!
//! The fixture server is a POSIX shell script speaking JSON-RPC on stdio. A real MCP server would
//! pull a runtime into the test environment; this one exercises the exact contract the client
//! depends on (initialize, notifications/initialized, tools/list, tools/call) with no such cost.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kanon_core::instance::{InstanceDraft, InstanceRegistry, ItemPolicy};
use kanon_core::mcp::{
    McpConfigStore, McpPool, McpServer, McpServerConfig, McpTransport, host_id,
    qualified_tool_name, unqualified_tool_name,
};
use kanon_core::toggle::{MCP_SECTION, ToggleStore};
use kanon_llm::tool_router::ToolHost;
use kanon_proto::v1::ToolCallRequest;

/// Writes the fixture MCP server and returns its path.
///
/// The script extracts the request id from every line so the client's id matching is exercised for
/// real rather than being bypassed by a fixed id.
fn write_fixture_server(dir: &Path) -> PathBuf {
    let path = dir.join("fake_mcp_server.sh");
    std::fs::write(
        &path,
        r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"Echo a string","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}\n' "$id" ;;
    *'"tools/call"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"pong"}]}}\n' "$id" ;;
  esac
done
"#,
    )
    .expect("fixture server");

    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    path
}

/// Server definition pointing at the fixture script.
fn fixture_config(script: &Path, id: &str) -> McpServerConfig {
    McpServerConfig {
        id: id.to_string(),
        name: "Fake".to_string(),
        transport: McpTransport::Stdio {
            command: script.to_string_lossy().to_string(),
            args: Vec::new(),
            env: HashMap::new(),
        },
    }
}

#[tokio::test]
async fn connect_handshakes_and_exposes_qualified_tools() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = write_fixture_server(dir.path());
    let server = McpServer::new(fixture_config(&script, "fake"));

    server.connect().await.expect("handshake");

    let health = server.health().await;
    assert_eq!(health.state, "connected");
    assert_eq!(health.tools, 1);
    assert_eq!(health.failures, 0);
    assert_eq!(health.last_error, None);

    let metas = server.plugin_metas();
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].id, host_id("fake"));
    assert_eq!(metas[0].tools.len(), 1);
    // The model-facing name is namespaced by server so two servers may expose the same tool.
    assert_eq!(metas[0].tools[0].name, qualified_tool_name("fake", "echo"));
    assert_eq!(
        unqualified_tool_name("fake", &metas[0].tools[0].name).as_deref(),
        Some("echo")
    );

    // The full ToolHost entry point resolves the namespaced name back to the server's own tool.
    let response = server
        .call_tool(ToolCallRequest {
            call_id: "call-1".to_string(),
            tool_name: qualified_tool_name("fake", "echo"),
            session_id: String::new(),
            payload: None,
        })
        .await
        .expect("tool call");
    assert!(response.success, "{}", response.error_message);

    // A tool name belonging to another server is refused instead of being sent upstream.
    let foreign = server
        .call_tool(ToolCallRequest {
            call_id: "call-2".to_string(),
            tool_name: qualified_tool_name("other", "echo"),
            session_id: String::new(),
            payload: None,
        })
        .await
        .expect("tool call");
    assert!(!foreign.success);
    assert!(foreign.error_message.contains("does not belong"));
}

#[tokio::test]
async fn a_missing_server_records_the_concrete_failure() {
    let dir = tempfile::tempdir().expect("temp dir");
    let server = McpServer::new(McpServerConfig {
        id: "absent".to_string(),
        name: "Absent".to_string(),
        transport: McpTransport::Stdio {
            command: dir
                .path()
                .join("does-not-exist")
                .to_string_lossy()
                .to_string(),
            args: Vec::new(),
            env: HashMap::new(),
        },
    });

    let err = server.connect().await.err().expect("connect must fail");
    assert!(err.to_string().contains("does-not-exist"));

    // The console reads the health snapshot, so the cause must survive there rather than being
    // reported as a permanent "connecting".
    let health = server.health().await;
    assert_eq!(health.state, "reconnecting");
    assert_eq!(health.failures, 1);
    assert!(
        health
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("does-not-exist")),
        "expected the transport error, got {:?}",
        health.last_error
    );

    server.disconnect().await;
    assert_eq!(server.health().await.state, "disconnected");
}

/// Creates an instance with a policy for one MCP server.
async fn instance_with_policy(
    registry: &InstanceRegistry,
    server_id: &str,
    policy: ItemPolicy,
    adapter: &str,
) -> String {
    registry
        .create(InstanceDraft {
            name: format!("MCP Bot {adapter}"),
            enabled: true,
            // One enabled instance per adapter: the catalog refuses an adapter claimed twice.
            adapters: vec![adapter.to_string()],
            persona_id: None,
            system_prompt: None,
            model: None,
            plugins: Default::default(),
            skills: Default::default(),
            mcp: HashMap::from([(server_id.to_string(), policy)]),
        })
        .await
        .expect("create instance")
        .id
}

#[tokio::test]
async fn pool_honours_the_global_switch_and_the_instance_policy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = write_fixture_server(dir.path());

    let config = Arc::new(
        McpConfigStore::open(dir.path().join("mcp.json"))
            .await
            .expect("config store"),
    );
    config
        .upsert(fixture_config(&script, "fake"))
        .await
        .expect("upsert");

    let pool = Arc::new(McpPool::new());
    pool.sync_from_config(&config).await;
    let toggles = Arc::new(ToggleStore::in_memory());
    let registry = Arc::new(InstanceRegistry::default());

    // Node-wide switch off: no host is offered, even though the server would connect fine.
    toggles
        .set_enabled(MCP_SECTION, "fake", false)
        .await
        .expect("disable");
    assert!(
        pool.hosts_for_instance(&toggles, None).await.is_empty(),
        "a globally disabled server must not be offered"
    );

    toggles
        .set_enabled(MCP_SECTION, "fake", true)
        .await
        .expect("enable");
    let instance_id =
        instance_with_policy(&registry, "fake", ItemPolicy::Disable, "qqofficial").await;
    let instance = registry.get(&instance_id).await.expect("instance");
    assert!(
        pool.hosts_for_instance(&toggles, Some(&instance))
            .await
            .is_empty(),
        "an instance that disabled the server must not receive it"
    );

    // An explicit opt-in behaves like inherit: the global switch still decides.
    let enabled_id = instance_with_policy(&registry, "fake", ItemPolicy::Enable, "telegram").await;
    let enabled_instance = registry.get(&enabled_id).await.expect("instance");
    assert_eq!(
        pool.hosts_for_instance(&toggles, Some(&enabled_instance))
            .await
            .len(),
        1
    );

    // With no instance (console sandbox) the node-wide switch alone decides.
    assert_eq!(
        pool.hosts_for_instance(&toggles, None).await.len(),
        1,
        "with no instance the node-wide switch alone decides"
    );
}

/// The pool's connection is reused across calls rather than re-established per request.
#[tokio::test]
async fn a_connected_server_reuses_its_transport() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = write_fixture_server(dir.path());
    let server = McpServer::new(fixture_config(&script, "fake"));

    server.connect().await.expect("first connect");
    // A second connect must be a no-op: spawning a new child process per call would leak one
    // process per tool call.
    server.connect().await.expect("second connect");
    assert_eq!(server.health().await.state, "connected");
    assert_eq!(server.health().await.tools, 1);
}
