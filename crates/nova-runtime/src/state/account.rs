//! Hesap state yapısı — nonce, balance, code_hash

use nova_primitives::Address;
use serde::{Deserialize, Serialize};

/// AccountState — bir hesabın anlık durumu
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    /// İşlem tekrar koruma sayacı
    pub nonce: u64,
    /// NSC bakiye (en küçük birim)
    pub balance: u128,
    /// Sözleşme kodu hash'i (EOA = None)
    pub code_hash: Option<[u8; 32]>,
    /// Storage kökü (EOA = None)
    pub storage_root: Option<[u8; 32]>,
}

impl AccountState {
    pub fn new_eoa(balance: u128) -> Self {
        Self { nonce: 0, balance, code_hash: None, storage_root: None }
    }

    pub fn is_empty(&self) -> bool {
        self.nonce == 0 && self.balance == 0 && self.code_hash.is_none()
    }

    pub fn increment_nonce(&mut self) -> u64 {
        self.nonce += 1;
        self.nonce
    }
}

/// GenesisAccount — genesis bloğu için başlangıç hesap tanımı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    pub address: Address,
    pub balance: u128,
    pub label: String,
}

impl GenesisAccount {
    pub fn new(address: Address, balance: u128, label: &str) -> Self {
        Self { address, balance, label: label.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_state_default() {
        let acc = AccountState::default();
        assert!(acc.is_empty());
    }

    #[test]
    fn test_new_eoa() {
        let acc = AccountState::new_eoa(1_000);
        assert_eq!(acc.balance, 1_000);
        assert_eq!(acc.nonce, 0);
        assert!(acc.code_hash.is_none());
        assert!(!acc.is_empty()); // Balance var → boş değil
    }

    #[test]
    fn test_increment_nonce() {
        let mut acc = AccountState::new_eoa(0);
        let n1 = acc.increment_nonce();
        let n2 = acc.increment_nonce();
        assert_eq!(n1, 1);
        assert_eq!(n2, 2);
        assert_eq!(acc.nonce, 2);
    }

    #[test]
    fn test_genesis_account() {
        let addr = Address::from_bytes([1u8; 20]);
        let ga = GenesisAccount::new(addr, 1_000_000, "ecosystem");
        assert_eq!(ga.balance, 1_000_000);
        assert_eq!(ga.label, "ecosystem");
    }
}
