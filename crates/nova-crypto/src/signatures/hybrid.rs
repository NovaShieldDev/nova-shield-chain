//! Hibrit ECDSA + Dilithium3 imza şeması.
//!
//! §6.1.2 — Post-quantum güvenlik katmanı.
//! Her TX iki imzaya sahiptir:
//! 1. secp256k1 ECDSA (65 byte) — klasik güvenlik + Ethereum uyumluluk
//! 2. Dilithium3 (3293 byte) — kuantum sonrası güvenlik
//!
//! ## Güvenlik Modeli
//! - Kuantum bilgisayar ECDSA'yı kırsa bile Dilithium3 korur.
//! - Klasik bilgisayar Dilithium3'ü kıramaz (kuantum olmadan).
//! - Her iki imza da doğrulanmalı → her ikisi de geçerli olmalı.
//!
//! ## Boyut
//! ECDSA(65) + Dilithium3(3293) = 3358 byte toplam imza.
//! Bu kabul edilebilir — TX boyutuna eklenir, konsensüs imzalarına değil.
//! Konsensüs: BLS aggregate (n validator → 96 byte).

use nova_primitives::crypto::{PublicKey, Signature, SignatureType};
use crate::errors::{CryptoError, CryptoResult};
use crate::signatures::{
    dilithium::{dilithium3_sig_size, DilithiumKeypair, DilithiumVerifier, DILITHIUM3_PK_SIZE},
    ecdsa::{EcdsaKeypair, EcdsaVerifier},
};
use nova_primitives::crypto::Hash;

/// Hibrit imza boyutu: ECDSA(65) + Dilithium3(3293) = 3358 byte.
pub fn hybrid_sig_size() -> usize { 65 + dilithium3_sig_size() }

/// Hibrit public key boyutu: secp256k1(33) + Dilithium3(1952) = 1985 byte.
pub const HYBRID_PK_SIZE: usize = 33 + DILITHIUM3_PK_SIZE;

/// Hibrit key pair: ECDSA + Dilithium3.
pub struct HybridKeypair {
    ecdsa: EcdsaKeypair,
    dilithium: DilithiumKeypair,
}

impl HybridKeypair {
    /// Yeni hibrit key pair üret.
    pub fn generate() -> CryptoResult<Self> {
        Ok(Self {
            ecdsa: EcdsaKeypair::generate()?,
            dilithium: DilithiumKeypair::generate()?,
        })
    }

    /// ECDSA compressed public key (33 byte).
    pub fn ecdsa_public_key(&self) -> CryptoResult<PublicKey> {
        self.ecdsa.public_key()
    }

    /// Dilithium3 public key bytes (1952 byte).
    pub fn dilithium_public_key_bytes(&self) -> &[u8] {
        self.dilithium.public_key_bytes()
    }

    /// Hibrit public key bytes = ecdsa_pk(33) || dilithium_pk(1952).
    pub fn hybrid_public_key_bytes(&self) -> CryptoResult<Vec<u8>> {
        let ecdsa_pk = self.ecdsa.public_key()?;
        let mut bytes = Vec::with_capacity(HYBRID_PK_SIZE);
        bytes.extend_from_slice(ecdsa_pk.as_bytes());
        bytes.extend_from_slice(self.dilithium.public_key_bytes());
        Ok(bytes)
    }

    /// Hash'i hibrit imzala.
    /// Çıktı: ECDSA_sig(65) || Dilithium3_sig(3293) = 3358 byte.
    pub fn sign_hash(&self, hash: &Hash) -> CryptoResult<Signature> {
        let ecdsa_sig = self.ecdsa.sign_hash(hash)?;
        let dil_sig = self.dilithium.sign(hash.as_bytes())?;

        let mut bytes = Vec::with_capacity(hybrid_sig_size());
        bytes.extend_from_slice(ecdsa_sig.as_bytes());
        bytes.extend_from_slice(&dil_sig);

        Ok(Signature::new_hybrid(bytes))
    }

    /// Veriyi BLAKE3 hashleyip hibrit imzala.
    pub fn sign_data(&self, data: &[u8]) -> CryptoResult<Signature> {
        let hash = crate::hash::blake3_hash(data);
        self.sign_hash(&hash)
    }
}

/// Hibrit imza doğrulayıcı.
pub struct HybridVerifier;

impl HybridVerifier {
    /// Hibrit imzayı doğrula.
    ///
    /// Her iki imza da geçerli olmalı — birinin bile başarısız olması reddir.
    pub fn verify(
        hash: &Hash,
        signature: &Signature,
        ecdsa_pk: &PublicKey,
        dilithium_pk_bytes: &[u8],
    ) -> CryptoResult<()> {
        if signature.sig_type() != SignatureType::EcdsaDilithium {
            return Err(CryptoError::InvalidParameter(
                "hibrit imza tipi gerekli (EcdsaDilithium)".into(),
            ));
        }

        let dil_sig_size = dilithium3_sig_size();
        let total_expected = 65 + dil_sig_size;

        if signature.len() < total_expected {
            return Err(CryptoError::InvalidSignatureSize {
                expected: total_expected,
                got: signature.len(),
            });
        }

        let bytes = signature.as_bytes();
        let ecdsa_sig_bytes = &bytes[..65];
        let dil_sig_bytes = &bytes[65..65 + dil_sig_size];

        // ECDSA imzasını doğrula
        let ecdsa_sig = Signature::new_ecdsa(ecdsa_sig_bytes.to_vec());
        EcdsaVerifier::verify(hash, &ecdsa_sig, ecdsa_pk)
            .map_err(|e| CryptoError::EcdsaVerify(format!("hibrit ECDSA başarısız: {e}")))?;

        // Dilithium3 imzasını doğrula
        DilithiumVerifier::verify(hash.as_bytes(), dil_sig_bytes, dilithium_pk_bytes)
            .map_err(|e| CryptoError::DilithiumVerify(format!("hibrit Dilithium başarısız: {e}")))?;

        Ok(())
    }

    /// Hibrit public key bytes'tan ayrıştır: (ecdsa_pk, dilithium_pk).
    pub fn split_hybrid_pk(hybrid_pk: &[u8]) -> CryptoResult<(PublicKey, &[u8])> {
        if hybrid_pk.len() < HYBRID_PK_SIZE {
            return Err(CryptoError::InvalidKeySize {
                expected: HYBRID_PK_SIZE,
                got: hybrid_pk.len(),
            });
        }

        let ecdsa_pk = PublicKey::from_slice(&hybrid_pk[..33])
            .ok_or_else(|| CryptoError::InvalidParameter("ecdsa pk ayrıştırma hatası".into()))?;
        let dil_pk = &hybrid_pk[33..33 + DILITHIUM3_PK_SIZE];

        Ok((ecdsa_pk, dil_pk))
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::blake3_hash;

    #[test]
    fn test_hybrid_keygen() {
        let kp = HybridKeypair::generate().unwrap();
        let hybrid_pk = kp.hybrid_public_key_bytes().unwrap();
        assert_eq!(hybrid_pk.len(), HYBRID_PK_SIZE);
    }

    #[test]
    fn test_hybrid_sign_verify() {
        let kp = HybridKeypair::generate().unwrap();
        let hash = blake3_hash(b"Nova Shield hybrid signature test");

        let sig = kp.sign_hash(&hash).unwrap();
        assert_eq!(sig.len(), hybrid_sig_size());

        let ecdsa_pk = kp.ecdsa_public_key().unwrap();
        let dil_pk = kp.dilithium_public_key_bytes();

        HybridVerifier::verify(&hash, &sig, &ecdsa_pk, dil_pk)
            .expect("hibrit doğrulama başarılı olmalı");
    }

    #[test]
    fn test_hybrid_tamper_detection() {
        let kp = HybridKeypair::generate().unwrap();
        let hash1 = blake3_hash(b"original");
        let hash2 = blake3_hash(b"tampered");

        let sig = kp.sign_hash(&hash1).unwrap();
        let ecdsa_pk = kp.ecdsa_public_key().unwrap();
        let dil_pk = kp.dilithium_public_key_bytes();

        let result = HybridVerifier::verify(&hash2, &sig, &ecdsa_pk, dil_pk);
        assert!(result.is_err(), "değiştirilmiş hash reddedilmeli");
    }
}
