//! Threshold alert rules over the observability tiers (migration 0081).
//!
//! Mirrors `metric_rules`: the same persisted state machine
//! (`breach_since` / `firing_at` + `for_seconds`) reusing
//! `rampart_core::metric_rule::rule_transition`. What differs is the observed
//! value — `evaluate_tick` computes a windowed aggregate over one tier,
//! selected by the rule's `kind`, then drives the identical fire/resolve
//! persistence and hands transitions back to the scheduler for fan-out.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{NotificationId, TelemetryRuleId};
use rampart_core::metric_rule::{rule_transition, RuleOp, RuleTransition};
use rampart_core::telemetry_rule::{
    NewTelemetryRule, TelemetryRule, TelemetryRuleKind, UpdateTelemetryRule,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct RuleRow {
    id: Uuid,
    name: String,
    kind: String,
    target: String,
    match_text: String,
    min_level: i16,
    op: String,
    threshold: f64,
    window_seconds: i32,
    for_seconds: i32,
    enabled: bool,
    channel_ids: Vec<Uuid>,
    escalation_policy_id: Option<Uuid>,
    breach_since: Option<OffsetDateTime>,
    firing_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

impl From<RuleRow> for TelemetryRule {
    fn from(r: RuleRow) -> Self {
        TelemetryRule {
            id: TelemetryRuleId::from_uuid(r.id),
            name: r.name,
            kind: TelemetryRuleKind::from_db(&r.kind),
            target: r.target,
            match_text: r.match_text,
            min_level: r.min_level,
            op: RuleOp::from_db(&r.op),
            threshold: r.threshold,
            window_seconds: r.window_seconds,
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

pub async fn list(pool: &DbPool) -> DbResult<Vec<TelemetryRule>> {
    let rows = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, name, kind, target, match_text, min_level, op, threshold,
               window_seconds, for_seconds, enabled,
               channel_ids AS "channel_ids!", escalation_policy_id,
               breach_since, firing_at, created_at
        FROM telemetry_alert_rules
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: TelemetryRuleId) -> DbResult<TelemetryRule> {
    let row = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, name, kind, target, match_text, min_level, op, threshold,
               window_seconds, for_seconds, enabled,
               channel_ids AS "channel_ids!", escalation_policy_id,
               breach_since, firing_at, created_at
        FROM telemetry_alert_rules
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewTelemetryRule) -> DbResult<TelemetryRule> {
    let id = TelemetryRuleId::new();
    let channel_ids: Vec<Uuid> = input.channel_ids.iter().map(|c| c.0).collect();
    sqlx::query!(
        r#"
        INSERT INTO telemetry_alert_rules
            (id, name, kind, target, match_text, min_level, op, threshold,
             window_seconds, for_seconds, enabled, channel_ids, escalation_policy_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        id.0,
        input.name,
        input.kind.as_str(),
        input.target,
        input.match_text,
        input.min_level,
        input.op.as_str(),
        input.threshold,
        input.window_seconds,
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
    id: TelemetryRuleId,
    patch: UpdateTelemetryRule,
) -> DbResult<TelemetryRule> {
    let channel_ids: Option<Vec<Uuid>> = patch.channel_ids.map(|v| v.iter().map(|c| c.0).collect());
    let result = sqlx::query!(
        r#"
        UPDATE telemetry_alert_rules SET
            name           = COALESCE($2, name),
            kind           = COALESCE($3, kind),
            target         = COALESCE($4, target),
            match_text     = COALESCE($5, match_text),
            min_level      = COALESCE($6, min_level),
            op             = COALESCE($7, op),
            threshold      = COALESCE($8, threshold),
            window_seconds = COALESCE($9, window_seconds),
            for_seconds    = COALESCE($10, for_seconds),
            enabled        = COALESCE($11, enabled),
            channel_ids    = COALESCE($12, channel_ids),
            escalation_policy_id = COALESCE($13, escalation_policy_id),
            -- Any edit resets in-flight breach state: the old markers
            -- describe the OLD condition.
            breach_since   = NULL,
            firing_at      = NULL
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.kind.map(|k| k.as_str().to_string()),
        patch.target,
        patch.match_text,
        patch.min_level,
        patch.op.map(|o| o.as_str().to_string()),
        patch.threshold,
        patch.window_seconds,
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

pub async fn delete(pool: &DbPool, id: TelemetryRuleId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM telemetry_alert_rules WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// One fired/resolved transition from an evaluation tick, with the observed
/// value for the alert text (None when the tier had no data in the window).
pub struct RuleEvent {
    pub rule: TelemetryRule,
    pub transition: RuleTransition,
    pub value: Option<f64>,
}

/// Compute the rule's observed value over its rolling window. `None` means
/// "no data" (no matching rows) — which resolves rather than fires, matching
/// the metric tier's staleness stance. Count-based kinds always return a
/// value (0 when empty); ratio/percentile kinds return `None` when empty.
async fn observe(pool: &DbPool, rule: &TelemetryRule) -> DbResult<Option<f64>> {
    let window = rule.window_seconds as f64;
    match rule.kind {
        TelemetryRuleKind::ErrorRate => {
            let n = sqlx::query_scalar!(
                r#"
                SELECT COUNT(*) AS "count!"
                FROM error_events e
                WHERE e.ts >= now() - make_interval(secs => $1)
                  AND ($2 = '' OR e.project_id =
                        (SELECT id FROM error_projects WHERE name = $2 LIMIT 1))
                "#,
                window,
                rule.target,
            )
            .fetch_one(pool)
            .await?;
            Ok(Some(n as f64))
        }
        TelemetryRuleKind::TraceLatency => {
            let p = sqlx::query_scalar!(
                r#"
                SELECT percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)
                FROM spans
                WHERE received_at >= now() - make_interval(secs => $1)
                  AND ($2 = '' OR service_name = $2)
                "#,
                window,
                rule.target,
            )
            .fetch_one(pool)
            .await?;
            Ok(p)
        }
        TelemetryRuleKind::TraceErrorRate => {
            let pct = sqlx::query_scalar!(
                r#"
                SELECT CASE WHEN COUNT(*) = 0 THEN NULL
                       ELSE 100.0 * (COUNT(*) FILTER (WHERE status_code = 2))::double precision
                            / COUNT(*) END
                FROM spans
                WHERE received_at >= now() - make_interval(secs => $1)
                  AND ($2 = '' OR service_name = $2)
                "#,
                window,
                rule.target,
            )
            .fetch_one(pool)
            .await?;
            Ok(pct)
        }
        TelemetryRuleKind::LogVolume => {
            let n = sqlx::query_scalar!(
                r#"
                SELECT COUNT(*) AS "count!"
                FROM logs
                WHERE ts >= now() - make_interval(secs => $1)
                  AND severity >= $2
                  AND ($3 = '' OR service_name = $3)
                  AND ($4 = '' OR body ILIKE '%' || $4 || '%')
                "#,
                window,
                rule.min_level,
                rule.target,
                rule.match_text,
            )
            .fetch_one(pool)
            .await?;
            Ok(Some(n as f64))
        }
        TelemetryRuleKind::ProfileSamples => {
            let n = sqlx::query_scalar!(
                r#"
                SELECT COALESCE(SUM(sample_count), 0)::bigint AS "count!"
                FROM profiles
                WHERE received_at >= now() - make_interval(secs => $1)
                  AND ($2 = '' OR service_name = $2)
                "#,
                window,
                rule.target,
            )
            .fetch_one(pool)
            .await?;
            Ok(Some(n as f64))
        }
        TelemetryRuleKind::RumLcpP75 => {
            let p = sqlx::query_scalar!(
                r#"
                SELECT percentile_cont(0.75) WITHIN GROUP (ORDER BY lcp_ms)
                FROM rum_events
                WHERE received_at >= now() - make_interval(secs => $1)
                  AND lcp_ms IS NOT NULL
                  AND ($2 = '' OR app = $2)
                "#,
                window,
                rule.target,
            )
            .fetch_one(pool)
            .await?;
            Ok(p)
        }
    }
}

/// Evaluate every enabled rule against its tier aggregate, persist state
/// changes, and return Fire / Resolve transitions for notification fan-out.
pub async fn evaluate_tick(pool: &DbPool) -> DbResult<Vec<RuleEvent>> {
    let now = OffsetDateTime::now_utc();
    let mut out = Vec::new();

    for rule in list(pool).await?.into_iter().filter(|r| r.enabled) {
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
                sqlx::query!(
                    "UPDATE telemetry_alert_rules SET breach_since = $2 WHERE id = $1",
                    rule.id.0,
                    now,
                )
                .execute(pool)
                .await?;
            }
            RuleTransition::ClearPending => {
                sqlx::query!(
                    "UPDATE telemetry_alert_rules SET breach_since = NULL WHERE id = $1",
                    rule.id.0,
                )
                .execute(pool)
                .await?;
            }
            RuleTransition::Fire => {
                sqlx::query!(
                    "UPDATE telemetry_alert_rules SET breach_since = COALESCE(breach_since, $2), firing_at = $2 WHERE id = $1",
                    rule.id.0,
                    now,
                )
                .execute(pool)
                .await?;
                out.push(RuleEvent {
                    rule,
                    transition,
                    value,
                });
            }
            RuleTransition::Resolve => {
                sqlx::query!(
                    "UPDATE telemetry_alert_rules SET breach_since = NULL, firing_at = NULL WHERE id = $1",
                    rule.id.0,
                )
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
