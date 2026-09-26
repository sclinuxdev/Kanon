//! Persisted enable/disable state for discovered plugins.
//!
//! # Why a separate store
//! A plugin's `plugin.toml` describes what the plugin *is*; whether an operator wants it running
//! on this node changes over time and must not rewrite files inside the user's plugin directory.
//! The state therefore lives next to the node's other settings (`data/plugins_state.json`) and is
//! applied at startup and on every toggle:
//!
//! - **disabled** → the host process is not spawned (or is stopped when it was running), so the
//!   plugin's pre-filters, commands, tools and adapter disappear from the node entirely;
//! - **enabled** (the default for a plugin absent from the store) → normal supervision.
//!
//! Keeping "not mentioned yet" as enabled means a freshly cloned plugin starts running without
//! the operator having to opt in, matching the behaviour of a node without this store.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Default location of the plugin state document, relative to the node working directory.
pub const DEFAULT_PLUGIN_STATE: &str = "./data/plugins_state.json";

/// Document persisted at the state path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginStateDocument {
    /// Schema version, so a future format change can be migrated explicitly.
    #[serde(default = "default_version")]
    version: u32,
    /// Plugin identifier -> enabled flag. Absent identifiers are enabled.
    #[serde(default)]
    plugins: HashMap<String, bool>,
    /// Unrecognized keys are carried through verbatim, so writing this document never discards
    /// settings another (possibly newer) component stored beside it.
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

/// Current schema version of the plugin state document.
fn default_version() -> u32 {
    1
}

/// Thread-safe plugin enable/disable state, optionally persisted.
///
/// A store without a path is *in-memory*: it behaves identically within the process but writes
/// nothing, which is what tests and embedded cores use.
#[derive(Debug)]
pub struct PluginStateStore {
    /// Path of the persisted document; `None` for an in-memory store.
    path: Option<PathBuf>,
    /// Plugin identifier -> enabled flag, for every plugin the operator has ever toggled.
    states: RwLock<HashMap<String, bool>>,
}

impl Default for PluginStateStore {
    /// In-memory store: the safe default for tests and embedded cores.
    fn default() -> Self {
        Self::in_memory()
    }
}

impl PluginStateStore {
    /// Creates a store that lives only for this process.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            states: RwLock::new(HashMap::new()),
        }
    }

    /// Opens (or creates) the store at `path`.
    ///
    /// A missing file means "nothing has been toggled yet" (every plugin enabled); a malformed
    /// file is an error, because silently enabling a plugin the operator disabled is exactly the
    /// surprise this store exists to prevent.
    pub async fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let states = match read_document(&path)? {
            Some(document) => document.plugins,
            None => HashMap::new(),
        };

        Ok(Self {
            path: Some(path),
            states: RwLock::new(states),
        })
    }

    /// Path of the persisted document, or `None` for an in-memory store.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns whether `plugin_id` is enabled (absent identifiers are enabled).
    pub async fn is_enabled(&self, plugin_id: &str) -> bool {
        self.states
            .read()
            .await
            .get(plugin_id)
            .copied()
            .unwrap_or(true)
    }

    /// Returns every plugin the operator explicitly disabled.
    pub async fn disabled_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .states
            .read()
            .await
            .iter()
            .filter(|(_, enabled)| !**enabled)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Sets the state of one plugin and persists it.
    ///
    /// Returns `true` when the state actually changed, so callers can skip side effects (stopping
    /// or spawning a host) for an idempotent request.
    pub async fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<bool, String> {
        let mut states = self.states.write().await;
        if states.get(plugin_id).copied() == Some(enabled) {
            return Ok(false);
        }

        states.insert(plugin_id.to_string(), enabled);
        self.persist(&states)?;
        Ok(true)
    }

    /// Atomically writes the document.
    fn persist(&self, states: &HashMap<String, bool>) -> Result<(), String> {
        let Some(path) = self.path.as_deref() else {
            return Ok(());
        };

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }

        // Read-modify-write so unrelated keys survive a toggle.
        let mut document = read_document(path)?.unwrap_or_default();
        document.version = default_version();
        document.plugins = states.clone();

        let payload = serde_json::to_string_pretty(&document)
            .map_err(|err| format!("failed to serialize plugin state: {err}"))?;

        let temp_path = path.with_extension("json.tmp");
        std::fs::write(&temp_path, payload)
            .map_err(|err| format!("failed to write {}: {err}", temp_path.display()))?;
        std::fs::rename(&temp_path, path).map_err(|err| {
            format!(
                "failed to move {} into place at {}: {err}",
                temp_path.display(),
                path.display()
            )
        })
    }
}

/// Reads the state document, returning `None` when the file does not exist.
fn read_document(path: &Path) -> Result<Option<PluginStateDocument>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let document: PluginStateDocument = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;

    Ok(Some(document))
}
