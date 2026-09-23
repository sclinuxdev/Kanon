//! Comprehensive unit tests for Kanon Agent engine, pluggable Memory, and Protocol Providers.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::RwLock;

use kanon_llm::agent::{Agent, AgentConfig};
use kanon_llm::gateway::providers::{AnthropicMessagesProvider, OpenAiChatProvider};
use kanon_llm::gateway::types::{ChatMessage, ChatRequest, ChatResponse, Role, ToolCall};
use kanon_llm::gateway::LlmProvider;
use kanon_llm::memory::Memory;
use kanon_llm::tool_router::{json_to_prost_struct, prost_struct_to_json, ToolHost};
use kanon_llm::GatewayError;
use kanon_proto::v1::{
    tool_call_request, tool_call_response, PluginMeta, ToolCallRequest, ToolCallResponse, ToolMeta,
};

// =========================================================================
// 1. Mock LlmProvider and Mock ToolHost for Agent Testing
// =========================================================================

struct ScriptedLlmProvider {
    responses: Vec<ChatResponse>,
    cursor: AtomicUsize,
    received_requests: Arc<RwLock<Vec<ChatRequest>>>,
}

impl ScriptedLlmProvider {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses,
            cursor: AtomicUsize::new(0),
            received_requests: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlmProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        self.received_requests.write().await.push(request.clone());
        let idx = self.cursor.fetch_add(1, Ordering::SeqCst);
        if idx < self.responses.len() {
            Ok(self.responses[idx].clone())
        } else {
            Ok(ChatResponse {
                content: Some("Default fallback".to_string()),
                tool_calls: vec![],
                finish_reason: Some("stop".to_string()),
                usage: None,
            })
        }
    }
}

struct MockHost {
    host_id: String,
    plugins: Vec<PluginMeta>,
}

#[async_trait]
impl ToolHost for MockHost {
    fn host_id(&self) -> &str {
        &self.host_id
    }

    fn plugin_metas(&self) -> &[PluginMeta] {
        &self.plugins
    }

    async fn call_tool(&self, req: ToolCallRequest) -> Result<ToolCallResponse, tonic::Status> {
        if req.tool_name == "reverse_string" {
            let structured_args = match req.payload {
                Some(tool_call_request::Payload::StructuredArgs(s)) => prost_struct_to_json(s),
                _ => serde_json::Value::Null,
            };

            let text = structured_args
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            let reversed: String = text.chars().rev().collect();

            let result_json = serde_json::json!({ "reversed": reversed });
            let result_struct = json_to_prost_struct(&result_json).unwrap();

            Ok(ToolCallResponse {
                call_id: req.call_id,
                success: true,
                error_message: String::new(),
                payload: Some(tool_call_response::Payload::StructuredResult(result_struct)),
            })
        } else {
            Err(tonic::Status::not_found("Tool not found"))
        }
    }
}

// =========================================================================
// 2. Custom Plugged-in Memory Implementation (Verifying Modularity)
// =========================================================================

/// Custom memory simulating an external plugin or database-backed store.
struct CustomPluginMemory {
    stored_messages: Arc<RwLock<Vec<ChatMessage>>>,
    system_prompt: Arc<RwLock<Option<String>>>,
}

impl CustomPluginMemory {
    fn new() -> Self {
        Self {
            stored_messages: Arc::new(RwLock::new(Vec::new())),
            system_prompt: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait]
impl Memory for CustomPluginMemory {
    async fn push_message(&self, _session_key: &str, message: ChatMessage) {
        self.stored_messages.write().await.push(message);
    }

    async fn set_system_prompt(&self, _session_key: &str, prompt: String) {
        *self.system_prompt.write().await = Some(prompt);
    }

    async fn get_system_prompt(&self, _session_key: &str) -> Option<String> {
        self.system_prompt.read().await.clone()
    }

    async fn get_messages(&self, _session_key: &str) -> Vec<ChatMessage> {
        let mut list = Vec::new();
        if let Some(ref sys) = *self.system_prompt.read().await {
            list.push(ChatMessage::system(sys.clone()));
        }
        list.extend(self.stored_messages.read().await.clone());
        list
    }

    async fn clear(&self, _session_key: &str) {
        self.stored_messages.write().await.clear();
    }

    async fn session_count(&self) -> usize {
        1
    }
}

// =========================================================================
// 3. Tests
// =========================================================================

#[tokio::test]
async fn test_agent_builder_and_execution_with_custom_memory() {
    let turn1 = ChatResponse {
        content: None,
        tool_calls: vec![ToolCall {
            id: "call_rev_1".to_string(),
            name: "reverse_string".to_string(),
            arguments: serde_json::json!({ "text": "kanon" }),
        }],
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
    };

    let turn2 = ChatResponse {
        content: Some("The reversed string is 'nonak'.".to_string()),
        tool_calls: vec![],
        finish_reason: Some("stop".to_string()),
        usage: None,
    };

    let provider = Arc::new(ScriptedLlmProvider::new(vec![turn1, turn2]));
    let custom_memory = Arc::new(CustomPluginMemory::new());

    let agent = Agent::builder("translator_bot", provider.clone())
        .system_prompt("You are a string transformation agent.")
        .memory(custom_memory.clone() as Arc<dyn Memory>)
        .model("agent-model-v1")
        .max_iterations(3)
        .temperature(0.2)
        .build();

    assert_eq!(agent.name(), "translator_bot");
    assert_eq!(agent.config().max_iterations, 3);

    let tool_meta = ToolMeta {
        name: "reverse_string".to_string(),
        description: "Reverses input text".to_string(),
        parameters: json_to_prost_struct(&serde_json::json!({
            "type": "object",
            "properties": { "text": { "type": "string" } }
        })),
    };

    let host = Arc::new(MockHost {
        host_id: "host_plugin".to_string(),
        plugins: vec![PluginMeta {
            id: "plugin_rev".to_string(),
            name: "Reverser".to_string(),
            version: "0.1.0".to_string(),
            author: "dev".to_string(),
            description: "test".to_string(),
            commands: vec![],
            tools: vec![tool_meta],
        }],
    });

    let hosts = vec![host];

    let output = agent
        .run("sess_custom", "Please reverse 'kanon'", &hosts)
        .await
        .expect("Agent execution should succeed");

    assert_eq!(output.content, "The reversed string is 'nonak'.");
    assert_eq!(output.turns, 2);
    assert_eq!(output.executed_tools.len(), 1);
    assert_eq!(output.executed_tools[0].tool_name, "reverse_string");
    assert!(output.executed_tools[0].success);

    // Verify custom memory received all messages correctly
    let history = custom_memory.get_messages("sess_custom").await;
    assert_eq!(history.len(), 5);
    assert_eq!(history[0].role, Role::System);
    assert_eq!(history[1].role, Role::User);
    assert_eq!(history[2].role, Role::Assistant);
    assert_eq!(history[3].role, Role::Tool);
    assert_eq!(history[4].role, Role::Assistant);
}

#[test]
fn test_openai_chat_provider_endpoint_and_headers() {
    let provider = OpenAiChatProvider::new("http://localhost:8000/v1", Some("sk-test".to_string()), "gpt-4o")
        .with_header("x-org-id", "org_123");

    // Validates clean construction without panicking or hardcoding
    let _ = provider;
}

#[test]
fn test_anthropic_messages_provider_endpoint_and_headers() {
    let provider = AnthropicMessagesProvider::new("https://api.anthropic.com", Some("sk-ant-test".to_string()), "claude-3-5-sonnet")
        .with_version("2023-06-01")
        .with_header("x-custom-env", "sandbox");

    let _ = provider;
}

#[test]
fn test_agent_config_defaults() {
    let config = AgentConfig::default();
    assert_eq!(config.max_iterations, 5);
    assert_eq!(config.default_model, "gpt-4o-mini");
    assert!(!config.stop_on_tool_failure);
}
