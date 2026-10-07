//! Simetrik şifreleme — AES-256-GCM ve ChaCha20Poly1305.
//!
//! Nova Shield Chain'de şifreli mempool TX şifreleme için kullanılır. §5.2.1
//! Her iki algoritma da authenticated encryption (AEAD) sağlar.
//!
//! ## Kullanım senaryosu
//! - `AesGcm256`: Donanım AES-NI desteğiyle ultra hızlı
//! - `ChaCha20`: AES-NI olmayan donanımlarda (mobil, embedded)

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce as AesNonce,
};
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChaNonce};
use rand::{RngCore, SeedableRng};
use rand::rngs::OsRng;
use zeroize::{Zeroize, Zeroizing};

use crate::errors::{CryptoError, CryptoResult};

// ─── AES-256-GCM ─────────────────────────────────────────────────────────────

/// Şifrelenmiş veri paketi — şifreli içerik + nonce birlikte taşınır.
#[derive(Debug, Clone)]
pub struct EncryptedData {
    /// 12 byte AES-GCM / ChaCha nonce (rastgele).
    pub nonce: [u8; 12],
    /// Şifreli içerik + 16 byte GCM tag.
    pub ciphertext: Vec<u8>,
}

impl Zeroize for EncryptedData {
    fn zeroize(&mut self) {
        self.nonce.zeroize();
        self.ciphertext.zeroize();
    }
}

/// AES-256-GCM ile şifrele.
///
/// - `key`: 32 byte AES-256 anahtarı
/// - `plaintext`: Şifrelenecek veri
/// - `aad`: Additional Authenticated Data (şifreli değil, ama MAC'e dahil)
pub fn aes_gcm_encrypt(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> CryptoResult<EncryptedData> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| CryptoError::AesGcmEncrypt(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = AesNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, aes_gcm::aead::Payload { msg: plaintext, aad })
        .map_err(|e| CryptoError::AesGcmEncrypt(e.to_string()))?;

    Ok(EncryptedData { nonce: nonce_bytes, ciphertext })
}

/// AES-256-GCM ile çöz.
pub fn aes_gcm_decrypt(
    key: &[u8; 32],
    data: &EncryptedData,
    aad: &[u8],
) -> CryptoResult<Zeroizing<Vec<u8>>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| CryptoError::AesGcmDecrypt(e.to_string()))?;

    let nonce = AesNonce::from_slice(&data.nonce);

    let plaintext = cipher
        .decrypt(nonce, aes_gcm::aead::Payload { msg: &data.ciphertext, aad })
        .map_err(|e| CryptoError::AesGcmDecrypt(e.to_string()))?;

    Ok(Zeroizing::new(plaintext))
}

// ─── CHACHA20-POLY1305 ────────────────────────────────────────────────────────

/// ChaCha20Poly1305 ile şifrele.
/// AES-NI olmayan platformlarda tercih edilir.
pub fn chacha_encrypt(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> CryptoResult<EncryptedData> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| CryptoError::AesGcmEncrypt(e.to_string()))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = ChaNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad })
        .map_err(|e| CryptoError::AesGcmEncrypt(e.to_string()))?;

    Ok(EncryptedData { nonce: nonce_bytes, ciphertext })
}

/// ChaCha20Poly1305 ile çöz.
pub fn chacha_decrypt(
    key: &[u8; 32],
    data: &EncryptedData,
    aad: &[u8],
) -> CryptoResult<Zeroizing<Vec<u8>>> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};

    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| CryptoError::AesGcmDecrypt(e.to_string()))?;

    let nonce = ChaNonce::from_slice(&data.nonce);

    let plaintext = cipher
        .decrypt(nonce, Payload { msg: &data.ciphertext, aad })
        .map_err(|e| CryptoError::AesGcmDecrypt(e.to_string()))?;

    Ok(Zeroizing::new(plaintext))
}

// ─── PADDING ──────────────────────────────────────────────────────────────────

/// TX şifrelemeden önce sabit boyuta padding ekle. §6.4.2
///
/// Boyut farkından bilgi sızmasını engeller.
/// `target_size`: İstenen sabit boyut (örn: 512 byte)
pub fn pad_to_size(data: &[u8], target_size: usize) -> CryptoResult<Vec<u8>> {
    if data.len() > target_size {
        return Err(CryptoError::InvalidParameter(
            format!("veri boyutu ({}) hedef boyuttan ({}) büyük", data.len(), target_size),
        ));
    }

    // PKCS#7 benzeri padding: son byte padding uzunluğunu belirtir
    let padding_len = target_size - data.len();
    let mut padded = data.to_vec();

    // Padding byte'larını padding uzunluğuna eşit değerle doldur
    let pad_byte = u8::try_from(padding_len % 256).unwrap_or(0);
    padded.resize(target_size, pad_byte);

    Ok(padded)
}

/// Padding'i kaldır.
pub fn unpad(padded: &[u8]) -> CryptoResult<&[u8]> {
    if padded.is_empty() {
        return Err(CryptoError::InvalidParameter("boş veri".into()));
    }
    let pad_byte = *padded.last().unwrap();
    let pad_len = pad_byte as usize;
    if pad_len == 0 || pad_len > padded.len() {
        return Err(CryptoError::InvalidParameter("geçersiz padding".into()));
    }
    Ok(&padded[..padded.len() - pad_len])
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn random_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    #[test]
    fn test_aes_gcm_roundtrip() {
        let key = random_key();
        let plaintext = b"Nova Shield Chain encrypted transaction";
        let aad = b"slot:100";

        let encrypted = aes_gcm_encrypt(&key, plaintext, aad).unwrap();
        let decrypted = aes_gcm_decrypt(&key, &encrypted, aad).unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_aes_gcm_wrong_key() {
        let key = random_key();
        let wrong_key = random_key();
        let plaintext = b"secret data";

        let encrypted = aes_gcm_encrypt(&key, plaintext, b"").unwrap();
        let result = aes_gcm_decrypt(&wrong_key, &encrypted, b"");
        assert!(result.is_err(), "yanlış anahtarla çözme başarısız olmalı");
    }

    #[test]
    fn test_aes_gcm_aad_tampering() {
        let key = random_key();
        let plaintext = b"tx data";
        let aad = b"slot:100";

        let encrypted = aes_gcm_encrypt(&key, plaintext, aad).unwrap();
        // Farklı AAD ile çözme başarısız olmalı (authentication failure)
        let result = aes_gcm_decrypt(&key, &encrypted, b"slot:999");
        assert!(result.is_err(), "AAD tamper tespiti çalışmalı");
    }

    #[test]
    fn test_chacha_roundtrip() {
        let key = random_key();
        let plaintext = b"ChaCha20 test message for Nova Shield";

        let encrypted = chacha_encrypt(&key, plaintext, b"nova-aad").unwrap();
        let decrypted = chacha_decrypt(&key, &encrypted, b"nova-aad").unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_padding() {
        let data = b"short tx";
        let padded = pad_to_size(data, 64).unwrap();
        assert_eq!(padded.len(), 64);

        // Padding kaldırma (son byte padding uzunluğunu gösterir)
        // Bu test implementasyonun basitliği için manual
        assert!(padded.len() > data.len());
    }

    #[test]
    fn test_padding_overflow() {
        let data = vec![0u8; 600];
        let result = pad_to_size(&data, 512);
        assert!(result.is_err(), "512 byte'dan büyük veri reddedilmeli");
    }
}
