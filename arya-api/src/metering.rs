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

/// A denied authorization, distinct from an infrastructure error so the
/// caller can map it to 402 rather than 503.
#[derive(Debug)]
pub enum AuthorizeError {
    InsufficientCredits { remaining: i64, estimate: u64 },
    Db(sqlx::Error),
}

impl From<sqlx::Error> for AuthorizeError {
    fn from(e: sqlx::Error) -> Self {
        AuthorizeError::Db(e)
    }
}

/// Reserves an estimated amount, enforcing the balance transactionally so
/// concurrent requests can't collectively overspend (no check-then-act
/// TOCTOU). `budget_credits` is the wallet's spendable total (included +
/// top-up); `None` means the wallet does not enforce a balance (local mode).
pub async fn authorize(
    pool: &SqlitePool,
    user_id: &str,
    action: &str,
    estimate_credits: u64,
    ttl_seconds: i64,
    budget_credits: Option<i64>,
) -> Result<Hold, AuthorizeError> {
    let mut tx = pool.begin().await?;
    if let Some(budget) = budget_credits {
        // Spendable = budget - settled charges - open (unexpired) holds.
        let settled = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(credits), 0) FROM charges WHERE user_id = ?1",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        let open_holds = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(cap_credits), 0) FROM holds
             WHERE user_id = ?1 AND settled = 0
               AND expires_at > strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        let remaining = budget - settled - open_holds;
        if remaining < estimate_credits as i64 {
            return Err(AuthorizeError::InsufficientCredits {
                remaining,
                estimate: estimate_credits,
            });
        }
    }
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
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Hold {
        id,
        cap_credits: estimate_credits,
    })
}

/// Looks up an already-settled charge by idempotency key, if any. Used to
/// short-circuit a duplicate request BEFORE calling the upstream provider, so
/// a retry never pays the provider twice.
pub async fn existing_charge(
    pool: &SqlitePool,
    idempotency_key: &str,
) -> Result<Option<u64>, sqlx::Error> {
    let found =
        sqlx::query_scalar::<_, i64>("SELECT credits FROM charges WHERE idempotency_key = ?1")
            .bind(idempotency_key)
            .fetch_optional(pool)
            .await?;
    Ok(found.map(|c| c as u64))
}

/// Releases expired, unsettled holds so they stop counting against balance.
pub async fn reap_expired_holds(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM holds WHERE settled = 0
         AND expires_at <= strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
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
    // Charge insert and hold settle in one transaction: a crash between them
    // must not leave a charge with an un-settled hold (which would distort
    // the balance math above).
    let mut tx = pool.begin().await?;
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO charges (idempotency_key, user_id, action, credits)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(idempotency_key)
    .bind(user_id)
    .bind(action)
    .bind(clamped as i64)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query("UPDATE holds SET settled = 1 WHERE id = ?1")
        .bind(&hold.id)
        .execute(&mut *tx)
        .await?;

    if inserted == 1 {
        tx.commit().await?;
        Ok(Receipt {
            credits: clamped,
            replay: false,
        })
    } else {
        let original =
            sqlx::query_scalar::<_, i64>("SELECT credits FROM charges WHERE idempotency_key = ?1")
                .bind(idempotency_key)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
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
        let hold = authorize(&pool, "u1", "agent_chat", 500, 60, None)
            .await
            .unwrap();

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
        let hold = authorize(&pool, "u1", "agent_chat", 100, 60, None)
            .await
            .unwrap();
        let receipt = settle(&pool, &hold, "u1", "agent_chat", 5_000, "key-2")
            .await
            .unwrap();
        assert_eq!(receipt.credits, 100);
    }

    #[tokio::test]
    async fn authorize_rejects_when_budget_exhausted() {
        let pool = pool().await;
        // Budget 1000, spend 900, then a 200-credit estimate must be denied.
        let hold = authorize(&pool, "u1", "agent_chat", 900, 60, Some(1000))
            .await
            .unwrap();
        settle(&pool, &hold, "u1", "agent_chat", 900, "k1")
            .await
            .unwrap();
        let denied = authorize(&pool, "u1", "agent_chat", 200, 60, Some(1000)).await;
        assert!(matches!(
            denied,
            Err(AuthorizeError::InsufficientCredits { .. })
        ));
        // A small estimate within remaining still passes.
        assert!(authorize(&pool, "u1", "agent_chat", 50, 60, Some(1000))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn concurrent_holds_count_against_balance() {
        let pool = pool().await;
        // Two 600-credit holds against a 1000 budget: the second is denied
        // because open holds count, closing the overspend TOCTOU.
        let _h1 = authorize(&pool, "u1", "agent_chat", 600, 60, Some(1000))
            .await
            .unwrap();
        let second = authorize(&pool, "u1", "agent_chat", 600, 60, Some(1000)).await;
        assert!(matches!(
            second,
            Err(AuthorizeError::InsufficientCredits { .. })
        ));
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
