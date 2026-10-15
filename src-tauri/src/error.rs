//! The app's single error type, replacing the ~190 ad-hoc `Result<_, String>`
//! signatures so errors carry one type everywhere and serialize to the frontend
//! as a plain message (no behavioural change to the UI).

use serde::Serialize;

/// A user-facing error message. Commands return [`AryaResult`]; Tauri serializes
/// the inner message as a plain string, identical to the previous `String` error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AryaError(String);

impl AryaError {
    /// Build an error from a message.
    pub fn message(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }

    /// Convert any `Display` source into an error. Kept as an associated fn (not
    /// a blanket `From<E: Display>`, which would conflict with std's `From<T> for
    /// T`) so call sites can write `map_err(AryaError::from_display)`.
    pub fn from_display<E: std::fmt::Display>(e: E) -> Self {
        Self(e.to_string())
    }
}

impl std::fmt::Display for AryaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AryaError {}

impl Serialize for AryaError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl From<String> for AryaError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AryaError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<sqlx::Error> for AryaError {
    fn from(e: sqlx::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<std::io::Error> for AryaError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<serde_json::Error> for AryaError {
    fn from(e: serde_json::Error) -> Self {
        Self(e.to_string())
    }
}

/// The app-wide result alias.
pub type AryaResult<T> = Result<T, AryaError>;
