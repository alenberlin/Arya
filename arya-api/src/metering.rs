//! Hold -> settle metering with exactly-once settlement.
//!
//! authorize() records a hold (estimated cap, TTL). settle() debits actual
//! usage exactly once per idempotency key: retries return the original
//! receipt with `replay = true`. Stripe wallet sync arrives in M12; the
//! ledger here is the source of truth the desktop sees.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

pub async fn init_pool(path: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    // A `:memory:` database is per-connection, so a multi-connection pool
    // would scatter tables across separate databases. Pin it to one.
    let mut builder = SqlitePoolOptions::new();
    if path == ":memory:" {
        builder = builder.max_connections(1);
    }
    let pool = builder.connect_with(options).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS holds (
             id TEXT PRIMARY KEY,
             user_id TEXT NOT NULL,
             action TEXT NOT NULL,
             cap_credits INTEGER NOT NULL,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
             expires_at TEXT NOT NULL,
             settled INTEGER NOT NULL DEFAULT 0
         )",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS charges (
             idempotency_key TEXT PRIMARY KEY,
             user_id TEXT NOT NULL,
             action TEXT NOT NULL,
             credits INTEGER NOT NULL,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         )",
    )
    .execute(&pool)
    .await?;
    Ok(pool)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Hold {
    pub id: String,
    pub cap_credits: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Receipt {
    pub credits: u64,
    pub replay: bool,
}

/// Reserves an estimated amount for an action. (Balance enforcement joins in
/// M12 with the wallet; the hold discipline and shapes are fixed now.)
pub async fn authorize(
    pool: &SqlitePool,
    user_id: &str,
    action: &str,
    estimate_credits: u64,
    ttl_seconds: i64,
) -> Result<Hold, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO holds (id, user_id, action, cap_credits, expires_at)
         VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ','now', '+' || ?5 || ' seconds'))",
    )
    .bind(&id)
    .bind(user_id)
    .bind(action)
    .bind(estimate_credits as i64)
    .bind(ttl_seconds)
    .execute(pool)
    .await?;
    Ok(Hold {
        id,
        cap_credits: estimate_credits,
    })
}

/// Settles actual usage exactly once. A repeated idempotency key returns the
/// originally recorded credits with `replay = true` and never double-debits.
pub async fn settle(
    pool: &SqlitePool,
    hold: &Hold,
    user_id: &str,
    action: &str,
    actual_credits: u64,
    idempotency_key: &str,
) -> Result<Receipt, sqlx::Error> {
    let clamped = actual_credits.min(hold.cap_credits.max(1));
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO charges (idempotency_key, user_id, action, credits)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(idempotency_key)
    .bind(user_id)
    .bind(action)
    .bind(clamped as i64)
    .execute(pool)
    .await?
    .rows_affected();

    sqlx::query("UPDATE holds SET settled = 1 WHERE id = ?1")
        .bind(&hold.id)
        .execute(pool)
        .await?;

    if inserted == 1 {
        Ok(Receipt {
            credits: clamped,
            replay: false,
        })
    } else {
        let original =
            sqlx::query_scalar::<_, i64>("SELECT credits FROM charges WHERE idempotency_key = ?1")
                .bind(idempotency_key)
                .fetch_one(pool)
                .await?;
        Ok(Receipt {
            credits: original as u64,
            replay: true,
        })
    }
}

/// Total credits charged to a user (the dev-mode "usage" figure).
pub async fn total_charged(pool: &SqlitePool, user_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(credits), 0) FROM charges WHERE user_id = ?1")
        .bind(user_id)
        .fetch_one(pool)
        .await
}

/// Credits for token usage at catalog prices, rounding up so work is never
/// free due to truncation.
pub fn credits_for_tokens(
    input_tokens: u64,
    output_tokens: u64,
    input_per_mtok: u64,
    output_per_mtok: u64,
) -> u64 {
    let input = (input_tokens as u128 * input_per_mtok as u128).div_ceil(1_000_000) as u64;
    let output = (output_tokens as u128 * output_per_mtok as u128).div_ceil(1_000_000) as u64;
    (input + output).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        init_pool(":memory:").await.expect("pool")
    }

    #[tokio::test]
    async fn settle_is_exactly_once_under_forced_retry() {
        let pool = pool().await;
        let hold = authorize(&pool, "u1", "agent_chat", 500, 60).await.unwrap();

        let first = settle(&pool, &hold, "u1", "agent_chat", 123, "key-1")
            .await
            .unwrap();
        assert_eq!(first.credits, 123);
        assert!(!first.replay);

        // Forced retry with the same idempotency key - even with different
        // claimed actuals - must not double-charge.
        let retry = settle(&pool, &hold, "u1", "agent_chat", 999, "key-1")
            .await
            .unwrap();
        assert_eq!(retry.credits, 123);
        assert!(retry.replay);

        assert_eq!(total_charged(&pool, "u1").await.unwrap(), 123);
    }

    #[tokio::test]
    async fn settle_clamps_to_hold_cap() {
        let pool = pool().await;
        let hold = authorize(&pool, "u1", "agent_chat", 100, 60).await.unwrap();
        let receipt = settle(&pool, &hold, "u1", "agent_chat", 5_000, "key-2")
            .await
            .unwrap();
        assert_eq!(receipt.credits, 100);
    }

    #[test]
    fn token_pricing_rounds_up_and_never_zero() {
        // 1000 tokens at 3000 credits/Mtok = 3 credits exactly.
        assert_eq!(credits_for_tokens(1_000, 0, 3_000, 15_000), 3);
        // Each side rounds up independently (10 in + 10 out at 1/Mtok = 1+1).
        assert_eq!(credits_for_tokens(10, 10, 1, 1), 2);
        // Zero usage still charges the one-credit floor.
        assert_eq!(credits_for_tokens(0, 0, 3_000, 15_000), 1);
        // Rounding is up, not down.
        assert_eq!(credits_for_tokens(1, 0, 3_000, 0), 1);
    }
}
