//! Plugin static manifest (`plugin.toml`) schema and parser.
//!
//! Parses static metadata, runtime target requirements, command definitions,
//! and tool schemas from plugin configuration files.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Error types occurring during plugin manifest parsing.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// File I/O failure while accessing the manifest file.
    #[error("Failed to read manifest file: {0}")]
    Io(#[from] std::io::Error),
    /// Deserialization failure due to invalid TOML syntax or schema violation.
    #[error("Failed to parse TOML manifest: {0}")]
    Toml(#[from] toml::de::Error),
}

/// Metadata section `[plugin]` in `plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSection {
    /// Unique identifier for the plugin (e.g., `org.kanon.plugin.weather`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Author or organization attribution.
    pub author: Option<String>,
    /// Brief explanation of the plugin functionality.
    pub description: Option<String>,
    /// Runtime platform ("rust", "python", "typescript").
    pub runtime: String,
    /// Entrypoint executable path or script relative to plugin root.
    pub entrypoint: String,
    /// Whether this plugin must run in a dedicated sub-process.
    pub isolated: Option<bool>,
}

/// Command metadata declared under `[[commands]]` in `plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    /// Command trigger word (e.g. `rustcalc`).
    pub name: String,
    /// Short help text describing the command purpose.
    pub description: Option<String>,
    /// Usage example syntax.
    pub usage: Option<String>,
    /// Dispatch priority (lower numbers execute first, default 500).
    pub priority: Option<i32>,
}

/// Complete representation of a parsed `plugin.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Core plugin metadata.
    pub plugin: PluginSection,
    /// List of statically declared commands.
    #[serde(default)]
    pub commands: Vec<CommandDefinition>,
}

impl PluginManifest {
    /// Loads and parses a `plugin.toml` manifest from a given file path.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let manifest: Self = toml::from_str(&content)?;
        Ok(manifest)
    }
}
