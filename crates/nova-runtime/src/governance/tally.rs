//! Governance Tally — oy sayımı (Cosmos SDK x/gov tally.go'dan birebir port)
//!
//! Cosmos SDK Tally parametreleri (Nova Shield için aynı):
//!   Quorum:     %33.4 → oylama geçerli kabul edilir
//!   Threshold:  %50   → çoğunluk kabul edilir
//!   Veto:       %33.4 → NoWithVeto bu eşiği geçerse reddedilir + depozito yakılır

use crate::governance::proposal::{ProposalStatus, TallyResult};
use crate::tx::types::VoteOption;
use nova_primitives::Address;
use std::collections::HashMap;

// ─── Tally Parametreleri ──────────────────────────────────────────────────────

/// Minimum katılım eşiği: %33.4 → 334 BPS (10_000 bazında)
pub const QUORUM_BPS: u32 = 3_340;

/// Kabul eşiği: %50
pub const THRESHOLD_BPS: u32 = 5_000;

/// Veto eşiği: %33.4
pub const VETO_THRESHOLD_BPS: u32 = 3_340;

/// Hızlandırılmış kabul eşiği: %66.7
pub const EXPEDITED_THRESHOLD_BPS: u32 = 6_670;

// ─── TallyEngine ──────────────────────────────────────────────────────────────

/// TallyEngine — oy sayımı motoru
pub struct TallyEngine;

impl TallyEngine {
    /// Tüm oyları say ve sonucu hesapla
    pub fn tally(
        votes: &HashMap<Address, (VoteOption, u128)>,
        total_bonded_stake: u128,
    ) -> TallyResult {
        let mut result = TallyResult::default();
        result.total_bonded = total_bonded_stake;

        for (_, (option, power)) in votes {
            result.total_voted += power;
            match option {
                VoteOption::Yes => result.yes_power += power,
                VoteOption::No => result.no_power += power,
                VoteOption::Abstain => result.abstain_power += power,
                VoteOption::NoWithVeto => result.no_with_veto_power += power,
            }
        }

        result
    }

    /// Tally sonucuna göre öneri durumunu belirle
    /// Döndürür: (yeni durum, depozito yakılsın mı?)
    pub fn evaluate(tally: &TallyResult, expedited: bool) -> (ProposalStatus, bool) {
        // 1. Quorum kontrolü: toplam oy / toplam stake >= %33.4
        if tally.total_bonded == 0 {
            return (ProposalStatus::Rejected, false);
        }

        let percent_voted = tally.total_voted * 10_000 / tally.total_bonded;
        if percent_voted < QUORUM_BPS as u128 {
            return (ProposalStatus::Rejected, false); // Quorum yok
        }

        // 2. Herkes çekimser mi?
        let non_abstain = tally.total_non_abstain();
        if non_abstain == 0 {
            return (ProposalStatus::Rejected, false);
        }

        // 3. Veto kontrolü: NoWithVeto / total_voted > %33.4
        let veto_ratio = tally.no_with_veto_power * 10_000 / tally.total_voted;
        if veto_ratio > VETO_THRESHOLD_BPS as u128 {
            return (ProposalStatus::Vetoed, true); // Depozito yakılır
        }

        // 4. Çoğunluk kontrolü: Yes / non_abstain > threshold
        let threshold = if expedited {
            EXPEDITED_THRESHOLD_BPS as u128
        } else {
            THRESHOLD_BPS as u128
        };

        let yes_ratio = tally.yes_power * 10_000 / non_abstain;
        if yes_ratio > threshold {
            (ProposalStatus::Passed, false)
        } else {
            (ProposalStatus::Rejected, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_votes(
        yes: u128,
        no: u128,
        abstain: u128,
        veto: u128,
    ) -> HashMap<Address, (VoteOption, u128)> {
        let mut votes = HashMap::new();
        if yes > 0 {
            votes.insert(addr(1), (VoteOption::Yes, yes));
        }
        if no > 0 {
            votes.insert(addr(2), (VoteOption::No, no));
        }
        if abstain > 0 {
            votes.insert(addr(3), (VoteOption::Abstain, abstain));
        }
        if veto > 0 {
            votes.insert(addr(4), (VoteOption::NoWithVeto, veto));
        }
        votes
    }

    #[test]
    fn test_tally_basic() {
        let votes = make_votes(600, 200, 100, 100);
        let result = TallyEngine::tally(&votes, 1_000);
        assert_eq!(result.yes_power, 600);
        assert_eq!(result.no_power, 200);
        assert_eq!(result.abstain_power, 100);
        assert_eq!(result.no_with_veto_power, 100);
        assert_eq!(result.total_voted, 1_000);
    }

    #[test]
    fn test_tally_passes() {
        // Quorum: 1000/1000 = 100% ✓
        // Yes/non-abstain = 600/(600+200+100) = 66% > 50% ✓
        let votes = make_votes(600, 200, 100, 100);
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, burn) = TallyEngine::evaluate(&tally, false);
        assert!(matches!(status, ProposalStatus::Passed));
        assert!(!burn);
    }

    #[test]
    fn test_tally_rejected_no_quorum() {
        // Sadece 100/1000 oy kullanıldı → quorum yok
        let votes = make_votes(100, 0, 0, 0);
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, burn) = TallyEngine::evaluate(&tally, false);
        assert!(matches!(status, ProposalStatus::Rejected));
        assert!(!burn);
    }

    #[test]
    fn test_tally_vetoed() {
        // Toplam 1000 oy, %34 NoWithVeto → veto eşiği aşıldı
        let votes = make_votes(400, 260, 0, 340); // 340/1000 = 34% > 33.4%
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, burn) = TallyEngine::evaluate(&tally, false);
        assert!(matches!(status, ProposalStatus::Vetoed));
        assert!(burn); // Depozito yakılır
    }

    #[test]
    fn test_tally_rejected_insufficient_yes() {
        // Quorum var ama Yes < %50
        let votes = make_votes(400, 600, 0, 0); // Yes %40
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, _) = TallyEngine::evaluate(&tally, false);
        assert!(matches!(status, ProposalStatus::Rejected));
    }

    #[test]
    fn test_tally_abstain_excluded() {
        // Quorum var: 1000/1000
        // Yes/(non-abstain) = 500/(500+400) = 55.6% > 50% → Geçti
        let votes = make_votes(500, 400, 100, 0);
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, _) = TallyEngine::evaluate(&tally, false);
        assert!(matches!(status, ProposalStatus::Passed));
    }

    #[test]
    fn test_expedited_threshold() {
        // Expedited: Yes/(non-abstain) > %66.7
        let votes = make_votes(700, 300, 0, 0); // 700/1000 = 70% > 66.7%
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, _) = TallyEngine::evaluate(&tally, true);
        assert!(matches!(status, ProposalStatus::Passed));
    }

    #[test]
    fn test_expedited_threshold_not_met() {
        // Expedited: Yes/(non-abstain) = 60% < 66.7% → Reddedildi
        let votes = make_votes(600, 400, 0, 0);
        let tally = TallyEngine::tally(&votes, 1_000);
        let (status, _) = TallyEngine::evaluate(&tally, true);
        assert!(matches!(status, ProposalStatus::Rejected));
    }

    #[test]
    fn test_quorum_constants() {
        assert_eq!(QUORUM_BPS, 3_340); // %33.4
        assert_eq!(THRESHOLD_BPS, 5_000); // %50
        assert_eq!(VETO_THRESHOLD_BPS, 3_340); // %33.4
        assert_eq!(EXPEDITED_THRESHOLD_BPS, 6_670); // %66.7
    }
}
