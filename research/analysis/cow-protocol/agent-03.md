# CoW Protocol Agent-03 Analizi
## Kapsam: number/, serde-ext/, ethrpc/, observe/, gas-price-estimation/ crates

**Toplam analiz edilen kaynak:** ~1,800 satır
**Tarih:** 2026-07-22

---

## 1. `crates/number/` — Sayısal Tip Uzantıları

### 1.1 `src/lib.rs`
Tüm modülleri re-export eder: `conversions`, `nonzero`, `ratio_ext`, `serialization`, `testing`, `u256_ext`, `units`.

### 1.2 `src/u256_ext.rs` (97 satır)

**`U256Ext` trait:**

```rust
pub trait U256Ext: Sized {
    fn checked_ceil_div(&self, other: &Self) -> Option<Self>;
    fn ceil_div(&self, other: &Self) -> Self;
    fn checked_mul_f64(&self, factor: f64) -> Option<Self>;
    fn to_big_rational(&self) -> BigRational;
    fn from_big_int(input: &BigInt) -> Option<Self>;
    fn from_big_rational(value: &BigRational) -> Option<Self>;
}
```

**`checked_ceil_div` implementasyonu:**
- `(!other.is_zero()).then(|| self.div_ceil(*other))` — alloy'nun yerleşik `div_ceil` kullanır
- Tavan bölmesi: `(self + other - 1) / other`

**`checked_mul_f64` — Kritik DEX Fonksiyonu:**
- Factor negatif, NaN, infinity → `None`
- Factor == 1.0 → identity (büyük U256 taşmasını önler)
- Algoritma: `scaled_factor = factor * 10^18` → U256'ya dönüştür → `(self * scaled_factor) / 10^18`
- u128 sınırı aşılırsa BigUint::from_f64 kullanılır
- Önemli kural: Büyük config değerleri için decimal-intent korunur (BigRational'ın binary-f64 semantiğinden kaçınılır)

**Fee hesabı örneği:**
```
25 ETH * 0.2 = 5 ETH (tam sonuç)
U256::from(25e18).checked_mul_f64(0.2) == Some(U256::from(5e18))
```

**`from_big_rational`:**
- Sıfır denominator → None
- Trunation (floor): `numer / denom`

### 1.3 `src/conversions.rs` (270 satır)

Tüm sayısal tip dönüşüm fonksiyonları:

| Fonksiyon | Açıklama |
|-----------|----------|
| `big_decimal_to_big_uint` | BigDecimal → BigUint (tam sayı değilse None) |
| `rational_to_big_decimal` | Ratio<T> → BigDecimal |
| `big_decimal_to_big_rational` | BigDecimal → BigRational (scale'i handle eder) |
| `big_uint_to_u256` | BigUint → U256 (32 byte sınır) |
| `big_int_to_u256` | BigInt → U256 (negatif → err) |
| `big_decimal_to_u256` | BigDecimal → U256 (tam sayı değilse None) |
| `big_rational_to_u256` | BigRational → U256 (trunation) |
| `u256_to_big_uint` | U256 → BigUint (BE bytes) |
| `u256_to_big_int` | U256 → BigInt (Sign::Plus) |
| `u256_to_big_rational` | U256 → BigRational (denom=1) |
| `u256_to_big_decimal` | U256 → BigDecimal |
| `u160_to_big_decimal` | U160 → BigDecimal (20 byte) |
| `big_decimal_to_u160` | BigDecimal → U160 |
| `big_uint_to_u160` | BigUint → U160 (20 byte sınır) |
| `i512_to_big_int` | I512 → BigInt (sign aware) |
| `i512_to_big_rational` | I512 → BigRational |
| `i512_to_u256` | I512 → U256 (BE slice[32..]) |

**`big_decimal_to_big_rational` özel mantığı:**
```rust
match scale.cmp(&0) {
    Equal   => (numer, BigInt::from(1)),
    Greater => (numer, BigInt::from(10).pow(scale as u32)),
    Less    => (numer * BigInt::from(10).pow((-scale) as u32), BigInt::from(1)),
}
```
Negatif scale = sayının 10^|scale| ile çarpımı (büyük sayı tarafı).

**`i512_to_u256`:** `input.to_be_bytes::<64>()[32..]` — son 32 byte alır.

### 1.4 `src/nonzero.rs` (76 satır)

```rust
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct NonZeroU256(U256);

impl NonZeroU256 {
    pub const MAX: Self = NonZeroU256(U256::MAX);
    pub const ONE: Self = NonZeroU256(U256::ONE);
    pub fn new(value: U256) -> Option<Self>;
    pub fn get(self) -> U256;
}
```

- `TryFrom<U256>`: anyhow error "Value cannot be zero!"
- `Default`: ONE (sıfır değil!)
- `Serialize`: decimal string
- `Deserialize`: U256::deserialize → NonZeroU256::try_from
- DEX'te bölünen fiyatlar, fee denominatörleri vs. için güvenli wrapper

### 1.5 `src/ratio_ext.rs` (38 satır)

```rust
pub trait RatioExt<N> {
    const ZERO: Ratio<N>;
    const ONE: Ratio<N>;
    fn scalar_mul(&self, scalar: N) -> Option<N>;
    fn full_scalar_mul(&self, scalar: N) -> Option<N>;
}
```

**`scalar_mul`:** `scalar * numer / denom` — checked arithmetic, intermediate overflow → None
**`full_scalar_mul`:** `U512` genişletme — intermediate overflow izin veriliyor, sonuç U256'ya sığmak zorunda:
```rust
U256::uint_try_from(
    scalar.widening_mul(*self.numer())
          .checked_div(U512::from(*self.denom()))?,
)
```

**DEX kritik:** AMM fiyat hesabı `x * y = k` dönüşümlerde trunation/overflow koruması.

### 1.6 `src/serialization.rs` (29 satır)

```rust
pub struct HexOrDecimalU256;
```
- DeserializeAs: `U256::deserialize` (alloy deserializer — hex veya decimal kabul eder)
- SerializeAs: `source.to_string()` — decimal format (alloy varsayılan hex'in tersine)

API'de U256 → string decimal dönerken hex almak yerine decimal yazmak için.

### 1.7 `src/units.rs` (43 satır)

```rust
pub trait EthUnit: Sized {
    fn atom(self) -> U256;       // wei (base unit)
    fn matom(self) -> U256;      // * 10^6 (Mwei)
    fn gatom(self) -> U256;      // * 10^9 (Gwei)
    fn eth(self) -> U256;        // * 10^18 (Ether)
}
```

u64, u128, f64 için implement edilmiş. f64::eth() = `U256::from((self * 1e18) as u128)`.

### 1.8 `src/testing.rs` (198 satır)

```rust
pub trait ApproxEq {
    fn is_approx_eq(&self, other: &Self, delta: Option<f64>) -> bool;
}
```

- Relative error: `|self - other| / |other|`
- Default delta: `1e-9` (0.0000001%)
- BigRational ile exact rational aritmetik — floating point hatası yok
- u64, u128, U256 için impl (BigInt'e dönüşen her T)
- Sıfır kıyaslaması: early equality check (sıfır/sıfır div hatasını önler)

**DEX Uygulama Notu:**
- AMM hesaplamalarında rounding error testleri için ApproxEq kullan
- `checked_mul_f64` pattern'i: fee_factor * amount için 10^18 scaling zorunlu
- `full_scalar_mul` (U512 genişletme) vs `scalar_mul` trade-off: DEX price ratio hesabında full_scalar_mul tercih edilmeli

---

## 2. `crates/serde-ext/` — Serializasyon Yardımcıları

### 2.1 `src/lib.rs`
`pub use {self::hex::Hex, u256::U256};`

### 2.2 `src/hex.rs` (94 satır)

```rust
pub struct Hex;
```

- `DeserializeAs<Vec<u8>>`: "0x" prefix zorunlu, `const_hex::decode` (SIMD-hızlı)
- `SerializeAs<Vec<u8>>`: `const_hex::encode_prefixed`
- `DeserializeAs<[u8; N]>`: const generic — exact N byte check
- `SerializeAs<[u8; N]>`: fixed array hex
- Hata mesajları: "missing 0x prefix", "expected N bytes, got M"

### 2.3 `src/u256.rs` (implicit)
`HexOrDecimalU256` — `number` crate'in serialization.rs ile aynı mantık.

**DEX Uygulama Notu:**
- Nova Shield'da API katmanında `Hex` serde helper ile `Vec<u8>` / `[u8; N]` alanlar için standart hex serializasyonu sağla
- serde_with ile birleşerek zero-cost wrapper pattern

---

## 3. `crates/ethrpc/` — Ethereum RPC İstemcisi

### 3.1 `src/lib.rs` (102 satır)

```rust
pub const MAX_BATCH_SIZE: usize = 100;
pub type AlloyProvider = DynProvider;

pub struct Web3 {
    pub provider: AlloyProvider,
    pub wallet: MutWallet,
}

pub struct Config {
    pub ethrpc_max_batch_size: usize,          // default: 20
    pub ethrpc_max_concurrent_requests: usize, // default: 10
    pub ethrpc_batch_delay: Duration,          // default: ZERO
}
```

- `web3(args, url, label)`: batch_size=0|1 AND concurrent=0 → unbuffered_provider, else provider
- `Web3::new_from_url`: unbuffered + MutWallet::default
- Test: mevblocker.io HTTPS bağlantı kontrolü

### 3.2 `src/alloy/mod.rs` (186 satır)

**Layer Stack (dıştan içe):**
```
LabelingLayer → InstrumentationLayer → TracingRequestIdLayer → BatchCallLayer → RpcHeadersLayer → HTTP
```

- `TracingRequestIdLayer` — caller'ın task'ında çalışır (span'dan request_id alır)
- `RpcHeadersLayer` — innermost: batch koalesce sonrasında çalışır
- `rpc()` vs `unbuffered_rpc()` — batch katmanı yok/var

**`RpcClientRandomIdExt::with_random_id`:**
```rust
let id = rand::random::<u32>().into(); // u32→u64, overflow önlemi
```
Batch'te duplicate ID'lerden kaçınmak için random ID.

**`ProviderSignerExt::without_wallet`:**
Anvil impersonation için signer olmadan provider — key yokken ecrecover hatasından kaçınır.

**`ProviderExt::send_and_watch`** (test-util):
```rust
const DEFAULT_WATCH_TIMEOUT: Duration = Duration::from_secs(2);
```

### 3.3 `src/alloy/buffering.rs` (323 satır) — Kritik Bileşen

**`BatchCallLayer` (Tower Layer pattern):**
```rust
pub(crate) struct BatchCallProvider<S> {
    inner: PhantomData<S>,
    calls: mpsc::UnboundedSender<CallContext>,
}

type CallContext = (
    oneshot::Sender<Result<Response, TransportError>>,
    SerializedRequest,
);
```

**Mimari: Nagle-benzeri batch algoritması**
1. Her `call()` → oneshot channel aç → `mpsc::UnboundedSender`'a gönder
2. Background task: `calls.chunks_timeout(max_batch_size, batch_delay)` — ya batch dolar ya timeout
3. Batch → `RequestPacket::Batch(requests)` → HTTP node'a gönder
4. Cevap → `ResponsePacket::Batch(responses)` → ID'ye göre eşleştir → oneshot receivers

**`BatchRequestEntry` — Duplicate ID yönetimi:**
```rust
struct BatchRequestEntry {
    value: Option<ResponseSender>,
    duplicates: VecDeque<ResponseSender>,
}
```
- İlk sender: `value` (head)
- Aynı ID'ye ek senderlar: `duplicates` VecDeque (FIFO)
- `pop_front()`: önce `value.take()`, sonra `duplicates.pop_front()`
- Mantık: extra allocation yapmadan non-duplicate case'i optimize et

**Error propagation:**
- Tek batch fail → tüm senderlar aynı error alır
- `sender.is_canceled()` check: önce iptal edilen request'ler filtrelenir
- Worker panics → `UnboundedSender::unbounded_send` panic → pod restart

**Concurrency:**
```rust
.for_each_concurrent(config.ethrpc_max_concurrent_requests, process_batch)
```
Birden fazla batch aynı anda inflight olabilir.

**Desteklenmeyen:** Manuel batch çağrısı `RequestPacket::Batch` → error. Layer yalnızca otomatik batching yapar.

### 3.4 `src/alloy/instrumentation.rs` (246 satır)

**ProviderLabel:** Request'lere label metadata ekler:
```rust
struct ProviderLabel(String); // pre-allocated capacity: 30
```
- `append`: string başına insert (öne prepend → son katman önce)
- Son underscore: `From<ProviderLabel> for String` ile pop edilir

**Metrics:**
```rust
#[metric(subsystem = "alloy_rpc")]
struct Metrics {
    requests_inflight:                IntGaugeVec,   // (component, method)
    requests_complete:                IntCounterVec, // (component, method)
    requests_duration_seconds:        HistogramVec,  // (component, method)
    inner_batch_requests_initiated:   IntCounterVec, // (component, method)
}
```
- `on_request_start()` → `scopeguard::guard` ile RAII timer — drop'ta otomatik metric güncelleme
- Batch: timers vec, drop'ta toplu kayıt

**`ProviderLabelingExt::labeled`:** Mevcut Web3 üzerine ek LabelingLayer ekler, yeni RpcClient random ID ile.

### 3.5 `src/alloy/wallet.rs` (134 satır)

```rust
pub struct MutWallet(Arc<RwLock<EthereumWallet>>);
```

- `Arc<RwLock>` → cheap clone, mutable inner wallet
- `register_signer()`: Default address check → ilk signer otomatik default olur
- **`handle_blocking_operation`:** Tokio runtime flavor sorunu çözümü:
  - `CurrentThread` → `thread::spawn` (blocking lock'u ayrı thread'de çalıştır)
  - `MultiThread` → `block_in_place` (worker thread'i block et)
  - Diğer → panic

### 3.6 `src/alloy/evm_ext.rs` (implicit)
EVM extension methods.

### 3.7 `src/block_stream.rs` (417 satır)

```rust
pub struct BlockInfo {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub gas_limit: U256,
    pub gas_price: U256,
    pub base_fee: u64,
    pub observed_at: Instant, // PartialEq'den hariç!
}

pub type CurrentBlockWatcher = watch::Receiver<BlockInfo>;
```

**`current_block_ws_stream` — Yeni WebSocket tabanlı:**
1. WsConnect (auto-reconnection)
2. `subscribe_blocks()` stream aç
3. HTTP ile ilk block fetch (race condition azaltmak için)
4. `watch::channel(first_block)` — broadcast
5. Background task: yeni block gelince:
   - Hash aynıysa skip
   - Number azalmışsa skip (reorg?)
   - `sender.send(block_info)` fail → panic (pod restart)

**`current_block_stream` (deprecated):**
- HTTP polling, `poll_interval` bazlı
- `unbuffered_provider` (batch olmadan — ArbitrumOne 250ms gibi yüksek frekanslı chainlerde)
- WebSocket versiyonu tercih edilmeli

**`mock_single_block`:**
```rust
std::mem::forget(sender); // sender asla drop olmaz → receiver açık kalır
```

**`next_block`:**
```rust
let _ = stream.next().await; // mevcut değeri skip et
stream.next().await.expect("must never end")
```

**Metrics:**
```rust
block_stream_update_delta: HistogramVec, // labels("sign") — buckets: 0,1,2,4,8,25
last_block_number: GenericGauge<AtomicU64>
```
Delta negatif → "negative" label (reorg/tekrar); pozitif → "positive".

**DEX Uygulama Notu:**
- Nova Shield'da block stream için WebSocket subscription zorunlu
- `mock_single_block` pattern: `std::mem::forget(sender)` trick test infrastructure'da kullanışlı
- `BlockInfo.observed_at`: latency monitoring için — settlement timing ile karşılaştır
- `ethrpc_batch_delay: Duration::ZERO` default: delay yok, sadece burst batching

---

## 4. `crates/observe/` — Gözlemlenebilirlik (18 dosya)

### 4.1 `src/lib.rs`
Modüller: `config`, `event_bus`, `future`, `heap_dump_handler` (unix-only), `http_body`, `metrics`, `panic_hook`, `tracing`, `version`

### 4.2 `src/metrics.rs` (220 satır)

**Global Registry Pattern:**
```rust
static REGISTRY: OnceLock<prometheus_metric_storage::StorageRegistry> = OnceLock::new();
```

- `setup_registry(prefix, labels)`: tek seferlik init, ikinci call panic
- `setup_registry_reentrant`: test için — ikinci call ignore
- `get_storage_registry()`: init edilmemişse default (test için)

**HTTP Server:** `/metrics`, `/liveness`, `/ready`, `/startup` endpoints
- `DEFAULT_METRICS_PORT: u16 = 9586`
- Liveness: `Arc<dyn LivenessChecking>` trait objesi
- Readiness/Startup: `Arc<Option<AtomicBool>>` — None → immediately ready

**Shared Metrics:**
```rust
pub struct Metrics {
    pub auction_overhead_time:  GenericCounterVec<AtomicF64>,  // (component, phase)
    pub auction_overhead_count: GenericCounterVec<AtomicU64>,  // (component, phase)
}
```
`on_auction_overhead_start()` → `scopeguard::guard` RAII — drop'ta `start.elapsed()` ölçülür.

### 4.3 `src/tracing/init.rs` (186 satır)

**Tracing Stack:**
```
tracing_subscriber::registry()
  .with(LevelFilter::TRACE)
  .with(RequestIdLayer)
  .with(fmt_layer!(env_filter, stderr_threshold, use_json_format))
  .with(OpenTelemetry layer)  // opsiyonel
```

**Log formatlar:**
- JSON: `TraceIdJsonFormat`, `JsonFields`, no ANSI
- Text: `TraceIdFmt`, UtcTime `[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z`

**Stderr/Stdout routing:**
```rust
std::io::stderr.with_filter(|meta| meta.level() <= stderr_threshold)
               .or_else(std::io::stdout)
```

**OpenTelemetry (opsiyonel):**
- gRPC/Tonic OTLP exporter
- `Sampler::AlwaysOn` (TODO: sampling konfigürasyonu)
- `RandomIdGenerator`

**Reload handler (unix):**
- UNIX socket ile runtime log filter değiştirme
- `/tmp/log_filter_override_<program>_<pid>.sock`

**Tokio Console (playground-only):**
- `#[cfg(all(tokio_unstable, feature = "tokio-console"))]`
- `TOKIO_CONSOLE=true` env var ile

**`tracing_panic_hook`:**
```rust
let backtrace = std::backtrace::Backtrace::force_capture();
tracing::error!("thread '{name}' {panic}\nstack backtrace:\n{backtrace}");
```

### 4.4 `src/event_bus/mod.rs` (242 satır) — NATS Event Bus

**Architecture:**
```
publish() → mpsc::channel(1000) → publish_messages() → NATS JetStream → await_acks()
```

```rust
struct EventBusConnector {
    message_queue: Sender<Message>,
    subject_prefix: String,  // "event.{chain_id}."
}

static BUS: OnceCell<EventBusConnector> = OnceCell::const_new();
```

**Init:**
- Failure tolerant: NATS bağlantı hatası → log error, BUS uninitialized kalır, `publish` no-op
- `get_stream()` check: stream yoksa fail fast

**Two-stage JetStream publish:**
1. `client.publish()` → local buffer'a → `PublishAckFuture` döner
2. `PublishAckFuture.await` → server acknowledgment

**Ayrı tasks:**
- `publish_messages`: queue → NATS publish → ack future'ı ack_tx'e gönder
- `await_acks`: `FuturesUnordered` ile concurrent ack bekleme

**`biased` select:** pending acks önce drain edilir (backpressure yönetimi).

**`DropReason`:**
- `ChannelFull`: in-memory queue doldu (1000 kapasiteli)
- `Serialize`: JSON serialize hatası
- `Publish`: NATS client reject
- `Ack`: JetStream ack fail

**`publish_event<E: Event>`:** Typed event — `E::SUBJECT` constant kullanır
**`publish`:** Generic `impl Serialize` — envelope wrap + chain_id prefix

**Envelope:**
```rust
Envelope::new(request_id, data)  // tracing request_id dahil
```

**DEX Uygulama Notu:**
- Nova Shield'da order lifecycle eventlerini NATS üzerinden yayınlamak için aynı pattern
- Event bus failure tolerant: sistem durmamalı

---

## 5. `crates/gas-price-estimation/` — Gas Fiyatı Tahmini (7 dosya)

### 5.1 `src/lib.rs` (127 satır)

```rust
#[async_trait::async_trait]
pub trait GasPriceEstimating: Send + Sync {
    async fn estimate(&self) -> Result<Eip1559Estimation>;
    async fn base_fee(&self) -> Result<Option<u64>>;
    async fn effective_gas_price(&self) -> Result<u128> {
        // calc_effective_gas_price(max_fee, max_priority_fee, base_fee)
    }
}
```
`mockall::automock` ile mock desteği.

**EIP-1559 Estimation struct (alloy):**
```rust
Eip1559Estimation {
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
}
```

**`Eip1559EstimationExt`:**
```rust
fn effective(self, base_fee: u64) -> u128;
fn scaled_by_pml(self, pml: u64) -> Self; // parts-per-mille
```
`scaled_by_pml(100)` → %10 artış: `n * 1100 / 1000`

**`GasEstimatorType`:**
- `Web3` → `NodeGasPriceEstimator` (direct RPC)
- `Driver(url)` → `DriverGasEstimator` (driver API'ye sorma)
- `Alloy` → `ConfigurableGasPriceEstimator` (fee history based)

**`create_priority_estimator`:**
- Birden fazla estimator → `PriorityGasPriceEstimating` (fallback zinciri)
- Chain ID'yi node'dan alır

### 5.2 `src/priority.rs` (144 satır)

```rust
pub struct PriorityGasPriceEstimating {
    estimators: Vec<Estimator>,
}

struct Estimator {
    estimator: Box<dyn GasPriceEstimating>,
    errors_in_a_row: AtomicUsize,
}
```

**`prioritize` generic:**
```rust
async fn prioritize<'a, T, F, O>(&'a self, operation: T) -> Result<O>
where
    T: Fn(&'a dyn GasPriceEstimating) -> F,
    F: Future<Output = Result<O>>,
```
İlk başarılıya dön. Fail → `AtomicUsize` counter artır.

**Log strategy:** N < 10 hata → warn, N >= 10 → error. `LOG_ERROR_AFTER_N_ERRORS = 10`.

### 5.3 `src/configurable_alloy.rs` (266 satır) — Blok-tabanlı Cache

```rust
pub struct ConfigurableGasPriceEstimator {
    current_block: CurrentBlockWatcher,
    lastest_gas_price: watch::Receiver<Eip1559Estimation>,
}
```

**Initialization:**
- İlk değer: `base_fee * 2` (hem max_fee hem max_priority_fee)
- Background task: her yeni block'ta `get_fee_history()` → `eip1559_default_estimator()`

**`current_gas_price` algoritması:**
1. `get_fee_history(past_blocks, Latest, [reward_percentile])`
2. `latest_block_base_fee()` → 0 veya None ise block.base_fee kullan
3. `eip1559_default_estimator(base_fee, rewards)` → alloy'un varsayılan algoritması
4. EIP-1559 formül: `max_fee = base_fee * 2 + tip`, `max_priority = tip`

**Test örnekleri:**
```
base_fee=10, tip=1 → max_fee=10*2+1=21, max_priority=1
```

**Cache semantiği:** `estimate()` → `self.lastest_gas_price.borrow()` — sadece okuma, RPC yok.

**DEX Uygulama Notu:**
- Nova Shield settlement'larında EIP-1559 gas estimation zorunlu
- `scaled_by_pml` pattern: gas buffer için +10% = `scaled_by_pml(100)`
- Block-başına cache: aynı block'ta birden fazla estimation → tek RPC
- PriorityEstimating: birden fazla estimator kaynağı, fallback güvencesi

---

## Özet: Nova Shield İçin Kritik Öğrenimler

### Sayısal Güvenlik Katmanı
1. **`checked_mul_f64`:** Fee/ratio hesaplarında 10^18 scaling; BigRational yerine decimal-intent korur
2. **`NonZeroU256`:** Sıfır bölme koruması; fiyat denominatörleri için zorunlu
3. **`full_scalar_mul` (U512):** AMM fiyat ratio'larında intermediate overflow önler
4. **`ApproxEq`:** Relative error 1e-9 toleransı; DEX test altyapısında kullan

### RPC Katmanı
5. **Batch Layer:** Nagle-benzeri otomatik batch → tek HTTP round-trip'te 20 RPC call
6. **MutWallet:** `Arc<RwLock>` pattern — transaction signing runtime-safe
7. **Block Stream:** WS subscription + HTTP initial fetch race condition azaltması
8. **Random Request ID:** u32→u64 trick — u64::MAX overflow önlemi

### Gözlemlenebilirlik
9. **RAII Metrics:** `scopeguard::guard` ile otomatik timer/counter update
10. **NATS Event Bus:** Failure-tolerant, two-stage JetStream publish/ack

### Gas Estimation
11. **EIP-1559 default:** `max_fee = base_fee * 2 + tip` formülü
12. **Block-tabanlı cache:** Her block'ta tek `get_fee_history` RPC
13. **`scaled_by_pml`:** Parts-per-mille scaling, integer overflow-safe
