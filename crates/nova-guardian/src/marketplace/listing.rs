//! Guardian listeleme — marketplace'te görünür kayıt

use crate::fees::models::FeeModel;
use crate::registry::categories::GuardianCategory;
use crate::registry::verification::SlaTerms;
use nova_primitives::Address;
use serde::{Deserialize, Serialize};

// ─── GuardianListing ──────────────────────────────────────────────────────────

/// Marketplace'te yayınlanan guardian ilanı
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardianListing {
    pub guardian_id: Address,
    pub category: GuardianCategory,
    /// Hesaplanan Trust Score (0-100)
    pub trust_score: u32,
    /// Ücret modeli
    pub fee_model: FeeModel,
    /// SLA koşulları (opsiyonel)
    pub sla: Option<SlaTerms>,
    /// Kaç hesabı daha kabul edebilir
    pub available_slots: u32,
    /// Kısa tanıtım metni
    pub description: String,
    /// İlan oluşturulma slot'u
    pub listed_at_slot: u64,
}

impl GuardianListing {
    pub fn new(
        guardian_id: Address,
        category: GuardianCategory,
        trust_score: u32,
        fee_model: FeeModel,
        available_slots: u32,
        description: String,
        listed_at_slot: u64,
    ) -> Self {
        Self {
            guardian_id,
            category,
            trust_score,
            fee_model,
            sla: None,
            available_slots,
            description,
            listed_at_slot,
        }
    }

    pub fn with_sla(mut self, sla: SlaTerms) -> Self {
        self.sla = Some(sla);
        self
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fees::models::{AnnualFee, FeeModel};

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_guardian_listing_creation() {
        let listing = GuardianListing::new(
            addr(1),
            GuardianCategory::CertifiedIndividual,
            85,
            FeeModel::Annual(AnnualFee { nsc_per_year: 50 * 1_000_000_000_000_000_000 }),
            10,
            "Deneyimli bireysel guardian".to_string(),
            1000,
        );
        assert_eq!(listing.trust_score, 85);
        assert!(listing.sla.is_none());
    }

    #[test]
    fn test_listing_with_sla() {
        let sla = SlaTerms::new(216_000, 9_500, 100);
        let listing = GuardianListing::new(
            addr(2),
            GuardianCategory::Institutional,
            95,
            FeeModel::Free,
            50,
            "Kurumsal guardian".to_string(),
            0,
        )
        .with_sla(sla);
        assert!(listing.sla.is_some());
    }
}
