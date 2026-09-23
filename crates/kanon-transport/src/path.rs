//! Cross-platform runtime directory and IPC socket path resolution.
//!
//! Provides standardized path resolution for Kanon runtime endpoints,
//! including `core.sock` and per-host `host_<id>.sock` endpoints.

use std::path::{Path, PathBuf};

/// Default relative directory used for storing runtime sockets and ephemeral state.
pub const DEFAULT_RUN_DIR: &str = "./run";

/// Resolves the default runtime directory for Kanon IPC endpoints.
///
/// Priority:
/// 1. If `KANON_RUN_DIR` environment variable is set, use that path.
/// 2. If `XDG_RUNTIME_DIR` is set on Unix platforms, use `$XDG_RUNTIME_DIR/kanon/run`.
/// 3. Fallback to the local relative directory `./run`.
pub fn default_run_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("KANON_RUN_DIR") {
        return PathBuf::from(dir);
    }

    #[cfg(unix)]
    if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
        let xdg_path = PathBuf::from(xdg_runtime).join("kanon").join("run");
        return xdg_path;
    }

    PathBuf::from(DEFAULT_RUN_DIR)
}

/// Resolves the socket path for the Kanon Core IPC endpoint (`core.sock`).
///
/// If `base_dir` is `None`, the default runtime directory resolved by
/// [`default_run_dir`] will be used.
pub fn core_socket_path(base_dir: Option<&Path>) -> PathBuf {
    match base_dir {
        Some(dir) => dir.join("core.sock"),
        None => default_run_dir().join("core.sock"),
    }
}

/// Resolves the socket path for a dedicated plugin host IPC endpoint (`host_<host_id>.sock`).
///
/// If `base_dir` is `None`, the default runtime directory resolved by
/// [`default_run_dir`] will be used.
pub fn host_socket_path(host_id: &str, base_dir: Option<&Path>) -> PathBuf {
    let filename = format!("host_{host_id}.sock");
    match base_dir {
        Some(dir) => dir.join(filename),
        None => default_run_dir().join(filename),
    }
}

/// Ensures that the parent directory of a given file path exists.
///
/// If the directory does not exist, it is recursively created.
/// On Unix platforms, this directory is created with restrictive permissions (0700)
/// if created anew, preventing unauthorized local users from inspecting or tampering
/// with the Unix domain sockets.
pub fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::Permissions::from_mode(0o700);
                let _ = std::fs::set_permissions(parent, permissions);
            }
        }
    }
    Ok(())
}
