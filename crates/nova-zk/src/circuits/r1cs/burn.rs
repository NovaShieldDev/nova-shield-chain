//! BurnCircuit R1CS — Shielded → Transparent (§5.7.3)
//!
//! Kanıt: "Bu nullifier benim, bu miktarı şu adrese çek"
//!
//! Public inputs:
//!   1. nullifier_fr
//!   2. value_fr
//!   3. recipient_hash_fr (alıcı adresinin hash'i)
//!
//! Private witness:
//!   value, randomness, asset_id, transmission_key, nullifier_key, recipient

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_note_commitment, domain_nullifier, mimc_sponge_r1cs, u128_to_fr,
};

// ─── BurnCircuit ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BurnCircuit {
    // Public
    pub nullifier:      Option<[u8; 32]>,
    pub value:          Option<u128>,
    pub recipient_hash: Option<[u8; 32]>,  // blake3(recipient_address)
    // Private
    pub note_value:         Option<u128>,
    pub randomness:         Option<[u8; 32]>,
    pub asset_id:           Option<[u8; 32]>,
    pub transmission_key:   Option<[u8; 32]>,
    pub nullifier_key:      Option<[u8; 32]>,
    pub recipient:          Option<[u8; 20]>,
}

impl BurnCircuit {
    pub fn new(
        note_value: u128,
        randomness: [u8; 32],
        asset_id: [u8; 32],
        transmission_key: [u8; 32],
        nullifier_key: [u8; 32],
        recipient: [u8; 20],
        nullifier: [u8; 32],
        recipient_hash: [u8; 32],
    ) -> Self {
        Self {
            nullifier: Some(nullifier),
            value: Some(note_value),
            recipient_hash: Some(recipient_hash),
            note_value: Some(note_value),
            randomness: Some(randomness),
            asset_id: Some(asset_id),
            transmission_key: Some(transmission_key),
            nullifier_key: Some(nullifier_key),
            recipient: Some(recipient),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            nullifier: None,
            value: None,
            recipient_hash: None,
            note_value: None,
            randomness: None,
            asset_id: None,
            transmission_key: None,
            nullifier_key: None,
            recipient: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for BurnCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private witness ────────────────────────────────────────────────────
        let val_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.note_value.unwrap_or(0)))
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
        // Recipient [u8;20] → [u8;32] padded → Fr
        let recipient_var = FpVar::new_witness(cs.clone(), || {
            let mut padded = [0u8; 32];
            if let Some(r) = self.recipient {
                padded[..20].copy_from_slice(&r);
            }
            Ok(bytes_to_fr(&padded))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier.unwrap_or([0u8; 32])))
        })?;
        let value_pub = FpVar::new_input(cs.clone(), || {
            Ok(u128_to_fr(self.value.unwrap_or(0)))
        })?;
        let recipient_hash_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.recipient_hash.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt 1: note commitment ───────────────────────────────────────────
        let domain_note = FpVar::Constant(domain_note_commitment());
        let cm = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note, val_var.clone(), rand_var, asset_var, tk_var],
        )?;

        // ── Kısıt 2: nullifier ────────────────────────────────────────────────
        let domain_nf = FpVar::Constant(domain_nullifier());
        let nf_computed = mimc_sponge_r1cs(cs.clone(), &[domain_nf, nk_var, cm])?;
        nf_computed.enforce_equal(&nf_pub)?;

        // ── Kısıt 3: değer tutarlılığı ────────────────────────────────────────
        val_var.enforce_equal(&value_pub)?;

        // ── Kısıt 4: recipient hash ────────────────────────────────────────────
        // recipient_hash = mimc_sponge([recipient_fr])
        let rh_computed = mimc_sponge_r1cs(cs, &[recipient_var])?;
        rh_computed.enforce_equal(&recipient_hash_pub)?;

        Ok(())
    }
}

/// Recipient hash hesaplama (native) — BurnCircuit::new ile tutarlı
pub fn compute_recipient_hash(recipient: &[u8; 20]) -> [u8; 32] {
    use crate::circuits::mimc::{bytes_to_fr, fr_to_bytes, mimc_sponge};
    let mut padded = [0u8; 32];
    padded[..20].copy_from_slice(recipient);
    let h = mimc_sponge(&[bytes_to_fr(&padded)]);
    fr_to_bytes(h)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::{mimc_note_commitment, mimc_nullifier};

    fn nsc_asset() -> [u8; 32] {
        let mut a = [0u8; 32];
        a[0] = 0x4E; a[1] = 0x53; a[2] = 0x43; a[3] = 0x01;
        a
    }

    fn build_burn(value: u128, recipient: [u8; 20]) -> BurnCircuit {
        let asset = nsc_asset();
        let rand = [0x11u8; 32];
        let tk   = [0x22u8; 32];
        let nk   = [0x33u8; 32];

        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let nf = mimc_nullifier(&nk, &cm);
        let rh = compute_recipient_hash(&recipient);

        BurnCircuit::new(value, rand, asset, tk, nk, recipient, nf, rh)
    }

    #[test]
    fn test_burn_circuit_satisfied() {
        let circuit = build_burn(500, [0xABu8; 20]);
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
        println!("BurnCircuit constraint: {}", cs.num_constraints());
    }

    #[test]
    fn test_burn_wrong_value_fails() {
        let asset = nsc_asset();
        let rand = [0x11u8; 32];
        let tk   = [0x22u8; 32];
        let nk   = [0x33u8; 32];
        let recipient = [0xABu8; 20];

        let cm = mimc_note_commitment(500, &rand, &asset, &tk);
        let nf = mimc_nullifier(&nk, &cm);
        let rh = compute_recipient_hash(&recipient);

        // value public input yanlış (500 yerine 999)
        let mut circuit = BurnCircuit::new(500, rand, asset, tk, nk, recipient, nf, rh);
        circuit.value = Some(999); // yanlış değer

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış değer kısıtı bozmalı");
    }

    #[test]
    fn test_burn_setup_dummy() {
        let circuit = BurnCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
