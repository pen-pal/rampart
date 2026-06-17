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
pub use service::{send_system_email, NotifierHandle, NotifierService};

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("config invalid: {0}")]
    BadConfig(String),
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),
    #[error("upstream returned {0}: {1}")]
    Upstream(u16, String),
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("other: {0}")]
    Other(String),
}

/// What every channel adapter implements. The adapter owns its config
/// (deserialized from the `notifications.config` JSONB column) and knows
/// how to talk to its upstream. The `body` is the pre-rendered text from
/// the shared template engine.
#[async_trait]
pub trait Channel: Send + Sync {
    async fn send(&self, subject: &str, body: &str, event: &Event) -> Result<(), ChannelError>;
}
