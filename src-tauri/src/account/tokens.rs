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
        return Some(std::env::var("ARYA_API_TOKEN").unwrap_or_else(|_| "local-dev-token".into()));
    }
    load_stored()
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
