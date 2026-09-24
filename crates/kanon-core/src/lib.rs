//! Kanon Core Microkernel Engine.
//!
//! Provides the central event pipeline, IPC gRPC server on `core.sock`,
//! static manifest parser, process supervisor for managing out-of-process
//! plugin hosts, and the platform adapter contract that connects the
//! microkernel to chat platforms.

pub mod adapter;
pub mod ipc;
pub mod manifest;
pub mod pipeline;
pub mod supervisor;

pub use adapter::{
    AdapterDescriptor, AdapterError, AdapterKind, AdapterRegistry, EventIngress, IngestError,
    PlatformAdapter,
};
pub use ipc::{CoreApiService, CoreIpcServer, HostRegistration};
pub use manifest::{AdapterSection, PluginManifest, PluginSection, ToolDefinitionEntry};
pub use pipeline::{
    CommandRouter, DeliveryOutcome, MatchedCommand, PipelineEngine, PipelineObserver,
    PipelineResult, PipelineStage, PreFilterChain, PreFilterOutcome,
    DEFAULT_OUTBOUND_QUEUE_CAPACITY,
};
pub use supervisor::{AdapterRoute, LaunchSpec, ManagedHost, Supervisor, SupervisorError};
