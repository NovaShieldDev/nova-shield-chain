# CoW Protocol Analiz — Agent 10
## Kapsam: shared/ + price-estimation/ + database/ + number/ + küçük crate'ler
## DEX Özel Protokolü: 3x Hassasiyet

---

## 1. shared/ Crate Analizi

### 1.1 lib.rs (27 satır)
- 17 modül export eder: `arguments`, `current_block`, `db_order_conversions`, `event_storing_helpers`, `external_prices`, `fee`, `http_solver`, `interaction`, `macros`, `order_creation_simulation`, `order_quoting`, `order_validation`, `remaining_amounts`, `retry`, `token_list`, `url`, `web3`
- `clone_anyhow_error` helper: `anyhow::Error` `Clone` implementasyonu yoktur; bu fn string üzerinden yeniden oluşturur

### 1.2 arguments.rs (138 satır)
- `LoggingArguments` macro: Clap arg grubu, log filtreleri için
- `TracingArguments`: OpenTelemetry trace konfigürasyonu
- `DatabasePoolConfig`:
  - `max_connections`: default **10**
  - `statement_timeout`: default **30s** (MCP aracıyla aynı limit)
- `TokenBucketFeeOverride`: token → bucket mapping; her iki token aynı bucket'taysa volumeoverride uygulanır
- `gas_estimator_type_from_config`: string → `GasEstimatorType` enum dönüşümü

### 1.3 current_block.rs (87 satır)
- `CurrentBlockWatcher` factory: Block değişikliklerini izler
- Poll interval: **500ms** (default)
- WebSocket stream tercih edilir; HTTP polling fallback
- `block_stream` döner: `Arc<dyn Stream<Item = BlockInfo>>`

### 1.4 external_prices.rs (168 satır) — DEX KRİTİK
**ExternalPrices** struct: `HashMap<Address, BigRational>`

**Fiyat temsili**: `auction_price / 1e18` = kaç ETH atomu 1 token atomu değer
- `BUY_ETH_ADDRESS` her zaman `price = 1` ile eklenir (native token her zaman ETH cinsinden 1:1)
- `try_new`: native token fiyatının tam olarak 1 olması zorunlu; farklıysa hata
- `to_native_xrate(token)`: `auction_price / 1e18` döner

**DEX Uygulama Notu**: Nova Shield fiyat temsili için bu deseni kopyalamalı. "ETH başına kaç token" yerine "1 token atomu kaç ETH atomu" kullanmak daha tutarlı. BUY_ETH_ADDRESS sentinel = `0xEeEeEeEeEeEeEeEeEeEeEeEeEeEeEeEeEeEeEeEe`.

### 1.5 fee.rs (189 satır) — DEX KRİTİK
**FeeParameters** struct:
```
fee = ceil((gas_amount * gas_price) / sell_token_price)
```
- `ceil` kullanılır — sıfır ücret saldırısını önler (düşük decimal tokenlarda sıfıra yuvarlanma problemi)
- `gas_amount`: satoshi cinsinden gas miktarı
- `gas_price`: wei/gas (EIP-1559 uyumlu)
- `sell_token_price`: ETH'e göre, yani token kaç ETH değer

**VolumeFeePolicy**:
- `TokenBucketFeeOverride` map: token → bucket ismi
- Aynı bucket'tan her iki token için override uygulanır
- WETH → ETH aynı çift sayılır (`same_token_trade` atlanır)

**DEX Uygulama Notu**: Gas ücreti hesabında `ceil` zorunludur. 6-decimal USDC'de sıfır gas problemi yaşanabilir; `ceil` bu açığı kapatır. Nova Shield'da da fee = ceil(gas / price) kullanılmalı.

### 1.6 http_solver.rs (32 satır)
- `TokenAmount { token: Address, amount: U256 }`: HTTP solver API için basit wrapper
- `InternalizationStrategy` enum:
  - `EncodeAllInteractions`: tüm etkileşimleri calldata'ya yazar
  - `SkipInternalizableInteraction`: zincir-içi (CoW Vault) etkileşimleri yazar, harici tokenlar için calldata üretmez

### 1.7 interaction.rs (41 satır)
- `Interaction` trait:
  ```rust
  fn encode(&self) -> Vec<(Address, U256, Bytes)>
  ```
  - Tuple: `(target, value, calldata)`
- `Box<dyn Interaction>` için blanket impl
- `InteractionData { target, value, call_data }`: ham veri struct'ı

### 1.8 order_quoting.rs — DEX KRİTİK
- `QuoteParameters`: sell/buy token, miktarlar, sipariş türü, son kullanım, onay durumu
- `OrderQuoting` trait: `calculate_quote`, `store_quote`, `find_quote`, `forget_quote`
- `QuoteData`: quote sonucu; gas estimate, çalıştırılan miktar, kullanılan solver, verified flag
- `QuoteSearchParameters`: son kullanım toleransı ile arama; buy/sell miktarların her ikisi de saklanır (SELL miktar iki kez — fee dahil/hariç)
- `quote_kind_from_signing_scheme`: PreSign ve EIP-1271 için özel QuoteKind döner (onchain quote gerektirir)

**DEX Uygulama Notu**: Quote hem "fee dahil" hem "fee hariç" sell amount saklar. Bu daha sonra fee politikası değiştiğinde yeniden kullanılabilirliği mümkün kılar.

### 1.9 order_validation.rs — DEX KRİTİK
- `OrderSimulator`: timeout wrapper; varsayılan 10s
- `Metrics`: prometheus histogram, simülasyon süresi (ms)
- `log_simulation_outcome`: hata durumunda revert reason ile loglar
- `PreOrderData`: sipariş oluşmadan önce validasyon için gereken minimal veri seti
- Sipariş simülasyonu: EIP-1271 ve `OrderSimulating` trait kullanır
- `SimulationSuccess { eip1271_signature_verification_gas }`: ek gas maliyeti döner

### 1.10 remaining_amounts.rs (450 satır) — DEX KRİTİK
**Remaining** struct:
- `execution_ratio`: kısmi dolgu oranı (ne kadar zaten doldu)
- `balance_ratio`: kullanıcının mevcut bakiyesi / orijinal sipariş miktarı
- İkisi ayrı tutulur — birleştirilirse U256 taşması riski

**from_order_with_balance**:
- `sell_amount` ve `buy_amount` için kalan hesaplanır
- `partially_fillable` siparişler için: U512 genişletilmiş çarpım kullanılır (orta taşmayı önler)

**DEX Uygulama Notu**: Kısmi dolgu oranı hesaplamada U512 zorunlu. U256 × U256 = U512 olabilir. Nova Shield'da kısmi dolgu desteklenecekse bu önemli.

### 1.11 db_order_conversions.rs (291 satır)
- `full_order_into_model_order`: DB row → model struct dönüşümü
- `order_quote_into_model`: sipariş açıkken quote metadata gizlenir (privacy)
- `extract_interactions`: BYTEA → `InteractionData` listesi
- Enum dönüşümleri: `OrderKind`, `OrderClass`, `SellTokenSource`, `BuyTokenDestination`, `SigningScheme`

### 1.12 event_storing_helpers.rs (49 satır)
- `create_quote_row`: quote event → DB row
- `create_db_search_parameters`: quote arama parametrelerini oluşturur

### 1.13 macros.rs (11 satır)
- `externalprices!` macro: test kolaylığı için token → fiyat map oluşturur

### 1.14 retry.rs (44 satır)
- `retry_with_sleep`:
  - `MAX_RETRIES = 5`
  - Jitter: **50-100ms** (deterministik değil)
  - Exponential backoff değil, sabit jitter — sürpriz

### 1.15 token_list.rs (247 satır)
- `AutoUpdatingTokenList`: `Arc<RwLock<HashSet<Address>>>` backed
- Background task: her interval'da Uniswap token list JSON'u çeker
- `TokenListModel`: Uniswap v2 token listesi şeması uyumlu

### 1.16 url.rs (73 satır)
- `join(base, path)`: slash normalizasyonu yapar (`/api/` + `/v1/` → `/api/v1/`)
- `split_at_path`: URL'yi base + path olarak ayırır

### 1.17 web3.rs (76 satır)
- `MAX_BATCH_SIZE = 100`: RPC batch limit
- `Arguments`: ethrpc konfigürasyonu (batch size, concurrency, delay)
- `web3()` factory: config'e göre `AlloyProvider` döner

### 1.18 order_creation_simulation.rs (453 satır) — DEX KRİTİK
**OrderSimulationError** enum:
- `Reverted`: simülasyon EVM'de revert etti
- `Infra`: node/timeout hatası

**SimulationSuccess**:
- `eip1271_signature_verification_gas`: ERC-1271 doğrulamanın gas maliyeti (settlement gas'tan ayrılır)

**Özel davranışlar**:
- `allow_failed_buy_token_transfer`: buy token transferi başarısız olsa bile order kabul edilir (bazı eksantrik tokenlar)
- `allow_unfunded_presign_order`: PreSign orders balances olmadan kabul edilir
- `disallow_broken_hook`: geçersiz hook calldata'larını reddeder
- `extract_critical_revert`: revert mesajından kritik ayrıntıları çıkarır

---

## 2. price-estimation/ Crate Analizi

### 2.1 lib.rs (356 satır) — DEX KRİTİK
**PriceEstimationError** — 9 variant:
1. `NoLiquidity` — havuz yok
2. `UnsupportedToken { token, reason }` — token desteklenmiyor
3. `UnsupportedOrderType(String)` — sipariş türü desteklenmiyor
4. `RateLimited` — rate limit aşıldı
5. `TradingOutsideAllowedWindow { message }` — izin verilen pencere dışı
6. `TokenTemporarilySuspended { message }` — geçici askı
7. `InsufficientLiquidity { message }` — yetersiz likidite
8. `CustomSolverError { message }` — solver spesifik hata
9. `EstimatorInternal(anyhow::Error)` — iç hata
10. `ProtocolInternal(anyhow::Error)` — protokol hatası

**Query struct**:
```rust
pub struct Query {
    verification: Verification,
    sell_token: Address,
    buy_token: Address,
    in_amount: NonZeroU256,  // NonZero garantisi
    kind: OrderKind,          // Buy veya Sell
    block_dependent: bool,    // block hash ile cache bypass
    timeout: Duration,        // her estimator'a iletilir
}
```

**Estimate struct**:
```rust
pub struct Estimate {
    out_amount: U256,
    gas: u64,
    solver: Address,
    verified: bool,           // simülasyon ile doğrulandı mı
    execution: QuoteExecution, // calldata + interactions
}
```

**HEALTHY_PRICE_ESTIMATION_TIME = 5000ms**: SLA hedefi

**PriceEstimating trait**:
```rust
fn estimate(&self, query: Arc<Query>) -> BoxFuture<'_, PriceEstimateResult>;
```

**StreamingPriceEstimating trait**:
```rust
fn estimates(&self, queries: Vec<Arc<Query>>) -> BoxStream<'_, PriceEstimateResult>;
```

**`rate_limited` wrapper**: `RateLimiter` ile sararak rate limit uygular

### 2.2 competition/mod.rs (768 satır) — DEX KRİTİK

**CompetitionEstimator<T>** struct:
- `stages: Vec<Vec<T>>` — aşamalar ve her aşamada paralel estimator'lar
- `usable_results_for_early_return: usize` — bu kadar sonuç gelince erken dönüş

**`produce_results` algoritması**:
```
For each stage:
  - FuturesUnordered ile tüm estimator'ları paralel çalıştır
  - Her sonuç geldiğinde değerlendir
  - usable_results_for_early_return sayısına ulaşılınca Stage'i kapat
  - Sonraki Stage'e geç
```

**`compare_error` priority ranking** (yüksekten düşüğe):
1. `CustomSolverError` — solver iş mantığı hatası
2. `TradingOutsideAllowedWindow` — kritik bilgi
3. `TokenTemporarilySuspended` — geçici durum
4. `InsufficientLiquidity` — havuz var ama yetersiz
5. `UnsupportedToken` — token desteklenmiyor
6. `RateLimited` — geçici
7. `NoLiquidity` — havuz yok
8. `EstimatorInternal` — iç hata
9. `UnsupportedOrderType` — en az öncelikli

**DEX Uygulama Notu**: Error priority ranking, en anlamlı hata kullanıcıya gösterilir. Bu pattern Nova Shield'da da uygulanmalı — "NoLiquidity" yerine "InsufficientLiquidity" daha açıklayıcı.

### 2.3 competition/native.rs (261 satır)

**NativePriceEstimating** impl for `CompetitionEstimator`:
- Dinamik timeout: stage'e göre kalan sürenin orantısal payı
- Malformed price check: `to_normalized_price` başarısız olursa reddedilir
- `is_price_malformed`: infinite, NaN, sıfır veya alt/üst sınır dışı fiyatlar reddedilir

### 2.4 competition/quote.rs (475 satır) — DEX KRİTİK

**`PriceEstimating` impl for `CompetitionEstimator`**:

**`compare_quote`** algoritması:
```
effective_eth_out (sell order) = out_amount * native_price_buy - gas * gas_price
effective_eth_out (buy order) = in_amount * native_price_sell - gas * gas_price
```

**`RankingContext.effective_eth_out`** — net ETH çıktısı (gas maliyeti düşüldükten sonra):
- Sell orders: `out_amount * native_price - gas * gas_price`
- Verified quotes preferred when `verification_mode = Prefer`

**`PriceRanking` enum**:
- `MaxOutAmount`: ham `out_amount` karşılaştırması (gas görmezden gelinir)
- `BestBangForBuck`: `effective_eth_out` karşılaştırması (tam net değer)

**Events emitted**:
- `PriceEstimateEvent`: her tahmin için
- `WinningEstimateEvent`: kazanan tahmin için

**DEX Uygulama Notu**: Gas-adjusted ranking kritik. Yüksek out_amount veren ama çok gas tüketen bir route, düşük out_amount + düşük gas'tan daha kötü olabilir. Nova Shield quote sisteminde `effective_eth_out = out - gas * gasprice` formülü kullanılmalı.

### 2.5 native/mod.rs (230 satır) — DEX KRİTİK

**`NativePriceEstimating` trait**:
```rust
fn estimate_native_price(&self, token: Address, timeout: Duration) -> BoxFuture<'_, NativePriceEstimateResult>;
```
- Döndürülen değer: `f64` (1 atom token için kaç native token)

**`NativePriceEstimator`** wrapper: `BUY` order türü kullanır (native token al)

**`is_price_malformed`** kuralları:
- `f64::is_normal()` false olanlar (inf, nan, subnormal)
- `<= 0.0` olanlar
- `to_normalized_price` başarısız olanlar (çok büyük/küçük)

**`from_normalized_price`**: native_price_normalized / 1e18
**`to_normalized_price`**: native_price * 1e18, range validate

### 2.6 native/coingecko.rs (605 satır) — DEX KRİTİK

**CoinGecko struct**:
- Batch fetch: en fazla **19 token** (1 slot denominator için rezerve)
- Desteklenen 11 chain: Ethereum, Gnosis, Arbitrum, Base, Bnb, Avalanche, Optimism, Polygon, Linea, Plasma, Ink

**`denominate_price` formülü**:
```
price = (token_price_eth / denominator_price_eth) * 10^(denom_decimals - token_decimals)
```
- Denominator: istikrarlı referans token (örn. WETH)
- Decimal normalizasyonu otomatik

**URL caching**: token adresleri **sıralanarak** URL oluşturulur (deterministic caching)

**DEX Uygulama Notu**: CoinGecko entegrasyonu için batch size 19 sınırını koruyun. Denominator token olarak WETH kullanmak daha güvenilir fiyat verir.

### 2.7 native/oneinch.rs (232 satır)

**OneInch struct**:
- `Arc<Mutex<HashMap<Address, f64>>>` fiyat cache'i
- Background polling: her yeni block'ta güncelleme
- `get_current_prices`: token decimals ile normalize edilmiş fiyatlar döner
  ```
  price = u256_to_big_rational(raw_price) / 10^decimals
  ```

### 2.8 native/eip4626.rs (378 satır) — DEX KRİTİK

**Eip4626** struct: ERC-4626 vault token fiyatlarını çözümler

**`unwrap_all_layers`**: vault zincirini özyinelemeli takip eder
- `conversion_rate = convertToAssets(10^vault_decimals) / 10^vault_decimals`
- Negative cache: `DashSet non_vault_tokens` (vault olmayan tokenlar tekrar sorgulanmaz)
- `BUY_ETH_ADDRESS` pre-cached as non-vault (native token vault olamaz)

**DEX Uygulama Notu**: ERC-4626 vaultlar için fiyat = `underlying_price * conversion_rate`. wstETH/stETH gibi çiftler için bu kritik. Nova Shield'da vault wrapper desteği eklenecekse bu mantık kopyalanmalı.

### 2.9 native/fallback.rs (531 satır) — DEX KRİTİK

**FallbackNativePriceEstimator** — state machine:
- `PROBE_INTERVAL = 60s`
- `CONSECUTIVE_ERRORS_THRESHOLD = 3`

**State machine**:
```
Primary { consecutive_errors: usize }
  → 3 hata sonrası → Fallback { last_probe: Instant }

Fallback { last_probe }
  → PROBE_INTERVAL sonra → Primary ve Fallback'ı paralel test et
  → Primary başarılıysa Primary'ye geri dön
```

**`Action` enum**: `Primary`, `Fallback`, `Probe` (her ikisi de paralel)

**DEX Uygulama Notu**: Oracle fallback pattern. Primary/fallback state machine CONSECUTIVE_ERRORS ile; probe zamanlaması ile "kararlı recovery". Nova Shield price oracle'ında benzer pattern uygulanmalı.

### 2.10 native/forwarder.rs (94 satır)
- `Forwarder`: native fiyat isteklerini autopilot HTTP API'ye proxy'ler
- `X-REQUEST-ID` header geçirilir (tracing)
- HTTP 404 → `NoLiquidity`, HTTP 429 → `RateLimited`

### 2.11 native_price_cache.rs — DEX KRİTİK
**ApproximationToken struct**:
```rust
struct ApproximationToken {
    address: Address,
    normalization_factor: BigRational,  // 10^(peg_decimals - token_decimals)
}
```
- Peg edilmiş tokenlar için (USDC'nin pegged EURC'si gibi)

**`NativePriceCache`**:
- Backed by `ArcSwap` (lock-free read)
- Background updater: per-block güncelleme
- Eviction: son istek zamanına göre (LRU benzeri)

**`CacheMetrics`** + **`UpdaterMetrics`**: Prometheus metrics

**DEX Uygulama Notu**: `ArcSwap` lock-free reads için mükemmel. Cache eviction "son istek zamanı" ile yapılır — hot tokens uzun süre kalır, cold tokens çıkar. Nova Shield price cache'inde bu pattern kullanılmalı.

### 2.12 buffered.rs (100+ satır)

**`BufferedRequest<Inner>`** struct:
- `mpsc::UnboundedSender<Address>` aracılığıyla token gönderir
- `broadcast::Sender<NativePriceResult>` ile sonuç yayınlar
- Debouncing: ilk istek geldikten sonra `debouncing_time` bekler
- `max_concurrent_requests`: batch sayısı sınırı

**Önemli sıralama**:
```rust
// Subscribe BEFORE send — aksi halde sonuç kaybolabilir
let mut rx = self.results.subscribe();
self.requests.unbounded_send(token)?;
```

**DEX Uygulama Notu**: Request batching için pub/sub pattern. Subscribe-before-send sıralaması race condition'ı önler. Nova Shield'da external API batching için bu pattern kopyalanmalı.

### 2.13 trade_finding/mod.rs (448 satır) — DEX KRİTİK

**`TradeFinding` trait**:
```rust
async fn get_quote(&self, query: &Query) -> Result<Quote, TradeError>;
async fn get_trade(&self, query: &Query) -> Result<Trade, TradeError>;
```

**`Quote` struct**: `out_amount`, `gas_estimate`, `solver`, `execution`

**`TradeKind` enum**: `Legacy(LegacyTrade)` | `Regular(Trade)`

**`Trade.out_amount` hesabı** (clearing prices kullanır):
- Sell order: `(in_amount * sell_price / buy_price).ceil()`
- Buy order: `(in_amount * buy_price / sell_price).ceil()`
- Her zaman `ceil` — alıcı aleyhine yuvarlama engellenir

**`TradeError` enum** — 9 variant, `PriceEstimationError` ile aynı semantik

**DEX Uygulama Notu**: Uniform clearing price formülü. Sell ve buy için farklı yönler; her zaman `ceil` ile yuvarlama. Nova Shield settlement contract'ında bu formül kullanılmalı.

### 2.14 trade_finding/external.rs

**`ExternalTradeFinder` struct**:
- `quote_endpoint: Url` — driver'ın `/quote` endpoint'i
- `sharing: BoxRequestSharing<Query, SharedTradeResponse>` — in-flight request deduplication
- `block_stream: CurrentBlockWatcher` — block-dependent queryler için

**`SharedTradeResponse`**: sonuç + `request_id` (tracing için)

**Request sharing**: aynı Query için 2 istek uçaktaysa, ikincisi birincinin sonucunu bekler (hiçbir duplicate request gönderilmez).

### 2.15 trade_finding/trade_estimator.rs

**`TradeEstimator`**:
- `Inner { finder, verifier: Option<...> }`
- `with_verifier()`: optional verification ekler
- `rate_limited()` ile tüm tahminler rate limiter'dan geçer
- Verifier varsa: `get_trade` → `verifier.verify`; yoksa `get_quote` ile basit tahmin

### 2.16 external.rs
**`ExternalPriceEstimator`**: `TradeEstimator` etrafında ince wrapper
- Driver URL alır, `ExternalTradeFinder` oluşturur, `TradeEstimator` ile sarar
- `verified()`: verifier ekler ve yeni instance döner

### 2.17 sanitized.rs (607 satır)

**`SanitizedPriceEstimator`** — ön-doğrulama katmanı:
- Deny-listed token kontrolü
- ETH/WETH trivial dönüşümleri:
  - `BUY_ETH_ADDRESS` buy → native token + `GAS_PER_WETH_UNWRAP (9_223 gas)` eklenir
  - `BUY_ETH_ADDRESS` sell → native token + `GAS_PER_WETH_WRAP (24_038 gas)` eklenir
  - WETH → ETH sell: 1:1, gas = unwrap + overhead
  - ETH → WETH buy: 1:1, gas = wrap + overhead
- Aynı token: trivial 1:1 estimate (gas=0, verified=true)
- `is_estimating_native_price`: sell=buy durumunda inner'a gitmez, kendi çözer

### 2.18 instrumented.rs (235 satır)

**`InstrumentedPriceEstimator<T>`**:
- Prometheus metrics: `price_estimates` (counter) + `price_estimation_times` (histogram)
- Her sonuç türü için ayrı label: `success`, `no_liquidity`, `unsupported_token`, vb.
- `tracing::info_span!` ile distributed tracing

### 2.19 gas.rs (63 satır) — DEX KRİTİK

**Gas sabitleri** (multivariate regression ile türetilmiş):
```
INITIALIZATION_COST = 32_000     # initial tx gas
SETTLEMENT = 7_365               # isSolver check
TRADE = 35_000 + 2*3_000 + 3_000 = 44_000  # per trade overhead
ERC20_TRANSFER = 27_513          # 52. percentile median (Oct 2021)
GAS_REFUNDS = 13_000             # empirik
SETTLEMENT_SINGLE_TRADE = 32_000 + 7_365 + 44_000 + 2*27_513 - 13_000 = 125_391
GAS_PER_ORDER = 66_315           # regression
GAS_PER_UNISWAP = 90_171         # Dune analytics
GAS_PER_ZEROEX_ORDER = 66_358    # Dune analytics
GAS_PER_BALANCER_SWAP = 88_892   # Dune analytics
GAS_PER_WETH_UNWRAP = 9_223
GAS_PER_WETH_WRAP = 24_038
```

**DEX Uygulama Notu**: Bu sabitler gerçek Ethereum verilerinden türetilmiş. Nova Shield'da da benzer Dune queries ile chain-specific gas calibration yapılmalı.

### 2.20 factory.rs (80+ satır)

**`PriceEstimatorFactory`**:
- `EstimatorEntry { optimal, fast, native }` — 3 tür estimator per solver
- `Network`: web3, chain, native_token, settlement, block_stream, vb.
- `Components`: http_factory, deny_listed_tokens, token_info
- `PriceEstimation` config'den factory oluşturur
- Tenderly simülatör entegrasyonu ile `TradeVerifier` oluşturur

---

## 3. winner-selection/ Crate Analizi

### 3.1 arbitrator.rs (746 satır) — DEX KRİTİK — CORE ALGORİTMA

**`Arbitrator` struct**:
```rust
pub struct Arbitrator {
    max_winners: usize,
    weth: Address,
}
```

**`arbitrate()` pipeline**:
1. Her çözüm için `compute_order_score` → `Scored`
2. `filter_unfair`: baseline score altındaki çözümler elenir
3. `mark_winners`: `pick_winners` (greedy) çağrılır
4. `compute_reference_scores`: VCG-style referans skoru
5. Sort: winner'lar referans skoruna göre sıralanır

**`compute_order_score` (CIP-38)** — sipariş skoru:
```
score = surplus_over_limit_price + protocol_fees
```
Her ikisi de native token cinsinden:

**`surplus_over` formülleri**:
- Sell order: `surplus = bought_amount - limit_buy_amount`
  - ETH'e çeviri: `surplus_eth = surplus * native_price_buy / 1e18`
- Buy order: `surplus = limit_sell_amount - sold_amount`
  - ETH'e çeviri: `surplus * buy_amount / sell_amount * native_price_buy / 1e18`

**`protocol_fees` hesabı** — FeePolicy listesi **ters sırayla** işlenir:
- Her fee politikası "custom prices" vektörünü güncelliyor (önceki politikaların etkisi dahil)

**`surplus_fee` formülü** (Surplus policy):
```
surplus_fee = surplus * factor / (1 - factor)
```
(post-fee trade üzerinden hesaplandığı için oran ayarlanıyor)

**`volume_fee` formülleri**:
- Sell order: `buy_amount * factor / (1 - factor)`
- Buy order: `sell_amount * factor / (1 + factor)`

**`PriceImprovement` fee**:
- Quote fiyatı ile gerçek fiyat arasındaki fark üzerinden
- `adjust_quote_to_order_limits`: quote miktarlarını sipariş limit fiyatlarına göre kırpar

**`pick_winners` — greedy algoritması**:
```
Sort solutions by score desc
For each solution:
  If directed token pairs DISJOINT with current winners:
    Add to winners
  Until max_winners reached
```
- Disjoint: aynı yönde (sell A → buy B) aynı çifti işleyen iki çözüm birlikte kazanamaz

**`compute_reference_scores` (VCG-like)**:
```
reference_score(solver) = score of best alternative winning set WITHOUT this solver
```
- Her kazanan için: bu solver çıkarılırsa hangi çözüm kümesi en iyi?
- Bu referans skorun üzerindeki surplus = solver'ın "ödülü"

**`compute_baseline_scores` (fairness)**:
- Her çözüm kümesi için maximum mümkün skor (alt sınır testi)

**DEX Uygulama Notu**: CIP-38 VCG-mekanizması. Gerçek mekanizma tasarımı — her solver en iyi alternatif olmadan ne kadar değer katar? Bu "ödül = katkı" prensibi Sybil-resistant. Nova Shield'da solver rekabeti için bu mekanizma uygulanmalı.

### 3.2 solution.rs (124 satır)
**`Solution<State>` generic struct** — type-state pattern:
- `id: u64`, `solver: Address`, `orders: Vec<Order>`, `state: State`
- `Solution<Unscored>` → `Solution<Scored<U256>>` → `Solution<Ranked<U256>>`
- `with_state()` ile dönüşüm

**`Order` struct** — minimal:
```rust
pub struct Order {
    uid: OrderUid,
    sell_token: Address, buy_token: Address,
    sell_amount: U256, buy_amount: U256,  // limit prices
    executed_sell: U256, executed_buy: U256,  // gerçek miktarlar
    side: Side,
}
```

### 3.3 primitives.rs (84 satır)
- `NATIVE_TOKEN = 0xEe...Ee`: sentinel address
- `as_erc20(token, wrapped)`: native → WETH dönüşümü
- `price_in_eth(price, amount) = amount * price / 1e18`
- `DirectedTokenPair { sell, buy }`: yönlü çift (A→B ≠ B→A)
- `OrderUid([u8; 56])`: 32 byte digest + 20 byte owner + 4 byte valid_to
  - `owner()`: bytes 32-51 çıkarır
- `Side { Buy, Sell }`
- `FeePolicy { Surplus, PriceImprovement, Volume }`
- `Quote { sell_amount, buy_amount, fee, solver }`

### 3.4 auction.rs (52 satır)
**`AuctionContext`**:
```rust
pub struct AuctionContext {
    fee_policies: HashMap<OrderUid, Vec<FeePolicy>>,
    surplus_capturing_jit_order_owners: HashSet<Address>,
    native_prices: HashMap<Address, U256>,
}
```
- `contributes_to_score`: fee_policies varsa VEYA JIT owner whitelist'teyse true
- JIT orders: Just-In-Time (solver tarafından oluşturulan anlık likidite emirleri)

### 3.5 state.rs (156 satır) — Type-State Pattern
**State types**: `Unscored`, `Scored<S>`, `Ranked<S>`
- `HasState` trait: `type State`, `type Next<NewState>`, `with_state()`, `state()`
- Blanket impls:
  - `UnscoredItem` — `state = Unscored`
  - `ScoredItem` — `score()` döner
  - `RankedItem` — `rank_type()` döner
- `RankType { Winner, NonWinner, FilteredOut }`: kazananlar/kazanamayanlar

---

## 4. database/ Crate Analizi

### 4.1 lib.rs (131 satır) — DB Şeması

**25 modül** — tam DB katmanı

**18 tablo**:
```
TABLES = [
    orders, quotes, trades, settlements, order_events,
    order_execution, fee_policies, auction_prices, solver_competition,
    solver_competition_v2, jit_orders, cow_amms, ethflow_orders,
    last_indexed_blocks, reference_scores, settlement_executions,
    onchain_broadcasted_orders, onchain_invalidations
]
```

**LARGE_TABLES** (9 adet):
```
orders, quotes, trades, settlements, solver_competitions, order_execution, fee_policies, auction_prices, settlement_executions
```

**`clear_DANGER_`**: test helper — tüm tabloları TRUNCATE CASCADE yapar (FK döngüleri için CASCADE zorunlu)

**Tip aliasları**:
- `Address = ByteArray<20>`
- `AppId = ByteArray<32>`
- `TransactionHash = ByteArray<32>`
- `OrderUid = ByteArray<56>`
- `PgTransaction<'a> = sqlx::Transaction<'a, Postgres>`

### 4.2 orders.rs — DEX KRİTİK

**Enum türleri** (sqlx::Type uyumlu):
- `OrderKind { Buy, Sell }`
- `OrderClass { Market, Liquidity, Limit }`
- `SigningScheme { Eip712, EthSign, Eip1271, PreSign }`
- `SellTokenSource { Erc20, Internal, External }`
- `BuyTokenDestination { Erc20, Internal }`

**`Order` struct** — tam DB row:
- uid, owner, creation_timestamp, sell_token, buy_token, amounts, fee
- partially_fillable, kind, class, signing_scheme, app_data
- cancellation_timestamp, is_liquidity_order

**`insert_orders_and_ignore_conflicts`**:
- Duplicate siparişleri sessizce atar (ON CONFLICT DO NOTHING)
- Her insert sonrası `order_event` de oluşturur

### 4.3 settlements.rs

**`find_settlement_transaction`**: tx_hash ile settlement bulur
**`get_settlements_without_auction`**: auction_id olmayan settlements (indexer gecikmesi)
**`update_settlement_auction`**: settlement'a auction_id bağlar
**`update_settlement_solver`**: kazanan solver adresini günceller

### 4.4 fee_policies.rs (205 satır) — DEX KRİTİK

**`FeePolicy` DB row**:
```rust
pub struct FeePolicy {
    auction_id: AuctionId,
    order_uid: OrderUid,
    application_order: i32,  // uygulama sırası
    kind: FeePolicyKind,
    surplus_factor: Option<f64>,
    surplus_max_volume_factor: Option<f64>,
    volume_factor: Option<f64>,
    price_improvement_factor: Option<f64>,
    price_improvement_max_volume_factor: Option<f64>,
}
```

**`FeePolicyKind` enum**: `Surplus`, `Volume`, `PriceImprovement`

**`insert_batch`**: birden fazla fee policy tek sorguda eklenir

**`fetch_all`**: `(auction_id, order_uid)` key'e göre, `application_order` sırasıyla döner
- Bu sıra kritik: fee'ler ters sırayla uygulanıyor (arbitrator.rs)

**`fetch_by_order_uid`**: verilen order için tüm fee politikalarını döner

### 4.5 quotes.rs — DEX KRİTİK

**`Quote` DB row**:
- sell_token, buy_token, sell_amount, buy_amount, gas_amount, gas_price
- sell_token_price, order_kind, buy_token_balance, sell_token_balance
- signing_scheme, solver (address), verified (bool), metadata (JSON)
- `sell_amount_0`, `sell_amount_1`: iki farklı sell amount (fee dahil/hariç)

**`QuoteKind` enum**:
- `Standard`: normal EIP-712/EthSign siparişler
- `Eip1271OnchainOrder`: ERC-1271 onchain siparişler
- `PreSignOnchainOrder`: PreSign onchain siparişler

**`save`**: yeni quote ekler, `QuoteId (i64)` döner

**`QuoteSearchParameters`**: mevcut quote bulma; `sell_amount_0` ve `sell_amount_1` ikisi de sorgulanır

### 4.6 trades.rs — DEX KRİTİK

**`TradesQueryRow`**:
- block_number, log_index, order_uid
- buy_amount, sell_amount, sell_amount_before_fees (= sell_amount - fee_amount)
- owner, buy_token, sell_token, tx_hash, auction_id

**SQL query**:
- `LATERAL JOIN`: aynı block'ta, aynı log_index sonrası ilk settlement'ı bulur
- Bu sayede her trade kendi settlement transaction'ına bağlanır
- Offset/limit ile pagination desteği

### 4.7 solver_competition.rs

**Legacy JSON-tabanlı** (v2 öncesi):
- `save`: `solver_competitions (id, json)` tablosuna INSERT
- `load_by_id`: id + tx_hash listesi ile döner
- `auction_start_block`: JSON'dan `auctionStartBlock` alanını çeker
- `solver_competition_v2.rs`: yeni versiyon — individual typed columns kullanan versiyon

---

## 5. number/ Crate Analizi

### 5.1 lib.rs (7 satır)
- 6 modül: `conversions`, `nonzero`, `ratio_ext`, `serialization`, `testing`, `u256_ext`, `units`

### 5.2 u256_ext.rs (216 satır) — DEX KRİTİK

**`U256Ext` trait** — alloy `U256` için extension:

**`checked_ceil_div(lhs, rhs)`**:
```rust
// alloy'un built-in div_ceil kullanır
lhs.div_ceil(rhs)
```
- `rhs == 0` → `None`
- Fee hesaplarında `ceil` için standart yöntem

**`checked_mul_f64(value, factor)`**:
```rust
// factor'ı 1e18 ile ölçeklendir
// integer multiply → divide ile hassas hesap
let scaled = (factor * 1e18) as u64;
value.checked_mul(U256::from(scaled))?.checked_div(U256::from(1e18 as u64))
```
- f64 hassasiyet kaybını minimize eder
- Overflow checked

**`to_big_rational`**: `U256 → BigRational` (kayıpsız)

**`from_big_int`**: `BigInt → U256` (negatif = hata)

**`from_big_rational`**: `BigRational → U256` (kesen bölme, truncate)

### 5.3 conversions.rs (270 satır) — DEX KRİTİK

**Bidirectional dönüşümler**:
- `BigDecimal ↔ BigUint/BigInt/U256/BigRational`
- `U256 ↔ U160/I512`
- `BigRational ↔ U256`

**Önemli davranışlar**:
- `big_decimal_to_u256`: kesirli kısım varsa hata (integer zorunlu)
- `big_rational_to_u256`: `floor()` ile truncate (kesme yönü: aşağı)
- `i512_to_u256`: işaret bit kontrolü (negatif = hata)

**DEX Uygulama Notu**: Fee ve surplus hesaplarında rounding direction kritik. BigDecimal → U256 için "integer only" kural, kasıtlı precision loss'u önler. Nova Shield'da aynı sertlik uygulanmalı.

### 5.4 nonzero.rs (76 satır)
**`NonZeroU256`** wrapper:
- Default değer: `ONE` (sıfır değil!)
- Serde: decimal string olarak serialize/deserialize
- `TryFrom<U256>`: sıfır → `NonZeroError`
- `TryFrom<u128>`: u128 → U256 → NonZeroU256

### 5.5 ratio_ext.rs (38 satır) — DEX KRİTİK

**`RatioExt<U256>` trait**:

**`scalar_mul(ratio, scalar)`**:
```rust
// checked multiply: taşma kontrolü
ratio.numer().checked_mul(scalar)? / ratio.denom()
```

**`full_scalar_mul(ratio, scalar)`**:
```rust
// U512 widening multiply (intermediate taşmayı önler)
let wide = U512::from(*ratio.numer()) * U512::from(scalar);
wide / U512::from(*ratio.denom())
// → U256'ya geri dönüşüm
```

**DEX Uygulama Notu**: `full_scalar_mul` kritik. Büyük miktar × fiyat oranı hesaplarında U256 taşabilir. U512 genişletmesi kayıpsız orta hesap sağlar. Nova Shield surplus/fee hesaplarında `full_scalar_mul` kullanılmalı.

### 5.6 units.rs (43 satır)
**`EthUnit` trait**:
```rust
fn atom(self) -> U256;      // 1
fn matom(self) -> U256;     // * 1_000_000
fn gatom(self) -> U256;     // * 1_000_000_000
fn eth(self) -> U256;       // * 1_000_000_000_000_000_000
```
- `u64`, `u128`, `f64` için implement edilmiş
- `f64::eth()`: `(self * 1e18) as u128` → U256 (dikkat: f64 precision sınırı)

### 5.7 serialization.rs (29 satır)
**`HexOrDecimalU256`** struct:
- Deserialize: hex VEYA decimal kabul eder
- Serialize: her zaman **decimal** string yazar (alloy varsayılan hex'dir, bu override eder)

### 5.8 testing.rs
- Test yardımcı fonksiyonları

---

## 6. Küçük Crate'ler Analizi

### 6.1 ethrpc/src/lib.rs — DEX ALTYAPI

**`Web3` struct**:
```rust
pub struct Web3 {
    provider: AlloyProvider,  // DynProvider (alloy)
    wallet: MutWallet,
}
```
- `AlloyProvider = DynProvider` — web3 → alloy geçişi (kademeli migration)
- `web3()` factory: batch size/concurrency ayarına göre uygun provider seçer
  - `max_batch_size=0|1`: unbuffered provider
  - Diğer: batched provider

**`Config` default**:
- `ethrpc_max_batch_size = 20`
- `ethrpc_max_concurrent_requests = 10`
- `ethrpc_batch_delay = Duration::ZERO`

**`MAX_BATCH_SIZE = 100`**: global upper bound

### 6.2 chain/src/lib.rs (250 satır) — DEX ALTYAPI

**`Chain` enum — 13 chain**:
| Chain | Chain ID |
|-------|----------|
| Mainnet | 1 |
| Gnosis | 100 |
| ArbitrumOne | 42161 |
| Base | 8453 |
| Bnb | 56 |
| Avalanche | 43114 |
| Optimism | 10 |
| Polygon | 137 |
| Linea | 59144 |
| Plasma | 9745 |
| Ink | 57073 |
| Sepolia | 11155111 |
| Hardhat | 31337 |

**`default_amount_to_estimate_native_prices_with`**:
- Çoğu chain: **0.1 ETH**
- Gnosis/Avalanche: **1 ETH** (daha düşük değerli)
- Polygon/Plasma: **100 MATIC**

**`block_time_in_ms`**: chain'e göre (Mainnet=12000ms, Arbitrum=250ms, vb.)

**`blocks_in(time_ms)`**: süreyi block sayısına çevirir

### 6.3 bytes-hex/src/lib.rs (60 satır)

**`serialize` / `deserialize`**:
- `Vec<u8>` → `"0x..."` hex string (const_hex kullanır)
- "0x" prefix zorunlu (yoksa hata)

**`BytesHex`** struct: `serde_with` uyumlu adapter

### 6.4 number/src/serialization.rs — bakınız 5.7

### 6.5 account-balances/ — DEX KRİTİK

**`lib.rs` (252 satır)**:

**`Query` struct**:
```rust
pub struct Query {
    owner: Address,
    token: Address,
    source: SellTokenSource,
    interactions: Vec<Interaction>,    // pre-hook interactions
    balance_override: Option<...>,     // test/simulation override
}
```

**`BalanceFetching` trait**:
- `get_balances(queries) → Vec<Result<U256>>`
- `can_transfer(query) → bool`
- `allowance(query) → U256`

**`BalanceSimulator`**:
- `GPv2Settlement.simulateDelegatecall` → `Balances` yardımcı contract'ı
- Sonuç decode: `(token_balance, allowance, effective_balance, can_transfer, revert_reason)`
- `SIMULATION_ACCOUNT = Address::random()` (lazy): rasgele EOA simüle eder

**`simulation.rs` (80+ satır)**:
**`Balances` struct**:
- `tradable_balance_simulated`: delegatecall simülasyonu ile (pre-interactions dahil)
- `tradable_balance_simple`: direkt `balanceOf + allowance` (simülasyon gerektirmez)
- Her ikisi de `min(balance, allowance)` döner (efektif çekilebilir miktar)

**`cached.rs`**:
- `EVICTION_TIME = 5 blocks`
- `BalanceCache { HashMap<Query, BalanceEntry { balance, requested_at, updated_at }> }`
- Background updater: her block'ta mevcut girişleri günceller
- `insert_balance`: yalnızca yeni istekler için (mevcut girişlere dokunmaz)

**DEX Uygulama Notu**: Balance fetch için delegatecall simülasyonu = pre-interactions etkisini önceden hesaplar. Bu sayede "Composable CoW" (safe tx ile balance setup) siparişleri desteklenir. Nova Shield'da pre-hook desteklenecekse bu pattern zorunlu.

### 6.6 signature-validator/ — DEX KRİTİK

**`lib.rs` (129 satır)**:

**`SignatureCheck` struct**:
```rust
pub struct SignatureCheck {
    signer: Address,
    hash: H256,
    signature: Bytes,
    interactions: Vec<Interaction>,    // ERC-1271 setup için
    balance_override: Option<...>,     // flashloan desteği
}
```

**`MAGICAL_VALUE = 0x1626ba7e`**: EIP-1271 magic bytes

**`requires_setup`**: interactions VEYA balance_override varsa true

**`SignatureValidating` trait**:
- `validate_signature(check) → Result<(), Error>`
- `validate_signature_and_get_additional_gas(check) → Result<u64, Error>`: ek gas maliyeti döner (settlement için)

**`simulation.rs` (80+ satır)**:

**`Validator` struct**:
- `IS_VALID_SIGNATURE_MAGIC_BYTES = "1626ba7e"`
- `simulate_without_setup`: direkt `isValidSignature` call
- Setup'lı durumda: delegatecall via GPv2Settlement
- `balance_overrider: Arc<dyn StateOverriding>`: flashloan state simulation

**DEX Uygulama Notu**: ERC-1271 iki modda çalışır: (1) setup gerektirmeyen: direkt call; (2) setup gerektiren: settlement üzerinden delegatecall. Bu pre-hook'lar çalıştıktan sonra imzanın geçerli olacağını doğrular. Nova Shield'da ERC-1271 desteği için her iki mod da implemente edilmeli.

### 6.7 bad-tokens/ — DEX KRİTİK

**`lib.rs` (22 satır)**:
**`TokenQuality` enum**:
```rust
pub enum TokenQuality {
    Good,
    Bad { reason: String },
}
```

**`trace_call.rs`**:

**`TraceCallDetectorRaw`**:
- `test_transfer`: `trace_callMany` ile 2 ERC-20 transfer simüle eder
- Transfer 1: token sahibinden rastgele adrese
- Transfer 2: alıcıdan başka rastgele adrese
- Fee-on-transfer tokenlarda alınan miktar < gönderilen miktar olur → **Bad**
- Graceful handling: `METHOD_NOT_FOUND (-32601)` veya `HTTP 400` → **Good** (node desteklemiyor ama token muhtemelen iyi)

**`list_based.rs` (27 satır)**:
**`DenyListedTokens`**:
- `Arc<Inner { deny_list: HashSet<Address> }>`
- `contains(token)`: O(1) lookup
- `SanitizedPriceEstimator`'a inject edilir

### 6.8 balance-overrides/ — DEX KRİTİK

**`lib.rs` (80+ satır)**:

**`StateOverriding` trait**:
```rust
async fn balance_override(request) -> Option<(Address, AccountOverride)>;
async fn approval_override(request) -> Option<(Address, AccountOverride)>;
```

**`StateOverrides` struct**:
- `balance_detector: balance::Detector`
- `approval_detector: approval::Detector`
- Default config: `probing_depth=60`, `verification_timeout=DEFAULT_VERIFICATION_TIMEOUT`, `cache_size=1000`

**`balance::Detector`**: token için storage slot'unu keşfeder (balanceOf mapping slot'unu bulur)
**`approval::Detector`**: allowance mapping slot'unu bulur

**Kullanım**: ERC-1271 simülasyonunda, flashloan balance'ı simüle etmek için `eth_call` state override kullanılır

**DEX Uygulama Notu**: Balance state override = RPC seviyesinde bakiye manipülasyonu (simülasyon amaçlı). `eth_call` state override ile gerçek token transfer olmadan bakiye test edilebilir. Nova Shield verified quotes için bu gerekli.

---

## 7. Genel DEX Mimari Değerlendirmesi

### 7.1 Price Estimation Pipeline

```
Query (sell/buy/amount/kind)
  → SanitizedPriceEstimator (deny-list, ETH/WETH trivial)
    → CompetitionEstimator (multi-stage, parallel)
      → Stage 1: [ExternalPriceEstimator_1, ExternalPriceEstimator_2, ...]
        → TradeEstimator (rate-limited)
          → ExternalTradeFinder (request sharing, /quote endpoint)
            → optional: TradeVerifier (simulation verify)
      → Stage 2: fallback estimators
    → InstrumentedPriceEstimator (metrics)
```

### 7.2 Winner Selection Pipeline

```
Unscored Solutions
  → compute_order_score (CIP-38: surplus + protocol_fees)
  → compute_baseline_scores (fairness filter)
  → filter_unfair (baseline threshold)
  → pick_winners (greedy, disjoint directed pairs)
  → compute_reference_scores (VCG-like)
  → Final ranking
```

### 7.3 Database Schema Özeti

**Order lifecycle** (DB perspective):
1. INSERT orders (`order_events` ile birlikte)
2. SELECT for auction (kısmi dolgu, geçerlilik kontrolü)
3. INSERT quotes (tahmin sonuçları)
4. INSERT fee_policies (auction bazlı)
5. INSERT trades (settlement event'inden)
6. INSERT settlements (block event'inden)
7. UPDATE settlements (auction_id bağlanınca)

### 7.4 Kritik Matematiksel Formüller Özeti

| Formül | Kullanım |
|--------|----------|
| `fee = ceil(gas * gas_price / token_price)` | Gas fee hesabı |
| `price_in_eth = amount * price / 1e18` | Token → ETH çevirisi |
| `surplus (sell) = bought - limit_buy` | Surplus hesabı |
| `surplus (buy) = limit_sell - sold` | Surplus hesabı |
| `surplus_fee = surplus * f / (1-f)` | Surplus fee (post-fee) |
| `volume_fee (sell) = buy_amount * f / (1-f)` | Volume fee |
| `volume_fee (buy) = sell_amount * f / (1+f)` | Volume fee |
| `effective_eth_out = out_amount * price - gas * gas_price` | Quote ranking |
| `trade.out (sell) = ceil(in * sell_price / buy_price)` | Clearing price |
| `full_scalar_mul: U512 intermediate` | Overflow-safe multiply |

---

## 8. Nova Shield DEX Uygulama Notu — Eylem Planı

### 8.1 Acil Alınması Gereken Kararlar

1. **Fiyat temsili**: Token fiyatlarını "1 token atom = X ETH atom" formatında tut (`BigRational`). BUY_ETH sentinel adresi seç.

2. **Fee hesabı**: `ceil(gas * gas_price / token_price)` — asla `floor`. Özellikle düşük decimal tokenlarda.

3. **Gas sabitlerini kalibre et**: Nova Shield'ın gas maliyetleri farklı olacak. `gas.rs` sabitleri CoW'a özgü; Nova Shield için kendi regression yapılmalı.

4. **Quote ranking**: `effective_eth_out = out - gas * gas_price` formülü. Hem gas-oblivious hem gas-adjusted ranking seçeneği sun.

5. **U512 overflow protection**: `surplus_fee`, `volume_fee`, `score` hesaplarında U256 × U256 → U512 kullanılmalı.

6. **State machine oracle fallback**: Primary/fallback ile 3 ardışık hata → fallback; probe interval = 60s.

7. **ERC-1271 desteği**: İki mod: setup-free (direkt call) ve setup-required (delegatecall via settlement).

8. **Balance fetching**: `min(balance, allowance)` efektif bakiye. Pre-interactions için delegatecall simülasyonu.

9. **Token deny list**: `DenyListedTokens` + `TraceCallDetectorRaw` (fee-on-transfer detection) kombinasyonu.

10. **Directed pair uniqueness**: Winner selection'da aynı yönde aynı çifti işleyen çözümler yarışamaz. Bu Sybil resistance için kritik.

### 8.2 Yapılmaması Gerekenler

- Gas fee hesabında `floor` kullanmak — sıfır fee saldırısı açığı
- U256 ile ara hesap yapmak (büyük miktar × oran) — taşma riski
- `f64` native price'ları doğrudan hesapta kullanmak — precision loss
- Aynı quote'u birden fazla dolgu için yeniden kullanmak — fee_amount satırı bakılmadan

### 8.3 Kopya Alınabilecek Bileşenler (Direkt Uyarlama)

| CoW Bileşeni | Nova Shield Karşılığı |
|--------------|----------------------|
| `ExternalPrices` | `NativeTokenPrices` |
| `CompetitionEstimator` | `MultiSourcePriceAggregator` |
| `FallbackNativePriceEstimator` | `OracleFallback` |
| `NativePriceCache (ArcSwap)` | `PriceCache` |
| `U256Ext::checked_ceil_div` | `FeeCalc::ceil_div` |
| `RatioExt::full_scalar_mul` | `SafeMath::wide_mul` |
| `BufferedRequest` | `BatchedPriceFetcher` |
| `SanitizedPriceEstimator` | `TokenSafetyLayer` |
| `DenyListedTokens` | `TokenBlocklist` |
| `BalanceSimulator` | `BalanceProber` |

---

*Agent 10 analizi tamamlandı. Toplam analiz: shared/ (18 dosya) + price-estimation/ (20+ dosya) + winner-selection/ (5 dosya) + database/ (25 dosya) + number/ (8 dosya) + ethrpc/ + chain/ + bytes-hex/ + balance-overrides/ + account-balances/ + signature-validator/ + bad-tokens/ crate'leri.*
