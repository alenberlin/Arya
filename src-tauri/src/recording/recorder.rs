//! Disk-first recording worker.
//!
//! A dedicated thread owns the cpal stream and the WAV sink. Samples flow
//! from the audio callback over a channel and hit disk continuously, so a
//! crash at any moment loses at most the unflushed tail.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::wav_file::WavSink;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecorderState {
    Idle,
    Recording,
    Paused,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderStatus {
    pub state: RecorderState,
    pub elapsed_ms: u64,
    pub level: f32,
    pub session_id: Option<String>,
    pub note_id: Option<String>,
}

pub struct StartSpec {
    pub session_id: String,
    pub note_id: String,
    pub final_path: PathBuf,
    pub device: Option<String>,
}

enum Command {
    Start(StartSpec, mpsc::Sender<Result<(u32, u16), String>>),
    Pause(mpsc::Sender<Result<(), String>>),
    Resume(mpsc::Sender<Result<(), String>>),
    /// Finalize and rename; replies with the final path.
    Finish(mpsc::Sender<Result<PathBuf, String>>),
    Status(mpsc::Sender<RecorderStatus>),
    /// Drains the rolling live-preview buffer: (interleaved samples, rate, channels).
    TakePreview(mpsc::Sender<Option<(Vec<f32>, u32, u16)>>),
}

#[derive(Clone)]
pub struct Recorder {
    sender: mpsc::Sender<Command>,
    elapsed_ms: Arc<AtomicU64>,
}

struct Active {
    spec: StartSpec,
    sink: WavSink,
    stream: Option<cpal::Stream>,
    sample_rx: mpsc::Receiver<Vec<f32>>,
    sample_tx: mpsc::Sender<Vec<f32>>,
    sample_rate: u32,
    channels: u16,
    written_frames: u64,
    paused: bool,
    /// Rolling buffer of recent raw samples for the ephemeral live preview.
    preview: Vec<f32>,
}

impl Recorder {
    pub fn spawn() -> Self {
        let (sender, receiver) = mpsc::channel::<Command>();
        let level_milli = Arc::new(AtomicU32::new(0));
        let elapsed_ms = Arc::new(AtomicU64::new(0));
        let level_out = Arc::clone(&level_milli);
        let elapsed_out = Arc::clone(&elapsed_ms);
        std::thread::Builder::new()
            .name("arya-recorder".into())
            .spawn(move || run_loop(receiver, level_out, elapsed_out))
            .expect("spawn recorder thread");
        Self { sender, elapsed_ms }
    }

    pub fn start(&self, spec: StartSpec) -> Result<(u32, u16), String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Start(spec, tx))
            .map_err(|_| "recorder gone".to_string())?;
        rx.recv().map_err(|_| "recorder gone".to_string())?
    }

    pub fn pause(&self) -> Result<(), String> {
        self.round_trip(Command::Pause)
    }

    pub fn resume(&self) -> Result<(), String> {
        self.round_trip(Command::Resume)
    }

    pub fn finish(&self) -> Result<PathBuf, String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Command::Finish(tx))
            .map_err(|_| "recorder gone".to_string())?;
        rx.recv().map_err(|_| "recorder gone".to_string())?
    }

    pub fn status(&self) -> RecorderStatus {
        let (tx, rx) = mpsc::channel();
        if self.sender.send(Command::Status(tx)).is_err() {
            return RecorderStatus {
                state: RecorderState::Idle,
                elapsed_ms: 0,
                level: 0.0,
                session_id: None,
                note_id: None,
            };
        }
        rx.recv().unwrap_or(RecorderStatus {
            state: RecorderState::Idle,
            elapsed_ms: 0,
            level: 0.0,
            session_id: None,
            note_id: None,
        })
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms.load(Ordering::Relaxed)
    }

    /// Takes whatever live-preview audio accumulated since the last call.
    pub fn take_preview(&self) -> Option<(Vec<f32>, u32, u16)> {
        let (tx, rx) = mpsc::channel();
        self.sender.send(Command::TakePreview(tx)).ok()?;
        rx.recv().ok().flatten()
    }

    fn round_trip(
        &self,
        make: impl FnOnce(mpsc::Sender<Result<(), String>>) -> Command,
    ) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(make(tx))
            .map_err(|_| "recorder gone".to_string())?;
        rx.recv().map_err(|_| "recorder gone".to_string())?
    }
}

fn run_loop(
    receiver: mpsc::Receiver<Command>,
    level_out: Arc<AtomicU32>,
    elapsed_out: Arc<AtomicU64>,
) {
    let mut active: Option<Active> = None;
    loop {
        // Drain pending samples to disk, then service one command (or tick).
        if let Some(state) = active.as_mut() {
            drain_samples(state, &level_out, &elapsed_out);
        }
        match receiver.recv_timeout(std::time::Duration::from_millis(40)) {
            Ok(Command::Start(spec, reply)) => {
                if active.is_some() {
                    let _ = reply.send(Err("already recording".into()));
                    continue;
                }
                match begin(spec) {
                    Ok(state) => {
                        let _ = reply.send(Ok((state.sample_rate, state.channels)));
                        active = Some(state);
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Ok(Command::Pause(reply)) => {
                let result = match active.as_mut() {
                    Some(state) if !state.paused => {
                        state.stream.take();
                        state.paused = true;
                        Ok(())
                    }
                    Some(_) => Err("already paused".into()),
                    None => Err("not recording".into()),
                };
                let _ = reply.send(result);
            }
            Ok(Command::Resume(reply)) => {
                let result = match active.as_mut() {
                    Some(state) if state.paused => {
                        build_stream(state.spec.device.as_deref(), state.sample_tx.clone()).map(
                            |(stream, _, _)| {
                                state.stream = Some(stream);
                                state.paused = false;
                            },
                        )
                    }
                    Some(_) => Err("not paused".into()),
                    None => Err("not recording".into()),
                };
                let _ = reply.send(result);
            }
            Ok(Command::Finish(reply)) => match active.take() {
                Some(mut state) => {
                    state.stream.take();
                    drain_samples(&mut state, &level_out, &elapsed_out);
                    let final_path = state.spec.final_path.clone();
                    let result = state
                        .sink
                        .finalize(&final_path)
                        .map(|()| final_path)
                        .map_err(|e| e.to_string());
                    level_out.store(0, Ordering::Relaxed);
                    let _ = reply.send(result);
                }
                None => {
                    let _ = reply.send(Err("not recording".into()));
                }
            },
            Ok(Command::Status(reply)) => {
                let status = match &active {
                    Some(state) => RecorderStatus {
                        state: if state.paused {
                            RecorderState::Paused
                        } else {
                            RecorderState::Recording
                        },
                        elapsed_ms: elapsed_out.load(Ordering::Relaxed),
                        level: level_out.load(Ordering::Relaxed) as f32 / 1000.0,
                        session_id: Some(state.spec.session_id.clone()),
                        note_id: Some(state.spec.note_id.clone()),
                    },
                    None => RecorderStatus {
                        state: RecorderState::Idle,
                        elapsed_ms: 0,
                        level: 0.0,
                        session_id: None,
                        note_id: None,
                    },
                };
                let _ = reply.send(status);
            }
            Ok(Command::TakePreview(reply)) => {
                let payload = active.as_mut().map(|state| {
                    (
                        std::mem::take(&mut state.preview),
                        state.sample_rate,
                        state.channels,
                    )
                });
                let _ = reply.send(payload);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn begin(spec: StartSpec) -> Result<Active, String> {
    let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>();
    let (stream, sample_rate, channels) = build_stream(spec.device.as_deref(), sample_tx.clone())?;
    let sink =
        WavSink::create(&spec.final_path, sample_rate, channels).map_err(|e| e.to_string())?;
    Ok(Active {
        spec,
        sink,
        stream: Some(stream),
        sample_rx,
        sample_tx,
        sample_rate,
        channels,
        written_frames: 0,
        paused: false,
        preview: Vec::new(),
    })
}

fn build_stream(
    device_name: Option<&str>,
    sample_tx: mpsc::Sender<Vec<f32>>,
) -> Result<(cpal::Stream, u32, u16), String> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .input_devices()
            .map_err(|e| e.to_string())?
            .find(|d| {
                d.description()
                    .map(|desc| desc.name() == name)
                    .unwrap_or(false)
            })
            .ok_or_else(|| "input device not found".to_string())?,
        None => host
            .default_input_device()
            .ok_or_else(|| "no input device".to_string())?,
    };
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let sample_rate = config.sample_rate();
    let channels = config.channels();
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let err_fn = |e| eprintln!("recording stream error: {e}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                stream_config,
                move |data: &[f32], _| {
                    let _ = sample_tx.send(data.to_vec());
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                stream_config,
                move |data: &[i16], _| {
                    let floats: Vec<f32> =
                        data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                    let _ = sample_tx.send(floats);
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        other => return Err(format!("unsupported sample format {other:?}")),
    };
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, sample_rate, channels))
}

fn drain_samples(state: &mut Active, level_out: &AtomicU32, elapsed_out: &AtomicU64) {
    let mut latest_level = None;
    while let Ok(chunk) = state.sample_rx.try_recv() {
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
        latest_level = Some(rms);
        if state.sink.write_f32(&chunk).is_err() {
            eprintln!("recording: failed to write samples");
        }
        state.preview.extend_from_slice(&chunk);
        let cap = (state.sample_rate as usize) * (state.channels as usize) * 30;
        if state.preview.len() > cap {
            let excess = state.preview.len() - cap;
            state.preview.drain(..excess);
        }
        state.written_frames += chunk.len() as u64 / state.channels as u64;
    }
    let _ = state.sink.flush();
    if let Some(rms) = latest_level {
        level_out.store((rms.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }
    elapsed_out.store(
        state.written_frames * 1000 / state.sample_rate as u64,
        Ordering::Relaxed,
    );
}
