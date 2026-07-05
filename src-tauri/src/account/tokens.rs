//! Session-token storage in the macOS Keychain, with a local-dev fallback.

const SERVICE: &str = "dev.arya.app.account";
const ACCOUNT: &str = "session-token";

/// Whether hosted auth (Clerk) is configured. When not, the app runs in
/// local mode with a built-in token and no sign-in wall.
pub fn hosted_auth_configured() -> bool {
    std::env::var("ARYA_CLERK_SIGN_IN_URL")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// The bearer token the app sends to Arya API. In local mode this is the
/// shared dev token; with hosted auth it's the stored Clerk session token.
pub fn current_token() -> Option<String> {
    if !hosted_auth_configured() {
        let token = std::env::var("ARYA_API_TOKEN").unwrap_or_else(|_| "local-dev-token".into());
        // Never send the shared dev token to a non-loopback API URL — that would
        // expose a guessable bearer off-box. A custom ARYA_API_TOKEN is the
        // operator's explicit choice and is allowed anywhere.
        if token == "local-dev-token" && !api_url_is_loopback() {
            return None;
        }
        return Some(token);
    }
    load_stored()
}

fn api_url_is_loopback() -> bool {
    url_is_loopback(
        &std::env::var("ARYA_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8477".into()),
    )
}

/// True when `url`'s host is genuine loopback. Parsing the host as an IP makes
/// decimal/octal/look-alike spoofs fail closed.
fn url_is_loopback(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = match authority.rfind(':') {
        Some(i) if !authority[i..].contains(']') => &authority[..i],
        _ => authority,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

pub fn store(token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    entry.set_password(token).map_err(|e| e.to_string())
}

pub fn clear() -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn load_stored() -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).ok()?;
    entry.get_password().ok()
}

/// True when the app should present as signed in.
pub fn signed_in() -> bool {
    current_token().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_url_detection() {
        assert!(url_is_loopback("http://127.0.0.1:8477"));
        assert!(url_is_loopback("http://localhost:8477"));
        assert!(url_is_loopback("http://[::1]:8477"));
        assert!(!url_is_loopback("https://api.arya.example.com"));
        assert!(!url_is_loopback("http://2130706433:8477"));
        assert!(!url_is_loopback("http://10.0.0.5:8477"));
    }
}
