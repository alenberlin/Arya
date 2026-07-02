//! WAV loading into the canonical inference format (mono 16 kHz f32).
//!
//! M2 scope: reads files already at 16 kHz mono (16-bit PCM or float).
//! Arbitrary-rate capture normalization arrives with the recording pipeline.

use std::path::Path;

use super::{AudioClip, SpeechError};

pub fn load_16k_mono(path: &Path) -> Result<AudioClip, SpeechError> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| SpeechError::InvalidAudio(e.to_string()))?;
    let spec = reader.spec();
    if spec.sample_rate != AudioClip::SAMPLE_RATE {
        return Err(SpeechError::InvalidAudio(format!(
            "expected {} Hz, got {} Hz",
            AudioClip::SAMPLE_RATE,
            spec.sample_rate
        )));
    }
    if spec.channels != 1 {
        return Err(SpeechError::InvalidAudio(format!(
            "expected mono, got {} channels",
            spec.channels
        )));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<_, _>>()
                .map_err(|e| SpeechError::InvalidAudio(e.to_string()))?
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| SpeechError::InvalidAudio(e.to_string()))?,
    };
    Ok(AudioClip { samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, sample_rate: u32, channels: u16, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for s in samples {
            writer.write_sample(*s).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn loads_16k_mono_pcm16() {
        let path = std::env::temp_dir().join(format!("arya-wav-{}.wav", uuid::Uuid::new_v4()));
        write_wav(&path, 16_000, 1, &[0, 16_384, -16_384, 32_767]);
        let clip = load_16k_mono(&path).unwrap();
        assert_eq!(clip.samples.len(), 4);
        assert!((clip.samples[1] - 0.5).abs() < 1e-3);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let path = std::env::temp_dir().join(format!("arya-wav-{}.wav", uuid::Uuid::new_v4()));
        write_wav(&path, 44_100, 1, &[0; 8]);
        let err = load_16k_mono(&path).unwrap_err();
        assert!(err.to_string().contains("44100"));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_stereo() {
        let path = std::env::temp_dir().join(format!("arya-wav-{}.wav", uuid::Uuid::new_v4()));
        write_wav(&path, 16_000, 2, &[0; 8]);
        assert!(load_16k_mono(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
