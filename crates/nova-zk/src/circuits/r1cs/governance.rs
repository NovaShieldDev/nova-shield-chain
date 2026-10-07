//! GovernanceVoteCircuit R1CS — Kimlik açıklamadan oy kullanma
//!
//! Kanıt: "Oy hakkım var (note sahibiyim, Merkle tree'de), bu teklif için oy kullandım"
//! Oy içeriği gizli kalabilir veya taahhüt yoluyla açıklanabilir.
//!
//! Public inputs:
//!   1. vote_nullifier_fr  — çifte oy önleme
//!   2. proposal_id_fr     — hangi teklif
//!   3. vote_commitment_fr — oy taahhüdü (vote içeriğini açıklayınca kanıtlanabilir)
//!   4. anchor_fr          — commitment Merkle tree kökü (NOVA-RED-ZK-003 fix)
//!
//! Private witness:
//!   note (value, rand, asset, tk), nullifier_key, vote_value,
//!   merkle_path (NOVA-RED-ZK-003: note ∈ tree kanıtı)

use ark_bls12_381::Fr;
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::circuits::mimc::{
    bytes_to_fr, domain_merkle, domain_note_commitment, domain_nullifier, domain_vote,
    mimc_sponge_r1cs, u128_to_fr, MERKLE_DEPTH,
};

// ─── GovernanceVoteCircuit ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct GovernanceVoteCircuit {
    // Public
    pub vote_nullifier:  Option<[u8; 32]>,
    pub proposal_id:     Option<[u8; 32]>,
    pub vote_commitment: Option<[u8; 32]>,
    /// NOVA-RED-ZK-003: Commitment tree Merkle root (public input).
    pub anchor:          Option<[u8; 32]>,
    // Private
    pub value:           Option<u128>,
    pub randomness:      Option<[u8; 32]>,
    pub asset_id:        Option<[u8; 32]>,
    pub transmission_key:Option<[u8; 32]>,
    pub nullifier_key:   Option<[u8; 32]>,
    pub vote_value:      Option<u8>,   // 0=ret, 1=kabul, 2=çekimser
    pub vote_randomness: Option<[u8; 32]>,
    /// NOVA-RED-ZK-003: Merkle inclusion path — note ∈ tree kanıtı.
    /// Her eleman: (sibling_hash, is_right).
    pub merkle_path:     Option<Vec<([u8; 32], bool)>>,
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
        anchor: [u8; 32],
        merkle_path: Vec<([u8; 32], bool)>,
    ) -> Self {
        Self {
            vote_nullifier: Some(vote_nullifier),
            proposal_id: Some(proposal_id),
            vote_commitment: Some(vote_commitment),
            anchor: Some(anchor),
            value: Some(value),
            randomness: Some(randomness),
            asset_id: Some(asset_id),
            transmission_key: Some(transmission_key),
            nullifier_key: Some(nullifier_key),
            vote_value: Some(vote_value),
            vote_randomness: Some(vote_randomness),
            merkle_path: Some(merkle_path),
        }
    }

    pub fn setup_dummy() -> Self {
        Self {
            vote_nullifier: None, proposal_id: None, vote_commitment: None,
            anchor: None,
            value: None, randomness: None, asset_id: None, transmission_key: None,
            nullifier_key: None, vote_value: None, vote_randomness: None,
            merkle_path: None,
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
        // NOVA-RED-ZK-003: Merkle root public input — note'un tree'de bulunduğunu kanıtlar.
        let anchor_pub = FpVar::new_input(cs.clone(), || {
            Ok(bytes_to_fr(&self.anchor.unwrap_or([0u8; 32])))
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
            &[domain_vt, nk_var, cm.clone(), proposal_pub.clone()],
        )?;
        nf_computed.enforce_equal(&nf_pub)?;

        // ── Kısıt 3: oy taahhüdü = mimc_sponge([vote_val, vote_rand, proposal]) ─
        let vc_computed = mimc_sponge_r1cs(
            cs.clone(),
            &[vote_val_var, vote_rand_var, proposal_pub],
        )?;
        vc_computed.enforce_equal(&vote_cm_pub)?;

        // ── Kısıt 4 (NOVA-RED-ZK-003): Merkle inclusion proof ────────────────
        // Kanıt: cm ∈ commitment tree (anchor ile root eşleşmeli)
        // SpendCircuit ile aynı Merkle gadget pattern.
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
    use crate::circuits::mimc::{mimc_merkle_root_padded, mimc_note_commitment};

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
        // NOVA-RED-ZK-003: Merkle root hesapla (boş path = tek note tree)
        let anchor = mimc_merkle_root_padded(&cm, &[]);

        GovernanceVoteCircuit::new(
            value, rand, asset, tk, nk, vote_value, vote_rand, proposal, nf, vc,
            anchor, vec![],
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
        let anchor = mimc_merkle_root_padded(&cm, &[]);
        let wrong_vc = [0xFFu8; 32];

        let circuit = GovernanceVoteCircuit::new(
            1000, rand, asset, tk, nk, 1, vote_rand, proposal, nf, wrong_vc,
            anchor, vec![],
        );
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap());
    }

    /// NOVA-RED-ZK-003: Yanlış Merkle root (tree'de olmayan note) reddedilmeli.
    #[test]
    fn test_governance_wrong_anchor_fails() {
        let circuit = build_vote(1);
        // anchor'ı bozan bir wrapper oluştur
        let asset = nsc();
        let rand = [0x10u8; 32]; let tk = [0x20u8; 32]; let nk = [0x30u8; 32];
        let vote_rand = [0x40u8; 32]; let proposal = [0x50u8; 32];
        let cm = mimc_note_commitment(1000, &rand, &asset, &tk);
        let nf = compute_vote_nullifier(&nk, &cm, &proposal);
        let vc = compute_vote_commitment(1, &vote_rand, &proposal);
        let wrong_anchor = [0xFFu8; 32]; // tree'de yok

        let circuit = GovernanceVoteCircuit::new(
            1000, rand, asset, tk, nk, 1, vote_rand, proposal, nf, vc,
            wrong_anchor, vec![],
        );
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "Yanlış anchor kısıtı bozmalı");
    }

    #[test]
    fn test_governance_setup_dummy() {
        let circuit = GovernanceVoteCircuit::setup_dummy();
        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.num_constraints() > 0);
    }
}
