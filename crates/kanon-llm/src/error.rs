//! Error types for the Kanon LLM module.
//!
//! Provides granular errors for model gateway transport/API failures
//! and tool calling routing/execution failures.

use thiserror::Error;

/// Errors arising during LLM gateway requests and provider communication.
#[derive(Debug, Error)]
pub enum GatewayError {
    /// HTTP transport or network failure.
    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    /// Serialization or JSON parsing failure.
    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// API returned a non-success HTTP status code.
    #[error("Provider API error (status {status}): {message}")]
    ApiStatus {
        /// HTTP status code returned by the provider endpoint.
        status: u16,
        /// Error message or body extracted from provider response.
        message: String,
    },

    /// Provider returned an empty or malformed payload.
    #[error("Invalid or empty response from model provider: {0}")]
    InvalidResponse(String),
}

/// Errors arising during tool routing and state machine loop execution.
#[derive(Debug, Error)]
pub enum ToolRouterError {
    /// Error originating from the underlying LLM gateway.
    #[error("LLM gateway failure: {0}")]
    Gateway(#[from] GatewayError),

    /// gRPC RPC status error returned by the plugin host.
    #[error("Tool execution gRPC failure: {0}")]
    Rpc(Box<tonic::Status>),

    /// Requested tool name was not declared by any active plugin.
    #[error("Tool '{0}' is not registered on any active plugin host")]
    ToolNotFound(String),
}

impl From<tonic::Status> for ToolRouterError {
    fn from(status: tonic::Status) -> Self {
        Self::Rpc(Box::new(status))
    }
}
