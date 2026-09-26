//! Shared management-gateway state and its composition-root builder.
//!
//! [`ApiState`] is the single dependency bundle handed to every route handler. It is cheap to
//! clone (one `Arc` bump) because Axum clones state per request; all shared owners — supervisor,
//! session manager, persona registry, agent, observability channels — are themselves `Arc`-held.
//!
//! The builder is the honest composition root: it either receives fully constructed components
//! or builds them from a model provider, and it refuses to fabricate a fake agent when no
//! provider is configured. In that case `/api/v1/chat/completions` answers `503` explicitly.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use kanon_core::{EventIngress, InstanceRegistry, PluginStateStore, Supervisor};
use kanon_llm::{
    Agent, AgentConfig, AgentFactory, AgentSlot, LlmProvider, Memory, PersonaRegistry,
    SessionManager, SlidingWindowMemory,
};

use crate::error::ApiError;
use crate::llm_config::SystemConfigStore;
use crate::observability::Observability;
use crate::plugin_config::PluginConfigStore;

/// Default sliding-window depth applied when the builder must create a memory backend.
const DEFAULT_MEMORY_WINDOW: usize = 40;

/// Shared, cloneable state injected into every management route.
#[derive(Clone)]
pub struct ApiState {
    inner: Arc<ApiStateInner>,
}

/// Owners shared across all requests.
struct ApiStateInner {
    /// Instant the gateway process started, used for uptime reporting.
    started_at: Instant,
    /// Semantic version reported by the health and metrics endpoints.
    version: String,
    /// Process supervisor owning plugin host sub-processes.
    supervisor: Arc<Supervisor>,
    /// Conversation session lifecycle manager.
    sessions: Arc<SessionManager>,
    /// Persona catalog backing the persona endpoints.
    personas: Arc<PersonaRegistry>,
    /// Live agent runtime backing chat completions, conversational pipeline turns and IPC
    /// `RequestLLM`.
    ///
    /// The factory owns the node's provider slot and builds per-instance model overrides, so
    /// provider changes made through the control plane take effect on the next request without
    /// restarting the node.
    factory: Arc<AgentFactory>,
    /// Catalog of bot instances deciding whether and how inbound events are answered.
    instances: Arc<InstanceRegistry>,
    /// Persisted enable/disable state for discovered plugins.
    plugin_state: Arc<PluginStateStore>,
    /// Persistence for per-plugin configuration values.
    config_store: Arc<PluginConfigStore>,
    /// Persistence for node-level system settings, including the console-selected provider.
    system_config: Arc<SystemConfigStore>,
    /// Real-time log and trace channels plus the metrics registry.
    observability: Arc<Observability>,
    /// Fast-ACK ingest handle driving the inbound data plane, absent when no pipeline is attached.
    ingress: Option<EventIngress>,
    /// Base directory where discovered and installed plugins are stored.
    plugins_dir: PathBuf,
}

impl ApiState {
    /// Returns a fluent builder rooted at a supervisor instance.
    pub fn builder(supervisor: Arc<Supervisor>) -> ApiStateBuilder {
        ApiStateBuilder::new(supervisor)
    }

    /// Gateway start instant, used to compute uptime.
    pub fn started_at(&self) -> Instant {
        self.inner.started_at
    }

    /// Reported gateway version.
    pub fn version(&self) -> &str {
        &self.inner.version
    }

    /// Plugin supervisor handle.
    pub fn supervisor(&self) -> &Arc<Supervisor> {
        &self.inner.supervisor
    }

    /// Session manager handle.
    pub fn sessions(&self) -> &Arc<SessionManager> {
        &self.inner.sessions
    }

    /// Persona registry handle.
    pub fn personas(&self) -> &Arc<PersonaRegistry> {
        &self.inner.personas
    }

    /// Agent runtime handle, if a model provider was configured.
    ///
    /// Returns a snapshot of the live slot: callers always observe the provider that is
    /// configured *now*, which is what lets the console change it at runtime.
    pub fn agent(&self) -> Option<Arc<Agent>> {
        self.inner.factory.node_agent()
    }

    /// Agent that should serve a request using an optional model override.
    ///
    /// A blank or default model resolves to the node agent, so callers never need to compare
    /// model identifiers themselves.
    pub fn agent_for_model(&self, model: Option<&str>) -> Option<Arc<Agent>> {
        self.inner.factory.agent_for_model(model)
    }

    /// Requires an agent runtime, failing with `503` when none is configured.
    pub fn require_agent(&self) -> Result<Arc<Agent>, ApiError> {
        self.agent().ok_or_else(|| {
            ApiError::Unavailable(
                "No LLM provider is configured for this core; chat completions are disabled"
                    .to_string(),
            )
        })
    }

    /// Agent factory shared with the pipeline worker and the console sandbox.
    pub fn agent_factory(&self) -> &Arc<AgentFactory> {
        &self.inner.factory
    }

    /// Shared agent slot, used by the composition root to wire the IPC service to the node's
    /// provider source.
    pub fn llm_slot(&self) -> &Arc<AgentSlot> {
        self.inner.factory.slot()
    }

    /// Bot-instance catalog.
    pub fn instances(&self) -> &Arc<InstanceRegistry> {
        &self.inner.instances
    }

    /// Persisted plugin enable/disable state.
    pub fn plugin_state(&self) -> &Arc<PluginStateStore> {
        &self.inner.plugin_state
    }

    /// Installs a model provider on the running node and returns the resulting agent.
    ///
    /// The pipeline worker, the chat endpoints and the IPC gateway all observe the swap on their
    /// next call; nothing needs to be restarted and no request in flight is disturbed.
    pub fn apply_llm_provider(
        &self,
        name: impl Into<String>,
        provider: Arc<dyn LlmProvider>,
        config: AgentConfig,
    ) -> Arc<Agent> {
        self.inner.factory.install(name, provider, config)
    }

    /// Clears the configured provider, disabling chat and conversational routing.
    pub fn clear_llm_provider(&self) {
        self.inner.factory.clear();
    }

    /// Plugin configuration store handle.
    pub fn config_store(&self) -> &Arc<PluginConfigStore> {
        &self.inner.config_store
    }

    /// Node-level system configuration store handle.
    pub fn system_config(&self) -> &Arc<SystemConfigStore> {
        &self.inner.system_config
    }

    /// Observability hub handle.
    pub fn observability(&self) -> &Arc<Observability> {
        &self.inner.observability
    }

    /// Inbound ingest handle used by the adapter data plane.
    pub fn ingress(&self) -> Option<&EventIngress> {
        self.inner.ingress.as_ref()
    }

    /// Plugins directory handle.
    pub fn plugins_dir(&self) -> &std::path::Path {
        &self.inner.plugins_dir
    }
}

/// Fluent builder assembling [`ApiState`].
pub struct ApiStateBuilder {
    supervisor: Arc<Supervisor>,
    version: String,
    sessions: Option<Arc<SessionManager>>,
    personas: Option<Arc<PersonaRegistry>>,
    memory: Option<Arc<dyn Memory>>,
    agent: Option<Arc<Agent>>,
    pending_llm: Option<PendingLlm>,
    agent_slot: Option<Arc<AgentSlot>>,
    instances: Option<Arc<InstanceRegistry>>,
    plugin_state: Option<Arc<PluginStateStore>>,
    system_config: Option<Arc<SystemConfigStore>>,
    config_base_dir: Option<PathBuf>,
    observability: Option<Arc<Observability>>,
    ingress: Option<EventIngress>,
    plugins_dir: Option<PathBuf>,
}

/// Model provider awaiting agent construction at build time.
struct PendingLlm {
    name: String,
    provider: Arc<dyn LlmProvider>,
    config: AgentConfig,
}

impl ApiStateBuilder {
    /// Creates a builder for the given supervisor.
    pub fn new(supervisor: Arc<Supervisor>) -> Self {
        Self {
            supervisor,
            version: env!("CARGO_PKG_VERSION").to_string(),
            sessions: None,
            personas: None,
            memory: None,
            agent: None,
            pending_llm: None,
            agent_slot: None,
            instances: None,
            plugin_state: None,
            system_config: None,
            config_base_dir: None,
            observability: None,
            ingress: None,
            plugins_dir: None,
        }
    }

    /// Overrides the version string reported by the gateway.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Overrides the directory where plugins are discovered and installed.
    pub fn with_plugins_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.plugins_dir = Some(dir.into());
        self
    }

    /// Injects a pre-built conversation memory backend.
    pub fn with_memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Injects a pre-built session manager (its memory becomes the shared memory backend).
    pub fn with_sessions(mut self, sessions: Arc<SessionManager>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// Injects a pre-built persona registry.
    pub fn with_personas(mut self, personas: Arc<PersonaRegistry>) -> Self {
        self.personas = Some(personas);
        self
    }

    /// Injects a fully constructed agent.
    ///
    /// Prefer [`ApiStateBuilder::with_llm_provider`] unless the agent already carries its own
    /// lifecycle hooks: an agent built elsewhere will not publish tool-calling trace events.
    /// Ignored when an explicit slot is injected, which stays authoritative.
    pub fn with_agent(mut self, agent: Arc<Agent>) -> Self {
        self.agent = Some(agent);
        self
    }

    /// Shares an externally owned agent slot as the node's provider source.
    ///
    /// The composition root uses this to hand the *same* slot to the pipeline worker, the IPC
    /// service and the management gateway. When a slot is injected it is authoritative: the
    /// builder neither constructs nor overwrites an agent for it.
    pub fn with_agent_slot(mut self, slot: Arc<AgentSlot>) -> Self {
        self.agent_slot = Some(slot);
        self
    }

    /// Builds the agent runtime from a model provider.
    ///
    /// The resulting agent shares this gateway's memory, session manager and persona registry,
    /// and carries the event bus as an [`kanon_llm::AgentHook`] so that LLM request/response and
    /// tool-calling stages appear on `/ws/v1/events`.
    pub fn with_llm_provider(
        mut self,
        name: impl Into<String>,
        provider: Arc<dyn LlmProvider>,
        config: AgentConfig,
    ) -> Self {
        self.pending_llm = Some(PendingLlm {
            name: name.into(),
            provider,
            config,
        });
        self
    }

    /// Shares the persisted plugin enable/disable state.
    pub fn with_plugin_state(mut self, state: Arc<PluginStateStore>) -> Self {
        self.plugin_state = Some(state);
        self
    }

    /// Shares the bot-instance catalog that gates and partitions inbound events.
    pub fn with_instances(mut self, instances: Arc<InstanceRegistry>) -> Self {
        self.instances = Some(instances);
        self
    }

    /// Overrides the node-level system configuration store.
    ///
    /// Defaults to the node's `data/system.json`, which is where the provider endpoints persist
    /// the operator's selection.
    pub fn with_system_config(mut self, store: Arc<SystemConfigStore>) -> Self {
        self.system_config = Some(store);
        self
    }

    /// Overrides the base directory used to persist plugin configuration.
    pub fn with_config_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config_base_dir = Some(dir.into());
        self
    }

    /// Injects a pre-built observability hub (shared log/trace channels and metrics).
    pub fn with_observability(mut self, observability: Arc<Observability>) -> Self {
        self.observability = Some(observability);
        self
    }

    /// Injects the Fast-ACK ingest handle that connects the adapter data plane to the pipeline.
    ///
    /// Without it the ingest endpoint answers `503`: the gateway refuses to accept messages it
    /// cannot hand to a running pipeline.
    pub fn with_ingress(mut self, ingress: EventIngress) -> Self {
        self.ingress = Some(ingress);
        self
    }

    /// Finalizes the state graph.
    pub fn build(self) -> ApiState {
        let observability = self
            .observability
            .unwrap_or_else(|| Arc::new(Observability::new()));

        // Memory resolution order: explicit backend, session manager's backend, in-memory default.
        let memory = self
            .memory
            .or_else(|| self.sessions.as_ref().map(|sm| sm.memory().clone()))
            .unwrap_or_else(|| Arc::new(SlidingWindowMemory::new(DEFAULT_MEMORY_WINDOW)));

        let sessions = self
            .sessions
            .unwrap_or_else(|| Arc::new(SessionManager::new(memory.clone())));
        let personas = self.personas.unwrap_or_default();

        // Agent construction is centralised in the factory so the node agent, per-instance model
        // overrides and the console sandbox all share one memory, session manager, persona
        // registry and trace bus. A caller-supplied slot is adopted verbatim (the composition root
        // hands the same slot to the pipeline and the IPC service).
        let slot = self
            .agent_slot
            .unwrap_or_else(|| Arc::new(AgentSlot::new()));
        let factory = Arc::new(AgentFactory::new(
            "kanon-core",
            slot.clone(),
            memory,
            sessions.clone(),
            personas.clone(),
            Some(observability.events.clone()),
        ));

        // A provider the builder was handed is installed into the shared slot; dropping it
        // silently would leave the node reporting a provider it cannot use.
        if let Some(agent) = self.agent {
            slot.set(Some(agent));
        } else if let Some(pending) = self.pending_llm {
            factory.install(pending.name, pending.provider, pending.config);
        }

        let instances = self.instances.unwrap_or_default();
        let plugin_state = self.plugin_state.unwrap_or_default();

        let config_store = Arc::new(match self.config_base_dir {
            Some(dir) => PluginConfigStore::new(dir),
            None => PluginConfigStore::default(),
        });

        let system_config = self
            .system_config
            .unwrap_or_else(|| Arc::new(SystemConfigStore::default()));

        let plugins_dir = self.plugins_dir.unwrap_or_else(|| PathBuf::from("./plugins"));

        ApiState {
            inner: Arc::new(ApiStateInner {
                started_at: Instant::now(),
                version: self.version,
                supervisor: self.supervisor,
                sessions,
                personas,
                factory,
                instances,
                plugin_state,
                config_store,
                system_config,
                observability,
                ingress: self.ingress,
                plugins_dir,
            }),
        }
    }
}

/// Applies the default agent configuration used by the standalone gateway binary.
pub fn default_agent_config(model: impl Into<String>) -> AgentConfig {
    AgentConfig {
        default_model: model.into(),
        ..AgentConfig::default()
    }
}
