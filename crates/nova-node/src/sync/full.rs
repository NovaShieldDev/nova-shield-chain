//! Tam senkronizasyon — tüm blokları indir

use super::{SyncEngine, SyncMode, SyncState};
use crate::errors::{NodeError, NodeResult};

/// Tam senkronizasyon motoru
pub struct FullSync {
    state: SyncState,
    best_block: u64,
    blocks_processed: u64,
    /// NOVA-RED-NODE-007: Son alınan blok hash'i — zincir bütünlüğü takibi
    best_hash: [u8; 32],
}

impl FullSync {
    pub fn new() -> Self {
        Self {
            state: SyncState::Idle,
            best_block: 0,
            blocks_processed: 0,
            best_hash: [0u8; 32],
        }
    }

    pub fn blocks_processed(&self) -> u64 { self.blocks_processed }

    /// Son alınan geçerli blok hash'i
    pub fn best_hash(&self) -> [u8; 32] { self.best_hash }
}

impl SyncEngine for FullSync {
    fn mode(&self) -> SyncMode { SyncMode::Full }
    fn state(&self) -> &SyncState { &self.state }

    fn start_sync(&mut self, target_block: u64) -> NodeResult<()> {
        if target_block <= self.best_block {
            self.state = SyncState::Synced { best_block: self.best_block };
            return Ok(());
        }
        self.state = SyncState::Syncing {
            from_block: self.best_block,
            to_block: target_block,
            current_block: self.best_block,
        };
        Ok(())
    }

    fn process_block(&mut self, block_number: u64, block_hash: [u8; 32]) -> NodeResult<()> {
        // NOVA-RED-NODE-007: Sıfır hash (sahte/boş blok) reddedilir.
        // Saldırgan geçersiz hash göndererek zinciri manipüle edemez.
        if block_hash == [0u8; 32] {
            return Err(NodeError::SyncError {
                reason: format!(
                    "geçersiz blok hash'i: blok {} için sıfır hash reddedildi",
                    block_number
                ),
            });
        }

        match &mut self.state {
            SyncState::Syncing { to_block, current_block, .. } => {
                if block_number != *current_block + 1 && *current_block != 0 {
                    return Err(NodeError::SyncError {
                        reason: format!(
                            "beklenen blok {}, alınan {}",
                            *current_block + 1,
                            block_number
                        ),
                    });
                }
                *current_block = block_number;
                self.best_block = block_number;
                self.best_hash = block_hash;
                self.blocks_processed += 1;

                if block_number >= *to_block {
                    let best = *to_block;
                    self.state = SyncState::Synced { best_block: best };
                }
            }
            SyncState::Idle | SyncState::Synced { .. } => {
                // Yeni blok alındı, best güncelle
                if block_number == self.best_block + 1 {
                    self.best_block = block_number;
                    self.best_hash = block_hash;
                    self.blocks_processed += 1;
                    self.state = SyncState::Synced { best_block: block_number };
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn best_block(&self) -> u64 { self.best_block }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_sync_start() {
        let mut sync = FullSync::new();
        sync.start_sync(100).unwrap();
        assert!(matches!(sync.state(), SyncState::Syncing { to_block: 100, .. }));
    }

    #[test]
    fn test_full_sync_already_synced() {
        let mut sync = FullSync::new();
        sync.start_sync(0).unwrap();
        assert!(sync.state().is_synced());
    }

    #[test]
    fn test_full_sync_process_blocks() {
        let mut sync = FullSync::new();
        sync.start_sync(3).unwrap();

        for i in 1u64..=3 {
            sync.process_block(i, [i as u8; 32]).unwrap();
        }
        assert!(sync.state().is_synced());
        assert_eq!(sync.best_block(), 3);
        assert_eq!(sync.blocks_processed(), 3);
    }

    #[test]
    fn test_full_sync_progress() {
        let mut sync = FullSync::new();
        sync.start_sync(10).unwrap();

        sync.process_block(1, [1u8; 32]).unwrap();
        let pct = sync.state().progress_pct();
        assert!(pct > 0.0 && pct < 100.0);
    }

    /// NOVA-RED-NODE-007: Sıfır hash reddedilmeli — sahte blok koruması
    #[test]
    fn test_full_sync_zero_hash_rejected() {
        let mut sync = FullSync::new();
        sync.start_sync(5).unwrap();

        // Sıfır hash → hata
        let result = sync.process_block(1, [0u8; 32]);
        assert!(result.is_err(), "sıfır hash reddedilmeli");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("sıfır hash"), "hata mesajı sıfır hash içermeli");
    }

    /// NOVA-RED-NODE-007: Geçerli hash kaydedilmeli
    #[test]
    fn test_full_sync_stores_block_hash() {
        let mut sync = FullSync::new();
        sync.start_sync(3).unwrap();

        let hash1 = [0xAAu8; 32];
        sync.process_block(1, hash1).unwrap();
        assert_eq!(sync.best_hash(), hash1);

        let hash2 = [0xBBu8; 32];
        sync.process_block(2, hash2).unwrap();
        assert_eq!(sync.best_hash(), hash2);
    }
}
