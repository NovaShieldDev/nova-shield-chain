//! Dilithium3 post-quantum dijital imza.
//!
//! §6.1.2 — Kuantum bilgisayar saldırılarına karşı güvenlik katmanı.
//! NIST PQC standartı (ML-DSA / Dilithium3 Level 3).
//!
//! ## Parametre Seçimi
//! - Dilithium3 (NIST Level 3): 128-bit kuantum güvenlik
//! - Public key: 1952 byte
//! - Secret key: 4000 byte
//! - İmza: 3293 byte (detached)

use pqcrypto_dilithium::dilithium3::{
    self,
    PublicKey as DilPublicKey,
    SecretKey as DilSecretKey,
};
use pqcrypto_traits::sign::{
    DetachedSignature, PublicKey as _, SecretKey as _, SignedMessage as _,
};
use zeroize::Zeroize;

use crate::errors::{CryptoError, CryptoResult};

/// Dilithium3 public key boyutu (byte).
pub const DILITHIUM3_PK_SIZE: usize = 1952;

/// Dilithium3 imza boyutu (byte, detached).
pub fn dilithium3_sig_size() -> usize {
    dilithium3::signature_bytes()
}

/// Dilithium3 key pair.
pub struct DilithiumKeypair {
    public_key: DilPublicKey,
    secret_key: DilSecretKey,
}

impl Zeroize for DilithiumKeypair {
    fn zeroize(&mut self) {
        let mut sk_bytes = self.secret_key.as_bytes().to_vec();
        sk_bytes.zeroize();
    }
}

impl Drop for DilithiumKeypair {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl DilithiumKeypair {
    /// Rastgele yeni Dilithium3 key pair üret.
    pub fn generate() -> CryptoResult<Self> {
        let (pk, sk) = dilithium3::keypair();
        Ok(Self { public_key: pk, secret_key: sk })
    }

    /// Public key byte'larını döndür (1952 byte).
    pub fn public_key_bytes(&self) -> &[u8] {
        self.public_key.as_bytes()
    }

    /// Mesajı detached imzala.
    /// Dönen imza boyutu = `dilithium3_sig_size()` byte.
    pub fn sign(&self, message: &[u8]) -> CryptoResult<Vec<u8>> {
        let sig = dilithium3::detached_sign(message, &self.secret_key);
        Ok(sig.as_bytes().to_vec())
    }

    /// Public key referansı.
    pub fn verifying_key(&self) -> &DilPublicKey {
        &self.public_key
    }
}

/// Dilithium3 imza doğrulama.
pub struct DilithiumVerifier;

impl DilithiumVerifier {
    /// Detached imzayı doğrula.
    ///
    /// - `message`: Orijinal mesaj
    /// - `signature`: Dilithium3 detached imzası
    /// - `public_key_bytes`: 1952 byte Dilithium3 public key
    pub fn verify(
        message: &[u8],
        signature: &[u8],
        public_key_bytes: &[u8],
    ) -> CryptoResult<()> {
        if public_key_bytes.len() != DILITHIUM3_PK_SIZE {
            return Err(CryptoError::InvalidKeySize {
                expected: DILITHIUM3_PK_SIZE,
                got: public_key_bytes.len(),
            });
        }

        let pk = DilPublicKey::from_bytes(public_key_bytes)
            .map_err(|e| CryptoError::DilithiumVerify(e.to_string()))?;

        // DetachedSignature'ı yeniden oluştur
        let det_sig = <dilithium3::DetachedSignature as DetachedSignature>::from_bytes(signature)
            .map_err(|e| CryptoError::DilithiumVerify(e.to_string()))?;

        dilithium3::verify_detached_signature(&det_sig, message, &pk)
            .map_err(|_| CryptoError::DilithiumVerify("doğrulama başarısız".into()))
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dilithium_keygen() {
        let kp = DilithiumKeypair::generate().unwrap();
        assert_eq!(kp.public_key_bytes().len(), DILITHIUM3_PK_SIZE);
    }

    #[test]
    fn test_dilithium_sig_size() {
        let size = dilithium3_sig_size();
        // Dilithium3 signature boyutu pqcrypto versiyonuna göre değişebilir
        // (standart: 3293 veya 3309 byte)
        assert!(size >= 3293 && size <= 3320,
            "Dilithium3 imza boyutu beklenen aralıkta olmalı, got={size}");
    }

    #[test]
    fn test_dilithium_sign_verify() {
        let kp = DilithiumKeypair::generate().unwrap();
        let message = b"Nova Shield post-quantum test message";

        let sig = kp.sign(message).unwrap();
        assert_eq!(sig.len(), dilithium3_sig_size());

        DilithiumVerifier::verify(message, &sig, kp.public_key_bytes())
            .expect("Dilithium doğrulama başarılı olmalı");
    }

    #[test]
    fn test_dilithium_tamper_detection() {
        let kp = DilithiumKeypair::generate().unwrap();
        let message = b"original";
        let sig = kp.sign(message).unwrap();

        let result = DilithiumVerifier::verify(b"tampered", &sig, kp.public_key_bytes());
        assert!(result.is_err(), "değiştirilmiş mesaj reddedilmeli");
    }

    #[test]
    fn test_dilithium_wrong_key() {
        let kp1 = DilithiumKeypair::generate().unwrap();
        let kp2 = DilithiumKeypair::generate().unwrap();
        let message = b"test";

        let sig = kp1.sign(message).unwrap();
        let result = DilithiumVerifier::verify(message, &sig, kp2.public_key_bytes());
        assert!(result.is_err(), "yanlış key ile doğrulama reddedilmeli");
    }
}
