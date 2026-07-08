//! The connected-brain edge store (F1).
//!
//! One polymorphic table of directed edges between nodes. A *node* is any
//! first-class item in the brain, named by `(kind, id)`: a note, a dictation, a
//! meeting, or a mind map. Edges are **not** enforced by SQL foreign keys — a
//! target may be of any kind and a dangling target is permitted (resolved at
//! read time) — so referential cleanup when a node is deleted happens here, in
//! the app layer, via [`delete_for_node`] / [`delete_for_kind`].
//!
//! Creating an edge is **idempotent**: the same `(source, target, relation)`
//! collapses to one row (the insert upserts), so reconciling a document's
//! mentions on every save never duplicates. Distinct relations between the same
//! pair coexist (a note can both `mention` and be `semantic`ally near another).

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

/// The node kinds an edge may connect — the graph's type system. Validated at
/// the command boundary so a typo can't silently create an unresolvable edge.
pub const NODE_KINDS: [&str; 4] = ["note", "dictation", "meeting", "mindmap"];

fn valid_kind(kind: &str) -> bool {
    NODE_KINDS.contains(&kind)
}

/// Reject edges that can never resolve: an unknown kind, an empty id, or a
/// self-loop (a node linking to itself carries no information).
fn validate_edge(
    source_kind: &str,
    source_id: &str,
    target_kind: &str,
    target_id: &str,
) -> Result<(), String> {
    if !valid_kind(source_kind) {
        return Err(format!("unknown source kind: {source_kind}"));
    }
    if !valid_kind(target_kind) {
        return Err(format!("unknown target kind: {target_kind}"));
    }
    if source_id.trim().is_empty() || target_id.trim().is_empty() {
        return Err("node id is required".into());
    }
    if source_kind == target_kind && source_id == target_id {
        return Err("a node cannot link to itself".into());
    }
    Ok(())
}

/// A directed edge between two nodes.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: String,
    pub source_kind: String,
    pub source_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub relation: String,
    pub origin: String,
    pub weight: f64,
    pub created_at: String,
}

const SELECT_COLS: &str =
    "id, source_kind, source_id, target_kind, target_id, relation, origin, weight, created_at";

/// Insert an edge, or return the existing one for the same
/// `(source, target, relation)` — creating a link is idempotent. On a repeat the
/// stored `weight`/`origin` are refreshed while the original `id`/`created_at`
/// are preserved, so callers always receive the canonical edge.
#[allow(clippy::too_many_arguments)]
pub async fn insert_link(
    pool: &SqlitePool,
    source_kind: &str,
    source_id: &str,
    target_kind: &str,
    target_id: &str,
    relation: &str,
    origin: &str,
    weight: f64,
) -> Result<Link, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query_as::<_, Link>(
        "INSERT INTO links
             (id, source_kind, source_id, target_kind, target_id, relation, origin, weight, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(source_kind, source_id, target_kind, target_id, relation)
             DO UPDATE SET weight = excluded.weight, origin = excluded.origin
         RETURNING id, source_kind, source_id, target_kind, target_id, relation, origin, weight, created_at",
    )
    .bind(&id)
    .bind(source_kind)
    .bind(source_id)
    .bind(target_kind)
    .bind(target_id)
    .bind(relation)
    .bind(origin)
    .bind(weight)
    .fetch_one(pool)
    .await
}

/// Outbound edges from a node (what it links to).
pub async fn links_from(pool: &SqlitePool, kind: &str, id: &str) -> Result<Vec<Link>, sqlx::Error> {
    sqlx::query_as::<_, Link>(&format!(
        "SELECT {SELECT_COLS} FROM links
         WHERE source_kind = ?1 AND source_id = ?2
         ORDER BY created_at DESC, rowid DESC"
    ))
    .bind(kind)
    .bind(id)
    .fetch_all(pool)
    .await
}

/// Inbound edges to a node — its backlinks (what links to it).
pub async fn links_to(pool: &SqlitePool, kind: &str, id: &str) -> Result<Vec<Link>, sqlx::Error> {
    sqlx::query_as::<_, Link>(&format!(
        "SELECT {SELECT_COLS} FROM links
         WHERE target_kind = ?1 AND target_id = ?2
         ORDER BY created_at DESC, rowid DESC"
    ))
    .bind(kind)
    .bind(id)
    .fetch_all(pool)
    .await
}

/// Delete one edge by id.
pub async fn delete_link_by_id(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM links WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Delete every edge touching a node (either endpoint). Called when a node is
/// deleted so the graph never keeps a dangling edge.
pub async fn delete_for_node(pool: &SqlitePool, kind: &str, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM links
         WHERE (source_kind = ?1 AND source_id = ?2)
            OR (target_kind = ?1 AND target_id = ?2)",
    )
    .bind(kind)
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Delete every edge touching *any* node of a kind. Called when a whole kind is
/// cleared at once (e.g. "delete all notes").
pub async fn delete_for_kind(pool: &SqlitePool, kind: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM links WHERE source_kind = ?1 OR target_kind = ?1")
        .bind(kind)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Create (or idempotently return) a **user-initiated** edge between two nodes.
/// Origin is always `user` and the weight is the default here; semantic and
/// agent edges (other origins, weighted) are created server-side via
/// [`insert_link`], never from the client.
#[tauri::command]
pub async fn create_link(
    pool: State<'_, SqlitePool>,
    source_kind: String,
    source_id: String,
    target_kind: String,
    target_id: String,
    relation: Option<String>,
) -> Result<Link, String> {
    validate_edge(&source_kind, &source_id, &target_kind, &target_id)?;
    let relation = relation.unwrap_or_else(|| "mention".into());
    if relation.trim().is_empty() {
        return Err("relation is required".into());
    }
    insert_link(
        &pool,
        &source_kind,
        &source_id,
        &target_kind,
        &target_id,
        &relation,
        "user",
        1.0,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Outbound edges from a node.
#[tauri::command]
pub async fn list_links_from(
    pool: State<'_, SqlitePool>,
    kind: String,
    id: String,
) -> Result<Vec<Link>, String> {
    if !valid_kind(&kind) {
        return Err(format!("unknown node kind: {kind}"));
    }
    links_from(&pool, &kind, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Inbound edges to a node — its backlinks.
#[tauri::command]
pub async fn list_links_to(
    pool: State<'_, SqlitePool>,
    kind: String,
    id: String,
) -> Result<Vec<Link>, String> {
    if !valid_kind(&kind) {
        return Err(format!("unknown node kind: {kind}"));
    }
    links_to(&pool, &kind, &id).await.map_err(|e| e.to_string())
}

/// Delete one edge by id.
#[tauri::command]
pub async fn delete_link(pool: State<'_, SqlitePool>, id: String) -> Result<(), String> {
    delete_link_by_id(&pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::notes::insert_note;

    async fn note(pool: &SqlitePool, title: &str) -> String {
        insert_note(pool, title).await.unwrap().id
    }

    async fn link(pool: &SqlitePool, from: &str, to: &str, relation: &str) -> Link {
        insert_link(pool, "note", from, "note", to, relation, "user", 1.0)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn insert_then_read_round_trips_both_directions() {
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        let b = note(&pool, "B").await;
        link(&pool, &a, &b, "mention").await;

        let out = links_from(&pool, "note", &a).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_id, b);
        assert_eq!(out[0].relation, "mention");

        let back = links_to(&pool, "note", &b).await.unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].source_id, a, "backlink resolves the source");
    }

    #[tokio::test]
    async fn insert_is_idempotent_per_relation() {
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        let b = note(&pool, "B").await;
        let first = link(&pool, &a, &b, "mention").await;
        let second = link(&pool, &a, &b, "mention").await;
        assert_eq!(first.id, second.id, "same edge returns the canonical row");
        assert_eq!(links_from(&pool, "note", &a).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn distinct_relations_between_the_same_pair_coexist() {
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        let b = note(&pool, "B").await;
        link(&pool, &a, &b, "mention").await;
        link(&pool, &a, &b, "semantic").await;
        assert_eq!(links_from(&pool, "note", &a).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn delete_link_removes_the_edge() {
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        let b = note(&pool, "B").await;
        let edge = link(&pool, &a, &b, "mention").await;
        delete_link_by_id(&pool, &edge.id).await.unwrap();
        assert!(links_from(&pool, "note", &a).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_for_node_removes_edges_on_both_sides() {
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        let b = note(&pool, "B").await;
        let c = note(&pool, "C").await;
        link(&pool, &a, &b, "mention").await; // a is a source
        link(&pool, &c, &a, "mention").await; // a is a target
        delete_for_node(&pool, "note", &a).await.unwrap();
        assert!(links_from(&pool, "note", &a).await.unwrap().is_empty());
        assert!(links_to(&pool, "note", &a).await.unwrap().is_empty());
        // The unrelated endpoints' rows are gone with the edges, but c→? is empty
        // only because its single edge pointed at a.
        assert!(links_from(&pool, "note", &c).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn dangling_target_is_permitted() {
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        // No such note exists for the target; there is no FK, so this is allowed
        // and resolved (or shown as deleted) at read time.
        link(&pool, &a, "ghost-node-id", "mention").await;
        let out = links_from(&pool, "note", &a).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target_id, "ghost-node-id");
    }

    #[tokio::test]
    async fn deleting_a_note_cleans_its_links() {
        // Proves the referential-cleanup wiring in notes::delete_note_inner.
        let pool = test_pool().await;
        let a = note(&pool, "A").await;
        let b = note(&pool, "B").await;
        link(&pool, &a, &b, "mention").await;
        crate::notes::delete_note_inner(&pool, &a).await.unwrap();
        assert!(
            links_to(&pool, "note", &b).await.unwrap().is_empty(),
            "the deleted note's outbound edge is gone"
        );
    }

    #[test]
    fn validate_edge_rejects_bad_edges() {
        assert!(validate_edge("note", "a", "note", "b").is_ok());
        assert!(
            validate_edge("bogus", "a", "note", "b").is_err(),
            "unknown kind"
        );
        assert!(
            validate_edge("note", "a", "note", "a").is_err(),
            "self-loop"
        );
        assert!(validate_edge("note", "", "note", "b").is_err(), "empty id");
    }
}
