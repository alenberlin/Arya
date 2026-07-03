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

use crate::{billing, catalog, metering, AppState};

/// GET /v1/account — the snapshot the desktop shows (tier, credits, usage).
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

    // Duplicate/retry guard: if this exact request already settled, return the
    // receipt WITHOUT calling the provider again (a retry must never bill the
    // upstream twice).
    if let Ok(Some(credits)) = metering::existing_charge(&state.pool, &idempotency_key).await {
        return Ok(Json(serde_json::json!({
            "success": true,
            "data": serde_json::Value::Null,
            "meta": {
                "creditsCharged": credits,
                "idempotentReplay": true,
                "model": entry.id,
                "privacyTier": entry.privacy_tier,
                "note": "duplicate request; upstream not re-invoked",
            }
        })));
    }

    // Balance-enforced hold: authorize() checks the wallet's budget against
    // settled charges + open holds transactionally, so concurrent requests
    // can't collectively overspend. A `None` budget means the wallet does not
    // meter (offline dev with no cloud keys).
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
    let hold = match metering::authorize(&state.pool, &user_id, action, 500, 60, budget).await {
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
                       // Never surface a provider key: redact the actual key and any
                       // Bearer/sk- token pattern before forwarding the error to the client.
        let safe = scrub_secrets(&text.chars().take(500).collect::<String>(), &key);
        return Err(error(
            StatusCode::BAD_GATEWAY,
            "upstream_failure",
            &format!("upstream {status}: {safe}"),
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

/// Redacts the live provider key and any Bearer/`sk-` token from text that
/// will be surfaced to the client, so a chatty upstream error can't leak a
/// secret. Defense in depth: providers don't normally echo the key, but the
/// proxy must guarantee it regardless.
fn scrub_secrets(text: &str, key: &str) -> String {
    let mut out = text.to_string();
    if key.len() >= 8 {
        out = out.replace(key, "[redacted]");
    }
    // Redact any remaining Bearer token and OpenAI/Anthropic-style key.
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
        // Only redact if there's an actual token body after the prefix.
        if end > after {
            text.replace_range(start..end, "[redacted]");
        } else {
            // Avoid an infinite loop on a bare prefix with no token.
            return;
        }
    }
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
        // A prefix with no token body must not loop forever.
        let out = scrub_secrets("stray Bearer  and sk-", "");
        assert!(out.contains("Bearer"));
    }
}
