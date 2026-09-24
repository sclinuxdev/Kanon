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

use crate::error::ApiError;
use crate::routes;
use crate::state::ApiState;
use crate::ws;

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
        .fallback(routes::not_found)
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
