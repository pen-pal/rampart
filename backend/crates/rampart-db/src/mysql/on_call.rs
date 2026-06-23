//! MySQL `on_call` domain — on-call schedules + the notifier's "who's on call
//! now" resolver. Ported from PG. Rotation math is pure `rampart_core::on_call`,
//! reused verbatim; this only fetches the row. JSONB participant rings→LONGTEXT;
//! anchor timestamptz→BIGINT; no `RETURNING` → INSERT-then-re-select; `update`
//! drops the rows_affected gate (changed-vs-matched → trailing get()).

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::ids::{NotificationId, OnCallScheduleId, OrgId};
use rampart_core::on_call::{
    on_call_channel, on_call_target, NewOnCallSchedule, OnCallSchedule, OnCallTarget,
    UpdateOnCallSchedule,
};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

fn schedule_from(r: &sqlx::mysql::MySqlRow) -> OnCallSchedule {
    OnCallSchedule {
        id: OnCallScheduleId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        rotation_seconds: r.get::<i64, _>("rotation_seconds"),
        anchor: ts(r.get::<i64, _>("anchor")),
        participant_ids: serde_json::from_str(&r.get::<String, _>("participant_ids"))
            .unwrap_or_default(),
        participant_user_ids: serde_json::from_str(&r.get::<String, _>("participant_user_ids"))
            .unwrap_or_default(),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

const COLS: &str = "id, name, rotation_seconds, anchor, participant_ids, participant_user_ids, \
     created_at";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<OnCallSchedule>> {
    let sql = format!("SELECT {COLS} FROM on_call_schedules WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(schedule_from).collect())
}

pub async fn get(
    pool: &MySqlPool,
    id: OnCallScheduleId,
    org_id: OrgId,
) -> DbResult<OnCallSchedule> {
    let sql = format!("SELECT {COLS} FROM on_call_schedules WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(schedule_from(&row))
}

pub async fn get_unscoped(pool: &MySqlPool, id: OnCallScheduleId) -> DbResult<OnCallSchedule> {
    let sql = format!("SELECT {COLS} FROM on_call_schedules WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(schedule_from(&row))
}

pub async fn create(
    pool: &MySqlPool,
    input: NewOnCallSchedule,
    org_id: OrgId,
) -> DbResult<OnCallSchedule> {
    let id = OnCallScheduleId::new();
    sqlx::query(
        "INSERT INTO on_call_schedules
            (id, name, rotation_seconds, anchor, participant_ids, participant_user_ids, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.rotation_seconds)
    .bind(input.anchor.unix_timestamp())
    .bind(serde_json::to_string(&input.participant_ids).unwrap_or_else(|_| "[]".into()))
    .bind(serde_json::to_string(&input.participant_user_ids).unwrap_or_else(|_| "[]".into()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: OnCallScheduleId,
    patch: UpdateOnCallSchedule,
    org_id: OrgId,
) -> DbResult<OnCallSchedule> {
    let participants = patch
        .participant_ids
        .map(|p| serde_json::to_string(&p).unwrap_or_else(|_| "[]".into()));
    let user_participants = patch
        .participant_user_ids
        .map(|p| serde_json::to_string(&p).unwrap_or_else(|_| "[]".into()));
    sqlx::query(
        "UPDATE on_call_schedules SET
            name                 = COALESCE(?, name),
            rotation_seconds     = COALESCE(?, rotation_seconds),
            anchor               = COALESCE(?, anchor),
            participant_ids      = COALESCE(?, participant_ids),
            participant_user_ids = COALESCE(?, participant_user_ids)
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.rotation_seconds)
    .bind(patch.anchor.map(|a| a.unix_timestamp()))
    .bind(participants)
    .bind(user_participants)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // NotFound surfaces here (no rows_affected gate — MySQL changed-vs-matched).
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: OnCallScheduleId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM on_call_schedules WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// The channel on call at `at`; `None` for an empty/malformed ring.
pub async fn current_channel(
    pool: &MySqlPool,
    id: OnCallScheduleId,
    at: OffsetDateTime,
) -> DbResult<Option<NotificationId>> {
    let schedule = get_unscoped(pool, id).await?;
    Ok(on_call_channel(&schedule, at))
}

/// Who is on call now — a channel or a user — over the combined ring.
pub async fn current_target(
    pool: &MySqlPool,
    id: OnCallScheduleId,
    at: OffsetDateTime,
) -> DbResult<Option<OnCallTarget>> {
    let schedule = get_unscoped(pool, id).await?;
    Ok(on_call_target(&schedule, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_and_resolve(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let ch = NotificationId::new();
        let s = create(
            &pool,
            NewOnCallSchedule {
                name: "primary".into(),
                rotation_seconds: 86400,
                anchor: OffsetDateTime::now_utc() - time::Duration::hours(1),
                participant_ids: vec![ch],
                participant_user_ids: vec![],
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(s.name, "primary");
        assert_eq!(s.participant_ids, vec![ch]);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // single-participant ring → that channel is on call now.
        let now = OffsetDateTime::now_utc();
        assert_eq!(current_channel(&pool, s.id, now).await.unwrap(), Some(ch));
        assert!(current_target(&pool, s.id, now).await.unwrap().is_some());

        // no-op update must not 404 (changed-vs-matched).
        let u = update(
            &pool,
            s.id,
            serde_json::from_value(serde_json::json!({ "name": "primary-v2" })).unwrap(),
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.name, "primary-v2");

        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, s.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, s.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, s.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
