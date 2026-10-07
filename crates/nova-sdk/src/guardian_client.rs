//! Guardian SDK — marketplace arama, sözleşme, itibar
//!
//! §5.8 Guardian Marketplace ve İtibar Sistemi

use crate::errors::{SdkError, SdkResult};
use serde::{Deserialize, Serialize};

// ─── GuardianCategory ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianCategory {
    Personal,
    Institutional,
    Dao,
    Hardware,
}

// ─── GuardianListing ─────────────────────────────────────────────────────────

/// Guardian marketplace listesi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianListing {
    pub address: [u8; 20],
    pub name: String,
    pub category: GuardianCategory,
    /// Yıllık ücret (NSC, en küçük birim)
    pub annual_fee: u64,
    /// KYC doğrulandı mı?
    pub kyc_verified: bool,
    /// İtibar puanı (0-100)
    pub reputation_score: u8,
    /// Başarılı kurtarma sayısı
    pub successful_recoveries: u64,
    /// Yanıt süresi SLA (slot)
    pub response_time_slots: u64,
    /// Aktif mi?
    pub active: bool,
}

impl GuardianListing {
    pub fn is_eligible(&self) -> bool {
        self.active && self.kyc_verified && self.reputation_score >= 50
    }
}

// ─── GuardianFilter ──────────────────────────────────────────────────────────

/// Arama filtresi
#[derive(Debug, Clone, Default)]
pub struct GuardianFilter {
    pub category: Option<GuardianCategory>,
    pub max_annual_fee: Option<u64>,
    pub min_reputation: Option<u8>,
    pub kyc_only: bool,
}

impl GuardianFilter {
    pub fn matches(&self, listing: &GuardianListing) -> bool {
        if let Some(cat) = &self.category {
            if &listing.category != cat {
                return false;
            }
        }
        if let Some(max_fee) = self.max_annual_fee {
            if listing.annual_fee > max_fee {
                return false;
            }
        }
        if let Some(min_rep) = self.min_reputation {
            if listing.reputation_score < min_rep {
                return false;
            }
        }
        if self.kyc_only && !listing.kyc_verified {
            return false;
        }
        true
    }
}

// ─── GuardianContract ────────────────────────────────────────────────────────

/// Guardian sözleşmesi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianContract {
    pub contract_id: u64,
    pub user: [u8; 20],
    pub guardian: [u8; 20],
    pub annual_fee: u64,
    pub prepaid_slots: u64,
    pub created_at_slot: u64,
    pub active: bool,
}

// ─── GuardianMarketplace ─────────────────────────────────────────────────────

/// Guardian marketplace istemcisi
pub struct GuardianMarketplace {
    listings: Vec<GuardianListing>,
    contracts: Vec<GuardianContract>,
    contract_counter: u64,
}

impl GuardianMarketplace {
    pub fn new() -> Self {
        Self { listings: vec![], contracts: vec![], contract_counter: 1 }
    }

    /// Guardian listesi ekle (mock: marketplace'e kayıt)
    pub fn register_guardian(&mut self, listing: GuardianListing) {
        self.listings.push(listing);
    }

    /// Guardian ara
    pub fn search(&self, filter: &GuardianFilter) -> Vec<&GuardianListing> {
        self.listings.iter()
            .filter(|l| filter.matches(l) && l.active)
            .collect()
    }

    /// En iyi guardian'ı bul (itibar puanına göre)
    pub fn top_guardians(&self, n: usize, filter: &GuardianFilter) -> Vec<&GuardianListing> {
        let mut matched: Vec<&GuardianListing> = self.search(filter);
        matched.sort_by(|a, b| b.reputation_score.cmp(&a.reputation_score));
        matched.truncate(n);
        matched
    }

    /// Sözleşme oluştur
    pub fn create_contract(
        &mut self,
        user: [u8; 20],
        guardian_addr: [u8; 20],
        current_slot: u64,
    ) -> SdkResult<u64> {
        let listing = self.listings.iter()
            .find(|l| l.address == guardian_addr)
            .ok_or(SdkError::GuardianNotFound)?;

        if !listing.is_eligible() {
            return Err(SdkError::GuardianNotFound);
        }

        let contract = GuardianContract {
            contract_id: self.contract_counter,
            user,
            guardian: guardian_addr,
            annual_fee: listing.annual_fee,
            prepaid_slots: 0,
            created_at_slot: current_slot,
            active: true,
        };
        self.contract_counter += 1;
        self.contracts.push(contract);
        Ok(self.contract_counter - 1)
    }

    pub fn listing_count(&self) -> usize {
        self.listings.len()
    }

    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }

    pub fn get_contract(&self, id: u64) -> Option<&GuardianContract> {
        self.contracts.iter().find(|c| c.contract_id == id)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_listing(
        n: u8,
        cat: GuardianCategory,
        fee: u64,
        rep: u8,
        kyc: bool,
    ) -> GuardianListing {
        GuardianListing {
            address: [n; 20],
            name: format!("Guardian-{}", n),
            category: cat,
            annual_fee: fee,
            kyc_verified: kyc,
            reputation_score: rep,
            successful_recoveries: 5,
            response_time_slots: 100,
            active: true,
        }
    }

    fn user() -> [u8; 20] {
        [0xAAu8; 20]
    }

    #[test]
    fn test_guardian_listing_eligible() {
        let l = make_listing(1, GuardianCategory::Institutional, 1_000, 80, true);
        assert!(l.is_eligible());
    }

    #[test]
    fn test_guardian_listing_not_eligible_low_rep() {
        let l = make_listing(1, GuardianCategory::Institutional, 1_000, 30, true);
        assert!(!l.is_eligible());
    }

    #[test]
    fn test_guardian_listing_not_eligible_no_kyc() {
        let l = make_listing(1, GuardianCategory::Institutional, 1_000, 80, false);
        assert!(!l.is_eligible());
    }

    #[test]
    fn test_guardian_filter_category() {
        let filter = GuardianFilter {
            category: Some(GuardianCategory::Institutional),
            ..Default::default()
        };
        let l_inst = make_listing(1, GuardianCategory::Institutional, 1000, 80, true);
        let l_pers = make_listing(2, GuardianCategory::Personal, 1000, 80, true);
        assert!(filter.matches(&l_inst));
        assert!(!filter.matches(&l_pers));
    }

    #[test]
    fn test_guardian_filter_fee() {
        let filter = GuardianFilter {
            max_annual_fee: Some(500),
            ..Default::default()
        };
        let cheap = make_listing(1, GuardianCategory::Personal, 300, 80, true);
        let expensive = make_listing(2, GuardianCategory::Personal, 800, 80, true);
        assert!(filter.matches(&cheap));
        assert!(!filter.matches(&expensive));
    }

    #[test]
    fn test_guardian_filter_kyc_only() {
        let filter = GuardianFilter { kyc_only: true, ..Default::default() };
        let kyc = make_listing(1, GuardianCategory::Personal, 500, 80, true);
        let no_kyc = make_listing(2, GuardianCategory::Personal, 500, 80, false);
        assert!(filter.matches(&kyc));
        assert!(!filter.matches(&no_kyc));
    }

    #[test]
    fn test_marketplace_register_and_search() {
        let mut mp = GuardianMarketplace::new();
        mp.register_guardian(make_listing(1, GuardianCategory::Institutional, 1000, 90, true));
        mp.register_guardian(make_listing(2, GuardianCategory::Personal, 200, 70, true));
        mp.register_guardian(make_listing(3, GuardianCategory::Institutional, 1500, 85, true));

        let filter = GuardianFilter {
            category: Some(GuardianCategory::Institutional),
            ..Default::default()
        };
        let results = mp.search(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_marketplace_top_guardians() {
        let mut mp = GuardianMarketplace::new();
        for i in 0u8..5 {
            mp.register_guardian(make_listing(i + 1, GuardianCategory::Personal, 500, i * 20, true));
        }
        let top3 = mp.top_guardians(3, &Default::default());
        assert_eq!(top3.len(), 3);
        // İtibar puanına göre sıralı
        assert!(top3[0].reputation_score >= top3[1].reputation_score);
    }

    #[test]
    fn test_marketplace_create_contract() {
        let mut mp = GuardianMarketplace::new();
        mp.register_guardian(make_listing(1, GuardianCategory::Institutional, 1000, 90, true));

        let contract_id = mp.create_contract(user(), [1u8; 20], 100).unwrap();
        assert_eq!(mp.contract_count(), 1);

        let contract = mp.get_contract(contract_id).unwrap();
        assert_eq!(contract.guardian, [1u8; 20]);
        assert_eq!(contract.user, user());
    }

    #[test]
    fn test_marketplace_guardian_not_found() {
        let mut mp = GuardianMarketplace::new();
        let result = mp.create_contract(user(), [0xFFu8; 20], 100);
        assert!(matches!(result, Err(SdkError::GuardianNotFound)));
    }

    #[test]
    fn test_marketplace_ineligible_guardian() {
        let mut mp = GuardianMarketplace::new();
        // Düşük itibar → eligible değil
        mp.register_guardian(make_listing(1, GuardianCategory::Personal, 500, 20, true));
        let result = mp.create_contract(user(), [1u8; 20], 100);
        assert!(matches!(result, Err(SdkError::GuardianNotFound)));
    }
}
