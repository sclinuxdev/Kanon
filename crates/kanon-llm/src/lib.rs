//! # Kanon LLM Module
//!
//! Provides a unified, high-performance LLM gateway, concurrent sliding-window
//! session memory, and a cross-language Tool Calling state machine loop.
//!
//! ## Submodules
//! - [`error`]: Granular error types for gateway and tool routing.
//! - [`gateway`]: Multi-provider LLM client abstractions (OpenAI-compatible, Ollama native, etc.).
//! - [`memory`]: Lightweight lock-free conversation memory keyed by `channel_id:sender_id`.
//! - [`tool_router`]: Dynamic tool aggregation, in-memory Protobuf/JSON payload translation,
//!   and recursion-guarded tool execution loop over gRPC IPC.

pub mod error;
pub mod gateway;
pub mod memory;
pub mod tool_router;

pub use error::{GatewayError, ToolRouterError};
pub use gateway::{LlmGateway, LlmProvider};
pub use memory::ConversationManager;
pub use tool_router::{aggregate_tools, ToolHost, ToolRouter, ToolRouterOutput};
