//! MySQL `api_keys` domain — bearer API keys (SHA-256-hashed; lookup on the
//! unique key_hash). Ported from PG. ts→BIGINT; no `RETURNING` → INSERT-then-
//! re-select; the legacy `scopes` array is dropped (`scope` is authoritative).
//! Reuses `crate::api_keys::sha256_hex` so hashes match the PG/ingest stores.

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::api_key::{ApiKey, IssuedApiKey, KeyScope, NewApiKey};
use rampart_core::ids::{ApiKeyId, OrgId, UserId};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

const TOKEN_PREFIX: &str = "rmp_";
const TOKEN_BODY_LEN: usize = 32;
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn key_from(r: &sqlx::mysql::MySqlRow) -> ApiKey {
    ApiKey {
        id: ApiKeyId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        key_prefix: r.get("key_prefix"),
        scope: KeyScope::from_db(&r.get::<String, _>("scope")),
        created_by: r
            .get::<Option<String>, _>("created_by")
            .map(|s| UserId::from_uuid(raw_uuid(&s))),
        created_at: ts(r.get::<i64, _>("created_at")),
        last_used_at: r.get::<Option<i64>, _>("last_used_at").map(ts),
        expires_at: r.get::<Option<i64>, _>("expires_at").map(ts),
        rate_limit_per_hour: r.get::<i32, _>("rate_limit_per_hour"),
    }
}

const COLS: &str = "id, name, key_prefix, scope, created_by, created_at, last_used_at, \
     expires_at, rate_limit_per_hour";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<ApiKey>> {
    let sql = format!("SELECT {COLS} FROM api_keys WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(key_from).collect())
}

pub async fn create(
    pool: &MySqlPool,
    input: NewApiKey,
    created_by: UserId,
    org_id: OrgId,
) -> DbResult<IssuedApiKey> {
    let token = generate_token();
    let hash = crate::api_keys::sha256_hex(&token);
    let prefix = token[..(TOKEN_PREFIX.len() + 8)].to_string();
    let id = ApiKeyId::new();
    sqlx::query(
        "INSERT INTO api_keys
            (id, name, key_hash, key_prefix, scope, created_by, expires_at, rate_limit_per_hour, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(hash)
    .bind(prefix)
    .bind(input.scope.as_str())
    .bind(created_by.0.to_string())
    .bind(input.expires_at.map(|t| t.unix_timestamp()))
    .bind(input.rate_limit_per_hour)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    let sql = format!("SELECT {COLS} FROM api_keys WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_one(pool)
        .await?;
    Ok(IssuedApiKey {
        key: key_from(&row),
        token,
    })
}

pub async fn delete(pool: &MySqlPool, id: ApiKeyId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM api_keys WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Resolve a bearer token to (key, created_by, org). NotFound for unknown hash,
/// expired, or orphan (no created_by). The org is the key's own owning org.
pub async fn lookup(pool: &MySqlPool, token: &str) -> DbResult<(ApiKey, UserId, OrgId)> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Err(DbError::NotFound);
    }
    let hash = crate::api_keys::sha256_hex(token);
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let sql = format!(
        "SELECT {COLS}, org_id FROM api_keys
         WHERE key_hash = ? AND (expires_at IS NULL OR expires_at > ?)"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(hash)
        .bind(now)
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    let org_id = super::oid(&row.get::<String, _>("org_id"));
    let key = key_from(&row);
    let created_by = key.created_by.ok_or(DbError::NotFound)?;
    Ok((key, created_by, org_id))
}

/// Fire-and-forget last-used bump on every authenticated request.
pub async fn touch_last_used(pool: &MySqlPool, id: ApiKeyId) -> DbResult<()> {
    sqlx::query("UPDATE api_keys SET last_used_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let body: String = (0..TOKEN_BODY_LEN)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect();
    format!("{TOKEN_PREFIX}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn create_lookup_delete(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let creator = UserId::new();
        let input: NewApiKey = serde_json::from_value(serde_json::json!({
            "name": "ci key", "scope": "read", "rate_limit_per_hour": 1000
        }))
        .unwrap();
        let issued = create(&pool, input, creator, org).await.unwrap();
        assert!(issued.token.starts_with("rmp_"));
        assert_eq!(issued.key.rate_limit_per_hour, 1000);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // bearer lookup resolves to (key, creator, org).
        let (k, by, found_org) = lookup(&pool, &issued.token).await.unwrap();
        assert_eq!(k.id, issued.key.id);
        assert_eq!(by, creator);
        assert_eq!(found_org, org);
        // wrong prefix / unknown → NotFound.
        assert!(matches!(
            lookup(&pool, "nope").await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            lookup(&pool, "rmp_unknownxxx").await,
            Err(DbError::NotFound)
        ));

        touch_last_used(&pool, issued.key.id).await.unwrap();
        assert!(list(&pool, org).await.unwrap()[0].last_used_at.is_some());

        // cross-org delete isolation.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            delete(&pool, issued.key.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, issued.key.id, org).await.unwrap();
        assert!(matches!(
            lookup(&pool, &issued.token).await,
            Err(DbError::NotFound)
        ));
    }
}
