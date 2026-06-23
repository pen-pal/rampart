//! MySQL `telemetry_rules` domain — threshold alert rules over the telemetry
//! tiers. Mirrors the PG/SQLite free-fn surface: list / list_all / get /
//! get_unscoped / create / update / delete / evaluate_tick.
//!
//! `evaluate_tick` reuses the pure `rule_transition` state machine. `observe`
//! computes the per-tier aggregate: the **logs + traces** tiers are real (those
//! MySQL domains are ported), the **trace_latency** percentile is computed
//! app-side (`p_cont`, no MySQL percentile_cont). The error_tracking / profiles
//! / rum tiers aren't forked to MySQL yet → those kinds return `None` (no-data →
//! resolve, never a false fire) until they land.
//!
//! Dialect: uuid→CHAR(36), UUID[] channel_ids→LONGTEXT(JSON), ts→BIGINT,
//! `SUM(CASE…)` → `CAST(… AS SIGNED)`, `||` → `CONCAT`. `update` does NOT gate on
//! `rows_affected()` (MySQL counts changed not matched rows → a no-op patch would
//! falsely 404); the trailing `get()` surfaces NotFound.

use super::{raw_uuid, ts};
use crate::telemetry_rules::RuleEvent;
use crate::{DbError, DbResult};
use rampart_core::ids::{EscalationPolicyId, NotificationId, OrgId, TelemetryRuleId};
use rampart_core::metric_rule::{rule_transition, RuleOp, RuleTransition};
use rampart_core::telemetry_rule::{
    NewTelemetryRule, TelemetryRule, TelemetryRuleKind, UpdateTelemetryRule,
};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

/// Continuous percentile (matches PG `percentile_cont`).
fn p_cont(vals: &mut [f64], p: f64) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    if n == 1 {
        return Some(vals[0]);
    }
    let rank = p * (n as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    Some(vals[lo] + (vals[hi] - vals[lo]) * (rank - lo as f64))
}

fn channel_ids_from(s: &str) -> Vec<NotificationId> {
    serde_json::from_str::<Vec<String>>(s)
        .unwrap_or_default()
        .iter()
        .map(|u| NotificationId::from_uuid(raw_uuid(u)))
        .collect()
}

fn rule_from(r: &sqlx::mysql::MySqlRow) -> TelemetryRule {
    TelemetryRule {
        id: TelemetryRuleId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        org_id: super::oid(&r.get::<String, _>("org_id")),
        name: r.get("name"),
        kind: TelemetryRuleKind::from_db(&r.get::<String, _>("kind")),
        target: r.get("target"),
        match_text: r.get("match_text"),
        min_level: r.get::<i16, _>("min_level"),
        op: RuleOp::from_db(&r.get::<String, _>("op")),
        threshold: r.get::<f64, _>("threshold"),
        window_seconds: r.get::<i32, _>("window_seconds"),
        for_seconds: r.get::<i32, _>("for_seconds"),
        enabled: r.get::<i64, _>("enabled") != 0,
        channel_ids: channel_ids_from(&r.get::<String, _>("channel_ids")),
        escalation_policy_id: r
            .get::<Option<String>, _>("escalation_policy_id")
            .map(|s| EscalationPolicyId::from_uuid(raw_uuid(&s))),
        breach_since: r.get::<Option<i64>, _>("breach_since").map(ts),
        firing_at: r.get::<Option<i64>, _>("firing_at").map(ts),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, org_id, name, kind, target, match_text, min_level, op, threshold, \
     window_seconds, for_seconds, enabled, channel_ids, escalation_policy_id, breach_since, \
     firing_at, created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<TelemetryRule>> {
    let sql =
        format!("SELECT {COLS} FROM telemetry_alert_rules WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(rule_from).collect())
}

pub async fn list_all(pool: &MySqlPool) -> DbResult<Vec<TelemetryRule>> {
    let sql = format!("SELECT {COLS} FROM telemetry_alert_rules ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(rule_from).collect())
}

pub async fn get(pool: &MySqlPool, id: TelemetryRuleId, org_id: OrgId) -> DbResult<TelemetryRule> {
    let sql = format!("SELECT {COLS} FROM telemetry_alert_rules WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(rule_from(&row))
}

pub async fn get_unscoped(pool: &MySqlPool, id: TelemetryRuleId) -> DbResult<TelemetryRule> {
    let sql = format!("SELECT {COLS} FROM telemetry_alert_rules WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(rule_from(&row))
}

pub async fn create(
    pool: &MySqlPool,
    input: NewTelemetryRule,
    org_id: OrgId,
) -> DbResult<TelemetryRule> {
    let id = TelemetryRuleId::new();
    let channel_ids: Vec<String> = input.channel_ids.iter().map(|c| c.0.to_string()).collect();
    sqlx::query(
        "INSERT INTO telemetry_alert_rules
            (id, name, kind, target, match_text, min_level, op, threshold, window_seconds,
             for_seconds, enabled, channel_ids, escalation_policy_id, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.kind.as_str())
    .bind(input.target)
    .bind(input.match_text)
    .bind(input.min_level)
    .bind(input.op.as_str())
    .bind(input.threshold)
    .bind(input.window_seconds)
    .bind(input.for_seconds)
    .bind(input.enabled as i64)
    .bind(serde_json::to_string(&channel_ids).unwrap_or_else(|_| "[]".into()))
    .bind(input.escalation_policy_id.map(|p| p.0.to_string()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: TelemetryRuleId,
    patch: UpdateTelemetryRule,
    org_id: OrgId,
) -> DbResult<TelemetryRule> {
    let channel_ids: Option<String> = patch.channel_ids.map(|v| {
        let ids: Vec<String> = v.iter().map(|c| c.0.to_string()).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())
    });
    sqlx::query(
        "UPDATE telemetry_alert_rules SET
            name = COALESCE(?, name), kind = COALESCE(?, kind), target = COALESCE(?, target),
            match_text = COALESCE(?, match_text), min_level = COALESCE(?, min_level),
            op = COALESCE(?, op), threshold = COALESCE(?, threshold),
            window_seconds = COALESCE(?, window_seconds), for_seconds = COALESCE(?, for_seconds),
            enabled = COALESCE(?, enabled), channel_ids = COALESCE(?, channel_ids),
            escalation_policy_id = COALESCE(?, escalation_policy_id),
            breach_since = NULL, firing_at = NULL
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.kind.map(|k| k.as_str().to_string()))
    .bind(patch.target)
    .bind(patch.match_text)
    .bind(patch.min_level)
    .bind(patch.op.map(|o| o.as_str().to_string()))
    .bind(patch.threshold)
    .bind(patch.window_seconds)
    .bind(patch.for_seconds)
    .bind(patch.enabled.map(|b| b as i64))
    .bind(channel_ids)
    .bind(patch.escalation_policy_id.map(|p| p.0.to_string()))
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // NotFound surfaces here (no rows_affected gate — see module note).
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: TelemetryRuleId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM telemetry_alert_rules WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Compute the rule's observed value over its rolling window. Logs + traces
/// tiers are real; error_tracking / profiles / rum tiers aren't forked to MySQL
/// yet → `None` (no-data → resolve, never a false fire).
async fn observe(pool: &MySqlPool, rule: &TelemetryRule) -> DbResult<Option<f64>> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - rule.window_seconds as i64;
    let org = rule.org_id.0.to_string();
    match rule.kind {
        TelemetryRuleKind::TraceLatency => {
            let rows = sqlx::query(
                "SELECT duration_ms FROM spans
                 WHERE received_at >= ? AND (? = '' OR service_name = ?) AND org_id = ?",
            )
            .bind(cutoff)
            .bind(&rule.target)
            .bind(&rule.target)
            .bind(&org)
            .fetch_all(pool)
            .await?;
            let mut durs: Vec<f64> = rows
                .iter()
                .map(|r| r.get::<f64, _>("duration_ms"))
                .collect();
            Ok(p_cont(&mut durs, 0.95))
        }
        TelemetryRuleKind::TraceErrorRate => {
            let (total, errs): (i64, i64) = sqlx::query_as(
                "SELECT COUNT(*),
                        CAST(COALESCE(SUM(CASE WHEN status_code = 2 THEN 1 ELSE 0 END), 0) AS SIGNED)
                 FROM spans
                 WHERE received_at >= ? AND (? = '' OR service_name = ?) AND org_id = ?",
            )
            .bind(cutoff)
            .bind(&rule.target)
            .bind(&rule.target)
            .bind(&org)
            .fetch_one(pool)
            .await?;
            if total == 0 {
                Ok(None)
            } else {
                Ok(Some(100.0 * errs as f64 / total as f64))
            }
        }
        TelemetryRuleKind::LogVolume => {
            let (n,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM logs
                 WHERE received_at >= ? AND severity >= ?
                   AND (? = '' OR service_name = ?)
                   AND (? = '' OR body LIKE CONCAT('%', ?, '%'))
                   AND org_id = ?",
            )
            .bind(cutoff)
            .bind(rule.min_level)
            .bind(&rule.target)
            .bind(&rule.target)
            .bind(&rule.match_text)
            .bind(&rule.match_text)
            .bind(&org)
            .fetch_one(pool)
            .await?;
            Ok(Some(n as f64))
        }
        // Not-yet-ported tiers → no data (safe: resolves, never false-fires).
        TelemetryRuleKind::ErrorRate
        | TelemetryRuleKind::ProfileSamples
        | TelemetryRuleKind::RumLcpP75 => Ok(None),
    }
}

/// Evaluate every enabled rule, persist state, return Fire/Resolve transitions.
/// Identical state machine to PG.
pub async fn evaluate_tick(pool: &MySqlPool) -> DbResult<Vec<RuleEvent>> {
    let now = OffsetDateTime::now_utc();
    let now_unix = now.unix_timestamp();
    let mut out = Vec::new();
    for rule in list_all(pool).await?.into_iter().filter(|r| r.enabled) {
        let value = observe(pool, &rule).await?;
        let breached = value
            .map(|v| rule.op.breached(v, rule.threshold))
            .unwrap_or(false);
        let transition = rule_transition(
            rule.breach_since,
            rule.firing_at,
            rule.for_seconds,
            breached,
            now,
        );
        match transition {
            RuleTransition::None => {}
            RuleTransition::Pending => {
                sqlx::query("UPDATE telemetry_alert_rules SET breach_since = ? WHERE id = ?")
                    .bind(now_unix)
                    .bind(rule.id.0.to_string())
                    .execute(pool)
                    .await?;
            }
            RuleTransition::ClearPending => {
                sqlx::query("UPDATE telemetry_alert_rules SET breach_since = NULL WHERE id = ?")
                    .bind(rule.id.0.to_string())
                    .execute(pool)
                    .await?;
            }
            RuleTransition::Fire => {
                sqlx::query("UPDATE telemetry_alert_rules SET breach_since = COALESCE(breach_since, ?), firing_at = ? WHERE id = ?")
                    .bind(now_unix)
                    .bind(now_unix)
                    .bind(rule.id.0.to_string())
                    .execute(pool)
                    .await?;
                out.push(RuleEvent {
                    rule,
                    transition,
                    value,
                });
            }
            RuleTransition::Resolve => {
                sqlx::query("UPDATE telemetry_alert_rules SET breach_since = NULL, firing_at = NULL WHERE id = ?")
                    .bind(rule.id.0.to_string())
                    .execute(pool)
                    .await?;
                out.push(RuleEvent {
                    rule,
                    transition,
                    value,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::logs;
    use rampart_core::log::ParsedLog;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_rule(kind: TelemetryRuleKind, op: RuleOp, threshold: f64) -> NewTelemetryRule {
        NewTelemetryRule {
            name: "rule".into(),
            kind,
            target: String::new(),
            match_text: String::new(),
            min_level: 0,
            op,
            threshold,
            window_seconds: 3600,
            for_seconds: 0,
            enabled: true,
            channel_ids: vec![],
            escalation_policy_id: None,
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_and_log_volume_fire(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        // log_volume rule: fire when > 2 logs in window.
        let r = create(
            &pool,
            new_rule(TelemetryRuleKind::LogVolume, RuleOp::Gt, 2.0),
            org,
        )
        .await
        .unwrap();
        assert_eq!(r.kind, TelemetryRuleKind::LogVolume);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // 0 logs → observe 0 → not breached → no event.
        assert!(evaluate_tick(&pool).await.unwrap().is_empty());

        // 3 logs → observe 3 > 2 → fires.
        let mk = |b: &str| ParsedLog {
            time_ns: 0,
            severity: 9,
            severity_text: None,
            service_name: "api".into(),
            body: b.into(),
            trace_id: None,
            span_id: None,
            attributes: serde_json::json!({}),
        };
        logs::insert_logs(&pool, &[mk("a"), mk("b"), mk("c")], org)
            .await
            .unwrap();
        let fired = evaluate_tick(&pool).await.unwrap();
        assert_eq!(fired.len(), 1);
        assert!(matches!(fired[0].transition, RuleTransition::Fire));
        assert_eq!(fired[0].value, Some(3.0));
        assert!(get(&pool, r.id, org).await.unwrap().firing_at.is_some());

        // a not-yet-ported tier (error_rate) observes None → never fires.
        let er = create(
            &pool,
            new_rule(TelemetryRuleKind::ErrorRate, RuleOp::Gt, 0.0),
            org,
        )
        .await
        .unwrap();
        let evs = evaluate_tick(&pool).await.unwrap();
        assert!(
            !evs.iter().any(|e| e.rule.id == er.id),
            "error_rate tier no-data, no fire"
        );

        // no-op update over an existing row must NOT 404 (MySQL changed-vs-matched).
        let noop = update(
            &pool,
            r.id,
            serde_json::from_value(serde_json::json!({})).unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(noop.id, r.id);

        // update + cross-org + delete.
        let u = update(
            &pool,
            r.id,
            serde_json::from_value(serde_json::json!({ "threshold": 99.0 })).unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.threshold, 99.0);
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, r.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, r.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, r.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
