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
    /// Whether an action of `estimate` credits may proceed. Local mode always
    /// allows; Stripe mode checks the real balance.
    fn can_spend(&self, snapshot: &AccountSnapshot, estimate: i64) -> bool;
}

/// Dev/open-source wallet: everyone is effectively funded so the product is
/// fully usable without Stripe. Tier is read from an env override for demos.
pub struct LocalWallet {
    tier: Tier,
    topup: i64,
}

impl LocalWallet {
    pub fn from_env() -> Self {
        let tier = match std::env::var("ARYA_LOCAL_TIER").as_deref() {
            Ok("max") => Tier::Max,
            Ok("free") => Tier::Free,
            _ => Tier::Pro,
        };
        Self { tier, topup: 0 }
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

    fn can_spend(&self, _snapshot: &AccountSnapshot, _estimate: i64) -> bool {
        // Local mode is always funded; the metering ledger still records
        // usage so the UI shows a real number.
        true
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
        };
        let snap = wallet.snapshot("u1", 30_000);
        assert_eq!(snap.included_credits, 500_000);
        assert_eq!(snap.topup_credits, 10_000);
        assert_eq!(snap.remaining_credits, 480_000);
        assert!(snap.subscribed);
        assert!(wallet.can_spend(&snap, 999_999));
    }

    #[test]
    fn free_tier_is_not_subscribed() {
        let wallet = LocalWallet {
            tier: Tier::Free,
            topup: 0,
        };
        let snap = wallet.snapshot("u1", 0);
        assert!(!snap.subscribed);
        assert_eq!(snap.remaining_credits, 20_000);
    }
}
