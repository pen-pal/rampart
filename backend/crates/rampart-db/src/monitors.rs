//! Monitor repository.
//!
//! Single-tenant — no workspace_id scoping. AuthZ happens at the API
//! layer (is the caller logged in? do they have the right scope?), not
//! the SQL layer.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{AgentId, EscalationPolicyId, MonitorGroupId, OrgId, ProxyId, TagId};
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
    last_run_started_at: Option<OffsetDateTime>,
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
    agent_id: Option<Uuid>,
    escalation_policy_id: Option<Uuid>,
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
            last_run_started_at: r.last_run_started_at,
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
            agent_id: r.agent_id.map(AgentId::from_uuid),
            escalation_policy_id: r.escalation_policy_id.map(EscalationPolicyId::from_uuid),
        }
    }
}

/// `MonitorRow` plus the joined agent name — only used by the
/// stale-agent watchdog query. Converts through `MonitorRow` so the
/// field mapping lives in exactly one place.
struct StaleAgentRow {
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
    last_run_started_at: Option<OffsetDateTime>,
    active: bool,
    current_status: MonitorStatus,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    cert_days_left: Option<i32>,
    cert_subject: Option<String>,
    cert_checked_at: Option<OffsetDateTime>,
    group_id: Option<Uuid>,
    slo_target_pct: Option<f64>,
    slo_window_days: Option<i32>,
    agent_id: Option<Uuid>,
    escalation_policy_id: Option<Uuid>,
    agent_name: String,
}

impl From<StaleAgentRow> for Monitor {
    fn from(r: StaleAgentRow) -> Self {
        MonitorRow {
            id: r.id,
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
            proxy_id: r.proxy_id,
            push_token: r.push_token,
            last_push_at: r.last_push_at,
            last_run_started_at: r.last_run_started_at,
            active: r.active,
            current_status: r.current_status,
            created_at: r.created_at,
            updated_at: r.updated_at,
            cert_days_left: r.cert_days_left,
            cert_subject: r.cert_subject,
            cert_checked_at: r.cert_checked_at,
            group_id: r.group_id,
            slo_target_pct: r.slo_target_pct,
            slo_window_days: r.slo_window_days,
            agent_id: r.agent_id,
            escalation_policy_id: r.escalation_policy_id,
        }
        .into()
    }
}

pub async fn create(
    pool: &DbPool,
    input: NewMonitor,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<Monitor> {
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
            slo_target_pct, slo_window_days, agent_id, escalation_policy_id,
            org_id
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10,
            $11, $12, $13,
            $14, $15, $16,
            $17, $18, $19, $20,
            $21, $22,
            $23::float8::numeric, $24, $25, $26,
            $27
        )
        RETURNING
            id, name,
            kind   AS "kind: MonitorKind",
            url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id,
            push_token, last_push_at, last_run_started_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days, agent_id, escalation_policy_id
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
        input.agent_id.map(|a| a.0),
        input.escalation_policy_id.map(|e| e.0),
        org_id.0,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
}

/// Rotate the push token on an existing push monitor. The new token
/// replaces the old one atomically; any in-flight push requests still
/// holding the old token start failing with 404 immediately. Errors
/// with NotFound when the monitor doesn't exist or isn't a push kind.
pub async fn regenerate_push_token(
    pool: &DbPool,
    id: MonitorId,
    org_id: OrgId,
) -> DbResult<String> {
    let token = generate_push_token();
    let result = sqlx::query!(
        r#"
        UPDATE monitors
           SET push_token = $1, updated_at = NOW()
         WHERE id = $2 AND kind = 'push' AND org_id = $3
        "#,
        token,
        id.0,
        org_id.0,
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
/// `state=run` ping: open a run. Overwrites any prior open run — a new
/// start supersedes a crashed predecessor.
pub async fn mark_run_started(pool: &DbPool, id: MonitorId) -> DbResult<()> {
    sqlx::query!(
        r#"UPDATE monitors SET last_run_started_at = NOW() WHERE id = $1"#,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Terminal ping (`complete` / `fail` / legacy `up` / `down`): stamp the
/// liveness clock, close any open run, and hand back when that run
/// started so the caller can compute the job's duration. Single
/// statement so a concurrent ping can't observe a half-applied state.
pub async fn close_run(pool: &DbPool, id: MonitorId) -> DbResult<Option<OffsetDateTime>> {
    let row = sqlx::query!(
        r#"
        UPDATE monitors m SET
            last_push_at        = NOW(),
            last_run_started_at = NULL
        FROM (
            SELECT id, last_run_started_at AS prior
            FROM monitors WHERE id = $1
            FOR UPDATE
        ) prev
        WHERE m.id = prev.id
        RETURNING prev.prior
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.prior)
}

/// Fresh (last_push_at, last_run_started_at) pair for the scheduler's
/// push tick — read together so the two clocks can't tear.
pub async fn push_state(
    pool: &DbPool,
    id: MonitorId,
) -> DbResult<(Option<OffsetDateTime>, Option<OffsetDateTime>)> {
    let row = sqlx::query!(
        r#"SELECT last_push_at, last_run_started_at FROM monitors WHERE id = $1"#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok((row.last_push_at, row.last_run_started_at))
}

pub async fn bump_push_at(pool: &DbPool, id: MonitorId) -> DbResult<()> {
    sqlx::query!(
        r#"UPDATE monitors SET last_push_at = NOW() WHERE id = $1"#,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Monitors owned by one org — the dashboard list. Multi-tenancy: this is the
/// highest-risk cross-tenant enumeration surface, so it filters by `org_id`.
/// (Tag hydration is by the org-filtered ids, so it can't leak cross-org tags.)
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<Monitor>> {
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
            push_token, last_push_at, last_run_started_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days, agent_id, escalation_policy_id
        FROM monitors
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
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

/// Every monitor across ALL orgs — for system callers that legitimately need
/// the whole fleet (the scheduler's probe loop, retention/seed/import tooling,
/// scheduled-report generation). NEVER call this on a request path: it is not
/// org-scoped and would leak cross-tenant. The org-scoped [`list`] is the
/// request-path entry point.
pub async fn list_all(pool: &DbPool) -> DbResult<Vec<Monitor>> {
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
            push_token, last_push_at, last_run_started_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days, agent_id, escalation_policy_id
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

/// Monitors assigned to one agent, active only — the agent pull feed.
/// No tag hydration: the agent needs probe config, not cosmetics.
pub async fn list_for_agent(pool: &DbPool, agent: AgentId) -> DbResult<Vec<Monitor>> {
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
            push_token, last_push_at, last_run_started_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days, agent_id, escalation_policy_id
        FROM monitors
        WHERE agent_id = $1 AND active
        ORDER BY created_at
        "#,
        agent.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Monitor::from).collect())
}

/// Agent-assigned monitors that have gone quiet: active, assigned to an
/// agent, and whose newest heartbeat is older than twice the monitor's
/// interval plus a 30s grace (or that never produced one since
/// assignment). Drives the scheduler's stale-agent watchdog; the row
/// carries the agent name so the synthesized heartbeat can say which
/// agent went dark.
pub async fn list_stale_agent_monitors(pool: &DbPool) -> DbResult<Vec<(Monitor, String)>> {
    let rows = sqlx::query_as!(
        StaleAgentRow,
        r#"
        SELECT
            m.id, m.name,
            m.kind   AS "kind: MonitorKind",
            m.url, m.hostname, m.port, m.config,
            m.interval_seconds, m.retry_interval_sec, m.max_retries,
            m.timeout_seconds, m.resend_interval_sec, m.upside_down,
            m.http_method, m.http_body, m.http_headers,
            m.accepted_statuses, m.follow_redirect, m.ignore_tls, m.proxy_id,
            m.push_token, m.last_push_at, m.last_run_started_at,
            m.active,
            m.current_status AS "current_status: MonitorStatus",
            m.created_at, m.updated_at,
            m.cert_days_left, m.cert_subject, m.cert_checked_at,
            m.group_id,
            m.slo_target_pct::float8 AS "slo_target_pct?",
            m.slo_window_days, m.agent_id, m.escalation_policy_id,
            a.name AS agent_name
        FROM monitors m
        JOIN agents a ON a.id = m.agent_id
        WHERE m.active
          AND m.current_status <> 'paused'
          AND COALESCE(
                (SELECT MAX(h.ts) FROM heartbeats h WHERE h.monitor_id = m.id),
                m.updated_at
              ) < NOW() - (m.interval_seconds * 2 + 30) * INTERVAL '1 second'
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let agent_name = r.agent_name.clone();
            (r.into(), agent_name)
        })
        .collect())
}

/// Fetch one monitor, scoped to an org. Returns `NotFound` if the monitor
/// doesn't exist OR belongs to another org (so a cross-org id is an IDOR-safe
/// 404, not a leak). The request path uses this; system/runtime callers that
/// already hold a trusted `MonitorId` use [`get_unscoped`].
pub async fn get(pool: &DbPool, id: MonitorId, org_id: OrgId) -> DbResult<Monitor> {
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
            push_token, last_push_at, last_run_started_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days, agent_id, escalation_policy_id
        FROM monitors
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    let mut m: Monitor = row.into();
    m.tags = crate::tags::list_for_monitor(pool, m.id).await?;
    Ok(m)
}

/// Fetch one monitor WITHOUT org scoping — for system/runtime callers that
/// already hold a trusted `MonitorId` (the scheduler probe loop, the notifier
/// fan-out, push ingest, an agent reporting its own monitor, a status-page
/// resolving a linked monitor). NEVER call this directly from a request
/// handler that takes a user-supplied id — use the org-scoped [`get`].
pub async fn get_unscoped(pool: &DbPool, id: MonitorId) -> DbResult<Monitor> {
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
            push_token, last_push_at, last_run_started_at,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at,
            cert_days_left, cert_subject, cert_checked_at,
            group_id,
            slo_target_pct::float8 AS "slo_target_pct?",
            slo_window_days, agent_id, escalation_policy_id
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
pub async fn update(
    pool: &DbPool,
    id: MonitorId,
    patch: UpdateMonitor,
    org_id: OrgId,
) -> DbResult<Monitor> {
    let proxy_uuid: Option<Uuid> = patch.proxy_id.map(|p| p.0);
    let accepted: Option<&[i32]> = patch.accepted_statuses.as_deref();

    // The primary UPDATE gates on org_id: a monitor in another org matches 0
    // rows → NotFound below → the follow-up per-field UPDATEs (which target the
    // same id) never run, so org scoping holds for the whole function.
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
        WHERE id = $1 AND org_id = $20
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
        org_id.0,
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

    // Probe-agent assignment: Option<Option<…>> like group_id. `null`
    // returns the monitor to local probing.
    if let Some(a) = patch.agent_id {
        sqlx::query!(
            r#"UPDATE monitors SET agent_id = $1 WHERE id = $2"#,
            a.map(|x| x.0),
            id.0,
        )
        .execute(pool)
        .await?;
    }

    // Escalation policy: same pattern. `null` detaches.
    if let Some(e) = patch.escalation_policy_id {
        sqlx::query!(
            r#"UPDATE monitors SET escalation_policy_id = $1 WHERE id = $2"#,
            e.map(|x| x.0),
            id.0,
        )
        .execute(pool)
        .await?;
    }

    get(pool, id, org_id).await
}

pub async fn delete(pool: &DbPool, id: MonitorId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM monitors WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_active(pool: &DbPool, id: MonitorId, active: bool, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE monitors SET active = $1, updated_at = NOW() WHERE id = $2 AND org_id = $3",
        active,
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Flip `active` on every monitor carrying `tag` in a single statement.
/// Used by the "pause/resume all with this tag" bulk action. Returns the
/// number of rows actually changed — a monitor already in the target
/// state isn't re-counted (the `active <> $1` guard keeps the count to
/// genuine transitions, which is what the operator wants to hear back).
pub async fn set_active_by_tag(
    pool: &DbPool,
    tag: TagId,
    active: bool,
    org_id: OrgId,
) -> DbResult<u64> {
    let result = sqlx::query!(
        r#"
        UPDATE monitors
           SET active = $1, updated_at = NOW()
         WHERE active <> $1
           AND org_id = $3
           AND id IN (SELECT monitor_id FROM monitor_tags WHERE tag_id = $2)
        "#,
        active,
        tag.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Assign (or clear, with None) a monitor's group. Used by bulk ops. Scoped to
/// the monitor's org. (Validating the target group is in the same org lands
/// with the monitor_groups domain; behaviour-identical while one org exists.)
pub async fn set_group(
    pool: &DbPool,
    id: MonitorId,
    group: Option<MonitorGroupId>,
    org_id: OrgId,
) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE monitors SET group_id = $1 WHERE id = $2 AND org_id = $3",
        group.map(|g| g.0),
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Field set for a transactional bulk edit. Every field is tri-/bi-state so
/// "not supplied" is distinct from "supplied":
/// - `interval_seconds` / `timeout_seconds` / `active`: `None` → leave alone,
///   `Some(v)` → set on every listed monitor.
/// - `group_id`: `Option<Option<…>>` — `None` → leave alone, `Some(None)` →
///   clear the group, `Some(Some(g))` → set it. (Double-option, same shape as
///   `UpdateMonitor::group_id`.)
/// - `tags`: `None` → leave the monitor's tags untouched, `Some(list)` →
///   replace the full tag set with `list` (empty list clears all tags).
#[derive(Debug, Clone, Default)]
pub struct BulkEditPatch {
    pub interval_seconds: Option<i32>,
    pub timeout_seconds: Option<i32>,
    pub active: Option<bool>,
    pub group_id: Option<Option<MonitorGroupId>>,
    pub tags: Option<Vec<TagId>>,
}

impl BulkEditPatch {
    /// True when the patch carries at least one column-level change (i.e.
    /// something to UPDATE on the `monitors` row itself). Tag replacement is
    /// handled separately and is not counted here.
    fn touches_columns(&self) -> bool {
        self.interval_seconds.is_some()
            || self.timeout_seconds.is_some()
            || self.active.is_some()
            || self.group_id.is_some()
    }
}

/// A snapshot of the fields a bulk edit is allowed to touch, captured for a
/// single monitor BEFORE the edit is applied. Only the columns the patch is
/// actually about are meaningful for undo/preview, but we capture them all so
/// the caller can diff and build an inverse patch. `tags` holds the prior tag
/// id set (order-independent).
#[derive(Debug, Clone)]
pub struct MonitorPrior {
    pub id: MonitorId,
    pub name: String,
    pub interval_seconds: i32,
    pub timeout_seconds: i32,
    pub active: bool,
    pub group_id: Option<MonitorGroupId>,
    pub tags: Vec<TagId>,
}

/// Outcome of a [`bulk_edit`] call: how many of the requested ids were found
/// and mutated, and how many were unknown (no such monitor) and skipped.
/// `priors` carries the pre-edit snapshot of every monitor that was actually
/// mutated, in request order, so the caller can construct an inverse ("undo")
/// payload without any server-side undo state.
#[derive(Debug, Clone)]
pub struct BulkEditOutcome {
    pub updated: usize,
    pub skipped_unknown: usize,
    pub priors: Vec<MonitorPrior>,
}

/// Read-only twin of [`bulk_edit`]: resolve which monitors exist and capture
/// their current (pre-edit) field snapshot WITHOUT opening a write
/// transaction or mutating anything. Returns the priors for every id that
/// resolves to a monitor (in request order) plus a count of unknown ids.
/// The caller diffs each prior against `patch` to build the preview.
pub async fn bulk_edit_preview(
    pool: &DbPool,
    ids: &[MonitorId],
    want_tags: bool,
    org_id: OrgId,
) -> DbResult<(Vec<MonitorPrior>, usize)> {
    let mut priors = Vec::new();
    let mut skipped_unknown = 0usize;

    for id in ids {
        match load_prior(pool, *id, want_tags, org_id).await? {
            Some(prior) => priors.push(prior),
            None => skipped_unknown += 1,
        }
    }

    Ok((priors, skipped_unknown))
}

/// Fetch the editable-field snapshot for one monitor. `None` when the id
/// doesn't resolve *within `org_id`* — an id in another org is reported as
/// unknown (skipped), never read. `with_tags` skips the tag read when the
/// patch isn't touching tags (avoids a query per monitor for the common case).
async fn load_prior<'e, E>(
    executor: E,
    id: MonitorId,
    with_tags: bool,
    org_id: OrgId,
) -> DbResult<Option<MonitorPrior>>
where
    E: sqlx::PgExecutor<'e> + Copy,
{
    let row = sqlx::query!(
        r#"
        SELECT name, interval_seconds, timeout_seconds, active, group_id
        FROM monitors
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(executor)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let tags = if with_tags {
        sqlx::query_scalar!(
            r#"SELECT tag_id FROM monitor_tags WHERE monitor_id = $1 ORDER BY tag_id"#,
            id.0,
        )
        .fetch_all(executor)
        .await?
        .into_iter()
        .map(TagId::from_uuid)
        .collect()
    } else {
        Vec::new()
    };

    Ok(Some(MonitorPrior {
        id,
        name: row.name,
        interval_seconds: row.interval_seconds,
        timeout_seconds: row.timeout_seconds,
        active: row.active,
        group_id: row.group_id.map(MonitorGroupId::from_uuid),
        tags,
    }))
}

/// Apply `patch` to every monitor in `ids` inside ONE transaction — either
/// the whole batch commits or nothing does. Ids that don't resolve to a
/// monitor row are skipped (counted in `skipped_unknown`) rather than
/// aborting the batch. Fields omitted from `patch` are left untouched on
/// every monitor (COALESCE for the scalar columns; group/tags only mutated
/// when explicitly supplied).
pub async fn bulk_edit(
    pool: &DbPool,
    ids: &[MonitorId],
    patch: &BulkEditPatch,
    org_id: OrgId,
) -> DbResult<BulkEditOutcome> {
    let mut tx = pool.begin().await?;

    let interval = patch.interval_seconds;
    let timeout = patch.timeout_seconds;
    let active = patch.active;
    // Flatten the group double-option into "should we touch it?" + value.
    let set_group = patch.group_id.is_some();
    let group_uuid: Option<Uuid> = patch.group_id.flatten().map(|g| g.0);

    let mut updated = 0usize;
    let mut skipped_unknown = 0usize;
    let mut priors: Vec<MonitorPrior> = Vec::new();
    let capture_tags = patch.tags.is_some();

    for id in ids {
        // Lock + snapshot the prior values in one shot so unknown ids are
        // reported separately from genuine no-op updates, the row stays
        // stable for the rest of this transaction (FOR UPDATE), and we
        // retain the pre-edit state needed to build the inverse undo patch.
        let prior_row = sqlx::query!(
            r#"
            SELECT name, interval_seconds, timeout_seconds, active, group_id
            FROM monitors
            WHERE id = $1 AND org_id = $2
            FOR UPDATE
            "#,
            id.0,
            org_id.0,
        )
        .fetch_optional(&mut *tx)
        .await?;

        // Unknown id OR an id in another org: skip (counted as unknown), never
        // touched. The subsequent column/group/tag updates run only on this
        // org-confirmed, row-locked monitor.
        let Some(prior_row) = prior_row else {
            skipped_unknown += 1;
            continue;
        };

        // Snapshot the prior tag set (only when the patch replaces tags;
        // otherwise tags are untouched and irrelevant to the undo).
        let prior_tags: Vec<TagId> = if capture_tags {
            sqlx::query_scalar!(
                r#"SELECT tag_id FROM monitor_tags WHERE monitor_id = $1 ORDER BY tag_id"#,
                id.0,
            )
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(TagId::from_uuid)
            .collect()
        } else {
            Vec::new()
        };

        priors.push(MonitorPrior {
            id: *id,
            name: prior_row.name,
            interval_seconds: prior_row.interval_seconds,
            timeout_seconds: prior_row.timeout_seconds,
            active: prior_row.active,
            group_id: prior_row.group_id.map(MonitorGroupId::from_uuid),
            tags: prior_tags,
        });

        if patch.touches_columns() {
            sqlx::query!(
                r#"
                UPDATE monitors SET
                    interval_seconds = COALESCE($2, interval_seconds),
                    timeout_seconds  = COALESCE($3, timeout_seconds),
                    active           = COALESCE($4, active),
                    group_id         = CASE WHEN $5 THEN $6 ELSE group_id END,
                    updated_at       = NOW()
                WHERE id = $1
                "#,
                id.0,
                interval,
                timeout,
                active,
                set_group,
                group_uuid,
            )
            .execute(&mut *tx)
            .await?;
        }

        // Tag replacement: clear then re-insert the supplied set. Empty list
        // is a deliberate "remove all tags".
        if let Some(tags) = &patch.tags {
            sqlx::query!(r#"DELETE FROM monitor_tags WHERE monitor_id = $1"#, id.0)
                .execute(&mut *tx)
                .await?;
            for tag in tags {
                sqlx::query!(
                    r#"
                    INSERT INTO monitor_tags (monitor_id, tag_id)
                    VALUES ($1, $2)
                    ON CONFLICT DO NOTHING
                    "#,
                    id.0,
                    tag.0,
                )
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
