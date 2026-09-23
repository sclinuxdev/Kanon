//! Unified LLM Gateway client and provider abstractions.
//!
//! Exposes the [`LlmProvider`] trait for model backends, domain types in [`types`],
//! and protocol implementations in [`providers`].

pub mod providers;
pub mod types;

use std::sync::Arc;
use async_trait::async_trait;

use crate::error::GatewayError;
pub use providers::{
    AnthropicMessagesProvider, AnthropicProvider, OpenAiChatProvider, OpenAiProvider,
};
pub use types::{ChatMessage, ChatRequest, ChatResponse, Role, TokenUsage, ToolCall, ToolDefinition};

/// Asynchronous trait defining interaction with an LLM backend.
///
/// Providers translate the unified [`ChatRequest`] domain format into their
/// wire representations, transmit the payload via non-blocking HTTP,
/// and map the response back into [`ChatResponse`].
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Executes a chat completion query against the underlying model backend.
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError>;
}

/// High-level LLM gateway managing default parameters and dispatching to a provider backend.
pub struct LlmGateway {
    /// The active backend provider implementation.
    provider: Arc<dyn LlmProvider>,
    /// Default model tag used when not explicitly specified in a request.
    default_model: String,
}

impl LlmGateway {
    /// Creates a new `LlmGateway` instance wrapping the given provider.
    pub fn new(provider: Arc<dyn LlmProvider>, default_model: impl Into<String>) -> Self {
        Self {
            provider,
            default_model: default_model.into(),
        }
    }

    /// Returns a reference to the active provider implementation.
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Dispatches a chat completion request to the active provider.
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        let mut req = request.clone();
        if req.model.is_empty() {
            req.model = self.default_model.clone();
        }
        self.provider.chat(&req).await
    }
}
