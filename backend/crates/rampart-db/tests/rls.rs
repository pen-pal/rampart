//! Row-Level Security validation (defense-in-depth, behind `RAMPART_RLS`).
//!
//! Migration 0114 ships the `rampart_app` role + grants; 0115 ships the
//! org_isolation policies but leaves RLS DORMANT (never ENABLE/FORCE). These
//! tests prove the machinery WORKS by enabling RLS *inside the test* on a
//! couple of roots only — they never touch the real migrations' dormant state,
//! and the changes live in the per-test temp DB which sqlx drops afterward.
//!
//! What they assert:
//!   (a) as `rampart_app` with org=A, only A's rows are visible;
//!   (b) as the owner (BYPASSRLS / superuser), all rows are visible — the
//!       system-loop path;
//!   (c) `app_current_org()` returns NULL (not a 22P02 error) when the GUC is
//!       empty, and a SELECT under an empty GUC returns 0 rows (no 500);
//!   (d) an INSERT *as* `rampart_app` succeeds — proves the S1 sequence grants.

use sqlx::{Executor, PgPool, Row};
use uuid::Uuid;

const ORG_A: &str = "00000000-0000-0000-0000-0000000000aa";
const ORG_B: &str = "00000000-0000-0000-0000-0000000000bb";

/// Seed two orgs + one monitor each, enable RLS on `monitors` only, and grant
/// the connecting (superuser) login role membership in `rampart_app` so the
/// test can `SET ROLE`. Returns the two org UUIDs.
async fn setup(pool: &PgPool) -> (Uuid, Uuid) {
    let org_a = Uuid::parse_str(ORG_A).unwrap();
    let org_b = Uuid::parse_str(ORG_B).unwrap();

    // The migration already granted rampart_app to current_user, but re-grant
    // defensively (idempotent) so SET ROLE below is guaranteed to work.
    pool.execute(
        "DO $$ BEGIN EXECUTE format('GRANT rampart_app TO %I', current_user); END $$;",
    )
    .await
    .unwrap();

    for (i, org) in [org_a, org_b].into_iter().enumerate() {
        // Slug must be unique AND match ^[a-z0-9-]{2,40}$, so derive a short
        // distinct one per org (the two UUIDs share an 8-char prefix).
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, $2, $3)")
            .bind(org)
            .bind(format!("org-{org}"))
            .bind(format!("rls-test-org-{i}"))
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO monitors (id, name, kind, org_id) VALUES ($1, $2, 'http', $3)")
            .bind(Uuid::now_v7())
            .bind(format!("mon-{org}"))
            .bind(org)
            .execute(pool)
            .await
            .unwrap();
    }

    // Enable (and FORCE so even the table owner is subject) RLS on monitors —
    // ONLY in this temp DB. The shipped migrations never do this.
    pool.execute("ALTER TABLE monitors ENABLE ROW LEVEL SECURITY")
        .await
        .unwrap();
    pool.execute("ALTER TABLE monitors FORCE ROW LEVEL SECURITY")
        .await
        .unwrap();

    (org_a, org_b)
}

/// Visible monitor count on `conn` after binding `org` (or unbinding when None)
/// and switching into `rampart_app`.
async fn count_as_app(pool: &PgPool, org: Option<&str>) -> i64 {
    let mut conn = pool.acquire().await.unwrap();
    // Bind the GUC then assume the policy-subject role (GUC-before-role, mirrors
    // the production before_acquire hook).
    let guc = org.unwrap_or("");
    conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SELECT set_config('app.current_org', '{guc}', false)"
    ))))
    .await
    .unwrap();
    conn.execute("SET ROLE rampart_app").await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM monitors")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    // Restore so the pooled connection is clean for the next acquire.
    conn.execute("RESET ROLE").await.unwrap();
    conn.execute("SELECT set_config('app.current_org', '', false)")
        .await
        .unwrap();
    n
}

#[sqlx::test(migrations = "../../migrations")]
async fn rls_role_migration_is_idempotent(pool: PgPool) {
    // The 0114 role DDL must be safe to re-run (CREATE ROLE is cluster-global;
    // it already ran once for this temp DB and once per every other temp DB in
    // the cluster). Re-execute the guarded create + grants and assert no error
    // (a bare CREATE ROLE would raise 42710 here).
    let role_ddl = "DO $$ BEGIN \
        IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'rampart_app') \
        THEN CREATE ROLE rampart_app NOLOGIN; END IF; END $$;";
    pool.execute(role_ddl).await.expect("first re-run ok");
    pool.execute(role_ddl).await.expect("second re-run ok");
    pool.execute("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO rampart_app")
        .await
        .expect("re-grant tables ok");
    pool.execute("GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO rampart_app")
        .await
        .expect("re-grant sequences ok");

    // The role exists, is NOLOGIN, and is NOT a bypass role.
    let row = sqlx::query(
        "SELECT rolcanlogin, rolbypassrls FROM pg_roles WHERE rolname = 'rampart_app'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let can_login: bool = row.get(0);
    let bypass: bool = row.get(1);
    assert!(!can_login, "rampart_app must be NOLOGIN");
    assert!(!bypass, "rampart_app must NOT be a BYPASSRLS role");
}

#[sqlx::test(migrations = "../../migrations")]
async fn policies_ship_dormant(pool: PgPool) {
    // The shipped migrations define org_isolation policies but must leave RLS
    // DORMANT — no table ENABLED or FORCED. (This guards against a future edit
    // accidentally turning S7 on.)
    let enabled: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_class WHERE relrowsecurity")
        .fetch_one(&pool)
        .await
        .unwrap();
    let forced: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_class WHERE relforcerowsecurity")
            .fetch_one(&pool)
            .await
            .unwrap();
    let policies: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_policies WHERE policyname = 'org_isolation'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(enabled, 0, "no table may have RLS ENABLED in shipped migrations");
    assert_eq!(forced, 0, "no table may have RLS FORCED in shipped migrations");
    assert_eq!(policies, 34, "30 root + 4 child org_isolation policies expected");
}

#[sqlx::test(migrations = "../../migrations")]
async fn rampart_app_sees_only_its_org(pool: PgPool) {
    let (_a, _b) = setup(&pool).await;

    // (a) org A sees exactly its 1 monitor; org B likewise.
    assert_eq!(count_as_app(&pool, Some(ORG_A)).await, 1, "org A sees only A");
    assert_eq!(count_as_app(&pool, Some(ORG_B)).await, 1, "org B sees only B");
}

#[sqlx::test(migrations = "../../migrations")]
async fn owner_bypasses_rls(pool: PgPool) {
    setup(&pool).await;
    // (b) the connecting superuser/owner (the system-loop path: no SET ROLE)
    // sees BOTH orgs' rows. NB: we FORCE'd RLS above, but a superuser still
    // bypasses (rolbypassrls is implied) — exactly the system-loop guarantee.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM monitors")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2, "owner/system-loop path sees all orgs");
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_guc_returns_null_and_no_rows(pool: PgPool) {
    setup(&pool).await;

    // (c) app_current_org() is NULL (not a 22P02 invalid-uuid error) when the
    // GUC is empty — proves the NULLIF(..., '') + current_setting(.., true).
    let mut conn = pool.acquire().await.unwrap();
    conn.execute("SELECT set_config('app.current_org', '', false)")
        .await
        .unwrap();
    let is_null: bool = sqlx::query("SELECT app_current_org() IS NULL")
        .fetch_one(&mut *conn)
        .await
        .unwrap()
        .get(0);
    assert!(is_null, "empty GUC -> app_current_org() must be NULL");

    // ...and a SELECT under the empty GUC as rampart_app returns 0 rows, NOT a
    // 500. (org_id = NULL is NULL, never true, so nothing matches.)
    assert_eq!(
        count_as_app(&pool, None).await,
        0,
        "empty GUC must yield 0 rows, not an error"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_as_rampart_app_succeeds(pool: PgPool) {
    let (org_a, _b) = setup(&pool).await;

    // (d) an INSERT as rampart_app with the matching org passes the WITH CHECK
    // policy AND succeeds at the privilege level — proving the S1 sequence
    // grants (delivery_log/profiles are BIGSERIAL; insert one to exercise a
    // sequence-backed default under the role).
    let mut conn = pool.acquire().await.unwrap();
    conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "SELECT set_config('app.current_org', '{org_a}', false)"
    ))))
    .await
    .unwrap();
    conn.execute("SET ROLE rampart_app").await.unwrap();

    // Plain INSERT into the RLS-FORCED monitors table (WITH CHECK org_id = A).
    sqlx::query("INSERT INTO monitors (id, name, kind, org_id) VALUES ($1, $2, 'http', $3)")
        .bind(Uuid::now_v7())
        .bind("inserted-as-app")
        .bind(org_a)
        .execute(&mut *conn)
        .await
        .expect("INSERT as rampart_app (matching org) must succeed");

    // BIGSERIAL sequence exercise: delivery_log has no RLS enabled here, so this
    // isolates the SEQUENCE grant (nextval on the owned sequence) under the role.
    sqlx::query(
        "INSERT INTO delivery_log (channel_kind, event_kind, ok, org_id) \
         VALUES ('email', 'status_flip', true, $1)",
    )
    .bind(org_a)
    .execute(&mut *conn)
    .await
    .expect("BIGSERIAL INSERT as rampart_app must succeed (sequence grant)");

    conn.execute("RESET ROLE").await.unwrap();
}
