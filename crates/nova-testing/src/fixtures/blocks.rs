//! Blok fixture'ları

use serde::{Deserialize, Serialize};

/// Test bloğu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestBlock {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    pub validator: [u8; 20],
    pub slot: u64,
    pub tx_count: u32,
    pub gas_used: u64,
}

impl TestBlock {
    pub fn genesis() -> Self {
        Self {
            number: 0,
            hash: [0u8; 32],
            parent_hash: [0u8; 32],
            validator: [0u8; 20],
            slot: 0,
            tx_count: 0,
            gas_used: 0,
        }
    }

    pub fn at_height(number: u64) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-test-block");
        h.update(&number.to_le_bytes());
        let hash = *h.finalize().as_bytes();

        let mut ph = blake3::Hasher::new();
        ph.update(b"nova-test-block");
        ph.update(&(number.saturating_sub(1)).to_le_bytes());
        let parent_hash = *ph.finalize().as_bytes();

        Self {
            number,
            hash,
            parent_hash,
            validator: [0x01u8; 20],
            slot: number,
            tx_count: (number % 10) as u32,
            gas_used: (number % 10) as u64 * 21_000,
        }
    }

    pub fn hash_hex(&self) -> String {
        format!("0x{}", self.hash.iter().map(|b| format!("{:02x}", b)).collect::<String>())
    }
}

/// Test blockchain — bir dizi blok
pub struct TestBlockchain {
    pub blocks: Vec<TestBlock>,
}

impl TestBlockchain {
    /// Genesis + n blok
    pub fn with_blocks(n: u64) -> Self {
        let blocks = (0..=n).map(TestBlock::at_height).collect();
        Self { blocks }
    }

    pub fn best_block(&self) -> &TestBlock {
        self.blocks.last().unwrap()
    }

    pub fn get_block(&self, number: u64) -> Option<&TestBlock> {
        self.blocks.iter().find(|b| b.number == number)
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Blokların zincir bütünlüğünü doğrula
    pub fn verify_chain(&self) -> bool {
        if self.blocks.is_empty() {
            return true;
        }
        for i in 1..self.blocks.len() {
            if self.blocks[i].parent_hash != self.blocks[i - 1].hash {
                // Mock: basit sayısal doğrulama
                if self.blocks[i].number != self.blocks[i - 1].number + 1 {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_block() {
        let g = TestBlock::genesis();
        assert_eq!(g.number, 0);
        assert_eq!(g.hash, [0u8; 32]);
    }

    #[test]
    fn test_block_at_height() {
        let b = TestBlock::at_height(100);
        assert_eq!(b.number, 100);
        assert_ne!(b.hash, [0u8; 32]);
    }

    #[test]
    fn test_block_hash_deterministic() {
        let b1 = TestBlock::at_height(42);
        let b2 = TestBlock::at_height(42);
        assert_eq!(b1.hash, b2.hash);
    }

    #[test]
    fn test_block_hash_unique() {
        let b1 = TestBlock::at_height(1);
        let b2 = TestBlock::at_height(2);
        assert_ne!(b1.hash, b2.hash);
    }

    #[test]
    fn test_blockchain_with_blocks() {
        let chain = TestBlockchain::with_blocks(9);
        assert_eq!(chain.block_count(), 10); // 0..=9
        assert_eq!(chain.best_block().number, 9);
    }

    #[test]
    fn test_blockchain_get_block() {
        let chain = TestBlockchain::with_blocks(5);
        assert!(chain.get_block(3).is_some());
        assert!(chain.get_block(10).is_none());
    }

    #[test]
    fn test_blockchain_verify_chain() {
        let chain = TestBlockchain::with_blocks(10);
        assert!(chain.verify_chain());
    }

    #[test]
    fn test_hash_hex_format() {
        let b = TestBlock::at_height(1);
        let hex = b.hash_hex();
        assert!(hex.starts_with("0x"));
        assert_eq!(hex.len(), 66);
    }
}
