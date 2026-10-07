//! Shred — Solana Turbine blok yayılım birimi
//!
//! Solana: Bir blok → birden fazla shred'e bölünür
//! Shred türleri: Data, Code (silme kodlaması)
//! Merkle kanıtı ile bütünlük doğrulaması

use blake3;
use serde::{Deserialize, Serialize};

/// Shred veri yükü boyutu — Solana: SHRED_DATA_PAYLOAD_SIZE=1232
pub const SHRED_DATA_PAYLOAD_SIZE: usize = 1_232;

/// Maksimum shred indeksi
pub const MAX_SHRED_INDEX: u32 = 32_768;

// ─── ShredType ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShredType {
    /// Veri shred'i — blok verisi taşır
    Data,
    /// Kod shred'i — silme kodlaması (erasure code)
    Code,
}

// ─── ShredFlags ───────────────────────────────────────────────────────────────

/// Shred bayrakları
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShredFlags {
    /// Son shred mi (blok tamamlandı)
    pub last_in_slot: bool,
    /// Veri şeridi son parçası
    pub last_in_data_set: bool,
}

// ─── ShredId ──────────────────────────────────────────────────────────────────

/// Benzersiz shred tanımlayıcısı
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShredId {
    pub slot: u64,
    pub index: u32,
    pub shred_type: ShredType,
}

impl ShredId {
    pub fn new(slot: u64, index: u32, shred_type: ShredType) -> Self {
        Self { slot, index, shred_type }
    }
}

// ─── Shred ────────────────────────────────────────────────────────────────────

/// Turbine shred'i
/// Solana `Shred` struct benzeri
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shred {
    pub id: ShredId,
    /// Lider validator (block üretici)
    pub leader: [u8; 32],
    /// Atlama sayısı (0=lider, 1=birinci tur, ...)
    pub hop: u8,
    /// Veri yükü
    pub payload: Vec<u8>,
    /// Merkle kökü (bütünlük)
    pub merkle_root: [u8; 32],
    /// Lider imzası (BLAKE3 tabanlı mock)
    pub signature: [u8; 32],
    /// Bayraklar
    pub flags: ShredFlags,
}

impl Shred {
    /// Yeni shred oluştur
    pub fn new(
        slot: u64,
        index: u32,
        shred_type: ShredType,
        leader: [u8; 32],
        payload: Vec<u8>,
    ) -> Self {
        let merkle_root = Self::compute_merkle_root(&payload);
        let signature = Self::sign(&leader, slot, index, &merkle_root);
        Self {
            id: ShredId::new(slot, index, shred_type),
            leader,
            hop: 0,
            payload,
            merkle_root,
            signature,
            flags: ShredFlags::default(),
        }
    }

    /// Son shred olarak işaretle
    pub fn mark_last_in_slot(mut self) -> Self {
        self.flags.last_in_slot = true;
        self
    }

    /// Bütünlük doğrula
    pub fn verify(&self) -> bool {
        // 1. Merkle kök doğrula
        let expected_merkle = Self::compute_merkle_root(&self.payload);
        if expected_merkle != self.merkle_root {
            return false;
        }
        // 2. İmza doğrula
        let expected_sig = Self::sign(
            &self.leader,
            self.id.slot,
            self.id.index,
            &self.merkle_root,
        );
        expected_sig == self.signature
    }

    /// Shred hash'i (yayılım tekilleştirme için)
    pub fn hash(&self) -> [u8; 32] {
        self.signature
    }

    /// Atlama sayısını artır (retransmit)
    pub fn increment_hop(&mut self) {
        self.hop = self.hop.saturating_add(1);
    }

    /// Atlama sınırını aştı mı? — Solana: MAX_NUM_TURBINE_HOPS=4
    pub fn exceeded_max_hops(&self, max_hops: u8) -> bool {
        self.hop >= max_hops
    }

    fn compute_merkle_root(payload: &[u8]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-turbine-merkle");
        hasher.update(payload);
        *hasher.finalize().as_bytes()
    }

    fn sign(leader: &[u8; 32], slot: u64, index: u32, merkle_root: &[u8; 32]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-turbine-shred-sig");
        hasher.update(leader);
        hasher.update(&slot.to_le_bytes());
        hasher.update(&index.to_le_bytes());
        hasher.update(merkle_root);
        *hasher.finalize().as_bytes()
    }
}

// ─── ShredBuilder ─────────────────────────────────────────────────────────────

/// Blok verilerini shred'lere böl
/// Solana: `shredder.entries_to_shreds(entries, ...)`
pub struct ShredBuilder {
    leader: [u8; 32],
    shred_size: usize,
}

impl ShredBuilder {
    pub fn new(leader: [u8; 32], shred_size: usize) -> Self {
        Self { leader, shred_size }
    }

    /// Blok verisini shred'lere böl
    pub fn shred_block(&self, slot: u64, block_data: &[u8]) -> Vec<Shred> {
        let chunks: Vec<&[u8]> = block_data.chunks(self.shred_size).collect();
        let total = chunks.len();
        let mut shreds = Vec::with_capacity(total);

        for (i, chunk) in chunks.iter().enumerate() {
            let mut shred = Shred::new(
                slot,
                i as u32,
                ShredType::Data,
                self.leader,
                chunk.to_vec(),
            );
            if i + 1 == total {
                shred = shred.mark_last_in_slot();
            }
            shreds.push(shred);
        }
        shreds
    }

    /// Shred'leri bloğa birleştir (yeniden yapılandır)
    pub fn deshred(&self, shreds: &[Shred]) -> Option<Vec<u8>> {
        // Sıralı olmalı
        let mut sorted: Vec<&Shred> = shreds.iter().collect();
        sorted.sort_by_key(|s| s.id.index);

        // Tümünü doğrula
        if sorted.iter().any(|s| !s.verify()) {
            return None;
        }

        let mut data = Vec::new();
        for shred in sorted {
            data.extend_from_slice(&shred.payload);
        }
        Some(data)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn leader() -> [u8; 32] {
        [1u8; 32]
    }

    #[test]
    fn test_shred_verify() {
        let shred = Shred::new(100, 0, ShredType::Data, leader(), b"data".to_vec());
        assert!(shred.verify());
    }

    #[test]
    fn test_shred_tampered_payload_fails_verify() {
        let mut shred = Shred::new(100, 0, ShredType::Data, leader(), b"data".to_vec());
        shred.payload[0] ^= 0xFF;
        assert!(!shred.verify());
    }

    #[test]
    fn test_shred_hop_tracking() {
        let mut shred = Shred::new(100, 0, ShredType::Data, leader(), b"data".to_vec());
        assert_eq!(shred.hop, 0);
        shred.increment_hop();
        assert_eq!(shred.hop, 1);
        assert!(!shred.exceeded_max_hops(4));
        for _ in 0..3 {
            shred.increment_hop();
        }
        assert!(shred.exceeded_max_hops(4));
    }

    #[test]
    fn test_shred_builder_single_shred() {
        let builder = ShredBuilder::new(leader(), SHRED_DATA_PAYLOAD_SIZE);
        let data = vec![0u8; 100];
        let shreds = builder.shred_block(50, &data);
        assert_eq!(shreds.len(), 1);
        assert!(shreds[0].flags.last_in_slot);
    }

    #[test]
    fn test_shred_builder_multiple_shreds() {
        let builder = ShredBuilder::new(leader(), 100);
        let data = vec![0xABu8; 350]; // 4 shred
        let shreds = builder.shred_block(99, &data);
        assert_eq!(shreds.len(), 4);
        // Sadece son shred işaretli
        assert!(!shreds[0].flags.last_in_slot);
        assert!(shreds[3].flags.last_in_slot);
    }

    #[test]
    fn test_shred_deshred_roundtrip() {
        let builder = ShredBuilder::new(leader(), 100);
        let data: Vec<u8> = (0..255).collect();
        let shreds = builder.shred_block(1, &data);
        let reconstructed = builder.deshred(&shreds).unwrap();
        assert_eq!(&reconstructed[..data.len()], &data);
    }

    #[test]
    fn test_shred_id_uniqueness() {
        let id1 = ShredId::new(100, 0, ShredType::Data);
        let id2 = ShredId::new(100, 1, ShredType::Data);
        let id3 = ShredId::new(101, 0, ShredType::Data);
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_shred_last_in_slot_flag() {
        let shred = Shred::new(1, 0, ShredType::Data, leader(), b"x".to_vec())
            .mark_last_in_slot();
        assert!(shred.flags.last_in_slot);
        assert!(shred.verify()); // Bayrak eklenmesi doğrulamayı bozmaz
    }
}
