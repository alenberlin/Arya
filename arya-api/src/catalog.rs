//! Model catalog: every servable model with credit pricing, privacy tier,
//! and capability flags. A model with no catalog entry is rejected before
//! any wallet or provider work ("no default rate").
//!
//! Credits: $1 = 1000 credits. Prices are credits per million tokens.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub provider: &'static str,
    /// "local" (never leaves the machine), "private" (zero-retention
    /// agreement), "standard" (provider's own policy).
    pub privacy_tier: &'static str,
    pub input_credits_per_mtok: u64,
    pub output_credits_per_mtok: u64,
    pub supports_tools: bool,
    pub supports_vision: bool,
}

/// The static registry. Extended, never mutated at runtime; the desktop can
/// always rely on an id keeping its meaning.
pub const CATALOG: &[ModelEntry] = &[
    ModelEntry {
        id: "anthropic:claude-sonnet-5",
        display_name: "Claude Sonnet 5",
        provider: "anthropic",
        privacy_tier: "standard",
        input_credits_per_mtok: 3_000,
        output_credits_per_mtok: 15_000,
        supports_tools: true,
        supports_vision: true,
    },
    ModelEntry {
        id: "anthropic:claude-opus-4-8",
        display_name: "Claude Opus 4.8",
        provider: "anthropic",
        privacy_tier: "standard",
        input_credits_per_mtok: 15_000,
        output_credits_per_mtok: 75_000,
        supports_tools: true,
        supports_vision: true,
    },
    ModelEntry {
        id: "openai:gpt-5.2",
        display_name: "GPT-5.2",
        provider: "openai",
        privacy_tier: "standard",
        input_credits_per_mtok: 2_500,
        output_credits_per_mtok: 10_000,
        supports_tools: true,
        supports_vision: true,
    },
    ModelEntry {
        id: "openai:gpt-5-mini",
        display_name: "GPT-5 mini",
        provider: "openai",
        privacy_tier: "standard",
        input_credits_per_mtok: 300,
        output_credits_per_mtok: 1_200,
        supports_tools: true,
        supports_vision: true,
    },
    // Dev/free upstream: exercises the full metering pipeline at a symbolic
    // price so exactly-once settlement is tested with real numbers.
    ModelEntry {
        id: "ollama:*",
        display_name: "Local model (via server Ollama)",
        provider: "ollama",
        privacy_tier: "local",
        input_credits_per_mtok: 1,
        output_credits_per_mtok: 1,
        supports_tools: true,
        supports_vision: false,
    },
];

/// Finds the catalog entry for a model id ("provider:model"). Ollama models
/// match the wildcard entry.
pub fn find(model_id: &str) -> Option<&'static ModelEntry> {
    if let Some(exact) = CATALOG.iter().find(|m| m.id == model_id) {
        return Some(exact);
    }
    if model_id.starts_with("ollama:") {
        return CATALOG.iter().find(|m| m.id == "ollama:*");
    }
    None
}

pub async fn list_models(State(state): State<AppState>) -> Json<serde_json::Value> {
    // Only list providers the server can actually reach.
    let models: Vec<&ModelEntry> = CATALOG
        .iter()
        .filter(|m| match m.provider {
            "anthropic" => state.config.anthropic_key.is_some(),
            "openai" => state.config.openai_key.is_some(),
            "ollama" => true,
            _ => false,
        })
        .collect();
    Json(serde_json::json!({ "models": models }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ids_resolve_and_unknown_rejects() {
        assert!(find("anthropic:claude-sonnet-5").is_some());
        assert!(find("openai:gpt-5-mini").is_some());
        assert!(find("mystery:model").is_none());
    }

    #[test]
    fn ollama_wildcard_matches_any_local_model() {
        let entry = find("ollama:qwen3.6:35b").unwrap();
        assert_eq!(entry.provider, "ollama");
        assert_eq!(entry.privacy_tier, "local");
    }

    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<_> = CATALOG.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
    }
}
