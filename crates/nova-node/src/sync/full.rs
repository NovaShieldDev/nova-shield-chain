//! Tam senkronizasyon — tüm blokları indir

use super::{SyncEngine, SyncMode, SyncState};
use crate::errors::{NodeError, NodeResult};

/// Tam senkronizasyon motoru
pub struct FullSync {
    state: SyncState,
    best_block: u64,
    blocks_processed: u64,
}

impl FullSync {
    pub fn new() -> Self {
        Self { state: SyncState::Idle, best_block: 0, blocks_processed: 0 }
    }

    pub fn blocks_processed(&self) -> u64 { self.blocks_processed }
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
        let _ = block_hash;
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
}
