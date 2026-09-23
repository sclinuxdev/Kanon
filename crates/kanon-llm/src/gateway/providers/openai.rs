//! OpenAI Chat Completions protocol implementation.
//!
//! Protocol-level client conforming to the industry-standard OpenAI `/chat/completions`
//! specification. Compatible with any provider or local engine adhering to this wire format
//! (e.g. OpenAI, DeepSeek, Ollama, vLLM, Groq, Mistral, Moonshot, Qwen).
//!
//! This module avoids hardcoding vendor names, domains, or proprietary endpoints.
//! All configuration is protocol-level: base URL, auth token/headers, model, and parameters.

use std::time::Duration;
use async_trait::async_trait;

use crate::error::GatewayError;
use crate::gateway::types::{ChatMessage, ChatRequest, ChatResponse, Role, TokenUsage, ToolCall};
use crate::gateway::LlmProvider;

/// Private wire structures representing the standard OpenAI Chat Completions JSON schema.
#[allow(dead_code)]
mod wire {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize)]
    pub struct OpenAiChatRequest<'a> {
        pub model: &'a str,
        pub messages: Vec<OpenAiMessageWire>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tools: Option<Vec<OpenAiToolWire>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub temperature: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub max_tokens: Option<u32>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OpenAiMessageWire {
        pub role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_calls: Option<Vec<OpenAiToolCallWire>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_call_id: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OpenAiToolWire {
        pub r#type: String, // "function"
        pub function: OpenAiFunctionWire,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OpenAiFunctionWire {
        pub name: String,
        pub description: String,
        pub parameters: serde_json::Value,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct OpenAiToolCallWire {
        pub id: String,
        pub r#type: String, // "function"
        pub function: OpenAiFunctionCallWire,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct OpenAiFunctionCallWire {
        pub name: String,
        /// OpenAI protocol encodes tool arguments as an escaped JSON string.
        pub arguments: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct OpenAiChatResponse {
        #[serde(default)]
        pub id: Option<String>,
        pub choices: Vec<OpenAiChoiceWire>,
        pub usage: Option<OpenAiUsageWire>,
    }

    #[derive(Debug, Deserialize)]
    pub struct OpenAiChoiceWire {
        pub index: usize,
        pub message: OpenAiResponseMessageWire,
        pub finish_reason: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    pub struct OpenAiResponseMessageWire {
        pub role: String,
        pub content: Option<String>,
        #[serde(default)]
        pub tool_calls: Option<Vec<OpenAiToolCallWire>>,
    }

    #[derive(Debug, Deserialize)]
    pub struct OpenAiUsageWire {
        pub prompt_tokens: u32,
        pub completion_tokens: u32,
        pub total_tokens: u32,
    }
}

/// Generic, protocol-level HTTP client implementing the OpenAI Chat Completions API.
pub struct OpenAiChatProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: Option<String>,
    default_model: String,
    custom_headers: Vec<(String, String)>,
}

/// Backward-compatible type alias.
pub type OpenAiProvider = OpenAiChatProvider;

impl OpenAiChatProvider {
    /// Creates a new `OpenAiChatProvider` from a protocol base URL or full endpoint.
    ///
    /// # Arguments
    /// - `base_url`: Base URL or endpoint (e.g. `https://api.openai.com/v1` or `http://localhost:11434/v1`).
    ///   If the URL does not end with `/chat/completions`, it is automatically appended.
    /// - `api_key`: Optional Bearer authentication token.
    /// - `default_model`: Default model identifier.
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        default_model: impl Into<String>,
    ) -> Self {
        let raw_url = base_url.into();
        let trimmed = raw_url.trim_end_matches('/');
        let endpoint = if trimmed.ends_with("/chat/completions") {
            trimmed.to_string()
        } else {
            format!("{trimmed}/chat/completions")
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(10)
            .build()
            .unwrap_or_default();

        Self {
            client,
            endpoint,
            api_key,
            default_model: default_model.into(),
            custom_headers: Vec::new(),
        }
    }

    /// Appends a custom HTTP header to all outbound requests (useful for proxies or custom auth).
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_headers.push((key.into(), value.into()));
        self
    }

    /// Converts an internal domain `ChatMessage` into the wire format `OpenAiMessageWire`.
    fn map_message_to_wire(msg: &ChatMessage) -> wire::OpenAiMessageWire {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
        .to_string();

        let tool_calls = msg.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| wire::OpenAiToolCallWire {
                    id: c.id.clone(),
                    r#type: "function".to_string(),
                    function: wire::OpenAiFunctionCallWire {
                        name: c.name.clone(),
                        arguments: c.arguments.to_string(),
                    },
                })
                .collect()
        });

        wire::OpenAiMessageWire {
            role,
            content: msg.content.clone(),
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiChatProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        let model = if !request.model.is_empty() {
            &request.model
        } else {
            &self.default_model
        };

        let messages: Vec<wire::OpenAiMessageWire> = request
            .messages
            .iter()
            .map(Self::map_message_to_wire)
            .collect();

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| wire::OpenAiToolWire {
                        r#type: "function".to_string(),
                        function: wire::OpenAiFunctionWire {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        };

        let wire_req = wire::OpenAiChatRequest {
            model,
            messages,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };

        let mut req_builder = self.client.post(&self.endpoint).json(&wire_req);

        if let Some(ref key) = self.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {key}"));
        }

        for (k, v) in &self.custom_headers {
            req_builder = req_builder.header(k, v);
        }

        let resp = req_builder.send().await?;

        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(GatewayError::ApiStatus {
                status: status.as_u16(),
                message: err_body,
            });
        }

        let wire_resp: wire::OpenAiChatResponse = resp.json().await?;

        let choice = wire_resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| GatewayError::InvalidResponse("No choices returned in response".to_string()))?;

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let parsed_args = serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));

                ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments: parsed_args,
                }
            })
            .collect();

        let usage = wire_resp.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(ChatResponse {
            content: choice.message.content,
            tool_calls,
            finish_reason: choice.finish_reason,
            usage,
        })
    }
}
