//! Developer toolchain library for Kanon plugin lifecycle management.
//!
//! Provides scaffolding, static manifest linting, distribution archive packaging (.kpk),
//! and offline interactive and automated sandbox testing.

pub mod lint;
pub mod pack;
pub mod sandbox;
pub mod scaffold;

pub use lint::{lint_plugin, LintError, LintReport};
pub use pack::{pack_plugin, PackError, PackReport};
pub use sandbox::{run_sandbox, SandboxError, SandboxOptions};
pub use scaffold::{create_plugin_project, ScaffoldError};
