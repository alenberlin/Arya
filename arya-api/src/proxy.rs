//! Metered chat-completions proxy.
//!
//! POST /v1/{provider}/chat/completions with an OpenAI-shaped body. Flow:
//! verify identity -> resolve+price the model (reject if unpriced) ->
//! authorize a hold -> call upstream with the server-held key (streaming or
//! buffered, per the client's own `stream` flag) -> settle actual token usage
//! exactly once.
//!
//! The response is a RAW OpenAI-compatible payload (SSE for `stream:true`, JSON
//! otherwise), NOT an envelope, so an OpenAI-compatible client (the sidecar's
//! `streamText`, the desktop translator) can consume it directly. Metering
//! metadata rides in `X-Arya-*` response headers. The upstream never sees the
//! user id or any Arya identifier (structural anonymization).

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::Json;
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;

use crate::{billing, catalog, metering, AppState};

/// Per-request hold estimate. Actual usage settles against this; metering::settle
/// warns if a response exceeds it (cost-recovery leak) so the cap can be tuned.
const HOLD_CAP_CREDITS: u64 = 500;

/// Bytes we buffer (non-streaming) or accumulate for usage parsing (streaming).
/// The streamed bytes forwarded to the client are NOT capped by this — only the
/// server-side copy kept for metering/caching is.
const MAX_UPSTREAM_BYTES: usize = 16 * 1024 * 1024;

/// GET /v1/account — the snapshot the desktop shows (tier, credits, usage).
/// This endpoint is Arya-specific and keeps the `{success,data}` envelope.
pub async fn account(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = state
        .verifier
        .verify(&headers)
        .map_err(|e| error(StatusCode::UNAUTHORIZED, "unauthorized", &e))?;
    let snapshot = billing::account_snapshot(&state.pool, state.wallet.as_ref(), &user_id)
        .await
        .map_err(|e| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "metering_failure",
                &e.to_string(),
            )
        })?;
    Ok(Json(
        serde_json::json!({ "success": true, "data": snapshot }),
    ))
}

/// Everything settlement needs, so the streaming and buffered paths meter the
/// same way.
struct SettleCtx {
    pool: sqlx::SqlitePool,
    hold: metering::Hold,
    user_id: String,
    action: &'static str,
    idempotency_key: String,
    entry: &'static catalog::ModelEntry,
}

pub async fn forward(
    State(state): State<AppState>,
    Path((provider, path)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let user_id = state
        .verifier
        .verify(&headers)
        .map_err(|e| error(StatusCode::UNAUTHORIZED, "unauthorized", &e))?;

    if path != "chat/completions" {
        return Err(error(
            StatusCode::NOT_FOUND,
            "unknown_endpoint",
            "only chat/completions is proxied in v1",
        ));
    }

    // The desktop may send a bare ("claude-sonnet-5") or an already-qualified
    // ("anthropic:claude-sonnet-5") model id; normalize both to one catalog id
    // instead of double-prefixing into a 404.
    let raw_model = body.get("model").and_then(|m| m.as_str()).ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "model_required",
            "model is required",
        )
    })?;
    let prefix = format!("{provider}:");
    let bare_model = raw_model.strip_prefix(&prefix).unwrap_or(raw_model);
    let qualified = format!("{provider}:{bare_model}");
    let entry = catalog::find(&qualified).ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "model_not_priced",
            "model has no catalog price and cannot be served",
        )
    })?;

    // Idempotency is OPT-IN: a client-supplied `Idempotency-Key` dedups billing
    // and replays the cached response on retry. Without it, every request runs
    // and settles under a unique key — identical messages are never collapsed
    // (the old body-digest auto-dedup returned an empty body for legit repeats).
    let action = "agent_chat";
    let client_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty());
    let idempotency_key = match client_key {
        Some(k) => format!("{action}:{user_id}:{k}"),
        None => format!("{action}:{user_id}:{}", uuid::Uuid::new_v4()),
    };
    if client_key.is_some() {
        if let Ok(Some((body, content_type))) =
            metering::cached_response(&state.pool, &idempotency_key).await
        {
            return Ok(replay_response(body, content_type));
        }
    }

    // Balance-enforced hold (transactional; concurrent requests can't overspend).
    let snapshot = billing::account_snapshot(&state.pool, state.wallet.as_ref(), &user_id)
        .await
        .map_err(|e| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "metering_failure",
                &e.to_string(),
            )
        })?;
    let budget = state.wallet.budget_credits(&snapshot);
    let hold = match metering::authorize(
        &state.pool,
        &user_id,
        action,
        HOLD_CAP_CREDITS,
        60,
        budget,
    )
    .await
    {
        Ok(hold) => hold,
        Err(metering::AuthorizeError::InsufficientCredits { .. }) => {
            return Err(error(
                StatusCode::PAYMENT_REQUIRED,
                "insufficient_credits",
                "not enough credits; top up or upgrade to continue",
            ))
        }
        Err(metering::AuthorizeError::Db(e)) => {
            return Err(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "metering_failure",
                &e.to_string(),
            ))
        }
    };

    let ctx = SettleCtx {
        pool: state.pool.clone(),
        hold,
        user_id,
        action,
        idempotency_key,
        entry,
    };
    let streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let (url, key) = upstream_target(&state, &provider, entry)?;
    let body = anonymize(body);
    if streaming {
        stream_upstream(&state, url, key, body, ctx).await
    } else {
        buffered_upstream(&state, url, key, body, ctx).await
    }
}

/// Non-streaming path: buffer the upstream JSON, settle, return it raw with
/// metering headers.
async fn buffered_upstream(
    state: &AppState,
    url: String,
    key: String,
    body: Value,
    ctx: SettleCtx,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let response = send_upstream(state, &url, &key, &body).await?;
    let text = read_capped(response, MAX_UPSTREAM_BYTES).await?;
    let (input, output) = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|j| extract_usage(&j))
        .unwrap_or((0, 0));
    let credits = meter_response(&ctx, input, output, &text, "application/json").await;
    Ok(json_with_meta(text, &ctx, credits))
}

/// Streaming path: forward the upstream SSE bytes to the client as they arrive,
/// while a background task accumulates a copy to parse usage and settle exactly
/// once when the stream ends. Draining continues even if the client disconnects,
/// so settlement is never skipped.
async fn stream_upstream(
    state: &AppState,
    url: String,
    key: String,
    mut body: Value,
    ctx: SettleCtx,
) -> Result<Response, (StatusCode, Json<Value>)> {
    ensure_stream_usage(&mut body);
    let response = send_upstream(state, &url, &key, &body).await?;

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        let mut upstream = response.bytes_stream();
        let mut accumulated = String::new();
        let mut capped = false;
        while let Some(chunk) = upstream.next().await {
            match chunk {
                Ok(bytes) => {
                    if !capped {
                        if accumulated.len() + bytes.len() <= MAX_UPSTREAM_BYTES {
                            accumulated.push_str(&String::from_utf8_lossy(&bytes));
                        } else {
                            capped = true;
                        }
                    }
                    // Ignore a send error (client gone); keep draining upstream
                    // so settlement below still runs.
                    let _ = tx.send(Ok(bytes)).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    break;
                }
            }
        }
        let (input, output) = usage_from_sse(&accumulated);
        meter_response(&ctx, input, output, &accumulated, "text/event-stream").await;
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .map_err(|e| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "stream_setup",
                &e.to_string(),
            )
        })
}

/// Settles usage and caches the response body for opt-in idempotent replay.
/// Returns the credits charged (used for the buffered path's header).
async fn meter_response(
    ctx: &SettleCtx,
    input: u64,
    output: u64,
    body: &str,
    content_type: &str,
) -> u64 {
    let credits = metering::credits_for_tokens(
        input,
        output,
        ctx.entry.input_credits_per_mtok,
        ctx.entry.output_credits_per_mtok,
    );
    if let Err(e) = metering::settle(
        &ctx.pool,
        &ctx.hold,
        &ctx.user_id,
        ctx.action,
        credits,
        &ctx.idempotency_key,
    )
    .await
    {
        tracing::error!("settle failed: {e}");
    }
    if let Err(e) =
        metering::cache_response(&ctx.pool, &ctx.idempotency_key, body, content_type).await
    {
        tracing::warn!("cache_response failed: {e}");
    }
    credits
}

/// Resolves the upstream URL and server-held key for a provider.
fn upstream_target(
    state: &AppState,
    provider: &str,
    _entry: &catalog::ModelEntry,
) -> Result<(String, String), (StatusCode, Json<Value>)> {
    let (url, key) = match provider {
        "anthropic" => (
            "https://api.anthropic.com/v1/chat/completions".to_string(),
            state.config.anthropic_key.clone(),
        ),
        "openai" => (
            "https://api.openai.com/v1/chat/completions".to_string(),
            state.config.openai_key.clone(),
        ),
        "ollama" => (
            format!("{}/v1/chat/completions", state.config.ollama_url),
            Some(String::new()),
        ),
        other => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "unknown_provider",
                &format!("unknown provider {other}"),
            ))
        }
    };
    let key = key.ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_unconfigured",
            &format!("{provider} is not configured on this server"),
        )
    })?;
    Ok((url, key))
}

/// Sends the request upstream and maps a non-2xx into a scrubbed error.
async fn send_upstream(
    state: &AppState,
    url: &str,
    key: &str,
    body: &Value,
) -> Result<reqwest::Response, (StatusCode, Json<Value>)> {
    let mut request = state.http.post(url).json(body);
    if !key.is_empty() {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, "upstream_failure", &e.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        // Never surface a provider key: redact the actual key and any
        // Bearer/sk- token pattern before forwarding the error to the client.
        let safe = scrub_secrets(&text.chars().take(500).collect::<String>(), key);
        return Err(error(
            StatusCode::BAD_GATEWAY,
            "upstream_failure",
            &format!("upstream {status}: {safe}"),
        ));
    }
    Ok(response)
}

/// Reads a response body with a hard byte cap so a hostile/huge upstream can't
/// OOM the proxy.
async fn read_capped(
    response: reqwest::Response,
    max: usize,
) -> Result<String, (StatusCode, Json<Value>)> {
    let mut stream = response.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| error(StatusCode::BAD_GATEWAY, "upstream_failure", &e.to_string()))?;
        if buf.len() + chunk.len() > max {
            return Err(error(
                StatusCode::BAD_GATEWAY,
                "upstream_too_large",
                "upstream response exceeded the size cap",
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf)
        .map_err(|e| error(StatusCode::BAD_GATEWAY, "upstream_failure", &e.to_string()))
}

/// Strips anything that could identify the user before it leaves the proxy.
fn anonymize(mut body: Value) -> Value {
    if let Some(obj) = body.as_object_mut() {
        obj.remove("user");
        obj.remove("metadata");
    }
    body
}

/// Asks an OpenAI-compatible upstream to emit a final usage chunk in the stream
/// so streamed responses can still be metered. Harmless if the provider ignores
/// it.
fn ensure_stream_usage(body: &mut Value) {
    if let Some(obj) = body.as_object_mut() {
        let opts = obj
            .entry("stream_options")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(opts) = opts.as_object_mut() {
            opts.insert("include_usage".to_string(), Value::Bool(true));
        }
    }
}

fn extract_usage(json: &Value) -> Option<(u64, u64)> {
    let input = json.pointer("/usage/prompt_tokens")?.as_u64()?;
    let output = json.pointer("/usage/completion_tokens")?.as_u64()?;
    Some((input, output))
}

/// Parses the final usage numbers out of an accumulated OpenAI SSE stream. The
/// last `data:` chunk carrying a usage object wins.
fn usage_from_sse(text: &str) -> (u64, u64) {
    let mut usage = (0, 0);
    for line in text.lines() {
        let Some(payload) = line.trim_start().strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<Value>(payload) {
            if let Some(u) = extract_usage(&json) {
                usage = u;
            }
        }
    }
    usage
}

fn json_with_meta(body: String, ctx: &SettleCtx, credits: u64) -> Response {
    let mut response = Response::new(Body::from(body));
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    h.insert("x-arya-model", HeaderValue::from_static(ctx.entry.id));
    h.insert(
        "x-arya-privacy-tier",
        HeaderValue::from_static(ctx.entry.privacy_tier),
    );
    if let Ok(v) = HeaderValue::from_str(&credits.to_string()) {
        h.insert("x-arya-credits-charged", v);
    }
    response
}

fn replay_response(body: String, content_type: String) -> Response {
    let ct = HeaderValue::from_str(&content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/json"));
    let mut response = Response::new(Body::from(body));
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, ct);
    h.insert("x-arya-idempotent-replay", HeaderValue::from_static("true"));
    response
}

/// Redacts the live provider key and any Bearer/`sk-` token from text that will
/// be surfaced to the client, so a chatty upstream error can't leak a secret.
fn scrub_secrets(text: &str, key: &str) -> String {
    let mut out = text.to_string();
    if key.len() >= 8 {
        out = out.replace(key, "[redacted]");
    }
    scrub_pattern(&mut out, "Bearer ");
    scrub_pattern(&mut out, "sk-");
    out
}

/// Replaces `prefix` and the run of token characters after it with a redaction.
fn scrub_pattern(text: &mut String, prefix: &str) {
    loop {
        let Some(start) = text.find(prefix) else {
            return;
        };
        let after = start + prefix.len();
        let end = text[after..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
            .map(|i| after + i)
            .unwrap_or(text.len());
        if end > after {
            text.replace_range(start..end, "[redacted]");
        } else {
            return;
        }
    }
}

fn error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "errors": [{ "code": code, "message": message }]
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_parsed_from_sse_stream() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                   data: {\"choices\":[],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7}}\n\n\
                   data: [DONE]\n\n";
        assert_eq!(usage_from_sse(sse), (11, 7));
    }

    #[test]
    fn usage_absent_defaults_to_zero() {
        assert_eq!(
            usage_from_sse("data: {\"choices\":[]}\n\ndata: [DONE]\n"),
            (0, 0)
        );
        assert_eq!(extract_usage(&serde_json::json!({ "choices": [] })), None);
    }

    #[test]
    fn ensure_stream_usage_sets_include_usage() {
        let mut body = serde_json::json!({ "model": "m", "stream": true });
        ensure_stream_usage(&mut body);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn scrub_redacts_the_live_key_and_token_patterns() {
        let key = "sk-ant-secretkey1234567890";
        let text = format!("bad auth for {key} using Bearer {key} (sk-other-abc123)");
        let scrubbed = scrub_secrets(&text, key);
        assert!(!scrubbed.contains("secretkey"), "key leaked: {scrubbed}");
        assert!(
            !scrubbed.contains("sk-other-abc123"),
            "token leaked: {scrubbed}"
        );
        assert!(scrubbed.contains("[redacted]"));
    }

    #[test]
    fn scrub_leaves_ordinary_text_untouched() {
        let msg = "model overloaded, retry later";
        assert_eq!(scrub_secrets(msg, "sk-live-xyz"), msg);
    }

    #[test]
    fn scrub_handles_bare_prefix_without_hanging() {
        let out = scrub_secrets("stray Bearer  and sk-", "");
        assert!(out.contains("Bearer"));
    }
}
