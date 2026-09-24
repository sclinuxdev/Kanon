//! Kanon Core IPC server implementation.
//!
//! Provides the central gRPC endpoint (`core.sock`) through which plugin hosts
//! communicate with the Core microkernel via [`BotApiService`].

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tonic::{Request, Response, Status};

use kanon_proto::v1::bot_api_service_server::{BotApiService, BotApiServiceServer};
use kanon_proto::v1::{
    GetStorageRequest, GetStorageResponse, IngestEventRequest, IngestEventResponse,
    LlmChunk, LlmRequest, RegisterHostRequest, RegisterHostResponse,
    SendMessageRequest, SendMessageResponse, SetStorageRequest, SetStorageResponse,
};
use kanon_transport::{core_socket_path, IpcListener};

use crate::adapter::{EventIngress, IngestError};
use kanon_llm::{ChatMessage, ChatRequest, LlmGateway};
use tokio_stream::StreamExt;

/// Default capacity for the inbound asynchronous event ingest queue.
///
/// Under high throughput, this queue serves as a backpressure boundary separating
/// external IM adapters from downstream LLM reasoning and plugin pipelines.
pub const DEFAULT_INGEST_QUEUE_CAPACITY: usize = 10_000;

/// Registered plugin host metadata retained in the Core state.
#[derive(Debug, Clone)]
pub struct HostRegistration {
    /// Unique identifier for the host process.
    pub host_id: String,
    /// Runtime language ("rust", "python", "typescript").
    pub runtime: String,
    /// IPC endpoint path or address for reaching this host.
    pub endpoint: String,
    /// List of plugin identifiers loaded by this host.
    pub loaded_plugin_ids: Vec<String>,
}

/// Core implementation of the [`BotApiService`] gRPC service.
#[derive(Debug, Clone)]
pub struct CoreApiService {
    /// Shared Fast-ACK ingest handle; also handed to platform adapters.
    ingress: EventIngress,
    /// Thread-safe registry of connected and registered plugin hosts.
    hosts: Arc<RwLock<HashMap<String, HostRegistration>>>,
    /// Optional shared LLM gateway instance for delegating model completions.
    gateway: Option<Arc<LlmGateway>>,
}

impl CoreApiService {
    /// Creates a new `CoreApiService` with the specified event queue sender.
    ///
    /// Accepts anything convertible into an [`EventIngress`] so callers may keep handing over a
    /// raw `mpsc::Sender` while adapters share the very same ingress handle.
    pub fn new(ingress: impl Into<EventIngress>) -> Self {
        Self {
            ingress: ingress.into(),
            hosts: Arc::new(RwLock::new(HashMap::new())),
            gateway: None,
        }
    }

    /// Returns the shared ingest handle, so adapters can push inbound events.
    pub fn ingress(&self) -> &EventIngress {
        &self.ingress
    }

    /// Configures the active LLM gateway instance.
    pub fn with_gateway(mut self, gateway: Arc<LlmGateway>) -> Self {
        self.gateway = Some(gateway);
        self
    }

    /// Returns a snapshot of currently registered hosts.
    pub async fn get_registered_hosts(&self) -> Vec<HostRegistration> {
        self.hosts.read().await.values().cloned().collect()
    }
}


#[tonic::async_trait]
impl BotApiService for CoreApiService {
    /// Registers a newly initialized plugin host with the Core microkernel.
    async fn register_host(
        &self,
        request: Request<RegisterHostRequest>,
    ) -> Result<Response<RegisterHostResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(
            host_id = %req.host_id,
            runtime = %req.runtime,
            endpoint = %req.endpoint,
            "Received Host registration request"
        );

        let registration = HostRegistration {
            host_id: req.host_id.clone(),
            runtime: req.runtime,
            endpoint: req.endpoint,
            loaded_plugin_ids: req.loaded_plugin_ids,
        };

        self.hosts.write().await.insert(req.host_id.clone(), registration);

        Ok(Response::new(RegisterHostResponse {
            success: true,
            message: format!("Host '{}' registered successfully", req.host_id),
            core_metadata: None,
        }))
    }

    /// Ingests an inbound event into the core pipeline with millisecond Fast-ACK.
    ///
    /// # Fast-ACK Concurrency Guarantee
    /// To ensure external IM heartbeat keep-alive connections (e.g. WebSocket pings)
    /// never block on slow downstream LLM generation or plugin I/O, this method
    /// uses non-blocking `try_send` into a bounded Tokio MPSC channel.
    /// Execution returns in < 50µs, severing the backpressure chain from IM adapters.
    async fn ingest_event(
        &self,
        request: Request<IngestEventRequest>,
    ) -> Result<Response<IngestEventResponse>, Status> {
        let req = request.into_inner();

        let event_id = req
            .event
            .as_ref()
            .map(|e| e.event_id.clone())
            .unwrap_or_default();

        // Non-blocking enqueue to guarantee Fast-ACK (< 50µs).
        match self.ingress.try_ingest(req) {
            Ok(()) => {
                Ok(Response::new(IngestEventResponse {
                    accepted: true,
                    event_id,
                }))
            }
            Err(IngestError::QueueFull) => {
                // High watermark reached: report backpressure but acknowledge failure quickly.
                tracing::warn!(
                    event_id = %event_id,
                    "Inbound event queue full; dropping event to protect microkernel stability"
                );
                Ok(Response::new(IngestEventResponse {
                    accepted: false,
                    event_id,
                }))
            }
            Err(IngestError::Closed) => {
                Err(Status::unavailable("Microkernel event queue has been closed"))
            }
        }
    }

    /// Dispatches an outbound message to the target platform adapter.
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();
        tracing::debug!(
            platform = %req.platform,
            channel_id = %req.channel_id,
            "Core received SendMessage request"
        );
        // Stub implementation for Phase 1.
        Ok(Response::new(SendMessageResponse {
            success: true,
            message_id: "stub_msg_1".to_string(),
            error_message: String::new(),
        }))
    }

    /// Server streaming response type for LLM token generation chunks.
    type RequestLLMStream = tokio_stream::wrappers::ReceiverStream<Result<LlmChunk, Status>>;

    /// Invokes the LLM gateway for streaming text completions.
    async fn request_llm(
        &self,
        request: Request<LlmRequest>,
    ) -> Result<Response<Self::RequestLLMStream>, Status> {
        let req = request.into_inner();
        let gateway = match &self.gateway {
            Some(g) => g.clone(),
            None => {
                // When no gateway is bound, return a single completed fallback chunk
                let (tx, rx) = mpsc::channel(1);
                let _ = tx
                    .send(Ok(LlmChunk {
                        delta_text: format!("Echo: {}", req.prompt),
                        is_finished: true,
                    }))
                    .await;
                return Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)));
            }
        };

        let mut messages = Vec::new();
        let mut temperature = None;
        let mut max_tokens = None;

        if let Some(ref params) = req.parameters {
            if let Some(kanon_proto::prost_types::value::Kind::StringValue(s)) =
                params.fields.get("system_prompt").and_then(|v| v.kind.as_ref())
            {
                messages.push(ChatMessage::system(s));
            }
            if let Some(kanon_proto::prost_types::value::Kind::NumberValue(n)) =
                params.fields.get("temperature").and_then(|v| v.kind.as_ref())
            {
                temperature = Some(*n as f32);
            }
            if let Some(kanon_proto::prost_types::value::Kind::NumberValue(n)) =
                params.fields.get("max_tokens").and_then(|v| v.kind.as_ref())
            {
                max_tokens = Some(*n as u32);
            }
        }

        messages.push(ChatMessage::user(req.prompt));

        let chat_req = ChatRequest {
            model: req.model,
            messages,
            tools: Vec::new(),
            temperature,
            max_tokens,
        };

        let stream = gateway.chat_stream(&chat_req).await.map_err(|e| {
            Status::internal(format!("LLM Gateway error: {e}"))
        })?;

        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut stream = stream;
            while let Some(chunk_res) = stream.next().await {
                match chunk_res {
                    Ok(chunk) => {
                        let proto_chunk = LlmChunk {
                            delta_text: chunk.delta_text,
                            is_finished: chunk.is_finished,
                        };
                        if tx.send(Ok(proto_chunk)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(Status::internal(format!("Stream chunk error: {e}"))))
                            .await;
                        break;
                    }
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }


    /// Sets an embedded KV key-value pair.
    async fn set_storage(
        &self,
        _request: Request<SetStorageRequest>,
    ) -> Result<Response<SetStorageResponse>, Status> {
        // Stub implementation for Phase 1.
        Ok(Response::new(SetStorageResponse { success: true }))
    }

    /// Retrieves an embedded KV key-value pair.
    async fn get_storage(
        &self,
        _request: Request<GetStorageRequest>,
    ) -> Result<Response<GetStorageResponse>, Status> {
        // Stub implementation for Phase 1.
        Ok(Response::new(GetStorageResponse {
            found: false,
            value: vec![],
        }))
    }
}

/// IPC Server hosting the Core microkernel services on `./run/core.sock`.
pub struct CoreIpcServer {
    /// Path to the Unix domain socket or loopback address file.
    socket_path: PathBuf,
    /// Core API service implementation.
    service: CoreApiService,
}

impl CoreIpcServer {
    /// Creates a new `CoreIpcServer` with the specified socket path and service instance.
    pub fn new(socket_path: impl Into<PathBuf>, service: CoreApiService) -> Self {
        Self {
            socket_path: socket_path.into(),
            service,
        }
    }

    /// Creates a `CoreIpcServer` bound to the default socket path (`./run/core.sock`).
    pub fn with_default_path(service: CoreApiService) -> Self {
        Self::new(core_socket_path(None), service)
    }

    /// Returns a reference to the bound socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Runs the Core IPC server until the provided shutdown signal resolves.
    ///
    /// Binds to the designated socket path via [`IpcListener`] and registers
    /// the [`BotApiServiceServer`]. On graceful termination, the socket file is cleaned up.
    pub async fn run<F>(self, shutdown_signal: F) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let socket_path = self.socket_path.clone();
        let listener = IpcListener::bind(&socket_path)?;
        tracing::info!(socket = %socket_path.display(), "Core IPC server listening");

        let incoming = listener.incoming();
        let service = BotApiServiceServer::new(self.service);

        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming_shutdown(incoming, shutdown_signal)
            .await?;

        // Clean up socket file upon server exit.
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
            tracing::debug!(socket = %socket_path.display(), "Cleaned up Core socket file");
        }

        Ok(())
    }
}
