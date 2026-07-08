//! Mind maps (F11/M12): a React Flow canvas per map, stored as one opaque JSON
//! document (`doc_json` = nodes + edges + viewport). The app treats the document
//! as a blob — React Flow owns its shape — and keeps only the title separate for
//! listing. Single-user, single-writer: the frontend debounces saves.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

/// Row for the map list.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MindmapSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

/// A full mind map, including its canvas document.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Mindmap {
    pub id: String,
    pub title: String,
    pub doc_json: String,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn insert_mindmap(pool: &SqlitePool, title: &str) -> Result<Mindmap, sqlx::Error> {
    sqlx::query_as::<_, Mindmap>(
        "INSERT INTO mindmaps (id, title, doc_json, created_at, updated_at)
         VALUES (?1, ?2, '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, doc_json, created_at, updated_at",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(title)
    .fetch_one(pool)
    .await
}

pub async fn fetch_mindmaps(pool: &SqlitePool) -> Result<Vec<MindmapSummary>, sqlx::Error> {
    sqlx::query_as::<_, MindmapSummary>(
        "SELECT id, title, updated_at FROM mindmaps ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn fetch_mindmap(pool: &SqlitePool, id: &str) -> Result<Mindmap, sqlx::Error> {
    sqlx::query_as::<_, Mindmap>(
        "SELECT id, title, doc_json, created_at, updated_at FROM mindmaps WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Patch a map's title and/or document. `None` leaves a field unchanged.
pub async fn update_mindmap_fields(
    pool: &SqlitePool,
    id: &str,
    title: Option<&str>,
    doc_json: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE mindmaps SET
             title = COALESCE(?2, title),
             doc_json = COALESCE(?3, doc_json),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(id)
    .bind(title)
    .bind(doc_json)
    .execute(pool)
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn create_mindmap(pool: State<'_, SqlitePool>, title: String) -> Result<Mindmap, String> {
    let title = if title.trim().is_empty() {
        "New mind map".to_string()
    } else {
        title
    };
    insert_mindmap(&pool, &title)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_mindmaps(pool: State<'_, SqlitePool>) -> Result<Vec<MindmapSummary>, String> {
    fetch_mindmaps(&pool).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_mindmap(pool: State<'_, SqlitePool>, id: String) -> Result<Mindmap, String> {
    fetch_mindmap(&pool, &id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_mindmap(
    pool: State<'_, SqlitePool>,
    id: String,
    title: Option<String>,
    doc_json: Option<String>,
) -> Result<(), String> {
    update_mindmap_fields(&pool, &id, title.as_deref(), doc_json.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_mindmap(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    // A mind map is a graph node; drop its edges too so nothing dangles.
    let _ = crate::links::delete_for_node(&pool, "mindmap", &id).await;
    sqlx::query("DELETE FROM mindmaps WHERE id = ?1")
        .bind(id)
        .execute(&*pool)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn mindmap_crud_round_trips() {
        let pool = test_pool().await;
        let map = insert_mindmap(&pool, "Ideas").await.unwrap();
        assert_eq!(map.title, "Ideas");
        assert_eq!(map.doc_json, "");

        update_mindmap_fields(&pool, &map.id, Some("Ideas v2"), Some(r#"{"nodes":[]}"#))
            .await
            .unwrap();
        let loaded = fetch_mindmap(&pool, &map.id).await.unwrap();
        assert_eq!(loaded.title, "Ideas v2");
        assert_eq!(loaded.doc_json, r#"{"nodes":[]}"#);

        assert_eq!(fetch_mindmaps(&pool).await.unwrap().len(), 1);

        // Deleting also clears its graph edges.
        crate::links::insert_link(
            &pool, "mindmap", &map.id, "note", "n1", "manual", "user", 1.0,
        )
        .await
        .unwrap();
        delete_mindmap_inner(&pool, &map.id).await;
        assert!(fetch_mindmaps(&pool).await.unwrap().is_empty());
        assert!(crate::links::links_from(&pool, "mindmap", &map.id)
            .await
            .unwrap()
            .is_empty());
    }

    // Testable core of the delete command (which takes State).
    async fn delete_mindmap_inner(pool: &SqlitePool, id: &str) {
        let _ = crate::links::delete_for_node(pool, "mindmap", id).await;
        sqlx::query("DELETE FROM mindmaps WHERE id = ?1")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
}
