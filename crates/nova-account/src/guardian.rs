//! nova-account — Guardian Yönetimi
//!
//! Whitepaper §5.1.5 — Guardian Seçimi ve Yönetimi
//!
//! ## Araştırma Kaynakları ve Uygulanan Desenler
//!
//! **TSS-Lib** (`tss/party_id.go`):
//!   `SortPartyIDs` → `GuardianManager::assign_indices()` ile sıralama.
//!   `ReSharingParameters` → guardian seti değişikliği yönetimi.
//!   `BaseParty.WaitingFor()` → bekleyen guardian imzaları takibi.
//!
//! **Polkadot-SDK** (staking pallet, slashing):
//!   `SlashDeferDuration` → 7 günlük guardian değişikliği bekleme penceresi.
//!   `OffenceReportSystem` → `GuardianManager::report_misconduct()`.
//!   `ValidatorPrefs {commission, blocked}` → `GuardianPreferences`.
//!   `StakerStatus` (Idle/Validator/Nominator) → `GuardianStatus` durumları.
//!
//! **Avalanchego** (uptime tracking, `vms/platformvm/validators/uptime/`):
//!   Validator uptime hesabı → guardian responsiveness puanlaması.
//!   `UpdateUptime()` → `GuardianManager::record_signature_outcome()`.
//!
//! **Cosmos-SDK** (`x/staking/keeper/`):
//!   Keeper pattern (KV store erişimi) → `GuardianManager` struct.
//!   `Bond/Unbond/Slash` dispatch → `add/remove/penalize_guardian` metodları.
//!
//! **Go-Ethereum** (accounts/manager.go):
//!   `Backend.Subscribe()` → `GuardianManager::notify_change_event()`.
//!   `WalletEvent{Wallet, Kind}` → `GuardianEvent {guardian_id, kind}`.

use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::errors::{AccountError, AccountResult};
use crate::types::{
    AccountId, GuardianCategory, GuardianPartyId, GuardianPreferences, GuardianRecord,
    GuardianSet, GuardianSetChange, GuardianStatus, ThresholdConfig, GUARDIAN_CHANGE_DELAY_SECS,
    MAX_GUARDIANS, MIN_GUARDIANS,
};
use nova_primitives::types::{Address, SlotId};

// ─── GUARDIAN OLAYI ──────────────────────────────────────────────────────────

/// Guardian yönetim olayı — Go-Ethereum WalletEvent benzeri.
/// Üst katmanlara bildirim için kullanılır.
#[derive(Debug, Clone)]
pub enum GuardianEvent {
    /// Yeni guardian eklendi (aktivasyon bekleniyor).
    Added {
        account: AccountId,
        guardian_id: String,
        category: String,
    },
    /// Guardian aktif hale geldi.
    Activated {
        account: AccountId,
        guardian_id: String,
    },
    /// Guardian kaldırma talebi yapıldı.
    RemovalPending {
        account: AccountId,
        guardian_id: String,
        executes_at_slot: SlotId,
    },
    /// Guardian kaldırıldı.
    Removed {
        account: AccountId,
        guardian_id: String,
    },
    /// Guardian kötü davranış tespit edildi.
    MisconductReported {
        account: AccountId,
        guardian_id: String,
        reason: String,
    },
    /// Guardian askıya alındı.
    Suspended {
        account: AccountId,
        guardian_id: String,
        reason: String,
    },
    /// Guardian set değişikliği başlatıldı.
    SetChangePending {
        account: AccountId,
        old_threshold: ThresholdConfig,
        new_threshold: ThresholdConfig,
        executes_at_slot: SlotId,
    },
    /// Guardian set değişikliği yürütüldü.
    SetChangeExecuted {
        account: AccountId,
        new_guardian_count: usize,
    },
}

// ─── MİSCONDUCT RAPORU ───────────────────────────────────────────────────────

/// Guardian kötü davranış raporu — Polkadot OffenceReportSystem benzeri.
#[derive(Debug, Clone)]
pub struct MisconductReport {
    /// Raporlayan (başka guardian veya kullanıcı).
    pub reporter: Address,
    /// Hedef guardian ID'si.
    pub guardian_id: String,
    /// Suç türü.
    pub offence: GuardianOffence,
    /// Kanıt (imza hash'leri, zaman damgaları vb.).
    pub evidence: Vec<u8>,
    /// Raporlanma slot'u.
    pub reported_at_slot: SlotId,
}

/// Guardian suç türleri.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardianOffence {
    /// §5.5.4: Yanlış freeze girişimi.
    FalseFreezeAttempt,
    /// §5.1.4: Guardian'ların birleşerek hesabı ele geçirme girişimi.
    CoordinatedTakeover,
    /// Tekrarlanan signing isteği reddi.
    RepeatedSigningRefusal,
    /// Yetkisiz key share paylaşımı.
    UnauthorizedKeyShareDisclosure,
    /// Genel kötü niyet.
    GeneralMisconduct { detail: String },
}

impl GuardianOffence {
    /// Bu suç ne kadar ağır? (0-100 arası ceza puanı)
    pub fn severity(&self) -> u8 {
        match self {
            Self::FalseFreezeAttempt => 20,           // §5.5.4: %5 stake kesintisi
            Self::CoordinatedTakeover => 100,          // En ağır suç — kalıcı deaktivasyon
            Self::RepeatedSigningRefusal => 40,        // Orta ağırlık
            Self::UnauthorizedKeyShareDisclosure => 80, // Ağır
            Self::GeneralMisconduct { .. } => 10,      // Hafif
        }
    }

    /// Bu suç otomatik deaktivasyon gerektiriyor mu?
    pub fn requires_immediate_deactivation(&self) -> bool {
        matches!(
            self,
            Self::CoordinatedTakeover | Self::UnauthorizedKeyShareDisclosure
        )
    }
}

// ─── GUARDIAN YÖNETİCİSİ ─────────────────────────────────────────────────────

/// Guardian yönetim motoru.
///
/// Cosmos-SDK Keeper pattern: tek sorumluluk, iş mantığı burada,
/// veri erişimi dışarıdan enjekte edilir.
///
/// TSS-Lib'in party management (SortPartyIDs, GenerateTestPartyIDs) yaklaşımından
/// ilham alınarak guardian seti yönetimi merkezi yapıda tasarlandı.
pub struct GuardianManager;

impl GuardianManager {
    // ─── GUARDIAN EKLEME ──────────────────────────────────────────────────

    /// Yeni guardian ekle.
    ///
    /// §5.1.5: Guardian değişikliği 7 günlük bekleme süresi gerektirir.
    /// Polkadot SlashDeferDuration benzeri güvenlik penceresi.
    ///
    /// Yeni guardian hemen aktif olmaz — `PendingActivation` durumunda başlar.
    pub fn add_guardian(
        guardian_set: &mut GuardianSet,
        guardian_id: impl Into<String>,
        moniker: impl Into<String>,
        category: GuardianCategory,
        signing_pubkey: Vec<u8>,
        preferences: Option<GuardianPreferences>,
        current_slot: SlotId,
    ) -> AccountResult<GuardianEvent> {
        let guardian_id = guardian_id.into();
        let moniker = moniker.into();

        // Zaten kayıtlı mı?
        if guardian_set.guardians.contains_key(&guardian_id) {
            return Err(AccountError::GuardianAlreadyRegistered {
                guardian_id: guardian_id.clone(),
            });
        }

        // Maksimum guardian sayısı kontrolü
        let total = guardian_set.guardians.len();
        if total >= MAX_GUARDIANS as usize {
            return Err(AccountError::TooManyGuardians {
                max: MAX_GUARDIANS,
                given: (total + 1) as u8,
            });
        }

        // Public key geçerliliği temel kontrolü
        if signing_pubkey.is_empty() {
            return Err(AccountError::InvalidParameter {
                param: "signing_pubkey".into(),
                reason: "boş olamaz".into(),
            });
        }

        // Guardian kaydı oluştur
        let party_id = GuardianPartyId::new(&guardian_id, &moniker, &signing_pubkey);
        let mut record = GuardianRecord::new(party_id, category.clone(), signing_pubkey, current_slot);

        // Tercih varsa uygula
        if let Some(prefs) = preferences {
            record.preferences = prefs;
        }

        let activate_slot = match &record.status {
            GuardianStatus::PendingActivation { activates_at_slot, .. } => *activates_at_slot,
            _ => unreachable!(),
        };

        guardian_set.guardians.insert(guardian_id.clone(), record);

        // TSS-Lib SortPartyIDs: indeksleri yeniden ata
        Self::assign_indices(guardian_set);

        info!(
            guardian_id = %guardian_id,
            category = %category.category_name(),
            activate_at_slot = activate_slot.0,
            "guardian eklendi, aktivasyon bekleniyor"
        );

        Ok(GuardianEvent::Added {
            account: AccountId::ZERO, // Çağıran katman dolduracak
            guardian_id,
            category: category.category_name().to_string(),
        })
    }

    // ─── GUARDIAN AKTİVASYON ──────────────────────────────────────────────

    /// Bekleme süresi dolan guardian'ları aktifleştir.
    ///
    /// Polkadot-SDK'nın era-based validator activation benzeri.
    /// Her slot işleme sırasında çağrılır (Avalanchego BeginBlock benzeri).
    pub fn process_pending_activations(
        guardian_set: &mut GuardianSet,
        current_slot: SlotId,
    ) -> Vec<GuardianEvent> {
        let mut events = Vec::new();

        for (id, guardian) in guardian_set.guardians.iter_mut() {
            if let GuardianStatus::PendingActivation { activates_at_slot, .. } = &guardian.status {
                if current_slot >= *activates_at_slot {
                    guardian.status = GuardianStatus::Active;
                    info!(guardian_id = %id, slot = current_slot.0, "guardian aktif hale geldi");
                    events.push(GuardianEvent::Activated {
                        account: AccountId::ZERO,
                        guardian_id: id.clone(),
                    });
                }
            }
        }

        events
    }

    // ─── GUARDIAN KALDIRMA ────────────────────────────────────────────────

    /// Guardian kaldırma talebi başlat (7 gün bekleme süresi).
    ///
    /// §5.1.5: "Guardian değişikliği: 7 günlük bekleme süresi uygulanır.
    /// Bu süre içinde eski guardian'lar işlemi iptal edebilir."
    ///
    /// Polkadot unbonding `BondingDuration` benzeri: anında kaldırma yok.
    pub fn initiate_guardian_removal(
        guardian_set: &mut GuardianSet,
        guardian_id: &str,
        current_slot: SlotId,
    ) -> AccountResult<GuardianEvent> {
        let guardian = guardian_set
            .guardians
            .get_mut(guardian_id)
            .ok_or_else(|| AccountError::guardian_not_found(guardian_id))?;

        // Zaten kaldırılmayı bekliyorsa hata
        if matches!(guardian.status, GuardianStatus::PendingRemoval { .. }) {
            return Err(AccountError::GuardianChangePending {
                remaining_seconds: GUARDIAN_CHANGE_DELAY_SECS,
            });
        }

        // Kaldırma sonrası threshold sağlanabilir mi kontrol et
        let active_after = guardian_set.active_count().saturating_sub(1);
        if active_after < guardian_set.threshold.threshold as usize {
            return Err(AccountError::InsufficientSignatures {
                provided: active_after,
                required: guardian_set.threshold.threshold as usize,
            });
        }

        let remove_slot = SlotId(current_slot.0 + guardian_change_delay_in_slots());
        let guardian = guardian_set.guardians.get_mut(guardian_id).unwrap();

        guardian.status = GuardianStatus::PendingRemoval {
            removes_at_slot: remove_slot,
            remaining_secs: GUARDIAN_CHANGE_DELAY_SECS,
        };

        warn!(
            guardian_id = %guardian_id,
            removes_at_slot = remove_slot.0,
            "guardian kaldırma talebi başlatıldı (7 gün bekleme)"
        );

        Ok(GuardianEvent::RemovalPending {
            account: AccountId::ZERO,
            guardian_id: guardian_id.to_string(),
            executes_at_slot: remove_slot,
        })
    }

    /// Beklemedeki guardian kaldırma işlemlerini yürüt.
    pub fn process_pending_removals(
        guardian_set: &mut GuardianSet,
        current_slot: SlotId,
    ) -> Vec<GuardianEvent> {
        let mut to_remove = Vec::new();

        for (id, guardian) in guardian_set.guardians.iter() {
            if let GuardianStatus::PendingRemoval { removes_at_slot, .. } = &guardian.status {
                if current_slot >= *removes_at_slot {
                    to_remove.push(id.clone());
                }
            }
        }

        let mut events = Vec::new();
        for id in to_remove {
            guardian_set.guardians.remove(&id);
            Self::assign_indices(guardian_set);
            info!(guardian_id = %id, "guardian kaldırıldı");
            events.push(GuardianEvent::Removed {
                account: AccountId::ZERO,
                guardian_id: id,
            });
        }

        events
    }

    // ─── GUARDIAN SET DEĞİŞİKLİĞİ ────────────────────────────────────────

    /// Yeni guardian seti değişikliği başlat.
    ///
    /// TSS-Lib ReSharingParameters benzeri: eski komiteden yeni komiteye geçiş.
    /// Threshold imzası gerektirir (whitepaper §5.1.5).
    pub fn initiate_set_change(
        current_set: GuardianSet,
        new_threshold: ThresholdConfig,
        current_slot: SlotId,
        initiated_by: nova_primitives::types::Address,
    ) -> AccountResult<GuardianSetChange> {
        // Yeni threshold geçerliliği
        if !new_threshold.is_valid() {
            return Err(AccountError::invalid_threshold(
                new_threshold.threshold,
                new_threshold.total,
            ));
        }

        let executes_at_slot = SlotId(current_slot.0 + guardian_change_delay_in_slots());
        let old_threshold = current_set.threshold;
        let new_set = GuardianSet::new(new_threshold, current_slot);

        Ok(GuardianSetChange {
            old_set: current_set,
            new_set,
            old_threshold,
            new_threshold,
            initiated_at_slot: current_slot,
            executes_at_slot,
            initiated_by,
        })
    }

    // ─── SUISTIMAL RAPORLAMA ──────────────────────────────────────────────

    /// Guardian kötü davranış rapor et.
    ///
    /// Polkadot-SDK OffenceReportSystem benzeri:
    /// - `publish_evidence`: kanıt yayınla
    /// - `process_evidence`: ceza uygula
    ///
    /// §5.5.4: "Yanlış freeze girişimi → guardian stake'inden kesinti (%5 per yanlış freeze)"
    pub fn report_misconduct(
        guardian_set: &mut GuardianSet,
        report: MisconductReport,
        current_slot: SlotId,
    ) -> AccountResult<GuardianEvent> {
        let guardian = guardian_set
            .guardians
            .get_mut(&report.guardian_id)
            .ok_or_else(|| AccountError::guardian_not_found(&report.guardian_id))?;

        debug!(
            guardian_id = %report.guardian_id,
            severity = report.offence.severity(),
            "guardian suistimal raporu işleniyor"
        );

        // §5.5.4: Yanlış freeze sayacı
        if matches!(report.offence, GuardianOffence::FalseFreezeAttempt) {
            guardian.false_freeze_attempts += 1;
            // §5.5.4: 30 gün içinde 3 yanlış freeze → yetkisi askıya alınır
            if guardian.false_freeze_attempts >= 3 {
                let suspended_until = SlotId(current_slot.0 + 30 * 24 * 3600 * 1000 / 400);
                guardian.status = GuardianStatus::Suspended {
                    reason: "3 yanlış freeze girişimi — §5.5.4".into(),
                    suspended_until_slot: suspended_until,
                };
                warn!(
                    guardian_id = %report.guardian_id,
                    "guardian askıya alındı: 3 yanlış freeze girişimi"
                );
                return Ok(GuardianEvent::Suspended {
                    account: AccountId::ZERO,
                    guardian_id: report.guardian_id.clone(),
                    reason: "3 yanlış freeze girişimi".into(),
                });
            }
        }

        // Ağır suçlar → kalıcı deaktivasyon
        if report.offence.requires_immediate_deactivation() {
            guardian.status = GuardianStatus::Deactivated {
                reason: format!("{:?}", report.offence),
            };
            warn!(
                guardian_id = %report.guardian_id,
                "guardian kalıcı olarak devre dışı bırakıldı"
            );
        }

        Ok(GuardianEvent::MisconductReported {
            account: AccountId::ZERO,
            guardian_id: report.guardian_id,
            reason: format!("{:?}", report.offence),
        })
    }

    // ─── İMZA SONUCU KAYDI ───────────────────────────────────────────────

    /// Guardian imza sonucunu kaydet.
    ///
    /// Avalanchego uptime tracking benzeri: her slot için guardian katılımı izlenir.
    pub fn record_signature_outcome(
        guardian_set: &mut GuardianSet,
        guardian_id: &str,
        success: bool,
        current_slot: SlotId,
    ) -> AccountResult<()> {
        let guardian = guardian_set
            .guardians
            .get_mut(guardian_id)
            .ok_or_else(|| AccountError::guardian_not_found(guardian_id))?;

        if success {
            guardian.signatures_provided += 1;
            guardian.last_active_slot = current_slot;
        } else {
            guardian.missed_signing_requests += 1;
        }

        // Düşük responsiveness → uyarı
        if guardian.responsiveness_bps() < 5_000 && guardian.missed_signing_requests > 10 {
            warn!(
                guardian_id = %guardian_id,
                responsiveness_bps = guardian.responsiveness_bps(),
                "guardian düşük responsiveness"
            );
        }

        Ok(())
    }

    // ─── GUARDIAN SORGULAMA ───────────────────────────────────────────────

    /// Guardian'ın imzalama yetkisi var mı?
    pub fn can_sign(
        guardian_set: &GuardianSet,
        guardian_id: &str,
        current_slot: SlotId,
    ) -> bool {
        match guardian_set.guardians.get(guardian_id) {
            None => false,
            Some(g) => {
                g.status.can_sign() && g.category.is_active_at_slot(current_slot)
            }
        }
    }

    /// Freeze yetkisi olan guardian'lar.
    pub fn freeze_authorized_guardians(guardian_set: &GuardianSet) -> Vec<&GuardianRecord> {
        guardian_set
            .guardians
            .values()
            .filter(|g| g.status.is_active() && g.preferences.can_freeze)
            .collect()
    }

    /// Sosyal kurtarma onaylayabilen guardian'lar.
    pub fn recovery_authorized_guardians(guardian_set: &GuardianSet) -> Vec<&GuardianRecord> {
        guardian_set
            .guardians
            .values()
            .filter(|g| g.status.is_active() && g.preferences.can_approve_recovery)
            .collect()
    }

    /// Guardian seti sağlıklı mı? (Threshold karşılanabiliyor + minimum sağlıklı guardian)
    pub fn is_set_healthy(guardian_set: &GuardianSet) -> bool {
        guardian_set.threshold_reachable()
            && guardian_set
                .guardians
                .values()
                .filter(|g| g.is_healthy())
                .count()
                >= guardian_set.threshold.threshold as usize
    }

    /// Aktif guardian'ları sıralı getir — TSS-Lib SortPartyIDs benzeri.
    pub fn sorted_active_guardians(guardian_set: &GuardianSet) -> Vec<&GuardianRecord> {
        guardian_set.sorted_active()
    }

    /// Guardian tercihlerini güncelle.
    pub fn update_preferences(
        guardian_set: &mut GuardianSet,
        guardian_id: &str,
        preferences: GuardianPreferences,
    ) -> AccountResult<()> {
        let guardian = guardian_set
            .guardians
            .get_mut(guardian_id)
            .ok_or_else(|| AccountError::guardian_not_found(guardian_id))?;

        if !guardian.status.is_active() {
            return Err(AccountError::GuardianNotActive {
                guardian_id: guardian_id.to_string(),
                status: guardian.status.status_name().to_string(),
            });
        }

        guardian.preferences = preferences;
        Ok(())
    }

    // ─── YARDIMCI METODLAR ────────────────────────────────────────────────

    /// TSS-Lib SortPartyIDs benzeri: sort_key'e göre sırala, indeks ata.
    fn assign_indices(guardian_set: &mut GuardianSet) {
        let mut all: Vec<(&String, &mut GuardianRecord)> = guardian_set.guardians.iter_mut().collect();
        all.sort_by(|a, b| a.1.party_id.sort_key.cmp(&b.1.party_id.sort_key));
        for (idx, (_, record)) in all.iter_mut().enumerate() {
            record.party_id.index = idx;
        }
    }

    /// Guardian seti versiyon numarasını artır (reshare benzeri).
    pub fn bump_version(guardian_set: &mut GuardianSet) {
        guardian_set.version += 1;
    }

    /// Guardian sayısı minimum koşulu sağlıyor mu?
    pub fn validate_minimum_size(count: usize) -> AccountResult<()> {
        if count < MIN_GUARDIANS as usize {
            Err(AccountError::InvalidAccountConfig {
                reason: format!("minimum {} guardian gerekli", MIN_GUARDIANS),
            })
        } else {
            Ok(())
        }
    }
}

/// 7 günlük bekleme süresini slot cinsine çevir (400ms/slot varsayımıyla).
fn guardian_change_delay_in_slots() -> u64 {
    GUARDIAN_CHANGE_DELAY_SECS * 1000 / 400
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccountConfig, AccountState, GuardianSet, ThresholdConfig};

    fn test_address() -> Address {
        Address::from_bytes([1u8; 20])
    }

    fn test_slot() -> SlotId {
        SlotId(1000)
    }

    fn make_guardian_set() -> GuardianSet {
        GuardianSet::new(ThresholdConfig::DEFAULT, test_slot())
    }

    fn add_test_guardian(set: &mut GuardianSet, id: &str, active: bool) {
        let pid = GuardianPartyId::new(id, id, id.as_bytes());
        let cat = GuardianCategory::Personal { address: test_address() };
        let mut rec = GuardianRecord::new(pid, cat, id.as_bytes().to_vec(), test_slot());
        if active {
            rec.status = GuardianStatus::Active;
        }
        set.guardians.insert(id.to_string(), rec);
    }

    // ─── Guardian ekleme testleri ──────────────────────────────────────────

    #[test]
    fn guardian_ekleme_beklemede_baslar() {
        let mut set = make_guardian_set();
        let result = GuardianManager::add_guardian(
            &mut set,
            "g1",
            "Guardian 1",
            GuardianCategory::Personal { address: test_address() },
            vec![0x02; 33],
            None,
            test_slot(),
        );
        assert!(result.is_ok());
        assert_eq!(set.guardians.len(), 1);
        let g = set.guardians.get("g1").unwrap();
        assert!(matches!(g.status, GuardianStatus::PendingActivation { .. }));
    }

    #[test]
    fn guardian_ekleme_tekrar_reddedilir() {
        let mut set = make_guardian_set();
        GuardianManager::add_guardian(
            &mut set, "g1", "G1",
            GuardianCategory::Personal { address: test_address() },
            vec![0x02; 33], None, test_slot(),
        ).unwrap();

        let result = GuardianManager::add_guardian(
            &mut set, "g1", "G1 dup",
            GuardianCategory::Personal { address: test_address() },
            vec![0x03; 33], None, test_slot(),
        );
        assert!(matches!(result, Err(AccountError::GuardianAlreadyRegistered { .. })));
    }

    #[test]
    fn guardian_ekleme_bos_pubkey_reddedilir() {
        let mut set = make_guardian_set();
        let result = GuardianManager::add_guardian(
            &mut set, "g1", "G1",
            GuardianCategory::Personal { address: test_address() },
            vec![], // boş pubkey
            None, test_slot(),
        );
        assert!(matches!(result, Err(AccountError::InvalidParameter { .. })));
    }

    // ─── Aktivasyon testleri ───────────────────────────────────────────────

    #[test]
    fn guardian_aktivasyon_suresi_doldugunda_aktif_olur() {
        let mut set = make_guardian_set();
        GuardianManager::add_guardian(
            &mut set, "g1", "G1",
            GuardianCategory::Personal { address: test_address() },
            vec![0x02; 33], None, test_slot(),
        ).unwrap();

        // Bekleme süresi dolmadan
        let events = GuardianManager::process_pending_activations(&mut set, SlotId(1001));
        assert!(events.is_empty());
        assert!(matches!(
            set.guardians.get("g1").unwrap().status,
            GuardianStatus::PendingActivation { .. }
        ));

        // Bekleme süresi dolduktan sonra (7 gün)
        let activate_slot = SlotId(1000 + guardian_change_delay_in_slots());
        let events = GuardianManager::process_pending_activations(&mut set, activate_slot);
        assert_eq!(events.len(), 1);
        assert!(set.guardians.get("g1").unwrap().status.can_sign());
    }

    // ─── Kaldırma testleri ─────────────────────────────────────────────────

    #[test]
    fn guardian_kaldirma_talebi_beklemede_baslar() {
        let mut set = make_guardian_set();
        // Threshold'u karşılayacak kadar aktif guardian ekle
        for i in 0..5 {
            add_test_guardian(&mut set, &format!("g{}", i), true);
        }

        let result = GuardianManager::initiate_guardian_removal(&mut set, "g0", test_slot());
        assert!(result.is_ok());
        assert!(matches!(
            set.guardians.get("g0").unwrap().status,
            GuardianStatus::PendingRemoval { .. }
        ));
    }

    #[test]
    fn guardian_kaldirma_threshold_bozuyorsa_reddedilir() {
        let mut set = make_guardian_set(); // 3/5 threshold
        // Sadece threshold kadar aktif guardian ekle (3 tane)
        for i in 0..3 {
            add_test_guardian(&mut set, &format!("g{}", i), true);
        }

        // Birini kaldırırsak 2 kalır, threshold 3 — geçersiz
        let result = GuardianManager::initiate_guardian_removal(&mut set, "g0", test_slot());
        assert!(matches!(result, Err(AccountError::InsufficientSignatures { .. })));
    }

    #[test]
    fn guardian_kaldirma_suresi_dolunca_kaldirilir() {
        let mut set = make_guardian_set();
        for i in 0..5 {
            add_test_guardian(&mut set, &format!("g{}", i), true);
        }

        GuardianManager::initiate_guardian_removal(&mut set, "g0", test_slot()).unwrap();

        let remove_slot = SlotId(1000 + guardian_change_delay_in_slots());
        let events = GuardianManager::process_pending_removals(&mut set, remove_slot);
        assert_eq!(events.len(), 1);
        assert!(!set.guardians.contains_key("g0"));
    }

    // ─── Suistimal raporlama testleri ──────────────────────────────────────

    #[test]
    fn yanlis_freeze_uc_denemede_askiya_alir() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", true);

        for _ in 0..3 {
            GuardianManager::report_misconduct(
                &mut set,
                MisconductReport {
                    reporter: test_address(),
                    guardian_id: "g1".into(),
                    offence: GuardianOffence::FalseFreezeAttempt,
                    evidence: vec![],
                    reported_at_slot: test_slot(),
                },
                test_slot(),
            ).unwrap();
        }

        let g = set.guardians.get("g1").unwrap();
        assert!(matches!(g.status, GuardianStatus::Suspended { .. }));
        assert!(!g.status.can_sign());
    }

    #[test]
    fn koordineli_devralma_kalici_deaktivasyon() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", true);

        GuardianManager::report_misconduct(
            &mut set,
            MisconductReport {
                reporter: test_address(),
                guardian_id: "g1".into(),
                offence: GuardianOffence::CoordinatedTakeover,
                evidence: vec![],
                reported_at_slot: test_slot(),
            },
            test_slot(),
        ).unwrap();

        let g = set.guardians.get("g1").unwrap();
        assert!(matches!(g.status, GuardianStatus::Deactivated { .. }));
    }

    // ─── İmza sonucu testleri ─────────────────────────────────────────────

    #[test]
    fn imza_sonucu_responsiveness_gunceller() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", true);

        GuardianManager::record_signature_outcome(&mut set, "g1", true, SlotId(1001)).unwrap();
        GuardianManager::record_signature_outcome(&mut set, "g1", true, SlotId(1002)).unwrap();
        GuardianManager::record_signature_outcome(&mut set, "g1", false, SlotId(1003)).unwrap();

        let g = set.guardians.get("g1").unwrap();
        assert_eq!(g.signatures_provided, 2);
        assert_eq!(g.missed_signing_requests, 1);
        assert_eq!(g.responsiveness_bps(), 6_666); // 2/3 ≈ 66.66%
    }

    // ─── İmzalama yetkisi testleri ────────────────────────────────────────

    #[test]
    fn imzalama_yetkisi_aktif_guardian_icin_var() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", true);
        assert!(GuardianManager::can_sign(&set, "g1", test_slot()));
    }

    #[test]
    fn imzalama_yetkisi_bekleyen_guardian_icin_yok() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", false); // pending
        assert!(!GuardianManager::can_sign(&set, "g1", test_slot()));
    }

    #[test]
    fn imzalama_yetkisi_var_olmayan_icin_yok() {
        let set = make_guardian_set();
        assert!(!GuardianManager::can_sign(&set, "yok", test_slot()));
    }

    // ─── Sıralama testleri ────────────────────────────────────────────────

    #[test]
    fn guardian_indeksler_sort_key_sirasina_gore_atanir() {
        let mut set = make_guardian_set();
        // sort_key'leri farklı sırada ekle
        let pid_c = GuardianPartyId::new("g_c", "C", &[0xCC]);
        let pid_a = GuardianPartyId::new("g_a", "A", &[0xAA]);
        let pid_b = GuardianPartyId::new("g_b", "B", &[0xBB]);

        for (pid, id) in [(pid_c, "g_c"), (pid_a, "g_a"), (pid_b, "g_b")] {
            let cat = GuardianCategory::Personal { address: test_address() };
            let mut rec = GuardianRecord::new(pid, cat, vec![], test_slot());
            rec.status = GuardianStatus::Active;
            set.guardians.insert(id.to_string(), rec);
        }

        let sorted = GuardianManager::sorted_active_guardians(&set);
        // [0xAA] < [0xBB] < [0xCC]
        assert_eq!(sorted[0].party_id.moniker, "A");
        assert_eq!(sorted[1].party_id.moniker, "B");
        assert_eq!(sorted[2].party_id.moniker, "C");
    }

    // ─── Set sağlık kontrolü ──────────────────────────────────────────────

    #[test]
    fn set_saglikli_yeterli_guardian_ile() {
        let mut set = GuardianSet::new(ThresholdConfig::new(2, 3).unwrap(), test_slot());
        for i in 0..3 {
            let id = format!("g{}", i);
            let pid = GuardianPartyId::new(&id, &id, id.as_bytes());
            let cat = GuardianCategory::Personal { address: test_address() };
            let mut rec = GuardianRecord::new(pid, cat, vec![], test_slot());
            rec.status = GuardianStatus::Active;
            rec.signatures_provided = 10; // sağlıklı
            set.guardians.insert(id, rec);
        }
        assert!(GuardianManager::is_set_healthy(&set));
    }

    #[test]
    fn set_sagliksiz_yetersiz_guardian_ile() {
        let set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        assert!(!GuardianManager::is_set_healthy(&set));
    }

    // ─── Tercih güncelleme testleri ───────────────────────────────────────

    #[test]
    fn guardian_tercih_aktif_guardian_guncellenir() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", true);

        let prefs = GuardianPreferences {
            can_freeze: false,
            can_approve_recovery: true,
            notification_endpoint_hash: None,
        };
        let result = GuardianManager::update_preferences(&mut set, "g1", prefs);
        assert!(result.is_ok());
        assert!(!set.guardians.get("g1").unwrap().preferences.can_freeze);
    }

    #[test]
    fn guardian_tercih_bekleyen_guardian_reddedilir() {
        let mut set = make_guardian_set();
        add_test_guardian(&mut set, "g1", false); // PendingActivation

        let prefs = GuardianPreferences::default();
        let result = GuardianManager::update_preferences(&mut set, "g1", prefs);
        assert!(matches!(result, Err(AccountError::GuardianNotActive { .. })));
    }

    // ─── Set değişikliği testleri ─────────────────────────────────────────

    #[test]
    fn guardian_set_degisikligi_baslatilir() {
        let set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        let new_threshold = ThresholdConfig::new(3, 4).unwrap();
        let change = GuardianManager::initiate_set_change(
            set,
            new_threshold,
            test_slot(),
            test_address(),
        );
        assert!(change.is_ok());
        let c = change.unwrap();
        assert_eq!(c.new_threshold, new_threshold);
        assert!(c.executes_at_slot.0 > test_slot().0);
    }

    #[test]
    fn guardian_set_degisikligi_gecersiz_threshold_reddedilir() {
        let set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        // 1/5 < 2/3 → geçersiz
        let bad_threshold = ThresholdConfig {
            threshold: 1,
            total: 5,
        };
        let result = GuardianManager::initiate_set_change(
            set,
            bad_threshold,
            test_slot(),
            test_address(),
        );
        assert!(matches!(result, Err(AccountError::InvalidThreshold { .. })));
    }
}
