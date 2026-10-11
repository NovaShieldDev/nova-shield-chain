# Solana Banking Stage Analizi — Agent 11

**Analiz Tarihi:** 2026-07-22
**Kapsam:** core/src/banking_stage/ (tüm dosyalar)
**Toplam Dosya:** 31 (banking_stage.rs + 19 modül dosyası + 11 transaction_scheduler alt dosyası)

---

## Genel Mimari Özeti

Banking Stage, Solana'nın blok üretim motorunun kalbidir. Sigverify stage'den gelen paketleri alır, deserialize eder, öncelik sırasına görer, çakışma analizi yapar, PoH'a kaydeder ve commit eder. İki farklı mimari paralel olarak mevcuttur:

1. **Legacy BankingStage** (`banking_stage.rs`) — thread-per-type modeli, doğrudan `UnprocessedTransactionStorage` üzerinden çalışır.
2. **Yeni Scheduler Mimarisi** (`transaction_scheduler/`) — `PrioGraphScheduler` + `SchedulerController` + `ConsumeWorker` pipeline'ı, `prio_graph` kütüphanesiyle çakışma grafiği kullanır.

---

## 1. banking_stage.rs (Ana Modül, 1457 satır)

**Konum:** `core/src/banking_stage.rs`

### Sabitler ve Type Alias'lar

```rust
// Satır ~40-50 arası
const TOTAL_BUFFERED_PACKETS: usize = 700_000;  // ⚠️ Düzeltme: 500_000 DEĞİL, 700_000
const NUM_VOTE_PROCESSING_THREADS: u32 = 2;
// TOTAL_BUFFERED_PACKET_THRESHOLD = TOTAL_BUFFERED_PACKETS / (num_threads - NUM_VOTE_PROCESSING_THREADS)
pub const MAX_RECEIVE_BATCH_SIZE: usize = 10_000;
```

- `TOTAL_BUFFERED_PACKETS = 500_000`: Tüm bankacılık thread'lerinde tamponlanacak maksimum paket sayısı.
- `NUM_VOTE_PROCESSING_THREADS = 2`: Oy işleme için ayrılan thread sayısı.

### Enum: ForwardOption

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardOption {
    NotForward,           // Yönlendirme yapılmaz
    ForwardTransaction,   // Normal tx → leader'ın tpu_forwards adresine
    ForwardTpuVote,       // Oy tx → leader'ın tpu_vote adresine (sadece UDP)
}
```

**Tasarım kararı:** Oy tx'leri ve normal tx'ler farklı soket/protokol üzerinden gönderilir. Oylar her zaman UDP ile gönderilir, normal tx'ler QUIC kullanabilir.

### Struct: BankingStageStats

Büyük atomik sayaç koleksiyonu. Her istatistik bir `AtomicUsize` veya `AtomicU64`:
- `receive_and_buffer_packets_count`
- `dropped_packets_count`
- `newly_buffered_packets_count`
- `current_buffered_packets_count`
- `rebuffered_packets_count`
- `consumed_buffered_packets_count`
- `consume_buffered_packets_elapsed`
- `filter_pending_packets_elapsed`
- `packet_conversion_elapsed`
- `transaction_processing_elapsed`
- `forwarded_transaction_count`
- `forwarded_vote_count`
- `dropped_forward_packets_count`
- `batch_packet_indexes_len` — `histogram::Histogram` ile dağılım takibi

`report()` metodu 1000ms'de bir tüm sayaçları `datapoint_info!` makrosuyla raporlar ve sıfırlar.

### Struct: BatchedTransactionDetails

```rust
pub struct BatchedTransactionDetails {
    pub costs: BatchedTransactionCostDetails,
    pub errors: BatchedTransactionErrorDetails,
}

pub struct BatchedTransactionCostDetails {
    pub batched_signature_cost: u64,
    pub batched_write_lock_cost: u64,
    pub batched_data_bytes_cost: u64,
    pub batched_programs_execute_cost: u64,
}

pub struct BatchedTransactionErrorDetails {
    pub batched_retried_txs_per_block_limit_count: u64,
    pub batched_retried_txs_per_vote_limit_count: u64,
    pub batched_retried_txs_per_account_limit_count: u64,
    pub batched_retried_txs_per_account_data_block_limit_count: u64,
    pub batched_dropped_txs_per_account_data_total_limit_count: u64,
}
```

QosService'in biriktirdiği maliyet ve hata detayları. Blok kapasitesi yönetimi için.

### Struct: BankingStage

```rust
pub struct BankingStage {
    bank_thread_hdls: Vec<JoinHandle<()>>,
}
```

Ana thread konteynerı. `new()` metodu tüm alt thread'leri başlatır.

### fn BankingStage::new() — Ana Başlatma (satır ~200-400 arası)

`BlockProductionMethod` enum'una göre iki farklı pipeline başlatır:

**CentralScheduler:**
1. `PacketDeserializer` oluşturulur
2. `N` adet `ConsumeWorker` thread'i başlatılır (her birine `consume_receiver` ve `consumed_sender` kanalı)
3. `PrioGraphScheduler` oluşturulur (tüm consume_sender'ları alır)
4. `SchedulerController` oluşturulur ve tek bir thread'de çalıştırılır
5. `ForwardWorker` thread'leri başlatılır

**ThreadLocalScheduler (Legacy):**
1. `NUM_VOTE_PROCESSING_THREADS` adet oy thread'i
2. Ek non-vote transaction thread'leri
3. Her thread kendi `UnprocessedTransactionStorage` bufferına sahip
4. Her thread `decision_maker → consume/forward/hold` döngüsü çalıştırır

### fn BankingStage::new_num_threads()

```rust
pub fn num_threads() -> u32 {
    cmp::max(
        env::var("SOLANA_BANKING_THREADS")
            .map(|x| x.parse().unwrap_or(NUM_THREADS))
            .unwrap_or(NUM_THREADS),
        MIN_TOTAL_THREADS,
    )
}
```

Thread sayısı env var ile override edilebilir.

### fn BankingStage::banking_stage_thread (Legacy Ana Döngü)

Her legacy thread şu döngüyü çalıştırır:
```
loop {
    receive_and_buffer_packets()   // SigVerify'dan al, buffer'a ekle
    decision = make_consume_or_forward_decision()
    match decision {
        Consume → consume_buffered_packets()
        Forward → handle_forwarding(hold=false)
        ForwardAndHold → handle_forwarding(hold=true)
        Hold → (bekle)
    }
    slot_metrics_tracker.report_if_slot_changed()
}
```

### Test Fonksiyonları (satır ~1100-1457)

- `create_slow_genesis_config(lamports)` — Test için yavaş (wallclock'suz) genesis config
- `create_slow_genesis_config_with_leader(lamports, leader)` — Leader'lı versiyon
- `sanitize_transactions(txs)` — `Vec<Transaction>` → `Vec<SanitizedTransaction>`
- `simulate_poh(record_receiver, poh_recorder)` — PoH'u simüle eden arka plan thread'i döndürür
- `new_test_cluster_info(validator_keypair)` — Test için ClusterInfo ve Node

---

## 2. committer.rs (179 satır)

**Konum:** `core/src/banking_stage/committer.rs`

### Enum: CommitTransactionDetails (satır 27-30)

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitTransactionDetails {
    Committed { compute_units: u64 },
    NotCommitted,
}
```

Her tx için commit sonucu. `Committed` variantı fiilen kaç CU harcandığını taşır.

### Struct: PreBalanceInfo (satır 33-37)

```rust
#[derive(Default)]
pub(super) struct PreBalanceInfo {
    pub native: Vec<Vec<u64>>,                      // her tx için her account bakiyesi
    pub token: Vec<Vec<TransactionTokenBalance>>,    // token bakiyeleri
    pub mint_decimals: HashMap<Pubkey, u8>,          // decimals cache
}
```

Transaction commit öncesinde toplanan bakiye snapshot'ı. RPC üzerinden status yayını için gerekli.

### Struct: Committer (satır 40-44)

```rust
#[derive(Clone)]
pub struct Committer {
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: ReplayVoteSender,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
}
```

**Fields:**
- `transaction_status_sender`: RPC node'lara tx sonuçları yollamak için; `None` ise status yayını devre dışı.
- `replay_vote_sender`: Commit edilen oy tx'lerini replay stage'e bildirir.
- `prioritization_fee_cache`: Öncelik ücreti istatistiklerini tutar, wallet'ların fee tahmini için kullanılır.

### fn Committer::commit_transactions (satır 64-133)

**İmza:**
```rust
pub(super) fn commit_transactions(
    &self,
    batch: &TransactionBatch,
    loaded_transactions: &mut [TransactionLoadResult],
    execution_results: Vec<TransactionExecutionResult>,
    last_blockhash: Hash,
    lamports_per_signature: u64,
    starting_transaction_index: Option<usize>,
    bank: &Arc<Bank>,
    pre_balance_info: &mut PreBalanceInfo,
    execute_and_commit_timings: &mut LeaderExecuteAndCommitTimings,
    signature_count: u64,
    executed_transactions_count: usize,
    executed_non_vote_transactions_count: usize,
    executed_with_successful_result_count: usize,
) -> (u64, Vec<CommitTransactionDetails>)
```

**Algoritma:**
1. `execution_results` içinden yalnızca execute edilen tx'leri filtrele
2. `bank.commit_transactions()` çağrısını süre ölçerek yap (`measure_us!`)
3. Her execution result'ı `CommitTransactionDetails::Committed { compute_units }` veya `NotCommitted`'e dönüştür
4. `bank_utils::find_and_send_votes()` ile oy tx'lerini replay stage'e bildir
5. `collect_balances_and_send_status_batch()` ile RPC'ye bildir
6. `prioritization_fee_cache.update()` ile fee cache'i güncelle
7. `(commit_time_us, commit_transaction_statuses)` döndür

**Kritik tasarım:** `last_blockhash` commit'ten **önce** alınmalıdır, çünkü PoH `MaxHeightReached` durumuna geçtikten sonra hash güncellenmektedir. Race condition önlemi.

### fn collect_balances_and_send_status_batch (satır 135-178)

Eğer `transaction_status_sender` varsa:
- Post-commit bakiyeleri topla
- Post-commit token bakiyeleri topla
- Her tx için block içindeki `transaction_index` hesapla (sadece execute edilenler için artım)
- `TransactionStatusSender::send_transaction_status_batch()` ile toplu yayın yap

---

## 3. consumer.rs (~826 satır)

**Konum:** `core/src/banking_stage/consumer.rs`

### Sabit (satır 46)

```rust
pub const TARGET_NUM_TRANSACTIONS_PER_BATCH: usize = 64;
```

Her çalıştırma adımında işlenecek hedef tx sayısı.

### Struct: ProcessTransactionBatchOutput (satır 48-54)

```rust
pub struct ProcessTransactionBatchOutput {
    pub(crate) cost_model_throttled_transactions_count: usize,
    pub(crate) cost_model_us: u64,
    pub execute_and_commit_transactions_output: ExecuteAndCommitTransactionsOutput,
}
```

### Struct: ExecuteAndCommitTransactionsOutput (satır 56-75)

```rust
pub struct ExecuteAndCommitTransactionsOutput {
    pub(crate) transactions_attempted_execution_count: usize,
    pub(crate) executed_transactions_count: usize,
    pub(crate) executed_with_successful_result_count: usize,
    pub(crate) retryable_transaction_indexes: Vec<usize>,
    pub commit_transactions_result: Result<Vec<CommitTransactionDetails>, PohRecorderError>,
    pub(crate) execute_and_commit_timings: LeaderExecuteAndCommitTimings,
    pub(crate) error_counters: TransactionErrorMetrics,
    pub(crate) min_prioritization_fees: u64,
    pub(crate) max_prioritization_fees: u64,
}
```

**retryable_transaction_indexes:** `AccountInUse`, `WouldExceedMaxBlockCostLimit` gibi geçici hatalar nedeniyle tekrar denenebilecek tx indeksleri.

### Struct: Consumer (satır 77-82)

```rust
pub struct Consumer {
    committer: Committer,
    transaction_recorder: TransactionRecorder,
    qos_service: QosService,
    log_messages_bytes_limit: Option<usize>,
}
```

**log_messages_bytes_limit:** Log mesajlarını kesme sınırı. Validator memory'yi korumak için.

### fn Consumer::consume_buffered_packets (satır 99-152)

**İmza:**
```rust
pub fn consume_buffered_packets(
    &self,
    bank_start: &BankStart,
    unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
    banking_stage_stats: &BankingStageStats,
    slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
)
```

**Algoritma:**
1. `unprocessed_transaction_storage.process_packets()` ile callback-based iteration
2. Her batch için `do_process_packets()` çağrısı
3. `reached_end_of_slot` bayrağını takip et
4. İstatistikleri güncelle

### fn Consumer::do_process_packets (satır 154-213)

`payload.reached_end_of_slot` kontrolüyle erken çıkış. `process_packets_transactions()` çağırır. Retryable tx sayısını günceller. `None` döndürmesi halinde dış döngüyü durdurur.

### fn Consumer::process_transactions (satır 271-404)

Ana işlem döngüsü:
```
chunk_start = 0
while chunk_start != transactions.len():
    chunk_end = min(len, chunk_start + 64)
    output = process_and_record_transactions(bank, txs[chunk_start..chunk_end], chunk_start)
    if MaxHeightReached or !should_bank_still_be_processing:
        push remaining as retryable
        break
    chunk_start = chunk_end
```

`overall_min/max_prioritization_fees` takibi. `saturating_add_assign!` ile overflow koruması.

### fn Consumer::process_and_record_transactions (satır 406-432)

```rust
pub fn process_and_record_transactions(
    &self,
    bank: &Arc<Bank>,
    txs: &[SanitizedTransaction],
    chunk_offset: usize,
) -> ProcessTransactionBatchOutput
```

1. `bank.check_transactions()` ile blockhash/nonce kontrolü
2. `process_and_record_transactions_with_pre_results()` ile ana akış

### fn Consumer::process_and_record_aged_transactions (satır 434-465)

Scheduler tarafından kullanılır. Tx'lerin epoch sınırı geçip geçmediğini kontrol eder:
- `max_slot_age < bank.slot()` ise: `bank.fully_verify_transaction()` ile tekrar sanitize et. Sonuç farklıysa `ResanitizationNeeded` hatası.
- Değilse: sadece address lookup table'ları kontrol et (kapatılmış olabilir)

### fn Consumer::process_and_record_transactions_with_pre_results (satır 467-558)

**Ana akış (en önemli fonksiyon):**
1. `qos_service.select_and_accumulate_transaction_costs()` — hangi tx'lerin bloğa girebileceğini belirle
2. `bank.prepare_sanitized_batch_with_results()` — account lock'larını al
3. `execute_and_commit_transactions_locked()` çağrısı
4. `drop(batch)` — account lock'larını serbest bırak (unlock_us ölçülür)
5. `QosService::remove_costs()` — commit edilmeyenlerin maliyetlerini sil
6. Feature flag kontrolüyle `QosService::update_costs()` — gerçek CU ile güncelle
7. `chunk_offset` ile retryable index'leri düzelt
8. QoS metrikleri raporla

### fn Consumer::execute_and_commit_transactions_locked (satır 560-733)

Tam pipeline:
1. Pre-balance toplama (RPC aktifse)
2. Min/max compute_unit_price bulma (`Itertools::minmax()`)
3. `bank.load_and_execute_transactions()` — yükle ve çalıştır
4. Execute edilen tx'leri `VersionedTransaction`'a çevir
5. `bank.freeze_lock()` — freeze lock'ı al (kritik: commit ve PoH kaydı arasında tutulur)
6. `bank.last_blockhash_and_lamports_per_signature()` — blockhash'i al (record'dan önce!)
7. `transaction_recorder.record_transactions()` — PoH'a kaydet
8. Kayıt hatası varsa: execute edilenleri retryable olarak işaretle, erken dön
9. Başarılı kayıt: `committer.commit_transactions()` çağır
10. `drop(freeze_lock)` — lock'ı serbest bırak

**Kritik sıralama:** `freeze_lock` alındıktan sonra `last_blockhash` alınmalı, sonra `record_transactions`, sonra `commit_transactions`. Bu sıra PoH bütünlüğü için zorunludur.

### fn Consumer::check_fee_payer_unlocked (satır 735-768, pub)

Fee payer account'ının:
- Var olup olmadığı
- Ücret ödemeye yetecek bakiyesi olup olmadığı
- Rent-exempt eşiği karşılayıp karşılamadığı

`process_compute_budget_instructions()` ile fee hesabı, `validate_fee_payer()` ile doğrulama.

### fn Consumer::filter_pending_packets_from_pending_txs (satır 788-803)

Retryable tx'lerin gerçekten hala geçerli olup olmadığını kontrol eder. `check_transactions_with_forwarding_delay()` kullanarak `FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET` kadar ek zaman verir. Geçersiz olanları filtreler.

### fn Consumer::prepare_filter_for_pending_transactions (satır 806-814)

Bekleyen tx'ler için `Ok(())`, beklemeyenler için `Err(BlockhashNotFound)` maskesi oluşturur.

### fn Consumer::accumulate_execute_units_and_time (satır 770-782)

`ExecuteTimings::details::per_program_timings` üzerinden tüm programların CU ve zaman toplamını döndürür.

### Test Fonksiyonları (satır 827+)

- `execute_transactions_with_dummy_poh_service()` — bank + poh kaydı ile tam test
- Çok sayıda entegrasyon testi: nonce, address lookup tables, duplicate signatures, compute budget, ALT, age filtering, vb.

---

## 4. consume_worker.rs (966 satır)

**Konum:** `core/src/banking_stage/consume_worker.rs`

### Enum: ConsumeWorkerError (satır 23-28)

```rust
#[derive(Debug, Error)]
pub enum ConsumeWorkerError {
    #[error("Failed to receive work from scheduler: {0}")]
    Recv(#[from] RecvError),
    #[error("Failed to send finalized consume work to scheduler: {0}")]
    Send(#[from] SendError<FinishedConsumeWork>),
}
```

### Struct: ConsumeWorker (satır 30-37)

```rust
pub(crate) struct ConsumeWorker {
    consume_receiver: Receiver<ConsumeWork>,
    consumer: Consumer,
    consumed_sender: Sender<FinishedConsumeWork>,
    leader_bank_notifier: Arc<LeaderBankNotifier>,
    metrics: Arc<ConsumeWorkerMetrics>,
}
```

**leader_bank_notifier:** Scheduler thread'i banker thread'ini bank değişikliklerinden haberdar eder. Worker bu sayede hangi bank üzerinde çalışacağını bilir.

### fn ConsumeWorker::run (satır 61-66)

```rust
pub fn run(self) -> Result<(), ConsumeWorkerError> {
    loop {
        let work = self.consume_receiver.recv()?;
        self.consume_loop(work)?;
    }
}
```

Blocking `recv()` ile iş bekler. Channel kapanınca `RecvError` ile döner.

### fn ConsumeWorker::consume_loop (satır 68-85)

1. `get_consume_bank()` ile bank al (50ms timeout)
2. Bank yoksa: tüm batch'leri retry olarak işaretle (`retry_drain`)
3. Bank varsa: `bank.is_complete()` kontrol edilir; slot tamamlanmışsa yeni bank al
4. `try_drain_iter` ile channel'daki tüm mevcut işleri non-blocking şekilde tüket

### fn ConsumeWorker::consume (satır 88-105)

`consumer.process_and_record_aged_transactions()` çağrısı. Sonuçları metrics'e ekler. `FinishedConsumeWork` mesajını scheduler'a gönderir.

### fn ConsumeWorker::get_consume_bank (satır 108-112)

```rust
fn get_consume_bank(&self) -> Option<Arc<Bank>> {
    self.leader_bank_notifier
        .get_or_wait_for_in_progress(Duration::from_millis(50))
        .upgrade()
}
```

50ms bekleme ile `Weak<Bank>` → `Option<Arc<Bank>>`. Bank drop edilmişse `None`.

### fn ConsumeWorker::retry_drain (satır 115-120)

Bank yokken gelen tüm iş'leri retry olarak işaretler. Banka gelene kadar tx'leri kaybetmez.

### fn try_drain_iter (satır 145-147)

```rust
fn try_drain_iter<T>(work: T, receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    std::iter::once(work).chain(receiver.try_iter())
}
```

Mevcut item + channel'daki tüm hazır item'lar. **Non-blocking** drain. `try_iter()` boş channel'da hemen biter.

### Struct: ConsumeWorkerMetrics (satır 153-161)

```rust
pub(crate) struct ConsumeWorkerMetrics {
    id: u32,
    interval: AtomicInterval,
    has_data: AtomicBool,
    count_metrics: ConsumeWorkerCountMetrics,
    error_metrics: ConsumeWorkerTransactionErrorMetrics,
    timing_metrics: ConsumeWorkerTimingMetrics,
}
```

**Tasarım:** Tüm alanlar atomik — metrics thread'i (scheduler) ve consume thread aynı struct'a eş zamanlı yazıp okuyabilir.

### fn ConsumeWorkerMetrics::maybe_report_and_reset (satır 164-174)

```rust
pub fn maybe_report_and_reset(&self) {
    const REPORT_INTERVAL_MS: u64 = 1000;
    if self.interval.should_update(REPORT_INTERVAL_MS)
        && self.has_data.swap(false, Ordering::Relaxed)
    { ... }
}
```

1000ms'de bir rapor. `has_data` ile boş periyotlarda rapor atlanır.

### Struct: ConsumeWorkerCountMetrics (satır 386-395)

Fields:
- `transactions_attempted_execution_count: AtomicUsize`
- `executed_transactions_count: AtomicUsize`
- `executed_with_successful_result_count: AtomicUsize`
- `retryable_transaction_count: AtomicUsize`
- `retryable_expired_bank_count: AtomicUsize`
- `cost_model_throttled_transactions_count: AtomicUsize`
- `min_prioritization_fees: AtomicU64` — `u64::MAX` ile initialize (minimum bulmak için)
- `max_prioritization_fees: AtomicU64` — `0` ile initialize

### Struct: ConsumeWorkerTimingMetrics (satır 466-475)

```rust
#[derive(Default)]
struct ConsumeWorkerTimingMetrics {
    cost_model_us: AtomicU64,
    collect_balances_us: AtomicU64,
    load_execute_us: AtomicU64,
    freeze_lock_us: AtomicU64,
    last_blockhash_us: AtomicU64,
    record_us: AtomicU64,
    commit_us: AtomicU64,
    find_and_send_votes_us: AtomicU64,
}
```

### Struct: ConsumeWorkerTransactionErrorMetrics (satır 518-543)

21 ayrı hata sayacı. Her bir `TransactionError` varyantı için ayrı `AtomicUsize`. `report_and_reset()` datapoint olarak yayınlar.

### Test Modülü (satır 655-965)

**struct TestFrame:** Test için tüm gerekli kaynakları (channel'lar, bank, poh_recorder, vb.) tutar. `#[allow(dead_code)]` — tüm alanlar test sırasında gerekli olmayabilir.

**test_worker_consume_no_bank:** Bank olmadan gelen iş'in retry edildiğini doğrular.

**test_worker_consume_simple:** Normal başarılı tx işlemi.

**test_worker_consume_self_conflicting:** Aynı batch'teki çakışan tx'lerin düzgün ayrıştırılması (id2 retry olmalı).

**test_worker_consume_multiple_messages:** Ardışık iki batch'in doğru sırayla işlenmesi.

---

## 5. decision_maker.rs (294 satır)

**Konum:** `core/src/banking_stage/decision_maker.rs`

### Enum: BufferedPacketsDecision (satır 13-19)

```rust
#[derive(Debug, Clone)]
pub enum BufferedPacketsDecision {
    Consume(BankStart),   // Aktif bank var, hemen işle
    Forward,              // Başka leader var, yönlendir ve at
    ForwardAndHold,       // ~20 slot içinde leader olunacak, yönlendir ama tut
    Hold,                 // Belirsiz durum veya yakın gelecekte leader, bekle
}
```

`bank_start()` metodu: `Consume` variantından `BankStart` çıkarır.

### Struct: DecisionMaker (satır 31-35)

```rust
#[derive(Clone)]
pub struct DecisionMaker {
    my_pubkey: Pubkey,
    poh_recorder: Arc<RwLock<PohRecorder>>,
}
```

### fn DecisionMaker::make_consume_or_forward_decision (satır 45-59)

Lock süresi minimize edilir: `poh_recorder.read()` lock altında tüm bilgiler toplanır, karar lock dışında verilir (aslında closure ile içeride verilir ama lock scope minimized).

### fn DecisionMaker::consume_or_forward_packets (satır 61-92)

Karar ağacı (öncelik sırasıyla):
1. Aktif bank varsa → **Consume**
2. `would_be_leader_shortly()` (FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET - 1 ticks içinde) → **Hold**
3. `would_be_leader()` (HOLD_TRANSACTIONS_SLOT_OFFSET ticks içinde) → **ForwardAndHold**
4. `leader_pubkey()` biliniyorsa:
   - Benim pubkey'm değilse → **Forward**
   - Benim pubkey'im ise → **Hold**
5. Hiçbiri → **Hold** (bilinmeyen leader)

**Tasarım:** Closure parametreleri ile testedilebilirlik sağlanır. Her check bir closure, unit testlerde `panic!("should not be called")` ile çağrılmaması doğrulanır.

### Sabitler (kullanılan)

- `FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET`: 2 slot — bu kadar önce yönlendirmeye başla
- `HOLD_TRANSACTIONS_SLOT_OFFSET`: 20 slot — bu kadar önce tut
- `DEFAULT_TICKS_PER_SLOT`: Slot başına tick sayısı

### Test Modülü (satır 115-294)

- `test_buffered_packet_decision_bank_start()`: `bank_start()` accessor doğrulaması
- `test_make_consume_or_forward_decision()`: Gerçek PohRecorder ile entegrasyon testi; slot offset'leri doğru karar üretiyor mu
- `test_should_process_or_forward_packets()`: Pure unit test, closure injection ile tüm karar dalları test edilir

---

## 6. forwarder.rs (496 satır)

**Konum:** `core/src/banking_stage/forwarder.rs`

### Struct: Forwarder (satır 27-34)

```rust
pub(crate) struct Forwarder {
    poh_recorder: Arc<RwLock<PohRecorder>>,
    bank_forks: Arc<RwLock<BankForks>>,
    socket: UdpSocket,              // sadece oy yönlendirme için
    cluster_info: Arc<ClusterInfo>,
    connection_cache: Arc<ConnectionCache>, // QUIC/UDP connection pool
    data_budget: Arc<DataBudget>,   // rate limiter
}
```

`socket` `UdpSocket::bind("0.0.0.0:0")` ile random port'ta oluşturulur. Oy tx'leri UDP, normal tx'ler QUIC.

### fn Forwarder::handle_forwarding (satır 54-142)

Ana yönlendirme akışı:
1. Root bank'ı al (sanitizasyon için)
2. `ForwardPacketBatchesByAccounts::new_with_default_batch_limits()` oluştur
3. `filter_forwardable_packets_and_add_batches()` — geçersiz/eski tx'leri filtrele, batch'lere ekle
4. Her batch için `forward_buffered_packets()` çağrısı
5. `hold=false` ise: yönlendirilen tx'leri buffer'dan sil

### fn Forwarder::forward_packets (satır 147-180, pub)

```rust
pub(crate) fn forward_packets<'a>(
    &self,
    forward_option: &ForwardOption,
    forwardable_packets: impl Iterator<Item = &'a Packet>,
) -> (Result<(), TransportError>, usize, u64, Option<Pubkey>)
```

1. `get_leader_and_addr()` ile hedef al; yoksa `(Ok(()), 0, 0, None)` döndür
2. `update_data_budget()` ile rate limiti güncelle
3. `p.meta().forwarded()` zaten yönlendirilmiş olanları filtrele
4. `data_budget.take(p.meta().size)` ile rate limiti uygula
5. `forward()` ile gönder

### fn Forwarder::update_data_budget (satır 235-247)

```rust
fn update_data_budget(&self) {
    const INTERVAL_MS: u64 = 100;
    const MAX_BYTES_PER_SECOND: usize = 12_000_000;    // 12 MB/s
    const MAX_BYTES_PER_INTERVAL: usize = MAX_BYTES_PER_SECOND * INTERVAL_MS as usize / 1000;
    const MAX_BYTES_BUDGET: usize = MAX_BYTES_PER_INTERVAL * 5;   // burst: 5 interval'lık birikim
    self.data_budget.update(INTERVAL_MS, |bytes| {
        std::cmp::min(bytes.saturating_add(MAX_BYTES_PER_INTERVAL), MAX_BYTES_BUDGET)
    });
}
```

100ms'de bir 1.2MB token eklenir. Maksimum biriktirebilecek: 6MB (5 interval). 12MB/s çıkış hız sınırı.

### fn Forwarder::forward (satır 249-267)

```rust
fn forward(&self, forward_option: &ForwardOption, packet_vec: Vec<Vec<u8>>, addr: &SocketAddr)
    -> Result<(), TransportError>
{
    match forward_option {
        ForwardTpuVote => batch_send(&self.socket, &pkts), // UDP
        ForwardTransaction => conn.send_data_batch_async(packet_vec), // QUIC
        NotForward => panic!("should not forward"),
    }
}
```

### fn Forwarder::get_leader_and_addr (satır 220-232)

```rust
fn get_leader_and_addr(&self, forward_option: &ForwardOption) -> Option<(Pubkey, SocketAddr)> {
    match forward_option {
        NotForward => None,
        ForwardTransaction => next_leader(&cluster_info, &poh_recorder, |node| {
            node.tpu_forwards(connection_cache.protocol())
        }),
        ForwardTpuVote => next_leader_tpu_vote(&cluster_info, &poh_recorder),
    }
}
```

### Test Modülü (satır 270-495)

- `test_forwarder_budget` ve `test_handle_forwarding` — `#[ignore]` ile işaretlenmiş (network testi gerektiriyor)
- DataBudget kısıtlı vs açık testler
- `forwarded` flag'li paketlerin yönlendirilmediği doğrulanıyor
- `hold=true` ve `hold=false` senaryoları

---

## 7. forward_packet_batches_by_accounts.rs (336 satır)

**Konum:** `core/src/banking_stage/forward_packet_batches_by_accounts.rs`

### Sabitler (satır 16-22)

```rust
const FORWARDED_BLOCK_COMPUTE_RATIO: u32 = 2;    // Her batch, normal blok limitinin yarısı
const DEFAULT_NUMBER_OF_BATCHES: u32 = 100;       // 100 batch → 12k default tx kapasitesi
```

### Struct: ForwardBatch (satır 27-32)

```rust
#[derive(Debug)]
pub struct ForwardBatch {
    cost_tracker: CostTracker,
    forwardable_packets: Vec<Arc<ImmutableDeserializedPacket>>,
}
```

Her batch kendi `CostTracker`'ına sahip. Böylece her batch bağımsız account ve blok cost limitlerine sahip.

### impl Default for ForwardBatch (satır 34-39)

`ForwardBatch::new(1)` — full block limits.

### fn ForwardBatch::new (satır 46-57)

```rust
fn new(limit_ratio: u32) -> Self {
    let mut cost_tracker = CostTracker::default();
    cost_tracker.set_limits(
        MAX_WRITABLE_ACCOUNT_UNITS / limit_ratio,
        MAX_BLOCK_UNITS / limit_ratio,
        MAX_VOTE_UNITS / limit_ratio,
    );
    Self { cost_tracker, forwardable_packets: Vec::default() }
}
```

`limit_ratio=2` → her account en fazla blok limitinin yarısı kadar CU alabilir.

### fn ForwardBatch::try_add (satır 59-71)

1. `CostModel::calculate_cost()` ile tx maliyeti hesapla
2. `cost_tracker.try_add()` — limit aşılırsa `Err(CostTrackerError)`
3. Başarılıysa paketi ekle

### Struct: ForwardPacketBatchesByAccounts (satır 93-98)

```rust
#[derive(Debug)]
pub struct ForwardPacketBatchesByAccounts {
    forward_batches: Vec<ForwardBatch>,
}
```

100 batch içerir. Sıcak account'ların farklı batch'lere dağılmasını sağlar.

### fn ForwardPacketBatchesByAccounts::try_add_packet (satır 113-128)

Tx'i birinci uygun batch'e ekler. İlk batch dolarsa ikinci batch'e, o da dolarsa üçüncüye. 100 batch dolunca `false` döner (tx yönlendirilemez).

**Tasarım kararı:** Hot account'ların blokları tıkaması engellenir. Örn: tek bir account'ı kullanan 100 yüksek öncelikli tx aynı batch'leri doldurmaz; düşük öncelikli ama farklı account kullanan tx'ler için yer kalır.

### Test Modülü (satır 135-335)

- `test_try_add_to_forward_batch()`: Aynı account ikinci kez eklenemiyor
- `test_try_add_packeti_to_multiple_batches()`: Hot account batch'lere dağılıyor, düşük öncelikli tx 1. batch'e girebiliyor
- `test_try_add_packet_to_single_batch()`: Tek batch senaryosu

---

## 8. forward_worker.rs (228 satır)

**Konum:** `core/src/banking_stage/forward_worker.rs`

### Enum: ForwardWorkerError (satır 12-17)

```rust
#[derive(Debug, Error)]
pub enum ForwardWorkerError {
    #[error("Failed to receive work from scheduler: {0}")]
    Recv(#[from] RecvError),
    #[error("Failed to send finalized forward work to scheduler: {0}")]
    Send(#[from] SendError<FinishedForwardWork>),
}
```

### Struct: ForwardWorker (satır 19-24)

```rust
pub(crate) struct ForwardWorker {
    forward_receiver: Receiver<ForwardWork>,
    forward_option: ForwardOption,
    forwarder: Forwarder,
    forwarded_sender: Sender<FinishedForwardWork>,
}
```

### fn ForwardWorker::run (satır 42-47)

ConsumeWorker ile aynı pattern: blocking recv + loop.

### fn ForwardWorker::forward_loop (satır 49-64)

`try_drain_iter` ile tüm hazır işleri tüket. İlk hata durumunda `failed_forward_drain()`.

### fn ForwardWorker::failed_forward_drain (satır 66-74)

Hata sonrası tüm bekleyen işleri `successful: false` ile scheduler'a bildir.

### fn try_drain_iter — consume_worker.rs ile aynı pattern

```rust
fn try_drain_iter<T>(work: T, receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    std::iter::once(work).chain(receiver.try_iter())
}
```

### Test Modülü (satır 83-228)

`test_worker_forward_simple`: İki tx yönlendirme, `forwarded.successful == true` doğrulaması.

---

## 9. immutable_deserialized_packet.rs (169 satır)

**Konum:** `core/src/banking_stage/immutable_deserialized_packet.rs`

### Enum: DeserializedPacketError (satır 21-35)

```rust
#[derive(Debug, Error)]
pub enum DeserializedPacketError {
    #[error("ShortVec Failed to Deserialize")]
    ShortVecError(()),
    #[error("Deserialization Error: {0}")]
    DeserializationError(#[from] bincode::Error),
    #[error("overflowed on signature size {0}")]
    SignatureOverflowed(usize),
    #[error("packet failed sanitization {0}")]
    SanitizeError(#[from] SanitizeError),
    #[error("transaction failed prioritization")]
    PrioritizationFailure,
    #[error("vote transaction failure")]
    VoteTransactionError,
}
```

### Struct: ImmutableDeserializedPacket (satır 37-44)

```rust
#[derive(Debug, PartialEq, Eq)]
pub struct ImmutableDeserializedPacket {
    original_packet: Packet,
    transaction: SanitizedVersionedTransaction,
    message_hash: Hash,
    is_simple_vote: bool,
    compute_budget_details: ComputeBudgetDetails,
}
```

**Immutable:** Deserialize edildikten sonra değiştirilemez. `Arc<ImmutableDeserializedPacket>` olarak paylaşılır.

**message_hash:** `Message::hash_raw_message()` ile ham bayt'tan hash. Deduplication için kullanılır.

### fn ImmutableDeserializedPacket::new (satır 47-71)

```rust
pub fn new(packet: Packet) -> Result<Self, DeserializedPacketError> {
    let versioned_transaction: VersionedTransaction = packet.deserialize_slice(..)?;
    let sanitized_transaction = SanitizedVersionedTransaction::try_from(versioned_transaction)?;
    let message_bytes = packet_message(&packet)?;
    let message_hash = Message::hash_raw_message(message_bytes);
    let is_simple_vote = packet.meta().is_simple_vote_tx();

    let mut compute_budget_details = sanitized_transaction
        .get_compute_budget_details(packet.meta().round_compute_unit_price())
        .ok_or(DeserializedPacketError::PrioritizationFailure)?;

    if is_simple_vote {
        compute_budget_details.compute_unit_price = 0;  // oylar ücretsiz kuyruğa girer
    }
    Ok(Self { ... })
}
```

**Tasarım:** Oy tx'leri compute_unit_price = 0 olarak zorlanır. Bu, oy tx'lerinin fee tabanlı öncelik sıralamasını atlamasını sağlar (özel oy kuyruğunda işlenir).

### impl Ord for ImmutableDeserializedPacket (satır 130-134)

```rust
impl Ord for ImmutableDeserializedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compute_unit_price().cmp(&other.compute_unit_price())
    }
}
```

`MinMaxHeap` öncelik sıralaması için.

### fn packet_message (satır 136-147)

```rust
fn packet_message(packet: &Packet) -> Result<&[u8], DeserializedPacketError> {
    let (sig_len, sig_size) = packet.data(..)
        .and_then(|bytes| decode_shortu16_len(bytes).ok())
        .ok_or(ShortVecError(()))?;
    sig_len.checked_mul(size_of::<Signature>())
        .and_then(|v| v.checked_add(sig_size))
        .and_then(|msg_start| packet.data(msg_start..))
        .ok_or(SignatureOverflowed(sig_size))
}
```

ShortVec formatından imza sayısını decode eder, imza boyutunu atlar, mesaj başlangıcını bulur. `checked_mul/add` ile integer overflow koruması.

### fn build_sanitized_transaction (satır 103-121)

Address loader ile tam sanitize işlemi + precompile doğrulaması (secp256k1, ed25519). `votes_only=true` ise non-vote tx'leri filtreler.

---

## 10. latest_unprocessed_votes.rs (872 satır)

**Konum:** `core/src/banking_stage/latest_unprocessed_votes.rs`

### Enum: VoteSource (satır 23-27)

```rust
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum VoteSource {
    Gossip,  // gossip ağından gelen oy
    Tpu,     // TPU üzerinden gelen oy (validator direkt yollamış)
}
```

### Struct: LatestValidatorVotePacket (satır 31-38)

```rust
#[derive(Debug, Clone)]
pub struct LatestValidatorVotePacket {
    vote_source: VoteSource,
    pubkey: Pubkey,
    vote: Option<Arc<ImmutableDeserializedPacket>>,  // None = already taken
    slot: Slot,          // oy verilen son slot
    forwarded: bool,
    timestamp: Option<UnixTimestamp>,
}
```

**vote: Option:** Oy paketi alındığında `Some`, işlendiğinde `take()` ile `None` yapılır. Gossip oyları her zaman `is_forwarded() = true` sayılır.

### fn LatestValidatorVotePacket::new_from_immutable (satır 50-83)

1. Paketin `is_simple_vote_tx()` olduğunu doğrula
2. İlk instruction'ı deserialize et → `VoteInstruction`
3. Sadece `is_single_vote_state_update()` olan oyları kabul et (VoteStateUpdate)
4. pubkey, slot, timestamp çıkar

**Tasarım kararı:** Sadece `VoteStateUpdate` tipini destekler. Eski `Vote` ve `VoteSwitch` tipleri reddedilir. Tower BFT'nin yeni oy formatı.

### fn weighted_random_order_by_stake (satır 118-136)

```rust
pub(crate) fn weighted_random_order_by_stake<'a>(
    bank: &Bank,
    pubkeys: impl Iterator<Item = &'a Pubkey>,
) -> impl Iterator<Item = Pubkey>
```

**Efraimidis-Spirakis algoritması:** Her validator için `random^(1/stake)` değeri hesaplanır. Büyük stake'e sahip validatörler daha yüksek değer alır (power mean). Listeyi sırala → büyük stake daha erken işlenir.

**Amaç:** Stake-weighted sampling. 0 stake'li validatörler atlanır.

### Struct: LatestUnprocessedVotes (satır 144-147)

```rust
#[derive(Debug, Default)]
pub struct LatestUnprocessedVotes {
    latest_votes_per_pubkey: RwLock<HashMap<Pubkey, Arc<RwLock<LatestValidatorVotePacket>>>>,
}
```

**İç içe RwLock:** Outer lock HashMap yapısını, inner lock ise her paketin durumunu korur. Farklı pubkey'lerin oyları paralel güncellenebilir.

### fn LatestUnprocessedVotes::update_latest_vote (satır 201-237)

**Çift kontrol (double-checked locking) pattern:**
1. Önce `read()` lock ile son slot/timestamp kontrol et
2. Eğer yeni oy daha güncel ise `write()` lock al
3. Write lock altında tekrar kontrol et (TOCTOU koruması)
4. Eski oyu `std::mem::replace()` ile al ve döndür

**Optimistic locking:** Çoğu durumda oy güncel değildir, sadece read lock yeter.

### fn LatestUnprocessedVotes::get_and_insert_forwardable_packets (satır 261-308)

1. `weighted_random_order_by_stake()` ile stake-weighted sıralama
2. Her pubkey için: alınmamış ve yönlendirilmemiş oyları sanitize et
3. `forward_packet_batches_by_accounts.try_add_packet()` ile batch'e ekle
4. İlk başarısızlıkta yönlendirmeyi durdur (normal tx davranışıyla tutarlı)

### fn LatestUnprocessedVotes::drain_unprocessed (satır 311-326)

Stake-weighted sırayla tüm işlenmemiş oyları al. `take_vote()` ile paketi `None` yap (idempotent drain).

### fn LatestUnprocessedVotes::clear_forwarded_packets (satır 330-342)

Yönlendirilmiş ve gossip'ten gelen oyları temizle. Double-check with write lock.

### Test Modülü (satır 345-872)

- `test_deserialize_vote_packets()`: 5 paket türü arasında sadece 2'si kabul edilir (VoteStateUpdate)
- `test_update_latest_vote()`: Slot güncelleme, timestamp güncelleme, eski oy yenisiyle değiştirilir
- `test_simulate_threads()`: Gossip + TPU thread'leri eş zamanlı 1000 oy gönderir, veri yarışı testi
- `test_forwardable_packets()`: 0 stake validatörü yönlendirilmez, gossip oyu yönlendirilmez
- `test_clear_forwarded_packets()`: 4 oy ekle → temizle → 1 kalır (Tpu, yönlendirilmemiş)

---

## 11. leader_slot_metrics.rs

**Konum:** `core/src/banking_stage/leader_slot_metrics.rs`

Çok kapsamlı metrics takibi. Her leader slot için:
- Alınan/düşürülen paket sayıları
- İşleme süreleri (microsecond granülerlikte)
- Slot sonu durumları

Ana struct: `LeaderSlotMetricsTracker` — slot başlamadan önce `None`, slot başladıktan sonra `Some(LeaderSlotMetrics)`.

`LeaderSlotMetrics` içinde:
- `slot: Slot`
- Çok sayıda `u64` sayaç (tüm atomik olmayan — sadece leader slot thread'i kullanır)
- `ProcessTransactionsSummary` accumulator metodları

`set_end_of_slot_unprocessed_buffer_len()` — slot sonunda buffer'da kalan paket sayısını kaydeder.

---

## 12. leader_slot_timing_metrics.rs

**Konum:** `core/src/banking_stage/leader_slot_timing_metrics.rs`

### Struct: LeaderExecuteAndCommitTimings

```rust
pub struct LeaderExecuteAndCommitTimings {
    pub collect_balances_us: u64,
    pub load_execute_us: u64,
    pub freeze_lock_us: u64,
    pub last_blockhash_us: u64,
    pub record_us: u64,
    pub commit_us: u64,
    pub find_and_send_votes_us: u64,
    pub execute_timings: ExecuteTimings,
    pub record_transactions_timings: RecordTransactionsTimings,
}
```

`accumulate()` metodu: Tüm alanları toplama ekler.

---

## 13. multi_iterator_scanner.rs (397 satır)

**Konum:** `core/src/banking_stage/multi_iterator_scanner.rs`

### Kavramsal Temel

Öncelik sıralı tx listesinde çakışan tx'ler birbirinin önüne geçmesini önler. Aynı harfle gösterilen tx'ler çakışır:

```
[A, A, B, A, C, D, B, C, D]
→ Batch 1: [A, B, C, D]
→ Batch 2: [A, B, C, D]
→ Batch 3: [A]
```

### Enum: ProcessingDecision (satır 18-25)

```rust
pub enum ProcessingDecision {
    Now,    // Bu batch'e dahil et
    Later,  // Bu batch'e ekleme, sonraki batch'lerde dene
    Never,  // Atla ve handled olarak işaretle
}
```

### Struct: MultiIteratorScanner (satır 62-83)

```rust
pub struct MultiIteratorScanner<'a, T, U, F>
where F: FnMut(&T, &mut U) -> ProcessingDecision,
{
    max_iterators: usize,
    slice: &'a [T],
    payload: U,                    // shared mutable state
    should_process: F,             // karar fonksiyonu (closure)
    already_handled: Vec<bool>,    // işlenmiş flag'ler
    current_positions: Vec<usize>, // her iterator'ın pozisyonu
    current_items: Vec<&'a T>,     // mevcut batch item'ları
    initialized: bool,
}
```

Generic: `T` = element tipi, `U` = payload (lock state vb.), `F` = karar closure.

### fn MultiIteratorScanner::iterate (satır 112-120)

```rust
pub fn iterate(&mut self) -> Option<(&[&'a T], &mut U)> {
    if !self.initialized {
        self.initialized = true;
        self.initialize_current_positions();
    } else {
        self.advance_current_positions();
    }
    self.get_current_items()
}
```

İlk çağrıda initialize, sonraki çağrılarda advance. `None` dönünce bitti.

### fn MultiIteratorScanner::march_iterator (satır 179-200)

Verilen `starting_index`'ten itibaren `slice`'ı tara:
- `already_handled[index]` atla
- `ProcessingDecision::Now`: `already_handled[index] = true`, bu index döndür
- `ProcessingDecision::Later`: geç, bir sonraki iterasyonda dene
- `ProcessingDecision::Never`: `already_handled[index] = true`, geç

### fn MultiIteratorScanner::initialize_current_positions (satır 131-143)

`max_iterators` kadar pozisyon bul. Her iterator bir öncekinin `index + 1`'inden başlar. Böylece batch içi örtüşme olmaz.

### fn MultiIteratorScanner::advance_current_positions (satır 145-166)

Her iterator için `start_index = max(current_pos + 1, prev_index + 1)`. Son iterator sınırı aşarsa `truncate`.

### Struct: PayloadAndAlreadyHandled (satır 85-88)

```rust
pub struct PayloadAndAlreadyHandled<U> {
    pub payload: U,
    pub already_handled: Vec<bool>,
}
```

`finalize()` metodu bu struct'ı döndürür. Hangi öğelerin işlendiği bilgisi dışarıya taşınır.

### Test Modülü (satır 203-396)

- `test_multi_iterator_scanner_empty()`: Boş slice → hemen `None`
- `test_multi_iterator_scanner_iterate()`: max_iterators=2, 11 elemanlı liste → 6 batch
- `test_multi_iterator_scanner_iterate_with_gaps()`: Lock-based çakışma testi
- `test_multi_iterator_scanner_iterate_conflicts_not_at_front()`: Çakışmalar başta değil ortada
- `test_multi_iterator_scanner_iterate_with_never_process()`: `Never` olanlar atlanır
- `test_multi_iterator_scanner_iterate_not_handled()`: `Later` olanlar `already_handled=false` kalır

---

## 14. packet_deserializer.rs (232 satır)

**Konum:** `core/src/banking_stage/packet_deserializer.rs`

### Struct: ReceivePacketResults (satır 19-28)

```rust
pub struct ReceivePacketResults {
    pub deserialized_packets: Vec<ImmutableDeserializedPacket>,
    pub new_tracer_stats_option: Option<SigverifyTracerPacketStats>,
    pub passed_sigverify_count: u64,
    pub failed_sigverify_count: u64,
}
```

### Struct: PacketDeserializer (satır 30-35)

```rust
pub struct PacketDeserializer {
    packet_batch_receiver: BankingPacketReceiver,
    bank_forks: Arc<RwLock<BankForks>>,
}
```

### fn PacketDeserializer::receive_packets (satır 49-66)

1. `receive_until()` ile paket topla
2. Feature flag kontrol (TODO: `round_compute_unit_price`)
3. `deserialize_and_collect_packets()` ile deserialize et

**TODO notu (satır 59):** `round_compute_unit_price_enabled = false` — feature aktifleştirme sonradan yapılacak.

### fn PacketDeserializer::receive_until (satır 116-146)

1. İlk paket batchi `recv_timeout` ile bekle (blocking)
2. Sonraki batchleri `try_recv()` ile non-blocking al
3. Timeout veya `packet_count_upperbound` dolana kadar devam et

**Batch toplama:** Tek bir `recv_timeout` süresi içinde gelen tüm batchleri birleştirir. Bu throughput'u artırır.

### fn PacketDeserializer::generate_packet_indexes (satır 148-155)

`discard()` flag'i set edilmemiş paketlerin index'lerini toplar. SigVerify tarafından geçersiz imzalı paketler `discard=true` ile işaretlenir.

### fn PacketDeserializer::deserialize_packets (satır 157-169)

Her paketi klonla, `round_compute_unit_price` flag'ini set et, `ImmutableDeserializedPacket::new()` ile deserialize et. Hata durumunda filtrele (`filter_map`).

### Test Modülü (satır 172-232)

- `test_deserialize_and_collect_packets_empty()`: Boş input
- `test_deserialize_and_collect_packets_simple_batches()`: 2 tx, 2 batch
- `test_deserialize_and_collect_packets_simple_batches_with_failure()`: 1 discard → 1 passed, 1 failed

---

## 15. packet_receiver.rs (180 satır)

**Konum:** `core/src/banking_stage/packet_receiver.rs`

### Struct: PacketReceiver (satır 20-23)

```rust
pub struct PacketReceiver {
    id: u32,
    packet_deserializer: PacketDeserializer,
}
```

### fn PacketReceiver::receive_and_buffer_packets (satır 38-74)

Ana giriş noktası. `recv_timeout` hesaplar, `PacketDeserializer::receive_packets()` çağırır, `buffer_packets()` ile buffer'a ekler.

### fn PacketReceiver::get_receive_timeout (satır 76-90)

```rust
fn get_receive_timeout(storage: &UnprocessedTransactionStorage) -> Duration {
    if !storage.is_empty() {
        Duration::from_millis(0)  // buffer doluysa try_recv
    } else {
        Duration::from_millis(100)  // boşsa 100ms bekle
    }
}
```

**Adaptif timeout:** Buffer doluysa yeni paket gelmesini beklemeden işlemeye devam eder. Bu, cost model throttled tx'lerin oturmasını engeller.

### fn PacketReceiver::buffer_packets (satır 92-142)

Gelen deserialized paketleri storage'a ekler. Tracer stats toplar. Dropped/buffered sayaçlarını günceller.

### fn PacketReceiver::push_unprocessed (satır 144-179)

`unprocessed_transaction_storage.insert_batch()` çağrısı. Düşürülen tracer paket sayısını takip eder.

---

## 16. qos_service.rs (kapsamlı)

**Konum:** `core/src/banking_stage/qos_service.rs`

### Struct: QosService (satır 25-27)

```rust
pub struct QosService {
    metrics: QosServiceMetrics,
}
```

Her banking thread'e bir tane. Thread-local (Rc yok ama Arc da yok).

### fn QosService::select_and_accumulate_transaction_costs (satır 40-63)

Ana QoS akışı:
1. `compute_transaction_costs()`: Her tx için `CostModel::calculate_cost()`
2. `select_transactions_per_cost()`: `CostTracker` üzerinden blok kapasitesi kontrolü
3. `accumulate_estimated_transaction_costs()`: Metrics biriktir

Döndürür: `(Vec<Result<TransactionCost>>, throttled_count)`.

### fn QosService::select_transactions_per_cost (satır 93-130)

`bank.write_cost_tracker()` ile maliyet takipçisini kitle. Her tx için `cost_tracker.try_add()`:
- Başarılı → `Ok(cost)`, `num_included++`
- Başarısız → `Err(WouldExceedMaxBlockCostLimit)` vb.

**Önemli:** Cost tracker lock'u bu fonksiyon süresince tutulur. Bu, paralel tx'lerin birbirinin kapasitesini aşmamasını sağlar.

### fn QosService::update_costs (satır 134-146)

Commit edilen tx'lerin gerçek CU'larıyla cost tracker güncellenir. Feature `apply_cost_tracker_during_replay` aktif değilse çağrılır.

### fn QosService::remove_costs (satır 149-162)

Commit edilmemiş tx'lerin maliyetlerini geri alır. `CommitTransactionDetails::NotCommitted` → `cost_tracker.remove()`.

### fn QosService::remove_uncommitted_transaction_costs (satır 164-181)

Commit durumuna göre seçici silme. Sadece QoS servisinin dahil ettiği tx'ler (`Ok(tx_cost)`) kontrol edilir.

### fn QosService::update_data_budget — qos_service içinde değil, forwarder'da

### QosServiceMetrics

İki alt struct: `QosServiceStats` ve `QosServiceErrors`. Her ikisi de atomik sayaçlar. 1 saniyelik raporlama. Datapoint'ler: `qos_service_stats`, `qos_service_errors`.

---

## 17. read_write_account_set.rs (283 satır)

**Konum:** `core/src/banking_stage/read_write_account_set.rs`

### Struct: ReadWriteAccountSet (satır 8-13)

```rust
#[derive(Debug, Default)]
pub struct ReadWriteAccountSet {
    read_set: HashSet<Pubkey>,
    write_set: HashSet<Pubkey>,
}
```

**Lock semantiği:**
- `read_set`'e eklemek: write_set'te yoksa OK
- `write_set`'e eklemek: her iki set'te de yoksa OK
- Read-read: çakışma yok
- Read-write veya write-write: çakışma var

### fn check_locks (satır 18-30)

`take_locks`'tan farklı olarak set'leri değiştirmez. Sadece kontrol eder.

### fn take_locks (satır 34-46)

```rust
pub fn take_locks(&mut self, message: &SanitizedMessage) -> bool {
    message.account_keys().iter().enumerate()
        .fold(true, |all_available, (index, pubkey)| {
            if message.is_writable(index) {
                all_available & self.add_write(pubkey)
            } else {
                all_available & self.add_read(pubkey)
            }
        })
}
```

Hata durumunda da tüm lock'lar alınır (`fold` devam eder). `all_available` sadece boolean sonucu takip eder.

**Tasarım kararı:** Lock rollback yok. Çağıran conflict durumunda set'i `clear()` etmeli veya başka mekanizma kullanmalı.

### Test Modülü (satır 83-283)

`create_test_versioned_message()`, `create_test_sanitized_transaction()`, `create_test_address_lookup_table()` yardımcıları.

Test matrisi (`test_check_and_take_locks`):
- `conflict_index = 0,1,2,3` × `add_write = true/false` × beklenti

---

## 18. scheduler_messages.rs (67 satır)

**Konum:** `core/src/banking_stage/scheduler_messages.rs`

### Struct: TransactionBatchId (satır 9)

```rust
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct TransactionBatchId(u64);
```

Newtype wrapper. `Display` impl.

### Struct: TransactionId (satır 25)

```rust
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct TransactionId(u64);
```

### Struct: ConsumeWork (satır 41-46)

```rust
pub struct ConsumeWork {
    pub batch_id: TransactionBatchId,
    pub ids: Vec<TransactionId>,
    pub transactions: Vec<SanitizedTransaction>,
    pub max_age_slots: Vec<Slot>,
}
```

**[Scheduler → ConsumeWorker]** mesajı. `max_age_slots`: her tx'in ne kadar eski olabileceği.

### Struct: ForwardWork (satır 49-53)

```rust
pub struct ForwardWork {
    pub ids: Vec<TransactionId>,
    pub packets: Vec<Arc<ImmutableDeserializedPacket>>,
}
```

**[Scheduler → ForwardWorker]** mesajı.

### Struct: FinishedConsumeWork (satır 57-60)

```rust
pub struct FinishedConsumeWork {
    pub work: ConsumeWork,
    pub retryable_indexes: Vec<usize>,
}
```

**[ConsumeWorker → Scheduler]** cevabı.

### Struct: FinishedForwardWork (satır 63-66)

```rust
pub struct FinishedForwardWork {
    pub work: ForwardWork,
    pub successful: bool,
}
```

---

## 19. transaction_scheduler/

### 19.1 mod.rs

```rust
mod batch_id_generator;
#[allow(dead_code)] mod in_flight_tracker;
pub(crate) mod prio_graph_scheduler;
pub(crate) mod scheduler_controller;
pub(crate) mod scheduler_error;
mod scheduler_metrics;
mod thread_aware_account_locks;
mod transaction_id_generator;
mod transaction_priority_id;
#[allow(dead_code)] mod transaction_state;
#[allow(dead_code)] mod transaction_state_container;
```

`#[allow(dead_code)]` — aktif geliştirme altında olan modüller.

### 19.2 scheduler_error.rs

```rust
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Sending channel disconnected: {0}")]
    DisconnectedSendChannel(&'static str),
    #[error("Recv channel disconnected: {0}")]
    DisconnectedRecvChannel(&'static str),
}
```

### 19.3 batch_id_generator.rs

```rust
#[derive(Default)]
pub struct BatchIdGenerator {
    next_id: u64,
}

impl BatchIdGenerator {
    pub fn next(&mut self) -> TransactionBatchId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_sub(1); // u64::MAX'tan geriye doğru
        TransactionBatchId::new(id)
    }
}
```

**Tasarım:** `0`'dan başlar, `wrapping_sub(1)` ile `u64::MAX`, `u64::MAX-1`... şeklinde devam eder. Bu `InFlightTracker`'ın `BatchIdGenerator`'la entegrasyonunda birbirini tamamlar.

### 19.4 transaction_priority_id.rs

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransactionPriorityId {
    pub(crate) priority: u64,
    pub(crate) id: TransactionId,
}
```

`Ord` impl sadece `priority` üzerinden. `Hash` impl sadece `id` üzerinden. Bu sayede `MinMaxHeap`'te priority'e göre sıralama, `HashMap`'te id'ye göre lookup.

`TopLevelId<Self>` trait implementasyonu — `prio_graph` kütüphanesinin arayüzü.

### 19.5 transaction_state.rs

**Enum: TransactionState (satır 29-38)**

```rust
#[allow(clippy::large_enum_variant)]
pub(crate) enum TransactionState {
    Unprocessed {
        transaction_ttl: SanitizedTransactionTTL,
        priority: u64,
        cost: u64,
    },
    Pending { priority: u64, cost: u64 },
}
```

`#[allow(clippy::large_enum_variant)]` — `Unprocessed` varyantı `SanitizedTransaction` içerdiği için büyük. Boxing yapmak yerine stack'te tutulur (performans).

**State machine:**
- `Unprocessed` → (transition_to_pending) → `Pending` → tx gönderildi
- `Pending` → (transition_to_unprocessed) → `Unprocessed` → retry

**fn take() (satır 125-132):** `std::mem::replace` ile dummy `Pending{0,0}` yerleştirir, eski state'i döndürür. Klonlamadan state transferi.

### Struct: SanitizedTransactionTTL (satır 4-7)

```rust
pub(crate) struct SanitizedTransactionTTL {
    pub(crate) transaction: SanitizedTransaction,
    pub(crate) max_age_slot: Slot,
}
```

### 19.6 transaction_state_container.rs

**Struct: TransactionStateContainer (satır 37-40)**

```rust
pub(crate) struct TransactionStateContainer {
    priority_queue: MinMaxHeap<TransactionPriorityId>,
    id_to_transaction_state: HashMap<TransactionId, TransactionState>,
}
```

İki veri yapısının senkronizasyonu kritik:
- `priority_queue`: pending olmayan id'ler (sıralamak için)
- `id_to_transaction_state`: tüm id'ler (hem pending hem unprocessed)

**fn insert_new_transaction (satır 96-109):**
1. Önce `id_to_transaction_state`'e ekle
2. Sonra `push_id_into_queue()` ile kuyruğa ekle
3. Kapasite dolunca en düşük öncelikli atılır

**fn push_id_into_queue (satır 129-138):**
```rust
if self.remaining_queue_capacity() == 0 {
    let popped_id = self.priority_queue.push_pop_min(priority_id);
    self.remove_by_id(&popped_id.id);
    true  // dropped
} else {
    self.priority_queue.push(priority_id);
    false
}
```

`push_pop_min` = push ve minimum'u al, tek operasyon (heap invariant korunur). Düşük öncelikli tx atılır.

**fn retry_transaction (satır 113-124):** State'i `Unprocessed`'a çevirir, kuyruğa geri koyar. Priority korunur.

**fn get_min_max_priority (satır 147-155):** `MinMaxHeap::peek_min/max()` O(1). Metrics için kullanılır.

### 19.7 in_flight_tracker.rs

**Struct: InFlightTracker (satır 8-13)**

```rust
pub struct InFlightTracker {
    num_in_flight_per_thread: Vec<usize>,
    cus_in_flight_per_thread: Vec<u64>,
    batches: HashMap<TransactionBatchId, BatchEntry>,
    batch_id_generator: BatchIdGenerator,
}
```

**struct BatchEntry (satır 15-19):**
```rust
struct BatchEntry {
    thread_id: ThreadId,
    num_transactions: usize,
    total_cus: u64,
}
```

**fn track_batch:** Yeni batch'in thread ve CU bilgisini kaydeder.
**fn complete_batch:** Batch tamamlandığında in-flight sayılarını azaltır. Olmayan batch_id → panic.

### 19.8 thread_aware_account_locks.rs

**Sabitler ve Tipler:**
```rust
pub(crate) const MAX_THREADS: usize = u64::BITS as usize; // 64
pub(crate) type ThreadId = usize;
type LockCount = u32;
```

**Struct: ThreadSet (satır 18)**

```rust
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct ThreadSet(u64);
```

64-bitlik thread bitmask. `ThreadSet::any(n)` → n bit 1 (n <= 64). `ThreadSet::only(id)` → sadece `id`'inci bit 1. `BitAnd`, `BitAndAssign`, `Sub` implement eder.

**struct AccountWriteLocks:** Sadece bir thread yazabilir.
**struct AccountReadLocks:** `thread_set` (kim okuyor) + `lock_counts[thread_id]` (kaç kez).

**fn schedulable_threads<const WRITE: bool> (satır 146-169):**

| Durum | Read-schedulable | Write-schedulable |
|-------|-----------------|-------------------|
| Lock yok | all threads | all threads |
| Sadece read lock | all threads | Tüm read thread'ler tek ise o thread, değilse none |
| Sadece write lock | write thread | write thread |
| Write + read (aynı thread) | write thread | write thread |

**Tasarım:** Aynı thread hem write hem read lock tutabilir (birden fazla tx aynı account'ı kullanabilir). Farklı thread'ler çakışamaz.

**fn try_lock_accounts (satır 70-86):**
1. `accounts_schedulable_threads()` — hangi thread'ler bu transaction'ı alabilir
2. `allowed_threads` ile kesişim
3. Boş değilse `thread_selector(schedulable_threads)` çağır ve lock al

**fn select_thread (PrioGraphScheduler'da):** En az in-flight tx'e sahip thread'i seç. Load balancing.

### 19.9 prio_graph_scheduler.rs (kapsamlı)

**Struct: PrioGraphScheduler (satır 25-31)**

```rust
pub(crate) struct PrioGraphScheduler {
    in_flight_tracker: InFlightTracker,
    account_locks: ThreadAwareAccountLocks,
    consume_work_senders: Vec<Sender<ConsumeWork>>,
    finished_consume_work_receiver: Receiver<FinishedConsumeWork>,
    look_ahead_window_size: usize,    // default: 2048
}
```

**fn PrioGraphScheduler::schedule (satır 64-253) — Ana Zamanlayıcı Algoritması:**

```
Parameters:
  - container: &mut TransactionStateContainer (pending tx'ler)
  - pre_graph_filter: fn(&[&SanitizedTx], &mut [bool])  → erken eleme
  - pre_lock_filter: fn(&SanitizedTx) -> bool           → lock öncesi eleme

Algorithm:
1. Batches::new(num_threads) → her thread için transaction listesi
2. unschedulable_ids = []  → çakışan tx'ler
3. blocking_locks = ReadWriteAccountSet::default()
4. prio_graph = PrioGraph::new(...)

5. chunked_pops closure (128'lik chunk'lar):
   - container'dan MAX_FILTER_CHUNK_SIZE'a kadar al
   - pre_graph_filter uygula (blok kontrolü, fee payer kontrolü)
   - Filtreyi geçenler → prio_graph.insert_transaction()
   - Filtreyi geçmeyenler → container'dan sil

6. İlk 2048'lik window doldur

7. Ana döngü (MAX_TRANSACTIONS_PER_SCHEDULING_PASS = 100_000):
   a. prio_graph.pop() — en yüksek öncelikli çakışmasız tx
   b. pre_lock_filter kontrolü
   c. blocking_locks.check_locks() → önceki unschedulable'larla çakışma var mı?
   d. account_locks.try_lock_accounts() → thread atama
   e. Atanamazsa: blocking_locks'a ekle, unschedulable olarak işaretle
   f. Atanırsa: transition_to_pending, batch'e ekle
   g. Batch dolunca send_batch()

8. Kalan batch'leri gönder
9. unschedulable_ids → container'a geri koy
10. prio_graph'ta kalan → container'a geri koy
```

**prio_graph kütüphanesi:** Tx'ler arası bağımlılık grafiği. Yüksek öncelikli tx hesaplanana kadar düşük öncelikli bağımlılar bekler. `pop()` bağımsız en yüksek öncelikli olanı döndürür.

**blocking_locks:** Multi-thread çakışmasından dolayı atılamayan tx'lerin account'larını takip eder. Bu account'larla çakışan başka tx'lerin önüne geçmesini önler.

**fn PrioGraphScheduler::send_batch (satır ~260-285):**

```rust
fn send_batch(&mut self, batches: &mut Batches, thread_id: usize) -> Result<usize, SchedulerError> {
    let batch_id = self.in_flight_tracker.track_batch(
        batches.ids[thread_id].len(),
        batches.total_cus[thread_id],
        thread_id,
    );
    let num_sent = batches.ids[thread_id].len();
    let work = ConsumeWork {
        batch_id,
        ids: std::mem::take(&mut batches.ids[thread_id]),
        transactions: std::mem::take(&mut batches.transactions[thread_id]),
        max_age_slots: std::mem::take(&mut batches.max_age_slots[thread_id]),
    };
    self.consume_work_senders[thread_id].send(work)
        .map_err(|_| SchedulerError::DisconnectedSendChannel("consume work sender"))?;
    Ok(num_sent)
}
```

`std::mem::take` ile batch'i taşır (klonlamaz). `track_batch` ile in-flight kaydeder.

**fn PrioGraphScheduler::receive_completed (satır 256-272):** Non-blocking loop ile tüm tamamlanmış batch'leri al.

**fn PrioGraphScheduler::complete_batch (satır ~305):**

```rust
fn complete_batch(&mut self, batch_id: TransactionBatchId, transactions: &[SanitizedTransaction]) {
    let thread_id = self.in_flight_tracker.complete_batch(batch_id);
    let transaction_locks = // her tx'in account lock'ları
    for (write_locks, read_locks) in transaction_locks {
        self.account_locks.unlock_accounts(write_locks, read_locks, thread_id);
    }
}
```

**fn get_transaction_account_access:** Tx'in tüm account access'lerini `(pubkey, AccessKind)` çiftleri olarak döndürür. `prio_graph` kütüphanesinin bağımlılık analizi için.

**fn select_thread:**
```rust
fn select_thread(thread_set: ThreadSet, batches_txs: &[Vec<_>], in_flight: &[usize]) -> ThreadId {
    thread_set.contained_threads_iter()
        .map(|t| (t, batches_txs[t].len() + in_flight[t]))
        .min_by_key(|(_, load)| *load)
        .map(|(t, _)| t)
        .unwrap()
}
```

Load balancing: en az yüklü thread'i seç.

### 19.10 scheduler_controller.rs (kapsamlı)

**Struct: SchedulerController (satır 44-65)**

```rust
pub(crate) struct SchedulerController {
    decision_maker: DecisionMaker,
    packet_receiver: PacketDeserializer,
    bank_forks: Arc<RwLock<BankForks>>,
    transaction_id_generator: TransactionIdGenerator,
    container: TransactionStateContainer,
    scheduler: PrioGraphScheduler,
    count_metrics: SchedulerCountMetrics,
    timing_metrics: SchedulerTimingMetrics,
    worker_metrics: Vec<Arc<ConsumeWorkerMetrics>>,
}
```

**fn SchedulerController::run (satır 88-134):**

Ana kontrol döngüsü:
```
loop:
    decision = decision_maker.make_consume_or_forward_decision()
    process_transactions(&decision)
    receive_completed()
    if !receive_and_buffer_packets(&decision): break
    metrics raporla
```

**fn process_transactions (satır 137-189):**
- `Consume` → `scheduler.schedule()` çağrısı
- `Forward` → `clear_container()` (tx'leri at)
- `ForwardAndHold` → `clean_queue()` (geçersizleri temizle)
- `Hold` → (hiçbir şey)

**fn pre_graph_filter (satır 192-214):**
```rust
fn pre_graph_filter(transactions: &[&SanitizedTransaction], results: &mut [bool], bank: &Bank) {
    // 1. bank.check_transactions() — blockhash, nonce, duplikasyon
    // 2. Consumer::check_fee_payer_unlocked() — ücret ödeyebilir mi?
    // results[i] = fee_check_result.is_ok()
}
```

**fn clean_queue (satır 233-282):** `MAX_TRANSACTION_CHECKS = 10_000` limit. 128'lik chunk'larda `bank.check_transactions()`. Geçersizleri container'dan sil.

**fn receive_and_buffer_packets (satır 300+):**

Karar bazlı paket kabul:
- `Forward`: Sadece oy tx'lerini kabul et (normal tx'ler zaten atılacak)
- Diğer: Tüm paketleri kabul et

Her paketi sanitize et → priority ve cost hesapla → `TransactionStateContainer::insert_new_transaction()`.

**Sanitizasyon pipeline (receive_and_buffer_packets içinde):**
```rust
let tx = packet.build_sanitized_transaction(&bank.feature_set, bank.vote_only_bank(), &bank)?;
let priority = calculate_priority(&tx, bank);
let cost = CostModel::calculate_cost(&tx, &bank.feature_set).sum();
let ttl = SanitizedTransactionTTL { transaction: tx, max_age_slot: bank.slot() + MAX_PROCESSING_AGE };
container.insert_new_transaction(id, ttl, priority, cost)
```

**fn calculate_priority:** `compute_budget_details` varsa `compute_unit_price / compute_unit_limit`. Yoksa 0.

### 19.11 scheduler_metrics.rs

**SchedulerCountMetrics** ve **SchedulerTimingMetrics** — hem interval-bazlı hem de slot-bazlı raporlama. Her ikisi de `maybe_report_and_reset_interval()` ve `maybe_report_and_reset_slot()` metodlarına sahip.

**Sayaçlar (SchedulerCountMetrics):**
- `num_received`, `num_buffered`
- `num_scheduled`, `num_unschedulable`, `num_schedule_filtered_out`
- `num_finished`, `num_retryable`
- `num_dropped_on_capacity`, `num_dropped_on_age_and_status`, `num_dropped_on_clear`
- `min_priority`, `max_priority`

---

## 20. unprocessed_packet_batches.rs (kapsamlı)

**Konum:** `core/src/banking_stage/unprocessed_packet_batches.rs`

### Struct: DeserializedPacket (satır 16-19)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializedPacket {
    immutable_section: Arc<ImmutableDeserializedPacket>,
    pub forwarded: bool,
}
```

`ImmutableDeserializedPacket` üzerinde mutable wrapper. `forwarded` flag değiştirilebilir.

`Ord` impl: `compute_unit_price()` üzerinden (priority queue için).

### Struct: UnprocessedPacketBatches (satır 67-71)

```rust
#[derive(Debug, Default)]
pub struct UnprocessedPacketBatches {
    pub packet_priority_queue: MinMaxHeap<Arc<ImmutableDeserializedPacket>>,
    pub message_hash_to_transaction: HashMap<Hash, DeserializedPacket>,
    batch_limit: usize,
}
```

İki veri yapısı senkronize:
- `MinMaxHeap`: O(1) min/max, O(log n) push/pop — öncelik sıralaması
- `HashMap<message_hash, DeserializedPacket>`: O(1) lookup — deduplication ve forwarded takibi

### fn UnprocessedPacketBatches::push (satır 130-145)

```rust
pub fn push(&mut self, packet: DeserializedPacket) -> Option<DeserializedPacket> {
    if self.message_hash_to_transaction.contains_key(packet.immutable_section().message_hash()) {
        return None;  // duplicate
    }
    if self.len() == self.batch_limit {
        Some(self.push_pop_min(packet))  // kapasite dolunca en düşük öncelikliyi at
    } else {
        self.push_internal(packet);
        None
    }
}
```

**push_pop_min:** `MinMaxHeap::push_pop_min()` — heap invariant'ını bozmudan atomik push+pop. Yeni paket daha düşük öncelikli ise hemen atılır (None dönen degenerate case yok — her zaman bir şey atılır).

### fn UnprocessedPacketBatches::retain (satır 155-186)

```rust
pub fn retain<F>(&mut self, mut f: F) where F: FnMut(&mut DeserializedPacket) -> bool {
    let new_queue = self.packet_priority_queue.drain()
        .filter(|immutable_packet| {
            match self.message_hash_to_transaction.entry(*immutable_packet.message_hash()) {
                Entry::Vacant(_) => panic!("inconsistency"),
                Entry::Occupied(mut e) => {
                    let should_retain = f(e.get_mut());
                    if !should_retain { e.remove_entry(); }
                    should_retain
                }
            }
        })
        .collect();
    self.packet_priority_queue = new_queue;
}
```

Her iki veri yapısını senkronize tutar. HashMap üzerinden `retain` yapılamadığı için priority queue drain + collect pattern.

### fn get_min_compute_unit_price / get_max_compute_unit_price

```rust
pub fn get_min_compute_unit_price(&self) -> Option<u64> {
    self.packet_priority_queue.peek_min().map(|x| x.compute_unit_price())
}
```

O(1) min/max price erişimi.

---

## 21. unprocessed_transaction_storage.rs (kapsamlı)

**Konum:** `core/src/banking_stage/unprocessed_transaction_storage.rs`

### Enum: ThreadType (satır ~15)

```rust
pub enum ThreadType {
    Transactions,   // normal tx thread'i
    Voting(VoteSource),  // oy thread'i
}
```

### Enum: UnprocessedTransactionStorage (satır ~25)

```rust
pub enum UnprocessedTransactionStorage {
    VoteStorage(LatestUnprocessedVotes, VoteSource),
    LocalTransactionStorage(UnprocessedPacketBatches),
}
```

**Polimorfizm:** Oy thread'leri ve tx thread'leri farklı storage kullanır. Oy thread'leri `LatestUnprocessedVotes`, tx thread'leri `UnprocessedPacketBatches`.

### fn UnprocessedTransactionStorage::len (satır ~50)

Her varyant için farklı implementasyon. Oy storage'ı `LatestUnprocessedVotes::len()` kullanır (pahalı — tüm lock'ları döner).

### fn UnprocessedTransactionStorage::process_packets

Callback-based iteration. Her batch için closure çağrısı:
```rust
fn process_packets<F: FnMut(&[Arc<ImmutableDeserializedPacket>], &mut Payload) -> Option<Vec<usize>>>(
    &mut self,
    bank: Arc<Bank>,
    banking_stage_stats: &BankingStageStats,
    slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
    f: F,
) -> bool
```

`MultiIteratorScanner` kullanarak non-conflicting batch'ler oluşturur.

### fn UnprocessedTransactionStorage::filter_forwardable_packets_and_add_batches

Her tx'i root bank'a göre sanitize et, `ForwardPacketBatchesByAccounts`'a ekle.

### Struct: ConsumeScannerPayload

```rust
pub struct ConsumeScannerPayload<'a> {
    pub reached_end_of_slot: bool,
    pub sanitized_transactions: Vec<SanitizedTransaction>,
    pub account_locks: ReadWriteAccountSet,
    pub slot_metrics_tracker: &'a mut LeaderSlotMetricsTracker,
}
```

`MultiIteratorScanner`'ın `payload`'ı olarak kullanılır. Scanner ile consumer arasında shared mutable state.

---

## Nova Shield Chain İçin Dersler

### 1. Transaction Pipeline Tasarımı

**Ders:** Solana'nın iki katmanlı pipeline'ı (deserialize → priority queue → schedule → execute → commit) net sorumluluk ayrımı sağlar. Nova Shield Chain'in transaction pipeline'ı bu katmanları benimlemeli:

```
Receive → Deserialize → Prioritize → Schedule → Execute → Commit
```

Her katman arasında channel kullanmak, backpressure'ı doğal olarak yönetir.

### 2. Priority Queue + Deduplication

**Ders:** `UnprocessedPacketBatches`'ın `MinMaxHeap + HashMap` çift veri yapısı mükemmel:
- `MinMaxHeap`: O(1) min-max, O(log n) push/pop
- `HashMap<message_hash>`: O(1) deduplication

Nova Shield Chain'de benzer bir yapı şart: aynı tx'in iki kez işlenmesini önlemek için message hash deduplication + öncelik sırası.

### 3. Multi-Iterator Scanner (Conflict-Free Batching)

**Ders:** `MultiIteratorScanner` bağımsız olmayan tx'lerin batch'lere nasıl dağıtılacağını elegantly çözer. Nova Shield Chain'de hesap çakışmalarını önlemek için benzer bir mekanizma gerekli (özellikle MPC protokollü paylaşımlı hesaplar için).

### 4. Thread-Aware Account Locks

**Ders:** `ThreadAwareAccountLocks + ThreadSet` bitmask yaklaşımı son derece verimli. 64 thread'e kadar `u64` bitmask ile O(1) thread seçimi. Nova Shield Chain'de çok-thread execution için bu pattern adaptable.

### 5. Prio-Graph Scheduling

**Ders:** `prio_graph` kütüphanesi DAG tabanlı bağımlılık analizi yapar. Çakışan tx'lerin doğru sırayla (öncelik korunarak) işlenmesini garantiler. Nova Shield Chain'in ZK proof generation'ı gibi uzun süreli işlemlerde bu yaklaşım yararlı.

### 6. Rate Limiting (DataBudget)

**Ders:** `DataBudget` token bucket algoritması — 12MB/s çıkış sınırı, 100ms periyotlarda doldurulan. Nova Shield Chain'de P2P forwarding'de benzer bir rate limiter gerekli.

### 7. Adaptif Timeout

**Ders:** `PacketReceiver::get_receive_timeout()` — buffer doluysa 0ms, boşsa 100ms. Bu simple heuristic, işlem başarısız olduğunda (cost model throttle) yeni paket bekleme yerine mevcut buffer'ı işlemeye öncelik verir.

### 8. Atomik Metrikler

**Ders:** `ConsumeWorkerMetrics`'in tüm alanları atomik. Metrics thread başka bir thread'den raporlayabilir. Nova Shield Chain'de her modülün kendi atomik metrik struct'ı olmalı.

### 9. Karar Fonksiyonu (DecisionMaker)

**Ders:** `BufferedPacketsDecision` enum'u + `consume_or_forward_packets` closure-tabanlı implementasyon, hem test edilebilirlik hem de mantık netliği açısından ideal. Nova Shield Chain'in consensus akışında benzer state machine faydalı:
- `IsLeader → Consume`
- `WillBeLeaderSoon → Hold`
- `OtherLeader → Forward`

### 10. Oy Tx'lerinin Özel İşlenmesi

**Ders:** Oy tx'leri:
- Ayrı queue (`LatestUnprocessedVotes`)
- Her validator için sadece en son oy tutulur
- Stake-weighted random sıralama
- Compute unit price = 0 (fee sıralamasını atlar)
- UDP ile yönlendirilir (latency kritik)

Nova Shield Chain'de eğer validator oylaması varsa, benzer bir ayrım yapılmalı: konsensüs mesajları normal tx'lerden ayrı tutulmalı.

### 11. Commit Güvenliği

**Ders:** `execute_and_commit_transactions_locked` içindeki sıra kritik:
1. `freeze_lock` al
2. `last_blockhash` al (freeze altında!)
3. `record_transactions` (PoH'a kaydet)
4. `commit_transactions`
5. `freeze_lock` bırak

Bu sıra, blockhash güncellemesi ile commit arasında race condition'ı önler. Nova Shield Chain'de benzer bir "critical section ordering" garantisi gerekli.

### 12. Kapasite Yönetimi

**Ders:** `TOTAL_BUFFERED_PACKETS = 500_000` sabit sınır. Dolunca en düşük öncelikli tx atılır (`push_pop_min`). Nova Shield Chain'de bellek sınırı kritik — sınırsız büyüme OOM'a yol açar.

### 13. Cost Model Entegrasyonu

**Ders:** `QosService` her tx'in tahmini maliyetini hesaplar ve `CostTracker`'a ekler. Commit sonrası gerçek CU ile günceller (feature flag kontrolüyle). Bu, blok kapasitesini doğru kullanmayı sağlar. Nova Shield Chain'de gas model benzer bir entegrasyon gerektirir.

### 14. Error Recovery (Retry Mechanism)

**Ders:** `retryable_transaction_indexes` — geçici hatalar (AccountInUse, WouldExceedMaxBlockCostLimit) için tx'i kuyruğa geri koy. Kalıcı hatalar (TooOld, DuplicateSignature) için at. Nova Shield Chain'de aynı ayrım MPC hataları için de geçerli (timeout vs. definitively invalid).

### 15. Wrapping ID Generators

**Ders:** `BatchIdGenerator` — `wrapping_sub(1)` ile `u64::MAX`'tan geriye doğru gider. Bu, 0'dan başlayan `TransactionIdGenerator` ile çakışmayı önler. Unique ID generation'da wrapping overflow güvenliği.

---

## Dosya Özeti Tablosu

| Dosya | Satır | Özet |
|-------|-------|------|
| banking_stage.rs | 1457 | Ana modül, iki pipeline modu, BankingStageStats |
| committer.rs | 179 | Tx commit, bakiye toplama, PoH-sonrası status yayını |
| consumer.rs | ~826 | Ana execution pipeline, QoS entegrasyonu |
| consume_worker.rs | 966 | Scheduler-Worker iletişimi, atomik metrikler |
| decision_maker.rs | 294 | Consume/Forward/Hold/ForwardAndHold kararı |
| forwarder.rs | 496 | Rate-limited paket yönlendirme, QUIC/UDP seçimi |
| forward_packet_batches_by_accounts.rs | 336 | Hot account batching, cost-tracked batches |
| forward_worker.rs | 228 | Forward pipeline worker |
| immutable_deserialized_packet.rs | 169 | Immutable paket sarmalayıcı, compute priority |
| latest_unprocessed_votes.rs | 872 | Stake-weighted oy yönetimi |
| leader_slot_metrics.rs | - | Per-slot metrics tracking |
| leader_slot_timing_metrics.rs | - | Timing metrics |
| multi_iterator_scanner.rs | 397 | Conflict-free batch oluşturucu |
| packet_deserializer.rs | 232 | SigVerify → ImmutableDeserializedPacket |
| packet_receiver.rs | 180 | Buffer yönetimi, adaptif timeout |
| qos_service.rs | - | Blok kapasitesi yönetimi |
| read_write_account_set.rs | 283 | Account lock tracking |
| scheduler_messages.rs | 67 | Scheduler ↔ Worker mesaj tipleri |
| transaction_scheduler/batch_id_generator.rs | 14 | wrapping_sub ID üretimi |
| transaction_scheduler/in_flight_tracker.rs | 123 | Per-thread in-flight tx/CU takibi |
| transaction_scheduler/prio_graph_scheduler.rs | - | PrioGraph + ThreadAwareLocks scheduling |
| transaction_scheduler/scheduler_controller.rs | - | Tüm yeni scheduler akışı |
| transaction_scheduler/scheduler_error.rs | 9 | Channel disconnect hataları |
| transaction_scheduler/scheduler_metrics.rs | - | Interval + slot bazlı metrikler |
| transaction_scheduler/thread_aware_account_locks.rs | - | Bitmask ThreadSet, R/W lock tracking |
| transaction_scheduler/transaction_id_generator.rs | - | Monoton ID üretimi |
| transaction_scheduler/transaction_priority_id.rs | 43 | Priority+ID pair, Ord+Hash |
| transaction_scheduler/transaction_state.rs | 282 | Unprocessed↔Pending state machine |
| transaction_scheduler/transaction_state_container.rs | 252 | MinMaxHeap+HashMap container |
| unprocessed_packet_batches.rs | - | MinMaxHeap priority queue + dedup |
| unprocessed_transaction_storage.rs | - | Tx/Vote thread polymorphic storage |
