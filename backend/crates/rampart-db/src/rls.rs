//! Row-Level Security: per-request tenant binding (defense-in-depth).
//!
//! App-level `WHERE org_id = $org` is the primary tenant filter and already
//! ships + is tested. Postgres RLS is belt-and-suspenders behind the
//! `RAMPART_RLS` env flag (default OFF, fully reversible). When on, the pool's
//! `before_acquire` hook (see [`crate::connect`]) reads the task-local
//! [`CURRENT_ORG`] of the running request and, if an org is bound, switches the
//! checked-out connection into the `rampart_app` role with
//! `app.current_org` set to that org so the dormant org-isolation policies
//! (migration 0115, enabled only by an operator) scope every statement. System
//! loops never set the task-local, so they run as the BYPASSRLS owner and are
//! unaffected.
//!
//! Nothing here changes any `pool: &DbPool` repository signature: the binding
//! flows implicitly through the task-local, set by [`with_org`] at the request
//! chokepoints (auth middleware + the ingest handlers).

use rampart_core::ids::OrgId;
use std::future::Future;
use uuid::Uuid;

tokio::task_local! {
    /// The org the current request/task is acting in, if any. `None` (or simply
    /// unset) means "no tenant bound" — the connection runs as the owner and
    /// bypasses RLS, which is exactly what the system loops want.
    pub static CURRENT_ORG: Option<Uuid>;
}

/// Run `fut` with the current org bound to `org` for the duration of the
/// future. The pool's `before_acquire` hook reads this to bind the org onto any
/// connection checked out while the future runs.
pub async fn with_org<F: Future>(org: OrgId, fut: F) -> F::Output {
    CURRENT_ORG.scope(Some(org.0), fut).await
}

/// Whether RLS connection-binding is enabled (`RAMPART_RLS=1`/`true`/`TRUE`/
/// `yes`). Default OFF. Mirrors the truthy parse used by the ingest ops flags
/// (`rampart_api::ingest_util::flag_set`) so operators have one mental model.
pub fn rls_enabled() -> bool {
    matches!(
        std::env::var("RAMPART_RLS").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn rls_disabled_by_default() {
        // The full test suite runs with RAMPART_RLS unset; assert that means
        // OFF (flag-off must be byte-identical to today).
        std::env::remove_var("RAMPART_RLS");
        assert!(!rls_enabled());
    }

    #[tokio::test]
    async fn with_org_binds_task_local() {
        let org = OrgId::new();
        let seen = with_org(org, async { CURRENT_ORG.get() }).await;
        assert_eq!(seen, Some(org.0));
    }

    #[tokio::test]
    async fn current_org_unset_outside_scope() {
        // Reading the task-local outside any scope yields the default (None).
        // `try_with` returns Err when unset; that's the "system loop" path.
        let bound = CURRENT_ORG.try_with(|o| *o);
        assert!(bound.is_err(), "no org should be bound outside with_org");
        let _ = Uuid::nil();
    }
}
