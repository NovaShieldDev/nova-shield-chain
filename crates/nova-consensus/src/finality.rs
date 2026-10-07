//! Tek-slot kesinlik izleyici — §5.10 "Her blok üretildiği slot'ta kesinleşir"
//!
//! Araştırma kaynakları:
//! - Avalanchego simplex `storage.go`:
//!   `Storage.Accept(b Block)` → finalized block store, `GetBlock(blkID)`, `LastAccepted()`.
//!   Bizde: `FinalityTracker::record_finalized` + `get_finalized_block`.
//! - Polkadot GRANDPA `finalize_block`:
//!   `best_finalized_number`, `FinalizedBlock { block_hash, round }`.
//!   Bizde: `FinalityTracker::last_finalized_height` + `FinalizedBlockRecord`.
//! - go-ethereum `blockchain.go SetFinalized(header)`:
//!   finalized chain head tracking, `currentFinalBlock`.
//!   Bizde: `FinalityTracker::set_finalized_head` ile son kesinleşmiş blok.

use crate::errors::{ConsensusError, ConsensusResult};
use crate::types::{Block, CheckpointRecord, ConsensusEvent, ConsensusEventType, QuorumCertificate, CHECKPOINT_INTERVAL};
use nova_primitives::{BlockHash, EpochId, SlotId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

// ─── FinalizedBlockRecord ──────────────────────────────────────────────────────

/// Kesinleşmiş blok kaydı.
///
/// Pattern: Avalanchego `storage.go` accepted block record.
#[derive(Debug, Clone)]
pub struct FinalizedBlockRecord {
    /// Blok.
    pub block: Block,
    /// QC (quorum certificate).
    pub qc: QuorumCertificate,
    /// Kesinleştirildiği slot.
    pub finalized_at_slot: SlotId,
}

// ─── FinalityTracker ──────────────────────────────────────────────────────────

/// Kesinleşmiş blok takipçisi.
///
/// Pattern: Avalanchego `simplex.Storage` + Polkadot GRANDPA finality gadget.
/// Tek-slot kesinlik: her blok BFT quorum'u tamamlanınca kesinleşir.
pub struct FinalityTracker {
    /// Kesinleşmiş bloklar: height → kayıt.
    finalized_blocks: Arc<RwLock<HashMap<u64, FinalizedBlockRecord>>>,
    /// Hash → height indeksi (hızlı arama).
    hash_to_height: Arc<RwLock<HashMap<BlockHash, u64>>>,
    /// Son kesinleşmiş yükseklik.
    last_finalized_height: Arc<RwLock<u64>>,
    /// Checkpoint kayıtları: height → checkpoint.
    checkpoints: Arc<RwLock<HashMap<u64, CheckpointRecord>>>,
}

impl FinalityTracker {
    /// Yeni FinalityTracker (genesis → height=0 finalized).
    pub fn new(genesis_block: Block) -> Self {
        let genesis_height = genesis_block.header.height;
        let genesis_hash = genesis_block.block_hash;

        let genesis_qc = QuorumCertificate {
            block_hash: genesis_hash,
            height: genesis_height,
            signers: vec![],
            aggregate_signature: vec![0u8; 96],
            signed_at_slot: SlotId(0),
        };

        let record = FinalizedBlockRecord {
            block: genesis_block,
            qc: genesis_qc,
            finalized_at_slot: SlotId(0),
        };

        let mut finalized = HashMap::new();
        finalized.insert(genesis_height, record);

        let mut idx = HashMap::new();
        idx.insert(genesis_hash, genesis_height);

        Self {
            finalized_blocks: Arc::new(RwLock::new(finalized)),
            hash_to_height: Arc::new(RwLock::new(idx)),
            last_finalized_height: Arc::new(RwLock::new(genesis_height)),
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ─── Kesinleştirme ────────────────────────────────────────────────────────

    /// Bloğu kesinleştir.
    ///
    /// Pattern: Avalanchego `Storage.Accept(b)` — bloğu kalıcı depoya yaz.
    /// Çağıran: `BftEngine::finalize_block` QC doğrulandıktan sonra.
    pub fn record_finalized(
        &self,
        block: Block,
        qc: QuorumCertificate,
        current_slot: SlotId,
    ) -> ConsensusResult<Vec<ConsensusEvent>> {
        let height = block.header.height;
        let block_hash = block.block_hash;

        // Zaten kesinleşmişse hata
        if self.finalized_blocks.read().contains_key(&height) {
            return Err(ConsensusError::AlreadyFinalized { height });
        }

        // Sıralı kesinleşme kontrolü (genesis hariç)
        if height > 0 {
            let last = *self.last_finalized_height.read();
            if height != last + 1 {
                return Err(ConsensusError::HeightMismatch {
                    expected: last + 1,
                    got: height,
                });
            }
        }

        let mut events = Vec::new();

        // Kayıt
        let record = FinalizedBlockRecord {
            block: block.clone(),
            qc: qc.clone(),
            finalized_at_slot: current_slot,
        };

        {
            let mut blocks = self.finalized_blocks.write();
            let mut idx = self.hash_to_height.write();
            blocks.insert(height, record);
            idx.insert(block_hash, height);
        }

        *self.last_finalized_height.write() = height;

        events.push(ConsensusEvent {
            slot: current_slot,
            event_type: ConsensusEventType::BlockFinalized {
                height,
                block_hash,
            },
        });

        info!(
            target: "nova_consensus",
            height,
            slot = current_slot.0,
            "blok kesinleşti"
        );

        // Checkpoint kontrolü
        if height % CHECKPOINT_INTERVAL == 0 && height > 0 {
            let epoch = block.header.epoch;
            let state_root = block.header.state_root;
            let cp = self.create_checkpoint(height, block_hash, state_root, epoch, qc, current_slot);
            events.push(ConsensusEvent {
                slot: current_slot,
                event_type: ConsensusEventType::CheckpointCreated {
                    height,
                    block_hash,
                },
            });
            debug!(
                target: "nova_consensus",
                height,
                "checkpoint oluşturuldu"
            );
            self.checkpoints.write().insert(height, cp);
        }

        Ok(events)
    }

    // ─── Sorgular ─────────────────────────────────────────────────────────────

    /// Bu hash kesinleşti mi?
    pub fn is_finalized_hash(&self, hash: &BlockHash) -> bool {
        self.hash_to_height.read().contains_key(hash)
    }

    /// Bu height kesinleşti mi?
    pub fn is_finalized_height(&self, height: u64) -> bool {
        self.finalized_blocks.read().contains_key(&height)
    }

    /// Son kesinleşmiş yükseklik.
    pub fn last_finalized_height(&self) -> u64 {
        *self.last_finalized_height.read()
    }

    /// Kesinleşmiş bloğu al (height ile).
    pub fn get_finalized_block(&self, height: u64) -> Option<FinalizedBlockRecord> {
        self.finalized_blocks.read().get(&height).cloned()
    }

    /// Kesinleşmiş bloğu hash ile al.
    pub fn get_finalized_block_by_hash(&self, hash: &BlockHash) -> Option<FinalizedBlockRecord> {
        let height = *self.hash_to_height.read().get(hash)?;
        self.finalized_blocks.read().get(&height).cloned()
    }

    /// Toplam kesinleşmiş blok sayısı.
    pub fn finalized_count(&self) -> usize {
        self.finalized_blocks.read().len()
    }

    // ─── Checkpoint ───────────────────────────────────────────────────────────

    /// Checkpoint al.
    pub fn get_checkpoint(&self, height: u64) -> Option<CheckpointRecord> {
        self.checkpoints.read().get(&height).cloned()
    }

    /// Son checkpoint height'ı.
    pub fn last_checkpoint_height(&self) -> Option<u64> {
        let last = *self.last_finalized_height.read();
        // Son checkpoint = en büyük CHECKPOINT_INTERVAL katı
        let cp_height = (last / CHECKPOINT_INTERVAL) * CHECKPOINT_INTERVAL;
        if cp_height == 0 {
            None
        } else {
            Some(cp_height)
        }
    }

    /// Checkpoint oluştur (iç yardımcı).
    fn create_checkpoint(
        &self,
        height: u64,
        block_hash: BlockHash,
        state_root: [u8; 32],
        epoch: EpochId,
        qc: QuorumCertificate,
        created_at_slot: SlotId,
    ) -> CheckpointRecord {
        CheckpointRecord {
            height,
            block_hash,
            state_root,
            epoch,
            qc,
            created_at_slot,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockHeader, CHECKPOINT_INTERVAL};
    use nova_primitives::{BlockHash, EpochId, SlotId, ValidatorId};

    fn make_block(height: u64, parent_hash: BlockHash) -> Block {
        let header = BlockHeader {
            height,
            slot: SlotId(height * 10),
            epoch: EpochId(height / 100),
            parent_hash,
            state_root: [height as u8; 32],
            proposer: ValidatorId(1),
            timestamp: height * 400,
            tx_count: 0,
        };
        Block::new(header)
    }

    fn make_qc(block: &Block) -> QuorumCertificate {
        QuorumCertificate {
            block_hash: block.block_hash,
            height: block.header.height,
            signers: vec![ValidatorId(1)],
            aggregate_signature: vec![0u8; 96],
            signed_at_slot: block.header.slot,
        }
    }

    fn genesis() -> Block {
        let header = BlockHeader {
            height: 0,
            slot: SlotId(0),
            epoch: EpochId(0),
            parent_hash: BlockHash::from_bytes([0u8; 32]),
            state_root: [0u8; 32],
            proposer: ValidatorId(0),
            timestamp: 0,
            tx_count: 0,
        };
        Block::new(header)
    }

    // ─── genesis ──────────────────────────────────────────────────────────────

    #[test]
    fn test_genesis_is_finalized() {
        let gen = genesis();
        let gen_hash = gen.block_hash;
        let tracker = FinalityTracker::new(gen);
        assert!(tracker.is_finalized_height(0));
        assert!(tracker.is_finalized_hash(&gen_hash));
        assert_eq!(tracker.last_finalized_height(), 0);
    }

    #[test]
    fn test_genesis_finalized_count_one() {
        let tracker = FinalityTracker::new(genesis());
        assert_eq!(tracker.finalized_count(), 1);
    }

    // ─── record_finalized ─────────────────────────────────────────────────────

    #[test]
    fn test_finalize_height_1() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let b1 = make_block(1, gen.block_hash);
        let qc = make_qc(&b1);
        let events = tracker.record_finalized(b1.clone(), qc, SlotId(10)).unwrap();

        assert!(tracker.is_finalized_height(1));
        assert!(tracker.is_finalized_hash(&b1.block_hash));
        assert_eq!(tracker.last_finalized_height(), 1);
        assert!(!events.is_empty());
    }

    #[test]
    fn test_finalize_emits_block_finalized_event() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let b1 = make_block(1, gen.block_hash);
        let qc = make_qc(&b1);
        let events = tracker.record_finalized(b1.clone(), qc, SlotId(10)).unwrap();

        let has_event = events.iter().any(|e| {
            matches!(
                e.event_type,
                ConsensusEventType::BlockFinalized { height: 1, .. }
            )
        });
        assert!(has_event);
    }

    #[test]
    fn test_finalize_sequential_blocks() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let mut parent = gen.block_hash;
        for h in 1..=5u64 {
            let b = make_block(h, parent);
            let qc = make_qc(&b);
            parent = b.block_hash;
            tracker.record_finalized(b, qc, SlotId(h * 10)).unwrap();
        }

        assert_eq!(tracker.last_finalized_height(), 5);
        assert_eq!(tracker.finalized_count(), 6); // genesis + 5
    }

    #[test]
    fn test_finalize_already_finalized_returns_error() {
        let gen = genesis();
        let gen_hash = gen.block_hash;
        let tracker = FinalityTracker::new(gen.clone());

        let b1 = make_block(1, gen_hash);
        let qc = make_qc(&b1);
        tracker.record_finalized(b1.clone(), qc.clone(), SlotId(10)).unwrap();

        // Aynı height'ı tekrar finalize et
        let err = tracker.record_finalized(b1, qc, SlotId(11));
        assert!(matches!(err, Err(ConsensusError::AlreadyFinalized { height: 1 })));
    }

    #[test]
    fn test_finalize_out_of_order_returns_error() {
        let gen = genesis();
        let gen_hash = gen.block_hash;
        let tracker = FinalityTracker::new(gen);

        // height=2 atla, height=3 dene
        let b3 = make_block(3, gen_hash);
        let qc = make_qc(&b3);
        let err = tracker.record_finalized(b3, qc, SlotId(30));
        assert!(matches!(err, Err(ConsensusError::HeightMismatch { expected: 1, got: 3 })));
    }

    // ─── get_finalized_block ──────────────────────────────────────────────────

    #[test]
    fn test_get_finalized_block_by_height() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let b1 = make_block(1, gen.block_hash);
        let qc = make_qc(&b1);
        tracker.record_finalized(b1.clone(), qc, SlotId(10)).unwrap();

        let record = tracker.get_finalized_block(1).unwrap();
        assert_eq!(record.block.header.height, 1);
    }

    #[test]
    fn test_get_finalized_block_by_hash() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let b1 = make_block(1, gen.block_hash);
        let hash = b1.block_hash;
        let qc = make_qc(&b1);
        tracker.record_finalized(b1, qc, SlotId(10)).unwrap();

        let record = tracker.get_finalized_block_by_hash(&hash).unwrap();
        assert_eq!(record.block.header.height, 1);
    }

    #[test]
    fn test_get_nonexistent_block_returns_none() {
        let tracker = FinalityTracker::new(genesis());
        assert!(tracker.get_finalized_block(99).is_none());
        assert!(tracker.get_finalized_block_by_hash(&BlockHash::from_bytes([0xFFu8; 32])).is_none());
    }

    // ─── checkpoint ───────────────────────────────────────────────────────────

    #[test]
    fn test_checkpoint_created_at_interval() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let mut parent = gen.block_hash;
        for h in 1..=(CHECKPOINT_INTERVAL as u64) {
            let b = make_block(h, parent);
            let qc = make_qc(&b);
            parent = b.block_hash;
            tracker.record_finalized(b, qc, SlotId(h)).unwrap();
        }

        let cp = tracker.get_checkpoint(CHECKPOINT_INTERVAL as u64);
        assert!(cp.is_some());
        assert_eq!(cp.unwrap().height, CHECKPOINT_INTERVAL as u64);
    }

    #[test]
    fn test_no_checkpoint_before_interval() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let b1 = make_block(1, gen.block_hash);
        let qc = make_qc(&b1);
        tracker.record_finalized(b1, qc, SlotId(1)).unwrap();

        // CHECKPOINT_INTERVAL = 100, henüz çok erken
        assert!(tracker.get_checkpoint(1).is_none());
    }

    #[test]
    fn test_checkpoint_event_emitted() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        let mut parent = gen.block_hash;
        let mut checkpoint_events = vec![];

        for h in 1..=(CHECKPOINT_INTERVAL as u64) {
            let b = make_block(h, parent);
            let qc = make_qc(&b);
            parent = b.block_hash;
            let events = tracker.record_finalized(b, qc, SlotId(h)).unwrap();
            for e in events {
                if matches!(e.event_type, ConsensusEventType::CheckpointCreated { .. }) {
                    checkpoint_events.push(e);
                }
            }
        }

        assert_eq!(checkpoint_events.len(), 1);
    }

    #[test]
    fn test_last_checkpoint_height() {
        let gen = genesis();
        let tracker = FinalityTracker::new(gen.clone());

        // Henüz checkpoint yok
        assert!(tracker.last_checkpoint_height().is_none());

        let mut parent = gen.block_hash;
        for h in 1..=(CHECKPOINT_INTERVAL as u64) {
            let b = make_block(h, parent);
            let qc = make_qc(&b);
            parent = b.block_hash;
            tracker.record_finalized(b, qc, SlotId(h)).unwrap();
        }

        assert_eq!(
            tracker.last_checkpoint_height(),
            Some(CHECKPOINT_INTERVAL as u64)
        );
    }
}
