//! İşlem fixture'ları

use serde::{Deserialize, Serialize};

/// Test işlemi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTx {
    pub hash: [u8; 32],
    pub from: [u8; 20],
    pub to: [u8; 20],
    pub value: u128,
    pub gas_price: u64,
    pub gas_limit: u64,
    pub nonce: u64,
    pub data: Vec<u8>,
}

impl TestTx {
    pub fn hash_hex(&self) -> String {
        format!("0x{}", self.hash.iter().map(|b| format!("{:02x}", b)).collect::<String>())
    }

    pub fn is_contract_call(&self) -> bool {
        !self.data.is_empty()
    }
}

/// Basit transfer işlemi oluştur
pub fn make_transfer_tx(from: [u8; 20], to: [u8; 20], value: u128, nonce: u64) -> TestTx {
    let mut h = blake3::Hasher::new();
    h.update(b"nova-test-tx");
    h.update(&from);
    h.update(&to);
    h.update(&value.to_le_bytes());
    h.update(&nonce.to_le_bytes());
    let hash = *h.finalize().as_bytes();

    TestTx {
        hash,
        from,
        to,
        value,
        gas_price: 1_000_000_000, // 1 Gwei
        gas_limit: 21_000,
        nonce,
        data: vec![],
    }
}

/// Sözleşme çağrısı işlemi oluştur
pub fn make_contract_call_tx(from: [u8; 20], contract: [u8; 20], calldata: Vec<u8>, nonce: u64) -> TestTx {
    let mut h = blake3::Hasher::new();
    h.update(b"nova-test-contract-call");
    h.update(&from);
    h.update(&contract);
    h.update(&calldata);
    h.update(&nonce.to_le_bytes());
    let hash = *h.finalize().as_bytes();

    TestTx {
        hash,
        from,
        to: contract,
        value: 0,
        gas_price: 1_000_000_000,
        gas_limit: 100_000,
        nonce,
        data: calldata,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice() -> [u8; 20] { [0xAAu8; 20] }
    fn bob() -> [u8; 20] { [0xBBu8; 20] }

    #[test]
    fn test_transfer_tx() {
        let tx = make_transfer_tx(alice(), bob(), 1_000_000, 0);
        assert_eq!(tx.from, alice());
        assert_eq!(tx.to, bob());
        assert_eq!(tx.value, 1_000_000);
        assert!(!tx.is_contract_call());
    }

    #[test]
    fn test_contract_call_tx() {
        let calldata = vec![0x12, 0x34, 0x56, 0x78];
        let tx = make_contract_call_tx(alice(), [0x10u8; 20], calldata, 1);
        assert!(tx.is_contract_call());
        assert_eq!(tx.gas_limit, 100_000);
    }

    #[test]
    fn test_tx_hash_deterministic() {
        let t1 = make_transfer_tx(alice(), bob(), 100, 0);
        let t2 = make_transfer_tx(alice(), bob(), 100, 0);
        assert_eq!(t1.hash, t2.hash);
    }

    #[test]
    fn test_tx_hash_unique_by_nonce() {
        let t1 = make_transfer_tx(alice(), bob(), 100, 0);
        let t2 = make_transfer_tx(alice(), bob(), 100, 1);
        assert_ne!(t1.hash, t2.hash);
    }

    #[test]
    fn test_hash_hex_format() {
        let tx = make_transfer_tx(alice(), bob(), 100, 0);
        let hex = tx.hash_hex();
        assert!(hex.starts_with("0x"));
        assert_eq!(hex.len(), 66);
    }
}
