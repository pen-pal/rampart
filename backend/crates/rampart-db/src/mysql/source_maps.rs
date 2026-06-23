//! MySQL `source_maps` domain — source-map storage for error-tier symbolication.
//! Ported from PG. `release` is a reserved word → backticked. JSONB→LONGTEXT;
//! BIGSERIAL→BIGINT AUTO_INCREMENT; `ON CONFLICT DO UPDATE … RETURNING id` →
//! `ON DUPLICATE KEY UPDATE` then re-select id by the unique key (LAST_INSERT_ID
//! is unreliable across the upsert path).

use super::ts;
use crate::source_maps::{NewSourceMap, SourceMapMeta};
use crate::DbResult;
use sqlx::{MySqlPool, Row};
use uuid::Uuid;

/// Insert or replace the map for a `(project, release, filename)`; returns id.
pub async fn upsert(pool: &MySqlPool, m: NewSourceMap<'_>) -> DbResult<i64> {
    let map_text = serde_json::to_string(&m.map).unwrap_or_else(|_| "null".into());
    sqlx::query(
        "INSERT INTO source_maps (project_id, `release`, filename, map)
         VALUES (?, ?, ?, ?)
         ON DUPLICATE KEY UPDATE map = VALUES(map), uploaded_at = UNIX_TIMESTAMP()",
    )
    .bind(m.project_id.to_string())
    .bind(m.release)
    .bind(m.filename)
    .bind(map_text)
    .execute(pool)
    .await?;
    let (id,): (i64,) = sqlx::query_as(
        "SELECT id FROM source_maps WHERE project_id = ? AND `release` = ? AND filename = ?",
    )
    .bind(m.project_id.to_string())
    .bind(m.release)
    .bind(m.filename)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn get(
    pool: &MySqlPool,
    project_id: Uuid,
    release: &str,
    filename: &str,
) -> DbResult<Option<serde_json::Value>> {
    let row = sqlx::query(
        "SELECT map FROM source_maps WHERE project_id = ? AND `release` = ? AND filename = ?",
    )
    .bind(project_id.to_string())
    .bind(release)
    .bind(filename)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|r| serde_json::from_str(&r.get::<String, _>("map")).ok()))
}

pub async fn list(pool: &MySqlPool, project_id: Uuid) -> DbResult<Vec<SourceMapMeta>> {
    let rows = sqlx::query(
        "SELECT id, `release`, filename, uploaded_at FROM source_maps
         WHERE project_id = ? ORDER BY `release`, filename",
    )
    .bind(project_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| SourceMapMeta {
            id: r.get::<i64, _>("id"),
            release: r.get("release"),
            filename: r.get("filename"),
            uploaded_at: ts(r.get::<i64, _>("uploaded_at")),
        })
        .collect())
}

pub async fn delete(pool: &MySqlPool, project_id: Uuid, id: i64) -> DbResult<bool> {
    let r = sqlx::query("DELETE FROM source_maps WHERE id = ? AND project_id = ?")
        .bind(id)
        .bind(project_id.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../../migrations-mysql")]
    async fn upsert_get_list_delete(pool: MySqlPool) {
        let proj = Uuid::now_v7();
        let id = upsert(
            &pool,
            NewSourceMap {
                project_id: proj,
                release: "1.0.0",
                filename: "main.abc.js",
                map: serde_json::json!({ "version": 3, "sources": ["a.ts"] }),
            },
        )
        .await
        .unwrap();
        assert!(id > 0);

        // get returns the stored map.
        let got = get(&pool, proj, "1.0.0", "main.abc.js")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got["version"], 3);
        // miss → None.
        assert!(get(&pool, proj, "9.9.9", "x.js").await.unwrap().is_none());

        // upsert same key → same id, replaced body.
        let id2 = upsert(
            &pool,
            NewSourceMap {
                project_id: proj,
                release: "1.0.0",
                filename: "main.abc.js",
                map: serde_json::json!({ "version": 3, "sources": ["b.ts"] }),
            },
        )
        .await
        .unwrap();
        assert_eq!(id2, id);
        assert_eq!(
            get(&pool, proj, "1.0.0", "main.abc.js")
                .await
                .unwrap()
                .unwrap()["sources"][0],
            "b.ts"
        );

        // list (metadata only).
        let metas = list(&pool, proj).await.unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].release, "1.0.0");

        // delete (project-scoped).
        assert!(delete(&pool, proj, id).await.unwrap());
        assert!(!delete(&pool, proj, id).await.unwrap());
        assert!(list(&pool, proj).await.unwrap().is_empty());
    }
}
