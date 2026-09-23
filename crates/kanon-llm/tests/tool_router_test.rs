//! Unit tests for ToolRouter state machine loop and Tool Calling mechanics.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use async_trait::async_trait;

use kanon_llm::gateway::types::{ChatRequest, ChatResponse, ToolCall};
use kanon_llm::gateway::LlmProvider;
use kanon_llm::memory::ConversationManager;
use kanon_llm::tool_router::{json_to_prost_struct, prost_struct_to_json, ToolHost, ToolRouter};
use kanon_llm::GatewayError;
use kanon_proto::v1::{
    tool_call_request, tool_call_response, PluginMeta, ToolCallRequest, ToolCallResponse, ToolMeta,
};

/// Mock LLM provider that simulates multi-turn reasoning and tool invocation responses.
struct MockLlmProvider {
    /// Invocations counter to simulate progression through reasoning turns.
    call_count: AtomicUsize,
    /// Scripted responses returned in order.
    responses: Vec<ChatResponse>,
}

impl MockLlmProvider {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            responses,
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    async fn chat(&self, _request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        if idx < self.responses.len() {
            Ok(self.responses[idx].clone())
        } else {
            Ok(ChatResponse {
                content: Some("Default fallback content".to_string()),
                tool_calls: vec![],
                finish_reason: Some("stop".to_string()),
                usage: None,
            })
        }
    }
}

/// Mock plugin host simulating a child process with registered tools.
struct MockToolHost {
    host_id: String,
    plugins: Vec<PluginMeta>,
    tool_calls_received: Arc<AtomicUsize>,
}

impl MockToolHost {
    fn new(host_id: &str, tools: Vec<ToolMeta>) -> Self {
        Self {
            host_id: host_id.to_string(),
            plugins: vec![PluginMeta {
                id: "mock.plugin.test".to_string(),
                name: "Mock Test Plugin".to_string(),
                version: "1.0.0".to_string(),
                author: "Test".to_string(),
                description: "Test".to_string(),
                commands: vec![],
                tools,
            }],
            tool_calls_received: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl ToolHost for MockToolHost {
    fn host_id(&self) -> &str {
        &self.host_id
    }

    fn plugin_metas(&self) -> &[PluginMeta] {
        &self.plugins
    }

    async fn call_tool(&self, req: ToolCallRequest) -> Result<ToolCallResponse, tonic::Status> {
        self.tool_calls_received.fetch_add(1, Ordering::SeqCst);

        if req.tool_name == "add_numbers" {
            let structured_args = match req.payload {
                Some(tool_call_request::Payload::StructuredArgs(s)) => prost_struct_to_json(s),
                _ => serde_json::Value::Null,
            };

            let a = structured_args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = structured_args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let sum = a + b;

            let result_json = serde_json::json!({ "sum": sum });
            let result_struct = json_to_prost_struct(&result_json).unwrap();

            Ok(ToolCallResponse {
                call_id: req.call_id,
                success: true,
                error_message: String::new(),
                payload: Some(tool_call_response::Payload::StructuredResult(result_struct)),
            })
        } else {
            Err(tonic::Status::not_found(format!("Unknown tool: {}", req.tool_name)))
        }
    }
}

#[tokio::test]
async fn test_tool_router_direct_text_no_tools() {
    let mock_provider = Arc::new(MockLlmProvider::new(vec![ChatResponse {
        content: Some("Hello! How can I assist you today?".to_string()),
        tool_calls: vec![],
        finish_reason: Some("stop".to_string()),
        usage: None,
    }]));

    let memory = Arc::new(ConversationManager::new(10));
    let router = ToolRouter::new(mock_provider, memory.clone(), "test-model");

    let hosts: Vec<Arc<MockToolHost>> = vec![];
    let output = router
        .execute("session_1", "Hello there", &hosts)
        .await
        .expect("Execution should succeed");

    assert_eq!(output.content, "Hello! How can I assist you today?");
    assert!(output.executed_tools.is_empty());

    // Verify conversation memory contains both user and assistant messages
    let history = memory.get_messages("session_1");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content.as_deref(), Some("Hello there"));
    assert_eq!(history[1].content.as_deref(), Some("Hello! How can I assist you today?"));
}

#[tokio::test]
async fn test_tool_router_successful_tool_loop() {
    // Turn 1: Model asks to call `add_numbers` with arguments { a: 15, b: 27 }
    let turn1 = ChatResponse {
        content: None,
        tool_calls: vec![ToolCall {
            id: "call_math_1".to_string(),
            name: "add_numbers".to_string(),
            arguments: serde_json::json!({ "a": 15.0, "b": 27.0 }),
        }],
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
    };

    // Turn 2: After receiving { sum: 42 }, model generates final reply
    let turn2 = ChatResponse {
        content: Some("The sum of 15 and 27 is 42.".to_string()),
        tool_calls: vec![],
        finish_reason: Some("stop".to_string()),
        usage: None,
    };

    let mock_provider = Arc::new(MockLlmProvider::new(vec![turn1, turn2]));
    let memory = Arc::new(ConversationManager::new(10));
    let router = ToolRouter::new(mock_provider, memory.clone(), "test-model");

    let tool_meta = ToolMeta {
        name: "add_numbers".to_string(),
        description: "Adds two floating numbers".to_string(),
        parameters: json_to_prost_struct(&serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            }
        })),
    };

    let host = Arc::new(MockToolHost::new("host_math", vec![tool_meta]));
    let hosts = vec![host.clone()];

    let output = router
        .execute("session_math", "What is 15 + 27?", &hosts)
        .await
        .expect("Tool loop execution should succeed");

    assert_eq!(output.content, "The sum of 15 and 27 is 42.");
    assert_eq!(output.executed_tools.len(), 1);
    assert_eq!(output.executed_tools[0].tool_name, "add_numbers");
    assert!(output.executed_tools[0].success);
    assert_eq!(host.tool_calls_received.load(Ordering::SeqCst), 1);

    // Verify complete memory sequence: User -> Assistant (with tool_calls) -> Tool -> Assistant
    let history = memory.get_messages("session_math");
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].content.as_deref(), Some("What is 15 + 27?"));
    assert!(history[1].tool_calls.is_some());
    assert_eq!(history[2].role, kanon_llm::gateway::Role::Tool);
    assert!(history[2].content.as_ref().unwrap().contains("\"sum\":42"));
    assert_eq!(history[3].content.as_deref(), Some("The sum of 15 and 27 is 42."));
}

#[tokio::test]
async fn test_tool_router_max_recursion_limit() {
    // Model keeps returning tool calls in an infinite loop
    let infinite_turn = ChatResponse {
        content: None,
        tool_calls: vec![ToolCall {
            id: "call_inf".to_string(),
            name: "add_numbers".to_string(),
            arguments: serde_json::json!({ "a": 1.0, "b": 1.0 }),
        }],
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
    };

    let mock_provider = Arc::new(MockLlmProvider::new(vec![
        infinite_turn.clone(),
        infinite_turn.clone(),
        infinite_turn.clone(),
    ]));

    let memory = Arc::new(ConversationManager::new(10));
    // Set max iterations = 2
    let router = ToolRouter::new(mock_provider, memory, "test-model").with_max_iterations(2);

    let tool_meta = ToolMeta {
        name: "add_numbers".to_string(),
        description: "Adds numbers".to_string(),
        parameters: None,
    };

    let host = Arc::new(MockToolHost::new("host_math", vec![tool_meta]));
    let hosts = vec![host.clone()];

    let output = router
        .execute("session_inf", "Keep calculating", &hosts)
        .await
        .expect("Execution should terminate gracefully without error");

    // Loop must halt after exactly 2 iterations
    assert_eq!(host.tool_calls_received.load(Ordering::SeqCst), 2);
    assert!(output.content.contains("recursion limit reached"));
}
