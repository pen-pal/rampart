//! Audit-write convenience.
//!
//! Each mutating handler calls `record(...)` with the actor and a
//! resource hint. Failures are logged but never block the request —
//! the audit log is best-effort observability, not a transactional
//! guarantee.

use axum::http::HeaderMap;
use rampart_db::audit::NewEntry;
use rampart_db::users::User;
use rampart_db::DbPool;
use sqlx::types::ipnetwork::IpNetwork;
use std::str::FromStr;
use uuid::Uuid;

pub async fn record(
    pool: &DbPool,
    user: &User,
    headers: &HeaderMap,
    action: &str,
    resource_kind: &str,
    resource_id: Option<Uuid>,
    payload: Option<serde_json::Value>,
) {
    let ip = client_ip(headers);
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok());
    let entry = NewEntry {
        actor_user_id:    Some(user.id),
        actor_api_key_id: None,
        action,
        resource_kind,
        resource_id,
        payload,
        ip_addr:          ip,
        user_agent:       ua,
    };
    if let Err(e) = rampart_db::audit::insert(pool, entry).await {
        tracing::warn!(error = %e, action, "audit insert failed");
    }
}

/// Honor X-Forwarded-For if present (one trusted proxy hop). Otherwise
/// give up — axum doesn't expose ConnectInfo at this layer cheaply.
fn client_ip(headers: &HeaderMap) -> Option<IpNetwork> {
    let raw = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))?
        .to_str()
        .ok()?
        .split(',')
        .next()?
        .trim();
    IpNetwork::from_str(raw).ok()
}
