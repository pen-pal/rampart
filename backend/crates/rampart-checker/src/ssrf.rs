//! SSRF guard for outbound probes.
//!
//! Rampart probes operator/editor-defined targets, so it is inherently an
//! outbound-request engine. To stop it being abused to reach the cloud
//! metadata endpoint or internal-only services, every probe that takes a
//! user-supplied host resolves it through [`resolve_guarded`] first:
//!
//!   * **Always blocked** (no legitimate uptime reason): loopback, link-local
//!     incl. the cloud metadata IP `169.254.169.254`, the v6 equivalents, and
//!     unspecified/broadcast addresses.
//!   * **Private ranges** (RFC1918, CGNAT 100.64/10, IPv6 ULA fc00::/7) are
//!     blocked only when `RAMPART_SSRF_BLOCK_PRIVATE` is set — homelabs
//!     legitimately monitor private IPs, so it is opt-in, but recommended for
//!     multi-user / internet-exposed deployments.
//!
//! Callers connect to the returned, vetted [`SocketAddr`]s (pin them — e.g.
//! `reqwest`'s `resolve_to_addrs`) so a DNS rebind can't swap in a blocked IP
//! between the check and the connect.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// A probe target rejected by the guard.
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

/// Whether to also block private/internal ranges (env `RAMPART_SSRF_BLOCK_PRIVATE`).
pub fn block_private_enabled() -> bool {
    matches!(
        std::env::var("RAMPART_SSRF_BLOCK_PRIVATE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

/// IPs that must never be probed regardless of configuration.
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
}
