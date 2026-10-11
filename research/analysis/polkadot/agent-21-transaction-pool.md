# Agent-21: substrate/client/transaction-pool Analizi

## Kapsanan Dosyalar

### API Katmani
- `api/src/lib.rs` — public TransactionPool trait, TransactionStatus enum, ChainEvent
- `api/src/error.rs` — Error enum, IntoPoolError, IntoMetricsLabel

### Graph (Cekirdek Motor)
- `src/graph/base_pool.rs` — BasePool, Transaction, Imported, PruneStatus, Limit
- `src/graph/ready.rs` — ReadyTransactions, TransactionRef, ReadyTx, BestIterator
- `src/graph/future.rs` — FutureTransactions, WaitingTransaction
- `src/graph/validated_pool.rs` — ValidatedPool, ValidatedTransaction, EventDispatcher
- `src/graph/pool.rs` — Pool, ChainApi trait, Options, verify pipeline
- `src/graph/rotator.rs` — PoolRotator, BanReason
- `src/graph/watcher.rs` — Watcher, Sender (per-tx durum streami)
- `src/graph/listener.rs` — EventDispatcher, EventHandler trait, MAX_FINALITY_WATCHERS
- `src/graph/tracked_map.rs` — TrackedMap (boyut izlemeli HashMap)
- `src/graph/mod.rs` — re-export

### Single-State Pool
- `src/single_state_txpool/single_state_txpool.rs` — BasicPool, RevalidationType, handle_enactment
- `src/single_state_txpool/revalidation.rs` — RevalidationQueue, RevalidationWorker, batch_revalidate
- `src/single_state_txpool/metrics.rs` — Prometheus metrikleri

### Fork-Aware Pool
- `src/fork_aware_txpool/fork_aware_txpool.rs` — ForkAwareTxPool, FINALITY_TIMEOUT_THRESHOLD, MEMPOOL_TO_VIEW_BATCH_SIZE
- `src/fork_aware_txpool/tx_mem_pool.rs` — TxMemPool, TxInMemPool, TXMEMPOOL_REVALIDATION_PERIOD, TXMEMPOOL_MAX_REVALIDATION_BATCH_SIZE
- `src/fork_aware_txpool/view.rs` — View, ImportedStatus, RevalidationResult
- `src/fork_aware_txpool/view_store.rs` — ViewStore
- `src/fork_aware_txpool/multi_view_listener.rs` — MultiViewListener
- `src/fork_aware_txpool/dropped_watcher.rs` — MultiViewDroppedWatcherController
- `src/fork_aware_txpool/import_notification_sink.rs` — MultiViewImportNotificationSink
- `src/fork_aware_txpool/revalidation_worker.rs` — RevalidationQueue (fork-aware)
- `src/fork_aware_txpool/metrics.rs` — EventsMetricsCollector

### Common / Yardimci
- `src/common/api.rs` — FullChainApi (runtime validate_transaction implementation)
- `src/common/enactment_state.rs` — EnactmentState, EnactmentAction
- `src/common/metrics.rs` — GenericMetricsLink, ApiMetrics
- `src/common/sliding_stat.rs` — SlidingStats, DurationSlidingStats, SyncSlidingStats
- `src/common/error.rs`
- `src/common/tracing_log_xt.rs`
- `src/common/mod.rs` — STAT_SLIDING_WINDOW sabiti
- `src/builder.rs` — TransactionPoolOptions, TransactionPoolType
- `src/lib.rs` — re-export, LOG_TARGET
- `src/transaction_pool_wrapper.rs`

### Test Dosyalari
- `tests/pool.rs` — single-state pool testleri
- `tests/fatp.rs` — fork-aware pool testleri
- `tests/fatp_prios.rs` — oncelik testleri
- `tests/fatp_limits.rs` — limit/eviction testleri
- `tests/fatp_finality_timeout.rs` — finality timeout testleri
- `tests/fatp_invalid.rs` — gecersiz transaction testleri
- `tests/fatp_common/mod.rs` — ortak test yardimcilari
- `tests/integration.rs`
- `tests/zombienet/`
- `benches/basics.rs`

---

## 1. Mimari Genel Bakis

Substrate transaction pool, iki farkli implementasyona sahip cift katmanli bir mimaridir:

```
                     TransactionPool trait (api/src/lib.rs)
                    /                              \
          BasicPool                         ForkAwareTxPool
    (SingleState)                           (Fork-Aware)
         |                                       |
    graph::Pool                            ViewStore
         |                                  (N Views)
    ValidatedPool                              |
         |                               graph::Pool (per-view)
    BasePool                                   |
    /       \                             ValidatedPool
ReadyTx  FutureQueue                          |
                                          BasePool
```

**`BasicPool` (single_state_txpool)**:
- Tek bir blockchain state'i referans alarak calisir
- `graph::Pool` uzerinde dogrudan oturur
- Reorg'da retract edilen bloklardan tx'leri yeniden submit eder
- `RevalidationType::Light` veya `Full` ile konfigure edilir

**`ForkAwareTxPool` (fork_aware_txpool)**:
- Her aktif fork/head icin ayri bir `View` (graph::Pool ornegi) tutar
- `TxMemPool` intermediary buffer: view yoksa, race condition'da, veya bazi fork'larda gecersiz olan tx'ler icin
- Her view ayri validasyon yapar => her fork icin farkli ready set
- `MultiViewListener`: cok sayida view'dan gelen tx durum olaylarini birlestirip tek stream'e donusturur

---

## 2. Temel Veri Yapilari

### 2.1 Transaction (graph/base_pool.rs:125-145)

```rust
pub struct Transaction<Hash, Extrinsic> {
    pub data: Extrinsic,        // ham extrinsic verisi
    pub bytes: usize,           // encoding uzunlugu (limit kontrolu icin)
    pub hash: Hash,             // benzersiz kimlik
    pub priority: Priority,     // u64 — yuksek = daha iyi
    pub valid_till: Longevity,  // hangi blok numarasina kadar gecerli
    pub requires: Vec<Tag>,     // gereksinim etiketleri
    pub provides: Vec<Tag>,     // saglanan etiketler
    pub propagate: bool,        // networke yayilsin mi
    pub source: TimedTransactionSource, // kaynak + zaman damgasi
}
```

**Onemli**: `valid_till = current_block_number + longevity` seklinde hesaplanir.
Kaynak: `validated_pool.rs:83`: `valid_till: at.saturated_into::<u64>().saturating_add(validity.longevity)`

### 2.2 TimedTransactionSource (graph/base_pool.rs:88-122)

```rust
pub struct TimedTransactionSource {
    pub source: TransactionSource,   // InBlock | External | Local
    pub timestamp: Option<Instant>,
}
```

`timestamp`, future pool eviction stratejisi icin kullanilir: en eski (en kucuk timestamp) once atilir.

### 2.3 TransactionRef (graph/ready.rs:42-47)

```rust
pub struct TransactionRef<Hash, Ex> {
    pub transaction: Arc<Transaction<Hash, Ex>>,
    pub insertion_id: u64,    // her insert'te artan monoton ID
}
```

**Siralama kuralları** (`Ord` impl, ready.rs:56-63):
1. `priority` DESC (yuksek oncelik daha iyi)
2. `valid_till` ASC (daha az omru kalan once)
3. `insertion_id` DESC (daha eski olan once)

### 2.4 ReadyTx (graph/ready.rs:78-89)

```rust
pub struct ReadyTx<Hash, Ex> {
    pub transaction: TransactionRef<Hash, Ex>,
    pub unlocks: Vec<Hash>,       // bu tx tamamlaninca kilidini acabilecegi tx'ler
    pub requires_offset: usize,   // harici olarak saglanan gecmis gereksinimler
}
```

### 2.5 WaitingTransaction (graph/future.rs:33-40)

```rust
pub struct WaitingTransaction<Hash, Ex> {
    pub transaction: Arc<Transaction<Hash, Ex>>,
    pub missing_tags: HashSet<Tag>,   // hala karsilanmamis gereksinimler
    pub imported_at: Instant,         // future queue'ya giris zamani
}
```

### 2.6 BasePool (graph/base_pool.rs:249-259)

```rust
pub struct BasePool<Hash, Ex> {
    reject_future_transactions: bool,
    future: FutureTransactions<Hash, Ex>,
    ready: ReadyTransactions<Hash, Ex>,
    recently_pruned: [HashSet<Tag>; RECENTLY_PRUNED_TAGS],  // [HashSet; 2]
    recently_pruned_index: usize,
}
```

**`recently_pruned`**: Son 2 prune round'inda silinen tag'leri tutar. Yeni gelen tx'in `requires` tag'lerinin bu sette olup olmadigini kontrol ederek, yeni validate edilen bir tx'in yanlislikla future'a gitmesini onler. Kaynak: `base_pool.rs:236`, `RECENTLY_PRUNED_TAGS = 2`.

### 2.7 ReadyTransactions (graph/ready.rs:110-121)

```rust
pub struct ReadyTransactions<Hash, Ex> {
    insertion_id: u64,
    provided_tags: HashMap<Tag, Hash>,      // tag -> hangi tx sagliyor
    ready: TrackedMap<Hash, ReadyTx<...>>,  // hash -> ReadyTx
    best: BTreeSet<TransactionRef<...>>,    // bagimsiz tx'ler (hicbir beklentisi olmayan)
}
```

`best` BTreeSet: sadece tum gereksinimleri karsilanmis ve baska tx'e bagimli olmayan tx'leri icinde tutar. Iterator buradan baslar.

### 2.8 FutureTransactions (graph/future.rs:112-117)

```rust
pub struct FutureTransactions<Hash, Ex> {
    wanted_tags: HashMap<Tag, HashSet<Hash>>,  // beklenen tag -> bekleyen tx hash setleri
    waiting: HashMap<Hash, WaitingTransaction<...>>,
}
```

### 2.9 ValidatedPool (graph/validated_pool.rs:163-173)

```rust
pub struct ValidatedPool<B: ChainApi, L: EventHandler<B>> {
    api: Arc<B>,
    is_validator: IsValidator,
    options: Options,
    event_dispatcher: RwLock<EventDispatcher<B, L>>,
    pub(crate) pool: RwLock<BasePool<...>>,
    import_notification_sinks: Mutex<Vec<Sender<ExtrinsicHash<B>>>>,
    rotator: PoolRotator<ExtrinsicHash<B>>,
    enforce_limits_stats: SyncDurationSlidingStats,
}
```

### 2.10 PoolRotator (graph/rotator.rs:60-67)

```rust
pub struct PoolRotator<Hash> {
    ban_time: Duration,
    banned_until: RwLock<HashMap<Hash, (Instant, BanReason)>>,
    expected_size: usize,
}
```

`BanReason` enum (rotator.rs:43-53):
- `Validation` — runtime validasyon hatasi; mempool revalidation ile kaldirilabilir
- `LimitsEnforced` — kapasite kararı; **hicbir zaman otomatik kaldirilmaz**

### 2.11 ValidatedTransaction (graph/validated_pool.rs:53-62)

```rust
pub enum ValidatedTransaction<Hash, Ex, Error> {
    Valid(base::Transaction<Hash, Ex>),
    Invalid(Hash, Error),
    Unknown(Hash, Error),
}
```

`Invalid` → ban + error
`Unknown` → sadece listener notification, ban yok

### 2.12 TxInMemPool (fork_aware_txpool/tx_mem_pool.rs:114-139)

```rust
pub(crate) struct TxInMemPool<ChainApi, Block> {
    watched: bool,
    tx: ExtrinsicFor<ChainApi>,
    bytes: usize,
    source: TimedTransactionSource,
    validated_at: AtomicU64,     // son revalidasyon blogu
    priority: RwLock<Option<TransactionPriority>>,
    needs_unban: atomic::AtomicBool,  // view'lardan dusunce unban gerekebilir
}
```

### 2.13 Options (graph/pool.rs:152-172)

```rust
pub struct Options {
    pub ready: base::Limit { count: usize, total_bytes: usize },
    pub future: base::Limit { count: usize, total_bytes: usize },
    pub reject_future_transactions: bool,
    pub ban_time: Duration,
}
```

**Default degerleri** (pool.rs:164-171):
```rust
ready:  Limit { count: 8192, total_bytes: 20 * 1024 * 1024 }   // 20 MB
future: Limit { count: 512,  total_bytes: 1 * 1024 * 1024 }    // 1 MB
ban_time: Duration::from_secs(60 * 30)                          // 30 dakika
```

---

## 3. Transaction Validasyon Akisi

### 3.1 Asenkron Submit Yolu (Pool::submit_at → verify → validated_pool.submit)

```
Dis cagri: submit_at(at, xts)
    |
    v
Pool::submit_at (pool.rs:233-242)
    |
    v
Pool::verify (pool.rs:505-522)
    |-- join_all: paralel verify_one cagrilari
    |
    v
Pool::verify_one (pool.rs:525-582)
    |
    |--1. hash_and_length(&xt) ile hash hesapla
    |
    |--2. ValidatedPool::check_is_known(hash, ignore_banned)
    |       banned? -> TemporarilyBanned hatasi
    |       zaten imported? -> AlreadyImported hatasi
    |
    |--3. ChainApi::validate_transaction(block_hash, source, xt)
    |       runtime'a WASM cagri yapilir
    |       Ok(Ok(validity)) -> gecerli
    |       Ok(Err(Invalid(e))) -> Invalid hata
    |       Ok(Err(Unknown(e))) -> Unknown hata
    |       Err(e) -> Invalid hata (runtime hatasi)
    |
    |--4. validity.provides bos mu? -> NoTagsProvided hatasi (KRITIK)
    |
    |--5. ValidatedTransaction::valid_at() olustur
    |       valid_till = block_number + validity.longevity
    |
    v
ValidatedPool::submit (validated_pool.rs:306-340)
    |
    |-- submit_one() her tx icin
    |       propagate=false AND not_validator -> Unactionable hatasi
    |       pool.write().import(tx) -- BasePool::import
    |       ready => import_notification_sinks'e gonder
    |       fire_events(event_dispatcher, &imported)
    |
    |-- enforce_limits() en az 1 basarili import varsa
    |       limits asildiysa -> worst tx'leri remove
    |       ban removed tx'leri: LimitsEnforced nedenle
    |       removed.contains(hash)? -> ImmediatelyDropped hatasi
    |
    v
Sonuc: Vec<Result<ValidatedPoolSubmitOutcome, Error>>
```

### 3.2 `submit_and_watch` ile `submit` Farki

`submit_and_watch` (pool.rs:274-291):
1. `verify_one` cagrilir (ayni validasyon)
2. `ValidatedPool::create_watcher(hash)` ile watcher olusturulur
3. `ValidatedPool::submit_and_watch(tx)` cagrilir
4. submit icinde watcher set edilir: `outcome.with_watcher(watcher)`
5. Caller bir `TransactionStatusStream` alir — bunu bekleyerek tx durumunu takip eder

`submit` (plain):
- Watcher olusturulmaz
- Sadece hash return edilir
- Durum takibi yapilmaz

**Temel fark**: `submit_and_watch` bir subscription stream dondurur; `submit` sadece hash dondurur. Her iki yol da ayni validasyon ve pool insert mantigi kullanir.

### 3.3 Blocking Submit (LocalTransactionPool::submit_local)

`BasicPool::submit_local` (single_state_txpool.rs:479-514):
- `validate_transaction_blocking()` — mevcut thread'i bloklar, WASM call
- Sadece `TransactionSource::Local` kaynak kullanilir
- Offchain worker extrinsics icin tasarlanmistir

---

## 4. Ready Pool Mekanizmasi

### 4.1 ReadyTransactions::import (ready.rs:178-233)

```
import(tx: WaitingTransaction)
    |
    |--1. assert: tx.is_ready() (missing_tags bos olmali)
    |
    |--2. insertion_id += 1
    |
    |--3. replace_previous(&transaction)
    |       provides tag'leri cakisan eski tx'leri bul
    |       eski tx'lerin toplam priority'si >= yeni tx.priority?
    |           -> TooLowPriority hatasi
    |       degil? -> eski tx subgraph'ini kaldir (remove_subtree_with_tag_filter)
    |       unlocks listesini topla
    |
    |--4. requires icin provided_tags kontrol
    |       her required tag icin:
    |           provided_tags'ta var mi?
    |               -> saglayan tx'in unlocks listesine hash ekle
    |               -> goes_to_best = false
    |           yok (zaten pruned/external)?
    |               -> requires_offset += 1
    |
    |--5. provided_tags guncelle (tag -> this.hash)
    |
    |--6. goes_to_best? -> best BTreeSet'e ekle
    |
    |--7. ready map'e ekle: ReadyTx { transaction, unlocks, requires_offset }
```

### 4.2 BestIterator (ready.rs:484-578)

`ReadyTransactions::get()` cagrisinda snapshot alinir:
- `all`: ready map'in klonu (HashMap<Hash, ReadyTx>)
- `best`: current best BTreeSet klonu
- `awaiting`: HashMap<Hash, (satisfied_count, TransactionRef)>

Iterator mantigi:
```
next():
    loop:
        best BTreeSet'ten en yuksek priority tx'i al (next_back)
        invalid sette mi? -> devam et (skip)
        all'da var mi? -> yoksa devam et
        tx'in unlocks listesi:
            her hash icin:
                awaiting'te var mi?
                    -> satisfied += 1
                    -> satisfied >= requires.len()? -> best'e ekle, yoksa awaiting'te birak
                yok? all'dan al:
                    -> requires_offset + 1 >= requires.len()? -> best'e ekle
        tx'i return et
```

**Siralama**: Bagimliliklara saygi gosterilerek, icerisinde oncelik sirali (yuksek oncelik once). Bagimsiz tx'ler arasi kesin siralama: priority DESC, valid_till ASC, insertion_id DESC.

### 4.3 report_invalid (ready.rs:520-531)

Block builder gecersiz bir tx buldugunda:
```rust
pub fn report_invalid(&mut self, tx: &Arc<Transaction<Hash, Ex>>) {
    if let Some(to_report) = self.all.get(&tx.hash) {
        for hash in &to_report.unlocks {
            self.invalid.insert(hash.clone());
        }
    }
}
```
Sadece iterator'un o anda tuttuklari `invalid` setine eklenir. Pool'daki veriye dokunulmaz — sadece bu iterator instance'i etkilenir. Sonraki `next()` cagrisi bu tx'leri atlar.

---

## 5. Future Pool Mekanizmasi

### 5.1 FutureTransactions Yapisi

Iki map:
- `waiting: HashMap<Hash, WaitingTransaction>` — tx detaylari
- `wanted_tags: HashMap<Tag, HashSet<Hash>>` — hangi tag'i hangi tx'ler bekliyor

### 5.2 Future'a Import (future.rs:141-156)

```
import(tx: WaitingTransaction)
    |
    |-- assert: !tx.is_ready() (missing_tags bos olmamali)
    |-- assert: henuz import edilmemis
    |
    |-- her missing_tag icin:
    |       wanted_tags[tag].insert(tx.hash)
    |
    |-- waiting.insert(tx.hash, tx)
```

### 5.3 Ready'e Yukseltme (future.rs:195-219)

`satisfy_tags(tags)` — genellikle bir tx ready pool'a girdiginde provides etiketleriyle cagrilir:

```
for tag in tags:
    hashes = wanted_tags.remove(tag)?
    for hash in hashes:
        tx = waiting[hash]
        tx.satisfy_tag(tag)     // missing_tags.remove(tag)
        if tx.is_ready():
            became_ready.push(waiting.remove(hash))
return became_ready
```

Bu tx'ler BasePool::import_to_ready'e gonderilir.

### 5.4 Future'dan Pruning (future.rs:173-188)

`prune_tags(tags)`:
- `waiting` icinde `provides` tag'leri verilen etiketleri icerenleri bulup siler
- Mantik: bloga dahil edilen bir tx, bloktaki tx ile ayni tag'i saglayan future tx'leri gecersiz kilar

### 5.5 Future Eviction Stratejisi

`BasePool::enforce_limits` (base_pool.rs:504-534):
```rust
let worst = self.future.fold(|worst, current| {
    match (worst.transaction.source.timestamp, current.transaction.source.timestamp) {
        (Some(worst_ts), Some(current_ts)) => {
            if worst_ts > current_ts { current.clone() } else { worst }
        },
        _ => {
            if worst.imported_at > current.imported_at { current.clone() } else { worst }
        },
    }
});
```

**Future eviction kuralı**: Timestamp varsa en eski timestamp; yoksa `imported_at` ile karsılastirma — her zaman en eski (en erken) future tx once atılır.

---

## 6. Transaction Bagimlilık Grafigi

### 6.1 Tag Sistemi

Her tx iki set tag tanimlar:
- `provides: Vec<Tag>` — hangi "slotlari" doldurduğunu beyan eder
- `requires: Vec<Tag>` — hangi "slotlarin" onceden doldurulmasi gerektigini talep eder

Tipik kullanim (pallet-balances ornegi):
- `provides` = [(account_id, nonce)] — bu nonce'u dolduruyor
- `requires` = [(account_id, nonce-1)] — bir onceki nonce'u gerektiriyor

**Kritik kural**: `provides` bos ise tx **reddedilir** (NoTagsProvided). Kaynak: `pool.rs:560-562`.

### 6.2 Bağımlılık Grafiği Yapısı

Ready pool'da her tx icin `unlocks: Vec<Hash>` tutulur. Bu, "bu tx provide ettiginde hangi tx'ler hazir hale gelir" listesidir.

`provides_tags: HashMap<Tag, Hash>` — her tag'i hangi ready tx'in sagladigini gosterir.

### 6.3 replace_previous: Cakisma Cozumu (ready.rs:419-471)

Yeni bir tx, mevcut tx ile ayni tag'i saglıyorsa:
1. Cakisan tx'lerin **toplam** priority'si hesaplanir (saturating_add)
2. Yeni tx'in priority > toplam eski priority? -> devam
3. Degil? -> `TooLowPriority` hatasi
4. Eski tx'in subgraph'i silinir (provides_tag_filter ile sadece yeni tx'in saglamadigı tag'ler etkilenir)
5. Sonuc: eski tx "usurped" olur

### 6.4 Cycle Tespiti (base_pool.rs:415-426)

`import_to_ready` icinde:
```rust
if removed.iter().any(|tx| tx.hash == tx_hash) {
    self.ready.remove_subtree(&promoted);
    return Err(error::Error::CycleDetected);
}
```

Bir tx kendisini promote etmek icin gereken tx'leri displayer olursa (cycle), `CycleDetected` hatasi donerken, promote edilmis subgraph tamamen temizlenir.

### 6.5 Prune Sonrasi Promotion

`BasePool::prune_tags` (base_pool.rs:564-606):
1. Future pool'dan silinen tx'ler (provides silinen tag'leri icerenleri)
2. Her tag icin: `future.satisfy_tags` — ilgili future tx'leri aktif hale getir
3. Ready pool'dan prune (sadece provide eden tx ve bagimlilari)
4. Yeni hazir olan tx'leri ready'ye import et (`import_to_ready`)
5. `recently_pruned` guncelle (son 2 round'luk ring buffer)

---

## 7. Pruning ve Eviction Stratejileri

### 7.1 Tag-Based Pruning (Blok dahil edilmesinde)

`Pool::prune` (pool.rs:331-421):
1. Blok icindeki extrinsic'lerin hash'leri hesaplanir
2. Pool'dakilerin tag'leri alinir (runtime cagri olmadan)
3. Pool'da olmayanlar icin parent block'ta runtime validate_transaction cagrilir
4. Toplanan `future_tags` ile `Pool::prune_tags` cagrilir
5. `prune_tags`:
   - Tag'leri saglayan tx'leri ve bagimlılarını kaldir
   - Blokta olan tx'leri gecici olarak ban'la (yeniden import'u onlemek icin)
   - Pruned tx'leri reverify et (runtime cagri)
   - Gecerli olanları yeniden submit et

### 7.2 Stale (Kullanim Suresi Dolmus) Tx Temizligi

`ValidatedPool::clear_stale` (validated_pool.rs:720-751):
```rust
fn clear_stale(&self, at: &HashAndNumber<B::Block>) {
    let number = number.saturated_into::<u64>();
    // ready ve future'dan valid_till <= current_block olanlari bul
    // ban_if_stale: valid_till > current_block? false : ban + true
    self.remove_invalid(&to_remove);
    self.remove_invalid(&futures_to_remove);
    self.rotator.clear_timeouts(&now);
}
```

`ban_if_stale` (rotator.rs:142-154): `xt.valid_till > current_block` ise false (hala gecerli); değilse ban + true.

### 7.3 Limit Zorlamasi (enforce_limits)

`BasePool::enforce_limits` (base_pool.rs:463-537):

**Ready pool eviction** (count veya bytes asildiysa):
```rust
// worst tx bulma: priority ASC, sonra insertion_id DESC (yeni olanlar once atilir)
// NOT: BestIterator'dan FARKLI siralama — amacli tasarim
let worst = self.ready.fold(...) {
    match priority.cmp(other.priority) {
        Less => worst,
        Equal => if worst.insertion_id > current.insertion_id { current } else { worst },
        Greater => current,
    }
};
remove_subtree(&[worst.hash])
```

**KRITIK FARK**: `enforce_limits`'ta ready pool eviction icin **yeni eklenen tx'ler once atilir** (ayni priority'de yuksek insertion_id = daha yeni = once gitsin). BestIterator'da ise tam tersi: eski tx'ler once iterasyonda onemsenir.

**Future pool eviction**: En eski (en erken imported_at / timestamp) once atilir.

### 7.4 Ban Listesi Yonetimi

Ban: `now + ban_time` seklinde expire zamani set edilir.

**Garbage collection** (rotator.rs:113-119):
```rust
if banned.len() > 2 * expected_size {
    while banned.len() > expected_size {
        // ilk eklenen elemani sil (HashMap keyler sirasinda)
        banned.remove(&key);
    }
}
```

`expected_size` default: `ready.count + future.count` (Options::total_count())
Trigger: `banned.len() > 2 * expected_size`
Sonuc: `banned.len()` == `expected_size`'a indirilir

**DEFAULT_EXPECTED_SIZE** (statically sized rotator): 2048 (rotator.rs:34)

`LimitsEnforced` ban vs `Validation` ban:
- `Validation` ban: `unban_if_validation()` ile mempool revalidation tarafindan kaldirilabilir
- `LimitsEnforced` ban: **hicbir zaman otomatik kaldirilmaz** — kapasite karari

---

## 8. Watcher/Notification Sistemi

### 8.1 Watcher (graph/watcher.rs)

```rust
pub struct Watcher<H, BH> {
    receiver: TracingUnboundedReceiver<TransactionStatus<H, BH>>,
    hash: H,
}
pub struct Sender<H, BH> {
    receivers: Vec<TracingUnboundedSender<TransactionStatus<H, BH>>>,
    is_finalized: bool,
}
```

`tracing_unbounded("mpsc_txpool_watcher", 100_000)` — 100,000 kapasiteli unbounded kanal.

Bir hash icin birden fazla watcher olabilir (`receivers: Vec<...>`).

`Sender::is_done()` (watcher.rs:133-135):
```rust
self.is_finalized || self.receivers.is_empty()
```
is_finalized = true yapan event'lar: Usurped, Finalized, FinalityTimeout, Invalid, Dropped

### 8.2 EventDispatcher (graph/listener.rs:74-82)

```rust
pub struct EventDispatcher<H, C: ChainApi, L: EventHandler<C>> {
    watchers: HashMap<H, watcher::Sender<H, BlockHash<C>>>,
    finality_watchers: LinkedHashMap<ExtrinsicHash<C>, Vec<H>>,
    event_handler: Option<L>,
}
```

`finality_watchers: LinkedHashMap<BlockHash, Vec<TxHash>>`:
- Insertion-order koruyan linked list + map
- Hangi blokta hangi tx'lerin izlendigini tutar
- `MAX_FINALITY_WATCHERS = 512` (listener.rs:85)

### 8.3 Finality Timeout Mekanizmasi

`EventDispatcher::pruned` (listener.rs:220-243):
```rust
let txs = self.finality_watchers.entry(block_hash).or_insert(vec![]);
txs.push(*tx_hash);
let tx_index = txs.len() - 1;
self.fire(tx_hash, |watcher| watcher.in_block(block_hash, tx_index));

while self.finality_watchers.len() > MAX_FINALITY_WATCHERS {
    if let Some((hash, txs)) = self.finality_watchers.pop_front() {
        for tx in txs {
            self.fire(&tx, |watcher| watcher.finality_timeout(hash));
        }
    }
}
```

**`MAX_FINALITY_WATCHERS = 512`**: Bekleyen finalizasyon blogu sayisi bu limiti asarsa, en eski bloktan baslayarak `FinalityTimeout` eventi gonderilir.

API doc'ta "512 block" yaziyor (api/src/lib.rs:109): _"The `FinalityTimeout` event will be emitted when the block did not reach finality within 512 blocks."_

### 8.4 TransactionStatus Durumu Akisi

```
                    [Future] <--+
                        |       |
                        v       |
                    [Ready] ----+
                        |
                   [Broadcast] (optional, informational)
                        |
              +---------+---------+---------+---------+
              |         |         |         |         |
         [InBlock]  [Usurped] [Dropped] [Invalid]  [Retracted]
              |                                         |
        [Finalized] --------------------------> [Ready/Future]
         veya
       [FinalityTimeout]
```

Final durumlar (stream burada biter): `Usurped`, `Finalized`, `FinalityTimeout`, `Invalid`, `Dropped`.

### 8.5 import_notification_stream

`ValidatedPool::import_notification_stream` (validated_pool.rs:762-768):
```rust
const CHANNEL_BUFFER_SIZE: usize = 1024;
let (sink, stream) = channel(CHANNEL_BUFFER_SIZE);
self.import_notification_sinks.lock().push(sink);
stream
```

Kapasite **1024** — sadece ready queue'ya giren tx'ler bildirilir (future'a girenler bildirilmez). Full oldugunda uyari logu verilir, kanal drop edilmez.

---

## 9. Kritik Sayisal Degerler ve Sabitler

### Graph / BasePool

| Sabit | Deger | Konum |
|-------|-------|-------|
| `RECENTLY_PRUNED_TAGS` | 2 | base_pool.rs:236 |
| Default ready count | 8192 | pool.rs:166 |
| Default ready bytes | 20 * 1024 * 1024 = 20 MB | pool.rs:166 |
| Default future count | 512 | pool.rs:167 |
| Default future bytes | 1 * 1024 * 1024 = 1 MB | pool.rs:167 |
| Default ban_time | 30 * 60 = 1800 saniye (30 dakika) | pool.rs:169 |

### PoolRotator

| Sabit | Deger | Konum |
|-------|-------|-------|
| `DEFAULT_BAN_TIME_SECS` | 30 * 60 = 1800 saniye | rotator.rs:37 |
| `DEFAULT_EXPECTED_SIZE` (static rotator) | 2048 | rotator.rs:34 |
| GC trigger threshold | 2 * expected_size | rotator.rs:113 |
| GC hedef boyut | expected_size | rotator.rs:114 |

### EventDispatcher / Watcher

| Sabit | Deger | Konum |
|-------|-------|-------|
| `MAX_FINALITY_WATCHERS` | 512 | listener.rs:85 |
| Watcher kanal kapasitesi | 100_000 | watcher.rs:65 |
| import_notification CHANNEL_BUFFER_SIZE | 1024 | validated_pool.rs:763 |

### Single-State Pool / Revalidasyon

| Sabit | Deger | Konum |
|-------|-------|-------|
| `BACKGROUND_REVALIDATION_INTERVAL` | 200 ms | revalidation.rs:38 |
| `MIN_BACKGROUND_REVALIDATION_BATCH_SIZE` | 20 | revalidation.rs:40 |
| revalidasyon batch boyutu | max(20, members.len() / 4) | revalidation.rs:181 |
| Periyodik revalidasyon tetikleyici | 60 saniye VEYA 20 blok | single_state_txpool.rs:667-669 |

### Fork-Aware Pool

| Sabit | Deger | Konum |
|-------|-------|-------|
| `FINALITY_TIMEOUT_THRESHOLD` | 128 blok | fork_aware_txpool.rs:84 |
| `MEMPOOL_TO_VIEW_BATCH_SIZE` | 7_000 tx | fork_aware_txpool.rs:90 |
| `TXMEMPOOL_REVALIDATION_PERIOD` | 10 blok | tx_mem_pool.rs:73 |
| `TXMEMPOOL_MAX_REVALIDATION_BATCH_SIZE` | 1000 tx | tx_mem_pool.rs:76 |

### EnactmentState

| Sabit | Deger | Konum |
|-------|-------|-------|
| `SKIP_MAINTENANCE_THRESHOLD` | 20 blok | enactment_state.rs:31 |

### Builder / Yapilandirma

| Deger | Aciklama | Konum |
|-------|----------|-------|
| future.count = pool_limit / 10 | future queue = ready queue'nun 1/10'u | builder.rs:82 |
| future.total_bytes = pool_bytes / 10 | ayni oran | builder.rs:83 |
| dev mode'da ban_time | 0 saniye (ban yok) | builder.rs:88 |
| Benchmark ready/future | 100_000 / 100 MB | builder.rs:100-107 |

### Metrics / Istatistik

| Sabit | Deger | Konum |
|-------|-------|-------|
| `STAT_SLIDING_WINDOW` | 3 saniye | common/mod.rs:34 |

---

## 10. Test Analizi

### 10.1 base_pool.rs Birim Testleri

**`should_import_transaction_to_ready`**: `requires=[]` olan tx dogrudan ready'e gider.

**`should_import_transaction_to_future_and_promote_it_later`**: nonce=1 tx once future'a, sonra nonce=0 tx ready'ye girince nonce=1 de ready'e yukselir.

**`should_promote_a_subgraph`**: Tek bir tx provide edince tum bagimlilık zinciri (subgraph) ready'e yukseltilir. Test sirasi: tx5 (bagimsiz), tx1, tx2, tx4, tx3.

**`should_handle_a_cycle`**: tx1->tx3->tx2->tx1 dongusu. Donguyu kapatan tx4 (yeterli priority ile) gelince tx2 replace edilir, tx1+tx3 ready'e girer.

**`should_handle_a_cycle_with_low_priority`**: Ayni dongu ama dusuk priority'li tx4 -> `CycleDetected` hatasi, future queue temizlenir.

**`should_remove_invalid_transactions`**: `remove_subtree([6, 1])` -> tx1'e bagli tx2, tx3, tx4 de silinir. Sadece tx5 kalir.

**`should_reject_future_transactions`**: `reject_future_transactions = true` iken future'a gidecek tx `RejectedFutureTransaction` hatasi verir.

**`should_accept_future_transactions_when_explicitly_asked_to`**: `with_futures_enabled` closure sonrasinda flag eski haline doner.

### 10.2 ready.rs Birim Testleri

**`should_order_refs`**: TransactionRef siralama dogrulama:
- Higher priority > Lower priority
- Lower valid_till > Higher valid_till (daha az omrü kalan once)
- Lower insertion_id > Higher insertion_id (daha eski once)

**`should_return_best_transactions_in_correct_order`**: Karmasik bagimlılık grafigi (tx1->tx2->tx3, tx1->tx4->tx5->tx6, tx1->tx7) icin beklenen siralama: 1, 2, 3, 4, 5, 6, 7.

**`should_skip_invalid_transactions_while_iterating`**: tx4 `report_invalid` edilince tx5, tx6 atlaniyor; tx7 hala gozukuyor (ayri branch).

**`should_replace_transaction_that_provides_the_same_tag`**: tx2+tx3 toplam priority=2, tx1.priority=1 -> TooLowPriority. tx1.priority=10 -> replace basarili.

**`should_remove_tx_from_unlocks_set_of_its_parent`**: tx2 silinince tx1'in unlocks listesinden tx2 ve tx3 kaldirilir, tx4 hala orada.

### 10.3 pool.rs (Pool) Testleri

**`should_notify_about_pool_events`**: import_notification_stream sadece ready'e giren tx'leri bildirir (future'daki tx bildirilmez). Stream, sink'ler dusunce biter.

**`should_clear_stale_transactions`**: `clear_stale(block 5)` -> valid_till=64 olan tum tx'ler (block 5 <= 64 degil, ancak testde farkli davranabilir) banlanir; test dogruluyor ki `is_banned() == true`.

**`should_ban_mined_transactions`**: `prune_tags` sonrasi bilinen imported hash'ler geçici olarak banlanir.

**`should_limit_futures`**: future limit = 1 tx byte'i. 2. tx eklenince ilki duser (LimitsEnforced ban), ikincisi banlı degil.

**`should_trigger_dropped_older`**: limit=1 tx. Bob'un tx'i gelince Alice'in tx'i (daha eski insertion_id) `Dropped` eventi alir.

**`should_handle_pruning_in_the_middle_of_import`**: Race condition testi: tx1 (nonce=1) validate edilirken tx0 (nonce=0) import edilip prune edilirse, tx1 dogrudan ready'e gider (`recently_pruned` mekanizması sayesinde).

### 10.4 EnactmentState Testleri

`SKIP_MAINTENANCE_THRESHOLD = 20` blok fark asilinca `EnactmentAction::Skip` doner.

Sira onemli: Finalize olmus blok retract edilemez. Dogru siralama: NBB sonra Finalize veya Finalize sonra NBB (ikisinde de cift islem olmaz).

---

## 11. Nova Shield Chain Icin Cikarsamlar

### 11.1 Ayri Pool Implementasyonu mu, Yoksa Entegrasyon mu?

Polkadot'un transaction pool'u Rust trait-based ve genellikle `FullChainApi` ile runtime'a baglidir. Nova Shield Chain'de:
- **Tavsiye**: `sc_transaction_pool_api::TransactionPool` trait'ini implemente eden ozel bir pool yazmak yerine, `BasicPool` veya `ForkAwareTxPool`'u `ChainApi` trait implementasyonu yoluyla kullanmak.
- MPC threshold signature tx'leri: `provides/requires` tag mekaniği ile sıralı imzalama oturumlari modellenebilir.

### 11.2 Tag Tasarimi (provides/requires)

Nova Shield Chain icin tag semantigi:
```
Tag = (account_address || sequence_number) [u8]
```

Her tx:
- `provides = [(sender, nonce)]`
- `requires = [(sender, nonce-1)]` (ilk tx icin bos)

MPC multisig tx'leri:
- `provides = [(multisig_address, session_id)]`
- `requires = [(multisig_address, session_id - 1)]` veya bos (ilk oturum)

**Kritik**: `provides` **bos birakilamaz** — `NoTagsProvided` hatasi alınır ve tx reddedilir.

### 11.3 Priority Stratejisi

Nova Shield Chain icin onerilir:
```rust
priority = fee / tx_byte_size   // fee-per-byte bazli oncelik
```

veya daha basit:
```rust
priority = base_fee + priority_fee  // gas benzeri
```

**Usurpation**: Ayni tag saglayan tx replace icin yeni tx'in priority'si eski tx'in priority'sinden yuksek olmali. Bu "fee bump" mekanizmasini saglar.

### 11.4 Longevity/valid_till Kullanimi

```rust
valid_till = current_block + validity.longevity
```

Nova Shield Chain icin:
- **Normal tx**: longevity = 64 blok (yaklaşık)
- **Acil/system tx**: longevity = u64::MAX
- **Zaman sinirli MPC oturumları**: longevity = oturum_bitis_bloku - current_block

`clear_stale` her blok maintain'de cagrilir; `valid_till <= current_block` olan tx'ler otomatik banlanir.

### 11.5 Ban Mekanizması

**Default ban suresi: 30 dakika** — production icin uygun.

Dev/test modunda: `ban_time = 0` (ban yok) — `TransactionPoolOptions::new_with_params` ile konfigure edilebilir.

**`LimitsEnforced` ban dikkat**: Pool doldugunda atilan tx'ler hic otomatik geri gelmez; kullanicinin yeniden submit etmesi gerekir. Bu nova-shield'de kullanici deneyimi acisindan onemli bir edge case.

### 11.6 Fork-Aware vs Single-State Secimi

| Kriter | SingleState (BasicPool) | ForkAware |
|--------|------------------------|-----------|
| Kucuk/simple chain | Tercih edilir | Gereksizsizdir |
| Cok fork beklenen proof-of-stake | Yetersiz | Gerekli |
| Memory kullanimi | Dusuk | Yuksek (N view) |
| Complexity | Dusuk | Yuksek |
| Reorg handling | Temel | Gelismis |

**Nova Shield Chain icin tavsiye**: Ilk release'de `BasicPool` (SingleState) kullanmak, sonra `ForkAwareTxPool`'a gecis yapmak. MPC-based consensus'ta cok az fork beklendigi icin `BasicPool` yeterli olabilir.

### 11.7 Limit Yapilandirmasi

Onerilen production degerleri:
```rust
Options {
    ready: Limit { count: 8192,  total_bytes: 20 * 1024 * 1024 },  // 20 MB
    future: Limit { count: 512,  total_bytes: 1 * 1024 * 1024 },   // 1 MB
    ban_time: Duration::from_secs(30 * 60),
    reject_future_transactions: false,
}
```

**Future limit = 10x kucuk**: Builder'da `future.count = pool_limit / 10`. Bu kasitli — future tx'ler potansiyel spam.

### 11.8 Revalidasyon Stratejisi

**`RevalidationType::Full`** (production icin):
- Background worker her 200ms'de 20-25% tx revalidate eder
- Her 60 saniye VEYA 20 blokta bir revalidasyon tetiklenir

**`RevalidationType::Light`**:
- Retract edilen bloklardan tx resubmit edilmez
- Daha dusuk overhead

Nova Shield Chain icin `Full` revalidation onerilir cunku:
1. MPC tx'lerin validity'si blok gecikmesiyle degisebilir
2. 200ms interval ile pool'un temiz kalmasi saglanir

### 11.9 `recently_pruned` Mekanizmasinin Onemi

**Kritik race condition**: Bir tx validate edilmekte iken ayni tag bloga giriyor olabilir. `recently_pruned[2]` bu durumu yakalar: Son 2 prune round'unda silinen tag'ler `requires`'da var ise tx future'a gitmez, dogrudan ready'e gider.

Nova Shield Chain'de blok uretimi sirasinda bu durumun yasanmasi mumkundur. `RECENTLY_PRUNED_TAGS = 2` default uygundur.

### 11.10 Offchain Worker Tx Destegi

`OffchainTransactionPoolFactory` + `LocalTransactionPool` kullanarak offchain worker'lar pool'a blocking submit yapabilir. `TransactionSource::Local` ile isaretlenir ve propagate=true ise networke yayilir.

### 11.11 Dikkat Edilmesi Gereken Tasarim Kararlari

1. **`provides` hic bos birakilmasin**: Runtime `validate_transaction` implementasyonunda `ValidTransaction::provides` mutlaka dolu olmali.

2. **Cycle detection**: Circular dependency (`CycleDetected`) transaction'lari future queue'da biriktirir. Runtime'da saglikli nonce sistemi bunu onler.

3. **Usurpation semantigi**: `fire_events` icinde `usurped` eventi gonderilir: `removed.iter().for_each(|r| event_dispatcher.usurped(&r.hash, hash))`. Eski tx'in watcher'larini `Usurped` eventi alir.

4. **Concurrent prune + import race**: `recently_pruned` + `with_futures_enabled` mekanizmalari bu race condition'ı handle eder. Nova Shield'de test senaryolariyla dogrulanmali.

5. **TrackedMap boyut sapması**: `AtomicIsize` ile track ediliyor, yaklaşık deger — kesin degil. `max(0, value)` ile negatif olmasi engellenir. Minör sapma kabul edilebilir.

6. **`LinkedHashMap` ile finality_watcher**: Insertion-order korunmasi sayesinde `pop_front()` her zaman en eski blogu dondurur; `MAX_FINALITY_WATCHERS = 512` asildığında timeout gonderilir.

7. **`submit_and_watch` watcher lifecycle**: Watcher `is_done()` olduğunda `EventDispatcher::watchers` map'inden silinir. Memory leak riski yok.

### 11.12 Metrik Entegrasyonu

Prometheus entegrasyonu hazir:
- `substrate_sub_txpool_validations_scheduled` / `validations_finished`
- `block_transactions_pruned` / `block_transactions_resubmitted`
- `submitted_transactions` / `validations_invalid`

Nova Shield Chain icin ek metric onerilir:
- MPC oturum tx'lerinin pool kalış süresi
- Threshold imza bekleme süresi (future queue kalış suresi)
- Cycle detected sayaci

---

## Ozet: En Kritik Degerler

```
DEFAULT: ready=8192tx/20MB | future=512tx/1MB | ban=30dk
FINALITY_TIMEOUT: 512 blok
REVALIDATION: 200ms interval, min 20 tx/batch, 1/4 pool her round
RECENTLY_PRUNED: son 2 round tag tutulur
IMPORT_NOTIFICATION: 1024 buffer (async channel)
WATCHER: 100_000 buffer (unbounded)
FUTURE_EVICTION: en eski timestamp/imported_at once atilir
READY_EVICTION: en dusuk priority VEYA en yeni insertion_id once atilir (limit enforcement)
BAN_GC: > 2*expected_size -> expected_size'a indir
SKIP_MAINTENANCE_THRESHOLD: 20 blok fark asildiysa maintain atlan
FATP_FINALITY_TIMEOUT_THRESHOLD: 128 blok
MEMPOOL_TO_VIEW_BATCH: 7000 tx
TXMEMPOOL_REVALIDATION_PERIOD: 10 blok
TXMEMPOOL_MAX_REVALIDATION_BATCH: 1000 tx
LimitsEnforced ban: HICBIR ZAMAN otomatik kaldirilmaz
Validation ban: mempool revalidation ile kaldirilabilir (unban_if_validation)
```
