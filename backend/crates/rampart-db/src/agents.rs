//! Remote probe agent repository.
//!
//! Tokens are generated as `rmpa_<40 url-safe random chars>` — one extra
//! prefix letter distinguishes them from personal API keys (`rmp_`) at a
//! glance in configs and logs. Only the SHA-256 hash lives in the DB;
//! lookup is a single hit on the unique `token_hash` index, mirroring
//! `api_keys`.

use crate::{DbError, DbPool, DbResult};
use rampart_core::agent::{Agent, IssuedAgent, NewAgent, UpdateAgent, ONLINE_GRACE_SECONDS};
use rampart_core::ids::{AgentId, OrgId};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

const TOKEN_PREFIX: &str = "rmpa_";
const TOKEN_BODY_LEN: usize = 40;
const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

struct AgentRow {
    id: Uuid,
    name: String,
    location: Option<String>,
    version: Option<String>,
    last_seen_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    monitor_count: Option<i64>,
}

impl From<AgentRow> for Agent {
    fn from(r: AgentRow) -> Self {
        let online = r
            .last_seen_at
            .map(|t| (OffsetDateTime::now_utc() - t).whole_seconds() < ONLINE_GRACE_SECONDS)
            .unwrap_or(false);
        Agent {
            id: AgentId::from_uuid(r.id),
            name: r.name,
            location: r.location,
            version: r.version,
            last_seen_at: r.last_seen_at,
            created_at: r.created_at,
            monitor_count: r.monitor_count.unwrap_or(0),
            online,
        }
    }
}

/// Agents owned by one org — the management list (org-scoped). The
/// agent-token resolver [`lookup`] stays unscoped (it IS the agent auth path).
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<Agent>> {
    let rows = sqlx::query_as!(
        AgentRow,
        r#"
        SELECT a.id, a.name, a.location, a.version, a.last_seen_at, a.created_at,
               COUNT(m.id) AS monitor_count
        FROM agents a
        LEFT JOIN monitors m ON m.agent_id = a.id
        WHERE a.org_id = $1
        GROUP BY a.id
        ORDER BY a.created_at
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Fetch one agent scoped to an org (cross-org id → NotFound). Request path
/// (incl. validating an agent for monitor assignment).
pub async fn get(pool: &DbPool, id: AgentId, org_id: OrgId) -> DbResult<Agent> {
    let row = sqlx::query_as!(
        AgentRow,
        r#"
        SELECT a.id, a.name, a.location, a.version, a.last_seen_at, a.created_at,
               COUNT(m.id) AS monitor_count
        FROM agents a
        LEFT JOIN monitors m ON m.agent_id = a.id
        WHERE a.id = $1 AND a.org_id = $2
        GROUP BY a.id
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(
    pool: &DbPool,
    input: NewAgent,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<IssuedAgent> {
    let token = generate_token();
    let hash = sha256_hex(&token);
    let id = AgentId::new();

    let row = sqlx::query_as!(
        AgentRow,
        r#"
        INSERT INTO agents (id, name, location, token_hash, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, location, version, last_seen_at, created_at,
                  0::bigint AS monitor_count
        "#,
        id.0,
        input.name,
        input.location,
        hash,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;

    Ok(IssuedAgent {
        agent: row.into(),
        token,
    })
}

pub async fn update(
    pool: &DbPool,
    id: AgentId,
    patch: UpdateAgent,
    org_id: OrgId,
) -> DbResult<Agent> {
    // Empty-string location clears it; None leaves it untouched.
    let clear_location = patch.location.as_deref() == Some("");
    let result = sqlx::query!(
        r#"
        UPDATE agents SET
            name     = COALESCE($2, name),
            location = CASE WHEN $4 THEN NULL ELSE COALESCE($3, location) END
        WHERE id = $1 AND org_id = $5
        "#,
        id.0,
        patch.name,
        patch.location,
        clear_location,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

/// Deleting an agent returns its monitors to local probing via the
/// `ON DELETE SET NULL` on monitors.agent_id, so the scheduler picks
/// them back up on the next reconcile.
pub async fn delete(pool: &DbPool, id: AgentId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM agents WHERE id = $1 AND org_id = $2",
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

/// Resolve a raw bearer token to the agent. NotFound for unknown hashes —
/// same single-index lookup as api_keys.
pub async fn lookup(pool: &DbPool, token: &str) -> DbResult<Agent> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Err(DbError::NotFound);
    }
    let hash = sha256_hex(token);
    let row = sqlx::query_as!(
        AgentRow,
        r#"
        SELECT id, name, location, version, last_seen_at, created_at,
               0::bigint AS monitor_count
        FROM agents
        WHERE token_hash = $1
        "#,
        hash,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Bump last_seen (and the self-reported version, when sent) on every
/// agent poll/report. Fire-and-forget on the hot path.
pub async fn touch_seen(pool: &DbPool, id: AgentId, version: Option<&str>) -> DbResult<()> {
    sqlx::query!(
        r#"
        UPDATE agents SET
            last_seen_at = NOW(),
            version      = COALESCE($2, version)
        WHERE id = $1
        "#,
        id.0,
        version,
    )
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
