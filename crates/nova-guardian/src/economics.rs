//! Guardian ekonomi özeti (§5.8)
//!
//! Guardian başına gelir tahmini, toplam ekosistem istatistikleri,
//! staking APY hesabı.

use crate::fees::models::FeeModel;
use crate::registry::categories::GuardianCategory;
use serde::{Deserialize, Serialize};

// ─── GuardianEconomics ────────────────────────────────────────────────────────

/// Guardian başına gelir tahmini
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianEconomics {
    pub guardian_category: GuardianCategory,
    pub staked_amount: u128,
    pub accounts_guarding: u32,
    pub fee_model: FeeModel,
    /// Yıllık beklenen gelir (NSC)
    pub estimated_annual_revenue: u128,
    /// Basit APY (staking geliri / stake × 10_000 bps)
    pub apy_bps: u64,
}

impl GuardianEconomics {
    pub fn new(
        guardian_category: GuardianCategory,
        staked_amount: u128,
        accounts_guarding: u32,
        fee_model: FeeModel,
    ) -> Self {
        let annual_per_account = fee_model.estimated_annual_cost();
        let estimated_annual_revenue = annual_per_account * accounts_guarding as u128;

        let apy_bps = if staked_amount > 0 {
            // APY = (gelir / stake) × 10_000
            let bps = estimated_annual_revenue
                .saturating_mul(10_000)
                .checked_div(staked_amount)
                .unwrap_or(0);
            bps.min(u64::MAX as u128) as u64
        } else {
            0
        };

        Self {
            guardian_category,
            staked_amount,
            accounts_guarding,
            fee_model,
            estimated_annual_revenue,
            apy_bps,
        }
    }

    /// Aylık gelir tahmini
    pub fn monthly_revenue(&self) -> u128 {
        self.estimated_annual_revenue / 12
    }

    /// APY yüzdesi (insana okunabilir)
    pub fn apy_percent_display(&self) -> f64 {
        self.apy_bps as f64 / 100.0
    }
}

// ─── EcosystemStats ───────────────────────────────────────────────────────────

/// Tüm guardian ekosistemi istatistikleri
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EcosystemStats {
    pub total_guardians: u32,
    pub active_guardians: u32,
    pub total_staked_nsc: u128,
    pub total_accounts_protected: u32,
    pub insurance_fund_balance: u128,
    /// Toplam başarılı kurtarma sayısı
    pub total_successful_recoveries: u64,
    /// Toplam başarısız kurtarma sayısı
    pub total_failed_recoveries: u64,
}

impl EcosystemStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Genel kurtarma başarı oranı (bps)
    pub fn recovery_success_rate_bps(&self) -> u32 {
        let total = self.total_successful_recoveries + self.total_failed_recoveries;
        if total == 0 {
            return 10_000;
        }
        ((self.total_successful_recoveries as u128 * 10_000) / total as u128) as u32
    }

    /// Ortalama hesap başına stake
    pub fn avg_stake_per_protected_account(&self) -> u128 {
        if self.total_accounts_protected == 0 {
            return 0;
        }
        self.total_staked_nsc / self.total_accounts_protected as u128
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fees::models::AnnualFee;
    use crate::registry::categories::MIN_STAKE_CERTIFIED_INDIVIDUAL;

    const NSC: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn test_annual_economics() {
        let econ = GuardianEconomics::new(
            GuardianCategory::CertifiedIndividual,
            MIN_STAKE_CERTIFIED_INDIVIDUAL,
            10,
            FeeModel::Annual(AnnualFee { nsc_per_year: 50 * NSC }),
        );
        assert_eq!(econ.estimated_annual_revenue, 500 * NSC);
        assert_eq!(econ.monthly_revenue(), 500 * NSC / 12);
    }

    #[test]
    fn test_free_model_economics() {
        let econ = GuardianEconomics::new(
            GuardianCategory::Personal,
            0,
            1,
            FeeModel::Free,
        );
        assert_eq!(econ.estimated_annual_revenue, 0);
        assert_eq!(econ.apy_bps, 0);
    }

    #[test]
    fn test_apy_calculation() {
        // Stake = 1000 NSC, gelir = 100 NSC/yıl → APY = %10 = 1000 bps
        let econ = GuardianEconomics::new(
            GuardianCategory::CertifiedIndividual,
            1_000 * NSC,
            2,
            FeeModel::Annual(AnnualFee { nsc_per_year: 50 * NSC }),
        );
        // 2 hesap × 50 NSC = 100 NSC gelir, stake = 1000 NSC → APY = 10% = 1000 bps
        assert_eq!(econ.estimated_annual_revenue, 100 * NSC);
        assert_eq!(econ.apy_bps, 1_000);
        assert!((econ.apy_percent_display() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_ecosystem_stats_success_rate() {
        let mut stats = EcosystemStats::new();
        stats.total_successful_recoveries = 95;
        stats.total_failed_recoveries = 5;
        assert_eq!(stats.recovery_success_rate_bps(), 9_500); // %95
    }

    #[test]
    fn test_ecosystem_stats_empty() {
        let stats = EcosystemStats::new();
        assert_eq!(stats.recovery_success_rate_bps(), 10_000); // sıfır bölmeden koruma
    }

    #[test]
    fn test_avg_stake_per_account() {
        let mut stats = EcosystemStats::new();
        stats.total_staked_nsc = 1_000 * NSC;
        stats.total_accounts_protected = 10;
        assert_eq!(stats.avg_stake_per_protected_account(), 100 * NSC);
    }
}
