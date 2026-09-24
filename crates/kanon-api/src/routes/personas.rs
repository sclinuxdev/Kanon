//! Persona catalog routes (`GET /api/v1/personas`).
//!
//! Personas are served straight from the shared [`kanon_llm::PersonaRegistry`], which is the
//! same registry the agent consults through its dynamic prompt hook. A persona registered at
//! runtime therefore becomes switchable immediately, with no gateway restart.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;

use crate::state::ApiState;

/// Registers the persona catalog route.
pub fn routes() -> Router<ApiState> {
    Router::new().route("/api/v1/personas", get(list_personas))
}

/// Serializable description of a registered persona.
#[derive(Debug, Serialize)]
pub struct PersonaView {
    /// Persona identifier used by the session persona endpoint.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Description of tone and capabilities.
    pub description: String,
    /// Raw system prompt template, including `{{variable}}` slots.
    pub template: String,
    /// Variable names referenced by the template.
    pub variables: Vec<String>,
    /// Variable names without a default value.
    pub required_variables: Vec<String>,
    /// Preferred sampling temperature, when declared.
    pub default_temperature: Option<f32>,
    /// Preferred model family, when declared.
    pub default_model: Option<String>,
}

/// Persona catalog payload.
#[derive(Debug, Serialize)]
pub struct PersonaCatalog {
    /// Total number of registered personas.
    pub total: usize,
    /// Registered personas, sorted by identifier for stable console rendering.
    pub personas: Vec<PersonaView>,
}

/// Lists every built-in and dynamically registered persona.
async fn list_personas(State(state): State<ApiState>) -> Json<PersonaCatalog> {
    let mut personas: Vec<PersonaView> = state
        .personas()
        .list()
        .into_iter()
        .map(|persona| PersonaView {
            id: persona.id,
            name: persona.name,
            description: persona.description,
            template: persona.template.raw().to_string(),
            variables: persona.template.variables(),
            required_variables: persona.template.required_variables(),
            default_temperature: persona.default_temperature,
            default_model: persona.default_model,
        })
        .collect();

    personas.sort_by(|a, b| a.id.cmp(&b.id));

    Json(PersonaCatalog {
        total: personas.len(),
        personas,
    })
}
