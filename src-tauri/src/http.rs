//! Shared HTTP clients. Constructing a reqwest client builds a fresh connection
//! pool + TLS config, so the local-service callers (Ollama, the Arya proxy)
//! reuse one pooled blocking client instead of paying that on every request.

use std::sync::OnceLock;

/// A process-wide blocking HTTP client with a pooled connection. Callers set
/// their own per-request timeouts.
pub fn blocking_client() -> reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::blocking::Client::new).clone()
}
