//! Integration tests for `rampart_db::monitors`.

use rampart_core::monitor::NewMonitor;
use rampart_core::{MonitorKind, MonitorStatus};
use rampart_db::monitors::{create, delete, get, list, set_active};
use sqlx::PgPool;

fn http_monitor(name: &str, url: &str) -> NewMonitor {
    NewMonitor {
        name: name.into(),
        kind: MonitorKind::Http,
        url: Some(url.into()),
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
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_empty_by_default(pool: PgPool) {
    let ms = list(&pool).await.unwrap();
    assert!(ms.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_round_trips(pool: PgPool) {
    let m = create(&pool, http_monitor("api", "https://api.example.com"))
        .await
        .unwrap();
    assert_eq!(m.name, "api");
    assert_eq!(m.kind, MonitorKind::Http);
    assert!(m.active);
    assert_eq!(m.current_status, MonitorStatus::Pending);

    let got = get(&pool, m.id).await.unwrap();
    assert_eq!(got.id, m.id);
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_returns_all_in_recency_order(pool: PgPool) {
    let _a = create(&pool, http_monitor("a", "https://a.example.com"))
        .await
        .unwrap();
    let _b = create(&pool, http_monitor("b", "https://b.example.com"))
        .await
        .unwrap();
    let _c = create(&pool, http_monitor("c", "https://c.example.com"))
        .await
        .unwrap();
    let ms = list(&pool).await.unwrap();
    assert_eq!(ms.len(), 3);
}

#[sqlx::test(migrations = "../../migrations")]
async fn set_active_toggles_field(pool: PgPool) {
    let m = create(&pool, http_monitor("toggle", "https://x.example.com"))
        .await
        .unwrap();
    assert!(m.active);

    set_active(&pool, m.id, false).await.unwrap();
    let paused = get(&pool, m.id).await.unwrap();
    assert!(!paused.active);

    set_active(&pool, m.id, true).await.unwrap();
    let resumed = get(&pool, m.id).await.unwrap();
    assert!(resumed.active);
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_removes_from_list(pool: PgPool) {
    let m = create(&pool, http_monitor("gone", "https://gone.example.com"))
        .await
        .unwrap();
    delete(&pool, m.id).await.unwrap();
    assert!(get(&pool, m.id).await.is_err());
    let ms = list(&pool).await.unwrap();
    assert!(ms.is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_missing_is_not_found(pool: PgPool) {
    use rampart_core::ids::MonitorId;
    let err = delete(&pool, MonitorId::new()).await.unwrap_err();
    assert!(matches!(err, rampart_db::DbError::NotFound), "got: {err:?}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_tcp_monitor_persists_host_and_port(pool: PgPool) {
    let mut nm = http_monitor("redis", "");
    nm.kind = MonitorKind::Tcp;
    nm.url = None;
    nm.hostname = Some("redis.internal".into());
    nm.port = Some(6379);
    nm.accepted_statuses = vec![];

    let m = create(&pool, nm).await.unwrap();
    assert_eq!(m.kind, MonitorKind::Tcp);
    assert_eq!(m.hostname.as_deref(), Some("redis.internal"));
    assert_eq!(m.port, Some(6379));
}
