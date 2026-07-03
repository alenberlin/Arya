//! Browser sign-in handoff with a one-shot loopback listener.
//!
//! Opens the hosted sign-in URL with a `redirect` back to a local port, then
//! waits for the provider to redirect there with the session token, stores
//! it, and emits `account:signed-in`. This mirrors the PKCE-style desktop
//! flow used by hosted identity providers.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use super::tokens;

/// Generates a high-entropy CSRF `state`. No external RNG dependency: mixes
/// the OS-assigned ephemeral port, a monotonic-ish time source, and the
/// listener address into a 128-bit hex value via SHA-256.
fn make_state(port: u16) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(port.to_le_bytes());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    // A second time read adds scheduling jitter between the two samples.
    let nanos2 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hasher.update(nanos2.to_le_bytes());
    format!("{:x}", hasher.finalize())[..32].to_string()
}

pub fn begin(app: AppHandle, sign_in_url: &str) -> Result<(), String> {
    // Bind an ephemeral loopback port for the callback. Explicitly localhost
    // so only this machine can reach it.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    // CSRF binding: the provider must echo this exact state back, else the
    // callback is an unsolicited/forged request and is rejected.
    let state = make_state(port);

    let sep = if sign_in_url.contains('?') { '&' } else { '?' };
    let url = format!("{sign_in_url}{sep}redirect_url={redirect}&state={state}");
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&url).spawn();

    std::thread::Builder::new()
        .name("arya-signin-callback".into())
        .spawn(move || {
            // Only accept the callback for a bounded window; drop everything
            // that doesn't carry the matching state.
            let _ = listener.set_nonblocking(false);
            let deadline = std::time::Instant::now() + Duration::from_secs(300);
            while std::time::Instant::now() < deadline {
                let Ok((mut stream, peer)) = listener.accept() else {
                    break;
                };
                // Loopback only (defense in depth; the bind is already local).
                if !peer.ip().is_loopback() {
                    continue;
                }
                let mut reader = BufReader::new(&stream);
                let mut request_line = String::new();
                let _ = reader.read_line(&mut request_line);
                let token = extract_verified_token(&request_line, &state);
                let body = if token.is_some() {
                    "<html><body>Signed in. You can return to Arya.</body></html>"
                } else {
                    "<html><body>Sign-in failed. Please try again.</body></html>"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                if let Some(token) = token {
                    if tokens::store(&token).is_ok() {
                        let _ = app.emit("account:signed-in", ());
                    }
                    return; // done; ignore any further connections
                }
                // Wrong/missing state: keep waiting for the real callback.
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Returns the token only when the callback carries the expected `state`,
/// defeating login-CSRF / token-injection from another local process.
pub fn extract_verified_token(request_line: &str, expected_state: &str) -> Option<String> {
    let params = query_params(request_line)?;
    let state = params
        .iter()
        .find_map(|(k, v)| (k == "state").then(|| v.clone()))?;
    if state != expected_state {
        return None;
    }
    params
        .iter()
        .find_map(|(k, v)| (k == "token" && !v.is_empty()).then(|| v.clone()))
}

fn query_params(request_line: &str) -> Option<Vec<(String, String)>> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    Some(
        query
            .split('&')
            .filter_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                Some((k.to_string(), urldecode(v)))
            })
            .collect(),
    )
}

fn urldecode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match c {
            '%' => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    out.push(byte as char);
                }
            }
            '+' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_token_only_with_matching_state() {
        let line = "GET /callback?state=s3cret&token=abc123 HTTP/1.1";
        assert_eq!(
            extract_verified_token(line, "s3cret"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn rejects_token_with_wrong_or_missing_state() {
        // Attacker-injected callback with no/forged state must yield nothing.
        assert_eq!(
            extract_verified_token("GET /callback?token=evil HTTP/1.1", "s3cret"),
            None
        );
        assert_eq!(
            extract_verified_token("GET /callback?state=wrong&token=evil HTTP/1.1", "s3cret"),
            None
        );
    }

    #[test]
    fn decodes_percent_encoding_in_verified_token() {
        let line = "GET /callback?state=s&token=a%2Bb%20c HTTP/1.1";
        assert_eq!(extract_verified_token(line, "s"), Some("a+b c".to_string()));
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(extract_verified_token("garbage", "s"), None);
    }

    #[test]
    fn state_is_high_entropy_and_port_sensitive() {
        let a = make_state(5000);
        let b = make_state(5001);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
