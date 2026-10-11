# Solana PoH + Turbine + Entry Analizi — Agent 20

**Analiz Tarihi:** 2026-07-22
**Kapsam:** poh/src/, turbine/src/, entry/src/

---

## BÖLÜM 1: PoH (Proof of History)

### Dosyalar
- `poh/src/lib.rs` — crate kök modülü
- `poh/src/poh_recorder.rs` — PoH kayıt makinesi, ~1300 satır
- `poh/src/poh_service.rs` — arka plan hash üretim servisi, ~572 satır
- `poh/src/leader_bank_notifier.rs` — lider banka durum bildirici, ~275 satır

---

### 1.1 entry/src/poh.rs — Temel Poh Struct (332 satır)

Bu dosya PoH zincirinin matematiksel çekirdeğidir. `entry` crate'inde yaşar, `poh` crate'i tarafından import edilir.

#### Sabitler

```rust
// satır 8
const LOW_POWER_MODE: u64 = std::u64::MAX;
```
`hashes_per_tick` None olarak geçildiğinde bu değer kullanılır. LOW_POWER_MODE'da her `tick()` çağrısı bir PohEntry üretir (hash sayısı sınırsız). Mainnet'te bu mod kullanılmaz.

#### Struct: Poh (satır 10-17)

```rust
pub struct Poh {
    pub hash: Hash,          // Mevcut SHA-256 zincir durumu (32 byte)
    num_hashes: u64,         // Bu tick/record döngüsündeki hash sayacı
    hashes_per_tick: u64,    // Bir tick'te kaç hash yapılacak
    remaining_hashes: u64,   // Bu tick tamamlanana kadar kalan hash sayısı
    tick_number: u64,        // Slot başından itibaren tick sırası
    slot_start_time: Instant,// Bu slotn başladığı an (timing için)
}
```

**Tasarım kararı:** `hash` field public; PohRecorder bunu okuyup blockhash olarak kullanır. Geri kalanlar private; invariant korunur.

#### Struct: PohEntry (satır 19-23)

```rust
#[derive(Debug)]
pub struct PohEntry {
    pub num_hashes: u64,  // Bu entry'ye gelen hash sayısı
    pub hash: Hash,       // Son hash değeri
}
```

Entry yapısına giden çıktı budur. `Entry.num_hashes` ve `Entry.hash` bu değerlerden doldurulur.

---

#### Poh::new ve new_with_slot_info (satır 26-42)

```rust
pub fn new(hash: Hash, hashes_per_tick: Option<u64>) -> Self {
    Self::new_with_slot_info(hash, hashes_per_tick, 0)
}

pub fn new_with_slot_info(hash: Hash, hashes_per_tick: Option<u64>, tick_number: u64) -> Self {
    let hashes_per_tick = hashes_per_tick.unwrap_or(LOW_POWER_MODE);
    assert!(hashes_per_tick > 1);  // En az 2 hash/tick zorunlu
    let now = Instant::now();
    Poh {
        hash,
        num_hashes: 0,
        hashes_per_tick,
        remaining_hashes: hashes_per_tick,
        tick_number,
        slot_start_time: now,
    }
}
```

**Önemli:** `hashes_per_tick > 1` assert'i. Neden? Çünkü `record()` en az 1 hash yakarak transaction'ı karıştırır, ardından tick için 1 hash daha gerekir. Minimum 2 zorunlu.

`remaining_hashes` başlangıçta `hashes_per_tick` ile eşit başlar, yani ilk tick'e kadar tam `hashes_per_tick` kadar hash üretilebilir.

---

#### Poh::hash — Toplu Hash Üretimi (satır 61-72)

```rust
pub fn hash(&mut self, max_num_hashes: u64) -> bool {
    let num_hashes = std::cmp::min(self.remaining_hashes - 1, max_num_hashes);
    for _ in 0..num_hashes {
        self.hash = hash(self.hash.as_ref());
    }
    self.num_hashes += num_hashes;
    self.remaining_hashes -= num_hashes;
    assert!(self.remaining_hashes > 0);
    self.remaining_hashes == 1  // true ise caller tick() çağırmalı
}
```

**Kritik detay:** `remaining_hashes - 1` ile sınırlama. Son hash tick'e ayrılmış. Bu metot asla tick'i tamamlamaz; sadece `remaining_hashes == 1` durumunu bildirerek caller'a "tick vakti" sinyali verir.

Matematiksel olarak: `H_new = SHA256(H_old)` işlemi `num_hashes` kez tekrarlanır.

---

#### Poh::record — Transaction Karıştırma (satır 74-88)

```rust
pub fn record(&mut self, mixin: Hash) -> Option<PohEntry> {
    if self.remaining_hashes == 1 {
        return None;  // Tick gerekli, önce tick() çağır
    }
    self.hash = hashv(&[self.hash.as_ref(), mixin.as_ref()]);
    let num_hashes = self.num_hashes + 1;
    self.num_hashes = 0;
    self.remaining_hashes -= 1;
    Some(PohEntry { num_hashes, hash: self.hash })
}
```

**PoH Hash Zinciri — Record Matematigi:**

```
record(mixin):
  H_new = SHA256(H_prev || mixin)
```

`mixin` = transaction'ların Merkle kökünün hash'i (bkz. `hash_transactions()`). Bu işlem sayesinde:
1. Transaction zamanlaması zincire gömülür
2. Transaction sıraları değiştirilemez (Merkle kök değişir)
3. Zincir kırılmaz — H_new önceki H_prev'e bağımlıdır

`hashv` = SHA-256 çoklu girdi varyantı: `SHA256([H_prev_bytes, mixin_bytes])` — iki dilim tek geçişte hash'lenir.

`num_hashes` sıfırlanır çünkü yeni tick döngüsü başlar.

---

#### Poh::tick — Tick Üretimi (satır 90-109)

```rust
pub fn tick(&mut self) -> Option<PohEntry> {
    self.hash = hash(self.hash.as_ref());
    self.num_hashes += 1;
    self.remaining_hashes -= 1;

    // Low power: her tick üretir. Normal: sadece remaining_hashes == 0'da
    if self.hashes_per_tick != LOW_POWER_MODE && self.remaining_hashes != 0 {
        return None;
    }

    let num_hashes = self.num_hashes;
    self.remaining_hashes = self.hashes_per_tick;  // reset
    self.num_hashes = 0;
    self.tick_number += 1;
    Some(PohEntry { num_hashes, hash: self.hash })
}
```

**Tick Matematigi:**

```
tick():
  H_new = SHA256(H_prev)
  eğer remaining_hashes == 0:
    PohEntry { num_hashes: toplam_hash_sayisi, hash: H_new }
    remaining_hashes = hashes_per_tick  (reset)
```

Tick, "zaman birimi"dir. `ticks_per_slot` adet tick = 1 slot. Mainnet'te `DEFAULT_HASHES_PER_TICK = 12_500` ve `DEFAULT_TICKS_PER_SLOT = 64`.

Bir slot = 64 * 12_500 = 800_000 SHA-256 hash işlemi.

---

#### Poh::target_poh_time — Zamanlama (satır 54-59)

```rust
pub fn target_poh_time(&self, target_ns_per_tick: u64) -> Instant {
    assert!(self.hashes_per_tick > 0);
    let offset_tick_ns = target_ns_per_tick * self.tick_number;
    let offset_ns = target_ns_per_tick * self.num_hashes / self.hashes_per_tick;
    self.slot_start_time + Duration::from_nanos(offset_ns + offset_tick_ns)
}
```

Bu fonksiyon "PoH şu anda nerede olmalı" sorusunu yanıtlar. `PohService` bu değeri kullanarak hash üretim hızını throttle eder: gerçek saat ideal saatten öndeyse busy-wait yapılır. Bu mekanizma, slot süresini wall clock'a sabitleyen temel unsurdur.

---

#### Yardımcı Fonksiyonlar (satır 112-125)

```rust
pub fn compute_hash_time_ns(hashes_sample_size: u64) -> u64 {
    let mut v = Hash::default();
    let start = Instant::now();
    for _ in 0..hashes_sample_size {
        v = hash(v.as_ref());
    }
    start.elapsed().as_nanos() as u64
}

pub fn compute_hashes_per_tick(duration: Duration, hashes_sample_size: u64) -> u64 {
    let elapsed_ms = compute_hash_time_ns(hashes_sample_size) / (1000 * 1000);
    duration.as_millis() as u64 * hashes_sample_size / elapsed_ms
}
```

Bu fonksiyonlar genesis sırasında çalıştırılarak CPU hızına göre `hashes_per_tick` değeri kalibre edilir. Her donanımda aynı wall-clock tick süresi hedeflenir.

---

#### Test Fonksiyonları (satır 127-331)

**test_poh_verify (satır 194):** `tick → record → record → tick` sekansını üretir ve bağımsız `verify` fonksiyonuyla doğrular. Zincirin matematiksel tutarlılığını kanıtlar.

**test_poh_tick (satır 290):** `hashes_per_tick=2` ile:
- `tick()` → None (1 hash yaptı, remaining=1, henüz bitmedi)
- `tick()` → Some(PohEntry { num_hashes: 2 }) (2. hash, tick tamamlandı)

**test_poh_record_not_permitted_at_final_hash (satır 319):**
`remaining_hashes == 1` olunca `record()` None döner. Bu invariant kritik: son hash tick'e ayrılmış, transaction karıştırmak için kullanılamaz.

**test_poh_tick_large_batch (satır 300):**
`hash(1_000_000)` çağrısı bile `remaining_hashes = 1`'i aşamaz (guard ile korunur).

---

### 1.2 poh/src/poh_service.rs — Arka Plan Hash Servisi (572 satır)

#### Sabitler (satır 30-34)

```rust
pub const DEFAULT_HASHES_PER_BATCH: u64 = 64;
pub const DEFAULT_PINNED_CPU_CORE: usize = 0;
const TARGET_SLOT_ADJUSTMENT_NS: u64 = 50_000_000;  // 50ms
```

`DEFAULT_HASHES_PER_BATCH = 64`: Her kilit alımında 64 hash yapılır. Çok küçük → kilit çekişmesi artar. Çok büyük → transaction kaydı gecikmesi artar.

`TARGET_SLOT_ADJUSTMENT_NS = 50ms`: Slot süresinden çıkarılan tampon. Transaction işleme, ağ gecikmesi vs. için alan bırakılır.

---

#### Struct: PohTiming (satır 37-47)

```rust
#[derive(Debug)]
struct PohTiming {
    num_ticks: u64,
    num_hashes: u64,
    total_sleep_us: u64,
    total_lock_time_ns: u64,
    total_hash_time_ns: u64,
    total_tick_time_ns: u64,
    last_metric: Instant,
    total_record_time_us: u64,
    total_send_record_result_us: u64,
}
```

Performans metrikleri. Her 1 saniyede bir `datapoint_info!` makrosuyla Prometheus/InfluxDB'ye gönderilir.

---

#### Struct: PohService (satır 20-22)

```rust
pub struct PohService {
    tick_producer: JoinHandle<()>,
}
```

Tek bir arka plan thread'ini sarar. Thread ismi: `"solPohTickProd"`.

---

#### PohService::new (satır 97-150)

Üç farklı mod seçilir:

1. `hashes_per_tick = None AND target_tick_count = None` → `low_power_tick_producer`: Wall-clock'a göre tick üretir. Test/geliştirme ortamı.

2. `hashes_per_tick = None AND target_tick_count = Some(n)` → `short_lived_low_power_tick_producer`: Sadece n tick üretip durur. Genesis bootstrap için.

3. `hashes_per_tick = Some(n)` → `tick_producer`: Tam performans, CPU core'a pin'lenir, tight loop.

CPU pin'leme:
```rust
if let Some(cores) = core_affinity::get_core_ids() {
    core_affinity::set_for_current(cores[pinned_cpu_core]);
}
```
Default `pinned_cpu_core = 0`. Dedicated CPU core, cache locality artırır, hash hızını maximize eder.

---

#### PohService::target_ns_per_tick (satır 152-161)

```rust
pub fn target_ns_per_tick(ticks_per_slot: u64, target_tick_duration_ns: u64) -> u64 {
    let adjustment_per_tick = if ticks_per_slot > 0 {
        TARGET_SLOT_ADJUSTMENT_NS / ticks_per_slot
    } else { 0 };
    target_tick_duration_ns.saturating_sub(adjustment_per_tick)
}
```

Mainnet: `target_tick_duration = 400ms/64 = 6.25ms`, `adjustment = 50ms/64 ≈ 0.78ms`
Efektif tick süresi ≈ 5.47ms. Bu, slotun son ~50ms'ini transaction işlemeye ayırır.

---

#### PohService::record_or_hash — Ana Döngü Mantığı (satır 239-330)

Bu fonksiyon PoH servisinin kalbidir. İki dal:

**Dal 1: Record isteği varsa**
```
poh_recorder.write() kilidi al
loop:
  poh_recorder.record(slot, mixin, transactions) çağır
  sonucu sender'a gönder
  try_recv ile sıradaki record'u kontrol et
  varsa tekrar kaydet (kilit hâlâ elimizde)
  yoksa çık
```
Birden fazla record isteği varsa tek kilit alımında işlenir — verimlilik.

**Dal 2: Record isteği yoksa**
```
poh.lock() al
loop:
  poh.hash(hashes_per_batch)  # 64 hash
  ideal_time = poh.target_poh_time(target_ns_per_tick)
  if should_tick: return true
  if record isteği geldi: kaydet, break
  if ideal_time <= now: devam et (hız yetişmiyor)
  else:
    poh kilidini bırak
    ideal_time'a kadar busy-wait (her iterasyonda record kontrolü)
    break
```

**Tasarım tercihi:** Hız kontrolü için `sleep()` yerine busy-wait. OS scheduler'ın uyutma gecikmelerinden kaçınılır. Yüksek CPU kullanımı ama düşük jitter.

---

#### PohService::tick_producer (satır 332-373)

```rust
fn tick_producer(
    poh_recorder: Arc<RwLock<PohRecorder>>,
    poh_exit: &AtomicBool,
    ticks_per_slot: u64,
    hashes_per_batch: u64,
    record_receiver: Receiver<Record>,
    target_ns_per_tick: u64,
) {
    let poh = poh_recorder.read().unwrap().poh.clone();
    // ...
    loop {
        let should_tick = Self::record_or_hash(...);
        if should_tick {
            // PohRecorder yazma kilidi al, tick() çağır
            poh_recorder.write().unwrap().tick();
            timing.num_ticks += 1;
            timing.report(ticks_per_slot);
            if poh_exit.load(Ordering::Relaxed) { break; }
        }
    }
}
```

`poh` Arc'ı `poh_recorder`'dan klonlanır. Hash üretimi `poh.lock()` ile yapılır (hafif Mutex). Tick ve record işlemleri `poh_recorder.write()` ile yapılır (ağır RwLock). Bu ayrım, hashing'in record/tick ile çakışmamasını sağlar — kilit çekişmesini minimize eder.

---

### 1.3 poh/src/poh_recorder.rs — PoH Kaydedici (~1300 satır)

#### Sabitler (satır 46-48)

```rust
pub const GRACE_TICKS_FACTOR: u64 = 2;
pub const MAX_GRACE_SLOTS: u64 = 2;
```

Grace ticks: Lider önceki slot'ta değilse, slot başında biraz bekleme süresi. Önceki liderin geç gelen bloklarını kabul etmek için.

---

#### Enum: PohRecorderError (satır 49-59)

```rust
#[derive(Error, Debug, Clone)]
pub enum PohRecorderError {
    #[error("max height reached")]
    MaxHeightReached,
    #[error("min height not reached")]
    MinHeightNotReached,
    #[error("send WorkingBankEntry error")]
    SendError(#[from] SendError<WorkingBankEntry>),
}
```

---

#### Type Aliases (satır 61-63)

```rust
type Result<T> = std::result::Result<T, PohRecorderError>;
pub type WorkingBankEntry = (Arc<Bank>, (Entry, u64));  // (bank, (entry, tick_height))
```

`WorkingBankEntry` BroadcastStage'e gönderilir. Bank referansı, entry ve tick_height.

---

#### Struct: BankStart (satır 65-86)

```rust
#[derive(Debug, Clone)]
pub struct BankStart {
    pub working_bank: Arc<Bank>,
    pub bank_creation_time: Arc<Instant>,
}
```

`get_working_bank_if_not_expired()`: `Bank::should_bank_still_be_processing_txs()` kontrol eder. ns_per_slot süre aşılırsa None döner — lider slot'u "timeout" olur.

---

#### Struct: Record (satır 92-112)

```rust
pub struct Record {
    pub mixin: Hash,
    pub transactions: Vec<VersionedTransaction>,
    pub slot: Slot,
    pub sender: RecordResultSender,  // Sender<Result<Option<usize>>>
}
```

`TransactionRecorder` → `PohService` channel aracılığıyla iletişim. `sender` ile sonuç geri bildirimi yapılır (oneshot pattern, unbounded channel).

---

#### Struct: TransactionRecorder (satır 142-241)

```rust
#[derive(Clone)]
pub struct TransactionRecorder {
    pub record_sender: Sender<Record>,
    pub is_exited: Arc<AtomicBool>,
}
```

Banking stage bu struct'ı kullanır. `record_transactions()` metodu:
1. `hash_transactions()` ile Merkle kökünü hesaplar
2. `Record` oluşturur, channel'a gönderir
3. 1 saniye timeout ile sonuç bekler
4. MaxHeightReached → slot bitti, transaction'ları at
5. Başarı → `starting_transaction_index` döner

---

#### Struct: WorkingBank (satır 265-271)

```rust
pub struct WorkingBank {
    pub bank: BankWithScheduler,
    pub start: Arc<Instant>,
    pub min_tick_height: u64,
    pub max_tick_height: u64,
    pub transaction_index: Option<usize>,
}
```

`min_tick_height`: Bu slot için ilk geçerli tick. Önceki tick'ler cache'de bekler.
`max_tick_height`: Bu slot için son tick. Aşılınca `clear_bank()` çağrılır.
`transaction_index`: Slot içindeki transaction sırasını takip eder (isteğe bağlı).

---

#### Enum: PohLeaderStatus (satır 273-277)

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum PohLeaderStatus {
    NotReached,
    Reached { poh_slot: Slot, parent_slot: Slot },
}
```

`reached_leader_slot()` bu enum'u döner. `poh_slot`: Üretilecek slot. `parent_slot`: Bir önceki slot (parent).

---

#### Struct: PohRecorder (satır 279-310)

```rust
pub struct PohRecorder {
    pub poh: Arc<Mutex<Poh>>,          // Hash makinesi
    tick_height: u64,                   // Toplam tick sayacı (tüm slot'lar)
    clear_bank_signal: Option<Sender<bool>>,
    start_bank: Arc<Bank>,             // Slot başlangıç bankası
    start_tick_height: u64,            // Bu slot'un ilk tick height'ı
    tick_cache: Vec<(Entry, u64)>,     // Henüz flushed olmayan tick'ler
    working_bank: Option<WorkingBank>, // Aktif banka (lider ise Some)
    sender: Sender<WorkingBankEntry>,  // BroadcastStage'e çıkış
    poh_timing_point_sender: Option<PohTimingSender>,
    leader_first_tick_height_including_grace_ticks: Option<u64>,
    leader_last_tick_height: u64,
    grace_ticks: u64,
    id: Pubkey,
    blockstore: Arc<Blockstore>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    ticks_per_slot: u64,
    target_ns_per_tick: u64,
    // Performans metrikleri:
    record_lock_contention_us: u64,
    flush_cache_no_tick_us: u64,
    flush_cache_tick_us: u64,
    send_entry_us: u64,
    tick_lock_contention_us: u64,
    total_sleep_us: u64,
    record_us: u64,
    report_metrics_us: u64,
    ticks_from_record: u64,
    last_metric: Instant,
    record_sender: Sender<Record>,
    leader_bank_notifier: Arc<LeaderBankNotifier>,
    pub is_exited: Arc<AtomicBool>,
}
```

`tick_cache`: Lider olmayan dönemlerde üretilen tick entry'leri burada birikir. Lider olunduğunda `flush_cache()` ile bankaya ve sender'a gönderilir.

---

#### PohRecorder::slot_for_tick_height (satır 368-373)

```rust
fn slot_for_tick_height(&self, tick_height: u64) -> Slot {
    tick_height.saturating_sub(1) / self.ticks_per_slot
}
```

`tick_height` 1-based. [1..64] → slot 0. [65..128] → slot 1. Bu şema, sıfırıncı tick'in olmadığı gerçeğini yansıtır.

---

#### PohRecorder::compute_leader_slot_tick_heights (satır 514-545)

```rust
fn compute_leader_slot_tick_heights(
    next_leader_slot: Option<(Slot, Slot)>,
    ticks_per_slot: u64,
) -> (Option<u64>, u64, u64) {
    next_leader_slot.map(|(first_slot, last_slot)| {
        let leader_first_tick_height = first_slot * ticks_per_slot + 1;
        let last_tick_height = (last_slot + 1) * ticks_per_slot;
        let num_slots = last_slot - first_slot + 1;
        let grace_ticks = cmp::min(
            ticks_per_slot * MAX_GRACE_SLOTS,
            ticks_per_slot * num_slots / GRACE_TICKS_FACTOR,
        );
        let leader_first_tick_height_including_grace_ticks =
            leader_first_tick_height + grace_ticks;
        (Some(leader_first_tick_height_including_grace_ticks), last_tick_height, grace_ticks)
    }).unwrap_or(...)
}
```

`grace_ticks = min(2 * ticks_per_slot, num_slots * ticks_per_slot / 2)`.
Lider 4 consecutive slot'a atanır → `num_slots=4`, `grace_ticks = min(128, 128) = 128` tick = 2 slot.
Bu 2 slot süresince önceki liderin blokları beklenebilir.

---

#### PohRecorder::tick (satır 773-829)

```rust
pub fn tick(&mut self) {
    let ((poh_entry, target_time), ...) = measure!({
        let mut poh_l = self.poh.lock().unwrap();
        let poh_entry = poh_l.tick();
        let target_time = poh_entry.is_some()
            .then(|| poh_l.target_poh_time(self.target_ns_per_tick));
        (poh_entry, target_time)
    }, ...);

    if let Some(poh_entry) = poh_entry {
        self.tick_height += 1;
        self.report_poh_timing_point();
        // ...
        self.tick_cache.push((Entry {
            num_hashes: poh_entry.num_hashes,
            hash: poh_entry.hash,
            transactions: vec![],
        }, self.tick_height));

        let _ = self.flush_cache(true);

        // Busy-wait: target_time'a kadar spin_loop
        while Instant::now() < target_time.unwrap() {
            std::hint::spin_loop();
        }
    }
}
```

`std::hint::spin_loop()`: CPU'ya "bu döngü bekleme döngüsü" sinyali. x86'da `PAUSE` instruction'a çevrilir — CPU power tasarrufu ve spekülatif yürütmeye yardım.

---

#### PohRecorder::record (satır 864-932)

```rust
pub fn record(&mut self, bank_slot: Slot, mixin: Hash, transactions: Vec<VersionedTransaction>)
    -> Result<Option<usize>>
{
    assert!(!transactions.is_empty());
    // ...
    loop {
        self.flush_cache(false)?;
        // bank_slot kontrol
        let mut poh_lock = self.poh.lock().unwrap();
        let record_mixin_res = poh_lock.record(mixin);
        drop(poh_lock);

        if let Some(poh_entry) = record_mixin_res {
            let entry = Entry {
                num_hashes: poh_entry.num_hashes,
                hash: poh_entry.hash,
                transactions,
            };
            self.sender.send((bank_clone, (entry, self.tick_height)))?;
            return Ok(starting_transaction_index);
        }
        // Poh.record() None döndü → tick gerekli
        self.ticks_from_record += 1;
        self.tick();  // tick yap, sonra tekrar dene
    }
}
```

`record()` None döndürdüğünde (remaining_hashes == 1): önce tick, sonra tekrar record. Bu loop en fazla 1 kez döner çünkü tick sonrası remaining_hashes sıfırlanır.

---

#### PohRecorder::flush_cache (satır 655-715)

Biriktirilen tick entry'lerini working bank'a ve sender'a gönderir.

Koşullar:
- `tick_height >= min_tick_height` olmadan flush yapılmaz
- `tick` modunda: `tick_height > min_tick_height` gerekli
- Entry'ler sırayla `bank.register_tick()` ile bankaya kaydedilir
- `max_tick_height` aşılırsa `clear_bank()` çağrılır

---

#### PohRecorder::reset (satır 572-594)

```rust
pub fn reset(&mut self, reset_bank: Arc<Bank>, next_leader_slot: Option<(Slot, Slot)>) {
    self.clear_bank();
    self.reset_poh(reset_bank, true);
    // poh_timing_point bildirimi
    // grace_ticks hesapla
}
```

Fork seçimi yapıldığında veya yeni slot başladığında çağrılır. PoH hash zinciri `reset_bank.last_blockhash()` ile yeniden başlatılır. `tick_cache` temizlenir.

---

### 1.4 poh/src/leader_bank_notifier.rs (275 satır)

#### Enum: Status (satır 22-29)

```rust
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum Status {
    #[default]
    StandBy,   // Lider banka yok (başlangıç veya tamamlanmış)
    InProgress, // Aktif lider banka var
}
```

İki durumlu state machine.

---

#### Struct: LeaderBankNotifier (satır 13-19)

```rust
#[derive(Debug, Default)]
pub struct LeaderBankNotifier {
    state: Mutex<SlotAndBankWithStatus>,
    condvar: Condvar,
}
```

`Condvar` + `Mutex` kombinasyonu. Thread'ler lider banka başlamasını veya bitmesini bekleyebilir.

---

#### Struct: SlotAndBankWithStatus (satır 31-36)

```rust
#[derive(Debug, Default)]
struct SlotAndBankWithStatus {
    status: Status,
    slot: Option<Slot>,
    bank: Weak<Bank>,  // Weak referans — bank drop olabilir
}
```

`Weak<Bank>`: Arc döngüsel referansı önler. Bank drop olunca Weak upgrade edilemez hale gelir.

---

#### Metodlar

`set_in_progress(bank)`: Status → InProgress, condvar notify_all. Sadece StandBy'dan geçilebilir (assert).

`set_completed(slot)`: Status → StandBy, condvar notify_all. Sadece InProgress'ten geçilebilir.

`get_or_wait_for_in_progress(timeout)`: InProgress değilse timeout kadar bekler. TPU bu ile lider bankayı bekler.

`wait_for_completed(timeout)`: Önce InProgress bekler, sonra StandBy bekler. Slot tamamlanma bildirimi için.

---

## BÖLÜM 2: Entry

### Dosyalar
- `entry/src/lib.rs` — modül bildirimi
- `entry/src/poh.rs` — temel Poh struct (yukarıda analiz edildi)
- `entry/src/entry.rs` — Entry struct ve doğrulama, ~1100 satır

---

### 2.1 entry/src/entry.rs — Detaylı Analiz

#### Global Thread Pool (satır 46-52)

```rust
lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_max_thread_count())
        .thread_name(|i| format!("solEntry{i:02}"))
        .build()
        .unwrap();
}
```

Entry doğrulama için global Rayon thread pool. `get_max_thread_count()` → CPU çekirdek sayısı.

---

#### SIMD API (satır 99-105)

```rust
#[derive(SymBorApi)]
pub struct Api<'a> {
    pub poh_verify_many_simd_avx512skx:
        Symbol<'a, unsafe extern "C" fn(hashes: *mut u8, num_hashes: *const u64)>,
    pub poh_verify_many_simd_avx2:
        Symbol<'a, unsafe extern "C" fn(hashes: *mut u8, num_hashes: *const u64)>,
}
```

`libpoh-simd.so` dinamik kütüphanesi. AVX-512 (16 hash paralel) ve AVX2 (8 hash paralel) versiyonları. `dlopen2` crate ile runtime'da yüklenir.

`init_poh()` (satır 59): `libpoh-simd.so` yükler. `api()` (satır 86): Yüklendiyse referans döner.

---

#### Struct: Entry (satır 134-146)

```rust
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Eq, Clone)]
pub struct Entry {
    pub num_hashes: u64,
    pub hash: Hash,
    pub transactions: Vec<VersionedTransaction>,
}
```

**Üç alan, üç anlam:**
- `num_hashes`: Bu entry'den önce kaç SHA-256 hash yapıldı. Geçen zamanın delili.
- `hash`: PoH zincirinin mevcut durumu. Önceki entry'nin hash'inden hesaplanır.
- `transactions`: Bu entry'ye dahil edilen işlemler. Boş ise bu entry bir "tick"tir.

`is_tick()`: `transactions.is_empty()` — basit ama kritik sınır.

---

#### Struct: EntrySummary (satır 148-162)

```rust
pub struct EntrySummary {
    pub num_hashes: u64,
    pub hash: Hash,
    pub num_transactions: u64,
}
```

Tam Entry'yi klonlamadan özet bilgi taşır.

---

#### Enum: EntryType (satır 164-168)

```rust
pub enum EntryType {
    Transactions(Vec<SanitizedTransaction>),
    Tick(Hash),
}
```

Doğrulanmış entry türü. `verify_transactions()` sonucu bu enum döner.

---

#### Entry::new (satır 172-186)

```rust
pub fn new(prev_hash: &Hash, mut num_hashes: u64, transactions: Vec<Transaction>) -> Self {
    if num_hashes == 0 && !transactions.is_empty() {
        num_hashes = 1;
    }
    let transactions = transactions.into_iter().map(Into::into).collect::<Vec<_>>();
    let hash = next_hash(prev_hash, num_hashes, &transactions);
    Entry { num_hashes, hash, transactions }
}
```

`num_hashes == 0` ve transaction varsa `num_hashes = 1` — en az 1 hash yapılmalı (zincir ilerlemeli).

---

#### Entry::verify (satır 209-221)

```rust
pub fn verify(&self, start_hash: &Hash) -> bool {
    let ref_hash = next_hash(start_hash, self.num_hashes, &self.transactions);
    if self.hash != ref_hash {
        warn!("next_hash is invalid expected: {:?} actual: {:?}", self.hash, ref_hash);
        return false;
    }
    true
}
```

**Doğrulama mantığı:** `start_hash`'ten `num_hashes` kadar hash yapıldığında ve transactions mixin'lendikten sonra `self.hash` elde edilmeli.

---

#### hash_transactions (satır 228-240)

```rust
pub fn hash_transactions(transactions: &[VersionedTransaction]) -> Hash {
    let signatures: Vec<_> = transactions
        .iter()
        .flat_map(|tx| tx.signatures.iter())
        .collect();
    let merkle_tree = MerkleTree::new(&signatures);
    if let Some(root_hash) = merkle_tree.get_root() {
        *root_hash
    } else {
        Hash::default()
    }
}
```

**Transaction hash'leme algoritması:**
1. Tüm transaction'ların tüm signature'ları toplanır
2. Signature'lardan Merkle ağacı oluşturulur
3. Merkle kökü `mixin` olarak kullanılır

Neden signature Merkle'ı? Signature = transaction'ın tam parmak izi. İçerik değişirse signature değişir. Merkle kökü sıraya duyarlı — sıra değişirse kök değişir.

---

#### next_hash (satır 246-262)

```rust
pub fn next_hash(start_hash: &Hash, num_hashes: u64, transactions: &[VersionedTransaction]) -> Hash {
    if num_hashes == 0 && transactions.is_empty() {
        return *start_hash;
    }
    let mut poh = Poh::new(*start_hash, None);
    poh.hash(num_hashes.saturating_sub(1));
    if transactions.is_empty() {
        poh.tick().unwrap().hash
    } else {
        poh.record(hash_transactions(transactions)).unwrap().hash
    }
}
```

**Hash zinciri matematigi özeti:**

Tick entry:
```
H_n = SHA256^(num_hashes)(H_{n-1})
```
Son hash: `SHA256(SHA256^(num_hashes-1)(H_{n-1}))`

Transaction entry:
```
H_n = SHA256(SHA256^(num_hashes-1)(H_{n-1}) || merkle_root(tx_signatures))
```

---

#### Enum: VerifyAction (satır 264-272)

```rust
enum VerifyAction {
    Mixin(Hash),  // Transaction entry: son hash'de mixin karıştır
    Tick,         // Tick entry: son hash'de sadece SHA256
    None,         // num_hashes == 0 tick: hash değişmedi
}
```

GPU doğrulama için: GPU `num_hashes-1` kadar hash yapar, CPU artık `VerifyAction`'a göre son adımı tamamlar.

---

#### Struct: GpuVerificationData (satır 274-278)

```rust
pub struct GpuVerificationData {
    thread_h: Option<JoinHandle<u64>>,          // GPU thread handle
    hashes: Option<Arc<Mutex<PinnedVec<Hash>>>>,// GPU pinned memory
    verifications: Option<Vec<(VerifyAction, Hash)>>, // CPU final check listesi
}
```

`PinnedVec<Hash>`: Page-locked memory (RAM → GPU DMA transferi için).

---

#### EntryVerificationState ve EntrySigVerificationState

İki ayrı doğrulama state'i:
1. **PoH doğrulama**: Hash zinciri tutarlılığı. `EntryVerificationState`.
2. **Signature doğrulama**: Transaction imzaları. `EntrySigVerificationState`.

Her ikisi `Pending` durumuna geçebilir (GPU async) ve `finish_verify()` ile tamamlanır.

---

#### VerifyRecyclers (satır 337-344)

```rust
#[derive(Default, Clone)]
pub struct VerifyRecyclers {
    hash_recycler: Recycler<PinnedVec<Hash>>,
    tick_count_recycler: Recycler<PinnedVec<u64>>,
    packet_recycler: PacketBatchRecycler,
    out_recycler: Recycler<PinnedVec<u8>>,
    tx_offset_recycler: Recycler<sigverify::TxOffset>,
}
```

Memory pool — GPU doğrulama için sık allocate/deallocate edilecek buffer'ları recycle eder.

---

#### EntrySlice trait (satır 619-633)

```rust
pub trait EntrySlice {
    fn verify_cpu(&self, start_hash: &Hash) -> EntryVerificationState;
    fn verify_cpu_generic(&self, start_hash: &Hash) -> EntryVerificationState;
    fn verify_cpu_x86_simd(&self, start_hash: &Hash, simd_len: usize) -> EntryVerificationState;
    fn start_verify(&self, start_hash: &Hash, recyclers: VerifyRecyclers) -> EntryVerificationState;
    fn verify(&self, start_hash: &Hash) -> bool;
    fn verify_tick_hash_count(&self, tick_hash_count: &mut u64, hashes_per_tick: u64) -> bool;
    fn tick_count(&self) -> u64;
}
```

`[Entry]` slice'ı bu trait'i implement eder.

---

#### verify_cpu_generic (satır 641-673)

```rust
fn verify_cpu_generic(&self, start_hash: &Hash) -> EntryVerificationState {
    let genesis = [Entry { num_hashes: 0, hash: *start_hash, transactions: vec![] }];
    let entry_pairs = genesis.par_iter().chain(self).zip(self);
    let res = PAR_THREAD_POOL.install(|| {
        entry_pairs.all(|(x0, x1)| {
            let r = x1.verify(&x0.hash);
            r
        })
    });
    // ...
}
```

`genesis` ile `self` zip'lenerek `(prev, curr)` çiftleri oluşturulur. Her çift paralel doğrulanır. Rayon `par_iter` ile CPU çekirdeklerine dağıtılır.

---

#### verify_cpu_x86_simd (satır 675-754)

SIMD optimizasyonu. AVX2 (8 paralel) veya AVX-512 (16 paralel):

1. Tüm entry hash'lerini düz byte dizisine kopyala
2. `num_hashes - 1` değerlerini hazırla
3. `libpoh-simd.so` fonksiyonu ile toplu hash: GPU'ya benzer SIMD pipeline
4. Sonuçları CPU'da `compare_hashes()` ile doğrula

```rust
unsafe {
    (api().unwrap().poh_verify_many_simd_avx512skx)(
        chunk.as_mut_ptr(),
        num_hashes.as_ptr(),
    );
}
```

Unsafe blok: C fonksiyonu çağrısı. FFI boundary.

---

#### start_verify — GPU PoH Doğrulama (satır 778-869)

```rust
fn start_verify(&self, start_hash: &Hash, recyclers: VerifyRecyclers) -> EntryVerificationState {
    let Some(api) = perf_libs::api() else {
        return self.verify_cpu(start_hash);
    };
    // ...
    let gpu_verify_thread = thread::Builder::new()
        .name("solGpuPohVerify".into())
        .spawn(move || {
            unsafe {
                res = (api.poh_verify_many)(hashes.as_mut_ptr() as *mut u8, ...);
            }
            // ...
        }).unwrap();

    // CPU'da VerifyAction listesi hazırla (paralel)
    let verifications = PAR_THREAD_POOL.install(|| {
        self.into_par_iter().map(|entry| {
            let action = if entry.transactions.is_empty() {
                if entry.num_hashes == 0 { VerifyAction::None }
                else { VerifyAction::Tick }
            } else {
                VerifyAction::Mixin(hash_transactions(&entry.transactions))
            };
            (action, entry.hash)
        }).collect()
    });
    // ...
    EntryVerificationState { verification_status: EntryVerificationStatus::Pending, ... }
}
```

GPU ve CPU paralel çalışır:
- GPU: `num_hashes-1` kadar SHA256 zinciri ilerletir
- CPU: Her entry için son adım (Mixin/Tick/None) ve beklenen hash listesi hazırlar
- `finish_verify()` çağrısında GPU sonucu beklenir, CPU son adımı tamamlar

---

#### verify_tick_hash_count (satır 871-893)

```rust
fn verify_tick_hash_count(&self, tick_hash_count: &mut u64, hashes_per_tick: u64) -> bool {
    if hashes_per_tick == 0 { return true; }
    for entry in self {
        *tick_hash_count = tick_hash_count.saturating_add(entry.num_hashes);
        if entry.is_tick() {
            if *tick_hash_count != hashes_per_tick {
                warn!("invalid tick hash count!: ...");
                return false;
            }
            *tick_hash_count = 0;
        }
    }
    *tick_hash_count < hashes_per_tick
}
```

Her tick'te tam `hashes_per_tick` hash yapıldığını doğrular. State (`tick_hash_count`) bir sonraki entry slice'a taşınır — slot sınırları aşan doğrulama için.

---

#### Signature Doğrulama

`verify_transactions()` (satır 406): Rayon paralel, her entry için:
- Tick → EntryType::Tick(hash)
- Transaction → verify fonksiyonunu çağır → EntryType::Transactions(sanitized)

`start_verify_transactions()` (satır 430): CPU (< 512 tx) veya GPU (>= 512 tx) seçer.

`start_verify_transactions_gpu()` (satır 497):
1. Transaction'ları Packet'lere paketler
2. `sigverify::ed25519_verify()` GPU fonksiyonu ile imzaları doğrular
3. Thread: `"solGpuSigVerify"`

**Unsafe blok (satır 550-552):**
```rust
unsafe {
    packet_batch.set_len(vec_size);
}
```
Neden: Bellek allocate edildi ama initialize edilmedi. İçeriğe hemen yazılacak, default değer yazmak gereksiz.

---

#### Yardımcı Fonksiyonlar

`next_entry_mut()` (satır 900): `start` hash'ini in-place günceller.
`create_ticks()` (satır 906): Test için n tick entry oluşturur.
`next_versioned_entry()` (satır 928): VersionedTransaction ile entry oluşturur.

---

#### Test Fonksiyonları (satır 941+)

**test_transaction_reorder_attack (satır 1081):** İki transaction'ı swap eder, doğrulamanın başarısız olduğunu kanıtlar. Merkle kök sıra-bağımlı olduğundan attack tespit edilir.

**test_entry_gpu_verify (satır 1024):** 1025 geçersiz ve 1025 geçerli transaction, GPU ve CPU sonuçları karşılaştırılır.

---

## BÖLÜM 3: Turbine

### Dosyalar
- `turbine/src/lib.rs` — modül bildirimi
- `turbine/src/cluster_nodes.rs` — Turbine ağacı yapısı
- `turbine/src/broadcast_stage.rs` — Lider yayım aşaması
- `turbine/src/broadcast_stage/standard_broadcast_run.rs` — Standart yayım
- `turbine/src/broadcast_stage/broadcast_utils.rs` — Entry→Shred pipeline
- `turbine/src/broadcast_stage/broadcast_metrics.rs` — Metrikler
- `turbine/src/broadcast_stage/broadcast_duplicates_run.rs` — Test amaçlı
- `turbine/src/broadcast_stage/broadcast_fake_shreds_run.rs` — Test amaçlı
- `turbine/src/broadcast_stage/fail_entry_verification_broadcast_run.rs` — Test amaçlı
- `turbine/src/retransmit_stage.rs` — Validator retransmit
- `turbine/src/sigverify_shreds.rs` — Shred imza doğrulama
- `turbine/src/quic_endpoint.rs` — QUIC taşıma katmanı

---

### 3.1 turbine/src/cluster_nodes.rs — Turbine Ağaç Yapısı

#### Sabitler (satır 39-40)

```rust
const DATA_PLANE_FANOUT: usize = 200;
pub(crate) const MAX_NUM_TURBINE_HOPS: usize = 4;
```

`DATA_PLANE_FANOUT = 200`: Her node, 200 başka node'a iletir. `MAX_NUM_TURBINE_HOPS = 4`: Maksimum 4 hop. 200^3 = 8,000,000 > Solana validator sayısı → 3 hop yeterli, 4 marjin.

---

#### Enum: NodeId (satır 48-54)

```rust
#[allow(clippy::large_enum_variant)]
enum NodeId {
    ContactInfo(ContactInfo), // Gossip'te bilinen node
    Pubkey(Pubkey),           // Staked ama gossip'te bilinmeyen node
}
```

Staked node'lar turbine ağacına dahil edilir ancak contact info olmayabilir (gossip'e yeni katıldı, stale bilgi vs.).

---

#### Struct: Node (satır 56-59)

```rust
pub struct Node {
    node: NodeId,
    stake: u64,
}
```

---

#### Struct: ClusterNodes<T> (satır 61-70)

```rust
pub struct ClusterNodes<T> {
    pubkey: Pubkey,
    // Stake'e göre azalan sırada tüm node'lar
    nodes: Vec<Node>,
    // pubkey → nodes index reverse mapping
    index: HashMap<Pubkey, usize>,
    // Stake ağırlıklı shuffle için
    weighted_shuffle: WeightedShuffle<u64>,
    _phantom: PhantomData<T>,
}
```

Generic `T`: `BroadcastStage` (lider için) veya `RetransmitStage` (validator için). Phantom type ile aynı struct iki farklı davranış için kullanılır.

---

#### Struct: ClusterNodesCache<T> (satır 74-79)

```rust
pub struct ClusterNodesCache<T> {
    cache: Mutex<LruCache<Epoch, Arc<RwLock<CacheEntry<T>>>>>,
    ttl: Duration,
}
```

Epoch başına bir cache girişi. TTL = 5 saniye (gossip bilgisi güncel tutulur). LRU kapasitesi = 8 epoch.

Cache güncelleme stratejisi: İlk okuma thread cache miss'te write lock alır ve günceller. Diğer thread'ler read lock ile bekler. Double-check pattern (satır 423-436).

---

#### Struct: RetransmitPeers (satır 81-87)

```rust
pub struct RetransmitPeers<'a> {
    root_distance: usize,    // Turbine kökünden uzaklık (0=root, 1=1. katman, ...)
    children: Vec<&'a Node>, // Bu node'un iletmesi gereken çocuklar
    addrs: HashMap<SocketAddr, Pubkey>, // Adres → ilk node (dedup için)
}
```

---

#### ClusterNodes::get_broadcast_peer (satır 154-158)

```rust
pub(crate) fn get_broadcast_peer(&self, shred: &ShredId) -> Option<&ContactInfo> {
    let mut rng = get_seeded_rng(&self.pubkey, shred);
    let index = self.weighted_shuffle.first(&mut rng)?;
    self.nodes[index].contact_info()
}
```

Lider, her shred için deterministik RNG ile weighted shuffle yapar ve ilk node'u seçer. Shred'in slot+index'i seed olarak kullanılır → farklı shred'ler farklı ilk alıcılara gider → yük dengeleme.

---

#### ClusterNodes::get_retransmit_peers (satır 183-233)

```rust
pub fn get_retransmit_peers(&self, slot_leader: &Pubkey, shred: &ShredId, fanout: usize)
    -> Result<RetransmitPeers, Error>
{
    // Slot leader'ı listeden çıkar (döngü önleme)
    if slot_leader == &self.pubkey { return Err(Error::Loopback); }
    if let Some(index) = self.index.get(slot_leader) {
        weighted_shuffle.remove_index(*index);
    }
    let mut rng = get_seeded_rng(slot_leader, shred);
    let nodes: Vec<_> = weighted_shuffle.shuffle(&mut rng)
        .map(|index| &self.nodes[index])
        .inspect(|node| { /* adres haritası doldur */ })
        .collect();

    let self_index = nodes.iter().position(|node| node.pubkey() == self.pubkey).unwrap();
    let root_distance = /* 0, 1, 2, veya 3 */;
    let peers = get_retransmit_peers(fanout, self_index, &nodes);
    Ok(RetransmitPeers { root_distance, children: peers.collect(), addrs })
}
```

**Deterministik shuffle:** `ChaChaRng::from_seed(shred.seed(slot_leader))`. Tüm cluster aynı sonucu üretir → herkes kime iletmesi gerektiğini bilir, koordinasyon gereksiz.

---

#### get_retransmit_peers algoritması (satır 352-368)

```rust
// Turbine ağaç yapısı:
// root     : [0]
// 1st layer: [1, 2, ..., fanout]
// 2nd layer: [[fanout+1, ..., 2*fanout], [2*fanout+1, ..., 3*fanout], ...]
// k. node'un çocukları:
fn get_retransmit_peers<T: Copy>(fanout: usize, index: usize, nodes: &[T]) -> impl Iterator<Item=T> {
    let offset = index.saturating_sub(1) % fanout;
    let anchor = index - offset;
    let step = if index == 0 { 1 } else { fanout };
    (anchor * fanout + offset + 1..)
        .step_by(step)
        .take(fanout)
        .map(|i| nodes.get(i))
        .while_some()
        .copied()
}
```

**Turbine Ağaç Matematigi:**

Node `k`'nın çocukları (0-based index):
- `offset = (k-1) % fanout` (node'un kendi neighborhood içindeki konumu)
- `anchor = k - offset` (neighborhood'un ilk node'u)
- İlk çocuk: `anchor * fanout + offset + 1`
- Adım: `fanout`
- Toplam: `fanout` çocuk

Fanout=2 örneği (satır 684-714 testinde):
- Node 0 (root): çocuklar [1, 2]
- Node 1: çocuklar [3, 5] (1. neighborhood, offset=0)
- Node 2: çocuklar [4, 6] (1. neighborhood, offset=1)

---

#### get_retransmit_parent (satır 372-383)

```rust
fn get_retransmit_parent<T: Copy>(fanout: usize, index: usize, nodes: &[T]) -> Option<T> {
    let offset = index.saturating_sub(1) % fanout;
    let index = index.checked_sub(1)? / fanout;
    let index = index - index.saturating_sub(1) % fanout;
    let index = if index == 0 { index } else { index + offset };
    nodes.get(index).copied()
}
```

Ters yönde: çocuk → parent.

---

#### get_nodes (satır 301-332)

Node listesi oluşturma:
1. Self (kendi node'u)
2. Gossip'ten bilinen TVU peer'ları (contact info ile)
3. Tüm staked node'lar (contact info olmayan dahil)

`sorted_by_key(|node| Reverse((node.stake, node.pubkey())))`: Stake azalan, aynı stake'de pubkey artan. Deterministik sıra.

`dedup_by(|a, b| a.pubkey() == b.pubkey())`: Kararlı sıralamada ilk kopya kalır — contact info'su olan tercih edilir.

---

#### get_seeded_rng (satır 334-337)

```rust
fn get_seeded_rng(leader: &Pubkey, shred: &ShredId) -> ChaChaRng {
    let seed = shred.seed(leader);
    ChaChaRng::from_seed(seed)
}
```

`shred.seed(leader)` = `sha2::hash(leader_pubkey || shred_slot || shred_index)` (Ledger crate'inde). Her node aynı seed'i üretir → aynı shuffle.

---

#### get_data_plane_fanout (satır 528-544)

```rust
pub(crate) fn get_data_plane_fanout(shred_slot: Slot, root_bank: &Bank) -> usize {
    if enable_turbine_fanout_experiments(shred_slot, root_bank) {
        match shred_slot % 359 {
            11 => 64, 61 => 768, 111 => 128, 161 => 640,
            211 => 256, 261 => 512, 311 => 384,
            _ => DATA_PLANE_FANOUT,
        }
    } else {
        DATA_PLANE_FANOUT
    }
}
```

Slot sayısına göre fanout deneyleri. ~%2 slot'ta test yapılır (359 asal sayı, cycle minimize edilir).

---

### 3.2 turbine/src/broadcast_stage.rs — Lider Yayım

#### Sabitler (satır 56-61)

```rust
const CLUSTER_NODES_CACHE_NUM_EPOCH_CAP: usize = 8;
const CLUSTER_NODES_CACHE_TTL: Duration = Duration::from_secs(5);
pub(crate) const NUM_INSERT_THREADS: usize = 2;
```

---

#### Enum: BroadcastStageType (satır 99-104)

```rust
pub enum BroadcastStageType {
    Standard,
    FailEntryVerification,  // Test: hatalı entry gönder
    BroadcastFakeShreds,    // Test: sahte shred gönder
    BroadcastDuplicates(BroadcastDuplicatesConfig), // Test: duplicate shred
}
```

Mainnet sadece `Standard` kullanır. Diğerleri consensus test senaryoları için.

---

#### Enum: Error (satır 63-89)

```rust
pub enum Error {
    Blockstore(BlockstoreError),
    ClusterInfo(ClusterInfoError),
    InvalidMerkleRoot { slot: Slot, index: u64 },
    Io(std::io::Error),
    Recv(RecvError),
    RecvTimeout(RecvTimeoutError),
    Send,
    Serialize(Box<bincode::ErrorKind>),
    ShredNotFound { slot: Slot, index: u64 },
    TransportError(...),
    UnknownLastIndex(Slot),
    UnknownSlotMeta(Slot),
}
```

---

#### Trait: BroadcastRun (satır 172-190)

```rust
trait BroadcastRun {
    fn run(&mut self, keypair, blockstore, receiver, socket_sender, blockstore_sender) -> Result<()>;
    fn transmit(&mut self, receiver, cluster_info, sock, bank_forks, quic_endpoint_sender) -> Result<()>;
    fn record(&mut self, receiver, blockstore) -> Result<()>;
}
```

3 aşama: run (entry → shred), transmit (shred → ağ), record (shred → blockstore).

---

#### Struct: BroadcastStage (satır 210-212)

```rust
pub struct BroadcastStage {
    thread_hdls: Vec<JoinHandle<BroadcastStageReturnType>>,
}
```

Thread listesi:
- 1 `"solBroadcast"` thread: entry → shred pipeline
- N `"solBroadcastTx"` thread: shred → UDP/QUIC (socket sayısı kadar)
- 2 `"solBroadcastRec"` thread: shred → blockstore
- 1 `"solBroadcastRtx"` thread: retransmit sinyalleri

---

#### BroadcastStage::new — Thread Mimarisi (satır 273-369)

```rust
fn new(socks, cluster_info, receiver, retransmit_slots_receiver, exit, blockstore, bank_forks, quic_endpoint_sender, broadcast_stage_run) -> Self {
    let (socket_sender, socket_receiver) = unbounded();
    let (blockstore_sender, blockstore_receiver) = unbounded();

    // Thread 1: run pipeline
    let thread_hdl = Builder::new().name("solBroadcast").spawn(move || {
        Self::run(cluster_info, &blockstore, &receiver, &socket_sender_, &blockstore_sender, bs_run)
    });

    // Thread N: transmit
    thread_hdls.extend(socks.into_iter().map(|sock| {
        Builder::new().name("solBroadcastTx").spawn(move || loop {
            bs_transmit.transmit(&socket_receiver, ...);
        })
    }));

    // Thread 2: record
    thread_hdls.extend(repeat_with(|| {
        Builder::new().name("solBroadcastRec").spawn(move || loop {
            bs_record.record(&blockstore_receiver, &btree);
        })
    }).take(NUM_INSERT_THREADS));

    // Thread 1: retransmit signals
    let retransmit_thread = Builder::new().name("solBroadcastRtx").spawn(move || loop {
        Self::check_retransmit_signals(&blockstore, &retransmit_slots_receiver, &socket_sender);
    });
}
```

**Pipeline:** `receiver (WorkingBankEntry)` → `run()` → `socket_sender` → `transmit()` → UDP/QUIC
                                                         → `blockstore_sender` → `record()` → RocksDB

---

#### check_retransmit_signals (satır 371-413)

Kendisinin ürettiği shred'leri yeniden yayımlar. Fork geçişlerinde veya yavaş peer'lar için.

---

### 3.3 broadcast_stage/standard_broadcast_run.rs — Standart Yayım

#### Struct: StandardBroadcastRun (satır 27-39)

```rust
#[derive(Clone)]
pub struct StandardBroadcastRun {
    process_shreds_stats: ProcessShredsStats,
    transmit_shreds_stats: Arc<Mutex<SlotBroadcastStats<TransmitShredsStats>>>,
    insert_shreds_stats: Arc<Mutex<SlotBroadcastStats<InsertShredsStats>>>,
    unfinished_slot: Option<UnfinishedSlotInfo>,
    current_slot_and_parent: Option<(u64, u64)>,
    slot_broadcast_start: Option<Instant>,
    shred_version: u16,
    last_datapoint_submit: Arc<AtomicInterval>,
    num_batches: usize,
    cluster_nodes_cache: Arc<ClusterNodesCache<BroadcastStage>>,
    reed_solomon_cache: Arc<ReedSolomonCache>,
}
```

`unfinished_slot`: Önceki slot'tan henüz tamamlanmamış shred bilgisi.
`reed_solomon_cache`: Reed-Solomon FEC matrisleri cache'lenir (hesaplama maliyetli).

---

#### entries_to_shreds (satır 109-201)

Entry → Shred dönüşümü:

```rust
fn entries_to_shreds(&mut self, keypair, entries, blockstore, reference_tick, is_slot_end, ...) {
    let shredder = Shredder::new(slot, parent_slot, reference_tick, self.shred_version).unwrap();
    let (data_shreds, coding_shreds) = shredder.entries_to_shreds(
        keypair, entries, is_slot_end,
        chained_merkle_root,  // Önceki slot'un son shred'inin Merkle kökü
        next_shred_index, next_code_index,
        true, // merkle_variant
        &self.reed_solomon_cache,
        process_stats,
    );
    // unfinished_slot güncelle
}
```

**Shred zinciri:** Her slot'un son shred'i bir sonraki slot'un ilk shred'ine chain'lenir (`chained_merkle_root`). Fork detection için kullanılır.

---

#### process_receive_results (satır 227-370'e uzanır)

1. Slot değiştiyse: `finish_prev_slot()` — önceki slotun son shred'lerini üret
2. `entries_to_shreds()` — data ve coding shred'leri üret
3. İlk data shred'i (index 0) blockstore'a hemen kaydet — duplicate broadcast önleme
4. `socket_sender` ve `blockstore_sender`'a gönder

---

### 3.4 broadcast_stage/broadcast_utils.rs — recv_slot_entries

#### Struct: UnfinishedSlotInfo (satır 30-36)

```rust
#[derive(Clone)]
pub struct UnfinishedSlotInfo {
    pub(super) chained_merkle_root: Hash,
    pub next_shred_index: u32,
    pub(crate) next_code_index: u32,
    pub slot: Slot,
    pub parent: Slot,
}
```

---

#### recv_slot_entries (satır 38-101)

```rust
pub(super) fn recv_slot_entries(receiver: &Receiver<WorkingBankEntry>) -> Result<ReceiveResults> {
    let target_batch_bytes = 32 * ShredData::capacity(None).unwrap() as u64;
    // İlk entry'yi 1s timeout ile al
    let (mut bank, (entry, mut last_tick_height)) = receiver.recv_timeout(1s)?;
    let mut entries = vec![entry];

    // Slot dolana kadar drain
    while last_tick_height != bank.max_tick_height() {
        let Ok((try_bank, (entry, tick_height))) = receiver.try_recv() else { break; };
        if try_bank.slot() != bank.slot() {
            // Slot değişti, önceki slot iptal
            entries.clear(); bank = try_bank;
        }
        entries.push(entry);
    }

    // 50ms coalesce: 32 shred'lik batch'e ulaşmaya çalış
    while ... && serialized_batch_byte_count < target_batch_bytes {
        let Ok((try_bank, (entry, tick_height))) = receiver.recv_deadline(coalesce_start + 50ms) ...;
        entries.push(entry);
    }
}
```

**Coalesce optimizasyonu:** 50ms bekleyerek entry'leri biriktir, ~32 shred'lik batch oluşturmaya çalış. Reed-Solomon FEC verimi için önemli.

---

#### get_chained_merkle_root_from_parent (satır 104-129)

```rust
pub(super) fn get_chained_merkle_root_from_parent(slot, parent, blockstore) -> Result<Hash> {
    if slot == parent { return Ok(Hash::default()); }  // Genesis
    let index = blockstore.meta(parent)?.last_index?;
    let shred = blockstore.get_data_shred(parent, index)?;
    shred::layout::get_merkle_root(&shred)
}
```

Önceki slot'un son shred'inin Merkle kökünü okur. Slot zinciri bütünlüğü için.

---

### 3.5 turbine/src/retransmit_stage.rs — Validator Retransmit

#### Struct: ShredDeduper (satır 135-138)

```rust
struct ShredDeduper<const K: usize> {
    deduper: Deduper<K, [u8]>,                         // Ham shred dedup
    shred_id_filter: Deduper<K, (ShredId, usize)>,    // (ShredId, duplicate_sayısı) dedup
}
```

İki katmanlı deduplication:
1. Tam shred içeriği dedup (flood protection)
2. ShredId + duplicate index dedup (legitimate duplicate detection için boşluk)

`MAX_DUPLICATE_COUNT = 2`: Aynı ShredId için 2 farklı shred'e izin verilir (duplicate block detection senaryosu).

---

#### retransmit fonksiyonu (satır 168-304)

```rust
fn retransmit(thread_pool, bank_forks, leader_schedule_cache, cluster_info, shreds_receiver,
              sockets, quic_endpoint_sender, stats, cluster_nodes_cache, shred_deduper, ...) {
    let mut shreds = shreds_receiver.recv_timeout(1s)?;
    shreds.extend(shreds_receiver.try_iter().flatten());  // Drain

    // Her shred için slot_leader bul ve cluster_nodes al
    let shreds: Vec<_> = shreds.into_iter()
        .filter_map(|shred| {
            let key = shred::layout::get_shred_id(&shred)?;
            if shred_deduper.dedup(key, &shred, MAX_DUPLICATE_COUNT) { return None; }
            Some((key, shred))
        })
        .into_group_map_by(|(key, _)| key.slot())
        .into_iter()
        .filter_map(|(slot, shreds)| {
            let slot_leader = leader_schedule_cache.slot_leader_at(slot, ...)?;
            let cluster_nodes = cluster_nodes_cache.get(slot, ...);
            Some(izip!(shreds, repeat(slot_leader), repeat(cluster_nodes)))
        })
        .flatten().collect();

    // Paralel retransmit
    if shreds.len() < 2 {
        // Seri
    } else {
        thread_pool.install(|| {
            shreds.into_par_iter().filter_map(|(key, shred), slot_leader, cluster_nodes)| {
                retransmit_shred(&key, &shred, &slot_leader, &root_bank, &cluster_nodes, ...)
            }).fold(...).reduce(...)
        })
    }
}
```

---

#### retransmit_shred (satır 306-362)

```rust
fn retransmit_shred(key, shred, slot_leader, root_bank, cluster_nodes, socket_addr_space, socket, quic_endpoint_sender, stats) {
    let data_plane_fanout = cluster_nodes::get_data_plane_fanout(key.slot(), root_bank);
    let (root_distance, addrs) = cluster_nodes.get_retransmit_addrs(slot_leader, key, data_plane_fanout)?;
    let addrs = addrs.into_iter().filter(|addr| socket_addr_space.check(addr)).collect();

    match cluster_nodes::get_broadcast_protocol(key) {
        Protocol::QUIC => {
            // QUIC datagram
            addrs.into_iter().filter_map(|addr| quic_endpoint_sender.try_send((addr, shred.clone())).ok()).count()
        }
        Protocol::UDP => {
            multi_target_send(socket, shred, &addrs)  // sendmmsg
        }
    };
}
```

`multi_target_send`: Linux `sendmmsg` syscall ile tek seferde çok hedefe gönderim. Syscall overhead'ini azaltır.

---

#### Struct: RetransmitStage (satır 421-423)

```rust
pub struct RetransmitStage {
    retransmit_thread_handle: JoinHandle<()>,
}
```

`retransmitter()` fonksiyonu ile başlatılan tek thread. Thread ismi: `"solRetransmittr"`. İçinde `num_threads = min(get_thread_count(), 8).max(sockets.len())` Rayon pool.

---

#### RetransmitStats (satır 63-79)

```rust
struct RetransmitStats {
    since: Instant,
    num_nodes: AtomicUsize,
    num_addrs_failed: AtomicUsize,
    num_loopback_errs: AtomicUsize,
    num_shreds: usize,
    num_shreds_skipped: usize,
    num_small_batches: usize,
    total_batches: usize,
    total_time: u64,
    epoch_fetch: u64,
    epoch_cache_update: u64,
    retransmit_total: AtomicU64,
    compute_turbine_peers_total: AtomicU64,
    slot_stats: LruCache<Slot, RetransmitSlotStats>,
    unknown_shred_slot_leader: usize,
}
```

Her 2 saniyede bir `datapoint_info!("retransmit-stage", ...)` ile yayımlanır.

---

### 3.6 turbine/src/sigverify_shreds.rs — Shred İmza Doğrulama

#### Sabitler (satır 20-22)

```rust
const DEDUPER_FALSE_POSITIVE_RATE: f64 = 0.001;
const DEDUPER_NUM_BITS: u64 = 637_534_199;  // ~76MB bloom filter
const DEDUPER_RESET_CYCLE: Duration = Duration::from_secs(5 * 60);  // 5 dakikada reset
```

76MB bloom filter: `n = -bits * ln(2)^2 / ln(p)`. p=0.001 ile yaklaşık 5M element destekler.

---

#### spawn_shred_sigverify (satır 31-79)

Ana entry point. Thread: `"solShredVerifr"`.

```rust
pub fn spawn_shred_sigverify(cluster_info, bank_forks, leader_schedule_cache,
    shred_fetch_receiver, retransmit_sender, verified_sender) -> JoinHandle<()>
{
    let thread_pool = ThreadPoolBuilder::new().num_threads(get_thread_count()).build();
    let run = move || {
        let mut deduper = Deduper::<2, [u8]>::new(&mut rng, DEDUPER_NUM_BITS);
        loop {
            deduper.maybe_reset(&mut rng, 0.001, 5min);
            run_shred_sigverify(&thread_pool, &cluster_info.id(), ...);
        }
    };
    Builder::new().name("solShredVerifr").spawn(run)
}
```

---

#### run_shred_sigverify (satır 82-139)

```rust
fn run_shred_sigverify<const K: usize>(...) -> Result<(), Error> {
    let packets = shred_fetch_receiver.recv_timeout(1s)?;
    let mut packets: Vec<_> = once(packets).chain(shred_fetch_receiver.try_iter()).collect();

    // 1. Dedup
    stats.num_duplicates += thread_pool.install(|| {
        packets.par_iter_mut().flatten()
            .filter(|p| !p.meta().discard() && deduper.dedup(p.data(..)))
            .map(|p| p.meta_mut().set_discard(true))
            .count()
    });

    // 2. Slot leader bularak imza doğrula
    verify_packets(thread_pool, self_pubkey, bank_forks, leader_schedule_cache, recycler_cache, &mut packets);

    // 3. Repair shred'leri retransmit'ten hariç tut
    let shreds: Vec<_> = packets.iter().flat_map(...)
        .filter(|p| !p.meta().discard() && !p.meta().repair())
        .filter_map(shred::layout::get_shred)
        .map(<[u8]>::to_vec).collect();

    retransmit_sender.send(shreds)?;
    verified_sender.send(packets)?;
}
```

**Pipeline:** fetch → dedup → sigverify → split(retransmit, verified)

---

#### verify_packets (satır 141-158)

```rust
fn verify_packets(thread_pool, self_pubkey, bank_forks, leader_schedule_cache, recycler_cache, packets) {
    let working_bank = bank_forks.read().unwrap().working_bank();
    let leader_slots: HashMap<Slot, Pubkey> =
        get_slot_leaders(self_pubkey, packets, leader_schedule_cache, &working_bank)
            .into_iter()
            .filter_map(|(slot, pubkey)| Some((slot, pubkey?)))
            .chain(once((Slot::MAX, Pubkey::default())))
            .collect();
    let out = verify_shreds_gpu(thread_pool, packets, &leader_slots, recycler_cache);
    solana_perf::sigverify::mark_disabled(packets, &out);
}
```

`verify_shreds_gpu`: GPU ed25519 doğrulama. `leader_slots` haritası: her slot için beklenen lider pubkey'i.

`Slot::MAX → Pubkey::default()`: Bilinmeyen slot'lardaki shred'ler varsayılan pubkey ile kontrol edilir → başarısız → discard.

---

#### get_slot_leaders (satır 165-193)

```rust
fn get_slot_leaders(self_pubkey, batches, leader_schedule_cache, bank) -> HashMap<Slot, Option<Pubkey>> {
    batches.iter_mut().flat_map(PacketBatch::iter_mut)
        .filter(|p| !p.meta().discard())
        .filter(|p| {
            let slot = shred::layout::get_slot(shred::layout::get_shred(p))?;
            leaders.entry(slot).or_insert_with(|| {
                leader_schedule_cache.slot_leader_at(slot, Some(bank))
                    .filter(|leader| leader != self_pubkey)  // Loopback engelle
            }).is_none()  // None ise discard yap
        })
        .for_each(|p| p.meta_mut().set_discard(true));
    leaders
}
```

---

#### Struct: ShredSigVerifyStats (satır 218-228)

```rust
struct ShredSigVerifyStats {
    since: Instant,
    num_iters: usize,
    num_packets: usize,
    num_deduper_saturations: usize,
    num_discards_post: usize,
    num_discards_pre: usize,
    num_duplicates: usize,
    num_retransmit_shreds: usize,
    elapsed_micros: u64,
}
```

Her 2 saniyede `"shred_sigverify"` datapoint.

---

### 3.7 turbine/src/quic_endpoint.rs — QUIC Taşıma

#### Sabitler (satır 37-55)

```rust
const CLIENT_CHANNEL_BUFFER: usize = 1 << 14;    // 16384
const ROUTER_CHANNEL_BUFFER: usize = 64;
const CONNECTION_CACHE_CAPACITY: usize = 3072;
const ALPN_TURBINE_PROTOCOL_ID: &[u8] = b"solana-turbine";
const DATAGRAM_RECEIVE_BUFFER_SIZE: usize = 256 * 1024 * 1024;  // 256MB
const DATAGRAM_SEND_BUFFER_SIZE: usize = 128 * 1024 * 1024;     // 128MB
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(4);
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const MINIMUM_MAXIMUM_TRANSMISSION_UNIT: u16 = 1280;
```

QUIC datagram modu kullanılır (stream değil). Shred kaybına toleranslı: UDP gibi unreliable ama QUIC'in connection management'ı var.

Connection cache: 3072 bağlantı. Büyük cluster için yeterli.

---

#### Error enum (satır 65-80)

```rust
pub enum Error {
    ChannelSendError,
    ConnectError(ConnectError),
    ConnectionError(ConnectionError),
    InvalidIdentity(SocketAddr),
    IoError(IoError),
    SendDatagramError(SendDatagramError),
    TlsError(rustls::Error),
}
```

---

## BÖLÜM 4: PoH Hash Zinciri — Bütünleşik Matematik

### Tam PoH Zinciri Algoritması

```
Başlangıç: H_0 = genesis_hash

// Tick entry üretimi:
H_t = SHA256^(hashes_per_tick)(H_{t-1})
Entry { num_hashes: hashes_per_tick, hash: H_t, transactions: [] }

// Transaction entry üretimi (tick arasında):
tx_mixin = Merkle_SHA256(tx_signatures)
H_r = SHA256^(k)(H_{prev})  // k hashes önceden
H_r = SHA256(H_r || tx_mixin)  // mixin karıştır
Entry { num_hashes: k+1, hash: H_r, transactions: [...] }

// Slot sonu:
// Son H değeri = o slot'un blockhash
// PoH sıfırla: H_0_new = blockhash

// Verifikasyon (tek bir entry için):
// Tick: verify(start_hash, entry) = (SHA256^(num_hashes)(start_hash) == entry.hash)
// Record: verify(start_hash, entry) = (SHA256(SHA256^(num_hashes-1)(start_hash) || merkle_root(txs)) == entry.hash)
```

### Zincir Özellikleri

1. **VDF (Verifiable Delay Function):** Her hash en az T_hash nanosaniye sürer. `num_hashes * T_hash` = geçen sürenin alt sınırı.

2. **Bağlayıcılık:** Her entry önceki entry'ye bağımlı. Tarihin yeniden yazılması için tüm zincirin yeniden hesaplanması gerekir.

3. **Transaction binding:** Transaction'lar mixin olarak zincire gömülür. Sırası ve zamanlaması değiştirilemez.

4. **Paralel doğrulama:** Her validator, PoH üretimini doğrularken GPU SIMD kullanır. Üretim tek thread, doğrulama çok thread.

---

## BÖLÜM 5: Nova Shield Chain İçin Dersler

### 5.1 PoH'tan Alınacak Dersler

**Ders 1: VDF olarak SHA-256 zinciri yeterli ama pahalı**
Solana 800,000 SHA-256/slot yapıyor. Nova Shield, BLS12-381 VDF veya arkworks tabanlı snark-friendly VDF kullanabilir. Bu zinciri hem VDF hem de consensus proof olarak kullanmak mümkün.

**Ders 2: tick/record ayrımı kusursuzdur**
Tick = zaman geçti delili. Record = bu zamanda bu işlem gerçekleşti. Bu iki kavramın ayrılması temiz bir API sağlar. Nova Shield'in PoH benzeri mekanizması aynı ayrımı korumalıdır.

**Ders 3: hashes_per_tick kalibrasyonu kritik**
Farklı donanımda aynı wall-clock süreyi hedeflemek için `compute_hashes_per_tick()`. Nova Shield da benzer kalibrasyon yapmalı.

**Ders 4: tick_cache flush stratejisi akıllıca**
Lider olmadığında tick'ler cache'lenir. Lider olunduğunda hepsi bir anda gönderilir. Min/max tick height kontrolleri sayesinde çakışma olmaz. Nova Shield blok üretim pipelinesi benzer buffer stratejisi kullanabilir.

**Ders 5: CPU pin + busy-wait**
PoH thread'i tek CPU core'a sabitlenir, spin_loop ile zaman kontrolü yapılır. Jitter minimize edilir. Nova Shield'in BFT consensus timing'i için benzer teknik uygulanabilir.

**Ders 6: PohService/PohRecorder kilitsiz ayırım**
`poh` Arc<Mutex<Poh>> → sadece hash üretimi
`poh_recorder` RwLock<PohRecorder> → tick/record ve flush

Sık path (hashing) hafif kilit, nadir path (tick/record) ağır kilit. Bu pattern Nova Shield'in herhangi bir kritik path performans tasarımında kullanılabilir.

**Ders 7: DEFAULT_HASHES_PER_BATCH = 64 tune edilmiş değer**
Kilitlenme süresini transaction kayıt gecikmesiyle dengeler. Nova Shield benzer batch size optimization yapmalıdır.

**Ders 8: LeaderBankNotifier condvar pattern**
PoH başlangıcı ve bitişini diğer thread'lere bildiren basit state machine. Nova Shield'in slot/round geçiş bildirimleri için aynı pattern uygulanabilir.

### 5.2 Entry Doğrulamasından Alınan Dersler

**Ders 9: GPU + CPU paralel doğrulama mimarisi**
GPU `num_hashes-1` hash yapıyor, CPU son adımı (mixin/tick) tamamlıyor. Nova Shield ZK proof doğrulamasında GPU offload + CPU final check aynı şekilde yapılabilir.

**Ders 10: SIMD AVX-512 ile 16 paralel SHA256**
libpoh-simd.so ile 16x hızlanma. Nova Shield'in hash-yoğun işlemlerinde (Merkle proof, BLS hash vs.) SIMD optimizasyonu kritik.

**Ders 11: VerifyRecyclers — memory pool**
GPU doğrulama buffer'larını recycle etmek allocator pressure'ı azaltır. Nova Shield'in ZK verifier'ı benzer memory pool kullanmalı.

**Ders 12: Merkle ağacı transaction hash**
Transaction'ları signature'larından Merkle hash yapıp mixin olarak kullanmak güçlü bir yapı. Hem sıra bağımlılığı hem de içerik bütünlüğü sağlanıyor. Nova Shield transaction commitments için aynı yapıyı kullanabilir.

**Ders 13: verify_tick_hash_count — slot sınırı aşan durum bilgisi**
`tick_hash_count` bir entry slice'dan diğerine taşınıyor. Nova Shield'in VDF/round doğrulaması da slot sınırlarını aşabilir; durum bilgisinin caller'da tutulması tercih edilmeli.

### 5.3 Turbine'den Alınan Dersler

**Ders 14: Stake-ağırlıklı deterministik ağaç**
`ChaChaRng::from_seed(shred.seed(slot_leader))` ile tüm cluster aynı ağacı hesaplar. Koordinasyon sıfır, güvenilirlik yüksek. Nova Shield'in blok yayımı da aynı deterministik mantığı kullanabilir.

**Ders 15: Fanout=200 matematik**
200^1 = 200, 200^2 = 40,000, 200^3 = 8M. 4 hop ile tüm bilinen node'lara ulaşılır. Nova Shield P2P gossip fanout hesaplamasında bu formül kullanılabilir.

**Ders 16: Merkle shred chaining**
Her slot'un son shred'i bir sonraki slot'un ilk shred'ine zincirlenmiş (`chained_merkle_root`). Bu yapı, fork detection'ı O(1) yapar. Nova Shield blok header'larına benzer Merkle chaining eklenebilir.

**Ders 17: Turbine tree ile neighborhood kavramı**
Fiziksel ağ topolojisinden bağımsız mantıksal katmanlar. Her node'un coğrafi konumu yerine stake ağırlığı belirler. Nova Shield validatör ağı için saf logik katman tasarımı tercih edilmeli.

**Ders 18: Shred deduplication — Bloom filter**
76MB bloom filter, 5 dakikada bir reset, %0.1 false positive. Flood attack'lara karşı ucuz koruma. Nova Shield P2P katmanında benzer probabilistic dedup kullanılabilir.

**Ders 19: recv_slot_entries coalesce (50ms)**
Küçük entry'leri biriktirerek 32 shred'lik tam batch oluşturmaya çalışır. Reed-Solomon FEC verimliliği için. Nova Shield'in erasure coding kullandığı her yerde benzer batch coalesce stratejisi uygulanabilir.

**Ders 20: BroadcastStage pipeline tasarımı**
Entry → Shred → UDP/QUIC, Blockstore → 3 ayrı kanal, 5+ thread. Her aşama bağımsız thread'de çalışır, backpressure channel ile kontrol edilir. Nova Shield yayım pipeline'ı aynı actor-like yapıyı kullanabilir.

**Ders 21: QUIC datagram modu**
Turbine, stream yerine QUIC datagram kullanıyor — UDP semantiği ama QUIC'in TLS ve connection management faydaları. Nova Shield P2P katmanında libp2p yerine veya üstüne QUIC datagram düşünülebilir.

**Ders 22: Repair hariç tutma**
Repair paketleri retransmit'ten hariç tutulur (meta().repair() kontrolü). Bu ayrım, normal yayım vs. eksik shred recovery'yi ayırır. Nova Shield'de de farklı paket türleri (normal yayım, recovery, gossip) net etiketlemeli.

**Ders 23: get_data_plane_fanout deneyleri**
Mainnet'te %2 slot'ta farklı fanout değerleri deneniyor. Bu A/B test yaklaşımı, Nova Shield'in mainnet'e çıktıktan sonra parametre optimizasyonu için ilham verici.

---

## BÖLÜM 6: Dosya Özeti Tablosu

| Dosya | Satır | Ana Kavram |
|-------|-------|------------|
| `entry/src/poh.rs` | 332 | Poh struct, SHA256 zincirleme, tick/record matematiği |
| `poh/src/poh_service.rs` | 572 | Arka plan hash servisi, CPU pin, busy-wait throttle |
| `poh/src/poh_recorder.rs` | ~1300 | PoH/Bank senkronizasyonu, tick cache, flush logic |
| `poh/src/leader_bank_notifier.rs` | 275 | Condvar state machine, leader slot lifecycle |
| `entry/src/entry.rs` | ~1100 | Entry struct, CPU/GPU/SIMD doğrulama, Merkle tx hash |
| `turbine/src/cluster_nodes.rs` | ~800 | Turbine ağaç algoritması, weighted shuffle, fanout |
| `turbine/src/broadcast_stage.rs` | ~450 | Lider yayım pipeline, çoklu thread mimarisi |
| `turbine/src/broadcast_stage/standard_broadcast_run.rs` | ~400 | Entry→Shred dönüşüm, Merkle chaining |
| `turbine/src/broadcast_stage/broadcast_utils.rs` | 232 | recv_slot_entries, coalesce, chained merkle root |
| `turbine/src/retransmit_stage.rs` | ~450 | Validator retransmit, paralel shred iletimi |
| `turbine/src/sigverify_shreds.rs` | 334 | GPU shred imza doğrulama, bloom filter dedup |
| `turbine/src/quic_endpoint.rs` | ~500+ | QUIC datagram taşıma, connection cache |
| `turbine/src/broadcast_stage/broadcast_metrics.rs` | ~150 | Metrik struct'ları, datapoint raporlama |

---

*Agent 20 — PoH + Turbine + Entry tam analizi tamamlandı.*
