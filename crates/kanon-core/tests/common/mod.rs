#![allow(dead_code)]
//! Shared fixtures for the `kanon-core` integration tests.
//!
//! Each integration test binary compiles this module independently and uses a different subset of
//! the helpers, so unused-item warnings are expected and suppressed here only.

use std::sync::Arc;

use async_trait::async_trait;
use kanon_core::{AdapterError, PlatformAdapter};
use kanon_proto::v1::{DeliverMessageRequest, DeliverMessageResponse};
use tokio::sync::mpsc;

/// Built-in adapter that forwards every delivered message into an MPSC channel.
///
/// Integration tests register it for the platform their fixtures use, which lets them assert the
/// exact outbound request the pipeline produced while exercising the real adapter routing path.
pub struct ChannelAdapter {
    platform: String,
    sender: mpsc::Sender<DeliverMessageRequest>,
}

impl ChannelAdapter {
    /// Creates a forwarding adapter for `platform`.
    pub fn new(platform: impl Into<String>, sender: mpsc::Sender<DeliverMessageRequest>) -> Self {
        Self {
            platform: platform.into(),
            sender,
        }
    }

    /// Convenience constructor returning the shared adapter handle.
    pub fn shared(platform: impl Into<String>, sender: mpsc::Sender<DeliverMessageRequest>) -> Arc<Self> {
        Arc::new(Self::new(platform, sender))
    }
}

#[async_trait]
impl PlatformAdapter for ChannelAdapter {
    fn platform(&self) -> &str {
        &self.platform
    }

    fn display_name(&self) -> &str {
        "Test channel adapter"
    }

    async fn deliver(
        &self,
        request: DeliverMessageRequest,
    ) -> Result<DeliverMessageResponse, AdapterError> {
        let message_id = format!("test-msg-{}", request.channel_id);
        self.sender
            .send(request)
            .await
            .map_err(|_| AdapterError::Delivery {
                platform: self.platform.clone(),
                reason: "test channel closed".to_string(),
            })?;

        Ok(DeliverMessageResponse {
            success: true,
            message_id,
            error_message: String::new(),
        })
    }
}
