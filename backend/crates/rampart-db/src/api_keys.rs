//! API-key repository.
//!
//! Tokens are generated as `rmp_<32 url-safe random chars>`. Only the
//! SHA-256 hash lives in the DB. Lookup is single-shot: hash the bearer
//! token, hit the unique index on `key_hash`. No timing-attack surface
//! beyond what a UNIQUE index already exposes.

use crate::{DbError, DbPool, DbResult};
use rampart_core::api_key::{ApiKey, IssuedApiKey, KeyScope, NewApiKey};
use rampart_core::ids::{ApiKeyId, OrgId, UserId};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

const TOKEN_PREFIX: &str = "rmp_";
const TOKEN_BODY_LEN: usize = 32;
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

struct KeyRow {
    id: Uuid,
    name: String,
    key_prefix: String,
    scope: String,
    created_by: Option<Uuid>,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
    expires_at: Option<OffsetDateTime>,
    rate_limit_per_hour: i32,
}

impl From<KeyRow> for ApiKey {
    fn from(r: KeyRow) -> Self {
        ApiKey {
            id: ApiKeyId::from_uuid(r.id),
            name: r.name,
            key_prefix: r.key_prefix,
            scope: KeyScope::from_db(&r.scope),
            created_by: r.created_by.map(UserId::from_uuid),
            created_at: r.created_at,
            last_used_at: r.last_used_at,
            expires_at: r.expires_at,
            rate_limit_per_hour: r.rate_limit_per_hour,
        }
    }
}

/// API keys owned by one org — the management list (org-scoped). The
/// bearer-token resolver [`lookup`] stays unscoped: it IS the auth path and
/// can't know the org before resolving the key.
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<ApiKey>> {
    let rows = sqlx::query_as!(
        KeyRow,
        r#"
        SELECT id, name, key_prefix, scope, created_by,
               created_at, last_used_at, expires_at, rate_limit_per_hour
        FROM api_keys
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn create(pool: &DbPool, input: NewApiKey, created_by: UserId) -> DbResult<IssuedApiKey> {
    let token = generate_token();
    let hash = sha256_hex(&token);
    let prefix = token[..(TOKEN_PREFIX.len() + 8)].to_string();
    let id = ApiKeyId::new();

    let row = sqlx::query_as!(
        KeyRow,
        r#"
        INSERT INTO api_keys (id, name, key_hash, key_prefix, scope, scopes, created_by, expires_at, rate_limit_per_hour)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, name, key_prefix, scope, created_by,
                  created_at, last_used_at, expires_at, rate_limit_per_hour
        "#,
        id.0,
        input.name,
        hash,
        prefix,
        input.scope.as_str(),
        // `scopes` (the legacy array) is a deprecated rollback shim kept one
        // release; it's NOT NULL with no default, so we satisfy it with an
        // empty array. `scope` is authoritative.
        &Vec::<String>::new(),
        created_by.0,
        input.expires_at,
        input.rate_limit_per_hour,
    )
    .fetch_one(pool)
    .await?;

    Ok(IssuedApiKey {
        key: row.into(),
        token,
    })
}

pub async fn delete(pool: &DbPool, id: ApiKeyId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM api_keys WHERE id = $1 AND org_id = $2",
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

/// Resolve a raw bearer token to (key, created_by). Returns NotFound for
/// any of: unknown hash, expired, no `created_by` (orphan key — shouldn't
/// happen but we defend against it).
pub async fn lookup(pool: &DbPool, token: &str) -> DbResult<(ApiKey, UserId)> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Err(DbError::NotFound);
    }
    let hash = sha256_hex(token);
    let row = sqlx::query_as!(
        KeyRow,
        r#"
        SELECT id, name, key_prefix, scope, created_by,
               created_at, last_used_at, expires_at, rate_limit_per_hour
        FROM api_keys
        WHERE key_hash = $1
          AND (expires_at IS NULL OR expires_at > NOW())
        "#,
        hash,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;

    let created_by = row.created_by.ok_or(DbError::NotFound)?;
    Ok((row.into(), UserId::from_uuid(created_by)))
}

/// Fire-and-forget last-used bump. Called on every authenticated request
/// — failures swallowed since they're not critical to the request itself.
pub async fn touch_last_used(pool: &DbPool, id: ApiKeyId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE api_keys SET last_used_at = NOW() WHERE id = $1",
        id.0,
    )
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

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}
