//! Arya API: the confidential proxy between the desktop app and cloud model
//! providers. Holds provider keys server-side, verifies user identity
//! (Clerk JWT or local dev token), and meters usage with hold -> settle
//! semantics and exactly-once idempotency.
//!
//! Contract rule: `/v1/*` is additive-only. Never remove or repurpose an
//! endpoint, request field, or response field - shipped desktop builds keep
//! calling old shapes forever.

pub mod auth;
pub mod billing;
pub mod catalog;
pub mod config;
pub mod metering;
pub mod proxy;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<config::Config>,
    pub pool: sqlx::SqlitePool,
    pub http: reqwest::Client,
    pub verifier: Arc<auth::Verifier>,
    pub wallet: Arc<dyn billing::Wallet>,
}

/// Builds the router for a given state (shared by main and tests).
pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { axum::Json(serde_json::json!({ "ok": true, "service": "arya-api" })) }),
        )
        .route("/v1/models", get(catalog::list_models))
        .route("/v1/account", get(proxy::account))
        .route("/v1/{provider}/{*path}", post(proxy::forward))
        // Cap request bodies so an authenticated client can't exhaust memory.
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state)
}

/// Constructs application state from the environment.
pub async fn build_state() -> AppState {
    let config = Arc::new(config::Config::from_env());
    let pool = metering::init_pool(&config.database_path)
        .await
        .expect("metering database");
    let verifier = Arc::new(auth::Verifier::new(&config).await);
    AppState {
        config,
        pool,
        http: reqwest::Client::new(),
        verifier,
        wallet: Arc::new(billing::LocalWallet::from_env()),
    }
}
