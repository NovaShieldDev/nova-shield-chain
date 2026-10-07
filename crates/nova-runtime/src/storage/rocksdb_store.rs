//! RocksDB kalıcı depolama katmanı — Column Family mimarisi (FAZ J)
//!
//! ## Mimari
//!
//! ```text
//! RocksDbStore
//!   ├── CF: accounts      — Address(20B) → AccountState (bincode)
//!   ├── CF: blocks        — height u64 BE(8B) → StoredBlock (bincode)
//!   ├── CF: transactions  — tx_hash [u8;32] → StoredTx (bincode)
//!   └── CF: metadata      — str key → raw bytes
//! ```
//!
//! ## Referanslar
//! - go-ethereum `core/rawdb/accessors_chain.go` — blok/TX depolama
//! - Polkadot `sc-client-db/src/lib.rs` — column family yaklaşımı
//! - Solana `ledger/src/blockstore.rs` — RocksDB wrapper pattern

use crate::state::account::AccountState;
use nova_primitives::Address;
use rocksdb::{
    ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options,
    WriteBatch as RocksWriteBatch,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

// ─── Column Family isimleri ───────────────────────────────────────────────────

const CF_ACCOUNTS: &str = "accounts";
const CF_BLOCKS: &str = "blocks";
const CF_TRANSACTIONS: &str = "transactions";
const CF_METADATA: &str = "metadata";

/// Tüm column family isimleri
pub const ALL_CFS: &[&str] = &[CF_ACCOUNTS, CF_BLOCKS, CF_TRANSACTIONS, CF_METADATA];

// ─── Hata tipleri ─────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("RocksDB hatası: {0}")]
    RocksDb(#[from] rocksdb::Error),

    #[error("Serileştirme hatası: {0}")]
    Serialize(#[from] bincode::Error),

    #[error("Column family bulunamadı: {0}")]
    MissingCf(String),

    #[error("Anahtar bulunamadı")]
    NotFound,

    #[error("Geçersiz veri: {0}")]
    Corrupt(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

// ─── Depolanan tipler ─────────────────────────────────────────────────────────

/// Veritabanında saklanan blok kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredBlock {
    pub height: u64,
    /// BLAKE3 blok hash'i
    pub hash: [u8; 32],
    /// Parent blok hash'i
    pub parent_hash: [u8; 32],
    /// State root
    pub state_root: [u8; 32],
    /// TX Merkle kökü
    pub tx_root: [u8; 32],
    /// Bloğu öneren validator
    pub proposer_id: u64,
    /// Baz ücret
    pub base_fee: u128,
    /// Kullanılan gas
    pub gas_used: u64,
    /// Gas limiti
    pub gas_limit: u64,
    /// Unix timestamp (sn)
    pub timestamp: u64,
    /// TX hash listesi
    pub tx_hashes: Vec<[u8; 32]>,
}

impl StoredBlock {
    /// Blok anahtarı: height'ı 8-byte big-endian'a çevir
    pub fn key(height: u64) -> [u8; 8] {
        height.to_be_bytes()
    }
}

/// Veritabanında saklanan işlem kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTx {
    /// TX hash'i (anahtar kopyası)
    pub tx_hash: [u8; 32],
    /// Gönderici adres
    pub from: [u8; 20],
    /// Alıcı adres (contract create = None)
    pub to: Option<[u8; 20]>,
    /// Transfer miktarı
    pub value: u128,
    /// Nonce
    pub nonce: u64,
    /// Gas limiti
    pub gas_limit: u64,
    /// Max ücret (wei/gas)
    pub max_fee: u128,
    /// TX bulunduğu blok yüksekliği
    pub block_height: u64,
    /// TX'in bloktaki sırası
    pub tx_index: u32,
    /// Başarılı mı?
    pub success: bool,
    /// Harcanan gas
    pub gas_used: u64,
    /// Ham TX verisi
    pub data: Vec<u8>,
}

// ─── WriteBatch ───────────────────────────────────────────────────────────────

/// Atomik yazma grubu — tüm değişiklikler birlikte commit edilir
///
/// go-ethereum `rawdb` batch yazma yaklaşımından ilham.
#[derive(Default)]
pub struct WriteBatch {
    pub(crate) account_writes: Vec<(Address, AccountState)>,
    pub(crate) account_deletes: Vec<Address>,
    pub(crate) block_writes: Vec<StoredBlock>,
    pub(crate) tx_writes: Vec<StoredTx>,
    pub(crate) meta_writes: Vec<(String, Vec<u8>)>,
}

impl WriteBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_account(&mut self, addr: Address, state: AccountState) -> &mut Self {
        self.account_writes.push((addr, state));
        self
    }

    pub fn delete_account(&mut self, addr: Address) -> &mut Self {
        self.account_deletes.push(addr);
        self
    }

    pub fn put_block(&mut self, block: StoredBlock) -> &mut Self {
        self.block_writes.push(block);
        self
    }

    pub fn put_tx(&mut self, tx: StoredTx) -> &mut Self {
        self.tx_writes.push(tx);
        self
    }

    pub fn put_meta(&mut self, key: impl Into<String>, value: Vec<u8>) -> &mut Self {
        self.meta_writes.push((key.into(), value));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.account_writes.is_empty()
            && self.account_deletes.is_empty()
            && self.block_writes.is_empty()
            && self.tx_writes.is_empty()
            && self.meta_writes.is_empty()
    }
}

// ─── StateBackend trait ───────────────────────────────────────────────────────

/// Depolama arka uç arayüzü — hem in-memory hem de RocksDB için
///
/// Bu soyutlama sayesinde runtime, depolama implementasyonundan bağımsız çalışır.
/// Test ortamı `AccountStateCache`'i kullanırken, prodüksiyon `RocksDbStore` kullanır.
pub trait StateBackend: Send + Sync {
    // ── Hesap CRUD ───────────────────────────────────────────────────────────

    /// Hesabı oku (yoksa None)
    fn get_account(&self, addr: &Address) -> StorageResult<Option<AccountState>>;

    /// Hesabı yaz
    fn put_account(&self, addr: Address, state: &AccountState) -> StorageResult<()>;

    /// Hesabı sil
    fn delete_account(&self, addr: &Address) -> StorageResult<()>;

    // ── Blok depolama ────────────────────────────────────────────────────────

    /// Bloku yüksekliğe göre oku
    fn get_block(&self, height: u64) -> StorageResult<Option<StoredBlock>>;

    /// Blok hash'ine göre yükseklik ara
    fn get_block_by_hash(&self, hash: &[u8; 32]) -> StorageResult<Option<StoredBlock>>;

    /// Blok yaz
    fn put_block(&self, block: &StoredBlock) -> StorageResult<()>;

    /// En son finalize edilmiş blok yüksekliği
    fn get_latest_height(&self) -> StorageResult<Option<u64>>;

    // ── İşlem depolama ───────────────────────────────────────────────────────

    /// TX'i hash'e göre oku
    fn get_tx(&self, tx_hash: &[u8; 32]) -> StorageResult<Option<StoredTx>>;

    /// TX yaz
    fn put_tx(&self, tx: &StoredTx) -> StorageResult<()>;

    // ── Metadata ─────────────────────────────────────────────────────────────

    /// Metadata değeri oku
    fn get_meta(&self, key: &str) -> StorageResult<Option<Vec<u8>>>;

    /// Metadata değeri yaz
    fn put_meta(&self, key: &str, value: &[u8]) -> StorageResult<()>;

    // ── Atomik yazma ─────────────────────────────────────────────────────────

    /// Batch'i atomik olarak commit et
    fn write_batch(&self, batch: WriteBatch) -> StorageResult<()>;

    /// Tüm bekleyen yazmaları diske flush et
    fn flush(&self) -> StorageResult<()>;
}

// ─── RocksDbStore ─────────────────────────────────────────────────────────────

/// Production-grade RocksDB depolama arka ucu
///
/// `Arc<DBWithThreadMode<MultiThreaded>>` ile thread-safe paylaşım.
/// Column family başına ayrı `Options` ile her CF optimize edilebilir.
#[derive(Clone)]
pub struct RocksDbStore {
    db: Arc<DBWithThreadMode<MultiThreaded>>,
}

impl RocksDbStore {
    /// Veritabanını aç (yoksa oluştur)
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        db_opts.set_max_open_files(512);
        db_opts.set_write_buffer_size(64 * 1024 * 1024); // 64 MB write buffer
        db_opts.set_max_write_buffer_number(3);

        let cfs: Vec<ColumnFamilyDescriptor> = ALL_CFS
            .iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                // Bloklar için: büyük sequential IO, daha az bloom filter
                if *name == CF_BLOCKS || *name == CF_TRANSACTIONS {
                    cf_opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                }
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = DBWithThreadMode::<MultiThreaded>::open_cf_descriptors(
            &db_opts,
            path.as_ref(),
            cfs,
        )?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Sadece okuma için aç (snapshot/archive node)
    pub fn open_readonly(path: impl AsRef<Path>) -> StorageResult<Self> {
        let db_opts = Options::default();
        let cf_names: Vec<&str> = ALL_CFS.to_vec();
        let db = DBWithThreadMode::<MultiThreaded>::open_cf_for_read_only(
            &db_opts,
            path.as_ref(),
            cf_names,
            false,
        )?;
        Ok(Self { db: Arc::new(db) })
    }

    // ── Dahili yardımcılar ────────────────────────────────────────────────────

    fn cf(&self, name: &str) -> StorageResult<Arc<rocksdb::BoundColumnFamily>> {
        self.db.cf_handle(name).ok_or_else(|| StorageError::MissingCf(name.to_string()))
    }

    fn encode<T: Serialize>(val: &T) -> StorageResult<Vec<u8>> {
        bincode::serialize(val).map_err(StorageError::Serialize)
    }

    fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> StorageResult<T> {
        bincode::deserialize(bytes).map_err(StorageError::Serialize)
    }
}

impl StateBackend for RocksDbStore {
    // ── Hesap ────────────────────────────────────────────────────────────────

    fn get_account(&self, addr: &Address) -> StorageResult<Option<AccountState>> {
        let cf = self.cf(CF_ACCOUNTS)?;
        match self.db.get_cf(&cf, addr.as_bytes())? {
            Some(bytes) => Ok(Some(Self::decode(&bytes)?)),
            None => Ok(None),
        }
    }

    fn put_account(&self, addr: Address, state: &AccountState) -> StorageResult<()> {
        let cf = self.cf(CF_ACCOUNTS)?;
        let bytes = Self::encode(state)?;
        self.db.put_cf(&cf, addr.as_bytes(), bytes)?;
        Ok(())
    }

    fn delete_account(&self, addr: &Address) -> StorageResult<()> {
        let cf = self.cf(CF_ACCOUNTS)?;
        self.db.delete_cf(&cf, addr.as_bytes())?;
        Ok(())
    }

    // ── Blok ─────────────────────────────────────────────────────────────────

    fn get_block(&self, height: u64) -> StorageResult<Option<StoredBlock>> {
        let cf = self.cf(CF_BLOCKS)?;
        match self.db.get_cf(&cf, StoredBlock::key(height))? {
            Some(bytes) => Ok(Some(Self::decode(&bytes)?)),
            None => Ok(None),
        }
    }

    fn get_block_by_hash(&self, hash: &[u8; 32]) -> StorageResult<Option<StoredBlock>> {
        // Hash → height index'i metadata CF'de saklanır
        let cf_meta = self.cf(CF_METADATA)?;
        let mut meta_key = b"block_hash:".to_vec();
        meta_key.extend_from_slice(hash);
        match self.db.get_cf(&cf_meta, &meta_key)? {
            Some(bytes) if bytes.len() == 8 => {
                let height = u64::from_be_bytes(bytes.try_into().unwrap());
                self.get_block(height)
            }
            _ => Ok(None),
        }
    }

    fn put_block(&self, block: &StoredBlock) -> StorageResult<()> {
        let cf_blocks = self.cf(CF_BLOCKS)?;
        let cf_meta = self.cf(CF_METADATA)?;
        let bytes = Self::encode(block)?;

        // Blok verisini yaz
        self.db.put_cf(&cf_blocks, StoredBlock::key(block.height), &bytes)?;

        // Hash → height index
        let mut hash_key = b"block_hash:".to_vec();
        hash_key.extend_from_slice(&block.hash);
        self.db.put_cf(&cf_meta, hash_key, block.height.to_be_bytes())?;

        // Son yüksekliği güncelle
        let current = self.get_latest_height()?.unwrap_or(0);
        if block.height >= current {
            self.db.put_cf(
                &cf_meta,
                b"latest_height",
                block.height.to_be_bytes(),
            )?;
        }
        Ok(())
    }

    fn get_latest_height(&self) -> StorageResult<Option<u64>> {
        let cf = self.cf(CF_METADATA)?;
        match self.db.get_cf(&cf, b"latest_height")? {
            Some(bytes) if bytes.len() == 8 => {
                let h = u64::from_be_bytes(bytes.try_into().unwrap());
                Ok(Some(h))
            }
            _ => Ok(None),
        }
    }

    // ── İşlem ────────────────────────────────────────────────────────────────

    fn get_tx(&self, tx_hash: &[u8; 32]) -> StorageResult<Option<StoredTx>> {
        let cf = self.cf(CF_TRANSACTIONS)?;
        match self.db.get_cf(&cf, tx_hash)? {
            Some(bytes) => Ok(Some(Self::decode(&bytes)?)),
            None => Ok(None),
        }
    }

    fn put_tx(&self, tx: &StoredTx) -> StorageResult<()> {
        let cf = self.cf(CF_TRANSACTIONS)?;
        let bytes = Self::encode(tx)?;
        self.db.put_cf(&cf, tx.tx_hash, bytes)?;
        Ok(())
    }

    // ── Metadata ─────────────────────────────────────────────────────────────

    fn get_meta(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let cf = self.cf(CF_METADATA)?;
        Ok(self.db.get_cf(&cf, key.as_bytes())?)
    }

    fn put_meta(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        let cf = self.cf(CF_METADATA)?;
        self.db.put_cf(&cf, key.as_bytes(), value)?;
        Ok(())
    }

    // ── Atomik batch ─────────────────────────────────────────────────────────

    fn write_batch(&self, batch: WriteBatch) -> StorageResult<()> {
        if batch.is_empty() {
            return Ok(());
        }

        let cf_accounts = self.cf(CF_ACCOUNTS)?;
        let cf_blocks = self.cf(CF_BLOCKS)?;
        let cf_txs = self.cf(CF_TRANSACTIONS)?;
        let cf_meta = self.cf(CF_METADATA)?;

        let mut rocks_batch = RocksWriteBatch::default();
        let mut new_max_height: Option<u64> = None;

        // Hesap yazmaları
        for (addr, state) in &batch.account_writes {
            let bytes = Self::encode(state)?;
            rocks_batch.put_cf(&cf_accounts, addr.as_bytes(), bytes);
        }

        // Hesap silmeleri
        for addr in &batch.account_deletes {
            rocks_batch.delete_cf(&cf_accounts, addr.as_bytes());
        }

        // Blok yazmaları
        for block in &batch.block_writes {
            let bytes = Self::encode(block)?;
            rocks_batch.put_cf(&cf_blocks, StoredBlock::key(block.height), &bytes);

            // Hash index
            let mut hash_key = b"block_hash:".to_vec();
            hash_key.extend_from_slice(&block.hash);
            rocks_batch.put_cf(&cf_meta, hash_key, block.height.to_be_bytes());

            // En yüksek yüksekliği izle
            match new_max_height {
                Some(h) if block.height <= h => {}
                _ => new_max_height = Some(block.height),
            }
        }

        // TX yazmaları
        for tx in &batch.tx_writes {
            let bytes = Self::encode(tx)?;
            rocks_batch.put_cf(&cf_txs, tx.tx_hash, bytes);
        }

        // Metadata yazmaları
        for (key, value) in &batch.meta_writes {
            rocks_batch.put_cf(&cf_meta, key.as_bytes(), value);
        }

        // Son yükseklik meta güncellemesi
        if let Some(height) = new_max_height {
            let current = self.get_latest_height()?.unwrap_or(0);
            if height >= current {
                rocks_batch.put_cf(&cf_meta, b"latest_height", height.to_be_bytes());
            }
        }

        self.db.write(rocks_batch)?;
        Ok(())
    }

    fn flush(&self) -> StorageResult<()> {
        self.db.flush()?;
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_db_path() -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("nova_rocksdb_test_{}_{}", std::process::id(), id))
    }

    fn open_db() -> RocksDbStore {
        RocksDbStore::open(temp_db_path()).expect("DB açılmalı")
    }

    fn make_addr(b: u8) -> Address {
        Address::from_bytes([b; 20])
    }

    fn make_account(balance: u128, nonce: u64) -> AccountState {
        AccountState { nonce, balance, code_hash: None, storage_root: None }
    }

    fn make_block(height: u64) -> StoredBlock {
        StoredBlock {
            height,
            hash: {
                let mut h = [0u8; 32];
                h[..8].copy_from_slice(&height.to_be_bytes());
                h
            },
            parent_hash: [0u8; 32],
            state_root: [1u8; 32],
            tx_root: [2u8; 32],
            proposer_id: 1,
            base_fee: 1_000_000_000,
            gas_used: 21000,
            gas_limit: 30_000_000,
            timestamp: 1_700_000_000 + height,
            tx_hashes: vec![],
        }
    }

    fn make_tx(hash_byte: u8, block_height: u64) -> StoredTx {
        StoredTx {
            tx_hash: [hash_byte; 32],
            from: [0xAAu8; 20],
            to: Some([0xBBu8; 20]),
            value: 1_000_000,
            nonce: 0,
            gas_limit: 21_000,
            max_fee: 1_000_000_000,
            block_height,
            tx_index: 0,
            success: true,
            gas_used: 21_000,
            data: vec![],
        }
    }

    // ── Hesap testleri ────────────────────────────────────────────────────────

    #[test]
    fn test_account_get_put() {
        let db = open_db();
        let addr = make_addr(0x01);
        let state = make_account(1_000_000, 5);

        assert!(db.get_account(&addr).unwrap().is_none());
        db.put_account(addr, &state).unwrap();
        let loaded = db.get_account(&addr).unwrap().unwrap();
        assert_eq!(loaded.balance, 1_000_000);
        assert_eq!(loaded.nonce, 5);
    }

    #[test]
    fn test_account_update() {
        let db = open_db();
        let addr = make_addr(0x02);

        db.put_account(addr, &make_account(100, 0)).unwrap();
        db.put_account(addr, &make_account(200, 1)).unwrap();
        let loaded = db.get_account(&addr).unwrap().unwrap();
        assert_eq!(loaded.balance, 200);
        assert_eq!(loaded.nonce, 1);
    }

    #[test]
    fn test_account_delete() {
        let db = open_db();
        let addr = make_addr(0x03);

        db.put_account(addr, &make_account(500, 0)).unwrap();
        assert!(db.get_account(&addr).unwrap().is_some());
        db.delete_account(&addr).unwrap();
        assert!(db.get_account(&addr).unwrap().is_none());
    }

    #[test]
    fn test_account_not_found_returns_none() {
        let db = open_db();
        let addr = make_addr(0xFF);
        assert!(db.get_account(&addr).unwrap().is_none());
    }

    // ── Blok testleri ─────────────────────────────────────────────────────────

    #[test]
    fn test_block_get_put() {
        let db = open_db();
        let block = make_block(42);

        assert!(db.get_block(42).unwrap().is_none());
        db.put_block(&block).unwrap();
        let loaded = db.get_block(42).unwrap().unwrap();
        assert_eq!(loaded.height, 42);
        assert_eq!(loaded.base_fee, 1_000_000_000);
    }

    #[test]
    fn test_block_by_hash() {
        let db = open_db();
        let block = make_block(10);
        db.put_block(&block).unwrap();
        let loaded = db.get_block_by_hash(&block.hash).unwrap().unwrap();
        assert_eq!(loaded.height, 10);
    }

    #[test]
    fn test_latest_height_tracking() {
        let db = open_db();
        assert!(db.get_latest_height().unwrap().is_none());

        db.put_block(&make_block(1)).unwrap();
        assert_eq!(db.get_latest_height().unwrap(), Some(1));

        db.put_block(&make_block(5)).unwrap();
        assert_eq!(db.get_latest_height().unwrap(), Some(5));

        // Eski blok → latest değişmemeli
        db.put_block(&make_block(3)).unwrap();
        assert_eq!(db.get_latest_height().unwrap(), Some(5));
    }

    #[test]
    fn test_block_sequential_chain() {
        let db = open_db();
        for h in 0..10u64 {
            db.put_block(&make_block(h)).unwrap();
        }
        assert_eq!(db.get_latest_height().unwrap(), Some(9));
        for h in 0..10u64 {
            assert_eq!(db.get_block(h).unwrap().unwrap().height, h);
        }
    }

    // ── TX testleri ───────────────────────────────────────────────────────────

    #[test]
    fn test_tx_get_put() {
        let db = open_db();
        let tx = make_tx(0xAA, 1);

        assert!(db.get_tx(&tx.tx_hash).unwrap().is_none());
        db.put_tx(&tx).unwrap();
        let loaded = db.get_tx(&tx.tx_hash).unwrap().unwrap();
        assert_eq!(loaded.value, 1_000_000);
        assert!(loaded.success);
    }

    #[test]
    fn test_tx_not_found() {
        let db = open_db();
        assert!(db.get_tx(&[0u8; 32]).unwrap().is_none());
    }

    // ── Metadata testleri ─────────────────────────────────────────────────────

    #[test]
    fn test_metadata_get_put() {
        let db = open_db();
        assert!(db.get_meta("chain_id").unwrap().is_none());
        db.put_meta("chain_id", b"nova-42097").unwrap();
        assert_eq!(db.get_meta("chain_id").unwrap().unwrap(), b"nova-42097");
    }

    #[test]
    fn test_metadata_overwrite() {
        let db = open_db();
        db.put_meta("version", b"1").unwrap();
        db.put_meta("version", b"2").unwrap();
        assert_eq!(db.get_meta("version").unwrap().unwrap(), b"2");
    }

    // ── WriteBatch testleri ───────────────────────────────────────────────────

    #[test]
    fn test_write_batch_accounts() {
        let db = open_db();
        let mut batch = WriteBatch::new();
        for i in 0u8..10 {
            batch.put_account(make_addr(i), make_account(i as u128 * 1000, i as u64));
        }
        db.write_batch(batch).unwrap();

        for i in 0u8..10 {
            let acc = db.get_account(&make_addr(i)).unwrap().unwrap();
            assert_eq!(acc.balance, i as u128 * 1000);
        }
    }

    #[test]
    fn test_write_batch_blocks_and_txs() {
        let db = open_db();
        let mut batch = WriteBatch::new();
        for h in 0..5u64 {
            batch.put_block(make_block(h));
            batch.put_tx(make_tx(h as u8, h));
        }
        db.write_batch(batch).unwrap();

        for h in 0..5u64 {
            assert!(db.get_block(h).unwrap().is_some());
        }
        assert_eq!(db.get_latest_height().unwrap(), Some(4));
    }

    #[test]
    fn test_write_batch_atomic_delete() {
        let db = open_db();
        let addr = make_addr(0x10);
        db.put_account(addr, &make_account(999, 0)).unwrap();

        let mut batch = WriteBatch::new();
        batch.delete_account(addr);
        db.write_batch(batch).unwrap();

        assert!(db.get_account(&addr).unwrap().is_none());
    }

    #[test]
    fn test_write_batch_empty_is_noop() {
        let db = open_db();
        db.write_batch(WriteBatch::new()).unwrap();
        // Sadece hata çıkmaması yeterli
    }

    // ── Flush ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_flush() {
        let db = open_db();
        db.put_account(make_addr(0x20), &make_account(100, 0)).unwrap();
        db.flush().unwrap();
    }

    // ── Persist / re-open ─────────────────────────────────────────────────────

    #[test]
    fn test_data_persists_across_reopen() {
        let path = temp_db_path();
        {
            let db = RocksDbStore::open(&path).unwrap();
            db.put_account(make_addr(0x30), &make_account(42_000, 7)).unwrap();
            db.put_block(&make_block(100)).unwrap();
            db.flush().unwrap();
        }
        // Yeni handle ile aynı dizini aç
        {
            let db2 = RocksDbStore::open(&path).unwrap();
            let acc = db2.get_account(&make_addr(0x30)).unwrap().unwrap();
            assert_eq!(acc.balance, 42_000);
            assert_eq!(acc.nonce, 7);
            assert_eq!(db2.get_block(100).unwrap().unwrap().height, 100);
            assert_eq!(db2.get_latest_height().unwrap(), Some(100));
        }
    }

    // ── WriteBatch is_empty ───────────────────────────────────────────────────

    #[test]
    fn test_write_batch_is_empty() {
        let mut batch = WriteBatch::new();
        assert!(batch.is_empty());
        batch.put_meta("k", vec![1, 2, 3]);
        assert!(!batch.is_empty());
    }

    // ── StoredBlock key encoding ──────────────────────────────────────────────

    #[test]
    fn test_stored_block_key_ordering() {
        // BE encoding → lexicographic sıralama = sayısal sıralama
        let k0 = StoredBlock::key(0);
        let k1 = StoredBlock::key(1);
        let k_max = StoredBlock::key(u64::MAX);
        assert!(k0 < k1);
        assert!(k1 < k_max);
    }
}
