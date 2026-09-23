//! Demonstration Rust plugin for the Kanon microkernel.
//!
//! Provides an example out-of-process plugin that exposes static metadata,
//! responds to `GetPluginMeta`, and processes the `/rustcalc` command.

use kanon_sdk::prelude::message_segment::Segment;
use kanon_sdk::prelude::*;

/// Demonstration plugin implementation.
struct DemoPlugin;

#[async_trait]
impl Plugin for DemoPlugin {
    /// Returns the static metadata declaring plugin identity and supported commands.
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            id: "org.kanon.plugin.demo_rust".to_string(),
            name: "Demo Rust Plugin".to_string(),
            version: "0.1.0".to_string(),
            author: "Kanon Dev".to_string(),
            description: "Demonstration plugin written in Rust".to_string(),
            commands: vec![CommandMeta {
                name: "rustcalc".to_string(),
                description: "High-performance calculation command".to_string(),
                usage: "/rustcalc <expr>".to_string(),
                priority: 100,
            }],
            tools: vec![],
        }
    }

    /// Lifecycle hook called when the plugin is loaded by the host runner.
    async fn on_load(&mut self, ctx: &mut PluginContext) -> PluginResult<()> {
        println!(
            "Demo Rust Plugin initialized with data directory: {:?}",
            ctx.data_dir
        );
        Ok(())
    }

    /// Handles command execution for the `/rustcalc` command.
    async fn on_execute_command(
        &self,
        req: CommandExecuteRequest,
    ) -> PluginResult<CommandExecuteResponse> {
        let reply_text = if req.command == "rustcalc" {
            let expr = req.args.join(" ");
            format!("Rust calculation result for [{expr}]: 42 (fast-path)")
        } else {
            format!("Unknown command: {}", req.command)
        };

        let reply = MessageSegment {
            segment: Some(Segment::Text(TextSegment {
                content: reply_text,
            })),
        };

        Ok(CommandExecuteResponse {
            success: true,
            replies: vec![reply],
            error_message: String::new(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Launch plugin host listening on assigned socket.
    KanonHost::new(DemoPlugin).run().await?;
    Ok(())
}

