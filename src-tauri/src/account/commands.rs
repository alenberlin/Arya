//! Account commands: sign-in/out, snapshot fetch, upgrade/top-up handoff.

use tauri::State;

use super::tokens;
use super::{AccountSnapshot, SignInState};

fn api_base() -> String {
    std::env::var("ARYA_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8477".into())
}

#[derive(Default)]
pub struct AccountState {
    client: reqwest::Client,
}

#[tauri::command]
pub fn account_signin_state() -> SignInState {
    SignInState {
        signed_in: tokens::signed_in(),
        hosted_auth: tokens::hosted_auth_configured(),
    }
}

/// Opens the hosted sign-in page (Clerk). The loopback callback that stores
/// the returned token is handled by a one-shot listener started here.
/// In local mode there is nothing to do; the app is already "signed in".
#[tauri::command]
pub fn account_begin_signin(app: tauri::AppHandle) -> Result<(), String> {
    if !tokens::hosted_auth_configured() {
        return Ok(());
    }
    let url = std::env::var("ARYA_CLERK_SIGN_IN_URL").map_err(|e| e.to_string())?;
    super::signin_flow::begin(app, &url)
}

/// Dev/local: store a token directly (used by the loopback callback and by
/// tests). No-op-safe.
#[tauri::command]
pub fn account_set_token(token: String) -> Result<(), String> {
    tokens::store(&token)
}

#[tauri::command]
pub fn account_sign_out() -> Result<(), String> {
    tokens::clear()
}

#[tauri::command]
pub async fn account_snapshot(state: State<'_, AccountState>) -> Result<AccountSnapshot, String> {
    let token = tokens::current_token().ok_or("not signed in")?;
    let response = state
        .client
        .get(format!("{}/v1/account", api_base()))
        .header("authorization", format!("Bearer {token}"))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("session expired; sign in again".into());
    }
    let envelope: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    if envelope.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return Err(envelope
            .pointer("/errors/0/message")
            .and_then(|m| m.as_str())
            .unwrap_or("account fetch failed")
            .to_string());
    }
    serde_json::from_value(envelope["data"].clone()).map_err(|e| e.to_string())
}

/// Opens the billing/upgrade page in the browser (Stripe checkout / portal).
/// The URL comes from config; in local mode there is no portal, so this is a
/// no-op with a clear signal.
#[tauri::command]
pub fn account_open_billing(target: String) -> Result<bool, String> {
    let base = std::env::var("ARYA_BILLING_URL").ok();
    match base {
        Some(base) if !base.is_empty() => {
            let url = format!("{base}?intent={target}");
            open_url(&url);
            Ok(true)
        }
        _ => Ok(false), // local mode: no hosted billing
    }
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(target_os = "macos"))]
    let _ = url;
}
