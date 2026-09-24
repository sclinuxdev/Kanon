//! Generic HTTP webhook platform adapter.
//!
//! # Contract
//! - **Inbound**: the external system POSTs to `/api/v1/adapters/{platform}/ingest`; the gateway
//!   pushes the message into the core's Fast-ACK ingest queue. Nothing is polled.
//! - **Outbound**: every reply for this platform is POSTed as JSON to the configured callback URL.
//!
//! Because the adapter is push-driven, [`PlatformAdapter::start`] needs no inbound loop; the
//! gateway route *is* the loop. A missing callback URL does not disable the adapter — inbound
//! keeps working — but outbound deliveries then fail with an explicit configuration error and the
//! adapter reports `connected: false` to the console, so the state is never ambiguous.

use std::time::Duration;

use async_trait::async_trait;
use kanon_core::{AdapterError, EventIngress, PlatformAdapter};
use kanon_proto::v1::message_segment::Segment;
use kanon_proto::v1::{DeliverMessageRequest, DeliverMessageResponse, audio_segment, image_segment};
use serde::Serialize;

/// Timeout applied to callback deliveries.
///
/// Kept short on purpose: the delivery queue is drained sequentially, so a hanging endpoint must
/// fail fast instead of stalling every other platform's outbound traffic.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// A generic HTTP bridge adapter.
pub struct WebhookAdapter {
    /// Platform identifier served by this adapter instance.
    platform: String,
    /// Console-facing adapter name.
    display_name: String,
    /// Destination for outbound messages; absent when the adapter is inbound-only.
    callback_url: Option<String>,
    /// Shared HTTP client (connection pooling across deliveries).
    client: reqwest::Client,
}

impl WebhookAdapter {
    /// Creates a webhook adapter for `platform`.
    ///
    /// `callback_url` may be `None`, which makes the adapter inbound-only.
    pub fn new(
        platform: impl Into<String>,
        display_name: Option<String>,
        callback_url: Option<String>,
    ) -> Result<Self, AdapterError> {
        let platform = platform.into();
        if platform.trim().is_empty() {
            return Err(AdapterError::Configuration {
                platform: platform.clone(),
                reason: "platform identifier must not be empty".to_string(),
            });
        }

        let callback_url = callback_url.filter(|url| !url.trim().is_empty());

        let client = reqwest::Client::builder()
            .timeout(DELIVERY_TIMEOUT)
            .build()
            .map_err(|err| AdapterError::Configuration {
                platform: platform.clone(),
                reason: format!("failed to build HTTP client: {err}"),
            })?;

        Ok(Self {
            display_name: display_name.unwrap_or_else(|| platform.clone()),
            platform,
            callback_url,
            client,
        })
    }

    /// Returns the configured callback URL, if any.
    pub fn callback_url(&self) -> Option<&str> {
        self.callback_url.as_deref()
    }
}

#[async_trait]
impl PlatformAdapter for WebhookAdapter {
    fn platform(&self) -> &str {
        &self.platform
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn is_connected(&self) -> bool {
        self.callback_url.is_some()
    }

    async fn deliver(
        &self,
        request: DeliverMessageRequest,
    ) -> Result<DeliverMessageResponse, AdapterError> {
        let Some(ref callback_url) = self.callback_url else {
            return Err(AdapterError::Configuration {
                platform: self.platform.clone(),
                reason: "no callback URL configured; outbound delivery is unavailable".to_string(),
            });
        };

        let payload = WebhookPayload::from_request(&request);

        let response =
            self.client
                .post(callback_url)
                .json(&payload)
                .send()
                .await
                .map_err(|err| AdapterError::Delivery {
                    platform: self.platform.clone(),
                    reason: format!("POST {callback_url} failed: {err}"),
                })?;

        let status = response.status();
        if !status.is_success() {
            // Read the body for diagnostics, but never let a huge error page blow up the log.
            let body = response.text().await.unwrap_or_default();
            let excerpt: String = body.chars().take(256).collect();
            return Err(AdapterError::Delivery {
                platform: self.platform.clone(),
                reason: format!("callback returned HTTP {status}: {excerpt}"),
            });
        }

        // The callback may answer with a message id; any 2xx without a body still counts as
        // delivered, so parsing failure is not treated as a delivery failure.
        let message_id = response
            .json::<WebhookAck>()
            .await
            .map(|ack| ack.message_id)
            .unwrap_or_default();

        Ok(DeliverMessageResponse {
            success: true,
            message_id,
            error_message: String::new(),
        })
    }

    async fn start(&self, _ingress: EventIngress) -> Result<(), AdapterError> {
        // Push-driven adapter: inbound arrives on the gateway ingest route, so there is no loop to
        // spawn. Reporting a started loop here would be a lie.
        Ok(())
    }
}

/// JSON payload POSTed to the callback URL.
#[derive(Debug, Serialize)]
struct WebhookPayload {
    /// Platform the message belongs to.
    platform: String,
    /// Destination channel.
    channel_id: String,
    /// Destination user.
    recipient_id: String,
    /// Ordered message segments.
    segments: Vec<WebhookSegment>,
}

impl WebhookPayload {
    /// Flattens protobuf segments into a JSON-friendly representation.
    fn from_request(request: &DeliverMessageRequest) -> Self {
        Self {
            platform: request.platform.clone(),
            channel_id: request.channel_id.clone(),
            recipient_id: request.recipient_id.clone(),
            segments: request
                .segments
                .iter()
                .map(WebhookSegment::from_segment)
                .collect(),
        }
    }
}

/// JSON representation of one outbound message segment.
#[derive(Debug, Serialize)]
struct WebhookSegment {
    /// Segment kind (`text`, `image`, `audio`, `mention`, `reply`, `custom`, `unknown`).
    kind: &'static str,
    /// Plain text content for text segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    /// Remote reference (URL or path) for media segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// Mention target for mention segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    target_user_id: Option<String>,
    /// Quoted message identifier for reply segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<String>,
    /// Custom type name for plugin-defined segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    type_name: Option<String>,
}

impl WebhookSegment {
    /// Converts one protobuf segment into its JSON form.
    ///
    /// Inline binary is summarised rather than inlined: the webhook contract has no binary
    /// channel, and base64 would inflate payloads by a third while pretending to be text.
    fn from_segment(segment: &kanon_proto::v1::MessageSegment) -> Self {
        match segment.segment.as_ref() {
            Some(Segment::Text(text)) => Self::text(text.content.clone()),
            Some(Segment::Image(image)) => Self {
                kind: "image",
                text: None,
                source: image_source(image.source.as_ref()),
                target_user_id: None,
                reply_to: None,
                type_name: None,
            },
            Some(Segment::Audio(audio)) => Self {
                kind: "audio",
                text: None,
                source: audio_source(audio.source.as_ref()),
                target_user_id: None,
                reply_to: None,
                type_name: None,
            },
            Some(Segment::Mention(mention)) => Self {
                kind: "mention",
                text: None,
                source: None,
                target_user_id: Some(mention.target_user_id.clone()),
                reply_to: None,
                type_name: None,
            },
            Some(Segment::Reply(reply)) => Self {
                kind: "reply",
                text: None,
                source: None,
                target_user_id: None,
                reply_to: Some(reply.target_message_id.clone()),
                type_name: None,
            },
            Some(Segment::Custom(custom)) => Self {
                kind: "custom",
                text: None,
                source: None,
                target_user_id: None,
                reply_to: None,
                type_name: Some(custom.type_name.clone()),
            },
            None => Self {
                kind: "unknown",
                text: None,
                source: None,
                target_user_id: None,
                reply_to: None,
                type_name: None,
            },
        }
    }

    /// Builds a plain text segment row.
    fn text(content: String) -> Self {
        Self {
            kind: "text",
            text: Some(content),
            source: None,
            target_user_id: None,
            reply_to: None,
            type_name: None,
        }
    }
}

/// Resolves the transferable reference of an image segment.
fn image_source(source: Option<&image_segment::Source>) -> Option<String> {
    match source {
        Some(image_segment::Source::Url(url)) => Some(url.clone()),
        Some(image_segment::Source::FilePath(path)) => Some(path.clone()),
        Some(image_segment::Source::RawBytes(bytes)) => Some(inline_hint(bytes.len())),
        None => None,
    }
}

/// Resolves the transferable reference of an audio segment.
fn audio_source(source: Option<&audio_segment::Source>) -> Option<String> {
    match source {
        Some(audio_segment::Source::Url(url)) => Some(url.clone()),
        Some(audio_segment::Source::FilePath(path)) => Some(path.clone()),
        Some(audio_segment::Source::RawBytes(bytes)) => Some(inline_hint(bytes.len())),
        None => None,
    }
}

/// Describes inline binary that cannot travel over JSON.
fn inline_hint(len: usize) -> String {
    format!("<inline {len} bytes>")
}

/// Optional acknowledgement payload returned by a callback endpoint.
#[derive(Debug, serde::Deserialize)]
struct WebhookAck {
    /// Platform-assigned message identifier.
    #[serde(default)]
    message_id: String,
}
