//! Account Keys — Sealevel paralel execution için hesap beyanı (Solana'dan ilham)
//!
//! Her işlem, okuyacağı ve yazacağı hesapları önceden beyan eder.
//! Bu sayede bağımsız işlemler paralel yürütülür.

use nova_primitives::Address;
use std::collections::HashSet;

/// Hesap erişim tipi
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AccountAccess {
    /// Sadece okuma (paylaşılabilir)
    Read(Address),
    /// Okuma + Yazma (özel kilit)
    Write(Address),
}

impl AccountAccess {
    pub fn address(&self) -> &Address {
        match self {
            AccountAccess::Read(a) | AccountAccess::Write(a) => a,
        }
    }

    pub fn is_write(&self) -> bool {
        matches!(self, AccountAccess::Write(_))
    }
}

/// AccountKeys — bir işlemin erişim beyanı
#[derive(Debug, Clone, Default)]
pub struct AccountKeys {
    reads: HashSet<Address>,
    writes: HashSet<Address>,
}

impl AccountKeys {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_read(&mut self, addr: Address) -> &mut Self {
        self.reads.insert(addr);
        self
    }

    pub fn add_write(&mut self, addr: Address) -> &mut Self {
        // Write hesapları da read edilebilir
        self.writes.insert(addr);
        self.reads.remove(&addr); // write varsa read gereksiz
        self
    }

    pub fn reads(&self) -> &HashSet<Address> {
        &self.reads
    }

    pub fn writes(&self) -> &HashSet<Address> {
        &self.writes
    }

    /// İki AccountKeys çakışıyor mu?
    /// - Write-Write çakışması → sıralı işleme gerekli
    /// - Write-Read çakışması → sıralı işleme gerekli
    /// - Read-Read → paralel çalışabilir
    pub fn conflicts_with(&self, other: &AccountKeys) -> bool {
        // self'in write'ları other'ın read/write'larıyla çakışıyor mu?
        for addr in &self.writes {
            if other.reads.contains(addr) || other.writes.contains(addr) {
                return true;
            }
        }
        // other'ın write'ları self'in read/write'larıyla çakışıyor mu?
        for addr in &other.writes {
            if self.reads.contains(addr) || self.writes.contains(addr) {
                return true;
            }
        }
        false
    }

    pub fn all_accessed(&self) -> impl Iterator<Item = &Address> {
        self.reads.iter().chain(self.writes.iter())
    }
}

/// TX bağımlılık grafiği — paralel gruplara ayırır (Sealevel ilkesi)
pub struct DependencyGraph {
    tx_keys: Vec<AccountKeys>,
}

impl DependencyGraph {
    pub fn new(tx_keys: Vec<AccountKeys>) -> Self {
        Self { tx_keys }
    }

    /// Bağımsız gruplar oluştur (paralel çalıştırılabilir)
    /// Her grup içindeki TX'ler birbirini etkilemez
    pub fn build_parallel_groups(&self) -> Vec<Vec<usize>> {
        let n = self.tx_keys.len();
        let mut assigned = vec![false; n];
        let mut groups: Vec<Vec<usize>> = Vec::new();

        for i in 0..n {
            if assigned[i] {
                continue;
            }
            let mut group = vec![i];
            assigned[i] = true;

            // i ile çakışmayan diğer tx'leri aynı gruba ekle
            for j in (i + 1)..n {
                if assigned[j] {
                    continue;
                }
                // Bu j, gruptaki TÜM üyelerle çakışıyor mu kontrol et
                let conflicts = group
                    .iter()
                    .any(|&k| self.tx_keys[k].conflicts_with(&self.tx_keys[j]));
                if !conflicts {
                    group.push(j);
                    assigned[j] = true;
                }
            }
            groups.push(group);
        }

        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_no_conflict_read_read() {
        let mut k1 = AccountKeys::new();
        k1.add_read(addr(1));
        let mut k2 = AccountKeys::new();
        k2.add_read(addr(1));
        assert!(!k1.conflicts_with(&k2));
    }

    #[test]
    fn test_conflict_write_read() {
        let mut k1 = AccountKeys::new();
        k1.add_write(addr(1));
        let mut k2 = AccountKeys::new();
        k2.add_read(addr(1));
        assert!(k1.conflicts_with(&k2));
    }

    #[test]
    fn test_conflict_write_write() {
        let mut k1 = AccountKeys::new();
        k1.add_write(addr(1));
        let mut k2 = AccountKeys::new();
        k2.add_write(addr(1));
        assert!(k1.conflicts_with(&k2));
    }

    #[test]
    fn test_no_conflict_different_accounts() {
        let mut k1 = AccountKeys::new();
        k1.add_write(addr(1));
        let mut k2 = AccountKeys::new();
        k2.add_write(addr(2));
        assert!(!k1.conflicts_with(&k2));
    }

    #[test]
    fn test_dependency_graph_independent_txs() {
        // 3 TX, farklı hesaplar → hepsi paralel
        let keys: Vec<AccountKeys> = (1u8..=3)
            .map(|i| {
                let mut k = AccountKeys::new();
                k.add_write(addr(i));
                k
            })
            .collect();

        let graph = DependencyGraph::new(keys);
        let groups = graph.build_parallel_groups();
        assert_eq!(groups.len(), 1); // Tek grup, paralel
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn test_dependency_graph_conflicting_txs() {
        // TX0 ve TX1 aynı hesabı yazıyor → ayrı gruplar
        let mut k0 = AccountKeys::new();
        k0.add_write(addr(1));
        let mut k1 = AccountKeys::new();
        k1.add_write(addr(1));
        let mut k2 = AccountKeys::new();
        k2.add_write(addr(2)); // Bağımsız

        let graph = DependencyGraph::new(vec![k0, k1, k2]);
        let groups = graph.build_parallel_groups();

        // TX0 ve TX1 farklı gruplarda, TX2 biriyle aynı grupta
        let total_txs: usize = groups.iter().map(|g| g.len()).sum();
        assert_eq!(total_txs, 3);
        assert!(groups.len() >= 2);
    }

    #[test]
    fn test_account_access_write_overwrites_read() {
        let mut k = AccountKeys::new();
        k.add_read(addr(1));
        k.add_write(addr(1)); // Aynı adres
        assert!(k.reads().is_empty()); // Read kaldırıldı
        assert!(k.writes().contains(&addr(1)));
    }

    #[test]
    fn test_all_accessed_iterator() {
        let mut k = AccountKeys::new();
        k.add_read(addr(1));
        k.add_write(addr(2));
        let accessed: Vec<_> = k.all_accessed().collect();
        assert_eq!(accessed.len(), 2);
    }
}
