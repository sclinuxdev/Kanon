//! Decoupled LLM provider modules.
//!
//! Each provider maintains its own isolated wire protocols and serialization logic.

pub mod ollama;
pub mod openai;

pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
