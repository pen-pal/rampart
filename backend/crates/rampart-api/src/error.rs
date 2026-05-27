//! API error → HTTP response mapping.
//!
//! One enum, one IntoResponse impl. Every handler returns
//! `Result<Json<T>, ApiError>` and gets consistent error rendering for free.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rampart_db::DbError;
use serde_json::json;
use thiserror::Error;
use tracing::error;

// NotFound, Conflict, Unauthorized are constructed via the routes
// modules but clippy occasionally misses cross-module use in incremental
// builds. They're API surface, not dead code.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error(transparent)]
    Db(#[from] DbError),

    #[error(transparent)]
    Validation(#[from] validator::ValidationErrors),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", self.to_string()),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request", self.to_string()),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict", self.to_string()),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", self.to_string()),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden", self.to_string()),
            ApiError::Validation(v) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation",
                v.to_string(),
            ),
            ApiError::Db(DbError::NotFound) => {
                (StatusCode::NOT_FOUND, "not_found", "not found".into())
            }
            ApiError::Db(DbError::Conflict(m)) => (StatusCode::CONFLICT, "conflict", m.clone()),
            ApiError::Db(e) => {
                // Don't leak database details to clients. Log internally,
                // surface a generic 500.
                error!(error = %e, "db error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal error".into(),
                )
            }
        };
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}
