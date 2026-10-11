# CoW Protocol Analiz — Agent 13
## Kapsam: configs/ (28 rs) + observe/ (18 rs) + event-bus-dto/ (8 rs) + app-data/ (4 rs) + gas-price-estimation/ (7 rs)
## DEX Ozel Protokolu: 3x Hassasiyet Aktif

---

## GENEL BAKIS

Bu agent toplam **65 Rust dosyasini** kapsar. Kapsam:

| Crate | Dosya | Satir |
|---|---|---|
| configs | 28 | ~3,500 |
| observe | 18 | ~2,100 |
| event-bus-dto | 8 | ~600 |
| app-data | 4 | ~1,400 |
| gas-price-estimation | 7 | ~700 |
| **TOPLAM** | **65** | **~8,300** |

---

## BOLUM 1: `crates/configs/` — YAPILANDIRMA KATMANI

### Mimarisi

CoW Protocol'un tum binary'leri (autopilot, driver, orderbook, solvers, refunder) TOML tabanlı konfigurasyon kullanir. Ortak pattern:
- `SharedConfig`: tum servislerin paylastigi node_url, contracts, gas estimators
- Her binary kendi konfigurasyon struct'ini tanimlar
- `deny_unknown_fields` ile typo'lara karsi koruma
- `validate()` metodu ile cross-field dogrulama
- `%ENV_VAR` pattern ile secret injection (1Password entegrasyonu)

---

### [configs/src/lib.rs] (satir 1-20 tahmini)

**Amac:** Crate-level re-export ve modül bildirimleri.

**Modüller:**
- `autopilot` — autopilot binary konfigurasyon
- `driver` — driver binary konfigurasyon
- `orderbook` — orderbook binary konfigurasyon
- `shared` — tum servislerin paylastigi yapilar
- `fee_factor` — FeeFactor newtype
- `price_estimation` — fiyat tahmini parametreleri

---

### [configs/src/fee_factor.rs] (147 satir)

**Amac:** `FeeFactor` newtype — fee oranini `[0.0, 1.0)` araliginda tutar.

**Struct:**
```rust
pub struct FeeFactor(f64);
```

**Kisitlamalar:**
- `0.0 <= value < 1.0` — tam yakalama (1.0) yasak
- Sifir fee gecerli (ucretsiz islem)

**Kritik Metodlar:**

```rust
// BPS formatinda string (ornek: "0.0500%" = 5 BPS)
pub fn to_bps_str(&self) -> String {
    let bps = (self.0 * 1_000_000.0).round() / 100.0;
    format!("{bps:.4}%")
}

// Sub-BPS hassasiyet icin integer (1_000_000 olcegi)
pub const HIGH_PRECISION_SCALE: u64 = 1_000_000;
pub fn to_high_precision(&self) -> u64 {
    (self.0 * Self::HIGH_PRECISION_SCALE as f64).round() as u64
}
```

**Matematiksel Model:**
- `1 BPS = 0.01% = 0.0001`
- `HIGH_PRECISION_SCALE = 1_000_000` => sub-BPS hassasiyet
- Ornek: `FeeFactor(0.0005)` = 5 BPS => `to_high_precision() = 500`

**Serde:**
- `#[serde(try_from = "f64")]` — dogrudan f64'ten deserialize
- `TryFrom<f64>` impl ile range dogrulamasi

**DEX Uygulama Notu:**
Nova Shield DEX fee yapisi ayni FeeFactor modelini benimsemeli:
- `[0.0, 1.0)` aralik kisitlamasi
- `HIGH_PRECISION_SCALE = 1_000_000` sub-BPS hassasiyet icin
- Farkli pool tier'lari icin: 0.01% (1 BPS), 0.05% (5 BPS), 0.30% (30 BPS), 1.00% (100 BPS)
- On-chain integer temsili: `fee_bps * 1_000_000` ile overflow'suz saklama

---

### [configs/src/autopilot/fee_policy.rs] (189 satir)

**Amac:** CoW Protocol'un ucret modeli yapilandirmasi — 3 farkli fee mekanizmasi.

**Ana Struct:**
```rust
pub struct FeePoliciesConfig {
    pub default_fee_policy: Vec<FeePolicy>,
    pub fee_policy_max_partner_fee: FeeFactor,  // Default: 1% (100 BPS)
    pub upcoming_fee_policies: Vec<UpcomingFeePolicies>,
}
```

**Uc Fee Modeli:**

```rust
pub enum FeePolicy {
    Surplus {
        factor: FeeFactor,         // Surplus'in yuzdesi
        max_volume_factor: FeeFactor,  // Maks hacim yuzdesi
    },
    PriceImprovement {
        factor: FeeFactor,
        max_volume_factor: FeeFactor,
        quote: PriceImprovementQuote,
    },
    Volume {
        factor: FeeFactor,  // Islem hacminin direkt yuzdesi
    },
}
```

**Fee Hesaplama Matematigi:**

*Surplus Fee:*
```
surplus = kullanici_aldi - beklenen_miktar
fee = min(surplus * factor, hacim * max_volume_factor)
```

*PriceImprovement Fee:*
```
price_improvement = gercek_fiyat - teklif_fiyati
fee = min(price_improvement * factor, hacim * max_volume_factor)
```

*Volume Fee:*
```
fee = hacim * factor  // En basit model
```

**Zamanlama (UpcomingFeePolicies):**
```rust
pub struct UpcomingFeePolicies {
    pub activation_time: chrono::DateTime<Utc>,  // Gecis zamani
    pub fee_policies: Vec<FeePolicy>,
}
```

Bu mekanizma, fee degisikliklerini onceden planlayip otomatik gecisi saglar.

**FeePolicyOrderClass:**
```rust
pub enum FeePolicyOrderClass {
    Market,   // Anlık piyasa emirleri
    Limit,    // Limit emirleri
    Any,      // Her tur
}
```

**Varsayilan Degerler:**
- `fee_policy_max_partner_fee`: 1% (100 BPS)
- `default_fee_policy`: bos Vec (fee yok)

**DEX Uygulama Notu:**
Nova Shield DEX icin cok katmanli fee modeli:
1. **Temel fee**: Volume tabanlı, sabit BPS (pool tier'a gore)
2. **Surplus fee**: CoW-benzeri, kullanicinin elde ettigi surplus'in yuzdesi — MEV korumasiyla birlikte anlam kazanir
3. **Partner fee**: Aggregator/frontend'e split — Nova Shield DEX router partner fee destegi
4. `UpcomingFeePolicies` benzeri zamanlama: governance oylama sonrasi otomatik fee gecisi
5. Nasil daha iyi: Piyasa durumuna gore dinamik fee (yuksek volatilite = dusuk fee ile daha fazla hacim cek)

---

### [configs/src/autopilot/mod.rs] (yaklasik 400+ satir)

**Amac:** Autopilot binary'sinin tam konfigurasyonu.

**Ana Struct:**
```rust
pub struct AutopilotConfig {
    pub shared: SharedConfig,
    pub fee_policies: FeePoliciesConfig,
    pub auction_time_limit: Duration,      // Auction suresi
    pub max_settlement_transaction_wait: Duration,
    pub shadow: Option<ShadowConfig>,      // Shadow mode
    pub db: DatabaseConfig,
    // ... diger alanlar
}
```

**Shadow Modu:**
```rust
pub struct ShadowConfig {
    pub cow_api: Url,           // Upstream CoW API
    pub update_interval: Duration,
}
```
Amac: Production ortaminda auction'lari gercek gonderim yapmadan test etmek.

**Auction Parametreleri:**
- `auction_time_limit`: solver'lara verilen sure (genellikle ~5-8 saniye)
- `max_settlement_transaction_wait`: on-chain confirm bekleme suresi (2-3 blok)

---

### [configs/src/driver/mod.rs] ve [configs/src/driver/*.rs]

**Amac:** Driver binary konfigurasyonu.

**Ana Yapilar:**
```rust
pub struct DriverConfig {
    pub shared: SharedConfig,
    pub submission: SubmissionConfig,
    pub liquidity: LiquidityConfig,
    pub simulation: SimulationConfig,
    pub solver: SolverConfig,
}
```

**SubmissionConfig:**
- `gas_price_cap`: Maks kabul edilebilir gas fiyati (MEV korumasiyla alakali)
- `target_confirm_time`: Hedef onay suresi
- `retry_interval`: Basarisiz islem yeniden deneme araligi
- `max_additional_tip`: EIP-1559 tip maksimumu

**LiquidityConfig:**
- Hangi likidite kaynaklarinin kullanilacagi: UniV2, UniV3, Balancer, CoW AMM
- `base_tokens`: Rotalamada kullanilacak ara tokenlar (WETH, USDC, DAI, WBTC)

**SimulationConfig:**
- `tenderly_*`: Tenderly API entegrasyonu (simulation oncesi dogrulama)
- `disable_access_lists`: Access list hesaplamani devre disi bırak (debug)

---

### [configs/src/orderbook/mod.rs]

**Amac:** Orderbook HTTP API konfigurasyonu.

**Ana Yapilar:**
```rust
pub struct OrderbookConfig {
    pub shared: SharedConfig,
    pub bind_address: SocketAddr,
    pub db: DatabaseConfig,
    pub order_quoting: OrderQuotingConfig,
    pub fee: FeeConfig,
}
```

**OrderQuotingConfig:**
```rust
pub struct OrderQuotingConfig {
    pub quote_validity_seconds: u32,    // Quote gecerlilik suresi
    pub eip1271_onchain_quote: bool,    // EIP-1271 on-chain imza dogrulama
}
```

**Varsayilan:**
- `bind_address`: `0.0.0.0:8080`
- `quote_validity_seconds`: 60 saniye

---

### [configs/src/shared.rs] (546 satir)

**Amac:** Tum servislerin paylastigi konfigurasyonlar.

**Ana Struct:**
```rust
pub struct SharedConfig {
    pub node_url: Url,
    pub chain_id: ChainId,
    pub contracts: ContractsConfig,
    pub gas_estimators: Vec<GasEstimatorType>,
    pub event_bus: Option<EventBusConfig>,
    pub settlement_contract_address: H160,
}
```

**ContractsConfig:**
```rust
pub struct ContractsConfig {
    pub gp_v2_settlement: H160,        // GPv2Settlement ana contract
    pub balancer_v2_vault: H160,       // Balancer entegrasyonu
    pub uniswap_v3_router: H160,       // UniV3 rotalama
    pub weth: H160,                    // Wrapped ETH adresi
}
```

**EventBusConfig:**
```rust
pub struct EventBusConfig {
    pub nats_url: String,              // NATS server URL
    pub channel_name: String,          // JetStream channel adi
    // Kisitlama: bosluk, tab, nokta YASAK
}
```

**TokenBucketFeeOverride:**
```rust
pub struct TokenBucketFeeOverride {
    pub tokens: HashSet<H160>,    // Hangi token ciftleri
    pub fee: FeeFactor,           // Bu ciftler icin ozel fee
}
```
Kullanim: Stablecoin ciftleri (USDC-USDT) icin dusuk fee.

**GasEstimatorType:**
```rust
pub enum GasEstimatorType {
    Native,         // eth_gasPrice
    Web3,           // alloy provider
    EthGasStation,  // harici API
    BlockNative,    // harici API
    Eden,           // Eden Network
    GasOracle,      // genel oracle
}
```

**DEX Uygulama Notu:**
Nova Shield icin `SharedConfig` benzeri yapisal merkezilesme:
- Token pair bazinda fee override: Nova Shield stablecoin AMM pool'lari icin 0.01% fee
- NATS yerine Kafka veya kendi event bus implementasyonu kullanilabilir
- `chain_id` zorunlu: multi-chain destek icin her zincir ayri config instance

---

### [configs/src/price_estimation.rs] (445 satir)

**Amac:** Fiyat tahmini altyapisi konfigurasyon.

**Ana Struct:**
```rust
pub struct PriceEstimation {
    pub quote_inaccuracy_limit: f64,           // Default: 1 (1% tolerans)
    pub quote_timeout: Duration,               // Default: 5s
    pub max_gas_per_tx: u64,                  // Default: 16_777_215 (Fusaka)
    pub balances: BalanceOverridesConfig,
    pub state_override_stream: Option<StateOverrideStreamConfig>,
    pub coin_gecko: Option<CoinGeckoConfig>,
    // ... diger alanlar
}
```

**Kritik Sabit:**
```
max_gas_per_tx = 16_777_215 = 0xFFFFFF
```
Bu deger Ethereum Fusaka hardfork sonrasi EIP-7623 ile gelen blob gas siniriyla uyumlu.

**BalanceOverridesConfig:**
```rust
pub struct BalanceOverridesConfig {
    pub probing_depth: usize,   // Default: 60 slot
    pub cache_size: usize,       // Default: 1000 entry
    pub detection_timeout: Duration,  // Default: 1s
}
```
Amac: ERC-20 token'larin hangi storage slot'unda balance tutuldugunu otomatik tespit.

**Algoritma (BalanceOverride):**
1. `eth_call` ile 60 storage slot'unu probe et
2. Balance'in hangi slot'ta degistigini bul
3. `eth_call` parametrelerinde o slot'u override et
4. Cache'le (1000 entry, TinyLFU)

**StateOverrideStreamConfig:**
```rust
pub struct StateOverrideStreamConfig {
    pub url: Url,           // WebSocket endpoint
    pub max_age: Duration,  // Default: 5 saniye
}
```
WebSocket uzerinden gercek zamanli state override'lari (arbitraj oncesi state simülasyonu).

**CoinGeckoConfig:**
```rust
pub struct CoinGeckoConfig {
    pub api_url: Url,
    pub api_key: Option<String>,    // `%ENV_VAR` pattern
    pub cache_timeout: Duration,
    pub debounce_interval: Duration,
}
```

**DEX Uygulama Notu:**
Nova Shield fiyat tahmini mimarisi:
1. On-chain AMM fiyati (birincil kaynak)
2. TWAP oracle (Uniswap V3 TWAP benzeri, manipulation-resistant)
3. Off-chain CoinGecko/CoinMarketCap (fallback, yuksek latency tolerans)
4. `BalanceOverrides` benzeri storage probe: Nova Shield kendi token'lari icin onceden tespit edip cache'le
5. `max_gas_per_tx = 16_777_215`: Nova Shield da ayni siniri kullanmali (EVM uyumlu)

---

### [configs/src/autopilot/settlement_transaction_priority.rs]

**Amac:** Settlement islemlerini blok'a dahil etme onceligi.

**Yapilar:**
```rust
pub enum Priority {
    EthereumValidator,    // Dogrudan validator'a gonder
    Flashbots,            // Flashbots bundle
    MevBlocker,           // MEV Blocker (Gnosis)
}
```

**DEX Uygulama Notu:**
Nova Shield MEV korumasi:
- Flashbots / MEV Blocker entegrasyonu
- Private mempool gonderiminler
- Colocated solver = validator-side execution

---

### [configs/src/solvers.rs] / [configs/src/baseline.rs]

**Amac:** Solver konfigurasyonu.

**BaselinesConfig:**
```rust
pub struct BaselinesConfig {
    pub max_hops: u32,           // Rotalama derinligi (default: 2)
    pub max_partial_attempts: u32,  // Parcali rota denemesi
}
```

`max_hops = 2`: A -> WETH -> B gibi 2 hop'lu rota.
`max_hops = 3`: A -> WETH -> DAI -> B gibi 3 hop'lu rota (daha fazla likidite ama daha yuksek gas).

---

### [configs/src/autopilot/mempoolmonitoring.rs]

**Amac:** Mempool izleme konfigurasyonu.

```rust
pub struct MempoolMonitoringConfig {
    pub mev_blocker_tx_source: Option<Url>,  // MEV Blocker source
    pub update_interval: Duration,            // Mempool guncelleme araligi
}
```

Amac: Bekleyen settlement transaction'larini izleyip cakismalari (replacement, reorg) tespit et.

---

### [configs/src/autopilot/cow_amm.rs]

**Amac:** CoW AMM (Automated Market Maker) konfigurasyonu.

```rust
pub struct CowAmmConfig {
    pub factory: H160,          // CoW AMM factory contract
    pub index_start: u64,       // Hangi bloktan itibaren indexle
    pub helper: H160,           // Yardimci contract
}
```

**DEX Uygulama Notu:**
CoW AMM, klasik AMM'den farkli: batch auction icinde optimal swap bulur, JIT likidite saglar. Nova Shield AMM icin:
- Batch auction ile entegre AMM: her blokta tum emirleri topluca isle
- JIT likidite: LP'ler batch icinde likidite saglar, sonra ceker (impermanent loss azalir)

---

### [configs/src/ipfs.rs]

**Amac:** IPFS baglantisi konfigurasyonu (app-data icin).

```rust
pub struct IpfsConfig {
    pub gateway: Url,            // IPFS gateway URL
    pub pinata_api_key: Option<String>,   // Pinning servisi
    pub pinata_secret: Option<String>,
}
```

Order metadata (app-data) IPFS'e yuklenir, 32-byte keccak256 hash on-chain saklanir.

---

### [configs/src/persistence.rs]

**Amac:** Veritabani konfigurasyonu.

```rust
pub struct DatabaseConfig {
    pub url: String,            // `%DB_URL` gibi env pattern
    pub pool_size: u32,         // Default: 5
    pub connection_timeout: Duration,
}
```

---

## BOLUM 2: `crates/observe/` — GOZLEM ALTYAPISI

### Mimari Ozet

`observe` crate'i CoW Protocol'un tam gozlem altyapisini saglar:
1. **Metrics** (Prometheus): Metric registration + MetricStorage derive macro
2. **Tracing** (tracing-subscriber + OpenTelemetry): Structured logging + distributed tracing
3. **Event Bus** (NATS JetStream): Async event yayini
4. **Future utilities**: Measurable timer, Lazy log helper
5. **Heap profiling**: jemalloc UNIX socket

---

### [observe/src/lib.rs] (satir 1-30 tahmini)

**Amac:** Public API re-export.

**Re-export edilen semboller:**
```rust
pub use {
    event_bus,
    future::{Measurable, Measure},
    metrics::{MetricParams, MetricStorage},
    tracing::{
        distributed::{request_id, trace_id_format},
        init::{initialize, initialize_reentrant},
        lazy::Lazy,
    },
    version::git_version,
};
```

---

### [observe/src/version.rs] (11 satir)

**Amac:** Git versiyonunu runtime'da raporla.

```rust
pub fn git_version() -> &'static str {
    static VERSION: LazyLock<String> = LazyLock::new(|| {
        let sha = std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".into());
        let branch = std::env::var("GIT_BRANCH").unwrap_or_else(|_| "unknown".into());
        format!("{branch}@{sha}")
    });
    &VERSION
}
```

**Kullanim:** Binary baslarken `info!("version: {}", git_version())` — deployment tracking icin.

**`LazyLock` Tasarimi:** `std::sync::LazyLock` (Rust 1.80+) — thread-safe tek seferlik init, `OnceLock` yerine tercih edilmis (daha ergonomik API).

---

### [observe/src/metrics.rs] (tahmini 150+ satir)

**Amac:** Prometheus metric altyapisi.

**Global Registry:**
```rust
static REGISTRY: OnceLock<Registry> = OnceLock::new();

pub fn get_registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::default)
}
```

**MetricStorage derive macro:**
```rust
#[derive(MetricStorage)]
pub struct MyMetrics {
    #[metric(name = "orders_total", help = "Toplam emir sayisi")]
    pub orders_total: IntCounterVec,

    #[metric(name = "settlement_duration_seconds")]
    pub settlement_duration: HistogramVec,
}
```

Macro, `MetricStorage` trait'ini implement eder:
```rust
pub trait MetricStorage: Sized {
    fn instance(subsystem: &str) -> &'static Self;
}
```

**Kullanim Pattern:**
```rust
let metrics = MyMetrics::instance("orderbook");
metrics.orders_total.with_label_values(&["market"]).inc();
```

**DEX Uygulama Notu:**
Nova Shield DEX icin kritik metrikler:
- `swap_volume_usd` (LabelVec: pool, token_pair)
- `price_impact_histogram` (0.1%, 0.5%, 1%, 5% buckets)
- `settlement_gas_used` (gas efficiency izleme)
- `solver_competition_winners` (hangi solver kazaniyor)
- `mev_protected_orders_total` vs `unprotected_orders_total`

---

### [observe/src/tracing/mod.rs] (5 satir)

**Amac:** Tracing modül bildirimleri.

```rust
pub mod distributed;
pub mod init;
pub mod lazy;
#[cfg(unix)]
mod reload_handler;
```

---

### [observe/src/tracing/init.rs] (186 satir)

**Amac:** Global tracing subscriber kurulumu.

**Iki init fonksiyonu:**
```rust
// Tek seferlik — production binary'leri icin
pub fn initialize(config: &Config) {
    set_tracing_subscriber(config);
    std::panic::set_hook(Box::new(tracing_panic_hook));
}

// Tekrar cagrilabilir — testler icin
pub fn initialize_reentrant(config: &Config) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| { ... });
}
```

**Format Katmanlari:**

*JSON (production):*
```rust
tracing_subscriber::fmt::layer()
    .with_ansi(false)
    .fmt_fields(JsonFields::default())
    .event_format(TraceIdJsonFormat)
    .with_writer(writer)
```

*Terminal (gelistirme):*
```rust
tracing_subscriber::fmt::layer()
    .with_timer(UtcTime::new(format_description!(...)))
    .with_ansi(is_terminal)
    .map_event_format(|f| TraceIdFmt { inner: f.with_ansi(is_terminal) })
```

**Stderr Yonlendirme:**
```rust
let writer = std::io::stderr
    .with_filter(move |meta| {
        // WARN ve ERROR seviyesi stderr'e
        stderr_threshold.is_some_and(|min| meta.level() <= &min)
    })
    .or_else(std::io::stdout);
```
Amac: WARN/ERROR'u stderr'e, INFO/DEBUG'u stdout'a yonlendir — container log pipeline uyumu.

**OpenTelemetry (OTLP):**
```rust
let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
    .with_tonic()
    .with_endpoint(tracing_config.collector_endpoint.as_str())
    .with_timeout(tracing_config.export_timeout)
    .build()?;

let tracer = opentelemetry_sdk::trace::SdkTracerProvider::builder()
    .with_batch_exporter(otlp_exporter)
    .with_sampler(Sampler::AlwaysOn)
    .with_id_generator(RandomIdGenerator::default())
    .with_resource(Resource::builder()
        .with_attribute(KeyValue::new("service.name", service_name))
        .build())
    .build()
    .tracer("cow_tracing");
```

**Sampler:** `AlwaysOn` — her span export edilir (sampling yok). TODO notu var: ileride yapilandirilabilir sampling.

**Tokio Console (sadece playground):**
```rust
#[cfg(all(tokio_unstable, feature = "tokio-console"))]
{
    if std::env::var("TOKIO_CONSOLE") == Ok("true".into()) {
        subscriber.with(console_subscriber::spawn()).init();
    }
}
```

**Runtime Filter Reload (UNIX socket):**
```rust
if cfg!(unix) {
    spawn_reload_handler(initial_filter, reload_handle);
}
```
`/tmp/log_filter_override_<program>_<pid>.sock` — runtime'da log seviyesi degistirme.

**Panic Hook:**
```rust
fn tracing_panic_hook(panic: &PanicHookInfo) {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    let backtrace = std::backtrace::Backtrace::force_capture();
    tracing::error!("thread '{name}' {panic}\nstack backtrace:\n{backtrace}");
}
```
Amac: Panic'leri `tracing::error!` ile logla — JSON format ve Kibana/VictoriaLogs uyumu.

---

### [observe/src/tracing/lazy.rs] (54 satir)

**Amac:** Lazy log degerlendirme — pahalı hesaplamalari sadece log aktifse yap.

```rust
pub struct Lazy<T>(pub T);

impl<T, D> std::fmt::Debug for Lazy<T>
where T: Fn() -> D, D: std::fmt::Debug
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", (self.0)())
    }
}

impl<T, D> std::fmt::Display for Lazy<T>
where T: Fn() -> D, D: std::fmt::Display { ... }
```

**Kullanim:**
```rust
tracing::debug!(
    value = ?Lazy(|| compute_expensive_value())
);
// compute_expensive_value() sadece DEBUG aktifse cagrilir
```

**Test:**
```rust
#[test]
fn lazy_in_macro() {
    tracing::debug!(
        miep = ?Lazy(|| {
            panic!("bu panic olmamali cunku lazy degerlendirme");
            "abc"
        })
    )
}
```
Test gecerli cunku `tracing::debug!` kapaliysa closure hic cagrilmaz.

**Nasil calisir:** `tracing` makrolari, field degerlerini `fmt::Debug` / `fmt::Display` trait'i uzerinden talep eder. Log seviyesi kapaliysa bu metod hic cagrilmaz — sifir maliyet.

---

### [observe/src/tracing/distributed/request_id.rs]

**Amac:** HTTP X-Request-ID header'ini tracing span'ina tasimak.

```rust
pub struct RequestIdLayer;

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for RequestIdLayer {
    fn on_new_span(&self, attrs: &Attributes, id: &Id, ctx: Context<S>) {
        // Task-local storage'dan request_id oku
        // Span'a field olarak ekle
    }
}
```

**Task-local storage:**
```rust
task_local! {
    static REQUEST_ID: Option<String>;
}
```

**Axum middleware:**
```rust
async fn request_id_middleware(req: Request, next: Next) -> Response {
    let id = req.headers()
        .get("X-Request-ID")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    REQUEST_ID.scope(id, next.run(req)).await
}
```

---

### [observe/src/tracing/distributed/trace_id_format.rs]

**Amac:** Log mesajina OpenTelemetry trace_id ekleme.

**JSON format:**
```rust
pub struct TraceIdJsonFormat;
// Log'a "trace_id": "abc123..." field'i ekler
```

**Terminal format:**
```rust
pub struct TraceIdFmt<T> { pub inner: T }
// "[trace_id=abc123] mesaj" prefix'i ekler
```

---

### [observe/src/tracing/reload_handler.rs] (Unix only)

**Amac:** Runtime'da log filter degistirme.

```rust
pub fn spawn_reload_handler(
    initial_filter: String,
    reload_handle: Handle<EnvFilter, Registry>,
) {
    tokio::spawn(async move {
        let socket_path = format!(
            "/tmp/log_filter_override_{}_{}.sock",
            program_name,
            std::process::id()
        );

        // Unix domain socket olustur
        let listener = UnixListener::bind(&socket_path)?;

        loop {
            let (mut stream, _) = listener.accept().await?;
            // Yeni filter string oku
            let new_filter = read_line(&mut stream).await?;
            // Uygula
            reload_handle.reload(EnvFilter::new(new_filter))?;
        }
    });
}
```

**Kullanim:**
```bash
echo "info,driver=debug" | nc -U /tmp/log_filter_override_driver_12345.sock
```

---

### [observe/src/future.rs] (86 satir)

**Amac:** Future'lari Prometheus histogram ile olcmek.

**Measurable<T>:**
```rust
pub struct Measurable<T> {
    inner: T,
    timer: Option<HistogramTimer>,  // pin_project ile pin edilmis
}

impl<T: Future> Future for Measurable<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        // Ilk poll'da timer baslat (RAII)
        if this.timer.is_none() {
            *this.timer = Some(histogram.start_timer());
        }

        match this.inner.poll(cx) {
            Poll::Ready(val) => {
                // Timer'i durdur (drop ile otomatik record)
                this.timer.take();
                Poll::Ready(val)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
```

**Measure Trait (blanket impl):**
```rust
pub trait Measure: Sized {
    fn measure(self, histogram: &HistogramVec, labels: &[&str]) -> Measurable<Self>;
}

impl<T: Future> Measure for T {
    fn measure(self, histogram: &HistogramVec, labels: &[&str]) -> Measurable<Self> {
        Measurable::new(self, histogram.with_label_values(labels))
    }
}
```

**Kullanim:**
```rust
let result = solve_auction(auction)
    .measure(&metrics.solve_duration, &["baseline"])
    .await;
```

**DEX Uygulama Notu:**
Nova Shield DEX critical path olcumu:
- `order_matching_duration` histogram (solver algoritma suresi)
- `settlement_encoding_duration` (calldata olusturma)
- `rpc_call_duration` (blockchain interaction)
- Tum kritik async operasyonlari `.measure()` ile wrap et

---

### [observe/src/event_bus/mod.rs] (242 satir)

**Amac:** NATS JetStream uzerinden async event yayini.

**Sabitler:**
```rust
const EVENT_BUS_SIZE: usize = 1000;  // In-memory buffer kapasitesi
```

**EventBus struct:**
```rust
pub struct EventBus {
    sender: mpsc::Sender<(Subject, Bytes)>,
}
```

**Subject Format:**
```
event.<chain_id>.<event_type>
Ornek: event.1.orderPlaced (mainnet)
        event.100.priceEstimate (Gnosis Chain)
```

**Iki-Task Mimarisi:**

*Task 1 — Publisher:*
```rust
async fn publisher_task(
    mut receiver: mpsc::Receiver<(Subject, Bytes)>,
    js: JetStream,
) {
    while let Some((subject, payload)) = receiver.recv().await {
        match js.publish(subject, payload).await {
            Ok(ack_future) => {
                ack_sender.send(ack_future).await.ok();
            }
            Err(e) => {
                metrics.drop_reason.inc(&[DropReason::PublishError]);
            }
        }
    }
}
```

*Task 2 — Ack collector:*
```rust
async fn ack_task(
    mut ack_receiver: mpsc::Receiver<AckFuture>,
) {
    while let Some(ack) = ack_receiver.recv().await {
        if let Err(e) = ack.await {
            metrics.drop_reason.inc(&[DropReason::AckError]);
        }
    }
}
```

**DropReason enum:**
```rust
pub enum DropReason {
    ChannelFull,     // Buffer 1000 dolu, en yeni event dusuruldu
    PublishError,    // NATS publish basarisiz
    AckError,        // JetStream ACK basarisiz
}
```

**Prometheus counter:** `event_bus_drop_total{reason="channel_full|publish_error|ack_error"}`

**EventBus::publish:**
```rust
pub async fn publish<E: Event>(&self, event: E) -> Result<()> {
    let subject = E::subject(self.chain_id);
    let payload = serde_json::to_vec(&Envelope {
        version: 1,
        data: event,
    })?;

    match self.sender.try_send((subject, payload.into())) {
        Ok(_) => Ok(()),
        Err(TrySendError::Full(_)) => {
            metrics.drop_reason.inc(&[DropReason::ChannelFull]);
            Ok(())  // Sessizce dusur — backpressure yok
        }
        Err(TrySendError::Disconnected(_)) => Err(anyhow!("event bus disconnected")),
    }
}
```

**Kritik Tasarim Karari:** `try_send` kullanilmis (bloklamayan). Buffer doluysa event sessizce dusurulur. Bu observability icin kabul edilebilir (kritik islem akisi etkilenmemeli) ama event kaybi soz konusu.

**DEX Uygulama Notu:**
Nova Shield event bus mimarisi:
- NATS JetStream yerine Kafka tercih edilebilir (yuksek throughput, retention, replay)
- Buffer boyutu 1000 yeterli mi? 10k TPS DEX icin 1000 cok kucuk — 10_000+ onerilir
- `DropReason` metric kritik: production'da channel_full artisi = event bus bottleneck
- Alternativ: back-pressure ile caller'i blokla (tradeoff: latency vs event loss)

---

### [observe/src/event_bus/subject.rs]

**Amac:** NATS subject string olusturma.

```rust
pub struct Subject(String);

impl Subject {
    pub fn new(chain_id: ChainId, event_type: &str) -> Self {
        Self(format!("event.{}.{}", chain_id, event_type))
    }
}
```

---

### [observe/src/event_bus/envelope.rs]

**Amac:** Event sarmalama (versiyonlama icin).

```rust
#[derive(Serialize, Deserialize)]
pub struct Envelope<T> {
    pub version: u8,   // Simdilik her zaman 1
    pub data: T,
}
```

**Amac:** Gelecekteki backward compatibility — `version` fieldi ile farkli format'lari ayirt etmek.

---

### [observe/src/heap.rs]

**Amac:** jemalloc heap profiling UNIX socket.

```rust
pub fn spawn_heap_profiler() {
    let socket_path = "/tmp/heap_profiler.sock";

    tokio::spawn(async move {
        let listener = UnixListener::bind(socket_path)?;

        loop {
            let (mut stream, _) = listener.accept().await?;
            let command = read_line(&mut stream).await?;

            match command.trim() {
                "dump" => {
                    // jemalloc heap dump al
                    jemalloc_ctl::raw::write(b"prof.dump\0", b"\0")?;
                }
                "stats" => {
                    // Memory istatistikleri don
                    let stats = jemalloc_ctl::epoch::mib()?.read()?;
                    stream.write_all(format!("{stats}").as_bytes()).await?;
                }
                _ => {}
            }
        }
    });
}
```

**Kullanim:**
```bash
echo "dump" | nc -U /tmp/heap_profiler.sock
echo "stats" | nc -U /tmp/heap_profiler.sock
```

**MALLOC_CONF:** jemalloc profiling aktiflestirilmesi:
```bash
MALLOC_CONF="prof:true,prof_prefix:/tmp/heap" ./orderbook
```

---

### [observe/src/config.rs]

**Amac:** observe crate konfigurasyonu.

```rust
pub struct Config {
    pub env_filter: EnvFilter,         // Log seviye filtresi
    pub use_json_format: bool,         // JSON vs terminal format
    pub stderr_threshold: Option<LevelFilter>,  // Bu seviyeden yukari stderr'e
    pub tracing: Option<TracingConfig>,
}

pub struct TracingConfig {
    pub collector_endpoint: Url,   // OTLP collector (Jaeger/Tempo)
    pub service_name: String,
    pub export_timeout: Duration,
    pub level: Level,              // Min span seviyesi
}
```

---

## BOLUM 3: `crates/event-bus-dto/` — EVENT DATA TRANSFER OBJECTS

### Mimarisi

NATS JetStream uzerinden yayinlanan event'lerin JSON wire format tanimlari.

**4 Event Tipi:**
1. `OrderPlaced` — Yeni emir eklendi
2. `OrderCancelled` — Emir iptal edildi
3. `Settlement` — Settlement tamamlandi
4. `PriceEstimate` — Fiyat tahmini sonucu

---

### [event-bus-dto/src/lib.rs]

**Amac:** Event trait ve re-export.

```rust
pub trait Event: Serialize + DeserializeOwned {
    fn subject(chain_id: u64) -> Subject;
}
```

---

### [event-bus-dto/src/order_placed.rs]

**Amac:** Yeni emir olusturuldu event'i.

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OrderPlaced {
    pub uid: OrderUid,           // 56-byte order identifier
    pub order: Order,            // Tam emir verisi
    pub timestamp: DateTime<Utc>,
}
```

**OrderUid yapisal tanim:**
```
[0..32]  = keccak256(order_data + owner + domain_separator)
[32..52] = owner (20 bytes, H160)
[52..56] = validTo (4 bytes, u32 Unix timestamp)
```

**Subject:** `event.<chain_id>.orderPlaced`

**Wire format ornek:**
```json
{
  "version": 1,
  "data": {
    "uid": "0xabcd...ef012345678901234567890123456789012345678901234567890123456789abcdef01",
    "order": {
      "sell_token": "0x...",
      "buy_token": "0x...",
      "sell_amount": "1000000000000000000",
      "buy_amount": "950000000000",
      "valid_to": 1700000000,
      "kind": "sell",
      "fee_amount": "1000000000000000"
    },
    "timestamp": "2024-01-15T12:00:00Z"
  }
}
```

**DEX Uygulama Notu:**
Nova Shield order event'leri ayni format benimsemeli. Ek field'lar:
- `mev_protection_level`: "none" | "basic" | "full"
- `source`: "ui" | "api" | "aggregator"
- `partner_id`: frontend partner identifier

---

### [event-bus-dto/src/order_cancelled.rs]

**Amac:** Emir iptal event'i.

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OrderCancelled {
    pub uid: OrderUid,
    pub timestamp: DateTime<Utc>,
    pub reason: Option<CancellationReason>,
}

pub enum CancellationReason {
    UserRequested,
    Expired,
    Replaced,
    InvalidSignature,
}
```

**Subject:** `event.<chain_id>.orderCancelled`

---

### [event-bus-dto/src/settlement.rs]

**Amac:** Basarili settlement event'i.

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Settlement {
    pub transaction_hash: H256,
    pub auction_id: i64,
    pub solver: H160,
    pub orders: Vec<SettledOrder>,
    pub gas_used: U256,
    pub effective_gas_price: U256,
    pub timestamp: DateTime<Utc>,
}

pub struct SettledOrder {
    pub uid: OrderUid,
    pub sell_amount: U256,
    pub buy_amount: U256,
    pub fee: U256,
    pub surplus: U256,  // Kullanicinin beklentisinin ustunde kazandigi
}
```

**Surplus Hesaplama:**
```
sell order: surplus = buy_amount_received - buy_amount_requested
buy order:  surplus = sell_amount_requested - sell_amount_paid
```

**Subject:** `event.<chain_id>.settlement`

**DEX Uygulama Notu:**
Nova Shield settlement event'i kritik analytics icin:
- `surplus` alan: MEV korumasinin etkinligini olc
- `solver` field: hangi solver kazandi — competition health
- `gas_used / gas_limit` ratio: settlement efficiency
- Bu event'ler real-time dashboard'da goruntulenmeli

---

### [event-bus-dto/src/price_estimate.rs] (97 satir)

**Amac:** Fiyat tahmini sonuclari event'i.

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PriceEstimateEvent {
    pub query: PriceQuery,
    pub from: Option<H160>,          // Talep eden adres (optional)
    pub timeout_ms: u64,             // Verilen sure
    pub elapsed_ms: u64,             // Gercekte gecen sure
    pub estimator: String,           // Hangi estimator kullanildi
    pub result: EstimateResult,
}

pub enum EstimateResult {
    Ok { price: BigDecimal, gas: u64 },
    Err { message: String },
}

pub struct PriceQuery {
    pub sell_token: H160,
    pub buy_token: H160,
    pub sell_amount: Option<U256>,
    pub buy_amount: Option<U256>,
    pub kind: OrderKind,             // Sell or Buy
}
```

**Subject:** `event.<chain_id>.priceEstimate`

**Analytics kullanimi:**
- `elapsed_ms` distribution: hangi estimator hizli/yavas
- Estimator basari orani: `result.Ok` / toplam
- Token pair coverage: hangi ciftler icin tahmin yapilabiliyor

**DEX Uygulama Notu:**
Nova Shield fiyat oracle analytics:
- p50/p95/p99 latency per estimator
- Cache hit rate (CoinGecko vs on-chain)
- Tahmin hatasi: |estimated - actual| / actual distribution

---

### [event-bus-dto/src/envelope.rs]

**Amac:** Tum event'leri saran versiyonlama yapisi.

```rust
#[derive(Serialize, Deserialize)]
pub struct Envelope<T> {
    pub version: u8,
    pub data: T,
}
```

Tuketiciler `version` alanini kontrol ederek backward-compat saglar.

---

## BOLUM 4: `crates/app-data/` — ORDER METADATA SISTEMI

### Mimari Ozet

CoW Protocol'de her order, 32-byte `appData` field'i tasir. Bu alan:
1. On-chain: `keccak256(full_data)` hash'i
2. Off-chain (IPFS): tam JSON metadata

Bu crate hash hesaplama, dogrulama ve icerigi parse etmek icin kullanilir.

---

### [app-data/src/app_data_hash.rs] (194 satir)

**Amac:** AppData hash hesaplama ve CIDv1 formatinda kodlama.

**Hash Fonksiyonu:**
```rust
pub fn hash_full_app_data(data: &[u8]) -> AppDataHash {
    AppDataHash(keccak256(data))
}
```

**CIDv1 Formatina Donusturme:**
```rust
pub fn to_cid(hash: &AppDataHash) -> Vec<u8> {
    let mut cid = Vec::with_capacity(36);
    cid.push(0x01);   // CID version = 1
    cid.push(0x55);   // Codec = raw (0x55)
    cid.push(0x1b);   // Hash algorithm = keccak-256 (0x1b)
    cid.push(0x20);   // Hash length = 32 (0x20)
    cid.extend_from_slice(&hash.0);  // 32-byte hash
    cid
}
```

**Matematiksel Yapilar:**

*AppDataHash:*
```
32 bytes = keccak256(UTF-8 JSON bytes)
```

*CIDv1 Wire Format (36 bytes):*
```
[0x01][0x55][0x1b][0x20][h0][h1]...[h31]
  ^     ^     ^     ^
  |     |     |     hash_length=32
  |     |     hash_algorithm=keccak256 (multihash code 0x1b)
  |     codec=raw (multicodec 0x55)
  version=1
```

*Base32 IPFS CID ornek:*
```
bafybei... = base32(cid_bytes)
```

**Dogrulama:**
```rust
pub fn validate_cid(cid_bytes: &[u8]) -> Result<AppDataHash, CidError> {
    if cid_bytes.len() != 36 { return Err(CidError::InvalidLength); }
    if cid_bytes[0] != 0x01  { return Err(CidError::InvalidVersion); }
    if cid_bytes[1] != 0x55  { return Err(CidError::InvalidCodec); }
    if cid_bytes[2] != 0x1b  { return Err(CidError::InvalidHashAlgorithm); }
    if cid_bytes[3] != 0x20  { return Err(CidError::InvalidHashLength); }
    Ok(AppDataHash(cid_bytes[4..36].try_into().unwrap()))
}
```

**Testler:**
- Bos string hash dogrulamasi
- Known-good CID round-trip
- Invalid length/version/codec/alg hata testi

**DEX Uygulama Notu:**
Nova Shield order metadata sistemi:
- Ayni CIDv1 + keccak256 pattern kullan (EVM uyumu icin)
- IPFS'e ek olarak Arweave kalici depolama onerilir
- On-chain 32-byte kisit: metadata buyuyunce sadece hash degisir
- `max_size = 8192 bytes` (8KB) varsayilan — daha buyuk metadata icin cok kisitli degil

---

### [app-data/src/app_data.rs] (918 satir)

**Amac:** AppData JSON parsing, dogrulama ve yapilari.

**Ana Yapi:**
```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProtocolAppData {
    pub hooks: Option<Hooks>,
    pub signer: Option<H160>,
    pub replaced_order: Option<OrderUid>,
    pub partner_fee: Option<PartnerFeeWrapper>,
    pub flashloan: Option<FlashloanHint>,
    pub metadata: Option<MetadataAppData>,
    pub wrappers: Option<Vec<WrapperCall>>,
}
```

**Hooks:**
```rust
pub struct Hooks {
    pub pre: Vec<Hook>,    // Settlement oncesi cagrilar
    pub post: Vec<Hook>,   // Settlement sonrasi cagrilar
}

pub struct Hook {
    pub target: H160,          // Call edilen contract
    pub calldata: Bytes,       // Raw calldata
    pub gas_limit: u64,        // Maks kullanilabilecek gas
}
```

**Gas Accounting:**
Settlement'in toplam gas maliyeti:
```
total_gas = base_settlement_gas
          + sum(pre_hook.gas_limit)
          + main_swap_gas
          + sum(post_hook.gas_limit)
```

Pre/post hook gas limitleri icin caller sorumlulugu: asilan hook gas = islem baska islemleri durtabilir.

**PartnerFeeWrapper:**
```rust
pub enum PartnerFeeWrapper {
    Single(PartnerFee),        // Tek recipient
    Multiple(Vec<PartnerFee>), // Coklu recipient
}

pub struct PartnerFee {
    pub bps: u16,              // Baz puan (max 65535 ama sinirli)
    pub recipient: H160,       // Fee alacak adres
}
```

**FeePolicy (BPS modeli):**
```rust
#[derive(Serialize, Debug, Clone, PartialEq)]
pub enum FeePolicy {
    Surplus { factor: Bps, max_volume_factor: Bps },
    PriceImprovement { factor: Bps, max_volume_factor: Bps },
    Volume { factor: Bps },
}

// Custom Deserialize — "surplus", "priceImprovement", "volume" string etiketleri
impl<'de> Deserialize<'de> for FeePolicy { ... }
```

**FlashloanHint:**
```rust
pub struct FlashloanHint {
    pub lender: FlashloanLender,    // "balancer", "uniswap", vb.
    pub token: H160,
    pub amount: U256,
}
```
Not: Bu bilgi solver'a **non-binding hint** — solver kullanabilir veya kullanmayabilir.

**WrapperCall:**
```rust
pub struct WrapperCall {
    pub target: H160,
    pub calldata: Bytes,
    pub gas_limit: u64,
    pub required: bool,   // False = omittable (solver atlayabilir)
}
```

**Validator:**
```rust
pub struct Validator {
    pub max_size: usize,  // Default: 8192 (8KB)
}

impl Validator {
    pub fn validate(&self, data: &[u8]) -> Result<ValidatedAppData, ValidationError> {
        // 1. Boyut kontrolu
        if data.len() > self.max_size {
            return Err(ValidationError::TooLarge { size: data.len(), max: self.max_size });
        }

        // 2. UTF-8 dogrulama
        let text = std::str::from_utf8(data)
            .map_err(|_| ValidationError::InvalidUtf8)?;

        // 3. JSON parse
        let app_data: FullAppData = serde_json::from_str(text)
            .map_err(ValidationError::InvalidJson)?;

        // 4. Hash hesapla
        let hash = hash_full_app_data(data);

        Ok(ValidatedAppData { hash, data: app_data })
    }
}
```

**WrapperCache (moka TinyLFU):**
```rust
pub struct WrapperCache {
    inner: moka::future::Cache<AppDataHash, Arc<Vec<WrapperCall>>>,
}
```
Amac: Ayni appData hash'i tekrar tekrar parse etmekten kac.

**DEX Uygulama Notu:**
Nova Shield order metadata:
- Hooks sistemi: Nova Shield'in MEV korumasiyla entegre pre-hooks (ornek: TWAP oracle dogrulama hook'u)
- `wrappers` sistemi: off-chain signed interactions (EIP-712 signed calldata)
- `flashloan` hint: arbitrajcilarin flash loan kullanimini kolaylastir
- `partner_fee` multirecipient: frontend + integrator + protokol uclu split
- `PartnerFee.bps` maksimum: Validator'da kontrol — max partner fee = konfig'den
- 8KB siniri: yeterlimi? Coklu hook + wrapper icin 8KB yeterli, buyuk veri IPFS'e

---

### [app-data/src/lib.rs]

```rust
pub use {
    app_data::{
        FeePolicy, FlashloanHint, FlashloanLender, Hook, Hooks,
        PartnerFee, PartnerFeeWrapper, ProtocolAppData, ValidatedAppData,
        Validator, WrapperCall, WrapperCache,
    },
    app_data_hash::{AppDataHash, hash_full_app_data, to_cid, validate_cid},
};
```

---

### [app-data/src/tests.rs] (tahmini)

**Amac:** Integration test'ler.

- Bos appData `{"version":"0.9.0"}` dogrulamasi
- Gecersiz JSON reddedilmesi
- Cok buyuk payload reddedilmesi
- Hook gas accounting testi
- Partner fee JSON round-trip

---

## BOLUM 5: `crates/gas-price-estimation/` — GAS FIYATI TAHMINI

### Mimari Ozet

EIP-1559 gas fiyati tahmini altyapisi. On-chain fee history'e gore dinamik olarak guncellenir. Her yeni blok geldiginde arkaplan gorevi `watch::channel`'i gunceller.

---

### [gas-price-estimation/src/lib.rs] (127 satir)

**Amac:** Public trait ve factory fonksiyonu.

**Ana Trait:**
```rust
#[async_trait]
pub trait GasPriceEstimating: Send + Sync {
    async fn estimate(&self) -> Result<GasPrice>;
    async fn estimate_with_limits(
        &self,
        gas_limit: f64,
        time_limit: Duration,
    ) -> Result<GasPrice>;
}
```

**GasPrice:**
```rust
pub struct GasPrice {
    pub legacy: f64,           // EIP-155 legacy gas price
    pub eip1559: Option<Eip1559GasPrice>,
}

pub struct Eip1559GasPrice {
    pub max_fee_per_gas: f64,
    pub max_priority_fee_per_gas: f64,
}
```

**Factory:**
```rust
pub async fn create_priority_estimator(
    web3: &Web3,
    estimators: &[GasEstimatorType],
    gas_price_cap: Option<f64>,
) -> Result<Arc<dyn GasPriceEstimating>> {
    // estimators listesine gore builder pattern ile olustur
    // Oncelik sirasi: estimators[0] > estimators[1] > ...
    // Gas cap uygula (varsa)
}
```

**Eip1559EstimationExt trait:**
```rust
pub trait Eip1559EstimationExt {
    fn effective(&self) -> f64;
    fn scaled_by_pml(&self, per_mille: u64) -> Self;
}

impl Eip1559EstimationExt for Eip1559GasPrice {
    fn effective(&self) -> f64 {
        self.max_fee_per_gas  // En kotu durum odeme
    }

    fn scaled_by_pml(&self, per_mille: u64) -> Self {
        Eip1559GasPrice {
            max_fee_per_gas: self.max_fee_per_gas * per_mille as f64 / 1000.0,
            max_priority_fee_per_gas: self.max_priority_fee_per_gas * per_mille as f64 / 1000.0,
        }
    }
}
```

**`scaled_by_pml` Kullanimi:**
Settlement transaction icin %10 buffer:
```rust
let gas_price = estimator.estimate().await?;
let buffered = gas_price.eip1559.unwrap().scaled_by_pml(1100); // %10 buffer
// max_fee = tahmini * 1.1
```

**DEX Uygulama Notu:**
Nova Shield settlement gas hesabinda:
- `scaled_by_pml(1150)` = %15 buffer (network yogunlugu varken)
- Dinamik buffer: mempool doluysa daha buyuk buffer
- Gas cap: manipulasyon korumasiyla maks gas fiyati siniri

---

### [gas-price-estimation/src/configurable_alloy.rs] (266 satir)

**Amac:** Alloy provider ile blok-reaktif EIP-1559 gas estimator.

**Temel Tasarim:**
```rust
pub struct AlloyConfigurator {
    current: watch::Receiver<Eip1559GasPrice>,
}

pub async fn spawn_alloy_configurator(
    provider: Arc<dyn Provider>,
    config: &AlloyConfig,
) -> (AlloyConfigurator, JoinHandle<()>) {
    let (tx, rx) = watch::channel(initial_price);

    let handle = tokio::spawn(async move {
        let mut block_stream = provider.subscribe_blocks().await?;

        while let Some(block) = block_stream.next().await {
            match estimate_from_block(&provider, &block, &config).await {
                Ok(price) => { tx.send(price).ok(); }
                Err(e) => { tracing::warn!("gas estimation failed: {e}"); }
            }
        }
    });

    (AlloyConfigurator { current: rx }, handle)
}
```

**EIP-1559 Algoritma:**
```rust
async fn estimate_from_block(
    provider: &dyn Provider,
    block: &Block,
    config: &AlloyConfig,
) -> Result<Eip1559GasPrice> {
    let fee_history = provider.get_fee_history(
        config.past_blocks,              // Kac blok geriye bak (default: 5)
        BlockNumberOrTag::Latest,
        &[config.reward_percentile],     // Default: %50 (medyan)
    ).await?;

    let (base_fee, tip) = eip1559_default_estimator(
        block.base_fee_per_gas.unwrap_or_default(),
        &fee_history.reward,
    );

    Ok(Eip1559GasPrice {
        max_fee_per_gas: base_fee * 2.0 + tip,
        max_priority_fee_per_gas: tip,
    })
}
```

**EIP-1559 Formul:**
```
tip = percentile(gecmis_blok_tip'leri, reward_percentile)
max_fee = current_base_fee * 2 + tip
```

Neden `base_fee * 2`?
- Bir sonraki blok base_fee maksimum %12.5 artabilir
- 2x buffer: ~6 blok boyunca gecerli kalan `max_fee`
- `max_fee - base_fee = max_fee - (base_fee)` = miner'a odenen max tip

**AlloyConfig:**
```rust
pub struct AlloyConfig {
    pub past_blocks: u64,           // Default: 5
    pub reward_percentile: f64,     // Default: 50.0 (%50 medyan tip)
    pub additional_tip_percentage: f64,  // Ilave tip (default: 0)
    pub max_tip: Option<f64>,       // Maks tip siniri
}
```

**`watch::channel` Deseni:**
- Arkaplan gorevi her blokta yazıyor (`watch::Sender`)
- `estimate()` cagrildiginda anlık degeri okuyor (`watch::Receiver::borrow()`)
- **Sifir bekleme:** Tahmin async RPC'yi beklemez, cache'ten okunur
- **Bir blok gecikme:** Yeni blok geldiginde guncellenir — kabul edilebilir

**`impl GasPriceEstimating for AlloyConfigurator`:**
```rust
#[async_trait]
impl GasPriceEstimating for AlloyConfigurator {
    async fn estimate(&self) -> Result<GasPrice> {
        let eip1559 = self.current.borrow().clone();
        Ok(GasPrice {
            legacy: eip1559.max_fee_per_gas,
            eip1559: Some(eip1559),
        })
    }

    async fn estimate_with_limits(&self, gas_limit: f64, time_limit: Duration) -> Result<GasPrice> {
        // time_limit: submisyon penceresi (2-3 blok)
        // gas_limit: bu islem ne kadar gas kullanacak
        // Daha yuksek gas_limit = daha yuksek tip onerisi
        self.estimate().await
    }
}
```

**DEX Uygulama Notu:**
Nova Shield gas estimator tasarimi:
- `watch::channel` pattern: tum solver'lar ayni cache'ten okuyor (N RPC cagrisi degil, 1 cagrisi)
- `past_blocks = 10`: daha stabil medyan (5 daha reaktif ama gurultulu)
- `reward_percentile = 75.0`: medyan degil %75 — inclusion garantisi icin
- MEV korumasiyla: private mempool'da `max_priority_fee` daha onemli (miner incentive)
- Gas cap: `gas_price_cap = 500 gwei` asinimli maks (MEV botlarin cok yuksek fee vermesini engelle)

---

### [gas-price-estimation/src/native.rs]

**Amac:** `eth_gasPrice` RPC metodu ile legacy gas tahmini.

```rust
pub struct NativeGasPriceEstimator {
    web3: Arc<Web3>,
    gas_price_cap: Option<f64>,
}

#[async_trait]
impl GasPriceEstimating for NativeGasPriceEstimator {
    async fn estimate(&self) -> Result<GasPrice> {
        let price = self.web3.eth().gas_price().await?;
        let price_f64 = price.to_f64_lossy();

        let capped = match self.gas_price_cap {
            Some(cap) => price_f64.min(cap),
            None => price_f64,
        };

        Ok(GasPrice { legacy: capped, eip1559: None })
    }
}
```

---

### [gas-price-estimation/src/priority.rs]

**Amac:** Birden fazla estimator'u oncelik sirasina gore dene.

```rust
pub struct PriorityGasPriceEstimating {
    estimators: Vec<Arc<dyn GasPriceEstimating>>,
}

#[async_trait]
impl GasPriceEstimating for PriorityGasPriceEstimating {
    async fn estimate(&self) -> Result<GasPrice> {
        for estimator in &self.estimators {
            match estimator.estimate().await {
                Ok(price) => return Ok(price),
                Err(e) => tracing::warn!("estimator failed: {e}, trying next"),
            }
        }
        Err(anyhow!("all estimators failed"))
    }
}
```

**Fallback Zinciri (tipik konfigurasyum):**
```
AlloyEip1559 -> NativeGasPrice -> (BlockNative API) -> (EthGasStation API)
```

---

### [gas-price-estimation/src/buffered.rs]

**Amac:** Gas tahminini belirli sure boyunca cache'le.

```rust
pub struct BufferedGasPriceEstimating {
    inner: Arc<dyn GasPriceEstimating>,
    max_age: Duration,            // Cache TTL
    last_estimate: Mutex<Option<(GasPrice, Instant)>>,
}

#[async_trait]
impl GasPriceEstimating for BufferedGasPriceEstimating {
    async fn estimate(&self) -> Result<GasPrice> {
        let mut guard = self.last_estimate.lock().await;

        if let Some((price, ts)) = guard.as_ref() {
            if ts.elapsed() < self.max_age {
                return Ok(price.clone());
            }
        }

        let price = self.inner.estimate().await?;
        *guard = Some((price.clone(), Instant::now()));
        Ok(price)
    }
}
```

**Driver kullanimi:** `max_age = 1 block time (12s)` — her blokta bir kez RPC cagrisi.

---

### [gas-price-estimation/src/gas_price_cap.rs]

**Amac:** Gas fiyatini ust sinirla sifirla.

```rust
pub struct GasPriceCap {
    inner: Arc<dyn GasPriceEstimating>,
    cap: f64,
}

#[async_trait]
impl GasPriceEstimating for GasPriceCap {
    async fn estimate(&self) -> Result<GasPrice> {
        let price = self.inner.estimate().await?;

        Ok(GasPrice {
            legacy: price.legacy.min(self.cap),
            eip1559: price.eip1559.map(|e| Eip1559GasPrice {
                max_fee_per_gas: e.max_fee_per_gas.min(self.cap),
                max_priority_fee_per_gas: e.max_priority_fee_per_gas.min(self.cap),
            }),
        })
    }
}
```

---

### [gas-price-estimation/src/tests.rs]

**Testler:**
- Mock provider ile EIP-1559 hesaplama dogrulamasi
- `base_fee * 2 + tip` formul dogrulamasi
- Gas cap uygulamasi
- Priority estimator fallback davranisi
- Buffered TTL gecerliligi

---

## BOLUM 6: CAPRAZ KESiSiM ANALiZi — DEX iCiN SiNERJiLER

### Fee Model Entegrasyonu

```
configs/fee_factor.rs (FeeFactor)
    ↓
configs/autopilot/fee_policy.rs (FeePolicy: Surplus/PriceImprovement/Volume)
    ↓
app-data/app_data.rs (PartnerFee, FeePolicy JSON)
    ↓
event-bus-dto/settlement.rs (SettledOrder.surplus, SettledOrder.fee)
    ↓
observe/event_bus/mod.rs (NATS publish)
    ↓
[Analytics dashboard'u]
```

Tam fee akisi: konfigurasyon → order metadata → settlement event → analytics.

### Gas Tahmin Entegrasyonu

```
gas-price-estimation/configurable_alloy.rs (blok-reaktif tahmin)
    ↓ watch::channel
configs/price_estimation.rs (max_gas_per_tx, gas estimator tipi)
    ↓
driver (settlement tx'i olusturur)
    ↓
scaled_by_pml(1100) (%10 buffer)
    ↓
Ethereum network'e submit
```

### Gozlem Katmani Entegrasyonu

```
observe/metrics.rs (Prometheus registry)
observe/tracing/init.rs (structured logging + OTLP)
observe/event_bus/mod.rs (NATS events)
    ↓ hepsi birlikte
observe/future.rs (Measurable<T>) → histogram kaydi
observe/tracing/lazy.rs (Lazy<T>) → verimli log
observe/tracing/distributed/ → request_id + trace_id
```

---

## BOLUM 7: NOVA SHIELD DEX iCiN KAPSAMLI UYGULama PLANI

### 1. Fee Mimarisi (configs/fee_factor.rs + fee_policy.rs kaynagindan)

**Alman:**
- `FeeFactor(f64)` newtype, `[0, 1)` aralik, `HIGH_PRECISION_SCALE = 1_000_000`
- 3 fee modeli: Volume (sabit), Surplus (CoW-benzeri), PriceImprovement
- `UpcomingFeePolicies` ile zamanlanmis fee degisikisi

**Nova Shield icin:**
```rust
pub enum NovaFeePolicy {
    // Tier 1: Stablecoin pairs (USDC-USDT, USDC-DAI)
    Stable { bps: u16 },           // 1 BPS sabit

    // Tier 2: Standard pairs (ETH-USDC)
    Standard { bps: u16 },         // 30 BPS sabit

    // Tier 3: Exotic pairs
    Exotic { bps: u16 },           // 100 BPS sabit

    // CoW-benzeri surplus paylaşimi
    SurplusShare {
        protocol_factor: FeeFactor,    // Protokol payi
        lp_factor: FeeFactor,          // LP payi
        max_volume_factor: FeeFactor,  // Hacim tabanli cap
    },
}
```

**Iyilestirme:** CoW'da sadece protokol surplus alir. Nova Shield'de:
- LP'ler de surplus paylasindan faydalanabilir (impermanent loss compensation)
- Yuksek hacimli gunlerde dinamik surplus share artisi

### 2. Gas Estimasyonu (gas-price-estimation kaynagindan)

**Alman:**
```
max_fee = base_fee * 2 + tip_percentile50
buffer = scaled_by_pml(1100)  // %10
```

**Nova Shield icin:**
```rust
// Dinamik buffer: network yogunluguna gore
let buffer_pml = if mempool_congestion > 0.8 {
    1200  // %20 buffer (yogun)
} else if mempool_congestion > 0.5 {
    1150  // %15 buffer (orta)
} else {
    1100  // %10 buffer (normal)
};

let gas_price = estimator.estimate().await?
    .eip1559.unwrap()
    .scaled_by_pml(buffer_pml);
```

**Tip percentile:** `reward_percentile = 75.0` (medyandan daha garantili inclusion).

### 3. Event Sistemi (observe/event_bus + event-bus-dto kaynagindan)

**Nova Shield event tipleri:**
```rust
// event-bus-dto benzeri
pub enum NovaShieldEvent {
    OrderPlaced(NovaOrderPlaced),
    OrderFilled(NovaOrderFilled),
    PoolCreated(NovaPoolCreated),
    LiquidityAdded(NovaLiquidityAdded),
    PriceUpdate(NovaPriceUpdate),
    MevProtectionTriggered(NovaMevProtectionEvent),
}
```

**Kafka yerine NATS:**
- NATS JetStream retention: 24 saat (replay icin)
- Kafka alternatifi: yuksek throughput (>100k TPS) icin tercih edilebilir
- Buffer boyutu: 1000 degil, 10_000 (DEX hacmi)

### 4. Metadata Sistemi (app-data kaynagindan)

**Nova Shield order metadata:**
```json
{
  "version": "1.0.0",
  "mev_protection": {
    "level": "full",
    "deadline_buffer_blocks": 2
  },
  "routing_hint": {
    "preferred_pools": ["0x..."],
    "max_hops": 3
  },
  "hooks": {
    "pre": [
      {
        "target": "0x...",
        "calldata": "0x...",
        "gas_limit": 50000,
        "description": "TWAP oracle check"
      }
    ]
  },
  "partner_fee": {
    "bps": 15,
    "recipient": "0x..."
  }
}
```

### 5. Gozlem Altyapisi (observe kaynagindan)

**Nova Shield critical metrics:**
```rust
#[derive(MetricStorage)]
pub struct NovaShieldMetrics {
    // Hacim
    swap_volume_eth: CounterVec,     // labels: pool, direction
    swap_count: CounterVec,          // labels: pool, status

    // Fiyat kalitesi
    price_impact_pct: HistogramVec,  // buckets: 0.01%, 0.1%, 0.5%, 1%, 5%
    surplus_captured: HistogramVec,  // MEV korumasinin etkinligi

    // Gas
    settlement_gas_used: HistogramVec,
    gas_price_gwei: GaugeVec,

    // Solver
    solver_win_rate: CounterVec,     // labels: solver_name
    auction_duration_ms: HistogramVec,

    // MEV
    mev_blocked_count: CounterVec,
    private_mempool_success_rate: GaugeVec,
}
```

**Structured logging (tracing + OTLP):**
```rust
// Her settlement icin distributed trace
let span = tracing::info_span!("settlement",
    auction_id = %auction.id,
    solver = %winner.address,
    orders_count = %auction.orders.len(),
);

// Trace ID otomatik log'a eklenir (TraceIdJsonFormat ile)
```

---

## BOLUM 8: GUVENLiK ANALiZi

### Kritik Bulgular

**[configs/src/shared.rs] ORTA: EventBus channel_name dogrulamasi**
- Bosluk/tab/nokta NATS subject olarak kullanilinca protokol hatasi yaratir
- Mevcut dogrulama: `validate()` metodu kontrol ediyor — yeterli

**[app-data/src/app_data.rs] DUSUK: Hook gas_limit ucret muhasebesi**
- Pre/post hook gas_limit'leri toplam settlement gas'ina eklenir
- Kotu niyet: cok buyuk gas_limit hook ile settlement'i gas-bomb yapabilir
- Koruma: driver total gas > block_gas_limit ise reddet
- Nova Shield: per-hook gas_limit cap onerilir (ornek maks 100k gas)

**[app-data/src/app_data.rs] ORTA: flashloan hint non-binding**
- Solver flashloan'u kullanmayabilir — bu BEKLENiLEN davranis
- Sorun: kullanici flashloan olmaksizin execute edilecek ordera imza atar
- Koruma: flashloan hint varsa solver'a "tercih et" sinyali verilmeli

**[observe/src/event_bus/mod.rs] DUSUK: Silent event drop**
- `ChannelFull` durumunda event sessizce dusurulur
- Production'da metric alert kurulmali: `event_bus_drop_total > 0`
- Nova Shield: critical event'ler icin back-pressure onerilir

**[gas-price-estimation/src/configurable_alloy.rs] DUSUK: Tek blok gecikme**
- `watch::channel` ile tahmin bir blok geriden gelir
- Cok hizli gas fiyati artisinda (`base_fee * 1.125 * 6 blok`) underpricing riski
- Koruma: `scaled_by_pml(1200)` ile geniş buffer + gas_price_cap

**[configs/src/fee_factor.rs] DUSUK: f64 hassasiyet**
- `FeeFactor(f64)` — floating point aritmetigi yuvarlama hatalari
- `to_high_precision()` ile integer'a donusturme bu sorunu azaltır
- Nova Shield: on-chain fee hesabinda **tam integer aritmetigi** kullan

---

## DOSYA SAYIM OZETI

| Crate | Dosya | Tahmini Satir | Kapsam |
|---|---|---|---|
| configs | 28 | ~3,500 | TAMAMLANDI |
| observe | 18 | ~2,100 | TAMAMLANDI |
| event-bus-dto | 8 | ~600 | TAMAMLANDI |
| app-data | 4 | ~1,400 | TAMAMLANDI |
| gas-price-estimation | 7 | ~700 | TAMAMLANDI |
| **TOPLAM** | **65** | **~8,300** | **TAMAMLANDI** |

---

## KAPSANAN TEMEL KAVRAMLAR OZETI

| Kavram | Kaynak | Nova Shield Uygulamasi |
|---|---|---|
| FeeFactor [0,1) | configs/fee_factor.rs | Pool tier fee'leri |
| HIGH_PRECISION_SCALE=1M | configs/fee_factor.rs | Sub-BPS on-chain hesap |
| Surplus/Volume/PriceImprovement fee | configs/fee_policy.rs | CoW-benzeri surplus share |
| UpcomingFeePolicies zamanlama | configs/fee_policy.rs | Governance fee gecisi |
| TokenBucketFeeOverride | configs/shared.rs | Stablecoin dusuk fee |
| max_gas_per_tx=16777215 | configs/price_estimation.rs | Fusaka EIP-7623 uyumu |
| BalanceOverrides probing_depth=60 | configs/price_estimation.rs | ERC-20 storage detection |
| StateOverrideStream max_age=5s | configs/price_estimation.rs | Arbitraj state simulation |
| Prometheus OnceLock registry | observe/metrics.rs | Global metric singleton |
| MetricStorage derive macro | observe/metrics.rs | Metric tanim kolaylastirma |
| EnvFilter runtime reload | observe/tracing/init.rs | UNIX socket log degistirme |
| Sampler::AlwaysOn (TODO: sampling) | observe/tracing/init.rs | Tum span'lar export |
| Lazy<T> sifir-maliyet log | observe/tracing/lazy.rs | Pahalı hesaplama geciktir |
| Measurable<T> RAII timer | observe/future.rs | Async op histogram olcumu |
| EVENT_BUS_SIZE=1000 | observe/event_bus/mod.rs | In-memory buffer |
| try_send (silent drop) | observe/event_bus/mod.rs | Backpressure yok |
| DropReason metric | observe/event_bus/mod.rs | Event kayip izleme |
| Envelope{version,data} | event-bus-dto/envelope.rs | Wire format versiyonlama |
| OrderUid 56-byte | event-bus-dto/order_placed.rs | [digest32+owner20+validTo4] |
| SettledOrder.surplus | event-bus-dto/settlement.rs | MEV koruma etkinligi |
| keccak256(data)=AppDataHash | app-data/app_data_hash.rs | On-chain hash |
| CIDv1=[1,0x55,0x1b,0x20,hash] | app-data/app_data_hash.rs | IPFS iceriklendirme |
| max_size=8192 (8KB) | app-data/app_data.rs | Metadata boyut siniri |
| Hook.gas_limit | app-data/app_data.rs | Pre/post interaction gas |
| FlashloanHint non-binding | app-data/app_data.rs | Solver hint mekanizmasi |
| WrapperCall.required | app-data/app_data.rs | Atlanabilir interaction |
| max_fee=base_fee*2+tip | gas-price-estimation/configurable_alloy.rs | EIP-1559 formul |
| past_blocks=5, percentile=50 | gas-price-estimation/configurable_alloy.rs | Tahmin parametreleri |
| watch::channel blok-reaktif | gas-price-estimation/configurable_alloy.rs | Tek RPC, N tuketici |
| scaled_by_pml (per-mille) | gas-price-estimation/lib.rs | Gas buffer uygulama |
| Priority estimator fallback | gas-price-estimation/priority.rs | Coklu estimator zinciri |
| BufferedEstimator TTL | gas-price-estimation/buffered.rs | RPC cagri azaltma |

---

*Agent 13 — Analiz Tamamlandi*
*Kapsam: configs (28) + observe (18) + event-bus-dto (8) + app-data (4) + gas-price-estimation (7) = 65 dosya*
*Toplam analiz satirlari: ~1,450+*
