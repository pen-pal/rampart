//! Background pruning with retention tiering.
//!
//! Heartbeat retention is tiered:
//!   * raw tier  — full-resolution `heartbeats` rows are kept for
//!     `settings.retention_days.heartbeats` days;
//!   * rollup tier — once raw rows age past the raw tier they are folded
//!     into hourly `heartbeat_rollups` buckets (up/down/other counts +
//!     avg latency), and the raw rows are deleted. Rollups themselves are
//!     pruned after the longer `retention_days.rollup_days` window.
//!
//! `audit_log` keeps the flat age-based DELETE — it has no rollup tier.
//! Designed to be called from a tokio task on a loop.

use crate::{DbPool, DbResult};
use serde::Deserialize;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_hb")]
    pub heartbeats: i32,
    #[serde(default = "default_audit")]
    pub audit_log: i32,
    /// How long hourly heartbeat rollups are retained, in days. Defaults
    /// to a year so historical uptime charts survive long after the
    /// high-resolution raw heartbeats have been pruned.
    #[serde(default = "default_rollup")]
    pub rollup_days: i32,
    /// How long externally-ingested metric samples are retained, in
    /// days. Defaults to 30 — metrics are operational telemetry, not
    /// the uptime record.
    #[serde(default = "default_metrics")]
    pub metrics_days: i32,
    /// How long ingested trace spans are retained, in days. Defaults to 7 —
    /// traces are high-volume, short-lived debugging data.
    #[serde(default = "default_traces")]
    pub traces_days: i32,
    /// How long ingested logs are retained, in days. Defaults to 7.
    #[serde(default = "default_logs")]
    pub logs_days: i32,
    /// How long RUM events are retained, in days. Defaults to 14.
    #[serde(default = "default_rum")]
    pub rum_days: i32,
    /// How long profiles are retained, in days. Defaults to 7 — profiles are
    /// the heaviest tier per row.
    #[serde(default = "default_profiles")]
    pub profiles_days: i32,
}
fn default_hb() -> i32 {
    365
}
fn default_audit() -> i32 {
    365
}
fn default_rollup() -> i32 {
    DEFAULT_ROLLUP_DAYS
}
fn default_metrics() -> i32 {
    DEFAULT_METRICS_DAYS
}
fn default_traces() -> i32 {
    DEFAULT_TRACES_DAYS
}
fn default_logs() -> i32 {
    DEFAULT_LOGS_DAYS
}
fn default_rum() -> i32 {
    DEFAULT_RUM_DAYS
}
fn default_profiles() -> i32 {
    DEFAULT_PROFILES_DAYS
}

/// Default metric-sample retention when the setting predates the field.
pub const DEFAULT_METRICS_DAYS: i32 = 30;
/// Default trace-span retention when the setting predates the field.
pub const DEFAULT_TRACES_DAYS: i32 = 7;
/// Default log retention when the setting predates the field.
pub const DEFAULT_LOGS_DAYS: i32 = 7;
/// Default RUM-event retention when the setting predates the field.
pub const DEFAULT_RUM_DAYS: i32 = 14;
/// Default profile retention when the setting predates the field.
pub const DEFAULT_PROFILES_DAYS: i32 = 7;

/// Default rollup-tier retention when no `retention_days` setting is
/// present (or the row predates this field).
pub const DEFAULT_ROLLUP_DAYS: i32 = 365;

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            heartbeats: 365,
            audit_log: 365,
            rollup_days: DEFAULT_ROLLUP_DAYS,
            metrics_days: DEFAULT_METRICS_DAYS,
            traces_days: DEFAULT_TRACES_DAYS,
            logs_days: DEFAULT_LOGS_DAYS,
            rum_days: DEFAULT_RUM_DAYS,
            profiles_days: DEFAULT_PROFILES_DAYS,
        }
    }
}

/// Counts from one prune sweep, for logging.
#[derive(Debug, Default, Clone, Copy)]
pub struct PruneStats {
    /// Hourly rollup buckets created or updated this sweep.
    pub rollups_upserted: u64,
    /// External metric samples dropped past their retention window.
    pub metrics_deleted: u64,
    /// Trace spans dropped past the trace-retention window.
    pub spans_deleted: u64,
    /// Logs dropped past the log-retention window.
    pub logs_deleted: u64,
    /// RUM events dropped past the RUM-retention window.
    pub rum_deleted: u64,
    /// Profiles dropped past the profile-retention window.
    pub profiles_deleted: u64,
    /// Raw heartbeats folded into rollups and deleted.
    pub heartbeats_rolled: u64,
    /// Rollup buckets dropped past the rollup tier.
    pub rollups_deleted: u64,
    /// Audit-log rows dropped past the audit tier.
    pub audit_deleted: u64,
    /// Error-tracking events dropped past each project's retention window.
    pub errors_deleted: u64,
}

impl PruneStats {
    fn is_empty(&self) -> bool {
        self.rollups_upserted == 0
            && self.heartbeats_rolled == 0
            && self.rollups_deleted == 0
            && self.audit_deleted == 0
            && self.metrics_deleted == 0
            && self.spans_deleted == 0
            && self.logs_deleted == 0
            && self.errors_deleted == 0
            && self.rum_deleted == 0
            && self.profiles_deleted == 0
    }
}

/// One hourly rollup bucket for a monitor.
#[derive(Debug, Clone)]
pub struct HeartbeatRollup {
    pub monitor_id: Uuid,
    pub bucket_start: OffsetDateTime,
    pub up_count: i32,
    pub down_count: i32,
    pub other_count: i32,
    pub sample_count: i32,
    pub avg_latency_ms: Option<f64>,
}

/// Load the retention setting; fall back to defaults if the row is
/// missing or malformed (e.g. an old install pre-migration 0020).
pub async fn load_config(pool: &DbPool) -> DbResult<RetentionConfig> {
    let raw = crate::settings::get(pool, "retention_days").await?;
    Ok(raw
        .and_then(|v| serde_json::from_value::<RetentionConfig>(v).ok())
        .unwrap_or_default())
}

/// One day's derived uptime, oldest first. `uptime_pct` is `None` when no
/// samples were recorded that day (caller renders a "no data" gap rather
/// than a misleading 0%).
#[derive(Debug, Clone)]
pub struct DailyUptimePoint {
    pub day: time::Date,
    pub up_count: i64,
    pub sample_count: i64,
    pub uptime_pct: Option<f64>,
}

/// Daily uptime% derived from the hourly `heartbeat_rollups`, over the
/// half-open `[since, until)` range, oldest first. Each day's percentage is
/// `SUM(up_count) / SUM(sample_count) * 100` across that day's hourly
/// buckets. Days with no rollup rows are absent from the result — the
/// caller pivots into a dense series and fills the gaps.
///
/// This is the long-range path: rollups are retained ~1y (the `rollup_days`
/// tier) so this answers uptime questions long after the high-resolution
/// raw heartbeats have been pruned.
pub async fn daily_uptime_from_rollups(
    pool: &DbPool,
    monitor: Uuid,
    since: OffsetDateTime,
    until: OffsetDateTime,
) -> DbResult<Vec<DailyUptimePoint>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            (date_trunc('day', bucket_start AT TIME ZONE 'UTC'))::date AS "day!",
            SUM(up_count)::bigint     AS "up_count!",
            SUM(sample_count)::bigint AS "sample_count!"
        FROM heartbeat_rollups
        WHERE monitor_id = $1
          AND bucket_start >= $2
          AND bucket_start <  $3
        GROUP BY 1
        ORDER BY 1 ASC
        "#,
        monitor,
        since,
        until,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DailyUptimePoint {
            day: r.day,
            up_count: r.up_count,
            sample_count: r.sample_count,
            uptime_pct: if r.sample_count > 0 {
                Some(r.up_count as f64 / r.sample_count as f64 * 100.0)
            } else {
                None
            },
        })
        .collect())
}

/// Daily uptime% derived from the high-resolution raw `heartbeats`, over the
/// half-open `[since, until)` range, oldest first. Same per-day shape as
/// [`daily_uptime_from_rollups`] so the two can be stitched into one series:
/// raw covers the recent within-retention portion, rollups cover everything
/// older. `up_count` counts `status = 'up'`; everything else is non-up.
pub async fn daily_uptime_from_raw(
    pool: &DbPool,
    monitor: Uuid,
    since: OffsetDateTime,
    until: OffsetDateTime,
) -> DbResult<Vec<DailyUptimePoint>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            (date_trunc('day', ts AT TIME ZONE 'UTC'))::date AS "day!",
            COUNT(*) FILTER (WHERE status = 'up')::bigint    AS "up_count!",
            COUNT(*)::bigint                                 AS "sample_count!"
        FROM heartbeats
        WHERE monitor_id = $1
          AND ts >= $2
          AND ts <  $3
        GROUP BY 1
        ORDER BY 1 ASC
        "#,
        monitor,
        since,
        until,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DailyUptimePoint {
            day: r.day,
            up_count: r.up_count,
            sample_count: r.sample_count,
            uptime_pct: if r.sample_count > 0 {
                Some(r.up_count as f64 / r.sample_count as f64 * 100.0)
            } else {
                None
            },
        })
        .collect())
}

/// Fetch hourly rollups for a monitor over `[since, until)`, oldest first.
pub async fn rollups_for_monitor(
    pool: &DbPool,
    monitor: Uuid,
    since: OffsetDateTime,
    until: OffsetDateTime,
) -> DbResult<Vec<HeartbeatRollup>> {
    let rows = sqlx::query!(
        r#"
        SELECT monitor_id, bucket_start, up_count, down_count,
               other_count, sample_count, avg_latency_ms
        FROM heartbeat_rollups
        WHERE monitor_id = $1
          AND bucket_start >= $2
          AND bucket_start <  $3
        ORDER BY bucket_start ASC
        "#,
        monitor,
        since,
        until,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| HeartbeatRollup {
            monitor_id: r.monitor_id,
            bucket_start: r.bucket_start,
            up_count: r.up_count,
            down_count: r.down_count,
            other_count: r.other_count,
            sample_count: r.sample_count,
            avg_latency_ms: r.avg_latency_ms,
        })
        .collect())
}

/// One sweep — best-effort, logs and continues on partial failure.
///
/// The heartbeat tiering (aggregate → delete raw → prune rollups) runs in
/// a single transaction so a crash can never delete raw rows that were not
/// durably rolled up first.
pub async fn run_once(pool: &DbPool) -> DbResult<PruneStats> {
    let cfg = load_config(pool).await?;
    let mut stats = PruneStats::default();

    let mut tx = pool.begin().await?;

    // (1) Aggregate raw heartbeats older than the raw tier into hourly
    // buckets. UPSERT so a partially-folded hour (e.g. a previous sweep
    // that fell mid-hour, or a re-run) accumulates rather than double
    // counts: each sweep folds only the raw rows still present, and step
    // (2) deletes exactly those rows, so re-running is a no-op per bucket.
    stats.rollups_upserted = sqlx::query!(
        r#"
        INSERT INTO heartbeat_rollups
            (monitor_id, bucket_start, up_count, down_count,
             other_count, sample_count, avg_latency_ms)
        SELECT
            monitor_id,
            date_trunc('hour', ts AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' AS bucket_start,
            COUNT(*) FILTER (WHERE status = 'up')::int                   AS up_count,
            COUNT(*) FILTER (WHERE status = 'down')::int                 AS down_count,
            COUNT(*) FILTER (WHERE status NOT IN ('up','down'))::int     AS other_count,
            COUNT(*)::int                                                AS sample_count,
            AVG(latency_ms) FILTER (WHERE latency_ms IS NOT NULL)        AS avg_latency_ms
        FROM heartbeats
        WHERE ts < NOW() - make_interval(days => $1)
        GROUP BY monitor_id, bucket_start
        ON CONFLICT (monitor_id, bucket_start) DO UPDATE SET
            up_count       = heartbeat_rollups.up_count     + EXCLUDED.up_count,
            down_count     = heartbeat_rollups.down_count   + EXCLUDED.down_count,
            other_count    = heartbeat_rollups.other_count  + EXCLUDED.other_count,
            sample_count   = heartbeat_rollups.sample_count + EXCLUDED.sample_count,
            avg_latency_ms = CASE
                WHEN heartbeat_rollups.avg_latency_ms IS NULL THEN EXCLUDED.avg_latency_ms
                WHEN EXCLUDED.avg_latency_ms IS NULL THEN heartbeat_rollups.avg_latency_ms
                ELSE (heartbeat_rollups.avg_latency_ms * heartbeat_rollups.sample_count
                      + EXCLUDED.avg_latency_ms * EXCLUDED.sample_count)
                     / NULLIF(heartbeat_rollups.sample_count + EXCLUDED.sample_count, 0)
            END
        "#,
        cfg.heartbeats,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // (2) Delete the raw heartbeats now represented in rollups — the same
    // age predicate, so we only drop rows that step (1) just folded.
    stats.heartbeats_rolled = sqlx::query!(
        "DELETE FROM heartbeats WHERE ts < NOW() - make_interval(days => $1)",
        cfg.heartbeats,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // (3) Drop rollup buckets past the (longer) rollup tier.
    stats.rollups_deleted = sqlx::query!(
        "DELETE FROM heartbeat_rollups WHERE bucket_start < NOW() - make_interval(days => $1)",
        cfg.rollup_days,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;

    // audit_log keeps the flat tier — independent of the heartbeat txn.
    stats.audit_deleted = sqlx::query!(
        "DELETE FROM audit_log WHERE ts < NOW() - make_interval(days => $1)",
        cfg.audit_log,
    )
    .execute(pool)
    .await?
    .rows_affected();

    // External metric samples: flat age-based tier, no rollup — telemetry
    // past its window is just gone, like audit rows.
    stats.metrics_deleted = sqlx::query!(
        "DELETE FROM metric_samples WHERE ts < NOW() - make_interval(days => $1)",
        cfg.metrics_days,
    )
    .execute(pool)
    .await?
    .rows_affected();

    // Trace spans: flat age-based tier, like metric samples.
    stats.spans_deleted = crate::traces::prune(pool, cfg.traces_days).await?;
    stats.logs_deleted = crate::logs::prune(pool, cfg.logs_days).await?;
    // Error-tracking events: per-project retention (the window lives on each
    // error_projects row, so the delete joins rather than reading cfg).
    stats.errors_deleted = crate::error_tracking::prune(pool).await?;
    stats.rum_deleted = crate::rum::prune(pool, cfg.rum_days).await?;
    stats.profiles_deleted = crate::profiles::prune(pool, cfg.profiles_days).await?;

    Ok(stats)
}

/// Spawnable loop — call once at startup, runs forever. Default cadence
/// is one sweep per hour; tunable via the `interval` arg if a deployment
/// runs hot enough to need shorter windows.
pub async fn run_loop(
    pool: DbPool,
    interval: Duration,
    leadership: std::sync::Arc<crate::leader::Leadership>,
) {
    let mut ticker = tokio::time::interval(interval);
    // Skip the immediate tick — let the scheduler warm up first.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        // Leader-only: a single prune pass across the cluster, no duplicate
        // DELETEs racing each other.
        if !leadership.is_leader() {
            continue;
        }
        match run_once(&pool).await {
            Ok(s) if !s.is_empty() => {
                tracing::info!(
                    rollups_upserted = s.rollups_upserted,
                    heartbeats_rolled = s.heartbeats_rolled,
                    rollups_deleted = s.rollups_deleted,
                    audit_deleted = s.audit_deleted,
                    "retention prune complete"
                );
            }
            Ok(_) => {} // nothing to do; stay quiet
            Err(e) => tracing::warn!(error = %e, "retention prune failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::monitor::NewMonitor;
    use rampart_core::{Heartbeat, MonitorKind, MonitorStatus};
    use sqlx::PgPool;

    fn http_monitor(name: &str) -> NewMonitor {
        NewMonitor {
            name: name.into(),
            kind: MonitorKind::Http,
            url: Some(format!("https://{name}.example.com")),
            hostname: None,
            port: None,
            config: serde_json::Value::Null,
            interval_seconds: 60,
            timeout_seconds: 10,
            max_retries: 0,
            retry_interval_sec: 60,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: None,
            accepted_statuses: vec![200],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            group_id: None,
            slo_target_pct: None,
            slo_window_days: None,
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    fn hb(
        monitor_id: rampart_core::MonitorId,
        status: MonitorStatus,
        latency: Option<i32>,
        secs_ago: i64,
    ) -> Heartbeat {
        Heartbeat {
            monitor_id,
            ts: OffsetDateTime::now_utc() - time::Duration::seconds(secs_ago),
            status,
            latency_ms: latency,
            status_code: None,
            msg: None,
            retries: 0,
            important: false,
        }
    }

    /// Pin the raw + rollup tiers so the test is independent of the
    /// install defaults: raw kept 30 days, rollups kept 400 days.
    async fn set_retention(pool: &PgPool, raw_days: i32, rollup_days: i32) {
        let v = serde_json::json!({
            "heartbeats": raw_days,
            "audit_log": 365,
            "rollup_days": rollup_days,
        });
        crate::settings::put(pool, "retention_days", &v)
            .await
            .unwrap();
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn old_heartbeats_roll_up_and_raw_is_deleted(pool: PgPool) {
        set_retention(&pool, 30, 400).await;
        let m = crate::monitors::create(&pool, http_monitor("rollup"))
            .await
            .unwrap();

        // All ~90 days old → past the 30-day raw tier. Distinct timestamps
        // (so the in-batch ON CONFLICT doesn't dedupe them); statuses mix
        // up/down/other. 90 days = 7_776_000s.
        const D90: i64 = 90 * 86_400;
        let hbs = vec![
            hb(m.id, MonitorStatus::Up, Some(100), D90),
            hb(m.id, MonitorStatus::Up, Some(200), D90 + 60),
            hb(m.id, MonitorStatus::Down, None, D90 + 120),
            hb(m.id, MonitorStatus::Warn, Some(50), D90 + 180),
            // Recent — must be left untouched.
            hb(m.id, MonitorStatus::Up, Some(70), 5),
        ];
        crate::heartbeats::insert_many(&pool, &hbs).await.unwrap();

        let stats = run_once(&pool).await.unwrap();
        assert!(stats.rollups_upserted >= 1, "at least one bucket written");
        assert_eq!(
            stats.heartbeats_rolled, 4,
            "4 old raw rows folded + deleted"
        );

        // The recent heartbeat survives in raw.
        let raw_left: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM heartbeats WHERE monitor_id = $1",
            m.id.0
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .unwrap_or(0);
        assert_eq!(raw_left, 1, "only the recent heartbeat remains raw");

        // Rollups cover all four old heartbeats, with correct status splits
        // and a latency average over the latency-bearing samples only.
        let since = OffsetDateTime::now_utc() - time::Duration::days(400);
        let until = OffsetDateTime::now_utc();
        let rollups = rollups_for_monitor(&pool, m.id.0, since, until)
            .await
            .unwrap();
        let total_samples: i32 = rollups.iter().map(|r| r.sample_count).sum();
        let total_up: i32 = rollups.iter().map(|r| r.up_count).sum();
        let total_down: i32 = rollups.iter().map(|r| r.down_count).sum();
        let total_other: i32 = rollups.iter().map(|r| r.other_count).sum();
        assert_eq!(total_samples, 4);
        assert_eq!(total_up, 2);
        assert_eq!(total_down, 1);
        assert_eq!(total_other, 1, "warn counts as other");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rollups_past_rollup_tier_are_deleted(pool: PgPool) {
        // Raw tier 30d, rollup tier 100d. A rollup bucket 200 days old is
        // dropped; one 50 days old survives.
        set_retention(&pool, 30, 100).await;
        let m = crate::monitors::create(&pool, http_monitor("rolldrop"))
            .await
            .unwrap();

        let ancient = OffsetDateTime::now_utc() - time::Duration::days(200);
        let recent = OffsetDateTime::now_utc() - time::Duration::days(50);
        for (ts, n) in [(ancient, 1_i32), (recent, 2_i32)] {
            sqlx::query!(
                r#"INSERT INTO heartbeat_rollups
                   (monitor_id, bucket_start, up_count, down_count,
                    other_count, sample_count, avg_latency_ms)
                   VALUES ($1, date_trunc('hour', $2::timestamptz), $3, 0, 0, $3, 10.0)"#,
                m.id.0,
                ts,
                n,
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let stats = run_once(&pool).await.unwrap();
        assert_eq!(
            stats.rollups_deleted, 1,
            "the 200-day-old bucket is dropped"
        );

        let left: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM heartbeat_rollups WHERE monitor_id = $1",
            m.id.0
        )
        .fetch_one(&pool)
        .await
        .unwrap()
        .unwrap_or(0);
        assert_eq!(left, 1, "the 50-day-old bucket survives");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn daily_uptime_from_rollups_aggregates_per_day(pool: PgPool) {
        let m = crate::monitors::create(&pool, http_monitor("uphist"))
            .await
            .unwrap();

        // Seed three hourly buckets on day -100 (well past raw retention) and
        // two on day -101, so the daily aggregation must sum across buckets.
        // Day -100: 9 up / 10 samples → 90%. Day -101: 1 up / 4 → 25%.
        let day100 = OffsetDateTime::now_utc() - time::Duration::days(100);
        let day101 = OffsetDateTime::now_utc() - time::Duration::days(101);
        let seed = |ts: OffsetDateTime, up: i32, samples: i32| {
            let pool = pool.clone();
            let mid = m.id.0;
            async move {
                sqlx::query!(
                    r#"INSERT INTO heartbeat_rollups
                       (monitor_id, bucket_start, up_count, down_count,
                        other_count, sample_count, avg_latency_ms)
                       VALUES ($1, date_trunc('hour', $2::timestamptz),
                               $3, $4, 0, $5, 10.0)"#,
                    mid,
                    ts,
                    up,
                    samples - up,
                    samples,
                )
                .execute(&pool)
                .await
                .unwrap();
            }
        };
        // Distinct hours within day -100 so each is its own bucket.
        seed(day100, 4, 4).await;
        seed(day100 + time::Duration::hours(1), 3, 3).await;
        seed(day100 + time::Duration::hours(2), 2, 3).await;
        // Distinct hours within day -101.
        seed(day101, 1, 2).await;
        seed(day101 + time::Duration::hours(1), 0, 2).await;

        let since = OffsetDateTime::now_utc() - time::Duration::days(365);
        let until = OffsetDateTime::now_utc();
        let series = daily_uptime_from_rollups(&pool, m.id.0, since, until)
            .await
            .unwrap();
        assert_eq!(series.len(), 2, "two distinct days");
        // Oldest first: day -101 then day -100.
        assert_eq!(series[0].sample_count, 4);
        assert_eq!(series[0].up_count, 1);
        assert!((series[0].uptime_pct.unwrap() - 25.0).abs() < 1e-9);
        assert_eq!(series[1].sample_count, 10);
        assert_eq!(series[1].up_count, 9);
        assert!((series[1].uptime_pct.unwrap() - 90.0).abs() < 1e-9);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rollup_upsert_is_idempotent(pool: PgPool) {
        // Running the sweep twice must not double-count: the second sweep
        // finds the raw rows already deleted, so the bucket is unchanged.
        set_retention(&pool, 30, 400).await;
        let m = crate::monitors::create(&pool, http_monitor("idem"))
            .await
            .unwrap();
        const D90: i64 = 90 * 86_400;
        let hbs = vec![
            hb(m.id, MonitorStatus::Up, Some(100), D90),
            hb(m.id, MonitorStatus::Up, Some(100), D90 + 60),
        ];
        crate::heartbeats::insert_many(&pool, &hbs).await.unwrap();

        run_once(&pool).await.unwrap();
        run_once(&pool).await.unwrap();

        let since = OffsetDateTime::now_utc() - time::Duration::days(400);
        let until = OffsetDateTime::now_utc();
        let rollups = rollups_for_monitor(&pool, m.id.0, since, until)
            .await
            .unwrap();
        let total: i32 = rollups.iter().map(|r| r.sample_count).sum();
        assert_eq!(total, 2, "second sweep must not re-count folded rows");
    }
}
