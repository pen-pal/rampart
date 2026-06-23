//! MySQL `monitors` domain — the core monitoring entity. Mirrors the PG/SQLite
//! surface. MySQL deltas: no `RETURNING` (insert-then-get); `unixepoch()` →
//! `UNIX_TIMESTAMP()`; `ON CONFLICT DO NOTHING` → `ON DUPLICATE KEY`; the wide
//! row is read by-name (MySQL is strict about integer types — all Monitor int
//! fields are i32 → INT columns; bool TINYINT decoded as i64; ts BIGINT).
//! Tag hydration goes through `super::tags`.

use super::{kind_from, kind_str, mid, mstatus_from, mstatus_str, raw_uuid, ts};
use crate::monitors::SloState;
use crate::monitors::{BulkEditOutcome, BulkEditPatch, MonitorPrior};
use crate::{DbError, DbResult};
use rampart_core::ids::{
    AgentId, EscalationPolicyId, MonitorGroupId, MonitorId, OrgId, ProxyId, TagId,
};
use rampart_core::monitor::{Monitor, MonitorStatus, NewMonitor, UpdateMonitor};
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

fn monitor_from(r: &sqlx::mysql::MySqlRow) -> Monitor {
    let opt_id = |col: &str| r.get::<Option<String>, _>(col);
    Monitor {
        id: mid(&r.get::<String, _>("id")),
        name: r.get("name"),
        kind: kind_from(&r.get::<String, _>("kind")),
        url: r.get("url"),
        hostname: r.get("hostname"),
        port: r.get("port"),
        config: serde_json::from_str(&r.get::<String, _>("config")).unwrap_or_default(),
        interval_seconds: r.get("interval_seconds"),
        retry_interval_sec: r.get("retry_interval_sec"),
        max_retries: r.get("max_retries"),
        timeout_seconds: r.get("timeout_seconds"),
        resend_interval_sec: r.get("resend_interval_sec"),
        upside_down: r.get::<i64, _>("upside_down") != 0,
        http_method: r.get("http_method"),
        http_body: r.get("http_body"),
        http_headers: r
            .get::<Option<String>, _>("http_headers")
            .and_then(|s| serde_json::from_str(&s).ok()),
        accepted_statuses: serde_json::from_str(&r.get::<String, _>("accepted_statuses"))
            .unwrap_or_default(),
        follow_redirect: r.get::<i64, _>("follow_redirect") != 0,
        ignore_tls: r.get::<i64, _>("ignore_tls") != 0,
        proxy_id: opt_id("proxy_id").map(|s| ProxyId::from_uuid(raw_uuid(&s))),
        push_token: r.get("push_token"),
        last_push_at: r.get::<Option<i64>, _>("last_push_at").map(ts),
        last_run_started_at: r.get::<Option<i64>, _>("last_run_started_at").map(ts),
        active: r.get::<i64, _>("active") != 0,
        current_status: mstatus_from(&r.get::<String, _>("current_status")),
        created_at: ts(r.get::<i64, _>("created_at")),
        updated_at: ts(r.get::<i64, _>("updated_at")),
        tags: Vec::new(),
        cert_days_left: r.get("cert_days_left"),
        cert_subject: r.get("cert_subject"),
        cert_checked_at: r.get::<Option<i64>, _>("cert_checked_at").map(ts),
        check_cert: r.get::<i64, _>("check_cert") != 0,
        cert_expiry_days: r.get("cert_expiry_days"),
        group_id: opt_id("group_id").map(|s| MonitorGroupId::from_uuid(raw_uuid(&s))),
        slo_target_pct: r.get("slo_target_pct"),
        slo_window_days: r.get("slo_window_days"),
        agent_id: opt_id("agent_id").map(|s| AgentId::from_uuid(raw_uuid(&s))),
        escalation_policy_id: opt_id("escalation_policy_id")
            .map(|s| EscalationPolicyId::from_uuid(raw_uuid(&s))),
    }
}

pub async fn create(pool: &MySqlPool, input: NewMonitor, org_id: OrgId) -> DbResult<Monitor> {
    let id = MonitorId::new();
    sqlx::query(
        "INSERT INTO monitors (
            id, name, kind, url, hostname, port, config, interval_seconds,
            retry_interval_sec, max_retries, timeout_seconds, resend_interval_sec,
            upside_down, http_method, http_body, http_headers, accepted_statuses,
            follow_redirect, ignore_tls, proxy_id, group_id, check_cert,
            cert_expiry_days, slo_target_pct, slo_window_days, agent_id,
            escalation_policy_id, org_id)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(id.0.to_string())
    .bind(&input.name)
    .bind(kind_str(input.kind))
    .bind(&input.url)
    .bind(&input.hostname)
    .bind(input.port)
    .bind(input.config.to_string())
    .bind(input.interval_seconds)
    .bind(input.retry_interval_sec)
    .bind(input.max_retries)
    .bind(input.timeout_seconds)
    .bind(input.resend_interval_sec)
    .bind(input.upside_down as i64)
    .bind(&input.http_method)
    .bind(&input.http_body)
    .bind(input.http_headers.as_ref().map(|v| v.to_string()))
    .bind(serde_json::to_string(&input.accepted_statuses).unwrap_or_else(|_| "[]".into()))
    .bind(input.follow_redirect as i64)
    .bind(input.ignore_tls as i64)
    .bind(input.proxy_id.map(|p| p.0.to_string()))
    .bind(input.group_id.map(|g| g.0.to_string()))
    .bind(input.check_cert as i64)
    .bind(input.cert_expiry_days)
    .bind(input.slo_target_pct)
    .bind(input.slo_window_days)
    .bind(input.agent_id.map(|a| a.0.to_string()))
    .bind(input.escalation_policy_id.map(|e| e.0.to_string()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn get(pool: &MySqlPool, id: MonitorId, org_id: OrgId) -> DbResult<Monitor> {
    let row = sqlx::query("SELECT * FROM monitors WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    let mut m = row.map(|r| monitor_from(&r)).ok_or(DbError::NotFound)?;
    m.tags = super::tags::list_for_monitor(pool, m.id).await?;
    Ok(m)
}

pub async fn get_unscoped(pool: &MySqlPool, id: MonitorId) -> DbResult<Monitor> {
    let row = sqlx::query("SELECT * FROM monitors WHERE id = ?")
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?;
    let mut m = row.map(|r| monitor_from(&r)).ok_or(DbError::NotFound)?;
    m.tags = super::tags::list_for_monitor(pool, m.id).await?;
    Ok(m)
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query("SELECT * FROM monitors WHERE org_id = ? ORDER BY created_at ASC")
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(monitor_from).collect()).await
}

pub async fn list_all(pool: &MySqlPool) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query("SELECT * FROM monitors ORDER BY created_at ASC")
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(monitor_from).collect()).await
}

async fn hydrate(pool: &MySqlPool, mut monitors: Vec<Monitor>) -> DbResult<Vec<Monitor>> {
    if monitors.is_empty() {
        return Ok(monitors);
    }
    let ids: Vec<MonitorId> = monitors.iter().map(|m| m.id).collect();
    let mut by = super::tags::hydrate_for_monitors(pool, &ids).await?;
    for m in &mut monitors {
        if let Some(t) = by.remove(&m.id) {
            m.tags = t;
        }
    }
    Ok(monitors)
}

pub async fn delete(pool: &MySqlPool, id: MonitorId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM monitors WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_active(
    pool: &MySqlPool,
    id: MonitorId,
    active: bool,
    org_id: OrgId,
) -> DbResult<()> {
    let r = sqlx::query(
        "UPDATE monitors SET active = ?, updated_at = UNIX_TIMESTAMP() WHERE id = ? AND org_id = ?",
    )
    .bind(active as i64)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        // No-op (already in target state) is fine; only a missing row is NotFound.
        if get(pool, id, org_id).await.is_err() {
            return Err(DbError::NotFound);
        }
    }
    Ok(())
}

pub async fn set_status(pool: &MySqlPool, id: MonitorId, status: MonitorStatus) -> DbResult<()> {
    sqlx::query(
        "UPDATE monitors SET current_status = ?, updated_at = UNIX_TIMESTAMP() WHERE id = ?",
    )
    .bind(mstatus_str(status))
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Partial update — COALESCE simple Options + per-field clears for double-Option
/// fields. Existence is checked via a SELECT (MySQL UPDATE reports changed not
/// matched rows, so a no-op patch isn't a false NotFound).
pub async fn update(
    pool: &MySqlPool,
    id: MonitorId,
    patch: UpdateMonitor,
    org_id: OrgId,
) -> DbResult<Monitor> {
    let exists: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM monitors WHERE id = ? AND org_id = ?")
            .bind(id.0.to_string())
            .bind(org_id.0.to_string())
            .fetch_optional(pool)
            .await?;
    if exists.is_none() {
        return Err(DbError::NotFound);
    }
    sqlx::query(
        "UPDATE monitors SET
            name                = COALESCE(?, name),
            url                 = COALESCE(?, url),
            hostname            = COALESCE(?, hostname),
            port                = COALESCE(?, port),
            config              = COALESCE(?, config),
            interval_seconds    = COALESCE(?, interval_seconds),
            timeout_seconds     = COALESCE(?, timeout_seconds),
            max_retries         = COALESCE(?, max_retries),
            retry_interval_sec  = COALESCE(?, retry_interval_sec),
            resend_interval_sec = COALESCE(?, resend_interval_sec),
            upside_down         = COALESCE(?, upside_down),
            http_method         = COALESCE(?, http_method),
            http_body           = COALESCE(?, http_body),
            http_headers        = COALESCE(?, http_headers),
            accepted_statuses   = COALESCE(?, accepted_statuses),
            follow_redirect     = COALESCE(?, follow_redirect),
            ignore_tls          = COALESCE(?, ignore_tls),
            proxy_id            = COALESCE(?, proxy_id),
            check_cert          = COALESCE(?, check_cert),
            cert_expiry_days    = COALESCE(?, cert_expiry_days),
            updated_at          = UNIX_TIMESTAMP()
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.url)
    .bind(patch.hostname)
    .bind(patch.port)
    .bind(patch.config.as_ref().map(|v| v.to_string()))
    .bind(patch.interval_seconds)
    .bind(patch.timeout_seconds)
    .bind(patch.max_retries)
    .bind(patch.retry_interval_sec)
    .bind(patch.resend_interval_sec)
    .bind(patch.upside_down.map(|b| b as i64))
    .bind(patch.http_method)
    .bind(patch.http_body)
    .bind(patch.http_headers.as_ref().map(|v| v.to_string()))
    .bind(
        patch
            .accepted_statuses
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".into())),
    )
    .bind(patch.follow_redirect.map(|b| b as i64))
    .bind(patch.ignore_tls.map(|b| b as i64))
    .bind(patch.proxy_id.map(|p| p.0.to_string()))
    .bind(patch.check_cert.map(|b| b as i64))
    .bind(patch.cert_expiry_days)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;

    if let Some(g) = patch.group_id {
        set_col_opt(pool, id, "group_id", g.map(|x| x.0.to_string())).await?;
    }
    if let Some(t) = patch.slo_target_pct {
        sqlx::query("UPDATE monitors SET slo_target_pct = ? WHERE id = ?")
            .bind(t)
            .bind(id.0.to_string())
            .execute(pool)
            .await?;
    }
    if let Some(w) = patch.slo_window_days {
        sqlx::query("UPDATE monitors SET slo_window_days = ? WHERE id = ?")
            .bind(w)
            .bind(id.0.to_string())
            .execute(pool)
            .await?;
    }
    if let Some(a) = patch.agent_id {
        set_col_opt(pool, id, "agent_id", a.map(|x| x.0.to_string())).await?;
    }
    if let Some(e) = patch.escalation_policy_id {
        set_col_opt(pool, id, "escalation_policy_id", e.map(|x| x.0.to_string())).await?;
    }

    get(pool, id, org_id).await
}

/// Set one FK-id CHAR(36) column to a value or NULL. `col` is a fixed literal.
async fn set_col_opt(
    pool: &MySqlPool,
    id: MonitorId,
    col: &'static str,
    val: Option<String>,
) -> DbResult<()> {
    let sql = match col {
        "group_id" => "UPDATE monitors SET group_id = ? WHERE id = ?",
        "agent_id" => "UPDATE monitors SET agent_id = ? WHERE id = ?",
        "escalation_policy_id" => "UPDATE monitors SET escalation_policy_id = ? WHERE id = ?",
        _ => unreachable!("set_col_opt: unknown column {col}"),
    };
    sqlx::query(sql)
        .bind(val)
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_group(
    pool: &MySqlPool,
    id: MonitorId,
    group: Option<MonitorGroupId>,
    org_id: OrgId,
) -> DbResult<()> {
    let r = sqlx::query("UPDATE monitors SET group_id = ? WHERE id = ? AND org_id = ?")
        .bind(group.map(|g| g.0.to_string()))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 && get(pool, id, org_id).await.is_err() {
        return Err(DbError::NotFound);
    }
    Ok(())
}

async fn load_prior(
    pool: &MySqlPool,
    id: MonitorId,
    with_tags: bool,
    org_id: OrgId,
) -> DbResult<Option<MonitorPrior>> {
    let row = sqlx::query(
        "SELECT name, interval_seconds, timeout_seconds, active, group_id
         FROM monitors WHERE id = ? AND org_id = ?",
    )
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let tags = if with_tags {
        prior_tags(pool, id).await?
    } else {
        Vec::new()
    };
    Ok(Some(MonitorPrior {
        id,
        name: row.get("name"),
        interval_seconds: row.get("interval_seconds"),
        timeout_seconds: row.get("timeout_seconds"),
        active: row.get::<i64, _>("active") != 0,
        group_id: row
            .get::<Option<String>, _>("group_id")
            .map(|s| MonitorGroupId::from_uuid(raw_uuid(&s))),
        tags,
    }))
}

async fn prior_tags(pool: &MySqlPool, id: MonitorId) -> DbResult<Vec<TagId>> {
    let rows = sqlx::query("SELECT tag_id FROM monitor_tags WHERE monitor_id = ? ORDER BY tag_id")
        .bind(id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| TagId::from_uuid(raw_uuid(&r.get::<String, _>("tag_id"))))
        .collect())
}

pub async fn bulk_edit_preview(
    pool: &MySqlPool,
    ids: &[MonitorId],
    want_tags: bool,
    org_id: OrgId,
) -> DbResult<(Vec<MonitorPrior>, usize)> {
    let mut priors = Vec::new();
    let mut skipped_unknown = 0usize;
    for id in ids {
        match load_prior(pool, *id, want_tags, org_id).await? {
            Some(p) => priors.push(p),
            None => skipped_unknown += 1,
        }
    }
    Ok((priors, skipped_unknown))
}

/// Apply `patch` to every id in ONE tx. Unknown/cross-org ids skipped (counted).
pub async fn bulk_edit(
    pool: &MySqlPool,
    ids: &[MonitorId],
    patch: &BulkEditPatch,
    org_id: OrgId,
) -> DbResult<BulkEditOutcome> {
    let touches_columns = patch.interval_seconds.is_some()
        || patch.timeout_seconds.is_some()
        || patch.active.is_some()
        || patch.group_id.is_some();
    let set_group = patch.group_id.is_some();
    let group_str: Option<String> = patch.group_id.flatten().map(|g| g.0.to_string());
    let capture_tags = patch.tags.is_some();

    let mut tx = pool.begin().await?;
    let mut updated = 0usize;
    let mut skipped_unknown = 0usize;
    let mut priors: Vec<MonitorPrior> = Vec::new();

    for id in ids {
        let prior_row = sqlx::query(
            "SELECT name, interval_seconds, timeout_seconds, active, group_id
             FROM monitors WHERE id = ? AND org_id = ?",
        )
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(prior_row) = prior_row else {
            skipped_unknown += 1;
            continue;
        };

        let prior_tag_set: Vec<TagId> = if capture_tags {
            let rows =
                sqlx::query("SELECT tag_id FROM monitor_tags WHERE monitor_id = ? ORDER BY tag_id")
                    .bind(id.0.to_string())
                    .fetch_all(&mut *tx)
                    .await?;
            rows.iter()
                .map(|r| TagId::from_uuid(raw_uuid(&r.get::<String, _>("tag_id"))))
                .collect()
        } else {
            Vec::new()
        };

        priors.push(MonitorPrior {
            id: *id,
            name: prior_row.get("name"),
            interval_seconds: prior_row.get("interval_seconds"),
            timeout_seconds: prior_row.get("timeout_seconds"),
            active: prior_row.get::<i64, _>("active") != 0,
            group_id: prior_row
                .get::<Option<String>, _>("group_id")
                .map(|s| MonitorGroupId::from_uuid(raw_uuid(&s))),
            tags: prior_tag_set,
        });

        if touches_columns {
            sqlx::query(
                "UPDATE monitors SET
                    interval_seconds = COALESCE(?, interval_seconds),
                    timeout_seconds  = COALESCE(?, timeout_seconds),
                    active           = COALESCE(?, active),
                    group_id         = CASE WHEN ? THEN ? ELSE group_id END,
                    updated_at       = UNIX_TIMESTAMP()
                 WHERE id = ?",
            )
            .bind(patch.interval_seconds)
            .bind(patch.timeout_seconds)
            .bind(patch.active.map(|b| b as i64))
            .bind(set_group as i64)
            .bind(group_str.clone())
            .bind(id.0.to_string())
            .execute(&mut *tx)
            .await?;
        }

        if let Some(tags) = &patch.tags {
            sqlx::query("DELETE FROM monitor_tags WHERE monitor_id = ?")
                .bind(id.0.to_string())
                .execute(&mut *tx)
                .await?;
            for tag in tags {
                sqlx::query(
                    "INSERT INTO monitor_tags (monitor_id, tag_id) VALUES (?, ?)
                     ON DUPLICATE KEY UPDATE monitor_id = monitor_id",
                )
                .bind(id.0.to_string())
                .bind(tag.0.to_string())
                .execute(&mut *tx)
                .await?;
            }
        }

        updated += 1;
    }

    tx.commit().await?;
    Ok(BulkEditOutcome {
        updated,
        skipped_unknown,
        priors,
    })
}

pub async fn set_active_by_tag(
    pool: &MySqlPool,
    tag: TagId,
    active: bool,
    org_id: OrgId,
) -> DbResult<u64> {
    let r = sqlx::query(
        "UPDATE monitors
            SET active = ?, updated_at = UNIX_TIMESTAMP()
          WHERE active <> ?
            AND org_id = ?
            AND id IN (SELECT monitor_id FROM monitor_tags WHERE tag_id = ?)",
    )
    .bind(active as i64)
    .bind(active as i64)
    .bind(org_id.0.to_string())
    .bind(tag.0.to_string())
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

// ── push-token + run lifecycle ───────────────────────────────────────────────

fn generate_push_token() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..24)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect()
}

pub async fn regenerate_push_token(
    pool: &MySqlPool,
    id: MonitorId,
    org_id: OrgId,
) -> DbResult<String> {
    let token = generate_push_token();
    let r = sqlx::query(
        "UPDATE monitors SET push_token = ?, updated_at = UNIX_TIMESTAMP()
         WHERE id = ? AND kind = 'push' AND org_id = ?",
    )
    .bind(&token)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(token)
}

pub async fn find_by_push_token(pool: &MySqlPool, token: &str) -> DbResult<Option<MonitorId>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM monitors WHERE push_token = ? AND active = 1")
            .bind(token)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id,)| mid(&id)))
}

pub async fn fetch_last_push_at(
    pool: &MySqlPool,
    id: MonitorId,
) -> DbResult<Option<OffsetDateTime>> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT last_push_at FROM monitors WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(t,)| t).map(ts))
}

pub async fn mark_run_started(pool: &MySqlPool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET last_run_started_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn close_run(pool: &MySqlPool, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
    let mut tx = pool.begin().await?;
    let prior: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT last_run_started_at FROM monitors WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&mut *tx)
            .await?;
    let prior = prior.ok_or(DbError::NotFound)?.0;
    sqlx::query(
        "UPDATE monitors SET last_push_at = UNIX_TIMESTAMP(), last_run_started_at = NULL WHERE id = ?",
    )
    .bind(id.0.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(prior.map(ts))
}

pub async fn push_state(
    pool: &MySqlPool,
    id: MonitorId,
) -> DbResult<(Option<OffsetDateTime>, Option<OffsetDateTime>)> {
    let row: Option<(Option<i64>, Option<i64>)> =
        sqlx::query_as("SELECT last_push_at, last_run_started_at FROM monitors WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(pool)
            .await?;
    let (push, run) = row.ok_or(DbError::NotFound)?;
    Ok((push.map(ts), run.map(ts)))
}

pub async fn bump_push_at(pool: &MySqlPool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET last_push_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

// ── cert + SLO + agent + batch reads ─────────────────────────────────────────

pub async fn set_cert_info(
    pool: &MySqlPool,
    id: MonitorId,
    days_left: i32,
    subject: &str,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE monitors SET cert_days_left = ?, cert_subject = ?, cert_checked_at = UNIX_TIMESTAMP()
         WHERE id = ?",
    )
    .bind(days_left)
    .bind(subject)
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn slo_state(pool: &MySqlPool, id: MonitorId) -> DbResult<Option<SloState>> {
    let row: Option<(Option<f64>, Option<i32>, Option<i64>)> = sqlx::query_as(
        "SELECT slo_target_pct, slo_window_days, slo_breached_at FROM monitors WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(target_pct, window_days, breached_at)| SloState {
        target_pct,
        window_days,
        breached_at: breached_at.map(ts),
    }))
}

pub async fn mark_slo_breached(pool: &MySqlPool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET slo_breached_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_slo_breached(pool: &MySqlPool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET slo_breached_at = NULL WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_agent(pool: &MySqlPool, agent: AgentId) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query(
        "SELECT * FROM monitors WHERE agent_id = ? AND active = 1 ORDER BY created_at ASC",
    )
    .bind(agent.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(monitor_from).collect())
}

/// Agent-assigned monitors gone stale (last heartbeat or updated_at older than
/// interval*2+30s). Paired with the agent name. Cross-tenant (in-proc scheduler).
pub async fn list_stale_agent_monitors(pool: &MySqlPool) -> DbResult<Vec<(Monitor, String)>> {
    let rows = sqlx::query(
        "SELECT m.*, a.name AS agent_name
         FROM monitors m JOIN agents a ON a.id = m.agent_id
         WHERE m.active = 1
           AND m.current_status <> 'paused'
           AND COALESCE(
                 (SELECT MAX(h.ts) FROM heartbeats h WHERE h.monitor_id = m.id),
                 m.updated_at
               ) < UNIX_TIMESTAMP() - (m.interval_seconds * 2 + 30)",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (monitor_from(r), r.get::<String, _>("agent_name")))
        .collect())
}

/// Public (id → name + status) for a set of monitor ids. Per-id lookups.
pub async fn public_fields_batch(
    pool: &MySqlPool,
    ids: &[Uuid],
) -> DbResult<HashMap<Uuid, (String, MonitorStatus)>> {
    let mut out = HashMap::with_capacity(ids.len());
    for id in ids {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT name, current_status FROM monitors WHERE id = ?")
                .bind(id.to_string())
                .fetch_optional(pool)
                .await?;
        if let Some((name, status)) = row {
            out.insert(*id, (name, mstatus_from(&status)));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::monitor::MonitorKind;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_http(name: &str) -> NewMonitor {
        NewMonitor {
            name: name.into(),
            kind: MonitorKind::Http,
            url: Some("https://example.com".into()),
            hostname: None,
            port: None,
            config: serde_json::json!({}),
            interval_seconds: 60,
            timeout_seconds: 10,
            max_retries: 2,
            retry_interval_sec: 30,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: Some(serde_json::json!({"X-Trace": "1"})),
            accepted_statuses: vec![200, 204],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            group_id: None,
            check_cert: true,
            cert_expiry_days: 14,
            slo_target_pct: Some(99.9),
            slo_window_days: Some(30),
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn create_read_update_delete(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let m = create(&pool, new_http("api"), org).await.unwrap();
        assert_eq!(m.kind, MonitorKind::Http);
        assert_eq!(m.accepted_statuses, vec![200, 204]);
        assert_eq!(m.http_headers.unwrap()["X-Trace"], "1");
        assert_eq!(m.slo_target_pct, Some(99.9));
        assert!(m.check_cert);
        assert_eq!(m.current_status, MonitorStatus::Pending);
        assert!(m.active);

        assert_eq!(get(&pool, m.id, org).await.unwrap().name, "api");
        assert_eq!(get_unscoped(&pool, m.id).await.unwrap().name, "api");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(list_all(&pool).await.unwrap().len(), 1);

        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, m.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert_eq!(list(&pool, other.id).await.unwrap().len(), 0);

        set_active(&pool, m.id, false, org).await.unwrap();
        assert!(!get(&pool, m.id, org).await.unwrap().active);
        set_status(&pool, m.id, MonitorStatus::Up).await.unwrap();
        assert_eq!(
            get(&pool, m.id, org).await.unwrap().current_status,
            MonitorStatus::Up
        );

        assert!(matches!(
            delete(&pool, m.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, m.id, org).await.unwrap();
        assert!(matches!(
            get_unscoped(&pool, m.id).await,
            Err(DbError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn update_group_slo_cert_and_public(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let m = create(&pool, new_http("u"), org).await.unwrap();

        let patch: UpdateMonitor = serde_json::from_value(
            serde_json::json!({ "name": "renamed", "interval_seconds": 120 }),
        )
        .unwrap();
        let upd = update(&pool, m.id, patch, org).await.unwrap();
        assert_eq!(upd.name, "renamed");
        assert_eq!(upd.interval_seconds, 120);
        assert_eq!(upd.url.as_deref(), Some("https://example.com"));

        let clear: UpdateMonitor =
            serde_json::from_value(serde_json::json!({ "slo_target_pct": null })).unwrap();
        update(&pool, m.id, clear, org).await.unwrap();
        assert!(get(&pool, m.id, org)
            .await
            .unwrap()
            .slo_target_pct
            .is_none());

        let g = MonitorGroupId::new();
        set_group(&pool, m.id, Some(g), org).await.unwrap();
        assert_eq!(get(&pool, m.id, org).await.unwrap().group_id, Some(g));
        let agent = AgentId::new();
        let setagent: UpdateMonitor =
            serde_json::from_value(serde_json::json!({ "agent_id": agent.0.to_string() })).unwrap();
        update(&pool, m.id, setagent, org).await.unwrap();
        assert_eq!(list_for_agent(&pool, agent).await.unwrap().len(), 1);

        let pf = public_fields_batch(&pool, &[m.id.0]).await.unwrap();
        assert_eq!(pf.get(&m.id.0).unwrap().0, "renamed");

        set_cert_info(&pool, m.id, 7, "CN=example").await.unwrap();
        assert_eq!(get(&pool, m.id, org).await.unwrap().cert_days_left, Some(7));
        mark_slo_breached(&pool, m.id).await.unwrap();
        assert!(slo_state(&pool, m.id)
            .await
            .unwrap()
            .unwrap()
            .breached_at
            .is_some());
        clear_slo_breached(&pool, m.id).await.unwrap();
        assert!(slo_state(&pool, m.id)
            .await
            .unwrap()
            .unwrap()
            .breached_at
            .is_none());
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn push_token_and_run_lifecycle(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let mut nm = new_http("push");
        nm.kind = MonitorKind::Push;
        let m = create(&pool, nm, org).await.unwrap();

        let token = regenerate_push_token(&pool, m.id, org).await.unwrap();
        assert_eq!(find_by_push_token(&pool, &token).await.unwrap(), Some(m.id));
        assert!(find_by_push_token(&pool, "nope").await.unwrap().is_none());

        mark_run_started(&pool, m.id).await.unwrap();
        let (_, run) = push_state(&pool, m.id).await.unwrap();
        assert!(run.is_some());
        let prior = close_run(&pool, m.id).await.unwrap();
        assert!(prior.is_some());
        let (push, run) = push_state(&pool, m.id).await.unwrap();
        assert!(run.is_none());
        assert!(push.is_some());

        bump_push_at(&pool, m.id).await.unwrap();
        assert!(fetch_last_push_at(&pool, m.id).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn stale_agent_watchdog(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        // agents domain not ported to MySQL yet — seed an agent row directly.
        let agent = AgentId::new();
        sqlx::query("INSERT INTO agents (id, name, token_hash, org_id) VALUES (?, 'probe', ?, ?)")
            .bind(agent.0.to_string())
            .bind(format!("tok-{}", agent.0))
            .bind(DEF)
            .execute(&pool)
            .await
            .unwrap();

        let stale = create(&pool, new_http("stale"), org).await.unwrap();
        let fresh = create(&pool, new_http("fresh"), org).await.unwrap();
        let local_old = create(&pool, new_http("local"), org).await.unwrap();
        for id in [stale.id, fresh.id] {
            let patch: UpdateMonitor =
                serde_json::from_value(serde_json::json!({ "agent_id": agent.0.to_string() }))
                    .unwrap();
            update(&pool, id, patch, org).await.unwrap();
        }
        for id in [stale.id, local_old.id] {
            sqlx::query("UPDATE monitors SET updated_at = UNIX_TIMESTAMP() - 100000 WHERE id = ?")
                .bind(id.0.to_string())
                .execute(&pool)
                .await
                .unwrap();
        }

        let out = list_stale_agent_monitors(&pool).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.id, stale.id);
        assert_eq!(out[0].1, "probe");

        // A recent heartbeat clears it (heartbeats domain not ported — raw insert).
        sqlx::query(
            "INSERT INTO heartbeats (monitor_id, ts, status) VALUES (?, UNIX_TIMESTAMP(), 'up')",
        )
        .bind(stale.id.0.to_string())
        .execute(&pool)
        .await
        .unwrap();
        assert!(list_stale_agent_monitors(&pool).await.unwrap().is_empty());
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn bulk_edit_tags_and_set_active_by_tag(pool: MySqlPool) {
        use crate::monitors::BulkEditPatch;
        use crate::mysql::tags;
        let org = super::super::oid(DEF);
        let a = create(&pool, new_http("a"), org).await.unwrap();
        let b = create(&pool, new_http("b"), org).await.unwrap();
        let unknown = MonitorId::new();
        let tag = tags::create(
            &pool,
            rampart_core::tag::NewTag {
                name: "prod".into(),
                color: "#f00".into(),
            },
            org,
        )
        .await
        .unwrap();

        let patch = BulkEditPatch {
            interval_seconds: Some(300),
            timeout_seconds: None,
            active: Some(false),
            group_id: None,
            tags: Some(vec![tag.id]),
        };

        let (priors, skipped) = bulk_edit_preview(&pool, &[a.id, b.id, unknown], true, org)
            .await
            .unwrap();
        assert_eq!(priors.len(), 2);
        assert_eq!(skipped, 1);
        assert_eq!(priors[0].interval_seconds, 60);

        let out = bulk_edit(&pool, &[a.id, b.id, unknown], &patch, org)
            .await
            .unwrap();
        assert_eq!(out.updated, 2);
        assert_eq!(out.skipped_unknown, 1);

        let a2 = get(&pool, a.id, org).await.unwrap();
        assert_eq!(a2.interval_seconds, 300);
        assert!(!a2.active);
        assert_eq!(a2.timeout_seconds, 10);
        assert_eq!(a2.tags.len(), 1); // tag hydration + replacement

        // set_active_by_tag re-activates only the tagged monitors.
        let flipped = set_active_by_tag(&pool, tag.id, true, org).await.unwrap();
        assert_eq!(flipped, 2);
        assert!(get(&pool, a.id, org).await.unwrap().active);
        assert_eq!(
            set_active_by_tag(&pool, tag.id, true, org).await.unwrap(),
            0
        );

        // tags::usage counts the 2 monitor attachments.
        let u = tags::usage(&pool, org).await.unwrap();
        assert_eq!(u.iter().find(|x| x.tag_id == tag.id).unwrap().monitors, 2);
    }
}
