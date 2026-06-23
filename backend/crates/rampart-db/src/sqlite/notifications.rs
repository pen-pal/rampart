//! SQLite `notifications` (channels) domain. Mirrors the Postgres
//! `crate::notifications` free-fn surface: list / list_all / get / get_unscoped
//! / create / update / counts_per_monitor / delete / attach / detach /
//! for_monitor / mark_fired.
//!
//! Structs (`Notification`, `NewNotification`, `UpdateNotification`,
//! `MonitorChannelCount`) and the clamp helpers are REUSED from the PG module —
//! they're dialect-neutral serde types, so behavior (the `double_option` merge,
//! the 0..=3600 / 0..=23 / >=0 clamps) stays identical across backends.
//!
//! Encryption: channel `config` is sealed by [`crate::secrets::seal`] before
//! write and re-opened by [`crate::secrets::open`] on EVERY read (the #112 bug
//! was a read path that forgot `open`). The SQLite column stores the TEXT
//! serialization of the sealed `Value`. Dialect: uuid→TEXT, timestamps→INTEGER
//! unix-seconds, booleans→INTEGER 0/1, jsonb→TEXT, channel_kind→serde-round-trip
//! TEXT. Tags are hydrated via the existing [`super::tags::hydrate_for_channels`].

use super::{raw_uuid, ts};
use crate::notifications::{
    clamp_digest_window, clamp_hour, clamp_rate, MonitorChannelCount, NewNotification,
    Notification, UpdateNotification,
};
use crate::{DbError, DbResult};
use rampart_core::ids::{MonitorId, NotificationId, NotificationTemplateId, OrgId};
use rampart_core::ChannelKind;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

/// The 13 channel columns every read selects (bare names so the get-by-name
/// row helper resolves them after a `for_monitor` JOIN aliases `n.*`).
pub(crate) const COLS: &str =
    "id, kind, name, config, active, template_id, created_at, cooldown_seconds, \
     digest_window_secs, quiet_hours_start, quiet_hours_end, rate_limit_per_hour, last_fired_at";

fn channel_kind_str(k: ChannelKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "custom".into())
}
fn channel_kind_from(s: &str) -> ChannelKind {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or(ChannelKind::Custom)
}

/// Build a `Notification` from a wide row (get-by-name avoids the 13-tuple +
/// clippy `type_complexity`). Config is re-opened here so EVERY materialization
/// decrypts — there is no read path that can forget it. Integers are read as
/// `i64` then narrowed, matching the rest of the SQLite layer.
pub(crate) fn notification_from(r: &sqlx::sqlite::SqliteRow) -> Notification {
    let cfg: serde_json::Value =
        serde_json::from_str(&r.get::<String, _>("config")).unwrap_or_default();
    Notification {
        id: NotificationId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        kind: channel_kind_from(&r.get::<String, _>("kind")),
        name: r.get("name"),
        config: crate::secrets::open(cfg),
        active: r.get::<i64, _>("active") != 0,
        template_id: r
            .get::<Option<String>, _>("template_id")
            .map(|s| NotificationTemplateId::from_uuid(raw_uuid(&s))),
        created_at: ts(r.get::<i64, _>("created_at")),
        cooldown_seconds: r.get::<i64, _>("cooldown_seconds") as i32,
        digest_window_secs: r.get::<i64, _>("digest_window_secs") as i32,
        quiet_hours_start: r
            .get::<Option<i64>, _>("quiet_hours_start")
            .map(|v| v as i16),
        quiet_hours_end: r.get::<Option<i64>, _>("quiet_hours_end").map(|v| v as i16),
        rate_limit_per_hour: r.get::<i64, _>("rate_limit_per_hour") as i32,
        last_fired_at: r.get::<Option<i64>, _>("last_fired_at").map(ts),
        tags: Vec::new(),
    }
}

/// Batch-attach routing tags to a channel list (one round trip), mirroring PG.
async fn hydrate(pool: &SqlitePool, mut chans: Vec<Notification>) -> DbResult<Vec<Notification>> {
    if chans.is_empty() {
        return Ok(chans);
    }
    let ids: Vec<NotificationId> = chans.iter().map(|c| c.id).collect();
    let mut by = super::tags::hydrate_for_channels(pool, &ids).await?;
    for c in &mut chans {
        if let Some(t) = by.remove(&c.id) {
            c.tags = t;
        }
    }
    Ok(chans)
}

async fn hydrate_one(pool: &SqlitePool, mut n: Notification) -> DbResult<Notification> {
    let mut by = super::tags::hydrate_for_channels(pool, &[n.id]).await?;
    n.tags = by.remove(&n.id).unwrap_or_default();
    Ok(n)
}

pub async fn list(pool: &SqlitePool, org_id: OrgId) -> DbResult<Vec<Notification>> {
    let sql = format!("SELECT {COLS} FROM notifications WHERE org_id = ? ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(org_id.0.to_string())
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(notification_from).collect()).await
}

pub async fn list_all(pool: &SqlitePool) -> DbResult<Vec<Notification>> {
    let sql = format!("SELECT {COLS} FROM notifications ORDER BY created_at DESC");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(notification_from).collect()).await
}

pub async fn get(pool: &SqlitePool, id: NotificationId, org_id: OrgId) -> DbResult<Notification> {
    let sql = format!("SELECT {COLS} FROM notifications WHERE id = ? AND org_id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    hydrate_one(pool, notification_from(&row)).await
}

pub async fn get_unscoped(pool: &SqlitePool, id: NotificationId) -> DbResult<Notification> {
    let sql = format!("SELECT {COLS} FROM notifications WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.0.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(DbError::NotFound)?;
    hydrate_one(pool, notification_from(&row)).await
}

pub async fn create(
    pool: &SqlitePool,
    input: NewNotification,
    org_id: OrgId,
) -> DbResult<Notification> {
    let id = Uuid::now_v7();
    let sealed = crate::secrets::seal(&input.config);
    let sql = format!(
        "INSERT INTO notifications
           (id, kind, name, config, active, template_id, cooldown_seconds, digest_window_secs,
            quiet_hours_start, quiet_hours_end, rate_limit_per_hour, org_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING {COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.to_string())
        .bind(channel_kind_str(input.kind))
        .bind(input.name)
        .bind(serde_json::to_string(&sealed).unwrap_or_else(|_| "{}".into()))
        .bind(input.active as i64)
        .bind(input.template_id.map(|t| t.0.to_string()))
        .bind(input.cooldown_seconds)
        .bind(clamp_digest_window(input.digest_window_secs))
        .bind(clamp_hour(input.quiet_hours_start).map(|h| h as i64))
        .bind(clamp_hour(input.quiet_hours_end).map(|h| h as i64))
        .bind(clamp_rate(input.rate_limit_per_hour))
        .bind(org_id.0.to_string())
        .fetch_one(pool)
        .await?;
    // Fresh row has no tags; skip the hydrate round trip.
    Ok(notification_from(&row))
}

pub async fn update(
    pool: &SqlitePool,
    id: NotificationId,
    input: UpdateNotification,
    org_id: OrgId,
) -> DbResult<Notification> {
    // Read-modify-write, identical to PG (existence/cross-org checked by `get`).
    let cur = get(pool, id, org_id).await?;
    let new_name = input.name.unwrap_or(cur.name);
    let new_config = input.config.unwrap_or(cur.config);
    let new_active = input.active.unwrap_or(cur.active);
    let new_cooldown = input.cooldown_seconds.unwrap_or(cur.cooldown_seconds);
    let new_digest =
        clamp_digest_window(input.digest_window_secs.unwrap_or(cur.digest_window_secs));
    let new_template_id = match input.template_id {
        None => cur.template_id.map(|t| t.0.to_string()),
        Some(None) => None,
        Some(Some(t)) => Some(t.0.to_string()),
    };
    let new_quiet_start = clamp_hour(match input.quiet_hours_start {
        None => cur.quiet_hours_start,
        Some(v) => v,
    });
    let new_quiet_end = clamp_hour(match input.quiet_hours_end {
        None => cur.quiet_hours_end,
        Some(v) => v,
    });
    let new_rate = clamp_rate(input.rate_limit_per_hour.unwrap_or(cur.rate_limit_per_hour));
    let sealed = crate::secrets::seal(&new_config);

    let sql = format!(
        "UPDATE notifications
            SET name = ?, config = ?, active = ?, template_id = ?, cooldown_seconds = ?,
                digest_window_secs = ?, quiet_hours_start = ?, quiet_hours_end = ?,
                rate_limit_per_hour = ?
          WHERE id = ? AND org_id = ?
          RETURNING {COLS}"
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(new_name)
        .bind(serde_json::to_string(&sealed).unwrap_or_else(|_| "{}".into()))
        .bind(new_active as i64)
        .bind(new_template_id)
        .bind(new_cooldown)
        .bind(new_digest)
        .bind(new_quiet_start.map(|h| h as i64))
        .bind(new_quiet_end.map(|h| h as i64))
        .bind(new_rate)
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .fetch_one(pool)
        .await?;
    hydrate_one(pool, notification_from(&row)).await
}

pub async fn counts_per_monitor(
    pool: &SqlitePool,
    org_id: OrgId,
) -> DbResult<Vec<MonitorChannelCount>> {
    let rows = sqlx::query(
        "SELECT mn.monitor_id AS monitor_id, COUNT(*) AS count
         FROM monitor_notifications mn
         JOIN notifications n ON n.id = mn.notification_id
         WHERE n.active = 1 AND n.org_id = ?
         GROUP BY mn.monitor_id",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| MonitorChannelCount {
            monitor_id: MonitorId::from_uuid(raw_uuid(&r.get::<String, _>("monitor_id"))),
            count: r.get::<i64, _>("count"),
        })
        .collect())
}

pub async fn delete(pool: &SqlitePool, id: NotificationId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM notifications WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn attach(pool: &SqlitePool, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO monitor_notifications (monitor_id, notification_id)
         VALUES (?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(monitor.0.to_string())
    .bind(notif.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach(pool: &SqlitePool, monitor: MonitorId, notif: NotificationId) -> DbResult<()> {
    sqlx::query("DELETE FROM monitor_notifications WHERE monitor_id = ? AND notification_id = ?")
        .bind(monitor.0.to_string())
        .bind(notif.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn for_monitor(pool: &SqlitePool, monitor: MonitorId) -> DbResult<Vec<Notification>> {
    let sql = format!(
        "SELECT {COLS}
         FROM notifications n
         JOIN monitor_notifications mn ON mn.notification_id = n.id
         WHERE mn.monitor_id = ? AND n.active = 1"
    );
    // COLS are bare column names; the JOIN doesn't introduce a collision (only
    // `notifications` carries them), so get-by-name resolves cleanly.
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(monitor.0.to_string())
        .fetch_all(pool)
        .await?;
    hydrate(pool, rows.iter().map(notification_from).collect()).await
}

pub async fn mark_fired(pool: &SqlitePool, id: NotificationId) -> DbResult<()> {
    sqlx::query("UPDATE notifications SET last_fired_at = unixepoch() WHERE id = ?")
        .bind(id.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::{monitors, orgs, tags};
    use rampart_core::monitor::{MonitorKind, NewMonitor};
    use rampart_core::tag::NewTag;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    fn new_chan(name: &str, kind: ChannelKind) -> NewNotification {
        NewNotification {
            kind,
            name: name.into(),
            config: serde_json::json!({ "url": "https://hooks.example/x" }),
            active: true,
            template_id: None,
            cooldown_seconds: 0,
            digest_window_secs: 0,
            quiet_hours_start: None,
            quiet_hours_end: None,
            rate_limit_per_hour: 0,
        }
    }

    async fn a_monitor(pool: &SqlitePool, name: &str) -> MonitorId {
        let org = super::super::oid(DEF);
        monitors::create(
            pool,
            NewMonitor {
                name: name.into(),
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

    fn patch(v: serde_json::Value) -> UpdateNotification {
        serde_json::from_value(v).unwrap()
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn crud_clamps_and_double_option(pool: SqlitePool) {
        let org = super::super::oid(DEF);

        // create clamps the out-of-range fields.
        let mut nc = new_chan("pager", ChannelKind::Slack);
        nc.digest_window_secs = 99999;
        nc.quiet_hours_start = Some(50);
        nc.rate_limit_per_hour = -5;
        let c = create(&pool, nc, org).await.unwrap();
        assert_eq!(c.digest_window_secs, 3600);
        assert_eq!(c.quiet_hours_start, Some(23));
        assert_eq!(c.rate_limit_per_hour, 0);
        // config opened back to plaintext on read (seal/open symmetry).
        assert_eq!(c.config["url"], "https://hooks.example/x");

        assert_eq!(list(&pool, org).await.unwrap().len(), 1);
        assert_eq!(get(&pool, c.id, org).await.unwrap().name, "pager");
        assert_eq!(get_unscoped(&pool, c.id).await.unwrap().name, "pager");

        // update: rename + flip active; quiet_start explicit-null clears, end kept.
        let u = update(
            &pool,
            c.id,
            patch(
                serde_json::json!({ "name": "pager2", "active": false, "quiet_hours_start": null }),
            ),
            org,
        )
        .await
        .unwrap();
        assert_eq!(u.name, "pager2");
        assert!(!u.active);
        assert!(u.quiet_hours_start.is_none());

        // cross-org isolation.
        let other = orgs::create(&pool, "other", "Other").await.unwrap();
        assert!(matches!(
            get(&pool, c.id, other.id).await,
            Err(DbError::NotFound)
        ));
        assert!(list(&pool, other.id).await.unwrap().is_empty());
        assert!(matches!(
            delete(&pool, c.id, other.id).await,
            Err(DbError::NotFound)
        ));

        delete(&pool, c.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, c.id, org).await,
            Err(DbError::NotFound)
        ));
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn enum_round_trip(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        for k in [
            ChannelKind::Slack,
            ChannelKind::Discord,
            ChannelKind::Webhook,
            ChannelKind::Custom,
        ] {
            let c = create(&pool, new_chan("c", k), org).await.unwrap();
            assert_eq!(get(&pool, c.id, org).await.unwrap().kind, k, "kind {k:?}");
        }
    }

    #[sqlx::test(migrations = "../../migrations-sqlite")]
    async fn attach_for_monitor_counts_and_tags(pool: SqlitePool) {
        let org = super::super::oid(DEF);
        let m = a_monitor(&pool, "m").await;
        let active = create(&pool, new_chan("on", ChannelKind::Slack), org)
            .await
            .unwrap();
        let mut off_nc = new_chan("off", ChannelKind::Webhook);
        off_nc.active = false;
        let off = create(&pool, off_nc, org).await.unwrap();

        attach(&pool, m, active.id).await.unwrap();
        attach(&pool, m, off.id).await.unwrap();
        attach(&pool, m, active.id).await.unwrap(); // idempotent

        // for_monitor returns only the active channel.
        let on = for_monitor(&pool, m).await.unwrap();
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].id, active.id);

        // counts_per_monitor counts active-only, org-scoped.
        let counts = counts_per_monitor(&pool, org).await.unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].count, 1);

        // tag hydration on read.
        let t = tags::create(
            &pool,
            NewTag {
                name: "prod".into(),
                color: "#f00".into(),
            },
            org,
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO notification_tags (notification_id, tag_id) VALUES (?, ?)")
            .bind(active.id.0.to_string())
            .bind(t.id.0.to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(get(&pool, active.id, org).await.unwrap().tags.len(), 1);

        // mark_fired stamps last_fired_at.
        assert!(get(&pool, active.id, org)
            .await
            .unwrap()
            .last_fired_at
            .is_none());
        mark_fired(&pool, active.id).await.unwrap();
        assert!(get(&pool, active.id, org)
            .await
            .unwrap()
            .last_fired_at
            .is_some());

        detach(&pool, m, active.id).await.unwrap();
        assert!(for_monitor(&pool, m).await.unwrap().is_empty());
    }
}
