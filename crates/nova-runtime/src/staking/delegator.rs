//! Delegatör — NSC staker, oy kullanıcı (Cosmos SDK x/staking delegation'dan ilham)

use crate::errors::{RuntimeError, RuntimeResult};
use crate::token::fee_dist::{DelegatorRewardInfo, F1RewardTracker};
use nova_primitives::{Address, ValidatorId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Bir delegasyonu temsil eder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    pub delegator: Address,
    pub validator_id: ValidatorId,
    /// Stake miktarı (NSC en küçük birim)
    pub amount: u128,
    /// Delegasyon başlama slot'u
    pub delegated_at_slot: u64,
    /// F1 için başlangıç kümülatif oranı
    pub starting_cumulative: u128,
}

impl Delegation {
    pub fn new(
        delegator: Address,
        validator_id: ValidatorId,
        amount: u128,
        delegated_at_slot: u64,
        starting_cumulative: u128,
    ) -> Self {
        Self { delegator, validator_id, amount, delegated_at_slot, starting_cumulative }
    }
}

/// UnbondingEntry — unbonding kuyruğundaki bir giriş
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbondingEntry {
    pub delegator: Address,
    pub validator_id: ValidatorId,
    pub amount: u128,
    pub completion_slot: u64,
}

/// DelegatorStore — delegasyon deposu
#[derive(Debug)]
pub struct DelegatorStore {
    /// (delegator, validator) → Delegation
    delegations: Arc<RwLock<HashMap<(Address, ValidatorId), Delegation>>>,
    /// Validator başına F1 reward tracker
    reward_trackers: Arc<RwLock<HashMap<ValidatorId, F1RewardTracker>>>,
    /// Unbonding kuyruğu
    unbonding: Arc<RwLock<Vec<UnbondingEntry>>>,
}

impl Default for DelegatorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DelegatorStore {
    pub fn new() -> Self {
        Self {
            delegations: Arc::new(RwLock::new(HashMap::new())),
            reward_trackers: Arc::new(RwLock::new(HashMap::new())),
            unbonding: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Delegasyon ekle
    pub fn delegate(
        &self,
        delegator: Address,
        validator_id: ValidatorId,
        amount: u128,
        current_slot: u64,
    ) -> RuntimeResult<()> {
        let starting_cumulative = self
            .reward_trackers
            .read()
            .get(&validator_id)
            .map(|t| t.cumulative_reward_per_unit())
            .unwrap_or(0);

        let mut delegations = self.delegations.write();
        let key = (delegator, validator_id);
        if let Some(existing) = delegations.get_mut(&key) {
            // Mevcut delegasyona ekle — starting_cumulative'i güncelle
            // (Mevcut pending ödüller önce çekilmiş sayılır)
            existing.amount += amount;
            existing.starting_cumulative = starting_cumulative;
        } else {
            delegations.insert(
                key,
                Delegation::new(delegator, validator_id, amount, current_slot, starting_cumulative),
            );
        }
        Ok(())
    }

    /// Delegasyonu kaldır — unbonding kuyruğuna ekle
    pub fn undelegate(
        &self,
        delegator: Address,
        validator_id: ValidatorId,
        amount: u128,
        current_slot: u64,
        unbonding_completion_slot: u64,
    ) -> RuntimeResult<()> {
        let key = (delegator, validator_id);
        let mut delegations = self.delegations.write();
        let delegation = delegations.get_mut(&key).ok_or(RuntimeError::DelegationNotFound)?;

        if delegation.amount < amount {
            return Err(RuntimeError::InsufficientStake {
                min: amount,
                provided: delegation.amount,
            });
        }

        delegation.amount -= amount;
        if delegation.amount == 0 {
            delegations.remove(&key);
        }

        // Unbonding kuyruğuna ekle
        self.unbonding.write().push(UnbondingEntry {
            delegator,
            validator_id,
            amount,
            completion_slot: unbonding_completion_slot,
        });

        let _ = current_slot; // Kullanılıyor
        Ok(())
    }

    /// Tamamlanmış unbonding'leri al ve temizle
    pub fn mature_unbondings(&self, current_slot: u64) -> Vec<UnbondingEntry> {
        let mut unbonding = self.unbonding.write();
        let (mature, remaining): (Vec<_>, Vec<_>) =
            unbonding.drain(..).partition(|e| current_slot >= e.completion_slot);
        *unbonding = remaining;
        mature
    }

    /// Validator için F1 ödül tracker'ı al veya oluştur
    pub fn get_or_create_tracker(&self, validator_id: ValidatorId) -> F1RewardTracker {
        self.reward_trackers
            .read()
            .get(&validator_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Validator için F1 tracker'ı güncelle
    pub fn update_tracker(&self, validator_id: ValidatorId, tracker: F1RewardTracker) {
        self.reward_trackers.write().insert(validator_id, tracker);
    }

    /// Delegatörün bir validator'daki bekleyen ödülünü hesapla
    pub fn pending_reward(&self, delegator: &Address, validator_id: &ValidatorId) -> u128 {
        let key = (*delegator, *validator_id);
        let delegations = self.delegations.read();
        let Some(delegation) = delegations.get(&key) else { return 0 };

        let trackers = self.reward_trackers.read();
        let Some(tracker) = trackers.get(validator_id) else { return 0 };

        let info = DelegatorRewardInfo::new(
            *delegator,
            delegation.amount,
            delegation.starting_cumulative,
        );
        info.pending(tracker)
    }

    pub fn delegation(&self, delegator: &Address, validator_id: &ValidatorId) -> Option<Delegation> {
        self.delegations.read().get(&(*delegator, *validator_id)).cloned()
    }

    pub fn validator_delegations(&self, validator_id: &ValidatorId) -> Vec<Delegation> {
        self.delegations
            .read()
            .values()
            .filter(|d| &d.validator_id == validator_id)
            .cloned()
            .collect()
    }

    pub fn total_delegated(&self, validator_id: &ValidatorId) -> u128 {
        self.validator_delegations(validator_id).iter().map(|d| d.amount).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn vid(n: u64) -> ValidatorId {
        ValidatorId(n)
    }

    #[test]
    fn test_delegate_basic() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 1_000, 0).unwrap();
        let del = store.delegation(&addr(1), &vid(1)).unwrap();
        assert_eq!(del.amount, 1_000);
    }

    #[test]
    fn test_delegate_adds_to_existing() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 1_000, 0).unwrap();
        store.delegate(addr(1), vid(1), 500, 10).unwrap();
        let del = store.delegation(&addr(1), &vid(1)).unwrap();
        assert_eq!(del.amount, 1_500);
    }

    #[test]
    fn test_undelegate_basic() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 1_000, 0).unwrap();
        store.undelegate(addr(1), vid(1), 400, 100, 200).unwrap();
        let del = store.delegation(&addr(1), &vid(1)).unwrap();
        assert_eq!(del.amount, 600);
    }

    #[test]
    fn test_undelegate_all_removes_entry() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 1_000, 0).unwrap();
        store.undelegate(addr(1), vid(1), 1_000, 100, 200).unwrap();
        assert!(store.delegation(&addr(1), &vid(1)).is_none());
    }

    #[test]
    fn test_undelegate_insufficient() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 100, 0).unwrap();
        let err = store.undelegate(addr(1), vid(1), 200, 10, 20);
        assert!(matches!(err, Err(RuntimeError::InsufficientStake { .. })));
    }

    #[test]
    fn test_mature_unbondings() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 1_000, 0).unwrap();
        store.undelegate(addr(1), vid(1), 500, 100, 200).unwrap();

        let mature = store.mature_unbondings(200);
        assert_eq!(mature.len(), 1);
        assert_eq!(mature[0].amount, 500);

        // Tekrar çağırınca boş gelir
        let mature2 = store.mature_unbondings(300);
        assert!(mature2.is_empty());
    }

    #[test]
    fn test_f1_reward_tracking() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 500, 0).unwrap();
        store.delegate(addr(2), vid(1), 500, 0).unwrap();

        // Validator 1000 ödül aldı
        let mut tracker = store.get_or_create_tracker(vid(1));
        tracker.set_total_stake(1_000);
        tracker.add_reward(1_000);
        store.update_tracker(vid(1), tracker);

        // Her delegatör 500 ödül bekliyor
        let r1 = store.pending_reward(&addr(1), &vid(1));
        let r2 = store.pending_reward(&addr(2), &vid(1));
        assert_eq!(r1, 500);
        assert_eq!(r2, 500);
    }

    #[test]
    fn test_total_delegated() {
        let store = DelegatorStore::new();
        store.delegate(addr(1), vid(1), 1_000, 0).unwrap();
        store.delegate(addr(2), vid(1), 2_000, 0).unwrap();
        assert_eq!(store.total_delegated(&vid(1)), 3_000);
    }
}
