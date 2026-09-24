//! Kanon Official Rust SDK.
//!
//! Provides the trait and runtime types for developing out-of-process Rust plugins, including
//! platform adapters: declare `[adapter] platform = "..."` in `plugin.toml`, implement
//! [`Plugin::on_deliver_message`] for outbound delivery, and push inbound messages back through
//! the [`CoreHandle`] exposed on [`PluginContext::core`].

pub mod plugin;
pub mod context;
pub mod host;

pub use context::CoreHandle;
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
