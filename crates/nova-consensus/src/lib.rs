//! nova-consensus — Simplex BFT + DPoS + Slashing (§5.10)
//!
//! Modüller:
//! - `errors`     — ConsensusError
//! - `types`      — temel tipler ve sabitler
//! - `validator`  — ValidatorRegistry (register, stake, unbond, slash, jail)
//! - `election`   — ElectionEngine (Phragmén DPoS)
//! - `epoch`      — EpochManager (24h rotasyon)
//! - `bft`        — BftEngine (Simplex BFT, quorum, QC)
//! - `finality`   — FinalityTracker (tek-slot kesinlik)
//! - `slashing`   — SlashingEngine (double-sign, downtime, mempool, batch)
//! - `checkpoint` — CheckpointManager (periyodik kesinlik çıpası)

pub mod bft;
pub mod equivocation;
pub mod checkpoint;
pub mod election;
pub mod epoch;
pub mod epoch_randomness;
pub mod errors;
pub mod finality;
pub mod grandpa;
pub mod signing;
pub mod slashing;
pub mod snowball;
pub mod types;
pub mod uptime;
pub mod validator;
pub mod vrf;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use bft::{BftEngine, Vote};
pub use signing::{
    AggregateVote, BftSigner, BftVerifier, SignedVote, SigningError, SigningResult, vote_message,
};
pub use checkpoint::CheckpointManager;
pub use election::{ElectionEngine, ElectionResult, ElectionScore};
pub use epoch::EpochManager;
pub use errors::{ConsensusError, ConsensusResult};
pub use finality::{FinalityTracker, FinalizedBlockRecord};
pub use slashing::{
    GraduatedSlashCalculator, SlashTier, SlashingEngine, ValidatorSlashHistory,
};
pub use types::{
    // Sabitler
    ACTIVE_VALIDATOR_COUNT,
    CHECKPOINT_INTERVAL,
    EPOCH_DURATION_SLOTS,
    MAX_EVIDENCE_AGE_SLOTS,
    MIN_VALIDATOR_STAKE,
    MIN_SIGNED_PER_WINDOW_BPS,
    QUORUM_DENOMINATOR,
    QUORUM_NUMERATOR,
    SIGNED_BLOCKS_WINDOW,
    SLASH_DOUBLE_SIGN_BPS,
    SLASH_DOWNTIME_BPS,
    SLASH_MEMPOOL_LEAK_BPS,
    SLASH_WRONG_BATCH_BPS,
    SLOT_DURATION_MS,
    UNBONDING_PERIOD_SLOTS,
    // Tipler
    Block,
    BlockHeader,
    CheckpointRecord,
    ConsensusEvent,
    ConsensusEventType,
    DoubleSignEvidence,
    EpochInfo,
    QuorumCertificate,
    SlashReason,
    SlashRecord,
    ValidatorInfo,
    ValidatorStatus,
    // Fonksiyonlar
    block_hash_to_hex,
    quorum_threshold,
    // Slot zamanlayıcı (BUG-2 fix)
    SlotTimer,
};
pub use uptime::{
    uptime_tier, UptimeTier, UptimeTracker, ValidatorUptimeRecord,
    REWARD_MULTIPLIER_TIER1_BPS, REWARD_MULTIPLIER_TIER2_BPS, REWARD_MULTIPLIER_TIER3_BPS,
    UPTIME_TIER1_BPS, UPTIME_TIER2_BPS, UPTIME_TIER3_BPS, UPTIME_WINDOW,
};
pub use epoch_randomness::{
    compute_epoch_randomness, EpochRandomnessAccumulator, EpochRandomnessHistory,
    EpochRandomnessManager, EpochRandomnessRecord, RandomnessSegment,
    RANDOMNESS_HISTORY_DEPTH, RANDOMNESS_WINDOW_SLOTS,
};
pub use snowball::{
    SnowballConfig, SnowballEngine, SnowballInstance, SnowballPreference,
    SNOWBALL_ALPHA, SNOWBALL_BETA_ROGUE, SNOWBALL_BETA_VIRTUOUS, SNOWBALL_CONCURRENT, SNOWBALL_K,
};
pub use grandpa::{
    GrandpaEngine, GrandpaError, GrandpaRound, GrandpaVote, VoteType,
    GRANDPA_QUORUM_DEN, GRANDPA_QUORUM_NUM, MAX_GRANDPA_ROUNDS,
};
pub use equivocation::{
    EquivocationReport, EquivocationReportBuilder, EquivocationReporter, EquivocationVote,
    MAX_REPORTS_PER_REPORTER_PER_WINDOW, REPORTER_RATE_WINDOW_SLOTS,
};
pub use validator::ValidatorRegistry;
pub use vrf::{
    BabeEngine, BabeEpochConfig, BabeSlotClaim, VrfKeypair, VrfProof,
    BABE_EPOCH_DURATION, PRIMARY_SLOT_THRESHOLD, VRF_OUTPUT_SIZE,
};

// ─── Integration Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{Address, Balance, BlockHash, EpochId, SlotId, ValidatorId};
    use std::sync::Arc;

    // ─── Yardımcılar ──────────────────────────────────────────────────────────

    fn make_vid(seed: u8) -> ValidatorId {
        ValidatorId(seed as u64)
    }

    fn make_addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_validators_in_registry(count: u8) -> Arc<ValidatorRegistry> {
        let reg = Arc::new(ValidatorRegistry::new());
        for i in 1..=count {
            reg.register(
                make_vid(i),
                make_addr(i),
                Balance(MIN_VALIDATOR_STAKE * (count - i + 1) as u128),
                500,
                [i; 32],
                SlotId(0),
            )
            .unwrap();
        }
        reg
    }

    fn make_genesis() -> Block {
        let header = BlockHeader {
            height: 0,
            slot: SlotId(0),
            epoch: EpochId(0),
            parent_hash: BlockHash::from_bytes([0u8; 32]),
            state_root: [0u8; 32],
            proposer: make_vid(1),
            timestamp: 0,
            tx_count: 0,
        };
        Block::new(header)
    }

    fn make_block(height: u64, parent: BlockHash) -> Block {
        let header = BlockHeader {
            height,
            slot: SlotId(height * 10),
            epoch: EpochId(0),
            parent_hash: parent,
            state_root: [height as u8; 32],
            proposer: make_vid(1),
            timestamp: height * 400,
            tx_count: 0,
        };
        Block::new(header)
    }

    // ─── Tam BFT → Finality akışı ─────────────────────────────────────────────

    #[test]
    fn test_full_bft_finality_pipeline() {
        // Gerçek BLS imzalı pipeline: NOVA-RED-CONSENSUS-002 düzeltmesi sonrası
        // process_signed_vote + new_with_pubkeys kullanılmalı
        let genesis = make_genesis();

        // BLS pubkey'leri ile engine oluştur
        let validators_with_pks: Vec<(ValidatorId, nova_primitives::BlsPublicKey)> = (1..=5u8)
            .map(|i| {
                let ikm = [(i as u8).wrapping_mul(37); 32];
                let kp = nova_crypto::BlsKeypair::from_ikm(&ikm).unwrap();
                (make_vid(i), kp.bls_public_key())
            })
            .collect();
        let bft = BftEngine::new_with_pubkeys(validators_with_pks);
        let finality = FinalityTracker::new(genesis.clone());

        // Blok 1
        bft.verify_block(&genesis).unwrap();
        let b1 = make_block(1, genesis.block_hash);
        bft.verify_block(&b1).unwrap();

        // 5 validator gerçek BLS SignedVote kullan
        let mut qc = None;
        for i in 1u8..=5 {
            let ikm = [(i as u8).wrapping_mul(37); 32];
            let signer = BftSigner::from_ikm(make_vid(i), &ikm).unwrap();
            let signed = signer.sign_vote(b1.block_hash, 1, SlotId(10)).unwrap();
            if let Some(q) = bft.process_signed_vote(signed, SlotId(10)).unwrap() {
                qc = Some(q);
            }
        }

        let qc = qc.expect("QC üretilmeli");
        let finalized = bft.finalize_block(b1.block_hash, &qc, SlotId(10)).unwrap();
        let events = finality.record_finalized(finalized, qc, SlotId(10)).unwrap();

        assert!(finality.is_finalized_height(1));
        assert!(!events.is_empty());
    }

    // ─── Epoch geçişi → yeni validator seti ──────────────────────────────────

    #[test]
    fn test_epoch_transition_updates_validator_set() {
        let validators: Vec<ValidatorId> = (1..=5u8).map(make_vid).collect();
        let total = Balance(MIN_VALIDATOR_STAKE * 5);
        let manager = EpochManager::new(validators, total);

        let candidates: Vec<types::ValidatorInfo> = (1..=5u8)
            .map(|i| {
                types::ValidatorInfo::new(
                    make_vid(i),
                    make_addr(i),
                    Balance(MIN_VALIDATOR_STAKE * i as u128),
                    500,
                    SlotId(0),
                    [i; 32],
                )
            })
            .collect();

        let (transitioned, events) =
            manager.process_slot(SlotId(EPOCH_DURATION_SLOTS), &candidates);

        assert!(transitioned);
        assert_eq!(manager.current_epoch_id(), nova_primitives::EpochId(1));

        let has_transition = events.iter().any(|e| {
            matches!(
                e.event_type,
                ConsensusEventType::EpochTransition { old_epoch: 0, new_epoch: 1, .. }
            )
        });
        assert!(has_transition);
    }

    // ─── Double-sign tespiti → tombstone + event ──────────────────────────────

    #[test]
    fn test_double_sign_tombstone_pipeline() {
        let reg = make_validators_in_registry(3);
        let engine = SlashingEngine::new(Arc::clone(&reg));
        let v = make_vid(1);

        let evidence = DoubleSignEvidence {
            validator: v,
            height: 50,
            block_hash_a: BlockHash::from_bytes([0xAAu8; 32]),
            block_hash_b: BlockHash::from_bytes([0xBBu8; 32]),
            submitted_at_slot: SlotId(1_000),
        };

        let events = engine.handle_double_sign(evidence, SlotId(1_000)).unwrap();

        // Tombstone + slash events
        assert!(events
            .iter()
            .any(|e| matches!(e.event_type, ConsensusEventType::ValidatorTombstoned { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.event_type, ConsensusEventType::ValidatorSlashed { .. })));

        // Registry'de tombstoned
        let info = reg.get(&v).unwrap();
        assert!(info.status.is_tombstoned());
    }

    // ─── Checkpoint pipeline ──────────────────────────────────────────────────

    #[test]
    fn test_checkpoint_pipeline() {
        let manager = CheckpointManager::new();

        let ci = CHECKPOINT_INTERVAL as u64;
        let cp = CheckpointRecord {
            height: ci,
            block_hash: BlockHash::from_bytes([ci as u8; 32]),
            state_root: [0u8; 32],
            epoch: EpochId(0),
            qc: QuorumCertificate {
                block_hash: BlockHash::from_bytes([ci as u8; 32]),
                height: ci,
                signers: vec![],
                aggregate_signature: vec![0u8; 96],
                signed_at_slot: SlotId(ci),
            },
            created_at_slot: SlotId(ci),
        };

        manager.record_checkpoint(cp).unwrap();
        assert!(manager.has_checkpoint(ci));
        assert_eq!(manager.latest_height(), Some(ci));
    }

    // ─── Election → en yüksek stake kazanır ───────────────────────────────────

    #[test]
    fn test_election_selects_highest_stake() {
        let candidates: Vec<types::ValidatorInfo> = (1..=5u8)
            .map(|i| {
                types::ValidatorInfo::new(
                    make_vid(i),
                    make_addr(i),
                    Balance(MIN_VALIDATOR_STAKE * i as u128),
                    500,
                    SlotId(0),
                    [i; 32],
                )
            })
            .collect();

        let result = ElectionEngine::elect(&candidates, 3);
        assert_eq!(result.winners.len(), 3);
        // En yüksek stake = i=5
        assert_eq!(result.winners[0].0, make_vid(5));
    }

    // ─── ValidatorRegistry CRUD ───────────────────────────────────────────────

    #[test]
    fn test_registry_register_and_get() {
        let reg = ValidatorRegistry::new();
        let v = make_vid(10);

        reg.register(
            v,
            make_addr(10),
            Balance(MIN_VALIDATOR_STAKE),
            300,
            [10u8; 32],
            SlotId(0),
        )
        .unwrap();

        let info = reg.get(&v).unwrap();
        assert_eq!(info.validator_id, v);
        assert!(info.status.is_active());
    }

    #[test]
    fn test_registry_duplicate_register_errors() {
        let reg = ValidatorRegistry::new();
        let v = make_vid(11);

        reg.register(v, make_addr(11), Balance(MIN_VALIDATOR_STAKE), 300, [11u8; 32], SlotId(0))
            .unwrap();

        let err = reg.register(
            v,
            make_addr(11),
            Balance(MIN_VALIDATOR_STAKE),
            300,
            [11u8; 32],
            SlotId(0),
        );
        assert!(matches!(err, Err(ConsensusError::ValidatorAlreadyRegistered { .. })));
    }

    // ─── BFT quorum eşiği ─────────────────────────────────────────────────────

    #[test]
    fn test_quorum_threshold_values() {
        assert_eq!(quorum_threshold(1), 1);
        assert_eq!(quorum_threshold(3), 2);  // ceil(2*3/3) = 2
        assert_eq!(quorum_threshold(4), 3);
        assert_eq!(quorum_threshold(10), 7);
        assert_eq!(quorum_threshold(200), 134);
    }

    // ─── BFT quorum eksik → QC üretilmez ─────────────────────────────────────

    #[test]
    fn test_bft_no_qc_without_quorum() {
        let reg = make_validators_in_registry(10);
        let active = reg.get_active_validators();
        let active_ids: Vec<ValidatorId> = active.iter().map(|v| v.validator_id).collect();
        let bft = BftEngine::new(active_ids);

        let genesis = make_genesis();
        bft.verify_block(&genesis).unwrap();
        let b1 = make_block(1, genesis.block_hash);
        bft.verify_block(&b1).unwrap();

        // Sadece 1 oy — 10 validator'da quorum = 7
        let vote = Vote::mock(make_vid(1), b1.block_hash, 1, SlotId(10));
        let result = bft.process_vote(vote, SlotId(10)).unwrap();
        assert!(result.is_none(), "1 oy yeterli değil");
    }

    // ─── Downtime slash ───────────────────────────────────────────────────────

    #[test]
    fn test_downtime_slash_reduces_stake() {
        let reg = make_validators_in_registry(3);
        let v = make_vid(1);
        let initial_stake = reg.get(&v).unwrap().stake.0;

        let engine = SlashingEngine::new(Arc::clone(&reg));
        engine.slash_downtime(v, SlotId(500)).unwrap();

        let after_stake = reg.get(&v).unwrap().stake.0;
        assert!(after_stake < initial_stake);
    }

    // ─── Slashing geçmişi ─────────────────────────────────────────────────────

    #[test]
    fn test_slash_history_tracks_all_types() {
        let reg = Arc::new(ValidatorRegistry::new());
        for i in 1u8..=3 {
            reg.register(
                make_vid(i),
                make_addr(i),
                Balance(MIN_VALIDATOR_STAKE * 10),
                500,
                [i; 32],
                SlotId(0),
            )
            .unwrap();
        }
        let engine = SlashingEngine::new(Arc::clone(&reg));

        engine.slash_downtime(make_vid(1), SlotId(100)).unwrap();
        engine.slash_mempool_leak(make_vid(2), SlotId(200)).unwrap();
        engine.slash_wrong_batch(make_vid(3), SlotId(300)).unwrap();

        assert_eq!(engine.total_slash_count(), 3);
    }

    // ─── FinalityTracker hash indeksi ─────────────────────────────────────────

    #[test]
    fn test_finality_hash_lookup() {
        let genesis = make_genesis();
        let tracker = FinalityTracker::new(genesis.clone());

        let b1 = make_block(1, genesis.block_hash);
        let hash = b1.block_hash;

        let qc = QuorumCertificate {
            block_hash: hash,
            height: 1,
            signers: vec![],
            aggregate_signature: vec![0u8; 96],
            signed_at_slot: SlotId(10),
        };
        tracker.record_finalized(b1, qc, SlotId(10)).unwrap();

        assert!(tracker.is_finalized_hash(&hash));
        let rec = tracker.get_finalized_block_by_hash(&hash).unwrap();
        assert_eq!(rec.block.header.height, 1);
    }

    // ─── EpochManager slot proposer ───────────────────────────────────────────

    #[test]
    fn test_epoch_slot_proposer_round_robin() {
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

    // ─── ElectionEngine doğrulama ─────────────────────────────────────────────

    #[test]
    fn test_election_verify_result() {
        let candidates: Vec<types::ValidatorInfo> = (1..=5u8)
            .map(|i| {
                types::ValidatorInfo::new(
                    make_vid(i),
                    make_addr(i),
                    Balance(MIN_VALIDATOR_STAKE * i as u128),
                    500,
                    SlotId(0),
                    [i; 32],
                )
            })
            .collect();

        let result = ElectionEngine::elect(&candidates, 5);
        assert!(ElectionEngine::verify_result(&result));
    }

    // ─── block_hash_to_hex yardımcı ───────────────────────────────────────────

    #[test]
    fn test_block_hash_to_hex() {
        let hash = BlockHash::from_bytes([0xABu8; 32]);
        let hex = block_hash_to_hex(&hash);
        assert!(hex.starts_with("abab"));
        assert_eq!(hex.len(), 64);
    }
}
