//! The Galaxy knowledge-graph assembly (F10).
//!
//! Builds a node+edge graph of the connected brain from the durable *documents*
//! a person actually keeps: notes (a "meeting" is a note with a transcript —
//! same kind) and mind maps. Raw dictation-history rows are deliberately NOT
//! nodes: that table is a transient capture log (every phrase ever spoken,
//! including one-word tests), so surfacing each as a star buried the real
//! documents in noise. A dictation worth keeping becomes a note. Edges come
//! from three sources: `mention`/manual edges in the `links` table (the
//! `@`-mentions), structural `child` edges from note nesting, and —
//! best-effort, only when the local embeddings exist — `semantic` edges from
//! cosine similarity over `rag_chunks` (per-node averaged vector, top-K
//! neighbours). Without an Ollama reindex the semantic layer is simply empty;
//! the mention/structural graph always renders. Node ids are composite:
//! `"<kind>:<uuid>"`.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::vecmath::{blob_to_f32, cosine};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// How many nearest neighbours each node links to semantically, and the minimum
/// cosine similarity — a per-node top-K window (not a global threshold) so a
/// homogeneous corpus doesn't collapse into a fully-connected "hairball".
const SEMANTIC_TOP_K: usize = 3;
const SEMANTIC_MIN: f32 = 0.75;

pub async fn assemble_graph(pool: &SqlitePool) -> Result<Graph, sqlx::Error> {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut node_ids: HashSet<String> = HashSet::new();

    let notes: Vec<(String, String)> = sqlx::query_as("SELECT id, title FROM notes")
        .fetch_all(pool)
        .await?;
    for (id, title) in notes {
        let cid = format!("note:{id}");
        node_ids.insert(cid.clone());
        let label = if title.trim().is_empty() {
            "Untitled".to_string()
        } else {
            title
        };
        nodes.push(GraphNode {
            id: cid,
            kind: "note".into(),
            label,
        });
    }

    let mindmaps: Vec<(String, String)> = sqlx::query_as("SELECT id, title FROM mindmaps")
        .fetch_all(pool)
        .await?;
    for (id, title) in mindmaps {
        let cid = format!("mindmap:{id}");
        node_ids.insert(cid.clone());
        let label = if title.trim().is_empty() {
            "Untitled map".to_string()
        } else {
            title
        };
        nodes.push(GraphNode {
            id: cid,
            kind: "mindmap".into(),
            label,
        });
    }

    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut seen: HashSet<(String, String, String)> = HashSet::new();

    // `links` table edges (mentions, manual, agent, …).
    let links: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT source_kind, source_id, target_kind, target_id, relation FROM links",
    )
    .fetch_all(pool)
    .await?;
    for (sk, si, tk, ti, relation) in links {
        let s = format!("{sk}:{si}");
        let t = format!("{tk}:{ti}");
        if s != t
            && node_ids.contains(&s)
            && node_ids.contains(&t)
            && seen.insert((s.clone(), t.clone(), relation.clone()))
        {
            edges.push(GraphEdge {
                source: s,
                target: t,
                relation,
            });
        }
    }

    // Structural: note nesting → parent → child.
    let nesting: Vec<(String, String)> =
        sqlx::query_as("SELECT parent_note_id, id FROM notes WHERE parent_note_id IS NOT NULL")
            .fetch_all(pool)
            .await?;
    for (parent, child) in nesting {
        let s = format!("note:{parent}");
        let t = format!("note:{child}");
        if node_ids.contains(&s)
            && node_ids.contains(&t)
            && seen.insert((s.clone(), t.clone(), "child".to_string()))
        {
            edges.push(GraphEdge {
                source: s,
                target: t,
                relation: "child".into(),
            });
        }
    }

    add_semantic_edges(pool, &node_ids, &mut edges, &mut seen).await?;
    Ok(Graph { nodes, edges })
}

/// Add best-effort semantic edges between notes. No-op when there are no note
/// embeddings (the local index hasn't been built), so the graph degrades to
/// structural + mention edges offline.
async fn add_semantic_edges(
    pool: &SqlitePool,
    node_ids: &HashSet<String>,
    edges: &mut Vec<GraphEdge>,
    seen: &mut HashSet<(String, String, String)>,
) -> Result<(), sqlx::Error> {
    let chunks: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT source_kind, source_id, embedding FROM rag_chunks
         WHERE source_kind = 'note'",
    )
    .fetch_all(pool)
    .await?;

    // Average each node's chunk embeddings into one vector.
    let mut sums: HashMap<String, (Vec<f32>, usize)> = HashMap::new();
    for (kind, id, blob) in chunks {
        let cid = format!("{kind}:{id}");
        if !node_ids.contains(&cid) {
            continue;
        }
        let v = blob_to_f32(&blob);
        if v.is_empty() {
            continue;
        }
        let entry = sums.entry(cid).or_insert_with(|| (vec![0.0; v.len()], 0));
        if entry.0.len() == v.len() {
            for (acc, x) in entry.0.iter_mut().zip(v.iter()) {
                *acc += x;
            }
            entry.1 += 1;
        }
    }
    let vectors: Vec<(String, Vec<f32>)> = sums
        .into_iter()
        .filter(|(_, (_, n))| *n > 0)
        .map(|(id, (sum, n))| (id, sum.iter().map(|x| x / n as f32).collect()))
        .collect();

    // Link each node to its top-K nearest neighbours (undirected, deduped).
    for (i, (id_a, va)) in vectors.iter().enumerate() {
        let mut sims: Vec<(f32, &String)> = vectors
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, (id_b, vb))| (cosine(va, vb), id_b))
            .filter(|(s, _)| *s >= SEMANTIC_MIN)
            .collect();
        sims.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, id_b) in sims.into_iter().take(SEMANTIC_TOP_K) {
            let (s, t) = if id_a < id_b {
                (id_a.clone(), id_b.clone())
            } else {
                (id_b.clone(), id_a.clone())
            };
            if seen.insert((s.clone(), t.clone(), "semantic".to_string())) {
                edges.push(GraphEdge {
                    source: s,
                    target: t,
                    relation: "semantic".into(),
                });
            }
        }
    }
    Ok(())
}

/// Assemble the Galaxy graph for the whole workspace (F10).
#[tauri::command]
pub async fn galaxy_graph(pool: State<'_, SqlitePool>) -> Result<Graph, String> {
    assemble_graph(&pool).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[tokio::test]
    async fn assembles_nodes_and_edges_from_links_and_nesting() {
        let pool = test_pool().await;
        let a = crate::notes::insert_note(&pool, "A").await.unwrap();
        let b = crate::notes::insert_note_under(&pool, "B", Some(&a.id))
            .await
            .unwrap();
        crate::links::insert_link(&pool, "note", &a.id, "note", &b.id, "mention", "user", 1.0)
            .await
            .unwrap();

        let g = assemble_graph(&pool).await.unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert!(g.nodes.iter().all(|n| n.kind == "note"));
        assert!(
            g.edges
                .iter()
                .any(|e| e.relation == "mention" && e.source == format!("note:{}", a.id)),
            "mention edge from the links table"
        );
        assert!(
            g.edges.iter().any(|e| e.relation == "child"),
            "structural nesting edge"
        );
        // No embeddings in the test DB → no semantic edges.
        assert!(!g.edges.iter().any(|e| e.relation == "semantic"));
    }

    #[tokio::test]
    async fn mindmaps_are_nodes_but_dictation_history_is_not() {
        let pool = test_pool().await;
        let note = crate::notes::insert_note(&pool, "Real note").await.unwrap();
        sqlx::query(
            "INSERT INTO mindmaps (id, title, doc_json, created_at, updated_at)
             VALUES ('m1', 'My map', '', '2026-01-01', '2026-01-01')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // A transient dictation-history row is a capture-log entry, not a document.
        sqlx::query(
            "INSERT INTO dictation_history (id, raw_text, clean_text, duration_ms, asr_ms, created_at)
             VALUES ('d1', 'test', 'test', 100, 50, '2026-01-01')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let g = assemble_graph(&pool).await.unwrap();
        assert!(g.nodes.iter().any(|n| n.id == format!("note:{}", note.id)));
        assert!(
            g.nodes
                .iter()
                .any(|n| n.kind == "mindmap" && n.id == "mindmap:m1"),
            "mind maps are documents and should be stars"
        );
        assert!(
            !g.nodes.iter().any(|n| n.kind == "dictation"),
            "dictation-history rows must never become stars"
        );
    }
}
