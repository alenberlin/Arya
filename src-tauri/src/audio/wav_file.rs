//! Crash-safe WAV persistence for recordings.
//!
//! While recording, samples stream to `<name>.partial.wav`. A clean finish
//! finalizes the header and renames to `<name>.wav` (the rename is the
//! durability commit). After a crash the partial file has a stale header but
//! valid PCM bytes; [`repair_header`] patches the RIFF/data sizes from the
//! file length so recovery can proceed. Bytes on disk win over any DB state.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WavFileError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("wav error: {0}")]
    Hound(#[from] hound::Error),
    #[error("not a canonical PCM wav file")]
    NotCanonical,
    #[error("no audio data in file")]
    Empty,
}

/// Canonical 16-bit PCM header size written by hound and the system-audio helper.
pub const HEADER_LEN: u64 = 44;

/// A WAV file being written incrementally (i16 PCM at the capture rate).
pub struct WavSink {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    partial_path: PathBuf,
}

impl WavSink {
    /// Creates `<final_path minus extension>.partial.wav` for streaming.
    pub fn create(
        final_path: &Path,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self, WavFileError> {
        let partial_path = partial_path_for(final_path);
        if let Some(parent) = partial_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(&partial_path, spec)?;
        Ok(Self {
            writer,
            partial_path,
        })
    }

    pub fn write_f32(&mut self, samples: &[f32]) -> Result<(), WavFileError> {
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            self.writer.write_sample(v)?;
        }
        Ok(())
    }

    /// Flushes sample data so a crash right now still leaves bytes on disk.
    pub fn flush(&mut self) -> Result<(), WavFileError> {
        self.writer.flush()?;
        Ok(())
    }

    /// Finalizes the header and renames partial -> final.
    pub fn finalize(self, final_path: &Path) -> Result<(), WavFileError> {
        self.writer.finalize()?;
        std::fs::rename(&self.partial_path, final_path)?;
        Ok(())
    }

    pub fn partial_path(&self) -> &Path {
        &self.partial_path
    }
}

pub fn partial_path_for(final_path: &Path) -> PathBuf {
    let stem = final_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    final_path.with_file_name(format!("{stem}.partial.wav"))
}

/// Rewrites the RIFF and data chunk sizes of a canonical PCM wav from the
/// actual file length. Returns the repaired data length in bytes.
pub fn repair_header(path: &Path) -> Result<u64, WavFileError> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let len = file.metadata()?.len();
    if len < HEADER_LEN {
        return Err(WavFileError::NotCanonical);
    }
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"RIFF" {
        return Err(WavFileError::NotCanonical);
    }
    file.seek(SeekFrom::Start(8))?;
    let mut wave = [0u8; 8];
    file.read_exact(&mut wave)?;
    if &wave[0..4] != b"WAVE" || &wave[4..8] != b"fmt " {
        return Err(WavFileError::NotCanonical);
    }
    file.seek(SeekFrom::Start(36))?;
    let mut data_tag = [0u8; 4];
    file.read_exact(&mut data_tag)?;
    if &data_tag != b"data" {
        return Err(WavFileError::NotCanonical);
    }
    let data_len = len - HEADER_LEN;
    if data_len == 0 {
        return Err(WavFileError::Empty);
    }
    // Canonical WAV sizes are u32; refuse absurd lengths rather than wrapping.
    if len - 8 > u32::MAX as u64 {
        return Err(WavFileError::NotCanonical);
    }
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&((len - 8) as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&(data_len as u32).to_le_bytes())?;
    file.flush()?;
    Ok(data_len)
}

/// Loads any PCM wav (any rate, any channel count) and converts to the
/// canonical mono 16 kHz clip.
pub fn load_normalized(path: &Path) -> Result<crate::speech::AudioClip, WavFileError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if !matches!(spec.bits_per_sample, 8 | 16 | 24 | 32) {
        return Err(WavFileError::NotCanonical);
    }
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|v| v as f32 / max)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
    };
    if raw.is_empty() {
        return Err(WavFileError::Empty);
    }
    let mono = super::resample::downmix_interleaved(&raw, spec.channels);
    let samples = super::resample::resample_to_16k(&mono, spec.sample_rate)
        .map_err(|e| WavFileError::Hound(hound::Error::IoError(std::io::Error::other(e))))?;
    Ok(crate::speech::AudioClip { samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_wav(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("arya-wavfile-{}-{name}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn sink_finalize_renames_to_final() {
        let final_path = temp_wav("a.wav");
        let mut sink = WavSink::create(&final_path, 48_000, 1).unwrap();
        sink.write_f32(&vec![0.25; 4800]).unwrap();
        let partial = sink.partial_path().to_path_buf();
        assert!(partial.exists());
        sink.finalize(&final_path).unwrap();
        assert!(final_path.exists());
        assert!(!partial.exists());
        let clip = load_normalized(&final_path).unwrap();
        assert!((clip.samples.len() as i64 - 1600).abs() < 50);
        std::fs::remove_file(&final_path).unwrap();
    }

    #[test]
    fn repair_header_fixes_unfinalized_partial() {
        let final_path = temp_wav("b.wav");
        let mut sink = WavSink::create(&final_path, 16_000, 1).unwrap();
        sink.write_f32(&vec![0.5; 16_000]).unwrap();
        sink.flush().unwrap();
        let partial = sink.partial_path().to_path_buf();
        // Simulate a crash: drop the sink without finalize; hound's Drop
        // finalizes, so instead corrupt the header the way a kill -9 leaves
        // it (sizes zeroed) after copying the bytes out.
        let mut bytes = std::fs::read(&partial).unwrap();
        bytes[4..8].copy_from_slice(&[0, 0, 0, 0]);
        bytes[40..44].copy_from_slice(&[0, 0, 0, 0]);
        let crashed = temp_wav("crashed.wav");
        std::fs::write(&crashed, &bytes).unwrap();

        // Unreadable before repair (hound trusts the zero-length header).
        let before = hound::WavReader::open(&crashed)
            .map(|r| r.len())
            .unwrap_or(0);
        assert_eq!(before, 0);

        let data_len = repair_header(&crashed).unwrap();
        assert_eq!(data_len, 32_000); // 16k samples * 2 bytes
        let clip = load_normalized(&crashed).unwrap();
        assert_eq!(clip.samples.len(), 16_000);

        drop(sink);
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_file(&crashed);
        let _ = std::fs::remove_file(&final_path);
    }

    #[test]
    fn repair_rejects_non_wav() {
        let path = temp_wav("junk.bin");
        std::fs::write(&path, vec![0u8; 100]).unwrap();
        assert!(repair_header(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
