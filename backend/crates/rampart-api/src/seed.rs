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
    pub profiles: usize,
    pub synthetics: usize,
    pub cron_jobs: usize,
    pub on_call: usize,
    pub escalations: usize,
    pub maintenance: usize,
    pub silences: usize,
    pub metric_rules: usize,
    pub telemetry_rules: usize,
    pub incidents: usize,
    pub subscribers: usize,
    pub api_keys: usize,
    pub agents: usize,
    pub proxies: usize,
    pub tags: usize,
    pub scheduled_reports: usize,
    pub skipped: bool,
}

impl fmt::Display for SeedStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.skipped {
            return write!(f, "demo data already present — nothing seeded");
        }
        write!(
            f,
            "seeded: {} monitors, {} heartbeats, {} error events, {} spans, {} logs, {} RUM events, {} metric samples, {} detection findings, {} profiles, {} synthetics, {} cron jobs, {} on-call schedules, {} escalation policies, {} maintenance windows, {} silences, {} metric rules, {} telemetry rules, {} incidents, {} subscribers, {} api keys, {} agents, {} proxies, {} tags, {} scheduled reports",
            self.monitors,
            self.heartbeats,
            self.error_events,
            self.spans,
            self.logs,
            self.rum,
            self.metrics,
            self.detection_findings,
            self.profiles,
            self.synthetics,
            self.cron_jobs,
            self.on_call,
            self.escalations,
            self.maintenance,
            self.silences,
            self.metric_rules,
            self.telemetry_rules,
            self.incidents,
            self.subscribers,
            self.api_keys,
            self.agents,
            self.proxies,
            self.tags,
            self.scheduled_reports,
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

    // Idempotency sentinel: the demo folder. Re-running on an already-seeded
    // demo is a clean no-op.
    let groups = rampart_db::monitor_groups::list(pool).await?;
    if groups.iter().any(|g| g.name == DEMO_GROUP) {
        stats.skipped = true;
        return Ok(stats);
    }

    // Production safety: all demo data is `[demo]`-prefixed. If this DB already
    // holds monitors that AREN'T demo data, it's almost certainly a real
    // deployment — refuse to pollute it with sample traces/RUM/etc. The example
    // compose stacks run seed-demo against a fresh DB, so they're unaffected.
    // `RAMPART_SEED_FORCE=1` overrides for the rare intentional case.
    let force = std::env::var("RAMPART_SEED_FORCE").is_ok_and(|v| v == "1" || v == "true");
    if !force {
        let real = rampart_db::monitors::list(pool)
            .await?
            .into_iter()
            .filter(|m| !m.name.starts_with("[demo]"))
            .count();
        if real > 0 {
            anyhow::bail!(
                "refusing to seed: found {real} non-demo monitor(s) — this looks like a \
                 real instance, and seed-demo would add sample data ([demo] monitors, \
                 traces, RUM, logs, …). Set RAMPART_SEED_FORCE=1 to override."
            );
        }
    }

    let group = rampart_db::monitor_groups::create(
        pool,
        serde_json::from_value(json!({ "name": DEMO_GROUP, "sort_order": 0 }))?,
    )
    .await?;

    let gid = group.id.0;
    seed_monitors(pool, gid, &mut stats).await?;
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
    seed_profiles(pool, &mut stats).await?;

    // ── Advanced demo: exercise every remaining feature tier so a fresh
    // install shows the whole product, not just the core monitors. Each is
    // best-effort + idempotent-by-fresh-folder; a failure in one tier must not
    // abort the rest, so they swallow errors and only bump stats on success.
    seed_advanced_monitors(pool, gid, &mut stats).await; // synthetic + cron + more apps
    seed_on_call(pool, &mut stats).await;
    seed_escalation(pool, &mut stats).await;
    seed_maintenance(pool, &mut stats).await;
    seed_silences(pool, &mut stats).await;
    seed_metric_rules(pool, &mut stats).await;
    seed_more_telemetry_rules(pool, &mut stats).await;
    seed_more_detection(pool, &mut stats).await;
    seed_incidents(pool, &mut stats).await;
    seed_subscribers(pool, &mut stats).await;
    seed_api_keys(pool, &mut stats).await;
    seed_agents(pool, &mut stats).await;
    seed_proxies(pool, &mut stats).await;
    seed_tags(pool, &mut stats).await;
    seed_scheduled_reports(pool, &mut stats).await;

    // Depth for the deep-dive views: more traces, RUM sessions, profiles, and
    // metric series so the trace list / service map, RUM pages, profiling
    // service+type pickers (incl. diff), and metric explorer all have variety.
    seed_more_traces(pool, &mut stats).await;
    seed_more_rum(pool, &mut stats).await;
    seed_more_profiles(pool, &mut stats).await;
    seed_more_metrics(pool, &mut stats).await;

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
    // Background traffic: a time-spread set so the demo Logs view, level
    // counts, and histogram look populated rather than showing only 7 rows.
    // Deterministic (index-hashed) so repeated seeds stay stable.
    let services = ["[demo] api", "[demo] web", "[demo] payments", "[demo] worker"];
    let info_bodies = [
        "request completed status=200 path=/api/orders",
        "cache hit key=session:abc duration=2ms",
        "healthcheck ok",
        "page rendered route=/dashboard in 140ms",
        "job processed queue=emails id=8821",
    ];
    let warn_bodies = [
        "slow query 540ms on orders",
        "retrying upstream call attempt=2",
        "rate limit near threshold client=10.0.0.9",
    ];
    let error_bodies = [
        "charge failed: upstream timeout",
        "unhandled rejection in worker job=reindex",
        "connection reset by peer host=db-primary",
    ];
    for j in 0..150u64 {
        let svc = services[(j as usize) % services.len()];
        // index hash → mostly info, ~16% warn, ~6% error.
        let r = (j.wrapping_mul(2_654_435_761) >> 13) % 100;
        let (sev, sevt, body) = if r < 6 {
            (17, "error", error_bodies[(j as usize) % error_bodies.len()])
        } else if r < 22 {
            (13, "warn", warn_bodies[(j as usize) % warn_bodies.len()])
        } else {
            (9, "info", info_bodies[(j as usize) % info_bodies.len()])
        };
        // ~every 5 min back from now → ~12.5h of history.
        let offset_ns = (j as i64 + 10) * 300 * 1_000_000_000;
        logs.push(rampart_core::log::ParsedLog {
            time_ns: now_ns - offset_ns,
            severity: sev,
            severity_text: Some(sevt.to_string()),
            service_name: svc.to_string(),
            body: body.to_string(),
            trace_id: None,
            span_id: None,
            attributes: json!({ "env": "production" }),
        });
    }
    stats.logs += logs.len();
    rampart_db::logs::insert_logs(pool, &logs).await?;
    Ok(())
}

/// One representative CPU flamegraph for `[demo] api` so the Profiling view
/// shows a real merged tree out of the box (otherwise it reads "no profiles
/// yet" on a fresh install). Folded stacks are the uniform internal form, so a
/// few hand-written lines exercise the same read/merge path live ingest uses.
async fn seed_profiles(pool: &DbPool, stats: &mut SeedStats) -> Result<()> {
    use std::io::Write;
    // `frame;frame;… count` — a plausible request-handling CPU profile.
    let folded = "\
main;tokio::runtime::worker;handle_request;router::dispatch;checkout::handler;db::query;pg::execute 420
main;tokio::runtime::worker;handle_request;router::dispatch;checkout::handler;db::query;pg::parse 110
main;tokio::runtime::worker;handle_request;router::dispatch;checkout::handler;serde_json::to_writer 90
main;tokio::runtime::worker;handle_request;router::dispatch;cart::handler;db::query;pg::execute 260
main;tokio::runtime::worker;handle_request;router::dispatch;cart::handler;cache::get 70
main;tokio::runtime::worker;handle_request;router::dispatch;catalog::handler;search::rank 180
main;tokio::runtime::worker;handle_request;middleware::auth;jwt::verify 130
main;tokio::runtime::worker;handle_request;middleware::log;write_line 40
main;tokio::runtime::worker;background::flush_metrics 50
main;tokio::runtime::worker;background::reap_idle 20";
    let sample_count: i32 = folded
        .lines()
        .filter_map(|l| l.rsplit_once(' ').and_then(|(_, n)| n.trim().parse::<i32>().ok()))
        .sum();

    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(folded.as_bytes())?;
    let folded_gz = e.finish()?;

    // `received_at` (window filter) defaults to now() on insert, so the profile
    // lands in the live window automatically. period_ns is the sampling period.
    rampart_db::profiles::insert(
        pool,
        rampart_db::profiles::NewProfile {
            service_name: "[demo] api",
            profile_type: "cpu",
            period_ns: 10_000_000,           // 10ms sampling period
            duration_ns: 30 * 1_000_000_000, // 30s capture window
            sample_count,
            labels: json!({ "host": "demo-1", "env": "production" }),
            folded_gz: &folded_gz,
        },
    )
    .await?;
    stats.profiles += 1;
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

// ───────────────────────── advanced demo seeders ─────────────────────────
//
// Everything below is best-effort: it queries back the ids the core seeders
// created and swallows errors so one missing tier never aborts the rest. The
// goal is that a fresh `seed-demo` lights up *every* feature in the product.

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// 48 hourly up heartbeats for a freshly-created monitor so its status/uptime
/// strip renders (mirrors the loop in `seed_monitors`).
async fn push_hbs(pool: &DbPool, mon_uuid: uuid::Uuid, stats: &mut SeedStats) {
    let now = OffsetDateTime::now_utc();
    let mut hbs = Vec::new();
    for h in 0..48i64 {
        hbs.push(Heartbeat {
            monitor_id: MonitorId::from_uuid(mon_uuid),
            ts: now - Duration::hours(h),
            status: MonitorStatus::Up,
            latency_ms: Some(40 + (h as i32 * 11) % 140),
            status_code: Some(200),
            msg: None,
            retries: 0,
            important: false,
        });
    }
    stats.heartbeats += hbs.len();
    let _ = rampart_db::heartbeats::insert_many(pool, &hbs).await;
}

/// First notification channel id (the seeded Slack webhook), if any.
async fn demo_channel(pool: &DbPool) -> Option<rampart_core::ids::NotificationId> {
    rampart_db::notifications::list(pool).await.ok()?.into_iter().next().map(|n| n.id)
}

/// More app monitors across more probe kinds, a multi-step synthetic
/// transaction, and a cron-scheduled push monitor.
async fn seed_advanced_monitors(pool: &DbPool, gid: uuid::Uuid, stats: &mut SeedStats) {
    let extra = [
        json!({ "name": "[demo] Auth service", "kind": "http", "url": "https://auth.demo.rampart.local/healthz", "config": {}, "group_id": gid, "slo_target_pct": 99.95 }),
        json!({ "name": "[demo] Payments gRPC", "kind": "grpc", "hostname": "pay.demo.rampart.local", "port": 50051, "config": {}, "group_id": gid }),
        json!({ "name": "[demo] Search", "kind": "http", "url": "https://search.demo.rampart.local/_cluster/health", "config": {}, "group_id": gid }),
        json!({ "name": "[demo] Message queue", "kind": "mqtt", "hostname": "mq.demo.rampart.local", "port": 1883, "config": {}, "group_id": gid }),
        json!({ "name": "[demo] Public DNS", "kind": "dns", "hostname": "demo.rampart.local", "config": { "record_type": "A" }, "group_id": gid }),
        json!({ "name": "[demo] TLS certificate", "kind": "tls", "hostname": "demo.rampart.local", "port": 443, "config": {}, "group_id": gid }),
    ];
    for spec in extra {
        if let Ok(nm) = serde_json::from_value::<NewMonitor>(spec) {
            if let Ok(mon) = rampart_db::monitors::create(pool, nm).await {
                stats.monitors += 1;
                push_hbs(pool, mon.id.0, stats).await;
            }
        }
    }

    // Multi-step synthetic transaction (homepage → login → checkout) with
    // extraction + assertions — exercises the synthetic monitor kind.
    let syn = json!({
        "name": "[demo] Checkout journey", "kind": "synthetic",
        "url": "https://demo.rampart.local", "group_id": gid,
        "config": { "steps": [
            { "name": "Load homepage", "method": "GET", "url": "https://demo.rampart.local/",
              "assert": [ { "kind": "status", "op": "eq", "value": "200" } ] },
            { "name": "Log in", "method": "POST", "url": "https://demo.rampart.local/api/login",
              "body": "{\"user\":\"demo\"}",
              "extract": [ { "var": "token", "from": "json", "path": "$.token" } ],
              "assert": [ { "kind": "status", "op": "eq", "value": "200" } ] },
            { "name": "Checkout", "method": "POST", "url": "https://demo.rampart.local/api/checkout",
              "headers": { "Authorization": "Bearer {{token}}" },
              "assert": [ { "kind": "status", "op": "lt", "value": "400" },
                          { "kind": "body_contains", "op": "contains", "value": "orderId" } ] }
        ] }
    });
    if let Ok(nm) = serde_json::from_value::<NewMonitor>(syn) {
        if let Ok(mon) = rampart_db::monitors::create(pool, nm).await {
            stats.synthetics += 1;
            push_hbs(pool, mon.id.0, stats).await;
        }
    }

    // Cron-scheduled push monitor (a nightly batch job that checks in).
    let cron = json!({
        "name": "[demo] Nightly backup", "kind": "push", "group_id": gid,
        "config": { "cron": "0 3 * * *", "cron_grace_seconds": 600, "max_run_seconds": 1800 }
    });
    if let Ok(nm) = serde_json::from_value::<NewMonitor>(cron) {
        if rampart_db::monitors::create(pool, nm).await.is_ok() {
            stats.cron_jobs += 1;
        }
    }
}

/// A weekly on-call rotation over the admin user.
async fn seed_on_call(pool: &DbPool, stats: &mut SeedStats) {
    let Some(admin) = rampart_db::users::list(pool).await.ok().and_then(|u| u.into_iter().next())
    else { return };
    let spec = json!({
        "name": "[demo] Primary on-call",
        "rotation_seconds": 604_800,
        "anchor": rfc3339(OffsetDateTime::now_utc()),
        "participant_user_ids": [admin.id],
    });
    if let Ok(input) = serde_json::from_value::<rampart_core::on_call::NewOnCallSchedule>(spec) {
        if rampart_db::on_call::create(pool, input).await.is_ok() {
            stats.on_call += 1;
        }
    }
}

/// A two-step escalation ladder paging the demo channel immediately, then again
/// after 5 minutes.
async fn seed_escalation(pool: &DbPool, stats: &mut SeedStats) {
    let Some(ch) = demo_channel(pool).await else { return };
    let spec = json!({
        "name": "[demo] Sev-1 ladder",
        "steps": [
            { "wait_seconds": 0,   "channel_ids": [ch] },
            { "wait_seconds": 300, "channel_ids": [ch] }
        ]
    });
    if let Ok(input) = serde_json::from_value::<rampart_core::escalation::NewEscalationPolicy>(spec) {
        if rampart_db::escalations::create(pool, input).await.is_ok() {
            stats.escalations += 1;
        }
    }
}

/// An upcoming maintenance window covering the database monitor.
async fn seed_maintenance(pool: &DbPool, stats: &mut SeedStats) {
    let now = OffsetDateTime::now_utc();
    let spec = json!({
        "name": "[demo] Postgres 16 upgrade",
        "description": "Planned database major-version upgrade — brief failover expected",
        "start_at": rfc3339(now + Duration::days(1)),
        "end_at":   rfc3339(now + Duration::days(1) + Duration::hours(2)),
    });
    let Ok(input) = serde_json::from_value::<rampart_core::maintenance::NewMaintenanceWindow>(spec)
    else { return };
    let Ok(win) = rampart_db::maintenance::create(pool, input).await else { return };
    stats.maintenance += 1;
    // Attach the database + cache monitors so the window names something.
    if let Ok(mons) = rampart_db::monitors::list(pool).await {
        for m in mons.iter().filter(|m| m.name.contains("Database") || m.name.contains("Cache")) {
            let _ = rampart_db::maintenance::attach(pool, win.id, m.id).await;
        }
    }
}

/// A scoped, time-boxed silence on the cache monitor.
async fn seed_silences(pool: &DbPool, stats: &mut SeedStats) {
    let mon_uuid = rampart_db::monitors::list(pool).await.ok()
        .and_then(|m| m.into_iter().find(|m| m.name.contains("Cache")).map(|m| m.id.0));
    let now = OffsetDateTime::now_utc();
    let s = rampart_db::silences::NewSilence {
        monitor_id: mon_uuid,
        reason: "[demo] muted during the nightly backup window",
        created_by: None,
        expires_at: Some(now + Duration::hours(8)),
    };
    if rampart_db::silences::create(pool, s).await.is_ok() {
        stats.silences += 1;
    }
}

/// A metric alert rule on the seeded p95-latency series, evaluated once.
async fn seed_metric_rules(pool: &DbPool, stats: &mut SeedStats) {
    let spec = json!({
        "name": "[demo] p95 latency high",
        "metric": "demo_p95_latency_ms",
        "labels": { "service": "[demo] api" },
        "op": "gt", "threshold": 150.0, "for_seconds": 300, "enabled": true
    });
    if let Ok(input) = serde_json::from_value::<rampart_core::metric_rule::NewMetricRule>(spec) {
        if rampart_db::metric_rules::create(pool, input).await.is_ok() {
            stats.metric_rules += 1;
            let _ = rampart_db::metric_rules::evaluate_tick(pool).await;
        }
    }
}

/// One telemetry rule per remaining kind, so every alert-rule type is shown.
async fn seed_more_telemetry_rules(pool: &DbPool, stats: &mut SeedStats) {
    let specs = [
        json!({ "name": "[demo] Log volume spike", "kind": "log_volume", "target": "[demo] auth", "op": "gt", "threshold": 50.0, "window_seconds": 300 }),
        json!({ "name": "[demo] Trace latency p95", "kind": "trace_latency", "target": "[demo] api", "op": "gt", "threshold": 500.0, "window_seconds": 300 }),
        json!({ "name": "[demo] RUM LCP p75", "kind": "rum_lcp_p75", "target": "[demo] storefront", "op": "gt", "threshold": 2500.0, "window_seconds": 3600 }),
        json!({ "name": "[demo] Profile sample surge", "kind": "profile_samples", "target": "[demo] api", "op": "gt", "threshold": 1000.0, "window_seconds": 3600 }),
        json!({ "name": "[demo] Error event rate", "kind": "error_rate", "target": "[demo] web", "op": "gt", "threshold": 10.0, "window_seconds": 300 }),
    ];
    for spec in specs {
        if let Ok(input) = serde_json::from_value::<rampart_core::telemetry_rule::NewTelemetryRule>(spec) {
            if rampart_db::telemetry_rules::create(pool, input).await.is_ok() {
                stats.telemetry_rules += 1;
            }
        }
    }
}

/// More SIEM detection rules matching the seeded background logs, evaluated to
/// raise findings into the triage queue.
async fn seed_more_detection(pool: &DbPool, stats: &mut SeedStats) {
    let specs = [
        json!({ "name": "[demo] Connection resets", "description": "Backend connection churn to the primary DB", "severity": "medium", "service": "[demo] worker", "min_level": 17, "body_regex": "connection reset", "threshold": 1, "window_seconds": 3600 }),
        json!({ "name": "[demo] Unhandled rejections", "description": "Worker jobs throwing uncaught errors", "severity": "high", "service": "[demo] worker", "min_level": 17, "body_regex": "unhandled rejection", "threshold": 1, "window_seconds": 3600 }),
    ];
    let mut created = false;
    for spec in specs {
        if let Ok(input) = serde_json::from_value::<rampart_core::detection::NewDetectionRule>(spec) {
            if rampart_db::detection::create(pool, input).await.is_ok() {
                created = true;
            }
        }
    }
    if created {
        if let Ok(events) = rampart_db::detection::evaluate_tick(pool).await {
            stats.detection_findings += events.len();
        }
    }
}

/// A couple of status-page incidents so the dashboard "recent incidents" widget
/// and the public page have history.
async fn seed_incidents(pool: &DbPool, stats: &mut SeedStats) {
    let Some(page) = rampart_db::status_pages::list(pool).await.ok()
        .and_then(|p| p.into_iter().find(|p| p.slug == "demo"))
    else { return };
    let specs = [
        json!({ "title": "Elevated checkout latency", "content": "We are investigating slow responses on the checkout API. Payments are unaffected.", "style": "warning", "pinned": true }),
        json!({ "title": "Scheduled cache failover completed", "content": "The Redis cache failover finished successfully; all services nominal.", "style": "success", "pinned": false }),
    ];
    for spec in specs {
        if let Ok(input) = serde_json::from_value::<rampart_db::incidents::NewIncident>(spec) {
            if rampart_db::incidents::create(pool, page.id, None, input).await.is_ok() {
                stats.incidents += 1;
            }
        }
    }
}

/// A status-page email subscriber.
async fn seed_subscribers(pool: &DbPool, stats: &mut SeedStats) {
    let Some(page) = rampart_db::status_pages::list(pool).await.ok()
        .and_then(|p| p.into_iter().find(|p| p.slug == "demo"))
    else { return };
    if rampart_db::subscribers::subscribe_email(pool, page.id, "ops@demo.example").await.is_ok() {
        stats.subscribers += 1;
    }
}

/// A read-only API key for the demo admin.
async fn seed_api_keys(pool: &DbPool, stats: &mut SeedStats) {
    let Some(admin) = rampart_db::users::list(pool).await.ok().and_then(|u| u.into_iter().next())
    else { return };
    let spec = json!({ "name": "[demo] CI read-only", "scope": "read", "rate_limit_per_hour": 1000 });
    if let Ok(input) = serde_json::from_value::<rampart_core::api_key::NewApiKey>(spec) {
        if rampart_db::api_keys::create(pool, input, admin.id).await.is_ok() {
            stats.api_keys += 1;
        }
    }
}

/// A remote probe agent.
async fn seed_agents(pool: &DbPool, stats: &mut SeedStats) {
    let spec = json!({ "name": "[demo] eu-west probe", "location": "Frankfurt, DE" });
    if let Ok(input) = serde_json::from_value::<rampart_core::agent::NewAgent>(spec) {
        if rampart_db::agents::create(pool, input).await.is_ok() {
            stats.agents += 1;
        }
    }
}

/// An outbound proxy definition.
async fn seed_proxies(pool: &DbPool, stats: &mut SeedStats) {
    let spec = json!({ "protocol": "http", "host": "proxy.demo.rampart.local", "port": 3128, "active": true });
    if let Ok(input) = serde_json::from_value::<rampart_core::proxy::NewProxy>(spec) {
        if rampart_db::proxies::create(pool, input).await.is_ok() {
            stats.proxies += 1;
        }
    }
}

/// Tags + attachments so the tag filter has something to filter by.
async fn seed_tags(pool: &DbPool, stats: &mut SeedStats) {
    let specs = [
        json!({ "name": "production", "color": "#10b981" }),
        json!({ "name": "tier-1", "color": "#ef4444" }),
        json!({ "name": "team-payments", "color": "#6366f1" }),
    ];
    let mons = rampart_db::monitors::list(pool).await.unwrap_or_default();
    for spec in specs {
        if let Ok(input) = serde_json::from_value::<rampart_core::tag::NewTag>(spec) {
            if let Ok(tag) = rampart_db::tags::create(pool, input).await {
                stats.tags += 1;
                // Attach each tag to the first couple of demo monitors.
                for m in mons.iter().take(2) {
                    let _ = rampart_db::tags::attach(pool, m.id, tag.id).await;
                }
            }
        }
    }
}

/// A weekly scheduled uptime digest.
async fn seed_scheduled_reports(pool: &DbPool, stats: &mut SeedStats) {
    let spec = json!({ "name": "[demo] Weekly uptime digest", "recipients": ["ops@demo.example"], "cadence": "weekly" });
    if let Ok(input) = serde_json::from_value::<rampart_db::scheduled_reports::NewScheduledReport>(spec) {
        if rampart_db::scheduled_reports::create(pool, input).await.is_ok() {
            stats.scheduled_reports += 1;
        }
    }
}

/// ~10 more distributed traces across services with varied latency + a couple
/// of error traces, so the trace list, service map, and latency distribution
/// have real shape.
async fn seed_more_traces(pool: &DbPool, stats: &mut SeedStats) {
    let now_ns = OffsetDateTime::now_utc().unix_timestamp_nanos() as i64;
    // (service, operation, span-kind, is_error)
    let ops = [
        ("[demo] api", "GET /products", 2, false),
        ("[demo] api", "GET /search", 2, false),
        ("[demo] api", "POST /cart", 2, false),
        ("[demo] api", "GET /product/{id}", 2, false),
        ("[demo] payments", "POST /charge", 2, true),
        ("[demo] api", "GET /checkout", 2, false),
        ("[demo] api", "GET /orders", 2, false),
        ("[demo] search", "GET /_search", 2, false),
        ("[demo] payments", "POST /refund", 2, false),
        ("[demo] api", "POST /login", 2, true),
    ];
    let mut spans = Vec::new();
    for (i, (svc, op, kind, err)) in ops.into_iter().enumerate() {
        let base = now_ns - (i as i64 + 1) * 60_000_000_000; // 1 min apart
        let dur = 60_000_000 + ((i as i64 * 73) % 9) * 70_000_000; // 60–700ms
        let trace = hex32(&format!("demotrace{:04}", i + 10));
        let root = hex16(&format!("root{:08}", i));
        let child = hex16(&format!("chld{:08}", i));
        spans.push(ParsedSpan {
            trace_id: trace.clone(), span_id: root.clone(), parent_span_id: None,
            service_name: svc.to_string(), name: op.to_string(), kind,
            start_ns: base, end_ns: base + dur,
            status_code: if err { 2 } else { 0 },
            status_message: if err { Some("upstream error".into()) } else { None },
            attributes: json!({ "http.route": op, "http.method": op.split(' ').next().unwrap_or("GET") }),
        });
        // a downstream DB/dependency child span
        spans.push(ParsedSpan {
            trace_id: trace, span_id: child, parent_span_id: Some(root),
            service_name: svc.to_string(), name: "SELECT".into(), kind: 3,
            start_ns: base + 8_000_000, end_ns: base + dur - 8_000_000,
            status_code: 0, status_message: None,
            attributes: json!({ "db.system": "postgresql" }),
        });
    }
    stats.spans += spans.len();
    let _ = rampart_db::traces::insert_spans(pool, &spans).await;
}

/// More RUM beacons: several pages × a few sessions with device / connection
/// variety, so the RUM pages list, sessions, and Web-Vitals distributions fill
/// out.
async fn seed_more_rum(pool: &DbPool, stats: &mut SeedStats) {
    let pages = [
        ("/", 1700.0, 850.0, 0.03, 90.0),
        ("/product/42", 2400.0, 1100.0, 0.09, 160.0),
        ("/search?q=shoes", 2000.0, 980.0, 0.05, 110.0),
        ("/cart", 2600.0, 1200.0, 0.12, 220.0),
        ("/checkout", 3400.0, 1500.0, 0.21, 360.0),
        ("/account", 1900.0, 920.0, 0.04, 100.0),
    ];
    let agents = [
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17) Safari/604",
        "Mozilla/5.0 (Windows NT 10.0) Chrome/120 Safari/537",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 14) Chrome/120",
        "Mozilla/5.0 (Linux; Android 14) Chrome/120 Mobile",
    ];
    for (s, (url, lcp, fcp, cls, inp)) in pages.into_iter().enumerate() {
        for sess in 0..3usize {
            let ua = agents[(s + sess) % agents.len()];
            // jitter the vitals a little per session so distributions aren't flat
            let j = (sess as f64) * 120.0;
            let beacon: Result<RumBeacon, _> = serde_json::from_value(json!({
                "app": "[demo] storefront",
                "url": url,
                "session": format!("demo-sess-{s}-{sess}"),
                "ua": ua,
                "user_id": format!("demo-user-{}@example.com", (s + sess) % 5),
                "metrics": { "lcp": lcp + j, "fcp": fcp + j * 0.5, "cls": cls,
                             "inp": inp + j * 0.3, "ttfb": 200.0 + j * 0.2, "load": lcp + 300.0 + j }
            }));
            if let Ok(b) = beacon {
                if rampart_db::rum::insert_event(pool, &b).await.is_ok() {
                    stats.rum += 1;
                }
            }
        }
    }
}

/// More profiles: a second `[demo] api` cpu capture (so the diff view has two
/// captures to compare) and a `[demo] worker` cpu profile (so the service
/// picker has options).
async fn seed_more_profiles(pool: &DbPool, stats: &mut SeedStats) {
    use std::io::Write;
    let extra = [
        ("[demo] api", "main;tokio::runtime::worker;handle_request;router::dispatch;checkout::handler;db::query;pg::execute 510\nmain;tokio::runtime::worker;handle_request;router::dispatch;cart::handler;cache::get 95\nmain;tokio::runtime::worker;handle_request;middleware::auth;jwt::verify 160\nmain;tokio::runtime::worker;handle_request;router::dispatch;catalog::handler;search::rank 240"),
        ("[demo] worker", "main;tokio::runtime::worker;job::dispatch;reindex::run;es::bulk_index 380\nmain;tokio::runtime::worker;job::dispatch;email::send;smtp::write 120\nmain;tokio::runtime::worker;job::dispatch;thumbnail::resize;image::decode 210\nmain;tokio::runtime::worker;background::reap_idle 30"),
    ];
    for (svc, folded) in extra {
        let count: i32 = folded.lines()
            .filter_map(|l| l.rsplit_once(' ').and_then(|(_, n)| n.trim().parse::<i32>().ok()))
            .sum();
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        if e.write_all(folded.as_bytes()).is_err() { continue; }
        let Ok(gz) = e.finish() else { continue };
        let p = rampart_db::profiles::NewProfile {
            service_name: svc, profile_type: "cpu",
            period_ns: 10_000_000, duration_ns: 30 * 1_000_000_000,
            sample_count: count, labels: json!({ "host": "demo-2", "env": "production" }),
            folded_gz: &gz,
        };
        if rampart_db::profiles::insert(pool, p).await.is_ok() {
            stats.profiles += 1;
        }
    }
}

/// More metric series so the explorer has variety beyond the SLO counters.
async fn seed_more_metrics(pool: &DbPool, stats: &mut SeedStats) {
    let mut samples = Vec::new();
    for (name, base) in [("demo_queue_depth", 12.0), ("demo_error_rate", 0.8), ("demo_cpu_pct", 47.0)] {
        for inst in ["api-1", "api-2", "worker-1"] {
            let mut labels = BTreeMap::new();
            labels.insert("service".to_string(), "[demo] api".to_string());
            labels.insert("instance".to_string(), inst.to_string());
            samples.push(PromSample {
                name: name.to_string(),
                labels,
                value: base + (inst.len() as f64),
            });
        }
    }
    // Rampart's own runtime metrics (service=rampart) — seeded so the Metrics
    // view shows app self-metrics out of the box; live values are then pushed
    // by the self-metrics task (crate::self_metrics) once traffic flows.
    for (name, value) in [("rampart_http_requests_per_sec", 8.0), ("rampart_http_latency_ms_avg", 24.0)] {
        let mut labels = BTreeMap::new();
        labels.insert("service".to_string(), "rampart".to_string());
        samples.push(PromSample { name: name.to_string(), labels, value });
    }
    stats.metrics += samples.len();
    let _ = rampart_db::metric_samples::insert_many(pool, &samples).await;
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
