//! Guardian kurtarma geçmişi — başarılı/başarısız kurtarma takibi

use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── RecoveryOutcome ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOutcome {
    /// Başarılı kurtarma
    Success,
    /// Başarısız (zaman aşımı)
    Timeout,
    /// Kullanıcı iptal etti
    CancelledByUser,
    /// Guardian yanıt vermedi
    GuardianUnresponsive,
}

impl RecoveryOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, RecoveryOutcome::Success)
    }
}

// ─── RecoveryRecord ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRecord {
    pub account_id: Address,
    pub outcome: RecoveryOutcome,
    pub response_slots: u64, // Yanıt süresi (slot)
    pub slot: u64,
}

// ─── RecoveryHistory ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RecoveryHistory {
    pub total_recoveries: u32,
    pub successful_recoveries: u32,
    pub records: Vec<RecoveryRecord>,
}

impl RecoveryHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, record: RecoveryRecord) {
        if record.outcome.is_success() {
            self.successful_recoveries += 1;
        }
        self.total_recoveries += 1;
        self.records.push(record);
    }

    /// Başarı oranı (0-100)
    pub fn success_rate(&self) -> u32 {
        if self.total_recoveries == 0 {
            return 70; // Deneyim yok: orta skor
        }
        self.successful_recoveries * 100 / self.total_recoveries
    }

    /// Ortalama yanıt süresi (slot)
    pub fn avg_response_slots(&self) -> u64 {
        if self.records.is_empty() {
            return 0;
        }
        let total: u64 = self.records.iter().map(|r| r.response_slots).sum();
        total / self.records.len() as u64
    }
}

// ─── RecoveryHistoryStore ─────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct RecoveryHistoryStore {
    histories: Arc<DashMap<Address, RecoveryHistory>>,
}

impl RecoveryHistoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &self,
        guardian: Address,
        account: Address,
        outcome: RecoveryOutcome,
        response_slots: u64,
        slot: SlotId,
    ) {
        let record = RecoveryRecord {
            account_id: account,
            outcome,
            response_slots,
            slot: slot.0,
        };
        self.histories.entry(guardian).or_default().add(record);
    }

    pub fn history(&self, guardian: &Address) -> RecoveryHistory {
        self.histories.get(guardian).map(|r| r.clone()).unwrap_or_default()
    }

    pub fn success_rate(&self, guardian: &Address) -> u32 {
        self.history(guardian).success_rate()
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
    fn test_recovery_history_success_rate() {
        let store = RecoveryHistoryStore::new();
        store.record(addr(1), addr(2), RecoveryOutcome::Success, 5_000, SlotId(100));
        store.record(addr(1), addr(3), RecoveryOutcome::Success, 3_000, SlotId(200));
        store.record(addr(1), addr(4), RecoveryOutcome::Timeout, 100_000, SlotId(300));
        assert_eq!(store.success_rate(&addr(1)), 66); // 2/3
    }

    #[test]
    fn test_empty_history_default_score() {
        let store = RecoveryHistoryStore::new();
        assert_eq!(store.success_rate(&addr(99)), 70); // orta skor
    }

    #[test]
    fn test_avg_response_time() {
        let store = RecoveryHistoryStore::new();
        store.record(addr(1), addr(2), RecoveryOutcome::Success, 4_000, SlotId(0));
        store.record(addr(1), addr(3), RecoveryOutcome::Success, 8_000, SlotId(0));
        let hist = store.history(&addr(1));
        assert_eq!(hist.avg_response_slots(), 6_000);
    }
}
