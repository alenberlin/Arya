//! Accounts (client side). Arya is free, open source, with no paid tiers.
//!
//! Sign-in uses a browser handoff: the app opens the hosted sign-in page with
//! a loopback redirect, receives the session token on a one-shot local HTTP
//! listener, and stores it in the macOS Keychain. In local/open-source mode
//! (no Clerk configured) a built-in dev token is used so the whole product
//! works offline. The account snapshot (credits, usage) comes from Arya
//! API's `/v1/account` — credits meter the optional hosted cloud proxy only,
//! never a paywall.

pub mod commands;
pub mod signin_flow;
pub mod tokens;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    pub user_id: String,
    pub included_credits: i64,
    pub used_credits: i64,
    pub topup_credits: i64,
    pub remaining_credits: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInState {
    pub signed_in: bool,
    /// True when Clerk is configured; false in local/open-source mode.
    pub hosted_auth: bool,
}
