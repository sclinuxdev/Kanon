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
use kanon_proto::v1::{
    DeliverMessageRequest, DeliverMessageResponse, audio_segment, image_segment,
};
use serde::Serialize;

/// Timeout applied to callback deliveries.
///
/// Kept short on purpose: the delivery queue is drained sequentially per platform, so a hanging
/// endpoint must fail fast instead of stalling subsequent deliveries.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Configuration for outbound HTTP delivery retries with exponential backoff.
#[derive(Debug, Clone)]
pub struct WebhookRetryConfig {
    /// Maximum number of retry attempts after the initial delivery attempt fails.
    /// Default is 2 (total 3 attempts). Set to 0 to disable retries.
    pub max_retries: usize,
    /// Base backoff duration before the first retry attempt.
    /// Subsequent retries back off exponentially (`initial_backoff * 2^attempt`).
    /// Default is 50 milliseconds.
    pub initial_backoff: Duration,
}

impl Default for WebhookRetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_backoff: Duration::from_millis(50),
        }
    }
}

/// A generic HTTP bridge adapter.
pub struct WebhookAdapter {
    /// Platform identifier served by this adapter instance.
    platform: String,
    /// Console-facing adapter name.
    display_name: String,
    /// Destination for outbound messages; absent when the adapter is inbound-only.
    callback_url: Option<String>,
    /// Optional shared secret for HMAC-SHA256 inbound authentication and outbound signing.
    secret: Option<String>,
    /// Retry configuration for outbound deliveries.
    retry_config: WebhookRetryConfig,
    /// Shared HTTP client (connection pooling across deliveries).
    client: reqwest::Client,
}

impl WebhookAdapter {
    /// Creates a webhook adapter for `platform` with default retry configuration.
    ///
    /// `callback_url` may be `None`, which makes the adapter inbound-only.
    pub fn new(
        platform: impl Into<String>,
        display_name: Option<String>,
        callback_url: Option<String>,
    ) -> Result<Self, AdapterError> {
        Self::with_options(
            platform,
            display_name,
            callback_url,
            None,
            WebhookRetryConfig::default(),
        )
    }

    /// Full constructor specifying platform, callback URL, secret, and retry settings.
    pub fn with_options(
        platform: impl Into<String>,
        display_name: Option<String>,
        callback_url: Option<String>,
        secret: Option<String>,
        retry_config: WebhookRetryConfig,
    ) -> Result<Self, AdapterError> {
        let platform = platform.into();
        if platform.trim().is_empty() {
            return Err(AdapterError::Configuration {
                platform: platform.clone(),
                reason: "platform identifier must not be empty".to_string(),
            });
        }

        let callback_url = callback_url.filter(|url| !url.trim().is_empty());
        let secret = secret.filter(|s| !s.trim().is_empty());

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
            secret,
            retry_config,
            client,
        })
    }

    /// Attaches an HMAC-SHA256 secret for inbound signature verification and outbound signing.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        let s = secret.into();
        self.secret = if s.trim().is_empty() { None } else { Some(s) };
        self
    }

    /// Configures the retry policy for outbound deliveries.
    pub fn with_retry_config(mut self, retry_config: WebhookRetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// Returns the configured callback URL, if any.
    pub fn callback_url(&self) -> Option<&str> {
        self.callback_url.as_deref()
    }

    /// Returns the configured HMAC secret, if any.
    pub fn secret(&self) -> Option<&str> {
        self.secret.as_deref()
    }

    /// Returns the configured retry settings.
    pub fn retry_config(&self) -> &WebhookRetryConfig {
        &self.retry_config
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

    fn verify_inbound(&self, signature: Option<&str>, payload: &[u8]) -> Result<(), AdapterError> {
        let Some(ref secret) = self.secret else {
            // No secret configured: verification is a no-op and accepts all inbound payloads.
            return Ok(());
        };

        let Some(sig) = signature.filter(|s| !s.trim().is_empty()) else {
            return Err(AdapterError::Authentication {
                platform: self.platform.clone(),
                reason:
                    "missing signature header (expected X-Hub-Signature-256 or X-Kanon-Signature)"
                        .to_string(),
            });
        };

        if !verify_hmac_sha256(secret.as_bytes(), payload, sig) {
            return Err(AdapterError::Authentication {
                platform: self.platform.clone(),
                reason: "HMAC-SHA256 signature verification failed".to_string(),
            });
        }

        Ok(())
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
        let payload_bytes = serde_json::to_vec(&payload).map_err(|err| AdapterError::Delivery {
            platform: self.platform.clone(),
            reason: format!("failed to serialize webhook payload: {err}"),
        })?;

        // Attach HMAC signature header when secret is configured
        let signature_header = self.secret.as_ref().map(|sec| {
            let sig = sign_hmac_sha256(sec.as_bytes(), &payload_bytes);
            format!("sha256={sig}")
        });

        let mut attempt = 0;
        let max_retries = self.retry_config.max_retries;
        let mut backoff = self.retry_config.initial_backoff;

        loop {
            let mut req_builder = self
                .client
                .post(callback_url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(payload_bytes.clone());

            if let Some(ref sig) = signature_header {
                req_builder = req_builder
                    .header("x-hub-signature-256", sig.as_str())
                    .header("x-kanon-signature", sig.as_str());
            }

            match req_builder.send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        // The callback may answer with a message id; any 2xx without a body still counts
                        // as delivered, so parsing failure is not treated as a delivery failure.
                        let message_id = response
                            .json::<WebhookAck>()
                            .await
                            .map(|ack| ack.message_id)
                            .unwrap_or_default();

                        return Ok(DeliverMessageResponse {
                            success: true,
                            message_id,
                            error_message: String::new(),
                        });
                    }

                    // 4xx client errors (except 429 Too Many Requests) are permanent faults: do NOT retry
                    let is_transient = status.is_server_error()
                        || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
                    let body = response.text().await.unwrap_or_default();
                    let excerpt: String = body.chars().take(256).collect();

                    if !is_transient || attempt >= max_retries {
                        return Err(AdapterError::Delivery {
                            platform: self.platform.clone(),
                            reason: format!(
                                "callback returned HTTP {status} (attempt {}/{}): {excerpt}",
                                attempt + 1,
                                max_retries + 1
                            ),
                        });
                    }

                    tracing::warn!(
                        platform = %self.platform,
                        attempt = attempt + 1,
                        max_retries = max_retries,
                        status = %status,
                        backoff_ms = backoff.as_millis(),
                        "Transient HTTP failure; backing off and retrying"
                    );
                }
                Err(err) => {
                    // Network errors (connection refused, reset, timeout) are transient
                    if attempt >= max_retries {
                        return Err(AdapterError::Delivery {
                            platform: self.platform.clone(),
                            reason: format!(
                                "POST {callback_url} failed after {} attempts: {err}",
                                attempt + 1
                            ),
                        });
                    }

                    tracing::warn!(
                        platform = %self.platform,
                        attempt = attempt + 1,
                        max_retries = max_retries,
                        error = %err,
                        backoff_ms = backoff.as_millis(),
                        "Network failure during delivery; backing off and retrying"
                    );
                }
            }

            // Exponential backoff sleep before next attempt
            tokio::time::sleep(backoff).await;
            backoff = backoff.saturating_mul(2);
            attempt += 1;
        }
    }

    async fn start(&self, _ingress: EventIngress) -> Result<(), AdapterError> {
        // Push-driven adapter: inbound arrives on the gateway ingest route, so there is no loop to
        // spawn. Reporting a started loop here would be a lie.
        Ok(())
    }
}

/// Verifies an HMAC-SHA256 signature for a payload.
///
/// Strips optional `sha256=` prefix and compares in constant time using `ring::hmac::verify`.
pub fn verify_hmac_sha256(secret: &[u8], payload: &[u8], signature: &str) -> bool {
    let raw_hex = signature
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature.trim());
    let Ok(sig_bytes) = decode_hex(raw_hex) else {
        return false;
    };
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret);
    ring::hmac::verify(&key, payload, &sig_bytes).is_ok()
}

/// Generates an HMAC-SHA256 signature hex string for a payload.
pub fn sign_hmac_sha256(secret: &[u8], payload: &[u8]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret);
    let tag = ring::hmac::sign(&key, payload);
    encode_hex(tag.as_ref())
}

/// Decodes an even-length hexadecimal string into raw bytes.
fn decode_hex(hex: &str) -> Result<Vec<u8>, ()> {
    if !hex.len().is_multiple_of(2) {
        return Err(());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

/// Formats raw bytes into a lowercase hexadecimal string.
fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
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
