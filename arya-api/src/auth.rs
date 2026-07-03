//! Request authentication: local shared token (dev/open-source mode) or
//! Clerk-issued RS256 JWTs verified against the instance JWKS.

use axum::http::HeaderMap;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

use crate::config::{AuthMode, Config};

pub struct Verifier {
    mode: VerifierMode,
}

enum VerifierMode {
    Local { token: String },
    Clerk { issuer: String, keys: Vec<Jwk> },
}

#[derive(Clone, serde::Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
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
            AuthMode::Clerk { issuer, jwks_url } => {
                #[derive(serde::Deserialize)]
                struct Jwks {
                    keys: Vec<Jwk>,
                }
                let keys = reqwest::get(jwks_url)
                    .await
                    .and_then(|r| r.error_for_status())
                    .expect("fetch Clerk JWKS")
                    .json::<Jwks>()
                    .await
                    .expect("parse Clerk JWKS")
                    .keys;
                VerifierMode::Clerk {
                    issuer: issuer.clone(),
                    keys,
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
                if token == expected {
                    Ok("usr_local_dev".to_string())
                } else {
                    Err("invalid token".into())
                }
            }
            VerifierMode::Clerk { issuer, keys } => {
                let header = decode_header(token).map_err(|e| e.to_string())?;
                let kid = header.kid.ok_or("token missing kid")?;
                let jwk = keys.iter().find(|k| k.kid == kid).ok_or("unknown kid")?;
                let key =
                    DecodingKey::from_rsa_components(&jwk.n, &jwk.e).map_err(|e| e.to_string())?;
                let mut validation = Validation::new(Algorithm::RS256);
                validation.set_issuer(&[issuer]);
                validation.validate_aud = false;
                let data = decode::<Claims>(token, &key, &validation).map_err(|e| e.to_string())?;
                Ok(data.claims.sub)
            }
        }
    }
}
