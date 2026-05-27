//! Domain errors shared across crates.

use thiserror::Error;

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("validation failed: {0}")]
    Validation(String),

    #[error("not found: {entity} {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("forbidden")]
    Forbidden,

    #[error("internal: {0}")]
    Internal(String),
}

impl CoreError {
    pub fn not_found<I: std::fmt::Display>(entity: &'static str, id: I) -> Self {
        Self::NotFound {
            entity,
            id: id.to_string(),
        }
    }
}
