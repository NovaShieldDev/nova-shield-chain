//! Shred yayılımı — Solana Turbine RetransmitStage pattern
//!
//! Solana `retransmit_stage.rs`:
//! - Shred'leri alır → ClusterNodes üzerinde stake ağırlıklı dağıtır
//! - Deduper ile tekrar iletimi önler
//! - Rayon ile paralel retransmit (mock'ta sequential)
//! - LruCache<Slot, RetransmitSlotStats>

use crate::turbine::cluster::{ClusterNodes, DATA_PLANE_FANOUT, MAX_NUM_TURBINE_HOPS};
use crate::turbine::shred::{Shred, ShredId};
use std::collections::{HashMap, HashSet};

// ─── Deduper ──────────────────────────────────────────────────────────────────

/// Tekrar iletim önleyici — Solana `Deduper`
/// Bloom filtresi tabanlı: `packet_filter.deduplicate(&packet)`
#[derive(Debug, Default)]
pub struct Deduper {
    seen: HashSet<[u8; 32]>,
    max_size: usize,
}

impl Deduper {
    pub fn new(max_size: usize) -> Self {
        Self { seen: HashSet::new(), max_size }
    }

    /// Bu shred daha önce görüldü mü?
    pub fn is_duplicate(&mut self, shred: &Shred) -> bool {
        let hash = shred.hash();
        if self.seen.contains(&hash) {
            true
        } else {
            if self.seen.len() >= self.max_size {
                // Basit eviction: %10 temizle
                let to_remove: Vec<_> = self.seen.iter().take(self.max_size / 10).cloned().collect();
                for h in to_remove {
                    self.seen.remove(&h);
                }
            }
            self.seen.insert(hash);
            false
        }
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }
}

// ─── RetransmitSlotStats ──────────────────────────────────────────────────────

/// Slot başına retransmit istatistikleri
/// Solana: `LruCache<Slot, RetransmitSlotStats>`
#[derive(Debug, Default, Clone)]
pub struct RetransmitSlotStats {
    pub slot: u64,
    pub total_shreds: u32,
    pub retransmit_count: u32,
    pub duplicate_count: u32,
    pub failed_count: u32,
}

// ─── RetransmitResult ─────────────────────────────────────────────────────────

/// Tek bir retransmit işleminin sonucu
#[derive(Debug, Clone)]
pub struct RetransmitResult {
    pub shred_id: ShredId,
    /// Hangi node'lara gönderildi
    pub sent_to: Vec<[u8; 32]>,
    pub hop: u8,
}

// ─── ShredPropagator ──────────────────────────────────────────────────────────

/// Shred yayıcı — Turbine retransmit stage
/// Solana: `RetransmitStage { cluster_nodes_cache, deduper, stats }`
pub struct ShredPropagator {
    local_id: [u8; 32],
    cluster: ClusterNodes,
    deduper: Deduper,
    /// Slot istatistikleri (son 100 slot)
    slot_stats: HashMap<u64, RetransmitSlotStats>,
    max_hops: u8,
    fanout: usize,
}

impl ShredPropagator {
    pub fn new(
        local_id: [u8; 32],
        cluster: ClusterNodes,
        max_hops: u8,
        fanout: usize,
    ) -> Self {
        Self {
            local_id,
            cluster,
            deduper: Deduper::new(65_536),
            slot_stats: HashMap::new(),
            max_hops,
            fanout,
        }
    }

    /// Shred yayılım pipeline'ına al
    /// Solana: `retransmit_shred(shred, cluster_nodes, socket, stats)`
    pub fn retransmit(&mut self, mut shred: Shred) -> RetransmitResult {
        let shred_id = shred.id.clone();
        let slot = shred.id.slot;

        let stats = self.slot_stats.entry(slot).or_insert_with(|| RetransmitSlotStats {
            slot,
            ..Default::default()
        });
        stats.total_shreds += 1;

        // Tekilleştirme
        if self.deduper.is_duplicate(&shred) {
            stats.duplicate_count += 1;
            return RetransmitResult {
                shred_id,
                sent_to: vec![],
                hop: shred.hop,
            };
        }

        // Maksimum atlama kontrolü
        if shred.exceeded_max_hops(self.max_hops) {
            stats.failed_count += 1;
            return RetransmitResult {
                shred_id,
                sent_to: vec![],
                hop: shred.hop,
            };
        }

        // Retransmit hedefleri — Turbine tree'den stake ağırlıklı
        let targets: Vec<[u8; 32]> = {
            let my_id = self.local_id;
            let peers = self.cluster.retransmit_peers(&my_id, slot);
            peers.iter().take(self.fanout).map(|n| n.node_id).collect()
        };

        // Atlama artır
        shred.increment_hop();
        stats.retransmit_count += targets.len() as u32;

        RetransmitResult {
            shred_id,
            sent_to: targets,
            hop: shred.hop,
        }
    }

    /// Slot istatistikleri al
    pub fn slot_stats(&self, slot: u64) -> Option<&RetransmitSlotStats> {
        self.slot_stats.get(&slot)
    }

    /// Toplam işlenen shred sayısı
    pub fn total_processed(&self) -> u32 {
        self.slot_stats.values().map(|s| s.total_shreds).sum()
    }

    /// Cluster boyutu
    pub fn cluster_size(&self) -> usize {
        self.cluster.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turbine::cluster::WeightedNode;
    use crate::turbine::shred::{Shred, ShredType};

    fn leader() -> [u8; 32] { [0u8; 32] }

    fn make_cluster() -> ClusterNodes {
        let nodes: Vec<WeightedNode> = (1u8..=10)
            .map(|i| WeightedNode::new([i; 32], format!("127.0.0.{i}:30333"), i as u64 * 100_000))
            .collect();
        ClusterNodes::new(nodes)
    }

    fn make_shred(slot: u64, index: u32) -> Shred {
        Shred::new(slot, index, ShredType::Data, leader(), vec![0u8; 100])
    }

    fn make_propagator(my_id: u8) -> ShredPropagator {
        ShredPropagator::new(
            [my_id; 32],
            make_cluster(),
            MAX_NUM_TURBINE_HOPS as u8,
            DATA_PLANE_FANOUT,
        )
    }

    #[test]
    fn test_retransmit_basic() {
        let mut prop = make_propagator(0);
        let shred = make_shred(100, 0);
        let result = prop.retransmit(shred);
        // Yeni shred → işlendi
        assert_eq!(result.shred_id.slot, 100);
        assert_eq!(result.shred_id.index, 0);
    }

    #[test]
    fn test_retransmit_deduplication() {
        let mut prop = make_propagator(0);
        let shred = make_shred(100, 0);
        let r1 = prop.retransmit(shred.clone());
        let r2 = prop.retransmit(shred);
        // İkinci kez: duplicate → hedef yok
        assert!(r2.sent_to.is_empty());
        // Stats
        let stats = prop.slot_stats(100).unwrap();
        assert_eq!(stats.duplicate_count, 1);
    }

    #[test]
    fn test_retransmit_max_hops() {
        let mut prop = ShredPropagator::new([0u8; 32], make_cluster(), 2, DATA_PLANE_FANOUT);
        let mut shred = make_shred(200, 0);
        shred.hop = 2; // Zaten sınırda
        let result = prop.retransmit(shred);
        assert!(result.sent_to.is_empty());
    }

    #[test]
    fn test_slot_stats() {
        let mut prop = make_propagator(0);
        for i in 0u32..5 {
            prop.retransmit(make_shred(300, i));
        }
        let stats = prop.slot_stats(300).unwrap();
        assert_eq!(stats.total_shreds, 5);
        assert_eq!(stats.slot, 300);
    }

    #[test]
    fn test_total_processed() {
        let mut prop = make_propagator(0);
        prop.retransmit(make_shred(1, 0));
        prop.retransmit(make_shred(2, 0));
        prop.retransmit(make_shred(3, 0));
        assert_eq!(prop.total_processed(), 3);
    }

    #[test]
    fn test_deduper() {
        let mut d = Deduper::new(100);
        let shred = make_shred(1, 0);
        assert!(!d.is_duplicate(&shred));
        assert!(d.is_duplicate(&shred));
    }

    #[test]
    fn test_cluster_size() {
        let prop = make_propagator(0);
        assert_eq!(prop.cluster_size(), 10);
    }
}
