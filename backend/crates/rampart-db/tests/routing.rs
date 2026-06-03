//! Integration tests for the tag-based channel routing resolver.
//!
//! Covers each inclusion path (explicit attach, monitor-tag match,
//! folder-tag match, folder-level attach) and the exclusion override.

use rampart_core::monitor::NewMonitor;
use rampart_core::monitor_group::NewMonitorGroup;
use rampart_core::tag::NewTag;
use rampart_core::{ChannelKind, MonitorKind};
use rampart_db::notifications::NewNotification;
use rampart_db::{monitor_groups, monitors, notifications, routing, tags};
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

fn channel(name: &str) -> NewNotification {
    NewNotification {
        kind: ChannelKind::Webhook,
        name: name.into(),
        config: serde_json::json!({ "url": "https://example.com/hook" }),
        active: true,
        template_id: None,
        cooldown_seconds: 0,
    }
}

async fn mk_tag(pool: &PgPool, name: &str) -> rampart_core::ids::TagId {
    tags::create(
        pool,
        NewTag {
            name: name.into(),
            color: "#14b8a6".into(),
        },
    )
    .await
    .unwrap()
    .id
}

fn names(chans: &[notifications::Notification]) -> Vec<String> {
    let mut v: Vec<String> = chans.iter().map(|c| c.name.clone()).collect();
    v.sort();
    v
}

#[sqlx::test(migrations = "../../migrations")]
async fn explicit_attach_resolves(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("m1")).await.unwrap();
    let c = notifications::create(&pool, channel("explicit"))
        .await
        .unwrap();
    notifications::attach(&pool, m.id, c.id).await.unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["explicit"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn monitor_tag_match_resolves(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("m1")).await.unwrap();
    let c = notifications::create(&pool, channel("prod-pager"))
        .await
        .unwrap();
    let prod = mk_tag(&pool, "prod").await;
    // Monitor tagged prod; channel tagged prod → auto-routed, no attach.
    tags::attach(&pool, m.id, prod).await.unwrap();
    routing::tag_channel(&pool, c.id, prod).await.unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["prod-pager"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn folder_tag_match_resolves(pool: PgPool) {
    let g = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "Edge".into(),
            sort_order: 0,
            parent_id: None,
        },
    )
    .await
    .unwrap();
    let mut nm = http_monitor("m1");
    nm.group_id = Some(g.id);
    let m = monitors::create(&pool, nm).await.unwrap();
    let c = notifications::create(&pool, channel("edge-chan"))
        .await
        .unwrap();
    let edge = mk_tag(&pool, "edge").await;
    // Tag the FOLDER (not the monitor) + the channel → monitor inherits.
    routing::tag_group(&pool, g.id, edge).await.unwrap();
    routing::tag_channel(&pool, c.id, edge).await.unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["edge-chan"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn folder_attached_channel_resolves(pool: PgPool) {
    let g = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "DB".into(),
            sort_order: 0,
            parent_id: None,
        },
    )
    .await
    .unwrap();
    let mut nm = http_monitor("m1");
    nm.group_id = Some(g.id);
    let m = monitors::create(&pool, nm).await.unwrap();
    let c = notifications::create(&pool, channel("folder-chan"))
        .await
        .unwrap();
    routing::attach_group_channel(&pool, g.id, c.id)
        .await
        .unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["folder-chan"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn exclusion_wins_over_every_inclusion(pool: PgPool) {
    let g = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "G".into(),
            sort_order: 0,
            parent_id: None,
        },
    )
    .await
    .unwrap();
    let mut nm = http_monitor("m1");
    nm.group_id = Some(g.id);
    let m = monitors::create(&pool, nm).await.unwrap();
    let c = notifications::create(&pool, channel("noisy"))
        .await
        .unwrap();
    let t = mk_tag(&pool, "t").await;

    // Include via ALL paths at once.
    notifications::attach(&pool, m.id, c.id).await.unwrap();
    tags::attach(&pool, m.id, t).await.unwrap();
    routing::tag_channel(&pool, c.id, t).await.unwrap();
    routing::attach_group_channel(&pool, g.id, c.id)
        .await
        .unwrap();
    // Then exclude it on this monitor.
    routing::exclude_channel(&pool, m.id, c.id).await.unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert!(
        got.is_empty(),
        "exclusion must override every inclusion path"
    );

    // Removing the exclusion restores it.
    routing::unexclude_channel(&pool, m.id, c.id).await.unwrap();
    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["noisy"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn dedupes_across_paths_and_skips_inactive(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("m1")).await.unwrap();
    let c = notifications::create(&pool, channel("dup")).await.unwrap();
    let t = mk_tag(&pool, "t").await;
    // Same channel reachable via explicit attach AND tag match.
    notifications::attach(&pool, m.id, c.id).await.unwrap();
    tags::attach(&pool, m.id, t).await.unwrap();
    routing::tag_channel(&pool, c.id, t).await.unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(
        got.len(),
        1,
        "channel reachable via 2 paths must appear once"
    );

    // Disable it → resolver drops it.
    notifications::update(
        &pool,
        c.id,
        notifications::UpdateNotification {
            name: None,
            config: None,
            active: Some(false),
            template_id: None,
            cooldown_seconds: None,
        },
    )
    .await
    .unwrap();
    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert!(got.is_empty(), "inactive channels are never routed");
}

#[sqlx::test(migrations = "../../migrations")]
async fn nested_folder_tag_propagates_to_child_monitor(pool: PgPool) {
    // Production (root)  →  Databases (nested)  →  m1 lives in Databases.
    // Tag on Production + channel sharing that tag must reach m1.
    let prod = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "Production".into(),
            sort_order: 0,
            parent_id: None,
        },
    )
    .await
    .unwrap();
    let db = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "Databases".into(),
            sort_order: 0,
            parent_id: Some(prod.id),
        },
    )
    .await
    .unwrap();
    let mut nm = http_monitor("m1");
    nm.group_id = Some(db.id);
    let m = monitors::create(&pool, nm).await.unwrap();
    let c = notifications::create(&pool, channel("prod-pager"))
        .await
        .unwrap();
    let prod_tag = mk_tag(&pool, "prod").await;
    routing::tag_group(&pool, prod.id, prod_tag).await.unwrap();
    routing::tag_channel(&pool, c.id, prod_tag).await.unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["prod-pager"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn nested_folder_attached_channel_propagates_to_child_monitor(pool: PgPool) {
    // Channel attached at the GRANDPARENT folder reaches a monitor two
    // levels down.
    let root = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "All".into(),
            sort_order: 0,
            parent_id: None,
        },
    )
    .await
    .unwrap();
    let mid = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "Region".into(),
            sort_order: 0,
            parent_id: Some(root.id),
        },
    )
    .await
    .unwrap();
    let leaf = monitor_groups::create(
        &pool,
        NewMonitorGroup {
            name: "Service".into(),
            sort_order: 0,
            parent_id: Some(mid.id),
        },
    )
    .await
    .unwrap();
    let mut nm = http_monitor("m1");
    nm.group_id = Some(leaf.id);
    let m = monitors::create(&pool, nm).await.unwrap();
    let c = notifications::create(&pool, channel("ops-pager"))
        .await
        .unwrap();
    routing::attach_group_channel(&pool, root.id, c.id)
        .await
        .unwrap();

    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert_eq!(names(&got), vec!["ops-pager"]);
}

#[sqlx::test(migrations = "../../migrations")]
async fn no_tags_no_attach_yields_nothing(pool: PgPool) {
    let m = monitors::create(&pool, http_monitor("m1")).await.unwrap();
    let _c = notifications::create(&pool, channel("orphan"))
        .await
        .unwrap();
    let got = routing::resolve_channels_for_monitor(&pool, m.id)
        .await
        .unwrap();
    assert!(got.is_empty());
}
