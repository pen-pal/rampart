//! Integration tests for `rampart_db::sessions`.

use rampart_db::sessions::{cleanup_expired, create, delete, get};
use rampart_db::users::NewUser;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool, email: &str) -> rampart_core::ids::UserId {
    rampart_db::users::create(
        pool,
        NewUser {
            email: email.into(),
            name: None,
            password_hash: "fake".into(),
            is_admin: false,
        },
    )
    .await
    .unwrap()
    .id
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_and_read_back(pool: PgPool) {
    let uid = make_user(&pool, "s@x.com").await;
    let s = create(&pool, uid, 3600, None, Some("curl/8".into()))
        .await
        .unwrap();
    let got = get(&pool, s.id).await.unwrap();
    assert_eq!(got.id, s.id);
    assert_eq!(got.user_id, uid);
}

#[sqlx::test(migrations = "../../migrations")]
async fn expired_session_is_not_returned(pool: PgPool) {
    let uid = make_user(&pool, "exp@x.com").await;
    // Negative TTL → already expired.
    let s = create(&pool, uid, -10, None, None).await.unwrap();
    let err = get(&pool, s.id).await.unwrap_err();
    assert!(
        matches!(err, rampart_db::DbError::NotFound),
        "expired session should not be returned"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_removes_session(pool: PgPool) {
    let uid = make_user(&pool, "del@x.com").await;
    let s = create(&pool, uid, 3600, None, None).await.unwrap();
    delete(&pool, s.id).await.unwrap();
    let err = get(&pool, s.id).await.unwrap_err();
    assert!(matches!(err, rampart_db::DbError::NotFound));
}

#[sqlx::test(migrations = "../../migrations")]
async fn cleanup_expired_only_drops_old(pool: PgPool) {
    let uid = make_user(&pool, "cleanup@x.com").await;
    let live = create(&pool, uid, 3600, None, None).await.unwrap();
    let _expired = create(&pool, uid, -10, None, None).await.unwrap();

    let dropped = cleanup_expired(&pool).await.unwrap();
    assert!(
        dropped >= 1,
        "expected at least the one expired session deleted"
    );
    // Live session still around.
    assert!(get(&pool, live.id).await.is_ok());
}

#[sqlx::test(migrations = "../../migrations")]
async fn get_unknown_returns_not_found(pool: PgPool) {
    let err = get(&pool, Uuid::new_v4()).await.unwrap_err();
    assert!(matches!(err, rampart_db::DbError::NotFound));
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_unknown_is_idempotent(pool: PgPool) {
    // Deleting a non-existent session is not an error — calling
    // delete twice on the same id should both succeed.
    let uid = make_user(&pool, "idem@x.com").await;
    let s = create(&pool, uid, 3600, None, None).await.unwrap();
    delete(&pool, s.id).await.unwrap();
    delete(&pool, s.id).await.unwrap();
}
