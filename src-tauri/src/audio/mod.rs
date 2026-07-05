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
pub struct CaptureHandle {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
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
        self.level_milli.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// Stops the stream and returns everything captured, normalized to
    /// mono 16 kHz.
    pub fn stop(mut self) -> Result<AudioClip, CaptureError> {
        // Explicitly drop the stream before draining so the callback can't
        // race the take.
        self.stream.take();
        let raw = {
            let mut guard = self.buffer.lock().expect("capture buffer lock");
            std::mem::take(&mut *guard)
        };
        let mono = resample::downmix_interleaved(&raw, self.channels);
        let samples = resample::resample_to_16k(&mono, self.sample_rate)
            .map_err(|e| CaptureError::Resample(e.to_string()))?;
        Ok(AudioClip { samples })
    }

    /// A copy of everything captured so far, normalized to mono 16 kHz,
    /// without stopping the stream — for live/partial transcription.
    pub fn snapshot(&self) -> Result<AudioClip, CaptureError> {
        let raw = {
            let guard = self.buffer.lock().expect("capture buffer lock");
            guard.clone()
        };
        let mono = resample::downmix_interleaved(&raw, self.channels);
        let samples = resample::resample_to_16k(&mono, self.sample_rate)
            .map_err(|e| CaptureError::Resample(e.to_string()))?;
        Ok(AudioClip { samples })
    }

    /// Seconds of audio captured so far (approximate, pre-resample).
    pub fn elapsed_secs(&self) -> f64 {
        let frames = {
            let guard = self.buffer.lock().expect("capture buffer lock");
            guard.len() as f64 / self.channels as f64
        };
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

    // Pre-allocate ~10s so the realtime callback rarely reallocates while
    // extending. (A lock-free SPSC ring in the callback is the fuller fix, but
    // it needs on-device microphone verification; this bounds the growth safely
    // in the meantime.)
    let buffer = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
        sample_rate as usize * channels as usize * 10,
    )));
    let level_milli = Arc::new(AtomicU32::new(0));
    let cb_buffer = Arc::clone(&buffer);
    let cb_level = Arc::clone(&level_milli);

    let err_fn = |e| eprintln!("capture stream error: {e}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                stream_config,
                move |data: &[f32], _| {
                    push_samples(&cb_buffer, &cb_level, data);
                },
                err_fn,
                None,
            )
            .map_err(|e| CaptureError::Device(e.to_string()))?,
        cpal::SampleFormat::I16 => {
            let cb_buffer = Arc::clone(&buffer);
            let cb_level = Arc::clone(&level_milli);
            device
                .build_input_stream(
                    stream_config,
                    move |data: &[i16], _| {
                        // Divide by 32768 so i16::MIN maps to exactly -1.0
                        // (dividing by i16::MAX would overshoot to -1.00003).
                        let floats: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                        push_samples(&cb_buffer, &cb_level, &floats);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| CaptureError::Device(e.to_string()))?
        }
        other => return Err(CaptureError::UnsupportedFormat(format!("{other:?}"))),
    };
    stream
        .play()
        .map_err(|e| CaptureError::Device(e.to_string()))?;

    Ok(CaptureHandle {
        stream: Some(stream),
        buffer,
        level_milli,
        sample_rate,
        channels,
    })
}

/// Upper bound on buffered raw samples (~5 min at 48 kHz stereo). An abnormally
/// long capture drops its oldest audio rather than growing without limit.
const MAX_CAPTURE_SAMPLES: usize = 48_000 * 2 * 300;

fn push_samples(buffer: &Arc<Mutex<Vec<f32>>>, level: &Arc<AtomicU32>, data: &[f32]) {
    let mut guard = buffer.lock().expect("capture buffer lock");
    guard.extend_from_slice(data);
    if guard.len() > MAX_CAPTURE_SAMPLES {
        let excess = guard.len() - MAX_CAPTURE_SAMPLES;
        guard.drain(0..excess);
    }
    let rms = (data.iter().map(|s| s * s).sum::<f32>() / data.len().max(1) as f32).sqrt();
    level.store((rms.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
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
