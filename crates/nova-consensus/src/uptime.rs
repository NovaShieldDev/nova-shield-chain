//! GÖREV 101: Validator Uptime ≥80% Reward Sistemi
//!
//! ## Mimari
//!
//! ### Cosmos SDK x/slashing UptimeTracker
//! - `SigningInfo.MissedBlocksCounter`: son N slot kaçırılan blok sayısı
//! - Bitmap window: hangi slot'larda imza atıldı?
//! - `HandleValidatorSignature`: her blok sonrası kontrol
//! - Threshold: `MinSignedPerWindow = 5000 bps (%50)` → ceza
//!
//! ### Nova ≥80% Ödül Sistemi
//! - Uptime %80'in üzerinde → bonus ödül
//! - Her epoch sonunda hesaplanır
//! - `UptimeRewardMultiplier`: tiered bonus (80%, 90%, 99%)
//! - `ValidatorUptimeRecord`: her validator için sliding window

use nova_primitives::{SlotId, ValidatorId};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use crate::types::{SIGNED_BLOCKS_WINDOW, MIN_SIGNED_PER_WINDOW_BPS};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Uptime ödül eşiği 1: ≥%80 → temel bonus
pub const UPTIME_TIER1_BPS: u64 = 8_000;

/// Uptime ödül eşiği 2: ≥%90 → orta bonus
pub const UPTIME_TIER2_BPS: u64 = 9_000;

/// Uptime ödül eşiği 3: ≥%99 → maksimum bonus
pub const UPTIME_TIER3_BPS: u64 = 9_900;

/// Tier 1 ödül çarpanı (bps olarak ek bonus): %5
pub const REWARD_MULTIPLIER_TIER1_BPS: u64 = 500;

/// Tier 2 ödül çarpanı: %10
pub const REWARD_MULTIPLIER_TIER2_BPS: u64 = 1_000;

/// Tier 3 ödül çarpanı: %20
pub const REWARD_MULTIPLIER_TIER3_BPS: u64 = 2_000;

/// Sliding window boyutu (blok sayısı)
pub const UPTIME_WINDOW: usize = 100;

// ─── UptimeTier ───────────────────────────────────────────────────────────────

/// Uptime ödül kademesi
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UptimeTier {
    /// Eşiğin altında — ödül yok (slash tehlikesi)
    Below,
    /// ≥%80 — temel bonus
    Tier1,
    /// ≥%90 — orta bonus
    Tier2,
    /// ≥%99 — maksimum bonus
    Tier3,
}

impl UptimeTier {
    /// Bu tier için ödül çarpanı (bps olarak ek)
    pub fn reward_bonus_bps(&self) -> u64 {
        match self {
            UptimeTier::Below => 0,
            UptimeTier::Tier1 => REWARD_MULTIPLIER_TIER1_BPS,
            UptimeTier::Tier2 => REWARD_MULTIPLIER_TIER2_BPS,
            UptimeTier::Tier3 => REWARD_MULTIPLIER_TIER3_BPS,
        }
    }

    /// Temel ödülü bonus ile artır
    pub fn apply_bonus(&self, base_reward: u128) -> u128 {
        let bonus = base_reward * self.reward_bonus_bps() as u128 / 10_000;
        base_reward.saturating_add(bonus)
    }

    pub fn is_eligible(&self) -> bool {
        *self != UptimeTier::Below
    }
}

/// Uptime bps'den tier belirle
pub fn uptime_tier(uptime_bps: u64) -> UptimeTier {
    if uptime_bps >= UPTIME_TIER3_BPS {
        UptimeTier::Tier3
    } else if uptime_bps >= UPTIME_TIER2_BPS {
        UptimeTier::Tier2
    } else if uptime_bps >= UPTIME_TIER1_BPS {
        UptimeTier::Tier1
    } else {
        UptimeTier::Below
    }
}

// ─── ValidatorUptimeRecord ────────────────────────────────────────────────────

/// Validator'ın sliding window uptime kaydı
///
/// Cosmos SDK: `ValidatorSigningInfo { MissedBlocksCounter, StartHeight, IndexOffset }`
#[derive(Debug, Clone)]
pub struct ValidatorUptimeRecord {
    pub validator_id: ValidatorId,
    /// Sliding window: son UPTIME_WINDOW blok için imzaladı mı?
    window: VecDeque<bool>,
    /// Toplam imzalanan blok (window içinde)
    signed_count: usize,
    /// Toplam kaçırılan blok (tüm süre)
    total_missed: u64,
    /// Başlangıç sloту
    start_slot: u64,
}

impl ValidatorUptimeRecord {
    pub fn new(validator_id: ValidatorId, start_slot: u64) -> Self {
        Self {
            validator_id,
            window: VecDeque::with_capacity(UPTIME_WINDOW),
            signed_count: 0,
            total_missed: 0,
            start_slot,
        }
    }

    /// Bir slot için imza kaydı ekle
    pub fn record_slot(&mut self, signed: bool) {
        // Window'u güncelle
        if self.window.len() >= UPTIME_WINDOW {
            // En eski kaydı çıkar
            if let Some(old) = self.window.pop_front() {
                if old { self.signed_count -= 1; }
            }
        }
        self.window.push_back(signed);
        if signed {
            self.signed_count += 1;
        } else {
            self.total_missed += 1;
        }
    }

    /// Mevcut uptime (bps, 0-10000)
    pub fn uptime_bps(&self) -> u64 {
        if self.window.is_empty() { return 10_000; }
        (self.signed_count as u64 * 10_000) / self.window.len() as u64
    }

    /// Uptime tier
    pub fn tier(&self) -> UptimeTier {
        uptime_tier(self.uptime_bps())
    }

    /// Downtime threshold aşıldı mı? (slash tetikleme)
    pub fn is_jailable(&self) -> bool {
        self.uptime_bps() < MIN_SIGNED_PER_WINDOW_BPS
    }

    /// Toplam kaçırılan blok
    pub fn total_missed(&self) -> u64 {
        self.total_missed
    }

    /// Window'daki blok sayısı
    pub fn window_size(&self) -> usize {
        self.window.len()
    }
}

// ─── UptimeTracker ────────────────────────────────────────────────────────────

/// Tüm validator'ların uptime'ını takip eder
///
/// Cosmos SDK: `x/slashing Keeper`
pub struct UptimeTracker {
    inner: Mutex<UptimeTrackerInner>,
}

struct UptimeTrackerInner {
    records: HashMap<u64, ValidatorUptimeRecord>, // validator_id.0 → record
    current_slot: u64,
}

impl UptimeTracker {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(UptimeTrackerInner {
                records: HashMap::new(),
                current_slot: 0,
            }),
        }
    }

    /// Validator kaydını başlat
    pub fn register(&self, validator_id: ValidatorId, start_slot: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.records.insert(
            validator_id.0,
            ValidatorUptimeRecord::new(validator_id, start_slot),
        );
    }

    /// Blok başına: hangi validator'ların imzaladığını kaydet
    ///
    /// Cosmos SDK: `HandleValidatorSignature(ctx, valAddr, signed)`
    pub fn record_block(&self, slot: u64, signers: &[ValidatorId]) {
        let signer_set: std::collections::HashSet<u64> = signers.iter().map(|v| v.0).collect();
        let mut inner = self.inner.lock().unwrap();
        inner.current_slot = slot;

        for (vid, record) in inner.records.iter_mut() {
            let signed = signer_set.contains(vid);
            record.record_slot(signed);
        }
    }

    /// Belirli bir validator'ın uptime kaydını al
    pub fn get_record(&self, validator_id: ValidatorId) -> Option<ValidatorUptimeRecord> {
        self.inner.lock().unwrap()
            .records.get(&validator_id.0)
            .cloned()
    }

    /// Validator'ın uptime bps
    pub fn uptime_bps(&self, validator_id: ValidatorId) -> u64 {
        self.inner.lock().unwrap()
            .records.get(&validator_id.0)
            .map(|r| r.uptime_bps())
            .unwrap_or(10_000) // Yeni kayıt → %100 varsayılan
    }

    /// Validator'ın uptime tier'ı
    pub fn tier(&self, validator_id: ValidatorId) -> UptimeTier {
        uptime_tier(self.uptime_bps(validator_id))
    }

    /// Jailable validator'lar (uptime < %50)
    pub fn jailable_validators(&self) -> Vec<ValidatorId> {
        self.inner.lock().unwrap()
            .records.values()
            .filter(|r| r.is_jailable())
            .map(|r| r.validator_id)
            .collect()
    }

    /// Epoch sonunda ödülleri hesapla
    ///
    /// Her validator için: base_reward * uptime_tier_bonus
    pub fn calculate_epoch_rewards(
        &self,
        base_rewards: &HashMap<ValidatorId, u128>,
    ) -> HashMap<ValidatorId, u128> {
        let inner = self.inner.lock().unwrap();
        base_rewards.iter().map(|(vid, &base)| {
            let record = inner.records.get(&vid.0);
            let tier = record.map(|r| r.tier()).unwrap_or(UptimeTier::Tier1);
            let final_reward = tier.apply_bonus(base);
            (*vid, final_reward)
        }).collect()
    }

    /// Toplam kayıtlı validator sayısı
    pub fn validator_count(&self) -> usize {
        self.inner.lock().unwrap().records.len()
    }
}

impl Default for UptimeTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(n: u64) -> ValidatorId { ValidatorId(n) }

    // 1. Uptime tier sınırları
    #[test]
    fn test_uptime_tier_boundaries() {
        assert_eq!(uptime_tier(10_000), UptimeTier::Tier3); // %100
        assert_eq!(uptime_tier(9_900), UptimeTier::Tier3);
        assert_eq!(uptime_tier(9_899), UptimeTier::Tier2);
        assert_eq!(uptime_tier(9_000), UptimeTier::Tier2);
        assert_eq!(uptime_tier(8_999), UptimeTier::Tier1);
        assert_eq!(uptime_tier(8_000), UptimeTier::Tier1);
        assert_eq!(uptime_tier(7_999), UptimeTier::Below);
        assert_eq!(uptime_tier(0), UptimeTier::Below);
    }

    // 2. Tier ödül çarpanı
    #[test]
    fn test_tier_reward_bonus() {
        let base = 1_000_000u128;
        assert_eq!(UptimeTier::Below.apply_bonus(base), base); // bonus yok
        assert_eq!(UptimeTier::Tier1.apply_bonus(base), 1_050_000); // +5%
        assert_eq!(UptimeTier::Tier2.apply_bonus(base), 1_100_000); // +10%
        assert_eq!(UptimeTier::Tier3.apply_bonus(base), 1_200_000); // +20%
    }

    // 3. UptimeRecord sliding window
    #[test]
    fn test_uptime_record_sliding_window() {
        let mut record = ValidatorUptimeRecord::new(vid(1), 0);
        // 80 imza, 20 kaçır
        for i in 0..100 {
            record.record_slot(i < 80);
        }
        let uptime = record.uptime_bps();
        assert_eq!(uptime, 8_000);
        assert_eq!(record.tier(), UptimeTier::Tier1);
    }

    // 4. UptimeRecord %100 uptime
    #[test]
    fn test_uptime_record_perfect() {
        let mut record = ValidatorUptimeRecord::new(vid(1), 0);
        for _ in 0..100 {
            record.record_slot(true);
        }
        assert_eq!(record.uptime_bps(), 10_000);
        assert_eq!(record.tier(), UptimeTier::Tier3);
    }

    // 5. UptimeRecord jailable
    #[test]
    fn test_uptime_record_jailable() {
        let mut record = ValidatorUptimeRecord::new(vid(1), 0);
        // Sadece %30 imzala → jailable (%50 eşiği altında)
        for i in 0..100 {
            record.record_slot(i < 30);
        }
        assert!(record.is_jailable());
    }

    // 6. Sliding window boyut sınırı
    #[test]
    fn test_uptime_record_window_bounded() {
        let mut record = ValidatorUptimeRecord::new(vid(1), 0);
        for i in 0..200 {
            record.record_slot(i < 150); // son 100 slot'ta bazı kaçırmalar var
        }
        assert_eq!(record.window_size(), UPTIME_WINDOW);
    }

    // 7. UptimeTracker blok kaydı
    #[test]
    fn test_uptime_tracker_record_block() {
        let tracker = UptimeTracker::new();
        tracker.register(vid(1), 0);
        tracker.register(vid(2), 0);

        // Slot 1: sadece validator 1 imzaladı
        tracker.record_block(1, &[vid(1)]);
        // Slot 2: ikisi de imzaladı
        tracker.record_block(2, &[vid(1), vid(2)]);

        // v1: 2/2 = %100
        assert_eq!(tracker.uptime_bps(vid(1)), 10_000);
        // v2: 1/2 = %50
        assert_eq!(tracker.uptime_bps(vid(2)), 5_000);
    }

    // 8. Jailable validator tespiti
    #[test]
    fn test_uptime_tracker_jailable() {
        let tracker = UptimeTracker::new();
        tracker.register(vid(1), 0);
        tracker.register(vid(2), 0);

        // v1 hiç imzalamadı (100 slot)
        for slot in 1..=100 {
            tracker.record_block(slot, &[vid(2)]);
        }

        let jailable = tracker.jailable_validators();
        assert!(jailable.contains(&vid(1)));
        assert!(!jailable.contains(&vid(2)));
    }

    // 9. Epoch reward hesabı
    #[test]
    fn test_epoch_reward_with_uptime_bonus() {
        let tracker = UptimeTracker::new();
        tracker.register(vid(1), 0);
        tracker.register(vid(2), 0);

        // v1: %100 uptime (Tier3)
        // v2: %50 uptime (Below)
        for slot in 1..=100 {
            tracker.record_block(slot, &[vid(1)]);
        }

        let base_rewards: HashMap<ValidatorId, u128> = [
            (vid(1), 1_000_000u128),
            (vid(2), 1_000_000u128),
        ].into_iter().collect();

        let final_rewards = tracker.calculate_epoch_rewards(&base_rewards);

        let v1_reward = *final_rewards.get(&vid(1)).unwrap();
        let v2_reward = *final_rewards.get(&vid(2)).unwrap();

        // v1: Tier3 → +20% bonus
        assert_eq!(v1_reward, 1_200_000);
        // v2: Below → ödül baz (no bonus)
        assert_eq!(v2_reward, 1_000_000);
        assert!(v1_reward > v2_reward);
    }

    // 10. Tier eligible kontrolü
    #[test]
    fn test_tier_eligibility() {
        assert!(!UptimeTier::Below.is_eligible());
        assert!(UptimeTier::Tier1.is_eligible());
        assert!(UptimeTier::Tier2.is_eligible());
        assert!(UptimeTier::Tier3.is_eligible());
    }

    // 11. Validator count
    #[test]
    fn test_validator_count() {
        let tracker = UptimeTracker::new();
        tracker.register(vid(1), 0);
        tracker.register(vid(2), 0);
        tracker.register(vid(3), 0);
        assert_eq!(tracker.validator_count(), 3);
    }

    // 12. Total missed counter
    #[test]
    fn test_total_missed_counter() {
        let mut record = ValidatorUptimeRecord::new(vid(1), 0);
        for i in 0..200 {
            record.record_slot(i % 2 == 0); // Her 2 slot'ta 1 kaçır
        }
        assert_eq!(record.total_missed(), 100);
    }

    // 13. Tier ordering
    #[test]
    fn test_tier_ordering() {
        assert!(UptimeTier::Below < UptimeTier::Tier1);
        assert!(UptimeTier::Tier1 < UptimeTier::Tier2);
        assert!(UptimeTier::Tier2 < UptimeTier::Tier3);
    }
}
