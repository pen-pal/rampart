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
use std::time::Duration;

static CLIENT: Lazy<Client> = Lazy::new(|| {
    rampart_ssrf::guarded_client_builder()
        // reqwest has NO default timeout; the ssrf builder doesn't add one
        // either. Without these, a sink that accepts the connection but never
        // responds (tarpit, overloaded collector, or a malicious operator-set
        // webhook) hangs the spawned dispatch task forever — and `dispatch_one`
        // then blocks awaiting that handle, wedging the event and leaking the
        // task + socket. Bound both so a dead target fails fast and retries.
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        // Mirror the previous `reqwest::Client::new()` behavior on the
        // (practically unreachable) builder-failure path.
        .unwrap_or_else(|_| Client::new())
});

/// The shared SSRF-guarded HTTP client (cheap clone; shares the pool).
pub fn client() -> Client {
    CLIENT.clone()
}
