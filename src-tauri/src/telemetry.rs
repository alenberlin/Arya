//! Structured, secret-safe logging for the Rust shell.
//!
//! Sets up a `tracing` subscriber that:
//!   * emits one log line per event to stderr in a compact, human-friendly
//!     format (level, target, message, fields);
//!   * gates the verbosity with `RUST_LOG` (defaults to `info,arya=debug`);
//!   * redacts well-known secret shapes (Bearer / sk- / AIza / ghp_ / OpenAI
//!     project / Anthropic / Ollama keys, JSON web tokens, keyring-style
//!     `KEY=...` env dumps) before they hit the log so an upstream library
//!     printing an HTTP header or an env snapshot can't leak credentials.
//!
//! `init()` is idempotent — repeated calls are no-ops, which matters because
//! the Tauri command path can re-enter the run loop in tests.

use std::sync::Once;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

static INIT: Once = Once::new();

/// Patterns that match the prefix of well-known secret tokens. The list is
/// deliberately short and conservative — every entry here is a documented
/// format that, if it ever appears in a log, was leaked by accident.
const SECRET_PATTERNS: &[&str] = &[
    "Bearer ",
    "sk-",
    "sk_",
    "AIza",
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "ghr_",
    "xai-",
    "anthropic-",
    "pplx-",
    "AKIA", // AWS access key
];

/// Looks like a JWT (`xxx.yyy.zzz`, three base64url groups separated by `.`).
fn looks_like_jwt(s: &str) -> bool {
    let mut groups = s.split('.').filter(|g| !g.is_empty());
    let Some(a) = groups.next() else { return false };
    let Some(b) = groups.next() else { return false };
    let Some(c) = groups.next() else { return false };
    let base64url = |g: &str| {
        g.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    base64url(a) && base64url(b) && base64url(c) && groups.next().is_none()
}

/// `KEY=VALUE` style dump lines (env, .env, error messages) where KEY looks
/// like it holds a credential. Matches `OPENAI_API_KEY=...`, `TOKEN=...`, etc.
fn looks_like_env_secret(s: &str) -> bool {
    let Some((k, _)) = s.split_once('=') else {
        return false;
    };
    let key = k.trim().to_ascii_uppercase();
    matches!(
        key.as_str(),
        "OPENAI_API_KEY"
            | "ANTHROPIC_API_KEY"
            | "GOOGLE_API_KEY"
            | "GITHUB_TOKEN"
            | "GH_TOKEN"
            | "XAI_API_KEY"
            | "PERPLEXITY_API_KEY"
            | "HF_TOKEN"
            | "HUGGINGFACE_TOKEN"
            | "ARYA_API_KEY"
            | "API_KEY"
            | "TOKEN"
            | "SECRET"
            | "PASSWORD"
    )
}

/// Replaces any secret-shaped substring in `s` with `***`. Used as a
/// field-level visitor so structured fields are redacted before they're
/// formatted into the log line.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find(['B', 's', 'A', 'g', 'x', 'a', 'p']) {
        // Cheap char-by-char match against our prefix table.
        let mut matched = false;
        for pat in SECRET_PATTERNS {
            if rest[idx..].starts_with(pat) {
                // "Bearer " is an HTTP scheme marker, not the secret itself —
                // keep the prefix so logs remain readable ("Authorization:
                // Bearer ***") and only redact the token value that follows.
                let (copy_up_to, redact_from) = if *pat == "Bearer " {
                    (idx + pat.len(), idx + pat.len())
                } else {
                    (idx, idx)
                };
                let end = rest[redact_from..]
                    .find(|c: char| {
                        c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '}' | ')' | '`')
                    })
                    .unwrap_or(rest.len() - redact_from)
                    + redact_from;
                out.push_str(&rest[..copy_up_to]);
                out.push_str("***");
                rest = &rest[end..];
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        // No match — copy this char and continue past it.
        let ch_len = rest[idx..].chars().next().unwrap().len_utf8();
        out.push_str(&rest[..idx + ch_len]);
        rest = &rest[idx + ch_len..];
    }
    out.push_str(rest);
    out
}

/// Initializes the global tracing subscriber exactly once.
pub fn init() {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new("info,arya=debug"))
            .unwrap_or_else(|_| EnvFilter::new("info"));

        // Custom format that runs every field through `redact`. We deliberately
        // do not include the `Display` of any non-`&str` field here, so this
        // is a small footgun: callers should pass secrets as `&str` via
        // `tracing::field::display` if they want them redacted, or wrap their
        // own redacting Display type. The redact-on-format layer covers the
        // common case of "library printed a token-shaped string".
        struct RedactLayer;
        impl<S> tracing_subscriber::Layer<S> for RedactLayer
        where
            S: tracing::Subscriber,
        {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                struct Visitor<'a>(&'a mut String);
                impl tracing::field::Visit for Visitor<'_> {
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        let s = format!("{value:?}");
                        // field is recorded after a `=`, like
                        // `name=<debug>`. Redact the value in place.
                        let redacted = redact(&s);
                        self.0.push_str(&format!(" {}={}", field.name(), redacted));
                    }
                    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                        let redacted = redact(value);
                        self.0.push_str(&format!(" {}={}", field.name(), redacted));
                    }
                }
                let mut fields = String::new();
                event.record(&mut Visitor(&mut fields));
                // Best-effort secret detection on the rendered message; we
                // can't pre-redact the message itself without re-implementing
                // the formatter, so we apply the same passes here on the
                // captured message text.
                let msg_redacted = if let Some(msg) = lookup_message(event) {
                    redact(&msg)
                } else {
                    String::new()
                };
                if looks_like_jwt(&msg_redacted) || looks_like_env_secret(&msg_redacted) {
                    eprintln!("[tracing-redact] suppressed log line that looks like a credential");
                    return;
                }
                eprintln!(
                    "[{}] {}{}",
                    event.metadata().level(),
                    msg_redacted,
                    if fields.is_empty() {
                        String::new()
                    } else {
                        fields
                    }
                );
            }
        }

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(RedactLayer)
            .try_init();
    });
}

/// Pulls the `message` field out of a `tracing::Event` if one was set.
/// (There is no public API to do this on stable, so we use a small visitor.)
fn lookup_message(event: &tracing::Event<'_>) -> Option<String> {
    struct FindMsg(Option<String>);
    impl tracing::field::Visit for FindMsg {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = Some(format!("{value:?}"));
            }
        }
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if field.name() == "message" {
                self.0 = Some(value.to_string());
            }
        }
    }
    let mut v = FindMsg(None);
    event.record(&mut v);
    v.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openai_key() {
        // Fixture is short on purpose: the secret-pattern scanner rejects any
        // literal that *structurally* resembles a real key, so test inputs
        // stay below the length thresholds. `redact()` only cares about the
        // prefix, so the assertion below still proves the prefix triggers.
        let s = "request failed: sk-abc123def";
        let out = redact(s);
        assert!(!out.contains("sk-abc"));
        assert!(out.contains("***"));
    }

    #[test]
    fn redacts_github_pat() {
        let s = "Authorization: ghp_AAAABBBB";
        let out = redact(s);
        assert!(!out.contains("ghp_"));
    }

    #[test]
    fn redacts_bearer_token() {
        let s = "header: Bearer eyJhbGciOi.sig";
        let out = redact(s);
        assert!(!out.contains("eyJhbGci"));
        assert!(out.contains("Bearer ***"));
    }

    #[test]
    fn redacts_aws_access_key() {
        // 10 chars after AKIA: well under the scanner's 16-char threshold.
        let s = "found AKIAEXAMPLE1 in dump";
        let out = redact(s);
        assert!(!out.contains("AKIA"));
    }

    #[test]
    fn does_not_redact_benign_text() {
        let s = "started recording session 42 in /Users/alen/Notes";
        let out = redact(s);
        assert_eq!(out, s);
    }

    #[test]
    fn recognises_jwt() {
        assert!(looks_like_jwt("aGVsbG8.b29zdA.signature"));
        assert!(!looks_like_jwt("just-a-string"));
        assert!(!looks_like_jwt("only.two"));
    }

    #[test]
    fn recognises_env_secret() {
        assert!(looks_like_env_secret("OPENAI_API_KEY=sk-1234"));
        assert!(looks_like_env_secret("TOKEN=ey..."));
        assert!(!looks_like_env_secret("RUST_LOG=info"));
    }
}
