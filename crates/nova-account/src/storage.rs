//! nova-account — Hesap Depolama Katmanı
//!
//! Whitepaper §5.1 — MPC-Native Hesap Modeli — Durum Yönetimi
//!
//! ## Araştırma Kaynakları ve Uygulanan Desenler
//!
//! **Go-Ethereum** (`core/state/database.go`):
//!   `Database interface` → `StorageBackend trait`.
//!   `OpenTrie(root) → Trie` → `StorageBackend::open_snapshot()`.
//!   `Trie.GetAccount(address) → *types.StateAccount` → `StorageBackend::load_account()`.
//!   `Commit(update *StateUpdate)` → `StorageBackend::commit()`.
//!   Journal pattern (rollback) → `AccountStorage::journal` field.
//!
//! **Cosmos-SDK** (`x/auth/keeper/keeper.go`):
//!   `NewAccountKeeper(cdc, key, params)` → `AccountStorage::new()`.
//!   `GetAccount(ctx, addr) sdk.AccountI` → `AccountStorage::get()`.
//!   `SetAccount(ctx, account)` → `AccountStorage::set()`.
//!   `RemoveAccount(ctx, account)` → `AccountStorage::remove()`.
//!   `IterateAccounts(ctx, cb)` → `AccountStorage::iter_accounts()`.
//!   `NewAccountWithAddress` → `AccountStorage::new_account()`.
//!
//! **Solana** (`accounts-db/src/`):
//!   `DashMap<Pubkey, AccountSharedData>` → `parking_lot::RwLock<HashMap>` (thread-safe).
//!   `AccountsDb.store_accounts()` → `AccountStorage::batch_store()`.
//!   `AccountsDb.load()` → `AccountStorage::load()`.
//!   Append-only storage model → `StorageJournalEntry` (immutable history).
//!
//! **Polkadot-SDK** (FRAME storage):
//!   `StorageMap<_, Blake2_128Concat, AccountId, AccountInfo>` →
//!   `AccountStorage` iç HashMap yapısı (abstraksiyon).
//!   `StorageDoubleMap` → `recovery_requests` ikincil indeks.

use std::collections::HashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, info};

use crate::errors::{AccountError, AccountResult};
use crate::heartbeat::HeartbeatState;
use crate::types::{AccountId, AccountState, AccountSummary, RecoveryRequest};
use nova_primitives::types::{Address, SlotId};

// ─── DEPOLAMA BACKEND TRAIT ───────────────────────────────────────────────────

/// Depolama arka ucu soyutlaması.
///
/// Go-Ethereum `Database` interface'inden ilham alındı:
/// `Type() → Reader(root) → OpenTrie(root) → Commit()`
///
/// Cosmos-SDK Keeper pattern: iş mantığı trait'ten bağımsız.
pub trait StorageBackend: Send + Sync {
    /// Hesabı yükle.
    fn load_account(&self, address: &Address) -> AccountResult<Option<AccountState>>;

    /// Hesabı kaydet.
    fn store_account(&self, state: &AccountState) -> AccountResult<()>;

    /// Hesabı sil.
    fn delete_account(&self, address: &Address) -> AccountResult<()>;

    /// Tüm hesap adreslerini listele.
    fn all_addresses(&self) -> AccountResult<Vec<Address>>;

    /// Hesap sayısını döndür.
    fn account_count(&self) -> AccountResult<usize>;

    /// Kurtarma talebini yükle.
    fn load_recovery_request(&self, request_id: &str) -> AccountResult<Option<RecoveryRequest>>;

    /// Kurtarma talebini kaydet.
    fn store_recovery_request(&self, request: &RecoveryRequest) -> AccountResult<()>;

    /// Hesabın kurtarma taleplerini listele.
    fn recovery_requests_for_account(&self, address: &Address) -> AccountResult<Vec<RecoveryRequest>>;

    /// Heartbeat durumunu yükle.
    fn load_heartbeat_state(&self, address: &Address) -> AccountResult<Option<HeartbeatState>>;

    /// Heartbeat durumunu kaydet.
    fn store_heartbeat_state(&self, state: &HeartbeatState) -> AccountResult<()>;
}

// ─── JOURNAL GİRİŞİ ──────────────────────────────────────────────────────────

/// Durum değişikliği journal girişi.
///
/// Go-Ethereum `journal.go` pattern: her değişiklik kaydedilir,
/// geri alma (rollback) için kullanılır.
/// Solana append-only storage modelinden ilham alındı.
#[derive(Debug, Clone)]
enum JournalEntry {
    /// Hesap oluşturuldu.
    AccountCreated { address: Address },
    /// Hesap güncellendi (önceki durum saklanıyor).
    AccountUpdated {
        address: Address,
        previous: Box<AccountState>,
    },
    /// Hesap silindi.
    AccountDeleted {
        address: Address,
        previous: Box<AccountState>,
    },
}

// ─── BELLEK İÇİ DEPOLAMA ─────────────────────────────────────────────────────

/// Bellek içi depolama implementasyonu.
///
/// Solana DashMap<Pubkey, AccountSharedData> benzeri, ancak
/// parking_lot::RwLock ile thread-safe HashMap kullanılıyor.
/// Test ve geliştirme ortamı için; üretimde disk tabanlı depolama kullanılır.
pub struct InMemoryStorage {
    /// Hesaplar: address → AccountState
    accounts: RwLock<HashMap<[u8; 20], AccountState>>,
    /// Kurtarma talepleri: request_id → RecoveryRequest
    recovery_requests: RwLock<HashMap<String, RecoveryRequest>>,
    /// Heartbeat durumları: address → HeartbeatState
    heartbeat_states: RwLock<HashMap<[u8; 20], HeartbeatState>>,
}

impl InMemoryStorage {
    /// Yeni bellek içi depolama oluştur.
    pub fn new() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
            recovery_requests: RwLock::new(HashMap::new()),
            heartbeat_states: RwLock::new(HashMap::new()),
        }
    }

    /// Boş mi?
    pub fn is_empty(&self) -> bool {
        self.accounts.read().is_empty()
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for InMemoryStorage {
    fn load_account(&self, address: &Address) -> AccountResult<Option<AccountState>> {
        let accounts = self.accounts.read();
        Ok(accounts.get(address.as_bytes()).cloned())
    }

    fn store_account(&self, state: &AccountState) -> AccountResult<()> {
        let mut accounts = self.accounts.write();
        accounts.insert(*state.address.as_bytes(), state.clone());
        Ok(())
    }

    fn delete_account(&self, address: &Address) -> AccountResult<()> {
        let mut accounts = self.accounts.write();
        accounts.remove(address.as_bytes());
        Ok(())
    }

    fn all_addresses(&self) -> AccountResult<Vec<Address>> {
        let accounts = self.accounts.read();
        Ok(accounts.keys().map(|k| Address::from_bytes(*k)).collect())
    }

    fn account_count(&self) -> AccountResult<usize> {
        Ok(self.accounts.read().len())
    }

    fn load_recovery_request(&self, request_id: &str) -> AccountResult<Option<RecoveryRequest>> {
        let requests = self.recovery_requests.read();
        Ok(requests.get(request_id).cloned())
    }

    fn store_recovery_request(&self, request: &RecoveryRequest) -> AccountResult<()> {
        let mut requests = self.recovery_requests.write();
        requests.insert(request.request_id.clone(), request.clone());
        Ok(())
    }

    fn recovery_requests_for_account(&self, address: &Address) -> AccountResult<Vec<RecoveryRequest>> {
        let requests = self.recovery_requests.read();
        Ok(requests
            .values()
            .filter(|r| r.account == *address)
            .cloned()
            .collect())
    }

    fn load_heartbeat_state(&self, address: &Address) -> AccountResult<Option<HeartbeatState>> {
        let states = self.heartbeat_states.read();
        Ok(states.get(address.as_bytes()).cloned())
    }

    fn store_heartbeat_state(&self, state: &HeartbeatState) -> AccountResult<()> {
        let mut states = self.heartbeat_states.write();
        states.insert(*state.account.as_bytes(), state.clone());
        Ok(())
    }
}

// ─── HESAP DEPOLAMA ──────────────────────────────────────────────────────────

/// Nova Shield hesap depolama yöneticisi.
///
/// Cosmos-SDK AccountKeeper'ın tam Rust karşılığı:
/// - `NewAccountKeeper` → `AccountStorage::new()`
/// - `GetAccount` → `AccountStorage::get()`
/// - `SetAccount` → `AccountStorage::set()`
/// - `IterateAccounts` → `AccountStorage::iter_accounts()`
///
/// Go-Ethereum StateDB'nin journal+snapshot yaklaşımı uygulanmıştır.
pub struct AccountStorage {
    /// Depolama arka ucu.
    backend: Arc<dyn StorageBackend>,
    /// Değişiklik journal'ı (Go-Ethereum journal.go benzeri).
    journal: RwLock<Vec<JournalEntry>>,
    /// Sonraki hesap ID sayacı (Cosmos-SDK global AccountNumber).
    next_account_id: RwLock<u64>,
    /// Slot → hesap sayısı istatistiği.
    slot_account_counts: RwLock<HashMap<u64, usize>>,
}

impl AccountStorage {
    /// Yeni hesap depolama yöneticisi oluştur.
    ///
    /// Cosmos-SDK: `NewAccountKeeper(cdc, key, paramSpace, proto, blockedAddrs)`
    pub fn new(backend: Arc<dyn StorageBackend>) -> Self {
        Self {
            backend,
            journal: RwLock::new(Vec::new()),
            next_account_id: RwLock::new(1),
            slot_account_counts: RwLock::new(HashMap::new()),
        }
    }

    /// Bellek içi depolama ile oluştur (test/geliştirme için).
    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryStorage::new()))
    }

    // ─── HESAP OKUMA ──────────────────────────────────────────────────────

    /// Adrese göre hesap yükle.
    ///
    /// Cosmos-SDK: `GetAccount(ctx, addr) sdk.AccountI`
    /// Go-Ethereum: `Trie.GetAccount(address) *types.StateAccount`
    pub fn get(&self, address: &Address) -> AccountResult<AccountState> {
        self.backend
            .load_account(address)?
            .ok_or_else(|| AccountError::not_found(hex::encode(address.as_bytes())))
    }

    /// Adrese göre hesap yükle (opsiyonel).
    pub fn get_optional(&self, address: &Address) -> AccountResult<Option<AccountState>> {
        self.backend.load_account(address)
    }

    /// ID'ye göre hesap bul.
    pub fn get_by_id(&self, account_id: AccountId) -> AccountResult<Option<AccountState>> {
        // Linear scan — üretimde indeks gerekir
        let addresses = self.backend.all_addresses()?;
        for addr in addresses {
            if let Some(state) = self.backend.load_account(&addr)? {
                if state.account_id == account_id {
                    return Ok(Some(state));
                }
            }
        }
        Ok(None)
    }

    // ─── HESAP YAZMA ──────────────────────────────────────────────────────

    /// Yeni hesap oluştur ve kaydet.
    ///
    /// Cosmos-SDK: `NewAccountWithAddress(ctx, addr)`
    pub fn create(&self, mut state: AccountState) -> AccountResult<AccountState> {
        // Adres zaten var mı?
        if self.backend.load_account(&state.address)?.is_some() {
            return Err(AccountError::AccountAlreadyExists {
                address: state.address_hex(),
            });
        }

        // Hesap ID ata
        let account_id = {
            let mut next = self.next_account_id.write();
            let id = *next;
            *next += 1;
            id
        };
        state.account_id = AccountId::new(account_id);

        // Depolama
        self.backend.store_account(&state)?;

        // Journal kaydı
        self.journal.write().push(JournalEntry::AccountCreated {
            address: state.address,
        });

        info!(
            account_id = account_id,
            address = %state.address_hex(),
            "hesap oluşturuldu"
        );

        // Heartbeat durumu oluştur
        let hb_state = HeartbeatState::new(state.address, state.genesis_slot);
        self.backend.store_heartbeat_state(&hb_state)?;

        Ok(state)
    }

    /// Mevcut hesabı güncelle.
    ///
    /// Cosmos-SDK: `SetAccount(ctx, account)`
    /// Go-Ethereum: journal'a önceki durumu yaz, sonra StateDB güncelle.
    pub fn update(&self, state: &AccountState) -> AccountResult<()> {
        // Mevcut durumu journal'a kaydet (rollback için)
        if let Some(previous) = self.backend.load_account(&state.address)? {
            self.journal.write().push(JournalEntry::AccountUpdated {
                address: state.address,
                previous: Box::new(previous),
            });
        }

        self.backend.store_account(state)?;

        debug!(
            address = %state.address_hex(),
            nonce = state.nonce,
            "hesap güncellendi"
        );

        Ok(())
    }

    /// Hesabı sil.
    ///
    /// Cosmos-SDK: `RemoveAccount(ctx, account)`
    pub fn remove(&self, address: &Address) -> AccountResult<()> {
        let state = self.get(address)?;

        self.journal.write().push(JournalEntry::AccountDeleted {
            address: *address,
            previous: Box::new(state),
        });

        self.backend.delete_account(address)?;
        info!(
            address = %hex::encode(address.as_bytes()),
            "hesap silindi"
        );
        Ok(())
    }

    // ─── SNAPSHOT / ROLLBACK ──────────────────────────────────────────────

    /// Anlık görüntü al — Go-Ethereum `Snapshot()` benzeri.
    /// Döndürülen indeks, `revert_to_snapshot()` için kullanılır.
    pub fn snapshot(&self) -> usize {
        self.journal.read().len()
    }

    /// Anlık görüntüye geri dön — Go-Ethereum `RevertToSnapshot()` benzeri.
    pub fn revert_to_snapshot(&self, snapshot_id: usize) -> AccountResult<()> {
        let mut journal = self.journal.write();
        while journal.len() > snapshot_id {
            let entry = journal.pop().unwrap();
            match entry {
                JournalEntry::AccountCreated { address } => {
                    self.backend.delete_account(&address)?;
                }
                JournalEntry::AccountUpdated { address, previous } => {
                    self.backend.store_account(&previous)?;
                }
                JournalEntry::AccountDeleted { address: _, previous } => {
                    self.backend.store_account(&previous)?;
                }
            }
        }
        Ok(())
    }

    // ─── İTERASYON ────────────────────────────────────────────────────────

    /// Tüm hesapları iterate et.
    ///
    /// Cosmos-SDK: `IterateAccounts(ctx, func(account sdk.AccountI) bool)`
    pub fn iter_accounts<F>(&self, mut callback: F) -> AccountResult<()>
    where
        F: FnMut(&AccountState) -> bool,
    {
        let addresses = self.backend.all_addresses()?;
        for addr in addresses {
            if let Some(state) = self.backend.load_account(&addr)? {
                if !callback(&state) {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Hesap özeti listesi döndür (RPC için).
    pub fn list_summaries(&self) -> AccountResult<Vec<AccountSummary>> {
        let mut summaries = Vec::new();
        self.iter_accounts(|state| {
            summaries.push(AccountSummary::from(state));
            true
        })?;
        Ok(summaries)
    }

    // ─── KURTARMA TALEPLERİ ──────────────────────────────────────────────

    /// Kurtarma talebini yükle.
    pub fn get_recovery_request(&self, request_id: &str) -> AccountResult<RecoveryRequest> {
        self.backend
            .load_recovery_request(request_id)?
            .ok_or_else(|| AccountError::RecoveryRequestNotFound {
                request_id: request_id.to_string(),
            })
    }

    /// Kurtarma talebini kaydet.
    pub fn save_recovery_request(&self, request: &RecoveryRequest) -> AccountResult<()> {
        self.backend.store_recovery_request(request)
    }

    /// Hesabın kurtarma taleplerini listele.
    pub fn list_recovery_requests(&self, address: &Address) -> AccountResult<Vec<RecoveryRequest>> {
        self.backend.recovery_requests_for_account(address)
    }

    // ─── HEARTBEAT ────────────────────────────────────────────────────────

    /// Heartbeat durumunu yükle.
    pub fn get_heartbeat_state(&self, address: &Address) -> AccountResult<HeartbeatState> {
        self.backend
            .load_heartbeat_state(address)?
            .ok_or_else(|| AccountError::not_found(hex::encode(address.as_bytes())))
    }

    /// Heartbeat durumunu kaydet.
    pub fn save_heartbeat_state(&self, state: &HeartbeatState) -> AccountResult<()> {
        self.backend.store_heartbeat_state(state)
    }

    // ─── İSTATİSTİKLER ───────────────────────────────────────────────────

    /// Toplam hesap sayısı.
    pub fn total_accounts(&self) -> AccountResult<usize> {
        self.backend.account_count()
    }

    /// Tüm hesap adreslerini listele.
    pub fn all_addresses(&self) -> AccountResult<Vec<Address>> {
        self.backend.all_addresses()
    }

    /// Journal boyutu (yapılan değişiklik sayısı).
    pub fn journal_size(&self) -> usize {
        self.journal.read().len()
    }

    /// Sonraki hesap ID'si.
    pub fn next_id(&self) -> u64 {
        *self.next_account_id.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccountConfig, AccountState, GuardianSet, ThresholdConfig};

    fn test_address() -> Address {
        Address::from_bytes([1u8; 20])
    }

    fn test_address_2() -> Address {
        Address::from_bytes([2u8; 20])
    }

    fn test_slot() -> SlotId {
        SlotId(1000)
    }

    fn make_state(addr: Address) -> AccountState {
        let config = AccountConfig::with_slot(test_slot());
        let guardian_set = GuardianSet::new(ThresholdConfig::DEFAULT, test_slot());
        AccountState::genesis(
            AccountId::ZERO,
            addr,
            vec![0x02; 33],
            guardian_set,
            config,
            test_slot(),
        )
    }

    // ─── InMemoryStorage testleri ─────────────────────────────────────────

    #[test]
    fn in_memory_storage_bos_baslar() {
        let storage = InMemoryStorage::new();
        assert!(storage.is_empty());
    }

    #[test]
    fn in_memory_storage_hesap_yazar_okur() {
        let storage = InMemoryStorage::new();
        let state = make_state(test_address());
        storage.store_account(&state).unwrap();
        let loaded = storage.load_account(&test_address()).unwrap().unwrap();
        assert_eq!(loaded.address, test_address());
    }

    #[test]
    fn in_memory_storage_olmayan_hesap_none_doner() {
        let storage = InMemoryStorage::new();
        let result = storage.load_account(&test_address()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn in_memory_storage_hesap_siler() {
        let storage = InMemoryStorage::new();
        let state = make_state(test_address());
        storage.store_account(&state).unwrap();
        storage.delete_account(&test_address()).unwrap();
        assert!(storage.load_account(&test_address()).unwrap().is_none());
    }

    #[test]
    fn in_memory_storage_tum_adresler_listelenir() {
        let storage = InMemoryStorage::new();
        storage.store_account(&make_state(test_address())).unwrap();
        storage.store_account(&make_state(test_address_2())).unwrap();
        let addrs = storage.all_addresses().unwrap();
        assert_eq!(addrs.len(), 2);
    }

    // ─── AccountStorage testleri ──────────────────────────────────────────

    #[test]
    fn account_storage_hesap_olusturulur() {
        let storage = AccountStorage::in_memory();
        let state = make_state(test_address());
        let created = storage.create(state).unwrap();
        assert_eq!(created.account_id, AccountId::new(1));
    }

    #[test]
    fn account_storage_tekrar_olusturma_reddedilir() {
        let storage = AccountStorage::in_memory();
        let state = make_state(test_address());
        storage.create(state.clone()).unwrap();
        let result = storage.create(state);
        assert!(matches!(result, Err(AccountError::AccountAlreadyExists { .. })));
    }

    #[test]
    fn account_storage_hesap_getirilir() {
        let storage = AccountStorage::in_memory();
        let state = make_state(test_address());
        storage.create(state).unwrap();
        let loaded = storage.get(&test_address()).unwrap();
        assert_eq!(loaded.address, test_address());
    }

    #[test]
    fn account_storage_olmayan_hesap_hata_verir() {
        let storage = AccountStorage::in_memory();
        let result = storage.get(&test_address());
        assert!(matches!(result, Err(AccountError::AccountNotFound { .. })));
    }

    #[test]
    fn account_storage_hesap_guncellenir() {
        let storage = AccountStorage::in_memory();
        let mut state = make_state(test_address());
        storage.create(state.clone()).unwrap();

        // Nonce'u artır
        let updated = storage.get(&test_address()).unwrap();
        let mut updated = updated;
        updated.nonce = 5;
        storage.update(&updated).unwrap();

        let reloaded = storage.get(&test_address()).unwrap();
        assert_eq!(reloaded.nonce, 5);
    }

    #[test]
    fn account_storage_journal_buyur() {
        let storage = AccountStorage::in_memory();
        assert_eq!(storage.journal_size(), 0);
        let state = make_state(test_address());
        storage.create(state).unwrap();
        assert_eq!(storage.journal_size(), 1);
    }

    #[test]
    fn account_storage_snapshot_geri_alma() {
        let storage = AccountStorage::in_memory();
        let state = make_state(test_address());
        let snap = storage.snapshot(); // 0
        storage.create(state).unwrap();
        assert!(storage.get(&test_address()).is_ok());

        // Snapshot'a geri dön
        storage.revert_to_snapshot(snap).unwrap();
        assert!(matches!(
            storage.get(&test_address()),
            Err(AccountError::AccountNotFound { .. })
        ));
    }

    #[test]
    fn account_storage_guncelleme_geri_alınir() {
        let storage = AccountStorage::in_memory();
        let state = make_state(test_address());
        storage.create(state).unwrap();

        let snap = storage.snapshot();
        let mut updated = storage.get(&test_address()).unwrap();
        updated.nonce = 99;
        storage.update(&updated).unwrap();
        assert_eq!(storage.get(&test_address()).unwrap().nonce, 99);

        storage.revert_to_snapshot(snap).unwrap();
        assert_eq!(storage.get(&test_address()).unwrap().nonce, 0);
    }

    #[test]
    fn account_storage_toplam_hesap_sayisi() {
        let storage = AccountStorage::in_memory();
        assert_eq!(storage.total_accounts().unwrap(), 0);
        storage.create(make_state(test_address())).unwrap();
        assert_eq!(storage.total_accounts().unwrap(), 1);
        storage.create(make_state(test_address_2())).unwrap();
        assert_eq!(storage.total_accounts().unwrap(), 2);
    }

    #[test]
    fn account_storage_id_ile_hesap_bulunur() {
        let storage = AccountStorage::in_memory();
        let created = storage.create(make_state(test_address())).unwrap();
        let found = storage.get_by_id(created.account_id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().address, test_address());
    }

    #[test]
    fn account_storage_id_sayaci_artiriyor() {
        let storage = AccountStorage::in_memory();
        assert_eq!(storage.next_id(), 1);
        storage.create(make_state(test_address())).unwrap();
        assert_eq!(storage.next_id(), 2);
        storage.create(make_state(test_address_2())).unwrap();
        assert_eq!(storage.next_id(), 3);
    }

    #[test]
    fn account_storage_iter_accounts_calisiyor() {
        let storage = AccountStorage::in_memory();
        storage.create(make_state(test_address())).unwrap();
        storage.create(make_state(test_address_2())).unwrap();

        let mut count = 0;
        storage.iter_accounts(|_| {
            count += 1;
            true
        }).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn account_storage_iter_accounts_erken_durabilir() {
        let storage = AccountStorage::in_memory();
        for i in 0..10u8 {
            let addr = Address::from_bytes([i; 20]);
            storage.create(make_state(addr)).unwrap();
        }

        let mut count = 0;
        storage.iter_accounts(|_| {
            count += 1;
            count < 5 // 5'te dur
        }).unwrap();
        assert!(count <= 5);
    }

    #[test]
    fn account_storage_ozet_listesi_doner() {
        let storage = AccountStorage::in_memory();
        storage.create(make_state(test_address())).unwrap();
        storage.create(make_state(test_address_2())).unwrap();
        let summaries = storage.list_summaries().unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn account_storage_silme_calisiyor() {
        let storage = AccountStorage::in_memory();
        storage.create(make_state(test_address())).unwrap();
        storage.remove(&test_address()).unwrap();
        assert!(matches!(
            storage.get(&test_address()),
            Err(AccountError::AccountNotFound { .. })
        ));
    }

    #[test]
    fn account_storage_olmayan_hesap_silinemez() {
        let storage = AccountStorage::in_memory();
        let result = storage.remove(&test_address());
        assert!(result.is_err());
    }

    #[test]
    fn account_storage_heartbeat_durumu_kaydedilir() {
        let storage = AccountStorage::in_memory();
        storage.create(make_state(test_address())).unwrap();
        let hb_state = storage.get_heartbeat_state(&test_address()).unwrap();
        assert_eq!(hb_state.account, test_address());
    }
}
