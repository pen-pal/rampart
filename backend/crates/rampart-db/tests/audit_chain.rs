//! The audit log is tamper-evident: a hash chain over the rows means any
//! edit/delete/reorder is detected by `verify_chain`.

use rampart_db::audit::{self, NewEntry};
use sqlx::PgPool;

fn entry(action: &str) -> NewEntry<'_> {
    NewEntry {
        actor_user_id: None,
        actor_api_key_id: None,
        action,
        resource_kind: "monitor",
        resource_id: None,
        payload: Some(serde_json::json!({ "name": "x" })),
        ip_addr: None,
        user_agent: Some("test"),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn intact_chain_verifies(pool: PgPool) {
    for a in ["monitor.create", "monitor.update", "monitor.delete"] {
        audit::insert(&pool, entry(a)).await.unwrap();
    }
    let r = audit::verify_chain(&pool).await.unwrap();
    assert!(r.ok, "fresh chain should verify");
    assert_eq!(r.checked, 3);
    assert!(r.first_bad_id.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn tampering_is_detected(pool: PgPool) {
    for a in ["a.one", "a.two", "a.three"] {
        audit::insert(&pool, entry(a)).await.unwrap();
    }
    // Edit the middle row's action directly — simulates an attacker rewriting
    // history in the DB without the MAC key.
    let mid: i64 = sqlx::query_scalar("SELECT id FROM audit_log ORDER BY id ASC OFFSET 1 LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE audit_log SET action = 'tampered' WHERE id = $1")
        .bind(mid)
        .execute(&pool)
        .await
        .unwrap();

    let r = audit::verify_chain(&pool).await.unwrap();
    assert!(!r.ok, "tampered chain must not verify");
    assert_eq!(r.first_bad_id, Some(mid), "should flag the edited row");
}

#[sqlx::test(migrations = "../../migrations")]
async fn head_truncation_with_watermark_verifies(pool: PgPool) {
    // Routine retention deletes the OLDEST rows. With a prune watermark seeded
    // from the newest deleted row, the surviving chain must still verify (no
    // false tamper report).
    for a in ["w.one", "w.two", "w.three", "w.four"] {
        audit::insert(&pool, entry(a)).await.unwrap();
    }
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, hash FROM audit_log WHERE hash IS NOT NULL ORDER BY id ASC")
            .fetch_all(&pool)
            .await
            .unwrap();
    // Prune the first two rows; the 2nd is the newest deleted → the watermark.
    let cut = &rows[1];
    audit::set_chain_watermark(&pool, cut.0, &cut.1).await.unwrap();
    sqlx::query("DELETE FROM audit_log WHERE id <= $1")
        .bind(cut.0)
        .execute(&pool)
        .await
        .unwrap();

    let r = audit::verify_chain(&pool).await.unwrap();
    assert!(r.ok, "head-truncation with a watermark must verify");
    assert_eq!(r.checked, 2, "the two surviving rows are checked");
    assert!(r.first_bad_id.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn deletion_after_watermark_still_detected(pool: PgPool) {
    // The watermark must NOT mask a deletion of a surviving row (tampering
    // after the prune point still breaks the chain).
    for a in ["x.one", "x.two", "x.three", "x.four"] {
        audit::insert(&pool, entry(a)).await.unwrap();
    }
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, hash FROM audit_log WHERE hash IS NOT NULL ORDER BY id ASC")
            .fetch_all(&pool)
            .await
            .unwrap();
    // Watermark at row[0] (as if row[0] was pruned). Then delete row[2], a
    // SURVIVING middle row → row[3]'s prev_hash linkage breaks.
    audit::set_chain_watermark(&pool, rows[0].0, &rows[0].1)
        .await
        .unwrap();
    sqlx::query("DELETE FROM audit_log WHERE id IN ($1, $2)")
        .bind(rows[0].0)
        .bind(rows[2].0)
        .execute(&pool)
        .await
        .unwrap();

    let r = audit::verify_chain(&pool).await.unwrap();
    assert!(!r.ok, "deleting a surviving row must still break the chain");
}

#[sqlx::test(migrations = "../../migrations")]
async fn deletion_is_detected(pool: PgPool) {
    for a in ["d.one", "d.two", "d.three"] {
        audit::insert(&pool, entry(a)).await.unwrap();
    }
    // Delete the middle row — the next row's prev_hash linkage breaks.
    let mid: i64 = sqlx::query_scalar("SELECT id FROM audit_log ORDER BY id ASC OFFSET 1 LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM audit_log WHERE id = $1")
        .bind(mid)
        .execute(&pool)
        .await
        .unwrap();

    let r = audit::verify_chain(&pool).await.unwrap();
    assert!(!r.ok, "a deleted row must break the chain");
}
