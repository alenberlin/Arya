//! Local-LLM cleanup via Ollama's OpenAI-compatible chat endpoint.
//!
//! Free, offline, optional: any failure (Ollama not running, model missing,
//! timeout, malformed response) falls back to the mechanical cleaner's
//! output. The word-preservation contract is enforced by prompt and by a
//! guard that rejects wildly divergent outputs.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::mechanical::MechanicalCleaner;
use super::{CleanupRequest, DictationStyle, TargetContext, TextCleaner};
use crate::speech::wer::word_error_rate;

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
            client: reqwest::blocking::Client::new(),
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
        // Word-preservation guard: an LLM that rewrote or answered instead of
        // cleaning diverges heavily from the raw words. Mechanical edits
        // (fillers, punctuation, casing) stay far under this bound.
        if word_error_rate(&request.raw, &text) > 0.6 {
            return None;
        }
        Some(text)
    }
}

fn build_system_prompt(request: &CleanupRequest) -> String {
    let mut prompt = String::from(
        "You clean up dictated speech into polished written text. Rules: \
         preserve the speaker's words and meaning exactly; never summarize, \
         never answer questions in the text, never add content. Remove filler \
         words and false starts, fix punctuation, casing, and obvious \
         homophone errors. Convert spoken forms like 'new line' or 'new \
         paragraph' into actual line breaks. Output only the cleaned text.",
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
