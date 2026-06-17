//! Shared SSRF-guarded HTTP client for outbound notification delivery.
//!
//! Every notification channel POSTs to an operator/editor-configured URL, so
//! delivery is a user-controlled outbound surface exactly like probing. All
//! channels build their client via [`client`], which routes DNS through the
//! shared `rampart-ssrf` guarded resolver — so a webhook (or a compromised
//! editor key) can't point a channel at `169.254.169.254` (cloud metadata) or
//! internal admin ports. The guard runs at connect time, covering redirects
//! too. The client is built once and cloned (`reqwest::Client` is internally an
//! `Arc`, so clones are cheap and share the connection pool).

use once_cell::sync::Lazy;
use reqwest::Client;

static CLIENT: Lazy<Client> = Lazy::new(|| {
    rampart_ssrf::guarded_client_builder()
        .build()
        // Mirror the previous `reqwest::Client::new()` behavior on the
        // (practically unreachable) builder-failure path.
        .unwrap_or_else(|_| Client::new())
});

/// The shared SSRF-guarded HTTP client (cheap clone; shares the pool).
pub fn client() -> Client {
    CLIENT.clone()
}
