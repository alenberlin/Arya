//! Arya API server entry point.

use arya_api::{build_app, build_state};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "arya_api=info,axum=info".into()),
        )
        .init();

    let state = build_state().await;
    let bind = state.config.bind.clone();
    let label = state.config.auth_mode_label();
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
    tracing::info!(%bind, mode = %label, "arya-api listening");
    axum::serve(listener, app).await.expect("serve");
}
