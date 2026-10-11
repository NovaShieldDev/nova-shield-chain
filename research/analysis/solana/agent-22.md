# Solana Analiz — Agent 22
## Kapsam: Snapshot Sistemi (15 dosya)

Toplam: ~10,837 satır | 15 dosya

---

## Dosya Listesi ve Satır Sayıları

| Dosya | Satır |
|-------|-------|
| serde_snapshot.rs | 956 |
| serde_snapshot/newer.rs | 492 |
| serde_snapshot/storage.rs | 40 |
| serde_snapshot/utils.rs | 118 |
| serde_snapshot/tests.rs | 848 |
| snapshot_archive_info.rs | 163 |
| snapshot_bank_utils.rs | 2641 |
| snapshot_config.rs | 99 |
| snapshot_hash.rs | 53 |
| snapshot_minimizer.rs | 688 |
| snapshot_package.rs | 337 |
| snapshot_package/compare.rs | 499 |
| snapshot_utils.rs | 3214 |
| snapshot_utils/archive_format.rs | 172 |
| snapshot_utils/snapshot_storage_rebuilder.rs | 517 |

---

## Genel Mimari: Snapshot Pipeline

Solana snapshot sistemi 5 ana aşamadan oluşur:

```
[Bank] → add_bank_snapshot() → [BankSnapshotDir PRE]
                 ↓
    AccountsHashVerifier hesaplar hash
                 ↓
    reserialize_bank_with_new_accounts_hash() → [BankSnapshotDir POST]
                 ↓
    archive_snapshot_package() → [tar.zst / tar.lz4 arşiv]
                 ↓
    Validators download → unarchive_snapshot() → bank_from_streams()
```

Full snapshot ve incremental snapshot iki ayrı akış olarak paralel işlenir, sonra `collapse_into()` ile birleştirilir.

---

### [serde_snapshot.rs] (956 satır)

**Amaç:** Bank ve AccountsDb nesnelerinin binary serializasyon/deserializasyon altyapısı — snapshot sisteminin çekirdeği.

**Struct/Enum/Trait Listesi:**

- `SerdeStyle` (satır 70): `enum { Newer }` — Yalnızca tek bir varyant. Eski formatlar kaldırılmış, yalnızca "newer" (V1_2_0) destekleniyor. Copy+Clone+Eq+PartialEq.

- `AccountsDbFields<T>` (satır 77): Tuple struct — `HashMap<Slot, Vec<T>>, StoredMetaWriteVersion, Slot, BankHashInfo, Vec<Slot>, Vec<(Slot, Hash)>`. T genellikle `SerializableAccountStorageEntry`. Serde `#[serde(deserialize_with = "default_on_eof")]` ile son iki alan backward-compatible.

- `BankIncrementalSnapshotPersistence` (satır 99): `full_slot: Slot, full_hash: SerdeAccountsHash, full_capitalization: u64, incremental_hash: SerdeIncrementalAccountsHash, incremental_capitalization: u64`. Incremental snapshot'ın base snapshot'a göre tutarlılık sağlamak için kullanılır. AbiExample + Serialize + Deserialize.

- `BankHashInfo` (satır 113): Private struct — `accounts_delta_hash: SerdeAccountsDeltaHash, accounts_hash: SerdeAccountsHash, stats: BankHashStats`. Snapshot'ta saklanır.

- `SnapshotStreams<'a, R>` (satır 121): `full_snapshot_stream: &'a mut BufReader<R>, incremental_snapshot_stream: Option<&'a mut BufReader<R>>`. Full+incremental snapshot'ı aynı anda işlemek için wrapper.

- `SnapshotBankFields` (satır 129): `full: BankFieldsToDeserialize, incremental: Option<BankFieldsToDeserialize>`. `collapse_into()` metodu: incremental varsa onu, yoksa full'u döner.

- `SnapshotAccountsDbFields<T>` (satır 144): `full_snapshot_accounts_db_fields: AccountsDbFields<T>, incremental_snapshot_accounts_db_fields: Option<AccountsDbFields<T>>`. `collapse_into()` kritik mantık içerir.

- `TypeContext<'a>` (satır 194): Trait — serde versiyonlama için soyutlama katmanı. Her serde versiyonu bu trait'i implement eder:
  - `type SerializableAccountStorageEntry`
  - `fn serialize_bank_and_storage()`
  - `fn serialize_bank_and_storage_without_extra_fields()` (yalnızca test)
  - `fn serialize_accounts_db_fields()`
  - `fn deserialize_bank_fields()`
  - `fn deserialize_accounts_db_fields()`
  - `fn reserialize_bank_fields_with_hash()`

- `SerializableBankAndStorage<'a, C>` (satır 493): `bank: &'a Bank, snapshot_storages: &'a [Vec<Arc<AccountStorageEntry>>], phantom: PhantomData<C>`. Serialize impl: `C::serialize_bank_and_storage()` delegasyonu.

- `SerializableBankAndStorageNoExtra<'a, C>` (satır 526): Yalnızca #[cfg(test)]. Extra field'lar (lamports_per_signature, epoch_accounts_hash vb.) olmadan serialize eder.

- `SerializableAccountsDb<'a, C>` (satır 558): `accounts_db: &'a AccountsDb, slot: Slot, account_storage_entries: &'a [Vec<Arc<AccountStorageEntry>>], phantom: PhantomData<C>`. `IgnoreAsHelper` impl #[cfg(RUSTC_WITH_SPECIALIZATION)].

- `ReconstructedAccountsDbInfo` (satır 724): `accounts_data_len: u64`. Snapshot'tan AccountsDb rebuild edilirken hesaplanan metadata.

**Kritik Fonksiyonlar:**

- `deserialize_from<R, T>()` (satır 255-265): bincode options — `with_limit(MAX_STREAM_SIZE=32GiB)` + `with_fixint_encoding()` + `allow_trailing_bytes()`. `allow_trailing_bytes()` kritik: extra field'lar EOF'ta ignore edilir, bu sayede forward-compatible deserializasyon sağlanır.

- `compare_two_serialized_banks()` (satır 270-283): `#[cfg(feature = "dev-context-only-utils")]`. İki farklı bank snapshot dosyasını deserialize edip field bazında karşılaştırır. HashMap gibi non-deterministic yapılar binary karşılaştırmayı imkansız kıldığından struct karşılaştırması yapılır.

- `snapshot_storage_lengths_from_fields()` (satır 286-302): `AccountsDbFields`'dan `HashMap<Slot, HashMap<SerializedAppendVecId, usize>>` çıkarır. Rebuilder'a kaç byte bekleneceğini söyler.

- `fields_from_stream()` (satır 304-317): Single-stream deserializasyon. `SerdeStyle` match ile dispatch.

- `fields_from_streams()` (satır 319-348): Full + incremental iki stream'den `SnapshotBankFields` ve `SnapshotAccountsDbFields` döner. `transpose()?.unzip()` idiomatic kullanımı.

- `bank_from_streams()` (satır 351-389): 16 parametre! Deserialization + reconstruction pipeline'ı tek çağrıda yürütür.

- `bank_to_stream()` (satır 391-410): Bank + storages'ı stream'e serialize eder.

- `reserialize_bank_fields_with_new_hash()` (satır 437-453): Read-modify-write: bank dosyasını okur, accounts_hash'i günceller, tekrar yazar.

- `reserialize_bank_with_new_accounts_hash()` (satır 460-491): Disk üzerinde `.pre` dosyasını okur, hash'i günceller, `.post` olarak yazar, `.pre`'yi siler. `found = false` ile "pre dosyası yok" durumunu handle eder.

- `reconstruct_single_storage()` (satır 644-657): Tek bir AppendVec dosyasından `AccountStorageEntry` reconstruct eder.

- `remap_append_vec_file()` (satır 659-696): Full+incremental snapshot farklı node'lardan gelebileceğinden AppendVec ID çakışmaları olabilir. Bu fn ID'yi `next_append_vec_id.fetch_add(1, Ordering::AcqRel)` ile yeniden atayarak çakışmayı çözer. Loop: ID uygunsa kır, değilse `num_collisions.fetch_add(1)` ve devam et.

- `remap_and_reconstruct_single_storage()` (satır 698-720): `remap_append_vec_file()` + `reconstruct_single_storage()` pipeline'ı.

- `reconstruct_accountsdb_from_fields()` (satır 729-956): 14 parametre! En kritik fonksiyon. Adımlar:
  1. `AccountsDb::new_with_config()` oluştur
  2. `epoch_accounts_hash` set et
  3. Full snapshot'tan accounts hash + capitalization kaydet
  4. Incremental snapshot varsa incremental hash kaydet
  5. Cross-validate: `incremental_snapshot_persistence.full_slot == full_slot` assert
  6. `AccountsDbFields` collapse
  7. `accounts_db.paths` için dizinler oluştur
  8. `max_append_vec_id <= AppendVecId::MAX / 2` kontrol (ID alanı bitmeden önce uyarı)
  9. `accounts_delta_hash`, `bank_hash_stats`, `storage`, `next_id`, `write_version` set et
  10. "solNfyAccRestor" thread'i spawn: `notify_account_restore_from_snapshot()`
  11. `accounts_db.generate_index()` — paralel index rebuild
  12. Handle join
  13. `datapoint_info!` ile metrik emit

**Unsafe Bloklar:** Yok. Unsafe yerine `Arc::try_unwrap()` ve `AtomicUsize::fetch_add(Ordering::AcqRel)` tercih edilmiş.

**Const/Static:**
- `MAX_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024` (32 GiB) — deserializasyon güvenlik limiti.

**Tasarım Kararları:**
1. `TypeContext` trait ile versiyonlama: Yeni format geldiğinde trait impl eklemek yeterli, existing code değişmiyor.
2. `allow_trailing_bytes()`: Extra field'lar eski client'larda graceful skip. Forward-compat.
3. `default_on_eof` attribute: EOF gelirse field'ı default değeriyle doldur. Backward-compat.
4. PRE/POST dosya çifti: AHV hesap hash'ini async hesaplar, meanwhile validator block üretmeye devam eder. PRE=hash placeholder, POST=real hash.
5. `num_collisions` AtomicUsize: Append vec ID çakışma sayacı, metrik olarak izlenebilir.

**Nova Shield İçin Öğrenilen:**
- Snapshot serializasyonunda `bincode::options().with_limit().with_fixint_encoding().allow_trailing_bytes()` pattern'i forward/backward compat için kritik.
- PRE/POST dosya paradigması: state değişimi asenkron iken tutarlılık sağlar.
- AppendVec ID çakışma çözümü: Birden fazla node'dan gelen snapshot'ları merge ederken ID remapping şart.
- `TypeContext` versiyonlama: Nova Shield'da da serde version abstraction için benzer trait kullanılabilir.

---

### [serde_snapshot/newer.rs] (492 satır)

**Amaç:** "Newer" (V1_2_0) serde versiyonunun concrete implement — TypeContext trait impl + bank serializasyon detayları.

**Struct/Enum/Trait Listesi:**

- `UnusedAccounts` (satır 19): `unused1: HashSet<Pubkey>, unused2: HashSet<Pubkey>, unused3: HashMap<Pubkey, u64>`. Eski format artıkları. Serialize/Deserialize ama kullanılmıyor. AbiExample.

- `DeserializableVersionedBank` (satır 30): Clone + Deserialize, NOT Serialize. 29 field: `blockhash_queue, ancestors, hash, parent_hash, parent_slot, hard_forks, transaction_count, tick_height, signature_count, capitalization, max_tick_height, hashes_per_tick, ticks_per_slot, ns_per_slot, genesis_creation_time, slots_per_year, accounts_data_len, slot, epoch, block_height, collector_id, collector_fees, fee_calculator, fee_rate_governor, collected_rent, rent_collector, epoch_schedule, inflation, stakes, unused_accounts, epoch_stakes, is_delta`. `stakes: Stakes<Delegation>`.

- `SerializableVersionedBank<'a>` (satır 110): Serialize, NOT Deserialize. Reference-based — clone'dan kaçınmak için. `stakes: StakesEnum` (serialize_with="serde_stakes_enum_compat::serialize"), diğerleri reference field'lar. `#[cfg(RUSTC_WITH_SPECIALIZATION)] IgnoreAsHelper`.

- `Context` (satır 189): `pub(super) struct Context {}` — zero-size struct, TypeContext marker.

- `BankFieldsToSerialize<'a, T: Serialize>` (satır 439): Enum:
  - `WithoutEpochRewardStatus((SerializableVersionedBank<'a>, T, u64, Option<BankIncrementalSnapshotPersistence>, Option<Hash>))` — 1.14 uyumlu
  - `WithEpochRewardStatus((..aynı + &'a EpochRewardStatus))` — 1.14 ile okunamaz

**Kritik Fonksiyonlar:**

- `Context::serialize_bank_and_storage()` (satır 194-229): Ana serialization entry point:
  1. Bank ancestors'ı HashMap'e çevir
  2. `bank.get_fields_to_serialize(&ancestors)` çağır
  3. `lamports_per_signature` + `epoch_reward_status` al
  4. `get_serialize_bank_fields()` ile format seç
  5. WithoutEpochRewardStatus veya WithEpochRewardStatus olarak serialize et
  - **NOT (satır 218):** `None::<BankIncrementalSnapshotPersistence>` — Full snapshot'ta incremental persistence yok. Incremental snapshot'ta AHV tarafından reserialize edilirken eklenir.
  - **TODO (satır 217):** "if we do a snapshot version bump, consider moving this out" — lamports_per_signature ayrı field olarak sonradan eklendi.

- `Context::serialize_accounts_db_fields()` (satır 253-324): Detaylı:
  1. `write_version` load (Acquire ordering)
  2. `RefCell<usize>` ile entry sayısı say (interior mutability — non-Send closure içinde)
  3. `serialize_iter_as_map()` ile storage entries'i map olarak serialize et
  4. `get_accounts_delta_hash(slot)` — panic if None
  5. `get_accounts_hash(slot)` — NOT FOUND beklenir (AHV sonradan hesaplar), `unwrap_or_default()` ile placeholder
  6. `get_bank_hash_stats(slot)` — panic if None
  7. Tuple serialize: `(entries, version, slot, bank_hash_info, [], [])`
  8. `datapoint_info!("serialize_account_storage_ms", duration, num_entries)`

- `Context::deserialize_bank_fields()` (satır 326-351): Sıralı deserializasyon:
  1. `DeserializableVersionedBank` deserialize → `BankFieldsToDeserialize` dönüştür
  2. `deserialize_accounts_db_fields()`
  3. `lamports_per_signature` — `ignore_eof_error`: EOF=None, var=Some
  4. `incremental_snapshot_persistence` — `ignore_eof_error`
  5. `epoch_accounts_hash` — `ignore_eof_error`
  6. `epoch_reward_status` — `ignore_eof_error`
  - Her adım EOF-safe: eski format = missing field = default value.

- `Context::reserialize_bank_fields_with_hash()` (satır 365-436): Read → modify → write:
  1. `deserialize_bank_fields()` ile oku
  2. `accounts_db_fields.3.accounts_hash = (*accounts_hash).into()` — tuple index 3 = BankHashInfo, accounts_hash field güncelle
  3. `RwLock::new(mem::take(&mut rhs.blockhash_queue))` — ownership transfer
  4. epoch_reward_status varsa WithEpochRewardStatus, yoksa WithoutEpochRewardStatus
  5. `bincode::serialize_into(stream_writer, &data)`

- `get_serialize_bank_fields()` (satır 467-492): Pure function — epoch_reward_status None/Some'a göre enum varyant seç.

- `DeserializableVersionedBank -> BankFieldsToDeserialize` (From impl, satır 66-105): `incremental_snapshot_persistence: None, epoch_accounts_hash: None, epoch_reward_status: EpochRewardStatus::Inactive` default değerler.

- `BankFieldsToSerialize<'a> -> SerializableVersionedBank<'a>` (From impl, satır 146-183): `stakes: StakesEnum::from(rhs.stakes.stakes().clone())`, `unused_accounts: UnusedAccounts::default()`.

**Unsafe Bloklar:** Yok.

**Tasarım Kararları:**
1. `DeserializableVersionedBank` ayrı (Deserialize-only), `SerializableVersionedBank` ayrı (Serialize-only): Reference-based serialize + owned deserialize ayrımı. Clone maliyetinden kaçınır.
2. `ignore_eof_error` pattern: Eski snapshot'larla compat — yeni field'lar sona eklenir, eski reader EOF'ta durur, yeni reader None/default alır.
3. `#[allow(dead_code)] unused_accounts`: Eski format artığı korunuyor, silinmesi ABI kırılması demek.
4. `accounts_hash = unwrap_or_default()` (satır 292): Snapshot alınırken hash hesap VERİLMEZ, placeholder yazılır. AHV sonradan reserialize eder. Bu critical pipeline detail.
5. `WithEpochRewardStatus` 1.14 ile okunamaz — feature flag kontrolü yok, eski client panics. Bu bilinçli breaking change.

**Nova Shield İçin Öğrenilen:**
- Serialize-only ve Deserialize-only struct ayrımı referance-based serialization için mükemmel pattern.
- `ignore_eof_error` backward compat için; yeni field'lar her zaman sona eklenmeli.
- Placeholder hash yazma + sonradan reserialize: Async hash hesabına olanak tanır.
- Zero-size `Context` struct ile trait dispatch: runtime overhead yok.

---

### [serde_snapshot/storage.rs] (40 satır)

**Amaç:** AccountStorageEntry'nin disk formatı temsili — yalnızca id + length.

**Struct/Enum/Trait Listesi:**

- `SerializedAppendVecId = usize` (satır 7): Type alias. `usize` olarak fixed — platform-specific boyut riski var ama snapshot'lar aynı platform'da kullanılıyor.

- `SerializableAccountStorageEntry` (satır 11): `id: SerializedAppendVecId, accounts_current_len: usize`. Copy+Clone+Debug+Default+Eq+PartialEq+Serialize+Deserialize. `#[cfg(RUSTC_WITH_SPECIALIZATION)] IgnoreAsHelper`.

- `SerializableStorage` (satır 16): Trait — `fn id() -> SerializedAppendVecId`, `fn current_len() -> usize`. `pub(super)`.

**Kritik Fonksiyonlar:**

- `From<&AccountStorageEntry> for SerializableAccountStorageEntry` (satır 30-37): `id: rhs.append_vec_id() as SerializedAppendVecId, accounts_current_len: rhs.accounts.len()`. Runtime `AccountStorageEntry`'den minimal snapshot formatına dönüşüm.

**Unsafe Bloklar:** Yok.

**Tasarım Kararları:**
- Minimal format: Yalnızca id + current_len snapshot'ta saklanır. Actual data AppendVec dosyalarında. Bu sayede bank snapshot dosyası küçük kalır (metadata only), büyük veri ayrı.
- `accounts_current_len` critical: Rebuilder bu sayıyı AppendVec'i memory map ederken kullanır; fazla/az değer corruption.

**Nova Shield İçin Öğrenilen:**
- Metadata-only snapshot (id + len) + separate data files ayrımı: Bank state küçük, account data büyük. Bu ayrım performans için kritik.
- `usize` as storage ID: 32-bit platformda snapshot uyumsuzluğu. Nova Shield'da explicit `u64` kullanılmalı.

---

### [serde_snapshot/utils.rs] (118 satır)

**Amaç:** Iterator'ları serde seq/map/tuple'a çeviren helper fonksiyonlar.

**Struct/Enum/Trait Listesi:**

- `SerializableSequencedIterator<I>` (2 adet, satır 16 ve 55): İçeride tanımlı local struct'lar. `RefCell<Option<I>>` ile interior mutability. `IgnoreAsHelper` impl (test+specialization).

- `SerializableMappedIterator<I>` (satır 94): (K, V) iterator'ları serde map olarak serialize eder.

**Kritik Fonksiyonlar:**

- `serialize_iter_as_seq<I>()` (satır 10-45): `I: IntoIterator + ExactSizeIterator`. `serializer.serialize_seq(Some(iter.len()))` — boyutu önceden belirtmek bazı format'lar için optimization sağlar. `RefCell::borrow_mut().take()` — iterator'ı tek kullanım için tüketir.

- `serialize_iter_as_tuple<I>()` (satır 49-84): `serialize_tuple` — serde'de tuple format bazı codec'lerde farklı encoding.

- `serialize_iter_as_map<K, V, I>()` (satır 88-118): `serializer.collect_map()` — daha verimli map serialization.

**Unsafe Bloklar:** Yok.

**Tasarım Kararları:**
- `#[allow(dead_code)]` hepsi — bazıları kullanılmıyor, ama API tamamlığı için korunuyor.
- `RefCell<Option<I>>`: Iterator `Serialize::serialize()` `&self` alır (immutable ref), ama tüketim `mut` gerektirir. `RefCell` bu bağdaşmazlığı çözer.
- `ExactSizeIterator` bound: `serialize_seq(Some(n))` ile serde hint verir, bazı format'larda prefix length yazılır.

**Nova Shield İçin Öğrenilen:**
- `RefCell<Option<T>>` with `take()`: Immutable context'te lazy owned consumption pattern.
- Iterator-based streaming serialization: Tüm collection'ı önceden Vec'e toplamak yerine iterator üzerinden serialize et → memory efficiency.

---

### [serde_snapshot/tests.rs] (848 satır)

**Amaç:** AccountsDb serialization/deserialization roundtrip testleri — snapshot güvenliğini doğrular.

**Test Fonksiyonları:**

- `linear_ancestors()` (satır 45): Test helper — slot 0..end_slot lineer Ancestors oluşturur.

- `context_accountsdb_from_stream<C, R>()` (satır 53): Generic — herhangi bir `TypeContext` ile AccountsDb deserialize eder.

- `accountsdb_from_stream()` + `accountsdb_to_stream()` (satır 90, 108): SerdeStyle dispatch wrapper'ları.

- `copy_append_vecs()` (satır 132-170): Test helper — "unpack from tarball" simüle eder. AccountsDb storage'ları output_dir'e kopyalar, yeni `StorageAndNextAppendVecId` döner.

- `reconstruct_accounts_db_via_serialization()` (satır 172-208): Full roundtrip — serialize → buf → deserialize. `accounts_db.temp_paths.push(copied_accounts)` trick: TempDir lifetime'ını uzatır.

- `check_accounts_local()` (satır 210-221): Random 100 account check.

- `test_accounts_serialize_style()` (satır 223-273): Core test — 100 account oluştur, serialize, deserialize, delta hash ve accounts hash eşit mi kontrol et.

- `#[test] test_accounts_serialize_newer()` (satır 276): `test_accounts_serialize_style(SerdeStyle::Newer)`.

- `#[test] test_remove_unrooted_slot_snapshot()` (satır 281): Purge → snapshot → check purged account absent.

- `#[test] test_accounts_db_serialize1()` (satır 316): İki pass — 3 slot, 100+ account, clean, modify, serialize, deserialize, hash kontrol.

- `#[test] test_accounts_db_serialize_zero_and_free()` (satır 430): Zero-lamport account serialization.

- `with_chained_zero_lamport_accounts()` (satır 479): Zero-lamport chain test helper. `verify_accounts_hash_and_lamports` çağrısı.

- `#[test] test_accounts_purge_chained_purge_before_snapshot_restore()` (satır 549): clean → serialize.

- `#[test] test_accounts_purge_chained_purge_after_snapshot_restore()` (satır 558): serialize → clean → serialize.

- `#[test] test_accounts_purge_long_chained_after_snapshot_restore()` (satır 569): Çok adımlı purge zinciri.

- `#[test] test_accounts_clean_after_snapshot_restore_then_old_revives()` (satır 637): Karmaşık ref_count senaryosu — `alive_roots.remove/insert` ile root manipülasyonu. `accounts.ref_count_for_pubkey()` tracking.

- `#[test] test_shrink_stale_slots_processed()` (satır 771): `shrink_all_slots()` öncesi/sonrası count kontrol + verify_accounts_hash_and_lamports.

**Test Coverage:**
- Serialize/deserialize roundtrip
- Zero-lamport account preservation
- Purge + snapshot + restore consistency
- Chained zero-lamport accounts
- AccountsDeltaHash + AccountsHash eşitliği
- Shrink + snapshot integration
- Unrooted slot purge

**Nova Shield İçin Öğrenilen:**
- Test kütüphanesi pattern: `reconstruct_accounts_db_via_serialization()` gibi "roundtrip helper" fonksiyon. Test isolation + reusability.
- Zero-lamport account'ların snapshot'ta korunması kritik — purge mantığı snapshot'tan önce/sonra farklı davranır.
- `verify_accounts_hash_and_lamports()` snapshot sonrası doğrulama: Kritik integrity check.

---

### [snapshot_archive_info.rs] (163 satır)

**Amaç:** Snapshot arşiv dosyalarının metadata tipi ve trait abstraksiyonu.

**Struct/Enum/Trait Listesi:**

- `SnapshotArchiveInfoGetter` (satır 13): Trait — `snapshot_archive_info() -> &SnapshotArchiveInfo`. Default impl'ler: `path()`, `slot()`, `hash()`, `archive_format()`, `is_remote()`.
  - `is_remote()`: `path.parent().ends_with("remote")` kontrolü — remote download dir'den gelen snapshot'ları identify eder.

- `SnapshotArchiveInfo` (satır 44): `path: PathBuf, slot: Slot, hash: SnapshotHash, archive_format: ArchiveFormat`. PartialEq+Eq+Debug+Clone.

- `FullSnapshotArchiveInfo` (satır 60): Newtype wrapper over `SnapshotArchiveInfo`. `Ord` impl: slot'a göre.

- `IncrementalSnapshotArchiveInfo` (satır 103): `base_slot: Slot, inner: SnapshotArchiveInfo`. `Ord` impl: önce `base_slot`, sonra `slot` — aynı base'e sahip incremental'lar slot sırasına göre sıralanır.

**Kritik Fonksiyonlar:**

- `FullSnapshotArchiveInfo::new_from_path()` (satır 64): `parse_full_snapshot_archive_filename()` ile filename parse. Format: `snapshot-{slot}-{hash}.{ext}`.

- `IncrementalSnapshotArchiveInfo::new_from_path()` (satır 115): `parse_incremental_snapshot_archive_filename()` ile parse. Format: `incremental-snapshot-{base_slot}-{slot}-{hash}.{ext}`.

- `IncrementalSnapshotArchiveInfo::base_slot()` (satır 138): Getter — hangi full snapshot'a dayandığını döner.

**Tasarım Kararları:**
- `is_remote()` path-based detection: Remote download dir'deki snapshot'lar validator'dan gelen arşivler. Bunlar için `shrink` skip edilir (hız optimizasyonu).
- `Ord` incremental: `base_slot` → `slot` sırası — aynı base için en yeni snapshot'ı bulmak O(n) sort + last.

**Nova Shield İçin Öğrenilen:**
- Snapshot dosya adı format standartlaştırma: `snapshot-{slot}-{hash}.{ext}` → filename parsing otomatik.
- Remote vs. local ayrımı path-based: Minimal metadata ile derin davranış farkı.

---

### [snapshot_bank_utils.rs] (2641 satır)

**Amaç:** Bank ↔ Snapshot dönüşümü için yüksek seviyeli utility fonksiyonlar — snapshot alma ve yükleme orkestrasyonu.

**Struct/Enum/Trait Listesi:**

- `BankFromArchivesTimings` (satır 210): `untar_full_snapshot_archive_us: u64, untar_incremental_snapshot_archive_us: u64, rebuild_bank_us: u64, verify_bank_us: u64`. Debug.

- `BankFromDirTimings` (satır 218): `rebuild_storages_us: u64, rebuild_bank_us: u64`. Debug.

- `VerifySlotDeltasStructuralInfo` (satır 896): Private — `slots: HashSet<Slot>`. `verify_slot_deltas_structural()` return type.

**Const/Static:**
- `DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 25_000` (satır 60)
- `DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 100` (satır 61)
- `DISABLED_SNAPSHOT_ARCHIVE_INTERVAL: Slot = Slot::MAX` (satır 62) — Snapshot devre dışı bırakmak için sentinel.

**Kritik Fonksiyonlar:**

- `add_bank_snapshot()` (satır 70-197): **DEVELOPER NOTE**: "Any error returned may bring down the node!" Adımlar:
  1. `bank_snapshot_dir` zaten var mı kontrol et (idempotency koruması)
  2. `fs::create_dir_all()`
  3. `hard_link_storages_to_snapshot()` — accounts hardlink
  4. `bank_to_stream()` → `.pre` dosyasına serialize
  5. `serialize_status_cache()` → `status_cache` dosyası
  6. `version` dosyası yaz
  7. `state_complete` flag dosyası oluştur
  8. `datapoint_info!("snapshot_bank")` metrics
  - Returns `BankSnapshotInfo { snapshot_kind: BankSnapshotKind::Pre, .. }`

- `serialize_status_cache()` (satır 199-207): BankSlotDelta bincode serialize.

- `bank_fields_from_snapshot_archives()` (satır 225-260): Validation-only: Archive'dan Bank fields okur ama Bank reconstruct etmez. Leader schedule, epoch schedule gibi metadata için kullanılır.

- `bank_from_snapshot_archives()` (satır 264-408): Main entry point — Full+incremental archive'dan Bank:
  1. `verify_and_unarchive_snapshots()` — extract
  2. Storage merge (full + incremental)
  3. `rebuild_bank_from_unarchived_snapshots()` — deserialize
  4. `verify_bank_against_expected_slot_hash()` — hash doğrula
  5. incremental + feature active ise `base_capitalization` al
  6. `bank.verify_snapshot_bank()` — full integrity check (hash verify, shrink, clean)
  7. `datapoint_info!("bank_from_snapshot_archives")` metrics

- `bank_from_latest_snapshot_archives()` (satır 415-475): `get_highest_full_snapshot_archive_info()` + `get_highest_incremental_snapshot_archive_info()` ile en yeni archive'ı bul, `bank_from_snapshot_archives()` çağır.

- `bank_from_snapshot_dir()` (satır 479-558): Archive yerine dir'den:
  1. Account path'leri temizle (`delete_contents_of_path`)
  2. `rebuild_storages_from_snapshot_dir()` — hardlink'leri kopyala
  3. `rebuild_bank_from_snapshot()` — tek snapshot, incremental yok
  4. `set_initial_accounts_hash_verification_completed()` — verify skip (güven: local state)

- `bank_from_latest_snapshot_dir()` (satır 562-598): `get_highest_bank_snapshot_post()` + `bank_from_snapshot_dir()`.

- `verify_bank_against_expected_slot_hash()` (satır 602-618): `bank.slot() == expected_slot && bank.get_snapshot_hash() == expected_hash`. Mismatch → `SnapshotError::MismatchedSlotHash`.

- `verify_slot_deltas_structural()` (satır 852-892): Kontroller:
  1. `len <= MAX_CACHE_ENTRIES` (status cache sınırı)
  2. Her entry root olmalı
  3. Slot ≤ bank_slot
  4. Duplicate slot yok

- `verify_slot_deltas_with_history()` (satır 903-939): SlotHistory ile cross-validate:
  1. `slot_history.newest() == bank_slot`
  2. Her slot delta → history'de var mı (`Check::Found`)
  3. History'deki son MAX_CACHE_ENTRIES slot → delta'da var mı (highest-to-lowest traverse)

- `bank_to_full_snapshot_archive()` (satır 964-1005): Convenience — Bank'tan full archive:
  1. `assert!(bank.is_complete())`
  2. `bank.squash(), force_flush_accounts_cache(), clean_accounts(), update_accounts_hash(), rehash()`
  3. `add_bank_snapshot()` + `package_and_archive_full_snapshot()`

- `bank_to_incremental_snapshot_archive()` (satır 1014-1065): Incremental:
  1. `assert!(bank.slot() > full_snapshot_slot)`
  2. Feature check: `update_incremental_accounts_hash(full_snapshot_slot)` veya `update_accounts_hash()`
  3. `package_and_archive_incremental_snapshot()`

- `package_and_archive_full_snapshot()` (satır 1069-1114):
  1. `AccountsPackage::new_for_snapshot(SnapshotKind::FullSnapshot)`
  2. `bank.get_accounts_hash()` al
  3. `reserialize_bank_with_new_accounts_hash()` — hash'i PRE'ye yaz → POST oluştur
  4. `SnapshotPackage::new()` + `archive_snapshot_package()`

- `package_and_archive_incremental_snapshot()` (satır 1118-1200): Feature-gated logic:
  - Feature active: `BankIncrementalSnapshotPersistence` oluştur (full_slot, full_hash, full_cap, incremental_hash, incremental_cap). Accounts hash placeholder.
  - Feature inactive: normal accounts hash kullan.

- `get_snapshot_storages()` (satır 942-956): `bank.get_snapshot_storages(None)` + metric.

- `rebuild_bank_from_unarchived_snapshots()` (satır 683-779): Archive'dan unpack edilmiş dir'den bank rebuild. Full + incremental SnapshotRootPaths oluştur, `deserialize_snapshot_data_files()` ile streams aç, `bank_from_streams()` çağır, status cache deserialize et, `verify_slot_deltas()`, `bank.status_cache.append()`.

- `rebuild_bank_from_snapshot()` (satır 782-839): Dir'den bank rebuild (tek snapshot). `bank_from_streams()` + status cache.

- `verify_slot_deltas()` (satır 842-848): Structural + history check pipeline.

- `create_snapshot_dirs_for_tests()` (satır 1203-1252): `#[cfg(feature = "dev-context-only-utils")]`. PRE/POST döngüsü test setup.

**Test Coverage (satır 1254+):**
- `test_roundtrip_bank_to_and_from_full_snapshot_simple()` — en basit roundtrip
- `test_roundtrip_bank_to_and_from_snapshot_complex()` — çok slot, çok transfer (devamı kesildi ama varlığı görüldü)

**Unsafe Bloklar:** Yok.

**Tasarım Kararları:**
1. `add_bank_snapshot`: "any error may bring down node" — AccountsBackgroundService'ten çağrılır, kritik path.
2. `state_complete` flag: İki aşamalı commit pattern. Dosyalar yazılır, sonra tamamlandı bayrağı set edilir. Crash recovery: bayraksız dizinler incomplete sayılır.
3. Full snapshot: `bank.rehash()` snapshot öncesi — manuel account değişikliklerini yakalar.
4. Slot delta doğrulama çift katman: Structural (format doğruluğu) + History (semantik doğruluk) — iki ayrı fonksiyon.
5. `test_hash_calculation` flag: Skip edilirse `verify_snapshot_bank` false dönse bile panic yok.

**Nova Shield İçin Öğrenilen:**
- Two-phase commit for snapshot dirs: write all files → set state_complete flag. Crash-safe.
- Slot delta validation: Status cache corruption detection için history cross-validation şart.
- `DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS = 25_000` (~10 dakika 400ms/slot): Nova Shield için benzer aralık belirlenebilir.
- `DISABLED_SNAPSHOT_ARCHIVE_INTERVAL = Slot::MAX` sentinel: Testlerde/özel modlarda snapshot devre dışı bırakma.

---

### [snapshot_config.rs] (99 satır)

**Amaç:** Snapshot subsystem konfigürasyon struct'ı — tüm snapshot parametrelerini tek yerde toplar.

**Struct/Enum/Trait Listesi:**

- `SnapshotConfig` (satır 12): Clone+Debug.
  - `usage: SnapshotUsage`
  - `full_snapshot_archive_interval_slots: Slot` (default: 25_000)
  - `incremental_snapshot_archive_interval_slots: Slot` (default: 100)
  - `full_snapshot_archives_dir: PathBuf`
  - `incremental_snapshot_archives_dir: PathBuf`
  - `bank_snapshots_dir: PathBuf`
  - `archive_format: ArchiveFormat` (default: TarZstd)
  - `snapshot_version: SnapshotVersion` (default: V1_2_0)
  - `maximum_full_snapshot_archives_to_retain: NonZeroUsize` (default: 2)
  - `maximum_incremental_snapshot_archives_to_retain: NonZeroUsize` (default: 4)
  - `accounts_hash_debug_verify: bool`
  - `packager_thread_niceness_adj: i8` (thread priority adjustment)

- `SnapshotUsage` (satır 93): `enum { LoadOnly, LoadAndGenerate }`. LoadOnly: startup only. LoadAndGenerate: hem startup hem steady-state.

**Kritik Fonksiyonlar:**

- `SnapshotConfig::new_load_only()` (satır 77): `#[must_use]` — `usage: SnapshotUsage::LoadOnly` ile default.

- `SnapshotConfig::should_generate_snapshots()` (satır 86): `#[must_use]` — `usage == LoadAndGenerate`.

**Tasarım Kararları:**
- `NonZeroUsize` for retain counts: Compile-time guarantee ≥1 archive korunur.
- `packager_thread_niceness_adj: i8`: Snapshot arşivleme thread'inin CPU önceliği. Validator thread'leri etkilemesin diye niceness ayarı.
- `DEFAULT_ARCHIVE_COMPRESSION = "zstd"`: LZ4'ten daha iyi sıkıştırma, makul hız.

**Nova Shield İçin Öğrenilen:**
- Snapshot config'i tek struct'ta topla — `SnapshotConfig` pattern. Farklı component'lar aynı config'e referans alır.
- `NonZeroUsize` for counts: "0 archive sakla" mantıksal hata, type-level engellenmiş.
- Thread niceness: I/O-heavy background işler için priority lowering önemli.

---

### [snapshot_hash.rs] (53 satır)

**Amaç:** Snapshot doğrulama için hash hesaplama — accounts hash + epoch accounts hash kombinasyonu.

**Struct/Enum/Trait Listesi:**

- `StartingSnapshotHashes` (satır 15): `full: FullSnapshotHash, incremental: Option<IncrementalSnapshotHash>`. Validator startup'ta CRDS'e push etmek için. Copy+Clone+Debug+PartialEq+Eq.

- `FullSnapshotHash` (satır 23): Newtype — `(Slot, SnapshotHash)`. Type safety: CRDS'e push ederken yanlış hash tipini engellemek için.

- `IncrementalSnapshotHash` (satır 28): Newtype — `(Slot, SnapshotHash)`. Aynı amaç.

- `SnapshotHash` (satır 32): Newtype — `Hash`. Debug+PartialEq+Eq+Clone+Copy.

**Kritik Fonksiyonlar:**

- `SnapshotHash::new()` (satır 37-53): `#[must_use]`:
  ```
  epoch_accounts_hash = None  → snapshot_hash = accounts_hash
  epoch_accounts_hash = Some  → snapshot_hash = SHA256(accounts_hash || epoch_accounts_hash)
  ```
  Kombinasyon: `Hasher::default()` + `hasher.hash(accounts_hash.as_hash().as_ref())` + `hasher.hash(epoch_accounts_hash.as_ref().as_ref())` + `hasher.result()`.

**Tasarım Kararları:**
- Epoch Accounts Hash opsiyonel: Her epoch başında tüm hesaplar hash'lenir (tam mutabakat). Bu hash yalnızca epoch sınırında mevcut. Normal slot'larda `None`.
- Gossip/CRDS'e hash yayma: Diğer validator'lar snapshot hash'ini gossip aracılığıyla öğrenir. `StartingSnapshotHashes` bu bilgiyi taşır.
- Newtype pattern: `FullSnapshotHash` ve `IncrementalSnapshotHash` ayrı tipler — `SnapshotPackagerService`'te yanlış hash yanlış yere gönderilmez.

**Nova Shield İçin Öğrenilen:**
- Snapshot hash = accounts_hash (+ optionally epoch_accounts_hash). İki hash'i birleştirmek → hasher'a hash_bytes olarak feed et.
- CRDS hash yayma: Gossip tabanlı snapshot discovery → validator'lar birbirlerinin snapshot'larını hashten doğrular.
- Newtype ile type-safe gossip: Nova Shield'da benzer tip ayrımı kritik.

---

### [snapshot_minimizer.rs] (688 satır)

**Amaç:** Minimal snapshot oluşturma — yalnızca belirli slot aralığını replay edebilmek için gereken minimum account set'ini belirle ve fazlasını sil.

**Struct/Enum/Trait Listesi:**

- `SnapshotMinimizer<'a>` (satır 36): `bank: &'a Bank, starting_slot: Slot, ending_slot: Slot, minimized_account_set: DashSet<Pubkey>`. DashSet = thread-safe concurrent HashSet.

**Kritik Fonksiyonlar:**

- `SnapshotMinimizer::minimize()` (satır 50-83): Static factory + orchestration. Sıralı accumulation:
  1. `get_active_bank_features()` — feature set aktif pubkey'ler
  2. `get_inactive_bank_features()` — inactive feature pubkey'ler
  3. `get_builtins()` — BUILTINS static list
  4. `get_static_runtime_accounts()` — STATIC_IDS
  5. `get_sdk_accounts()` — SDK_IDS
  6. `get_rent_collection_accounts()` — slot range için rent partition accounts
  7. `get_vote_accounts()` — vote + node pubkey'ler
  8. `get_stake_accounts()` — stake accounts
  9. `get_owner_accounts()` — her account'ın owner'ı
  10. `get_programdata_accounts()` — upgradeable BPF program data
  11. `minimize_accounts_db()` — fazlasını sil
  12. `bank.force_flush_accounts_cache()` + `bank.set_capitalization()` — güncelle

- `add_accounts<F>()` (satır 86-98): Wrapper — `initial_accounts_len` farkıyla kaç account eklendi logla + measure! timing.

- `get_rent_collection_accounts()` (satır 136-160): Slot range için rent partition'larını hesapla (`variable_cycle_partitions_between_slots` veya `fixed_cycle_partitions_between_slots`). Her partition → `pubkey_range_from_partition()` → `load_to_collect_rent_eagerly()` → `into_par_iter()`.

- `get_vote_accounts()` (satır 163-174): `bank.vote_accounts().par_iter()` → pubkey + `vote_state.node_pubkey`.

- `get_owner_accounts()` (satır 184-194): `minimized_account_set.par_iter()` → `bank.get_account(pubkey)` → `account.owner()` → yeni pubkey'leri ekle.

- `get_programdata_accounts()` (satır 197-219): Executable + BPF upgradeable program → `UpgradeableLoaderState::Program { programdata_address }` → programdata pubkey ekle.

- `minimize_accounts_db()` (satır 222-249):
  1. `get_minimized_slot_set()` — hangi slot'lar gerekli
  2. `process_snapshot_storages()` — dead_slots + dead_storages tespit
  3. `log_dead_slots = false` (logging suppress)
  4. `purge_dead_slots()` — gereksiz slot'ları sil
  5. `drop(dead_storages)` — Arc release
  6. `log_dead_slots = true` restore

- `get_minimized_slot_set()` (satır 252-274): `minimized_account_set.par_iter()` → accounts_index → `slot_list.iter().map(slot).max()` → max slot set.

- `process_snapshot_storages()` (satır 277-303): `get_snapshot_storages(..=starting_slot)`. Starting slot'u atla, minimized_slot_set'te olanları `filter_storage()`, olmayanları dead_slots'a ekle.

- `filter_storage()` (satır 306-390): Storage entry'den yalnızca `minimized_account_set`'tekileri tut:
  1. `get_unique_accounts_from_storage()`
  2. `par_chunks(CHUNK_SIZE=50)` parallel process
  3. keep/purge listele
  4. `purge_keys_exact()`
  5. Yeni storage oluştur (page-aligned), `store_accounts_frozen()`
  6. `mark_dirty_dead_stores()` → dead_storages'a ekle

- `purge_dead_slots()` (satır 393-397): `purge_slots_from_cache_and_store(dead_slots.iter(), &stats, false)`.

- `accounts_db()` (satır 400-402): `&self.bank.rc.accounts.accounts_db` convenience getter.

**Test Coverage (satır 405+):**
- `test_get_rent_collection_accounts`: Slot range → rent partition pubkey
- `test_minimization_get_vote_accounts`: Bootstrap validator vote+node pubkey
- `test_minimization_get_stake_accounts`: Genesis stake accounts
- `test_minimization_get_owner_accounts`: Owner pubkey transitivity
- `test_minimization_add_programdata_accounts`: Upgradeable program data address
- `test_minimize_accounts_db`: 5 slot × 300 account, 6 keep → 3 slot remaining (306 accounts)

**Unsafe Bloklar:** Yok.

**Tasarım Kararları:**
1. `DashSet<Pubkey>` — concurrent set, parallel accumulation mümkün.
2. `log_dead_slots = false/true` around purge: Binlerce dead slot log'u flood etmez.
3. `par_chunks(50)` — CHUNK_SIZE 50: Lock contention balance.
4. Minimizer sadece `ledger-tool` ve testlerde kullanılır (`info!` yorumunda belirtilmiş) — production hot path değil.

**Nova Shield İçin Öğrenilen:**
- Minimal snapshot = "replay the specified slot range" guarantee. Feature, builtin, rent, vote, stake, owner, programdata — hepsi gerekli.
- `DashSet` + `par_iter()` combination: Lock-free concurrent accumulation.
- Storage filter: Yalnızca needed account'ları tutan yeni storage oluştur, eskisini dead olarak işaretle → GC.

---

### [snapshot_package.rs] (337 satır)

**Amaç:** Snapshot paketleme için veri transfer nesneleri — AccountsBackgroundService → AccountsHashVerifier → SnapshotPackagerService pipeline.

**Struct/Enum/Trait Listesi:**

- `AccountsPackage` (satır 30): ABS'den AHV'ye gönderilen paket:
  - `package_kind: AccountsPackageKind`
  - `slot: Slot, block_height: Slot`
  - `snapshot_storages: Vec<Arc<AccountStorageEntry>>`
  - `expected_capitalization: u64`
  - `accounts_hash_for_testing: Option<AccountsHash>`
  - `accounts: Arc<Accounts>`
  - `epoch_schedule: EpochSchedule, rent_collector: RentCollector`
  - `is_incremental_accounts_hash_feature_enabled: bool`
  - `snapshot_info: Option<SupplementalSnapshotInfo>`
  - `enqueued: Instant` — queue wait time tracking

- `SupplementalSnapshotInfo` (satır 218): Snapshot-specific alanlar:
  - `bank_snapshot_dir: PathBuf`
  - `archive_format: ArchiveFormat, snapshot_version: SnapshotVersion`
  - `full_snapshot_archives_dir, incremental_snapshot_archives_dir: PathBuf`
  - `epoch_accounts_hash: Option<EpochAccountsHash>`

- `AccountsPackageKind` (satır 231): `AccountsHashVerifier, Snapshot(SnapshotKind), EpochAccountsHash`. Copy+Clone+Eq+PartialEq+Debug.

- `SnapshotPackage` (satır 238): AHV'den SnapshotPackagerService'e gönderilen paket:
  - `snapshot_archive_info: SnapshotArchiveInfo`
  - `block_height: Slot`
  - `bank_snapshot_dir: PathBuf`
  - `snapshot_storages: Vec<Arc<AccountStorageEntry>>`
  - `snapshot_version: SnapshotVersion, snapshot_kind: SnapshotKind`
  - `enqueued: Instant`

- `SnapshotKind` (satır 325): `FullSnapshot, IncrementalSnapshot(Slot)` — base_slot parametreli. Copy+Clone+Eq+PartialEq+Debug.

**Kritik Fonksiyonlar:**

- `AccountsPackage::new_for_snapshot()` (satır 53-97): Snapshot türü için ek kontrol — `IncrementalSnapshot(base_slot)` için `bank.slot() > base_slot` assert. `epoch_accounts_hash` snapshot'a dahil edilir.

- `AccountsPackage::new_for_accounts_hash_verifier()` (satır 101): `assert_eq!(package_kind, AccountsHashVerifier)`.

- `AccountsPackage::new_for_epoch_accounts_hash()` (satır 119): `assert_eq!(package_kind, EpochAccountsHash)`.

- `AccountsPackage::_new()` (satır 135-159): Ortak impl — `is_incremental_accounts_hash_feature_enabled` feature check, `enqueued: Instant::now()`.

- `AccountsPackage::bank_snapshot_dir()` (satır 192-204): EpochAccountsHash için panic — "EAH accounts packages do not contain snapshot information".

- `AccountsPackage::default_for_tests()` (satır 163-187): Test helper — `ArchiveFormat::Tar`, default fields.

- `SnapshotPackage::new()` (satır 252-303): `AccountsPackage` + `AccountsHashKind` → `SnapshotPackage`:
  1. AccountsPackageKind::Snapshot assert
  2. snapshot_info.take() unwrap
  3. `SnapshotHash::new(&accounts_hash, epoch_accounts_hash)` hesapla
  4. Incremental: `snapshot_storages.retain(|s| s.slot() > base_slot)` — yalnızca yeni slot'lar
  5. Archive path build
  6. `enqueued: Instant::now()` — AHV → SPS queue time

- `SnapshotKind::is_full_snapshot()` / `is_incremental_snapshot()` (satır 331-337): Convenience matches.

**Debug impl'leri:** Her iki paket için custom `Debug` — `finish_non_exhaustive()` ile storages gizlenir (çok büyük).

**Tasarım Kararları:**
1. İki aşamalı paket: `AccountsPackage` → hash hesapla → `SnapshotPackage`. Hash hesabı pahalı, ayrı pipeline stage.
2. `enqueued: Instant` her iki pakette: Queue wait latency monitoring.
3. `snapshot_info: Option<_>`: EpochAccountsHash paketi için None.
4. Incremental storage filter: `retain(slot > base_slot)` — full snapshot storages'ı çıkar.

**Nova Shield İçin Öğrenilen:**
- Pipeline DTO pattern: Farklı stage'ler için farklı paket tipleri — type-safe pipeline garantisi.
- Queue time tracking: `Instant::now()` at enqueue, measure at dequeue — backpressure detection.
- Incremental snapshot: Storages client-side filter edilir (base_slot'tan sonraki).

---

### [snapshot_package/compare.rs] (499 satır)

**Amaç:** Snapshot paket öncelik sıralama — AccountsHashVerifier kuyrukta birden fazla paket varsa önce önemlisini işle.

**Kritik Fonksiyonlar:**

- `cmp_snapshot_packages_by_priority()` (satır 11): `cmp_snapshot_kinds_by_priority().then(a.slot().cmp(&b.slot()))`.

- `cmp_accounts_packages_by_priority()` (satır 17): Kind önce, slot tiebreaker.

- `cmp_accounts_package_kinds_by_priority()` (satır 32-53): **Öncelik (yüksek → düşük):**
  1. `EpochAccountsHash` — en yüksek (epoch sınırı kritik)
  2. `Snapshot(Full)` > `Snapshot(Incremental)`
  3. `AccountsHashVerifier` — en düşük
  - `Incremental` vs `Incremental`: base_slot tiebreaker (yüksek base_slot = daha yeni full snapshot baz alıyor)

- `cmp_snapshot_kinds_by_priority()` (satır 60-70): `Full > Incremental`. İki `Incremental`: `base_slot` compare.

**Test Coverage (satır 72-499):** 3 test fonksiyonu:
- `test_cmp_snapshot_packages_by_priority`: 10 senaryo (Full/Incremental kombinasyonları)
- `test_cmp_accounts_packages_by_priority`: 23 senaryo (tüm tip kombinasyonları)
- `test_cmp_accounts_package_kinds_by_priority`: 15 senaryo (pure kind comparison)

**Tasarım Kararları:**
- EAH en yüksek: Epoch accounts hash tüm validator'larla konsensüs kritik.
- Full > Incremental: Full snapshot daha az sıklıkta, daha değerli.
- Incremental base_slot tiebreaker: Aynı incremental slot'ta birden fazla paket → daha yeni full base seç.

**Nova Shield İçin Öğrenilen:**
- Priority queue'da snapshot paketleri için explicit priority ordering: EAH > Full > Incremental > Verifier.
- `Ordering::then()` chaining: Birincil + secondary sort tek satırda.

---

### [snapshot_utils.rs] (3214 satır)

**Amaç:** Snapshot arşiv oluşturma, açma, doğrulama ve yönetimi için düşük seviyeli utility'ler.

**Const/Static (önemli olanlar):**
- `SNAPSHOT_STATUS_CACHE_FILENAME: &str = "status_cache"` (satır 56)
- `SNAPSHOT_VERSION_FILENAME: &str = "version"` (satır 57)
- `SNAPSHOT_STATE_COMPLETE_FILENAME: &str = "state_complete"` (satır 58)
- `SNAPSHOT_ACCOUNTS_HARDLINKS: &str = "accounts_hardlinks"` (satır 59)
- `SNAPSHOT_ARCHIVE_DOWNLOAD_DIR: &str = "remote"` (satır 60)
- `MAX_SNAPSHOT_DATA_FILE_SIZE: u64 = 32 * 1024 * 1024 * 1024` (satır 61) — 32 GiB
- `MAX_SNAPSHOT_VERSION_FILE_SIZE: u64 = 8` (satır 62) — 8 byte
- `TMP_SNAPSHOT_ARCHIVE_PREFIX: &str = "tmp-snapshot-archive-"` (satır 64)
- `BANK_SNAPSHOT_PRE_FILENAME_EXTENSION: &str = "pre"` (satır 65)
- `DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) }` (satır 70)
- `DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(4) }` (satır 72)
- `FULL_SNAPSHOT_ARCHIVE_FILENAME_REGEX: &str = r"^snapshot-(?P<slot>[[:digit:]]+)-(?P<hash>[[:alnum:]]+)\.(?P<ext>...)$"` (satır 74)
- `INCREMENTAL_SNAPSHOT_ARCHIVE_FILENAME_REGEX: &str = r"^incremental-snapshot-(?P<base>[[:digit:]]+)-(?P<slot>[[:digit:]]+)-(?P<hash>[[:alnum:]]+)\.(?P<ext>...)$"` (satır 75)
- `PARALLEL_UNTAR_READERS_DEFAULT: usize = 4` (satır 1220)

**Struct/Enum/Trait Listesi:**

- `SnapshotVersion` (satır 78): `enum { #[default] V1_2_0 }`. `Display`, `From<SnapshotVersion> for &'static str`, `FromStr`. `as_str()` helper. VERSION_STRING = "1.2.0".

- `BankSnapshotInfo` (satır 126): `slot: Slot, snapshot_kind: BankSnapshotKind, snapshot_dir: PathBuf, snapshot_version: SnapshotVersion`. `Ord` by slot. `new_from_dir()` + `snapshot_path()`.

- `BankSnapshotKind` (satır 239): `Pre, Post`. Pre: hash hesaplanmamış. Post: hash hesaplanmış + yazılmış.

- `SnapshotFrom` (satır 250): `Archive, Dir`. Farklı rebuild path'leri için.

- `SnapshotRootPaths` (satır 260): `full_snapshot_root_file_path: PathBuf, incremental_snapshot_root_file_path: Option<PathBuf>`.

- `UnarchivedSnapshot` (satır 267): `#[allow(dead_code)] unpack_dir: TempDir, storage: AccountStorageMap, unpacked_snapshots_dir_and_version: UnpackedSnapshotsDirAndVersion, measure_untar: Measure`. TempDir drop = auto cleanup.

- `UnpackedSnapshotsDirAndVersion` (satır 276): `unpacked_snapshots_dir: PathBuf, snapshot_version: SnapshotVersion`.

- `StorageAndNextAppendVecId` (satır 283): `pub(crate) struct` — `storage: AccountStorageMap, next_append_vec_id: AtomicAppendVecId`.

- `SnapshotError` (satır 289): `#[allow(clippy::large_enum_variant)]` — 25+ varyant. Önemli varyantlar:
  - `MismatchedSlotHash((Slot, SnapshotHash), (Slot, SnapshotHash))` — slot/hash mismatch
  - `MismatchedBaseSlot(Slot, Slot)` — incremental/full uyumsuzluğu
  - `VerifySlotDeltas(VerifySlotDeltasError)` — status cache corruption
  - `AddBankSnapshot(AddBankSnapshotError, Slot)` — snapshot alma hatası

- `SnapshotNewFromDirError` (satır 360): 6 varyant — dir format validation hataları.

- `VerifySlotDeltasError` (satır 383): 7 varyant — status cache doğrulama hataları.

- `AddBankSnapshotError` (satır 408): 7 varyant — snapshot oluşturma hataları.

- `ArchiveSnapshotPackageError` (satır 433): 22 varyant — arşivleme hataları. Her adım için ayrı varyant.

- `HardLinkStoragesToSnapshotError` (satır 500): 4 varyant.

- `GetSnapshotAccountsHardLinkDirError` (satır 516): 3 varyant, `SymlinkSnapshotHardLinkDir { source, original, link }` struct-varyant.

- `VerifyBank` (satır 2037): `Deterministic, NonDeterministic`. Test için — reserialized bank binary karşılaştırma vs struct karşılaştırma.

**Kritik Fonksiyonlar:**

- `BankSnapshotInfo::new_from_dir()` (satır 151-216): Validation sırası önemli:
  1. Dir var mı?
  2. `is_bank_snapshot_complete()` — `state_complete` file
  3. `status_cache` file var mı?
  4. `version` file okuyup parse et
  5. `.pre` file varsa → Pre kind (NOTE: Pre check ÖNCE gelir!)
  6. `.post` file varsa → Post kind
  - **Kritik not (satır 195-200):** Pre check önce gelir — AHV aktif iken yarım yazılmış Post dosyası olabilir. Pre+Post birlikte varsa → Pre dön (güvenli).

- `clean_orphaned_account_snapshot_dirs()` (satır 539-589): Symlink graph traversal — accounts_hardlinks'te referans edilmeyen account snapshot dirs sil.

- `purge_incomplete_bank_snapshots()` (satır 592-618): `!is_bank_snapshot_complete()` olan dirs sil.

- `remove_tmp_snapshot_archives()` (satır 630-654): `TMP_SNAPSHOT_ARCHIVE_PREFIX` ile başlayan geçici dosyaları temizle.

- `archive_snapshot_package()` (satır 656-856): Ana arşivleme fonksiyonu. Pipeline:
  1. Staging dir oluştur (tempdir in tar_dir)
  2. `staging/snapshots/`, `staging/accounts/` oluştur
  3. Snapshot file → symlink to staging
  4. Status cache → symlink
  5. Version file → symlink
  6. Her AppendVec: `storage.flush()` + canonical path + symlink to staging/accounts
  7. `staging_storage_path.is_file()` kontrol (symlink dangling mi?)
  8. Archive dosyası oluştur (format'a göre encoder seç)
  9. `tar::Builder` ile önce version+snapshots, sonra accounts (sıra önemli: hızlı metadata erişim için)
  10. Encoder.finish()
  11. `fs::rename()` — atomic move to final path
  12. `purge_old_snapshot_archives()`
  - Sıralama neden önemli: "we want to interleave unpacking with reconstruction" — version dosyasını önce okursan paralel unpack+rebuild mümkün.

- `get_bank_snapshots()` (satır 859-893): Dir listing + `BankSnapshotInfo::new_from_dir()`. Hata → debug log, not panic.

- `serialize_snapshot_data_file()` (satır 945): `MAX_SNAPSHOT_DATA_FILE_SIZE` limiti ile.

- `deserialize_snapshot_data_files_capped()` (satır 1011-1058): Full+incremental her ikisini de açar, closure çalıştırır, consumed bytes kontrol eder.

- `check_deserialize_file_consumed()` (satır 1086-1104): `consumed_size != file_size` ise hata — extra data kalmış = format hatası.

- `hard_link_storages_to_snapshot()` (satır 1172-1206): Her storage için:
  1. `storage.flush()` — pending writes commit
  2. `get_snapshot_accounts_hardlink_dir()` — `account_path/snapshot/slot/` dizini bul/oluştur
  3. `fs::hard_link(src, hardlink_path)` — inode'u paylaş

- `get_storages_to_serialize()` (satır 1210-1217): `Vec<Arc<AccountStorageEntry>>` → `Vec<Vec<Arc<AccountStorageEntry>>>`. Her element tek-elemanlı Vec. Serialize API uyumu için.

- `verify_and_unarchive_snapshots()` (satır 1223-1274): Full+incremental uyumlu mu kontrol et, sonra paralel untar. `parallel_divisions = (num_cpus::get() / 4).clamp(1, 4)`. Tek `next_append_vec_id` Arc paylaşımı.

- `streaming_unarchive_snapshot()` (satır 1301-1341): `SharedBuffer` tek archive'dan N reader oluştur → N thread → her thread `parallel_selector` ile N'de-1'ini işler.

- `unarchive_snapshot()` (satır 1379-1433): Ortak unarchive logic:
  1. TempDir oluştur
  2. `streaming_unarchive_snapshot()` + `SnapshotStorageRebuilder::rebuild_storage(Archive)`
  3. `create_snapshot_meta_files_for_unarchived_snapshot()` — dir'e meta files ekle (version, status_cache, state_complete hardlinks)

- `streaming_snapshot_dir_files()` (satır 1437-1453): Dir'den streaming — snapshot file, version file, account files channel'a gönder.

- `rebuild_storages_from_snapshot_dir()` (satır 1459-1542): Dir'den hardlink → run/ kopyala → `SnapshotStorageRebuilder::rebuild_storage(Dir)`.

- `snapshot_version_from_file()` (satır 1547-1582): File size `> MAX_SNAPSHOT_VERSION_FILE_SIZE` (8 byte) → hata. `read_to_string().trim()`.

- `check_are_snapshots_compatible()` (satır 1586-1604): `full.slot() == incremental.base_slot()`.

- `parse_full_snapshot_archive_filename()` (satır 1656-1684): `lazy_static! Regex`. Named captures: slot, hash, ext.

- `parse_incremental_snapshot_archive_filename()` (satır 1688-1720): Named captures: base, slot, hash, ext.

- `purge_old_snapshot_archives()` (satır 1822-1913): Full snapshot'ların son N tanesini tut, incremental'ları base_slot'a göre grupla:
  - highest_full_snapshot_slot için → `maximum_incremental_to_retain` tane
  - diğer retained full slot'lar için → 1 tane
  - orphan (retained olmayan full) → 0

- `untar_snapshot_create_shared_buffer()` (satır 1957-1982): Format'a göre decoder:
  - TarBzip2 → `BzDecoder<BufReader<File>>`
  - TarGzip → `GzDecoder<BufReader<File>>`
  - TarZstd → `zstd::stream::read::Decoder<BufReader<File>>`
  - TarLz4 → `lz4::Decoder<BufReader<File>>`
  - Tar → `BufReader<File>`
  Hepsi `SharedBuffer::new()` içine → çoklu reader mümkün.

- `should_take_full_snapshot()` (satır 2236): `block_height % interval == 0`.

- `should_take_incremental_snapshot()` (satır 2243): `block_height % interval == 0 && last_full_snapshot_slot.is_some()`.

**Unsafe Bloklar:**
- Satır 71, 73: `NonZeroUsize::new_unchecked(2)` ve `new_unchecked(4)`. Yorumda açıklanmış: "Safe because the values are fixed, known non-zero constants". Compile-time const'ta `Option::unwrap()` mümkün değil.

**Tasarım Kararları:**
1. Atomic archive rename: `TMP_SNAPSHOT_ARCHIVE_PREFIX` → final path `fs::rename()`. Yarım dosya görünmez.
2. Symlink-based staging: Copy yerine symlink → disk space tasarrufu, hız.
3. `SharedBuffer` + çoklu reader: Tek I/O + N-way parallel decompress.
4. Version+snapshots ÖNCE accounts: Streaming unpack sırasında bank state bilinir, accounts paralel yüklenir.
5. `MAX_SNAPSHOT_VERSION_FILE_SIZE = 8 byte`: Directory traversal saldırısı: büyük "version" dosyası yüklenemez.
6. `check_deserialize_file_consumed()`: Trailing garbage = format hatasına dönüştürülür.

**Nova Shield İçin Öğrenilen:**
- Archive format: tar + compression (zstd/lz4) → streaming decompress + parallel unpack.
- `SharedBuffer` pattern: Tek read stream'den çok sayıda parallel consumer.
- Atomic snapshot lifecycle: tmp → final via rename. Never see partial archive.
- Hardlink + symlink storage management: Aynı inode birden fazla path'ten erişilebilir → disk copy yok.
- Version file 8 byte limit: Trivial security hardening, önemli.
- Snapshot retention: Full N adet, incremental yalnızca en yeni full için M adet.

---

### [snapshot_utils/archive_format.rs] (172 satır)

**Amaç:** Snapshot arşiv format enum'u ve string dönüşümleri.

**Struct/Enum/Trait Listesi:**

- `ArchiveFormat` (satır 23): Copy+Clone+Debug+Eq+PartialEq+Display (strum).
  - `TarBzip2` → `tar.bz2` (deprecated)
  - `TarGzip` → `tar.gz` (deprecated)
  - `TarZstd` → `tar.zst` (aktif)
  - `TarLz4` → `tar.lz4` (aktif)
  - `Tar` → `tar` (deprecated, uncompressed)

- `ParseError` (satır 78): `InvalidExtension(String)`. Debug+Clone+Eq+PartialEq.

**Const/Static:**
- `SUPPORTED_ARCHIVE_COMPRESSION: &[&str] = &["zstd", "lz4"]` (satır 12) — CLI'dan kabul edilenler
- `DEFAULT_ARCHIVE_COMPRESSION: &str = "zstd"` (satır 13)
- Extension constants: `TAR_BZIP2_EXTENSION = "tar.bz2"`, vb.

**Kritik Fonksiyonlar:**

- `extension()` (satır 33): Format → dosya uzantısı string.

- `from_cli_arg()` (satır 43): `"zstd"` → `TarZstd`, `"lz4"` → `TarLz4`, diğerleri → None. Deprecated formatları CLI'dan kabul ETMEZ.

- `TryFrom<&str> for ArchiveFormat` (satır 54): Extension string → format. Deprecated dahil tüm format'ları parse eder (backward compat için).

- `FromStr` (satır 69): `TryFrom` delegasyonu.

**Test Coverage:** `test_extension()`, `test_try_from()`, `test_from_str()`, `test_from_cli_arg()`. INVALID_EXTENSION = "zip" ile negative test.

**Tasarım Kararları:**
- CLI'dan yalnızca zstd/lz4: Yeni arşivler bunlarla oluşturulur. Eski bzip2/gzip/tar arşivleri hala okunabilir.
- `strum::Display`: Format isimlerini otomatik Display.

**Nova Shield İçin Öğrenilen:**
- İki seviyeli format policy: "Can create" (CLI) vs "Can read" (TryFrom). Backward compat + forward lean.
- Zstd default: En iyi compression ratio + modern streaming support.

---

### [snapshot_utils/snapshot_storage_rebuilder.rs] (517 satır)

**Amaç:** Snapshot arşivinden ya da dizininden AppendVec storage'larının paralel rebuild edilmesi.

**Struct/Enum/Trait Listesi:**

- `RebuiltSnapshotStorage` (satır 44): `snapshot_version: SnapshotVersion, storage: AccountStorageMap`. `pub(crate)`.

- `SnapshotStorageRebuilder` (satır 53): `Debug`. Tüm rebuild state'i tutar:
  - `file_receiver: Receiver<PathBuf>` — crossbeam channel
  - `num_threads: usize`
  - `snapshot_storage_lengths: HashMap<Slot, HashMap<SerializedAppendVecId, usize>>` — beklenen boyutlar
  - `storage_paths: DashMap<Slot, Mutex<Vec<PathBuf>>>` — slot başına gelen dosyalar
  - `storage: AccountStorageMap` — sonuç (DashMap)
  - `next_append_vec_id: Arc<AtomicAppendVecId>`
  - `processed_slot_count: AtomicUsize`
  - `num_collisions: AtomicUsize`
  - `snapshot_from: SnapshotFrom`

- `SnapshotFileKind` (satır 429): Private enum — `Version, BankFields, Storage`. PartialEq+Debug.

**lazy_static! Regex:**
- `VERSION_FILE_REGEX: r"^version$"`
- `BANK_FIELDS_FILE_REGEX: r"^[0-9]+(\.pre)?$"` — slot numarası veya slot.pre
- `STORAGE_FILE_REGEX: r"^(?P<slot>[0-9]+)\.(?P<id>[0-9]+)$"` — slot.id

**Kritik Fonksiyonlar:**

- `SnapshotStorageRebuilder::rebuild_storage()` (satır 76-106): Static entry point:
  1. `get_version_and_snapshot_files()` — version+snapshot file gelene kadar bekle, ara gelen append_vecs buf'a ekle
  2. `snapshot_version_from_file()` + parse
  3. `process_snapshot_file()` — AccountsDbFields'tan storage lengths map al
  4. `spawn_rebuilder_threads()` — paralel rebuild
  5. `RebuiltSnapshotStorage { snapshot_version, storage }` döner

- `SnapshotStorageRebuilder::new()` (satır 111-135): Pre-allocate `storage_paths` per slot.

- `get_version_and_snapshot_files()` (satır 140-180): Channel'dan dosyalar gelirken buffer. `SnapshotFileKind`'a göre route et. Hem version hem bank fields gelince loop kır. Sıra garanti değil (paralel unpack!).

- `process_snapshot_file()` (satır 183-197): `serde_snapshot::fields_from_stream(SerdeStyle::Newer)` → `snapshot_storage_lengths_from_fields()`. Bank fields okur ama bank'ı reconstruct etmez.

- `spawn_rebuilder_threads()` (satır 200-233):
  1. `Arc<SnapshotStorageRebuilder>` oluştur
  2. `build_thread_pool()` (rayon)
  3. Archive ise: önce buffered files'ı `process_buffered_files()` (parallel)
  4. N thread spawn: her biri channel'ı iterate eder
  5. `(exit_sender, exit_receiver)` — thread completion signaling
  6. `wait_for_completion()` — select! loop

- `process_buffered_files()` (satır 236-241): `into_par_iter()` → `process_append_vec_file()`.

- `spawn_receiver_thread()` (satır 244-266): Thread pool'da spawn. Channel iterate. Hata → `exit_sender.send(Err(err))` → early exit. Başarı → `exit_sender.send(Ok(()))`.

- `process_append_vec_file()` (satır 269-289):
  1. Filename parse → `SnapshotFileKind::Storage` mi?
  2. Dir mode ise: `next_append_vec_id.fetch_max(append_vec_id + 1, Ordering::Relaxed)` — max ID track
  3. `insert_storage_file()` → count = len
  4. count == `snapshot_storage_lengths[slot].len()` ise slot complete → `process_complete_slot()`

- `insert_storage_file()` (satır 292-297): `DashMap` get + `Mutex<Vec>` lock + push + len döndür.

- `process_complete_slot()` (satır 300-365): Slot için tüm storage files hazır:
  1. Her file için: `old_append_vec_id` + `current_len` al
  2. Archive ise: `remap_and_reconstruct_single_storage()` → ID remapping + reconstruct
  3. Dir ise: `reconstruct_single_storage()` → direct reconstruct (ID zaten doğru)
  4. Eğer slot için birden fazla storage varsa: `AccountsDb::combine_multiple_slots_into_one_at_startup()` → merge
  5. `storage.insert(slot, AccountStorageReference { id, storage })`

- `get_unique_append_vec_id()` (satır 369-383): Loop: `fetch_add(1, AcqRel)` → `AppendVec::file_name(slot, id)` → dosya var mı? Yoksa bu ID kullan.

- `wait_for_completion()` (satır 386-416): `crossbeam_channel::select!`:
  - `recv(exit_receiver)`: Ok(Ok) → continue, Ok(Err) → return Err, Err (disconnected) → break
  - `default(100ms)`: Her 2 saniyede progress log

- `build_thread_pool()` (satır 419-425): `ThreadPoolBuilder::num_threads(n).thread_name("solRbuildSnap{i:02}")`.

- `get_snapshot_file_kind()` (satır 437-447): Regex match → SnapshotFileKind.

- `get_slot_and_append_vec_id()` (satır 450-461): `STORAGE_FILE_REGEX.captures()` → named groups "slot" + "id" → parse.

**Test Coverage (satır 463-517):**
- `test_get_unique_append_vec_id`: File oluştur → collision → next ID
- `test_get_snapshot_file_kind`: Version/BankFields/Storage detection
- `test_get_slot_and_append_vec_id`: "12345.9987" parse

**Unsafe Bloklar:** Yok.

**Tasarım Kararları:**
1. Channel-based pipeline: Unpack thread → rebuild thread. Unpack ve rebuild paralel çalışır (streaming).
2. Slot completion detection: `len == expected_len` → atomic, no explicit locking needed except for the Mutex<Vec>.
3. Dir vs Archive fark: Archive → ID remap, Dir → ID olduğu gibi kullan.
4. `combine_multiple_slots_into_one_at_startup()`: Bir slot için birden fazla storage → startup'ta tek storage'a merge. Normal çalışmada her slot tek storage.
5. `fetch_max` for Dir mode: Max ID'yi doğrudan güncelle, no need to scan all.
6. 100ms select! timeout: Progress log throttle.

**Nova Shield İçin Öğrenilen:**
- Slot-completion detection: "expected file count" ile "received file count" karşılaştırması — tam gelme beklemek için efficient pattern.
- Channel-based streaming rebuild: I/O ve reconstruction overlap → startup hızlanır.
- `DashMap<Slot, Mutex<Vec<PathBuf>>>`: Per-slot lock granularity — farklı slot'lar parallel.
- Archive: ID remap şart. Dir: ID direkt kullan. Bu iki path ayrı.

---

## Sistem Genelinde Önemli Tasarım Kararları

### 1. PRE/POST İki Aşamalı Bank Snapshot

```
[Bank serialize] → .pre dosyası
                      ↓ (async AHV hesaplar)
[reserialize] → .post dosyası (.pre silinir)
```

Bu pattern, accounts hash hesabının (yavaş) kritik path'ten çıkarılmasını sağlar. Validator blok üretmeye devam ederken hash arka planda hesaplanır.

### 2. Snapshot Pipeline Separasyon

```
AccountsBackgroundService
    → AccountsPackage (queue)
        → AccountsHashVerifier (hash hesapla)
            → SnapshotPackage (queue)
                → SnapshotPackagerService (arşivle)
```

Her aşama ayrı thread ve queue. Backpressure için `enqueued: Instant` monitoring.

### 3. Storage Hard-link Mimarisi

```
accounts/run/slot.id    (canlı dosya)
        ↓ hard_link
accounts/snapshot/slot/slot.id  (snapshot snapshot'ı)
        ↓ symlink
snapshots/slot/accounts_hardlinks/account_path_N → accounts/snapshot/slot/
```

Kopyalama yok. Snapshot silinirken symlink takip edilerek snapshot dizini de silinir.

### 4. Paralel Unarchive Architecture

```
[SharedBuffer] → N × SharedBufferReader → N × Archive
                                              ↓ (parallel)
                              file_sender channel
                                    ↓
              [SnapshotStorageRebuilder threads] × M
                                    ↓
                              AccountStorageMap
```

N = CPU/4 (max 4) unpack thread. M = CPU - N rebuild thread.

### 5. Versiyonlama

Tek aktif versiyon: `V1_2_0`. Eski versiyon kaldırıldı. Format değişikliği → yeni enum varyant + yeni `TypeContext` impl.

Backward compat: `ignore_eof_error` + `default_on_eof` ile yeni field'lar eski snapshot'larla çalışır.

---

## Nova Shield İçin Kapsamlı Eylem Planı

### Snapshot Sistemi Tasarım Temelleri

1. **Format**: Bincode + limit + fixint_encoding + allow_trailing_bytes. Bu üçlü forward/backward compat için şart.

2. **PRE/POST paradigması**: State değişimi async iken tutarlılık:
   - PRE = placeholder hash
   - POST = gerçek hash (AHV sonrası)
   - `state_complete` flag ile two-phase commit

3. **Hard-link storage**: Snapshot'lar için veri kopyalamak yerine inode paylaş. Disk tasarrufu + hız.

4. **Archive format**: `tar.zst` default. Streaming decompress + parallel unpack. Version file'ı archive'da EN BAŞTA.

5. **ID collision handling**: Multi-node snapshot merge için remap_append_vec_file() pattern şart.

6. **Snapshot hash**: `SnapshotHash = SHA256(accounts_hash || epoch_accounts_hash)`. İki bileşen.

7. **Retention policy**: Full N adet, incremental yalnızca en yeni full base için M adet. Diğerleri silinir.

8. **Priority queue**: EAH > Full > Incremental > Verifier.

9. **Slot delta validation**: Status cache corruption'ı iki katman ile yakala: structural + history cross-validate.

10. **Minimal snapshot (minimizer)**: Feature + builtin + rent + vote + stake + owner + programdata — hepsi gereken minimum set.

### Nova Shield Özel Notlar

- `SerializedAppendVecId = usize`: 32-bit sistem sorunu. Nova Shield `u64` kullanmalı.
- `MAX_SNAPSHOT_VERSION_FILE_SIZE = 8 byte`: Trivial ama önemli güvenlik. Nova Shield: version file max 16 byte.
- `should_take_incremental_snapshot`: `last_full_snapshot_slot.is_some()` kontrolü — incremental full olmadan başlamaz.
- `SnapshotUsage::LoadOnly`: Validator'ın sadece startup'ta snapshot kullanmasını sağlar. Nova Shield'da: "bootstrap mode" için.
- `PARALLEL_UNTAR_READERS_DEFAULT = 4`: Pratikte 4'ten fazla paralel okuyucu CPU-I/O bottleneck'te avantaj sağlamıyor.
- `packager_thread_niceness_adj`: Snapshot thread'lerinin CPU önceliğini düşür — consensus kritik thread'leri etkileme.
