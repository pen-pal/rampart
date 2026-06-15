//! Threshold alert rules over ingested metrics (migration 0073).
//!
//! `evaluate_tick` owns the IO half of the rule state machine: read the
//! latest fresh sample per enabled rule, ask the pure
//! `rampart_core::metric_rule::rule_transition` what changed, persist
//! the new state, and hand the fired/resolved transitions back to the
//! caller (the scheduler) for notification fan-out. Keeping the sends
//! out of this crate preserves the layering — db never talks to
//! channels.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{MetricRuleId, NotificationId};
use rampart_core::metric_rule::{
    rule_transition, MetricRule, NewMetricRule, RuleOp, RuleTransition, UpdateMetricRule,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// A sample older than this is treated as "no data": a series that
/// stopped reporting resolves (and stays quiet) rather than firing
/// forever on its last breached value.
pub const SAMPLE_FRESHNESS_SECONDS: i64 = 900;
/// Trailing window the anomaly op computes its mean/stddev baseline over (6h).
pub const ANOMALY_BASELINE_SECONDS: i64 = 6 * 3600;

struct RuleRow {
    id: Uuid,
    name: String,
    metric: String,
    labels: serde_json::Value,
    op: String,
    threshold: f64,
    for_seconds: i32,
    enabled: bool,
    channel_ids: Vec<Uuid>,
    escalation_policy_id: Option<Uuid>,
    breach_since: Option<OffsetDateTime>,
    firing_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

impl From<RuleRow> for MetricRule {
    fn from(r: RuleRow) -> Self {
        MetricRule {
            id: MetricRuleId::from_uuid(r.id),
            name: r.name,
            metric: r.metric,
            labels: r.labels,
            op: RuleOp::from_db(&r.op),
            threshold: r.threshold,
            for_seconds: r.for_seconds,
            enabled: r.enabled,
            channel_ids: r
                .channel_ids
                .into_iter()
                .map(NotificationId::from_uuid)
                .collect(),
            escalation_policy_id: r
                .escalation_policy_id
                .map(rampart_core::ids::EscalationPolicyId::from_uuid),
            breach_since: r.breach_since,
            firing_at: r.firing_at,
            created_at: r.created_at,
        }
    }
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<MetricRule>> {
    let rows = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, name, metric, labels AS "labels!", op, threshold, for_seconds,
               enabled, channel_ids AS "channel_ids!", escalation_policy_id,
               breach_since, firing_at, created_at
        FROM metric_rules
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: MetricRuleId) -> DbResult<MetricRule> {
    let row = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, name, metric, labels AS "labels!", op, threshold, for_seconds,
               enabled, channel_ids AS "channel_ids!", escalation_policy_id,
               breach_since, firing_at, created_at
        FROM metric_rules
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewMetricRule) -> DbResult<MetricRule> {
    let id = MetricRuleId::new();
    let channel_ids: Vec<Uuid> = input.channel_ids.iter().map(|c| c.0).collect();
    sqlx::query!(
        r#"
        INSERT INTO metric_rules (id, name, metric, labels, op, threshold, for_seconds, enabled, channel_ids, escalation_policy_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        id.0,
        input.name,
        input.metric,
        input.labels,
        input.op.as_str(),
        input.threshold,
        input.for_seconds,
        input.enabled,
        &channel_ids,
        input.escalation_policy_id.map(|p| p.0),
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: MetricRuleId,
    patch: UpdateMetricRule,
) -> DbResult<MetricRule> {
    let channel_ids: Option<Vec<Uuid>> = patch.channel_ids.map(|v| v.iter().map(|c| c.0).collect());
    let result = sqlx::query!(
        r#"
        UPDATE metric_rules SET
            name        = COALESCE($2, name),
            metric      = COALESCE($3, metric),
            labels      = COALESCE($4, labels),
            op          = COALESCE($5, op),
            threshold   = COALESCE($6, threshold),
            for_seconds = COALESCE($7, for_seconds),
            enabled     = COALESCE($8, enabled),
            channel_ids = COALESCE($9, channel_ids),
            escalation_policy_id = COALESCE($10, escalation_policy_id),
            -- Any edit resets in-flight breach state: the old pending /
            -- firing markers describe the OLD condition.
            breach_since = NULL,
            firing_at    = NULL
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.metric,
        patch.labels,
        patch.op.map(|o| o.as_str().to_string()),
        patch.threshold,
        patch.for_seconds,
        patch.enabled,
        channel_ids.as_deref(),
        patch.escalation_policy_id.map(|p| p.0),
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: MetricRuleId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM metric_rules WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// One fired/resolved transition from an evaluation tick, with the
/// observed value for the alert text (None on resolve-by-staleness).
pub struct RuleEvent {
    pub rule: MetricRule,
    pub transition: RuleTransition,
    pub value: Option<f64>,
}

/// Evaluate every enabled rule against its series' latest fresh sample,
/// persist state changes, and return the transitions that need
/// notifications (Fire / Resolve only).
pub async fn evaluate_tick(pool: &DbPool) -> DbResult<Vec<RuleEvent>> {
    let now = OffsetDateTime::now_utc();
    let mut out = Vec::new();

    for rule in list(pool).await?.into_iter().filter(|r| r.enabled) {
        let latest = crate::metric_samples::latest(pool, &rule.metric, &rule.labels).await?;
        let fresh_value = latest.and_then(|(v, ts)| {
            ((now - ts).whole_seconds() < SAMPLE_FRESHNESS_SECONDS).then_some(v)
        });
        let breached = match fresh_value {
            // Anomaly: z-score against a rolling baseline (threshold = σ).
            Some(v) if rule.op.is_anomaly() => {
                match crate::metric_samples::baseline(
                    pool,
                    &rule.metric,
                    &rule.labels,
                    ANOMALY_BASELINE_SECONDS,
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
                sqlx::query!(
                    "UPDATE metric_rules SET breach_since = $2 WHERE id = $1",
                    rule.id.0,
                    now,
                )
                .execute(pool)
                .await?;
            }
            RuleTransition::ClearPending => {
                sqlx::query!(
                    "UPDATE metric_rules SET breach_since = NULL WHERE id = $1",
                    rule.id.0,
                )
                .execute(pool)
                .await?;
            }
            RuleTransition::Fire => {
                sqlx::query!(
                    "UPDATE metric_rules SET breach_since = COALESCE(breach_since, $2), firing_at = $2 WHERE id = $1",
                    rule.id.0,
                    now,
                )
                .execute(pool)
                .await?;
                out.push(RuleEvent {
                    rule,
                    transition,
                    value: fresh_value,
                });
            }
            RuleTransition::Resolve => {
                sqlx::query!(
                    "UPDATE metric_rules SET breach_since = NULL, firing_at = NULL WHERE id = $1",
                    rule.id.0,
                )
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
