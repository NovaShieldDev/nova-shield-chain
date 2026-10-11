# Solana ReplayStage Analizi — Agent 13

**Analiz Tarihi:** 2026-07-22
**Kapsam:** core/src/replay_stage.rs (8754 satır)

---

## 1. DOSYAYA GENEL BAKIS

`replay_stage.rs`, Solana'nın konsensüs motorunun kalbi sayılan dosyadır. 8754 satır ile projenin en büyük tek dosyasıdır. Bu dosya şunlardan sorumludur:

- Blockstore'dan blokları okuyup tekrar oynatmak (replay)
- Fork ağacını takip etmek ve en ağır fork'u seçmek
- Tower BFT ile vote gönderme kararları vermek
- Duplicate (çift) blok tespiti ve kurtarma
- Leader slot başlatma
- Yeni bank fork'ları oluşturma
- Bölünme (partition) tespiti
- Propagasyon eşiği kontrolü

---

## 2. SABITLER VE TUNING PARAMETRELERI

```rust
// Satır 93-103
pub const MAX_ENTRY_RECV_PER_ITER: usize = 512;
pub const SUPERMINORITY_THRESHOLD: f64 = 1f64 / 3f64;      // %33.3 - superminority eşiği
pub const MAX_UNCONFIRMED_SLOTS: usize = 5;
pub const DUPLICATE_LIVENESS_THRESHOLD: f64 = 0.1;          // %10 - canlılık eşiği
pub const DUPLICATE_THRESHOLD: f64 = 1.0 - SWITCH_FORK_THRESHOLD - DUPLICATE_LIVENESS_THRESHOLD;
const MAX_VOTE_SIGNATURES: usize = 200;                      // Kaydedilen max vote imzası
const MAX_VOTE_REFRESH_INTERVAL_MILLIS: usize = 5000;        // 5 saniye vote yenileme aralığı
const MAX_CONCURRENT_FORKS_TO_REPLAY: usize = 4;            // Paralel replay thread sayısı
const MAX_REPAIR_RETRY_LOOP_ATTEMPTS: usize = 10;            // Max onarım denemesi
```

**Kritik tasarım notu:** `DUPLICATE_THRESHOLD + SWITCH_FORK_THRESHOLD + DUPLICATE_LIVENESS_THRESHOLD = 1.0` olacak şekilde dengelenmiştir. Bu, duplicate slot protokolünün liveness garantisi sağlamasını mümkün kılar.

---

## 3. TÜM STRUCT TANIMLAMALARI

### 3.1 `HeaviestForkFailures` — (satır 105-124)
```rust
#[derive(PartialEq, Eq, Debug)]
pub enum HeaviestForkFailures {
    LockedOut(u64),                                         // Slot kilitlenmiş
    FailedThreshold(Slot, /* vote depth */ u64, /* Observed stake */ u64, /* Total stake */ u64),
    FailedSwitchThreshold(Slot, /* Observed stake */ u64, /* Total stake */ u64),
    NoPropagatedConfirmation(Slot, /* Observed stake */ u64, /* Total stake */ u64),
}
```
En ağır fork'a oy verilememesinin sebeplerini temsil eder. Her varyant izleme için stake bilgisi taşır.

---

### 3.2 `ForkReplayMode` — (satır 126-129)
```rust
enum ForkReplayMode {
    Serial,
    Parallel(ThreadPool),
}
```
Fork'ların tekrar oynatılma modunu belirler. `Parallel` modu Rayon thread pool içerir. Sadece birden fazla aktif fork varsa ve `replay_slots_concurrently` true ise parallel mod devreye girer.

---

### 3.3 `ConfirmationType` — (satır 131-135)
```rust
#[derive(PartialEq, Eq, Debug)]
enum ConfirmationType {
    SupermajorityVoted,    // %2/3+ stake oyu
    DuplicateConfirmed,    // Gossip üzerinden duplicate onayı
}
```

---

### 3.4 `GenerateVoteTxResult` — (satır 137-149)
```rust
enum GenerateVoteTxResult {
    NonVoting,      // Oy kullanmayan validator, yenileme için uygun değil
    Failed,         // Üretim başarısız, yenileme için uygun
    Tx(Transaction), // Başarılı oy transaction'ı
}
```
`is_non_voting()` metodu: `matches!(self, Self::NonVoting)` döner.

---

### 3.5 `ConfirmedSlot` — (satır 151-174)
```rust
#[derive(PartialEq, Eq, Debug)]
struct ConfirmedSlot {
    slot: Slot,
    frozen_hash: Hash,
    confirmation_type: ConfirmationType,
}
```
Constructor'lar:
- `new_supermajority_voted(slot, frozen_hash)` — SupermajorityVoted tipinde oluşturur
- `new_duplicate_confirmed_slot(slot, frozen_hash)` — DuplicateConfirmed tipinde oluşturur

---

### 3.6 `Finalizer` — (satır 178-193)
```rust
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}
```
Drop semantiği ile çalışır. ReplayStage thread'i paniklese bile exit flag'ini set eder. Bu pattern, tüm sistemi düzenli kapatmak için kullanılır.

---

### 3.7 `ReplaySlotFromBlockstore` — (satır 195-200)
```rust
struct ReplaySlotFromBlockstore {
    is_slot_dead: bool,
    bank_slot: Slot,
    replay_result: Option<Result<usize /* tx count */, BlockstoreProcessorError>>,
}
```
Bir slot'un blockstore'dan replay sonucunu taşır.

---

### 3.8 `LastVoteRefreshTime` — (satır 201-204)
```rust
struct LastVoteRefreshTime {
    last_refresh_time: Instant,
    last_print_time: Instant,
}
```
Vote yenileme zamanlamasını takip eder. `MAX_VOTE_REFRESH_INTERVAL_MILLIS` süre geçmeden yenileme yapmaz.

---

### 3.9 `SkippedSlotsInfo` — (satır 206-210)
```rust
#[derive(Default)]
struct SkippedSlotsInfo {
    last_retransmit_slot: u64,
    last_skipped_slot: u64,
}
```
Leader slot atlamalarını ve retransmit durumunu takip eder. Aynı slot için gereksiz log'lamayı önler.

---

### 3.10 `PartitionInfo` — (satır 212-273)
```rust
struct PartitionInfo {
    partition_start_time: Option<Instant>,
}
```
Ağ bölünmesinin başlangıç zamanını takip eder.

**`update()` metodu (satır 223-272):**
- `partition_detected == true` ve önceden bölünme yoksa: PARTITION DETECTED uyarısı loglar ve datapoint gönderir
- `partition_detected == false` ve önceden bölünme vardıysa: PARTITION resolved uyarısı loglar, süreyi raporlar

---

### 3.11 `ReplayStageConfig` — (satır 275-295)
```rust
pub struct ReplayStageConfig {
    pub vote_account: Pubkey,
    pub authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
    pub exit: Arc<AtomicBool>,
    pub rpc_subscriptions: Arc<RpcSubscriptions>,
    pub leader_schedule_cache: Arc<LeaderScheduleCache>,
    pub accounts_background_request_sender: AbsRequestSender,
    pub block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    pub transaction_status_sender: Option<TransactionStatusSender>,
    pub rewards_recorder_sender: Option<RewardsRecorderSender>,
    pub cache_block_meta_sender: Option<CacheBlockMetaSender>,
    pub entry_notification_sender: Option<EntryNotifierSender>,
    pub bank_notification_sender: Option<BankNotificationSenderConfig>,
    pub wait_for_vote_to_start_leader: bool,
    pub ancestor_hashes_replay_update_sender: AncestorHashesReplayUpdateSender,
    pub tower_storage: Arc<dyn TowerStorage>,
    pub wait_to_vote_slot: Option<Slot>,      // Slashing'den kaçınmak için
    pub replay_slots_concurrently: bool,
}
```

---

### 3.12 `ReplayLoopTiming` — (satır 298-330)
```rust
#[derive(Default)]
struct ReplayLoopTiming {
    last_submit: u64,
    loop_count: u64,
    collect_frozen_banks_elapsed_us: u64,
    compute_bank_stats_elapsed_us: u64,
    select_vote_and_reset_forks_elapsed_us: u64,
    start_leader_elapsed_us: u64,
    reset_bank_elapsed_us: u64,
    voting_elapsed_us: u64,
    generate_vote_us: u64,
    update_commitment_cache_us: u64,
    select_forks_elapsed_us: u64,
    compute_slot_stats_elapsed_us: u64,
    generate_new_bank_forks_elapsed_us: u64,
    replay_active_banks_elapsed_us: u64,
    wait_receive_elapsed_us: u64,
    heaviest_fork_failures_elapsed_us: u64,
    bank_count: u64,
    process_ancestor_hashes_duplicate_slots_elapsed_us: u64,
    process_duplicate_confirmed_slots_elapsed_us: u64,
    process_duplicate_slots_elapsed_us: u64,
    process_unfrozen_gossip_verified_vote_hashes_elapsed_us: u64,
    process_popular_pruned_forks_elapsed_us: u64,
    repair_correct_slots_elapsed_us: u64,
    retransmit_not_propagated_elapsed_us: u64,
    generate_new_bank_forks_read_lock_us: u64,
    generate_new_bank_forks_get_slots_since_us: u64,
    generate_new_bank_forks_loop_us: u64,
    generate_new_bank_forks_write_lock_us: u64,
    replay_blockstore_us: u64,
}
```
Ana döngünün her aşamasının microsaniye cinsinden süresini toplar. `maybe_submit()` saniyede bir Prometheus-tarzı datapoint gönderir.

---

### 3.13 `ReplayStage` — (satır 526-529)
```rust
pub struct ReplayStage {
    t_replay: JoinHandle<()>,
    commitment_service: AggregateCommitmentService,
}
```
Dışa açık tek struct. Tek thread handle + commitment service içerir.

---

### 3.14 Test Yardımcı Struct — `ReplayBlockstoreComponents` — (satır 4409-4419)
```rust
pub struct ReplayBlockstoreComponents {
    pub blockstore: Arc<Blockstore>,
    validator_node_to_vote_keys: HashMap<Pubkey, Pubkey>,
    pub(crate) my_pubkey: Pubkey,
    cluster_info: ClusterInfo,
    pub(crate) leader_schedule_cache: Arc<LeaderScheduleCache>,
    poh_recorder: RwLock<PohRecorder>,
    tower: Tower,
    rpc_subscriptions: Arc<RpcSubscriptions>,
    pub vote_simulator: VoteSimulator,
}
```

---

## 4. TÜM FONKSIYON TANIMLAMALARI

### 4.1 `ReplayStage::new()` — (satır 533-1210)
```rust
pub fn new(
    config: ReplayStageConfig,
    blockstore: Arc<Blockstore>,
    bank_forks: Arc<RwLock<BankForks>>,
    cluster_info: Arc<ClusterInfo>,
    ledger_signal_receiver: Receiver<bool>,
    duplicate_slots_receiver: DuplicateSlotReceiver,
    poh_recorder: Arc<RwLock<PohRecorder>>,
    mut tower: Tower,
    vote_tracker: Arc<VoteTracker>,
    cluster_slots: Arc<ClusterSlots>,
    retransmit_slots_sender: Sender<Slot>,
    ancestor_duplicate_slots_receiver: AncestorDuplicateSlotsReceiver,
    replay_vote_sender: ReplayVoteSender,
    duplicate_confirmed_slots_receiver: DuplicateConfirmedSlotsReceiver,
    gossip_verified_vote_hash_receiver: GossipVerifiedVoteHashReceiver,
    cluster_slots_update_sender: ClusterSlotsUpdateSender,
    cost_update_sender: Sender<CostUpdate>,
    voting_sender: Sender<VoteOp>,
    drop_bank_sender: Sender<Vec<Arc<Bank>>>,
    block_metadata_notifier: Option<BlockMetadataNotifierArc>,
    log_messages_bytes_limit: Option<usize>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
    dumped_slots_sender: DumpedSlotsSender,
    banking_tracer: Arc<BankingTracer>,
    popular_pruned_forks_receiver: PopularPrunedForksReceiver,
) -> Result<Self, String>
```
Ana kurucu. `AggregateCommitmentService`'i başlatır ve `"solReplayStage"` thread'ini spawn eder. Ana döngü bu thread içinde çalışır.

---

### 4.2 `ReplayStage::load_tower()` — (satır 1215-1246)
```rust
fn load_tower(
    tower_storage: &dyn TowerStorage,
    node_pubkey: &Pubkey,
    vote_account: &Pubkey,
    bank_forks: &Arc<RwLock<BankForks>>,
) -> Result<Tower, TowerError>
```
Tower'ı diskten yükler. Dosya yoksa veya çok eski ise `Tower::new_from_bankforks()` ile yeni tower oluşturur. Sadece ciddi hatalar (korrupted data vb.) `Err` döner.

---

### 4.3 `ReplayStage::check_for_vote_only_mode()` — (satır 1248-1281)
```rust
fn check_for_vote_only_mode(
    heaviest_bank_slot: Slot,
    forks_root: Slot,
    in_vote_only_mode: &AtomicBool,
    bank_forks: &RwLock<BankForks>,
)
```
Root'tan uzaklık `MAX_ROOT_DISTANCE_FOR_VOTE_ONLY`'yi aşarsa vote-only moduna girer. CAS (compare-and-swap) ile thread-safe geçiş yapar. Yalnızca vote tx'lerini kabul eden bir mod; normal transaction'lar reddedilir.

---

### 4.4 `ReplayStage::maybe_retransmit_unpropagated_slots()` — (satır 1283-1321)
```rust
fn maybe_retransmit_unpropagated_slots(
    metric_name: &'static str,
    retransmit_slots_sender: &Sender<Slot>,
    progress: &mut ProgressMap,
    latest_leader_slot: Slot,
)
```
Propagasyonu tamamlanmamış leader slot'larını yeniden iletir. Retransmit threshold kontrolü yaparak exponential backoff uygular.

---

### 4.5 `ReplayStage::retransmit_latest_unpropagated_leader_slot()` — (satır 1323-1344)
```rust
fn retransmit_latest_unpropagated_leader_slot(
    poh_recorder: &Arc<RwLock<PohRecorder>>,
    retransmit_slots_sender: &Sender<Slot>,
    progress: &mut ProgressMap,
)
```
PoH recorder'ın start slot'undan başlayarak propagasyonu tamamlanmamış leader slot'ları için `maybe_retransmit_unpropagated_slots()` çağırır.

---

### 4.6 `ReplayStage::is_partition_detected()` — (satır 1346-1356)
```rust
fn is_partition_detected(
    ancestors: &HashMap<Slot, HashSet<Slot>>,
    last_voted_slot: Slot,
    heaviest_slot: Slot,
) -> bool
```
`last_voted_slot != heaviest_slot` VE `last_voted_slot`, `heaviest_slot`'un atası değilse partition tespit eder. True döndüğünde ağ bölünmüş demektir.

---

### 4.7 `ReplayStage::initialize_progress_and_fork_choice_with_locked_bank_forks()` — (satır 1358-1389)
```rust
fn initialize_progress_and_fork_choice_with_locked_bank_forks(
    bank_forks: &RwLock<BankForks>,
    my_pubkey: &Pubkey,
    vote_account: &Pubkey,
    blockstore: &Blockstore,
) -> (ProgressMap, HeaviestSubtreeForkChoice)
```
Başlangıçta bank forks'u lock alarak progress map ve fork choice'ı initialize eder. Blockstore'daki duplicate slot'ları da hesaba katar.

---

### 4.8 `ReplayStage::initialize_progress_and_fork_choice()` — (satır 1391-1421)
```rust
pub fn initialize_progress_and_fork_choice(
    root_bank: &Bank,
    mut frozen_banks: Vec<Arc<Bank>>,
    my_pubkey: &Pubkey,
    vote_account: &Pubkey,
    duplicate_slot_hashes: Vec<(Slot, Hash)>,
) -> (ProgressMap, HeaviestSubtreeForkChoice)
```
Frozen bank'ları slot'a göre sıralayıp her biri için `ForkProgress::new_from_bank()` ile progress map'e ekler. `HeaviestSubtreeForkChoice::new_from_frozen_banks()` ile fork choice ağacını kurar.

---

### 4.9 `ReplayStage::dump_then_repair_correct_slots()` — (satır 1423-1560)
```rust
pub fn dump_then_repair_correct_slots(
    duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
    ancestors: &mut HashMap<Slot, HashSet<Slot>>,
    descendants: &mut HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    bank_forks: &RwLock<BankForks>,
    blockstore: &Blockstore,
    poh_bank_slot: Option<Slot>,
    purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
    dumped_slots_sender: &DumpedSlotsSender,
    my_pubkey: &Pubkey,
    leader_schedule_cache: &LeaderScheduleCache,
)
```
Duplicate olarak işaretlenmiş ve cluster tarafından onaylanmış doğru versiyonun farklı olduğu slot'ları temizler.

**Kritik güvenlik kontrolleri:**
1. PoH bank'ı duplicate fork üzerinde inşa ediyorsa slot'u temizleme (race condition kaçınma)
2. Eğer biz o slot'un leader'ıysak **panic!** — Bu runtime bug'ını gösterir
3. `MAX_REPAIR_RETRY_LOOP_ATTEMPTS` aşılırsa panic ile çıkar

**Adımlar:** `purge_unconfirmed_duplicate_slot()` çağırır → `dumped_slots_sender`'a bildirir → repair service devreye girer.

---

### 4.10 `ReplayStage::process_ancestor_hashes_duplicate_slots()` — (satır 1562-1615)
```rust
fn process_ancestor_hashes_duplicate_slots(
    pubkey: &Pubkey,
    blockstore: &Blockstore,
    ancestor_duplicate_slots_receiver: &AncestorDuplicateSlotsReceiver,
    ...
)
```
`ancestor_hashes_service`'ten gelen duplicate slot bildirimlerini işler. Her bildirim için `EpochSlotsFrozenState` oluşturup `check_slot_agrees_with_cluster()` çağırır.

---

### 4.11 `ReplayStage::purge_unconfirmed_duplicate_slot()` — (satır 1617-1722)
```rust
fn purge_unconfirmed_duplicate_slot(
    duplicate_slot: Slot,
    ancestors: &mut HashMap<Slot, HashSet<Slot>>,
    descendants: &mut HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    root_bank: &Bank,
    bank_forks: &RwLock<BankForks>,
    blockstore: &Blockstore,
)
```
Bir slot ve tüm torunlarını temizler:
1. Ancestors/descendants map'lerini günceller
2. BankForks'tan bankaları kaldırır
3. `root_bank.remove_unrooted_slots()` ile account'ları temizler
4. Status cache'i temizler
5. Program cache'den deployment'ları kaldırır
6. Blockstore'u temizler
7. Progress map'ten kaldırır

**Tasarım inceliği:** Eğer descendant dead ise tam purge yapmaz, sadece dead flag'i kaldırır. Bu sayede parent onarıldığında child tekrar replay edilebilir.

---

### 4.12 `ReplayStage::purge_ancestors_descendants()` — (satır 1724-1762)
```rust
fn purge_ancestors_descendants(
    slot: Slot,
    slot_descendants: &HashSet<Slot>,
    ancestors: &mut HashMap<Slot, HashSet<Slot>>,
    descendants: &mut HashMap<Slot, HashSet<Slot>>,
)
```
Ancestors ve descendants map'lerini tutarlı halde tutar. Purge edilen slot'u her atanın descendant listesinden çıkarır.

---

### 4.13 `ReplayStage::process_popular_pruned_forks()` — (satır 1764-1796)
```rust
fn process_popular_pruned_forks(
    popular_pruned_forks_receiver: &PopularPrunedForksReceiver,
    ...
)
```
`repair_service`'ten gelen ve %52+ stake ile desteklenen ama pruned olan fork'ları işler. Her slot için `check_slot_agrees_with_cluster()` çağırır.

---

### 4.14 `ReplayStage::process_duplicate_confirmed_slots()` — (satır 1802-1848)
```rust
fn process_duplicate_confirmed_slots(
    duplicate_confirmed_slots_receiver: &DuplicateConfirmedSlotsReceiver,
    ...
)
```
Gossip veya replay oyları üzerinden duplicate olarak onaylanan slot'ları işler. Her slot için `DuplicateConfirmedState` oluşturup `check_slot_agrees_with_cluster()` çağırır. Root'tan küçük veya zaten işlenmiş slot'ları atlar.

---

### 4.15 `ReplayStage::process_gossip_verified_vote_hashes()` — (satır 1850-1867)
```rust
fn process_gossip_verified_vote_hashes(
    gossip_verified_vote_hash_receiver: &GossipVerifiedVoteHashReceiver,
    unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
    heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice,
    latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
)
```
Gossip üzerinden gelen verify edilmiş vote hash'lerini işler. Bunlar henüz blok içine girmemiş oylar olabilir — fork choice ve switch threshold için kritik.

---

### 4.16 `ReplayStage::process_duplicate_slots()` — (satır 1870-1918)
```rust
fn process_duplicate_slots(
    blockstore: &Blockstore,
    duplicate_slots_receiver: &DuplicateSlotReceiver,
    ...
)
```
`WindowService`'ten gelen duplicate slot sinyallerini işler. Sadece `tpu_has_bank == false` iken çağrılır. Her duplicate slot için `DuplicateState::new_from_state()` oluşturur.

---

### 4.17 `ReplayStage::log_leader_change()` — (satır 1920-1942)
```rust
fn log_leader_change(
    my_pubkey: &Pubkey,
    bank_slot: Slot,
    current_leader: &mut Option<Pubkey>,
    new_leader: &Pubkey,
)
```
Leader değişimini loglar. "I am now the leader" / "I am no longer the leader" mesajları üretir.

---

### 4.18 `ReplayStage::check_propagation_for_start_leader()` — (satır 1944-1982)
```rust
fn check_propagation_for_start_leader(
    poh_slot: Slot,
    parent_slot: Slot,
    progress_map: &ProgressMap,
) -> bool
```
Leader olarak blok üretmeye başlamadan önce önceki leader slot'un propagasyonunu kontrol eder.

**Önemli kural:** `poh_slot - latest_leader_slot < NUM_CONSECUTIVE_LEADER_SLOTS` ise propagation check atlanır. Bu, ardışık leader slot'larının propagation beklememesini sağlar.

---

### 4.19 `ReplayStage::should_retransmit()` — (satır 1984-1993)
```rust
fn should_retransmit(poh_slot: Slot, last_retransmit_slot: &mut Slot) -> bool
```
`poh_slot < last_retransmit_slot` VEYA `poh_slot >= last_retransmit_slot + NUM_CONSECUTIVE_LEADER_SLOTS` ise retransmit yapar.

---

### 4.20 `ReplayStage::maybe_start_leader()` — (satır 1995-2138)
```rust
fn maybe_start_leader(
    my_pubkey: &Pubkey,
    bank_forks: &Arc<RwLock<BankForks>>,
    poh_recorder: &Arc<RwLock<PohRecorder>>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    rpc_subscriptions: &Arc<RpcSubscriptions>,
    progress_map: &mut ProgressMap,
    retransmit_slots_sender: &Sender<Slot>,
    skipped_slots_info: &mut SkippedSlotsInfo,
    banking_tracer: &Arc<BankingTracer>,
    has_new_vote_been_rooted: bool,
    track_transaction_indexes: bool,
)
```
Bu validator için leader slot başlatır.

**Adımlar:**
1. `poh_recorder.reached_leader_slot()` kontrolü
2. `leader_schedule_cache.slot_leader_at()` ile kendimiz olup olmadığını doğrular
3. `has_new_vote_been_rooted` false ise atlar (vote landing beklenir)
4. `check_propagation_for_start_leader()` geçmezse retransmit ve atla
5. `MAX_ROOT_DISTANCE_FOR_VOTE_ONLY` aşıldıysa vote-only bank oluştur
6. `new_bank_from_parent_with_notify()` ile yeni bank yarat
7. `poh_recorder.set_bank()` ile PoH'u bağla

---

### 4.21 `ReplayStage::replay_blockstore_into_bank()` — (satır 2140-2178)
```rust
fn replay_blockstore_into_bank(
    bank: &BankWithScheduler,
    blockstore: &Blockstore,
    replay_tx_thread_pool: &ThreadPool,
    replay_stats: &RwLock<ReplaySlotStats>,
    replay_progress: &RwLock<ConfirmationProgress>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    entry_notification_sender: Option<&EntryNotifierSender>,
    replay_vote_sender: &ReplayVoteSender,
    verify_recyclers: &VerifyRecyclers,
    log_messages_bytes_limit: Option<usize>,
    prioritization_fee_cache: &PrioritizationFeeCache,
) -> result::Result<usize, BlockstoreProcessorError>
```
`blockstore_processor::confirm_slot()` çağırarak blok içeriğini bank'a uygular. Transaction sayısını döndürür. **Her hata dead slot ile sonuçlanmalıdır** (yorum satırında belirtilmiş).

---

### 4.22 `ReplayStage::mark_dead_slot()` — (satır 2180-2279)
```rust
fn mark_dead_slot(
    blockstore: &Blockstore,
    bank: &Bank,
    root: Slot,
    err: &BlockstoreProcessorError,
    rpc_subscriptions: &Arc<RpcSubscriptions>,
    ...
)
```
Bir slot'u dead olarak işaretler. `TooFewTicks` hatasını "serious" saymaz (leader abandon etmiş olabilir). Blockstore'a dead flag yazar, RPC'ye bildirir, `check_slot_agrees_with_cluster()` çağırır.

---

### 4.23 `ReplayStage::handle_votable_bank()` — (satır 2281-2409)
```rust
fn handle_votable_bank(
    bank: &Arc<Bank>,
    switch_fork_decision: &SwitchForkDecision,
    bank_forks: &Arc<RwLock<BankForks>>,
    tower: &mut Tower,
    ...
)
```
Oy vermeye karar verilmiş bir bank için:
1. `tower.record_bank_vote(bank)` — Tower'a kaydet, yeni root döner
2. Yeni root varsa `handle_new_root()` çağır, blockstore/leader schedule güncelle
3. `update_commitment_cache()` — Commitment'ı güncelle
4. `push_vote()` — Oy tx'ini gönder

---

### 4.24 `ReplayStage::generate_vote_tx()` — (satır 2411-2519)
```rust
fn generate_vote_tx(
    node_keypair: &Keypair,
    bank: &Bank,
    vote_account_pubkey: &Pubkey,
    authorized_voter_keypairs: &[Arc<Keypair>],
    vote: VoteTransaction,
    switch_fork_decision: &SwitchForkDecision,
    vote_signatures: &mut Vec<Signature>,
    has_new_vote_been_rooted: bool,
    wait_to_vote_slot: Option<Slot>,
) -> GenerateVoteTxResult
```
Vote transaction üretir. Başarısızlık sebepleri:
- Startup verification tamamlanmamış
- `authorized_voter_keypairs` boş → NonVoting
- `wait_to_vote_slot` henüz gelmemiş
- Vote account bulunamadı
- Vote state okunamadı
- `node_pubkey` uyumsuzluğu
- Epoch için authorized voter yok

`VoteStateUpdate` tipini `CompactVoteStateUpdate`'e çevirir. İki partial sign yapar: node keypair + authorized voter keypair.

---

### 4.25 `ReplayStage::refresh_last_vote()` — (satır 2521-2624)
```rust
fn refresh_last_vote(
    tower: &mut Tower,
    heaviest_bank_on_same_fork: &Bank,
    my_latest_landed_vote: Slot,
    ...
)
```
Vote tx'i geçerliliğini yitirmiş (`MAX_PROCESSING_AGE` geçmiş) ve henüz landing yapılmamışsa vote'u yeniler. Birden fazla fork üzerinde oylama yapılmasını önlemek için `MAX_VOTE_REFRESH_INTERVAL_MILLIS` kontrolü yapar.

---

### 4.26 `ReplayStage::push_vote()` — (satır 2626-2673)
```rust
fn push_vote(
    bank: &Bank,
    vote_account_pubkey: &Pubkey,
    identity_keypair: &Keypair,
    authorized_voter_keypairs: &[Arc<Keypair>],
    tower: &mut Tower,
    switch_fork_decision: &SwitchForkDecision,
    vote_signatures: &mut Vec<Signature>,
    has_new_vote_been_rooted: bool,
    replay_timing: &mut ReplayLoopTiming,
    voting_sender: &Sender<VoteOp>,
    wait_to_vote_slot: Option<Slot>,
)
```
`generate_vote_tx()` sonucuna göre `VoteOp::PushVote` mesajını `voting_sender` kanalına gönderir. Tower'ı ve SavedTower versiyonunu da birlikte gönderir.

---

### 4.27 `ReplayStage::update_commitment_cache()` — (satır 2675-2686)
```rust
fn update_commitment_cache(
    bank: Arc<Bank>,
    root: Slot,
    total_stake: Stake,
    lockouts_sender: &Sender<CommitmentAggregationData>,
)
```
`CommitmentAggregationData` oluşturup `lockouts_sender`'a gönderir. `AggregateCommitmentService` bunu işleyerek block commitment cache'i günceller.

---

### 4.28 `ReplayStage::reset_poh_recorder()` — (satır 2688-2716)
```rust
fn reset_poh_recorder(
    my_pubkey: &Pubkey,
    blockstore: &Blockstore,
    bank: Arc<Bank>,
    poh_recorder: &RwLock<PohRecorder>,
    leader_schedule_cache: &LeaderScheduleCache,
)
```
PoH recorder'ı yeni bank üzerine reset eder. `next_leader_slot` hesaplayarak recorder'a iletir.

---

### 4.29 `ReplayStage::replay_active_banks_concurrently()` — (satır 2718-2832)
```rust
fn replay_active_banks_concurrently(
    blockstore: &Blockstore,
    bank_forks: &RwLock<BankForks>,
    fork_thread_pool: &ThreadPool,
    replay_tx_thread_pool: &ThreadPool,
    ...
    active_bank_slots: &[Slot],
    ...
) -> Vec<ReplaySlotFromBlockstore>
```
Birden fazla fork'u Rayon thread pool ile paralel replay eder. Progress map'i `RwLock<ProgressMap>` ile thread-safe paylaşır. Her fork ayrı thread'de `replay_blockstore_into_bank()` çağırır. `AtomicU64` ile en uzun replay süresini kaydeder.

---

### 4.30 `ReplayStage::replay_active_bank()` — (satır 2834-2913)
```rust
fn replay_active_bank(
    ...
    bank_slot: Slot,
    ...
) -> ReplaySlotFromBlockstore
```
Tek bir bank slot'unu serial olarak replay eder. Dead ise atlar, bank kendi block producer'ımız ise de atlar.

---

### 4.31 `ReplayStage::process_replay_results()` — (satır 2915-3166)
```rust
fn process_replay_results(
    ...
    replay_result_vec: &[ReplaySlotFromBlockstore],
    ...
) -> bool /* completed a bank */
```
Replay sonuçlarını işler. Hatalı slot'ları dead olarak işaretler. Tamamlanan bank'ları freeze eder, fork choice ağacına ekler, reward'ları kaydeder, cluster'a bildirir.

**Bank freeze sırası kritiktir:**
1. Transaction status gönder
2. `bank.freeze()` çağır
3. Fork choice ağacına ekle
4. `check_slot_agrees_with_cluster()` çağır (duplicate check)
5. RPC'ye bildirim
6. `cache_block_meta()` çağır
7. Gossip vote hash'lerini işle
8. Reward'ları kaydet

---

### 4.32 `ReplayStage::replay_active_banks()` — (satır 3168-3279)
```rust
fn replay_active_banks(...) -> bool /* completed a bank */
```
Aktif bank slot'larını alır, mode'a göre parallel veya serial replay seçer, ardından `process_replay_results()` çağırır. Sadece >1 aktif bank varsa parallel mod kullanılır.

---

### 4.33 `ReplayStage::compute_bank_stats()` — (satır 3281-3464)
```rust
pub fn compute_bank_stats(
    my_vote_pubkey: &Pubkey,
    ancestors: &HashMap<u64, HashSet<u64>>,
    frozen_banks: &mut [Arc<Bank>],
    tower: &mut Tower,
    progress: &mut ProgressMap,
    vote_tracker: &VoteTracker,
    cluster_slots: &ClusterSlots,
    bank_forks: &RwLock<BankForks>,
    heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
    latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
) -> Vec<Slot>
```
Her frozen bank için stake ağırlıklı istatistik hesaplar.

**Kritik tower sync mantığı (satır 3309-3399):**
Eğer bankta kaydedilmiş vote state bizim local tower'ımızdan daha yeniyse, bankın vote state'ini tower'a alırız. Bu, node restart veya identity değişikliğinden sonra tower'ı güncel tutmak için gerekli. Ayrıca local root bankın onchain root'tan büyük olması durumunu da ele alır (supermajority root senkronizasyonu).

`Tower::collect_vote_lockouts()` ile voted_stakes, total_stake, fork_stake, lockout_intervals hesaplanır. `heaviest_subtree_fork_choice.compute_bank_stats()` çağrılır. `update_propagation_status()` ve `cache_tower_stats()` de burada çağrılır.

---

### 4.34 `ReplayStage::cache_tower_stats()` — (satır 3466-3486)
```rust
fn cache_tower_stats(
    progress: &mut ProgressMap,
    tower: &Tower,
    slot: Slot,
    ancestors: &HashMap<u64, HashSet<u64>>,
)
```
Tower'ın mevcut durumuna göre fork stats'ı günceller: `vote_threshold`, `is_locked_out`, `has_voted`, `is_recent`.

---

### 4.35 `ReplayStage::update_propagation_status()` — (satır 3488-3533)
```rust
fn update_propagation_status(
    progress: &mut ProgressMap,
    slot: Slot,
    bank_forks: &RwLock<BankForks>,
    vote_tracker: &VoteTracker,
    cluster_slots: &ClusterSlots,
)
```
Slot'un propagasyon durumunu kontrol eder. VoteTracker'dan yeni vote'ları ve ClusterSlots'tan cluster pubkey'lerini alarak `update_fork_propagated_threshold_from_votes()` çağırır.

---

### 4.36 `ReplayStage::select_forks_failed_switch_threshold()` — (satır 3535-3624)
```rust
fn select_forks_failed_switch_threshold(
    reset_bank: Option<&Bank>,
    progress: &ProgressMap,
    tower: &Tower,
    heaviest_bank_slot: Slot,
    failure_reasons: &mut Vec<HeaviestForkFailures>,
    switch_proof_stake: u64,
    total_stake: u64,
    switch_fork_decision: SwitchForkDecision,
) -> SwitchForkDecision
```
Switch threshold başarısız olduğunda davranışı belirler. Eğer son oy blok hashes geçmişi dışına çıkmışsa, aynı fork üzerinde oy kullanmaya devam etmeye zorlar (deadlock önleme). Aksi halde voted fork'ta bekler.

**Figure 1 açıklaması (satır 3591-3615):** 90% validator'ın %9'u landed slot üzerinde oylarını vermiş durumda. Eğer hepsi %10 ağırlıklı heavier fork'a geçerse, %90'lık fork'un oyları hiçbir bloka giremez ve ağ durur. Bunun önüne geçmek için switch threshold başarısız olduğunda voted fork'ta reset yapılır.

---

### 4.37 `ReplayStage::select_vote_and_reset_forks()` — (satır 3626-3869)
```rust
pub fn select_vote_and_reset_forks(
    heaviest_bank: &Arc<Bank>,
    heaviest_bank_on_same_voted_fork: Option<&Arc<Bank>>,
    ancestors: &HashMap<u64, HashSet<u64>>,
    descendants: &HashMap<u64, HashSet<u64>>,
    progress: &ProgressMap,
    tower: &mut Tower,
    latest_validator_votes_for_frozen_banks: &LatestValidatorVotesForFrozenBanks,
    fork_choice: &HeaviestSubtreeForkChoice,
) -> SelectVoteAndResetForkResult
```
Fork seçim kararının merkezidir. İki aşamalı karar:

**Aşama 1 — Switch threshold kontrolü:**
- `FailedSwitchThreshold`: Voted fork'ta reset, ya heaviest ya da SameFork vote
- `FailedSwitchDuplicateRollback`: Heaviest fork'ta reset, vote yok (duplicate güvenliği)
- Diğerleri: Heaviest fork'ta hem reset hem vote

**Aşama 2 — Vote kontrolleri** (candidate_vote_bank varsa):
- `is_locked_out` → LockedOut failure
- `vote_threshold.passed()` → FailedThreshold failure
- `propagation_confirmed` → NoPropagatedConfirmation failure

Tüm kontroller geçilirse: `vote_bank = Some(candidate)`, `reset_bank = Some(candidate)` → OY VER.

---

### 4.38 `ReplayStage::update_fork_propagated_threshold_from_votes()` — (satır 3871-3927)
```rust
fn update_fork_propagated_threshold_from_votes(
    progress: &mut ProgressMap,
    mut newly_voted_pubkeys: Vec<Pubkey>,
    mut cluster_slot_pubkeys: Vec<Pubkey>,
    fork_tip: Slot,
    bank_forks: &BankForks,
)
```
Fork ucu'ndan geriye doğru linked list üzerinde yürüyerek her leader slot'un propagasyon durumunu günceller. Bir descendant eşiğe ulaşırsa tüm atalar da propagated kabul edilir.

---

### 4.39 `ReplayStage::update_slot_propagated_threshold_from_votes()` — (satır 3929-3993)
```rust
fn update_slot_propagated_threshold_from_votes(
    newly_voted_pubkeys: &mut Vec<Pubkey>,
    cluster_slot_pubkeys: &mut Vec<Pubkey>,
    leader_bank: &Bank,
    leader_propagated_stats: &mut PropagatedStats,
    did_child_reach_threshold: bool,
) -> bool /* did newly reach threshold */
```
Tek bir leader slot için propagasyon eşiğini kontrol eder. `SUPERMINORITY_THRESHOLD` (%33.3) geçilirse propagated olarak işaretler. Önceden görülen pubkey'leri filtreler.

---

### 4.40 `ReplayStage::mark_slots_confirmed()` — (satır 3995-4064)
```rust
fn mark_slots_confirmed(
    confirmed_slots: &[ConfirmedSlot],
    ...
)
```
`confirm_forks()` çıktısını işler. SupermajorityVoted tipinde olanları supermajority_confirmed olarak işaretler. Sonra `duplicate_confirmed_slots` map'ine ekleyip `check_slot_agrees_with_cluster()` çağırır.

---

### 4.41 `ReplayStage::confirm_forks()` — (satır 4066-4120)
```rust
fn confirm_forks(
    tower: &Tower,
    voted_stakes: &VotedStakes,
    total_stake: Stake,
    progress: &ProgressMap,
    bank_forks: &RwLock<BankForks>,
) -> Vec<ConfirmedSlot>
```
Henüz supermajority confirmed olmayan her frozen bank için `tower.is_slot_confirmed()` veya `tower.is_slot_duplicate_confirmed()` kontrolü yapar. Onaylanmış slot'ları döndürür.

---

### 4.42 `ReplayStage::handle_new_root()` — (satır 4122-4175)
```rust
pub fn handle_new_root(
    new_root: Slot,
    bank_forks: &RwLock<BankForks>,
    progress: &mut ProgressMap,
    accounts_background_request_sender: &AbsRequestSender,
    highest_super_majority_root: Option<Slot>,
    heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
    duplicate_slots_tracker: &mut DuplicateSlotsTracker,
    duplicate_confirmed_slots: &mut DuplicateConfirmedSlots,
    unfrozen_gossip_verified_vote_hashes: &mut UnfrozenGossipVerifiedVoteHashes,
    has_new_vote_been_rooted: &mut bool,
    voted_signatures: &mut Vec<Signature>,
    epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
    drop_bank_sender: &Sender<Vec<Arc<Bank>>>,
)
```
Yeni root belirlendikten sonra tüm state'leri temizler:
1. Program cache prune
2. `bank_forks.set_root()` — Eski fork'ları kaldırır
3. Kaldırılan bank'ları `drop_bank_sender`'a gönderir (async drop)
4. `has_new_vote_been_rooted` kontrolü — Vote imzalarını kontrol eder
5. Progress map, fork choice, duplicate tracker, epoch slots günceller
6. Root öncesi entry'leri `split_off()` ile temizler

---

### 4.43 `ReplayStage::generate_new_bank_forks()` — (satır 4177-4275)
```rust
fn generate_new_bank_forks(
    blockstore: &Blockstore,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &Arc<LeaderScheduleCache>,
    rpc_subscriptions: &Arc<RpcSubscriptions>,
    progress: &mut ProgressMap,
    replay_timing: &mut ReplayLoopTiming,
)
```
Blockstore'dan yeni slot'ları keşfeder ve bank fork'ları oluşturur.

**Adımlar:**
1. Frozen bank slot'larını topla
2. `blockstore.get_slots_since()` ile child slot'ları bul
3. Zaten active/frozen olanları atla
4. Her yeni slot için `new_bank_from_parent_with_notify()` çağır
5. Leader için propagation güncelle
6. Write lock alarak bank_forks'a ekle

Dikkat: Read lock → hesapla → Write lock → ekle pattern'i kullanılır. Lock granülaritesi performans için kritik.

---

### 4.44 `ReplayStage::new_bank_from_parent_with_notify()` — (satır 4277-4287)
```rust
fn new_bank_from_parent_with_notify(
    parent: Arc<Bank>,
    slot: u64,
    root_slot: u64,
    leader: &Pubkey,
    rpc_subscriptions: &Arc<RpcSubscriptions>,
    new_bank_options: NewBankOptions,
) -> Bank
```
RPC'ye slot bildirimi göndererek yeni bank yaratır.

---

### 4.45 `ReplayStage::record_rewards()` — (satır 4289-4301)
```rust
fn record_rewards(bank: &Bank, rewards_recorder_sender: &Option<RewardsRecorderSender>)
```
Bank'ın reward'larını `RewardsRecorderSender`'a batch ve complete mesajları ile gönderir.

---

### 4.46 `ReplayStage::get_unlock_switch_vote_slot()` — (satır 4303-4312)
```rust
pub fn get_unlock_switch_vote_slot(cluster_type: ClusterType) -> Slot
```
Cluster tipine göre switch vote unlock slot'unu döndürür:
- Development/Devnet: 0 (hemen)
- Testnet: 21_692_256 (Epoch 63)
- MainnetBeta: 26_752_000 (Epoch 61'de 400K slot)

---

### 4.47 `ReplayStage::join()` — (satır 4314-4317)
```rust
pub fn join(self) -> thread::Result<()>
```
Commitment service ve replay thread'lerini bekler.

---

## 5. ANA DÖNGÜ — DETAYLI AKIS ANALIZİ

`run_replay` closure (satır 587-1200), `"solReplayStage"` thread'i içinde döngü çalıştırır.

### 5.1 Başlangıç Adımları (satır 587-681)
```
1. VerifyRecyclers::default() oluştur
2. Finalizer'ı başlat (exit_sender drop guard)
3. Identity keypair'i al
4. Eğer identity startup'ta değiştiyse → load_tower()
5. initialize_progress_and_fork_choice_with_locked_bank_forks()
6. current_leader = None, last_reset = Hash::default()
7. Tüm tracker/map'leri initialize et
8. replay_mode: Parallel(pool) veya Serial
9. replay_tx_thread_pool: get_max_thread_count() thread
10. reset_poh_recorder() ile PoH'u ilk bank'a resetle
```

### 5.2 Ana Döngü Adımları (satır 682-1199)

Her iteration:

```
[ÇIKIŞ KONTROLÜ] exit.load(Relaxed) → break

[A] generate_new_bank_forks()
    → Blockstore'dan yeni slot'ları keşfet, Bank fork'ları oluştur

[B] replay_active_banks()
    → Aktif bank'ları Blockstore'dan replay et
    → did_complete_bank: bool

[C] process_ancestor_hashes_duplicate_slots()
    → ancestor_hashes_service'ten gelen duplicate sinyalleri

[D] process_duplicate_confirmed_slots()
    → gossip/replay confirmed duplicate'ler

[E] process_gossip_verified_vote_hashes()
    → Gossip'ten gelen henüz bloka girmemiş oylar

[F] process_popular_pruned_forks()
    → %52+ stake ile desteklenen ama pruned fork'lar

[G] process_duplicate_slots() [sadece !tpu_has_bank]
    → WindowService'ten gelen duplicate sinyalleri

[H] collect frozen_banks (forks_root >= root filtresi)

[I] compute_bank_stats()
    → Her frozen bank için ağırlık hesapla
    → Tower sync, propagasyon güncelleme

[J] compute_slot_stats_time:
    → Her yeni computed slot için confirm_forks()
    → mark_slots_confirmed()

[K] select_forks()
    → heaviest_bank, heaviest_bank_on_same_voted_fork seç

[L] check_for_vote_only_mode()

[M] select_vote_and_reset_forks()
    → vote_bank, reset_bank, heaviest_fork_failures

[N] Eğer vote_bank.is_none():
    → refresh_last_vote() — Süresi dolmuş vote'u yenile

[O] Heaviest fork failures logla

[P] Eğer vote_bank.is_some():
    → handle_votable_bank() — Oy ver, commitment güncelle

[Q] Eğer reset_bank.is_some() ve last_blockhash değiştiyse:
    → Identity değişikliği kontrolü (set-identity)
    → reset_poh_recorder()
    → partition_info.update()

[R] dump_then_repair_correct_slots()
    → Onaylı doğru hash'e sahip slot'ları temizle ve repair et

[S] retransmit_latest_unpropagated_leader_slot()

[T] maybe_start_leader()
    → Kendi leader slot'umuzu başlat

[U] Eğer did_complete_bank == false:
    → ledger_signal_receiver.recv_timeout(100ms) — Bekle

[V] replay_timing.update() — İstatistikleri topla
```

### 5.3 Kanal Listesi (Okunan)

| Kanal | Kaynak | Ne İçin |
|-------|--------|---------|
| `ledger_signal_receiver` | WindowService | Yeni shred sinyali |
| `duplicate_slots_receiver` | WindowService | Duplicate slot tespiti |
| `ancestor_duplicate_slots_receiver` | AncestorHashesService | Ancestor duplicate |
| `duplicate_confirmed_slots_receiver` | ClusterInfoVoteListener | Gossip/replay onaylı |
| `gossip_verified_vote_hash_receiver` | ClusterInfoVoteListener | Gossip oyları |
| `popular_pruned_forks_receiver` | RepairService | Popüler pruned fork'lar |

### 5.4 Kanal Listesi (Yazılan)

| Kanal | Hedef | Ne İçin |
|-------|-------|---------|
| `voting_sender` | VotingService | Vote tx gönderme |
| `retransmit_slots_sender` | RetransmitStage | Slot retransmit |
| `cluster_slots_update_sender` | ClusterSlotsService | Cluster slot güncel |
| `cost_update_sender` | CostUpdateService | Maliyet güncelleme |
| `drop_bank_sender` | DropBankService | Async bank drop |
| `lockouts_sender` | CommitmentService | Commitment güncelleme |
| `dumped_slots_sender` | RepairService | Dumped slot bildirimi |
| `ancestor_hashes_replay_update_sender` | AncestorHashesService | Replay güncel |

---

## 6. FORK SEÇİM ALGORİTMASI — HEAVIEST FORK

### 6.1 Kavramsal Model
Solana Tower BFT'de fork seçimi iki aşamalıdır:
1. **En ağır fork'u bul** (`HeaviestSubtreeForkChoice::select_forks()`)
2. **O fork'a oy verebilir miyiz?** (`select_vote_and_reset_forks()`)

### 6.2 Ağırlık Hesabı
`compute_bank_stats()` her frozen bank için:
- `Tower::collect_vote_lockouts()` ile tüm validator oylarını toplar
- Her oyu lockout'a göre ağırlıklandırır
- `fork_stake`: Bu fork üzerinde stake miktarı
- `fork_weight()`: `fork_stake / total_stake`

### 6.3 Switch Threshold
Farklı bir fork'a geçmek için gerekli:
- `SWITCH_FORK_THRESHOLD` (%38, `consensus.rs`'den) stake'in o fork üzerinde oylaması gerekir
- Veya: oy verdiğimiz fork duplicate olarak işaretlenmiş olmalı
- `tower.check_switch_threshold()` bu kararı verir

### 6.4 Vote Kuralları
Bir bank'a oy vermek için tüm bunlar doğru olmalı:
1. `!is_locked_out` — Tower lockout yok
2. `vote_threshold.passed()` — %2/3 eşiği geçilmiş
3. `propagation_confirmed` — Leader slot propagated
4. `switch_fork_decision.can_vote()` — Fork switch onaylandı

---

## 7. VOTE GÖNDERME MANTIĞI

### 7.1 Normal Vote Akışı
```
handle_votable_bank()
  ├── tower.record_bank_vote(bank)      → Yeni root hesapla
  ├── [yeni root varsa] handle_new_root()
  ├── update_commitment_cache()
  └── push_vote()
        ├── generate_vote_tx()          → Transaction oluştur
        ├── tower.refresh_last_vote_tx_blockhash()
        ├── SavedTower::new()           → Tower'ı serializela
        └── voting_sender.send(PushVote { tx, tower_slots, saved_tower })
```

### 7.2 Vote Refresh Akışı
Vote tx'i zamanaşımına uğrarsa (blockhash süresi dolmuş) vote yenilenir:
```
refresh_last_vote()
  ├── tower.last_voted_slot() → Son oy slot
  ├── tower.last_vote_tx_blockhash() kontrolü
  ├── MAX_VOTE_REFRESH_INTERVAL_MILLIS kontrolü (5 saniye)
  ├── tower.refresh_last_vote_timestamp()
  ├── generate_vote_tx() ile SwitchForkDecision::SameFork
  └── voting_sender.send(RefreshVote { tx, last_voted_slot })
```

### 7.3 CompactVoteStateUpdate
Vote transaction'ı gönderilmeden önce `VoteStateUpdate` → `CompactVoteStateUpdate` dönüşümü yapılır (satır 2490-2494). Bu, transaction boyutunu küçültür.

---

## 8. DUPLICATE SLOT TESPİTİ VE KURTARMA

### 8.1 Duplicate Tespit Kaynakları
```
WindowService       → duplicate_slots_receiver       (lokal tespit)
AncestorHashesService → ancestor_duplicate_slots_receiver (uzak tespit)
ClusterInfoVoteListener → duplicate_confirmed_slots_receiver (gossip onay)
RepairService       → popular_pruned_forks_receiver  (pruned popular fork)
```

### 8.2 Duplicate State Machine
`check_slot_agrees_with_cluster()` çağrısıyla yönetilir. State güncellemeleri:
- `SlotStateUpdate::Duplicate`
- `SlotStateUpdate::DuplicateConfirmed`
- `SlotStateUpdate::BankFrozen`
- `SlotStateUpdate::Dead`
- `SlotStateUpdate::EpochSlotsFrozen`
- `SlotStateUpdate::PopularPrunedFork`

### 8.3 Kurtarma Adımları
```
Cluster doğru hash'i onayladı
    ↓
duplicate_slots_to_repair'e ekle
    ↓
dump_then_repair_correct_slots()
    ↓
purge_unconfirmed_duplicate_slot() [eğer hata versiyonu bizde varsa]
    ├── ancestors/descendants temizle
    ├── BankForks'tan kaldır
    ├── Account state sıfırla
    ├── Blockstore temizle
    └── Progress map'ten kaldır
    ↓
dumped_slots_sender.send() [repair service'e bildir]
    ↓
RepairService doğru versiyonu indirir
    ↓
generate_new_bank_forks() yeni bank oluşturur
    ↓
replay_active_banks() yeniden oynatır
```

### 8.4 Güvenlik Garantileri
- Kendi ürettiğimiz blokları dump edemeyiz → panic (satır 1507-1511)
- MAX_REPAIR_RETRY_LOOP_ATTEMPTS (10) aşılırsa → panic (satır 1519-1524)
- PoH bank duplicate fork üzerindeyse → bekle (satır 1458-1465)

---

## 9. SNAPSHOT TETİKLEME

ReplayStage snapshot'ı doğrudan tetiklemez. Ancak `handle_new_root()` içinde `bank_forks.set_root()` çağrısı sırasında `accounts_background_request_sender` kullanılır:

```rust
// satır 4139-4143
let removed_banks = bank_forks.write().unwrap().set_root(
    new_root,
    accounts_background_request_sender,  // ← AccountsBackgroundService'e istek
    highest_super_majority_root,
);
```

`AccountsBackgroundService`, root değişiminde snapshot almayı kararlaştırır. Snapshot politikası `accounts_background_service.rs`'de tanımlıdır.

---

## 10. BÖLÜNME (PARTITION) TESPİTİ

### 10.1 Tespit
```rust
// satır 1346-1356
fn is_partition_detected(
    ancestors: &HashMap<Slot, HashSet<Slot>>,
    last_voted_slot: Slot,
    heaviest_slot: Slot,
) -> bool {
    last_voted_slot != heaviest_slot
        && !ancestors
            .get(&heaviest_slot)
            .map(|ancestors| ancestors.contains(&last_voted_slot))
            .unwrap_or(true)
}
```
Son oy verilen slot, en ağır slot'un atası değilse bölünme var demektir.

### 10.2 Raporlama
`PartitionInfo::update()` (satır 223-272):
- Bölünme başladığında: `replay_stage-partition-start` datapoint
- Bölünme çözüldüğünde: `replay_stage-partition-resolved` datapoint + süre
- `PARTITION DETECTED` / `PARTITION resolved` warn mesajları

### 10.3 Bölünme Sırasında Davranış
```
last_voted_slot     heaviest_bank.slot()
       |                   |
    [fork A]           [fork B]   ← Bölünme!
       ↓
reset_bank = heaviest_bank_on_same_voted_fork  (fork A'nın ucu)
vote_bank = None (switch threshold geçilmeli)
PoH = fork A'nın ucuna sıfırla
```

---

## 11. UNSAFE BLOKLAR

**Dosyada hiçbir `unsafe` bloğu bulunmamaktadır.** Bu, kritik bir tasarım tercihidir — konsensüs kodu Rust'ın tam güvenli subset'i içinde yazılmıştır.

---

## 12. TEST FONKSİYONLARI — KAPSAMLI LİSTE

Tüm testler `#[cfg(test)] pub(crate) mod tests` (satır 4320) modülünde yer alır.

### 12.1 Partition Tespiti
**`test_is_partition_detected`** (satır 4393) — Ancestor ilişkisine göre partition tespiti doğrular.

### 12.2 Fork Oluşturma
**`test_child_slots_of_same_parent`** (satır 4518) — Aynı parent'tan iki farklı child slot oluşturulabildiğini ve propagasyon durumunun doğru güncellendiğini test eder.

### 12.3 Root İşleme
**`test_handle_new_root`** (satır 4640) — Yeni root sonrası tüm state'lerin (progress, duplicate_slots_tracker, duplicate_confirmed_slots vb.) temizlendiğini doğrular.

**`test_handle_new_root_ahead_of_highest_super_majority_root`** (satır 4725) — highest_super_majority_root'un root'tan küçük olduğu durumda confirmed_root'un korunduğunu test eder.

### 12.4 Dead Fork Testi
**`test_dead_fork_transaction_error`** (satır 4783) — AccountNotFound hatası → dead slot
**`test_dead_fork_entry_verification_failure`** (satır 4825) — InvalidEntryHash → dead slot
**`test_dead_fork_invalid_tick_hash_count`** (satır 4861) — InvalidTickHashCount → dead slot
**`test_dead_fork_invalid_slot_tick_count`** (satır 4887) — TooManyTicks ve TooFewTicks
**`test_dead_fork_invalid_last_tick`** (satır 4933) — InvalidLastTick
**`test_dead_fork_trailing_entry`** (satır 4956) — TrailingEntry
**`test_dead_fork_entry_deserialize_failure`** (satır 4986) — InvalidShredData

Bunlar `check_dead_fork()` yardımcı fonksiyonu (satır 5015) üzerinden çalışır.

### 12.5 Commitment Cache
**`test_replay_commitment_cache`** (satır 5113) — Bank completion sonrası commitment cache'in doğru güncellenmesini 3 slot üzerinde test eder.

### 12.6 Transaction Status
**`test_write_persist_transaction_status`** (satır 5243) — İşlem sonuçlarının blockstore'a doğru yazıldığını test eder.

### 12.7 Banka İstatistikleri
**`test_compute_bank_stats_confirmed`** (satır 5314) — Supermajority confirmation hesabı
**`test_same_weight_select_lower_slot`** (satır 5454) — Eşit ağırlıkta düşük slot seçilir
**`test_child_bank_heavier`** (satır 5513) — Child bank parent'tan her zaman ağır

### 12.8 Retransmit
**`test_should_retransmit`** (satır 5586) — Retransmit zamanlaması
**`test_retransmit_latest_unpropagated_leader_slot`** (satır 8127) — Exponential backoff retransmit
**`test_maybe_retransmit_unpropagated_slots`** (satır 8304) — Consecutive slot range retransmit

### 12.9 Propagasyon
**`test_update_slot_propagated_threshold_from_votes`** (satır 5623) — 10/10 validator, 4'ü yeterli
**`test_update_slot_propagated_threshold_from_votes2`** (satır 5731) — Child threshold → parent da propagated
**`test_update_propagation_status`** (satır 5782) — VoteTracker entegrasyonu
**`test_chain_update_propagation_status`** (satır 5870) — Zincir boyunca propagasyon yayılımı
**`test_chain_update_propagation_status2`** (satır 5952) — Kısmi eşik, sadece ilk 5 bank propagated
**`test_check_propagation_for_start_leader`** (satır 6038) — Leader başlatma için propagasyon kontrolü
**`test_check_propagation_skip_propagation_check`** (satır 6145) — Consecutive leader slot'lar için skip
**`test_leader_snapshot_restart_propagation`** (satır 6615) — Leader restart sonrası propagasyon

### 12.10 Duplicate Purge
**`test_purge_unconfirmed_duplicate_slot`** (satır 6228) — Slot 5, 4, 1 purge senaryoları, dead slot davranışı
**`test_purge_unconfirmed_duplicate_slots_and_reattach`** (satır 6379) — Purge sonrası repair ve yeniden attach
**`test_purge_ancestors_descendants`** (satır 6563) — Map tutarlılığı

### 12.11 Duplicate + Lockout
**`test_unconfirmed_duplicate_slots_and_lockouts_for_non_heaviest_fork`** (satır 6691) — Karmaşık fork senaryosu, duplicate confirmation sonrası davranış
**`test_unconfirmed_duplicate_slots_and_lockouts`** (satır 6899) — 2 validator, 2 fork, duplicate + lockout etkileşimi

### 12.12 Dump ve Repair
**`test_dump_then_repair_correct_slots`** (satır 7072) — İki slot dump edilir, purge_repair_slot_counter doğrulanır
**`test_dump_own_slots_fails`** (satır 8386) — `#[should_panic]` — Kendi slot'umuzu dump etmeye çalışmak panikler

### 12.13 Rollback ve Vote
**`test_duplicate_rollback_then_vote_locked_out`** (satır 7315) — Slot 5'e oy → rollback → slot 7 locked out
**`test_duplicate_rollback_then_vote_success`** (satır 7333) — Slot 4'e oy → rollback → slot 7 başarılı switch
**`test_duplicate_rollback_then_vote_on_other_duplicate_success`** (satır 7438) — Başka duplicate üzerine başarılı oy
**`test_duplicate_rollback_then_vote_on_other_duplicate_same_slot_locked_out`** (satır 7457) — Aynı slot tekrar oy → locked out
**`test_duplicate_rollback_then_vote_on_other_duplicate_different_slot_locked_out`** (satır 7475) — `#[ignore]` — Farklı slot locked out

### 12.14 Gossip Vote
**`test_gossip_vote_doesnt_affect_fork_choice`** (satır 7494) — Gossip oyları fork choice'ı değiştirmez

### 12.15 Vote Refresh
**`test_replay_stage_refresh_last_vote`** (satır 7538) — Vote tx zamanaşımı, refresh, landing senaryoları
**`test_replay_stage_last_vote_outside_slot_hashes`** (satır 7918) — SlotHashes dışına çıkan vote, tip'e oy verme

### 12.16 Tower Sync
**`test_tower_sync_from_bank_failed_switch`** (satır 8551) — Güncelsiz tower'ı banktan senkronize et, switch threshold başarısız
**`test_tower_sync_from_bank_failed_lockout`** (satır 8632) — Tower sync, locked out ama reset başarılı

### 12.17 Tower Storage
**`test_tower_load_missing`** (satır 8703) — Eksik tower dosyası, bankforks'tan yeni tower
**`test_tower_load`** (satır 8728) — Mevcut tower dosyasını yükle

### 12.18 Vote Only Mode
**`test_check_for_vote_only_mode`** (satır 8539) — Uzak slot → vote only mode aktif/pasif

### 12.19 Yardımcı Fonksiyonlar
- `new_bank_from_parent_with_bank_forks()` (satır 4379)
- `replay_blockstore_components()` (satır 4421) — Tam test ortamı kurar
- `check_dead_fork()` (satır 5015)
- `run_compute_and_select_forks()` (satır 8434) — compute + select birleşik çalıştırma
- `setup_vote_then_rollback()` (satır 7142) — Vote → rollback senaryosu kurulumu
- `send_vote_in_new_bank()` (satır 7838) — Vote gönder ve bank oluştur
- `setup_forks_from_tree()` (satır 8488) — Tree'den fork yapısı kur
- `setup_default_forks()` (satır 8509) — Standart fork yapısı: `0/1/(2/4)/(3/5/6)`
- `check_map_eq()` (satır 8532) — HashMap eşitlik kontrolü
- `receive_slots()` (satır 8296) — Receiver'dan tüm slot'ları topla

---

## 13. ÖNEMLI TASARIM KARARLARI

### 13.1 İki Thread Pool Stratejisi
- **`fork_thread_pool`**: `MAX_CONCURRENT_FORKS_TO_REPLAY` (4) thread — Farklı fork'ların paralel replay'i
- **`replay_tx_thread_pool`**: `get_max_thread_count()` thread — Tek blok içinde transaction paralel işleme

Bu ayrım, fork düzeyi paralelliği (coarse-grained) ile transaction düzeyi paralelliği (fine-grained) karıştırmaz.

### 13.2 Lock Granülaritesi
`generate_new_bank_forks()` 4 ayrı timing ölçümü alır:
1. `read_lock` — Frozen bank'ları al
2. `get_slots_since` — Blockstore sorgusu (lock yok)
3. `loop` — Bank oluşturma (lock yok)
4. `write_lock` — Bank ekle

Bu pattern, write lock süresini minimum tutar ve blockstore I/O'su için lock'u serbest bırakır.

### 13.3 Identity Değişikliği Desteği
Ana döngü içinde (satır 1038-1067) `cluster_info.id()` ile identity değişikliği kontrol edilir. Değişim olursa:
1. Yeni keypair alınır
2. Yeni tower yüklenir (başarısız olursa process exit)
3. `has_new_vote_been_rooted` sıfırlanır (yeni identity için vote landing beklenir)

### 13.4 Vote-Only Mode
`MAX_ROOT_DISTANCE_FOR_VOTE_ONLY` aşıldığında `in_vote_only_mode = true` olur ve yeni bank'lar `vote_only_bank = true` bayrağıyla oluşturulur. Bu mode'da normal user tx'leri kabul edilmez, sadece vote tx'leri.

### 13.5 Deterministic Duplicate Recovery
`MAX_REPAIR_RETRY_LOOP_ATTEMPTS` (10) sayacı, sonsuz döngü olmasını önler. Sayaç dolunca `panic!` ile çıkar — bu, runtime bug tespiti için kasıtlı bir güvenlik ağıdır.

### 13.6 Finalizer Drop Guard
```rust
let _exit = Finalizer::new(exit.clone());
```
Thread panikleseydi bile `exit` flag set edilir, tüm sistem düzenli kapanır.

---

## 14. NOVA SHIELD CHAIN İÇİN DERSLER

### 14.1 Konsensüs Thread Tasarımı
Solana'nın tüm konsensüs mantığını tek thread'e koyması (ReplayStage) hem basitlik sağlar hem de race condition riskini minimize eder. Nova Shield'de de konsensüs karar alma single-threaded olabilir; sadece blockstore I/O ve transaction replay paralel yapılabilir.

**Öneri:** `replay_stage.rs` → `consensus_stage.rs` benzeri bir yapı. Karar alma tek thread, execution paralel.

### 14.2 Fork Choice Ayrımı
Ağırlık hesabı (`compute_bank_stats`) ile fork seçimi (`select_forks`) ve oy kararı (`select_vote_and_reset_forks`) ayrı fonksiyonlarda. Bu ayrım test edilebilirliği artırır. Nova Shield'de de ağırlık hesabı, seçim ve oy mantığı farklı modüllere ayrılmalı.

### 14.3 Switch Threshold Tasarımı
`DUPLICATE_THRESHOLD + SWITCH_FORK_THRESHOLD + DUPLICATE_LIVENESS_THRESHOLD = 1.0` denklemi matematiksel liveness garantisi sağlar. Nova Shield'de de benzer parametre dengesi kurulmalı.

### 14.4 Tower BFT Entegrasyonu
Tower state'ini diskte saklamak (`TowerStorage`) ve her restart'ta yeniden yüklemek, slashing koruma mekanizmasının temelidir. `wait_to_vote_slot` parametresi restart sonrası duplicate vote'u önler. Nova Shield'de MPC threshold signing ile tower güvenliği daha da güçlendirilebilir.

### 14.5 Duplicate Slot Recovery Protocol
Multi-source duplicate tespit (local, ancestor hash, gossip, popular pruned) ve deterministik kurtarma prosedürü sağlam bir tasarım. Nova Shield'de:
- ZK proof ile duplicate kanıtlama (merkle proof yerine)
- Slashing condition olarak kullanılabilir

### 14.6 Propagation Threshold
`SUPERMINORITY_THRESHOLD` (%33.3) ile leader slot'un ağa yayılıp yayılmadığını kontrol etmek, leader'ın kendi bloklarını fork choice'a sürüklememesini önler. Nova Shield'de benzer "self-endorsement prevention" gerekebilir.

### 14.7 Partition Detection ve Telemetri
`PartitionInfo::update()` bölünmeyi Prometheus datasına dönüştürür. Nova Shield'de izleme için benzer datapoint'ler kritik. Bölünme süresi ve o andaki fork durumu loglanmalı.

### 14.8 Exponential Backoff Retransmit
`maybe_retransmit_unpropagated_slots()` exponential backoff ile retransmit yapar. Nova Shield P2P katmanında (libp2p + gossipsub) benzer retry politikası gerekli.

### 14.9 Paralel Fork Replay
`replay_active_banks_concurrently()` Rayon ile paralel replay yapar. Progress map'i `RwLock<&mut ProgressMap>` ile thread-safe yapar. Nova Shield'de Rayon veya Tokio task'ları ile benzer yaklaşım kullanılabilir.

### 14.10 Vote TX Refresh Mekanizması
Blockhash süresi dolmuş vote tx'lerini yenileme mantığı `MAX_VOTE_REFRESH_INTERVAL_MILLIS` ile sınırlı. Bu, ağ tıkanıklığında bile oy mekanizmasının çalışmasını sağlar. Nova Shield'de de offline dönemden sonra vote recovery planı olmalı.

### 14.11 Unsafe Yokluğu
8754 satırlık kritik konsensüs kodu tamamen safe Rust ile yazılmış. Bu, Nova Shield için de güvenli-önce yaklaşımın mümkün olduğunu kanıtlar. Performans gerektiğinde unsafe yerine Rayon ve SIMD kullanılabilir.

### 14.12 Test Kapsamı
100+ test, gerçek bankforks ve blockstore ile entegrasyon testi yapıyor. Nova Shield'de de simüle edilmiş fork ağaçları üzerinde konsensüs senaryoları test edilmeli. `VoteSimulator` benzeri bir yapı çok değerli.

---

## 15. KRİTİK KOD BLOKLARI

### 15.1 Temel Bölünme Hata Durumu — Figure 1 (satır 3590-3623)
```
slot 0
    |
slot 1
/        \
slot 2 (last vote)     |
    |      slot 8 (10%)
slot 4 (9%)
```
Bu senaryoda switch threshold başarısız olursa slot 8'e geçmek yerine slot 4'te bekle. Aksi halde ağ durur.

### 15.2 Tower Senkronizasyon Mantığı (satır 3309-3399)
Bankta onchain vote state bizim local tower'ımızdan daha yeni ise: Onchain state'i al, local root'u koru.

### 15.3 Duplicate Kurtarma Güvenlik Garantisi (satır 1507-1511)
```rust
panic!("We are attempting to dump a block that we produced. \
    This indicates that we are producing duplicate blocks, \
    or that there is a bug in our runtime/replay code...");
```
Kendi ürettiğimiz blokta hash uyumsuzluğu varsa bu bir runtime bug'ıdır, panic ile çıkmak doğru davranış.

### 15.4 Vote Only Mode Tetikleme (satır 2111-2116)
```rust
let vote_only_bank = if root_distance > MAX_ROOT_DISTANCE_FOR_VOTE_ONLY {
    datapoint_info!("vote-only-bank", ("slot", poh_slot, i64));
    true
} else {
    false
};
```
Bu, validator'ın geride kaldığını anlayıp sadece vote tx'lerini işlemesini sağlar.

---

## 16. DOSYA İSTATİSTİKLERİ

| Metrik | Değer |
|--------|-------|
| Toplam satır | 8754 |
| Public struct sayısı | 2 (ReplayStage, ReplayStageConfig) |
| Private struct sayısı | 8 |
| Public enum sayısı | 1 (HeaviestForkFailures) |
| Private enum sayısı | 3 |
| Public fonksiyon sayısı | ~8 |
| Private fonksiyon sayısı | ~39 |
| Test sayısı | ~40+ |
| Unsafe blok sayısı | 0 |
| Import sayısı | ~60+ |
