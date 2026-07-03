//! Turn-level speaker diarization, fully on-device.
//!
//! Design: turns are already segmented by energy per source. For each turn
//! long enough to carry voice identity, a speaker embedding (WeSpeaker
//! CAM++, ONNX via sherpa) is computed; turns are clustered by cosine
//! similarity within each source, and clusters are matched against enrolled
//! voice profiles for real names. Unmatched clusters get "Speaker N".
//! The microphone source defaults to the enrolled owner profile or "Me".
//!
//! Sub-turn speaker changes are out of scope for v1 (documented limitation);
//! turn-level labels cover the dominant meeting shape.

use std::sync::{Arc, Mutex, OnceLock};

use sqlx::SqlitePool;

use crate::speech::models::ModelSpec;

/// Process-wide cached speaker-embedding extractor. Loading the 29 MB CAM++
/// ONNX model costs hundreds of ms, so build it once and reuse it across
/// notes and enrollments (mirrors the whisper engine cache). The sherpa
/// extractor is not `Sync`; a `Mutex` serializes the (fast) embed calls.
type SharedExtractor = Arc<Mutex<sherpa_rs::speaker_id::EmbeddingExtractor>>;

fn extractor_cache() -> &'static Mutex<Option<(String, SharedExtractor)>> {
    static CACHE: OnceLock<Mutex<Option<(String, SharedExtractor)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Returns the cached extractor for `model_path`, loading it on first use.
pub fn get_or_load_extractor(model_path: &str) -> Result<SharedExtractor, String> {
    {
        let guard = extractor_cache().lock().expect("extractor cache lock");
        if let Some((path, extractor)) = guard.as_ref() {
            if path == model_path {
                return Ok(Arc::clone(extractor));
            }
        }
    }
    let extractor =
        sherpa_rs::speaker_id::EmbeddingExtractor::new(sherpa_rs::speaker_id::ExtractorConfig {
            model: model_path.to_string(),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
    let shared = Arc::new(Mutex::new(extractor));
    *extractor_cache().lock().expect("extractor cache lock") =
        Some((model_path.to_string(), Arc::clone(&shared)));
    Ok(shared)
}

/// Pinned speaker-embedding model (sha256 computed from the official
/// k2-fsa/sherpa-onnx release artifact).
pub const SPEAKER_MODEL: ModelSpec = ModelSpec {
    id: "wespeaker-en-voxceleb-campp",
    file_name: "wespeaker_en_voxceleb_CAM++.onnx",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_CAM%2B%2B.onnx",
    sha256: "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef",
    approx_bytes: 29_292_684,
};

/// Same-speaker cosine similarity threshold for CAM++ embeddings.
pub const SAME_SPEAKER_THRESHOLD: f32 = 0.55;
/// Minimum turn length that reliably carries voice identity.
pub const MIN_EMBED_MS: u64 = 1_200;

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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

/// Greedy centroid clustering: each embedding joins the best cluster above
/// `threshold` (running-mean centroids) or starts a new one. Returns a
/// cluster label per embedding, in input order.
pub fn cluster_embeddings(embeddings: &[Vec<f32>], threshold: f32) -> Vec<usize> {
    let mut centroids: Vec<(Vec<f32>, usize)> = Vec::new(); // (sum, count)
    let mut labels = Vec::with_capacity(embeddings.len());
    for embedding in embeddings {
        let mut best: Option<(usize, f32)> = None;
        for (index, (sum, count)) in centroids.iter().enumerate() {
            let centroid: Vec<f32> = sum.iter().map(|v| v / *count as f32).collect();
            let similarity = cosine_similarity(embedding, &centroid);
            if similarity >= threshold && best.map(|(_, s)| similarity > s).unwrap_or(true) {
                best = Some((index, similarity));
            }
        }
        match best {
            Some((index, _)) => {
                let (sum, count) = &mut centroids[index];
                for (s, v) in sum.iter_mut().zip(embedding) {
                    *s += v;
                }
                *count += 1;
                labels.push(index);
            }
            None => {
                centroids.push((embedding.clone(), 1));
                labels.push(centroids.len() - 1);
            }
        }
    }
    labels
}

/// An enrolled voice profile.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub embedding: Vec<f32>,
}

/// Names a cluster centroid: the best-matching enrolled profile, or None.
pub fn match_profile<'a>(
    centroid: &[f32],
    profiles: &'a [Profile],
    threshold: f32,
) -> Option<&'a str> {
    let best = profiles
        .iter()
        .map(|p| (p, cosine_similarity(centroid, &p.embedding)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    #[cfg(debug_assertions)]
    if let Some((p, s)) = &best {
        eprintln!("diarize: best profile match {} similarity {s:.3}", p.name);
    }
    best.filter(|(_, s)| *s >= threshold)
        .map(|(p, _)| p.name.as_str())
}

/// Computes centroids per cluster label.
pub fn centroids(embeddings: &[Vec<f32>], labels: &[usize]) -> Vec<Vec<f32>> {
    let cluster_count = labels.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let mut sums: Vec<(Vec<f32>, usize)> = vec![(Vec::new(), 0); cluster_count];
    for (embedding, label) in embeddings.iter().zip(labels) {
        let (sum, count) = &mut sums[*label];
        if sum.is_empty() {
            *sum = vec![0.0; embedding.len()];
        }
        for (s, v) in sum.iter_mut().zip(embedding) {
            *s += v;
        }
        *count += 1;
    }
    sums.into_iter()
        .map(|(sum, count)| {
            if count == 0 {
                sum
            } else {
                sum.into_iter().map(|v| v / count as f32).collect()
            }
        })
        .collect()
}

pub async fn load_profiles(pool: &SqlitePool) -> Result<Vec<Profile>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT name, embedding FROM speaker_profiles ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, blob)| Profile {
            name,
            embedding: blob_to_f32(&blob),
        })
        .collect())
}

pub fn f32_to_blob(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn blob_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(direction: usize, dims: usize) -> Vec<f32> {
        let mut v = vec![0.0; dims];
        v[direction] = 1.0;
        v
    }

    fn noisy(base: &[f32], noise: f32, seed: usize) -> Vec<f32> {
        base.iter()
            .enumerate()
            .map(|(i, v)| v + noise * (((i * 7 + seed * 13) % 10) as f32 / 10.0 - 0.5))
            .collect()
    }

    #[test]
    fn clusters_two_distinct_speakers() {
        let a = unit(0, 8);
        let b = unit(4, 8);
        let embeddings = vec![
            noisy(&a, 0.1, 1),
            noisy(&b, 0.1, 2),
            noisy(&a, 0.1, 3),
            noisy(&b, 0.1, 4),
            noisy(&a, 0.1, 5),
        ];
        let labels = cluster_embeddings(&embeddings, 0.55);
        assert_eq!(labels[0], labels[2]);
        assert_eq!(labels[2], labels[4]);
        assert_eq!(labels[1], labels[3]);
        assert_ne!(labels[0], labels[1]);
    }

    #[test]
    fn single_speaker_is_one_cluster() {
        let a = unit(2, 8);
        let embeddings: Vec<_> = (0..4).map(|i| noisy(&a, 0.05, i)).collect();
        let labels = cluster_embeddings(&embeddings, 0.55);
        assert!(labels.iter().all(|l| *l == 0), "{labels:?}");
    }

    #[test]
    fn profile_matching_picks_best_above_threshold() {
        let profiles = vec![
            Profile {
                name: "Alice".into(),
                embedding: unit(0, 8),
            },
            Profile {
                name: "Bob".into(),
                embedding: unit(4, 8),
            },
        ];
        let near_bob = noisy(&unit(4, 8), 0.1, 9);
        assert_eq!(match_profile(&near_bob, &profiles, 0.55), Some("Bob"));
        let nowhere = unit(7, 8);
        assert_eq!(match_profile(&nowhere, &profiles, 0.55), None);
    }

    #[test]
    fn blob_round_trips() {
        let values = vec![0.5f32, -1.25, 3.0];
        assert_eq!(blob_to_f32(&f32_to_blob(&values)), values);
    }

    #[test]
    fn centroids_average_members() {
        let embeddings = vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![3.0, 0.0]];
        let labels = vec![0, 1, 0];
        let c = centroids(&embeddings, &labels);
        assert_eq!(c[0], vec![2.0, 0.0]);
        assert_eq!(c[1], vec![0.0, 1.0]);
    }
}
