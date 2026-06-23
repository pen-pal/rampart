//! MySQL `escalations` domain — escalation policies + the episode state machine.
//! Mirrors the PG/SQLite surface. MySQL deltas: no `RETURNING` (insert/UPDATE
//! then re-select); no partial unique index → the `open_key` STORED generated
//! column gives the atomic "one open episode per subject" guarantee (a duplicate
//! open INSERT hits the unique key → `Ok(None)`); `unixepoch()`→`UNIX_TIMESTAMP()`.
//! `next_due` is pure Rust (same as PG).

use super::{raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::escalation::{
    EscalationEpisode, EscalationPolicy, EscalationStep, NewEscalationPolicy,
    UpdateEscalationPolicy,
};
use rampart_core::ids::{EscalationPolicyId, MonitorId, OrgId, UserId};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

fn policy_from(r: &sqlx::mysql::MySqlRow) -> EscalationPolicy {
    EscalationPolicy {
        id: EscalationPolicyId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        steps: serde_json::from_str(&r.get::<String, _>("steps")).unwrap_or_default(),
        created_at: ts(r.get::<i64, _>("created_at")),
        monitor_count: r.get::<i64, _>("monitor_count"),
    }
}

fn episode_from(r: &sqlx::mysql::MySqlRow) -> EscalationEpisode {
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

const EP_COLS: &str = "id, monitor_id, subject_kind, subject_ref, policy_id, started_at, \
     last_step, next_escalation_at, acked_at, acked_by, resolved_at";

fn next_due(policy: &EscalationPolicy, fired_step: i32) -> Option<OffsetDateTime> {
    policy
        .steps
        .get(fired_step as usize + 1)
        .map(|s: &EscalationStep| {
            OffsetDateTime::now_utc() + time::Duration::seconds(s.wait_seconds)
        })
}

/// Re-fetch one episode by id (the MySQL stand-in for RETURNING).
async fn get_episode(pool: &MySqlPool, id: &str) -> DbResult<Option<EscalationEpisode>> {
    let sql = format!("SELECT {EP_COLS} FROM escalation_episodes WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(episode_from))
}

fn is_unique(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}

// ── policies ──────────────────────────────────────────────────────────────

const POLICY_SELECT: &str =
    "SELECT p.id, p.name, p.steps, p.created_at, COUNT(m.id) AS monitor_count
     FROM escalation_policies p LEFT JOIN monitors m ON m.escalation_policy_id = p.id";

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<EscalationPolicy>> {
    let sql = format!("{POLICY_SELECT} WHERE p.org_id = ? GROUP BY p.id ORDER BY p.created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(policy_from).collect())
}

pub async fn get(
    pool: &MySqlPool,
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

pub async fn get_unscoped(pool: &MySqlPool, id: EscalationPolicyId) -> DbResult<EscalationPolicy> {
    let sql = format!("{POLICY_SELECT} WHERE p.id = ? GROUP BY p.id");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(policy_from(&row))
}

pub async fn create(
    pool: &MySqlPool,
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
    pool: &MySqlPool,
    id: EscalationPolicyId,
    patch: UpdateEscalationPolicy,
    org_id: OrgId,
) -> DbResult<EscalationPolicy> {
    get(pool, id, org_id).await?; // NotFound if absent (MySQL UPDATE counts changed rows)
    let steps = patch
        .steps
        .map(|s| serde_json::to_string(&s).unwrap_or_else(|_| "[]".into()));
    sqlx::query(
        "UPDATE escalation_policies SET name = COALESCE(?, name), steps = COALESCE(?, steps)
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(steps)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: EscalationPolicyId, org_id: OrgId) -> DbResult<()> {
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
    pool: &MySqlPool,
    monitor_id: MonitorId,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let id = Uuid::now_v7().to_string();
    let next = next_due(policy, 0).map(|t| t.unix_timestamp());
    let res = sqlx::query(
        "INSERT INTO escalation_episodes
            (id, monitor_id, subject_kind, subject_ref, policy_id, next_escalation_at)
         VALUES (?, ?, 'monitor', ?, ?, ?)",
    )
    .bind(&id)
    .bind(monitor_id.0.to_string())
    .bind(monitor_id.0.to_string())
    .bind(policy.id.0.to_string())
    .bind(next)
    .execute(pool)
    .await;
    match res {
        Ok(_) => get_episode(pool, &id).await,
        Err(e) if is_unique(&e) => Ok(None), // already an open episode for this subject
        Err(e) => Err(e.into()),
    }
}

pub async fn open_episode_for_subject(
    pool: &MySqlPool,
    kind: &str,
    subject_ref: &str,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let id = Uuid::now_v7().to_string();
    let next = next_due(policy, 0).map(|t| t.unix_timestamp());
    let res = sqlx::query(
        "INSERT INTO escalation_episodes (id, subject_kind, subject_ref, policy_id, next_escalation_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(kind)
    .bind(subject_ref)
    .bind(policy.id.0.to_string())
    .bind(next)
    .execute(pool)
    .await;
    match res {
        Ok(_) => get_episode(pool, &id).await,
        Err(e) if is_unique(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn resolve_subject(
    pool: &MySqlPool,
    kind: &str,
    subject_ref: &str,
) -> DbResult<Option<EscalationEpisode>> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM escalation_episodes
         WHERE subject_kind = ? AND subject_ref = ? AND resolved_at IS NULL",
    )
    .bind(kind)
    .bind(subject_ref)
    .fetch_optional(pool)
    .await?;
    let Some(id) = id else { return Ok(None) };
    sqlx::query("UPDATE escalation_episodes SET resolved_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await?;
    get_episode(pool, &id).await
}

pub async fn ack_episode(
    pool: &MySqlPool,
    episode_id: Uuid,
    by: UserId,
) -> DbResult<EscalationEpisode> {
    let id = episode_id.to_string();
    let res = sqlx::query(
        "UPDATE escalation_episodes SET acked_at = UNIX_TIMESTAMP(), acked_by = ?
         WHERE id = ? AND resolved_at IS NULL AND acked_at IS NULL",
    )
    .bind(by.0.to_string())
    .bind(&id)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get_episode(pool, &id).await?.ok_or(DbError::NotFound)
}

pub async fn list_open(pool: &MySqlPool) -> DbResult<Vec<EscalationEpisode>> {
    let sql = format!(
        "SELECT {EP_COLS} FROM escalation_episodes WHERE resolved_at IS NULL ORDER BY started_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(episode_from).collect())
}

pub async fn list_open_for_org(
    pool: &MySqlPool,
    org_id: OrgId,
) -> DbResult<Vec<EscalationEpisode>> {
    let cols = EP_COLS
        .split(", ")
        .map(|c| format!("e.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {cols} FROM escalation_episodes e
         JOIN escalation_policies p ON p.id = e.policy_id
         WHERE e.resolved_at IS NULL AND p.org_id = ? ORDER BY e.started_at DESC"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(episode_from).collect())
}

pub async fn episode_in_org(pool: &MySqlPool, episode: Uuid, org_id: OrgId) -> DbResult<()> {
    let (exists,): (i64,) = sqlx::query_as(
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
    pool: &MySqlPool,
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
    pool: &MySqlPool,
    monitor_id: MonitorId,
    by: UserId,
) -> DbResult<EscalationEpisode> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM escalation_episodes
         WHERE monitor_id = ? AND resolved_at IS NULL AND acked_at IS NULL",
    )
    .bind(monitor_id.0.to_string())
    .fetch_optional(pool)
    .await?;
    let id = id.ok_or(DbError::NotFound)?;
    sqlx::query(
        "UPDATE escalation_episodes SET acked_at = UNIX_TIMESTAMP(), acked_by = ? WHERE id = ?",
    )
    .bind(by.0.to_string())
    .bind(&id)
    .execute(pool)
    .await?;
    get_episode(pool, &id).await?.ok_or(DbError::NotFound)
}

pub async fn resolve(
    pool: &MySqlPool,
    monitor_id: MonitorId,
) -> DbResult<Option<EscalationEpisode>> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM escalation_episodes WHERE monitor_id = ? AND resolved_at IS NULL",
    )
    .bind(monitor_id.0.to_string())
    .fetch_optional(pool)
    .await?;
    let Some(id) = id else { return Ok(None) };
    sqlx::query("UPDATE escalation_episodes SET resolved_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await?;
    get_episode(pool, &id).await
}

/// Atomically claim one due advance (re-checks due-ness so racing ticks can't
/// double-claim a step). Mirrors PG.
pub async fn advance(
    pool: &MySqlPool,
    episode_id: Uuid,
    policy: &EscalationPolicy,
) -> DbResult<Option<EscalationEpisode>> {
    let id = episode_id.to_string();
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT last_step FROM escalation_episodes
         WHERE id = ? AND resolved_at IS NULL AND acked_at IS NULL
           AND next_escalation_at IS NOT NULL AND next_escalation_at <= UNIX_TIMESTAMP()",
    )
    .bind(&id)
    .fetch_optional(pool)
    .await?;
    let Some(cur) = current else { return Ok(None) };
    let new_step = (cur as i32) + 1;
    let next = next_due(policy, new_step).map(|t| t.unix_timestamp());
    let res = sqlx::query(
        "UPDATE escalation_episodes SET last_step = ?, next_escalation_at = ?
         WHERE id = ? AND resolved_at IS NULL AND acked_at IS NULL AND last_step = ?",
    )
    .bind(new_step)
    .bind(next)
    .bind(&id)
    .bind(cur)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Ok(None); // raced — another tick claimed it
    }
    get_episode(pool, &id).await
}

pub async fn due(pool: &MySqlPool) -> DbResult<Vec<EscalationEpisode>> {
    let sql = format!(
        "SELECT {EP_COLS} FROM escalation_episodes
         WHERE resolved_at IS NULL AND acked_at IS NULL
           AND next_escalation_at IS NOT NULL AND next_escalation_at <= UNIX_TIMESTAMP()"
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

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn policy_crud_and_episode_lifecycle(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let p = create(&pool, policy_with_steps(&[0, -1]), org)
            .await
            .unwrap();
        assert_eq!(p.steps.len(), 2);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, p.id, org).await.unwrap().name, "ladder");

        let ep = open_episode_for_subject(&pool, "telemetry_rule", "rule-1", &p)
            .await
            .unwrap()
            .expect("opened");
        // a second open for the same subject is rejected by the unique open_key.
        assert!(
            open_episode_for_subject(&pool, "telemetry_rule", "rule-1", &p)
                .await
                .unwrap()
                .is_none()
        );

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
        assert!(due(&pool).await.unwrap().is_empty()); // last step → no next

        // resolve_subject closes it, freeing the open_key so a new one can open.
        assert!(resolve_subject(&pool, "telemetry_rule", "rule-1")
            .await
            .unwrap()
            .is_some());
        assert!(list_open(&pool).await.unwrap().is_empty());
        assert!(
            open_episode_for_subject(&pool, "telemetry_rule", "rule-1", &p)
                .await
                .unwrap()
                .is_some()
        );

        delete(&pool, p.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, p.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
