//! Integration test for the cross-page recent-incidents feed used by the
//! dashboard (`rampart_db::incidents::recent`).

use rampart_core::status_page::NewStatusPage;
use rampart_db::incidents::{self, NewIncident};
use rampart_db::status_pages;
use sqlx::PgPool;
use time::OffsetDateTime;

#[sqlx::test(migrations = "../../migrations")]
async fn recent_active_first_then_newest(pool: PgPool) {
    let page: NewStatusPage =
        serde_json::from_value(serde_json::json!({ "slug": "demo", "title": "Demo" })).unwrap();
    let sp = status_pages::create(&pool, page).await.unwrap();

    let mk = |title: &str, style: &str| -> NewIncident {
        serde_json::from_value(serde_json::json!({ "title": title, "content": "x", "style": style }))
            .unwrap()
    };
    let outage = incidents::create(&pool, sp.id, None, mk("DB outage", "danger")).await.unwrap();
    let degraded = incidents::create(&pool, sp.id, None, mk("Slow", "warning")).await.unwrap();
    // Resolve the degraded one → inactive; the active outage should sort first.
    incidents::resolve(&pool, degraded.id, OffsetDateTime::now_utc()).await.unwrap();

    let recent = incidents::recent(&pool, 10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, outage.id, "active incident sorts before resolved");
    assert!(recent[0].active && !recent[1].active);
    assert_eq!(recent[0].style, rampart_core::incident::IncidentStyle::Danger);
}
