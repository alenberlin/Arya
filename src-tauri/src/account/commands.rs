//! Account commands: sign-in/out, credit snapshot fetch.

use tauri::{Emitter, State};

use super::tokens;
use super::{AccountSnapshot, SignInState};

fn api_base() -> String {
    tokens::api_url()
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
    let url = tokens::clerk_sign_in_url().ok_or("ARYA_CLERK_SIGN_IN_URL is not configured")?;
    super::signin_flow::begin(app, &url)
}

/// Dev-only: store a token directly (tests / local scripting). Gated out of
/// release builds so a content-injection in the webview can't hijack the
/// account by calling it; the real sign-in path is the loopback callback in
/// signin_flow, which validates a CSRF `state`.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn account_set_token(token: String) -> Result<(), String> {
    tokens::store(&token)
}

#[tauri::command]
pub fn account_sign_out(app: tauri::AppHandle) -> Result<(), String> {
    tokens::clear()?;
    // Let the shell (sidebar credits) refresh instead of showing a stale
    // balance until reload.
    let _ = app.emit("account:signed-out", ());
    Ok(())
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
