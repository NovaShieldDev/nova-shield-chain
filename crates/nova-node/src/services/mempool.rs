//! Mempool servisi — şifreli mempool runner

use super::{NodeService, ServiceStatus};
use crate::errors::{NodeError, NodeResult};
use std::collections::VecDeque;

// ─── PendingTx ────────────────────────────────────────────────────────────────

/// Bekleyen işlem (şifrelenmiş)
#[derive(Debug, Clone)]
pub struct PendingTx {
    pub tx_hash: [u8; 32],
    pub sender: [u8; 20],
    pub gas_price: u64,
    pub encrypted_payload: Vec<u8>,
    pub received_at_slot: u64,
}

impl PendingTx {
    pub fn new(sender: [u8; 20], gas_price: u64, payload: Vec<u8>, slot: u64) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-tx-v1");
        h.update(&sender);
        h.update(&gas_price.to_le_bytes());
        h.update(&payload);
        h.update(&slot.to_le_bytes());
        let tx_hash = *h.finalize().as_bytes();

        Self {
            tx_hash,
            sender,
            gas_price,
            encrypted_payload: payload,
            received_at_slot: slot,
        }
    }
}

// ─── MempoolService ───────────────────────────────────────────────────────────

/// Mempool servisi — işlemleri toplar ve batch'lere hazırlar
pub struct MempoolService {
    status: ServiceStatus,
    pending: VecDeque<PendingTx>,
    max_size: usize,
    current_slot: u64,
    total_received: u64,
    total_dropped: u64,
}

impl MempoolService {
    pub fn new(max_size: usize) -> Self {
        Self {
            status: ServiceStatus::Stopped,
            pending: VecDeque::new(),
            max_size,
            current_slot: 0,
            total_received: 0,
            total_dropped: 0,
        }
    }

    /// İşlem ekle
    pub fn add_transaction(
        &mut self,
        sender: [u8; 20],
        gas_price: u64,
        payload: Vec<u8>,
    ) -> NodeResult<[u8; 32]> {
        if !self.status.is_running() {
            return Err(NodeError::ServiceStartError {
                service: "MempoolService".into(),
                reason: "servis çalışmıyor".into(),
            });
        }
        if self.pending.len() >= self.max_size {
            self.total_dropped += 1;
            return Err(NodeError::ServiceStartError {
                service: "MempoolService".into(),
                reason: "mempool dolu".into(),
            });
        }

        let tx = PendingTx::new(sender, gas_price, payload, self.current_slot);
        let hash = tx.tx_hash;
        self.pending.push_back(tx);
        self.total_received += 1;
        Ok(hash)
    }

    /// Batch için n işlem al (gas price sırasına göre)
    pub fn take_for_batch(&mut self, n: usize) -> Vec<PendingTx> {
        let take_n = n.min(self.pending.len());
        // Gas price'a göre sırala (yüksek öncelik)
        let mut all: Vec<PendingTx> = self.pending.drain(..).collect();
        all.sort_by(|a, b| b.gas_price.cmp(&a.gas_price));
        let batch = all.drain(..take_n).collect();
        // Kalanları geri koy
        for tx in all {
            self.pending.push_back(tx);
        }
        batch
    }

    /// Slot güncelle
    pub fn advance_slot(&mut self, slot: u64) {
        self.current_slot = slot;
    }

    pub fn pending_count(&self) -> usize { self.pending.len() }
    pub fn total_received(&self) -> u64 { self.total_received }
    pub fn total_dropped(&self) -> u64 { self.total_dropped }
}

impl NodeService for MempoolService {
    fn name(&self) -> &'static str { "MempoolService" }

    fn start(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Starting;
        self.status = ServiceStatus::Running;
        Ok(())
    }

    fn stop(&mut self) -> NodeResult<()> {
        self.status = ServiceStatus::Stopping;
        self.pending.clear();
        self.status = ServiceStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> &ServiceStatus { &self.status }

    fn health_check(&self) -> bool {
        self.status.is_running() && self.pending.len() < self.max_size
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sender(n: u8) -> [u8; 20] { [n; 20] }

    #[test]
    fn test_mempool_service_lifecycle() {
        let mut svc = MempoolService::new(1000);
        svc.start().unwrap();
        assert!(svc.health_check());
        svc.stop().unwrap();
        assert!(!svc.health_check());
    }

    #[test]
    fn test_mempool_add_transaction() {
        let mut svc = MempoolService::new(100);
        svc.start().unwrap();

        let hash = svc.add_transaction(sender(1), 1000, vec![0xAAu8; 32], ).unwrap();
        assert_ne!(hash, [0u8; 32]);
        assert_eq!(svc.pending_count(), 1);
        assert_eq!(svc.total_received(), 1);
    }

    #[test]
    fn test_mempool_max_size() {
        let mut svc = MempoolService::new(2);
        svc.start().unwrap();

        svc.add_transaction(sender(1), 100, vec![1], ).unwrap();
        svc.add_transaction(sender(2), 200, vec![2], ).unwrap();
        let result = svc.add_transaction(sender(3), 300, vec![3]);
        assert!(result.is_err());
        assert_eq!(svc.total_dropped(), 1);
    }

    #[test]
    fn test_mempool_take_for_batch_priority() {
        let mut svc = MempoolService::new(100);
        svc.start().unwrap();

        svc.add_transaction(sender(1), 100, vec![1]).unwrap();
        svc.add_transaction(sender(2), 500, vec![2]).unwrap();
        svc.add_transaction(sender(3), 300, vec![3]).unwrap();

        let batch = svc.take_for_batch(2);
        assert_eq!(batch.len(), 2);
        // En yüksek gas price önce
        assert_eq!(batch[0].gas_price, 500);
        assert_eq!(batch[1].gas_price, 300);
        // 1 tane kaldı
        assert_eq!(svc.pending_count(), 1);
    }

    #[test]
    fn test_mempool_tx_hash_unique() {
        let mut svc = MempoolService::new(100);
        svc.start().unwrap();

        let h1 = svc.add_transaction(sender(1), 100, vec![1]).unwrap();
        let h2 = svc.add_transaction(sender(2), 200, vec![2]).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_mempool_add_not_running() {
        let mut svc = MempoolService::new(100);
        let result = svc.add_transaction(sender(1), 100, vec![]);
        assert!(result.is_err());
    }
}
