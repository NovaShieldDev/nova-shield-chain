//! GÖREV 113: Cosmos-SDK ValidatorByHistoricalConsAddr
//!
//! ## Mimari
//!
//! ### Cosmos SDK Staking Keeper
//! - `SetValidatorByConsAddr(ctx, validator)` → `ValidatorByConsAddr` store → validator.ConsAddr → validator
//! - `GetValidatorByConsAddr(ctx, consAddr)` → eski anahtar dahil arama
//! - Key rotation sonrası eski ConsAddr hâlâ tabloda kalır (slashing delili için)
//! - `SetConsAddrByPubKey(consAddr, pubKey)` → ek çift yönlü eşleme
//!
//! ### Ne İçin Gerekli?
//! - Slashing: kanıt eski ConsAddr içeriyor → o dönemin validator'ı bulunmalı
//! - ABCI BeginBlock: validator proposer ConsAddr üzerinden kimlik tespiti
//! - IBC light client güncelleme: header'daki ConsAddr doğrulaması
//!
//! ### Nova Uyarlaması
//! - `ConsAddr`: validator konsensüs adresi (bls public key hash'i, 20 byte)
//! - `HistoricalConsAddrRegistry`: ConsAddr → ValidatorId ters arama tablosu
//! - `ConsAddrRecord`: bir ConsAddr kaydı (hangi slot'ta aktive/deaktive oldu)

use nova_primitives::{Address, SlotId, ValidatorId};
use std::collections::HashMap;

// ─── ConsAddr ─────────────────────────────────────────────────────────────────

/// Konsensüs adresi — validator'ın BLS/Ed25519 public key hash'i (20 byte)
///
/// Cosmos: `sdk.ConsAddress` ([]byte, 20 byte)
pub type ConsAddr = Address;

// ─── ConsAddrRecord ───────────────────────────────────────────────────────────

/// Bir ConsAddr'ın geçerlilik kaydı
///
/// Cosmos: `ValidatorByConsAddr` KV store entry
#[derive(Debug, Clone)]
pub struct ConsAddrRecord {
    /// Bu ConsAddr'a sahip validator
    pub validator_id: ValidatorId,
    /// ConsAddr aktive olduğu slot
    pub activated_at_slot: SlotId,
    /// ConsAddr deaktive olduğu slot (None = hâlâ aktif)
    pub deactivated_at_slot: Option<SlotId>,
    /// Karşılık gelen public key (doğrulama için)
    pub pub_key: [u8; 32],
}

impl ConsAddrRecord {
    /// Bu kayıt belirtilen slotta aktif miydi?
    pub fn was_active_at(&self, slot: SlotId) -> bool {
        if slot.0 < self.activated_at_slot.0 {
            return false;
        }
        match self.deactivated_at_slot {
            None => true, // hâlâ aktif
            Some(deact) => slot.0 < deact.0, // deaktive olmadan önce
        }
    }

    /// Bu kayıt hâlâ aktif mi?
    pub fn is_currently_active(&self) -> bool {
        self.deactivated_at_slot.is_none()
    }
}

// ─── HistoricalConsAddrRegistry ───────────────────────────────────────────────

/// Tüm geçmiş ve güncel ConsAddr kayıtlarını tutan ters arama tablosu
///
/// Cosmos: `keeper.GetValidatorByConsAddr(ctx, consAddr)`
///
/// Özellikler:
/// - Bir ConsAddr birden fazla slot aralığında farklı validator'lara ait olabilir
///   (konsensüs adresi "recycling" teorik, ama güvenlik için kayıt tutulur)
/// - Slashing delili: eski ConsAddr → o dönemin validator_id'si
/// - O(1) ters arama (HashMap tabanlı)
pub struct HistoricalConsAddrRegistry {
    /// ConsAddr → Kayıt listesi (son aktif önce)
    records: HashMap<ConsAddr, Vec<ConsAddrRecord>>,
    /// ValidatorId → Tüm ConsAddr'ları (ileri yön)
    validator_to_addrs: HashMap<ValidatorId, Vec<ConsAddr>>,
}

impl HistoricalConsAddrRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            validator_to_addrs: HashMap::new(),
        }
    }

    /// Yeni ConsAddr kaydı ekle (validator kayıt veya key rotation)
    ///
    /// Cosmos: `keeper.SetValidatorByConsAddr(ctx, validator)`
    pub fn register(
        &mut self,
        cons_addr: ConsAddr,
        validator_id: ValidatorId,
        pub_key: [u8; 32],
        current_slot: SlotId,
    ) {
        let record = ConsAddrRecord {
            validator_id,
            activated_at_slot: current_slot,
            deactivated_at_slot: None,
            pub_key,
        };

        self.records.entry(cons_addr).or_default().push(record);
        self.validator_to_addrs.entry(validator_id).or_default().push(cons_addr);
    }

    /// Eski ConsAddr'ı deaktive et (key rotation sonrası)
    ///
    /// Cosmos: eski ConsAddr kaydı silinmez, soft-deactivated kalır
    pub fn deactivate(&mut self, cons_addr: &ConsAddr, current_slot: SlotId) -> bool {
        if let Some(records) = self.records.get_mut(cons_addr) {
            for record in records.iter_mut().rev() {
                if record.is_currently_active() {
                    record.deactivated_at_slot = Some(current_slot);
                    return true;
                }
            }
        }
        false
    }

    /// ConsAddr ile validator bul (geçmiş dahil)
    ///
    /// Cosmos: `keeper.GetValidatorByConsAddr(ctx, consAddr)`
    pub fn get_by_cons_addr(&self, cons_addr: &ConsAddr) -> Option<ValidatorId> {
        self.records.get(cons_addr)?
            .iter()
            .rev() // En yeni kayıt önce
            .next()
            .map(|r| r.validator_id)
    }

    /// Belirli bir slotta aktif olan ConsAddr sahibini bul
    ///
    /// Slashing için kritik: "bu slot'ta bu ConsAddr kimin anahtarıydı?"
    pub fn get_at_slot(&self, cons_addr: &ConsAddr, slot: SlotId) -> Option<ValidatorId> {
        let records = self.records.get(cons_addr)?;
        records.iter().rev().find(|r| r.was_active_at(slot)).map(|r| r.validator_id)
    }

    /// Validator'ın tüm ConsAddr'larını getir (güncel + eski)
    pub fn get_all_addrs(&self, validator_id: &ValidatorId) -> Vec<ConsAddr> {
        self.validator_to_addrs.get(validator_id).cloned().unwrap_or_default()
    }

    /// Validator'ın güncel (aktif) ConsAddr'ını getir
    pub fn get_current_addr(&self, validator_id: &ValidatorId) -> Option<ConsAddr> {
        let addrs = self.validator_to_addrs.get(validator_id)?;
        // En son eklenen aktif olanı bul
        for addr in addrs.iter().rev() {
            if let Some(records) = self.records.get(addr) {
                if records.iter().any(|r| r.is_currently_active()) {
                    return Some(*addr);
                }
            }
        }
        None
    }

    /// Kayıtlı benzersiz ConsAddr sayısı
    pub fn cons_addr_count(&self) -> usize {
        self.records.len()
    }

    /// Kayıtlı benzersiz validator sayısı
    pub fn validator_count(&self) -> usize {
        self.validator_to_addrs.len()
    }

    /// Tam kayıt detayını getir
    pub fn records_for(&self, cons_addr: &ConsAddr) -> Option<&Vec<ConsAddrRecord>> {
        self.records.get(cons_addr)
    }
}

impl Default for HistoricalConsAddrRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── ConsAddrDeriver ──────────────────────────────────────────────────────────

/// Public key'den ConsAddr türeten yardımcı
///
/// Cosmos: `sdk.GetConsAddress(pubKey)` → BLAKE3/keccak hash[..20]
pub fn derive_cons_addr(pub_key: &[u8; 32]) -> ConsAddr {
    let hash = blake3::hash(&[b"nova-cons-addr:", pub_key.as_slice()].concat());
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&hash.as_bytes()[..20]);
    ConsAddr::from_bytes(bytes)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(n: u64) -> ValidatorId { ValidatorId(n) }
    fn pk(n: u8) -> [u8; 32] { [n; 32] }
    fn slot(n: u64) -> SlotId { SlotId(n) }

    fn make_cons_addr(n: u8) -> ConsAddr {
        derive_cons_addr(&pk(n))
    }

    // 1. Temel kayıt ve arama
    #[test]
    fn test_basic_register_and_lookup() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let ca = make_cons_addr(1);

        reg.register(ca, vid(1), pk(1), slot(100));

        assert_eq!(reg.get_by_cons_addr(&ca), Some(vid(1)));
    }

    // 2. Kayıt sonrası slotta doğrulama
    #[test]
    fn test_get_at_slot() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let ca = make_cons_addr(1);

        reg.register(ca, vid(1), pk(1), slot(100));

        assert_eq!(reg.get_at_slot(&ca, slot(100)), Some(vid(1)));
        assert_eq!(reg.get_at_slot(&ca, slot(99)), None); // slot öncesi
        assert_eq!(reg.get_at_slot(&ca, slot(200)), Some(vid(1))); // sonrası da aktif
    }

    // 3. Deaktivasyondan sonra slot bazlı arama
    #[test]
    fn test_deactivate_then_query() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let ca = make_cons_addr(1);

        reg.register(ca, vid(1), pk(1), slot(100));
        reg.deactivate(&ca, slot(500));

        // 100-499 arası aktif
        assert_eq!(reg.get_at_slot(&ca, slot(100)), Some(vid(1)));
        assert_eq!(reg.get_at_slot(&ca, slot(499)), Some(vid(1)));
        // 500+ artık aktif değil
        assert_eq!(reg.get_at_slot(&ca, slot(500)), None);
        assert_eq!(reg.get_at_slot(&ca, slot(1000)), None);
    }

    // 4. Key rotation: eski ConsAddr geçmişte, yeni ConsAddr şu an
    #[test]
    fn test_key_rotation_historical_lookup() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let old_ca = make_cons_addr(1);
        let new_ca = make_cons_addr(2);

        // Validator 1: önce eski key
        reg.register(old_ca, vid(1), pk(1), slot(0));
        // Slot 200'de key rotation
        reg.deactivate(&old_ca, slot(200));
        reg.register(new_ca, vid(1), pk(2), slot(200));

        // Eski ConsAddr ile eski slotta bul
        assert_eq!(reg.get_at_slot(&old_ca, slot(100)), Some(vid(1)));
        assert_eq!(reg.get_at_slot(&old_ca, slot(199)), Some(vid(1)));
        assert_eq!(reg.get_at_slot(&old_ca, slot(200)), None);

        // Yeni ConsAddr ile yeni slotta bul
        assert_eq!(reg.get_at_slot(&new_ca, slot(200)), Some(vid(1)));
        assert_eq!(reg.get_at_slot(&new_ca, slot(500)), Some(vid(1)));
    }

    // 5. Bilinmeyen ConsAddr → None
    #[test]
    fn test_unknown_cons_addr() {
        let reg = HistoricalConsAddrRegistry::new();
        let unknown = make_cons_addr(99);
        assert_eq!(reg.get_by_cons_addr(&unknown), None);
        assert_eq!(reg.get_at_slot(&unknown, slot(0)), None);
    }

    // 6. Validator için tüm ConsAddr'lar
    #[test]
    fn test_get_all_addrs() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let ca1 = make_cons_addr(1);
        let ca2 = make_cons_addr(2);

        reg.register(ca1, vid(1), pk(1), slot(0));
        reg.deactivate(&ca1, slot(100));
        reg.register(ca2, vid(1), pk(2), slot(100));

        let addrs = reg.get_all_addrs(&vid(1));
        assert_eq!(addrs.len(), 2);
        assert!(addrs.contains(&ca1));
        assert!(addrs.contains(&ca2));
    }

    // 7. Güncel ConsAddr
    #[test]
    fn test_get_current_addr() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let ca1 = make_cons_addr(1);
        let ca2 = make_cons_addr(2);

        reg.register(ca1, vid(1), pk(1), slot(0));
        reg.deactivate(&ca1, slot(100));
        reg.register(ca2, vid(1), pk(2), slot(100));

        // Güncel ConsAddr = ca2
        assert_eq!(reg.get_current_addr(&vid(1)), Some(ca2));
    }

    // 8. Farklı validator'lar bağımsız
    #[test]
    fn test_multiple_validators() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let ca1 = make_cons_addr(1);
        let ca2 = make_cons_addr(2);

        reg.register(ca1, vid(1), pk(1), slot(0));
        reg.register(ca2, vid(2), pk(2), slot(0));

        assert_eq!(reg.get_by_cons_addr(&ca1), Some(vid(1)));
        assert_eq!(reg.get_by_cons_addr(&ca2), Some(vid(2)));
        assert_eq!(reg.validator_count(), 2);
        assert_eq!(reg.cons_addr_count(), 2);
    }

    // 9. derive_cons_addr deterministik
    #[test]
    fn test_derive_cons_addr_deterministic() {
        let k = pk(42);
        let ca1 = derive_cons_addr(&k);
        let ca2 = derive_cons_addr(&k);
        assert_eq!(ca1, ca2);
    }

    // 10. Farklı public key → farklı ConsAddr
    #[test]
    fn test_different_pk_different_cons_addr() {
        let ca1 = derive_cons_addr(&pk(1));
        let ca2 = derive_cons_addr(&pk(2));
        assert_ne!(ca1, ca2);
    }

    // 11. Slashing senaryosu: eski kanıtla validator bul
    #[test]
    fn test_slashing_with_old_evidence() {
        let mut reg = HistoricalConsAddrRegistry::new();
        let old_ca = make_cons_addr(10);
        let new_ca = make_cons_addr(20);

        // Validator 5, slot 0-999 arası eski key
        reg.register(old_ca, vid(5), pk(10), slot(0));
        reg.deactivate(&old_ca, slot(1000));
        reg.register(new_ca, vid(5), pk(20), slot(1000));

        // Slot 500'deki kanıt → validator 5 sorumlu
        let slashable = reg.get_at_slot(&old_ca, slot(500));
        assert_eq!(slashable, Some(vid(5)));
    }

    // 12. ConsAddrRecord was_active_at
    #[test]
    fn test_cons_addr_record_was_active_at() {
        let record = ConsAddrRecord {
            validator_id: vid(1),
            activated_at_slot: slot(100),
            deactivated_at_slot: Some(slot(500)),
            pub_key: pk(1),
        };

        assert!(!record.was_active_at(slot(99)));
        assert!(record.was_active_at(slot(100)));
        assert!(record.was_active_at(slot(499)));
        assert!(!record.was_active_at(slot(500)));
        assert!(!record.was_active_at(slot(1000)));
    }
}
