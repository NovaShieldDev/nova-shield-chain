//! GÖREV 90: AppendVec — Solana Memory-Mapped Account Storage
//!
//! ## Mimari
//!
//! ### Solana AppendVec
//! - Hesap verilerini append-only dosyaya yazar (mmap benzeri)
//! - Sabit slot büyüklüğü: her hesap = header(fixed) + data(variable)
//! - Okuma sıfır-kopyalı (mmap): doğrudan bellek adreslemesi
//! - Yazma atomic: yeni hesap sadece append edilir, üzerine yazılmaz
//! - Silinmiş hesaplar: `lamports=0` ile işaretlenir (compact sonra silinir)
//!
//! ### Nova Uyarlaması
//! - Gerçek mmap yerine `Vec<u8>` (platform bağımsız test)
//! - API aynı: `append()`, `get()`, `get_mut()`, `flush()`
//! - `AccountSlot`: başlık + veri
//! - `AppendVecStore`: çok sayıda AppendVec yönetir (slot bazlı)

use nova_primitives::Address;
use std::collections::BTreeMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Hesap başlığı boyutu (sabit)
pub const ACCOUNT_HEADER_SIZE: usize = 64;

/// Varsayılan AppendVec kapasitesi (bayt)
pub const DEFAULT_APPEND_VEC_CAPACITY: usize = 64 * 1024 * 1024; // 64 MB

/// Maksimum hesap verisi boyutu
pub const MAX_ACCOUNT_DATA_SIZE: usize = 10 * 1024 * 1024; // 10 MB

// ─── StoredAccountMeta ────────────────────────────────────────────────────────

/// Bir hesabın AppendVec'teki meta verisi
///
/// Solana: `StoredAccountMeta { meta, account_meta, data, offset, data_len }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAccountMeta {
    /// Hesap adresi
    pub address: Address,
    /// Bakiye (u64 — header'da 8 byte)
    pub lamports: u64,
    /// Veri uzunluğu
    pub data_len: u32,
    /// Sahibi (program adresi)
    pub owner: Address,
    /// Yürütülebilir mi?
    pub executable: bool,
    /// Oluşturulduğu slot
    pub rent_epoch: u64,
}

impl StoredAccountMeta {
    /// Başlık + veri toplam boyutu
    pub fn stored_size(&self, data_len: u32) -> usize {
        ACCOUNT_HEADER_SIZE + data_len as usize
    }

    /// Hesap silinmiş mi? (lamports=0)
    pub fn is_dead(&self) -> bool {
        self.lamports == 0
    }
}

// ─── AccountSlot ──────────────────────────────────────────────────────────────

/// AppendVec'te saklanan hesap (meta + veri)
#[derive(Debug, Clone)]
pub struct AccountSlot {
    pub meta: StoredAccountMeta,
    pub data: Vec<u8>,
    /// Bu slot'un AppendVec içindeki bayt ofseti
    pub offset: usize,
}

impl AccountSlot {
    pub fn new(meta: StoredAccountMeta, data: Vec<u8>, offset: usize) -> Self {
        Self { meta, data, offset }
    }

    pub fn total_size(&self) -> usize {
        ACCOUNT_HEADER_SIZE + self.data.len()
    }
}

// ─── AppendVec ────────────────────────────────────────────────────────────────

/// Append-only hesap depolama
///
/// Solana: `AppendVec { path, map: MmapMut, current_len, file_size }`
/// Nova: `Vec<u8>` backend (gerçek implementasyonda `memmap2` kullanılır)
#[derive(Debug)]
pub struct AppendVec {
    /// Depolama adı (gerçekte dosya yolu)
    pub name: String,
    /// Dahili buffer (gerçekte memory-mapped file)
    buffer: Vec<u8>,
    /// Mevcut yazma pozisyonu
    current_len: usize,
    /// Kapasite
    capacity: usize,
    /// Hesap offsetleri: address → [offset, ...]  (birden fazla versiyon)
    index: BTreeMap<[u8; 20], Vec<usize>>,
}

impl AppendVec {
    pub fn new(name: impl Into<String>, capacity: usize) -> Self {
        Self {
            name: name.into(),
            buffer: vec![0u8; capacity],
            current_len: 0,
            capacity,
            index: BTreeMap::new(),
        }
    }

    pub fn with_default_capacity(name: impl Into<String>) -> Self {
        Self::new(name, DEFAULT_APPEND_VEC_CAPACITY)
    }

    /// Hesap append et
    ///
    /// Solana: `append_account_data(storage_size, account, hash)`
    /// Returns: başlangıç ofseti (None = kapasite doldu)
    pub fn append(&mut self, meta: StoredAccountMeta, data: Vec<u8>) -> Option<usize> {
        if data.len() > MAX_ACCOUNT_DATA_SIZE {
            return None;
        }

        let total = ACCOUNT_HEADER_SIZE + data.len();
        if self.current_len + total > self.capacity {
            return None; // kapasite doldu
        }

        let offset = self.current_len;

        // Başlık yaz (sabit 64 byte)
        let header = self.serialize_header(&meta, data.len() as u32);
        self.buffer[offset..offset + ACCOUNT_HEADER_SIZE].copy_from_slice(&header);

        // Veri yaz
        let data_start = offset + ACCOUNT_HEADER_SIZE;
        self.buffer[data_start..data_start + data.len()].copy_from_slice(&data);

        self.current_len += total;

        // İndeks güncelle
        self.index.entry(*meta.address.as_bytes()).or_default().push(offset);

        Some(offset)
    }

    /// Offsetten hesap oku
    pub fn get(&self, offset: usize) -> Option<AccountSlot> {
        if offset + ACCOUNT_HEADER_SIZE > self.current_len {
            return None;
        }

        let header_bytes = &self.buffer[offset..offset + ACCOUNT_HEADER_SIZE];
        let meta = self.deserialize_header(header_bytes)?;
        let data_len = meta.data_len as usize;

        let data_start = offset + ACCOUNT_HEADER_SIZE;
        if data_start + data_len > self.current_len {
            return None;
        }

        let data = self.buffer[data_start..data_start + data_len].to_vec();

        Some(AccountSlot::new(meta, data, offset))
    }

    /// Adrese göre son hesabı oku (en son yazılan versiyon)
    pub fn get_account(&self, address: &Address) -> Option<AccountSlot> {
        let offsets = self.index.get(address.as_bytes())?;
        let &last_offset = offsets.last()?;
        self.get(last_offset)
    }

    /// Adrese göre tüm versiyonları oku (eski → yeni)
    pub fn get_all_versions(&self, address: &Address) -> Vec<AccountSlot> {
        match self.index.get(address.as_bytes()) {
            None => vec![],
            Some(offsets) => offsets.iter().filter_map(|&o| self.get(o)).collect(),
        }
    }

    /// Tüm canlı hesapları iterate et (son versiyon)
    pub fn for_each_account<F>(&self, mut f: F)
    where
        F: FnMut(&AccountSlot),
    {
        for offsets in self.index.values() {
            if let Some(&last) = offsets.last() {
                if let Some(slot) = self.get(last) {
                    if !slot.meta.is_dead() {
                        f(&slot);
                    }
                }
            }
        }
    }

    /// Canlı hesap sayısı
    pub fn live_account_count(&self) -> usize {
        self.index.values()
            .filter(|offsets| {
                offsets.last()
                    .and_then(|&o| self.get(o))
                    .map(|s| !s.meta.is_dead())
                    .unwrap_or(false)
            })
            .count()
    }

    /// Kullanılan bayt miktarı
    pub fn len(&self) -> usize {
        self.current_len
    }

    pub fn is_empty(&self) -> bool {
        self.current_len == 0
    }

    /// Kapasite doluluk oranı
    pub fn fill_ratio(&self) -> f64 {
        self.current_len as f64 / self.capacity as f64
    }

    /// Kaç adres var?
    pub fn address_count(&self) -> usize {
        self.index.len()
    }

    // ─── Serializasyon yardımcıları ───────────────────────────────────────

    fn serialize_header(&self, meta: &StoredAccountMeta, data_len: u32) -> [u8; ACCOUNT_HEADER_SIZE] {
        let mut h = [0u8; ACCOUNT_HEADER_SIZE];
        // [0..20] address
        h[0..20].copy_from_slice(meta.address.as_bytes());
        // [20..28] lamports
        h[20..28].copy_from_slice(&meta.lamports.to_le_bytes());
        // [28..32] data_len
        h[28..32].copy_from_slice(&data_len.to_le_bytes());
        // [32..52] owner
        h[32..52].copy_from_slice(meta.owner.as_bytes());
        // [52] executable
        h[52] = meta.executable as u8;
        // [53..61] rent_epoch
        h[53..61].copy_from_slice(&meta.rent_epoch.to_le_bytes());
        // [61..64] reserved
        h
    }

    fn deserialize_header(&self, bytes: &[u8]) -> Option<StoredAccountMeta> {
        if bytes.len() < ACCOUNT_HEADER_SIZE {
            return None;
        }

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&bytes[0..20]);

        let lamports = u64::from_le_bytes(bytes[20..28].try_into().ok()?);
        let data_len = u32::from_le_bytes(bytes[28..32].try_into().ok()?);

        let mut owner_bytes = [0u8; 20];
        owner_bytes.copy_from_slice(&bytes[32..52]);

        let executable = bytes[52] != 0;
        let rent_epoch = u64::from_le_bytes(bytes[53..61].try_into().ok()?);

        Some(StoredAccountMeta {
            address: Address::from_bytes(addr_bytes),
            lamports,
            data_len,
            owner: Address::from_bytes(owner_bytes),
            executable,
            rent_epoch,
        })
    }
}

// ─── AppendVecStore ───────────────────────────────────────────────────────────

/// Çok sayıda AppendVec'i slot bazlı yöneten depo
///
/// Solana: `AccountsDb { storage: HashMap<Slot, AccountStorageEntry> }`
#[derive(Debug, Default)]
pub struct AppendVecStore {
    /// slot → AppendVec
    stores: BTreeMap<u64, AppendVec>,
    /// Aktif (yazılan) slot
    current_slot: u64,
}

impl AppendVecStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni slot oluştur
    pub fn new_slot(&mut self, slot: u64) {
        let name = format!("slot_{slot}");
        self.stores.insert(slot, AppendVec::with_default_capacity(name));
        self.current_slot = slot;
    }

    /// Mevcut slota hesap yaz
    pub fn store_account(
        &mut self,
        meta: StoredAccountMeta,
        data: Vec<u8>,
    ) -> Option<usize> {
        self.stores.get_mut(&self.current_slot)?.append(meta, data)
    }

    /// Herhangi bir slotta adrese göre hesap oku (en yeni slot önce)
    pub fn load_account(&self, address: &Address) -> Option<AccountSlot> {
        for (_, store) in self.stores.iter().rev() {
            if let Some(slot) = store.get_account(address) {
                return Some(slot);
            }
        }
        None
    }

    /// Toplam hesap sayısı
    pub fn total_accounts(&self) -> usize {
        self.stores.values().map(|s| s.live_account_count()).sum()
    }

    /// Toplam depolama boyutu (bayt)
    pub fn total_bytes(&self) -> usize {
        self.stores.values().map(|s| s.len()).sum()
    }

    pub fn slot_count(&self) -> usize {
        self.stores.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address::from_bytes([byte; 20])
    }

    fn zero_addr() -> Address {
        Address::from_bytes([0u8; 20])
    }

    fn make_meta(address: Address, lamports: u64) -> StoredAccountMeta {
        StoredAccountMeta {
            address,
            lamports,
            data_len: 0,
            owner: zero_addr(),
            executable: false,
            rent_epoch: 0,
        }
    }

    // 1. Temel append ve get
    #[test]
    fn append_and_get_basic() {
        let mut av = AppendVec::new("test", 1024 * 1024);
        let meta = make_meta(addr(1), 1_000_000);
        let data = vec![0xABu8; 64];

        let offset = av.append(meta.clone(), data.clone()).unwrap();
        assert_eq!(offset, 0);

        let slot = av.get(offset).unwrap();
        assert_eq!(slot.meta.address, addr(1));
        assert_eq!(slot.meta.lamports, 1_000_000);
        assert_eq!(slot.data, data);
    }

    // 2. Birden fazla hesap append
    #[test]
    fn multiple_accounts_append() {
        let mut av = AppendVec::new("test2", 10 * 1024 * 1024);

        let offsets: Vec<usize> = (1..=5u8).map(|i| {
            av.append(make_meta(addr(i), i as u64 * 1000), vec![i; 32]).unwrap()
        }).collect();

        // Ofsetler farklı
        assert_eq!(offsets[0], 0);
        assert!(offsets[1] > offsets[0]);

        // Hepsi okunabilir
        for (i, &offset) in offsets.iter().enumerate() {
            let slot = av.get(offset).unwrap();
            assert_eq!(slot.meta.lamports, (i + 1) as u64 * 1000);
        }
    }

    // 3. Adrese göre okuma
    #[test]
    fn get_by_address() {
        let mut av = AppendVec::new("test3", 1024 * 1024);
        av.append(make_meta(addr(10), 5000), vec![0u8; 16]).unwrap();

        let slot = av.get_account(&addr(10)).unwrap();
        assert_eq!(slot.meta.lamports, 5000);
    }

    // 4. Hesap güncelleme (yeni versiyon append)
    #[test]
    fn account_update_appends_new_version() {
        let mut av = AppendVec::new("test4", 1024 * 1024);

        // İlk versiyon: 1000 lamport
        av.append(make_meta(addr(20), 1000), vec![0u8; 8]).unwrap();
        // İkinci versiyon: 2000 lamport
        av.append(make_meta(addr(20), 2000), vec![0u8; 8]).unwrap();

        // Son versiyon okunmalı
        let slot = av.get_account(&addr(20)).unwrap();
        assert_eq!(slot.meta.lamports, 2000);

        // Tüm versiyonlar
        let versions = av.get_all_versions(&addr(20));
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].meta.lamports, 1000);
        assert_eq!(versions[1].meta.lamports, 2000);
    }

    // 5. Canlı hesap sayısı
    #[test]
    fn live_account_count() {
        let mut av = AppendVec::new("test5", 1024 * 1024);
        av.append(make_meta(addr(1), 1000), vec![]).unwrap();
        av.append(make_meta(addr(2), 2000), vec![]).unwrap();
        // Ölü hesap (lamports=0)
        av.append(make_meta(addr(3), 0), vec![]).unwrap();

        assert_eq!(av.live_account_count(), 2);
    }

    // 6. Kapasite dolduğunda None
    #[test]
    fn capacity_full_returns_none() {
        let mut av = AppendVec::new("tiny", 100); // sadece 100 bayt
        let meta = make_meta(addr(1), 1000);
        let data = vec![0u8; 50]; // 50 bayt veri + 64 byte header = 114 > 100

        let result = av.append(meta, data);
        assert!(result.is_none());
    }

    // 7. for_each_account ölü hesapları atlar
    #[test]
    fn for_each_skips_dead_accounts() {
        let mut av = AppendVec::new("test7", 1024 * 1024);
        av.append(make_meta(addr(1), 1000), vec![]).unwrap();
        av.append(make_meta(addr(2), 0), vec![]).unwrap(); // ölü
        av.append(make_meta(addr(3), 3000), vec![]).unwrap();

        let mut count = 0;
        av.for_each_account(|_| count += 1);
        assert_eq!(count, 2); // sadece canlılar
    }

    // 8. AppendVecStore çok slot yönetimi
    #[test]
    fn store_manages_multiple_slots() {
        let mut store = AppendVecStore::new();

        store.new_slot(100);
        store.store_account(make_meta(addr(1), 1000), vec![]).unwrap();
        store.store_account(make_meta(addr(2), 2000), vec![]).unwrap();

        store.new_slot(101);
        store.store_account(make_meta(addr(3), 3000), vec![]).unwrap();

        assert_eq!(store.slot_count(), 2);
        assert_eq!(store.total_accounts(), 3);
    }

    // 9. Store: en yeni slot tercih edilir
    #[test]
    fn store_returns_latest_slot_account() {
        let mut store = AppendVecStore::new();

        store.new_slot(100);
        store.store_account(make_meta(addr(1), 1000), vec![]).unwrap();

        store.new_slot(101);
        // Aynı hesap güncellendi
        store.store_account(make_meta(addr(1), 9999), vec![]).unwrap();

        let account = store.load_account(&addr(1)).unwrap();
        assert_eq!(account.meta.lamports, 9999); // en yeni
    }

    // 10. fill_ratio doğru
    #[test]
    fn fill_ratio_correct() {
        let mut av = AppendVec::new("ratio_test", 10000);
        assert_eq!(av.fill_ratio(), 0.0);

        av.append(make_meta(addr(1), 100), vec![0u8; 36]).unwrap();
        // 64 + 36 = 100 bayt yazıldı → 100/10000 = 0.01
        assert!((av.fill_ratio() - 0.01).abs() < 1e-6);
    }

    // 11. Boş AppendVec
    #[test]
    fn empty_append_vec() {
        let av = AppendVec::new("empty", 1024);
        assert!(av.is_empty());
        assert_eq!(av.address_count(), 0);
        assert!(av.get_account(&addr(1)).is_none());
    }

    // 12. Veri bütünlüğü (round-trip)
    #[test]
    fn data_integrity_round_trip() {
        let mut av = AppendVec::new("integrity", 1024 * 1024);
        let original_data: Vec<u8> = (0..128).map(|i| i as u8).collect();
        let meta = StoredAccountMeta {
            address: addr(42),
            lamports: 999_999,
            data_len: original_data.len() as u32,
            owner: addr(255),
            executable: true,
            rent_epoch: 12345,
        };

        av.append(meta.clone(), original_data.clone()).unwrap();
        let recovered = av.get_account(&addr(42)).unwrap();

        assert_eq!(recovered.meta.lamports, 999_999);
        assert_eq!(recovered.meta.owner, addr(255));
        assert!(recovered.meta.executable);
        assert_eq!(recovered.meta.rent_epoch, 12345);
        assert_eq!(recovered.data, original_data);
    }
}
