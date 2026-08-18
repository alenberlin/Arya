//! Staging user-supplied files into the app workspace.
//!
//! Files reach Arya two ways. Picked through the OS dialog we get a path and
//! copy it; dropped onto the window we get *contents*, because the main window
//! runs with `dragDropEnabled: false` (see `tauri.conf.json`) so that in-page
//! HTML5 dragging keeps working — which means wry never sees the drag and the
//! frontend only has a `File`, never a path. Both routes land under a
//! uuid-prefixed name in the workspace, and both go through the same sanitizer:
//! a dropped name is untrusted input and must not escape its directory.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use tauri::ipc::Request;

/// Where a staged file's contents come from.
pub enum FileSource {
    /// Copy from a path the user picked in the OS dialog.
    Path(PathBuf),
    /// Write bytes that arrived over IPC from a window drop.
    Bytes(Vec<u8>),
}

/// Reduce an untrusted name to a bare file name safe to join onto a directory.
/// A dropped file reports whatever name its producer chose, so a value like
/// `../../.ssh/id_rsa` must not be able to walk out of the workspace.
pub fn safe_file_name(raw: &str) -> String {
    // `file_name` already discards every leading path segment as well as the
    // `.` and `..` components; the filter covers separators it leaves alone on
    // this platform.
    let base = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let cleaned: String = base
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "document".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Read a percent-encoded metadata header off a raw-body IPC request. Header
/// values must be ASCII, so the frontend encodes filenames before sending them.
pub fn header(request: &Request<'_>, key: &str) -> Result<String, String> {
    let raw = request
        .headers()
        .get(key)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("missing {key} header"))?;
    Ok(percent_decode_str(raw).decode_utf8_lossy().into_owned())
}

/// Stage `source` into `<dir>/<id>-<name>`, creating `dir` if needed. Returns
/// the stored path and its size on disk. The uuid prefix keeps the workspace
/// name unique even when two files share an original filename.
///
/// # Errors
/// Returns the underlying message if the directory cannot be created or the
/// file cannot be copied or written.
pub async fn stage(
    dir: PathBuf,
    id: &str,
    name: &str,
    source: FileSource,
) -> Result<(String, i64), String> {
    let dest = dir.join(format!("{id}-{name}"));
    let stored_path = dest.to_string_lossy().to_string();
    // A staged file can be hundreds of megabytes; never on the async threads.
    let size = tauri::async_runtime::spawn_blocking(move || -> Result<i64, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        match source {
            FileSource::Path(src) => {
                std::fs::copy(&src, &dest).map_err(|e| format!("copy failed: {e}"))?;
            }
            FileSource::Bytes(bytes) => {
                std::fs::write(&dest, &bytes).map_err(|e| format!("write failed: {e}"))?;
            }
        }
        Ok(std::fs::metadata(&dest)
            .map(|m| m.len() as i64)
            .unwrap_or(0))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok((stored_path, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_name_strips_traversal() {
        assert_eq!(safe_file_name("report.pdf"), "report.pdf");
        assert_eq!(safe_file_name("../../.ssh/id_rsa"), "id_rsa");
        assert_eq!(safe_file_name("/etc/passwd"), "passwd");
        assert_eq!(safe_file_name("a\\b.txt"), "ab.txt");
        // Names that carry no usable component fall back rather than erroring.
        assert_eq!(safe_file_name(".."), "document");
        assert_eq!(safe_file_name("/"), "document");
        assert_eq!(safe_file_name("   "), "document");
        // Non-ASCII names survive intact — they arrive percent-decoded.
        assert_eq!(safe_file_name("Résumé.pdf"), "Résumé.pdf");
    }

    #[tokio::test]
    async fn stage_writes_bytes_and_copies_paths() {
        let tmp = std::env::temp_dir().join(format!("arya-stage-{}", uuid::Uuid::new_v4()));
        let (path, size) = stage(
            tmp.clone(),
            "id1",
            "dropped.txt",
            FileSource::Bytes(b"hello".to_vec()),
        )
        .await
        .unwrap();
        assert!(path.ends_with("id1-dropped.txt"));
        assert_eq!(size, 5);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");

        let src = tmp.join("origin.txt");
        std::fs::write(&src, b"copied").unwrap();
        let (path, size) = stage(tmp.clone(), "id2", "picked.txt", FileSource::Path(src))
            .await
            .unwrap();
        assert_eq!(size, 6);
        assert_eq!(std::fs::read(&path).unwrap(), b"copied");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
