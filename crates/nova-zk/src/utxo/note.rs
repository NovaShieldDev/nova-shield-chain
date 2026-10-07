//! Shielded note — UTXO modeli (Penumbra/Zcash pattern'inden ilham)
//!
//! ## BUG-5: Note Commitment Hash Fonksiyonu
//!
//! **Mevcut (YANLIŞ):** `commit()` BLAKE3 kullanıyor — ZK devresinde kanıtlanamaz.
//!
//! **Hedef (DOĞRU):** Poseidon377 (ark-ff tabanlı, BLS12-377 sahası üzerinde).
//! Penumbra `note.rs`: `poseidon377::hash_6(domain_sep, note_blinding, amount, asset_id, g_d, tk_s)`
//!
//! Poseidon377 ZK-friendly: BLS12-377 Fq alanı içinde çalışır, R1CS devresinde
//! `~1000 constraints` (BLAKE3 ise R1CS'de `~100_000+ constraints`).
//!
//! **Mock Geçici Çözüm:** `MockPoseidon377` — BLAKE3 tabanlı ama Poseidon377 API'si.
//! Gerçek ZK devreler eklenince `poseidon377` crate'i ile değiştirilecek.
//!
//! Commitment = BLAKE3("nova-note-commit-v1" || value || asset_id || randomness || transmission_key)
//! Note, sahibi tarafından gizlice tutulur; zincirde sadece commitment yazılır.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

// ─── MockPoseidon377 ──────────────────────────────────────────────────────────

/// Mock Poseidon377 hash fonksiyonu — gerçek ZK sahası aritmetiği olmadan.
///
/// BUG-5 FIX (kısmi): Gerçek `poseidon377::hash_6()` API'sini taklit eder.
/// Gerçek implementasyonda: `poseidon377 = "0.4"` + `ark-bls12-377 = "0.4"` gerekir.
///
/// Poseidon377 parametreleri (Penumbra'dan):
/// - Alan: BLS12-377 Fq (377-bit prime field)
/// - t=6 (6 giriş), round constants Penumbra'nın `poseidon377_parameters` crate'inden
/// - `hash_6(domain, blinding, amount, asset, g_d, tk)` → Fq
pub(crate) mod mock_poseidon377 {
    /// 6 giriş alan mock Poseidon377 hash.
    /// Gerçekte: `poseidon377::hash_6` (ark-ff Fq öğeleri üzerinde).
    pub fn hash_6(inputs: &[[u8; 32]; 6]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova:mock-poseidon377:hash6:v1!!");
        for inp in inputs {
            hasher.update(inp);
        }
        *hasher.finalize().as_bytes()
    }
}

// ─── AssetId ──────────────────────────────────────────────────────────────────

/// Varlık tanımlayıcısı (NSC veya gelecekte diğer varlıklar)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct AssetId(pub [u8; 32]);

impl AssetId {
    /// NSC native token asset ID
    pub fn nsc() -> Self {
        let mut id = [0u8; 32];
        id[0] = b'N';
        id[1] = b'S';
        id[2] = b'C';
        id[3] = 0x01; // version
        AssetId(id)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub const NSC_ASSET_ID: AssetId = AssetId([
    b'N', b'S', b'C', 0x01, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
]);

// ─── NoteCommitment ───────────────────────────────────────────────────────────

/// Note commitment — zincirde herkese açık, içeriği gizli
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct NoteCommitment(pub [u8; 32]);

impl NoteCommitment {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

// ─── ValueCommitment ──────────────────────────────────────────────────────────

/// Değer commitment — transfer sırasında miktar gizliliği için
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ValueCommitment(pub [u8; 32]);

impl ValueCommitment {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ─── Note ─────────────────────────────────────────────────────────────────────

/// Shielded note — sahibi tarafından gizlice saklanan UTXO
#[derive(Clone, Debug, Serialize, Deserialize, Zeroize)]
pub struct Note {
    /// Token miktarı (u128 = 10^18 smallest unit)
    pub value: u128,
    /// Varlık kimliği (NSC vs. diğer) — zeroize yapılmaz (public)
    #[zeroize(skip)]
    pub asset_id: AssetId,
    /// Rastgele gizleme skalar (blinding factor)
    pub randomness: [u8; 32],
    /// Alıcının transmission key'i (sadece alıcı okuyabilir)
    pub transmission_key: [u8; 32],
}

impl Note {
    pub fn new(
        value: u128,
        asset_id: AssetId,
        randomness: [u8; 32],
        transmission_key: [u8; 32],
    ) -> Self {
        Self { value, asset_id, randomness, transmission_key }
    }

    /// Note commitment hesapla — deterministic.
    ///
    /// Penumbra pattern: `poseidon377::hash_6(domain, blinding, amount, asset_id, g_d, tk)`
    ///
    /// BUG-5: Gerçek implementasyonda `poseidon377::hash_6` kullanılmalı (ZK-friendly).
    /// Şimdi: `MockPoseidon377::hash_6` ile aynı API, BLAKE3 internals.
    pub fn commit(&self) -> NoteCommitment {
        // ZK-friendly MiMC hash — R1CS circuit ile tutarlı
        // mimc_sponge([domain_note, value, randomness, asset_id, transmission_key])
        let cm = crate::circuits::mimc::mimc_note_commitment(
            self.value,
            &self.randomness,
            &self.asset_id.0,
            &self.transmission_key,
        );
        NoteCommitment(cm)
    }

    /// Değer commitment — transfer sırasında miktar gizliliği
    pub fn value_commitment(&self) -> ValueCommitment {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-shield:value-commitment:v1!");
        hasher.update(&self.value.to_le_bytes());
        hasher.update(&self.randomness);
        ValueCommitment(*hasher.finalize().as_bytes())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_note(value: u128) -> Note {
        Note::new(value, NSC_ASSET_ID, [42u8; 32], [1u8; 32])
    }

    #[test]
    fn test_note_commitment_deterministic() {
        let n = make_note(1_000);
        let c1 = n.commit();
        let c2 = n.commit();
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_note_commitment_changes_with_value() {
        let n1 = make_note(1_000);
        let n2 = make_note(2_000);
        assert_ne!(n1.commit(), n2.commit());
    }

    #[test]
    fn test_note_commitment_changes_with_randomness() {
        let n1 = Note::new(1_000, NSC_ASSET_ID, [1u8; 32], [1u8; 32]);
        let n2 = Note::new(1_000, NSC_ASSET_ID, [2u8; 32], [1u8; 32]);
        assert_ne!(n1.commit(), n2.commit());
    }

    #[test]
    fn test_value_commitment() {
        let n = make_note(500);
        let vc1 = n.value_commitment();
        let vc2 = n.value_commitment();
        assert_eq!(vc1, vc2);
    }

    #[test]
    fn test_value_commitment_different_values() {
        let n1 = make_note(100);
        let n2 = make_note(200);
        assert_ne!(n1.value_commitment(), n2.value_commitment());
    }

    #[test]
    fn test_nsc_asset_id() {
        let id = AssetId::nsc();
        assert_eq!(id, NSC_ASSET_ID);
        assert_eq!(id.0[0], b'N');
        assert_eq!(id.0[1], b'S');
        assert_eq!(id.0[2], b'C');
    }

    #[test]
    fn test_commitment_to_hex() {
        let n = make_note(42);
        let hex = n.commit().to_hex();
        assert_eq!(hex.len(), 64); // 32 bytes = 64 hex chars
    }
}
