//! SQLite `monitors` domain — the core monitoring entity. Mirrors a CORE subset
//! of the Postgres `crate::monitors` surface against SQLite:
//! create / get / get_unscoped / list / list_all / delete / set_active /
//! set_status. DEFERRED to later slices (heavier / dialect-divergent): update
//! (wide COALESCE), bulk_edit (+tx), push-token + run lifecycle, SLO + cert
//! info, agent assignment, tag-scoped ops, tag hydration (tags read back empty
//! for now). The wide row is read via the `sqlx::Row` get-by-name API rather
//! than a 40-field tuple.
//!
//! Dialect: enums → TEXT (serde round-trip), `int[] accepted_statuses` → JSON
//! TEXT, jsonb → TEXT, timestamps → INTEGER unix-seconds, bools → INTEGER 0/1.

use super::{kind_from, kind_str, mid, mstatus_from, mstatus_str, raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::ids::{AgentId, EscalationPolicyId, MonitorGroupId, MonitorId, OrgId, ProxyId};
use rampart_core::monitor::{Monitor, MonitorStatus, NewMonitor};
use sqlx::{Row, SqlitePool};

fn monitor_from(r: &sqlx::sqlite::SqliteRow) -> Monitor {
    let opt_id = |col: &str| r.get::<Option<String>, _>(col);
    Monitor {
        id: mid(&r.get::<String, _>("id")),
        name: r.get("name"),
        kind: kind_from(&r.get::<String, _>("kind")),
        url: r.get("url"),
        hostname: r.get("hostname"),
        port: r.get("port"),
        config: serde_json::from_str(&r.get::<String, _>("config")).unwrap_or_default(),
        interval_seconds: r.get("interval_seconds"),
        retry_interval_sec: r.get("retry_interval_sec"),
        max_retries: r.get("max_retries"),
        timeout_seconds: r.get("timeout_seconds"),
        resend_interval_sec: r.get("resend_interval_sec"),
        upside_down: r.get::<i64, _>("upside_down") != 0,
        http_method: r.get("http_method"),
        http_body: r.get("http_body"),
        http_headers: r
            .get::<Option<String>, _>("http_headers")
            .and_then(|s| serde_json::from_str(&s).ok()),
        accepted_statuses: serde_json::from_str(&r.get::<String, _>("accepted_statuses"))
            .unwrap_or_default(),
        follow_redirect: r.get::<i64, _>("follow_redirect") != 0,
        ignore_tls: r.get::<i64, _>("ignore_tls") != 0,
        proxy_id: opt_id("proxy_id").map(|s| ProxyId::from_uuid(raw_uuid(&s))),
        push_token: r.get("push_token"),
        last_push_at: r.get::<Option<i64>, _>("last_push_at").map(ts),
        last_run_started_at: r.get::<Option<i64>, _>("last_run_started_at").map(ts),
        active: r.get::<i64, _>("active") != 0,
        current_status: mstatus_from(&r.get::<String, _>("current_status")),
        created_at: ts(r.get::<i64, _>("created_at")),
        updated_at: ts(r.get::<i64, _>("updated_at")),
        tags: Vec::new(), // hydration deferred to a later slice
        cert_days_left: r.get("cert_days_left"),
        cert_subject: r.get("cert_subject"),
        cert_checked_at: r.get::<Option<i64>, _>("cert_checked_at").map(ts),
        check_cert: r.get::<i64, _>("check_cert") != 0,
        cert_expiry_days: r.get("cert_expiry_days"),
        group_id: opt_id("group_id").map(|s| MonitorGroupId::from_uuid(raw_uuid(&s))),
        slo_target_pct: r.get("slo_target_pct"),
        slo_window_days: r.get("slo_window_days"),
        agent_id: opt_id("agent_id").map(|s| AgentId::from_uuid(raw_uuid(&s))),
        escalation_policy_id: opt_id("escalation_policy_id")
            .map(|s| EscalationPolicyId::from_uuid(raw_uuid(&s))),
    }
}

pub async fn create(pool: &SqlitePool, input: NewMonitor, org_id: OrgId) -> DbResult<Monitor> {
    let id = MonitorId::new();
    sqlx::query(
        "INSERT INTO monitors (
            id, name, kind, url, hostname, port, config, interval_seconds,
            retry_interval_sec, max_retries, timeout_seconds, resend_interval_sec,
            upside_down, http_method, http_body, http_headers, accepted_statuses,
            follow_redirect, ignore_tls, proxy_id, group_id, check_cert,
            cert_expiry_days, slo_target_pct, slo_window_days, agent_id,
            escalation_policy_id, org_id)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(id.0.to_string())
    .bind(&input.name)
    .bind(kind_str(input.kind))
    .bind(&input.url)
    .bind(&input.hostname)
    .bind(input.port)
    .bind(input.config.to_string())
    .bind(input.interval_seconds)
    .bind(input.retry_interval_sec)
    .bind(input.max_retries)
    .bind(input.timeout_seconds)
    .bind(input.resend_interval_sec)
    .bind(input.upside_down as i64)
    .bind(&input.http_method)
    .bind(&input.http_body)
    .bind(input.http_headers.as_ref().map(|v| v.to_string()))
    .bind(serde_json::to_string(&input.accepted_statuses).unwrap_or_else(|_| "[]".into()))
    .bind(input.follow_redirect as i64)
    .bind(input.ignore_tls as i64)
    .bind(input.proxy_id.map(|p| p.0.to_string()))
    .bind(input.group_id.map(|g| g.0.to_string()))
    .bind(input.check_cert as i64)
    .bind(input.cert_expiry_days)
    .bind(input.slo_target_pct)
    .bind(input.slo_window_days)
    .bind(input.agent_id.map(|a| a.0.to_string()))
    .bind(input.escalation_policy_id.map(|e| e.0.to_string()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_unscoped(pool, id).await
}

pub async fn get(pool: &SqlitePool, id: MonitorId, org_id: OrgId) -> DbResult<Monitor> {
    let row = sqlx::query("SELECT * FROM monitors WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?;
    row.map(|r| monitor_from(&r)).ok_or(DbError::NotFound)
}

pub async fn get_unscoped(pool: &SqlitePool, id: MonitorId) -> DbResult<Monitor> {
    let row = sqlx::query("SELECT * FROM monitors WHERE id = ?")
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?;
    row.map(|r| monitor_from(&r)).ok_or(DbError::NotFound)
}

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query("SELECT * FROM monitors WHERE org_id = ? ORDER BY created_at ASC")
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(monitor_from).collect())
}

pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<Monitor>> {
    let rows = sqlx::query("SELECT * FROM monitors ORDER BY created_at ASC")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(monitor_from).collect())
}

pub async fn delete(pool: &SqlitePool, id: MonitorId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM monitors WHERE id = ? AND org_id = ?")
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
    pool: &SqlitePool,
    id: MonitorId,
    active: bool,
    org_id: OrgId,
) -> DbResult<()> {
    let r = sqlx::query(
        "UPDATE monitors SET active = ?, updated_at = unixepoch() WHERE id = ? AND org_id = ?",
    )
    .bind(active as i64)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_status(pool: &SqlitePool, id: MonitorId, status: MonitorStatus) -> DbResult<()> {
    sqlx::query("UPDATE monitors SET current_status = ?, updated_at = unixepoch() WHERE id = ?")
        .bind(mstatus_str(status))
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rampart_core::monitor::MonitorKind;
    use sqlx::SqlitePool;

    fn new_http(name: &str) -> NewMonitor {
        NewMonitor {
            name: name.into(),
            kind: MonitorKind::Http,
            url: Some("https://example.com".into()),
            hostname: None,
            port: None,
            config: serde_json::json!({}),
            interval_seconds: 60,
            timeout_seconds: 10,
            max_retries: 2,
            retry_interval_sec: 30,
            resend_interval_sec: 0,
            upside_down: false,
            http_method: "GET".into(),
            http_body: None,
            http_headers: Some(serde_json::json!({"X-Trace": "1"})),
            accepted_statuses: vec![200, 204],
            follow_redirect: true,
            ignore_tls: false,
            proxy_id: None,
            group_id: None,
            check_cert: true,
            cert_expiry_days: 14,
            slo_target_pct: Some(99.9),
            slo_window_days: Some(30),
            agent_id: None,
            escalation_policy_id: None,
        }
    }

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn create_read_update_delete(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let m = create(&pool, new_http("api"), org).await.unwrap();
        assert_eq!(m.kind, MonitorKind::Http);
        assert_eq!(m.accepted_statuses, vec![200, 204]);
        assert_eq!(m.http_headers.unwrap()["X-Trace"], "1");
        assert_eq!(m.slo_target_pct, Some(99.9));
        assert!(m.check_cert);
        assert_eq!(m.current_status, MonitorStatus::Pending); // server default
        assert!(m.active);

        // get (org-scoped) + get_unscoped + list + list_all.
        assert_eq!(get(&pool, m.id, org).await.unwrap().name, "api");
        assert_eq!(get_unscoped(&pool, m.id).await.unwrap().name, "api");
        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(list_all(&pool).await.unwrap().len(), 1);

        // Cross-org isolation: a different org can't see it.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            get(&pool, m.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert_eq!(list(&pool, other.id).await.unwrap().len(), 0);

        // set_active + set_status.
        set_active(&pool, m.id, false, org).await.unwrap();
        assert!(!get(&pool, m.id, org).await.unwrap().active);
        set_status(&pool, m.id, MonitorStatus::Up).await.unwrap();
        assert_eq!(
            get(&pool, m.id, org).await.unwrap().current_status,
            MonitorStatus::Up
        );

        // delete is org-scoped.
        assert!(matches!(
            delete(&pool, m.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, m.id, org).await.unwrap();
        assert!(matches!(
            get_unscoped(&pool, m.id).await,
            Err(DbError::NotFound)
        ));
    }
}
