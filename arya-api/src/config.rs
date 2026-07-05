//! Environment-driven configuration. Provider keys live ONLY here, on the
//! server - never in the desktop binary.

/// The well-known local dev token. Refused on a non-loopback bind so a
/// misconfigured public deploy can't expose a shared, guessable bearer.
const DEFAULT_LOCAL_TOKEN: &str = "local-dev-token";

pub struct Config {
    pub bind: String,
    pub database_path: String,
    /// "local" (shared dev token) or "clerk" (JWKS-verified JWTs).
    pub auth_mode: AuthMode,
    pub anthropic_key: Option<String>,
    pub openai_key: Option<String>,
    /// Local Ollama upstream for the free tier / dev E2E.
    pub ollama_url: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redact provider keys to presence-only so an accidental `{:?}` on
        // Config can never leak a secret into logs.
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .field("database_path", &self.database_path)
            .field("auth_mode", &self.auth_mode_label())
            .field(
                "anthropic_key",
                &self.anthropic_key.as_ref().map(|_| "<set>"),
            )
            .field("openai_key", &self.openai_key.as_ref().map(|_| "<set>"))
            .field("ollama_url", &self.ollama_url)
            .finish()
    }
}

pub enum AuthMode {
    Local {
        token: String,
    },
    Clerk {
        issuer: String,
        jwks_url: String,
        /// Expected `aud`; only tokens for this audience are accepted.
        audience: String,
    },
}

impl Config {
    pub fn from_env() -> Self {
        let local_token =
            std::env::var("ARYA_API_LOCAL_TOKEN").unwrap_or_else(|_| DEFAULT_LOCAL_TOKEN.into());
        let auth_mode = match std::env::var("ARYA_API_MODE").as_deref() {
            Ok("clerk") => AuthMode::Clerk {
                issuer: std::env::var("CLERK_ISSUER").expect("CLERK_ISSUER required in clerk mode"),
                jwks_url: std::env::var("CLERK_JWKS_URL")
                    .expect("CLERK_JWKS_URL required in clerk mode"),
                audience: std::env::var("CLERK_AUDIENCE")
                    .expect("CLERK_AUDIENCE required in clerk mode"),
            },
            // Explicit "local", or unset, → local shared-token mode.
            Ok("local") | Err(_) => AuthMode::Local { token: local_token },
            // A typo must never silently fall back to a shared dev token.
            Ok(other) => panic!("ARYA_API_MODE must be 'local' or 'clerk', got '{other}'"),
        };
        Self {
            bind: std::env::var("ARYA_API_BIND").unwrap_or_else(|_| "127.0.0.1:8477".into()),
            database_path: std::env::var("ARYA_API_DB").unwrap_or_else(|_| "arya-api.db".into()),
            auth_mode,
            anthropic_key: std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|k| !k.is_empty()),
            openai_key: std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|k| !k.is_empty()),
            ollama_url: std::env::var("ARYA_OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
        }
    }

    /// Startup validation of security-sensitive config.
    ///
    /// - The local ollama upstream must be loopback: the proxy forwards a
    ///   client-controlled body to it, so a routable URL is an SSRF pivot.
    /// - The default local dev token must never be reachable off-box.
    ///
    /// Loopback is decided by parsing the host as an IP (127.0.0.0/8, ::1), so
    /// decimal/octal/hostname spoofs fail closed rather than sneaking through a
    /// naive string prefix match.
    pub fn validate(&self) -> Result<(), String> {
        if !host_is_loopback(host_of_url(&self.ollama_url)) {
            return Err(format!(
                "ARYA_OLLAMA_URL must be loopback (got '{}'); a routable ollama upstream is an SSRF risk",
                self.ollama_url
            ));
        }
        if let AuthMode::Local { token } = &self.auth_mode {
            if token == DEFAULT_LOCAL_TOKEN && !host_is_loopback(host_only(&self.bind)) {
                return Err(format!(
                    "refusing to serve the default local dev token on a non-loopback bind ('{}'); \
                     set ARYA_API_LOCAL_TOKEN to a strong secret or bind to loopback",
                    self.bind
                ));
            }
        }
        Ok(())
    }

    pub fn auth_mode_label(&self) -> &'static str {
        match self.auth_mode {
            AuthMode::Local { .. } => "local",
            AuthMode::Clerk { .. } => "clerk",
        }
    }
}

/// Strips a trailing `:port` from an authority, leaving the host (ipv6-aware).
fn host_only(authority: &str) -> &str {
    match authority.rfind(':') {
        Some(i) if !authority[i..].contains(']') => &authority[..i],
        _ => authority,
    }
}

/// Extracts the host from an `http(s)://host[:port][/...]` URL.
fn host_of_url(url: &str) -> &str {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    host_only(authority)
}

/// True only for genuine loopback: `localhost`, or an IP in 127.0.0.0/8 / ::1.
/// Parsing as an IP makes decimal (`2130706433`), octal (`0177.0.0.1`), and
/// look-alike hostnames (`127.evil.com`) fail closed instead of matching a
/// naive `starts_with("127.")`.
fn host_is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection_rejects_spoofed_hosts() {
        assert!(host_is_loopback("127.0.0.1"));
        assert!(host_is_loopback("localhost"));
        assert!(host_is_loopback("::1"));
        assert!(host_is_loopback("127.4.5.6"));
        // Decimal / octal / look-alike hostnames must fail closed.
        assert!(!host_is_loopback("2130706433"));
        assert!(!host_is_loopback("0177.0.0.1"));
        assert!(!host_is_loopback("127.evil.com"));
        assert!(!host_is_loopback("0.0.0.0"));
        assert!(!host_is_loopback("10.0.0.5"));
    }

    #[test]
    fn host_extraction_handles_ports_and_paths() {
        assert_eq!(host_of_url("http://127.0.0.1:11434"), "127.0.0.1");
        assert_eq!(host_of_url("http://localhost:11434/v1/x"), "localhost");
        assert_eq!(host_only("0.0.0.0:8477"), "0.0.0.0");
    }

    fn local_cfg(bind: &str, token: &str, ollama: &str) -> Config {
        Config {
            bind: bind.into(),
            database_path: ":memory:".into(),
            auth_mode: AuthMode::Local {
                token: token.into(),
            },
            anthropic_key: None,
            openai_key: None,
            ollama_url: ollama.into(),
        }
    }

    #[test]
    fn default_token_refused_on_public_bind() {
        let c = local_cfg(
            "0.0.0.0:8477",
            DEFAULT_LOCAL_TOKEN,
            "http://127.0.0.1:11434",
        );
        assert!(c.validate().is_err());
    }

    #[test]
    fn default_token_ok_on_loopback_bind() {
        let c = local_cfg(
            "127.0.0.1:8477",
            DEFAULT_LOCAL_TOKEN,
            "http://127.0.0.1:11434",
        );
        assert!(c.validate().is_ok());
    }

    #[test]
    fn custom_token_allowed_on_public_bind() {
        let c = local_cfg("0.0.0.0:8477", "a-strong-secret", "http://127.0.0.1:11434");
        assert!(c.validate().is_ok());
    }

    #[test]
    fn routable_ollama_rejected() {
        let c = local_cfg(
            "127.0.0.1:8477",
            DEFAULT_LOCAL_TOKEN,
            "http://10.0.0.5:11434",
        );
        assert!(c.validate().is_err());
    }
}
