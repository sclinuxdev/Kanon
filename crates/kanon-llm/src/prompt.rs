//! Prompt Templating, Composition, and Persona System.
//!
//! Provides:
//! - [`PromptTemplate`]: Template interpolation with variable slots (`{{var}}`, `{{var|default}}`);
//! - [`PromptComposer`]: Multi-layer modular system prompt assembly;
//! - [`Persona`]: Structured agent personality specification with model preferences;
//! - [`PersonaRegistry`]: Thread-safe catalog of built-in and dynamic personas;
//! - [`DynamicPromptHook`]: Lifecycle hook automatically binding session personas to LLM turns.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::agent::AgentHook;
use crate::error::AgentError;
use crate::gateway::types::{ChatMessage, ChatRequest, Role};
use crate::session::SessionManager;

/// Parsed segment of a prompt template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplateSegment {
    /// Static literal text.
    Literal(String),
    /// Variable slot with optional default value (`{{variable|default}}`).
    Slot {
        name: String,
        default_value: Option<String>,
    },
}

/// Prompt template supporting dynamic slot interpolation.
///
/// Syntax:
/// - `{{var_name}}`: Replaced with value of `var_name`.
/// - `{{var_name|fallback}}`: Replaced with value of `var_name`, or `fallback` if unset.
///
/// # Example:
/// ```rust
/// use kanon_llm::prompt::PromptTemplate;
/// use std::collections::HashMap;
///
/// let tmpl = PromptTemplate::parse("Hello {{user_name|friend}}! Welcome to {{platform|Kanon}}.");
/// let mut vars = HashMap::new();
/// vars.insert("user_name".to_string(), "Alice".to_string());
/// assert_eq!(tmpl.render(&vars), "Hello Alice! Welcome to Kanon.");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptTemplate {
    raw: String,
    #[serde(skip)]
    segments: Vec<TemplateSegment>,
}

impl PromptTemplate {
    /// Parses a raw template string into compiled template segments.
    pub fn parse(raw: impl Into<String>) -> Self {
        let raw_str = raw.into();
        let segments = Self::compile_segments(&raw_str);
        Self {
            raw: raw_str,
            segments,
        }
    }

    /// Internal parser identifying `{{ ... }}` variable blocks.
    fn compile_segments(raw: &str) -> Vec<TemplateSegment> {
        let mut segments = Vec::new();
        let mut rest = raw;

        while let Some(start_idx) = rest.find("{{") {
            // Push any preceding literal text
            if start_idx > 0 {
                segments.push(TemplateSegment::Literal(rest[..start_idx].to_string()));
            }

            let after_open = &rest[start_idx + 2..];
            if let Some(end_idx) = after_open.find("}}") {
                let slot_content = after_open[..end_idx].trim();
                if let Some((name, default_val)) = slot_content.split_once('|') {
                    segments.push(TemplateSegment::Slot {
                        name: name.trim().to_string(),
                        default_value: Some(default_val.trim().to_string()),
                    });
                } else {
                    segments.push(TemplateSegment::Slot {
                        name: slot_content.to_string(),
                        default_value: None,
                    });
                }
                rest = &after_open[end_idx + 2..];
            } else {
                // Unclosed opening bracket, treat rest as literal
                segments.push(TemplateSegment::Literal(rest.to_string()));
                rest = "";
                break;
            }
        }

        if !rest.is_empty() {
            segments.push(TemplateSegment::Literal(rest.to_string()));
        }

        segments
    }

    /// Renders the template by interpolating available variables.
    ///
    /// If a variable is missing and has no default value, it renders as an empty string.
    pub fn render(&self, variables: &HashMap<String, String>) -> String {
        self.render_with_fallback(variables, "")
    }

    /// Renders the template with a custom fallback string for unresolved slots without defaults.
    pub fn render_with_fallback(
        &self,
        variables: &HashMap<String, String>,
        fallback: &str,
    ) -> String {
        // Re-compile segments if deserialized without segments
        let segs = if self.segments.is_empty() && !self.raw.is_empty() {
            Self::compile_segments(&self.raw)
        } else {
            self.segments.clone()
        };

        let mut output = String::new();
        for seg in &segs {
            match seg {
                TemplateSegment::Literal(lit) => output.push_str(lit),
                TemplateSegment::Slot {
                    name,
                    default_value,
                } => {
                    if let Some(val) = variables.get(name) {
                        output.push_str(val);
                    } else if let Some(def) = default_value {
                        output.push_str(def);
                    } else {
                        output.push_str(fallback);
                    }
                }
            }
        }
        output
    }

    /// Returns the raw unparsed template string.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns a list of all variable names referenced in this template.
    pub fn variables(&self) -> Vec<String> {
        let mut set = HashSet::new();
        for seg in &self.segments {
            if let TemplateSegment::Slot { name, .. } = seg {
                set.insert(name.clone());
            }
        }
        set.into_iter().collect()
    }

    /// Returns a list of required variable names (slots without defaults).
    pub fn required_variables(&self) -> Vec<String> {
        let mut set = HashSet::new();
        for seg in &self.segments {
            if let TemplateSegment::Slot {
                name,
                default_value: None,
            } = seg
            {
                set.insert(name.clone());
            }
        }
        set.into_iter().collect()
    }
}

impl From<&str> for PromptTemplate {
    fn from(s: &str) -> Self {
        PromptTemplate::parse(s)
    }
}

impl From<String> for PromptTemplate {
    fn from(s: String) -> Self {
        PromptTemplate::parse(s)
    }
}

/// Multi-layer modular system prompt composer.
///
/// Builds a clean, structured system prompt by cleanly assembling:
/// 1. Identity / Persona;
/// 2. Behavioral Instructions;
/// 3. Negative Constraints / Safety boundaries;
/// 4. Dynamic Context Attributes;
/// 5. Tool Usage Guidelines.
#[derive(Debug, Clone, Default)]
pub struct PromptComposer {
    identity: Option<String>,
    instructions: Vec<String>,
    constraints: Vec<String>,
    context_items: Vec<(String, String)>,
    tool_guidelines: Option<String>,
}

impl PromptComposer {
    /// Creates a new empty `PromptComposer`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the base agent identity / persona.
    pub fn identity(mut self, text: impl Into<String>) -> Self {
        self.identity = Some(text.into());
        self
    }

    /// Appends a behavioral instruction.
    pub fn instruction(mut self, text: impl Into<String>) -> Self {
        self.instructions.push(text.into());
        self
    }

    /// Appends multiple behavioral instructions.
    pub fn instructions(mut self, texts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for t in texts {
            self.instructions.push(t.into());
        }
        self
    }

    /// Appends a constraint or guardrail rule.
    pub fn constraint(mut self, text: impl Into<String>) -> Self {
        self.constraints.push(text.into());
        self
    }

    /// Appends multiple constraint rules.
    pub fn constraints(mut self, texts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for t in texts {
            self.constraints.push(t.into());
        }
        self
    }

    /// Injects a dynamic context attribute (e.g. ("User", "Alice"), ("Channel", "#general")).
    pub fn context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context_items.push((key.into(), value.into()));
        self
    }

    /// Injects directives on tool usage.
    pub fn tool_guidelines(mut self, text: impl Into<String>) -> Self {
        self.tool_guidelines = Some(text.into());
        self
    }

    /// Composes the structured prompt into a unified system prompt string.
    pub fn compose(&self) -> String {
        let mut sections = Vec::new();

        if let Some(ref id) = self.identity {
            sections.push(id.trim().to_string());
        }

        if !self.context_items.is_empty() {
            let mut ctx_block = String::from("## Current Context\n");
            for (k, v) in &self.context_items {
                ctx_block.push_str(&format!("- {k}: {v}\n"));
            }
            sections.push(ctx_block.trim_end().to_string());
        }

        if !self.instructions.is_empty() {
            let mut inst_block = String::from("## Instructions\n");
            for inst in &self.instructions {
                inst_block.push_str(&format!("- {inst}\n"));
            }
            sections.push(inst_block.trim_end().to_string());
        }

        if let Some(ref tg) = self.tool_guidelines {
            let mut tool_block = String::from("## Tool Guidelines\n");
            tool_block.push_str(tg.trim());
            sections.push(tool_block);
        }

        if !self.constraints.is_empty() {
            let mut constr_block = String::from("## Constraints & Safety\n");
            for c in &self.constraints {
                constr_block.push_str(&format!("- {c}\n"));
            }
            sections.push(constr_block.trim_end().to_string());
        }

        sections.join("\n\n")
    }
}

/// Agent persona specification.
///
/// Encapsulates a distinct personality, system prompt template, model defaults,
/// and baseline variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    /// Unique identifier slug (e.g. `"assistant"`, `"coder"`, `"translator"`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Short description of this persona's capabilities and tone.
    pub description: String,
    /// Base system prompt template with variable slots.
    pub template: PromptTemplate,
    /// Optional model sampling temperature preferred by this persona.
    pub default_temperature: Option<f32>,
    /// Optional model family preference.
    pub default_model: Option<String>,
    /// Default variables bundled with this persona.
    pub default_variables: HashMap<String, String>,
}

impl Persona {
    /// Creates a new `Persona` with the specified identity and prompt template.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        template: impl Into<PromptTemplate>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            template: template.into(),
            default_temperature: None,
            default_model: None,
            default_variables: HashMap::new(),
        }
    }

    /// Sets preferred sampling temperature.
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.default_temperature = Some(temp);
        self
    }

    /// Sets preferred model identifier.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }

    /// Binds a default variable to this persona.
    pub fn with_variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_variables.insert(key.into(), value.into());
        self
    }

    /// Renders the persona's system prompt, merging default variables with runtime overrides.
    pub fn render_prompt(&self, runtime_vars: &HashMap<String, String>) -> String {
        let mut merged = self.default_variables.clone();
        for (k, v) in runtime_vars {
            merged.insert(k.clone(), v.clone());
        }
        self.template.render(&merged)
    }
}

/// Thread-safe registry and catalog of personas.
///
/// Pre-populated with standard production-ready personas, and supports
/// dynamic registration of custom personas at runtime.
pub struct PersonaRegistry {
    personas: DashMap<String, Persona>,
}

impl Default for PersonaRegistry {
    fn default() -> Self {
        let registry = Self {
            personas: DashMap::new(),
        };

        // 1. General Assistant
        registry.register(
            Persona::new(
                "assistant",
                "General Assistant",
                "Helpful, friendly, and reliable conversational assistant",
                "You are {{bot_name|Kanon}}, a helpful, intelligent, and versatile AI assistant. \
             Respond clearly, thoughtfully, and concisely.",
            )
            .with_temperature(0.7),
        );

        // 2. Coder / Software Architect
        registry.register(
            Persona::new(
                "coder",
                "Code Architect",
                "Senior systems architect and programmer providing idiomatic, robust code",
                "You are {{bot_name|Kanon}}, an expert software engineer and systems architect. \
             Provide correct, idiomatic, and highly efficient code. Always explain non-obvious \
             trade-offs and handle edge cases thoroughly.",
            )
            .with_temperature(0.2),
        );

        // 3. Translator
        registry.register(
            Persona::new(
                "translator",
                "Multilingual Translator",
                "Professional translator preserving tone, cultural nuance, and technical precision",
                "You are a professional multilingual translator. Translate text accurately while \
             preserving the original tone, context, and domain-specific terminology. Output only \
             the clean translation without conversational filler.",
            )
            .with_temperature(0.3),
        );

        // 4. Concise / Minimalist
        registry.register(Persona::new(
            "concise",
            "Minimalist Assistant",
            "Ultra-brief, direct, no-fluff factual assistant",
            "You are {{bot_name|Kanon}}. Be direct, factual, and strictly concise. \
             Omit pleasantries, preamble, and conversational filler. Provide only the essential answer.",
        ).with_temperature(0.2));

        // 5. Creative Companion
        registry.register(
            Persona::new(
                "creative",
                "Creative Companion",
                "Imaginative ideation and creative writing partner",
                "You are {{bot_name|Kanon}}, an imaginative and expressive creative partner. \
             Embrace novelty, vivid metaphors, and diverse viewpoints to inspire the user.",
            )
            .with_temperature(0.9),
        );

        registry
    }
}

impl PersonaRegistry {
    /// Creates an empty `PersonaRegistry` without built-in presets.
    pub fn empty() -> Self {
        Self {
            personas: DashMap::new(),
        }
    }

    /// Registers a persona in the registry.
    pub fn register(&self, persona: Persona) {
        self.personas.insert(persona.id.clone(), persona);
    }

    /// Retrieves a persona by its identifier slug.
    pub fn get(&self, id: &str) -> Option<Persona> {
        self.personas.get(id).map(|p| p.clone())
    }

    /// Removes a persona from the registry.
    pub fn remove(&self, id: &str) -> Option<Persona> {
        self.personas.remove(id).map(|(_, p)| p)
    }

    /// Returns a list of all registered personas.
    pub fn list(&self) -> Vec<Persona> {
        self.personas.iter().map(|p| p.clone()).collect()
    }

    /// Returns the number of registered personas.
    pub fn len(&self) -> usize {
        self.personas.len()
    }

    /// Returns `true` if no personas are registered.
    pub fn is_empty(&self) -> bool {
        self.personas.is_empty()
    }
}

/// Function type for injecting runtime context variables into dynamic prompt templates.
pub type RuntimeVariablesFn = Arc<dyn Fn(&str) -> HashMap<String, String> + Send + Sync>;

/// Lifecycle hook that dynamically resolves and binds the active persona
/// and prompt template for each session turn.
pub struct DynamicPromptHook {
    session_manager: Arc<SessionManager>,
    persona_registry: Arc<PersonaRegistry>,
    default_persona_id: String,
    runtime_vars_fn: Option<RuntimeVariablesFn>,
}

impl DynamicPromptHook {
    /// Constructs a new `DynamicPromptHook`.
    pub fn new(
        session_manager: Arc<SessionManager>,
        persona_registry: Arc<PersonaRegistry>,
    ) -> Self {
        Self {
            session_manager,
            persona_registry,
            default_persona_id: "assistant".to_string(),
            runtime_vars_fn: None,
        }
    }

    /// Sets the fallback persona used when a session has no explicit persona set.
    pub fn with_default_persona(mut self, persona_id: impl Into<String>) -> Self {
        self.default_persona_id = persona_id.into();
        self
    }

    /// Configures a dynamic runtime context provider (e.g. injects current time, channel name).
    pub fn with_runtime_vars<F>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> HashMap<String, String> + Send + Sync + 'static,
    {
        self.runtime_vars_fn = Some(Arc::new(f));
        self
    }
}

#[async_trait]
impl AgentHook for DynamicPromptHook {
    async fn on_llm_request(
        &self,
        session_id: &str,
        request: &mut ChatRequest,
    ) -> Result<(), AgentError> {
        let persona_id = self
            .session_manager
            .get_persona(session_id)
            .unwrap_or_else(|| self.default_persona_id.clone());

        if let Some(persona) = self.persona_registry.get(&persona_id) {
            // Aggregate variables: session variables + runtime variables
            let mut vars = self.session_manager.get_variables(session_id);
            if let Some(ref provider) = self.runtime_vars_fn {
                let runtime = provider(session_id);
                for (k, v) in runtime {
                    vars.insert(k, v);
                }
            }

            let rendered_system_prompt = persona.render_prompt(&vars);

            // Update model temperature default if unset on request
            if request.temperature.is_none() && persona.default_temperature.is_some() {
                request.temperature = persona.default_temperature;
            }

            // Update or inject System prompt into messages
            if let Some(first_msg) = request.messages.first_mut() {
                if first_msg.role == Role::System {
                    first_msg.content = Some(rendered_system_prompt);
                } else {
                    request
                        .messages
                        .insert(0, ChatMessage::system(rendered_system_prompt));
                }
            } else {
                request
                    .messages
                    .push(ChatMessage::system(rendered_system_prompt));
            }
        }

        Ok(())
    }
}
