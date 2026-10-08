//! UnstakeCircuit R1CS — Shielded unstaking kanıtı
//!
//! Kanıt: "Bu stake benim, geri alıyorum ve yeni shielded not oluşturuyorum"
//!
//! Public inputs:
//!   1. original_stake_nullifier_fr  — orijinal stake nullifier'ı
//!   2. new_commitment_fr            — yeni shielded output commitment
//!   3. unbonded_amount_fr           — geri alınan miktar
//!
//! Private witness:
//!   original note (value, rand, asset, tk), nk, new output note (value, rand, asset, tk)

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_note_commitment, domain_nullifier, domain_stake, mimc_sponge_r1cs,
    u128_to_fr,
};

// ─── UnstakeCircuit ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct UnstakeCircuit {
    // Public
    pub original_nullifier: Option<[u8; 32]>,
    pub new_commitment:     Option<[u8; 32]>,
    pub unbonded_amount:    Option<u128>,
    // Private — orijinal stake notu
    pub orig_value:         Option<u128>,
    pub orig_randomness:    Option<[u8; 32]>,
    pub orig_asset_id:      Option<[u8; 32]>,
    pub orig_transmission_key: Option<[u8; 32]>,
    pub nullifier_key:      Option<[u8; 32]>,
    // Private — yeni çıktı notu
    pub new_value:          Option<u128>,
    pub new_randomness:     Option<[u8; 32]>,
    pub new_asset_id:       Option<[u8; 32]>,
    pub new_transmission_key: Option<[u8; 32]>,
}

impl UnstakeCircuit {
    pub fn new(
        orig_value: u128,
        orig_randomness: [u8; 32],
        orig_asset_id: [u8; 32],
        orig_transmission_key: [u8; 32],
        nullifier_key: [u8; 32],
        new_value: u128,
        new_randomness: [u8; 32],
        new_asset_id: [u8; 32],
        new_transmission_key: [u8; 32],
        original_nullifier: [u8; 32],
        new_commitment: [u8; 32],
        unbonded_amount: u128,
    ) -> Self {
        Self {
            original_nullifier: Some(original_nullifier),
            new_commitment: Some(new_commitment),
            unbonded_amount: Some(unbonded_amount),
            orig_value: Some(orig_value),
            orig_randomness: Some(orig_randomness),
            orig_asset_id: Some(orig_asset_id),
            orig_transmission_key: Some(orig_transmission_key),
            nullifier_key: Some(nullifier_key),
            new_value: Some(new_value),
            new_randomness: Some(new_randomness),
            new_asset_id: Some(new_asset_id),
            new_transmission_key: Some(new_transmission_key),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            original_nullifier: None, new_commitment: None, unbonded_amount: None,
            orig_value: None, orig_randomness: None, orig_asset_id: None,
            orig_transmission_key: None, nullifier_key: None,
            new_value: None, new_randomness: None, new_asset_id: None,
            new_transmission_key: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for UnstakeCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ── Private: orijinal stake notu ───────────────────────────────────────
        let ov_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.orig_value.unwrap_or(0)))
        })?;
        let or_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.orig_randomness.unwrap_or([0u8; 32])))
        })?;
        let oa_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.orig_asset_id.unwrap_or([0u8; 32])))
        })?;
        let ot_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.orig_transmission_key.unwrap_or([0u8; 32])))
        })?;
        let nk_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.nullifier_key.unwrap_or([0u8; 32])))
        })?;
        // ── Private: yeni çıktı notu ───────────────────────────────────────────
        let nv_var = FpVar::new_witness(cs.clone(), || {
            Ok(u128_to_fr(self.new_value.unwrap_or(0)))
        })?;
        let nr_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.new_randomness.unwrap_or([0u8; 32])))
        })?;
        let na_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.new_asset_id.unwrap_or([0u8; 32])))
        })?;
        let nt_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.new_transmission_key.unwrap_or([0u8; 32])))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let orig_nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.original_nullifier.unwrap_or([0u8; 32])))
        })?;
        let new_cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.new_commitment.unwrap_or([0u8; 32])))
        })?;
        let amount_pub = FpVar::new_input(cs.clone(), || {
            Ok(u128_to_fr(self.unbonded_amount.unwrap_or(0)))
        })?;

        // ── Kısıt 1: orijinal note commitment ─────────────────────────────────
        let domain_note = FpVar::Constant(domain_note_commitment());
        let orig_cm = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note.clone(), ov_var.clone(), or_var, oa_var, ot_var],
        )?;

        // ── Kısıt 2: orijinal stake nullifier ─────────────────────────────────
        let domain_sk = FpVar::Constant(domain_stake());
        let orig_nf = mimc_sponge_r1cs(cs.clone(), &[domain_sk, nk_var, orig_cm])?;
        orig_nf.enforce_equal(&orig_nf_pub)?;

        // ── Kısıt 3: yeni çıktı commitment ────────────────────────────────────
        let new_cm = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note, nv_var.clone(), nr_var, na_var, nt_var],
        )?;
        new_cm.enforce_equal(&new_cm_pub)?;

        // ── Kısıt 4: unbonded amount tutarlılığı ──────────────────────────────
        nv_var.enforce_equal(&amount_pub)?;

        // ── Kısıt 5 (NOVA-RED-ZK-007): new_value <= orig_value ───────────────
        // diff = orig_value - new_value ≥ 0 (range check: diff ∈ [0, 2^128))
        // Eğer new_value > orig_value → diff büyük bir sayıdır → kısıt başarısız.
        {
            let diff = ov_var.clone() - nv_var.clone();
            let diff_bits = diff.to_bits_le()?;
            for bit in &diff_bits[128..] {
                bit.enforce_equal(&Boolean::constant(false))?;
            }
        }

        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::{mimc_note_commitment, mimc_sponge, fr_to_bytes};
    use crate::circuits::r1cs::stake::compute_stake_nullifier;

    fn nsc() -> [u8; 32] {
        let mut a = [0u8; 32]; a[0]=0x4E; a[1]=0x53; a[2]=0x43; a[3]=0x01; a
    }

    #[test]
    fn test_unstake_circuit_satisfied() {
        let asset = nsc();
        let or_rand = [0x01u8; 32]; let or_tk = [0x02u8; 32];
        let nk = [0x03u8; 32];
        let nr_rand = [0x04u8; 32]; let nr_tk = [0x05u8; 32];
        let orig_value = 5_000u128;
        let new_value  = 5_000u128; // tüm miktarı geri al

        let orig_cm = mimc_note_commitment(orig_value, &or_rand, &asset, &or_tk);
        let orig_nf = compute_stake_nullifier(&nk, &orig_cm);
        let new_cm  = mimc_note_commitment(new_value, &nr_rand, &asset, &nr_tk);

        let circuit = UnstakeCircuit::new(
            orig_value, or_rand, asset, or_tk,
            nk,
            new_value, nr_rand, asset, nr_tk,
            orig_nf, new_cm, new_value,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
        println!("UnstakeCircuit constraint: {}", cs.num_constraints());
    }

    #[test]
    fn test_unstake_setup_dummy() {
        let circuit = UnstakeCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
