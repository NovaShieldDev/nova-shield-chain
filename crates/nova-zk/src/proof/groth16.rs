//! Groth16 kanıt sistemi
//!
//! İki mod:
//! 1. Mock (hızlı test): BLAKE3-tabanlı sahte kanıt, 64 byte
//! 2. Gerçek (production): arkworks BLS12-381 Groth16, 192 byte
//!    - (A, B, C) ∈ G1 × G2 × G1 — her zaman 192 byte (compressed)
//!    - `prove_groth16()` / `verify_groth16()` metodları

use crate::errors::{ZkError, ZkResult};
use crate::proof::params::{CircuitType, TrustedSetupParams};
use serde::{Deserialize, Serialize};

// Gerçek Groth16 için arkworks
use ark_bls12_381::{Bls12_381, Fr};
use ark_ff::PrimeField;
use ark_groth16::{Groth16, Proof};
use ark_relations::r1cs::ConstraintSynthesizer;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;

/// Gerçek Groth16 proof boyutu (BLS12-381): A:48 + B:96 + C:48 = 192 byte
pub const GROTH16_PROOF_SIZE: usize = 192;

/// Groth16 kanıt verisi (production'da 192 byte, mock'ta daha küçük)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Groth16Proof {
    /// Kanıt byte'ları (A, B, C ∈ G1,G2,G1)
    pub proof_bytes: Vec<u8>,
    /// Public girdiler (circuit'e göre değişir)
    pub public_inputs: Vec<[u8; 32]>,
    /// Circuit türü
    pub circuit_type: CircuitType,
}

impl Groth16Proof {
    /// Mock kanıt üret — deterministik (aynı girdiler = aynı kanıt)
    pub fn mock_prove(
        params: &TrustedSetupParams,
        public_inputs: &[[u8; 32]],
        witness_inputs: &[[u8; 32]],
    ) -> ZkResult<Self> {
        // proof = BLAKE3(setup_hash || public_inputs... || witness_inputs...)
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-groth16-mock-proof:v1!!!!!!!");
        hasher.update(params.setup_hash());
        for pi in public_inputs {
            hasher.update(pi);
        }
        for wi in witness_inputs {
            hasher.update(wi);
        }
        let proof_hash = *hasher.finalize().as_bytes();

        // Mock: [proof_hash (32) || setup_hash (32)] = 64 byte
        let mut proof_bytes = Vec::with_capacity(64);
        proof_bytes.extend_from_slice(&proof_hash);
        proof_bytes.extend_from_slice(params.setup_hash());

        Ok(Self {
            proof_bytes,
            public_inputs: public_inputs.to_vec(),
            circuit_type: params.circuit_type,
        })
    }

    /// Kanıtı doğrula
    pub fn verify(&self, params: &TrustedSetupParams) -> ZkResult<bool> {
        if self.circuit_type != params.circuit_type {
            return Err(ZkError::InvalidProof);
        }
        if self.proof_bytes.len() < 64 {
            return Err(ZkError::InvalidProof);
        }
        // Mock: setup_hash ikinci 32 byte'ta eşleşmeli
        let stored_setup = &self.proof_bytes[32..64];
        if stored_setup != params.setup_hash() {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn proof_size_bytes(&self) -> usize {
        self.proof_bytes.len()
    }

    /// Gerçek kanıt mı (192 byte) yoksa mock mu (64 byte)?
    pub fn is_real(&self) -> bool {
        self.proof_bytes.len() == GROTH16_PROOF_SIZE
    }

    // ── Gerçek Groth16 API ────────────────────────────────────────────────────

    /// Gerçek Groth16 kanıtı üret (BLS12-381).
    ///
    /// # Argümanlar
    /// * `circuit`            — ConstraintSynthesizer implementasyonu (witness dolu)
    /// * `circuit_type`       — hangi devre (key seçimi için)
    /// * `public_inputs_bytes`— public input'lar ([u8;32] olarak, Fr'ye dönüştürülür)
    ///
    /// # Dönüş
    /// 192 byte proof içeren `Groth16Proof`.
    ///
    /// # Performans
    /// İlk çağrıda `CircuitKeys` setup çalışır (yavaş). Sonraki çağrılar cache'ten alır.
    pub fn prove_groth16<C>(
        circuit: C,
        circuit_type: CircuitType,
        public_inputs_bytes: Vec<[u8; 32]>,
    ) -> ZkResult<Self>
    where
        C: ConstraintSynthesizer<Fr>,
    {
        let keys = crate::proof::keys::get_keys(circuit_type)?;
        let mut rng = rand::thread_rng();

        let proof = Groth16::<Bls12_381>::prove(&keys.pk, circuit, &mut rng)
            .map_err(|e| ZkError::ProofGenerationFailed(e.to_string()))?;

        let mut proof_bytes = Vec::with_capacity(GROTH16_PROOF_SIZE);
        proof
            .serialize_compressed(&mut proof_bytes)
            .map_err(|e| ZkError::ProofGenerationFailed(e.to_string()))?;

        debug_assert_eq!(
            proof_bytes.len(),
            GROTH16_PROOF_SIZE,
            "BLS12-381 Groth16 kanıtı 192 byte olmalı"
        );

        Ok(Self {
            proof_bytes,
            public_inputs: public_inputs_bytes,
            circuit_type,
        })
    }

    /// Gerçek Groth16 kanıtını doğrula.
    ///
    /// `public_inputs` sırası devredeki `new_input` çağrılarının sırasıyla aynı olmalı.
    pub fn verify_groth16(&self) -> ZkResult<bool> {
        if self.proof_bytes.len() != GROTH16_PROOF_SIZE {
            return Err(ZkError::InvalidProof);
        }

        let proof = Proof::<Bls12_381>::deserialize_compressed(&self.proof_bytes[..])
            .map_err(|_| ZkError::InvalidProof)?;

        let keys = crate::proof::keys::get_keys(self.circuit_type)?;

        let public_inputs_fr: Vec<Fr> = self
            .public_inputs
            .iter()
            .map(|b| Fr::from_le_bytes_mod_order(b))
            .collect();

        let valid = Groth16::<Bls12_381>::verify_with_processed_vk(
            &keys.pvk,
            &public_inputs_fr,
            &proof,
        )
        .map_err(|e| ZkError::ProofGenerationFailed(e.to_string()))?;

        Ok(valid)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proof::params::CircuitType;

    fn mint_params() -> TrustedSetupParams {
        TrustedSetupParams::mock(CircuitType::Mint)
    }

    #[test]
    fn test_groth16_prove_and_verify() {
        let params = mint_params();
        let pub_in = [[1u8; 32]];
        let wit_in = [[2u8; 32]];
        let proof = Groth16Proof::mock_prove(&params, &pub_in, &wit_in).unwrap();
        assert!(proof.verify(&params).unwrap());
    }

    #[test]
    fn test_groth16_proof_deterministic() {
        let params = mint_params();
        let pub_in = [[1u8; 32]];
        let wit_in = [[2u8; 32]];
        let p1 = Groth16Proof::mock_prove(&params, &pub_in, &wit_in).unwrap();
        let p2 = Groth16Proof::mock_prove(&params, &pub_in, &wit_in).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_groth16_wrong_params_fails() {
        let params = mint_params();
        let transfer_params = TrustedSetupParams::mock(CircuitType::Transfer);
        let proof = Groth16Proof::mock_prove(&params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();
        assert!(matches!(proof.verify(&transfer_params), Err(ZkError::InvalidProof)));
    }

    #[test]
    fn test_groth16_tampered_proof_rejected() {
        let params = mint_params();
        let mut proof = Groth16Proof::mock_prove(&params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();
        // Tamper the setup hash in proof bytes
        proof.proof_bytes[32] ^= 0xFF;
        assert!(!proof.verify(&params).unwrap());
    }

    #[test]
    fn test_groth16_different_witnesses_different_proofs() {
        let params = mint_params();
        let pub_in = [[1u8; 32]];
        let p1 = Groth16Proof::mock_prove(&params, &pub_in, &[[10u8; 32]]).unwrap();
        let p2 = Groth16Proof::mock_prove(&params, &pub_in, &[[20u8; 32]]).unwrap();
        assert_ne!(p1.proof_bytes, p2.proof_bytes);
    }
}
