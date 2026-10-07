//! SpendCircuit R1CS — Shielded not sahipliği + nullifier türetme (§5.7.4)
//!
//! Kanıt: "Bu commitment'ın preimage'ını biliyorum ve bu nullifier'ı türetebilirim"
//!
//! Public inputs (sırayla):
//!   1. commitment_fr: note commitment
//!   2. nullifier_fr:  double-spend nullifier
//!   3. anchor_fr:     Merkle ağaç kökü (MERKLE_DEPTH seviye padded)
//!
//! Private witness:
//!   value, randomness, asset_id, transmission_key, nullifier_key, merkle_path
//!
//! Kısıtlar:
//!   1. cm = mimc_sponge([domain_note, value, rand, asset, tk])  → cm == commitment_fr
//!   2. nf = mimc_sponge([domain_nf, nk, cm])                    → nf == nullifier_fr
//!   3. Merkle path: MERKLE_DEPTH seviye, padded → root == anchor_fr
//!
//! Constraint sayısı: 910 + 546 + 8×3×182 = ~5,824

use ark_bls12_381::Fr;
use ark_r1cs_std::{boolean::Boolean, fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_merkle, domain_note_commitment, domain_nullifier,
    mimc_sponge_r1cs, u128_to_fr, MERKLE_DEPTH,
};

// ─── SpendCircuit ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SpendCircuit {
    // Public inputs
    pub commitment:   Option<[u8; 32]>,
    pub nullifier:    Option<[u8; 32]>,
    pub anchor:       Option<[u8; 32]>,
    // Private witness
    pub value:              Option<u128>,
    pub randomness:         Option<[u8; 32]>,
    pub asset_id:           Option<[u8; 32]>,
    pub transmission_key:   Option<[u8; 32]>,
    pub nullifier_key:      Option<[u8; 32]>,
    /// MERKLE_DEPTH adet (sibling, is_right) — kısa yollar sıfır ile doldurulur
    pub merkle_path:        Option<Vec<([u8; 32], bool)>>,
}

impl SpendCircuit {
    pub fn new(
        value: u128,
        randomness: [u8; 32],
        asset_id: [u8; 32],
        transmission_key: [u8; 32],
        nullifier_key: [u8; 32],
        merkle_path: Vec<([u8; 32], bool)>,
        commitment: [u8; 32],
        nullifier: [u8; 32],
        anchor: [u8; 32],
    ) -> Self {
        Self {
            commitment: Some(commitment),
            nullifier: Some(nullifier),
            anchor: Some(anchor),
            value: Some(value),
            randomness: Some(randomness),
            asset_id: Some(asset_id),
            transmission_key: Some(transmission_key),
            nullifier_key: Some(nullifier_key),
            merkle_path: Some(merkle_path),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            commitment: None,
            nullifier: None,
            anchor: None,
            value: None,
            randomness: None,
            asset_id: None,
            transmission_key: None,
            nullifier_key: None,
            merkle_path: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for SpendCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private: note alanları ─────────────────────────────────────────────
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
        let nk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier_key.unwrap_or([0u8; 32])))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.commitment.unwrap_or([0u8; 32])))
        })?;
        let nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier.unwrap_or([0u8; 32])))
        })?;
        let anchor_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.anchor.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt 1: note commitment ───────────────────────────────────────────
        let domain_note = FpVar::Constant(domain_note_commitment());
        let cm_computed = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note, value_var, rand_var, asset_var, tk_var],
        )?;
        cm_computed.enforce_equal(&cm_pub)?;

        // ── Kısıt 2: nullifier türetme ────────────────────────────────────────
        let domain_nf = FpVar::Constant(domain_nullifier());
        let nf_computed = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_nf, nk_var, cm_computed.clone()],
        )?;
        nf_computed.enforce_equal(&nf_pub)?;

        // ── Kısıt 3: Merkle path (MERKLE_DEPTH seviye, padded) ────────────────
        let path = self.merkle_path.unwrap_or_default();
        let domain_merkle_const = domain_merkle();

        let mut current = cm_computed;
        for i in 0..MERKLE_DEPTH {
            let (sibling_bytes, is_right) = if i < path.len() {
                path[i]
            } else {
                ([0u8; 32], false)
            };

            let sibling_var = FpVar::new_witness(cs.clone(), || {
                Ok(bytes_to_fr(&sibling_bytes))
            })?;
            let is_right_var = Boolean::<Fr>::new_witness(cs.clone(), || Ok(is_right))?;

            // Natif mimc_merkle_root ile aynı semantik:
            //   is_right=true  → hash(current, sibling): current solda, sibling sağda
            //   is_right=false → hash(sibling, current): sibling solda, current sağda
            // select(first_if_true, second_if_false):
            let left  = is_right_var.select(&current, &sibling_var)?;   // true→current, false→sibling
            let right = is_right_var.select(&sibling_var, &current)?;   // true→sibling, false→current

            let domain_var = FpVar::Constant(domain_merkle_const);
            current = mimc_sponge_r1cs(cs.clone(), &[domain_var, left, right])?;
        }

        current.enforce_equal(&anchor_pub)?;

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::{
        mimc_merkle_root_padded, mimc_note_commitment, mimc_nullifier,
    };

    fn build_circuit(value: u128, rand: [u8; 32], nk: [u8; 32]) -> SpendCircuit {
        let asset = [0x4Eu8, 0x53u8, 0x43u8, 0x01u8, 0, 0, 0, 0,
                      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tk = [0x01u8; 32]; // sıfır değil (address integrity)

        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let nf = mimc_nullifier(&nk, &cm);
        let anchor = mimc_merkle_root_padded(&cm, &[]); // boş path, padded

        SpendCircuit::new(value, rand, asset, tk, nk, vec![], cm, nf, anchor)
    }

    #[test]
    fn test_spend_circuit_satisfied() {
        let circuit = build_circuit(1_000_000, [1u8; 32], [42u8; 32]);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap(), "SpendCircuit kısıtları sağlanmalı");
        println!("SpendCircuit constraint sayısı: {}", cs.num_constraints());
    }

    #[test]
    fn test_spend_wrong_commitment_fails() {
        let asset = [0x4Eu8, 0x53u8, 0x43u8, 0x01u8, 0, 0, 0, 0,
                      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tk = [0x01u8; 32];
        let nk = [42u8; 32];
        let rand = [1u8; 32];
        let value = 500u128;

        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let nf = mimc_nullifier(&nk, &cm);
        let anchor = mimc_merkle_root_padded(&cm, &[]);
        let wrong_cm = [0xFFu8; 32]; // yanlış

        let circuit = SpendCircuit::new(value, rand, asset, tk, nk, vec![], wrong_cm, nf, anchor);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış commitment kısıtı bozmalı");
    }

    #[test]
    fn test_spend_wrong_nullifier_fails() {
        let asset = [0x4Eu8, 0x53u8, 0x43u8, 0x01u8, 0, 0, 0, 0,
                      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tk = [0x01u8; 32];
        let nk = [42u8; 32];
        let rand = [2u8; 32];
        let value = 999u128;

        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let anchor = mimc_merkle_root_padded(&cm, &[]);
        let wrong_nf = [0xAAu8; 32]; // yanlış nullifier

        let circuit = SpendCircuit::new(value, rand, asset, tk, nk, vec![], cm, wrong_nf, anchor);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış nullifier kısıtı bozmalı");
    }

    #[test]
    fn test_spend_setup_dummy_builds() {
        let circuit = SpendCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
