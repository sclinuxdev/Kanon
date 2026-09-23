//! Modular conversation memory subsystem.
//!
//! Exposes the [`Memory`] trait for pluggable conversational memory implementations,
//! allowing plugins or developers to swap in SQLite, Redis, vector, or remote memories.
//!
//! Ships with [`SlidingWindowMemory`] (also aliased as [`ConversationManager`]) as the
//! default high-performance in-memory implementation backed by a lock-free sharded [`DashMap`].

use std::collections::VecDeque;
use std::sync::Arc;
use async_trait::async_trait;
use dashmap::DashMap;

use crate::gateway::types::{ChatMessage, Role};

/// Pluggable interface for conversational memory backends.
///
/// Implementations may store history in memory, relational databases, distributed caches,
/// or external memory microservices.
#[async_trait]
pub trait Memory: Send + Sync {
    /// Appends a message to the specified session history.
    async fn push_message(&self, session_key: &str, message: ChatMessage);

    /// Appends multiple messages in sequence.
    async fn extend_messages(&self, session_key: &str, messages: Vec<ChatMessage>) {
        for msg in messages {
            self.push_message(session_key, msg).await;
        }
    }

    /// Sets or updates the system persona prompt for a session.
    async fn set_system_prompt(&self, session_key: &str, prompt: String);

    /// Returns the active system prompt for a session if configured.
    async fn get_system_prompt(&self, session_key: &str) -> Option<String>;

    /// Retrieves a complete snapshot of conversation messages for a session (including system prompt).
    async fn get_messages(&self, session_key: &str) -> Vec<ChatMessage>;

    /// Clears conversation history for the specified session.
    async fn clear(&self, session_key: &str);

    /// Returns the count of active sessions tracked by this backend.
    async fn session_count(&self) -> usize;
}

/// In-memory conversational state for an individual conversation session.
#[derive(Debug, Clone)]
pub struct SessionMemory {
    /// Optional dynamic system persona prompt configuring the model's behavior.
    system_prompt: Option<String>,
    /// Sliding window queue of conversational messages (User, Assistant, Tool).
    messages: VecDeque<ChatMessage>,
    /// Maximum count of messages retained before sliding window pruning kicks in.
    max_messages: usize,
    /// Optional maximum cumulative token budget before pruning older messages.
    max_tokens_budget: Option<usize>,
}

impl SessionMemory {
    /// Creates a new `SessionMemory` with the specified sliding window capacity.
    pub fn new(max_messages: usize) -> Self {
        Self::with_budget(max_messages, None)
    }

    /// Creates a new `SessionMemory` with message limit and token budget.
    pub fn with_budget(max_messages: usize, max_tokens_budget: Option<usize>) -> Self {
        Self {
            system_prompt: None,
            messages: VecDeque::with_capacity(max_messages.min(64)),
            max_messages,
            max_tokens_budget,
        }
    }

    /// Sets or updates the token budget for this session.
    pub fn set_token_budget(&mut self, budget: Option<usize>) {
        self.max_tokens_budget = budget;
        self.prune();
    }

    /// Returns the active token budget if set.
    pub fn token_budget(&self) -> Option<usize> {
        self.max_tokens_budget
    }

    /// Computes the current estimated token utilization of this session.
    pub fn estimated_tokens(&self) -> usize {
        crate::token::estimate_conversation_tokens(&self.get_messages())
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

    /// Prunes messages from the front to maintain the sliding window limit and token budget.
    ///
    /// Design rationale: In multi-turn tool calling, pruning must not leave an orphaned
    /// `Role::Tool` message at the beginning of the context, as standard LLM APIs
    /// require every Tool message to follow an Assistant message with matching `tool_calls`.
    /// When popping beyond `max_messages` or `max_tokens_budget`, we continue popping until
    /// the conversation starts cleanly on a User message boundary.
    fn prune(&mut self) {
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }

        if let Some(budget) = self.max_tokens_budget {
            while !self.messages.is_empty() {
                let current_tokens = crate::token::estimate_conversation_tokens(&self.get_messages());
                if current_tokens <= budget {
                    break;
                }
                self.messages.pop_front();
            }
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

/// Default high-performance sliding window conversation memory manager.
///
/// Uses `channel_id:sender_id` composite keys stored in a concurrent lock-sharded
/// [`DashMap`]. Sessions can be queried and updated independently without
/// coarse-grained global lock contention.
pub struct SlidingWindowMemory {
    /// Sharded concurrent map storing per-session histories.
    sessions: DashMap<String, SessionMemory>,
    /// Default maximum message window size configured per session.
    default_max_messages: usize,
    /// Default maximum token budget per session.
    default_max_tokens: Option<usize>,
}

/// Backward-compatible alias for [`SlidingWindowMemory`].
pub type ConversationManager = SlidingWindowMemory;

impl Default for SlidingWindowMemory {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_MESSAGES)
    }
}

impl SlidingWindowMemory {
    /// Default maximum count of historical messages kept per conversation window (~10 turns).
    pub const DEFAULT_MAX_MESSAGES: usize = 20;

    /// Creates a new `SlidingWindowMemory` with the specified default sliding window size.
    pub fn new(default_max_messages: usize) -> Self {
        Self {
            sessions: DashMap::new(),
            default_max_messages,
            default_max_tokens: None,
        }
    }

    /// Sets default token budget for sessions managed by this memory store.
    pub fn with_token_budget(mut self, budget: usize) -> Self {
        self.default_max_tokens = Some(budget);
        self
    }

    /// Returns estimated token count for a given session.
    pub fn estimated_tokens(&self, session_key: &str) -> usize {
        self.sessions
            .get(session_key)
            .map(|s| s.estimated_tokens())
            .unwrap_or(0)
    }

    /// Formats a standard Kanon composite session key from channel and sender IDs.
    pub fn make_session_key(channel_id: &str, sender_id: &str) -> String {
        format!("{channel_id}:{sender_id}")
    }

    /// Synchronous method to append a message to the specified session history.
    pub fn push_message_sync(&self, session_key: &str, message: ChatMessage) {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionMemory::with_budget(self.default_max_messages, self.default_max_tokens))
            .push_message(message);
    }

    /// Synchronous method to retrieve messages for a session.
    pub fn get_messages_sync(&self, session_key: &str) -> Vec<ChatMessage> {
        self.sessions
            .get(session_key)
            .map(|s| s.get_messages())
            .unwrap_or_default()
    }

    /// Synchronous method to set system persona prompt for a given session.
    pub fn set_system_prompt_sync(&self, session_key: &str, prompt: impl Into<String>) {
        self.sessions
            .entry(session_key.to_string())
            .or_insert_with(|| SessionMemory::with_budget(self.default_max_messages, self.default_max_tokens))
            .set_system_prompt(prompt);
    }

    /// Synchronous method to clear session history.
    pub fn clear_sync(&self, session_key: &str) {
        self.sessions.remove(session_key);
    }

    /// Synchronous method to get session count.
    pub fn session_count_sync(&self) -> usize {
        self.sessions.len()
    }

    /// Appends a message to the specified session history.
    pub fn push_message(&self, session_key: &str, message: ChatMessage) {
        self.push_message_sync(session_key, message);
    }

    /// Retrieves messages for a session (including system prompt).
    pub fn get_messages(&self, session_key: &str) -> Vec<ChatMessage> {
        self.get_messages_sync(session_key)
    }

    /// Sets system persona prompt for a given session.
    pub fn set_system_prompt(&self, session_key: &str, prompt: impl Into<String>) {
        self.set_system_prompt_sync(session_key, prompt);
    }

    /// Clears session history.
    pub fn clear(&self, session_key: &str) {
        self.clear_sync(session_key);
    }

    /// Returns active session count.
    pub fn session_count(&self) -> usize {
        self.session_count_sync()
    }
}

#[async_trait]
impl Memory for SlidingWindowMemory {
    async fn push_message(&self, session_key: &str, message: ChatMessage) {
        self.push_message_sync(session_key, message);
    }

    async fn set_system_prompt(&self, session_key: &str, prompt: String) {
        self.set_system_prompt_sync(session_key, prompt);
    }

    async fn get_system_prompt(&self, session_key: &str) -> Option<String> {
        self.sessions
            .get(session_key)
            .and_then(|s| s.system_prompt().map(|p| p.to_string()))
    }

    async fn get_messages(&self, session_key: &str) -> Vec<ChatMessage> {
        self.get_messages_sync(session_key)
    }

    async fn clear(&self, session_key: &str) {
        self.clear_sync(session_key);
    }

    async fn session_count(&self) -> usize {
        self.session_count_sync()
    }
}

#[async_trait]
impl Memory for Arc<dyn Memory> {
    async fn push_message(&self, session_key: &str, message: ChatMessage) {
        (**self).push_message(session_key, message).await;
    }

    async fn extend_messages(&self, session_key: &str, messages: Vec<ChatMessage>) {
        (**self).extend_messages(session_key, messages).await;
    }

    async fn set_system_prompt(&self, session_key: &str, prompt: String) {
        (**self).set_system_prompt(session_key, prompt).await;
    }

    async fn get_system_prompt(&self, session_key: &str) -> Option<String> {
        (**self).get_system_prompt(session_key).await
    }

    async fn get_messages(&self, session_key: &str) -> Vec<ChatMessage> {
        (**self).get_messages(session_key).await
    }

    async fn clear(&self, session_key: &str) {
        (**self).clear(session_key).await;
    }

    async fn session_count(&self) -> usize {
        (**self).session_count().await
    }
}

