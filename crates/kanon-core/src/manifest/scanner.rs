//! Plugin directory scanner and auto-discovery engine.
//!
//! Traverses plugin directories (such as `./plugins`), discovers `plugin.toml`
//! manifests, parses and validates static metadata, and sorts plugins according
//! to scheduling priority.

use std::fs;
use std::path::{Path, PathBuf};

use crate::manifest::PluginManifest;

/// Represents a statically discovered plugin on disk.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Parsed manifest declaration from `plugin.toml`.
    pub manifest: PluginManifest,
    /// Absolute or relative path to the `plugin.toml` manifest.
    pub manifest_path: PathBuf,
    /// Directory containing the plugin files.
    pub plugin_dir: PathBuf,
}

/// Scanner responsible for discovering and sorting plugins from filesystem directories.
pub struct PluginScanner;

impl PluginScanner {
    /// Default plugins directory relative to the microkernel working directory.
    pub const DEFAULT_PLUGINS_DIR: &'static str = "./plugins";

    /// Scans the target directory for plugins containing valid `plugin.toml` manifests.
    ///
    /// The scanner inspects immediate subdirectories (and nested subdirectories up to 2 levels deep),
    /// ignoring hidden directories (`.*`), `node_modules`, `target`, and Python virtual environments (`.venv`).
    /// Discovered plugins are sorted in ascending order of scheduling priority (lower numbers execute first),
    /// using plugin identifiers as a secondary tie-breaker.
    pub fn scan(base_dir: impl AsRef<Path>) -> Result<Vec<DiscoveredPlugin>, std::io::Error> {
        let base = base_dir.as_ref();
        if !base.exists() || !base.is_dir() {
            tracing::debug!(
                dir = %base.display(),
                "Target plugins directory does not exist or is not a directory; returning empty catalog"
            );
            return Ok(Vec::new());
        }

        let mut discovered = Vec::new();
        Self::scan_directory_recursive(base, 0, 2, &mut discovered)?;

        // Sort discovered plugins: lower priority number executes first, default is 500.
        discovered.sort_by(|a, b| {
            let prio_a = a.manifest.plugin.priority.unwrap_or(500);
            let prio_b = b.manifest.plugin.priority.unwrap_or(500);
            prio_a
                .cmp(&prio_b)
                .then_with(|| a.manifest.plugin.id.cmp(&b.manifest.plugin.id))
        });

        tracing::info!(
            count = discovered.len(),
            dir = %base.display(),
            "Completed plugin directory discovery scan"
        );

        Ok(discovered)
    }

    /// Recursively scans directory levels up to `max_depth`.
    fn scan_directory_recursive(
        dir: &Path,
        current_depth: usize,
        max_depth: usize,
        results: &mut Vec<DiscoveredPlugin>,
    ) -> Result<(), std::io::Error> {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(dir = %dir.display(), error = %err, "Failed to read directory; skipping");
                return Ok(());
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            // Skip hidden directories and known non-plugin dependency/build directories
            if name_str.starts_with('.')
                || name_str == "node_modules"
                || name_str == "target"
                || name_str == ".venv"
                || name_str == "venv"
                || name_str == "__pycache__"
            {
                continue;
            }

            if path.is_dir() {
                let manifest_file = path.join("plugin.toml");
                if manifest_file.is_file() {
                    match PluginManifest::load_from_file(&manifest_file) {
                        Ok(manifest) => {
                            tracing::debug!(
                                plugin_id = %manifest.plugin.id,
                                path = %manifest_file.display(),
                                "Discovered valid plugin manifest"
                            );
                            results.push(DiscoveredPlugin {
                                manifest,
                                manifest_path: manifest_file,
                                plugin_dir: path,
                            });
                        }
                        Err(err) => {
                            tracing::warn!(
                                path = %manifest_file.display(),
                                error = %err,
                                "Failed to parse plugin manifest; skipping plugin"
                            );
                        }
                    }
                } else if current_depth < max_depth {
                    Self::scan_directory_recursive(&path, current_depth + 1, max_depth, results)?;
                }
            }
        }

        Ok(())
    }

    /// Searches for a `plugin.toml` file directly inside `dir` or within its immediate single subdirectory.
    pub fn find_manifest_in_dir(dir: &Path) -> Option<PathBuf> {
        let direct = dir.join("plugin.toml");
        if direct.is_file() {
            return Some(direct);
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let candidate = path.join("plugin.toml");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }

        None
    }
}
