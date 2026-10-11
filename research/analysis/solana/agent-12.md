# Solana TPU/TVU/Sigverify Analizi — Agent 12

**Analiz Tarihi:** 2026-07-22
**Kapsam:** core/src/tpu.rs, tvu.rs, fetch_stage.rs, sigverify.rs, sigverify_stage.rs, shred_fetch_stage.rs

---

## Genel Mimari Özeti

Bu 6 dosya Solana validator'ının iki temel işlem pipeline'ını oluşturur:

- **TPU (Transaction Processing Unit):** Validator'ın *leader* olduğu slotlarda aktif olur. Dışarıdan gelen ham transaction'ları alır, imzalarını doğrular ve bloğa yazar.
- **TVU (Transaction Validation Unit):** Validator'ın *validator* rolündeyken aktif olur. Başka leader'lardan gelen blok parçalarını (shred) alır, doğrular ve ledger'a kaydeder.

TPU → FetchStage → SigVerifyStage → BankingStage → BroadcastStage
TVU → ShredFetchStage → ShredSigVerify → RetransmitStage → WindowService → ReplayStage

---

## 1. `tpu.rs` — Transaction Processing Unit

**Dosya:** `core/src/tpu.rs`
**Toplam Satır:** 314
**Rol:** Leader slot'unda gelen transaction'ları toplar, imzaları doğrular, bankaya işler ve bloğu ağa yayar.

### 1.1 Sabitler

```
Satır 53: pub const MAX_QUIC_CONNECTIONS_PER_PEER: usize = 8;
```
- Bir peer'dan aynı anda açık olabilecek maksimum QUIC bağlantı sayısı.
- Tasarım kararı: NAT geçişi ve bağlantı açma/kapama örtüşmesine tolerans sağlamak için 1'den fazla tutulmuş.
- `pub` olduğu için dışarıdan erişilebilir; diğer modüller bu limiti referans alabilir.

### 1.2 Struct: `TpuSockets`

```rust
// Satır 55-62
pub struct TpuSockets {
    pub transactions: Vec<UdpSocket>,          // Ana TPU UDP soketi
    pub transaction_forwards: Vec<UdpSocket>,  // Forward (yönlendirme) UDP soketi
    pub vote: Vec<UdpSocket>,                  // Oy transaction'ları için UDP soketi
    pub broadcast: Vec<UdpSocket>,             // Blok yayını için UDP soketi
    pub transactions_quic: UdpSocket,          // QUIC protokolü ana soket
    pub transactions_forwards_quic: UdpSocket, // QUIC protokolü forward soket
}
```

- `Vec<UdpSocket>` alanları: Her soket türü için paralel birden fazla soket açılabilir. Bu sayede yüksek throughput sağlanır; farklı core'lara pinlenmiş receiver thread'leri ayrı soketleri tüketir.
- `transactions_quic` ve `transactions_forwards_quic`: Tekli soket, QUIC protokolü için. QUIC, tek soket üzerinden çoklu bağlantı yönetir (multiplexing).
- Derive edilmemiş — sadece veri taşıyıcı, kopyalanabilir değil (UdpSocket Clone implement etmez).

### 1.3 Struct: `Tpu`

```rust
// Satır 64-76
pub struct Tpu {
    fetch_stage: FetchStage,                          // UDP'den packet alma
    sigverify_stage: SigVerifyStage,                  // Normal tx imza doğrulama
    vote_sigverify_stage: SigVerifyStage,             // Vote tx imza doğrulama
    banking_stage: BankingStage,                      // Bank işleme
    cluster_info_vote_listener: ClusterInfoVoteListener, // Gossip oy dinleyicisi
    broadcast_stage: BroadcastStage,                  // Blok yayını
    tpu_quic_t: thread::JoinHandle<()>,               // QUIC server thread
    tpu_forwards_quic_t: thread::JoinHandle<()>,      // QUIC forward server thread
    tpu_entry_notifier: Option<TpuEntryNotifier>,     // Entry bildirimi (opsiyonel)
    staked_nodes_updater_service: StakedNodesUpdaterService, // Stake güncelleme
    tracer_thread_hdl: TracerThread,                  // Banking tracer thread
}
```

- Her field bir pipeline stage'i temsil eder.
- `tpu_entry_notifier: Option<TpuEntryNotifier>` — Sadece dışarıya entry bildirimi gerektiğinde (örn. plugin sistemi) aktif olur; yoksa `None`.
- `tracer_thread_hdl: TracerThread` — `TracerThread = Option<JoinHandle<Result<(), ...>>>` tipinde; tracing disabled ise `None`.
- Struct `pub` ama field'lar `pub(crate)` de değil, tamamen private. Dış erişim sadece `new()` ve `join()` metodları üzerinden.

### 1.4 `impl Tpu`

#### `Tpu::new()` — Satır 80-283

**İmza:**
```rust
pub fn new(
    cluster_info: &Arc<ClusterInfo>,
    poh_recorder: &Arc<RwLock<PohRecorder>>,
    entry_receiver: Receiver<WorkingBankEntry>,
    retransmit_slots_receiver: Receiver<Slot>,
    sockets: TpuSockets,
    subscriptions: &Arc<RpcSubscriptions>,
    transaction_status_sender: Option<TransactionStatusSender>,
    entry_notification_sender: Option<EntryNotifierSender>,
    blockstore: Arc<Blockstore>,
    broadcast_type: &BroadcastStageType,
    exit: Arc<AtomicBool>,
    shred_version: u16,
    vote_tracker: Arc<VoteTracker>,
    bank_forks: Arc<RwLock<BankForks>>,
    verified_vote_sender: VerifiedVoteSender,
    gossip_verified_vote_hash_sender: GossipVerifiedVoteHashSender,
    replay_vote_receiver: ReplayVoteReceiver,
    replay_vote_sender: ReplayVoteSender,
    bank_notification_sender: Option<BankNotificationSender>,
    tpu_coalesce: Duration,
    duplicate_confirmed_slot_sender: DuplicateConfirmedSlotsSender,
    connection_cache: &Arc<ConnectionCache>,
    turbine_quic_endpoint_sender: AsyncSender<(SocketAddr, Bytes)>,
    keypair: &Keypair,
    log_messages_bytes_limit: Option<usize>,
    staked_nodes: &Arc<RwLock<StakedNodes>>,
    shared_staked_nodes_overrides: Arc<RwLock<HashMap<Pubkey, u64>>>,
    banking_tracer: Arc<BankingTracer>,
    tracer_thread_hdl: TracerThread,
    tpu_enable_udp: bool,
    prioritization_fee_cache: &Arc<PrioritizationFeeCache>,
    block_production_method: BlockProductionMethod,
    _generator_config: Option<GeneratorConfig>,
) -> (Self, Vec<Arc<dyn NotifyKeyUpdate + Sync + Send>>)
```

**Argüman sayısı:** 33 — `#[allow(clippy::too_many_arguments)]` ile lint susturulmuş.
**Dönüş:** `(Self, Vec<Arc<dyn NotifyKeyUpdate + Sync + Send>>)` — TPU struct'ı ve QUIC key güncelleme notifier'ları.

**Pipeline Kurulum Sırası (satır satır):**

1. **Satır 115-122:** `TpuSockets` destructure edilir.

2. **Satır 124-126:** 3 adet unbounded channel oluşturulur:
   - `(packet_sender, packet_receiver)` — normal tx için
   - `(vote_packet_sender, vote_packet_receiver)` — vote tx için
   - `(forwarded_packet_sender, forwarded_packet_receiver)` — forward tx için

3. **Satır 127-140: FetchStage kurulumu**
   - `FetchStage::new_with_sender(...)` çağrısı
   - Tüm UDP soketleri ve 3 channel sender'ı iletilir
   - `tpu_coalesce` süresi: paketlerin toplanma süresi (batch oluşumu)
   - `bank_forks.read().unwrap().get_vote_only_mode_signal()` — vote-only mode sinyali

4. **Satır 142-147: StakedNodesUpdaterService kurulumu**
   - `bank_forks` ve `staked_nodes` bağlanır
   - Stake bilgisini periyodik günceller

5. **Satır 149: BankingTracer non-vote channel**
   - `banking_tracer.create_channel_non_vote()` → `(non_vote_sender, non_vote_receiver)`
   - Tracer: transaction akışını kayıt altına alır, replay için

6. **Satır 151-169: TPU QUIC Server (ana transaction)**
   - `spawn_server("solQuicTpu", ...)` çağrısı
   - `MAX_QUIC_CONNECTIONS_PER_PEER = 8` — peer başına bağlantı limiti
   - `MAX_STAKED_CONNECTIONS` — stake'li düğümler için bağlantı havuzu
   - `MAX_UNSTAKED_CONNECTIONS` — stake'siz düğümler için bağlantı havuzu
   - `DEFAULT_WAIT_FOR_CHUNK_TIMEOUT` — QUIC chunk bekleme süresi
   - Döner: `SpawnServerResult { endpoint, thread: tpu_quic_t, key_updater }`

7. **Satır 171-189: TPU Forwards QUIC Server**
   - `spawn_server("solQuicTpuFwd", ...)` çağrısı
   - **Kritik fark:** `MAX_UNSTAKED_CONNECTIONS = 0` — forward sokette stake'siz düğümler engellenmiş!
     ```rust
     MAX_STAKED_CONNECTIONS.saturating_add(MAX_UNSTAKED_CONNECTIONS),
     0, // Prevent unstaked nodes from forwarding transactions
     ```
   - Bu tasarım kararı: Forward yolunu spam'den korur; sadece stake'li node'lar forward yapabilir.

8. **Satır 191-194: SigVerifyStage (normal tx)**
   - `TransactionSigVerifier::new(non_vote_sender)` — non-vote tx verifier
   - `SigVerifyStage::new(packet_receiver, verifier, "solSigVerTpu", "tpu-verifier")`
   - İsimler thread adı ve metrics etiketi olarak kullanılır

9. **Satır 196-206: Vote SigVerifyStage**
   - `TransactionSigVerifier::new_reject_non_vote(tpu_vote_sender)` — vote-only verifier
   - `SigVerifyStage::new(vote_packet_receiver, verifier, "solSigVerTpuVot", "tpu-vote-verifier")`
   - `reject_non_vote = true` flag'i aktif — vote olmayan tx'ler reddedilir

10. **Satır 208-224: ClusterInfoVoteListener**
    - Gossip ağından gelen oyları dinler
    - `banking_tracer.create_channel_gossip_vote()` → gossip vote channel

11. **Satır 226-239: BankingStage**
    - 3 channel receiver'ı alır: `non_vote_receiver`, `tpu_vote_receiver`, `gossip_vote_receiver`
    - Transaction'ları PoH ile sıralayıp Bank'a işler

12. **Satır 241-253: TpuEntryNotifier (opsiyonel)**
    ```rust
    let (entry_receiver, tpu_entry_notifier) =
        if let Some(entry_notification_sender) = entry_notification_sender {
            let (broadcast_entry_sender, broadcast_entry_receiver) = unbounded();
            let tpu_entry_notifier = TpuEntryNotifier::new(...);
            (broadcast_entry_receiver, Some(tpu_entry_notifier))
        } else {
            (entry_receiver, None)
        };
    ```
    - Geyser plugin veya benzeri sistemler için entry bildirimi
    - Eğer `entry_notification_sender` yoksa, entry_receiver doğrudan broadcast'e gider

13. **Satır 255-265: BroadcastStage**
    - Üretilen blok shred'lerini ağa yayar
    - `broadcast_type.new_broadcast_stage(...)` — strateji pattern

**Dönüş değeri (satır 267-282):**
```rust
(
    Self { fetch_stage, sigverify_stage, vote_sigverify_stage, banking_stage,
           cluster_info_vote_listener, broadcast_stage, tpu_quic_t,
           tpu_forwards_quic_t, tpu_entry_notifier, staked_nodes_updater_service,
           tracer_thread_hdl },
    vec![key_updater, forwards_key_updater],
)
```
- key_updater'lar: TLS sertifikası yenilendiğinde QUIC server'ları bilgilendirmek için

#### `Tpu::join()` — Satır 285-313

```rust
pub fn join(self) -> thread::Result<()>
```

- `self` consume eder (ownership alır), tüm thread'leri join eder
- Join sırası önemli: broadcast_stage ayrı join edilir, sonucu saklanır
- `tpu_entry_notifier` opsiyonel join
- `tracer_thread_hdl`: İç içe `join()` ve `?` — thread join başarılı ama içerideki Result hata içeriyorsa `error!()` log'lanır, panic oluşturmaz
- Broadcast sonucu en sonda kontrol edilir: `let _ = broadcast_result?`

### 1.5 TPU Mimari Özeti

```
UDP/QUIC geliri
    │
    ▼
[FetchStage] ─────────────────────────────────────────
    │                │                    │
(normal tx)    (vote tx)           (forward tx)
    │                │                    │
    ▼                ▼                    │
[SigVerifyStage] [VoteSigVerifyStage]    │
    │                │                    │
    ▼                ▼                    │
[BankingStage] ◄── (gossip votes)        │
    │                                     │
    ▼                               [handle_forwarded_packets]
[PoH Recorder]                           │
    │                               (forward sadece leader yakınsa)
    ▼
[BroadcastStage] → ağa shred'ler
```

**Tasarım Kararları:**
1. Vote tx'leri ayrı sokette, ayrı sigverify'da — öncelikli işlem
2. Forward tx'leri unstaked node'lardan bloke — spam koruması
3. BankingTracer channel'ları — prod'da replay/debug için
4. `_generator_config: Option<GeneratorConfig>` — vestigial code, eski replay invalidator için, kullanılmıyor

---

## 2. `tvu.rs` — Transaction Validation Unit

**Dosya:** `core/src/tvu.rs`
**Toplam Satır:** 511 (test kodu dahil)
**Rol:** Validator rolündeyken başka leader'lardan gelen shred'leri alır, doğrular, ledger'a yazar ve ağda retransmit eder.

### 2.1 Struct: `Tvu`

```rust
// Satır 62-75
pub struct Tvu {
    fetch_stage: ShredFetchStage,             // Shred alma (UDP + QUIC)
    shred_sigverify: JoinHandle<()>,          // Shred imza doğrulama thread'i
    retransmit_stage: RetransmitStage,        // Turbine retransmit
    window_service: WindowService,            // Shred depolama ve repair
    cluster_slots_service: ClusterSlotsService, // Cluster slot bilgisi
    replay_stage: ReplayStage,               // Bank replay
    blockstore_cleanup_service: Option<BlockstoreCleanupService>, // Ledger temizleme
    cost_update_service: CostUpdateService,  // İşlem maliyet güncelleme
    voting_service: VotingService,           // Oy gönderme
    warm_quic_cache_service: Option<WarmQuicCacheService>, // QUIC bağlantı ısıtma
    drop_bank_service: DropBankService,      // Bank düşürme (bellek yönetimi)
    duplicate_shred_listener: DuplicateShredListener, // Duplikat shred dinleyici
}
```

**TPU ile karşılaştırma:**
- TPU: `FetchStage` (transaction) → TVU: `ShredFetchStage` (shred)
- TPU: `SigVerifyStage` (Ed25519 batch) → TVU: `shred_sigverify` (Merkle/Ed25519 shred imzası)
- TPU: `BankingStage` → TVU: `ReplayStage` (lider'ın yaptığını replay eder)
- TVU'da `RetransmitStage` var, TPU'da yok — TVU Turbine'i yönetir

### 2.2 Struct: `TvuSockets`

```rust
// Satır 77-82
pub struct TvuSockets {
    pub fetch: Vec<UdpSocket>,               // Turbine shred alma
    pub repair: UdpSocket,                   // Repair request/response
    pub retransmit: Vec<UdpSocket>,          // Turbine retransmit
    pub ancestor_hashes_requests: UdpSocket, // Ancestor hash sorguları
}
```

**TPU vs TVU soket farkı:**
- TPU: `transactions`, `transaction_forwards`, `vote`, `broadcast` — gelen tx ve giden blok
- TVU: `fetch`, `repair`, `retransmit`, `ancestor_hashes` — gelen blok ve ağ bakımı

### 2.3 Struct: `TvuConfig`

```rust
// Satır 84-94
#[derive(Default)]
pub struct TvuConfig {
    pub max_ledger_shreds: Option<u64>,                    // Ledger boyut limiti
    pub shred_version: u16,                                // Ağ shred versiyonu
    pub repair_validators: Option<HashSet<Pubkey>>,        // Repair talep edilecek node'lar
    pub repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>,    // Repair öncelikli node'lar
    pub wait_for_vote_to_start_leader: bool,               // Leader öncesi oy bekleme
    pub replay_slots_concurrently: bool,                   // Paralel slot replay
}
```

- `#[derive(Default)]` — `max_ledger_shreds: None`, `repair_validators: None` gibi makul default'lar
- `repair_whitelist: Arc<RwLock<...>>` — runtime'da güncellenebilir whitelist (thread-safe)
- `repair_validators`: Sadece belirli validator'lardan repair isteme (test veya özel güven)
- `replay_slots_concurrently`: True ise birden fazla slot aynı anda replay edilir — throughput vs bellek dengesi

### 2.4 `impl Tvu`

#### `Tvu::new()` — Satır 103-352

**İmza:** 47 parametre (record kıran bir fonksiyon), `Result<Self, String>` döner.

**Pipeline Kurulum Sırası:**

1. **Satır 148-153:** `TvuSockets` destructure.

2. **Satır 155-173: ShredFetchStage kurulumu**
   ```rust
   let (fetch_sender, fetch_receiver) = unbounded();
   let repair_socket = Arc::new(repair_socket);
   let ancestor_hashes_socket = Arc::new(ancestor_hashes_socket);
   let fetch_sockets: Vec<Arc<UdpSocket>> = fetch_sockets.into_iter().map(Arc::new).collect();
   let (repair_quic_endpoint_response_sender, repair_quic_endpoint_response_receiver) = unbounded();
   let fetch_stage = ShredFetchStage::new(
       fetch_sockets,
       turbine_quic_endpoint_receiver,
       repair_socket.clone(),
       repair_quic_endpoint_response_receiver,
       fetch_sender,
       tvu_config.shred_version,
       bank_forks.clone(),
       cluster_info.clone(),
       turbine_disabled,
       exit.clone(),
   );
   ```
   - `repair_socket` Arc ile sarılır — repair ve window_service paylaşır
   - `turbine_quic_endpoint_receiver` — QUIC üzerinden gelen turbine shred'leri
   - `repair_quic_endpoint_response_receiver` — QUIC repair yanıtları

3. **Satır 175-184: Shred SigVerify**
   ```rust
   let (verified_sender, verified_receiver) = unbounded();
   let (retransmit_sender, retransmit_receiver) = unbounded();
   let shred_sigverify = solana_turbine::sigverify_shreds::spawn_shred_sigverify(
       cluster_info.clone(),
       bank_forks.clone(),
       leader_schedule_cache.clone(),
       fetch_receiver,
       retransmit_sender.clone(),
       verified_sender,
   );
   ```
   - `fetch_receiver` → verified_sender: shred doğrulanmış → verified_receiver
   - `retransmit_sender`: Doğrulama sürerken shred'ler retransmit için de gönderilir
   - Şred imzası: Turbine'de Merkle tree tabanlı; leader_schedule_cache ile leader public key doğrulanır

4. **Satır 186-195: RetransmitStage**
   - Turbine protokolüne göre diğer validator'lara shred yayar
   - `max_slots` ile slot limitlenir

5. **Satır 197-239: WindowService**
   - Gelen shred'leri slot pencerelerine yerleştirir
   - Eksik shred'ler için repair talep eder
   - `ancestor_hashes_socket`: Fork seçiminde ancestor shred hash'leri için
   - `repair_quic_endpoint_sender` + `repair_quic_endpoint_response_sender`: QUIC repair kanalları
   - `duplicate_slots_sender`: Duplikat slot tespiti

6. **Satır 241-249: ClusterSlotsService**
   - Cluster'daki slot durumlarını izler
   - Repair kararlarına girdi sağlar

7. **Satır 251-269: ReplayStageConfig**
   - Büyük config struct dolduruluyor
   - `wait_for_vote_to_start_leader`, `replay_slots_concurrently` aktarılıyor

8. **Satır 271-277: VotingService**
   - `voting_receiver` channel üzerinden gelen oyları gönderir
   - `tower_storage` ile vote tower persist edilir

9. **Satır 279-288: WarmQuicCacheService (opsiyonel)**
   ```rust
   let warm_quic_cache_service = if connection_cache.use_quic() {
       Some(WarmQuicCacheService::new(...))
   } else {
       None
   };
   ```
   - QUIC kullanılıyorsa: Lider rotasyonundan önce bağlantıları önceden ısıtır
   - UDP kullanılıyorsa: `None`

10. **Satır 289-294: CostUpdateService ve DropBankService**
    - `CostUpdateService`: İşlem maliyet modelini günceller
    - `DropBankService`: Kullanılmayan Bank nesnelerini asenkron düşürür (bellek sızıntısı önlemi)

11. **Satır 296-322: ReplayStage**
    - En karmaşık stage; `?` ile hata döner
    - Tüm duplikat/consensus/gossip channel'larını alır

12. **Satır 324-326: BlockstoreCleanupService (opsiyonel)**
    ```rust
    let blockstore_cleanup_service = tvu_config.max_ledger_shreds.map(|max_ledger_shreds| {
        BlockstoreCleanupService::new(blockstore.clone(), max_ledger_shreds, exit.clone())
    });
    ```
    - Sadece `max_ledger_shreds` belirtilmişse aktif

13. **Satır 328-337: DuplicateShredListener**
    - Gossip'ten duplicate shred kanıtlarını dinler
    - `DuplicateShredHandler` ile işler → `duplicate_slots_sender`

#### `Tvu::join()` — Satır 355-373

```rust
pub fn join(self) -> thread::Result<()>
```

Join sırası:
1. `retransmit_stage.join()`
2. `window_service.join()`
3. `cluster_slots_service.join()`
4. `fetch_stage.join()`
5. `shred_sigverify.join()`
6. `blockstore_cleanup_service` (opsiyonel, `.is_some()` kontrolü ile)
7. `replay_stage.join()`
8. `cost_update_service.join()`
9. `voting_service.join()`
10. `warm_quic_cache_service` (opsiyonel)
11. `drop_bank_service.join()`
12. `duplicate_shred_listener.join()`

Not: `blockstore_cleanup_service` için `is_some()` + `unwrap()` kullanılmış. `if let Some(s) = self.blockstore_cleanup_service { s.join()?; }` daha idiomatik Rust olurdu — bu bir kod kalite sorunudur.

### 2.5 Test: `test_tvu_exit()` — Satır 397-510

```rust
#[ignore]
#[test]
#[serial]
fn test_tvu_exit()
```

- `#[ignore]`: Normalde çalıştırılmaz, yalnızca explicit `--ignored` flag'i ile.
- `#[serial]`: `serial_test` crate'i ile; tek seferde bir test çalışır (soket çakışması önlemi).
- Test akışı:
  1. Genesis config ve bank oluşturulur
  2. Geçici blockstore açılır
  3. Boş kanallar bağlanır (None/empty)
  4. `Tvu::new(...)` çağrısı, `.expect("assume success")`
  5. `exit.store(true, Ordering::Relaxed)` ile kapatma sinyali
  6. `tvu.join().unwrap()`
  7. `poh_service.join().unwrap()`
- Amaç: TVU'nun temiz kapatılmasını test etmek

### 2.6 TPU vs TVU Karşılaştırması

| Özellik | TPU | TVU |
|---------|-----|-----|
| Rol | Leader: tx üretir | Validator: blok doğrular |
| Giriş | Transaction (UDP/QUIC) | Shred (UDP/QUIC/Repair) |
| Çıkış | Blok shred'leri (broadcast) | Ledger (blockstore) |
| İmza doğrulama | Ed25519 batch (CPU/GPU) | Merkle shred sigverify |
| Veri birimi | Transaction batch | PacketBatch (shred) |
| Hata dönüşü | `(Self, ...)` panic yok | `Result<Self, String>` |
| Repair | Yok | WindowService üzerinden |
| Retransmit | Yok | RetransmitStage (Turbine) |
| Vote handling | ClusterInfoVoteListener | VotingService (gönderme) |

---

## 3. `fetch_stage.rs` — Transaction Fetch Stage

**Dosya:** `core/src/fetch_stage.rs`
**Toplam Satır:** 277
**Rol:** UDP soketlerinden transaction paketlerini alır, 3 kanala dağıtır.

### 3.1 Struct: `FetchStage`

```rust
// Satır 28-30
pub struct FetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}
```

- Tüm thread handle'ları tek Vec'te tutulur
- `thread_hdls` private → sadece `join()` ile erişilir
- Basit wrapper pattern: Stage = thread koleksiyonu

### 3.2 `impl FetchStage`

#### `FetchStage::new()` — Satır 33-62

```rust
pub fn new(
    sockets: Vec<UdpSocket>,
    tpu_forwards_sockets: Vec<UdpSocket>,
    tpu_vote_sockets: Vec<UdpSocket>,
    exit: Arc<AtomicBool>,
    poh_recorder: &Arc<RwLock<PohRecorder>>,
    coalesce: Duration,
) -> (Self, PacketBatchReceiver, PacketBatchReceiver)
```

- 3 adet unbounded channel oluşturur
- `new_with_sender()` delegasyonu yapar
- `DEFAULT_TPU_ENABLE_UDP = true` ile UDP varsayılan açık
- Dönüş: `(Self, receiver, vote_receiver)` — forward receiver dışarıya verilmez, iç olarak bağlanır

#### `FetchStage::new_with_sender()` — Satır 64-96

```rust
pub fn new_with_sender(
    sockets: Vec<UdpSocket>,
    tpu_forwards_sockets: Vec<UdpSocket>,
    tpu_vote_sockets: Vec<UdpSocket>,
    exit: Arc<AtomicBool>,
    sender: &PacketBatchSender,
    vote_sender: &PacketBatchSender,
    forward_sender: &PacketBatchSender,
    forward_receiver: PacketBatchReceiver,
    poh_recorder: &Arc<RwLock<PohRecorder>>,
    coalesce: Duration,
    in_vote_only_mode: Option<Arc<AtomicBool>>,
    tpu_enable_udp: bool,
) -> Self
```

- `tpu.rs`'den çağrılan ana constructor
- Soketleri `Arc<UdpSocket>`'e dönüştürür (paylaşılabilir referans)
- `in_vote_only_mode`: Aktifken sadece vote transaction'ları kabul edilir
- `tpu_enable_udp`: UDP devre dışı bırakılabilir (sadece QUIC modunda çalışma)

#### `FetchStage::handle_forwarded_packets()` — Satır 98-138

```rust
fn handle_forwarded_packets(
    recvr: &PacketBatchReceiver,
    sendr: &PacketBatchSender,
    poh_recorder: &Arc<RwLock<PohRecorder>>,
) -> Result<()>
```

**Algoritma (kritik karar noktası):**

```rust
// Satır 103-105
let mark_forwarded = |packet: &mut Packet| {
    packet.meta_mut().flags |= PacketFlags::FORWARDED;
};
```

1. `recvr.recv()` — blocking receive (ilk batch)
2. `while let Ok(batch) = recvr.try_recv()` — non-blocking drain
3. **1024 paket limiti** (satır 116): `if num_packets > 1024 { break; }` — tek döngüde en fazla ~1K paket
4. **PoH recorder kontrolü** (satır 121-124):
   ```rust
   if poh_recorder.read().unwrap().would_be_leader(
       HOLD_TRANSACTIONS_SLOT_OFFSET.saturating_mul(DEFAULT_TICKS_PER_SLOT)
   )
   ```
   - `HOLD_TRANSACTIONS_SLOT_OFFSET` slot ilerisi için lider olacak mı? → forward paketleri kabul et
   - Değilse: `inc_new_counter_info!("fetch_stage-discard_forwards", ...)` ile sil
5. Forward onaylandıysa: tüm batch'ler `sendr.send()` ile normal tx kanalına gönderilir

**Tasarım kararı:** Forward paketleri sadece node yakında lider olacaksa işlenir. Bu, gecikmiş forward'ların gereksiz yere banking stage'i doldurmak yerine düşürülmesini sağlar.

#### `FetchStage::new_multi_socket()` — Satır 140-269

**Parametreler:** 12 parametre, `#[allow(clippy::too_many_arguments)]`

**Thread organizasyonu:**

1. **TPU receiver thread'leri** (satır 159-179):
   - `if tpu_enable_udp { ... } else { Vec::default() }`
   - Her soket için ayrı `streamer::receiver()` thread'i: `"solRcvrTpu{i:02}"`
   - `recycler.clone()` — PacketBatch memory pool paylaşımı
   - `in_vote_only_mode.clone()` — vote-only sinyal paylaşımı

2. **Forward receiver thread'leri** (satır 181-202):
   - Aynı pattern, soket: `"solRcvrTpuFwd{i:02}"`
   - UDP devre dışıysa boş Vec

3. **Vote receiver thread'leri** (satır 204-221):
   - `in_vote_only_mode: None` — vote thread her zaman çalışır, mode sinyaline bağlı değil
   - Soket: `"solRcvrTpuVot{i:02}"`

4. **Forward handler thread** (satır 226-241): `"solFetchStgFwRx"`
   - `loop { Self::handle_forwarded_packets(...) }` döngüsü
   - Hata durumları: `Disconnected`, `Send` → `break`; `Timeout` → devam

5. **Metrics thread** (satır 243-256): `"solFetchStgMetr"`
   - Her 1 saniyede `tpu_stats.report()`, `tpu_vote_stats.report()`, `tpu_forward_stats.report()`
   - `exit.load(Ordering::Relaxed)` ile kapatma kontrolü

**Tüm thread'lerin birleştirilmesi** (satır 258-268):
```rust
Self {
    thread_hdls: [
        tpu_threads,
        tpu_forwards_threads,
        tpu_vote_threads,
        vec![fwd_thread_hdl, metrics_thread_hdl],
    ]
    .into_iter()
    .flatten()
    .collect(),
}
```

#### `FetchStage::join()` — Satır 271-276

```rust
pub fn join(self) -> thread::Result<()> {
    for thread_hdl in self.thread_hdls {
        thread_hdl.join()?;
    }
    Ok(())
}
```

### 3.3 Recycler Kullanımı

```rust
// Satır 155
let recycler: PacketBatchRecycler = Recycler::warmed(1000, 1024);
```

- `warmed(1000, 1024)`: 1000 adet, her biri 1024 kapasiteli PacketBatch önceden allocate edilir
- Heap allocation yerine pool'dan alınır → GC baskısı olmaz, allocation latency azalır
- Tüm receiver thread'leri bu pool'u paylaşır (`recycler.clone()` — Arc clone)

### 3.4 Tasarım Kararları

1. **Üç ayrı soket tipi:** Normal tx, vote tx ve forward tx birbirinden bağımsız — vote'lar düşük gecikme garantisi alır
2. **UDP opsiyonel:** `tpu_enable_udp` ile UDP tamamen kapatılabilir, QUIC-only mod
3. **Forward karar noktası PoH'ta:** Forward ne zaman kabul edilir sorusu protokol katmanında değil, PoH'ta çözülür
4. **Metrics ayrı thread'de:** I/O-heavy metrics reporting asıl işi bloklamaz

---

## 4. `sigverify.rs` — İmza Doğrulama

**Dosya:** `core/src/sigverify.rs`
**Toplam Satır:** 151
**Rol:** `SigVerifier` trait'ini `TransactionSigVerifier` ile implemente eder; CPU/GPU imza doğrulamasını `solana_perf::sigverify`'a devreder.

### 4.1 Re-export'lar

```rust
// Satır 7-9
pub use solana_perf::sigverify::{
    count_packets_in_batches, ed25519_verify_cpu, ed25519_verify_disabled, init, TxOffset,
};
```

- Bu dosya büyük oranda `solana_perf::sigverify` modülünün üstünde ince bir katman
- `ed25519_verify_cpu`: CPU'da Ed25519 batch doğrulama
- `ed25519_verify_disabled`: Doğrulamayı atla (test/geliştirme için)
- `init()`: GPU sigverify başlatma (CUDA hazırlığı)
- `TxOffset`: GPU'da transaction offset'lerini tutan yapı

### 4.2 Struct: `SigverifyTracerPacketStats`

```rust
// Satır 19-26
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, AbiExample)]
pub struct SigverifyTracerPacketStats {
    pub total_removed_before_sigverify_stage: usize,  // Sigverify öncesi atılanlar
    pub total_tracer_packets_received_in_sigverify_stage: usize, // Sigverify'a ulaşanlar
    pub total_tracer_packets_deduped: usize,           // Duplikat tespiti ile kaldırılanlar
    pub total_excess_tracer_packets: usize,            // Limit aşımı ile atılanlar
    pub total_tracker_packets_passed_sigverify: usize, // İmzası geçerli tracer paketler
}
```

**Derive analizi:**
- `Debug`: Log'lama için
- `Default`: `0` değerleri ile başlatma
- `Clone`: Kopyalama (aggregation için gerekli)
- `PartialEq + Eq`: `is_default()` metodu için karşılaştırma
- `Serialize + Deserialize`: BankingTracer kayıtları için
- `AbiExample`: Solana ABI uyumluluk testi

Bu struct sadece "tracer packet" sayacı — banking tracing sisteminin debug altyapısı.

#### `SigverifyTracerPacketStats::is_default()` — Satır 29-31

```rust
pub fn is_default(&self) -> bool {
    *self == SigverifyTracerPacketStats::default()
}
```

- Özel bir kontrol: Tüm alanlar 0 mı? → Gereksiz send'i atla

#### `SigverifyTracerPacketStats::aggregate()` — Satır 33-54

```rust
pub fn aggregate(&mut self, other: &SigverifyTracerPacketStats) {
    saturating_add_assign!(self.total_removed_before_sigverify_stage, other.total_removed_before_sigverify_stage);
    // ... her alan için
}
```

- `saturating_add_assign!` macro: Overflow'da panic yerine u64::MAX'ta durur
- Bu macro `solana_sdk::saturating_add_assign!`'dan gelir

### 4.3 Struct: `TransactionSigVerifier`

```rust
// Satır 57-63
pub struct TransactionSigVerifier {
    packet_sender: BankingPacketSender,          // Doğrulanmış tx'leri banking'e gönderir
    tracer_packet_stats: SigverifyTracerPacketStats, // İstatistik toplayıcı
    recycler: Recycler<TxOffset>,                // GPU offset buffer pool
    recycler_out: Recycler<PinnedVec<u8>>,       // GPU çıktı buffer pool
    reject_non_vote: bool,                       // true ise vote olmayan tx'ler reddedilir
}
```

- `recycler: Recycler<TxOffset>`: GPU'ya gönderilecek offset bilgileri için bellek havuzu
- `recycler_out: Recycler<PinnedVec<u8>>`: GPU sonuçları için pinned (sayfa kilitli) bellek — DMA için zorunlu
- `PinnedVec<u8>`: CPU-GPU veri transferinde kullanılan page-locked bellek. Kernel bu belleği RAM'den swap etmez, DMA doğrudan erişir.

#### `TransactionSigVerifier::new()` — Satır 72-81

```rust
pub fn new(packet_sender: BankingPacketSender) -> Self {
    init();  // GPU sigverify başlatma
    Self {
        packet_sender,
        tracer_packet_stats: SigverifyTracerPacketStats::default(),
        recycler: Recycler::warmed(50, 4096),
        recycler_out: Recycler::warmed(50, 4096),
        reject_non_vote: false,
    }
}
```

- `init()`: GPU path için CUDA başlatma. GPU mevcut değilse no-op.
- `Recycler::warmed(50, 4096)`: 50 adet 4096 kapasiteli buffer havuzu

#### `TransactionSigVerifier::new_reject_non_vote()` — Satır 66-70

```rust
pub fn new_reject_non_vote(packet_sender: BankingPacketSender) -> Self {
    let mut new_self = Self::new(packet_sender);
    new_self.reject_non_vote = true;
    new_self
}
```

- Vote SigVerifyStage için kullanılır — sadece vote tx'ler geçer

### 4.4 `impl SigVerifier for TransactionSigVerifier`

#### `process_received_packet()` — Satır 87-107

```rust
#[inline(always)]
fn process_received_packet(
    &mut self,
    packet: &mut Packet,
    removed_before_sigverify_stage: bool,
    is_dup: bool,
) {
    sigverify::check_for_tracer_packet(packet);
    if packet.meta().is_tracer_packet() {
        if removed_before_sigverify_stage {
            self.tracer_packet_stats.total_removed_before_sigverify_stage += 1;
        } else {
            self.tracer_packet_stats.total_tracer_packets_received_in_sigverify_stage += 1;
            if is_dup {
                self.tracer_packet_stats.total_tracer_packets_deduped += 1;
            }
        }
    }
}
```

- `#[inline(always)]`: Hot path — her paket için çağrılır, inline zorunlu
- `sigverify::check_for_tracer_packet(packet)`: Paketin tracer olup olmadığını işaretler (meta flag'i set eder)
- Sayaçlar: İstatistik toplama; gerçek doğrulama yapmaz

#### `process_excess_packet()` — Satır 109-113

```rust
#[inline(always)]
fn process_excess_packet(&mut self, packet: &Packet) {
    if packet.meta().is_tracer_packet() {
        self.tracer_packet_stats.total_excess_tracer_packets += 1;
    }
}
```

- Limit aşımı nedeniyle atılan paketler için sayaç

#### `process_passed_sigverify_packet()` — Satır 115-122

```rust
#[inline(always)]
fn process_passed_sigverify_packet(&mut self, packet: &Packet) {
    if packet.meta().is_tracer_packet() {
        self.tracer_packet_stats.total_tracker_packets_passed_sigverify += 1;
    }
}
```

#### `send_packets()` — Satır 124-134

```rust
fn send_packets(
    &mut self,
    packet_batches: Vec<PacketBatch>,
) -> Result<(), SigVerifyServiceError<Self::SendType>> {
    let tracer_packet_stats_to_send = std::mem::take(&mut self.tracer_packet_stats);
    self.packet_sender.send(BankingPacketBatch::new((
        packet_batches,
        Some(tracer_packet_stats_to_send),
    )))?;
    Ok(())
}
```

- `std::mem::take(&mut self.tracer_packet_stats)`: stats'ı al, yerine `Default::default()` koy
- Stats her send döngüsünde sıfırlanır — "batch stats" semantiği
- `BankingPacketBatch`: `(Vec<PacketBatch>, Option<SigverifyTracerPacketStats>)` tuple wrapper

#### `verify_batches()` — Satır 136-149

```rust
fn verify_batches(
    &self,
    mut batches: Vec<PacketBatch>,
    valid_packets: usize,
) -> Vec<PacketBatch> {
    sigverify::ed25519_verify(
        &mut batches,
        &self.recycler,
        &self.recycler_out,
        self.reject_non_vote,
        valid_packets,
    );
    batches
}
```

- `sigverify::ed25519_verify()`: GPU varsa GPU, yoksa CPU'da batch Ed25519 doğrulama
- `valid_packets` hint: GPU'ya kaç paketin geçerli sayılacağı bilgisi — batch boyutunu optimize eder
- `reject_non_vote`: GPU/CPU sigverify'a iletilir; vote dışı tx'lerin imzası kontrol edilmez/reddedilir
- `batches` döndürülür — in-place işlem: geçersiz paketler `meta.discard = true` ile işaretlenir

### 4.5 GPU vs CPU İmza Doğrulama

```
GPU Path (perf-libs mevcut):
  ed25519_verify() → CUDA kernel → batch Ed25519 doğrulama
  - 10.000+ tx/sn throughput
  - PinnedVec (recycler_out) ile sıfır kopya DMA

CPU Path (varsayılan):
  ed25519_verify_cpu() → rayon parallel iterator
  - Her core bir tx grubu işler
  - Daha düşük throughput (~300ns/tx → ~3.000 tx/sn/core)
```

`sigverify.rs` bu seçimi kendisi yapmaz — `solana_perf::sigverify::ed25519_verify()` içinde GPU mevcut/hazır mı kontrolü yapılır.

---

## 5. `sigverify_stage.rs` — Sigverify Pipeline Stage

**Dosya:** `core/src/sigverify_stage.rs`
**Toplam Satır:** 668 (test kodu dahil)
**Rol:** Paket batch'lerini alır, dedup+discard+shrink+verify pipeline'ından geçirir ve downstream'e gönderir.

### 5.1 Sabitler

```rust
// Satır 33: MAX_DEDUP_BATCH
const MAX_DEDUP_BATCH: usize = 165_000;
// Yorum: 50ms/(300ns/packet) = 166666 paket ~ 1300 batch
// 300ns/paket → CPU dedup hızı (Bloom filter benzeri)
// 165_000: 166_666'ya yuvarlanmış güvenli limit

// Satır 36: MAX_SIGVERIFY_BATCH
const MAX_SIGVERIFY_BATCH: usize = 5_000;
// Yorum: 50ms/(10us/packet) = 5000 paket
// 10us/paket → Ed25519 CPU doğrulama hızı
// 5000 paketten fazlası atılır (excess discard)

// Satır 40: MAX_DISCARDED_PACKET_RATE
const MAX_DISCARDED_PACKET_RATE: f64 = 0.10;
// %10 veya daha fazla paket atıldıysa → batch'leri shrink et
```

**Performans mühendisliği:** Bu sabitler mainnet makine ölçümlerinden türetilmiş (~50ms hedef).

### 5.2 Enum: `SigVerifyServiceError`

```rust
// Satır 42-49
#[derive(Error, Debug)]
pub enum SigVerifyServiceError<SendType> {
    #[error("send packets batch error")]
    Send(#[from] SendError<SendType>),

    #[error("streamer error")]
    Streamer(#[from] StreamerError),
}
```

- Generic `SendType` parametresi — farklı channel tiplerine uyum
- `#[from]`: `?` operatörü ile otomatik dönüşüm (thiserror)

### 5.3 Type Alias

```rust
// Satır 51
type Result<T, SendType> = std::result::Result<T, SigVerifyServiceError<SendType>>;
```

### 5.4 Struct: `SigVerifyStage`

```rust
// Satır 53-55
pub struct SigVerifyStage {
    thread_hdl: JoinHandle<()>,
}
```

- Tek thread içerir — tüm pipeline tek iş parçacığında

### 5.5 Trait: `SigVerifier`

```rust
// Satır 57-70
pub trait SigVerifier {
    type SendType: std::fmt::Debug;
    fn verify_batches(&self, batches: Vec<PacketBatch>, valid_packets: usize) -> Vec<PacketBatch>;
    fn process_received_packet(
        &mut self, _packet: &mut Packet,
        _removed_before_sigverify_stage: bool, _is_dup: bool,
    ) { }
    fn process_excess_packet(&mut self, _packet: &Packet) {}
    fn process_passed_sigverify_packet(&mut self, _packet: &Packet) {}
    fn send_packets(&mut self, packet_batches: Vec<PacketBatch>) -> Result<(), Self::SendType>;
}
```

- `type SendType`: Bağımlı tip — `BankingPacketBatch` veya `()` (disabled için)
- `verify_batches` ve `send_packets`: Zorunlu methodlar
- Diğerleri: Default boş implementasyon — override opsiyonel (hook pattern)
- `process_received_packet`: Dedup aşamasında her paketle çağrılır
- `process_excess_packet`: Limit aşımında çağrılır
- `process_passed_sigverify_packet`: İmza geçerli paketlerde çağrılır

### 5.6 Struct: `DisabledSigVerifier`

```rust
// Satır 72-73
#[derive(Default, Clone)]
pub struct DisabledSigVerifier {}
```

#### `impl SigVerifier for DisabledSigVerifier` — Satır 221-235

```rust
fn verify_batches(&self, mut batches: Vec<PacketBatch>, _valid_packets: usize) -> Vec<PacketBatch> {
    sigverify::ed25519_verify_disabled(&mut batches);
    batches
}

fn send_packets(&mut self, _packet_batches: Vec<PacketBatch>) -> Result<(), Self::SendType> {
    Ok(())
}
```

- `ed25519_verify_disabled`: Tüm paketleri geçerli işaretler, gerçek doğrulama yapmaz
- `send_packets`: Hiçbir yere göndermez — test/benchmark için

### 5.7 Struct: `SigVerifierStats`

```rust
// Satır 75-96
#[derive(Default)]
struct SigVerifierStats {
    recv_batches_us_hist: histogram::Histogram,      // recv süre histogramı
    verify_batches_pp_us_hist: histogram::Histogram, // per-packet verify histogramı
    discard_packets_pp_us_hist: histogram::Histogram,// per-packet discard histogramı
    dedup_packets_pp_us_hist: histogram::Histogram,  // per-packet dedup histogramı
    batches_hist: histogram::Histogram,              // batch sayısı histogramı
    packets_hist: histogram::Histogram,              // paket sayısı histogramı
    num_deduper_saturations: usize,                  // Bloom filter reset sayısı
    total_batches: usize,
    total_packets: usize,
    total_dedup: usize,
    total_excess_fail: usize,
    total_valid_packets: usize,
    total_shrinks: usize,
    total_discard_random: usize,
    total_dedup_time_us: usize,
    total_discard_time_us: usize,
    total_discard_random_time_us: usize,
    total_verify_time_us: usize,
    total_shrink_time_us: usize,
}
```

- 6 adet histogram + 12 adet sayaç
- `histogram::Histogram`: Min/max/mean/percentile hesaplama

#### `SigVerifierStats::report()` — Satır 99-218

- `datapoint_info!(name, ...)` macro ile tüm istatistikleri metrics sistemine gönderir
- p90, min, max, mean her histogram için ayrı ayrı raporlanır
- Metrics adı `metrics_name` parametresinden gelir: "tpu-verifier", "tpu-vote-verifier"

### 5.8 `impl SigVerifyStage`

#### `SigVerifyStage::new()` — Satır 238-247

```rust
pub fn new<T: SigVerifier + 'static + Send>(
    packet_receiver: Receiver<PacketBatch>,
    verifier: T,
    thread_name: &'static str,
    metrics_name: &'static str,
) -> Self {
    let thread_hdl = Self::verifier_service(packet_receiver, verifier, thread_name, metrics_name);
    Self { thread_hdl }
}
```

- Generic `T: SigVerifier + 'static + Send` — thread'e taşınabilir herhangi bir verifier
- `'static`: Thread'in referans tutmaması garantisi

#### `SigVerifyStage::discard_excess_packets()` — Satır 249-277

```rust
pub fn discard_excess_packets(
    batches: &mut [PacketBatch],
    mut max_packets: usize,
    mut process_excess_packet: impl FnMut(&Packet),
)
```

**Algoritma (fair discard):**

```
1. Atılmamış paketleri IP'ye göre grupla
2. max_packets IP başına eşit bölünür
3. Her IP'den cap = ceil(max_packets / num_addrs) kadar paket tutulur
4. Geri kalanlar discard edilir
```

```rust
// Satır 255-261
let mut addrs = batches
    .iter_mut()
    .rev()            // Geriden işle — yeni paketler tutulur
    .flat_map(|batch| batch.iter_mut().rev())
    .filter(|packet| !packet.meta().discard())
    .map(|packet| (packet.meta().addr, packet))
    .into_group_map(); // itertools: IP → [paketler] map'i
```

```rust
// Satır 263-271: Adil dağılım döngüsü
while max_packets > 0 && !addrs.is_empty() {
    let num_addrs = addrs.len();
    addrs.retain(|_, packets| {
        let cap = (max_packets + num_addrs - 1) / num_addrs;  // ceiling division
        max_packets -= packets.len().min(cap);
        packets.truncate(packets.len().saturating_sub(cap));   // sondan kırp
        !packets.is_empty()
    });
}
```

- `.rev()` ile işleme: En yeni paketler önce gelir, eski paketler discard edilir
- `ceiling division`: `(a + b - 1) / b` — her IP en az 1 paket tutma garantisi
- `retain()`: Boşalan IP'ler map'ten kaldırılır

**Tasarım kararı:** Tek IP'den gelen burst, diğer IP'lerin hakkını yemiyor. DoS koruması.

#### `SigVerifyStage::maybe_shrink_batches()` — Satır 280-293

```rust
pub fn maybe_shrink_batches(packet_batches: &mut Vec<PacketBatch>) -> (u64, usize)
```

**Algoritma:**
```
1. Toplam paket sayısı = count_packets_in_batches()
2. Atılan paket sayısı = count_discarded_packets()
3. discard_rate = atılan / toplam
4. Eğer discard_rate >= 0.10:
   → shrink_batches() ile boş slotları sıkıştır
5. Dönüş: (shrink_time_us, shrink_edilen_batch_sayısı)
```

- Öncesi ve sonrası 2 kez çağrılır: dedup/discard sonrası ve sigverify sonrası
- `pub` — benchmark için erişilebilir

#### `SigVerifyStage::verifier()` — Satır 295-407

```rust
fn verifier<const K: usize, T: SigVerifier>(
    deduper: &Deduper<K, [u8]>,
    recvr: &Receiver<PacketBatch>,
    verifier: &mut T,
    stats: &mut SigVerifierStats,
) -> Result<(), T::SendType>
```

**Const generic `K`:** Bloom filter'daki hash fonksiyonu sayısı (2 kullanılıyor).

**Tam Pipeline (satır satır):**

```
Satır 301: recv_packet_batches() → (batches, num_packets, recv_duration)
           Birden fazla batch blocking recv

Satır 310-317: Rastgele discard (discard_batches_randomly)
           MAX_DEDUP_BATCH (165_000) aşılırsa rastgele paket at
           → num_discarded_randomly sayaç

Satır 319-331: Dedup (dedup_packets_and_count_discards)
           Bloom filter ile duplikat tespiti
           Her paket için process_received_packet() hook çağrısı
           → discard_or_dedup_fail sayaç

Satır 335-346: Excess discard
           num_unique > MAX_SIGVERIFY_BATCH (5000) ise
           discard_excess_packets() ile IP bazlı adil atma
           → excess_fail sayaç

Satır 350: Pre-shrink (maybe_shrink_batches)
           Dedup/discard sonrası batch kompaksiyon

Satır 352-358: verify_batches() + count_valid_packets()
           Gerçek Ed25519 doğrulama
           Her valid paket için process_passed_sigverify_packet()

Satır 362: Post-shrink (maybe_shrink_batches)
           Sigverify sonrası batch kompaksiyon

Satır 364: send_packets() → BankingStage'e gönder

Satır 375-404: Stats güncelleme
```

**Pipeline özeti:**
```
recv → random_discard → dedup → excess_discard → pre_shrink
     → sigverify → post_shrink → send
```

**İstatistik detayı:**
- `verify_time.as_us() / num_packets`: per-packet verify süresini histograma ekler
- `discard_time.as_us() / num_packets`: per-packet discard süresini histograma ekler

#### `SigVerifyStage::verifier_service()` — Satır 409-453

```rust
fn verifier_service<T: SigVerifier + 'static + Send>(
    packet_receiver: Receiver<PacketBatch>,
    mut verifier: T,
    thread_name: &'static str,
    metrics_name: &'static str,
) -> JoinHandle<()>
```

**Deduper parametreleri:**
```rust
const MAX_DEDUPER_AGE: Duration = Duration::from_secs(2);      // 2 saniye sonra reset
const DEDUPER_FALSE_POSITIVE_RATE: f64 = 0.001;                // %0.1 yanlış pozitif
const DEDUPER_NUM_BITS: u64 = 63_999_979;                      // ~64M bit Bloom filter
```

**Thread döngüsü:**
```rust
let mut rng = rand::thread_rng();
let mut deduper = Deduper::<2, [u8]>::new(&mut rng, DEDUPER_NUM_BITS);
loop {
    if deduper.maybe_reset(&mut rng, DEDUPER_FALSE_POSITIVE_RATE, MAX_DEDUPER_AGE) {
        stats.num_deduper_saturations += 1;
    }
    match Self::verifier(&deduper, ...) {
        Err(SigVerifyServiceError::Streamer(StreamerError::RecvTimeout(
            RecvTimeoutError::Disconnected,
        ))) => break,                          // Channel kapandı → çık
        Err(SigVerifyServiceError::Streamer(StreamerError::RecvTimeout(
            RecvTimeoutError::Timeout,
        ))) => (),                             // Timeout → devam
        Err(SigVerifyServiceError::Send(_)) => break,  // Downstream kapandı → çık
        Err(e) => error!("{:?}", e),           // Diğer hatalar → log, devam
        Ok(()) => (),
    }
    if last_print.elapsed().as_secs() > 2 {
        stats.report(metrics_name);
        stats = SigVerifierStats::default();   // Stats sıfırla
        last_print = Instant::now();
    }
}
```

**Deduper:** `Deduper::<2, [u8]>` — 2 hash fonksiyonlu Bloom filter. `[u8]` üzerinde çalışır, yani paket verisi raw bytes olarak hashlenir. `maybe_reset()`: Bloom filter dolduğunda (false positive rate aşıldığında) sıfırlanır.

### 5.9 Testler

#### `test_packet_discard` — Satır 484-518

- 10 paketlik batch: 1 tanesi önceden discard işaretli, 1 tanesi farklı IP
- `max = 3` ile `discard_excess_packets()` çağrısı
- Doğrulamalar:
  - `total_non_discard == 3`
  - Önceden discard olan paketin callback'te sayılmaması
  - Farklı IP'den gelen paketin tutulması

#### `test_sigverify_stage_with_same_tx` / `test_sigverify_stage_without_same_tx` — Satır 534-630

İki ayrı test:
1. `use_same_tx = true`: Aynı tx tekrarlanınca dedup devreye girmeli
2. `use_same_tx = false`: Her tx farklı, dedup = 0 olmalı

**test_sigverify_stage iç mantığı (satır 544-630):**
- 1920 paket, 128'lik batch'ler (15 batch) gönderilir
- `total_packets < MAX_SIGVERIFY_BATCH` assert'i — excess discard olmasın
- Tüm paketler TRACER_PACKET işaretlenir
- `verified_r.recv()` ile alınan istatistikler kontrol edilir:
  - same_tx: deduped = received - 1 (veya received) — deduper reset olabilir
  - different_tx: deduped = 0, passed = received
  - Her iki durumda: excess = 0

#### `test_maybe_shrink_batches` — Satır 632-667

- 4096 paket, 32 batch üretilir
- Önce discard yok → shrink = 0
- `%10 - 1` paket discard et → shrink yok (eşik aşılmadı)
- +1 daha discard et → shrink devreye girer
- `expected_num_shrunk_batches = max(1, floor(32 * 0.10)) = 3`
- Kalan batch sayısı = 32 - 3 = 29

---

## 6. `shred_fetch_stage.rs` — Shred Fetch Stage

**Dosya:** `core/src/shred_fetch_stage.rs`
**Toplam Satır:** 603 (test kodu dahil)
**Rol:** TVU için UDP/QUIC soketlerinden shred'leri alır, filtreler ve sigverify'a gönderir.

### 6.1 Sabitler

```rust
// Satır 31
const PACKET_COALESCE_DURATION: Duration = Duration::from_millis(1);
```

- 1ms birleştirme süresi — shred'ler 1ms beklenip batch'lenir
- TPU'daki `tpu_coalesce` (default 5ms) ile farklı — shred'ler daha küçük ve gecikme hassas

### 6.2 Struct: `ShredFetchStage`

```rust
// Satır 33-35
pub(crate) struct ShredFetchStage {
    thread_hdls: Vec<JoinHandle<()>>,
}
```

- `pub(crate)`: Sadece crate içi erişim — dışarıya kapalı

### 6.3 `impl ShredFetchStage`

#### `ShredFetchStage::modify_packets()` — Satır 38-146

```rust
fn modify_packets(
    recvr: PacketBatchReceiver,
    sendr: Sender<PacketBatch>,
    bank_forks: &RwLock<BankForks>,
    shred_version: u16,
    name: &'static str,
    flags: PacketFlags,
    repair_context: Option<(&UdpSocket, &ClusterInfo)>,
    turbine_disabled: Arc<AtomicBool>,
)
```

**Durum değişkenleri (satır 50-71):**
```rust
let mut last_updated = Instant::now();
let mut keypair = repair_context.as_ref().map(|(_, cluster_info)| cluster_info.keypair().clone());
let (mut last_root, mut slots_per_epoch, mut feature_set, mut epoch_schedule, mut last_slot) = {
    let bank_forks_r = bank_forks.read().unwrap();
    let root_bank = bank_forks_r.root_bank();
    (
        root_bank.slot(),
        root_bank.get_slots_in_epoch(root_bank.epoch()),
        root_bank.feature_set.clone(),
        root_bank.epoch_schedule().clone(),
        bank_forks_r.highest_slot(),
    )
};
```

- Epoch bilgisi `DEFAULT_MS_PER_SLOT` (400ms) aralıklarla güncellenir — slot başına bir kez
- `feature_set`: Dinamik feature flag'leri (hard fork değişiklikleri)

**Ana döngü (satır 74-145):**

```rust
for mut packet_batch in recvr {  // Blocking iterator
    // Slot aralığında güncelleme
    if last_updated.elapsed().as_millis() as u64 > DEFAULT_MS_PER_SLOT {
        // bank_forks'tan yeni bilgileri al
    }

    // Repair ping yanıtı işleme
    if let Some((udp_socket, _)) = repair_context {
        ServeRepair::handle_repair_response_pings(udp_socket, keypair, &mut packet_batch, &mut stats);
    }

    // Slot aralığı hesabı: max = last_slot + 2 * slots_per_epoch
    let max_slot = last_slot + 2 * slots_per_epoch;

    // Feature kontrolü için closure'lar
    let should_drop_legacy_shreds = |shred_slot| check_feature_activation(...);
    let enable_chained_merkle_shreds = |shred_slot| check_feature_activation(...);

    let turbine_disabled = turbine_disabled.load(Ordering::Relaxed);
    for packet in packet_batch.iter_mut().filter(|p| !p.meta().discard()) {
        if turbine_disabled || should_discard_shred(packet, last_root, max_slot, ...) {
            packet.meta_mut().set_discard(true);
        } else {
            packet.meta_mut().flags.insert(flags);  // REPAIR veya empty
        }
    }

    stats.maybe_submit(name, STATS_SUBMIT_CADENCE);
    if sendr.send(packet_batch).is_err() {
        break;  // Downstream kapandı
    }
}
```

**`should_discard_shred` kontrolü:**
- `last_root`: Shred slot'u köke eşit veya küçükse at
- `max_slot = last_slot + 2 * slots_per_epoch`: İlerideki shred'leri at (2 epoch limiti)
- `shred_version`: Yanlış ağ versiyonu → at
- `should_drop_legacy_shreds`: Feature aktifse eski format shred'leri at
- `enable_chained_merkle_shreds`: Feature aktifse zincirleme Merkle shred'leri kabul et

**Repair ping yanıtı:**
- `ServeRepair::handle_repair_response_pings()`: Repair server'ının ping/pong protokolü
- Sadece repair soketinde aktif (`repair_context.is_some()`)
- Keypair'i cluster_info'dan alır — her slot güncelleme döneminde yenilenir

#### `ShredFetchStage::packet_modifier()` — Satır 148-200

```rust
fn packet_modifier(
    receiver_thread_name: &'static str,
    modifier_thread_name: &'static str,
    sockets: Vec<Arc<UdpSocket>>,
    exit: Arc<AtomicBool>,
    sender: Sender<PacketBatch>,
    recycler: PacketBatchRecycler,
    bank_forks: Arc<RwLock<BankForks>>,
    shred_version: u16,
    name: &'static str,
    flags: PacketFlags,
    repair_context: Option<(Arc<UdpSocket>, Arc<ClusterInfo>)>,
    turbine_disabled: Arc<AtomicBool>,
) -> (Vec<JoinHandle<()>>, JoinHandle<()>)
```

**İki aşamalı thread pattern:**

```
UDP soketleri → [Receiver threads] → internal channel → [Modifier thread] → output sender
```

1. Her soket için ayrı `streamer::receiver()` thread'i (satır 163-179)
2. Tek `modify_packets()` thread'i (satır 181-198)

```rust
let (packet_sender, packet_receiver) = unbounded();
let streamers = sockets.into_iter().enumerate().map(|(i, socket)| {
    streamer::receiver(
        format!("{receiver_thread_name}{i:02}"),
        socket, exit.clone(), packet_sender.clone(), recycler.clone(),
        Arc::new(StreamerReceiveStats::new("packet_modifier")),
        PACKET_COALESCE_DURATION, true, None,
    )
}).collect();
let modifier_hdl = Builder::new().name(modifier_thread_name.to_string()).spawn(move || {
    let repair_context = repair_context.as_ref()
        .map(|(socket, cluster_info)| (socket.as_ref(), cluster_info.as_ref()));
    Self::modify_packets(packet_receiver, sender, &bank_forks, ...)
}).unwrap();
(streamers, modifier_hdl)
```

- `Arc<UdpSocket>` ve `Arc<ClusterInfo>`: Thread'e taşınabilir
- `repair_context` Arc dönüşümü: `modify_packets` `&` referans bekler

#### `ShredFetchStage::new()` — Satır 202-320

```rust
pub(crate) fn new(
    sockets: Vec<Arc<UdpSocket>>,
    turbine_quic_endpoint_receiver: Receiver<(Pubkey, SocketAddr, Bytes)>,
    repair_socket: Arc<UdpSocket>,
    repair_quic_endpoint_receiver: Receiver<(SocketAddr, Vec<u8>)>,
    sender: Sender<PacketBatch>,
    shred_version: u16,
    bank_forks: Arc<RwLock<BankForks>>,
    cluster_info: Arc<ClusterInfo>,
    turbine_disabled: Arc<AtomicBool>,
    exit: Arc<AtomicBool>,
) -> Self
```

**Kurulum (6 thread grubu):**

```
1. Turbine UDP: sockets → "solRcvrShred{i}" + "solTvuPktMod"
   flags = PacketFlags::empty(), repair_context = None

2. Repair UDP: repair_socket → "solRcvrShredRep{i}" + "solTvuRepPktMod"
   flags = PacketFlags::REPAIR, repair_context = Some((repair_socket, cluster_info))

3. Repair QUIC: repair_quic_endpoint_receiver → "solTvuRecvRpr" + "solTvuFetchRpr"
   receive_repair_quic_packets() + modify_packets(flags=REPAIR, repair_context=None)

4. Turbine QUIC: turbine_quic_endpoint_receiver → "solTvuRecvQuic" + "solTvuFetchQuic"
   receive_quic_datagrams() + modify_packets(flags=empty, repair_context=None)
```

**Önemli not:** QUIC repair paketi modifikasyonunda `repair_context = None` çünkü QUIC'te ping/pong UDP gibi çalışmaz — ServeRepair ping işlemesi sadece UDP'de.

### 6.4 `receive_quic_datagrams()` — Satır 330-373 (serbest fonksiyon)

```rust
fn receive_quic_datagrams(
    turbine_quic_endpoint_receiver: Receiver<(Pubkey, SocketAddr, Bytes)>,
    sender: Sender<PacketBatch>,
    recycler: PacketBatchRecycler,
    exit: Arc<AtomicBool>,
)
```

**Unsafe blok (satır 345-347):**
```rust
unsafe {
    packet_batch.set_len(PACKETS_PER_BATCH);
};
```

**Neden unsafe:**
- `PacketBatch` bir `Vec<Packet>` veya benzeri yapı
- `set_len()`: Vektörün uzunluğunu initialize etmeden artırır
- `PACKETS_PER_BATCH` kapasiteye kadar önceden allocate edilmiş ama initialize edilmemiş bellek var
- Bu unsafe çünkü `PACKETS_PER_BATCH` indexleri henüz geçerli `Packet` değeri içermiyor
- Hemen ardından gelen `.zip(packet_batch.iter_mut())` döngüsü bu slotları doldurur
- `truncate(size)` ile gerçekten doldurulan kısma indirilir
- Risk: Set_len ile truncate arası hata olursa uninitialized memory okunabilir
- Solana'nın bu pattern'i: Sıfır initialize overhead — performance critical path

**Algoritma:**
```rust
let mut packet_batch = PacketBatch::new_with_recycler(&recycler, PACKETS_PER_BATCH, "...");
unsafe { packet_batch.set_len(PACKETS_PER_BATCH); }
let deadline = Instant::now() + PACKET_COALESCE_DURATION;  // 1ms

let entries = std::iter::once(entry).chain(
    std::iter::repeat_with(|| receiver.recv_deadline(deadline).ok())
        .while_some()  // None gelene kadar devam
);

let size = entries
    .filter(|(_, _, bytes)| bytes.len() <= PACKET_DATA_SIZE)  // Büyük paketleri at
    .zip(packet_batch.iter_mut())                              // Slot'lara yerleştir
    .map(|((_pubkey, addr, bytes), packet)| {
        *packet.meta_mut() = Meta { size: bytes.len(), addr: addr.ip(), port: addr.port(), flags: PacketFlags::empty() };
        packet.buffer_mut()[..bytes.len()].copy_from_slice(&bytes);
    })
    .count();

if size > 0 {
    packet_batch.truncate(size);
    sender.send(packet_batch);
}
```

- `while_some()` (itertools): `Option` iterator'ı `None` geldiğinde durur
- `recv_deadline()`: 1ms deadline'a kadar yeni datagram bekler — coalescing
- `_pubkey` field'ı yok sayılır — Turbine pakette pubkey bilgisi gelir ama kullanılmaz; imza doğrulama ayrı yapılır

### 6.5 `receive_repair_quic_packets()` — Satır 375-418 (pub(crate))

Aynı pattern, farklı channel tipi: `Receiver<(SocketAddr, Vec<u8>)>` (pubkey yok).

**Unsafe blok (satır 390-392):** Aynı `set_len(PACKETS_PER_BATCH)` — aynı risk/gerekçe.

**Fark:**
```rust
*packet.meta_mut() = Meta {
    ...
    flags: PacketFlags::REPAIR,  // Her QUIC repair paketi REPAIR flagli
};
```

`pub(crate)` — `tvu.rs`'den `repair_quic_endpoint_response_sender` üzerinden erişilir.

### 6.6 `check_feature_activation()` — Satır 421-436

```rust
#[must_use]
fn check_feature_activation(
    feature: &Pubkey,
    shred_slot: Slot,
    feature_set: &FeatureSet,
    epoch_schedule: &EpochSchedule,
) -> bool {
    match feature_set.activated_slot(feature) {
        None => false,  // Feature henüz aktif değil
        Some(feature_slot) => {
            let feature_epoch = epoch_schedule.get_epoch(feature_slot);
            let shred_epoch = epoch_schedule.get_epoch(shred_slot);
            feature_epoch < shred_epoch  // Shred feature aktifleşmesinden sonraki epoch'ta mı?
        }
    }
}
```

- `#[must_use]`: Dönüş değeri görmezden gelinirse warning
- Feature'ın aktifleştiği epoch, shred'in epoch'undan önce olmalı — epoch sınırında geçiş garantisi
- Örnek: `drop_legacy_shreds` feature'ı epoch 100'de aktifleştiyse, epoch 101+ shred'ler legacy drop'a tabidir

### 6.7 Testler

#### `test_data_code_same_index` — Satır 449-497

- Data shred ve ondan türetilen coding shred aynı index için `should_discard_shred` kontrolü
- Her ikisi de geçerli olmalı (discard = false)
- ReedSolomon cache kullanımı: `ReedSolomonCache::default()`

#### `test_shred_filter` — Satır 499-602

Şu senaryoları test eder:
1. **Boş paket** → `index_overrun += 1`
2. **Slot < root** → `slot_out_of_range += 1`
3. **Yanlış shred_version** → `shred_version_mismatch += 1`
4. **Geçerli shred** → `should_discard_shred = false`
5. **Slot çok yüksek (1_000_000)** → atılır
6. **Index >= MAX_DATA_SHREDS_PER_SLOT** → atılır

Stats counter'ları her senaryoda increment edilir.

---

## 7. Unsafe Blok Envanteri

| Dosya | Satır | Unsafe kodu | Neden unsafe | Risk |
|-------|-------|-------------|-------------|------|
| shred_fetch_stage.rs | 345-347 | `packet_batch.set_len(PACKETS_PER_BATCH)` | Uninitialized memory uzunluk artırma | Sonraki kod doldurmadan erişirse UB |
| shred_fetch_stage.rs | 390-392 | `packet_batch.set_len(PACKETS_PER_BATCH)` | Aynı | Aynı |

**Güvenlik analizi:** Her iki unsafe blok da aynı pattern. `set_len` sonrası `zip().map()` ile her slot doldurulur, ardından `truncate(size)` ile uzunluk gerçek değere getirilir. Döngüdeki `filter()` ve `zip()` sebebiyle yalnızca `PACKET_DATA_SIZE`'ı geçmeyen, başarılı okunan datagram'lar yazılır. Teorik risk düşük ama panic ya da erken return durumunda uninit bellek okunabilir.

---

## 8. Tasarım Kararları ve Mimari Dersler

### 8.1 Pipeline Stage Composition

Her stage (`FetchStage`, `SigVerifyStage`, `ShredFetchStage`, vb.):
- Kendi thread'lerini yönetir
- `join()` ile temiz kapatılır
- Channel'lar üzerinden haberleşir (crossbeam unbounded)
- `Arc<AtomicBool> exit` sinyali ile durdurulur

Bu pattern Nova Shield Chain için doğrudan alınabilir.

### 8.2 Üç Seviyeli İmza Doğrulama Stratejisi

1. **Hızlı ön eleme:** random discard (165K limit) → Bloom dedup (5K limit aşımı)
2. **Adil excess discard:** IP başına eşit pay
3. **Gerçek doğrulama:** Ed25519 batch (CPU/GPU)

Nova Shield Chain bu sırayı benimser ve MPC/threshold imzalar için 2. aşamayı genişletebilir.

### 8.3 Coalescing Tradeoff

- TPU: `tpu_coalesce` (default 5ms) — büyük batch'ler, yüksek throughput, biraz gecikme
- TVU/ShredFetch: `PACKET_COALESCE_DURATION = 1ms` — shred'ler için düşük gecikme öncelikli
- Tasarım kararı: Transaction latency vs. consensus latency farklı öncelikler

### 8.4 Feature Gate Kontrolü Shred Katmanında

`check_feature_activation()` ile protokol yükseltmeleri epoch sınırında kesiksiz yapılır. Hard fork olmadan format değişikliği mümkün.

### 8.5 Vote Transaction Ayrımı

Vote tx'leri:
- Ayrı soket (UDP/QUIC)
- Ayrı receiver channel
- Ayrı SigVerifyStage (`reject_non_vote = true`)
- BankingStage'de ayrı channel

Bu sayede vote işlemi normal transaction yüküyle rekabet etmez.

### 8.6 UDP ve QUIC Paralel Çalışma

- TPU: `tpu_enable_udp` ile UDP açılabilir/kapatılabilir, QUIC her zaman
- TVU: Hem UDP hem QUIC shred alır, aynı pipeline'a besler
- Tasarım: QUIC geçiş sürecinde UDP geri dönüş (fallback) sağlar

### 8.7 Stake-Based Flow Control

- `MAX_STAKED_CONNECTIONS` vs `MAX_UNSTAKED_CONNECTIONS`: Stake'li node'lar daha fazla bağlantı hakkı
- Forward soket: Unstaked node'lardan `0` bağlantı (tam engel)
- QUIC layer, transaction işlenmeden önce spam'i keser

---

## 9. Nova Shield Chain İçin Dersler

### Ders 1: Transaction Pipeline Ayrımı
Vote, consensus ve normal transaction'ları başından ayrı pipeline'larda işle. Solana'nın vote ayrımı güçlü bir pattern.

### Ders 2: Dedup + Fair Discard Kombinasyonu
Bloom filter dedup (hız) + IP bazlı adil discard (adalet) ikili stratejisi DoS direnci için kritik. Nova Shield Chain'de MPC işlemleri için de benzer koruma katmanları eklenebilir.

### Ders 3: Memory Pool (Recycler)
`PacketBatchRecycler::warmed(N, CAP)` ile allocation overhead ortadan kalkar. Nova Shield Chain'de transaction batch'leri için benzer pool gerekli.

### Ders 4: Unsafe set_len Pattern
QUIC datagram toplama için `set_len` + doldurma + `truncate` pattern'i GC-free batch oluşturma sağlar. Nova Shield için dikkatli kullanılabilir; alternatif olarak `MaybeUninit` ile daha açık unsafe boundary tanımlanabilir.

### Ders 5: Feature Gate Tabanlı Protokol Evrimleşmesi
`check_feature_activation()` ile epoch bazlı feature aktivasyonu: Ağ çapında soft upgrade. Nova Shield Chain'in upgrade mekanizmasında bu modeli kullanmak sağlıklı geçiş sağlar.

### Ders 6: Stage Kapatma Sırası Önemli
`join()` sırasında upstream'i önce bitir, downstream'i sonra. Aksi takdirde channel backpressure oluşur ve deadlock riski var. Solana'nın `tvu.join()` sırası bunu dikkate alır.

### Ders 7: Coalesce Duration Ayarlanabilir
Her soket tipi için farklı coalesce süresi. Nova Shield Chain'in RPC, P2P ve consensus katmanları için ayrı coalesce süreleri belirlenebilir.

### Ders 8: `pub(crate)` Visibility
`ShredFetchStage` için `pub(crate)` — crate sınırı izolasyon noktası. Nova Shield Chain modüler yapısında bu visibility stratejisi uygulanmalı.

### Ders 9: Metrics Her Stage'de
Her stage kendi `report()` methoduna sahip. Nova Shield Chain'in her pipeline stage'i Prometheus/Grafana için metrics yaymak üzere tasarlanmalı.

### Ders 10: Repair Protokolü Entegrasyonu
UDP repair ile QUIC repair ayrı thread'lerde ama aynı output channel'a beslenir. Nova Shield Chain'in şred/chunk repair mekanizması da bu çok-path yaklaşımını benimseyebilir.

---

## 10. Bağımlılık Haritası (Channel Flow)

```
[External UDP/QUIC]
        │
        ▼
[FetchStage]
  ├── packet_sender → SigVerifyStage
  ├── vote_packet_sender → VoteSigVerifyStage
  └── forward_sender → handle_forwarded_packets
                              │
                    (leader yakınsa)
                              │
                              ▼
                        packet_sender (geri)

[SigVerifyStage]
  packet_receiver → [dedup+discard+verify] → non_vote_sender

[VoteSigVerifyStage]
  vote_packet_receiver → [verify, reject_non_vote=true] → tpu_vote_sender

[BankingTracer]
  create_channel_non_vote() → non_vote_receiver
  create_channel_tpu_vote() → tpu_vote_receiver
  create_channel_gossip_vote() → gossip_vote_receiver

[ClusterInfoVoteListener]
  gossip → gossip_vote_sender

[BankingStage]
  ← non_vote_receiver
  ← tpu_vote_receiver
  ← gossip_vote_receiver
  → entry_receiver

[BroadcastStage]
  ← entry_receiver → [shredding] → UDP/QUIC ağa


TVU FLOW:
[External UDP/QUIC/Repair]
        │
        ▼
[ShredFetchStage]
  fetch_sender → shred_sigverify
  retransmit_sender → RetransmitStage

[shred_sigverify]
  verified_sender → WindowService
  retransmit_sender → RetransmitStage

[WindowService]
  verified_receiver → blockstore yazma
  repair_quic_endpoint_sender → QUIC repair

[ReplayStage]
  ledger_signal_receiver → bank replay
  duplicate_slots_sender → consensus
```

---

*Analiz tamamlandı. 6 dosya, ~1700 satır kod kapsamlı biçimde incelendi.*
