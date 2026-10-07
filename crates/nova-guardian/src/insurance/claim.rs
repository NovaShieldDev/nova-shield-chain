//! Sigorta talebi yönetimi (§5.8.5)
//!
//! Başarısız kurtarma sonrası kullanıcı sigorta talebinde bulunur.
//! Talep incelenir, onaylanır veya reddedilir.

use crate::errors::{GuardianError, GuardianResult};
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── ClaimId ──────────────────────────────────────────────────────────────────

pub type ClaimId = u64;

// ─── ClaimStatus ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaimStatus {
    /// İncelemede
    Pending,
    /// Onaylandı — ödeme bekleniyor
    Approved,
    /// Reddedildi
    Rejected,
    /// Ödendi
    Paid,
}

// ─── ClaimReason ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaimReason {
    /// Guardian kurtarmayı reddetti
    RecoveryRefused,
    /// Guardian yanıt vermedi
    GuardianUnresponsive,
    /// Yanlış kurtarma girişimi
    WrongRecoveryAttempt,
    /// SLA ihlali
    SlaViolation,
}

impl ClaimReason {
    /// Talep için önerilen tazminat oranı (bps — yıllık ücretin yüzdesi)
    pub fn compensation_bps(&self) -> u64 {
        match self {
            ClaimReason::RecoveryRefused => 10_000,     // %100
            ClaimReason::GuardianUnresponsive => 5_000, // %50
            ClaimReason::WrongRecoveryAttempt => 7_500, // %75
            ClaimReason::SlaViolation => 2_500,         // %25
        }
    }
}

// ─── InsuranceClaim ───────────────────────────────────────────────────────────

/// Sigorta talebi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsuranceClaim {
    pub id: ClaimId,
    pub claimant: Address,
    pub guardian: Address,
    pub reason: ClaimReason,
    pub requested_amount: u128,
    /// Onaylanan tazminat (Approved/Paid durumunda dolu)
    pub approved_amount: Option<u128>,
    pub status: ClaimStatus,
    pub created_at_slot: u64,
    pub resolved_at_slot: Option<u64>,
}

impl InsuranceClaim {
    pub fn is_pending(&self) -> bool {
        self.status == ClaimStatus::Pending
    }

    pub fn is_approved(&self) -> bool {
        matches!(self.status, ClaimStatus::Approved | ClaimStatus::Paid)
    }
}

// ─── ClaimStore ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ClaimStore {
    claims: Arc<DashMap<ClaimId, InsuranceClaim>>,
    next_id: Arc<AtomicU64>,
}

impl ClaimStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni talep oluştur
    pub fn submit_claim(
        &self,
        claimant: Address,
        guardian: Address,
        reason: ClaimReason,
        requested_amount: u128,
        slot: SlotId,
    ) -> ClaimId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let claim = InsuranceClaim {
            id,
            claimant,
            guardian,
            reason,
            requested_amount,
            approved_amount: None,
            status: ClaimStatus::Pending,
            created_at_slot: slot.0,
            resolved_at_slot: None,
        };
        self.claims.insert(id, claim);
        id
    }

    /// Talebi onayla
    pub fn approve(&self, id: ClaimId, approved_amount: u128, slot: SlotId) -> GuardianResult<()> {
        let mut claim =
            self.claims.get_mut(&id).ok_or(GuardianError::ClaimNotFound)?;
        if !claim.is_pending() {
            return Err(GuardianError::InvalidClaimStatus);
        }
        claim.status = ClaimStatus::Approved;
        claim.approved_amount = Some(approved_amount);
        claim.resolved_at_slot = Some(slot.0);
        Ok(())
    }

    /// Talebi reddet
    pub fn reject(&self, id: ClaimId, slot: SlotId) -> GuardianResult<()> {
        let mut claim =
            self.claims.get_mut(&id).ok_or(GuardianError::ClaimNotFound)?;
        if !claim.is_pending() {
            return Err(GuardianError::InvalidClaimStatus);
        }
        claim.status = ClaimStatus::Rejected;
        claim.resolved_at_slot = Some(slot.0);
        Ok(())
    }

    /// Ödeme tamamlandı olarak işaretle
    pub fn mark_paid(&self, id: ClaimId) -> GuardianResult<()> {
        let mut claim =
            self.claims.get_mut(&id).ok_or(GuardianError::ClaimNotFound)?;
        if claim.status != ClaimStatus::Approved {
            return Err(GuardianError::InvalidClaimStatus);
        }
        claim.status = ClaimStatus::Paid;
        Ok(())
    }

    /// Talep al
    pub fn get(&self, id: ClaimId) -> GuardianResult<InsuranceClaim> {
        self.claims.get(&id).map(|c| c.clone()).ok_or(GuardianError::ClaimNotFound)
    }

    /// Kullanıcının talepleri
    pub fn claims_for_user(&self, user: &Address) -> Vec<InsuranceClaim> {
        self.claims.iter().filter(|e| &e.claimant == user).map(|e| e.clone()).collect()
    }

    /// Onay bekleyen talepler
    pub fn pending_claims(&self) -> Vec<InsuranceClaim> {
        self.claims.iter().filter(|e| e.is_pending()).map(|e| e.clone()).collect()
    }

    /// Onaylanmış ama henüz ödenmemiş taleplerin ID listesi
    pub fn approved_unpaid_claim_ids(&self) -> Vec<ClaimId> {
        self.claims
            .iter()
            .filter(|e| e.status == ClaimStatus::Approved)
            .map(|e| e.id)
            .collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    const NSC: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn test_submit_claim() {
        let store = ClaimStore::new();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::RecoveryRefused,
            100 * NSC,
            SlotId(0),
        );
        let claim = store.get(id).unwrap();
        assert_eq!(claim.status, ClaimStatus::Pending);
        assert_eq!(claim.requested_amount, 100 * NSC);
    }

    #[test]
    fn test_approve_claim() {
        let store = ClaimStore::new();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::GuardianUnresponsive,
            100 * NSC,
            SlotId(0),
        );
        store.approve(id, 50 * NSC, SlotId(100)).unwrap();
        let claim = store.get(id).unwrap();
        assert_eq!(claim.status, ClaimStatus::Approved);
        assert_eq!(claim.approved_amount, Some(50 * NSC));
    }

    #[test]
    fn test_reject_claim() {
        let store = ClaimStore::new();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::SlaViolation,
            50 * NSC,
            SlotId(0),
        );
        store.reject(id, SlotId(200)).unwrap();
        let claim = store.get(id).unwrap();
        assert_eq!(claim.status, ClaimStatus::Rejected);
    }

    #[test]
    fn test_double_approve_rejected() {
        let store = ClaimStore::new();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::WrongRecoveryAttempt,
            100 * NSC,
            SlotId(0),
        );
        store.approve(id, 75 * NSC, SlotId(100)).unwrap();
        // Zaten onaylandı — hata bekleniyor
        assert!(matches!(store.approve(id, 75 * NSC, SlotId(200)), Err(GuardianError::InvalidClaimStatus)));
    }

    #[test]
    fn test_mark_paid() {
        let store = ClaimStore::new();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::RecoveryRefused,
            100 * NSC,
            SlotId(0),
        );
        store.approve(id, 100 * NSC, SlotId(10)).unwrap();
        store.mark_paid(id).unwrap();
        let claim = store.get(id).unwrap();
        assert_eq!(claim.status, ClaimStatus::Paid);
    }

    #[test]
    fn test_claim_reason_compensation_bps() {
        assert_eq!(ClaimReason::RecoveryRefused.compensation_bps(), 10_000);
        assert_eq!(ClaimReason::GuardianUnresponsive.compensation_bps(), 5_000);
        assert_eq!(ClaimReason::WrongRecoveryAttempt.compensation_bps(), 7_500);
        assert_eq!(ClaimReason::SlaViolation.compensation_bps(), 2_500);
    }

    #[test]
    fn test_pending_claims_filter() {
        let store = ClaimStore::new();
        let id1 = store.submit_claim(addr(1), addr(10), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));
        store.submit_claim(addr(2), addr(10), ClaimReason::SlaViolation, 50 * NSC, SlotId(0));
        store.approve(id1, 100 * NSC, SlotId(10)).unwrap();

        let pending = store.pending_claims();
        assert_eq!(pending.len(), 1);
    }
}
