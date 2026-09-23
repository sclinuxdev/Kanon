//! Plugin trait definition for Kanon out-of-process plugins.
//!
//! Provides the primary interface for defining plugin lifecycle hooks,
//! command dispatchers, pre-filters, and LLM tools.

use async_trait::async_trait;
use kanon_proto::v1::{
    CommandExecuteRequest, CommandExecuteResponse, PipelineEventRequest,
    PluginMeta, PreFilterResult, ToolCallRequest, ToolCallResponse,
};
use crate::context::PluginContext;

/// Result alias for plugin operations.
pub type PluginResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Core trait representing an out-of-process Kanon plugin.
///
/// Implementors specify their static metadata via [`meta`](Plugin::meta) and
/// override lifecycle and pipeline handlers as needed.
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Returns the static metadata descriptor of this plugin.
    fn meta(&self) -> PluginMeta;

    /// Invoked once when the plugin host is initialized and ready.
    async fn on_load(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
        Ok(())
    }

    /// Invoked during graceful shutdown before the host process terminates.
    async fn on_unload(&mut self) -> PluginResult<()> {
        Ok(())
    }

    /// Intercepts inbound events before command dispatch or LLM reasoning.
    async fn on_pre_filter(&self, _req: PipelineEventRequest) -> PluginResult<Option<PreFilterResult>> {
        Ok(None)
    }

    /// Executes a registered slash command.
    async fn on_execute_command(&self, req: CommandExecuteRequest) -> PluginResult<CommandExecuteResponse> {
        Ok(CommandExecuteResponse {
            success: true,
            replies: vec![],
            error_message: format!("Command '{}' executed by default stub handler", req.command),
        })
    }

    /// Executes a registered LLM tool call.
    async fn on_call_tool(&self, req: ToolCallRequest) -> PluginResult<ToolCallResponse> {
        Ok(ToolCallResponse {
            call_id: req.call_id,
            success: true,
            error_message: String::new(),
            payload: None,
        })
    }
}

