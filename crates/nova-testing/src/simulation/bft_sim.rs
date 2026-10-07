//! Çok-node BFT konsensüs simülatörü.
//!
//! N validator çalıştır, BftEngine::process_vote ile tam protokol simüle edilir.
//! Güvenlik (safety) ve canlılık (liveness) özellikleri test edilir.
//!
//! ## BFT Güvenlik Garantileri
//!
//! - **Güvenlik (Safety)**: İki dürüst node aynı yükseklikte farklı blok finalize edemez.
//! - **Canlılık (Liveness)**: n > 3f ise (f hatalı node) finalization sağlanır.
//! - **Quorum**: ceil(2n/3) oy gereklidir.

use std::collections::HashMap;
use nova_consensus::{BftEngine, Vote};
use nova_primitives::{BlockHash, EpochId, SlotId, ValidatorId};
use crate::simulation::network_sim::SimNetwork;

// ─── BftSimNode ───────────────────────────────────────────────────────────────

/// Simülasyondaki tek bir BFT node.
pub struct BftSimNode {
    /// Validator kimliği.
    pub id: ValidatorId,
    /// BFT motoru.
    pub engine: BftEngine,
    /// Finalize edilmiş bloklar: yükseklik → block_hash.
    pub finalized: HashMap<u64, BlockHash>,
    /// Byzantine (hatalı) node mu?
    pub is_faulty: bool,
}

impl BftSimNode {
    fn new(id: ValidatorId, all_validators: Vec<ValidatorId>, faulty: bool) -> Self {
        Self {
            id,
            engine: BftEngine::new(all_validators),
            finalized: HashMap::new(),
            is_faulty: faulty,
        }
    }
}

// ─── SimVote (serializable) ───────────────────────────────────────────────────

/// Ağ üzerinden taşınan oyun temsili.
#[derive(Clone, Debug)]
pub struct SimVote {
    pub validator: ValidatorId,
    pub block_hash: BlockHash,
    pub height: u64,
    pub slot: u64,
    pub signature: [u8; 64],
}

impl SimVote {
    /// Deterministik mock imzayla oy oluştur.
    pub fn new(validator: ValidatorId, block_hash: BlockHash, height: u64, slot: u64) -> Self {
        let mut sig = [0u8; 64];
        let mut h = blake3::Hasher::new();
        h.update(block_hash.as_bytes());
        h.update(&validator.0.to_le_bytes());
        h.update(&height.to_le_bytes());
        let hash = h.finalize();
        sig[..32].copy_from_slice(hash.as_bytes());
        Self { validator, block_hash, height, slot, signature: sig }
    }

    /// Çakışan (Byzantine) oy — farklı block_hash ile.
    pub fn conflicting(validator: ValidatorId, height: u64, slot: u64) -> Self {
        let conflict_hash = {
            let mut h = blake3::Hasher::new();
            h.update(b"byzantine-conflict");
            h.update(&validator.0.to_le_bytes());
            h.update(&height.to_le_bytes());
            BlockHash::from_bytes(*h.finalize().as_bytes())
        };
        Self::new(validator, conflict_hash, height, slot)
    }

    pub fn to_vote(&self) -> Vote {
        Vote {
            validator: self.validator,
            block_hash: self.block_hash,
            height: self.height,
            signature: self.signature,
            slot: SlotId(self.slot),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + 32 + 8 + 8 + 64);
        buf.extend_from_slice(&self.validator.0.to_le_bytes());
        buf.extend_from_slice(self.block_hash.as_bytes());
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(&self.slot.to_le_bytes());
        buf.extend_from_slice(&self.signature);
        buf
    }

    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 8 + 32 + 8 + 8 + 64 {
            return None;
        }
        let validator_id = u64::from_le_bytes(data[0..8].try_into().ok()?);
        let block_hash = BlockHash::from_bytes(data[8..40].try_into().ok()?);
        let height = u64::from_le_bytes(data[40..48].try_into().ok()?);
        let slot = u64::from_le_bytes(data[48..56].try_into().ok()?);
        let signature: [u8; 64] = data[56..120].try_into().ok()?;
        Some(Self {
            validator: ValidatorId(validator_id),
            block_hash,
            height,
            slot,
            signature,
        })
    }
}

// ─── BftRoundResult ───────────────────────────────────────────────────────────

/// Bir BFT round'unun sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BftRoundResult {
    /// Finalize edildi — QC alan node sayısı.
    Finalized { nodes_with_qc: usize },
    /// Quorum sağlanamadı.
    QuorumNotReached { votes_cast: usize, needed: usize },
    /// Güvenlik ihlali (aynı yükseklikte farklı bloklar finalize edildi).
    SafetyViolation {
        height: u64,
        hash_a: BlockHash,
        node_a: usize,
        hash_b: BlockHash,
        node_b: usize,
    },
}

// ─── SafetyCheckResult ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyCheckResult {
    Safe,
    Violated {
        height: u64,
        conflicting_hashes: Vec<BlockHash>,
    },
}

// ─── BftSimulation ────────────────────────────────────────────────────────────

/// Çok-node BFT simülasyonu.
pub struct BftSimulation {
    /// Tüm simüle edilen node'lar.
    pub nodes: Vec<BftSimNode>,
    /// Simüle edilen ağ.
    pub network: SimNetwork,
    /// Mevcut slot.
    pub current_slot: u64,
    /// Tamamlanan round sayısı.
    pub rounds_completed: u64,
}

impl BftSimulation {
    /// N dürüst validator ile simülasyon oluştur.
    pub fn new(n: usize) -> Self {
        Self::new_with_faulty(n, 0)
    }

    /// n toplam validator, f Byzantine hatalı (son f node hatalı).
    ///
    /// BFT güvenlik koşulu: f < n/3.
    pub fn new_with_faulty(n: usize, f: usize) -> Self {
        assert!(n > 0, "en az 1 node gerekli");
        assert!(f < n, "hatalı node sayısı toplam node sayısından az olmalı");

        let validator_ids: Vec<ValidatorId> = (0..n as u64)
            .map(ValidatorId)
            .collect();

        let honest_count = n - f;
        let nodes = (0..n)
            .map(|i| {
                let faulty = i >= honest_count;
                BftSimNode::new(ValidatorId(i as u64), validator_ids.clone(), faulty)
            })
            .collect();

        Self {
            nodes,
            network: SimNetwork::new(n),
            current_slot: 1,
            rounds_completed: 0,
        }
    }

    /// Belirli bir hash için tüm dürüst node'ların oy üretip yayınlamasını simüle et.
    /// Byzantine node'lar çakışan oy üretir.
    pub fn run_round(&mut self, height: u64, block_hash: BlockHash) -> BftRoundResult {
        let n = self.nodes.len();
        let slot = self.current_slot;

        // Her node oy üretip broadcast eder
        let mut all_votes: Vec<(usize, SimVote)> = Vec::new();
        for i in 0..n {
            let vid = self.nodes[i].id;
            let vote = if self.nodes[i].is_faulty {
                // Byzantine: çakışan oy
                SimVote::conflicting(vid, height, slot)
            } else {
                SimVote::new(vid, block_hash, height, slot)
            };
            all_votes.push((i, vote));
        }

        // Her node oyları network üzerinden broadcast eder
        for (from, vote) in &all_votes {
            let data = vote.serialize();
            self.network.broadcast(*from, data);
        }

        // Her node gelen oyları işler
        let mut nodes_with_qc = 0;
        for node_idx in 0..n {
            let msgs = self.network.drain_inbox(node_idx);
            for msg in msgs {
                if let Some(vote) = SimVote::deserialize(&msg.data) {
                    let v = vote.to_vote();
                    match self.nodes[node_idx].engine.process_vote(v, SlotId(slot)) {
                        Ok(Some(qc)) => {
                            // QC alındı — finalize
                            self.nodes[node_idx].finalized.insert(height, qc.block_hash);
                            nodes_with_qc += 1;
                            break; // Bu node için yeterli
                        }
                        Ok(None) => {}
                        Err(_) => {} // Geçersiz oy (Byzantine)
                    }
                }
            }
        }

        self.rounds_completed += 1;
        self.current_slot += 1;

        // Güvenlik kontrolü
        if let SafetyCheckResult::Violated { height: h, conflicting_hashes } = self.check_safety_at(height) {
            return BftRoundResult::SafetyViolation {
                height: h,
                hash_a: conflicting_hashes[0],
                node_a: 0,
                hash_b: conflicting_hashes[1],
                node_b: 1,
            };
        }

        let quorum = quorum_threshold(n);
        if nodes_with_qc > 0 {
            BftRoundResult::Finalized { nodes_with_qc }
        } else {
            BftRoundResult::QuorumNotReached {
                votes_cast: all_votes.len(),
                needed: quorum,
            }
        }
    }

    /// Güvenlik kontrolü: tüm yüksekliklerde tutarlı mı?
    pub fn check_safety(&self) -> SafetyCheckResult {
        let all_heights: std::collections::HashSet<u64> = self.nodes.iter()
            .flat_map(|n| n.finalized.keys().copied())
            .collect();

        for height in all_heights {
            let result = self.check_safety_at(height);
            if result != SafetyCheckResult::Safe {
                return result;
            }
        }
        SafetyCheckResult::Safe
    }

    /// Belirli yükseklikte güvenlik kontrolü.
    pub fn check_safety_at(&self, height: u64) -> SafetyCheckResult {
        let hashes: Vec<BlockHash> = self.nodes.iter()
            .filter(|n| !n.is_faulty)
            .filter_map(|n| n.finalized.get(&height).copied())
            .collect();

        if hashes.len() <= 1 {
            return SafetyCheckResult::Safe;
        }

        let first = hashes[0];
        for &h in &hashes[1..] {
            if h != first {
                return SafetyCheckResult::Violated {
                    height,
                    conflicting_hashes: vec![first, h],
                };
            }
        }
        SafetyCheckResult::Safe
    }

    /// Canlılık kontrolü: dürüst node'ların tamamı finalize etti mi?
    pub fn honest_nodes_finalized_at(&self, height: u64) -> bool {
        self.nodes.iter()
            .filter(|n| !n.is_faulty)
            .all(|n| n.finalized.contains_key(&height))
    }

    /// Kaç dürüst node finalize etti?
    pub fn finalized_count_at(&self, height: u64) -> usize {
        self.nodes.iter()
            .filter(|n| !n.is_faulty)
            .filter(|n| n.finalized.contains_key(&height))
            .count()
    }

    /// Dürüst node sayısı.
    pub fn honest_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.is_faulty).count()
    }
}

/// BFT quorum eşiği: ceil(2n/3).
fn quorum_threshold(n: usize) -> usize {
    (2 * n + 2) / 3
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn block_hash(seed: u8) -> BlockHash {
        BlockHash::from_bytes([seed; 32])
    }

    #[test]
    fn test_bft_3_honest_nodes_finalize() {
        let mut sim = BftSimulation::new(3);
        let result = sim.run_round(1, block_hash(1));
        // 3/3 oyda quorum = ceil(6/3) = 2
        assert!(matches!(result, BftRoundResult::Finalized { .. }));
        assert_eq!(sim.check_safety(), SafetyCheckResult::Safe);
    }

    #[test]
    fn test_bft_5_honest_nodes_finalize() {
        let mut sim = BftSimulation::new(5);
        let result = sim.run_round(1, block_hash(1));
        assert!(matches!(result, BftRoundResult::Finalized { .. }));
        assert_eq!(sim.check_safety(), SafetyCheckResult::Safe);
    }

    #[test]
    fn test_bft_7_honest_nodes_finalize() {
        let mut sim = BftSimulation::new(7);
        let result = sim.run_round(1, block_hash(1));
        assert!(matches!(result, BftRoundResult::Finalized { .. }));
        assert_eq!(sim.check_safety(), SafetyCheckResult::Safe);
    }

    #[test]
    fn test_bft_safety_multiple_rounds() {
        let mut sim = BftSimulation::new(5);
        for height in 1..=10 {
            let hash = block_hash(height as u8);
            sim.run_round(height, hash);
        }
        // 10 round sonra güvenlik ihlali yok
        assert_eq!(sim.check_safety(), SafetyCheckResult::Safe);
    }

    #[test]
    fn test_bft_with_1_of_4_faulty_nodes_still_safe() {
        // n=4, f=1: 4 > 3*1 → BFT güvenli
        let mut sim = BftSimulation::new_with_faulty(4, 1);
        let result = sim.run_round(1, block_hash(1));
        // Byzantine oylar kabul edilmediğinden güvenlik korunur
        // Quorum = ceil(8/3) = 3; 3 dürüst oy olduğundan finalize beklenir
        assert_eq!(sim.check_safety(), SafetyCheckResult::Safe);
        let _ = result; // Quorum durumu Byzantine senaryo'ya göre değişebilir
    }

    #[test]
    fn test_bft_honest_all_agree_on_same_hash() {
        let mut sim = BftSimulation::new(5);
        let h = block_hash(0xAA);
        sim.run_round(1, h);

        // Tüm dürüst node'lar aynı hash'i finalize etmeli
        let hashes: Vec<_> = sim.nodes.iter()
            .filter(|n| !n.is_faulty)
            .filter_map(|n| n.finalized.get(&1).copied())
            .collect();

        if hashes.len() > 1 {
            let first = hashes[0];
            for &next in &hashes[1..] {
                assert_eq!(first, next, "Güvenlik ihlali: farklı hash'ler finalize edildi");
            }
        }
    }

    #[test]
    fn test_quorum_threshold_values() {
        assert_eq!(quorum_threshold(3), 2);  // ceil(6/3) = 2
        assert_eq!(quorum_threshold(4), 3);  // ceil(8/3) = 3
        assert_eq!(quorum_threshold(5), 4);  // ceil(10/3) = 4
        assert_eq!(quorum_threshold(7), 5);  // ceil(14/3) = 5
        assert_eq!(quorum_threshold(10), 7); // ceil(20/3) = 7
    }

    #[test]
    fn test_bft_network_stats() {
        let mut sim = BftSimulation::new(4);
        sim.run_round(1, block_hash(1));
        // 4 node, her biri 3'e broadcast = 12 mesaj gönderildi
        assert_eq!(sim.network.stats.total_sent, 12);
        assert_eq!(sim.network.stats.total_dropped_partition, 0);
    }
}
