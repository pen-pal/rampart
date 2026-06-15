//! Embedded frontend bundle.
//!
//! `rust-embed` reads from disk in debug builds and bakes bytes into the
//! binary in release builds, so a single `rampart-api` executable ships both
//! API and UI. The folder path is relative to this crate's Cargo.toml.
//!
//! Any request that doesn't match an API route falls through to `handler`,
//! which serves the matching file or, for unknown paths, returns
//! `index.html` so the React router can resolve the route client-side.

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;
use tracing::{info, warn};

#[derive(Embed)]
#[folder = "../../../frontend/dist/"]
struct Assets;

pub fn log_state() {
    match Assets::get("index.html") {
        Some(_) => info!("frontend bundle available (embedded or read from disk)"),
        None => warn!(
            "frontend bundle missing — run `npm run build` in frontend/ so rust-embed has something to serve"
        ),
    }
}

pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let lookup = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = Assets::get(lookup) {
        return file_response(lookup, file);
    }

    // SPA fallback: any unknown path returns index.html so the React router
    // can render the deep link client-side. Bare 404 only if the bundle
    // itself is missing.
    match Assets::get("index.html") {
        Some(file) => file_response("index.html", file),
        None => (StatusCode::NOT_FOUND, "frontend bundle not built").into_response(),
    }
}

fn file_response(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = file.metadata.mimetype();
    // Self-RUM: when RAMPART_SELF_RUM is set, inject the RUM snippet into the
    // dashboard shell so the running app reports its own Core Web Vitals (and
    // browser JS errors) — real data, same dogfood idea as the OTLP self-export.
    // Same-origin script + beacon, so the strict CSP (`script-src 'self'`,
    // `connect-src 'self'`) already allows it.
    let body = if path == "index.html" && self_rum_enabled() {
        Body::from(inject_rum_snippet(&file.data))
    } else {
        Body::from(file.data)
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(
            header::CACHE_CONTROL,
            if path == "index.html" {
                // Shell HTML is mutable across deploys; never cache it.
                "no-cache"
            } else {
                // Hashed asset filenames from Vite are immutable; cache hard.
                "public, max-age=31536000, immutable"
            },
        )
        .body(body)
        .unwrap()
}

fn self_rum_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("RAMPART_SELF_RUM").as_deref(),
            Ok("1") | Ok("true")
        )
    })
}

fn inject_rum_snippet(html: &[u8]) -> Vec<u8> {
    const TAG: &str =
        "<script src=\"/rum/snippet.js\" data-app=\"rampart-dashboard\"></script></head>";
    match std::str::from_utf8(html) {
        Ok(s) if s.contains("</head>") => s.replacen("</head>", TAG, 1).into_bytes(),
        _ => html.to_vec(),
    }
}
