//! Slash command extraction, routing, and dispatching.
//!
//! Inspects inbound message text for slash command prefixes (e.g. `/rustcalc <expr>`),
//! resolves target plugins via metadata discovered during the `GetPluginMeta` handshake,
//! and dispatches execution to [`ManagedHost::execute_command`].

use std::sync::Arc;

use kanon_proto::v1::{
    CommandExecuteRequest, CommandExecuteResponse, CommandMeta, PipelineEventRequest,
};

use crate::supervisor::ManagedHost;

/// Resolved target destination for an executed slash command.
#[derive(Clone)]
pub struct MatchedCommand {
    /// Host process managing the plugin.
    pub host: Arc<ManagedHost>,
    /// Unique identifier of the target plugin.
    pub plugin_id: String,
    /// Command metadata discovered during initial handshake.
    pub meta: CommandMeta,
}

/// Router responsible for parsing and matching slash commands to registered plugin hosts.
pub struct CommandRouter;

impl CommandRouter {
    /// Parses an incoming text string to extract a slash command name and argument list.
    ///
    /// Returns `Some((command_name, arguments))` if the string starts with a slash `/`,
    /// or `None` if the text represents plain conversation or an empty command.
    ///
    /// # Examples
    /// - `"/rustcalc 2 + 2"` -> `Some(("rustcalc", vec!["2", "+", "2"]))`
    /// - `"   /weather beijing  "` -> `Some(("weather", vec!["beijing"]))`
    /// - `"/help"` -> `Some(("help", vec![]))`
    /// - `"hello kanon"` -> `None`
    /// - `"/"` -> `None`
    pub fn parse_command(text: &str) -> Option<(String, Vec<String>)> {
        let trimmed = text.trim();
        let stripped = trimmed.strip_prefix('/')?;
        let mut tokens = stripped.split_whitespace();
        let name = tokens.next()?;
        if name.is_empty() {
            return None;
        }

        let args = tokens.map(String::from).collect();
        Some((name.to_string(), args))
    }

    /// Resolves a parsed command name to a matching plugin host and command metadata.
    ///
    /// Inspects the metadata registered on each active [`ManagedHost`].
    /// If multiple plugins register identical command names, candidates are sorted
    /// by command priority ascending, host priority ascending, and finally `host_id`
    /// to guarantee deterministic routing.
    pub fn resolve(
        command_name: &str,
        hosts: &[Arc<ManagedHost>],
    ) -> Option<MatchedCommand> {
        let target_name = command_name.trim_start_matches('/');
        let mut candidates = Vec::new();

        for host in hosts {
            for plugin in &host.meta {
                for cmd in &plugin.commands {
                    if cmd.name.trim_start_matches('/') == target_name {
                        candidates.push((
                            cmd.priority,
                            host.priority,
                            &host.host_id,
                            host.clone(),
                            plugin.id.clone(),
                            cmd.clone(),
                        ));
                    }
                }
            }
        }

        // Sort candidates: lowest numerical priority value wins.
        candidates.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(b.2))
        });

        candidates
            .into_iter()
            .next()
            .map(|(_, _, _, host, plugin_id, meta)| MatchedCommand {
                host,
                plugin_id,
                meta,
            })
    }

    /// Dispatches a command execution request to the target plugin host.
    #[allow(clippy::result_large_err)]
    pub async fn dispatch(
        target: &MatchedCommand,
        args: Vec<String>,
        context: PipelineEventRequest,
    ) -> Result<CommandExecuteResponse, tonic::Status> {
        let req = CommandExecuteRequest {
            plugin_id: target.plugin_id.clone(),
            command: target.meta.name.clone(),
            args,
            context: Some(context),
        };

        target.host.execute_command(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanon_proto::v1::PluginMeta;
    use std::path::PathBuf;

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
        assert_eq!(res, Some(("rustcalc".to_string(), vec!["2".to_string(), "+".to_string(), "2".to_string()])));

        let res = CommandRouter::parse_command("   /weather   beijing   shanghai   ");
        assert_eq!(res, Some(("weather".to_string(), vec!["beijing".to_string(), "shanghai".to_string()])));

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
        let host1 = create_mock_host("host1", 500, vec![CommandMeta {
            name: "calc".to_string(),
            description: "Host 1 calc".to_string(),
            usage: "/calc <expr>".to_string(),
            priority: 200,
        }]);

        let host2 = create_mock_host("host2", 500, vec![CommandMeta {
            name: "calc".to_string(),
            description: "Host 2 calc".to_string(),
            usage: "/calc <expr>".to_string(),
            priority: 50,
        }]);

        let hosts = vec![host1.clone(), host2.clone()];
        let resolved = CommandRouter::resolve("calc", &hosts).expect("Should resolve");
        assert_eq!(resolved.host.host_id, "host2");
        assert_eq!(resolved.meta.priority, 50);

        let resolved_slash = CommandRouter::resolve("/calc", &hosts).expect("Should resolve with leading slash");
        assert_eq!(resolved_slash.host.host_id, "host2");

        let not_found = CommandRouter::resolve("unknown", &hosts);
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_resolve_host_priority_fallback() {
        let host1 = create_mock_host("host_low_priority", 600, vec![CommandMeta {
            name: "echo".to_string(),
            description: "Low priority host echo".to_string(),
            usage: "/echo <msg>".to_string(),
            priority: 100,
        }]);

        let host2 = create_mock_host("host_high_priority", 100, vec![CommandMeta {
            name: "echo".to_string(),
            description: "High priority host echo".to_string(),
            usage: "/echo <msg>".to_string(),
            priority: 100,
        }]);

        let hosts = vec![host1, host2];
        let resolved = CommandRouter::resolve("echo", &hosts).expect("Should resolve");
        assert_eq!(resolved.host.host_id, "host_high_priority");
    }
}
