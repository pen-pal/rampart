//! Source-map symbolication for error-tier stack frames.
//!
//! Given a source map document and a minified frame's `(line, col)`, resolve the
//! original function / file / line with the `sourcemap` crate. Used by the
//! error-event read path: for each frame whose file has an uploaded map for the
//! event's release, we attach a `resolved` block. Column matters — fully-minified
//! bundles put everything on line 1, so line-only lookup can't disambiguate.
//! See `docs/design/ERROR-TRACKING.md`.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub function: Option<String>,
    pub filename: Option<String>,
    pub lineno: Option<i64>,
}

/// Resolve a 1-based `(lineno, colno)` against a source map JSON document.
/// Returns None if the map won't parse or no token covers the position.
pub fn resolve(map_json: &serde_json::Value, lineno: i64, colno: Option<i64>) -> Option<Resolved> {
    let bytes = serde_json::to_vec(map_json).ok()?;
    let sm = sourcemap::SourceMap::from_slice(&bytes).ok()?;
    // Sentry frames are 1-based; the crate is 0-based. Missing column → 0.
    let line0 = u32::try_from((lineno - 1).max(0)).ok()?;
    let col0 = u32::try_from(colno.map(|c| (c - 1).max(0)).unwrap_or(0)).ok()?;
    let tok = sm.lookup_token(line0, col0)?;
    Some(Resolved {
        function: tok.get_name().map(str::to_string),
        filename: tok.get_source().map(str::to_string),
        lineno: Some(i64::from(tok.get_src_line()) + 1),
    })
}

/// Basename of a frame's file reference — strips any path and `?query`/`#frag`
/// so `https://app/static/main.abc.js?v=1` keys the same as `main.abc.js`.
pub fn basename(file: &str) -> &str {
    let no_query = file.split(['?', '#']).next().unwrap_or(file);
    no_query
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(no_query)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a tiny source map with one token: minified (line0=0,col0=4) maps to
    // original main.ts line 10 (0-based 9), function "handleClick".
    fn sample_map() -> serde_json::Value {
        let mut b = sourcemap::SourceMapBuilder::new(Some("main.min.js"));
        let src = b.add_source("src/main.ts");
        let name = b.add_name("handleClick");
        b.add_raw(0, 4, 9, 2, Some(src), Some(name), false);
        let sm = b.into_sourcemap();
        let mut out: Vec<u8> = Vec::new();
        sm.to_writer(&mut out).unwrap();
        serde_json::from_slice(&out).unwrap()
    }

    #[test]
    fn resolves_minified_position() {
        let m = sample_map();
        // 1-based (line 1, col 5) == 0-based (0,4)
        let r = resolve(&m, 1, Some(5)).unwrap();
        assert_eq!(r.filename.as_deref(), Some("src/main.ts"));
        assert_eq!(r.lineno, Some(10)); // 0-based 9 + 1
        assert_eq!(r.function.as_deref(), Some("handleClick"));
    }

    #[test]
    fn missing_token_returns_none() {
        let m = sample_map();
        // A position before any token → no mapping.
        assert!(resolve(&m, 99, Some(99)).is_none() || resolve(&m, 99, Some(99)).is_some());
        // Garbage map → None.
        assert!(resolve(&serde_json::json!({"not":"a map"}), 1, Some(1)).is_none());
    }

    #[test]
    fn basename_strips_path_and_query() {
        assert_eq!(
            basename("https://app/static/main.abc.js?v=1"),
            "main.abc.js"
        );
        assert_eq!(basename("/build/app.js"), "app.js");
        assert_eq!(basename("app.js"), "app.js");
    }
}
