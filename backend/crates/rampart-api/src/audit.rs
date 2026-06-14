//! Audit-write convenience.
//!
//! Each mutating handler calls `record(...)` with the actor and a
//! resource hint. Failures are logged but never block the request —
//! the audit log is best-effort observability, not a transactional
//! guarantee.

use axum::http::HeaderMap;
use rampart_core::UserId;
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
    emit(
        pool,
        Some(user.id),
        headers,
        action,
        resource_kind,
        resource_id,
        payload,
    )
    .await;
}

/// Record a security event with no authenticated actor — e.g. a failed
/// login, where we have no trusted user identity to attribute. Same
/// best-effort, secret-redacting, chained-append semantics as `record`;
/// the actor column is left NULL and the source IP / UA carry the
/// forensic value. Bounded by the login rate limiter so a credential
/// stuffer can't flood the chain.
pub async fn record_anon(
    pool: &DbPool,
    headers: &HeaderMap,
    action: &str,
    resource_kind: &str,
    payload: Option<serde_json::Value>,
) {
    emit(pool, None, headers, action, resource_kind, None, payload).await;
}

#[allow(clippy::too_many_arguments)]
async fn emit(
    pool: &DbPool,
    actor_user_id: Option<UserId>,
    headers: &HeaderMap,
    action: &str,
    resource_kind: &str,
    resource_id: Option<Uuid>,
    payload: Option<serde_json::Value>,
) {
    let ip = client_ip(headers);
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok());
    // Defence-in-depth: the audit log is admin-readable + CSV-exportable.
    // Even if a route handler accidentally hands us a payload containing
    // a password / bearer / TOTP secret / API key, we redact in-place
    // before persistence so the secret never reaches an audit-export
    // CSV, a SIEM, or a curious admin's `select * from audit_log`.
    let payload = payload.map(|mut v| {
        redact_secret_keys(&mut v);
        v
    });
    let entry = NewEntry {
        actor_user_id,
        actor_api_key_id: None,
        action,
        resource_kind,
        resource_id,
        payload,
        ip_addr: ip,
        user_agent: ua,
    };
    if let Err(e) = rampart_db::audit::insert(pool, entry).await {
        tracing::warn!(error = %e, action, "audit insert failed");
    }
}

/// Field names that hold secrets we never want in the audit log.
/// Case-insensitive prefix-match against JSON object keys. Matched
/// fields are replaced with the literal string "[redacted]".
const SECRET_FIELD_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "private_key",
    "client_secret",
    "auth",
    "credential",
    "totp",
    "recovery",
    "vapid",
    "smtp_password",
    "bind_password",
];

/// In-place recursive walk: any object key whose lowercase form
/// CONTAINS any pattern in `SECRET_FIELD_PATTERNS` has its value
/// replaced with `"[redacted]"`. Arrays are recursed into; scalars
/// are passed through. The shape of the payload is preserved so the
/// audit-log UI still renders sensibly.
fn redact_secret_keys(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let lower = k.to_lowercase();
                let is_secret = SECRET_FIELD_PATTERNS.iter().any(|p| lower.contains(p));
                if is_secret {
                    *v = Value::String("[redacted]".into());
                } else {
                    redact_secret_keys(v);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                redact_secret_keys(v);
            }
        }
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::redact_secret_keys;
    use serde_json::json;

    #[test]
    fn redacts_top_level_password_field() {
        let mut v = json!({"email": "alice@example.com", "password": "hunter2"});
        redact_secret_keys(&mut v);
        assert_eq!(v["email"], "alice@example.com");
        assert_eq!(v["password"], "[redacted]");
    }

    #[test]
    fn redacts_nested_token_field() {
        let mut v = json!({"smtp": {"host": "smtp.x", "smtp_password": "p"}});
        redact_secret_keys(&mut v);
        assert_eq!(v["smtp"]["host"], "smtp.x");
        assert_eq!(v["smtp"]["smtp_password"], "[redacted]");
    }

    #[test]
    fn redacts_inside_array_of_objects() {
        let mut v = json!({"channels": [
            {"name": "ops", "api_key": "abc"},
            {"name": "alerts", "webhook_url": "https://x"}
        ]});
        redact_secret_keys(&mut v);
        assert_eq!(v["channels"][0]["api_key"], "[redacted]");
        // webhook_url is not in the secret-prefix list (operators do
        // legitimately need to see the URL they configured) — passes
        // through.
        assert_eq!(v["channels"][1]["webhook_url"], "https://x");
    }

    #[test]
    fn matching_is_case_insensitive_and_substring() {
        let mut v = json!({"BIND_PASSWORD": "x", "RecoveryCode": "y", "client_secret_jwt": "z"});
        redact_secret_keys(&mut v);
        assert_eq!(v["BIND_PASSWORD"], "[redacted]");
        assert_eq!(v["RecoveryCode"], "[redacted]");
        assert_eq!(v["client_secret_jwt"], "[redacted]");
    }

    #[test]
    fn leaves_innocuous_keys_alone() {
        let mut v = json!({
            "name": "Acme",
            "url": "https://example.com",
            "interval_seconds": 60,
            "headers": [{"k": "X-Trace", "v": "1"}]
        });
        let before = v.clone();
        redact_secret_keys(&mut v);
        assert_eq!(v, before);
    }
}
