//! HKDF anahtar türetme fonksiyonu.
//!
//! Nova Shield Chain'de anahtar türetmesi için HKDF-SHA256 ve BLAKE3 derive_key kullanılır.
//! §6.1.3 — Tüm simetrik anahtarlar bu modülden türetilir.

use crate::errors::{CryptoError, CryptoResult};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

/// HKDF-SHA256 ile anahtar türet.
///
/// - `ikm`: Input Key Material (çıkartma aşaması girdisi)
/// - `salt`: Tuz (opsiyonel, None → HKDF varsayılan sıfır tuz)
/// - `info`: Bağlam bilgisi (domain separation)
/// - `output_len`: İstenen çıktı uzunluğu (byte)
pub fn hkdf_sha256(
    ikm: &[u8],
    salt: Option<&[u8]>,
    info: &[u8],
    output_len: usize,
) -> CryptoResult<Zeroizing<Vec<u8>>> {
    let hk = Hkdf::<Sha256>::new(salt, ikm);
    let mut okm = Zeroizing::new(vec![0u8; output_len]);
    hk.expand(info, okm.as_mut_slice())
        .map_err(|e| CryptoError::Kdf(e.to_string()))?;
    Ok(okm)
}

/// 32 byte AES/ChaCha anahtar türet.
pub fn derive_symmetric_key(
    ikm: &[u8],
    salt: Option<&[u8]>,
    context: &str,
) -> CryptoResult<Zeroizing<[u8; 32]>> {
    let derived = hkdf_sha256(ikm, salt, context.as_bytes(), 32)?;
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&derived);
    Ok(key)
}

/// 32 byte IV/nonce türet.
pub fn derive_nonce(ikm: &[u8], salt: Option<&[u8]>, context: &str) -> CryptoResult<[u8; 12]> {
    let derived = hkdf_sha256(ikm, salt, context.as_bytes(), 12)?;
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&derived);
    Ok(nonce)
}

/// BLAKE3 derive_key — daha hızlı alternatif.
/// Context belirleme için sabit string kullanılır.
pub fn blake3_derive_symmetric_key(context: &str, ikm: &[u8]) -> Zeroizing<[u8; 32]> {
    let result = crate::hash::blake3_derive_key(context, ikm);
    Zeroizing::new(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hkdf_deterministic() {
        let ikm = b"test-master-key-material";
        let k1 = hkdf_sha256(ikm, None, b"nova-shield-context", 32).unwrap();
        let k2 = hkdf_sha256(ikm, None, b"nova-shield-context", 32).unwrap();
        assert_eq!(k1.as_slice(), k2.as_slice(), "HKDF deterministik olmalı");
    }

    #[test]
    fn test_hkdf_different_contexts() {
        let ikm = b"same-ikm";
        let k1 = hkdf_sha256(ikm, None, b"context-1", 32).unwrap();
        let k2 = hkdf_sha256(ikm, None, b"context-2", 32).unwrap();
        assert_ne!(k1.as_slice(), k2.as_slice(), "farklı context → farklı anahtar");
    }

    #[test]
    fn test_derive_symmetric_key() {
        let key = derive_symmetric_key(b"master", None, "nova-encryption-key").unwrap();
        assert_eq!(key.len(), 32);
    }
}
