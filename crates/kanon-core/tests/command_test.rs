//! Tests for command parsing, priority resolution, and dispatching.

use std::path::PathBuf;
use std::sync::Arc;

use kanon_core::pipeline::CommandRouter;
use kanon_core::supervisor::ManagedHost;
use kanon_proto::v1::{CommandMeta, PluginMeta};

fn create_mock_host(host_id: &str, priority: i32, commands: Vec<CommandMeta>) -> Arc<ManagedHost> {
    let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:1").connect_lazy();
    let plugin_id = format!("org.kanon.plugin.{host_id}");
    let meta = vec![PluginMeta {
        id: plugin_id,
        name: host_id.to_string(),
        version: "1.0.0".to_string(),
        author: "Tester".to_string(),
        description: "Test host".to_string(),
        commands,
        tools: vec![],
    }];
    Arc::new(ManagedHost::new(
        host_id.to_string(),
        PathBuf::from(format!("/tmp/{host_id}.sock")),
        channel,
        meta,
        priority,
    ))
}

#[test]
fn test_parse_command_valid() {
    let res = CommandRouter::parse_command("/rustcalc 2 + 2");
    assert_eq!(
        res,
        Some((
            "rustcalc".to_string(),
            vec!["2".to_string(), "+".to_string(), "2".to_string()]
        ))
    );

    let res = CommandRouter::parse_command("   /weather   beijing   shanghai   ");
    assert_eq!(
        res,
        Some((
            "weather".to_string(),
            vec!["beijing".to_string(), "shanghai".to_string()]
        ))
    );

    let res = CommandRouter::parse_command("/ping");
    assert_eq!(res, Some(("ping".to_string(), vec![])));
}

#[test]
fn test_parse_command_invalid() {
    assert_eq!(CommandRouter::parse_command("hello world"), None);
    assert_eq!(CommandRouter::parse_command("/"), None);
    assert_eq!(CommandRouter::parse_command("   "), None);
    assert_eq!(CommandRouter::parse_command(""), None);
}

#[tokio::test]
async fn test_resolve_command_priority() {
    let host1 = create_mock_host(
        "host1",
        500,
        vec![CommandMeta {
            name: "calc".to_string(),
            description: "Host 1 calc".to_string(),
            usage: "/calc <expr>".to_string(),
            priority: 200,
        }],
    );

    let host2 = create_mock_host(
        "host2",
        500,
        vec![CommandMeta {
            name: "calc".to_string(),
            description: "Host 2 calc".to_string(),
            usage: "/calc <expr>".to_string(),
            priority: 50,
        }],
    );

    let hosts = vec![host1.clone(), host2.clone()];
    let resolved = CommandRouter::resolve("calc", &hosts).expect("Should resolve");
    assert_eq!(resolved.host.host_id, "host2");
    assert_eq!(resolved.meta.priority, 50);

    let resolved_slash =
        CommandRouter::resolve("/calc", &hosts).expect("Should resolve with leading slash");
    assert_eq!(resolved_slash.host.host_id, "host2");

    let not_found = CommandRouter::resolve("unknown", &hosts);
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_resolve_host_priority_fallback() {
    let host1 = create_mock_host(
        "host_low_priority",
        600,
        vec![CommandMeta {
            name: "echo".to_string(),
            description: "Low priority host echo".to_string(),
            usage: "/echo <msg>".to_string(),
            priority: 100,
        }],
    );

    let host2 = create_mock_host(
        "host_high_priority",
        100,
        vec![CommandMeta {
            name: "echo".to_string(),
            description: "High priority host echo".to_string(),
            usage: "/echo <msg>".to_string(),
            priority: 100,
        }],
    );

    let hosts = vec![host1, host2];
    let resolved = CommandRouter::resolve("echo", &hosts).expect("Should resolve");
    assert_eq!(resolved.host.host_id, "host_high_priority");
}
