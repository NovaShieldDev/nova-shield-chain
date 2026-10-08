//! AccountStateCache — DashMap tabanlı thread-safe state cache (Solana AccountsDB'den ilham)
//!
//! Production'da bu katmanın arkasında RocksDB bulunur.
//! Bu modül: in-memory concurrent state operasyonları
//!
//! Güvenlik eklemeleri:
//! - `with_transaction()` — Polkadot `with_transaction` pattern: atomik snapshot/commit/rollback
//! - `write_locked_accounts()` — Solana AccountLocks: paralel execution write-lock desteği

use crate::state::account::AccountState;
use crate::errors::{RuntimeError, RuntimeResult};
use dashmap::DashMap;
use nova_primitives::Address;
use std::sync::Arc;

/// AccountStateCache — paralel TX execution için thread-safe hesap deposu
///
/// DashMap varsayılan shard sayısı (16) yüksek paralel yükte lock contention
/// yaratır. 64 shard ile Rayon thread pool (CPU core sayısı) yük dengesini
/// optimize eder. (Solana AppendVec: shard_count = num_cpus * 4 ilkesi)
#[derive(Debug, Clone, Default)]
pub struct AccountStateCache {
    inner: Arc<DashMap<Address, AccountState>>,
}

/// Varsayılan DashMap shard sayısı (16 → 64 ile yüksek paralel yük optimizasyonu)
const DEFAULT_SHARD_AMOUNT: usize = 64;

impl AccountStateCache {
    pub fn new() -> Self {
        Self { inner: Arc::new(DashMap::with_shard_amount(DEFAULT_SHARD_AMOUNT)) }
    }

    /// Hesabı al (yoksa default döner)
    pub fn get(&self, addr: &Address) -> AccountState {
        self.inner.get(addr).map(|a| a.clone()).unwrap_or_default()
    }

    /// Hesabı yaz
    pub fn set(&self, addr: Address, state: AccountState) {
        self.inner.insert(addr, state);
    }

    /// Nonce oku
    pub fn nonce(&self, addr: &Address) -> u64 {
        self.inner.get(addr).map(|a| a.nonce).unwrap_or(0)
    }

    /// Nonce ayarla (test/genesis için)
    pub fn set_nonce(&self, addr: Address, nonce: u64) {
        self.inner.entry(addr).or_default().nonce = nonce;
    }

    /// Bakiye oku
    pub fn balance(&self, addr: &Address) -> u128 {
        self.inner.get(addr).map(|a| a.balance).unwrap_or(0)
    }

    /// Bakiye ayarla
    pub fn set_balance(&self, addr: Address, balance: u128) {
        self.inner.entry(addr).or_default().balance = balance;
    }

    /// Nonce artır ve yeni değeri döndür
    pub fn increment_nonce(&self, addr: &Address) -> u64 {
        let mut entry = self.inner.entry(*addr).or_default();
        entry.nonce += 1;
        entry.nonce
    }

    /// Bakiye aktar: from → to
    ///
    /// RUNTIME-001: Bakiye kontrolü ve düşümü tek atomik write kilidi içinde yapılır.
    /// Eski kod: `balance()` okuma → ayrı `entry()` yazma arası TOCTOU race.
    /// Yeni kod: `entry(*from)` write kilidi alınır, içinde kontrol + düşüm atomik.
    pub fn transfer(&self, from: &Address, to: &Address, amount: u128) -> bool {
        // Atomik: bakiye yeterli mi kontrolü ve düşümü aynı shard kilidi altında
        {
            let mut from_entry = self.inner.entry(*from).or_default();
            if from_entry.balance < amount {
                return false;
            }
            from_entry.balance -= amount;
        }
        {
            let mut to_entry = self.inner.entry(*to).or_default();
            to_entry.balance += amount;
        }
        true
    }

    /// Hesap sayısı
    pub fn account_count(&self) -> usize {
        self.inner.len()
    }

    /// Boş olmayan hesaplar
    pub fn non_empty_accounts(&self) -> Vec<Address> {
        self.inner
            .iter()
            .filter(|e| !e.value().is_empty())
            .map(|e| *e.key())
            .collect()
    }

    /// State root hash — Binary Merkle Tree (FAZ P gerçek impl).
    ///
    /// Sıralı adreslerin yaprak hash'lerinden ikili Merkle kökü üretir.
    /// Domain separator ("nova-merkle-leaf:", "nova-merkle-node:") ile
    /// ikinci önceden görüntü saldırılarına karşı koruma.
    ///
    /// Pattern: Ethereum Patricia Trie, Bitcoin Merkle tree.
    pub fn compute_state_root(&self) -> [u8; 32] {
        let accounts: Vec<(Address, crate::state::account::AccountState)> = self
            .inner
            .iter()
            .map(|e| (*e.key(), e.value().clone()))
            .collect();
        crate::state::merkle::compute_accounts_root(&accounts)
    }

    /// Snapshot al — tüm state'i klonla
    pub fn snapshot(&self) -> Vec<(Address, AccountState)> {
        self.inner.iter().map(|e| (*e.key(), e.value().clone())).collect()
    }

    /// Snapshot'tan geri yükle
    pub fn restore(&self, snapshot: Vec<(Address, AccountState)>) {
        self.inner.clear();
        for (addr, state) in snapshot {
            self.inner.insert(addr, state);
        }
    }

    /// Transaksiyonel operasyon — Polkadot `with_transaction` pattern.
    ///
    /// 1. Snapshot al
    /// 2. Closure'ı çalıştır
    /// 3. Ok(v)  → değişiklikleri commit et, v döndür
    /// 4. Err(e) → snapshot'a geri dön (rollback), hatayı yeniden fırlat
    ///
    /// Kullanım alanları:
    /// - UTXO spend: nullifier set + bakiye azalt atomik
    /// - Slashing: stake kes + jaillendir atomik
    /// - Vesting: kilit güncelle + token transfer atomik
    /// - Governance: depozit kilitle + öneri kaydet atomik
    pub fn with_transaction<F, T>(&self, f: F) -> RuntimeResult<T>
    where
        F: FnOnce() -> RuntimeResult<T>,
    {
        let snapshot = self.snapshot();
        match f() {
            Ok(v) => Ok(v),
            Err(e) => {
                self.restore(snapshot);
                Err(e)
            }
        }
    }

    /// Belirtilen adresler için anlık bakiye + nonce snapshot al (Solana partial snapshot).
    ///
    /// Büyük state'lerde tüm cache'i klonlamak yerine sadece ilgili hesapları snapshot'a al.
    pub fn snapshot_accounts(&self, addrs: &[Address]) -> Vec<(Address, AccountState)> {
        addrs.iter().map(|addr| (*addr, self.get(addr))).collect()
    }

    /// Belirli hesapları snapshot'tan geri yükle (diğer hesaplar etkilenmez)
    pub fn restore_accounts(&self, snapshot: Vec<(Address, AccountState)>) {
        for (addr, state) in snapshot {
            self.inner.insert(addr, state);
        }
    }

    /// Seçici transaksiyonel operasyon — yalnızca `affected_addrs` hesaplarını rollback eder.
    ///
    /// Tam state snapshot yerine sadece etkilenen hesapları rollback eder → daha verimli.
    pub fn with_account_transaction<F, T>(
        &self,
        affected_addrs: &[Address],
        f: F,
    ) -> RuntimeResult<T>
    where
        F: FnOnce() -> RuntimeResult<T>,
    {
        let snapshot = self.snapshot_accounts(affected_addrs);
        match f() {
            Ok(v) => Ok(v),
            Err(e) => {
                self.restore_accounts(snapshot);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_default_account_zero_balance() {
        let cache = AccountStateCache::new();
        assert_eq!(cache.balance(&addr(1)), 0);
        assert_eq!(cache.nonce(&addr(1)), 0);
    }

    #[test]
    fn test_set_and_get_balance() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 5_000);
        assert_eq!(cache.balance(&addr(1)), 5_000);
    }

    #[test]
    fn test_transfer_success() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        assert!(cache.transfer(&addr(1), &addr(2), 400));
        assert_eq!(cache.balance(&addr(1)), 600);
        assert_eq!(cache.balance(&addr(2)), 400);
    }

    #[test]
    fn test_transfer_insufficient() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 100);
        assert!(!cache.transfer(&addr(1), &addr(2), 200));
        assert_eq!(cache.balance(&addr(1)), 100);
    }

    #[test]
    fn test_increment_nonce() {
        let cache = AccountStateCache::new();
        assert_eq!(cache.increment_nonce(&addr(1)), 1);
        assert_eq!(cache.increment_nonce(&addr(1)), 2);
        assert_eq!(cache.nonce(&addr(1)), 2);
    }

    #[test]
    fn test_state_root_deterministic() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        cache.set_balance(addr(2), 2_000);
        let root1 = cache.compute_state_root();
        let root2 = cache.compute_state_root();
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_state_root_changes_on_update() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        let root1 = cache.compute_state_root();
        cache.set_balance(addr(1), 2_000);
        let root2 = cache.compute_state_root();
        assert_ne!(root1, root2);
    }

    #[test]
    fn test_snapshot_and_restore() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        cache.set_balance(addr(2), 2_000);
        let snap = cache.snapshot();

        // State değiştir
        cache.set_balance(addr(1), 9_999);

        // Geri yükle
        cache.restore(snap);
        assert_eq!(cache.balance(&addr(1)), 1_000);
        assert_eq!(cache.balance(&addr(2)), 2_000);
    }

    #[test]
    fn test_non_empty_accounts() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        cache.set_balance(addr(2), 0); // Boş
        let non_empty = cache.non_empty_accounts();
        assert_eq!(non_empty.len(), 1);
    }

    // ─── WITH_TRANSACTION TESTLERİ ────────────────────────────────────────────

    #[test]
    fn test_with_transaction_commits_on_ok() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);

        let result = cache.with_transaction(|| {
            cache.set_balance(addr(1), 9_999);
            cache.set_balance(addr(2), 500);
            Ok::<_, crate::errors::RuntimeError>(42u32)
        });

        assert_eq!(result.unwrap(), 42);
        assert_eq!(cache.balance(&addr(1)), 9_999, "commit edilmeli");
        assert_eq!(cache.balance(&addr(2)), 500, "commit edilmeli");
    }

    #[test]
    fn test_with_transaction_rollback_on_err() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        cache.set_balance(addr(2), 2_000);

        let result = cache.with_transaction(|| {
            cache.set_balance(addr(1), 0);
            cache.set_balance(addr(2), 0);
            Err::<u32, _>(crate::errors::RuntimeError::AccountNotFound)
        });

        assert!(result.is_err(), "hata dönmeli");
        assert_eq!(cache.balance(&addr(1)), 1_000, "rollback: addr(1) eski bakiyeye dönmeli");
        assert_eq!(cache.balance(&addr(2)), 2_000, "rollback: addr(2) eski bakiyeye dönmeli");
    }

    #[test]
    fn test_with_account_transaction_partial_rollback() {
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 1_000);
        cache.set_balance(addr(2), 2_000);
        cache.set_balance(addr(3), 3_000); // etkilenmiyor

        let affected = [addr(1), addr(2)];
        let result = cache.with_account_transaction(&affected, || {
            cache.set_balance(addr(1), 0);
            cache.set_balance(addr(2), 0);
            cache.set_balance(addr(3), 9_999); // bu değişiklik rollback edilmez
            Err::<u32, _>(crate::errors::RuntimeError::AccountNotFound)
        });

        assert!(result.is_err());
        assert_eq!(cache.balance(&addr(1)), 1_000, "addr(1) rollback edilmeli");
        assert_eq!(cache.balance(&addr(2)), 2_000, "addr(2) rollback edilmeli");
        // Not: addr(3) affected'da değil → rollback edilmez
    }

    #[test]
    fn test_with_transaction_nested_ok() {
        // İç başarılı, dış başarılı → her ikisi commit
        let cache = AccountStateCache::new();
        cache.set_balance(addr(1), 100);

        cache.with_transaction(|| {
            cache.set_balance(addr(1), 200);
            cache.with_transaction(|| {
                cache.set_balance(addr(1), 300);
                Ok::<_, crate::errors::RuntimeError>(())
            })?;
            Ok::<_, crate::errors::RuntimeError>(())
        }).unwrap();

        assert_eq!(cache.balance(&addr(1)), 300);
    }
}
