//! MySQL `silences` domain — alert suppression. `is_silenced` is the single
//! dispatch chokepoint every alert flows through; `create`/`list_active`/
//! `delete` are the management surface. Ported from PG. Dialect: uuid→CHAR(36),
//! ts→BIGINT unix-seconds. No `RETURNING` → app-side UUID; `now()` → bound
//! cutoff; `id` is CHAR(36) (PG defaults the uuid, MySQL needs it supplied).

use super::{raw_uuid, ts};
use crate::silences::{NewSilence, Silence};
use crate::DbResult;
use rampart_core::ids::OrgId;
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

/// Is any active silence covering `monitor` (or a global silence)? Mirrors PG:
/// a silence matches when not expired AND (global OR scoped to this monitor).
pub async fn is_silenced(
    pool: &MySqlPool,
    monitor: Option<Uuid>,
    channel: Option<Uuid>,
) -> DbResult<bool> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    // A GLOBAL silence only mutes its own org; the alert's org comes from the
    // monitor row, else from the channel being paged (see the PG fn doc).
    let (n,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM silences
            WHERE (expires_at IS NULL OR expires_at > ?)
              AND (
                monitor_id = ?
                OR (monitor_id IS NULL AND org_id = COALESCE(
                     (SELECT org_id FROM monitors WHERE id = ?),
                     (SELECT org_id FROM notifications WHERE id = ?)
                ))
              )
        )",
    )
    .bind(now)
    .bind(monitor.map(|m| m.to_string()))
    .bind(monitor.map(|m| m.to_string()))
    .bind(channel.map(|c| c.to_string()))
    .fetch_one(pool)
    .await?;
    Ok(n != 0)
}

pub async fn create(pool: &MySqlPool, s: NewSilence<'_>, org_id: OrgId) -> DbResult<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO silences (id, monitor_id, reason, created_by, expires_at, org_id)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(s.monitor_id.map(|m| m.to_string()))
    .bind(s.reason)
    .bind(s.created_by.map(|u| u.to_string()))
    .bind(s.expires_at.map(|t| t.unix_timestamp()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    Ok(id)
}

/// Active (unexpired) silences for one org, newest first, monitor name resolved.
pub async fn list_active(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Silence>> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let rows = sqlx::query(
        "SELECT s.id, s.monitor_id, m.name AS monitor_name, s.reason, s.created_at, s.expires_at
         FROM silences s
         LEFT JOIN monitors m ON m.id = s.monitor_id
         WHERE s.org_id = ? AND (s.expires_at IS NULL OR s.expires_at > ?)
         ORDER BY s.created_at DESC",
    )
    .bind(org_id.0.to_string())
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Silence {
            id: raw_uuid(&r.get::<String, _>("id")),
            monitor_id: r
                .get::<Option<String>, _>("monitor_id")
                .map(|s| raw_uuid(&s)),
            monitor_name: r.get("monitor_name"),
            reason: r.get::<Option<String>, _>("reason").unwrap_or_default(),
            created_at: ts(r.get::<i64, _>("created_at")),
            expires_at: r.get::<Option<i64>, _>("expires_at").map(ts),
        })
        .collect())
}

pub async fn delete(pool: &MySqlPool, id: Uuid, org_id: OrgId) -> DbResult<bool> {
    let r = sqlx::query("DELETE FROM silences WHERE id = ? AND org_id = ?")
        .bind(id.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn global_scoped_and_expired(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        // A real monitor: a global silence only mutes monitors in its own org,
        // and the org is derived from the monitors row (v0.157.30).
        let new_monitor = |name: &str| -> rampart_core::monitor::NewMonitor {
            serde_json::from_value(
                serde_json::json!({"name": name, "kind": "http", "url": "https://x.test"}),
            )
            .unwrap()
        };
        let mon = super::super::monitors::create(&pool, new_monitor("m"), org)
            .await
            .unwrap()
            .id
            .0;
        // Channels: a rule / error alert has no monitor, so the org a global
        // silence is matched against comes from the channel being paged.
        let new_chan = |name: &str| -> crate::notifications::NewNotification {
            serde_json::from_value(serde_json::json!({
                "kind": "webhook", "name": name, "config": {"url": "https://hooks.example/x"}
            }))
            .unwrap()
        };
        let chan = super::super::notifications::create(&pool, new_chan("c"), org)
            .await
            .unwrap()
            .id
            .0;
        assert!(!is_silenced(&pool, Some(mon), None).await.unwrap());

        // global silence mutes the monitor AND rule (None) alerts.
        let g = create(
            &pool,
            NewSilence {
                monitor_id: None,
                reason: "deploy",
                created_by: None,
                expires_at: None,
            },
            org,
        )
        .await
        .unwrap();
        assert!(is_silenced(&pool, Some(mon), None).await.unwrap());
        assert!(is_silenced(&pool, None, Some(chan)).await.unwrap());
        assert_eq!(list_active(&pool, org).await.unwrap().len(), 1);

        // cross-org delete is a no-op; same-org delete works.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        // ...and another org's monitor is NOT muted by this org's global silence.
        let omon = super::super::monitors::create(&pool, new_monitor("o"), other.id)
            .await
            .unwrap()
            .id
            .0;
        assert!(!is_silenced(&pool, Some(omon), None).await.unwrap());
        // ...nor is an alert paging another org's channel, nor one with no org.
        let ochan = super::super::notifications::create(&pool, new_chan("oc"), other.id)
            .await
            .unwrap()
            .id
            .0;
        assert!(!is_silenced(&pool, None, Some(ochan)).await.unwrap());
        assert!(!is_silenced(&pool, None, None).await.unwrap());
        assert!(!delete(&pool, g, other.id).await.unwrap());
        assert!(delete(&pool, g, org).await.unwrap());
        assert!(!is_silenced(&pool, None, Some(chan)).await.unwrap());

        // expired silence doesn't count.
        create(
            &pool,
            NewSilence {
                monitor_id: None,
                reason: "old",
                created_by: None,
                expires_at: Some(OffsetDateTime::now_utc() - time::Duration::hours(1)),
            },
            org,
        )
        .await
        .unwrap();
        assert!(!is_silenced(&pool, None, Some(chan)).await.unwrap());
        assert!(list_active(&pool, org).await.unwrap().is_empty());
    }
}
