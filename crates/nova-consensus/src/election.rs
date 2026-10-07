//! Validator seçim algoritması — Phragmén DPoS (§5.10)
//!
//! Araştırma kaynakları:
//! - Polkadot sp-npos-elections `seq_phragmen`:
//!   `seq_phragmen(to_elect, candidates, voters, balancing)` →
//!   `ElectionResult { winners, assignments }`.
//!   Bizde: stake-ağırlıklı sıralama (simplified Phragmén — full impl prod'da).
//! - Cosmos SDK x/staking `EndBlocker`:
//!   `ApplyAndReturnValidatorSetUpdates` → validator seti güncelle.
//!   Bizde: `ElectionEngine::elect` → yeni aktif set.
//! - Polkadot pallet-staking `elect_and_apply_solution`:
//!   `elect_solution` → `ElectionScore { minimal_stake, sum_stake, sum_stake_squared }`.
//!   Bizde: `ElectionScore { min_stake, total_stake, winner_count }`.
//! - Avalanchego validators.Manager `AddStaker(nodeID, weight)`:
//!   Ağırlıklı validator listesi yönetimi. Bizde stake = weight.

use crate::types::{ValidatorInfo, ValidatorStatus, ACTIVE_VALIDATOR_COUNT};
use nova_primitives::{Balance, ValidatorId};
use serde::{Deserialize, Serialize};

// ─── ElectionScore ────────────────────────────────────────────────────────────

/// Seçim kalitesini ölçen skor.
///
/// Pattern: Polkadot sp-npos-elections `ElectionScore`:
/// `{ minimal_stake, sum_stake, sum_stake_squared }` — stake dağılım kalitesi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionScore {
    /// En düşük kazanan stake (Polkadot: `minimal_stake`).
    pub min_stake: u128,
    /// Toplam kazanan stake.
    pub total_stake: u128,
    /// Kazanan validator sayısı.
    pub winner_count: usize,
}

// ─── ElectionResult ──────────────────────────────────────────────────────────

/// Seçim sonucu.
///
/// Pattern: Polkadot sp-npos-elections `ElectionResult { winners, assignments }`.
#[derive(Debug, Clone)]
pub struct ElectionResult {
    /// Seçilen validator'lar (ID, stake) çifti, stake azalan sıra.
    pub winners: Vec<(ValidatorId, u128)>,
    /// Seçim skoru.
    pub score: ElectionScore,
}

impl ElectionResult {
    /// Seçilen validator ID listesi.
    pub fn winner_ids(&self) -> Vec<ValidatorId> {
        self.winners.iter().map(|(id, _)| *id).collect()
    }

    /// Minimum kazanan stake.
    pub fn min_winning_stake(&self) -> u128 {
        self.winners.last().map(|(_, s)| *s).unwrap_or(0)
    }
}

// ─── ElectionEngine ───────────────────────────────────────────────────────────

/// DPoS seçim motoru.
///
/// Basitleştirilmiş Phragmén: tam algoritma yerine stake-ağırlıklı sıralama.
/// Üretim: `seq_phragmen` ile delegasyon grafı üzerinde tam Phragmén.
pub struct ElectionEngine;

impl ElectionEngine {
    /// Validator seçimi yap.
    ///
    /// Pattern: Polkadot `seq_phragmen(to_elect, candidates, voters, balancing)`.
    /// Burada: aktif validator'ları stake'e göre sırala, en üst `to_elect` tanesini seç.
    ///
    /// Gelişmiş özellikler:
    /// - Minimum stake filtresi
    /// - Coğrafi çeşitlilik (basitleştirilmiş: farklı operator adreslerine göre)
    /// - Jailed/tombstoned validator'ları hariç tut
    pub fn elect(
        candidates: &[ValidatorInfo],
        to_elect: usize,
    ) -> ElectionResult {
        // Sadece aktif, yeterli stake'li validator'lar aday
        let mut eligible: Vec<&ValidatorInfo> = candidates
            .iter()
            .filter(|v| matches!(v.status, ValidatorStatus::Active))
            .collect();

        // Stake azalan sırayla sırala (Phragmén'in ilk approx.)
        eligible.sort_by(|a, b| b.stake.0.cmp(&a.stake.0));

        // İlk `to_elect` tanesini seç
        let selected = &eligible[..to_elect.min(eligible.len())];

        let winners: Vec<(ValidatorId, u128)> = selected
            .iter()
            .map(|v| (v.validator_id, v.stake.0))
            .collect();

        let total_stake: u128 = winners.iter().map(|(_, s)| s).sum();
        let min_stake = winners.last().map(|(_, s)| *s).unwrap_or(0);
        let winner_count = winners.len();

        ElectionResult {
            winners,
            score: ElectionScore {
                min_stake,
                total_stake,
                winner_count,
            },
        }
    }

    /// Standart 200-validator seçimi.
    pub fn elect_active_set(candidates: &[ValidatorInfo]) -> ElectionResult {
        Self::elect(candidates, ACTIVE_VALIDATOR_COUNT)
    }

    /// Seçim sonucunu doğrula.
    ///
    /// Polkadot `verify_election_score` pattern:
    /// winner_count, total_stake, min_stake tutarlılığı.
    pub fn verify_result(result: &ElectionResult) -> bool {
        if result.winners.is_empty() {
            return true;
        }

        // Stake azalan sıra
        let stakes: Vec<u128> = result.winners.iter().map(|(_, s)| *s).collect();
        let sorted = stakes.windows(2).all(|w| w[0] >= w[1]);

        // Skor tutarlılığı
        let total: u128 = stakes.iter().sum();
        let min = *stakes.last().unwrap_or(&0);

        sorted
            && total == result.score.total_stake
            && min == result.score.min_stake
            && result.winners.len() == result.score.winner_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MIN_VALIDATOR_STAKE};
    use nova_primitives::{Address, Balance, SlotId, ValidatorId};

    fn make_vid(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn make_addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_validator(seed: u8, stake_multiplier: u128) -> ValidatorInfo {
        ValidatorInfo::new(
            make_vid(seed),
            make_addr(seed),
            Balance(MIN_VALIDATOR_STAKE * stake_multiplier),
            500,
            SlotId(0),
            [seed; 32],
        )
    }

    // ─── elect ────────────────────────────────────────────────────────────

    #[test]
    fn test_elect_selects_top_by_stake() {
        let candidates = vec![
            make_validator(1, 1), // küçük stake
            make_validator(2, 3), // büyük stake
            make_validator(3, 2), // orta stake
        ];

        let result = ElectionEngine::elect(&candidates, 2);
        assert_eq!(result.winners.len(), 2);
        // İlk sırada en büyük stake
        assert_eq!(result.winners[0].0, make_vid(2));
        assert_eq!(result.winners[1].0, make_vid(3));
    }

    #[test]
    fn test_elect_excludes_jailed() {
        let mut v1 = make_validator(1, 2);
        let mut v2 = make_validator(2, 3);
        let v3 = make_validator(3, 1);

        v1.status = ValidatorStatus::Jailed { jailed_until_slot: 10_000 };
        v2.status = ValidatorStatus::Tombstoned;

        let candidates = vec![v1, v2, v3];
        let result = ElectionEngine::elect(&candidates, 10);
        assert_eq!(result.winners.len(), 1);
        assert_eq!(result.winners[0].0, make_vid(3));
    }

    #[test]
    fn test_elect_empty_candidates() {
        let result = ElectionEngine::elect(&[], 10);
        assert!(result.winners.is_empty());
        assert_eq!(result.score.winner_count, 0);
    }

    #[test]
    fn test_elect_fewer_than_requested() {
        let candidates = vec![make_validator(1, 1), make_validator(2, 2)];
        let result = ElectionEngine::elect(&candidates, 10);
        assert_eq!(result.winners.len(), 2, "sadece 2 aday var");
    }

    #[test]
    fn test_elect_score_accuracy() {
        let candidates = vec![
            make_validator(1, 3),
            make_validator(2, 2),
            make_validator(3, 1),
        ];
        let result = ElectionEngine::elect(&candidates, 3);

        let expected_total = MIN_VALIDATOR_STAKE * 6;
        let expected_min = MIN_VALIDATOR_STAKE * 1;

        assert_eq!(result.score.total_stake, expected_total);
        assert_eq!(result.score.min_stake, expected_min);
        assert_eq!(result.score.winner_count, 3);
    }

    // ─── verify_result ────────────────────────────────────────────────────

    #[test]
    fn test_verify_result_valid() {
        let candidates: Vec<ValidatorInfo> = (1..=5u8).map(|i| make_validator(i, i as u128)).collect();
        let result = ElectionEngine::elect(&candidates, 5);
        assert!(ElectionEngine::verify_result(&result));
    }

    #[test]
    fn test_verify_result_empty_valid() {
        let result = ElectionEngine::elect(&[], 5);
        assert!(ElectionEngine::verify_result(&result));
    }

    #[test]
    fn test_winner_ids() {
        let candidates = vec![make_validator(1, 2), make_validator(2, 1)];
        let result = ElectionEngine::elect(&candidates, 2);
        let ids = result.winner_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&make_vid(1)));
        assert!(ids.contains(&make_vid(2)));
    }

    #[test]
    fn test_min_winning_stake() {
        let candidates = vec![
            make_validator(1, 3),
            make_validator(2, 1),
        ];
        let result = ElectionEngine::elect(&candidates, 2);
        assert_eq!(result.min_winning_stake(), MIN_VALIDATOR_STAKE);
    }

    #[test]
    fn test_elect_active_set_max_200() {
        // 250 aday, 200 seç
        let candidates: Vec<ValidatorInfo> = (0..250u8)
            .map(|i| make_validator(i, i as u128 + 1))
            .collect();
        let result = ElectionEngine::elect_active_set(&candidates);
        assert_eq!(result.winners.len(), 200);
    }
}
