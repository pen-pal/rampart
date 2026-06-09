//! Scheduled uptime reports.
//!
//! A recurring email digest of per-monitor uptime. Each row names a set of
//! recipients and a cadence; the scheduler renders a 7-day per-monitor
//! uptime summary and emails it when the report is due (never sent, or
//! `last_sent_at` older than the cadence window). Admin-managed via
//! `/v1/scheduled-reports`.

use crate::ids::ScheduledReportId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledReport {
    pub id: ScheduledReportId,
    pub name: String,
    /// Recipient email addresses. Empty is allowed; the scheduler skips a
    /// report with no recipients.
    pub recipients: Vec<String>,
    /// Cadence keyword. Only `"weekly"` is recognised today; anything else
    /// is treated as weekly by the scheduler.
    pub cadence: String,
    /// `None` until the first successful send. The due check compares this
    /// against the cadence window.
    pub last_sent_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}
