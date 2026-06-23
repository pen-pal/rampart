//! MySQL `agents` domain — remote probe workers (the final scheduler/notifier-
//! tail slice). Mirrors the PG/SQLite surface: list / get / create / update /
//! delete / lookup (token resolver) / touch_seen.
//!
//! Tokens are `rmpa_<40 url-safe chars>`; only the SHA-256 hash is stored
//! (unique). `online` is derived from `last_seen_at` vs [`ONLINE_GRACE_SECONDS`]
//! at read time, like PG. Dialect: uuids CHAR(36), ts BIGINT unix-seconds,
//! `monitor_count` via `LEFT JOIN monitors … GROUP BY` (MySQL 8 functional-
//! dependency allows GROUP BY the PK). No `RETURNING` → INSERT-then-re-select.
//! `update` drops the `rows_affected()` gate (MySQL changed-vs-matched → trailing
//! `get()`). monitors.agent_id has no enforced FK, so `delete` clears it in-tx.

use super::{oid, ts};
use crate::{DbError, DbResult};
use rampart_core::agent::{Agent, IssuedAgent, NewAgent, UpdateAgent, ONLINE_GRACE_SECONDS};
use rampart_core::ids::{AgentId, OrgId};
use sha2::{Digest, Sha256};
use sqlx::{MySqlPool, Row};

const TOKEN_PREFIX: &str = "rmpa_";
const TOKEN_BODY_LEN: usize = 40;
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn agent_from(r: &sqlx::mysql::MySqlRow) -> Agent {
    let last_seen_at = r.get::<Option<i64>, _>("last_seen_at").map(ts);
    let online = last_seen_at
        .map(|t| (time::OffsetDateTime::now_utc() - t).whole_seconds() < ONLINE_GRACE_SECONDS)
        .unwrap_or(false);
    Agent {
        id: AgentId::from_uuid(super::raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        location: r.get("location"),
        version: r.get("version"),
        last_seen_at,
        created_at: ts(r.get::<i64, _>("created_at")),
        org_id: oid(&r.get::<String, _>("org_id")),
        monitor_count: r.get::<Option<i64>, _>("monitor_count").unwrap_or(0),
        online,
    }
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<Agent>> {
    let rows = sqlx::query(
        "SELECT a.id, a.name, a.location, a.version, a.last_seen_at,
                a.created_at, a.org_id, COUNT(m.id) AS monitor_count
         FROM agents a LEFT JOIN monitors m ON m.agent_id = a.id
         WHERE a.org_id = ?
         GROUP BY a.id
         ORDER BY a.created_at",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(agent_from).collect())
}

pub async fn get(pool: &MySqlPool, id: AgentId, org_id: OrgId) -> DbResult<Agent> {
    let row = sqlx::query(
        "SELECT a.id, a.name, a.location, a.version, a.last_seen_at,
                a.created_at, a.org_id, COUNT(m.id) AS monitor_count
         FROM agents a LEFT JOIN monitors m ON m.agent_id = a.id
         WHERE a.id = ? AND a.org_id = ?
         GROUP BY a.id",
    )
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(agent_from(&row))
}

pub async fn create(pool: &MySqlPool, input: NewAgent, org_id: OrgId) -> DbResult<IssuedAgent> {
    let token = generate_token();
    let hash = sha256_hex(&token);
    let id = AgentId::new();
    sqlx::query(
        "INSERT INTO agents (id, name, location, token_hash, org_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.location)
    .bind(hash)
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // No RETURNING → re-select (monitor_count = 0 for a fresh agent).
    let agent = get(pool, id, org_id).await?;
    Ok(IssuedAgent { agent, token })
}

pub async fn update(
    pool: &MySqlPool,
    id: AgentId,
    patch: UpdateAgent,
    org_id: OrgId,
) -> DbResult<Agent> {
    // Empty-string location clears it; None leaves it untouched (mirrors PG).
    let clear_location = patch.location.as_deref() == Some("");
    sqlx::query(
        "UPDATE agents SET
            name     = COALESCE(?, name),
            location = CASE WHEN ? THEN NULL ELSE COALESCE(?, location) END
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(clear_location as i64)
    .bind(patch.location)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // NotFound surfaces here (no rows_affected gate — MySQL changed-vs-matched).
    get(pool, id, org_id).await
}

/// Delete an agent and return its monitors to local probing. monitors.agent_id
/// has no enforced FK on MySQL (documentary inline ref), so clear it in one tx.
pub async fn delete(pool: &MySqlPool, id: AgentId, org_id: OrgId) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    let res = sqlx::query("DELETE FROM agents WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    sqlx::query("UPDATE monitors SET agent_id = NULL WHERE agent_id = ?")
        .bind(id.0.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Resolve a raw bearer token to its agent (unscoped — this IS the agent auth
/// path). Unknown / wrong-prefix → NotFound.
pub async fn lookup(pool: &MySqlPool, token: &str) -> DbResult<Agent> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Err(DbError::NotFound);
    }
    let hash = sha256_hex(token);
    let row = sqlx::query(
        "SELECT id, name, location, version, last_seen_at, created_at, org_id,
                0 AS monitor_count
         FROM agents WHERE token_hash = ?",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(agent_from(&row))
}

/// Bump `last_seen_at` (and the self-reported version when sent) on every agent
/// poll/report. Fire-and-forget on the hot path.
pub async fn touch_seen(pool: &MySqlPool, id: AgentId, version: Option<&str>) -> DbResult<()> {
    sqlx::query(
        "UPDATE agents SET last_seen_at = UNIX_TIMESTAMP(), version = COALESCE(?, version) WHERE id = ?",
    )
    .bind(version)
    .bind(id.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let body: String = (0..TOKEN_BODY_LEN)
        .map(|_| ALPHA[rng.gen_range(0..ALPHA.len())] as char)
        .collect();
    format!("{TOKEN_PREFIX}{body}")
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::monitors;
    use rampart_core::monitor::{MonitorKind, NewMonitor, UpdateMonitor};

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_agent(name: &str) -> NewAgent {
        NewAgent {
            name: name.into(),
            location: Some("eu-west".into()),
        }
    }

    async fn a_monitor(pool: &MySqlPool) -> rampart_core::ids::MonitorId {
        let org = oid(DEF);
        monitors::create(
            pool,
            NewMonitor {
                name: "m".into(),
                kind: MonitorKind::Http,
                url: Some("https://x".into()),
                hostname: None,
                port: None,
                config: serde_json::json!({}),
                interval_seconds: 60,
                timeout_seconds: 10,
                max_retries: 0,
                retry_interval_sec: 60,
                resend_interval_sec: 0,
                upside_down: false,
                http_method: "GET".into(),
                http_body: None,
                http_headers: None,
                accepted_statuses: vec![200],
                follow_redirect: true,
                ignore_tls: false,
                proxy_id: None,
                group_id: None,
                check_cert: false,
                cert_expiry_days: 14,
                slo_target_pct: None,
                slo_window_days: None,
                agent_id: None,
                escalation_policy_id: None,
            },
            org,
        )
        .await
        .unwrap()
        .id
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn create_lookup_touch_and_count(pool: MySqlPool) {
        let org = oid(DEF);
        let issued = create(&pool, new_agent("probe-1"), org).await.unwrap();
        assert!(issued.token.starts_with("rmpa_"));
        let id = issued.agent.id;

        assert_eq!(lookup(&pool, &issued.token).await.unwrap().id, id);
        assert!(matches!(
            lookup(&pool, "nope").await,
            Err(DbError::NotFound)
        ));

        // Freshly created → offline; touch_seen flips it + records version.
        assert!(!get(&pool, id, org).await.unwrap().online);
        touch_seen(&pool, id, Some("1.2.3")).await.unwrap();
        let a = get(&pool, id, org).await.unwrap();
        assert!(a.online);
        assert_eq!(a.version.as_deref(), Some("1.2.3"));

        // Assign a monitor → monitor_count reflects it.
        let m = a_monitor(&pool).await;
        let patch: UpdateMonitor =
            serde_json::from_value(serde_json::json!({ "agent_id": id.0.to_string() })).unwrap();
        monitors::update(&pool, m, patch, org).await.unwrap();
        assert_eq!(get(&pool, id, org).await.unwrap().monitor_count, 1);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn update_clear_location_and_delete_unassigns(pool: MySqlPool) {
        let org = oid(DEF);
        let id = create(&pool, new_agent("probe-2"), org)
            .await
            .unwrap()
            .agent
            .id;
        let m = a_monitor(&pool).await;
        let patch: UpdateMonitor =
            serde_json::from_value(serde_json::json!({ "agent_id": id.0.to_string() })).unwrap();
        monitors::update(&pool, m, patch, org).await.unwrap();

        // rename + empty-string clears location.
        let upd = update(
            &pool,
            id,
            UpdateAgent {
                name: Some("renamed".into()),
                location: Some(String::new()),
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(upd.name, "renamed");
        assert!(upd.location.is_none());

        // delete returns the monitor to local probing (agent_id → NULL).
        delete(&pool, id, org).await.unwrap();
        assert!(matches!(get(&pool, id, org).await, Err(DbError::NotFound)));
        assert!(monitors::get(&pool, m, org)
            .await
            .unwrap()
            .agent_id
            .is_none());
    }
}
