//! Sidecar process management and JSON-RPC transport.
//!
//! One sidecar per write-mode. Sandboxed mode wraps node in a macOS Seatbelt
//! profile that denies all file writes except the agent workspace and temp
//! dirs - a kernel boundary, not a policy suggestion.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use serde_json::{json, Value};

type PendingMap = Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteMode {
    Sandboxed,
    Unrestricted,
}

impl WriteMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            WriteMode::Sandboxed => "sandboxed",
            WriteMode::Unrestricted => "unrestricted",
        }
    }

    pub fn parse(value: &str) -> Self {
        if value == "unrestricted" {
            WriteMode::Unrestricted
        } else {
            WriteMode::Sandboxed
        }
    }
}

/// Seatbelt profile: everything allowed except writes, which are confined to
/// the agent workspace, temp locations, and devices node needs.
pub fn seatbelt_profile(workspace: &Path) -> String {
    format!(
        r#"(version 1)
(allow default)
(deny file-write*)
(allow file-write*
    (subpath "{workspace}")
    (subpath "/private/tmp")
    (subpath "/private/var/folders")
    (subpath "/dev")
    (literal "/dev/null")
    (literal "/dev/urandom"))
"#,
        workspace = workspace.display()
    )
}

pub struct Sidecar {
    child: Child,
    stdin: Mutex<ChildStdin>,
    next_id: AtomicU64,
    pending: PendingMap,
}

impl Sidecar {
    /// Spawns the sidecar. `on_event` receives every `event` notification.
    pub fn spawn(
        script: &Path,
        mode: WriteMode,
        workspace: &Path,
        on_event: impl Fn(Value) + Send + 'static,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(workspace).map_err(|e| e.to_string())?;
        let node = which_node()?;

        let mut command = if mode == WriteMode::Sandboxed && cfg!(target_os = "macos") {
            let mut c = Command::new("/usr/bin/sandbox-exec");
            c.arg("-p").arg(seatbelt_profile(workspace)).arg(&node);
            c
        } else {
            Command::new(&node)
        };
        command
            .arg(script)
            .current_dir(workspace)
            .env("ARYA_MODE", mode.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|e| format!("failed to spawn sidecar: {e}"))?;
        let stdin = child.stdin.take().ok_or("sidecar stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("sidecar stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("sidecar stderr unavailable")?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = Arc::clone(&pending);

        std::thread::Builder::new()
            .name(format!("arya-sidecar-{}-out", mode.as_str()))
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
                        let sender = pending_reader.lock().expect("pending lock").remove(&id);
                        if let Some(sender) = sender {
                            let result = if let Some(error) = message.get("error") {
                                Err(error
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("sidecar error")
                                    .to_string())
                            } else {
                                Ok(message.get("result").cloned().unwrap_or(Value::Null))
                            };
                            let _ = sender.send(result);
                        }
                    } else if message.get("method").and_then(|m| m.as_str()) == Some("event") {
                        if let Some(params) = message.get("params") {
                            on_event(params.clone());
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        std::thread::Builder::new()
            .name(format!("arya-sidecar-{}-err", mode.as_str()))
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    eprintln!("sidecar[{}]: {line}", mode.as_str());
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            next_id: AtomicU64::new(1),
            pending,
        })
    }

    /// Sends a request and waits for its response.
    pub fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().expect("pending lock").insert(id, tx);
        let payload = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        {
            let mut stdin = self.stdin.lock().expect("stdin lock");
            writeln!(stdin, "{payload}").map_err(|e| e.to_string())?;
        }
        rx.recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| "sidecar did not respond".to_string())?
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn shutdown(mut self) {
        let _ = self.request("runtime.shutdown", json!({}));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = self.child.kill();
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Locates a node binary: env override, bundled runtime (M14), or PATH.
fn which_node() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("ARYA_NODE_PATH") {
        return Ok(PathBuf::from(path));
    }
    for candidate in [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
    ] {
        if Path::new(candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }
    which_in_path("node").ok_or_else(|| "node runtime not found".to_string())
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.exists())
    })
}

/// Dev/bundled location of the sidecar script.
pub fn sidecar_script() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("ARYA_SIDECAR_PATH") {
        return Ok(PathBuf::from(path));
    }
    // Dev tree: <repo>/sidecar/dist/index.mjs relative to the crate.
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("sidecar/dist/index.mjs"));
    if let Some(dev) = dev {
        if dev.exists() {
            return Ok(dev);
        }
    }
    Err("sidecar script not found; run `pnpm --filter arya-sidecar build`".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_mode_round_trips() {
        assert_eq!(WriteMode::parse("unrestricted"), WriteMode::Unrestricted);
        assert_eq!(WriteMode::parse("sandboxed"), WriteMode::Sandboxed);
        assert_eq!(WriteMode::parse("junk"), WriteMode::Sandboxed);
    }

    /// The AC test: the Seatbelt jail must block writes outside the
    /// workspace and allow them inside.
    #[test]
    #[cfg(target_os = "macos")]
    fn seatbelt_jail_blocks_outside_writes() {
        let workspace = std::env::temp_dir().join(format!("arya-jail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let profile = seatbelt_profile(&workspace);

        let outside = dirs_home().join(format!("arya-jail-escape-{}.txt", uuid::Uuid::new_v4()));
        let status_outside = Command::new("/usr/bin/sandbox-exec")
            .arg("-p")
            .arg(&profile)
            .arg("/usr/bin/touch")
            .arg(&outside)
            .status()
            .expect("sandbox-exec runs");
        assert!(
            !status_outside.success(),
            "write OUTSIDE the jail must fail"
        );
        assert!(!outside.exists(), "escape file must not exist");

        let inside = workspace.join("allowed.txt");
        let status_inside = Command::new("/usr/bin/sandbox-exec")
            .arg("-p")
            .arg(&profile)
            .arg("/usr/bin/touch")
            .arg(&inside)
            .status()
            .expect("sandbox-exec runs");
        assert!(status_inside.success(), "write INSIDE the jail must work");
        assert!(inside.exists());

        std::fs::remove_dir_all(&workspace).unwrap();
    }

    #[cfg(target_os = "macos")]
    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").expect("HOME set"))
    }
}
