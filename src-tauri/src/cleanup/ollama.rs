//! Local-LLM cleanup via Ollama's OpenAI-compatible chat endpoint.
//!
//! Free, offline, optional: any failure (Ollama not running, model missing,
//! timeout, malformed response) falls back to the mechanical cleaner's output.
//! This is the "Polished" backend: it rewrites for grammatical correctness in
//! the speaker's language (never answering, summarizing, or adding content),
//! with a length backstop that rejects a runaway model.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::mechanical::MechanicalCleaner;
use super::{CleanupRequest, DictationStyle, TargetContext, TextCleaner};

pub struct OllamaCleaner {
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
    client: reqwest::blocking::Client,
}

impl OllamaCleaner {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            timeout,
            client: crate::http::blocking_client(),
        }
    }

    /// True when the Ollama server answers at all.
    pub fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(Duration::from_millis(800))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    fn try_clean(&self, request: &CleanupRequest) -> Option<String> {
        let system = build_system_prompt(request);
        let body = json!({
            "model": self.model,
            "stream": false,
            "options": { "temperature": 0.1 },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": request.raw },
            ],
        });
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .timeout(self.timeout)
            .json(&body)
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        let parsed: ChatResponse = response.json().ok()?;
        let text = parsed.message.content.trim().to_string();
        if text.is_empty() {
            return None;
        }
        // "Polished" deliberately rewrites for grammar, so word-level divergence
        // is expected and no longer disqualifying. The remaining backstop is
        // length: a grammatical rewrite stays roughly input-sized, so an output
        // several times longer means the model answered or rambled instead of
        // rewriting — fall back to the mechanical cleaner rather than paste that.
        let raw_words = request.raw.split_whitespace().count().max(1);
        if text.split_whitespace().count() > raw_words * 3 + 12 {
            return None;
        }
        Some(text)
    }
}

fn build_system_prompt(request: &CleanupRequest) -> String {
    let mut prompt = String::from(
        "You rewrite dictated speech into clear, grammatically correct writing in \
         the SAME language the speaker used. Fix grammar, verb tense, \
         subject-verb agreement, word order, and awkward or non-native phrasing \
         so the result reads as natural, correct writing in that language — even \
         when the spoken input was rough or a little off. Preserve the speaker's \
         meaning and intent; do NOT answer questions, do NOT summarize, do NOT \
         add new information, opinions, or content of your own. Remove filler \
         words and false starts, and fix homophones. Convert spoken forms like \
         'new line' or 'new paragraph' into actual line breaks. Output only the \
         rewritten text, nothing else.",
    );
    match request.style {
        DictationStyle::Standard => {}
        DictationStyle::CasualLowercase => {
            prompt.push_str(" Style: casual, all lowercase, minimal punctuation.");
        }
        DictationStyle::Formal => {
            prompt.push_str(" Style: formal; expand contractions; complete sentences.");
        }
    }
    if request.context == TargetContext::Email {
        prompt.push_str(
            " The text is being dictated into an email: if it contains a \
             greeting or sign-off, place them on their own lines with a blank \
             line between paragraphs.",
        );
    }
    if !request.dictionary.is_empty() {
        prompt.push_str(" Vocabulary (always spell these exactly as given):");
        for entry in &request.dictionary {
            prompt.push_str(&format!(" {};", entry.replacement));
        }
    }
    prompt
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

impl TextCleaner for OllamaCleaner {
    fn clean(&self, request: &CleanupRequest) -> String {
        match self.try_clean(request) {
            Some(text) => text,
            None => MechanicalCleaner.clean(request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_mentions_email_layout_only_in_email_context() {
        let base = CleanupRequest {
            raw: "hi".into(),
            style: DictationStyle::Standard,
            context: TargetContext::Generic,
            dictionary: vec![],
        };
        assert!(!build_system_prompt(&base).contains("email"));
        let email = CleanupRequest {
            context: TargetContext::Email,
            ..base
        };
        assert!(build_system_prompt(&email).contains("email"));
    }

    #[test]
    fn polished_prompt_asks_for_a_grammatical_rewrite_in_the_input_language() {
        let req = CleanupRequest {
            raw: "x".into(),
            style: DictationStyle::Standard,
            context: TargetContext::Generic,
            dictionary: vec![],
        };
        let p = build_system_prompt(&req).to_lowercase();
        assert!(
            p.contains("grammatic"),
            "should ask for grammatical correctness"
        );
        assert!(
            p.contains("same language"),
            "should keep the speaker's language"
        );
        assert!(
            p.contains("not answer"),
            "must still refuse to answer questions"
        );
    }

    #[test]
    fn unavailable_server_falls_back_to_mechanical() {
        // Port 1 is never an Ollama server.
        let cleaner = OllamaCleaner::new(
            "http://127.0.0.1:1",
            "any-model",
            Duration::from_millis(200),
        );
        let request = CleanupRequest {
            raw: "um send it tomorrow".into(),
            style: DictationStyle::Standard,
            context: TargetContext::Generic,
            dictionary: vec![],
        };
        assert_eq!(cleaner.clean(&request), "Send it tomorrow.");
    }
}
