//! Local workspace RAG: on-device embeddings over notes, transcripts,
//! dictation history, and agent sessions; brute-force cosine search.
//!
//! Embeddings come from a local Ollama model (nomic-embed-text, 768-dim) so
//! nothing leaves the Mac. Search is exact cosine over stored vectors -
//! trivially fast at personal-workspace scale and free of any SQLite
//! extension. The [`Embedder`] trait keeps the vector store swappable if
//! scale ever demands an ANN index.

pub mod commands;
pub mod embed;

use serde::Serialize;

/// A retrievable chunk with its similarity to a query.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub content: String,
    pub score: f32,
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

pub fn f32_to_blob(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn blob_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Splits text into overlapping word-window chunks for indexing.
pub fn chunk_text(text: &str, words_per_chunk: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    if words.len() <= words_per_chunk {
        return vec![words.join(" ")];
    }
    let step = words_per_chunk.saturating_sub(overlap).max(1);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let end = (start + words_per_chunk).min(words.len());
        chunks.push(words[start..end].join(" "));
        if end == words.len() {
            break;
        }
        start += step;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_is_one() {
        let v = vec![0.3, 0.4, 0.5];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    }

    #[test]
    fn cosine_mismatched_len_is_zero() {
        assert_eq!(cosine(&[1.0, 2.0], &[1.0]), 0.0);
    }

    #[test]
    fn blob_round_trips() {
        let v = vec![1.5f32, -2.25, 0.0, 3.75];
        assert_eq!(blob_to_f32(&f32_to_blob(&v)), v);
    }

    #[test]
    fn chunking_windows_with_overlap() {
        let text = (1..=10)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = chunk_text(&text, 4, 1);
        // step = 3: [1..4], [4..8], [7..10], [10]
        assert_eq!(chunks[0], "1 2 3 4");
        assert_eq!(chunks[1], "4 5 6 7");
        assert!(chunks.last().unwrap().ends_with("10"));
    }

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(chunk_text("a b c", 10, 2), vec!["a b c"]);
        assert!(chunk_text("", 10, 2).is_empty());
    }
}
