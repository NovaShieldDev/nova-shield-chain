//! Fee Yakma — §7.3: Base fee'nin %50'si yakılır
//!
//! Whitepaper §7.3:
//!   Her işlemin base fee'sinin %50'si yakılır
//!   Kalan %50 validator'lara dağıtılır

use nova_primitives::Address;
use parking_lot::RwLock;
use std::sync::Arc;

/// Fee yakma oranı: 5_000 BPS = %50
pub const BURN_RATE_BPS: u32 = 5_000;

/// Validator'a giden fee oranı: 5_000 BPS = %50
pub const VALIDATOR_FEE_RATE_BPS: u32 = 5_000;

/// Toplam fee'yi yakma ve validator payına böl
/// Döndürür: (burn_amount, validator_amount)
pub fn split_base_fee(total_fee: u128) -> (u128, u128) {
    let burn = total_fee * BURN_RATE_BPS as u128 / 10_000;
    let validator = total_fee - burn;
    (burn, validator)
}

/// BurnTracker — yakılan toplam NSC'yi takip eder
#[derive(Debug, Clone)]
pub struct BurnTracker {
    total_burned: Arc<RwLock<u128>>,
    burn_events: Arc<RwLock<Vec<BurnEvent>>>,
}

#[derive(Debug, Clone)]
pub struct BurnEvent {
    pub slot: u64,
    pub amount: u128,
    pub tx_sender: Address,
}

impl Default for BurnTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl BurnTracker {
    pub fn new() -> Self {
        Self {
            total_burned: Arc::new(RwLock::new(0)),
            burn_events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Base fee'nin yarısını yak ve kaydı tut
    pub fn record_burn(&self, base_fee: u128, tx_sender: Address, slot: u64) -> u128 {
        let (burn_amount, _) = split_base_fee(base_fee);
        *self.total_burned.write() += burn_amount;
        self.burn_events.write().push(BurnEvent {
            slot,
            amount: burn_amount,
            tx_sender,
        });
        burn_amount
    }

    pub fn total_burned(&self) -> u128 {
        *self.total_burned.read()
    }

    pub fn burn_event_count(&self) -> usize {
        self.burn_events.read().len()
    }

    /// Belirli bir slot aralığındaki yakma olayları
    pub fn events_in_range(&self, from_slot: u64, to_slot: u64) -> Vec<BurnEvent> {
        self.burn_events
            .read()
            .iter()
            .filter(|e| e.slot >= from_slot && e.slot <= to_slot)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_split_base_fee_50_50() {
        let (burn, validator) = split_base_fee(1_000);
        assert_eq!(burn, 500);
        assert_eq!(validator, 500);
        assert_eq!(burn + validator, 1_000);
    }

    #[test]
    fn test_split_base_fee_odd() {
        let (burn, validator) = split_base_fee(1_001);
        assert_eq!(burn, 500);   // floor
        assert_eq!(validator, 501);
        assert_eq!(burn + validator, 1_001);
    }

    #[test]
    fn test_split_base_fee_zero() {
        let (burn, validator) = split_base_fee(0);
        assert_eq!(burn, 0);
        assert_eq!(validator, 0);
    }

    #[test]
    fn test_burn_tracker_record() {
        let tracker = BurnTracker::new();
        let burned = tracker.record_burn(2_000, addr(1), 100);
        assert_eq!(burned, 1_000);
        assert_eq!(tracker.total_burned(), 1_000);
        assert_eq!(tracker.burn_event_count(), 1);
    }

    #[test]
    fn test_burn_tracker_accumulates() {
        let tracker = BurnTracker::new();
        tracker.record_burn(1_000, addr(1), 1);
        tracker.record_burn(2_000, addr(2), 2);
        assert_eq!(tracker.total_burned(), 1_500);
        assert_eq!(tracker.burn_event_count(), 2);
    }

    #[test]
    fn test_burn_tracker_events_in_range() {
        let tracker = BurnTracker::new();
        tracker.record_burn(1_000, addr(1), 10);
        tracker.record_burn(1_000, addr(2), 20);
        tracker.record_burn(1_000, addr(3), 30);

        let events = tracker.events_in_range(10, 20);
        assert_eq!(events.len(), 2);

        let events2 = tracker.events_in_range(25, 35);
        assert_eq!(events2.len(), 1);
    }

    #[test]
    fn test_burn_rate_constant() {
        assert_eq!(BURN_RATE_BPS + VALIDATOR_FEE_RATE_BPS, 10_000);
    }
}
