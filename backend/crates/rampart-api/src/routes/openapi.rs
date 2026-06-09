//! OpenAPI spec serving.
//!
//! The machine-readable description of the primary REST surface lives in
//! `docs/openapi.yaml` (OpenAPI 3.1), hand-curated rather than generated —
//! annotating ~100 handlers with `utoipa` would be far more invasive than
//! the spec is worth. The spec is embedded at compile time via
//! `include_str!` so the running binary always serves the document that
//! matches the source tree it was built from.
//!
//! Two routes, both public (the spec is not sensitive):
//!   * `GET /openapi.yaml` — the canonical hand-edited YAML, `text/yaml`.
//!   * `GET /openapi.json` — a JSON rendering of the same document. We keep
//!     a checked-in `docs/openapi.json` generated from the YAML (there is
//!     no YAML parser in the dependency tree, so converting at runtime
//!     isn't an option) and embed it the same way. A `just openapi-json`
//!     style regeneration step is documented in `docs/API.md`.
//!
//! No in-app Swagger-UI: the app CSP is `default-src 'self'` (see
//! `lib.rs`), which a CDN-hosted Swagger-UI bundle would violate, and
//! vendoring one is more surface than the deliverable warrants. To view
//! the spec rendered, point a local previewer at the served YAML, e.g.
//! `npx @redocly/cli preview-docs docs/openapi.yaml` — see `docs/API.md`.

use crate::state::AppState;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

/// The canonical spec, embedded at build time. Path is relative to this
/// source file: `routes/openapi.rs` → repo `docs/openapi.yaml`.
const OPENAPI_YAML: &str = include_str!("../../../../../docs/openapi.yaml");

/// JSON rendering of [`OPENAPI_YAML`], generated from the YAML and checked
/// in alongside it. Kept in sync by the regeneration step in `docs/API.md`.
const OPENAPI_JSON: &str = include_str!("../../../../../docs/openapi.json");

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/openapi.yaml", get(serve_yaml))
        .route("/openapi.json", get(serve_json))
}

async fn serve_yaml() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/yaml; charset=utf-8")],
        OPENAPI_YAML,
    )
}

async fn serve_json() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        OPENAPI_JSON,
    )
}
