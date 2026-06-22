//! Syslog + JSON-lines log parsing into [`ParsedLog`].
//!
//! Rampart already stores logs in one shape ([`ParsedLog`] → the `logs`
//! table). This module lowers three common wire formats into that shape so a
//! plain syslog forwarder (rsyslog/syslog-ng) or any JSON-lines shipper can
//! feed Rampart's log tier — and, for free, its detection engine (which
//! matches on the same `service_name` / `severity` / `body` / `attributes`
//! fields). All parsers are TOLERANT: a malformed line yields `None` (syslog)
//! or a best-effort record (NDJSON) and is never fatal to the batch.
//!
//! - **RFC 5424** — `<PRI>1 TIMESTAMP HOST APP PROCID MSGID [SD] MSG`
//! - **RFC 3164** (BSD) — `<PRI>MMM DD HH:MM:SS HOST TAG: MSG`
//! - **NDJSON** — one JSON log object per line (`{level,message,service,...}`)

use crate::log::ParsedLog;
use serde_json::{json, Value};
use time::OffsetDateTime;

/// Map a syslog severity (0=emerg … 7=debug) to an OTLP severity number
/// (TRACE 1-4 / DEBUG 5-8 / INFO 9-12 / WARN 13-16 / ERROR 17-20 / FATAL
/// 21-24) plus a coarse text label, matching how the rest of the log tier
/// classifies levels.
fn severity_to_otlp(sev: u8) -> (i16, &'static str) {
    match sev {
        0 => (21, "emerg"),   // FATAL
        1 => (21, "alert"),   // FATAL
        2 => (19, "crit"),    // ERROR
        3 => (17, "err"),     // ERROR
        4 => (13, "warning"), // WARN
        5 => (10, "notice"),  // INFO
        6 => (9, "info"),     // INFO
        _ => (5, "debug"),    // DEBUG (7 + any out-of-range)
    }
}

/// Parse a leading `<PRI>` token, returning `(facility, severity, rest)`.
/// PRI = facility*8 + severity, both small ints. Returns `None` if the line
/// doesn't start with a well-formed priority.
fn parse_pri(line: &str) -> Option<(u8, u8, &str)> {
    let rest = line.strip_prefix('<')?;
    let close = rest.find('>')?;
    let pri: u16 = rest[..close].parse().ok()?;
    if pri > 191 {
        return None; // max valid PRI = 23*8 + 7
    }
    let severity = (pri % 8) as u8;
    let facility = (pri / 8) as u8;
    Some((facility, severity, &rest[close + 1..]))
}

/// A nil syslog field is the literal `-`; map it to `None`.
fn nilable(s: &str) -> Option<&str> {
    if s == "-" || s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Parse one RFC 5424 line. Shape after `<PRI>`:
/// `1 TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA MSG`.
pub fn parse_rfc5424(line: &str) -> Option<ParsedLog> {
    let (facility, severity, rest) = parse_pri(line)?;
    // VERSION must be "1" for RFC 5424; otherwise let the 3164 path try.
    let rest = rest.strip_prefix("1 ")?;
    // Split the 6 fixed header fields off the front; the remainder (which may
    // contain spaces) is structured-data + message.
    let mut it = rest.splitn(6, ' ');
    let timestamp = it.next()?;
    let hostname = it.next()?;
    let appname = it.next()?;
    let procid = it.next()?;
    let msgid = it.next()?;
    let sd_and_msg = it.next().unwrap_or("");

    // STRUCTURED-DATA is either `-` or one-or-more `[...]` elements; the MSG is
    // whatever follows. We don't decompose SD key/values (kept as a blob).
    let (structured_data, message) = split_structured_data(sd_and_msg);

    let (sev_num, sev_text) = severity_to_otlp(severity);
    let mut attrs = json!({
        "log.source": "syslog",
        "syslog.format": "rfc5424",
        "syslog.facility": facility,
        "syslog.severity": severity,
    });
    if let Some(h) = nilable(hostname) {
        attrs["host.name"] = json!(h);
    }
    if let Some(p) = nilable(procid) {
        attrs["syslog.procid"] = json!(p);
    }
    if let Some(m) = nilable(msgid) {
        attrs["syslog.msgid"] = json!(m);
    }
    if let Some(sd) = structured_data {
        attrs["syslog.structured_data"] = json!(sd);
    }

    Some(ParsedLog {
        time_ns: parse_ts_5424(timestamp),
        severity: sev_num,
        severity_text: Some(sev_text.to_string()),
        service_name: nilable(appname).unwrap_or("syslog").to_string(),
        body: message.trim().to_string(),
        trace_id: None,
        span_id: None,
        attributes: attrs,
    })
}

/// Split `STRUCTURED-DATA MSG` into `(Some(sd_blob)|None, msg)`. SD is `-`
/// (none) or a run of `[...]` elements; the MSG begins after the last `]` (or
/// after the `- `).
fn split_structured_data(s: &str) -> (Option<String>, &str) {
    if let Some(after) = s.strip_prefix("- ") {
        return (None, after);
    }
    if s == "-" {
        return (None, "");
    }
    if s.starts_with('[') {
        // Walk balanced brackets to find where SD ends.
        let mut depth = 0usize;
        let mut end = 0usize;
        for (i, c) in s.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                    }
                }
                _ => {}
            }
        }
        if end > 0 {
            let sd = &s[..end];
            let msg = s[end..].trim_start();
            return (Some(sd.to_string()), msg);
        }
    }
    (None, s)
}

/// RFC 5424 timestamps are RFC3339; return unix nanos, or 0 when absent /
/// unparseable (the DB defaults `received_at` so 0 is safe).
fn parse_ts_5424(ts: &str) -> i64 {
    if ts == "-" {
        return 0;
    }
    OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
        .map(|t| t.unix_timestamp_nanos() as i64)
        .unwrap_or(0)
}

/// Parse one RFC 3164 (BSD) line: `<PRI>MMM DD HH:MM:SS HOST TAG: MSG`. The
/// legacy timestamp has no year/zone, so we don't attempt to recover an exact
/// instant (time_ns=0 → server `received_at`); the value is in HOST/TAG/MSG.
pub fn parse_rfc3164(line: &str) -> Option<ParsedLog> {
    let (facility, severity, rest) = parse_pri(line)?;
    let rest = rest.trim_start();
    // The header is "MMM DD HH:MM:SS HOST " = 4 space-separated tokens before
    // the tag/message. Be lenient: if it doesn't look like a BSD timestamp,
    // treat the whole remainder as the message.
    let looks_dated = rest.len() > 15
        && rest.as_bytes().get(3) == Some(&b' ')
        && rest[..3].chars().all(|c| c.is_ascii_alphabetic());

    let (host, tag_and_msg) = if looks_dated {
        // skip "MMM DD HH:MM:SS " (the first 3 space-delimited time tokens)
        let after_ts = rest.splitn(4, ' ').nth(3).unwrap_or("");
        // next token is HOST, remainder is "TAG: MSG"
        let mut sp = after_ts.splitn(2, ' ');
        (sp.next(), sp.next().unwrap_or(""))
    } else {
        (None, rest)
    };

    // Tag is up to the first ':' (rsyslog appends "tag: message").
    let (tag, message) = match tag_and_msg.find(": ") {
        Some(i) => (Some(&tag_and_msg[..i]), &tag_and_msg[i + 2..]),
        None => (None, tag_and_msg),
    };

    let (sev_num, sev_text) = severity_to_otlp(severity);
    let mut attrs = json!({
        "log.source": "syslog",
        "syslog.format": "rfc3164",
        "syslog.facility": facility,
        "syslog.severity": severity,
    });
    if let Some(h) = host.and_then(nilable) {
        attrs["host.name"] = json!(h);
    }

    Some(ParsedLog {
        time_ns: 0,
        severity: sev_num,
        severity_text: Some(sev_text.to_string()),
        service_name: tag.and_then(nilable).unwrap_or("syslog").to_string(),
        body: message.trim().to_string(),
        trace_id: None,
        span_id: None,
        attributes: attrs,
    })
}

/// Parse one syslog line, trying RFC 5424 then falling back to RFC 3164.
/// Returns `None` for a line without a valid `<PRI>` prefix.
pub fn parse_line(line: &str) -> Option<ParsedLog> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    parse_rfc5424(line).or_else(|| parse_rfc3164(line))
}

/// Parse a newline-framed syslog body into logs, skipping unparseable lines.
pub fn parse_syslog(body: &str) -> Vec<ParsedLog> {
    body.lines().filter_map(parse_line).collect()
}

/// Best-effort map of a JSON log object to [`ParsedLog`]. Recognises the common
/// field aliases (`level`/`severity`, `message`/`msg`/`body`,
/// `service`/`service.name`, `timestamp`/`ts`/`time`, `trace_id`/`span_id`);
/// everything else is preserved under `attributes`.
pub fn parse_ndjson_value(v: &Value) -> ParsedLog {
    let obj = v.as_object();
    let get = |keys: &[&str]| -> Option<String> {
        let o = obj?;
        for k in keys {
            if let Some(val) = o.get(*k) {
                return match val {
                    Value::String(s) => Some(s.clone()),
                    Value::Null => None,
                    other => Some(other.to_string()),
                };
            }
        }
        None
    };

    let level = get(&["level", "severity", "severity_text", "lvl"]).unwrap_or_default();
    let (severity, severity_text) = level_to_severity(&level);
    let body = get(&["message", "msg", "body", "text"]).unwrap_or_default();
    let service = get(&["service", "service.name", "service_name", "app", "logger"])
        .unwrap_or_else(|| "json".to_string());
    let time_ns = get(&["timestamp", "ts", "time", "@timestamp"])
        .map(|s| parse_ts_loose(&s))
        .unwrap_or(0);
    let trace_id = get(&["trace_id", "traceId", "trace.id"]);
    let span_id = get(&["span_id", "spanId", "span.id"]);

    // Preserve the full original object under attributes for forensic fidelity.
    let mut attrs = json!({ "log.source": "json" });
    if let Some(o) = obj {
        for (k, val) in o {
            attrs[k.clone()] = val.clone();
        }
    }

    ParsedLog {
        time_ns,
        severity,
        severity_text: if severity_text.is_empty() {
            None
        } else {
            Some(severity_text.to_string())
        },
        service_name: service,
        body,
        trace_id,
        span_id,
        attributes: attrs,
    }
}

/// Parse an NDJSON body (one JSON object per line). Lines that aren't valid
/// JSON objects are skipped.
pub fn parse_ndjson(body: &str) -> Vec<ParsedLog> {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v.is_object())
        .map(|v| parse_ndjson_value(&v))
        .collect()
}

/// Map a free-text level keyword to an OTLP severity number + canonical text.
fn level_to_severity(level: &str) -> (i16, &'static str) {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" => (1, "trace"),
        "debug" | "dbg" => (5, "debug"),
        "info" | "information" | "notice" => (9, "info"),
        "warn" | "warning" => (13, "warn"),
        "error" | "err" | "crit" | "critical" => (17, "error"),
        "fatal" | "alert" | "emerg" | "emergency" | "panic" => (21, "fatal"),
        _ => (0, ""),
    }
}

/// Parse a loose timestamp (RFC3339, or unix seconds/millis as a number string)
/// into unix nanos; 0 when unparseable.
fn parse_ts_loose(s: &str) -> i64 {
    if let Ok(t) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        return t.unix_timestamp_nanos() as i64;
    }
    if let Ok(n) = s.parse::<i64>() {
        // Heuristic: < 1e12 → seconds, < 1e15 → millis, else assume nanos.
        return if n < 1_000_000_000_000 {
            n * 1_000_000_000
        } else if n < 1_000_000_000_000_000 {
            n * 1_000_000
        } else {
            n
        };
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc5424_canonical() {
        let line = "<34>1 2003-10-11T22:14:15.003Z mymachine.example.com su - ID47 - 'su root' failed for lonvick";
        let log = parse_line(line).expect("parse");
        assert_eq!(log.service_name, "su");
        assert_eq!(log.severity, 19); // facility 4, severity 2 (crit) -> ERROR 19
        assert_eq!(log.severity_text.as_deref(), Some("crit"));
        assert_eq!(log.body, "'su root' failed for lonvick");
        assert_eq!(log.attributes["host.name"], "mymachine.example.com");
        assert!(log.time_ns > 0);
    }

    #[test]
    fn rfc5424_with_structured_data() {
        let line = "<165>1 2003-10-11T22:14:15.003Z host evntslog - ID47 [exampleSDID@32473 iut=\"3\"] an application event";
        let log = parse_line(line).expect("parse");
        assert_eq!(log.service_name, "evntslog");
        assert_eq!(log.body, "an application event");
        assert!(log.attributes["syslog.structured_data"]
            .as_str()
            .unwrap()
            .contains("exampleSDID"));
    }

    #[test]
    fn rfc3164_bsd() {
        let line = "<34>Oct 11 22:14:15 mymachine su: 'su root' failed for lonvick";
        let log = parse_line(line).expect("parse");
        assert_eq!(log.attributes["syslog.format"], "rfc3164");
        assert_eq!(log.service_name, "su");
        assert_eq!(log.body, "'su root' failed for lonvick");
        assert_eq!(log.attributes["host.name"], "mymachine");
        assert_eq!(log.severity, 19);
    }

    #[test]
    fn severity_mapping_covers_buckets() {
        assert_eq!(severity_to_otlp(0).0, 21); // emerg -> FATAL
        assert_eq!(severity_to_otlp(3).0, 17); // err -> ERROR
        assert_eq!(severity_to_otlp(4).0, 13); // warning -> WARN
        assert_eq!(severity_to_otlp(6).0, 9); // info -> INFO
        assert_eq!(severity_to_otlp(7).0, 5); // debug -> DEBUG
    }

    #[test]
    fn ndjson_object() {
        let v: Value = serde_json::from_str(
            r#"{"level":"error","message":"db down","service":"api","trace_id":"abc","extra":42}"#,
        )
        .unwrap();
        let log = parse_ndjson_value(&v);
        assert_eq!(log.severity, 17);
        assert_eq!(log.severity_text.as_deref(), Some("error"));
        assert_eq!(log.service_name, "api");
        assert_eq!(log.body, "db down");
        assert_eq!(log.trace_id.as_deref(), Some("abc"));
        assert_eq!(log.attributes["extra"], 42);
    }

    #[test]
    fn malformed_lines_skipped() {
        let body = "not a syslog line\n<34>1 2003-10-11T22:14:15Z h app - - - ok\ngarbage";
        let logs = parse_syslog(body);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "ok");
    }

    #[test]
    fn pri_out_of_range_rejected() {
        assert!(parse_pri("<999>1 ...").is_none());
        assert!(parse_line("<999> nope").is_none());
    }
}
