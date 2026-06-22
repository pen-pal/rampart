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
use rampart_core::ids::{EscalationPolicyId, MonitorId, OrgId, UserId};
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

/// Escalation policies owned by one org — the management list (org-scoped).
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>> {
    let rows = sqlx::query_as!(
        PolicyRow,
        r#"
        SELECT p.id, p.name, p.steps AS "steps!", p.created_at,
               COUNT(m.id) AS monitor_count
        FROM escalation_policies p
        LEFT JOIN monitors m ON m.escalation_policy_id = p.id
        WHERE p.org_id = $1
        GROUP BY p.id
        ORDER BY p.created_at
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Fetch one policy scoped to an org (cross-org id → NotFound). Request path.
pub async fn get(
    pool: &DbPool,
    id: EscalationPolicyId,
    org_id: OrgId,
) -> DbResult<EscalationPolicy> {
    let row = sqlx::query_as!(
        PolicyRow,
        r#"
        SELECT p.id, p.name, p.steps AS "steps!", p.created_at,
               COUNT(m.id) AS monitor_count
        FROM escalation_policies p
        LEFT JOIN monitors m ON m.escalation_policy_id = p.id
        WHERE p.id = $1 AND p.org_id = $2
        GROUP BY p.id
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Fetch one policy WITHOUT org scoping — for the scheduler/notifier escalation
/// page path (no request context) and the just-inserted-row return in
/// [`create`]. Not for request paths.
pub async fn get_unscoped(pool: &DbPool, id: EscalationPolicyId) -> DbResult<EscalationPolicy> {
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

pub async fn create(
    pool: &DbPool,
    input: NewEscalationPolicy,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<EscalationPolicy> {
    let id = EscalationPolicyId::new();
    sqlx::query!(
        "INSERT INTO escalation_policies (id, name, steps, org_id) VALUES ($1, $2, $3, $4)",
        id.0,
        input.name,
        serde_json::to_value(&input.steps).unwrap_or_else(|_| serde_json::json!([])),
        org_id.0,
    )
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: EscalationPolicyId,
    patch: UpdateEscalationPolicy,
    org_id: OrgId,
) -> DbResult<EscalationPolicy> {
    let steps = patch
        .steps
        .map(|s| serde_json::to_value(&s).unwrap_or_else(|_| serde_json::json!([])));
    let result = sqlx::query!(
        r#"
        UPDATE escalation_policies SET
            name  = COALESCE($2, name),
            steps = COALESCE($3, steps)
        WHERE id = $1 AND org_id = $4
        "#,
        id.0,
        patch.name,
        steps,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

pub async fn delete(pool: &DbPool, id: EscalationPolicyId, org_id: OrgId) -> DbResult<()> {
    // Monitors fall back to regular fan-out via ON DELETE SET NULL;
    // open episodes for this policy cascade away with it.
    let result = sqlx::query!(
        "DELETE FROM escalation_policies WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

struct EpisodeRow {
    id: Uuid,
    monitor_id: Option<Uuid>,
    subject_kind: String,
    subject_ref: String,
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
            monitor_id: r.monitor_id.map(MonitorId::from_uuid),
            subject_kind: r.subject_kind,
            subject_ref: r.subject_ref,
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
        INSERT INTO escalation_episodes
            (id, monitor_id, subject_kind, subject_ref, policy_id, next_escalation_at)
        VALUES ($1, $2, 'monitor', $3, $4, $5)
        ON CONFLICT (subject_kind, subject_ref) WHERE resolved_at IS NULL DO NOTHING
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        Uuid::now_v7(),
        monitor_id.0,
        monitor_id.0.to_string(),
        policy.id.0,
        next,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Open an episode for a non-monitor subject (e.g. a telemetry rule). `kind` +
/// `subject_ref` are the generic key; `monitor_id` stays NULL. Returns None if
/// one is already open for the subject.
pub async fn open_episode_for_subject(
    pool: &DbPool,
    kind: &str,
    subject_ref: &str,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let next = next_due(policy, 0);
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        INSERT INTO escalation_episodes
            (id, subject_kind, subject_ref, policy_id, next_escalation_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (subject_kind, subject_ref) WHERE resolved_at IS NULL DO NOTHING
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        Uuid::now_v7(),
        kind,
        subject_ref,
        policy.id.0,
        next,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Resolve the open episode for a generic subject (rule recovered). Returns it,
/// or None if nothing was open.
pub async fn resolve_subject(
    pool: &DbPool,
    kind: &str,
    subject_ref: &str,
) -> DbResult<Option<EscalationEpisode>> {
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        UPDATE escalation_episodes SET resolved_at = NOW()
        WHERE subject_kind = $1 AND subject_ref = $2 AND resolved_at IS NULL
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        kind,
        subject_ref,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Acknowledge an episode by id (subject-agnostic) — stops the ladder.
pub async fn ack_episode(
    pool: &DbPool,
    episode_id: Uuid,
    by: UserId,
) -> DbResult<EscalationEpisode> {
    let row = sqlx::query_as!(
        EpisodeRow,
        r#"
        UPDATE escalation_episodes
        SET acked_at = NOW(), acked_by = $2
        WHERE id = $1 AND resolved_at IS NULL AND acked_at IS NULL
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
                  next_escalation_at, acked_at, acked_by, resolved_at
        "#,
        episode_id,
        by.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// All currently-open episodes (any subject) for the on-call dashboard.
pub async fn list_open(pool: &DbPool) -> DbResult<Vec<EscalationEpisode>> {
    let rows = sqlx::query_as!(
        EpisodeRow,
        r#"
        SELECT id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
               next_escalation_at, acked_at, acked_by, resolved_at
        FROM escalation_episodes
        WHERE resolved_at IS NULL
        ORDER BY started_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Org-scoped twin of [`list_open`] for the authenticated dashboard: only
/// episodes whose owning policy belongs to `org_id`. Every episode has a
/// NOT-NULL `policy_id`, so the join covers monitor- and rule-subject episodes
/// alike. The unscoped [`list_open`] stays for system/test callers.
pub async fn list_open_for_org(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<EscalationEpisode>> {
    let rows = sqlx::query_as!(
        EpisodeRow,
        r#"
        SELECT e.id, e.monitor_id, e.subject_kind, e.subject_ref, e.policy_id, e.started_at,
               e.last_step, e.next_escalation_at, e.acked_at, e.acked_by, e.resolved_at
        FROM escalation_episodes e
        JOIN escalation_policies p ON p.id = e.policy_id
        WHERE e.resolved_at IS NULL AND p.org_id = $1
        ORDER BY e.started_at DESC
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// 404-gate: succeed only when `episode`'s owning policy belongs to `org`.
/// Used by the subject-agnostic ack-by-episode-id handler, which carries no
/// monitor in the path.
pub async fn episode_in_org(pool: &DbPool, episode: Uuid, org_id: OrgId) -> DbResult<()> {
    let ok = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM escalation_episodes e
            JOIN escalation_policies p ON p.id = e.policy_id
            WHERE e.id = $1 AND p.org_id = $2
        )
        "#,
        episode,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    if ok.unwrap_or(false) {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
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
        SELECT id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
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
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
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
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
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
        RETURNING id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
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
        SELECT id, monitor_id, subject_kind, subject_ref, policy_id, started_at, last_step,
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
