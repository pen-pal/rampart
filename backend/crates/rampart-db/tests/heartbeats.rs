//! Integration tests for `rampart_db::heartbeats`.

use rampart_core::monitor::NewMonitor;
use rampart_core::{Heartbeat, MonitorKind, MonitorStatus};
use rampart_db::heartbeats::{insert_many, recent_for_monitor, recent_per_monitor, summary_window};
use rampart_db::monitors;
use sqlx::PgPool;
use time::OffsetDateTime;

fn http_monitor(name: &str) -> NewMonitor {
    NewMonitor {
        name: name.into(),
        kind: MonitorKind::Http,
        url:  Some(format!("https://{name}.example.com")),
        hostname: None, port: None, config: serde_json::Value::Null,
        interval_seconds: 60, timeout_seconds: 10, max_retries: 0,
        retry_interval_sec: 60, resend_interval_sec: 0, upside_down: false,
        http_method: "GET".into(), http_body: None, http_headers: None,
        accepted_statuses: vec![200], follow_redirect: true, ignore_tls: false,
        proxy_id: None,
    }
}

fn hb(monitor_id: rampart_core::MonitorId, status: MonitorStatus, latency: i32, secs_ago: i64) -> Heartbeat {
    Heartbeat {
        monitor_id,
        ts:          OffsetDateTime::now_utc() - time::Duration::seconds(secs_ago),
        status,
        latency_ms:  Some(latency),
        status_code: Some(if matches!(status, MonitorStatus::Up) { 200 } else { 503 }),
        msg:         None,
        retries:     0,
        important:   false,
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
    insert_many(&pool, &[h.clone()]).await.unwrap();

    let recent = recent_for_monitor(&pool, m.id, 10).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].status,      h.status);
    assert_eq!(recent[0].latency_ms,  h.latency_ms);
    assert_eq!(recent[0].status_code, h.status_code);
}

#[sqlx::test(migrations = "../../migrations")]
async fn recent_for_monitor_orders_descending_by_ts(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("ord")).await.unwrap();
    let hbs = vec![
        hb(m.id, MonitorStatus::Up,   50,  300),  // oldest
        hb(m.id, MonitorStatus::Down, 100, 200),
        hb(m.id, MonitorStatus::Up,   60,  100),  // newest
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
    let hbs: Vec<_> = (0..50).map(|i| hb(m.id, MonitorStatus::Up, 50, i)).collect();
    insert_many(&pool, &hbs).await.unwrap();
    let r = recent_for_monitor(&pool, m.id, 7).await.unwrap();
    assert_eq!(r.len(), 7);
}

#[sqlx::test(migrations = "../../migrations")]
async fn summary_window_computes_uptime_and_avg_latency(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("sum")).await.unwrap();
    let hbs = vec![
        hb(m.id, MonitorStatus::Up,   50,  10),
        hb(m.id, MonitorStatus::Up,  100,  20),
        hb(m.id, MonitorStatus::Down,  0,  30),
        hb(m.id, MonitorStatus::Up,  150,  40),
    ];
    insert_many(&pool, &hbs).await.unwrap();

    let rollup = summary_window(&pool, 3600).await.unwrap();
    let row = rollup.iter().find(|r| r.monitor_id == m.id).expect("monitor in summary");
    assert_eq!(row.total, 4);
    assert_eq!(row.up,    3);
    // Avg latency over up-only: (50 + 100 + 150) / 3 = 100
    let avg = row.avg_latency_ms.expect("avg latency present when up heartbeats exist");
    assert!((avg - 100.0).abs() < 1e-6, "avg latency = {avg}, expected 100");
}

#[sqlx::test(migrations = "../../migrations")]
async fn summary_window_excludes_outside_window(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("win")).await.unwrap();
    insert_many(&pool, &[
        hb(m.id, MonitorStatus::Up, 50, 60),       // 60s ago — inside
        hb(m.id, MonitorStatus::Up, 50, 4000),     // ~67min ago — outside 30min window
    ]).await.unwrap();

    let rollup = summary_window(&pool, 1800).await.unwrap();  // 30 min
    let row = rollup.iter().find(|r| r.monitor_id == m.id).expect("monitor in summary");
    assert_eq!(row.total, 1, "only the inside-window heartbeat should count");
}

#[sqlx::test(migrations = "../../migrations")]
async fn recent_per_monitor_groups_correctly(pool: PgPool) {
    let a = monitors::create(&pool, http_monitor("rpm-a")).await.unwrap();
    let b = monitors::create(&pool, http_monitor("rpm-b")).await.unwrap();
    insert_many(&pool, &[
        hb(a.id, MonitorStatus::Up, 50, 10),
        hb(a.id, MonitorStatus::Up, 60, 20),
        hb(b.id, MonitorStatus::Up, 70, 5),
    ]).await.unwrap();

    let all = recent_per_monitor(&pool, 10).await.unwrap();
    let a_count = all.iter().filter(|h| h.monitor_id == a.id).count();
    let b_count = all.iter().filter(|h| h.monitor_id == b.id).count();
    assert_eq!(a_count, 2);
    assert_eq!(b_count, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_is_idempotent_on_conflict(pool: PgPool) {
    // Same (monitor_id, ts) → ON CONFLICT DO NOTHING. Caller can retry
    // without dup-key crash; row count unchanged.
    let m = monitors::create(&pool, http_monitor("idem")).await.unwrap();
    let h = hb(m.id, MonitorStatus::Up, 50, 10);
    insert_many(&pool, &[h.clone()]).await.unwrap();
    insert_many(&pool, &[h.clone()]).await.unwrap();
    let r = recent_for_monitor(&pool, m.id, 10).await.unwrap();
    assert_eq!(r.len(), 1, "duplicate ts should be deduped");
}
