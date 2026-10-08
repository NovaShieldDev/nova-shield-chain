//! Sigorta tazminat ödemesi yöneticisi (§5.8.5)
//!
//! Onaylı talepleri InsuranceFund'dan öder ve kaydeder.

use crate::errors::{GuardianError, GuardianResult};
use crate::insurance::claim::{ClaimId, ClaimStore};
use crate::insurance::fund::InsuranceFund;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};

// ─── PayoutRecord ─────────────────────────────────────────────────────────────

/// Tamamlanan tazminat ödeme kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutRecord {
    pub claim_id: ClaimId,
    pub recipient: Address,
    pub amount: u128,
    pub paid_at_slot: u64,
}

// ─── InsurancePayout ──────────────────────────────────────────────────────────

/// Tazminat ödeme yöneticisi
pub struct InsurancePayout<'a> {
    fund: &'a InsuranceFund,
    claim_store: &'a ClaimStore,
}

impl<'a> InsurancePayout<'a> {
    pub fn new(fund: &'a InsuranceFund, claim_store: &'a ClaimStore) -> Self {
        Self { fund, claim_store }
    }

    /// Onaylı talebi öde
    pub fn execute_payout(&self, claim_id: ClaimId, slot: SlotId) -> GuardianResult<PayoutRecord> {
        let claim = self.claim_store.get(claim_id)?;

        // Sadece Approved durumundaki talepler ödenir
        if !claim.is_approved() {
            return Err(GuardianError::InvalidClaimStatus);
        }

        // Zaten ödendi mi?
        if claim.status == crate::insurance::claim::ClaimStatus::Paid {
            return Err(GuardianError::InvalidClaimStatus);
        }

        let amount = claim.approved_amount.ok_or(GuardianError::InvalidClaimStatus)?;

        // Fon yeterliliği kontrolü
        if !self.fund.can_cover(amount) {
            return Err(GuardianError::InsufficientInsuranceFund {
                available: self.fund.balance(),
                required: amount,
            });
        }

        // Fondan düş
        self.fund.payout(amount)?;

        // Talebi Paid olarak işaretle
        self.claim_store.mark_paid(claim_id)?;

        Ok(PayoutRecord {
            claim_id,
            recipient: claim.claimant,
            amount,
            paid_at_slot: slot.0,
        })
    }

    /// Tüm onaylı (henüz ödenmemiş) talepleri toplu öde
    pub fn execute_all_approved(&self, slot: SlotId) -> Vec<GuardianResult<PayoutRecord>> {
        let approved_ids = self.claim_store.approved_unpaid_claim_ids();
        approved_ids.into_iter().map(|id| self.execute_payout(id, slot)).collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insurance::claim::{ClaimReason, ClaimStatus};

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    const NSC: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn test_execute_payout_success() {
        let fund = InsuranceFund::new();
        let store = ClaimStore::new(addr(99));
        fund.deposit(500 * NSC).unwrap();

        let claim_id =
            store.submit_claim(addr(1), addr(2), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));
        store.approve(addr(99), claim_id, 100 * NSC, SlotId(10)).unwrap();

        let payout_mgr = InsurancePayout::new(&fund, &store);
        let record = payout_mgr.execute_payout(claim_id, SlotId(20)).unwrap();

        assert_eq!(record.amount, 100 * NSC);
        assert_eq!(record.recipient, addr(1));
        assert_eq!(fund.balance(), 400 * NSC);

        // Talep artık Paid
        assert_eq!(store.get(claim_id).unwrap().status, ClaimStatus::Paid);
    }

    #[test]
    fn test_payout_pending_claim_fails() {
        let fund = InsuranceFund::new();
        let store = ClaimStore::new(addr(99));
        fund.deposit(500 * NSC).unwrap();

        let claim_id =
            store.submit_claim(addr(1), addr(2), ClaimReason::SlaViolation, 50 * NSC, SlotId(0));

        let payout_mgr = InsurancePayout::new(&fund, &store);
        // Henüz onaylanmadı
        assert!(matches!(
            payout_mgr.execute_payout(claim_id, SlotId(10)),
            Err(GuardianError::InvalidClaimStatus)
        ));
    }

    #[test]
    fn test_payout_insufficient_fund() {
        let fund = InsuranceFund::new();
        let store = ClaimStore::new(addr(99));
        fund.deposit(10 * NSC).unwrap();

        let claim_id =
            store.submit_claim(addr(1), addr(2), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));
        store.approve(addr(99), claim_id, 100 * NSC, SlotId(10)).unwrap();

        let payout_mgr = InsurancePayout::new(&fund, &store);
        assert!(matches!(
            payout_mgr.execute_payout(claim_id, SlotId(20)),
            Err(GuardianError::InsufficientInsuranceFund { .. })
        ));
    }

    #[test]
    fn test_execute_all_approved_pays_all() {
        let fund = InsuranceFund::new();
        let store = ClaimStore::new(addr(99));
        fund.deposit(1000 * NSC).unwrap();

        let id1 = store.submit_claim(addr(1), addr(10), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));
        let id2 = store.submit_claim(addr(2), addr(10), ClaimReason::SlaViolation, 200 * NSC, SlotId(0));
        let _id3 = store.submit_claim(addr(3), addr(10), ClaimReason::GuardianUnresponsive, 50 * NSC, SlotId(0));

        // id1 ve id2 onaylandı, id3 hâlâ pending
        store.approve(addr(99), id1, 100 * NSC, SlotId(5)).unwrap();
        store.approve(addr(99), id2, 200 * NSC, SlotId(5)).unwrap();

        let payout_mgr = InsurancePayout::new(&fund, &store);
        let results = payout_mgr.execute_all_approved(SlotId(10));

        assert_eq!(results.len(), 2, "2 onaylı talep ödenmiş olmalı");
        assert!(results.iter().all(|r| r.is_ok()), "her iki ödeme başarılı olmalı");
        assert_eq!(fund.balance(), 700 * NSC, "fon 300 NSC azalmış olmalı");

        // Paid olarak işaretlendiğini doğrula
        assert_eq!(store.get(id1).unwrap().status, ClaimStatus::Paid);
        assert_eq!(store.get(id2).unwrap().status, ClaimStatus::Paid);
    }

    #[test]
    fn test_execute_all_approved_empty_when_none() {
        let fund = InsuranceFund::new();
        let store = ClaimStore::new(addr(99));
        fund.deposit(500 * NSC).unwrap();

        // Sadece pending talepler var
        store.submit_claim(addr(1), addr(10), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));

        let payout_mgr = InsurancePayout::new(&fund, &store);
        let results = payout_mgr.execute_all_approved(SlotId(10));
        assert!(results.is_empty(), "onaylı talep yoksa boş dizi dönmeli");
    }

    #[test]
    fn test_double_payout_rejected() {
        let fund = InsuranceFund::new();
        let store = ClaimStore::new(addr(99));
        fund.deposit(500 * NSC).unwrap();

        let claim_id =
            store.submit_claim(addr(1), addr(2), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));
        store.approve(addr(99), claim_id, 100 * NSC, SlotId(10)).unwrap();

        let payout_mgr = InsurancePayout::new(&fund, &store);
        payout_mgr.execute_payout(claim_id, SlotId(20)).unwrap();

        // İkinci ödeme reddedilmeli
        assert!(matches!(
            payout_mgr.execute_payout(claim_id, SlotId(30)),
            Err(GuardianError::InvalidClaimStatus)
        ));
    }
}
