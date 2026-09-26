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
                        reasoning_text: None,
                        is_finished: false,
                        finish_reason: None,
                        tool_calls: resp.tool_calls.clone(),
                    }))
                    .await;
            }
            let _ = tx
                .send(Ok(ChatChunk {
                    delta_text: String::new(),
                    reasoning_text: None,
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

/// Configured model provider and the model identifier it should default to.
pub type ProviderSetup = (Arc<dyn LlmProvider>, String);

/// Configures an [`LlmProvider`] and default model name from standard environment variables:
///
/// - `KANON_LLM_BASE_URL` — model provider base URL. If unset or empty, returns `Ok(None)`.
/// - `KANON_LLM_MODEL` — default model name (default: `gpt-4o-mini`).
/// - `KANON_LLM_API_KEY` — provider credential (optional).
/// - `KANON_LLM_PROTOCOL` — protocol wire format: `openai` (or `openai_chat`), `openai_responses`, or `anthropic`.
pub fn provider_from_env() -> Result<Option<ProviderSetup>, String> {
    let Ok(base_url) = std::env::var("KANON_LLM_BASE_URL") else {
        return Ok(None);
    };
    let base_url = base_url.trim().to_string();
    if base_url.is_empty() {
        return Ok(None);
    }

    let model = std::env::var("KANON_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let api_key = std::env::var("KANON_LLM_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let protocol = std::env::var("KANON_LLM_PROTOCOL").unwrap_or_else(|_| "openai".to_string());

    let provider: Arc<dyn LlmProvider> = match protocol.as_str() {
        "openai" | "openai_chat" => {
            Arc::new(OpenAiChatProvider::new(base_url, api_key, model.clone()))
        }
        "openai_responses" => Arc::new(
            OpenAiResponsesProvider::new(api_key.unwrap_or_default()).with_base_url(base_url),
        ),
        "anthropic" => Arc::new(AnthropicMessagesProvider::new(
            base_url,
            api_key,
            model.clone(),
        )),
        other => {
            return Err(format!(
                "Unsupported KANON_LLM_PROTOCOL '{other}'; expected openai, openai_responses or anthropic"
            ));
        }
    };

    Ok(Some((provider, model)))
}

