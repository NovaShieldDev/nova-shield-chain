//! Sigorta talebi yönetimi (§5.8.5)
//!
//! Başarısız kurtarma sonrası kullanıcı sigorta talebinde bulunur.
//! Talep incelenir, onaylanır veya reddedilir.
//!
//! ## GUARDIAN-001 Güvenlik Düzeltmesi
//!
//! `approve()` ve `reject()` artık `approver: Address` parametresi alır.
//! Yalnızca `ClaimStore::authorized_approver` olarak kayıtlı adres onaylayabilir.

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

/// GUARDIAN-001: ClaimStore artık bir `authorized_approver` tutar.
/// Yalnızca bu adres `approve()`/`reject()` çağırabilir.
#[derive(Debug)]
pub struct ClaimStore {
    claims: Arc<DashMap<ClaimId, InsuranceClaim>>,
    next_id: Arc<AtomicU64>,
    /// Tek yetkili onaylayıcı (sigorta governance modülü adresi).
    authorized_approver: Address,
}

impl Default for ClaimStore {
    fn default() -> Self {
        // Varsayılan onaylayıcı = sıfır adres (production'da mutlaka override edilmeli)
        Self::new(Address::from_bytes([0u8; 20]))
    }
}

impl ClaimStore {
    pub fn new(authorized_approver: Address) -> Self {
        Self {
            claims: Arc::new(DashMap::new()),
            next_id: Arc::new(AtomicU64::new(0)),
            authorized_approver,
        }
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

    /// Talebi onayla — yalnızca `authorized_approver` çağırabilir (GUARDIAN-001)
    pub fn approve(
        &self,
        approver: Address,
        id: ClaimId,
        approved_amount: u128,
        slot: SlotId,
    ) -> GuardianResult<()> {
        // GUARDIAN-001: Yetki kontrolü — yalnızca kayıtlı onaylayıcı
        if approver != self.authorized_approver {
            return Err(GuardianError::Unauthorized);
        }
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

    /// Talebi reddet — yalnızca `authorized_approver` çağırabilir (GUARDIAN-001)
    pub fn reject(&self, approver: Address, id: ClaimId, slot: SlotId) -> GuardianResult<()> {
        // GUARDIAN-001: Yetki kontrolü — yalnızca kayıtlı onaylayıcı
        if approver != self.authorized_approver {
            return Err(GuardianError::Unauthorized);
        }
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

    /// Test için yetkili onaylayıcı (addr 99 = insurance governance)
    fn approver() -> Address { addr(99) }
    fn store() -> ClaimStore { ClaimStore::new(approver()) }

    const NSC: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn test_submit_claim() {
        let store = store();
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
        let store = store();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::GuardianUnresponsive,
            100 * NSC,
            SlotId(0),
        );
        store.approve(approver(), id, 50 * NSC, SlotId(100)).unwrap();
        let claim = store.get(id).unwrap();
        assert_eq!(claim.status, ClaimStatus::Approved);
        assert_eq!(claim.approved_amount, Some(50 * NSC));
    }

    #[test]
    fn test_reject_claim() {
        let store = store();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::SlaViolation,
            50 * NSC,
            SlotId(0),
        );
        store.reject(approver(), id, SlotId(200)).unwrap();
        let claim = store.get(id).unwrap();
        assert_eq!(claim.status, ClaimStatus::Rejected);
    }

    #[test]
    fn test_double_approve_rejected() {
        let store = store();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::WrongRecoveryAttempt,
            100 * NSC,
            SlotId(0),
        );
        store.approve(approver(), id, 75 * NSC, SlotId(100)).unwrap();
        // Zaten onaylandı — hata bekleniyor
        assert!(matches!(
            store.approve(approver(), id, 75 * NSC, SlotId(200)),
            Err(GuardianError::InvalidClaimStatus)
        ));
    }

    #[test]
    fn test_mark_paid() {
        let store = store();
        let id = store.submit_claim(
            addr(1),
            addr(2),
            ClaimReason::RecoveryRefused,
            100 * NSC,
            SlotId(0),
        );
        store.approve(approver(), id, 100 * NSC, SlotId(10)).unwrap();
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
        let store = store();
        let id1 = store.submit_claim(addr(1), addr(10), ClaimReason::RecoveryRefused, 100 * NSC, SlotId(0));
        store.submit_claim(addr(2), addr(10), ClaimReason::SlaViolation, 50 * NSC, SlotId(0));
        store.approve(approver(), id1, 100 * NSC, SlotId(10)).unwrap();

        let pending = store.pending_claims();
        assert_eq!(pending.len(), 1);
    }

    /// GUARDIAN-001: Yetkisiz onaylayıcı reddedilmeli
    #[test]
    fn test_approve_unauthorized_rejected() {
        let store = store();
        let id = store.submit_claim(addr(1), addr(2), ClaimReason::SlaViolation, 50 * NSC, SlotId(0));
        let err = store.approve(addr(42), id, 50 * NSC, SlotId(10));
        assert!(matches!(err, Err(GuardianError::Unauthorized)), "yetkisiz onaylayıcı reddedilmeli");
    }

    /// GUARDIAN-001: Yetkisiz reddetme reddedilmeli
    #[test]
    fn test_reject_unauthorized_rejected() {
        let store = store();
        let id = store.submit_claim(addr(1), addr(2), ClaimReason::SlaViolation, 50 * NSC, SlotId(0));
        let err = store.reject(addr(42), id, SlotId(10));
        assert!(matches!(err, Err(GuardianError::Unauthorized)), "yetkisiz red reddedilmeli");
    }
}
