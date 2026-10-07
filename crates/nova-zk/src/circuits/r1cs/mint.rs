//! MintCircuit R1CS — Transparent → Shielded (§5.7.3)
//!
//! Kanıt: "value adet NSC yakıldı ve şu commitment üretildi"
//!
//! Public inputs (sırayla ark-groth16'ya verilir):
//!   1. commitment_fr: note commitment (Fr field element)
//!
//! Private witness:
//!   value (u128), randomness ([u8;32]), asset_id ([u8;32]), transmission_key ([u8;32])
//!
//! Kısıt:
//!   cm = mimc_sponge([domain_note, value_fr, rand_fr, asset_fr, tk_fr])
//!   cm == commitment_fr  (enforce_equal)
//!
//! Constraint sayısı: 5 × MIMC_ROUNDS × 2 = 910

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_note_commitment, mimc_sponge_r1cs, u128_to_fr,
};

// ─── MintCircuit ──────────────────────────────────────────────────────────────

/// Mint ZK devresi — note preimage bilgisini kanıtlar.
///
/// `setup_dummy()` ile trusted setup, `new(...)` ile kanıt üretimi.
#[derive(Clone)]
pub struct MintCircuit {
    // Public input (ark-groth16 public_inputs dizisine eklenir)
    pub commitment: Option<[u8; 32]>,
    // Private witness (sadece kanıtlayıcı bilir)
    pub value: Option<u128>,
    pub randomness: Option<[u8; 32]>,
    pub asset_id: Option<[u8; 32]>,
    pub transmission_key: Option<[u8; 32]>,
}

impl MintCircuit {
    /// Gerçek kanıt üretimi için — tüm alanlar dolu
    pub fn new(
        value: u128,
        randomness: [u8; 32],
        asset_id: [u8; 32],
        transmission_key: [u8; 32],
        commitment: [u8; 32],
    ) -> Self {
        Self {
            commitment: Some(commitment),
            value: Some(value),
            randomness: Some(randomness),
            asset_id: Some(asset_id),
            transmission_key: Some(transmission_key),
        }
    }

    /// Trusted setup için — hiçbir değer yok (sadece devre yapısı)
    pub fn setup_dummy() -> Self {
        Self {
            commitment: None,
            value: None,
            randomness: None,
            asset_id: None,
            transmission_key: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for MintCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private witness allocation ─────────────────────────────────────────
        let value_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.value.unwrap_or(0)))
        })?;
        let rand_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.randomness.unwrap_or([0u8; 32])))
        })?;
        let asset_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.asset_id.unwrap_or([0u8; 32])))
        })?;
        let tk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.transmission_key.unwrap_or([0u8; 32])))
        })?;

        // ── Public input allocation ────────────────────────────────────────────
        let cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.commitment.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt: cm = mimc_sponge(domain, value, rand, asset, tk) ──────────
        let domain_var = FpVar::Constant(domain_note_commitment());
        let cm_computed = mimc_sponge_r1cs(
            cs,
            &[domain_var, value_var, rand_var, asset_var, tk_var],
        )?;

        // cm_computed == cm_pub
        cm_computed.enforce_equal(&cm_pub)?;

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::mimc_note_commitment;

    fn sample_note() -> (u128, [u8; 32], [u8; 32], [u8; 32]) {
        let value: u128 = 1_000_000_000_000_000_000; // 1 NSC
        let rand = [0x11u8; 32];
        let asset = [0x4Eu8, 0x53u8, 0x43u8, 0x01u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tk = [0x55u8; 32];
        (value, rand, asset, tk)
    }

    #[test]
    fn test_mint_circuit_constraints_satisfied() {
        let (value, rand, asset, tk) = sample_note();
        let cm = mimc_note_commitment(value, &rand, &asset, &tk);

        let circuit = MintCircuit::new(value, rand, asset, tk, cm);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap(), "MintCircuit kısıtları sağlanmalı");
        println!("MintCircuit constraint sayısı: {}", cs.num_constraints());
    }

    #[test]
    fn test_mint_circuit_wrong_commitment_fails() {
        let (value, rand, asset, tk) = sample_note();
        let wrong_cm = [0xFFu8; 32]; // yanlış commitment

        let circuit = MintCircuit::new(value, rand, asset, tk, wrong_cm);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(!cs.is_satisfied().unwrap(), "Yanlış commitment ile kısıt sağlanmamalı");
    }

    #[test]
    fn test_mint_circuit_setup_dummy_builds() {
        // Setup dummy'de None değerler ile devre yapısı oluşturulabilmeli
        let circuit = MintCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        // Constraint sayısı aynı olmalı (değerler önemli değil setup'ta)
        let n = cs.num_constraints();
        assert!(n > 0, "Setup dummy constraint üretmeli, n={}", n);
    }

    #[test]
    fn test_mint_circuit_constraint_count() {
        // Beklenen: domain sabiti (0) + 4 değişken × 91 × 2 + 1 enforce_equal = 729
        let (value, rand, asset, tk) = sample_note();
        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let circuit = MintCircuit::new(value, rand, asset, tk, cm);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        let n = cs.num_constraints();
        // Domain sabiti → 0 kısıt; 4 değişken giriş × 91 round × 2 = 728 + 1 enforce_equal = 729
        assert_eq!(n, 729, "MintCircuit 729 kısıt içermeli, gerçek: {}", n);
    }
}
