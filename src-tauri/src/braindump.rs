//! Brain-dump → coherent notes.
//!
//! Takes a jumble of unrelated or loosely-related ideas — typed, pasted, dropped
//! in as text files, or spoken — and asks the local model to split it into
//! separate, single-topic notes, preserving every idea and inventing nothing.
//! Like AI folder-sort, this is **suggest-then-confirm**: [`split_braindump_into_notes`]
//! only proposes; [`create_notes_from_split`] writes the notes the user accepts.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

use crate::translate::DEFAULT_LOCAL_MODEL;

/// Minimum input length worth splitting; below this there's no dump to organize.
const MIN_CHARS: usize = 20;
/// Context window for the split. A big brain dump can run to many thousands of
/// tokens; the default is small enough that llama-server would drop the middle
/// of the prompt (and the instruction with it), so it's set generously.
const SPLIT_NUM_CTX: u32 = 32768;
/// A large dump takes longer than a one-line rewrite, so the split gets a roomy
/// timeout rather than the 45s the inline transforms use.
const SPLIT_TIMEOUT: Duration = Duration::from_secs(300);

const SYSTEM_PROMPT: &str = "You reorganize a brain dump into separate, coherent notes. \
    The user's text is a jumble of unrelated or loosely related ideas. Group the content \
    by topic and produce ONE note per distinct topic. For each note write a short, \
    descriptive title and a clean body in Markdown that preserves ALL of the user's ideas \
    for that topic — reorganized and lightly cleaned, but never inventing new facts and \
    never dropping content. Every part of the input must land in exactly one note. Respond \
    with ONLY a JSON array shaped exactly like [{\"title\": \"...\", \"body\": \"...\"}]. \
    Output no prose, no explanation, and no code fences.";

/// One proposed note from the split. Also the shape the UI sends back to create.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedNote {
    pub title: String,
    pub body: String,
}

/// Read and concatenate the text of several files (UTF-8, lossy). Used when the
/// user adds text files to a brain dump. Reading happens off the async runtime.
#[tauri::command]
pub async fn read_text_files(paths: Vec<String>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut out = String::new();
        for path in &paths {
            let bytes = std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))?;
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&String::from_utf8_lossy(&bytes));
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Split a brain dump into proposed single-topic notes using the local model.
/// Suggestion only — writes nothing.
#[tauri::command]
pub async fn split_braindump_into_notes(
    source_text: String,
    model: Option<String>,
) -> Result<Vec<ProposedNote>, String> {
    let text = source_text.trim().to_string();
    if text.chars().count() < MIN_CHARS {
        return Err("there isn't enough text to organize into notes".into());
    }
    tokio::task::spawn_blocking(move || run_split(&text, model))
        .await
        .map_err(|e| e.to_string())?
}

fn run_split(text: &str, model: Option<String>) -> Result<Vec<ProposedNote>, String> {
    let model = model.unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string());
    // Constrain the decoder to a valid array of {title, body}. Local models
    // reliably produce *almost*-valid JSON on longer outputs (trailing commas,
    // unquoted keys, unescaped newlines); the schema makes the output always
    // parseable — the fix for "couldn't understand the model's response" on big
    // dumps — and is typically faster than free-form generation.
    let schema = serde_json::json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "body": { "type": "string" }
            },
            "required": ["title", "body"]
        }
    });
    let reply = crate::http::ollama_chat_ex(
        &crate::http::blocking_client(),
        &crate::transform::ollama_url(),
        &model,
        SYSTEM_PROMPT,
        &format!("Brain dump:\n\n{text}"),
        0.3,
        SPLIT_TIMEOUT,
        Some(SPLIT_NUM_CTX),
        Some(schema),
    )
    .ok_or("the local model didn't respond — is Ollama running?")?;

    let notes = parse_notes(&reply).ok_or("couldn't understand the model's response")?;
    // Drop empty-bodied entries; backfill a title from the body when the model
    // left one blank, so nothing lands as an untitled scrap.
    let notes: Vec<ProposedNote> = notes
        .into_iter()
        .filter(|n| !n.body.trim().is_empty())
        .map(|n| ProposedNote {
            title: if n.title.trim().is_empty() {
                crate::notes::title_from_text(&n.body)
            } else {
                n.title.trim().to_string()
            },
            body: n.body,
        })
        .collect();
    if notes.is_empty() {
        return Err("the model didn't produce any notes to create".into());
    }
    Ok(notes)
}

/// Pull the JSON array out of a model reply, tolerating stray preamble or code
/// fences by slicing from the first `[` to the last `]` (same as AI folder-sort).
fn parse_notes(raw: &str) -> Option<Vec<ProposedNote>> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    if end < start {
        return None;
    }
    serde_json::from_str::<Vec<ProposedNote>>(&raw[start..=end]).ok()
}

/// Create the accepted notes in one transaction, optionally all in `folder_id`.
/// Each note carries its markdown body (the block editor renders it via the
/// legacy `body_md` fallback, exactly like a dictation converted to a note).
/// Returns the new note ids.
#[tauri::command]
pub async fn create_notes_from_split(
    pool: State<'_, SqlitePool>,
    notes: Vec<ProposedNote>,
    folder_id: Option<String>,
) -> Result<Vec<String>, String> {
    create_notes_inner(&pool, notes, folder_id).await
}

/// Transactional core of [`create_notes_from_split`], callable without the Tauri
/// `State` wrapper (so it's unit-testable). All-or-nothing.
async fn create_notes_inner(
    pool: &SqlitePool,
    notes: Vec<ProposedNote>,
    folder_id: Option<String>,
) -> Result<Vec<String>, String> {
    let notes: Vec<ProposedNote> = notes
        .into_iter()
        .filter(|n| !n.body.trim().is_empty())
        .collect();
    if notes.is_empty() {
        return Err("there are no notes to create".into());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let mut ids = Vec::with_capacity(notes.len());
    for note in &notes {
        let id = uuid::Uuid::new_v4().to_string();
        let title = if note.title.trim().is_empty() {
            crate::notes::title_from_text(&note.body)
        } else {
            note.title.trim().to_string()
        };
        sqlx::query(
            "INSERT INTO notes (id, title, body_md, folder_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(&id)
        .bind(&title)
        .bind(&note.body)
        .bind(&folder_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        ids.push(id);
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn parse_notes_reads_clean_wrapped_and_noisy_json() {
        assert!(parse_notes(r#"[{"title":"A","body":"x"}]"#).is_some());
        assert!(
            parse_notes("```json\n[{\"title\":\"A\",\"body\":\"x\"}]\n```").is_some(),
            "code fences tolerated"
        );
        assert!(
            parse_notes("Sure!\n[{\"title\":\"A\",\"body\":\"x\"}]\ndone").is_some(),
            "preamble/suffix tolerated"
        );
        assert!(parse_notes("not json").is_none());
        assert!(parse_notes("]before[").is_none());
    }

    #[tokio::test]
    async fn create_notes_from_split_writes_notes_with_bodies_and_folder() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO folders (id, name, created_at) \
             VALUES ('f1', 'Ideas', strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notes = vec![
            ProposedNote {
                title: "Trip planning".into(),
                body: "Book flights to Lisbon.".into(),
            },
            ProposedNote {
                // Blank title → derived from the body.
                title: "  ".into(),
                body: "Refactor the auth module.".into(),
            },
            ProposedNote {
                title: "Empty".into(),
                body: "   ".into(), // dropped
            },
        ];

        let ids = create_notes_inner(&pool, notes, Some("f1".into()))
            .await
            .unwrap();
        assert_eq!(ids.len(), 2, "the empty-body note is skipped");

        let rows = crate::notes::fetch_notes(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert_eq!(r.folder_id.as_deref(), Some("f1"));
        }
        // The blank-title note took its title from the body.
        let derived = crate::notes::fetch_note_detail(&pool, &ids[1])
            .await
            .unwrap();
        assert_eq!(derived.title, "Refactor the auth module.");
        assert_eq!(derived.body_md, "Refactor the auth module.");
    }

    /// End-to-end split against the real local model (SuperGemma4): a dump of
    /// three clearly-unrelated topics must come back as multiple non-empty notes.
    /// Run with: `cargo test -p arya --lib -- --ignored real_split`.
    #[ignore = "requires a local Ollama with the chat model"]
    #[test]
    fn real_split_produces_multiple_topic_notes() {
        let dump = "Buy milk, eggs, and coffee for the week. \
             Separately, I need to fix the login bug in the auth service before Friday's release. \
             Oh and my sister's birthday is next month — find her a good gift, maybe a book.";
        let notes = run_split(dump, None).expect("split via local model");
        assert!(
            notes.len() >= 2,
            "expected the dump split into multiple notes, got {}",
            notes.len()
        );
        assert!(
            notes.iter().all(|n| !n.body.trim().is_empty()),
            "every proposed note should have a body"
        );
    }
}
