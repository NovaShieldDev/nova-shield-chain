# Solana Analiz — Agent 28
## Kapsam: cost-model/ + unified-scheduler-logic/ + unified-scheduler-pool/ + measure/ + metrics/

**Toplam Satır:** ~2.650 satır (5 crate, 12 dosya)
**Analiz Tarihi:** 2026-07-22

---

## 1. cost-model/src/

### [lib.rs] (11 satır)

**Amaç:** Cost-model crate'inin kök modül tanımı.

**Modüller:**
- `block_cost_limits` — blok düzeyindeki sabit limitler
- `cost_model` — transaction maliyet hesaplama mantığı
- `cost_tracker` — blok düzeyinde birikimli maliyet takibi
- `transaction_cost` — maliyet türleri (enum + struct)

**Özellikler:**
- `#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]` — ABI özelleştirme özelliği koşullu olarak etkinleştiriliyor
- `#![allow(clippy::arithmetic_side_effects)]` — saturating aritmetik yaygın kullanıldığından
- `#[macro_use] extern crate solana_frozen_abi_macro` — ABI serileştirme makroları

**Nova Shield İçin Öğrenilen:**
- Maliyet modeli ayrı bir crate olarak izole edilmiş; Nova Shield de bunu ayrı bir crate olarak tasarlamalı
- `frozen_abi_macro` sayesinde ABI değişiklikleri compile-time'da yakalanıyor; bu pattern kritik

---

### [block_cost_limits.rs] (88 satır)

**Amaç:** Blok düzeyindeki tüm sabit maliyet limitlerini ve birim dönüşüm oranlarını tanımlar.

**Önemli Sabitler:**

| Sabit | Değer | Açıklama |
|---|---|---|
| `MAX_BLOCK_REPLAY_TIME_US` | 400_000 | Bir bloğun replay edilmesi için izin verilen max mikrosaniye (400ms) |
| `MAX_CONCURRENCY` | 4 | Paralel işlem sayısı (sanal CPU mantığı) |
| `COMPUTE_UNIT_TO_US_RATIO` | 30 | 1 compute unit = 30 mikrosaniye (cluster telemetri verisi) |
| `SIGNATURE_COST` | 720 (30 × 24) | Her imza doğrulama için compute unit |
| `SECP256K1_VERIFY_COST` | 6_690 (30 × 223) | secp256k1 imza doğrulama CU maliyeti |
| `ED25519_VERIFY_COST` | 2_280 (30 × 76) | ed25519 imza doğrulama CU maliyeti |
| `WRITE_LOCK_UNITS` | 300 (30 × 10) | Her writable account kilidi başına CU |
| `INSTRUCTION_DATA_BYTES_COST` | 4 (140/30) | Her byte instruction data başına CU |
| `MAX_BLOCK_UNITS` | 48_000_000 | = 400_000 × 30 × 4 — toplam blok CU limiti |
| `MAX_WRITABLE_ACCOUNT_UNITS` | 12_000_000 | = 400_000 × 30 — tek account başına CU limiti |
| `MAX_VOTE_UNITS` | 36_000_000 | = MAX_BLOCK_UNITS × 0.75 — vote tx'ler için %75 |
| `MAX_BLOCK_ACCOUNTS_DATA_SIZE_DELTA` | 100_000_000 | Blok başına max account data artışı (100MB) |

**`BUILT_IN_INSTRUCTION_COSTS` (lazy_static HashMap):**
Built-in programların CU maliyetleri statik olarak tanımlanmış:
- `solana_stake_program` → `DEFAULT_COMPUTE_UNITS`
- `solana_config_program` → `DEFAULT_COMPUTE_UNITS`
- `solana_vote_program` → `DEFAULT_COMPUTE_UNITS`
- `solana_system_program` → `DEFAULT_COMPUTE_UNITS`
- `compute_budget` → `DEFAULT_COMPUTE_UNITS`
- `address_lookup_table` → `DEFAULT_COMPUTE_UNITS`
- `bpf_loader_upgradeable` → `UPGRADEABLE_LOADER_COMPUTE_UNITS`
- `bpf_loader_deprecated` → `DEPRECATED_LOADER_COMPUTE_UNITS`
- `bpf_loader` → `DEFAULT_LOADER_COMPUTE_UNITS`
- `loader_v4` → `DEFAULT_COMPUTE_UNITS`
- `secp256k1_program` → **0** (precompile, bank sanitize aşamasında çalışır)
- `ed25519_program` → **0** (precompile, bank sanitize aşamasında çalışır)

**Kritik Formüller:**
```
MAX_BLOCK_UNITS = MAX_BLOCK_REPLAY_TIME_US × COMPUTE_UNIT_TO_US_RATIO × MAX_CONCURRENCY
               = 400_000 × 30 × 4
               = 48_000_000 CU

MAX_WRITABLE_ACCOUNT_UNITS = MAX_BLOCK_REPLAY_TIME_US × COMPUTE_UNIT_TO_US_RATIO
                           = 400_000 × 30
                           = 12_000_000 CU

MAX_VOTE_UNITS = MAX_BLOCK_UNITS × 0.75
              = 36_000_000 CU
```

**Static Assertion:**
- `static_assertions::const_assert_eq!(MAX_BLOCK_UNITS, 48_000_000)` — derleme zamanında doğrulama
- `static_assertions::const_assert_eq!(MAX_WRITABLE_ACCOUNT_UNITS, 12_000_000)`
- `static_assertions::const_assert_eq!(MAX_VOTE_UNITS, 36_000_000)`

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Maliyet birimleri gerçek cluster telemetri verilerinden türetilmiş; Nova Shield de bu ölçümleri kendi validatörlerinden toplamalı
- secp256k1 ve ed25519 precompile'lar için CU = 0 ataması önemli: bu programlar consensus değil, sanitize aşamasında çalışıyor
- `%75 vote rezervasyonu` kritik: eğer vote tx'lere limit konmazsa vote flood ile blok dolabilir
- Account başına `MAX_WRITABLE_ACCOUNT_UNITS` limiti, tek bir account'a aşırı yük bindirmeyi önleyen backpressure mekanizması

---

### [transaction_cost.rs] (259 satır)

**Amaç:** Transaction maliyet türlerini tanımlar: `TransactionCost` (enum) ve `UsageCostDetails` (struct).

**Enum/Struct Listesi:**

**`TransactionCost` (satır 14-17):**
```rust
pub enum TransactionCost {
    SimpleVote { writable_accounts: Vec<Pubkey> },
    Transaction(UsageCostDetails),
}
```
- İki kategori: `SimpleVote` (sabit maliyet) ve `Transaction` (dinamik maliyet)
- `SimpleVote` sadece writable_accounts tutar; maliyet statik olarak 3428 CU

**`SIMPLE_VOTE_USAGE_COST = 3428` (satır 11):**
Hesabı compile-time assert ile doğrulanmış:
```
vote_program::DEFAULT_COMPUTE_UNITS + SIGNATURE_COST + 2 × WRITE_LOCK_UNITS + 8
```
- +8 = loaded_accounts_data_size_cost (simple vote < 32K = 1 sayfa = 8 CU)

**`UsageCostDetails` (satır 121-132):**
```rust
pub struct UsageCostDetails {
    pub writable_accounts: Vec<Pubkey>,       // writable account listesi
    pub signature_cost: u64,                   // imza doğrulama CU
    pub write_lock_cost: u64,                  // write lock CU
    pub data_bytes_cost: u64,                  // instruction data boyutu CU
    pub programs_execution_cost: u64,          // program çalışma CU
    pub loaded_accounts_data_size_cost: u64,   // yüklenen account data CU
    pub account_data_size: u64,                // hesap data büyümesi (bytes)
    pub num_transaction_signatures: u64,       // tx imza sayısı
    pub num_secp256k1_instruction_signatures: u64,
    pub num_ed25519_instruction_signatures: u64,
}
```

**`UsageCostDetails::sum()` (satır 188-194):**
```rust
pub fn sum(&self) -> u64 {
    self.signature_cost
        .saturating_add(self.write_lock_cost)
        .saturating_add(self.data_bytes_cost)
        .saturating_add(self.programs_execution_cost)
        .saturating_add(self.loaded_accounts_data_size_cost)
}
```
NOT: `account_data_size` sum'a dahil DEĞİL — bu ayrı bir limit mekanizması
Tüm toplama işlemleri `saturating_add` ile overflow-safe

**`Default::default()` (satır 134-149):**
- `writable_accounts: Vec::with_capacity(MAX_WRITABLE_ACCOUNTS)` — `MAX_WRITABLE_ACCOUNTS = 256`
- Diğer tüm alanlar: 0u64

**Test Coverage (satır 197-258):**
- `test_vote_transaction_cost()`: SimpleVote ile Transaction arasındaki maliyet farkını doğrular
  - SimpleVote: 3428 CU
  - Aynı tx non-vote olarak: 20535 CU (loaded_accounts_data_size_cost farkı)

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Vote tx'leri için statik maliyet (3428 CU) büyük bir optimizasyon; Nova Shield'in validator-vote mekanizmasında da buna benzer bir statik hesap yapılabilir
- `account_data_size` sum'a dahil edilmiyor — bu ayrı bir quota mekanizması (block-level data growth limit); ikisi birbirine karıştırılmamalı
- `MAX_WRITABLE_ACCOUNTS = 256` kapasitesiyle başlayan Vec — memory pre-allocation pattern

---

### [cost_model.rs] (714 satır)

**Amaç:** `SanitizedTransaction` için compute unit maliyetini hesaplayan ana servis.

**Struct Listesi:**
- `CostModel` (satır 32): Boş struct, tüm metodlar `impl CostModel` altında

**Kritik Fonksiyonlar:**

**`CostModel::calculate_cost()` (satır 35-54) — Ana giriş noktası:**
```rust
pub fn calculate_cost(
    transaction: &SanitizedTransaction,
    feature_set: &FeatureSet,
) -> TransactionCost {
    if transaction.is_simple_vote_transaction() {
        TransactionCost::SimpleVote {
            writable_accounts: Self::get_writable_accounts(transaction),
        }
    } else {
        let mut tx_cost = UsageCostDetails::new_with_default_capacity();
        Self::get_signature_cost(&mut tx_cost, transaction);
        Self::get_write_lock_cost(&mut tx_cost, transaction, feature_set);
        Self::get_transaction_cost(&mut tx_cost, transaction, feature_set);
        tx_cost.account_data_size = Self::calculate_account_data_size(transaction);
        TransactionCost::Transaction(tx_cost)
    }
}
```
SimpleVote → statik yol; diğer tx'ler → 4 ayrı hesaplama adımı

**`get_signature_cost()` (satır 56-76):**
- `message.get_signature_details()` — tx imza detaylarını alır
- `signature_cost = num_tx_sigs × SIGNATURE_COST + num_secp256k1 × SECP256K1_VERIFY_COST + num_ed25519 × ED25519_VERIFY_COST`
- Tüm çarpma işlemleri `saturating_mul` ile

**`get_write_lock_cost()` (satır 94-107):**
- Feature flag `cost_model_requested_write_lock_cost` etkinse: `transaction.message().num_write_locks()` kullanılır
- Aksi halde: `writable_accounts.len()` kullanılır (eski davranış)
- `write_lock_cost = WRITE_LOCK_UNITS × num_write_locks`
- ÖNEMLİ: Demote edilmiş write lock'lar (örn. system_program) eskiden sayılmıyordu; feature flag ile artık sayılıyor

**`get_transaction_cost()` (satır 109-176) — En karmaşık fonksiyon:**
```rust
for (program_id, instruction) in transaction.message().program_instructions_iter() {
    let ix_execution_cost = if let Some(builtin_cost) = BUILT_IN_INSTRUCTION_COSTS.get(program_id) {
        *builtin_cost
    } else {
        has_user_space_instructions = true;
        u64::from(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT)  // default: 200_000
    };

    programs_execution_costs = programs_execution_costs
        .saturating_add(ix_execution_cost)
        .min(u64::from(MAX_COMPUTE_UNIT_LIMIT));  // 1_400_000 cap

    data_bytes_len_total += instruction.data.len() as u64;
    // check if SetComputeUnitLimit ix var mı
}

// compute_budget process sonucu
match process_compute_budget_instructions(...) {
    Ok(limits) => {
        // has_user_space_instructions && compute_unit_limit_is_set ise
        // → programs_execution_costs = compute_budget_limits.compute_unit_limit
        if feature_set.is_active(&include_loaded_accounts_data_size_in_fee_calculation) {
            loaded_accounts_data_size_cost = FeeStructure::calculate_memory_usage_cost(...)
        }
    }
    Err(_) => programs_execution_costs = 0  // hatalı compute_budget = 0 CU
}
```
**Kritik tasarım kararı:** Kullanıcı-defined instruction varsa ve `SetComputeUnitLimit` belirlediyse → o değer kullanılır; built-in'ler asla override edilmez.

**`calculate_account_data_size()` (satır 221-229):**
- Sadece `system_program` instruction'larına bakar (CreateAccount, Allocate vb.)
- Diğer instruction'lar için 0 döner — şimdilik sadece yeni hesap oluşturma takip ediliyor

**Test Coverage (satır 232-713):**
12 test fonksiyonu:
- `test_cost_model_data_len_cost`: CreateAccount, Allocate vb. için account data size doğrulama
- `test_cost_model_simple_transaction`: Tek system transfer maliyeti
- `test_cost_model_token_transaction`: Bilinmeyen program → DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
- `test_cost_model_demoted_write_lock`: Feature flag öncesi/sonrası demoted lock davranışı
- `test_cost_model_compute_budget_transaction`: SetComputeUnitLimit override davranışı
- `test_cost_model_with_failed_compute_budget_transaction`: Duplicate SetComputeUnitLimit → programs_execution_cost = 0
- `test_cost_model_transaction_many_transfer_instructions`: Çoklu builtin instruction toplamı
- `test_cost_model_message_many_different_instructions`: İki bilinmeyen program → 2 × DEFAULT
- `test_cost_model_sort_message_accounts_by_type`: Writable accounts sıralaması
- `test_cost_model_calculate_cost_all_default`: Tüm feature'lar etkin haliyle tam maliyet
- `test_cost_model_calculate_cost_disabled_feature`: loaded_accounts_data_size feature kapalı → 0 CU
- `test_cost_model_calculate_cost_with_limit`: set_loaded_accounts_data_size_limit ile özel limit
- `test_transaction_cost_with_mix_instruction_without_compute_budget`: Builtin + BPF karışık, limit yok
- `test_transaction_cost_with_mix_instruction_with_cu_limit`: Builtin + CU limit → sadece builtin sayılır

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- **Maliyet hesabı deterministik ve stateless**: Aynı tx her zaman aynı CU verir; Nova Shield'in maliyet modeli de deterministik olmalı
- **Compute budget override mantığı kritik**: Kullanıcı CU limit belirlediyse (ve user-space instruction varsa) o değer kullanılır; bu, tx'in maliyetini tahmin etmeyi ve önceden bloka kabul/ret kararı vermeyi sağlar
- **Hatalı compute_budget = 0 CU**: Geçersiz CB instruction'ları olan tx'ler bank'ta execute edilemeyeceğinden maliyeti 0 sayılıyor; Nova Shield de bu pattern'i uygulamalı
- **Write lock feature flag**: Eski sistemde demoted lock'lar sayılmıyordu; yeni sistemde sayılıyor. Nova Shield baştan doğru tasarımı benimsemeli

---

### [cost_tracker.rs] (868 satır)

**Amaç:** Blok düzeyindeki toplam maliyet birikimini takip eder; yeni transaction'ların eklenmeden önce limit kontrolü yapar.

**Enum:**
**`CostTrackerError` (satır 18-33):**
```rust
pub enum CostTrackerError {
    WouldExceedBlockMaxLimit,
    WouldExceedVoteMaxLimit,
    WouldExceedAccountMaxLimit,
    WouldExceedAccountDataBlockLimit,
    WouldExceedAccountDataTotalLimit,
}
```
`impl From<CostTrackerError> for TransactionError` — hata dönüşümü

**`CostTracker` Struct (satır 52-64):**
```rust
pub struct CostTracker {
    account_cost_limit: u64,    // account başına max CU (default: MAX_WRITABLE_ACCOUNT_UNITS = 12M)
    block_cost_limit: u64,      // blok toplam max CU (default: MAX_BLOCK_UNITS = 48M)
    vote_cost_limit: u64,       // vote tx max CU (default: MAX_VOTE_UNITS = 36M)
    cost_by_writable_accounts: HashMap<Pubkey, u64>,  // account → birikimli CU
    block_cost: u64,            // toplam blok CU
    vote_cost: u64,             // toplam vote CU
    transaction_count: u64,     // blokdaki tx sayısı
    account_data_size: u64,     // blokda oluşturulan account data toplamı (bytes)
    transaction_signature_count: u64,
    secp256k1_instruction_signature_count: u64,
    ed25519_instruction_signature_count: u64,
}
```
- `#[derive(AbiExample)]` — frozen ABI testi için
- `cost_by_writable_accounts`: `HashMap::with_capacity(WRITABLE_ACCOUNTS_PER_BLOCK)` — `WRITABLE_ACCOUNTS_PER_BLOCK = 512`

**Kritik Fonksiyonlar:**

**`try_add()` (satır 103-107) — Atomic "fit check + add":**
```rust
pub fn try_add(&mut self, tx_cost: &TransactionCost) -> Result<u64, CostTrackerError> {
    self.would_fit(tx_cost)?;       // önce kontrol
    self.add_transaction_cost(tx_cost);  // sonra ekle
    Ok(self.block_cost)
}
```

**`would_fit()` (satır 188-231) — 5 adımlı limit kontrolü:**
1. `SimpleVote` ise: `vote_cost + cost <= vote_cost_limit` kontrol
2. `block_cost + cost <= block_cost_limit` kontrol (toplam blok limiti)
3. `cost <= account_cost_limit` kontrol (tek tx'in kendisi limite uyuyor mu)
4. `account_data_size + tx.account_data_size() <= MAX_BLOCK_ACCOUNTS_DATA_SIZE_DELTA` (100MB)
5. Her writable account için: `chained_cost + cost <= account_cost_limit`

**`update_execution_cost()` (satır 109-130) — Post-execution gerçek maliyet güncellemesi:**
```rust
pub fn update_execution_cost(&mut self, estimated_tx_cost: &TransactionCost, actual_execution_units: u64) {
    match actual_execution_units.cmp(&estimated_execution_units) {
        Ordering::Equal => (),
        Ordering::Greater => self.add_transaction_execution_cost(...),
        Ordering::Less => self.sub_transaction_execution_cost(...),
    }
}
```
Bu fonksiyon kritik: maliyet tahmini ile gerçek kullanım arasındaki farkı düzeltir.

**`add_transaction_execution_cost()` (satır 270-282):**
- Her writable account için `cost_by_writable_accounts[key] += adjustment`
- `block_cost += adjustment`
- SimpleVote ise `vote_cost += adjustment`

**`remove()` / `remove_transaction_cost()` (satır 132-134, 251-267):**
- Transaction geri alındığında tüm sayaçlar `saturating_sub` ile düşürülür

**`report_stats()` (satır 144-178):**
`datapoint_info!("cost_tracker_stats", ...)` ile InfluxDB'e metrik gönderir:
- `bank_slot`, `block_cost`, `vote_cost`, `transaction_count`
- `number_of_accounts`, `costliest_account`, `costliest_account_cost`
- `account_data_size`, üç signature sayacı

**`find_costliest_account()` (satır 180-186):**
HashMap üzerinde `max_by_key` — O(n) tarama, sık çağrılmamalı

**Test Coverage (satır 308-867):**
15 test:
- Temel başlatma, tek tx ekleme (vote/non-vote)
- İki aynı account'u kullanan tx'ler (chain limit testi)
- İki farklı account'u kullanan tx'ler (block limit testi)
- Vote limit ayrımı testi
- Account data limit testi
- Atomic try_add testi (acct2 sınırı aşıyorsa tüm tx reddedilir)
- Update execution cost testi (up/down/overflow/underflow)
- Remove transaction cost testi

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- **Atomic add mantığı (would_fit + add)**: Aynı struct'ta önce kontrol sonra ekleme; thread-safety CostTracker'ı `Mutex` veya `RwLock` ile koruyarak sağlanıyor
- **Per-account chained cost**: Aynı account'u kullanan tx'ler zincir oluşturur; zincir dolu ise yeni tx bloke edilir. Bu, tek bir hot account'un blok kapasitesini tek başına tüketmesini engeller — Nova Shield'in transaction scheduler'ında bu pattern zorunlu
- **estimate → actual correction (`update_execution_cost`)**: İki aşamalı commit; önce tahmini maliyet rezerve edilir, sonra gerçek maliyet ile güncellenir. Bu, speculative execution modeline uygundur
- **vote_cost için ayrı limit ve say aç**: Vote flood koruması zorunlu

---

## 2. unified-scheduler-logic/src/

### [lib.rs] (21 satır)

**Amaç:** Unified scheduler için temel `Task` tipini tanımlar.

**Struct:**
**`Task` (satır 3-6):**
```rust
pub struct Task {
    transaction: SanitizedTransaction,
    index: usize,
}
```

**Fonksiyonlar:**
- `create_task(transaction: SanitizedTransaction, index: usize) -> Self` (satır 9-11)
- `task_index(&self) -> usize` (satır 13-15)
- `transaction(&self) -> &SanitizedTransaction` (satır 17-19)

**Önemli Notlar:**
Bu dosya şaşırtıcı derecede minimal. Gerçek scheduling state machine (TaskId, Usage, Runnable/Blocked/Idle durumları) bu versiyonda **henüz implement edilmemiş**. Scheduler logic crate'i şu an sadece `Task` taşıyıcısı olarak işlev görüyor — gerçek state machine'i unified-scheduler-pool/src/lib.rs içindeki tek-thread'li FIFO implementation üstleniyor.

**Unsafe Bloklar:** Yok.
**Test Coverage:** Yok.

**Nova Shield İçin Öğrenilen:**
- Scheduling logic ve execution pool'u ayrı crate'lere bölmek iyi bir tasarım; ancak Solana bu ayrımı henüz tam implemente etmemiş
- Nova Shield, bu ayrımı en başından doğru uygulayabilir: `TaskId`, `AccountLockState`, `ConflictGraph` gibi tipleri ayrı bir crate'e koyarak test edilebilirliği artır

---

## 3. unified-scheduler-pool/src/

### [lib.rs] (~1.220 satır)

**Amaç:** `InstalledSchedulerPool`, `InstalledScheduler`, `UninstalledScheduler` trait'lerini implemente eden tek-thread'li, geri dönüştürülebilir FIFO scheduler pool'u.

**Bağımlılıklar (satır 11-41):**
- `crossbeam_channel`: `select!`, `unbounded`, `Sender`, `Receiver`
- `derivative`: Clone derive için
- `solana_ledger::blockstore_processor::execute_batch`
- `solana_runtime::installed_scheduler_pool::*` — trait tanımları
- `solana_unified_scheduler_logic::Task`
- `std::sync::{Arc, Mutex, OnceLock, Weak}`, `AtomicU64`

**Ana Tip Aliasları:**
- `AtomicSchedulerId = AtomicU64`
- `DefaultSchedulerPool = SchedulerPool<PooledScheduler<DefaultTaskHandler>, DefaultTaskHandler>`

---

**Struct/Enum Listesi:**

**`SchedulerPool<S, TH>` (satır 49-66):**
```rust
pub struct SchedulerPool<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    scheduler_inners: Mutex<Vec<S::Inner>>,  // geri dönen scheduler'lar (FILO stack)
    handler_count: usize,                     // handler thread sayısı (şu an sabit 1)
    handler_context: HandlerContext,          // log limiti, tx status sender vb.
    weak_self: Weak<Self>,                    // Arc circular ref için self-referans
    next_scheduler_id: AtomicSchedulerId,     // monotonically artan scheduler ID
    _phantom: PhantomData<TH>,
}
```

**`HandlerContext` (satır 69-74):**
```rust
pub struct HandlerContext {
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
}
```

**`PooledScheduler<TH>` (satır 397-400):**
```rust
pub struct PooledScheduler<TH: TaskHandler> {
    inner: PooledSchedulerInner<Self, TH>,
    context: SchedulingContext,
}
```

**`PooledSchedulerInner<S, TH>` (satır 403-405):**
```rust
pub struct PooledSchedulerInner<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    thread_manager: ThreadManager<S, TH>,
}
```

**`ThreadManager<S, TH>` (satır 413-423):**
```rust
struct ThreadManager<S: SpawnableScheduler<TH>, TH: TaskHandler> {
    scheduler_id: SchedulerId,
    pool: Arc<SchedulerPool<S, TH>>,
    new_task_sender: Sender<NewTaskPayload>,
    new_task_receiver: Receiver<NewTaskPayload>,
    session_result_sender: Sender<Option<ResultWithTimings>>,
    session_result_receiver: Receiver<Option<ResultWithTimings>>,
    session_result_with_timings: Option<ResultWithTimings>,
    scheduler_thread: Option<JoinHandle<()>>,
    handler_threads: Vec<JoinHandle<()>>,
}
```

**`ExecutedTask` (satır 244-256):**
```rust
struct ExecutedTask {
    task: Task,
    result_with_timings: ResultWithTimings,
}
```

**`SubchanneledPayload<P1, P2>` (satır 264-268):**
```rust
enum SubchanneledPayload<P1, P2> {
    Payload(P1),
    OpenSubchannel(P2),
    CloseSubchannel,
}
type NewTaskPayload = SubchanneledPayload<Task, SchedulingContext>;
```

**`chained_channel` (iç modül, satır 290-387):**
`ChainedChannel<P, C>`, `ChainedChannelSender<P, C>`, `ChainedChannelReceiver<P, C>` tipleri.

Mekanizma: Birden fazla handler thread'e SchedulingContext geçişini güvenli yapmak için tek kanal üzerinden multiplexed mesajlaşma:
- `send_payload(p)` — normal task
- `send_chained_channel(context, count)` — tüm handler thread'lere yeni context bildir; yeni kanal oluşturulur ve eski kanal üzerinden `count` adet context mesajı gönderilir; handler thread'ler eski kanalı bırakıp yenisini alır

**Trait'ler:**

**`TaskHandler` (satır 201-210):**
```rust
pub trait TaskHandler: Send + Sync + Debug + Sized + 'static {
    fn handle(
        result: &mut Result<()>,
        timings: &mut ExecuteTimings,
        bank: &Arc<Bank>,
        transaction: &SanitizedTransaction,
        index: usize,
        handler_context: &HandlerContext,
    );
}
```

**`SpawnableScheduler<TH>` (satır 695-705):**
```rust
pub trait SpawnableScheduler<TH: TaskHandler>: InstalledScheduler {
    type Inner: Debug + Send + Sync;
    fn into_inner(self) -> (ResultWithTimings, Self::Inner);
    fn from_inner(inner: Self::Inner, context: SchedulingContext) -> Self;
    fn spawn(pool: Arc<SchedulerPool<Self, TH>>, initial_context: SchedulingContext) -> Self;
}
```

**Kritik Fonksiyonlar:**

**`SchedulerPool::new()` (satır 86-111):**
- `handler_count` şu an sabit 1 olarak assert edilmiş: `assert_eq!(handler_count, 1)`
- `Arc::new_cyclic(|weak_self| ...)` — self-referencing Arc (weak_self pattern)

**`SchedulerPool::do_take_scheduler()` (satır 149-157):**
```rust
fn do_take_scheduler(&self, context: SchedulingContext) -> S {
    if let Some(inner) = self.scheduler_inners.lock().unwrap().pop() {
        S::from_inner(inner, context)  // FILO: son dönen scheduler alınır
    } else {
        S::spawn(self.self_arc(), context)  // yeni spawn
    }
}
```
FILO (First-In-Last-Out): Son dönen scheduler'ın cache'i warm olacağından önce o kullanılır.

**`SchedulerPool::calculate_default_handler_count()` (satır 167-176):**
```rust
pub fn calculate_default_handler_count(detected_cpu_core_count: Option<usize>) -> usize {
    detected_cpu_core_count
        .map(|core_count| (core_count / 4).max(1))
        .unwrap_or(4)
}
```
CPU core sayısının 1/4'ü — diğer subsystem'lar için yer bırakma politikası.

**`DefaultTaskHandler::handle()` (satır 215-242) — Gerçek tx execution:**
```rust
let batch = bank.prepare_unlocked_batch_from_single_tx(transaction);
// NOT: "scheduler must properly prevent conflicting tx executions"
// yani bank locking DEĞİL, scheduler sorumlu
let batch_with_indexes = TransactionBatchWithIndexes { batch, transaction_indexes: vec![index] };
*result = execute_batch(&batch_with_indexes, bank, ...);
```
Her tx ayrı batch olarak execute edilir — `execute_batch()` çağrısı.

**`ThreadManager::start_threads()` (satır 501-662) — Çekirdek scheduler mantığı:**

5 adımlı high-level mesaj akışı (koddan, satır 508-513):
```
1. replay stage thread → new task gönderir
2. scheduler thread → task'ı kabul eder
3. scheduler thread → task'ı handler'a dispatch eder (locking sonrası)
4. handler thread → task'ı execute eder
5. handler thread → executed task olarak scheduler thread'e geri gönderir
6. scheduler thread → executed task'ı post-process eder
```

**scheduler_main_loop() — tek thread, kritik performans:**
```rust
loop {
    while !is_finished {
        select! {
            recv(finished_task_receiver) -> executed_task => {
                // handler'dan gelen sonucu işle
                active_task_count -= 1;
                accumulate_result_with_timings(...);
            },
            recv(new_task_receiver) -> message => {
                match message {
                    Payload(task) => {
                        // FIFO: doğrudan handler'a gönder (locking analizi YOK)
                        runnable_task_sender.send_payload(task);
                        active_task_count += 1;
                    }
                    OpenSubchannel(context) => { /* yeni bank session */ }
                    CloseSubchannel => { session_ending = true; }
                }
            },
        }
        is_finished = session_ending && active_task_count == 0;
    }
    if session_ending {
        session_result_sender.send(Some(result_with_timings.take().unwrap()))...;
        session_ending = false;
    }
}
```

**Kritik Yorum (kod içi — satır 525-560):**
Scheduler thread, systemin throughput üst sınırını belirleyen single-threaded darboğaz. Tasarım hedefi: syscall, VDSO, memory allocation gibi her türlü overhead'den kaçınmak. Şu anki implementasyon bunu tam sağlamıyor (execute_batch() single-tx için optimize değil), gelecekte düzeltilecek.

**`InstalledScheduler for PooledScheduler<TH>` (satır 734-759):**
- `schedule_execution(&(transaction, index))` — task oluştur ve gönder
- `wait_for_termination()` — session sonucu al, `UninstalledScheduler` döner
- `pause_for_recent_blockhash()` — session bitişini bekle (blockhash kaydı için)

**`UninstalledScheduler for PooledSchedulerInner<S, TH>` (satır 761-769):**
- `return_to_pool()` — inner'ı pool'a geri döndürür

**Test Coverage (satır 771-son):**
- `test_scheduler_pool_new`: Arc/Weak sayaçları
- `test_scheduler_spawn`: Debug format
- `test_scheduler_pool_filo`: Dört scheduler, FILO sırası doğrulanıyor
- `test_scheduler_pool_context_drop_unless_reinitialized`: Pause + terminate
- `test_scheduler_pool_context_replace`: Yeni context ile aynı scheduler ID kullanımı
- `test_scheduler_pool_install_into_bank_forks`
- `test_scheduler_install_into_bank`: Scheduler yaşam döngüsü (insert → remove → drop)
- `test_scheduler_schedule_execution_success`: Gerçek tx execution
- `test_scheduler_schedule_execution_failure`: Başarısız tx → sonraki tx'e devam
- `AsyncScheduler<TRIGGER_RACE_CONDITION>` — race condition test senaryosu

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- **Weak<Self> self-referencing pattern**: Pool Arc circular referansları Weak ile kırıyor; bu pattern Nova Shield scheduler pool tasarımında da kullanılmalı
- **FILO scheduler reuse**: Son kullanılan scheduler'ı önce al — cache warmth; Nova Shield scheduler pool'u da FILO stack olarak implemente etmeli
- **ChainedChannel mekanizması**: Bank geçişlerini kanal değiştirerek yapmak, context switch'i lock-free biçimde çözüyor. Nova Shield'in slot/epoch geçişlerinde benzer bir pattern kullanılabilir
- **Handler thread sayısı = CPU/4**: Diğer subsystem'lar için yer bırakmak zorunlu; Nova Shield'de MPC, ZK proving thread'leri de var — bu oran Nova Shield'e göre ayarlanmalı
- **Scheduler tek-thread = darboğaz**: Gerçek paralel conflict-aware scheduler için scheduler thread'in O(1) per-task kararlar vermesi şart; Nova Shield, conflict detection için optimize edilmiş veri yapısı seçmeli (örn. interval tree veya account lock bitset)
- **`prepare_unlocked_batch_from_single_tx`**: Scheduler, bank kilitleme sorumluluğunu devralmış — locking logic scheduler'a taşındı. Nova Shield'de de account locking scheduler seviyesinde yapılmalı

---

## 4. measure/src/

### [lib.rs] (4 satır)

**Amaç:** `measure` ve `macros` modüllerini dışa aktarır.

---

### [measure.rs] (149 satır)

**Amaç:** Kod bloklarının çalışma süresini nanosaniye hassasiyetiyle ölçer.

**Struct:**
**`Measure` (satır 10-14):**
```rust
pub struct Measure {
    name: &'static str,
    start: Instant,
    duration: u64,  // nanosaniye cinsinden
}
```

**Fonksiyonlar:**
- `start(name: &'static str) -> Self` (satır 17-22): `Instant::now()` ile başlatır, `duration = 0`
- `stop(&mut self)` (satır 25-27): `duration = duration_as_ns(&self.start.elapsed())`
- `as_ns() -> u64` — nanosaniye
- `as_us() -> u64` — microsaniye (`duration / 1000`)
- `as_ms() -> u64` — milisaniye (`duration / 1_000_000`)
- `as_s() -> f32` — saniye (`duration as f32 / 1e9`)
- `as_duration() -> Duration`
- `end_as_ns(self) -> u64` — stop çağırmadan direkt değer (consuming)
- `end_as_us(self) -> u64`, `end_as_ms(self) -> u64`, `end_as_s(self) -> f32`
- `end_as_duration(self) -> Duration`

**`Display` impl (satır 70-83):**
Akıllı birim seçimi:
- `duration == 0` → "name running"
- `us < 1` → "name took Xns"
- `ms < 1` → "name took Xus"
- `s < 1` → "name took Xms"
- else → "name took X.Xs"

**Test Coverage (satır 86-148):**
- `test_measure`: Gerçek 100ms sleep ile çalışma süresi doğrulama
- `test_measure_as`: Manuel duration ataması ile birim dönüşüm doğrulama
- `test_measure_display`: Her birim için Display format testi

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Tek struct, çift kullanım pattern'i: `stop()` çağrılırsa yeniden kullanılabilir; `end_as_*()` çağrılırsa consuming — iyi API tasarımı
- `duration = 0` = "henüz durdurulmamış" sentinel değeri güzel bir convention

---

### [macros.rs] (189 satır)

**Amaç:** `measure!` ve `measure_us!` makrolarını tanımlar.

**Makrolar:**

**`measure!($val:expr, $name:tt)` (satır 71-81):**
```rust
macro_rules! measure {
    ($val:expr, $name:tt $(,)?) => {{
        let mut measure = Measure::start($name);
        let result = $val;
        measure.stop();
        (result, measure)
    }};
    ($val:expr) => { measure!($val, "") };
}
```
Döner: `(result, Measure)` tuple

**`measure_us!($val:expr)` (satır 83-90):**
```rust
macro_rules! measure_us {
    ($val:expr) => {{
        let start = Instant::now();
        let result = $val;
        (result, duration_as_us(&start.elapsed()))
    }};
}
```
Döner: `(result, u64)` — hafif versiyon, Measure struct oluşturmaz

**Test Coverage (satır 92-188):**
- `test_measure_macro`: Sleep, fonksiyon çağrısı, metod, blok, trailing comma, isimsiz
- `test_measure_us_macro`: Sleep, fonksiyon, metod, blok

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- `measure!` ve `measure_us!` pattern'i performans hot path'lerde yaygın — Nova Shield de benzer makro-tabanlı ölçüm altyapısı kurmalı
- `measure_us!` struct oluşturmadan `u64` döndürür — overhead-sensitive kod için tercih edilmeli

---

## 5. metrics/src/

### [lib.rs] (85 satır)

**Amaç:** Metrics crate'inin kök modülü; `MovingStat` ve `TokenCounter` yardımcı tiplerini tanımlar.

**Struct/Tipler:**

**`MovingStat` (satır 14-16):**
```rust
pub struct MovingStat {
    value: AtomicU64,
}
```
Sürekli artan (reset edilemeyen) harici sayaçları takip etmek için.

`update_stat(old_value, new_value)`:
```rust
let old = old_value.value.swap(new_value, Ordering::Acquire);
self.value.fetch_add(new_value.saturating_sub(old), Ordering::Release);
```
Delta hesabı: `new_value - old_value` kadar artış ekler.

`load_and_reset() -> u64` — `swap(0, Acquire)` ile okur ve sıfırlar.

**`TokenCounter` (satır 32-48):**
Arc strong_count üzerinden canlı token sayısını izler.
```rust
pub struct TokenCounter(Arc<&'static str>);
pub struct CounterToken(Arc<&'static str>);
```
- `create_token()`: Yeni `CounterToken` oluşturur, `Arc::strong_count` ile anlık değer raporlar
- `Drop for CounterToken`: Token yok edildiğinde `strong_count - 2` raporlar (biri TokenCounter, biri düşen token)
- `Clone for CounterToken`: Clone sırasında raporlar

**Reexportlar (satır 6):**
`pub use crate::metrics::{flush, query, set_host_id, set_panic_hook, submit};`

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Arc strong_count'u aktif token sayacı olarak kullanmak: RAII tabanlı yaşam süresi izleme. Nova Shield'de aktif connection sayısı, pending task sayısı gibi şeyler bu pattern ile izlenebilir
- `MovingStat`: Monoton artan dış sayaçlar (örn. toplam işlenen tx) için delta takibi — validator telemetrisinde gerekli

---

### [metrics.rs] (701 satır)

**Amaç:** InfluxDB'e datapoint gönderen asenkron metrics agent.

**Enum/Struct Listesi:**

**`MetricsError` (satır 26-37):**
```rust
pub enum MetricsError {
    VarError(#[from] env::VarError),
    ReqwestError(#[from] reqwest::Error),
    ConfigInvalid(String),
    ConfigIncomplete,
    DbMismatch(String),
}
```

**`MetricsCommand` (satır 55-59):**
```rust
enum MetricsCommand {
    Flush(Arc<Barrier>),
    Submit(DataPoint, log::Level),
    SubmitCounter(CounterPoint, log::Level, u64),
}
```

**`MetricsAgent` (satır 61-63):**
```rust
pub struct MetricsAgent {
    sender: Sender<MetricsCommand>,
}
```
Sadece bir `Sender`! Asıl işlem ayrı thread'de yapılıyor.

**`MetricsWriter` trait (satır 65-69):**
```rust
pub trait MetricsWriter {
    fn write(&self, points: Vec<DataPoint>);
}
```

**`InfluxDbMetricsWriter` (satır 71-73):** URL ile HTTP POST

**`MetricsConfig` (satır 388-394):** host, db, username, password alanları

**Kritik Fonksiyonlar:**

**`MetricsAgent::new()` (satır 191-204):**
```rust
pub fn new(writer, write_frequency, max_points_per_sec) -> Self {
    let (sender, receiver) = unbounded::<MetricsCommand>();
    thread::Builder::new()
        .name("solMetricsAgent")
        .spawn(move || Self::run(&receiver, &writer, write_frequency, max_points_per_sec))
        .unwrap();
    Self { sender }
}
```
Ayrı thread spawn eder, `sender` saklar.

**`MetricsAgent::run()` (satır 253-319) — metrics thread'i:**
```rust
loop {
    match receiver.recv_timeout(write_frequency / 2) {
        Ok(MetricsCommand::Flush(barrier)) => {
            write(points, counters);
            barrier.wait();  // çağıranı beklet
        }
        Ok(MetricsCommand::Submit(point, level)) => {
            log!(level, "{}",  point);
            points.push(point);
        }
        Ok(MetricsCommand::SubmitCounter(counter, _, bucket)) => {
            // aynı (name, bucket) anahtarla gelen counter'lar toplanır
            counters.entry((counter.name, bucket)).and_modify(|v| v.count += counter.count).or_insert(counter);
        }
        Err(Timeout) => {}
        Err(Disconnected) => break,
    }
    if elapsed >= write_frequency {
        write(points, counters);
    }
}
```
Varsayılan write_frequency: 10 saniye; recv timeout: 5 saniye (frequency/2)

**`MetricsAgent::write()` (satır 213-251):**
Rate limiting: `max_points_per_sec × write_frequency_secs` kadar veri kabul edilir; fazlası truncate edilir; son nokta olarak kayıp istatistiği eklenir.

**`get_singleton_agent()` (satır 350-356):**
`lazy_static! { static ref AGENT: MetricsAgent = MetricsAgent::default(); }` — global singleton

**`MetricsAgent::Default` (satır 173-188):**
```
max_points_per_sec = env::var("SOLANA_METRICS_MAX_POINTS_PER_SECOND").unwrap_or(4000)
write_frequency = Duration::from_secs(10)
```

**`serialize_points()` (satır 102-135):**
InfluxDB line protocol formatına çevirir:
```
<measurement>,host_id=<host_id>[,tag=value] [field=value[,field=value]] <timestamp_nanos>
```
Kapasite önceden hesaplanarak tek String allocation yapılır.

**`set_panic_hook()` (satır 471-501):**
Panic handler'ı hook eder: panic anında "panic" datapoint'i submit eder ve process'i exit(1) ile temizler. `Once` kullanarak tek kez çalışır.

**`metrics_config_sanity_check()` (satır 434-449):**
Cluster tipi ile db adı uyumunu kontrol eder (mainnet-beta → "mainnet-beta" db olmalı vb.)

**`test_mocks::MockMetricsWriter` (satır 503-542):**
Test için sahte writer; `Arc<Mutex<Vec<DataPoint>>>` içinde toplar.

**Test Coverage (satır 544-700):**
- `test_submit`: 42 datapoint + 1 metadata = 43 yazım
- `test_submit_counter`: 10 counter × 2 isim + 1 metadata = 21
- `test_submit_counter_increment`: Aynı bucket'a 10 kez 10 ekle → count=100
- `test_submit_bucketed_counter`: 50 mesaj / bucket 10 = 5 bucket × 2 counter + 1 metadata = 11
- `test_submit_with_delay`: 2 saniyelik bekleme → otomatik flush
- `test_submit_exceed_max_rate`: max 100/saniye, 102 gönder → 100 yazılır
- `test_multithread_submit`: 42 thread paralel submit
- `test_flush_before_drop`: Drop anında flush doğrulama

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- **Channel-based async metrics**: Tüm metrics gönderimi non-blocking; gönderme tek bir channel `send` — Hot path'lerde asla block etmez
- **Counter deduplication (bucket bazlı)**: Yüksek frekanslı sayaçlar, zaman dilimi (bucket) bazında toplanarak InfluxDB yüküne aşırı point gönderilmesi önleniyor. Nova Shield metrics sistemi de bu pattern'i kullanmalı
- **`set_panic_hook` + `flush()`**: Panic anında metrikleri kaydetme ve temiz çıkış; Nova Shield validatörü için kritik — watchdog/alert sistemleri için
- **InfluxDB line protocol**: Yazılı format öğrenildi; Nova Shield aynı format ile InfluxDB/VictoriaMetrics/Prometheus push gateway'e yazabilir
- **Rate limiting ile truncation**: Aşırı veri yerine truncation + metadata point gönderme; veri kaybını kabul eden ama sistemi koruyucu yaklaşım

---

### [counter.rs] (357 satır)

**Amaç:** Atom bazlı counter ve log/metrics gönderim makroları.

**Struct/Tipler:**

**`Counter` (satır 16-25):**
```rust
pub struct Counter {
    pub name: &'static str,
    pub counts: AtomicUsize,     // toplam birikimli değer
    pub times: AtomicUsize,      // inc() çağrı sayısı
    pub lastlog: AtomicUsize,    // son log anındaki counts değeri
    pub lograte: AtomicUsize,    // kaç times'da bir log basılsın
    pub metricsrate: AtomicU64,  // kaç milisaniyede bir metrics gönderilsin
}
```

**`CounterPoint` (satır 28-32):**
```rust
pub struct CounterPoint {
    pub name: &'static str,
    pub count: i64,
    pub timestamp: SystemTime,
}
```

**Makrolar:**
- `create_counter!(name, lograte, metricsrate)` — static Counter başlatma
- `inc_counter!(name, level, count)` — `unsafe { name.inc(level, count) }`
- `inc_counter_info!(name, count)` — Info seviyesi kısayol
- `inc_new_counter!(name, count, level, lograte, metricsrate)` — static mutable Counter ile; `Once` ile init
- `inc_new_counter_error!`, `inc_new_counter_warn!`, `inc_new_counter_info!`, `inc_new_counter_debug!` — seviye varyantları

**`Counter::inc()` (satır 173-205) — kritik fonksiyon:**
```rust
pub fn inc(&mut self, level: log::Level, events: usize) {
    let now = timing::timestamp();  // ms
    let counts = self.counts.fetch_add(events, Relaxed);
    let times = self.times.fetch_add(1, Relaxed);
    let lograte = self.lograte.load(Relaxed);
    let metricsrate = self.metricsrate.load(Relaxed);

    // her lograte times'da bir log bas
    if times % lograte == 0 && times > 0 && log_enabled!(level) {
        log!(level, "COUNTER:{...}");
    }

    // CAS ile lastlog güncelle ve delta gönder
    let lastlog = self.lastlog.load(Relaxed);
    let prev = self.lastlog.compare_and_swap(lastlog, counts, Relaxed);
    if prev == lastlog {
        let bucket = now / metricsrate;
        let counter = CounterPoint { name, count: counts - lastlog, timestamp: now };
        submit_counter(counter, level, bucket);
    }
}
```

**Unsafe Bloklar:**
- `inc_counter!` makrosu: `unsafe { name.inc(level, count) }` — static mutable Counter erişimi
- `inc_new_counter!` makrosu: `static mut INC_NEW_COUNTER` + `unsafe { INC_NEW_COUNTER.init() }`
- `get_env_lock()` test helper: `static mut ENV_LOCK: Option<RwLock<()>>` + unsafe init
- Neden: `Counter` static mutable global değişken olarak kullanılıyor; Rust'ın data race koruması bypass edilmeli; ancak `AtomicUsize` ve `AtomicU64` field'ları zaten thread-safe

**Test Coverage (satır 207-357):**
- `test_counter`: AtomicUsize değerleri doğrulama
- `test_metricsrate`: Default metricsrate (1000ms)
- `test_metricsrate_env`: `SOLANA_DEFAULT_METRICS_RATE` env var
- `test_inc_new_counter`: Sözdizimi doğrulama
- `test_lograte`: Default lograte (1000)
- `test_lograte_env`: `SOLANA_DEFAULT_LOG_RATE` env var

**Nova Shield İçin Öğrenilen:**
- `static mut Counter` + `Once` init pattern — legacy yaklaşım; `std::sync::LazyLock` veya `once_cell::sync::Lazy` ile daha güvenli yapılabilir
- `compare_and_swap` (deprecated) kullanımı: `compare_exchange` tercih edilmeli
- Bucket bazlı metrics: `bucket = now_ms / metricsrate` — aynı zaman dilimindeki counter artışları tek datapoint'te toplanır

---

### [datapoint.rs] (355 satır)

**Amaç:** InfluxDB datapoint yapısı ve `datapoint_*!` makroları.

**Struct:**
**`DataPoint` (satır 45-51):**
```rust
pub struct DataPoint {
    pub name: &'static str,
    pub timestamp: SystemTime,
    pub tags: Vec<(&'static str, String)>,     // group-by için
    pub fields: Vec<(&'static str, String)>,   // asıl veri
}
```

**Metodlar:**
- `add_tag(name, value: &str) -> &mut Self`
- `add_field_str(name, value)` — `"\"escaped_value\""` formatı
- `add_field_bool(name, value)` — `"true"/"false"`
- `add_field_i64(name, value)` — `"123i"` (InfluxDB integer suffix)
- `add_field_f64(name, value)` — `"1.23"`

**`Display` impl:** `"datapoint: name,tag=val field=val field2=val"`

**Makrolar:**
- `create_datapoint!(@point name, fields...)` — DataPoint oluşturur
- `datapoint!(level, name, fields...)` — log_enabled! kontrolü ile submit
- `datapoint_error!`, `datapoint_warn!`, `datapoint_info!`, `datapoint_debug!`, `datapoint_trace!`
- Optional field desteği: `("field", Some(value), Option<i64>)` — None ise alan eklenmez

**Test Coverage (satır 207-354):**
- `test_datapoint`: Tüm tipler, çoklu field, builder pattern
- `test_optional_datapoint`: Some/None kombinasyonları
- `test_datapoint_with_tags`: Tag + field kombinasyonları

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- InfluxDB line protocol: i64 değerlere `"i"` suffix eklemek zorunlu (integer vs. float ayrımı)
- Optional field pattern: None alanlar hiç gönderilmiyor — sparse datapoint desteği

---

### [poh_timing_point.rs] (174 satır)

**Amaç:** PoH slot zamanlama noktalarını channel üzerinden iletmek için tipler.

**Enum:**
**`PohTimingPoint` (satır 18-22):**
```rust
pub enum PohTimingPoint {
    PohSlotStart(u64),       // slot başlama timestamp (ms)
    PohSlotEnd(u64),         // slot bitiş timestamp (ms)
    FullSlotReceived(u64),   // tam slot alındı timestamp (ms)
}
```

**Struct:**
**`SlotPohTimingInfo` (satır 37-44):**
```rust
pub struct SlotPohTimingInfo {
    pub slot: Slot,
    pub root_slot: Option<Slot>,
    pub timing_point: PohTimingPoint,
}
```

**Tip Aliasları:**
- `PohTimingReceiver = Receiver<SlotPohTimingInfo>`
- `PohTimingSender = Sender<SlotPohTimingInfo>`

**Fonksiyonlar:**
- `new_slot_start_poh_time_point(slot, root_slot, timestamp) -> SlotPohTimingInfo`
- `new_slot_end_poh_time_point(slot, root_slot, timestamp) -> SlotPohTimingInfo`
- `new_slot_full_poh_time_point(slot, root_slot, timestamp) -> SlotPohTimingInfo`
- `send_poh_timing_point(sender, slot_timing)` — `try_send`; başarısız olursa sadece `info!` log (non-blocking)

**`Display` impl:** `"PohTimingPoint: poh_start=X, slot=Y, root_slot=Z"`

**Test Coverage (satır 108-173):**
Her üç timing point için root_slot Some/None kombinasyonları test edilmiş.

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- PoH timing measurement 3 noktada yapılıyor: start, end, full_received — latency metrikleri için bu 3 değerin farkı hesaplanabilir
- `try_send` + ignore: timing channel dolu olsa bile bloklanmaz — Nova Shield'in telemetri pipeline'ı için doğru yaklaşım; latency daha önemli

---

## Özet: Tüm Modüllerin Bütünleşik Mimarisi

### Transaction Yaşam Döngüsü (Maliyet Perspektifinden)

```
SanitizedTransaction
      │
      ▼
CostModel::calculate_cost()
      │ signature_cost + write_lock_cost + data_bytes_cost
      │ + programs_execution_cost + loaded_accounts_data_size_cost
      ▼
TransactionCost (SimpleVote|Transaction)
      │
      ▼
CostTracker::try_add()
      │ would_fit() → 5 limit kontrolü
      │ add_transaction_cost()
      ▼
Banking Stage → Scheduler
      │
      ▼
SchedulerPool::do_take_scheduler()  [FILO]
      │
      ▼
ThreadManager::send_task()
      │
      ▼ (scheduler thread)
runnable_task_sender.send_payload(task)
      │
      ▼ (handler thread)
DefaultTaskHandler::handle()
→ execute_batch()
      │
      ▼
CostTracker::update_execution_cost()  [gerçek CU ile düzelt]
```

### Sayısal Özet

| Metrik | Değer |
|---|---|
| MAX_BLOCK_UNITS | 48_000_000 CU |
| MAX_WRITABLE_ACCOUNT_UNITS | 12_000_000 CU/account |
| MAX_VOTE_UNITS | 36_000_000 CU (%75) |
| MAX_BLOCK_ACCOUNTS_DATA_SIZE_DELTA | 100 MB |
| SIMPLE_VOTE_USAGE_COST | 3_428 CU |
| SIGNATURE_COST | 720 CU |
| SECP256K1_VERIFY_COST | 6_690 CU |
| ED25519_VERIFY_COST | 2_280 CU |
| WRITE_LOCK_UNITS | 300 CU |
| Handler thread count | CPU_cores / 4 (min 1) |
| Metrics write frequency | 10 saniye |
| Max metrics/saniye | 4_000 |

---

## Nova Shield İçin Konsolide Eylem Planı

### 1. Maliyet Modeli

- **Cluster telemetrisi önce, sabitler sonra**: Solana `COMPUTE_UNIT_TO_US_RATIO = 30` değerini gerçek cluster ölçümlerinden türetti. Nova Shield de kendi test networkü üzerinde bu ölçümleri yapmalı; aksi halde keyfi sabitler yanlış kapasiteye yol açar
- **Vote tx fast-path**: Vote tx'leri için statik maliyet hesabı zorunlu; validator consensus performansını doğrudan etkiler
- **`account_data_size` ayrı quota**: CU limitinden bağımsız; Nova Shield de account storage artışını ayrı bir limit mekanizmasıyla kontrol etmeli
- **Feature flag ile geçiş**: Maliyet modeli değişikliklerini feature flag ile aktive et; retroaktif değişiklik testnet/mainnet geçişlerinde sorun çıkarır

### 2. Scheduler

- **Conflict-free FIFO önce, paralel conflict-aware sonra**: Solana'nın mevcut scheduler FIFO; Nova Shield başlangıçta aynısını uygulayabilir
- **Account lock scheduler'da**: Bank'ta değil, scheduler'da — daha ince grained kontrol
- **ChainedChannel pattern**: Context geçişleri (slot/epoch) için lock-free channel switch; MPC threshhold session geçişlerinde de uygulanabilir
- **Scheduler thread minimal overhead**: Her tx için kernel syscall/allocation yok; lock-free data structure kullan

### 3. Metrics

- **Async channel-based**: Metrics gönderimi hiçbir zaman hot path'i bloklamamalı
- **InfluxDB/VictoriaMetrics + Grafana**: Solana'nın altyapısı; Nova Shield aynı stack'i kullanabilir
- **`set_panic_hook` zorunlu**: Validatör panic'lerini metrics sistemine kaydet — production kritik
- **Bucket deduplication**: Yüksek frekanslı counter'ları bucket'layarak InfluxDB'e aşırı yük verme

### 4. Measure

- `measure!` makrosu → `(result, Measure)` pattern: Rust'ta ergonomik performance instrumentasyon; Nova Shield'in tüm kritik fonksiyonlarına entegre edilmeli
- `measure_us!` → overhead-sensitive path'ler için; struct allocate etmeksizin µs değeri döner
