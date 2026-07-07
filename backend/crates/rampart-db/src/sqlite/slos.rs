//! SQLite `slos` domain — service level objectives + budget evaluation. Mirrors
//! the PG free-fn surface: list / list_all / get / get_unscoped / create /
//! update / delete / compute / trend / list_with_snapshots / evaluate_tick.
//!
//! The budget state machine (`snapshot`, `slo_transition`) is pure rampart_core,
//! reused verbatim. Monitor-SLI ratios run over the ported heartbeats; Metric-SLI
//! ratios run over the ported metric_samples. PG-isms translated:
//! `COUNT/SUM(...) FILTER (WHERE …)` → `SUM(CASE …)`; `date_bin(make_interval,
//! ts, origin)` → integer bucket `(ts - since)/step`; and jsonb `labels @>
//! matcher` (containment) → one bound `json_extract(labels, ?) = ?` per matcher
//! key (Prometheus labels are string→string).

use super::{raw_uuid, ts};
use crate::slos::{SloEvent, SloWithSnapshot};
use crate::{DbError, DbResult};
use rampart_core::ids::{EscalationPolicyId, MonitorId, NotificationId, OrgId, SloId};
use rampart_core::slo::{
    slo_transition, snapshot, NewSlo, SliKind, Slo, SloSnapshot, SloTransition, UpdateSlo,
};
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;

const FAST_BURN_WINDOW_SECONDS: i64 = 3600;
/// Short confirmation window for multiwindow fast-burn (see PG `slos.rs`).
const SHORT_BURN_WINDOW_SECONDS: i64 = 600;

fn channel_ids_from(s: &str) -> Vec<NotificationId> {
    serde_json::from_str::<Vec<String>>(s)
        .unwrap_or_default()
        .iter()
        .map(|u| NotificationId::from_uuid(raw_uuid(u)))
        .collect()
}

fn slo_from(r: &sqlx::sqlite::SqliteRow) -> Slo {
    Slo {
        id: SloId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        org_id: super::oid(&r.get::<String, _>("org_id")),
        name: r.get("name"),
        description: r.get("description"),
        sli_kind: SliKind::from_db(&r.get::<String, _>("sli_kind")),
        monitor_id: r
            .get::<Option<String>, _>("monitor_id")
            .map(|s| MonitorId::from_uuid(raw_uuid(&s))),
        good_metric: r.get("good_metric"),
        total_metric: r.get("total_metric"),
        labels: serde_json::from_str(&r.get::<String, _>("labels")).unwrap_or_default(),
        objective_pct: r.get::<f64, _>("objective_pct"),
        window_days: r.get::<i64, _>("window_days") as i32,
        enabled: r.get::<i64, _>("enabled") != 0,
        channel_ids: channel_ids_from(&r.get::<String, _>("channel_ids")),
        escalation_policy_id: r
            .get::<Option<String>, _>("escalation_policy_id")
            .map(|s| EscalationPolicyId::from_uuid(raw_uuid(&s))),
        breaching_at: r.get::<Option<i64>, _>("breaching_at").map(ts),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, org_id, name, description, sli_kind, monitor_id, good_metric, \
     total_metric, labels, objective_pct, window_days, enabled, channel_ids, \
     escalation_policy_id, breaching_at, created_at";

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<Slo>> {
    let sql = format!("SELECT {COLS} FROM slos WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(slo_from).collect())
}

pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<Slo>> {
    let sql = format!("SELECT {COLS} FROM slos ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(slo_from).collect())
}

pub async fn get(pool: &SqlitePool, id: SloId, org_id: OrgId) -> DbResult<Slo> {
    let sql = format!("SELECT {COLS} FROM slos WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(slo_from(&row))
}

pub async fn get_unscoped(pool: &SqlitePool, id: SloId) -> DbResult<Slo> {
    let sql = format!("SELECT {COLS} FROM slos WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(slo_from(&row))
}

pub async fn create(pool: &SqlitePool, input: NewSlo, org_id: OrgId) -> DbResult<Slo> {
    let id = SloId::new();
    let channel_ids: Vec<String> = input.channel_ids.iter().map(|c| c.0.to_string()).collect();
    sqlx::query(
        "INSERT INTO slos (id, name, description, sli_kind, monitor_id, good_metric, total_metric,
                           labels, objective_pct, window_days, enabled, channel_ids,
                           escalation_policy_id, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.description)
    .bind(input.sli_kind.as_str())
    .bind(input.monitor_id.map(|m| m.0.to_string()))
    .bind(input.good_metric)
    .bind(input.total_metric)
    .bind(serde_json::to_string(&input.labels).unwrap_or_else(|_| "{}".into()))
    .bind(input.objective_pct)
    .bind(input.window_days)
    .bind(input.enabled as i64)
    .bind(serde_json::to_string(&channel_ids).unwrap_or_else(|_| "[]".into()))
    .bind(input.escalation_policy_id.map(|p| p.0.to_string()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: SloId,
    patch: UpdateSlo,
    org_id: OrgId,
) -> DbResult<Slo> {
    let channel_ids: Option<String> = patch.channel_ids.map(|v| {
        let ids: Vec<String> = v.iter().map(|c| c.0.to_string()).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())
    });
    let res = sqlx::query(
        "UPDATE slos SET
            name          = COALESCE(?, name),
            description   = COALESCE(?, description),
            good_metric   = COALESCE(?, good_metric),
            total_metric  = COALESCE(?, total_metric),
            labels        = COALESCE(?, labels),
            objective_pct = COALESCE(?, objective_pct),
            window_days   = COALESCE(?, window_days),
            enabled       = COALESCE(?, enabled),
            channel_ids   = COALESCE(?, channel_ids),
            escalation_policy_id = COALESCE(?, escalation_policy_id),
            breaching_at  = NULL
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.description)
    .bind(patch.good_metric)
    .bind(patch.total_metric)
    .bind(
        patch
            .labels
            .map(|l| serde_json::to_string(&l).unwrap_or_else(|_| "{}".into())),
    )
    .bind(patch.objective_pct)
    .bind(patch.window_days)
    .bind(patch.enabled.map(|b| b as i64))
    .bind(channel_ids)
    .bind(patch.escalation_policy_id.map(|p| p.0.to_string()))
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

pub async fn delete(pool: &SqlitePool, id: SloId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM slos WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// `labels @> matcher` containment as a SQL fragment + ordered binds: one
/// `json_extract(labels, ?) = ?` per matcher key (path, value). Empty matcher →
/// no constraint. Prometheus labels are string→string; non-string matcher
/// values are compared by their JSON text form.
fn containment(matcher: &serde_json::Value) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut binds = Vec::new();
    if let Some(obj) = matcher.as_object() {
        for (k, v) in obj {
            sql.push_str(" AND json_extract(labels, ?) = ?");
            binds.push(format!("$.\"{}\"", k.replace('"', "\"\"")));
            binds.push(match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
    }
    (sql, binds)
}

/// Up/total heartbeat ratio (0–100) for a monitor since `since`, or None when
/// the window had no heartbeats. Maintenance excluded from both sides.
async fn monitor_ratio(pool: &SqlitePool, monitor: MonitorId, since: i64) -> DbResult<Option<f64>> {
    let (total, ok): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END), 0)
         FROM heartbeats WHERE monitor_id = ? AND ts >= ? AND status <> 'maintenance'",
    )
    .bind(monitor.0.to_string())
    .bind(since)
    .fetch_one(pool)
    .await?;
    if total == 0 {
        return Ok(None);
    }
    Ok(Some(ok as f64 / total as f64 * 100.0))
}

/// SUM(good)/SUM(total) ratio (0–100) over matching metric samples since
/// `since`, or None when no `total` was observed.
async fn metric_ratio(
    pool: &SqlitePool,
    good: &str,
    total: &str,
    labels: &serde_json::Value,
    since: i64,
    org_id: OrgId,
) -> DbResult<Option<f64>> {
    let (contain_sql, contain_binds) = containment(labels);
    let sql = format!(
        "SELECT COALESCE(SUM(CASE WHEN name = ? THEN value ELSE 0 END), 0) AS good,
                COALESCE(SUM(CASE WHEN name = ? THEN value ELSE 0 END), 0) AS total
         FROM metric_samples
         WHERE name IN (?, ?) AND ts >= {since} AND org_id = ?{contain_sql}"
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(good)
        .bind(total)
        .bind(good)
        .bind(total)
        .bind(org_id.0.to_string());
    for b in &contain_binds {
        q = q.bind(b);
    }
    let row = q.fetch_one(pool).await?;
    let total_v = row.get::<f64, _>("total");
    if total_v <= 0.0 {
        return Ok(None);
    }
    Ok(Some(
        (row.get::<f64, _>("good") / total_v * 100.0).clamp(0.0, 100.0),
    ))
}

async fn achieved(pool: &SqlitePool, slo: &Slo, window_seconds: i64) -> DbResult<Option<f64>> {
    let since = OffsetDateTime::now_utc().unix_timestamp() - window_seconds;
    match slo.sli_kind {
        SliKind::Monitor => match slo.monitor_id {
            Some(m) => monitor_ratio(pool, m, since).await,
            None => Ok(None),
        },
        SliKind::Metric => match (&slo.good_metric, &slo.total_metric) {
            (Some(g), Some(t)) => metric_ratio(pool, g, t, &slo.labels, since, slo.org_id).await,
            _ => Ok(None),
        },
    }
}

pub async fn compute(pool: &SqlitePool, slo: &Slo) -> DbResult<SloSnapshot> {
    let full = achieved(pool, slo, slo.window_days as i64 * 86400).await?;
    let long = achieved(pool, slo, FAST_BURN_WINDOW_SECONDS).await?;
    let confirm = achieved(pool, slo, SHORT_BURN_WINDOW_SECONDS).await?;
    Ok(snapshot(full, long, confirm, slo.objective_pct))
}

/// Achieved-ratio trend bucketed into ~`buckets` points (oldest→newest). PG's
/// `date_bin(make_interval(secs=>step), ts, since)` → integer bucket
/// `(ts - since)/step` (since/step are server-computed i64, inlined safely).
pub async fn trend(pool: &SqlitePool, slo: &Slo, buckets: i64) -> DbResult<Vec<f64>> {
    let buckets = buckets.clamp(2, 200);
    let window = slo.window_days as i64 * 86400;
    let since = OffsetDateTime::now_utc().unix_timestamp() - window;
    let step = (window / buckets).max(1);
    match slo.sli_kind {
        SliKind::Monitor => {
            let Some(m) = slo.monitor_id else {
                return Ok(vec![]);
            };
            let sql = format!(
                "SELECT CAST(SUM(CASE WHEN status = 'up' THEN 1 ELSE 0 END) AS REAL)
                          / NULLIF(COUNT(*), 0) * 100.0 AS pct
                 FROM heartbeats WHERE monitor_id = ? AND ts >= {since}
                 GROUP BY (ts - {since}) / {step} ORDER BY (ts - {since}) / {step}"
            );
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(m.0.to_string())
                .fetch_all(pool)
                .await?;
            Ok(rows
                .iter()
                .filter_map(|r| r.get::<Option<f64>, _>("pct"))
                .collect())
        }
        SliKind::Metric => {
            let (Some(g), Some(t)) = (&slo.good_metric, &slo.total_metric) else {
                return Ok(vec![]);
            };
            let (contain_sql, contain_binds) = containment(&slo.labels);
            let sql = format!(
                "SELECT SUM(CASE WHEN name = ? THEN value ELSE 0 END)
                          / NULLIF(SUM(CASE WHEN name = ? THEN value ELSE 0 END), 0) * 100.0 AS pct
                 FROM metric_samples
                 WHERE name IN (?, ?) AND ts >= {since} AND org_id = ?{contain_sql}
                 GROUP BY (ts - {since}) / {step} ORDER BY (ts - {since}) / {step}"
            );
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(g)
                .bind(t)
                .bind(g)
                .bind(t)
                .bind(slo.org_id.0.to_string());
            for b in &contain_binds {
                q = q.bind(b);
            }
            let rows = q.fetch_all(pool).await?;
            Ok(rows
                .iter()
                .filter_map(|r| r.get::<Option<f64>, _>("pct").map(|p| p.clamp(0.0, 100.0)))
                .collect())
        }
    }
}

pub async fn list_with_snapshots(
    pool: &SqlitePool,
    org_id: OrgId,
) -> DbResult<Vec<SloWithSnapshot>> {
    let mut out = Vec::new();
    for slo in list(pool, org_id).await? {
        let snapshot = compute(pool, &slo).await?;
        let trend = trend(pool, &slo, 24).await?;
        out.push(SloWithSnapshot {
            slo,
            snapshot,
            trend,
        });
    }
    Ok(out)
}

/// Evaluate every enabled SLO, persist the breach marker, return Fire/Resolve.
pub async fn evaluate_tick(pool: &SqlitePool) -> DbResult<Vec<SloEvent>> {
    let now_unix = OffsetDateTime::now_utc().unix_timestamp();
    let mut out = Vec::new();
    for slo in list_all(pool).await?.into_iter().filter(|s| s.enabled) {
        let snapshot = compute(pool, &slo).await?;
        let transition = slo_transition(slo.breaching_at, snapshot.breaching);
        match transition {
            SloTransition::None => {}
            SloTransition::Fire => {
                sqlx::query("UPDATE slos SET breaching_at = ? WHERE id = ?")
                    .bind(now_unix)
                    .bind(slo.id.0.to_string())
                    .execute(pool)
                    .await?;
                out.push(SloEvent {
                    slo,
                    transition,
                    snapshot,
                });
            }
            SloTransition::Resolve => {
                sqlx::query("UPDATE slos SET breaching_at = NULL WHERE id = ?")
                    .bind(slo.id.0.to_string())
                    .execute(pool)
                    .await?;
                out.push(SloEvent {
                    slo,
                    transition,
                    snapshot,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::{heartbeats, metric_samples, monitors};
    use rampart_core::monitor::{MonitorKind, MonitorStatus, NewMonitor};
    use rampart_core::Heartbeat;
    use std::collections::BTreeMap;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    async fn a_monitor(pool: &SqlitePool) -> MonitorId {
        let org = super::super::oid(DEF);
        monitors::create(
            pool,
            NewMonitor {
                name: "m".into(),
                kind: MonitorKind::Http,
                url: Some("https://x".into()),
                hostname: None,
                port: None,
                config: serde_json::json!({}),
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
                check_cert: false,
                cert_expiry_days: 14,
                slo_target_pct: None,
                slo_window_days: None,
                agent_id: None,
                escalation_policy_id: None,
            },
            org,
        )
        .await
        .unwrap()
        .id
    }

    fn monitor_slo(monitor: MonitorId, objective: f64) -> NewSlo {
        NewSlo {
            name: "api uptime".into(),
            description: String::new(),
            sli_kind: SliKind::Monitor,
            monitor_id: Some(monitor),
            good_metric: None,
            total_metric: None,
            labels: serde_json::json!({}),
            objective_pct: objective,
            window_days: 30,
            enabled: true,
            channel_ids: vec![],
            escalation_policy_id: None,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn monitor_slo_crud_and_compute(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let m = a_monitor(&pool).await;
        let s = create(&pool, monitor_slo(m, 99.0), org).await.unwrap();
        assert_eq!(s.sli_kind, SliKind::Monitor);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // 9 up / 1 down → 90% achieved over the window.
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let hbs: Vec<Heartbeat> = (0..10)
            .map(|i| Heartbeat {
                monitor_id: m,
                ts: OffsetDateTime::from_unix_timestamp(now - (i as i64) - 1).unwrap(),
                status: if i == 0 {
                    MonitorStatus::Down
                } else {
                    MonitorStatus::Up
                },
                latency_ms: Some(5),
                status_code: Some(200),
                msg: None,
                retries: 0,
                important: false,
            })
            .collect();
        heartbeats::insert_many(&pool, &hbs).await.unwrap();

        let snap = compute(&pool, &s).await.unwrap();
        // achieved 90% vs 99% objective → breaching.
        assert!(snap.breaching, "90% < 99% objective should breach");

        // evaluate fires, then recovering (all up) resolves.
        let fired = evaluate_tick(&pool).await.unwrap();
        assert_eq!(fired.len(), 1);
        assert!(matches!(fired[0].transition, SloTransition::Fire));
        assert!(get(&pool, s.id, org).await.unwrap().breaching_at.is_some());
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn metric_slo_containment(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        // Metric SLO: good/total ratio, matched by labels {job: web}.
        let s = create(
            &pool,
            NewSlo {
                name: "success ratio".into(),
                description: String::new(),
                sli_kind: SliKind::Metric,
                monitor_id: None,
                good_metric: Some("http_2xx".into()),
                total_metric: Some("http_total".into()),
                labels: serde_json::json!({ "job": "web" }),
                objective_pct: 99.0,
                window_days: 30,
                enabled: true,
                channel_ids: vec![],
                escalation_policy_id: None,
            },
            org,
        )
        .await
        .unwrap();

        let mk = |name: &str, v: f64, job: &str| {
            let mut labels = BTreeMap::new();
            labels.insert("job".to_string(), job.to_string());
            rampart_core::promtext::PromSample {
                name: name.into(),
                labels,
                value: v,
            }
        };
        // 90 good / 100 total for job=web → 90%. A decoy job=db row must be
        // excluded by the `@>` containment translation.
        metric_samples::insert_many(
            &pool,
            &[
                mk("http_2xx", 90.0, "web"),
                mk("http_total", 100.0, "web"),
                mk("http_total", 9999.0, "db"),
            ],
            org,
        )
        .await
        .unwrap();

        let ratio = metric_ratio(&pool, "http_2xx", "http_total", &s.labels, 0, org)
            .await
            .unwrap()
            .unwrap();
        assert!(
            (ratio - 90.0).abs() < 1e-9,
            "containment-scoped ratio {ratio}"
        );
        // compute → breaching (90% < 99%).
        assert!(compute(&pool, &s).await.unwrap().breaching);
    }
}
