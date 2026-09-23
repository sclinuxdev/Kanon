//! Kanon Core Microkernel Engine.
//!
//! Provides the central event pipeline, IPC gRPC server on `core.sock`,
//! static manifest parser, and process supervisor for managing out-of-process
//! plugin hosts.

pub mod ipc;
pub mod manifest;
pub mod pipeline;
pub mod supervisor;

pub use ipc::{CoreApiService, CoreIpcServer, HostRegistration};
pub use manifest::PluginManifest;
pub use pipeline::{
    CommandRouter, MatchedCommand, PipelineEngine, PipelineResult, PreFilterChain,
    PreFilterOutcome,
};
pub use supervisor::{ManagedHost, Supervisor, SupervisorError};
