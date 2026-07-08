//! Accounts and wallets.
//!
//! Arya is 100% free and open source — there are no paid tiers and nothing
//! here is a billing system. A [`Wallet`] backend answers "how many credits
//! does this user have and how many remain"; its only purpose is to meter
//! usage of the shared, optional hosted cloud proxy so it can't be abused,
//! never to gate a paywall.
//!
//! [`LocalWallet`] is the only implementation: every user gets the same
//! generous, regularly-refilling credit allowance. Local models never
//! consume credits at all (priced at the catalog floor and, more
//! importantly, the client marks them local and skips the proxy).

use crate::metering;

/// Monthly included credits — one flat, generous allowance for everyone.
pub const INCLUDED_CREDITS: i64 = 500_000;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    pub user_id: String,
    pub included_credits: i64,
    pub used_credits: i64,
    pub topup_credits: i64,
    pub remaining_credits: i64,
}

/// The wallet backend seam.
pub trait Wallet: Send + Sync {
    /// The account snapshot the desktop shows (balance, usage).
    fn snapshot(&self, user_id: &str, used_credits: i64) -> AccountSnapshot;
    /// The spendable budget (included + top-up credits) the metering layer
    /// enforces holds against, or `None` when this wallet does not enforce a
    /// balance. Returning `None` is only safe when the wallet genuinely has
    /// no metered spend (pure local, no cloud keys).
    fn budget_credits(&self, snapshot: &AccountSnapshot) -> Option<i64>;
}

/// Dev/open-source wallet: every user gets the same included-credit
/// allowance, and the metering layer enforces that budget (so even in local
/// mode a user cannot run unbounded paid-provider calls). `enforce = false`
/// is a deliberate, no-cloud dev escape hatch.
pub struct LocalWallet {
    topup: i64,
    enforce: bool,
}

impl LocalWallet {
    pub fn from_env() -> Self {
        // Enforce the budget whenever a real cloud provider key is present -
        // otherwise a local wallet in front of paid keys is unmetered access.
        // Explicit opt-out only via ARYA_LOCAL_UNMETERED=1 for offline dev.
        let has_cloud = std::env::var("ANTHROPIC_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false)
            || std::env::var("OPENAI_API_KEY")
                .map(|k| !k.is_empty())
                .unwrap_or(false);
        let opt_out = std::env::var("ARYA_LOCAL_UNMETERED").as_deref() == Ok("1");
        Self {
            topup: 0,
            enforce: has_cloud && !opt_out,
        }
    }
}

impl Wallet for LocalWallet {
    fn snapshot(&self, user_id: &str, used_credits: i64) -> AccountSnapshot {
        AccountSnapshot {
            user_id: user_id.to_string(),
            included_credits: INCLUDED_CREDITS,
            used_credits,
            topup_credits: self.topup,
            remaining_credits: (INCLUDED_CREDITS + self.topup - used_credits).max(0),
        }
    }

    fn budget_credits(&self, snapshot: &AccountSnapshot) -> Option<i64> {
        if self.enforce {
            Some(snapshot.included_credits + snapshot.topup_credits)
        } else {
            None
        }
    }
}

/// Convenience: build a snapshot from the ledger for a user.
pub async fn account_snapshot(
    pool: &sqlx::SqlitePool,
    wallet: &dyn Wallet,
    user_id: &str,
) -> Result<AccountSnapshot, sqlx::Error> {
    let used = metering::total_charged(pool, user_id).await?;
    Ok(wallet.snapshot(user_id, used))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_snapshot_reports_remaining_after_usage() {
        let wallet = LocalWallet {
            topup: 10_000,
            enforce: false,
        };
        let snap = wallet.snapshot("u1", 30_000);
        assert_eq!(snap.included_credits, INCLUDED_CREDITS);
        assert_eq!(snap.topup_credits, 10_000);
        assert_eq!(snap.remaining_credits, INCLUDED_CREDITS + 10_000 - 30_000);
        // Unmetered dev wallet exposes no budget cap.
        assert_eq!(wallet.budget_credits(&snap), None);
    }

    #[test]
    fn enforcing_wallet_exposes_budget() {
        let wallet = LocalWallet {
            topup: 10_000,
            enforce: true,
        };
        let snap = wallet.snapshot("u1", 0);
        assert_eq!(
            wallet.budget_credits(&snap),
            Some(INCLUDED_CREDITS + 10_000)
        );
    }

    #[test]
    fn remaining_credits_never_goes_negative() {
        let wallet = LocalWallet {
            topup: 0,
            enforce: false,
        };
        let snap = wallet.snapshot("u1", INCLUDED_CREDITS * 2);
        assert_eq!(snap.remaining_credits, 0);
    }
}
