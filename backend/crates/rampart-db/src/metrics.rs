//! Aggregate queries used by the Prometheus exposition at `/metrics`.
//!
//! Single-purpose: each function returns a small Vec of `(label, count)`
//! pairs cheap enough to compute on every scrape (typically 15-60s).
//! No caching layer — Postgres aggregates over the dashboard's typical
//! ~hundred-row monitor + channel tables in microseconds.
//!
//! These queries are deliberately read-only and parameter-free so the
//! `/metrics` endpoint can be hit by an unauthenticated Prometheus
//! scraper without leaking anything more than the dashboard's own
//! summary panel already exposes.

use crate::DbError;
use sqlx::PgPool;

/// `(monitor_status, count)` pairs across the `monitors` table.
/// Includes every `monitor_status` variant currently observed —
/// rows whose status is `pending` / `up` / `down` / `warn` / `paused`
/// / `maintenance` all surface, so the consumer can compute totals
/// without a separate query.
pub async fn monitors_by_status(pool: &PgPool) -> Result<Vec<(String, i64)>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT current_status::text AS "status!", COUNT(*) AS "count!"
        FROM monitors
        GROUP BY current_status
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.status, r.count)).collect())
}

/// `(monitor_kind, count)` pairs across the `monitors` table —
/// "how many of each probe kind is this deployment running".
pub async fn monitors_by_kind(pool: &PgPool) -> Result<Vec<(String, i64)>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT kind::text AS "kind!", COUNT(*) AS "count!"
        FROM monitors
        GROUP BY kind
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.kind, r.count)).collect())
}

/// Total active notification channels.
pub async fn channels_active(pool: &PgPool) -> Result<i64, DbError> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM notifications WHERE active"#,)
        .fetch_one(pool)
        .await?;
    Ok(row.count)
}

/// Total web-push subscribers across all channels — useful to know
/// before pushing a fan-out notification touches.
pub async fn webpush_subscribers(pool: &PgPool) -> Result<i64, DbError> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM webpush_subscriptions"#,)
        .fetch_one(pool)
        .await?;
    Ok(row.count)
}

/// `(monitor_status, count)` of heartbeats observed over the trailing
/// window (seconds before now). 24h is the default Prometheus scrape
/// horizon — the consumer can rate-aggregate as needed.
pub async fn heartbeats_recent_by_status(
    pool: &PgPool,
    window_seconds: i64,
) -> Result<Vec<(String, i64)>, DbError> {
    let rows = sqlx::query!(
        r#"
        SELECT status::text AS "status!", COUNT(*) AS "count!"
        FROM heartbeats
        WHERE ts >= NOW() - make_interval(secs => $1::double precision)
        GROUP BY status
        "#,
        window_seconds as f64,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| (r.status, r.count)).collect())
}

/// Open / unresolved incident count.
pub async fn incidents_open(pool: &PgPool) -> Result<i64, DbError> {
    let row = sqlx::query!(r#"SELECT COUNT(*) AS "count!" FROM incidents WHERE active"#,)
        .fetch_one(pool)
        .await?;
    Ok(row.count)
}
