//! Durable per-API-key rate-limit counter (item 6).
//!
//! A FIXED-window counter persisted in `api_key_rate_usage`: one row per key
//! carrying the current window's start time and the count of requests
//! admitted within it. Durable across restarts (unlike the previous
//! in-process deque). The fixed-window model trades the rolling-window's
//! precision for simplicity and persistence; a burst straddling a window
//! boundary can briefly exceed the budget within any trailing hour. That's
//! acceptable here — this is a courtesy throttle backing advisory
//! `X-RateLimit-*` headers, not a hard cross-node quota.

use crate::{DbPool, DbResult};
use rampart_core::ids::ApiKeyId;
use time::OffsetDateTime;

/// Length of the fixed window, in seconds (1 hour).
const WINDOW_SECS: i64 = 3600;

/// Outcome of admitting one request for a key against its budget.
#[derive(Debug, Clone, Copy)]
pub struct RateDecision {
    /// Whether the request is under the budget (false → caller returns 429).
    pub allowed: bool,
    /// Requests remaining in the current window after this one. 0 when over.
    pub remaining: u32,
    /// Seconds until the current window ends and the counter resets.
    pub reset_secs: u64,
}

/// Atomically admit (or reject) one request for `api_key_id` against `budget`
/// (the key's `rate_limit_per_hour`) for a one-hour fixed window anchored at
/// `now`.
///
/// Race-safe under concurrent requests: the whole window-roll-or-increment is
/// expressed in ONE `INSERT ... ON CONFLICT DO UPDATE`. The `CASE` rolls the
/// window when the stored `window_start` has aged past the window length
/// (reset to `{window_start = now, count = 1}`), otherwise increments `count`.
/// The row's post-write `window_start` + `count` are returned so the caller
/// can derive `allowed` / `remaining` / `reset_secs` without a second query.
pub async fn admit(pool: &DbPool, api_key_id: ApiKeyId, budget: u32) -> DbResult<RateDecision> {
    let now = OffsetDateTime::now_utc();

    let row = sqlx::query!(
        r#"
        INSERT INTO api_key_rate_usage (api_key_id, window_start, count)
        VALUES ($1, $2, 1)
        ON CONFLICT (api_key_id) DO UPDATE SET
            window_start = CASE
                WHEN api_key_rate_usage.window_start <= $2 - make_interval(secs => $3::double precision)
                    THEN $2
                ELSE api_key_rate_usage.window_start
            END,
            count = CASE
                WHEN api_key_rate_usage.window_start <= $2 - make_interval(secs => $3::double precision)
                    THEN 1
                ELSE api_key_rate_usage.count + 1
            END
        RETURNING window_start, count
        "#,
        api_key_id.0,
        now,
        WINDOW_SECS as f64,
    )
    .fetch_one(pool)
    .await?;

    let count = row.count.max(0) as u32;
    let allowed = count <= budget;
    let remaining = budget.saturating_sub(count);

    // The window ends WINDOW_SECS after its start; reset is the remaining
    // time until then (never negative, clamped to the full window).
    let elapsed = (now - row.window_start).whole_seconds();
    let reset_secs = (WINDOW_SECS - elapsed).clamp(0, WINDOW_SECS) as u64;

    Ok(RateDecision {
        allowed,
        remaining,
        reset_secs,
    })
}
