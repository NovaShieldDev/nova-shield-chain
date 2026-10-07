//! Governance öneri yapısı (Cosmos SDK x/gov'dan ilham)

use crate::tx::types::{ProposalPayload, VoteOption};
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Minimum depozit: 1,000 NSC (bkz. token/nsc.rs MIN_GOV_DEPOSIT_NSC)
pub const MIN_DEPOSIT_NSC: u128 = 1_000 * 1_000_000_000_000_000_000u128;

/// Depozit toplama süresi: 7 gün = 7 * 24h * 216_000 slot/era
pub const DEPOSIT_PERIOD_SLOTS: u64 = 7 * 24 * 216_000;

/// Oylama süresi: 14 gün
pub const VOTING_PERIOD_SLOTS: u64 = 14 * 24 * 216_000;

// ─── Öneri Durumu ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Depozit bekleniyor
    DepositPeriod,
    /// Oylama aktif
    VotingPeriod,
    /// Geçti — yürütme bekleniyor
    Passed,
    /// Reddedildi
    Rejected,
    /// Veto edildi — depozit yakıldı
    Vetoed,
    /// Yürütüldü
    Executed,
    /// Yürütme başarısız
    Failed,
}

impl ProposalStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ProposalStatus::Executed | ProposalStatus::Failed | ProposalStatus::Vetoed | ProposalStatus::Rejected
        )
    }
}

// ─── Öneri ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub payload: ProposalPayload,
    pub status: ProposalStatus,

    /// Toplanan depozit
    pub total_deposit: u128,
    /// Oylama başlangıç slot'u
    pub voting_start_slot: Option<u64>,
    /// Oylama bitiş slot'u
    pub voting_end_slot: Option<u64>,
    /// Gönderim slot'u
    pub submitted_at_slot: u64,

    /// Depozit toplandı mı?
    pub deposit_threshold_met: bool,
}

impl Proposal {
    pub fn new(
        id: u64,
        proposer: Address,
        title: String,
        description: String,
        payload: ProposalPayload,
        initial_deposit: u128,
        submitted_at_slot: u64,
    ) -> Self {
        let deposit_threshold_met = initial_deposit >= MIN_DEPOSIT_NSC;
        let (voting_start, voting_end) = if deposit_threshold_met {
            (
                Some(submitted_at_slot),
                Some(submitted_at_slot + VOTING_PERIOD_SLOTS),
            )
        } else {
            (None, None)
        };

        Self {
            id,
            proposer,
            title,
            description,
            payload,
            status: if deposit_threshold_met {
                ProposalStatus::VotingPeriod
            } else {
                ProposalStatus::DepositPeriod
            },
            total_deposit: initial_deposit,
            voting_start_slot: voting_start,
            voting_end_slot: voting_end,
            submitted_at_slot,
            deposit_threshold_met,
        }
    }

    /// Ek depozit ekle
    pub fn add_deposit(&mut self, amount: u128, current_slot: u64) {
        self.total_deposit += amount;
        if !self.deposit_threshold_met && self.total_deposit >= MIN_DEPOSIT_NSC {
            self.deposit_threshold_met = true;
            self.status = ProposalStatus::VotingPeriod;
            self.voting_start_slot = Some(current_slot);
            self.voting_end_slot = Some(current_slot + VOTING_PERIOD_SLOTS);
        }
    }

    /// Oylama süresi doldu mu?
    pub fn is_voting_expired(&self, current_slot: SlotId) -> bool {
        match self.voting_end_slot {
            Some(end) => current_slot.0 >= end,
            None => false,
        }
    }

    pub fn is_in_voting_period(&self, current_slot: SlotId) -> bool {
        matches!(self.status, ProposalStatus::VotingPeriod)
            && !self.is_voting_expired(current_slot)
    }
}

/// TallyResult — oylama sonucu
#[derive(Debug, Clone, Default)]
pub struct TallyResult {
    pub yes_power: u128,
    pub no_power: u128,
    pub abstain_power: u128,
    pub no_with_veto_power: u128,
    pub total_voted: u128,
    pub total_bonded: u128,
}

impl TallyResult {
    pub fn total_non_abstain(&self) -> u128 {
        self.yes_power + self.no_power + self.no_with_veto_power
    }
}

/// Vote — bir oy kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: Address,
    pub proposal_id: u64,
    pub option: VoteOption,
    pub voting_power: u128,
    pub cast_at_slot: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn payload() -> ProposalPayload {
        ProposalPayload::Text { content: "test".to_string() }
    }

    #[test]
    fn test_proposal_with_full_deposit() {
        let p = Proposal::new(
            1,
            addr(1),
            "Test".to_string(),
            "Açıklama".to_string(),
            payload(),
            MIN_DEPOSIT_NSC,
            100,
        );
        assert!(matches!(p.status, ProposalStatus::VotingPeriod));
        assert!(p.deposit_threshold_met);
        assert!(p.voting_start_slot.is_some());
    }

    #[test]
    fn test_proposal_insufficient_deposit() {
        let p = Proposal::new(1, addr(1), "T".to_string(), "D".to_string(), payload(), 100, 100);
        assert!(matches!(p.status, ProposalStatus::DepositPeriod));
        assert!(!p.deposit_threshold_met);
    }

    #[test]
    fn test_proposal_add_deposit_activates_voting() {
        let mut p =
            Proposal::new(1, addr(1), "T".to_string(), "D".to_string(), payload(), 0, 100);
        p.add_deposit(MIN_DEPOSIT_NSC, 200);
        assert!(matches!(p.status, ProposalStatus::VotingPeriod));
        assert_eq!(p.voting_start_slot, Some(200));
        assert_eq!(p.voting_end_slot, Some(200 + VOTING_PERIOD_SLOTS));
    }

    #[test]
    fn test_proposal_is_voting_expired() {
        let p = Proposal::new(
            1,
            addr(1),
            "T".to_string(),
            "D".to_string(),
            payload(),
            MIN_DEPOSIT_NSC,
            0,
        );
        assert!(!p.is_voting_expired(SlotId(100)));
        assert!(p.is_voting_expired(SlotId(VOTING_PERIOD_SLOTS)));
    }

    #[test]
    fn test_proposal_status_terminal() {
        assert!(ProposalStatus::Executed.is_terminal());
        assert!(ProposalStatus::Rejected.is_terminal());
        assert!(ProposalStatus::Vetoed.is_terminal());
        assert!(!ProposalStatus::VotingPeriod.is_terminal());
    }
}
