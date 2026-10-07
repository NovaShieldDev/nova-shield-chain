//! Konsensüs servisi — BFT engine runner

use super::{NodeService, ServiceStatus};
use crate::errors::{NodeError, NodeResult};

/// Konsensüs servisi — BFT blok üretimi ve onaylaması
pub struct ConsensusService {
    validator_id: u64,
    status: ServiceStatus,
    /// Üretilen blok sayısı
    blocks_produced: u64,
    /// Onaylanan blok sayısı
    blocks_finalized: u64,
    /// Slot süresi (ms)
    slot_duration_ms: u64,
    /// Şu anki slot
    current_slot: u64,
}

impl ConsensusService {
    pub fn new(validator_id: u64, slot_duration_ms: u64) -> Self {
        Self {
            validator_id,
            status: ServiceStatus::Stopped,
            blocks_produced: 0,
            blocks_finalized: 0,
            slot_duration_ms,
            current_slot: 0,
        }
    }

    /// Yeni blok üret (mock — gerçek ağ yok)
    pub fn produce_block(&mut self) -> NodeResult<[u8; 32]> {
        if !self.status.is_running() {
            return Err(NodeError::ServiceStartError {
                service: "ConsensusService".into(),
                reason: "servis çalışmıyor".into(),
            });
        }
        self.current_slot += 1;
        self.blocks_produced += 1;

        // BLAKE3 ile mock blok hash'i üret
        let mut h = blake3::Hasher::new();
        h.update(b"nova-block-v1");
        h.update(&self.validator_id.to_le_bytes());
        h.update(&self.current_slot.to_le_bytes());
        h.update(&self.blocks_produced.to_le_bytes());
        Ok(*h.finalize().as_bytes())
    }

    /// Blok finalize et
    pub fn finalize_block(&mut self, block_hash: [u8; 32]) -> NodeResult<()> {
        if !self.status.is_running() {
            return Err(NodeError::ServiceStartError {
                service: "ConsensusService".into(),
                reason: "servis çalışmıyor".into(),
            });
        }
        let _ = block_hash;
        self.blocks_finalized += 1;
        Ok(())
    }

    pub fn blocks_produced(&self) -> u64 { self.blocks_produced }
    pub fn blocks_finalized(&self) -> u64 { self.blocks_finalized }
    pub fn current_slot(&self) -> u64 { self.current_slot }
    pub fn slot_duration_ms(&self) -> u64 { self.slot_duration_ms }
}

impl NodeService for ConsensusService {
    fn name(&self) -> &'static str { "ConsensusService" }

    fn start(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Starting;
        // Gerçek implementasyonda: BFT engine'i başlat, slot timer kur
        self.status = ServiceStatus::Running;
        Ok(())
    }

    fn stop(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Stopping;
        self.status = ServiceStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> &ServiceStatus { &self.status }

    fn health_check(&self) -> bool {
        self.status.is_running()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_service_lifecycle() {
        let mut svc = ConsensusService::new(1, 400);
        assert!(!svc.health_check());

        svc.start().unwrap();
        assert!(svc.health_check());
        assert_eq!(svc.status(), &ServiceStatus::Running);

        svc.stop().unwrap();
        assert_eq!(svc.status(), &ServiceStatus::Stopped);
    }

    #[test]
    fn test_consensus_produce_block() {
        let mut svc = ConsensusService::new(1, 400);
        svc.start().unwrap();

        let hash = svc.produce_block().unwrap();
        assert_ne!(hash, [0u8; 32]);
        assert_eq!(svc.blocks_produced(), 1);
        assert_eq!(svc.current_slot(), 1);
    }

    #[test]
    fn test_consensus_produce_multiple_blocks() {
        let mut svc = ConsensusService::new(2, 400);
        svc.start().unwrap();

        let h1 = svc.produce_block().unwrap();
        let h2 = svc.produce_block().unwrap();
        assert_ne!(h1, h2);
        assert_eq!(svc.blocks_produced(), 2);
    }

    #[test]
    fn test_consensus_finalize_block() {
        let mut svc = ConsensusService::new(1, 400);
        svc.start().unwrap();

        let hash = svc.produce_block().unwrap();
        svc.finalize_block(hash).unwrap();
        assert_eq!(svc.blocks_finalized(), 1);
    }

    #[test]
    fn test_consensus_produce_block_not_running() {
        let mut svc = ConsensusService::new(1, 400);
        let result = svc.produce_block();
        assert!(result.is_err());
    }
}
