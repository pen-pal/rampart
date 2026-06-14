//! Detection-rule IO: CRUD over `detection_rules`, the evaluation tick that
//! matches new log records and raises `detection_findings`, and the findings
//! feed (list / acknowledge). See `rampart_core::detection` for the types and
//! docs/design/DETECTION.md for the model.

use crate::{DbError, DbPool, DbResult};
use rampart_core::detection::{
    DetectionFinding, DetectionRule, DetectionSeverity, NewDetectionRule, UpdateDetectionRule,
};
use rampart_core::ids::{DetectionFindingId, DetectionRuleId, NotificationId};
use time::OffsetDateTime;
use uuid::Uuid;

struct RuleRow {
    id: Uuid,
    name: String,
    description: String,
    enabled: bool,
    severity: String,
    service: String,
    min_level: i16,
    body_regex: String,
    threshold: i32,
    window_seconds: i32,
    channel_ids: Vec<Uuid>,
    last_checked_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

impl From<RuleRow> for DetectionRule {
    fn from(r: RuleRow) -> Self {
        DetectionRule {
            id: DetectionRuleId::from_uuid(r.id),
            name: r.name,
            description: r.description,
            enabled: r.enabled,
            severity: DetectionSeverity::from_db(&r.severity),
            service: r.service,
            min_level: r.min_level,
            body_regex: r.body_regex,
            threshold: r.threshold,
            window_seconds: r.window_seconds,
            channel_ids: r
                .channel_ids
                .into_iter()
                .map(NotificationId::from_uuid)
                .collect(),
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

pub async fn list(pool: &DbPool) -> DbResult<Vec<DetectionRule>> {
    let rows = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, name, description, enabled, severity, service, min_level,
               body_regex, threshold, window_seconds,
               channel_ids AS "channel_ids!", last_checked_at, created_at
        FROM detection_rules
        ORDER BY created_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get(pool: &DbPool, id: DetectionRuleId) -> DbResult<DetectionRule> {
    let row = sqlx::query_as!(
        RuleRow,
        r#"
        SELECT id, name, description, enabled, severity, service, min_level,
               body_regex, threshold, window_seconds,
               channel_ids AS "channel_ids!", last_checked_at, created_at
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

pub async fn create(pool: &DbPool, input: NewDetectionRule) -> DbResult<DetectionRule> {
    let id = DetectionRuleId::new();
    let channel_ids: Vec<Uuid> = input.channel_ids.iter().map(|c| c.0).collect();
    sqlx::query!(
        r#"
        INSERT INTO detection_rules
            (id, name, description, enabled, severity, service, min_level,
             body_regex, threshold, window_seconds, channel_ids)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
        id.0,
        input.name,
        input.description,
        input.enabled,
        input.severity.as_str(),
        input.service,
        input.min_level,
        input.body_regex,
        input.threshold,
        input.window_seconds,
        &channel_ids,
    )
    .execute(pool)
    .await?;
    get(pool, id).await
}

pub async fn update(
    pool: &DbPool,
    id: DetectionRuleId,
    patch: UpdateDetectionRule,
) -> DbResult<DetectionRule> {
    let channel_ids: Option<Vec<Uuid>> = patch.channel_ids.map(|v| v.iter().map(|c| c.0).collect());
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
            threshold      = COALESCE($9, threshold),
            window_seconds = COALESCE($10, window_seconds),
            channel_ids    = COALESCE($11, channel_ids)
        WHERE id = $1
        "#,
        id.0,
        patch.name,
        patch.description,
        patch.enabled,
        patch.severity.map(|s| s.as_str().to_string()),
        patch.service,
        patch.min_level,
        patch.body_regex,
        patch.threshold,
        patch.window_seconds,
        channel_ids.as_deref(),
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get(pool, id).await
}

pub async fn delete(pool: &DbPool, id: DetectionRuleId) -> DbResult<()> {
    let result = sqlx::query!("DELETE FROM detection_rules WHERE id = $1", id.0)
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
pub async fn preview(
    pool: &DbPool,
    service: &str,
    min_level: i16,
    body_regex: &str,
    window_seconds: i32,
) -> DbResult<PreviewResult> {
    let window = window_seconds as f64;
    let count = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM logs
        WHERE ts >= now() - make_interval(secs => $1)
          AND ($2 = '' OR service_name = $2)
          AND severity >= $3
          AND ($4 = '' OR body ~* $4)
        "#,
        window,
        service,
        min_level,
        body_regex,
    )
    .fetch_one(pool)
    .await?;

    let samples = sqlx::query_scalar!(
        r#"
        SELECT LEFT(body, 300) AS "body!"
        FROM logs
        WHERE ts >= now() - make_interval(secs => $1)
          AND ($2 = '' OR service_name = $2)
          AND severity >= $3
          AND ($4 = '' OR body ~* $4)
        ORDER BY ts DESC
        LIMIT 5
        "#,
        window,
        service,
        min_level,
        body_regex,
    )
    .fetch_all(pool)
    .await?;

    Ok(PreviewResult { count, samples })
}

/// A finding raised this tick, paired with the channels to notify.
pub struct FindingEvent {
    pub finding: DetectionFinding,
    pub channel_ids: Vec<NotificationId>,
}

/// Evaluate every enabled rule once. For each, count log records matching the
/// rule spec with `ts` in `(last_checked_at, now]` (or the rule's lookback
/// window on first run); when the count reaches `threshold`, insert a finding
/// and queue a notification. The watermark advances to `now` on every rule
/// regardless of outcome, so matches are never counted twice.
pub async fn evaluate_tick(pool: &DbPool) -> DbResult<Vec<FindingEvent>> {
    let mut out = Vec::new();

    for rule in list(pool).await?.into_iter().filter(|r| r.enabled) {
        // Compute the window bounds and the match count in one statement so
        // both ends use the DB clock — mixing an app-side `now` with
        // DB-generated `ts` would skew the boundary. `wfrom` is the rule's
        // watermark, or `now - window` on first run.
        let row = sqlx::query!(
            r#"
            WITH bounds AS (
                SELECT COALESCE($1::timestamptz, now() - make_interval(secs => $2)) AS wfrom,
                       now() AS wto
            )
            SELECT b.wfrom AS "wfrom!", b.wto AS "wto!", COUNT(l.id) AS "cnt!"
            FROM bounds b
            LEFT JOIN logs l
              ON l.ts > b.wfrom AND l.ts <= b.wto
             AND ($3 = '' OR l.service_name = $3)
             AND l.severity >= $4
             AND ($5 = '' OR l.body ~* $5)
            GROUP BY b.wfrom, b.wto
            "#,
            rule.last_checked_at,
            rule.window_seconds as f64,
            rule.service,
            rule.min_level,
            rule.body_regex,
        )
        .fetch_one(pool)
        .await?;
        let (wfrom, wto, count) = (row.wfrom, row.wto, row.cnt);

        if count >= rule.threshold as i64 {
            // Newest matching body as the analyst's sample (truncated).
            let sample = sqlx::query_scalar!(
                r#"
                SELECT LEFT(body, 500)
                FROM logs
                WHERE ts > $1 AND ts <= $2
                  AND ($3 = '' OR service_name = $3)
                  AND severity >= $4
                  AND ($5 = '' OR body ~* $5)
                ORDER BY ts DESC
                LIMIT 1
                "#,
                wfrom,
                wto,
                rule.service,
                rule.min_level,
                rule.body_regex,
            )
            .fetch_optional(pool)
            .await?
            .flatten();

            let service = (!rule.service.is_empty()).then(|| rule.service.clone());
            let fid = DetectionFindingId::new();
            let frow = sqlx::query_as!(
                FindingRow,
                r#"
                INSERT INTO detection_findings
                    (id, rule_id, rule_name, severity, match_count, sample,
                     service, window_from, window_to)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                RETURNING id, rule_id, rule_name, severity, match_count, sample,
                          service, window_from, window_to, created_at,
                          acknowledged_at
                "#,
                fid.0,
                rule.id.0,
                rule.name,
                rule.severity.as_str(),
                count,
                sample,
                service,
                wfrom,
                wto,
            )
            .fetch_one(pool)
            .await?;

            out.push(FindingEvent {
                finding: frow.into(),
                channel_ids: rule.channel_ids.clone(),
            });
        }

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
        SELECT id, rule_id, rule_name, severity, match_count, sample, service,
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

/// Count of unacknowledged findings — drives the nav badge.
pub async fn open_count(pool: &DbPool) -> DbResult<i64> {
    let n = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM detection_findings WHERE acknowledged_at IS NULL"#,
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Acknowledge one finding (idempotent — re-acking keeps the first timestamp).
pub async fn ack_finding(pool: &DbPool, id: DetectionFindingId) -> DbResult<DetectionFinding> {
    let row = sqlx::query_as!(
        FindingRow,
        r#"
        UPDATE detection_findings
        SET acknowledged_at = COALESCE(acknowledged_at, now())
        WHERE id = $1
        RETURNING id, rule_id, rule_name, severity, match_count, sample, service,
                  window_from, window_to, created_at, acknowledged_at
        "#,
        id.0,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(row.into())
}
