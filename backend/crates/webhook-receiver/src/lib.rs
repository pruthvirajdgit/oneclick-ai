//! OneClick.ai — Inbound webhook processing.
//!
//! Phase 1 stub. Telegram and Slack integration coming in Phase 1.5.
//! This crate will handle inbound webhook events, validate signatures,
//! and route messages to the appropriate agent via the message queue.

use uuid::Uuid;

/// Placeholder receiver for inbound webhooks (Telegram, Slack, etc.).
///
/// Will be fleshed out in Phase 1.5 with:
/// - Telegram Bot API webhook handler
/// - Slack Events API webhook handler
/// - Signature verification per platform
/// - Message normalisation and routing to agents
pub struct WebhookReceiver {
    _private: (),
}

impl WebhookReceiver {
    /// Create a new (no-op) [`WebhookReceiver`].
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Placeholder — will return the agent ID mapped to a given platform
    /// channel once the integration is implemented.
    pub fn resolve_agent(&self, _platform: &str, _channel_id: &str) -> Option<Uuid> {
        None
    }
}

impl Default for WebhookReceiver {
    fn default() -> Self {
        Self::new()
    }
}
