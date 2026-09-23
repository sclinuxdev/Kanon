//! Kanon Official Rust SDK (Skeleton)
//!
//! Provides traits and types for developing out-of-process Rust plugins.

pub mod plugin;
pub mod context;

pub mod prelude {
    pub use super::plugin::*;
    pub use super::context::*;
    pub use async_trait::async_trait;
}
