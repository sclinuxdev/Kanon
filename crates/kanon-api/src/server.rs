//! HTTP server bootstrap and router assembly.
//!
//! [`app`] builds the complete gateway router (REST + WebSocket + middleware) and is reused
//! verbatim by integration tests, so the tested surface is exactly the deployed surface.
//! [`ApiServer`] owns the bound listener separately from its serving future, which lets callers
//! discover the effective port (useful with port `0`) before traffic starts.

use std::future::Future;
use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use axum::http::Uri;
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::error::ApiError;
use crate::routes;
use crate::state::ApiState;
use crate::ws;

/// Static web assets embedded into the microkernel binary.
#[derive(RustEmbed)]
#[folder = "../../webui/dist/"]
struct WebUiAssets;

/// Fallback route handler that serves embedded static WebUI assets with SPA fallback,
/// while guaranteeing structured JSON 404 responses for unmatched `/api/` and `/ws/` paths.
async fn static_or_not_found(uri: Uri) -> Response {
    let path = uri.path();

    // Preserve JSON 404 envelope for any unmatched API or WebSocket routes
    if path.starts_with("/api/") || path.starts_with("/ws/") {
        return routes::not_found().await.into_response();
    }

    let trimmed = path.trim_start_matches('/');
    let target = if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    };

    if let Some(asset) = WebUiAssets::get(target) {
        let mime = mime_guess::from_path(target).first_or_octet_stream();
        let cache_control = if target.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };

        return (
            [
                (axum::http::header::CONTENT_TYPE, mime.as_ref()),
                (axum::http::header::CACHE_CONTROL, cache_control),
            ],
            asset.data,
        )
            .into_response();
    }

    // SPA fallback: client-side routing routes (e.g. /overview, /plugins, /settings)
    // resolve to index.html with 200 OK.
    if let Some(index) = WebUiAssets::get("index.html") {
        return (
            [
                (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (axum::http::header::CACHE_CONTROL, "no-cache"),
            ],
            index.data,
        )
            .into_response();
    }

    routes::not_found().await.into_response()
}

/// Builds the complete management gateway router.
///
/// CORS is permissive by design: the WebUI is a separately deployed frontend (static hosting,
/// Docker Compose or Vercel) whose origin cannot be known ahead of time. Request tracing is
/// delegated to `tower_http` so every management call appears in the structured log stream
/// that `/ws/v1/logs` already broadcasts.
pub fn app(state: ApiState) -> Router {
    Router::new()
        .merge(routes::api_router())
        .merge(ws::routes())
        .fallback(static_or_not_found)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Bound management gateway awaiting its serving future.
pub struct ApiServer {
    listener: TcpListener,
    state: ApiState,
    local_addr: SocketAddr,
}

impl ApiServer {
    /// Binds the gateway to `addr` without starting to serve.
    pub async fn bind(addr: SocketAddr, state: ApiState) -> Result<Self, ApiError> {
        let listener = TcpListener::bind(addr).await.map_err(|err| {
            ApiError::Internal(format!(
                "Failed to bind management gateway on {addr}: {err}"
            ))
        })?;

        let local_addr = listener
            .local_addr()
            .map_err(|err| ApiError::Internal(format!("Failed to read bound address: {err}")))?;

        Ok(Self {
            listener,
            state,
            local_addr,
        })
    }

    /// Returns the effective bound address (resolved even when port `0` was requested).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serves requests until `shutdown` resolves, then drains in-flight connections.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), ApiError> {
        let router = app(self.state);
        let addr = self.local_addr;

        tracing::info!(address = %addr, "Management gateway listening");

        axum::serve(self.listener, router)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|err| ApiError::Internal(format!("Management gateway failed: {err}")))
    }
}
