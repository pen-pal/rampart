//! SLO persistence + evaluation (migration 0098).
//!
//! Mirrors the metric-rule layering: this crate computes the indicator ratio
//! and runs the budget state machine via the pure
//! [`rampart_core::slo`] helpers, persisting the `breaching_at` de-dup marker
//! and handing fire/resolve transitions back to the scheduler for fan-out. It
//! never talks to notification channels.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{EscalationPolicyId, MonitorId, NotificationId, SloId};
use rampart_core::slo::{
    slo_transition, snapshot, NewSlo, SliKind, Slo, SloSnapshot, SloTransition, UpdateSlo,
};
use time::OffsetDateTime;
use uuid::Uuid;

/// Short window for the fast-burn signal.
const FAST_BURN_WINDOW_SECONDS: i64 = 3600;

struct SloRow {
    id: Uuid,
    name: String,
    description: String,
    sli_kind: String,
    monitor_id: Option<Uuid>,
    good_metric: Option<String>,
    total_metric: Option<String>,
    labels: serde_json::Value,
    objective_pct: f64,
    window_days: i32,
    enabled: bool,
    channel_ids: Vec<Uuid>,
    escalation_policy_id: Option<Uuid>,
    breaching_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

impl From<SloRow> for Slo {
    fn from(r: SloRow) -> Self {
        Slo {
            id: SloId::from_uuid(r.id),
            name: r.name,
            description: r.description,
            sli_kind: SliKind::from_db(&r.sli_kind),
            monitor_id: r.monitor_id.map(MonitorId::from_uuid),
            good_metric: r.good_metric,
            total_metric: r.total_metric,
            labels: r.labels,
            objective_pct: r.objective_pct,
            window_days: r.window_days,
            enabled: r.enabled,
            channel_ids: r
                .channel_ids
                .into_iter()
                .map(NotificationId::from_uuid)
                .collect(),
            escalation_policy_id: r.escalation_policy_id.map(EscalationPolicyId::from_uuid),
            breaching_at: r.breaching_at,
            created_at: r.created_at,
        }
    }
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<Slo>> {
    let rows = sqlx::query_as!(
        SloRow,
        r#"
        SELECT id, name, description, sli_kind, monitor_id, good_metric, total_metric,
               labels AS "labels!", objective_pct AS "objective_pct!: f64", window_days, enabled,
               channel_ids AS "channel_ids!", escalation_policy_id, breaching_at, created_at
        FROM slos
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: SloId) -> DbResult<Slo> {
    let row = sqlx::query_as!(
        SloRow,
        r#"
        SELECT id, name, description, sli_kind, monitor_id, good_metric, total_metric,
               labels AS "labels!", objective_pct AS "objective_pct!: f64", window_days, enabled,
               channel_ids AS "channel_ids!", escalation_policy_id, breaching_at, created_at
        FROM slos
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewSlo) -> DbResult<Slo> {
    let id = SloId::new();
    let channel_ids: Vec<Uuid> = input.channel_ids.iter().map(|c| c.0).collect();
    sqlx::query!(
        r#"
        INSERT INTO slos (id, name, description, sli_kind, monitor_id, good_metric, total_metric,
                          labels, objective_pct, window_days, enabled, channel_ids, escalation_policy_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        id.0,
        input.name,
        input.description,
        input.sli_kind.as_str(),
        input.monitor_id.map(|m| m.0),
        input.good_metric,
        input.total_metric,
        input.labels,
        input.objective_pct,
        input.window_days,
        input.enabled,
        &channel_ids,
        input.escalation_policy_id.map(|p| p.0),
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(pool: &DbPool, id: SloId, patch: UpdateSlo) -> DbResult<Slo> {
    let channel_ids: Option<Vec<Uuid>> =
        patch.channel_ids.map(|v| v.iter().map(|c| c.0).collect());
    let result = sqlx::query!(
        r#"
        UPDATE slos SET
            name          = COALESCE($2, name),
            description   = COALESCE($3, description),
            good_metric   = COALESCE($4, good_metric),
            total_metric  = COALESCE($5, total_metric),
            labels        = COALESCE($6, labels),
            objective_pct = COALESCE($7, objective_pct),
            window_days   = COALESCE($8, window_days),
            enabled       = COALESCE($9, enabled),
            channel_ids   = COALESCE($10, channel_ids),
            escalation_policy_id = COALESCE($11, escalation_policy_id),
            -- An edit redefines the objective; clear the in-flight breach marker.
            breaching_at  = NULL
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.description,
        patch.good_metric,
        patch.total_metric,
        patch.labels,
        patch.objective_pct,
        patch.window_days,
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

pub async fn delete(pool: &DbPool, id: SloId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM slos WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Up / total heartbeat ratio (0–100) for a monitor since `since`, or `None`
/// when the window held no heartbeats.
async fn monitor_ratio(
    pool: &DbPool,
    monitor: Uuid,
    since: OffsetDateTime,
) -> DbResult<Option<f64>> {
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "total!", COUNT(*) FILTER (WHERE status = 'up') AS "ok!"
        FROM heartbeats WHERE monitor_id = $1 AND ts >= $2
        "#,
        monitor,
        since,
    )
    .fetch_one(pool)
    .await?;
    if row.total == 0 {
        return Ok(None);
    }
    Ok(Some(row.ok as f64 / row.total as f64 * 100.0))
}

/// SUM(good) / SUM(total) ratio (0–100) over matching metric samples since
/// `since`, or `None` when no `total` was observed.
async fn metric_ratio(
    pool: &DbPool,
    good: &str,
    total: &str,
    labels: &serde_json::Value,
    since: OffsetDateTime,
) -> DbResult<Option<f64>> {
    let row = sqlx::query!(
        r#"
        SELECT
            COALESCE(SUM(value) FILTER (WHERE name = $1), 0)::float8 AS "good!",
            COALESCE(SUM(value) FILTER (WHERE name = $2), 0)::float8 AS "total!"
        FROM metric_samples
        WHERE name IN ($1, $2) AND ts >= $3 AND labels @> $4
        "#,
        good,
        total,
        since,
        labels,
    )
    .fetch_one(pool)
    .await?;
    if row.total <= 0.0 {
        return Ok(None);
    }
    Ok(Some((row.good / row.total * 100.0).clamp(0.0, 100.0)))
}

/// Achieved ratio over an arbitrary window for one SLO.
async fn achieved(pool: &DbPool, slo: &Slo, window_seconds: i64) -> DbResult<Option<f64>> {
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_seconds);
    match slo.sli_kind {
        SliKind::Monitor => match slo.monitor_id {
            Some(m) => monitor_ratio(pool, m.0, since).await,
            None => Ok(None),
        },
        SliKind::Metric => match (&slo.good_metric, &slo.total_metric) {
            (Some(g), Some(t)) => metric_ratio(pool, g, t, &slo.labels, since).await,
            _ => Ok(None),
        },
    }
}

/// Compute the live budget snapshot for one SLO (full window + 1h fast-burn).
pub async fn compute(pool: &DbPool, slo: &Slo) -> DbResult<SloSnapshot> {
    let full = achieved(pool, slo, slo.window_days as i64 * 86400).await?;
    let short = achieved(pool, slo, FAST_BURN_WINDOW_SECONDS).await?;
    Ok(snapshot(full, short, slo.objective_pct))
}

/// One SLO row with its freshly computed snapshot — what the API hands the UI.
pub struct SloWithSnapshot {
    pub slo: Slo,
    pub snapshot: SloSnapshot,
}

pub async fn list_with_snapshots(pool: &DbPool) -> DbResult<Vec<SloWithSnapshot>> {
    let mut out = Vec::new();
    for slo in list(pool).await? {
        let snapshot = compute(pool, &slo).await?;
        out.push(SloWithSnapshot { slo, snapshot });
    }
    Ok(out)
}

/// A fired/resolved SLO transition from an evaluation tick.
pub struct SloEvent {
    pub slo: Slo,
    pub transition: SloTransition,
    pub snapshot: SloSnapshot,
}

/// Evaluate every enabled SLO, persist the breach marker, and return the
/// transitions needing notification (Fire / Resolve).
pub async fn evaluate_tick(pool: &DbPool) -> DbResult<Vec<SloEvent>> {
    let now = OffsetDateTime::now_utc();
    let mut out = Vec::new();

    for slo in list(pool).await?.into_iter().filter(|s| s.enabled) {
        let snapshot = compute(pool, &slo).await?;
        let transition = slo_transition(slo.breaching_at, snapshot.breaching);
        match transition {
            SloTransition::None => {}
            SloTransition::Fire => {
                sqlx::query!("UPDATE slos SET breaching_at = $2 WHERE id = $1", slo.id.0, now)
                    .execute(pool)
                    .await?;
                out.push(SloEvent {
                    slo,
                    transition,
                    snapshot,
                });
            }
            SloTransition::Resolve => {
                sqlx::query!("UPDATE slos SET breaching_at = NULL WHERE id = $1", slo.id.0)
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
