//! Slash kaydı — Cosmos SDK x/slashing SigningInfo pattern'inden ilham
//!
//! Her guardian için:
//! - Slash sayacı (kaç kez ceza aldı)
//! - Her slash kaydı (ne zaman, neden, ne kadar)
//! - 30 gün içinde 3 yanlış freeze → otomatik jailing (§5.5)

use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Yanlış freeze başına slash oranı: %5
pub const WRONG_FREEZE_SLASH_BPS: u32 = 500;
/// 30 gün içinde maksimum yanlış freeze (§5.5): 3
pub const MAX_WRONG_FREEZES_30_DAYS: u32 = 3;
/// 30 gün = 30 * 216_000 slot
pub const THIRTY_DAYS_SLOTS: u64 = 30 * 216_000;
/// Jailing süresi: 7 gün
pub const JAIL_DURATION_SLOTS: u64 = 7 * 216_000;

// ─── SlashReason ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashReason {
    /// Yanlış freeze girişimi (§5.5)
    WrongFreezeAttempt,
    /// SLA ihlali
    SlaViolation,
    /// Yanıtsızlık (uzun süre cevap vermeme)
    Unresponsive,
    /// Kötü niyetli davranış (kanıtlı)
    Malicious,
}

impl SlashReason {
    pub fn slash_bps(&self) -> u32 {
        match self {
            SlashReason::WrongFreezeAttempt => WRONG_FREEZE_SLASH_BPS,
            SlashReason::SlaViolation => 200,     // %2
            SlashReason::Unresponsive => 300,      // %3
            SlashReason::Malicious => 10_000,      // %100 — tüm stake kesilir
        }
    }

    pub fn triggers_jail(&self) -> bool {
        matches!(self, SlashReason::Malicious)
    }
}

// ─── SlashEvent ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashEvent {
    pub reason: SlashReason,
    pub slash_bps: u32,
    pub slashed_amount: u128,
    pub slot: u64,
}

// ─── GuardianSlashRecord ──────────────────────────────────────────────────────

/// Bir guardian'ın tüm slash geçmişi — Cosmos SigningInfo benzeri
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuardianSlashRecord {
    pub total_slash_events: u32,
    pub events: Vec<SlashEvent>,
    /// Son 30 gün içindeki yanlış freeze sayısı
    pub recent_wrong_freezes: u32,
    pub last_wrong_freeze_slot: u64,
}

impl GuardianSlashRecord {
    pub fn new() -> Self {
        Self::default()
    }

    /// Slash ekle
    pub fn record(&mut self, event: SlashEvent) {
        if matches!(event.reason, SlashReason::WrongFreezeAttempt) {
            self.recent_wrong_freezes += 1;
            self.last_wrong_freeze_slot = event.slot;
        }
        self.total_slash_events += 1;
        self.events.push(event);
    }

    /// 3 yanlış freeze sonrası otomatik jailing tetiklensin mi?
    pub fn should_auto_jail(&self, current_slot: u64) -> bool {
        // Son 30 günün dışındaki freeze'leri sayma
        if current_slot.saturating_sub(self.last_wrong_freeze_slot) > THIRTY_DAYS_SLOTS {
            return false;
        }
        self.recent_wrong_freezes >= MAX_WRONG_FREEZES_30_DAYS
    }

    /// Slash miktarı hesapla (staked_amount üzerinden)
    pub fn calc_slash_amount(staked: u128, slash_bps: u32) -> u128 {
        staked * slash_bps as u128 / 10_000
    }
}

// ─── SlashRecordStore ─────────────────────────────────────────────────────────

/// Tüm guardian'ların slash kayıtları
#[derive(Debug, Default, Clone)]
pub struct SlashRecordStore {
    records: Arc<DashMap<Address, GuardianSlashRecord>>,
}

impl SlashRecordStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_slash(
        &self,
        guardian: Address,
        reason: SlashReason,
        staked_amount: u128,
        slot: SlotId,
    ) -> SlashEvent {
        let slash_bps = reason.slash_bps();
        let slashed_amount = GuardianSlashRecord::calc_slash_amount(staked_amount, slash_bps);
        let event = SlashEvent { reason, slash_bps, slashed_amount, slot: slot.0 };
        self.records.entry(guardian).or_default().record(event.clone());
        event
    }

    pub fn get_record(&self, guardian: &Address) -> GuardianSlashRecord {
        self.records.get(guardian).map(|r| r.clone()).unwrap_or_default()
    }

    pub fn slash_count(&self, guardian: &Address) -> u32 {
        self.get_record(guardian).total_slash_events
    }

    pub fn should_auto_jail(&self, guardian: &Address, current_slot: SlotId) -> bool {
        self.get_record(guardian).should_auto_jail(current_slot.0)
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
    fn test_slash_amount_calculation() {
        let amount = GuardianSlashRecord::calc_slash_amount(10_000, WRONG_FREEZE_SLASH_BPS);
        assert_eq!(amount, 500); // 5%
    }

    #[test]
    fn test_slash_record_stores_event() {
        let store = SlashRecordStore::new();
        store.record_slash(addr(1), SlashReason::WrongFreezeAttempt, 10_000, SlotId(100));
        assert_eq!(store.slash_count(&addr(1)), 1);
    }

    #[test]
    fn test_auto_jail_trigger() {
        let store = SlashRecordStore::new();
        // 3 yanlış freeze yakın slot'ta
        for _ in 0..3 {
            store.record_slash(addr(1), SlashReason::WrongFreezeAttempt, 10_000, SlotId(100));
        }
        assert!(store.should_auto_jail(&addr(1), SlotId(200)));
    }

    #[test]
    fn test_auto_jail_not_triggered_below_threshold() {
        let store = SlashRecordStore::new();
        for _ in 0..2 {
            store.record_slash(addr(1), SlashReason::WrongFreezeAttempt, 10_000, SlotId(100));
        }
        assert!(!store.should_auto_jail(&addr(1), SlotId(200)));
    }

    #[test]
    fn test_auto_jail_expired_window() {
        let store = SlashRecordStore::new();
        for _ in 0..3 {
            store.record_slash(addr(1), SlashReason::WrongFreezeAttempt, 10_000, SlotId(0));
        }
        // 31 gün sonra → pencere dışında
        assert!(!store.should_auto_jail(&addr(1), SlotId(THIRTY_DAYS_SLOTS + 1)));
    }

    #[test]
    fn test_malicious_full_slash() {
        let slash = SlashReason::Malicious.slash_bps();
        assert_eq!(slash, 10_000); // %100
        let amount = GuardianSlashRecord::calc_slash_amount(1_000_000, slash);
        assert_eq!(amount, 1_000_000);
    }
}
