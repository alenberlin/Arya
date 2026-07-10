//! Import a Notion "Markdown & CSV" export (unzipped folder) as a page tree (F4).
//!
//! Notion names each page file `Title <32-hex-id>.md` and puts a page's children
//! in a sibling folder `Title <32-hex-id>/`. We mirror that structure into ARYA:
//! the folder hierarchy becomes `parent_note_id`, the markdown becomes `body_md`
//! (the block editor lazy-converts it to blocks on first open), and internal
//! links to other exported pages become `mention` edges in the connected-brain
//! graph. Best-effort: a file that fails to import is counted as skipped and
//! never aborts the whole run. Non-`.md` files (assets, database CSVs — a
//! non-goal) are ignored.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::notes::{insert_note_under, update_note_fields};

/// What an import did — surfaced to the user afterward.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub pages_created: usize,
    pub links_resolved: usize,
    pub skipped: usize,
}

/// The trailing 32-char lowercase hex id Notion appends to a page name, if any.
fn notion_hex_id(stem: &str) -> Option<String> {
    let candidate = stem.rsplit(' ').next()?;
    if candidate.len() == 32 && candidate.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(candidate.to_ascii_lowercase())
    } else {
        None
    }
}

/// The human title: everything before the trailing hex id (or the whole stem).
fn notion_title(stem: &str) -> String {
    match notion_hex_id(stem) {
        Some(id) => stem
            .strip_suffix(&id)
            .map(|s| s.trim_end().to_string())
            .unwrap_or_else(|| stem.to_string()),
        None => stem.to_string(),
    }
}

/// Import an unzipped Notion export rooted at `root`.
pub async fn import_notion_folder(
    pool: &SqlitePool,
    root: &std::path::Path,
) -> Result<ImportReport, String> {
    if !root.is_dir() {
        return Err("please choose the unzipped Notion export folder".into());
    }
    let mut report = ImportReport::default();
    let mut hex_to_note: HashMap<String, String> = HashMap::new();
    let mut created: Vec<(String, String)> = Vec::new(); // (note_id, markdown)

    // Iterative walk (avoids async recursion): each dir carries the note id of
    // the page it belongs under. A page's children live in `Name <hex>/`, which
    // we resolve to the note created for `Name <hex>.md` in the same directory.
    let mut stack: Vec<(PathBuf, Option<String>)> = vec![(root.to_path_buf(), None)];
    while let Some((dir, parent)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut md_files: Vec<PathBuf> = Vec::new();
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                md_files.push(path);
            }
        }

        // Create the pages in this directory first, so its subdirs can resolve
        // their parent by hex id.
        for md in &md_files {
            let stem = md.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let title = notion_title(stem);
            let markdown = std::fs::read_to_string(md).unwrap_or_default();
            match insert_note_under(pool, &title, parent.as_deref()).await {
                Ok(note) => {
                    let _ =
                        update_note_fields(pool, &note.id, None, Some(&markdown), None, None).await;
                    if let Some(hex) = notion_hex_id(stem) {
                        hex_to_note.insert(hex, note.id.clone());
                    }
                    created.push((note.id, markdown));
                    report.pages_created += 1;
                }
                Err(_) => report.skipped += 1,
            }
        }

        for sub in subdirs {
            let stem = sub.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let sub_parent = notion_hex_id(stem)
                .and_then(|h| hex_to_note.get(&h).cloned())
                .or_else(|| parent.clone());
            stack.push((sub, sub_parent));
        }
    }

    // Second pass: every internal link (a reference to another exported page's
    // hex id) becomes a `mention` edge, so the connection lives in the graph even
    // though the inline markdown link isn't a mention chip yet.
    for (note_id, markdown) in &created {
        for (hex, target_id) in &hex_to_note {
            if target_id == note_id {
                continue;
            }
            if markdown.contains(hex.as_str())
                && crate::links::insert_link(
                    pool, "note", note_id, "note", target_id, "mention", "user", 1.0,
                )
                .await
                .is_ok()
            {
                report.links_resolved += 1;
            }
        }
    }

    Ok(report)
}

/// Import an unzipped Notion export folder chosen by the user.
#[tauri::command]
pub async fn import_notion(
    pool: State<'_, SqlitePool>,
    dir_path: String,
) -> Result<ImportReport, String> {
    import_notion_folder(&pool, &PathBuf::from(dir_path)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn parses_notion_titles_and_ids() {
        let hex = "a".repeat(32);
        let stem = format!("My Page {hex}");
        assert_eq!(notion_hex_id(&stem), Some(hex.clone()));
        assert_eq!(notion_title(&stem), "My Page");
        // No trailing hex id → the whole stem is the title.
        assert_eq!(notion_hex_id("Plain Note"), None);
        assert_eq!(notion_title("Plain Note"), "Plain Note");
    }

    #[tokio::test]
    async fn imports_a_nested_export_with_internal_links() {
        let pool = test_pool().await;
        let root = std::env::temp_dir().join(format!("arya-notion-{}", uuid::Uuid::new_v4()));
        let parent_hex = "a".repeat(32);
        let child_hex = "b".repeat(32);
        let parent_dir = root.join(format!("Parent {parent_hex}"));
        std::fs::create_dir_all(&parent_dir).unwrap();
        // The parent page links to the child by its hex id → becomes an edge.
        std::fs::write(
            root.join(format!("Parent {parent_hex}.md")),
            format!("# Parent\n\nSee [Child](Child%20{child_hex}.md)"),
        )
        .unwrap();
        std::fs::write(
            parent_dir.join(format!("Child {child_hex}.md")),
            "# Child\n\nbody",
        )
        .unwrap();

        let report = import_notion_folder(&pool, &root).await.unwrap();
        assert_eq!(report.pages_created, 2);

        let notes = crate::notes::fetch_notes(&pool).await.unwrap();
        let parent = notes
            .iter()
            .find(|n| n.title == "Parent")
            .expect("parent note");
        let child = notes
            .iter()
            .find(|n| n.title == "Child")
            .expect("child note");
        assert_eq!(
            child.parent_note_id.as_deref(),
            Some(parent.id.as_str()),
            "folder structure became the page hierarchy"
        );
        let out = crate::links::links_from(&pool, "note", &parent.id)
            .await
            .unwrap();
        assert!(
            out.iter().any(|l| l.target_id == child.id),
            "the internal link became a mention edge"
        );
        assert!(report.links_resolved >= 1);

        let _ = std::fs::remove_dir_all(&root);
    }
}
