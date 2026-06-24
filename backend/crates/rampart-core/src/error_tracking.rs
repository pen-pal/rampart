//! Error & exception tracking — Sentry-compatible event model + grouping.
//!
//! The ingest endpoint accepts Sentry-format event JSON (so existing Sentry
//! SDKs work by pointing their DSN at Rampart). [`ParsedEvent::from_sentry_json`]
//! pulls out the fields we persist and group on; [`fingerprint`] decides which
//! issue an event belongs to. Both are pure and unit-tested — no DB, no
//! network. See `docs/design/ERROR-TRACKING.md`.

use crate::ids::{ErrorIssueId, ErrorProjectId, NotificationId, UserId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use validator::Validate;

/// Cap on a fingerprint's input length / title length, to keep rows tidy.
const TITLE_MAX: usize = 240;

// ─────────────────────────── domain types ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorProject {
    pub id: ErrorProjectId,
    pub name: String,
    pub slug: String,
    /// DSN public key — embedded in client SDKs, NOT a secret.
    pub public_key: String,
    pub platform: Option<String>,
    pub retention_days: i32,
    /// Notification channels paged on a new / regressed issue.
    pub alert_channel_ids: Vec<NotificationId>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewErrorProject {
    #[validate(length(min = 1, max = 120))]
    pub name: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub alert_channel_ids: Vec<NotificationId>,
}

/// Max length of an error-project name (mirrors the `NewErrorProject` validator).
/// The public RUM browser-error beacon auto-provisions a project named after its
/// attacker-controllable `app` field via `find_or_create_by_name`, which bypasses
/// the DTO validator — so the db layer clamps to this.
pub const PROJECT_NAME_MAX: usize = 120;

/// Cap on auto-provisioned error projects per org from the public RUM beacon — a
/// DoS guard so a beacon spraying distinct `app` names can't amplify into
/// unbounded rows + slug/key generation. Generous for legit multi-app sites;
/// beyond it the beacon's errors drop rather than create rows.
pub const MAX_AUTO_PROJECTS_PER_ORG: i64 = 100;

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct UpdateErrorProject {
    #[validate(length(min = 1, max = 120))]
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub retention_days: Option<i32>,
    #[serde(default)]
    pub alert_channel_ids: Option<Vec<NotificationId>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorIssue {
    pub id: ErrorIssueId,
    pub project_id: ErrorProjectId,
    pub fingerprint: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: String,
    /// `unresolved` | `resolved` | `ignored`.
    pub status: String,
    pub first_seen: OffsetDateTime,
    pub last_seen: OffsetDateTime,
    pub times_seen: i64,
    pub assignee: Option<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub id: uuid::Uuid,
    pub issue_id: ErrorIssueId,
    pub project_id: ErrorProjectId,
    pub ts: OffsetDateTime,
    pub level: String,
    pub message: Option<String>,
    pub exception_type: Option<String>,
    pub culprit: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub server_name: Option<String>,
    pub stacktrace: Option<serde_json::Value>,
    pub context: Option<serde_json::Value>,
}

// ─────────────────────────── parsed ingest event ───────────────────────────

/// One normalized stack frame (the subset we group + render on).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Frame {
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub lineno: Option<i64>,
    #[serde(default)]
    pub colno: Option<i64>,
    #[serde(default)]
    pub in_app: Option<bool>,
}

/// A Sentry event reduced to what we store and group on. Construct via
/// [`ParsedEvent::from_sentry_json`]; the original payload is kept in `raw`.
#[derive(Debug, Clone)]
pub struct ParsedEvent {
    pub event_id: Option<String>,
    pub level: String,
    pub platform: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub server_name: Option<String>,
    /// Sentry `transaction` — the route/handler the error happened in.
    pub transaction: Option<String>,
    pub message: Option<String>,
    pub exception_type: Option<String>,
    pub exception_value: Option<String>,
    /// Frames of the most recent exception, in Sentry order (oldest first).
    pub frames: Vec<Frame>,
    /// SDK-supplied `fingerprint` override, if any.
    pub fingerprint_override: Option<Vec<String>>,
    pub raw: serde_json::Value,
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

impl ParsedEvent {
    /// Tolerant extraction from Sentry event JSON. Never fails — missing or
    /// oddly-shaped fields just come back as `None`/defaults so a slightly
    /// off SDK payload still produces a usable issue.
    pub fn from_sentry_json(raw: serde_json::Value) -> ParsedEvent {
        let level = str_field(&raw, "level").unwrap_or_else(|| "error".to_string());

        // message: top-level string, or logentry.message / .formatted.
        let message = match raw.get("message") {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            _ => raw
                .get("logentry")
                .and_then(|l| l.get("formatted").or_else(|| l.get("message")))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string()),
        };

        // exception: exception.values[] — take the last (most recent) value.
        let exc = raw
            .get("exception")
            .and_then(|e| e.get("values"))
            .and_then(|vals| vals.as_array())
            .and_then(|arr| arr.last());
        let exception_type = exc.and_then(|e| str_field(e, "type"));
        let exception_value = exc.and_then(|e| str_field(e, "value"));
        let frames = exc
            .and_then(|e| e.get("stacktrace"))
            .and_then(|st| st.get("frames"))
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|f| Frame {
                        module: str_field(f, "module"),
                        function: str_field(f, "function"),
                        filename: str_field(f, "filename"),
                        lineno: f.get("lineno").and_then(|l| l.as_i64()),
                        colno: f
                            .get("colno")
                            .or_else(|| f.get("column"))
                            .and_then(|l| l.as_i64()),
                        in_app: f.get("in_app").and_then(|b| b.as_bool()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let fingerprint_override = raw
            .get("fingerprint")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        ParsedEvent {
            event_id: str_field(&raw, "event_id"),
            level,
            platform: str_field(&raw, "platform"),
            environment: str_field(&raw, "environment"),
            release: str_field(&raw, "release"),
            server_name: str_field(&raw, "server_name"),
            transaction: str_field(&raw, "transaction"),
            message,
            exception_type,
            exception_value,
            frames,
            fingerprint_override,
            raw,
        }
    }

    /// Issue title: "Type: value", else the message, else a generic label.
    pub fn title(&self) -> String {
        let t = match (&self.exception_type, &self.exception_value) {
            (Some(ty), Some(val)) if !val.trim().is_empty() => format!("{ty}: {val}"),
            (Some(ty), _) => ty.clone(),
            _ => self.message.clone().unwrap_or_else(|| "Error".to_string()),
        };
        truncate(&t, TITLE_MAX)
    }

    /// Where it happened: the Sentry transaction, else the top in-app frame.
    pub fn culprit(&self) -> Option<String> {
        if let Some(tx) = &self.transaction {
            if !tx.trim().is_empty() {
                return Some(tx.clone());
            }
        }
        // Frames are oldest-first, so the top of the stack is the last in-app one.
        self.frames
            .iter()
            .rev()
            .find(|f| f.in_app != Some(false) && (f.function.is_some() || f.module.is_some()))
            .map(|f| match (&f.function, &f.module) {
                (Some(fun), Some(m)) => format!("{fun} ({m})"),
                (Some(fun), None) => fun.clone(),
                (None, Some(m)) => m.clone(),
                _ => String::new(),
            })
    }
}

/// Group key for an event. Same bug → same hash → same issue.
///
/// Priority: an explicit SDK `fingerprint`, else the exception type plus its
/// in-app `(module, function)` frames (line numbers and paths deliberately
/// dropped so a shifted line or a different deploy path doesn't fork the
/// issue), else the normalized message.
pub fn fingerprint(ev: &ParsedEvent) -> String {
    if let Some(fp) = &ev.fingerprint_override {
        return hash(&fp.join("\u{1f}"));
    }
    if ev.exception_type.is_some() || !ev.frames.is_empty() {
        let mut key = String::new();
        key.push_str(ev.exception_type.as_deref().unwrap_or(""));
        for f in ev.frames.iter().filter(|f| f.in_app != Some(false)) {
            key.push('\n');
            key.push_str(f.module.as_deref().unwrap_or(""));
            key.push(':');
            key.push_str(f.function.as_deref().unwrap_or(""));
        }
        return hash(&key);
    }
    hash(&format!(
        "msg:{}",
        normalize_message(ev.message.as_deref().unwrap_or(""))
    ))
}

/// Collapse variable bits (digit runs) so messages like "user 4821 not found"
/// and "user 9 not found" group together. Lowercased; each maximal run of
/// ASCII digits becomes a single `0`.
fn normalize_message(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut prev_digit = false;
    for c in msg.trim().to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !prev_digit {
                out.push('0');
            }
            prev_digit = true;
        } else {
            out.push(c);
            prev_digit = false;
        }
    }
    out
}

fn hash(input: &str) -> String {
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    hex::encode(h.finalize())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(raw: serde_json::Value) -> ParsedEvent {
        ParsedEvent::from_sentry_json(raw)
    }

    #[test]
    fn parses_exception_event() {
        let p = ev(json!({
            "event_id": "abc",
            "level": "error",
            "platform": "python",
            "transaction": "GET /users",
            "exception": { "values": [{
                "type": "ValueError",
                "value": "bad id",
                "stacktrace": { "frames": [
                    { "module": "app.views", "function": "list_users", "lineno": 42, "in_app": true },
                    { "module": "lib.db", "function": "query", "lineno": 9, "in_app": false }
                ] }
            }] }
        }));
        assert_eq!(p.exception_type.as_deref(), Some("ValueError"));
        assert_eq!(p.exception_value.as_deref(), Some("bad id"));
        assert_eq!(p.frames.len(), 2);
        assert_eq!(p.title(), "ValueError: bad id");
        assert_eq!(p.culprit().as_deref(), Some("GET /users"));
    }

    #[test]
    fn culprit_falls_back_to_top_in_app_frame() {
        let p = ev(json!({
            "exception": { "values": [{
                "type": "E",
                "stacktrace": { "frames": [
                    { "module": "app.a", "function": "f", "in_app": true },
                    { "module": "app.b", "function": "g", "in_app": true },
                    { "module": "vendor", "function": "h", "in_app": false }
                ] }
            }] }
        }));
        // Top of stack = last frame that's in-app → g (app.b).
        assert_eq!(p.culprit().as_deref(), Some("g (app.b)"));
    }

    #[test]
    fn message_only_event() {
        let p = ev(json!({ "message": "disk almost full" }));
        assert!(p.exception_type.is_none());
        assert_eq!(p.title(), "disk almost full");
        assert!(p.culprit().is_none());
    }

    #[test]
    fn logentry_message_form() {
        let p = ev(json!({ "logentry": { "formatted": "took 5s" } }));
        assert_eq!(p.message.as_deref(), Some("took 5s"));
    }

    #[test]
    fn fingerprint_groups_same_exception_ignoring_line_numbers() {
        let mk = |lineno: i64| {
            ev(json!({
                "exception": { "values": [{
                    "type": "ValueError",
                    "stacktrace": { "frames": [
                        { "module": "app.views", "function": "list_users", "lineno": lineno, "in_app": true }
                    ] }
                }] }
            }))
        };
        // Same type + frame (module/function), different line → same group.
        assert_eq!(fingerprint(&mk(42)), fingerprint(&mk(99)));
    }

    #[test]
    fn fingerprint_separates_different_exceptions() {
        let a = ev(json!({ "exception": { "values": [{ "type": "ValueError" }] } }));
        let b = ev(json!({ "exception": { "values": [{ "type": "KeyError" }] } }));
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_respects_explicit_override() {
        let a = ev(
            json!({ "fingerprint": ["my-group"], "exception": { "values": [{ "type": "ValueError" }] } }),
        );
        let b = ev(
            json!({ "fingerprint": ["my-group"], "exception": { "values": [{ "type": "KeyError" }] } }),
        );
        // Different exceptions, same explicit fingerprint → same group.
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_groups_messages_ignoring_numbers() {
        let a = ev(json!({ "message": "user 4821 not found" }));
        let b = ev(json!({ "message": "user 9 not found" }));
        assert_eq!(fingerprint(&a), fingerprint(&b));
        let c = ev(json!({ "message": "user not authorized" }));
        assert_ne!(fingerprint(&a), fingerprint(&c));
    }

    #[test]
    fn title_truncates_long_values() {
        let long = "x".repeat(500);
        let p = ev(json!({ "message": long }));
        assert!(p.title().chars().count() <= TITLE_MAX);
        assert!(p.title().ends_with('…'));
    }
}
