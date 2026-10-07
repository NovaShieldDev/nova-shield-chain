//! Transfer devresi — Shielded → Shielded (§5.7.3)
//!
//! Gizli transfer: kimse kimin kime ne kadar gönderdiğini bilmiyor.
//! Kanıt: "girdi nullifier geçerli && çıktı commitment geçerli && değer dengesi sıfır"
//!
//! Public inputs:  input_nullifier, output_commitment, fee_commitment
//! Private witness: input_note, nullifier_key, output_note, randomness

use crate::errors::{ZkError, ZkResult};
use crate::proof::groth16::Groth16Proof;
use crate::proof::params::CircuitType;
use crate::utxo::note::{Note, NoteCommitment};
use crate::utxo::nullifier::{Nullifier, NullifierKey};
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

// ─── TransferStatement ────────────────────────────────────────────────────────

/// Transfer public girdileri (zincire yazılır)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferStatement {
    pub input_nullifier: Nullifier,
    pub output_commitment: NoteCommitment,
    /// Değer dengesi: girdi - çıktı (0 = dengeli, pozitif = fee)
    pub value_balance: u128,
    pub slot: SlotId,
    /// NOVA-RED-ZK-002: Girdi commitment'ı public — on-chain pool.has_commitment()
    /// kontrolüne izin verir. Kanıt, R1CS'de input_cm == input_commitment_fr ile
    /// enforce edilir; prover gerçek commitment'ı kullanmak zorundadır.
    pub input_commitment: NoteCommitment,
}

// ─── TransferWitness ──────────────────────────────────────────────────────────

/// Transfer gizli tanıklığı
pub struct TransferWitness {
    pub input_note: Note,
    pub input_commitment: NoteCommitment,
    pub nullifier_key: NullifierKey,
    pub output_note: Note,
}

impl TransferWitness {
    pub fn new(
        input_note: Note,
        nullifier_key: NullifierKey,
        output_note: Note,
    ) -> ZkResult<Self> {
        // Değer kontrolü: girdi >= çıktı (fee = girdi - çıktı)
        if input_note.value < output_note.value {
            return Err(ZkError::ValueImbalance {
                input: input_note.value,
                output: output_note.value,
            });
        }
        let input_commitment = input_note.commit();
        Ok(Self { input_note, input_commitment, nullifier_key, output_note })
    }

    pub fn statement(&self, slot: SlotId) -> TransferStatement {
        let input_nullifier = self.nullifier_key.derive_nullifier(&self.input_commitment);
        let output_commitment = self.output_note.commit();
        let value_balance = self.input_note.value - self.output_note.value; // fee
        // NOVA-RED-ZK-002: input_commitment public statement'a ekleniyor
        let input_commitment = self.input_commitment;
        TransferStatement { input_nullifier, output_commitment, value_balance, slot, input_commitment }
    }

    pub fn prove(&self, slot: SlotId) -> ZkResult<TransferProof> {
        let stmt = self.statement(slot);

        let mut value_balance_bytes = [0u8; 32];
        value_balance_bytes[..16].copy_from_slice(&stmt.value_balance.to_le_bytes());

        let circuit = crate::circuits::r1cs::TransferCircuit::new(
            self.input_note.value,
            self.input_note.randomness,
            self.input_note.asset_id.0,
            self.input_note.transmission_key,
            self.nullifier_key.0,
            self.output_note.value,
            self.output_note.randomness,
            self.output_note.asset_id.0,
            self.output_note.transmission_key,
            stmt.input_nullifier.0,
            stmt.output_commitment.0,
            stmt.value_balance,
        );

        // NOVA-RED-ZK-002: 4. public input — input_commitment (R1CS sırası ile aynı)
        // R1CS sırası: nf_pub, out_cm_pub, balance_pub, in_cm_pub
        let pub_inputs = vec![
            stmt.input_nullifier.0,
            stmt.output_commitment.0,
            value_balance_bytes,
            stmt.input_commitment.0,
        ];

        let proof = Groth16Proof::prove_groth16(circuit, CircuitType::Transfer, pub_inputs)?;
        Ok(TransferProof { statement: stmt, proof })
    }
}

// ─── TransferProof ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferProof {
    pub statement: TransferStatement,
    pub proof: Groth16Proof,
}

impl TransferProof {
    pub fn verify(&self) -> ZkResult<bool> {
        self.proof.verify_groth16()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::note::NSC_ASSET_ID;

    fn make_note(value: u128, rng: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [rng; 32], [rng + 50; 32])
    }

    fn make_nk() -> NullifierKey {
        NullifierKey([7u8; 32])
    }

    #[test]
    fn test_transfer_prove_and_verify() {
        let input = make_note(1_000, 1);
        let output = make_note(900, 2); // 100 fee
        let witness = TransferWitness::new(input, make_nk(), output).unwrap();
        let proof = witness.prove(SlotId(10)).unwrap();
        assert!(proof.verify().unwrap());
    }

    #[test]
    fn test_transfer_value_balance() {
        let input = make_note(1_000, 1);
        let output = make_note(800, 2);
        let witness = TransferWitness::new(input, make_nk(), output).unwrap();
        let stmt = witness.statement(SlotId(0));
        assert_eq!(stmt.value_balance, 200); // fee
    }

    #[test]
    fn test_transfer_overdraft_rejected() {
        let input = make_note(100, 1);
        let output = make_note(200, 2); // çıktı > girdi
        let err = TransferWitness::new(input, make_nk(), output);
        assert!(matches!(err, Err(ZkError::ValueImbalance { .. })));
    }

    #[test]
    fn test_transfer_nullifier_derived() {
        let input = make_note(500, 3);
        let output = make_note(500, 4);
        let nk = make_nk();
        let witness = TransferWitness::new(input.clone(), nk, output).unwrap();
        let expected_nullifier = nk.derive_nullifier(&input.commit());
        let stmt = witness.statement(SlotId(0));
        assert_eq!(stmt.input_nullifier, expected_nullifier);
    }

    #[test]
    fn test_transfer_zero_fee_allowed() {
        let input = make_note(500, 5);
        let output = make_note(500, 6); // tam değer transferi
        let witness = TransferWitness::new(input, make_nk(), output).unwrap();
        let stmt = witness.statement(SlotId(0));
        assert_eq!(stmt.value_balance, 0);
    }
}
