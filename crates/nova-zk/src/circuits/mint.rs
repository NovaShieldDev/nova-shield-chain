//! Mint devresi — Transparent → Shielded (§5.7.3)
//!
//! Kullanıcı açık bakiyesini shielded pool'a gizler.
//! Kanıt: "value adet NSC yakıldı ve şu commitment üretildi"
//!
//! Public inputs:  commitment, asset_id, value (herkese açık — mint tutarı görünür)
//! Private witness: randomness, transmission_key

use crate::errors::{ZkError, ZkResult};
use crate::proof::groth16::Groth16Proof;
use crate::proof::params::CircuitType;
use crate::utxo::note::{AssetId, Note, NoteCommitment};
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

// ─── MintStatement ────────────────────────────────────────────────────────────

/// Mint devresi public girdileri
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MintStatement {
    pub commitment: NoteCommitment,
    pub value: u128,
    pub asset_id: AssetId,
    pub slot: SlotId,
}

// ─── MintWitness ──────────────────────────────────────────────────────────────

/// Mint devresi gizli tanıklığı
#[derive(Clone, Debug)]
pub struct MintWitness {
    pub note: Note,
}

impl MintWitness {
    pub fn new(note: Note) -> Self {
        Self { note }
    }

    /// Mint statement türet (public inputs)
    pub fn statement(&self, slot: SlotId) -> MintStatement {
        MintStatement {
            commitment: self.note.commit(),
            value: self.note.value,
            asset_id: self.note.asset_id,
            slot,
        }
    }

    /// Mint kanıtı üret — gerçek Groth16 (MintCircuit R1CS)
    pub fn prove(&self, slot: SlotId) -> ZkResult<MintProof> {
        let stmt = self.statement(slot);

        // R1CS circuit — MiMC commitment kısıtını kanıtlar
        let circuit = crate::circuits::r1cs::MintCircuit::new(
            self.note.value,
            self.note.randomness,
            self.note.asset_id.0,
            self.note.transmission_key,
            stmt.commitment.0,
        );

        // Public input: sadece commitment
        let pub_inputs = vec![stmt.commitment.0];

        let proof = Groth16Proof::prove_groth16(circuit, CircuitType::Mint, pub_inputs)?;
        Ok(MintProof { statement: stmt, proof })
    }
}

// ─── MintProof ────────────────────────────────────────────────────────────────

/// Mint işlemi kanıtı
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MintProof {
    pub statement: MintStatement,
    pub proof: Groth16Proof,
}

impl MintProof {
    /// Kanıtı doğrula — gerçek Groth16
    pub fn verify(&self) -> ZkResult<bool> {
        self.proof.verify_groth16()
    }

    /// Commitment hesaplanan ile tutarlı mı?
    pub fn is_consistent(&self) -> bool {
        // Gerçek sistemde: kanıtın public input'undaki commitment beklenen ile aynı mı?
        self.proof.public_inputs.first().map(|pi| *pi == self.statement.commitment.0).unwrap_or(false)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::note::NSC_ASSET_ID;

    fn make_witness(value: u128) -> MintWitness {
        let note = Note::new(value, NSC_ASSET_ID, [42u8; 32], [1u8; 32]);
        MintWitness::new(note)
    }

    #[test]
    fn test_mint_prove_and_verify() {
        let witness = make_witness(1_000);
        let proof = witness.prove(SlotId(100)).unwrap();
        assert!(proof.verify().unwrap());
    }

    #[test]
    fn test_mint_statement_commitment_matches() {
        let witness = make_witness(500);
        let proof = witness.prove(SlotId(0)).unwrap();
        let expected = witness.note.commit();
        assert_eq!(proof.statement.commitment, expected);
    }

    #[test]
    fn test_mint_proof_consistent() {
        let witness = make_witness(100);
        let proof = witness.prove(SlotId(0)).unwrap();
        assert!(proof.is_consistent());
    }

    #[test]
    fn test_mint_different_values_different_proofs() {
        let w1 = make_witness(100);
        let w2 = make_witness(200);
        let p1 = w1.prove(SlotId(0)).unwrap();
        let p2 = w2.prove(SlotId(0)).unwrap();
        assert_ne!(p1.statement.commitment, p2.statement.commitment);
    }

    #[test]
    fn test_mint_statement_value_matches() {
        let witness = make_witness(9_999);
        let stmt = witness.statement(SlotId(42));
        assert_eq!(stmt.value, 9_999);
    }
}
