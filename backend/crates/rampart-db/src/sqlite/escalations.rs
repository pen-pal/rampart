//! SQLite `escalations` domain — escalation policies + the episode state
//! machine. Standalone (no cross-domain reads). Mirrors the full PG surface:
//! policy CRUD (list/get/get_unscoped/create/update/delete) + episodes
//! (open_episode / open_episode_for_subject / resolve_subject / ack_episode /
//! list_open / list_open_for_org / episode_in_org / open_for_monitor / ack /
//! resolve / advance / due).
//!
//! Dialect: uuid→TEXT, jsonb steps→TEXT, timestamps→INTEGER unix-seconds,
//! `NOW()`→`unixepoch()`. The "one open episode per subject" partial unique
//! index ports verbatim, incl. partial-target `ON CONFLICT(...) WHERE … DO
//! NOTHING` (SQLite 3.35+). `next_due` is pure Rust (same as PG).

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::escalation::{
    EscalationEpisode, EscalationPolicy, EscalationStep, NewEscalationPolicy,
    UpdateEscalationPolicy,
};
use rampart_core::ids::{EscalationPolicyId, MonitorId, OrgId, UserId};
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;
use uuid::Uuid;

fn policy_from(r: &sqlx::sqlite::SqliteRow) -> EscalationPolicy {
    EscalationPolicy {
        id: EscalationPolicyId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        steps: serde_json::from_str(&r.get::<String, _>("steps")).unwrap_or_default(),
        created_at: ts(r.get::<i64, _>("created_at")),
        monitor_count: r.get::<i64, _>("monitor_count"),
    }
}

fn episode_from(r: &sqlx::sqlite::SqliteRow) -> EscalationEpisode {
    EscalationEpisode {
        id: raw_uuid(&r.get::<String, _>("id")),
        monitor_id: r
            .get::<Option<String>, _>("monitor_id")
            .map(|s| MonitorId::from_uuid(raw_uuid(&s))),
        subject_kind: r.get("subject_kind"),
        subject_ref: r.get("subject_ref"),
        policy_id: EscalationPolicyId::from_uuid(raw_uuid(&r.get::<String, _>("policy_id"))),
        started_at: ts(r.get::<i64, _>("started_at")),
        last_step: r.get::<i64, _>("last_step") as i32,
        next_escalation_at: r.get::<Option<i64>, _>("next_escalation_at").map(ts),
        acked_at: r.get::<Option<i64>, _>("acked_at").map(ts),
        acked_by: r
            .get::<Option<String>, _>("acked_by")
            .map(|s| UserId::from_uuid(raw_uuid(&s))),
        resolved_at: r.get::<Option<i64>, _>("resolved_at").map(ts),
    }
}

/// The 11 episode columns every read/RETURNING projects, in a stable order.
const EP_COLS: &str = "id, monitor_id, subject_kind, subject_ref, policy_id, started_at, \
     last_step, next_escalation_at, acked_at, acked_by, resolved_at";

/// When the step after `fired_step` is due, or None if the ladder is exhausted.
/// Pure (same as PG `next_due`).
fn next_due(policy: &EscalationPolicy, fired_step: i32) -> Option<OffsetDateTime> {
    policy
        .steps
        .get(fired_step as usize + 1)
        .map(|s: &EscalationStep| {
            OffsetDateTime::now_utc() + time::Duration::seconds(s.wait_seconds)
        })
}

// ── policies ──────────────────────────────────────────────────────────────

const POLICY_SELECT: &str =
    "SELECT p.id, p.name, p.steps, p.created_at, COUNT(m.id) AS monitor_count
     FROM escalation_policies p LEFT JOIN monitors m ON m.escalation_policy_id = p.id";

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>> {
    let sql = format!("{POLICY_SELECT} WHERE p.org_id = ? GROUP BY p.id ORDER BY p.created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(policy_from).collect())
}

pub async fn get(
    pool: &SqlitePool,
    id: EscalationPolicyId,
    org_id: OrgId,
) -> DbResult<EscalationPolicy> {
    let sql = format!("{POLICY_SELECT} WHERE p.id = ? AND p.org_id = ? GROUP BY p.id");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(policy_from(&row))
}

pub async fn get_unscoped(pool: &SqlitePool, id: EscalationPolicyId) -> DbResult<EscalationPolicy> {
    let sql = format!("{POLICY_SELECT} WHERE p.id = ? GROUP BY p.id");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(policy_from(&row))
}

pub async fn create(
    pool: &SqlitePool,
    input: NewEscalationPolicy,
    org_id: OrgId,
) -> DbResult<EscalationPolicy> {
    let id = EscalationPolicyId::new();
    let steps = serde_json::to_string(&input.steps).unwrap_or_else(|_| "[]".into());
    sqlx::query("INSERT INTO escalation_policies (id, name, steps, org_id) VALUES (?, ?, ?, ?)")
        .bind(id.0.to_string())
        .bind(input.name)
        .bind(steps)
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: EscalationPolicyId,
    patch: UpdateEscalationPolicy,
    org_id: OrgId,
) -> DbResult<EscalationPolicy> {
    let steps = patch
        .steps
        .map(|s| serde_json::to_string(&s).unwrap_or_else(|_| "[]".into()));
    let res = sqlx::query(
        "UPDATE escalation_policies SET name = COALESCE(?, name), steps = COALESCE(?, steps)
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(steps)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

pub async fn delete(pool: &SqlitePool, id: EscalationPolicyId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM escalation_policies WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

// ── episodes ──────────────────────────────────────────────────────────────

pub async fn open_episode(
    pool: &SqlitePool,
    monitor_id: MonitorId,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let next = next_due(policy, 0).map(|t| t.unix_timestamp());
    let sql = format!(
        "INSERT INTO escalation_episodes
            (id, monitor_id, subject_kind, subject_ref, policy_id, next_escalation_at)
         VALUES (?, ?, 'monitor', ?, ?, ?)
         ON CONFLICT(subject_kind, subject_ref) WHERE resolved_at IS NULL DO NOTHING
         RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::now_v7().to_string())
        .bind(monitor_id.0.to_string())
        .bind(monitor_id.0.to_string())
        .bind(policy.id.0.to_string())
        .bind(next)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

pub async fn open_episode_for_subject(
    pool: &SqlitePool,
    kind: &str,
    subject_ref: &str,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let next = next_due(policy, 0).map(|t| t.unix_timestamp());
    let sql = format!(
        "INSERT INTO escalation_episodes (id, subject_kind, subject_ref, policy_id, next_escalation_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(subject_kind, subject_ref) WHERE resolved_at IS NULL DO NOTHING
         RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(Uuid::now_v7().to_string())
        .bind(kind)
        .bind(subject_ref)
        .bind(policy.id.0.to_string())
        .bind(next)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

pub async fn resolve_subject(
    pool: &SqlitePool,
    kind: &str,
    subject_ref: &str,
) -> DbResult<Option<EscalationEpisode>> {
    let sql = format!(
        "UPDATE escalation_episodes SET resolved_at = unixepoch()
         WHERE subject_kind = ? AND subject_ref = ? AND resolved_at IS NULL
         RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(kind)
        .bind(subject_ref)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

pub async fn ack_episode(
    pool: &SqlitePool,
    episode_id: Uuid,
    by: UserId,
) -> DbResult<EscalationEpisode> {
    let sql = format!(
        "UPDATE escalation_episodes SET acked_at = unixepoch(), acked_by = ?
         WHERE id = ? AND resolved_at IS NULL AND acked_at IS NULL
         RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(by.0.to_string())
        .bind(episode_id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(episode_from(&row))
}

pub async fn list_open(pool: &SqlitePool) -> DbResult<Vec<EscalationEpisode>> {
    let sql = format!(
        "SELECT {EP_COLS} FROM escalation_episodes WHERE resolved_at IS NULL ORDER BY started_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(episode_from).collect())
}

pub async fn list_open_for_org(
    pool: &SqlitePool,
    org_id: OrgId,
) -> DbResult<Vec<EscalationEpisode>> {
    let sql = format!(
        "SELECT {} FROM escalation_episodes e
         JOIN escalation_policies p ON p.id = e.policy_id
         WHERE e.resolved_at IS NULL AND p.org_id = ? ORDER BY e.started_at DESC",
        // alias the columns with the `e.` qualifier for the join.
        EP_COLS
            .split(", ")
            .map(|c| format!("e.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(episode_from).collect())
}

pub async fn episode_in_org(pool: &SqlitePool, episode: Uuid, org_id: OrgId) -> DbResult<()> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM escalation_episodes e
            JOIN escalation_policies p ON p.id = e.policy_id
            WHERE e.id = ? AND p.org_id = ?)",
    )
    .bind(episode.to_string())
    .bind(org_id.0.to_string())
    .fetch_one(pool)
    .await?;
    if exists != 0 {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn open_for_monitor(
    pool: &SqlitePool,
    monitor_id: MonitorId,
) -> DbResult<Option<EscalationEpisode>> {
    let sql = format!(
        "SELECT {EP_COLS} FROM escalation_episodes WHERE monitor_id = ? AND resolved_at IS NULL"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(monitor_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

pub async fn ack(
    pool: &SqlitePool,
    monitor_id: MonitorId,
    by: UserId,
) -> DbResult<EscalationEpisode> {
    let sql = format!(
        "UPDATE escalation_episodes SET acked_at = unixepoch(), acked_by = ?
         WHERE monitor_id = ? AND resolved_at IS NULL AND acked_at IS NULL
         RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(by.0.to_string())
        .bind(monitor_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(episode_from(&row))
}

pub async fn resolve(
    pool: &SqlitePool,
    monitor_id: MonitorId,
) -> DbResult<Option<EscalationEpisode>> {
    let sql = format!(
        "UPDATE escalation_episodes SET resolved_at = unixepoch()
         WHERE monitor_id = ? AND resolved_at IS NULL RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(monitor_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

/// Atomically claim one due advance (re-checks due-ness so racing ticks can't
/// double-claim a step). Mirrors PG.
pub async fn advance(
    pool: &SqlitePool,
    episode_id: Uuid,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT last_step FROM escalation_episodes
         WHERE id = ? AND resolved_at IS NULL AND acked_at IS NULL
           AND next_escalation_at IS NOT NULL AND next_escalation_at <= unixepoch()",
    )
    .bind(episode_id.to_string())
    .fetch_optional(pool)
    .await?;
    let Some(cur) = current else { return Ok(None) };
    let new_step = (cur as i32) + 1;
    let next = next_due(policy, new_step).map(|t| t.unix_timestamp());
    let sql = format!(
        "UPDATE escalation_episodes SET last_step = ?, next_escalation_at = ?
         WHERE id = ? AND resolved_at IS NULL AND acked_at IS NULL AND last_step = ?
         RETURNING {EP_COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(new_step)
        .bind(next)
        .bind(episode_id.to_string())
        .bind(cur)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

pub async fn due(pool: &SqlitePool) -> DbResult<Vec<EscalationEpisode>> {
    let sql = format!(
        "SELECT {EP_COLS} FROM escalation_episodes
         WHERE resolved_at IS NULL AND acked_at IS NULL
           AND next_escalation_at IS NOT NULL AND next_escalation_at <= unixepoch()"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(episode_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::escalation::EscalationStep;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn policy_with_steps(secs: &[i64]) -> NewEscalationPolicy {
        NewEscalationPolicy {
            name: "ladder".into(),
            steps: secs
                .iter()
                .map(|s| EscalationStep {
                    wait_seconds: *s,
                    channel_ids: vec![],
                    schedule_ids: vec![],
                })
                .collect(),
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn policy_crud_and_episode_lifecycle(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        // step 0 immediate, step 1 due in -1s (already due for the advance test).
        let p = create(&pool, policy_with_steps(&[0, -1]), org)
            .await
            .unwrap();
        assert_eq!(p.steps.len(), 2);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, p.id, org).await.unwrap().name, "ladder");

        // open a subject episode (telemetry_rule kind), idempotent per subject.
        let ep = open_episode_for_subject(&pool, "telemetry_rule", "rule-1", &p)
            .await
            .unwrap()
            .expect("opened");
        assert!(
            open_episode_for_subject(&pool, "telemetry_rule", "rule-1", &p)
                .await
                .unwrap()
                .is_none()
        ); // already open

        assert_eq!(list_open(&pool).await.unwrap().len(), 1);
        assert_eq!(list_open_for_org(&pool, org).await.unwrap().len(), 1);
        episode_in_org(&pool, ep.id, org).await.unwrap();
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            episode_in_org(&pool, ep.id, other.id).await,
            Err(DbError::NotFound)
        ));

        // due → advance bumps last_step 0→1 (step 1 was due at -1s).
        assert_eq!(due(&pool).await.unwrap().len(), 1);
        let advanced = advance(&pool, ep.id, &p).await.unwrap().expect("advanced");
        assert_eq!(advanced.last_step, 1);
        // step 1 is the last → next_escalation_at now NULL → no longer due.
        assert!(due(&pool).await.unwrap().is_empty());

        // resolve_subject closes it.
        assert!(resolve_subject(&pool, "telemetry_rule", "rule-1")
            .await
            .unwrap()
            .is_some());
        assert!(list_open(&pool).await.unwrap().is_empty());

        delete(&pool, p.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, p.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
