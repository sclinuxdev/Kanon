//! Ollama native API provider implementation.
//!
//! Connects directly to Ollama's native `/api/chat` endpoint.
//! Its request and response types are independent and completely decoupled from
//! OpenAI wire protocols.

use std::time::Duration;
use async_trait::async_trait;

use crate::error::GatewayError;
use crate::gateway::types::{ChatMessage, ChatRequest, ChatResponse, Role, ToolCall};
use crate::gateway::LlmProvider;

/// Private wire structures representing Ollama's native `/api/chat` endpoint format.
#[allow(dead_code)]
mod wire {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize)]
    pub struct OllamaChatRequest<'a> {
        pub model: &'a str,
        pub messages: Vec<OllamaMessageWire>,
        pub stream: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tools: Option<Vec<OllamaToolWire>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OllamaMessageWire {
        pub role: String,
        pub content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_calls: Option<Vec<OllamaToolCallWire>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OllamaToolWire {
        pub r#type: String,
        pub function: OllamaFunctionWire,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OllamaFunctionWire {
        pub name: String,
        pub description: String,
        pub parameters: serde_json::Value,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OllamaToolCallWire {
        pub function: OllamaFunctionCallWire,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct OllamaFunctionCallWire {
        pub name: String,
        /// Ollama native delivers tool arguments as parsed JSON value, not as escaped string.
        pub arguments: serde_json::Value,
    }

    #[derive(Debug, Deserialize)]
    pub struct OllamaChatResponse {
        pub model: String,
        pub message: OllamaMessageWire,
        #[serde(default)]
        pub done: bool,
        #[serde(default)]
        pub prompt_eval_count: Option<u32>,
        #[serde(default)]
        pub eval_count: Option<u32>,
    }
}

/// High-performance HTTP client for Ollama's native `/api/chat` endpoint.
pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
    default_model: String,
}

impl OllamaProvider {
    /// Creates a new `OllamaProvider`.
    ///
    /// # Arguments
    /// - `base_url`: Base URL of Ollama (defaults to `http://localhost:11434` if empty).
    /// - `default_model`: Default model identifier (e.g. `llama3.2`, `qwen2.5:7b`).
    pub fn new(base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        let mut base_url = base_url.into();
        if base_url.is_empty() {
            base_url = "http://localhost:11434".to_string();
        }
        let base_url = base_url.trim_end_matches('/').to_string();

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(5)
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url,
            default_model: default_model.into(),
        }
    }

    /// Converts internal domain `ChatMessage` into Ollama native `OllamaMessageWire`.
    fn map_message_to_wire(msg: &ChatMessage) -> wire::OllamaMessageWire {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
        .to_string();

        let content = msg.content.clone().unwrap_or_default();

        let tool_calls = msg.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| wire::OllamaToolCallWire {
                    function: wire::OllamaFunctionCallWire {
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    },
                })
                .collect()
        });

        wire::OllamaMessageWire {
            role,
            content,
            tool_calls,
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        let url = format!("{}/api/chat", self.base_url);

        let model = if !request.model.is_empty() {
            &request.model
        } else {
            &self.default_model
        };

        let messages: Vec<wire::OllamaMessageWire> = request
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
                    .map(|t| wire::OllamaToolWire {
                        r#type: "function".to_string(),
                        function: wire::OllamaFunctionWire {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        };

        let wire_req = wire::OllamaChatRequest {
            model,
            messages,
            stream: false,
            tools,
        };

        let resp = self.client.post(&url).json(&wire_req).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(GatewayError::ApiStatus {
                status: status.as_u16(),
                message: err_body,
            });
        }

        let wire_resp: wire::OllamaChatResponse = resp.json().await?;

        // Allocate unique call IDs if Ollama doesn't supply them
        let tool_calls = wire_resp
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(idx, tc)| ToolCall {
                id: format!("call_ollama_{idx}"),
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        let content = if wire_resp.message.content.is_empty() {
            None
        } else {
            Some(wire_resp.message.content)
        };

        let usage = match (wire_resp.prompt_eval_count, wire_resp.eval_count) {
            (Some(prompt), Some(eval)) => Some(crate::gateway::types::TokenUsage {
                prompt_tokens: prompt,
                completion_tokens: eval,
                total_tokens: prompt + eval,
            }),
            _ => None,
        };

        Ok(ChatResponse {
            content,
            tool_calls,
            finish_reason: Some("stop".to_string()),
            usage,
        })
    }
}
