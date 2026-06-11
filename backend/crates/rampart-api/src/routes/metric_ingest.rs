//! External metric ingestion + reads.
//!
//! POST /v1/metrics/ingest — Pushgateway-style: the raw request body is
//! Prometheus text exposition format. Anything a shell script can emit
//! with two echos works:
//!
//!   echo "backup_duration_seconds 312.5" | \
//!     curl --data-binary @- -H "Authorization: Bearer rmp_…" \
//!       https://rampart.example.com/v1/metrics/ingest
//!
//! Client timestamps in the payload are ignored — samples are
//! server-stamped, the same trust stance as push pings and agent
//! reports. Mounted in the editor slice: API keys with `write` scope
//! (the automation path) and editor sessions may push; readonly keys
//! get GETs only.
//!
//! GET /v1/metrics/series — distinct (name, label-set) series.
//! GET /v1/metrics/query — bucketed range read for one series.

use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::Path;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rampart_core::ids::MetricRuleId;
use rampart_core::metric_rule::{MetricRule, NewMetricRule, UpdateMetricRule};
use rampart_core::promtext;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;
use validator::Validate;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ingest", post(ingest))
        .route("/series", get(series))
        .route("/query", get(query))
        .route("/rules", get(list_rules).post(create_rule))
        .route(
            "/rules/{id}",
            axum::routing::patch(update_rule).delete(delete_rule),
        )
}

fn parse_rule_id(s: &str) -> Result<MetricRuleId, ApiError> {
    Uuid::from_str(s)
        .map(MetricRuleId::from_uuid)
        .map_err(|_| ApiError::BadRequest("invalid rule id".into()))
}

async fn list_rules(State(s): State<AppState>) -> Result<Json<Vec<MetricRule>>, ApiError> {
    Ok(Json(rampart_db::metric_rules::list(s.pool()).await?))
}

async fn create_rule(
    State(s): State<AppState>,
    Json(input): Json<NewMetricRule>,
) -> Result<(axum::http::StatusCode, Json<MetricRule>), ApiError> {
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !input.labels.is_object() {
        return Err(ApiError::BadRequest("labels must be a JSON object".into()));
    }
    let rule = rampart_db::metric_rules::create(s.pool(), input).await?;
    Ok((axum::http::StatusCode::CREATED, Json(rule)))
}

async fn update_rule(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateMetricRule>,
) -> Result<Json<MetricRule>, ApiError> {
    let rule_id = parse_rule_id(&id)?;
    input
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if let Some(l) = &input.labels {
        if !l.is_object() {
            return Err(ApiError::BadRequest("labels must be a JSON object".into()));
        }
    }
    Ok(Json(
        rampart_db::metric_rules::update(s.pool(), rule_id, input).await?,
    ))
}

async fn delete_rule(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let rule_id = parse_rule_id(&id)?;
    rampart_db::metric_rules::delete(s.pool(), rule_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct IngestOutcome {
    accepted: usize,
    skipped: usize,
}

async fn ingest(State(s): State<AppState>, body: String) -> Result<Json<IngestOutcome>, ApiError> {
    let parsed = promtext::parse(&body);
    if parsed.samples.is_empty() && parsed.skipped > 0 {
        return Err(ApiError::BadRequest(
            "no parseable samples in payload (expected Prometheus text format)".into(),
        ));
    }
    rampart_db::metric_samples::insert_many(s.pool(), &parsed.samples).await?;
    Ok(Json(IngestOutcome {
        accepted: parsed.samples.len(),
        skipped: parsed.skipped,
    }))
}

#[derive(Serialize)]
struct SeriesOut {
    name: String,
    labels: serde_json::Value,
    last_ts: OffsetDateTime,
    samples: i64,
}

async fn series(State(s): State<AppState>) -> Result<Json<Vec<SeriesOut>>, ApiError> {
    let series = rampart_db::metric_samples::list_series(s.pool()).await?;
    Ok(Json(
        series
            .into_iter()
            .map(|x| SeriesOut {
                name: x.name,
                labels: x.labels,
                last_ts: x.last_ts,
                samples: x.samples,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct QueryParams {
    name: String,
    /// JSON object of the series' exact label set; defaults to `{}`.
    #[serde(default)]
    labels: Option<String>,
    /// RFC3339; defaults to 24h ago.
    #[serde(default)]
    from: Option<String>,
    /// RFC3339; defaults to now.
    #[serde(default)]
    to: Option<String>,
    /// Bucket width; clamped to [10, 86400], default 300.
    #[serde(default)]
    step_seconds: Option<i64>,
}

#[derive(Serialize)]
struct QueryPoint {
    ts: OffsetDateTime,
    avg: f64,
    min: f64,
    max: f64,
    samples: i64,
}

fn parse_rfc3339(s: &str) -> Result<OffsetDateTime, ApiError> {
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|_| ApiError::BadRequest(format!("invalid RFC3339 timestamp: {s}")))
}

async fn query(
    State(s): State<AppState>,
    Query(p): Query<QueryParams>,
) -> Result<Json<Vec<QueryPoint>>, ApiError> {
    let labels: serde_json::Value = match p.labels.as_deref() {
        None | Some("") => serde_json::json!({}),
        Some(raw) => serde_json::from_str(raw)
            .map_err(|_| ApiError::BadRequest("labels must be a JSON object".into()))?,
    };
    if !labels.is_object() {
        return Err(ApiError::BadRequest("labels must be a JSON object".into()));
    }
    let now = OffsetDateTime::now_utc();
    let to =
        p.to.as_deref()
            .map(parse_rfc3339)
            .transpose()?
            .unwrap_or(now);
    let from = p
        .from
        .as_deref()
        .map(parse_rfc3339)
        .transpose()?
        .unwrap_or(now - time::Duration::hours(24));
    if from >= to {
        return Err(ApiError::BadRequest("from must precede to".into()));
    }
    let step = p.step_seconds.unwrap_or(300).clamp(10, 86_400);

    let points =
        rampart_db::metric_samples::range_query(s.pool(), &p.name, &labels, from, to, step).await?;
    Ok(Json(
        points
            .into_iter()
            .map(|x| QueryPoint {
                ts: x.bucket,
                avg: x.avg,
                min: x.min,
                max: x.max,
                samples: x.samples,
            })
            .collect(),
    ))
}
