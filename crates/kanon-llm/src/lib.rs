//! # Kanon LLM Module
//!
//! Provides a unified, high-performance LLM gateway, concurrent pluggable
//! session memory, and a general-purpose agent engine with cross-language Tool Calling.
//!
//! ## Submodules
//! - [`agent`]: General-purpose agent engine for conversational bots and autonomous task workers.
//! - [`error`]: Granular error types for gateway, agent, and tool routing.
//! - [`gateway`]: Protocol-level LLM client implementations (OpenAI Chat, OpenAI Responses, Anthropic Messages).
//! - [`memory`]: Pluggable conversation memory subsystem with [`Memory`] trait and lock-free [`SlidingWindowMemory`].
//! - [`tool_router`]: Specialized pipeline router adapter, dynamic tool aggregation, and in-memory Protobuf/JSON translation.

pub mod agent;
pub mod error;
pub mod gateway;
pub mod memory;
pub mod sqlite_memory;
pub mod summary;
pub mod token;
pub mod tool_router;

pub use agent::{
    Agent, AgentBuilder, AgentConfig, AgentHook, AgentOutput, AgentTool, NativeTool,
    NativeToolFn, NoopHost,
};
pub use error::{AgentError, GatewayError, ToolRouterError};
pub use gateway::providers::{
    AnthropicMessagesProvider, AnthropicProvider, OpenAiChatProvider, OpenAiProvider,
    OpenAiResponsesProvider, SseDecoder, SseEvent,
};
pub use gateway::{
    ChatChunk, ChatChunkStream, ChatMessage, ChatRequest, ChatResponse, LlmGateway, LlmProvider,
    Role, TokenUsage, ToolCall, ToolDefinition,
};
pub use memory::{ConversationManager, Memory, SessionMemory, SlidingWindowMemory};
pub use sqlite_memory::{PersistentMemory, SqliteMemory};
pub use summary::{ContextSummarizer, SummaryConfig, SummaryHook};
pub use token::{
    estimate_conversation_tokens, estimate_message_tokens, estimate_text_tokens, estimate_tokens,
};
pub use tool_router::{aggregate_tools, ToolHost, ToolRouter, ToolRouterOutput};


