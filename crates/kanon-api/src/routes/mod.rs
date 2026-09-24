//! RESTful management routes (`/api/v1`).
//!
//! Each submodule owns one resource family and exposes a `routes()` constructor returning a
//! `Router<ApiState>` fragment. [`api_router`] merges the fragments so the gateway can be
//! assembled once in [`crate::server`] and reused verbatim by integration tests.
//!
//! Endpoint map (mirrors the management gateway spec):
//!
//! | Method | Path | Purpose |
//! | :--- | :--- | :--- |
//! | `GET` | `/api/v1/health` | Liveness, uptime, memory footprint |
//! | `GET` | `/api/v1/metrics` | Prometheus text exposition |
//! | `GET` | `/api/v1/adapters` | Platform adapter catalog (built-in and plugin) |
//! | `POST` | `/api/v1/adapters/:platform/ingest` | Fast-ACK inbound message ingress |
//! | `GET` | `/api/v1/plugins` | Plugin and host catalog |
//! | `GET` | `/api/v1/plugins/:id/config` | Current values plus declaration schema |
//! | `PUT` | `/api/v1/plugins/:id/config` | Validate, hot reload, then persist |
//! | `POST` | `/api/v1/plugins/:id/restart` | Restart the owning host process |
//! | `GET` | `/api/v1/sessions` | Paginated session metadata |
//! | `POST` | `/api/v1/sessions/:id/reset` | Clear history, keep persona and variables |
//! | `POST` | `/api/v1/sessions/:id/persona` | Hot-swap the session persona |
//! | `GET` | `/api/v1/personas` | Persona catalog |
//! | `POST` | `/api/v1/chat/completions` | Sandbox chat with JSON or SSE responses |

pub mod adapters;
pub mod chat;
pub mod health;
pub mod metrics;
pub mod personas;
pub mod plugins;
pub mod sessions;

use axum::Router;

use crate::error::ApiError;
use crate::state::ApiState;

/// Assembles every `/api/v1` REST route fragment.
pub fn api_router() -> Router<ApiState> {
    Router::new()
        .merge(health::routes())
        .merge(metrics::routes())
        .merge(adapters::routes())
        .merge(plugins::routes())
        .merge(sessions::routes())
        .merge(personas::routes())
        .merge(chat::routes())
}

/// Fallback handler returning a structured `404` for unknown paths.
///
/// A JSON body is returned instead of Axum's empty default so console clients can rely on a
/// single error envelope for every failure, including routing mistakes.
pub async fn not_found() -> ApiError {
    ApiError::NotFound("No such management endpoint".to_string())
}
