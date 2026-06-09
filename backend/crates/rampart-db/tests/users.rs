//! Integration tests for `rampart_db::users` against a real Postgres.
//!
//! `sqlx::test` creates an isolated, migrated database per test (from
//! the `migrations` arg) and drops it afterwards, so tests don't share
//! state and can run in parallel.

use rampart_core::Role;
use rampart_db::users::{count, create, get, get_by_email, mark_login, set_role, NewUser};
use sqlx::PgPool;

fn sample(email: &str) -> NewUser {
    NewUser {
        email: email.into(),
        name: Some("Sample".into()),
        // Argon2 hash of literal "password" — not used for verification
        // here; users::create just stores whatever string we give it.
        password_hash: "$argon2id$v=19$m=19456,t=2,p=1$fake$hash".into(),
        role: Role::Admin,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn count_is_zero_on_fresh_db(pool: PgPool) {
    let n = count(&pool).await.unwrap();
    assert_eq!(n, 0, "fresh db should have no users");
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_and_read_back(pool: PgPool) {
    let u = create(&pool, sample("alice@example.com")).await.unwrap();
    assert_eq!(u.email, "alice@example.com");
    assert!(u.is_admin);
    assert_eq!(u.role, Role::Admin);
    assert!(
        u.last_login_at.is_none(),
        "freshly created user has no login yet"
    );

    let again = get(&pool, u.id).await.unwrap();
    assert_eq!(again.id, u.id);
    assert_eq!(again.email, "alice@example.com");
}

#[sqlx::test(migrations = "../../migrations")]
async fn set_role_keeps_is_admin_shim_in_sync(pool: PgPool) {
    let u = create(&pool, sample("role@example.com")).await.unwrap();
    assert!(u.is_admin);
    assert_eq!(u.role, Role::Admin);

    set_role(&pool, u.id, Role::Readonly).await.unwrap();
    let ro = get(&pool, u.id).await.unwrap();
    assert_eq!(ro.role, Role::Readonly);
    assert!(!ro.is_admin, "is_admin shim must follow role");
    assert!(!ro.role.can_write());

    set_role(&pool, u.id, Role::Editor).await.unwrap();
    let ed = get(&pool, u.id).await.unwrap();
    assert_eq!(ed.role, Role::Editor);
    assert!(!ed.is_admin);
    assert!(ed.role.can_write());

    set_role(&pool, u.id, Role::Admin).await.unwrap();
    let ad = get(&pool, u.id).await.unwrap();
    assert!(ad.is_admin, "promoting to admin re-sets the shim");
    assert!(ad.role.is_admin());
}

#[sqlx::test(migrations = "../../migrations")]
async fn email_is_unique(pool: PgPool) {
    create(&pool, sample("dup@example.com")).await.unwrap();
    let err = create(&pool, sample("dup@example.com")).await.unwrap_err();
    assert!(
        matches!(err, rampart_db::DbError::Conflict(_)),
        "second insert should hit unique violation, got: {err:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn email_lookup_is_case_insensitive(pool: PgPool) {
    // `email` is a CITEXT column.
    create(&pool, sample("CaseSensitive@example.com"))
        .await
        .unwrap();
    let u = get_by_email(&pool, "casesensitive@EXAMPLE.com")
        .await
        .unwrap();
    assert_eq!(u.email.to_lowercase(), "casesensitive@example.com");
}

#[sqlx::test(migrations = "../../migrations")]
async fn count_reflects_creations(pool: PgPool) {
    assert_eq!(count(&pool).await.unwrap(), 0);
    create(&pool, sample("a@x.com")).await.unwrap();
    create(&pool, sample("b@x.com")).await.unwrap();
    create(&pool, sample("c@x.com")).await.unwrap();
    assert_eq!(count(&pool).await.unwrap(), 3);
}

#[sqlx::test(migrations = "../../migrations")]
async fn mark_login_sets_timestamp(pool: PgPool) {
    let u = create(&pool, sample("x@x.com")).await.unwrap();
    assert!(u.last_login_at.is_none());
    mark_login(&pool, u.id).await.unwrap();
    let after = get(&pool, u.id).await.unwrap();
    assert!(
        after.last_login_at.is_some(),
        "mark_login should populate last_login_at"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn get_missing_returns_not_found(pool: PgPool) {
    let err = get_by_email(&pool, "ghost@nowhere.com").await.unwrap_err();
    assert!(matches!(err, rampart_db::DbError::NotFound), "got: {err:?}");
}
