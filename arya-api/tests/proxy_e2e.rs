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
        wallet: Arc::new(arya_api::billing::LocalWallet::from_env()),
        rate_limit: Arc::new(arya_api::ratelimit::RateLimit::new(
            10_000,
            std::time::Duration::from_secs(60),
        )),
    })
}

/// A proxy app whose ollama upstream points at a caller-supplied URL (a fake
/// upstream server), with no cloud provider keys.
async fn app_ollama(ollama_url: String) -> axum::Router {
    let (app, _) = app_ollama_with_pool(ollama_url).await;
    app
}

async fn app_ollama_with_pool(ollama_url: String) -> (axum::Router, sqlx::SqlitePool) {
    let config = Arc::new(Config {
        bind: "127.0.0.1:0".into(),
        database_path: ":memory:".into(),
        auth_mode: AuthMode::Local {
            token: "test-token".into(),
        },
        anthropic_key: None,
        openai_key: None,
        ollama_url,
    });
    let pool = arya_api::metering::init_pool(":memory:").await.unwrap();
    let verifier = Arc::new(arya_api::auth::Verifier::new(&config).await);
    let app = build_app(AppState {
        config,
        pool: pool.clone(),
        http: arya_api::build_http_client(),
        verifier,
        wallet: Arc::new(arya_api::billing::LocalWallet::from_env()),
        rate_limit: Arc::new(arya_api::ratelimit::RateLimit::new(
            10_000,
            std::time::Duration::from_secs(60),
        )),
    });
    (app, pool)
}

/// Spawns a fake OpenAI-compatible upstream on a loopback port and returns its
/// base URL. `sse` selects a streamed vs a buffered JSON response.
async fn spawn_fake_upstream(sse: bool) -> String {
    use axum::routing::post;
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        post(move || async move {
            if sse {
                axum::response::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                         data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n\
                         data: [DONE]\n\n",
                    ))
                    .unwrap()
            } else {
                axum::response::Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#,
                    ))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn spawn_large_upstream() -> String {
    use axum::routing::post;
    let payload = Arc::new("x".repeat(17 * 1024 * 1024));
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let payload = Arc::clone(&payload);
            async move {
                axum::response::Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from((*payload).clone()))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
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
async fn account_endpoint_returns_snapshot() {
    let app = app_with(true, true).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/account")
                .header("authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["success"], true);
    assert!(json["data"]["remainingCredits"].as_i64().unwrap() > 0);
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

/// The streaming contract fix: a `stream:true` request must come back as raw
/// OpenAI SSE (text/event-stream, forwarded chunks, `[DONE]`), NOT buffered
/// inside a `{success,data}` envelope the streaming client can't parse.
#[tokio::test]
async fn streams_sse_through_as_raw_openai() {
    let upstream = spawn_fake_upstream(true).await;
    let app = app_ollama(upstream).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ollama/chat/completions")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"llama3.2","messages":[{"role":"user","content":"hi"}],"stream":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("[DONE]"), "missing DONE: {text}");
    assert!(
        text.contains(r#""content":"ok""#),
        "missing forwarded content: {text}"
    );
    assert!(
        !text.contains(r#""success""#),
        "should be raw SSE, not an envelope: {text}"
    );
}

/// The non-streaming contract fix: raw OpenAI JSON (top-level `choices`, no
/// envelope) plus metering metadata in `X-Arya-*` headers.
#[tokio::test]
async fn buffered_returns_raw_openai_with_meta_headers() {
    let upstream = spawn_fake_upstream(false).await;
    let app = app_ollama(upstream).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ollama/chat/completions")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"llama3.2","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("x-arya-model").unwrap(), "ollama:*");
    assert!(response.headers().get("x-arya-credits-charged").is_some());
    let json = body_json(response).await;
    assert!(
        json["choices"].is_array(),
        "expected raw OpenAI choices: {json}"
    );
    assert!(
        json.get("success").is_none(),
        "must not be enveloped: {json}"
    );
}

/// Opt-in idempotency: two requests carrying the same `Idempotency-Key` bill
/// once and the retry replays the cached body (provider not re-invoked).
#[tokio::test]
async fn idempotency_key_replays_cached_response() {
    let upstream = spawn_fake_upstream(false).await;
    let app = app_ollama(upstream).await;
    let make = || {
        Request::builder()
            .method("POST")
            .uri("/v1/ollama/chat/completions")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .header("idempotency-key", "abc-123")
            .body(Body::from(
                r#"{"model":"llama3.2","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
            ))
            .unwrap()
    };
    let first = app.clone().oneshot(make()).await.unwrap();
    assert!(first.headers().get("x-arya-idempotent-replay").is_none());

    let retry = app.oneshot(make()).await.unwrap();
    assert_eq!(
        retry.headers().get("x-arya-idempotent-replay").unwrap(),
        "true"
    );
    let json = body_json(retry).await;
    assert!(
        json["choices"].is_array(),
        "replay should return the cached body: {json}"
    );
}

#[tokio::test]
async fn oversized_buffered_response_settles_hold_without_replay_cache() {
    let upstream = spawn_large_upstream().await;
    let (app, pool) = app_ollama_with_pool(upstream).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ollama/chat/completions")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .header("idempotency-key", "too-large")
                .body(Body::from(
                    r#"{"model":"llama3.2","messages":[{"role":"user","content":"hi"}],"stream":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let json = body_json(response).await;
    assert_eq!(json["errors"][0]["code"], "upstream_too_large");

    let open_holds: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM holds WHERE settled = 0")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(open_holds, 0);
    assert_eq!(
        arya_api::metering::total_charged(&pool, "usr_local_dev")
            .await
            .unwrap(),
        500
    );
    let cached: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM response_cache WHERE idempotency_key = ?1")
            .bind("agent_chat:usr_local_dev:too-large")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cached, 0);
}

/// Full metered round trip against a REAL local Ollama model (ignored by
/// default): raw OpenAI shape + metering headers, then an opt-in idempotent
/// retry replays without re-billing.
#[tokio::test]
#[ignore = "requires a local Ollama with a small model"]
async fn metered_roundtrip_settles_exactly_once() {
    let model = std::env::var("ARYA_TEST_MODEL").unwrap_or_else(|_| "qwen3.6:35b".into());
    let app = app_with(false, false).await;
    let make = || {
        Request::builder()
            .method("POST")
            .uri("/v1/ollama/chat/completions")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .header("idempotency-key", "roundtrip-1")
            .body(Body::from(format!(
                r#"{{"model":"{model}","messages":[{{"role":"user","content":"Reply with just: ok"}}],"stream":false}}"#
            )))
            .unwrap()
    };

    let first = app.clone().oneshot(make()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let charged: u64 = first
        .headers()
        .get("x-arya-credits-charged")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(charged >= 1);
    let first_json = body_json(first).await;
    assert!(first_json["choices"].is_array(), "raw shape: {first_json}");

    let retry = app.oneshot(make()).await.unwrap();
    assert_eq!(
        retry.headers().get("x-arya-idempotent-replay").unwrap(),
        "true"
    );
}
