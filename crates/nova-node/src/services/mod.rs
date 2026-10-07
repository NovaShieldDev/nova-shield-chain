//! Node servisleri — Polkadot SDK ServiceBuilder pattern

pub mod consensus;
pub mod engine;
pub mod guardian;
pub mod mempool;
pub mod metrics;
pub mod network;
pub mod pipeline;

pub use consensus::ConsensusService;
pub use engine::{ConsensusEngine, EngineStateHandle, FullNodeEngine, NetworkEngine};
pub use guardian::GuardianService;
pub use mempool::MempoolService;
pub use metrics::MetricsService;
pub use network::NetworkService;
pub use pipeline::TxPipeline;

use crate::errors::NodeResult;

// ─── ServiceStatus ────────────────────────────────────────────────────────────

/// Servis durumu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed { reason: String },
}

impl ServiceStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ServiceStatus::Running)
    }
}

// ─── NodeService trait ────────────────────────────────────────────────────────

/// Tüm servislerin uyguladığı temel trait
pub trait NodeService: Send + Sync {
    fn name(&self) -> &'static str;
    fn start(&mut self) -> NodeResult<()>;
    fn stop(&mut self) -> NodeResult<()>;
    fn status(&self) -> &ServiceStatus;
    fn health_check(&self) -> bool;
}
