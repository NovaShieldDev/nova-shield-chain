//! StakeCircuit R1CS — Shielded staking kanıtı
//!
//! Kanıt: "Bu notu sahiplendim (Merkle tree'de), şu validator'a şu miktarı stake ediyorum"
//!
//! Public inputs:
//!   1. stake_nullifier_fr   — double-stake önleme
//!   2. stake_amount_fr      — stake edilen miktar
//!   3. validator_hash_fr    — validator kimlik hash'i
//!   4. anchor_fr            — commitment tree Merkle root (NOVA-RED-ZK-003)
//!
//! Private witness:
//!   value, randomness, asset_id, transmission_key, nullifier_key, validator_id,
//!   merkle_path (NOVA-RED-ZK-003)

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_merkle, domain_note_commitment, domain_nullifier, domain_stake,
    mimc_sponge_r1cs, u128_to_fr, MERKLE_DEPTH,
};

// ─── StakeCircuit ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StakeCircuit {
    // Public
    pub stake_nullifier: Option<[u8; 32]>,
    pub stake_amount:    Option<u128>,
    pub validator_hash:  Option<[u8; 32]>,
    /// NOVA-RED-ZK-003: Commitment tree Merkle root.
    pub anchor:          Option<[u8; 32]>,
    // Private
    pub value:           Option<u128>,
    pub randomness:      Option<[u8; 32]>,
    pub asset_id:        Option<[u8; 32]>,
    pub transmission_key:Option<[u8; 32]>,
    pub nullifier_key:   Option<[u8; 32]>,
    pub validator_id:    Option<[u8; 32]>,
    /// NOVA-RED-ZK-003: Merkle inclusion path.
    pub merkle_path:     Option<Vec<([u8; 32], bool)>>,
}

impl StakeCircuit {
    pub fn new(
        value: u128,
        randomness: [u8; 32],
        asset_id: [u8; 32],
        transmission_key: [u8; 32],
        nullifier_key: [u8; 32],
        validator_id: [u8; 32],
        stake_amount: u128,
        stake_nullifier: [u8; 32],
        validator_hash: [u8; 32],
        anchor: [u8; 32],
        merkle_path: Vec<([u8; 32], bool)>,
    ) -> Self {
        Self {
            stake_nullifier: Some(stake_nullifier),
            stake_amount: Some(stake_amount),
            validator_hash: Some(validator_hash),
            anchor: Some(anchor),
            value: Some(value),
            randomness: Some(randomness),
            asset_id: Some(asset_id),
            transmission_key: Some(transmission_key),
            nullifier_key: Some(nullifier_key),
            validator_id: Some(validator_id),
            merkle_path: Some(merkle_path),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            stake_nullifier: None,
            stake_amount: None,
            validator_hash: None,
            anchor: None,
            value: None,
            randomness: None,
            asset_id: None,
            transmission_key: None,
            nullifier_key: None,
            validator_id: None,
            merkle_path: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for StakeCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private ───────────────────────────────────────────────────────────
        let val_var = FpVar::new_witness(cs.clone(), || {
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
        let validator_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.validator_id.unwrap_or([0u8; 32])))
        })?;
        let stake_amount_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.stake_amount.unwrap_or(0)))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.stake_nullifier.unwrap_or([0u8; 32])))
        })?;
        let amount_pub = FpVar::new_input(cs.clone(), || {
            Ok(u128_to_fr(self.stake_amount.unwrap_or(0)))
        })?;
        let validator_hash_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.validator_hash.unwrap_or([0u8; 32])))
        })?;
        // NOVA-RED-ZK-003: Merkle root public input.
        let anchor_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.anchor.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt 1: note commitment ──────────────────────────────────────────
        let domain_note = FpVar::Constant(domain_note_commitment());
        let cm = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note, val_var.clone(), rand_var, asset_var, tk_var],
        )?;

        // ── Kısıt 2: stake nullifier ──────────────────────────────────────────
        let domain_sk = FpVar::Constant(domain_stake());
        let nf_computed = mimc_sponge_r1cs(cs.clone(), &[domain_sk, nk_var, cm.clone()])?;
        nf_computed.enforce_equal(&nf_pub)?;

        // ── Kısıt 3: stake miktarı tutarlılık ────────────────────────────────
        stake_amount_var.enforce_equal(&amount_pub)?;

        // ── Kısıt 4: validator hash ────────────────────────────────────────────
        let vh_computed = mimc_sponge_r1cs(cs.clone(), &[validator_var])?;
        vh_computed.enforce_equal(&validator_hash_pub)?;

        // ── Kısıt 5 (NOVA-RED-ZK-003): Merkle inclusion proof ────────────────
        {
            let path = self.merkle_path.unwrap_or_default();
            let domain_merkle_const = domain_merkle();
            let mut current = cm;

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

                let left  = is_right_var.select(&current, &sibling_var)?;
                let right = is_right_var.select(&sibling_var, &current)?;

                let domain_var = FpVar::Constant(domain_merkle_const);
                current = mimc_sponge_r1cs(cs.clone(), &[domain_var, left, right])?;
            }

            current.enforce_equal(&anchor_pub)?;
        }

        Ok(())
    }
}

/// Stake nullifier hesaplama (native)
pub fn compute_stake_nullifier(nullifier_key: &[u8; 32], commitment: &[u8; 32]) -> [u8; 32] {
    use crate::circuits::mimc::{bytes_to_fr, fr_to_bytes, mimc_sponge};
    let h = mimc_sponge(&[
        domain_stake(),
        bytes_to_fr(nullifier_key),
        bytes_to_fr(commitment),
    ]);
    fr_to_bytes(h)
}

/// Validator hash (native)
pub fn compute_validator_hash(validator_id: &[u8; 32]) -> [u8; 32] {
    use crate::circuits::mimc::{bytes_to_fr, fr_to_bytes, mimc_sponge};
    let h = mimc_sponge(&[bytes_to_fr(validator_id)]);
    fr_to_bytes(h)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::{mimc_merkle_root_padded, mimc_note_commitment};

    fn build_stake(value: u128, stake_amount: u128) -> StakeCircuit {
        let asset = {
            let mut a = [0u8; 32];
            a[0] = 0x4E; a[1] = 0x53; a[2] = 0x43; a[3] = 0x01;
            a
        };
        let rand = [0x10u8; 32];
        let tk   = [0x20u8; 32];
        let nk   = [0x30u8; 32];
        let validator = [0x40u8; 32];

        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let nf = compute_stake_nullifier(&nk, &cm);
        let vh = compute_validator_hash(&validator);
        // NOVA-RED-ZK-003: Boş path = tek note tree
        let anchor = mimc_merkle_root_padded(&cm, &[]);

        StakeCircuit::new(value, rand, asset, tk, nk, validator, stake_amount, nf, vh, anchor, vec![])
    }

    #[test]
    fn test_stake_circuit_satisfied() {
        let circuit = build_stake(10_000, 5_000);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
        println!("StakeCircuit constraint: {}", cs.num_constraints());
    }

    #[test]
    fn test_stake_wrong_nullifier_fails() {
        let asset = { let mut a = [0u8; 32]; a[0]=0x4E; a[1]=0x53; a[2]=0x43; a[3]=0x01; a };
        let rand = [0x10u8; 32]; let tk = [0x20u8; 32]; let nk = [0x30u8; 32];
        let validator = [0x40u8; 32];
        let cm = mimc_note_commitment(1000, &rand, &asset, &tk);
        let vh = compute_validator_hash(&validator);
        let anchor = mimc_merkle_root_padded(&cm, &[]);
        let wrong_nf = [0xFFu8; 32];

        let circuit = StakeCircuit::new(1000, rand, asset, tk, nk, validator, 500, wrong_nf, vh, anchor, vec![]);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());
    }

    /// NOVA-RED-ZK-003: Yanlış Merkle root ile stake reddedilmeli.
    #[test]
    fn test_stake_wrong_anchor_fails() {
        let asset = { let mut a = [0u8; 32]; a[0]=0x4E; a[1]=0x53; a[2]=0x43; a[3]=0x01; a };
        let rand = [0x10u8; 32]; let tk = [0x20u8; 32]; let nk = [0x30u8; 32];
        let validator = [0x40u8; 32];
        let cm = mimc_note_commitment(1000, &rand, &asset, &tk);
        let nf = compute_stake_nullifier(&nk, &cm);
        let vh = compute_validator_hash(&validator);
        let wrong_anchor = [0xFFu8; 32]; // tree'de yok

        let circuit = StakeCircuit::new(1000, rand, asset, tk, nk, validator, 500, nf, vh, wrong_anchor, vec![]);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış anchor kısıtı bozmalı");
    }

    #[test]
    fn test_stake_setup_dummy() {
        let circuit = StakeCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
