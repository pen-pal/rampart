//! Monitor repository.
//!
//! Single-tenant — no workspace_id scoping. AuthZ happens at the API
//! layer (is the caller logged in? do they have the right scope?), not
//! the SQL layer.

use crate::{DbError, DbPool, DbResult};
use rampart_core::{Monitor, MonitorId, MonitorKind, MonitorStatus};
use rampart_core::monitor::NewMonitor;
use rampart_core::ids::ProxyId;
use time::OffsetDateTime;
use uuid::Uuid;

/// Private row mirror. The API tier always receives `Monitor`.
struct MonitorRow {
    id:                   Uuid,
    name:                 String,
    kind:                 MonitorKind,
    url:                  Option<String>,
    hostname:             Option<String>,
    port:                 Option<i32>,
    config:               serde_json::Value,
    interval_seconds:     i32,
    retry_interval_sec:   i32,
    max_retries:          i32,
    timeout_seconds:      i32,
    resend_interval_sec:  i32,
    upside_down:          bool,
    http_method:          String,
    http_body:            Option<String>,
    http_headers:         Option<serde_json::Value>,
    accepted_statuses:    Vec<i32>,
    follow_redirect:      bool,
    ignore_tls:           bool,
    proxy_id:             Option<Uuid>,
    active:               bool,
    current_status:       MonitorStatus,
    created_at:           OffsetDateTime,
    updated_at:           OffsetDateTime,
}

impl From<MonitorRow> for Monitor {
    fn from(r: MonitorRow) -> Self {
        Monitor {
            id:                   MonitorId::from_uuid(r.id),
            name:                 r.name,
            kind:                 r.kind,
            url:                  r.url,
            hostname:             r.hostname,
            port:                 r.port,
            config:               r.config,
            interval_seconds:     r.interval_seconds,
            retry_interval_sec:   r.retry_interval_sec,
            max_retries:          r.max_retries,
            timeout_seconds:      r.timeout_seconds,
            resend_interval_sec:  r.resend_interval_sec,
            upside_down:          r.upside_down,
            http_method:          r.http_method,
            http_body:            r.http_body,
            http_headers:         r.http_headers,
            accepted_statuses:    r.accepted_statuses,
            follow_redirect:      r.follow_redirect,
            ignore_tls:           r.ignore_tls,
            proxy_id:             r.proxy_id.map(ProxyId::from_uuid),
            active:               r.active,
            current_status:       r.current_status,
            created_at:           r.created_at,
            updated_at:           r.updated_at,
        }
    }
}

pub async fn create(pool: &DbPool, input: NewMonitor) -> DbResult<Monitor> {
    let id = MonitorId::new();
    let proxy_uuid: Option<Uuid> = input.proxy_id.map(|p| p.0);

    let row = sqlx::query_as!(
        MonitorRow,
        r#"
        INSERT INTO monitors (
            id, name, kind, url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10,
            $11, $12, $13,
            $14, $15, $16,
            $17, $18, $19, $20
        )
        RETURNING
            id, name,
            kind   AS "kind: MonitorKind",
            url, hostname, port, config,
            interval_seconds, retry_interval_sec, max_retries,
            timeout_seconds, resend_interval_sec, upside_down,
            http_method, http_body, http_headers,
            accepted_statuses, follow_redirect, ignore_tls, proxy_id,
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at
        "#,
        id.0, input.name, input.kind as MonitorKind,
        input.url, input.hostname, input.port, input.config,
        input.interval_seconds, input.retry_interval_sec, input.max_retries,
        input.timeout_seconds, input.resend_interval_sec, input.upside_down,
        input.http_method, input.http_body, input.http_headers,
        &input.accepted_statuses, input.follow_redirect, input.ignore_tls,
        proxy_uuid,
    )
    .fetch_one(pool)
    .await?;

    Ok(row.into())
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
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at
        FROM monitors
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Monitor::from).collect())
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
            active,
            current_status AS "current_status: MonitorStatus",
            created_at, updated_at
        FROM monitors
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    Ok(row.into())
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
        active, id.0,
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
pub async fn set_status(
    pool: &DbPool,
    id: MonitorId,
    status: MonitorStatus,
) -> DbResult<()> {
    sqlx::query!(
        "UPDATE monitors SET current_status = $1, updated_at = NOW() WHERE id = $2",
        status as MonitorStatus,
        id.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}
