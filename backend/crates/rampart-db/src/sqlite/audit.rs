//! SQLite `audit` domain — append-only, tamper-evident audit log. Mirrors the
//! PG free-fn surface: insert / set_chain_watermark / verify_chain /
//! security_insights / list / fetch_since / export_batch. Structs reused from
//! `crate::audit`.
//!
//! The hash chain reuses `crate::audit::chain_hash` VERBATIM so insert and
//! verify feed byte-identical inputs (self-consistency within SQLite). IP is
//! stored + hashed + displayed as the plain `IpAddr` string (no PG INET / `/32`
//! mask). PG-isms translated: `pg_advisory_xact_lock` dropped (a SQLite write
//! transaction already serializes appends); `COUNT(*) FILTER` → `SUM(CASE …)`;
//! `make_interval(hours=>?)` → `unixepoch() - hours*3600`; `date_trunc('hour')`
//! → `(ts/3600)*3600`; `host(ip_addr)` → the stored TEXT; INET column → TEXT.

use super::{raw_uuid, ts};
use crate::audit::{
    AuditEntry, AuditFilter, ExportFilter, ExportRow, HourCount, IpCount, NewEntry,
    SecurityInsights, VerifyReport,
};
use crate::DbResult;
use rampart_core::{ApiKeyId, UserId};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

fn opt_uuid(r: &sqlx::sqlite::SqliteRow, col: &str) -> Option<Uuid> {
    r.get::<Option<String>, _>(col).map(|s| raw_uuid(&s))
}

fn entry_from(r: &sqlx::sqlite::SqliteRow) -> AuditEntry {
    AuditEntry {
        id: r.get::<i64, _>("id"),
        actor_user_id: opt_uuid(r, "actor_user_id").map(UserId::from_uuid),
        actor_api_key_id: opt_uuid(r, "actor_api_key_id").map(ApiKeyId::from_uuid),
        action: r.get("action"),
        resource_kind: r.get("resource_kind"),
        resource_id: opt_uuid(r, "resource_id"),
        payload: r
            .get::<Option<String>, _>("payload")
            .and_then(|s| serde_json::from_str(&s).ok()),
        ip_addr: r.get("ip_addr"),
        user_agent: r.get("user_agent"),
        ts: ts(r.get::<i64, _>("ts")),
    }
}

const COLS: &str = "id, actor_user_id, actor_api_key_id, action, resource_kind, resource_id, \
     payload, ip_addr, user_agent, ts";

/// Append one audit row + extend the hash chain. One tx (SQLite serializes
/// writes, so no advisory lock needed). Best-effort by contract.
pub async fn insert(pool: &SqlitePool, entry: NewEntry<'_>) -> DbResult<()> {
    let ip_str = entry.ip_addr.map(|i| i.to_string());
    let mut tx = pool.begin().await?;
    let prev_hash: Option<String> = sqlx::query_scalar(
        "SELECT hash FROM audit_log WHERE hash IS NOT NULL ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?
    .flatten();

    let row = sqlx::query(
        "INSERT INTO audit_log
            (actor_user_id, actor_api_key_id, action, resource_kind, resource_id, payload,
             ip_addr, user_agent, prev_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id, ts",
    )
    .bind(entry.actor_user_id.map(|u| u.0.to_string()))
    .bind(entry.actor_api_key_id.map(|k| k.0.to_string()))
    .bind(entry.action)
    .bind(entry.resource_kind)
    .bind(entry.resource_id.map(|r| r.to_string()))
    .bind(entry.payload.as_ref().map(|p| p.to_string()))
    .bind(&ip_str)
    .bind(entry.user_agent)
    .bind(&prev_hash)
    .fetch_one(&mut *tx)
    .await?;
    let id = row.get::<i64, _>("id");
    let ts_odt = ts(row.get::<i64, _>("ts"));

    let hash = crate::audit::chain_hash(
        prev_hash.as_deref(),
        id,
        ts_odt,
        entry.action,
        entry.resource_kind,
        entry.resource_id,
        entry.actor_user_id.map(|u| u.0),
        entry.actor_api_key_id.map(|k| k.0),
        entry.payload.as_ref(),
        ip_str.as_deref(),
        entry.user_agent,
    );
    sqlx::query("UPDATE audit_log SET hash = ? WHERE id = ?")
        .bind(hash)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Record the prune watermark (newest deleted hashed row) so `verify_chain`
/// seeds from it instead of genesis. Stored in sqlite settings, like PG.
pub async fn set_chain_watermark(pool: &SqlitePool, id: i64, hash: &str) -> DbResult<()> {
    let v = serde_json::json!({ "id": id, "hash": hash });
    super::settings::put_setting(pool, crate::audit::CHAIN_WATERMARK_KEY, &v).await
}

async fn chain_watermark_hash(pool: &SqlitePool) -> DbResult<Option<String>> {
    let raw = super::settings::get_setting(pool, crate::audit::CHAIN_WATERMARK_KEY).await?;
    Ok(raw.and_then(|v| v.get("hash").and_then(|h| h.as_str()).map(str::to_owned)))
}

/// Re-walk + recompute the hash chain (oldest first). Same chain_hash inputs as
/// `insert`, so a clean chain verifies; any edit/delete/reorder breaks it.
pub async fn verify_chain(pool: &SqlitePool) -> DbResult<VerifyReport> {
    let rows = sqlx::query(
        "SELECT id, ts, action, resource_kind, resource_id, payload, ip_addr, user_agent,
                actor_user_id, actor_api_key_id, prev_hash, hash
         FROM audit_log WHERE hash IS NOT NULL ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await?;
    let mut prev: Option<String> = chain_watermark_hash(pool).await?;
    let mut checked = 0i64;
    for r in &rows {
        let id = r.get::<i64, _>("id");
        let expect = crate::audit::chain_hash(
            prev.as_deref(),
            id,
            ts(r.get::<i64, _>("ts")),
            &r.get::<String, _>("action"),
            &r.get::<String, _>("resource_kind"),
            opt_uuid(r, "resource_id"),
            opt_uuid(r, "actor_user_id"),
            opt_uuid(r, "actor_api_key_id"),
            r.get::<Option<String>, _>("payload")
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .as_ref(),
            r.get::<Option<String>, _>("ip_addr").as_deref(),
            r.get::<Option<String>, _>("user_agent").as_deref(),
        );
        let row_prev = r.get::<Option<String>, _>("prev_hash");
        let row_hash = r.get::<Option<String>, _>("hash");
        if row_prev.as_deref() != prev.as_deref() || row_hash.as_deref() != Some(&expect) {
            return Ok(VerifyReport {
                ok: false,
                checked,
                first_bad_id: Some(id),
            });
        }
        prev = row_hash;
        checked += 1;
    }
    Ok(VerifyReport {
        ok: true,
        checked,
        first_bad_id: None,
    })
}

pub async fn security_insights(pool: &SqlitePool, hours: i32) -> DbResult<SecurityInsights> {
    let cutoff = time::OffsetDateTime::now_utc().unix_timestamp() - hours as i64 * 3600;

    let (failed, ok, totp): (i64, i64, i64) = sqlx::query_as(
        "SELECT
            COALESCE(SUM(CASE WHEN action = 'auth.login_failed' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN action = 'auth.login'        THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN action = 'auth.totp_failed'  THEN 1 ELSE 0 END), 0)
         FROM audit_log WHERE ts >= ?",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let top_rows = sqlx::query(
        "SELECT ip_addr AS ip, COUNT(*) AS count FROM audit_log
         WHERE action = 'auth.login_failed' AND ip_addr IS NOT NULL AND ts >= ?
         GROUP BY ip_addr ORDER BY COUNT(*) DESC LIMIT 10",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;

    let hourly = sqlx::query(
        "SELECT (ts / 3600) * 3600 AS hour_bucket, COUNT(*) AS count FROM audit_log
         WHERE action = 'auth.login_failed' AND ts >= ?
         GROUP BY hour_bucket ORDER BY hour_bucket",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;

    Ok(SecurityInsights {
        failed_logins: failed,
        successful_logins: ok,
        totp_failures: totp,
        top_ips: top_rows
            .iter()
            .map(|r| IpCount {
                ip: r.get("ip"),
                count: r.get::<i64, _>("count"),
            })
            .collect(),
        by_hour: hourly
            .iter()
            .map(|r| HourCount {
                hour: ts(r.get::<i64, _>("hour_bucket")),
                count: r.get::<i64, _>("count"),
            })
            .collect(),
    })
}

pub async fn list(
    pool: &SqlitePool,
    limit: i64,
    filter: AuditFilter<'_>,
) -> DbResult<Vec<AuditEntry>> {
    let limit = limit.clamp(1, 500);
    let action_like = filter.action_prefix.map(|p| format!("{p}%"));
    let actor = filter.actor.map(|a| a.to_string());
    let from = filter.from.map(|t| t.unix_timestamp());
    let to = filter.to.map(|t| t.unix_timestamp());
    let sql = format!(
        "SELECT {COLS} FROM audit_log
         WHERE (? IS NULL OR id < ?)
           AND (? IS NULL OR resource_kind = ?)
           AND (? IS NULL OR action LIKE ?)
           AND (? IS NULL OR actor_user_id = ?)
           AND (? IS NULL OR ts >= ?)
           AND (? IS NULL OR ts <= ?)
         ORDER BY id DESC LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(filter.before_id)
        .bind(filter.before_id)
        .bind(filter.kind)
        .bind(filter.kind)
        .bind(&action_like)
        .bind(&action_like)
        .bind(&actor)
        .bind(&actor)
        .bind(from)
        .bind(from)
        .bind(to)
        .bind(to)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(entry_from).collect())
}

pub async fn fetch_since(
    pool: &SqlitePool,
    after_id: i64,
    limit: i64,
) -> DbResult<Vec<AuditEntry>> {
    let limit = limit.clamp(1, 1000);
    let sql = format!("SELECT {COLS} FROM audit_log WHERE id > ? ORDER BY id ASC LIMIT ?");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(after_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(entry_from).collect())
}

pub async fn export_batch(
    pool: &SqlitePool,
    before_id: Option<i64>,
    batch: i64,
    filter: ExportFilter,
) -> DbResult<Vec<ExportRow>> {
    let batch = batch.clamp(1, 5_000);
    let from = filter.from.map(|t| t.unix_timestamp());
    let to = filter.to.map(|t| t.unix_timestamp());
    let rows = sqlx::query(
        "SELECT a.id, a.ts, a.action, a.resource_kind, a.resource_id, a.ip_addr, a.user_agent,
                u.email AS actor_email, a.actor_api_key_id AS actor_api_key_id
         FROM audit_log a LEFT JOIN users u ON u.id = a.actor_user_id
         WHERE (? IS NULL OR a.id < ?)
           AND (? IS NULL OR a.ts >= ?)
           AND (? IS NULL OR a.ts <= ?)
         ORDER BY a.id DESC LIMIT ?",
    )
    .bind(before_id)
    .bind(before_id)
    .bind(from)
    .bind(from)
    .bind(to)
    .bind(to)
    .bind(batch)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let actor = r
                .get::<Option<String>, _>("actor_email")
                .or_else(|| r.get::<Option<String>, _>("actor_api_key_id"))
                .unwrap_or_default();
            ExportRow {
                id: r.get::<i64, _>("id"),
                ts: ts(r.get::<i64, _>("ts")),
                actor,
                action: r.get("action"),
                resource_kind: r.get("resource_kind"),
                resource_id: opt_uuid(r, "resource_id"),
                ip_addr: r.get("ip_addr"),
                user_agent: r.get("user_agent"),
            }
        })
        .collect())
}

/// Flat age-based retention prune: drop audit_log rows older than `days`.
/// Returns rows deleted.
pub async fn prune(pool: &SqlitePool, days: i32) -> DbResult<u64> {
    let cutoff = time::OffsetDateTime::now_utc().unix_timestamp() - days.max(0) as i64 * 86400;
    let res = sqlx::query("DELETE FROM audit_log WHERE ts < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn entry<'a>(action: &'a str, ip: Option<IpAddr>) -> NewEntry<'a> {
        NewEntry {
            actor_user_id: None,
            actor_api_key_id: None,
            action,
            resource_kind: "session",
            resource_id: None,
            payload: None,
            ip_addr: ip,
            user_agent: None,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn chain_verifies_and_detects_tamper(pool: SqlitePool) {
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        insert(&pool, entry("auth.login_failed", Some(ip)))
            .await
            .unwrap();
        insert(&pool, entry("auth.login_failed", Some(ip)))
            .await
            .unwrap();
        insert(&pool, entry("auth.login", None)).await.unwrap();
        insert(&pool, entry("auth.totp_failed", None))
            .await
            .unwrap();

        // clean chain verifies.
        let v = verify_chain(&pool).await.unwrap();
        assert!(v.ok, "clean chain ok");
        assert_eq!(v.checked, 4);

        // security insights aggregate (mirrors PG test).
        let i = security_insights(&pool, 24).await.unwrap();
        assert_eq!(i.failed_logins, 2);
        assert_eq!(i.successful_logins, 1);
        assert_eq!(i.totp_failures, 1);
        assert_eq!(i.top_ips.len(), 1);
        assert_eq!(i.top_ips[0].ip, "1.2.3.4");
        assert_eq!(i.top_ips[0].count, 2);
        assert!(!i.by_hour.is_empty());

        // list + fetch_since.
        assert_eq!(
            list(
                &pool,
                100,
                AuditFilter {
                    before_id: None,
                    kind: None,
                    action_prefix: Some("auth."),
                    actor: None,
                    from: None,
                    to: None,
                }
            )
            .await
            .unwrap()
            .len(),
            4
        );
        assert_eq!(fetch_since(&pool, 0, 100).await.unwrap().len(), 4);

        // tamper row 2's action → chain breaks at it.
        sqlx::query("UPDATE audit_log SET action = 'auth.login' WHERE id = 2")
            .execute(&pool)
            .await
            .unwrap();
        let bad = verify_chain(&pool).await.unwrap();
        assert!(!bad.ok, "tamper detected");
        assert_eq!(bad.first_bad_id, Some(2));
    }
}
