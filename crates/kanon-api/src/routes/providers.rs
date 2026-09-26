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
        .route("/api/v1/providers/models", post(fetch_models))
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
        },
        ProviderPreset {
            id: "anthropic",
            name: "Anthropic Claude",
            protocol: "anthropic",
            base_url: "https://api.anthropic.com/v1",
        },
        ProviderPreset {
            id: "deepseek",
            name: "DeepSeek",
            protocol: "openai",
            base_url: "https://api.deepseek.com/v1",
        },
        ProviderPreset {
            id: "ollama",
            name: "Ollama (Local)",
            protocol: "openai",
            base_url: "http://127.0.0.1:11434/v1",
        },
        ProviderPreset {
            id: "vllm",
            name: "vLLM (Local / Server)",
            protocol: "openai",
            base_url: "http://127.0.0.1:8000/v1",
        },
        ProviderPreset {
            id: "openrouter",
            name: "OpenRouter",
            protocol: "openai",
            base_url: "https://openrouter.ai/api/v1",
        },
        ProviderPreset {
            id: "siliconflow",
            name: "SiliconFlow (硅基流动)",
            protocol: "openai",
            base_url: "https://api.siliconflow.cn/v1",
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

/// Request payload to fetch available model tags from a provider endpoint.
#[derive(Debug, Deserialize)]
pub struct FetchModelsRequest {
    /// Wire protocol (e.g. `openai`, `anthropic`).
    #[serde(default)]
    pub protocol: Option<String>,
    /// Provider API base URL.
    pub base_url: String,
    /// Optional API key credential.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Response payload containing list of model IDs available on the provider.
#[derive(Debug, Serialize)]
pub struct FetchModelsResponse {
    /// Discovered model identifiers.
    pub models: Vec<String>,
}

/// Handler for `POST /api/v1/providers/models`.
async fn fetch_models(
    State(_state): State<ApiState>,
    Json(payload): Json<FetchModelsRequest>,
) -> Result<Json<FetchModelsResponse>, ApiError> {
    let protocol = payload.protocol.as_deref().unwrap_or("openai");
    let base_url = payload.base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        return Err(ApiError::BadRequest("base_url must not be empty".to_string()));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::Internal(format!("Failed to build HTTP client: {e}")))?;

    let models = if protocol == "anthropic" {
        // Query Anthropic models endpoint (GET /v1/models)
        let models_url = if base_url.ends_with("/models") {
            base_url
        } else if base_url.ends_with("/v1") {
            format!("{base_url}/models")
        } else {
            format!("{base_url}/v1/models")
        };
        let mut req = client.get(&models_url);
        if let Some(ref key) = payload.api_key {
            if !key.trim().is_empty() {
                req = req
                    .header("x-api-key", key.trim())
                    .header("anthropic-version", "2023-06-01");
            }
        }

        let resp = req.send().await.map_err(|e| {
            ApiError::Internal(format!("Failed to connect to Anthropic provider at {models_url}: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = if status == reqwest::StatusCode::UNAUTHORIZED {
                if payload.api_key.as_deref().unwrap_or("").trim().is_empty() {
                    format!("Anthropic provider returned HTTP 401 Unauthorized: API key is required but was not provided. Upstream: {text}")
                } else {
                    format!("Anthropic provider returned HTTP 401 Unauthorized: Authentication failed (check API key credentials). Upstream: {text}")
                }
            } else {
                format!("Anthropic provider returned HTTP {status}: {text}")
            };
            return Err(ApiError::Internal(msg));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ApiError::Internal(format!("Failed to parse models response JSON: {e}"))
        })?;

        extract_model_ids(&json)
    } else {
        // OpenAI-compatible /v1/models (compatible with DeepSeek, Ollama, SiliconFlow, vLLM, OpenRouter, etc.)
        let models_url = if base_url.ends_with("/models") {
            base_url
        } else if base_url.ends_with("/v1") {
            format!("{base_url}/models")
        } else {
            format!("{base_url}/v1/models")
        };

        let mut req = client.get(&models_url);
        if let Some(ref key) = payload.api_key {
            if !key.trim().is_empty() {
                req = req.bearer_auth(key.trim());
            }
        }

        let resp = req.send().await.map_err(|e| {
            ApiError::Internal(format!("Failed to connect to provider at {models_url}: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = if status == reqwest::StatusCode::UNAUTHORIZED {
                if payload.api_key.as_deref().unwrap_or("").trim().is_empty() {
                    format!("Provider returned HTTP 401 Unauthorized: API key is required but was not provided. Upstream: {text}")
                } else {
                    format!("Provider returned HTTP 401 Unauthorized: Authentication failed (check API key credentials). Upstream: {text}")
                }
            } else {
                format!("Provider returned HTTP {status}: {text}")
            };
            return Err(ApiError::Internal(msg));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            ApiError::Internal(format!("Failed to parse models response JSON: {e}"))
        })?;

        extract_model_ids(&json)
    };

    Ok(Json(FetchModelsResponse { models }))
}

/// Extracts model identifiers from either OpenAI standard `{ data: [{ id: ... }] }`
/// or Ollama `{ models: [{ name: ... }] }` payload formats.
fn extract_model_ids(json: &serde_json::Value) -> Vec<String> {
    let mut list = Vec::new();
    // OpenAI standard: { "data": [ { "id": "..." } ] }
    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|s| s.as_str()) {
                list.push(id.to_string());
            }
        }
    }
    // Ollama native: { "models": [ { "name": "..." } ] }
    if list.is_empty() {
        if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
            for item in models {
                if let Some(name) = item.get("name").and_then(|s| s.as_str()) {
                    list.push(name.to_string());
                } else if let Some(id) = item.get("id").and_then(|s| s.as_str()) {
                    list.push(id.to_string());
                }
            }
        }
    }
    list.sort();
    list.dedup();
    list
}
