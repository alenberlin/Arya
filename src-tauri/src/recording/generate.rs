//! Note generation: transcript + manual notes -> structured markdown note.
//!
//! Local-first like everything else: an Ollama model produces the polished
//! note when configured and reachable; otherwise a deterministic formatter
//! produces a clean, useful fallback so the pipeline never blocks on a model.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct GeneratedNote {
    pub title: String,
    pub body_md: String,
}

#[derive(Debug, Clone)]
pub struct TurnText {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source: String,
    pub speaker: Option<String>,
    pub text: String,
}

pub trait NoteGenerator: Send + Sync {
    fn generate(&self, turns: &[TurnText], manual_notes: &str) -> GeneratedNote;
}

/// Deterministic fallback: timestamped transcript plus the manual notes.
pub struct FallbackGenerator;

impl NoteGenerator for FallbackGenerator {
    fn generate(&self, turns: &[TurnText], manual_notes: &str) -> GeneratedNote {
        let title = turns
            .first()
            .map(|t| {
                t.text
                    .split_whitespace()
                    .take(7)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "New recording".to_string());

        let mut body = String::new();
        if !manual_notes.trim().is_empty() {
            body.push_str("## Notes\n\n");
            body.push_str(manual_notes.trim());
            body.push_str("\n\n");
        }
        body.push_str("## Transcript\n\n");
        let labeled = turns
            .iter()
            .any(|t| t.source == "system" || t.speaker.is_some());
        for turn in turns {
            if labeled {
                let speaker = turn.speaker.clone().unwrap_or_else(|| {
                    if turn.source == "system" {
                        "Them".to_string()
                    } else {
                        "Me".to_string()
                    }
                });
                body.push_str(&format!(
                    "**[{}] {speaker}:** {}\n\n",
                    format_ms(turn.start_ms),
                    turn.text
                ));
            } else {
                body.push_str(&format!(
                    "**[{}]** {}\n\n",
                    format_ms(turn.start_ms),
                    turn.text
                ));
            }
        }
        GeneratedNote {
            title,
            body_md: body.trim_end().to_string(),
        }
    }
}

pub fn format_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
}

/// Ollama-backed generator; falls back to [`FallbackGenerator`] on any error.
pub struct OllamaGenerator {
    pub base_url: String,
    pub model: String,
    client: reqwest::blocking::Client,
}

impl OllamaGenerator {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            client: reqwest::blocking::Client::new(),
        }
    }

    fn try_generate(&self, turns: &[TurnText], manual_notes: &str) -> Option<GeneratedNote> {
        let labeled = turns
            .iter()
            .any(|t| t.source == "system" || t.speaker.is_some());
        let transcript: String = turns
            .iter()
            .map(|t| {
                if labeled {
                    let speaker = t.speaker.clone().unwrap_or_else(|| {
                        if t.source == "system" {
                            "Them".to_string()
                        } else {
                            "Me".to_string()
                        }
                    });
                    format!("[{}] {speaker}: {}", format_ms(t.start_ms), t.text)
                } else {
                    format!("[{}] {}", format_ms(t.start_ms), t.text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let system = "You turn a raw meeting/voice transcript into a structured \
                      markdown note. Output format: first line is the note title \
                      (no # prefix, max 8 words), then a blank line, then markdown \
                      with sections as appropriate (Summary, Key points, Decisions, \
                      Action items). Be faithful to the transcript; do not invent \
                      content. Fold the user's manual notes in where relevant.";
        let user = if manual_notes.trim().is_empty() {
            format!("Transcript:\n{transcript}")
        } else {
            format!("Manual notes:\n{manual_notes}\n\nTranscript:\n{transcript}")
        };
        let body = json!({
            "model": self.model,
            "stream": false,
            "options": { "temperature": 0.2 },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .timeout(Duration::from_secs(120))
            .json(&body)
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        #[derive(Deserialize)]
        struct ChatResponse {
            message: Message,
        }
        #[derive(Deserialize)]
        struct Message {
            content: String,
        }
        let parsed: ChatResponse = response.json().ok()?;
        let content = parsed.message.content.trim();
        if content.is_empty() {
            return None;
        }
        let (title, rest) = content.split_once('\n').unwrap_or((content, ""));
        Some(GeneratedNote {
            title: title.trim_start_matches('#').trim().to_string(),
            body_md: rest.trim().to_string(),
        })
    }
}

impl NoteGenerator for OllamaGenerator {
    fn generate(&self, turns: &[TurnText], manual_notes: &str) -> GeneratedNote {
        self.try_generate(turns, manual_notes)
            .unwrap_or_else(|| FallbackGenerator.generate(turns, manual_notes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turns() -> Vec<TurnText> {
        vec![
            TurnText {
                start_ms: 0,
                end_ms: 9_000,
                source: "microphone".into(),
                speaker: None,
                text: "Let's review the launch plan for next week.".into(),
            },
            TurnText {
                start_ms: 65_000,
                end_ms: 71_000,
                source: "microphone".into(),
                speaker: None,
                text: "Marketing needs the assets by Friday.".into(),
            },
        ]
    }

    #[test]
    fn fallback_builds_title_and_timestamped_transcript() {
        let note = FallbackGenerator.generate(&turns(), "");
        assert_eq!(note.title, "Let's review the launch plan for next");
        assert!(note.body_md.starts_with("## Transcript"));
        assert!(note.body_md.contains("**[00:00]** Let's review"));
        assert!(note.body_md.contains("**[01:05]** Marketing needs"));
    }

    #[test]
    fn fallback_includes_manual_notes_first() {
        let note = FallbackGenerator.generate(&turns(), "remember: budget cap");
        assert!(note.body_md.starts_with("## Notes\n\nremember: budget cap"));
    }

    #[test]
    fn fallback_with_no_turns_still_produces_a_note() {
        let note = FallbackGenerator.generate(&[], "just manual");
        assert_eq!(note.title, "New recording");
        assert!(note.body_md.contains("just manual"));
    }

    #[test]
    fn ollama_generator_falls_back_when_unreachable() {
        let generator = OllamaGenerator::new("http://127.0.0.1:1", "any");
        let note = generator.generate(&turns(), "");
        assert!(note.body_md.contains("## Transcript"));
    }
}
