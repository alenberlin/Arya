//! AI-assisted sorting of notes into existing folders.
//!
//! Distinct from [`crate::transform`] (free-form text rewriting): this reads a
//! batch of notes and the current set of folders and returns, per note, the one
//! existing folder it best fits — or nothing when it doesn't clearly fit any.
//! Classification only: it never creates folders or edits a note. The UI shows
//! the result as a suggestion the user confirms before anything actually moves,
//! so a wrong guess costs a rejected row, never a silent misfiling.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tauri::State;

use crate::translate::DEFAULT_LOCAL_MODEL;

/// How many notes go in one model call. Small enough that the returned JSON
/// stays short and quick to generate; large enough that a few hundred notes
/// need only a handful of calls.
const BATCH_SIZE: usize = 15;
/// Per-note body characters shown to the model. A topic is clear from the title
/// plus a snippet, and short prompts keep each call fast.
const EXCERPT_CHARS: usize = 240;
/// A batch is bigger than a one-line transform but still short output; 60s is
/// comfortable headroom even on a cold model load.
const BATCH_TIMEOUT: Duration = Duration::from_secs(60);

const SYSTEM_PROMPT: &str = "You are a note classifier. You are given a list of \
    existing folders and a batch of notes. For each note, choose the single \
    existing folder whose topic best fits the note, or \"none\" if the note does \
    not clearly belong to any of them. Use only the folder names provided — never \
    invent a folder. Respond with ONLY a JSON array, one object per note, shaped \
    exactly like [{\"n\": 1, \"folder\": \"<exact folder name, or none>\"}]. Output \
    no prose, no explanation, and no code fences.";

/// One proposed move: a note and the existing folder the model chose for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSuggestion {
    pub note_id: String,
    pub folder_id: String,
}

#[derive(sqlx::FromRow)]
struct NoteRow {
    id: String,
    title: String,
    body_md: String,
}

#[derive(sqlx::FromRow)]
struct FolderRow {
    id: String,
    name: String,
}

/// One entry in the model's JSON reply: `n` is the 1-based index within the
/// batch, `folder` the chosen folder name (or "none").
#[derive(Deserialize)]
struct ClassifyItem {
    n: usize,
    folder: String,
}

/// The placeholder title a brand-new, untyped note carries (see
/// `createNote("New note")`). Combined with an empty body it marks a note with
/// nothing to classify.
const PLACEHOLDER_TITLE: &str = "New note";

/// Whether a note has enough signal to classify. An empty note still on its
/// placeholder title has no topic — asking the model about it just files blank
/// scraps into folders (the model guesses rather than answering "none"), so
/// those are dropped before they ever reach a batch.
fn has_classifiable_content(title: &str, body: &str) -> bool {
    if !body.trim().is_empty() {
        return true;
    }
    let t = title.trim();
    !t.is_empty() && !t.eq_ignore_ascii_case(PLACEHOLDER_TITLE)
}

/// A single-line snippet of a note body: whitespace collapsed, capped so the
/// prompt stays short. Character-based (not byte) so it never splits a UTF-8
/// codepoint.
fn excerpt(body: &str, max: usize) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let head: String = collapsed.chars().take(max).collect();
        format!("{head}…")
    }
}

fn build_user_prompt(folders: &[FolderRow], batch: &[NoteRow]) -> String {
    let mut s = String::from("Folders:\n");
    for f in folders {
        s.push_str("- ");
        s.push_str(&f.name);
        s.push('\n');
    }
    s.push_str("\nNotes:\n");
    for (i, note) in batch.iter().enumerate() {
        s.push_str(&format!(
            "{}. Title: {} | Body: {}\n",
            i + 1,
            note.title,
            excerpt(&note.body_md, EXCERPT_CHARS)
        ));
    }
    s
}

/// Pull the JSON array out of a model reply, tolerating stray preamble or code
/// fences by slicing from the first `[` to the last `]`.
fn parse_items(raw: &str) -> Option<Vec<ClassifyItem>> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    if end < start {
        return None;
    }
    serde_json::from_str::<Vec<ClassifyItem>>(&raw[start..=end]).ok()
}

/// Turn the model's parsed answer into concrete suggestions: keep only in-range
/// indices whose folder name matches an existing folder (case-insensitively);
/// drop "none", blanks, and hallucinated folder names.
fn to_suggestions(
    items: &[ClassifyItem],
    batch_ids: &[String],
    by_name: &HashMap<String, String>,
) -> Vec<FolderSuggestion> {
    let mut out = Vec::new();
    for item in items {
        if item.n == 0 || item.n > batch_ids.len() {
            continue;
        }
        let folder = item.folder.trim();
        if folder.is_empty() || folder.eq_ignore_ascii_case("none") {
            continue;
        }
        if let Some(folder_id) = by_name.get(&folder.to_lowercase()) {
            out.push(FolderSuggestion {
                note_id: batch_ids[item.n - 1].clone(),
                folder_id: folder_id.clone(),
            });
        }
    }
    out
}

/// Blocking: classify every note in `notes` against `folders`, one batch per
/// model call. A batch that fails (model down, timeout, unparseable reply) is
/// skipped rather than failing the whole run — a partial suggestion set is more
/// useful than none.
fn run_classify(
    base_url: &str,
    model: &str,
    folders: &[FolderRow],
    notes: &[NoteRow],
) -> Vec<FolderSuggestion> {
    let client = crate::http::blocking_client();
    let by_name: HashMap<String, String> = folders
        .iter()
        .map(|f| (f.name.to_lowercase(), f.id.clone()))
        .collect();

    let mut out = Vec::new();
    for batch in notes.chunks(BATCH_SIZE) {
        let user = build_user_prompt(folders, batch);
        let Some(reply) = crate::http::ollama_chat(
            &client,
            base_url,
            model,
            SYSTEM_PROMPT,
            &user,
            0.1,
            BATCH_TIMEOUT,
        ) else {
            continue;
        };
        let Some(items) = parse_items(&reply) else {
            continue;
        };
        let batch_ids: Vec<String> = batch.iter().map(|n| n.id.clone()).collect();
        out.extend(to_suggestions(&items, &batch_ids, &by_name));
    }
    out
}

/// Suggest a folder for each of `note_ids` from the existing folder set, using
/// the local model. Returns only confident matches; the caller (the review
/// modal) decides what to actually apply. Never mutates anything.
#[tauri::command]
pub async fn classify_notes_into_folders(
    pool: State<'_, SqlitePool>,
    note_ids: Vec<String>,
    model: Option<String>,
) -> Result<Vec<FolderSuggestion>, String> {
    if note_ids.is_empty() {
        return Ok(Vec::new());
    }
    let folders: Vec<FolderRow> =
        sqlx::query_as::<_, FolderRow>("SELECT id, name FROM folders ORDER BY name")
            .fetch_all(&*pool)
            .await
            .map_err(|e| e.to_string())?;
    if folders.is_empty() {
        return Ok(Vec::new());
    }

    // SQLite has no array binding, so build an IN clause with one placeholder
    // per requested id.
    let placeholders = std::iter::repeat_n("?", note_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT id, title, body_md FROM notes WHERE id IN ({placeholders})");
    let mut query = sqlx::query_as::<_, NoteRow>(&sql);
    for id in &note_ids {
        query = query.bind(id);
    }
    // Empty placeholder notes have no topic — drop them before classifying so
    // Auto-sort never proposes filing blank scraps into folders.
    let notes: Vec<NoteRow> = query
        .fetch_all(&*pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|n| has_classifiable_content(&n.title, &n.body_md))
        .collect();
    if notes.is_empty() {
        return Ok(Vec::new());
    }

    let model = model.unwrap_or_else(|| DEFAULT_LOCAL_MODEL.to_string());
    let base_url = crate::transform::ollama_url();
    tokio::task::spawn_blocking(move || run_classify(&base_url, &model, &folders, &notes))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_notes_with_real_signal_are_classifiable() {
        // Empty placeholder note → skipped.
        assert!(!has_classifiable_content("New note", ""));
        assert!(!has_classifiable_content("  new note ", "   "));
        assert!(!has_classifiable_content("", ""));
        // Real body, or a meaningful title even with no body → classifiable.
        assert!(has_classifiable_content("New note", "some actual content"));
        assert!(has_classifiable_content("Dentist appointment", ""));
    }

    #[test]
    fn excerpt_collapses_whitespace_and_caps_length() {
        assert_eq!(excerpt("  a\n\n  b   c ", 240), "a b c");
        let long = "word ".repeat(200);
        let out = excerpt(&long, 20);
        assert_eq!(out.chars().count(), 21); // 20 chars + the ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn parse_items_reads_clean_wrapped_and_noisy_json() {
        assert!(parse_items(r#"[{"n":1,"folder":"Health"}]"#).is_some());
        assert!(parse_items("```json\n[{\"n\":1,\"folder\":\"Health\"}]\n```").is_some());
        assert!(
            parse_items("Sure! Here you go:\n[{\"n\":1,\"folder\":\"Health\"}] done").is_some()
        );
        assert!(parse_items("not json at all").is_none());
        assert!(parse_items("]before[").is_none());
    }

    #[test]
    fn to_suggestions_keeps_matches_and_drops_none_unknown_and_out_of_range() {
        let by_name: HashMap<String, String> = [
            ("health".into(), "f-health".into()),
            ("work".into(), "f-work".into()),
        ]
        .into_iter()
        .collect();
        let batch_ids = vec!["n1".to_string(), "n2".to_string(), "n3".to_string()];
        let items = vec![
            ClassifyItem {
                n: 1,
                folder: "Health".into(),
            }, // exact
            ClassifyItem {
                n: 2,
                folder: "  WORK ".into(),
            }, // case + whitespace
            ClassifyItem {
                n: 3,
                folder: "none".into(),
            }, // opt-out → dropped
            ClassifyItem {
                n: 3,
                folder: "Fitness".into(),
            }, // hallucinated → dropped
            ClassifyItem {
                n: 9,
                folder: "Health".into(),
            }, // out of range → dropped
        ];
        let out = to_suggestions(&items, &batch_ids, &by_name);
        assert_eq!(
            out,
            vec![
                FolderSuggestion {
                    note_id: "n1".into(),
                    folder_id: "f-health".into()
                },
                FolderSuggestion {
                    note_id: "n2".into(),
                    folder_id: "f-work".into()
                },
            ]
        );
    }
}
