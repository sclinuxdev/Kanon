//! Integration tests for embedded SQLite-backed conversation memory (`SqliteMemory`).

use kanon_llm::gateway::types::{ChatMessage, Role, ToolCall};
use kanon_llm::memory::Memory;
use kanon_llm::sqlite_memory::SqliteMemory;
use tempfile::tempdir;

#[tokio::test]
async fn test_sqlite_memory_in_memory_isolation_and_prompts() {
    let memory = SqliteMemory::open_in_memory(10).expect("Failed to initialize in-memory SQLite");

    // Configure distinct sessions
    memory
        .set_system_prompt("session_a", "System persona for A".to_string())
        .await;
    memory
        .set_system_prompt("session_b", "System persona for B".to_string())
        .await;

    memory
        .push_message("session_a", ChatMessage::user("Hello A"))
        .await;
    memory
        .push_message("session_b", ChatMessage::user("Hello B"))
        .await;

    // Verify session isolation
    assert_eq!(
        memory.get_system_prompt("session_a").await.as_deref(),
        Some("System persona for A")
    );
    assert_eq!(
        memory.get_system_prompt("session_b").await.as_deref(),
        Some("System persona for B")
    );

    let msgs_a = memory.get_messages("session_a").await;
    assert_eq!(msgs_a.len(), 2); // System prompt + User message
    assert_eq!(msgs_a[0].role, Role::System);
    assert_eq!(msgs_a[1].content.as_deref(), Some("Hello A"));

    let msgs_b = memory.get_messages("session_b").await;
    assert_eq!(msgs_b.len(), 2);
    assert_eq!(msgs_b[1].content.as_deref(), Some("Hello B"));

    assert_eq!(memory.session_count().await, 2);
}

#[tokio::test]
async fn test_sqlite_memory_file_persistence_and_reload() {
    let dir = tempdir().expect("Failed to create temporary directory");
    let db_path = dir.path().join("test_memory.db");

    // Phase 1: Write messages with tool calls and drop the memory instance
    {
        let memory = SqliteMemory::open(&db_path, 10).expect("Failed to open SQLite database");
        memory
            .set_system_prompt("sess_tools", "You are a weather bot.".to_string())
            .await;

        memory
            .push_message("sess_tools", ChatMessage::user("Check weather in Tokyo"))
            .await;

        let tool_call = ToolCall {
            id: "call_tokyo_001".to_string(),
            name: "get_weather".to_string(),
            arguments: serde_json::json!({ "city": "Tokyo", "units": "celsius" }),
        };
        memory
            .push_message(
                "sess_tools",
                ChatMessage::assistant_tool_calls(vec![tool_call], None),
            )
            .await;

        memory
            .push_message(
                "sess_tools",
                ChatMessage::tool_response("call_tokyo_001", r#"{"temp": 18, "condition": "Cloudy"}"#),
            )
            .await;

        memory
            .push_message(
                "sess_tools",
                ChatMessage::assistant("It is currently 18°C and cloudy in Tokyo."),
            )
            .await;
    }

    // Phase 2: Open a fresh SqliteMemory instance on the persisted database file
    {
        let reloaded = SqliteMemory::open(&db_path, 10).expect("Failed to reload SQLite database");

        assert_eq!(
            reloaded.get_system_prompt("sess_tools").await.as_deref(),
            Some("You are a weather bot.")
        );

        let messages = reloaded.get_messages("sess_tools").await;
        // Total messages: 1 system prompt + 1 user + 1 assistant tool call + 1 tool response + 1 assistant final = 5
        assert_eq!(messages.len(), 5);

        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);

        // Verify tool call round-trip deserialization
        assert_eq!(messages[2].role, Role::Assistant);
        let calls = messages[2].tool_calls.as_ref().expect("Tool calls missing");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_tokyo_001");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].arguments["city"], "Tokyo");

        // Verify tool response
        assert_eq!(messages[3].role, Role::Tool);
        assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_tokyo_001"));
        assert!(messages[3].content.as_deref().unwrap().contains("Cloudy"));

        // Verify final assistant message
        assert_eq!(messages[4].role, Role::Assistant);
        assert!(messages[4].content.as_deref().unwrap().contains("18°C"));
    }
}

#[tokio::test]
async fn test_sqlite_memory_sliding_window_pruning() {
    let dir = tempdir().expect("Failed to create temporary directory");
    let db_path = dir.path().join("pruning_memory.db");

    // Configure sliding window of 3 messages max
    let memory = SqliteMemory::open(&db_path, 3).expect("Failed to open SQLite database");
    memory
        .set_system_prompt("sess_prune", "Fixed System Prompt".to_string())
        .await;

    // Push 5 user messages
    for i in 1..=5 {
        memory
            .push_message("sess_prune", ChatMessage::user(format!("Message {i}")))
            .await;
    }

    // The in-memory cache and persisted SQLite should retain:
    // System Prompt + last 3 messages ("Message 3", "Message 4", "Message 5")
    let msgs = memory.get_messages("sess_prune").await;
    assert_eq!(msgs.len(), 4); // 1 system + 3 windowed
    assert_eq!(msgs[0].role, Role::System);
    assert_eq!(msgs[1].content.as_deref(), Some("Message 3"));
    assert_eq!(msgs[2].content.as_deref(), Some("Message 4"));
    assert_eq!(msgs[3].content.as_deref(), Some("Message 5"));

    // Reload from disk to verify SQLite was pruned synchronously
    let reloaded = SqliteMemory::open(&db_path, 3).expect("Failed to reload SQLite database");
    let disk_msgs = reloaded.get_messages("sess_prune").await;
    assert_eq!(disk_msgs.len(), 4);
    assert_eq!(disk_msgs[1].content.as_deref(), Some("Message 3"));
    assert_eq!(disk_msgs[3].content.as_deref(), Some("Message 5"));
}

#[tokio::test]
async fn test_sqlite_memory_token_budget_pruning() {
    // 40 token budget per session
    let memory = SqliteMemory::open_in_memory(50)
        .expect("Failed to open SQLite database")
        .with_token_budget(40);

    memory
        .set_system_prompt("sess_budget", "System".to_string())
        .await;

    // Push messages that will exceed the 40 token budget
    for i in 1..=10 {
        memory
            .push_message(
                "sess_budget",
                ChatMessage::user(format!("Long sentence payload token test number {i}")),
            )
            .await;
    }

    let estimated = memory.estimated_tokens("sess_budget").await;
    assert!(
        estimated <= 40,
        "Estimated tokens {estimated} should be within budget 40"
    );

    let msgs = memory.get_messages("sess_budget").await;
    // System prompt is retained
    assert_eq!(msgs[0].role, Role::System);
    // Recent messages are kept
    assert!(msgs.len() >= 2);
}

#[tokio::test]
async fn test_sqlite_memory_clear_and_session_count() {
    let memory = SqliteMemory::open_in_memory(10).expect("Failed to open SQLite database");

    memory.push_message("s1", ChatMessage::user("Hello 1")).await;
    memory.push_message("s2", ChatMessage::user("Hello 2")).await;

    assert_eq!(memory.session_count().await, 2);

    memory.clear("s1").await;
    assert_eq!(memory.session_count().await, 1);
    assert!(memory.get_messages("s1").await.is_empty());
    assert_eq!(memory.get_messages("s2").await.len(), 1);
}
