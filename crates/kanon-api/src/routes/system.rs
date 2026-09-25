//! Node system configuration inspection endpoint (`GET /api/v1/system/config`).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;

use crate::state::ApiState;

/// Registers the system configuration route.
pub fn routes() -> Router<ApiState> {
    Router::new().route("/api/v1/system/config", get(system_config))
}

/// Comprehensive node and system configuration payload.
#[derive(Debug, Serialize)]
pub struct SystemConfigResponse {
    /// Gateway and microkernel version.
    pub version: String,
    /// Seconds elapsed since the node process started.
    pub uptime_seconds: u64,
    /// IPC socket path for the core kernel.
    pub ipc_socket_path: String,
    /// Runtime socket directory.
    pub run_dir: String,
    /// Persistent data directory.
    pub data_dir: String,
    /// Session memory sliding window depth.
    pub memory_window: usize,
    /// Platform webhook adapter status.
    pub webhook: WebhookConfigSection,
    /// LLM provider and agent configuration.
    pub llm: LlmConfigSection,
    /// Host operating system and architecture.
    pub environment: EnvironmentSection,
}

/// Webhook adapter configuration details.
#[derive(Debug, Serialize)]
pub struct WebhookConfigSection {
    /// Inbound platform identifier.
    pub platform: String,
    /// Whether outbound webhook delivery callback is configured.
    pub callback_configured: bool,
    /// Outbound callback URL.
    pub callback_url: Option<String>,
    /// Whether HMAC-SHA256 signature verification is active.
    pub signature_verification: bool,
}

/// LLM provider configuration details.
#[derive(Debug, Serialize)]
pub struct LlmConfigSection {
    /// Whether an LLM provider is active.
    pub configured: bool,
    /// Active protocol identifier.
    pub protocol: String,
    /// Default model tag.
    pub model: String,
    /// Base URL if configured.
    pub base_url: Option<String>,
    /// Whether an API key credential was supplied.
    pub api_key_configured: bool,
    /// Maximum tool reasoning loop iterations.
    pub max_iterations: usize,
    /// Sampling temperature if configured.
    pub temperature: Option<f32>,
    /// Max generation tokens limit if configured.
    pub max_tokens: Option<u32>,
}

/// Host environment details.
#[derive(Debug, Serialize)]
pub struct EnvironmentSection {
    /// Target operating system.
    pub os: &'static str,
    /// Target CPU architecture.
    pub arch: &'static str,
    /// Rust edition used to build the binary.
    pub rust_edition: &'static str,
}

/// Handler for `GET /api/v1/system/config`.
async fn system_config(State(state): State<ApiState>) -> Json<SystemConfigResponse> {
    let webhook_adapter = state.supervisor().adapters().get("webhook").await;
    let callback_url_env = std::env::var("KANON_WEBHOOK_CALLBACK_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let secret_env = std::env::var("KANON_WEBHOOK_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let webhook = WebhookConfigSection {
        platform: std::env::var("KANON_WEBHOOK_PLATFORM")
            .unwrap_or_else(|_| "webhook".to_string()),
        callback_configured: webhook_adapter
            .as_ref()
            .map(|w| w.is_connected())
            .unwrap_or(false)
            || callback_url_env.is_some(),
        callback_url: callback_url_env,
        signature_verification: secret_env.is_some(),
    };

    let base_url = std::env::var("KANON_LLM_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let api_key_configured = std::env::var("KANON_LLM_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    let protocol = std::env::var("KANON_LLM_PROTOCOL").unwrap_or_else(|_| "openai".to_string());

    let (configured, model, max_iterations, temperature, max_tokens) =
        if let Some(agent) = state.agent() {
            let cfg = agent.config();
            (
                true,
                cfg.default_model.clone(),
                cfg.max_iterations,
                cfg.temperature,
                cfg.max_tokens,
            )
        } else {
            (
                false,
                std::env::var("KANON_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
                5,
                None,
                None,
            )
        };

    let llm = LlmConfigSection {
        configured,
        protocol,
        model,
        base_url,
        api_key_configured,
        max_iterations,
        temperature,
        max_tokens,
    };

    Json(SystemConfigResponse {
        version: state.version().to_string(),
        uptime_seconds: state.started_at().elapsed().as_secs(),
        ipc_socket_path: state
            .supervisor()
            .core_sock_path()
            .to_string_lossy()
            .to_string(),
        run_dir: state.supervisor().run_dir().to_string_lossy().to_string(),
        data_dir: state.config_store().base_dir().to_string_lossy().to_string(),
        memory_window: 40,
        webhook,
        llm,
        environment: EnvironmentSection {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            rust_edition: "2024",
        },
    })
}
