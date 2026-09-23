//! Decoupled LLM protocol provider implementations.
//!
//! Provides protocol-level clients strictly adhering to industry specifications:
//! - [`openai`]: Standard OpenAI Chat Completions protocol (`/chat/completions`).
//! - [`anthropic`]: Standard Anthropic Messages protocol (`/v1/messages`).
//!
//! No vendor endpoints or brand-specific APIs are hardcoded.

pub mod anthropic;
pub mod openai;

pub use anthropic::{AnthropicMessagesProvider, AnthropicProvider};
pub use openai::{OpenAiChatProvider, OpenAiProvider};
