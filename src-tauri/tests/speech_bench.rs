//! M2 acceptance benchmark: on-device transcription accuracy and speed.
//!
//! Ignored by default because it downloads a model (~148 MB) on first run.
//! Run with:
//!   cargo test --release --test speech_bench -- --ignored --nocapture
//!
//! Acceptance criteria (PLAN.md M2):
//!   - reference WAV transcribes within the accuracy baseline (WER <= 0.15)
//!   - real-time factor < 0.5 on Apple Silicon

use std::path::PathBuf;
use std::time::Instant;

use arya_lib::speech::{
    models, wav, wer::word_error_rate, whisper::WhisperEngine, SpeechEngine, TranscribeOptions,
};

const JFK_URL: &str = "https://github.com/ggml-org/whisper.cpp/raw/master/samples/jfk.wav";
const JFK_SHA256: &str = "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e";
const JFK_REFERENCE: &str = "And so my fellow Americans ask not what your country can do for you \
                             ask what you can do for your country.";

fn bench_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/speech-bench")
}

#[tokio::test]
#[ignore = "downloads a speech model; run explicitly for the M2 gate"]
async fn transcribes_reference_within_accuracy_and_speed_budget() {
    let dir = bench_dir();
    let model_id = std::env::var("ARYA_SPEECH_MODEL").unwrap_or_else(|_| "whisper-base.en".into());
    let spec = models::find(&model_id).expect("model id in catalog");

    let model_path = models::ensure_model(spec, &dir)
        .await
        .expect("model download");
    let fixture = dir.join("jfk.wav");
    if !fixture.exists() {
        models::download_verified(JFK_URL, &fixture, JFK_SHA256, "jfk.wav")
            .await
            .expect("fixture download");
    }

    let clip = wav::load_16k_mono(&fixture).expect("fixture loads");
    let audio_secs = clip.duration_secs();
    assert!(audio_secs > 5.0, "fixture should be several seconds long");

    let load_start = Instant::now();
    let engine = WhisperEngine::load(&model_path).expect("engine loads");
    let load_secs = load_start.elapsed().as_secs_f64();

    let options = TranscribeOptions {
        language: Some("en".into()),
    };
    let infer_start = Instant::now();
    let transcript = engine.transcribe(&clip, &options).expect("transcribes");
    let infer_secs = infer_start.elapsed().as_secs_f64();

    // The engine reuses one decode state across calls (whisper.cpp clears its
    // result buffers each `full`). A second transcription of the same clip must
    // reset cleanly and reproduce the first result — greedy decoding is
    // deterministic — proving reuse is correct, not just non-crashing.
    let repeat = engine
        .transcribe(&clip, &options)
        .expect("second transcribe on the reused state");
    assert_eq!(
        repeat.text, transcript.text,
        "reused decode state must reproduce identical output across calls"
    );

    let rtf = infer_secs / audio_secs;
    let wer = word_error_rate(JFK_REFERENCE, &transcript.text);

    println!("--- M2 speech benchmark ---");
    println!("model:        {}", spec.id);
    println!("model load:   {load_secs:.2}s");
    println!("audio:        {audio_secs:.1}s");
    println!("inference:    {infer_secs:.2}s");
    println!("RTF:          {rtf:.3}");
    println!("WER:          {wer:.3}");
    println!("text:         {}", transcript.text);
    println!(
        "segments:     {} ({}ms..{}ms)",
        transcript.segments.len(),
        transcript.segments.first().map(|s| s.start_ms).unwrap_or(0),
        transcript.segments.last().map(|s| s.end_ms).unwrap_or(0),
    );

    assert!(
        wer <= 0.15,
        "WER {wer:.3} exceeds the 0.15 accuracy baseline; text: {}",
        transcript.text
    );
    assert!(rtf < 0.5, "RTF {rtf:.3} exceeds the 0.5 budget");
    assert!(
        !transcript.segments.is_empty(),
        "segments must be populated"
    );
}
