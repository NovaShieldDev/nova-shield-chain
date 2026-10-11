# Solana Runtime Analizi — Agent 10
## Dosyalar: bank.rs, bank_forks.rs, accounts_background_service.rs

**Analiz Tarihi:** 2026-07-22
**Toplam Satır:** bank.rs (8003) + bank_forks.rs (1112) + accounts_background_service.rs (1184) = 10299 satır

---

## 1. bank.rs — Solana'nın Kalbi

### 1.1 Modül Dokümantasyonu (satır 1-35)

`bank` modülü, tüm client hesaplarını ve on-chain programların ilerleyişini takip eder.

**Temel yaşam döngüsü:**
1. Bank yeni oluşturulur, işlemlere açıktır.
2. İşlemler uygulanır (leader slot tick count'a veya tüm Entry'ler tükenene kadar).
3. Bank **freeze** edilir: rent uygulanır, sysvar hesapları güncellenir, hash hesaplanır.
4. Yeterli oy aldıktan sonra bank **rooted** olur: zincirden çıkarılamaz, final state.

### 1.2 Import'lar ve Dependency'ler (satır 36-199)

Kritik bağımlılıklar:
- `solana_accounts_db` — AccountsDb, BlockhashQueue, EpochAccountsHash, StakeReward
- `solana_program_runtime` — ProgramCache, InvokeContext, LoadedProgram
- `solana_svm` — TransactionBatchProcessor, TransactionExecutionResult
- `dashmap::DashMap` — concurrent HashMap (lock-free okuma)
- `rayon` — paralel işlem (thread pool)
- Atomic tipler: `AtomicU64`, `AtomicBool`, `AtomicI64`

### 1.3 Yardımcı Struct'lar (satır 200-340)

**`VerifyAccountsHashConfig` (satır 202-208)**
```rust
struct VerifyAccountsHashConfig {
    test_hash_calculation: bool,
    ignore_mismatch: bool,
    require_rooted_bank: bool,
    run_in_background: bool,
    store_hash_raw_data_for_debug: bool,
}
```
Accounts hash doğrulama parametreleri.

**`RentMetrics` (satır 225-233)** — Rent toplama performans metrikleri, tüm alanlar `AtomicU64`/`AtomicUsize`.

**`BankRc` (satır 259-299)** — Bank'ın reference-counted çekirdeği:
```rust
pub struct BankRc {
    pub accounts: Arc<Accounts>,            // Tüm hesaplar
    pub(crate) parent: RwLock<Option<Arc<Bank>>>,  // Parent bank
    pub(crate) slot: Slot,
    pub(crate) bank_id_generator: Arc<AtomicU64>,
}
```
- `parent` bir `RwLock` içinde — squash sonrası `None` yapılır (parent referansı kesilir).
- `bank_id_generator` tüm child bank'lar arasında paylaşılır (`Arc`).

**`BankRc::new()` (satır 291-298):**
```rust
pub(crate) fn new(accounts: Accounts, slot: Slot) -> Self {
    Self {
        accounts: Arc::new(accounts),
        parent: RwLock::new(None),
        slot,
        bank_id_generator: Arc::new(AtomicU64::new(0)),
    }
}
```

**`LoadAndExecuteTransactionsOutput` (satır 301-316)** — İşlem yükleme/çalıştırma çıktısı:
- `loaded_transactions`, `execution_results`, `retryable_transaction_indexes`
- `executed_transactions_count`, `executed_non_vote_transactions_count`
- `executed_with_successful_result_count`, `signature_count`, `error_counters`

**`TransactionSimulationResult` (satır 318-325)** — RPC simülasyonu için:
- `result`, `logs`, `post_simulation_accounts`, `units_consumed`, `return_data`, `inner_instructions`

**`TransactionBalancesSet` (satır 326-340)** — Pre/post balance karşılaştırması.

**`TransactionLogCollectorFilter` enum (satır 342-354):**
```rust
pub enum TransactionLogCollectorFilter {
    All,
    AllWithVotes,
    None,
    OnlyMentionedAddresses,
}
```
Default: `None`.

### 1.4 BankFieldsToDeserialize / BankFieldsToSerialize (satır 398-484)

Snapshot serializasyonu için Bank'ın field'larını yansıtan iki struct. Versiyonlar arası uyumluluk için birbirinden ayrıdır. Önemli notlar:
- Yeni field'lar sonradan eklenmekte; backward compat zorludur.
- `BankFieldsToSerialize<'a>` referans kullanır (clone maliyeti önleme).

**Serializasyon field'larının listesi (her iki struct'ta):**
- `blockhash_queue`, `ancestors`, `hash`, `parent_hash`, `parent_slot`
- `hard_forks`, `transaction_count`, `tick_height`, `signature_count`, `capitalization`
- `max_tick_height`, `hashes_per_tick`, `ticks_per_slot`, `ns_per_slot`
- `genesis_creation_time`, `slots_per_year`, `slot`, `epoch`, `block_height`
- `collector_id`, `collector_fees`, `fee_calculator`, `fee_rate_governor`
- `collected_rent`, `rent_collector`, `epoch_schedule`, `inflation`
- `stakes`, `epoch_stakes`, `is_delta`, `accounts_data_len`
- `incremental_snapshot_persistence`, `epoch_accounts_hash`, `epoch_reward_status`

### 1.5 Bank::PartialEq (satır 487-588)

RwLock `PartialEq` implement etmediği için manuel impl gerekiyor. Karşılaştırma:
- Önce pointer equality (`std::ptr::eq`) — hızlı yol.
- Sonra tüm önemli field'lar tek tek karşılaştırılır.
- `rewards`, `cluster_type`, `feature_set` gibi operasyonel field'lar **kasıtlı olarak atlanır**.

### 1.6 Trait Tanımları (satır 590-620)

**`RewardCalcTracer` trait (satır 598-600):**
```rust
pub trait RewardCalcTracer: Fn(&RewardCalculationEvent) + Send + Sync {}
impl<T: Fn(&RewardCalculationEvent) + Send + Sync> RewardCalcTracer for T {}
```
Closure'ları trait olarak sarma pattern'i — closure type alias olmadığı için.

**`DropCallback` trait (satır 606-609):**
```rust
pub trait DropCallback: fmt::Debug {
    fn callback(&self, b: &Bank);
    fn clone_box(&self) -> Box<dyn DropCallback + Send + Sync>;
}
```
Bank drop edildiğinde (pruned) çağrılır. `clone_box()` pattern — object-safe Box cloning.

### 1.7 EpochRewardStatus Enum (satır 629-640)

```rust
pub(crate) enum EpochRewardStatus {
    Active(StartBlockHeightAndRewards),
    #[default]
    Inactive,
}
```
- `Active`: Epoch ödülü dağıtım aşamasında. `StartBlockHeightAndRewards` içeriyor.
- `Inactive`: Normal çalışma.

**`StartBlockHeightAndRewards` (satır 621-627):**
```rust
pub(crate) struct StartBlockHeightAndRewards {
    pub(crate) start_block_height: u64,
    pub(crate) stake_rewards_by_partition: Arc<Vec<StakeRewards>>,
}
```
Bölümlere ayrılmış stake ödülleri (partitioned epoch rewards).

### 1.8 Bank Struct — Tüm Field'lar (satır 645-816)

```rust
pub struct Bank {
    // --- Temel altyapı ---
    pub rc: BankRc,                                    // Hesaplar, parent referansı
    pub status_cache: Arc<RwLock<BankStatusCache>>,    // İşlem imza durumları (paylaşımlı)

    // --- Blok zinciri kimliği ---
    blockhash_queue: RwLock<BlockhashQueue>,           // FIFO blockhash kuyruğu
    pub ancestors: Ancestors,                          // Bu bank dahil tüm atalar
    hash: RwLock<Hash>,                                // Bank state hash (freeze sonrası anlamlı)
    parent_hash: Hash,
    parent_slot: Slot,
    hard_forks: Arc<RwLock<HardForks>>,                // Hard fork slot'ları

    // --- İşlem sayaçları (atomic) ---
    transaction_count: AtomicU64,                      // Başarısız olmayan işlem sayısı
    non_vote_transaction_count_since_restart: AtomicU64,
    transaction_error_count: AtomicU64,
    transaction_entries_count: AtomicU64,
    transactions_per_entry_max: AtomicU64,

    // --- PoH mekanizması ---
    tick_height: AtomicU64,                            // Mevcut tick yüksekliği
    signature_count: AtomicU64,                        // Slot'taki geçerli imza sayısı
    capitalization: AtomicU64,                         // Toplam token arzı (inflation hesabı için)
    max_tick_height: u64,                              // Slot sonu tick sınırı
    hashes_per_tick: Option<u64>,                      // None = hashing devre dışı
    ticks_per_slot: u64,
    ns_per_slot: u128,                                 // Slot süresi (nanosaniye)
    genesis_creation_time: UnixTimestamp,
    slots_per_year: f64,

    // --- Slot/Epoch bilgisi ---
    slot: Slot,
    bank_id: BankId,                                   // Global benzersiz bank ID (atomic counter)
    epoch: Epoch,
    block_height: u64,

    // --- Ücret mekanizması ---
    collector_id: Pubkey,                              // İşlem ücretleri bu adrese gönderilir
    collector_fees: AtomicU64,                         // Toplanmış ücretler
    pub(crate) fee_rate_governor: FeeRateGovernor,
    collected_rent: AtomicU64,
    rent_collector: RentCollector,

    // --- Epoch ve stake ---
    pub(crate) epoch_schedule: EpochSchedule,
    inflation: Arc<RwLock<Inflation>>,
    stakes_cache: StakesCache,                         // Vote + stake hesap cache
    epoch_stakes: HashMap<Epoch, EpochStakes>,         // Epoch sınırlarında kaydedilen stake

    // --- Durum bayrakları ---
    is_delta: AtomicBool,                              // Bu slot'ta PoH kaydı var mı?
    builtin_programs: HashSet<Pubkey>,
    pub(crate) runtime_config: Arc<RuntimeConfig>,

    // --- Ödüller ---
    pub rewards: RwLock<Vec<(Pubkey, RewardInfo)>>,

    // --- Cluster/Env ---
    pub cluster_type: Option<ClusterType>,
    pub lazy_rent_collection: AtomicBool,
    pub rewards_pool_pubkeys: Arc<HashSet<Pubkey>>,

    // --- Debugging ---
    transaction_debug_keys: Option<Arc<HashSet<Pubkey>>>,
    pub transaction_log_collector_config: Arc<RwLock<TransactionLogCollectorConfig>>,
    pub transaction_log_collector: Arc<RwLock<TransactionLogCollector>>,

    // --- Feature set ---
    pub feature_set: Arc<FeatureSet>,                  // Aktif özellikler

    // --- Yaşam döngüsü ---
    pub drop_callback: RwLock<OptionalDropCallback>,
    pub freeze_started: AtomicBool,
    vote_only_bank: bool,

    // --- Maliyet takibi ---
    cost_tracker: RwLock<CostTracker>,

    // --- Accounts data boyutu ---
    accounts_data_size_initial: u64,
    accounts_data_size_delta_on_chain: AtomicI64,      // İşlemlerden kaynaklı değişim
    accounts_data_size_delta_off_chain: AtomicI64,     // Rent collection'dan değişim

    // --- Hash mekanizması ---
    skipped_rewrites: Mutex<HashMap<Pubkey, AccountHash>>,  // Yeniden yazılmayan hesap hash'leri

    // --- Ücret yapısı ---
    pub fee_structure: FeeStructure,
    pub incremental_snapshot_persistence: Option<BankIncrementalSnapshotPersistence>,

    // --- Program runtime ---
    program_cache: Arc<RwLock<ProgramCache<BankForks>>>,

    // --- Epoch ödül durumu ---
    epoch_reward_status: EpochRewardStatus,

    // --- SVM işlemci ---
    transaction_processor: TransactionBatchProcessor<BankForks>,

    check_program_modification_slot: bool,
}
```

**Tasarım notları:**
- `Arc<RwLock<...>>` paylaşımlı mutable state için (program_cache, inflation, status_cache).
- `AtomicU64/AtomicBool` lock-free sayaçlar için.
- `Mutex` daha az kullanılıyor (`skipped_rewrites`).
- `DashMap` ile concurrent read-heavy yapılar (reward calculation).

### 1.9 Yardımcı Yapılar — Reward Hesabı (satır 818-934)

**`VoteWithStakeDelegations` (satır 818-832):**
```rust
struct VoteWithStakeDelegations {
    vote_state: Arc<VoteState>,
    vote_account: AccountSharedData,
    delegations: Vec<(Pubkey, StakeAccount<Delegation>)>,
}
type VoteWithStakeDelegationsMap = DashMap<Pubkey, VoteWithStakeDelegations>;
```
Paralel reward hesaplaması için `DashMap` kullanılıyor.

**`EpochRewardCalculateParamInfo` (satır 854-859):**
```rust
struct EpochRewardCalculateParamInfo<'a> {
    stake_history: StakeHistory,
    stake_delegations: Vec<(&'a Pubkey, &'a StakeAccount<Delegation>)>,
    cached_vote_accounts: &'a VoteAccounts,
}
```
Hesaplama parametrelerini bir arada tutan yapı.

**`PartitionedRewardsCalculation` (satır 862-873)** — Bölümlü ödül dağıtımı sonuçları:
```rust
struct PartitionedRewardsCalculation {
    vote_account_rewards: VoteRewardsAccounts,
    stake_rewards_by_partition: StakeRewardCalculationPartitioned,
    old_vote_balance_and_staked: u64,
    validator_rewards: u64,
    validator_rate: f64,
    foundation_rate: f64,
    prev_epoch_duration_in_years: f64,
    capitalization: u64,
}
```

**`RewardInterval` enum (satır 928-934):**
```rust
pub(super) enum RewardInterval {
    InsideInterval,   // Ödül dağıtım penceresi içinde
    OutsideInterval,  // Normal işlem penceresi
}
```
Bu değer `check_account_access()` içinde kullanılıyor — ödül dağıtımı sırasında stake hesaplarına yazma engelleniyor!

### 1.10 Bank::new() — default_with_accounts (satır 937-1018)

```rust
fn default_with_accounts(accounts: Accounts) -> Self {
    let mut bank = Self {
        // ... tüm field'lar default değerle başlatılır ...
        program_cache: Arc::new(RwLock::new(ProgramCache::new(Slot::default(), Epoch::default()))),
        // ...
    };

    bank.transaction_processor = TransactionBatchProcessor::new(
        bank.slot, bank.epoch, bank.epoch_schedule.clone(),
        bank.fee_structure.clone(), bank.runtime_config.clone(),
        bank.program_cache.clone(),
    );

    let accounts_data_size_initial = bank.get_total_accounts_stats().unwrap().data_len as u64;
    bank.accounts_data_size_initial = accounts_data_size_initial;
    bank
}
```
`TransactionBatchProcessor` bank oluştuktan sonra ayrıca set ediliyor (borrow checker gereği).

### 1.11 Bank::new_with_paths() — Genesis'ten Bank (satır 1021-1080)

```rust
pub fn new_with_paths(
    genesis_config: &GenesisConfig,
    runtime_config: Arc<RuntimeConfig>,
    paths: Vec<PathBuf>,          // Disk path'leri
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_indexes: AccountSecondaryIndexes,
    shrink_ratio: AccountShrinkThreshold,
    debug_do_not_add_builtins: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    collector_id_for_tests: Option<Pubkey>,  // sadece test build'lerinde
    exit: Arc<AtomicBool>,
) -> Self
```

**Adımlar:**
1. `AccountsDb::new_with_config()` — disk storage oluştur.
2. `Accounts::new(Arc::new(accounts_db))` — in-memory layer.
3. `Bank::default_with_accounts(accounts)` — bank çekirdeği.
4. `bank.process_genesis_config(genesis_config)` — genesis hesaplarını yükle.
5. `bank.finish_init(...)` — builtin program'ları ekle.
6. Genesis stake'leri epoch_stakes'e ekle.
7. Sysvar'ları güncelle: `update_stake_history`, `update_clock`, `update_rent`, `update_epoch_schedule`, `update_recent_blockhashes`, `update_last_restart_slot`, `fill_missing_sysvar_cache_entries`.

### 1.12 Bank::new_from_parent() (satır 1083-1114)

Public API — null_tracer ile `_new_from_parent` çağrılır.

```rust
pub fn new_from_parent(parent: Arc<Bank>, collector_id: &Pubkey, slot: Slot) -> Self {
    Self::_new_from_parent(parent, collector_id, slot, null_tracer(), NewBankOptions::default())
}
```

**Varyantlar:**
- `new_from_parent_with_options` — `vote_only_bank` bayrağı için.
- `new_from_parent_with_tracer` — reward debugging için.

### 1.13 Bank::_new_from_parent() — Kritik Fonksiyon (satır 1191-1474)

Bu fonksiyon yeni bir bank oluşturmada tüm ağır işi yapar. Her adım `measure_us!` makrosuyla zamanlıdır.

**Adım 1: Parent'ı dondur ve yeni BankRc oluştur (satır 1201-1216):**
```rust
parent.freeze();  // Parent freeze edilmeden child oluşturulamaz!
assert_ne!(slot, parent.slot());

let (rc, bank_rc_creation_time_us) = measure_us!({
    let accounts_db = Arc::clone(&parent.rc.accounts.accounts_db);  // AccountsDb paylaşılıyor!
    accounts_db.insert_default_bank_hash_stats(slot, parent.slot());
    BankRc {
        accounts: Arc::new(Accounts::new(accounts_db)),
        parent: RwLock::new(Some(Arc::clone(&parent))),
        slot,
        bank_id_generator: Arc::clone(&parent.rc.bank_id_generator),
    }
});
```
**Önemli:** `AccountsDb` Arc ile paylaşılıyor — kopyalanmıyor, sadece referans alınıyor.

**Adım 2: State kopyalama (satır 1218-1322):**
- `status_cache` — `Arc::clone` (paylaşımlı)
- `fee_rate_governor` — yeni türetilmiş: `FeeRateGovernor::new_derived(&parent.fee_rate_governor, parent.signature_count())`
- `bank_id` — `rc.bank_id_generator.fetch_add(1, Relaxed) + 1`
- `blockhash_queue` — clone (not shared)
- `stakes_cache`, `epoch_stakes`, `builtin_programs` — clone
- `feature_set` — `Arc::clone` (shared)
- `drop_callback` — `clone_box()` ile kopyalanır

**Yeni Bank struct'ında dikkat:**
- `block_height: parent.block_height + 1`
- `max_tick_height: (slot + 1) * parent.ticks_per_slot`
- `transaction_count`: parent'ın sayısından devam eder
- `signature_count`: sıfırdan başlar
- `collected_rent`: sıfırdan başlar
- `collector_fees`: sıfırdan başlar

**Adım 3: TransactionProcessor (satır 1324-1331):**
```rust
new.transaction_processor = TransactionBatchProcessor::new(
    new.slot, new.epoch, new.epoch_schedule.clone(),
    new.fee_structure.clone(), new.runtime_config.clone(),
    new.program_cache.clone(),
);
```

**Adım 4: Ancestors (satır 1333-1340):**
Yeni bank'ın slot'u + tüm parent slot'ları ancestor listesine eklenir.

**Adım 5: Epoch değişimi kontrolü (satır 1343-1359):**
```rust
if parent.epoch() < new.epoch() {
    new.process_new_epoch(parent.epoch(), parent.slot(), parent.block_height(), reward_calc_tracer);
} else {
    let leader_schedule_epoch = new.epoch_schedule().get_leader_schedule_epoch(slot);
    new.update_epoch_stakes(leader_schedule_epoch);
}
if new.is_partitioned_rewards_code_enabled() {
    new.distribute_partitioned_epoch_rewards();
}
```

**Adım 6: Program recompilation (satır 1361-1425):**
Eğer `upcoming_environments` varsa, bir program yeniden derlenir. Eğer epoch değişimindeyse veya epoch'un son yarısındaysak, yeni runtime environment hazırlanır ve programlar recompilation kuyruğuna alınır.

**Adım 7: Sysvar güncellemeleri (satır 1428-1434):**
```rust
new.update_slot_hashes();
new.update_stake_history(Some(parent.epoch()));
new.update_clock(Some(parent.epoch()));
new.update_fees();
new.update_last_restart_slot();
```

**Adım 8: Sysvar cache doldurma + metrik raporlama.**

### 1.14 process_new_epoch() — Epoch Geçiş Mekanizması (satır 1513-1593)

```rust
fn process_new_epoch(
    &mut self,
    parent_epoch: Epoch,
    parent_slot: Slot,
    parent_height: u64,
    reward_calc_tracer: Option<impl RewardCalcTracer>,
)
```

**Adımlar:**
1. Rayon thread pool oluştur (her epoch değişiminde yeni pool).
2. `apply_feature_activations(NewFromParent, false)` — yeni feature'ları aktive et.
3. `stakes_cache.activate_epoch(epoch, &thread_pool, new_warmup_cooldown_rate_epoch)` — stake history güncelle, vote accounts'ları ısıt.
4. `update_epoch_stakes(leader_schedule_epoch)` — consensus için stake snapshot'ı kaydet.
5. Ödül dağıtımı:
   - Partitioned rewards aktifse: `begin_partitioned_rewards(...)` — çok bloklu dağıtım.
   - Değilse: `update_rewards_with_thread_pool(...)` — tek blokta dağıtım.

### 1.15 Partitioned Epoch Rewards Mekanizması (satır 1121-1698)

**Temel fikir:** Büyük epoch'larda on binlerce stake hesabına tek blokta ödül dağıtımı çok yavaş. Bu nedenle ödüller N bloğa bölünür.

**`get_reward_distribution_num_blocks()` (satır 1158-1175):**
- Warmup epoch'larında: 1 blok.
- Normal epoch'larda: `ceil(total_stake_accounts / stores_per_block)`.
- Sınır: epoch slot sayısının %10'u.

**`begin_partitioned_rewards()` (satır 1625-1663):**
1. `calculate_rewards_and_distribute_vote_rewards()` — vote ödüllerini anında dağıt, stake ödüllerini partition'la.
2. `set_epoch_reward_status_active(stake_rewards_by_partition)` — durumu aktif yap.
3. `create_epoch_rewards_sysvar(total_rewards, distributed_rewards, credit_end_exclusive)` — sysvar'ı güncelle.

**`distribute_partitioned_epoch_rewards()` (satır 1666-1698):**
Her blokta çağrılır. Eğer `height >= credit_start && height < credit_end_exclusive` ise o partition'ı dağıt. Son partition bitince `deactivate_epoch_reward_status()`.

**`deactivate_epoch_reward_status()` (satır 1597-1614):**
EpochRewards sysvar'ında kalan lamport varsa burn edilir (hassas hesaplama hataları için koruma).

### 1.16 freeze() — Bank Dondurma (satır 3701-3727)

```rust
pub fn freeze(&self) {
    let mut hash = self.hash.write().unwrap();
    if *hash == Hash::default() {
        // Idempotent — sadece bir kez çalışır
        self.collect_rent_eagerly();         // Rent topla
        self.distribute_transaction_fees();  // Ücretleri dağıt
        self.distribute_rent_fees();         // Rent ücretlerini dağıt
        self.update_slot_history();          // SlotHistory sysvar'ı güncelle
        self.run_incinerator();              // Incinerator hesabını temizle

        self.freeze_started.store(true, Relaxed);
        *hash = self.hash_internal_state();  // Deterministic hash hesapla
        self.rc.accounts.accounts_db.mark_slot_frozen(self.slot());
    }
}
```

**Tasarım:** `hash` write lock'u banking stage'i engeller — son tick gelene kadar hiçbir commit yapılamaz. Bu sayede freeze güvenli.

**`run_incinerator()` (satır 5056-5063):** Incinerator adresine gönderilen token'lar yakılır (capitalization düşer).

### 1.17 squash() — Bank Root Yapma (satır 3743-3780)

```rust
pub fn squash(&self) -> SquashTiming {
    self.freeze();
    let mut roots = vec![self.slot()];
    roots.append(&mut self.parents().iter().map(|p| p.slot()).collect());

    for slot in roots.iter().rev() {
        let add_root_timing = self.rc.accounts.add_root(*slot);
        // ...
    }

    *self.rc.parent.write().unwrap() = None;  // Parent referansı kes!

    roots.iter().for_each(|slot| self.status_cache.write().unwrap().add_root(*slot));
}
```
Parent referansı sıfırlanıyor — bellek serbest bırakılıyor.

### 1.18 process_genesis_config() (satır 3795-3862)

Genesis hesapları yüklenir, capitalization güncellenir. `collector_id` en yüksek stake'li node'dan belirlenir. İlk blockhash genesis config'in hash'inden oluşturulur.

### 1.19 register_tick() ve Blockhash (satır 4175-4191)

```rust
pub fn register_tick(&self, hash: &Hash, scheduler: &InstalledSchedulerRwLock) {
    assert!(!self.freeze_started(), "...");
    if self.is_block_boundary(self.tick_height.load(Relaxed) + 1) {
        self.register_recent_blockhash(hash, scheduler);
    }
    self.tick_height.fetch_add(1, Relaxed);
}
```

`register_recent_blockhash()` (satır 4147-4158): Blokhash kuyruğuna yeni hash ekler, recent blockhashes sysvar'ını günceller. Önemli: Scheduler bekleniyor (`wait_for_paused_scheduler`) — tutarlı tx age check için senkronizasyon.

### 1.20 load_and_execute_transactions() (satır 4582-4783)

Bu, production işlem pipeline'ının ana fonksiyonu:

```rust
pub fn load_and_execute_transactions(
    &self,
    batch: &TransactionBatch,
    max_age: usize,
    recording_config: ExecutionRecordingConfig,
    timings: &mut ExecuteTimings,
    account_overrides: Option<&AccountOverrides>,
    log_messages_bytes_limit: Option<usize>,
    limit_to_load_programs: bool,
) -> LoadAndExecuteTransactionsOutput
```

**Adımlar:**
1. `lock_results` içindeki retryable/non-retryable hataları sınıflandır.
2. `check_transactions()` — yaş ve status cache kontrolü.
3. `transaction_processor.load_and_execute_sanitized_transactions()` — SVM'de çalıştır.
4. Log toplama — `transaction_log_collector_config` filtrelerine göre.
5. `signature_count`, `executed_transactions_count` hesapla.
6. `LoadAndExecuteTransactionsOutput` döndür.

**Retryable hatalar:**
- `AccountInUse`, `WouldExceedMaxBlockCostLimit`, `WouldExceedMaxVoteCostLimit`
- `WouldExceedMaxAccountCostLimit`, `WouldExceedAccountDataBlockLimit`

**Non-retryable:** `TooManyAccountLocks`, diğer tüm hatalar.

### 1.21 commit_transactions() (satır 4910-5030)

```rust
pub fn commit_transactions(
    &self,
    sanitized_txs: &[SanitizedTransaction],
    loaded_txs: &mut [TransactionLoadResult],
    execution_results: Vec<TransactionExecutionResult>,
    last_blockhash: Hash,
    lamports_per_signature: u64,
    counts: CommitTransactionCounts,
    timings: &mut ExecuteTimings,
) -> TransactionResults
```

**Adımlar:**
1. Sayaçları güncelle (`transaction_count`, `signature_count`, `transaction_error_count`).
2. `is_delta` bayrağını set et (bu slot'ta işlem var).
3. `rc.accounts.store_cached()` — hesapları yaz.
4. `collect_rent()` — başarılı işlemlerde rent topla.
5. `update_stakes_cache()` — vote/stake hesap değişikliklerini cache'e yansıt.
6. `program_cache.merge()` — yeni deploy edilen programları cache'e ekle.
7. Accounts data size delta'yı güncelle.
8. `update_transaction_statuses()` — status cache'e ekle.
9. `filter_program_errors_and_collect_fee()` — başarısız işlemlerde ücret çek.

### 1.22 Rent Collection Mekanizması (satır 5032-5497)

**`collect_rent()` (satır 5032-5054):** Başarılı işlemlerden rent_debits alınır, `collected_rent` atomically güncellenir.

**`collect_rent_eagerly()` (satır 5148-5220):** freeze() sırasında çağrılır. Partition listesi alınır. Eğer birden fazla partition varsa ve range'ler örtüşmüyorsa paralel çalıştırılır.

**`collect_rent_in_range()` (satır 5403-5497):** Çok adımlı paralel rent toplama:
1. Range'i memory'de tut (`hold_range_in_memory`).
2. Rayon thread pool ile range'i `num_threads` parçaya böl, paralel işle.
3. `collect_rent_from_accounts()` — her chunk için.
4. `CollectRentInPartitionInfo::reduce()` — sonuçları birleştir.
5. `skipped_rewrites` güncelle.
6. `collected_rent`, `accounts_data_size_delta_off_chain`, `rewards` güncelle.

**`collect_rent_from_accounts()` (satır 5267-5370):**
- `bank_hash_skips_rent_rewrites` feature aktifse: sadece rent borçlu hesapları yaz.
- Değilse: tüm incelenen hesapları yaz (bank hash doğrulaması için).
- `skip_rewrite()`: rent amount 0 ve rent exempt ise rewrite atlana bilir.

### 1.23 Feature Activation Mekanizması (satır 7212-7373)

**`apply_feature_activations()` (satır 7212-7283):**
```rust
fn apply_feature_activations(
    &mut self,
    caller: ApplyFeatureActivationsCaller,
    debug_do_not_add_builtins: bool,
)
```

`caller` enum'u üç durum:
- `FinishInit` — yeni activation yok
- `NewFromParent` — yeni activation'lara izin verilir
- `WarpFromParent` — yeni activation yok

**`compute_active_feature_set()` (satır 7303-7335):**
1. Tüm inactive feature'ları tara.
2. Her biri için on-chain account'a bak.
3. `activated_at = None` ve `include_pending = true` ise aktivasyon beklemede → pending olarak işaretle.
4. `activated_at = Some(slot)` ve `slot >= activation_slot` ise zaten aktif.
5. Yeni `FeatureSet { active, inactive }` döndür.

**Feature aktivasyonu ne tetikler:**
- `pico_inflation` → `Inflation::pico()`, %50 fee burn, %50 rent burn.
- `full_inflation_features_enabled` → `Inflation::full()`.
- `update_hashes_per_tick[1-6]` → `hashes_per_tick` güncellenir.
- Builtin program feature geçişleri → `add_builtin()` / `remove_builtin()`.

**`apply_builtin_program_feature_transitions()` (satır 7337-7373):**
BUILTINS listesini dolaşır. Eğer bir builtin'in feature_id'si yeni aktive olduysa `add_builtin()` çağrılır.

### 1.24 TransactionProcessingCallback impl (satır 7534-7594)

Bank, SVM'nin `TransactionProcessingCallback` trait'ini implemente eder:

```rust
impl TransactionProcessingCallback for Bank {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize>
    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData>
    fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64)
    fn get_rent_collector(&self) -> &RentCollector
    fn get_feature_set(&self) -> Arc<FeatureSet>
    fn check_account_access(&self, message, account_index, account, error_counters) -> Result<()>
    fn get_program_match_criteria(&self, program: &Pubkey) -> LoadedProgramMatchCriteria
}
```

**`check_account_access()` (satır 7563-7581):**
```rust
if self.get_reward_interval() == RewardInterval::InsideInterval
    && message.is_writable(account_index)
    && solana_stake_program::check_id(account.owner())
{
    error_counters.program_execution_temporarily_restricted += 1;
    Err(TransactionError::ProgramExecutionTemporarilyRestricted { account_index: account_index as u8 })
}
```
**Kritik:** Ödül dağıtımı sırasında stake hesaplarına yazma engeli.

### 1.25 check_transactions() (satır 4558-4567)

```rust
pub fn check_transactions(
    &self,
    sanitized_txs: &[impl core::borrow::Borrow<SanitizedTransaction>],
    lock_results: &[Result<()>],
    max_age: usize,
    error_counters: &mut TransactionErrorMetrics,
) -> Vec<TransactionCheckResult> {
    let lock_results = self.check_age(sanitized_txs, lock_results, max_age, error_counters);
    self.check_status_cache(sanitized_txs, lock_results, error_counters)
}
```

İki aşamalı kontrol: yaş kontrolü + status cache (tekrar işleme engeli).

### 1.26 update_stakes_cache() (satır 6946-6968)

Başarılı işlemlerden etkilenen hesaplar `stakes_cache.check_and_store()` ile güncellenir. `izip!` macro'su ile zip işlemi.

### 1.27 Fee Hesaplama (satır 4053-4069)

```rust
pub fn get_fee_for_message_with_lamports_per_signature(&self, message, lamports_per_signature) -> u64 {
    self.fee_structure.calculate_fee(
        message,
        lamports_per_signature,
        &process_compute_budget_instructions(message.program_instructions_iter())
            .unwrap_or_default().into(),
        self.feature_set.is_active(&include_loaded_accounts_data_size_in_fee_calculation::id()),
        self.feature_set.is_active(&remove_rounding_in_fee_calculation::id()),
    )
}
```

Compute budget instruction'lardan türetilen fee — feature gated.

### 1.28 filter_program_errors_and_collect_fee() (satır 4850-4904)

Başarısız işlemlerde bile (nonce hariç) ücret alınır:
```rust
if execution_status.is_err() && !is_nonce {
    self.withdraw(tx.message().fee_payer(), fee)?;
}
fees += fee;
```
`collector_fees.fetch_add(fees, Relaxed)` ile biriktirilir.

### 1.29 Accounts Data Size Tracking (satır 4785-4848)

İki delta takip edilir:
- `accounts_data_size_delta_on_chain` (AtomicI64) — başarılı işlemlerden
- `accounts_data_size_delta_off_chain` (AtomicI64) — rent collection'dan

`fetch_update(AcqRel, Acquire, ...)` — saturating add, thread-safe.

`load_accounts_data_size()` = `accounts_data_size_initial.saturating_add_signed(delta)`

### 1.30 Snapshot Hash (satır 6663-6707)

```rust
pub fn get_snapshot_hash(&self) -> SnapshotHash {
    // Full snapshot VEYA incremental (ikisi aynı anda olamaz)
    let accounts_hash = match (accounts_hash, incremental_accounts_hash) {
        (Some(_), Some(_)) => panic!("..."),
        (Some(h), None) => h.into(),
        (None, Some(h)) => h.into(),
        (None, None) => panic!("accounts hash is required"),
    };
    let epoch_accounts_hash = self.get_epoch_accounts_hash_to_serialize();
    SnapshotHash::new(&accounts_hash, epoch_accounts_hash.as_ref())
}
```

### 1.31 Unsafe Bloklar

**Hiç `unsafe` blok yok** `bank.rs` içinde. Tüm concurrency, Rust'ın safe abstraction'ları (Arc, RwLock, Mutex, Atomic) ile yönetiliyor.

---

## 2. bank_forks.rs — Fork Yönetimi (1112 satır)

### 2.1 Sabitler ve Tipler (satır 31-44)

```rust
pub const MAX_ROOT_DISTANCE_FOR_VOTE_ONLY: Slot = 400;
pub type AtomicSlot = AtomicU64;

pub struct ReadOnlyAtomicSlot {
    slot: Arc<AtomicSlot>,
}
impl ReadOnlyAtomicSlot {
    pub fn get(&self) -> Slot {
        self.slot.load(Ordering::Acquire)  // Acquire semantics — lock-free cross-thread
    }
}
```

`ReadOnlyAtomicSlot`: Farklı thread'den lock almadan root slot'u okuma. `Acquire` ordering — `BankForks::set_root()`'daki `Release` ile eşleşir.

### 2.2 Metrik Yapıları (satır 46-63)

```rust
struct SetRootMetrics {
    timings: SetRootTimings,
    total_parent_banks: i64,
    tx_count: i64,
    dropped_banks_len: i64,
    accounts_data_len: i64,
}

struct SetRootTimings {
    total_squash_time: SquashTiming,
    total_snapshot_ms: i64,
    prune_non_rooted_ms: i64,
    drop_parent_banks_ms: i64,
    prune_slots_ms: i64,
    prune_remove_ms: i64,
}
```

### 2.3 BankForks Struct (satır 65-77)

```rust
pub struct BankForks {
    banks: HashMap<Slot, BankWithScheduler>,      // Tüm aktif bank'lar
    descendants: HashMap<Slot, HashSet<Slot>>,    // Torun ilişkileri
    root: Arc<AtomicSlot>,                        // Mevcut root slot

    pub snapshot_config: Option<SnapshotConfig>,
    pub accounts_hash_interval_slots: Slot,
    last_accounts_hash_slot: Slot,
    in_vote_only_mode: Arc<AtomicBool>,
    highest_slot_at_startup: Slot,
    scheduler_pool: Option<InstalledSchedulerPoolArc>,
}
```

**Not:** `banks` ve `descendants` ayrı tutulur — birinden diğerine hızlı geçiş sağlar.

### 2.4 BankForks::new_rw_arc() (satır 87-131)

```rust
pub fn new_rw_arc(root_bank: Bank) -> Arc<RwLock<Self>> {
    let root_bank = Arc::new(root_bank);
    let root_slot = root_bank.slot();

    // Tüm parent'ları da banks'a ekle
    let parents = root_bank.parents();
    for parent in parents {
        if banks.insert(...).is_some() { break; }  // Önceki fork zaten eklemiş
    }

    // descendants hesapla
    descendants.entry(root_slot).or_default();
    for parent in root_bank.proper_ancestors() {
        descendants.entry(parent).or_default().insert(root_slot);
    }

    let bank_forks = Arc::new(RwLock::new(Self { ... }));
    root_bank.set_fork_graph_in_program_cache(bank_forks.clone());  // Geri referans!
    bank_forks
}
```

**Önemli:** `program_cache` içindeki `ForkGraph`, `BankForks`'a back-reference tutar.

### 2.5 BankForks::insert() (satır 218-239)

```rust
pub fn insert(&mut self, mut bank: Bank) -> BankWithScheduler {
    if self.root.load(Ordering::Relaxed) < self.highest_slot_at_startup {
        bank.check_program_modification_slot();  // Startup sonrası program değişim kontrolü
    }

    let bank = Arc::new(bank);
    let bank = if let Some(scheduler_pool) = &self.scheduler_pool {
        let context = SchedulingContext::new(bank.clone());
        let scheduler = scheduler_pool.take_scheduler(context);
        BankWithScheduler::new(bank, Some(scheduler))
    } else {
        BankWithScheduler::new_without_scheduler(bank)
    };

    let prev = self.banks.insert(bank.slot(), bank.clone_with_scheduler());
    assert!(prev.is_none());  // Aynı slot iki kez eklenemez!
    // descendants güncelle...
}
```

### 2.6 BankForks::remove() (satır 246-264)

Slot kaldırılır, descendants map'i güncellenir. Eğer bir slot'un descendant'ı kalmadı ve banks'ta yoksa, descendants'tan da kaldırılır.

### 2.7 do_set_root_return_metrics() (satır 278-445) — Core Logic

```rust
fn do_set_root_return_metrics(
    &mut self,
    root: Slot,
    accounts_background_request_sender: &AbsRequestSender,
    highest_super_majority_root: Option<Slot>,
) -> (Vec<Arc<Bank>>, SetRootMetrics)
```

**Adımlar:**
1. `self.root.store(root, Ordering::Release)` — root'u atomik güncelle (**Release** ordering).
2. Epoch değişimini logla.
3. EpochAccountsHash gerekli mi? `should_request_epoch_accounts_hash()`.
   - Eğer evet: squash + ABS'e `EpochAccountsHash` request gönder.
4. Regular snapshot gerekli mi? (`accounts_hash_interval_slots` modulo kontrolü).
   - Eğer evet: squash + ABS'e `Snapshot` request gönder.
5. Root bank'ı squash et (eğer yukarıda olmadıysa).
6. `prune_non_rooted()` — root olmayan bank'ları temizle.

### 2.8 set_root() (satır 453-552)

`do_set_root_return_metrics()` çağrıları + kapsamlı `datapoint_info!` metrik çıktısı.

### 2.9 prune_non_rooted() (satır 615-653) — Detaylı Dokümantasyonlu

ASCII tree diagram ile belgelenmiş:
```
slot 6                   * (G)
                        /
slot 5        (F)  *   /
                   |  /
slot 4    (E) *    | /
              |    |/
slot 3        |    * (D) <-- root
              |    |
slot 2    (C) *    |
               \   |
slot 1          \  * (B)
                 \ |
slot 0             * (A) <-- highest confirmed root
```

**Pruning kuralları:**
- Root olan slot: Kalır.
- Root'un descendant'ı: Kalır.
- Root'un ancestor'ı **VE** `slot >= highest_super_majority_root`: Kalır (RPC commitment için).
- Diğerleri: Silinir.

### 2.10 ForkGraph impl (satır 677-704)

`BankForks`, `ForkGraph` trait'ini implemente eder (program cache için):

```rust
impl ForkGraph for BankForks {
    fn relationship(&self, a: Slot, b: Slot) -> BlockRelation {
        // Equal, Ancestor, Descendant, Unrelated, Unknown
    }
    fn slot_epoch(&self, slot: Slot) -> Option<Epoch> { ... }
}
```

### 2.11 Testler (satır 706-1112)

Test coverage'ı:
- `test_bank_forks_new` — temel oluşturma
- `test_bank_forks_descendants` — torun ilişkileri
- `test_bank_forks_children` — çocuk bank'lar
- `test_bank_forks_frozen_banks` — dondurulmuş bank filtresi
- `test_bank_forks_active_banks` — aktif bank listesi
- Geniş kapsamlı `set_root`, prune, snapshot testleri

---

## 3. accounts_background_service.rs — Arka Plan Servis (1184 satır)

### 3.1 Sabitler (satır 39-50)

```rust
const INTERVAL_MS: u64 = 100;              // Ana loop 100ms'de bir çalışır
const CLEAN_INTERVAL_BLOCKS: u64 = 100;   // ~100 blokta bir temizlik
const BANK_DROP_SIGNAL_CHANNEL_REPORT_INTERVAL: u64 = 60_000;  // 60s
const MAX_DROP_BANK_SIGNAL_QUEUE_SIZE: usize = 10_000;
```

### 3.2 Tip Tanımları (satır 42-45)

```rust
pub type SnapshotRequestSender = Sender<SnapshotRequest>;
pub type SnapshotRequestReceiver = Receiver<SnapshotRequest>;
pub type DroppedSlotsSender = Sender<(Slot, BankId)>;
pub type DroppedSlotsReceiver = Receiver<(Slot, BankId)>;
```

`crossbeam_channel` kullanılıyor — Rust'ın standart `mpsc`'den daha hızlı.

### 3.3 PrunedBankQueueLenReporter (satır 52-68)

Prune kuyruğu çok uzarsa uyarı datapoint'i gönderilir (60 saniyede bir limit).

### 3.4 SendDroppedBankCallback (satır 75-103)

```rust
pub struct SendDroppedBankCallback {
    sender: DroppedSlotsSender,
}

impl DropCallback for SendDroppedBankCallback {
    fn callback(&self, bank: &Bank) {
        BANK_DROP_QUEUE_REPORTER.report(self.sender.len());
        if let Err(SendError(_)) = self.sender.send((bank.slot(), bank.bank_id())) {
            info!("bank DropCallback signal queue disconnected.");
        }
    }
    fn clone_box(&self) -> Box<dyn DropCallback + Send + Sync> {
        Box::new(self.clone())
    }
}
```

Bank drop edildiğinde ABS'e signal gönderilir. Kanal kopuksa sessizce devam.

### 3.5 SnapshotRequest (satır 105-122)

```rust
pub struct SnapshotRequest {
    pub snapshot_root_bank: Arc<Bank>,
    pub status_cache_slot_deltas: Vec<BankSlotDelta>,
    pub request_kind: SnapshotRequestKind,
    pub enqueued: Instant,  // Kuyruk bekleme süresi ölçümü için
}

pub enum SnapshotRequestKind {
    Snapshot,
    EpochAccountsHash,
}
```

### 3.6 SnapshotRequestHandler (satır 135-463)

#### handle_snapshot_requests() (satır 145-178)

En son snapshot isteğini alır, işler. Önce `get_next_snapshot_request()` ile en yüksek öncelikli seçilir.

#### get_next_snapshot_request() (satır 189-289) — Priority Queue Logic

Öncelik sırası (yüksekten düşüğe):
1. EpochAccountsHash
2. FullSnapshot
3. IncrementalSnapshot
4. AccountsHashVerifier

**Özel durum (satır 252-265):** Eğer en yüksek öncelikli EAH ise VE ikinci yüksek FullSnapshot ise VE FullSnapshot daha eski slot'taysa, önce FullSnapshot işlenir. (Aksi takdirde incremental snapshot bozulur.)

Daha yüksek slot'taki işlenmemiş requestler yeniden kuyruğa alınır. Düşük slot'takiler düşürülür.

#### handle_snapshot_request() (satır 291-463)

```rust
fn handle_snapshot_request(
    &self, test_hash_calculation, non_snapshot_time_us,
    last_full_snapshot_slot, snapshot_request, accounts_package_kind, exit,
) -> Result<u64, SnapshotError>
```

**Adımlar:**
1. `snapshot_root_bank.is_startup_verification_complete()` assert et.
2. Opsiyonel: önceki hash'i hesapla (test için).
3. `force_flush_accounts_cache()` — cache'i diske at.
4. `clean_accounts()` — eski/dead account'ları temizle.
5. `shrink_ancient_slots()` + `shrink_candidate_slots()` — storage küçültme.
6. `get_snapshot_storages()` — storage listesini al.
7. Request türüne göre `AccountsPackage` oluştur:
   - `Snapshot → FullSnapshot/IncrementalSnapshot` → bank snapshot dosyası oluştur.
   - `Snapshot → AccountsHashVerifier` → sadece package gönder.
   - `EpochAccountsHash` → EAH package.
8. `accounts_package_sender.send(accounts_package)` — işleyiciye gönder.

### 3.7 AbsRequestSender (satır 466-492)

```rust
pub struct AbsRequestSender {
    snapshot_request_sender: Option<SnapshotRequestSender>,
}
```

`None` ise snapshot creation devre dışı. `is_snapshot_creation_enabled()` ile kontrol.

### 3.8 PrunedBanksRequestHandler (satır 494-558)

```rust
fn handle_request(&self, bank: &Bank) -> usize {
    let mut banks_to_purge: Vec<_> = self.pruned_banks_receiver.try_iter().collect();
    banks_to_purge.sort_by_key(|(slot, _id)| *slot);  // Stable sort — sıra önemli!

    let grouped_banks_to_purge = GroupBy::new(banks_to_purge.as_slice(), |a, b| a.0 == b.0);

    let accounts_db = bank.rc.accounts.accounts_db.as_ref();
    accounts_db.thread_pool_clean.install(|| {
        grouped_banks_to_purge.into_par_iter().for_each(|group| {
            group.iter().for_each(|(slot, bank_id)| {
                accounts_db.purge_slot(*slot, *bank_id, true);
            })
        });
    });
}
```

Aynı slot'taki bank'lar sıralı temizlenir (aralarındaki sıra önemli). Farklı slot'lar paralel temizlenir.

### 3.9 AccountsBackgroundService (satır 585-751)

```rust
pub struct AccountsBackgroundService {
    t_background: JoinHandle<()>,
}
```

Tek bir arka plan thread'i ("solBgAccounts") 100ms loop ile çalışır:

**Main loop mantığı (satır 607-717):**
```
loop:
  exit check
  root_bank = bank_forks.read().root_bank()

  1. remove_dead_slots() — pruned bank'ları temizle

  2. handle_snapshot_requests() IF startup verification complete
     → Snapshot işleme süresi ölçülür (non_snapshot_time)

  3. flush_accounts_cache_if_needed()

  4. Eğer snapshot işlendiyse:
     last_cleaned_block_height = snapshot_block_height
  5. Değilse:
     if block_height - last_cleaned_block_height > (100 + random(0..10)):
       force_flush_accounts_cache()
       clean_accounts()
       shrink_ancient_slots() [sadece startup complete ise]
     shrink_candidate_slots() [sadece startup complete ise]

  sleep(100ms)
```

**`CLEAN_INTERVAL_BLOCKS + random(0..10)`:** Rastgelelik varyansı azaltır — tüm validator'lar aynı anda temizlik yapmasın.

**Startup verification guard:** Shrink ve snapshot işlemleri startup hash doğrulaması tamamlanmadan başlamaz.

### 3.10 setup_bank_drop_callback() (satır 725-746)

```rust
pub fn setup_bank_drop_callback(bank_forks: Arc<RwLock<BankForks>>) -> DroppedSlotsReceiver {
    assert_eq!(bank_forks.read().unwrap().banks().len(), 1);  // Sadece root bank olmalı!

    let (pruned_banks_sender, pruned_banks_receiver) = crossbeam_channel::unbounded();
    let root_bank = bank_forks.read().unwrap().root_bank();
    root_bank.rc.accounts.accounts_db.enable_bank_drop_callback();
    root_bank.set_callback(Some(Box::new(SendDroppedBankCallback::new(pruned_banks_sender))));
    pruned_banks_receiver
}
```

Drop callback sadece bir kez, başlangıçta kurulur. Tüm child bank'lar `_new_from_parent()` içinde `clone_box()` ile callback'i miras alır.

### 3.11 Snapshot Priority Logic (satır 753-807)

**`new_accounts_package_kind()` (satır 755-782):**
```rust
fn new_accounts_package_kind(snapshot_request, snapshot_config, last_full_snapshot_slot) {
    match snapshot_request.request_kind {
        EpochAccountsHash => AccountsPackageKind::EpochAccountsHash,
        _ => {
            if should_take_full_snapshot(block_height, full_snapshot_interval) {
                Snapshot(FullSnapshot)
            } else if should_take_incremental_snapshot(block_height, incremental_interval, last_full) {
                Snapshot(IncrementalSnapshot(last_full.unwrap()))
            } else {
                AccountsHashVerifier
            }
        }
    }
}
```

**`cmp_requests_by_priority()` (satır 793-807):** EAH > Full > Incremental > AHV. Aynı türde: slot sayısı tiebreaker.

### 3.12 GroupBy Iterator (satır 820-857)

Stable nightly API'nin stable versiyonu — pencere-tabanlı contiguous group iterator. `windows(2)` ile bitişik elemanlar karşılaştırılır.

### 3.13 Testler (satır 859-1184)

- `test_accounts_background_service_remove_dead_slots` — temel purge testi
- `test_get_next_snapshot_request` — kapsamlı priority queue testi (303 bank oluşturuluyor!)
  - EAH slot 100'de, Full Snapshot 80/160/240'da, Incremental ve AHV aralıkta
  - Beklenen işlem sırası: EAH → FullSnapshot → IncrementalSnapshot → AHV

---

## 4. Unsafe Blok Özeti

**bank.rs:** Sıfır `unsafe` blok.
**bank_forks.rs:** Sıfır `unsafe` blok.
**accounts_background_service.rs:** Sıfır `unsafe` blok.

Tüm concurrency Rust'ın type system'ı ile yönetiliyor.

---

## 5. State Machine Geçişleri

### Bank Yaşam Döngüsü
```
NEW (open) → [register_tick loop] → TICK_MAX_REACHED →
  freeze() →
    collect_rent_eagerly()
    distribute_transaction_fees()
    distribute_rent_fees()
    update_slot_history()
    run_incinerator()
    hash_internal_state()
    mark_slot_frozen()
  → FROZEN →
    [voting consensus] →
    squash() →
      freeze()
      add_root() [hesap merging]
      parent = None
      status_cache.add_root()
    → ROOTED (immutable)
```

### Epoch Reward State Machine
```
EpochRewardStatus::Inactive →
  [yeni epoch başı: process_new_epoch()]
  begin_partitioned_rewards() →
    calculate_rewards_and_distribute_vote_rewards()
    set_epoch_reward_status_active(partitions)
    create_epoch_rewards_sysvar()
  → EpochRewardStatus::Active(StartBlockHeightAndRewards) →
    [her blokta: distribute_partitioned_epoch_rewards()]
    distribute_epoch_rewards_in_partition(partition_index)
    [son partition] →
    deactivate_epoch_reward_status()
    [kalan lamport burn edilir]
  → EpochRewardStatus::Inactive
```

### Feature Activation State Machine
```
Feature Account:
  {activated_at: None}   → PENDING (next epoch'ta aktive olacak)
  {activated_at: Some(X)} → ACTIVE (slot >= X ise)

Geçiş (process_new_epoch → apply_feature_activations → compute_active_feature_set):
  INACTIVE → PENDING (account var, activated_at = None, include_pending = true)
           → feature.activated_at = Some(self.slot()) yazılır
           → ACTIVE
```

---

## 6. Nova Shield Chain İçin Dersler

### 6.1 Bank Mimari Deseni
**Ders:** Bank = bir slot'un tüm state'i. Her blok yeni bir Bank instance'ı. Parent-child zincir, AccountsDb paylaşımıyla implement edilir.

**NSC için:** Bizim "Block" struct'ımız benzer şekilde tasarlanmalı. Tüm mutable state atomic tipler veya RwLock içinde tutulmalı. `Arc<AccountsDb>` benzeri bir paylaşımlı storage katmanı şart.

### 6.2 Freeze Mekanizması
**Ders:** Blok tamamlanmadan hash hesaplanmamalı. Freeze fonksiyonu idempotent olmalı (aynı sonucu tekrar verir). Freeze öncesi tüm hesaplar settle edilmeli (rent, fees).

**NSC için:** `freeze()` eşdeğeri:
```rust
fn freeze_block(&self) {
    // idempotent guard
    if self.is_frozen() { return; }
    self.collect_fees();
    self.settle_pending_state();
    self.hash = self.compute_hash();
    self.frozen.store(true, Relaxed);
}
```

### 6.3 Partitioned Epoch Rewards
**Ders:** Büyük stake dağıtımları tek blokta yapılmamalı. `EpochRewardStatus` enum'u ile durum takibi. Reward dağıtımı sırasında ilgili hesaplara yazma bloğu.

**NSC için:** MPC threshold ödülleri de benzer şekilde bölünebilir. `EpochRewardStatus::Active` → staking hesaplarına write engeli pattern'i güvenlik açısından önemli.

### 6.4 Feature Activation
**Ders:** Feature'lar on-chain account üzerinden aktive edilir. `FeatureSet { active: HashMap<Pubkey, Slot>, inactive: HashSet<Pubkey> }` pattern'i çok temiz. Yeni feature activate edilince: inflation, burn percent, builtin programs güncellenir.

**NSC için:** Protocol upgrade mekanizmamız için aynı pattern kullanılabilir. Feature account'u bir Pubkey. Activated_at slot kaydedilir. Ardından `apply_feature_activations()` eşdeğeri — governance kararlarını uygular.

### 6.5 Atomic Ordering
**Ders:** `root.store(root, Ordering::Release)` + `ReadOnlyAtomicSlot::get(Ordering::Acquire)` çifti. Doğru memory ordering kritik.

**NSC için:** Cross-thread paylaşılan slot/block değerlerinde mutlaka `Acquire/Release` kullan. `Relaxed` sadece tek thread'de kullanılan counter'lar için uygun.

### 6.6 Accounts Background Service Pattern
**Ders:** Ağır I/O işlemleri (clean, shrink, snapshot) ayrı bir arka plan thread'e taşınır. Ana konsensüs loop'u bloke edilmez. Channel-based iletişim (crossbeam).

**NSC için:** Storage maintenance (eski state temizleme, snapshot alma, hash doğrulama) ayrı `tokio::spawn` veya thread ile yapılmalı. Interval + randomness (100 + random(0..10)) yük dengeleme için iyi pattern.

### 6.7 Snapshot Priority Queue
**Ders:** Birden fazla bekleyen snapshot isteği varsa en yüksek önceliklisi seçilir. EAH ve FullSnapshot arasında özel öncelik mantığı (aksi takdirde incremental bozulur).

**NSC için:** Checkpoint/snapshot sistemimizde benzer önceliklendirme gerekli. Full checkpoint her zaman incremental'dan önce işlenmeli.

### 6.8 Drop Callback Pattern
**Ders:** Bank drop edildiğinde otomatik cleanup. `DropCallback` trait + `clone_box()`. Her child bank callback'i miras alır.

**NSC için:** Block/epoch cleanup için `Drop` trait implement etmek yerine explicit callback tercih edilebilir. `clone_box()` pattern — Box<dyn Trait> cloning için standart.

### 6.9 Rent Collection ve Skipped Rewrites
**Ders:** Rent exempt hesaplar için gereksiz disk yazması önleniyor (`skip_rewrite` feature). Ancak bank hash'e dahil edilmeleri gerekiyor (consensus için). `skipped_rewrites: Mutex<HashMap<Pubkey, AccountHash>>` ile takip.

**NSC için:** Hesaplar için "dirty" flag mekanizması. Değişmeyen hesaplar hash'e dahil edilmeli ama yeniden yazılmamalı.

### 6.10 CostTracker
**Ders:** Her blok için compute cost takibi (`cost_tracker: RwLock<CostTracker>`). Block cost limit, vote cost limit, account cost limit ayrı ayrı kontrol edilir.

**NSC için:** Blok başına gas/compute limiti için benzer yapı şart. Özellikle MPC işlemleri pahalı — bunlar için ayrı cost bucket tanımlanmalı.

---

## 7. Performans Notları

- **Rayon thread pool:** Her epoch başında yeni oluşturulur. Rent collection ve reward hesaplama paralel.
- **DashMap:** Vote/stake reward hesabında lock-free concurrent write.
- **`measure_us!` ve `Measure`:** Her kritik operasyon zamanlıdır, datapoint olarak gönderilir.
- **`izip!` macro:** `zip` + assert_eq ile birden fazla iterator senkronizasyonu.
- **AtomicI64 saturating:** `accounts_data_size_delta` overflow'dan korunur.
- **`select_nth_unstable_by`:** O(n) partial sort — tam sort yapmadan en yüksek 2 önceliği bul.

---

## 8. Kritik Tasarım Kararları

1. **AccountsDb paylaşımı:** Child bank'lar `Arc::clone` ile parent'ın AccountsDb'sini paylaşır. Copy-on-write benzeri — sadece değişen hesaplar yeni slot'a yazılır.

2. **`bank_id` vs `slot`:** Aynı slot'ta birden fazla bank olabilir (fork). `bank_id` global benzersiz kimlik.

3. **Status cache paylaşımı:** Tüm bank'lar aynı `Arc<RwLock<BankStatusCache>>` paylaşır — tek bir işlem iki farklı fork'ta kabul edilemez.

4. **`program_cache` paylaşımı:** Child bank'lar aynı `Arc<RwLock<ProgramCache>>` paylaşır. Deploy edilen programlar hemen cache'e alınır ve sonraki bank'larda görünür.

5. **Fee collection timing:** Ücretler anında `collector_fees`'e eklenir. Freeze sırasında distributor'a aktarılır.

6. **Vote-only bank:** `vote_only_bank: bool` — sadece vote işlemlerini kabul eden özel mod. `MAX_ROOT_DISTANCE_FOR_VOTE_ONLY = 400` slot gerisinde kalındığında aktive edilir.

7. **hard_forks paylaşımı:** `Arc<RwLock<HardForks>>` — tüm bank'lar aynı hard fork listesini paylaşır.

---

*Agent 10 analizi tamamlandı. Dosyalar: bank.rs (8003 satır), bank_forks.rs (1112 satır), accounts_background_service.rs (1184 satır) — toplam 10299 satır.*
