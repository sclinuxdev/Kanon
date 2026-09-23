//! Unified LLM Gateway client and provider abstractions.
//!
//! Exposes the [`LlmProvider`] trait for model backends, domain types in [`types`],
//! and protocol implementations in [`providers`].

pub mod providers;
pub mod types;

use std::pin::Pin;
use std::sync::Arc;
use async_trait::async_trait;
use tokio_stream::Stream;

use crate::error::GatewayError;
pub use providers::{
    AnthropicMessagesProvider, AnthropicProvider, OpenAiChatProvider, OpenAiProvider,
    OpenAiResponsesProvider, SseDecoder, SseEvent,
};
pub use types::{
    ChatChunk, ChatMessage, ChatRequest, ChatResponse, Role, TokenUsage, ToolCall, ToolDefinition,
};

/// Pinned, boxed stream of asynchronous chat completion chunks.
pub type ChatChunkStream = Pin<Box<dyn Stream<Item = Result<ChatChunk, GatewayError>> + Send>>;

/// Asynchronous trait defining interaction with an LLM backend.
///
/// Providers translate the unified [`ChatRequest`] domain format into their
/// wire representations, transmit the payload via non-blocking HTTP,
/// and map the response back into [`ChatResponse`] or stream [`ChatChunk`]s.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Executes a chat completion query against the underlying model backend.
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError>;

    /// Executes a streaming chat completion query against the underlying model backend.
    ///
    /// Default implementation wraps a non-streaming [`chat`] call into a two-chunk stream.
    async fn chat_stream(&self, request: &ChatRequest) -> Result<ChatChunkStream, GatewayError> {
        let resp = self.chat(request).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            if let Some(text) = resp.content {
                let _ = tx
                    .send(Ok(ChatChunk {
                        delta_text: text,
                        is_finished: false,
                        finish_reason: None,
                        tool_calls: resp.tool_calls.clone(),
                    }))
                    .await;
            }
            let _ = tx
                .send(Ok(ChatChunk {
                    delta_text: String::new(),
                    is_finished: true,
                    finish_reason: resp.finish_reason,
                    tool_calls: resp.tool_calls,
                }))
                .await;
        });
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

/// High-level LLM gateway managing default parameters and dispatching to a provider backend.
pub struct LlmGateway {
    /// The active backend provider implementation.
    provider: Arc<dyn LlmProvider>,
    /// Default model tag used when not explicitly specified in a request.
    default_model: String,
}

impl std::fmt::Debug for LlmGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmGateway")
            .field("default_model", &self.default_model)
            .finish()
    }
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

    /// Dispatches a streaming chat completion request to the active provider.
    pub async fn chat_stream(&self, request: &ChatRequest) -> Result<ChatChunkStream, GatewayError> {
        let mut req = request.clone();
        if req.model.is_empty() {
            req.model = self.default_model.clone();
        }
        self.provider.chat_stream(&req).await
    }
}

