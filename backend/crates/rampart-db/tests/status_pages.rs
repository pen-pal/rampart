//! Parity test for `rampart_db::status_pages::public_view`.
//!
//! `public_view` was rewritten from ~6 queries per monitor to five set-based
//! batch queries total (`monitors::public_fields_batch` +
//! `heartbeats::{uptime_pct,avg_latency_ms,daily_status,monthly_uptime}_batch`).
//! The assembled `PublicStatusPage` must stay byte-identical to the old
//! per-monitor output. This is the load-bearing parity guard: it seeds known
//! heartbeats across a few days and asserts every projected field (uptime,
//! avg latency, daily strip, monthly chips) against hand-computed values, and
//! covers the two output-parity edge cases — a monitor with NO heartbeats and
//! page ordering.

use rampart_core::ids::OrgId;
use rampart_core::monitor::NewMonitor;
use rampart_core::org::DEFAULT_ORG_ID;
use rampart_core::status_page::NewStatusPage;
use rampart_core::{Heartbeat, MonitorId, MonitorKind, MonitorStatus};
use rampart_db::{heartbeats, monitors, status_pages};
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
        check_cert: false,
        cert_expiry_days: 14,
    }
}

/// A heartbeat `secs_ago` before now. Latency only set for `up` rows (mirrors
/// real scheduler behaviour and exercises the avg-latency-of-up filter).
fn hb(monitor_id: MonitorId, status: MonitorStatus, latency: i32, secs_ago: i64) -> Heartbeat {
    Heartbeat {
        monitor_id,
        ts: OffsetDateTime::now_utc() - time::Duration::seconds(secs_ago),
        status,
        latency_ms: matches!(status, MonitorStatus::Up).then_some(latency),
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

const DAY: i64 = 86_400;

#[sqlx::test(migrations = "../../migrations")]
async fn public_view_batch_parity(pool: PgPool) {
    // ── Monitor A: mixed history across three days ────────────────────────
    // Today : 2 up (latency 100, 200), 1 down  → daily 'd', up-avg today only
    // 1d ago: 1 up                              → daily 'u'
    // 2d ago: 1 warn                            → daily 'w'
    // All four up heartbeats are within the 24h avg window? No — only today's
    // two are inside 86_400s. avg_latency_24h = (100 + 200) / 2 = 150.
    // uptime_90d over all 5 rows = up(3) / total(5) * 100 = 60.0.
    let a = monitors::create(&pool, http_monitor("svc-a"), def_org())
        .await
        .unwrap();
    monitors::set_status(&pool, a.id, MonitorStatus::Down)
        .await
        .unwrap();
    heartbeats::insert_many(
        &pool,
        &[
            hb(a.id, MonitorStatus::Up, 100, 60),           // today
            hb(a.id, MonitorStatus::Up, 200, 120),          // today
            hb(a.id, MonitorStatus::Down, 0, 180),          // today
            hb(a.id, MonitorStatus::Up, 999, DAY + 60),     // 1d ago (outside 24h avg window)
            hb(a.id, MonitorStatus::Warn, 0, 2 * DAY + 60), // 2d ago
        ],
    )
    .await
    .unwrap();

    // ── Monitor B: all up today ───────────────────────────────────────────
    // uptime_90d = 100.0; avg_24h = (40 + 60) / 2 = 50.0; daily today 'u'.
    let b = monitors::create(&pool, http_monitor("svc-b"), def_org())
        .await
        .unwrap();
    monitors::set_status(&pool, b.id, MonitorStatus::Up)
        .await
        .unwrap();
    heartbeats::insert_many(
        &pool,
        &[
            hb(b.id, MonitorStatus::Up, 40, 30),
            hb(b.id, MonitorStatus::Up, 60, 90),
        ],
    )
    .await
    .unwrap();

    // ── Monitor C: NO heartbeats at all (edge case) ───────────────────────
    // uptime_90d None, avg_24h None, daily strip all 'n', monthly all None.
    let c = monitors::create(&pool, http_monitor("svc-c"), def_org())
        .await
        .unwrap();
    monitors::set_status(&pool, c.id, MonitorStatus::Pending)
        .await
        .unwrap();

    // Attach in a deliberate order (B, A, C) to prove page order is preserved.
    let page = status_pages::create(
        &pool,
        NewStatusPage {
            slug: "parity-page".into(),
            title: "Parity Page".into(),
            description: None,
            theme: "auto".into(),
            custom_domain: None,
            logo_url: None,
            custom_css: None,
            password: None,
            monitor_ids: vec![b.id, a.id, c.id],
        },
        def_org(),
    )
    .await
    .unwrap();

    let view = status_pages::public_view(&pool, &page.slug).await.unwrap();

    // ── Top-level shape ───────────────────────────────────────────────────
    assert_eq!(view.slug, "parity-page");
    assert_eq!(view.monitors.len(), 3, "all three monitors projected");
    // Page order preserved: B, A, C.
    assert_eq!(view.monitors[0].name, "svc-b");
    assert_eq!(view.monitors[1].name, "svc-a");
    assert_eq!(view.monitors[2].name, "svc-c");

    // ── Monitor B (index 0) ───────────────────────────────────────────────
    let mb = &view.monitors[0];
    assert_eq!(mb.current_status, MonitorStatus::Up);
    assert_approx(mb.uptime_90d, Some(100.0), "B uptime_90d");
    assert_approx(mb.avg_latency_ms_24h, Some(50.0), "B avg latency");
    assert_eq!(mb.daily_status_90d.len(), 90, "90-char strip");
    assert_eq!(last_char(&mb.daily_status_90d), 'u', "B today all-up");

    // ── Monitor A (index 1) ───────────────────────────────────────────────
    let ma = &view.monitors[1];
    assert_eq!(ma.current_status, MonitorStatus::Down);
    assert_approx(ma.uptime_90d, Some(60.0), "A uptime_90d (3 up / 5 total)");
    assert_approx(ma.avg_latency_ms_24h, Some(150.0), "A avg latency (24h up)");
    assert_eq!(ma.daily_status_90d.len(), 90);
    let a_strip: Vec<char> = ma.daily_status_90d.chars().collect();
    assert_eq!(a_strip[89], 'd', "A today: any-down → 'd'");
    assert_eq!(a_strip[88], 'u', "A 1d ago: all up → 'u'");
    assert_eq!(a_strip[87], 'w', "A 2d ago: warn → 'w'");
    assert_eq!(a_strip[86], 'n', "A 3d ago: no data → 'n'");

    // ── Monitor C (index 2) — the no-heartbeat edge case ──────────────────
    let mc = &view.monitors[2];
    assert_eq!(mc.current_status, MonitorStatus::Pending);
    assert!(mc.uptime_90d.is_none(), "C uptime None (no heartbeats)");
    assert!(mc.avg_latency_ms_24h.is_none(), "C avg latency None");
    assert_eq!(mc.daily_status_90d.len(), 90);
    assert!(
        mc.daily_status_90d.chars().all(|ch| ch == 'n'),
        "C daily strip all 'n'"
    );

    // ── Monthly chips: 12 dense points, oldest first ──────────────────────
    for m in &view.monitors {
        assert_eq!(m.monthly_uptime_12mo.len(), 12, "12 monthly points");
        for w in m.monthly_uptime_12mo.windows(2) {
            assert!(
                w[0].year_month < w[1].year_month,
                "months oldest-first, strictly increasing"
            );
        }
    }
    // C has no heartbeats → every monthly chip is None.
    assert!(
        mc.monthly_uptime_12mo
            .iter()
            .all(|p| p.uptime_pct.is_none()),
        "C monthly all None"
    );

    // ── Full parity vs. the per-monitor fns (date-boundary-robust) ────────
    // The batch path must produce EXACTLY what calling the original
    // per-monitor rollups would. This catches any drift (incl. month/day
    // boundary cases the hand-computed asserts above could miss).
    for (idx, mid) in [b.id, a.id, c.id].iter().enumerate() {
        let pm = &view.monitors[idx];

        let want_uptime = heartbeats::uptime_pct(&pool, *mid, 90 * DAY)
            .await
            .unwrap()
            .map(|v| v as f32);
        assert_approx(pm.uptime_90d, want_uptime, "uptime parity");

        let want_avg = heartbeats::avg_latency_ms(&pool, *mid, DAY)
            .await
            .unwrap()
            .map(|v| v as f32);
        assert_approx(pm.avg_latency_ms_24h, want_avg, "avg latency parity");

        let want_daily =
            String::from_utf8(heartbeats::daily_status(&pool, *mid, 90).await.unwrap()).unwrap();
        assert_eq!(pm.daily_status_90d, want_daily, "daily strip parity");

        let want_monthly = heartbeats::monthly_uptime(&pool, *mid, 12).await.unwrap();
        assert_eq!(
            pm.monthly_uptime_12mo.len(),
            want_monthly.len(),
            "monthly len parity"
        );
        for (got, exp) in pm.monthly_uptime_12mo.iter().zip(want_monthly.iter()) {
            assert_eq!(got.year_month, exp.year_month, "monthly year_month parity");
            assert_approx(got.uptime_pct, exp.uptime_pct, "monthly pct parity");
        }
    }
}

/// Last char of an ASCII status strip.
fn last_char(s: &str) -> char {
    s.chars().next_back().expect("non-empty strip")
}

/// Assert two `Option<f32>` percentages match within float tolerance.
fn assert_approx(got: Option<f32>, want: Option<f32>, label: &str) {
    match (got, want) {
        (Some(g), Some(w)) => assert!((g - w).abs() < 1e-4, "{label}: got {g}, expected {w}"),
        (None, None) => {}
        _ => panic!("{label}: got {got:?}, expected {want:?}"),
    }
}
