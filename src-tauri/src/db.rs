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
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
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
