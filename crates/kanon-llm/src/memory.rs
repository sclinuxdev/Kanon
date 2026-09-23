//! Lightweight in-memory conversation memory with sliding-window pruning.
//!
//! Stores conversational contexts indexed by `channel_id:sender_id`.
//! Employs a concurrent sharded `DashMap` to enable lock-free, zero-contention
//! concurrent reads and writes across simultaneous conversations.

use std::collections::VecDeque;
use dashmap::DashMap;

use crate::gateway::types::{ChatMessage, Role};

/// In-memory conversational state for an individual conversation session.
#[derive(Debug, Clone)]
pub struct SessionMemory {
    /// Optional dynamic system persona prompt configuring the model's behavior.
    system_prompt: Option<String>,
    /// Sliding window queue of conversational messages (User, Assistant, Tool).
    messages: VecDeque<ChatMessage>,
    /// Maximum count of messages retained before sliding window pruning kicks in.
    max_messages: usize,
}

impl SessionMemory {
    /// Creates a new `SessionMemory` with the specified sliding window capacity.
    pub fn new(max_messages: usize) -> Self {
        Self {
            system_prompt: None,
            messages: VecDeque::with_capacity(max_messages.min(64)),
            max_messages,
        }
    }

    /// Sets or updates the system persona prompt for this session.
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Returns the active system prompt, if any.
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    /// Appends a new message to the session history, automatically applying
    /// sliding window pruning if the message threshold is exceeded.
    pub fn push_message(&mut self, message: ChatMessage) {
        self.messages.push_back(message);
        self.prune();
    }

    /// Appends multiple messages in sequence.
    pub fn extend_messages(&mut self, messages: impl IntoIterator<Item = ChatMessage>) {
        for msg in messages {
            self.messages.push_back(msg);
        }
        self.prune();
    }

    /// Returns a full snapshot of the conversation messages, prepending
    /// the dynamic system persona prompt before historical messages.
    pub fn get_messages(&self) -> Vec<ChatMessage> {
        let mut result = Vec::with_capacity(self.messages.len() + 1);
        if let Some(ref prompt) = self.system_prompt {
            result.push(ChatMessage::system(prompt.clone()));
        }
        result.extend(self.messages.iter().cloned());
        result
    }

    /// Current number of historical messages stored (excluding the system prompt).
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Checks if the history is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Clears historical messages while keeping the system prompt intact.
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    /// Prunes messages from the front to maintain the sliding window limit.
    ///
    /// Design rationale: In multi-turn tool calling, pruning must not leave an orphaned
    /// `Role::Tool` message at the beginning of the context, as standard LLM APIs
    /// require every Tool message to follow an Assistant message with matching `tool_calls`.
    /// When popping beyond `max_messages`, we continue popping until the conversation starts
    /// cleanly on a User message boundary.
    fn prune(&mut self) {
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }

        // Drop any orphaned tool responses that lost their preceding assistant call.
        while let Some(front) = self.messages.front() {
            if front.role == Role::Tool {
                self.messages.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Global concurrent conversation memory manager.
///
/// Uses `channel_id:sender_id` composite keys stored in a concurrent lock-sharded
/// [`DashMap`]. Sessions can be queried and updated independently without
/// coarse-grained global lock contention.
pub struct ConversationManager {
    /// Sharded concurrent map storing per-session histories.
    sessions: DashMap<String, SessionMemory>,
    /// Default maximum message window size configured per session.
    default_max_messages: usize,
}

impl Default for ConversationManager {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_MESSAGES)
    }
}

impl ConversationManager {
    /// Default maximum count of historical messages kept per conversation window (~10 turns).
    pub const DEFAULT_MAX_MESSAGES: usize = 20;

    /// Creates a new `ConversationManager` with the specified default sliding window size.
    pub fn new(default_max_messages: usize) -> Self {
        Self {
            sessions: DashMap::new(),
            default_max_messages,
        }
    }

    /// Formats a standard Kanon composite session key from channel and sender IDs.
    pub fn make_session_key(channel_id: &str, sender_id: &str) -> String {
        format!("{channel_id}:{sender_id}")
    }

    /// Appends a message to the specified session history.
    pub fn push_message(&self, session_key: &str, message: ChatMessage) {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionMemory::new(self.default_max_messages))
            .push_message(message);
    }

    /// Sets or updates the system persona prompt for a given session.
    pub fn set_system_prompt(&self, session_key: &str, prompt: impl Into<String>) {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionMemory::new(self.default_max_messages))
            .set_system_prompt(prompt);
    }

    /// Retrieves a complete snapshot of conversation messages for a given session.
    pub fn get_messages(&self, session_key: &str) -> Vec<ChatMessage> {
        self.sessions
            .get(session_key)
            .map(|s| s.get_messages())
            .unwrap_or_default()
    }

    /// Clears conversation history for the specified session.
    pub fn clear(&self, session_key: &str) {
        self.sessions.remove(session_key);
    }

    /// Returns the number of active conversations currently tracked.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_conversation_manager_composite_key() {
        let mgr = ConversationManager::new(10);
        let key = ConversationManager::make_session_key("chan_1", "user_alice");

        mgr.push_message(&key, ChatMessage::user("Hello"));
        mgr.push_message(&key, ChatMessage::assistant("Hi Alice!"));

        let history = mgr.get_messages(&key);
        assert_eq!(history.len(), 2);
        assert_eq!(mgr.session_count(), 1);

        mgr.clear(&key);
        assert_eq!(mgr.session_count(), 0);
    }
}
