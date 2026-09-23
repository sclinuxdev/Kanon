//! Kanon Official Rust SDK (Skeleton)
//!
//! Provides traits and types for developing out-of-process Rust plugins.

pub mod plugin;
pub mod context;
pub mod host;

pub use host::KanonHost;
pub use kanon_proto as proto;

pub mod prelude {
    pub use super::plugin::*;
    pub use super::context::*;
    pub use super::host::KanonHost;
    pub use async_trait::async_trait;
    pub use kanon_proto::v1::*;
    pub use prost_types;
}
