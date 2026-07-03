//! Out-of-process system-audio capture (macOS).
//!
//! The signed-at-bundle-time Swift helper (embedded in the binary, written
//! to the app data dir on first use) captures system output via a CoreAudio
//! process tap into `system.partial.wav`, reporting over a status file.
//! Control is via signals: SIGUSR1 pause, SIGUSR2 resume, SIGTERM stop.
//!
//! macOS zeroes the tap stream when the "System Audio Recording" TCC grant
//! is missing, so readiness is probe-driven and the processing layer drops
//! silent system tracks instead of failing the note.

use std::io::{BufRead, Seek};
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "event", rename_all = "lowercase")]
pub enum HelperEvent {
    Ready {
        #[serde(rename = "sampleRate")]
        sample_rate: u32,
        channels: u16,
    },
    Level {
        value: f64,
    },
    Error {
        message: String,
    },
    Stopped,
}

/// Parses one status line; unknown events are ignored (forward compat).
pub fn parse_status_line(line: &str) -> Option<HelperEvent> {
    serde_json::from_str(line.trim()).ok()
}

#[cfg(target_os = "macos")]
const HELPER_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/arya-system-audio-helper"));

/// A running system-audio helper process.
pub struct SystemCapture {
    child: std::process::Child,
    status_path: PathBuf,
    status_offset: u64,
    pub partial_path: PathBuf,
    ready: bool,
}

impl SystemCapture {
    /// Writes the embedded helper to `bin_dir` (if missing) and spawns it,
    /// capturing into `<dir>/system.partial.wav`.
    #[cfg(target_os = "macos")]
    pub fn start(bin_dir: &Path, session_dir: &Path) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;

        std::fs::create_dir_all(bin_dir).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(session_dir).map_err(|e| e.to_string())?;
        let helper = bin_dir.join("arya-system-audio-helper");
        let needs_write = match std::fs::read(&helper) {
            Ok(existing) => existing != HELPER_BYTES,
            Err(_) => true,
        };
        if needs_write {
            std::fs::write(&helper, HELPER_BYTES).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }

        let partial_path = session_dir.join("system.partial.wav");
        let status_path = session_dir.join("system-status.jsonl");
        let child = std::process::Command::new(&helper)
            .arg("--output")
            .arg(&partial_path)
            .arg("--status")
            .arg(&status_path)
            .spawn()
            .map_err(|e| format!("failed to spawn system-audio helper: {e}"))?;
        Ok(Self {
            child,
            status_path,
            status_offset: 0,
            partial_path,
            ready: false,
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn start(_bin_dir: &Path, _session_dir: &Path) -> Result<Self, String> {
        Err("system audio capture is macOS-only".into())
    }

    /// Drains new status events since the last poll.
    pub fn poll_events(&mut self) -> Vec<HelperEvent> {
        let mut events = Vec::new();
        let Ok(file) = std::fs::File::open(&self.status_path) else {
            return events;
        };
        let mut reader = std::io::BufReader::new(file);
        if reader
            .seek(std::io::SeekFrom::Start(self.status_offset))
            .is_err()
        {
            return events;
        }
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            self.status_offset += n as u64;
            if let Some(event) = parse_status_line(&line) {
                if matches!(event, HelperEvent::Ready { .. }) {
                    self.ready = true;
                }
                events.push(event);
            }
            line.clear();
        }
        events
    }

    /// Waits up to `timeout` for the helper to report ready or error.
    pub fn wait_ready(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            for event in self.poll_events() {
                match event {
                    HelperEvent::Ready { .. } => return Ok(()),
                    HelperEvent::Error { message } => return Err(message),
                    _ => {}
                }
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!("helper exited early ({status})"));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err("system-audio helper did not become ready in time".into())
    }

    pub fn pause(&self) {
        self.signal(libc::SIGUSR1);
    }

    pub fn resume(&self) {
        self.signal(libc::SIGUSR2);
    }

    /// Graceful stop: SIGTERM, wait for exit (helper finalizes the WAV),
    /// escalate to SIGKILL + header repair if it hangs. Returns the path to
    /// the finalized (still `.partial.wav`-named) file.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.signal(libc::SIGTERM);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    // Recompute header sizes from file length: a no-op for a
                    // cleanly finalized file, a rescue for a crashed helper.
                    let _ = super::wav_file::repair_header(&self.partial_path);
                    break;
                }
                Ok(None) if std::time::Instant::now() > deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    // Helper never finalized: repair from bytes on disk.
                    super::wav_file::repair_header(&self.partial_path)
                        .map_err(|e| format!("helper hung and repair failed: {e}"))?;
                    break;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(self.partial_path.clone())
    }

    fn signal(&self, sig: i32) {
        unsafe {
            libc::kill(self.child.id() as i32, sig);
        }
    }
}

impl Drop for SystemCapture {
    fn drop(&mut self) {
        // Never leave an orphan capturing audio.
        if matches!(self.child.try_wait(), Ok(None)) {
            self.signal(libc::SIGTERM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_event_kinds() {
        assert_eq!(
            parse_status_line(r#"{"event":"ready","sampleRate":48000,"channels":2}"#),
            Some(HelperEvent::Ready {
                sample_rate: 48000,
                channels: 2
            })
        );
        assert_eq!(
            parse_status_line(r#"{"event":"level","value":0.5}"#),
            Some(HelperEvent::Level { value: 0.5 })
        );
        assert_eq!(
            parse_status_line(r#"{"event":"error","message":"boom"}"#),
            Some(HelperEvent::Error {
                message: "boom".into()
            })
        );
        assert_eq!(
            parse_status_line(r#"{"event":"stopped"}"#),
            Some(HelperEvent::Stopped)
        );
        assert_eq!(parse_status_line("not json"), None);
        // Unknown events tolerated.
        assert_eq!(parse_status_line(r#"{"event":"future-thing"}"#), None);
    }
}
