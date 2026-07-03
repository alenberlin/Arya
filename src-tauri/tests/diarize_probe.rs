//! Diagnostic probe for speaker-embedding sanity (ignored; run on demand).
//!
//! Embeds two different slices of the same clean recording (JFK fixture) and
//! prints their cosine similarity. Same speaker + same channel should score
//! high; a low score here means the embedding pipeline itself is wrong.

use arya_lib::recording_diarize::{cosine_similarity, SPEAKER_MODEL};
use arya_lib::speech::wav;

fn bench_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/speech-bench")
}

#[tokio::test]
#[ignore = "diagnostic; requires local models"]
async fn same_speaker_slices_score_high() {
    let dir = bench_dir();
    let fixture = dir.join("jfk.wav");
    assert!(
        fixture.exists(),
        "run the speech bench first to fetch jfk.wav"
    );
    let model_path = arya_lib::speech::models::ensure_model(&SPEAKER_MODEL, &dir)
        .await
        .expect("speaker model");

    let clip = wav::load_16k_mono(&fixture).expect("fixture loads");
    let rate = 16_000usize;
    let a = clip.samples[0..(5 * rate)].to_vec();
    let b = clip.samples[(6 * rate)..(11 * rate).min(clip.samples.len())].to_vec();

    let mut extractor =
        sherpa_rs::speaker_id::EmbeddingExtractor::new(sherpa_rs::speaker_id::ExtractorConfig {
            model: model_path.to_string_lossy().to_string(),
            ..Default::default()
        })
        .expect("extractor");
    let ea = extractor
        .compute_speaker_embedding(a, 16_000)
        .expect("embed a");
    let eb = extractor
        .compute_speaker_embedding(b, 16_000)
        .expect("embed b");
    let sim = cosine_similarity(&ea, &eb);
    println!("--- diarize probe ---");
    println!("embedding dim: {}", ea.len());
    println!("same-speaker clean similarity: {sim:.3}");
    assert!(
        sim > 0.4,
        "same-speaker similarity {sim:.3} is implausibly low; embedding pipeline suspect"
    );
}
