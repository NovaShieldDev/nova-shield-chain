//! Senkronizasyon modülleri

pub mod full;
pub mod light;
pub mod warp;

pub use full::FullSync;
pub use light::LightSync;
pub use warp::WarpSync;

use crate::errors::NodeResult;

// ─── SyncMode ─────────────────────────────────────────────────────────────────

/// Senkronizasyon modu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncMode {
    /// Tüm blokları indir (tam node)
    Full,
    /// Sadece blok başlıklarını indir (light client)
    Light,
    /// Warp sync: checkpoint'ten başla (Polkadot GRANDPA warp sync)
    Warp,
}

impl SyncMode {
    pub fn is_full(&self) -> bool { matches!(self, SyncMode::Full) }
    pub fn is_light(&self) -> bool { matches!(self, SyncMode::Light) }
    pub fn is_warp(&self) -> bool { matches!(self, SyncMode::Warp) }
}

// ─── SyncState ────────────────────────────────────────────────────────────────

/// Senkronizasyon durumu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Syncing { from_block: u64, to_block: u64, current_block: u64 },
    Synced { best_block: u64 },
    Failed { reason: String },
}

impl SyncState {
    pub fn is_synced(&self) -> bool { matches!(self, SyncState::Synced { .. }) }

    pub fn progress_pct(&self) -> f64 {
        match self {
            SyncState::Syncing { from_block, to_block, current_block } => {
                let total = to_block.saturating_sub(*from_block) as f64;
                if total == 0.0 { return 100.0; }
                let done = current_block.saturating_sub(*from_block) as f64;
                (done / total * 100.0).min(100.0)
            }
            SyncState::Synced { .. } => 100.0,
            _ => 0.0,
        }
    }
}

// ─── SyncEngine trait ─────────────────────────────────────────────────────────

/// Senkronizasyon motoru trait
pub trait SyncEngine {
    fn mode(&self) -> SyncMode;
    fn state(&self) -> &SyncState;
    fn start_sync(&mut self, target_block: u64) -> NodeResult<()>;
    fn process_block(&mut self, block_number: u64, block_hash: [u8; 32]) -> NodeResult<()>;
    fn best_block(&self) -> u64;
}
