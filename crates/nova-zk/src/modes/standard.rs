//! Standart mod — mempool şifreli, zincir açık (varsayılan)
//!
//! ZK kanıt gerektirmez; sadece şifreli mempool üzerinden işlem yapar.
//! Zincire yazılan TX içeriği açık: gönderen, alıcı, miktar görünür.

use nova_primitives::Address;
use serde::{Deserialize, Serialize};

/// Standart mod işlem verisi (ZK gerektirmez)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StandardTxData {
    pub sender: Address,
    pub recipient: Address,
    pub amount: u128,
    pub nonce: u64,
}

/// Standart mod handler
pub struct StandardMode;

impl StandardMode {
    pub fn new() -> Self {
        Self
    }

    /// TX geçerliliğini doğrula (basit bakiye kontrolü)
    pub fn validate(&self, tx: &StandardTxData, sender_balance: u128) -> bool {
        tx.amount > 0 && sender_balance >= tx.amount
    }

    /// Şeffaf TX için gas maliyeti (standart)
    pub fn gas_cost(&self) -> u64 {
        21_000 // EIP-1559 minimum
    }
}

impl Default for StandardMode {
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

    fn make_tx(amount: u128) -> StandardTxData {
        StandardTxData {
            sender: addr(1),
            recipient: addr(2),
            amount,
            nonce: 0,
        }
    }

    #[test]
    fn test_standard_mode_valid_tx() {
        let mode = StandardMode::new();
        assert!(mode.validate(&make_tx(100), 1_000));
    }

    #[test]
    fn test_standard_mode_insufficient_balance() {
        let mode = StandardMode::new();
        assert!(!mode.validate(&make_tx(1_000), 500));
    }

    #[test]
    fn test_standard_mode_zero_amount_rejected() {
        let mode = StandardMode::new();
        assert!(!mode.validate(&make_tx(0), 1_000));
    }

    #[test]
    fn test_standard_gas_cost() {
        assert_eq!(StandardMode::new().gas_cost(), 21_000);
    }
}
