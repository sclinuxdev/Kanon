//! Tests for the node agent factory: default agent, per-instance model overrides and their cache.

use std::sync::Arc;

use async_trait::async_trait;
use kanon_llm::{
    AgentConfig, AgentFactory, AgentSlot, ChatRequest, ChatResponse, GatewayError, LlmProvider,
    Memory, PersonaRegistry, SessionManager, SlidingWindowMemory,
};

/// Provider stub; identity matters more than behaviour in these tests.
struct StubProvider;

#[async_trait]
impl LlmProvider for StubProvider {
    async fn chat(&self, _request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        Ok(ChatResponse {
            content: Some("stub".to_string()),
            tool_calls: Vec::new(),
            finish_reason: Some("stop".to_string()),
            usage: None,
        })
    }
}

/// Builds a factory over fresh runtime parts, returning them for sharing assertions.
fn factory() -> (
    AgentFactory,
    Arc<dyn LlmProvider>,
    Arc<dyn Memory>,
    Arc<SessionManager>,
    Arc<PersonaRegistry>,
) {
    let slot = Arc::new(AgentSlot::new());
    let provider: Arc<dyn LlmProvider> = Arc::new(StubProvider);
    let memory: Arc<dyn Memory> = Arc::new(SlidingWindowMemory::new(10));
    let sessions = Arc::new(SessionManager::new(memory.clone()));
    let personas = Arc::new(PersonaRegistry::default());

    let factory = AgentFactory::new(
        "node",
        slot,
        memory.clone(),
        sessions.clone(),
        personas.clone(),
        Vec::new(),
        Vec::new(),
    );

    (factory, provider, memory, sessions, personas)
}

fn config(model: &str) -> AgentConfig {
    AgentConfig {
        default_model: model.to_string(),
        ..AgentConfig::default()
    }
}

#[test]
fn without_a_provider_every_lookup_is_empty() {
    let (factory, _provider, _memory, _sessions, _personas) = factory();

    assert!(factory.node_agent().is_none());
    assert!(factory.agent_for_model(None).is_none());
    assert!(factory.agent_for_model(Some("any-model")).is_none());
}

#[test]
fn blank_and_default_models_resolve_to_the_node_agent() {
    let (factory, provider, _memory, _sessions, _personas) = factory();
    let node = factory.install("node", provider, config("default-model"));

    for requested in [None, Some(""), Some("   "), Some("default-model")] {
        let resolved = factory
            .agent_for_model(requested)
            .unwrap_or_else(|| panic!("{requested:?} must resolve to the node agent"));
        assert!(
            Arc::ptr_eq(&resolved, &node),
            "{requested:?} must reuse the node agent rather than rebuild it"
        );
    }
}

#[test]
fn a_different_model_yields_a_shared_runtime_agent_and_is_cached() {
    let (factory, provider, memory, _sessions, _personas) = factory();
    factory.install("node", provider.clone(), config("default-model"));

    let override_agent = factory
        .agent_for_model(Some("other-model"))
        .expect("override agent");
    assert_eq!(override_agent.config().default_model, "other-model");
    // Everything except the model tag is shared with the node's runtime.
    assert!(Arc::ptr_eq(override_agent.memory(), &memory));
    assert!(Arc::ptr_eq(override_agent.provider(), &provider));

    // Repeated lookups reuse the cached agent instead of rebuilding one per message.
    let again = factory
        .agent_for_model(Some("other-model"))
        .expect("cached override agent");
    assert!(Arc::ptr_eq(&override_agent, &again));
}

#[test]
fn changing_the_provider_drops_overrides_built_for_the_previous_one() {
    let (factory, first_provider, _memory, _sessions, _personas) = factory();
    factory.install("node", first_provider, config("default-model"));
    let stale = factory
        .agent_for_model(Some("other-model"))
        .expect("override agent");

    let second_provider: Arc<dyn LlmProvider> = Arc::new(StubProvider);
    factory.install("node", second_provider.clone(), config("default-model"));

    let rebuilt = factory
        .agent_for_model(Some("other-model"))
        .expect("override agent after provider change");
    assert!(
        !Arc::ptr_eq(&stale, &rebuilt),
        "an override must not survive the provider it was built for"
    );
    assert!(Arc::ptr_eq(rebuilt.provider(), &second_provider));
}

#[test]
fn clearing_the_provider_removes_the_node_and_its_overrides() {
    let (factory, provider, _memory, _sessions, _personas) = factory();
    factory.install("node", provider, config("default-model"));
    assert!(factory.agent_for_model(Some("other-model")).is_some());

    factory.clear();

    assert!(factory.node_agent().is_none());
    assert!(factory.agent_for_model(Some("other-model")).is_none());
}

#[test]
fn build_with_keeps_the_node_runtime_parts() {
    let (factory, _provider, memory, sessions, personas) = factory();
    factory.install("node", Arc::new(StubProvider), config("default-model"));

    let ephemeral_provider: Arc<dyn LlmProvider> = Arc::new(StubProvider);
    let agent = factory.build_with(ephemeral_provider.clone(), config("sandbox-model"));

    assert_eq!(agent.config().default_model, "sandbox-model");
    assert!(Arc::ptr_eq(agent.memory(), &memory));
    assert!(Arc::ptr_eq(agent.provider(), &ephemeral_provider));
    // The sandbox agent must still see the node's sessions and personas.
    assert!(agent.session_manager().is_some());
    assert!(agent.persona_registry().is_some());
    assert!(Arc::ptr_eq(
        agent.session_manager().expect("session manager"),
        &sessions
    ));
    assert!(Arc::ptr_eq(
        agent.persona_registry().expect("persona registry"),
        &personas
    ));
}
