//! Kanon Storage Module.
//!
//! Provides embedded persistence and per-plugin isolated data directories.

pub mod dir;
pub mod id;

pub use dir::PluginDataDir;
pub use id::{PluginId, PluginIdError};
