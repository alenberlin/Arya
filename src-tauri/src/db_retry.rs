//! Resilient write helpers: retry transient SQLite errors with exponential
//! backoff, and persist uncommitted writes to a local WAL-replay file so a
//! crash mid-flush can be picked up on next session start.
//!
//! The single-writer shell used to surface `SQLITE_BUSY` straight to the UI
//! during recording peaks (mic WAV finalize + system-audio stop racing RAG
//! reindex). Wrapping write paths here turns a transient lock into a small
//! wait, and an outright "I can't write this" into a durable spool that
//! replays next launch.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::time::sleep;
use tracing::warn;

use crate::error::{AryaError, AryaResult};

/// Default max attempts (1 + 2 retries) before falling back to the WAL file.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Base delay; doubled each retry with light jitter.
pub const BASE_BACKOFF: Duration = Duration::from_millis(80);
/// Cap so the longest retry waits for at most a quarter-second.
pub const MAX_BACKOFF: Duration = Duration::from_millis(250);

/// Returns true when the SQLite error looks like a transient lock / busy
/// condition rather than a logical SQL error (constraint violation, bad
/// query). Logical errors should fail fast so the caller can correct them.
pub fn is_transient(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db_err) => {
            // SQLITE_BUSY = 5, SQLITE_LOCKED = 6, SQLITE_BUSY_SNAPSHOT = 517.
            // Anything that hints "try again" rather than "your SQL is wrong".
            db_err.code().as_deref() == Some("5")
                || db_err.code().as_deref() == Some("6")
                || db_err.code().as_deref() == Some("517")
        }
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => true,
        sqlx::Error::Io(_) => true,
        _ => false,
    }
}

/// Run `op`, retrying transient SQLite errors with exponential backoff.
/// Non-transient errors are returned immediately so the caller can surface
/// them. The future factory returns a fresh future per attempt so retries
/// don't share state with the first try.
pub async fn write_with_retry<F, T>(pool: SqlitePool, op: F) -> AryaResult<T>
where
    F: Fn(SqlitePool) -> futures_util::future::BoxFuture<'static, Result<T, sqlx::Error>>,
{
    let mut attempt: u32 = 0;
    let mut backoff = BASE_BACKOFF;
    loop {
        let next = op(pool.clone());
        match next.await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < DEFAULT_MAX_ATTEMPTS && is_transient(&err) => {
                attempt += 1;
                warn!(
                    attempt,
                    max = DEFAULT_MAX_ATTEMPTS,
                    backoff_ms = backoff.as_millis() as u64,
                    error = %err,
                    "db_retry: transient error; retrying"
                );
                sleep(backoff).await;
                // Mild jitter: ±20%.
                let jitter_ms = (backoff.as_millis() as f64 * 0.2) as u64;
                let jitter = Duration::from_millis(jitter_ms);
                backoff = (backoff * 2 + jitter).min(MAX_BACKOFF);
            }
            Err(err) => return Err(AryaError::from_display(err)),
        }
    }
}

/// Append a serialized write intent to the on-disk replay file. Used as a
/// last-resort fallback when the retry budget is exhausted; the next session
/// start drains the file before opening the UI.
///
/// The intent format is intentionally minimal: a length-prefixed newline-
/// terminated UTF-8 record. Anything that can be parsed is replayed in order.
pub async fn spool_for_replay(data_dir: &Path, intent: &str) -> AryaResult<()> {
    let path = spool_path(data_dir);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(AryaError::from_display)?;
    }
    use tokio::io::AsyncWriteExt;
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .map_err(AryaError::from_display)?;
    f.write_all(format!("{}\n", intent.len()).as_bytes())
        .await
        .map_err(AryaError::from_display)?;
    f.write_all(intent.as_bytes())
        .await
        .map_err(AryaError::from_display)?;
    f.write_all(b"\n")
        .await
        .map_err(AryaError::from_display)?;
    f.flush().await.map_err(AryaError::from_display)?;
    Ok(())
}

pub fn spool_path(data_dir: &Path) -> PathBuf {
    data_dir.join("db-write-replay.log")
}

/// Read and clear the replay log, returning each intent in order. Caller is
/// responsible for interpreting intents (currently a small closed enum).
pub async fn drain_replay(data_dir: &Path) -> AryaResult<Vec<String>> {
    let path = spool_path(data_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(AryaError::from_display)?;
    // Best-effort truncation; crash before truncation is fine — replay is
    // idempotent because the table writes are themselves idempotent (INSERT
    // OR REPLACE / upserts).
    let _ = tokio::fs::remove_file(&path).await;
    // The on-disk format is `len\nbody\n`. Skip malformed records silently so
    // one bad line can't block the rest from replaying.
    let mut out = Vec::new();
    let mut lines = raw.lines();
    while let Some(len_line) = lines.next() {
        let Ok(len) = len_line.parse::<usize>() else {
            continue;
        };
        if let Some(body) = lines.next() {
            if body.len() == len {
                out.push(body.to_string());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_transient_and_logical() {
        // PoolTimedOut and PoolClosed are transient.
        assert!(is_transient(&sqlx::Error::PoolTimedOut));
        assert!(is_transient(&sqlx::Error::PoolClosed));
        // RowNotFound is logical, not transient.
        assert!(!is_transient(&sqlx::Error::RowNotFound));
        // Configuration errors are logical, not transient.
        assert!(!is_transient(&sqlx::Error::Configuration(
            "not a transient err".into(),
        )));
    }

    #[tokio::test]
    async fn replay_round_trips() {
        let dir = std::env::temp_dir().join(format!("arya-replay-{}", uuid::Uuid::new_v4()));
        spool_for_replay(&dir, "hello").await.unwrap();
        spool_for_replay(&dir, "world").await.unwrap();
        let drained = drain_replay(&dir).await.unwrap();
        assert_eq!(drained, vec!["hello", "world"]);
        // Re-draining is a no-op once the file is removed.
        let drained2 = drain_replay(&dir).await.unwrap();
        assert!(drained2.is_empty());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}