# Solana accounts-db/src/ — Kapsamlı Analiz (Agent 09)

**Tarih:** 2026-07-22
**Analiz edilen dizin:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/accounts-db/src/`
**Toplam dosya sayısı:** ~40 .rs dosyası + alt dizinler
**Toplam satır sayısı (ana dosyalar):** ~42.000+ satır

---

## Dosya Listesi ve Boyutları

| Dosya | Satır |
|---|---|
| accounts_db.rs | 17.765 |
| accounts_index.rs | 4.221 |
| ancient_append_vecs.rs | 3.329 |
| accounts.rs | 2.343 |
| accounts_hash.rs | 2.440 |
| cache_hash_data.rs | 621 |
| account_storage.rs | 559 |
| storable_accounts.rs | 544 |
| bucket_map_holder_stats.rs | 645 |
| bucket_map_holder.rs | 570 |
| append_vec.rs | 1.340 |
| rolling_bit_field.rs | 1.004 |
| shared_buffer_reader.rs | 913 |
| hardened_unpack.rs | 1.059 |
| accounts_cache.rs | 353 |
| accounts_partition.rs | 706 |
| sorted_storages.rs | 485 |
| read_only_accounts_cache.rs | 378 |
| secondary_index.rs | 240 |
| account_info.rs | 223 |
| accounts_file.rs | 215 |
| ancestors.rs | 248 |
| pubkey_bins.rs | 202 |
| accounts_index_storage.rs | 179 |
| blockhash_queue.rs | 274 |
| verify_accounts_hash_in_background.rs | 165 |
| active_stats.rs | 89 |
| waitable_condvar.rs | 86 |
| partitioned_rewards.rs | 112 |
| contains.rs | 49 |
| utils.rs | 214 |
| stake_rewards.rs | 101 |
| tiered_storage.rs | 430 |
| lib.rs | 52 |
| inline_spl_token.rs | 91 |
| inline_spl_token_2022.rs | 18 |
| cache_hash_data_stats.rs | 80 |

---

## 1. accounts_db.rs (17.765 satır) — Ana Storage Engine

### Dosya Başlığı ve Mimari Özet (satır 1–19)

```
//! Persistent accounts are stored at this path location:
//!  `<path>/<pid>/data/`
//!
//! The persistent store would allow for this mode of operation:
//!  - Concurrent single thread append with many concurrent readers.
//!
//! The underlying memory is memory mapped to a file.
//! The only required in memory data structure with a write lock is the index,
//! which should be fast to update.
```

Solana'nın tüm account verisi bu tek dosya aracılığıyla yönetilir. Tasarım ilkesi: **mmap + append-only + in-memory index**.

### Sabitler (satır 117–143)

| Sabit | Değer | Açıklama |
|---|---|---|
| `PAGE_SIZE` | 4096 bytes | Disk page boyutu |
| `WRITE_CACHE_LIMIT_BYTES_DEFAULT` | 15 GB | Write-back cache üst limiti |
| `SCAN_SLOT_PAR_ITER_THRESHOLD` | 4000 | Parallel scan eşiği |
| `UNREF_ACCOUNTS_BATCH_SIZE` | 10.000 | Unref batch büyüklüğü |
| `DEFAULT_FILE_SIZE` | 4 * 1024 * 1024 (4 MB) | Yeni AppendVec dosya boyutu |
| `DEFAULT_NUM_THREADS` | 8 | Thread sayısı |
| `DEFAULT_NUM_DIRS` | 4 | Storage dizin sayısı |
| `PUBKEY_BINS_FOR_CALCULATING_HASHES` | 65.536 | Hash hesaplama için bin sayısı |
| `MAX_ITEMS_PER_CHUNK` | 2.500 (Slot) | Scan chunk boyutu |
| `SHRINK_COLLECT_CHUNK_SIZE` | 50 | Shrink sırasında thread başına account sayısı |
| `ANCIENT_APPEND_VEC_DEFAULT_OFFSET` | Some(-10.000) | Ancient slot eşiği (1 epoch - 10.000) |

### Enum Tanımları

#### `CreateAncientStorage` (satır 145–152)
```rust
pub enum CreateAncientStorage {
    Append,  // varsayılan: append-only
    Pack,    // 1-shot write, daha verimli
}
```
Ancient storage oluşturma stratejisi. `Pack` modu, birden fazla hesabı tek seferde sıkıştırılmış şekilde yazar.

#### `StoreTo<'a>` (satır 154–166)
```rust
enum StoreTo<'a> {
    Cache,
    Storage(&'a Arc<AccountStorageEntry>),
}
```
Bir write işleminin cache'e mi yoksa kalıcı storage'a mı gideceğini belirtir. `is_cached()` metodu bulunur.

#### `ScanAccountStorageResult` (satır 168–173)
```rust
enum ScanAccountStorageResult {
    CacheFileAlreadyExists(CacheHashDataFileReference),
    CacheFileNeedsToBeCreated((String, Range<Slot>)),
}
```
Hash scan sırasında cache dosyasının durumunu temsil eder.

#### `LoadZeroLamports` (satır 409–421)
```rust
enum LoadZeroLamports {
    None,
    #[cfg(feature = "dev-context-only-utils")]
    SomeWithZeroLamportAccountForTests,
}
```
Sıfır lamportlu hesapların yüklenip yüklenmeyeceğini kontrol eder. Üretimde her zaman `None`; bu sayede silinmiş hesaplar görünmez.

#### `LoadHint` (satır 780–794)
```rust
pub enum LoadHint {
    FixedMaxRoot,   // fork tip — max root artmayacak
    Unspecified,    // RPC ve diğer çağrılar
}
```
Account yükleme hızlı yolu için hint. `FixedMaxRoot` olduğunda daha agresif assert'ler devreye girer.

#### `LoadedAccountAccessor<'a>` (satır 796–803)
```rust
pub enum LoadedAccountAccessor<'a> {
    Stored(Option<(Arc<AccountStorageEntry>, usize)>),
    Cached(Option<Cow<'a, CachedAccount>>),
}
```
Bir hesabın AppendVec'ten mi yoksa cache'ten mi okunduğunu soyutlar. `usize` değeri storage içindeki offset'tir.

`get_loaded_account()` ve `check_and_get_loaded_account()` metodları bu enum üzerinden çalışır.

#### `LoadedAccount<'a>` (satır 887–936)
```rust
pub enum LoadedAccount<'a> {
    Stored(StoredAccountMeta<'a>),
    Cached(Cow<'a, CachedAccount>),
}
```
`ReadableAccount` trait'ini implement eder; her iki kaynak için `lamports()`, `data()`, `owner()`, `executable()`, `rent_epoch()` metodları desteklenir.

#### `StoreReclaims` (satır 314–319)
```rust
pub enum StoreReclaims {
    Default,
    Ignore,
}
```
Store işlemi sonrası index reclaim'inin nasıl işleneceğini kontrol eder.

### Struct Tanımları

#### `AliveAccounts<'a>` (satır 175–183)
```rust
pub(crate) struct AliveAccounts<'a> {
    pub(crate) slot: Slot,
    pub(crate) accounts: Vec<&'a StoredAccountMeta<'a>>,
    pub(crate) bytes: usize,
}
```
Shrink sırasında canlı (index'te mevcut) hesapları tutar. `bytes` alanı toplam storage boyutunu takip eder.

#### `ShrinkCollectAliveSeparatedByRefs<'a>` (satır 186–194)
```rust
pub(crate) struct ShrinkCollectAliveSeparatedByRefs<'a> {
    pub(crate) one_ref: AliveAccounts<'a>,
    pub(crate) many_refs_this_is_newest_alive: AliveAccounts<'a>,
    pub(crate) many_refs_old_alive: AliveAccounts<'a>,
}
```
Shrink sırasında refcount'a göre ayrılmış canlı hesaplar. Ref count = 1 olan hesaplar doğrudan taşınabilir; birden fazla ref olan hesaplar için dikkatli unref gerekir.

#### `VerifyAccountsHashAndLamportsConfig<'a>` (satır 196–213)
```rust
pub struct VerifyAccountsHashAndLamportsConfig<'a> {
    pub ancestors: &'a Ancestors,
    pub test_hash_calculation: bool,
    pub epoch_schedule: &'a EpochSchedule,
    pub rent_collector: &'a RentCollector,
    pub ignore_mismatch: bool,
    pub store_detailed_debug_info: bool,
    pub use_bg_thread_pool: bool,
}
```

#### `CurrentAncientAppendVec` (satır 330–407)
```rust
struct CurrentAncientAppendVec {
    slot_and_append_vec: Option<(Slot, Arc<AccountStorageEntry>)>,
}
```
Ancient consolidation sırasında aktif hedef storage'ı takip eder. `create_ancient_append_vec()` ve `create_if_necessary()` metotları `ShrinkInProgress` döndürür.

`store_ancient_accounts()`: `store_accounts_frozen()` çağırır; önceki ve sonraki `remaining_bytes` farkını alarak yazılan byte'ı doğrular.

#### `AncientSlotPubkeys` (satır 429–472)
```rust
struct AncientSlotPubkeys {
    inner: Option<AncientSlotPubkeysInner>,
}
struct AncientSlotPubkeysInner {
    pubkeys: HashSet<Pubkey>,
    slot: Slot,
}
```
Ancient append vec'e taşınan slot için mevcut pubkey setini tutar; çakışan pubkey'lerin unref edilmesini sağlar.

#### `ShrinkCollect<'a, T>` (satır 475–489)
```rust
pub(crate) struct ShrinkCollect<'a, T: ShrinkCollectRefs<'a>> {
    pub(crate) slot: Slot,
    pub(crate) capacity: u64,
    pub(crate) unrefed_pubkeys: Vec<&'a Pubkey>,
    pub(crate) alive_accounts: T,
    pub(crate) alive_total_bytes: usize,
    pub(crate) total_starting_accounts: usize,
    pub(crate) all_are_zero_lamports: bool,
    pub(crate) _index_entries_being_shrunk: Vec<AccountMapEntry<AccountInfo>>,
}
```
`_index_entries_being_shrunk`: Bu field okunmuyor; sadece Arc referansını tutarak index entry'nin flush edilmesini engelliyor.

#### `AccountsDbConfig` (satır 544–561)
```rust
pub struct AccountsDbConfig {
    pub index: Option<AccountsIndexConfig>,
    pub base_working_path: Option<PathBuf>,
    pub accounts_hash_cache_path: Option<PathBuf>,
    pub shrink_paths: Option<Vec<PathBuf>>,
    pub write_cache_limit_bytes: Option<u64>,
    pub ancient_append_vec_offset: Option<i64>,
    pub test_skip_rewrites_but_include_in_bank_hash: bool,
    pub skip_initial_hash_calc: bool,
    pub exhaustively_verify_refcounts: bool,
    pub create_ancient_storage: CreateAncientStorage,
    pub test_partitioned_epoch_rewards: TestPartitionedEpochRewards,
}
```

#### `AccountShrinkThreshold` (satır 566–576)
```rust
pub enum AccountShrinkThreshold {
    TotalSpace { shrink_ratio: f64 },        // tüm storage'lar için toplam oran
    IndividualStore { shrink_ratio: f64 },   // bireysel storage için oran
}
```
Varsayılan: `TotalSpace { shrink_ratio: 0.80 }`.

#### `SplitAncientStorages` (satır 1529–1547)
```rust
struct SplitAncientStorages {
    ancient_slot_count: usize,
    ancient_slots: Vec<Slot>,
    first_non_ancient_slot: Slot,
    first_chunk_start: Slot,
    non_ancient_slot_count: usize,
    chunk_count: usize,
    normal_slot_range: Range<Slot>,
}
```
Hash hesaplama için slot aralığını parçalara böler:
1. Her ancient slot = 1 ayrı chunk
2. İlk hizalanmamış chunk
3. `MAX_ITEMS_PER_CHUNK` (2500 slot) büyüklüğünde tam chunk'lar
4. Son hizalanmamış chunk

`get_slot_range(chunk)` metodu her chunk için slot aralığını döndürür.

#### `AccountStorageEntry` (satır 1006–1031) — KRİTİK
```rust
pub struct AccountStorageEntry {
    pub(crate) id: AppendVecId,
    pub(crate) slot: Slot,
    pub accounts: AccountsFile,
    count_and_status: SeqLock<(usize, AccountStorageStatus)>,
    approx_store_count: AtomicUsize,
    alive_bytes: AtomicUsize,
}
```

**Concurrency modeli:**
- `count_and_status`: `SeqLock` kullanılır. SeqLock, okuma işlemlerinde kilit almaz; sadece sequence numarasını kontrol eder. Bu, write-heavy olmayan durumlarda çok hızlıdır.
- `approx_store_count`: `AtomicUsize`, yalnızca yaklaşık değer; shrink kararları için kullanılır.
- `alive_bytes`: `AtomicUsize`, `SeqCst` ordering ile okunup yazılır (en güçlü garantiler).

**Metodlar:**
- `set_status()` (satır 1065–1084): Eğer storage tamamen doluysa (`Full`) ve tüm account'lar silinmişse, `reset()` çağırarak storage'ı yeniden kullanılabilir yapar.
- `add_account()` (satır 1130–1135): count+1, alive_bytes artırır.
- `remove_account()` (satır 1153–1186): count-1, alive_bytes azaltır. `count == 0` olduğunda assert kontrolü yapar ("double remove" koruması).
- `get_stored_account_meta()` (satır 1126–1128): offset'e göre account okur.

#### `BankHashStats` (satır 1208–1242)
```rust
pub struct BankHashStats {
    pub num_updated_accounts: u64,
    pub num_removed_accounts: u64,
    pub num_lamports_stored: u64,
    pub total_data_len: u64,
    pub num_executable_accounts: u64,
}
```
`update()` ve `accumulate()` metodları bulunur. Serializable + AbiExample derive.

#### `RemoveUnrootedSlotsSynchronization` (satır 1262–1267)
```rust
struct RemoveUnrootedSlotsSynchronization {
    slots_under_contention: Mutex<IntSet<Slot>>,
    signal: Condvar,
}
```
Cache flush ve slot removal arasındaki senkronizasyonu sağlar.

#### **`AccountsDb`** (satır 1272–1407) — ANA STRUCT

```rust
pub struct AccountsDb {
    pub accounts_index: AccountInfoAccountsIndex,
    pub ancient_append_vec_offset: Option<i64>,
    pub skip_initial_hash_calc: bool,
    pub storage: AccountStorage,
    create_ancient_storage: CreateAncientStorage,
    pub test_skip_rewrites_but_include_in_bank_hash: bool,
    pub accounts_cache: AccountsCache,
    write_cache_limit_bytes: Option<u64>,
    sender_bg_hasher: Option<Sender<CachedAccount>>,
    read_only_accounts_cache: ReadOnlyAccountsCache,
    pub next_id: AtomicAppendVecId,
    pub shrink_candidate_slots: Mutex<ShrinkCandidates>,
    pub write_version: AtomicU64,
    pub paths: Vec<PathBuf>,
    base_working_path: PathBuf,
    base_working_temp_dir: Option<TempDir>,
    accounts_hash_cache_path: PathBuf,
    shrink_paths: Vec<PathBuf>,
    pub temp_paths: Option<Vec<TempDir>>,
    file_size: u64,
    pub thread_pool: ThreadPool,          // rayon, 8M stack
    pub thread_pool_clean: ThreadPool,    // düşük öncelik
    bank_hash_stats: Mutex<HashMap<Slot, BankHashStats>>,
    accounts_delta_hashes: Mutex<HashMap<Slot, AccountsDeltaHash>>,
    accounts_hashes: Mutex<HashMap<Slot, (AccountsHash, u64)>>,
    incremental_accounts_hashes: Mutex<HashMap<Slot, (IncrementalAccountsHash, u64)>>,
    pub stats: AccountsStats,
    clean_accounts_stats: CleanAccountsStats,
    external_purge_slots_stats: PurgeStats,
    pub shrink_stats: ShrinkStats,
    pub(crate) shrink_ancient_stats: ShrinkAncientStats,
    pub cluster_type: Option<ClusterType>,
    pub account_indexes: AccountSecondaryIndexes,
    uncleaned_pubkeys: DashMap<Slot, Vec<Pubkey>>,
    is_bank_drop_callback_enabled: AtomicBool,
    remove_unrooted_slots_synchronization: RemoveUnrootedSlotsSynchronization,
    shrink_ratio: AccountShrinkThreshold,
    dirty_stores: DashMap<Slot, Arc<AccountStorageEntry>>,
    zero_lamport_accounts_to_purge_after_full_snapshot: DashSet<(Slot, Pubkey)>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    pub(crate) active_stats: ActiveStats,
    pub verify_accounts_hash_in_bg: VerifyAccountsHashInBackground,
    pub log_dead_slots: AtomicBool,
    exhaustively_verify_refcounts: bool,
    pub partitioned_epoch_rewards_config: PartitionedEpochRewardsConfig,
    pub epoch_accounts_hash_manager: EpochAccountsHashManager,
}
```

**Concurrency katmanları:**
- `accounts_index`: `Arc<InMemAccountsIndex>` + disk bucket → RwLock per bin
- `accounts_cache`: `DashMap<Slot, SlotCache>` → shard-based lock-free
- `storage`: `AccountStorage` → `DashMap` tabanlı
- `dirty_stores`: `DashMap` → lock-free
- `shrink_candidate_slots`: `Mutex<IntSet<Slot>>`
- Hash map'ler: `Mutex<HashMap<...>>`
- `next_id`: `AtomicU32`
- `write_version`: `AtomicU64`

**Thread pool yapılandırması:**
```rust
thread_pool: rayon::ThreadPoolBuilder::new()
    .num_threads(num_threads)
    .thread_name(|i| format!("solAccounts{i:02}"))
    .stack_size(8 * 1024 * 1024)  // 8MB stack — rayon için gerekli
    .build()
    .unwrap(),
thread_pool_clean: make_min_priority_thread_pool(),  // CPU sayısı / 4 thread
```

### `AccountsDb` Kritik Metodlar

#### `new_with_config()` (satır 2402–2496)
Ana constructor. `AccountsIndex` oluşturur, tüm path'leri ayarlar, `start_background_hasher()` çağırır.

#### `start_background_hasher()` + `background_hasher()` (satır 2707–2737)
```rust
fn background_hasher(receiver: Receiver<CachedAccount>) {
    loop {
        let result = receiver.recv();
        match result {
            Ok(account) => {
                if Arc::strong_count(&account) > 1 {
                    let _ = (*account).hash();
                };
            }
            Err(err) => { break; }
        }
    }
}
```
`"solDbStoreHashr"` adlı bir background thread başlatır. Account'ların hash değerlerini önceden hesaplar. Arc strong count kontrolü: eğer sadece bu kanalda tutuluyorsa (başka referans yok), hash hesaplanmaz.

#### `next_id()` (satır 2507–2511)
```rust
fn next_id(&self) -> AppendVecId {
    let next_id = self.next_id.fetch_add(1, Ordering::AcqRel);
    assert!(next_id != AppendVecId::MAX, "We've run out of storage ids!");
    next_id
}
```
`AcqRel` ordering: hem acquire hem release semantiği sağlar.

#### `clean_accounts_older_than_root()` (satır 2522–2602)
Paralel temizlik:
```rust
let reclaim_vecs = purges
    .par_chunks(INDEX_CLEAN_BULK_COUNT)  // 4096'lık batch
    .filter_map(|pubkeys| { ... })
    .collect::<Vec<_>>();
```
`clean_rooted_entries()` her pubkey için index'teki eski slot girişlerini siler.

#### `calc_delete_dependencies()` (satır 2614–2705)
Store'ların silinip silinemeyeceğini belirler. Bir store'un silinebilmesi için:
1. O store'daki tüm pubkey'ler ya silinecek ya da silinecek tüm store'larda bulunmalı
2. Bir pubkey'nin tüm store referansları silinecekse tüm silinebilir

Döngüsel bağımlılık tespiti için `pending_stores: IntSet<Slot>` kullanılır.

#### `background_hasher()` loop'u
Crossbeam unbounded channel ile çalışır. Strong count > 1 kontrolü kritik: CachedAccount başka yerden de tutuluyorsa hash önceden hesaplanır, sadece bu kanaldaysa hesaplanmaz (zaten kullanılmayacak).

#### `process_dead_slots()` (satır 3613–3651)
Dead slot'ları temizler:
1. `clean_stored_dead_slots()` — index güncellemesi
2. `purge_dead_slots_from_storage()` — storage entry silme
3. `shrink_candidate_slots`'tan çıkarır

#### `filter_zero_lamport_clean_for_incremental_snapshots()` (satır 3533–3607)
Incremental snapshot desteği için sıfır lamportlu hesaplar silinmez eğer son full snapshot'tan sonra güncellenmişlerse. `zero_lamport_accounts_to_purge_after_full_snapshot: DashSet` ile takip edilir.

#### `shrink_ancient_slots()` (satır 4213–4226)
```rust
pub fn shrink_ancient_slots(&self, epoch_schedule: &EpochSchedule) {
    if self.ancient_append_vec_offset.is_none() { return; }
    let oldest_non_ancient_slot = self.get_oldest_non_ancient_slot(epoch_schedule);
    let sorted_slots = self.get_sorted_potential_ancient_slots(oldest_non_ancient_slot);
    if self.create_ancient_storage == CreateAncientStorage::Append {
        self.combine_ancient_slots(sorted_slots, can_randomly_shrink);
    } else {
        self.combine_ancient_slots_packed(sorted_slots, can_randomly_shrink);
    }
}
```
1 epoch'tan eski slot'ları birleştirir. `can_randomly_shrink = true` ile canlı sistemde rastgele shrink de tetiklenebilir.

#### `should_move_to_ancient_append_vec()` (satır 4295–4347)
Bir slot'un ancient'a taşınıp taşınmayacağını belirler:
- Alive ratio < 90% → kesin taşı
- `gen_range(0..10000) == 0` → 0.01% ihtimalle rastgele taşı
- Zaten ancient ve sağlıklıysa → `current_ancient` olarak işaretle, kendisi taşınmaz

#### `load_accounts_index_for_shrink()` (satır 3657–3699)
Shrink sırasında index'ten canlı/ölü ayrımı yapar:
- `is_alive`: slot_list'te bu slot varsa canlı
- Ölü pubkey'ler: `unref` edilir, `unrefed_pubkeys`'e eklenir
- Canlı pubkey'ler: index entry Arc tutulur (flush engellenir)

### Type Alias'lar (satır 766–774)
```rust
pub type AtomicAppendVecId = AtomicU32;
pub type AppendVecId = u32;
type AccountSlots = HashMap<Pubkey, HashSet<Slot>>;
type SlotOffsets = HashMap<Slot, HashSet<usize>>;
type ReclaimResult = (AccountSlots, SlotOffsets);
type PubkeysRemovedFromAccountsIndex = HashSet<Pubkey>;
type ShrinkCandidates = IntSet<Slot>;
type AccountInfoAccountsIndex = AccountsIndex<AccountInfo, AccountInfo>;
```

### Trait Tanımları

#### `ShrinkCollectRefs<'a>` (satır 215–227)
```rust
pub(crate) trait ShrinkCollectRefs<'a>: Sync + Send {
    fn with_capacity(capacity: usize, slot: Slot) -> Self;
    fn collect(&mut self, other: Self);
    fn add(&mut self, ref_count: u64, account: &'a StoredAccountMeta<'a>, slot_list: &[(Slot, AccountInfo)]);
    fn len(&self) -> usize;
    fn alive_bytes(&self) -> usize;
    fn alive_accounts(&self) -> &Vec<&'a StoredAccountMeta<'a>>;
}
```
`AliveAccounts` ve `ShrinkCollectAliveSeparatedByRefs` bu trait'i implement eder.

#### `AppendVecScan` (satır 2191–2201)
```rust
trait AppendVecScan: Send + Sync + Clone {
    fn filter(&mut self, pubkey: &Pubkey) -> bool;
    fn set_slot(&mut self, slot: Slot);
    fn found_account(&mut self, account: &LoadedAccount);
    fn scanning_complete(self) -> BinnedHashData;
    fn init_accum(&mut self, count: usize);
}
```
Hash hesaplama sırasında AppendVec tarama callback'i.

### İstatistik Struct'ları

#### `AccountsStats` (satır 1409–1433)
Tüm store işlemleri için AtomicU64 sayaçlar: `store_hash_accounts`, `store_accounts`, `store_update_index`, vb.

#### `PurgeStats` (satır 1436–1451)
Purge operasyonları için metrikler.

#### `ShrinkStats` (satır 1883–1905)
Shrink operasyonları için metrikler: `bytes_removed`, `bytes_written`, `dead_accounts`, `alive_accounts`, vb.

#### `CleanAccountsStats` (satır 1826–1838)
`purge_stats` + `latest_accounts_index_roots_stats` + clean-specific timings.

### `ScanState<'a>` (satır 2204–2271) — AppendVecScan impl

Hash hesaplama scan state'i:
- `found_account()`: pubkey'i bin'e yerleştirir, hash hesaplar (eksikse), `CalculateHashIntermediate` oluşturur
- `scanning_complete()`: bin'leri sıralar, timing'i kaydeder

---

## 2. append_vec.rs (1.340 satır) — mmap Storage

### Kritik Sabitler ve Struct'lar

```rust
pub const STORE_META_OVERHEAD: usize = 136;
// = size_of::<StoredMeta>() + size_of::<AccountMeta>() + size_of::<AccountHash>()
```
Compile-time assertion ile doğrulanır.

### `AppendVec` Struct (satır ~55–100)
```rust
pub struct AppendVec {
    path: PathBuf,
    map: MmapMut,           // memory-mapped dosya
    append_lock: Mutex<()>, // sadece append için
    current_len: AtomicUsize,
    file_size: u64,
    remove_on_drop: AtomicBool,
}
```

**unsafe bloklar:**
```rust
// MmapMut::map_mut() çağrısı
let map = unsafe { MmapMut::map_mut(&data) };
```
Gerekçe: OS mmap API'si unsafe; dosya başka thread tarafından değiştirilebilir.

**append() metodu — concurrent okuma izni:**
- Sadece tek thread append yapabilir (`append_lock: Mutex<()>`)
- Ancak okuyucular kilitsiz çalışır — `current_len` atomik okunur
- Yeni append sonrası eski okuyucular etkilenmez (append-only garanti)

### Storage Mimarisi (mmap + CoW)
- Dosya belleğe map edilir (`MmapMut`)
- Append: `current_len` atomik artırılır, eski data asla değişmez
- OS copy-on-write: aynı dosya birden fazla process tarafından okunabilir
- `APPEND_VEC_MMAPPED_FILES_OPEN: AtomicUsize` — açık mmap sayısını takip eder (global istatistik)

---

## 3. accounts_cache.rs (353 satır) — Write-Back Cache

### Struct Hiyerarşisi

#### `AccountsCache` (satır 157–165)
```rust
pub struct AccountsCache {
    cache: DashMap<Slot, SlotCache>,
    maybe_unflushed_roots: RwLock<BTreeSet<Slot>>,
    max_flushed_root: AtomicU64,
    total_size: Arc<AtomicU64>,
}
```

#### `SlotCacheInner` (satır 23–31)
```rust
pub struct SlotCacheInner {
    cache: DashMap<Pubkey, CachedAccount>,
    same_account_writes: AtomicU64,      // kaç kez aynı key üzerine yazıldı
    same_account_writes_size: AtomicU64, // bu yazmaların toplam boyutu
    unique_account_writes_size: AtomicU64,
    size: AtomicU64,                     // bu slot'un net cache boyutu
    total_size: Arc<AtomicU64>,          // paylaşılan global counter
    is_frozen: AtomicBool,
}
```

**Drop impl (satır 33–38):** Drop sırasında `total_size`'dan bu slot'un boyutunu çıkarır. RAII pattern ile memory accounting.

#### `CachedAccountInner` (satır 133–138)
```rust
pub struct CachedAccountInner {
    pub account: AccountSharedData,
    hash: SeqLock<Option<AccountHash>>,  // lazy computed hash
    pubkey: Pubkey,
}
```

**`hash()` metodu (satır 141–154):** SeqLock ile okur; None ise hesaplar ve saklar (lazy initialization, lock-free read).

### Önemli Metodlar

**`insert()` (satır 68–98):** Aynı key varsa boyut farkını atomik olarak ayarlar. `grow = new - old`, `shrink = old - new`.

**`store()` (satır 211–224):** DashMap entry API'sini kullanır: `entry(slot).or_insert(self.new_inner()).clone()`. RefMut lock süresi minimize edilir.

**`clear_roots()` (satır 246–259):** `BTreeSet::split_off()` kullanarak max_root'tan büyük slot'ları ayırır, geri kalanları döndürür. Flush edilecek root'ları belirler.

**`remove_slots_le()` (satır 267–277):** `retain()` ile `max_root <= slot` olan entry'leri çıkarır.

**`set_max_flush_root()` (satır 308–310):** `fetch_max()` — monotonically increasing, race condition yok.

---

## 4. accounts_index.rs (4.221 satır) — In-Memory Index

### Sabitler
```rust
pub const ITER_BATCH_SIZE: usize = 1000;
pub const BINS_DEFAULT: usize = 8192;
pub const BINS_FOR_TESTING: usize = 2;
pub const BINS_FOR_BENCHMARKS: usize = 8192;
```

### Trait'ler

#### `IsCached` (satır 140–142)
```rust
pub trait IsCached {
    fn is_cached(&self) -> bool;
}
```

#### `IndexValue: 'static + IsCached + ZeroLamport + DiskIndexValue` (satır 144)
Index entry değeri için süper trait.

#### `DiskIndexValue: 'static + Clone + Debug + PartialEq + Copy + Default + Sync + Send` (satır 146–148)
Disk index değerleri için trait.

### Enum'lar

#### `UpsertReclaim` (satır 83–93)
```rust
pub enum UpsertReclaim {
    PreviousSlotEntryWasCached,
    PopulateReclaims,
    IgnoreReclaims,
}
```

#### `IndexKey` (satır 164–169)
```rust
pub enum IndexKey {
    ProgramId(Pubkey),
    SplTokenMint(Pubkey),
    SplTokenOwner(Pubkey),
}
```
Secondary index key tipleri.

#### `AccountIndex` (satır 171–176)
```rust
pub enum AccountIndex {
    ProgramId,
    SplTokenMint,
    SplTokenOwner,
}
```

#### `IndexLimitMb` (satır 185–193)
```rust
pub enum IndexLimitMb {
    Unspecified,
    Limit(usize),
    InMemOnly,
}
```
In-memory index boyut limiti.

#### `ScanError` (satır 151–157)
```rust
pub enum ScanError {
    SlotRemoved { slot: Slot, bank_id: BankId },
    Aborted(String),
}
```

### Struct'lar

#### `AccountMapEntryMeta` (satır 234–242)
```rust
pub struct AccountMapEntryMeta {
    pub dirty: AtomicBool,
    pub age: AtomicAge,
}
```
Dirty flag + LRU age. `new_dirty()` ve `new_clean()` factory metodları.

#### `AccountMapEntryInner<T>` (satır 264–277) — KRİTİK
```rust
pub struct AccountMapEntryInner<T> {
    ref_count: AtomicU64,
    pub slot_list: RwLock<SlotList<T>>,
    pub meta: AccountMapEntryMeta,
}
```

**Metodlar:**
- `ref_count()` → `Acquire` ordering
- `addref()` → `fetch_add(1, Release)` + `set_dirty(true)`
- `unref()` → `fetch_sub(1, Release)`, previous == 0 ise hata (under-refcount)
- `dirty()` → `Acquire`, `set_dirty()` → `Release`
- `clear_dirty()` → `compare_exchange(true, false, AcqRel, Relaxed)`
- `age()` → `Acquire`, `set_age()` → `Release`
- `try_exchange_age()` → CAS, ignore failure

#### `PreAllocatedAccountMapEntry<T>` (satır 342–409)
```rust
pub enum PreAllocatedAccountMapEntry<T: IndexValue> {
    Entry(AccountMapEntry<T>),
    Raw((Slot, T)),
}
```
Index dışında pre-allocate edilmiş entry; kilit alınmadan hazırlanır.

`allocate()`: `is_cached` ise `ref_count = 0`, değilse `ref_count = 1`.

#### `RootsTracker` (satır 412–442)
```rust
pub struct RootsTracker {
    pub alive_roots: RollingBitField,   // 4M bit = 0.5MB
    uncleaned_roots: IntSet<Slot>,
}
```
`alive_roots`: Sliding window bit field, ~400k slot kapasiteli.

#### `AccountsIndexConfig` (satır 201–211)
```rust
pub struct AccountsIndexConfig {
    pub bins: Option<usize>,
    pub flush_threads: Option<usize>,
    pub drives: Option<Vec<PathBuf>>,
    pub index_limit_mb: IndexLimitMb,
    pub ages_to_stay_in_cache: Option<Age>,
    pub scan_results_limit_bytes: Option<usize>,
    pub started_from_validator: bool,
}
```

#### `AccountsIndexIterator<'a, T, U>` (satır 455–597)
Binned accounts index üzerinde iterator. `hold_range_in_memory()` paralel olarak çalışır (thread pool kullanır). `next()` her çağrıda `ITER_BATCH_SIZE (1000)` item döndürür.

---

## 5. accounts_hash.rs (2.440 satır) — Merkle Hash Hesaplama

### Sabitler
```rust
pub const MERKLE_FANOUT: usize = 16;
```
16-ary Merkle tree.

### Struct'lar

#### `MmapAccountHashesFile` (satır 36–41)
```rust
struct MmapAccountHashesFile {
    mmap: MmapMut,
    count: usize,
}
```
Sıralı hash'lerin tutulduğu mmap dosyası. `read(index)` ve `write(hash)` metodları.

#### `AccountHashesFile` (satır 62–163)
```rust
struct AccountHashesFile {
    writer: Option<MmapAccountHashesFile>,
    dir_for_temp_cache_files: PathBuf,
    capacity: usize,
}
```
Lazy creation: `write()` çağrısında dosya yoksa oluşturur.

**unsafe blok (satır 146):**
```rust
let map = unsafe { MmapMut::map_mut(&data) };
```
Gerekçe: mmap API'si.

**Retry mekanizması (satır 113–143):** Dosya oluşturma 5 kez denenir (bellek fragmantasyonu için).

**Hata yönetimi:** `vm.max_map_count` aşılırsa process exit (satır 152–154).

#### `CalcAccountsHashConfig<'a>` (satır 166–180)
```rust
pub struct CalcAccountsHashConfig<'a> {
    pub use_bg_thread_pool: bool,
    pub check_hash: bool,
    pub ancestors: Option<&'a Ancestors>,
    pub epoch_schedule: &'a EpochSchedule,
    pub rent_collector: &'a RentCollector,
    pub store_detailed_debug_info_on_failure: bool,
}
```

#### `HashStats` (satır 185–210)
Kapsamlı hash hesaplama istatistikleri. Mix: normal u64 field'lar + AtomicU64'ler (cross-thread erişim için).

#### `CalculateHashIntermediate` (satır 319–324) — KRİTİK
```rust
#[repr(C)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Pod, Zeroable)]
pub struct CalculateHashIntermediate {
    pub hash: AccountHash,
    pub lamports: u64,
    pub pubkey: Pubkey,
}
```
`Pod + Zeroable`: bytemuck ile doğrudan cast edilebilir (sıfır kopya). `#[repr(C)]` + padding assertion:
```rust
const _: () = assert!(
    std::mem::size_of::<CalculateHashIntermediate>()
        == std::mem::size_of::<AccountHash>() + std::mem::size_of::<u64>() + std::mem::size_of::<Pubkey>()
);
```

#### `AccountsHasher<'a>` (satır 471–476)
```rust
pub struct AccountsHasher<'a> {
    pub zero_lamport_accounts: ZeroLamportAccounts,
    pub dir_for_temp_cache_files: PathBuf,
    pub(crate) active_stats: &'a ActiveStats,
}
```

### Kritik Metodlar

#### `calculate_hash()` (satır 497–509)
`CumulativeOffsets` ile flattenlanmış Vec<Vec<Hash>> üzerinde Merkle root hesaplar.

#### `compute_merkle_root_loop<T, F>()` (satır 530–567)
Paralel Merkle computation:
```rust
let result: Vec<_> = (0..chunks)
    .into_par_iter()
    .map(|i| {
        let mut hasher = Hasher::default();
        for item in hashes.iter().take(end_index).skip(start_index) {
            hasher.hash(h.as_ref());
        }
        hasher.result()
    })
    .collect();
```
Recursive: tek sonuç kaldıysa return, yoksa tekrar çağır.

#### `calculate_three_level_chunks()` (satır 569–593)
3-level optimization: `fanout^3 = 16^3 = 4096` hash'i tek pass'te işler. 4 seviyeden fazla veri varsa aktif olur. Bellek ve zaman tasarrufu sağlar.

#### `compute_merkle_root_from_slices()` (satır 596–~750)
Genel Merkle hesaplama: Vec<Vec<T>> yapısını flatten etmeden işler. `CumulativeOffsets` ile doğru slice'a erişim.

#### `de_dup_accounts_in_parallel()` (satır 1123–1186)
Paralel deduplication: working_set (sorted Vec) ile en düşük pubkey'den başlar. Her slot group için binary search. Sıfır lamportlu hesaplar için `blake3::hash(pubkey_bytes)` kullanır (incremental snapshot).

#### `find_first_pubkey_in_bin()` (satır 940–976)
Uniform distribution varsayımıyla ilk tahmini yapar, sonra binary search.

---

## 6. blockhash_queue.rs (274 satır)

### Struct'lar

#### `HashAge` (satır 12–16)
```rust
struct HashAge {
    fee_calculator: FeeCalculator,
    hash_index: u64,
    timestamp: u64,
}
```

#### `BlockhashQueue` (satır 19–32)
```rust
pub struct BlockhashQueue {
    last_hash_index: u64,
    last_hash: Option<Hash>,
    ages: HashMap<Hash, HashAge>,
    max_age: usize,
}
```
ABI frozen struct (`frozen_abi(digest = ...)`).

### Metodlar

**`register_hash()` (satır 96–114):**
```rust
pub fn register_hash(&mut self, hash: &Hash, lamports_per_signature: u64) {
    self.last_hash_index += 1;
    if self.ages.len() >= self.max_age {
        self.ages.retain(|_, age| {
            Self::is_hash_index_valid(self.last_hash_index, self.max_age, age.hash_index)
        });
    }
    // ...
}
```
`MAX_RECENT_BLOCKHASHES` (default 150) korunur. Queue dolduğunda eski hash'ler temizlenir.

**`is_hash_valid()` (satır 61–63):** O(1) lookup.

**`is_hash_valid_for_age()` (satır 66–71):** `last_hash_index - hash_index <= max_age`.

**`get_hash_age()` (satır 73–77):** `last_hash_index - age.hash_index` = yaş.

**Off-by-one tasarım notu (satır 210–214):** Test kodu bu durumu belgeler: queue `MAX_AGE + 1` eleman tutabilir çünkü en son hash yaş 0'dır. Bu kasıtlı bir off-by-one.

---

## 7. account_info.rs (223 satır) — Kompakt Index Entry

### Tasarım: Bit Packing

```rust
#[bitfield(bits = 32)]
pub struct PackedOffsetAndFlags {
    offset_reduced: B31,      // 2^31 * 8 bytes = 16GB max AppendVec
    is_zero_lamport: bool,
}
```

#### `AccountInfo` (satır 85–91)
```rust
pub struct AccountInfo {
    store_id: AppendVecId,          // u32
    account_offset_and_flags: AccountOffsetAndFlags,  // u32
}
```
**Toplam boyut: 8 bytes** — son derece kompakt.

```rust
const CACHED_OFFSET: OffsetReduced = (1 << (OffsetReduced::BITS - 1)) - 1;
// = 2^31 - 1 = 2,147,483,647
```
Bu sentinel değer cache'deki entry'leri temsil eder. Gerçek bir offset asla bu değere ulaşamaz (her entry en az 32 byte pubkey içerir).

**`CACHE_VIRTUAL_STORAGE_ID = AppendVecId::MAX`**: Cache entry'ler için sahte store_id.

`get_reduced_offset()`: `offset / 8` (alignment 8 byte, yani 3 bit kayar).
`reduced_offset_to_offset()`: `reduced_offset * 8`.

---

## 8. accounts_cache.rs — accounts_index.rs İlişkisi

Bir hesap yazıldığında:
1. `AccountsCache` → `DashMap<Slot, SlotCache>` → `DashMap<Pubkey, CachedAccount>`
2. Index'e `AccountInfo { store_id=MAX, offset=CACHED_OFFSET }` eklenir
3. Flush sırasında AppendVec'e yazılır, index güncellenir

---

## 9. read_only_accounts_cache.rs (378 satır) — LRU Cache

### Struct'lar

#### `ReadOnlyAccountsCache` (satır 59–74)
```rust
pub(crate) struct ReadOnlyAccountsCache {
    cache: DashMap<ReadOnlyCacheKey, ReadOnlyAccountCacheEntry>,
    queue: Mutex<IndexList<ReadOnlyCacheKey>>,
    max_data_size: usize,
    data_size: AtomicUsize,
    ms_to_skip_lru_update: u32,
    stats: ReadOnlyCacheStats,
}
```
`ReadOnlyCacheKey = (Pubkey, Slot)`.

#### `ReadOnlyAccountCacheEntry` (satır 24–31)
```rust
struct ReadOnlyAccountCacheEntry {
    account: AccountSharedData,
    index: AtomicU32,       // IndexList içindeki pozisyon
    last_update_time: AtomicU32,
}
```

**unsafe blok (satır 198–199):**
```rust
fn new(account: AccountSharedData, index: Index) -> Self {
    let index = unsafe { std::mem::transmute::<Index, u32>(index) };
```
`Index` tipini `u32`'ye transmute eder. İlgili kütüphane `Index`'i u32 olarak implement ettiği bilindiğinden güvenli kabul edilir.

### LRU Algoritması
- Insert/update: `queue.insert_last(key)` — kuyruğun sonuna
- Load (read): `queue.remove() + insert_last()` — sonuna taşır (MRU)
- Eviction: `queue.get_first()` — başdan sil (LRU)
- **Optimizasyon:** `ms_to_skip_lru_update`: Son güncellemeden bu yana belirli ms geçmemişse LRU update atlanır. Bu, hot path'de Mutex alımını azaltır.

**Yük durumu:** `MAX_READ_ONLY_CACHE_DATA_SIZE = 400MB`.

---

## 10. rolling_bit_field.rs (1.004 satır) — Sliding Window Set

### `RollingBitField` (satır 12–23)
```rust
pub struct RollingBitField {
    max_width: u64,          // 2'nin kuvveti
    min: u64,
    max_exclusive: u64,
    bits: BitVec,
    count: usize,
    excess: IntSet<u64>,     // max_width'i aşan eski değerler
}
```

**get_address():** `key % max_width` (2'nin kuvveti olduğu için bit shift olarak derlenir).

**insert() algoritması (satır 83–146):**
- `key < min` → excess'e ekle
- `key >= max_exclusive` → max genişletilir, `min` değerleri taşırılarak `excess`'e atılır
- Bitfield'e ekle

**Motivasyon:** `RootsTracker.alive_roots` için kullanılır. 4M bit = 512KB ile ~400k slot'u takip eder. Normal HashSet yerine bit array kullanmak belleği ~50-100x azaltır.

---

## 11. ancient_append_vecs.rs (3.329 satır)

### Struct'lar

#### `PackedAncientStorageTuning` (satır 32–44)
```rust
struct PackedAncientStorageTuning {
    percent_of_alive_shrunk_data: u64,
    max_ancient_slots: usize,
    ideal_storage_size: NonZeroU64,
    can_randomly_shrink: bool,
}
```

#### `SlotInfo` (satır 48–59)
```rust
struct SlotInfo {
    storage: Arc<AccountStorageEntry>,
    slot: Slot,
    capacity: u64,
    alive_bytes: u64,
    should_shrink: bool,
}
```

#### `AncientSlotInfos` (satır 63–74)
```rust
struct AncientSlotInfos {
    all_infos: Vec<SlotInfo>,
    shrink_indexes: Vec<usize>,
    total_alive_bytes_shrink: u64,
    total_alive_bytes: u64,
}
```

### `add()` metodu (satır 79–121)
Alive ratio < 90% ise `should_shrink = true`. 1/10000 ihtimalle random shrink. Alive bytes tracking.

### `filter_ancient_slots()` (satır 125–131)
1. `choose_storages_to_shrink()`: bytes saved'e göre sırala, cutoff uygula
2. `filter_by_smallest_capacity()`: max slot sayısını korumak için

### `truncate_to_max_storages()` (satır 179–199)
Kümülatif bytes ve gereken ancient storage sayısına göre kesme noktası belirler.

---

## 12. cache_hash_data.rs (621 satır) — Mmap'd Hash Cache

### Struct'lar

#### `Header` (satır 25–27)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Header {
    count: usize,
}
```

#### `CacheHashDataFile` (satır 46–50)
```rust
pub(crate) struct CacheHashDataFile {
    cell_size: u64,
    mmap: MmapMut,
    capacity: u64,
}
```

**unsafe bloklar:**
- `CacheHashDataFileReference::load_map()` (satır 107): `MmapMut::map_mut()` unsafe
- `CacheHashDataFile::new_map()` (satır 189): aynı

**`get_slice()` (satır 155–161):**
```rust
fn get_slice(&self, ix: u64) -> &[EntryType] {
    let start = self.get_element_offset_byte(ix);
    let bytes = &self.mmap[start..];
    bytemuck::cast_slice(bytes)  // sıfır kopya cast
}
```

**`map()` (satır 54–104):** Alignment doğrular:
```rust
unsafe {
    assert_eq!(mmap.align_to::<EntryType>().0.len(), 0, "mmap is not aligned");
}
```

#### `CacheHashData` (satır 193–198)
```rust
pub(crate) struct CacheHashData {
    cache_dir: PathBuf,
    pre_existing_cache_files: Arc<Mutex<HashSet<PathBuf>>>,
    deletion_policy: DeletionPolicy,
    pub stats: Arc<CacheHashDataStats>,
}
```
Hash hesaplama sonuçlarını mmap'd dosyalarda saklar. Bir sonraki hesaplamada aynı slot aralığı için yeniden kullanılır.

---

## 13. accounts_index.rs — `AccountsIndex` Ana Struct

### `AccountsIndex<T, U>` (satır 598–700 civarı)
```rust
pub struct AccountsIndex<T: IndexValue, U: DiskIndexValue + From<T> + Into<T>> {
    pub account_maps: LockMapTypeSlice<T, U>,
    pub bin_calculator: PubkeyBinCalculator24,
    storage: AccountsIndexStorage<T, U>,
    pub roots_tracker: RwLock<RootsTracker>,
    ongoing_scan_roots: RwLock<BTreeMap<Slot, u64>>,
    removed_bank_ids: Mutex<HashSet<BankId>>,
    pub stats: AccountsIndexStats,
    pub scan_results_limit_bytes: Option<usize>,
    pub(crate) exit: Arc<AtomicBool>,
}
```

`LockMapTypeSlice<T, U>` = `[AccountMap<T, U>]` = `[Arc<InMemAccountsIndex<T, U>>]`

- 8192 bin (varsayılan), her bin bağımsız kilitlenir
- Disk bucket desteği: `AccountsIndexStorage`
- `ongoing_scan_roots`: aktif scan'ların min root'unu takip eder (clean için güvenli limit)

---

## 14. accounts_partition.rs (706 satır)

### `RentPayingAccountsByPartition`
Rent ödeyecek hesapları partition'a göre organize eder. Partition: pubkey'in belirli bit'lerine göre hesaplanır. Epoch başında her partition belirli slot'larda işlenir.

---

## 15. Genel Mimari Özet

### Storage Katmanları (en hızlıdan en yavaşa)

```
1. AccountsCache (write-back)
   DashMap<Slot, DashMap<Pubkey, CachedAccount>>
   Boyut: ~15GB varsayılan

2. ReadOnlyAccountsCache (LRU)
   DashMap<(Pubkey, Slot), Entry> + IndexList (LRU queue)
   Boyut: 400MB

3. AppendVec (mmap'd dosyalar)
   memory-mapped, append-only
   Boyut: 4MB varsayılan per file

4. Ancient AppendVec (1 epoch+ eski)
   Birleştirilmiş büyük dosyalar

5. CacheHashData (mmap'd hash cache)
   CalculateHashIntermediate'lerin önbelleği
```

### Index Katmanları

```
AccountsIndex (in-memory, 8192 bin)
  └── InMemAccountsIndex per bin
       └── AccountMapEntryInner { ref_count, slot_list, meta }
  └── BucketMapHolder (disk spill)
       └── disk buckets per bin
```

### Concurrency Modeli

| Bileşen | Mekanizma | Neden |
|---|---|---|
| AppendVec append | `Mutex<()>` (tek) + atomic len | Append seri, okuma kilitsiz |
| AccountStorageEntry.count | `SeqLock` | Okuma kilitsiz, yazma nadir |
| AccountsCache | `DashMap` (sharded) | Yüksek eşzamanlılık |
| Index bin | `RwLock<InMemAccountsIndex>` | Okuma ağırlıklı |
| Hash computation | `rayon` + mmap | CPU paralel + IO efficient |
| Background hash | `crossbeam::channel` | Lock-free producer/consumer |

---

## 16. Nova Shield Chain İçin Dersler

### 1. Storage Engine Mimarisi

**Solana yaklaşımı:** Append-only mmap files + in-memory index.

**NSC'ye uyarlama:**
- Benzer AppendVec mimarisi kullanılabilir
- mmap ile disk I/O maliyetini azalt
- `SeqLock` ile okuma kilitsiz yap
- `STORE_META_OVERHEAD: 136 bytes` per account — overhead olarak hesaba kat

### 2. Bit-Packed AccountInfo

Solana 8 byte'da store_id + offset + zero_lamport flag sığdırıyor. NSC'de benzer kompakt index entry:
```rust
// Örnek: 64-bit'te tüm bilgiler
// [store_id: 32bit][offset_reduced: 31bit][zero_lamport: 1bit]
```
Bu tek cache line'a sığar → cache locality yüksek.

### 3. Write-Back Cache ile Latency Azaltma

`AccountsCache` write-back pattern:
- Her write önce cache'e gider (bellek hızı)
- Flush arka planda yapılır
- `15GB` büyük cache → burst write absorbe edilir

NSC'de state machine transition'ları benzer şekilde: önce bellek cache'e, sonra storage'a.

### 4. Hash Hesaplama: Mmap + Paralel Merkle

`CacheHashData`: Hash sonuçları mmap'li dosyalarda saklanır, bir sonraki hesaplamada yeniden kullanılır. `CalculateHashIntermediate: Pod` → sıfır kopya dosyaya yazılır/okunur.

NSC'de account state hash'i için aynı yaklaşım kullanılabilir.

### 5. Ancient Append Vec: Compaction Stratejisi

Solana 1 epoch'tan eski slot'ları birleştirir. NSC'de:
- Eski blokların account'ları kompakt storage'a taşınabilir
- Alive ratio < %80-90 → compaction tetikle
- Random shrink (%0.01) → uzun vadede birikimi önle

### 6. RollingBitField: Slot Tracking

4MB yerine 512KB ile 400k slot takibi. NSC'de validators/block tracking için aynı yapı kullanılabilir.

### 7. Background Hasher Thread

`Arc::strong_count() > 1` kontrolü ile gereksiz hash hesaplama önlenir. NSC'de lazy computation + background processing için model.

### 8. Incremental Snapshot İçin Sıfır Lamport Koruması

Zero lamport account'lar son full snapshot'tan sonra silinmez. NSC'de benzer: state silme işlemi snapshot sınırlarına göre kısıtlanmalı.

### 9. SeqLock Kullanımı

`count_and_status: SeqLock<(usize, AccountStorageStatus)>` — okuma kilitsiz, yazma çok nadir. NSC'de metadata tracking için uygun.

### 10. Thread Pool Boyutlandırma

```rust
quarter_thread_count() = max(2, num_cpus / 4)  // background clean
full thread count                               // foreground scan
```
NSC'de benzer düşük öncelik / yüksek öncelik thread pool ayrımı.

### 11. DashMap Kullanımı

Solana boyunca `DashMap` sık kullanılır (lock-free sharded HashMap). NSC'de: `slot_to_storage`, `pubkey_to_slots` gibi mapping'ler için.

### 12. Crossbeam Channel

Background hasher `crossbeam::unbounded()` channel kullanır. NSC'de producer/consumer pattern'ler için.

---

## unsafe Bloklar Özeti

| Dosya | unsafe Neden | Satır civarı |
|---|---|---|
| append_vec.rs | `MmapMut::map_mut()` | ~100 |
| accounts_hash.rs | `MmapMut::map_mut()` | 146 |
| cache_hash_data.rs | `MmapMut::map_mut()` | 107, 189 |
| cache_hash_data.rs | `mmap.align_to::<EntryType>()` | 67 |
| read_only_accounts_cache.rs | `transmute::<Index, u32>` | 198, 210, 215 |

Tüm unsafe bloklar mmap veya type-transmute için; tasarım gerekçeleri açık.

---

## Test Coverage

- `accounts_cache.rs`: `test_remove_slots_le`, `test_cached_frozen_slots`
- `blockhash_queue.rs`: `test_register_hash`, `test_reject_old_last_hash`, `test_queue_init_blockhash`, `test_get_recent_blockhashes`, `test_len`, `test_get_hash_age`, `test_is_hash_valid_for_age`
- `read_only_accounts_cache.rs`: `test_accountsdb_sizeof`, `test_read_only_accounts_cache`, `test_read_only_accounts_cache_random`
- `account_info.rs`: `test_limits`, `test_illegal_offset`, `test_alignment`
- `accounts_db.rs`: çok sayıda #[cfg(test)] bloğu (dosya sonunda)

---

*Agent 09 — Analiz tamamlandı. Toplam ~42.000 satır Rust kodu incelendi.*
