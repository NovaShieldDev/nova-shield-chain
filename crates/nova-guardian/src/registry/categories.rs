//! Guardian kategorileri (§5.8.2)
//!
//! Kategori 1 — Bireysel (kişisel ilişki, stake zorunlu değil)
//! Kategori 2 — Sertifikalı bireysel (NSC stake, itibar skoru var)
//! Kategori 3 — Kurumsal (KYC/AML, sigorta fonu katkısı, SLA)
//! Kategori 4 — DAO (akıllı kontrat tabanlı, on-chain yönetişim)

use serde::{Deserialize, Serialize};

// ─── Minimum Stake Miktarları ─────────────────────────────────────────────────

/// Sertifikalı bireysel için minimum stake (1,000 NSC)
pub const MIN_STAKE_CERTIFIED_INDIVIDUAL: u128 = 1_000 * 1_000_000_000_000_000_000;
/// Kurumsal guardian için minimum stake (10,000 NSC)
pub const MIN_STAKE_INSTITUTIONAL: u128 = 10_000 * 1_000_000_000_000_000_000;
/// DAO guardian için minimum stake (5,000 NSC)
pub const MIN_STAKE_DAO: u128 = 5_000 * 1_000_000_000_000_000_000;

// ─── GuardianCategory ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianCategory {
    /// Kişisel ilişkiye dayalı (aile, arkadaş)
    Personal,
    /// NSC stake etmiş, itibar skoru olan bireyler
    CertifiedIndividual,
    /// KYC/AML uyumlu kurumlar (bankalar, hukuk firmaları)
    Institutional,
    /// Akıllı kontrat tabanlı DAO guardian
    Dao,
}

impl GuardianCategory {
    pub fn name(&self) -> &'static str {
        match self {
            GuardianCategory::Personal => "personal",
            GuardianCategory::CertifiedIndividual => "certified-individual",
            GuardianCategory::Institutional => "institutional",
            GuardianCategory::Dao => "dao",
        }
    }

    /// Bu kategori için minimum stake miktarı
    pub fn min_stake(&self) -> u128 {
        match self {
            GuardianCategory::Personal => 0,
            GuardianCategory::CertifiedIndividual => MIN_STAKE_CERTIFIED_INDIVIDUAL,
            GuardianCategory::Institutional => MIN_STAKE_INSTITUTIONAL,
            GuardianCategory::Dao => MIN_STAKE_DAO,
        }
    }

    /// Sigorta fonu katkısı gerekli mi?
    pub fn requires_insurance_contribution(&self) -> bool {
        matches!(self, GuardianCategory::Institutional)
    }

    /// KYC/AML zorunlu mu?
    pub fn requires_kyc(&self) -> bool {
        matches!(self, GuardianCategory::Institutional)
    }

    /// SLA yazılabilir mi?
    pub fn supports_sla(&self) -> bool {
        matches!(self, GuardianCategory::Institutional | GuardianCategory::CertifiedIndividual)
    }

    /// Ücretli hizmet verebilir mi?
    pub fn can_charge_fees(&self) -> bool {
        !matches!(self, GuardianCategory::Personal)
    }
}

// ─── KycStatus ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KycStatus {
    NotRequired,
    Pending,
    Verified,
    Rejected,
}

impl KycStatus {
    pub fn is_verified_or_not_required(&self) -> bool {
        matches!(self, KycStatus::Verified | KycStatus::NotRequired)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_min_stakes() {
        assert_eq!(GuardianCategory::Personal.min_stake(), 0);
        assert!(GuardianCategory::CertifiedIndividual.min_stake() > 0);
        assert!(
            GuardianCategory::Institutional.min_stake()
                > GuardianCategory::CertifiedIndividual.min_stake()
        );
    }

    #[test]
    fn test_insurance_requirement() {
        assert!(GuardianCategory::Institutional.requires_insurance_contribution());
        assert!(!GuardianCategory::CertifiedIndividual.requires_insurance_contribution());
        assert!(!GuardianCategory::Personal.requires_insurance_contribution());
    }

    #[test]
    fn test_fee_permission() {
        assert!(!GuardianCategory::Personal.can_charge_fees());
        assert!(GuardianCategory::CertifiedIndividual.can_charge_fees());
        assert!(GuardianCategory::Institutional.can_charge_fees());
    }

    #[test]
    fn test_kyc_status() {
        assert!(KycStatus::Verified.is_verified_or_not_required());
        assert!(KycStatus::NotRequired.is_verified_or_not_required());
        assert!(!KycStatus::Pending.is_verified_or_not_required());
    }
}
