//! GÖREV 102: Era Payout Formula (Polkadot staking pattern)
//!
//! ## Mimari
//!
//! ### Polkadot Era Reward Dağıtımı
//! - `era_payout(total_staked, total_issuance, ideal_stake) -> (validator_payout, remainder)`
//! - NPoS ödül eğrisi: ideal stake = %50 → maksimum APY
//! - %50 üstü → APY düşer (fazla stake → dilution)
//! - `ValidatorEraReward`: komisyon + nominatör payları
//!
//! ### Nova Uyarlaması
//! - `EraPayoutEngine`: era bazlı dağıtım hesabı
//! - Komisyon ayrımı: validator kısmı + delegatör kısmı
//! - Uptime bonus (Task 101 ile entegre)
//! - Staking oranına göre APY eğrisi

use crate::token::emission::{era_emission, era_to_year, SLOTS_PER_ERA};
use crate::token::nsc::NSC_TOTAL_SUPPLY;
use nova_primitives::ValidatorId;
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// İdeal stake oranı (%50 = 5000 bps)
pub const IDEAL_STAKE_RATIO_BPS: u64 = 5_000;

/// İdeal APY (5000 bps = %50 stake oranında)
/// Polkadot: `i_0 = 0.025` → %2.5 ideal APY (Nova: %5)
pub const IDEAL_APY_BPS: u64 = 500; // %5

/// Falloff rate (stake %50 üstündeyse APY düşüş hızı)
pub const FALLOFF_RATE_BPS: u64 = 200; // %2 per %1 over ideal

/// Maksimum komisyon oranı (%50)
pub const MAX_COMMISSION_BPS: u32 = 5_000;

/// Maksimum uptime bonus oranı (%20) — NOVA-RED-RUNTIME-008: taşmayı önler
pub const MAX_UPTIME_BONUS_BPS: u64 = 2_000;

/// Minimum komisyon oranı (%0 — izin verilir)
pub const MIN_COMMISSION_BPS: u32 = 0;

// ─── EraPayoutInfo ────────────────────────────────────────────────────────────

/// Era payout hesaplama girişi
#[derive(Debug, Clone)]
pub struct EraPayoutInput {
    /// Era numarası
    pub era: u64,
    /// Toplam staked miktar
    pub total_staked: u128,
    /// Toplam dolaşımdaki arz
    pub total_issuance: u128,
    /// Aktif validator sayısı
    pub active_validators: u64,
    /// Her validator'ın stake miktarı: validator_id → stake
    pub validator_stakes: HashMap<ValidatorId, u128>,
    /// Her validator'ın komisyon oranı (bps): validator_id → commission_bps
    pub validator_commissions: HashMap<ValidatorId, u32>,
    /// Her validator'ın uptime bonus çarpanı (bps): validator_id → bonus_bps
    pub validator_uptime_bonuses: HashMap<ValidatorId, u64>,
}

/// Era payout sonucu
#[derive(Debug, Clone)]
pub struct EraPayoutResult {
    /// Toplam era ödülü (emission)
    pub total_era_reward: u128,
    /// Her validator'ın aldığı pay: validator_id → (validator_reward, staker_reward)
    pub validator_rewards: HashMap<ValidatorId, ValidatorEraReward>,
    /// Treasury'e giden kısım
    pub treasury_reward: u128,
    /// Kullanılmayan emisyon (yuvarlama farkı)
    pub dust: u128,
}

/// Bir validator'ın era ödülü dökümü
#[derive(Debug, Clone)]
pub struct ValidatorEraReward {
    /// Validator'ın kendi komisyon ödülü
    pub commission_reward: u128,
    /// Delegatörlere giden pay (stake orantılı)
    pub staker_reward: u128,
    /// Uptime bonusu
    pub uptime_bonus: u128,
    /// Toplam (commission + staker + uptime_bonus)
    pub total: u128,
}

// ─── EraPayoutEngine ──────────────────────────────────────────────────────────

/// Era payout hesaplama motoru
///
/// Polkadot: `pallet_staking::era_payout()`
pub struct EraPayoutEngine;

impl EraPayoutEngine {
    /// Ana era payout hesabı
    pub fn calculate(input: &EraPayoutInput) -> EraPayoutResult {
        let year = era_to_year(input.era);
        let total_era_reward = era_emission(input.total_issuance, year);

        if input.active_validators == 0 || total_era_reward == 0 {
            return EraPayoutResult {
                total_era_reward,
                validator_rewards: HashMap::new(),
                treasury_reward: 0,
                dust: total_era_reward,
            };
        }

        // Staking oranı (bps)
        let stake_ratio_bps = if input.total_issuance > 0 {
            (input.total_staked * 10_000 / input.total_issuance).min(10_000)
        } else {
            0
        } as u64;

        // APY eğrisi: ideal_ratio → ideal_apy, dışına çıkınca düşer
        let reward_factor_bps = Self::npos_reward_factor(stake_ratio_bps);

        // Efektif toplam dağıtılacak ödül
        let effective_reward = total_era_reward * reward_factor_bps as u128 / 10_000;
        let treasury_cut = total_era_reward.saturating_sub(effective_reward);

        // Her validator'a stake orantılı dağıt
        let total_stake: u128 = input.validator_stakes.values().sum();
        let mut validator_rewards = HashMap::new();
        let mut distributed = 0u128;

        for (vid, &stake) in &input.validator_stakes {
            if total_stake == 0 { break; }

            // Bu validator'ın hak ettiği pay
            let base_share = effective_reward * stake / total_stake;

            // Uptime bonusu uygula
            // NOVA-RED-RUNTIME-008: uptime_bonus_bps MAX_UPTIME_BONUS_BPS ile sınırlandırılır —
            // sınırsız değer u128 taşmasına neden olur (base_share * u64::MAX → overflow).
            let uptime_bonus_bps = input
                .validator_uptime_bonuses
                .get(vid)
                .copied()
                .unwrap_or(0)
                .min(MAX_UPTIME_BONUS_BPS);
            let uptime_bonus = base_share * uptime_bonus_bps as u128 / 10_000;
            let with_uptime = base_share.saturating_add(uptime_bonus);

            // Komisyon ayır
            let commission_bps = input.validator_commissions.get(vid).copied()
                .unwrap_or(0)
                .min(MAX_COMMISSION_BPS);
            let commission_reward = with_uptime * commission_bps as u128 / 10_000;
            let staker_reward = with_uptime.saturating_sub(commission_reward);

            distributed += with_uptime;
            validator_rewards.insert(*vid, ValidatorEraReward {
                commission_reward,
                staker_reward,
                uptime_bonus,
                total: with_uptime,
            });
        }

        let dust = effective_reward.saturating_sub(distributed.saturating_sub(
            // uptime bonus miktarı effective'in üstüne çıkabilir, dust'u düzeltelim
            validator_rewards.values().map(|r| r.uptime_bonus).sum::<u128>()
        ));

        EraPayoutResult {
            total_era_reward,
            validator_rewards,
            treasury_reward: treasury_cut,
            dust,
        }
    }

    /// NPoS ödül faktörü (bps olarak)
    ///
    /// Polkadot: `f(x) = i_0 + (x/x_0) * (i_0 - i_1)` için parabolik yaklaşım
    /// Nova simplifikasyonu:
    /// - stake < ideal: lineer artış (0 → 10000 bps)
    /// - stake == ideal: 10000 bps (tam ödül)
    /// - stake > ideal: lineer azalış (falloff ile)
    pub fn npos_reward_factor(stake_ratio_bps: u64) -> u64 {
        let ideal = IDEAL_STAKE_RATIO_BPS;

        if stake_ratio_bps <= ideal {
            // 0 → ideal: lineer artış
            if ideal == 0 { return 10_000; }
            stake_ratio_bps * 10_000 / ideal
        } else {
            // ideal üstü: her %1 fazla için FALLOFF_RATE_BPS azalır
            let excess_bps = stake_ratio_bps - ideal;
            let penalty = excess_bps * FALLOFF_RATE_BPS / 100;
            10_000u64.saturating_sub(penalty)
        }
    }

    /// Delegatör payını hesapla (bir validator'a stake eden)
    ///
    /// Polkadot: `Exposure::NominatorExposure { who, value }`
    pub fn delegator_share(
        delegator_stake: u128,
        total_delegator_stake: u128,
        staker_reward: u128,
    ) -> u128 {
        if total_delegator_stake == 0 { return 0; }
        staker_reward * delegator_stake / total_delegator_stake
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(n: u64) -> ValidatorId { ValidatorId(n) }

    fn make_input(
        total_staked: u128,
        total_issuance: u128,
        stakes: &[(u64, u128)],
        commissions: &[(u64, u32)],
    ) -> EraPayoutInput {
        EraPayoutInput {
            era: 0,
            total_staked,
            total_issuance,
            active_validators: stakes.len() as u64,
            validator_stakes: stakes.iter().map(|(id, s)| (vid(*id), *s)).collect(),
            validator_commissions: commissions.iter().map(|(id, c)| (vid(*id), *c)).collect(),
            validator_uptime_bonuses: HashMap::new(),
        }
    }

    // 1. NPoS reward factor at ideal stake
    #[test]
    fn test_npos_reward_factor_at_ideal() {
        let factor = EraPayoutEngine::npos_reward_factor(IDEAL_STAKE_RATIO_BPS);
        assert_eq!(factor, 10_000); // %100 ödül
    }

    // 2. NPoS reward factor below ideal
    #[test]
    fn test_npos_reward_factor_below_ideal() {
        let factor = EraPayoutEngine::npos_reward_factor(2_500); // %25 staked
        assert_eq!(factor, 5_000); // %50 ödül (lineer)
    }

    // 3. NPoS reward factor above ideal (falloff)
    #[test]
    fn test_npos_reward_factor_above_ideal() {
        let factor_ideal = EraPayoutEngine::npos_reward_factor(IDEAL_STAKE_RATIO_BPS);
        let factor_over = EraPayoutEngine::npos_reward_factor(IDEAL_STAKE_RATIO_BPS + 500);
        assert!(factor_over < factor_ideal);
    }

    // 4. Era payout temel dağıtım
    #[test]
    fn test_era_payout_basic() {
        let input = make_input(
            NSC_TOTAL_SUPPLY / 2, // %50 staked (ideal)
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000), (2, 1_000)],
            &[(1, 0), (2, 0)],
        );

        let result = EraPayoutEngine::calculate(&input);
        assert!(result.total_era_reward > 0);
        // İki eşit validator → eşit pay
        let r1 = result.validator_rewards.get(&vid(1)).unwrap();
        let r2 = result.validator_rewards.get(&vid(2)).unwrap();
        assert_eq!(r1.total, r2.total);
    }

    // 5. Komisyon ayrımı
    #[test]
    fn test_commission_split() {
        let input = make_input(
            NSC_TOTAL_SUPPLY / 2,
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000)],
            &[(1, 2_000)], // %20 komisyon
        );

        let result = EraPayoutEngine::calculate(&input);
        let r = result.validator_rewards.get(&vid(1)).unwrap();

        // %20 komisyon → komisyon = total * 0.2
        let expected_commission = r.total * 2_000 / 10_000;
        // Rounding toleransı
        assert!((r.commission_reward as i128 - expected_commission as i128).abs() <= 1);
        assert_eq!(r.commission_reward + r.staker_reward, r.total - r.uptime_bonus);
    }

    // 6. Uptime bonus entegrasyonu
    #[test]
    fn test_uptime_bonus_increases_reward() {
        let mut input = make_input(
            NSC_TOTAL_SUPPLY / 2,
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000), (2, 1_000)],
            &[(1, 0), (2, 0)],
        );
        // v1 için %20 uptime bonus
        input.validator_uptime_bonuses.insert(vid(1), 2_000);

        let result = EraPayoutEngine::calculate(&input);
        let r1 = result.validator_rewards.get(&vid(1)).unwrap();
        let r2 = result.validator_rewards.get(&vid(2)).unwrap();

        assert!(r1.total > r2.total);
        assert!(r1.uptime_bonus > 0);
        assert_eq!(r2.uptime_bonus, 0);
    }

    // 7. Boş validator seti
    #[test]
    fn test_empty_validators() {
        let input = EraPayoutInput {
            era: 0,
            total_staked: 0,
            total_issuance: NSC_TOTAL_SUPPLY,
            active_validators: 0,
            validator_stakes: HashMap::new(),
            validator_commissions: HashMap::new(),
            validator_uptime_bonuses: HashMap::new(),
        };
        let result = EraPayoutEngine::calculate(&input);
        assert!(result.validator_rewards.is_empty());
        assert_eq!(result.dust, result.total_era_reward);
    }

    // 8. Orantılı stake dağıtımı
    #[test]
    fn test_proportional_stake_distribution() {
        let input = make_input(
            NSC_TOTAL_SUPPLY / 2,
            NSC_TOTAL_SUPPLY,
            &[(1, 3_000), (2, 1_000)], // v1 3x stake
            &[(1, 0), (2, 0)],
        );
        let result = EraPayoutEngine::calculate(&input);
        let r1 = result.validator_rewards.get(&vid(1)).unwrap();
        let r2 = result.validator_rewards.get(&vid(2)).unwrap();
        // v1 ~3x reward almalı
        let ratio = r1.total * 10_000 / r2.total;
        assert!((ratio as i64 - 30_000).abs() < 100); // ~3x ± küçük hata
    }

    // 9. Delegatör payı hesabı
    #[test]
    fn test_delegator_share() {
        let staker_reward = 1_000_000u128;
        let delegator_stake = 300u128;
        let total_delegator_stake = 1_000u128;

        let share = EraPayoutEngine::delegator_share(
            delegator_stake, total_delegator_stake, staker_reward
        );
        assert_eq!(share, 300_000); // %30
    }

    // 10. Era>0 → farklı emission (yıl geçiş)
    #[test]
    fn test_different_eras_different_emission() {
        let make = |era: u64| {
            let mut input = make_input(
                NSC_TOTAL_SUPPLY / 2, NSC_TOTAL_SUPPLY,
                &[(1, 1_000)], &[(1, 0)],
            );
            input.era = era;
            EraPayoutEngine::calculate(&input)
        };

        // Era 0 (yıl 0) vs era 365 (yıl ~1)
        let r0 = make(0);
        let r365 = make(365 * 1000); // farklı yıl

        // Yıl 0 > yıl N (emisyon azalıyor)
        assert!(r0.total_era_reward >= r365.total_era_reward);
    }

    // 11. Treasury cut (non-ideal stake → some goes to treasury)
    #[test]
    fn test_treasury_cut_when_understaked() {
        let input = make_input(
            NSC_TOTAL_SUPPLY / 10, // %10 staked (ideal altı)
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000)],
            &[(1, 0)],
        );
        let result = EraPayoutEngine::calculate(&input);
        // %10 staked → reward_factor < 10000 → treasury_reward > 0
        assert!(result.treasury_reward > 0 || result.total_era_reward == 0);
    }

    /// NOVA-RED-RUNTIME-008: aşırı büyük uptime_bonus_bps taşmaya neden olmamalı
    #[test]
    fn test_uptime_bonus_overflow_capped() {
        let mut input = make_input(
            NSC_TOTAL_SUPPLY / 2,
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000)],
            &[(1, 0)],
        );
        // u64::MAX bonus → MAX_UPTIME_BONUS_BPS ile kesilmeli, panik/overflow olmamalı
        input.validator_uptime_bonuses.insert(vid(1), u64::MAX);

        let result = EraPayoutEngine::calculate(&input);
        let r = result.validator_rewards.get(&vid(1)).unwrap();

        // Bonus maksimum %20 olmalı
        let base_without_bonus = r.total - r.uptime_bonus;
        let max_allowed_bonus = base_without_bonus * MAX_UPTIME_BONUS_BPS as u128 / 10_000;
        assert!(
            r.uptime_bonus <= max_allowed_bonus + 1,
            "uptime bonus MAX_UPTIME_BONUS_BPS ile sınırlı olmalı, got: {}",
            r.uptime_bonus
        );
    }

    /// NOVA-RED-RUNTIME-008: MAX_UPTIME_BONUS_BPS = 2000 (% 20) üstü kesilmeli
    #[test]
    fn test_uptime_bonus_exceeding_max_is_capped() {
        let mut input = make_input(
            NSC_TOTAL_SUPPLY / 2,
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000), (2, 1_000)],
            &[(1, 0), (2, 0)],
        );
        input.validator_uptime_bonuses.insert(vid(1), 9_000); // %90 → %20'ye kesilmeli
        input.validator_uptime_bonuses.insert(vid(2), 2_000); // zaten %20

        let result = EraPayoutEngine::calculate(&input);
        let r1 = result.validator_rewards.get(&vid(1)).unwrap();
        let r2 = result.validator_rewards.get(&vid(2)).unwrap();

        // Her iki validator aynı bonus (her ikisi de %20 ile sınırlı)
        assert_eq!(r1.total, r2.total, "her iki validator eşit bonus almalı (%20 cap)");
    }

    // 12. Max komisyon sınırı
    #[test]
    fn test_max_commission_capped() {
        let input = make_input(
            NSC_TOTAL_SUPPLY / 2,
            NSC_TOTAL_SUPPLY,
            &[(1, 1_000)],
            &[(1, 9_000)], // %90 → capped at %50
        );
        let result = EraPayoutEngine::calculate(&input);
        let r = result.validator_rewards.get(&vid(1)).unwrap();
        // Commission ≤ %50 of total
        assert!(r.commission_reward <= r.total / 2 + 1); // +1 rounding tolerans
    }
}
