//! Kanon Core Microkernel Engine.
//!
//! Provides the central event pipeline, IPC gRPC server on `core.sock`,
//! static manifest parser, process supervisor for managing out-of-process
//! plugin hosts, and the platform adapter contract that connects the
//! microkernel to chat platforms.

pub mod adapter;
pub mod instance;
pub mod ipc;
pub mod manifest;
pub mod pipeline;
pub mod plugin_state;
pub mod supervisor;

pub use adapter::{
    AdapterDescriptor, AdapterError, AdapterKind, AdapterRegistry, EventIngress, IngestError,
    PlatformAdapter,
};
pub use instance::{
    BotInstance, InstanceDraft, InstanceError, InstanceRegistry, instance_persona_id,
    sync_instance_personas, DEFAULT_INSTANCE_CATALOG,
};
pub use ipc::{CoreApiService, CoreIpcServer};
pub use manifest::{
    AdapterSection, DiscoveredPlugin, PluginManifest, PluginScanner, PluginSection,
    ToolDefinitionEntry,
};
pub use plugin_state::{PluginStateStore, DEFAULT_PLUGIN_STATE};
pub use pipeline::{
    CommandRouter, DeliveryOutcome, MatchedCommand, PipelineEngine, PipelineObserver,
    PipelineResult, PipelineStage, PreFilterChain, PreFilterOutcome, NEW_SESSION_COMMAND,
    DEFAULT_OUTBOUND_QUEUE_CAPACITY,
};
pub use supervisor::{
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState},
    AdapterRoute, HostHealth, LaunchSpec, ManagedHost, Supervisor, SupervisorError,
    UnavailablePlugin, HOST_WATCHDOG_INTERVAL, HOST_WATCHDOG_MAX_RESTARTS,
};
