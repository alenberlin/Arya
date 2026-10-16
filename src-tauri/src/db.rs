use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// Errors from database initialization.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("failed to create data directory: {0}")]
    CreateDir(#[from] std::io::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// Opens (creating if needed) the SQLite database at `path` and runs all
/// pending migrations. The shell is the single writer; WAL keeps readers
/// unblocked during writes.
pub async fn init_pool(path: &Path) -> Result<SqlitePool, DbError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        // Wait for a contended write lock instead of failing immediately with
        // SQLITE_BUSY: several blocking paths (dictation history, processing
        // status, RAG reindex) can write concurrently under WAL.
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true)
        // WAL already gives crash safety, so NORMAL synchronous avoids an fsync
        // on every commit (FULL is redundant with WAL). A modest page cache and
        // memory-mapped I/O — both caps, not reservations — keep KB/RAG scans
        // fast without a fixed memory tax. TEMP store in MEMORY keeps RAG/KB
        // scratch tables off disk for short-lived aggregations.
        .pragma("synchronous", "NORMAL")
        .pragma("cache_size", "-64000")
        .pragma("mmap_size", "268435456")
        .pragma("temp_store", "MEMORY");
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    // PRAGMA optimize refreshes the query planner's statistics; without it
    // long-lived pools can drift toward stale plans after many writes. Cheap,
    // safe to call after migrations.
    sqlx::query("PRAGMA optimize").execute(&pool).await?;
    Ok(pool)
}

/// In-memory pool for tests. Single connection: each `:memory:` connection
/// is otherwise its own database.
#[cfg(test)]
pub async fn test_pool() -> SqlitePool {
    use std::str::FromStr;
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("valid options")
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("in-memory pool");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations apply");
    pool
}
