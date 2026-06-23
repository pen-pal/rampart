//! MySQL `subscribers` domain — status-page email subscribers. Ported from PG.
//!
//! Dialect: uuid→CHAR(36), ts→BIGINT, bool→TINYINT. `lower(x) = lower(?)` ports
//! verbatim (utf8mb4 is case-insensitive by default, but the explicit LOWER()
//! keeps the match collation-independent). No `RETURNING`; subscribe is a
//! SELECT-then-INSERT (idempotent: an already-subscribed email returns its row).

use super::{raw_uuid, ts};
use crate::subscribers::{ManagedSubscription, Subscriber};
use crate::{DbError, DbResult};
use rampart_core::ids::{StatusPageId, StatusPageSubscriberId};
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

// ponytail: tiny token generator duplicated from the PG module rather than
// widening its visibility — 8 lines, no shared state.
fn generate_token() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect()
}

fn sub_from(r: &sqlx::mysql::MySqlRow) -> Subscriber {
    Subscriber {
        id: StatusPageSubscriberId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        status_page_id: StatusPageId::from_uuid(raw_uuid(&r.get::<String, _>("status_page_id"))),
        channel: r.get("channel"),
        destination: r.get("destination"),
        confirmed: r.get::<i64, _>("confirmed") != 0,
        subscribed_at: ts(r.get::<i64, _>("subscribed_at")),
    }
}

pub async fn subscribe_email(
    pool: &MySqlPool,
    page: StatusPageId,
    email: &str,
) -> DbResult<(Subscriber, String)> {
    let existing = sqlx::query(
        "SELECT id, status_page_id, channel, destination, confirmed, confirm_token, subscribed_at
         FROM status_page_subscribers
         WHERE status_page_id = ? AND channel = 'email' AND LOWER(destination) = LOWER(?)",
    )
    .bind(page.0.to_string())
    .bind(email)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = existing {
        let token = row
            .get::<Option<String>, _>("confirm_token")
            .unwrap_or_else(generate_token);
        return Ok((sub_from(&row), token));
    }

    let id = StatusPageSubscriberId::new();
    let token = generate_token();
    sqlx::query(
        "INSERT INTO status_page_subscribers
            (id, status_page_id, channel, destination, confirmed, confirm_token)
         VALUES (?, ?, 'email', ?, 1, ?)",
    )
    .bind(id.0.to_string())
    .bind(page.0.to_string())
    .bind(email)
    .bind(&token)
    .execute(pool)
    .await?;

    let sub = Subscriber {
        id,
        status_page_id: page,
        channel: "email".into(),
        destination: email.into(),
        confirmed: true,
        subscribed_at: time::OffsetDateTime::now_utc(),
    };
    Ok((sub, token))
}

pub async fn list_for_page(pool: &MySqlPool, page: StatusPageId) -> DbResult<Vec<Subscriber>> {
    let rows = sqlx::query(
        "SELECT id, status_page_id, channel, destination, confirmed, confirm_token, subscribed_at
         FROM status_page_subscribers WHERE status_page_id = ? ORDER BY subscribed_at DESC",
    )
    .bind(page.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(sub_from).collect())
}

pub async fn confirmed_emails_for_page(
    pool: &MySqlPool,
    page: StatusPageId,
) -> DbResult<Vec<String>> {
    let rows = sqlx::query(
        "SELECT destination FROM status_page_subscribers
         WHERE status_page_id = ? AND channel = 'email' AND confirmed = 1",
    )
    .bind(page.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| r.get::<String, _>("destination"))
        .collect())
}

pub async fn delete(pool: &MySqlPool, id: StatusPageSubscriberId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM status_page_subscribers WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn unsubscribe_by_token(pool: &MySqlPool, token: &str) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM status_page_subscribers WHERE confirm_token = ?")
        .bind(token)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn email_for_token(pool: &MySqlPool, token: &str) -> DbResult<Option<String>> {
    let row =
        sqlx::query("SELECT destination FROM status_page_subscribers WHERE confirm_token = ?")
            .bind(token)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.get::<String, _>("destination")))
}

pub async fn subscriptions_for_email(
    pool: &MySqlPool,
    email: &str,
) -> DbResult<Vec<ManagedSubscription>> {
    let rows = sqlx::query(
        "SELECT p.slug AS status_page_slug, p.title AS status_page_title, s.subscribed_at
         FROM status_page_subscribers s
         JOIN status_pages p ON p.id = s.status_page_id
         WHERE s.channel = 'email' AND LOWER(s.destination) = LOWER(?)
         ORDER BY s.subscribed_at DESC",
    )
    .bind(email)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ManagedSubscription {
            status_page_slug: r.get("status_page_slug"),
            status_page_title: r.get("status_page_title"),
            subscribed_at: ts(r.get::<i64, _>("subscribed_at")),
        })
        .collect())
}

pub async fn unsubscribe_all_for_email(pool: &MySqlPool, email: &str) -> DbResult<u64> {
    let res = sqlx::query(
        "DELETE FROM status_page_subscribers WHERE channel = 'email' AND LOWER(destination) = LOWER(?)",
    )
    .bind(email)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn unsubscribe_email_from_page(
    pool: &MySqlPool,
    page: StatusPageId,
    email: &str,
) -> DbResult<()> {
    sqlx::query(
        "DELETE FROM status_page_subscribers
         WHERE status_page_id = ? AND channel = 'email' AND LOWER(destination) = LOWER(?)",
    )
    .bind(page.0.to_string())
    .bind(email)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn page_for(
    pool: &MySqlPool,
    id: StatusPageSubscriberId,
) -> DbResult<Option<StatusPageId>> {
    let row = sqlx::query("SELECT status_page_id FROM status_page_subscribers WHERE id = ?")
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| StatusPageId::from_uuid(raw_uuid(&r.get::<String, _>("status_page_id")))))
}

pub async fn token_for(pool: &MySqlPool, id: Uuid) -> DbResult<Option<String>> {
    let row = sqlx::query("SELECT confirm_token FROM status_page_subscribers WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|r| r.get::<Option<String>, _>("confirm_token")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    async fn seed_page(
        pool: &MySqlPool,
        org: rampart_core::ids::OrgId,
        slug: &str,
    ) -> StatusPageId {
        let id = StatusPageId::new();
        sqlx::query("INSERT INTO status_pages (id, slug, title, org_id) VALUES (?, ?, ?, ?)")
            .bind(id.0.to_string())
            .bind(slug)
            .bind("P")
            .bind(org.0.to_string())
            .execute(pool)
            .await
            .unwrap();
        id
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn subscribe_list_manage_unsubscribe(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let p1 = seed_page(&pool, org, "one").await;
        let p2 = seed_page(&pool, org, "two").await;

        let (s, token) = subscribe_email(&pool, p1, "Alice@Example.com")
            .await
            .unwrap();
        assert!(s.confirmed);
        // idempotent: same email (different case) returns the same row + token.
        let (s2, token2) = subscribe_email(&pool, p1, "alice@example.com")
            .await
            .unwrap();
        assert_eq!(s2.id, s.id);
        assert_eq!(token2, token);
        assert_eq!(list_for_page(&pool, p1).await.unwrap().len(), 1);
        assert_eq!(confirmed_emails_for_page(&pool, p1).await.unwrap().len(), 1);

        // subscribe the same email to a second page → two managed subscriptions.
        subscribe_email(&pool, p2, "alice@example.com")
            .await
            .unwrap();
        assert_eq!(
            subscriptions_for_email(&pool, "ALICE@example.com")
                .await
                .unwrap()
                .len(),
            2
        );

        // token round-trips: email + page lookups.
        assert_eq!(
            email_for_token(&pool, &token).await.unwrap().as_deref(),
            Some("Alice@Example.com")
        );
        assert_eq!(page_for(&pool, s.id).await.unwrap(), Some(p1));
        assert_eq!(
            token_for(&pool, s.id.0).await.unwrap().as_deref(),
            Some(token.as_str())
        );

        // unsubscribe one page (idempotent), then all.
        unsubscribe_email_from_page(&pool, p1, "alice@example.com")
            .await
            .unwrap();
        unsubscribe_email_from_page(&pool, p1, "alice@example.com")
            .await
            .unwrap(); // no-op, no error
        assert!(list_for_page(&pool, p1).await.unwrap().is_empty());
        assert_eq!(
            unsubscribe_all_for_email(&pool, "alice@example.com")
                .await
                .unwrap(),
            1
        );
        assert!(subscriptions_for_email(&pool, "alice@example.com")
            .await
            .unwrap()
            .is_empty());

        // delete + token-unsubscribe NotFound paths.
        let (s3, t3) = subscribe_email(&pool, p2, "bob@x.com").await.unwrap();
        delete(&pool, s3.id).await.unwrap();
        assert!(matches!(delete(&pool, s3.id).await, Err(DbError::NotFound)));
        assert!(matches!(
            unsubscribe_by_token(&pool, &t3).await,
            Err(DbError::NotFound)
        ));

        // confirmed_subscriber_emails_for_monitors: a monitor on p2 reaches its subs.
        let m = super::super::monitors::create(
            &pool,
            serde_json::from_value(serde_json::json!({
                "name": "m", "kind": "http", "url": "https://x",
                "interval_seconds": 60, "timeout_seconds": 10, "max_retries": 0,
                "retry_interval_sec": 60, "resend_interval_sec": 0, "upside_down": false,
                "http_method": "GET", "accepted_statuses": [200],
                "follow_redirect": true, "ignore_tls": false, "check_cert": false,
                "cert_expiry_days": 14
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap()
        .id;
        sqlx::query(
            "INSERT INTO status_page_monitors (page_id, monitor_id, position) VALUES (?, ?, 0)",
        )
        .bind(p2.0.to_string())
        .bind(m.0.to_string())
        .execute(&pool)
        .await
        .unwrap();
        subscribe_email(&pool, p2, "carol@x.com").await.unwrap();
        let reach =
            super::super::maintenance::confirmed_subscriber_emails_for_monitors(&pool, &[m])
                .await
                .unwrap();
        assert_eq!(reach, vec!["carol@x.com".to_string()]);
    }
}
