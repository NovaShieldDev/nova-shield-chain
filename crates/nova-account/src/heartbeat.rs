//! nova-account — Heartbeat ve Proof of Life Mekanizması
//!
//! Whitepaper §5.4.4 ve §5.4.9 — Hayatta Olduğunu Kanıtlama + İnaktivite Tespiti
//!
//! ## Araştırma Kaynakları ve Uygulanan Desenler
//!
//! **Polkadot-SDK** (Im-Online pallet, `frame/im-online/`):
//!   Im-Online heartbeat: her session'da validator online olduğunu kanıtlar.
//!   `Heartbeat { block_number, session_index, authority_index, validators_len, network_state }`
//!   → Nova'da `HeartbeatSubmission { account, slot, signature, is_duress }`.
//!   Missed heartbeat tracking: `ReceivedHeartbeats` storage → Nova: `HeartbeatState.missed_count`.
//!
//! **Avalanchego** (uptime tracking):
//!   `validator.UptimeCalc.CalculateUptime()` → `HeartbeatProcessor::calculate_activity_rate()`.
//!   `ConnectedTime`, `LastUpdated` → `last_heartbeat_slot`, `last_activity_slot`.
//!
//! **Solana** (`sdk/program/src/vote/state/mod.rs`):
//!   `VoteState.epoch_credits` → rolling window heartbeat history.
//!   `MAX_LOCKOUT_HISTORY = 31` → `HEARTBEAT_HISTORY_SIZE = 1000` (daha uzun pencere).
//!
//! **Go-Ethereum** (accounts/manager.go):
//!   Event subscription model → `HeartbeatProcessor` olay sistemi.
//!
//! **Rolling Shutter** (slots.go):
//!   Slot-based temporal arithmetic → `HeartbeatWindow` slot hesaplamaları.

use tracing::{debug, info, warn};

use crate::errors::{AccountError, AccountResult};
use crate::types::{
    AccountConfig, AccountState, HeartbeatRecord, HEARTBEAT_MIN_INTERVAL_SECS,
    MIN_INACTIVITY_PERIOD_SECS,
};
use nova_primitives::types::{Address, Hash32, SlotId};

// ─── SABITLER ─────────────────────────────────────────────────────────────────

/// Heartbeat geçmişi penceresi boyutu (kayıt sayısı).
/// Solana VoteState.epoch_credits rolling window'dan ilham alındı.
pub const HEARTBEAT_HISTORY_SIZE: usize = 1000;

/// Ön uyarı penceresi: tetikleyici süresi dolmadan 30 gün önce (§5.4.3 Aşama 1).
pub const PRE_WARNING_PERIOD_SECS: u64 = 30 * 24 * 3600;

/// Son uyarı penceresi: tetikleyici süresi dolmadan 7 gün önce (§5.4.3 Aşama 2).
pub const FINAL_WARNING_PERIOD_SECS: u64 = 7 * 24 * 3600;

/// Grace period minimum süresi (§5.4.3 Aşama 3): 7 gün.
pub const GRACE_PERIOD_MIN_SECS: u64 = 7 * 24 * 3600;

/// Grace period maksimum süresi (§5.4.3): 365 gün.
pub const GRACE_PERIOD_MAX_SECS: u64 = 365 * 24 * 3600;

/// Guardian uzatma onayı maksimum kullanım sayısı (§5.4.4 Yöntem 3).
pub const MAX_GUARDIAN_EXTENSION_COUNT: u8 = 3;

/// Slot başına milisaniye (400ms/slot).
pub const MS_PER_SLOT: u64 = 400;

// ─── İNAKTİVİTE DURUMU ───────────────────────────────────────────────────────

/// İnaktivite değerlendirmesi sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityStatus {
    /// Hesap aktif — hiçbir uyarı yok.
    Active,
    /// Ön uyarı — §5.4.3 Aşama 1: tetikleyiciye 30 gün kaldı.
    PreWarning {
        /// Tetikleyici süresi dolmaya kaç saniye kaldı.
        remaining_secs: u64,
    },
    /// Son uyarı — §5.4.3 Aşama 2: tetikleyiciye 7 gün kaldı.
    FinalWarning {
        remaining_secs: u64,
    },
    /// Grace period — §5.4.3 Aşama 3: tetikleyici doldu, devir başlamadı.
    GracePeriod {
        /// Grace period bitiş slot'u.
        grace_ends_at_slot: SlotId,
        /// Guardian uzatma kullanım sayısı.
        guardian_extensions_used: u8,
    },
    /// Miras protokolü tetiklendi — §5.4.3 Aşama 4.
    InheritanceTriggered {
        triggered_at_slot: SlotId,
    },
}

// ─── HEARTBEAT GÖNDERİMİ ─────────────────────────────────────────────────────

/// Heartbeat gönderme talebi.
///
/// Polkadot Im-Online pallet'in Heartbeat struct'ından ilham alındı:
/// `{ block_number, session_index, authority_index, validators_len, network_state }`
/// →
/// `{ account, slot, timestamp_secs, signature, is_duress }`
#[derive(Debug, Clone)]
pub struct HeartbeatSubmission {
    /// Gönderen hesap adresi.
    pub account: Address,
    /// Gönderildiği slot (Polkadot: block_number).
    pub slot: SlotId,
    /// Unix timestamp saniye.
    pub timestamp_secs: u64,
    /// MPC threshold imzası (en az threshold kadar guardian'ın onayı).
    /// Ya da tek guardian imzası (birincil cihaz).
    pub signature: Vec<u8>,
    /// Duress heartbeat mi? (§5.4.4 Yöntem 2 — gizli alarm)
    pub is_duress: bool,
    /// İmzalanan mesaj hash'i (signature doğrulama için).
    pub signed_hash: Hash32,
}

impl HeartbeatSubmission {
    /// Yeni heartbeat gönderimi oluştur.
    pub fn new(
        account: Address,
        slot: SlotId,
        timestamp_secs: u64,
        signature: Vec<u8>,
        is_duress: bool,
    ) -> Self {
        let signed_hash = HeartbeatRecord::compute_hash(&account, &slot, timestamp_secs);
        Self {
            account,
            slot,
            timestamp_secs,
            signature,
            is_duress,
            signed_hash,
        }
    }
}

// ─── HEARTBEAT DURUMU ────────────────────────────────────────────────────────

/// Hesabın heartbeat geçmişi ve inaktivite sayacı.
///
/// Polkadot Im-Online'ın `ReceivedHeartbeats` storage ile
/// Avalanchego uptime tracking'in birleştirilmiş versiyonu.
#[derive(Debug, Clone)]
pub struct HeartbeatState {
    /// Hesap adresi.
    pub account: Address,
    /// Son heartbeat slot'u.
    pub last_heartbeat_slot: SlotId,
    /// Son herhangi bir aktivite slot'u (tx, imza, heartbeat).
    pub last_activity_slot: SlotId,
    /// Toplam heartbeat sayısı.
    pub total_heartbeats: u64,
    /// Grace period başlangıç slot'u (varsa).
    pub grace_period_started_at: Option<SlotId>,
    /// Grace period bitiş slot'u (varsa).
    pub grace_period_ends_at: Option<SlotId>,
    /// Guardian uzatma onayı kullanım sayısı (maks 3).
    pub guardian_extensions_used: u8,
    /// Duress heartbeat sayısı (alarm sayacı).
    pub duress_count: u64,
    /// Miras protokolü tetiklendi mi?
    pub inheritance_triggered: bool,
    /// Miras tetiklenme slot'u.
    pub inheritance_triggered_at: Option<SlotId>,
}

impl HeartbeatState {
    /// Yeni heartbeat durumu oluştur (genesis anında).
    pub fn new(account: Address, genesis_slot: SlotId) -> Self {
        Self {
            account,
            last_heartbeat_slot: genesis_slot,
            last_activity_slot: genesis_slot,
            total_heartbeats: 0,
            grace_period_started_at: None,
            grace_period_ends_at: None,
            guardian_extensions_used: 0,
            duress_count: 0,
            inheritance_triggered: false,
            inheritance_triggered_at: None,
        }
    }
}

// ─── HEARTBEAT İŞLEMCİSİ ─────────────────────────────────────────────────────

/// Heartbeat işleme ve inaktivite değerlendirme motoru.
///
/// Polkadot Im-Online pallet offchain worker'ından ilham alınarak
/// her slot/block başında inaktivite değerlendirmesi yapar.
pub struct HeartbeatProcessor;

impl HeartbeatProcessor {
    // ─── HEARTBEAT İŞLEME ────────────────────────────────────────────────

    /// Heartbeat gönderimini işle.
    ///
    /// §5.4.4: "İşlem göndermek, MPC imza üretmek, heartbeat mesajı" inaktiviteyi sıfırlar.
    /// §5.4.4 Yöntem 2: Duress heartbeat → gizli alarm gönder, sayacı sıfırla.
    ///
    /// Polkadot Im-Online: `fn heartbeat(origin, heartbeat, signature)`.
    pub fn process_heartbeat(
        state: &mut AccountState,
        hb_state: &mut HeartbeatState,
        submission: HeartbeatSubmission,
        current_timestamp_secs: u64,
    ) -> AccountResult<HeartbeatProcessResult> {
        // Spam koruması — minimum aralık kontrolü (Polkadot: era-based)
        let secs_since_last = Self::secs_since_slot(
            hb_state.last_heartbeat_slot,
            submission.slot,
        );
        if secs_since_last < HEARTBEAT_MIN_INTERVAL_SECS {
            return Err(AccountError::HeartbeatTooFrequent {
                seconds_since_last: secs_since_last,
                minimum_interval: HEARTBEAT_MIN_INTERVAL_SECS,
            });
        }

        // İmza temel doğrulama (tam MPC doğrulama signing modülünde)
        if submission.signature.is_empty() {
            return Err(AccountError::InvalidHeartbeatSignature {
                address: state.address_hex(),
            });
        }

        // Hash tutarlılığı kontrolü
        let expected_hash = HeartbeatRecord::compute_hash(
            &submission.account,
            &submission.slot,
            submission.timestamp_secs,
        );
        if expected_hash != submission.signed_hash {
            return Err(AccountError::InvalidHeartbeatSignature {
                address: state.address_hex(),
            });
        }

        // Duress heartbeat işleme (§5.4.4 Yöntem 2)
        if submission.is_duress {
            hb_state.duress_count += 1;
            warn!(
                account = %state.address_hex(),
                duress_count = hb_state.duress_count,
                "DURESS HEARTBEAT — gizli alarm tetiklendi! Guardian'lara bildirim gönderilmeli"
            );
            // Duress heartbeat dışarıdan normal görünür ama alarm flag'i set edilir
            // NOT: Return'den önce guardian'lara bildirim gönderilmeli (üst katman sorumluluğu)
            return Ok(HeartbeatProcessResult {
                is_duress: true,
                activity_reset: true,
                inheritance_cancelled: hb_state.grace_period_started_at.is_some(),
                guardian_alarm: true,
                heartbeat_hash: submission.signed_hash,
            });
        }

        // Normal heartbeat — inaktivite sayacını sıfırla
        hb_state.last_heartbeat_slot = submission.slot;
        hb_state.last_activity_slot = submission.slot;
        hb_state.total_heartbeats += 1;

        // AccountState güncelle
        state.last_heartbeat_slot = submission.slot;
        state.last_activity_slot = submission.slot;
        state.heartbeat_count = hb_state.total_heartbeats;

        // Grace period iptal — §5.4.3 Aşama 3: kullanıcı tek imzayla her şeyi durdurabilir
        let inheritance_cancelled = hb_state.grace_period_started_at.is_some();
        if inheritance_cancelled {
            hb_state.grace_period_started_at = None;
            hb_state.grace_period_ends_at = None;
            info!(
                account = %state.address_hex(),
                "grace period heartbeat ile iptal edildi"
            );
        }

        debug!(
            account = %state.address_hex(),
            slot = submission.slot.0,
            heartbeat_count = hb_state.total_heartbeats,
            "heartbeat işlendi"
        );

        Ok(HeartbeatProcessResult {
            is_duress: false,
            activity_reset: true,
            inheritance_cancelled,
            guardian_alarm: false,
            heartbeat_hash: submission.signed_hash,
        })
    }

    // ─── İNAKTİVİTE DEĞERLENDİRMESİ ─────────────────────────────────────

    /// Mevcut aktivite durumunu değerlendir.
    ///
    /// §5.4.3 Çok Aşamalı Uyarı ve Grace Period Sistemi:
    /// Aşama 1 → Aşama 2 → Aşama 3 (grace) → Aşama 4 (miras)
    ///
    /// Polkadot Im-Online'ın session-bazlı missed heartbeat tespiti benzeri,
    /// ancak Nova'da slot-bazlı sürekli değerlendirme yapılır.
    pub fn evaluate_activity(
        config: &AccountConfig,
        hb_state: &HeartbeatState,
        current_slot: SlotId,
    ) -> ActivityStatus {
        if hb_state.inheritance_triggered {
            return ActivityStatus::InheritanceTriggered {
                triggered_at_slot: hb_state.inheritance_triggered_at.unwrap_or(current_slot),
            };
        }

        // Grace period içinde mi?
        if let Some(grace_ends) = hb_state.grace_period_ends_at {
            if current_slot < grace_ends {
                return ActivityStatus::GracePeriod {
                    grace_ends_at_slot: grace_ends,
                    guardian_extensions_used: hb_state.guardian_extensions_used,
                };
            } else {
                // Grace period bitti → miras tetiklenmeli
                return ActivityStatus::InheritanceTriggered {
                    triggered_at_slot: grace_ends,
                };
            }
        }

        // İnaktivite hesapla
        let inactive_secs = Self::secs_since_slot(hb_state.last_activity_slot, current_slot);
        let inactivity_threshold = config.inactivity_period_secs;

        if inactive_secs >= inactivity_threshold {
            // Tetikleyici süresi doldu — Grace period başlatılmalı
            return ActivityStatus::GracePeriod {
                grace_ends_at_slot: SlotId(
                    current_slot.0 + GRACE_PERIOD_MIN_SECS * 1000 / MS_PER_SLOT,
                ),
                guardian_extensions_used: 0,
            };
        }

        let remaining = inactivity_threshold - inactive_secs;

        if remaining <= FINAL_WARNING_PERIOD_SECS {
            return ActivityStatus::FinalWarning {
                remaining_secs: remaining,
            };
        }

        if remaining <= PRE_WARNING_PERIOD_SECS {
            return ActivityStatus::PreWarning {
                remaining_secs: remaining,
            };
        }

        ActivityStatus::Active
    }

    // ─── GRACE PERIOD YÖNETİMİ ───────────────────────────────────────────

    /// Grace period başlat — §5.4.3 Aşama 3.
    pub fn start_grace_period(
        hb_state: &mut HeartbeatState,
        config: &AccountConfig,
        current_slot: SlotId,
    ) -> AccountResult<SlotId> {
        let grace_duration_secs = config
            .inactivity_period_secs
            .min(GRACE_PERIOD_MAX_SECS)
            .max(GRACE_PERIOD_MIN_SECS);
        let grace_ends_slot = SlotId(current_slot.0 + grace_duration_secs * 1000 / MS_PER_SLOT);

        hb_state.grace_period_started_at = Some(current_slot);
        hb_state.grace_period_ends_at = Some(grace_ends_slot);

        info!(
            account = %hb_state.account.as_bytes().iter().map(|b| format!("{:02x}", b)).collect::<String>(),
            grace_ends_slot = grace_ends_slot.0,
            "grace period başlatıldı"
        );

        Ok(grace_ends_slot)
    }

    /// §5.4.4 Yöntem 3: Guardian onayıyla grace period'u uzat.
    /// Maksimum 3 kez kullanılabilir.
    pub fn guardian_extend_grace_period(
        hb_state: &mut HeartbeatState,
        extension_secs: u64,
        current_slot: SlotId,
    ) -> AccountResult<SlotId> {
        if hb_state.guardian_extensions_used >= MAX_GUARDIAN_EXTENSION_COUNT {
            return Err(AccountError::InvalidAccountConfig {
                reason: format!(
                    "guardian uzatma hakkı tükendi: maksimum {} kez",
                    MAX_GUARDIAN_EXTENSION_COUNT
                ),
            });
        }

        if hb_state.grace_period_ends_at.is_none() {
            return Err(AccountError::InternalError {
                reason: "grace period aktif değil, uzatılamaz".into(),
            });
        }

        let new_end = SlotId(
            hb_state
                .grace_period_ends_at
                .unwrap()
                .0
                .saturating_add(extension_secs * 1000 / MS_PER_SLOT),
        );

        hb_state.grace_period_ends_at = Some(new_end);
        hb_state.guardian_extensions_used += 1;

        info!(
            extension_count = hb_state.guardian_extensions_used,
            new_end_slot = new_end.0,
            "guardian grace period uzattı"
        );

        Ok(new_end)
    }

    /// Miras protokolünü tetikle — §5.4.3 Aşama 4.
    pub fn trigger_inheritance(
        hb_state: &mut HeartbeatState,
        current_slot: SlotId,
    ) -> AccountResult<()> {
        if hb_state.inheritance_triggered {
            return Err(AccountError::InternalError {
                reason: "miras protokolü zaten aktif".into(),
            });
        }
        hb_state.inheritance_triggered = true;
        hb_state.inheritance_triggered_at = Some(current_slot);
        warn!(
            slot = current_slot.0,
            "miras protokolü tetiklendi — §5.4.3 Aşama 4"
        );
        Ok(())
    }

    // ─── AKTİVİTE KAYDI ──────────────────────────────────────────────────

    /// Herhangi bir aktiviteyi kaydet (tx, imza, heartbeat).
    /// §5.4.9: tablo — "İşlem göndermek, MPC imza üretmek, Heartbeat mesajı"
    pub fn record_activity(
        state: &mut AccountState,
        hb_state: &mut HeartbeatState,
        activity_slot: SlotId,
    ) {
        hb_state.last_activity_slot = activity_slot;
        state.last_activity_slot = activity_slot;

        // Grace period iptali
        if hb_state.grace_period_started_at.is_some() {
            hb_state.grace_period_started_at = None;
            hb_state.grace_period_ends_at = None;
            info!(
                slot = activity_slot.0,
                "grace period aktivite ile iptal edildi"
            );
        }
    }

    // ─── AKTİVİTE ORANI ──────────────────────────────────────────────────

    /// Aktivite oranı hesapla — Avalanchego uptime benzeri.
    /// Son N slot içinde heartbeat var mı? (Basit heuristik)
    pub fn calculate_activity_rate(
        hb_state: &HeartbeatState,
        config: &AccountConfig,
        current_slot: SlotId,
    ) -> u32 {
        let elapsed_secs = Self::secs_since_slot(hb_state.last_activity_slot, current_slot);
        let threshold = config.inactivity_period_secs;

        if elapsed_secs == 0 {
            return 10_000; // %100 aktif
        }

        if elapsed_secs >= threshold {
            return 0; // %0 — tamamen inaktif
        }

        // Lineer oran: 1 - (elapsed / threshold)
        let inactive_ratio = elapsed_secs * 10_000 / threshold;
        10_000u32.saturating_sub(inactive_ratio as u32)
    }

    // ─── YARDIMCI METODLAR ────────────────────────────────────────────────

    /// İki slot arasındaki saniye farkı (400ms/slot).
    pub fn secs_since_slot(from: SlotId, to: SlotId) -> u64 {
        to.0.saturating_sub(from.0) * MS_PER_SLOT / 1000
    }

    /// Saniyeyi slot farkına çevir.
    pub fn secs_to_slots(secs: u64) -> u64 {
        secs * 1000 / MS_PER_SLOT
    }
}

// ─── HEARTBEAT İŞLEM SONUCU ──────────────────────────────────────────────────

/// Heartbeat işleme sonucu.
#[derive(Debug, Clone)]
pub struct HeartbeatProcessResult {
    /// Bu bir duress heartbeat miydi?
    pub is_duress: bool,
    /// İnaktivite sayacı sıfırlandı mı?
    pub activity_reset: bool,
    /// Miras/grace period iptal edildi mi?
    pub inheritance_cancelled: bool,
    /// Guardian'lara gizli alarm gönderilmeli mi?
    pub guardian_alarm: bool,
    /// Heartbeat hash'i.
    pub heartbeat_hash: Hash32,
}

// ─── InheritanceTriggered için triggered_at alanı düzeltmesi ────────────────

impl ActivityStatus {
    /// Miras tetiklendi mi?
    pub fn is_inheritance_triggered(&self) -> bool {
        matches!(self, Self::InheritanceTriggered { .. })
    }

    /// Grace period'da mı?
    pub fn is_in_grace_period(&self) -> bool {
        matches!(self, Self::GracePeriod { .. })
    }

    /// Uyarı durumunda mı?
    pub fn is_warning(&self) -> bool {
        matches!(self, Self::PreWarning { .. } | Self::FinalWarning { .. })
    }

    /// Durum adı.
    pub fn status_name(&self) -> &'static str {
        match self {
            Self::Active => "aktif",
            Self::PreWarning { .. } => "ön uyarı",
            Self::FinalWarning { .. } => "son uyarı",
            Self::GracePeriod { .. } => "grace period",
            Self::InheritanceTriggered { .. } => "miras tetiklendi",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccountConfig, AccountState, GuardianSet, ThresholdConfig, AccountId};
    use nova_primitives::types::{Address, SlotId};

    fn test_address() -> Address {
        Address::from_bytes([1u8; 20])
    }

    fn test_slot() -> SlotId {
        SlotId(1_000_000) // 1 milyon slot başlangıç
    }

    fn make_account_state() -> AccountState {
        let config = AccountConfig::with_slot(test_slot());
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        AccountState::genesis(
            AccountId::new(1),
            test_address(),
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        )
    }

    fn make_heartbeat_state() -> HeartbeatState {
        HeartbeatState::new(test_address(), test_slot())
    }

    fn make_heartbeat(slot: SlotId, is_duress: bool) -> HeartbeatSubmission {
        let addr = test_address();
        let ts = slot.0 * MS_PER_SLOT / 1000 + 1_700_000_000;
        let signed_hash = HeartbeatRecord::compute_hash(&addr, &slot, ts);
        HeartbeatSubmission {
            account: addr,
            slot,
            timestamp_secs: ts,
            signature: vec![0xFF; 64], // sahte ama boş olmayan
            is_duress,
            signed_hash,
        }
    }

    // ─── HeartbeatProcessor testleri ──────────────────────────────────────

    #[test]
    fn heartbeat_normal_islenir() {
        let mut state = make_account_state();
        let mut hb_state = make_heartbeat_state();
        let slot = SlotId(test_slot().0 + HEARTBEAT_MIN_INTERVAL_SECS * 1000 / MS_PER_SLOT + 1);
        let hb = make_heartbeat(slot, false);
        let result = HeartbeatProcessor::process_heartbeat(
            &mut state, &mut hb_state, hb, 1_700_000_000,
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(!r.is_duress);
        assert!(r.activity_reset);
        assert!(!r.guardian_alarm);
        assert_eq!(hb_state.total_heartbeats, 1);
    }

    #[test]
    fn heartbeat_cok_sik_reddedilir() {
        let mut state = make_account_state();
        let mut hb_state = make_heartbeat_state();
        // Hemen arkasına gönder (0 saniye sonra)
        let slot = SlotId(test_slot().0 + 1);
        let hb = make_heartbeat(slot, false);
        let result = HeartbeatProcessor::process_heartbeat(
            &mut state, &mut hb_state, hb, 1_700_000_000,
        );
        assert!(matches!(result, Err(AccountError::HeartbeatTooFrequent { .. })));
    }

    #[test]
    fn heartbeat_bos_imza_reddedilir() {
        let mut state = make_account_state();
        let mut hb_state = make_heartbeat_state();
        let slot = SlotId(test_slot().0 + HEARTBEAT_MIN_INTERVAL_SECS * 1000 / MS_PER_SLOT + 1);
        let addr = test_address();
        let ts = slot.0 * MS_PER_SLOT / 1000 + 1_700_000_000;
        let signed_hash = HeartbeatRecord::compute_hash(&addr, &slot, ts);
        let hb = HeartbeatSubmission {
            account: addr,
            slot,
            timestamp_secs: ts,
            signature: vec![], // BOS
            is_duress: false,
            signed_hash,
        };
        let result = HeartbeatProcessor::process_heartbeat(
            &mut state, &mut hb_state, hb, ts,
        );
        assert!(matches!(result, Err(AccountError::InvalidHeartbeatSignature { .. })));
    }

    #[test]
    fn duress_heartbeat_alarm_tetikler() {
        let mut state = make_account_state();
        let mut hb_state = make_heartbeat_state();
        let slot = SlotId(test_slot().0 + HEARTBEAT_MIN_INTERVAL_SECS * 1000 / MS_PER_SLOT + 1);
        let hb = make_heartbeat(slot, true); // is_duress = true
        let result = HeartbeatProcessor::process_heartbeat(
            &mut state, &mut hb_state, hb, 1_700_000_000,
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.is_duress);
        assert!(r.guardian_alarm);
        assert_eq!(hb_state.duress_count, 1);
    }

    #[test]
    fn heartbeat_grace_period_iptal_eder() {
        let mut state = make_account_state();
        let mut hb_state = make_heartbeat_state();
        hb_state.grace_period_started_at = Some(test_slot());
        hb_state.grace_period_ends_at = Some(SlotId(test_slot().0 + 100_000));

        let slot = SlotId(test_slot().0 + HEARTBEAT_MIN_INTERVAL_SECS * 1000 / MS_PER_SLOT + 1);
        let hb = make_heartbeat(slot, false);
        let result = HeartbeatProcessor::process_heartbeat(
            &mut state, &mut hb_state, hb, 1_700_000_000,
        ).unwrap();
        assert!(result.inheritance_cancelled);
        assert!(hb_state.grace_period_started_at.is_none());
        assert!(hb_state.grace_period_ends_at.is_none());
    }

    // ─── evaluate_activity testleri ───────────────────────────────────────

    #[test]
    fn activity_aktif_hesap_aktif_doner() {
        let mut config = AccountConfig::with_slot(test_slot());
        config.inactivity_period_secs = 180 * 24 * 3600;
        let hb_state = make_heartbeat_state();
        // Hemen sonraki slot
        let status = HeartbeatProcessor::evaluate_activity(&config, &hb_state, SlotId(test_slot().0 + 1));
        assert_eq!(status, ActivityStatus::Active);
    }

    #[test]
    fn activity_on_uyari_suresi_tespit_edilir() {
        let mut config = AccountConfig::with_slot(test_slot());
        config.inactivity_period_secs = 180 * 24 * 3600; // 180 gün

        let mut hb_state = make_heartbeat_state();
        // 180 gün - 20 gün = 160 gün inaktif → 20 gün kaldı → ön uyarı
        let elapsed = 160 * 24 * 3600;
        let current = SlotId(test_slot().0 + elapsed * 1000 / MS_PER_SLOT);
        hb_state.last_activity_slot = test_slot();

        let status = HeartbeatProcessor::evaluate_activity(&config, &hb_state, current);
        assert!(matches!(status, ActivityStatus::PreWarning { .. }));
    }

    #[test]
    fn activity_son_uyari_suresi_tespit_edilir() {
        let mut config = AccountConfig::with_slot(test_slot());
        config.inactivity_period_secs = 180 * 24 * 3600;

        let mut hb_state = make_heartbeat_state();
        // 180 - 5 = 175 gün inaktif → 5 gün kaldı → son uyarı
        let elapsed = 175 * 24 * 3600;
        let current = SlotId(test_slot().0 + elapsed * 1000 / MS_PER_SLOT);
        hb_state.last_activity_slot = test_slot();

        let status = HeartbeatProcessor::evaluate_activity(&config, &hb_state, current);
        assert!(matches!(status, ActivityStatus::FinalWarning { .. }));
    }

    #[test]
    fn activity_grace_period_tespit_edilir() {
        let mut config = AccountConfig::with_slot(test_slot());
        config.inactivity_period_secs = 30 * 24 * 3600;

        let mut hb_state = make_heartbeat_state();
        // 30 gün tamamen inaktif → grace period başlar
        let elapsed = 30 * 24 * 3600;
        let current = SlotId(test_slot().0 + elapsed * 1000 / MS_PER_SLOT);
        hb_state.last_activity_slot = test_slot();

        let status = HeartbeatProcessor::evaluate_activity(&config, &hb_state, current);
        assert!(matches!(status, ActivityStatus::GracePeriod { .. }));
    }

    // ─── Grace period yönetimi testleri ──────────────────────────────────

    #[test]
    fn grace_period_baslatilir() {
        let config = AccountConfig::with_slot(test_slot());
        let mut hb_state = make_heartbeat_state();
        let grace_end = HeartbeatProcessor::start_grace_period(
            &mut hb_state, &config, test_slot(),
        ).unwrap();
        assert!(grace_end.0 > test_slot().0);
        assert!(hb_state.grace_period_started_at.is_some());
        assert!(hb_state.grace_period_ends_at.is_some());
    }

    #[test]
    fn grace_period_guardian_uzatilir() {
        let config = AccountConfig::with_slot(test_slot());
        let mut hb_state = make_heartbeat_state();
        HeartbeatProcessor::start_grace_period(&mut hb_state, &config, test_slot()).unwrap();
        let initial_end = hb_state.grace_period_ends_at.unwrap();

        HeartbeatProcessor::guardian_extend_grace_period(
            &mut hb_state, 7 * 24 * 3600, test_slot(),
        ).unwrap();
        assert!(hb_state.grace_period_ends_at.unwrap().0 > initial_end.0);
        assert_eq!(hb_state.guardian_extensions_used, 1);
    }

    #[test]
    fn grace_period_max_uzatma_siniri() {
        let config = AccountConfig::with_slot(test_slot());
        let mut hb_state = make_heartbeat_state();
        HeartbeatProcessor::start_grace_period(&mut hb_state, &config, test_slot()).unwrap();

        for _ in 0..MAX_GUARDIAN_EXTENSION_COUNT {
            HeartbeatProcessor::guardian_extend_grace_period(
                &mut hb_state, 7 * 24 * 3600, test_slot(),
            ).unwrap();
        }

        // 4. uzatma başarısız
        let result = HeartbeatProcessor::guardian_extend_grace_period(
            &mut hb_state, 7 * 24 * 3600, test_slot(),
        );
        assert!(result.is_err());
    }

    // ─── Aktivite oranı testleri ──────────────────────────────────────────

    #[test]
    fn aktivite_orani_aktif_hesap_tam_puan() {
        let config = AccountConfig::with_slot(test_slot());
        let hb_state = make_heartbeat_state();
        let rate = HeartbeatProcessor::calculate_activity_rate(
            &hb_state, &config, SlotId(test_slot().0 + 1),
        );
        assert!(rate > 9_900); // Neredeyse %100
    }

    #[test]
    fn aktivite_orani_tamamen_inaktif_sifir() {
        let mut config = AccountConfig::with_slot(test_slot());
        config.inactivity_period_secs = 30 * 24 * 3600;

        let hb_state = make_heartbeat_state();
        let far_future = SlotId(test_slot().0 + 31 * 24 * 3600 * 1000 / MS_PER_SLOT);
        let rate = HeartbeatProcessor::calculate_activity_rate(&hb_state, &config, far_future);
        assert_eq!(rate, 0);
    }

    // ─── Aktivite kaydı testleri ──────────────────────────────────────────

    #[test]
    fn aktivite_kaydi_grace_period_iptal_eder() {
        let mut state = make_account_state();
        let mut hb_state = make_heartbeat_state();
        hb_state.grace_period_started_at = Some(test_slot());
        hb_state.grace_period_ends_at = Some(SlotId(test_slot().0 + 100_000));

        HeartbeatProcessor::record_activity(&mut state, &mut hb_state, SlotId(test_slot().0 + 1));
        assert!(hb_state.grace_period_started_at.is_none());
    }

    // ─── Yardımcı metod testleri ──────────────────────────────────────────

    #[test]
    fn slot_saniye_donusumu_dogru() {
        // 100 slot × 400ms = 40,000ms = 40 saniye
        let secs = HeartbeatProcessor::secs_since_slot(SlotId(0), SlotId(100));
        assert_eq!(secs, 40);
    }

    #[test]
    fn saniye_slot_donusumu_dogru() {
        // 60 saniye × 1000ms / 400ms = 150 slot
        let slots = HeartbeatProcessor::secs_to_slots(60);
        assert_eq!(slots, 150);
    }

    // ─── ActivityStatus metod testleri ───────────────────────────────────

    #[test]
    fn activity_status_siniflandirma_dogru() {
        assert!(!ActivityStatus::Active.is_warning());
        assert!(!ActivityStatus::Active.is_in_grace_period());
        assert!(ActivityStatus::PreWarning { remaining_secs: 100 }.is_warning());
        assert!(ActivityStatus::FinalWarning { remaining_secs: 50 }.is_warning());
        assert!(ActivityStatus::GracePeriod {
            grace_ends_at_slot: SlotId(100),
            guardian_extensions_used: 0,
        }.is_in_grace_period());
        assert!(ActivityStatus::InheritanceTriggered {
            triggered_at_slot: SlotId(100),
        }.is_inheritance_triggered());
    }

    #[test]
    fn miras_tetiklenme_islenir() {
        let mut hb_state = make_heartbeat_state();
        HeartbeatProcessor::trigger_inheritance(&mut hb_state, test_slot()).unwrap();
        assert!(hb_state.inheritance_triggered);
        assert_eq!(hb_state.inheritance_triggered_at, Some(test_slot()));
    }

    #[test]
    fn miras_tekrar_tetiklenme_hata() {
        let mut hb_state = make_heartbeat_state();
        HeartbeatProcessor::trigger_inheritance(&mut hb_state, test_slot()).unwrap();
        let result = HeartbeatProcessor::trigger_inheritance(&mut hb_state, test_slot());
        assert!(result.is_err());
    }
}
