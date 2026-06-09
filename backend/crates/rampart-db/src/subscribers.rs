//! Status-page email subscribers.
//!
//! v1 is single-opt-in: the row lands `confirmed = TRUE` straight from
//! the subscribe call. A double-opt-in flow (separate confirm endpoint)
//! lands later; the column is in place so the upgrade is additive.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{StatusPageId, StatusPageSubscriberId};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct Subscriber {
    pub id: StatusPageSubscriberId,
    pub status_page_id: StatusPageId,
    pub channel: String,
    pub destination: String,
    pub confirmed: bool,
    pub subscribed_at: OffsetDateTime,
}

/// Add an email subscriber. Generates a random unsubscribe token. If
/// the same email is already subscribed to the same page, this returns
/// the existing row instead of erroring — subscribe is idempotent from
/// the user's POV.
pub async fn subscribe_email(
    pool: &DbPool,
    page: StatusPageId,
    email: &str,
) -> DbResult<(Subscriber, String)> {
    let token = generate_token();
    // INSERT … ON CONFLICT DO NOTHING then SELECT — two round-trips but
    // avoids a unique constraint we don't have and keeps the path simple.
    let existing = sqlx::query!(
        r#"
        SELECT id, status_page_id, channel, destination, confirmed,
               confirm_token, subscribed_at
        FROM status_page_subscribers
        WHERE status_page_id = $1
          AND channel = 'email'
          AND lower(destination) = lower($2)
        "#,
        page.0,
        email,
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = existing {
        let token = row.confirm_token.clone().unwrap_or_else(generate_token);
        return Ok((
            Subscriber {
                id: StatusPageSubscriberId::from_uuid(row.id),
                status_page_id: StatusPageId::from_uuid(row.status_page_id),
                channel: row.channel,
                destination: row.destination,
                confirmed: row.confirmed,
                subscribed_at: row.subscribed_at,
            },
            token,
        ));
    }

    let id = StatusPageSubscriberId::new();
    sqlx::query!(
        r#"
        INSERT INTO status_page_subscribers
            (id, status_page_id, channel, destination, confirmed, confirm_token)
        VALUES ($1, $2, 'email', $3, TRUE, $4)
        "#,
        id.0,
        page.0,
        email,
        token,
    )
    .execute(pool)
    .await?;

    let sub = Subscriber {
        id,
        status_page_id: page,
        channel: "email".into(),
        destination: email.into(),
        confirmed: true,
        subscribed_at: OffsetDateTime::now_utc(),
    };
    Ok((sub, token))
}

pub async fn list_for_page(pool: &DbPool, page: StatusPageId) -> DbResult<Vec<Subscriber>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, status_page_id, channel, destination, confirmed, subscribed_at
        FROM status_page_subscribers
        WHERE status_page_id = $1
        ORDER BY subscribed_at DESC
        "#,
        page.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Subscriber {
            id: StatusPageSubscriberId::from_uuid(r.id),
            status_page_id: StatusPageId::from_uuid(r.status_page_id),
            channel: r.channel,
            destination: r.destination,
            confirmed: r.confirmed,
            subscribed_at: r.subscribed_at,
        })
        .collect())
}

/// Email recipients for fan-out — confirmed only.
pub async fn confirmed_emails_for_page(pool: &DbPool, page: StatusPageId) -> DbResult<Vec<String>> {
    let rows = sqlx::query!(
        r#"
        SELECT destination
        FROM status_page_subscribers
        WHERE status_page_id = $1 AND channel = 'email' AND confirmed
        "#,
        page.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.destination).collect())
}

pub async fn delete(pool: &DbPool, id: StatusPageSubscriberId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM status_page_subscribers WHERE id = $1", id.0,)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn unsubscribe_by_token(pool: &DbPool, token: &str) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM status_page_subscribers WHERE confirm_token = $1",
        token,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// A single subscription as shown on the self-manage page: which page
/// the email is subscribed to, joined with the page's slug + title.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedSubscription {
    pub status_page_slug: String,
    pub status_page_title: String,
    pub subscribed_at: OffsetDateTime,
}

/// Resolve a subscriber's email from any of their per-page unsubscribe
/// tokens. Returns `None` when the token matches no row.
pub async fn email_for_token(pool: &DbPool, token: &str) -> DbResult<Option<String>> {
    let row = sqlx::query!(
        "SELECT destination FROM status_page_subscribers WHERE confirm_token = $1",
        token,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.destination))
}

/// Every page the given email is subscribed to (case-insensitive),
/// newest first, joined with the page's slug + title.
pub async fn subscriptions_for_email(
    pool: &DbPool,
    email: &str,
) -> DbResult<Vec<ManagedSubscription>> {
    let rows = sqlx::query!(
        r#"
        SELECT p.slug AS status_page_slug,
               p.title AS status_page_title,
               s.subscribed_at
        FROM status_page_subscribers s
        JOIN status_pages p ON p.id = s.status_page_id
        WHERE s.channel = 'email' AND lower(s.destination) = lower($1)
        ORDER BY s.subscribed_at DESC
        "#,
        email,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ManagedSubscription {
            status_page_slug: r.status_page_slug,
            status_page_title: r.status_page_title,
            subscribed_at: r.subscribed_at,
        })
        .collect())
}

/// Remove the email (case-insensitive) from every page. Returns the
/// number of subscriptions removed.
pub async fn unsubscribe_all_for_email(pool: &DbPool, email: &str) -> DbResult<u64> {
    let result = sqlx::query!(
        "DELETE FROM status_page_subscribers WHERE channel = 'email' AND lower(destination) = lower($1)",
        email,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Remove a single email (case-insensitive) from one page. Idempotent:
/// a no-op delete is not an error from the self-manage flow's POV.
pub async fn unsubscribe_email_from_page(
    pool: &DbPool,
    page: StatusPageId,
    email: &str,
) -> DbResult<()> {
    sqlx::query!(
        "DELETE FROM status_page_subscribers \
         WHERE status_page_id = $1 AND channel = 'email' AND lower(destination) = lower($2)",
        page.0,
        email,
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn generate_token() -> String {
    use rand::Rng;
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect()
}

pub async fn token_for(pool: &DbPool, id: Uuid) -> DbResult<Option<String>> {
    let row = sqlx::query!(
        "SELECT confirm_token FROM status_page_subscribers WHERE id = $1",
        id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|r| r.confirm_token))
}
