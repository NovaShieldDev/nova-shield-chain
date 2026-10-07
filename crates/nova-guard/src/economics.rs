//! Protocol fee ekonomisi — §5.6.5
//!
//! "Nova Shield Guard kullanımı için küçük bir protocol fee
//!  BTC/ETH güvenliğini nova shield'de tutmak için yıllık %0.05 fee
//!  Bu fee NSC staker'lara ve guardian'lara dağıtılır"

use crate::errors::{GuardError, GuardResult};
use crate::types::ChainId;
use serde::{Deserialize, Serialize};

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Yıllık fee oranı: %0.05 = 5 bps
pub const ANNUAL_FEE_BPS: u64 = 5;

/// Bps böleni
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Bir yılda slot sayısı (400ms slot → 365*24*3600*1000/400 = 78_840_000)
pub const SLOTS_PER_YEAR: u64 = 78_840_000;

/// Staker payı: %60 (6000 bps)
pub const STAKER_SHARE_BPS: u64 = 6_000;

/// Guardian payı: %40 (4000 bps)
pub const GUARDIAN_SHARE_BPS: u64 = 4_000;

// ─── FeeCalculation ───────────────────────────────────────────────────────────

/// Fee hesabı sonucu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeCalculation {
    /// Vault değeri (USD cinsinden, 6 ondalık hassasiyet)
    pub vault_value_usd: u64,
    /// Yıllık fee (USD)
    pub annual_fee_usd: u64,
    /// Slot başına fee (NSC)
    pub fee_per_slot: u64,
    /// NSC/USD oranı (kullanılan)
    pub nsc_price_usd: u64,
    /// Staker payı (yıllık, NSC)
    pub staker_annual_share: u64,
    /// Guardian payı (yıllık, NSC)
    pub guardian_annual_share: u64,
}

// ─── ProtocolFeeCalculator ────────────────────────────────────────────────────

/// Protocol fee hesaplayıcı — §5.6.5
pub struct ProtocolFeeCalculator {
    /// NSC/USD fiyatı (6 ondalık, ör. 1_000_000 = $1.00)
    nsc_price_usd_6dec: u64,
}

impl ProtocolFeeCalculator {
    pub fn new(nsc_price_usd_6dec: u64) -> Self {
        Self { nsc_price_usd_6dec }
    }

    /// Varsayılan: NSC = $1.00
    pub fn default_price() -> Self {
        Self::new(1_000_000)
    }

    /// Yıllık fee hesapla
    /// vault_value_usd_6dec: vault değeri (6 ondalık USD, ör. 10_000 * 1_000_000 = $10,000)
    pub fn calculate(&self, vault_value_usd_6dec: u64) -> GuardResult<FeeCalculation> {
        if self.nsc_price_usd_6dec == 0 {
            return Err(GuardError::FeeCalculationError {
                reason: "NSC fiyatı sıfır".into(),
            });
        }

        // Yıllık fee: vault_value * 0.05% = vault_value * 5 / 10000
        let annual_fee_usd_6dec = vault_value_usd_6dec
            .saturating_mul(ANNUAL_FEE_BPS)
            / BPS_DENOMINATOR;

        // NSC cinsinden fee
        let annual_fee_nsc = annual_fee_usd_6dec
            .saturating_mul(1_000_000)
            / self.nsc_price_usd_6dec;

        // Slot başına fee
        let fee_per_slot = annual_fee_nsc / SLOTS_PER_YEAR;

        // Staker / guardian payları
        let staker_share = annual_fee_nsc.saturating_mul(STAKER_SHARE_BPS) / BPS_DENOMINATOR;
        let guardian_share = annual_fee_nsc.saturating_mul(GUARDIAN_SHARE_BPS) / BPS_DENOMINATOR;

        Ok(FeeCalculation {
            vault_value_usd: vault_value_usd_6dec / 1_000_000,
            annual_fee_usd: annual_fee_usd_6dec / 1_000_000,
            fee_per_slot,
            nsc_price_usd: self.nsc_price_usd_6dec / 1_000_000,
            staker_annual_share: staker_share,
            guardian_annual_share: guardian_share,
        })
    }

    /// Zincire özgü fee çarpanı
    /// BTC/ETH için %100, SOL için %80, Cosmos için %60
    pub fn chain_multiplier(chain: &ChainId) -> u64 {
        match chain {
            ChainId::Bitcoin | ChainId::Ethereum => 100,
            ChainId::Solana => 80,
            ChainId::Cosmos => 60,
        }
    }

    /// Zincire özgü fee hesapla
    pub fn calculate_for_chain(
        &self,
        vault_value_usd_6dec: u64,
        chain: &ChainId,
    ) -> GuardResult<FeeCalculation> {
        let multiplier = Self::chain_multiplier(chain);
        let adjusted_value = vault_value_usd_6dec.saturating_mul(multiplier) / 100;
        self.calculate(adjusted_value)
    }
}

// ─── FeeDistribution ──────────────────────────────────────────────────────────

/// Fee dağıtım kaydı — yıl sonu dağıtım
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeDistribution {
    /// Dağıtım yapılan slot
    pub distribution_slot: u64,
    /// Toplam toplanan fee (NSC)
    pub total_collected: u64,
    /// Staker'lara dağıtılan
    pub staker_distribution: u64,
    /// Guardian'lara dağıtılan
    pub guardian_distribution: u64,
    /// Hazineye kalan (rounding artığı)
    pub treasury_remainder: u64,
}

impl FeeDistribution {
    pub fn new(distribution_slot: u64, total_collected: u64) -> Self {
        let staker = total_collected.saturating_mul(STAKER_SHARE_BPS) / BPS_DENOMINATOR;
        let guardian = total_collected.saturating_mul(GUARDIAN_SHARE_BPS) / BPS_DENOMINATOR;
        let remainder = total_collected.saturating_sub(staker + guardian);

        Self {
            distribution_slot,
            total_collected,
            staker_distribution: staker,
            guardian_distribution: guardian,
            treasury_remainder: remainder,
        }
    }

    pub fn verify_total(&self) -> bool {
        self.staker_distribution + self.guardian_distribution + self.treasury_remainder
            == self.total_collected
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn calc() -> ProtocolFeeCalculator {
        ProtocolFeeCalculator::default_price()
    }

    #[test]
    fn test_annual_fee_rate() {
        // $10,000 vault → %0.05 = $5 yıllık fee
        let result = calc().calculate(10_000 * 1_000_000).unwrap();
        assert_eq!(result.vault_value_usd, 10_000);
        assert_eq!(result.annual_fee_usd, 5); // $5
    }

    #[test]
    fn test_annual_fee_1m() {
        // $1,000,000 vault → %0.05 = $500 yıllık fee
        let result = calc().calculate(1_000_000 * 1_000_000).unwrap();
        assert_eq!(result.annual_fee_usd, 500);
    }

    #[test]
    fn test_fee_per_slot_positive() {
        let result = calc().calculate(10_000_000 * 1_000_000).unwrap(); // $10M vault
        // Pozitif olmalı
        assert!(result.fee_per_slot > 0);
    }

    #[test]
    fn test_staker_guardian_split() {
        let result = calc().calculate(1_000_000 * 1_000_000).unwrap();
        // Staker 60%, guardian 40%
        assert!(result.staker_annual_share > result.guardian_annual_share);
        let ratio = result.staker_annual_share * 100 / result.guardian_annual_share;
        assert!(ratio >= 148 && ratio <= 152); // 60/40 = 1.5
    }

    #[test]
    fn test_chain_multiplier() {
        assert_eq!(ProtocolFeeCalculator::chain_multiplier(&ChainId::Bitcoin), 100);
        assert_eq!(ProtocolFeeCalculator::chain_multiplier(&ChainId::Ethereum), 100);
        assert_eq!(ProtocolFeeCalculator::chain_multiplier(&ChainId::Solana), 80);
        assert_eq!(ProtocolFeeCalculator::chain_multiplier(&ChainId::Cosmos), 60);
    }

    #[test]
    fn test_calculate_for_chain_sol_lower() {
        let base = calc().calculate(1_000_000 * 1_000_000).unwrap();
        let sol = calc().calculate_for_chain(1_000_000 * 1_000_000, &ChainId::Solana).unwrap();
        // SOL fee: BTC/ETH'den düşük
        assert!(sol.annual_fee_usd < base.annual_fee_usd);
    }

    #[test]
    fn test_zero_price_error() {
        let calc = ProtocolFeeCalculator::new(0);
        assert!(matches!(calc.calculate(1_000_000), Err(GuardError::FeeCalculationError { .. })));
    }

    #[test]
    fn test_fee_constants() {
        assert_eq!(ANNUAL_FEE_BPS, 5);
        assert_eq!(STAKER_SHARE_BPS + GUARDIAN_SHARE_BPS, 10_000);
    }

    #[test]
    fn test_fee_distribution_verify_total() {
        let dist = FeeDistribution::new(1000, 1_000_000);
        assert!(dist.verify_total());
        assert_eq!(dist.staker_distribution, 600_000); // 60%
        assert_eq!(dist.guardian_distribution, 400_000); // 40%
    }

    #[test]
    fn test_slots_per_year_constant() {
        // 365 gün * 24 saat * 3600 saniye * 1000ms / 400ms = 78_840_000
        assert_eq!(SLOTS_PER_YEAR, 78_840_000);
    }
}
