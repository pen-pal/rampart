//! Audit log — append-only record of mutating actions.
//!
//! Append fits a single INSERT; reads use the descending `ts` index
//! plus an optional `(before_id)` cursor so the UI can paginate without
//! offset/limit drift.

use crate::{DbPool, DbResult};
use rampart_core::{ApiKeyId, UserId};
use serde::Serialize;
use sqlx::types::ipnetwork::IpNetwork;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub actor_user_id: Option<UserId>,
    pub actor_api_key_id: Option<ApiKeyId>,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<Uuid>,
    pub payload: Option<serde_json::Value>,
    pub ip_addr: Option<String>,
    pub user_agent: Option<String>,
    pub ts: OffsetDateTime,
}

pub struct NewEntry<'a> {
    pub actor_user_id: Option<UserId>,
    pub actor_api_key_id: Option<ApiKeyId>,
    pub action: &'a str,
    pub resource_kind: &'a str,
    pub resource_id: Option<Uuid>,
    pub payload: Option<serde_json::Value>,
    pub ip_addr: Option<IpNetwork>,
    pub user_agent: Option<&'a str>,
}

pub async fn insert(pool: &DbPool, entry: NewEntry<'_>) -> DbResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO audit_log
            (actor_user_id, actor_api_key_id, action, resource_kind,
             resource_id, payload, ip_addr, user_agent)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        entry.actor_user_id.map(|u| u.0),
        entry.actor_api_key_id.map(|k| k.0),
        entry.action,
        entry.resource_kind,
        entry.resource_id,
        entry.payload,
        entry.ip_addr,
        entry.user_agent,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub struct AuditFilter<'a> {
    pub before_id: Option<i64>,
    pub kind: Option<&'a str>,
    /// Prefix match on `action` (e.g. "monitor." matches every monitor
    /// action). Empty/None = no filter.
    pub action_prefix: Option<&'a str>,
    pub actor: Option<Uuid>,
    /// Inclusive lower bound on `ts`. None = no lower bound.
    pub from: Option<OffsetDateTime>,
    /// Inclusive upper bound on `ts`. None = no upper bound.
    pub to: Option<OffsetDateTime>,
}

pub async fn list(pool: &DbPool, limit: i64, filter: AuditFilter<'_>) -> DbResult<Vec<AuditEntry>> {
    let limit = limit.clamp(1, 500);
    // Build the LIKE pattern host-side so the SQL stays a plain
    // parameter bind (no string concat into the query text).
    let action_like = filter.action_prefix.map(|p| format!("{p}%"));
    let rows = sqlx::query!(
        r#"
        SELECT id, actor_user_id, actor_api_key_id, action, resource_kind,
               resource_id, payload, ip_addr, user_agent, ts
        FROM audit_log
        WHERE ($1::bigint IS NULL OR id < $1)
          AND ($2::text   IS NULL OR resource_kind = $2)
          AND ($3::text   IS NULL OR action LIKE $3)
          AND ($4::uuid   IS NULL OR actor_user_id = $4)
          AND ($5::timestamptz IS NULL OR ts >= $5)
          AND ($6::timestamptz IS NULL OR ts <= $6)
        ORDER BY id DESC
        LIMIT $7
        "#,
        filter.before_id,
        filter.kind,
        action_like,
        filter.actor,
        filter.from,
        filter.to,
        limit,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| AuditEntry {
            id: r.id,
            actor_user_id: r.actor_user_id.map(UserId::from_uuid),
            actor_api_key_id: r.actor_api_key_id.map(ApiKeyId::from_uuid),
            action: r.action,
            resource_kind: r.resource_kind,
            resource_id: r.resource_id,
            payload: r.payload,
            ip_addr: r.ip_addr.map(|n| n.network().to_string()),
            user_agent: r.user_agent,
            ts: r.ts,
        })
        .collect())
}

/// One row of the CSV export, with the actor already resolved to a human
/// label: the user's email when the action was performed by a logged-in
/// admin, otherwise the api-key id. The `payload` is intentionally absent —
/// it is sanitised at write time but is free-form JSON we do not surface in
/// the flat CSV. Every textual field here is already redaction-safe.
#[derive(Debug, Clone)]
pub struct ExportRow {
    pub id: i64,
    pub ts: OffsetDateTime,
    /// Email of the acting user, or the api-key id, or empty (system action).
    pub actor: String,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: Option<Uuid>,
    pub ip_addr: Option<String>,
    pub user_agent: Option<String>,
}

/// Time-range filter for [`export_batch`]. Mirrors the `from`/`to` of
/// [`AuditFilter`]; the export deliberately does not expose the other
/// (kind/action/actor) filters — an export is a full window dump.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExportFilter {
    /// Inclusive lower bound on `ts`. None = no lower bound.
    pub from: Option<OffsetDateTime>,
    /// Inclusive upper bound on `ts`. None = no upper bound.
    pub to: Option<OffsetDateTime>,
}

/// Fetch a single keyset page of export rows in descending id order.
///
/// Pass `before_id = None` for the first page; thereafter pass the `id` of
/// the last row of the previous page. Returns up to `batch` rows. The caller
/// streams these pages back-to-back so the server never holds more than one
/// batch in memory regardless of how large the audit log is.
///
/// `batch` is clamped to a sane window so a caller can't ask for the whole
/// table in one query.
pub async fn export_batch(
    pool: &DbPool,
    before_id: Option<i64>,
    batch: i64,
    filter: ExportFilter,
) -> DbResult<Vec<ExportRow>> {
    let batch = batch.clamp(1, 5_000);
    let rows = sqlx::query!(
        r#"
        SELECT a.id,
               a.ts,
               a.action,
               a.resource_kind,
               a.resource_id,
               a.ip_addr,
               a.user_agent,
               u.email::text       AS actor_email,
               a.actor_api_key_id  AS actor_api_key_id
        FROM audit_log a
        LEFT JOIN users u ON u.id = a.actor_user_id
        WHERE ($1::bigint IS NULL OR a.id < $1)
          AND ($2::timestamptz IS NULL OR a.ts >= $2)
          AND ($3::timestamptz IS NULL OR a.ts <= $3)
        ORDER BY a.id DESC
        LIMIT $4
        "#,
        before_id,
        filter.from,
        filter.to,
        batch,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let actor = r
                .actor_email
                .or_else(|| r.actor_api_key_id.map(|k| k.to_string()))
                .unwrap_or_default();
            ExportRow {
                id: r.id,
                ts: r.ts,
                actor,
                action: r.action,
                resource_kind: r.resource_kind,
                resource_id: r.resource_id,
                ip_addr: r.ip_addr.map(|n| n.network().to_string()),
                user_agent: r.user_agent,
            }
        })
        .collect())
}
