//! Prometheus text exposition format parser (the ingest side).
//!
//! Hand-rolled for the same reason as `cron.rs`: the official
//! `prometheus-parse` / `openmetrics` crates drag in dependency trees we
//! don't otherwise need, and the line format is small:
//!
//!   metric_name{label="value",other="v"} 42.5 1719000000000
//!
//! Supported: bare and labelled samples, `\\` / `\"` / `\n` escapes in
//! label values, float values (including scientific notation), `#`
//! comment / TYPE / HELP lines (skipped). Client timestamps are parsed
//! but DISCARDED — the server stamps receive time, the same trust stance
//! as push pings and agent reports. Non-finite values (`NaN`, `±Inf`)
//! are skipped: they're valid exposition but meaningless to threshold
//! rules and poisonous to averages.

use std::collections::BTreeMap;

/// One parsed sample. Labels are sorted (BTreeMap) so identical label
/// sets always serialize identically — series identity must be stable.
#[derive(Debug, Clone, PartialEq)]
pub struct PromSample {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub value: f64,
}

/// Outcome of parsing one payload: the good samples plus a count of
/// lines that didn't parse (surfaced to the caller so a half-broken
/// exporter is visible instead of silently dropped).
#[derive(Debug, Default)]
pub struct PromParseOutcome {
    pub samples: Vec<PromSample>,
    pub skipped: usize,
}

/// Hard cap on samples accepted from one payload — a runaway client
/// guard, mirroring the agent report batch cap.
pub const MAX_SAMPLES_PER_PUSH: usize = 10_000;

pub fn parse(input: &str) -> PromParseOutcome {
    let mut out = PromParseOutcome::default();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if out.samples.len() >= MAX_SAMPLES_PER_PUSH {
            out.skipped += 1;
            continue;
        }
        match parse_line(line) {
            Some(s) if s.value.is_finite() => out.samples.push(s),
            _ => out.skipped += 1,
        }
    }
    out
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

fn parse_line(line: &str) -> Option<PromSample> {
    // Split off the name: up to '{' or whitespace.
    let name_end = line.find(|c: char| c == '{' || c.is_whitespace())?;
    let name = &line[..name_end];
    if !valid_name(name) {
        return None;
    }
    let rest = &line[name_end..];

    let (labels, rest) = if let Some(body) = rest.strip_prefix('{') {
        parse_labels(body)?
    } else {
        (BTreeMap::new(), rest)
    };

    // Value, then an optional timestamp we deliberately ignore.
    let mut parts = rest.split_whitespace();
    let value: f64 = parts.next()?.parse().ok()?;

    Some(PromSample {
        name: name.to_string(),
        labels,
        value,
    })
}

/// Parse `label="value",…}` (the caller stripped the opening brace).
/// Returns the labels and the remainder after the closing brace.
fn parse_labels(mut s: &str) -> Option<(BTreeMap<String, String>, &str)> {
    let mut labels = BTreeMap::new();
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix('}') {
            return Some((labels, rest));
        }
        let eq = s.find('=')?;
        let key = s[..eq].trim();
        if !valid_name(key) {
            return None;
        }
        s = s[eq + 1..].trim_start().strip_prefix('"')?;

        // Walk the quoted value honouring \\ \" \n escapes.
        let mut value = String::new();
        let mut chars = s.char_indices();
        let close = loop {
            let (i, c) = chars.next()?;
            match c {
                '"' => break i,
                '\\' => match chars.next()?.1 {
                    'n' => value.push('\n'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    other => value.push(other),
                },
                other => value.push(other),
            }
        };
        labels.insert(key.to_string(), value);
        s = s[close + 1..].trim_start();
        s = s.strip_prefix(',').unwrap_or(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_and_labelled_samples() {
        let out = parse(
            r#"
# HELP backup_duration_seconds How long the nightly backup took.
# TYPE backup_duration_seconds gauge
backup_duration_seconds 312.5
queue_depth{queue="emails",shard="0"} 42 1719000000000
"#,
        );
        assert_eq!(out.skipped, 0);
        assert_eq!(out.samples.len(), 2);
        assert_eq!(out.samples[0].name, "backup_duration_seconds");
        assert!(out.samples[0].labels.is_empty());
        assert_eq!(out.samples[0].value, 312.5);
        assert_eq!(out.samples[1].labels["queue"], "emails");
        assert_eq!(out.samples[1].labels["shard"], "0");
    }

    #[test]
    fn escapes_in_label_values() {
        let out = parse(r#"m{path="C:\\temp",note="say \"hi\"\nbye"} 1"#);
        assert_eq!(out.samples.len(), 1);
        assert_eq!(out.samples[0].labels["path"], r"C:\temp");
        assert_eq!(out.samples[0].labels["note"], "say \"hi\"\nbye");
    }

    #[test]
    fn scientific_notation_and_negatives() {
        let out = parse("a 1.5e3\nb -0.25\nc +2");
        let vals: Vec<f64> = out.samples.iter().map(|s| s.value).collect();
        assert_eq!(vals, vec![1500.0, -0.25, 2.0]);
    }

    #[test]
    fn skips_garbage_and_nonfinite() {
        let out = parse(
            "ok 1\n\
             1bad_name 2\n\
             no_value\n\
             nan_metric NaN\n\
             inf_metric +Inf\n\
             unclosed{a=\"b\" 3\n\
             ok2{} 4",
        );
        assert_eq!(out.samples.len(), 2);
        assert_eq!(out.skipped, 5);
        assert_eq!(out.samples[1].name, "ok2");
    }

    #[test]
    fn series_identity_is_label_order_independent() {
        let a = parse(r#"m{b="2",a="1"} 1"#).samples.remove(0);
        let b = parse(r#"m{a="1",b="2"} 1"#).samples.remove(0);
        assert_eq!(a.labels, b.labels);
        assert_eq!(
            serde_json::to_string(&a.labels).unwrap(),
            serde_json::to_string(&b.labels).unwrap()
        );
    }
}
