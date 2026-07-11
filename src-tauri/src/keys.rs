//! Bring-your-own provider API keys, stored in the macOS Keychain.
//!
//! Arya is local-first and free: the agent, dictation, and notes all run on
//! free local models via Ollama. But a user with no Ollama — or who simply
//! wants frontier models — can paste their own OpenAI / Anthropic key here.
//! The key is written to the login Keychain (never to disk in plaintext, never
//! to a config file) and is handed to the model paths at call time:
//!
//!   * the agent sidecar receives it as `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`
//!     and calls the provider directly (see `agent::sidecar`),
//!   * the Rust cloud features (translate / transform / classify) read it via
//!     [`get`] and call the provider's API directly (see `crate::cloud`).
//!
//! The key material never crosses back to the frontend — the UI only ever sees
//! "set" / "not set" ([`KeysStatus`]) so a stored secret can't leak through a
//! log, a screenshot, or the devtools.

use serde::Serialize;

const SERVICE: &str = "dev.arya.app.keys";

/// A cloud LLM provider a user can supply their own key for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    Anthropic,
}

impl Provider {
    /// Parse the wire id used by the frontend and env plumbing.
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }

    /// Stable id used as the Keychain account and in `provider:model` strings.
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }
}

/// Which providers currently have a key stored. Booleans only — never the
/// secret itself, so this is safe to serialize to the frontend.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KeysStatus {
    pub openai: bool,
    pub anthropic: bool,
}

fn entry(provider: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, provider.id()).map_err(|e| e.to_string())
}

/// The stored key for `provider`, or `None` if the user hasn't set one.
/// Trims surrounding whitespace so a copy-paste with a trailing newline still
/// authenticates.
pub fn get(provider: Provider) -> Option<String> {
    let value = entry(provider).ok()?.get_password().ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Store (or replace) the key for `provider`. An empty/blank value clears it,
/// so the UI can treat "save an empty box" as "remove my key".
pub fn set(provider: Provider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return clear(provider);
    }
    entry(provider)?
        .set_password(key)
        .map_err(|e| e.to_string())
}

/// Remove the stored key for `provider`. Idempotent: clearing an absent key is
/// success, not an error.
pub fn clear(provider: Provider) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Which providers have a key stored (booleans only).
#[tauri::command]
pub fn keys_status() -> KeysStatus {
    KeysStatus {
        openai: get(Provider::OpenAi).is_some(),
        anthropic: get(Provider::Anthropic).is_some(),
    }
}

/// Store or replace a provider key. A blank `key` removes it. Returns the
/// refreshed status so the UI updates in one round-trip. Resets the agent
/// sidecars so the new key is picked up on the next message, no restart needed.
#[tauri::command]
pub fn keys_set(
    runtime: tauri::State<'_, crate::agent::AgentRuntime>,
    provider: String,
    key: String,
) -> Result<KeysStatus, String> {
    let provider =
        Provider::parse(&provider).ok_or_else(|| format!("unknown provider: {provider}"))?;
    set(provider, &key)?;
    runtime.reset_sidecars();
    Ok(keys_status())
}

/// Remove a provider key. Returns the refreshed status and resets the sidecars
/// so the agent stops offering that provider's models.
#[tauri::command]
pub fn keys_clear(
    runtime: tauri::State<'_, crate::agent::AgentRuntime>,
    provider: String,
) -> Result<KeysStatus, String> {
    let provider =
        Provider::parse(&provider).ok_or_else(|| format!("unknown provider: {provider}"))?;
    clear(provider)?;
    runtime.reset_sidecars();
    Ok(keys_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parse_roundtrip() {
        assert_eq!(Provider::parse("openai"), Some(Provider::OpenAi));
        assert_eq!(Provider::parse("anthropic"), Some(Provider::Anthropic));
        assert_eq!(Provider::parse("gemini"), None);
        assert_eq!(Provider::OpenAi.id(), "openai");
        assert_eq!(Provider::Anthropic.id(), "anthropic");
    }
}
