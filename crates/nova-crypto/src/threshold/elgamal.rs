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
    /// NOVA-RED-CRYPTO-003: pk_share = value · G — DLEQ proof için gerekli.
    pub pub_key_share: [u8; 32],
}

impl SecretKeyShare {
    /// Secret key share'den oluştur; pub_key_share otomatik hesaplanır.
    pub fn new(index: u8, value: Scalar) -> Self {
        let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
        let pub_key_share = (value * g).compress().to_bytes();
        Self { index, value, pub_key_share }
    }
}

/// Tek bir validator'ın decryption share'i.
///
/// NOVA-RED-CRYPTO-003: Chaum-Pedersen DLEQ proof ile birlikte gelir.
/// Proof: DL(pk_share, G) == DL(D_i, C1) — aynı sk_share_i kullanıldı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptionShare {
    /// Hangi validator'dan (1-indexed).
    pub index: u8,
    /// D_i = sk_share_i · C1 (compressed Ristretto, 32 byte).
    pub d_i: [u8; 32],
    /// NOVA-RED-CRYPTO-003: pk_share_i = sk_share_i · G.
    pub pub_key_share: [u8; 32],
    /// NOVA-RED-CRYPTO-003: Chaum-Pedersen DLEQ challenge (e).
    pub dleq_e: [u8; 32],
    /// NOVA-RED-CRYPTO-003: Chaum-Pedersen DLEQ response (s = r + e·sk).
    pub dleq_s: [u8; 32],
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

/// Tek bir validator'ın decryption share'ini hesapla ve DLEQ proof üret.
///
/// NOVA-RED-CRYPTO-003: Chaum-Pedersen DLEQ proof ile D_i'nin doğru
/// hesaplandığını kanıtlar: aynı `sk_share_i` hem D_i=sk_i·C1 hem de
/// pk_share=sk_i·G için kullanıldı.
pub fn compute_decryption_share(
    share: &SecretKeyShare,
    ciphertext: &ElGamalCiphertext,
) -> CryptoResult<DecryptionShare> {
    use curve25519_dalek::ristretto::CompressedRistretto;

    let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

    let c1_point = CompressedRistretto::from_slice(&ciphertext.c1)
        .map_err(|_| CryptoError::ThresholdDecrypt("C1 decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| CryptoError::ThresholdDecrypt("C1 geçersiz nokta".into()))?;

    // D_i = sk_share_i · C1
    let d_i_point = share.value * c1_point;
    let d_i_bytes = d_i_point.compress().to_bytes();

    // ── Chaum-Pedersen DLEQ proof ─────────────────────────────────────────
    // Prover: x = sk_share_i, A = pk_share = x·G, B = D_i = x·C1
    // Kanıt: DL(A, G) == DL(B, C1) — aynı x kullanıldı
    //
    // 1. r ←$ random
    // 2. R1 = r·G, R2 = r·C1
    // 3. e = BLAKE3(G || C1 || A || B || R1 || R2)  (Fiat-Shamir)
    // 4. s = r + e·x
    // ──────────────────────────────────────────────────────────────────────
    let r = Scalar::random(&mut OsRng);
    let r1_point = r * g;
    let r2_point = r * c1_point;

    let e_hash = blake3_hash_many(&[
        g.compress().as_bytes(),
        c1_point.compress().as_bytes(),
        &share.pub_key_share,
        &d_i_bytes,
        r1_point.compress().as_bytes(),
        r2_point.compress().as_bytes(),
    ]);
    let e_bytes: [u8; 32] = e_hash.as_bytes().to_owned().try_into().expect("32 byte");
    let e_scalar = Scalar::from_bytes_mod_order(e_bytes);

    // s = r + e·sk
    let s_scalar = r + e_scalar * share.value;

    Ok(DecryptionShare {
        index: share.index,
        d_i: d_i_bytes,
        pub_key_share: share.pub_key_share,
        dleq_e: e_bytes,
        dleq_s: s_scalar.to_bytes(),
    })
}

/// t share ile Lagrange interpolasyonu → decryption.
///
/// NOVA-RED-CRYPTO-003: Her share'in Chaum-Pedersen DLEQ proof'u doğrulanır.
/// Geçersiz proof → `ThresholdDecrypt` hatası.
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

    let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

    let c1_point = CompressedRistretto::from_slice(&ciphertext.c1)
        .map_err(|_| CryptoError::ThresholdDecrypt("C1 decompress hatası".into()))?
        .decompress()
        .ok_or_else(|| CryptoError::ThresholdDecrypt("C1 geçersiz nokta".into()))?;

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

    // ── NOVA-RED-CRYPTO-003: Her share için DLEQ proof doğrula ────────────
    // Verify: s·G - e·pk_share == R1  ve  s·C1 - e·D_i == R2
    // Sonra Fiat-Shamir: e == BLAKE3(G || C1 || pk_share || D_i || R1 || R2)
    // ──────────────────────────────────────────────────────────────────────
    for (share, d_point) in shares.iter().zip(d_points.iter()) {
        let pk_share_point = CompressedRistretto::from_slice(&share.pub_key_share)
            .map_err(|_| CryptoError::ThresholdDecrypt(
                format!("share {} pk_share decompress hatası", share.index)
            ))?
            .decompress()
            .ok_or_else(|| CryptoError::ThresholdDecrypt(
                format!("share {} pk_share geçersiz nokta", share.index)
            ))?;

        let e_scalar = Scalar::from_bytes_mod_order(share.dleq_e);
        let s_scalar = Scalar::from_bytes_mod_order(share.dleq_s);

        // R1 = s·G - e·pk_share
        let r1_computed = s_scalar * g - e_scalar * pk_share_point;
        // R2 = s·C1 - e·D_i
        let r2_computed = s_scalar * c1_point - e_scalar * d_point;

        // Fiat-Shamir challenge yeniden hesapla
        let e_check_hash = blake3_hash_many(&[
            g.compress().as_bytes(),
            c1_point.compress().as_bytes(),
            &share.pub_key_share,
            &share.d_i,
            r1_computed.compress().as_bytes(),
            r2_computed.compress().as_bytes(),
        ]);
        let e_check: [u8; 32] = e_check_hash.as_bytes().to_owned().try_into().expect("32 byte");

        if e_check != share.dleq_e {
            return Err(CryptoError::ThresholdDecrypt(
                format!("share {} DLEQ proof geçersiz — sahte share reddedildi", share.index)
            ));
        }
    }

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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::scalar::Scalar;

    /// Basit 1-of-1 şifreleme + çözme roundtrip.
    #[test]
    fn test_elgamal_encrypt_decrypt_roundtrip() {
        let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

        // Rastgele anahtar çifti
        let sk = Scalar::random(&mut OsRng);
        let pk_point = sk * g;
        let threshold_pk = ThresholdPublicKey::from_bytes(pk_point.compress().to_bytes());

        // Şifrele
        let (ciphertext, aes_key) = elgamal_encrypt_key(&threshold_pk).unwrap();

        // SecretKeyShare oluştur
        let sk_share = SecretKeyShare::new(1, sk);

        // Decryption share (DLEQ proof ile)
        let dec_share = compute_decryption_share(&sk_share, &ciphertext).unwrap();

        // Tek share ile t=1 combine
        let key_point_recovered = combine_decryption_shares(&[dec_share], &ciphertext, 1).unwrap();

        // AES anahtarını recover et
        let aes_key_recovered: [u8; 32] = blake3_hash_many(&[key_point_recovered.compress().as_bytes()])
            .as_bytes()
            .to_owned()
            .try_into()
            .unwrap();

        assert_eq!(aes_key, aes_key_recovered, "AES anahtarı eşleşmeli");
    }

    /// NOVA-RED-CRYPTO-003: Sahte D_i gönderilirse DLEQ proof reddedilmeli.
    #[test]
    fn test_fake_decryption_share_rejected() {
        let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

        let sk = Scalar::random(&mut OsRng);
        let pk_point = sk * g;
        let threshold_pk = ThresholdPublicKey::from_bytes(pk_point.compress().to_bytes());

        let (ciphertext, _aes_key) = elgamal_encrypt_key(&threshold_pk).unwrap();

        let sk_share = SecretKeyShare::new(1, sk);
        let mut dec_share = compute_decryption_share(&sk_share, &ciphertext).unwrap();

        // Saldırgan D_i'yi geçerli ama yanlış bir nokta ile değiştiriyor
        // (geçersiz byte dizisi değil — geçerli EC noktası ama sk·C1 değil)
        let fake_d_i = (Scalar::random(&mut OsRng) * g).compress().to_bytes();
        dec_share.d_i = fake_d_i;

        let result = combine_decryption_shares(&[dec_share], &ciphertext, 1);
        assert!(result.is_err(), "Sahte D_i DLEQ proof doğrulamasını geçmemeli");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("DLEQ proof geçersiz"), "Hata mesajı DLEQ içermeli: {err_msg}");
    }

    /// NOVA-RED-CRYPTO-003: Sahte pub_key_share ile DLEQ proof reddedilmeli.
    #[test]
    fn test_fake_pub_key_share_rejected() {
        let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

        let sk = Scalar::random(&mut OsRng);
        let pk_point = sk * g;
        let threshold_pk = ThresholdPublicKey::from_bytes(pk_point.compress().to_bytes());

        let (ciphertext, _) = elgamal_encrypt_key(&threshold_pk).unwrap();

        let sk_share = SecretKeyShare::new(1, sk);
        let mut dec_share = compute_decryption_share(&sk_share, &ciphertext).unwrap();

        // Saldırgan pub_key_share'i değiştiriyor
        let fake_pk = (Scalar::random(&mut OsRng) * g).compress().to_bytes();
        dec_share.pub_key_share = fake_pk;

        let result = combine_decryption_shares(&[dec_share], &ciphertext, 1);
        assert!(result.is_err(), "Sahte pub_key_share DLEQ proof doğrulamasını geçmemeli");
    }

    /// NOVA-RED-CRYPTO-003: dleq_e/dleq_s değiştirilince reddedilmeli.
    #[test]
    fn test_tampered_dleq_proof_rejected() {
        let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

        let sk = Scalar::random(&mut OsRng);
        let pk_point = sk * g;
        let threshold_pk = ThresholdPublicKey::from_bytes(pk_point.compress().to_bytes());

        let (ciphertext, _) = elgamal_encrypt_key(&threshold_pk).unwrap();

        let sk_share = SecretKeyShare::new(1, sk);
        let mut dec_share = compute_decryption_share(&sk_share, &ciphertext).unwrap();

        // Proof response'u bozuyoruz
        dec_share.dleq_s[0] ^= 0xFF;

        let result = combine_decryption_shares(&[dec_share], &ciphertext, 1);
        assert!(result.is_err(), "Bozuk DLEQ proof reddedilmeli");
    }

    /// SecretKeyShare::new pub_key_share'i doğru hesaplamalı.
    #[test]
    fn test_secret_key_share_pub_key_consistency() {
        let g = curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
        let sk = Scalar::random(&mut OsRng);
        let expected_pk = (sk * g).compress().to_bytes();

        let share = SecretKeyShare::new(1, sk);
        assert_eq!(share.pub_key_share, expected_pk);
    }
}
