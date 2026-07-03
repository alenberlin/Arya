//! End-to-end proxy tests driving the real router in-process against a local
//! Ollama upstream. Exercises auth -> catalog -> hold -> upstream ->
//! exactly-once settle. The upstream-dependent test is ignored by default.

use std::sync::Arc;

use arya_api::config::{AuthMode, Config};
use arya_api::{build_app, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn app_with(anthropic: bool, openai: bool) -> axum::Router {
    let config = Arc::new(Config {
        bind: "127.0.0.1:0".into(),
        database_path: ":memory:".into(),
        auth_mode: AuthMode::Local {
            token: "test-token".into(),
        },
        anthropic_key: anthropic.then(|| "sk-test".into()),
        openai_key: openai.then(|| "sk-test".into()),
        ollama_url: std::env::var("ARYA_OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
    });
    let pool = arya_api::metering::init_pool(":memory:").await.unwrap();
    let verifier = Arc::new(arya_api::auth::Verifier::new(&config).await);
    build_app(AppState {
        config,
        pool,
        http: reqwest::Client::new(),
        verifier,
    })
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn rejects_missing_bearer_token() {
    let app = app_with(true, true).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/anthropic/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"claude-sonnet-5","messages":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_unpriced_model_before_touching_provider() {
    let app = app_with(true, true).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/anthropic/chat/completions")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"totally-made-up","messages":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["errors"][0]["code"], "model_not_priced");
}

#[tokio::test]
async fn models_endpoint_lists_only_configured_providers() {
    let app = app_with(false, true).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(response).await;
    let ids: Vec<String> = json["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert!(ids.iter().any(|id| id.starts_with("openai:")));
    assert!(!ids.iter().any(|id| id.starts_with("anthropic:")));
    // Local models are always listed.
    assert!(ids.iter().any(|id| id == "ollama:*"));
}

/// Full metered round trip against a local Ollama model, then a forced retry
/// with the same body to prove exactly-once settlement across the HTTP layer.
#[tokio::test]
#[ignore = "requires a local Ollama with a small model"]
async fn metered_roundtrip_settles_exactly_once() {
    let model = std::env::var("ARYA_TEST_MODEL").unwrap_or_else(|_| "qwen3.6:35b".into());
    let app = app_with(false, false).await;
    let body = format!(
        r#"{{"model":"{model}","messages":[{{"role":"user","content":"Reply with just: ok"}}],"stream":false}}"#
    );
    let make = || {
        Request::builder()
            .method("POST")
            .uri("/v1/ollama/chat/completions")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .body(Body::from(body.clone()))
            .unwrap()
    };

    let first = body_json(app.clone().oneshot(make()).await.unwrap()).await;
    assert_eq!(first["success"], true, "first call: {first}");
    assert_eq!(first["meta"]["idempotentReplay"], false);
    let charged = first["meta"]["creditsCharged"].as_u64().unwrap();
    assert!(charged >= 1);

    let retry = body_json(app.oneshot(make()).await.unwrap()).await;
    assert_eq!(retry["meta"]["idempotentReplay"], true, "retry: {retry}");
    assert_eq!(retry["meta"]["creditsCharged"].as_u64().unwrap(), charged);
}
