//! Applying a free-form instruction to text via a local (Ollama) or cloud LLM.
//!
//! The generalization of [`crate::translate`]: the inline `@node + instruction`
//! action (F15) and the "Sort" brain-dump reorganizer (F16) both run through
//! here. **Local Ollama is the default** (private, offline); cloud via the
//! user's own key is opt-in. The system prompt forbids inventing content, so a transform
//! reorganizes/rephrases/translates only what it is given — never fabricates.

use std::time::Duration;

use crate::translate::{TranslateProvider, DEFAULT_LOCAL_MODEL};

/// The guard: apply only the instruction, output only the result, invent nothing.
const SYSTEM_PROMPT: &str = "You transform the user's text according to their \
    instruction. Apply ONLY the instruction. Output ONLY the resulting text — no \
    preamble, no surrounding quotes, no commentary. Never add facts that are not \
    present in the text; you may reorganize, rephrase, summarize, or translate \
    what is given.";

fn user_message(instruction: &str, text: &str) -> String {
    format!("Instruction: {instruction}\n\nText:\n{text}")
}

/// Run the transform on a blocking thread. `Err` carries a user-facing reason;
/// the caller keeps the source text either way, so nothing is ever lost.
fn run(
    provider: TranslateProvider,
    model: Option<String>,
    text: &str,
    instruction: &str,
) -> Result<String, String> {
    let user = user_message(instruction, text);
    let qualified = match provider {
        TranslateProvider::Local => {
            format!(
                "ollama:{}",
                model.unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string())
            )
        }
        TranslateProvider::Cloud => model
            .filter(|m| m.contains(':'))
            .or_else(crate::provider::default_model)
            .ok_or_else(|| "no cloud API key set — add one in Account.".to_string())?,
    };
    crate::provider::chat(
        &qualified,
        &crate::provider::default_ollama_url(),
        SYSTEM_PROMPT,
        &user,
        0.3,
        Duration::from_secs(45),
    )
}

/// Apply an instruction to `source_text` and return the result. Local by default
/// (private); pass `provider: "cloud"` to route to the cloud provider.
#[tauri::command]
pub async fn ai_transform(
    source_text: String,
    instruction: String,
    provider: Option<TranslateProvider>,
    model: Option<String>,
) -> Result<String, String> {
    if source_text.trim().is_empty() {
        return Err("there is nothing to transform".into());
    }
    if instruction.trim().is_empty() {
        return Err("an instruction is required".into());
    }
    let provider = provider.unwrap_or_default();
    tokio::task::spawn_blocking(move || run(provider, model, &source_text, &instruction))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_includes_instruction_and_text() {
        let m = user_message("translate to German", "hello world");
        assert!(m.contains("translate to German"));
        assert!(m.contains("hello world"));
        assert!(m.starts_with("Instruction:"));
    }

    #[test]
    fn local_transform_errors_cleanly_when_ollama_is_down() {
        // Port 1 is never an Ollama server: the call must fail with a clear
        // message (not panic) so the caller keeps the source text.
        let out = crate::provider::chat(
            "ollama:nope",
            "http://127.0.0.1:1",
            SYSTEM_PROMPT,
            "some text",
            0.3,
            Duration::from_secs(45),
        );
        assert!(out.is_err());
    }

    #[test]
    fn system_prompt_forbids_inventing() {
        assert!(SYSTEM_PROMPT.contains("Never add facts"));
    }
}
