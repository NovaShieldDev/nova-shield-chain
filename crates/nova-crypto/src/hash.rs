//! BLAKE3 hash wrapper ve Merkle ağaç implementasyonu.
//!
//! Nova Shield Chain'de tüm hash işlemleri BLAKE3 kullanır. §6.1.3
//!
//! ## Neden BLAKE3?
//! - SHA3'ten 3-10x daha hızlı (SIMD optimized, paralel)
//! - SHA256'dan 8-14x daha hızlı
//! - Güvenlik: 256-bit collision resistance
//! - `rayon` feature ile paralel Merkle tree
//! - Referans: blst, solana banking_stage (yüksek TX throughput)

use nova_primitives::crypto::Hash;
use crate::errors::{CryptoError, CryptoResult};

// ─── TEMEL HASH FONKSİYONLARI ─────────────────────────────────────────────────

/// Tek bir byte slice'ı BLAKE3 ile hashle.
///
/// # Örnek
/// ```
/// use nova_crypto::hash::blake3_hash;
/// let h = blake3_hash(b"Nova Shield Chain");
/// assert_ne!(h, nova_primitives::crypto::Hash::ZERO);
/// ```
pub fn blake3_hash(data: &[u8]) -> Hash {
    let result = blake3::hash(data);
    Hash::from_bytes(*result.as_bytes())
}

/// Birden fazla slice'ı (sırasıyla) BLAKE3 ile hashle.
/// `blake3::Hasher::update` ile zincirleme — tüm parçaları concat'lamakla eşdeğer.
pub fn blake3_hash_many(parts: &[&[u8]]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(part);
    }
    Hash::from_bytes(*hasher.finalize().as_bytes())
}

/// Keyed (HMAC benzeri) BLAKE3 hash.
/// MAC ve PRF olarak kullanılır.
pub fn blake3_keyed_hash(key: &[u8; 32], data: &[u8]) -> Hash {
    let result = blake3::keyed_hash(key, data);
    Hash::from_bytes(*result.as_bytes())
}

/// Türetilmiş anahtar (KDF benzeri) — BLAKE3 derive_key.
/// Context string + material → 32 byte anahtar.
pub fn blake3_derive_key(context: &str, key_material: &[u8]) -> [u8; 32] {
    // blake3::derive_key(context, ikm) → [u8; 32]
    blake3::derive_key(context, key_material)
}

// ─── BLAKE3 HASHER WRAPPER ────────────────────────────────────────────────────

/// İncremenal BLAKE3 hasher.
/// Büyük veriyi parça parça hash'lemek için kullanılır.
pub struct Blake3Hasher {
    inner: blake3::Hasher,
}

impl Blake3Hasher {
    /// Yeni hasher başlat.
    pub fn new() -> Self {
        Self { inner: blake3::Hasher::new() }
    }

    /// Keyed hasher başlat.
    pub fn new_keyed(key: &[u8; 32]) -> Self {
        Self { inner: blake3::Hasher::new_keyed(key) }
    }

    /// Veri ekle.
    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        self.inner.update(data);
        self
    }

    /// Hash'i tamamla.
    pub fn finalize(&self) -> Hash {
        Hash::from_bytes(*self.inner.finalize().as_bytes())
    }

    /// Hash'i tamamla ve hasher'ı sıfırla.
    pub fn finalize_and_reset(&mut self) -> Hash {
        let result = self.finalize();
        self.inner.reset();
        result
    }
}

impl Default for Blake3Hasher {
    fn default() -> Self {
        Self::new()
    }
}

// ─── MERKLE AĞACI ─────────────────────────────────────────────────────────────

/// Binary Merkle ağacı (BLAKE3 tabanlı).
///
/// Referans: go-ethereum trie/merkle, solana AccountsDB Merkle.
///
/// ## Özellikler
/// - Yaprak hash'leri: `H(0x00 || leaf_data)` (domain separation)
/// - Iç düğüm hash'leri: `H(0x01 || left || right)` (second-preimage koruması)
/// - Tek eleman → o elemanın hash'i root'tur
/// - Sıfır eleman → `Hash::ZERO`
pub struct MerkleTree {
    /// Tüm seviyelerin hash'leri (0. seviye = yaprak, son seviye = root).
    layers: Vec<Vec<Hash>>,
}

impl MerkleTree {
    // Domain separation prefix'leri (ikinci ön görüntü saldırısına karşı)
    const LEAF_PREFIX: u8 = 0x00;
    const INNER_PREFIX: u8 = 0x01;

    /// Yaprak verilerinden Merkle ağacı inşa et.
    pub fn new(leaves: &[Vec<u8>]) -> Self {
        if leaves.is_empty() {
            return Self { layers: vec![] };
        }

        // 0. katman: yaprak hash'leri
        let leaf_hashes: Vec<Hash> = leaves
            .iter()
            .map(|leaf| Self::hash_leaf(leaf))
            .collect();

        let mut layers = vec![leaf_hashes];

        // Üst katmanları inşa et
        while layers.last().map_or(0, |l| l.len()) > 1 {
            let current = layers.last().unwrap();
            let next = Self::build_next_layer(current);
            layers.push(next);
        }

        Self { layers }
    }

    /// Hash'lenmiş yapraklardan Merkle ağacı inşa et.
    /// Yaprak hash'leri zaten hesaplanmışsa bu daha verimlidir.
    pub fn from_leaf_hashes(leaf_hashes: Vec<Hash>) -> Self {
        if leaf_hashes.is_empty() {
            return Self { layers: vec![] };
        }

        let mut layers = vec![leaf_hashes];
        while layers.last().map_or(0, |l| l.len()) > 1 {
            let current = layers.last().unwrap();
            let next = Self::build_next_layer(current);
            layers.push(next);
        }

        Self { layers }
    }

    /// Merkle root'u döndür.
    pub fn root(&self) -> Hash {
        self.layers
            .last()
            .and_then(|l| l.first())
            .copied()
            .unwrap_or(Hash::ZERO)
    }

    /// `index` numaralı yaprak için Merkle kanıtı üret.
    /// Kanıt: root'a çıkmak için gereken sibling hash'lerin listesi.
    pub fn proof(&self, index: usize) -> CryptoResult<MerkleProof> {
        if self.layers.is_empty() {
            return Err(CryptoError::InvalidMerkleProof);
        }

        let leaf_count = self.layers[0].len();
        if index >= leaf_count {
            return Err(CryptoError::InvalidParameter(
                format!("index {index} >= leaf count {leaf_count}"),
            ));
        }

        let mut siblings = Vec::new();
        let mut current_index = index;

        for layer in &self.layers[..self.layers.len().saturating_sub(1)] {
            let sibling_index = if current_index % 2 == 0 {
                current_index + 1
            } else {
                current_index - 1
            };

            // Eğer sibling yoksa (tek düğüm), kendi hash'ini kullan
            let sibling = if sibling_index < layer.len() {
                layer[sibling_index]
            } else {
                layer[current_index]
            };

            siblings.push(MerkleProofNode {
                hash: sibling,
                direction: if current_index % 2 == 0 {
                    SiblingDirection::Right
                } else {
                    SiblingDirection::Left
                },
            });

            current_index /= 2;
        }

        Ok(MerkleProof {
            leaf_index: index,
            siblings,
            root: self.root(),
        })
    }

    /// Yaprak sayısı.
    pub fn leaf_count(&self) -> usize {
        self.layers.first().map_or(0, |l| l.len())
    }

    /// Belirli bir yaprağın hash'ini döndür.
    pub fn leaf_hash(&self, index: usize) -> Option<Hash> {
        self.layers.first()?.get(index).copied()
    }

    // ─── Özel yardımcı fonksiyonlar ──────────────────────────────────────────

    fn hash_leaf(data: &[u8]) -> Hash {
        blake3_hash_many(&[&[Self::LEAF_PREFIX], data])
    }

    fn hash_pair(left: &Hash, right: &Hash) -> Hash {
        blake3_hash_many(&[&[Self::INNER_PREFIX], left.as_bytes(), right.as_bytes()])
    }

    fn build_next_layer(current: &[Hash]) -> Vec<Hash> {
        let mut next = Vec::with_capacity((current.len() + 1) / 2);
        let mut i = 0;
        while i < current.len() {
            let left = &current[i];
            let right = if i + 1 < current.len() {
                &current[i + 1]
            } else {
                // Tek eleman: kendisiyle eşleştir (sağa kopyala)
                &current[i]
            };
            next.push(Self::hash_pair(left, right));
            i += 2;
        }
        next
    }
}

// ─── MERKLE KANITI ────────────────────────────────────────────────────────────

/// Merkle kanıt düğümündeki sibling'in konumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiblingDirection {
    /// Sibling solda (mevcut düğüm sağda).
    Left,
    /// Sibling sağda (mevcut düğüm solda).
    Right,
}

/// Merkle kanıt düğümü.
#[derive(Debug, Clone)]
pub struct MerkleProofNode {
    /// Sibling hash'i.
    pub hash: Hash,
    /// Sibling'in konumu.
    pub direction: SiblingDirection,
}

/// Merkle üyelik kanıtı.
/// Bir yaprağın belirli bir root'a ait olduğunu kanıtlar.
#[derive(Debug, Clone)]
pub struct MerkleProof {
    /// Kanıtlanan yaprağın index'i.
    pub leaf_index: usize,
    /// Sibling hash'leri (yapraktan root'a doğru).
    pub siblings: Vec<MerkleProofNode>,
    /// Beklenen root.
    pub root: Hash,
}

impl MerkleProof {
    /// Verilen yaprak hash'inin bu kanıtla root'a uyduğunu doğrula.
    pub fn verify(&self, leaf_hash: &Hash) -> bool {
        let mut current = *leaf_hash;

        for node in &self.siblings {
            current = match node.direction {
                SiblingDirection::Right => {
                    // Mevcut solda, sibling sağda
                    blake3_hash_many(&[
                        &[MerkleTree::INNER_PREFIX],
                        current.as_bytes(),
                        node.hash.as_bytes(),
                    ])
                }
                SiblingDirection::Left => {
                    // Sibling solda, mevcut sağda
                    blake3_hash_many(&[
                        &[MerkleTree::INNER_PREFIX],
                        node.hash.as_bytes(),
                        current.as_bytes(),
                    ])
                }
            };
        }

        current == self.root
    }
}

// ─── BAĞIMSIZ merkle_root FONKSİYONU ─────────────────────────────────────────

/// Hash listesinden Merkle root hesapla (tam ağaç inşa etmeden).
/// Blok TX root'u gibi tek seferlik hesaplamalar için optimize.
pub fn merkle_root(leaf_hashes: &[Hash]) -> Hash {
    if leaf_hashes.is_empty() {
        return Hash::ZERO;
    }
    if leaf_hashes.len() == 1 {
        return leaf_hashes[0];
    }

    let tree = MerkleTree::from_leaf_hashes(leaf_hashes.to_vec());
    tree.root()
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_basic() {
        let h1 = blake3_hash(b"hello");
        let h2 = blake3_hash(b"hello");
        let h3 = blake3_hash(b"world");
        assert_eq!(h1, h2, "aynı girdi → aynı hash");
        assert_ne!(h1, h3, "farklı girdi → farklı hash");
        assert_ne!(h1, Hash::ZERO, "sıfır hash değil");
    }

    #[test]
    fn test_blake3_hash_many_vs_concat() {
        // blake3_hash_many([a, b]) == blake3_hash([a || b])
        let a = b"Nova";
        let b = b"Shield";
        let h_many = blake3_hash_many(&[a, b]);
        let mut combined = a.to_vec();
        combined.extend_from_slice(b);
        let h_concat = blake3_hash(&combined);
        assert_eq!(h_many, h_concat, "hash_many == concat hash");
    }

    #[test]
    fn test_merkle_single() {
        let leaves = vec![b"genesis".to_vec()];
        let tree = MerkleTree::new(&leaves);
        // Tek yaprak → root = leaf_hash
        let expected = blake3_hash_many(&[&[MerkleTree::LEAF_PREFIX], b"genesis"]);
        assert_eq!(tree.root(), expected);
    }

    #[test]
    fn test_merkle_two_leaves() {
        let leaves: Vec<Vec<u8>> = vec![b"tx1".to_vec(), b"tx2".to_vec()];
        let tree = MerkleTree::new(&leaves);
        assert_ne!(tree.root(), Hash::ZERO);
    }

    #[test]
    fn test_merkle_proof_verify() {
        let leaves: Vec<Vec<u8>> = (0..8)
            .map(|i| format!("leaf-{i}").into_bytes())
            .collect();
        let tree = MerkleTree::new(&leaves);

        for i in 0..8 {
            let proof = tree.proof(i).expect("kanıt oluşturulmalı");
            let leaf_hash = blake3_hash_many(&[&[MerkleTree::LEAF_PREFIX], &leaves[i]]);
            assert!(proof.verify(&leaf_hash), "kanıt doğrulanmalı: index={i}");
        }
    }

    #[test]
    fn test_merkle_proof_tamper_detection() {
        let leaves: Vec<Vec<u8>> = vec![
            b"legitimate-tx".to_vec(),
            b"another-tx".to_vec(),
        ];
        let tree = MerkleTree::new(&leaves);
        let proof = tree.proof(0).unwrap();

        // Farklı yaprak hash'iyle doğrulama başarısız olmalı
        let wrong_hash = blake3_hash(b"malicious-tx");
        assert!(!proof.verify(&wrong_hash), "sahte TX kanıtı geçersiz olmalı");
    }

    #[test]
    fn test_merkle_empty() {
        let root = merkle_root(&[]);
        assert_eq!(root, Hash::ZERO);
    }

    #[test]
    fn test_hasher_incremental() {
        let mut h = Blake3Hasher::new();
        h.update(b"Nova").update(b"Shield");
        let incremental = h.finalize();

        let direct = blake3_hash_many(&[b"Nova", b"Shield"]);
        assert_eq!(incremental, direct);
    }
}
