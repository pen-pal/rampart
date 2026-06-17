//! Integration tests for `rampart_db::heartbeats`.

use rampart_core::ids::OrgId;
use rampart_core::monitor::NewMonitor;
use rampart_core::org::DEFAULT_ORG_ID;
use rampart_core::{Heartbeat, MonitorKind, MonitorStatus};
use rampart_db::heartbeats::{insert_many, recent_for_monitor, recent_per_monitor, summary_window};
use rampart_db::monitors;
use sqlx::PgPool;
use time::OffsetDateTime;

fn def_org() -> OrgId {
    OrgId::from_uuid(DEFAULT_ORG_ID)
}

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
    latency: i32,
    secs_ago: i64,
) -> Heartbeat {
    Heartbeat {
        monitor_id,
        ts: OffsetDateTime::now_utc() - time::Duration::seconds(secs_ago),
        status,
        latency_ms: Some(latency),
        status_code: Some(if matches!(status, MonitorStatus::Up) {
            200
        } else {
            503
        }),
        msg: None,
        retries: 0,
        important: false,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_many_empty_is_noop(pool: PgPool) {
    insert_many(&pool, &[]).await.unwrap();
    // No assertion needed — should not error.
}

#[sqlx::test(migrations = "../../migrations")]
async fn round_trip_single_heartbeat(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("rt")).await.unwrap();
    let h = hb(m.id, MonitorStatus::Up, 87, 1);
    insert_many(&pool, std::slice::from_ref(&h)).await.unwrap();

    let recent = recent_for_monitor(&pool, m.id, 10).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].status, h.status);
    assert_eq!(recent[0].latency_ms, h.latency_ms);
    assert_eq!(recent[0].status_code, h.status_code);
}

#[sqlx::test(migrations = "../../migrations")]
async fn recent_for_monitor_orders_descending_by_ts(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("ord")).await.unwrap();
    let hbs = vec![
        hb(m.id, MonitorStatus::Up, 50, 300), // oldest
        hb(m.id, MonitorStatus::Down, 100, 200),
        hb(m.id, MonitorStatus::Up, 60, 100), // newest
    ];
    insert_many(&pool, &hbs).await.unwrap();

    let recent = recent_for_monitor(&pool, m.id, 10).await.unwrap();
    assert_eq!(recent.len(), 3);
    // Most recent first.
    assert_eq!(recent[0].status, MonitorStatus::Up);
    assert_eq!(recent[1].status, MonitorStatus::Down);
    assert_eq!(recent[2].status, MonitorStatus::Up);
}

#[sqlx::test(migrations = "../../migrations")]
async fn limit_respected(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("lim")).await.unwrap();
    let hbs: Vec<_> = (0..50)
        .map(|i| hb(m.id, MonitorStatus::Up, 50, i))
        .collect();
    insert_many(&pool, &hbs).await.unwrap();
    let r = recent_for_monitor(&pool, m.id, 7).await.unwrap();
    assert_eq!(r.len(), 7);
}

#[sqlx::test(migrations = "../../migrations")]
async fn summary_window_computes_uptime_and_avg_latency(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("sum")).await.unwrap();
    let hbs = vec![
        hb(m.id, MonitorStatus::Up, 50, 10),
        hb(m.id, MonitorStatus::Up, 100, 20),
        hb(m.id, MonitorStatus::Down, 0, 30),
        hb(m.id, MonitorStatus::Up, 150, 40),
    ];
    insert_many(&pool, &hbs).await.unwrap();

    let rollup = summary_window(&pool, 3600, def_org()).await.unwrap();
    let row = rollup
        .iter()
        .find(|r| r.monitor_id == m.id)
        .expect("monitor in summary");
    assert_eq!(row.total, 4);
    assert_eq!(row.up, 3);
    // Avg latency over up-only: (50 + 100 + 150) / 3 = 100
    let avg = row
        .avg_latency_ms
        .expect("avg latency present when up heartbeats exist");
    assert!(
        (avg - 100.0).abs() < 1e-6,
        "avg latency = {avg}, expected 100"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn summary_window_excludes_outside_window(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("win")).await.unwrap();
    insert_many(
        &pool,
        &[
            hb(m.id, MonitorStatus::Up, 50, 60),   // 60s ago — inside
            hb(m.id, MonitorStatus::Up, 50, 4000), // ~67min ago — outside 30min window
        ],
    )
    .await
    .unwrap();

    let rollup = summary_window(&pool, 1800, def_org()).await.unwrap(); // 30 min
    let row = rollup
        .iter()
        .find(|r| r.monitor_id == m.id)
        .expect("monitor in summary");
    assert_eq!(
        row.total, 1,
        "only the inside-window heartbeat should count"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn recent_per_monitor_groups_correctly(pool: PgPool) {
    let a = monitors::create(&pool, http_monitor("rpm-a"))
        .await
        .unwrap();
    let b = monitors::create(&pool, http_monitor("rpm-b"))
        .await
        .unwrap();
    insert_many(
        &pool,
        &[
            hb(a.id, MonitorStatus::Up, 50, 10),
            hb(a.id, MonitorStatus::Up, 60, 20),
            hb(b.id, MonitorStatus::Up, 70, 5),
        ],
    )
    .await
    .unwrap();

    let all = recent_per_monitor(&pool, 10, def_org()).await.unwrap();
    let a_count = all.iter().filter(|h| h.monitor_id == a.id).count();
    let b_count = all.iter().filter(|h| h.monitor_id == b.id).count();
    assert_eq!(a_count, 2);
    assert_eq!(b_count, 1);
}

mod tests {
    //! MTBF / MTTR + SLO-error-budget rollup tests. Kept in their own
    //! module so cargo-test filters like `heartbeats::tests::mtbf*` and
    //! `heartbeats::tests::error_budget*` match just these cases without
    //! dragging in every other heartbeat integration test.

    use super::{hb, http_monitor};
    use rampart_core::MonitorStatus;
    use rampart_db::heartbeats::{error_budget, error_budget_burndown, insert_many, mtbf_mttr};
    use rampart_db::monitors;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn mtbf_empty_history(pool: PgPool) {
        let m = monitors::create(&pool, http_monitor("mtbf-empty"))
            .await
            .unwrap();
        let r = mtbf_mttr(&pool, m.id, 86_400).await.unwrap();
        assert_eq!(r.downtime_events, 0);
        assert!(r.mtbf_secs.is_none());
        assert!(r.mttr_secs.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mtbf_no_failures_when_always_up(pool: PgPool) {
        let m = monitors::create(&pool, http_monitor("mtbf-allup"))
            .await
            .unwrap();
        let hbs: Vec<_> = (0..5)
            .map(|i| hb(m.id, MonitorStatus::Up, 50, (5 - i) * 60))
            .collect();
        insert_many(&pool, &hbs).await.unwrap();
        let r = mtbf_mttr(&pool, m.id, 86_400).await.unwrap();
        assert_eq!(r.downtime_events, 0);
        assert!(r.mtbf_secs.is_none(), "no failures → MTBF undefined");
        assert!(r.mttr_secs.is_none(), "no recoveries → MTTR undefined");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mtbf_basic_failure_and_recovery(pool: PgPool) {
        // Timeline (oldest → newest), in seconds-ago at insert time:
        //   400s-ago  up
        //   300s-ago  up    (segment 400→300 = 100s up)
        //   200s-ago  down  (segment 300→200 = 100s up, transition up→down)
        //   100s-ago  up    (segment 200→100 = 100s down, transition down→up)
        //     0s-ago  up    (segment 100→0   = 100s up)
        //
        // total_up_secs   = 100 + 100 + 100 = 300 → MTBF = 300 / 1 failure  = 300
        // total_down_secs = 100               → MTTR = 100 / 1 recovery   = 100
        let m = monitors::create(&pool, http_monitor("mtbf-basic"))
            .await
            .unwrap();
        let hbs = vec![
            hb(m.id, MonitorStatus::Up, 50, 400),
            hb(m.id, MonitorStatus::Up, 50, 300),
            hb(m.id, MonitorStatus::Down, 0, 200),
            hb(m.id, MonitorStatus::Up, 50, 100),
            hb(m.id, MonitorStatus::Up, 50, 0),
        ];
        insert_many(&pool, &hbs).await.unwrap();

        let r = mtbf_mttr(&pool, m.id, 86_400).await.unwrap();
        assert_eq!(r.downtime_events, 1, "one up→down transition");
        assert_eq!(r.mtbf_secs, Some(300));
        assert_eq!(r.mttr_secs, Some(100));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn mtbf_window_excludes_older_heartbeats(pool: PgPool) {
        // One failure inside the 1h window, one ancient failure outside
        // it. Only the in-window one should contribute.
        let m = monitors::create(&pool, http_monitor("mtbf-window"))
            .await
            .unwrap();
        let hbs = vec![
            // Outside (~2h ago): up→down→up. Should be excluded.
            hb(m.id, MonitorStatus::Up, 50, 8000),
            hb(m.id, MonitorStatus::Down, 0, 7900),
            hb(m.id, MonitorStatus::Up, 50, 7800),
            // Inside (last 1h): up→down→up.
            hb(m.id, MonitorStatus::Up, 50, 2400),
            hb(m.id, MonitorStatus::Up, 50, 2100),
            hb(m.id, MonitorStatus::Down, 0, 1800),
            hb(m.id, MonitorStatus::Up, 50, 1500),
        ];
        insert_many(&pool, &hbs).await.unwrap();

        let r = mtbf_mttr(&pool, m.id, 3600).await.unwrap();
        assert_eq!(
            r.downtime_events, 1,
            "only the in-window failure should be counted"
        );
        assert!(r.mtbf_secs.is_some());
        assert!(r.mttr_secs.is_some());
    }

    // ── SLO error-budget tests ────────────────────────────────────────────
    // window_days=30 → window_seconds = 30 * 86_400 = 2_592_000s.
    //   target 99.9%  → allowed = (0.1 / 100) * 2_592_000 = 2_592s ≈ 43m
    //   target 99.0%  → allowed = (1   / 100) * 2_592_000 = 25_920s ≈ 7h12m
    //   target 100.0% → allowed = 0  (no error budget by definition)

    #[sqlx::test(migrations = "../../migrations")]
    async fn error_budget_empty_history_full_budget(pool: PgPool) {
        let m = monitors::create(&pool, http_monitor("eb-empty"))
            .await
            .unwrap();
        let b = error_budget(&pool, m.id, 30, 99.9).await.unwrap();
        assert_eq!(b.window_days, 30);
        assert!((b.target_pct - 99.9).abs() < 1e-9);
        assert_eq!(b.allowed_downtime_secs, 2_592);
        assert_eq!(b.used_downtime_secs, 0);
        assert_eq!(b.remaining_downtime_secs, 2_592);
        assert!(
            (b.remaining_pct - 100.0).abs() < 1e-9,
            "no downtime yet → 100% budget remaining, got {}",
            b.remaining_pct
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn error_budget_consumed_by_down_segment(pool: PgPool) {
        // 200s down segment inside the window. Target 99.0% over 30d
        // allows 25_920s of downtime; 200s used → 25_720s remaining.
        let m = monitors::create(&pool, http_monitor("eb-down"))
            .await
            .unwrap();
        let hbs = vec![
            hb(m.id, MonitorStatus::Up, 50, 1000),
            hb(m.id, MonitorStatus::Down, 0, 800), // down segment: 800→600 = 200s
            hb(m.id, MonitorStatus::Up, 50, 600),
            hb(m.id, MonitorStatus::Up, 50, 0),
        ];
        insert_many(&pool, &hbs).await.unwrap();

        let b = error_budget(&pool, m.id, 30, 99.0).await.unwrap();
        assert_eq!(b.allowed_downtime_secs, 25_920);
        assert_eq!(b.used_downtime_secs, 200);
        assert_eq!(b.remaining_downtime_secs, 25_720);
        let expected_pct = 25_720.0 / 25_920.0 * 100.0;
        assert!(
            (b.remaining_pct - expected_pct).abs() < 1e-6,
            "remaining_pct = {}, expected {}",
            b.remaining_pct,
            expected_pct
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn error_budget_exhausted_clamps_to_zero(pool: PgPool) {
        // Down segment far longer than the allowed budget. remaining must
        // clamp to 0 (not go negative) and remaining_pct must be 0.
        let m = monitors::create(&pool, http_monitor("eb-blown"))
            .await
            .unwrap();
        let hbs = vec![
            // Place the long-down segment recently so the whole 10_000s
            // sits inside any sensible window (we use 1 day = 86_400s).
            hb(m.id, MonitorStatus::Up, 50, 12_000),
            hb(m.id, MonitorStatus::Down, 0, 11_000), // 11_000 → 1_000 = 10_000s down
            hb(m.id, MonitorStatus::Up, 50, 1_000),
            hb(m.id, MonitorStatus::Up, 50, 0),
        ];
        insert_many(&pool, &hbs).await.unwrap();

        // 1-day window @ 99.9% → allowed = 86.4s ≈ 86s, but we burned
        // 10_000s. Budget is gone.
        let b = error_budget(&pool, m.id, 1, 99.9).await.unwrap();
        assert!(b.allowed_downtime_secs < b.used_downtime_secs);
        assert_eq!(
            b.remaining_downtime_secs, 0,
            "remaining must clamp at 0 when over-burned"
        );
        assert!(
            b.remaining_pct.abs() < 1e-9,
            "remaining_pct should be 0, got {}",
            b.remaining_pct
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn error_budget_one_hundred_pct_target_handles_zero_allowed(pool: PgPool) {
        // A 100% SLO leaves no budget by definition — but the response
        // must still serialise as finite numbers (not NaN), so the
        // helper reports remaining_pct = 100.0 when there's no downtime
        // to divide against an allowance of zero.
        let m = monitors::create(&pool, http_monitor("eb-100pct"))
            .await
            .unwrap();
        let b = error_budget(&pool, m.id, 30, 100.0).await.unwrap();
        assert_eq!(b.allowed_downtime_secs, 0);
        assert_eq!(b.used_downtime_secs, 0);
        assert_eq!(b.remaining_downtime_secs, 0);
        assert!(b.remaining_pct.is_finite());
        assert!(
            (b.remaining_pct - 100.0).abs() < 1e-9,
            "100% target with no downtime → 100% remaining, got {}",
            b.remaining_pct
        );
    }

    // ── SLO error-budget burn-down tests ──────────────────────────────────
    #[sqlx::test(migrations = "../../migrations")]
    async fn burndown_empty_history_full_budget_every_day(pool: PgPool) {
        // No heartbeats → budget never depletes: every point sits at full
        // budget (100% remaining, 0 cumulative down). One point per day in
        // the window, oldest first.
        let m = monitors::create(&pool, http_monitor("bd-empty"))
            .await
            .unwrap();
        let pts = error_budget_burndown(&pool, m.id, 30, 99.9).await.unwrap();
        // 30-day window spanning [today-30d, today] inclusive → 31 points.
        assert_eq!(pts.len(), 31, "one dense point per day across the window");
        // Oldest first: days strictly increasing.
        for w in pts.windows(2) {
            assert!(w[0].day < w[1].day, "points must be oldest-first by day");
        }
        for p in &pts {
            assert_eq!(p.cumulative_down_secs, 0);
            assert_eq!(p.budget_remaining_secs, 2_592);
            assert!(
                (p.budget_remaining_pct - 100.0).abs() < 1e-9,
                "no downtime → 100% remaining every day, got {}",
                p.budget_remaining_pct
            );
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn burndown_down_segment_depletes_monotonically(pool: PgPool) {
        // A single down segment depletes budget; from that point on the
        // remaining budget never recovers. Remaining-% is monotonically
        // non-increasing across the whole series, and the final cumulative
        // matches `error_budget`'s used-seconds for the same window.
        let m = monitors::create(&pool, http_monitor("bd-down"))
            .await
            .unwrap();
        let hbs = vec![
            hb(m.id, MonitorStatus::Up, 50, 1000),
            hb(m.id, MonitorStatus::Down, 0, 800), // 800→600 = 200s down
            hb(m.id, MonitorStatus::Up, 50, 600),
            hb(m.id, MonitorStatus::Up, 50, 0),
        ];
        insert_many(&pool, &hbs).await.unwrap();

        let pts = error_budget_burndown(&pool, m.id, 30, 99.0).await.unwrap();
        assert!(!pts.is_empty());

        // Monotonically non-increasing remaining-% and non-decreasing
        // cumulative down-seconds.
        for w in pts.windows(2) {
            assert!(
                w[1].budget_remaining_pct <= w[0].budget_remaining_pct + 1e-9,
                "remaining-% must never increase: {} → {}",
                w[0].budget_remaining_pct,
                w[1].budget_remaining_pct
            );
            assert!(
                w[1].cumulative_down_secs >= w[0].cumulative_down_secs,
                "cumulative down-seconds must never decrease"
            );
        }

        // The down segment lands today, so the final point carries all 200s
        // and lines up with the point-in-time fuel gauge.
        let last = pts.last().unwrap();
        assert_eq!(last.cumulative_down_secs, 200);
        let eb = error_budget(&pool, m.id, 30, 99.0).await.unwrap();
        assert_eq!(last.budget_remaining_secs, eb.remaining_downtime_secs);
        assert!(
            (last.budget_remaining_pct - eb.remaining_pct).abs() < 1e-6,
            "final burn-down point must match error_budget: {} vs {}",
            last.budget_remaining_pct,
            eb.remaining_pct
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_is_idempotent_on_conflict(pool: PgPool) {
    // Same (monitor_id, ts) → ON CONFLICT DO NOTHING. Caller can retry
    // without dup-key crash; row count unchanged.
    let m = monitors::create(&pool, http_monitor("idem")).await.unwrap();
    let h = hb(m.id, MonitorStatus::Up, 50, 10);
    insert_many(&pool, std::slice::from_ref(&h)).await.unwrap();
    insert_many(&pool, std::slice::from_ref(&h)).await.unwrap();
    let r = recent_for_monitor(&pool, m.id, 10).await.unwrap();
    assert_eq!(r.len(), 1, "duplicate ts should be deduped");
}
