//! Protocol Fee Dağıtımı — §7.4
//!
//! Whitepaper §7.4 — %0.25 DEX protokol fee dağıtımı:
//!   Validator'lar (stake oranında): %30
//!   NSC Staker'lar:                 %30
//!   Ekosistem Fonu:                 %25
//!   Hazine (DAO):                   %15

use nova_primitives::Address;

// ─── Dağıtım oranları (BPS) ────────────────────────────────────────────────────

pub const VALIDATOR_SHARE_BPS: u32 = 3_000;    // %30
pub const STAKER_SHARE_BPS: u32 = 3_000;        // %30
pub const ECOSYSTEM_SHARE_BPS: u32 = 2_500;     // %25
pub const TREASURY_SHARE_BPS: u32 = 1_500;      // %15

/// Protocol DEX fee oranı: 25 BPS = %0.25
pub const PROTOCOL_FEE_BPS: u32 = 25;

/// Fee dağıtımı hesapla: (validators, stakers, ecosystem, treasury)
pub fn distribute_protocol_fee(total_fee: u128) -> FeeDistribution {
    let validators = total_fee * VALIDATOR_SHARE_BPS as u128 / 10_000;
    let stakers = total_fee * STAKER_SHARE_BPS as u128 / 10_000;
    let ecosystem = total_fee * ECOSYSTEM_SHARE_BPS as u128 / 10_000;
    // Rounding remainder → hazineye
    let treasury = total_fee - validators - stakers - ecosystem;
    FeeDistribution { validators, stakers, ecosystem, treasury }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeDistribution {
    pub validators: u128,
    pub stakers: u128,
    pub ecosystem: u128,
    pub treasury: u128,
}

impl FeeDistribution {
    pub fn total(&self) -> u128 {
        self.validators + self.stakers + self.ecosystem + self.treasury
    }
}

/// ValidatorFeeShare — validator başına fee payı (stake oranına göre)
pub fn validator_fee_share(
    validator_stake: u128,
    total_validator_stake: u128,
    total_validator_pool: u128,
) -> u128 {
    if total_validator_stake == 0 {
        return 0;
    }
    total_validator_pool * validator_stake / total_validator_stake
}

/// F1 Fee Distribution — Cosmos SDK pattern
/// Kümülatif ödül/pay oranı takibi (O(1) withdraw)
#[derive(Debug, Clone)]
pub struct F1RewardTracker {
    /// Kümülatif ödül per stake unit (scaled by 1e18)
    cumulative_reward_per_unit: u128,
    /// Toplam stake (pay hesabı için)
    total_stake: u128,
    /// Toplam dağıtılmış ödül
    total_distributed: u128,
}

impl Default for F1RewardTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl F1RewardTracker {
    const SCALE: u128 = 1_000_000_000_000_000_000u128; // 1e18

    pub fn new() -> Self {
        Self {
            cumulative_reward_per_unit: 0,
            total_stake: 0,
            total_distributed: 0,
        }
    }

    /// Toplam stake güncelle
    pub fn set_total_stake(&mut self, stake: u128) {
        self.total_stake = stake;
    }

    /// Yeni ödül ekle — kümülatif oran güncellenir
    pub fn add_reward(&mut self, reward: u128) {
        if self.total_stake == 0 {
            return;
        }
        self.cumulative_reward_per_unit += reward * Self::SCALE / self.total_stake;
        self.total_distributed += reward;
    }

    /// Delegatörün ödülü hesapla
    /// starting_cumulative: delegatörün son withdraw/stake zamanındaki kümülatif oran
    pub fn pending_reward(&self, stake: u128, starting_cumulative: u128) -> u128 {
        if self.cumulative_reward_per_unit < starting_cumulative {
            return 0;
        }
        let delta = self.cumulative_reward_per_unit - starting_cumulative;
        delta * stake / Self::SCALE
    }

    pub fn cumulative_reward_per_unit(&self) -> u128 {
        self.cumulative_reward_per_unit
    }

    pub fn total_distributed(&self) -> u128 {
        self.total_distributed
    }
}

/// DelegatorRewardInfo — delegatör başına ödül takip bilgisi
#[derive(Debug, Clone)]
pub struct DelegatorRewardInfo {
    pub delegator: Address,
    pub stake: u128,
    pub starting_cumulative: u128,
}

impl DelegatorRewardInfo {
    pub fn new(delegator: Address, stake: u128, starting_cumulative: u128) -> Self {
        Self { delegator, stake, starting_cumulative }
    }

    /// Bekleyen ödül hesapla
    pub fn pending(&self, tracker: &F1RewardTracker) -> u128 {
        tracker.pending_reward(self.stake, self.starting_cumulative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_distribution_sum() {
        let dist = distribute_protocol_fee(10_000);
        assert_eq!(dist.total(), 10_000);
    }

    #[test]
    fn test_fee_distribution_ratios() {
        let dist = distribute_protocol_fee(10_000);
        assert_eq!(dist.validators, 3_000); // %30
        assert_eq!(dist.stakers, 3_000);    // %30
        assert_eq!(dist.ecosystem, 2_500);  // %25
        assert_eq!(dist.treasury, 1_500);   // %15
    }

    #[test]
    fn test_fee_distribution_zero() {
        let dist = distribute_protocol_fee(0);
        assert_eq!(dist.total(), 0);
    }

    #[test]
    fn test_fee_distribution_rounding() {
        // 10_001 — remainder %15'e gider
        let dist = distribute_protocol_fee(10_001);
        assert_eq!(dist.total(), 10_001);
    }

    #[test]
    fn test_validator_fee_share_equal_stake() {
        // 3 validator, eşit stake → eşit pay
        let share = validator_fee_share(100, 300, 3_000);
        assert_eq!(share, 1_000);
    }

    #[test]
    fn test_validator_fee_share_proportional() {
        // V1: 200 stake, V2: 100 stake → V1 2x pay alır
        let v1_share = validator_fee_share(200, 300, 3_000);
        let v2_share = validator_fee_share(100, 300, 3_000);
        assert_eq!(v1_share, 2_000);
        assert_eq!(v2_share, 1_000);
    }

    #[test]
    fn test_validator_fee_share_zero_total() {
        assert_eq!(validator_fee_share(100, 0, 3_000), 0);
    }

    #[test]
    fn test_f1_tracker_basic() {
        let mut tracker = F1RewardTracker::new();
        tracker.set_total_stake(1_000);
        tracker.add_reward(100);

        // Stake=500, tüm dönem boyunca stake edildi
        let pending = tracker.pending_reward(500, 0);
        assert_eq!(pending, 50); // 500/1000 * 100
    }

    #[test]
    fn test_f1_tracker_multiple_rewards() {
        let mut tracker = F1RewardTracker::new();
        tracker.set_total_stake(1_000);
        tracker.add_reward(100);
        let cumulative_after_first = tracker.cumulative_reward_per_unit();
        tracker.add_reward(200);

        // Delegatör sadece 2. ödülü alır
        let pending = tracker.pending_reward(500, cumulative_after_first);
        assert_eq!(pending, 100); // 500/1000 * 200
    }

    #[test]
    fn test_f1_tracker_zero_stake() {
        let mut tracker = F1RewardTracker::new();
        tracker.set_total_stake(0);
        tracker.add_reward(100); // Hiçbir şey olmaz
        assert_eq!(tracker.total_distributed(), 0);
    }

    #[test]
    fn test_delegator_reward_info() {
        let addr = Address::from_bytes([1u8; 20]);
        let mut tracker = F1RewardTracker::new();
        tracker.set_total_stake(1_000);
        tracker.add_reward(1_000);

        let info = DelegatorRewardInfo::new(addr, 250, 0);
        let pending = info.pending(&tracker);
        assert_eq!(pending, 250); // 250/1000 * 1000
    }

    #[test]
    fn test_protocol_fee_bps() {
        assert_eq!(PROTOCOL_FEE_BPS, 25); // %0.25
    }

    #[test]
    fn test_all_shares_sum_to_10000() {
        let total = VALIDATOR_SHARE_BPS + STAKER_SHARE_BPS + ECOSYSTEM_SHARE_BPS + TREASURY_SHARE_BPS;
        assert_eq!(total, 10_000);
    }
}
