//! MySQL `proxies` domain — outbound proxy configs for probe routing. Mirrors
//! the PG/SQLite surface. MySQL deltas: no `RETURNING` (insert-then-get);
//! bool→TINYINT (read as i64); ts→BIGINT. `auth` derived from username/password
//! on create; password stored verbatim (no DB-layer sealing, same as PG).

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::proxy::{NewProxy, Proxy};
use rampart_core::ProxyId;
use sqlx::{MySqlPool, Row};

fn proxy_from(r: &sqlx::mysql::MySqlRow) -> Proxy {
    Proxy {
        id: ProxyId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        protocol: r.get("protocol"),
        host: r.get("host"),
        port: r.get("port"),
        auth: r.get::<i64, _>("auth") != 0,
        username: r.get("username"),
        password: r.get("password"),
        active: r.get::<i64, _>("active") != 0,
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, protocol, host, port, auth, username, password, active, created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Proxy>> {
    let sql = format!("SELECT {COLS} FROM proxies WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(proxy_from).collect())
}

pub async fn get(pool: &MySqlPool, id: ProxyId, org_id: OrgId) -> DbResult<Proxy> {
    let sql = format!("SELECT {COLS} FROM proxies WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(proxy_from(&row))
}

pub async fn get_unscoped(pool: &MySqlPool, id: ProxyId) -> DbResult<Proxy> {
    let sql = format!("SELECT {COLS} FROM proxies WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(proxy_from(&row))
}

pub async fn create(pool: &MySqlPool, input: NewProxy, org_id: OrgId) -> DbResult<Proxy> {
    let id = ProxyId::new();
    let auth = input.username.is_some() || input.password.is_some();
    sqlx::query(
        "INSERT INTO proxies (id, protocol, host, port, auth, username, password, active, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.protocol)
    .bind(input.host)
    .bind(input.port)
    .bind(auth as i64)
    .bind(input.username)
    .bind(input.password)
    .bind(input.active as i64)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn delete(pool: &MySqlPool, id: ProxyId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM proxies WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_active(
    pool: &MySqlPool,
    id: ProxyId,
    active: bool,
    org_id: OrgId,
) -> DbResult<()> {
    let r = sqlx::query("UPDATE proxies SET active = ? WHERE id = ? AND org_id = ?")
        .bind(active as i64)
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 && get(pool, id, org_id).await.is_err() {
        return Err(DbError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_proxy() -> NewProxy {
        NewProxy {
            protocol: "socks5".into(),
            host: "10.0.0.1".into(),
            port: 1080,
            username: Some("u".into()),
            password: Some("p".into()),
            active: true,
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_and_active(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let p = create(&pool, new_proxy(), org).await.unwrap();
        assert!(p.auth, "auth derived from username/password");
        assert_eq!(p.protocol, "socks5");
        assert_eq!(p.port, 1080);

        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, p.id, org).await.unwrap().host, "10.0.0.1");
        assert_eq!(get_unscoped(&pool, p.id).await.unwrap().id, p.id);

        set_active(&pool, p.id, false, org).await.unwrap();
        assert!(!get(&pool, p.id, org).await.unwrap().active);

        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            delete(&pool, p.id, other.id).await,
            Err(DbError::NotFound)
        ));

        delete(&pool, p.id, org).await.unwrap();
        assert!(matches!(
            get(&pool, p.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
