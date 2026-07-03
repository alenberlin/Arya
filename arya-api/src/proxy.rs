//! Metered chat-completions proxy.
//!
//! POST /v1/{provider}/chat/completions with an OpenAI-shaped body. Flow:
//! verify identity -> resolve+price the model (reject if unpriced) ->
//! authorize a hold -> call upstream with the server-held key -> settle
//! actual token usage exactly once. The upstream never sees the user id or
//! any Arya identifier (structural anonymization).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{catalog, metering, AppState};

pub async fn forward(
    State(state): State<AppState>,
    Path((provider, path)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

    // Model id the desktop uses is provider-qualified; the body carries the
    // bare model name for the upstream.
    let bare_model = body.get("model").and_then(|m| m.as_str()).ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "model_required",
            "model is required",
        )
    })?;
    let qualified = format!("{provider}:{bare_model}");
    let entry = catalog::find(&qualified).ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "model_not_priced",
            "model has no catalog price and cannot be served",
        )
    })?;

    // Deterministic idempotency key: same user + action + body digest.
    let action = "agent_chat";
    let digest = body_digest(&body);
    let idempotency_key = format!("{action}:{user_id}:{digest}");

    let hold = metering::authorize(&state.pool, &user_id, action, 500, 60)
        .await
        .map_err(|e| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "metering_failure",
                &e.to_string(),
            )
        })?;

    // Call upstream with the server-held key. Body forwarded as-is minus any
    // client-supplied identity fields (structural anonymization).
    let (usage, response_body) = call_upstream(&state, &provider, entry, body).await?;

    let credits = metering::credits_for_tokens(
        usage.0,
        usage.1,
        entry.input_credits_per_mtok,
        entry.output_credits_per_mtok,
    );
    let receipt = metering::settle(
        &state.pool,
        &hold,
        &user_id,
        action,
        credits,
        &idempotency_key,
    )
    .await
    .map_err(|e| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "metering_failure",
            &e.to_string(),
        )
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "data": response_body,
        "meta": {
            "creditsCharged": receipt.credits,
            "idempotentReplay": receipt.replay,
            "model": entry.id,
            "privacyTier": entry.privacy_tier,
        }
    })))
}

/// (input_tokens, output_tokens), response JSON.
type Usage = (u64, u64);

async fn call_upstream(
    state: &AppState,
    provider: &str,
    entry: &catalog::ModelEntry,
    mut body: Value,
) -> Result<(Usage, Value), (StatusCode, Json<Value>)> {
    // Strip anything that could identify the user before it leaves the TEE.
    if let Some(obj) = body.as_object_mut() {
        obj.remove("user");
        obj.remove("metadata");
    }

    let (url, key_header, key) = match provider {
        "anthropic" => (
            "https://api.anthropic.com/v1/chat/completions".to_string(),
            "authorization",
            state.config.anthropic_key.clone(),
        ),
        "openai" => (
            "https://api.openai.com/v1/chat/completions".to_string(),
            "authorization",
            state.config.openai_key.clone(),
        ),
        "ollama" => (
            format!("{}/v1/chat/completions", state.config.ollama_url),
            "authorization",
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

    let mut request = state.http.post(&url).json(&body);
    if !key.is_empty() {
        request = request.header(key_header, format!("Bearer {key}"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, "upstream_failure", &e.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let _ = entry; // entry used above for pricing
        return Err(error(
            StatusCode::BAD_GATEWAY,
            "upstream_failure",
            &format!(
                "upstream {status}: {}",
                text.chars().take(500).collect::<String>()
            ),
        ));
    }
    let json: Value = response
        .json()
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, "upstream_failure", &e.to_string()))?;

    let input = json
        .pointer("/usage/prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = json
        .pointer("/usage/completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(((input, output), json))
}

fn body_digest(body: &Value) -> String {
    let canonical = serde_json::to_string(body).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
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
    fn body_digest_is_stable_and_sensitive() {
        let a =
            serde_json::json!({ "model": "m", "messages": [{ "role": "user", "content": "hi" }] });
        let b = a.clone();
        let c =
            serde_json::json!({ "model": "m", "messages": [{ "role": "user", "content": "yo" }] });
        assert_eq!(body_digest(&a), body_digest(&b));
        assert_ne!(body_digest(&a), body_digest(&c));
    }
}
