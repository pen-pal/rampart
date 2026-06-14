//! Per-IP rate limiting for high-value endpoints (auth surface only).
//!
//! Token-bucket per client IP, kept in a process-local mutex-protected
//! HashMap. Designed for the brute-force-defence shape, not for
//! general throughput limiting — only login + register + TOTP-verify
//! attach this layer in `routes/auth.rs::router`.
//!
//! Buckets:
//!
//! - capacity = 10 tokens
//! - refill = 1 token / 6 seconds (so the long-term average sits at
//!   ~10 attempts/minute per IP — fast enough that a user fumbling
//!   their password isn't locked out, slow enough that a brute-force
//!   run gets capped at ~600 attempts/hour from one address)
//!
//! Limiter dimensions one IP-keyed bucket at startup; subsequent
//! requests hit the same buckets cheaply (one HashMap lookup + a
//! 16-byte struct mutation under the bucket-list lock).
//!
//! Garbage collection: opportunistic, on each insert. The IP map is
//! swept of buckets that haven't been touched in the last hour. Bounded
//! growth even under a slowloris-style scanner that mints new IPs.
//!
//! The same limiter instance is shared via AppState so all auth
//! routes converge on one consistent view of how many attempts each
//! IP has burned.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Bucket parameters — chosen for the brute-force-defence shape.
pub const BUCKET_CAPACITY: f64 = 10.0;
pub const REFILL_PER_SECOND: f64 = 1.0 / 6.0;
const GC_INTERVAL: Duration = Duration::from_secs(60);
const GC_IDLE_THRESHOLD: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
    last_touched: Instant,
}

impl Bucket {
    fn new(now: Instant, capacity: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: now,
            last_touched: now,
        }
    }

    /// Returns true if a request is allowed. Refills the bucket
    /// according to elapsed time, then deducts one token. Returning
    /// false means the caller should reject with 429.
    fn take(&mut self, now: Instant, capacity: f64, refill: f64) -> bool {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        self.tokens = (self.tokens + elapsed * refill).min(capacity);
        self.last_refill = now;
        self.last_touched = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// In-process per-IP rate limiter. Cheap to clone (Arc-shared).
#[derive(Clone)]
pub struct IpRateLimiter {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    buckets: HashMap<IpAddr, Bucket>,
    last_gc: Instant,
    capacity: f64,
    refill: f64,
}

impl Default for IpRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl IpRateLimiter {
    /// Brute-force-defence shape (auth surface): 10 burst, ~10/min refill.
    pub fn new() -> Self {
        Self::with_params(BUCKET_CAPACITY, REFILL_PER_SECOND)
    }

    /// A limiter with custom bucket params — e.g. the ingest surface uses a
    /// much larger burst + faster refill since legitimate telemetry is
    /// frequent, while still capping a single IP's flood.
    pub fn with_params(capacity: f64, refill: f64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buckets: HashMap::new(),
                last_gc: Instant::now(),
                capacity,
                refill,
            })),
        }
    }

    /// Check whether the given IP can spend one token. The first
    /// request from an IP is always allowed (the bucket starts full).
    pub fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut g = self.inner.lock().unwrap();

        // Opportunistic GC. Cheap O(buckets) sweep at most once a
        // minute keeps the map from growing without bound.
        if now.duration_since(g.last_gc) >= GC_INTERVAL {
            g.buckets
                .retain(|_, b| now.duration_since(b.last_touched) < GC_IDLE_THRESHOLD);
            g.last_gc = now;
        }

        let (capacity, refill) = (g.capacity, g.refill);
        let bucket = g.buckets.entry(ip).or_insert_with(|| Bucket::new(now, capacity));
        bucket.take(now, capacity, refill)
    }

    #[cfg(test)]
    pub fn current_tokens(&self, ip: IpAddr) -> Option<f64> {
        self.inner
            .lock()
            .unwrap()
            .buckets
            .get(&ip)
            .map(|b| b.tokens)
    }
}

/// Axum middleware: applies the limiter to requests whose client IP
/// resolved successfully. Requests with no resolvable IP (no
/// X-Forwarded-For, no X-Real-IP) pass through — refusing them would
/// break local-host testing + lock out operators behind proxies that
/// don't forward the original IP. Operators wanting stricter
/// behaviour should configure their proxy to set X-Forwarded-For.
pub async fn enforce_ip_rate_limit(
    state: axum::extract::State<IpRateLimiter>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if let Some(ip) = client_ip(req.headers()) {
        if !state.0.check(ip) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "6")],
                "rate limit exceeded — try again shortly",
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Pull the client IP from the headers a reverse proxy in front of
/// Rampart would set. Same precedence as `crate::audit::client_ip` —
/// X-Forwarded-For first (left-most entry), X-Real-IP fallback. Returns
/// None when neither is present.
fn client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let raw = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))?
        .to_str()
        .ok()?
        .split(',')
        .next()?
        .trim();
    IpAddr::from_str(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))
    }

    #[test]
    fn first_burst_of_capacity_passes_then_rejects() {
        let lim = IpRateLimiter::new();
        for _ in 0..(BUCKET_CAPACITY as u32) {
            assert!(lim.check(ip()));
        }
        // 11th immediate request should be rejected.
        assert!(!lim.check(ip()));
    }

    #[test]
    fn separate_ips_have_separate_buckets() {
        let lim = IpRateLimiter::new();
        let a = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let b = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        for _ in 0..(BUCKET_CAPACITY as u32) {
            assert!(lim.check(a));
        }
        // a is exhausted; b still has its full bucket.
        assert!(!lim.check(a));
        assert!(lim.check(b));
    }
}
