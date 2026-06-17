//! SSRF guard for outbound requests — shared by the probe engine
//! (`rampart-checker`) and the notification delivery path (`rampart-notifier`).
//!
//! Rampart makes outbound requests to operator/editor-defined targets (probe
//! URLs, webhook/notification endpoints), so it is inherently an outbound
//! engine. To stop it being abused to reach the cloud metadata endpoint or
//! internal-only services, hosts are vetted before connect:
//!
//!   * **Always blocked** (no legitimate reason): loopback, link-local incl. the
//!     cloud metadata IP `169.254.169.254`, the v6 equivalents, and
//!     unspecified/broadcast addresses.
//!   * **Private ranges** (RFC1918, CGNAT 100.64/10, IPv6 ULA fc00::/7) are
//!     blocked only when `RAMPART_SSRF_BLOCK_PRIVATE` is set — homelabs
//!     legitimately monitor private IPs, so it is opt-in, but recommended for
//!     multi-user / internet-exposed deployments.
//!
//! Two ways to use the guard:
//!   * [`resolve_guarded`] — resolve + vet a `host:port`, returning the vetted
//!     addresses (caller connects to these).
//!   * [`GuardedResolver`] / [`guarded_client_builder`] — a `reqwest` DNS
//!     resolver that vets at **connect** time, so every request *and every
//!     redirect hop* is guarded with no TOCTOU/DNS-rebinding window. This is the
//!     preferred integration: build the HTTP client via
//!     [`guarded_client_builder`] and the guard is automatic.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

/// A target rejected by the guard.
#[derive(Debug, Clone)]
pub struct SsrfBlocked {
    pub host: String,
    pub reason: &'static str,
}

impl std::fmt::Display for SsrfBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "blocked by SSRF guard ({}): {}", self.reason, self.host)
    }
}

impl std::error::Error for SsrfBlocked {}

/// Whether to also block private/internal ranges (env `RAMPART_SSRF_BLOCK_PRIVATE`).
pub fn block_private_enabled() -> bool {
    matches!(
        std::env::var("RAMPART_SSRF_BLOCK_PRIVATE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

/// IPs that must never be reached regardless of configuration.
pub fn is_always_blocked(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast()
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_always_blocked(&IpAddr::V4(v4));
            }
            v6.is_loopback() || v6.is_unspecified() || is_v6_link_local(v6)
        }
    }
}

/// Private / internal ranges, blocked only under `block_private`.
pub fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || is_cgnat(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4.is_private() || is_cgnat(&v4);
            }
            is_ula(v6)
        }
    }
}

fn is_v6_link_local(a: &Ipv6Addr) -> bool {
    (a.segments()[0] & 0xffc0) == 0xfe80
}
fn is_ula(a: &Ipv6Addr) -> bool {
    (a.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7
}
fn is_cgnat(a: &Ipv4Addr) -> bool {
    let o = a.octets();
    o[0] == 100 && (o[1] & 0xc0) == 64 // 100.64.0.0/10
}

/// Classify a single already-resolved IP. `Ok(())` = allowed.
pub fn check_ip(host: &str, ip: IpAddr, block_private: bool) -> Result<(), SsrfBlocked> {
    if is_always_blocked(&ip) {
        return Err(SsrfBlocked {
            host: host.to_string(),
            reason: "loopback/link-local/metadata",
        });
    }
    if block_private && is_private(&ip) {
        return Err(SsrfBlocked {
            host: host.to_string(),
            reason: "private/internal range",
        });
    }
    Ok(())
}

/// Resolve `host:port` and reject if ANY resolved address is blocked. Returns
/// the vetted addresses — connect to these (pinned) to avoid a rebind.
pub async fn resolve_guarded(
    host: &str,
    port: u16,
    block_private: bool,
) -> Result<Vec<SocketAddr>, SsrfBlocked> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| SsrfBlocked {
            host: host.to_string(),
            reason: "DNS resolution failed",
        })?
        .collect();
    if addrs.is_empty() {
        return Err(SsrfBlocked {
            host: host.to_string(),
            reason: "DNS resolution empty",
        });
    }
    for sa in &addrs {
        check_ip(host, sa.ip(), block_private)?;
    }
    Ok(addrs)
}

/// Pre-flight SSRF guard for a full URL. Use this on any outbound URL built
/// from a guarded [`guarded_client_builder`] client: reqwest invokes the
/// [`GuardedResolver`] only for *hostname* targets, so a literal-IP URL (e.g.
/// `http://169.254.169.254/…`) connects without ever hitting the resolver. This
/// parses the URL and vets the host through [`resolve_guarded`], which resolves
/// a hostname and passes an IP-literal straight to the IP check — so it blocks
/// both. Honors `RAMPART_SSRF_BLOCK_PRIVATE`.
pub async fn guard_url(url: &str) -> Result<(), SsrfBlocked> {
    let parsed = reqwest::Url::parse(url).map_err(|_| SsrfBlocked {
        host: url.to_string(),
        reason: "unparseable url",
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SsrfBlocked {
            host: url.to_string(),
            reason: "url has no host",
        })?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    resolve_guarded(&host, port, block_private_enabled())
        .await
        .map(|_| ())
}

/// A `reqwest` DNS resolver that vets every resolved address through the SSRF
/// guard at connect time. Because reqwest calls the resolver for the initial
/// request *and* for each followed redirect, this closes the TOCTOU /
/// DNS-rebinding window that a check-then-connect-on-the-original-URL approach
/// leaves open: the IP that is actually dialed is the IP that was vetted.
#[derive(Debug, Clone)]
pub struct GuardedResolver {
    block_private: bool,
}

impl GuardedResolver {
    /// Resolver honoring `RAMPART_SSRF_BLOCK_PRIVATE` (read once at build).
    pub fn from_env() -> Self {
        Self {
            block_private: block_private_enabled(),
        }
    }
}

impl Default for GuardedResolver {
    fn default() -> Self {
        Self::from_env()
    }
}

impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let block_private = self.block_private;
        Box::pin(async move {
            // Port 0: reqwest substitutes the URL's port (or the scheme default).
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                .collect();
            if addrs.is_empty() {
                return Err(Box::new(SsrfBlocked {
                    host: host.clone(),
                    reason: "DNS resolution empty",
                }) as Box<dyn std::error::Error + Send + Sync>);
            }
            // Reject if ANY resolved address is blocked, so a host answering with
            // [public, 169.254.169.254] can't trick the connector into the bad one.
            for sa in &addrs {
                check_ip(&host, sa.ip(), block_private)
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A `reqwest::ClientBuilder` pre-wired with the [`GuardedResolver`]. Callers add
/// their own timeouts / redirect policy / headers, then `.build()`. Every
/// connection the resulting client makes is SSRF-vetted at dial time.
pub fn guarded_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().dns_resolver(Arc::new(GuardedResolver::from_env()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn ip(s: &str) -> IpAddr {
        IpAddr::from_str(s).unwrap()
    }

    #[test]
    fn metadata_and_loopback_always_blocked() {
        for s in ["169.254.169.254", "127.0.0.1", "0.0.0.0", "::1", "fe80::1"] {
            assert!(is_always_blocked(&ip(s)), "{s} should be always-blocked");
        }
        // IPv4-mapped metadata must not slip through.
        assert!(is_always_blocked(&ip("::ffff:169.254.169.254")));
    }

    #[test]
    fn public_ips_allowed() {
        for s in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(!is_always_blocked(&ip(s)), "{s} should be allowed");
            assert!(!is_private(&ip(s)), "{s} not private");
        }
    }

    #[test]
    fn private_ranges_detected() {
        for s in ["10.0.0.5", "192.168.1.1", "172.16.0.1", "100.64.0.1", "fc00::1"] {
            assert!(is_private(&ip(s)), "{s} should be private");
            assert!(!is_always_blocked(&ip(s)), "{s} private != always-blocked");
        }
    }

    #[test]
    fn check_ip_respects_block_private_flag() {
        // Private allowed when flag off, blocked when on.
        assert!(check_ip("h", ip("10.0.0.1"), false).is_ok());
        assert!(check_ip("h", ip("10.0.0.1"), true).is_err());
        // Metadata blocked regardless of the flag.
        assert!(check_ip("h", ip("169.254.169.254"), false).is_err());
    }

    // guard_url must block literal-IP URLs (loopback + cloud metadata) — the
    // exact case the reqwest DNS-resolver hook never sees. A literal IP needs no
    // network to "resolve", so these run offline.
    #[tokio::test]
    async fn guard_url_blocks_loopback_and_metadata_literals() {
        assert!(guard_url("http://127.0.0.1:9201/").await.is_err());
        assert!(guard_url("http://169.254.169.254/latest/meta-data/").await.is_err());
        assert!(guard_url("http://[::1]:8080/").await.is_err());
    }

    #[tokio::test]
    async fn guard_url_allows_public_literal() {
        assert!(guard_url("https://1.1.1.1/").await.is_ok());
    }
}
