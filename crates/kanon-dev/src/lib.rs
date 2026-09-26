//! Developer toolchain library for Kanon plugin lifecycle management.
//!
//! Provides scaffolding, static manifest linting, distribution archive packaging (.kpk),
//! and offline interactive and automated sandbox testing.

pub mod lint;
pub mod pack;
pub mod sandbox;
pub mod scaffold;

pub use lint::{LintError, LintReport, lint_plugin};
pub use pack::{PackError, PackReport, pack_plugin};
pub use sandbox::{SandboxError, SandboxOptions, run_sandbox};
pub use scaffold::{ScaffoldError, create_plugin_project};
