//! Hizmet teklifi — kullanıcı ↔ guardian anlaşma süreci

use crate::errors::{GuardianError, GuardianResult};
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Teklif geçerlilik süresi: 1 hafta = 604_800s ÷ 0.4s/slot = 1_512_000 slot
pub const OFFER_EXPIRY_SLOTS: u64 = 1_512_000;

// ─── OfferId ──────────────────────────────────────────────────────────────────

pub type OfferId = u64;

// ─── OfferStatus ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferStatus {
    /// Kullanıcı teklif gönderdi
    Pending,
    /// Guardian kabul etti → sözleşme hazır
    Accepted,
    /// Guardian reddetti
    Rejected,
    /// Süresi doldu
    Expired,
    /// İptal edildi
    Cancelled,
}

// ─── ServiceOffer ─────────────────────────────────────────────────────────────

/// Hizmet teklifi — kullanıcı tarafından oluşturulur
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceOffer {
    pub id: OfferId,
    pub user: Address,
    pub guardian: Address,
    pub status: OfferStatus,
    /// Teklif edilen yıllık ücret (NSC)
    pub proposed_annual_fee: u128,
    /// Teklif geçerlilik süresi (slot)
    pub expires_at_slot: u64,
    pub created_at_slot: u64,
}

impl ServiceOffer {
    pub fn is_expired(&self, current_slot: SlotId) -> bool {
        current_slot.0 >= self.expires_at_slot
    }
}

// ─── OfferStore ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct OfferStore {
    offers: Arc<DashMap<OfferId, ServiceOffer>>,
    next_id: Arc<AtomicU64>,
}

impl OfferStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_offer(
        &self,
        user: Address,
        guardian: Address,
        proposed_annual_fee: u128,
        slot: SlotId,
    ) -> OfferId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let offer = ServiceOffer {
            id,
            user,
            guardian,
            status: OfferStatus::Pending,
            proposed_annual_fee,
            expires_at_slot: slot.0 + OFFER_EXPIRY_SLOTS, // 1 hafta
            created_at_slot: slot.0,
        };
        self.offers.insert(id, offer);
        id
    }

    pub fn accept(&self, id: OfferId) -> GuardianResult<()> {
        self.offers
            .get_mut(&id)
            .ok_or(GuardianError::OfferNotFound)
            .map(|mut o| o.status = OfferStatus::Accepted)
    }

    pub fn reject(&self, id: OfferId) -> GuardianResult<()> {
        self.offers
            .get_mut(&id)
            .ok_or(GuardianError::OfferNotFound)
            .map(|mut o| o.status = OfferStatus::Rejected)
    }

    pub fn get(&self, id: OfferId) -> GuardianResult<ServiceOffer> {
        self.offers.get(&id).map(|o| o.clone()).ok_or(GuardianError::OfferNotFound)
    }

    pub fn pending_offers_for(&self, guardian: &Address) -> Vec<ServiceOffer> {
        self.offers
            .iter()
            .filter(|e| &e.guardian == guardian && e.status == OfferStatus::Pending)
            .map(|e| e.clone())
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

    #[test]
    fn test_create_offer() {
        let store = OfferStore::new();
        let id = store.create_offer(addr(1), addr(2), 50, SlotId(0));
        let offer = store.get(id).unwrap();
        assert_eq!(offer.status, OfferStatus::Pending);
        assert_eq!(offer.user, addr(1));
    }

    #[test]
    fn test_accept_offer() {
        let store = OfferStore::new();
        let id = store.create_offer(addr(1), addr(2), 50, SlotId(0));
        store.accept(id).unwrap();
        assert_eq!(store.get(id).unwrap().status, OfferStatus::Accepted);
    }

    #[test]
    fn test_reject_offer() {
        let store = OfferStore::new();
        let id = store.create_offer(addr(1), addr(2), 50, SlotId(0));
        store.reject(id).unwrap();
        assert_eq!(store.get(id).unwrap().status, OfferStatus::Rejected);
    }

    #[test]
    fn test_offer_not_found() {
        let store = OfferStore::new();
        assert!(matches!(store.get(999), Err(GuardianError::OfferNotFound)));
    }

    #[test]
    fn test_offer_expiry_is_one_week() {
        let store = OfferStore::new();
        let id = store.create_offer(addr(1), addr(2), 50, SlotId(0));
        let offer = store.get(id).unwrap();

        // 1 hafta = 1_512_000 slot (400ms/slot)
        assert_eq!(offer.expires_at_slot, OFFER_EXPIRY_SLOTS);

        // Biraz önce → henüz süresi dolmadı
        assert!(!offer.is_expired(SlotId(OFFER_EXPIRY_SLOTS - 1)));

        // Tam süre → süresi doldu
        assert!(offer.is_expired(SlotId(OFFER_EXPIRY_SLOTS)));
    }

    #[test]
    fn test_pending_offers_for_guardian() {
        let store = OfferStore::new();
        store.create_offer(addr(1), addr(10), 50, SlotId(0));
        store.create_offer(addr(2), addr(10), 100, SlotId(0));
        store.create_offer(addr(3), addr(20), 75, SlotId(0)); // farklı guardian
        let pending = store.pending_offers_for(&addr(10));
        assert_eq!(pending.len(), 2);
    }
}
