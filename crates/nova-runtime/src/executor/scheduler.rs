//! TX bağımlılık zamanlayıcı (Sealevel account-key dependency pattern)

use crate::tx::account_keys::{AccountKeys, DependencyGraph};
use crate::tx::types::Transaction;
use crate::virtual_account::{
    STAKING_MODULE_ADDR, GOVERNANCE_MODULE_ADDR, UPGRADE_MODULE_ADDR,
};
use nova_primitives::Address;

/// TX'e ait account keys'i türet
pub fn derive_account_keys(tx: &Transaction) -> AccountKeys {
    use crate::tx::types::TransactionKind;

    let mut keys = AccountKeys::new();
    // Gönderenin hesabı her zaman yazılır (nonce + balance)
    keys.add_write(tx.sender);

    match &tx.kind {
        TransactionKind::Transfer { to, .. } => {
            keys.add_write(*to); // Alıcı bakiyesi yazılır
        }
        TransactionKind::Stake { .. } | TransactionKind::Unstake { .. } => {
            keys.add_write(Address::from_bytes(STAKING_MODULE_ADDR));
        }
        TransactionKind::GovernanceVote { .. } | TransactionKind::GovernancePropose { .. } => {
            keys.add_write(Address::from_bytes(GOVERNANCE_MODULE_ADDR));
        }
        TransactionKind::RegisterValidator { .. } => {
            keys.add_write(Address::from_bytes(STAKING_MODULE_ADDR));
        }
        TransactionKind::WithdrawUnbonded { .. } => {
            keys.add_write(Address::from_bytes(STAKING_MODULE_ADDR));
            keys.add_write(tx.sender);
        }
        TransactionKind::RuntimeUpgrade { .. } => {
            keys.add_write(Address::from_bytes(UPGRADE_MODULE_ADDR));
        }
    }

    keys
}

/// Paralel grupları hesapla
pub fn build_parallel_groups(txs: &[Transaction]) -> Vec<Vec<usize>> {
    let keys: Vec<AccountKeys> = txs.iter().map(derive_account_keys).collect();
    let graph = DependencyGraph::new(keys);
    graph.build_parallel_groups()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::{Address, ValidatorId};

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_transfer_keys_contain_sender_and_receiver() {
        let tx = Transaction::mock_transfer(addr(1), addr(2), 100, 0);
        let keys = derive_account_keys(&tx);
        assert!(keys.writes().contains(&addr(1)));
        assert!(keys.writes().contains(&addr(2)));
    }

    #[test]
    fn test_different_transfer_no_conflict() {
        let tx1 = Transaction::mock_transfer(addr(1), addr(2), 100, 0);
        let tx2 = Transaction::mock_transfer(addr(3), addr(4), 100, 0);
        let k1 = derive_account_keys(&tx1);
        let k2 = derive_account_keys(&tx2);
        assert!(!k1.conflicts_with(&k2));
    }

    #[test]
    fn test_same_recipient_conflict() {
        let tx1 = Transaction::mock_transfer(addr(1), addr(5), 100, 0);
        let tx2 = Transaction::mock_transfer(addr(2), addr(5), 100, 0); // Aynı alıcı
        let k1 = derive_account_keys(&tx1);
        let k2 = derive_account_keys(&tx2);
        assert!(k1.conflicts_with(&k2));
    }

    #[test]
    fn test_build_parallel_groups_independent() {
        let txs: Vec<_> = (1u8..=4)
            .map(|i| Transaction::mock_transfer(addr(i), addr(i + 10), 100, 0))
            .collect();
        let groups = build_parallel_groups(&txs);
        assert_eq!(groups.len(), 1); // Hepsi bağımsız → tek grup
        assert_eq!(groups[0].len(), 4);
    }

    #[test]
    fn test_build_parallel_groups_with_conflict() {
        let tx1 = Transaction::mock_transfer(addr(1), addr(5), 100, 0);
        let tx2 = Transaction::mock_transfer(addr(2), addr(5), 100, 0); // Çakışıyor
        let tx3 = Transaction::mock_transfer(addr(3), addr(6), 100, 0); // Bağımsız

        let txs = vec![tx1, tx2, tx3];
        let groups = build_parallel_groups(&txs);
        let total: usize = groups.iter().map(|g| g.len()).sum();
        assert_eq!(total, 3);
        // TX1 ve TX2 ayrı gruplarda
        assert!(groups.len() >= 2);
    }
}
