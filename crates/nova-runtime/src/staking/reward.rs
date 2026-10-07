//! Staking ödül hesabı (Polkadot NPoS era payout'tan ilham)
//!
//! Era payout formülü (Polkadot'tan uyarlama):
//!   era_reward = toplam_emisyon / aktif_validator_sayısı (eşit dağıtım varsayımı)
//!   validator_cut = era_reward * validator_points / total_points
//!   komisyon = validator_cut * commission_bps / 10_000
//!   delegator_pool = validator_cut - komisyon
//!
//! F1 Fee Distribution: Delegatör ödülleri kümülatif oran ile hesaplanır.

use crate::staking::delegator::DelegatorStore;
use crate::staking::validator::{Validator, ValidatorPool};
use crate::token::emission::{era_emission, era_to_year};
use crate::token::fee_dist::{distribute_protocol_fee, validator_fee_share, FeeDistribution};
use crate::token::nsc::NSC_TOTAL_SUPPLY;
use nova_primitives::ValidatorId;
use std::collections::HashMap;

/// EraRewardResult — era sonunda hesaplanan ödüller
#[derive(Debug, Clone)]
pub struct EraRewardResult {
    pub era: u64,
    /// Validator başına tahsis (ValidatorId → ödül miktarı)
    pub validator_rewards: HashMap<ValidatorId, u128>,
    /// Toplam emisyon bu era için
    pub total_emission: u128,
    /// Protocol fee dağıtımı (bu era'da biriken)
    pub fee_distribution: Option<FeeDistribution>,
}

/// Era payout hesabı
pub fn compute_era_rewards(
    era: u64,
    active_validators: &[Validator],
    validator_points: &HashMap<ValidatorId, u64>,
    accumulated_protocol_fee: u128,
) -> EraRewardResult {
    let year = era_to_year(era);
    let total_emission = era_emission(NSC_TOTAL_SUPPLY, year);

    let total_points: u64 = validator_points.values().sum();
    let mut validator_rewards = HashMap::new();

    if total_points > 0 {
        for v in active_validators {
            let points = validator_points.get(&v.id).copied().unwrap_or(0);
            if points == 0 {
                continue;
            }
            // Noktasına göre orantılı ödül
            let reward = total_emission * points as u128 / total_points as u128;
            validator_rewards.insert(v.id, reward);
        }
    } else if !active_validators.is_empty() {
        // Eşit dağıtım (points yoksa)
        let per_validator = total_emission / active_validators.len() as u128;
        for v in active_validators {
            validator_rewards.insert(v.id, per_validator);
        }
    }

    let fee_distribution = if accumulated_protocol_fee > 0 {
        Some(distribute_protocol_fee(accumulated_protocol_fee))
    } else {
        None
    };

    EraRewardResult { era, validator_rewards, total_emission, fee_distribution }
}

/// ValidatorPointsTracker — era boyunca blok üretim noktaları
#[derive(Debug, Default, Clone)]
pub struct ValidatorPointsTracker {
    points: HashMap<ValidatorId, u64>,
}

impl ValidatorPointsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Blok önerisi için puan ekle
    pub fn add_block_points(&mut self, validator_id: ValidatorId, points: u64) {
        *self.points.entry(validator_id).or_insert(0) += points;
    }

    /// Era sonunda noktaları al ve sıfırla
    pub fn drain(&mut self) -> HashMap<ValidatorId, u64> {
        std::mem::take(&mut self.points)
    }

    pub fn get(&self, id: &ValidatorId) -> u64 {
        self.points.get(id).copied().unwrap_or(0)
    }

    pub fn total(&self) -> u64 {
        self.points.values().sum()
    }
}

// ─── ERA YÖNETİCİSİ ───────────────────────────────────────────────────────────

/// AŞAMA 7 Görev 55: Era sistemi — epoch başına ödül hesabı ve geçiş yönetimi.
///
/// ## Polkadot Referansı
///
/// `frame/staking/src/lib.rs`:
/// - `EraIndex`, `ActiveEra`, `CurrentEra` storage items
/// - `end_era()` → ödülleri hesapla, `start_era()` → yeni validator seti seç
/// - Era süresi: `SessionsPerEra * SessionLength` slot
///
/// ## Nova Uyarlaması
///
/// - `ERA_DURATION_SLOTS`: 1 era = 1 gün = 216_000 slot (400ms/slot)
/// - Era başında: Phragmén ile validator seti seç
/// - Era sonunda: ödül dağıt, noktaları sıfırla
pub const ERA_DURATION_SLOTS: u64 = 216_000; // 1 gün

/// Era geçiş olayı.
#[derive(Debug, Clone)]
pub enum EraEvent {
    /// Yeni era başladı.
    EraStarted { era: u64, slot: u64 },
    /// Era tamamlandı, ödüller hesaplandı.
    EraEnded { era: u64, reward_result: EraRewardResult },
}

/// Era yöneticisi — era başı/sonu geçişlerini yönetir.
#[derive(Debug)]
pub struct EraManager {
    /// Mevcut era numarası.
    pub current_era: u64,
    /// Bu era'nın başladığı slot.
    pub era_start_slot: u64,
    /// Era süresi (slot cinsinden).
    pub era_duration_slots: u64,
    /// Bu era boyunca biriken protokol ücreti.
    pub accumulated_fee: u128,
    /// Bu era boyunca biriken olaylar.
    events: Vec<EraEvent>,
}

impl EraManager {
    /// Yeni era yöneticisi oluştur.
    pub fn new(genesis_slot: u64, era_duration_slots: u64) -> Self {
        Self {
            current_era: 0,
            era_start_slot: genesis_slot,
            era_duration_slots,
            accumulated_fee: 0,
            events: Vec::new(),
        }
    }

    /// Varsayılan (1 gün) süreli era yöneticisi.
    pub fn default_at(genesis_slot: u64) -> Self {
        Self::new(genesis_slot, ERA_DURATION_SLOTS)
    }

    /// Verilen slot'ta era geçişi gerekiyor mu?
    pub fn should_end_era(&self, current_slot: u64) -> bool {
        current_slot >= self.era_start_slot + self.era_duration_slots
    }

    /// Mevcut era kaç slot sürdü?
    pub fn elapsed_slots(&self, current_slot: u64) -> u64 {
        current_slot.saturating_sub(self.era_start_slot)
    }

    /// Protokol ücreti ekle (her TX'ten).
    pub fn add_protocol_fee(&mut self, amount: u128) {
        self.accumulated_fee = self.accumulated_fee.saturating_add(amount);
    }

    /// Era'yı kapat: ödülleri hesapla, yeni era başlat.
    ///
    /// `active_validators`: Mevcut aktif validator listesi
    /// `validator_points`: Bu era boyunca biriken blok noktaları
    /// `current_slot`: Geçiş anındaki slot numarası
    ///
    /// Returns: Era geçiş olayları
    pub fn end_era(
        &mut self,
        active_validators: &[Validator],
        validator_points: HashMap<ValidatorId, u64>,
        current_slot: u64,
    ) -> Vec<EraEvent> {
        let mut events = Vec::new();

        // Era ödüllerini hesapla
        let reward_result = compute_era_rewards(
            self.current_era,
            active_validators,
            &validator_points,
            self.accumulated_fee,
        );

        events.push(EraEvent::EraEnded {
            era: self.current_era,
            reward_result,
        });

        // Yeni era başlat
        self.current_era += 1;
        self.era_start_slot = current_slot;
        self.accumulated_fee = 0;

        events.push(EraEvent::EraStarted {
            era: self.current_era,
            slot: current_slot,
        });

        events
    }

    /// Era geçişi yoksa slot bazlı güncelleme yap.
    /// `protocol_fee`: Bu slot'ta toplanan fee
    pub fn process_slot(&mut self, current_slot: u64, protocol_fee: u128) -> Option<u64> {
        self.add_protocol_fee(protocol_fee);
        if self.should_end_era(current_slot) {
            Some(self.current_era)
        } else {
            None
        }
    }
}

/// EraRewardDistributor — era ödüllerini validator ve delegatörlere dağıt
pub struct EraRewardDistributor<'a> {
    pool: &'a ValidatorPool,
    delegator_store: &'a DelegatorStore,
}

impl<'a> EraRewardDistributor<'a> {
    pub fn new(pool: &'a ValidatorPool, delegator_store: &'a DelegatorStore) -> Self {
        Self { pool, delegator_store }
    }

    /// Validator'a ödül dağıt: komisyon → validator, kalan → F1 tracker
    pub fn distribute_to_validator(&self, validator_id: ValidatorId, reward: u128) {
        let Some(validator) = self.pool.get(&validator_id) else { return };

        let commission = validator.commission_amount(reward);
        let delegator_pool = reward - commission;

        // Delegatör pool'unu F1 tracker'a ekle
        let total_delegated = self.delegator_store.total_delegated(&validator_id);
        if delegator_pool > 0 && total_delegated > 0 {
            let mut tracker = self.delegator_store.get_or_create_tracker(validator_id);
            tracker.set_total_stake(total_delegated);
            tracker.add_reward(delegator_pool);
            self.delegator_store.update_tracker(validator_id, tracker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staking::validator::Validator;
    use crate::token::nsc::MIN_VALIDATOR_STAKE_NSC;
    use nova_primitives::Address;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_validator(id: u64) -> Validator {
        Validator::new(
            ValidatorId(id),
            addr(id as u8),
            MIN_VALIDATOR_STAKE_NSC,
            500,
            MIN_VALIDATOR_STAKE_NSC,
            0,
        )
    }

    #[test]
    fn test_era_rewards_equal_distribution() {
        let validators: Vec<_> = (1..=5).map(make_validator).collect();
        let points: HashMap<ValidatorId, u64> = HashMap::new(); // Nokta yok → eşit dağıtım
        let result = compute_era_rewards(0, &validators, &points, 0);
        assert_eq!(result.validator_rewards.len(), 5);
        let per = result.total_emission / 5;
        for (_, &reward) in &result.validator_rewards {
            assert_eq!(reward, per);
        }
    }

    #[test]
    fn test_era_rewards_points_based() {
        let validators: Vec<_> = (1..=2).map(make_validator).collect();
        let mut points = HashMap::new();
        points.insert(ValidatorId(1), 70u64);
        points.insert(ValidatorId(2), 30u64);
        let result = compute_era_rewards(0, &validators, &points, 0);

        let r1 = result.validator_rewards[&ValidatorId(1)];
        let r2 = result.validator_rewards[&ValidatorId(2)];
        // V1: 70% ödül, V2: 30% ödül
        assert!(r1 > r2);
        // Integer division may leave ≤1 dust unassigned
        assert!(result.total_emission - (r1 + r2) <= 1);
    }

    #[test]
    fn test_era_rewards_zero_validators() {
        let result = compute_era_rewards(0, &[], &HashMap::new(), 0);
        assert!(result.validator_rewards.is_empty());
        assert!(result.total_emission > 0);
    }

    #[test]
    fn test_era_rewards_with_protocol_fee() {
        let validators: Vec<_> = (1..=2).map(make_validator).collect();
        let result = compute_era_rewards(0, &validators, &HashMap::new(), 10_000);
        assert!(result.fee_distribution.is_some());
        let dist = result.fee_distribution.unwrap();
        assert_eq!(dist.total(), 10_000);
    }

    #[test]
    fn test_points_tracker() {
        let mut tracker = ValidatorPointsTracker::new();
        tracker.add_block_points(ValidatorId(1), 10);
        tracker.add_block_points(ValidatorId(1), 5);
        tracker.add_block_points(ValidatorId(2), 20);
        assert_eq!(tracker.get(&ValidatorId(1)), 15);
        assert_eq!(tracker.total(), 35);

        let drained = tracker.drain();
        assert_eq!(drained[&ValidatorId(1)], 15);
        assert_eq!(tracker.total(), 0); // Sıfırlandı
    }

    #[test]
    fn test_era_emission_decreases_over_time() {
        let result_y0 = compute_era_rewards(0, &[make_validator(1)], &HashMap::new(), 0);
        let result_y1 = compute_era_rewards(365, &[make_validator(1)], &HashMap::new(), 0);
        // Yıl 1 (era 365) → year = 365 / 365 = 1 → daha az emisyon
        // Not: era_to_year hesabına göre değişir
        assert!(result_y0.total_emission >= result_y1.total_emission);
    }

    // ─── EraManager Testleri ──────────────────────────────────────────────────

    #[test]
    fn test_era_manager_initial_state() {
        let mgr = EraManager::default_at(0);
        assert_eq!(mgr.current_era, 0);
        assert_eq!(mgr.era_start_slot, 0);
        assert!(!mgr.should_end_era(100));
    }

    #[test]
    fn test_era_manager_should_end() {
        let mgr = EraManager::default_at(0);
        assert!(mgr.should_end_era(ERA_DURATION_SLOTS));
        assert!(mgr.should_end_era(ERA_DURATION_SLOTS + 1000));
        assert!(!mgr.should_end_era(ERA_DURATION_SLOTS - 1));
    }

    #[test]
    fn test_era_manager_end_era() {
        let mut mgr = EraManager::new(0, 1000);
        mgr.add_protocol_fee(5_000);

        let validators = vec![make_validator(1), make_validator(2)];
        let mut pts = HashMap::new();
        pts.insert(ValidatorId(1), 60u64);
        pts.insert(ValidatorId(2), 40u64);

        let events = mgr.end_era(&validators, pts, 1000);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], EraEvent::EraEnded { era: 0, .. }));
        assert!(matches!(events[1], EraEvent::EraStarted { era: 1, slot: 1000 }));

        // Yeni era başladı
        assert_eq!(mgr.current_era, 1);
        assert_eq!(mgr.era_start_slot, 1000);
        assert_eq!(mgr.accumulated_fee, 0); // Sıfırlandı
    }

    #[test]
    fn test_era_manager_process_slot() {
        let mut mgr = EraManager::new(0, 100);
        // Fee birikiyor
        mgr.process_slot(50, 1_000);
        assert_eq!(mgr.accumulated_fee, 1_000);
        // Era bitişinde signal döner
        let signal = mgr.process_slot(100, 500);
        assert_eq!(signal, Some(0)); // Era 0 bitmeli
        assert_eq!(mgr.accumulated_fee, 1_500);
    }

    #[test]
    fn test_era_manager_consecutive_eras() {
        let mut mgr = EraManager::new(0, 1000);
        let validators = vec![make_validator(1)];

        // Era 0 → 1
        mgr.end_era(&validators, HashMap::new(), 1000);
        assert_eq!(mgr.current_era, 1);

        // Era 1 → 2
        mgr.end_era(&validators, HashMap::new(), 2000);
        assert_eq!(mgr.current_era, 2);
        assert_eq!(mgr.era_start_slot, 2000);
    }
}
