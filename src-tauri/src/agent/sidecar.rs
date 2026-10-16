//! Sidecar process management and JSON-RPC transport.
//!
//! One sidecar per write-mode. Sandboxed mode wraps node in a macOS Seatbelt
//! profile that denies all file writes except the agent workspace and temp
//! dirs - a kernel boundary, not a policy suggestion.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use serde_json::{json, Value};
use tracing::info;

type PendingMap = Arc<Mutex<HashMap<u64, mpsc::Sender<crate::error::AryaResult<Value>>>>>;

/// The agent always runs sandboxed. A single-variant enum keeps the session
/// `mode` column and the shell↔sidecar protocol stable while making it
/// impossible to spawn an unsandboxed sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteMode {
    Sandboxed,
}

impl WriteMode {
    pub fn as_str(&self) -> &'static str {
        "sandboxed"
    }

    /// Legacy sessions may carry `unrestricted`; they degrade to sandboxed
    /// (strictly safer) rather than failing.
    pub fn parse(_value: &str) -> Self {
        WriteMode::Sandboxed
    }
}

/// Seatbelt profile: everything allowed except writes, which are confined to
/// the agent workspace and the few device nodes node needs. We deliberately
/// do NOT grant the shared temp roots (`/tmp`, `/private/var/folders`): those
/// are world/other-app writable and would let a "sandboxed" agent drop files
/// another process consumes. node's scratch is redirected into the workspace
/// via TMPDIR (see spawn), so it needs nothing outside.
///
/// Threat model: this is a WRITE jail plus an inbound-network deny, and by
/// design NOT a kernel network-outbound/process-exec jail. `(allow default)`
/// still permits outbound network, process-exec, and reads, because the sidecar
/// needs network to reach the cloud providers/Ollama and exec to run its
/// (approved) `run_command` tool. Outbound network and exec are governed at the
/// tool layer by human-in-the-loop approval instead — the agent has no ungated
/// network/exec tool: `read_file`/`list_dir` are confined to the workspace
/// (out-of-workspace reads require approval), `run_command` is approved per
/// program with no shell, and MCP tools are approved per tool. The only ungated
/// network use is the model's own inference (the user's chosen cloud provider,
/// or local Ollama on loopback), which cannot be per-call-approved without
/// prompting on every token. Decision (owner, recorded): keep the tool-layer
/// approval + write jail as the boundary; do NOT add a kernel network/exec deny,
/// since no shipped feature requires ungated outbound calls.
pub fn seatbelt_profile(workspace: &Path) -> String {
    // The kernel matches subpaths against the REAL path, so canonicalize
    // (resolves symlinks like /var -> /private/var on macOS); otherwise a
    // symlinked workspace ancestor makes the allow rule silently miss.
    let canonical = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    // Escape for the TinyScheme string literal so a `"` or `\` in the path
    // cannot terminate or corrupt the `(subpath "…")` rule.
    let workspace = escape_sandbox_literal(&canonical.to_string_lossy());
    format!(
        r#"(version 1)
(allow default)
(deny file-write*)
;; The sidecar is a stdio client; it never accepts connections, so listening is
;; denied outright. (Outbound is still allowed: it must reach Ollama and the
;; user's cloud providers, and the tool layer + approvals gate the rest.)
(deny network-inbound)
(allow file-write*
    (subpath "{workspace}")
    (literal "/dev/null")
    (literal "/dev/urandom")
    (literal "/dev/dtracehelper")
    (literal "/dev/tty"))
"#,
        workspace = workspace
    )
}

/// Escapes a string for embedding in a Seatbelt (TinyScheme) string literal.
/// Backslash is escaped first so the escape we add for `"` isn't re-escaped.
fn escape_sandbox_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub struct Sidecar {
    // Behind a Mutex so liveness checks and kills work through a shared Arc.
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending: PendingMap,
    /// Set when a request times out; `is_alive` treats a hung sidecar as dead
    /// so the next `get_or_spawn` respawns it instead of failing forever.
    hung: AtomicBool,
}

impl Sidecar {
    /// Spawns the sidecar. `on_event` receives every `event` notification;
    /// `on_context` answers reverse `context.search` requests (query, limit).
    pub fn spawn(
        script: &Path,
        mode: WriteMode,
        workspace: &Path,
        on_event: impl Fn(Value) + Send + 'static,
        on_context: impl Fn(String, i64) -> crate::error::AryaResult<Value> + Send + 'static,
    ) -> crate::error::AryaResult<Self> {
        std::fs::create_dir_all(workspace).map_err(crate::error::AryaError::from_display)?;
        let node = which_node()?;

        // Redirect node's scratch into the jailed workspace so it never needs
        // write access to the shared temp roots.
        let tmp_dir = workspace.join(".tmp");
        std::fs::create_dir_all(&tmp_dir).map_err(crate::error::AryaError::from_display)?;

        let mut command = if cfg!(target_os = "macos") {
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
            .env("TMPDIR", &tmp_dir)
            .env("TMP", &tmp_dir)
            .env("TEMP", &tmp_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Local-only: no proxy. The agent reaches cloud providers directly with
        // the user's own keys. With none set, it offers only local Ollama models.
        // The env-removes keep a stray proxy variable from a parent environment
        // out of the sidecar.
        command.env_remove("ARYA_API_URL");
        command.env_remove("ARYA_API_TOKEN");
        for (provider, var) in [
            (crate::keys::Provider::Anthropic, "ANTHROPIC_API_KEY"),
            (crate::keys::Provider::OpenAi, "OPENAI_API_KEY"),
        ] {
            match crate::keys::get(provider) {
                Some(key) => command.env(var, key),
                None => command.env_remove(var),
            };
        }

        // Put the sidecar in its own process group so Drop can kill the whole
        // tree (the sidecar spawns MCP child servers) instead of orphaning them.
        #[cfg(unix)]
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let mut child = command
            .spawn()
            .map_err(|e| format!("failed to spawn sidecar: {e}"))?;
        let stdin = Arc::new(Mutex::new(
            child.stdin.take().ok_or("sidecar stdin unavailable")?,
        ));
        let stdout = child.stdout.take().ok_or("sidecar stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("sidecar stderr unavailable")?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = Arc::clone(&pending);
        let stdin_reader = Arc::clone(&stdin);

        std::thread::Builder::new()
            .name(format!("arya-sidecar-{}-out", mode.as_str()))
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let method = message.get("method").and_then(|m| m.as_str());
                    if method == Some("context.search") {
                        // Reverse RPC: answer with a rev-id response.
                        let id = message.get("id").cloned().unwrap_or(Value::Null);
                        let query = message
                            .get("params")
                            .and_then(|p| p.get("query"))
                            .and_then(|q| q.as_str())
                            .unwrap_or("")
                            .to_string();
                        let limit = message
                            .get("params")
                            .and_then(|p| p.get("limit"))
                            .and_then(|l| l.as_i64())
                            .unwrap_or(6);
                        let response = match on_context(query, limit) {
                            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                            Err(message) => json!({
                                "jsonrpc": "2.0", "id": id,
                                "error": { "code": -32001, "message": message }
                            }),
                        };
                        if let Ok(mut stdin) = stdin_reader.lock() {
                            let _ = writeln!(stdin, "{response}");
                        }
                    } else if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
                        let sender = pending_reader.lock().expect("pending lock").remove(&id);
                        if let Some(sender) = sender {
                            let result = if let Some(error) = message.get("error") {
                                Err(crate::error::AryaError::message(
                                    error
                                        .get("message")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("sidecar error"),
                                ))
                            } else {
                                Ok(message.get("result").cloned().unwrap_or(Value::Null))
                            };
                            let _ = sender.send(result);
                        }
                    } else if method == Some("event") {
                        if let Some(params) = message.get("params") {
                            on_event(params.clone());
                        }
                    }
                }
            })
            .map_err(crate::error::AryaError::from_display)?;

        std::thread::Builder::new()
            .name(format!("arya-sidecar-{}-err", mode.as_str()))
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    info!(mode = %mode.as_str(), "{}", line);
                }
            })
            .map_err(crate::error::AryaError::from_display)?;

        Ok(Self {
            child: Mutex::new(child),
            stdin,
            next_id: AtomicU64::new(1),
            pending,
            hung: AtomicBool::new(false),
        })
    }

    /// Sends a request and waits for its response.
    pub fn request(&self, method: &str, params: Value) -> crate::error::AryaResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().expect("pending lock").insert(id, tx);
        let payload = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let write_result = {
            let mut stdin = self.stdin.lock().expect("stdin lock");
            writeln!(stdin, "{payload}")
        };
        if let Err(e) = write_result {
            self.pending.lock().expect("pending lock").remove(&id);
            return Err(crate::error::AryaError::from_display(e));
        }
        // The reader only removes an entry when a matching response arrives, so
        // a write failure or timeout must clean up its own pending entry — else
        // the map (and its mpsc sender) leaks one slot per failed request.
        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(result) => {
                self.hung.store(false, Ordering::SeqCst);
                result
            }
            Err(_) => {
                self.pending.lock().expect("pending lock").remove(&id);
                // A request that never got a response means the sidecar is
                // wedged; mark it hung so the next get_or_spawn respawns it.
                self.hung.store(true, Ordering::SeqCst);
                Err(crate::error::AryaError::message("sidecar did not respond"))
            }
        }
    }

    /// Liveness via `&self` so it works through a shared `Arc`.
    pub fn is_alive(&self) -> bool {
        !self.hung.load(Ordering::SeqCst)
            && matches!(self.child.lock().expect("child lock").try_wait(), Ok(None))
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            #[cfg(unix)]
            {
                // Kill the whole process group (negative pid) so MCP servers the
                // sidecar spawned don't outlive it as orphaned children. Fall
                // back to killing just the child if setpgid never took effect.
                let pid = child.id() as i32;
                if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
                    let _ = child.kill();
                }
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            // Reap the direct child so it doesn't linger as a zombie.
            let _ = child.wait();
        }
    }
}

/// Locates a node binary: env override, bundled runtime (M14), or PATH.
fn which_node() -> crate::error::AryaResult<PathBuf> {
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
    which_in_path("node").ok_or_else(|| crate::error::AryaError::message("node runtime not found"))
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.exists())
    })
}

/// Dev/bundled location of the sidecar script.
pub fn sidecar_script() -> crate::error::AryaResult<PathBuf> {
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
    Err(crate::error::AryaError::message(
        "sidecar script not found; run `pnpm --filter arya-sidecar build`",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_unrestricted_mode_degrades_to_sandboxed() {
        assert_eq!(WriteMode::parse("unrestricted"), WriteMode::Sandboxed);
        assert_eq!(WriteMode::parse("sandboxed"), WriteMode::Sandboxed);
        assert_eq!(WriteMode::parse("junk"), WriteMode::Sandboxed);
        assert_eq!(WriteMode::Sandboxed.as_str(), "sandboxed");
    }

    #[test]
    fn escapes_quotes_and_backslashes_in_sandbox_literal() {
        assert_eq!(escape_sandbox_literal(r#"a"b"#), r#"a\"b"#);
        assert_eq!(escape_sandbox_literal(r"a\b"), r"a\\b");
        // Backslash escaped before the quote — the added `\` is not re-escaped.
        assert_eq!(escape_sandbox_literal(r#"\""#), r#"\\\""#);
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

        // The shared temp roots must NOT be writable (regression guard for
        // the sandbox-escape finding): /tmp is world-shared.
        let tmp_escape =
            std::path::PathBuf::from(format!("/tmp/arya-jail-tmp-{}.txt", uuid::Uuid::new_v4()));
        let status_tmp = Command::new("/usr/bin/sandbox-exec")
            .arg("-p")
            .arg(&profile)
            .arg("/usr/bin/touch")
            .arg(&tmp_escape)
            .status()
            .expect("sandbox-exec runs");
        assert!(!status_tmp.success(), "write to /tmp must be denied");
        assert!(!tmp_escape.exists(), "no file may appear in /tmp");

        std::fs::remove_dir_all(&workspace).unwrap();
    }

    #[cfg(target_os = "macos")]
    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").expect("HOME set"))
    }
}
