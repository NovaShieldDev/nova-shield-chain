//! Tam gizlilik modu — mempool + zincir şifreli (§5.7.5)
//!
//! Zincire sadece: proof + nullifier + commitment yazılır.
//! Kim, kime, ne kadar? → Bilinmez.

use crate::circuits::transfer::{TransferProof, TransferWitness};
use crate::errors::ZkResult;
use crate::utxo::note::Note;
use crate::utxo::nullifier::NullifierKey;
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

/// Tam gizlilik TX zincir kaydı (şifreli)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FullShieldTxRecord {
    /// Harcanan note'un nullifier'ı
    pub nullifier_hex: String,
    /// Yeni note'un commitment'ı
    pub commitment_hex: String,
    /// Fee (açık — validator görmeli)
    pub fee: u128,
    /// ZK kanıt
    pub proof: TransferProof,
}

/// Tam gizlilik modu handler
pub struct FullShieldMode;

impl FullShieldMode {
    pub fn new() -> Self {
        Self
    }

    /// Tam gizlilik transferi gerçekleştir
    pub fn create_transfer(
        &self,
        input_note: Note,
        nk: NullifierKey,
        output_note: Note,
        slot: SlotId,
    ) -> ZkResult<FullShieldTxRecord> {
        let witness = TransferWitness::new(input_note, nk, output_note)?;
        let stmt = witness.statement(slot);
        let proof = witness.prove(slot)?;

        Ok(FullShieldTxRecord {
            nullifier_hex: stmt.input_nullifier.to_hex(),
            commitment_hex: stmt.output_commitment.to_hex(),
            fee: stmt.value_balance,
            proof,
        })
    }

    /// Gas maliyeti (ZK kanıt nedeniyle daha yüksek)
    pub fn gas_cost(&self) -> u64 {
        200_000 // Groth16 doğrulama maliyeti
    }

    /// Zincir kaydı geçerli mi?
    pub fn verify_record(&self, record: &FullShieldTxRecord) -> ZkResult<bool> {
        record.proof.verify()
    }
}

impl Default for FullShieldMode {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utxo::note::NSC_ASSET_ID;

    fn make_note(value: u128, rng: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [rng; 32], [rng + 1; 32])
    }

    #[test]
    fn test_full_shield_transfer() {
        let mode = FullShieldMode::new();
        let nk = NullifierKey([9u8; 32]);
        let record = mode.create_transfer(
            make_note(1_000, 1),
            nk,
            make_note(900, 2),
            SlotId(50),
        ).unwrap();

        assert!(mode.verify_record(&record).unwrap());
        assert_eq!(record.fee, 100); // 1000 - 900
    }

    #[test]
    fn test_full_shield_nullifier_in_record() {
        let mode = FullShieldMode::new();
        let nk = NullifierKey([3u8; 32]);
        let input = make_note(500, 5);
        let record = mode.create_transfer(input, nk, make_note(500, 6), SlotId(0)).unwrap();
        assert_eq!(record.nullifier_hex.len(), 64);
    }

    #[test]
    fn test_full_shield_gas_cost() {
        assert!(FullShieldMode::new().gas_cost() > crate::modes::standard::StandardMode::new().gas_cost());
    }
}
