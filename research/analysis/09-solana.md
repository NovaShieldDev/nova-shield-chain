# Repo 9: Solana — Kapsamlı Analiz

**Repo:** `research/chains/solana`
**Dil:** Rust
**İnceleme Tarihi:** 2026-07-17
**Analist:** Nova Shield Chain Araştırma Ekibi

---

## İçindekiler

1. Genel Mimari ve Felsefe
2. Proof of History (PoH) Çekirdek Algoritması
3. PoH Recorder — Leader ile Entegrasyon
4. Tower BFT — Konsensüs Protokolü
5. VoteState ve Lockout Mekanizması
6. Fork Seçimi (Fork Choice)
7. Sealevel — Paralel İşlem Çalıştırma
8. Banking Stage — İşlem Pipeline
9. Bank Struct — Blok Yaşam Döngüsü
10. Turbine — Blok Yayma Protokolü
11. Gulf Stream — İşlem Yönlendirme
12. ClusterInfo ve Gossip Katmanı
13. CRDS — Çakışmasız Replike Veri Yapısı
14. Hesap Modeli ve AccountsDB
15. Şifreleme ve İmza Altyapısı
16. Nova Shield için Çıkarımlar

---

## 1. Genel Mimari ve Felsefe

Solana, 2020'de mainnet'e geçen yüksek-throughput Layer 1 blockchain'dir. Temel tasarım kararları:

- **PoH (Proof of History):** Zaman damgası olarak güvenilir kriptografik saat — konsensüs öncesi sıralama
- **Tower BFT:** PoH üzerine inşa edilmiş PBFT türevi — PoH'un global sırasından güç alan opt-in lockout mekanizması
- **Sealevel:** Paralel işlem çalıştırma motoru — birbirini "dokunmayan" işlemler aynı anda çalışır
- **Turbine:** Reed-Solomon erasure coding ile shred tabanlı blok yayma
- **Gulf Stream:** İşlemleri gelecekteki lider'a önceden iletme
- **Pipeline mimarisi:** Fetch → Verify → Banking → Broadcast tüm aşamaları paralel çalışır

Solana'nın hedefi: TEK bir lider, TEK bir zincir (fork yoksa), 400ms slot süresi, 65,000+ TPS.

---

## 2. Proof of History (PoH) Çekirdek Algoritması

### Dosya: `entry/src/poh.rs`

```rust
pub struct Poh {
    pub hash: Hash,          // Mevcut PoH hash değeri
    num_hashes: u64,         // Bu tick'te kaç hash üretildi
    hashes_per_tick: u64,    // Bir tick için kaç hash gerekiyor (sabit)
    remaining_hashes: u64,   // Tick tamamlanana kadar kaç hash kaldı
    tick_number: u64,        // Kaçıncı tick
    slot_start_time: Instant, // Slot başlangıç zamanı (metrik için)
}
```

### Üç Temel Operasyon

**hash(max_num_hashes) — Boş zaman damgası**
```rust
// Yeni işlem yokken, sadece zaman geçiyor
hash = SHA256(hash)
num_hashes++
remaining_hashes--
```

**record(mixin: Hash) — İşlem kaydı**
```rust
// Bir işlem paketi kayıt edilirken
hash = hashv([hash || mixin])  // mixin = işlem listesinin hash'i
// PohEntry { num_hashes, hash } döndürür
```

**tick() — Tick oluşturma**
```rust
// remaining_hashes == 0 olduğunda
hash = SHA256(hash)
// PohEntry { num_hashes, hash } döndürür
tick_number++
remaining_hashes = hashes_per_tick
num_hashes = 0
```

### Matematiksel Anlam

PoH, **Verifiable Delay Function (VDF)**'nin bir uygulamasıdır:

```
H₀ = genesis_hash
H₁ = SHA256(H₀)
H₂ = SHA256(H₁)
...
Hₙ = SHA256(Hₙ₋₁)

Kayıt anında: Hₖ = hashv([Hₖ₋₁ || işlem_hash])
```

**Güvenlik özelliği:** Sıralı SHA256 zinciri paralel hesaplanamaz. Belirli bir Hₙ değerini elde etmek için n adımı MUTLAKA sırayla yapmak gerekir. Bu, geçen sürenin kriptografik kanıtıdır.

**Sonuç:** Bir doğrulayıcı, herhangi bir anda hangi işlemin ne zaman eklendiğini tarihsel olarak kanıtlayabilir.

### Performans Parametreleri

- hashes_per_tick: Sabit bir değer (genesis'ten gelir), her tick için zorunlu SHA256 sayısı
- Tipik değer: ~12,500 hash/tick
- Tick sayısı/slot: 64 tick/slot
- Slot süresi: 400ms → hashes_per_tick ayarlanarak kalibrasyon yapılır

---

## 3. PoH Recorder — Leader ile Entegrasyon

### Dosya: `poh/src/poh_recorder.rs`

```rust
pub struct Record {
    pub mixin: Hash,                        // Kayıt edilecek işlem hash'i
    pub transactions: Vec<VersionedTransaction>,
    pub slot: Slot,
    pub sender: RecordResultSender,         // Sonucu bildirmek için kanal
}

pub struct BankStart {
    pub working_bank: Arc<Bank>,            // Aktif bank (bu slot için)
    pub bank_creation_time: Arc<Instant>,  // Slot başlangıcı
}

// WorkingBankEntry = (Arc<Bank>, (Entry, u64))
```

### Önemli Sabitler

```rust
const GRACE_TICKS_FACTOR: u64 = 2;   // Yavaş işlemler için ek tick toleransı
const MAX_GRACE_SLOTS: u64 = 2;      // Maksimum ek slot toleransı
```

### PohRecorder'ın Rolü

```
Banking Stage → Record(mixin) → PohRecorder → poh.record(mixin) → Entry
                                             ↓
                                    Tick dolduğunda → Ledger'a yaz
```

PohRecorder, PoH çekirdek algoritmasını Banking Stage ile bağlar:
1. Banking Stage işlemleri alır, imzaları doğrular
2. İşlemleri `Record` mesajı olarak PohRecorder'a gönderir
3. PohRecorder `poh.record(mixin)` çağırır → PoH zincire işler
4. Entry oluşturulur ve ledger'a yazılır

---

## 4. Tower BFT — Konsensüs Protokolü

### Dosya: `core/src/consensus.rs`

### Temel Sabitler

```rust
pub const VOTE_THRESHOLD_DEPTH: usize = 8;      // Kaç slot derinlikte %2/3 staker onayı gerekiyor
pub const SWITCH_FORK_THRESHOLD: f64 = 0.38;   // Fork değiştirmek için %38 stake görülmeli
```

### Tower Struct

```rust
pub struct Tower {
    pub node_pubkey: Pubkey,
    threshold_depth: usize,           // = VOTE_THRESHOLD_DEPTH (8)
    threshold_size: f64,              // = VOTE_THRESHOLD_SIZE (0.667)
    pub(crate) vote_state: VoteState, // Validator'ın oy durumu
    last_vote: VoteTransaction,
    last_vote_tx_blockhash: BlockhashStatus,
    last_timestamp: BlockTimestamp,
    stray_restored_slot: Option<Slot>, // Restart sonrası kurtarılan slot
    last_switch_threshold_check: Option<(Slot, SwitchForkDecision)>,
}
```

### SwitchForkDecision — Fork Değiştirme Kararı

```rust
pub enum SwitchForkDecision {
    SwitchProof(Hash),                          // Fork değiştirilebilir + kanıt
    SameFork,                                   // Aynı fork'ta devam
    FailedSwitchThreshold(u64, u64),            // (görülen_stake, toplam_stake) yetersiz
    FailedSwitchDuplicateRollback(Slot),        // Duplicate slot algılandı
}
```

### ComputedBankState

```rust
pub(crate) struct ComputedBankState {
    pub voted_stakes: VotedStakes,          // HashMap<Slot, Stake> — her slot için görülen stake
    pub total_stake: Stake,
    pub fork_stake: Stake,                  // Bu fork'ta görülen stake
    pub lockout_intervals: LockoutIntervals,
    pub my_latest_landed_vote: Option<Slot>,
}
```

### Oy Verme Mekanizması

**collect_vote_lockouts:**
```rust
// Her validator'ın oyları incelenir
for (vote_account, stake) in vote_accounts {
    for vote in vote_account.votes() {
        // Lockout dolmamışsa bu slot için oy sayılır
        if vote.is_locked_out_at_slot(current_slot) {
            voted_stakes[vote.slot] += stake
        }
    }
}
```

**is_slot_confirmed:**
```rust
fn is_slot_confirmed(slot, voted_stakes, total_stake, threshold_size) -> bool {
    voted_stakes.get(slot) / total_stake > threshold_size  // > %66.7
}
```

**record_bank_vote:**
```rust
// Yeni oy kaydedilir
// 1. VoteState güncellenir (pop+push cycle)
// 2. Lockout'lar güncellenir (güncelleme sırası önemli)
// 3. Threshold kontrolü yapılır
```

---

## 5. VoteState ve Lockout Mekanizması

### Dosya: `sdk/program/src/vote/state/mod.rs`

### Sabitler

```rust
pub const MAX_LOCKOUT_HISTORY: usize = 31;  // Maksimum oy tarihi derinliği
pub const INITIAL_LOCKOUT: usize = 2;       // İlk lockout = 2 slot
pub const VOTE_CREDITS_GRACE_SLOTS: u8 = 2;
pub const VOTE_CREDITS_MAXIMUM_PER_SLOT: u8 = 16;
```

### Lockout Struct

```rust
pub struct Lockout {
    slot: Slot,
    confirmation_count: u32,  // Kaç kez onaylandı
}

impl Lockout {
    pub fn lockout(&self) -> u64 {
        (INITIAL_LOCKOUT as u64).pow(self.confirmation_count())
        // confirmation_count=1 → lockout=2
        // confirmation_count=2 → lockout=4
        // confirmation_count=3 → lockout=8
        // confirmation_count=n → lockout=2^n
    }

    pub fn last_locked_out_slot(&self) -> Slot {
        self.slot.saturating_add(self.lockout())
    }

    pub fn is_locked_out_at_slot(&self, slot: Slot) -> bool {
        self.last_locked_out_slot() >= slot
    }
}
```

### VoteState Struct

```rust
pub struct VoteState {
    pub node_pubkey: Pubkey,          // Validator node kimliği
    pub authorized_withdrawer: Pubkey,
    pub commission: u8,               // Komisyon oranı (%0-100)
    pub votes: VecDeque<LandedVote>,  // Son MAX_LOCKOUT_HISTORY(31) oy
    pub root_slot: Option<Slot>,      // Finalize edilmiş root slot
    authorized_voters: AuthorizedVoters, // Dönem bazlı yetkili oylayıcılar
    prior_voters: CircBuf<(Pubkey, Epoch, Epoch)>, // MAX_ITEMS=32 geçmiş
    pub epoch_credits: Vec<(Epoch, u64, u64)>,     // Dönem kredi geçmişi
    pub last_timestamp: BlockTimestamp,
}
```

### LandedVote

```rust
pub struct LandedVote {
    pub latency: u8,       // Oy slotuyla landing slotu arasındaki fark
    pub lockout: Lockout,  // Temel lockout
}
```

### Lockout Dinamiği — Geometrik Büyüme

Yeni oy eklendiğinde tüm eski lockout'ların `confirmation_count` artar:

```
Slot 100 eklenir → stack: [(100, count=1)]
Slot 101 eklenir → stack: [(100, count=2), (101, count=1)]
Slot 102 eklenir → stack: [(100, count=3), (101, count=2), (102, count=1)]
...
Slot 108 eklenir → stack: [(100, count=9, lockout=512), ...]
```

Slot 100 confirmation_count=9 olduğunda: lockout = 2^9 = 512 slot
Yani slot 100 için 512 slot boyunca "başka fork'ta oy veremezsin" garantisi var.

**Slashing kuralı:** Kilitli bir slot'ta başka fork'a oy vermek → slash (stake kaybı).

### CircBuf — Dairesel Tarih Tamponu

```rust
pub struct CircBuf<I> {
    buf: [I; MAX_ITEMS],   // MAX_ITEMS=32 (sabit dizi)
    idx: usize,            // Sonraki yazma pozisyonu
    is_empty: bool,
}
```

32 öğelik döngüsel tampon — geçmiş yetkili oylayıcılar burada tutulur (slashing için referans).

---

## 6. Fork Seçimi (Fork Choice)

Tower BFT'de fork seçimi şu kurallara dayanır:

### Threshold Kuralı

- Son `VOTE_THRESHOLD_DEPTH` (8) oy için her biri %66.7 onay almalı
- Aksi halde yeni oy verilmez (bekle)

### Switch Kuralı (Fork Değiştirme)

Farklı fork'a geçmek için:
1. Mevcut fork dışında **%38 stake** görülmeli
2. Geçilecek fork için bir `SwitchProof(Hash)` üretilmeli
3. Lockout süresi dolmuş olmalı

```rust
if switch_stake / total_stake >= SWITCH_FORK_THRESHOLD {
    SwitchForkDecision::SwitchProof(proof_hash)
} else {
    SwitchForkDecision::FailedSwitchThreshold(switch_stake, total_stake)
}
```

### Duplicate Slot Koruması

`FailedSwitchDuplicateRollback(Slot)` — Duplicate slot algılandığında fork değiştirme engellenir. Bu, split-brain senaryolarını önler.

---

## 7. Sealevel — Paralel İşlem Çalıştırma

### Dosya: `runtime/src/bank.rs`

### Temel Prensip

İki işlem BAĞIMSIZ ise (ortak hesaba yazmıyorlarsa) eş zamanlı çalıştırılabilir.

Solana'da her işlem, okuyacağı ve yazacağı hesapları önceden BEYAN ETMEK zorundadır.

### İşlem Listesinden Paralel Gruplara Ayrılma

```
İşlem A: [AccountX (yazma), AccountY (okuma)]
İşlem B: [AccountZ (yazma), AccountW (okuma)]
İşlem C: [AccountX (yazma), AccountZ (okuma)]  ← A ile çakışıyor!

Paralel Grup 1: [A, B] → aynı anda çalış
Sıralı: C → A veya B bittikten sonra çalış
```

### rayon ThreadPool

```rust
// ThreadPool ile paralel yürütme
use rayon::prelude::*;

// İşlemler paralel gruplar halinde işlenir
transactions.par_iter()
    .for_each(|tx| execute_transaction(tx))
```

### DashMap — Concurrent Hesap Deposu

```rust
// DashMap: thread-safe HashMap, shard tabanlı kilitleme
use dashmap::DashMap;

// Hesap durumu DashMap'te tutulur
// Her shard bağımsız kilitlenebilir → gerçek paralellik
let accounts: DashMap<Pubkey, AccountSharedData>
```

### TransactionBatchProcessor

```rust
pub struct TransactionBatchProcessor<FG: ForkGraph> {
    // SVM (Solana Virtual Machine) — WASM/BPF program çalıştırıcı
    // rayon ile paralel çalıştırma
}

pub struct LoadAndExecuteTransactionsOutput {
    pub loaded_transactions: Vec<TransactionLoadResult>,
    pub execution_results: Vec<TransactionExecutionResult>,
    pub retryable_transaction_indexes: Vec<usize>,
    pub executed_transactions_count: usize,
    pub executed_non_vote_transactions_count: usize,
    pub executed_with_successful_result_count: usize,
    pub signature_count: u64,
    pub error_counters: TransactionErrorMetrics,
}
```

---

## 8. Banking Stage — İşlem Pipeline

### Dosya: `core/src/banking_stage.rs`

### Temel Sabitler

```rust
pub const NUM_THREADS: u32 = 6;              // Paralel thread sayısı
const TOTAL_BUFFERED_PACKETS: usize = 700_000; // Toplam buffer kapasitesi
```

### Bileşenler

```
İşlem Paketi (UDP/QUIC) → PacketReceiver
                        ↓
                 SigVerify (GPU/CPU)
                        ↓
              DecisionMaker (bu transaction forward mı, execute mı?)
                    ↙          ↘
            Forwarder        Consumer
          (Başka leader'a)  (Bu slot'ta çalıştır)
                              ↓
                        PrioGraphScheduler
                              ↓
                      (Öncelik grafiği çözme)
                              ↓
                      ParallelConsumer (6 thread)
```

### Consumer Struct

```rust
pub struct Consumer {
    committer: Committer,
    transaction_recorder: TransactionRecorder,  // PoH'a kayıt
    qos_service: QosService,                    // Compute unit hesabı
    log_messages_bytes_limit: Option<usize>,
}

pub const TARGET_NUM_TRANSACTIONS_PER_BATCH: usize = 64;  // Batch boyutu
```

### QosService — Compute Unit Yönetimi

Her işlemin compute unit (CU) maliyeti vardır. QosService:
1. Her batch için toplam CU hesaplar
2. Block limit'i aşmamak için kesiyor
3. Öncelikli işlemlere önce yer veriyor

### PrioGraphScheduler

İşlemler önce bir bağımlılık grafiğine dönüştürülür:
- Aynı hesaba yazan işlemler arasında kenar var → sıralı
- Farklı hesaplara yazan işlemler → paralel

Grafiği çözmek için topolojik sıralama + öncelik (fee/CU) kombinasyonu.

---

## 9. Bank Struct — Blok Yaşam Döngüsü

### Dosya: `runtime/src/bank.rs`

### BankRc

```rust
pub struct BankRc {
    pub accounts: Arc<Accounts>,            // Tüm hesaplar
    pub(crate) parent: RwLock<Option<Arc<Bank>>>, // Üst bank (parent slot)
    pub(crate) slot: Slot,
    pub(crate) bank_id_generator: Arc<AtomicU64>,
}
```

### Bank Yaşam Döngüsü

```
genesis
   ↓
new(parent_bank, slot, ...)  → AÇIK (işlem kabul eder)
   ↓
process_transactions(...)    → İşlemler çalıştırılır
   ↓
freeze()                     → DONDURULDU (yeni işlem yok)
   ↓
root(bank)                   → KÖKLENMIŞ (kesinleşti, snapshot alınabilir)
```

### BankFieldsToDeserialize — Snapshot Yapısı

Snapshot'ta kaydedilen kritik alanlar:
```rust
pub struct BankFieldsToDeserialize {
    blockhash_queue: BlockhashQueue,
    ancestors: AncestorsForSerialization,
    hash: Hash,                             // Bu bank'ın hash'i
    parent_hash: Hash,
    parent_slot: Slot,
    transaction_count: u64,
    tick_height: u64,
    capitalization: u64,                    // Toplam lamport (enflasyon hesabı için)
    hashes_per_tick: Option<u64>,
    ticks_per_slot: u64,
    ns_per_slot: u128,
    slot: Slot,
    epoch: Epoch,
    block_height: u64,
    collector_id: Pubkey,                   // Leader (ücret toplayan)
    collector_fees: u64,
    fee_rate_governor: FeeRateGovernor,
    inflation: Inflation,
    stakes: Stakes<Delegation>,
    epoch_stakes: HashMap<Epoch, EpochStakes>,
    epoch_reward_status: EpochRewardStatus,
}
```

### Atomik Sayaçlar — Lock-free Performans

```rust
// RwLock yerine atomik değişkenler
transaction_count: AtomicU64,
tick_height: AtomicU64,
signature_count: AtomicU64,
capitalization: AtomicU64,
// Ordering::Relaxed kullanılır — zaten paralel yazma yok (tek leader)
```

---

## 10. Turbine — Blok Yayma Protokolü

### Dosya: `turbine/src/broadcast_stage.rs`

### Temel Konsept

Solana büyük blokları doğrudan yayınlamaz. Shred tabanlı erasure coding kullanır:

```
Blok (64KB+) → Shred'lere böl (ör. 1280 byte/shred)
             ↓
      Erasure coding (Reed-Solomon)
             ↓
      Turbine tree üzerinden dağıt
```

### Broadcast Run'lar

```rust
// İki mod:
StandardBroadcastRun   // Normal çalışma
broadcast_duplicates_run  // Test/pentest — kasıtlı çift shred (simülasyon)
```

### ClusterNodes Cache

```rust
const CLUSTER_NODES_CACHE_NUM_EPOCH_CAP: usize = 8;  // 8 epoch cache
const TTL: Duration = Duration::from_secs(5);          // 5 saniye TTL
```

### Turbine Tree Yapısı

Ağ katmanlıdır (gossip layer'dan farklı olarak broadcast için özel):
- **Layer 0:** Leader (blok üreticisi)
- **Layer 1:** Stake-weighted shuffle ile seçilen ~200 node
- **Layer 2+:** Her layer-1 node kendi subtree'sine yayar

Her node sadece kendi shred setini alır + komşularına iletir → bant genişliği O(log N).

### Shred Yapısı

```rust
// Shred: blok parçası + erasure coding metadata
// DATA_SHREDS_PER_FEC_BLOCK: Her FEC (Forward Error Correction) bloğu kaç data shred içeriyor
// CODING_SHREDS_PER_FEC_BLOCK: Her FEC bloğu kaç coding shred içeriyor
// k data shred + (n-k) coding shred → herhangi k tanesi yeterli
```

---

## 11. Gulf Stream — İşlem Yönlendirme

Gulf Stream, işlemlerin gelecekteki lider'a önceden iletilmesi mekanizmasıdır.

### Nasıl Çalışır

```
Kullanıcı → İşlem gönderir (RPC aracılığıyla)
         → Mevcut lider değil, GELECEK lider'a gider
         → Lider işlemi Banking Stage buffer'ına ekler
         → Slot geldiğinde işlem zaten hazır
```

### Lider Çizelgesi

```rust
// LeaderSchedule: her epoch için hangi validator'ın hangi slot'ta lider olduğu
// Deterministik + stake-weighted random shuffle
// ~4 slot önceden bilinir

// Epoch = 432,000 slot ≈ 2 gün
// Her validator 4 ardışık slot liderlik yapar
```

### Avantajları

- Mempool yoktur (traditional sense'de)
- İşlemler doğru lider'a önceden gider → gecikme azalır
- Lider slot başladığında buffer dolup taşmaz

---

## 12. ClusterInfo ve Gossip Katmanı

### Dosya: `gossip/src/cluster_info.rs`

### Temel Sabitler

```rust
pub const GOSSIP_SLEEP_MILLIS: u64 = 100;           // Gossip döngüsü: 100ms
pub const MAX_CRDS_OBJECT_SIZE: usize = 928;        // Maks CRDS nesne boyutu
const MAX_GOSSIP_TRAFFIC: usize = 128_000_000 / PACKET_DATA_SIZE; // 1Gbps sınırı
const PUSH_MESSAGE_MAX_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE - 44;
pub(crate) const CRDS_UNIQUE_PUBKEY_CAPACITY: usize = 8192; // Gossip'te maks node
const MIN_STAKE_FOR_GOSSIP: u64 = LAMPORTS_PER_SOL; // Gossip için min stake
const MIN_NUM_STAKED_NODES: usize = 500;            // Stake enforcing için min node
```

### ClusterInfo Struct

```rust
pub struct ClusterInfo {
    pub gossip: CrdsGossip,                          // Gossip protokol motoru
    keypair: RwLock<Arc<Keypair>>,                   // İmzalama anahtarı
    entrypoints: RwLock<Vec<LegacyContactInfo>>,    // Ağ giriş noktaları
    outbound_budget: DataBudget,                    // Çıkış bant genişliği sınırlama
    my_contact_info: RwLock<ContactInfo>,
    ping_cache: Mutex<PingCache>,                   // UDP ping/pong önbelleği
    stats: GossipStats,
    socket: UdpSocket,
    local_message_pending_push_queue: Mutex<Vec<CrdsValue>>,
    contact_debug_interval: u64,
    instance: RwLock<NodeInstance>,
    socket_addr_space: SocketAddrSpace,
}
```

### Gossip Protokol Mesajları

```rust
pub(crate) enum Protocol {
    PullRequest(CrdsFilter, CrdsValue),      // Filtre + kendi verisi
    PullResponse(Pubkey, Vec<CrdsValue>),    // Karşılık veriler
    PushMessage(Pubkey, Vec<CrdsValue>),     // Aktif yayın
    PruneMessage(Pubkey, PruneData),         // "Bu kaynaktan veri almayı kes"
    PingMessage(Ping),                       // UDP erişilebilirlik testi
    PongMessage(Pong),
}
```

### PruneData — Gossip Optimizasyonu

```rust
pub(crate) struct PruneData {
    pubkey: Pubkey,       // Prune mesajı gönderen
    prunes: Vec<Pubkey>, // Kesilecek kaynaklar (maks MAX_PRUNE_DATA_NODES=32)
    signature: Signature,
    destination: Pubkey, // Alıcı
    wallclock: u64,
}
```

Prune, gossip'te flood'u önler: "Artık X'ten veri alma, benden alırsın."

### WeightedShuffle — Stake-ağırlıklı Seçim

```rust
use crate::weighted_shuffle::WeightedShuffle;
// Gossip partner'lar stake ağırlığına göre seçilir
// Yüksek stake = daha yüksek seçilme olasılığı
```

---

## 13. CRDS — Çakışmasız Replike Veri Yapısı

### Ne İçerir

```rust
pub enum CrdsData {
    LegacyContactInfo(LegacyContactInfo),   // Eski iletişim bilgisi
    Vote(EpochSlotsIndex, Vote),            // Validator oyu
    LowestSlot(u8, LowestSlot),             // En düşük replike edilen slot
    EpochSlots(EpochSlotsIndex, EpochSlots), // Replike edilmiş slot seti
    Version(Version),                       // Validator yazılım versiyonu
    NodeInstance(NodeInstance),             // Çalışan instance bilgisi
    DuplicateShred(DuplicateShredIndex, DuplicateShred), // Duplicate blok kanıtı
    SnapshotHashes(SnapshotHashes),         // Snapshot hash listesi
    // ...
}
```

### CRDS Yapısının Özellikleri

- **Last-write-wins:** Aynı key için her zaman yeni versiyon kazanır
- **Wallclock tabanlı:** Versiyonlama için monoton zaman damgası
- **İmzalı:** Her değer, sahibi tarafından imzalanmış
- **Bounded:** CRDS_UNIQUE_PUBKEY_CAPACITY=8192 ile sınırlı

---

## 14. Hesap Modeli ve AccountsDB

### Solana Hesap Modeli

Her şey "hesap"tır:
- Kullanıcı hesabı (SOL bakiyesi)
- Program (BPF bytecode)
- Program veri hesabı (program state)
- Mint (SPL Token tanımı)
- Token hesabı

```rust
pub struct AccountSharedData {
    lamports: u64,
    data: Vec<u8>,
    owner: Pubkey,    // Hesabı kontrol eden program
    executable: bool, // Program mı?
    rent_epoch: Epoch, // Rent borcunun ödeneceği epoch
}
```

### AccountsDB — Kalıcı Depolama

```
AccountsDB
├── AppendVec (her slot için ayrı dosya)
├── AccountsIndex (Pubkey → AppendVec konumu)
└── AccountsCache (hot accounts için RAM önbelleği)
```

**Snapshot mekanizması:**
- Full snapshot: Tüm hesap durumu (Genesis'ten itibaren)
- Incremental snapshot: Son full snapshot'tan değişenler

### Rent Mekanizması

```rust
// Rent exempt: min bakiye = rent.minimum_balance(data.len())
// Aksi halde her epoch rent tahsil edilir
// RENT_EXEMPT_RENT_EPOCH: rent_epoch = u64::MAX → muaf
```

---

## 15. Şifreleme ve İmza Altyapısı

### Kullanılan Kriptografi

| Bileşen | Algoritma |
|---------|-----------|
| İmza | Ed25519 (Schnorr üzeri) |
| Hash | SHA256 (PoH için), SHA512 (bazı iç hash'ler) |
| Ağ şifreleme | TLS 1.3 (QUIC üzerinde) |
| UDP | Düz (gossip/turbine — imzalanmış ama şifrelenmemiş) |
| BPF JIT | LLVM tabanlı |

### Ed25519 — Solana'nın Tercih Sebebi

- Küçük imza boyutu: 64 byte
- Hızlı doğrulama
- GPU'da toplu doğrulama yapılabilir (SigVerify stage)

### Precompile'lar

```rust
// Yerleşik program olarak uygulanan kriptografik işlemler:
pub fn get_precompiles() -> Vec<Precompile> {
    // ed25519_program: Ed25519 imza doğrulama precompile
    // secp256k1_program: Ethereum uyumlu imzalar için
}
```

---

## 16. Nova Shield için Çıkarımlar

### A. Zaman Damgası Mekanizması — PoH Uyarlaması

**Sorun:** Nova Shield, 400ms slot hedefliyor. Zaman damgası nasıl sağlanır?

**Solana çözümü:** PoH — sıralı SHA256 zinciri, kriptografik zaman kanıtı

**Nova Shield için:**
- PoH tam olarak alınmayabilir (validator ağı Solana gibi homojen değil)
- Ancak FIKIR alınabilir: Slot içi işlem sıralamayı BLS agregat imzasıyla damgala
- Alternatif: Mediandaki slot zamanı (TEE'den), 2/3 validator imzalı

### B. Tower BFT → Single-Slot Finality

**Solana:** Tower BFT optimistik konfirmasyon + 32-slot kesinleşme
**Nova Shield:** Avalanche Simplex BFT modeli → single-slot finality (2 round)

**Alınan fikir:** VOTE_THRESHOLD_DEPTH=8 ve lockout geometrisi
- Her validator oyunu sharde eder (BLS birleşik oy)
- Lockout yerine: Nova Shield'da BLS threshold imzası (3/5 validator imzaladı = kesinleşti)
- Avantaj: Lockout bekleme yok, slot sonunda hemen kesinleşme

### C. Lockout Geometrisi → Slashing Tasarımı

**Solana lockout:** `lockout = 2^confirmation_count` — üstel büyüme
**Nova Shield için:** Benzer slashing eğrisi:

```
İhlal cezası = base_slash × 2^(ihlalin_derinliği)
```

- Eski bir slot'ta double vote → küçük ceza (eski kural ihlali)
- Son slot'ta double vote → büyük ceza (aktif saldırı)

### D. Sealevel Paralel Yürütme → Nova VM

**Solana:** İşlem, okuma/yazma hesap listesini önceden beyan eder → paralel gruplandırma

**Nova Shield için:**
- Nova VM'de aynı ilke: Her işlem, "erişeceği state key'leri" beyan eder
- ZK tx için: Şifreli işlemler açılınca aynı analiz yapılır
- Enclave açma sonrası BatchAuction: Bağımsız tx'ler paralel çalışır

```rust
// Nova Shield işlem yapısı (fikir)
pub struct NovaTransaction {
    accounts: Vec<(AccountKey, AccessType)>, // ReadOnly / ReadWrite
    payload: EncryptedPayload,               // Şifreli içerik
    // ...
}
```

### E. Banking Stage Pipeline → Nova Shield Mempool Mimarisi

**Solana pipeline:**
```
Packet → SigVerify → DecisionMaker → Consumer/Forwarder → PoH
```

**Nova Shield şifreli mempool pipeline:**
```
EncryptedTx → DecryptRequest(shard) → Threshold Decrypt →
BatchCollect → SealedBidAuction → BLS Sign → Commit
```

- Consumer/Forwarder yerine: Threshold decrypt + Auction engine
- PoH kaydı yerine: BLS threshold imza + epoch block finalization

### F. Turbine Shred → Nova Shield Blok Yayma

**Solana:** Reed-Solomon erasure shred, stake-weighted tree

**Nova Shield için:**
- Aynı katmanlı tree yapısı (stake-weight)
- Shred boyutu: Solana 1280 byte → Nova Shield QUIC transport için uyarla
- Erasure coding: zfec veya reed-solomon → herhangi k/n validator bloğu yeniden üretebilir

### G. CRDS Gossip → Nova Shield P2P State

**Solana:** CRDS (Conflict-free Replicated Data Structure) — last-write-wins

**Nova Shield zaten:** libp2p GossipSub (Shutter'dan alındı)

**Eklenecek:** CRDS benzeri validator metadata propagasyonu:
- Epoch'taki DKG durumu (hangi shard hangi validatorda)
- Guardian atama duyuruları
- Heartbeat + uptime metrikleri

### H. VoteState Epoch Credits → Nova Shield Ödül Metrikleri

```rust
// Solana epoch_credits: Vec<(Epoch, credits, prev_credits)>
// Kümülatif oy performansı tracking

// Nova Shield için benzer yapı:
pub struct ValidatorMetrics {
    epoch_participation: Vec<(Epoch, signed_slots, total_slots)>,
    dkg_contributions: Vec<(Epoch, contributed, failed)>,
    uptime_score: f64,  // Avalanche'tan: >= %80 = ödül
}
```

### I. Gulf Stream → Nova Shield TX Pre-routing

**Solana:** Gelecek lider'ın pubkey'i bilinir → tx doğrudan oraya gider

**Nova Shield için:**
- Slot schedule bilinir (DPoS + VRF)
- Şifreli tx'ler gelecek lider'ın DKG threshold public key'i ile şifrelenir
- Lider slotu başladığında threshold decrypt açar

### J. Snapshot Mekanizması → Nova Shield State Synchronization

**Solana:** Full snapshot + Incremental snapshot

**Nova Shield için:**
- Epoch sonunda merkle trie root hash → tüm validatorlara BLS imzalı
- Yeni katılan validator: Son epoch snapshot → incremental sync
- Validator setine güven: Trie root'u imzalayan validatorlara güven

---

## Özet Tablo

| Bileşen | Solana Yaklaşımı | Nova Shield Kararı |
|---------|-----------------|-------------------|
| Zaman damgası | SHA256 PoH zinciri (VDF) | BLS agregat imzalı slot timestamp |
| Konsensüs | Tower BFT (lockout geometrisi) | Simplex BFT (Avalanche'tan) — 2 round finality |
| Slashing | Lockout ihlali → %100 stake | Geometrik ceza: 2^depth × base_slash |
| Paralel TX | Sealevel (hesap listesi beyan) | Nova VM: account key beyanı + paralel batch |
| TX Pipeline | Banking Stage (6 thread, 700K buffer) | Şifreli mempool + BatchAuction + decrypt pipeline |
| Blok yayma | Turbine shred (Reed-Solomon, stake-tree) | Benzer: libp2p + stake-weighted katman + erasure |
| Gossip | CRDS (last-write-wins, 8192 node cap) | GossipSub + CRDS benzeri validator metadata |
| TX yönlendirme | Gulf Stream (gelecek lider'a pre-route) | Gelecek lider DKG key ile şifreli pre-routing |
| Hesap modeli | Account-based (owner program) | Account-based + ZK hesap gizliliği katmanı |
| Snapshot | Full + Incremental AppendVec | Epoch merkle trie root + BLS imzalı incremental |
| İmza | Ed25519 | Dilithium3 (post-quantum, NIST FIPS 204) |
| Hash | SHA256 | BLAKE3 (daha hızlı, nova shield tercihi) |

---

## Nova Shield Teknik Kararlarına Katkısı

Bu analiz sonrasında **kesinleşen** ek kararlar:

1. **TX Beyan Zorunluluğu:** Nova Shield'da her işlem hangi state key'lere erişeceğini beyan etmek zorunda. Bu, Sealevel'in en güçlü fikri.

2. **Geometrik Slashing Eğrisi:** Lockout geometrisinden ilham alınarak: ihlal ne kadar "taze" ise ceza o kadar büyük.

3. **Stake-weighted Blok Yayma:** Turbine katmanlı ağacı, Nova Shield ağı için 1:1 uyarlanabilir.

4. **CRDS benzeri Gossip Metadata:** Validator DKG durumu, guardian atama ve uptime tracking için CRDS tipi yapı eklenmeli.

5. **Pipeline Mimarisi Benimsenecek:** Fetch → Decrypt → BatchAuction → Execute → BLS Finalize

---

*Analiz tamamlandı. Sıradaki: Repo 10 — polkadot-sdk (Rust)*
