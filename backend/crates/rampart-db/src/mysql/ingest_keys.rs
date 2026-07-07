//! MySQL `ingest_keys` domain — per-org ingest credentials (multi-tenancy
//! Phase 5). Ported from PG. TEXT[] allowed_origins→LONGTEXT(JSON); ts→BIGINT;
//! dual-write token + token_hash so the hash-primary lookup keeps a plaintext
//! fallback. Reuses `crate::api_keys::sha256_hex`.

use super::{raw_uuid, ts};
use crate::ingest_keys::IngestKey;
use crate::DbResult;
use rampart_core::ids::OrgId;
use sqlx::{MySqlPool, Row};
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

fn key_from(r: &sqlx::mysql::MySqlRow) -> IngestKey {
    IngestKey {
        id: raw_uuid(&r.get::<String, _>("id")),
        org_id: super::oid(&r.get::<String, _>("org_id")),
        label: r.get("label"),
        kind: r.get("kind"),
        allowed_origins: origins_of(r.get::<Option<String>, _>("allowed_origins")),
        created_at: ts(r.get::<i64, _>("created_at")),
        last_used_at: r.get::<Option<i64>, _>("last_used_at").map(ts),
    }
}

const COLS: &str = "id, org_id, label, kind, allowed_origins, created_at, last_used_at";

pub async fn create(
    pool: &MySqlPool,
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
    sqlx::query(
        "INSERT INTO ingest_keys (id, org_id, token, token_hash, label, kind, allowed_origins)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(org_id.0.to_string())
    .bind(token.clone())
    .bind(token_hash)
    .bind(label)
    .bind(kind)
    .bind(origins)
    .execute(pool)
    .await?;
    let sql = format!("SELECT {COLS} FROM ingest_keys WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.to_string())
        .fetch_one(pool)
        .await?;
    Ok((key_from(&row), token))
}

/// Resolve a presented token to `(key id, org, allowed_origins)`. Hash-primary
/// with a plaintext fallback (for a key minted by an old build mid-deploy).
pub async fn find_by_token(
    pool: &MySqlPool,
    token: &str,
) -> DbResult<Option<(Uuid, OrgId, String, Vec<String>)>> {
    let hash = crate::api_keys::sha256_hex(token);
    let row = sqlx::query(
        "SELECT id, org_id, kind, allowed_origins FROM ingest_keys WHERE token_hash = ? OR token = ?",
    )
    .bind(hash)
    .bind(token)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        (
            raw_uuid(&r.get::<String, _>("id")),
            super::oid(&r.get::<String, _>("org_id")),
            r.get::<String, _>("kind"),
            origins_of(r.get::<Option<String>, _>("allowed_origins")),
        )
    }))
}

pub async fn touch_last_used(pool: &MySqlPool, id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE ingest_keys SET last_used_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_org(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<IngestKey>> {
    let sql = format!("SELECT {COLS} FROM ingest_keys WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(key_from).collect())
}

pub async fn delete(pool: &MySqlPool, id: Uuid, org_id: OrgId) -> DbResult<bool> {
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

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn create_find_touch_list_delete(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let origins = vec!["https://app.example.com".to_string()];
        let (key, token) = create(&pool, org, "rum", "rum", &origins).await.unwrap();
        assert!(token.starts_with("ingk_"));

        // find_by_token → org + origins round-trip.
        let (id, found_org, got) = find_by_token(&pool, &token).await.unwrap().unwrap();
        assert_eq!(id, key.id);
        assert_eq!(found_org, org);
        assert_eq!(got, origins);
        // unknown → None (caller falls back to the legacy gate).
        assert!(find_by_token(&pool, "ingk_nope").await.unwrap().is_none());

        touch_last_used(&pool, key.id).await.unwrap();
        let listed = list_for_org(&pool, org).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].last_used_at.is_some());
        assert_eq!(listed[0].allowed_origins, origins);

        assert!(delete(&pool, key.id, org).await.unwrap());
        assert!(!delete(&pool, key.id, org).await.unwrap());
        assert!(find_by_token(&pool, &token).await.unwrap().is_none());
    }
}
