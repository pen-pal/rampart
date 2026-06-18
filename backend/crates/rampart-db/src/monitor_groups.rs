//! Monitor groups + dependency graph.
//!
//! Groups: trivial CRUD over `monitor_groups`. Deleting a group does NOT
//! delete its monitors (FK is `ON DELETE SET NULL`).
//!
//! Dependencies: many-to-many between `monitors(id)` and itself via
//! `monitor_dependencies`. Helpers expose:
//! * `parents_of(child)` — ids the child waits on
//! * `children_of(parent)` — reverse direction (for UI)
//! * `any_parent_down(child)` — true if any parent's `current_status` is
//!   Down or Pending. Used by the notifier to suppress alerts whose root
//!   cause is upstream.
//! * `would_form_cycle(child, parent)` — guard for attach.

use crate::{DbError, DbPool, DbResult};
use rampart_core::ids::{MonitorGroupId, MonitorId, OrgId};
use rampart_core::monitor_group::{MonitorGroup, NewMonitorGroup, UpdateMonitorGroup};
use std::collections::HashSet;
use time::OffsetDateTime;
use uuid::Uuid;

struct GroupRow {
    id: Uuid,
    name: String,
    sort_order: i32,
    parent_id: Option<Uuid>,
    created_at: OffsetDateTime,
}

impl From<GroupRow> for MonitorGroup {
    fn from(r: GroupRow) -> Self {
        MonitorGroup {
            id: MonitorGroupId::from_uuid(r.id),
            name: r.name,
            sort_order: r.sort_order,
            parent_id: r.parent_id.map(MonitorGroupId::from_uuid),
            created_at: r.created_at,
        }
    }
}

/// 404-gate: succeed only when `group` belongs to `org`. There is no single-id
/// group getter (the UI lists groups per org), so the tag-routing handlers that
/// act on a folder by id gate through this before touching a routing junction.
pub async fn in_org(pool: &DbPool, group: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
    let ok = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM monitor_groups WHERE id = $1 AND org_id = $2)",
        group.0,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    if ok.unwrap_or(false) {
        Ok(())
    } else {
        Err(DbError::NotFound)
    }
}

pub async fn list(pool: &DbPool, org_id: OrgId) -> DbResult<Vec<MonitorGroup>> {
    let rows = sqlx::query_as!(
        GroupRow,
        r#"
        SELECT id, name, sort_order, parent_id, created_at
        FROM monitor_groups
        WHERE org_id = $1
        ORDER BY sort_order, name
        "#,
        org_id.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn create(
    pool: &DbPool,
    input: NewMonitorGroup,
    org_id: rampart_core::ids::OrgId,
) -> DbResult<MonitorGroup> {
    let id = MonitorGroupId::new();
    let row = sqlx::query_as!(
        GroupRow,
        r#"
        INSERT INTO monitor_groups (id, name, sort_order, parent_id, org_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, sort_order, parent_id, created_at
        "#,
        id.0,
        input.name,
        input.sort_order,
        input.parent_id.map(|g| g.0),
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn update(
    pool: &DbPool,
    id: MonitorGroupId,
    patch: UpdateMonitorGroup,
    org_id: OrgId,
) -> DbResult<MonitorGroup> {
    let result = sqlx::query!(
        r#"
        UPDATE monitor_groups
           SET name       = COALESCE($2, name),
               sort_order = COALESCE($3, sort_order)
         WHERE id = $1 AND org_id = $4
        "#,
        id.0,
        patch.name,
        patch.sort_order,
        org_id.0,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    // Reparenting is tri-state (omitted / null / set), so it can't ride the
    // COALESCE above — COALESCE can't tell "leave alone" from "set to null".
    if let Some(parent) = patch.parent_id {
        if let Some(p) = parent {
            if p == id {
                return Err(DbError::Conflict("folder cannot be its own parent".into()));
            }
            if would_form_group_cycle(pool, id, p).await? {
                return Err(DbError::Conflict("nesting would form a cycle".into()));
            }
        }
        sqlx::query!(
            r#"UPDATE monitor_groups SET parent_id = $1 WHERE id = $2 AND org_id = $3"#,
            parent.map(|g| g.0),
            id.0,
            org_id.0,
        )
        .execute(pool)
        .await?;
    }
    let row = sqlx::query_as!(
        GroupRow,
        r#"SELECT id, name, sort_order, parent_id, created_at FROM monitor_groups WHERE id = $1 AND org_id = $2"#,
        id.0,
        org_id.0,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

/// True iff making `new_parent` the parent of `group` would create a cycle
/// — i.e. `group` is already an ancestor of `new_parent`. Walks up the
/// parent chain from `new_parent`; the forest is shallow so this is cheap.
pub async fn would_form_group_cycle(
    pool: &DbPool,
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
            break; // pre-existing cycle (shouldn't happen) — stop walking
        }
        let row = sqlx::query!(
            r#"SELECT parent_id FROM monitor_groups WHERE id = $1"#,
            node.0,
        )
        .fetch_optional(pool)
        .await?;
        current = row.and_then(|r| r.parent_id).map(MonitorGroupId::from_uuid);
    }
    Ok(false)
}

pub async fn delete(pool: &DbPool, id: MonitorGroupId, org_id: OrgId) -> DbResult<()> {
    let result = sqlx::query!(
        "DELETE FROM monitor_groups WHERE id = $1 AND org_id = $2",
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

// ── Dependencies ────────────────────────────────────────────────────────

pub async fn parents_of(pool: &DbPool, child: MonitorId) -> DbResult<Vec<MonitorId>> {
    let rows = sqlx::query!(
        r#"SELECT depends_on_id FROM monitor_dependencies WHERE monitor_id = $1"#,
        child.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MonitorId::from_uuid(r.depends_on_id))
        .collect())
}

pub async fn children_of(pool: &DbPool, parent: MonitorId) -> DbResult<Vec<MonitorId>> {
    let rows = sqlx::query!(
        r#"SELECT monitor_id FROM monitor_dependencies WHERE depends_on_id = $1"#,
        parent.0,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| MonitorId::from_uuid(r.monitor_id))
        .collect())
}

/// True iff any direct parent of `child` is currently Down or Pending.
/// We don't walk the transitive closure — the immediate parent's status
/// is itself influenced by *its* parents through the same suppression
/// rule, so the one-hop check propagates blocking up the chain.
pub async fn any_parent_down(pool: &DbPool, child: MonitorId) -> DbResult<bool> {
    let row = sqlx::query!(
        r#"
        SELECT 1 AS hit
        FROM monitor_dependencies d
        JOIN monitors p ON p.id = d.depends_on_id
        WHERE d.monitor_id = $1
          AND p.active
          AND (p.current_status = 'down' OR p.current_status = 'pending')
        LIMIT 1
        "#,
        child.0,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Adds the edge `child -> parent`. Returns an error if it would form a
/// cycle (the dep graph must remain a DAG so notification suppression
/// can't deadlock).
pub async fn attach_dependency(pool: &DbPool, child: MonitorId, parent: MonitorId) -> DbResult<()> {
    if child == parent {
        return Err(DbError::Conflict("monitor cannot depend on itself".into()));
    }
    if would_form_cycle(pool, child, parent).await? {
        return Err(DbError::Conflict("dependency would form a cycle".into()));
    }
    sqlx::query!(
        r#"INSERT INTO monitor_dependencies (monitor_id, depends_on_id)
           VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
        child.0,
        parent.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn detach_dependency(pool: &DbPool, child: MonitorId, parent: MonitorId) -> DbResult<()> {
    sqlx::query!(
        r#"DELETE FROM monitor_dependencies WHERE monitor_id = $1 AND depends_on_id = $2"#,
        child.0,
        parent.0,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// BFS forward from `parent` following its own parents. If we hit
/// `child` we'd close a cycle.
pub async fn would_form_cycle(
    pool: &DbPool,
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
        let next = parents_of(pool, node).await?;
        frontier.extend(next);
    }
    Ok(false)
}
