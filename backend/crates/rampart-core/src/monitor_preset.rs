//! Reusable monitor-config presets.
//!
//! A preset is a named, reusable bag of one slice of monitor config the
//! New-Monitor wizard can apply with a click. Two kinds today:
//!
//! - `http_headers` — a saved set of HTTP request headers, e.g. a fixed
//!   `User-Agent` + `X-Api-Version` an org stamps on every internal probe.
//! - `tls` — a saved TLS posture, e.g. `ignore_tls = true` for a fleet of
//!   self-signed internal boxes.
//!
//! `data` is intentionally loose JSONB so the wizard owns the shape and
//! new knobs don't need a migration. The DB CHECK constraint pins `kind`
//! to the known set.

use crate::ids::MonitorPresetId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// What slice of monitor config a preset carries. Mirrors the DB CHECK
/// on `monitor_presets.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorPresetKind {
    /// A saved set of HTTP request headers. `data` shape:
    /// `{ "headers": { "Name": "value", … } }`.
    HttpHeaders,
    /// A saved TLS posture. `data` shape: `{ "ignore_tls": bool }`.
    Tls,
}

impl MonitorPresetKind {
    /// The DB string form (matches the CHECK constraint values).
    pub fn as_str(self) -> &'static str {
        match self {
            MonitorPresetKind::HttpHeaders => "http_headers",
            MonitorPresetKind::Tls => "tls",
        }
    }

    /// Parse the DB string form. Returns `None` for anything outside the
    /// known set (which the CHECK constraint should already prevent).
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "http_headers" => Some(MonitorPresetKind::HttpHeaders),
            "tls" => Some(MonitorPresetKind::Tls),
            _ => None,
        }
    }
}

/// A stored preset row as returned to the API tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorPreset {
    pub id: MonitorPresetId,
    pub name: String,
    pub kind: MonitorPresetKind,
    /// Free-form config bag whose shape depends on `kind`.
    pub data: serde_json::Value,
    pub created_at: OffsetDateTime,
}

/// Create payload. `kind` + `data` shape are validated at the API tier.
#[derive(Debug, Clone, Deserialize)]
pub struct NewMonitorPreset {
    pub name: String,
    pub kind: MonitorPresetKind,
    #[serde(default)]
    pub data: serde_json::Value,
}
