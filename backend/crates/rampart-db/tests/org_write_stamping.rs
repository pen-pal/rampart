//! Multi-tenancy Phase 4a regression tests: every create/insert DB fn stamps
//! the `org_id` column EXPLICITLY from the passed `OrgId`, not from the column
//! DEFAULT (which is always the Default org). A missed/incorrect stamp would
//! silently misfile rows cross-tenant, so these prove the write path is correct
//! once a second org exists: each fn is called with a NON-default org and the
//! persisted row is asserted to carry exactly that org_id.

use rampart_core::ids::OrgId;
use rampart_core::log::ParsedLog;
use rampart_core::monitor::NewMonitor;
use rampart_core::status_page::NewStatusPage;
use rampart_core::ChannelKind;
use rampart_db::delivery_log::{self, NewDelivery};
use rampart_db::notifications::{self, NewNotification};
use rampart_db::{logs, monitors, status_pages};
use sqlx::PgPool;
use uuid::Uuid;

/// A non-Default org, inserted via raw SQL so the test does not depend on the
/// `orgs::create` path. Distinct from `DEFAULT_ORG_ID` (`Uuid::from_u128(1)`).
const OTHER_ORG_UUID: Uuid = Uuid::from_u128(0x0c0ffe);

async fn make_other_org(pool: &PgPool) -> OrgId {
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'other', 'Other')")
        .bind(OTHER_ORG_UUID)
        .execute(pool)
        .await
        .unwrap();
    OrgId::from_uuid(OTHER_ORG_UUID)
}

fn http_monitor(name: &str) -> NewMonitor {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "kind": "http",
        "url": format!("https://{name}.example.com"),
    }))
    .unwrap()
}

fn line(body: &str) -> ParsedLog {
    ParsedLog {
        time_ns: 0,
        severity: 9,
        severity_text: None,
        service_name: "svc".into(),
        body: body.into(),
        trace_id: None,
        span_id: None,
        attributes: serde_json::json!({}),
    }
}

fn webhook(name: &str) -> NewNotification {
    NewNotification {
        kind: ChannelKind::Webhook,
        name: name.into(),
        config: serde_json::json!({ "url": "https://hooks.example.com/x" }),
        active: true,
        template_id: None,
        cooldown_seconds: 0,
        digest_window_secs: 0,
        quiet_hours_start: None,
        quiet_hours_end: None,
        rate_limit_per_hour: 0,
    }
}

/// Read one row's org_id directly (bypassing the org-scoped read filter, which
/// would otherwise hide a wrongly-stamped row and mask the bug).
async fn org_of(pool: &PgPool, sql: &'static str, id: Uuid) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A single-row create stamps the passed (non-default) org, not the DEFAULT.
#[sqlx::test(migrations = "../../migrations")]
async fn monitor_create_stamps_passed_org(pool: PgPool) {
    let other = make_other_org(&pool).await;
    let m = monitors::create(&pool, http_monitor("stamp"), other)
        .await
        .unwrap();
    let stamped = org_of(&pool, "SELECT org_id FROM monitors WHERE id = $1", m.id.0).await;
    assert_eq!(stamped, OTHER_ORG_UUID, "monitor must carry the passed org");
}

#[sqlx::test(migrations = "../../migrations")]
async fn status_page_create_stamps_passed_org(pool: PgPool) {
    let other = make_other_org(&pool).await;
    let page: NewStatusPage =
        serde_json::from_value(serde_json::json!({ "slug": "sp", "title": "SP" })).unwrap();
    let sp = status_pages::create(&pool, page, other).await.unwrap();
    let stamped = org_of(
        &pool,
        "SELECT org_id FROM status_pages WHERE id = $1",
        sp.id.0,
    )
    .await;
    assert_eq!(stamped, OTHER_ORG_UUID);
}

/// The UNNEST bulk path (`ARRAY_FILL(...)`) stamps every inserted row.
#[sqlx::test(migrations = "../../migrations")]
async fn logs_insert_stamps_passed_org_on_every_row(pool: PgPool) {
    let other = make_other_org(&pool).await;
    let n = logs::insert_logs(&pool, &[line("one"), line("two"), line("three")], other)
        .await
        .unwrap();
    assert_eq!(n, 3);
    // All three rows are stamped with the passed org; none defaulted.
    let other_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logs WHERE org_id = $1")
        .bind(OTHER_ORG_UUID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        other_count, 3,
        "every bulk-inserted log row carries the org"
    );
    let default_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logs WHERE org_id <> $1")
        .bind(OTHER_ORG_UUID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(default_count, 0, "no row fell back to the column DEFAULT");
}

/// `delivery_log::record` derives org_id from the referenced notification via
/// COALESCE, so a delivery for an other-org channel lands in that org.
#[sqlx::test(migrations = "../../migrations")]
async fn delivery_log_inherits_notification_org(pool: PgPool) {
    let other = make_other_org(&pool).await;
    let chan = notifications::create(&pool, webhook("other-chan"), other)
        .await
        .unwrap();

    let entry = delivery_log::record(
        &pool,
        NewDelivery {
            notification_id: Some(chan.id),
            channel_kind: "webhook",
            event_kind: "monitor_down",
            monitor_id: None,
            ok: true,
            error: None,
        },
    )
    .await
    .unwrap();
    let stamped = sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM delivery_log WHERE id = $1")
        .bind(entry.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        stamped, OTHER_ORG_UUID,
        "delivery org must follow the channel's org"
    );
}

/// With no notification (system event) the COALESCE falls back to the Default
/// org so the row stays visible to the Default-org read filter.
#[sqlx::test(migrations = "../../migrations")]
async fn delivery_log_falls_back_to_default_when_unlinked(pool: PgPool) {
    // No other org needed; just record a notification-less delivery.
    let entry = delivery_log::record(
        &pool,
        NewDelivery {
            notification_id: None,
            channel_kind: "webhook",
            event_kind: "system",
            monitor_id: None,
            ok: true,
            error: None,
        },
    )
    .await
    .unwrap();
    let stamped = sqlx::query_scalar::<_, Uuid>("SELECT org_id FROM delivery_log WHERE id = $1")
        .bind(entry.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        stamped,
        rampart_core::org::DEFAULT_ORG_ID,
        "an unlinked delivery falls back to the Default org"
    );
}
