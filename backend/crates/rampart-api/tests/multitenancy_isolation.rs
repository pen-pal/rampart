//! Cross-org isolation over the real request path.
//!
//! Phase-3 multi-tenancy gives every tenant-root management surface an
//! `org_id` filter sourced from the request's `OrgContext`. This suite proves
//! the end-to-end chain (session → OrgContext → org-scoped query) actually
//! isolates: an admin in the Default org creates resources, those resources are
//! reparented into a second org via a direct DB write (the only way to put a row
//! in another org until Phase 4 adds org-aware creation), and the Default admin
//! must then no longer see or mutate them — while a public status page stays
//! reachable (org-scoping must not touch the public surface).

mod common;

use axum::http::{Method, StatusCode};
use common::{register_admin, request};
use serde_json::{json, Value};
use sqlx::PgPool;

const OTHER_ORG: &str = "00000000-0000-0000-0000-0000000c0ffe";

/// Count rows in a JSON array body from a GET.
async fn list_len(router: &axum::Router, path: &str, cookie: &str) -> usize {
    let (status, _, body) = request(router, Method::GET, path, None, Some(cookie)).await;
    assert_eq!(status, StatusCode::OK, "GET {path}");
    let v: Value = serde_json::from_slice(&body).unwrap();
    v.as_array().map(|a| a.len()).unwrap_or(0)
}

fn http_monitor(name: &str) -> Value {
    json!({
        "name": name, "kind": "http", "url": format!("https://{name}.example.com"),
        "interval_seconds": 60, "timeout_seconds": 10, "max_retries": 0,
        "retry_interval_sec": 60, "resend_interval_sec": 0, "upside_down": false,
        "http_method": "GET", "accepted_statuses": [200], "follow_redirect": true,
        "ignore_tls": false
    })
}

#[sqlx::test(migrations = "../../migrations")]
async fn monitors_isolated_across_orgs(pool: PgPool) {
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    // Create two monitors in the Default org via the API.
    let m1: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("keep")),
        Some(&admin),
    )
    .await;
    let m2: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("move")),
        Some(&admin),
    )
    .await;
    let move_id = m2["id"].as_str().unwrap();
    assert_eq!(list_len(&router, "/v1/monitors", &admin).await, 2);

    // Reparent m2 into another org (simulates a future multi-org install).
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE monitors SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(move_id)
        .execute(&pool)
        .await
        .unwrap();

    // The Default admin now sees only m1, and m2 is an IDOR-safe 404.
    assert_eq!(list_len(&router, "/v1/monitors", &admin).await, 1);
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{move_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org GET");
    let (s, _, _) = request(
        &router,
        Method::DELETE,
        &format!("/v1/monitors/{move_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org DELETE");
    let (s, _, _) = request(
        &router,
        Method::PATCH,
        &format!("/v1/monitors/{move_id}"),
        Some(json!({"name":"x"})),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org PATCH");

    // m1 (still Default) is untouched.
    let keep_id = m1["id"].as_str().unwrap();
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{keep_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "own-org GET still works");
}

#[sqlx::test(migrations = "../../migrations")]
async fn tags_channels_escalations_isolated(pool: PgPool) {
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let _: Value = common::json(
        &router,
        Method::POST,
        "/v1/tags",
        Some(json!({"name":"t","color":"#fff"})),
        Some(&admin),
    )
    .await;
    let ch: Value = common::json(&router, Method::POST, "/v1/notifications", Some(json!({"name":"c","kind":"webhook","config":{"url":"https://e.example.com/h"},"active":true})), Some(&admin)).await;
    let cid = ch["id"].as_str().unwrap();
    let _: Value = common::json(
        &router,
        Method::POST,
        "/v1/escalation-policies",
        Some(json!({"name":"p","steps":[{"wait_seconds":0,"channel_ids":[cid]}]})),
        Some(&admin),
    )
    .await;

    assert_eq!(list_len(&router, "/v1/tags", &admin).await, 1);
    assert_eq!(list_len(&router, "/v1/notifications", &admin).await, 1);
    assert_eq!(
        list_len(&router, "/v1/escalation-policies", &admin).await,
        1
    );

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE tags SET org_id = $1::uuid")
        .bind(OTHER_ORG)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE notifications SET org_id = $1::uuid")
        .bind(OTHER_ORG)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE escalation_policies SET org_id = $1::uuid")
        .bind(OTHER_ORG)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        list_len(&router, "/v1/tags", &admin).await,
        0,
        "tags isolated"
    );
    assert_eq!(
        list_len(&router, "/v1/notifications", &admin).await,
        0,
        "channels isolated"
    );
    assert_eq!(
        list_len(&router, "/v1/escalation-policies", &admin).await,
        0,
        "policies isolated"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn status_page_management_isolated_but_public_view_open(pool: PgPool) {
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let page: Value = common::json(
        &router,
        Method::POST,
        "/v1/status-pages",
        Some(json!({"slug":"acme","title":"Acme","monitor_ids":[]})),
        Some(&admin),
    )
    .await;
    let page_id = page["id"].as_str().unwrap();
    assert_eq!(list_len(&router, "/v1/status-pages", &admin).await, 1);

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE status_pages SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(page_id)
        .execute(&pool)
        .await
        .unwrap();

    // Management surface is org-scoped: gone from the Default admin's view.
    assert_eq!(
        list_len(&router, "/v1/status-pages", &admin).await,
        0,
        "mgmt list isolated"
    );
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/status-pages/{page_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org mgmt GET");
    // Cross-org section access is gated through the page.
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/status-pages/{page_id}/sections"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org sections");

    // The PUBLIC view resolves by slug with no session and must stay open
    // regardless of which org owns the page.
    let (s, _, _) = request(
        &router,
        Method::GET,
        "/v1/public/status-pages/acme",
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "public view stays open");
}

#[sqlx::test(migrations = "../../migrations")]
async fn incidents_isolated_via_owning_page(pool: PgPool) {
    // Incidents have no org_id of their own — they inherit the owning status
    // page's org. Every authenticated incident op must gate through that page.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let page: Value = common::json(
        &router,
        Method::POST,
        "/v1/status-pages",
        Some(json!({"slug":"ops","title":"Ops","monitor_ids":[]})),
        Some(&admin),
    )
    .await;
    let page_id = page["id"].as_str().unwrap();
    let inc: Value = common::json(
        &router,
        Method::POST,
        &format!("/v1/status-pages/{page_id}/incidents"),
        Some(json!({"title":"Outage","content":"is down"})),
        Some(&admin),
    )
    .await;
    let inc_id = inc["id"].as_str().unwrap();
    assert_eq!(
        list_len(
            &router,
            &format!("/v1/status-pages/{page_id}/incidents"),
            &admin
        )
        .await,
        1
    );

    // Reparent the owning page into another org.
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE status_pages SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(page_id)
        .execute(&pool)
        .await
        .unwrap();

    // Page-scoped list gates through the owning page → 404.
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/status-pages/{page_id}/incidents"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org incident list");
    // Top-level by-id ops resolve the incident → owning page → org → 404.
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/incidents/{inc_id}/updates"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org incident updates");
    let (s, _, _) = request(
        &router,
        Method::PATCH,
        &format!("/v1/incidents/{inc_id}"),
        Some(json!({"title":"x"})),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org incident PATCH");
    let (s, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/incidents/{inc_id}/resolve"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org incident resolve");
    let (s, _, _) = request(
        &router,
        Method::DELETE,
        &format!("/v1/incidents/{inc_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org incident DELETE");
}

#[sqlx::test(migrations = "../../migrations")]
async fn error_projects_isolated_across_orgs(pool: PgPool) {
    // error_projects is a tenant-root with its own org_id; its issues/events/
    // histograms/source-maps inherit it. List is org-scoped; every project- and
    // issue-keyed handler gates through the owning project's org.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let proj: Value = common::json(
        &router,
        Method::POST,
        "/v1/error-projects",
        Some(json!({"name":"checkout"})),
        Some(&admin),
    )
    .await;
    let pid = proj["id"].as_str().unwrap();
    assert_eq!(list_len(&router, "/v1/error-projects", &admin).await, 1);

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE error_projects SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();

    // List is org-scoped → project vanishes.
    assert_eq!(
        list_len(&router, "/v1/error-projects", &admin).await,
        0,
        "project list isolated"
    );
    // Project-keyed child + mutation handlers gate via project_in_org → 404.
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/error-projects/{pid}/issues"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org issue list");
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/error-projects/{pid}/histogram"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org histogram");
    let (s, _, _) = request(
        &router,
        Method::PATCH,
        &format!("/v1/error-projects/{pid}"),
        Some(json!({"name":"x"})),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org project PATCH");
    let (s, _, _) = request(
        &router,
        Method::DELETE,
        &format!("/v1/error-projects/{pid}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org project DELETE");
}

#[sqlx::test(migrations = "../../migrations")]
async fn bulk_edit_skips_cross_org_monitors(pool: PgPool) {
    // The id-list bulk endpoints resolve each monitor WITHIN the caller's org;
    // an id in another org is reported as skipped, never read or mutated.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let keep: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("keep")),
        Some(&admin),
    )
    .await;
    let mv: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("move")),
        Some(&admin),
    )
    .await;
    let keep_id = keep["id"].as_str().unwrap();
    let move_id = mv["id"].as_str().unwrap();

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE monitors SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(move_id)
        .execute(&pool)
        .await
        .unwrap();

    // Bulk-edit BOTH ids with enabled=false: only the in-org monitor is touched.
    let res: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors/bulk-edit",
        Some(json!({"ids": [keep_id, move_id], "patch": {"enabled": false}})),
        Some(&admin),
    )
    .await;
    assert_eq!(res["updated"], 1, "only the in-org monitor is updated");
    assert_eq!(res["skipped"], 1, "the cross-org monitor is skipped");

    // The cross-org monitor keeps its original (active) state — never paused.
    let still_active: bool = sqlx::query_scalar("SELECT active FROM monitors WHERE id = $1::uuid")
        .bind(move_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(still_active, "cross-org monitor must NOT have been mutated");
}

#[sqlx::test(migrations = "../../migrations")]
async fn monitor_junctions_isolated(pool: PgPool) {
    // Attaching a tag / channel to a monitor must verify BOTH ends are in the
    // caller's org. Junction handlers key only on ids, so they gate through
    // monitors::get + tags::get / notifications::get.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let mine: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("mine")),
        Some(&admin),
    )
    .await;
    let mine_id = mine["id"].as_str().unwrap();
    let tag: Value = common::json(
        &router,
        Method::POST,
        "/v1/tags",
        Some(json!({"name":"prod","color":"#0f0"})),
        Some(&admin),
    )
    .await;
    let tag_id = tag["id"].as_str().unwrap();
    let victim: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("victim")),
        Some(&admin),
    )
    .await;
    let victim_id = victim["id"].as_str().unwrap();

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE monitors SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(victim_id)
        .execute(&pool)
        .await
        .unwrap();

    // Cross-org monitor: can't list its tags/channels, can't attach to it.
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{victim_id}/tags"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "list cross-org monitor tags");
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/monitors/{victim_id}/notifications"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "list cross-org monitor channels");
    let (s, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/monitors/{victim_id}/tags/{tag_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "attach my tag to cross-org monitor"
    );

    // Reparent the TAG: can't attach a cross-org tag to my own monitor.
    sqlx::query("UPDATE tags SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(tag_id)
        .execute(&pool)
        .await
        .unwrap();
    let (s, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/monitors/{mine_id}/tags/{tag_id}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "attach cross-org tag to my monitor"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn tag_routing_isolated(pool: PgPool) {
    // Folder-level tag-routing (group↔tag, group↔channel, monitor excludes)
    // gates through the owning folder/monitor's org. A cross-org folder id is
    // a 404 for every read and write.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let grp: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitor-groups",
        Some(json!({"name":"infra","sort_order":0})),
        Some(&admin),
    )
    .await;
    let gid = grp["id"].as_str().unwrap();
    let tag: Value = common::json(
        &router,
        Method::POST,
        "/v1/tags",
        Some(json!({"name":"prod","color":"#0f0"})),
        Some(&admin),
    )
    .await;
    let tid = tag["id"].as_str().unwrap();

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE monitor_groups SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(gid)
        .execute(&pool)
        .await
        .unwrap();

    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/monitor-groups/{gid}/tags"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "list cross-org group tags");
    let (s, _, _) = request(
        &router,
        Method::GET,
        &format!("/v1/monitor-groups/{gid}/channels"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "list cross-org group channels");
    let (s, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/monitor-groups/{gid}/tags/{tid}"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "tag a cross-org group");
}

#[sqlx::test(migrations = "../../migrations")]
async fn escalation_episodes_isolated(pool: PgPool) {
    // Escalation episodes carry no org_id; they inherit the owning policy's
    // (NOT-NULL policy_id). The dashboard episode list is org-scoped and the
    // subject-agnostic ack-by-id gates through the episode's policy org.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let mon: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("svc")),
        Some(&admin),
    )
    .await;
    let mid = mon["id"].as_str().unwrap();
    let ch: Value = common::json(&router, Method::POST, "/v1/notifications", Some(json!({"name":"c","kind":"webhook","config":{"url":"https://e.example.com/h"},"active":true})), Some(&admin)).await;
    let cid = ch["id"].as_str().unwrap();
    let pol: Value = common::json(
        &router,
        Method::POST,
        "/v1/escalation-policies",
        Some(json!({"name":"esc","steps":[{"wait_seconds":0,"channel_ids":[cid]}]})),
        Some(&admin),
    )
    .await;
    let pid = pol["id"].as_str().unwrap();

    // Open an episode for that monitor under that policy (the scheduler does
    // this in production; here we insert directly).
    let eid = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO escalation_episodes (id, policy_id, subject_kind, subject_ref, monitor_id) VALUES ($1::uuid, $2::uuid, 'monitor', $3, $4::uuid)")
        .bind(&eid).bind(pid).bind(mid).bind(mid).execute(&pool).await.unwrap();
    assert_eq!(
        list_len(&router, "/v1/escalation-policies/episodes", &admin).await,
        1
    );

    // Reparent the owning policy into another org.
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE escalation_policies SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();

    // Episode drops off the dashboard list; ack-by-id 404s.
    assert_eq!(
        list_len(&router, "/v1/escalation-policies/episodes", &admin).await,
        0,
        "episode list org-scoped"
    );
    let (s, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/escalation-policies/episodes/{eid}/ack"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org episode ack");
}

#[sqlx::test(migrations = "../../migrations")]
async fn detection_findings_isolated(pool: PgPool) {
    // detection_findings carry no org_id; they inherit the owning rule's
    // (NOT-NULL rule_id). The triage feed is org-scoped and ack-by-id gates
    // through the finding's rule org.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    // Insert a rule (stamped Default org — the column DEFAULT was dropped in
    // Phase 6 migration 0112) + one open finding under it.
    let rid = uuid::Uuid::new_v4().to_string();
    let fid = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO detection_rules (id, name, org_id) VALUES ($1::uuid, 'bruteforce', '00000000-0000-0000-0000-000000000001')")
        .bind(&rid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO detection_findings (id, rule_id, rule_name, severity, match_count, window_from, window_to) VALUES ($1::uuid, $2::uuid, 'bruteforce', 'high', 3, now(), now())")
        .bind(&fid).bind(&rid).execute(&pool).await.unwrap();
    assert_eq!(
        list_len(&router, "/v1/detection-rules/findings?open=true", &admin).await,
        1
    );

    // Reparent the owning rule into another org.
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE detection_rules SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(&rid)
        .execute(&pool)
        .await
        .unwrap();

    // Feed drops the finding; ack-by-id 404s.
    assert_eq!(
        list_len(&router, "/v1/detection-rules/findings?open=true", &admin).await,
        0,
        "findings feed org-scoped"
    );
    let (s, _, _) = request(
        &router,
        Method::POST,
        &format!("/v1/detection-rules/findings/{fid}/ack"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "cross-org finding ack");
}

#[sqlx::test(migrations = "../../migrations")]
async fn monitor_heartbeat_reads_isolated(pool: PgPool) {
    // The monitor-keyed heartbeat reads (reliability / heartbeats /
    // heartbeats.csv) gate through monitors::get like their rollup siblings.
    let router = common::router(pool.clone());
    let admin = register_admin(&router).await;

    let mon: Value = common::json(
        &router,
        Method::POST,
        "/v1/monitors",
        Some(http_monitor("svc")),
        Some(&admin),
    )
    .await;
    let mid = mon["id"].as_str().unwrap();

    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1::uuid, 'other', 'Other') ON CONFLICT DO NOTHING")
        .bind(OTHER_ORG).execute(&pool).await.unwrap();
    sqlx::query("UPDATE monitors SET org_id = $1::uuid WHERE id = $2::uuid")
        .bind(OTHER_ORG)
        .bind(mid)
        .execute(&pool)
        .await
        .unwrap();

    for path in ["reliability", "heartbeats", "heartbeats.csv"] {
        let (s, _, _) = request(
            &router,
            Method::GET,
            &format!("/v1/monitors/{mid}/{path}"),
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "cross-org monitor {path}");
    }
}
