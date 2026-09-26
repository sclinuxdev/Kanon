//! Node-wide agent construction, shared by every consumer that needs a model runtime.
//!
//! # Why a factory instead of a single captured agent
//! One node runs several bot instances, and each instance may pick its own model while sharing
//! the node's provider, conversation memory, session manager, persona registry and trace bus.
//! Rebuilding an [`Agent`] for such an override is cheap (it clones `Arc`s) but must not diverge
//! from the node's constructor, so every agent — the node default and every per-instance
//! override — is built here.
//!
//! The node's default agent lives in an [`AgentSlot`] because it is replaced whenever the
//! operator configures a provider; per-model overrides are derived from whichever provider the
//! slot holds at that moment and are cached until the provider changes.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::agent::{Agent, AgentConfig, AgentHook, AgentTool};
use crate::gateway::LlmProvider;
use crate::memory::Memory;
use crate::prompt::PersonaRegistry;
use crate::session::SessionManager;
use crate::slot::AgentSlot;

/// Builds every agent the node runs, so they all share one memory, session manager, persona
/// registry and trace bus.
pub struct AgentFactory {
    /// Name given to the node's default agent.
    name: String,
    /// The node's default agent, replaced when the operator changes the provider.
    slot: Arc<AgentSlot>,
    /// Conversation memory shared by every agent.
    memory: Arc<dyn Memory>,
    /// Session lifecycle manager shared by every agent.
    sessions: Arc<SessionManager>,
    /// Persona catalog shared by every agent.
    personas: Arc<PersonaRegistry>,
    /// Lifecycle hooks shared by every agent: trace publishing, skill catalogs, ...
    hooks: Vec<Arc<dyn AgentHook>>,
    /// Native in-process tools available to every agent (e.g. `read_skill`).
    tools: Vec<Arc<dyn AgentTool>>,
    /// Agents built for per-instance model overrides, keyed by model identifier.
    ///
    /// Bounded by the number of distinct models operators configure, and dropped wholesale when
    /// the provider changes so an override can never outlive the provider it was built for.
    overrides: RwLock<HashMap<String, Arc<Agent>>>,
}

impl std::fmt::Debug for AgentFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentFactory")
            .field("name", &self.name)
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

impl AgentFactory {
    /// Creates a factory bound to one node's shared runtime parts.
    pub fn new(
        name: impl Into<String>,
        slot: Arc<AgentSlot>,
        memory: Arc<dyn Memory>,
        sessions: Arc<SessionManager>,
        personas: Arc<PersonaRegistry>,
        hooks: Vec<Arc<dyn AgentHook>>,
        tools: Vec<Arc<dyn AgentTool>>,
    ) -> Self {
        Self {
            name: name.into(),
            slot,
            memory,
            sessions,
            personas,
            hooks,
            tools,
            overrides: RwLock::new(HashMap::new()),
        }
    }

    /// The slot holding the node's default agent.
    pub fn slot(&self) -> &Arc<AgentSlot> {
        &self.slot
    }

    /// Native in-process tools shared by every agent (e.g. `read_skill`).
    ///
    /// Exposed so the management gateway can list every tool the node offers, including the ones
    /// that never come from a plugin host.
    pub fn native_tools(&self) -> &[Arc<dyn AgentTool>] {
        &self.tools
    }

    /// Conversation memory shared by every agent.
    pub fn memory(&self) -> &Arc<dyn Memory> {
        &self.memory
    }

    /// Session manager shared by every agent.
    pub fn sessions(&self) -> &Arc<SessionManager> {
        &self.sessions
    }

    /// Persona catalog shared by every agent.
    pub fn personas(&self) -> &Arc<PersonaRegistry> {
        &self.personas
    }

    /// Installs the node's provider and returns the resulting default agent.
    ///
    /// `name` labels the agent (it appears in trace events); the composition root passes the
    /// node's name. Cached per-model overrides are dropped: they were built against the previous
    /// provider and would otherwise keep sending requests to it.
    pub fn install(
        &self,
        name: impl Into<String>,
        provider: Arc<dyn LlmProvider>,
        config: AgentConfig,
    ) -> Arc<Agent> {
        let agent = Arc::new(self.build_agent_named(name.into(), provider, config));
        self.overrides
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.slot.set(Some(agent.clone()));
        agent
    }

    /// Clears the node's provider together with every derived override.
    pub fn clear(&self) {
        self.overrides
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.slot.set(None);
    }

    /// Agent for the node's configured provider.
    pub fn node_agent(&self) -> Option<Arc<Agent>> {
        self.slot.current()
    }

    /// Agent that should serve a conversation using an optional model override.
    ///
    /// `None`, a blank model, or the node's own default model all resolve to the default agent;
    /// anything else produces (and caches) an agent that shares everything except the model tag.
    pub fn agent_for_model(&self, model: Option<&str>) -> Option<Arc<Agent>> {
        let default_agent = self.slot.current()?;

        let requested = model.map(str::trim).filter(|model| !model.is_empty());
        let Some(requested) = requested else {
            return Some(default_agent);
        };
        if requested == default_agent.config().default_model {
            return Some(default_agent);
        }

        if let Some(cached) = self
            .overrides
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(requested)
        {
            return Some(cached.clone());
        }

        let mut config = default_agent.config().clone();
        config.default_model = requested.to_string();
        let agent = Arc::new(self.build_agent(default_agent.provider().clone(), config));
        self.overrides
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(requested.to_string(), agent.clone());
        Some(agent)
    }

    /// Builds an agent for an explicitly supplied provider, sharing this node's runtime parts.
    ///
    /// Used by the console sandbox, which may run a one-off request against credentials the
    /// operator typed without persisting them; such an agent must still see the same memory,
    /// sessions, personas and trace bus as the node's own.
    pub fn build_with(&self, provider: Arc<dyn LlmProvider>, config: AgentConfig) -> Arc<Agent> {
        Arc::new(self.build_agent(provider, config))
    }

    /// Builds one agent sharing this factory's memory, sessions, personas and hook.
    fn build_agent(&self, provider: Arc<dyn LlmProvider>, config: AgentConfig) -> Agent {
        self.build_agent_named(self.name.clone(), provider, config)
    }

    /// Builds one agent under an explicit name, sharing this factory's runtime parts.
    fn build_agent_named(
        &self,
        name: String,
        provider: Arc<dyn LlmProvider>,
        config: AgentConfig,
    ) -> Agent {
        // The builder exposes fluent setters rather than a whole-config setter, so optional
        // sampling knobs are applied only when configured.
        let mut builder = Agent::builder(name, provider)
            .memory(self.memory.clone())
            .session_manager(self.sessions.clone())
            .persona_registry(self.personas.clone())
            .model(config.default_model.clone())
            .max_iterations(config.max_iterations)
            .stop_on_tool_failure(config.stop_on_tool_failure);

        for hook in &self.hooks {
            builder = builder.hook_arc(hook.clone());
        }
        for tool in &self.tools {
            builder = builder.tool_arc(tool.clone());
        }
        if let Some(temperature) = config.temperature {
            builder = builder.temperature(temperature);
        }
        if let Some(max_tokens) = config.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }

        builder.build()
    }
}
