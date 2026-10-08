//! Marketplace arama ve filtreleme

use crate::marketplace::listing::GuardianListing;
use crate::registry::categories::GuardianCategory;
use nova_primitives::constants::GUARDIAN_MIN_TRUST_SCORE;
use serde::{Deserialize, Serialize};

// ─── SearchFilter ─────────────────────────────────────────────────────────────

/// Arama filtresi
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilter {
    pub min_trust_score: Option<u32>,
    pub category: Option<GuardianCategory>,
    pub max_annual_fee_nsc: Option<u128>,
    pub requires_sla: bool,
    pub min_available_slots: Option<u32>,
}

impl SearchFilter {
    /// ## NOVA-RED-PRIM-002 Güvenlik Düzeltmesi
    ///
    /// Varsayılan min_trust_score = GUARDIAN_MIN_TRUST_SCORE (60).
    /// Düşük güvenilirlikli guardian'lar otomatik filtrelenir.
    pub fn new() -> Self {
        Self {
            min_trust_score: Some(GUARDIAN_MIN_TRUST_SCORE as u32),
            ..Self::default()
        }
    }

    pub fn with_min_trust_score(mut self, score: u32) -> Self {
        self.min_trust_score = Some(score);
        self
    }

    pub fn with_category(mut self, category: GuardianCategory) -> Self {
        self.category = Some(category);
        self
    }

    pub fn with_max_fee(mut self, max_fee: u128) -> Self {
        self.max_annual_fee_nsc = Some(max_fee);
        self
    }

    pub fn with_sla(mut self) -> Self {
        self.requires_sla = true;
        self
    }

    /// Bir listing filtreyle eşleşiyor mu?
    pub fn matches(&self, listing: &GuardianListing) -> bool {
        if let Some(min_score) = self.min_trust_score {
            if listing.trust_score < min_score {
                return false;
            }
        }
        if let Some(ref cat) = self.category {
            if &listing.category != cat {
                return false;
            }
        }
        if self.requires_sla && listing.sla.is_none() {
            return false;
        }
        if let Some(max_fee) = self.max_annual_fee_nsc {
            if listing.fee_model.estimated_annual_cost() > max_fee {
                return false;
            }
        }
        if let Some(min_slots) = self.min_available_slots {
            if listing.available_slots < min_slots {
                return false;
            }
        }
        true
    }
}

// ─── MarketplaceSearch ────────────────────────────────────────────────────────

pub struct MarketplaceSearch;

impl MarketplaceSearch {
    /// Listingleri filtrele ve güven skoruna göre sırala
    pub fn search(listings: &[GuardianListing], filter: &SearchFilter) -> Vec<GuardianListing> {
        let mut results: Vec<GuardianListing> =
            listings.iter().filter(|l| filter.matches(l)).cloned().collect();
        // Yüksek Trust Score önce
        results.sort_by(|a, b| b.trust_score.cmp(&a.trust_score));
        results
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fees::models::FeeModel;
    use crate::registry::verification::SlaTerms;
    use nova_primitives::Address;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_listing(n: u8, category: GuardianCategory, trust_score: u32) -> GuardianListing {
        GuardianListing::new(addr(n), category, trust_score, FeeModel::Free, 10, "".to_string(), 0)
    }

    #[test]
    fn test_search_by_trust_score() {
        let listings = vec![
            make_listing(1, GuardianCategory::Personal, 80),
            make_listing(2, GuardianCategory::Personal, 50),
            make_listing(3, GuardianCategory::Personal, 90),
        ];
        let filter = SearchFilter::new().with_min_trust_score(75);
        let results = MarketplaceSearch::search(&listings, &filter);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].trust_score, 90); // sıralı
    }

    #[test]
    fn test_search_by_category() {
        let listings = vec![
            make_listing(1, GuardianCategory::Personal, 70),
            make_listing(2, GuardianCategory::Institutional, 90),
            make_listing(3, GuardianCategory::Personal, 60),
        ];
        let filter = SearchFilter::new().with_category(GuardianCategory::Institutional);
        let results = MarketplaceSearch::search(&listings, &filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].guardian_id, addr(2));
    }

    #[test]
    fn test_search_requires_sla() {
        let sla = SlaTerms::new(216_000, 9_500, 100);
        let mut l1 = make_listing(1, GuardianCategory::Institutional, 95);
        l1.sla = Some(sla);
        let l2 = make_listing(2, GuardianCategory::Institutional, 80);

        let filter = SearchFilter::new().with_sla();
        let results = MarketplaceSearch::search(&[l1, l2], &filter);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_by_max_annual_fee() {
        use crate::fees::models::{AnnualFee, FeeModel};
        const NSC: u128 = 1_000_000_000_000_000_000;

        let cheap = GuardianListing::new(
            addr(1), GuardianCategory::Personal, 80,
            FeeModel::Annual(AnnualFee { nsc_per_year: 10 * NSC }), 10, "".to_string(), 0,
        );
        let expensive = GuardianListing::new(
            addr(2), GuardianCategory::Personal, 90,
            FeeModel::Annual(AnnualFee { nsc_per_year: 100 * NSC }), 10, "".to_string(), 0,
        );
        let free = GuardianListing::new(
            addr(3), GuardianCategory::Personal, 70,
            FeeModel::Free, 10, "".to_string(), 0,
        );

        let filter = SearchFilter::new().with_max_fee(50 * NSC);
        let results = MarketplaceSearch::search(&[cheap, expensive, free], &filter);

        assert_eq!(results.len(), 2, "pahalı guardian filtrelenmeli: cheap + free");
        assert!(!results.iter().any(|r| r.guardian_id == addr(2)), "pahalı guardian sonuçta olmamalı");
    }

    #[test]
    fn test_search_empty_result() {
        let listings = vec![make_listing(1, GuardianCategory::Personal, 40)];
        let filter = SearchFilter::new().with_min_trust_score(90);
        let results = MarketplaceSearch::search(&listings, &filter);
        assert!(results.is_empty());
    }
}
