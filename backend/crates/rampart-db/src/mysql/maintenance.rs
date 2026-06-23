//! MySQL `maintenance` domain — maintenance windows + their monitor sets.
//! Mirrors the PG/SQLite surface for the 12 self-contained fns: list / get /
//! create / update / delete / set_active / attach / detach / is_in_active_window
//! / transitions_needing_notification / mark_notified_start / mark_notified_end.
//!
//! DEFERRED (couple to the not-yet-ported `status_pages` tables):
//! `confirmed_subscriber_emails_for_monitors` + `public_for_status_page` stay
//! `unimplemented!()` on `MysqlStore` until status_pages is forked.
//!
//! Dialect: uuid→CHAR(36), ts→BIGINT, bool→TINYINT, recurrence jsonb→LONGTEXT.
//! `Recurrence::contains` is pure `rampart_core`, reused verbatim. `ON CONFLICT
//! DO NOTHING` → `INSERT IGNORE`; `= ANY($1)` → bound `IN (?,…)`. `update` drops
//! the `rows_affected()` gate (MySQL changed-vs-matched; the trailing `get()`
//! surfaces NotFound); `set_active` disambiguates a 0-row UPDATE (no-op vs
//! absent) with an existence SELECT.

use super::{in_placeholders, mid, raw_uuid, ts};
use crate::maintenance::MaintenanceTransition;
use crate::{DbError, DbResult};
use rampart_core::ids::OrgId;
use rampart_core::maintenance::{
    MaintenanceWindow, NewMaintenanceWindow, Recurrence, UpdateMaintenanceWindow,
};
use rampart_core::{MaintenanceId, MonitorId};
use sqlx::{MySqlPool, Row};
use time::OffsetDateTime;

fn recurrence_from(s: &str) -> Recurrence {
    serde_json::from_str(s).unwrap_or(Recurrence::None)
}

fn window_from(r: &sqlx::mysql::MySqlRow) -> MaintenanceWindow {
    MaintenanceWindow {
        id: MaintenanceId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        description: r.get("description"),
        start_at: ts(r.get::<i64, _>("start_at")),
        end_at: ts(r.get::<i64, _>("end_at")),
        active: r.get::<i64, _>("active") != 0,
        created_at: ts(r.get::<i64, _>("created_at")),
        monitor_ids: Vec::new(),
        recurrence: recurrence_from(&r.get::<String, _>("recurrence")),
    }
}

const COLS: &str = "id, name, description, start_at, end_at, active, created_at, recurrence";

/// Hydrate `monitor_ids` for a set of windows via one `IN (?,…)` round trip.
async fn hydrate_monitors(pool: &MySqlPool, windows: &mut [MaintenanceWindow]) -> DbResult<()> {
    if windows.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "SELECT window_id, monitor_id FROM maintenance_window_monitors WHERE window_id IN ({})",
        in_placeholders(windows.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for w in windows.iter() {
        q = q.bind(w.id.0.to_string());
    }
    let edges = q.fetch_all(pool).await?;
    for e in &edges {
        let wid = raw_uuid(&e.get::<String, _>("window_id"));
        if let Some(w) = windows.iter_mut().find(|w| w.id.0 == wid) {
            w.monitor_ids.push(mid(&e.get::<String, _>("monitor_id")));
        }
    }
    Ok(())
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<MaintenanceWindow>> {
    let sql =
        format!("SELECT {COLS} FROM maintenance_windows WHERE org_id = ? ORDER BY start_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    let mut windows: Vec<MaintenanceWindow> = rows.iter().map(window_from).collect();
    hydrate_monitors(pool, &mut windows).await?;
    Ok(windows)
}

pub async fn get(
    pool: &MySqlPool,
    id: MaintenanceId,
    org_id: OrgId,
) -> DbResult<MaintenanceWindow> {
    let sql = format!("SELECT {COLS} FROM maintenance_windows WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    let mut w = window_from(&row);
    let edges =
        sqlx::query("SELECT monitor_id FROM maintenance_window_monitors WHERE window_id = ?")
            .bind(id.0.to_string())
            .fetch_all(pool)
            .await?;
    w.monitor_ids = edges
        .iter()
        .map(|e| mid(&e.get::<String, _>("monitor_id")))
        .collect();
    Ok(w)
}

pub async fn create(
    pool: &MySqlPool,
    input: NewMaintenanceWindow,
    org_id: OrgId,
) -> DbResult<MaintenanceWindow> {
    let id = MaintenanceId::new();
    let recurrence_json = serde_json::to_string(&input.recurrence)
        .map_err(|e| DbError::Conflict(format!("serialize recurrence: {e}")))?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO maintenance_windows (id, name, description, start_at, end_at, active, recurrence, org_id)
         VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.description)
    .bind(input.start_at.unix_timestamp())
    .bind(input.end_at.unix_timestamp())
    .bind(recurrence_json)
    .bind(org_id.0.to_string())
    .execute(&mut *tx)
    .await?;
    for m in &input.monitor_ids {
        sqlx::query(
            "INSERT IGNORE INTO maintenance_window_monitors (window_id, monitor_id) VALUES (?, ?)",
        )
        .bind(id.0.to_string())
        .bind(m.0.to_string())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    get(pool, id, org_id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: MaintenanceId,
    patch: UpdateMaintenanceWindow,
    org_id: OrgId,
) -> DbResult<MaintenanceWindow> {
    // description: double-Option (absent=keep, Some(None)=clear, Some(Some)=set).
    let desc_provided = patch.description.is_some();
    let desc_value: Option<String> = patch.description.flatten();
    let recurrence_json = match &patch.recurrence {
        Some(r) => Some(
            serde_json::to_string(r)
                .map_err(|e| DbError::Conflict(format!("serialize recurrence: {e}")))?,
        ),
        None => None,
    };
    sqlx::query(
        "UPDATE maintenance_windows
            SET name        = COALESCE(?, name),
                description = CASE WHEN ? THEN ? ELSE description END,
                start_at    = COALESCE(?, start_at),
                end_at      = COALESCE(?, end_at),
                recurrence  = COALESCE(?, recurrence)
          WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(desc_provided as i64)
    .bind(desc_value)
    .bind(patch.start_at.map(|t| t.unix_timestamp()))
    .bind(patch.end_at.map(|t| t.unix_timestamp()))
    .bind(recurrence_json)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // NotFound surfaces here (no rows_affected gate — MySQL changed-vs-matched).
    get(pool, id, org_id).await
}

pub async fn delete(pool: &MySqlPool, id: MaintenanceId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM maintenance_windows WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_active(
    pool: &MySqlPool,
    id: MaintenanceId,
    active: bool,
    org_id: OrgId,
) -> DbResult<()> {
    let r = sqlx::query("UPDATE maintenance_windows SET active = ? WHERE id = ? AND org_id = ?")
        .bind(active as i64)
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        // MySQL counts CHANGED rows → 0 could be a no-op (active already == ?) OR
        // an absent/cross-org row. Disambiguate with an existence check.
        let (exists,): (i64,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM maintenance_windows WHERE id = ? AND org_id = ?)",
        )
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_one(pool)
        .await?;
        if exists == 0 {
            return Err(DbError::NotFound);
        }
    }
    Ok(())
}

pub async fn attach(pool: &MySqlPool, window: MaintenanceId, monitor: MonitorId) -> DbResult<()> {
    sqlx::query(
        "INSERT IGNORE INTO maintenance_window_monitors (window_id, monitor_id) VALUES (?, ?)",
    )
    .bind(window.0.to_string())
    .bind(monitor.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &MySqlPool, window: MaintenanceId, monitor: MonitorId) -> DbResult<()> {
    sqlx::query("DELETE FROM maintenance_window_monitors WHERE window_id = ? AND monitor_id = ?")
        .bind(window.0.to_string())
        .bind(monitor.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// True iff `monitor` is covered by any active window containing NOW() per its
/// recurrence. Recurrence eval is the pure core logic (reused).
pub async fn is_in_active_window(pool: &MySqlPool, monitor: MonitorId) -> DbResult<bool> {
    let rows = sqlx::query(
        "SELECT w.start_at, w.end_at, w.recurrence
         FROM maintenance_windows w
         JOIN maintenance_window_monitors m ON m.window_id = w.id
         WHERE m.monitor_id = ? AND w.active = 1",
    )
    .bind(monitor.0.to_string())
    .fetch_all(pool)
    .await?;
    let now = OffsetDateTime::now_utc();
    for r in &rows {
        let rec = recurrence_from(&r.get::<String, _>("recurrence"));
        if rec.contains(
            ts(r.get::<i64, _>("start_at")),
            ts(r.get::<i64, _>("end_at")),
            now,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Active windows whose start/end transition just crossed `now` and hasn't been
/// notified yet, hydrated with their monitor sets. Cross-tenant (the scheduler
/// scans every org), mirroring PG.
pub async fn transitions_needing_notification(
    pool: &MySqlPool,
) -> DbResult<Vec<MaintenanceTransition>> {
    let rows = sqlx::query(
        "SELECT id, name, start_at, end_at, recurrence, notified_start_at, notified_end_at
         FROM maintenance_windows WHERE active = 1",
    )
    .fetch_all(pool)
    .await?;
    let now = OffsetDateTime::now_utc();
    let mut out: Vec<MaintenanceTransition> = Vec::new();
    for r in &rows {
        let rec = recurrence_from(&r.get::<String, _>("recurrence"));
        let currently_active = rec.contains(
            ts(r.get::<i64, _>("start_at")),
            ts(r.get::<i64, _>("end_at")),
            now,
        );
        let notified_start_at = r.get::<Option<i64>, _>("notified_start_at").map(ts);
        let notified_end_at = r.get::<Option<i64>, _>("notified_end_at").map(ts);
        let start_pending = currently_active && notified_start_at.is_none();
        let end_pending =
            !currently_active && notified_start_at.is_some() && notified_end_at.is_none();
        if !start_pending && !end_pending {
            continue;
        }
        out.push(MaintenanceTransition {
            id: MaintenanceId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
            name: r.get("name"),
            monitor_ids: Vec::new(),
            currently_active,
            notified_start_at,
            notified_end_at,
        });
    }
    if out.is_empty() {
        return Ok(out);
    }
    let sql = format!(
        "SELECT window_id, monitor_id FROM maintenance_window_monitors WHERE window_id IN ({})",
        in_placeholders(out.len())
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    for t in out.iter() {
        q = q.bind(t.id.0.to_string());
    }
    let edges = q.fetch_all(pool).await?;
    for e in &edges {
        let wid = raw_uuid(&e.get::<String, _>("window_id"));
        if let Some(t) = out.iter_mut().find(|t| t.id.0 == wid) {
            t.monitor_ids.push(mid(&e.get::<String, _>("monitor_id")));
        }
    }
    Ok(out)
}

pub async fn mark_notified_start(pool: &MySqlPool, id: MaintenanceId) -> DbResult<()> {
    sqlx::query("UPDATE maintenance_windows SET notified_start_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_notified_end(pool: &MySqlPool, id: MaintenanceId) -> DbResult<()> {
    sqlx::query("UPDATE maintenance_windows SET notified_end_at = UNIX_TIMESTAMP() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::monitors;
    use rampart_core::monitor::{MonitorKind, NewMonitor};

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    async fn a_monitor(pool: &MySqlPool) -> MonitorId {
        let org = super::super::oid(DEF);
        monitors::create(
            pool,
            NewMonitor {
                name: "m".into(),
                kind: MonitorKind::Http,
                url: Some("https://x".into()),
                hostname: None,
                port: None,
                config: serde_json::json!({}),
                interval_seconds: 60,
                timeout_seconds: 10,
                max_retries: 0,
                retry_interval_sec: 60,
                resend_interval_sec: 0,
                upside_down: false,
                http_method: "GET".into(),
                http_body: None,
                http_headers: None,
                accepted_statuses: vec![200],
                follow_redirect: true,
                ignore_tls: false,
                proxy_id: None,
                group_id: None,
                check_cert: false,
                cert_expiry_days: 14,
                slo_target_pct: None,
                slo_window_days: None,
                agent_id: None,
                escalation_policy_id: None,
            },
            org,
        )
        .await
        .unwrap()
        .id
    }

    fn new_window(name: &str, from: OffsetDateTime, to: OffsetDateTime) -> NewMaintenanceWindow {
        NewMaintenanceWindow {
            name: name.into(),
            description: Some("planned".into()),
            start_at: from,
            end_at: to,
            recurrence: Recurrence::None,
            monitor_ids: vec![],
        }
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn crud_attach_and_active_window(pool: MySqlPool) {
        let org = super::super::oid(DEF);
        let m = a_monitor(&pool).await;
        let now = OffsetDateTime::now_utc();
        let w = create(
            &pool,
            new_window(
                "db upgrade",
                now - time::Duration::hours(1),
                now + time::Duration::hours(1),
            ),
            org,
        )
        .await
        .unwrap();
        assert_eq!(w.description.as_deref(), Some("planned"));
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);

        attach(&pool, w.id, m).await.unwrap();
        assert_eq!(get(&pool, w.id, org).await.unwrap().monitor_ids.len(), 1);
        assert!(is_in_active_window(&pool, m).await.unwrap());

        // a start transition is pending (active, never notified).
        let trans = transitions_needing_notification(&pool).await.unwrap();
        assert_eq!(trans.len(), 1);
        assert!(trans[0].currently_active);
        assert_eq!(trans[0].monitor_ids.len(), 1);
        mark_notified_start(&pool, w.id).await.unwrap();
        assert!(transitions_needing_notification(&pool)
            .await
            .unwrap()
            .is_empty());

        // update: clear description (explicit null) + rename.
        let u = update(
            &pool,
            w.id,
            UpdateMaintenanceWindow {
                name: Some("db upgrade v2".into()),
                description: Some(None),
                start_at: None,
                end_at: None,
                recurrence: None,
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.name, "db upgrade v2");
        assert!(u.description.is_none());

        // no-op set_active to the SAME value must NOT 404 (MySQL changed-vs-matched).
        set_active(&pool, w.id, true, org).await.unwrap();

        detach(&pool, w.id, m).await.unwrap();
        assert!(!is_in_active_window(&pool, m).await.unwrap());
        set_active(&pool, w.id, false, org).await.unwrap();
        assert!(!get(&pool, w.id, org).await.unwrap().active);

        // cross-org isolation + delete.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, w.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(matches!(
            set_active(&pool, w.id, true, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, w.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, w.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
