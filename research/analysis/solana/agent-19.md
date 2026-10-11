# Solana SVM + Runtime Ek Modüller Analizi — Agent 19

**Analiz Tarihi:** 2026-07-22
**Kapsam:** svm/src/ (tüm dosyalar) + runtime/src/ (seçili 15 dosya)
**Toplam Analiz Edilen Satır:** ~8.000+ satır

---

## BÖLÜM 1: SVM (Solana Virtual Machine) — svm/src/

### 1.1 svm/src/lib.rs

**Satır sayısı:** 17 satır
**Konum:** `/research/chains/solana/svm/src/lib.rs`

Bu dosya SVM crate'inin root modül tanımlamalarını içerir.

**Crate-level attribute'lar:**
```rust
// Satır 1
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
// Satır 2
#![allow(clippy::arithmetic_side_effects)]
```

**Modül tanımlamaları (satır 4-10):**
```rust
pub mod account_loader;
pub mod account_overrides;
pub mod account_rent_state;
pub mod transaction_account_state_info;
pub mod transaction_error_metrics;
pub mod transaction_processor;
pub mod transaction_results;
```

**Dış bağımlılıklar (satır 12-16):**
```rust
#[macro_use]
extern crate solana_metrics;

#[macro_use]
extern crate solana_frozen_abi_macro;
```

**Nova Shield Chain Dersleri:**
- SVM, tek bir crate olarak 7 modülden oluşuyor. Modüler yapı; account yükleme, execution, result ve metrics'i birbirinden ayırıyor.
- `solana_metrics` ve `solana_frozen_abi_macro` macro olarak inject ediliyor, tüm modüllerde kullanılabilir oluyor.
- Minimal lib.rs: işlevsellik modüllerde, lib.rs sadece re-export noktası.

---

### 1.2 svm/src/account_overrides.rs

**Satır sayısı:** 63 satır
**Konum:** `/research/chains/solana/svm/src/account_overrides.rs`

**Struct (satır 8-10):**
```rust
#[derive(Default)]
pub struct AccountOverrides {
    accounts: HashMap<Pubkey, AccountSharedData>,
}
```

**Metotlar:**

- `set_account(pubkey, account: Option<AccountSharedData>)` — Satır 14: `Some` → insert, `None` → remove. Insert-or-remove pattern.
- `set_slot_history(slot_history: Option<AccountSharedData>)` — Satır 24: `sysvar::slot_history::id()` pubkey'e shortcut.
- `get(pubkey: &Pubkey) -> Option<&AccountSharedData>` — Satır 29: HashMap lookup.

**Test modülü (satır 34-63):**
- `test_set_account` — insert/remove testi.
- `test_slot_history` — sysvar ID ile set/get testi.

**Tasarım kararı:** `AccountOverrides`, transaction simulasyonları için tasarlanmış. RPC `simulateTransaction` endpoint'i, gerçek hesap verileri yerine override'larla çalışır. `Option<AccountSharedData>` → `None` means delete pattern, güzel idiomatic Rust.

**Nova Shield Chain Dersleri:**
- Transaction simulation için account override mekanizması şart. State simulation katmanında bu paterni uygula.
- `set_slot_history` gibi strongly-typed sysvar accessor'lar, güvenli API tasarımı örneği.
- Basit `HashMap<Pubkey, AccountSharedData>` yeterli, complex cache gerekmez.

---

### 1.3 svm/src/account_rent_state.rs

**Satır sayısı:** 310 satır
**Konum:** `/research/chains/solana/svm/src/account_rent_state.rs`

**Enum (satır 12-23):**
```rust
#[derive(Debug, PartialEq, Eq)]
pub enum RentState {
    /// account.lamports == 0
    Uninitialized,
    /// 0 < account.lamports < rent-exempt-minimum
    RentPaying {
        lamports: u64,
        data_size: usize,
    },
    /// account.lamports >= rent-exempt-minimum
    RentExempt,
}
```

**Kritik metotlar:**

**`from_account(account, rent) -> Self` — Satır 27:**
```rust
pub fn from_account(account: &AccountSharedData, rent: &Rent) -> Self {
    if account.lamports() == 0 {
        Self::Uninitialized
    } else if rent.is_exempt(account.lamports(), account.data().len()) {
        Self::RentExempt
    } else {
        Self::RentPaying { data_size: account.data().len(), lamports: account.lamports() }
    }
}
```

**`transition_allowed_from(pre_rent_state) -> bool` — Satır 42:**
Geçiş kuralları:
- `Uninitialized` veya `RentExempt` hedef → her zaman izin
- `RentPaying` hedef → sadece pre_state de `RentPaying` VE (data_size değişmediyse VE lamport azaldıysa/aynıysa) izin
- Kritik kural: lamport eklendi ama hala `RentPaying` → YASAK (kredi + rent-paying invalid)

**`check_rent_state(pre, post, context, index) -> Result<()>` — Satır 63:**
TransactionContext üzerinden index-based hesap state kontrolü.

**`check_rent_state_with_account(pre, post, address, account, index) -> Result<()>` — Satır 88:**
- Incinerator adresi özel case: her zaman `Ok`
- `submit_rent_state_metrics` ile telemetri

**`submit_rent_state_metrics` — Satır 110:**
`inc_new_counter_info!` macro'su ile metrik counter'ları:
- `rent_paying_err-new_account` — yeni hesap ama rent paying
- `rent_paying_ok-legacy` — eski format kabul
- `rent_paying_err-other` — diğer geçersiz geçişler

**Test modülü (satır 126-309):**
- `test_from_account` — 3 state senaryosu
- `test_transition_allowed_from` — tüm geçiş kombinasyonları
- `test_check_rent_state_with_account` — error propagation
- `test_check_rent_state` — context üzerinden check

**Nova Shield Chain Dersleri:**
- Rent state machine 3-state FSM olarak modellenmiş. Nova Shield'da benzer bir account lifecycle state machine şart.
- Geçiş kuralları: bir hesap rent-paying olarak KALAMAZ eğer lamport aldıysa — bunu enforce et.
- Incinerator pattern: belirli adreslerin tüm kural dışı olmasına izin ver.
- Pre/post state comparison pattern: tx öncesi ve sonrası state snapshot'ı alıp compare et.

---

### 1.4 svm/src/transaction_account_state_info.rs

**Satır sayısı:** 238 satır
**Konum:** `/research/chains/solana/svm/src/transaction_account_state_info.rs`

**Struct (satır 13-16):**
```rust
#[derive(PartialEq, Debug)]
pub(crate) struct TransactionAccountStateInfo {
    rent_state: Option<RentState>, // None: readonly account
}
```

`Option<RentState>`: `None` → readonly hesap (rent check gerekmez), `Some(state)` → writable hesap.

**`new(rent, transaction_context, message) -> Vec<Self>` — Satır 19:**
- `message.account_keys().len()` kadar döngü
- `message.is_writable(i)` → writable ise RentState hesapla
- Readonly ise `None`
- Debug assert: `native_loader::check_id(account.owner())` → native program writable olamaz
- Context sync kontrolü: context ile message arasında hesap sayısı uyumsuzluğu → `debug_assert!` panic

**`verify_changes(pre, post, context) -> Result<()>` — Satır 53:**
- `pre_state_infos.iter().zip(post_state_infos)` → pair-wise comparison
- Her çift için `RentState::check_rent_state` çağrısı

**Test modülü (satır 72-238):**
- `test_new` — writable/readonly state assignment testi
- `test_new_panic` — context-message mismatch → should_panic
- `test_verify_changes` — valid/invalid geçiş kombinasyonları

**Nova Shield Chain Dersleri:**
- Pre/post state tracking için `Vec<StateInfo>` paterni — transaction execution wrapper olarak kullan.
- `Option<State>` ile readonly/writable ayrımı — state tracking overhead'i minimize eder.
- Context ve mesaj arasındaki sync'i `debug_assert!` ile erken yakala, production'da performance kaybı yok.
- `verify_changes` → transaction commit öncesi son doğrulama hook'u olarak kullan.

---

### 1.5 svm/src/transaction_error_metrics.rs

**Satır sayısı:** 177 satır
**Konum:** `/research/chains/solana/svm/src/transaction_error_metrics.rs`

**Struct (satır 3-28):**
```rust
#[derive(Debug, Default)]
pub struct TransactionErrorMetrics {
    pub total: usize,
    pub account_in_use: usize,
    pub too_many_account_locks: usize,
    pub account_loaded_twice: usize,
    pub account_not_found: usize,
    pub blockhash_not_found: usize,
    pub blockhash_too_old: usize,
    pub call_chain_too_deep: usize,
    pub already_processed: usize,
    pub instruction_error: usize,
    pub insufficient_funds: usize,
    pub invalid_account_for_fee: usize,
    pub invalid_account_index: usize,
    pub invalid_program_for_execution: usize,
    pub not_allowed_during_cluster_maintenance: usize,
    pub invalid_writable_account: usize,
    pub invalid_rent_paying_account: usize,
    pub would_exceed_max_block_cost_limit: usize,
    pub would_exceed_max_account_cost_limit: usize,
    pub would_exceed_max_vote_cost_limit: usize,
    pub would_exceed_account_data_block_limit: usize,
    pub max_loaded_accounts_data_size_exceeded: usize,
    pub program_execution_temporarily_restricted: usize,
}
```

23 ayrı hata kategorisi takip ediliyor.

**Metotlar:**

- `new() -> Self` — Satır 31: `Default::default()` wrapper.
- `accumulate(&mut self, other: &TransactionErrorMetrics)` — Satır 35: `saturating_add_assign!` macro ile her field toplanır. Saturation: overflow değil saturation (u64::MAX'ta kalır).
- `report(id: u32, slot: Slot)` — Satır 91: `datapoint_info!` macro ile `banking_stage-leader_slot_transaction_errors` metric'i yayınlar.

**Nova Shield Chain Dersleri:**
- 23 hata kategorisi → bunları kendi error taxonomy'mize adapt et. Özellikle: blockhash timeout, account lock contention, cost limit aşımı.
- `accumulate` pattern: batch processing'de birden fazla worker'ın metriklerini merge etmek için kullan.
- `saturating_add_assign!`: counter overflow'a karşı güvenli yaklaşım.
- Metrics raporlama işlemi transaction processing'den ayrı — sadece `report()` çağrıldığında yayınlanır, hot-path'de değil.

---

### 1.6 svm/src/transaction_results.rs

**Satır sayısı:** 107 satır
**Konum:** `/research/chains/solana/svm/src/transaction_results.rs`

**Struct (satır 17-21):**
```rust
pub struct TransactionResults {
    pub fee_collection_results: Vec<transaction::Result<()>>,
    pub execution_results: Vec<TransactionExecutionResult>,
    pub rent_debits: Vec<RentDebits>,
}
```

**Enum (satır 34-40):**
```rust
#[derive(Debug, Clone)]
pub enum TransactionExecutionResult {
    Executed {
        details: TransactionExecutionDetails,
        programs_modified_by_tx: Box<LoadedProgramsForTxBatch>,
    },
    NotExecuted(TransactionError),
}
```

Kritik tasarım: `Executed` → hem başarılı hem başarısız execution sonuçlarını taşır. İçindeki `details.status` ayrıca kontrol gerektirir. `NotExecuted` → execution hiç denenmedi.

**Metotlar:**
- `was_executed_successfully() -> bool` — Satır 43: `Executed` ve `details.status.is_ok()` her ikisi de true olmalı.
- `was_executed() -> bool` — Satır 50: sadece `Executed` variant kontrolü.
- `details() -> Option<&TransactionExecutionDetails>` — Satır 56: `Executed`'dan details çıkar.
- `flattened_result() -> transaction::Result<()>` — Satır 64: `Executed`'daki `details.status` veya `NotExecuted`'daki error.

**Struct (satır 72-83):**
```rust
#[derive(Debug, Clone)]
pub struct TransactionExecutionDetails {
    pub status: transaction::Result<()>,
    pub log_messages: Option<Vec<String>>,
    pub inner_instructions: Option<InnerInstructionsList>,
    pub durable_nonce_fee: Option<DurableNonceFee>,
    pub return_data: Option<TransactionReturnData>,
    pub executed_units: u64,
    /// NOTE: This value is valid IFF `status` is `Ok`.
    pub accounts_data_len_delta: i64,
}
```

**Enum (satır 85-89):**
```rust
#[derive(Debug, Clone)]
pub enum DurableNonceFee {
    Valid(u64),
    Invalid,
}
```

`DurableNonceFee::Valid(lamports_per_signature)` veya `Invalid` (nonce account eksik veri).

**From impl (satır 91-97):**
`From<&NonceFull> for DurableNonceFee` — `lamports_per_signature()` → `Valid` veya `Invalid`.

**Nova Shield Chain Dersleri:**
- `TransactionExecutionResult` → executed-but-failed ve not-executed ayrımı kritik. Nova Shield'da bu ayrımı zorla: ledger'a sadece `Executed` olanlar yazılmalı, `NotExecuted`'lar da drop edilmeli ama fee alınmamalı.
- `was_executed_successfully()` convenience method: iç iç kontrol hatalarını önler.
- `accounts_data_len_delta: i64` → execution sonrası toplam data büyümesi/küçülmesi. Nova Shield'da state growth tracking için kullan.
- `programs_modified_by_tx: Box<LoadedProgramsForTxBatch>` → çalıştırılan transaction hangi programları update etti? Program cache invalidation için gerekli.

---

### 1.7 svm/src/account_loader.rs

**Satır sayısı:** ~450+ satır
**Konum:** `/research/chains/solana/svm/src/account_loader.rs`

**Tip tanımları (satır 38-41):**
```rust
pub(crate) type TransactionRent = u64;
pub(crate) type TransactionProgramIndices = Vec<Vec<IndexOfAccount>>;
pub type TransactionCheckResult = (transaction::Result<()>, Option<NoncePartial>, Option<u64>);
pub type TransactionLoadResult = (Result<LoadedTransaction>, Option<NonceFull>);
```

**Struct (satır 43-49):**
```rust
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LoadedTransaction {
    pub accounts: Vec<TransactionAccount>,
    pub program_indices: TransactionProgramIndices,
    pub rent: TransactionRent,
    pub rent_debits: RentDebits,
}
```

**`validate_fee_payer(payer_address, payer_account, payer_index, error_counters, rent_collector, fee)` — Satır 56:**
Fee payer doğrulama pipeline:
1. `lamports() == 0` → `AccountNotFound`
2. `get_system_account_kind()` → `InvalidAccountForFee` (native program veya data hesabı fee payer olamaz)
3. Nonce account ise minimum balance hesabı: `rent.minimum_balance(NonceState::size())`
4. `checked_sub` ile fee çıkarma — underflow → `InsufficientFundsForFee`
5. `RentState` pre/post kontrolü: fee çıkarınca RentPaying'e düşemez

**`load_accounts(callbacks, txs, lock_results, error_counters, fee_structure, account_overrides, program_accounts, loaded_programs)` — Satır 110:**
Ana batch load loop:
```rust
txs.iter()
    .zip(lock_results)
    .map(|etx| match etx {
        (tx, (Ok(()), nonce, lamports_per_signature)) => { ... }
        (_, (Err(e), ...)) => (Err(e.clone()), None),
    })
    .collect()
```
- `fee_structure.calculate_fee()` → feature flag'lere göre fee hesabı
- `load_transaction_accounts()` çağrısı
- `NonceFull::from_partial()` ile nonce güncelleme

**`load_transaction_accounts(callbacks, message, fee, error_counters, account_overrides, program_accounts, loaded_programs)` — Satır 179:**
Hesap yükleme ana fonksiyonu:
1. `requested_loaded_accounts_data_size_limit` kontrolü
2. Her hesap için yükleme stratejisi belirleme:
   - `sysvar::instructions::check_id` → construct in-place
   - Sadece program olarak kullanılan hesap + loaded_programs'da var → skip loading (optimization)
   - Account override varsa → override kullan
   - Diğer → callback'ten yükle
3. Rent deduction
4. Accumulated accounts data size check
5. Program indices oluşturma

**`get_requested_loaded_accounts_data_size_limit(message)` — Satır 393:**
ComputeBudget instruction'larından `set_loaded_accounts_data_size_limit` okur.

**`account_shared_data_from_program(key, program_accounts)` — Satır 409:**
Program hesabı için stub `AccountSharedData` üretir.

**`accumulate_and_check_loaded_account_data_size(accumulated, size, limit, error_counters)` — Satır 429:**
Running sum + limit kontrolü. `max_loaded_accounts_data_size_exceeded` counter artışı.

**`construct_instructions_account(message)` — Satır 448:**
`sysvar::instructions` özel hesabı oluşturur, mesaj talimatlarını serialize eder.

**Nova Shield Chain Dersleri:**
- Fee validation pipeline: Sıralı adım adım doğrulama. Her adım erken return + counter artışı. Nova Shield'da aynı pattern.
- Program account yükleme optimizasyonu: sadece top-level program olarak kullanılan hesapları gerçek storage'dan yükleme; cache'den stub data döndür.
- `TransactionCheckResult = (Result<()>, Option<NoncePartial>, Option<u64>)` tuple pattern: lock check, nonce info ve lamports_per_signature aynı anda taşınıyor. Nova Shield'da benzer check result structure.
- Loaded accounts data size limit: tx başına toplam yüklenen account data miktarını sınırla. DOS koruması için kritik.
- `account_overrides` her yükleme kararından önce kontrol ediliyor. Override mechanism her zaman priority.

---

### 1.8 svm/src/transaction_processor.rs

**Satır sayısı:** ~950+ satır (test dahil)
**Konum:** `/research/chains/solana/svm/src/transaction_processor.rs`

**Tip tanımı (satır 59):**
```rust
pub type TransactionLogMessages = Vec<String>;
```

**Struct (satır 61-66):**
```rust
pub struct LoadAndExecuteSanitizedTransactionsOutput {
    pub loaded_transactions: Vec<TransactionLoadResult>,
    pub execution_results: Vec<TransactionExecutionResult>,
}
```

**Struct (satır 69-74):**
```rust
#[derive(Copy, Clone)]
pub struct ExecutionRecordingConfig {
    pub enable_cpi_recording: bool,
    pub enable_log_recording: bool,
    pub enable_return_data_recording: bool,
}
```

**`ExecutionRecordingConfig::new_single_setting(option: bool)` — Satır 77:**
Tüm recording flag'lerini tek bir bool ile ayarlar.

**Trait `TransactionProcessingCallback` — Satır 86:**
```rust
pub trait TransactionProcessingCallback {
    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize>;
    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData>;
    fn get_last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64);
    fn get_rent_collector(&self) -> &RentCollector;
    fn get_feature_set(&self) -> Arc<FeatureSet>;
    // Default implementasyonlar:
    fn check_account_access(&self, ..) -> transaction::Result<()> { Ok(()) }
    fn get_program_match_criteria(&self, ..) -> LoadedProgramMatchCriteria { NoCriteria }
}
```

Bu trait, Bank ve diğer runtime'ların SVM'ye interface sağlamak için implement ettiği callback. Dependency inversion: SVM, Bank'ı doğrudan bilmiyor.

**Enum `ProgramAccountLoadResult` — Satır 112:**
```rust
#[derive(Debug)]
enum ProgramAccountLoadResult {
    AccountNotFound,
    InvalidAccountData(ProgramRuntimeEnvironment),
    ProgramOfLoaderV1orV2(AccountSharedData),
    ProgramOfLoaderV3(AccountSharedData, AccountSharedData, Slot),
    ProgramOfLoaderV4(AccountSharedData, Slot),
}
```
3 farklı loader versiyonu + not found + invalid data durumları.

**Struct `TransactionBatchProcessor<FG: ForkGraph>` — Satır 122:**
```rust
#[derive(AbiExample)]
pub struct TransactionBatchProcessor<FG: ForkGraph> {
    slot: Slot,
    epoch: Epoch,
    epoch_schedule: EpochSchedule,
    fee_structure: FeeStructure,
    runtime_config: Arc<RuntimeConfig>,
    pub sysvar_cache: RwLock<SysvarCache>,
    pub program_cache: Arc<RwLock<ProgramCache<FG>>>,
}
```

Generic over `ForkGraph` — fork yönetimi için trait bound. Bu ayrım SVM'yi fork-aware yapıyor.

**`Debug` impl (satır 144):** Manuel `fmt` impl, `FG: ForkGraph` bound'u `Debug` gerektirmediği için.

**`Default` impl (satır 158):** Sıfır slot/epoch, default scheduler vb.

**`TransactionBatchProcessor::new(slot, epoch, epoch_schedule, fee_structure, runtime_config, program_cache)` — Satır 176:**
Constructor. `sysvar_cache` her zaman yeni oluşturulur.

**`load_and_execute_sanitized_transactions<CB: TransactionProcessingCallback>(...)` — Satır 197:**
**SVM'nin ana giriş noktası.** Pipeline:

```
1. filter_executable_program_accounts() → program_accounts_map
2. Insert builtin programs into map
3. replenish_program_cache() → programs_loaded_for_tx_batch
4. limit check → early return if hit_max_limit
5. load_accounts() → loaded_transactions (LOAD PHASE)
6. For each tx: execute_loaded_transaction() (EXECUTE PHASE)
7. Collect execution_results
```

Load phase + Execute phase açıkça ayrılmış.

**`load_program_with_pubkey<CB: TransactionProcessingCallback>` — Satır 380:**
Tek bir programı pubkey ile yükle. Cache lookup → miss → load from storage.

**`program_modification_slot<CB: TransactionProcessingCallback>` — Satır 747:**
Program'ın son değiştirildiği slot'u döndür (loader versiyonuna göre farklı logic).

**Unsafe bloklar (satır 792, 1110, 1431, 1591):**
`slice::from_raw_parts` ve benzeri unsafe ops — muhtemelen VM execute context sırasında borrowed data erişimi için.

**Nova Shield Chain Dersleri:**
- `TransactionProcessingCallback` trait → Nova Shield'da SVM benzeri bir execution engine, runtime'dan callback interface üzerinden ayrılmalı. Testability açısından kritik.
- `ForkGraph` generic bound → execution engine, fork tree'den habersiz; sadece fork graph'ı implement eden type geliyor. Nova Shield'da da execution'ı fork-agnostic tut.
- `program_cache: Arc<RwLock<ProgramCache<FG>>>` → program cache thread-safe arc içinde, birden fazla thread paylaşıyor.
- `hit_max_limit` early return: program cache kapasitesi dolduğunda batch'i tamamen düşür. Nova Shield'da benzer circuit breaker pattern.
- `programs_loaded_for_tx_batch` Rc<RefCell<...>> — tek thread içinde paylaşım, Arc değil. Batch scope boyunca yaşıyor.
- Execute phase sırasında telemetry için `ExecuteTimings` her execution'a geçiliyor — timing granülaritesi çok yüksek.
- `ProgramAccountLoadResult` enum: loader versiyonu V1/V2/V3/V4 farklı loading stratejileri. Nova Shield'da da loader versiyonlama gerekecek.

---

## BÖLÜM 2: Runtime Ek Dosyaları — runtime/src/

### 2.1 runtime/src/epoch_stakes.rs

**Satır sayısı:** 228 satır
**Konum:** `/research/chains/solana/runtime/src/epoch_stakes.rs`

**Tip tanımları (satır 9-10):**
```rust
pub type NodeIdToVoteAccounts = HashMap<Pubkey, NodeVoteAccounts>;
pub type EpochAuthorizedVoters = HashMap<Pubkey, Pubkey>;
```

**Struct (satır 12-16):**
```rust
#[derive(Clone, Serialize, Debug, Deserialize, Default, PartialEq, Eq, AbiExample)]
pub struct NodeVoteAccounts {
    pub vote_accounts: Vec<Pubkey>,
    pub total_stake: u64,
}
```

Bir validator node'unun tüm vote account pubkey'leri + toplam stake.

**Struct (satır 18-25):**
```rust
#[derive(Clone, Debug, Serialize, Deserialize, AbiExample, PartialEq)]
pub struct EpochStakes {
    #[serde(with = "crate::stakes::serde_stakes_enum_compat")]
    stakes: Arc<StakesEnum>,
    total_stake: u64,
    node_id_to_vote_accounts: Arc<NodeIdToVoteAccounts>,
    epoch_authorized_voters: Arc<EpochAuthorizedVoters>,
}
```

`Arc<StakesEnum>` ile referans sayılan stake snapshot. Tüm field'lar `Arc` ile paylaşılabiliyor.

**`EpochStakes::new(stakes, leader_schedule_epoch)` — Satır 28:**
```rust
pub(crate) fn new(stakes: Arc<StakesEnum>, leader_schedule_epoch: Epoch) -> Self {
    let epoch_vote_accounts = stakes.vote_accounts();
    let (total_stake, node_id_to_vote_accounts, epoch_authorized_voters) =
        Self::parse_epoch_vote_accounts(epoch_vote_accounts.as_ref(), leader_schedule_epoch);
    Self { stakes, total_stake, node_id_to_vote_accounts: Arc::new(...), epoch_authorized_voters: Arc::new(...) }
}
```

**Accessor metotlar:**
- `stakes() -> &StakesEnum` — Satır 40
- `total_stake() -> u64` — Satır 44
- `set_total_stake(&mut self, total_stake: u64)` — Satır 49: test için
- `node_id_to_vote_accounts() -> &Arc<NodeIdToVoteAccounts>` — Satır 53
- `epoch_authorized_voters() -> &Arc<EpochAuthorizedVoters>` — Satır 57
- `vote_account_stake(vote_account: &Pubkey) -> u64` — Satır 61

**`parse_epoch_vote_accounts(epoch_vote_accounts, leader_schedule_epoch)` — Satır 67:**
Ana parsing logic:
1. `total_stake = sum(all stake amounts)`
2. Her vote account için:
   - `vote_state` deserialize
   - `stake > 0` filtresi
   - `authorized_voter` için `leader_schedule_epoch`'u kullan
   - `node_id_to_vote_accounts[node_pubkey].push(vote_account_key)`
   - `epoch_authorized_voters[vote_account_key] = authorized_voter`
3. Başarısız deserialize → `datapoint_warn!` + skip

**Test (satır 124-227):**
- `test_parse_epoch_vote_accounts` — 10 node, her node için 2 vote account, 100 lamports stake.

**Nova Shield Chain Dersleri:**
- EpochStakes, epoch başında bir kez hesaplanıp snapshot alınıyor. Nova Shield'da da validator stake bilgisi epoch başında sabitlenmeli.
- `node_id_to_vote_accounts` → bir node'un birden fazla vote account'u olabilir. Nova Shield'da da bu modeling gerekli.
- `authorized_voter` epoch-specific: her epoch için authorize değişebilir. `leader_schedule_epoch` parametresi bunu handle ediyor.
- `datapoint_warn!` ile invalid vote state → sessizce skip, sistemi durdurmaz.
- `Arc<NodeIdToVoteAccounts>` paylaşım: birden fazla banka aynı epoch'ta aynı EpochStakes'i paylaşabilir — clone yerine Arc.

---

### 2.2 runtime/src/genesis_utils.rs

**Satır sayısı:** 285 satır
**Konum:** `/research/chains/solana/runtime/src/genesis_utils.rs`

**Sabitler (satır 22-47):**
```rust
const VALIDATOR_LAMPORTS: u64 = 42;
pub fn bootstrap_validator_stake_lamports() -> u64 {
    Rent::default().minimum_balance(StakeStateV2::size_of())
}
pub const fn genesis_sysvar_and_builtin_program_lamports() -> u64 {
    // NUM_BUILTIN_PROGRAMS = 9, NUM_PRECOMPILES = 2
    // FEES_SYSVAR_MIN_BALANCE = 946_560
    // STAKE_HISTORY_MIN_BALANCE = 114_979_200
    // ... toplam sabit
}
```

**Struct (satır 50-54):**
```rust
pub struct ValidatorVoteKeypairs {
    pub node_keypair: Keypair,
    pub vote_keypair: Keypair,
    pub stake_keypair: Keypair,
}
```

**Struct (satır 74-79):**
```rust
pub struct GenesisConfigInfo {
    pub genesis_config: GenesisConfig,
    pub mint_keypair: Keypair,
    pub voting_keypair: Keypair,
    pub validator_pubkey: Pubkey,
}
```

**Fonksiyonlar:**

- `create_genesis_config(mint_lamports)` — Satır 81: zero validator stake ile genesis (test friendly).
- `create_genesis_config_with_vote_accounts(mint_lamports, voting_keypairs, stakes)` — Satır 93: Development cluster.
- `create_genesis_config_with_vote_accounts_and_cluster_type(...)` — Satır 106: Cluster type parametre.
- `create_genesis_config_with_leader(mint_lamports, validator_pubkey, validator_stake_lamports)` — Satır 169: Tek leader ile genesis.
- `activate_all_features(genesis_config)` — Satır 199: FeatureSet::default().inactive'deki tüm feature'ları genesis'te aktive et (development mode).
- `activate_feature(genesis_config, feature_id)` — Satır 206: Tek feature aktive et.
- `create_genesis_config_with_leader_ex(...)` — Satır 218: Full parametric genesis oluşturucu:
  - vote account, stake account, mint, validator accounts oluşturur
  - `inline_spl_token::native_mint` hesabı ekler
  - Development cluster'da tüm feature'ları aktive eder

**Nova Shield Chain Dersleri:**
- Genesis utilities: test için ayrı builder fonksiyonları. Nova Shield testlerinde benzer `create_genesis_config` helpers kullan.
- `VALIDATOR_LAMPORTS = 42` — test convenience value. Prodüksiyonda gerçek değer.
- `activate_all_features` → development'ta tüm feature'ları açık başla. Nova Shield'da feature flag system gerekli.
- `native_mint` account genesis'e ekleniyor — built-in token ekonomisi başlangıcı.
- `bootstrap_validator_stake_lamports()` → rent-exempt minimum hesabı, hardcoded değil.
- `FeeRateGovernor::new(0, 0)` ve `Rent::free()` test defaults — test ortamı kolaylığı.

---

### 2.3 runtime/src/builtins.rs

**Satır sayısı:** 110 satır
**Konum:** `/research/chains/solana/runtime/src/builtins.rs`

**Struct (satır 9-14):**
```rust
pub struct BuiltinPrototype {
    pub feature_id: Option<Pubkey>,
    pub program_id: Pubkey,
    pub name: &'static str,
    pub entrypoint: BuiltinFunctionWithContext,
}
```

`feature_id: None` → her zaman aktif. `feature_id: Some(id)` → feature aktif olduğunda devreye girer.

**Debug impl (satır 16-24):** `entrypoint` function pointer `Debug` implement etmediğinden manuel impl.

**AbiExample impl (satır 26-41):** Mock builtin fonksiyon ile örnek üretimi.

**`BUILTINS` static array (satır 43-109):**
11 builtin program:
1. `system_program` → `solana_system_program` (feature_id: None)
2. `vote_program` → `solana_vote_program` (feature_id: None)
3. `stake_program` → `solana_stake_program` (feature_id: None)
4. `config_program` → `solana_config_program` (feature_id: None)
5. `solana_bpf_loader_deprecated_program` → `bpf_loader_deprecated` (feature_id: None)
6. `solana_bpf_loader_program` → `bpf_loader` (feature_id: None)
7. `solana_bpf_loader_upgradeable_program` → `bpf_loader_upgradeable` (feature_id: None)
8. `compute_budget_program` → `solana_sdk::compute_budget` (feature_id: None)
9. `address_lookup_table_program` → `solana_address_lookup_table_program` (feature_id: None)
10. `zk_token_proof_program` → `feature_set::zk_token_sdk_enabled` (conditional)
11. `loader_v4` → `feature_set::enable_program_runtime_v2_and_loader_v4` (conditional)

**Nova Shield Chain Dersleri:**
- Nova Shield'da built-in programlar için aynı `BuiltinPrototype` pattern kullan.
- Feature-gated builtins: yeni sistem programları feature flag ile devreye girer. Hard fork yoktur.
- `&'static str` name field: program adı binary'ye gömülü.
- `entrypoint: BuiltinFunctionWithContext` — fonksiyon pointer olarak tutulur. Dispatcher overhead'ı yok.
- Nova Shield minimum builtins: system, stake, vote (veya consensus), compute budget, BPF loader.

---

### 2.4 runtime/src/commitment.rs

**Satır sayısı:** 345 satır
**Konum:** `/research/chains/solana/runtime/src/commitment.rs`

**Sabit (satır 7):**
```rust
pub const VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64;
```
Süpermajority threshold: 2/3.

**Tip tanımı (satır 9):**
```rust
pub type BlockCommitmentArray = [u64; MAX_LOCKOUT_HISTORY + 1];
```
`MAX_LOCKOUT_HISTORY` vote lockout geçmişi uzunluğu. Son slot rooted.

**Struct (satır 11-14):**
```rust
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockCommitment {
    pub commitment: BlockCommitmentArray,
}
```
Her index, o confirmation depth'te lock olan stake miktarı. `commitment[MAX_LOCKOUT_HISTORY]` → rooted stake.

**Metotlar:**
- `increase_confirmation_stake(confirmation_count, stake)` — Satır 17: index `confirmation_count - 1`'e stake ekle.
- `get_confirmation_stake(confirmation_count) -> u64` — Satır 22.
- `increase_rooted_stake(stake)` — Satır 27: `commitment[MAX_LOCKOUT_HISTORY]` artır.
- `get_rooted_stake() -> u64` — Satır 31.
- `new(commitment: BlockCommitmentArray)` — Satır 35.

**Struct (satır 42-51):**
```rust
#[derive(Default)]
pub struct BlockCommitmentCache {
    block_commitment: HashMap<Slot, BlockCommitment>,
    commitment_slots: CommitmentSlots,
    total_stake: u64,
}
```

**Metotlar:**
- `new(block_commitment, total_stake, commitment_slots)` — Satır 68.
- `get_block_commitment(slot) -> Option<&BlockCommitment>` — Satır 80.
- `total_stake()`, `slot()`, `root()`, `highest_confirmed_slot()`, `highest_super_majority_root()` — Satır 84-101.
- `slot_with_commitment(commitment_level) -> Slot` — Satır 114: CommitmentLevel'a göre slot seç:
  - `Processed/Recent` → current slot
  - `Root` → root slot
  - `Confirmed/Single/SingleGossip` → highest confirmed
  - `Finalized/Max` → highest super majority root
- `highest_slot_with_confirmation_count(confirmation_count) -> Slot` — Satır 127: root'tan slot'a kadar iterasyon, threshold'u geçen ilk slot.
- `calculate_highest_confirmed_slot() -> Slot` — Satır 139: confirmation_count=1 ile.
- `get_confirmation_count(slot) -> Option<usize>` — Satır 143: `VOTE_THRESHOLD_SIZE` ile.
- `get_lockout_count(slot, minimum_stake_percentage) -> Option<usize>` — Satır 149: Stake toplamı threshold'u geçtiğinde lock seviyesi döner.

**Struct (satır 209-218):**
```rust
#[derive(Default, Clone, Copy)]
pub struct CommitmentSlots {
    pub slot: Slot,                          // current bank slot
    pub root: Slot,                          // current node root
    pub highest_confirmed_slot: Slot,        // cluster confirmed
    pub highest_super_majority_root: Slot,   // 2/3+ rooted
}
```

**Nova Shield Chain Dersleri:**
- `BlockCommitmentArray` → confirmation depth başına stake array'i. Her vote yeni bir confirmation depth oluşturur.
- 4 commitment level: Processed, Confirmed, Finalized, Root. Nova Shield'da aynı hierarchy.
- `VOTE_THRESHOLD_SIZE = 2/3` → supermajority. Nova Shield'da PBFT-like commit için benzer threshold.
- `get_lockout_count` → array'in sonundan başa iterasyon, stake kümülatif toplamı threshold geçince döner. O(N) ama N küçük (MAX_LOCKOUT_HISTORY ~32).
- `CommitmentSlots` 4 farklı slot: finalized, confirmed, processed, root. RPC endpoint'ler için farklı consistency guarantee'ler sağlar.

---

### 2.5 runtime/src/prioritization_fee.rs

**Satır sayısı:** 392 satır
**Konum:** `/research/chains/solana/runtime/src/prioritization_fee.rs`

**Private struct (satır 7-36):**
```rust
#[derive(Debug, Default)]
struct PrioritizationFeeMetrics {
    total_writable_accounts_count: u64,
    relevant_writable_accounts_count: u64,
    prioritized_transactions_count: u64,
    non_prioritized_transactions_count: u64,
    attempted_update_on_finalized_fee_count: u64,
    total_prioritization_fee: u64,
    min_prioritization_fee: Option<u64>,
    max_prioritization_fee: u64,
    total_update_elapsed_us: u64,
}
```

**Metotlar:**
- `accumulate_total_prioritization_fee(val)`, `accumulate_total_update_elapsed_us(val)` — Satır 39-46: `saturating_add_assign!`
- `increment_attempted_update_on_finalized_fee_count(val)` — Satır 47.
- `update_prioritization_fee(fee)` — Satır 51: fee=0 → non_prioritized_count++; fee>0 → max/min güncelle, prioritized_count++.
- `report(slot)` — Satır 68: `datapoint_info!` ile `block_prioritization_fee`.

**Enum (satır 121-133):**
```rust
#[derive(Debug)]
pub enum PrioritizationFeeError {
    FailGetTransactionAccountLocks,
    FailGetComputeBudgetDetails,
    BlockIsAlreadyFinalized,
}
```

**Struct (satır 139-153):**
```rust
#[derive(Debug)]
pub struct PrioritizationFee {
    min_transaction_fee: u64,                           // blok içindeki minimum fee
    min_writable_account_fees: HashMap<Pubkey, u64>,   // hesap başına min fee
    is_finalized: bool,
    metrics: PrioritizationFeeMetrics,
}
```

`Default`: `min_transaction_fee = u64::MAX` (ilk update'te her değer daha küçük olacak).

**`update(transaction_fee, writable_accounts) -> Result<(), PrioritizationFeeError>` — Satır 168:**
```
if !is_finalized:
    if fee < min_transaction_fee: update min
    for each write_account: entry(..).and_modify(min).or_insert(fee)
    metrics güncelle
else:
    attempted_update_on_finalized++
```
`measure!` macro ile süre takibi.

**`prune_irrelevant_writable_accounts()` — Satır 207:**
Blok minimum fee'sinden küçük veya eşit account fee'lerini sil. Memory optimizasyonu.

**`mark_block_completed()` — Satır 214:**
`is_finalized = true`, prune çağır.

**Accessor metotlar:**
- `get_min_transaction_fee() -> Option<u64>` — `u64::MAX` ise None (hiç tx yok demek).
- `get_writable_account_fee(key) -> Option<u64>` — hesap min fee.
- `get_writable_account_fees() -> impl Iterator<Item = (&Pubkey, &u64)>`.
- `get_writable_accounts_count() -> usize`.
- `is_finalized() -> bool`.
- `report_metrics(slot)` — block ve top-10 account fee raporu.

**Nova Shield Chain Dersleri:**
- Prioritization fee sistemi: hesap başına minimum fee tracking → kullanıcılara "bu hesabı write etmek için en az X fee öde" bilgisi.
- `u64::MAX` sentinel: "henüz hiç tx yoksa minimum fee bilinmiyor" state'i için.
- `prune_irrelevant_writable_accounts` → block finalize'da memory shrink. Blok kapandıktan sonra account-level detail gerekmez, sadece block minimum kalır.
- `is_finalized` guard: finalize sonrası update → sadece metrics sayacı artırılır, veri değiştirilmez. Immutable after finalization.
- `measure!` macro: kritik path'lerde çift değer return (result, measure). Nova Shield'da timing instrumentation için kullan.

---

### 2.6 runtime/src/prioritization_fee_cache.rs

**Satır sayısı:** 958 satır
**Konum:** `/research/chains/solana/runtime/src/prioritization_fee_cache.rs`

**Sabit (satır 26):**
```rust
const MAX_NUM_RECENT_BLOCKS: u64 = 150;
```
Son 150 blok için fee history tutulur.

**Private struct (satır 28-47):**
```rust
#[derive(Debug, Default)]
struct PrioritizationFeeCacheMetrics {
    successful_transaction_update_count: AtomicU64,
    purged_duplicated_bank_count: AtomicU64,
    total_update_elapsed_us: AtomicU64,
    total_cache_lock_elapsed_us: AtomicU64,
    total_entry_update_elapsed_us: AtomicU64,
    total_block_finalize_elapsed_us: AtomicU64,
}
```

`AtomicU64` — metrics multi-thread safe, lock yok.

**Enum (satır 121-133):**
```rust
enum CacheServiceUpdate {
    TransactionUpdate {
        slot: Slot,
        bank_id: BankId,
        transaction_fee: u64,
        writable_accounts: Arc<Vec<Pubkey>>,
    },
    BankFinalized { slot: Slot, bank_id: BankId },
    Exit,
}
```
Channel message types.

**Tip (satır 137):**
```rust
type SlotPrioritizationFee = DashMap<BankId, PrioritizationFee>;
```
`DashMap` — concurrent hash map (shard-based lock-free).

**Struct (satır 143-148):**
```rust
#[derive(Debug)]
pub struct PrioritizationFeeCache {
    cache: Arc<RwLock<LruCache<Slot, Arc<SlotPrioritizationFee>>>>,
    service_thread: Option<JoinHandle<()>>,
    sender: Sender<CacheServiceUpdate>,
    metrics: Arc<PrioritizationFeeCacheMetrics>,
}
```

Architecture:
- `cache`: LRU cache (150 slot), RwLock ile korunan
- `service_thread`: arka plan thread → channel'dan update alır, işler
- `sender`: main thread → service_thread'e mesaj gönderir (async)
- `metrics`: Arc ile paylaşılan, atomic metrics

**`Drop` impl (satır 156-164):**
```rust
fn drop(&mut self) {
    let _ = self.sender.send(CacheServiceUpdate::Exit);
    self.service_thread.take().unwrap().join().expect("...");
}
```
Graceful shutdown: Exit mesajı gönder, thread'in bitmesini bekle.

**`new(capacity: u64)` — Satır 168:**
```
1. metrics Arc oluştur
2. channel (unbounded) oluştur
3. LruCache oluştur
4. service_thread spawn: "solPrFeeCachSvc"
5. PrioritizationFeeCache döndür
```

**`get_prioritization_fee(cache, slot)` — Satır 193:**
LRU cache'den slot entry al veya yeni oluştur (DashMap).

**`update<'a>(&self, bank, txs)` — Satır 211:**
```
for each non-vote tx:
    get compute_budget_details (cu_price)
    get account_locks
    if cu_limit == 0: skip
    sender.send(TransactionUpdate { slot, bank_id, fee: cu_price, writable_accounts })
```
Async → main thread bloklanmaz.

**`finalize_priority_fee(slot, bank_id)` — Satır 271:**
`sender.send(BankFinalized { slot, bank_id })` → service thread handle eder.

**`update_cache(cache, slot, bank_id, transaction_fee, writable_accounts, metrics)` — Satır 284:**
Service thread içinde:
1. LRU cache'den SlotPrioritizationFee al
2. bank_id entry → PrioritizationFee::update()

**`finalize_slot(cache, slot, bank_id, metrics)` — Satır 309:**
1. Duplicate bank'ları temizle (sadece OC-ed bank kalsın)
2. `mark_block_completed()` → prune + finalize

**`service_loop(cache, receiver, metrics)` — Satır 356:**
```rust
for update in receiver.iter() {
    match update {
        TransactionUpdate { .. } => Self::update_cache(..),
        BankFinalized { .. } => {
            Self::finalize_slot(..);
            metrics.report(slot);
        }
        Exit => break,
    }
}
```

**`available_block_count()` — Satır 389:** Finalize edilmiş blok sayısı.

**`get_prioritization_fees(account_keys: &[Pubkey]) -> HashMap<Slot, u64>` — Satır 402:**
Her finalized slot için max(block_min, account_min) döndür.

**Nova Shield Chain Dersleri:**
- Producer-consumer pattern: main thread mesaj gönderir, service thread LRU cache'i günceller. Lock contention minimize edilir.
- `DashMap<BankId, PrioritizationFee>` → bir slot'ta fork'lar için farklı bank_id'ler. Fork resolution sırasında doğru bank seçilir.
- `LruCache<Slot, ...>` → sadece son 150 slot fee history. Memory footprint sınırlı.
- `unbounded()` channel: backpressure yok, main thread hiç bloklanmaz. Memory spike riski var ama kabul edilebilir.
- Graceful shutdown via Drop: kritik background thread için şart.
- `round_compute_unit_price_enabled = false` — TODO yorum satırı: feature flag henüz eklenmeyi bekliyor.
- Duplicate bank purge: aynı slot için birden fazla bank olabilir; finalize olunca sadece OC-ed bank kalır.
- Nova Shield'da priority fee system: compute_unit_price ile fee belirle, hesap bazında minimum tut, LRU cache ile son N blok history.

---

### 2.7 runtime/src/bank_utils.rs

**Satır sayısı:** 61 satır
**Konum:** `/research/chains/solana/runtime/src/bank_utils.rs`

**Feature flag:**
```rust
#[cfg(feature = "dev-context-only-utils")]
```
`setup_bank_and_vote_pubkeys_for_tests` sadece development feature ile derlenir.

**`setup_bank_and_vote_pubkeys_for_tests(num_vote_accounts, stake) -> (Bank, Vec<Pubkey>)` — Satır 16:**
Test helper: N vote account ile Bank oluştur.

**`find_and_send_votes(sanitized_txs, tx_results, vote_sender: Option<&ReplayVoteSender>)` — Satır 39:**
Production fonksiyon:
```rust
sanitized_txs
    .iter()
    .zip(execution_results.iter())
    .for_each(|(tx, result)| {
        if tx.is_simple_vote_transaction() && result.was_executed_successfully() {
            if let Some(parsed_vote) = vote_parser::parse_sanitized_vote_transaction(tx) {
                if parsed_vote.1.last_voted_slot().is_some() {
                    let _ = vote_sender.send(parsed_vote);
                }
            }
        }
    });
```
Başarılı vote tx'leri parse et ve replay vote channel'a gönder.

**Nova Shield Chain Dersleri:**
- `find_and_send_votes`: transaction execution sonrası vote'ları extract et ve consensus layer'a feed et. Nova Shield'da benzer post-execution hook.
- `vote_sender: Option<...>` → replay stage'e bağlı değilsek None, bağlıysak Some. Optional processing.
- `is_simple_vote_transaction()` + `was_executed_successfully()` double guard: yanlışlıkla başarısız vote tx gönderilmesin.

---

### 2.8 runtime/src/non_circulating_supply.rs

**Satır sayısı:** 332 satır
**Konum:** `/research/chains/solana/runtime/src/non_circulating_supply.rs`

**Struct (satır 14-17):**
```rust
pub struct NonCirculatingSupply {
    pub lamports: u64,
    pub accounts: Vec<Pubkey>,
}
```

**`calculate_non_circulating_supply(bank) -> ScanResult<NonCirculatingSupply>` — Satır 19:**
Algoritma:
1. `non_circulating_accounts()` listesinden `HashSet` oluştur
2. Stake program accounts tara:
   - Index varsa: `get_filtered_indexed_accounts` (hızlı)
   - Yoksa: `get_program_accounts` (yavaş scan)
3. Her stake account için:
   - `StakeStateV2::Initialized(meta)` veya `StakeStateV2::Stake(meta, ..)` → lockup veya withdraw authority kontrolü
4. Non-circulating set'teki hesapların bakiyeleri topla

**`non_circulating_accounts()` — Satır 84:**
`solana_sdk::pubkeys!` macro ile static pubkey listesi. 100+ mainnet-beta hesabı. Foundation, team vesting, genesis allocations.

**`withdraw_authority()` — Satır 201:**
10 withdraw authority pubkey. Bunlar bu hesaplardan withdraw yapabilir ama hala non-circulating.

**Test (satır 217-331):**
- Çeşitli stake account lockup/unlock senaryoları
- Epoch ilerledikçe unlock

**Nova Shield Chain Dersleri:**
- Non-circulating supply hesaplama: stake lockup ve withdraw authority bazlı filtreleme.
- `solana_sdk::pubkeys!` macro: compile-time constant pubkey array. Nova Shield'da genesis allocation adresleri için kullan.
- Index-first lookup: `AccountIndex::ProgramId` varsa indexed scan, yoksa full scan. Performance-aware query pattern.
- `ScanResult` → scan işleminin başarı/başarısızlığı. Bank snapshot sırasında scan başarısız olabilir.

---

### 2.9 runtime/src/root_bank_cache.rs

**Satır sayısı:** 98 satır
**Konum:** `/research/chains/solana/runtime/src/root_bank_cache.rs`

**Struct (satır 14-18):**
```rust
pub struct RootBankCache {
    bank_forks: Arc<RwLock<BankForks>>,
    cached_root_bank: Weak<Bank>,
    root_slot: ReadOnlyAtomicSlot,
}
```

**Tasarım:** `Weak<Bank>` — RootBankCache Bank'ı sahiplenmez; eğer Bank drop olursa cache de güncellenmek zorunda kalır.

**`new(bank_forks) -> Self` — Satır 21:**
```rust
let (cached_root_bank, root_slot) = {
    let lock = bank_forks.read().unwrap();
    (Arc::downgrade(&lock.root_bank()), lock.get_atomic_root())
};
```

**`root_bank(&mut self) -> Arc<Bank>` — Satır 33:**
```rust
match self.cached_root_bank.upgrade() {
    Some(cached_root_bank) if cached_root_bank.slot() == self.root_slot.get() => {
        cached_root_bank  // cache hit — BankForks lock almadan döner
    }
    _ => {
        let root_bank = self.bank_forks.read().unwrap().root_bank();
        self.cached_root_bank = Arc::downgrade(&root_bank);
        root_bank  // cache miss — BankForks'u kilitle
    }
}
```

Cache invalidation: slot değiştiyse veya Weak upgrade başarısız olduysa (Bank drop edildi).

**Nova Shield Chain Dersleri:**
- `Weak<Bank>` cache: bank forks'u kilitlemeden root bank'a erişim. Lock elision pattern.
- `ReadOnlyAtomicSlot` ile atomic slot okuma: lock olmadan current root slot check.
- Cache invalidation: Weak upgrade başarısızlığı = bank drop edildi = cache stale. Otomatik invalidation.
- Bu pattern, hot-path'de RwLock contention'ı minimize eder.

---

### 2.10 runtime/src/snapshot_config.rs

**Satır sayısı:** 99 satır
**Konum:** `/research/chains/solana/runtime/src/snapshot_config.rs`

**Struct (satır 12-49):**
```rust
#[derive(Clone, Debug)]
pub struct SnapshotConfig {
    pub usage: SnapshotUsage,
    pub full_snapshot_archive_interval_slots: Slot,       // DEFAULT: 25_000
    pub incremental_snapshot_archive_interval_slots: Slot, // DEFAULT: 100
    pub full_snapshot_archives_dir: PathBuf,
    pub incremental_snapshot_archives_dir: PathBuf,
    pub bank_snapshots_dir: PathBuf,
    pub archive_format: ArchiveFormat,                    // DEFAULT: TarZstd
    pub snapshot_version: SnapshotVersion,
    pub maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    pub maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
    pub accounts_hash_debug_verify: bool,
    pub packager_thread_niceness_adj: i8,
}
```

**`new_load_only() -> Self` — Satır 77:** `usage: SnapshotUsage::LoadOnly`.
**`should_generate_snapshots() -> bool` — Satır 86:** `LoadAndGenerate` ise true.

**Enum (satır 92-98):**
```rust
pub enum SnapshotUsage {
    LoadOnly,        // Sadece startup'ta yükle
    LoadAndGenerate, // Hem yükle hem üret
}
```

**Nova Shield Chain Dersleri:**
- Full snapshot interval 25,000 slot (~7 dakika), incremental 100 slot (~3 saniye). Nova Shield için benzer timing.
- `TarZstd` format: zstandard compression, iyi compression ratio + hızlı.
- `packager_thread_niceness_adj: i8` — snapshot I/O thread'inin CPU önceliği. Snapshot I/O kritik işleri etkilemesin.
- `NonZeroUsize` retention counts: sıfır geçerli değil, tip sistemi enforce eder.
- `LoadOnly` vs `LoadAndGenerate`: test ortamında sadece load, prod'da her ikisi de.

---

### 2.11 runtime/src/bank_client.rs

**Satır sayısı:** 394 satır
**Konum:** `/research/chains/solana/runtime/src/bank_client.rs`

**Struct (satır 31-34):**
```rust
pub struct BankClient {
    bank: Arc<Bank>,
    transaction_sender: Mutex<Sender<VersionedTransaction>>,
}
```

**Trait implementasyonları:**

`impl Client for BankClient` — Satır 36: `tpu_addr()` → "Local BankClient".

`impl AsyncClient for BankClient` — Satır 42:
- `async_send_versioned_transaction(tx) -> Result<Signature>`: tx'i channel'a gönderir, signature döndürür.

`impl SyncClient for BankClient` — Satır 54: Tüm RPC-benzeri sync metodlar:
- `send_and_confirm_message(keypairs, message) -> Result<Signature>` — Satır 55: blockhash al, sign, process.
- `send_and_confirm_instruction(keypair, instruction)` — Satır 67.
- `transfer_and_confirm(lamports, keypair, pubkey)` — Satır 76.
- `get_account_data`, `get_account`, `get_account_with_commitment` — Satır 88-105.
- `get_balance`, `get_balance_with_commitment` — Satır 107-117.
- `get_minimum_balance_for_rent_exemption` — Satır 119.
- `get_recent_blockhash`, `get_recent_blockhash_with_commitment` — Satır 123-145.
- `get_fee_calculator_for_blockhash` — Satır 147.
- `get_fee_rate_governor` — Satır 154.
- `get_signature_status`, `get_signature_status_with_commitment` — Satır 159-176.
- `get_slot`, `get_slot_with_commitment` — Satır 174-180.
- `get_transaction_count`, `get_transaction_count_with_commitment` — Satır 182-191.
- `poll_for_signature_confirmation` — Satır 193: min_confirmed_blocks=1 zorunlu (BankClient multi-block geçişi göremez).
- `poll_for_signature` — Satır 221: 15 saniye timeout.
- `get_new_blockhash`, `get_epoch_info`, `get_latest_blockhash` — Satır 244-265.
- `get_latest_blockhash_with_commitment` — Satır 267.
- `is_blockhash_valid` — Satır 279.
- `get_fee_for_message` — Satır 287.

**`BankClient::run(bank, transaction_receiver)` — Satır 301:**
Background thread loop:
```rust
while let Ok(tx) = transaction_receiver.recv() {
    let mut transactions = vec![tx];
    while let Ok(tx) = transaction_receiver.try_recv() { // batch collect
        transactions.push(tx);
    }
    let _ = bank.try_process_entry_transactions(transactions);
}
```
Batch collect pattern: ilk tx'i bekle, sonra hızla dolu olan channel'ı drain et.

**`new_shared(bank: Arc<Bank>) -> Self` — Satır 311:** Thread spawn + channel setup.

**`advance_slot` (dev-context-only)** — Satır 334: Test için slot ilerletme.

**Nova Shield Chain Dersleri:**
- `BankClient`, test ortamları için `SyncClient` interface'ini local Bank üzerinde implement eder. Integration test kolaylığı.
- Batch collect pattern: `recv()` ile blokla, `try_recv()` ile hızlıca drain et — batch transaction processing için kullan.
- `poll_for_signature` 250ms aralık, 15s timeout — test ortamı için makul değerler.
- `min_confirmed_blocks=1` assertion: local Bank'ın tek blok perspektifini belgeliyor.
- Nova Shield'da test client interface benzer şekilde tasarlanabilir.

---

### 2.12 runtime/src/loader_utils.rs

**Satır sayısı:** 316 satır
**Konum:** `/research/chains/solana/runtime/src/loader_utils.rs`

**Feature flag:** `#![cfg(feature = "dev-context-only-utils")]` — tüm dosya test-only.

**Sabit (satır 26):** `const CHUNK_SIZE: usize = 512;` — program yükleme chunk boyutu.

**`load_program_from_file(name: &str) -> Vec<u8>` — Satır 28:**
Binary'nin `sbf/` dizininden `.so` dosyası yükle.

**`create_program(bank, loader_id, name) -> Pubkey` — Satır 45:**
Test helper: bypass loader built-in, doğrudan executable account oluştur.

**`load_and_finalize_program<T: Client>(bank_client, loader_id, program_keypair, payer_keypair, name)` — Satır 59:**
1. create_account instruction
2. 512 byte chunk'larla write loop
3. finalize instruction döndür (henüz çalıştırmaz)

**`load_program<T: Client>` — Satır 101:** `load_and_finalize_program` + finalize message gönder.

**`load_upgradeable_buffer<T: Client>(...)` — Satır 116:**
`bpf_loader_upgradeable::create_buffer` + write loop.

**`load_upgradeable_program(bank_client, from, buffer, executable, authority, name)` — Satır 170:**
1. `load_upgradeable_buffer`
2. `deploy_with_max_program_len` (program len * 2 max size)
3. Clock sysvar slot=1 olarak set et (activation delay için)

**`upgrade_program<T: Client>` — Satır 258:** Buffer yükle + upgrade instruction.
**`set_upgrade_authority<T: Client>` — Satır 287:** Authority değiştir.
**`create_invoke_instruction<T: Serialize>` — Satır 307:** Test instruction factory.

**Nova Shield Chain Dersleri:**
- Program deployment pipeline: create → write (chunked) → finalize. Büyük program binary'leri tek tx'e sığmaz, chunk'lanır.
- `program.len() * 2` max size: programı ileride upgrade edebilmek için 2x alan ayrılır.
- Clock slot=1 set etme: loader activation delay simülasyonu. Nova Shield'da program aktivasyonu için benzer delay mekanizması.
- `load_and_finalize_program` separation: finalize instruction'ı caller'a bırakır, başka işler de eklenebilir.

---

### 2.13 runtime/src/installed_scheduler_pool.rs

**Satır sayısı:** 580 satır
**Konum:** `/research/chains/solana/runtime/src/installed_scheduler_pool.rs`

**Trait `InstalledSchedulerPool` — Satır 41:**
```rust
pub trait InstalledSchedulerPool: Send + Sync + Debug {
    fn take_scheduler(&self, context: SchedulingContext) -> InstalledSchedulerBox;
}
```

**Trait `InstalledScheduler` — Satır 100:**
```rust
pub trait InstalledScheduler: Send + Sync + Debug + 'static {
    fn id(&self) -> SchedulerId;
    fn context(&self) -> &SchedulingContext;
    fn schedule_execution<'a>(
        &'a self,
        transaction_with_index: &'a (&'a SanitizedTransaction, usize),
    );
    fn wait_for_termination(
        self: Box<Self>,
        is_dropped: bool,
    ) -> (ResultWithTimings, UninstalledSchedulerBox);
    fn pause_for_recent_blockhash(&mut self);
}
```

**Trait `UninstalledScheduler` — Satır 133:**
```rust
pub trait UninstalledScheduler: Send + Sync + Debug + 'static {
    fn return_to_pool(self: Box<Self>);
}
```

**Tip tanımları (satır 138-143):**
```rust
pub type InstalledSchedulerBox = Box<dyn InstalledScheduler>;
pub type UninstalledSchedulerBox = Box<dyn UninstalledScheduler>;
pub type InstalledSchedulerPoolArc = Arc<dyn InstalledSchedulerPool>;
pub type SchedulerId = u64;
```

**Struct `SchedulingContext` — Satır 154-158):**
```rust
#[derive(Clone, Debug)]
pub struct SchedulingContext {
    bank: Arc<Bank>,
}
```
Bank + slot bilgisi taşır.

**Enum `WaitReason` — Satır 178-193:**
```rust
enum WaitReason {
    TerminatedToFreeze,       // Normal freeze, en yaygın
    DroppedFromBankForks,     // Drop() caller
    PausedForRecentBlockhash, // Pause etmek, sonra devam
}
```

**Tip `ResultWithTimings` — Satır 174:**
```rust
pub type ResultWithTimings = (Result<()>, ExecuteTimings);
```

**Struct `BankWithScheduler` — Satır 233-240:**
```rust
#[derive(Debug)]
pub struct BankWithScheduler {
    inner: Arc<BankWithSchedulerInner>,
}

#[derive(Debug)]
pub struct BankWithSchedulerInner {
    bank: Arc<Bank>,
    scheduler: InstalledSchedulerRwLock,
}
pub type InstalledSchedulerRwLock = RwLock<Option<InstalledSchedulerBox>>;
```

**Kritik metotlar:**

- `new(bank, scheduler: Option<InstalledSchedulerBox>)` — Satır 246: Bank ve scheduler context'inin uyumluluğunu assert ile kontrol et.
- `schedule_transaction_executions<'a>(...)` — Satır 290: RwLock read, tüm tx'leri scheduler'a schedule et.
- `wait_for_paused_scheduler` — Satır 313: `PausedForRecentBlockhash` ile bekle.
- `wait_for_completed_scheduler()` — Satır 326: `TerminatedToFreeze` ile bekle.

**`BankWithSchedulerInner::wait_for_scheduler_termination(bank, scheduler, reason)` — Satır 350:**
```
if reason.is_paused():
    scheduler.pause_for_recent_blockhash() → None döner
elif scheduler mevcut:
    scheduler.wait_for_termination(is_dropped) → (result, uninstalled)
    uninstalled.return_to_pool()
    Some(result) döner
else:
    None döner
```

**Drop impl (satır 407-410):**
```rust
impl Drop for BankWithSchedulerInner {
    fn drop(&mut self) { self.drop_scheduler(); }
}
```
Scheduler drop → wait_for_termination(is_dropped=true) → pool'a geri ver.

**Deref impl (satır 413-418):**
```rust
impl Deref for BankWithScheduler {
    type Target = Arc<Bank>;
    fn deref(&self) -> &Self::Target { &self.inner.bank }
}
```
`BankWithScheduler`, `Arc<Bank>` gibi davranır.

**Nova Shield Chain Dersleri:**
- Dynamic dispatch: `dyn InstalledScheduler`, `dyn InstalledSchedulerPool` — cyclic dependency çözümü için. `solana-runtime` → `solana-unified-scheduler-pool` dependency'den kaçınmak için trait object.
- `BankWithScheduler` thin wrapper: `Arc<Bank>` + optional scheduler. Deref ile Bank gibi kullanılır.
- `WaitReason` enum: pause vs terminate ayrımı. Pause: scheduler durur ama return edilmez (blockhash update için). Terminate: scheduler pool'a döner.
- `return_to_pool()` → scheduler lifecycle management, pool pattern.
- `schedule_execution<'a>(&'a self, (&'a SanitizedTransaction, usize))` — lifetime: self ve transaction aynı lifetime'da. Batch'in tamamı schedule edilene kadar transaction'lar yaşıyor.
- `InstalledSchedulerRwLock = RwLock<Option<InstalledSchedulerBox>>` — scheduler değişebilir, thread-safe.
- Nova Shield'da execution pipeline: scheduler pool → BankWithScheduler → schedule tx → wait for result.

---

### 2.14 runtime/src/serde_snapshot.rs

**Satır sayısı:** 956 satır
**Konum:** `/research/chains/solana/runtime/src/serde_snapshot.rs`

**Alt modüller (satır 57-59):**
```rust
mod newer;   // V1.2.0 format context
mod storage; // AppendVec serialization
mod tests;
mod utils;
```

**Enum (satır 69-72):**
```rust
#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum SerdeStyle {
    Newer,  // Tek desteklenen format şu an
}
```

**Sabit (satır 74):**
```rust
const MAX_STREAM_SIZE: u64 = 32 * 1024 * 1024 * 1024; // 32 GB
```

**Struct `AccountsDbFields<T>` — Satır 77:**
```rust
#[derive(Clone, Debug, Deserialize, Serialize, AbiExample, PartialEq, Eq)]
pub struct AccountsDbFields<T>(
    HashMap<Slot, Vec<T>>,        // slot → storage entries
    StoredMetaWriteVersion,       // write version
    Slot,                         // current slot
    BankHashInfo,                 // hash info
    #[serde(deserialize_with = "default_on_eof")] Vec<Slot>,          // historical roots
    #[serde(deserialize_with = "default_on_eof")] Vec<(Slot, Hash)>,  // roots with hash
);
```

**Struct `BankIncrementalSnapshotPersistence` — Satır 99:**
```rust
pub struct BankIncrementalSnapshotPersistence {
    pub full_slot: Slot,
    pub full_hash: SerdeAccountsHash,
    pub full_capitalization: u64,
    pub incremental_hash: SerdeIncrementalAccountsHash,
    pub incremental_capitalization: u64,
}
```
Full + incremental snapshot'ın hash ve capitalization bilgisi.

**Struct `BankHashInfo` — Satır 112:**
```rust
struct BankHashInfo {
    accounts_delta_hash: SerdeAccountsDeltaHash,
    accounts_hash: SerdeAccountsHash,
    stats: BankHashStats,
}
```

**Struct `SnapshotStreams<'a, R>` — Satır 121:**
```rust
pub struct SnapshotStreams<'a, R> {
    pub full_snapshot_stream: &'a mut BufReader<R>,
    pub incremental_snapshot_stream: Option<&'a mut BufReader<R>>,
}
```

**Struct `SnapshotBankFields` — Satır 128:**
```rust
pub struct SnapshotBankFields {
    full: BankFieldsToDeserialize,
    incremental: Option<BankFieldsToDeserialize>,
}
impl SnapshotBankFields {
    pub fn collapse_into(self) -> BankFieldsToDeserialize {
        self.incremental.unwrap_or(self.full)
    }
}
```

**Struct `SnapshotAccountsDbFields<T>` — Satır 143:**
Full + incremental storage entries. `collapse_into()`:
- Incremental yoksa → full snapshot fields
- Incremental varsa → incremental slot'larını filtrele (full_slot'tan büyük olanlar), birleştir, overlap check

**Trait `TypeContext<'a>` — Satır 194:**
```rust
trait TypeContext<'a>: PartialEq {
    type SerializableAccountStorageEntry: ...;
    fn serialize_bank_and_storage<S: serde::ser::Serializer>(...);
    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(...);
    fn deserialize_bank_fields<R>(stream: ...) -> Result<(BankFieldsToDeserialize, AccountsDbFields<...>), Error>;
    fn deserialize_accounts_db_fields<R>(stream: ...) -> Result<AccountsDbFields<...>, Error>;
    fn reserialize_bank_fields_with_hash<R, W>(...);
}
```

**Ana fonksiyonlar:**

- `deserialize_from<R, T>(reader)` — Satır 255: bincode config ile `MAX_STREAM_SIZE`, fixint encoding, allow trailing.
- `fields_from_stream(serde_style, stream)` — Satır 304: Dispatch to `newer::Context::deserialize_bank_fields`.
- `fields_from_streams(serde_style, streams)` — Satır 319: Full + optional incremental deserialization.
- `bank_from_streams(serde_style, streams, ...)` — Satır 351: Full bank reconstruction from streams.
- `bank_to_stream(serde_style, stream, bank, snapshot_storages)` — Satır 391: Bank + storage serialize.
- `reserialize_bank_with_new_accounts_hash(bank_snapshot_dir, slot, accounts_hash, incremental_persistence)` — Satır 460: Pre file → hash update → post file.
- `reconstruct_bank_from_fields(bank_fields, snapshot_accounts_db_fields, ...)` — Satır 578: Fields → Bank.

**Nova Shield Chain Dersleri:**
- Snapshot format versioning: `SerdeStyle::Newer` enum — gelecekte `Older` formatlar eklenebilir, backward compat.
- `MAX_STREAM_SIZE = 32 GB`: snapshot sonsuz büyüyemez, DOS koruması.
- `default_on_eof` serde attribute: eski snapshot formatlarında olmayan yeni field'lar için backward compat.
- Two-phase snapshot: full + incremental. Nova Shield'da aynı incremental snapshot mimarisi.
- `collapse_into()` pattern: full + incremental → tek canonical state. Overlap check kritik.
- `reserialize_bank_with_new_accounts_hash`: snapshot'a accounts hash yazılmıştır, sonradan update için file'ı rewrite et.
- `bincode` + `with_limit` + `with_fixint_encoding` + `allow_trailing_bytes`: space-efficient, forward-compat serialization.

---

### 2.15 runtime/src/snapshot_bank_utils.rs

**Satır sayısı:** 2641 satır
**Konum:** `/research/chains/solana/runtime/src/snapshot_bank_utils.rs`

**Sabitler (satır 60-62):**
```rust
pub const DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 25_000;
pub const DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 100;
pub const DISABLED_SNAPSHOT_ARCHIVE_INTERVAL: Slot = Slot::MAX;
```

**Struct `BankFromArchivesTimings` — Satır 210:**
```rust
pub struct BankFromArchivesTimings {
    pub untar_full_snapshot_archive_us: u64,
    pub untar_incremental_snapshot_archive_us: u64,
    pub rebuild_bank_us: u64,
    pub verify_bank_us: u64,
}
```

**Struct `BankFromDirTimings` — Satır 217:**
```rust
pub struct BankFromDirTimings {
    pub rebuild_storages_us: u64,
    pub rebuild_bank_us: u64,
}
```

**`add_bank_snapshot(bank_snapshots_dir, bank, snapshot_storages, snapshot_version, slot_deltas)` — Satır 70:**
Kritik fonksiyon — node çökmemeli! Adımlar:
1. Snapshot dir kontrolü (zaten var mı?)
2. `fs::create_dir_all(bank_snapshot_dir)`
3. `hard_link_storages_to_snapshot` → account storage dosyaları hard-link ile kopyalanır
4. `bank_to_stream` → bank state serialize
5. `serialize_status_cache` → status cache serialize
6. Version file yaz
7. State complete flag file oluştur (atomik completion marker)
8. `datapoint_info!` ile metrics

**`serialize_status_cache(slot_deltas, status_cache_path)` — Satır 199:**
Bincode ile slot delta'ları serialize.

**`bank_fields_from_snapshot_archives(full_dir, incremental_dir)` — Satır 225:**
En yüksek snapshot archive'larından bank fields parse et.

**`bank_from_snapshot_archives(account_paths, bank_snapshots_dir, full_info, incremental_info, ...)` — Satır 265:**
Pipeline:
1. `verify_and_unarchive_snapshots` → storage'ları untar
2. Storage'ları birleştir
3. `rebuild_bank_from_unarchived_snapshots`
4. `verify_bank_against_expected_slot_hash`
5. `bank.verify_snapshot_bank` (hash verification)
6. Timings döndür

**`bank_from_latest_snapshot_archives(...)` — Satır 415:**
En yüksek full + uygun incremental'ı otomatik bul ve yükle.

**`bank_from_snapshot_dir(account_paths, bank_snapshot, ...)` — Satır 479:**
Archive değil, dizinden bank yükle.

**Nova Shield Chain Dersleri:**
- `add_bank_snapshot` failure = node shutdown. Error handling çok kritik; her hata `AddBankSnapshotError` ile wrap.
- Hard-link strategy: account storage dosyaları kopyalanmaz, hard-link edilir. Disk IO minimize.
- State complete flag: atomic completion marker pattern. Okuyucular bu flag'i görene kadar snapshot'ı geçersiz sayar.
- `DISABLED_SNAPSHOT_ARCHIVE_INTERVAL = Slot::MAX`: snapshot'ı devre dışı bırakmak için özel sentinel.
- `verify_snapshot_bank`: hash doğrulama. Snapshot corrupt olursa node panic ile kapanır.
- Full + incremental split: 25,000 slot full → 100 slot incremental. Recovery point daha sık, full rebuild daha az.
- Nova Shield snapshot sistemi: aynı iki-katmanlı yapı, zstd compression, hard-link optimization, atomic completion flag.

---

## BÖLÜM 3: KAPSAMLI TASARIM ANALİZİ

### 3.1 SVM Mimarisi Özeti

```
TransactionBatchProcessor<FG: ForkGraph>
│
├── load_and_execute_sanitized_transactions() [main entry]
│   ├── filter_executable_program_accounts()
│   ├── replenish_program_cache()
│   ├── load_accounts() [LOAD PHASE]
│   │   ├── validate_fee_payer()
│   │   ├── load_transaction_accounts()
│   │   │   ├── check sysvar::instructions → construct in-memory
│   │   │   ├── check program cache → stub data (optimization)
│   │   │   ├── check account_overrides → override
│   │   │   └── callbacks.get_account_shared_data() → load from storage
│   │   └── NonceFull::from_partial()
│   └── execute_loaded_transaction() per tx [EXECUTE PHASE]
│       ├── TransactionAccountStateInfo::new() → pre_state
│       ├── MessageProcessor::process_message()
│       └── TransactionAccountStateInfo::verify_changes() → post_state
│
└── TransactionProcessingCallback [trait]
    ← implemented by Bank
```

### 3.2 Transaction Lifecycle

```
SanitizedTransaction
→ [CHECK PHASE: bank.rs]
  check_age, check_status, check_fee
  TransactionCheckResult: (Ok|Err, Option<NoncePartial>, Option<lamports_per_sig>)

→ [LOAD PHASE: account_loader.rs]
  validate_fee_payer → subtract fee
  load_transaction_accounts → all accounts + program indices
  TransactionLoadResult: (Ok<LoadedTransaction>|Err, Option<NonceFull>)

→ [EXECUTE PHASE: transaction_processor.rs]
  execute_loaded_transaction
  → pre_rent_state capture
  → MessageProcessor::process_message → BPF execution
  → post_rent_state verify
  → collect inner_instructions, logs, return_data
  TransactionExecutionResult::Executed { details, programs_modified }

→ [COMMIT PHASE: bank.rs]
  commit_transactions
  → update account states
  → update bank hash
  → emit rent_debits
```

### 3.3 Rent State Machine

```
                   lamports += 0 AND data_size unchanged
          ┌─────────────────────────────────────────────┐
          │                                             │
          ▼                                             │
  [Uninitialized] ──→ FORBIDDEN ──→ [RentPaying]  ─────┘
       lamports=0        ↑        lamports>0 AND
                         │        < rent-exempt min
                    [RentExempt] ──────────────────→ [RentPaying] FORBIDDEN
                    lamports >=
                    rent-exempt-min
```

### 3.4 Program Cache Mimarisi

```
ProgramCache<FG: ForkGraph>
│
├── LoadedProgram (type: BuiltinFunction | BpfLoaderV1 | ... | LoaderV4)
│   ├── program_runtime_environment: Arc<ProgramRuntimeEnvironment>
│   ├── account_size: usize
│   └── ...
│
└── LoadedProgramsForTxBatch (per-batch view)
    ├── find(pubkey) → Option<Arc<LoadedProgram>>
    └── hit_max_limit: bool
```

### 3.5 Snapshot Architecture

```
Bank State
│
├── Full Snapshot (every 25,000 slots)
│   ├── bank_to_stream() → bincode binary
│   ├── hard_link_storages_to_snapshot()
│   ├── serialize_status_cache()
│   └── state_complete marker file
│
└── Incremental Snapshot (every 100 slots)
    ├── Only accounts changed since last full snapshot
    └── BankIncrementalSnapshotPersistence
        (full_hash, full_capitalization, incremental_hash, incremental_capitalization)

Recovery:
  unarchive full + incremental
  → merge storages (incremental slots > full slot)
  → verify no slot overlap
  → reconstruct_bank_from_fields()
  → verify_snapshot_bank() [hash check]
```

---

## BÖLÜM 4: NOVA SHIELD CHAIN İÇİN UYGULANACAK DERSLER

### 4.1 SVM Layer Tasarımı

1. **Dependency Inversion:** `TransactionProcessingCallback` trait → execution engine, runtime'dan tamamen bağımsız. Nova Shield'da `RuntimeCallback` trait implementasyonu şart.

2. **Two-Phase Execution:** LOAD → EXECUTE ayrımı. Load başarısızlığında execution denenmez, fee gene alınır. Nova Shield'da aynı ayrım.

3. **Pre/Post State Verification:** `TransactionAccountStateInfo::new()` → execution → `verify_changes()`. State mutation validation için şablon.

4. **Rent State FSM:** 3-state: Uninitialized, RentPaying, RentExempt. Geçiş kuralları katı. Nova Shield'da account lifecycle için adapt et.

5. **Program Account Optimization:** Sadece top-level program hesabı + cache'de mevcut → gerçek account yükleme yapma, stub döndür. I/O optimizasyonu.

6. **Error Metrics:** `TransactionErrorMetrics` 23 kategoride hata sayacı. Nova Shield'da aynı granülaritede error taxonomy.

7. **Account Overrides:** Transaction simulation için override layer. Nova Shield RPC simulation endpoint'i için şart.

### 4.2 Runtime Layer Tasarımı

1. **EpochStakes:** Epoch başında stake snapshot al, epoch boyunca değiştirme. `Arc<StakesEnum>` ile paylaş.

2. **Commitment Levels:** 4 seviye: Processed → Confirmed → Finalized → Root. RPC endpoint'lerin farklı consistency guarantee'leri için.

3. **Prioritization Fee Cache:** LRU (150 slot) + async service thread + DashMap (bank_id bazlı). Priority fee estimation için.

4. **Builtin Programs:** `BuiltinPrototype` pattern, `feature_id: Option` ile feature-gated activation.

5. **Snapshot System:**
   - Hard-link storage files (disk I/O minimize)
   - Atomic completion flag (state_complete file)
   - Full (25,000 slot) + Incremental (100 slot) two-tier
   - `CollapsedInto` pattern: full + incremental → canonical state
   - Hash verification on restore

6. **Scheduler Pool:** `InstalledSchedulerPool` trait + dynamic dispatch. Cyclic dependency avoid için.

7. **RootBankCache:** `Weak<Bank>` + `ReadOnlyAtomicSlot` → lock-free cache hit path.

### 4.3 Kritik Güvenlik Noktaları

1. **Fee Payer Validation:** Sistem program account veya nonce account olmayan hesap fee payer olamaz.

2. **RentPaying + Credit Forbidden:** Hesap kredi aldıktan sonra hala rent-paying ise → `InsufficientFundsForRent` hatası.

3. **Loaded Accounts Data Size Limit:** ComputeBudget instruction ile belirlenir, yükleme sırasında running sum kontrolü.

4. **Snapshot Overlap Check:** Full + incremental storage'larda aynı slot'ta entry olmamalı.

5. **Incinerator Exception:** Rent state validation'dan muaf.

6. **program_execution_temporarily_restricted:** Cluster maintenance mode guard.

### 4.4 Performance Patterns

1. `measure!` macro — hot-path timing, production overhead minimal.
2. `AtomicU64` metrics — lock-free counter update.
3. `DashMap` — shard-based concurrent HashMap.
4. `LruCache` — bounded memory footprint.
5. `Weak<Bank>` — Arc cycle prevention + lazy update.
6. Batch collect pattern: `recv()` → `try_recv()` drain loop.
7. Hard-link vs copy: disk I/O reduction for snapshots.
8. `default_on_eof` serde: backward-compat snapshot format evolution.

---

## BÖLÜM 5: DOSYA REFERANS TABLOSU

| Dosya | Satır | Kritik Yapılar | Önem |
|-------|-------|----------------|------|
| svm/src/lib.rs | 17 | module declarations | Orta |
| svm/src/account_overrides.rs | 63 | AccountOverrides | Yüksek |
| svm/src/account_rent_state.rs | 310 | RentState enum, transition rules | ÇOK YÜKSEK |
| svm/src/transaction_account_state_info.rs | 238 | TransactionAccountStateInfo | Yüksek |
| svm/src/transaction_error_metrics.rs | 177 | TransactionErrorMetrics (23 cat) | Yüksek |
| svm/src/transaction_results.rs | 107 | TransactionExecutionResult enum | ÇOK YÜKSEK |
| svm/src/account_loader.rs | ~450 | LoadedTransaction, validate_fee_payer, load_accounts | KRİTİK |
| svm/src/transaction_processor.rs | ~950 | TransactionBatchProcessor, TransactionProcessingCallback | KRİTİK |
| runtime/src/epoch_stakes.rs | 228 | EpochStakes, NodeVoteAccounts | Yüksek |
| runtime/src/genesis_utils.rs | 285 | GenesisConfigInfo, ValidatorVoteKeypairs | Orta |
| runtime/src/builtins.rs | 110 | BuiltinPrototype, BUILTINS static | Yüksek |
| runtime/src/commitment.rs | 345 | BlockCommitmentCache, CommitmentSlots | ÇOK YÜKSEK |
| runtime/src/prioritization_fee.rs | 392 | PrioritizationFee, PrioritizationFeeError | Yüksek |
| runtime/src/prioritization_fee_cache.rs | 958 | PrioritizationFeeCache, CacheServiceUpdate | ÇOK YÜKSEK |
| runtime/src/bank_utils.rs | 61 | find_and_send_votes | Orta |
| runtime/src/non_circulating_supply.rs | 332 | calculate_non_circulating_supply | Orta |
| runtime/src/root_bank_cache.rs | 98 | RootBankCache (Weak pattern) | Yüksek |
| runtime/src/snapshot_config.rs | 99 | SnapshotConfig, SnapshotUsage | Yüksek |
| runtime/src/bank_client.rs | 394 | BankClient (SyncClient impl) | Orta |
| runtime/src/loader_utils.rs | 316 | Program loading utilities | Orta |
| runtime/src/installed_scheduler_pool.rs | 580 | InstalledScheduler trait, BankWithScheduler | ÇOK YÜKSEK |
| runtime/src/serde_snapshot.rs | 956 | AccountsDbFields, SnapshotStreams, bank_from_streams | KRİTİK |
| runtime/src/snapshot_bank_utils.rs | 2641 | add_bank_snapshot, bank_from_snapshot_archives | KRİTİK |

---

*Analiz tamamlandı. Toplam 23 dosya, ~8,000+ satır analiz edildi.*
