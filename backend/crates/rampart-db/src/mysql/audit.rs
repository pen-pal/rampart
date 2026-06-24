//! MySQL `audit` domain — append-only, tamper-evident audit log. Mirrors the
//! PG/SQLite free-fn surface: insert / set_chain_watermark / verify_chain /
//! security_insights / list / fetch_since / export_batch.
//!
//! The hash chain reuses `crate::audit::chain_hash` VERBATIM so insert + verify
//! feed byte-identical inputs. MySQL deltas: no `RETURNING` → `LAST_INSERT_ID()`
//! plus an explicitly-bound `ts` (so the hashed ts == the stored ts with no
//! re-select); `SUM(CASE…)` → `CAST(… AS SIGNED)`; `date_trunc('hour')` →
//! `(ts DIV 3600)*3600`; INET → plain TEXT.
//!
//! ## Chain serialization (two locks, on purpose)
//!
//! `pg_advisory_xact_lock` has no InnoDB equivalent, so the insert tx takes
//! TWO tx-scoped locks (both auto-released on commit, no `GET_LOCK` leak):
//!   1. `FOR UPDATE` on the single-row `audit_chain_lock` — orders WRITERS,
//!      and (being always present) covers the genesis/empty-table case.
//!   2. `FOR UPDATE` on the tip read (`ORDER BY id DESC LIMIT 1`) — a LOCKING
//!      read so it observes the LATEST COMMITTED hash regardless of the tx's
//!      consistent-read snapshot. This is load-bearing: under REPEATABLE READ a
//!      bare SELECT would read the snapshot established by the first non-locking
//!      read, which can predate the predecessor's commit → two serialized
//!      inserts read the same stale tip → forked chain. The locking read
//!      decouples chain linearity from read ordering. Do NOT downgrade it.
//!
//! ## Tamper model vs PG (honest scope)
//!
//! The prune watermark is stored in PLAINTEXT here (and on SQLite): `mysql::
//! settings` omits the AES-GCM secrets envelope that PG seals `audit_chain_
//! watermark` with. So the PG "sealed watermark can't be forged to hide a head
//! deletion" guarantee does NOT hold on this backend — a DB-write attacker can
//! forge the watermark + delete a prefix and verify still passes. Backward
//! linkage + middle/surviving-row tamper detection are intact. Accepted
//! multi-DB-port debt (matches SQLite); seal when `mysql::settings` grows the
//! envelope.
//!
//! ponytail: VARCHAR caps (action 128 / resource_kind 64 / ip_addr 64) exceed
//! every reachable value today, but a non-strict `sql_mode` would silently
//! truncate an over-length write → stored ≠ hashed → chain breaks. The eventual
//! `MysqlStore::connect` MUST set `sql_mode=STRICT_TRANS_TABLES` (after_connect
//! hook) so over-length writes fail loud. Not set here: pool is built by the
//! capstone, not this module.

use super::{raw_uuid, ts};
use crate::audit::{
    AuditEntry, AuditFilter, ExportFilter, ExportRow, HourCount, IpCount, NewEntry,
    SecurityInsights, VerifyReport,
};
use crate::DbResult;
use rampart_core::{ApiKeyId, UserId};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

fn opt_uuid(r: &sqlx::mysql::MySqlRow, col: &str) -> Option<Uuid> {
    r.get::<Option<String>, _>(col).map(|s| raw_uuid(&s))
}

fn entry_from(r: &sqlx::mysql::MySqlRow) -> AuditEntry {
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

/// Append one audit row + extend the hash chain. Two tx-scoped `FOR UPDATE`
/// locks (writer-order + locking tip read) keep the chain strictly linear —
/// see the module-level "Chain serialization" note.
pub async fn insert(pool: &MySqlPool, entry: NewEntry<'_>) -> DbResult<()> {
    let ip_str = entry.ip_addr.map(|i| i.to_string());
    let ts_unix = OffsetDateTime::now_utc().unix_timestamp();
    let ts_odt = ts(ts_unix);
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT k FROM audit_chain_lock WHERE k = 1 FOR UPDATE")
        .execute(&mut *tx)
        .await?;
    // Locking read: observe the latest COMMITTED tip, not the tx snapshot.
    let prev_hash: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT hash FROM audit_log WHERE hash IS NOT NULL ORDER BY id DESC LIMIT 1 FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?
    .flatten();

    let res = sqlx::query(
        "INSERT INTO audit_log
            (actor_user_id, actor_api_key_id, action, resource_kind, resource_id, payload,
             ip_addr, user_agent, ts, prev_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry.actor_user_id.map(|u| u.0.to_string()))
    .bind(entry.actor_api_key_id.map(|k| k.0.to_string()))
    .bind(entry.action)
    .bind(entry.resource_kind)
    .bind(entry.resource_id.map(|r| r.to_string()))
    .bind(entry.payload.as_ref().map(|p| p.to_string()))
    .bind(&ip_str)
    .bind(entry.user_agent)
    .bind(ts_unix)
    .bind(&prev_hash)
    .execute(&mut *tx)
    .await?;
    let id = res.last_insert_id() as i64;

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

pub async fn set_chain_watermark(pool: &MySqlPool, id: i64, hash: &str) -> DbResult<()> {
    let v = serde_json::json!({ "id": id, "hash": hash });
    super::settings::put_setting(pool, crate::audit::CHAIN_WATERMARK_KEY, &v).await
}

async fn chain_watermark_hash(pool: &MySqlPool) -> DbResult<Option<String>> {
    let raw = super::settings::get_setting(pool, crate::audit::CHAIN_WATERMARK_KEY).await?;
    Ok(raw.and_then(|v| v.get("hash").and_then(|h| h.as_str()).map(str::to_owned)))
}

/// Re-walk + recompute the chain (oldest first); same `chain_hash` inputs as
/// `insert`, so a clean chain verifies and any edit/delete/reorder breaks it.
pub async fn verify_chain(pool: &MySqlPool) -> DbResult<VerifyReport> {
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

pub async fn security_insights(pool: &MySqlPool, hours: i32) -> DbResult<SecurityInsights> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - hours as i64 * 3600;

    let (failed, ok, totp): (i64, i64, i64) = sqlx::query_as(
        "SELECT
            CAST(COALESCE(SUM(CASE WHEN action = 'auth.login_failed' THEN 1 ELSE 0 END), 0) AS SIGNED),
            CAST(COALESCE(SUM(CASE WHEN action = 'auth.login'        THEN 1 ELSE 0 END), 0) AS SIGNED),
            CAST(COALESCE(SUM(CASE WHEN action = 'auth.totp_failed'  THEN 1 ELSE 0 END), 0) AS SIGNED)
         FROM audit_log WHERE ts >= ?",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await?;

    let top_rows = sqlx::query(
        "SELECT ip_addr AS ip, CAST(COUNT(*) AS SIGNED) AS cnt FROM audit_log
         WHERE action = 'auth.login_failed' AND ip_addr IS NOT NULL AND ts >= ?
         GROUP BY ip_addr ORDER BY COUNT(*) DESC LIMIT 10",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;

    let hourly = sqlx::query(
        "SELECT (ts DIV 3600) * 3600 AS hour_bucket, CAST(COUNT(*) AS SIGNED) AS cnt FROM audit_log
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
                count: r.get::<i64, _>("cnt"),
            })
            .collect(),
        by_hour: hourly
            .iter()
            .map(|r| HourCount {
                hour: ts(r.get::<i64, _>("hour_bucket")),
                count: r.get::<i64, _>("cnt"),
            })
            .collect(),
    })
}

pub async fn list(
    pool: &MySqlPool,
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

pub async fn fetch_since(pool: &MySqlPool, after_id: i64, limit: i64) -> DbResult<Vec<AuditEntry>> {
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
    pool: &MySqlPool,
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
pub async fn prune(pool: &MySqlPool, days: i32) -> DbResult<u64> {
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

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn chain_verifies_and_detects_tamper(pool: MySqlPool) {
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

        let v = verify_chain(&pool).await.unwrap();
        assert!(v.ok, "clean chain ok");
        assert_eq!(v.checked, 4);

        let i = security_insights(&pool, 24).await.unwrap();
        assert_eq!(i.failed_logins, 2);
        assert_eq!(i.successful_logins, 1);
        assert_eq!(i.totp_failures, 1);
        assert_eq!(i.top_ips.len(), 1);
        assert_eq!(i.top_ips[0].ip, "1.2.3.4");
        assert_eq!(i.top_ips[0].count, 2);
        assert!(!i.by_hour.is_empty());

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

    async fn surviving(pool: &MySqlPool) -> Vec<(i64, String)> {
        sqlx::query_as("SELECT id, hash FROM audit_log WHERE hash IS NOT NULL ORDER BY id ASC")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    /// Head-truncation (oldest rows pruned) with a seeded watermark must still
    /// verify — exercises set_chain_watermark round-trip + the seed at the
    /// start of verify_chain (mirrors PG head_truncation_with_watermark_verifies).
    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn head_truncation_with_watermark_verifies(pool: MySqlPool) {
        for a in ["w.one", "w.two", "w.three", "w.four"] {
            insert(&pool, entry(a, None)).await.unwrap();
        }
        let rows = surviving(&pool).await;
        let cut = &rows[1]; // 2nd row is the newest deleted → the watermark.
        set_chain_watermark(&pool, cut.0, &cut.1).await.unwrap();
        sqlx::query("DELETE FROM audit_log WHERE id <= ?")
            .bind(cut.0)
            .execute(&pool)
            .await
            .unwrap();

        let r = verify_chain(&pool).await.unwrap();
        assert!(r.ok, "head-truncation with a watermark must verify");
        assert_eq!(r.checked, 2, "two surviving rows checked");
        assert!(r.first_bad_id.is_none());
    }

    /// The watermark must NOT mask deletion of a SURVIVING row (mirrors PG
    /// deletion_after_watermark_still_detected).
    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn deletion_after_watermark_still_detected(pool: MySqlPool) {
        for a in ["x.one", "x.two", "x.three", "x.four"] {
            insert(&pool, entry(a, None)).await.unwrap();
        }
        let rows = surviving(&pool).await;
        set_chain_watermark(&pool, rows[0].0, &rows[0].1)
            .await
            .unwrap();
        // Delete row[0] (as if pruned) + row[2] (a surviving middle row) →
        // row[3]'s prev_hash linkage breaks.
        sqlx::query("DELETE FROM audit_log WHERE id IN (?, ?)")
            .bind(rows[0].0)
            .bind(rows[2].0)
            .execute(&pool)
            .await
            .unwrap();

        let r = verify_chain(&pool).await.unwrap();
        assert!(!r.ok, "deleting a surviving row must still break the chain");
        assert_eq!(r.first_bad_id, Some(rows[3].0));
    }

    /// Plain middle-row deletion with no watermark breaks linkage (mirrors PG
    /// deletion_is_detected).
    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn deletion_is_detected(pool: MySqlPool) {
        for a in ["d.one", "d.two", "d.three"] {
            insert(&pool, entry(a, None)).await.unwrap();
        }
        let rows = surviving(&pool).await;
        sqlx::query("DELETE FROM audit_log WHERE id = ?")
            .bind(rows[1].0)
            .execute(&pool)
            .await
            .unwrap();

        let r = verify_chain(&pool).await.unwrap();
        assert!(!r.ok, "a deleted row must break the chain");
        assert_eq!(r.first_bad_id, Some(rows[2].0));
    }
}
