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
pub mod ratelimit;

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
    pub rate_limit: Arc<ratelimit::RateLimit>,
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
        http: build_http_client(),
        verifier,
        wallet: Arc::new(billing::LocalWallet::from_env()),
        // Per-user throttle so a leaked-but-valid token can't drain the wallet
        // as fast as the upstream will answer.
        rate_limit: Arc::new(ratelimit::RateLimit::new(
            120,
            std::time::Duration::from_secs(60),
        )),
    }
}

/// Ceiling on a single upstream request's total wall-clock time. It MUST stay
/// below the metering hold TTL (`proxy::HOLD_TTL_SECONDS`): if a request could
/// outlive its hold, the hold would expire mid-flight, stop counting against
/// the balance, and let a concurrent request overspend (the C1 billing TOCTOU).
pub const UPSTREAM_TOTAL_TIMEOUT_SECS: u64 = 300;

/// The upstream HTTP client. Redirects are disabled so a 302 can't replay the
/// `Authorization: Bearer <provider-key>` to an attacker host. `connect_timeout`
/// bounds the handshake, `read_timeout` kills a stalled stream (per-read), and
/// the total `timeout` caps the whole request so it can never outlive its
/// metering hold (see `UPSTREAM_TOTAL_TIMEOUT_SECS`).
pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(120))
        .timeout(std::time::Duration::from_secs(UPSTREAM_TOTAL_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build upstream http client")
}
