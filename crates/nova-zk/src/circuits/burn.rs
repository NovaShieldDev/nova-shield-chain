//! Burn devresi — Shielded → Transparent (§5.7.3)
//!
//! Kullanıcı gizli bakiyesini şeffaf adrese çeker.
//! Kanıt: "şu commitment benim, harcamadım, şu miktarı şu adrese gönder"
//!
//! Public inputs:  nullifier, value, recipient (transparent address)
//! Private witness: note, nullifier_key, commitment

use crate::errors::{ZkResult};
use crate::proof::groth16::Groth16Proof;
use crate::proof::params::CircuitType;
use crate::utxo::note::{Note, NoteCommitment};
use crate::utxo::nullifier::{Nullifier, NullifierKey};
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

// ─── BurnStatement ────────────────────────────────────────────────────────────

/// Burn public girdileri
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BurnStatement {
    pub nullifier: Nullifier,
    pub value: u128,
    /// Şeffaf alıcı adresi (görünür)
    pub recipient: [u8; 20],
    pub slot: SlotId,
}

// ─── BurnWitness ──────────────────────────────────────────────────────────────

pub struct BurnWitness {
    pub note: Note,
    pub commitment: NoteCommitment,
    pub nullifier_key: NullifierKey,
    pub recipient: [u8; 20],
}

impl BurnWitness {
    pub fn new(note: Note, nullifier_key: NullifierKey, recipient: [u8; 20]) -> Self {
        let commitment = note.commit();
        Self { note, commitment, nullifier_key, recipient }
    }

    pub fn statement(&self, slot: SlotId) -> BurnStatement {
        let nullifier = self.nullifier_key.derive_nullifier(&self.commitment);
        BurnStatement {
            nullifier,
            value: self.note.value,
            recipient: self.recipient,
            slot,
        }
    }

    pub fn prove(&self, slot: SlotId) -> ZkResult<BurnProof> {
        let stmt = self.statement(slot);

        let recipient_hash = crate::circuits::r1cs::compute_recipient_hash(&self.recipient);

        let mut value_bytes = [0u8; 32];
        value_bytes[..16].copy_from_slice(&stmt.value.to_le_bytes());

        let circuit = crate::circuits::r1cs::BurnCircuit::new(
            self.note.value,
            self.note.randomness,
            self.note.asset_id.0,
            self.note.transmission_key,
            self.nullifier_key.0,
            self.recipient,
            stmt.nullifier.0,
            recipient_hash,
        );

        let pub_inputs = vec![
            stmt.nullifier.0,
            value_bytes,
            recipient_hash,
        ];

        let proof = Groth16Proof::prove_groth16(circuit, CircuitType::Burn, pub_inputs)?;
        Ok(BurnProof { statement: stmt, proof })
    }
}

// ─── BurnProof ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BurnProof {
    pub statement: BurnStatement,
    pub proof: Groth16Proof,
}

impl BurnProof {
    pub fn verify(&self) -> ZkResult<bool> {
        self.proof.verify_groth16()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::note::NSC_ASSET_ID;

    fn make_note(value: u128) -> Note {
        Note::new(value, NSC_ASSET_ID, [33u8; 32], [44u8; 32])
    }

    fn make_nk() -> NullifierKey {
        NullifierKey([5u8; 32])
    }

    fn recipient() -> [u8; 20] {
        [0xABu8; 20]
    }

    #[test]
    fn test_burn_prove_and_verify() {
        let witness = BurnWitness::new(make_note(500), make_nk(), recipient());
        let proof = witness.prove(SlotId(0)).unwrap();
        assert!(proof.verify().unwrap());
    }

    #[test]
    fn test_burn_statement_correct() {
        let note = make_note(1_500);
        let nk = make_nk();
        let witness = BurnWitness::new(note.clone(), nk, recipient());
        let stmt = witness.statement(SlotId(5));
        assert_eq!(stmt.value, 1_500);
        assert_eq!(stmt.recipient, recipient());

        // Nullifier doğru türetilmiş mi?
        let expected_null = nk.derive_nullifier(&note.commit());
        assert_eq!(stmt.nullifier, expected_null);
    }

    #[test]
    fn test_burn_different_recipients_different_proofs() {
        let w1 = BurnWitness::new(make_note(100), make_nk(), [0x01u8; 20]);
        let w2 = BurnWitness::new(make_note(100), make_nk(), [0x02u8; 20]);
        let p1 = w1.prove(SlotId(0)).unwrap();
        let p2 = w2.prove(SlotId(0)).unwrap();
        assert_ne!(p1.proof.proof_bytes, p2.proof.proof_bytes);
    }

    #[test]
    fn test_burn_full_lifecycle() {
        use crate::utxo::pool::ShieldedPool;
        let pool = ShieldedPool::new();
        let note = make_note(250);
        let nk = make_nk();

        // Önce mint et
        let cm = pool.mint(note.clone(), SlotId(0)).unwrap();
        assert!(pool.has_commitment(&cm));

        // Burn kanıtı üret
        let witness = BurnWitness::new(note, nk, recipient());
        let proof = witness.prove(SlotId(1)).unwrap();
        assert!(proof.verify().unwrap());

        // Havuzdan harca
        let out = pool.spend(&cm, &nk, SlotId(1)).unwrap();
        assert_eq!(out.note.value, 250);
        assert!(pool.is_spent(&proof.statement.nullifier));
    }
}
