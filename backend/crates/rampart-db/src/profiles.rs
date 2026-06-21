//! Profiling storage (migration 0084).
//!
//! One row per ingested profile. `folded` holds the gzipped folded-stack map —
//! the uniform internal form every wire format (folded text, pprof, OTLP
//! profiles) is lowered into before storage. The flamegraph is assembled on
//! read by the API layer: fetch the folded blobs in a window via
//! [`folded_in_window`], gunzip + `rampart_core::profile::parse_folded` each,
//! merge, then `fold_to_tree`. Profiles age out via [`prune`]. See
//! `docs/design/PROFILING.md`.

use crate::{DbPool, DbResult};
use rampart_core::ids::OrgId;
use serde::Serialize;
use time::OffsetDateTime;

/// A profile ready to store. The caller has already lowered the wire format to a
/// folded map, serialized it to canonical text, and gzipped it (`folded_gz`).
pub struct NewProfile<'a> {
    pub service_name: &'a str,
    pub profile_type: &'a str,
    pub period_ns: i64,
    pub duration_ns: i64,
    pub sample_count: i32,
    pub labels: serde_json::Value,
    pub folded_gz: &'a [u8],
}

/// Insert a profile, returning its id.
pub async fn insert(
    pool: &DbPool,
    p: NewProfile<'_>,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<i64> {
    let row = sqlx::query!(
        r#"
        INSERT INTO profiles
            (service_name, profile_type, period_ns, duration_ns, sample_count, labels, folded, org_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
        p.service_name,
        p.profile_type,
        p.period_ns,
        p.duration_ns,
        p.sample_count,
        p.labels,
        p.folded_gz,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// Profile metadata for the list/picker — everything but the folded bytes.
#[derive(Debug, Serialize)]
pub struct ProfileMeta {
    pub id: i64,
    pub received_at: OffsetDateTime,
    pub service_name: String,
    pub profile_type: String,
    pub period_ns: i64,
    pub duration_ns: i64,
    pub sample_count: i32,
    pub labels: serde_json::Value,
}

/// List profiles in the last `hours`, optionally scoped to a service and/or
/// type, newest first.
pub async fn list(
    pool: &DbPool,
    service: Option<&str>,
    profile_type: Option<&str>,
    hours: i32,
    limit: i64,
    org_id: OrgId,
) -> DbResult<Vec<ProfileMeta>> {
    let rows = sqlx::query_as!(
        ProfileMeta,
        r#"
        SELECT
            id,
            received_at,
            service_name,
            profile_type,
            period_ns,
            duration_ns,
            sample_count,
            labels AS "labels!: serde_json::Value"
        FROM profiles
        WHERE received_at >= now() - make_interval(hours => $1)
          AND ($2::text IS NULL OR service_name = $2)
          AND ($3::text IS NULL OR profile_type = $3)
          AND org_id = $5
        ORDER BY received_at DESC
        LIMIT $4
        "#,
        hours,
        service,
        profile_type,
        limit.clamp(1, 500),
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The gzipped folded blobs for one (service, type) in a half-open time window —
/// the set the read path merges into a single flamegraph.
pub async fn folded_in_window(
    pool: &DbPool,
    service: &str,
    profile_type: &str,
    from: OffsetDateTime,
    to: OffsetDateTime,
    org_id: OrgId,
) -> DbResult<Vec<Vec<u8>>> {
    let rows = sqlx::query!(
        r#"
        SELECT folded
        FROM profiles
        WHERE service_name = $1
          AND profile_type = $2
          AND received_at >= $3
          AND received_at <  $4
          AND org_id = $5
        ORDER BY received_at
        "#,
        service,
        profile_type,
        from,
        to,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.folded).collect())
}

/// The gzipped folded blob + type for a single profile (the per-profile
/// flamegraph view).
pub async fn fetch_folded(
    pool: &DbPool,
    id: i64,
    org_id: OrgId,
) -> DbResult<Option<(String, Vec<u8>)>> {
    let row = sqlx::query!(
        r#"SELECT profile_type, folded FROM profiles WHERE id = $1 AND org_id = $2"#,
        id,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.profile_type, r.folded)))
}

/// Distinct service names that have profiles (the service picker).
pub async fn services(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<String>> {
    let rows = sqlx::query_scalar!(
        r#"SELECT DISTINCT service_name AS "service_name!" FROM profiles
           WHERE org_id = $1 ORDER BY service_name"#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Distinct profile types, optionally scoped to a service (the type picker).
pub async fn profile_types(
    pool: &DbPool,
    service: Option<&str>,
    org_id: OrgId,
) -> DbResult<Vec<String>> {
    let rows = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT profile_type AS "profile_type!"
        FROM profiles
        WHERE ($1::text IS NULL OR service_name = $1)
          AND org_id = $2
        ORDER BY profile_type
        "#,
        service,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete profiles older than `days`. Flat age-based tier, like trace spans.
pub async fn prune(pool: &DbPool, days: i32) -> DbResult<u64> {
    // Chunked so a large profile backlog doesn't lock the table in one big DELETE.
    crate::prune::batched_delete(pool, "profiles", "received_at", days).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    const TEST_ORG: rampart_core::ids::OrgId =
        rampart_core::ids::OrgId::from_uuid(rampart_core::org::DEFAULT_ORG_ID);

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

    #[sqlx::test(migrations = "../../migrations")]
    async fn insert_list_window_pickers_prune(pool: PgPool) {
        let id1 = insert(&pool, np("api", b"a;b 3\n"), TEST_ORG).await.unwrap();
        insert(&pool, np("api", b"a;c 2\n"), TEST_ORG).await.unwrap();
        assert!(id1 > 0);

        // list — unscoped + scoped + miss
        assert_eq!(list(&pool, None, None, 24, 100, TEST_ORG).await.unwrap().len(), 2);
        assert_eq!(
            list(&pool, Some("api"), Some("cpu"), 24, 100, TEST_ORG)
                .await
                .unwrap()
                .len(),
            2
        );
        assert!(list(&pool, Some("nope"), None, 24, 100, TEST_ORG)
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
            TEST_ORG,
        )
        .await
        .unwrap();
        assert_eq!(blobs.len(), 2);

        // pickers
        assert_eq!(services(&pool, TEST_ORG).await.unwrap(), vec!["api".to_string()]);
        assert_eq!(
            profile_types(&pool, Some("api"), TEST_ORG).await.unwrap(),
            vec!["cpu".to_string()]
        );

        // single fetch carries the type
        assert_eq!(fetch_folded(&pool, id1, TEST_ORG).await.unwrap().unwrap().0, "cpu");

        // prune: fresh rows survive; an aged row is dropped
        assert_eq!(prune(&pool, 7).await.unwrap(), 0);
        sqlx::query("UPDATE profiles SET received_at = now() - interval '10 days' WHERE id = $1")
            .bind(id1)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(prune(&pool, 7).await.unwrap(), 1);
    }
}
