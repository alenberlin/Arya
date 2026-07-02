//! Energy-based turn detection over mono 16 kHz audio.
//!
//! No diarization: a turn is a contiguous span of vocal energy on one
//! source. RMS is computed over 30 ms windows; the activity threshold is a
//! noise-floor estimate (20th percentile of window energy) times a
//! multiplier, with hysteresis to avoid flapping.

use crate::speech::AudioClip;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnSpan {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TurnConfig {
    pub window_ms: u64,
    /// Threshold = max(noise_floor * multiplier, absolute_floor).
    pub noise_multiplier: f32,
    pub absolute_floor: f32,
    /// Silence run that ends a turn.
    pub end_silence_ms: u64,
    /// Discard turns shorter than this.
    pub min_turn_ms: u64,
    /// Merge adjacent turns separated by less than this.
    pub merge_gap_ms: u64,
}

impl Default for TurnConfig {
    fn default() -> Self {
        Self {
            window_ms: 30,
            noise_multiplier: 2.5,
            absolute_floor: 0.008,
            end_silence_ms: 900,
            min_turn_ms: 400,
            merge_gap_ms: 600,
        }
    }
}

pub fn detect_turns(clip: &AudioClip, config: &TurnConfig) -> Vec<TurnSpan> {
    let window = (AudioClip::SAMPLE_RATE as u64 * config.window_ms / 1000) as usize;
    if window == 0 || clip.samples.len() < window {
        return Vec::new();
    }
    let energies: Vec<f32> = clip
        .samples
        .chunks(window)
        .map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt())
        .collect();

    let mut sorted = energies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = sorted[sorted.len() / 5]; // 20th percentile
    let threshold = (noise_floor * config.noise_multiplier).max(config.absolute_floor);

    let end_silence_windows = (config.end_silence_ms / config.window_ms).max(1) as usize;
    let mut raw: Vec<TurnSpan> = Vec::new();
    let mut active_start: Option<usize> = None;
    let mut silence_run = 0usize;

    for (i, energy) in energies.iter().enumerate() {
        if *energy >= threshold {
            if active_start.is_none() {
                active_start = Some(i);
            }
            silence_run = 0;
        } else if let Some(start) = active_start {
            silence_run += 1;
            if silence_run >= end_silence_windows {
                let end = i + 1 - silence_run;
                raw.push(span_from_windows(start, end, config.window_ms));
                active_start = None;
                silence_run = 0;
            }
        }
    }
    if let Some(start) = active_start {
        let end = energies.len() - silence_run;
        raw.push(span_from_windows(start, end, config.window_ms));
    }

    // Merge close turns, then drop the too-short ones.
    let mut merged: Vec<TurnSpan> = Vec::new();
    for turn in raw {
        match merged.last_mut() {
            Some(last) if turn.start_ms.saturating_sub(last.end_ms) < config.merge_gap_ms => {
                last.end_ms = turn.end_ms;
            }
            _ => merged.push(turn),
        }
    }
    merged.retain(|t| t.end_ms - t.start_ms >= config.min_turn_ms);
    merged
}

fn span_from_windows(start_window: usize, end_window: usize, window_ms: u64) -> TurnSpan {
    TurnSpan {
        start_ms: start_window as u64 * window_ms,
        end_ms: end_window as u64 * window_ms,
    }
}

/// Extracts the samples for one turn, with a little padding on both sides so
/// ASR sees the word boundaries.
pub fn slice_turn(clip: &AudioClip, turn: &TurnSpan, pad_ms: u64) -> AudioClip {
    let rate = AudioClip::SAMPLE_RATE as u64;
    let start = (turn.start_ms.saturating_sub(pad_ms) * rate / 1000) as usize;
    let end = (((turn.end_ms + pad_ms) * rate / 1000) as usize).min(clip.samples.len());
    AudioClip {
        samples: clip.samples[start.min(end)..end].to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a clip alternating speech-like noise bursts and silence.
    fn synthetic(pattern: &[(bool, u64)]) -> AudioClip {
        let rate = AudioClip::SAMPLE_RATE as u64;
        let mut samples = Vec::new();
        let mut phase = 0f32;
        for (active, ms) in pattern {
            let n = (rate * ms / 1000) as usize;
            for i in 0..n {
                if *active {
                    // Loud-ish modulated tone approximates voiced energy.
                    phase += 0.11;
                    samples.push(0.35 * phase.sin() * (0.7 + 0.3 * ((i / 160) % 2) as f32));
                } else {
                    // Low noise floor.
                    samples.push(0.0015 * ((i * 7919) % 17) as f32 / 17.0);
                }
            }
        }
        AudioClip { samples }
    }

    #[test]
    fn detects_two_separated_turns() {
        let clip = synthetic(&[
            (false, 1000),
            (true, 1500),
            (false, 2000),
            (true, 1200),
            (false, 800),
        ]);
        let turns = detect_turns(&clip, &TurnConfig::default());
        assert_eq!(turns.len(), 2, "turns: {turns:?}");
        assert!(turns[0].start_ms >= 900 && turns[0].start_ms <= 1100);
        assert!(turns[1].start_ms >= 4300 && turns[1].start_ms <= 4700);
    }

    #[test]
    fn merges_gaps_below_threshold() {
        let clip = synthetic(&[(true, 800), (false, 300), (true, 800), (false, 1000)]);
        let turns = detect_turns(&clip, &TurnConfig::default());
        assert_eq!(turns.len(), 1, "turns: {turns:?}");
        assert!(turns[0].end_ms >= 1800);
    }

    #[test]
    fn drops_blips_shorter_than_min_turn() {
        let clip = synthetic(&[(false, 1500), (true, 120), (false, 2000)]);
        let turns = detect_turns(&clip, &TurnConfig::default());
        assert!(turns.is_empty(), "turns: {turns:?}");
    }

    #[test]
    fn silence_only_has_no_turns() {
        let clip = synthetic(&[(false, 3000)]);
        assert!(detect_turns(&clip, &TurnConfig::default()).is_empty());
    }

    #[test]
    fn slice_turn_pads_and_clamps() {
        let clip = synthetic(&[(true, 1000)]);
        let turn = TurnSpan {
            start_ms: 0,
            end_ms: 1000,
        };
        let sliced = slice_turn(&clip, &turn, 200);
        assert_eq!(sliced.samples.len(), clip.samples.len());
    }
}
