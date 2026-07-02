//! Owns cpal capture on a dedicated thread.
//!
//! cpal streams are not Send on macOS, so a worker thread owns the
//! `CaptureHandle` and the rest of the app talks to it over channels.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::audio::{start_capture, CaptureError};
use crate::speech::AudioClip;

enum Command {
    Start {
        device: Option<String>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Stop {
        reply: mpsc::Sender<Result<AudioClip, String>>,
    },
    Abort,
}

/// Cloneable handle to the capture worker thread.
#[derive(Clone)]
pub struct CaptureWorker {
    sender: mpsc::Sender<Command>,
    /// Shared live level (0..1) for HUD meters.
    level: Arc<Mutex<f32>>,
}

impl CaptureWorker {
    pub fn spawn() -> Self {
        let (sender, receiver) = mpsc::channel::<Command>();
        let level = Arc::new(Mutex::new(0.0f32));
        let level_out = Arc::clone(&level);
        std::thread::Builder::new()
            .name("arya-capture".into())
            .spawn(move || {
                let mut active: Option<crate::audio::CaptureHandle> = None;
                loop {
                    // Poll with a timeout so the live level updates while
                    // recording even without commands arriving.
                    match receiver.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(Command::Start { device, reply }) => {
                            if active.is_some() {
                                let _ = reply.send(Err("already recording".into()));
                                continue;
                            }
                            match start_capture(device.as_deref()) {
                                Ok(handle) => {
                                    active = Some(handle);
                                    let _ = reply.send(Ok(()));
                                }
                                Err(e) => {
                                    let _ = reply.send(Err(e.to_string()));
                                }
                            }
                        }
                        Ok(Command::Stop { reply }) => match active.take() {
                            Some(handle) => {
                                let _ = reply.send(handle.stop().map_err(|e| e.to_string()));
                            }
                            None => {
                                let _ = reply.send(Err("not recording".into()));
                            }
                        },
                        Ok(Command::Abort) => {
                            active = None;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    if let Some(handle) = &active {
                        *level_out.lock().expect("level lock") = handle.level();
                    } else {
                        *level_out.lock().expect("level lock") = 0.0;
                    }
                }
            })
            .expect("spawn capture worker");
        Self { sender, level }
    }

    pub fn start(&self, device: Option<String>) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        self.sender
            .send(Command::Start { device, reply })
            .map_err(|_| "capture worker gone".to_string())?;
        rx.recv().map_err(|_| "capture worker gone".to_string())?
    }

    pub fn stop(&self) -> Result<AudioClip, String> {
        let (reply, rx) = mpsc::channel();
        self.sender
            .send(Command::Stop { reply })
            .map_err(|_| "capture worker gone".to_string())?;
        rx.recv().map_err(|_| "capture worker gone".to_string())?
    }

    pub fn abort(&self) {
        let _ = self.sender.send(Command::Abort);
    }

    pub fn level(&self) -> f32 {
        *self.level.lock().expect("level lock")
    }
}

impl std::fmt::Debug for CaptureWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureWorker").finish_non_exhaustive()
    }
}

/// Convenience conversion so callers can surface capture failures uniformly.
impl From<CaptureError> for String {
    fn from(e: CaptureError) -> Self {
        e.to_string()
    }
}
