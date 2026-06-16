//! Integration tests for RUM read aggregates (`rampart_db::rum`).
//! Covers the browser breakdown (coarse UA classification) and the
//! recent-traced feed (RUM → trace correlation).

use rampart_core::rum::RumBeacon;
use rampart_db::rum;
use sqlx::PgPool;

fn beacon(url: &str, ua: &str, trace_id: Option<&str>, lcp: f64) -> RumBeacon {
    serde_json::from_value(serde_json::json!({
        "app": "web",
        "url": url,
        "ua": ua,
        "trace_id": trace_id,
        "metrics": { "lcp": lcp, "load": lcp + 200.0 }
    }))
    .unwrap()
}

const CHROME: &str = "Mozilla/5.0 (X11; Linux) AppleWebKit/537 (KHTML) Chrome/120 Safari/537";
const FIREFOX: &str = "Mozilla/5.0 (X11; Linux; rv:121) Gecko/20100101 Firefox/121";
const EDGE: &str = "Mozilla/5.0 (Windows NT 10) AppleWebKit/537 Chrome/120 Safari/537 Edg/120";

#[sqlx::test(migrations = "../../migrations")]
async fn browser_breakdown_classifies_and_counts(pool: PgPool) {
    for b in [
        beacon("/", CHROME, None, 1800.0),
        beacon("/x", CHROME, None, 2200.0),
        beacon("/", FIREFOX, None, 1500.0),
        beacon("/", EDGE, None, 1200.0),
    ] {
        rum::insert_event(&pool, &b).await.unwrap();
    }

    let bd = rum::browser_breakdown(&pool, None, 24).await.unwrap();
    let get = |name: &str| bd.iter().find(|r| r.browser == name).map(|r| r.views);
    // Edge UA also contains "Chrome" — must classify as Edge, not Chrome.
    assert_eq!(get("Chrome"), Some(2));
    assert_eq!(get("Firefox"), Some(1));
    assert_eq!(get("Edge"), Some(1));
    // Busiest browser first.
    assert_eq!(bd[0].browser, "Chrome");
    assert!(bd[0].lcp_p75.is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn recent_traced_returns_only_traced(pool: PgPool) {
    rum::insert_event(&pool, &beacon("/checkout", CHROME, Some("trace-abc"), 3000.0))
        .await
        .unwrap();
    rum::insert_event(&pool, &beacon("/", CHROME, None, 1500.0))
        .await
        .unwrap();

    let traced = rum::recent_traced(&pool, None, 24, 50).await.unwrap();
    assert_eq!(traced.len(), 1, "only the beacon carrying a trace_id");
    assert_eq!(traced[0].trace_id, "trace-abc");
    assert_eq!(traced[0].url, "/checkout");
}

fn beacon_user(url: &str, user: &str) -> RumBeacon {
    serde_json::from_value(serde_json::json!({
        "app": "web",
        "url": url,
        "ua": CHROME,
        "user_id": user,
        "session": "sess-1",
        "metrics": { "lcp": 1800.0, "inp": 120.0, "load": 2000.0 }
    }))
    .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn page_samples_drilldown_carries_user(pool: PgPool) {
    rum::insert_event(&pool, &beacon_user("/checkout", "alice@example.com")).await.unwrap();
    rum::insert_event(&pool, &beacon_user("/checkout", "bob@example.com")).await.unwrap();
    rum::insert_event(&pool, &beacon("/other", CHROME, None, 1500.0)).await.unwrap();

    let samples = rum::page_samples(&pool, None, "/checkout", 24, 100).await.unwrap();
    assert_eq!(samples.len(), 2, "only /checkout loads");
    assert!(samples.iter().all(|s| s.user_id.is_some()));
    let users: Vec<_> = samples.iter().filter_map(|s| s.user_id.as_deref()).collect();
    assert!(users.contains(&"alice@example.com") && users.contains(&"bob@example.com"));
    assert!(samples[0].lcp_ms.is_some() && samples[0].inp_ms.is_some());

    // A different URL is isolated.
    assert_eq!(rum::page_samples(&pool, None, "/other", 24, 100).await.unwrap().len(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn user_breakdown_groups_by_user(pool: PgPool) {
    rum::insert_event(&pool, &beacon_user("/", "alice@example.com")).await.unwrap();
    rum::insert_event(&pool, &beacon_user("/checkout", "alice@example.com")).await.unwrap();
    rum::insert_event(&pool, &beacon_user("/", "bob@example.com")).await.unwrap();
    rum::insert_event(&pool, &beacon("/", CHROME, None, 1500.0)).await.unwrap(); // anon

    let bd = rum::user_breakdown(&pool, None, 24).await.unwrap();
    assert_eq!(bd.len(), 2, "only users with a user_id, anon excluded");
    assert_eq!(bd[0].user_id, "alice@example.com"); // busiest first
    assert_eq!(bd[0].views, 2);
    assert!(bd[0].lcp_p75.is_some());
}
