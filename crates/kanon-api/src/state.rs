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

use kanon_core::{EventIngress, Supervisor};
use kanon_llm::{
    Agent, AgentConfig, LlmProvider, Memory, PersonaRegistry, SessionManager, SlidingWindowMemory,
};

use crate::error::ApiError;
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
    /// Agent runtime backing sandbox chat completions, absent when no provider is configured.
    agent: Option<Arc<Agent>>,
    /// Persistence for per-plugin configuration values.
    config_store: Arc<PluginConfigStore>,
    /// Real-time log and trace channels plus the metrics registry.
    observability: Arc<Observability>,
    /// Fast-ACK ingest handle driving the inbound data plane, absent when no pipeline is attached.
    ingress: Option<EventIngress>,
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
    pub fn agent(&self) -> Option<&Arc<Agent>> {
        self.inner.agent.as_ref()
    }

    /// Requires an agent runtime, failing with `503` when none is configured.
    pub fn require_agent(&self) -> Result<&Arc<Agent>, ApiError> {
        self.inner.agent.as_ref().ok_or_else(|| {
            ApiError::Unavailable(
                "No LLM provider is configured for this core; chat completions are disabled"
                    .to_string(),
            )
        })
    }

    /// Plugin configuration store handle.
    pub fn config_store(&self) -> &Arc<PluginConfigStore> {
        &self.inner.config_store
    }

    /// Observability hub handle.
    pub fn observability(&self) -> &Arc<Observability> {
        &self.inner.observability
    }

    /// Inbound ingest handle used by the adapter data plane.
    pub fn ingress(&self) -> Option<&EventIngress> {
        self.inner.ingress.as_ref()
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
    config_base_dir: Option<PathBuf>,
    observability: Option<Arc<Observability>>,
    ingress: Option<EventIngress>,
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
            config_base_dir: None,
            observability: None,
            ingress: None,
        }
    }

    /// Overrides the version string reported by the gateway.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
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
    pub fn with_agent(mut self, agent: Arc<Agent>) -> Self {
        self.agent = Some(agent);
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

        let agent = self.agent.or_else(|| {
            self.pending_llm.map(|pending| {
                let config = pending.config;

                // The builder exposes fluent setters rather than a whole-config setter, so the
                // optional sampling knobs are applied only when explicitly configured.
                let mut builder = Agent::builder(pending.name, pending.provider)
                    .memory(memory.clone())
                    .session_manager(sessions.clone())
                    .persona_registry(personas.clone())
                    .hook_arc(observability.events.clone())
                    .model(config.default_model.clone())
                    .max_iterations(config.max_iterations)
                    .stop_on_tool_failure(config.stop_on_tool_failure);

                if let Some(temperature) = config.temperature {
                    builder = builder.temperature(temperature);
                }
                if let Some(max_tokens) = config.max_tokens {
                    builder = builder.max_tokens(max_tokens);
                }

                Arc::new(builder.build())
            })
        });

        let config_store = Arc::new(match self.config_base_dir {
            Some(dir) => PluginConfigStore::new(dir),
            None => PluginConfigStore::default(),
        });

        ApiState {
            inner: Arc::new(ApiStateInner {
                started_at: Instant::now(),
                version: self.version,
                supervisor: self.supervisor,
                sessions,
                personas,
                agent,
                config_store,
                observability,
                ingress: self.ingress,
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
