//! Decoupled LLM protocol provider implementations.
//!
//! Provides protocol-level clients strictly adhering to industry specifications:
//! - [`openai`]: Standard OpenAI Chat Completions protocol (`/chat/completions`).
//! - [`openai_responses`]: Modern OpenAI Responses protocol (`/v1/responses`).
//! - [`anthropic`]: Standard Anthropic Messages protocol (`/v1/messages`).
//!
//! No vendor endpoints or brand-specific APIs are hardcoded.

pub mod anthropic;
pub mod openai;
pub mod openai_responses;
pub mod sse;

pub use anthropic::{AnthropicMessagesProvider, AnthropicProvider};
pub use openai::{OpenAiChatProvider, OpenAiProvider};
pub use openai_responses::OpenAiResponsesProvider;
pub use sse::{SseDecoder, SseEvent};


