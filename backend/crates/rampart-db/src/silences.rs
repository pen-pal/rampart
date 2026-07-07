//! Alert silences (migration 0088).
//!
//! A silence suppresses notifications while active. `monitor_id = NULL` is a
//! global mute (covers monitor *and* rule alerts); a set `monitor_id` mutes just
//! that monitor. The notifier checks [`is_silenced`] at its single dispatch
//! chokepoint, so every alert path (status flip, SLO, metric/telemetry rules,
//! escalation) honours it. See `docs/design/ALERT-RULES.md`.

use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct NewSilence<'a> {
    /// None = global (mute everything).
    pub monitor_id: Option<Uuid>,
    pub reason: &'a str,
    pub created_by: Option<Uuid>,
    /// None = until manually removed.
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
pub struct Silence {
    pub id: Uuid,
    pub monitor_id: Option<Uuid>,
    pub monitor_name: Option<String>,
    pub reason: String,
    pub created_at: OffsetDateTime,
    pub expires_at: Option<OffsetDateTime>,
}

/// Is an alert for `monitor` currently silenced? `None` (a rule / non-monitor
/// alert) matches only global silences; `Some(id)` matches global + that
/// monitor's silences. One indexed EXISTS.
pub async fn is_silenced(pool: &DbPool, monitor: Option<Uuid>) -> DbResult<bool> {
    let row = sqlx::query!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM silences
            WHERE (expires_at IS NULL OR expires_at > now())
              AND (
                monitor_id = $1
                OR (monitor_id IS NULL AND (
                     $1::uuid IS NULL
                     OR org_id = (SELECT org_id FROM monitors WHERE id = $1)
                ))
              )
        ) AS "silenced!"
        "#,
        monitor,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.silenced)
}

pub async fn create(
    pool: &DbPool,
    s: NewSilence<'_>,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<Uuid> {
    let row = sqlx::query!(
        r#"
        INSERT INTO silences (monitor_id, reason, created_by, expires_at, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
        s.monitor_id,
        s.reason,
        s.created_by,
        s.expires_at,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// Active (unexpired) silences for one org, newest first, monitor name
/// resolved. Org-scoped — a silence belongs to the org that created it.
pub async fn list_active(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<Silence>> {
    let rows = sqlx::query!(
        r#"
        SELECT s.id, s.monitor_id, m.name AS "monitor_name?",
               s.reason, s.created_at, s.expires_at
        FROM silences s
        LEFT JOIN monitors m ON m.id = s.monitor_id
        WHERE s.org_id = $1
          AND (s.expires_at IS NULL OR s.expires_at > now())
        ORDER BY s.created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Silence {
            id: r.id,
            monitor_id: r.monitor_id,
            monitor_name: r.monitor_name,
            reason: r.reason,
            created_at: r.created_at,
            expires_at: r.expires_at,
        })
        .collect())
}

pub async fn delete(pool: &DbPool, id: Uuid, org_id: OrgId) -> DbResult<bool> {
    let r = sqlx::query!(
        "DELETE FROM silences WHERE id = $1 AND org_id = $2",
        id,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::org::DEFAULT_ORG_ID;
    use sqlx::PgPool;

    fn def_org() -> OrgId {
        OrgId::from_uuid(DEFAULT_ORG_ID)
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn global_and_scoped_silences(pool: PgPool) {
        // A real monitor in the Default org (a global silence scopes to a
        // monitor's org, derived from the monitors table).
        let nm: rampart_core::monitor::NewMonitor = serde_json::from_value(
            serde_json::json!({"name":"m","kind":"http","url":"https://x.test"}),
        )
        .unwrap();
        let mon = crate::monitors::create(&pool, nm, def_org())
            .await
            .unwrap()
            .id
            .0;
        // Nothing silenced initially.
        assert!(!is_silenced(&pool, Some(mon)).await.unwrap());
        assert!(!is_silenced(&pool, None).await.unwrap());

        // A global silence in the Default org mutes that org's monitor AND rules (None).
        let g = create(
            &pool,
            NewSilence {
                monitor_id: None,
                reason: "deploy",
                created_by: None,
                expires_at: None,
            },
            def_org(),
        )
        .await
        .unwrap();
        assert!(is_silenced(&pool, Some(mon)).await.unwrap());
        assert!(is_silenced(&pool, None).await.unwrap());

        // Cross-org: a monitor in ANOTHER org is NOT muted by the Default org's
        // global silence — the isolation this fix adds.
        let other = crate::orgs::create(&pool, "other", "Other")
            .await
            .unwrap()
            .id;
        let onm: rampart_core::monitor::NewMonitor = serde_json::from_value(
            serde_json::json!({"name":"o","kind":"http","url":"https://y.test"}),
        )
        .unwrap();
        let omon = crate::monitors::create(&pool, onm, other)
            .await
            .unwrap()
            .id
            .0;
        assert!(
            !is_silenced(&pool, Some(omon)).await.unwrap(),
            "another org's monitor must not be muted by this org's global silence"
        );

        // list_active is org-scoped; the silence landed in the Default org.
        assert_eq!(list_active(&pool, def_org()).await.unwrap().len(), 1);
        assert!(delete(&pool, g, def_org()).await.unwrap());
        assert!(!is_silenced(&pool, None).await.unwrap());
        assert!(!is_silenced(&pool, Some(mon)).await.unwrap());

        // An expired silence doesn't count.
        create(
            &pool,
            NewSilence {
                monitor_id: None,
                reason: "old",
                created_by: None,
                expires_at: Some(OffsetDateTime::now_utc() - time::Duration::hours(1)),
            },
            def_org(),
        )
        .await
        .unwrap();
        assert!(!is_silenced(&pool, None).await.unwrap());
    }
}
