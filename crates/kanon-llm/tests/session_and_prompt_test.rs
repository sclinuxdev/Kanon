//! Comprehensive integration tests for Kanon Session Management and Prompt/Persona System.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::RwLock;

use kanon_llm::agent::Agent;
use kanon_llm::error::GatewayError;
use kanon_llm::gateway::LlmProvider;
use kanon_llm::gateway::types::{ChatMessage, ChatRequest, ChatResponse, Role};
use kanon_llm::memory::SlidingWindowMemory;
use kanon_llm::prompt::{
    DynamicPromptHook, Persona, PersonaRegistry, PromptComposer, PromptTemplate,
};
use kanon_llm::session::{SessionKey, SessionManager, SessionScope, SessionStatus};

/// Hook that appends one system message, standing in for the skill catalog / RAG style hooks.
struct AppendingHook;

#[async_trait]
impl kanon_llm::agent::AgentHook for AppendingHook {
    async fn on_llm_request(
        &self,
        _session_id: &str,
        request: &mut ChatRequest,
    ) -> Result<(), kanon_llm::AgentError> {
        let position = request
            .messages
            .iter()
            .rposition(|message| message.role == Role::System)
            .map(|index| index + 1)
            .unwrap_or(0);
        request
            .messages
            .insert(position, ChatMessage::system("INJECTED-CONTEXT"));
        Ok(())
    }
}

/// Provider that records the messages of the request it receives.
struct RecordingProvider {
    seen: Arc<std::sync::Mutex<Vec<ChatMessage>>>,
}

#[async_trait]
impl LlmProvider for RecordingProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        *self.seen.lock().expect("recording lock") = request.messages.clone();
        Ok(ChatResponse {
            content: Some("ok".to_string()),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn injected_system_context_survives_the_persona_hook() {
    // The persona hook rewrites the first system message in place. If it ran *after* a hook that
    // appends context, the injected context would be overwritten — which is exactly how the skill
    // catalog silently disappeared for sessions that had no system prompt yet.
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let memory = Arc::new(SlidingWindowMemory::new(20));
    let sessions = Arc::new(SessionManager::new(memory.clone()));
    let personas = Arc::new(PersonaRegistry::default());
    sessions.set_persona("session-1", "assistant");

    let agent = Agent::builder(
        "hook-order",
        Arc::new(RecordingProvider { seen: seen.clone() }),
    )
    .memory(memory)
    .session_manager(sessions)
    .persona_registry(personas)
    .hook(AppendingHook)
    .model("test-model")
    .build();

    agent.run("session-1", "hello", &[]).await.expect("run");

    let messages = seen.lock().expect("recording lock").clone();
    assert_eq!(
        messages.len(),
        3,
        "persona + injected context + user: {messages:?}"
    );
    assert_eq!(messages[0].role, Role::System);
    assert!(
        messages[0]
            .content
            .as_deref()
            .is_some_and(|content| content.contains("Kanon")),
        "the persona must own the base system message: {messages:?}"
    );
    assert_eq!(messages[1].role, Role::System);
    assert_eq!(messages[1].content.as_deref(), Some("INJECTED-CONTEXT"));
    assert_eq!(messages[2].role, Role::User);
}

// =========================================================================
// 1. Session Management Tests
// =========================================================================

#[test]
fn test_session_key_scoping_and_display() {
    let u_key = SessionKey::user("usr_42");
    assert_eq!(u_key.as_str(), "user:usr_42");
    assert_eq!(*u_key.scope(), SessionScope::User);
    assert_eq!(format!("{u_key}"), "user:usr_42");

    let c_key = SessionKey::channel("chan_general");
    assert_eq!(c_key.as_str(), "channel:chan_general");
    assert_eq!(*c_key.scope(), SessionScope::Channel);

    let cu_key = SessionKey::channel_user("chan_123", "usr_999");
    assert_eq!(cu_key.as_str(), "channel:chan_123:user:usr_999");
    assert_eq!(*cu_key.scope(), SessionScope::ChannelUser);

    let t_key = SessionKey::thread("chan_123", "th_root");
    assert_eq!(t_key.as_str(), "channel:chan_123:thread:th_root");
    assert_eq!(*t_key.scope(), SessionScope::Thread);

    let legacy = SessionKey::from_legacy("group_1", "user_1");
    assert_eq!(legacy.as_str(), "group_1:user_1");
    assert_eq!(*legacy.scope(), SessionScope::ChannelUser);
}

#[tokio::test]
async fn test_session_manager_metadata_and_variable_lifecycle() {
    let memory = Arc::new(SlidingWindowMemory::new(10));
    let sm = Arc::new(SessionManager::new(memory));

    let key = "channel:g1:user:u1";

    // 1. Get or create initializes metadata
    let meta = sm.get_or_create(key);
    assert_eq!(meta.session_key, key);
    assert_eq!(meta.status, SessionStatus::Active);
    assert_eq!(meta.turn_count, 0);
    assert_eq!(meta.total_tokens_used, 0);
    assert!(meta.persona_id.is_none());

    // 2. Set and query session variables
    sm.set_variable(key, "lang", "zh-CN");
    sm.set_variable(key, "timezone", "Asia/Shanghai");

    assert_eq!(sm.get_variable(key, "lang").as_deref(), Some("zh-CN"));
    assert_eq!(
        sm.get_variable(key, "timezone").as_deref(),
        Some("Asia/Shanghai")
    );

    let all_vars = sm.get_variables(key);
    assert_eq!(all_vars.len(), 2);
    assert_eq!(all_vars.get("lang").map(String::as_str), Some("zh-CN"));

    // Remove variable
    let removed = sm.remove_variable(key, "timezone");
    assert_eq!(removed.as_deref(), Some("Asia/Shanghai"));
    assert!(sm.get_variable(key, "timezone").is_none());

    // 3. Record interaction turns
    sm.record_turn(key, 150);
    sm.record_turn(key, 250);

    let updated = sm.get_metadata(key).expect("Metadata must exist");
    assert_eq!(updated.turn_count, 2);
    assert_eq!(updated.total_tokens_used, 400);
    assert_eq!(updated.status, SessionStatus::Active);

    // 4. Set persona
    sm.set_persona(key, "coder");
    assert_eq!(sm.get_persona(key).as_deref(), Some("coder"));

    // 5. Reset session clears memory and turn counts, but preserves persona & variables
    sm.memory()
        .push_message(key, ChatMessage::user("Hello"))
        .await
        .unwrap();
    assert_eq!(sm.memory().get_messages(key).await.unwrap().len(), 1);

    sm.reset_session(key).await.expect("Reset should succeed");

    assert!(sm.memory().get_messages(key).await.unwrap().is_empty());
    let after_reset = sm.get_metadata(key).unwrap();
    assert_eq!(after_reset.turn_count, 0);
    assert_eq!(after_reset.total_tokens_used, 0);
    assert_eq!(after_reset.persona_id.as_deref(), Some("coder"));
    assert_eq!(sm.get_variable(key, "lang").as_deref(), Some("zh-CN"));

    // 6. Close session
    sm.close_session(key);
    assert_eq!(sm.get_metadata(key).unwrap().status, SessionStatus::Closed);
}

#[tokio::test]
async fn test_session_manager_idle_sweep() {
    let memory = Arc::new(SlidingWindowMemory::new(10));
    let sm = Arc::new(SessionManager::new(memory));

    let active_key = "session:active";
    let idle_key = "session:idle";

    sm.get_or_create(active_key);
    sm.get_or_create(idle_key);

    assert_eq!(sm.active_session_count(), 2);

    // Manually artificially simulate age on idle_key
    if let Some(mut meta) = sm.get_metadata(idle_key) {
        meta.last_active_at = meta.last_active_at.saturating_sub(3600);
        // We simulate this by checking sweep with a short duration
    }

    // Sweep with 0 second threshold will transition all sessions whose last_active is <= now
    let swept = sm.sweep_idle_sessions(Duration::from_secs(0));
    assert_eq!(swept, 2);
    assert_eq!(sm.active_session_count(), 0);

    // Record turn reactivates session
    sm.record_turn(active_key, 50);
    assert_eq!(sm.active_session_count(), 1);
    assert_eq!(
        sm.get_metadata(active_key).unwrap().status,
        SessionStatus::Active
    );
}

// =========================================================================
// 2. Prompt Templating and Composition Tests
// =========================================================================

#[test]
fn test_prompt_template_parsing_and_rendering() {
    let template_str =
        "You are {{bot_name|Kanon}}, an assistant for {{user|friend}}. Mode: {{mode}}.";
    let tmpl = PromptTemplate::parse(template_str);

    assert_eq!(tmpl.raw(), template_str);

    let vars_set = tmpl.variables();
    assert_eq!(vars_set.len(), 3);
    assert!(vars_set.contains(&"bot_name".to_string()));
    assert!(vars_set.contains(&"user".to_string()));
    assert!(vars_set.contains(&"mode".to_string()));

    let required = tmpl.required_variables();
    assert_eq!(required, vec!["mode".to_string()]);

    // 1. Render with defaults for bot_name and user
    let mut vars = HashMap::new();
    vars.insert("mode".to_string(), "autonomous".to_string());
    let rendered = tmpl.render(&vars);
    assert_eq!(
        rendered,
        "You are Kanon, an assistant for friend. Mode: autonomous."
    );

    // 2. Render overriding defaults
    vars.insert("bot_name".to_string(), "Sentinel".to_string());
    vars.insert("user".to_string(), "Commander".to_string());
    let rendered_custom = tmpl.render(&vars);
    assert_eq!(
        rendered_custom,
        "You are Sentinel, an assistant for Commander. Mode: autonomous."
    );

    // 3. Render with missing required slot and custom fallback
    let empty_vars = HashMap::new();
    let fallback_rendered = tmpl.render_with_fallback(&empty_vars, "[UNSET]");
    assert_eq!(
        fallback_rendered,
        "You are Kanon, an assistant for friend. Mode: [UNSET]."
    );
}

#[test]
fn test_prompt_composer_layered_assembly() {
    let prompt = PromptComposer::new()
        .identity("You are Kanon, a secure high-performance chatbot core.")
        .context("Channel", "#production-alerts")
        .context("Operator", "SysAdmin")
        .instruction("Analyze incoming log traces and highlight fatal anomalies.")
        .instruction("Output remediation commands when applicable.")
        .tool_guidelines("Call diagnostic tools before suggesting manual intervention.")
        .constraint("Never execute destructive shell commands without operator confirmation.")
        .constraint("Output strictly in Markdown format.")
        .compose();

    assert!(prompt.contains("You are Kanon, a secure high-performance chatbot core."));
    assert!(prompt.contains("## Current Context"));
    assert!(prompt.contains("- Channel: #production-alerts"));
    assert!(prompt.contains("- Operator: SysAdmin"));
    assert!(prompt.contains("## Instructions"));
    assert!(prompt.contains("- Analyze incoming log traces and highlight fatal anomalies."));
    assert!(prompt.contains("## Tool Guidelines"));
    assert!(prompt.contains("Call diagnostic tools before suggesting manual intervention."));
    assert!(prompt.contains("## Constraints & Safety"));
    assert!(
        prompt
            .contains("- Never execute destructive shell commands without operator confirmation.")
    );
}

// =========================================================================
// 3. Persona Registry Tests
// =========================================================================

#[test]
fn test_persona_registry_presets_and_custom() {
    let registry = PersonaRegistry::default();

    // Verify built-in presets
    assert!(registry.get("assistant").is_some());
    assert!(registry.get("coder").is_some());
    assert!(registry.get("translator").is_some());
    assert!(registry.get("concise").is_some());
    assert!(registry.get("creative").is_some());

    let coder = registry.get("coder").unwrap();
    assert_eq!(coder.name, "Code Architect");
    assert_eq!(coder.default_temperature, Some(0.2));

    // Render coder prompt with custom bot name
    let mut vars = HashMap::new();
    vars.insert("bot_name".to_string(), "RustBot".to_string());
    let prompt = coder.render_prompt(&vars);
    assert!(prompt.contains("You are RustBot, an expert software engineer"));

    // Register custom persona
    let custom = Persona::new(
        "sre",
        "Site Reliability Engineer",
        "Triages production outages and manages Kubernetes clusters",
        "You are {{bot_name|Kanon}}, an SRE specializing in high-availability distributed systems.",
    )
    .with_temperature(0.1);

    registry.register(custom);
    assert_eq!(registry.len(), 6);
    assert!(registry.get("sre").is_some());

    // Remove persona
    let removed = registry.remove("sre");
    assert!(removed.is_some());
    assert!(registry.get("sre").is_none());
}

// =========================================================================
// 4. Dynamic Prompt Hook & Agent Integration Tests
// =========================================================================

struct RequestCapturingProvider {
    captured_requests: Arc<RwLock<Vec<ChatRequest>>>,
}

#[async_trait]
impl LlmProvider for RequestCapturingProvider {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, GatewayError> {
        self.captured_requests.write().await.push(request.clone());
        Ok(ChatResponse {
            content: Some("I have processed your request according to my persona.".to_string()),
            tool_calls: vec![],
            finish_reason: Some("stop".to_string()),
            usage: None,
        })
    }
}

#[tokio::test]
async fn test_agent_dynamic_prompt_hook_integration() {
    let captured = Arc::new(RwLock::new(Vec::new()));
    let provider = Arc::new(RequestCapturingProvider {
        captured_requests: captured.clone(),
    });

    let memory = Arc::new(SlidingWindowMemory::new(20));
    let session_mgr = Arc::new(SessionManager::new(memory));
    let persona_reg = Arc::new(PersonaRegistry::default());

    // Construct Agent with session_manager and persona_registry via builder
    let agent = Agent::builder("persona_agent", provider)
        .session_manager(session_mgr.clone())
        .persona_registry(persona_reg.clone())
        .build();

    let session_id = "chan_dev:user_bob";

    // 1. First run with default persona ("assistant") and custom variable
    session_mgr.set_variable(session_id, "bot_name", "KanonAI");

    let out1 = agent
        .run_standalone(session_id, "Hello assistant!")
        .await
        .expect("Agent execution should succeed");
    assert_eq!(
        out1.content,
        "I have processed your request according to my persona."
    );

    // Verify first request received rendered "assistant" prompt
    {
        let reqs = captured.read().await;
        assert_eq!(reqs.len(), 1);
        let first_msg = &reqs[0].messages[0];
        assert_eq!(first_msg.role, Role::System);
        let content = first_msg.content.as_deref().unwrap();
        assert!(
            content
                .contains("You are KanonAI, a helpful, intelligent, and versatile AI assistant.")
        );
        // Assistant persona default temperature is 0.7
        assert_eq!(reqs[0].temperature, Some(0.7));
    }

    // Verify session manager recorded the turn
    let meta = session_mgr.get_metadata(session_id).unwrap();
    assert_eq!(meta.turn_count, 1);
    assert!(meta.total_tokens_used > 0);

    // 2. Switch session persona dynamically to "coder"
    session_mgr.set_persona(session_id, "coder");

    let _out2 = agent
        .run_standalone(session_id, "Write a binary search algorithm in Rust")
        .await
        .expect("Second turn should succeed");

    // Verify second request dynamically switched to "coder" persona
    {
        let reqs = captured.read().await;
        assert_eq!(reqs.len(), 2);
        let second_sys = &reqs[1].messages[0];
        assert_eq!(second_sys.role, Role::System);
        let content = second_sys.content.as_deref().unwrap();
        assert!(
            content.contains("You are KanonAI, an expert software engineer and systems architect.")
        );
        // Coder persona default temperature is 0.2
        assert_eq!(reqs[1].temperature, Some(0.2));
    }

    // Verify turn count updated to 2
    assert_eq!(session_mgr.get_metadata(session_id).unwrap().turn_count, 2);
}

#[tokio::test]
async fn test_dynamic_prompt_hook_with_runtime_context_provider() {
    let captured = Arc::new(RwLock::new(Vec::new()));
    let provider = Arc::new(RequestCapturingProvider {
        captured_requests: captured.clone(),
    });

    let memory = Arc::new(SlidingWindowMemory::new(10));
    let session_mgr = Arc::new(SessionManager::new(memory));
    let persona_reg = Arc::new(PersonaRegistry::empty());

    // Register persona requiring runtime context
    persona_reg.register(Persona::new(
        "custom_bot",
        "Custom Bot",
        "Test bot",
        "Bot Name: {{bot_name}}. Server: {{server_name}}. Current User: {{current_user}}.",
    ));

    session_mgr.set_persona("sess_1", "custom_bot");
    session_mgr.set_variable("sess_1", "bot_name", "ClusterNode");

    // DynamicPromptHook with runtime context provider callback
    let hook = Arc::new(
        DynamicPromptHook::new(session_mgr.clone(), persona_reg.clone())
            .with_default_persona("custom_bot")
            .with_runtime_vars(|_session_id| {
                let mut map = HashMap::new();
                map.insert("server_name".to_string(), "us-west-prod-1".to_string());
                map.insert("current_user".to_string(), "admin_carol".to_string());
                map
            }),
    );

    let agent = Agent::builder("runtime_agent", provider)
        .memory(session_mgr.memory().clone())
        .hook_arc(hook)
        .build();

    agent
        .run_standalone("sess_1", "Status report")
        .await
        .unwrap();

    let reqs = captured.read().await;
    let sys_msg = &reqs[0].messages[0];
    let content = sys_msg.content.as_deref().unwrap();
    assert_eq!(
        content,
        "Bot Name: ClusterNode. Server: us-west-prod-1. Current User: admin_carol."
    );
}
