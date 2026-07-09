//! Request authentication: local shared token (dev/open-source mode) or
//! Clerk-issued RS256 JWTs verified against the instance JWKS.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::http::HeaderMap;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::config::{AuthMode, Config};

/// How often the background task refreshes JWKS even without a cache miss.
const JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(600);

pub struct Verifier {
    mode: VerifierMode,
}

enum VerifierMode {
    Local {
        token: String,
    },
    Clerk {
        issuer: String,
        audience: String,
        /// Live key set, swapped by the background refresher on rotation.
        keys: Arc<RwLock<Vec<Jwk>>>,
        /// Wakes the refresher when an unknown kid is seen (rotation catch-up).
        refresh: Arc<Notify>,
    },
}

#[derive(Clone, serde::Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
    /// Key type. A JWKS may advertise non-RSA keys; we only build RSA keys and
    /// pin RS256, so anything else is filtered out before it enters the set.
    #[serde(default)]
    kty: Option<String>,
}

#[derive(serde::Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(serde::Deserialize)]
struct Claims {
    sub: String,
}

impl Verifier {
    pub async fn new(config: &Config) -> Self {
        let mode = match &config.auth_mode {
            AuthMode::Local { token } => VerifierMode::Local {
                token: token.clone(),
            },
            AuthMode::Clerk {
                issuer,
                jwks_url,
                audience,
            } => {
                // Retry at boot so a transient JWKS fetch failure doesn't take
                // the whole server down (DoS-on-boot). If every attempt fails,
                // start with an empty set and let the refresher heal — never
                // panic.
                let initial = fetch_jwks_with_retry(jwks_url, 3)
                    .await
                    .unwrap_or_else(|e| {
                        eprintln!(
                        "arya-api: initial JWKS fetch failed ({e}); starting empty, will refresh"
                    );
                        Vec::new()
                    });
                let keys = Arc::new(RwLock::new(initial));
                let refresh = Arc::new(Notify::new());
                spawn_jwks_refresher(jwks_url.clone(), Arc::clone(&keys), Arc::clone(&refresh));
                VerifierMode::Clerk {
                    issuer: issuer.clone(),
                    audience: audience.clone(),
                    keys,
                    refresh,
                }
            }
        };
        Self { mode }
    }

    /// Returns the authenticated user id, or an error message.
    pub fn verify(&self, headers: &HeaderMap) -> Result<String, String> {
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or("missing bearer token")?;
        match &self.mode {
            VerifierMode::Local { token: expected } => {
                if ct_eq(token, expected) {
                    Ok("usr_local_dev".to_string())
                } else {
                    Err("invalid token".into())
                }
            }
            VerifierMode::Clerk {
                issuer,
                audience,
                keys,
                refresh,
            } => {
                let header = decode_header(token).map_err(|e| e.to_string())?;
                let kid = header.kid.ok_or("token missing kid")?;
                let jwk = {
                    let guard = keys.read().map_err(|_| "jwks lock poisoned")?;
                    match guard.iter().find(|k| k.kid == kid) {
                        Some(k) => k.clone(),
                        None => {
                            // A key rotated in after the last refresh: wake the
                            // refresher so the next attempt succeeds. This one
                            // still fails closed rather than trusting a stale set.
                            refresh.notify_one();
                            return Err("unknown kid".into());
                        }
                    }
                };
                let key =
                    DecodingKey::from_rsa_components(&jwk.n, &jwk.e).map_err(|e| e.to_string())?;
                // RS256 pinned (alg-confusion safe); issuer AND audience bound
                // so a token minted for another app on the same Clerk instance
                // is rejected.
                let mut validation = Validation::new(Algorithm::RS256);
                validation.set_issuer(&[issuer]);
                validation.set_audience(&[audience]);
                // Require the security-critical claims to be present, not merely
                // valid-if-present: a token missing exp/iss/aud must be rejected
                // rather than leniently accepted. (nbf is still validated when
                // present via the default.) Replay within the token's lifetime
                // is an accepted risk of stateless verification — closing it
                // needs a short-TTL seen-jti store, out of scope here.
                validation.set_required_spec_claims(&["exp", "iss", "aud"]);
                let data = decode::<Claims>(token, &key, &validation).map_err(|e| e.to_string())?;
                Ok(data.claims.sub)
            }
        }
    }
}

/// Constant-time token comparison via fixed-size digests, so a timing side
/// channel can't reveal how many leading bytes matched (or the token length).
fn ct_eq(a: &str, b: &str) -> bool {
    let ha = Sha256::digest(a.as_bytes());
    let hb = Sha256::digest(b.as_bytes());
    let mut diff = 0u8;
    for (x, y) in ha.iter().zip(hb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn fetch_jwks(jwks_url: &str) -> Result<Vec<Jwk>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let jwks = client
        .get(jwks_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json::<Jwks>()
        .await
        .map_err(|e| e.to_string())?;
    Ok(rsa_signing_keys(jwks.keys))
}

/// Keeps only RSA keys. Since the verifier pins RS256 and builds keys with
/// `from_rsa_components`, an EC/oct entry is at best useless and at worst a
/// foot-gun; drop it before it enters the trusted set. An absent `kty` is
/// treated as RSA for compatibility with minimal JWKS.
fn rsa_signing_keys(keys: Vec<Jwk>) -> Vec<Jwk> {
    keys.into_iter()
        .filter(|k| match k.kty.as_deref() {
            Some(t) => t == "RSA",
            None => true,
        })
        .collect()
}

async fn fetch_jwks_with_retry(jwks_url: &str, attempts: u32) -> Result<Vec<Jwk>, String> {
    let mut last = String::from("no attempts");
    for attempt in 0..attempts {
        match fetch_jwks(jwks_url).await {
            Ok(keys) => return Ok(keys),
            Err(e) => last = e,
        }
        if attempt + 1 < attempts {
            tokio::time::sleep(Duration::from_millis(500u64 << attempt)).await;
        }
    }
    Err(last)
}

/// Background task: refreshes JWKS on a fixed interval and whenever a cache miss
/// wakes it, so key rotation neither locks users out (new kid) nor keeps trust
/// in a retired kid past the next refresh.
fn spawn_jwks_refresher(jwks_url: String, keys: Arc<RwLock<Vec<Jwk>>>, refresh: Arc<Notify>) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(JWKS_REFRESH_INTERVAL) => {}
                _ = refresh.notified() => {}
            }
            match fetch_jwks(&jwks_url).await {
                Ok(fresh) if !fresh.is_empty() => {
                    if let Ok(mut guard) = keys.write() {
                        *guard = fresh;
                    }
                }
                Ok(_) => {} // an empty set means we keep the current keys
                Err(e) => eprintln!("arya-api: JWKS refresh failed: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_matches_only_identical_tokens() {
        assert!(ct_eq("local-dev-token", "local-dev-token"));
        assert!(!ct_eq("local-dev-token", "local-dev-toke"));
        assert!(!ct_eq("local-dev-token", "wrong"));
        assert!(!ct_eq("", "x"));
        assert!(ct_eq("", ""));
    }

    #[test]
    fn jwks_parses_clerk_shape() {
        let json = r#"{"keys":[{"kid":"abc","n":"AQAB","e":"AQAB"}]}"#;
        let jwks: Jwks = serde_json::from_str(json).unwrap();
        assert_eq!(jwks.keys.len(), 1);
        assert_eq!(jwks.keys[0].kid, "abc");
    }

    #[test]
    fn rsa_signing_keys_drops_non_rsa_entries() {
        let json = r#"{"keys":[
            {"kty":"RSA","kid":"rsa1","n":"AQAB","e":"AQAB"},
            {"kty":"EC","kid":"ec1","n":"x","e":"y"},
            {"kid":"nokty","n":"AQAB","e":"AQAB"}
        ]}"#;
        let jwks: Jwks = serde_json::from_str(json).unwrap();
        let kept = rsa_signing_keys(jwks.keys);
        let kids: Vec<&str> = kept.iter().map(|k| k.kid.as_str()).collect();
        // RSA and kty-absent are kept; EC is dropped.
        assert_eq!(kids, vec!["rsa1", "nokty"]);
    }
}
