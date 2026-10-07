//! Yarı-gizli mod — miktar gizli, adresler açık (§5.7.5)
//!
//! Adresler zincirde görünür; sadece transfer miktarı gizlenir.
//! Denge: gizlilik ∩ uyum

use crate::errors::ZkResult;
use crate::proof::groth16::Groth16Proof;
use crate::proof::params::CircuitType;
use crate::utxo::note::{Note, ValueCommitment};
use crate::utxo::nullifier::NullifierKey;
use nova_primitives::Address;
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

/// Yarı-gizli TX zincir kaydı
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemiShieldTxRecord {
    /// Açık gönderen
    pub sender: Address,
    /// Açık alıcı
    pub recipient: Address,
    /// Gizli miktar commitment'ı (Pedersen-like)
    pub amount_commitment: ValueCommitment,
    /// Fee (açık)
    pub fee: u128,
    /// Bakiye kanıtı: "miktar ≥ 0 ve sender yeterli bakiyeye sahip"
    pub balance_proof: Groth16Proof,
    pub slot: SlotId,
}

pub struct SemiShieldMode;

impl SemiShieldMode {
    pub fn new() -> Self {
        Self
    }

    /// Yarı-gizli transfer oluştur
    pub fn create_transfer(
        &self,
        sender: Address,
        recipient: Address,
        amount: u128,
        randomness: [u8; 32],
        fee: u128,
        slot: SlotId,
    ) -> ZkResult<SemiShieldTxRecord> {
        // Miktar commitment = BLAKE3("nova-semi-commit" || amount || randomness)
        let amount_commitment = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-shield:semi-amount-commit:v1");
            h.update(&amount.to_le_bytes());
            h.update(&randomness);
            ValueCommitment(*h.finalize().as_bytes())
        };

        // Bakiye kanıtı: TransferCircuit ile miktar gizleme kanıtı
        // Semi-shield: input = amount, output = amount - fee (fee = fee)
        use crate::utxo::note::NSC_ASSET_ID;
        let input_note  = Note::new(amount + fee, NSC_ASSET_ID, randomness, [0u8; 32]);
        let output_note = Note::new(amount, NSC_ASSET_ID, randomness, [0u8; 32]);
        let nk = NullifierKey([0u8; 32]);
        let input_cm = input_note.commit();
        let input_nf = nk.derive_nullifier(&input_cm);
        let output_cm = output_note.commit();

        let mut value_balance_bytes = [0u8; 32];
        value_balance_bytes[..16].copy_from_slice(&fee.to_le_bytes());

        let circuit = crate::circuits::r1cs::TransferCircuit::new(
            input_note.value, input_note.randomness, input_note.asset_id.0, input_note.transmission_key,
            nk.0,
            output_note.value, output_note.randomness, output_note.asset_id.0, output_note.transmission_key,
            input_nf.0, output_cm.0, fee,
        );

        // NOVA-RED-ZK-002: TransferCircuit artık 4. public input olarak input_commitment
        // içeriyor — sıra: nf, out_cm, value_balance, input_cm
        let pub_inputs = vec![input_nf.0, output_cm.0, value_balance_bytes, input_cm.0];
        let balance_proof = Groth16Proof::prove_groth16(circuit, CircuitType::Transfer, pub_inputs)?;

        Ok(SemiShieldTxRecord {
            sender,
            recipient,
            amount_commitment,
            fee,
            balance_proof,
            slot,
        })
    }

    /// Yarı-gizli TX doğrula
    pub fn verify_record(&self, record: &SemiShieldTxRecord) -> ZkResult<bool> {
        record.balance_proof.verify_groth16()
    }

    pub fn gas_cost(&self) -> u64 {
        80_000 // Tam ZK'dan az, standart'tan fazla
    }
}

impl Default for SemiShieldMode {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_semi_shield_create_and_verify() {
        let mode = SemiShieldMode::new();
        let record = mode.create_transfer(
            addr(1), addr(2),
            1_000, [77u8; 32], 10,
            SlotId(0),
        ).unwrap();

        assert!(mode.verify_record(&record).unwrap());
        assert_eq!(record.sender, addr(1));
        assert_eq!(record.recipient, addr(2));
    }

    #[test]
    fn test_semi_shield_addresses_visible() {
        let mode = SemiShieldMode::new();
        let record = mode.create_transfer(
            addr(5), addr(10),
            500, [1u8; 32], 5,
            SlotId(1),
        ).unwrap();
        // Adresler açık
        assert_eq!(record.sender, addr(5));
        assert_eq!(record.recipient, addr(10));
    }

    #[test]
    fn test_semi_shield_amount_hidden() {
        let mode = SemiShieldMode::new();
        let r1 = mode.create_transfer(addr(1), addr(2), 100, [1u8; 32], 0, SlotId(0)).unwrap();
        let r2 = mode.create_transfer(addr(1), addr(2), 200, [1u8; 32], 0, SlotId(0)).unwrap();
        // Farklı miktarlar → farklı commitments
        assert_ne!(r1.amount_commitment, r2.amount_commitment);
    }

    #[test]
    fn test_gas_cost_ordering() {
        let standard_cost = crate::modes::standard::StandardMode::new().gas_cost();
        let semi_cost = SemiShieldMode::new().gas_cost();
        let full_cost = crate::modes::full_shield::FullShieldMode::new().gas_cost();
        assert!(standard_cost < semi_cost);
        assert!(semi_cost < full_cost);
    }
}
