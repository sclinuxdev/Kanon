//! Plugin distribution packager generating `.kpk` bundles.
//!
//! Creates standard ZIP archives containing plugin assets, excludes development
//! artifacts (e.g. `target/`, `.git/`, `node_modules/`, `__pycache__/`), and calculates
//! SHA-256 integrity checksums saved to `<bundle>.kpk.sha256`.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::lint::{LintError, LintReport, lint_plugin};

/// Errors occurring during plugin bundle packaging.
#[derive(Debug, Error)]
pub enum PackError {
    /// File I/O failure while reading files or creating archives.
    #[error("I/O error during packaging: {0}")]
    Io(#[from] std::io::Error),
    /// Manifest lint validation failed.
    #[error(
        "Plugin manifest validation failed with {0} error(s). Run 'kanon-dev lint' for details."
    )]
    ValidationFailed(usize),
    /// Underlying linting failure.
    #[error("Linting failure: {0}")]
    Lint(#[from] LintError),
    /// ZIP compression failure.
    #[error("ZIP packaging error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Detailed outcome of a successful plugin packaging operation.
#[derive(Debug, Clone)]
pub struct PackReport {
    /// Path to the generated `.kpk` bundle file.
    pub bundle_path: PathBuf,
    /// Path to the accompanying `.sha256` checksum file.
    pub checksum_path: PathBuf,
    /// Computed hex-encoded SHA-256 digest string.
    pub sha256_hex: String,
    /// Total number of files packaged into the archive.
    pub file_count: usize,
    /// Size of the generated archive in bytes.
    pub bundle_size_bytes: u64,
    /// Associated lint report.
    pub lint_report: LintReport,
}

/// Determines whether a filesystem entry should be included in the `.kpk` distribution package.
fn should_include_path(rel_path: &Path) -> bool {
    let rel_str = rel_path.to_string_lossy();

    // Exclude development, source control, and build artifact directories
    let excluded_dirs = [
        ".git",
        "target",
        "node_modules",
        "__pycache__",
        ".venv",
        "venv",
        ".idea",
        ".vscode",
    ];

    for component in rel_path.components() {
        let comp_str = component.as_os_str().to_string_lossy();
        if excluded_dirs.contains(&comp_str.as_ref()) {
            return false;
        }
    }

    // Exclude temporary or binary cache files
    let excluded_extensions = ["pyc", "pyo", "sock", "kpk", "sha256", "log"];
    if let Some(ext) = rel_path.extension()
        && excluded_extensions.contains(&ext.to_string_lossy().as_ref())
    {
        return false;
    }

    // Exclude files matching temporary patterns
    if rel_str.ends_with('~') || rel_str.starts_with('.') {
        return false;
    }

    true
}

/// Packages a plugin project into a standardized `.kpk` distribution archive.
pub fn pack_plugin(plugin_path: &Path, output_dir: Option<&Path>) -> Result<PackReport, PackError> {
    // 1. Run static validation first; refuse to pack invalid manifests
    let lint_report = lint_plugin(plugin_path)?;
    if !lint_report.is_valid() {
        return Err(PackError::ValidationFailed(lint_report.errors.len()));
    }

    let plugin_root = &lint_report.plugin_root;
    let plugin_id = lint_report
        .plugin_id
        .clone()
        .unwrap_or_else(|| "plugin".to_string());

    // 2. Resolve destination directory and archive filename
    let out_dir = match output_dir {
        Some(dir) => dir.to_path_buf(),
        None => plugin_root.to_path_buf(),
    };
    fs::create_dir_all(&out_dir)?;

    let archive_name = format!("{}.kpk", plugin_id);
    let bundle_path = out_dir.join(&archive_name);
    let checksum_path = out_dir.join(format!("{}.sha256", archive_name));

    // 3. Build ZIP archive
    let file = File::create(&bundle_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut file_count = 0;
    let mut walk_stack = vec![plugin_root.to_path_buf()];

    while let Some(current_dir) = walk_stack.pop() {
        let entries = fs::read_dir(&current_dir)?;
        for entry in entries {
            let entry = entry?;
            let entry_path = entry.path();
            let rel_path = match entry_path.strip_prefix(plugin_root) {
                Ok(p) => p,
                Err(_) => continue,
            };

            if !should_include_path(rel_path) {
                continue;
            }

            if entry_path.is_dir() {
                walk_stack.push(entry_path);
            } else if entry_path.is_file() {
                let name_in_zip = rel_path.to_string_lossy().replace('\\', "/");
                zip.start_file(name_in_zip, options)?;
                let mut f = File::open(&entry_path)?;
                std::io::copy(&mut f, &mut zip)?;
                file_count += 1;
            }
        }
    }

    zip.finish()?;

    // 4. Compute SHA-256 digest of the bundle
    let mut bundle_file = File::open(&bundle_path)?;
    let mut bundle_bytes = Vec::new();
    bundle_file.read_to_end(&mut bundle_bytes)?;

    let digest = ring::digest::digest(&ring::digest::SHA256, &bundle_bytes);
    let sha256_hex: String = digest
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    // 5. Write checksum file in standard format: `<sha256>  <filename>`
    let checksum_content = format!("{}  {}\n", sha256_hex, archive_name);
    fs::write(&checksum_path, checksum_content)?;

    let metadata = fs::metadata(&bundle_path)?;
    let bundle_size_bytes = metadata.len();

    Ok(PackReport {
        bundle_path,
        checksum_path,
        sha256_hex,
        file_count,
        bundle_size_bytes,
        lint_report,
    })
}
