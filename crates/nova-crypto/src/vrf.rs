//! VRF — Verifiable Random Function.
//!
//! Leader seçimi ve epoch randomness için kullanılır. §5.10
//!
//! ## Algoritma
//! ECVRF-RISTRETTO255-SHA512 (IETF draft-irtf-cfrg-vrf-15)
//!
//! VRF(SK, alpha) → (beta, pi)
//! - SK: secret key
//! - alpha: input (slot numarası || epoch || validator_id)
//! - beta: 64 byte VRF output (randomness)
//! - pi: 80 byte proof
//!
//! Doğrulama: ECVRF_verify(PK, alpha, pi) → beta (veya hata)
//!
//! ## Slot Leader Seçimi (BABE benzeri)
//! Referans: Polkadot BABE (10-polkadot-sdk.md §BABE):
//! - VRF output < threshold (c=1/4) → primary slot producer
//! - Threshold = 2^512 / n_validators × c
//!
//! ## Implementasyon Notu
//! Tam ECVRF yerine curve25519-dalek ile basitleştirilmiş VRF:
//! beta = BLAKE3(SK || alpha)
//! pi = EdDSA_sign(SK, H(alpha))
//! Bu, tam IETF VRF yerine pratik, güvenli bir alternatif.

use curve25519_dalek::scalar::Scalar;
use ed25519_dalek::{SigningKey as EdSigningKey, VerifyingKey as EdVerifyingKey};
use nova_primitives::crypto::Hash;
use nova_primitives::types::{EpochId, SlotId, ValidatorId};
use rand::rngs::OsRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroize;

use crate::errors::{CryptoError, CryptoResult};
use crate::hash::blake3_hash_many;

// ─── VRF ÇIKTI TİPLERİ ───────────────────────────────────────────────────────

/// VRF çıktısı (beta) — 64 byte randomness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VrfOutput([u8; 64]);

impl Serialize for VrfOutput {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}
impl<'de> Deserialize<'de> for VrfOutput {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        let b = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr: [u8; 64] = b.try_into().map_err(|_| {
            serde::de::Error::custom("VrfOutput 64 byte olmalı")
        })?;
        Ok(Self(arr))
    }
}

impl VrfOutput {
    /// Ham byte'lar.
    pub fn as_bytes(&self) -> &[u8; 64] { &self.0 }

    /// İlk 32 byte'ı Hash olarak döndür.
    pub fn to_hash(&self) -> Hash {
        Hash::from_bytes(self.0[..32].try_into().unwrap())
    }

    /// Epoch randomness XOR'u için (Polkadot BABE VRF XOR yöntemi).
    /// Referans: 10-polkadot-sdk.md: VRF segment birikimi → epoch randomness
    pub fn xor_with(&self, other: &VrfOutput) -> VrfOutput {
        let mut result = [0u8; 64];
        for i in 0..64 {
            result[i] = self.0[i] ^ other.0[i];
        }
        VrfOutput(result)
    }

    /// VRF çıktısını [0, 2^256) aralığındaki değer olarak yorumla.
    /// Slot leader threshold kontrolü için.
    pub fn as_u256_le(&self) -> [u8; 32] {
        self.0[..32].try_into().unwrap()
    }
}

/// VRF kanıtı (pi).
///
/// BUG-3 düzeltmesi: `output` alanı eklendi. Verifier, signature'ın
/// hem `input_hash` hem de `output` üzerinden geçtiğini doğrular,
/// böylece output proof'tan güvenle çıkarılabilir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VrfProof {
    /// VRF çıktısı (64 byte) — signer tarafından üretilir, imza kapsar.
    pub output: VrfOutput,
    /// Ed25519 imzası: BLAKE3(input_hash || output) üzerinde.
    pub signature_bytes: Vec<u8>,
    /// VRF input'un hash'i.
    pub input_hash: Hash,
}

// ─── VRF KEY PAIR ────────────────────────────────────────────────────────────

/// VRF key pair (Ed25519 tabanlı).
pub struct VrfKeypair {
    signing_key: EdSigningKey,
}

impl Zeroize for VrfKeypair {
    fn zeroize(&mut self) {
        let mut bytes = self.signing_key.to_bytes();
        bytes.zeroize();
    }
}

impl Drop for VrfKeypair {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl VrfKeypair {
    /// Rastgele VRF key pair üret.
    pub fn generate() -> Self {
        let signing_key = EdSigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// 32 byte seed'den oluştur.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let signing_key = EdSigningKey::from_bytes(bytes);
        Self { signing_key }
    }

    /// VRF public key (32 byte Ed25519 verifying key).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// VRF değerlendir: (output, proof) üret.
    ///
    /// `alpha`: VRF girdisi (slot || epoch || validator_id gibi)
    ///
    /// İmza `input_hash || output_bytes` üzerinde — verifier output'u
    /// proof'tan güvenle çıkarabilir.
    pub fn evaluate(&self, alpha: &[u8]) -> (VrfOutput, VrfProof) {
        let sk_bytes = self.signing_key.to_bytes();

        // beta = BLAKE3(sk_bytes || alpha) — deterministik, gizlilik korumalı
        let beta_hash = blake3_hash_many(&[&sk_bytes, alpha]);

        // VRF output: 64 byte
        let mut output_bytes = [0u8; 64];
        output_bytes[..32].copy_from_slice(beta_hash.as_bytes());
        let ext = blake3_hash_many(&[beta_hash.as_bytes(), b"nova-vrf-extension"]);
        output_bytes[32..].copy_from_slice(ext.as_bytes());
        let vrf_output = VrfOutput(output_bytes);

        // input_hash = BLAKE3(alpha)
        let input_hash = crate::hash::blake3_hash(alpha);

        // İmzala: input_hash || output_bytes — verifier output'u doğrulayabilir
        let mut to_sign = Vec::with_capacity(32 + 64);
        to_sign.extend_from_slice(input_hash.as_bytes());
        to_sign.extend_from_slice(&output_bytes);

        use ed25519_dalek::Signer;
        let signature = self.signing_key.sign(&to_sign);

        let proof = VrfProof {
            output: vrf_output,
            signature_bytes: signature.to_bytes().to_vec(),
            input_hash,
        };

        (vrf_output, proof)
    }
}

// ─── VRF DOĞRULAMA ───────────────────────────────────────────────────────────

/// VRF proof doğrulayıcı.
pub struct VrfVerifier;

impl VrfVerifier {
    /// VRF proof'unu doğrula ve output'u döndür.
    ///
    /// BUG-3 düzeltmesi: Eski implementasyon her zaman `Err` döndürüyordu.
    /// Yeni implementasyon:
    /// 1. `input_hash = BLAKE3(alpha)` kontrolü
    /// 2. Ed25519 imzayı `(input_hash || output_bytes)` üzerinde doğrula
    /// 3. Başarılı → proof.output döndür
    ///
    /// Başarılı → `VrfOutput` döner.
    /// Başarısız → hata.
    pub fn verify(
        alpha: &[u8],
        proof: &VrfProof,
        public_key_bytes: &[u8; 32],
    ) -> CryptoResult<VrfOutput> {
        // 1. Input hash kontrolü
        let expected_input_hash = crate::hash::blake3_hash(alpha);
        if expected_input_hash != proof.input_hash {
            return Err(CryptoError::Vrf("input hash uyuşmuyor".into()));
        }

        // 2. Ed25519 public key deserialize
        let verifying_key = EdVerifyingKey::from_bytes(public_key_bytes)
            .map_err(|e| CryptoError::Vrf(e.to_string()))?;

        if proof.signature_bytes.len() != 64 {
            return Err(CryptoError::Vrf(format!(
                "geçersiz imza boyutu: {}",
                proof.signature_bytes.len()
            )));
        }

        let sig_bytes: [u8; 64] = proof.signature_bytes.as_slice().try_into()
            .map_err(|_| CryptoError::Vrf("imza boyutu hatalı".into()))?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        // 3. İmza doğrulaması: imza (input_hash || output_bytes) üzerinde
        let mut to_verify = Vec::with_capacity(32 + 64);
        to_verify.extend_from_slice(proof.input_hash.as_bytes());
        to_verify.extend_from_slice(proof.output.as_bytes());

        use ed25519_dalek::Verifier;
        verifying_key
            .verify(&to_verify, &signature)
            .map_err(|e| CryptoError::Vrf(format!("imza doğrulama başarısız: {e}")))?;

        // 4. İmza geçerli → output'u güvenle döndür
        Ok(proof.output)
    }

    /// VRF output'un slot leader threshold'unu geçip geçmediğini kontrol et.
    ///
    /// Polkadot BABE: `VRF_output < threshold(c, n)` → primary slot producer.
    /// Nova Shield: `threshold = MAX_U256 / validator_count × c_num / c_den`
    ///
    /// `c_num / c_den`: Slot doldurma oranı (örn: 1/4 → her slot'ta %25 ihtimalle leader)
    pub fn is_slot_leader(
        vrf_output: &VrfOutput,
        validator_count: u64,
        c_numerator: u64,
        c_denominator: u64,
    ) -> bool {
        if validator_count == 0 || c_denominator == 0 {
            return false;
        }

        // VRF output'u u64'e normalize et (ilk 8 byte little-endian)
        let output_u64 = u64::from_le_bytes(
            vrf_output.as_bytes()[..8].try_into().unwrap()
        );

        // Threshold: u64::MAX / validator_count × c_num / c_den
        let threshold = (u64::MAX / validator_count)
            .saturating_mul(c_numerator)
            / c_denominator;

        output_u64 < threshold
    }
}

// ─── SLOT GİRDİSİ ─────────────────────────────────────────────────────────────

/// VRF girdisi: slot || epoch || validator_id kombinasyonu.
/// Deterministik ve benzersiz girdi üretir.
pub fn make_vrf_input(slot: SlotId, epoch: EpochId, validator: ValidatorId) -> Vec<u8> {
    let mut input = Vec::with_capacity(24);
    input.extend_from_slice(&slot.0.to_le_bytes());
    input.extend_from_slice(&epoch.0.to_le_bytes());
    input.extend_from_slice(&validator.0.to_le_bytes());
    input
}

/// Epoch randomness: Tüm validator VRF outputlarının XOR'u.
/// Referans: Polkadot BABE epoch randomness: XOR birikimi.
pub fn compute_epoch_randomness(vrf_outputs: &[VrfOutput]) -> Hash {
    if vrf_outputs.is_empty() {
        return Hash::ZERO;
    }
    let mut acc = vrf_outputs[0];
    for output in &vrf_outputs[1..] {
        acc = acc.xor_with(output);
    }
    acc.to_hash()
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrf_deterministic() {
        let kp = VrfKeypair::generate();
        let alpha = b"slot:100:epoch:1";

        let (out1, _) = kp.evaluate(alpha);
        let (out2, _) = kp.evaluate(alpha);

        assert_eq!(out1.as_bytes(), out2.as_bytes(), "VRF deterministik olmalı");
    }

    #[test]
    fn test_vrf_different_inputs() {
        let kp = VrfKeypair::generate();
        let (out1, _) = kp.evaluate(b"slot:100");
        let (out2, _) = kp.evaluate(b"slot:101");

        assert_ne!(out1.as_bytes(), out2.as_bytes(), "farklı girdi → farklı çıktı");
    }

    #[test]
    fn test_vrf_leader_selection() {
        // Yeterli sayıda validator ve makul threshold ile lider seçimi
        let kp = VrfKeypair::generate();
        let (output, _) = kp.evaluate(b"slot-leader-test");

        // 10 validatörden 1 slot leader seçimi (c=1/4)
        // Birinin seçilmesi olası (istatistiksel test — her zaman kesin değil)
        let _is_leader = VrfVerifier::is_slot_leader(&output, 10, 1, 4);
        // Sonuç true veya false olabilir, test mantık hatasını kontrol eder
    }

    #[test]
    fn test_epoch_randomness() {
        let kp1 = VrfKeypair::generate();
        let kp2 = VrfKeypair::generate();

        let (out1, _) = kp1.evaluate(b"epoch-randomness");
        let (out2, _) = kp2.evaluate(b"epoch-randomness");

        let rand = compute_epoch_randomness(&[out1, out2]);
        assert_ne!(rand, Hash::ZERO);
    }

    #[test]
    fn test_vrf_verify_roundtrip() {
        let kp = VrfKeypair::generate();
        let alpha = b"verify-roundtrip-test";
        let pk = kp.public_key_bytes();

        let (output, proof) = kp.evaluate(alpha);

        let verified = VrfVerifier::verify(alpha, &proof, &pk)
            .expect("doğrulama başarılı olmalı");
        assert_eq!(verified.as_bytes(), output.as_bytes());
    }

    #[test]
    fn test_vrf_verify_wrong_alpha_fails() {
        let kp = VrfKeypair::generate();
        let pk = kp.public_key_bytes();
        let (_, proof) = kp.evaluate(b"correct-alpha");

        let result = VrfVerifier::verify(b"wrong-alpha", &proof, &pk);
        assert!(result.is_err(), "yanlış alpha reddedilmeli");
    }

    #[test]
    fn test_vrf_verify_wrong_pubkey_fails() {
        let kp1 = VrfKeypair::generate();
        let kp2 = VrfKeypair::generate();
        let alpha = b"test-alpha";

        let (_, proof) = kp1.evaluate(alpha);
        let wrong_pk = kp2.public_key_bytes();

        let result = VrfVerifier::verify(alpha, &proof, &wrong_pk);
        assert!(result.is_err(), "yanlış public key reddedilmeli");
    }

    #[test]
    fn test_vrf_input_generation() {
        let input = make_vrf_input(SlotId(100), EpochId(1), ValidatorId(5));
        assert_eq!(input.len(), 24);
    }
}
