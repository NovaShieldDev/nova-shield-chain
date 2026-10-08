//! Şifreli Mempool + ZK-Shield Entegrasyonu (§5.7.4)
//!
//! İki katman birlikte çalışır:
//!   1. Şifreli mempool → TX içeriği sıralama sırasında gizli
//!   2. ZK kanıt → Zincirde kalıcı gizlilik
//!
//! Akış:
//!   Kullanıcı → ZK TX oluştur → Şifreli mempool'a gönder
//!             → Batch açılır → ZK kanıt doğrula → Zincire yaz

use crate::circuits::burn::BurnProof;
use crate::circuits::mint::MintProof;
use crate::circuits::transfer::TransferProof;
use crate::errors::{ZkError, ZkResult};
use crate::modes::ShieldMode;
use crate::utxo::pool::ShieldedPool;
use nova_primitives::SlotId;
use serde::{Deserialize, Serialize};

// ─── ShieldedAction ─────────���─────────────────────────────────────────────────

/// ZK-Shield TX aksiyonu
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ShieldedAction {
    /// Transparent → Shielded
    Mint(MintProof),
    /// Shielded → Shielded
    Transfer(TransferProof),
    /// Shielded → Transparent
    Burn(BurnProof),
}

impl ShieldedAction {
    pub fn action_type(&self) -> &'static str {
        match self {
            ShieldedAction::Mint(_) => "mint",
            ShieldedAction::Transfer(_) => "transfer",
            ShieldedAction::Burn(_) => "burn",
        }
    }

    /// ZK kanıtını doğrula
    pub fn verify_proof(&self) -> ZkResult<bool> {
        match self {
            ShieldedAction::Mint(p) => p.verify(),
            ShieldedAction::Transfer(p) => p.verify(),
            ShieldedAction::Burn(p) => p.verify(),
        }
    }
}

// ─── ShieldedTx ────────────────────────────���──────────────────────────────────

/// ZK-Shield işlemi — şifreli mempool + ZK kanıt entegrasyonu
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShieldedTx {
    pub action: ShieldedAction,
    pub mode: ShieldMode,
    pub slot: SlotId,
    pub fee: u128,
}

impl ShieldedTx {
    pub fn new(action: ShieldedAction, mode: ShieldMode, slot: SlotId, fee: u128) -> Self {
        Self { action, mode, slot, fee }
    }

    /// TX geçerli mi?
    pub fn is_valid(&self) -> ZkResult<bool> {
        if self.fee == 0 && self.mode.requires_zk_proof() {
            return Err(ZkError::ValueImbalance { input: 0, output: 1 });
        }
        self.action.verify_proof()
    }
}

// ─── ZkShieldEngine ────────────────────────────���──────────────────────────────

/// ZK-Shield ana motoru — pool yönetimi + TX doğrulama
pub struct ZkShieldEngine {
    pool: ShieldedPool,
}

impl ZkShieldEngine {
    pub fn new() -> Self {
        Self { pool: ShieldedPool::new() }
    }

    /// TX doğrula ve pool'a uygula
    pub fn apply(&self, tx: &ShieldedTx) -> ZkResult<()> {
        // Kanıt doğrula
        if !tx.is_valid()? {
            return Err(ZkError::InvalidProof);
        }

        // Pool güncelleme
        match &tx.action {
            ShieldedAction::Mint(proof) => {
                let commitment = proof.statement.commitment;
                if self.pool.has_commitment(&commitment) {
                    return Err(ZkError::CommitmentAlreadyExists);
                }
                // NOVA-RED-ZK-009: Mint commitment'ı pool'a ekle
                self.pool.commitment_tree_insert(commitment)?;
            }
            ShieldedAction::Transfer(proof) => {
                // NOVA-RED-ZK-002: Girdi commitment pool'da kayıtlı mı?
                let input_cm = &proof.statement.input_commitment;
                if !self.pool.has_commitment(input_cm) {
                    return Err(ZkError::CommitmentNotFound);
                }
                // Nullifier daha önce kullanıldı mı?
                let nullifier = proof.statement.input_nullifier;
                if self.pool.is_spent(&nullifier) {
                    return Err(ZkError::NullifierAlreadySpent);
                }
                // NOVA-RED-ZK-004: Nullifier'ı spent olarak işaretle (double-spend önleme)
                self.pool.mark_nullifier_spent(nullifier, tx.slot)?;
                // Output commitment'ı pool'a ekle
                self.pool.commitment_tree_insert(proof.statement.output_commitment)?;
            }
            ShieldedAction::Burn(proof) => {
                let nullifier = proof.statement.nullifier;
                if self.pool.is_spent(&nullifier) {
                    return Err(ZkError::NullifierAlreadySpent);
                }
                // NOVA-RED-ZK-004: Nullifier'ı spent olarak işaretle
                self.pool.mark_nullifier_spent(nullifier, tx.slot)?;
            }
        }
        Ok(())
    }

    pub fn pool(&self) -> &ShieldedPool {
        &self.pool
    }

    pub fn stats(&self) -> crate::utxo::pool::PoolStats {
        self.pool.stats()
    }
}

impl Default for ZkShieldEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────���─────────────────────────────────���──────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuits::mint::MintWitness;
    use crate::circuits::transfer::TransferWitness;
    use crate::utxo::note::{NSC_ASSET_ID, Note};
    use crate::utxo::nullifier::NullifierKey;

    fn make_note(value: u128, rng: u8) -> Note {
        Note::new(value, NSC_ASSET_ID, [rng; 32], [rng + 10; 32])
    }

    fn make_mint_tx(value: u128) -> ShieldedTx {
        let witness = MintWitness::new(make_note(value, 1));
        let proof = witness.prove(SlotId(0)).unwrap();
        ShieldedTx::new(ShieldedAction::Mint(proof), ShieldMode::FullShield, SlotId(0), 1_000)
    }

    #[test]
    fn test_shielded_action_verify() {
        let tx = make_mint_tx(1_000);
        assert!(tx.is_valid().unwrap());
    }

    #[test]
    fn test_shielded_tx_action_type() {
        let tx = make_mint_tx(500);
        assert_eq!(tx.action.action_type(), "mint");
    }

    #[test]
    fn test_engine_apply_mint() {
        let engine = ZkShieldEngine::new();
        let tx = make_mint_tx(1_000);
        assert!(engine.apply(&tx).is_ok());
    }

    #[test]
    fn test_engine_transfer_tx() {
        // NOVA-RED-ZK-002: Transfer öncesinde girdi commitment pool'da kayıtlı olmalı.
        // Önce mint ile commitment'ı pool'a ekle.
        let input = make_note(1_000, 1);
        let output = make_note(800, 2);
        let nk = NullifierKey([5u8; 32]);

        let engine = ZkShieldEngine::new();
        // Pool'a input note'un commitment'ını mint ile ekle
        engine.pool().mint(input.clone(), SlotId(5)).unwrap();

        let transfer_witness = TransferWitness::new(input, nk, output).unwrap();
        let transfer_proof = transfer_witness.prove(SlotId(5)).unwrap();
        let tx = ShieldedTx::new(
            ShieldedAction::Transfer(transfer_proof),
            ShieldMode::FullShield,
            SlotId(5),
            200,
        );
        assert!(engine.apply(&tx).is_ok());
    }

    #[test]
    fn test_engine_transfer_without_commitment_rejected() {
        // NOVA-RED-ZK-002 regression: Pool'da commitment yoksa transfer reddedilmeli
        let input = make_note(1_000, 3);
        let output = make_note(800, 4);
        let nk = NullifierKey([6u8; 32]);
        let witness = TransferWitness::new(input, nk, output).unwrap();
        let proof = witness.prove(SlotId(5)).unwrap();
        let tx = ShieldedTx::new(
            ShieldedAction::Transfer(proof),
            ShieldMode::FullShield,
            SlotId(5),
            200,
        );
        let engine = ZkShieldEngine::new(); // boş pool — commitment yok
        let result = engine.apply(&tx);
        assert!(
            matches!(result, Err(ZkError::CommitmentNotFound)),
            "Pool'da bulunmayan commitment ile transfer reddedilmeli"
        );
    }

    #[test]
    fn test_zero_fee_zk_rejected() {
        let tx = ShieldedTx::new(
            ShieldedAction::Mint(MintWitness::new(make_note(100, 1)).prove(SlotId(0)).unwrap()),
            ShieldMode::FullShield,
            SlotId(0),
            0, // fee = 0
        );
        assert!(tx.is_valid().is_err());
    }
}
