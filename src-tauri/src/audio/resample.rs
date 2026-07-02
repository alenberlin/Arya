//! Downmix and resample helpers (pure, unit-tested).

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType};

use crate::speech::AudioClip;

/// Averages interleaved channels into mono. A mono input is returned as-is.
pub fn downmix_interleaved(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Resamples mono audio from `input_rate` to 16 kHz with a windowed-sinc
/// filter (anti-aliased). Returns the input untouched when already 16 kHz.
pub fn resample_to_16k(mono: &[f32], input_rate: u32) -> Result<Vec<f32>, rubato::ResampleError> {
    if input_rate == AudioClip::SAMPLE_RATE {
        return Ok(mono.to_vec());
    }
    if mono.is_empty() {
        return Ok(Vec::new());
    }
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: rubato::WindowFunction::BlackmanHarris2,
    };
    let chunk = 1024usize;
    let ratio = AudioClip::SAMPLE_RATE as f64 / input_rate as f64;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 1.1, params, chunk, 1)
        .expect("valid resampler construction");

    let delay = resampler.output_delay();
    let expected_len = (mono.len() as f64 * ratio).round() as usize;

    let mut out = Vec::with_capacity(expected_len + chunk);
    let mut position = 0usize;
    while position + chunk <= mono.len() {
        let frames = resampler.process(&[&mono[position..position + chunk]], None)?;
        out.extend_from_slice(&frames[0]);
        position += chunk;
    }
    let remainder = &mono[position..];
    if !remainder.is_empty() {
        let frames = resampler.process_partial(Some(&[remainder]), None)?;
        out.extend_from_slice(&frames[0]);
    }
    // Flush the internal delay line, then drop the leading delay and pad
    // artifacts so output aligns with the input signal exactly.
    while out.len() < delay + expected_len {
        let frames = resampler.process_partial::<&[f32]>(None, None)?;
        if frames[0].is_empty() {
            break;
        }
        out.extend_from_slice(&frames[0]);
    }
    Ok(out.into_iter().skip(delay).take(expected_len).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn downmix_averages_stereo_frames() {
        let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(downmix_interleaved(&stereo, 2), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn downmix_passes_mono_through() {
        let mono = [0.1, 0.2, 0.3];
        assert_eq!(downmix_interleaved(&mono, 1), mono.to_vec());
    }

    #[test]
    fn resample_48k_to_16k_preserves_duration_and_pitch() {
        let input = sine(48_000, 440.0, 1.0);
        let out = resample_to_16k(&input, 48_000).unwrap();
        // Duration within 2%.
        let expected = 16_000.0;
        assert!(
            (out.len() as f64 - expected).abs() / expected < 0.02,
            "got {} samples",
            out.len()
        );
        // A 440 Hz tone keeps ~440 positive-going zero crossings per second.
        let crossings = zero_crossings(&out);
        assert!(
            (crossings as f64 - 440.0).abs() < 15.0,
            "got {crossings} crossings"
        );
    }

    #[test]
    fn resample_16k_is_identity() {
        let input = sine(16_000, 200.0, 0.25);
        let out = resample_to_16k(&input, 16_000).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn resample_empty_is_empty() {
        assert!(resample_to_16k(&[], 48_000).unwrap().is_empty());
    }
}
