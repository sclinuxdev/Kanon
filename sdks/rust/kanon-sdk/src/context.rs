//! Plugin execution context, environment metadata, and the core IPC handle.
//!
//! Exposes access to isolated persistent directories, active plugin configurations, and — for
//! plugins acting as platform adapters — a [`CoreHandle`] that pushes inbound messages back into
//! the microkernel pipeline.

use std::path::PathBuf;
use std::sync::Arc;

use kanon_proto::v1::bot_api_service_client::BotApiServiceClient;
use kanon_proto::v1::{
    IngestEventRequest, IngestEventResponse, PipelineEventRequest, RegisterHostRequest,
    RegisterHostResponse,
};
use tokio::sync::Mutex;
use tonic::transport::Channel;

/// Runtime context passed to plugins during initialization and invocation.
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// Dedicated directory for this plugin's local database and file storage (`./data/plugins/<id>/`).
    pub data_dir: PathBuf,
    /// Active dynamic configuration parameters provided by Core.
    pub config: Option<prost_types::Struct>,
    /// Handle to the Core microkernel, absent when the host runs standalone.
    ///
    /// Adapter plugins must capture this handle during `on_load` (the only hook receiving the
    /// context) and reuse it from their polling loops:
    /// ```ignore
    /// async fn on_load(&mut self, ctx: &mut PluginContext) -> PluginResult<()> {
    ///     self.core = ctx.core.clone();
    ///     Ok(())
    /// }
    /// ```
    pub core: Option<CoreHandle>,
}

impl PluginContext {
    /// Creates a new `PluginContext` for the given data directory.
    pub fn new(data_dir: PathBuf, config: Option<prost_types::Struct>) -> Self {
        Self {
            data_dir,
            config,
            core: None,
        }
    }

    /// Attaches a Core IPC handle, enabling inbound ingestion from this plugin.
    pub fn with_core(mut self, core: CoreHandle) -> Self {
        self.core = Some(core);
        self
    }
}

/// Cloneable handle for calling back into the Core microkernel (`BotApiService`).
///
/// A single gRPC channel is shared across every clone instead of dialing per call: plugin hosts
/// issue these calls from long-running loops (a chat platform poller, a socket reader), and
/// re-establishing a connection per message would dominate the cost of a text-only event.
#[derive(Debug, Clone)]
pub struct CoreHandle {
    client: Arc<Mutex<BotApiServiceClient<Channel>>>,
}

/// `tonic::Status` is inherently large (it carries metadata and a boxed source), and boxing it
/// here would force every plugin author to unwrap a second layer for no benefit. The rest of the
/// workspace follows the same convention.
#[allow(clippy::result_large_err)]
impl CoreHandle {
    /// Wraps an established core channel.
    pub fn new(channel: Channel) -> Self {
        Self {
            client: Arc::new(Mutex::new(BotApiServiceClient::new(channel))),
        }
    }

    /// Pushes an inbound event into the core pipeline and returns the Fast-ACK acknowledgement.
    ///
    /// The returned [`IngestEventResponse`] carries `accepted`: when the core's ingest queue is
    /// saturated it answers `accepted = false` instead of blocking, and the adapter decides
    /// whether to retry or drop. Transport failures surface as `tonic::Status` errors — this
    /// method never reports success for a message the core did not accept.
    pub async fn ingest_event(
        &self,
        request: IngestEventRequest,
    ) -> Result<IngestEventResponse, tonic::Status> {
        let mut client = self.client.lock().await;
        let response = client.ingest_event(request).await?;
        Ok(response.into_inner())
    }

    /// Answers a liveness probe from the core.
    ///
    /// Deliberately trivial: it opens no business path, so a host can distinguish "core is gone"
    /// from "core is busy". Used by [`crate::watchdog::watch_core`].
    pub async fn ping(&self) -> Result<(), tonic::Status> {
        let mut client = self.client.lock().await;
        let request = kanon_proto::v1::PingRequest {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default(),
        };
        client.ping(request).await?;
        Ok(())
    }

    /// Registers the hosting process with the core (`BotApiService.RegisterHost`).
    ///
    /// Used by the host runner during startup; exposed because registration is also the cheapest
    /// proof that the core endpoint is actually serving.
    pub async fn register_host(
        &self,
        request: RegisterHostRequest,
    ) -> Result<RegisterHostResponse, tonic::Status> {
        let mut client = self.client.lock().await;
        let response = client.register_host(request).await?;
        Ok(response.into_inner())
    }

    /// Convenience wrapper building a text-only event for `platform`.
    ///
    /// `event_id` is supplied by the caller because platform message identifiers (update ids,
    /// message ids) are the most useful trace keys; pass an empty string to let the core accept
    /// the event without a caller-assigned identifier.
    pub async fn ingest_text(
        &self,
        platform: impl Into<String>,
        channel_id: impl Into<String>,
        sender_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<IngestEventResponse, tonic::Status> {
        let platform = platform.into();
        let request = IngestEventRequest {
            platform: platform.clone(),
            event: Some(PipelineEventRequest {
                event_id: String::new(),
                platform,
                channel_id: channel_id.into(),
                sender_id: sender_id.into(),
                raw_text: text.into(),
                segments: Vec::new(),
                metadata: None,
            }),
        };

        self.ingest_event(request).await
    }
}
