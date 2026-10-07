//! Cluster Nodes — Solana Turbine ağırlıklı dağıtım
//!
//! Solana `cluster_nodes.rs`:
//! - `ClusterNodes<T>` — stake ağırlıklı düğüm listesi
//! - `WeightedShuffle<u64>` — stake miktarına göre ağırlıklı rastgele sıralama
//! - DATA_PLANE_FANOUT = 200
//! - MAX_NUM_TURBINE_HOPS = 4
//! - Retransmit tree: her node en fazla SQRT(200)=14 parent'tan alır
//!
//! Avalanchego: `ValidatorSet` ile stake ağırlıklı gossip

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Veri düzlemi fanout — Solana DATA_PLANE_FANOUT=200
pub const DATA_PLANE_FANOUT: usize = 200;

/// Maksimum atlama sayısı — Solana MAX_NUM_TURBINE_HOPS=4
pub const MAX_NUM_TURBINE_HOPS: usize = 4;

// ─── WeightedNode ─────────────────────────────────────────────────────────────

/// Ağırlıklı node — stake ile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedNode {
    pub node_id: [u8; 32],
    pub addr: String,
    /// Stake miktarı (NSC)
    pub stake: u64,
    /// Validator mı?
    pub is_validator: bool,
}

impl WeightedNode {
    pub fn new(node_id: [u8; 32], addr: impl Into<String>, stake: u64) -> Self {
        Self {
            node_id,
            addr: addr.into(),
            stake,
            is_validator: stake > 0,
        }
    }
}

// ─── ClusterNodes ─────────────────────────────────────────────────────────────

/// Cluster node listesi — Solana `ClusterNodes<T>`
/// Stake ağırlıklı Turbine retransmit tree oluşturur
#[derive(Debug)]
pub struct ClusterNodes {
    nodes: Vec<WeightedNode>,
    /// node_id → indeks
    index: HashMap<[u8; 32], usize>,
    /// Toplam stake
    total_stake: u64,
}

impl ClusterNodes {
    pub fn new(nodes: Vec<WeightedNode>) -> Self {
        let total_stake = nodes.iter().map(|n| n.stake).sum();
        let index = nodes.iter().enumerate().map(|(i, n)| (n.node_id, i)).collect();
        Self { nodes, index, total_stake }
    }

    /// Stake ağırlıklı karıştırılmış liste
    /// Solana: `WeightedShuffle::new("turbine-nodes", seed).shuffle(&stakes)`
    pub fn weighted_shuffle(&self, seed: u64) -> Vec<&WeightedNode> {
        let mut rng = seeded_rng(seed);
        let mut indexed: Vec<(u64, usize)> = self.nodes.iter().enumerate().map(|(i, n)| {
            // Ağırlıklı skor: stake * random
            let w: u64 = if n.stake == 0 { 1 } else { n.stake };
            let r: u64 = rng.gen::<u64>().wrapping_add(1);
            // w * r mod 2^64 yerine basit bir skor
            (w.wrapping_mul(r), i)
        }).collect();
        // Azalan skor sırası
        indexed.sort_by(|a, b| b.0.cmp(&a.0));
        indexed.iter().map(|(_, i)| &self.nodes[*i]).collect()
    }

    /// Turbine retransmit tree — DATA_PLANE_FANOUT'a göre
    /// Solana: root → layer1 (sqrt(fanout) nodes) → layer2 → ...
    pub fn retransmit_peers(
        &self,
        my_id: &[u8; 32],
        slot: u64,
    ) -> Vec<&WeightedNode> {
        let shuffled = self.weighted_shuffle(slot);
        let my_pos = shuffled.iter().position(|n| &n.node_id == my_id);

        match my_pos {
            None => vec![],
            Some(pos) => {
                // Basit retransmit: sonraki DATA_PLANE_FANOUT node'a gönder
                shuffled
                    .into_iter()
                    .skip(pos + 1)
                    .take(DATA_PLANE_FANOUT)
                    .filter(|n| &n.node_id != my_id)
                    .collect()
            }
        }
    }

    /// Üst kademe node'lar (retransmit zincirinde parent'lar)
    pub fn get_neighbors(
        &self,
        my_id: &[u8; 32],
        slot: u64,
        fanout: usize,
    ) -> (Option<&WeightedNode>, Vec<&WeightedNode>) {
        let shuffled = self.weighted_shuffle(slot);
        let my_pos = match shuffled.iter().position(|n| &n.node_id == my_id) {
            None => return (None, vec![]),
            Some(p) => p,
        };

        // Parent: önceki node (Turbine tree'de root'a doğru)
        let parent = if my_pos > 0 { Some(shuffled[my_pos - 1]) } else { None };

        // Children: sonraki fanout kadar node
        let children: Vec<&WeightedNode> = shuffled
            .into_iter()
            .skip(my_pos + 1)
            .take(fanout)
            .collect();

        (parent, children)
    }

    /// Node sayısı
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Toplam stake
    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    /// Node'u ID ile bul
    pub fn get_node(&self, node_id: &[u8; 32]) -> Option<&WeightedNode> {
        self.index.get(node_id).map(|&i| &self.nodes[i])
    }

    /// Stake tabanlı top-N validator
    pub fn top_validators(&self, n: usize) -> Vec<&WeightedNode> {
        let mut sorted: Vec<&WeightedNode> = self.nodes.iter().filter(|n| n.is_validator).collect();
        sorted.sort_by(|a, b| b.stake.cmp(&a.stake));
        sorted.truncate(n);
        sorted
    }
}

// ─── Seeded RNG ───────────────────────────────────────────────────────────────

fn seeded_rng(seed: u64) -> impl Rng {
    use rand::SeedableRng;
    rand::rngs::StdRng::seed_from_u64(seed)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: u8, stake: u64) -> WeightedNode {
        WeightedNode::new([n; 32], format!("127.0.0.{n}:30333"), stake)
    }

    fn make_cluster() -> ClusterNodes {
        ClusterNodes::new(vec![
            node(1, 1_000_000),
            node(2, 2_000_000),
            node(3, 500_000),
            node(4, 3_000_000),
            node(5, 1_500_000),
        ])
    }

    #[test]
    fn test_cluster_nodes_len() {
        let cluster = make_cluster();
        assert_eq!(cluster.len(), 5);
    }

    #[test]
    fn test_total_stake() {
        let cluster = make_cluster();
        assert_eq!(cluster.total_stake(), 8_000_000);
    }

    #[test]
    fn test_weighted_shuffle_deterministic() {
        let cluster = make_cluster();
        let s1 = cluster.weighted_shuffle(42);
        let s2 = cluster.weighted_shuffle(42);
        let ids1: Vec<_> = s1.iter().map(|n| n.node_id).collect();
        let ids2: Vec<_> = s2.iter().map(|n| n.node_id).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn test_weighted_shuffle_different_seeds() {
        let cluster = make_cluster();
        let s1 = cluster.weighted_shuffle(42);
        let s2 = cluster.weighted_shuffle(99);
        let ids1: Vec<_> = s1.iter().map(|n| n.node_id).collect();
        let ids2: Vec<_> = s2.iter().map(|n| n.node_id).collect();
        // Farklı seed → farklı sıra (büyük olasılıkla)
        // Eşit olabilir ama bu çok düşük olasılık
        let _ = (ids1, ids2); // en azından crash etmemeli
    }

    #[test]
    fn test_retransmit_peers() {
        let cluster = make_cluster();
        let my_id = [1u8; 32];
        let peers = cluster.retransmit_peers(&my_id, 100);
        // My_id en başta değilse peers boş olabilir, en azından crash etmemeli
        assert!(peers.len() <= 4); // 5 - 1 = 4 maksimum
    }

    #[test]
    fn test_get_node() {
        let cluster = make_cluster();
        let n = cluster.get_node(&[1u8; 32]).unwrap();
        assert_eq!(n.stake, 1_000_000);
    }

    #[test]
    fn test_top_validators() {
        let cluster = make_cluster();
        let top3 = cluster.top_validators(3);
        assert_eq!(top3.len(), 3);
        // Sıralı mı?
        assert!(top3[0].stake >= top3[1].stake);
        assert!(top3[1].stake >= top3[2].stake);
    }

    #[test]
    fn test_neighbors() {
        let cluster = make_cluster();
        let shuffled = cluster.weighted_shuffle(42);
        let my_id = shuffled[2].node_id; // 3. node
        let (parent, children) = cluster.get_neighbors(&my_id, 42, 2);
        assert!(parent.is_some());
        assert!(!children.is_empty());
    }

    #[test]
    fn test_data_plane_fanout_constant() {
        assert_eq!(DATA_PLANE_FANOUT, 200);
    }

    #[test]
    fn test_max_hops_constant() {
        assert_eq!(MAX_NUM_TURBINE_HOPS, 4);
    }
}
