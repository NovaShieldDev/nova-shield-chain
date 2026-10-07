//! Pedersen commitment şeması.
//!
//! §5.2.1 — Şifreli mempool TX commitment'ları için kullanılır.
//! Commitment = BLAKE3(TX || nonce || slot_id)
//!
//! ## Pedersen Commitment (kurve25519)
//! C = r·G + m·H
//! - G, H: bağımsız generator noktaları (nothing-up-my-sleeve)
//! - r: blinding factor (gizlilik sağlar)
//! - m: mesaj (miktar)
//! - Hiding: r rastgele → C'den m öğrenilemez
//! - Binding: C'yi farklı (m', r') ile açmak = DL problemi
//!
//! ## Basit Hash Commitment
//! Nova Shield mempool'da kriptografik Pedersen yerine hash commitment kullanır.
//! Neden: Validator'lar TX içeriğini görmeden sadece hash'i saklar.
//! Açılış (reveal) aşamasında TX commitment'la eşleşmeli.
//!
//! Referans: rolling-shutter mempool (03-rolling-shutter.md)

use nova_primitives::crypto::Hash;
use nova_primitives::types::{SlotId, Timestamp};
use crate::errors::{CryptoError, CryptoResult};
use crate::hash::{blake3_hash_many};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

// ─── TX COMMİTMENT ───────────────────────────────────────────────────────────

/// TX commitment — şifreli mempool'da TX'i temsil eder.
///
/// Format: `C = BLAKE3(encrypted_tx || nonce || slot_id)`
/// §5.2.1: TX gönderilmeden önce commitment hash'i yayınlanır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxCommitment {
    /// Commitment hash'i.
    pub hash: Hash,
    /// Slot numarası (bu slot'ta açılacak).
    pub target_slot: SlotId,
    /// Commitment zaman damgası.
    pub timestamp: Timestamp,
}

/// TX commitment builder.
pub struct CommitmentBuilder {
    nonce: [u8; 32],
}

impl CommitmentBuilder {
    /// Rastgele nonce ile yeni builder.
    pub fn new() -> Self {
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        Self { nonce }
    }

    /// Belirli nonce ile builder (determinizm için test'te kullanılır).
    pub fn with_nonce(nonce: [u8; 32]) -> Self {
        Self { nonce }
    }

    /// Şifreli TX için commitment oluştur.
    ///
    /// `encrypted_tx`: Şifreli TX byte'ları
    /// `slot`: Hedef slot numarası
    pub fn commit(
        &self,
        encrypted_tx: &[u8],
        slot: SlotId,
        timestamp: Timestamp,
    ) -> TxCommitment {
        let slot_bytes = slot.0.to_le_bytes();
        let hash = blake3_hash_many(&[encrypted_tx, &self.nonce, &slot_bytes]);
        TxCommitment { hash, target_slot: slot, timestamp }
    }

    /// Nonce'u döndür (reveal aşamasında kullanılır).
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }
}

impl Default for CommitmentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Zeroize for CommitmentBuilder {
    fn zeroize(&mut self) {
        self.nonce.zeroize();
    }
}

/// Commitment doğrulama.
/// Reveal aşamasında: TX + nonce + slot → hash == commitment.hash?
pub fn verify_commitment(
    commitment: &TxCommitment,
    encrypted_tx: &[u8],
    nonce: &[u8; 32],
) -> bool {
    let slot_bytes = commitment.target_slot.0.to_le_bytes();
    let expected = blake3_hash_many(&[encrypted_tx, nonce, &slot_bytes]);
    commitment.hash == expected
}

// ─── PEDERSEN COMMITMENT (EC tabanlı) ────────────────────────────────────────
//
// ZK-Shield UTXO gizliliği için Pedersen commitment.
// Miktar gizlemek için kullanılır: C = r·G + m·H
//
// Curve25519 Ristretto255 ile:
// - Güvenlik: 128-bit
// - Hız: secp256k1'den daha hızlı (Edwards eğrisi)
// - Constant-time: Yan kanal saldırısına dayanıklı

use curve25519_dalek::{
    ristretto::{CompressedRistretto, RistrettoPoint},
    scalar::Scalar,
    traits::MultiscalarMul,
};

/// Pedersen commitment sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PedersenCommitment {
    /// Commitment noktası (compressed Ristretto255, 32 byte).
    pub compressed: [u8; 32],
}

impl PedersenCommitment {
    /// Commitment'ı doğrula: C == r·G + m·H?
    pub fn verify(&self, amount: u64, blinding: &Scalar) -> CryptoResult<bool> {
        let g = PEDERSEN_G();
        let h = PEDERSEN_H();
        let m_scalar = Scalar::from(amount);

        let expected = RistrettoPoint::multiscalar_mul(
            &[*blinding, m_scalar],
            &[g, h],
        );

        let our_point = CompressedRistretto::from_slice(&self.compressed)
            .map_err(|_| CryptoError::Commitment("decompress başarısız".into()))?
            .decompress()
            .ok_or_else(|| CryptoError::Commitment("geçersiz nokta".into()))?;

        Ok(our_point == expected)
    }

    /// Commitment byte'larını döndür.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.compressed
    }
}

/// Pedersen commitment oluştur.
///
/// `amount`: Gizlenecek miktar
/// `blinding`: Blinding factor (gizlilik için rastgele Scalar)
pub fn pedersen_commit(amount: u64, blinding: &Scalar) -> PedersenCommitment {
    let g = PEDERSEN_G();
    let h = PEDERSEN_H();
    let m_scalar = Scalar::from(amount);

    let point = RistrettoPoint::multiscalar_mul(
        &[*blinding, m_scalar],
        &[g, h],
    );

    PedersenCommitment {
        compressed: point.compress().to_bytes(),
    }
}

/// Rastgele blinding factor üret.
pub fn random_blinding() -> Scalar {
    Scalar::random(&mut OsRng)
}

// Generator noktaları — "nothing up my sleeve" sayılar
// H = BLAKE3("nova-pedersen-H") → hash_to_curve

#[allow(non_snake_case)]
fn PEDERSEN_G() -> RistrettoPoint {
    curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT
}

#[allow(non_snake_case)]
fn PEDERSEN_H() -> RistrettoPoint {
    // H = hash_to_curve(BLAKE3("nova-shield-pedersen-H-generator"))
    // Deterministik, bağımsız generator noktası
    use curve25519_dalek::ristretto::RistrettoPoint;
    let hash = blake3_hash_many(&[b"nova-shield-pedersen-H-generator-2026"]);
    RistrettoPoint::hash_from_bytes::<sha2::Sha512>(hash.as_bytes())
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_commitment_roundtrip() {
        let builder = CommitmentBuilder::new();
        let encrypted_tx = b"encrypted-tx-data-padded-to-512-bytes";
        let slot = SlotId(100);
        let ts = Timestamp(1_700_000_000_000);

        let commitment = builder.commit(encrypted_tx, slot, ts);
        assert!(
            verify_commitment(&commitment, encrypted_tx, builder.nonce()),
            "doğru TX+nonce commitment doğrulanmalı"
        );
    }

    #[test]
    fn test_commitment_tamper_detection() {
        let builder = CommitmentBuilder::new();
        let commitment = builder.commit(b"original-tx", SlotId(1), Timestamp(0));

        // Farklı TX içeriği reddedilmeli
        assert!(
            !verify_commitment(&commitment, b"tampered-tx", builder.nonce()),
            "sahte TX commitment uyuşmamalı"
        );
    }

    #[test]
    fn test_commitment_wrong_nonce() {
        let builder = CommitmentBuilder::new();
        let commitment = builder.commit(b"tx", SlotId(1), Timestamp(0));

        let wrong_nonce = [99u8; 32];
        assert!(
            !verify_commitment(&commitment, b"tx", &wrong_nonce),
            "yanlış nonce ile commitment uyuşmamalı"
        );
    }

    #[test]
    fn test_pedersen_commit_verify() {
        let amount = 1_000_000u64; // 0.001 NSC
        let blinding = random_blinding();

        let commitment = pedersen_commit(amount, &blinding);
        assert!(
            commitment.verify(amount, &blinding).unwrap(),
            "Pedersen commitment doğrulanmalı"
        );
    }

    #[test]
    fn test_pedersen_wrong_amount() {
        let amount = 1_000u64;
        let blinding = random_blinding();
        let commitment = pedersen_commit(amount, &blinding);

        assert!(
            !commitment.verify(2_000u64, &blinding).unwrap(),
            "yanlış miktar reddedilmeli"
        );
    }

    #[test]
    fn test_pedersen_homomorphic_addition() {
        // Pedersen homomorphic: commit(a) + commit(b) == commit(a+b)
        // C1 + C2 = (r1·G + a·H) + (r2·G + b·H) = (r1+r2)·G + (a+b)·H
        let a = 100u64;
        let b = 200u64;
        let r1 = random_blinding();
        let r2 = random_blinding();

        let c1_point = {
            let g = PEDERSEN_G();
            let h = PEDERSEN_H();
            RistrettoPoint::multiscalar_mul(
                &[r1, Scalar::from(a)],
                &[g, h],
            )
        };
        let c2_point = {
            let g = PEDERSEN_G();
            let h = PEDERSEN_H();
            RistrettoPoint::multiscalar_mul(
                &[r2, Scalar::from(b)],
                &[g, h],
            )
        };

        let sum_commitment = PedersenCommitment {
            compressed: (c1_point + c2_point).compress().to_bytes(),
        };

        let r_sum = r1 + r2;
        assert!(
            sum_commitment.verify(a + b, &r_sum).unwrap(),
            "Pedersen homomorfik toplama çalışmalı"
        );
    }
}
