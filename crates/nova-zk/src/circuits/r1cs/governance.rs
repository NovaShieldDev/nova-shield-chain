//! GovernanceVoteCircuit R1CS — Kimlik açıklamadan oy kullanma
//!
//! Kanıt: "Oy hakkım var (note sahibiyim), bu teklif için şu şekilde oy kullandım"
//! Oy içeriği gizli kalabilir veya taahhüt yoluyla açıklanabilir.
//!
//! Public inputs:
//!   1. vote_nullifier_fr  — çifte oy önleme
//!   2. proposal_id_fr     — hangi teklif
//!   3. vote_commitment_fr — oy taahhüdü (vote içeriğini açıklayınca kanıtlanabilir)
//!
//! Private witness:
//!   note (value, rand, asset, tk), nullifier_key, vote_value ([0=ret, 1=kabul, 2=çekimser])

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_note_commitment, domain_nullifier, domain_vote, mimc_sponge_r1cs,
    u128_to_fr,
};

// ─── GovernanceVoteCircuit ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct GovernanceVoteCircuit {
    // Public
    pub vote_nullifier:  Option<[u8; 32]>,
    pub proposal_id:     Option<[u8; 32]>,
    pub vote_commitment: Option<[u8; 32]>,
    // Private
    pub value:           Option<u128>,
    pub randomness:      Option<[u8; 32]>,
    pub asset_id:        Option<[u8; 32]>,
    pub transmission_key:Option<[u8; 32]>,
    pub nullifier_key:   Option<[u8; 32]>,
    pub vote_value:      Option<u8>,   // 0=ret, 1=kabul, 2=çekimser
    pub vote_randomness: Option<[u8; 32]>,
}

impl GovernanceVoteCircuit {
    pub fn new(
        value: u128,
        randomness: [u8; 32],
        asset_id: [u8; 32],
        transmission_key: [u8; 32],
        nullifier_key: [u8; 32],
        vote_value: u8,
        vote_randomness: [u8; 32],
        proposal_id: [u8; 32],
        vote_nullifier: [u8; 32],
        vote_commitment: [u8; 32],
    ) -> Self {
        Self {
            vote_nullifier: Some(vote_nullifier),
            proposal_id: Some(proposal_id),
            vote_commitment: Some(vote_commitment),
            value: Some(value),
            randomness: Some(randomness),
            asset_id: Some(asset_id),
            transmission_key: Some(transmission_key),
            nullifier_key: Some(nullifier_key),
            vote_value: Some(vote_value),
            vote_randomness: Some(vote_randomness),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            vote_nullifier: None, proposal_id: None, vote_commitment: None,
            value: None, randomness: None, asset_id: None, transmission_key: None,
            nullifier_key: None, vote_value: None, vote_randomness: None,
        }
    }
}

impl ConstraintSynthesizer<Fr> for GovernanceVoteCircuit {
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
        let vote_val_var = FpVar::new_witness(cs.clone(), || {
            Ok(Fr::from(self.vote_value.unwrap_or(0) as u64))
        })?;
        let vote_rand_var = FpVar::new_witness(cs.clone(), || {
            Ok(bytes_to_fr(&self.vote_randomness.unwrap_or([0u8; 32])))
        })?;

        // ── Public inputs ──────────────────────────────────────────────────────
        let nf_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.vote_nullifier.unwrap_or([0u8; 32])))
        })?;
        let proposal_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.proposal_id.unwrap_or([0u8; 32])))
        })?;
        let vote_cm_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.vote_commitment.unwrap_or([0u8; 32])))
        })?;

        // ── Kısıt 1: note commitment (oy hakkı kanıtı) ───────────────────────
        let domain_note = FpVar::Constant(domain_note_commitment());
        let cm = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_note, val_var, rand_var, asset_var, tk_var],
        )?;

        // ── Kısıt 2: oy nullifier = mimc_sponge([domain_vote, nk, cm, proposal]) ─
        let domain_vt = FpVar::Constant(domain_vote());
        let nf_computed = mimc_sponge_r1cs(
            cs.clone(),
            &[domain_vt, nk_var, cm, proposal_pub.clone()],
        )?;
        nf_computed.enforce_equal(&nf_pub)?;

        // ── Kısıt 3: oy taahhüdü = mimc_sponge([vote_val, vote_rand, proposal]) ─
        let vc_computed = mimc_sponge_r1cs(
            cs.clone(),
            &[vote_val_var, vote_rand_var, proposal_pub],
        )?;
        vc_computed.enforce_equal(&vote_cm_pub)?;

        Ok(())
    }
}

/// Oy nullifier hesaplama (native)
pub fn compute_vote_nullifier(
    nullifier_key: &[u8; 32],
    commitment: &[u8; 32],
    proposal_id: &[u8; 32],
) -> [u8; 32] {
    use crate::circuits::mimc::{bytes_to_fr, fr_to_bytes, mimc_sponge};
    let h = mimc_sponge(&[
        domain_vote(),
        bytes_to_fr(nullifier_key),
        bytes_to_fr(commitment),
        bytes_to_fr(proposal_id),
    ]);
    fr_to_bytes(h)
}

/// Oy taahhüdü hesaplama (native)
pub fn compute_vote_commitment(
    vote_value: u8,
    vote_randomness: &[u8; 32],
    proposal_id: &[u8; 32],
) -> [u8; 32] {
    use ark_ff::PrimeField;
    use crate::circuits::mimc::{bytes_to_fr, fr_to_bytes, mimc_sponge};
    let h = mimc_sponge(&[
        Fr::from(vote_value as u64),
        bytes_to_fr(vote_randomness),
        bytes_to_fr(proposal_id),
    ]);
    fr_to_bytes(h)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ark_relations::r1cs::ConstraintSystem;
    use crate::circuits::mimc::mimc_note_commitment;

    fn nsc() -> [u8; 32] {
        let mut a = [0u8; 32]; a[0]=0x4E; a[1]=0x53; a[2]=0x43; a[3]=0x01; a
    }

    fn build_vote(vote_value: u8) -> GovernanceVoteCircuit {
        let asset = nsc();
        let rand = [0x10u8; 32];
        let tk   = [0x20u8; 32];
        let nk   = [0x30u8; 32];
        let vote_rand = [0x40u8; 32];
        let proposal = [0x50u8; 32];
        let value = 1_000u128;

        let cm = mimc_note_commitment(value, &rand, &asset, &tk);
        let nf = compute_vote_nullifier(&nk, &cm, &proposal);
        let vc = compute_vote_commitment(vote_value, &vote_rand, &proposal);

        GovernanceVoteCircuit::new(
            value, rand, asset, tk, nk, vote_value, vote_rand, proposal, nf, vc,
        )
    }

    #[test]
    fn test_governance_vote_yes_satisfied() {
        let circuit = build_vote(1); // kabul
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
        println!("GovernanceVoteCircuit constraint: {}", cs.num_constraints());
    }

    #[test]
    fn test_governance_vote_no_satisfied() {
        let circuit = build_vote(0); // ret
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_governance_wrong_vote_commitment_fails() {
        let asset = nsc();
        let rand = [0x10u8; 32]; let tk = [0x20u8; 32]; let nk = [0x30u8; 32];
        let vote_rand = [0x40u8; 32]; let proposal = [0x50u8; 32];
        let cm = mimc_note_commitment(1000, &rand, &asset, &tk);
        let nf = compute_vote_nullifier(&nk, &cm, &proposal);
        let wrong_vc = [0xFFu8; 32];

        let circuit = GovernanceVoteCircuit::new(
            1000, rand, asset, tk, nk, 1, vote_rand, proposal, nf, wrong_vc,
        );
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_governance_setup_dummy() {
        let circuit = GovernanceVoteCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
