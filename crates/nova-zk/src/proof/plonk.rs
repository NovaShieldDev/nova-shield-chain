//! PLONK kanıt sistemi — mock implementasyon
//!
//! Production: arkworks PLONK / halo2
//! PLONK avantajı: Universal trusted setup (bir kez, tüm circuit'ler)
//! Bu mod: mock API (Groth16'dan daha büyük kanıt, universal params)

use crate::errors::{ZkError, ZkResult};
use crate::proof::params::{CircuitType, ProofSystemType, TrustedSetupParams};
use serde::{Deserialize, Serialize};

/// PLONK kanıt verisi
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlonkProof {
    pub proof_bytes: Vec<u8>,
    pub public_inputs: Vec<[u8; 32]>,
    pub circuit_type: CircuitType,
}

impl PlonkProof {
    /// Mock PLONK kanıtı — Groth16'dan biraz daha büyük (96 byte)
    pub fn mock_prove(
        params: &TrustedSetupParams,
        public_inputs: &[[u8; 32]],
        witness_inputs: &[[u8; 32]],
    ) -> ZkResult<Self> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"nova-plonk-mock-proof:v1!!!!!!!!!");
        hasher.update(params.setup_hash());
        hasher.update(params.circuit_type.name().as_bytes());
        for pi in public_inputs {
            hasher.update(pi);
        }
        for wi in witness_inputs {
            hasher.update(wi);
        }
        let proof_hash = *hasher.finalize().as_bytes();

        // PLONK: [proof_hash (32) || setup_hash (32) || circuit_name_hash (32)] = 96 byte
        let circuit_hash = {
            let mut h = blake3::Hasher::new();
            h.update(params.circuit_type.name().as_bytes());
            *h.finalize().as_bytes()
        };

        let mut proof_bytes = Vec::with_capacity(96);
        proof_bytes.extend_from_slice(&proof_hash);
        proof_bytes.extend_from_slice(params.setup_hash());
        proof_bytes.extend_from_slice(&circuit_hash);

        Ok(Self {
            proof_bytes,
            public_inputs: public_inputs.to_vec(),
            circuit_type: params.circuit_type,
        })
    }

    /// PLONK kanıtı doğrula
    pub fn verify(&self, params: &TrustedSetupParams) -> ZkResult<bool> {
        if self.circuit_type != params.circuit_type {
            return Err(ZkError::InvalidProof);
        }
        if self.proof_bytes.len() < 96 {
            return Err(ZkError::InvalidProof);
        }
        // Setup hash kontrolü
        let stored_setup = &self.proof_bytes[32..64];
        if stored_setup != params.setup_hash() {
            return Ok(false);
        }
        // Circuit hash kontrolü
        let expected_circuit_hash = {
            let mut h = blake3::Hasher::new();
            h.update(params.circuit_type.name().as_bytes());
            *h.finalize().as_bytes()
        };
        let stored_circuit = &self.proof_bytes[64..96];
        if stored_circuit != expected_circuit_hash {
            return Ok(false);
        }
        Ok(true)
    }

    pub fn proof_size_bytes(&self) -> usize {
        self.proof_bytes.len()
    }
}

/// Universal SRS (Structured Reference String) — tüm circuit'ler için
pub struct UniversalSrs {
    pub srs_hash: [u8; 32],
    pub max_degree: usize,
}

impl UniversalSrs {
    pub fn mock(max_degree: usize) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-shield:universal-srs:v1!!!!");
        h.update(&max_degree.to_le_bytes());
        Self { srs_hash: *h.finalize().as_bytes(), max_degree }
    }

    pub fn specialize(&self, circuit_type: CircuitType) -> TrustedSetupParams {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-shield:specialize-srs:v1!!!");
        h.update(&self.srs_hash);
        h.update(circuit_type.name().as_bytes());
        TrustedSetupParams {
            circuit_type,
            proof_system: ProofSystemType::Plonk,
            setup_hash: *h.finalize().as_bytes(),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer_params() -> TrustedSetupParams {
        TrustedSetupParams::mock(CircuitType::Transfer)
    }

    #[test]
    fn test_plonk_prove_and_verify() {
        let params = transfer_params();
        let proof = PlonkProof::mock_prove(&params, &[[5u8; 32]], &[[6u8; 32]]).unwrap();
        assert!(proof.verify(&params).unwrap());
    }

    #[test]
    fn test_plonk_proof_size_larger_than_groth16() {
        let params = transfer_params();
        let proof = PlonkProof::mock_prove(&params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();
        assert_eq!(proof.proof_size_bytes(), 96); // 96 > 64 (Groth16)
    }

    #[test]
    fn test_plonk_wrong_circuit_rejected() {
        let params = transfer_params();
        let burn_params = TrustedSetupParams::mock(CircuitType::Burn);
        let proof = PlonkProof::mock_prove(&params, &[[1u8; 32]], &[[2u8; 32]]).unwrap();
        assert!(matches!(proof.verify(&burn_params), Err(ZkError::InvalidProof)));
    }

    #[test]
    fn test_universal_srs_specialization() {
        let srs = UniversalSrs::mock(1 << 20);
        let mint_params = srs.specialize(CircuitType::Mint);
        let transfer_params = srs.specialize(CircuitType::Transfer);
        assert_ne!(mint_params.setup_hash, transfer_params.setup_hash);
        assert!(!mint_params.is_mock()); // Plonk, not mock
    }
}
