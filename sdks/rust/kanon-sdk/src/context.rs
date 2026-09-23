//! Plugin execution context and environment metadata.
//!
//! Exposes access to isolated persistent directories and active plugin configurations.

use std::path::PathBuf;

/// Runtime context passed to plugins during initialization and invocation.
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// Dedicated directory for this plugin's local database and file storage (`./data/plugins/<id>/`).
    pub data_dir: PathBuf,
    /// Active dynamic configuration parameters provided by Core.
    pub config: Option<prost_types::Struct>,
}

impl PluginContext {
    /// Creates a new `PluginContext` for the given data directory.
    pub fn new(data_dir: PathBuf, config: Option<prost_types::Struct>) -> Self {
        Self { data_dir, config }
    }
}

