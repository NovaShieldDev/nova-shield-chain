//! Blok yaşam döngüsü — open → execute → freeze → finalize (go-ethereum state_processor'dan ilham)

use crate::errors::{RuntimeError, RuntimeResult};
use crate::executor::parallel::TxResult;
use crate::tx::types::Transaction;
use nova_primitives::{Address, EpochId, SlotId, ValidatorId};
use serde::{Deserialize, Serialize};

// ─── Blok Fazları ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockPhase {
    /// Açık — TX'ler ekleniyor
    Open,
    /// Donduruldu — TX eklenemiyor, state root hesaplanıyor
    Frozen,
    /// Finalize edildi — consensus'tan BLS imzası alındı
    Finalized,
}

// ─── Blok başlığı ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBlockHeader {
    pub height: u64,
    pub slot: SlotId,
    pub epoch: EpochId,
    pub parent_hash: [u8; 32],
    pub state_root: [u8; 32],
    pub tx_root: [u8; 32],
    pub proposer: ValidatorId,
    pub base_fee: u128,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub tx_count: u32,
}

impl RuntimeBlockHeader {
    /// BLAKE3 ile blok hash'i hesapla
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.slot.0.to_le_bytes());
        hasher.update(&self.parent_hash);
        hasher.update(&self.state_root);
        hasher.update(&self.tx_root);
        hasher.update(&self.proposer.0.to_le_bytes());
        hasher.update(&self.base_fee.to_le_bytes());
        hasher.update(&self.gas_used.to_le_bytes());
        *hasher.finalize().as_bytes()
    }
}

// ─── RuntimeBlock ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RuntimeBlock {
    pub header: RuntimeBlockHeader,
    pub transactions: Vec<Transaction>,
    pub tx_results: Vec<TxResult>,
    pub phase: BlockPhase,
    pub block_hash: [u8; 32],
}

impl RuntimeBlock {
    /// Yeni açık blok oluştur
    pub fn open(
        height: u64,
        slot: SlotId,
        epoch: EpochId,
        parent_hash: [u8; 32],
        proposer: ValidatorId,
        base_fee: u128,
    ) -> Self {
        let header = RuntimeBlockHeader {
            height,
            slot,
            epoch,
            parent_hash,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
            proposer,
            base_fee,
            gas_limit: crate::tx::fee::BLOCK_GAS_LIMIT,
            gas_used: 0,
            timestamp: slot.0 * 400, // 400ms/slot
            tx_count: 0,
        };
        RuntimeBlock {
            block_hash: header.compute_hash(),
            header,
            transactions: Vec::new(),
            tx_results: Vec::new(),
            phase: BlockPhase::Open,
        }
    }

    /// TX ekle (Open fazında)
    pub fn add_transaction(&mut self, tx: Transaction) -> RuntimeResult<()> {
        if self.phase != BlockPhase::Open {
            return Err(RuntimeError::BlockAlreadySealed);
        }
        self.header.gas_used += crate::tx::fee::TRANSFER_GAS_COST; // Tahmini
        self.transactions.push(tx);
        self.header.tx_count += 1;
        Ok(())
    }

    /// State root ve TX root ile bloğu dondur
    pub fn freeze(&mut self, state_root: [u8; 32], tx_results: Vec<TxResult>) -> RuntimeResult<()> {
        if self.phase != BlockPhase::Open {
            return Err(RuntimeError::BlockAlreadySealed);
        }
        self.header.state_root = state_root;
        self.header.tx_root = compute_tx_root(&self.transactions);
        self.tx_results = tx_results;
        self.phase = BlockPhase::Frozen;
        self.block_hash = self.header.compute_hash();
        Ok(())
    }

    /// Konsensüs imzası alındıktan sonra finalize et
    pub fn finalize(&mut self) -> RuntimeResult<()> {
        if self.phase != BlockPhase::Frozen {
            return Err(RuntimeError::BlockNotOpen);
        }
        self.phase = BlockPhase::Finalized;
        Ok(())
    }

    pub fn is_finalized(&self) -> bool {
        self.phase == BlockPhase::Finalized
    }

    pub fn success_count(&self) -> usize {
        self.tx_results.iter().filter(|r| r.success).count()
    }

    pub fn total_fees_collected(&self) -> u128 {
        self.tx_results.iter().map(|r| r.fee_paid).sum()
    }
}

/// TX root hesabı — tüm TX hash'lerinin BLAKE3 ağacı
fn compute_tx_root(txs: &[Transaction]) -> [u8; 32] {
    use crate::tx::types::compute_tx_hash;
    let mut hasher = blake3::Hasher::new();
    for tx in txs {
        hasher.update(&compute_tx_hash(tx));
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{EpochId, SlotId, ValidatorId};

    fn make_block(height: u64) -> RuntimeBlock {
        RuntimeBlock::open(
            height,
            SlotId(height * 10),
            EpochId(0),
            [0u8; 32],
            ValidatorId(1),
            1_000_000_000,
        )
    }

    #[test]
    fn test_block_open() {
        let block = make_block(1);
        assert_eq!(block.phase, BlockPhase::Open);
        assert_eq!(block.header.height, 1);
    }

    #[test]
    fn test_block_add_tx() {
        let mut block = make_block(1);
        let tx = Transaction::mock_transfer(
            nova_primitives::Address::from_bytes([1u8; 20]),
            nova_primitives::Address::from_bytes([2u8; 20]),
            100,
            0,
        );
        block.add_transaction(tx).unwrap();
        assert_eq!(block.transactions.len(), 1);
        assert_eq!(block.header.tx_count, 1);
    }

    #[test]
    fn test_block_freeze() {
        let mut block = make_block(1);
        block.freeze([1u8; 32], vec![]).unwrap();
        assert_eq!(block.phase, BlockPhase::Frozen);
        assert_eq!(block.header.state_root, [1u8; 32]);
    }

    #[test]
    fn test_block_finalize() {
        let mut block = make_block(1);
        block.freeze([1u8; 32], vec![]).unwrap();
        block.finalize().unwrap();
        assert!(block.is_finalized());
    }

    #[test]
    fn test_block_add_tx_to_sealed_fails() {
        let mut block = make_block(1);
        block.freeze([1u8; 32], vec![]).unwrap();
        let tx = Transaction::mock_transfer(
            nova_primitives::Address::from_bytes([1u8; 20]),
            nova_primitives::Address::from_bytes([2u8; 20]),
            100,
            0,
        );
        let err = block.add_transaction(tx);
        assert!(matches!(err, Err(RuntimeError::BlockAlreadySealed)));
    }

    #[test]
    fn test_block_hash_changes_after_freeze() {
        let mut block = make_block(1);
        let hash_before = block.block_hash;
        block.freeze([99u8; 32], vec![]).unwrap();
        let hash_after = block.block_hash;
        assert_ne!(hash_before, hash_after);
    }

    #[test]
    fn test_block_hash_deterministic() {
        let b1 = make_block(5);
        let b2 = make_block(5);
        assert_eq!(b1.block_hash, b2.block_hash);
    }
}
