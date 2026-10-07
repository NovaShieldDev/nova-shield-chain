//! CommitmentTree — shielded note commitment'larını tutan inkremental Merkle ağacı (§4.3)
//!
//! ## Tasarım
//!
//! Zcash Sapling "Incremental Merkle Tree" ve Penumbra TCT'den ilham alır:
//! - Ekleme sırası korunur (UTXO tarihi için gerekli)
//! - İkili Merkle ağacı: yaprak = `BLAKE3("nova-zk-leaf:" || cm)`, iç düğüm = `BLAKE3("nova-zk-node:" || sol || sağ)`
//! - Domain separator → ikinci önceden görüntü saldırısına karşı koruma
//! - `MerkleWitness`: tam auth path ile inclusion kanıtı (ZK proof içinde doğrulanabilir)
//!
//! ## MerkleWitness
//!
//! `auth_path: Vec<([u8; 32], bool)>` — `(sibling_hash, current_is_left)`:
//! - `true`  → mevcut düğüm sol, sibling sağ
//! - `false` → sibling sol, mevcut düğüm sağ
//!
//! ## Araştırma kaynakları
//! - Zcash Protocol Spec §4.4 — Incremental Merkle Tree
//! - Penumbra `tct::Tree` — Tiered Commitment Tree
//! - Bitcoin `ComputeMerkleRoot` — tek öğe çiftleme pattern

use crate::errors::{ZkError, ZkResult};
use crate::utxo::note::NoteCommitment;
use parking_lot::RwLock;
use std::sync::Arc;

/// Maksimum ağaç kapasitesi (testnet için)
pub const MAX_TREE_SIZE: usize = 1 << 20; // 1M note

// ─── MerkleWitness ────────────────────────────────────────────────────────────

/// Merkle witness — bir commitment'ın ağaçta bulunduğunun inclusion kanıtı
///
/// `auth_path` ZK proof içinde kullanılabilir:
/// - Her adım: `(sibling_hash, current_is_left)`
/// - `verify_inclusion()` ile kökü yeniden hesapla ve karşılaştır
///
/// Pattern: Zcash `MerklePath`, Penumbra `tct::Proof`.
#[derive(Clone, Debug)]
pub struct MerkleWitness {
    /// İspatlanacak commitment
    pub commitment: NoteCommitment,
    /// Ağaçtaki ekleme sırası pozisyonu
    pub position: usize,
    /// Ağaç kökü (witness oluşturulduğu andaki)
    pub root: [u8; 32],
    /// Auth path: `(sibling_hash, current_is_left)`
    /// - `true`  → mevcut sol, sibling sağ
    /// - `false` → sibling sol, mevcut sağ
    pub auth_path: Vec<([u8; 32], bool)>,
}

impl MerkleWitness {
    /// Hızlı geçerlilik kontrolü (root karşılaştırması)
    pub fn is_valid(&self, tree_root: &[u8; 32]) -> bool {
        self.root == *tree_root
    }

    /// Tam inclusion doğrulaması — auth path ile kökü yeniden hesapla.
    ///
    /// ZK proof içinde doğrulama için kullanılır.
    pub fn verify_inclusion(&self) -> bool {
        let mut current = merkle_leaf_hash(&self.commitment);
        for (sibling, current_is_left) in &self.auth_path {
            current = if *current_is_left {
                // Mevcut sol, sibling sağ
                merkle_node_hash(&current, sibling)
            } else {
                // Sibling sol, mevcut sağ
                merkle_node_hash(sibling, &current)
            };
        }
        current == self.root
    }
}

// ─── Merkle yardımcı fonksiyonlar ─────────────────────────────────────────────

/// Commitment için Merkle yaprak hash'i
///
/// `BLAKE3("nova-zk-merkle-leaf:" || commitment[32])`
pub fn merkle_leaf_hash(cm: &NoteCommitment) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"nova-zk-merkle-leaf:");
    h.update(cm.as_bytes());
    *h.finalize().as_bytes()
}

/// İkili Merkle iç düğüm hash'i
///
/// `BLAKE3("nova-zk-merkle-node:" || left[32] || right[32])`
pub fn merkle_node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"nova-zk-merkle-node:");
    h.update(left);
    h.update(right);
    *h.finalize().as_bytes()
}

/// Yaprak listesinden tüm Merkle seviyelerini inşa et.
///
/// Return: `levels[0]` = yapraklar, `levels[son]` = [kök]
/// Tek öğeli seviyede öğe kendisiyle çiftlenir.
fn build_merkle_levels(leaves: &[[u8; 32]]) -> Vec<Vec<[u8; 32]>> {
    if leaves.is_empty() {
        return vec![];
    }
    let mut levels: Vec<Vec<[u8; 32]>> = vec![leaves.to_vec()];
    while levels.last().unwrap().len() > 1 {
        let prev = levels.last().unwrap().clone();
        let mut next = Vec::with_capacity((prev.len() + 1) / 2);
        let mut i = 0;
        while i < prev.len() {
            let left = &prev[i];
            let right = if i + 1 < prev.len() { &prev[i + 1] } else { left };
            next.push(merkle_node_hash(left, right));
            i += 2;
        }
        levels.push(next);
    }
    levels
}

// ─── CommitmentTree ───────────────────────────────────────────────────────────

/// CommitmentTree — inkremental commitment deposu
#[derive(Debug, Default, Clone)]
pub struct CommitmentTree {
    commitments: Arc<RwLock<Vec<NoteCommitment>>>,
}

impl CommitmentTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Commitment ekle; pozisyonunu döndür
    pub fn insert(&self, commitment: NoteCommitment) -> ZkResult<usize> {
        let mut inner = self.commitments.write();
        if inner.len() >= MAX_TREE_SIZE {
            return Err(ZkError::PoolFull { max_capacity: MAX_TREE_SIZE });
        }
        if inner.contains(&commitment) {
            return Err(ZkError::CommitmentAlreadyExists);
        }
        let pos = inner.len();
        inner.push(commitment);
        Ok(pos)
    }

    /// Ağaç kök hash'i — inkremental ikili Merkle kökü.
    ///
    /// Ekleme sırası korunur (Zcash Incremental Merkle Tree pattern).
    /// Boş ağaç → `[0u8; 32]`.
    pub fn root(&self) -> [u8; 32] {
        let inner = self.commitments.read();
        if inner.is_empty() {
            return [0u8; 32];
        }
        let leaves: Vec<[u8; 32]> = inner.iter().map(merkle_leaf_hash).collect();
        let levels = build_merkle_levels(&leaves);
        *levels.last().unwrap().first().unwrap()
    }

    /// Commitment ağaçta mevcut mu?
    pub fn contains(&self, commitment: &NoteCommitment) -> bool {
        self.commitments.read().contains(commitment)
    }

    /// Toplam commitment sayısı
    pub fn len(&self) -> usize {
        self.commitments.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.commitments.read().is_empty()
    }

    /// Bir commitment için tam Merkle witness üret (auth path dahil).
    ///
    /// Auth path ZK proof içinde `verify_inclusion()` ile doğrulanabilir.
    /// Pattern: Zcash `MerklePath::auth_path`, Penumbra `tct::Proof`.
    pub fn witness(&self, commitment: &NoteCommitment) -> Option<MerkleWitness> {
        let inner = self.commitments.read();
        let position = inner.iter().position(|c| c == commitment)?;

        let leaves: Vec<[u8; 32]> = inner.iter().map(merkle_leaf_hash).collect();
        let levels = build_merkle_levels(&leaves);

        if levels.is_empty() {
            return None;
        }

        // Auth path: her seviyede sibling ve mevcut pozisyonun hangi tarafta olduğu
        let mut auth_path = Vec::new();
        let mut pos = position;
        for level in &levels[..levels.len() - 1] {
            let current_is_left = pos % 2 == 0;
            let sibling_pos = if current_is_left {
                (pos + 1).min(level.len() - 1) // Tek öğeyse kendisi sibling
            } else {
                pos - 1
            };
            auth_path.push((level[sibling_pos], current_is_left));
            pos /= 2;
        }

        let root = *levels.last().unwrap().first().unwrap();
        Some(MerkleWitness { commitment: *commitment, position, root, auth_path })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cm(seed: u8) -> NoteCommitment {
        NoteCommitment([seed; 32])
    }

    #[test]
    fn test_tree_empty_root() {
        let tree = CommitmentTree::new();
        assert_eq!(tree.root(), [0u8; 32]);
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_tree_insert_and_contains() {
        let tree = CommitmentTree::new();
        let c = cm(1);
        let pos = tree.insert(c).unwrap();
        assert_eq!(pos, 0);
        assert!(tree.contains(&c));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_tree_root_changes_on_insert() {
        let tree = CommitmentTree::new();
        let root0 = tree.root();
        tree.insert(cm(1)).unwrap();
        let root1 = tree.root();
        tree.insert(cm(2)).unwrap();
        let root2 = tree.root();
        assert_ne!(root0, root1);
        assert_ne!(root1, root2);
    }

    #[test]
    fn test_tree_root_deterministic() {
        let t1 = CommitmentTree::new();
        let t2 = CommitmentTree::new();
        t1.insert(cm(1)).unwrap();
        t1.insert(cm(2)).unwrap();
        t2.insert(cm(1)).unwrap();
        t2.insert(cm(2)).unwrap();
        assert_eq!(t1.root(), t2.root());
    }

    #[test]
    fn test_tree_duplicate_rejected() {
        let tree = CommitmentTree::new();
        tree.insert(cm(1)).unwrap();
        let err = tree.insert(cm(1));
        assert!(matches!(err, Err(ZkError::CommitmentAlreadyExists)));
    }

    #[test]
    fn test_tree_witness() {
        let tree = CommitmentTree::new();
        tree.insert(cm(1)).unwrap();
        tree.insert(cm(2)).unwrap();
        let w = tree.witness(&cm(1)).unwrap();
        assert_eq!(w.position, 0);
        assert!(w.is_valid(&tree.root()));
    }

    #[test]
    fn test_tree_witness_not_found() {
        let tree = CommitmentTree::new();
        assert!(tree.witness(&cm(99)).is_none());
    }

    // ─── FAZ Q: Gerçek Merkle testleri ───────────────────────────────────

    #[test]
    fn test_merkle_leaf_hash_deterministic() {
        let c = cm(42);
        assert_eq!(merkle_leaf_hash(&c), merkle_leaf_hash(&c));
    }

    #[test]
    fn test_merkle_leaf_hash_different_per_commitment() {
        assert_ne!(merkle_leaf_hash(&cm(1)), merkle_leaf_hash(&cm(2)));
    }

    #[test]
    fn test_merkle_node_domain_separation() {
        // Yaprak ve iç düğüm aynı girdide farklı hash üretmeli
        let leaf = merkle_leaf_hash(&cm(1));
        let node = merkle_node_hash(&leaf, &leaf);
        assert_ne!(leaf, node, "domain separator etkili olmalı");
    }

    #[test]
    fn test_witness_verify_inclusion_single() {
        let tree = CommitmentTree::new();
        tree.insert(cm(1)).unwrap();
        let w = tree.witness(&cm(1)).unwrap();
        assert!(w.verify_inclusion(), "tek öğe inclusion doğrulamalı");
    }

    #[test]
    fn test_witness_verify_inclusion_two_elements() {
        let tree = CommitmentTree::new();
        tree.insert(cm(1)).unwrap();
        tree.insert(cm(2)).unwrap();
        let w1 = tree.witness(&cm(1)).unwrap();
        let w2 = tree.witness(&cm(2)).unwrap();
        assert!(w1.verify_inclusion(), "cm(1) inclusion doğrulamalı");
        assert!(w2.verify_inclusion(), "cm(2) inclusion doğrulamalı");
    }

    #[test]
    fn test_witness_verify_inclusion_many_elements() {
        let tree = CommitmentTree::new();
        for i in 0u8..8 {
            tree.insert(cm(i)).unwrap();
        }
        for i in 0u8..8 {
            let w = tree.witness(&cm(i)).unwrap();
            assert!(
                w.verify_inclusion(),
                "cm({i}) inclusion doğrulamalı; auth_path len={}",
                w.auth_path.len()
            );
        }
    }

    #[test]
    fn test_witness_verify_inclusion_odd_tree() {
        let tree = CommitmentTree::new();
        for i in 0u8..5 {
            tree.insert(cm(i)).unwrap();
        }
        // Tüm öğeler doğrulanabilmeli (tek öğe çiftleme dahil)
        for i in 0u8..5 {
            let w = tree.witness(&cm(i)).unwrap();
            assert!(w.verify_inclusion(), "tek ağaç cm({i}) doğrulamalı");
        }
    }

    #[test]
    fn test_witness_root_matches_tree_root() {
        let tree = CommitmentTree::new();
        tree.insert(cm(10)).unwrap();
        tree.insert(cm(20)).unwrap();
        tree.insert(cm(30)).unwrap();
        let tree_root = tree.root();
        let w = tree.witness(&cm(20)).unwrap();
        assert_eq!(w.root, tree_root, "witness kökü ağaç köküyle eşleşmeli");
    }

    #[test]
    fn test_tampered_witness_fails_verification() {
        let tree = CommitmentTree::new();
        tree.insert(cm(1)).unwrap();
        tree.insert(cm(2)).unwrap();
        let mut w = tree.witness(&cm(1)).unwrap();
        // Kökü boz
        w.root = [0xff; 32];
        assert!(!w.is_valid(&tree.root()), "bozuk kök is_valid false dönmeli");
    }

    #[test]
    fn test_auth_path_length_correct() {
        // N yaprak için auth path uzunluğu = ceil(log2(N))
        let tree = CommitmentTree::new();
        tree.insert(cm(1)).unwrap(); // 1 yaprak → auth_path = 0 adım
        let w = tree.witness(&cm(1)).unwrap();
        assert_eq!(w.auth_path.len(), 0, "tek yaprak için auth path boş");

        let tree2 = CommitmentTree::new();
        for i in 0u8..4 {
            tree2.insert(cm(i)).unwrap();
        }
        let w2 = tree2.witness(&cm(0)).unwrap();
        assert_eq!(w2.auth_path.len(), 2, "4 yaprak → 2 adım auth path");
    }
}
