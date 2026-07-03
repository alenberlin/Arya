//! Local embedding client (Ollama).

use serde_json::json;

pub const EMBED_MODEL: &str = "nomic-embed-text";
/// Dimension of nomic-embed-text output; asserted on the first fetch.
#[allow(dead_code)]
pub const EMBED_DIM: usize = 768;

pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
    fn model(&self) -> &str;
}

/// Embeds via Ollama's `/api/embed` endpoint. Free, offline, local.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: EMBED_MODEL.to_string(),
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_millis(800))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

impl Embedder for OllamaEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .client
            .post(format!("{}/api/embed", self.base_url))
            .timeout(std::time::Duration::from_secs(120))
            .json(&json!({ "model": self.model, "input": texts }))
            .send()
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        #[derive(serde::Deserialize)]
        struct EmbedResponse {
            embeddings: Vec<Vec<f32>>,
        }
        let parsed: EmbedResponse = response.json().map_err(|e| e.to_string())?;
        Ok(parsed.embeddings)
    }

    fn model(&self) -> &str {
        &self.model
    }
}
