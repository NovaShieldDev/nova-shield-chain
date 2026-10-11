# Solana Analiz — Agent 29
## Kapsam: perf/ + send-transaction-service/ + streamer/ (25+ dosya)

**Tarih:** 2026-07-22
**Toplam Satır:** ~3200+ satır kaynak kod analiz edildi
**Dosya Sayısı:** 25 dosya (perf/: 12, send-transaction-service/: 3, streamer/: 13 [nonblocking/ dahil])

---

## BÖLÜM 1: perf/src/ Modülü

### [lib.rs] (93 satır)

**Amaç:** perf crate'inin kök modülü; CUDA, AVX, AVX2 CPU özellik tespiti ve platform doğrulaması yapar.

**Kritik Fonksiyonlar:**
- `is_rosetta_emulated()` (satır 30-47): macOS üzerinde Rosetta 2 emülasyonu çalışıp çalışmadığını `sysctl -in sysctl.proc_translated` çıktısına bakarak tespit eder. Emüle ortamda AVX kontrolleri atlanır.
- `report_target_features()` (satır 49-93): CUDA etkin mi raporlar. `build_target_feature_avx` ve `build_target_feature_avx2` build flag'lerine göre runtime'da CPU desteğini kontrol eder; eksikse `std::process::abort()` çağırır — kötü binary build'ini erken öldürür.

**Önemli Sabitler:** Yok, derleme zamanı flag'leri kullanır.

**Nova Shield İçin Öğrenilen:**
- Validator başlangıcında CPU özellik uyumsuzluğunu erken tespit etmek kritiktir — `abort()` kullanımı doğru seçim.
- Rosetta/emülasyon tespiti cross-platform Rust koduyla yapılabilir.

---

### [sigverify.rs] (600+ satır)

**Amaç:** Ed25519 imza doğrulamasını GPU (perf-libs/libcuda-crypt.so) veya CPU (rayon paralel) ile yapar.

**Struct/Enum/Trait Listesi:**
- `PacketOffsets` (satır 49-73): `sig_len: u32`, `sig_start: u32`, `msg_start: u32`, `pubkey_start: u32`, `pubkey_len: u32` — paket içindeki imza, mesaj ve pubkey pozisyonlarını tutar.
- `PacketError` enum (satır 76-86): `InvalidLen`, `InvalidPubkeyLen`, `InvalidShortVec`, `InvalidSignatureLen`, `MismatchSignatureLen`, `PayerNotWritable`, `InvalidProgramIdIndex`, `InvalidProgramLen`, `UnsupportedVersion` — 9 varyant.
- `TxOffset = PinnedVec<u32>` (satır 44): GPU'ya geçirilecek offset vektörü.
- `TxOffsets = (TxOffset, TxOffset, TxOffset, TxOffset, Vec<Vec<u32>>)` (satır 46): (sig_offsets, pubkey_offsets, msg_start_offsets, msg_sizes, per-batch sig sayıları).

**Kritik Sabitler:**
- `TRACER_KEY_BYTES: [u8; 32]` (satır 27): tKeYE4wtowRb8yRroZShTipE18YVnqwXjsSAoNsFU6g — debug/tracing için özel public key.
- `TRACER_KEY_OFFSET_IN_TRANSACTION: usize = 69` (satır 32): İşlemin 69. byte'ında bu key'e bakılır.
- `VERIFY_PACKET_CHUNK_SIZE: usize = 128` (satır 34): CPU verify batch boyutu — "~8ms gecikmeyi kısıtlamak için empirik olarak türetildi."

**Lazy Static:**
- `PAR_THREAD_POOL: ThreadPool` (satır 36-42): `get_thread_count()` kadar thread ile `solSigVerify{i:02}` adlı rayon thread pool.

**Kritik Fonksiyonlar:**

- `init()` (satır 100-108): GPU API varsa `ed25519_init()` ve `ed25519_set_verbose()` çağırır.

- `verify_packet()` (satır 113-153): Tek paket CPU doğrulama. Döngüyle her imzayı kontrol eder:
  1. `get_packet_offsets()` ile pozisyonlar alınır.
  2. Her imza için: `Signature::try_from(...)`, pubkey byte'ları ve mesaj byte'ları çıkarılır.
  3. `signature.verify(pubkey, message)` çağrısı — stack üzerinde, sıralı doğrulama.

- `do_get_packet_offsets()` (satır 188-312): İç kullanım, paket içindeki offsetleri çözer. Kritik güvenlik mantığı:
  - `short_vec` ile sig_len decode edilir (güvenilmez: sig_len_untrusted).
  - MessageHeader.num_required_signatures (güvenilir: sig_len_maybe_trusted) ile karşılaştırılır.
  - `MismatchSignatureLen` dönerse paket reddedilir.
  - `PayerNotWritable` kontrolü: `num_required_signatures > num_readonly_signed_accounts` zorunlu.
  - Versioned mesaj desteği: `MESSAGE_VERSION_PREFIX` biti set ise v0 mesaj işlenir, diğer sürümler `UnsupportedVersion` verir.

- `check_for_simple_vote_transaction()` (satır 345-426): Tek talimatla vote program'a çağrı yapan transaction'ları tespit eder; `SIMPLE_VOTE_TX` flag'i set edilir. `sig_len > 2` ise zaten vote değildir.

- `generate_offsets()` (satır 428-482): Tüm batch'lerdeki paketleri dolaşarak GPU için 4 ayrı `PinnedVec<u32>` üretir (sig_offsets, pubkey_offsets, msg_start_offsets, msg_sizes). Her paket `size_of::<Packet>()` boyutlu stride ile tutulur — GPU doğrudan bellekten okur.

- `shrink_batches()` (satır 485-516): İnplace: discard edilmiş paketlerin yerine geçerli paketleri taşır, batch sayısını küçültür. O(n) complexity.

- `ed25519_verify_cpu()` (satır 518-535): rayon PAR_THREAD_POOL ile `par_chunks_mut(VERIFY_PACKET_CHUNK_SIZE=128)` — her chunk'ta `verify_packet()` çağrısı. Tamamen paralel.

- `ed25519_verify_disabled()` (satır 537-546): Test modunda tüm paketlerin `discard` flag'ini false yapar.

- `copy_return_values()` (satır 548-561): GPU'dan gelen `out` buffer'ını (1=geçerli, 0=geçersiz) paket batch'lerine mappler.

- `ed25519_verify()` (satır 601-676): **Ana giriş noktası**. GPU/CPU kararı:
  ```
  GPU eşiği: valid_percentage >= 90 VE valid_packet_count >= 64
  Düşük kaliteli batch → CPU path
  Yüksek kaliteli, yeterince büyük batch → GPU path
  ```
  GPU path:
  1. `generate_offsets()` ile PinnedVec'ler hazırlanır (sayfa pinlenmiş bellek).
  2. Her batch için `perf_libs::Elems` struct'ı oluşturulur.
  3. `api.ed25519_verify_many(...)` unsafe FFI çağrısı — `USE_NON_DEFAULT_STREAM = 1`.
  4. `copy_return_values()` → `mark_disabled()`.

- `check_packed_ge_small_order()` (satır 564-574): GPU API varsa `ed25519_check_packed_ge_small_order` çağrısı — grupun küçük mertebeli nokta olup olmadığını kontrol eder (malleable imza koruması).

- `get_checked_scalar()` (satır 576-589): Ed25519 scalar doğrulaması, GPU hızlandırmalı.

**Unsafe Bloklar:**
- Satır 654-671: `api.ed25519_verify_many(...)` FFI çağrısı. Raw pointer'lar (elems.as_ptr(), out.as_mut_ptr()). Risk: FFI sınır güvenliği — kütüphane doğru parametreler almazsa UB.
- Satır 567-569: `api.ed25519_check_packed_ge_small_order(ge.as_ptr())` — pointer geçimi.
- Satır 580-583: `api.ed25519_get_checked_scalar(...)` — pointer çifti.

**Test Coverage:**
- `test_copy_return_values`: rasgele sig_lens matrisleriyle GPU çıktı haritalama.
- `test_layout`, `test_system_transaction_layout`: paket içi offset'lerin doğru konumda olduğunu doğrular.
- `test_untrustworthy_sigs`: güvenilmez sig sayısı → `MismatchSignatureLen`.
- `test_small_packet`, `test_pubkey_too_small`, `test_pubkey_len`: sınır aşımı korumaları.

**Nova Shield İçin Öğrenilen:**
- GPU/CPU eşik kararı (>= 64 paket + >= %90 geçerlilik): GPU başlatma maliyetini telafi etmek için pratik formül.
- Dual sig_len doğrulaması (untrusted short_vec + trusted MessageHeader) — imza sahteciliğine karşı çift kontrol.
- `SIMPLE_VOTE_TX` ayrıştırması performance path için kritik — farklı iş yükleri farklı pipeline'dan geçer.
- `shrink_batches()` in-place compaction: batch boyutunu küçülterek sonraki stage'lere daha az veri geçer.

---

### [packet.rs] (285 satır)

**Amaç:** `PacketBatch` veri yapısı — pinlenmiş bellek kullanan, recycler destekli paket koleksiyonu.

**Struct/Enum/Trait Listesi:**
- `PacketBatch` (satır 22-24): İçinde `PinnedVec<Packet>` tutar. `AbiExample` implement eder.
- `PacketBatchRecycler = Recycler<PinnedVec<Packet>>` (satır 26): Tip alias.

**Önemli Sabitler:**
- `NUM_PACKETS: usize = 1024 * 8 = 8192` (satır 16): Test batch boyutu.
- `PACKETS_PER_BATCH: usize = 64` (satır 18): Bir batch'teki paket sayısı (recvmmsg ile uyumlu).
- `NUM_RCVMMSGS: usize = 64` (satır 19): Aynı değer, `recvmmsg()` çağrısı başına maksimum paket.

**Kritik Fonksiyonlar:**
- `new_with_recycler()`: Belleği hem pinlenmiş hem de recycler'dan alır.
- `new_unpinned_with_recycler_data_and_dests()` (satır 75-98): `(SocketAddr, T)` listesini batch'e dönüştürür. Hatalı adres (unspecified/port 0) olan paketleri atlar.
- `deserialize_from_with_limit()` (satır 231-243): `PACKET_DATA_SIZE` ile bincode limit — memory exhaustion saldırısına karşı koruma.

**Unsafe Bloklar:**
- `PacketBatch::set_len()` (satır 160-162): `Vec::set_len()` ile bilinmeyen uzunluğu set eder. Önceden initialize edilmiş elementler gerektirir; özellikle `meta` alanı init edilmeli.

**Nova Shield İçin Öğrenilen:**
- `PACKETS_PER_BATCH = NUM_RCVMMSGS = 64`: recvmmsg ve batch boyutu birebir eşleşir — sıfır kopyayla uyumlu pipeline.
- `deserialize_from_with_limit()` patterni: ağdan gelen tüm serializasyonlarda boyut sınırı zorunlu.
- `with_recycler` vs `unpinned`: GPU doğrulama gereken path'ler pinli bellek kullanır, diğerleri kullanmaz.

---

### [deduper.rs] (282 satır)

**Amaç:** Gelen paketleri multi-hash bloom filter tabanlı deduplication ile duplicate olarak işaretler.

**Struct/Enum/Trait Listesi:**
- `Deduper<const K: usize, T: ?Sized>` (satır 16-23):
  - `num_bits: u64`: Bloom filter toplam bit sayısı.
  - `bits: Vec<AtomicU64>`: Lock-free bitset.
  - `state: [RandomState; K]`: K adet bağımsız hash fonksiyonu.
  - `clock: Instant`: Son reset zamanı.
  - `popcount: AtomicU64`: Set edilmiş bit sayısı (false positive rate hesabı için).

**Kritik Fonksiyonlar:**

- `Deduper::new()` (satır 26-37):
  - `num_bits` → ceil(num_bits / 64) adet `AtomicU64` oluşturur.
  - K adet bağımsız `RandomState` üretir.

- `false_positive_rate()` (satır 39-43):
  - Formül: `(popcount / num_bits)^K`
  - K=2 için: ones_ratio^2 — klasik Bloom filter false positive formülü.

- `maybe_reset()` (satır 48-63):
  - False positive rate eşiği aşıldıysa veya `reset_cycle` geçtiyse: yeni random state üret, tüm bitleri sıfırla.
  - `saturated` flag döner.

- `dedup()` (satır 68-83): **Ana algoritma**:
  ```rust
  for her hasher in K adet hasher:
      hash = data.hash() % num_bits
      index = hash >> 6  (64-bit word indeksi)
      mask = 1 << (hash & 63)  (bit pozisyonu)
      old = bits[index].fetch_or(mask, Relaxed)
      if old & mask == 0: popcount++, out = false
  // out=true → duplicate, out=false → yeni
  ```
  K hasher'ın HEPSİ daha önce set edilmişse duplicate. Biri bile 0 ise yeni paket.

- `dedup_packets_and_count_discards()` (satır 90-114): Batch listesini dolaşır, her paketi `deduper.dedup(data)` ile kontrol eder. Duplicate ise `set_discard(true)`. `process_received_packet` callback çağrısı: `(packet, already_discarded, is_dup)`.

**Test Coverage:**
- `test_dedup_same`: 1024 aynı tx → sadece 1 geçmeli.
- `test_dedup_diff`: 1024 farklı tx → 0 discard.
- `test_dedup_saturated` (ignore): Saturasyon testi.
- `test_dedup_capacity`: Parametrik kapasite formülü: `capacity = num_bits * fpr^(1/K)`.
- `test_dedup_seeded`: ChaCha20 seed'li deterministik testler, kesin dup_count ve popcount doğrulanır.

**Nova Shield İçin Öğrenilen:**
- K=2 Bloom filter: Düşük false positive için yeterli, single pass dedup.
- `AtomicU64::fetch_or` ile lock-free concurrent dedup: multiple thread aynı anda güvenli çalışır.
- `maybe_reset()` ile time-based ve saturation-based periyodik reset: Production'da başarılı strateji.
- `num_bits = 63_999_979` ile `K=2` → ~%0.1 false positive, ~2M paket kapasitesi.

---

### [discard.rs] (34 satır)

**Amaç:** Çok fazla paket geldiğinde rastgele batch'ler atarak buffer overflow'u önler.

**Kritik Fonksiyonlar:**
- `discard_batches_randomly()` (satır 6-17):
  - `total_packets > max_packets` olduğu sürece döngü.
  - Her iterasyonda rastgele bir batch indeksi seçer (`thread_rng().gen_range(...)`).
  - `swap_remove()` ile O(1) silme (son batch ile yerini değiştirir).
  - `total_packets -= removed.len()` (saturating_sub ile taşma güvenli).

**Test Coverage:**
- `test_batch_discard_random`: 100 batch × 1 paket, max=5 → 5 batch kalır.

**Nova Shield İçin Öğrenilen:**
- Rastgele batch discard: Fair scheduling (tek kaynağı cezalandırmaz), saldırı dirençli.
- `swap_remove()`: Vec'te O(1) silme — sırayı bozmak kabul edilebilir.

---

### [recycler.rs] (250 satır)

**Amaç:** Bellek havuzu (object pool) — GC baskısını azaltmak için önceden ayrılmış nesneleri yeniden kullanır.

**Struct/Enum/Trait Listesi:**
- `RecyclerStats` (satır 26-31): `total: AtomicUsize`, `reuse: AtomicUsize`, `freed: AtomicUsize`, `max_gc: AtomicUsize`.
- `Recycler<T>` (satır 33-36): `recycler: Arc<RecyclerX<T>>` — klonlanabilir handle.
- `RecyclerX<T>` (satır 39-45):
  - `gc: Mutex<Vec<T>>`: Geri dönüştürülmüş nesnelerin deposu.
  - `stats: RecyclerStats`.
  - `id: usize`: Debug tanımlayıcı (0-999 random).
  - `size_factor: AtomicUsize`: EMA hesabı için büyütülmüş moving average.
- `Reset` trait (satır 69-75): `reset()`, `warm(size_hint)`, `set_recycler(Weak<RecyclerX<Self>>)`.

**Önemli Sabitler:**
- `RECYCLER_SHRINK_SIZE: usize = 1024` (satır 16): GC havuzundaki eşik — bu sayının üzerinde gereksiz nesne tutulmaz.
- `RECYCLER_SHRINK_WINDOW: usize = 16384` (satır 23): EMA penceresi. Half-life ≈ 11356 tahsis.

**Kritik Fonksiyonlar:**

- `Recycler::warmed()` (satır 91-106): `num` kadar nesne önceden tahsis eder ve `warm(size_hint)` ile önceden boyutlandırır. `WARM_RECYCLERS` flag'i false ise atlanır.

- `Recycler::allocate()` (satır 108-153):
  1. EMA güncellemesi: `size_factor = size_factor * (WINDOW-1)/WINDOW + gc.len()` (tam sayı aritmetiğiyle, WINDOW/2 rounding için eklenir).
  2. `gc.pop()` ile varsa mevcut nesneyi döndür (reset() çağrısıyla temizlenmiş).
  3. Yoksa `T::default()` ile yeni nesne oluştur ve `set_recycler(weak)` ile recycler referansını set et.

- `RecyclerX::recycle()` (satır 156-198):
  1. `gc.push(x)`.
  2. `gc.len() > RECYCLER_SHRINK_SIZE` VE `size_factor >= RECYCLER_SHRINK_SIZE * RECYCLER_SHRINK_WINDOW` ise: RECYCLER_SHRINK_SIZE'ın üzerindeki nesneleri drain et (set_recycler(Weak::default()) ile referansı temizle).
  3. max_gc istatistiği güncellenir.
  4. `datapoint_debug!("recycler", ...)` metrik.

**Shrink Algoritması Detayı:**
- `SIZE_FACTOR_AFTER_SHRINK = RECYCLER_SHRINK_SIZE * RECYCLER_SHRINK_WINDOW = 1024 * 16384 = 16_777_216`
- size_factor bu değere ulaşırsa: ortalama gc boyutu uzun süredir RECYCLER_SHRINK_SIZE'ın üzerinde — geçici burst sonrası nesneler atılabilir.

**Nova Shield İçin Öğrenilen:**
- EMA-tabanlı pool shrink: Burst sonrası bellek geri vermek için zeki strateji.
- `Weak<RecyclerX<T>>` ile `Drop` entegrasyonu: Nesne drop olunca otomatik olarak pool'a geri döner (bkz. PinnedVec::drop).
- `enable_recycler_warming()`: Production başlangıcında cache warmup — ilk request latency spike'ını önler.

---

### [recycler_cache.rs] (22 satır)

**Amaç:** `sigverify` için özel recycler koleksiyonu — hem TxOffset hem de raw buffer recycler tutar.

**Struct:**
- `RecyclerCache` (satır 4-7): `recycler_offsets: Recycler<TxOffset>`, `recycler_buffer: Recycler<PinnedVec<u8>>`.

**Kritik Fonksiyonlar:**
- `RecyclerCache::warmed()` (satır 10-15): Her iki recycler'ı 50 öğe, 4096 byte önceden ısıtır.

**Nova Shield İçin Öğrenilen:**
- GPU pipeline için iki ayrı recycler: offset vektörleri ve ham buffer'lar — farklı ömürlü nesneler ayrı havuzlarda tutulur.

---

### [data_budget.rs] (115 satır)

**Amaç:** Bant genişliği / veri hızı limiti — atomik CAS döngüsüyle lock-free token bucket.

**Struct:**
- `DataBudget` (satır 4-10):
  - `bytes: AtomicUsize`: Mevcut bütçe (byte cinsinden).
  - `asof: AtomicU64`: Son güncelleme zamanı (ms).

**Kritik Fonksiyonlar:**
- `take(size)` (satır 24-40): CAS döngüsüyle atomik `bytes -= size`. Yetersiz bütçe varsa false döner. **Lock-free**.
- `can_update(duration_millis)` (satır 43-59): `asof + duration_millis <= now` ise timestamp'i günceller. CAS döngüsüyle atomik zaman güncellemesi — birden fazla thread yarışsa sadece biri kazanır.
- `update(duration_millis, updater: Fn)` (satır 64-83): `can_update()` döndüyse `updater(bytes)` ile bütçeyi günceller. Örnek: `|bytes| bytes + 1_000_000` her saniye 1MB ekler.
- `check(size)` (satır 86-88): Consume etmeden sadece kontrol (non-atomic preview).
- `restricted()` (satır 14-19): Test için sonsuz zaman `asof = u64::MAX` — update asla çalışmaz, bütçe hep 0.

**Test Coverage:**
- `test_data_budget`: `take`, `update`, sleep ile zaman geçişi, timeout kontrolü.

**Nova Shield İçin Öğrenilen:**
- Lock-free token bucket: Network rate limiting için standart pattern.
- `can_update()` + `update()` separasyonu: Güncelleme yetkisi ile güncelleme mantığını ayırmak yarış koşullarını önler.
- `u64::MAX` ile "dondurulmuş bütçe" test pattern'i — Nova Shield testlerinde kullanılabilir.

---

### [cuda_runtime.rs] (355 satır)

**Amaç:** CUDA page-pinned memory wrapper — `cudaHostRegister/cudaHostUnregister` API'si üzerine `PinnedVec<T>`.

**Struct/Enum/Trait Listesi:**
- `PinnedVec<T: Default + Clone + Sized>` (satır 58-64):
  - `x: Vec<T>`: İç vektör.
  - `pinned: bool`: Page pinned mi?
  - `pinnable: bool`: Pin yapılabilir mi?
  - `recycler: Weak<RecyclerX<PinnedVec<T>>>`: Opsiyonel recycler referansı.
- `Reset for PinnedVec<T>` (satır 66-77): `reset()` → len=0; `warm()` → pinnable+resize; `set_recycler()` → weak ref set.

**Önemli Sabitler:**
- `CUDA_SUCCESS: c_int = 0` (satır 24).

**Kritik Fonksiyonlar:**
- `pin<T>(mem: &mut Vec<T>)` (satır 26-40): perf_libs API varsa `cudaHostRegister(ptr, size, 0)` çağrısı. CUDA_SUCCESS değilse `assert!` panic.
- `unpin<T>(mem: *mut T)` (satır 42-52): `cudaHostUnregister(ptr)`.
- `PinnedVec::reserve_and_pin()` (satır 157-170): Kapasite yetersizse önce unpin, sonra reserve, sonra tekrar pin.
- `prepare_realloc()` (satır 212-221): push/resize/append öncesinde: realloc olacaksa unpin.
- `check_ptr()` (satır 280-299): Realloc sonrası pointer/capacity değiştiyse yeniden pin.
- `PinnedVec::Drop` (satır 326-334): Recycler varsa `recycle(self)`, yoksa `unpin(ptr)`.
- `PinnedVec::from vec → Vec<T>` dönüşümü (satır 79-95): Recycler hala aktifse clone döndür (pinli nesneyi pool'a geri bırakmak için). Recycler ölmüşse unpin + mem::take.

**Unsafe Bloklar:**
- `pin()` içinde: `(api.cuda_host_register)(ptr as *mut c_void, size, 0)` — FFI, raw pointer.
- `unpin()` içinde: `(api.cuda_host_unregister)(mem as *mut c_void)` — FFI.
- `PinnedVec::set_len()` (satır 272-274): `self.x.set_len(size)` — invariant: elementler initialize edilmiş olmalı.
- Rayon par_iter: unsafe değil, Send+Sync bound'larıyla güvenli.

**Nova Shield İçin Öğrenilen:**
- Page-pinned memory: GPU DMA için zorunlu. Nova Shield GPU sigverify kullanacaksa bu pattern şart.
- `prepare_realloc()` + `check_ptr()` guards: Realloc'tan önce unpin, sonra re-pin. Aksi halde dangling pinned pointer.
- Recycler + Drop entegrasyonu: Bellek havuzunu RAII ile yönetmek — sızdırmaz.

---

### [perf_libs.rs] (188 satır)

**Amaç:** `libcuda-crypt.so` dinamik kütüphanesini `dlopen2` ile yükler ve Ed25519/PoH GPU fonksiyon pointer'larını tutar.

**Struct/Enum/Trait Listesi:**
- `Elems` (satır 15-19): `elems: *const u8`, `num: u32` — GPU'ya geçirilen batch descriptor. `#[repr(C)]`.
- `Api<'a>` (satır 21-82): `dlopen2::SymBorApi` derive — tüm GPU fonksiyon pointer'ları:
  - `ed25519_init: unsafe fn() -> bool`
  - `ed25519_set_verbose: unsafe fn(val: bool)`
  - `ed25519_verify_many: unsafe fn(vecs, num, message_size, total_packets, total_signatures, message_lens, pubkey_offsets, signature_offsets, signed_message_offsets, out, use_non_default_stream) -> u32`
  - `ed25519_sign_many`: Benzer parametre seti, imzalama için.
  - `poh_verify_many: unsafe fn(hashes, num_hashes_arr, num_elems, stream) -> c_int`
  - `cuda_host_register: unsafe fn(ptr, size, flags) -> c_int`
  - `cuda_host_unregister: unsafe fn(ptr) -> c_int`
  - `ed25519_get_checked_scalar: unsafe fn(out_scalar, in_scalar) -> c_int`
  - `ed25519_check_packed_ge_small_order: unsafe fn(packed_ge) -> c_int`

**Global State:**
- `static mut API: Option<Container<Api>> = None` (satır 84): Global GPU API handle.

**Kritik Fonksiyonlar:**
- `init(name: &OsStr)` (satır 86-98): `Once` guard ile tek seferlik `Container::load(name)`. Başarısız olursa `exit(1)`.
- `locate_perf_libs()` (satır 100-109): Executable dizininde `perf-libs/` klasörü arar.
- `find_cuda_home(perf_libs_path)` (satır 111-141): `CUDA_HOME` env var veya `/usr/local/cuda-*` klasörü.
- `append_to_ld_library_path()` (satır 143-150): `env::set_var("LD_LIBRARY_PATH", ...)`.
- `init_cuda()` (satır 152-175): `locate_perf_libs()` + `find_cuda_home()` → `init("libcuda-crypt.so")`. Bulamazsa son çare: kör yükleme.
- `api()` (satır 177-188): `TEST_PERF_LIBS_CUDA` env var set ise `init_cuda()`. Her çağrıda `unsafe { API.as_ref() }`.

**Unsafe Bloklar:**
- `init()` içinde: `INIT_HOOK.call_once(|| { API = Some(...) })` — static mut erişimi Once koruması altında.
- `api()` içinde: `API.as_ref()` — static mut okuma, Once garantisi olmadan (init sonrası read-only).

**Nova Shield İçin Öğrenilen:**
- `dlopen2` ile runtime GPU kütüphane yükleme: GPU yoksa graceful degradation.
- `api()` None döndürmesi = CPU path — aynı kod GPU ve CPU için.
- `TEST_PERF_LIBS_CUDA` env var: CI/test ortamında GPU davranışını simüle etmek için.

---

### [thread.rs] (133 satır)

**Amaç:** Thread önceliği yönetimi — Linux `nice(3)` syscall wrapper.

**Kritik Fonksiyonlar:**
- `nice(adjustment: i8)` (satır 5-18): Linux-only. `libc::nice()` wrapper. Return value -1 olup errno≠0 ise Err.
- `renice_this_thread(adjustment: i8)` (satır 26-33/40-48): Platform bağımlı:
  - Linux: `nice()` çağrısı — per-thread niceness (POSIX'ten farklı, Linux'a özgü).
  - Diğer: sadece 0 kabul edilir, else Err.
- `is_renice_allowed(adjustment: i8)` (satır 53-70): Negatif adjustment (priority artışı) için root veya `CAP_SYS_NICE` yetki kontrolü (`caps::has_cap`).
- `is_niceness_adjustment_valid()` (satır 72-89): i8 parse + `is_renice_allowed()`.

**Unsafe Bloklar:**
- `nice()` içinde (satır 6-16): `libc::__errno_location()` + `libc::nice()` — raw C errno ve syscall.

**Test Coverage:**
- `test_nice`: Thread spawn ederek nice değişimi, inheritance testi.
- `test_is_niceness_adjustment_valid`: Sınır testleri (+128 overflow, -129 underflow).

**Nova Shield İçin Öğrenilen:**
- Linux'ta niceness thread-level (process değil) — doğrulama thread'lerini sigverify'dan daha yüksek öncelikle çalıştırmak mümkün.
- `CAP_SYS_NICE` ile root olmadan öncelik artışı: Production deployment için çözüm.

---

### [test_tx.rs] (72 satır)

**Amaç:** Test transaction'ları oluşturmak için yardımcı fonksiyonlar.

**Kritik Fonksiyonlar:**
- `test_tx()`: Yeni Keypair, zero Hash ile 42 lamport transfer.
- `test_invalid_tx()`: Geçersiz imzalı tx.
- `test_multisig_tx()`: 2 imzalı, system+stake program tx.
- `new_test_vote_tx<R: CryptoRng + RngCore>()`: 1-5 rastgele slot'lu, %50 switch_proof_hash ile vote tx.

**Nova Shield İçin Öğrenilen:**
- Modüler test fixture'ları: Her tx tipi için ayrı fonksiyon — test isolation.

---

## BÖLÜM 2: send-transaction-service/src/ Modülü

### [lib.rs] (6 satır)

**Amaç:** Crate kökü — `send_transaction_service` ve `tpu_info` modüllerini dışa açar.

---

### [tpu_info.rs] (29 satır)

**Amaç:** TPU leader endpoint bilgisi soyutlaması.

**Struct/Enum/Trait Listesi:**
- `TpuInfo` trait (satır 3-12):
  - `refresh_recent_peers(&mut self)`: Leader bilgisini güncelle.
  - `get_leader_tpus(max_count, protocol) -> Vec<&SocketAddr>`: En fazla `max_count` leader TPU adresi.
  - `get_leader_tpus_with_slots(max_count, protocol) -> Vec<(&SocketAddr, Slot)>`: Adres + slot bilgisi.
- `NullTpuInfo` (satır 14-29): Test stub — boş implementasyon.

**Nova Shield İçin Öğrenilen:**
- Trait abstraction: Leader discovery mekanizması test edilebilir hale gelir (`NullTpuInfo`).
- Slot bilgisi: Hangi slot için hangi leader'a gönderildiği takip edilebilir.

---

### [send_transaction_service.rs] (1000+ satır)

**Amaç:** RPC'den gelen transaction'ları TPU'ya gönderir, retry mantığı yönetir, expired/rooted/failed işlemleri temizler.

**Önemli Sabitler:**
- `MAX_TRANSACTION_RETRY_POOL_SIZE: usize = 10_000` (satır 31): Retry kuyruğunun maksimum boyutu.
- `DEFAULT_RETRY_RATE_MS: u64 = 2_000` (satır 34): Her 2 saniyede bir retry.
- `DEFAULT_LEADER_FORWARD_COUNT: u64 = 2` (satır 37): Kaç leader'a iletilir.
- `DEFAULT_SERVICE_MAX_RETRIES: usize = usize::MAX` (satır 39): Varsayılan sonsuz retry.
- `DEFAULT_TRANSACTION_BATCH_SIZE: usize = 1` (satır 43): Varsayılan tek paket gönder.
- `MAX_TRANSACTION_BATCH_SIZE: usize = 10_000` (satır 46): Maksimum batch boyutu.
- `MAX_TRANSACTION_SENDS_PER_SECOND: u64 = 1_000` (satır 49): Saniye başına max gönderim.
- `DEFAULT_BATCH_SEND_RATE_MS: u64 = 1` (satır 53): 1ms bekleme süresi.
- `MAX_BATCH_SEND_RATE_MS: usize = 100_000` (satır 56): 100 saniye max.
- `MAX_RETRY_SLEEP_MS: u64 = 1000` (satır 138): Retry thread max uyku süresi.
- `LEADER_INFO_REFRESH_RATE_MS: u64 = 1000` (satır 141): Leader bilgisi 1 saniyede bir güncellenir.
- `SEND_TRANSACTION_METRICS_REPORT_RATE_MS: u64 = 5000` (satır 331): Metrik raporlama 5 saniyede bir.

**Struct/Enum/Trait Listesi:**
- `SendTransactionService` (satır 58-62): `receive_txn_thread: JoinHandle<()>`, `retry_thread: JoinHandle<()>`, `exit: Arc<AtomicBool>`.
- `TransactionInfo` (satır 64-73):
  - `signature: Signature`
  - `wire_transaction: Vec<u8>`: Serialized transaction bytes.
  - `last_valid_block_height: u64`: Bu block_height geçilince expire.
  - `durable_nonce_info: Option<(Pubkey, Hash)>`: Nonce hesabı ve durable nonce.
  - `max_retries: Option<usize>`: Per-tx retry sınırı.
  - `retries: usize`: Kaç kez denendi.
  - `last_sent_time: Option<Instant>`: Son gönderim zamanı.
- `ProcessTransactionsResult` (satır 96-104): `rooted`, `expired`, `retried`, `max_retries_elapsed`, `failed`, `retained` sayıları.
- `Config` (satır 106-119): Tüm servis parametreleri bir arada.
- `CurrentLeaderInfo<T: TpuInfo>` (satır 144-157): Son refresh zamanı + leader bilgisi + refresh_rate.
- `SendTransactionServiceStats` (satır 191-234): 13 adet `AtomicU64` metrik.
- `SendTransactionServiceStatsReport` (satır 236-240): Stats + `last_report: AtomicInterval`.

**Kritik Fonksiyonlar:**

- `CurrentLeaderInfo::get_leader_info()` (satır 163-178): `refresh_rate` (1 sn) geçtiyse `refresh_recent_peers()` çağırır.

- `SendTransactionService::new_with_config()` (satır 360-401): 2 thread başlatır:
  1. `receive_txn_thread` — adı `"solStxReceive"`.
  2. `retry_thread` — adı `"solStxRetry"`.

- `receive_txn_thread()` (satır 403-507): **İlk gönderim ve batch yönetimi**:
  1. `receiver.recv_timeout(batch_send_rate_ms)` ile tx bekler.
  2. Duplicate kontrolü: `transactions` ve `retry_transactions` her ikisinde yoksa ekler.
  3. `transactions.len() >= batch_size` VEYA `elapsed >= batch_send_rate_ms` ise `send_transactions_in_batch()`.
  4. Gönderilen tx'ler `retry_transactions` pool'una taşınır. Pool doluysa overflow sayılır.

- `retry_thread()` (satır 509-559): **Retry döngüsü**:
  1. `sleep(min(MAX_RETRY_SLEEP_MS=1000, retry_interval_ms=2000))` → her 1 saniyede çalışır.
  2. `process_transactions()` çağrısı.

- `process_transactions()` (satır 601-734): **Retry kararı mantığı** — `HashMap::retain()` içinde her tx için:
  1. `root_bank.has_signature(signature)` → `rooted` → remove.
  2. Nonce tx: nonce hesabı değişmişse ve signature status yoksa ve `retry_rate` geçmişse → `expired` → remove.
  3. `last_valid_block_height < root_bank.block_height()` → `expired` → remove.
  4. `retries >= max_retries` → `max_retries_elapsed` → remove.
  5. `signature_status == None` ve `retry_rate` geçmişse → `batched_transactions`'a ekle, `retries++`.
  6. `signature_status == Some(_, Err(_))` → `failed` → remove.
  7. `signature_status == Some(_, Ok(_))` → `retained` (beklemeye devam, root'u bekle).

- `send_transactions_in_batch()` (satır 562-598): `tpu_peers` + `leader_tpus_with_slots` birleşimi → her adrese `send_transactions()`.

- `send_transactions()` (satır 755-779): 1 tx için `send_data_async`, birden fazla için `send_data_batch_async`. Hata varsa warn + stat.

- `get_tpu_addresses()` (satır 781-799): Leader bilgisi yoksa fallback olarak `tpu_address` kullanılır.

**Test Coverage:**
- `service_exit`: Drop sender → thread kapanır.
- `validator_exit`: `exit.store(true)` ile graceful shutdown.
- `process_transactions`: Expired, rooted, non-rooted, failed tx senaryoları.

**Nova Shield İçin Öğrenilen:**
- **2 thread mimarisi**: Alım + retry ayrı thread'ler — birbirini bloke etmez.
- **Retry backoff**: Sabit 2 saniye interval (exponential değil) — simplicity over sophistication.
- **Expire mekanizması**: `last_valid_block_height` Solana'ya özgü. Nova Shield'de equivalent: timeout veya slot sayısı.
- **Nonce tx özel işleme**: Durable nonce'lar farklı expire kriterleri gerektirir.
- **Pool overflow**: 10K limit aşılınca yeni tx discard (pool dolunca drop strategy).
- **Leader forward count = 2**: Hem mevcut hem sonraki leader'a gönder — fork ve timing koruma.
- **Retry pool signature-indexed HashMap**: O(1) duplicate ve rooted kontrol.

---

## BÖLÜM 3: streamer/src/ Modülü

### [lib.rs] (15 satır)

**Amaç:** Crate kökü — 5 ana modül ve `nonblocking` alt modülü.

---

### [packet.rs] (191 satır)

**Amaç:** UDP socket'ten paket okuma/yazma — recvmmsg entegrasyonu.

**Kritik Fonksiyonlar:**

- `recv_from()` (satır 20-62): **Çok önemli: amortize batch okuma**:
  ```
  1. socket.set_nonblocking(false) → ilk paketi bloklayarak bekle
  2. recv_mmsg() ile PACKETS_PER_BATCH=64'e kadar paket al
  3. İlk paket alınınca: set_nonblocking(true) → non-blocking moduna geç
  4. Döngü: max_wait süresi dolana veya PACKETS_PER_BATCH dolana kadar devam et
  5. batch.truncate(i) → gerçek boyuta küçült
  ```
  Fikir: İlk paket beklenir (blocking), sonraki paketler non-blocking burst ile çekilir.

- `send_to()` (satır 64-78): Her paketi `socket.send_to()` ile gönderir. `SocketAddrSpace::check()` ile private/loopback filtreleme.

**Nova Shield İçin Öğrenilen:**
- `blocking → nonblocking` geçişi: Tek read loop ile hem bekleme hem burst okuma. syscall sayısını minimize eder.
- `max_wait` parametresi: Batch koalesan süresi — küçük değer düşük latency, büyük değer yüksek throughput.

---

### [recvmmsg.rs] (288 satır)

**Amaç:** `recvmmsg(2)` syscall wrapper — tek syscall ile N paket alımı.

**Önemli Sabitler:**
- `NUM_RCVMMSGS: usize = 64` (perf::packet'ten re-export): Tek syscall başına maksimum paket sayısı.

**Kritik Fonksiyonlar:**

- `recv_mmsg()` — **Linux implementasyonu** (satır 72-125):
  ```rust
  hdrs: [mmsghdr; 64] = zeroed     // mesaj header'ları
  iovs: [iovec; 64] = uninit       // scatter/gather vektörler
  addrs: [sockaddr_storage; 64] = zeroed  // kaynak adres buffer'ları

  for her paket:
      iov = { ptr: packet.buffer, len: PACKET_DATA_SIZE }
      hdr.msg_iov = &iov; hdr.msg_iovlen = 1
      hdr.msg_name = &addr; hdr.msg_namelen = sizeof(sockaddr_storage)

  ts = { tv_sec: 1, tv_nsec: 0 }  // 1 saniyelik timeout
  nrecv = libc::recvmmsg(fd, &mut hdrs[0], 64, MSG_WAITFORONE, &mut ts)

  for her alınan paket:
      pkt.meta.size = hdr.msg_len
      pkt.meta.addr = cast_socket_addr(&addr, &hdr)
  ```
  `MSG_WAITFORONE`: En az 1 paket gelene kadar bekle, sonra mevcut olanları al.

- `recv_mmsg()` — **Linux olmayan implementasyon** (satır 19-43): `socket.recv_from()` döngüsü, en fazla `NUM_RCVMMSGS` kadar, birincisi blocking diğerleri break-on-error.

- `cast_socket_addr()` (satır 47-68): `sockaddr_storage` → `InetAddr` (V4/V6). Bilinmeyen address family'de error log.

**Unsafe Bloklar:**
- `iovs.assume_init()` (satır 79): `MaybeUninit<[iovec; 64]>::uninit().assume_init()` — iov_base pointer'ları hemen sonra set edilir, güvenli.
- `cast_socket_addr()` (satır 54, 60): `sockaddr_storage` → `sockaddr_in` / `sockaddr_in6` raw cast — ss_family ve msg_namelen kontrol edildikten sonra.
- `libc::recvmmsg(...)` (satır 104-111): Raw syscall.

**Test Coverage:**
- `test_recv_mmsg_one_iter`: 31 paket gönder, hepsini al.
- `test_recv_mmsg_multi_iter`: 42 paket gönder, 32+10 ile iki turda al.
- `test_recv_mmsg_multi_iter_timeout`: Non-blocking modda ikinci çağrı hemen dönmeli (<5 sn).
- `test_recv_mmsg_multi_addrs`: 2 farklı sender'dan paketler, doğru addr eşleşmesi.

**Nova Shield İçin Öğrenilen:**
- `recvmmsg(MSG_WAITFORONE)`: 1 syscall = 64 paket — sistem çağrısı overhead'i 64x azaltılır.
- Stack-allocated `[mmsghdr; 64]`: Heap allocation yok, cache-friendly.
- `assume_init()` pattern: iovec'lerin başlangıç değerleri önemli değil, hemen üzerine yazılır.

---

### [sendmmsg.rs] (378 satır)

**Amaç:** `sendmmsg(2)` syscall wrapper — tek syscall ile N paket gönderimi.

**Struct/Enum:**
- `SendPktsError` (satır 23-28): `IoError(io::Error, usize)` — ilk hata + başarısız paket sayısı.

**Kritik Fonksiyonlar:**

- `batch_send()` — **Linux implementasyonu** (satır 130-145):
  ```rust
  iovs = MaybeUninit vec, addrs = zeroed vec, hdrs = zeroed vec
  for her paket: mmsghdr_for_packet(pkt, dest, iov, addr, hdr)
  sendmmsg_retry(sock, &mut hdrs)
  ```

- `mmsghdr_for_packet()` (satır 61-94): Her paketi `iovec` + `sockaddr` ile `mmsghdr`'a dönüştürür. V4/V6 uyumlu.

- `sendmmsg_retry()` (satır 97-127): **Başarısız paketlerde retry**:
  ```rust
  while !pkts.is_empty():
      n = libc::sendmmsg(fd, &mut pkts[0], len, 0)
      if n == -1: ilk hatayı kaydet, 1 paket atla
      else: total_sent += n, ilerle
  ```
  Her hata durumunda tek paketi atlayarak devam eder — kısmi başarıyı raporlar.

- `multi_target_send()` (satır 147-159): Tek paketi N adrese göndermek için `repeat(&packet).zip(dests)`.

- `batch_send()` — **Linux olmayan** (satır 37-58): `socket.send_to()` döngüsü, hataları toplar.

**Unsafe Bloklar:**
- `iovec.assume_init()` (satır 138): iov_base hemen sonra set edilir.
- `addrs` zeroed init (satır 139).
- `hdrs` zeroed init (satır 140).
- `mmsghdr_for_packet()` içinde `std::ptr::write(addr as *mut _ as *mut _, dest)` (satır 83, 89): sockaddr yazımı için raw cast + ptr write.
- `libc::sendmmsg(...)` (satır 104): Raw syscall.

**Test Coverage:**
- `test_send_mmsg_one_dest`: 32 paket, tek dest.
- `test_send_mmsg_multi_dest`: 32 paket, 2 hedefe 16'şar.
- `test_multicast_msg`: 1 paket, 4 hedefe aynı anda.
- `test_intermediate_failures_mismatched_bind`: IPv4 socket → IPv6 adres → 1 başarısız.
- `test_intermediate_failures_unreachable_address`: broadcast adresleri → başarısız sayı kontrolü.

**Nova Shield İçin Öğrenilen:**
- `sendmmsg_retry()`: Kısmi başarıda retry + hata sayımı — UDP'de per-packet hata toleransı.
- `multi_target_send()`: Gossip/broadcast için pratik — tek paket, N hedef.

---

### [socket.rs] (39 satır)

**Amaç:** IP adresi doğrulama — private/loopback adresleri üretim ortamında filtreler.

**Struct/Enum:**
- `SocketAddrSpace` (satır 4-7): `Unspecified` (hepsi geçerli) veya `Global` (sadece public IP).

**Kritik Fonksiyonlar:**
- `SocketAddrSpace::check()` (satır 20-38):
  - `Unspecified` → true (her zaman geçer).
  - `Global`:
    - V4: `is_private() || is_loopback()` → false.
    - V6: `is_loopback()` → false.
    - Not: `is_global()` henüz stable değil, TODO var.

**Nova Shield İçin Öğrenilen:**
- Üretimde private IP filtreleme zorunlu — gossip/turbine sadece public IP'lere yayın yapmalı.
- `allow_private_addr` flag'i: Testnet/devnet için private IP'lere izin ver.

---

### [streamer.rs] (529 satır)

**Amaç:** UDP paket stream — `receiver()` ve `responder()` thread'leri ile gelen/giden paketleri yönetir.

**Struct/Enum/Trait Listesi:**
- `StakedNodes` (satır 30-36):
  - `stakes: Arc<HashMap<Pubkey, u64>>`: Ana stake tablosu.
  - `overrides: HashMap<Pubkey, u64>`: Stake geçersizleştirme tablosu.
  - `total_stake`, `max_stake`, `min_stake: u64`.
- `StreamerReceiveStats` (satır 56-62): `packets_count`, `packet_batches_count`, `full_packet_batches_count`, `max_channel_len` — hepsi `AtomicUsize`.
- `StreamerError` enum (satır 42-53): `Io`, `RecvTimeout`, `Send`, `SendPktsError`.
- `SendStats` (satır 190-193): `bytes: u64`, `count: u64`.
- `StreamerSendStats` (satır 196-199): `host_map: HashMap<IpAddr, SendStats>`, `since: Option<Instant>`.

**Kritik Fonksiyonlar:**

- `recv_loop()` (satır 104-157): **Ana alım döngüsü**:
  1. `use_pinned_memory` flag'ine göre `new_with_recycler` veya `with_capacity`.
  2. Exit kontrolü her iterasyonda.
  3. `in_vote_only_mode` aktifse 1ms sleep.
  4. `packet::recv_from()` → len > 0 ise channel'a gönder.
  5. `len == PACKETS_PER_BATCH=64` ise `full_packet_batches_count++`.

- `receiver()` (satır 159-187): Thread builder, `recv_loop()` çalıştırır. 1 saniyelik read timeout.

- `StreamerSendStats::maybe_submit()` (satır 270-289): 10 sn veya 1000 host eşiğine gelince stats raporla.
- `StreamerSendStats::report_stats()` (satır 201-268): Histogram + top-5 host raporlaması.

- `recv_send()` (satır 341-359): Alım kanalından batch al, `SocketAddrSpace` filtresinden geçir, `batch_send()` ile gönder.

- `recv_packet_batches()` (satır 361-382): 1 sn timeout ile ilk batch bekle, sonra `try_recv()` ile tüm mevcut batch'leri topla (burst).

- `responder()` (satır 384-429): Thread builder, `recv_send()` döngüsü. Hata varsa 1 sn'de bir log.

- `StakedNodes::new()` (satır 298-315): `overrides`'daki node'lar stake tablosundan gizlenir. `minmax()` ile en yüksek/düşük stake hesabı.
- `StakedNodes::get_node_stake()` (satır 317-323): Override önce, sonra stake tablosu. 0 stake → None.

**Test Coverage:**
- `streamer_send_test`: Receiver + responder, 5 paket gönder, hepsini al.

**Nova Shield İçin Öğrenilen:**
- **Pinned memory koşullu**: Sadece GPU doğrulama pipeline'larında pin, diğerlerinde normal.
- **full_batch detection**: `len == 64` full batch → yoğun dönem tespiti için metric.
- **`recv_packet_batches()` burst drain**: İlk batch blocking bekle, sonra try_recv ile mevcut olanları topla — latency-throughput dengesi.
- **StakedNodes override**: Testnet/devnet'te belirli node'ların stake'ini değiştirmek için — güvenlik bypass testi.

---

### [tls_certificates.rs] (121 satır)

**Amaç:** Ed25519 Keypair'den sahte X.509 sertifika üretimi — QUIC/TLS için.

**Kritik Fonksiyonlar:**

- `new_dummy_x509_certificate(keypair: &Keypair)` (satır 6-97):
  **PKCS#8 v1 özel anahtar formatı** (RFC 8410):
  ```
  PKCS8_PREFIX = [0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20]
  key_pkcs8_der = PREFIX + keypair.secret().as_bytes()
  ```
  ASN.1 yapısı: `PrivateKeyInfo SEQUENCE` → `version INTEGER 0` → `AlgorithmIdentifier (Ed25519 OID 1.3.101.112)` → `PrivateKey OCTET STRING`.

  **Dummy X.509 sertifika**:
  - İmzası kasıtlı geçersiz (0xFF byte'ları).
  - `SubjectPublicKeyInfo` alanı: `keypair.pubkey().to_bytes()`.
  - İssuer: "Solana node".
  - Validity: 1970-01-01 → 4096-01-01 (pratik olarak sonsuz).
  - Extensions: `subjectAltName = localhost` (critical), `basicConstraints = CA:false` (critical).

  TLS 1.3 `CertificateVerify` mesajında gerçek kimlik doğrulaması yapılır; sertifika imzası önemsiz.

- `get_pubkey_from_tls_certificate(der_cert)` (satır 99-105): DER X.509'dan `PublicKey::Unknown(bytes)` → `Pubkey::try_from(bytes)`. Ed25519 public key'in `Unknown` olarak gelmesi x509_parser'ın Ed25519'u tanımamasından kaynaklanır.

**Test Coverage:**
- `test_generate_tls_certificate`: Üretilen sertifikadan pubkey'i geri al ve Keypair.pubkey() ile karşılaştır.

**Nova Shield İçin Öğrenilen:**
- **Identity-based TLS**: X.509 sertifikası kimlik bilgisi olarak kullanılır, CA altyapısı gerekmez.
- **Dummy sertifika imzası**: TLS 1.3 mutual auth için sertifika imzası kritik değil, keypair'in TLS handshake'deki CertificateVerify imzası kritik.
- **OID 1.3.101.112**: Ed25519 algoritma tanımlayıcısı — PKCS#8 encoding için ezber gerektiren hardcoded prefix.
- **SubjectPublicKeyInfo**: QUIC peer kimliği bu alandan çıkarılır → `get_pubkey_from_tls_certificate()`.

---

### [quic.rs] (634 satır) — Senkron QUIC Wrapper

**Amaç:** Async QUIC sunucusunu senkron bir thread içinde çalıştırır.

**Struct/Enum/Trait Listesi:**
- `SkipClientVerification` (satır 31-58): `rustls::server::ClientCertVerifier` implementasyonu — istemci sertifikasını doğrulamadan kabul eder (kimlik `get_remote_pubkey()` ile ayrı alınır).
- `SpawnServerResult` (satır 39-43): `endpoint: Endpoint`, `thread: JoinHandle<()>`, `key_updater: Arc<EndpointKeyUpdater>`.
- `EndpointKeyUpdater` (satır 119-129): `NotifyKeyUpdate` trait — Keypair değişince `configure_server()` + `endpoint.set_server_config()`.
- `StreamStats` (satır 131-178): 40+ `AtomicUsize` metrik — connection, stream, chunk, packet istatistikleri.
- `QuicServerError` (satır 111-117): `EndpointFailed(io::Error)`, `TlsError(rustls::Error)`.

**Önemli Sabitler:**
- `MAX_STAKED_CONNECTIONS: usize = 2000` (satır 28): Stake'li bağlantı limiti.
- `MAX_UNSTAKED_CONNECTIONS: usize = 500` (satır 29): Stake'siz bağlantı limiti.

**Kritik Fonksiyonlar:**

- `configure_server()` (satır 62-101): QUIC sunucu konfigürasyonu:
  ```
  cert + priv_key = new_dummy_x509_certificate(keypair)
  rustls::ServerConfig:
    - ALPN: "solana-tpu"
    - ClientCertVerifier: SkipClientVerification (her istemciyi kabul)
    - use_retry: true (connection amplification koruması)
  quinn transport config:
    - max_concurrent_uni_streams: QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS * 2
    - stream_receive_window: PACKET_DATA_SIZE (1232 byte)
    - receive_window: PACKET_DATA_SIZE * MAX_CONCURRENT_UNI_STREAMS
    - max_idle_timeout: QUIC_MAX_TIMEOUT
    - max_concurrent_bidi_streams: 0 (sadece uni!)
    - datagram_receive_buffer_size: None (datagram devre dışı)
  ```

- `rt(name: String)` (satır 103-109): Multi-thread tokio runtime.

- `spawn_server()` (satır 432-480): `rt()` içinde `nonblocking::quic::spawn_server()` çağrısı, `runtime.block_on(task)` ile thread içinde çalıştırır.

**Test Coverage:**
- `test_quic_server_exit`, `test_quic_timeout`, `test_quic_server_block_multiple_connections`, `test_quic_server_multiple_streams`, `test_quic_server_multiple_writes`, `test_quic_server_unstaked_node_connect_failure`.

**Nova Shield İçin Öğrenilen:**
- **ALPN "solana-tpu"**: Protokol tanımlayıcısı — Nova Shield için ayrı ALPN.
- **use_retry: true**: QUIC 0-RTT amplification saldırısına karşı.
- **Sadece uni-directional streams**: Transaction gönderimi tek yönlüdür — bidi ve datagram kapalı.
- **`stream_receive_window = PACKET_DATA_SIZE = 1232`**: Her stream tek paket kapasitesinde.
- **Staked/Unstaked ayrımı**: 2000 + 500 = toplam 2500 bağlantı limiti.
- **Keypair rotation**: `EndpointKeyUpdater` + `NotifyKeyUpdate` ile hot key rotation desteği.

---

### [nonblocking/quic.rs] (500+ satır) — Async QUIC Çekirdeği

**Amaç:** Tokio async QUIC sunucu mantığı — bağlantı kabul, stream işleme, stake-based throttling.

**Önemli Sabitler:**
- `WAIT_FOR_STREAM_TIMEOUT: Duration = 100ms` (satır 58).
- `DEFAULT_WAIT_FOR_CHUNK_TIMEOUT: Duration = 10s` (satır 59).
- `ALPN_TPU_PROTOCOL_ID: &[u8] = b"solana-tpu"` (satır 61).
- Connection close kodları: `DROPPED_ENTRY=1`, `DISALLOWED=2`, `EXCEED_MAX_STREAM_COUNT=3`, `TOO_MANY=4`.

**Struct/Enum:**
- `PacketChunk` (satır 77-87): `bytes: Bytes`, `offset: usize`, `end_of_chunk: usize` — QUIC stream parçası.
- `PacketAccumulator` (satır 94-97): `meta: Meta`, `chunks: Vec<PacketChunk>` — paket biriktirici (zero-copy).
- `ConnectionPeerType` (satır 99-103): `Unstaked` / `Staked(u64)`.
- `NewConnectionHandlerParams` (satır 289-303): Yeni bağlantı parametreleri.
- `ConnectionHandlerError` (satır 283-286): `ConnectionAddError`, `MaxStreamError`.
- `ConnectionTable` (görülmedi tam): IP+pubkey → bağlantı eşlemesi.

**Kritik Fonksiyonlar:**

- `spawn_server()` (satır 112-150): `Endpoint::new()`, `tokio::spawn(run_server(...))`.

- `run_server()` (satır 153-212): Ana kabul döngüsü:
  1. `timeout(1s, incoming.accept())` ile bağlantı bekle.
  2. Her 5 sn'de `stats.report()`.
  3. `tokio::spawn(setup_connection(...))` ile her bağlantı async işlenir.

- `prune_unstaked_connection_table()` (satır 214-227): `total_size >= max_unstaked_connections` ise en eskilerini prune (90% hedef).

- `get_remote_pubkey()` (satır 229-238): TLS peer sertifikasından pubkey çıkarır. Sadece self-signed, uzunluk=1 sertifika kabul edilir.

- `get_connection_stake()` (satır 240-254): `get_remote_pubkey()` → `staked_nodes.get_node_stake(pubkey)` → `(pubkey, stake, total_stake, max_stake, min_stake)`.

- `compute_max_allowed_uni_streams()` (satır 256-281): **Stake-proportional stream allocation**:
  ```
  Staked: linear_map(peer_stake/total_stake, MIN=8, MAX=2048)
         = MIN + (MAX-MIN) * peer_stake / total_stake
         clamp(MIN_STAKED, MAX_STAKED)
  Unstaked: QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS (sabit, düşük)
  ```

- `compute_receive_window_ratio_for_staked_node()` (satır 430-453): Linear interpolation stake → ratio `[QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO, QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO]`. Optimum throughput `PACKET_DATA_SIZE * 10` deneysel.

- `compute_recieve_window()` (satır 455-470):
  - Unstaked: `PACKET_DATA_SIZE * QUIC_UNSTAKED_RECEIVE_WINDOW_RATIO`
  - Staked: `PACKET_DATA_SIZE * ratio(stake)`

- `handle_and_cache_new_connection()` (satır 324-397): Stream sayısı + receive window set et, `ConnectionTable::try_add_connection()`, `tokio::spawn(handle_connection(...))`.

- `prune_unstaked_connections_and_add_new_connection()` (satır 399-427): Unstaked tablo doluysa prune'la, sonra ekle.

- `setup_connection()` (satır 473-499+): `QUIC_CONNECTION_HANDSHAKE_TIMEOUT` içinde handshake tamamlanmalı. Staked/Unstaked'e göre farklı handler.

**Nova Shield İçin Öğrenilen:**
- **Stake-proportional stream limiti**: Daha fazla stake = daha fazla concurrent stream = daha yüksek TPS kotası.
- **Receive window linear mapping**: Stake büyüdükçe daha büyük buffer → throughput artar.
- **Unstaked pruning (90% target)**: Unstaked bağlantılar saldırıya karşı agresif prune edilir.
- **Handshake timeout = QUIC_CONNECTION_HANDSHAKE_TIMEOUT**: Slow loris tipi saldırılara karşı.
- **`PacketAccumulator` zero-copy**: QUIC stream chunk'ları byte slice referanslarıyla birikim — copy yok.
- **QUIC + TLS identity**: Stake bilgisi TLS cert'ten pubkey → gossip stake tablosundan alınır — CA'sız PKI.

---

### [nonblocking/stream_throttle.rs] (507 satır)

**Amaç:** QUIC stream hız sınırlaması — EMA tabanlı yük takibi, staked/unstaked ayrı politikalar.

**Önemli Sabitler:**
- `MAX_STREAMS_PER_MS: u64 = 250` — 250K paket/sn hedef (satır 18).
- `MAX_UNSTAKED_STREAMS_PERCENT: u64 = 20` — kapasitenin %20'si unstaked için (satır 19).
- `STREAM_THROTTLING_INTERVAL_MS: u64 = 100` — 100ms throttling penceresi (satır 20).
- `STREAM_STOP_CODE_THROTTLING: u32 = 15` — throttle sebebiyle kapatılan stream kodu (satır 21).
- `STREAM_LOAD_EMA_INTERVAL_MS: u64 = 5` — EMA 5ms'de bir güncellenir (satır 22).
- `STREAM_LOAD_EMA_INTERVAL_COUNT: u64 = 10` — 10 interval = 50ms EMA penceresi (satır 23).
- `EMA_WINDOW_MS: u64 = 50` (satır 24).

**Struct/Enum/Trait Listesi:**
- `StakedStreamLoadEMA` (satır 26-38):
  - `current_load_ema: AtomicU64`: Mevcut EMA değeri.
  - `load_in_recent_interval: AtomicU64`: Son interval'daki stream sayısı.
  - `last_update: RwLock<Instant>`.
  - `max_staked_load_in_ema_window: u64`: EMA penceresi boyunca staked bağlantılar için max stream.
  - `max_unstaked_load_in_throttling_window: u64`: 100ms'de unstaked için max stream.
- `ConnectionStreamCounter` (satır 193-197): `stream_count: AtomicU64`, `last_throttling_instant: RwLock<tokio::time::Instant>`.

**Kritik Fonksiyonlar:**

- `StakedStreamLoadEMA::new()` (satır 41-71):
  - `max_staked_load_in_ema_window = (250 - 20% of 250) * 50ms = 200 * 50 = 10_000`
  - `max_unstaked_load_in_throttling_window = 20% * (250 * 100ms) / 500 connections = 10`
  - Her unstaked bağlantı 100ms'de 10 stream hakkına sahip.

- `ema_function()` (satır 73-87): Integer EMA:
  ```
  multiplied_smoothing = 2 * 1024 / (10 + 1) = 186
  updated_ema = (recent_load * 186 + current_ema * (1024 - 186)) / 1024
  ```
  Floating point olmadan, 1024x büyütme faktörü ile.

- `update_ema()` (satır 89-123): `time_since_last_update` içindeki kaçırılan interval sayısını hesaplar ve her biri için EMA günceller.

- `available_load_capacity_in_throttling_duration()` (satır 146-190): **Throttle kararı**:
  ```
  Unstaked: max_unstaked_load_in_throttling_window (sabit, düşük = 10)
  Staked:
    current_load = max(ema, max_staked_load / 4)  // minimum yük = max/4 varsay
    capacity = (max^2 / current_load) * (stake / total_stake)
              * (throttling_window / ema_window)
    = max(capacity, unstaked_limit + 1)  // en az unstaked'den 1 fazla
  ```
  Bu formül: Yük düşükse kapasite artar (max^2/current_load büyür), stake büyükse kapasite büyür.

- `ConnectionStreamCounter::reset_throttling_params_if_needed()` (satır 207-221): 100ms geçmişse `stream_count = 0`. Double-checked locking with RwLock.

**Test Coverage:**
- `test_max_streams_for_unstaked_connection`: 250K/s * 20% / 500 = 10 stream.
- `test_max_streams_for_staked_connection`: Çeşitli stake/ema kombinasyonları, formül doğrulaması.
- `test_update_ema`, `test_update_ema_missing_interval`, `test_update_ema_if_needed`.

**Nova Shield İçin Öğrenilen:**
- **Stake-proportional throttling**: `capacity = max^2 / load * stake_weight` — hem yüke hem stake'e duyarlı.
- **EMA yük tahmini**: Anlık spike'lara aşırı tepki vermez, smooth bandwidth allocation.
- **Unstaked sabit limit (10 stream/100ms)**: DoS koruması — stake'siz node'lar flood yapamaz.
- **Integer EMA (1024 multiplier)**: Floating point maliyet olmadan smooth EMA.
- **Double-checked locking**: RwLock read → expire kontrol → write → recheck — klasik pattern.

---

### [nonblocking/recvmmsg.rs] (214 satır)

**Amaç:** Tokio async UDP recvmmsg wrapper.

**Kritik Fonksiyonlar:**
- `recv_mmsg()` (satır 14-38): `socket.readable().await` + `try_recv_from()` döngüsü. `WouldBlock` → break. `NUM_RCVMMSGS=64` limit.
- `recv_mmsg_exact()` (satır 42-54): `packets.len()` kadar paket dolana kadar `recv_mmsg()` tekrarlar.

**Nova Shield İçin Öğrenilen:**
- `readable().await`: Tokio async I/O notifikasyonu — kernel'dan event gelince okuma dene.
- `try_recv_from()` + `WouldBlock`: Non-blocking okuma, hazır olmayan soketi bekletmez.

---

### [nonblocking/sendmmsg.rs] (275 satır)

**Amaç:** Tokio async UDP batch gönderim.

**Kritik Fonksiyonlar:**
- `batch_send()` (satır 10-36): `join_all(futures)` ile tüm `sock.send_to()` future'ları paralel başlatılır. Tüm sonuçlar beklenir, hatalar sayılır.
- `multi_target_send()` (satır 38-50): `batch_send()` delegate.

**Nova Shield İçin Öğrenilen:**
- `join_all()`: Tüm send future'ları concurrently başlatılır — sendmmsg'nin async karşılığı.
- Not: Linux'ta kernel-level batch yok (sendmmsg syscall yok), ama async concurrency ile benzer efekt.

---

## GENEL ÖZET VE KRİTİK BULGULAR

### Sayısal Sabitler Özeti

| Sabit | Değer | Bağlam |
|-------|-------|--------|
| `VERIFY_PACKET_CHUNK_SIZE` | 128 | CPU sigverify chunk boyutu |
| `PACKETS_PER_BATCH` | 64 | Tek batch paket sayısı |
| `NUM_RCVMMSGS` | 64 | recvmmsg başına max paket |
| `RECYCLER_SHRINK_SIZE` | 1024 | Pool shrink eşiği |
| `RECYCLER_SHRINK_WINDOW` | 16384 | EMA penceresi (allocation sayısı) |
| `MAX_STAKED_CONNECTIONS` | 2000 | QUIC staked bağlantı limiti |
| `MAX_UNSTAKED_CONNECTIONS` | 500 | QUIC unstaked bağlantı limiti |
| `MAX_TRANSACTION_RETRY_POOL_SIZE` | 10_000 | Retry kuyruğu max |
| `DEFAULT_RETRY_RATE_MS` | 2_000 | 2 saniyede bir retry |
| `DEFAULT_LEADER_FORWARD_COUNT` | 2 | Kaç leader'a forward |
| `MAX_RETRY_SLEEP_MS` | 1_000 | Retry thread uyku max |
| `MAX_STREAMS_PER_MS` | 250 | 250K PPS hedef |
| `MAX_UNSTAKED_STREAMS_PERCENT` | 20 | Unstaked pay %20 |
| `STREAM_THROTTLING_INTERVAL_MS` | 100 | Throttle penceresi |
| `EMA_WINDOW_MS` | 50 | Stream load EMA penceresi |
| `DEFAULT_WAIT_FOR_CHUNK_TIMEOUT` | 10s | QUIC stream timeout |

### GPU vs CPU Sigverify Karar Matrisi

```
valid_percentage >= 90% AND valid_packet_count >= 64 → GPU
else → CPU (rayon parallel, VERIFY_PACKET_CHUNK_SIZE=128)
```

GPU path: PinnedVec (page-locked) → FFI `ed25519_verify_many()` → USE_NON_DEFAULT_STREAM=1
CPU path: `par_chunks_mut(128)` → per-packet `verify_packet()`

### Nova Shield İçin Eylem Planı

1. **Paket Pipeline Mimarisi**: `recvmmsg(64) → dedup(Bloom K=2) → sigverify(CPU/GPU) → banking` şeklinde pipeline kur. Her stage channel ile ayrılsın.

2. **Bellek Yönetimi**: `PinnedVec` + `Recycler` kombinasyonu GPU doğrulama için şart. CPU-only path için normal Vec yeterli.

3. **QUIC Transport**: `quinn` + `rustls` + self-signed Ed25519 cert pattern'i aynen kullan. ALPN = `"nova-shield-tpu"`. `use_retry=true` zorunlu.

4. **Stake-based QoS**: `compute_max_allowed_uni_streams()` formülü: `stake_weight * (MAX-MIN) + MIN` — Nova Shield'e direkt kopyalanabilir. Unstaked limit = 500 bağlantı, 10 stream/100ms.

5. **Transaction Retry**: 2 thread (receive + retry), 2 saniye interval, 10K pool, `last_valid_block_height` expire. Nonce tx ayrı handling.

6. **Rate Limiting**: `DataBudget` (lock-free token bucket) + `StakedStreamLoadEMA` (stake-proportional EMA throttle) — iki katmanlı.

7. **Thread Önceliği**: Sigverify thread'lerine `renice_this_thread(-5)` (CAP_SYS_NICE gerektirir). Validator başlangıcında feature check.

8. **Monitoring**: `StreamStats` (40+ metrik), `SendTransactionServiceStats` (13 metrik), `StreamerReceiveStats` (4 metrik) — datapoint_info! macro ile influx/prometheus entegrasyonu.

9. **Bloom Filter Dedup**: `Deduper<K=2, [u8]>`, `num_bits = 63_999_979`, false_positive_rate=0.001, reset_cycle belirlenmeli (Solana'da tipik 2-3 sn). `AtomicU64::fetch_or` ile concurrent güvenli.

10. **TLS Sertifika**: `new_dummy_x509_certificate(keypair)` → PKCS#8 v1 prefix + 32 byte secret. TLS 1.3 ile handshake imzası sertifika imzasından bağımsız.
