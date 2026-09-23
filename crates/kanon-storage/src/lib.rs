//! Kanon Storage Module.
//!
//! Provides embedded persistence and per-plugin isolated data directories.

pub mod dir;
pub mod kv;

pub use dir::PluginDataDir;
