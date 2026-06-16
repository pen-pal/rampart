//! Demo data seeder — `rampart-api seed-demo`.
//!
//! Populates one representative slice of (nearly) every tier so a fresh
//! instance shows a living dashboard: monitors with uptime history, a folder,
//! a notification channel, an error project with grouped issues + breadcrumbs,
//! a multi-service trace, logs, RUM web-vitals, a metric series, a SIEM
//! detection rule (with a raised finding) and a telemetry alert rule.
//!
//! Idempotent: if the `[demo]` monitor group already exists it does nothing, so
//! re-running is safe. Everything is prefixed/tagged so it is easy to spot and
//! delete. Inserts go through the same `rampart_db` APIs the live ingest paths
//! use, so this doubles as an end-to-end smoke exercise.

use anyhow::Result;
use rampart_core::error_tracking::ParsedEvent;
use rampart_core::heartbeat::Heartbeat;
use rampart_core::ids::{ErrorProjectId, MonitorId};
use rampart_core::monitor::{MonitorStatus, NewMonitor};
use rampart_core::promtext::PromSample;
use rampart_core::rum::RumBeacon;
use rampart_core::trace::ParsedSpan;
use rampart_db::DbPool;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt;
use time::{Duration, OffsetDateTime};

#[derive(Debug, Default)]
pub struct SeedStats {
    pub monitors: usize,
    pub heartbeats: usize,
    pub error_events: usize,
    pub spans: usize,
    pub logs: usize,
    pub rum: usize,
    pub metrics: usize,
    pub detection_findings: usize,
    pub skipped: bool,
}

impl fmt::Display for SeedStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.skipped {
            return write!(f, "demo data already present — nothing seeded");
        }
        write!(
            f,
            "seeded: {} monitors, {} heartbeats, {} error events, {} spans, {} logs, {} RUM events, {} metric samples, {} detection findings",
            self.monitors,
            self.heartbeats,
            self.error_events,
            self.spans,
            self.logs,
            self.rum,
            self.metrics,
            self.detection_findings,
        )
    }
}

/// Seed the demo dataset. No-op (skipped=true) if already seeded.
pub async fn run(pool: &DbPool) -> Result<SeedStats> {
    let mut stats = SeedStats::default();

    // Optional admin bootstrap from env — lets a demo run sign you in with your
    // own credentials (like the app's first-run signup) instead of forcing the
    // built-in demo account. Server-side create, so it bypasses the API
    // password policy. Only fires when no user exists yet.
    seed_admin_from_env(pool).await?;

    // Idempotency sentinel: the demo folder.
    let groups = rampart_db::monitor_groups::list(pool).await?;
    if groups.iter().any(|g| g.name == DEMO_GROUP) {
        stats.skipped = true;
        return Ok(stats);
    }

    let group = rampart_db::monitor_groups::create(
        pool,
        serde_json::from_value(json!({ "name": DEMO_GROUP, "sort_order": 0 }))?,
    )
    .await?;

    seed_monitors(pool, group.id.0, &mut stats).await?;
    seed_status_page(pool).await?;
    seed_notification(pool).await?;
    seed_errors(pool, &mut stats).await?;
    seed_traces(pool, &mut stats).await?;
    seed_logs(pool, &mut stats).await?;
    seed_rum(pool, &mut stats).await?;
    seed_metrics(pool, &mut stats).await?;
    seed_alert_rule(pool).await?;
    seed_detection(pool, &mut stats).await?;
    seed_slo(pool).await?;

    Ok(stats)
}

const DEMO_GROUP: &str = "[demo] Sample services";

async fn seed_admin_from_env(pool: &DbPool) -> Result<()> {
    let email = std::env::var("RAMPART_ADMIN_EMAIL").ok().filter(|s| !s.is_empty());
    let password = std::env::var("RAMPART_ADMIN_PASSWORD").ok().filter(|s| !s.is_empty());
    let (Some(email), Some(password)) = (email, password) else {
        return Ok(());
    };
    if rampart_db::users::count(pool).await? > 0 {
        return Ok(()); // someone already exists — don't clobber
    }
    let hash = crate::auth::hash_password(&password)
        .map_err(|e| anyhow::anyhow!("hash failed: {e:?}"))?;
    rampart_db::users::create(
        pool,
        rampart_db::users::NewUser {
            email,
            name: Some("Admin".to_string()),
            password_hash: hash,
            role: rampart_core::Role::Admin,
        },
    )
    .await?;
    Ok(())
}

async fn seed_monitors(pool: &DbPool, group_id: uuid::Uuid, stats: &mut SeedStats) -> Result<()> {
    let specs = [
        json!({ "name": "[demo] API", "kind": "http", "url": "https://demo.rampart.local/health", "config": {}, "group_id": group_id, "slo_target_pct": 99.9 }),
        json!({ "name": "[demo] Website", "kind": "keyword", "url": "https://demo.rampart.local/", "config": { "keyword": "welcome" }, "group_id": group_id }),
        json!({ "name": "[demo] Database", "kind": "tcp", "hostname": "db.demo.rampart.local", "port": 5432, "config": {}, "group_id": group_id }),
        json!({ "name": "[demo] Cache", "kind": "redis", "hostname": "cache.demo.rampart.local", "port": 6379, "config": {}, "group_id": group_id }),
    ];
    let now = OffsetDateTime::now_utc();
    for (i, spec) in specs.iter().enumerate() {
        let nm: NewMonitor = serde_json::from_value(spec.clone())?;
        let mon = rampart_db::monitors::create(pool, nm).await?;
        stats.monitors += 1;

        // 48 hourly heartbeats so the uptime strip + current status render.
        // One monitor (the Cache) takes a short outage to make it interesting.
        let mut hbs = Vec::new();
        for h in 0..48i64 {
            let down = i == 3 && (3..6).contains(&h);
            hbs.push(Heartbeat {
                monitor_id: MonitorId::from_uuid(mon.id.0),
                ts: now - Duration::hours(h),
                status: if down { MonitorStatus::Down } else { MonitorStatus::Up },
                latency_ms: Some(if down { 0 } else { 40 + (h as i32 * 7) % 120 }),
                status_code: Some(if down { 503 } else { 200 }),
                msg: if down { Some("connection refused".into()) } else { None },
                retries: 0,
                important: h == 3 || h == 6, // the flip into/out of the outage
            });
        }
        stats.heartbeats += hbs.len();
        rampart_db::heartbeats::insert_many(pool, &hbs).await?;
    }
    Ok(())
}

/// A public status page plus a **fixed** inbound ingest token, so the example
/// stack's Alertmanager can post to `/v1/public/ingest/alertmanager/<token>`
/// with a value known ahead of time (see examples/full-stack).
const DEMO_INGEST_TOKEN: &str = "ing_demo_alertmanager_000000000000000000";

async fn seed_status_page(pool: &DbPool) -> Result<()> {
    let page = rampart_db::status_pages::create(
        pool,
        serde_json::from_value(json!({ "slug": "demo", "title": "[demo] Status" }))?,
    )
    .await?;
    // Best-effort: a duplicate token (re-seed on a wiped folder) is harmless.
    let _ = rampart_db::ingest_tokens::create_with_token(
        pool,
        page.id,
        "[demo] alertmanager",
        DEMO_INGEST_TOKEN,
    )
    .await;
    Ok(())
}

async fn seed_notification(pool: &DbPool) -> Result<()> {
    let spec = json!({
        "name": "[demo] Ops Slack",
        "kind": "webhook",
        "config": { "url": "https://hooks.slack.example/demo" },
        "enabled": true,
    });
    // Best-effort: channel config shape varies by kind; ignore if it doesn't take.
    if let Ok(input) = serde_json::from_value(spec) {
        let _ = rampart_db::notifications::create(pool, input).await;
    }
    Ok(())
}

async fn seed_errors(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    let project = rampart_db::error_tracking::create(
        pool,
        serde_json::from_value(json!({ "name": "[demo] web", "platform": "javascript" }))?,
    )
    .await?;
    let pid: ErrorProjectId = project.id;

    // Two distinct issues; the first recurs several times across releases/users
    // so issue stats (users-affected, by-release) have something to show.
    for (n, (typ, msg, release, user)) in [
        ("TypeError", "Cannot read properties of undefined (reading 'id')", "1.4.2", "u-101"),
        ("TypeError", "Cannot read properties of undefined (reading 'id')", "1.4.3", "u-102"),
        ("TypeError", "Cannot read properties of undefined (reading 'id')", "1.4.3", "u-103"),
        ("TimeoutError", "Request to /api/checkout timed out after 30s", "1.4.3", "u-104"),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = json!({
            "event_id": format!("{:032x}", 0xde_0000_0000_u64 + n as u64),
            "level": "error",
            "platform": "javascript",
            "release": release,
            "environment": "production",
            "exception": { "values": [ { "type": typ, "value": msg,
                "stacktrace": { "frames": [
                    { "filename": "app.js", "function": "render", "lineno": 42, "in_app": true },
                    { "filename": "api.js", "function": "fetchUser", "lineno": 88, "in_app": true }
                ] } } ] },
            "user": { "id": user, "email": format!("{user}@demo.example") },
            "breadcrumbs": { "values": [
                { "category": "navigation", "level": "info", "message": "/dashboard" },
                { "category": "http", "level": "info", "message": "GET /api/user 200" },
                { "category": "ui.click", "level": "info", "message": "button#refresh" }
            ] }
        });
        let ev = ParsedEvent::from_sentry_json(raw);
        rampart_db::error_tracking::record_event(pool, pid, &ev).await?;
        stats.error_events += 1;
    }
    Ok(())
}

async fn seed_traces(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let base = (now.unix_timestamp_nanos()) as i64 - 5_000_000_000;
    let trace = hex32("demotrace0001");
    let root = hex16("rootspan01");
    let child = hex16("childspan1");
    let leaf = hex16("leafspan01");
    let spans = vec![
        ParsedSpan {
            trace_id: trace.clone(), span_id: root.clone(), parent_span_id: None,
            service_name: "[demo] api".to_string(), name: "GET /checkout".into(), kind: 2,
            start_ns: base, end_ns: base + 420_000_000, status_code: 0,
            status_message: None, attributes: json!({ "http.method": "GET", "http.route": "/checkout" }),
        },
        ParsedSpan {
            trace_id: trace.clone(), span_id: child.clone(), parent_span_id: Some(root.clone()),
            service_name: "[demo] api".to_string(), name: "SELECT carts".into(), kind: 3,
            start_ns: base + 30_000_000, end_ns: base + 250_000_000, status_code: 0,
            status_message: None, attributes: json!({ "db.system": "postgresql" }),
        },
        ParsedSpan {
            trace_id: trace, span_id: leaf, parent_span_id: Some(child),
            service_name: "[demo] payments".to_string(), name: "POST /charge".into(), kind: 3,
            start_ns: base + 260_000_000, end_ns: base + 410_000_000, status_code: 2,
            status_message: Some("upstream timeout".into()),
            attributes: json!({ "http.method": "POST", "error": true }),
        },
    ];
    stats.spans += spans.len();
    rampart_db::traces::insert_spans(pool, &spans).await?;
    Ok(())
}

async fn seed_logs(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    let now_ns = OffsetDateTime::now_utc().unix_timestamp_nanos() as i64;
    let mut logs = Vec::new();
    let lines = [
        (9, "info", "[demo] api", "request completed status=200 path=/checkout"),
        (13, "warn", "[demo] api", "slow query 820ms on carts"),
        (17, "error", "[demo] payments", "charge failed: upstream timeout"),
        (9, "info", "[demo] web", "user signed in"),
        // a few that the demo detection rule matches
        (17, "error", "[demo] auth", "failed login for user bob from 10.0.0.5"),
        (17, "error", "[demo] auth", "failed login for user alice from 10.0.0.6"),
        (17, "error", "[demo] auth", "failed login for user bob from 10.0.0.5"),
    ];
    // The first three lines are the checkout request path — tag them with the
    // seeded trace id so the log↔trace correlation is populated in the demo.
    let demo_trace = hex32("demotrace0001");
    for (i, (sev, sevt, svc, body)) in lines.into_iter().enumerate() {
        logs.push(rampart_core::log::ParsedLog {
            time_ns: now_ns - (i as i64 * 1_000_000_000),
            severity: sev,
            severity_text: Some(sevt.to_string()),
            service_name: svc.to_string(),
            body: body.to_string(),
            trace_id: (i < 3).then(|| demo_trace.clone()),
            span_id: None,
            attributes: json!({ "env": "production" }),
        });
    }
    stats.logs += logs.len();
    rampart_db::logs::insert_logs(pool, &logs).await?;
    Ok(())
}

async fn seed_rum(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    for (url, lcp, fcp, cls, inp) in [
        ("/", 1800.0, 900.0, 0.04, 120.0),
        ("/checkout", 3200.0, 1400.0, 0.18, 340.0),
        ("/dashboard", 2100.0, 1000.0, 0.06, 90.0),
    ] {
        // The /checkout page-load carries the seeded trace id so the RUM→trace
        // pivot resolves to the demo checkout trace.
        let trace_id = (url == "/checkout").then(|| hex32("demotrace0001"));
        let beacon: RumBeacon = serde_json::from_value(json!({
            "app": "[demo] storefront",
            "url": url,
            "session": "demo-session",
            "ua": "Mozilla/5.0 (demo) Chrome/120 Safari/537",
            "trace_id": trace_id,
            "user_id": "demo-user@example.com",
            "metrics": { "lcp": lcp, "fcp": fcp, "cls": cls, "inp": inp, "ttfb": 210.0, "load": lcp + 300.0 }
        }))?;
        rampart_db::rum::insert_event(pool, &beacon).await?;
        stats.rum += 1;
    }
    Ok(())
}

async fn seed_metrics(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    let mut samples = Vec::new();
    for inst in ["api-1", "api-2"] {
        let mut labels = BTreeMap::new();
        labels.insert("service".to_string(), "[demo] api".to_string());
        labels.insert("instance".to_string(), inst.to_string());
        samples.push(PromSample {
            name: "demo_requests_per_sec".to_string(),
            labels: labels.clone(),
            value: 120.0 + inst.len() as f64,
        });
        samples.push(PromSample {
            name: "demo_p95_latency_ms".to_string(),
            labels,
            value: 180.0,
        });
    }
    // Ratio counters for the demo SLO: 9 994 good out of 10 000 → 99.94%,
    // comfortably above a 99.9% objective but with the budget visibly burning.
    let mut svc = BTreeMap::new();
    svc.insert("service".to_string(), "[demo] api".to_string());
    samples.push(PromSample {
        name: "demo_req_success".to_string(),
        labels: svc.clone(),
        value: 9_994.0,
    });
    samples.push(PromSample {
        name: "demo_req_total".to_string(),
        labels: svc,
        value: 10_000.0,
    });
    stats.metrics += samples.len();
    rampart_db::metric_samples::insert_many(pool, &samples).await?;
    Ok(())
}

async fn seed_slo(pool: &DbPool) -> Result<()> {
    let spec = json!({
        "name": "[demo] API request success",
        "description": "99.9% of API requests succeed over 30 days",
        "sli_kind": "metric",
        "good_metric": "demo_req_success",
        "total_metric": "demo_req_total",
        "labels": { "service": "[demo] api" },
        "objective_pct": 99.9,
        "window_days": 30,
    });
    if let Ok(input) = serde_json::from_value(spec) {
        let _ = rampart_db::slos::create(pool, input).await;
    }
    Ok(())
}

async fn seed_alert_rule(pool: &DbPool) -> Result<()> {
    let spec = json!({
        "name": "[demo] API error rate",
        "kind": "trace_error_rate",
        "target": "[demo] api",
        "op": "gt",
        "threshold": 5.0,
        "window_seconds": 300,
    });
    if let Ok(input) = serde_json::from_value(spec) {
        let _ = rampart_db::telemetry_rules::create(pool, input).await;
    }
    Ok(())
}

async fn seed_detection(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    let spec = json!({
        "name": "[demo] Repeated failed logins",
        "description": "Brute-force / credential-stuffing signal",
        "severity": "high",
        "service": "[demo] auth",
        "min_level": 17,
        "body_regex": "failed login",
        "threshold": 2,
        "window_seconds": 3600,
    });
    if let Ok(input) = serde_json::from_value(spec) {
        if rampart_db::detection::create(pool, input).await.is_ok() {
            // Evaluate once so the seeded auth logs raise a finding the triage
            // queue can show.
            if let Ok(events) = rampart_db::detection::evaluate_tick(pool).await {
                stats.detection_findings += events.len();
            }
        }
    }
    Ok(())
}

fn hex32(seed: &str) -> String {
    let mut s = String::new();
    for b in seed.bytes().cycle().take(16) {
        s.push_str(&format!("{b:02x}"));
    }
    s.truncate(32);
    s
}

fn hex16(seed: &str) -> String {
    let mut s = hex32(seed);
    s.truncate(16);
    s
}
