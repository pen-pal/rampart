//! SQLite `profiles` domain — continuous-profiling storage. Ported from PG/MySQL.
//! Mirrors the free-fn surface: insert / list / folded_in_window / fetch_folded
//! / services / profile_types / prune.
//!
//! Dialect: BIGSERIAL→INTEGER PRIMARY KEY AUTOINCREMENT (no `RETURNING id` →
//! `last_insert_rowid()`), ts→INTEGER unix-seconds (Rust-computed cutoff),
//! JSONB→TEXT (serde round-trip), BYTEA→BLOB. Optional service/type filters use
//! the `(? IS NULL OR col = ?)` bind-twice form.

use super::ts;
use crate::profiles::{NewProfile, ProfileMeta};
use crate::DbResult;
use rampart_core::ids::OrgId;
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;

pub async fn insert(pool: &SqlitePool, p: NewProfile<'_>, org_id: OrgId) -> DbResult<i64> {
    let res = sqlx::query(
        "INSERT INTO profiles
            (service_name, profile_type, period_ns, duration_ns, sample_count, labels, folded, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(p.service_name)
    .bind(p.profile_type)
    .bind(p.period_ns)
    .bind(p.duration_ns)
    .bind(p.sample_count)
    .bind(p.labels.to_string())
    .bind(p.folded_gz)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    Ok(res.last_insert_rowid())
}

fn meta_from(r: &sqlx::sqlite::SqliteRow) -> ProfileMeta {
    ProfileMeta {
        id: r.get::<i64, _>("id"),
        received_at: ts(r.get::<i64, _>("received_at")),
        service_name: r.get("service_name"),
        profile_type: r.get("profile_type"),
        period_ns: r.get::<i64, _>("period_ns"),
        duration_ns: r.get::<i64, _>("duration_ns"),
        sample_count: r.get::<i32, _>("sample_count"),
        labels: serde_json::from_str(&r.get::<String, _>("labels")).unwrap_or_default(),
    }
}

pub async fn list(
    pool: &SqlitePool,
    service: Option<&str>,
    profile_type: Option<&str>,
    hours: i32,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<ProfileMeta>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - hours.max(0) as i64 * 3600;
    let rows = sqlx::query(
        "SELECT id, received_at, service_name, profile_type, period_ns, duration_ns,
                sample_count, labels
         FROM profiles
         WHERE received_at >= ?
           AND (? IS NULL OR service_name = ?)
           AND (? IS NULL OR profile_type = ?)
           AND org_id = ?
         ORDER BY received_at DESC
         LIMIT ?",
    )
    .bind(cutoff)
    .bind(service)
    .bind(service)
    .bind(profile_type)
    .bind(profile_type)
    .bind(org_id.0.to_string())
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(meta_from).collect())
}

pub async fn folded_in_window(
    pool: &SqlitePool,
    service: &str,
    profile_type: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
    org_id: OrgId,
) -> DbResult<Vec<Vec<u8>>> {
    let rows = sqlx::query(
        "SELECT folded FROM profiles
         WHERE service_name = ? AND profile_type = ?
           AND received_at >= ? AND received_at < ? AND org_id = ?
         ORDER BY received_at",
    )
    .bind(service)
    .bind(profile_type)
    .bind(from.unix_timestamp())
    .bind(to.unix_timestamp())
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(|r| r.get::<Vec<u8>, _>("folded")).collect())
}

pub async fn fetch_folded(
    pool: &SqlitePool,
    id: i64,
    org_id: OrgId,
) -> DbResult<Option<(String, Vec<u8>)>> {
    let row = sqlx::query("SELECT profile_type, folded FROM profiles WHERE id = ? AND org_id = ?")
        .bind(id)
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| {
        (
            r.get::<String, _>("profile_type"),
            r.get::<Vec<u8>, _>("folded"),
        )
    }))
}

pub async fn services(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<String>> {
    let rows = sqlx::query(
        "SELECT DISTINCT service_name FROM profiles WHERE org_id = ? ORDER BY service_name",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| r.get::<String, _>("service_name"))
        .collect())
}

pub async fn profile_types(
    pool: &SqlitePool,
    service: Option<&str>,
    org_id: OrgId,
) -> DbResult<Vec<String>> {
    let rows = sqlx::query(
        "SELECT DISTINCT profile_type FROM profiles
         WHERE (? IS NULL OR service_name = ?) AND org_id = ?
         ORDER BY profile_type",
    )
    .bind(service)
    .bind(service)
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| r.get::<String, _>("profile_type"))
        .collect())
}

/// Delete profiles older than `days`. Returns rows removed.
pub async fn prune(pool: &SqlitePool, days: i32) -> DbResult<u64> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - days.max(0) as i64 * 86400;
    super::chunked_delete_older(pool, "profiles", "received_at", cutoff).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn np<'a>(service: &'a str, folded_gz: &'a [u8]) -> NewProfile<'a> {
        NewProfile {
            service_name: service,
            profile_type: "cpu",
            period_ns: 0,
            duration_ns: 0,
            sample_count: 3,
            labels: serde_json::json!({}),
            folded_gz,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn insert_list_window_pickers_prune(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let id1 = insert(&pool, np("api", b"a;b 3\n"), org).await.unwrap();
        insert(&pool, np("api", b"a;c 2\n"), org).await.unwrap();
        assert!(id1 > 0);

        // list — unscoped + scoped + miss
        assert_eq!(
            list(&pool, None, None, 24, 100, org).await.unwrap().len(),
            2
        );
        assert_eq!(
            list(&pool, Some("api"), Some("cpu"), 24, 100, org)
                .await
                .unwrap()
                .len(),
            2
        );
        assert!(list(&pool, Some("nope"), None, 24, 100, org)
            .await
            .unwrap()
            .is_empty());

        // window blobs (the flamegraph merge set)
        let now = OffsetDateTime::now_utc();
        let blobs = folded_in_window(
            &pool,
            "api",
            "cpu",
            now - time::Duration::hours(1),
            now + time::Duration::hours(1),
            org,
        )
        .await
        .unwrap();
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[0], b"a;b 3\n");

        // pickers
        assert_eq!(services(&pool, org).await.unwrap(), vec!["api".to_string()]);
        assert_eq!(
            profile_types(&pool, Some("api"), org).await.unwrap(),
            vec!["cpu".to_string()]
        );

        // single fetch carries the type + bytes
        let (ty, bytes) = fetch_folded(&pool, id1, org).await.unwrap().unwrap();
        assert_eq!(ty, "cpu");
        assert_eq!(bytes, b"a;b 3\n");

        // cross-org isolation
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(list(&pool, None, None, 24, 100, other.id)
            .await
            .unwrap()
            .is_empty());
        assert!(fetch_folded(&pool, id1, other.id).await.unwrap().is_none());

        // prune: fresh rows survive; an aged row is dropped
        assert_eq!(prune(&pool, 7).await.unwrap(), 0);
        let aged = OffsetDateTime::now_utc().unix_timestamp() - 10 * 86400;
        sqlx::query("UPDATE profiles SET received_at = ? WHERE id = ?")
            .bind(aged)
            .bind(id1)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(prune(&pool, 7).await.unwrap(), 1);
    }
}
