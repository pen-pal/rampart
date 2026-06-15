//! On-call schedules (migration 0075).
//!
//! Thin CRUD over `on_call_schedules` plus `current_channel`, which the
//! notifier calls to resolve an escalation step's `schedule_ids` to the
//! channel on call right now. The rotation math itself is pure and lives
//! in `rampart_core::on_call`; this module only fetches the row.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{NotificationId, OnCallScheduleId};
use rampart_core::on_call::{
    on_call_target, NewOnCallSchedule, OnCallSchedule, OnCallTarget, UpdateOnCallSchedule,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct ScheduleRow {
    id: Uuid,
    name: String,
    rotation_seconds: i64,
    anchor: OffsetDateTime,
    participant_ids: serde_json::Value,
    participant_user_ids: serde_json::Value,
    created_at: OffsetDateTime,
}

impl From<ScheduleRow> for OnCallSchedule {
    fn from(r: ScheduleRow) -> Self {
        OnCallSchedule {
            id: OnCallScheduleId::from_uuid(r.id),
            name: r.name,
            rotation_seconds: r.rotation_seconds,
            anchor: r.anchor,
            // Validated on write; a malformed blob (manual SQL edit)
            // degrades to an empty ring rather than a panic — an empty
            // ring just resolves to "no one on call" and gets skipped.
            participant_ids: serde_json::from_value(r.participant_ids).unwrap_or_default(),
            participant_user_ids: serde_json::from_value(r.participant_user_ids).unwrap_or_default(),
            created_at: r.created_at,
        }
    }
}

pub async fn list(pool: &DbPool) -> DbResult<Vec<OnCallSchedule>> {
    let rows = sqlx::query_as!(
        ScheduleRow,
        r#"
        SELECT id, name, rotation_seconds, anchor,
               participant_ids AS "participant_ids!",
               participant_user_ids AS "participant_user_ids!", created_at
        FROM on_call_schedules
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: OnCallScheduleId) -> DbResult<OnCallSchedule> {
    let row = sqlx::query_as!(
        ScheduleRow,
        r#"
        SELECT id, name, rotation_seconds, anchor,
               participant_ids AS "participant_ids!",
               participant_user_ids AS "participant_user_ids!", created_at
        FROM on_call_schedules
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(pool: &DbPool, input: NewOnCallSchedule) -> DbResult<OnCallSchedule> {
    let id = OnCallScheduleId::new();
    sqlx::query!(
        r#"
        INSERT INTO on_call_schedules
            (id, name, rotation_seconds, anchor, participant_ids, participant_user_ids)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id.0,
        input.name,
        input.rotation_seconds,
        input.anchor,
        serde_json::to_value(&input.participant_ids).unwrap_or_else(|_| serde_json::json!([])),
        serde_json::to_value(&input.participant_user_ids).unwrap_or_else(|_| serde_json::json!([])),
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: OnCallScheduleId,
    patch: UpdateOnCallSchedule,
) -> DbResult<OnCallSchedule> {
    let participants = patch
        .participant_ids
        .map(|p| serde_json::to_value(&p).unwrap_or_else(|_| serde_json::json!([])));
    let user_participants = patch
        .participant_user_ids
        .map(|p| serde_json::to_value(&p).unwrap_or_else(|_| serde_json::json!([])));
    let result = sqlx::query!(
        r#"
        UPDATE on_call_schedules SET
            name                 = COALESCE($2, name),
            rotation_seconds     = COALESCE($3, rotation_seconds),
            anchor               = COALESCE($4, anchor),
            participant_ids      = COALESCE($5, participant_ids),
            participant_user_ids = COALESCE($6, participant_user_ids)
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.rotation_seconds,
        patch.anchor,
        participants,
        user_participants,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: OnCallScheduleId) -> DbResult<()> {
    // Escalation steps reference schedules by id in JSONB (no FK); a step
    // pointing at a deleted schedule simply resolves to nothing and is
    // skipped at page time, exactly like an unresolvable channel id.
    let result = sqlx::query!("DELETE FROM on_call_schedules WHERE id = $1", id.0)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// The channel on call for this schedule at `at`. `Ok(None)` when the ring
/// is empty/malformed; `Err(NotFound)` when the schedule itself is gone.
pub async fn current_channel(
    pool: &DbPool,
    id: OnCallScheduleId,
    at: OffsetDateTime,
) -> DbResult<Option<NotificationId>> {
    let schedule = get(pool, id).await?;
    Ok(rampart_core::on_call::on_call_channel(&schedule, at))
}

/// Who is on call now — a channel or a user — over the combined ring.
pub async fn current_target(
    pool: &DbPool,
    id: OnCallScheduleId,
    at: OffsetDateTime,
) -> DbResult<Option<OnCallTarget>> {
    let schedule = get(pool, id).await?;
    Ok(on_call_target(&schedule, at))
}
