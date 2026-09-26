//! Model provider catalog, activation and connectivity testing (`/api/v1/providers`).
//!
//! # Why activation lives here
//! The console owns provider selection, but a provider is a *node* property: it drives the
//! conversational pipeline, `RequestLLM` from plugin hosts and the sandbox chat endpoint. Writing
//! it into browser storage would configure nothing but the browser, so `PUT /active` validates
//! the description, persists it to `data/system.json` and installs it on the running node
//! through the shared agent slot — no restart, and the very next message uses it.

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post, put};
use serde::{Deserialize, Serialize};

use kanon_llm::gateway::types::{ChatMessage, ChatRequest};
use kanon_llm::gateway::LlmProvider;

use crate::error::ApiError;
use crate::llm_config::LlmProviderConfig;
use crate::state::ApiState;

/// Registers the providers endpoints.
pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/providers", get(list_providers))
        .route(
            "/api/v1/providers/active",
            put(activate_provider).delete(clear_active_provider),
        )
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

/// Summary of the currently effective model provider.
#[derive(Debug, Serialize)]
pub struct ActiveProviderInfo {
    /// Whether an LLM provider is loaded and available.
    pub configured: bool,
    /// Where the effective provider comes from: `console`, `env` or `none`.
    ///
    /// Reported explicitly so an operator can tell a provider saved through the console apart
    /// from one supplied by the environment bootstrap.
    pub source: &'static str,
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

/// Request payload for `PUT /api/v1/providers/active`.
///
/// Mirrors the `KANON_LLM_*` environment variables one-for-one; `temperature` and `max_tokens`
/// are optional agent tuning that the environment bootstrap cannot express.
#[derive(Debug, Deserialize)]
pub struct ActivateProviderRequest {
    /// Wire protocol: `openai` (alias `openai_chat`), `openai_responses` or `anthropic`.
    pub protocol: String,
    /// Provider base URL, e.g. `https://api.deepseek.com/v1`.
    pub base_url: String,
    /// Default model identifier, e.g. `deepseek-chat`.
    pub model: String,
    /// Provider credential. Optional for local runtimes that need none.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Sampling temperature applied to the node's agent.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Maximum generation tokens applied to the node's agent.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// Response payload after a provider is activated or cleared.
#[derive(Debug, Serialize)]
pub struct ActivateProviderResponse {
    /// Whether the change was applied to the running node.
    pub applied: bool,
    /// Human-readable confirmation, or the reason nothing changed.
    pub message: String,
    /// Effective provider state after the call.
    pub active: ActiveProviderInfo,
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

/// Resolves the provider that is effective on this node right now.
///
/// Precedence: a provider persisted by the console wins over the `KANON_LLM_*` environment
/// bootstrap, because the console is how an operator changes the node after it started. When
/// neither exists the node reports `none` rather than inventing a default.
pub(crate) fn effective_provider(state: &ApiState) -> Result<(LlmProviderConfig, &'static str), ApiError> {
    let persisted = state
        .system_config()
        .load()
        .map_err(ApiError::Internal)?;

    if let Some(config) = persisted {
        return Ok((config, "console"));
    }

    match LlmProviderConfig::from_env() {
        Some(config) => Ok((config, "env")),
        None => Err(ApiError::Unavailable(
            "No LLM provider is configured on this node".to_string(),
        )),
    }
}

/// Renders what the node is actually using, plus where that provider came from.
///
/// The agent slot is the ground truth for *whether* the node can answer and with which tuning
/// values; the persisted/environment description supplies the provenance (protocol, base URL,
/// credential presence). The two can legitimately disagree — an agent can be injected
/// programmatically with no description at all (`source: "runtime"`), which is why neither source
/// alone is sufficient.
pub(crate) fn active_provider_info(state: &ApiState) -> Result<ActiveProviderInfo, ApiError> {
    let agent = state.agent();
    let configured = agent.is_some();

    let described = match effective_provider(state) {
        Ok(resolved) => Some(resolved),
        // "No description" is the expected state of a fresh node, not a request failure.
        Err(ApiError::Unavailable(_)) => None,
        Err(other) => return Err(other),
    };

    // Only tuning values are read from the agent: they reflect what was applied, not merely what
    // was described on disk.
    let tuning = agent.as_ref().map(|agent| {
        let cfg = agent.config();
        (cfg.default_model.clone(), cfg.temperature, cfg.max_tokens)
    });

    match described {
        Some((config, source)) => {
            let (model, temperature, max_tokens) = match tuning {
                Some((model, temperature, max_tokens)) => (model, temperature, max_tokens),
                None => (config.model.clone(), config.temperature, config.max_tokens),
            };
            let api_key_configured = config.has_api_key();

            Ok(ActiveProviderInfo {
                configured,
                source,
                protocol: config.protocol,
                model,
                base_url: Some(config.base_url),
                api_key_configured,
                temperature,
                max_tokens,
            })
        }
        None => {
            let (model, temperature, max_tokens) = match tuning {
                Some((model, temperature, max_tokens)) => (model, temperature, max_tokens),
                None => ("gpt-4o-mini".to_string(), None, None),
            };

            Ok(ActiveProviderInfo {
                configured,
                // An agent with no persisted or environment description was injected in-process;
                // reporting `console` or `env` here would misattribute it.
                source: if configured { "runtime" } else { "none" },
                protocol: "openai".to_string(),
                model,
                base_url: None,
                api_key_configured: false,
                temperature,
                max_tokens,
            })
        }
    }
}

/// Handler for `GET /api/v1/providers`.
async fn list_providers(State(state): State<ApiState>) -> Result<Json<ProvidersCatalogResponse>, ApiError> {
    let active = active_provider_info(&state)?;

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

    Ok(Json(ProvidersCatalogResponse {
        active,
        available_protocols,
        presets,
    }))
}

/// Handler for `PUT /api/v1/providers/active`.
///
/// Order of operations is *validate → persist → apply*: the provider client is built first so an
/// unreachable protocol or malformed URL never reaches disk, and the running node is only
/// reconfigured once the choice is durable. A node that fails the intermediate step keeps serving
/// its previous provider.
async fn activate_provider(
    State(state): State<ApiState>,
    Json(payload): Json<ActivateProviderRequest>,
) -> Result<Json<ActivateProviderResponse>, ApiError> {
    let config = LlmProviderConfig {
        protocol: payload.protocol.trim().to_lowercase(),
        base_url: payload.base_url.trim().to_string(),
        model: payload.model.trim().to_string(),
        // An explicit empty string means "no credential", which is the same thing as omitting it.
        api_key: payload.api_key.filter(|key| !key.trim().is_empty()),
        temperature: payload.temperature,
        max_tokens: payload.max_tokens,
    };

    let provider = config.resolve().map_err(ApiError::BadRequest)?;
    state
        .system_config()
        .save(&config)
        .map_err(ApiError::Internal)?;

    let agent = state.apply_llm_provider("kanon-core", provider, config.agent_config());

    tracing::info!(
        protocol = %config.protocol,
        base_url = %config.base_url,
        model = %agent.config().default_model,
        temperature = ?agent.config().temperature,
        max_tokens = ?agent.config().max_tokens,
        "LLM provider activated through the control plane; effective immediately"
    );

    Ok(Json(ActivateProviderResponse {
        applied: true,
        message: format!(
            "Provider '{}' activated; new messages use it immediately",
            config.model
        ),
        active: active_provider_info(&state)?,
    }))
}

/// Handler for `DELETE /api/v1/providers/active`.
///
/// Clearing removes the persisted provider and disables chat on the running node. The environment
/// bootstrap is deliberately *not* re-applied: it seeds a node at startup and must not resurrect
/// a provider the operator just removed.
async fn clear_active_provider(
    State(state): State<ApiState>,
) -> Result<Json<ActivateProviderResponse>, ApiError> {
    state.system_config().clear().map_err(ApiError::Internal)?;
    state.clear_llm_provider();

    tracing::info!("LLM provider cleared through the control plane; chat is now disabled");

    Ok(Json(ActivateProviderResponse {
        applied: true,
        message: "Provider cleared; chat completions and conversational routing are disabled"
            .to_string(),
        active: active_provider_info(&state)?,
    }))
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
            // Built through the shared factory so a protocol accepted at activation time is
            // accepted here too, with identical wire semantics.
            let provider = kanon_llm::build_provider(
                &proto,
                url,
                payload.api_key,
                model.clone(),
            )
            .map_err(ApiError::BadRequest)?;
            (provider, model)
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
