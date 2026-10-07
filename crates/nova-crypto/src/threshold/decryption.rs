//! Threshold decryption koordinasyonu.
//!
//! §5.2.2 — Slot kapanınca validator'lar decryption share'lerini yayınlar,
//! koordinatör share'leri birleştirerek TX'leri açar.

use crate::errors::{CryptoError, CryptoResult};
use crate::symmetric::{aes_gcm_decrypt, EncryptedData};
use super::elgamal::{
    combine_decryption_shares, compute_decryption_share, DecryptionShare,
    ElGamalCiphertext, SecretKeyShare,
};

/// Şifreli TX paketi: ElGamal ile şifreli AES anahtarı + AES-GCM şifreli TX.
#[derive(Debug, Clone)]
pub struct EncryptedTxPacket {
    /// AES anahtarının ElGamal şifreli hali.
    pub key_ciphertext: ElGamalCiphertext,
    /// AES-GCM ile şifreli TX içeriği.
    pub tx_ciphertext: EncryptedData,
    /// AAD (slot numarası gibi doğrulama bilgisi).
    pub aad: Vec<u8>,
}

/// Validator'ın kendi decryption share'ini hesapla.
pub fn produce_decryption_share(
    secret_share: &SecretKeyShare,
    packet: &EncryptedTxPacket,
) -> CryptoResult<DecryptionShare> {
    compute_decryption_share(secret_share, &packet.key_ciphertext)
}

/// t adet share ile TX'i çöz.
///
/// 1. Lagrange ile ElGamal çöz → key_point
/// 2. key_point'ten AES anahtarını recover et
/// 3. AES-GCM ile TX içeriğini çöz
pub fn threshold_decrypt(
    shares: &[DecryptionShare],
    packet: &EncryptedTxPacket,
    t: u8,
) -> CryptoResult<Vec<u8>> {
    if shares.len() < t as usize {
        return Err(CryptoError::ThresholdDecrypt(
            format!("yetersiz share: {} < {}", shares.len(), t),
        ));
    }

    // ElGamal combine → key_point (m·G)
    let key_point = combine_decryption_shares(shares, &packet.key_ciphertext, t)?;

    // key_point'ten AES anahtarını türet
    // Not: Tam implementasyonda BSGS (Baby-step Giant-step) kullanılır.
    // Burada key_point'i BLAKE3 ile hash'leyip AES anahtarı olarak kullanıyoruz.
    // Bu "encode then decrypt" yerine "hash-based key agreement" yaklaşımı.
    let key_bytes: [u8; 32] = crate::hash::blake3_hash(key_point.compress().as_bytes())
        .as_bytes()
        .to_owned()
        .try_into()
        .expect("32 byte");

    // AES-GCM çöz
    let plaintext = aes_gcm_decrypt(&key_bytes, &packet.tx_ciphertext, &packet.aad)?;
    Ok(plaintext.to_vec())
}
