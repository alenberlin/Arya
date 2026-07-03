//! Browser sign-in handoff with a one-shot loopback listener.
//!
//! Opens the hosted sign-in URL with a `redirect` back to a local port, then
//! waits for the provider to redirect there with the session token, stores
//! it, and emits `account:signed-in`. This mirrors the PKCE-style desktop
//! flow used by hosted identity providers.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use tauri::{AppHandle, Emitter};

use super::tokens;

pub fn begin(app: AppHandle, sign_in_url: &str) -> Result<(), String> {
    // Bind an ephemeral loopback port for the callback.
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let url = if sign_in_url.contains('?') {
        format!("{sign_in_url}&redirect_url={redirect}")
    } else {
        format!("{sign_in_url}?redirect_url={redirect}")
    };
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&url).spawn();

    std::thread::Builder::new()
        .name("arya-signin-callback".into())
        .spawn(move || {
            // Accept one connection, parse the token from the query, respond,
            // store, and notify. Times out with the OS default if abandoned.
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(&stream);
                let mut request_line = String::new();
                let _ = reader.read_line(&mut request_line);
                let token = extract_token(&request_line);
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
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Pulls `token=<...>` from the callback request line.
pub fn extract_token(request_line: &str) -> Option<String> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            let decoded = urldecode(value);
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
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
    fn extracts_token_from_callback_line() {
        let line = "GET /callback?token=abc123&other=x HTTP/1.1";
        assert_eq!(extract_token(line), Some("abc123".to_string()));
    }

    #[test]
    fn decodes_percent_encoding() {
        let line = "GET /callback?token=a%2Bb%20c HTTP/1.1";
        assert_eq!(extract_token(line), Some("a+b c".to_string()));
    }

    #[test]
    fn missing_token_is_none() {
        assert_eq!(extract_token("GET /callback?foo=bar HTTP/1.1"), None);
        assert_eq!(extract_token("garbage"), None);
    }
}
