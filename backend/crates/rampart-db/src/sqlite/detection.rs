//! SQLite `detection` domain — SIEM detection rules over the `logs` tier, the
//! evaluation tick that raises `detection_findings`, and the findings feed.
//! Mirrors the PG free-fn surface (`crate::detection`); shares its `FindingEvent`
//! and `PreviewResult` types so `impl Store` delegates uniformly.
//!
//! Dialect notes vs PG:
//! - `body ~* regex` has no SQLite equivalent → degraded to a case-insensitive
//!   `LIKE '%pattern%'` substring (same homelab compromise as the logs FTS
//!   degrade). `regex_is_valid` therefore always accepts: a "regex" is treated
//!   as a literal substring, so no pattern is invalid.
//! - `attributes->>key` → `json_extract(attributes, '$."key"')`.
//! - window bounds use the process clock (`OffsetDateTime::now_utc`) — in the
//!   single-binary tier the app clock *is* the DB clock, so this matches PG's
//!   `now()`-folded bounds.
//! - the per-entity `array_agg(... ORDER BY ...)[1]` newest-sample is computed
//!   app-side (no ordered aggregate in SQLite).
//! - watermarks are whole-second `unixepoch()` (vs PG's sub-second `now()`), so a
//!   log landing in the exact second of a tick's watermark can be missed by the
//!   next tick's `received_at > wfrom` lower bound. Acceptable for the homelab
//!   tier; the strict `>` is kept so matches are never double-counted.

use super::{oid, raw_uuid, ts};
use crate::detection::{FindingEvent, PreviewResult};
use crate::{DbError, DbResult};
use rampart_core::detection::{
    DetectionCondition, DetectionFinding, DetectionRule, DetectionSeverity, NewDetectionRule,
    UpdateDetectionRule,
};
use rampart_core::ids::{
    DetectionFindingId, DetectionRuleId, EscalationPolicyId, NotificationId, OrgId,
};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use time::{Duration, OffsetDateTime};

const RCOLS: &str = "id, org_id, name, description, enabled, severity, service, min_level, \
     body_regex, attr_key, attr_val, threshold, window_seconds, cooldown_seconds, group_by, \
     condition, channel_ids, escalation_policy_id, last_checked_at, created_at";

const FCOLS: &str = "id, rule_id, rule_name, severity, match_count, sample, service, entity, \
     window_from, window_to, created_at, acknowledged_at";

fn channel_ids_from(s: &str) -> Vec<NotificationId> {
    serde_json::from_str::<Vec<String>>(s)
        .unwrap_or_default()
        .iter()
        .map(|u| NotificationId::from_uuid(raw_uuid(u)))
        .collect()
}

/// `json_extract` path for an attribute key (mirrors `slos::containment`).
fn attr_path(key: &str) -> String {
    format!("$.\"{}\"", key.replace('"', "\"\""))
}

/// Escape LIKE wildcards so a user substring matches literally (default `\`).
fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn rule_from(r: &sqlx::sqlite::SqliteRow) -> DetectionRule {
    DetectionRule {
        id: DetectionRuleId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        org_id: oid(&r.get::<String, _>("org_id")),
        name: r.get("name"),
        description: r.get("description"),
        enabled: r.get::<i64, _>("enabled") != 0,
        severity: DetectionSeverity::from_db(&r.get::<String, _>("severity")),
        service: r.get("service"),
        min_level: r.get::<i64, _>("min_level") as i16,
        body_regex: r.get("body_regex"),
        attr_key: r.get("attr_key"),
        attr_val: r.get("attr_val"),
        threshold: r.get::<i64, _>("threshold") as i32,
        window_seconds: r.get::<i64, _>("window_seconds") as i32,
        cooldown_seconds: r.get::<i64, _>("cooldown_seconds") as i32,
        group_by: r.get("group_by"),
        condition: r
            .get::<Option<String>, _>("condition")
            .and_then(|s| serde_json::from_str(&s).ok()),
        channel_ids: channel_ids_from(&r.get::<String, _>("channel_ids")),
        escalation_policy_id: r
            .get::<Option<String>, _>("escalation_policy_id")
            .map(|s| EscalationPolicyId::from_uuid(raw_uuid(&s))),
        last_checked_at: r.get::<Option<i64>, _>("last_checked_at").map(ts),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

fn finding_from(r: &sqlx::sqlite::SqliteRow) -> DetectionFinding {
    DetectionFinding {
        id: DetectionFindingId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        rule_id: DetectionRuleId::from_uuid(raw_uuid(&r.get::<String, _>("rule_id"))),
        rule_name: r.get("rule_name"),
        severity: DetectionSeverity::from_db(&r.get::<String, _>("severity")),
        match_count: r.get::<i64, _>("match_count"),
        sample: r.get::<Option<String>, _>("sample"),
        service: r.get::<Option<String>, _>("service"),
        entity: r.get::<Option<String>, _>("entity"),
        window_from: ts(r.get::<i64, _>("window_from")),
        window_to: ts(r.get::<i64, _>("window_to")),
        created_at: ts(r.get::<i64, _>("created_at")),
        acknowledged_at: r.get::<Option<i64>, _>("acknowledged_at").map(ts),
    }
}

/// SQLite degrades `~*` to a literal substring, so every pattern is valid.
pub async fn regex_is_valid(_pool: &SqlitePool, _pattern: &str) -> DbResult<bool> {
    Ok(true)
}

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<DetectionRule>> {
    let sql = format!("SELECT {RCOLS} FROM detection_rules WHERE org_id = ? ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(rule_from).collect())
}

pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<DetectionRule>> {
    let sql = format!("SELECT {RCOLS} FROM detection_rules ORDER BY created_at");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(rule_from).collect())
}

pub async fn get(pool: &SqlitePool, id: DetectionRuleId, org_id: OrgId) -> DbResult<DetectionRule> {
    let sql = format!("SELECT {RCOLS} FROM detection_rules WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(rule_from(&row))
}

pub async fn get_unscoped(pool: &SqlitePool, id: DetectionRuleId) -> DbResult<DetectionRule> {
    let sql = format!("SELECT {RCOLS} FROM detection_rules WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(rule_from(&row))
}

pub async fn create(
    pool: &SqlitePool,
    input: NewDetectionRule,
    org_id: OrgId,
) -> DbResult<DetectionRule> {
    let id = DetectionRuleId::new();
    let channel_ids: Vec<String> = input.channel_ids.iter().map(|c| c.0.to_string()).collect();
    let condition_json: Option<String> = input
        .condition
        .as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    sqlx::query(
        "INSERT INTO detection_rules
            (id, name, description, enabled, severity, service, min_level, body_regex, attr_key,
             attr_val, threshold, window_seconds, cooldown_seconds, group_by, condition,
             channel_ids, escalation_policy_id, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.description)
    .bind(input.enabled as i64)
    .bind(input.severity.as_str())
    .bind(input.service)
    .bind(input.min_level)
    .bind(input.body_regex)
    .bind(input.attr_key)
    .bind(input.attr_val)
    .bind(input.threshold)
    .bind(input.window_seconds)
    .bind(input.cooldown_seconds)
    .bind(input.group_by)
    .bind(condition_json)
    .bind(serde_json::to_string(&channel_ids).unwrap_or_else(|_| "[]".into()))
    .bind(input.escalation_policy_id.map(|p| p.0.to_string()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: DetectionRuleId,
    patch: UpdateDetectionRule,
    org_id: OrgId,
) -> DbResult<DetectionRule> {
    let channel_ids: Option<String> = patch.channel_ids.map(|v| {
        let ids: Vec<String> = v.iter().map(|c| c.0.to_string()).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())
    });
    // Direct set (not COALESCE), matching PG: the rule form always sends the full
    // intended condition (tree, or null to clear back to the flat matcher).
    let condition_json: Option<String> = patch
        .condition
        .as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let res = sqlx::query(
        "UPDATE detection_rules SET
            name           = COALESCE(?, name),
            description    = COALESCE(?, description),
            enabled        = COALESCE(?, enabled),
            severity       = COALESCE(?, severity),
            service        = COALESCE(?, service),
            min_level      = COALESCE(?, min_level),
            body_regex     = COALESCE(?, body_regex),
            attr_key       = COALESCE(?, attr_key),
            attr_val       = COALESCE(?, attr_val),
            threshold      = COALESCE(?, threshold),
            window_seconds = COALESCE(?, window_seconds),
            channel_ids    = COALESCE(?, channel_ids),
            escalation_policy_id = COALESCE(?, escalation_policy_id),
            cooldown_seconds = COALESCE(?, cooldown_seconds),
            condition      = ?,
            group_by       = COALESCE(?, group_by)
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.description)
    .bind(patch.enabled.map(|b| b as i64))
    .bind(patch.severity.map(|s| s.as_str().to_string()))
    .bind(patch.service)
    .bind(patch.min_level)
    .bind(patch.body_regex)
    .bind(patch.attr_key)
    .bind(patch.attr_val)
    .bind(patch.threshold)
    .bind(patch.window_seconds)
    .bind(channel_ids)
    .bind(patch.escalation_policy_id.map(|p| p.0.to_string()))
    .bind(patch.cooldown_seconds)
    .bind(condition_json)
    .bind(patch.group_by)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

pub async fn delete(pool: &SqlitePool, id: DetectionRuleId, org_id: OrgId) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM detection_rules WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn preview(
    pool: &SqlitePool,
    service: &str,
    min_level: i16,
    body_regex: &str,
    attr_key: &str,
    attr_val: &str,
    window_seconds: i32,
    org_id: OrgId,
) -> DbResult<PreviewResult> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - window_seconds as i64;
    let path = attr_path(attr_key);
    let org = org_id.0.to_string();
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM logs
         WHERE received_at >= ?
           AND (? = '' OR service_name = ?)
           AND severity >= ?
           AND (? = '' OR body LIKE '%' || ? || '%')
           AND (? = '' OR json_extract(attributes, ?) = ?)
           AND org_id = ?",
    )
    .bind(cutoff)
    .bind(service)
    .bind(service)
    .bind(min_level)
    .bind(body_regex)
    .bind(body_regex)
    .bind(attr_key)
    .bind(&path)
    .bind(attr_val)
    .bind(&org)
    .fetch_one(pool)
    .await?;

    let samples: Vec<(String,)> = sqlx::query_as(
        "SELECT substr(body, 1, 300) FROM logs
         WHERE received_at >= ?
           AND (? = '' OR service_name = ?)
           AND severity >= ?
           AND (? = '' OR body LIKE '%' || ? || '%')
           AND (? = '' OR json_extract(attributes, ?) = ?)
           AND org_id = ?
         ORDER BY received_at DESC
         LIMIT 5",
    )
    .bind(cutoff)
    .bind(service)
    .bind(service)
    .bind(min_level)
    .bind(body_regex)
    .bind(body_regex)
    .bind(attr_key)
    .bind(&path)
    .bind(attr_val)
    .bind(&org)
    .fetch_all(pool)
    .await?;

    Ok(PreviewResult {
        count,
        samples: samples.into_iter().map(|(s,)| s).collect(),
    })
}

// ── match evaluation ────────────────────────────────────────────────────────

/// Push the rule's match predicate (boolean tree if present, else flat fields).
fn push_match(qb: &mut QueryBuilder<Sqlite>, rule: &DetectionRule) {
    match &rule.condition {
        Some(c) => push_condition(qb, c),
        None => push_flat(qb, rule),
    }
}

/// Legacy flat-field match as bound SQL (mirrors PG `push_flat`).
fn push_flat(qb: &mut QueryBuilder<Sqlite>, rule: &DetectionRule) {
    qb.push("(");
    qb.push_bind(rule.service.clone());
    qb.push(" = '' OR l.service_name = ");
    qb.push_bind(rule.service.clone());
    qb.push(") AND l.severity >= ");
    qb.push_bind(rule.min_level);
    qb.push(" AND (");
    qb.push_bind(rule.body_regex.clone());
    qb.push(" = '' OR l.body LIKE '%' || ");
    qb.push_bind(rule.body_regex.clone());
    qb.push(" || '%') AND (");
    qb.push_bind(rule.attr_key.clone());
    qb.push(" = '' OR json_extract(l.attributes, ");
    qb.push_bind(attr_path(&rule.attr_key));
    qb.push(") = ");
    qb.push_bind(rule.attr_val.clone());
    qb.push(")");
}

/// Compile a condition node, binding every leaf value (no interpolation).
fn push_condition(qb: &mut QueryBuilder<Sqlite>, c: &DetectionCondition) {
    match c {
        DetectionCondition::And { conditions } => push_join(qb, conditions, " AND ", "1=1"),
        DetectionCondition::Or { conditions } => push_join(qb, conditions, " OR ", "1=0"),
        DetectionCondition::Not { condition } => {
            qb.push("NOT (");
            push_condition(qb, condition);
            qb.push(")");
        }
        DetectionCondition::Service { value } => {
            qb.push("l.service_name = ");
            qb.push_bind(value.clone());
        }
        DetectionCondition::MinLevel { value } => {
            qb.push("l.severity >= ");
            qb.push_bind(*value);
        }
        // Regex degraded to a literal substring (no SQLite regex engine).
        DetectionCondition::BodyRegex { value } => {
            qb.push("l.body LIKE '%' || ");
            qb.push_bind(value.clone());
            qb.push(" || '%'");
        }
        DetectionCondition::BodyContains { value } => {
            qb.push("l.body LIKE ");
            qb.push_bind(format!("%{}%", like_escape(value)));
            qb.push(" ESCAPE '\\'");
        }
        DetectionCondition::Attr { key, value } => {
            // COALESCE so a MISSING attribute reads as "" (NOT attr=val is TRUE
            // for an absent key — the intuitive detection semantics).
            qb.push("COALESCE(json_extract(l.attributes, ");
            qb.push_bind(attr_path(key));
            qb.push("), '') = ");
            qb.push_bind(value.clone());
        }
    }
}

fn push_join(
    qb: &mut QueryBuilder<Sqlite>,
    conditions: &[DetectionCondition],
    sep: &str,
    empty: &str,
) {
    if conditions.is_empty() {
        qb.push(empty);
        return;
    }
    qb.push("(");
    for (i, c) in conditions.iter().enumerate() {
        if i > 0 {
            qb.push(sep);
        }
        qb.push("(");
        push_condition(qb, c);
        qb.push(")");
    }
    qb.push(")");
}

/// Count log records matching the rule in `(wfrom, wto]`.
async fn count_matches(
    pool: &SqlitePool,
    rule: &DetectionRule,
    wfrom: i64,
    wto: i64,
) -> DbResult<i64> {
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new("SELECT COUNT(*) FROM logs l WHERE l.received_at > ");
    qb.push_bind(wfrom);
    qb.push(" AND l.received_at <= ");
    qb.push_bind(wto);
    qb.push(" AND l.org_id = ");
    qb.push_bind(rule.org_id.0.to_string());
    qb.push(" AND (");
    push_match(&mut qb, rule);
    qb.push(")");
    Ok(qb.build_query_scalar::<i64>().fetch_one(pool).await?)
}

/// Newest matching log body (≤ 500 chars) in `(wfrom, wto]`.
async fn sample_match(
    pool: &SqlitePool,
    rule: &DetectionRule,
    wfrom: i64,
    wto: i64,
) -> DbResult<Option<String>> {
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new("SELECT substr(l.body, 1, 500) FROM logs l WHERE l.received_at > ");
    qb.push_bind(wfrom);
    qb.push(" AND l.received_at <= ");
    qb.push_bind(wto);
    qb.push(" AND l.org_id = ");
    qb.push_bind(rule.org_id.0.to_string());
    qb.push(" AND (");
    push_match(&mut qb, rule);
    qb.push(") ORDER BY l.received_at DESC LIMIT 1");
    Ok(qb
        .build_query_scalar::<String>()
        .fetch_optional(pool)
        .await?)
}

/// Per-entity grouped eval: count matches per `json_extract(attributes,group_by)`
/// in the window, returning each entity reaching `threshold` with its newest
/// sample. Aggregated app-side (no ordered aggregate in SQLite).
async fn grouped_eval(
    pool: &SqlitePool,
    rule: &DetectionRule,
    wfrom: i64,
    wto: i64,
) -> DbResult<Vec<(String, i64, Option<String>)>> {
    let path = attr_path(&rule.group_by);
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT json_extract(l.attributes, ");
    qb.push_bind(path.clone());
    qb.push(") AS entity, substr(l.body, 1, 500) AS sample, l.received_at AS rat FROM logs l WHERE l.received_at > ");
    qb.push_bind(wfrom);
    qb.push(" AND l.received_at <= ");
    qb.push_bind(wto);
    qb.push(" AND l.org_id = ");
    qb.push_bind(rule.org_id.0.to_string());
    qb.push(" AND json_extract(l.attributes, ");
    qb.push_bind(path);
    qb.push(") IS NOT NULL AND (");
    push_match(&mut qb, rule);
    qb.push(") ORDER BY l.received_at DESC");
    let rows = qb.build().fetch_all(pool).await?;
    // Rows arrive newest-first, so the first sample seen per entity is the newest.
    let mut acc: std::collections::HashMap<String, (i64, Option<String>)> =
        std::collections::HashMap::new();
    for r in &rows {
        let entity = r.get::<Option<String>, _>("entity").unwrap_or_default();
        let sample = r.get::<Option<String>, _>("sample");
        let e = acc.entry(entity).or_insert((0, None));
        e.0 += 1;
        if e.1.is_none() {
            e.1 = sample;
        }
    }
    Ok(acc
        .into_iter()
        .filter(|(_, (count, _))| *count >= rule.threshold as i64)
        .map(|(entity, (count, sample))| (entity, count, sample))
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn insert_finding(
    pool: &SqlitePool,
    rule: &DetectionRule,
    count: i64,
    sample: Option<String>,
    service: Option<String>,
    entity: Option<String>,
    wfrom: OffsetDateTime,
    wto: OffsetDateTime,
) -> DbResult<DetectionFinding> {
    let fid = DetectionFindingId::new();
    let sql = format!(
        "INSERT INTO detection_findings
            (id, rule_id, rule_name, severity, match_count, sample, service, entity,
             window_from, window_to)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING {FCOLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(fid.0.to_string())
        .bind(rule.id.0.to_string())
        .bind(&rule.name)
        .bind(rule.severity.as_str())
        .bind(count)
        .bind(sample)
        .bind(service)
        .bind(entity)
        .bind(wfrom.unix_timestamp())
        .bind(wto.unix_timestamp())
        .fetch_one(pool)
        .await?;
    Ok(finding_from(&row))
}

/// Evaluate every enabled rule once. Mirrors PG `evaluate_tick`: count NEW
/// matches in `(last_checked_at, now]` (or the lookback on first run), raise a
/// finding at `threshold` (honoring per-rule / per-entity cooldown), then
/// advance the watermark to `now` regardless of outcome.
pub async fn evaluate_tick(pool: &SqlitePool) -> DbResult<Vec<FindingEvent>> {
    let now = OffsetDateTime::now_utc();
    let now_unix = now.unix_timestamp();
    let mut out = Vec::new();

    for rule in list_all(pool).await?.into_iter().filter(|r| r.enabled) {
        let cooldown = rule.cooldown_seconds as i64;
        let wfrom = rule
            .last_checked_at
            .unwrap_or_else(|| now - Duration::seconds(rule.window_seconds as i64));
        let (wfrom_u, wto_u) = (wfrom.unix_timestamp(), now_unix);

        if rule.group_by.is_empty() {
            let count = count_matches(pool, &rule, wfrom_u, wto_u).await?;
            let suppressed = rule.cooldown_seconds > 0
                && has_recent_finding(pool, rule.id, cooldown, None).await?;
            if count >= rule.threshold as i64 && !suppressed {
                let sample = sample_match(pool, &rule, wfrom_u, wto_u).await?;
                let service = (!rule.service.is_empty()).then(|| rule.service.clone());
                let finding =
                    insert_finding(pool, &rule, count, sample, service, None, wfrom, now).await?;
                out.push(FindingEvent {
                    finding,
                    channel_ids: rule.channel_ids.clone(),
                    escalation_policy_id: rule.escalation_policy_id,
                });
            }
        } else {
            for (entity, count, sample) in grouped_eval(pool, &rule, wfrom_u, wto_u).await? {
                if rule.cooldown_seconds > 0
                    && has_recent_finding(pool, rule.id, cooldown, Some(&entity)).await?
                {
                    continue;
                }
                let finding =
                    insert_finding(pool, &rule, count, sample, None, Some(entity), wfrom, now)
                        .await?;
                out.push(FindingEvent {
                    finding,
                    channel_ids: rule.channel_ids.clone(),
                    escalation_policy_id: rule.escalation_policy_id,
                });
            }
        }

        sqlx::query("UPDATE detection_rules SET last_checked_at = ? WHERE id = ?")
            .bind(wto_u)
            .bind(rule.id.0.to_string())
            .execute(pool)
            .await?;
    }
    Ok(out)
}

/// Did this rule raise any finding within the last `secs` seconds? `entity =
/// Some` scopes to that entity (per-entity cooldown for group_by rules).
pub async fn has_recent_finding(
    pool: &SqlitePool,
    rule_id: DetectionRuleId,
    secs: i64,
    entity: Option<&str>,
) -> DbResult<bool> {
    let cutoff = OffsetDateTime::now_utc().unix_timestamp() - secs;
    let (exists,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM detection_findings
            WHERE rule_id = ? AND created_at >= ? AND (? IS NULL OR entity = ?)
        )",
    )
    .bind(rule_id.0.to_string())
    .bind(cutoff)
    .bind(entity)
    .bind(entity)
    .fetch_one(pool)
    .await?;
    Ok(exists != 0)
}

pub async fn list_findings(
    pool: &SqlitePool,
    limit: i64,
    open_only: bool,
) -> DbResult<Vec<DetectionFinding>> {
    let sql = format!(
        "SELECT {FCOLS} FROM detection_findings
         WHERE (? = 0 OR acknowledged_at IS NULL)
         ORDER BY created_at DESC LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(open_only as i64)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(finding_from).collect())
}

pub async fn list_findings_for_org(
    pool: &SqlitePool,
    limit: i64,
    open_only: bool,
    org_id: OrgId,
) -> DbResult<Vec<DetectionFinding>> {
    let sql = "SELECT f.id, f.rule_id, f.rule_name, f.severity, f.match_count, f.sample, f.service,
                f.entity, f.window_from, f.window_to, f.created_at, f.acknowledged_at
         FROM detection_findings f
         JOIN detection_rules r ON r.id = f.rule_id
         WHERE (? = 0 OR f.acknowledged_at IS NULL) AND r.org_id = ?
         ORDER BY f.created_at DESC LIMIT ?";
    let rows = sqlx::query(sql)
        .bind(open_only as i64)
        .bind(org_id.0.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(finding_from).collect())
}

pub async fn finding_in_org(
    pool: &SqlitePool,
    finding: DetectionFindingId,
    org_id: OrgId,
) -> DbResult<()> {
    let (ok,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM detection_findings f
            JOIN detection_rules r ON r.id = f.rule_id
            WHERE f.id = ? AND r.org_id = ?
        )",
    )
    .bind(finding.0.to_string())
    .bind(org_id.0.to_string())
    .fetch_one(pool)
    .await?;
    if ok != 0 {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn open_count(pool: &SqlitePool) -> DbResult<i64> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM detection_findings WHERE acknowledged_at IS NULL")
            .fetch_one(pool)
            .await?;
    Ok(n)
}

pub async fn fetch_since(
    pool: &SqlitePool,
    after: Option<OffsetDateTime>,
    limit: i64,
) -> DbResult<Vec<DetectionFinding>> {
    let after_u = after.map(|t| t.unix_timestamp());
    let sql = format!(
        "SELECT {FCOLS} FROM detection_findings
         WHERE ? IS NULL OR created_at > ?
         ORDER BY created_at ASC LIMIT ?"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(after_u)
        .bind(after_u)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(finding_from).collect())
}

pub async fn ack_finding(pool: &SqlitePool, id: DetectionFindingId) -> DbResult<DetectionFinding> {
    let sql = format!(
        "UPDATE detection_findings
         SET acknowledged_at = COALESCE(acknowledged_at, unixepoch())
         WHERE id = ?
         RETURNING {FCOLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    Ok(finding_from(&row))
}

/// Flat age-based retention prune: drop detection_findings older than `days`.
/// Returns rows deleted.
pub async fn prune(pool: &SqlitePool, days: i32) -> DbResult<u64> {
    let cutoff = time::OffsetDateTime::now_utc().unix_timestamp() - days.max(0) as i64 * 86400;
    let res = sqlx::query("DELETE FROM detection_findings WHERE created_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::logs;
    use rampart_core::detection::DetectionSeverity;
    use rampart_core::log::ParsedLog;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_rule() -> NewDetectionRule {
        NewDetectionRule {
            name: "rule".into(),
            description: String::new(),
            enabled: true,
            severity: DetectionSeverity::High,
            service: String::new(),
            min_level: 0,
            body_regex: String::new(),
            attr_key: String::new(),
            attr_val: String::new(),
            threshold: 2,
            window_seconds: 3600,
            cooldown_seconds: 0,
            group_by: String::new(),
            condition: None,
            channel_ids: vec![],
            escalation_policy_id: None,
        }
    }

    fn mk(body: &str, attrs: serde_json::Value) -> ParsedLog {
        ParsedLog {
            time_ns: 0,
            severity: 9,
            severity_text: None,
            service_name: "api".into(),
            body: body.into(),
            trace_id: None,
            span_id: None,
            attributes: attrs,
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn crud_and_whole_set_fire(pool: SqlitePool) {
        let org = super::oid(DEF);
        let mut input = new_rule(); // threshold 2, severity High
        input.body_regex = "denied".into();
        let r = create(&pool, input, org).await.unwrap();
        assert_eq!(r.severity, DetectionSeverity::High);
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        // a high-threshold twin over the same logs must NOT fire — proves gating.
        let mut hi = new_rule();
        hi.name = "hi".into();
        hi.threshold = 99;
        hi.body_regex = "denied".into();
        create(&pool, hi, org).await.unwrap();

        // Insert logs first, then evaluate once: the first tick has no watermark
        // so it counts the rule's lookback window (no sub-second collision with a
        // prior tick's unixepoch() watermark). 3 matches + 1 non-match.
        logs::insert_logs(
            &pool,
            &[
                mk("access denied", serde_json::json!({})),
                mk("denied again", serde_json::json!({})),
                mk("denied once more", serde_json::json!({})),
                mk("all good", serde_json::json!({})),
            ],
            org,
        )
        .await
        .unwrap();
        let fired = evaluate_tick(&pool).await.unwrap();
        assert_eq!(fired.len(), 1, "only the threshold-2 rule fires");
        assert_eq!(fired[0].finding.rule_id, r.id);
        assert_eq!(fired[0].finding.match_count, 3);
        assert!(fired[0].finding.sample.is_some());
        assert_eq!(open_count(&pool).await.unwrap(), 1);

        // findings feed + org-scoped feed + ack.
        let feed = list_findings(&pool, 10, true).await.unwrap();
        assert_eq!(feed.len(), 1);
        let org_feed = list_findings_for_org(&pool, 10, true, org).await.unwrap();
        assert_eq!(org_feed.len(), 1);
        let fid = feed[0].id;
        finding_in_org(&pool, fid, org).await.unwrap();
        ack_finding(&pool, fid).await.unwrap();
        assert_eq!(open_count(&pool).await.unwrap(), 0);
        assert_eq!(list_findings(&pool, 10, true).await.unwrap().len(), 0);

        // cross-org isolation.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, r.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            finding_in_org(&pool, fid, other.id).await,
            Err(DbError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn group_by_and_condition_tree(pool: SqlitePool) {
        let org = super::oid(DEF);

        // group_by rule: one finding per offending user (threshold 2).
        let mut g = new_rule();
        g.group_by = "user".into();
        g.threshold = 2;
        let gr = create(&pool, g, org).await.unwrap();
        logs::insert_logs(
            &pool,
            &[
                mk("x", serde_json::json!({ "user": "alice" })),
                mk("y", serde_json::json!({ "user": "alice" })),
                mk("z", serde_json::json!({ "user": "bob" })), // only 1 → below threshold
                mk("no-entity", serde_json::json!({})),        // missing group_by → ignored
            ],
            org,
        )
        .await
        .unwrap();
        let fired = evaluate_tick(&pool).await.unwrap();
        assert_eq!(fired.len(), 1, "only alice reaches threshold");
        assert_eq!(fired[0].finding.entity.as_deref(), Some("alice"));
        assert_eq!(fired[0].finding.match_count, 2);
        delete(&pool, gr.id, org).await.unwrap();

        // condition tree: (BodyContains "fail" AND attr env=prod), threshold 1.
        let mut c = new_rule();
        c.threshold = 1;
        c.condition = Some(DetectionCondition::And {
            conditions: vec![
                DetectionCondition::BodyContains {
                    value: "fail".into(),
                },
                DetectionCondition::Attr {
                    key: "env".into(),
                    value: "prod".into(),
                },
            ],
        });
        create(&pool, c, org).await.unwrap();
        logs::insert_logs(
            &pool,
            &[
                mk("login fail", serde_json::json!({ "env": "prod" })), // match
                mk("login fail", serde_json::json!({ "env": "dev" })),  // wrong env
                mk("login ok", serde_json::json!({ "env": "prod" })),   // wrong body
            ],
            org,
        )
        .await
        .unwrap();
        let fired = evaluate_tick(&pool).await.unwrap();
        assert_eq!(fired.len(), 1, "only the prod failure matches the tree");
        assert_eq!(fired[0].finding.match_count, 1);
    }
}
