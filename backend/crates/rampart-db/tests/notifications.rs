//! Integration tests for `rampart_db::notifications`.

use rampart_core::monitor::NewMonitor;
use rampart_core::{ChannelKind, MonitorKind};
use rampart_db::monitors;
use rampart_db::notifications::{
    attach, counts_per_monitor, create, delete, detach, for_monitor, get, list, update,
    NewNotification, UpdateNotification,
};
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
    }
}

fn webhook_channel(name: &str) -> NewNotification {
    NewNotification {
        kind: ChannelKind::Webhook,
        name: name.into(),
        config: serde_json::json!({"url": "https://example.com/hook"}),
        active: true,
        template_id: None,
        cooldown_seconds: 0,
    }
}

fn empty_update() -> UpdateNotification {
    UpdateNotification {
        name: None,
        config: None,
        active: None,
        template_id: None,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_empty_initially(pool: PgPool) {
    assert!(list(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_and_get(pool: PgPool) {
    let n = create(&pool, webhook_channel("ch")).await.unwrap();
    assert_eq!(n.name, "ch");
    assert_eq!(n.kind, ChannelKind::Webhook);
    assert!(n.active);
    assert!(n.template_id.is_none());

    let g = get(&pool, n.id).await.unwrap();
    assert_eq!(g.id, n.id);
}

#[sqlx::test(migrations = "../../migrations")]
async fn update_renames_and_disables(pool: PgPool) {
    let n = create(&pool, webhook_channel("orig")).await.unwrap();
    let mut p = empty_update();
    p.name = Some("renamed".into());
    p.active = Some(false);
    let patched = update(&pool, n.id, p).await.unwrap();
    assert_eq!(patched.name, "renamed");
    assert!(!patched.active);
}

#[sqlx::test(migrations = "../../migrations")]
async fn update_can_set_and_clear_template(pool: PgPool) {
    use rampart_db::templates::{self, NewTemplate};
    let t = templates::create(
        &pool,
        NewTemplate {
            name: "T".into(),
            channel_kinds: vec![],
            event_kind: "monitor_down".into(),
            subject_template: None,
            body_template: "body".into(),
            is_default: false,
        },
    )
    .await
    .unwrap();
    let n = create(&pool, webhook_channel("templ")).await.unwrap();

    // Set
    let mut p = empty_update();
    p.template_id = Some(Some(t.id));
    let patched = update(&pool, n.id, p).await.unwrap();
    assert_eq!(patched.template_id, Some(t.id));

    // Clear
    let mut p = empty_update();
    p.template_id = Some(None);
    let cleared = update(&pool, n.id, p).await.unwrap();
    assert!(cleared.template_id.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn attach_detach_round_trip(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("m1")).await.unwrap();
    let c = create(&pool, webhook_channel("ch")).await.unwrap();

    let before = for_monitor(&pool, m.id).await.unwrap();
    assert!(before.is_empty());

    attach(&pool, m.id, c.id).await.unwrap();
    let after = for_monitor(&pool, m.id).await.unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, c.id);

    detach(&pool, m.id, c.id).await.unwrap();
    assert!(for_monitor(&pool, m.id).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn attach_is_idempotent(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("idem")).await.unwrap();
    let c = create(&pool, webhook_channel("ch")).await.unwrap();
    attach(&pool, m.id, c.id).await.unwrap();
    attach(&pool, m.id, c.id).await.unwrap();
    assert_eq!(
        for_monitor(&pool, m.id).await.unwrap().len(),
        1,
        "second attach should be a noop"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn disabled_channels_excluded_from_for_monitor(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("dis")).await.unwrap();
    let c = create(&pool, webhook_channel("ch")).await.unwrap();
    attach(&pool, m.id, c.id).await.unwrap();
    assert_eq!(for_monitor(&pool, m.id).await.unwrap().len(), 1);

    let mut p = empty_update();
    p.active = Some(false);
    update(&pool, c.id, p).await.unwrap();

    assert!(
        for_monitor(&pool, m.id).await.unwrap().is_empty(),
        "disabled channels should not be returned to the dispatcher"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn counts_per_monitor_excludes_disabled(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("counts"))
        .await
        .unwrap();
    let c1 = create(&pool, webhook_channel("c1")).await.unwrap();
    let c2 = create(&pool, webhook_channel("c2")).await.unwrap();
    attach(&pool, m.id, c1.id).await.unwrap();
    attach(&pool, m.id, c2.id).await.unwrap();

    // disable c2
    let mut p = empty_update();
    p.active = Some(false);
    update(&pool, c2.id, p).await.unwrap();

    let counts = counts_per_monitor(&pool).await.unwrap();
    let row = counts
        .iter()
        .find(|r| r.monitor_id == m.id)
        .expect("monitor in counts");
    assert_eq!(row.count, 1, "disabled channels should not be counted");
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_channel_cascades_attachments(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("cas")).await.unwrap();
    let c = create(&pool, webhook_channel("ch")).await.unwrap();
    attach(&pool, m.id, c.id).await.unwrap();
    delete(&pool, c.id).await.unwrap();
    // ON DELETE CASCADE on monitor_notifications.notification_id should
    // clear the row; monitor's channel list now empty.
    assert!(for_monitor(&pool, m.id).await.unwrap().is_empty());
}
