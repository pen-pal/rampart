//! Minimal RFC-4180 CSV field escaping, shared by the export endpoints
//! (audit log, delivery log, monitors, logs, traces).
//!
//! Only what the exporters need: quote a field when it contains a comma,
//! quote, CR or LF, doubling any embedded quotes. Empty stays empty (not
//! `""`) so blank cells round-trip cleanly.

/// Escape one CSV field per RFC 4180. Returns the input unchanged when it
/// needs no quoting; an empty string stays empty.
pub fn csv_escape(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if !(s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r')) {
        return s.to_string();
    }
    let escaped = s.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::csv_escape;

    #[test]
    fn passthrough_when_safe() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape(""), "");
    }

    #[test]
    fn quotes_and_doubles() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\nb"), "\"a\nb\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
