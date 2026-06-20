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

/// Advisory-lock key serializing audit appends so the hash chain stays linear.
const AUDIT_CHAIN_LOCK: i64 = 0x4155_4449; // "AUDI"

pub async fn insert(pool: &DbPool, entry: NewEntry<'_>) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    // Serialize appends — two concurrent inserts must not fork the chain.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(AUDIT_CHAIN_LOCK)
        .execute(&mut *tx)
        .await?;
    // Current chain tip (NULL for the first hashed row).
    let prev_hash: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?
    .flatten();

    // Insert first to get the BIGSERIAL id + ts that feed the hash.
    let row = sqlx::query!(
        r#"
        INSERT INTO audit_log
            (actor_user_id, actor_api_key_id, action, resource_kind,
             resource_id, payload, ip_addr, user_agent, prev_hash)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, ts
        "#,
        entry.actor_user_id.map(|u| u.0),
        entry.actor_api_key_id.map(|k| k.0),
        entry.action,
        entry.resource_kind,
        entry.resource_id,
        entry.payload,
        entry.ip_addr,
        entry.user_agent,
        prev_hash,
    )
    .fetch_one(&mut *tx)
    .await?;

    let hash = chain_hash(
        prev_hash.as_deref(),
        row.id,
        row.ts,
        entry.action,
        entry.resource_kind,
        entry.resource_id,
        entry.actor_user_id.map(|u| u.0),
        entry.actor_api_key_id.map(|k| k.0),
        entry.payload.as_ref(),
        entry.ip_addr.map(|i| i.to_string()).as_deref(),
        entry.user_agent,
    );
    sqlx::query!("UPDATE audit_log SET hash = $2 WHERE id = $1", row.id, hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// HMAC-SHA256 (keyed by RAMPART_SECRET_KEY, kept outside the DB) over the
/// previous hash + this row's immutable content; SHA-256 fallback when no key
/// is configured. Hex-encoded. Same inputs on insert + verify.
#[allow(clippy::too_many_arguments)]
fn chain_hash(
    prev: Option<&str>,
    id: i64,
    ts: OffsetDateTime,
    action: &str,
    resource_kind: &str,
    resource_id: Option<Uuid>,
    actor_user: Option<Uuid>,
    actor_key: Option<Uuid>,
    payload: Option<&serde_json::Value>,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> String {
    let opt = |s: Option<String>| s.unwrap_or_default();
    // \x1e (record separator) between fields so values can't run together.
    let canonical = [
        prev.unwrap_or("").to_string(),
        id.to_string(),
        ts.unix_timestamp_nanos().to_string(),
        action.to_string(),
        resource_kind.to_string(),
        opt(resource_id.map(|u| u.to_string())),
        opt(actor_user.map(|u| u.to_string())),
        opt(actor_key.map(|u| u.to_string())),
        opt(payload.map(|p| p.to_string())),
        opt(ip.map(|s| s.to_string())),
        opt(user_agent.map(|s| s.to_string())),
    ]
    .join("\x1e");
    mac_hex(canonical.as_bytes())
}

fn mac_key() -> Option<Vec<u8>> {
    let raw = std::env::var("RAMPART_SECRET_KEY").ok()?;
    let s = raw.trim();
    if s.len() == 64 {
        if let Ok(v) = hex::decode(s) {
            return Some(v);
        }
    }
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s).ok()
}

fn mac_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    match mac_key() {
        Some(key) => {
            use hmac::{Hmac, Mac};
            let mut m = <Hmac<Sha256>>::new_from_slice(&key).expect("hmac accepts any key length");
            m.update(data);
            hex::encode(m.finalize().into_bytes())
        }
        None => hex::encode(Sha256::digest(data)),
    }
}

/// Result of [`verify_chain`].
#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub ok: bool,
    pub checked: i64,
    /// First row whose stored hash / linkage doesn't match — i.e. where the
    /// chain was tampered with (or truncated before).
    pub first_bad_id: Option<i64>,
}

/// Settings key holding the prune watermark: `{id, hash}` of the newest hashed
/// audit row removed by retention. Listed in `settings` SECRET_KEYS, so when
/// `RAMPART_SECRET_KEY` is set the value is sealed (AES-GCM) at rest — a
/// DB-level attacker can't forge a watermark to mask a malicious head deletion
/// without the key.
const CHAIN_WATERMARK_KEY: &str = "audit_chain_watermark";

/// Record the prune watermark so [`verify_chain`] knows the legitimate
/// pre-prune chain head. Called by the retention prune right after it deletes
/// the oldest audit rows. `hash` is the hash of the newest deleted hashed row —
/// exactly the `prev_hash` the oldest *surviving* row links back to.
pub async fn set_chain_watermark(pool: &DbPool, id: i64, hash: &str) -> DbResult<()> {
    let v = serde_json::json!({ "id": id, "hash": hash });
    crate::settings::put(pool, CHAIN_WATERMARK_KEY, &v).await
}

/// The hash recorded by the last prune, if any. [`verify_chain`] seeds the
/// chain from this instead of `None` so routine retention isn't mistaken for
/// tamper.
async fn chain_watermark_hash(pool: &DbPool) -> DbResult<Option<String>> {
    let raw = crate::settings::get(pool, CHAIN_WATERMARK_KEY).await?;
    Ok(raw.and_then(|v| v.get("hash").and_then(|h| h.as_str()).map(str::to_owned)))
}

/// Re-walk the hash chain (rows with a hash, oldest first) and recompute each
/// entry's hash. Any edit/delete/reorder breaks the chain at the affected row.
///
/// The chain is seeded from the prune watermark (the hash of the newest audit
/// row retention deleted), so a legitimate head-truncation still verifies —
/// while deleting a row *after* the watermark, or editing a surviving row,
/// still breaks it. With `RAMPART_SECRET_KEY` set the watermark is sealed, so
/// it can't be forged to hide a malicious head deletion either.
pub async fn verify_chain(pool: &DbPool) -> DbResult<VerifyReport> {
    let rows = sqlx::query!(
        r#"
        SELECT id, ts, action, resource_kind, resource_id, payload,
               ip_addr AS "ip_addr: IpNetwork", user_agent,
               actor_user_id, actor_api_key_id, prev_hash, hash
        FROM audit_log
        WHERE hash IS NOT NULL
        ORDER BY id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    // Seed from the prune watermark so retention head-truncation verifies;
    // `None` (never pruned) keeps the original genesis-row behaviour.
    let mut prev: Option<String> = chain_watermark_hash(pool).await?;
    let mut checked = 0i64;
    for r in rows {
        let expect = chain_hash(
            prev.as_deref(),
            r.id,
            r.ts,
            &r.action,
            &r.resource_kind,
            r.resource_id,
            r.actor_user_id,
            r.actor_api_key_id,
            r.payload.as_ref(),
            r.ip_addr.map(|i| i.to_string()).as_deref(),
            r.user_agent.as_deref(),
        );
        if r.prev_hash.as_deref() != prev.as_deref() || r.hash.as_deref() != Some(&expect) {
            return Ok(VerifyReport {
                ok: false,
                checked,
                first_bad_id: Some(r.id),
            });
        }
        prev = r.hash;
        checked += 1;
    }
    Ok(VerifyReport {
        ok: true,
        checked,
        first_bad_id: None,
    })
}

#[derive(Debug, Serialize)]
pub struct IpCount {
    pub ip: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct HourCount {
    pub hour: OffsetDateTime,
    pub count: i64,
}

/// Aggregate security signal from the audit log over the last `hours`: auth
/// outcomes, the source IPs behind failed logins, and a per-hour failed-login
/// series for a sparkline. The audit log is the security-event store, so this is
/// "security insights" without a separate SIEM.
#[derive(Debug, Serialize)]
pub struct SecurityInsights {
    pub failed_logins: i64,
    pub successful_logins: i64,
    pub totp_failures: i64,
    pub top_ips: Vec<IpCount>,
    pub by_hour: Vec<HourCount>,
}

pub async fn security_insights(pool: &DbPool, hours: i32) -> DbResult<SecurityInsights> {
    let counts = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE action = 'auth.login_failed') AS "failed!",
            COUNT(*) FILTER (WHERE action = 'auth.login')        AS "ok!",
            COUNT(*) FILTER (WHERE action = 'auth.totp_failed')  AS "totp!"
        FROM audit_log
        WHERE ts >= now() - make_interval(hours => $1)
        "#,
        hours,
    )
    .fetch_one(pool)
    .await?;

    let top = sqlx::query!(
        r#"
        SELECT host(ip_addr) AS "ip!", COUNT(*) AS "count!"
        FROM audit_log
        WHERE action = 'auth.login_failed' AND ip_addr IS NOT NULL
          AND ts >= now() - make_interval(hours => $1)
        GROUP BY ip_addr
        ORDER BY COUNT(*) DESC
        LIMIT 10
        "#,
        hours,
    )
    .fetch_all(pool)
    .await?;

    let hourly = sqlx::query!(
        r#"
        SELECT date_trunc('hour', ts) AS "hour!", COUNT(*) AS "count!"
        FROM audit_log
        WHERE action = 'auth.login_failed'
          AND ts >= now() - make_interval(hours => $1)
        GROUP BY 1
        ORDER BY 1
        "#,
        hours,
    )
    .fetch_all(pool)
    .await?;

    Ok(SecurityInsights {
        failed_logins: counts.failed,
        successful_logins: counts.ok,
        totp_failures: counts.totp,
        top_ips: top
            .into_iter()
            .map(|r| IpCount {
                ip: r.ip,
                count: r.count,
            })
            .collect(),
        by_hour: hourly
            .into_iter()
            .map(|r| HourCount {
                hour: r.hour,
                count: r.count,
            })
            .collect(),
    })
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

/// Audit rows with `id > after_id`, oldest first — the forward cursor the SIEM
/// exporter tails. `after_id = 0` starts from the beginning.
pub async fn fetch_since(pool: &DbPool, after_id: i64, limit: i64) -> DbResult<Vec<AuditEntry>> {
    let limit = limit.clamp(1, 1000);
    let rows = sqlx::query!(
        r#"
        SELECT id, actor_user_id, actor_api_key_id, action, resource_kind,
               resource_id, payload, ip_addr, user_agent, ts
        FROM audit_log
        WHERE id > $1
        ORDER BY id ASC
        LIMIT $2
        "#,
        after_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    fn entry<'a>(action: &'a str, ip: Option<IpNetwork>) -> NewEntry<'a> {
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn security_insights_aggregates(pool: PgPool) {
        let ip: IpNetwork = "1.2.3.4".parse().unwrap();
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

        let i = security_insights(&pool, 24).await.unwrap();
        assert_eq!(i.failed_logins, 2);
        assert_eq!(i.successful_logins, 1);
        assert_eq!(i.totp_failures, 1);
        assert_eq!(i.top_ips.len(), 1);
        assert_eq!(i.top_ips[0].ip, "1.2.3.4");
        assert_eq!(i.top_ips[0].count, 2);
        assert!(!i.by_hour.is_empty());
    }
}
