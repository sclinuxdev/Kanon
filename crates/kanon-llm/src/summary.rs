//! Long Context Summary Compression Subsystem.
//!
//! Provides token-budget-aware conversation summarization. When cumulative tokens
//! in a conversation window exceed the configured threshold, older messages are
//! intelligently extracted, summarized via a lightweight model, and replaced with a
//! condensed summary system block, preventing context blowout.

use std::sync::Arc;
use async_trait::async_trait;

use crate::agent::AgentHook;
use crate::error::AgentError;
use crate::gateway::types::{ChatMessage, ChatRequest, Role};
use crate::gateway::LlmProvider;
use crate::memory::Memory;
use crate::token::estimate_conversation_tokens;

/// Configuration governing automated context summarization.
#[derive(Debug, Clone)]
pub struct SummaryConfig {
    /// Whether automated summary compression is enabled.
    pub enabled: bool,
    /// Token threshold triggering conversation summarization (e.g. 2,000 tokens).
    pub trigger_token_budget: usize,
    /// Number of most recent messages kept uncompressed (e.g. last 4 messages / 2 turns).
    pub preserve_recent_messages: usize,
    /// Optional lightweight model override for generating the summary (e.g. `gpt-4o-mini`).
    pub summary_model: Option<String>,
    /// Optional custom summarization instructions.
    pub custom_instruction: Option<String>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_token_budget: 2048,
            preserve_recent_messages: 4,
            summary_model: None,
            custom_instruction: None,
        }
    }
}

/// Context summarizer managing the lifecycle of conversation compaction.
pub struct ContextSummarizer {
    config: SummaryConfig,
    provider: Arc<dyn LlmProvider>,
    memory: Arc<dyn Memory>,
}

impl ContextSummarizer {
    /// Creates a new `ContextSummarizer` with specified configuration, model provider, and memory backend.
    pub fn new(
        config: SummaryConfig,
        provider: Arc<dyn LlmProvider>,
        memory: Arc<dyn Memory>,
    ) -> Self {
        Self {
            config,
            provider,
            memory,
        }
    }

    /// Configuration reference.
    pub fn config(&self) -> &SummaryConfig {
        &self.config
    }

    /// Evaluates the session conversation token budget and applies summary compression if exceeded.
    ///
    /// Returns `Ok(true)` if summarization occurred, or `Ok(false)` if the conversation
    /// remained within the token budget.
    pub async fn compress_session(&self, session_key: &str) -> Result<bool, AgentError> {
        if !self.config.enabled {
            return Ok(false);
        }

        let messages = self.memory.get_messages(session_key).await;
        if messages.len() <= self.config.preserve_recent_messages + 1 {
            return Ok(false);
        }

        let current_tokens = estimate_conversation_tokens(&messages);
        if current_tokens <= self.config.trigger_token_budget {
            return Ok(false);
        }

        tracing::info!(
            session_key = %session_key,
            current_tokens = current_tokens,
            budget = self.config.trigger_token_budget,
            "Context token budget exceeded; initiating conversation summarization"
        );

        // Determine split boundary (excluding the system prompt at index 0 if present)
        let has_system_prompt = messages.first().map(|m| m.role == Role::System).unwrap_or(false);
        let start_idx = if has_system_prompt { 1 } else { 0 };
        let end_idx = messages.len().saturating_sub(self.config.preserve_recent_messages);

        if end_idx <= start_idx {
            return Ok(false);
        }

        // Adjust boundary so we don't sever an assistant tool call from its matching tool response
        let mut split_point = end_idx;
        while split_point < messages.len() && messages[split_point].role == Role::Tool {
            split_point += 1;
        }

        let old_messages = &messages[start_idx..split_point];
        let recent_messages = &messages[split_point..];

        // Format older messages into dialogue transcript
        let mut transcript = String::new();
        for msg in old_messages {
            let role_name = match msg.role {
                Role::System => "System",
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::Tool => "Tool",
            };
            if let Some(ref text) = msg.content {
                transcript.push_str(&format!("{role_name}: {text}\n"));
            } else if let Some(ref calls) = msg.tool_calls {
                let call_names: Vec<String> = calls.iter().map(|c| c.name.clone()).collect();
                transcript.push_str(&format!("Assistant invoked tools: {}\n", call_names.join(", ")));
            }
        }

        let default_instruction = "Provide a concise, factual summary of the following prior conversation transcript. Preserve important entity names, user preferences, conclusions, and key state:";
        let instruction = self.config.custom_instruction.as_deref().unwrap_or(default_instruction);

        let summary_prompt = format!("{instruction}\n\n---\n{transcript}\n---");

        let model = self
            .config
            .summary_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());

        let req = ChatRequest {
            model,
            messages: vec![ChatMessage::user(summary_prompt)],
            tools: vec![],
            temperature: Some(0.2),
            max_tokens: Some(512),
        };

        let resp = self.provider.chat(&req).await?;
        let summary_text = resp.content.unwrap_or_else(|| "Previous context summarized.".to_string());

        tracing::info!(
            session_key = %session_key,
            summary_len = summary_text.len(),
            "Summary generated successfully; compacting memory"
        );

        // Reconstruct conversation history:
        // [Existing System Prompt (if any)] + [Summary Context Message] + [Preserved Recent Messages]
        let system_prompt = self.memory.get_system_prompt(session_key).await;

        self.memory.clear(session_key).await;

        if let Some(prompt) = system_prompt {
            self.memory.set_system_prompt(session_key, prompt).await;
        }

        let summary_message = ChatMessage::system(format!(
            "--- Context Summary of Previous Conversation ---\n{summary_text}\n--- End Summary ---"
        ));

        let mut compacted = Vec::with_capacity(recent_messages.len() + 1);
        compacted.push(summary_message);
        compacted.extend(recent_messages.iter().cloned());

        self.memory.extend_messages(session_key, compacted).await;

        Ok(true)
    }
}

/// Agent lifecycle hook that automatically checks token budget and runs summary compression
/// prior to each model reasoning invocation.
pub struct SummaryHook {
    summarizer: Arc<ContextSummarizer>,
}

impl SummaryHook {
    /// Creates a new `SummaryHook` wrapping a `ContextSummarizer`.
    pub fn new(summarizer: Arc<ContextSummarizer>) -> Self {
        Self { summarizer }
    }
}

#[async_trait]
impl AgentHook for SummaryHook {
    async fn on_llm_request(&self, session_id: &str, _request: &mut ChatRequest) -> Result<(), AgentError> {
        let _ = self.summarizer.compress_session(session_id).await?;
        Ok(())
    }
}
