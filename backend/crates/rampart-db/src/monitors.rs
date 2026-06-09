//! Monitor repository.
//!
//! Single-tenant — no workspace_id scoping. AuthZ happens at the API
//! layer (is the caller logged in? do they have the right scope?), not
//! the SQL layer.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{MonitorGroupId, ProxyId};
use rampart_core::monitor::{NewMonitor, UpdateMonitor};
use rampart_core::{Monitor, MonitorId, MonitorKind, MonitorStatus};
use time::OffsetDateTime;
use uuid::Uuid;

/// Private row mirror. The API tier always receives `Monitor`.
struct MonitorRow {
    id: Uuid,
    name: String,
    kind: MonitorKind,
    url: Option<String>,
    hostname: Option<String>,
    port: Option<i32>,
    config: serde_json::Value,
    interval_seconds: i32,
    retry_interval_sec: i32,
    max_retries: i32,
    timeout_seconds: i32,
    resend_interval_sec: i32,
    upside_down: bool,
    http_method: String,
    http_body: Option<String>,
    http_headers: Option<serde_json::Value>,
    accepted_statuses: Vec<i32>,
    follow_redirect: bool,
    ignore_tls: bool,
    proxy_id: Option<Uuid>,
    push_token: Option<String>,
    last_push_at: Option<OffsetDateTime>,
    active: bool,
    current_status: MonitorStatus,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    cert_days_left: Option<i32>,
    cert_subject: Option<String>,
    cert_checked_at: Option<OffsetDateTime>,
    group_id: Option<Uuid>,
    // SLO settings. Both NULL when no SLO is configured for this monitor.
    // Stored as f64 / i32 here so the row maps cleanly onto Monitor without
    // needing a numeric crate round-trip; the underlying column is
    // NUMERIC(5,3) so the SELECTs cast to float8 (see queries below).
    slo_target_pct: Option<f64>,
    slo_window_days: Option<i32>,
}

impl From<MonitorRow> for Monitor {
    fn from(r: MonitorRow) -> Self {
        Monitor {
            id: MonitorId::from_uuid(r.id),
            name: r.name,
            kind: r.kind,
            url: r.url,
            hostname: r.hostname,
            port: r.port,
            config: r.config,
            interval_seconds: r.interval_seconds,
            retry_interval_sec: r.retry_interval_sec,
            max_retries: r.max_retries,
            timeout_seconds: r.timeout_seconds,
            resend_interval_sec: r.resend_interval_sec,
            upside_down: r.upside_down,
            http_method: r.http_method,
            http_body: r.http_body,
            http_headers: r.http_headers,
            accepted_statuses: r.accepted_statuses,
            follow_redirect: r.follow_redirect,
            ignore_tls: r.ignore_tls,
            proxy_id: r.proxy_id.map(ProxyId::from_uuid),
            push_token: r.push_token,
            last_push_at: r.last_push_at,
            active: r.active,
            current_status: r.current_status,
            created_at: r.created_at,
            updated_at: r.updated_at,
            tags: Vec::new(),
            cert_days_left: r.cert_days_left,
            cert_subject: r.cert_subject,
            cert_checked_at: r.cert_checked_at,
            group_id: r.group_id.map(MonitorGroupId::from_uuid),
            slo_target_pct: r.slo_target_pct,
            slo_window_days: r.slo_window_days,
        }
    }
}

pub async fn create(pool: &DbPool, input: NewMonitor) -> DbResult<Monitor> {
    let id = MonitorId::new();
    let proxy_uuid: Option<Uuid> = input.proxy_id.map(|p| p.0);

    // Generate a push token only for push monitors. The token goes in the
    // public URL so external jobs can call POST /push/:token; it's checked
    // server-side against the unique index on monitors.push_token.
    let push_token: Option<String> = if input.kind == MonitorKind::Push {
        Some(generate_push_token())
    } else {
        None
    };

    let row = sqlx::query_as!(
        MonitorRow,
        r#"
        INSERT INTO monitors (
            id, name, kind, url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id,
            push_token, group_id,
            slo_target_pct, slo_window_days
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10,
            $11, $12, $13,
            $14, $15, $16,
            $17, $18, $19, $20,
            $21, $22,
            $23::float8::numeric, $24
        )
        RETURNING
            id, name,
            kind   AS "kind: MonitorKind",
            url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id,
            push_token, last_push_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days
        "#,
        id.0,
        input.name,
        input.kind as MonitorKind,
        input.url,
        input.hostname,
        input.port,
        input.config,
        input.interval_seconds,
        input.retry_interval_sec,
        input.max_retries,
        input.timeout_seconds,
        input.resend_interval_sec,
        input.upside_down,
        input.http_method,
        input.http_body,
        input.http_headers,
        &input.accepted_statuses,
        input.follow_redirect,
        input.ignore_tls,
        proxy_uuid,
        push_token,
        input.group_id.map(|g| g.0),
        input.slo_target_pct,
        input.slo_window_days,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

/// Rotate the push token on an existing push monitor. The new token
/// replaces the old one atomically; any in-flight push requests still
/// holding the old token start failing with 404 immediately. Errors
/// with NotFound when the monitor doesn't exist or isn't a push kind.
pub async fn regenerate_push_token(pool: &DbPool, id: MonitorId) -> DbResult<String> {
    let token = generate_push_token();
    let result = sqlx::query!(
        r#"
        UPDATE monitors
           SET push_token = $1, updated_at = NOW()
         WHERE id = $2 AND kind = 'push'
        "#,
        token,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(token)
}

/// 24 url-safe characters of entropy from the OS RNG. Unique-indexed so
/// even an extremely unlikely collision becomes a transient DB error
/// the caller can retry.
fn generate_push_token() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..24)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect()
}

/// Look up a monitor by its push_token. Used by the public /push/:token
/// route to find which monitor to update. Returns None when no monitor
/// matches (so the route can 404 without leaking timing info).
pub async fn find_by_push_token(pool: &DbPool, token: &str) -> DbResult<Option<MonitorId>> {
    let row = sqlx::query!(
        r#"SELECT id FROM monitors WHERE push_token = $1 AND active"#,
        token,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| MonitorId::from_uuid(r.id)))
}

/// Read just last_push_at — cheap one-column lookup for the scheduler's
/// per-tick freshness check on push monitors. Returns None for both
/// "no such monitor" and "never pushed" cases; the caller treats them
/// identically (status = Down).
pub async fn fetch_last_push_at(pool: &DbPool, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
    let row = sqlx::query!(r#"SELECT last_push_at FROM monitors WHERE id = $1"#, id.0,)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|r| r.last_push_at))
}

/// Stash the latest TLS cert snapshot for an https HTTP monitor.
/// `days_left` may be negative for already-expired certs.
pub async fn set_cert_info(
    pool: &DbPool,
    id: MonitorId,
    days_left: i32,
    subject: &str,
) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE monitors
           SET cert_days_left = $1,
               cert_subject   = $2,
               cert_checked_at = NOW()
         WHERE id = $3
        "#,
        days_left,
        subject,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Bump last_push_at to NOW() on a successful push receipt.
pub async fn bump_push_at(pool: &DbPool, id: MonitorId) -> DbResult<()> {
    sqlx::query!(
        r#"UPDATE monitors SET last_push_at = NOW() WHERE id = $1"#,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query_as!(
        MonitorRow,
        r#"
        SELECT
            id, name,
            kind   AS "kind: MonitorKind",
            url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id,
            push_token, last_push_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days
        FROM monitors
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut monitors: Vec<Monitor> = rows.into_iter().map(Monitor::from).collect();
    let ids: Vec<MonitorId> = monitors.iter().map(|m| m.id).collect();
    let tag_map = crate::tags::hydrate_for_monitors(pool, &ids).await?;
    for m in monitors.iter_mut() {
        if let Some(t) = tag_map.get(&m.id) {
            m.tags = t.clone();
        }
    }
    Ok(monitors)
}

pub async fn get(pool: &DbPool, id: MonitorId) -> DbResult<Monitor> {
    let row = sqlx::query_as!(
        MonitorRow,
        r#"
        SELECT
            id, name,
            kind   AS "kind: MonitorKind",
            url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id,
            push_token, last_push_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days
        FROM monitors
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    let mut m: Monitor = row.into();
    m.tags = crate::tags::list_for_monitor(pool, m.id).await?;
    Ok(m)
}

/// Apply a partial update. Every column uses COALESCE so the absence
/// of a field on `UpdateMonitor` leaves the row untouched. `kind` is
/// intentionally not editable. Returns the freshly-hydrated monitor.
pub async fn update(pool: &DbPool, id: MonitorId, patch: UpdateMonitor) -> DbResult<Monitor> {
    let proxy_uuid: Option<Uuid> = patch.proxy_id.map(|p| p.0);
    let accepted: Option<&[i32]> = patch.accepted_statuses.as_deref();

    let result = sqlx::query!(
        r#"
        UPDATE monitors SET
            name                = COALESCE($2, name),
            url                 = COALESCE($3, url),
            hostname            = COALESCE($4, hostname),
            port                = COALESCE($5, port),
            config              = COALESCE($6, config),
            interval_seconds    = COALESCE($7, interval_seconds),
            timeout_seconds     = COALESCE($8, timeout_seconds),
            max_retries         = COALESCE($9, max_retries),
            retry_interval_sec  = COALESCE($10, retry_interval_sec),
            resend_interval_sec = COALESCE($11, resend_interval_sec),
            upside_down         = COALESCE($12, upside_down),
            http_method         = COALESCE($13, http_method),
            http_body           = COALESCE($14, http_body),
            http_headers        = COALESCE($15, http_headers),
            accepted_statuses   = COALESCE($16, accepted_statuses),
            follow_redirect     = COALESCE($17, follow_redirect),
            ignore_tls          = COALESCE($18, ignore_tls),
            proxy_id            = COALESCE($19, proxy_id),
            updated_at          = NOW()
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.url,
        patch.hostname,
        patch.port,
        patch.config,
        patch.interval_seconds,
        patch.timeout_seconds,
        patch.max_retries,
        patch.retry_interval_sec,
        patch.resend_interval_sec,
        patch.upside_down,
        patch.http_method,
        patch.http_body,
        patch.http_headers,
        accepted,
        patch.follow_redirect,
        patch.ignore_tls,
        proxy_uuid,
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }

    // group_id needs Option<Option<…>> semantics — only mutate when the
    // caller explicitly sent the field. A second UPDATE keeps the COALESCE
    // pattern above clean.
    if let Some(g) = patch.group_id {
        sqlx::query!(
            r#"UPDATE monitors SET group_id = $1 WHERE id = $2"#,
            g.map(|x| x.0),
            id.0,
        )
        .execute(pool)
        .await?;
    }

    // SLO target + window: same Option<Option<…>> story. Send `null` to
    // clear; omit to leave alone. The DB CHECK constraint enforces the
    // 90.0–100.0 / 1–90 ranges, so a bad value surfaces as a DbError
    // even if the route layer's validation slips.
    if let Some(t) = patch.slo_target_pct {
        sqlx::query!(
            r#"UPDATE monitors SET slo_target_pct = $1::float8::numeric WHERE id = $2"#,
            t,
            id.0,
        )
        .execute(pool)
        .await?;
    }
    if let Some(w) = patch.slo_window_days {
        sqlx::query!(
            r#"UPDATE monitors SET slo_window_days = $1 WHERE id = $2"#,
            w,
            id.0,
        )
        .execute(pool)
        .await?;
    }

    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: MonitorId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM monitors WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_active(pool: &DbPool, id: MonitorId, active: bool) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE monitors SET active = $1, updated_at = NOW() WHERE id = $2",
        active,
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Assign (or clear, with None) a monitor's group. Used by bulk ops.
pub async fn set_group(
    pool: &DbPool,
    id: MonitorId,
    group: Option<MonitorGroupId>,
) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE monitors SET group_id = $1 WHERE id = $2",
        group.map(|g| g.0),
        id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Atomically transition `current_status`. Called from the scheduler
/// after a heartbeat lands; idempotent (same status → noop).
pub async fn set_status(pool: &DbPool, id: MonitorId, status: MonitorStatus) -> DbResult<()> {
    sqlx::query!(
        "UPDATE monitors SET current_status = $1, updated_at = NOW() WHERE id = $2",
        status as MonitorStatus,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Compact view of a monitor's SLO config + de-dup marker. The scheduler
/// fetches one of these per monitor in a batch after persistence to decide
/// whether to fire `SloBreached` / `SloRecovered`. Cheap single-row read
/// — no tag hydration, no heartbeat lookup. Returns `None` when the
/// monitor row is gone (deleted mid-batch); the caller treats that as
/// "skip silently".
#[derive(Debug, Clone)]
pub struct SloState {
    pub target_pct: Option<f64>,
    pub window_days: Option<i32>,
    pub breached_at: Option<OffsetDateTime>,
}

pub async fn slo_state(pool: &DbPool, id: MonitorId) -> DbResult<Option<SloState>> {
    let row = sqlx::query!(
        r#"
        SELECT
            slo_target_pct::float8 AS "target_pct?",
            slo_window_days,
            slo_breached_at
        FROM monitors
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| SloState {
        target_pct: r.target_pct,
        window_days: r.slo_window_days,
        breached_at: r.slo_breached_at,
    }))
}

/// Stamp `slo_breached_at = NOW()` so the next heartbeat batch won't
/// re-fire the breach notification. The scheduler calls this exactly
/// once when uptime first crosses below target.
pub async fn mark_slo_breached(pool: &DbPool, id: MonitorId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE monitors SET slo_breached_at = NOW() WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Clear `slo_breached_at` so a future breach can fire again. Called
/// exactly once when uptime climbs back at-or-above target.
pub async fn clear_slo_breached(pool: &DbPool, id: MonitorId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE monitors SET slo_breached_at = NULL WHERE id = $1",
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}
