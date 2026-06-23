//! MySQL `metric_rules` domain — threshold/anomaly alert rules over ingested
//! metrics. Mirrors the PG/SQLite free-fn surface: list / list_all / get /
//! get_unscoped / create / update / delete / evaluate_tick.
//!
//! `evaluate_tick` reuses the pure `rampart_core::metric_rule::rule_transition`
//! state machine + the ported `mysql::metric_samples::latest`/`baseline` reads.
//! Dialect: uuid→CHAR(36), jsonb labels + `UUID[]` channel_ids→LONGTEXT(JSON),
//! double→DOUBLE, ts→BIGINT, op/enabled as VARCHAR/TINYINT.
//!
//! **MySQL changed-vs-matched gotcha:** `update` does NOT gate on
//! `rows_affected()` (MySQL counts CHANGED rows, so a no-op patch over an
//! existing row reports 0 and would falsely 404). The final `get()` surfaces
//! NotFound when the row is genuinely absent — identical result on both engines.

use super::{raw_uuid, ts};
use crate::metric_rules::{RuleEvent, SAMPLE_FRESHNESS_SECONDS};
use crate::{DbError, DbResult};
use rampart_core::ids::{EscalationPolicyId, MetricRuleId, NotificationId, OrgId};
use rampart_core::metric_rule::{
    rule_transition, MetricRule, NewMetricRule, RuleOp, RuleTransition, UpdateMetricRule,
};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

/// Trailing baseline window for the anomaly op (6h), mirroring PG.
const ANOMALY_BASELINE_SECONDS: i64 = 6 * 3600;

fn channel_ids_from(s: &str) -> Vec<NotificationId> {
    serde_json::from_str::<Vec<String>>(s)
        .unwrap_or_default()
        .iter()
        .map(|u| NotificationId::from_uuid(raw_uuid(u)))
        .collect()
}

fn rule_from(r: &sqlx::mysql::MySqlRow) -> MetricRule {
    MetricRule {
        id: MetricRuleId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        org_id: super::oid(&r.get::<String, _>("org_id")),
        name: r.get("name"),
        metric: r.get("metric"),
        labels: serde_json::from_str(&r.get::<String, _>("labels")).unwrap_or_default(),
        op: RuleOp::from_db(&r.get::<String, _>("op")),
        threshold: r.get::<f64, _>("threshold"),
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

const COLS: &str = "id, org_id, name, metric, labels, op, threshold, for_seconds, enabled, \
     channel_ids, escalation_policy_id, breach_since, firing_at, created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<MetricRule>> {
    let sql = format!("SELECT {COLS} FROM metric_rules WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(rule_from).collect())
}

pub async fn list_all(pool: &MySqlPool) -> DbResult<Vec<MetricRule>> {
    let sql = format!("SELECT {COLS} FROM metric_rules ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(rule_from).collect())
}

pub async fn get(pool: &MySqlPool, id: MetricRuleId, org_id: OrgId) -> DbResult<MetricRule> {
    let sql = format!("SELECT {COLS} FROM metric_rules WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(rule_from(&row))
}

pub async fn get_unscoped(pool: &MySqlPool, id: MetricRuleId) -> DbResult<MetricRule> {
    let sql = format!("SELECT {COLS} FROM metric_rules WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(rule_from(&row))
}

pub async fn create(pool: &MySqlPool, input: NewMetricRule, org_id: OrgId) -> DbResult<MetricRule> {
    let id = MetricRuleId::new();
    let channel_ids: Vec<String> = input.channel_ids.iter().map(|c| c.0.to_string()).collect();
    sqlx::query(
        "INSERT INTO metric_rules
            (id, name, metric, labels, op, threshold, for_seconds, enabled, channel_ids,
             escalation_policy_id, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.metric)
    .bind(serde_json::to_string(&input.labels).unwrap_or_else(|_| "{}".into()))
    .bind(input.op.as_str())
    .bind(input.threshold)
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
    id: MetricRuleId,
    patch: UpdateMetricRule,
    org_id: OrgId,
) -> DbResult<MetricRule> {
    let channel_ids: Option<String> = patch.channel_ids.map(|v| {
        let ids: Vec<String> = v.iter().map(|c| c.0.to_string()).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())
    });
    // Any edit resets in-flight breach state (the markers describe the OLD rule).
    sqlx::query(
        "UPDATE metric_rules SET
            name        = COALESCE(?, name),
            metric      = COALESCE(?, metric),
            labels      = COALESCE(?, labels),
            op          = COALESCE(?, op),
            threshold   = COALESCE(?, threshold),
            for_seconds = COALESCE(?, for_seconds),
            enabled     = COALESCE(?, enabled),
            channel_ids = COALESCE(?, channel_ids),
            escalation_policy_id = COALESCE(?, escalation_policy_id),
            breach_since = NULL,
            firing_at    = NULL
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.metric)
    .bind(
        patch
            .labels
            .map(|l| serde_json::to_string(&l).unwrap_or_else(|_| "{}".into())),
    )
    .bind(patch.op.map(|o| o.as_str().to_string()))
    .bind(patch.threshold)
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

pub async fn delete(pool: &MySqlPool, id: MetricRuleId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM metric_rules WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Evaluate every enabled rule against its series' latest fresh sample, persist
/// state, and return Fire/Resolve transitions. Identical state machine to PG;
/// the sample reads delegate to the ported `mysql::metric_samples`.
pub async fn evaluate_tick(pool: &MySqlPool) -> DbResult<Vec<RuleEvent>> {
    let now = OffsetDateTime::now_utc();
    let now_unix = now.unix_timestamp();
    let mut out = Vec::new();

    for rule in list_all(pool).await?.into_iter().filter(|r| r.enabled) {
        let latest =
            super::metric_samples::latest(pool, &rule.metric, &rule.labels, rule.org_id).await?;
        let fresh_value = latest
            .and_then(|(v, t)| ((now - t).whole_seconds() < SAMPLE_FRESHNESS_SECONDS).then_some(v));
        let breached = match fresh_value {
            Some(v) if rule.op.is_anomaly() => {
                match super::metric_samples::baseline(
                    pool,
                    &rule.metric,
                    &rule.labels,
                    ANOMALY_BASELINE_SECONDS,
                    rule.org_id,
                )
                .await?
                {
                    Some((mean, stddev)) => {
                        RuleOp::anomaly_breached(v, mean, stddev, rule.threshold)
                    }
                    None => false,
                }
            }
            Some(v) => rule.op.breached(v, rule.threshold),
            None => false,
        };

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
                sqlx::query("UPDATE metric_rules SET breach_since = ? WHERE id = ?")
                    .bind(now_unix)
                    .bind(rule.id.0.to_string())
                    .execute(pool)
                    .await?;
            }
            RuleTransition::ClearPending => {
                sqlx::query("UPDATE metric_rules SET breach_since = NULL WHERE id = ?")
                    .bind(rule.id.0.to_string())
                    .execute(pool)
                    .await?;
            }
            RuleTransition::Fire => {
                sqlx::query(
                    "UPDATE metric_rules SET breach_since = COALESCE(breach_since, ?), firing_at = ? WHERE id = ?",
                )
                .bind(now_unix)
                .bind(now_unix)
                .bind(rule.id.0.to_string())
                .execute(pool)
                .await?;
                out.push(RuleEvent {
                    rule,
                    transition,
                    value: fresh_value,
                });
            }
            RuleTransition::Resolve => {
                sqlx::query(
                    "UPDATE metric_rules SET breach_since = NULL, firing_at = NULL WHERE id = ?",
                )
                .bind(rule.id.0.to_string())
                .execute(pool)
                .await?;
                out.push(RuleEvent {
                    rule,
                    transition,
                    value: fresh_value,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_rule(name: &str, op: RuleOp, threshold: f64) -> NewMetricRule {
        NewMetricRule {
            name: name.into(),
            metric: "q_depth".into(),
            labels: serde_json::json!({ "job": "web" }),
            op,
            threshold,
            for_seconds: 0,
            enabled: true,
            channel_ids: vec![],
            escalation_policy_id: None,
        }
    }

    async fn push_sample(pool: &MySqlPool, value: f64) {
        let org = super::super::oid(DEF);
        let mut labels = BTreeMap::new();
        labels.insert("job".to_string(), "web".to_string());
        super::super::metric_samples::insert_many(
            pool,
            &[rampart_core::promtext::PromSample {
                name: "q_depth".into(),
                labels,
                value,
            }],
            org,
        )
        .await
        .unwrap();
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_and_evaluate_fire_resolve(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let r = create(&pool, new_rule("hot queue", RuleOp::Gt, 100.0), org)
            .await
            .unwrap();
        assert_eq!(r.op, RuleOp::Gt);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, r.id, org).await.unwrap().metric, "q_depth");

        // No samples → no transition.
        assert!(evaluate_tick(&pool).await.unwrap().is_empty());

        // Sample over threshold (for_seconds=0 → fires immediately).
        push_sample(&pool, 150.0).await;
        let fired = evaluate_tick(&pool).await.unwrap();
        assert_eq!(fired.len(), 1);
        assert!(matches!(fired[0].transition, RuleTransition::Fire));
        assert_eq!(fired[0].value, Some(150.0));
        assert!(get(&pool, r.id, org).await.unwrap().firing_at.is_some());

        // Sample back under threshold → resolves.
        push_sample(&pool, 10.0).await;
        let resolved = evaluate_tick(&pool).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert!(matches!(resolved[0].transition, RuleTransition::Resolve));
        assert!(get(&pool, r.id, org).await.unwrap().firing_at.is_none());

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

        // update clears breach state + cross-org isolation.
        let u = update(
            &pool,
            r.id,
            serde_json::from_value(serde_json::json!({ "threshold": 200.0 })).unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.threshold, 200.0);
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
