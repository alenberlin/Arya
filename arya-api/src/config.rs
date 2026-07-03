//! Environment-driven configuration. Provider keys live ONLY here, on the
//! server - never in the desktop binary.

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
        let auth_mode = match std::env::var("ARYA_API_MODE").as_deref() {
            Ok("clerk") => AuthMode::Clerk {
                issuer: std::env::var("CLERK_ISSUER").expect("CLERK_ISSUER required in clerk mode"),
                jwks_url: std::env::var("CLERK_JWKS_URL")
                    .expect("CLERK_JWKS_URL required in clerk mode"),
                audience: std::env::var("CLERK_AUDIENCE")
                    .expect("CLERK_AUDIENCE required in clerk mode"),
            },
            _ => AuthMode::Local {
                token: std::env::var("ARYA_API_LOCAL_TOKEN")
                    .unwrap_or_else(|_| "local-dev-token".into()),
            },
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

    /// The local ollama upstream must be a loopback address: the proxy
    /// forwards a client-controlled body to it, so a routable URL would be an
    /// SSRF pivot into the deploy's network. Called at startup.
    pub fn validate(&self) -> Result<(), String> {
        let host = self
            .ollama_url
            .strip_prefix("http://")
            .or_else(|| self.ollama_url.strip_prefix("https://"))
            .unwrap_or(&self.ollama_url);
        let host = host.split([':', '/']).next().unwrap_or("");
        let is_loopback =
            host == "localhost" || host == "127.0.0.1" || host == "::1" || host.starts_with("127.");
        if !is_loopback {
            return Err(format!(
                "ARYA_OLLAMA_URL must be loopback (got '{}'); a routable ollama upstream is an SSRF risk",
                self.ollama_url
            ));
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
