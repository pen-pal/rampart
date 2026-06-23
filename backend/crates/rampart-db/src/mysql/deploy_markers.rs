//! MySQL `deploy_markers` domain — deploy-timeline annotations: create /
//! list_window / delete. Ported from the PG impl (no SQLite reference).
//!
//! Dialect: uuid→CHAR(36), timestamptz→BIGINT unix-seconds; `COALESCE($2, now())`
//! → `COALESCE(?, UNIX_TIMESTAMP())` with an app-bound optional ts; `make_interval`
//! → a Rust-computed `now - hours*3600` cutoff; no `RETURNING` → INSERT-then-
//! re-select.

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::deploy_marker::{DeployMarker, NewDeployMarker};
use rampart_core::ids::{DeployMarkerId, OrgId};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

fn marker_from(r: &sqlx::mysql::MySqlRow) -> DeployMarker {
    DeployMarker {
        id: DeployMarkerId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        ts: ts(r.get::<i64, _>("ts")),
        title: r.get("title"),
        description: r.get("description"),
        service: r.get("service"),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, ts, title, description, service, created_at";

pub async fn create(
    pool: &MySqlPool,
    input: NewDeployMarker,
    org_id: OrgId,
) -> DbResult<DeployMarker> {
    let id = DeployMarkerId::new();
    sqlx::query(
        "INSERT INTO deploy_markers (id, ts, title, description, service, org_id)
         VALUES (?, COALESCE(?, UNIX_TIMESTAMP()), ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.ts.map(|t| t.unix_timestamp()))
    .bind(input.title)
    .bind(input.description)
    .bind(input.service)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // No RETURNING → re-select by the app-generated id.
    let sql = format!("SELECT {COLS} FROM deploy_markers WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_one(pool)
        .await?;
    Ok(marker_from(&row))
}

/// Markers within the trailing `hours`, newest first; optionally scoped to a
/// service (service-scoped + global/unscoped markers). Org-scoped.
pub async fn list_window(
    pool: &MySqlPool,
    hours: i32,
    service: Option<&str>,
    org_id: OrgId,
) -> DbResult<Vec<DeployMarker>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - hours.clamp(1, 8760) as i64 * 3600;
    let sql = format!(
        "SELECT {COLS} FROM deploy_markers
         WHERE ts > ?
           AND (? IS NULL OR service IS NULL OR service = ?)
           AND org_id = ?
         ORDER BY ts DESC
         LIMIT 500"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(cutoff)
        .bind(service)
        .bind(service)
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(marker_from).collect())
}

pub async fn delete(pool: &MySqlPool, id: DeployMarkerId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM deploy_markers WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_marker(title: &str, service: Option<&str>) -> NewDeployMarker {
        NewDeployMarker {
            ts: None,
            title: title.into(),
            description: Some("deployed".into()),
            service: service.map(|s| s.into()),
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn create_list_filter_delete(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let m = create(&pool, new_marker("v1.2.3", Some("api")), org)
            .await
            .unwrap();
        assert_eq!(m.title, "v1.2.3");
        assert_eq!(m.service.as_deref(), Some("api"));
        // a global (unscoped) marker too.
        create(&pool, new_marker("infra change", None), org)
            .await
            .unwrap();

        // window lists both, newest first.
        let all = list_window(&pool, 24, None, org).await.unwrap();
        assert_eq!(all.len(), 2);

        // service filter returns the api marker + the global one (service IS NULL).
        let api = list_window(&pool, 24, Some("api"), org).await.unwrap();
        assert_eq!(api.len(), 2);
        // a different service → only the global marker matches.
        let db = list_window(&pool, 24, Some("db"), org).await.unwrap();
        assert_eq!(db.len(), 1);
        assert_eq!(db[0].title, "infra change");

        // cross-org isolation.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(list_window(&pool, 24, None, other.id)
            .await
            .unwrap()
            .is_empty());

        // delete + NotFound on re-delete.
        delete(&pool, m.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, m.id, org).await,
            Err(DbError::NotFound)
        ));
        assert_eq!(list_window(&pool, 24, None, org).await.unwrap().len(), 1);
    }
}
