//! Accounts, plans, and wallets.
//!
//! A [`Wallet`] backend answers "what plan is this user on and how many
//! credits remain". Two implementations:
//!   - [`LocalWallet`]: dev/open-source mode. Every user is on a generous
//!     Free-plus plan; credits refill so the whole app works with no Stripe.
//!   - Stripe-backed billing plugs in behind the same trait (M12 follow-up
//!     once a Stripe account exists); the wire shapes here are what the
//!     desktop already consumes, so swapping the backend needs no app change.
//!
//! Local models never consume credits (priced at the catalog floor and,
//! more importantly, the client marks them local and skips the proxy).

use crate::metering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Free,
    Pro,
    Max,
}

impl Tier {
    /// Monthly included credits per tier.
    pub fn included_credits(self) -> i64 {
        match self {
            Tier::Free => 20_000,
            Tier::Pro => 500_000,
            Tier::Max => 3_000_000,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::Free => "free",
            Tier::Pro => "pro",
            Tier::Max => "max",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSnapshot {
    pub user_id: String,
    pub tier: Tier,
    pub included_credits: i64,
    pub used_credits: i64,
    pub topup_credits: i64,
    pub remaining_credits: i64,
    /// True once the user has any paid entitlement (Pro or Max).
    pub subscribed: bool,
}

/// The billing backend seam.
pub trait Wallet: Send + Sync {
    /// The account snapshot the desktop shows (tier, balance, usage).
    fn snapshot(&self, user_id: &str, used_credits: i64) -> AccountSnapshot;
    /// The spendable budget (included + top-up credits) the metering layer
    /// enforces holds against, or `None` when this wallet does not enforce a
    /// balance. Returning `None` is only safe when the wallet genuinely has
    /// no metered spend (pure local, no cloud keys).
    fn budget_credits(&self, snapshot: &AccountSnapshot) -> Option<i64>;
}

/// Dev/open-source wallet: users are on a tier with real included credits,
/// and the metering layer enforces that budget (so even in local mode a user
/// cannot run unbounded paid-provider calls). `enforce = false` is a
/// deliberate, no-cloud dev escape hatch.
pub struct LocalWallet {
    tier: Tier,
    topup: i64,
    enforce: bool,
}

impl LocalWallet {
    pub fn from_env() -> Self {
        let tier = match std::env::var("ARYA_LOCAL_TIER").as_deref() {
            Ok("max") => Tier::Max,
            Ok("free") => Tier::Free,
            _ => Tier::Pro,
        };
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
            tier,
            topup: 0,
            enforce: has_cloud && !opt_out,
        }
    }
}

impl Wallet for LocalWallet {
    fn snapshot(&self, user_id: &str, used_credits: i64) -> AccountSnapshot {
        let included = self.tier.included_credits();
        AccountSnapshot {
            user_id: user_id.to_string(),
            tier: self.tier,
            included_credits: included,
            used_credits,
            topup_credits: self.topup,
            remaining_credits: (included + self.topup - used_credits).max(0),
            subscribed: self.tier != Tier::Free,
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
    fn tiers_have_ascending_credits() {
        assert!(Tier::Free.included_credits() < Tier::Pro.included_credits());
        assert!(Tier::Pro.included_credits() < Tier::Max.included_credits());
    }

    #[test]
    fn local_snapshot_reports_remaining_after_usage() {
        let wallet = LocalWallet {
            tier: Tier::Pro,
            topup: 10_000,
            enforce: false,
        };
        let snap = wallet.snapshot("u1", 30_000);
        assert_eq!(snap.included_credits, 500_000);
        assert_eq!(snap.topup_credits, 10_000);
        assert_eq!(snap.remaining_credits, 480_000);
        assert!(snap.subscribed);
        // Unmetered dev wallet exposes no budget cap.
        assert_eq!(wallet.budget_credits(&snap), None);
    }

    #[test]
    fn enforcing_wallet_exposes_budget() {
        let wallet = LocalWallet {
            tier: Tier::Pro,
            topup: 10_000,
            enforce: true,
        };
        let snap = wallet.snapshot("u1", 0);
        assert_eq!(wallet.budget_credits(&snap), Some(510_000));
    }

    #[test]
    fn free_tier_is_not_subscribed() {
        let wallet = LocalWallet {
            tier: Tier::Free,
            topup: 0,
            enforce: false,
        };
        let snap = wallet.snapshot("u1", 0);
        assert!(!snap.subscribed);
        assert_eq!(snap.remaining_credits, 20_000);
    }
}
