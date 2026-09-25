//! Model providers catalog and connectivity testing endpoints (`/api/v1/providers`).

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};

use kanon_llm::gateway::providers::{
    AnthropicMessagesProvider, OpenAiChatProvider, OpenAiResponsesProvider,
};
use kanon_llm::gateway::types::{ChatMessage, ChatRequest};
use kanon_llm::gateway::LlmProvider;

use crate::error::ApiError;
use crate::state::ApiState;

/// Registers the providers endpoints.
pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/providers", get(list_providers))
        .route("/api/v1/providers/test", post(test_provider))
}

/// Catalog response listing active and available providers and presets.
#[derive(Debug, Serialize)]
pub struct ProvidersCatalogResponse {
    /// Active provider configuration on this node.
    pub active: ActiveProviderInfo,
    /// Available wire protocols.
    pub available_protocols: Vec<ProtocolDescriptor>,
    /// Popular pre-configured provider templates.
    pub presets: Vec<ProviderPreset>,
}

/// Summary of the currently active model provider.
#[derive(Debug, Serialize)]
pub struct ActiveProviderInfo {
    /// Whether an LLM provider is loaded and available.
    pub configured: bool,
    /// Wire protocol (e.g. `openai`, `anthropic`).
    pub protocol: String,
    /// Default model identifier.
    pub model: String,
    /// Provider base URL if set.
    pub base_url: Option<String>,
    /// Whether an API credential is configured.
    pub api_key_configured: bool,
    /// Configured sampling temperature.
    pub temperature: Option<f32>,
    /// Configured maximum generation tokens.
    pub max_tokens: Option<u32>,
}

/// Protocol specification descriptor.
#[derive(Debug, Serialize)]
pub struct ProtocolDescriptor {
    /// Protocol identifier.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Standard base URL for this protocol.
    pub default_base_url: &'static str,
}

/// Pre-configured provider preset for quick configuration.
#[derive(Debug, Serialize)]
pub struct ProviderPreset {
    /// Preset identifier.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Protocol identifier.
    pub protocol: &'static str,
    /// Provider API base URL.
    pub base_url: &'static str,
    /// Recommended default model identifier.
    pub default_model: &'static str,
}

/// Request payload to test provider connectivity.
#[derive(Debug, Deserialize)]
pub struct TestProviderRequest {
    /// Protocol wire format: `openai`, `openai_responses`, or `anthropic` (defaults to active).
    pub protocol: Option<String>,
    /// Target base URL (defaults to active).
    pub base_url: Option<String>,
    /// API key credential (defaults to active).
    pub api_key: Option<String>,
    /// Model tag to query (defaults to active or `gpt-4o-mini`).
    pub model: Option<String>,
    /// Custom test prompt (defaults to "ping").
    pub prompt: Option<String>,
}

/// Response payload from provider connectivity and latency test.
#[derive(Debug, Serialize)]
pub struct TestProviderResponse {
    /// Test result: `ok` or `error`.
    pub status: &'static str,
    /// Measured round-trip latency in milliseconds.
    pub latency_ms: u64,
    /// Model queried.
    pub model: String,
    /// Assistant reply preview, when successful.
    pub reply: Option<String>,
    /// Human-readable error message, when failed.
    pub error: Option<String>,
}

/// Handler for `GET /api/v1/providers`.
async fn list_providers(State(state): State<ApiState>) -> Json<ProvidersCatalogResponse> {
    let base_url = std::env::var("KANON_LLM_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let api_key_configured = std::env::var("KANON_LLM_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    let protocol = std::env::var("KANON_LLM_PROTOCOL").unwrap_or_else(|_| "openai".to_string());

    let (configured, model, temperature, max_tokens) = if let Some(agent) = state.agent() {
        let cfg = agent.config();
        (true, cfg.default_model.clone(), cfg.temperature, cfg.max_tokens)
    } else {
        (
            false,
            std::env::var("KANON_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            None,
            None,
        )
    };

    let active = ActiveProviderInfo {
        configured,
        protocol,
        model,
        base_url,
        api_key_configured,
        temperature,
        max_tokens,
    };

    let available_protocols = vec![
        ProtocolDescriptor {
            id: "openai",
            name: "OpenAI / Compatible (v1/chat/completions)",
            default_base_url: "https://api.openai.com/v1",
        },
        ProtocolDescriptor {
            id: "openai_responses",
            name: "OpenAI Responses API (v1/responses)",
            default_base_url: "https://api.openai.com/v1",
        },
        ProtocolDescriptor {
            id: "anthropic",
            name: "Anthropic Claude (v1/messages)",
            default_base_url: "https://api.anthropic.com/v1",
        },
    ];

    let presets = vec![
        ProviderPreset {
            id: "openai",
            name: "OpenAI Official",
            protocol: "openai",
            base_url: "https://api.openai.com/v1",
            default_model: "gpt-4o-mini",
        },
        ProviderPreset {
            id: "anthropic",
            name: "Anthropic Claude",
            protocol: "anthropic",
            base_url: "https://api.anthropic.com/v1",
            default_model: "claude-3-5-sonnet-20241022",
        },
        ProviderPreset {
            id: "deepseek",
            name: "DeepSeek",
            protocol: "openai",
            base_url: "https://api.deepseek.com/v1",
            default_model: "deepseek-chat",
        },
        ProviderPreset {
            id: "ollama",
            name: "Ollama (Local)",
            protocol: "openai",
            base_url: "http://127.0.0.1:11434/v1",
            default_model: "llama3.2",
        },
        ProviderPreset {
            id: "vllm",
            name: "vLLM (Local / Server)",
            protocol: "openai",
            base_url: "http://127.0.0.1:8000/v1",
            default_model: "Qwen/Qwen2.5-7B-Instruct",
        },
        ProviderPreset {
            id: "openrouter",
            name: "OpenRouter",
            protocol: "openai",
            base_url: "https://openrouter.ai/api/v1",
            default_model: "openai/gpt-4o-mini",
        },
        ProviderPreset {
            id: "siliconflow",
            name: "SiliconFlow (硅基流动)",
            protocol: "openai",
            base_url: "https://api.siliconflow.cn/v1",
            default_model: "deepseek-ai/DeepSeek-V3",
        },
    ];

    Json(ProvidersCatalogResponse {
        active,
        available_protocols,
        presets,
    })
}

/// Handler for `POST /api/v1/providers/test`.
async fn test_provider(
    State(state): State<ApiState>,
    Json(payload): Json<TestProviderRequest>,
) -> Result<Json<TestProviderResponse>, ApiError> {
    let prompt = payload.prompt.unwrap_or_else(|| "ping".to_string());

    let (provider, model): (Arc<dyn LlmProvider>, String) = match (payload.protocol, payload.base_url) {
        (Some(proto), Some(url)) => {
            let model = payload
                .model
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            let key = payload.api_key.filter(|k| !k.trim().is_empty());
            let prov: Arc<dyn LlmProvider> = match proto.as_str() {
                "openai" | "openai_chat" => {
                    Arc::new(OpenAiChatProvider::new(url, key, model.clone()))
                }
                "openai_responses" => Arc::new(
                    OpenAiResponsesProvider::new(key.unwrap_or_default()).with_base_url(url),
                ),
                "anthropic" => {
                    Arc::new(AnthropicMessagesProvider::new(url, key, model.clone()))
                }
                other => {
                    return Err(ApiError::BadRequest(format!(
                        "Unsupported protocol '{other}'; expected openai, openai_responses or anthropic"
                    )));
                }
            };
            (prov, model)
        }
        _ => {
            if let Some(agent) = state.agent() {
                let model = payload
                    .model
                    .unwrap_or_else(|| agent.config().default_model.clone());
                (agent.provider().clone(), model)
            } else {
                return Err(ApiError::BadRequest(
                    "No LLM provider is configured on this node; please specify protocol and base_url to test".to_string(),
                ));
            }
        }
    };

    let start = Instant::now();
    let request = ChatRequest {
        model: model.clone(),
        messages: vec![ChatMessage::user(prompt)],
        tools: Vec::new(),
        temperature: Some(0.1),
        max_tokens: Some(32),
    };

    match tokio::time::timeout(std::time::Duration::from_secs(15), provider.chat(&request)).await {
        Ok(Ok(response)) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            Ok(Json(TestProviderResponse {
                status: "ok",
                latency_ms,
                model,
                reply: response.content,
                error: None,
            }))
        }
        Ok(Err(err)) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            Ok(Json(TestProviderResponse {
                status: "error",
                latency_ms,
                model,
                reply: None,
                error: Some(err.to_string()),
            }))
        }
        Err(_) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            Ok(Json(TestProviderResponse {
                status: "error",
                latency_ms,
                model,
                reply: None,
                error: Some("Provider connection timed out after 15 seconds".to_string()),
            }))
        }
    }
}
