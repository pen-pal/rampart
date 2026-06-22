//! Trusted client-IP resolution (rate-limiting + audit source IP).
//!
//! The naive "leftmost `X-Forwarded-For`" approach is spoofable: any client
//! can send `X-Forwarded-For: <anything>` and forge its apparent source IP,
//! defeating per-IP rate limits and poisoning the audit trail. Instead we
//! derive the client IP from the **TCP peer** (axum `ConnectInfo`) and only
//! trust `X-Forwarded-For` when the peer itself is an allow-listed proxy.
//!
//! ## Resolution rules
//!
//! - No peer (e.g. `oneshot` tests with no `ConnectInfo`) → `None`; the
//!   caller treats it as "no IP" and passes through.
//! - Peer is **not** in `RAMPART_TRUSTED_PROXIES` → the peer IS the client
//!   identity; `X-Forwarded-For` is **ignored entirely** (the secure default).
//! - Peer **is** a trusted proxy → walk `X-Forwarded-For` right-to-left and
//!   return the first entry that is *not* itself trusted (the real client
//!   behind the trusted hop chain). If none qualifies (empty / all-trusted /
//!   garbage) we fall back to the peer — never to a client-forged value.
//!
//! ## Why `RAMPART_TRUSTED_PROXIES` should be a SPECIFIC IP, not a broad CIDR
//!
//! Any host inside a trusted CIDR can forge the client IP via XFF. Set it to
//! the exact proxy/LB address(es) (e.g. `203.0.113.10` or a `/32`), NOT a
//! broad internal range like `10.0.0.0/8` that also contains untrusted
//! pods/tenants.
//!
//! ## Dual-stack
//!
//! A `[::]` (IPv6 wildcard) bind sees IPv4 clients as v4-mapped-v6
//! (`::ffff:a.b.c.d`). We canonicalize every address with
//! [`IpAddr::to_canonical`] before matching so a v4-mapped peer still matches
//! an IPv4 trusted CIDR — without this the match silently fails and every v4
//! client collapses into one shared rate-limit bucket / audit IP.

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use sqlx::types::ipnetwork::IpNetwork;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

/// Internal, server-set header carrying the resolved + trusted client IP.
/// The middleware STRIPS any inbound copy and re-sets it from the resolver so
/// downstream code reading only a `&HeaderMap` (the audit path) sees a value a
/// client cannot forge.
pub const CLIENT_IP_HEADER: &str = "x-rampart-client-ip";

/// Allow-list of reverse proxies that front Rampart, parsed once from
/// `RAMPART_TRUSTED_PROXIES`. Cheap to clone (Arc-backed).
#[derive(Clone, Default)]
pub struct TrustedProxies(Arc<Vec<IpNetwork>>);

impl TrustedProxies {
    /// Parse `RAMPART_TRUSTED_PROXIES` (comma-separated IPs and/or CIDRs).
    ///
    /// Each entry is tried as an `IpNetwork` (handles `10.0.0.0/8`); on failure
    /// it's tried as a bare `IpAddr` (a host route, i.e. `/32` or `/128`).
    /// Entries are trimmed; blanks are skipped. A malformed entry is logged and
    /// skipped — it never fails startup (an unparseable allow-list entry should
    /// not take the whole server down).
    pub fn from_env() -> Self {
        let raw = std::env::var("RAMPART_TRUSTED_PROXIES").unwrap_or_default();
        let mut nets = Vec::new();
        for entry in raw.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if let Ok(net) = IpNetwork::from_str(entry) {
                nets.push(net);
            } else if let Ok(ip) = IpAddr::from_str(entry) {
                nets.push(IpNetwork::from(ip));
            } else {
                tracing::warn!(
                    entry,
                    "RAMPART_TRUSTED_PROXIES: skipping unparseable entry (expected an IP or CIDR)"
                );
            }
        }
        Self(Arc::new(nets))
    }

    /// Whether `ip` falls inside any trusted proxy network. Canonicalizes
    /// first so a v4-mapped-v6 address (`::ffff:a.b.c.d`) matches an IPv4 CIDR.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.0.iter().any(|n| n.contains(ip))
    }

    /// Whether the allow-list is empty (no trusted proxies configured).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Read the trusted, resolved client IP out of a `&HeaderMap` (the internal
/// [`CLIENT_IP_HEADER`] set by [`resolve_client_ip`]). Handlers that only have
/// the request headers (e.g. session creation) use this so the recorded IP is
/// the trusted value, not a spoofable raw `X-Forwarded-For`. Returns `None`
/// when no peer was resolvable (e.g. in-process tests with no `ConnectInfo`).
pub fn from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    let raw = headers.get(CLIENT_IP_HEADER)?.to_str().ok()?.trim();
    IpAddr::from_str(raw).ok()
}

/// The resolved client IP, stamped into request extensions by the middleware.
/// `None` means "no resolvable peer" (e.g. in-process tests with no
/// `ConnectInfo`).
#[derive(Clone, Copy)]
pub struct ResolvedClientIp(pub Option<IpAddr>);

/// Resolve the trusted client IP from the TCP peer + headers + allow-list.
///
/// See the module docs for the full rule set.
pub fn resolve(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted: &TrustedProxies,
) -> Option<IpAddr> {
    // Missing ConnectInfo (e.g. oneshot tests) → None → caller passes through.
    let peer = peer?.to_canonical();

    // Untrusted / direct peer IS the identity; ignore any XFF it may have set.
    if !trusted.contains(peer) {
        return Some(peer);
    }

    // Trusted proxy: walk XFF right-to-left and return the first entry that is
    // not itself a trusted hop — that's the real client behind the chain.
    // Combine ALL `X-Forwarded-For` header lines in received order first
    // (RFC 7230 §3.2.2: repeated field lines are one comma-joined list). Using
    // only `headers.get()` would see just the FIRST line, so a proxy that
    // splits XFF across multiple lines could let an attacker hide the real
    // client in a later line — walking the joined list closes that gap.
    let joined = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join(",");
    for raw in joined.rsplit(',') {
        if let Ok(parsed) = IpAddr::from_str(raw.trim()) {
            let canon = parsed.to_canonical();
            if !trusted.contains(canon) {
                return Some(canon);
            }
        }
    }

    // Empty / all-trusted / garbage XFF → fall back to the peer (never a
    // client-forged value).
    Some(peer)
}

/// Middleware: resolve the trusted client IP and make it available downstream.
///
/// Stamps a [`ResolvedClientIp`] extension (read by the rate limiter) and the
/// internal [`CLIENT_IP_HEADER`] (read by the audit path, which only has a
/// `&HeaderMap`). Any inbound copy of the internal header is stripped first so
/// a client can't forge the trusted value.
pub async fn resolve_client_ip(
    State(trusted): State<TrustedProxies>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // Non-panicking Option form — never the `ConnectInfo` extractor, which
    // would 500 the request if the listener wasn't served with connect info.
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());

    let resolved = resolve(peer, req.headers(), &trusted);
    req.extensions_mut().insert(ResolvedClientIp(resolved));

    // Always strip any inbound copy of the internal header, then set the
    // trusted value (when we have one) so it can't be forged.
    req.headers_mut().remove(CLIENT_IP_HEADER);
    if let Some(ip) = resolved {
        if let Ok(value) = ip.to_string().parse() {
            req.headers_mut().insert(CLIENT_IP_HEADER, value);
        }
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn trusted(entries: &[&str]) -> TrustedProxies {
        // Build directly (don't touch the process env) so tests are isolated.
        let nets = entries
            .iter()
            .map(|e| {
                IpNetwork::from_str(e)
                    .or_else(|_| IpAddr::from_str(e).map(IpNetwork::from))
                    .unwrap()
            })
            .collect();
        TrustedProxies(Arc::new(nets))
    }

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn headers_xff(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", value.parse().unwrap());
        h
    }

    #[test]
    fn no_peer_resolves_to_none() {
        let t = trusted(&[]);
        assert_eq!(resolve(None, &HeaderMap::new(), &t), None);
    }

    #[test]
    fn untrusted_peer_uses_peer_and_ignores_xff() {
        // Even with a forged XFF, an untrusted/direct peer is the identity.
        let t = trusted(&["203.0.113.10"]);
        let peer = v4(198, 51, 100, 7);
        let h = headers_xff("1.2.3.4");
        assert_eq!(resolve(Some(peer), &h, &t), Some(peer));
    }

    #[test]
    fn no_trusted_proxies_ignores_xff() {
        // Secure default: empty allow-list → always the peer.
        let t = trusted(&[]);
        let peer = v4(198, 51, 100, 7);
        let h = headers_xff("1.2.3.4");
        assert!(t.is_empty());
        assert_eq!(resolve(Some(peer), &h, &t), Some(peer));
    }

    #[test]
    fn trusted_peer_returns_rightmost_non_trusted_xff() {
        let t = trusted(&["10.0.0.1"]);
        let peer = v4(10, 0, 0, 1);
        // Rightmost non-trusted entry is the real client.
        let h = headers_xff("9.9.9.9, 203.0.113.5");
        assert_eq!(resolve(Some(peer), &h, &t), Some(v4(203, 0, 113, 5)));
    }

    #[test]
    fn trusted_peer_skips_multiple_trusted_hops_right_to_left() {
        let t = trusted(&["10.0.0.0/8"]);
        let peer = v4(10, 0, 0, 1);
        // Two trusted internal hops on the right; the client is to their left.
        let h = headers_xff("203.0.113.5, 10.1.2.3, 10.4.5.6");
        assert_eq!(resolve(Some(peer), &h, &t), Some(v4(203, 0, 113, 5)));
    }

    #[test]
    fn trusted_peer_all_xff_trusted_falls_back_to_peer() {
        let t = trusted(&["10.0.0.0/8"]);
        let peer = v4(10, 0, 0, 1);
        let h = headers_xff("10.1.1.1, 10.2.2.2");
        assert_eq!(resolve(Some(peer), &h, &t), Some(peer));
    }

    #[test]
    fn trusted_peer_garbage_or_empty_xff_falls_back_to_peer() {
        let t = trusted(&["10.0.0.1"]);
        let peer = v4(10, 0, 0, 1);
        assert_eq!(
            resolve(Some(peer), &headers_xff("not-an-ip, also bad"), &t),
            Some(peer)
        );
        assert_eq!(resolve(Some(peer), &HeaderMap::new(), &t), Some(peer));
    }

    #[test]
    fn v4_mapped_v6_peer_matches_v4_cidr() {
        // CRITICAL: a [::] bind sees a v4 client as ::ffff:10.0.0.5. After
        // canonicalization it must match the 10.0.0.0/8 trusted CIDR, so the
        // XFF behind it is honoured rather than the whole thing collapsing.
        let t = trusted(&["10.0.0.0/8"]);
        let peer = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0a00, 0x0005));
        assert!(t.contains(peer), "v4-mapped-v6 peer must match v4 CIDR");
        let h = headers_xff("203.0.113.9");
        assert_eq!(resolve(Some(peer), &h, &t), Some(v4(203, 0, 113, 9)));
    }

    #[test]
    fn ipv6_trusted_cidr_with_v6_peer() {
        let t = trusted(&["2001:db8::/32"]);
        let peer = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        assert!(t.contains(peer));
        let client = IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111));
        let h = headers_xff("2606:4700::1111");
        assert_eq!(resolve(Some(peer), &h, &t), Some(client));
    }

    #[test]
    fn multiline_xff_is_walked_across_all_lines() {
        // A proxy that splits XFF across MULTIPLE header lines must not let an
        // attacker hide the real client. Line 1 is attacker-forged; line 2 is
        // what the trusted proxy actually appended. Joined in order =
        // "1.1.1.1,203.0.113.5,10.0.0.1"; right-to-left skips the trusted hop
        // and returns the real client, never the forged first-line value.
        let t = trusted(&["10.0.0.1"]);
        let peer = v4(10, 0, 0, 1);
        let mut h = HeaderMap::new();
        h.append("x-forwarded-for", "1.1.1.1".parse().unwrap());
        h.append("x-forwarded-for", "203.0.113.5, 10.0.0.1".parse().unwrap());
        assert_eq!(resolve(Some(peer), &h, &t), Some(v4(203, 0, 113, 5)));
    }
}
