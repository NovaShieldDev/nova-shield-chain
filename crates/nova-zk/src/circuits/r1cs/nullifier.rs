//! NullifierDerivationCircuit R1CS — Bağımsız nullifier türetme kanıtı
//!
//! Kanıt: "nullifier_key'i biliyorum ve nf = mimc(domain_nf, nk, cm) doğru"
//! Nullifier key'i açıklamadan nullifier türetme yetkinliği kanıtlanır.
//!
//! Public inputs:
//!   1. nullifier_fr   — türetilen nullifier
//!   2. commitment_fr  — ilgili note commitment
//!
//! Private witness:
//!   nullifier_key ([u8;32])
//!
//! Constraint sayısı: 3 × MIMC_ROUNDS × 2 = 546

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{bytes_to_fr, domain_nullifier, mimc_sponge_r1cs};

// ─── NullifierDerivationCircuit ───────────────────────────────────────────────

#[derive(Clone)]
pub struct NullifierDerivationCircuit {
    // Public
    pub nullifier:   Option<[u8; 32]>,
    pub commitment:  Option<[u8; 32]>,
    // Private
    pub nullifier_key: Option<[u8; 32]>,
}

impl NullifierDerivationCircuit {
    pub fn new(
        nullifier_key: [u8; 32],
        commitment: [u8; 32],
        nullifier: [u8; 32],
    ) -> Self {
        Self {
            nullifier: Some(nullifier),
            commitment: Some(commitment),
            nullifier_key: Some(nullifier_key),
        }
    }

    pub fn setup_dummy() -> Self {
        Self { nullifier: None, commitment: None, nullifier_key: None }
    }
}

impl ConstraintSynthesizer<Fr> for NullifierDerivationCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private: nullifier key ─────────────────────────────────────────────
        let nk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier_key.unwrap_or([0u8; 32])))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier.unwrap_or([0u8; 32])))
        })?;
        let cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.commitment.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt: nf = mimc_sponge([domain_nf, nk, cm]) ─────────────────────
        let domain_nf = FpVar::Constant(domain_nullifier());
        let nf_computed = mimc_sponge_r1cs(cs, &[domain_nf, nk_var, cm_pub])?;
        nf_computed.enforce_equal(&nf_pub)?;

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::mimc_nullifier;

    #[test]
    fn test_nullifier_circuit_satisfied() {
        let nk = [0xAAu8; 32];
        let cm = [0xBBu8; 32];
        let nf = mimc_nullifier(&nk, &cm);

        let circuit = NullifierDerivationCircuit::new(nk, cm, nf);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
        // Domain sabiti (0) + 2 değişken × 91 × 2 + 1 enforce_equal = 365
        let n = cs.num_constraints();
        println!("NullifierDerivationCircuit constraint: {}", n);
        assert_eq!(n, 365, "365 kısıt bekleniyor, gerçek: {}", n);
    }

    #[test]
    fn test_nullifier_wrong_nk_fails() {
        let nk = [0xAAu8; 32];
        let wrong_nk = [0xBBu8; 32]; // yanlış
        let cm = [0xCCu8; 32];
        let nf = mimc_nullifier(&nk, &cm);

        let circuit = NullifierDerivationCircuit::new(wrong_nk, cm, nf);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_nullifier_wrong_commitment_fails() {
        let nk = [0xAAu8; 32];
        let cm = [0xCCu8; 32];
        let wrong_cm = [0xDDu8; 32];
        let nf = mimc_nullifier(&nk, &cm);

        let circuit = NullifierDerivationCircuit::new(nk, wrong_cm, nf);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_nullifier_setup_dummy_builds() {
        let circuit = NullifierDerivationCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
