//! Epoch yönetimi — her 24 saatte bir validator seti rotasyonu (§5.10)
//!
//! Araştırma kaynakları:
//! - Polkadot pallet-session `SessionManager::new_session(session_idx)`:
//!   Session geçişini tetikler, yeni validator setini döner.
//!   Bizde: `EpochManager::try_advance_epoch` — slot-based tetikleyici.
//! - Cosmos SDK x/staking `EndBlocker → ApplyAndReturnValidatorSetUpdates`:
//!   Her blok sonunda validator değişikliklerini uygula.
//!   Bizde: `EpochManager::process_slot` — periyodik epoch geçişi.
//! - Polkadot `pallet-babe` epoch change logic:
//!   `GenesisSlot`, `CurrentSlot`, `EpochIndex`, `Randomness` →
//!   Bizde: `EpochInfo { started_at_slot, ends_at_slot, epoch_id }`.
//! - go-ethereum `LookupBlockNumberByTime`:
//!   Block number → timestamp mapping. Bizde: slot → epoch mapping.

use crate::election::ElectionEngine;
use crate::errors::{ConsensusError, ConsensusResult};
use crate::types::{
    ConsensusEvent, ConsensusEventType, EpochInfo, ValidatorInfo, EPOCH_DURATION_SLOTS,
};
use nova_primitives::{Balance, EpochId, SlotId, ValidatorId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::{debug, info};

// ─── EpochManager ─────────────────────────────────────────────────────────────

/// Epoch geçiş yöneticisi.
///
/// Pattern: Polkadot `pallet-session` + Cosmos SDK `EndBlocker`.
/// Her `EPOCH_DURATION_SLOTS` slot'ta bir yeni validator seti seçilir.
pub struct EpochManager {
    /// Mevcut epoch.
    current_epoch: RwLock<EpochInfo>,
    /// Epoch tarihçesi (son N epoch).
    epoch_history: RwLock<VecDeque<EpochInfo>>,
    /// Maksimum tarihçe boyutu.
    max_history: usize,
}

impl EpochManager {
    const MAX_EPOCH_HISTORY: usize = 30;

    /// Yeni EpochManager oluştur (genesis epoch ile).
    pub fn new(genesis_validators: Vec<ValidatorId>, genesis_total_stake: Balance) -> Self {
        let genesis_epoch = EpochInfo {
            epoch_id: EpochId(0),
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(EPOCH_DURATION_SLOTS),
            validator_set: genesis_validators,
            total_stake: genesis_total_stake,
            blocks_produced: 0,
        };

        Self {
            current_epoch: RwLock::new(genesis_epoch),
            epoch_history: RwLock::new(VecDeque::new()),
            max_history: Self::MAX_EPOCH_HISTORY,
        }
    }

    /// Mevcut epoch bilgisini al.
    pub fn current_epoch(&self) -> EpochInfo {
        self.current_epoch.read().clone()
    }

    /// Mevcut epoch ID'si.
    pub fn current_epoch_id(&self) -> EpochId {
        self.current_epoch.read().epoch_id
    }

    /// Bu slot hangi epoch'ta?
    pub fn epoch_for_slot(&self, slot: SlotId) -> EpochId {
        let epoch_idx = slot.0 / EPOCH_DURATION_SLOTS;
        EpochId(epoch_idx)
    }

    /// Blok üreticisini döner (round-robin).
    ///
    /// Whitepaper §5.10: "Her slot'ta 1 validator blok üretir (round-robin + VRF)"
    pub fn slot_proposer(&self, slot: SlotId) -> Option<ValidatorId> {
        self.current_epoch.read().slot_proposer(slot)
    }

    /// Slot işle — epoch doldu mu kontrol et.
    ///
    /// Pattern: Polkadot Scheduler `on_initialize(now)` + Cosmos `EndBlocker`.
    /// Epoch dolduğunda:
    /// 1. Mevcut epoch'u tarihçeye ekle
    /// 2. Yeni epoch'u oluştur (seçim sonuçlarıyla)
    /// Return: epoch geçişi oldu mu? + eventler
    pub fn process_slot(
        &self,
        current_slot: SlotId,
        all_candidates: &[ValidatorInfo],
    ) -> (bool, Vec<ConsensusEvent>) {
        let mut events = Vec::new();
        let epoch_expired = {
            let epoch = self.current_epoch.read();
            epoch.is_expired(current_slot)
        };

        if !epoch_expired {
            return (false, events);
        }

        // Epoch geçişi
        let (old_epoch_id, new_epoch_id, new_validators) =
            self.advance_epoch(current_slot, all_candidates);

        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::EpochTransition {
                old_epoch: old_epoch_id.0,
                new_epoch: new_epoch_id.0,
                new_validators: new_validators.clone(),
            },
        });

        info!(
            target: "nova_consensus",
            old_epoch = old_epoch_id.0,
            new_epoch = new_epoch_id.0,
            validator_count = new_validators.len(),
            "epoch geçişi tamamlandı"
        );

        (true, events)
    }

    /// Epoch geçişini zorla (process_slot ile de çağrılır).
    ///
    /// 1. Mevcut epoch → tarihçe
    /// 2. Seçim yap (ElectionEngine)
    /// 3. Yeni epoch oluştur
    fn advance_epoch(
        &self,
        current_slot: SlotId,
        all_candidates: &[ValidatorInfo],
    ) -> (EpochId, EpochId, Vec<ValidatorId>) {
        let old_epoch_id = {
            let epoch = self.current_epoch.read();
            epoch.epoch_id
        };

        // Seçim yap
        let election = ElectionEngine::elect_active_set(all_candidates);
        let new_validators = election.winner_ids();
        let total_stake = Balance(election.score.total_stake);
        let new_epoch_id = EpochId(old_epoch_id.0 + 1);

        // Eski epoch'u tarihçeye ekle
        {
            let old_epoch = self.current_epoch.read().clone();
            let mut history = self.epoch_history.write();
            if history.len() >= self.max_history {
                history.pop_front();
            }
            history.push_back(old_epoch);
        }

        // Yeni epoch oluştur
        let new_epoch = EpochInfo {
            epoch_id: new_epoch_id,
            started_at_slot: current_slot,
            ends_at_slot: SlotId(current_slot.0 + EPOCH_DURATION_SLOTS),
            validator_set: new_validators.clone(),
            total_stake,
            blocks_produced: 0,
        };

        *self.current_epoch.write() = new_epoch;

        (old_epoch_id, new_epoch_id, new_validators)
    }

    /// Blok üretildi — epoch istatistiklerini güncelle.
    pub fn record_block(&self) {
        self.current_epoch.write().blocks_produced += 1;
    }

    /// Epoch tarihçesini al (en yeni önce).
    pub fn epoch_history(&self) -> Vec<EpochInfo> {
        self.epoch_history.read().iter().rev().cloned().collect()
    }

    /// Belirli bir epoch'u tarihçeden al.
    pub fn get_epoch(&self, epoch_id: EpochId) -> Option<EpochInfo> {
        let current = self.current_epoch.read();
        if current.epoch_id == epoch_id {
            return Some(current.clone());
        }

        self.epoch_history
            .read()
            .iter()
            .find(|e| e.epoch_id == epoch_id)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MIN_VALIDATOR_STAKE, ACTIVE_VALIDATOR_COUNT};

    fn make_vid(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn make_validators(count: u8) -> Vec<ValidatorInfo> {
        (1..=count)
            .map(|i| ValidatorInfo::new(
                make_vid(i),
                nova_primitives::Address::from_bytes([i; 20]),
                Balance(MIN_VALIDATOR_STAKE * i as u128),
                0,
                SlotId(0),
                [i; 32],
            ))
            .collect()
    }

    fn make_manager(count: u8) -> EpochManager {
        let ids: Vec<ValidatorId> = (1..=count).map(make_vid).collect();
        let total_stake = Balance(MIN_VALIDATOR_STAKE * count as u128);
        EpochManager::new(ids, total_stake)
    }

    // ─── current_epoch ────────────────────────────────────────────────────

    #[test]
    fn test_genesis_epoch_id_is_zero() {
        let manager = make_manager(10);
        assert_eq!(manager.current_epoch_id(), EpochId(0));
    }

    #[test]
    fn test_genesis_epoch_duration() {
        let manager = make_manager(5);
        let epoch = manager.current_epoch();
        assert_eq!(epoch.started_at_slot, SlotId(0));
        assert_eq!(epoch.ends_at_slot, SlotId(EPOCH_DURATION_SLOTS));
    }

    #[test]
    fn test_genesis_validator_set() {
        let ids: Vec<ValidatorId> = vec![make_vid(1), make_vid(2), make_vid(3)];
        let manager = EpochManager::new(ids.clone(), Balance(MIN_VALIDATOR_STAKE * 3));
        let epoch = manager.current_epoch();
        assert_eq!(epoch.validator_set.len(), 3);
    }

    // ─── epoch_for_slot ───────────────────────────────────────────────────

    #[test]
    fn test_epoch_for_slot_zero_is_zero() {
        let manager = make_manager(5);
        assert_eq!(manager.epoch_for_slot(SlotId(0)), EpochId(0));
    }

    #[test]
    fn test_epoch_for_slot_epoch_boundary() {
        let manager = make_manager(5);
        assert_eq!(
            manager.epoch_for_slot(SlotId(EPOCH_DURATION_SLOTS)),
            EpochId(1)
        );
        assert_eq!(
            manager.epoch_for_slot(SlotId(EPOCH_DURATION_SLOTS - 1)),
            EpochId(0)
        );
    }

    // ─── slot_proposer ────────────────────────────────────────────────────

    #[test]
    fn test_slot_proposer_round_robin() {
        let v1 = make_vid(1);
        let v2 = make_vid(2);
        let manager = EpochManager::new(
            vec![v1, v2],
            Balance(MIN_VALIDATOR_STAKE * 2),
        );

        assert_eq!(manager.slot_proposer(SlotId(0)), Some(v1));
        assert_eq!(manager.slot_proposer(SlotId(1)), Some(v2));
        assert_eq!(manager.slot_proposer(SlotId(2)), Some(v1));
    }

    #[test]
    fn test_slot_proposer_empty_returns_none() {
        let manager = EpochManager::new(vec![], Balance(0));
        assert_eq!(manager.slot_proposer(SlotId(0)), None);
    }

    // ─── process_slot ─────────────────────────────────────────────────────

    #[test]
    fn test_process_slot_no_transition_before_end() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        let (transitioned, events) =
            manager.process_slot(SlotId(EPOCH_DURATION_SLOTS - 1), &candidates);
        assert!(!transitioned);
        assert!(events.is_empty());
        assert_eq!(manager.current_epoch_id(), EpochId(0));
    }

    #[test]
    fn test_process_slot_triggers_epoch_transition() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        let (transitioned, events) =
            manager.process_slot(SlotId(EPOCH_DURATION_SLOTS), &candidates);
        assert!(transitioned);
        assert!(!events.is_empty());
        assert_eq!(manager.current_epoch_id(), EpochId(1));
    }

    #[test]
    fn test_epoch_transition_event_type() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        let (_, events) = manager.process_slot(SlotId(EPOCH_DURATION_SLOTS), &candidates);

        let has_transition = events.iter().any(|e| {
            matches!(
                e.event_type,
                ConsensusEventType::EpochTransition { old_epoch: 0, new_epoch: 1, .. }
            )
        });
        assert!(has_transition);
    }

    #[test]
    fn test_multiple_epoch_transitions() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        for i in 1..=3u64 {
            let (transitioned, _) =
                manager.process_slot(SlotId(i * EPOCH_DURATION_SLOTS), &candidates);
            assert!(transitioned);
            assert_eq!(manager.current_epoch_id(), EpochId(i));
        }
    }

    #[test]
    fn test_epoch_history_grows() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        // 3 geçiş yap
        for i in 1..=3u64 {
            manager.process_slot(SlotId(i * EPOCH_DURATION_SLOTS), &candidates);
        }

        let history = manager.epoch_history();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_new_epoch_has_correct_duration() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        manager.process_slot(SlotId(EPOCH_DURATION_SLOTS), &candidates);

        let epoch = manager.current_epoch();
        assert_eq!(epoch.started_at_slot, SlotId(EPOCH_DURATION_SLOTS));
        assert_eq!(epoch.ends_at_slot, SlotId(2 * EPOCH_DURATION_SLOTS));
    }

    // ─── record_block ─────────────────────────────────────────────────────

    #[test]
    fn test_record_block_increments_count() {
        let manager = make_manager(5);
        assert_eq!(manager.current_epoch().blocks_produced, 0);

        manager.record_block();
        manager.record_block();

        assert_eq!(manager.current_epoch().blocks_produced, 2);
    }

    // ─── get_epoch ────────────────────────────────────────────────────────

    #[test]
    fn test_get_current_epoch() {
        let manager = make_manager(5);
        let epoch = manager.get_epoch(EpochId(0));
        assert!(epoch.is_some());
    }

    #[test]
    fn test_get_historical_epoch() {
        let manager = make_manager(5);
        let candidates = make_validators(5);

        manager.process_slot(SlotId(EPOCH_DURATION_SLOTS), &candidates);

        // Epoch 0 tarihçede olmalı
        let epoch0 = manager.get_epoch(EpochId(0));
        assert!(epoch0.is_some());
        assert_eq!(epoch0.unwrap().epoch_id, EpochId(0));
    }

    #[test]
    fn test_get_nonexistent_epoch_returns_none() {
        let manager = make_manager(5);
        assert!(manager.get_epoch(EpochId(99)).is_none());
    }
}
