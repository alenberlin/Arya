//! Integration tests for the FK + link invariants the schema enforces.
//!
//! These are not unit tests of any single module: they assert the *contract*
//! between migrations (which declare the constraints), the connection pool
//! (`init_pool` / `test_pool` enable `foreign_keys = ON`), and the app-layer
//! graph (`links::delete_for_node` cleans dangling edges on delete). A green
//! run means a delete can never leave a row pointing at a non-existent
//! parent, and a create-link can never silently duplicate.
//!
//! Run with:
//!   cargo test --test db_integrity
//!
//! The harness uses the `db::test_pool()` helper, which opens an in-memory
//! SQLite, runs every migration, and enables foreign keys. That gives us the
//! real production schema without writing a fixture file.

use arya_lib::db::test_pool;
use arya_lib::links;

async fn seed_note(pool: &sqlx::SqlitePool, id: &str, title: &str) {
    sqlx::query(
        "INSERT INTO notes (id, title, created_at) VALUES (?1, ?2, '2024-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(title)
    .execute(pool)
    .await
    .expect("seed note");
}

async fn seed_folder(pool: &sqlx::SqlitePool, id: &str, name: &str) {
    sqlx::query("INSERT INTO folders (id, name, created_at) VALUES (?1, ?2, '2024-01-01T00:00:00Z')")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed folder");
}

// -------- FK enforcement -----------------------------------------------------

#[tokio::test]
async fn deleting_a_note_cascades_to_attachments_sessions_and_turns() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;

    sqlx::query(
        "INSERT INTO note_attachments (id, note_id, name, path, size_bytes, created_at)
         VALUES ('a1', 'n1', 'f.pdf', '/tmp/f.pdf', 10, '2024-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO recording_sessions
            (id, note_id, status, source_mode, sample_rate, channels, started_at, updated_at)
         VALUES ('s1', 'n1', 'finished', 'microphone-only', 16000, 1,
                 '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO transcript_turns (id, note_id, source, turn_index, start_ms, end_ms, text)
         VALUES ('t1', 'n1', 'microphone', 0, 0, 1000, 'hello')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM notes WHERE id = 'n1'")
        .execute(&pool)
        .await
        .unwrap();

    for table in ["note_attachments", "recording_sessions", "transcript_turns"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "CASCADE did not clear {table}");
    }
}

#[tokio::test]
async fn inserting_with_a_missing_parent_is_rejected_by_fk() {
    let pool = test_pool().await;
    // note_id 'ghost' does not exist; FK should reject.
    let err = sqlx::query(
        "INSERT INTO note_attachments (id, note_id, name, path, size_bytes, created_at)
         VALUES ('a1', 'ghost', 'f.pdf', '/tmp/f.pdf', 10, '2024-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect_err("expected FK error");
    let msg = err.to_string();
    assert!(
        msg.contains("FOREIGN KEY") || msg.contains("foreign key"),
        "got unexpected error: {msg}"
    );
}

#[tokio::test]
async fn deleting_a_folder_nulls_notes_via_set_null() {
    let pool = test_pool().await;
    seed_folder(&pool, "f1", "inbox").await;
    seed_note(&pool, "n1", "alpha").await;
    sqlx::query("UPDATE notes SET folder_id = 'f1' WHERE id = 'n1'")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("DELETE FROM folders WHERE id = 'f1'")
        .execute(&pool)
        .await
        .unwrap();

    let folder: Option<String> = sqlx::query_scalar("SELECT folder_id FROM notes WHERE id = 'n1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        folder.is_none(),
        "ON DELETE SET NULL should null notes.folder_id (was {folder:?})"
    );
}

// -------- Link invariants ----------------------------------------------------

#[tokio::test]
async fn link_insert_is_idempotent_for_same_relation() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;
    seed_note(&pool, "n2", "beta").await;

    let a = links::insert_link(&pool, "note", "n1", "note", "n2", "mention", "user", 1.0)
        .await
        .unwrap();
    let b = links::insert_link(&pool, "note", "n1", "note", "n2", "mention", "user", 1.0)
        .await
        .unwrap();
    assert_eq!(a.id, b.id, "upsert must preserve the original id");
    assert_eq!(a.created_at, b.created_at, "created_at must be preserved");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "duplicate relation must collapse to one row");
}

#[tokio::test]
async fn distinct_relations_between_same_pair_coexist() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;
    seed_note(&pool, "n2", "beta").await;

    links::insert_link(&pool, "note", "n1", "note", "n2", "mention", "user", 1.0)
        .await
        .unwrap();
    links::insert_link(&pool, "note", "n1", "note", "n2", "semantic", "agent", 0.8)
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2, "different relations must coexist");
}

#[tokio::test]
async fn reconcile_replaces_only_one_relation_and_keeps_others() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;
    seed_note(&pool, "n2", "beta").await;
    seed_note(&pool, "n3", "gamma").await;

    // mention: n1 -> n2
    links::insert_link(&pool, "note", "n1", "note", "n2", "mention", "user", 1.0)
        .await
        .unwrap();
    // semantic: n1 -> n2 (kept across the reconcile)
    links::insert_link(&pool, "note", "n1", "note", "n2", "semantic", "agent", 0.5)
        .await
        .unwrap();

    links::reconcile_source_links(
        &pool,
        "note",
        "n1",
        "mention",
        &[("note".into(), "n3".into())],
    )
    .await
    .unwrap();

    let from = links::links_from(&pool, "note", "n1").await.unwrap();
    let relations: Vec<&str> = from.iter().map(|l| l.relation.as_str()).collect();
    assert_eq!(relations, vec!["semantic", "mention"]);
    let targets: Vec<&str> = from.iter().map(|l| l.target_id.as_str()).collect();
    assert_eq!(targets, vec!["n2", "n3"]);
}

#[tokio::test]
async fn reconcile_skips_invalid_targets_without_aborting() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;
    seed_note(&pool, "n2", "beta").await;

    // Mix of: valid, unknown kind, empty id, self-loop.
    links::reconcile_source_links(
        &pool,
        "note",
        "n1",
        "mention",
        &[
            ("note".into(), "n2".into()),
            ("widget".into(), "x".into()), // unknown kind, dropped
            ("note".into(), "".into()),    // empty id, dropped
            ("note".into(), "n1".into()),  // self-loop, dropped
        ],
    )
    .await
    .unwrap();

    let from = links::links_from(&pool, "note", "n1").await.unwrap();
    assert_eq!(from.len(), 1, "only the valid target should land");
    assert_eq!(from[0].target_id, "n2");
}

#[tokio::test]
async fn delete_for_node_removes_every_touching_edge() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;
    seed_note(&pool, "n2", "beta").await;
    seed_note(&pool, "n3", "gamma").await;

    // n1 in three places: source of one, target of two.
    links::insert_link(&pool, "note", "n1", "note", "n2", "mention", "user", 1.0)
        .await
        .unwrap();
    links::insert_link(&pool, "note", "n3", "note", "n1", "mention", "user", 1.0)
        .await
        .unwrap();
    links::insert_link(&pool, "note", "n2", "note", "n1", "semantic", "agent", 0.7)
        .await
        .unwrap();
    // Edge that does NOT touch n1 — must survive.
    links::insert_link(&pool, "note", "n2", "note", "n3", "mention", "user", 1.0)
        .await
        .unwrap();

    links::delete_for_node(&pool, "note", "n1").await.unwrap();

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 1, "exactly the n2->n3 edge should remain");
    let survivor = links::links_from(&pool, "note", "n2").await.unwrap();
    assert_eq!(survivor[0].target_id, "n3");
}

#[tokio::test]
async fn validate_edge_rejects_unknown_kinds_and_self_loops() {
    // The bare `validate_edge` is private; the *behavior* is observed
    // through `create_link` and `reconcile_source_links`. This test pins
    // NODE_KINDS so a kind added without bumping the constant is caught at
    // CI time (otherwise validation silently accepts it).
    assert_eq!(links::NODE_KINDS.len(), 4);
    for k in ["note", "dictation", "meeting", "mindmap"] {
        assert!(links::NODE_KINDS.contains(&k), "missing kind: {k}");
    }
}

// -------- Self-loop guard via reconcile -------------------------------------

#[tokio::test]
async fn self_loop_is_skipped_on_reconcile() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha").await;

    links::reconcile_source_links(
        &pool,
        "note",
        "n1",
        "mention",
        &[("note".into(), "n1".into())],
    )
    .await
    .unwrap();

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM links")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 0, "self-loop must not be persisted");
}

// -------- Search LIKE escape -------------------------------------------------

#[tokio::test]
async fn search_like_query_is_safe_against_wildcards_and_quotes() {
    let pool = test_pool().await;
    seed_note(&pool, "n1", "alpha bravo").await;
    seed_note(&pool, "n2", "100% complete").await;
    seed_note(&pool, "n3", "under_score").await;
    seed_note(&pool, "n4", "with 'apostrophe' and \"quote\"").await;

    // `%` and `_` are LIKE wildcards; the search path must escape them so
    // they don't match every row. With escape, only the literal row hits.
    let pct = arya_lib::notes::search_notes_query(&pool, "%").await.unwrap();
    assert_eq!(pct.len(), 1, "only the row literally containing %");
    assert_eq!(pct[0].id, "n2");

    let under = arya_lib::notes::search_notes_query(&pool, "_").await.unwrap();
    assert_eq!(under.len(), 1, "only the row literally containing _");
    assert_eq!(under[0].id, "n3");

    // Apostrophes / quotes must not break the LIKE pattern.
    let apos = arya_lib::notes::search_notes_query(&pool, "'").await.unwrap();
    assert_eq!(apos.len(), 1);
    assert_eq!(apos[0].id, "n4");

    // No match → empty list, not an error.
    let none = arya_lib::notes::search_notes_query(&pool, "absent-zzzz-no")
        .await
        .unwrap();
    assert!(none.is_empty());
}