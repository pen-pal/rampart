//! Per-org ingest credentials (multi-tenancy Phase 5).
//!
//! Generalizes the per-status-page [`crate::ingest_tokens`] to per-ORG keys: a
//! telemetry client (OTLP / Prometheus remote_write / RUM / profiles) presents
//! the key in the Bearer / X-Rampart-Token / `?k` slot and the ingest path
//! resolves the owning org from it via `resolve_ingest_org` (rampart-api). The
//! token is an opaque random string stored verbatim — same rationale as
//! `ingest_tokens` — looked up on the UNIQUE index. With no key minted, ingest
//! falls back to the legacy global-token gate (Default org), so single-org
//! installs are behaviour-identical.

use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
use time::OffsetDateTime;
use uuid::Uuid;

const TOKEN_PREFIX: &str = "ingk_";
const TOKEN_BODY_LEN: usize = 40;
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let body: String = (0..TOKEN_BODY_LEN)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect();
    format!("{TOKEN_PREFIX}{body}")
}

/// Metadata for an ingest key (never carries the plaintext token after create).
#[derive(Debug, Clone, serde::Serialize)]
pub struct IngestKey {
    pub id: Uuid,
    pub org_id: OrgId,
    pub label: String,
    pub kind: String,
    pub allowed_origins: Vec<String>,
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
}

struct KeyRow {
    id: Uuid,
    org_id: Uuid,
    label: String,
    kind: String,
    allowed_origins: Option<Vec<String>>,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

impl From<KeyRow> for IngestKey {
    fn from(r: KeyRow) -> Self {
        IngestKey {
            id: r.id,
            org_id: OrgId::from_uuid(r.org_id),
            label: r.label,
            kind: r.kind,
            allowed_origins: r.allowed_origins.unwrap_or_default(),
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        }
    }
}

/// Mint a new ingest key for `org`. Returns the metadata + the plaintext token
/// (shown to the operator ONCE — `list_for_org` never returns it again).
pub async fn create(
    pool: &DbPool,
    org_id: OrgId,
    label: &str,
    kind: &str,
    allowed_origins: &[String],
) -> DbResult<(IngestKey, String)> {
    let id = Uuid::now_v7();
    let token = generate_token();
    // Dual-write (Phase A): store the SHA-256 hash for the new hash-based lookup
    // AND the plaintext, so a rollback to the previous build (which reads
    // `WHERE token = $1`) still resolves this key. The plaintext column is
    // dropped in a later migration once every node reads the hash.
    let token_hash = crate::api_keys::sha256_hex(&token);
    let origins: Option<&[String]> = if allowed_origins.is_empty() {
        None
    } else {
        Some(allowed_origins)
    };
    let row = sqlx::query_as!(
        KeyRow,
        r#"
        INSERT INTO ingest_keys (id, org_id, token, token_hash, label, kind, allowed_origins)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, org_id, label, kind, allowed_origins, created_at, last_used_at
        "#,
        id,
        org_id.0,
        token,
        token_hash,
        label,
        kind,
        origins as Option<&[String]>,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.into(), token))
}

/// Resolve a presented token to its `(key id, org, allowed_origins)`. `None`
/// when the token isn't an ingest key — the caller then falls back to the
/// legacy global-token gate (Default org).
pub async fn find_by_token(
    pool: &DbPool,
    token: &str,
) -> DbResult<Option<(Uuid, OrgId, Vec<String>)>> {
    // Hash-primary lookup with a plaintext fallback (Phase A): the fallback
    // resolves a key created by an OLD build mid-rolling-deploy (it wrote only
    // `token`, no `token_hash`). Both columns are UNIQUE so this returns at most
    // one row. The fallback (and the plaintext column) go away in the Phase-D
    // migration once no old build remains.
    let hash = crate::api_keys::sha256_hex(token);
    let row = sqlx::query!(
        r#"SELECT id, org_id, allowed_origins FROM ingest_keys
           WHERE token_hash = $1 OR token = $2"#,
        hash,
        token,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        (
            r.id,
            OrgId::from_uuid(r.org_id),
            r.allowed_origins.unwrap_or_default(),
        )
    }))
}

/// Fire-and-forget last-used bump on every accepted ingest POST.
pub async fn touch_last_used(pool: &DbPool, id: Uuid) -> DbResult<()> {
    sqlx::query!("UPDATE ingest_keys SET last_used_at = NOW() WHERE id = $1", id)
        .execute(pool)
        .await?;
    Ok(())
}

/// All ingest keys for an org (metadata only — no token), oldest first.
pub async fn list_for_org(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<IngestKey>> {
    let rows = sqlx::query_as!(
        KeyRow,
        r#"
        SELECT id, org_id, label, kind, allowed_origins, created_at, last_used_at
        FROM ingest_keys WHERE org_id = $1 ORDER BY created_at
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Revoke a key. Org-scoped so an admin can only delete their own org's keys.
/// Returns `true` if a row was removed.
pub async fn delete(pool: &DbPool, id: Uuid, org_id: OrgId) -> DbResult<bool> {
    let r = sqlx::query!(
        "DELETE FROM ingest_keys WHERE id = $1 AND org_id = $2",
        id,
        org_id.0,
    )
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::org::DEFAULT_ORG_ID;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_find_touch_list_delete(pool: PgPool) {
        let org = OrgId::from_uuid(DEFAULT_ORG_ID);
        let (key, token) = create(&pool, org, "prod collector", "all", &[])
            .await
            .unwrap();
        assert!(token.starts_with("ingk_"));

        // find_by_token resolves to the org.
        let (id, found_org, origins) = find_by_token(&pool, &token).await.unwrap().unwrap();
        assert_eq!(id, key.id);
        assert_eq!(found_org, org);
        assert!(origins.is_empty());

        // unknown token -> None (caller falls back to the legacy gate).
        assert!(find_by_token(&pool, "ingk_nope").await.unwrap().is_none());

        touch_last_used(&pool, key.id).await.unwrap();
        let listed = list_for_org(&pool, org).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].last_used_at.is_some());

        assert!(delete(&pool, key.id, org).await.unwrap());
        assert!(!delete(&pool, key.id, org).await.unwrap()); // already gone
        assert!(find_by_token(&pool, &token).await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn allowed_origins_round_trip(pool: PgPool) {
        let org = OrgId::from_uuid(DEFAULT_ORG_ID);
        let origins = vec!["https://app.example.com".to_string()];
        let (key, token) = create(&pool, org, "rum", "rum", &origins).await.unwrap();
        let (_, _, got) = find_by_token(&pool, &token).await.unwrap().unwrap();
        assert_eq!(got, origins);
        let listed = list_for_org(&pool, org).await.unwrap();
        assert_eq!(listed[0].allowed_origins, origins);
        assert_eq!(listed[0].id, key.id);
    }
}
