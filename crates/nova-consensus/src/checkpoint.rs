//! Checkpoint yöneticisi — periyodik kesinlik çıpası (§5.10)
//!
//! Araştırma kaynakları:
//! - Polkadot GRANDPA `authority_set.rs`:
//!   `AuthoritySet::latest_finalized_block_number` → periyodik finalite checkpoint.
//!   Bizde: `CheckpointManager::latest_checkpoint`.
//! - Cosmos SDK x/staking `HistoricalInfo`:
//!   `SetHistoricalInfo(ctx, height, &hi)` — her blokta tarihsel bilgi yaz.
//!   Bizde: `CheckpointManager::record_checkpoint` — her `CHECKPOINT_INTERVAL` blokta.
//! - go-ethereum `core/rawdb checkpoints`:
//!   Trusted checkpoint list, `TrustedCheckpoint { SectionIndex, SectionHead, CHTRoot, BloomRoot }`.
//!   Bizde: `CheckpointRecord { height, block_hash, state_root, epoch, qc }`.
//! - Avalanchego `proposervm/state/state.go`:
//!   `State.SetLastAccepted(blkID)` — son kabul edilen blok takibi.
//!   Bizde: `CheckpointManager::latest_height`.

use crate::errors::{ConsensusError, ConsensusResult};
use crate::types::{CheckpointRecord, CHECKPOINT_INTERVAL};
use nova_primitives::{BlockHash, SlotId};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{debug, info};

// ─── CheckpointManager ────────────────────────────────────────────────────────

/// Checkpoint yöneticisi.
///
/// Pattern: Polkadot GRANDPA finality anchors + go-ethereum trusted checkpoints.
/// Her `CHECKPOINT_INTERVAL` blokta bir checkpoint oluşturulur.
/// Checkpoint = QC ile doğrulanmış kesinleşmiş blok özeti.
pub struct CheckpointManager {
    /// Checkpoint kayıtları: height → CheckpointRecord (BTreeMap → sıralı).
    checkpoints: Arc<RwLock<BTreeMap<u64, CheckpointRecord>>>,
    /// Hash → height indeksi.
    hash_to_height: Arc<RwLock<std::collections::HashMap<BlockHash, u64>>>,
}

impl CheckpointManager {
    /// Yeni CheckpointManager.
    pub fn new() -> Self {
        Self {
            checkpoints: Arc::new(RwLock::new(BTreeMap::new())),
            hash_to_height: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    // ─── Kayıt ────────────────────────────────────────────────────────────────

    /// Checkpoint kaydet.
    ///
    /// Pattern: Cosmos SDK `SetHistoricalInfo` — height bazlı kayıt.
    /// Sadece `CHECKPOINT_INTERVAL` katı olan height'lar kabul edilir.
    pub fn record_checkpoint(&self, record: CheckpointRecord) -> ConsensusResult<()> {
        let height = record.height;

        // Sadece interval katları
        if height % CHECKPOINT_INTERVAL != 0 {
            return Err(ConsensusError::InvalidCheckpoint);
        }

        // Genesis (height=0) kabul edilmez
        if height == 0 {
            return Err(ConsensusError::InvalidCheckpoint);
        }

        // NOVA-RED-CONSENSUS-004: QC temel doğrulaması.
        // Boş signer listesi veya boş/yanlış boyut aggregate_signature → sahte checkpoint.
        // Gerçek QC doğrulaması BftEngine::verify_qc ile yapılır (finalize_block sırasında).
        // Burada en az yapısal bütünlük kontrol edilir.
        if record.qc.signers.is_empty() {
            return Err(ConsensusError::InvalidCheckpoint);
        }
        if record.qc.aggregate_signature.len() != 96 {
            return Err(ConsensusError::InvalidCheckpoint);
        }
        // QC height ile checkpoint height eşleşmeli
        if record.qc.height != height {
            return Err(ConsensusError::InvalidCheckpoint);
        }
        // QC block hash ile checkpoint block hash eşleşmeli
        if record.qc.block_hash != record.block_hash {
            return Err(ConsensusError::InvalidCheckpoint);
        }

        // Zaten var mı?
        if self.checkpoints.read().contains_key(&height) {
            return Err(ConsensusError::AlreadyFinalized { height });
        }

        let block_hash = record.block_hash;

        self.checkpoints.write().insert(height, record);
        self.hash_to_height.write().insert(block_hash, height);

        info!(
            target: "nova_consensus",
            height,
            "checkpoint kaydedildi"
        );

        Ok(())
    }

    // ─── Sorgular ─────────────────────────────────────────────────────────────

    /// Checkpoint'i yüksekliğe göre al.
    pub fn get_checkpoint(&self, height: u64) -> Option<CheckpointRecord> {
        self.checkpoints.read().get(&height).cloned()
    }

    /// Checkpoint'i hash'e göre al.
    pub fn get_checkpoint_by_hash(&self, hash: &BlockHash) -> Option<CheckpointRecord> {
        let height = *self.hash_to_height.read().get(hash)?;
        self.checkpoints.read().get(&height).cloned()
    }

    /// Son checkpoint.
    ///
    /// Pattern: Polkadot `AuthoritySet::latest_finalized_block_number`.
    pub fn latest_checkpoint(&self) -> Option<CheckpointRecord> {
        self.checkpoints.read().values().next_back().cloned()
    }

    /// Son checkpoint yüksekliği.
    pub fn latest_height(&self) -> Option<u64> {
        self.checkpoints.read().keys().next_back().copied()
    }

    /// Toplam checkpoint sayısı.
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.read().len()
    }

    /// Tüm checkpoint'ler (yükseklik sırasıyla).
    pub fn all_checkpoints(&self) -> Vec<CheckpointRecord> {
        self.checkpoints.read().values().cloned().collect()
    }

    /// Belirli height aralığındaki checkpoint'ler.
    pub fn checkpoints_in_range(&self, from: u64, to: u64) -> Vec<CheckpointRecord> {
        self.checkpoints
            .read()
            .range(from..=to)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Bu height'ın checkpoint'i var mı?
    pub fn has_checkpoint(&self, height: u64) -> bool {
        self.checkpoints.read().contains_key(&height)
    }

    /// Checkpoint sayısından beklenen height (N. checkpoint).
    ///
    /// Yardımcı: N. checkpoint'in height'ını hesaplar.
    pub fn expected_checkpoint_height(n: u64) -> u64 {
        n * CHECKPOINT_INTERVAL
    }

    /// Bir sonraki checkpoint height'ı (mevcut height'a göre).
    pub fn next_checkpoint_height(&self, current_height: u64) -> u64 {
        let n = (current_height / CHECKPOINT_INTERVAL) + 1;
        n * CHECKPOINT_INTERVAL
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockHeader, QuorumCertificate, CHECKPOINT_INTERVAL};
    use nova_primitives::{BlockHash, EpochId, SlotId};

    fn make_qc(height: u64) -> QuorumCertificate {
        use nova_primitives::ValidatorId;
        QuorumCertificate {
            block_hash: BlockHash::from_bytes([height as u8; 32]),
            height,
            signers: vec![ValidatorId(1), ValidatorId(2), ValidatorId(3)], // NOVA-RED-CONSENSUS-004: boş değil
            aggregate_signature: vec![1u8; 96], // 96 byte geçerli boyut
            signed_at_slot: SlotId(height),
        }
    }

    fn make_checkpoint(height: u64) -> CheckpointRecord {
        CheckpointRecord {
            height,
            block_hash: BlockHash::from_bytes([height as u8; 32]),
            state_root: [0u8; 32],
            epoch: EpochId(height / 100),
            qc: make_qc(height),
            created_at_slot: SlotId(height),
        }
    }

    // ─── record_checkpoint ────────────────────────────────────────────────────

    #[test]
    fn test_record_checkpoint_at_interval() {
        let manager = CheckpointManager::new();
        let cp = make_checkpoint(CHECKPOINT_INTERVAL as u64);
        manager.record_checkpoint(cp).unwrap();
        assert_eq!(manager.checkpoint_count(), 1);
    }

    #[test]
    fn test_record_checkpoint_non_interval_errors() {
        let manager = CheckpointManager::new();
        let cp = make_checkpoint(50); // 50 % 100 != 0
        let err = manager.record_checkpoint(cp);
        assert!(matches!(err, Err(ConsensusError::InvalidCheckpoint)));
    }

    #[test]
    fn test_record_checkpoint_genesis_height_errors() {
        let manager = CheckpointManager::new();
        let cp = make_checkpoint(0);
        let err = manager.record_checkpoint(cp);
        assert!(matches!(err, Err(ConsensusError::InvalidCheckpoint)));
    }

    #[test]
    fn test_record_checkpoint_duplicate_errors() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        manager.record_checkpoint(make_checkpoint(h)).unwrap();
        let err = manager.record_checkpoint(make_checkpoint(h));
        assert!(matches!(err, Err(ConsensusError::AlreadyFinalized { .. })));
    }

    #[test]
    fn test_record_multiple_checkpoints() {
        let manager = CheckpointManager::new();
        for i in 1u64..=5 {
            manager
                .record_checkpoint(make_checkpoint(i * CHECKPOINT_INTERVAL as u64))
                .unwrap();
        }
        assert_eq!(manager.checkpoint_count(), 5);
    }

    // ─── get_checkpoint ───────────────────────────────────────────────────────

    #[test]
    fn test_get_checkpoint_by_height() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        manager.record_checkpoint(make_checkpoint(h)).unwrap();

        let cp = manager.get_checkpoint(h).unwrap();
        assert_eq!(cp.height, h);
    }

    #[test]
    fn test_get_checkpoint_by_hash() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        manager.record_checkpoint(make_checkpoint(h)).unwrap();

        let hash = BlockHash::from_bytes([h as u8; 32]);
        let cp = manager.get_checkpoint_by_hash(&hash).unwrap();
        assert_eq!(cp.height, h);
    }

    #[test]
    fn test_get_nonexistent_checkpoint_returns_none() {
        let manager = CheckpointManager::new();
        assert!(manager.get_checkpoint(999).is_none());
        assert!(manager.get_checkpoint_by_hash(&BlockHash::from_bytes([0xFFu8; 32])).is_none());
    }

    // ─── latest_checkpoint ────────────────────────────────────────────────────

    #[test]
    fn test_latest_checkpoint_empty() {
        let manager = CheckpointManager::new();
        assert!(manager.latest_checkpoint().is_none());
        assert!(manager.latest_height().is_none());
    }

    #[test]
    fn test_latest_checkpoint_returns_highest() {
        let manager = CheckpointManager::new();
        let ci = CHECKPOINT_INTERVAL as u64;
        manager.record_checkpoint(make_checkpoint(ci)).unwrap();
        manager.record_checkpoint(make_checkpoint(ci * 2)).unwrap();
        manager.record_checkpoint(make_checkpoint(ci * 3)).unwrap();

        let latest = manager.latest_checkpoint().unwrap();
        assert_eq!(latest.height, ci * 3);
        assert_eq!(manager.latest_height(), Some(ci * 3));
    }

    // ─── all_checkpoints ──────────────────────────────────────────────────────

    #[test]
    fn test_all_checkpoints_ordered() {
        let manager = CheckpointManager::new();
        let ci = CHECKPOINT_INTERVAL as u64;
        // Ters sırayla ekle
        manager.record_checkpoint(make_checkpoint(ci * 3)).unwrap();
        manager.record_checkpoint(make_checkpoint(ci * 1)).unwrap();
        manager.record_checkpoint(make_checkpoint(ci * 2)).unwrap();

        let all = manager.all_checkpoints();
        assert_eq!(all.len(), 3);
        // BTreeMap → sıralı gelir
        assert_eq!(all[0].height, ci);
        assert_eq!(all[1].height, ci * 2);
        assert_eq!(all[2].height, ci * 3);
    }

    // ─── checkpoints_in_range ─────────────────────────────────────────────────

    #[test]
    fn test_checkpoints_in_range() {
        let manager = CheckpointManager::new();
        let ci = CHECKPOINT_INTERVAL as u64;
        for i in 1u64..=5 {
            manager
                .record_checkpoint(make_checkpoint(i * ci))
                .unwrap();
        }

        let range = manager.checkpoints_in_range(ci * 2, ci * 4);
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].height, ci * 2);
        assert_eq!(range[2].height, ci * 4);
    }

    // ─── has_checkpoint ───────────────────────────────────────────────────────

    #[test]
    fn test_has_checkpoint() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        assert!(!manager.has_checkpoint(h));
        manager.record_checkpoint(make_checkpoint(h)).unwrap();
        assert!(manager.has_checkpoint(h));
    }

    // ─── expected_checkpoint_height / next_checkpoint_height ─────────────────

    #[test]
    fn test_expected_checkpoint_height() {
        assert_eq!(
            CheckpointManager::expected_checkpoint_height(1),
            CHECKPOINT_INTERVAL as u64
        );
        assert_eq!(
            CheckpointManager::expected_checkpoint_height(3),
            3 * CHECKPOINT_INTERVAL as u64
        );
    }

    #[test]
    fn test_next_checkpoint_height() {
        let manager = CheckpointManager::new();
        let ci = CHECKPOINT_INTERVAL as u64;
        assert_eq!(manager.next_checkpoint_height(0), ci);
        assert_eq!(manager.next_checkpoint_height(ci - 1), ci);
        assert_eq!(manager.next_checkpoint_height(ci), ci * 2);
        assert_eq!(manager.next_checkpoint_height(ci + 1), ci * 2);
    }

    // ─── NOVA-RED-CONSENSUS-004 regression testleri ───────────────────────────

    /// Boş signer listesi ile checkpoint kaydedilemez.
    #[test]
    fn test_checkpoint_empty_signers_rejected() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        let mut cp = make_checkpoint(h);
        cp.qc.signers = vec![]; // boş signer
        let err = manager.record_checkpoint(cp).unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidCheckpoint));
    }

    /// Yanlış boyutta aggregate_signature ile checkpoint kaydedilemez.
    #[test]
    fn test_checkpoint_wrong_sig_length_rejected() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        let mut cp = make_checkpoint(h);
        cp.qc.aggregate_signature = vec![0u8; 32]; // yanlış boyut (96 bekleniyordu)
        let err = manager.record_checkpoint(cp).unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidCheckpoint));
    }

    /// QC height uyuşmazlığı ile checkpoint kaydedilemez.
    #[test]
    fn test_checkpoint_qc_height_mismatch_rejected() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        let mut cp = make_checkpoint(h);
        cp.qc.height = h + 1; // height uyuşmazlığı
        let err = manager.record_checkpoint(cp).unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidCheckpoint));
    }

    /// QC block_hash uyuşmazlığı ile checkpoint kaydedilemez.
    #[test]
    fn test_checkpoint_qc_hash_mismatch_rejected() {
        let manager = CheckpointManager::new();
        let h = CHECKPOINT_INTERVAL as u64;
        let mut cp = make_checkpoint(h);
        cp.qc.block_hash = BlockHash::from_bytes([0xABu8; 32]); // farklı hash
        let err = manager.record_checkpoint(cp).unwrap_err();
        assert!(matches!(err, ConsensusError::InvalidCheckpoint));
    }
}
