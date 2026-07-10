//! Small vector-math helpers shared by the on-device diarizer (speaker
//! embeddings) and the RAG index (text embeddings): one source of truth so the
//! cosine metric and the f32<->blob encoding can't drift apart between them.

/// Cosine similarity of two equal-length vectors; 0.0 for empty, mismatched, or
/// zero-norm inputs.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Little-endian f32 blob encoding (for storing embeddings in SQLite).
pub fn f32_to_blob(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Inverse of [`f32_to_blob`].
pub fn blob_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_handles_degenerate_inputs_and_identity() {
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn blob_round_trips() {
        let v = vec![0.0_f32, 1.5, -2.25, 3.125];
        assert_eq!(blob_to_f32(&f32_to_blob(&v)), v);
    }
}
