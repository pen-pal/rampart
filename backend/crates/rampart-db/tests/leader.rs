//! Proves the Postgres advisory-lock exclusivity that the leader election
//! (`rampart_db::leader`) depends on: at most one session holds the scheduler
//! lock at a time, and it transfers when the holder releases (≈ a leader
//! exiting → a follower taking over).

use rampart_db::leader::ADVISORY_LOCK_KEY;
use sqlx::PgPool;

#[sqlx::test]
async fn advisory_lock_is_mutually_exclusive_and_transfers(pool: PgPool) {
    // Two independent sessions (= two replicas).
    let mut a = pool.acquire().await.unwrap();
    let mut b = pool.acquire().await.unwrap();

    let got_a: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .fetch_one(&mut *a)
        .await
        .unwrap();
    assert!(got_a, "first session must acquire the leader lock");

    let got_b: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .fetch_one(&mut *b)
        .await
        .unwrap();
    assert!(!got_b, "second session must NOT acquire while the first holds it");

    // Leader 'a' exits → releases the lock.
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .execute(&mut *a)
        .await
        .unwrap();

    let got_b2: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(ADVISORY_LOCK_KEY)
        .fetch_one(&mut *b)
        .await
        .unwrap();
    assert!(got_b2, "follower must take over once the leader releases");
}
