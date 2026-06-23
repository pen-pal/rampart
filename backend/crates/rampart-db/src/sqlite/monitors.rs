//! SQLite `monitors` domain — the core monitoring entity. Mirrors the Postgres
//! `crate::monitors` surface against SQLite: CRUD (create / get / get_unscoped /
//! list / list_all / delete / set_active / set_status), `update` (partial,
//! COALESCE + per-field clears for the double-Option fields), `set_group`, the
//! push-token + run lifecycle (regenerate_push_token / find_by_push_token /
//! fetch_last_push_at / mark_run_started / close_run / push_state /
//! bump_push_at), `set_cert_info`, the SLO state trio (slo_state /
//! mark_slo_breached / clear_slo_breached), `list_for_agent`, and
//! `public_fields_batch`. The wide row is read via the `sqlx::Row` get-by-name
//! API rather than a 40-field tuple.
//!
//! STILL DEFERRED (need tables not yet on SQLite): `list_stale_agent_monitors`
//! (agents), `set_active_by_tag` + `bulk_edit`/`bulk_edit_preview` + tag
//! hydration on reads (tags/monitor_tags — `Monitor.tags` reads back empty).
//!
//! Dialect: enums → TEXT (serde round-trip), `int[] accepted_statuses` → JSON
//! TEXT, jsonb → TEXT, timestamps → INTEGER unix-seconds, bools → INTEGER 0/1.

use super::{kind_from, kind_str, mid, mstatus_from, mstatus_str, raw_uuid, ts};
use crate::monitors::SloState;
use crate::{DbError, DbResult};
use rampart_core::ids::{
    AgentId, EscalationPolicyId, MonitorGroupId, MonitorId, OrgId, ProxyId, TagId,
};
use rampart_core::monitor::{Monitor, MonitorStatus, NewMonitor, UpdateMonitor};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

fn monitor_from(r: &sqlx::sqlite::SqliteRow) -> Monitor {
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
        tags: Vec::new(), // hydration deferred to a later slice
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

pub async fn create(pool: &SqlitePool, input: NewMonitor, org_id: OrgId) -> DbResult<Monitor> {
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

pub async fn get(pool: &SqlitePool, id: MonitorId, org_id: OrgId) -> DbResult<Monitor> {
    let row = sqlx::query("SELECT * FROM monitors WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    let mut m = row.map(|r| monitor_from(&r)).ok_or(DbError::NotFound)?;
    m.tags = super::tags::list_for_monitor(pool, m.id).await?;
    Ok(m)
}

pub async fn get_unscoped(pool: &SqlitePool, id: MonitorId) -> DbResult<Monitor> {
    let row = sqlx::query("SELECT * FROM monitors WHERE id = ?")
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?;
    let mut m = row.map(|r| monitor_from(&r)).ok_or(DbError::NotFound)?;
    m.tags = super::tags::list_for_monitor(pool, m.id).await?;
    Ok(m)
}

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query("SELECT * FROM monitors WHERE org_id = ? ORDER BY created_at ASC")
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(monitor_from).collect()).await
}

pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query("SELECT * FROM monitors ORDER BY created_at ASC")
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(monitor_from).collect()).await
}

/// Batch-attach tags to a list of monitors in one round trip (PG `list` /
/// `list_all` do the same via `hydrate_for_monitors`).
async fn hydrate(pool: &SqlitePool, mut monitors: Vec<Monitor>) -> DbResult<Vec<Monitor>> {
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

pub async fn delete(pool: &SqlitePool, id: MonitorId, org_id: OrgId) -> DbResult<()> {
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
    pool: &SqlitePool,
    id: MonitorId,
    active: bool,
    org_id: OrgId,
) -> DbResult<()> {
    let r = sqlx::query(
        "UPDATE monitors SET active = ?, updated_at = unixepoch() WHERE id = ? AND org_id = ?",
    )
    .bind(active as i64)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_status(pool: &SqlitePool, id: MonitorId, status: MonitorStatus) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET current_status = ?, updated_at = unixepoch() WHERE id = ?")
        .bind(mstatus_str(status))
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// Partial update — COALESCE for simple Options (omit → unchanged), plus a
/// per-field UPDATE for the double-Option fields (`Some(None)` clears,
/// `Some(Some)` sets, `None` leaves) so callers can distinguish unset from
/// explicit-null. Mirrors the Postgres `update`. `kind` is not editable.
pub async fn update(
    pool: &SqlitePool,
    id: MonitorId,
    patch: UpdateMonitor,
    org_id: OrgId,
) -> DbResult<Monitor> {
    let r = sqlx::query(
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
            updated_at          = unixepoch()
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
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }

    // Double-Option fields: only touch when the caller sent the key.
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

/// Set one of the FK-id TEXT columns to a value or NULL. `col` is a fixed
/// internal literal (never user input), so the small dynamic SQL is safe.
async fn set_col_opt(
    pool: &SqlitePool,
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
    pool: &SqlitePool,
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
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Bulk active/paused flip across every monitor carrying `tag` (org-scoped).
/// Returns the number of rows actually changed (idempotent — the `active <> ?`
/// guard skips monitors already in the target state). Mirrors PG.
pub async fn set_active_by_tag(
    pool: &SqlitePool,
    tag: TagId,
    active: bool,
    org_id: OrgId,
) -> DbResult<u64> {
    let r = sqlx::query(
        "UPDATE monitors
            SET active = ?, updated_at = unixepoch()
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

/// 24 url-safe chars of OS entropy (matches the Postgres token shape).
fn generate_push_token() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..24)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect()
}

pub async fn regenerate_push_token(
    pool: &SqlitePool,
    id: MonitorId,
    org_id: OrgId,
) -> DbResult<String> {
    let token = generate_push_token();
    let r = sqlx::query(
        "UPDATE monitors SET push_token = ?, updated_at = unixepoch()
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

pub async fn find_by_push_token(pool: &SqlitePool, token: &str) -> DbResult<Option<MonitorId>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT id FROM monitors WHERE push_token = ? AND active = 1")
            .bind(token)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id,)| mid(&id)))
}

pub async fn fetch_last_push_at(
    pool: &SqlitePool,
    id: MonitorId,
) -> DbResult<Option<OffsetDateTime>> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT last_push_at FROM monitors WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(t,)| t).map(ts))
}

pub async fn mark_run_started(pool: &SqlitePool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET last_run_started_at = unixepoch() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// Terminal push ping: stamp last_push_at, close any open run, and hand back
/// when that run started (for duration). SQLite is single-writer, so a small
/// read-then-write tx replaces the PG `FOR UPDATE` + `RETURNING prior`.
pub async fn close_run(pool: &SqlitePool, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
    let mut tx = pool.begin().await?;
    let prior: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT last_run_started_at FROM monitors WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(&mut *tx)
            .await?;
    let prior = prior.ok_or(DbError::NotFound)?.0;
    sqlx::query(
        "UPDATE monitors SET last_push_at = unixepoch(), last_run_started_at = NULL WHERE id = ?",
    )
    .bind(id.0.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(prior.map(ts))
}

pub async fn push_state(
    pool: &SqlitePool,
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

pub async fn bump_push_at(pool: &SqlitePool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET last_push_at = unixepoch() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

// ── cert + SLO + agent + batch reads ─────────────────────────────────────────

pub async fn set_cert_info(
    pool: &SqlitePool,
    id: MonitorId,
    days_left: i32,
    subject: &str,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE monitors SET cert_days_left = ?, cert_subject = ?, cert_checked_at = unixepoch()
         WHERE id = ?",
    )
    .bind(days_left)
    .bind(subject)
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn slo_state(pool: &SqlitePool, id: MonitorId) -> DbResult<Option<SloState>> {
    let row: Option<(Option<f64>, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT slo_target_pct, slo_window_days, slo_breached_at FROM monitors WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(target_pct, window_days, breached_at)| SloState {
        target_pct,
        window_days: window_days.map(|w| w as i32),
        breached_at: breached_at.map(ts),
    }))
}

pub async fn mark_slo_breached(pool: &SqlitePool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET slo_breached_at = unixepoch() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_slo_breached(pool: &SqlitePool, id: MonitorId) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET slo_breached_at = NULL WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_agent(pool: &SqlitePool, agent: AgentId) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query(
        "SELECT * FROM monitors WHERE agent_id = ? AND active = 1 ORDER BY created_at ASC",
    )
    .bind(agent.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(monitor_from).collect())
}

/// Agent-assigned monitors whose last heartbeat (or, lacking one, their
/// `updated_at`) is older than `interval*2 + 30s` — the scheduler's stale-agent
/// watchdog (the agent went dark and stopped probing). Returns each monitor
/// paired with its agent's name. Mirrors PG `list_stale_agent_monitors`; tags
/// are not hydrated (the watchdog doesn't need them). Cross-tenant by design —
/// the in-process scheduler watches every org.
pub async fn list_stale_agent_monitors(pool: &SqlitePool) -> DbResult<Vec<(Monitor, String)>> {
    let rows = sqlx::query(
        "SELECT m.*, a.name AS agent_name
         FROM monitors m JOIN agents a ON a.id = m.agent_id
         WHERE m.active = 1
           AND m.current_status <> 'paused'
           AND COALESCE(
                 (SELECT MAX(h.ts) FROM heartbeats h WHERE h.monitor_id = m.id),
                 m.updated_at
               ) < unixepoch() - (m.interval_seconds * 2 + 30)",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (monitor_from(r), r.get::<String, _>("agent_name")))
        .collect())
}

/// Public (id → name + status) for a set of monitor ids. Per-id lookups (the
/// id set is small — a status page's monitors); a batched `IN (...)` would need
/// dynamic SQL, which sqlx 0.9 only allows via `AssertSqlSafe`.
pub async fn public_fields_batch(
    pool: &SqlitePool,
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
    use sqlx::SqlitePool;

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

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_read_update_delete(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let m = create(&pool, new_http("api"), org).await.unwrap();
        assert_eq!(m.kind, MonitorKind::Http);
        assert_eq!(m.accepted_statuses, vec![200, 204]);
        assert_eq!(m.http_headers.unwrap()["X-Trace"], "1");
        assert_eq!(m.slo_target_pct, Some(99.9));
        assert!(m.check_cert);
        assert_eq!(m.current_status, MonitorStatus::Pending); // server default
        assert!(m.active);

        // get (org-scoped) + get_unscoped + list + list_all.
        assert_eq!(get(&pool, m.id, org).await.unwrap().name, "api");
        assert_eq!(get_unscoped(&pool, m.id).await.unwrap().name, "api");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(list_all(&pool).await.unwrap().len(), 1);

        // Cross-org isolation: a different org can't see it.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, m.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert_eq!(list(&pool, other.id).await.unwrap().len(), 0);

        // set_active + set_status.
        set_active(&pool, m.id, false, org).await.unwrap();
        assert!(!get(&pool, m.id, org).await.unwrap().active);
        set_status(&pool, m.id, MonitorStatus::Up).await.unwrap();
        assert_eq!(
            get(&pool, m.id, org).await.unwrap().current_status,
            MonitorStatus::Up
        );

        // delete is org-scoped.
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

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn update_group_slo_and_agent(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let m = create(&pool, new_http("u"), org).await.unwrap();

        // COALESCE update: rename + change interval, leave the rest.
        let patch: UpdateMonitor = serde_json::from_value(
            serde_json::json!({ "name": "renamed", "interval_seconds": 120 }),
        )
        .unwrap();
        let upd = update(&pool, m.id, patch, org).await.unwrap();
        assert_eq!(upd.name, "renamed");
        assert_eq!(upd.interval_seconds, 120);
        assert_eq!(upd.url.as_deref(), Some("https://example.com")); // untouched

        // Double-Option: explicit null clears slo_target_pct (created with 99.9).
        let clear: UpdateMonitor =
            serde_json::from_value(serde_json::json!({ "slo_target_pct": null })).unwrap();
        update(&pool, m.id, clear, org).await.unwrap();
        assert!(get(&pool, m.id, org)
            .await
            .unwrap()
            .slo_target_pct
            .is_none());

        // set_group + agent reassign (via update Some(Some)) → list_for_agent.
        let g = MonitorGroupId::new();
        set_group(&pool, m.id, Some(g), org).await.unwrap();
        assert_eq!(get(&pool, m.id, org).await.unwrap().group_id, Some(g));
        let agent = AgentId::new();
        let setagent: UpdateMonitor =
            serde_json::from_value(serde_json::json!({ "agent_id": agent.0.to_string() })).unwrap();
        update(&pool, m.id, setagent, org).await.unwrap();
        assert_eq!(list_for_agent(&pool, agent).await.unwrap().len(), 1);

        // public_fields_batch.
        let pf = public_fields_batch(&pool, &[m.id.0]).await.unwrap();
        assert_eq!(pf.get(&m.id.0).unwrap().0, "renamed");

        // cert + SLO breach lifecycle.
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

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn push_token_and_run_lifecycle(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let mut nm = new_http("push");
        nm.kind = MonitorKind::Push;
        let m = create(&pool, nm, org).await.unwrap();

        // Token regen (push-kind + org gated) → resolvable by token.
        let token = regenerate_push_token(&pool, m.id, org).await.unwrap();
        assert_eq!(find_by_push_token(&pool, &token).await.unwrap(), Some(m.id));
        assert!(find_by_push_token(&pool, "nope").await.unwrap().is_none());

        // Run lifecycle: start → push_state shows open run → close returns it.
        mark_run_started(&pool, m.id).await.unwrap();
        let (_, run) = push_state(&pool, m.id).await.unwrap();
        assert!(run.is_some(), "run open after mark_run_started");
        let prior = close_run(&pool, m.id).await.unwrap();
        assert!(prior.is_some(), "close_run returns the run start");
        let (push, run) = push_state(&pool, m.id).await.unwrap();
        assert!(run.is_none(), "run closed");
        assert!(push.is_some(), "last_push stamped by close_run");

        bump_push_at(&pool, m.id).await.unwrap();
        assert!(fetch_last_push_at(&pool, m.id).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn stale_agent_watchdog(pool: SqlitePool) {
        use crate::sqlite::agents;
        let org = super::super::oid(DEF);
        let agent = agents::create(
            &pool,
            rampart_core::agent::NewAgent {
                name: "probe".into(),
                location: None,
            },
            org,
        )
        .await
        .unwrap()
        .agent
        .id;

        // Two agent-assigned monitors; a third stays local.
        let stale = create(&pool, new_http("stale"), org).await.unwrap();
        let fresh = create(&pool, new_http("fresh"), org).await.unwrap();
        let local_old = create(&pool, new_http("local"), org).await.unwrap();
        for id in [stale.id, fresh.id] {
            let patch: UpdateMonitor =
                serde_json::from_value(serde_json::json!({ "agent_id": agent.0.to_string() }))
                    .unwrap();
            update(&pool, id, patch, org).await.unwrap();
        }

        // Backdate `stale` and `local_old` well past interval*2+30; `fresh`
        // keeps its just-now updated_at.
        for id in [stale.id, local_old.id] {
            sqlx::query("UPDATE monitors SET updated_at = unixepoch() - 100000 WHERE id = ?")
                .bind(id.0.to_string())
                .execute(&pool)
                .await
                .unwrap();
        }

        let out = list_stale_agent_monitors(&pool).await.unwrap();
        // Only the agent-assigned + backdated monitor qualifies; the local one
        // has no agent (JOIN drops it), the fresh one isn't old enough.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.id, stale.id);
        assert_eq!(out[0].1, "probe"); // agent name paired in

        // A recent heartbeat on `stale` clears it from the watchdog.
        crate::sqlite::heartbeats::insert_many(
            &pool,
            &[rampart_core::Heartbeat {
                monitor_id: stale.id,
                ts: OffsetDateTime::now_utc(),
                status: MonitorStatus::Up,
                latency_ms: Some(5),
                status_code: Some(200),
                msg: None,
                retries: 0,
                important: false,
            }],
        )
        .await
        .unwrap();
        assert!(list_stale_agent_monitors(&pool).await.unwrap().is_empty());
    }
}
