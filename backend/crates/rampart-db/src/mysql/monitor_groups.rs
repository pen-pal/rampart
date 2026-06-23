//! MySQL `monitor_groups` domain — folder tree + monitor dependency graph.
//! Ported from PG. Groups: CRUD over `monitor_groups` (delete SET-NULLs child
//! monitors via the FK). Dependencies: a DAG over `monitor_dependencies`;
//! `any_parent_down` is the dispatch-path suppression read, the cycle guards
//! walk the graph in Rust (reused logic). Dialect: uuid→CHAR(36), ts→BIGINT,
//! `ON CONFLICT DO NOTHING`→`INSERT IGNORE`; `update` gates via `in_org` first
//! (MySQL counts CHANGED rows, so a no-op COALESCE update can't carry the gate).

use super::{mid, raw_uuid, ts};
use crate::{DbError, DbResult};
use rampart_core::ids::{MonitorGroupId, MonitorId, OrgId};
use rampart_core::monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
use sqlx::{MySqlPool, Row};
use std::collections::HashSet;

fn group_from(r: &sqlx::mysql::MySqlRow) -> MonitorGroup {
    MonitorGroup {
        id: MonitorGroupId::from_uuid(raw_uuid(&r.get::<String, _>("id"))),
        name: r.get("name"),
        sort_order: r.get::<i32, _>("sort_order"),
        parent_id: r
            .get::<Option<String>, _>("parent_id")
            .map(|s| MonitorGroupId::from_uuid(raw_uuid(&s))),
        created_at: ts(r.get::<i64, _>("created_at")),
    }
}

pub async fn in_org(pool: &MySqlPool, group: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
    let (ok,): (i64,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM monitor_groups WHERE id = ? AND org_id = ?)")
            .bind(group.0.to_string())
            .bind(org_id.0.to_string())
            .fetch_one(pool)
            .await?;
    if ok != 0 {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn list(pool: &MySqlPool, org_id: OrgId) -> DbResult<Vec<MonitorGroup>> {
    let rows = sqlx::query(
        "SELECT id, name, sort_order, parent_id, created_at FROM monitor_groups
         WHERE org_id = ? ORDER BY sort_order, name",
    )
    .bind(org_id.0.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(group_from).collect())
}

async fn get_in_org(pool: &MySqlPool, id: MonitorGroupId, org_id: OrgId) -> DbResult<MonitorGroup> {
    let row = sqlx::query(
        "SELECT id, name, sort_order, parent_id, created_at FROM monitor_groups
         WHERE id = ? AND org_id = ?",
    )
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .fetch_optional(pool)
    .await?
    .ok_or(DbError::NotFound)?;
    Ok(group_from(&row))
}

pub async fn create(
    pool: &MySqlPool,
    input: NewMonitorGroup,
    org_id: OrgId,
) -> DbResult<MonitorGroup> {
    let id = MonitorGroupId::new();
    sqlx::query(
        "INSERT INTO monitor_groups (id, name, sort_order, parent_id, org_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(input.name)
    .bind(input.sort_order)
    .bind(input.parent_id.map(|g| g.0.to_string()))
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    get_in_org(pool, id, org_id).await
}

pub async fn update(
    pool: &MySqlPool,
    id: MonitorGroupId,
    patch: UpdateMonitorGroup,
    org_id: OrgId,
) -> DbResult<MonitorGroup> {
    in_org(pool, id, org_id).await?; // gate (no rows_affected gate — MySQL changed-vs-matched)
    sqlx::query(
        "UPDATE monitor_groups SET name = COALESCE(?, name), sort_order = COALESCE(?, sort_order)
         WHERE id = ? AND org_id = ?",
    )
    .bind(patch.name)
    .bind(patch.sort_order)
    .bind(id.0.to_string())
    .bind(org_id.0.to_string())
    .execute(pool)
    .await?;
    // Reparenting is tri-state (omitted / null / set) → separate UPDATE.
    if let Some(parent) = patch.parent_id {
        if let Some(p) = parent {
            if p == id {
                return Err(DbError::Conflict("folder cannot be its own parent".into()));
            }
            if would_form_group_cycle(pool, id, p).await? {
                return Err(DbError::Conflict("nesting would form a cycle".into()));
            }
        }
        sqlx::query("UPDATE monitor_groups SET parent_id = ? WHERE id = ? AND org_id = ?")
            .bind(parent.map(|g| g.0.to_string()))
            .bind(id.0.to_string())
            .bind(org_id.0.to_string())
            .execute(pool)
            .await?;
    }
    get_in_org(pool, id, org_id).await
}

/// True iff making `new_parent` the parent of `group` would cycle (group is
/// already an ancestor of new_parent). Walks up the parent chain.
pub async fn would_form_group_cycle(
    pool: &MySqlPool,
    group: MonitorGroupId,
    new_parent: MonitorGroupId,
) -> DbResult<bool> {
    let mut current = Some(new_parent);
    let mut seen: HashSet<MonitorGroupId> = HashSet::new();
    while let Some(node) = current {
        if node == group {
            return Ok(true);
        }
        if !seen.insert(node) {
            break;
        }
        let row = sqlx::query("SELECT parent_id FROM monitor_groups WHERE id = ?")
            .bind(node.0.to_string())
            .fetch_optional(pool)
            .await?;
        current = row
            .and_then(|r| r.get::<Option<String>, _>("parent_id"))
            .map(|s| MonitorGroupId::from_uuid(raw_uuid(&s)));
    }
    Ok(false)
}

pub async fn delete(pool: &MySqlPool, id: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
    let r = sqlx::query("DELETE FROM monitor_groups WHERE id = ? AND org_id = ?")
        .bind(id.0.to_string())
        .bind(org_id.0.to_string())
        .execute(pool)
        .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

// ── Dependencies ────────────────────────────────────────────────────────

pub async fn parents_of(pool: &MySqlPool, child: MonitorId) -> DbResult<Vec<MonitorId>> {
    let rows = sqlx::query("SELECT depends_on_id FROM monitor_dependencies WHERE monitor_id = ?")
        .bind(child.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| mid(&r.get::<String, _>("depends_on_id")))
        .collect())
}

pub async fn children_of(pool: &MySqlPool, parent: MonitorId) -> DbResult<Vec<MonitorId>> {
    let rows = sqlx::query("SELECT monitor_id FROM monitor_dependencies WHERE depends_on_id = ?")
        .bind(parent.0.to_string())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| mid(&r.get::<String, _>("monitor_id")))
        .collect())
}

/// Does `child` depend on any monitor that's currently down/pending? Active
/// parents only (a paused parent doesn't suppress). Mirrors PG/SQLite.
pub async fn any_parent_down(pool: &MySqlPool, child: MonitorId) -> DbResult<bool> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM monitor_dependencies d
            JOIN monitors p ON p.id = d.depends_on_id
            WHERE d.monitor_id = ? AND p.active = 1
              AND (p.current_status = 'down' OR p.current_status = 'pending')
        )",
    )
    .bind(child.0.to_string())
    .fetch_one(pool)
    .await?;
    Ok(n != 0)
}

pub async fn attach_dependency(
    pool: &MySqlPool,
    child: MonitorId,
    parent: MonitorId,
) -> DbResult<()> {
    if child == parent {
        return Err(DbError::Conflict("monitor cannot depend on itself".into()));
    }
    if would_form_cycle(pool, child, parent).await? {
        return Err(DbError::Conflict("dependency would form a cycle".into()));
    }
    sqlx::query(
        "INSERT IGNORE INTO monitor_dependencies (monitor_id, depends_on_id) VALUES (?, ?)",
    )
    .bind(child.0.to_string())
    .bind(parent.0.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach_dependency(
    pool: &MySqlPool,
    child: MonitorId,
    parent: MonitorId,
) -> DbResult<()> {
    sqlx::query("DELETE FROM monitor_dependencies WHERE monitor_id = ? AND depends_on_id = ?")
        .bind(child.0.to_string())
        .bind(parent.0.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// BFS up from `parent` following its own parents; hitting `child` = a cycle.
pub async fn would_form_cycle(
    pool: &MySqlPool,
    child: MonitorId,
    parent: MonitorId,
) -> DbResult<bool> {
    let mut seen: HashSet<MonitorId> = HashSet::new();
    let mut frontier: Vec<MonitorId> = vec![parent];
    while let Some(node) = frontier.pop() {
        if node == child {
            return Ok(true);
        }
        if !seen.insert(node) {
            continue;
        }
        frontier.extend(parents_of(pool, node).await?);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mysql::monitors;

    const DEF: &str = "00000000-0000-0000-0000-000000000001";

    async fn mk_monitor(pool: &MySqlPool, org: OrgId, name: &str) -> MonitorId {
        monitors::create(
            pool,
            serde_json::from_value(serde_json::json!({
                "name": name, "kind": "http", "url": "https://x",
                "interval_seconds": 60, "timeout_seconds": 10, "max_retries": 0,
                "retry_interval_sec": 60, "resend_interval_sec": 0, "upside_down": false,
                "http_method": "GET", "accepted_statuses": [200],
                "follow_redirect": true, "ignore_tls": false, "check_cert": false,
                "cert_expiry_days": 14
            }))
            .unwrap(),
            org,
        )
        .await
        .unwrap()
        .id
    }

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn groups_tree_and_dependencies(pool: MySqlPool) {
        let org = super::super::oid(DEF);

        // folders: create parent + child, reparent, cycle guard.
        let root = create(
            &pool,
            NewMonitorGroup {
                name: "Prod".into(),
                sort_order: 0,
                parent_id: None,
            },
            org,
        )
        .await
        .unwrap();
        let child = create(
            &pool,
            NewMonitorGroup {
                name: "API".into(),
                sort_order: 1,
                parent_id: Some(root.id),
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(list(&pool, org).await.unwrap().len(), 2);
        in_org(&pool, child.id, org).await.unwrap();

        // making root a child of its own child would cycle.
        assert!(would_form_group_cycle(&pool, root.id, child.id)
            .await
            .unwrap());
        let u = update(
            &pool,
            root.id,
            UpdateMonitorGroup {
                name: Some("Production".into()),
                sort_order: None,
                parent_id: Some(Some(child.id)),
            },
            org,
        )
        .await;
        assert!(matches!(u, Err(DbError::Conflict(_))));
        // a benign rename still works.
        let r = update(
            &pool,
            root.id,
            UpdateMonitorGroup {
                name: Some("Production".into()),
                sort_order: None,
                parent_id: None,
            },
            org,
        )
        .await
        .unwrap();
        assert_eq!(r.name, "Production");

        // dependencies: build a -> b chain, guard the cycle.
        let a = mk_monitor(&pool, org, "a").await;
        let b = mk_monitor(&pool, org, "b").await;
        attach_dependency(&pool, a, b).await.unwrap();
        assert_eq!(parents_of(&pool, a).await.unwrap(), vec![b]);
        assert_eq!(children_of(&pool, b).await.unwrap(), vec![a]);
        assert!(matches!(
            attach_dependency(&pool, a, a).await,
            Err(DbError::Conflict(_))
        ));
        assert!(matches!(
            attach_dependency(&pool, b, a).await,
            Err(DbError::Conflict(_))
        )); // cycle
            // a new monitor defaults to current_status='pending' + active=1, so the
            // dependency on b suppresses a's alert.
        assert!(any_parent_down(&pool, a).await.unwrap());
        detach_dependency(&pool, a, b).await.unwrap();
        assert!(!any_parent_down(&pool, a).await.unwrap());
        assert!(parents_of(&pool, a).await.unwrap().is_empty());

        // cross-org + delete.
        let other = super::super::orgs::create(&pool, "other", "Other")
            .await
            .unwrap();
        assert!(matches!(
            in_org(&pool, root.id, other.id).await,
            Err(DbError::NotFound)
        ));
        delete(&pool, child.id, org).await.unwrap();
        delete(&pool, root.id, org).await.unwrap();
        assert!(matches!(
            delete(&pool, root.id, org).await,
            Err(DbError::NotFound)
        ));
    }
}
