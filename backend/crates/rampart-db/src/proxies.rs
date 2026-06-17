//! Proxy repository.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::proxy::{NewProxy, Proxy};
use rampart_core::ProxyId;
use time::OffsetDateTime;
use uuid::Uuid;

struct ProxyRow {
    id: Uuid,
    protocol: String,
    host: String,
    port: i32,
    auth: bool,
    username: Option<String>,
    password: Option<String>,
    active: bool,
    created_at: OffsetDateTime,
}

impl From<ProxyRow> for Proxy {
    fn from(r: ProxyRow) -> Self {
        Proxy {
            id: ProxyId::from_uuid(r.id),
            protocol: r.protocol,
            host: r.host,
            port: r.port,
            auth: r.auth,
            username: r.username,
            password: r.password,
            active: r.active,
            created_at: r.created_at,
        }
    }
}

/// Proxies owned by one org — the management list (org-scoped).
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<Proxy>> {
    let rows = sqlx::query_as!(
        ProxyRow,
        r#"
        SELECT id, protocol, host, port, auth, username, password, active, created_at
        FROM proxies
        WHERE org_id = $1
        ORDER BY created_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Fetch one proxy scoped to an org (cross-org id → NotFound). Request path.
pub async fn get(pool: &DbPool, id: ProxyId, org_id: OrgId) -> DbResult<Proxy> {
    let row = sqlx::query_as!(
        ProxyRow,
        r#"
        SELECT id, protocol, host, port, auth, username, password, active, created_at
        FROM proxies
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Fetch one proxy WITHOUT org scoping — for the probe path (scheduler /
/// test-now routing a check through a monitor's proxy, no request context).
/// Not for request handlers that take a user-supplied id.
pub async fn get_unscoped(pool: &DbPool, id: ProxyId) -> DbResult<Proxy> {
    let row = sqlx::query_as!(
        ProxyRow,
        r#"
        SELECT id, protocol, host, port, auth, username, password, active, created_at
        FROM proxies
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewProxy) -> DbResult<Proxy> {
    let id = ProxyId::new();
    let auth = input.username.is_some() || input.password.is_some();
    let row = sqlx::query_as!(
        ProxyRow,
        r#"
        INSERT INTO proxies (id, protocol, host, port, auth, username, password, active)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, protocol, host, port, auth, username, password, active, created_at
        "#,
        id.0,
        input.protocol,
        input.host,
        input.port,
        auth,
        input.username,
        input.password,
        input.active,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn delete(pool: &DbPool, id: ProxyId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM proxies WHERE id = $1 AND org_id = $2",
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

pub async fn set_active(pool: &DbPool, id: ProxyId, active: bool, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "UPDATE proxies SET active = $1 WHERE id = $2 AND org_id = $3",
        active,
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
