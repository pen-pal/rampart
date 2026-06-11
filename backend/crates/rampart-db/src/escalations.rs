//! Escalation policies + episodes (migration 0074).
//!
//! The episode state machine lives here; the notifier opens/resolves
//! episodes on status flips and the scheduler advances due ones. All
//! mutations are single statements over the partial unique index
//! ("one open episode per monitor"), so flapping monitors and racing
//! ticks can't double-open or double-advance a ladder.

use crate::{DbError, DbPool, DbResult};
use rampart_core::escalation::{
    EscalationEpisode, EscalationPolicy, EscalationStep, NewEscalationPolicy,
    UpdateEscalationPolicy,
};
use rampart_core::ids::{EscalationPolicyId, MonitorId, UserId};
use time::OffsetDateTime;
use uuid::Uuid;

struct PolicyRow {
    id: Uuid,
    name: String,
    steps: serde_json::Value,
    created_at: OffsetDateTime,
    monitor_count: Option<i64>,
}

impl From<PolicyRow> for EscalationPolicy {
    fn from(r: PolicyRow) -> Self {
        EscalationPolicy {
            id: EscalationPolicyId::from_uuid(r.id),
            name: r.name,
            // Steps were validated on write; a malformed blob (manual
            // SQL edit) degrades to an empty ladder rather than a panic.
            steps: serde_json::from_value(r.steps).unwrap_or_default(),
            created_at: r.created_at,
            monitor_count: r.monitor_count.unwrap_or(0),
        }
    }
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<EscalationPolicy>> {
    let rows = sqlx::query_as!(
        PolicyRow,
        r#"
        SELECT p.id, p.name, p.steps AS "steps!", p.created_at,
               COUNT(m.id) AS monitor_count
        FROM escalation_policies p
        LEFT JOIN monitors m ON m.escalation_policy_id = p.id
        GROUP BY p.id
        ORDER BY p.created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: EscalationPolicyId) -> DbResult<EscalationPolicy> {
    let row = sqlx::query_as!(
        PolicyRow,
        r#"
        SELECT p.id, p.name, p.steps AS "steps!", p.created_at,
               COUNT(m.id) AS monitor_count
        FROM escalation_policies p
        LEFT JOIN monitors m ON m.escalation_policy_id = p.id
        WHERE p.id = $1
        GROUP BY p.id
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewEscalationPolicy) -> DbResult<EscalationPolicy> {
    let id = EscalationPolicyId::new();
    sqlx::query!(
        "INSERT INTO escalation_policies (id, name, steps) VALUES ($1, $2, $3)",
        id.0,
        input.name,
        serde_json::to_value(&input.steps).unwrap_or_else(|_| serde_json::json!([])),
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: EscalationPolicyId,
    patch: UpdateEscalationPolicy,
) -> DbResult<EscalationPolicy> {
    let steps = patch
        .steps
        .map(|s| serde_json::to_value(&s).unwrap_or_else(|_| serde_json::json!([])));
    let result = sqlx::query!(
        r#"
        UPDATE escalation_policies SET
            name  = COALESCE($2, name),
            steps = COALESCE($3, steps)
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        steps,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: EscalationPolicyId) -> DbResult<()> {
    // Monitors fall back to regular fan-out via ON DELETE SET NULL;
    // open episodes for this policy cascade away with it.
    let result = sqlx::query!("DELETE FROM escalation_policies WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

struct EpisodeRow {
    id: Uuid,
    monitor_id: Uuid,
    policy_id: Uuid,
    started_at: OffsetDateTime,
    last_step: i32,
    next_escalation_at: Option<OffsetDateTime>,
    acked_at: Option<OffsetDateTime>,
    acked_by: Option<Uuid>,
    resolved_at: Option<OffsetDateTime>,
}

impl From<EpisodeRow> for EscalationEpisode {
    fn from(r: EpisodeRow) -> Self {
        EscalationEpisode {
            id: r.id,
            monitor_id: MonitorId::from_uuid(r.monitor_id),
            policy_id: EscalationPolicyId::from_uuid(r.policy_id),
            started_at: r.started_at,
            last_step: r.last_step,
            next_escalation_at: r.next_escalation_at,
            acked_at: r.acked_at,
            acked_by: r.acked_by.map(UserId::from_uuid),
            resolved_at: r.resolved_at,
        }
    }
}

/// Open an episode for a monitor going Down. Returns None if one is
/// already open (flap protection via the partial unique index) — the
/// caller then skips step-0 firing too.
pub async fn open_episode(
    pool: &DbPool,
    monitor_id: MonitorId,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let next = next_due(policy, 0);
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        INSERT INTO escalation_episodes (id, monitor_id, policy_id, next_escalation_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (monitor_id) WHERE resolved_at IS NULL DO NOTHING
        RETURNING id, monitor_id, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        Uuid::now_v7(),
        monitor_id.0,
        policy.id.0,
        next,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// When the step AFTER `fired_step` becomes due, or None if the ladder
/// is exhausted.
fn next_due(policy: &EscalationPolicy, fired_step: i32) -> Option<OffsetDateTime> {
    policy
        .steps
        .get(fired_step as usize + 1)
        .map(|s: &EscalationStep| {
            OffsetDateTime::now_utc() + time::Duration::seconds(s.wait_seconds)
        })
}

/// The monitor's open episode, if any.
pub async fn open_for_monitor(
    pool: &DbPool,
    monitor_id: MonitorId,
) -> DbResult<Option<EscalationEpisode>> {
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        SELECT id, monitor_id, policy_id, started_at, last_step,
               next_escalation_at, acked_at, acked_by, resolved_at
        FROM escalation_episodes
        WHERE monitor_id = $1 AND resolved_at IS NULL
        "#,
        monitor_id.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Acknowledge the open episode: stops the ladder (the advance scan
/// skips acked rows). NotFound when nothing is open.
pub async fn ack(pool: &DbPool, monitor_id: MonitorId, by: UserId) -> DbResult<EscalationEpisode> {
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        UPDATE escalation_episodes
        SET acked_at = NOW(), acked_by = $2
        WHERE monitor_id = $1 AND resolved_at IS NULL AND acked_at IS NULL
        RETURNING id, monitor_id, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        monitor_id.0,
        by.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Close the open episode on recovery. Returns it (for recovery
/// fan-out to the steps already paged), or None if nothing was open.
pub async fn resolve(pool: &DbPool, monitor_id: MonitorId) -> DbResult<Option<EscalationEpisode>> {
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        UPDATE escalation_episodes
        SET resolved_at = NOW()
        WHERE monitor_id = $1 AND resolved_at IS NULL
        RETURNING id, monitor_id, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        monitor_id.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Atomically claim one due advance: bump `last_step` and recompute the
/// next deadline. The WHERE re-checks due-ness so two racing ticks
/// can't both claim the same step. Returns the post-bump episode.
pub async fn advance(
    pool: &DbPool,
    episode_id: Uuid,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    // Compute the new deadline from what last_step will become; read
    // current first under the due predicate.
    let current = sqlx::query!(
        r#"
        SELECT last_step FROM escalation_episodes
        WHERE id = $1 AND resolved_at IS NULL AND acked_at IS NULL
          AND next_escalation_at IS NOT NULL AND next_escalation_at <= NOW()
        "#,
        episode_id,
    )
    .fetch_optional(pool)
    .await?;
    let Some(cur) = current else { return Ok(None) };
    let new_step = cur.last_step + 1;
    let next = next_due(policy, new_step);

    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        UPDATE escalation_episodes
        SET last_step = $2, next_escalation_at = $3
        WHERE id = $1 AND resolved_at IS NULL AND acked_at IS NULL
          AND last_step = $4
        RETURNING id, monitor_id, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        episode_id,
        new_step,
        next,
        cur.last_step,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Episodes whose next step is due — the scheduler's advance scan.
pub async fn due(pool: &DbPool) -> DbResult<Vec<EscalationEpisode>> {
    let rows = sqlx::query_as!(
        EpisodeRow,
        r#"
        SELECT id, monitor_id, policy_id, started_at, last_step,
               next_escalation_at, acked_at, acked_by, resolved_at
        FROM escalation_episodes
        WHERE resolved_at IS NULL AND acked_at IS NULL
          AND next_escalation_at IS NOT NULL AND next_escalation_at <= NOW()
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}
