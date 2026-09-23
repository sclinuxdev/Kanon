use std::sync::Arc;
use kanon_llm::gateway::types::{ChatMessage, Role};
use kanon_llm::memory::{Memory, SessionMemory, SlidingWindowMemory};

#[test]
fn test_session_sliding_window_pruning() {
    let mut session = SessionMemory::new(4);
    session.set_system_prompt("You are a helpful assistant.");

    session.push_message(ChatMessage::user("msg 1"));
    session.push_message(ChatMessage::assistant("reply 1"));
    session.push_message(ChatMessage::user("msg 2"));
    session.push_message(ChatMessage::assistant("reply 2"));

    assert_eq!(session.len(), 4);

    // Pushing a 5th message should prune the oldest message (msg 1)
    session.push_message(ChatMessage::user("msg 3"));
    assert_eq!(session.len(), 4);

    let msgs = session.get_messages();
    // System prompt is always prepended at index 0
    assert_eq!(msgs.len(), 5);
    assert_eq!(msgs[0].role, Role::System);
    assert_eq!(msgs[0].content.as_deref(), Some("You are a helpful assistant."));
    assert_eq!(msgs[1].content.as_deref(), Some("reply 1"));
    assert_eq!(msgs[4].content.as_deref(), Some("msg 3"));
}

#[tokio::test]
async fn test_memory_trait_interface() {
    let memory: Arc<dyn Memory> = Arc::new(SlidingWindowMemory::new(10));
    let key = SlidingWindowMemory::make_session_key("chan_1", "user_alice");

    memory.push_message(&key, ChatMessage::user("Hello")).await;
    memory.push_message(&key, ChatMessage::assistant("Hi Alice!")).await;

    let history = memory.get_messages(&key).await;
    assert_eq!(history.len(), 2);
    assert_eq!(memory.session_count().await, 1);

    memory.clear(&key).await;
    assert_eq!(memory.session_count().await, 0);
}
