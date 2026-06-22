//! Detection-rule IO: CRUD over `detection_rules`, the evaluation tick that
//! matches new log records and raises `detection_findings`, and the findings
//! feed (list / acknowledge). See `rampart_core::detection` for the types and
//! docs/design/DETECTION.md for the model.

use crate::{DbError, DbPool, DbResult};
use rampart_core::detection::{
    DetectionCondition, DetectionFinding, DetectionRule, DetectionSeverity, NewDetectionRule,
    UpdateDetectionRule,
};
use rampart_core::ids::{DetectionFindingId, DetectionRuleId, NotificationId, OrgId};
use sqlx::{Postgres, QueryBuilder, Row};
use time::OffsetDateTime;
use uuid::Uuid;

struct RuleRow {
    id: Uuid,
    org_id: Uuid,
    name: String,
    description: String,
    enabled: bool,
    severity: String,
    service: String,
    min_level: i16,
    body_regex: String,
    attr_key: String,
    attr_val: String,
    threshold: i32,
    window_seconds: i32,
    cooldown_seconds: i32,
    group_by: String,
    condition: Option<serde_json::Value>,
    channel_ids: Vec<Uuid>,
    escalation_policy_id: Option<Uuid>,
    last_checked_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

impl From<RuleRow> for DetectionRule {
    fn from(r: RuleRow) -> Self {
        DetectionRule {
            id: DetectionRuleId::from_uuid(r.id),
            org_id: OrgId::from_uuid(r.org_id),
            name: r.name,
            description: r.description,
            enabled: r.enabled,
            severity: DetectionSeverity::from_db(&r.severity),
            service: r.service,
            min_level: r.min_level,
            body_regex: r.body_regex,
            attr_key: r.attr_key,
            attr_val: r.attr_val,
            threshold: r.threshold,
            window_seconds: r.window_seconds,
            cooldown_seconds: r.cooldown_seconds,
            group_by: r.group_by,
            condition: r.condition.and_then(|v| serde_json::from_value(v).ok()),
            channel_ids: r
                .channel_ids
                .into_iter()
                .map(NotificationId::from_uuid)
                .collect(),
            escalation_policy_id: r
                .escalation_policy_id
                .map(rampart_core::ids::EscalationPolicyId::from_uuid),
            last_checked_at: r.last_checked_at,
            created_at: r.created_at,
        }
    }
}

struct FindingRow {
    id: Uuid,
    rule_id: Uuid,
    rule_name: String,
    severity: String,
    match_count: i64,
    sample: Option<String>,
    service: Option<String>,
    entity: Option<String>,
    window_from: OffsetDateTime,
    window_to: OffsetDateTime,
    created_at: OffsetDateTime,
    acknowledged_at: Option<OffsetDateTime>,
}

impl From<FindingRow> for DetectionFinding {
    fn from(r: FindingRow) -> Self {
        DetectionFinding {
            id: DetectionFindingId::from_uuid(r.id),
            rule_id: DetectionRuleId::from_uuid(r.rule_id),
            rule_name: r.rule_name,
            severity: DetectionSeverity::from_db(&r.severity),
            match_count: r.match_count,
            sample: r.sample,
            service: r.service,
            entity: r.entity,
            window_from: r.window_from,
            window_to: r.window_to,
            created_at: r.created_at,
            acknowledged_at: r.acknowledged_at,
        }
    }
}

/// Ask Postgres whether `pattern` is a valid `~*` regex (empty = always valid).
/// Routes call this before create/update so a bad pattern is a 400, not a
/// per-tick evaluation error. Returns Ok(false) only for the invalid-regex
/// error code (`2201B`); other DB errors propagate.
pub async fn regex_is_valid(pool: &DbPool, pattern: &str) -> DbResult<bool> {
    if pattern.is_empty() {
        return Ok(true);
    }
    match sqlx::query_scalar::<_, bool>("SELECT 'x' ~* $1::text")
        .bind(pattern)
        .fetch_one(pool)
        .await
    {
        Ok(_) => Ok(true),
        Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("2201B") => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Detection rules owned by one org — the management list (org-scoped).
pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<DetectionRule>> {
    let rows = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, org_id AS "org_id!", name, description, enabled, severity, service, min_level,
               body_regex, attr_key, attr_val, threshold, window_seconds, cooldown_seconds,
               group_by, condition,
               channel_ids AS "channel_ids!", escalation_policy_id, last_checked_at, created_at
        FROM detection_rules
        WHERE org_id = $1
        ORDER BY created_at
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Every detection rule across ALL orgs — for the scheduler evaluation tick.
/// Not org-scoped; never call on a request path (use [`list`]).
pub async fn list_all(pool: &DbPool) -> DbResult<Vec<DetectionRule>> {
    let rows = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, org_id AS "org_id!", name, description, enabled, severity, service, min_level,
               body_regex, attr_key, attr_val, threshold, window_seconds, cooldown_seconds,
               group_by, condition,
               channel_ids AS "channel_ids!", escalation_policy_id, last_checked_at, created_at
        FROM detection_rules
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Fetch one rule scoped to an org (cross-org id → NotFound).
pub async fn get(pool: &DbPool, id: DetectionRuleId, org_id: OrgId) -> DbResult<DetectionRule> {
    let row = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, org_id AS "org_id!", name, description, enabled, severity, service, min_level,
               body_regex, attr_key, attr_val, threshold, window_seconds, cooldown_seconds,
               group_by, condition,
               channel_ids AS "channel_ids!", escalation_policy_id, last_checked_at, created_at
        FROM detection_rules
        WHERE id = $1 AND org_id = $2
        "#,
        id.0,
        org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

/// Fetch one rule WITHOUT org scoping — for the scheduler and the
/// just-inserted-row return in [`create`]. Not for request paths.
pub async fn get_unscoped(pool: &DbPool, id: DetectionRuleId) -> DbResult<DetectionRule> {
    let row = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, org_id AS "org_id!", name, description, enabled, severity, service, min_level,
               body_regex, attr_key, attr_val, threshold, window_seconds, cooldown_seconds,
               group_by, condition,
               channel_ids AS "channel_ids!", escalation_policy_id, last_checked_at, created_at
        FROM detection_rules
        WHERE id = $1
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}

pub async fn create(
    pool: &DbPool,
    input: NewDetectionRule,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<DetectionRule> {
    let id = DetectionRuleId::new();
    let channel_ids: Vec<Uuid> = input.channel_ids.iter().map(|c| c.0).collect();
    // Serializing our own enum can't fail; `None` stores SQL NULL (legacy match).
    let condition_json: Option<serde_json::Value> =
        input.condition.as_ref().and_then(|c| serde_json::to_value(c).ok());
    sqlx::query!(
        r#"
        INSERT INTO detection_rules
            (id, name, description, enabled, severity, service, min_level,
             body_regex, attr_key, attr_val, threshold, window_seconds, channel_ids,
             escalation_policy_id, cooldown_seconds, condition, group_by, org_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        "#,
        id.0,
        input.name,
        input.description,
        input.enabled,
        input.severity.as_str(),
        input.service,
        input.min_level,
        input.body_regex,
        input.attr_key,
        input.attr_val,
        input.threshold,
        input.window_seconds,
        &channel_ids,
        input.escalation_policy_id.map(|p| p.0),
        input.cooldown_seconds,
        condition_json,
        input.group_by,
        org_id.0,
    )
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: DetectionRuleId,
    patch: UpdateDetectionRule,
    org_id: OrgId,
) -> DbResult<DetectionRule> {
    let channel_ids: Option<Vec<Uuid>> = patch.channel_ids.map(|v| v.iter().map(|c| c.0).collect());
    let condition_json: Option<serde_json::Value> =
        patch.condition.as_ref().and_then(|c| serde_json::to_value(c).ok());
    let result = sqlx::query!(
        r#"
        UPDATE detection_rules SET
            name           = COALESCE($2, name),
            description    = COALESCE($3, description),
            enabled        = COALESCE($4, enabled),
            severity       = COALESCE($5, severity),
            service        = COALESCE($6, service),
            min_level      = COALESCE($7, min_level),
            body_regex     = COALESCE($8, body_regex),
            attr_key       = COALESCE($9, attr_key),
            attr_val       = COALESCE($10, attr_val),
            threshold      = COALESCE($11, threshold),
            window_seconds = COALESCE($12, window_seconds),
            channel_ids    = COALESCE($13, channel_ids),
            escalation_policy_id = COALESCE($14, escalation_policy_id),
            cooldown_seconds = COALESCE($15, cooldown_seconds),
            -- Direct set (not COALESCE): the rule form is the sole updater and
            -- always sends the full intended condition (tree, or null to clear
            -- when switched back to the flat matcher).
            condition      = $16,
            group_by       = COALESCE($17, group_by)
        WHERE id = $1 AND org_id = $18
        "#,
        id.0,
        patch.name,
        patch.description,
        patch.enabled,
        patch.severity.map(|s| s.as_str().to_string()),
        patch.service,
        patch.min_level,
        patch.body_regex,
        patch.attr_key,
        patch.attr_val,
        patch.threshold,
        patch.window_seconds,
        channel_ids.as_deref(),
        patch.escalation_policy_id.map(|p| p.0),
        patch.cooldown_seconds,
        condition_json,
        patch.group_by,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id, org_id).await
}

pub async fn delete(pool: &DbPool, id: DetectionRuleId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM detection_rules WHERE id = $1 AND org_id = $2",
        id.0,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

/// Dry-run match of a rule spec over recent logs, for authoring. Counts and
/// samples without writing a finding or moving any watermark.
#[derive(serde::Serialize)]
pub struct PreviewResult {
    pub count: i64,
    pub samples: Vec<String>,
}

/// Run a rule's match spec over the trailing `window_seconds` of logs without
/// persisting anything — the "test this rule" path. Caller validates
/// `body_regex` first (see [`regex_is_valid`]) so an invalid pattern is a 400.
#[allow(clippy::too_many_arguments)]
pub async fn preview(
    pool: &DbPool,
    service: &str,
    min_level: i16,
    body_regex: &str,
    attr_key: &str,
    attr_val: &str,
    window_seconds: i32,
    org_id: OrgId,
) -> DbResult<PreviewResult> {
    let window = window_seconds as f64;
    let count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM logs
        WHERE received_at >= now() - make_interval(secs => $1)
          AND ($2 = '' OR service_name = $2)
          AND severity >= $3
          AND ($4 = '' OR body ~* $4)
          AND ($5 = '' OR attributes->>$5 = $6)
          AND org_id = $7
        "#,
        window,
        service,
        min_level,
        body_regex,
        attr_key,
        attr_val,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;

    let samples = sqlx::query_scalar!(
        r#"
        SELECT LEFT(body, 300) AS "body!"
        FROM logs
        WHERE received_at >= now() - make_interval(secs => $1)
          AND ($2 = '' OR service_name = $2)
          AND severity >= $3
          AND ($4 = '' OR body ~* $4)
          AND ($5 = '' OR attributes->>$5 = $6)
          AND org_id = $7
        ORDER BY received_at DESC
        LIMIT 5
        "#,
        window,
        service,
        min_level,
        body_regex,
        attr_key,
        attr_val,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;

    Ok(PreviewResult { count, samples })
}

/// A finding raised this tick, paired with the channels to notify.
pub struct FindingEvent {
    pub finding: DetectionFinding,
    pub channel_ids: Vec<NotificationId>,
    pub escalation_policy_id: Option<rampart_core::ids::EscalationPolicyId>,
}

/// Evaluate every enabled rule once. For each, count log records matching the
/// rule spec with `received_at` in `(last_checked_at, now]` (or the rule's
/// lookback window on first run) — the server ingest clock, so a client that
/// backdates its event `ts` can't slip records out of the detection window;
/// when the count reaches `threshold`, insert a finding
/// and queue a notification. The watermark advances to `now` on every rule
/// regardless of outcome, so matches are never counted twice.
pub async fn evaluate_tick(pool: &DbPool) -> DbResult<Vec<FindingEvent>> {
    let mut out = Vec::new();

    for rule in list_all(pool).await?.into_iter().filter(|r| r.enabled) {
        let cooldown = rule.cooldown_seconds as i64;
        let wto = if rule.group_by.is_empty() {
            // ── whole-match-set: one count, one finding ──────────────────────
            // Window bounds + count in one statement so both ends use the DB
            // clock. v2 rules carry a boolean `condition` tree compiled to a
            // parameterized WHERE; legacy rules use the flat-field match.
            let (wfrom, wto, count) = match &rule.condition {
                Some(cond) => tree_window(pool, &rule, cond).await?,
                None => legacy_window(pool, &rule).await?,
            };
            // Cooldown: keep advancing the watermark but don't re-raise.
            let suppressed = rule.cooldown_seconds > 0
                && has_recent_finding(pool, rule.id, cooldown, None).await?;
            if count >= rule.threshold as i64 && !suppressed {
                let sample = match &rule.condition {
                    Some(cond) => tree_sample(pool, rule.org_id, wfrom, wto, cond).await?,
                    None => legacy_sample(pool, &rule, wfrom, wto).await?,
                };
                let service = (!rule.service.is_empty()).then(|| rule.service.clone());
                let frow =
                    insert_finding(pool, &rule, count, sample, service, None, wfrom, wto).await?;
                out.push(FindingEvent {
                    finding: frow.into(),
                    channel_ids: rule.channel_ids.clone(),
                    escalation_policy_id: rule.escalation_policy_id,
                });
            }
            wto
        } else {
            // ── per-entity (group_by): one finding per offending entity ──────
            let (wfrom, wto) = compute_bounds(pool, &rule).await?;
            for (entity, count, sample) in grouped_eval(pool, &rule, wfrom, wto).await? {
                // Per-ENTITY cooldown: a noisy entity doesn't mute the others.
                if rule.cooldown_seconds > 0
                    && has_recent_finding(pool, rule.id, cooldown, Some(&entity)).await?
                {
                    continue;
                }
                let frow =
                    insert_finding(pool, &rule, count, sample, None, Some(entity), wfrom, wto)
                        .await?;
                out.push(FindingEvent {
                    finding: frow.into(),
                    channel_ids: rule.channel_ids.clone(),
                    escalation_policy_id: rule.escalation_policy_id,
                });
            }
            wto
        };

        sqlx::query!(
            "UPDATE detection_rules SET last_checked_at = $2 WHERE id = $1",
            rule.id.0,
            wto,
        )
        .execute(pool)
        .await?;
    }
    Ok(out)
}

/// Insert one finding row (shared by the whole-set and per-entity paths).
#[allow(clippy::too_many_arguments)]
async fn insert_finding(
    pool: &DbPool,
    rule: &DetectionRule,
    count: i64,
    sample: Option<String>,
    service: Option<String>,
    entity: Option<String>,
    wfrom: OffsetDateTime,
    wto: OffsetDateTime,
) -> DbResult<FindingRow> {
    let fid = DetectionFindingId::new();
    Ok(sqlx::query_as!(
        FindingRow,
        r#"
        INSERT INTO detection_findings
            (id, rule_id, rule_name, severity, match_count, sample,
             service, entity, window_from, window_to)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING id, rule_id, rule_name, severity, match_count, sample,
                  service, entity, window_from, window_to, created_at,
                  acknowledged_at
        "#,
        fid.0,
        rule.id.0,
        rule.name,
        rule.severity.as_str(),
        count,
        sample,
        service,
        entity,
        wfrom,
        wto,
    )
    .fetch_one(pool)
    .await?)
}

// ── match evaluation ────────────────────────────────────────────────────────
// Two paths share the (wfrom, wto, count) + newest-sample shape: the legacy
// flat-field match (compile-checked queries) and the Detection v2 boolean tree
// (compiled to a parameterized WHERE via QueryBuilder — runtime-checked, so it
// can't be a `query!` macro).

/// Legacy flat-field window + count.
async fn legacy_window(
    pool: &DbPool,
    rule: &DetectionRule,
) -> DbResult<(OffsetDateTime, OffsetDateTime, i64)> {
    let row = sqlx::query!(
        r#"
        WITH bounds AS (
            SELECT COALESCE($1::timestamptz, now() - make_interval(secs => $2)) AS wfrom,
                   now() AS wto
        )
        SELECT b.wfrom AS "wfrom!", b.wto AS "wto!", COUNT(l.id) AS "cnt!"
        FROM bounds b
        LEFT JOIN logs l
          ON l.received_at > b.wfrom AND l.received_at <= b.wto
         AND ($3 = '' OR l.service_name = $3)
         AND l.severity >= $4
         AND ($5 = '' OR l.body ~* $5)
         AND ($6 = '' OR l.attributes->>$6 = $7)
         AND l.org_id = $8
        GROUP BY b.wfrom, b.wto
        "#,
        rule.last_checked_at,
        rule.window_seconds as f64,
        rule.service,
        rule.min_level,
        rule.body_regex,
        rule.attr_key,
        rule.attr_val,
        rule.org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.wfrom, row.wto, row.cnt))
}

/// Legacy flat-field newest sample.
async fn legacy_sample(
    pool: &DbPool,
    rule: &DetectionRule,
    wfrom: OffsetDateTime,
    wto: OffsetDateTime,
) -> DbResult<Option<String>> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT LEFT(body, 500)
        FROM logs
        WHERE received_at > $1 AND received_at <= $2
          AND ($3 = '' OR service_name = $3)
          AND severity >= $4
          AND ($5 = '' OR body ~* $5)
          AND ($6 = '' OR attributes->>$6 = $7)
          AND org_id = $8
        ORDER BY received_at DESC
        LIMIT 1
        "#,
        wfrom,
        wto,
        rule.service,
        rule.min_level,
        rule.body_regex,
        rule.attr_key,
        rule.attr_val,
        rule.org_id.0,
    )
    .fetch_optional(pool)
    .await?
    .flatten())
}

/// Boolean-tree window + count (Detection v2).
async fn tree_window(
    pool: &DbPool,
    rule: &DetectionRule,
    cond: &DetectionCondition,
) -> DbResult<(OffsetDateTime, OffsetDateTime, i64)> {
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new("WITH bounds AS (SELECT COALESCE(");
    qb.push_bind(rule.last_checked_at);
    qb.push("::timestamptz, now() - make_interval(secs => ");
    qb.push_bind(rule.window_seconds as f64);
    qb.push(")) AS wfrom, now() AS wto) SELECT b.wfrom AS wfrom, b.wto AS wto, COUNT(l.id) AS cnt FROM bounds b LEFT JOIN logs l ON l.received_at > b.wfrom AND l.received_at <= b.wto AND l.org_id = ");
    qb.push_bind(rule.org_id.0);
    qb.push(" AND (");
    push_condition(&mut qb, cond);
    qb.push(") GROUP BY b.wfrom, b.wto");
    let row = qb.build().fetch_one(pool).await?;
    Ok((row.try_get("wfrom")?, row.try_get("wto")?, row.try_get("cnt")?))
}

/// Boolean-tree newest sample (Detection v2).
async fn tree_sample(
    pool: &DbPool,
    org_id: OrgId,
    wfrom: OffsetDateTime,
    wto: OffsetDateTime,
    cond: &DetectionCondition,
) -> DbResult<Option<String>> {
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT LEFT(l.body, 500) AS sample FROM logs l WHERE l.received_at > ");
    qb.push_bind(wfrom);
    qb.push(" AND l.received_at <= ");
    qb.push_bind(wto);
    qb.push(" AND l.org_id = ");
    qb.push_bind(org_id.0);
    qb.push(" AND (");
    push_condition(&mut qb, cond);
    qb.push(") ORDER BY l.received_at DESC LIMIT 1");
    let row = qb.build().fetch_optional(pool).await?;
    Ok(row.and_then(|r| r.try_get::<Option<String>, _>("sample").ok().flatten()))
}

/// Window bounds on the DB clock (used by the per-entity path, which can't fold
/// bounds + the grouped count into one fetch_one the way the whole-set path does).
async fn compute_bounds(
    pool: &DbPool,
    rule: &DetectionRule,
) -> DbResult<(OffsetDateTime, OffsetDateTime)> {
    let row = sqlx::query!(
        r#"SELECT COALESCE($1::timestamptz, now() - make_interval(secs => $2)) AS "wfrom!",
                  now() AS "wto!""#,
        rule.last_checked_at,
        rule.window_seconds as f64,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.wfrom, row.wto))
}

/// Per-entity grouped evaluation: count matches per `attributes->>group_by`
/// within the window (honoring the same flat/boolean match), and return each
/// entity reaching the threshold with its newest sample. Records lacking the
/// group-by attribute are ignored.
async fn grouped_eval(
    pool: &DbPool,
    rule: &DetectionRule,
    wfrom: OffsetDateTime,
    wto: OffsetDateTime,
) -> DbResult<Vec<(String, i64, Option<String>)>> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT l.attributes->>");
    qb.push_bind(rule.group_by.clone());
    qb.push(" AS entity, COUNT(*) AS cnt, (array_agg(LEFT(l.body, 500) ORDER BY l.received_at DESC))[1] AS sample FROM logs l WHERE l.received_at > ");
    qb.push_bind(wfrom);
    qb.push(" AND l.received_at <= ");
    qb.push_bind(wto);
    qb.push(" AND l.org_id = ");
    qb.push_bind(rule.org_id.0);
    qb.push(" AND l.attributes->>");
    qb.push_bind(rule.group_by.clone());
    qb.push(" IS NOT NULL AND (");
    push_where(&mut qb, rule);
    qb.push(") GROUP BY entity HAVING COUNT(*) >= ");
    qb.push_bind(rule.threshold as i64);
    let rows = qb.build().fetch_all(pool).await?;
    rows.into_iter()
        .map(|r| {
            Ok((
                r.try_get::<Option<String>, _>("entity")?.unwrap_or_default(),
                r.try_get::<i64, _>("cnt")?,
                r.try_get::<Option<String>, _>("sample")?,
            ))
        })
        .collect()
}

/// Push the rule's match predicate (boolean tree if present, else the flat
/// fields) into a running query builder.
fn push_where(qb: &mut QueryBuilder<Postgres>, rule: &DetectionRule) {
    match &rule.condition {
        Some(c) => push_condition(qb, c),
        None => push_flat(qb, rule),
    }
}

/// The legacy flat-field match as bound SQL (mirrors `legacy_window`'s WHERE),
/// for the grouped path which can't use the static `query!` macro.
fn push_flat(qb: &mut QueryBuilder<Postgres>, rule: &DetectionRule) {
    qb.push("(");
    qb.push_bind(rule.service.clone());
    qb.push(" = '' OR l.service_name = ");
    qb.push_bind(rule.service.clone());
    qb.push(") AND l.severity >= ");
    qb.push_bind(rule.min_level);
    qb.push(" AND (");
    qb.push_bind(rule.body_regex.clone());
    qb.push(" = '' OR l.body ~* ");
    qb.push_bind(rule.body_regex.clone());
    qb.push(") AND (");
    qb.push_bind(rule.attr_key.clone());
    qb.push(" = '' OR l.attributes->>");
    qb.push_bind(rule.attr_key.clone());
    qb.push(" = ");
    qb.push_bind(rule.attr_val.clone());
    qb.push(")");
}

/// Escape LIKE wildcards in a user substring so `BodyContains` matches it
/// literally (the default `\` escape char applies).
fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Compile a condition node into the running `QueryBuilder`, binding every leaf
/// value as a parameter (no string interpolation of user input).
fn push_condition(qb: &mut QueryBuilder<Postgres>, c: &DetectionCondition) {
    match c {
        DetectionCondition::And { conditions } => push_join(qb, conditions, " AND ", "TRUE"),
        DetectionCondition::Or { conditions } => push_join(qb, conditions, " OR ", "FALSE"),
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
        DetectionCondition::BodyRegex { value } => {
            qb.push("l.body ~* ");
            qb.push_bind(value.clone());
        }
        DetectionCondition::BodyContains { value } => {
            qb.push("l.body ILIKE ");
            qb.push_bind(format!("%{}%", like_escape(value)));
        }
        DetectionCondition::Attr { key, value } => {
            // COALESCE so a MISSING attribute reads as "" rather than NULL:
            // `attr=val` is false for an absent key, and crucially `NOT attr=val`
            // is TRUE for an absent key — the intuitive detection semantics
            // (without COALESCE, NOT-of-NULL is NULL and silently drops the row).
            qb.push("COALESCE(l.attributes->>");
            qb.push_bind(key.clone());
            qb.push(", '') = ");
            qb.push_bind(value.clone());
        }
    }
}

fn push_join(
    qb: &mut QueryBuilder<Postgres>,
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

/// Did this rule raise any finding within the last `secs` seconds? Drives both
/// the cooldown suppression and the detection escalation auto-resolve (a quiet
/// rule means the threat stopped). When `entity` is `Some`, scope to that
/// entity's findings — per-entity cooldown for group_by rules.
pub async fn has_recent_finding(
    pool: &DbPool,
    rule_id: DetectionRuleId,
    secs: i64,
    entity: Option<&str>,
) -> DbResult<bool> {
    let exists = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM detection_findings
            WHERE rule_id = $1 AND created_at >= now() - make_interval(secs => $2)
              AND ($3::text IS NULL OR entity = $3)
        ) AS "exists!"
        "#,
        rule_id.0,
        secs as f64,
        entity,
    )
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Findings feed, newest first. `open_only` returns just the unacknowledged
/// ones (the triage queue).
pub async fn list_findings(
    pool: &DbPool,
    limit: i64,
    open_only: bool,
) -> DbResult<Vec<DetectionFinding>> {
    let rows = sqlx::query_as!(
        FindingRow,
        r#"
        SELECT id, rule_id, rule_name, severity, match_count, sample, service, entity,
               window_from, window_to, created_at, acknowledged_at
        FROM detection_findings
        WHERE NOT $2 OR acknowledged_at IS NULL
        ORDER BY created_at DESC
        LIMIT $1
        "#,
        limit,
        open_only,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Org-scoped twin of [`list_findings`] for the authenticated triage feed:
/// only findings whose owning rule belongs to `org_id` (every finding has a
/// NOT-NULL `rule_id`). The unscoped [`list_findings`] and the SIEM exporter
/// [`fetch_since`] stay as-is for system/test callers.
pub async fn list_findings_for_org(
    pool: &DbPool,
    limit: i64,
    open_only: bool,
    org_id: OrgId,
) -> DbResult<Vec<DetectionFinding>> {
    let rows = sqlx::query_as!(
        FindingRow,
        r#"
        SELECT f.id, f.rule_id, f.rule_name, f.severity, f.match_count, f.sample, f.service,
               f.entity, f.window_from, f.window_to, f.created_at, f.acknowledged_at
        FROM detection_findings f
        JOIN detection_rules r ON r.id = f.rule_id
        WHERE (NOT $2 OR f.acknowledged_at IS NULL) AND r.org_id = $3
        ORDER BY f.created_at DESC
        LIMIT $1
        "#,
        limit,
        open_only,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// 404-gate: succeed only when `finding`'s owning rule belongs to `org`. Used
/// by the ack-by-id handler.
pub async fn finding_in_org(
    pool: &DbPool,
    finding: DetectionFindingId,
    org_id: OrgId,
) -> DbResult<()> {
    let ok = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM detection_findings f
            JOIN detection_rules r ON r.id = f.rule_id
            WHERE f.id = $1 AND r.org_id = $2
        ) AS "exists!"
        "#,
        finding.0,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    if ok {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

/// Count of unacknowledged findings — drives the nav badge.
pub async fn open_count(pool: &DbPool) -> DbResult<i64> {
    let n = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM detection_findings WHERE acknowledged_at IS NULL"#,
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Forward tail for the SIEM exporter: findings with `created_at` strictly
/// after the cursor, oldest first, capped. `after = None` starts from the
/// beginning. Cursoring on `created_at` (not the UUID id) keeps the export in
/// emission order.
pub async fn fetch_since(
    pool: &DbPool,
    after: Option<OffsetDateTime>,
    limit: i64,
) -> DbResult<Vec<DetectionFinding>> {
    let rows = sqlx::query_as!(
        FindingRow,
        r#"
        SELECT id, rule_id, rule_name, severity, match_count, sample, service, entity,
               window_from, window_to, created_at, acknowledged_at
        FROM detection_findings
        WHERE $1::timestamptz IS NULL OR created_at > $1
        ORDER BY created_at ASC
        LIMIT $2
        "#,
        after,
        limit,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Acknowledge one finding (idempotent — re-acking keeps the first timestamp).
pub async fn ack_finding(pool: &DbPool, id: DetectionFindingId) -> DbResult<DetectionFinding> {
    let row = sqlx::query_as!(
        FindingRow,
        r#"
        UPDATE detection_findings
        SET acknowledged_at = COALESCE(acknowledged_at, now())
        WHERE id = $1
        RETURNING id, rule_id, rule_name, severity, match_count, sample, service, entity,
                  window_from, window_to, created_at, acknowledged_at
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}
