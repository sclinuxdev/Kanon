//! Out-of-process plugin host runner.
//!
//! Exposes [`KanonHost`], which hosts a [`Plugin`] implementation, serves
//! `PluginHostService` and `MessagePipelineService` over an IPC listener,
//! and optionally registers itself with the Core microkernel.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

use kanon_proto::v1::bot_api_service_client::BotApiServiceClient;
use kanon_proto::v1::message_pipeline_service_server::{
    MessagePipelineService, MessagePipelineServiceServer,
};
use kanon_proto::v1::plugin_host_service_server::{
    PluginHostService, PluginHostServiceServer,
};
use kanon_proto::v1::{
    CommandExecuteRequest, CommandExecuteResponse, DeliverMessageRequest,
    DeliverMessageResponse, EventAck, EventNotification, GetPluginMetaRequest,
    GetPluginMetaResponse, PipelineEventRequest, PreFilterResult, RegisterHostRequest,
    ReloadPluginConfigRequest, ReloadPluginConfigResponse, ToolCallRequest,
    ToolCallResponse,
};
use kanon_transport::{connect_ipc, IpcListener};
use crate::context::PluginContext;
use crate::plugin::Plugin;

/// Out-of-process gRPC host for a Kanon plugin.
pub struct KanonHost<P: Plugin> {
    /// Inner plugin instance wrapped for shared thread-safe access.
    plugin: Arc<RwLock<P>>,
    /// Path to the dedicated socket on which this host will listen.
    socket_path: PathBuf,
    /// Path to the Kanon Core socket (`core.sock`).
    core_sock: Option<PathBuf>,
}

impl<P: Plugin> KanonHost<P> {
    /// Creates a new `KanonHost` wrapping the given plugin.
    ///
    /// Reads `KANON_HOST_SOCK` from the environment if present, falling back to `./run/host_rust.sock`.
    /// Reads `KANON_CORE_SOCK` from the environment if present.
    pub fn new(plugin: P) -> Self {
        let socket_path = std::env::var("KANON_HOST_SOCK")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./run/host_rust.sock"));

        let core_sock = std::env::var("KANON_CORE_SOCK")
            .ok()
            .map(PathBuf::from);

        Self {
            plugin: Arc::new(RwLock::new(plugin)),
            socket_path,
            core_sock,
        }
    }

    /// Explicitly sets the IPC endpoint socket path for this host.
    pub fn with_socket_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_path = path.into();
        self
    }

    /// Explicitly sets the Core IPC socket path (`core.sock`).
    pub fn with_core_sock(mut self, path: impl Into<PathBuf>) -> Self {
        self.core_sock = Some(path.into());
        self
    }

    /// Runs the plugin host, listening for shutdown via `SIGINT` (Ctrl+C).
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.run_with_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    }

    /// Runs the plugin host until the provided `shutdown` future resolves.
    pub async fn run_with_shutdown<F>(self, shutdown: F) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // 1. Initialize plugin lifecycle
        let meta = self.plugin.read().await.meta();
        let plugin_id = meta.id.clone();
        let data_dir = PathBuf::from(format!("./data/plugins/{plugin_id}"));
        let mut ctx = PluginContext::new(data_dir, None);

        self.plugin.write().await.on_load(&mut ctx).await?;
        tracing::info!(plugin_id = %plugin_id, socket = %self.socket_path.display(), "Plugin loaded successfully");

        // 2. Bind IPC Listener
        let listener = IpcListener::bind(&self.socket_path)?;
        let incoming = listener.incoming();

        // 3. Register with Core if core_sock is specified
        if let Some(ref core_path) = self.core_sock {
            let host_id = std::env::var("KANON_HOST_ID").unwrap_or_else(|_| plugin_id.clone());
            let endpoint = self.socket_path.to_string_lossy().to_string();
            let loaded_plugin_ids = vec![plugin_id.clone()];

            if let Ok(channel) = connect_ipc(core_path.clone()).await {
                let mut client = BotApiServiceClient::new(channel);
                let reg_req = RegisterHostRequest {
                    host_id,
                    runtime: "rust".to_string(),
                    endpoint,
                    loaded_plugin_ids,
                };
                if let Err(e) = client.register_host(reg_req).await {
                    tracing::warn!(error = %e, "Failed to register with Core, will proceed with hosting");
                } else {
                    tracing::info!("Successfully registered with Core IPC server");
                }
            } else {
                tracing::debug!("Core socket not reachable at startup, continuing in standalone mode");
            }
        }

        // 4. Start gRPC services
        let host_svc = HostServiceImpl {
            plugin: self.plugin.clone(),
        };
        let pipeline_svc = PipelineServiceImpl {
            plugin: self.plugin.clone(),
        };

        let server_future = tonic::transport::Server::builder()
            .add_service(PluginHostServiceServer::new(host_svc))
            .add_service(MessagePipelineServiceServer::new(pipeline_svc))
            .serve_with_incoming_shutdown(incoming, shutdown);

        server_future.await?;

        // 5. Cleanup on shutdown
        self.plugin.write().await.on_unload().await?;
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
            tracing::debug!(socket = %self.socket_path.display(), "Cleaned up host socket file");
        }

        Ok(())
    }
}

/// Implementation of [`PluginHostService`] for managing host lifecycle and inspection.
struct HostServiceImpl<P: Plugin> {
    plugin: Arc<RwLock<P>>,
}

#[tonic::async_trait]
impl<P: Plugin> PluginHostService for HostServiceImpl<P> {
    async fn ping(
        &self,
        request: Request<kanon_proto::v1::PingRequest>,
    ) -> Result<Response<kanon_proto::v1::PingResponse>, Status> {
        let req = request.into_inner();
        Ok(Response::new(kanon_proto::v1::PingResponse {
            timestamp: req.timestamp,
        }))
    }

    async fn reload_plugin_config(
        &self,
        request: Request<ReloadPluginConfigRequest>,
    ) -> Result<Response<ReloadPluginConfigResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(plugin_id = %req.plugin_id, "Reloading plugin configuration");
        Ok(Response::new(ReloadPluginConfigResponse {
            success: true,
            error_message: String::new(),
        }))
    }

    async fn get_plugin_meta(
        &self,
        _request: Request<GetPluginMetaRequest>,
    ) -> Result<Response<GetPluginMetaResponse>, Status> {
        let meta = self.plugin.read().await.meta();
        Ok(Response::new(GetPluginMetaResponse {
            plugins: vec![meta],
        }))
    }
}

/// Implementation of [`MessagePipelineService`] for dispatching events and commands to the plugin.
struct PipelineServiceImpl<P: Plugin> {
    plugin: Arc<RwLock<P>>,
}

#[tonic::async_trait]
impl<P: Plugin> MessagePipelineService for PipelineServiceImpl<P> {
    async fn on_pre_filter(
        &self,
        request: Request<PipelineEventRequest>,
    ) -> Result<Response<PreFilterResult>, Status> {
        let req = request.into_inner();
        let plugin = self.plugin.read().await;
        match plugin.on_pre_filter(req).await {
            Ok(Some(result)) => Ok(Response::new(result)),
            Ok(None) => Ok(Response::new(PreFilterResult {
                action: kanon_proto::v1::pre_filter_result::Action::Pass as i32,
                modified_text: String::new(),
                reply_messages: vec![],
            })),
            Err(e) => Err(Status::internal(format!("PreFilter error: {e}"))),
        }
    }

    async fn on_execute_command(
        &self,
        request: Request<CommandExecuteRequest>,
    ) -> Result<Response<CommandExecuteResponse>, Status> {
        let req = request.into_inner();
        let plugin = self.plugin.read().await;
        match plugin.on_execute_command(req).await {
            Ok(resp) => Ok(Response::new(resp)),
            Err(e) => Ok(Response::new(CommandExecuteResponse {
                success: false,
                replies: vec![],
                error_message: e.to_string(),
            })),
        }
    }

    async fn on_call_tool(
        &self,
        request: Request<ToolCallRequest>,
    ) -> Result<Response<ToolCallResponse>, Status> {
        let req = request.into_inner();
        let plugin = self.plugin.read().await;
        match plugin.on_call_tool(req).await {
            Ok(resp) => Ok(Response::new(resp)),
            Err(e) => Ok(Response::new(ToolCallResponse {
                call_id: String::new(),
                success: false,
                error_message: e.to_string(),
                payload: None,
            })),
        }
    }

    async fn on_event(
        &self,
        _request: Request<EventNotification>,
    ) -> Result<Response<EventAck>, Status> {
        Ok(Response::new(EventAck { received: true }))
    }

    async fn on_deliver_message(
        &self,
        _request: Request<DeliverMessageRequest>,
    ) -> Result<Response<DeliverMessageResponse>, Status> {
        Ok(Response::new(DeliverMessageResponse {
            success: true,
            message_id: "delivered_1".to_string(),
            error_message: String::new(),
        }))
    }
}
