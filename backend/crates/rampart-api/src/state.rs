//! Shared application state.

use crate::http_metrics::HttpMetrics;
use crate::rate_limit::IpRateLimiter;
use rampart_core::heartbeat::Heartbeat;
use rampart_core::UserId;
use rampart_db::DbPool;
use rampart_scheduler::Scheduler;
use std::collections::HashMap;
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::sync::{broadcast, Mutex, Notify};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    pool: DbPool,
    /// Notify handle that triggers the scheduler to reconcile after
    /// monitor mutations (create / delete / pause / resume).
    scheduler_reload: Arc<Notify>,
    /// Optional — only present when the API was constructed with a live
    /// scheduler. Test harnesses can construct AppState without one.
    scheduler: Option<Arc<Scheduler>>,
    /// In-flight TOTP login challenges. Keyed by an opaque token we
    /// hand back to the browser; we look up the user_id when the user
    /// submits their 6-digit code. Lives in memory because the dataset
    /// is tiny (one entry per logged-in user, briefly) and persisting
    /// it adds nothing.
    totp_challenges: Mutex<HashMap<Uuid, TotpChallenge>>,
    /// In-process HTTP request counters + latency histogram, read by
    /// the `/metrics` Prometheus endpoint. One shared instance per
    /// AppState; the middleware in `lib.rs::build_router` calls
    /// `observe()` on every served request.
    http_metrics: Arc<HttpMetrics>,
    /// Per-client-IP token bucket. Layered as middleware on the auth
    /// router (login / register / 2fa verify) to cap brute-force
    /// attempts. Cloning is cheap (Arc-shared).
    auth_rate_limiter: IpRateLimiter,
}

pub struct TotpChallenge {
    pub user_id: UserId,
    pub expires_at: OffsetDateTime,
}

impl AppState {
    pub fn new(pool: DbPool, scheduler_reload: Arc<Notify>) -> Self {
        Self(Arc::new(Inner {
            pool,
            scheduler_reload,
            scheduler: None,
            totp_challenges: Mutex::new(HashMap::new()),
            http_metrics: Arc::new(HttpMetrics::new()),
            auth_rate_limiter: IpRateLimiter::new(),
        }))
    }

    pub fn with_scheduler(
        pool: DbPool,
        scheduler_reload: Arc<Notify>,
        scheduler: Arc<Scheduler>,
    ) -> Self {
        Self(Arc::new(Inner {
            pool,
            scheduler_reload,
            scheduler: Some(scheduler),
            totp_challenges: Mutex::new(HashMap::new()),
            http_metrics: Arc::new(HttpMetrics::new()),
            auth_rate_limiter: IpRateLimiter::new(),
        }))
    }

    pub fn http_metrics(&self) -> &Arc<HttpMetrics> {
        &self.0.http_metrics
    }

    pub fn auth_rate_limiter(&self) -> IpRateLimiter {
        self.0.auth_rate_limiter.clone()
    }

    pub fn pool(&self) -> &DbPool {
        &self.0.pool
    }
    /// Call after any monitor mutation so the scheduler picks it up
    /// without waiting for the slow 30-second fallback tick.
    pub fn poke_scheduler(&self) {
        self.0.scheduler_reload.notify_one();
    }

    /// Subscribe to the live heartbeat stream when the scheduler is
    /// wired in. Returns `None` in test harnesses that constructed the
    /// state without one.
    pub fn subscribe_heartbeats(&self) -> Option<broadcast::Receiver<Heartbeat>> {
        self.0.scheduler.as_ref().map(|s| s.subscribe_heartbeats())
    }

    /// Stash a pending TOTP challenge for `user`. Returns the opaque
    /// token to hand back to the browser. Challenges expire after 5
    /// minutes — long enough to handle the user fumbling for their
    /// phone, short enough to bound replay risk if the token leaks.
    pub async fn issue_totp_challenge(&self, user_id: UserId) -> Uuid {
        let token = Uuid::new_v4();
        let mut g = self.0.totp_challenges.lock().await;
        // Opportunistic cleanup — cheap O(n) sweep on the path that
        // adds new entries, so the map can't grow unbounded.
        let now = OffsetDateTime::now_utc();
        g.retain(|_, c| c.expires_at > now);
        g.insert(
            token,
            TotpChallenge {
                user_id,
                expires_at: now + time::Duration::minutes(5),
            },
        );
        token
    }

    pub async fn consume_totp_challenge(&self, token: Uuid) -> Option<UserId> {
        let mut g = self.0.totp_challenges.lock().await;
        let c = g.remove(&token)?;
        if c.expires_at < OffsetDateTime::now_utc() {
            return None;
        }
        Some(c.user_id)
    }
}
