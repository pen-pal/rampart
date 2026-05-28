//! Tags — colored labels that can be attached to monitors and used as
//! filters on the dashboard. Each tag has a unique name; colors are
//! 7-char hex strings (`#rrggbb`).

use crate::ids::TagId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    pub color: String,
    pub created_at: OffsetDateTime,
}

/// Compact projection used when a tag is rendered as a chip next to
/// a monitor row. Skips created_at to keep the payload tight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagBrief {
    pub id: TagId,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct NewTag {
    #[validate(length(min = 1, max = 40))]
    pub name: String,

    /// `#rrggbb`. Defaulted to the brand accent when omitted.
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_color() -> String {
    "#14b8a6".into()
}
