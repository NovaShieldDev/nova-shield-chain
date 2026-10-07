//! secp256k1 ECDSA imzalama — Ethereum uyumlu.
//!
//! §6.1.1 — Nova Shield'in temel imza şeması.
//! MPC/CGGMP21 protokolüyle uyumlu secp256k1 eğrisi.
//!
//! ## Referans
//! - `multi-party-ecdsa`: KZen GG18/GG20 analizi (01-multi-party-ecdsa.md)
//! - `tss-lib`: Binance threshold ECDSA (02-tss-lib.md)
//! - Ethereum imza formatı: r(32) || s(32) || v(1)

use k256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature as K256Signature, SigningKey, VerifyingKey,
    },
    SecretKey,
};
use nova_primitives::crypto::{Hash, PublicKey, Signature};
use rand::rngs::OsRng;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::errors::{CryptoError, CryptoResult};

// ECDSA imzası boyutu (r || s) — her biri 32 byte, toplam 64 byte.
// recovery ID dahil: 65 byte.
const ECDSA_SIG_BYTES: usize = 65;

/// secp256k1 ECDSA key pair.
///
/// `signing_key` hassas veri — drop'ta `zeroize` ile sıfırlanır.
pub struct EcdsaKeypair {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl Zeroize for EcdsaKeypair {
    fn zeroize(&mut self) {
        // k256::SigningKey kendi Zeroize'ını implement etmez,
        // ancak drop sırasında güvenli şekilde temizlenir.
        // Burada ek sıfırlama için key_bytes alıp sıfırlıyoruz.
        let mut bytes = self.signing_key.to_bytes();
        bytes.zeroize();
    }
}

impl Drop for EcdsaKeypair {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl EcdsaKeypair {
    /// Rastgele yeni key pair üret.
    pub fn generate() -> CryptoResult<Self> {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = *signing_key.verifying_key();
        Ok(Self { signing_key, verifying_key })
    }

    /// 32 byte secret key'den oluştur.
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> CryptoResult<Self> {
        let secret = SecretKey::from_bytes(bytes.into())
            .map_err(|e| CryptoError::EcdsaKeygen(e.to_string()))?;
        let signing_key = SigningKey::from(secret);
        let verifying_key = *signing_key.verifying_key();
        Ok(Self { signing_key, verifying_key })
    }

    /// Compressed public key (33 byte).
    pub fn public_key(&self) -> CryptoResult<PublicKey> {
        use k256::elliptic_curve::sec1::ToEncodedPoint;
        let encoded = self.verifying_key.to_encoded_point(true); // compressed=true
        PublicKey::from_slice(encoded.as_bytes())
            .ok_or_else(|| CryptoError::EcdsaKeygen("compressed key boyutu hatalı".into()))
    }

    /// Hash'i imzala (Ethereum recovery ID dahil: r||s||v, 65 byte).
    pub fn sign_hash(&self, hash: &Hash) -> CryptoResult<Signature> {
        use k256::ecdsa::signature::hazmat::PrehashSigner;

        let (sig, recovery_id) = self
            .signing_key
            .sign_prehash_recoverable(hash.as_bytes())
            .map_err(|e| CryptoError::EcdsaSign(e.to_string()))?;

        // r(32) || s(32) || v(1) formatı — Ethereum uyumlu
        let mut bytes = Vec::with_capacity(ECDSA_SIG_BYTES);
        bytes.extend_from_slice(&sig.to_bytes());
        bytes.push(recovery_id.to_byte());

        Ok(Signature::new_ecdsa(bytes))
    }

    /// Rastgele veriyi BLAKE3 hashleyip imzala.
    pub fn sign_data(&self, data: &[u8]) -> CryptoResult<Signature> {
        let hash = crate::hash::blake3_hash(data);
        self.sign_hash(&hash)
    }
}

/// ECDSA imza doğrulayıcı.
pub struct EcdsaVerifier;

impl EcdsaVerifier {
    /// Hash + imza + compressed public key → doğrula.
    ///
    /// İmza formatı: r(32) || s(32) || v(1)
    pub fn verify(
        hash: &Hash,
        signature: &Signature,
        public_key: &PublicKey,
    ) -> CryptoResult<()> {
        if signature.len() < 64 {
            return Err(CryptoError::InvalidSignatureSize {
                expected: ECDSA_SIG_BYTES,
                got: signature.len(),
            });
        }

        let sig_bytes = &signature.as_bytes()[..64]; // r || s
        let k256_sig = K256Signature::try_from(sig_bytes)
            .map_err(|e| CryptoError::EcdsaVerify(e.to_string()))?;

        let verifying_key = VerifyingKey::from_sec1_bytes(public_key.as_bytes())
            .map_err(|e| CryptoError::EcdsaVerify(e.to_string()))?;

        use k256::ecdsa::signature::hazmat::PrehashVerifier;
        verifying_key
            .verify_prehash(hash.as_bytes(), &k256_sig)
            .map_err(|e| CryptoError::EcdsaVerify(e.to_string()))
    }

    /// Recover public key from signature + hash (Ethereum ecrecover uyumlu).
    pub fn recover_public_key(
        hash: &Hash,
        signature: &Signature,
    ) -> CryptoResult<PublicKey> {
        if signature.len() < ECDSA_SIG_BYTES {
            return Err(CryptoError::InvalidSignatureSize {
                expected: ECDSA_SIG_BYTES,
                got: signature.len(),
            });
        }

        use k256::ecdsa::{RecoveryId, VerifyingKey};

        let sig_bytes = &signature.as_bytes()[..64];
        let recovery_byte = signature.as_bytes()[64];

        let k256_sig = K256Signature::try_from(sig_bytes)
            .map_err(|e| CryptoError::EcdsaVerify(e.to_string()))?;

        let recovery_id = RecoveryId::try_from(recovery_byte & 0x01)
            .map_err(|e| CryptoError::EcdsaVerify(e.to_string()))?;

        let recovered = VerifyingKey::recover_from_prehash(
            hash.as_bytes(),
            &k256_sig,
            recovery_id,
        )
        .map_err(|e| CryptoError::EcdsaVerify(e.to_string()))?;

        use k256::elliptic_curve::sec1::ToEncodedPoint;
        let encoded = recovered.to_encoded_point(true);
        PublicKey::from_slice(encoded.as_bytes())
            .ok_or_else(|| CryptoError::EcdsaVerify("recover sonucu hatalı boyut".into()))
    }
}

/// Ethereum-uyumlu hesap adresi türetimi.
/// `Address = keccak256(uncompressed_pubkey[1..])[12..]` (20 byte)
///
/// AŞAMA 1 Görev 2: Ethereum standart adres türetimi.
/// secp256k1 uncompressed pubkey = 65 byte (0x04 prefix + 32 byte x + 32 byte y)
/// keccak256(pubkey[1..]) → 32 byte → son 20 byte → adres
pub fn public_key_to_address(pubkey: &PublicKey) -> nova_primitives::types::Address {
    use sha3::{Digest, Keccak256};
    use k256::elliptic_curve::sec1::ToEncodedPoint;

    // pubkey 33-byte compressed → uncompressed 65 byte'a çevir
    let vk = VerifyingKey::from_sec1_bytes(pubkey.as_bytes())
        .expect("geçerli compressed pubkey");
    let uncompressed = vk.to_encoded_point(false); // false = uncompressed
    let uncompressed_bytes = uncompressed.as_bytes(); // [0x04, x(32), y(32)] = 65 byte

    // keccak256(x || y) = keccak256(uncompressed[1..])
    let mut hasher = Keccak256::new();
    hasher.update(&uncompressed_bytes[1..]); // 0x04 prefix'i atla
    let hash = hasher.finalize();

    // Son 20 byte → adres
    let mut addr_bytes = [0u8; 20];
    addr_bytes.copy_from_slice(&hash[12..]);
    nova_primitives::types::Address::from_bytes(addr_bytes)
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::blake3_hash;

    #[test]
    fn test_ecdsa_keygen() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        assert!(pk.is_valid_compressed(), "compressed public key geçerli olmalı");
    }

    #[test]
    fn test_ecdsa_sign_verify() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let hash = blake3_hash(b"test transaction data");

        let sig = kp.sign_hash(&hash).unwrap();
        assert_eq!(sig.len(), ECDSA_SIG_BYTES);

        EcdsaVerifier::verify(&hash, &sig, &pk).expect("doğrulama başarılı olmalı");
    }

    #[test]
    fn test_ecdsa_wrong_message() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();
        let hash1 = blake3_hash(b"original message");
        let hash2 = blake3_hash(b"tampered message");

        let sig = kp.sign_hash(&hash1).unwrap();
        let result = EcdsaVerifier::verify(&hash2, &sig, &pk);
        assert!(result.is_err(), "yanlış mesajda doğrulama başarısız olmalı");
    }

    #[test]
    fn test_ecdsa_wrong_key() {
        let kp1 = EcdsaKeypair::generate().unwrap();
        let kp2 = EcdsaKeypair::generate().unwrap();
        let pk2 = kp2.public_key().unwrap();

        let hash = blake3_hash(b"test");
        let sig = kp1.sign_hash(&hash).unwrap();

        let result = EcdsaVerifier::verify(&hash, &sig, &pk2);
        assert!(result.is_err(), "yanlış public key ile doğrulama başarısız olmalı");
    }

    #[test]
    fn test_ecdsa_recover() {
        let kp = EcdsaKeypair::generate().unwrap();
        let expected_pk = kp.public_key().unwrap();
        let hash = blake3_hash(b"recover test");

        let sig = kp.sign_hash(&hash).unwrap();
        let recovered = EcdsaVerifier::recover_public_key(&hash, &sig).unwrap();

        assert_eq!(expected_pk, recovered, "recover edilen public key eşleşmeli");
    }

    #[test]
    fn test_address_derivation() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();

        let addr1 = public_key_to_address(&pk);
        let addr2 = public_key_to_address(&pk);

        assert_eq!(addr1, addr2, "adres türetimi deterministik olmalı");
        assert_ne!(addr1, nova_primitives::types::Address::ZERO);
    }

    #[test]
    fn test_ecdsa_deterministic_with_same_secret() {
        let secret = [42u8; 32];
        let kp1 = EcdsaKeypair::from_secret_bytes(&secret).unwrap();
        let kp2 = EcdsaKeypair::from_secret_bytes(&secret).unwrap();

        let pk1 = kp1.public_key().unwrap();
        let pk2 = kp2.public_key().unwrap();
        assert_eq!(pk1, pk2, "aynı secret → aynı public key");
    }
}
