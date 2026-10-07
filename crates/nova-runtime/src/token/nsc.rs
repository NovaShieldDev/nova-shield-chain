//! NSC Token — Nova Shield Coin sabit arz + temel operasyonlar (§7.1)
//!
//! Whitepaper §7.2:
//!   Toplam Arz: 1,000,000,000 NSC
//!   Dağılım: Ekosistem %30 | Staking %25 | Hazine %20 | Yatırımcı %15 | Ekip %10

use nova_primitives::{Address, Balance};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Toplam NSC arzı: 1 milyar × 10^18 (en küçük birim)
pub const NSC_TOTAL_SUPPLY: u128 = 1_000_000_000 * 1_000_000_000_000_000_000u128;

/// 1 NSC = 10^18 en küçük birim
pub const NSC_DECIMALS: u128 = 1_000_000_000_000_000_000u128;

/// Minimum validator stake: 100,000 NSC
pub const MIN_VALIDATOR_STAKE_NSC: u128 = 100_000 * NSC_DECIMALS;

/// Minimum delegasyon: 100 NSC
pub const MIN_DELEGATION_NSC: u128 = 100 * NSC_DECIMALS;

/// Minimum governance deposit: 1,000 NSC
pub const MIN_GOV_DEPOSIT_NSC: u128 = 1_000 * NSC_DECIMALS;

// ─── Token Dağılım Payları (BPS) ──────────────────────────────────────────────

pub const ALLOC_ECOSYSTEM_BPS: u32 = 3_000;    // %30 — Ekosistem & Geliştirici
pub const ALLOC_STAKING_BPS: u32 = 2_500;      // %25 — Validator & Staking Ödülleri
pub const ALLOC_TREASURY_BPS: u32 = 2_000;     // %20 — Hazine
pub const ALLOC_INVESTORS_BPS: u32 = 1_500;    // %15 — Erken Destekçiler
pub const ALLOC_TEAM_BPS: u32 = 1_000;         // %10 — Ekip

// ─── NscLedger — Thread-safe bakiye deposu ────────────────────────────────────

/// NSC bakiye ledger'ı — her Address için Balance saklar.
/// Production'da bu RocksDB üzerinde çalışır; testler için in-memory.
#[derive(Debug)]
pub struct NscLedger {
    balances: Arc<RwLock<HashMap<Address, u128>>>,
    total_supply: Arc<RwLock<u128>>,
    total_burned: Arc<RwLock<u128>>,
}

impl Default for NscLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl NscLedger {
    pub fn new() -> Self {
        Self {
            balances: Arc::new(RwLock::new(HashMap::new())),
            total_supply: Arc::new(RwLock::new(0)),
            total_burned: Arc::new(RwLock::new(0)),
        }
    }

    /// Genesis dağıtımı: adrese NSC mint et.
    /// Toplam mint edilen miktar NSC_TOTAL_SUPPLY'ı geçemez.
    pub fn mint(&self, to: Address, amount: u128) -> crate::errors::RuntimeResult<()> {
        let mut supply = self.total_supply.write();
        if *supply + amount > NSC_TOTAL_SUPPLY {
            return Err(crate::errors::RuntimeError::MaxSupplyReached);
        }
        let mut balances = self.balances.write();
        *balances.entry(to).or_insert(0) += amount;
        *supply += amount;
        Ok(())
    }

    /// Adresten NSC yak (deflationary).
    pub fn burn(&self, from: Address, amount: u128) -> crate::errors::RuntimeResult<()> {
        let mut balances = self.balances.write();
        let bal = balances.get(&from).copied().unwrap_or(0);
        if bal < amount {
            return Err(crate::errors::RuntimeError::InsufficientBurnBalance);
        }
        *balances.entry(from).or_insert(0) -= amount;
        let mut supply = self.total_supply.write();
        *supply = supply.saturating_sub(amount);
        *self.total_burned.write() += amount;
        Ok(())
    }

    /// Transfer: from → to
    pub fn transfer(
        &self,
        from: Address,
        to: Address,
        amount: u128,
    ) -> crate::errors::RuntimeResult<()> {
        let mut balances = self.balances.write();
        let from_bal = balances.get(&from).copied().unwrap_or(0);
        if from_bal < amount {
            return Err(crate::errors::RuntimeError::InsufficientBalance {
                required: amount,
                available: from_bal,
            });
        }
        *balances.entry(from).or_insert(0) -= amount;
        *balances.entry(to).or_insert(0) += amount;
        Ok(())
    }

    pub fn balance_of(&self, addr: &Address) -> u128 {
        self.balances.read().get(addr).copied().unwrap_or(0)
    }

    pub fn total_supply(&self) -> u128 {
        *self.total_supply.read()
    }

    pub fn total_burned(&self) -> u128 {
        *self.total_burned.read()
    }

    /// Toplam dağıtılmamış (henüz mint edilmemiş) arz
    pub fn remaining_mintable(&self) -> u128 {
        NSC_TOTAL_SUPPLY.saturating_sub(*self.total_supply.read())
    }
}

// ─── Genesis dağıtımı yardımcıları ────────────────────────────────────────────

/// Genesis tahsisatını hesaplar (sadece referans, gerçek mint ayrı)
pub fn genesis_allocation() -> [(u32, u128); 5] {
    [
        (ALLOC_ECOSYSTEM_BPS, NSC_TOTAL_SUPPLY * 30 / 100),
        (ALLOC_STAKING_BPS, NSC_TOTAL_SUPPLY * 25 / 100),
        (ALLOC_TREASURY_BPS, NSC_TOTAL_SUPPLY * 20 / 100),
        (ALLOC_INVESTORS_BPS, NSC_TOTAL_SUPPLY * 15 / 100),
        (ALLOC_TEAM_BPS, NSC_TOTAL_SUPPLY * 10 / 100),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::Address;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_mint_and_balance() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 1_000 * NSC_DECIMALS).unwrap();
        assert_eq!(ledger.balance_of(&addr(1)), 1_000 * NSC_DECIMALS);
        assert_eq!(ledger.total_supply(), 1_000 * NSC_DECIMALS);
    }

    #[test]
    fn test_mint_exceeds_supply() {
        let ledger = NscLedger::new();
        let err = ledger.mint(addr(1), NSC_TOTAL_SUPPLY + 1);
        assert!(matches!(err, Err(crate::errors::RuntimeError::MaxSupplyReached)));
    }

    #[test]
    fn test_transfer() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 500 * NSC_DECIMALS).unwrap();
        ledger.transfer(addr(1), addr(2), 200 * NSC_DECIMALS).unwrap();
        assert_eq!(ledger.balance_of(&addr(1)), 300 * NSC_DECIMALS);
        assert_eq!(ledger.balance_of(&addr(2)), 200 * NSC_DECIMALS);
    }

    #[test]
    fn test_transfer_insufficient() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 100 * NSC_DECIMALS).unwrap();
        let err = ledger.transfer(addr(1), addr(2), 200 * NSC_DECIMALS);
        assert!(matches!(err, Err(crate::errors::RuntimeError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_burn() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 500 * NSC_DECIMALS).unwrap();
        ledger.burn(addr(1), 100 * NSC_DECIMALS).unwrap();
        assert_eq!(ledger.balance_of(&addr(1)), 400 * NSC_DECIMALS);
        assert_eq!(ledger.total_burned(), 100 * NSC_DECIMALS);
        assert_eq!(ledger.total_supply(), 400 * NSC_DECIMALS);
    }

    #[test]
    fn test_burn_insufficient() {
        let ledger = NscLedger::new();
        ledger.mint(addr(1), 50 * NSC_DECIMALS).unwrap();
        let err = ledger.burn(addr(1), 100 * NSC_DECIMALS);
        assert!(matches!(err, Err(crate::errors::RuntimeError::InsufficientBurnBalance)));
    }

    #[test]
    fn test_genesis_allocation_sums_to_total() {
        let allocs = genesis_allocation();
        let total: u128 = allocs.iter().map(|(_, amt)| amt).sum();
        assert_eq!(total, NSC_TOTAL_SUPPLY);
    }

    #[test]
    fn test_remaining_mintable() {
        let ledger = NscLedger::new();
        assert_eq!(ledger.remaining_mintable(), NSC_TOTAL_SUPPLY);
        ledger.mint(addr(1), 1_000 * NSC_DECIMALS).unwrap();
        assert_eq!(ledger.remaining_mintable(), NSC_TOTAL_SUPPLY - 1_000 * NSC_DECIMALS);
    }
}
