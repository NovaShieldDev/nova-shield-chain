//! Guardian freeze yetkilendirme ve slashing — §5.5.4
//!
//! Araştırma kaynakları:
//! - Cosmos SDK x/slashing `ValidatorSigningInfo`:
//!   `JailedUntil time.Time`, `MissedBlocksCounter`, kayan window.
//!   Bizde: `GuardianInfractionRecord { jailed_until_slot, window_infraction_slots }`.
//! - Cosmos SDK x/slashing `HandleValidatorSignature()`:
//!   "if height > minHeight && MissedBlocksCounter > maxMissed → slash and jail"
//!   Bizde: "if wrong_freeze_count > WRONG_FREEZE_JAIL_TRIGGER in window → jail".
//! - Cosmos SDK `Keeper.Jail()` + `Keeper.Tombstone()`:
//!   Jailed = geçici, Tombstoned = kalıcı. Bizde sadece geçici jail.
//! - CoW Protocol `BannedUsersConfig { addresses, max_cache_size }`:
//!   Per-hesap yetkili guardian listesi. Bizde: `FreezeAuthorizedGuardians`.

use crate::errors::{FreezeError, FreezeResult};
use crate::types::{
    FreezeEvent, FreezeEventType, FreezeId, GUARDIAN_JAIL_DURATION_SLOTS,
    WRONG_FREEZE_JAIL_TRIGGER, WRONG_FREEZE_SLASH_BPS, WRONG_FREEZE_WINDOW_SLOTS,
};
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── FreezeAuthorizedGuardians ────────────────────────────────────────────────

/// Bir hesap için freeze yetkisi olan guardian'lar.
///
/// Pattern: CoW Protocol `BannedUsersConfig` — explicit allowlist.
/// Whitepaper §5.5.4: "Kullanıcı, freeze yapma yetkisi olan guardian'ları
/// ayrıca tanımlayabilir (tüm guardian'lar değil)."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeAuthorizedGuardians {
    /// Hesap adresi.
    pub account: Address,
    /// Freeze yetkisi olan guardian adresleri.
    pub authorized: Vec<Address>,
    /// Threshold: kaç guardian imzalamalı.
    pub required_threshold: u8,
}

impl FreezeAuthorizedGuardians {
    /// Yeni yetkili guardian konfigürasyonu.
    pub fn new(account: Address, authorized: Vec<Address>, threshold: u8) -> Self {
        Self {
            account,
            authorized,
            required_threshold: threshold,
        }
    }

    /// Bu guardian yetkili mi?
    pub fn is_authorized(&self, guardian: &Address) -> bool {
        self.authorized.contains(guardian)
    }

    /// Verilen imzalayanlar arasında kaç tanesi yetkili?
    pub fn count_authorized(&self, signers: &[Address]) -> u8 {
        signers.iter().filter(|s| self.is_authorized(s)).count() as u8
    }

    /// Threshold karşılandı mı?
    pub fn threshold_met(&self, signers: &[Address]) -> bool {
        self.count_authorized(signers) >= self.required_threshold
    }
}

// ─── GuardianInfractionRecord ─────────────────────────────────────────────────

/// Guardian'ın yanlış freeze ihlal kaydı.
///
/// Pattern: Cosmos SDK slashing `ValidatorSigningInfo`:
/// - `MissedBlocksCounter` → `wrong_freeze_count`
/// - `JailedUntil` → `jailed_until_slot`
/// - Bitmap kayan penceresi → `window_infraction_slots` (30 günlük filtre)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInfractionRecord {
    /// Guardian adresi.
    pub guardian: Address,
    /// Toplam yanlış freeze sayısı (tüm zamanlar).
    pub wrong_freeze_count: u32,
    /// 30 günlük penceredeki yanlış freeze slot'ları.
    /// Pattern: Cosmos SDK bitmap kayan penceresi; burada basitleştirilmiş liste.
    pub window_infraction_slots: Vec<u64>,
    /// Jail bitiş slot'u (None = jailed değil).
    /// Pattern: Cosmos SDK `JailedUntil time.Time`.
    pub jailed_until_slot: Option<SlotId>,
    /// Toplam slash edilen bps miktarı.
    pub total_slashed_bps: u32,
}

impl GuardianInfractionRecord {
    pub fn new(guardian: Address) -> Self {
        Self {
            guardian,
            wrong_freeze_count: 0,
            window_infraction_slots: Vec::new(),
            jailed_until_slot: None,
            total_slashed_bps: 0,
        }
    }

    /// Guardian şu an jailed mi?
    /// Pattern: Cosmos SDK `isJailed, err := k.sk.IsValidatorJailed(ctx, consAddr)`.
    pub fn is_jailed(&self, current_slot: SlotId) -> bool {
        self.jailed_until_slot.map_or(false, |j| current_slot < j)
    }

    /// 30 günlük penceredeki ihlal sayısı.
    /// Pattern: Cosmos SDK sliding window MissedBlocksCounter reset upon unbonding.
    pub fn infractions_in_window(&self, current_slot: SlotId) -> u32 {
        let cutoff = current_slot.0.saturating_sub(WRONG_FREEZE_WINDOW_SLOTS);
        self.window_infraction_slots
            .iter()
            .filter(|&&s| s >= cutoff)
            .count() as u32
    }

    /// Yanlış freeze ihlali ekle.
    /// Eğer penceredeki ihlal sayısı WRONG_FREEZE_JAIL_TRIGGER'a ulaştıysa → jail et.
    ///
    /// Returns: jail event (eğer jail olduysa).
    pub fn add_infraction(&mut self, slot: SlotId) -> Option<JailEvent> {
        self.wrong_freeze_count += 1;
        // Eski pencere dışı girişleri temizle
        let cutoff = slot.0.saturating_sub(WRONG_FREEZE_WINDOW_SLOTS);
        self.window_infraction_slots.retain(|&s| s >= cutoff);
        self.window_infraction_slots.push(slot.0);

        // Slash uygula
        self.total_slashed_bps += WRONG_FREEZE_SLASH_BPS;

        // Jail kontrolü: penceredeki ihlal sayısı eşiğe ulaştı mı?
        let window_count = self.infractions_in_window(slot);
        if window_count >= WRONG_FREEZE_JAIL_TRIGGER && !self.is_jailed(slot) {
            let jailed_until = SlotId(slot.0 + GUARDIAN_JAIL_DURATION_SLOTS);
            self.jailed_until_slot = Some(jailed_until);
            Some(JailEvent {
                guardian: self.guardian,
                jailed_until_slot: jailed_until,
                wrong_freeze_count: window_count,
            })
        } else {
            None
        }
    }
}

/// Guardian jail eventi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JailEvent {
    pub guardian: Address,
    pub jailed_until_slot: SlotId,
    pub wrong_freeze_count: u32,
}

// ─── GuardianFreezeRegistry ───────────────────────────────────────────────────

/// Guardian freeze yetkilendirme ve ihlal kayıtları merkezi.
///
/// Pattern:
/// - Cosmos SDK slashing `Keeper { storeService, sk }` — alt keeper pattern.
/// - Avalanchego `Manager` — concurrent-safe DashMap storage.
pub struct GuardianFreezeRegistry {
    /// Her guardian için ihlal kaydı.
    infraction_records: Arc<DashMap<Address, GuardianInfractionRecord>>,
    /// Her hesap için yetkili guardian konfigürasyonu.
    authorized_per_account: Arc<DashMap<Address, FreezeAuthorizedGuardians>>,
}

impl GuardianFreezeRegistry {
    pub fn new() -> Self {
        Self {
            infraction_records: Arc::new(DashMap::new()),
            authorized_per_account: Arc::new(DashMap::new()),
        }
    }

    // ─── Yapılandırma ────────────────────────────────────────────────────────

    /// Hesap için freeze yetkili guardian listesini ayarla.
    pub fn set_authorized_guardians(&self, config: FreezeAuthorizedGuardians) {
        self.authorized_per_account.insert(config.account, config);
    }

    /// Hesap için yetkili guardian konfigürasyonunu al.
    pub fn get_authorized_guardians(&self, account: &Address) -> Option<FreezeAuthorizedGuardians> {
        self.authorized_per_account.get(account).map(|c| c.clone())
    }

    // ─── Yetkilendirme ───────────────────────────────────────────────────────

    /// Bu guardian, bu hesabı dondurmaya yetkili mi?
    pub fn is_guardian_authorized(&self, account: &Address, guardian: &Address) -> bool {
        self.authorized_per_account
            .get(account)
            .map_or(false, |config| config.is_authorized(guardian))
    }

    /// Guardian şu an jailed mi?
    /// Pattern: Cosmos SDK `isJailed` check — don't update missed blocks when jailed.
    pub fn is_guardian_jailed(&self, guardian: &Address, current_slot: SlotId) -> bool {
        self.infraction_records
            .get(guardian)
            .map_or(false, |r| r.is_jailed(current_slot))
    }

    /// Threshold doğrulaması: signers, hesabın guardian threshold'unu karşılıyor mu?
    pub fn verify_threshold(
        &self,
        account: &Address,
        signers: &[Address],
        current_slot: SlotId,
    ) -> FreezeResult<()> {
        let config = self
            .authorized_per_account
            .get(account)
            .ok_or(FreezeError::NoAuthorizedGuardians)?;

        if config.authorized.is_empty() {
            return Err(FreezeError::NoAuthorizedGuardians);
        }

        // Her imzalayanın yetkili olup olmadığını ve jailed olmadığını kontrol et
        for signer in signers {
            if !config.is_authorized(signer) {
                return Err(FreezeError::GuardianNotAuthorized);
            }
            if self.is_guardian_jailed(signer, current_slot) {
                return Err(FreezeError::GuardianJailed {
                    jailed_until_slot: self
                        .infraction_records
                        .get(signer)
                        .and_then(|r| r.jailed_until_slot)
                        .map(|s| s.0)
                        .unwrap_or(0),
                });
            }
        }

        // Threshold kontrol
        let authorized_count = config.count_authorized(signers);
        if authorized_count < config.required_threshold {
            return Err(FreezeError::ThresholdNotMet {
                required: config.required_threshold,
                got: authorized_count,
            });
        }

        Ok(())
    }

    // ─── Slashing ve Jailing ─────────────────────────────────────────────────

    /// Yanlış freeze bildir — guardian'ı slash et, gerekirse jail et.
    ///
    /// Pattern: Cosmos SDK slashing `HandleValidatorSignature()`:
    /// 1. MissedBlocksCounter++
    /// 2. if counter > maxMissed → slash + jail
    ///
    /// Returns: yayınlanacak event'ler.
    pub fn record_wrong_freeze(
        &self,
        guardian: Address,
        freeze_id: FreezeId,
        slot: SlotId,
    ) -> Vec<FreezeEvent> {
        let mut events = Vec::new();

        let mut record = self
            .infraction_records
            .entry(guardian)
            .or_insert_with(|| GuardianInfractionRecord::new(guardian));

        // Slash eventi
        events.push(FreezeEvent {
            account: guardian,
            event_type: FreezeEventType::GuardianSlashed {
                guardian,
                slash_bps: WRONG_FREEZE_SLASH_BPS,
                reason_freeze_id: freeze_id,
            },
            slot,
        });

        // İhlal ekle, jail kontrol et
        if let Some(jail_event) = record.add_infraction(slot) {
            events.push(FreezeEvent {
                account: guardian,
                event_type: FreezeEventType::GuardianJailed {
                    guardian,
                    jailed_until_slot: jail_event.jailed_until_slot,
                    wrong_freeze_count: jail_event.wrong_freeze_count,
                },
                slot,
            });
        }

        events
    }

    /// Guardian ihlal kaydını al.
    pub fn get_infraction_record(&self, guardian: &Address) -> Option<GuardianInfractionRecord> {
        self.infraction_records.get(guardian).map(|r| r.clone())
    }

    /// Guardian ihlal sayısını al (pencere içi).
    pub fn guardian_window_infractions(&self, guardian: &Address, current_slot: SlotId) -> u32 {
        self.infraction_records
            .get(guardian)
            .map_or(0, |r| r.infractions_in_window(current_slot))
    }
}

impl Default for GuardianFreezeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_freeze_id(seed: u8) -> FreezeId {
        [seed; 32]
    }

    fn make_registry_with_guardians(
        account: Address,
        guardians: Vec<Address>,
        threshold: u8,
    ) -> GuardianFreezeRegistry {
        let registry = GuardianFreezeRegistry::new();
        registry.set_authorized_guardians(FreezeAuthorizedGuardians::new(account, guardians, threshold));
        registry
    }

    // ─── FreezeAuthorizedGuardians ────────────────────────────────────────────

    #[test]
    fn test_authorized_guardians_is_authorized() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let config = FreezeAuthorizedGuardians::new(account, vec![g1, g2], 2);

        assert!(config.is_authorized(&g1));
        assert!(config.is_authorized(&g2));
        assert!(!config.is_authorized(&addr(99)));
    }

    #[test]
    fn test_authorized_guardians_threshold_met() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let g3 = addr(12);
        let config = FreezeAuthorizedGuardians::new(account, vec![g1, g2, g3], 2);

        assert!(config.threshold_met(&[g1, g2]));
        assert!(config.threshold_met(&[g1, g2, g3]));
        assert!(!config.threshold_met(&[g1]));
        assert!(!config.threshold_met(&[addr(99), addr(98)])); // unauthorized
    }

    // ─── GuardianInfractionRecord ────────────────────────────────────────────

    #[test]
    fn test_infraction_record_not_jailed_initially() {
        let record = GuardianInfractionRecord::new(addr(1));
        assert!(!record.is_jailed(SlotId(1_000_000)));
    }

    #[test]
    fn test_infraction_record_jail_after_3_wrong_freezes() {
        let mut record = GuardianInfractionRecord::new(addr(1));
        let slot = SlotId(1_000_000);

        // 1. ihlal → jail yok
        let jail = record.add_infraction(SlotId(slot.0));
        assert!(jail.is_none());

        // 2. ihlal → jail yok
        let jail = record.add_infraction(SlotId(slot.0 + 100));
        assert!(jail.is_none());

        // 3. ihlal → jail!
        let jail = record.add_infraction(SlotId(slot.0 + 200));
        assert!(jail.is_some());

        let jail_event = jail.unwrap();
        assert_eq!(jail_event.wrong_freeze_count, 3);
        assert!(record.is_jailed(SlotId(slot.0 + 300)));
    }

    #[test]
    fn test_infraction_record_jail_duration() {
        let mut record = GuardianInfractionRecord::new(addr(1));
        let base_slot = SlotId(1_000_000);
        for i in 0..3u64 {
            record.add_infraction(SlotId(base_slot.0 + i));
        }
        let jail_until = record.jailed_until_slot.unwrap();
        // Jail süresi = 30 gün = 6_480_000 slot
        assert_eq!(jail_until.0, base_slot.0 + 2 + GUARDIAN_JAIL_DURATION_SLOTS);
    }

    #[test]
    fn test_infraction_record_window_filters_old_infractions() {
        let mut record = GuardianInfractionRecord::new(addr(1));
        // Çok eski ihlaller (30 gün öncesi)
        let old_slot = SlotId(100);
        record.window_infraction_slots.push(old_slot.0);
        record.window_infraction_slots.push(old_slot.0 + 1);

        // Şimdiki slot (30 günden fazla sonra)
        let now = SlotId(100 + WRONG_FREEZE_WINDOW_SLOTS + 1_000);
        let window_count = record.infractions_in_window(now);
        assert_eq!(window_count, 0, "eski ihlaller pencere dışı sayılmamalı");
    }

    #[test]
    fn test_infraction_record_total_slash_accumulates() {
        let mut record = GuardianInfractionRecord::new(addr(1));
        record.add_infraction(SlotId(1_000_000));
        assert_eq!(record.total_slashed_bps, WRONG_FREEZE_SLASH_BPS);
        record.add_infraction(SlotId(1_000_100));
        assert_eq!(record.total_slashed_bps, WRONG_FREEZE_SLASH_BPS * 2);
    }

    // ─── GuardianFreezeRegistry ───────────────────────────────────────────────

    #[test]
    fn test_registry_set_and_get_authorized_guardians() {
        let registry = GuardianFreezeRegistry::new();
        let account = addr(1);
        let g1 = addr(10);
        let config = FreezeAuthorizedGuardians::new(account, vec![g1], 1);
        registry.set_authorized_guardians(config);

        let fetched = registry.get_authorized_guardians(&account).unwrap();
        assert_eq!(fetched.authorized.len(), 1);
        assert!(fetched.is_authorized(&g1));
    }

    #[test]
    fn test_registry_is_guardian_authorized() {
        let account = addr(1);
        let g1 = addr(10);
        let registry = make_registry_with_guardians(account, vec![g1], 1);

        assert!(registry.is_guardian_authorized(&account, &g1));
        assert!(!registry.is_guardian_authorized(&account, &addr(99)));
    }

    #[test]
    fn test_registry_verify_threshold_success() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let registry = make_registry_with_guardians(account, vec![g1, g2], 2);

        let result = registry.verify_threshold(&account, &[g1, g2], SlotId(1000));
        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_verify_threshold_not_met() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let registry = make_registry_with_guardians(account, vec![g1, g2], 2);

        let err = registry.verify_threshold(&account, &[g1], SlotId(1000)).unwrap_err();
        assert!(matches!(err, FreezeError::ThresholdNotMet { required: 2, got: 1 }));
    }

    #[test]
    fn test_registry_verify_threshold_jailed_guardian_rejected() {
        let account = addr(1);
        let g1 = addr(10);
        let g2 = addr(11);
        let registry = make_registry_with_guardians(account, vec![g1, g2], 2);

        // g1'i jail et (3 yanlış freeze)
        let fid = make_freeze_id(1);
        registry.record_wrong_freeze(g1, fid, SlotId(1_000_000));
        registry.record_wrong_freeze(g1, fid, SlotId(1_000_100));
        registry.record_wrong_freeze(g1, fid, SlotId(1_000_200));

        // g1 ve g2 ile threshold denemesi → g1 jailed
        let err = registry.verify_threshold(&account, &[g1, g2], SlotId(1_000_300)).unwrap_err();
        assert!(matches!(err, FreezeError::GuardianJailed { .. }));
    }

    #[test]
    fn test_registry_record_wrong_freeze_emits_slash_event() {
        let registry = GuardianFreezeRegistry::new();
        let guardian = addr(10);
        let fid = make_freeze_id(1);
        let events = registry.record_wrong_freeze(guardian, fid, SlotId(1_000_000));

        assert!(!events.is_empty());
        let has_slash = events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::GuardianSlashed { .. })
        });
        assert!(has_slash, "slash eventi yayınlanmalı");
    }

    #[test]
    fn test_registry_record_wrong_freeze_emits_jail_after_3() {
        let registry = GuardianFreezeRegistry::new();
        let guardian = addr(10);
        let fid = make_freeze_id(1);

        registry.record_wrong_freeze(guardian, fid, SlotId(1_000_000));
        registry.record_wrong_freeze(guardian, fid, SlotId(1_000_100));
        let events = registry.record_wrong_freeze(guardian, fid, SlotId(1_000_200));

        let has_jail = events.iter().any(|e| {
            matches!(e.event_type, FreezeEventType::GuardianJailed { .. })
        });
        assert!(has_jail, "3. yanlış freeze sonrası jail eventi yayınlanmalı");
    }

    #[test]
    fn test_registry_no_authorized_guardians() {
        let registry = GuardianFreezeRegistry::new();
        let account = addr(99); // yapılandırılmamış hesap

        let err = registry.verify_threshold(&account, &[addr(10)], SlotId(0)).unwrap_err();
        assert!(matches!(err, FreezeError::NoAuthorizedGuardians));
    }

    #[test]
    fn test_registry_is_jailed_after_threshold() {
        let registry = GuardianFreezeRegistry::new();
        let guardian = addr(10);
        let fid = make_freeze_id(1);

        for i in 0..3u64 {
            registry.record_wrong_freeze(guardian, fid, SlotId(1_000_000 + i * 100));
        }
        assert!(registry.is_guardian_jailed(&guardian, SlotId(1_000_500)));
        assert!(!registry.is_guardian_jailed(&guardian, SlotId(1_000_000 + 200 + GUARDIAN_JAIL_DURATION_SLOTS + 1)));
    }
}
