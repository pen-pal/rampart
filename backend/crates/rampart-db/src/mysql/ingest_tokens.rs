//! MySQL `ingest_tokens` domain — page-scoped inbound webhook credentials.
//! Ported from PG.
//!
//! Dialect: uuid→CHAR(36), ts→BIGINT, mapping jsonb→LONGTEXT (serde round-trip).
//! No `RETURNING` → INSERT-then-re-select. Dual-write token + token_hash;
//! `find_by_token` is hash-primary with a plaintext fallback (reuses
//! `crate::api_keys::sha256_hex` so hashes match the PG store). Org tenanting
//! flows through the owning status page (the gate is an `IN (SELECT … org_id)`
//! subquery, same as PG). `set_mapping` re-selects through the org gate to
//! distinguish a real miss from a no-op UPDATE (MySQL counts CHANGED rows).

use super::{raw_uuid, ts};
use crate::api_keys::sha256_hex;
use crate::{DbError, DbResult};
use rampart_core::ids::{IngestTokenId, OrgId, StatusPageId};
use rampart_core::ingest_token::{IngestToken, NewIngestToken};
use sqlx::{MySqlPool, Row};

const COLS: &str = "id, status_page_id, token, label, mapping, created_at, last_used_at";

fn token_from(r: &sqlx::mysql::MySqlRow) -> IngestToken {
    IngestToken {
        id: IngestTokenId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        status_page_id: StatusPageId::from_uuid(raw_uuid(&r.get::<String, _>("status_page_id"))),
        token: r.get("token"),
        label: r.get("label"),
        mapping: r
            .get::<Option<String>, _>("mapping")
            .and_then(|s| serde_json::from_str(&s).ok()),
        created_at: ts(r.get::<i64, _>("created_at")),
        last_used_at: r.get::<Option<i64>, _>("last_used_at").map(ts),
    }
}

async fn get_by_id(pool: &MySqlPool, id: IngestTokenId) -> DbResult<IngestToken> {
    let sql = format!("SELECT {COLS} FROM ingest_tokens WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(token_from(&row))
}

pub async fn create(
    pool: &MySqlPool,
    page: StatusPageId,
    input: NewIngestToken,
) -> DbResult<IngestToken> {
    let id = IngestTokenId::new();
    let token = generate_token();
    insert(
        pool,
        id,
        page,
        &token,
        input.label.as_deref(),
        input.mapping,
    )
    .await?;
    get_by_id(pool, id).await
}

pub async fn create_with_token(
    pool: &MySqlPool,
    page: StatusPageId,
    label: &str,
    token: &str,
) -> DbResult<IngestToken> {
    let id = IngestTokenId::new();
    insert(pool, id, page, token, Some(label), None).await?;
    get_by_id(pool, id).await
}

async fn insert(
    pool: &MySqlPool,
    id: IngestTokenId,
    page: StatusPageId,
    token: &str,
    label: Option<&str>,
    mapping: Option<serde_json::Value>,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO ingest_tokens (id, status_page_id, token, token_hash, label, mapping)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(page.0.to_string())
    .bind(token)
    .bind(sha256_hex(token))
    .bind(label)
    .bind(mapping.map(|v| v.to_string()))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_mapping(
    pool: &MySqlPool,
    id: IngestTokenId,
    mapping: Option<serde_json::Value>,
    org_id: OrgId,
) -> DbResult<IngestToken> {
    sqlx::query(
        "UPDATE ingest_tokens SET mapping = ?
         WHERE id = ? AND status_page_id IN (SELECT id FROM status_pages WHERE org_id = ?)",
    )
    .bind(mapping.map(|v| v.to_string()))
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // Re-select through the org gate — None covers both a missing id and a
    // cross-org one (and avoids a false NotFound on a same-value no-op UPDATE).
    let sql = format!(
        "SELECT {COLS} FROM ingest_tokens
         WHERE id = ? AND status_page_id IN (SELECT id FROM status_pages WHERE org_id = ?)"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(token_from(&row))
}

pub async fn list_for_page(pool: &MySqlPool, page: StatusPageId) -> DbResult<Vec<IngestToken>> {
    let sql = format!(
        "SELECT {COLS} FROM ingest_tokens WHERE status_page_id = ? ORDER BY created_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(page.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(token_from).collect())
}

pub async fn find_by_token(pool: &MySqlPool, token: &str) -> DbResult<IngestToken> {
    let sql = format!("SELECT {COLS} FROM ingest_tokens WHERE token_hash = ? OR token = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(sha256_hex(token))
        .bind(token)
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(token_from(&row))
}

pub async fn delete(pool: &MySqlPool, id: IngestTokenId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query(
        "DELETE FROM ingest_tokens
         WHERE id = ? AND status_page_id IN (SELECT id FROM status_pages WHERE org_id = ?)",
    )
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn touch_last_used(pool: &MySqlPool, id: IngestTokenId) -> DbResult<()> {
    sqlx::query("UPDATE ingest_tokens SET last_used_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

const TOKEN_PREFIX: &str = "ing_";
const TOKEN_BODY_LEN: usize = 40;

fn generate_token() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
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

    async fn seed_page(pool: &MySqlPool, org: OrgId, slug: &str) -> StatusPageId {
        let id = StatusPageId::new();
        sqlx::query("INSERT INTO status_pages (id, slug, title, org_id) VALUES (?, ?, ?, ?)")
            .bind(id.0.to_string())
            .bind(slug)
            .bind("P")
            .bind(org.0.to_string())
            .execute(pool)
            .await
            .unwrap();
        id
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn create_lookup_mapping_delete(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let page = seed_page(&pool, org, "p").await;

        let t = create(
            &pool,
            page,
            NewIngestToken {
                label: Some("alertmanager".into()),
                mapping: None,
            },
        )
        .await
        .unwrap();
        assert!(t.token.starts_with("ing_"));
        assert!(t.mapping.is_none());
        assert_eq!(list_for_page(&pool, page).await.unwrap().len(), 1);

        // lookup by raw token (hash-primary).
        assert_eq!(find_by_token(&pool, &t.token).await.unwrap().id, t.id);
        assert!(matches!(
            find_by_token(&pool, "ing_nope").await,
            Err(DbError::NotFound)
        ));

        // set mapping (org-gated), then a same-value re-set must not 404.
        let m = serde_json::json!({"down": "monitor_down"});
        let u = set_mapping(&pool, t.id, Some(m.clone()), org)
            .await
            .unwrap();
        assert_eq!(u.mapping, Some(m.clone()));
        set_mapping(&pool, t.id, Some(m), org).await.unwrap();

        // deterministic create_with_token + duplicate conflict.
        let d = create_with_token(&pool, page, "demo", "ing_fixed123")
            .await
            .unwrap();
        assert_eq!(d.token, "ing_fixed123");
        assert!(create_with_token(&pool, page, "dup", "ing_fixed123")
            .await
            .is_err());

        touch_last_used(&pool, t.id).await.unwrap();
        assert!(get_by_id(&pool, t.id).await.unwrap().last_used_at.is_some());

        // cross-org gate on mapping + delete.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            set_mapping(&pool, t.id, None, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            delete(&pool, t.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, t.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, t.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
