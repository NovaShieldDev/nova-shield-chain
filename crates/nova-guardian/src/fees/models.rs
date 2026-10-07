//! Ücret modelleri (§5.8.4): Yıllık, kurtarma başına, ücretsiz, karma

use serde::{Deserialize, Serialize};

// ─── AnnualFee ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnnualFee {
    pub nsc_per_year: u128,
}

// ─── PerRecoveryFee ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerRecoveryFee {
    pub nsc_per_recovery: u128,
}

// ─── HybridFee ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HybridFee {
    pub annual_base: u128,
    pub per_recovery_bonus: u128,
}

// ─── FeeModel ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FeeModel {
    /// Kişisel guardian — ücretsiz
    Free,
    /// Yıllık sabit ücret
    Annual(AnnualFee),
    /// Her başarılı kurtarma için ücret
    PerRecovery(PerRecoveryFee),
    /// Karma model (yıllık baz + kurtarma bonusu)
    Hybrid(HybridFee),
}

impl FeeModel {
    /// Yıllık toplam maliyet tahmini (1 kurtarma varsayımı)
    pub fn estimated_annual_cost(&self) -> u128 {
        match self {
            FeeModel::Free => 0,
            FeeModel::Annual(f) => f.nsc_per_year,
            FeeModel::PerRecovery(f) => f.nsc_per_recovery, // 1 kurtarma varsayımı
            FeeModel::Hybrid(f) => f.annual_base + f.per_recovery_bonus,
        }
    }

    /// Kurtarma başına ek maliyet
    pub fn recovery_cost(&self) -> u128 {
        match self {
            FeeModel::Free => 0,
            FeeModel::Annual(_) => 0, // yıllık ücrete dahil
            FeeModel::PerRecovery(f) => f.nsc_per_recovery,
            FeeModel::Hybrid(f) => f.per_recovery_bonus,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const NSC: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn test_free_model_zero_cost() {
        assert_eq!(FeeModel::Free.estimated_annual_cost(), 0);
        assert_eq!(FeeModel::Free.recovery_cost(), 0);
    }

    #[test]
    fn test_annual_fee_model() {
        let model = FeeModel::Annual(AnnualFee { nsc_per_year: 50 * NSC });
        assert_eq!(model.estimated_annual_cost(), 50 * NSC);
        assert_eq!(model.recovery_cost(), 0);
    }

    #[test]
    fn test_per_recovery_model() {
        let model = FeeModel::PerRecovery(PerRecoveryFee { nsc_per_recovery: 500 * NSC });
        assert_eq!(model.recovery_cost(), 500 * NSC);
    }

    #[test]
    fn test_hybrid_model() {
        let model = FeeModel::Hybrid(HybridFee {
            annual_base: 20 * NSC,
            per_recovery_bonus: 200 * NSC,
        });
        assert_eq!(model.estimated_annual_cost(), 220 * NSC);
        assert_eq!(model.recovery_cost(), 200 * NSC);
    }
}
