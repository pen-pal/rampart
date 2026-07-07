//! SQLite `ingest_keys` domain — per-org ingest credentials (multi-tenancy
//! Phase 5). Ported from PG/MySQL. Dialect: uuid→TEXT, ts→INTEGER unix-seconds,
//! TEXT[] allowed_origins→JSON TEXT; dual-write token + token_hash so the
//! hash-primary lookup keeps a plaintext fallback. Reuses
//! `crate::api_keys::sha256_hex`.

use super::{oid, raw_uuid, ts};
use crate::ingest_keys::IngestKey;
use crate::DbResult;
use rampart_core::ids::OrgId;
use sqlx::SqlitePool;
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

fn origins_of(s: Option<String>) -> Vec<String> {
    s.and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// (id, org_id, label, kind, allowed_origins, created_at, last_used_at)
type KeyRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    Option<i64>,
);

fn key_from(
    (id, org_id, label, kind, allowed_origins, created_at, last_used_at): KeyRow,
) -> IngestKey {
    IngestKey {
        id: raw_uuid(&id),
        org_id: oid(&org_id),
        label,
        kind,
        allowed_origins: origins_of(allowed_origins),
        created_at: ts(created_at),
        last_used_at: last_used_at.map(ts),
    }
}

pub async fn create(
    pool: &SqlitePool,
    org_id: OrgId,
    label: &str,
    kind: &str,
    allowed_origins: &[String],
) -> DbResult<(IngestKey, String)> {
    let id = Uuid::now_v7();
    let token = generate_token();
    let token_hash = crate::api_keys::sha256_hex(&token);
    let origins: Option<String> = if allowed_origins.is_empty() {
        None
    } else {
        Some(serde_json::to_string(allowed_origins).unwrap_or_else(|_| "[]".into()))
    };
    let row: KeyRow = sqlx::query_as(
        "INSERT INTO ingest_keys (id, org_id, token, token_hash, label, kind, allowed_origins)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id, org_id, label, kind, allowed_origins, created_at, last_used_at",
    )
    .bind(id.to_string())
    .bind(org_id.0.to_string())
    .bind(&token)
    .bind(token_hash)
    .bind(label)
    .bind(kind)
    .bind(origins)
    .fetch_one(pool)
    .await?;
    Ok((key_from(row), token))
}

/// Resolve a presented token to `(key id, org, kind, allowed_origins)`. Hash-
/// primary with a plaintext fallback (for a key minted by an old build). `None`
/// when the token isn't an ingest key — the caller falls back to the legacy gate.
pub async fn find_by_token(
    pool: &SqlitePool,
    token: &str,
) -> DbResult<Option<(Uuid, OrgId, String, Vec<String>)>> {
    let hash = crate::api_keys::sha256_hex(token);
    let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, org_id, kind, allowed_origins FROM ingest_keys
         WHERE token_hash = ? OR token = ?",
    )
    .bind(hash)
    .bind(token)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, org_id, kind, origins)| {
        (raw_uuid(&id), oid(&org_id), kind, origins_of(origins))
    }))
}

pub async fn touch_last_used(pool: &SqlitePool, id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE ingest_keys SET last_used_at = unixepoch() WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_org(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<IngestKey>> {
    let rows: Vec<KeyRow> = sqlx::query_as(
        "SELECT id, org_id, label, kind, allowed_origins, created_at, last_used_at
         FROM ingest_keys WHERE org_id = ? ORDER BY created_at",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(key_from).collect())
}

pub async fn delete(pool: &SqlitePool, id: Uuid, org_id: OrgId) -> DbResult<bool> {
    let r = sqlx::query("DELETE FROM ingest_keys WHERE id = ? AND org_id = ?")
        .bind(id.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_find_touch_list_delete(pool: SqlitePool) {
        let org = oid(DEF);
        let origins = vec!["https://app.example.com".to_string()];
        let (key, token) = create(&pool, org, "rum", "rum", &origins).await.unwrap();
        assert!(token.starts_with("ingk_"));

        // find_by_token → org + kind + origins round-trip.
        let (id, found_org, kind, got) = find_by_token(&pool, &token).await.unwrap().unwrap();
        assert_eq!(id, key.id);
        assert_eq!(found_org, org);
        assert_eq!(kind, "rum");
        assert_eq!(got, origins);
        // unknown → None (caller falls back to the legacy gate).
        assert!(find_by_token(&pool, "ingk_nope").await.unwrap().is_none());

        touch_last_used(&pool, key.id).await.unwrap();
        let listed = list_for_org(&pool, org).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].last_used_at.is_some());
        assert_eq!(listed[0].allowed_origins, origins);

        assert!(delete(&pool, key.id, org).await.unwrap());
        assert!(!delete(&pool, key.id, org).await.unwrap()); // already gone
        assert!(find_by_token(&pool, &token).await.unwrap().is_none());
    }
}
