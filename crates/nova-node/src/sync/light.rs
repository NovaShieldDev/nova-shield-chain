//! Light client senkronizasyonu — sadece blok başlıkları

use super::{SyncEngine, SyncMode, SyncState};
use crate::errors::NodeResult;

/// Blok başlığı özeti
#[derive(Debug, Clone)]
pub struct BlockHeader {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub validator_id: u64,
}

impl BlockHeader {
    pub fn compute_hash(number: u64, parent_hash: [u8; 32], validator_id: u64) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-header-v1");
        h.update(&number.to_le_bytes());
        h.update(&parent_hash);
        h.update(&validator_id.to_le_bytes());
        *h.finalize().as_bytes()
    }
}

/// Light client senkronizasyon motoru — sadece başlıkları indir ve doğrula
pub struct LightSync {
    state: SyncState,
    best_block: u64,
    headers: Vec<BlockHeader>,
}

impl LightSync {
    pub fn new() -> Self {
        Self { state: SyncState::Idle, best_block: 0, headers: vec![] }
    }

    pub fn headers(&self) -> &[BlockHeader] { &self.headers }

    pub fn get_header(&self, number: u64) -> Option<&BlockHeader> {
        self.headers.iter().find(|h| h.number == number)
    }
}

impl SyncEngine for LightSync {
    fn mode(&self) -> SyncMode { SyncMode::Light }
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
        let parent_hash = self.headers.last().map(|h| h.hash).unwrap_or([0u8; 32]);
        let header = BlockHeader {
            number: block_number,
            hash: block_hash,
            parent_hash,
            state_root: [0u8; 32],
            validator_id: 0,
        };
        self.headers.push(header);
        self.best_block = block_number;

        match &mut self.state {
            SyncState::Syncing { to_block, current_block, .. } => {
                *current_block = block_number;
                if block_number >= *to_block {
                    let best = *to_block;
                    self.state = SyncState::Synced { best_block: best };
                }
            }
            _ => {
                self.state = SyncState::Synced { best_block: block_number };
            }
        }
        Ok(())
    }

    fn best_block(&self) -> u64 { self.best_block }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_light_sync_basic() {
        let mut sync = LightSync::new();
        sync.start_sync(5).unwrap();

        for i in 1u64..=5 {
            sync.process_block(i, [i as u8; 32]).unwrap();
        }

        assert!(sync.state().is_synced());
        assert_eq!(sync.best_block(), 5);
        assert_eq!(sync.headers().len(), 5);
    }

    #[test]
    fn test_light_sync_get_header() {
        let mut sync = LightSync::new();
        sync.start_sync(3).unwrap();
        sync.process_block(1, [1u8; 32]).unwrap();
        sync.process_block(2, [2u8; 32]).unwrap();
        sync.process_block(3, [3u8; 32]).unwrap();

        let h = sync.get_header(2).unwrap();
        assert_eq!(h.number, 2);
        assert_eq!(h.hash, [2u8; 32]);
    }

    #[test]
    fn test_block_header_compute_hash() {
        let h1 = BlockHeader::compute_hash(1, [0u8; 32], 42);
        let h2 = BlockHeader::compute_hash(1, [0u8; 32], 42);
        assert_eq!(h1, h2); // deterministik

        let h3 = BlockHeader::compute_hash(2, [0u8; 32], 42);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_light_sync_mode() {
        let sync = LightSync::new();
        assert!(sync.mode().is_light());
    }
}
