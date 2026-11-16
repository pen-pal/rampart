// Test-module-in-the-middle-of-the-file is deliberate: keeps each
// adapter's tests next to its definition.
#![allow(clippy::items_after_test_module)]
// Many channel docs use 2-space indented continuation lines for the
// URL syntax examples — let them be.
#![allow(clippy::doc_overindented_list_items)]

//! Rampart · notification fan-out.
//!
//! When a probe's status flips, the scheduler emits an `Event` on a tokio
//! mpsc channel. This crate's `NotifierService` consumes those events,
//! looks up the channels attached to the affected monitor, renders the
//! template (default or user-supplied), and delivers via each channel's
//! adapter.
//!
//! No delivery guarantees: deliveries are best-effort and fire-and-forget
//! within a tokio task. We log failures and move on. If the user wants
//! "definitely-delivered" semantics, they can attach a generic webhook to
//! a queueing system they control.

pub mod channels;
pub mod event;
pub mod http;
pub mod service;
pub mod siem;
pub mod template;

/// Install the ring `CryptoProvider` as the global default if no
/// provider is set yet. Called once per test executable from each
/// channel-test module — `reqwest::Client::new()` panics with "No
/// provider set" on `rustls-no-provider` builds otherwise. Production
/// binaries get this via `rampart-api::main`'s startup install; tests
/// can't share that path, so each test crate that constructs a
/// reqwest client routes through here.
#[cfg(test)]
pub(crate) fn init_test_crypto() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub use event::{Event, EventKind};
pub use service::{dropped_events_total, send_system_email, NotifierHandle, NotifierService};

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("config invalid: {0}")]
    BadConfig(String),
    /// Built via `From<reqwest::Error>`, which strips the request URL first.
    #[error("network: {0}")]
    Network(reqwest::Error),
    #[error("upstream returned {0}: {1}")]
    Upstream(u16, String),
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("other: {0}")]
    Other(String),
}

/// Many channels carry their credential in the request URL because the provider
/// requires it there (Slack / Discord webhook URLs, Telegram `bot<token>`,
/// Pushy `?api_key=`, SIGNL4 / GTX path secrets). reqwest's error text includes
/// the full URL, and channel errors are logged and stored in the admin-visible
/// delivery log, so the URL is dropped before the error is kept.
impl From<reqwest::Error> for ChannelError {
    fn from(e: reqwest::Error) -> Self {
        ChannelError::Network(e.without_url())
    }
}

impl ChannelError {
    /// Whether a failed dispatch is worth retrying. Transient transport errors
    /// and retryable upstream statuses (408 Request Timeout, 429 Too Many
    /// Requests, any 5xx) get another attempt; a `BadConfig`, an SSRF/`Blocked`
    /// rejection, a permanent 4xx, or an unclassified `Other` would fail
    /// identically on retry and just hammer the sink, so they are terminal.
    ///
    /// Retrying is at-least-once: it only re-sends on errors where the prior
    /// attempt almost certainly did NOT deliver (transport failure, or an
    /// upstream that explicitly didn't accept the request), so a duplicate
    /// alert is possible but unlikely — and for alerting a rare duplicate beats
    /// a dropped page.
    pub fn is_retryable(&self) -> bool {
        match self {
            ChannelError::Network(_) => true,
            ChannelError::Upstream(code, _) => {
                *code == 408 || *code == 429 || (500..=599).contains(code)
            }
            ChannelError::BadConfig(_) | ChannelError::Blocked(_) | ChannelError::Other(_) => false,
        }
    }
}

/// What every channel adapter implements. The adapter owns its config
/// (deserialized from the `notifications.config` JSONB column) and knows
/// how to talk to its upstream. The `body` is the pre-rendered text from
/// the shared template engine.
#[async_trait]
pub trait Channel: Send + Sync {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError>;
}

#[cfg(test)]
mod error_tests {
    use super::ChannelError;

    /// A failed request must not carry its URL (often the credential) into the
    /// error text that ends up in logs and the delivery log.
    #[tokio::test]
    async fn network_error_text_omits_the_request_url() {
        crate::init_test_crypto();
        let secret = "SECRET-bot-token-123";
        // Nothing listens on port 1, so this fails fast with a connect error.
        let err = reqwest::Client::new()
            .post(format!(
                "http://127.0.0.1:1/bot{secret}/send?api_key={secret}"
            ))
            .send()
            .await
            .expect_err("connection must fail");
        assert!(
            err.to_string().contains(secret),
            "precondition: reqwest includes the URL"
        );
        let ce: ChannelError = err.into();
        assert!(matches!(ce, ChannelError::Network(_)));
        assert!(ce.is_retryable());
        assert!(!ce.to_string().contains(secret), "leaked: {ce}");
        assert!(
            !format!("{ce:?}").contains(secret),
            "leaked in Debug: {ce:?}"
        );
    }

    #[test]
    fn retryable_classification() {
        // Transient transport + retryable upstream statuses → retry.
        assert!(ChannelError::Upstream(429, "rate limited".into()).is_retryable());
        assert!(ChannelError::Upstream(408, "timeout".into()).is_retryable());
        assert!(ChannelError::Upstream(500, "boom".into()).is_retryable());
        assert!(ChannelError::Upstream(503, "unavailable".into()).is_retryable());

        // Permanent failures → no retry (would fail identically + spam the sink).
        assert!(!ChannelError::Upstream(400, "bad request".into()).is_retryable());
        assert!(!ChannelError::Upstream(401, "unauthorized".into()).is_retryable());
        assert!(!ChannelError::Upstream(404, "not found".into()).is_retryable());
        assert!(!ChannelError::BadConfig("missing token".into()).is_retryable());
        assert!(!ChannelError::Blocked("ssrf".into()).is_retryable());
        assert!(!ChannelError::Other("weird".into()).is_retryable());
    }
}
