# Solana Ledger / Blockstore / Shred Analizi — Agent 17

**Analiz Tarihi:** 2026-07-22
**Kapsam:** ledger/src/ (39 dosya)
**Analist:** Agent 17

---

## 1. DIZIN YAPISI — GENEL BAKIS

```
ledger/src/
├── blockstore.rs                        — Ana ledger depolama motoru (~3000+ satir)
├── blockstore_db.rs                     — RocksDB abstraction katmani
├── blockstore_meta.rs                   — SlotMeta, ErasureMeta, Index vs.
├── blockstore_options.rs                — Konfigürasyon tipleri
├── blockstore_metrics.rs                — Metrik struct'lari
├── blockstore_metric_report_service.rs  — Metrik raporlama servisi
├── blockstore_cleanup_service.rs        — Ledger temizleme servisi
├── blockstore_processor.rs              — Blok dogrulama ve isletme
├── blockstore/
│   └── blockstore_purge.rs             — Purge/temizlik mantigi
├── shred.rs                             — Ana shred modulu
├── shred/
│   ├── common.rs                        — Ortak shred mantigi
│   ├── legacy.rs                        — Legacy (eski) shred formati
│   ├── merkle.rs                        — Merkle shred formati
│   ├── shred_code.rs                    — Coding shred yapisi
│   ├── shred_data.rs                    — Data shred yapisi
│   ├── stats.rs                         — Shred istatistikleri
│   └── traits.rs                        — Shred trait tanimlari
├── shredder.rs                          — Shred uretici + Reed-Solomon cache
├── sigverify_shreds.rs                  — CPU/GPU shred imza dogrulama
├── leader_schedule.rs                   — Leader zamanlamasi
├── leader_schedule_cache.rs             — Leader zamanlama onbellegi
├── leader_schedule_utils.rs             — Yardimci fonksiyonlar
├── ancestor_iterator.rs                 — Ata slot iteratoru
├── next_slots_iterator.rs               — Sonraki slot iteratoru
├── rooted_slot_iterator.rs              — Rootlanmis slot iteratoru
├── slot_stats.rs                        — Slot istatistikleri
├── bank_forks_utils.rs                  — BankForks yardimcilari
├── bigtable_upload.rs                   — BigTable yuklemesi
├── bigtable_upload_service.rs           — BigTable yukleme servisi
├── bigtable_delete.rs                   — BigTable silme
├── block_error.rs                       — Blok hata tipleri
├── entry_notifier_interface.rs          — Entry bildirim arayüzü
├── entry_notifier_service.rs            — Entry bildirim servisi
├── genesis_utils.rs                     — Genesis yardimcilari
├── staking_utils.rs                     — Staking yardimcilari
├── token_balances.rs                    — Token bakiye yardimcilari
├── transaction_address_lookup_table_scanner.rs — Adres tablosu tarayicisi
├── use_snapshot_archives_at_startup.rs  — Snapshot baslatma secenegi
└── lib.rs                               — Crate kök modülü
```

---

## 2. BLOCKSTORE.RS — ANA LEDGER MOTORU

### 2.1 Sabits Tanimlar (Satir 94-104)

```rust
pub const MAX_REPLAY_WAKE_UP_SIGNALS: usize = 1;
pub const MAX_COMPLETED_SLOTS_IN_CHANNEL: usize = 100_000;
// 32K shreds → ~320K peak TPS (32K shreds/slot * 4 TX/shred * 2.5 slots/sn)
pub const MAX_DATA_SHREDS_PER_SLOT: usize = 32_768;
```

### 2.2 Hata Tipleri

**`InsertDataShredError`** (Satir 112-117):
```rust
pub enum InsertDataShredError {
    Exists,
    InvalidShred,
    BlockstoreError(#[from] BlockstoreError),
}
```

**`PossibleDuplicateShred`** (Satir 125-131):
```rust
pub enum PossibleDuplicateShred {
    Exists(Shred),
    LastIndexConflict(/* original */ Shred, /* conflict */ Vec<u8>),
    ErasureConflict(/* original */ Shred, /* conflict */ Vec<u8>),
    MerkleRootConflict(/* original */ Shred, /* conflict */ Vec<u8>),
}
```
Her varyant bir `slot()` metodu ile erisilir (Satir 133-142).

**`WorkingEntry<T>`** (Satir 144-162) — dahili yardimci enum:
```rust
enum WorkingEntry<T> {
    Dirty(T),  // Blockstore kolonuna göre degistirildi
    Clean(T),  // Blockstore ile eslesir
}
```
`should_write()` metodu sadece `Dirty` varyant icin true döner.

### 2.3 Ana Struct'lar

**`InsertResults`** (Satir 164-167):
```rust
pub struct InsertResults {
    completed_data_set_infos: Vec<CompletedDataSetInfo>,
    duplicate_shreds: Vec<PossibleDuplicateShred>,
}
```

**`CompletedDataSetInfo`** (Satir 178-190):
```rust
pub struct CompletedDataSetInfo {
    pub slot: Slot,
    pub start_index: u32,  // Dahil (inclusive)
    pub end_index: u32,    // Dahil (inclusive)
}
```
Tek bir `Vec<Entry>` tasisayan shred araligini temsil eder.

**`BlockstoreSignals`** (Satir 192-196):
```rust
pub struct BlockstoreSignals {
    pub blockstore: Blockstore,
    pub ledger_signal_receiver: Receiver<bool>,
    pub completed_slots_receiver: CompletedSlotsReceiver,
}
```

**`Blockstore`** (Satir 199-232) — ANA STRUCT:
```rust
pub struct Blockstore {
    ledger_path: PathBuf,
    db: Arc<Database>,
    // Column family handles — hepsi LedgerColumn<CF> tipinde:
    meta_cf: LedgerColumn<cf::SlotMeta>,
    dead_slots_cf: LedgerColumn<cf::DeadSlots>,
    duplicate_slots_cf: LedgerColumn<cf::DuplicateSlots>,
    roots_cf: LedgerColumn<cf::Root>,
    erasure_meta_cf: LedgerColumn<cf::ErasureMeta>,
    orphans_cf: LedgerColumn<cf::Orphans>,
    index_cf: LedgerColumn<cf::Index>,
    data_shred_cf: LedgerColumn<cf::ShredData>,
    code_shred_cf: LedgerColumn<cf::ShredCode>,
    transaction_status_cf: LedgerColumn<cf::TransactionStatus>,
    address_signatures_cf: LedgerColumn<cf::AddressSignatures>,
    transaction_memos_cf: LedgerColumn<cf::TransactionMemos>,
    transaction_status_index_cf: LedgerColumn<cf::TransactionStatusIndex>,
    highest_primary_index_slot: RwLock<Option<Slot>>,
    rewards_cf: LedgerColumn<cf::Rewards>,
    blocktime_cf: LedgerColumn<cf::Blocktime>,
    perf_samples_cf: LedgerColumn<cf::PerfSamples>,
    block_height_cf: LedgerColumn<cf::BlockHeight>,
    program_costs_cf: LedgerColumn<cf::ProgramCosts>,
    bank_hash_cf: LedgerColumn<cf::BankHash>,
    optimistic_slots_cf: LedgerColumn<cf::OptimisticSlots>,
    max_root: AtomicU64,
    merkle_root_meta_cf: LedgerColumn<cf::MerkleRootMeta>,
    insert_shreds_lock: Mutex<()>,       // Shred ekleme kilit
    new_shreds_signals: Mutex<Vec<Sender<bool>>>,
    completed_slots_senders: Mutex<Vec<CompletedSlotsSender>>,
    pub shred_timing_point_sender: Option<PohTimingSender>,
    pub lowest_cleanup_slot: RwLock<Slot>,
    pub slots_stats: SlotsStats,
    rpc_api_metrics: BlockstoreRpcApiMetrics,
}
```

**`IndexMetaWorkingSetEntry`** (Satir 234-239):
```rust
pub struct IndexMetaWorkingSetEntry {
    index: Index,
    did_insert_occur: bool,  // Bu yapi olusturuldugundan beri en az 1 shred eklendi mi
}
```

**`SlotMetaWorkingSetEntry`** (Satir 242-254):
```rust
pub struct SlotMetaWorkingSetEntry {
    new_slot_meta: Rc<RefCell<SlotMeta>>,  // Bellekteki kirli kopya
    old_slot_meta: Option<SlotMeta>,        // Son persist edilen hali
    did_insert_occur: bool,
}
```

### 2.4 Anahtar Fonksiyonlar

**`Blockstore::open()`** (Satir 282-284): `do_open()` icin wrapper.

**`Blockstore::do_open()`** (Satir 290-378):
- Dizin olusturur, ulimit ayarlar
- `Database::open()` ile RocksDB acar
- Her CF icin `db.column()` ile `LedgerColumn` handle alir
- `max_root`'u Root CF'deki son kayitten okur (AtomicU64)
- `cleanup_old_entries()` ve `update_highest_primary_index_slot()` cagirir

**`open_with_signal()`** (Satir 380-397):
- `open_with_options()` cagirip sonra channel ekler
- `bounded(MAX_REPLAY_WAKE_UP_SIGNALS)` — max 1 sinyal
- `bounded(MAX_COMPLETED_SLOTS_IN_CHANNEL)` — max 100K tamamlanan slot
- `BlockstoreSignals` döner

**`meta()`** (Satir 459-461): `self.meta_cf.get(slot)` — basit getter.

**`is_full()`** (Satir 463-469): SlotMeta'dan `meta.is_full()` kontrol eder.

**`erasure_meta()`** (Satir 471-474): `(slot, u64::from(fec_set_index))` anahtariyla.

**`merkle_root_meta()`** (Satir 476-478): `erasure_set.store_key()` ile.

**`orphan()`** (Satir 486-488): Orphan CF'den okur.

**`slot_meta_iterator()`** (Satir 490-505): SlotMeta CF uzerinde forward iterator.

**`live_slots_iterator()`** (Satir 508-513): root + orphan iteratorlerini zincirir.

**`slot_data_iterator()`** (Satir 519-529): Belirli slot icin data shred iterator.

**`slot_coding_iterator()`** (Satir 531-541): Belirli slot icin coding shred iterator.

**`rooted_slot_iterator()`** (Satir 554-556): Root CF uzerinde forward iterator.

**`reversed_rooted_slot_iterator()`** (Satir 558-563): Root CF uzerinde reverse.

**`reversed_optimistic_slots_iterator()`** (Satir 565-573): Optimistic slot iteratoru.

**`slot_range_connected()`** (Satir 577-598): `starting_slot`'tan `ending_slot`'a kadar tam slotlar mevcut mu kontrol.

### 2.5 Erasure Recovery Fonksiyonlari

**`get_recovery_data_shreds()`** (Satir 600-627):
- `erasure_meta.data_shreds_indices()` uzerinde iter eder
- Once `prev_inserted_shreds` hashmap'inde arar (bellekte)
- Yoksa `index.data()` bitmask kontrol eder
- Sonra `data_cf.get_bytes()` ile diskten okur
- `Shred::new_from_serialized_shred()` ile deserialize eder

**`get_recovery_coding_shreds()`** (Satir 629-656): Ayni mantik, coding shreds icin.

**`recover_shreds()`** (Satir 658-690):
- Data + coding shredleri toplar
- `shred::recover(available_shreds, reed_solomon_cache)` cagirir
- Basarili olursa `recovered_shreds`'e ekler

**`try_shred_recovery()`** (Satir 746-791):
- Her erasure set icin `ErasureMetaStatus` kontrol eder:
  - `CanRecover` → `recover_shreds()` cagir
  - `DataFull` → sadece metrik kaydet
  - `StillNeed(n)` → yeterli shred yok, metrik kaydet

### 2.6 do_insert_shreds() — Ana Ekleme Fonksiyonu (Satir 847-1000+)

```
1. insert_shreds_lock acquire (Mutex<()>)
2. write_batch olustur
3. Her shred icin:
   - Data shred: check_insert_data_shred()
   - Code shred: check_insert_coding_shred()
4. try_shred_recovery() — erasure recovery dene
5. Kurtarilan data shredleri de check_insert_data_shred() ile ekle
6. handle_chaining() — slot zinciri guncelle
7. commit_slot_meta_working_set() — SlotMeta persist et
8. write_batch.write() — atomik yazim
```

Parametreler:
- `shreds: Vec<Shred>` — eklenecek shredler
- `is_repaired: Vec<bool>` — repair mi, turbine mi
- `leader_schedule: Option<&LeaderScheduleCache>` — imza dogrulama icin
- `is_trusted: bool` — true ise duplikat/butunluk kontrolu atla
- `retransmit_sender` — kurtarilan shredler icin retransmit kanali
- `reed_solomon_cache` — RS cache
- `metrics` — istatistikler

---

## 3. BLOCKSTORE_DB.RS — ROCKSDB ABSTRACTION KATMANI

### 3.1 Sabitler (Satir 51-108)

```rust
const MAX_WRITE_BUFFER_SIZE: u64 = 256 * 1024 * 1024;   // 256MB
const FIFO_WRITE_BUFFER_SIZE: u64 = 2 * MAX_WRITE_BUFFER_SIZE; // 512MB
const PERIODIC_COMPACTION_SECONDS: u64 = 60 * 60 * 24;  // 1 gun
```

### 3.2 Column Family Isimleri (Satir 64-108)

| Sabit | Deger | Aciklama |
|-------|-------|---------|
| `META_CF` | `"meta"` | Slot metadata |
| `DEAD_SLOTS_CF` | `"dead_slots"` | Olu slot isaretleri |
| `DUPLICATE_SLOTS_CF` | `"duplicate_slots"` | Cift slot kanitlari |
| `ERASURE_META_CF` | `"erasure_meta"` | FEC set metadata |
| `ORPHANS_CF` | `"orphans"` | Ata olmayan slotlar |
| `BANK_HASH_CF` | `"bank_hashes"` | Bank hash'leri |
| `ROOT_CF` | `"root"` | Finalize edilmis slotlar |
| `INDEX_CF` | `"index"` | Shred varlik indeksleri |
| `DATA_SHRED_CF` | `"data_shred"` | Data shred ham verileri |
| `CODE_SHRED_CF` | `"code_shred"` | Coding shred ham verileri |
| `TRANSACTION_STATUS_CF` | `"transaction_status"` | Tx durum meta |
| `ADDRESS_SIGNATURES_CF` | `"address_signatures"` | Adres imza indeksi |
| `TRANSACTION_MEMOS_CF` | `"transaction_memos"` | Tx memoları |
| `TRANSACTION_STATUS_INDEX_CF` | `"transaction_status_index"` | Aktif indeks takibi |
| `REWARDS_CF` | `"rewards"` | Slot oduller |
| `BLOCKTIME_CF` | `"blocktime"` | Blok zamanlari |
| `PERF_SAMPLES_CF` | `"perf_samples"` | Performans ornekleri |
| `BLOCK_HEIGHT_CF` | `"block_height"` | Blok yüksekligi |
| `PROGRAM_COSTS_CF` | `"program_costs"` | Program maliyet veritabani |
| `OPTIMISTIC_SLOTS_CF` | `"optimistic_slots"` | Optimistik onay |
| `MERKLE_ROOT_META_CF` | `"merkle_root_meta"` | FEC set Merkle kök meta |

**TOPLAM: 21 Column Family**

### 3.3 BlockstoreError (Satir 110-154)

Önemli hata varyantlari:
```rust
pub enum BlockstoreError {
    ShredForIndexExists,
    InvalidShredData(Box<bincode::ErrorKind>),
    RocksDb(#[from] rocksdb::Error),
    SlotNotRooted,
    DeadSlot,
    Io(#[from] std::io::Error),
    Serialize(#[from] Box<bincode::ErrorKind>),
    SlotCleanedUp,
    TransactionStatusSlotMismatch,
    EmptyEpochStakes,
    NoVoteTimestampsInRange,
    ProtobufEncodeError(#[from] prost::EncodeError),
    ProtobufDecodeError(#[from] prost::DecodeError),
    ParentEntriesUnavailable,
    SlotUnavailable,
    UnsupportedTransactionVersion,
    MissingTransactionMetadata,
    TransactionIndexOverflow,
    // ... diger hata tipleri
}
```

### 3.4 Column Tanimlari (Satir 163-363) — `pub mod columns`

Her kolon turu icin ayri struct ve dokümantasyon:

```rust
pub struct SlotMeta;    // index: u64, value: blockstore_meta::SlotMeta
pub struct Orphans;     // index: u64, value: bool
pub struct DeadSlots;   // index: u64, value: bool
pub struct DuplicateSlots; // index: u64, value: DuplicateSlotProof
pub struct ErasureMeta; // index: (Slot, fec_set_index: u64), value: ErasureMeta
pub struct BankHash;    // index: u64, value: FrozenHashVersioned
pub struct Root;        // index: u64, value: bool
pub struct Index;       // index: u64, value: blockstore_meta::Index
pub struct ShredData;   // index: (u64, u64), value: Vec<u8>
pub struct ShredCode;   // index: (u64, u64), value: Vec<u8>
pub struct TransactionStatus;  // index: (Signature, Slot), value: TransactionStatusMeta
pub struct AddressSignatures;  // index: (Pubkey, Slot, u32, Signature), value: AddressSignatureMeta
pub struct TransactionMemos;   // index: Signature, value: String
pub struct TransactionStatusIndex; // index: u64, value: TransactionStatusIndexMeta
pub struct Rewards;     // index: u64, value: generated::Rewards
pub struct Blocktime;   // index: u64, value: UnixTimestamp
pub struct PerfSamples; // index: u64, value: PerfSample
pub struct BlockHeight; // index: u64, value: u64
pub struct ProgramCosts; // index: Pubkey, value: ProgramCost
pub struct OptimisticSlots; // index: u64, value: OptimisticSlotMetaVersioned
pub struct MerkleRootMeta; // index: (Slot, fec_set_index: u32), value: MerkleRootMeta
```

Gelistirici notu (Satir 358-363): Yeni kolon eklendiginde:
1. Struct ve `Column`/`ColumnName` trait'lerini implement et
2. `Rocks::cf_descriptors()` ve `columns()` fonksiyonlarina ekle
3. `run_purge_with_stats()` ve `compact_storage()`'a hesaba kat
4. `analyze_storage()` fonksiyonuna ekle

### 3.5 OldestSlot (Satir 366-397)

```rust
struct OldestSlot {
    slot: Arc<AtomicU64>,
    clean_slot_0: Arc<AtomicBool>,
}
```
Compaction filter ile paylasilan atomik deger. `Relaxed` ordering kullanilir cünkü compaction kesin senkronizasyon gerektirmez.

### 3.6 Rocks struct (Satir 399-406)

```rust
struct Rocks {
    db: rocksdb::DB,
    access_type: AccessType,
    oldest_slot: OldestSlot,
    column_options: LedgerColumnOptions,
    write_batch_perf_status: PerfSamplingStatus,
}
```

### 3.7 Rocks::cf_descriptors() (Satir 466-543)

- 21 CF descriptor olusturur
- `new_cf_descriptor_pair_shreds::<ShredData, ShredCode>()` ile shred CF çifti
- Secondary access modunda bilinmeyen CF'ler icin descriptor olusturulmaz
- Primary modunda: `DB::list_cf()` ile mevcut CF'leri tespit et, bilinmeyenleri de aç (write_buffer=1MB, compaction=disable)

### 3.8 Compaction Stratejisi (Satir 573-618)

- `should_disable_auto_compactions()`: Primary/Maintenance => false, Secondary => true
- `should_enable_cf_compaction()`: Data/code shred CF'leri icin periodic compaction
- `PERIODIC_COMPACTION_SECONDS = 86400` (1 gun): Dosyalar bu yasi gecince compaction hedefi
- Manuel compaction yerine periodic tercih sebebi: I/O yükü zaman boyunca dagitilir

### 3.9 Temel Operasyonlar

```rust
fn get_cf(&self, cf, key) -> Result<Option<Vec<u8>>>
fn get_pinned_cf(&self, cf, key) -> Result<Option<DBPinnableSlice>>  // zero-copy
fn put_cf(&self, cf, key, value) -> Result<()>
fn multi_get_cf(&self, cf, keys) -> Vec<Result<Option<DBPinnableSlice>>>  // batch get
fn delete_cf(&self, cf, key) -> Result<()>
fn delete_file_in_range_cf(&self, cf, from_key, to_key) -> Result<()>
fn iterator_cf<C>(&self, cf, iterator_mode) -> DBIterator
fn raw_iterator_cf(&self, cf) -> DBRawIterator
fn batch(&self) -> RWriteBatch
fn write(&self, batch: RWriteBatch) -> Result<()>
```

`write()` fonksiyonu RocksDB perf örneklemesi yapar: her `rocks_perf_sample_interval` islemde bir ölçüm.

---

## 4. SHRED.RS — SOLANA NETWORK TRANSMISSION BIRIMI

### 4.1 Shred Format Diyagrami (Satir 1-51)

```
DATA SHRED:
+-------------+------------+----------------------------------------------+
| common      | data       | payload                                      |
| header      | header     |                                              |
| [sig|var|.. | [poff|flg  | [data (ledger entries)            | restr ]  |
| |slot|idx]  | |size]     |                                              |
+-------------+------------+----------------------------------------------+

CODING SHRED:
+-------------+------------+----------------------------------------------+
| common      | coding     | payload                                      |
| header      | header     |                                              |
| [sig|var|.. | [n_data|   | [erasure coded shard                       ] |
| |slot|idx]  | n_code|pos]|                                              |
+-------------+------------+----------------------------------------------+
```

Kritik not: Coding shredler TÜM data shred'i encode eder (hem header hem payload).

### 4.2 Header Sabit Boyutlari (Satir 98-107)

```rust
const SIZE_OF_COMMON_SHRED_HEADER: usize = 83;
const SIZE_OF_DATA_SHRED_HEADERS: usize = 88;
const SIZE_OF_CODING_SHRED_HEADERS: usize = 89;
const SIZE_OF_SIGNATURE: usize = SIGNATURE_BYTES;  // 64 byte
const SIZE_OF_SHRED_VARIANT: usize = 1;
const SIZE_OF_SHRED_SLOT: usize = 8;

// Byte offsets:
const OFFSET_OF_SHRED_VARIANT: usize = SIZE_OF_SIGNATURE;         // 64
const OFFSET_OF_SHRED_SLOT: usize = SIZE_OF_SIGNATURE + 1;        // 65
const OFFSET_OF_SHRED_INDEX: usize = OFFSET_OF_SHRED_SLOT + 8;    // 73
```

### 4.3 FEC (Forward Error Correction) Sabitleri (Satir 113)

```rust
pub const DATA_SHREDS_PER_FEC_BLOCK: usize = 32;
```
Her FEC blogu hedef 32 data shred icerir (ama son blok daha az olabilir).

### 4.4 ShredFlags (Satir 121-128) — bitflags

```rust
pub struct ShredFlags: u8 {
    const SHRED_TICK_REFERENCE_MASK = 0b0011_1111;  // Bits 0-5: tick sayaci
    const DATA_COMPLETE_SHRED       = 0b0100_0000;  // Bit 6: veri set tamamlandi
    const LAST_SHRED_IN_SLOT        = 0b1100_0000;  // Bit 6+7: slot'un son shred'i
}
```
`LAST_SHRED_IN_SLOT`, `DATA_COMPLETE_SHRED`'i de ima eder.

### 4.5 ShredType (Satir 170-189) — #[repr(u8)]

```rust
pub enum ShredType {
    Data = 0b1010_0101,   // 0xA5
    Code = 0b0101_1010,   // 0x5A
}
```
Tersine cevirme: Code'un binary'si Data'nin tamamlayicisi.

### 4.6 ShredVariant (Satir 191-215) — Dahili enum

```rust
enum ShredVariant {
    LegacyCode,           // 0b0101_1010  (= ShredType::Code)
    LegacyData,           // 0b1010_0101  (= ShredType::Data)
    MerkleCode {          // 0b0100_????  (ust 4 bit: 0100)
        proof_size: u8,   // alt 4 bit: Merkle kanit uzunlugu
        chained: bool,
        resigned: bool,
    },
    MerkleData {          // 0b1000_????  (ust 4 bit: 1000)
        proof_size: u8,
        chained: bool,
        resigned: bool,
    },
}
```

Varyant Kodlama:
```
0b0100_???? = MerkleCode
0b0110_???? = MerkleCode chained
0b0111_???? = MerkleCode chained resigned
0b1000_???? = MerkleData
0b1001_???? = MerkleData chained
0b1011_???? = MerkleData chained resigned
```

### 4.7 Shred Header Struct'lari

**`ShredCommonHeader`** (Satir 218-226):
```rust
struct ShredCommonHeader {
    signature: Signature,       // 64 byte Ed25519
    shred_variant: ShredVariant,// 1 byte
    slot: Slot,                 // 8 byte (u64)
    index: u32,                 // 4 byte
    version: u16,               // 2 byte
    fec_set_index: u32,         // 4 byte — FEC setinin ilk data shred indexi
}
// Toplam: 83 byte (SIZE_OF_COMMON_SHRED_HEADER ile dogrulanir)
```

**`DataShredHeader`** (Satir 229-233):
```rust
struct DataShredHeader {
    parent_offset: u16,  // slot - parent_slot (cogunlukla 1)
    flags: ShredFlags,   // 1 byte
    size: u16,           // common header + data header + data boyutu
}
// Toplam: 5 byte
```

**`CodingShredHeader`** (Satir 236-241):
```rust
struct CodingShredHeader {
    num_data_shreds: u16,    // Bu FEC set'teki data shred sayisi
    num_coding_shreds: u16,  // Bu FEC set'teki coding shred sayisi
    position: u16,           // Bu coding shred'in pozisyonu [0, num_coding_shreds)
}
// Toplam: 6 byte
```

### 4.8 Ana Shred Enum (Satir 244-248)

```rust
pub enum Shred {
    ShredCode(ShredCode),
    ShredData(ShredData),
}
```

`dispatch!` makrosu (Satir 310-340) ile her iki varyant icin method dispatch otomasyonu saglanir.

### 4.9 ShredId (Satir 265-291)

```rust
pub struct ShredId(Slot, /*shred index:*/ u32, ShredType);
```
Benzersiz shred kimliği. `seed()` metodu: `hashv(&[slot_bytes, shred_type_byte, index_bytes, leader_pubkey_bytes])` — shred dagitimi icin seed.

### 4.10 ErasureSetId (Satir 294-308)

```rust
pub(crate) struct ErasureSetId(Slot, /*fec_set_index:*/ u32);
```
ErasureMeta ve MerkleRootMeta'nin RocksDB anahtari icin `store_key()` → `(Slot, u32)`.

### 4.11 Shred Metodlari

**`new_from_data()`** (Satir 371-391): Data shred olusturur.

**`new_from_serialized_shred()`** (Satir 393-412):
- Varyant byte'a göre dogru tip sececti:
  - `LegacyCode` → `legacy::ShredCode::from_payload()`
  - `LegacyData` → `legacy::ShredData::from_payload()`
  - `MerkleCode` → `merkle::ShredCode::from_payload()`
  - `MerkleData` → `merkle::ShredData::from_payload()`

**`new_from_parity_shard()`** (Satir 414-434): Erasure recovery sonrasi coding shred olusturur.

**`verify()`** (Satir 544-550):
```rust
pub fn verify(&self, pubkey: &Pubkey) -> bool {
    match self.signed_data() {
        Ok(data) => self.signature().verify(pubkey.as_ref(), data.as_ref()),
        Err(_) => false,
    }
}
```
Legacy: payload bytes imzalanir. Merkle: Merkle kök hash imzalanir.

**`sign()`** (Satir 496-500): `keypair.sign_message(signed_data)` ve signature set eder.

### 4.12 layout modülü (Satir 577-700+)

Shred'i tam deserialize etmeden layout'tan bilgi ceker:

```rust
pub fn get_shred(packet: &Packet) -> Option<&[u8]>
pub(crate) fn get_signature(shred: &[u8]) -> Option<Signature>
pub(crate) fn get_shred_variant(shred: &[u8]) -> Result<ShredVariant, Error>
pub fn get_slot(shred: &[u8]) -> Option<Slot>
pub fn get_version(shred: &[u8]) -> Option<u16>
pub fn get_signed_data(shred: &[u8]) -> Option<SignedData>
pub fn get_signed_data_offsets(shred: &[u8]) -> Option<Range<usize>>
pub fn get_shred_id(shred: &[u8]) -> Option<ShredId>
```

`get_parent_offset()` (Satir 644-649): Sadece data shredler icin, offset 83-85 (common header bitti, data header basladi).

---

## 5. SHRED/MERKLE.RS — MERKLE SHRED FORMATI

### 5.1 Sabitler (Satir 38-51)

```rust
const SIZE_OF_MERKLE_ROOT: usize = 32;           // Hash boyutu
const SIZE_OF_MERKLE_PROOF_ENTRY: usize = 20;    // Her kanit girisinin boyutu
const ShredData::SIZE_OF_PAYLOAD: usize = 1203;  // Sabit dogrulanir
```

Ikinci on imaj saldirisi korunmasi (Satir 44-49):
```rust
const MERKLE_HASH_PREFIX_LEAF: &[u8] = b"\x00SOLANA_MERKLE_SHREDS_LEAF";
const MERKLE_HASH_PREFIX_NODE: &[u8] = b"\x01SOLANA_MERKLE_SHREDS_NODE";
```

### 5.2 MerkleData Layout (Satir 53-65)

```
{common, data} headers
| data buffer
| [Merkle root of previous erasure batch if chained]
| Merkle proof entries (proof_size * 20 bytes)
| [Retransmitter's signature if resigned]
```

### 5.3 MerkleCode Layout (Satir 67-78)

```
{common, coding} headers
| erasure coded shard
| [Merkle root of previous erasure batch if chained]
| Merkle proof entries
| [Retransmitter's signature if resigned]
```

### 5.4 ShredData struct (Merkle) (Satir 61-65)

```rust
pub struct ShredData {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    payload: Vec<u8>,
}
```

### 5.5 ShredCode struct (Merkle) (Satir 74-78)

```rust
pub struct ShredCode {
    common_header: ShredCommonHeader,
    coding_header: CodingShredHeader,
    payload: Vec<u8>,
}
```

### 5.6 capacity() Hesabi (Satir 153-163 — ShredData)

```rust
fn capacity(proof_size: u8, chained: bool, resigned: bool) -> Result<usize, Error> {
    Self::SIZE_OF_PAYLOAD
        .checked_sub(
            Self::SIZE_OF_HEADERS                          // 88 byte
            + if chained { SIZE_OF_MERKLE_ROOT } else { 0 }  // 0 veya 32 byte
            + usize::from(proof_size) * SIZE_OF_MERKLE_PROOF_ENTRY // proof_size * 20
            + if resigned { SIZE_OF_SIGNATURE } else { 0 }  // 0 veya 64 byte
        )
        .ok_or(Error::InvalidProofSize(proof_size))
}
```
Maksimum data kapasitesi = 1203 - 88 - (proof_size*20) - (chained ? 32 : 0) - (resigned ? 64 : 0).

### 5.7 Merkle Root Hesabi

`get_merkle_root(index, node, proof)`:
1. Baslangic node hash = `hashv([LEAF_PREFIX, payload_slice])`
2. Her kanit giris icin: `hashv([NODE_PREFIX, min(cur, sibling), max(cur, sibling)])`
3. Kok'a kadar yuksel
4. Imzalanan sey bu Merkle kok hash'idir

`get_merkle_root()` (ShredData, Satir 281-309):
- `fec_set_index` = byte 79-83 arasi (u32 LE)
- `index = shred_index - fec_set_index` (FEC set icindeki konum)
- `proof_offset = get_proof_offset(proof_size, chained, resigned)`
- `node = get_merkle_node(shred, SIZE_OF_SIGNATURE..proof_offset)`
- Proof ile kok hesapla

`get_merkle_root()` (ShredCode, Satir 459-489):
- `index = num_data_shreds + position`
- Byte 83-85: `num_data_shreds` (u16 LE)
- Byte 87-89: `position` (u16 LE)

---

## 6. SHRED/LEGACY.RS — ESKI SHRED FORMATI

### 6.1 Imzalanan Veri (Satir 16-28)

```rust
// TUM payload (sifir dolgu dahil) imzalanir
pub(super) const SIGNED_MESSAGE_OFFSETS: Range<usize> =
    SIZE_OF_SIGNATURE..ShredData::SIZE_OF_PAYLOAD;  // 64..1228

const ShredData::SIZE_OF_PAYLOAD: usize = 1228;  // Legacy boyut
const ShredData::CAPACITY: usize = 1051;          // Kullanilabilir data boyutu
const SIZE_OF_ERASURE_ENCODED_SLICE: usize = 1139; // Kodlanabilen kisim
```

### 6.2 Legacy ShredData struct (Satir 36-39)

```rust
pub struct ShredData {
    common_header: ShredCommonHeader,
    data_header: DataShredHeader,
    payload: Vec<u8>,
}
```

`from_payload()` (Satir 60-81): Trailing zeros ile padler, minimum SIZE_OF_HEADERS kontrol eder.

`erasure_shard()` (Satir 90-97): `payload` truncated to `SIZE_OF_ERASURE_ENCODED_SLICE` (1139).

### 6.3 Legacy ShredCode struct (Satir 45-49)

`erasure_shard()` (Satir 152-160): Ons'taki `SIZE_OF_HEADERS` byte'lari cikar (kod = header'siz payload).

`signed_data()` (Satir 178-181): `payload[SIZE_OF_SIGNATURE..]` — signature'dan sonrasi.

---

## 7. SHREDDER.RS — SHRED ÜRETICI + REED-SOLOMON

### 7.1 ERASURE_BATCH_SIZE Tablosu (Satir 33-39)

```rust
pub(crate) const ERASURE_BATCH_SIZE: [usize; 33] = [
    0, 18, 20, 22, 23, 25, 27, 28, 30, // 1-8 data shred
    32, 33, 35, 36, 38, 39, 41, 42,    // 9-16
    43, 45, 46, 48, 49, 51, 52, 53,    // 17-24
    55, 56, 58, 59, 60, 62, 63, 64,    // 25-32
];
```
Indeks = data shred sayisi, deger = toplam (data + coding) shred sayisi.
32 data shred → 64 toplam = 32 data + 32 coding (1:1 oran).
Bu, 32:32 erasure batch ile ayni kurtarma olasiligini saglar.

### 7.2 ReedSolomonCache (Satir 41-43)

```rust
pub struct ReedSolomonCache(
    Mutex<LruCache<(/*data_shards:*/ usize, /*parity_shards:*/ usize), Arc<ReedSolomon>>>
);
```

Kapasite: `4 * DATA_SHREDS_PER_FEC_BLOCK = 4 * 32 = 128` entry.

`get(data_shards, parity_shards)` (Satir 421-441):
- Once cache kontrol et
- Cache miss: `ReedSolomon::new(data, parity)` olustur
- LRU cache'e ekle
- `Arc<ReedSolomon>` döner (paylasimli sahiplik)

### 7.3 Shredder struct (Satir 46-51)

```rust
pub struct Shredder {
    slot: Slot,
    parent_slot: Slot,
    version: u16,
    reference_tick: u8,
}
```

### 7.4 entries_to_shreds() (Satir 73-119)

```
merkle_variant == true:
    → shred::make_merkle_shreds_from_entries() (paralel, rayon)
    → partition by Shred::is_data

merkle_variant == false (legacy):
    → entries_to_data_shreds()
    → data_shreds_to_coding_shreds()
```

### 7.5 entries_to_data_shreds() (Satir 121-188)

1. `bincode::serialize(entries)` — tum entries'i serialize et
2. `data_buffer_size = ShredData::capacity(None)` — max veri boyutu
3. `num_shreds = ceil(serialized_len / data_buffer_size)`
4. `get_fec_set_offsets(num_shreds, 32)` — FEC set atamalari
5. `PAR_THREAD_POOL.install(|| shreds.into_par_iter()...)` — paralel shred olustur
6. Her shred icin `ShredFlags` set et:
   - `ShredFlags::empty()` — ara shred
   - `ShredFlags::DATA_COMPLETE_SHRED` — son shred (slot bitmeden)
   - `ShredFlags::LAST_SHRED_IN_SLOT` — slot'un son shredi

### 7.6 data_shreds_to_coding_shreds() (Satir 190-263)

1. Data shredleri `fec_set_index`'e göre grupla
2. Her grup icin `get_erasure_batch_size(num_data, is_last_in_slot)` hesapla
3. `generate_coding_shreds()` cagir (tek chunk ise sequential, coksa paralel)
4. Tüm coding shredleri paralel imzala

### 7.7 generate_coding_shreds() (Satir 265-328)

```rust
pub fn generate_coding_shreds<T: Borrow<Shred>>(
    data: &[T],
    next_code_index: u32,
    reed_solomon_cache: &ReedSolomonCache,
) -> Vec<Shred>
```

1. `num_data` ve `num_coding` hesapla
2. Her data shred'in `erasure_shard_as_slice()` al
3. `parity = vec![0u8; data[0].len(); num_coding]`
4. `reed_solomon.encode_sep(&data, &mut parity)` — GF(2^8) Galois field
5. Her parity shred icin `Shred::new_from_parity_shard()` olustur

### 7.8 try_recovery() (Satir 330-391)

1. `(num_data_shreds, num_coding_shreds)` coding shred'den oku
2. Her shred'i `shards[erasure_shard_index]` konumuna koy
3. Data shredler icin `mask[index] = true` (mevcut isareti)
4. `reed_solomon.reconstruct_data(&mut shards)` — eksik veri doldur
5. `mask` = false olan (eksikti, kurtarilan) shredleri filtrele
6. `Shred::new_from_serialized_shred()` ile deserialize et

### 7.9 deshred() (Satir 393-415)

```rust
pub fn deshred(shreds: &[Shred]) -> Result<Vec<u8>, Error>
```
- Shred index uyum kontrolu (her shred ardisik olmali)
- `data_complete || last_in_slot` olmali
- Her `shred.data()` slice'ini birlestir

---

## 8. BLOCKSTORE_META.RS — META VERI YAPILARI

### 8.1 ConnectedFlags (Satir 15-43) — bitflags

```rust
pub struct ConnectedFlags: u8 {
    const CONNECTED        = 0b0000_0001;  // Slot finalize edilmis zincire bagli
    const PARENT_CONNECTED = 0b1000_0000;  // Üst slot bagli (su an kullanilmiyor)
}
```

### 8.2 SlotMeta (Satir 52-84) — ANA META YAPILANMASI

```rust
pub struct SlotMeta {
    pub slot: Slot,                              // Slot numarasi
    pub consumed: u64,                           // Ardisik 0'dan baslayan shred sayisi
    pub received: u64,                           // En yüksek shred indexi + 1
    pub first_shred_timestamp: u64,              // Ilk shred'in Unix ms timestamp
    pub last_index: Option<u64>,                 // LAST_SHRED_IN_SLOT flag'li index
    pub parent_slot: Option<Slot>,               // Üst slot (None = orphan)
    pub next_slots: Vec<Slot>,                   // Alt slotlar (fork yapisi)
    pub connected_flags: ConnectedFlags,         // Baglanti durumu
    pub completed_data_indexes: BTreeSet<u32>,   // DATA_COMPLETE_SHRED flagli indexler
}
```

Önemli metodlar:
- `is_full()`: `consumed == last_index + 1`
- `is_orphan()`: `parent_slot.is_none()`
- `is_connected()`: `CONNECTED` flag set mi
- `set_parent_connected()`: parent baglandi; eger slot da doluysa CONNECTED set eder
- `clear_unconfirmed_slot()`: Slot'u temizler ama `next_slots` korur (repair icin)

### 8.3 Index (Satir 107-119) — Shred Varlik Indeksi

```rust
pub struct Index {
    pub slot: Slot,
    data: ShredIndex,
    coding: ShredIndex,
}

pub struct ShredIndex {
    index: BTreeSet<u64>,   // Hangi index'lerde shred var
}
```

### 8.4 ErasureMeta (Satir 121-133) — FEC Set Bilgisi

```rust
pub struct ErasureMeta {
    set_index: u64,             // FEC set'in ilk data shred index'i
    first_coding_index: u64,    // Ilk coding shred index'i
    __unused_size: usize,       // Geriye uyumluluk, kullanilmiyor
    config: ErasureConfig {
        num_data: usize,
        num_coding: usize,
    },
}
```

`status()` metodu (Satir 395-413):
```rust
pub(crate) fn status(&self, index: &Index) -> ErasureMetaStatus {
    // Mevcut data ve coding shred sayilari
    let num_coding = index.coding().range(self.coding_shreds_indices()).count();
    let num_data = index.data().range(self.data_shreds_indices()).count();
    let data_missing = self.config.num_data - num_data;
    let num_needed = data_missing - num_coding; // kurtarma icin gereken kodlar

    if data_missing == 0 { DataFull }
    else if num_needed == 0 { CanRecover }
    else { StillNeed(num_needed) }
}
```

### 8.5 MerkleRootMeta (Satir 141-149)

```rust
pub struct MerkleRootMeta {
    merkle_root: Option<Hash>,           // None = legacy shred
    first_received_shred_index: u32,
    first_received_shred_type: ShredType,
}
```

### 8.6 Diger Meta Yapilar

**`DuplicateSlotProof`**: `shred1: Vec<u8>` ve `shred2: Vec<u8>` — cakisma kaniti.

**`ErasureMetaStatus`**: `CanRecover | DataFull | StillNeed(usize)`.

**`FrozenHashVersioned`**: `Current(FrozenHashStatus)` — versiyonlu bank hash.

**`FrozenHashStatus`**: `frozen_hash: Hash, is_duplicate_confirmed: bool`.

**`TransactionStatusIndexMeta`**: `max_slot: Slot, frozen: bool`.

**`AddressSignatureMeta`**: `writeable: bool`.

**`PerfSample`**: `V1(PerfSampleV1) | V2(PerfSampleV2)`.

---

## 9. BLOCKSTORE_OPTIONS.RS — KONFIGÜRASYON

### 9.1 BlockstoreOptions (Satir 6-15)

```rust
pub struct BlockstoreOptions {
    pub access_type: AccessType,               // Default: Primary
    pub recovery_mode: Option<BlockstoreRecoveryMode>,
    pub enforce_ulimit_nofile: bool,           // Default: true
    pub column_options: LedgerColumnOptions,
}
```

### 9.2 AccessType (Satir 31-41)

```rust
pub enum AccessType {
    Primary,                 // Tek islem, okuma/yazma
    PrimaryForMaintenance,   // Otomatik compaction devre disi
    Secondary,               // Coklu islem, sadece okuma
}
```

### 9.3 BlockstoreRecoveryMode (Satir 43-49)

```rust
pub enum BlockstoreRecoveryMode {
    TolerateCorruptedTailRecords,
    AbsoluteConsistency,
    PointInTime,
    SkipAnyCorruptedRecord,
}
```

### 9.4 LedgerColumnOptions (Satir 83-96)

```rust
pub struct LedgerColumnOptions {
    pub shred_storage_type: ShredStorageType,          // Default: RocksLevel
    pub compression_type: BlockstoreCompressionType,   // Default: None
    pub rocks_perf_sample_interval: usize,             // Default: 0 (devre disi)
}
```

### 9.5 ShredStorageType (Satir 126-134)

```rust
pub enum ShredStorageType {
    RocksLevel,                             // RocksDB varsayilan compaction (leveled)
    RocksFifo(BlockstoreRocksFifoOptions),  // FIFO compaction (deneysel)
}
```

Dizinler:
- RocksLevel: `"rocksdb"`
- RocksFifo: `"rocksdb_fifo"`

### 9.6 BlockstoreRocksFifoOptions (Satir 193-213)

```rust
pub struct BlockstoreRocksFifoOptions {
    pub shred_data_cf_size: u64,    // Data CF maks boyutu (default: u64::MAX)
    pub shred_code_cf_size: u64,    // Code CF maks boyutu (default: u64::MAX)
}
```

Boyut asiminda en eski SST dosyalari silinir. Belirtilen boyutu `max_shred_storage_size / 2` olarak bölüstürür.

Varsayilan test degerleri: 150GB + 150GB = 300GB.

### 9.7 BlockstoreCompressionType (Satir 245-251)

```rust
pub enum BlockstoreCompressionType { None, Snappy, Lz4, Zlib }
```

---

## 10. BLOCKSTORE_PROCESSOR.RS — BLOK ISLETME

### 10.1 Önemli Struct'lar

**`TransactionBatchWithIndexes`** (Satir 82-85):
```rust
pub struct TransactionBatchWithIndexes<'a, 'b> {
    pub batch: TransactionBatch<'a, 'b>,
    pub transaction_indexes: Vec<usize>,
}
```

**`ProcessOptions`** (Satir 693-717): Blockstore isletme seçenekleri.
```rust
pub struct ProcessOptions {
    pub run_verification: bool,
    pub full_leader_cache: bool,
    pub halt_at_slot: Option<Slot>,
    pub slot_callback: Option<ProcessSlotCallback>,
    pub new_hard_forks: Option<Vec<Slot>>,
    pub debug_keys: Option<Arc<HashSet<Pubkey>>>,
    pub account_indexes: AccountSecondaryIndexes,
    pub limit_load_slot_count_from_snapshot: Option<usize>,
    pub allow_dead_slots: bool,
    pub accounts_db_test_hash_calculation: bool,
    pub accounts_db_skip_shrink: bool,
    pub accounts_db_force_initial_clean: bool,
    pub accounts_db_config: Option<AccountsDbConfig>,
    pub verify_index: bool,
    pub shrink_ratio: AccountShrinkThreshold,
    pub runtime_config: RuntimeConfig,
    pub on_halt_store_hash_raw_data_for_debug: bool,
    pub run_final_accounts_hash_calc: bool,
    pub use_snapshot_archives_at_startup: UseSnapshotArchivesAtStartup,
}
```

**`BlockstoreProcessorError`** (Satir 665-687):
```rust
pub enum BlockstoreProcessorError {
    FailedToLoadEntries(BlockstoreError),
    FailedToLoadMeta,
    InvalidBlock(BlockError),
    InvalidTransaction(TransactionError),
    NoValidForksFound,
    InvalidHardFork(Slot),
    RootBankWithMismatchedCapitalization(Slot),
}
```

### 10.2 execute_batch() (Satir 140-217)

Tek bir transaction batch'ini çalistirir:
1. Token balance kayit kontrolu
2. `bank.load_execute_and_commit_transactions()` — ana isletme
3. `bank_utils::find_and_send_votes()` — oylar
4. Transaction status yayinla (async)
5. Prioritization fee cache guncelle
6. Ilk hatayi döndür

### 10.3 process_entries() (Satir 553-663)

PoH entry listesini sirali isler:
```
for each entry:
    if Tick:
        tick_hashes.push(hash)
        if block_boundary:
            process_batches(pending_batches)
            bank.register_tick(hash)
    if Transactions:
        loop:
            batch = bank.prepare_sanitized_batch(txs)
            if lock_ok:
                batches.push(batch)
                break
            else if batches empty:
                panic "entry conflicts with itself"
            else:
                process_batches(batches)  // Bekleyenleri temizle
                batches.clear()
process_batches(remaining)
register remaining ticks
```

### 10.4 rebatch_and_execute_batches() (Satir 400-495)

Cost model tabanli yeniden gruplama:
1. Her tx'in `CostModel::calculate_cost()` hesapla
2. `target_batch_cost = total_cost / thread_count`
3. Tx'leri maliyet sinirina gore yeniden grupla
4. `execute_batches_internal()` ile paralel calistir (rayon)

### 10.5 process_blockstore_from_root() — Yapisallik

`process_entries()` → `process_batches()` →
- Scheduler varsa: `schedule_batches_for_execution()`
- Yoksa: `rebatch_and_execute_batches()` → `execute_batches_internal()` → `execute_batch()`

---

## 11. LEADER_SCHEDULE.RS

### 11.1 LeaderSchedule struct (Satir 17-21)

```rust
pub struct LeaderSchedule {
    slot_leaders: Vec<Pubkey>,                    // Slot -> leader pubkey
    index: HashMap<Pubkey, Arc<Vec<usize>>>,     // Leader -> slot indexleri (ters)
}
```

### 11.2 new() (Satir 25-39)

- `WeightedIndex::new(stakes)` ile agirlikli rastgele
- `ChaChaRng::from_seed(seed)` ile deterministik rastgelelik
- `repeat` parametresi: her lider kac slot arka arkaya uretir

### 11.3 get_indices() (Satir 66-89)

Belirli pubkey'in hangi slot indexlerinde lider oldugunu doner:
- Binary search ile `offset % num_slots` konumunu bul
- Sonsuz yinelenme: `slot_index % num_slots` ile döngü

### 11.4 Index<u64> (Satir 92-98)

```rust
impl Index<u64> for LeaderSchedule {
    fn index(&self, index: u64) -> &Pubkey {
        &self.slot_leaders[index as usize % self.slot_leaders.len()]
    }
}
```
Schedule sonsuz olarak tekrar eder.

### 11.5 FixedSchedule (Satir 11-13)

```rust
pub struct FixedSchedule {
    pub leader_schedule: Arc<LeaderSchedule>,
}
```
Test amaciyla sabit lider zamanlamasi.

---

## 12. LEADER_SCHEDULE_CACHE.RS

### 12.1 LeaderScheduleCache struct (Satir 31-39)

```rust
pub struct LeaderScheduleCache {
    pub cached_schedules: RwLock<CachedSchedules>,  // HashMap<Epoch, Arc<LeaderSchedule>>
    epoch_schedule: EpochSchedule,
    max_epoch: RwLock<Epoch>,                        // Bilinen en yüksek epoch
    max_schedules: CacheCapacity,                    // Default: 10
    fixed_schedule: Option<Arc<FixedSchedule>>,      // Test için sabit zamanlama
}
```

### 12.2 set_root() (Satir 79-95)

- Yeni max_epoch'u belirle
- `compute_epoch_schedule(new_max_epoch, root_bank)` cagir
- Epoch schedule'i bank'ten hesaplayarak cache'e ekle

### 12.3 slot_leader_at() (Satir 97-105)

- `bank` varsa: `slot_leader_at_else_compute()`
- Yoksa: `slot_leader_at_no_compute()` — sadece cache kontrol

### 12.4 next_leader_slot() (Satir 109-161)

Verilen node'un bir sonraki lider olacagi slot araligini bul:
1. `epoch..=max_epoch` boyunca her epoch'ta get_indices()
2. `skip_while` ile zaten gönderilen slotlari atla
3. Ardisik slot araligini bul

---

## 13. ANCESTOR_ITERATOR.RS

### 13.1 AncestorIterator (Satir 6-51)

```rust
pub struct AncestorIterator<'a> {
    current: Option<Slot>,
    blockstore: &'a Blockstore,
}
```

`new(start_slot, blockstore)`: `start_slot`'un parent'indan baslayan iterator.
`new_inclusive(start_slot, blockstore)`: `start_slot`'un kendisini de dahil eder.

Iterator.next(): `blockstore.meta(slot)?.parent_slot` ile zinciri geriye dogru gider.

### 13.2 AncestorIteratorWithHash (Satir 53-73)

Her ata slot icin `(Slot, Hash)` doner:
- `blockstore.get_bank_hash(ancestor_slot)` cagirarak bank hash alir

---

## 14. BLOCKSTORE/BLOCKSTORE_PURGE.RS

### 14.1 PurgeStats (Satir 8-13)

```rust
pub struct PurgeStats {
    delete_range: u64,
    write_batch: u64,
    delete_files_in_range: u64,
}
```

### 14.2 PurgeType (Satir 15-24)

```rust
pub enum PurgeType {
    Exact,             // Yavas ama tutarli; slot-meta güncellemeleri de yapar
    CompactionFilter,  // Hizli; compaction filterina birakir
}
```

### 14.3 purge_slots() (Satir 42-65)

- `run_purge_with_stats()` cagirip sonra `datapoint_info!` ile raporlar
- Hata olursa log atar ama panic yapmaz

### 14.4 set_max_expired_slot() (Satir 73-82)

- `OldestSlot.slot = to_slot + 1` — compaction filter icin sinir
- Compaction filter bu sinirdan eski shredleri siler

### 14.5 purge_slot_cleanup_chaining() (Satir 154-191)

Tek bir slotu temizler:
1. Slot'u `purge_range()` ile temizle
2. Parent'in `next_slots`'tan bu slotu cikar
3. Boş bir `SlotMeta` (sadece `next_slots` korunmus) ile yeniden kaydet

Bu, slot'un fork'unun repair ile tekrar oynanabilir kalmasini saglar.

---

## 15. BLOCKSTORE_CLEANUP_SERVICE.RS

### 15.1 Sabitler (Satir 33-48)

```rust
pub const DEFAULT_MAX_LEDGER_SHREDS: u64 = 200_000_000; // ~400GB, ~4.4 saat yük
pub const DEFAULT_MIN_MAX_LEDGER_SHREDS: u64 = 50_000_000; // ~100GB, 3.5 gun bosta
const DEFAULT_CLEANUP_SLOT_INTERVAL: u64 = 512;  // Her 512 slot kontrolu
```

### 15.2 BlockstoreCleanupService (Satir 50-89)

Thread pool ile calisan arka plan servisi:
- `LOOP_LIMITER = 512 * DEFAULT_MS_PER_SLOT / 10` ms araligi
- Her 1 saniyede exit flag kontrol eder
- `cleanup_ledger()` her `LOOP_LIMITER` sürede cagrilir

### 15.3 find_slots_to_clean() (Satir 103-...)

Algoritma:
1. `live_files_metadata()` ile data shred CF'in SST dosyalarindan toplam entry say
2. Yüksek - dusük slot farkindan ortalama shred/slot hesapla
3. `num_shreds > max_ledger_shreds` ise temizle
4. `root - slots_to_clean * mean_shreds_per_slot` formülle temizlenecek en düsük slot bul

---

## 16. SIGVERIFY_SHREDS.RS — IMZA DOGRULAMA

### 16.1 verify_shred_cpu() (Satir 27-49)

```rust
pub fn verify_shred_cpu(packet: &Packet, slot_leaders: &HashMap<Slot, Pubkey>) -> bool {
    // 1. Discard flag kontrol
    // 2. Shred slice al
    // 3. Slot'u layout'tan oku
    // 4. Slot leader pubkey bul
    // 5. Signature'i layout'tan oku
    // 6. signed_data al (legacy: payload slice; merkle: merkle root)
    // 7. signature.verify(pubkey, data)
}
```

CPU dogrulama paralel (rayon) yapilir.

### 16.2 GPU Dogrulama

`slot_key_data_for_gpu()` (Satir 73-129):
- GPU bellegine Pubkey vektörü ve offset'ler hazirlama
- `PinnedVec` — GPU tarafindan erisilebilir pin'li bellek

`get_merkle_roots()` (Satir 131-172):
- Her Merkle shred icin hash hesapla
- Buffer'a topla, GPU'ya gönder

`shred_gpu_offsets()` (Satir 195+):
- Imza, signed data ve pubkey offset'lerini hesapla
- GPU batch dogrulama icin hazirla

---

## 17. SLOT_STATS.RS

### 17.1 ShredSource (Satir 14-19)

```rust
pub(crate) enum ShredSource {
    Turbine,    // Normal P2P dagitim
    Repaired,   // Repair protokolü
    Recovered,  // Erasure recovery
}
```

### 17.2 SlotFlags (Satir 21-28) — bitflags

```rust
struct SlotFlags: u8 {
    const DEAD   = 0b00000001;
    const FULL   = 0b00000010;
    const ROOTED = 0b00000100;
}
```

### 17.3 SlotStats (Satir 30-37)

```rust
pub struct SlotStats {
    turbine_fec_set_index_counts: HashMap<u32, usize>,  // FEC set -> turbine shred sayisi
    num_repaired: usize,
    num_recovered: usize,
    last_index: u64,
    flags: SlotFlags,
}
```

### 17.4 SlotsStats (Satir 64-165)

```rust
pub struct SlotsStats {
    pub stats: Mutex<LruCache<Slot, SlotStats>>,  // Kapasite: 300 slot
}
```

`record_shred()`: Shred kaynagina göre sayaci güncelle. Slot dolunca `datapoint_info!("shred_insert_is_full", ...)` raporla. LRU tahliyesi sirasinda da `slot_stats.report(evicted_slot)` cagrilir.

---

## 18. SHRED/TRAITS.RS

### 18.1 Shred trait (Satir 6-37)

```rust
pub(super) trait Shred<'a>: Sized {
    const SIZE_OF_PAYLOAD: usize;
    const SIZE_OF_HEADERS: usize;
    type SignedData: AsRef<[u8]>;

    fn from_payload(shred: Vec<u8>) -> Result<Self, Error>;
    fn common_header(&self) -> &ShredCommonHeader;
    fn sanitize(&self) -> Result<(), Error>;
    fn set_signature(&mut self, signature: Signature);
    fn payload(&self) -> &Vec<u8>;
    fn into_payload(self) -> Vec<u8>;
    fn erasure_shard_index(&self) -> Result<usize, Error>;
    fn erasure_shard(self) -> Result<Vec<u8>, Error>;
    fn erasure_shard_as_slice(&self) -> Result<&[u8], Error>;
    fn signed_data(&'a self) -> Result<Self::SignedData, Error>;
    fn set_index(&mut self, index: u32);
    fn set_slot(&mut self, slot: Slot);
}
```

### 18.2 ShredData trait (Satir 39-59)

`parent()` default implementasyonu:
```rust
fn parent(&self) -> Result<Slot, Error> {
    let slot = self.common_header().slot;
    let parent_offset = self.data_header().parent_offset;
    if parent_offset == 0 && slot != 0 { return Err(...) }
    slot.checked_sub(Slot::from(parent_offset)).ok_or(...)
}
```

### 18.3 ShredCode trait (Satir 61-68)

`first_coding_index()` default:
```rust
fn first_coding_index(&self) -> Option<u32> {
    let position = u32::from(self.coding_header().position);
    self.common_header().index.checked_sub(position)
}
```

---

## 19. BLOCKSTORE_METRICS.RS

### 19.1 BlockstoreInsertionMetrics (Satir 17-44)

Shred ekleme performansini olcer:
```
insert_lock_elapsed_us, insert_shreds_elapsed_us,
shred_recovery_elapsed_us, chaining_elapsed_us,
commit_working_sets_elapsed_us, write_batch_elapsed_us,
total_elapsed_us, index_meta_time_us,
num_shreds, num_inserted, num_repair, num_recovered,
num_recovered_blockstore_error, num_recovered_inserted,
num_recovered_failed_sig, num_recovered_failed_invalid,
num_recovered_exists, num_repaired_data_shreds_exists,
num_turbine_data_shreds_exists, num_data_shreds_invalid,
num_data_shreds_blockstore_error, num_coding_shreds_exists,
num_coding_shreds_invalid, num_coding_shreds_invalid_erasure_config,
num_coding_shreds_inserted
```

### 19.2 RocksDB Perf Orneklemesi

Thread-local `PerfContext` kullanilir:
- `maybe_enable_rocksdb_perf(interval, &status)` — her `interval` islemde bir aktif et
- `report_rocksdb_read_perf()` / `report_rocksdb_write_perf()` — `datapoint_info!` ile yayinla

---

## 20. BIGTABLE UPLOAD (bigtable_upload.rs, bigtable_upload_service.rs)

BigTable entegrasyonu: Onaylanan bloklar (`VersionedConfirmedBlock`) ve transaction'lar BigTable'a asenkron yüklenir. Shred veritabani birebir kopyalanmaz; sadece finalize edilmis bloklar gönderilir.

---

## 21. VERI AKISI: SHRED YASAM DÖNGÜSÜ

```
ÜRETIM (Leader):
Entries → Shredder::entries_to_shreds()
    ↓ serialize (bincode)
    ↓ data shreds oluştur (paralel, rayon)
    ↓ FEC grupla (32 data/grup)
    ↓ ReedSolomon::encode_sep() → parity shreds
    ↓ Her shred imzala (Ed25519 veya Merkle root imzasi)
    ↓ Network'e gönder (Turbine)

ALIM (Validator):
PacketBatch → sigverify_shreds (CPU/GPU)
    ↓ Slot leader public key ile imza dogrula
    ↓ Gecerli shredler
    ↓ WindowService
    ↓ Blockstore::insert_shreds()
    ↓ check_insert_data_shred() / check_insert_coding_shred()
    ↓ ErasureMeta guncelle
    ↓ Erasure recovery dene (try_shred_recovery)
    ↓ SlotMeta guncelle (consumed, received, last_index)
    ↓ Slot tamamlandiysa completed_slots_sender'a bildir
    ↓ ReplayStage uyandirilan (new_shreds_signal)

REPLAY:
ReplayStage ← ledger_signal_receiver
    ↓ Blockstore::get_slot_entries()
    ↓ Shredler birlestirilir (deshred)
    ↓ Entries deserialize edilir
    ↓ process_entries() / Bank::load_execute_and_commit_transactions()
    ↓ Bank frozen, Bank hash hesapla
    ↓ Finalize → Root CF'e ekle
    ↓ max_root guncelle
```

---

## 22. UNSAFE BLOKLAR

`blockstore.rs` ve `blockstore_db.rs`'de direkt `unsafe` blok bulunmamaktadir. Ancak:
- `sigverify_shreds.rs`'de GPU bellek islemleri (`PinnedVec`, `perf_libs`) güvensiz operasyonlar icerebilir (FFI)
- RocksDB FFI interfaceleri (C++ bindings) dolayisiyla unsafe

---

## 23. TEST YAPILARI

### blockstore.rs test helper'lari:
- `get_tmp_ledger_path_auto_delete!()` — geçici dizin
- `make_slot_entries(slot, parent, num_entries, merkle_variant)` — test shred olusturma
- `entries_to_test_shreds()` — entries → shreds

### ancestor_iterator.rs testleri:
- `test_ancestor_iterator()` — fork yapisi kurulup ata listesi dogrulanir
- `test_ancestor_iterator_inclusive()` — start_slot dahil test
- `test_ancestor_iterator_with_hash()` — (Slot, Hash) cift dogrulamasi

### leader_schedule.rs testleri:
- `test_leader_schedule_index()` — index operatorü dogrulama
- `test_leader_schedule_basic()` — deterministik üretim dogrulama
- `test_repeated_leader_schedule()` — repeat parametresi dogrulama
- `test_repeated_leader_schedule_specific()` — özel slot listesi kontrolu
- `test_get_indices()` — `get_indices()` dogru aralik döndürüyor mu

---

## 24. NOVA SHIELD CHAIN ICIN DERSLER

### 24.1 Ledger Storage Mimarisi

**RocksDB Column Family Organizasyonu:**
- Her veri türü icin ayri CF kullan (Nova Shield Chain'de benzer ayirim yapilabilir)
- Shred veri CF'leri (data/code) diger meta CF'lerinden ayrilmali
- FIFO compaction shred verileri icin ideal (zaman siralamasi dogal olarak TTL saglar)
- Leveled compaction meta/transaction data icin daha iyi

**Önerilen CF Haritasi (Nova Shield Chain):**
```
block_meta_cf     → Slot/blok metadata (consumed, received, last_index benzerleri)
block_data_cf     → Ham blok verileri / shred ekvalani
erasure_meta_cf   → FEC set metadata
orphans_cf        → Ata olmayan bloklar
root_cf           → Finalize edilmis bloklar (BFT finality)
tx_status_cf      → Transaction durumu + metadata
address_sigs_cf   → Adres indeksi
rewards_cf        → Epoch odülleri
bank_hash_cf      → State hash'leri
```

### 24.2 Shred/Block Transmission Tasarimi

- **MTU boyutu:** Solana 1228 byte (legacy) / 1203 byte (Merkle). Nova Shield Chain icin 1200-1400 byte arasinda bir MTU secimi uygun.
- **FEC blok boyutu:** 32 data shred hedefi tutarlidir. Nova Shield Chain %30-50 erasure redundancy hedefleyebilir.
- **Reed-Solomon:** `reed_solomon_erasure` crate (Galois GF(2^8)) kullanilabilir. LRU cache kritik performans iyilestirmesi saglar.
- **Merkle shredler:** Her FEC set icin Merkle agaci, slot duzeyinde butünlük yerine FEC set duzeyinde butünlük saglar. Bu retransmitter saldirilarindan korur.

### 24.3 Erasure Recovery Stratejisi

Solana'nin ERASURE_BATCH_SIZE tablosu (1-32 data shred icin toplam boyutlar):
- 1 data → 18 toplam (17 coding — asiri redundant)
- 16 data → 42 toplam (26 coding — %163 redundancy)
- 32 data → 64 toplam (32 coding — %100 redundancy)

Nova Shield Chain icin öneri: 32:32 (1:1) oran; agir yük altinda 32:16 (2:1). Kurtarma eşiği: `(data_received + coding_received) >= num_data`.

### 24.4 SlotMeta / BlockMeta Tasarimi

Kritik alanlar (mutlaka desteklenmeli):
```rust
struct BlockMeta {
    slot: u64,
    consumed: u64,        // Ardisik shred sayisi
    received: u64,        // En yüksek index + 1
    last_index: Option<u64>, // LAST flag'li index
    parent_slot: Option<u64>,
    next_slots: Vec<u64>,
    is_connected: bool,
    completed_data_indexes: BTreeSet<u32>,
}
```

`is_full()` testi: `consumed == last_index + 1` basit ama güvenilir.

### 24.5 Shred Imza Dogrulamasi

- **Legacy:** Tüm payload imzalanir → daha yavaş GPU paketleme
- **Merkle:** Sadece Merkle kök imzalanir → FEC set içi butün shredler ortak imza
- **Nova Shield Chain:** Ed25519 imzali Merkle root; GPU batch dogrulama icin `sha2` + `rayon` kullan

### 24.6 Compaction ve Temizlik

- `DEFAULT_MAX_LEDGER_SHREDS = 200_000_000` (~400GB) → Nova Shield Chain kendi hedefini belirlemeli
- Periodic compaction (24 saat) manuel compaction'dan daha iyidir (I/O dagitimi)
- `OldestSlot` atomic ile compaction filter senkronizasyonu: Basit ama etkili (Relaxed ordering yeterli)

### 24.7 Paralellik Stratejisi

- **Shred üretimi:** `rayon::par_iter()` ile paralel (PAR_THREAD_POOL)
- **Shred imzalama:** Paralel (kritik: leader'in bir slot'taki tüm shredi hizla imzalaması)
- **Entry dogrulama:** `verify_transactions()` ile paralel PoH dogrulama
- **Transaction batch isletme:** `execute_batches_internal()` rayon ile paralel

### 24.8 Atom Atomik Operasyonlar ve Kilitler

`Blockstore::insert_shreds_lock: Mutex<()>` — tüm shred ekleme tek kilitli. Bu dar bogaz gibi görünse de write batch atomikligi saglar. Nova Shield Chain icin shred gruplarini bölümleyip paralel insert denenebilir.

`max_root: AtomicU64` — kilit olmadan guncellenebilir (sifir kopya senkronizasyon).

### 24.9 Performans Metrikleri

`BlockstoreInsertionMetrics` kapsamli: Lock bekleme, erasure recovery, chaining, write batch süreleri ayri ayri ölçülür. Nova Shield Chain benzer granülaritenin takibini yapmalı.

### 24.10 Recovery Mode Destegi

`BlockstoreRecoveryMode` dört seçenek sunar:
- Üretim: `TolerateCorruptedTailRecords`
- Denetim: `AbsoluteConsistency`
- Anlık: `PointInTime`
- Acil: `SkipAnyCorruptedRecord`

Nova Shield Chain startup parametresi olarak bu modları desteklemeli.

---

## 25. DOSYA BAZI ÖZET TABLOSU

| Dosya | Satir Sayisi (tahmini) | Ana Görev |
|-------|------------------------|-----------|
| blockstore.rs | ~3500 | Ana depolama API |
| blockstore_db.rs | ~1500 | RocksDB wrapper |
| blockstore_meta.rs | ~500 | Meta struct'lar |
| blockstore_options.rs | ~285 | Konfigürasyon |
| blockstore_metrics.rs | ~400 | Metrikler |
| blockstore_cleanup_service.rs | ~250 | Temizleme servisi |
| blockstore_processor.rs | ~1500 | Blok isletme |
| blockstore/blockstore_purge.rs | ~400 | Purge mantigi |
| shred.rs | ~900 | Shred API |
| shred/merkle.rs | ~800 | Merkle shred |
| shred/legacy.rs | ~300 | Legacy shred |
| shred/shred_data.rs | ~200 | Data shred |
| shred/shred_code.rs | ~200 | Code shred |
| shred/traits.rs | ~70 | Trait tanimlari |
| shred/common.rs | ~100 | Ortak mantik |
| shred/stats.rs | ~100 | Istatistikler |
| shredder.rs | ~450 | Shred üretici + RS |
| sigverify_shreds.rs | ~400 | Imza dogrulama |
| leader_schedule.rs | ~220 | Leader zamanlamasi |
| leader_schedule_cache.rs | ~350 | Zamanlama önbellek |
| leader_schedule_utils.rs | ~100 | Yardimcilar |
| ancestor_iterator.rs | ~186 | Ata iterator |
| next_slots_iterator.rs | ~80 | Sonraki slot iter |
| rooted_slot_iterator.rs | ~60 | Root slot iter |
| slot_stats.rs | ~165 | Slot istatistik |
| bank_forks_utils.rs | ~600 | BankForks yardimci |
| bigtable_upload.rs | ~500 | BigTable yükleme |
| bigtable_upload_service.rs | ~200 | Yükleme servisi |
| bigtable_delete.rs | ~100 | Silme |
| block_error.rs | ~50 | Hata tipleri |
| entry_notifier_interface.rs | ~30 | Entry arayüzü |
| entry_notifier_service.rs | ~100 | Entry servisi |
| genesis_utils.rs | ~200 | Genesis yardimci |
| staking_utils.rs | ~100 | Staking yardimci |
| token_balances.rs | ~150 | Token bakiye |
| transaction_address_lookup_table_scanner.rs | ~100 | Adres tarayici |
| use_snapshot_archives_at_startup.rs | ~80 | Snapshot baslatma |
| lib.rs | ~50 | Crate kök |

---

## 26. SONUC VE ÖZET

Solana'nın `ledger/src/` dizini, yüksek performanslı Layer 1 blockchain için gereken tüm depolama, iletim ve doğrulama bileşenlerini içerir.

**Temel bulgular:**

1. **RocksDB ile 21 Column Family** — Her veri türü için optimize edilmiş ayrı CF, FIFO compaction shred verileri için ideal.

2. **Shred formatı ikili evrim** — Legacy (1228 byte, tüm payload imzalı) ve Merkle (1203 byte, Merkle root imzalı). Merkle, FEC set düzeyinde bütünlük + retransmitter koruması sağlar.

3. **FEC: Reed-Solomon GF(2^8)** — 32:32 hedef oran, LRU cache ile RS nesneleri paylaşımlı. `ERASURE_BATCH_SIZE` tablosu farklı data shred sayıları için optimal toplam boyutu önceden hesaplar.

4. **Paralellik her yerde** — Rayon thread pool: shred üretimi, imzalama, entry doğrulama, transaction batch işletme.

5. **Lock stratejisi** — `insert_shreds_lock: Mutex<()>` — tek kilit ama write batch atomikliği garantisi. `max_root: AtomicU64` kilit gerektirmez.

6. **SlotMeta** — `consumed`, `received`, `last_index` üçlüsü ile slot tamamlanma takibi. `completed_data_indexes: BTreeSet` ile `Vec<Entry>` sınır indeksleri.

7. **GPU sigverify** — `PinnedVec` ile GPU-accessible bellek, batch Ed25519 doğrulama.

8. **Compaction** — Periodic (24 saat) > Manual; `OldestSlot` atomic değeri compaction filter ile senkronize.

Nova Shield Chain bu mimariyi RocksDB + Reed-Solomon + Merkle shred üçlüsüyle benimsemeli; MPC/threshold şifreleme ve ZK kanıtlar için ayrı CF'ler eklenmeli.
