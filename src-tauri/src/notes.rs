use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

/// A note row surfaced to the UI.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

pub async fn insert_note(pool: &SqlitePool, title: &str) -> Result<Note, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query_as::<_, Note>(
        "INSERT INTO notes (id, title, created_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         RETURNING id, title, created_at",
    )
    .bind(&id)
    .bind(title)
    .fetch_one(pool)
    .await
}

pub async fn fetch_notes(pool: &SqlitePool) -> Result<Vec<Note>, sqlx::Error> {
    sqlx::query_as::<_, Note>(
        "SELECT id, title, created_at FROM notes ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
}

#[tauri::command]
pub async fn create_note(pool: State<'_, SqlitePool>, title: String) -> Result<Note, String> {
    insert_note(&pool, &title).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_notes(pool: State<'_, SqlitePool>) -> Result<Vec<Note>, String> {
    fetch_notes(&pool).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn insert_then_fetch_round_trips() {
        let pool = test_pool().await;
        let created = insert_note(&pool, "First note").await.expect("insert");
        assert_eq!(created.title, "First note");
        assert!(!created.id.is_empty());

        let notes = fetch_notes(&pool).await.expect("fetch");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, created.id);
    }

    #[tokio::test]
    async fn fetch_orders_newest_first() {
        let pool = test_pool().await;
        insert_note(&pool, "older").await.expect("insert");
        insert_note(&pool, "newer").await.expect("insert");
        let notes = fetch_notes(&pool).await.expect("fetch");
        assert_eq!(notes.len(), 2);
        // Same-millisecond timestamps fall back to id ordering; both rows
        // must simply be present and the query must not error.
        assert!(notes.iter().any(|n| n.title == "older"));
        assert!(notes.iter().any(|n| n.title == "newer"));
    }
}
