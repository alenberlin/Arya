//! Microphone capture into the canonical inference format (mono 16 kHz f32).
//!
//! cpal drives the device at its native rate/channel count; we downmix and
//! resample when the stream stops. Live RMS levels are published while
//! recording so HUDs can render meters.

pub mod resample;
pub mod system_capture;
pub mod turns;
pub mod wav_file;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::speech::AudioClip;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no input device available")]
    NoDevice,
    #[error("input device error: {0}")]
    Device(String),
    #[error("unsupported sample format: {0}")]
    UnsupportedFormat(String),
    #[error("resample failed: {0}")]
    Resample(String),
}

/// A running microphone capture. Dropping it stops the stream.
///
/// The realtime callback only fills preallocated sample blocks and enqueues
/// them onto a bounded channel, and the handle assembles them into `buffer` on
/// demand. A long `snapshot()` clone can never block the audio thread.
pub struct CaptureHandle {
    stream: Option<cpal::Stream>,
    rx: Receiver<Vec<f32>>,
    free_tx: SyncSender<Vec<f32>>,
    buffer: Mutex<Vec<f32>>,
    level_milli: Arc<AtomicU32>,
    sample_rate: u32,
    channels: u16,
}

// cpal::Stream is !Send on macOS (CoreAudio); the handle lives inside a
// dedicated thread-bound holder in the dictation service, never crossing
// threads. See DictationService.
impl CaptureHandle {
    /// Current input level in [0, 1], rolling RMS over recent callbacks.
    pub fn level(&self) -> f32 {
        // Keep the bounded realtime queue moving even when no partial
        // transcript is being requested; otherwise small CoreAudio callbacks
        // can fill the pool and drop captured speech before stop().
        self.drain();
        self.level_milli.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// Moves any queued callback blocks into `buffer`, capping total growth.
    /// Only touches the channel and the handle-owned buffer — never a lock the
    /// realtime callback holds — so it can't stall the audio thread.
    fn drain(&self) {
        let mut guard = self.buffer.lock().expect("capture buffer lock");
        while let Ok(chunk) = self.rx.try_recv() {
            guard.extend_from_slice(&chunk);
            recycle_sample_block(&self.free_tx, chunk);
        }
        if guard.len() > MAX_CAPTURE_SAMPLES {
            let excess = guard.len() - MAX_CAPTURE_SAMPLES;
            guard.drain(0..excess);
        }
    }

    /// Stops the stream and returns everything captured, normalized to
    /// mono 16 kHz.
    pub fn stop(mut self) -> Result<AudioClip, CaptureError> {
        // Drop the stream first so the callback stops enqueuing, then drain the
        // final blocks before taking the buffer.
        self.stream.take();
        self.drain();
        let raw = std::mem::take(&mut *self.buffer.lock().expect("capture buffer lock"));
        let mono = resample::downmix_interleaved(&raw, self.channels);
        let samples = resample::resample_to_16k(&mono, self.sample_rate)
            .map_err(|e| CaptureError::Resample(e.to_string()))?;
        Ok(AudioClip { samples })
    }

    /// A copy of everything captured so far, normalized to mono 16 kHz,
    /// without stopping the stream — for live/partial transcription.
    pub fn snapshot(&self) -> Result<AudioClip, CaptureError> {
        self.drain();
        let raw = self.buffer.lock().expect("capture buffer lock").clone();
        let mono = resample::downmix_interleaved(&raw, self.channels);
        let samples = resample::resample_to_16k(&mono, self.sample_rate)
            .map_err(|e| CaptureError::Resample(e.to_string()))?;
        Ok(AudioClip { samples })
    }

    /// Seconds of audio captured so far (approximate, pre-resample).
    pub fn elapsed_secs(&self) -> f64 {
        self.drain();
        let frames =
            self.buffer.lock().expect("capture buffer lock").len() as f64 / self.channels as f64;
        frames / self.sample_rate as f64
    }
}

/// Starts capturing from `device_name` (or the default input device).
pub fn start_capture(device_name: Option<&str>) -> Result<CaptureHandle, CaptureError> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .input_devices()
            .map_err(|e| CaptureError::Device(e.to_string()))?
            .find(|d| {
                d.description()
                    .map(|desc| desc.name() == name)
                    .unwrap_or(false)
            })
            .ok_or(CaptureError::NoDevice)?,
        None => host.default_input_device().ok_or(CaptureError::NoDevice)?,
    };
    let config = device
        .default_input_config()
        .map_err(|e| CaptureError::Device(e.to_string()))?;
    let sample_rate = config.sample_rate();
    let channels = config.channels();
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    // The callback uses a bounded preallocated block pool; the handle
    // assembles those blocks into `buffer` on demand. Pre-size the long-lived
    // capture buffer for ~10s so the drain path rarely reallocates.
    let buffer = Mutex::new(Vec::<f32>::with_capacity(
        sample_rate as usize * channels as usize * 10,
    ));
    let level_milli = Arc::new(AtomicU32::new(0));

    let (stream, rx, free_tx) = match sample_format {
        cpal::SampleFormat::F32 => {
            let (callback, rx, free_tx) =
                sample_block_channel(sample_rate, channels, Arc::clone(&level_milli));
            let stream = device
                .build_input_stream(
                    stream_config,
                    move |data: &[f32], _| {
                        callback.send_f32(data);
                    },
                    |e| eprintln!("capture stream error: {e}"),
                    None,
                )
                .map_err(|e| CaptureError::Device(e.to_string()))?;
            (stream, rx, free_tx)
        }
        cpal::SampleFormat::I16 => {
            let (callback, rx, free_tx) =
                sample_block_channel(sample_rate, channels, Arc::clone(&level_milli));
            let stream = device
                .build_input_stream(
                    stream_config,
                    move |data: &[i16], _| {
                        callback.send_i16(data);
                    },
                    |e| eprintln!("capture stream error: {e}"),
                    None,
                )
                .map_err(|e| CaptureError::Device(e.to_string()))?;
            (stream, rx, free_tx)
        }
        other => return Err(CaptureError::UnsupportedFormat(format!("{other:?}"))),
    };
    stream
        .play()
        .map_err(|e| CaptureError::Device(e.to_string()))?;

    Ok(CaptureHandle {
        stream: Some(stream),
        rx,
        free_tx,
        buffer,
        level_milli,
        sample_rate,
        channels,
    })
}

/// Upper bound on buffered raw samples (~5 min at 48 kHz stereo). An abnormally
/// long capture drops its oldest audio rather than growing without limit.
const MAX_CAPTURE_SAMPLES: usize = 48_000 * 2 * 300;

const CALLBACK_BLOCK_POOL_LEN: usize = 64;

pub(crate) struct CallbackSampleSender {
    ready_tx: SyncSender<Vec<f32>>,
    free_rx: Receiver<Vec<f32>>,
    free_tx: SyncSender<Vec<f32>>,
    level_milli: Arc<AtomicU32>,
}

pub(crate) fn sample_block_channel(
    sample_rate: u32,
    channels: u16,
    level_milli: Arc<AtomicU32>,
) -> (
    CallbackSampleSender,
    Receiver<Vec<f32>>,
    SyncSender<Vec<f32>>,
) {
    let block_capacity = (sample_rate as usize * channels as usize).max(4096);
    sample_block_channel_with_capacity(block_capacity, CALLBACK_BLOCK_POOL_LEN, level_milli)
}

fn sample_block_channel_with_capacity(
    block_capacity: usize,
    block_count: usize,
    level_milli: Arc<AtomicU32>,
) -> (
    CallbackSampleSender,
    Receiver<Vec<f32>>,
    SyncSender<Vec<f32>>,
) {
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(block_count);
    let (free_tx, free_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(block_count);
    for _ in 0..block_count {
        let _ = free_tx.try_send(Vec::with_capacity(block_capacity));
    }
    (
        CallbackSampleSender {
            ready_tx,
            free_rx,
            free_tx: free_tx.clone(),
            level_milli,
        },
        ready_rx,
        free_tx,
    )
}

impl CallbackSampleSender {
    pub(crate) fn send_f32(&self, data: &[f32]) {
        self.store_level(sample_rms(data));
        self.with_block(data.len(), |block| block.extend_from_slice(data));
    }

    pub(crate) fn send_i16(&self, data: &[i16]) {
        self.store_level(sample_rms_i16(data));
        self.with_block(data.len(), |block| {
            block.extend(data.iter().map(|s| *s as f32 / 32768.0));
        });
    }

    fn with_block(&self, len: usize, fill: impl FnOnce(&mut Vec<f32>)) {
        let Ok(mut block) = self.free_rx.try_recv() else {
            return;
        };
        if block.capacity() < len {
            let _ = self.free_tx.try_send(block);
            return;
        }
        block.clear();
        fill(&mut block);
        match self.ready_tx.try_send(block) {
            Ok(()) => {}
            Err(TrySendError::Full(block)) | Err(TrySendError::Disconnected(block)) => {
                let _ = self.free_tx.try_send(block);
            }
        }
    }

    fn store_level(&self, rms: f32) {
        self.level_milli
            .store((rms.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }
}

pub(crate) fn recycle_sample_block(free_tx: &SyncSender<Vec<f32>>, mut block: Vec<f32>) {
    block.clear();
    let _ = free_tx.try_send(block);
}

/// RMS of a sample block — the meter math, shared so the capture paths can't
/// diverge.
pub fn sample_rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

fn sample_rms_i16(samples: &[i16]) -> f32 {
    (samples
        .iter()
        .map(|s| {
            let f = *s as f32 / 32768.0;
            f * f
        })
        .sum::<f32>()
        / samples.len().max(1) as f32)
        .sqrt()
}

/// Names of available input devices, default first.
pub fn input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|desc| desc.name().to_string());
    let mut names: Vec<String> = host
        .input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| d.description().ok())
                .map(|desc| desc.name().to_string())
                .collect()
        })
        .unwrap_or_default();
    if let Some(default) = default_name {
        names.retain(|n| *n != default);
        names.insert(0, default);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_sample_queue_recycles_blocks() {
        let level = Arc::new(AtomicU32::new(0));
        let (sender, rx, free_tx) = sample_block_channel_with_capacity(8, 1, Arc::clone(&level));

        sender.send_f32(&[0.25, -0.25]);
        let chunk = rx.try_recv().unwrap();
        assert_eq!(chunk, vec![0.25, -0.25]);
        assert!(level.load(Ordering::Relaxed) > 0);

        recycle_sample_block(&free_tx, chunk);
        sender.send_i16(&[16_384, -16_384]);
        let chunk = rx.try_recv().unwrap();
        assert_eq!(chunk, vec![0.5, -0.5]);
    }

    #[test]
    fn oversized_callback_block_is_dropped_and_pool_survives() {
        let level = Arc::new(AtomicU32::new(0));
        let (sender, rx, _free_tx) = sample_block_channel_with_capacity(2, 1, level);

        sender.send_f32(&[1.0, 1.0, 1.0]);
        assert!(rx.try_recv().is_err());

        sender.send_f32(&[0.5]);
        assert_eq!(rx.try_recv().unwrap(), vec![0.5]);
    }

    #[test]
    fn level_drains_blocks_so_pool_can_be_reused() {
        let level = Arc::new(AtomicU32::new(0));
        let (sender, rx, free_tx) = sample_block_channel_with_capacity(8, 1, Arc::clone(&level));
        let handle = CaptureHandle {
            stream: None,
            rx,
            free_tx,
            buffer: Mutex::new(Vec::new()),
            level_milli: level,
            sample_rate: AudioClip::SAMPLE_RATE,
            channels: 1,
        };

        sender.send_f32(&[0.25]);
        assert!(handle.level() > 0.0);
        sender.send_f32(&[0.5]);
        assert!(handle.level() > 0.0);

        let samples = handle.buffer.lock().expect("capture buffer lock").clone();
        assert_eq!(samples, vec![0.25, 0.5]);
    }
}
