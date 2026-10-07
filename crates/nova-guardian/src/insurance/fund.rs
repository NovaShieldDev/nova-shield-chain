//! Sigorta fonu havuzu (§5.8.5)
//!
//! Kurumsal guardian'ların katkıda bulunduğu ve başarısız kurtarma
//! durumunda kullanıcılara tazminat ödeyen havuz.

use crate::errors::{GuardianError, GuardianResult};
use nova_primitives::Address;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── InsuranceFund ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct FundState {
    balance: u128,
    paid_out: u128,
    contributions: u128,
}

/// Sigorta fonu — toplam NSC havuzu
#[derive(Debug, Default)]
pub struct InsuranceFund {
    state: Arc<Mutex<FundState>>,
}

impl InsuranceFund {
    pub fn new() -> Self {
        Self::default()
    }

    /// Guardian katkı ödemesi
    pub fn deposit(&self, amount: u128) -> GuardianResult<u128> {
        if amount == 0 {
            return Err(GuardianError::InvalidParameter("Katkı sıfır olamaz".into()));
        }
        let mut s = self.state.lock();
        s.balance += amount;
        s.contributions += 1;
        Ok(s.balance)
    }

    /// Tazminat ödemesi (claim onaylandıktan sonra)
    pub fn payout(&self, amount: u128) -> GuardianResult<u128> {
        let mut s = self.state.lock();
        if amount > s.balance {
            return Err(GuardianError::InsufficientInsuranceFund {
                available: s.balance,
                required: amount,
            });
        }
        s.balance -= amount;
        s.paid_out += amount;
        Ok(s.balance)
    }

    /// Mevcut fon bakiyesi
    pub fn balance(&self) -> u128 {
        self.state.lock().balance
    }

    /// Toplam ödenen tazminat
    pub fn total_paid_out(&self) -> u128 {
        self.state.lock().paid_out
    }

    /// Toplam katkı sayısı
    pub fn contribution_count(&self) -> u128 {
        self.state.lock().contributions
    }

    /// Fon yeterli mi?
    pub fn can_cover(&self, amount: u128) -> bool {
        self.balance() >= amount
    }
}

// ─── FundContribution ─────────────────────────────────────────────────────────

/// Guardian sigorta katkı kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundContribution {
    pub guardian: Address,
    pub amount: u128,
    pub contributed_at_slot: u64,
}

impl FundContribution {
    pub fn new(guardian: Address, amount: u128, slot: u64) -> Self {
        Self { guardian, amount, contributed_at_slot: slot }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const NSC: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn test_deposit_increases_balance() {
        let fund = InsuranceFund::new();
        fund.deposit(100 * NSC).unwrap();
        fund.deposit(50 * NSC).unwrap();
        assert_eq!(fund.balance(), 150 * NSC);
        assert_eq!(fund.contribution_count(), 2);
    }

    #[test]
    fn test_zero_deposit_rejected() {
        let fund = InsuranceFund::new();
        assert!(matches!(fund.deposit(0), Err(GuardianError::InvalidParameter(_))));
    }

    #[test]
    fn test_payout_reduces_balance() {
        let fund = InsuranceFund::new();
        fund.deposit(200 * NSC).unwrap();
        fund.payout(50 * NSC).unwrap();
        assert_eq!(fund.balance(), 150 * NSC);
        assert_eq!(fund.total_paid_out(), 50 * NSC);
    }

    #[test]
    fn test_payout_insufficient_funds() {
        let fund = InsuranceFund::new();
        fund.deposit(10 * NSC).unwrap();
        let err = fund.payout(100 * NSC);
        assert!(matches!(err, Err(GuardianError::InsufficientInsuranceFund { .. })));
    }

    #[test]
    fn test_can_cover() {
        let fund = InsuranceFund::new();
        fund.deposit(50 * NSC).unwrap();
        assert!(fund.can_cover(50 * NSC));
        assert!(!fund.can_cover(51 * NSC));
    }

    #[test]
    fn test_multiple_payouts() {
        let fund = InsuranceFund::new();
        fund.deposit(300 * NSC).unwrap();
        fund.payout(100 * NSC).unwrap();
        fund.payout(100 * NSC).unwrap();
        assert_eq!(fund.balance(), 100 * NSC);
        assert_eq!(fund.total_paid_out(), 200 * NSC);
    }
}
