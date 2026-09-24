//! Bundled platform adapters and their registration helper.
//!
//! Kanon ships one generic adapter so a fresh node can talk to *something* without a plugin: the
//! [`WebhookAdapter`] bridges any external system (a frontend, an IM bridge, a shell script) over
//! plain HTTP. Inbound messages enter through the management gateway's ingest endpoint, outbound
//! messages are POSTed to a configured callback URL.
//!
//! Platform-specific adapters (Telegram long polling, OneBot sockets, Discord gateways) implement
//! the same [`kanon_core::PlatformAdapter`] contract and are registered the same way — the
//! contract, not this particular bridge, is the extension point.

pub mod webhook;

pub use webhook::WebhookAdapter;
