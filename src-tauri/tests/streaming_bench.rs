//! Streaming ASR acceptance: the sherpa-onnx online transducer transcribes a
//! fixture WAV within an accuracy baseline, exercising both the chunk-feed path
//! and the cumulative-snapshot path the live dictation pipeline actually uses.
//!
//! Ignored by default because it downloads a streaming model (~73 MB) on first
//! run. Run with:
//!   cargo test --release --test streaming_bench -- --ignored --nocapture
//!
//! This verifies the ASR core (the FFI binding + decoding + reset). Wiring into
//! the live capture path and the pill is verified on-device with a microphone.

use std::path::{Path, PathBuf};
use std::time::Instant;

use arya_lib::speech::streaming::{SherpaStreamingEngine, StreamingSpeechEngine};
use arya_lib::speech::wer::word_error_rate;
use arya_lib::speech::{models, wav, AudioClip};

const REPO: &str =
    "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26/resolve/main";
const ENCODER: &str = "encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
const DECODER: &str = "decoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
const JOINER: &str = "joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
const TOKENS: &str = "tokens.txt";

const JFK_URL: &str = "https://github.com/ggml-org/whisper.cpp/raw/master/samples/jfk.wav";
const JFK_SHA256: &str = "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e";
const JFK_REFERENCE: &str = "And so my fellow Americans ask not what your country can do for you \
                             ask what you can do for your country.";
const BASELINE_WER: f64 = 0.25;

fn bench_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/streaming-bench")
}

/// Fetch a model artifact into `dir` (checksum unpinned — dev-only bench).
async fn fetch(dir: &Path, file: &str) -> PathBuf {
    let target = dir.join(file);
    if !target.exists() {
        let url = format!("{REPO}/{file}");
        models::download_verified(&url, &target, "", file)
            .await
            .expect("model artifact download");
    }
    target
}

/// Download the model + fixture, load the engine, and return both.
async fn prepared_engine() -> (SherpaStreamingEngine, AudioClip) {
    let dir = bench_dir();
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let encoder = fetch(&dir, ENCODER).await;
    let decoder = fetch(&dir, DECODER).await;
    let joiner = fetch(&dir, JOINER).await;
    let tokens = fetch(&dir, TOKENS).await;

    let fixture = dir.join("jfk.wav");
    if !fixture.exists() {
        models::download_verified(JFK_URL, &fixture, JFK_SHA256, "jfk.wav")
            .await
            .expect("fixture download");
    }

    let clip = wav::load_16k_mono(&fixture).expect("fixture loads");
    assert!(
        clip.duration_secs() > 5.0,
        "fixture should be several seconds"
    );
    let engine = SherpaStreamingEngine::load(&encoder, &decoder, &joiner, &tokens)
        .expect("streaming engine loads");
    (engine, clip)
}

#[tokio::test]
#[ignore = "downloads a streaming model; run explicitly for the streaming gate"]
async fn streams_reference_within_accuracy_budget() {
    let (engine, clip) = prepared_engine().await;
    let audio_secs = clip.duration_secs();

    // Feed audio in 100 ms chunks, as the live capture path would, and confirm a
    // partial appears before we finalize.
    let chunk = 1_600usize; // 100 ms @ 16 kHz
    let start = Instant::now();
    let mut saw_partial = false;
    for c in clip.samples.chunks(chunk) {
        engine.accept(c);
        if !saw_partial && !engine.partial().trim().is_empty() {
            saw_partial = true;
        }
    }
    let text = engine.finalize();
    let infer_secs = start.elapsed().as_secs_f64();

    let wer = word_error_rate(JFK_REFERENCE, &text);
    println!("--- streaming benchmark (chunk feed) ---");
    println!("audio:     {audio_secs:.1}s");
    println!("inference: {infer_secs:.2}s");
    println!("RTF:       {:.3}", infer_secs / audio_secs);
    println!("WER:       {wer:.3}");
    println!("text:      {text}");

    assert!(saw_partial, "a live partial should appear before finalize");
    assert!(!text.trim().is_empty(), "streaming produced no text");
    assert!(
        wer <= BASELINE_WER,
        "WER {wer:.3} exceeds baseline; text: {text}"
    );
}

#[tokio::test]
#[ignore = "downloads a streaming model; run explicitly for the streaming gate"]
async fn cumulative_feed_matches_the_live_finish_path() {
    let (engine, clip) = prepared_engine().await;

    // Mirror `run_streaming_pipeline`: reset, feed growing snapshots (as the live
    // ticker does with `feed_up_to`), feed the final tail, then finalize.
    engine.reset();
    let step = 16_000usize; // 1 s snapshots
    let mut end = step.min(clip.samples.len());
    while end < clip.samples.len() {
        engine.feed_up_to(&clip.samples[..end]);
        end = (end + step).min(clip.samples.len());
    }
    engine.feed_up_to(&clip.samples); // final tail (idempotent)
    let text = engine.finalize();

    let wer = word_error_rate(JFK_REFERENCE, &text);
    println!("--- streaming benchmark (cumulative feed) ---");
    println!("WER:  {wer:.3}");
    println!("text: {text}");
    assert!(!text.trim().is_empty(), "cumulative feed produced no text");
    assert!(
        wer <= BASELINE_WER,
        "WER {wer:.3} exceeds baseline; text: {text}"
    );

    // reset() must clear state so the next utterance is independent — proving a
    // second dictation on the reused cached engine transcribes cleanly.
    engine.reset();
    engine.feed_up_to(&clip.samples);
    let second = engine.finalize();
    let second_wer = word_error_rate(JFK_REFERENCE, &second);
    assert!(
        second_wer <= BASELINE_WER,
        "second utterance after reset should be clean (WER {second_wer:.3}): {second}"
    );
}
