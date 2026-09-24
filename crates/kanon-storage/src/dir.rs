//! Plugin data directory manager.
//!
//! Enforces physical directory isolation for plugins under `./data/plugins/<id>/`.
//! Every plugin is allocated a dedicated sandbox directory for local databases,
//! file caches, and persistent state without cross-tenant interference.

use std::path::{Path, PathBuf};

/// Data directory manager for sandboxed plugin persistence.
#[derive(Debug, Clone)]
pub struct PluginDataDir {
    base_dir: PathBuf,
}

impl Default for PluginDataDir {
    fn default() -> Self {
        Self::new(Self::DEFAULT_BASE)
    }
}

impl PluginDataDir {
    /// Default root directory path for plugin persistent storage.
    pub const DEFAULT_BASE: &'static str = "./data/plugins";

    /// Creates a new `PluginDataDir` instance rooted at the specified base directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Returns a `PluginDataDir` referencing the standard default location (`./data/plugins`).
    pub fn default_dir() -> Self {
        Self::default()
    }

    /// Returns the configured base directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Resolves and ensures the isolated directory for a specific plugin identifier.
    ///
    /// Validates `plugin_id` against directory traversal attacks (`..`, `/`, `\`, NUL)
    /// before creating the directory recursively if it does not yet exist.
    pub fn resolve_plugin_dir(&self, plugin_id: &str) -> std::io::Result<PathBuf> {
        crate::id::PluginId::validate(plugin_id).map_err(|err| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string())
        })?;
        let dir = self.base_dir.join(plugin_id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Resolves a database file path for a plugin (e.g. `./data/plugins/<id>/memory.db`).
    ///
    /// Automatically ensures the parent directory exists before returning the path.
    pub fn resolve_db_path(&self, plugin_id: &str, db_name: &str) -> std::io::Result<PathBuf> {
        let dir = self.resolve_plugin_dir(plugin_id)?;
        Ok(dir.join(db_name))
    }
}
