//! Shared application state.

use rampart_db::DbPool;
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    pool: DbPool,
    /// Notify handle that triggers the scheduler to reconcile after
    /// monitor mutations (create / delete / pause / resume).
    scheduler_reload: Arc<Notify>,
}

impl AppState {
    pub fn new(pool: DbPool, scheduler_reload: Arc<Notify>) -> Self {
        Self(Arc::new(Inner {
            pool,
            scheduler_reload,
        }))
    }
    pub fn pool(&self) -> &DbPool {
        &self.0.pool
    }
    /// Call after any monitor mutation so the scheduler picks it up
    /// without waiting for the slow 30-second fallback tick.
    pub fn poke_scheduler(&self) {
        self.0.scheduler_reload.notify_one();
    }
}
