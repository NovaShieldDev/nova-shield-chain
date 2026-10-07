//! Governance Oylama — stake-ağırlıklı oy (Cosmos SDK x/gov voting.go'dan ilham)

use crate::errors::{RuntimeError, RuntimeResult};
use crate::governance::proposal::{Proposal, ProposalStatus, Vote};
use crate::governance::tally::TallyEngine;
use crate::tx::types::VoteOption;
use nova_primitives::{Address, SlotId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// ProposalStore — öneri deposu
#[derive(Debug, Default)]
pub struct ProposalStore {
    proposals: Arc<RwLock<HashMap<u64, Proposal>>>,
    /// proposal_id → (voter → oy)
    votes: Arc<RwLock<HashMap<u64, HashMap<Address, (VoteOption, u128)>>>>,
    next_id: Arc<RwLock<u64>>,
}

impl ProposalStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_proposal_id(&self) -> u64 {
        let mut id = self.next_id.write();
        let current = *id;
        *id += 1;
        current
    }

    pub fn add_proposal(&self, proposal: Proposal) {
        self.proposals.write().insert(proposal.id, proposal);
    }

    pub fn get(&self, id: u64) -> RuntimeResult<Proposal> {
        self.proposals
            .read()
            .get(&id)
            .cloned()
            .ok_or(RuntimeError::ProposalNotFound { id })
    }

    pub fn update<F: FnOnce(&mut Proposal)>(&self, id: u64, f: F) -> RuntimeResult<()> {
        let mut proposals = self.proposals.write();
        let p = proposals.get_mut(&id).ok_or(RuntimeError::ProposalNotFound { id })?;
        f(p);
        Ok(())
    }

    /// Oy kullan
    ///
    /// `bonded_stake`: voter'ın gerçek staked bakiyesi (çağıran tarafından staking state'ten alınır).
    /// Talep edilen `voting_power` bu değeri aşamaz — NOVA-RED-RUNTIME-002 düzeltmesi.
    pub fn cast_vote(
        &self,
        proposal_id: u64,
        voter: Address,
        option: VoteOption,
        voting_power: u128,
        bonded_stake: u128,
        current_slot: SlotId,
    ) -> RuntimeResult<()> {
        let proposal = self.get(proposal_id)?;

        if !proposal.is_in_voting_period(current_slot) {
            return Err(RuntimeError::VotingPeriodExpired);
        }

        // NOVA-RED-RUNTIME-002: voting_power staked bakiyeyi aşamaz
        if voting_power > bonded_stake {
            return Err(RuntimeError::ExcessiveVotingPower {
                claimed: voting_power,
                max_allowed: bonded_stake,
            });
        }

        let mut votes = self.votes.write();
        let proposal_votes = votes.entry(proposal_id).or_default();

        // Cosmos SDK gibi: oy değiştirilebilir (son oy geçerlidir)
        proposal_votes.insert(voter, (option.clone(), voting_power));

        Ok(())
    }

    /// Oy sayımı yap ve öneri durumunu güncelle
    pub fn finalize_proposal(&self, proposal_id: u64, total_bonded: u128) -> RuntimeResult<()> {
        let votes = self.votes.read();
        let proposal_votes = votes.get(&proposal_id).cloned().unwrap_or_default();
        drop(votes);

        let tally = TallyEngine::tally(&proposal_votes, total_bonded);
        let (new_status, burn_deposit) = TallyEngine::evaluate(&tally, false);

        self.update(proposal_id, |p| {
            p.status = new_status;
            if burn_deposit {
                p.total_deposit = 0; // Depozito yakıldı
            }
        })?;

        Ok(())
    }

    /// Süresi dolmuş önerileri finalize et
    pub fn process_expired(&self, current_slot: SlotId, total_bonded: u128) -> Vec<u64> {
        let expired: Vec<u64> = self
            .proposals
            .read()
            .values()
            .filter(|p| {
                matches!(p.status, ProposalStatus::VotingPeriod)
                    && p.is_voting_expired(current_slot)
            })
            .map(|p| p.id)
            .collect();

        for id in &expired {
            let _ = self.finalize_proposal(*id, total_bonded);
        }

        expired
    }

    pub fn proposal_count(&self) -> usize {
        self.proposals.read().len()
    }

    pub fn active_proposals(&self) -> Vec<Proposal> {
        self.proposals
            .read()
            .values()
            .filter(|p| matches!(p.status, ProposalStatus::VotingPeriod))
            .cloned()
            .collect()
    }

    pub fn votes_for(&self, proposal_id: u64) -> HashMap<Address, (VoteOption, u128)> {
        self.votes.read().get(&proposal_id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governance::proposal::{MIN_DEPOSIT_NSC, VOTING_PERIOD_SLOTS};
    use crate::tx::types::ProposalPayload;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn make_proposal(store: &ProposalStore, slot: u64) -> u64 {
        let id = store.next_proposal_id();
        let p = Proposal::new(
            id,
            addr(1),
            "Test Öneri".to_string(),
            "Açıklama".to_string(),
            ProposalPayload::Text { content: "içerik".to_string() },
            MIN_DEPOSIT_NSC,
            slot,
        );
        store.add_proposal(p);
        id
    }

    #[test]
    fn test_cast_vote_basic() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        store.cast_vote(pid, addr(2), VoteOption::Yes, 100, 100, SlotId(1)).unwrap();
        let votes = store.votes_for(pid);
        assert_eq!(votes.len(), 1);
    }

    #[test]
    fn test_cast_vote_expired() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        // Oylama süresi doldu
        let err = store.cast_vote(pid, addr(2), VoteOption::Yes, 100, 100, SlotId(VOTING_PERIOD_SLOTS + 1));
        assert!(matches!(err, Err(RuntimeError::VotingPeriodExpired)));
    }

    #[test]
    fn test_vote_can_be_changed() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        store.cast_vote(pid, addr(2), VoteOption::No, 100, 100, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(2), VoteOption::Yes, 100, 100, SlotId(2)).unwrap();
        let votes = store.votes_for(pid);
        assert_eq!(votes[&addr(2)].0, VoteOption::Yes);
    }

    #[test]
    fn test_finalize_passed() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        // Quorum ve çoğunluk sağla
        store.cast_vote(pid, addr(2), VoteOption::Yes, 700, 700, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(3), VoteOption::No, 300, 300, SlotId(1)).unwrap();
        store.finalize_proposal(pid, 1_000).unwrap();
        let p = store.get(pid).unwrap();
        assert!(matches!(p.status, ProposalStatus::Passed));
    }

    #[test]
    fn test_finalize_rejected_no_quorum() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        // Sadece az oy
        store.cast_vote(pid, addr(2), VoteOption::Yes, 100, 100, SlotId(1)).unwrap();
        store.finalize_proposal(pid, 10_000).unwrap(); // total_bonded = 10_000
        let p = store.get(pid).unwrap();
        assert!(matches!(p.status, ProposalStatus::Rejected));
    }

    #[test]
    fn test_process_expired() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        store.cast_vote(pid, addr(2), VoteOption::Yes, 700, 700, SlotId(1)).unwrap();
        store.cast_vote(pid, addr(3), VoteOption::No, 300, 300, SlotId(1)).unwrap();

        // Oylama süresi geçtikten sonra process_expired çağır
        let finalized = store.process_expired(SlotId(VOTING_PERIOD_SLOTS + 1), 1_000);
        assert!(finalized.contains(&pid));
        let p = store.get(pid).unwrap();
        assert!(!matches!(p.status, ProposalStatus::VotingPeriod));
    }

    #[test]
    fn test_proposal_not_found() {
        let store = ProposalStore::new();
        let err = store.get(999);
        assert!(matches!(err, Err(RuntimeError::ProposalNotFound { id: 999 })));
    }

    #[test]
    fn test_active_proposals() {
        let store = ProposalStore::new();
        make_proposal(&store, 0);
        make_proposal(&store, 0);
        assert_eq!(store.active_proposals().len(), 2);
    }

    /// NOVA-RED-RUNTIME-002 regresyon: voting_power > bonded_stake → hata
    #[test]
    fn test_excessive_voting_power_rejected() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        // 100 bonded_stake, ama u128::MAX talep ediyor — saldırı simülasyonu
        let err = store.cast_vote(pid, addr(2), VoteOption::Yes, u128::MAX, 100, SlotId(1));
        assert!(
            matches!(err, Err(RuntimeError::ExcessiveVotingPower { .. })),
            "Aşırı oy gücü reddedilmeli: {err:?}"
        );
    }

    /// NOVA-RED-RUNTIME-002: bonded_stake'e tam eşit voting_power kabul edilmeli
    #[test]
    fn test_exact_voting_power_accepted() {
        let store = ProposalStore::new();
        let pid = make_proposal(&store, 0);
        store.cast_vote(pid, addr(2), VoteOption::Yes, 500, 500, SlotId(1)).unwrap();
        let votes = store.votes_for(pid);
        assert_eq!(votes[&addr(2)].1, 500);
    }
}
