//! ElGamal threshold şifreleme (Ristretto255 üzerinde).
//!
//! §5.2.2 — Şifreli TX şifreleme.
//! Threshold decryption: n validator'dan t'si çözme payı sağlamalı.
//!
//! ## Şema
//! KeyGen: Pedersen DKG → (pk_threshold, [sk_share_i])
//! Encrypt: (C1, C2) = (r·G, msg·G + r·pk_threshold) — El-Gamal
//! Decrypt_share_i: D_i = sk_share_i · C1
//! Combine: Lagrange interpolasyon → msg·G = C2 - Σ λ_i · D_i
//! Decode: BSGS veya lookup → msg (küçük mesajlar için)
//!
//! NOT: Nova Shield'de TX içeriği büyük → hibrit şifreleme:
//! - AES-GCM ile TX şifrele (simetrik anahtar K)
//! - K'yı ElGamal ile şifrele (threshold için)
//! - Threshold decryption → K → AES-GCM çöz → TX

use curve25519_dalek::{
    ristretto::RistrettoPoint,
    scalar::Scalar,
    traits::MultiscalarMul,
};
use rand::rngs::OsRng;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::errors::{CryptoError, CryptoResult};
use crate::hash::blake3_hash_many;

/// ElGamal şifreli metin: (C1, C2) çifti.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElGamalCiphertext {
    /// C1 = r·G (compressed Ristretto, 32 byte).
    pub c1: [u8; 32],
    /// C2 = key·G + r·pk (compressed Ristretto, 32 byte).
    pub c2: [u8; 32],
}

/// Threshold public key (Ristretto noktası, 32 byte).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ThresholdPublicKey([u8; 32]);

impl ThresholdPublicKey {
    /// Byte'lardan oluştur.
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
    /// Ham byte'lar.
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}

/// Tek bir validator'ın secret key share'i.
#[derive(Clone)]
pub struct SecretKeyShare {
    /// Share index (1-indexed).
    pub index: u8,
    /// Share değeri (Scalar).
    pub value: Scalar,
}

/// Tek bir validator'ın decryption share'i.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptionShare {
    /// Hangi validator'dan (1-indexed).
    pub index: u8,
    /// D_i = sk_share_i · C1 (compressed Ristretto, 32 byte).
    pub d_i: [u8; 32],
}

/// Threshold ElGamal hibrit şifreleme — AES anahtarı üret ve şifrele.
///
/// ## BUG-1 DÜZELTMESİ
/// Eski implementasyon `plaintext_key` parametresi alıyor, EC nokta olarak
/// encode edip şifreliyordu. Ancak decrypt tarafı `blake3(key_point)` ile
/// AES anahtarını türettiği için roundtrip kırıktı.
///
/// ## Yeni Tasarım (ECIES benzeri)
/// 1. Rastgele `key_scalar` üret → `key_point = key_scalar * G`
/// 2. `aes_key = BLAKE3(key_point.compress())` — symmetric key türet
/// 3. ElGamal: `C1 = r*G`, `C2 = key_point + r*pk`
/// 4. Caller: `aes_key` ile TX'i AES-GCM şifreler
/// 5. Decrypt: `key_point` recover → `BLAKE3(key_point)` = aynı `aes_key`
///
/// ## Dönüş Değeri
/// `(ElGamalCiphertext, [u8; 32])` — (şifreli nokta, AES anahtarı)
/// Caller bu AES anahtarını TX şifrelemek için kullanır.
pub fn elgamal_encrypt_key(
    threshold_pk: &ThresholdPublicKey,
) -> CryptoResult<(ElGamalCiphertext, [u8; 32])> {
    let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

    // pk noktasını deserialize et
    use curve25519_dalek::ristretto::CompressedRistretto;
    let pk_point = CompressedRistretto::from_slice(threshold_pk.as_bytes())
        .map_err(|_| CryptoError::ThresholdEncrypt("pk decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| CryptoError::ThresholdEncrypt("pk geçersiz nokta".into()))?;

    // Rastgele EC scalar — simetrik anahtarın kaynağı
    let key_scalar = Scalar::random(&mut OsRng);
    let key_point = key_scalar * g;

    // AES anahtarını EC noktasından türet — decrypt ile AYNI yöntem
    // BLAKE3(key_point.compress()) → 32 byte symmetric key
    let aes_key: [u8; 32] = blake3_hash_many(&[key_point.compress().as_bytes()])
        .as_bytes()
        .to_owned()
        .try_into()
        .expect("32 byte");

    // r: ephemeral scalar (ElGamal randomness)
    let r = Scalar::random(&mut OsRng);

    // C1 = r·G
    let c1_point = r * g;

    // C2 = key_point + r·pk
    let c2_point = key_point + r * pk_point;

    Ok((
        ElGamalCiphertext {
            c1: c1_point.compress().to_bytes(),
            c2: c2_point.compress().to_bytes(),
        },
        aes_key,
    ))
}

/// Tek bir validator'ın decryption share'ini hesapla.
pub fn compute_decryption_share(
    share: &SecretKeyShare,
    ciphertext: &ElGamalCiphertext,
) -> CryptoResult<DecryptionShare> {
    use curve25519_dalek::ristretto::CompressedRistretto;
    let c1_point = CompressedRistretto::from_slice(&ciphertext.c1)
        .map_err(|_| CryptoError::ThresholdDecrypt("C1 decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| CryptoError::ThresholdDecrypt("C1 geçersiz nokta".into()))?;

    // D_i = sk_share_i · C1
    let d_i_point = share.value * c1_point;

    Ok(DecryptionShare {
        index: share.index,
        d_i: d_i_point.compress().to_bytes(),
    })
}

/// t share ile Lagrange interpolasyonu → decryption.
///
/// `shares`: En az t adet DecryptionShare
/// `ciphertext`: Şifreli metin
/// `t`: Threshold
pub fn combine_decryption_shares(
    shares: &[DecryptionShare],
    ciphertext: &ElGamalCiphertext,
    t: u8,
) -> CryptoResult<RistrettoPoint> {
    use curve25519_dalek::ristretto::CompressedRistretto;

    if shares.len() < t as usize {
        return Err(CryptoError::ThresholdDecrypt(
            format!("yetersiz share: {} < {}", shares.len(), t)
        ));
    }

    // Sadece ilk t share'i kullan
    let shares = &shares[..t as usize];

    // Her D_i noktasını deserialize et
    let d_points: Vec<RistrettoPoint> = shares
        .iter()
        .map(|s| {
            CompressedRistretto::from_slice(&s.d_i)
                .map_err(|_| CryptoError::ThresholdDecrypt("D_i decompress hatası".into()))?
                .decompress()
                .ok_or_else(|| CryptoError::ThresholdDecrypt("D_i geçersiz nokta".into()))
        })
        .collect::<CryptoResult<_>>()?;

    // Lagrange interpolasyon katsayıları
    let indices: Vec<Scalar> = shares
        .iter()
        .map(|s| Scalar::from(s.index as u64))
        .collect();

    let mut secret_point = RistrettoPoint::default();
    for (i, (d_i, &x_i)) in d_points.iter().zip(indices.iter()).enumerate() {
        let mut lambda = Scalar::ONE;
        for (j, &x_j) in indices.iter().enumerate() {
            if i != j {
                // lambda_i = Π_{j≠i} x_j / (x_j - x_i)
                let diff = x_j - x_i;
                lambda *= x_j * diff.invert();
            }
        }
        secret_point += lambda * d_i;
    }

    // C2 - Σ(λ_i · D_i) = key_point
    let c2_point = CompressedRistretto::from_slice(&ciphertext.c2)
        .map_err(|_| CryptoError::ThresholdDecrypt("C2 decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| CryptoError::ThresholdDecrypt("C2 geçersiz nokta".into()))?;

    Ok(c2_point - secret_point)
}
