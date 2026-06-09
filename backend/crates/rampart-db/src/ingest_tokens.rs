//! Ingest-token repository.
//!
//! Page-scoped inbound webhook credentials. The token is an opaque random
//! string generated server-side and stored verbatim — see
//! `rampart_core::ingest_token` for why it isn't hashed. Lookup is a single
//! hit on the UNIQUE index on `token`.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{IngestTokenId, StatusPageId};
use rampart_core::ingest_token::{IngestToken, NewIngestToken};
use time::OffsetDateTime;
use uuid::Uuid;

const TOKEN_PREFIX: &str = "ing_";
const TOKEN_BODY_LEN: usize = 40;
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

struct TokenRow {
    id: Uuid,
    status_page_id: Uuid,
    token: String,
    label: Option<String>,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

impl From<TokenRow> for IngestToken {
    fn from(r: TokenRow) -> Self {
        IngestToken {
            id: IngestTokenId::from_uuid(r.id),
            status_page_id: StatusPageId::from_uuid(r.status_page_id),
            token: r.token,
            label: r.label,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        }
    }
}

pub async fn create(
    pool: &DbPool,
    page: StatusPageId,
    input: NewIngestToken,
) -> DbResult<IngestToken> {
    let id = IngestTokenId::new();
    let token = generate_token();
    let row = sqlx::query_as!(
        TokenRow,
        r#"
        INSERT INTO ingest_tokens (id, status_page_id, token, label)
        VALUES ($1, $2, $3, $4)
        RETURNING id, status_page_id, token, label, created_at, last_used_at
        "#,
        id.0,
        page.0,
        token,
        input.label,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn list_for_page(pool: &DbPool, page: StatusPageId) -> DbResult<Vec<IngestToken>> {
    let rows = sqlx::query_as!(
        TokenRow,
        r#"
        SELECT id, status_page_id, token, label, created_at, last_used_at
        FROM ingest_tokens
        WHERE status_page_id = $1
        ORDER BY created_at DESC
        "#,
        page.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Resolve a raw token to its record. Returns NotFound for an unknown token.
pub async fn find_by_token(pool: &DbPool, token: &str) -> DbResult<IngestToken> {
    let row = sqlx::query_as!(
        TokenRow,
        r#"
        SELECT id, status_page_id, token, label, created_at, last_used_at
        FROM ingest_tokens
        WHERE token = $1
        "#,
        token,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn delete(pool: &DbPool, id: IngestTokenId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM ingest_tokens WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Fire-and-forget last-used bump. Called on every accepted ingest POST —
/// failures are surfaced to the caller but are not request-critical.
pub async fn touch_last_used(pool: &DbPool, id: IngestTokenId) -> DbResult<()> {
    sqlx::query!(
        "UPDATE ingest_tokens SET last_used_at = NOW() WHERE id = $1",
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
