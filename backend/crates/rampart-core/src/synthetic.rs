//! Synthetic transactions — multi-step HTTP sequences.
//!
//! A `synthetic` monitor stores an ordered list of steps in its
//! `config.steps` JSONB. Each step makes an HTTP request, optionally
//! extracts values from the response into named variables, and asserts on
//! the response; variables interpolate into later steps via `{{name}}`. The
//! runner (rampart-checker's synthetic probe) executes the sequence and
//! stops at the first failed assertion. This module holds the wire types,
//! the config parser/validator, and the pure string/JSON/compare helpers
//! the runner builds on — all unit-testable without a network.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_STEPS: usize = 20;
/// Combined extract + assert rules a single step may carry.
pub const MAX_RULES_PER_STEP: usize = 15;

/// One request in the sequence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticStep {
    /// Optional human label, surfaced in failure messages ("step 2 (login)").
    #[serde(default)]
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    /// Header name → value; values may contain `{{var}}` placeholders.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Request body; may contain `{{var}}` placeholders.
    #[serde(default)]
    pub body: Option<String>,
    /// Values pulled from this step's response into the variable bag.
    #[serde(default)]
    pub extract: Vec<Extract>,
    /// Pass/fail checks against this step's response.
    #[serde(default)]
    pub assert: Vec<Assertion>,
}

/// Where an extracted value comes from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractSource {
    /// A dotted/indexed path into the JSON response body (see [`json_lookup`]).
    Json,
    /// A response header, named by `path`.
    Header,
    /// The numeric HTTP status code.
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extract {
    /// Variable name; later steps reference it as `{{var}}`.
    pub var: String,
    pub from: ExtractSource,
    /// JSON path (for `json`) or header name (for `header`); ignored for `status`.
    #[serde(default)]
    pub path: Option<String>,
}

/// What part of the response an assertion checks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssertKind {
    /// Compare the numeric status code.
    Status,
    /// Compare a value at a JSON path in the body.
    Json,
    /// Compare a response header value.
    Header,
    /// Substring match against the raw body (`op`/`path` ignored).
    BodyContains,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    #[default]
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Contains,
}

/// A single pass/fail check. Flat (rather than a tagged enum) so the
/// dashboard's step builder can produce one uniform shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assertion {
    pub kind: AssertKind,
    /// JSON path (for `json`) or header name (for `header`).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub op: CompareOp,
    /// The expected value (compared per `op`). For `body_contains` this is
    /// the required substring.
    pub value: String,
}

/// A parsed, validated step sequence.
#[derive(Debug, Clone)]
pub struct SyntheticPlan {
    pub steps: Vec<SyntheticStep>,
}

impl SyntheticPlan {
    /// Parse `config.steps` into a validated plan. Returns `None` when the
    /// key is absent, the JSON is malformed, or validation fails — the
    /// probe then surfaces a clear misconfiguration message (matching the
    /// lazy-validation convention of cron / json_query).
    pub fn from_config(config: &serde_json::Value) -> Option<SyntheticPlan> {
        let steps_val = config.get("steps")?;
        let steps: Vec<SyntheticStep> = serde_json::from_value(steps_val.clone()).ok()?;
        let plan = SyntheticPlan { steps };
        plan.validate().ok()?;
        Some(plan)
    }

    /// Shape validation: non-empty, bounded, each step has a method + URL
    /// and a bounded number of rules with non-empty variable names.
    pub fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty() {
            return Err("a synthetic monitor needs at least one step".into());
        }
        if self.steps.len() > MAX_STEPS {
            return Err(format!("at most {MAX_STEPS} steps"));
        }
        for (i, s) in self.steps.iter().enumerate() {
            let n = i + 1;
            if s.method.trim().is_empty() {
                return Err(format!("step {n} has no HTTP method"));
            }
            if s.url.trim().is_empty() {
                return Err(format!("step {n} has no URL"));
            }
            if s.extract.len() + s.assert.len() > MAX_RULES_PER_STEP {
                return Err(format!("step {n} has too many rules (max {MAX_RULES_PER_STEP})"));
            }
            for e in &s.extract {
                if e.var.trim().is_empty() {
                    return Err(format!("step {n} has an extraction with no variable name"));
                }
            }
        }
        Ok(())
    }
}

/// Replace every `{{name}}` in `template` with the matching variable. An
/// unknown name is left as the literal `{{name}}` so a missing extraction is
/// visible in the request (and in the resulting failure) rather than being
/// silently blanked. Hand-rolled — no template-engine dependency.
pub fn interpolate(template: &str, vars: &BTreeMap<String, String>) -> String {
    if !template.contains("{{") {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("}}") {
            Some(close) => {
                let name = after[..close].trim();
                match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => {
                        // Leave the placeholder intact.
                        out.push_str("{{");
                        out.push_str(&after[..close]);
                        out.push_str("}}");
                    }
                }
                rest = &after[close + 2..];
            }
            None => {
                // Unterminated `{{` — emit the rest verbatim and stop.
                out.push_str("{{");
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Resolve a dotted/indexed path into a JSON value and render the leaf as a
/// string (strings as-is; numbers/bools/null via their JSON form; objects and
/// arrays as compact JSON). Segments split on `.`; a segment that parses as a
/// number indexes into an array. Empty segments (leading/repeated dots) are
/// skipped, matching the existing json_query helper. `None` when the path
/// doesn't resolve.
pub fn json_lookup(root: &serde_json::Value, path: &str) -> Option<String> {
    let mut node = root;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        node = match node {
            serde_json::Value::Array(items) => {
                let idx: usize = segment.parse().ok()?;
                items.get(idx)?
            }
            _ => node.get(segment)?,
        };
    }
    Some(match node {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    })
}

/// Compare `actual` against `expected` under `op`. `eq`/`ne`/`contains` are
/// string operations; the ordering ops (`lt`/`lte`/`gt`/`gte`) parse both
/// sides as numbers and are false when either side isn't numeric.
pub fn compare(actual: &str, op: CompareOp, expected: &str) -> bool {
    match op {
        CompareOp::Eq => actual == expected,
        CompareOp::Ne => actual != expected,
        CompareOp::Contains => actual.contains(expected),
        CompareOp::Lt | CompareOp::Lte | CompareOp::Gt | CompareOp::Gte => {
            let (Ok(a), Ok(e)) = (actual.parse::<f64>(), expected.parse::<f64>()) else {
                return false;
            };
            match op {
                CompareOp::Lt => a < e,
                CompareOp::Lte => a <= e,
                CompareOp::Gt => a > e,
                CompareOp::Gte => a >= e,
                _ => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn interpolates_known_vars_and_keeps_unknown() {
        let v = vars(&[("token", "abc"), ("id", "7")]);
        assert_eq!(interpolate("Bearer {{token}}", &v), "Bearer abc");
        assert_eq!(interpolate("/u/{{id}}/x", &v), "/u/7/x");
        assert_eq!(interpolate("{{ token }}", &v), "abc"); // trims inside braces
        // Unknown var is left literal so the miss is visible.
        assert_eq!(interpolate("{{nope}}", &v), "{{nope}}");
        // No placeholders — untouched.
        assert_eq!(interpolate("plain", &v), "plain");
        // Unterminated brace — emitted verbatim, no panic.
        assert_eq!(interpolate("a {{ b", &v), "a {{ b");
    }

    #[test]
    fn json_lookup_walks_objects_and_arrays() {
        let v = json!({
            "data": { "token": "xyz", "items": [{"id": 1}, {"id": 2}] },
            "count": 5,
            "ok": true,
            "missing": null,
        });
        assert_eq!(json_lookup(&v, "data.token"), Some("xyz".to_string()));
        assert_eq!(json_lookup(&v, "data.items.1.id"), Some("2".to_string()));
        assert_eq!(json_lookup(&v, "count"), Some("5".to_string()));
        assert_eq!(json_lookup(&v, "ok"), Some("true".to_string()));
        assert_eq!(json_lookup(&v, "missing"), Some("null".to_string()));
        // Leading/repeated dots tolerated.
        assert_eq!(json_lookup(&v, ".data..token"), Some("xyz".to_string()));
        // Missing path / out-of-range index → None.
        assert_eq!(json_lookup(&v, "data.nope"), None);
        assert_eq!(json_lookup(&v, "data.items.9.id"), None);
    }

    #[test]
    fn compare_string_and_numeric_ops() {
        assert!(compare("200", CompareOp::Eq, "200"));
        assert!(compare("ok", CompareOp::Ne, "fail"));
        assert!(compare("hello world", CompareOp::Contains, "world"));
        assert!(compare("200", CompareOp::Lt, "300"));
        assert!(compare("300", CompareOp::Gte, "300"));
        assert!(!compare("500", CompareOp::Lt, "300"));
        // Non-numeric under an ordering op is false, not a panic.
        assert!(!compare("abc", CompareOp::Lt, "300"));
    }

    #[test]
    fn from_config_parses_and_validates() {
        let cfg = json!({
            "steps": [
                {
                    "name": "login",
                    "method": "POST",
                    "url": "https://api.example.com/login",
                    "headers": { "Content-Type": "application/json" },
                    "body": "{\"u\":\"x\"}",
                    "extract": [{ "var": "token", "from": "json", "path": "data.token" }],
                    "assert": [{ "kind": "status", "op": "eq", "value": "200" }]
                },
                {
                    "method": "GET",
                    "url": "https://api.example.com/me",
                    "headers": { "Authorization": "Bearer {{token}}" },
                    "assert": [
                        { "kind": "json", "path": "data.active", "op": "eq", "value": "true" },
                        { "kind": "body_contains", "value": "welcome" }
                    ]
                }
            ]
        });
        let plan = SyntheticPlan::from_config(&cfg).expect("valid plan");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].extract[0].from, ExtractSource::Json);
        assert_eq!(plan.steps[1].assert[0].op, CompareOp::Eq);
    }

    #[test]
    fn from_config_rejects_bad_plans() {
        // Missing key.
        assert!(SyntheticPlan::from_config(&json!({})).is_none());
        // Empty step list.
        assert!(SyntheticPlan::from_config(&json!({ "steps": [] })).is_none());
        // Step with no URL.
        let no_url = json!({ "steps": [{ "method": "GET", "url": "" }] });
        assert!(SyntheticPlan::from_config(&no_url).is_none());
        // Extraction with empty var name.
        let bad_var = json!({ "steps": [{
            "method": "GET", "url": "https://x",
            "extract": [{ "var": "", "from": "status" }]
        }] });
        assert!(SyntheticPlan::from_config(&bad_var).is_none());
    }

    #[test]
    fn op_defaults_to_eq_when_omitted() {
        let a: Assertion =
            serde_json::from_value(json!({ "kind": "status", "value": "200" })).unwrap();
        assert_eq!(a.op, CompareOp::Eq);
    }
}
