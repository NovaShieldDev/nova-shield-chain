# CoW Protocol Analiz — Agent 21
## Kapsam: price-estimation/ + pool-indexer/ + refunder/ — Kalan Crate'ler
## DEX Ozel Protokolu: 3x Hassasiyet
## Tarih: 2026-07-22

---

## Genel Bakis

Bu agent CoW Protocol reposunda onceki 20 agentin kapsamadigi son uc crate'i
A'dan Z'ye analiz eder:

| Crate | Dosya Sayisi | Tahmini Satir | Rol |
|---|---|---|---|
| `price-estimation/` | 27 | ~6,000 | Fiyat tahmin motoru — DEX quote altyapisi |
| `pool-indexer/` | 18 | ~3,000 | UniswapV3 pool indexleyici (tick + sqrtPrice) |
| `refunder/` | 9 | ~1,200 | EthFlow iptal siparisleri iade servisi |
| **TOPLAM** | **54** | **~10,200** | |

DEX Ozel Protokolu uyarinca `price-estimation/` crate'i MAKSIMUM hassasiyetle
analiz edilmistir; her struct, trait, algoritma ve matematiksel formul dokumante
edilmistir.

---

## 1. CRATE: price-estimation/

### 1.1 Mimari Genel Bakis

`price-estimation` crate'i CoW Protocol'un "ne kadar token alirim?" sorusunu
cozen altyapidir. Orderbook quote API'si (POST /api/v1/quote) bu crate uzerinden
calisir; solver'lar da pre-auction native fiyat tespiti icin bu crate'i kullanir.

```
                    Kullanici Quote Istegi
                           |
                           v
              SanitizedPriceEstimator
              (deny-list, ETH->WETH donusumu)
                           |
                           v
              CompetitionEstimator<T>
              (coklu kaynak, yarisma, sirala)
              /                           \
    Stage 1: [Driver1, Driver2]   Stage 2: [CoinGecko, 1inch]
             (paralel)                     (paralel, yalnizca
                                           Stage1 yetersizse)
                           |
                           v
              TradeEstimator (TradeFinding wrappers)
              |                     |
              TradeVerifying        ExternalTradeFinder
              (simulation dogrula)  (driver HTTP /quote)
```

### 1.2 src/lib.rs — Temel Tipler ve Trait'ler

#### Hata Tasnifi: `PriceEstimationError`

```
UnsupportedToken          — deny-list veya protokol kisitiamasi
NoLiquidity               — AMM havuzu yetersiz
UnsupportedOrderType      — ornegin "belirli solver buy desteklemez"
RateLimited               — rate limiter tetiklendi
TradingOutsideAllowedWindow — zaman penceresi kisitlamali token (RWA/xStocks)
TokenTemporarilySuspended — gecici askiya alinmis token
InsufficientLiquidity     — miktar cok buyuk, havuzda yetmez
CustomSolverError         — solver tanimli ozel hata
EstimatorInternal         — tahminci-ici hata (cache edilebilir)
ProtocolInternal          — gecici hata (yeniden dene)
```

**Hata Onceligi (compare_error):**

Hata tipleri "kacinkabilirlik" sirasiyla derecelendirilir:
```
6 (en yuksek): TradingOutsideAllowedWindow, TokenTemporarilySuspended,
               InsufficientLiquidity, CustomSolverError
5: RateLimited
4: ProtocolInternal
3: EstimatorInternal
2: UnsupportedToken
1: NoLiquidity
0 (en dusuk):  UnsupportedOrderType
```

Mantik: yuksek oncelikli hatalar "bir sey biliyoruz" anlamina gelir ve tercih
edilir; dusuk oncelikli hatalar "hicbir seyi bilmiyoruz" anlamina gelir.

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield price estimation motorunda benzer bir hata siniflandirmasi
uygulanmalidir. Ozellikle RWA token'lari icin `TradingOutsideAllowedWindow`
ekivalani kritik: trading saatleri disinda quote istekleri token'i bloke
etmeden reddedilmelidir.

#### `Query` Struct (satir 148-162):

```rust
pub struct Query {
    pub sell_token: Address,
    pub buy_token: Address,
    pub in_amount: NonZeroU256,  // sifir-olmayan garanti
    pub kind: OrderKind,         // Sell veya Buy
    pub verification: Verification,
    pub block_dependent: bool,   // false ise onceki blok sonucu kullanilabilir
    pub timeout: Duration,
}
```

`block_dependent: false` performans optimizasyonu: onceki bloktan cache
kullanilabilir. Volatile token'lar icin `true` yapilmalidir.

#### `Verification` Struct (satir 166-183):

```rust
pub struct Verification {
    pub from: Address,     // sell_token'i elinde bulunduran adres
    pub receiver: Address, // buy_token alacak adres (sifirsa from ile ayni)
    pub app_data: Arc<String>,  // hooks, custom wrappers icin
}
```

`effective_receiver()`: receiver sifirsa from adresini dondurur. Bu,
"kendi kendine takas" senaryolari icin onemlidir.

#### `Estimate` Struct (satir 186-198):

```rust
pub struct Estimate {
    pub out_amount: U256,    // kac token alinir (veya satilir)
    pub gas: u64,            // toplam gas maliyeti
    pub solver: Address,     // hangi solver verdi
    pub verified: bool,      // simulation ile dogrulandi mi?
    pub execution: QuoteExecution, // interactions + jit_orders
}
```

**Fiyat hesabi:**
```
price_in_buy_token = buy_amount / sell_amount
               = out_amount / in_amount  (Sell icin)
               = in_amount / out_amount  (Buy icin)
```

#### `PriceEstimating` Trait (satir 221-224):

```rust
pub trait PriceEstimating: Send + Sync + 'static {
    fn estimate(&self, query: Arc<Query>) -> BoxFuture<'_, PriceEstimateResult>;
}
```

Mockall ile otomatik mock uretimi: `#[cfg_attr(any(test, feature = "test-util"), mockall::automock)]`

#### `StreamingPriceEstimating` Trait (satir 228-231):

```rust
pub trait StreamingPriceEstimating: Send + Sync + 'static {
    fn estimate_stream(&self, query: Arc<Query>) -> BoxStream<'_, PriceEstimateResult>;
}
```

Her estimator sonucu geldikce yield eder, en iyiyi beklememize gerek kalmaz.
Quote streaming API'si (post_quote_stream.rs) bu trait'i kullanir.

#### `rate_limited` Fonksiyonu (satir 235-257):

Rate limiter ile fiyat tahminini entegre eder:
- Tahmin suresi `HEALTHY_PRICE_ESTIMATION_TIME = 5s`'den uzunsa VEYA
  sonuc `RateLimited` ise: rate limiter'a "basarisiz" bildirilir
- Rate limiter tokeni tuketildiyse `PriceEstimationError::RateLimited` doner

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield DEX'te rate limiting, solver/quote kaynaklari icin zorunludur.
Yavaslayan veya hata veren kaynaklar otomatik olarak geri basilmali (exponential
backoff), saglikli olanlar tam trafik almaya devam etmelidir.

---

### 1.3 src/competition/mod.rs — CompetitionEstimator

**Amaç:** Birden fazla fiyat kaynagini (solver, CoinGecko, 1inch) yaristirarak
en iyi fiyati secmek. "Coklu oracle secimi" algoritması.

#### `CompetitionEstimator<T>` Struct:

```rust
pub struct CompetitionEstimator<T> {
    stages: Vec<PriceEstimationStage<T>>,           // Stage'ler sirali
    usable_results_for_early_return: NonZeroUsize,  // Bu kadar iyi sonuc => erken don
    ranking: PriceRanking,                           // Nasil sirala?
    verification_mode: QuoteVerificationMode,        // Dogrulama zorunlu mu?
}
```

#### Stage Stratejisi — "Sequential Stages with Parallel within Stage":

```
Stage 0: [Driver_A, Driver_B] ——> paralel calistir
         usable_results >= threshold? --> erken bitir
         yoksa:
Stage 1: [CoinGecko, OneInch]  ——> paralel calistir
         ...
```

- Stage'ler sirali: Stage 0 bitmeden Stage 1 baslamaz
- Ayni stage icindeki tum estimatorlar paralel calisir
- `usable_results_for_early_return`: N basarili sonuc gelince dur (tumu bekleme)
- `FuturesUnordered` ile gerçek async racing

**Erken Don Optimizasyonu:**
```
missing_results = usable_results_for_early_return - mevcut_basarili_sayisi
stage kombinasyonlari: usable > tek_stage_boyutu ise sonraki stage de baslatilir
```

#### `PriceRanking` Enum:

```rust
pub enum PriceRanking {
    MaxOutAmount,  // En yuksek out_amount kazanir (basit)
    BestBangForBuck {
        native: Arc<dyn NativePriceEstimating>,
        gas: Arc<dyn GasPriceEstimating>,
    },             // out_amount - gas_maliyet_ETH_cinsinden en yuksek kazanir
}
```

`BestBangForBuck` formulu:
```
net_value = out_amount_ETH - (gas * gas_price_ETH)
```
Yani gas maliyeti dusuk ve out_amount yuksek olan kazanir.

**DEX Uygulama Notu — Nova Shield icin:**
`BestBangForBuck` yaklasimi Nova Shield DEX icin referans: kullaniciya
"en iyi net miktar" sunmali, sadece ham quote degil. Zincir icindeki gas
maliyeti ETH cinsinden hesaplanmali ve quote'tan dusulumelidir.

#### estimate_stream Implementasyonu (satir 177-188):

Butun stage'lerdeki butun estimatorlari AYNI ANDA baslatir ve sonuclar
geldikce stream eder. Ranking yok. Kullanici dilediginde stream'i birakir.
Bu, quote streaming API'si icin kullanilir.

**Invariant:** Stream hicbir sonucu drop etmez; hata dahil her sonuc yield edilir.

---

### 1.4 src/sanitized.rs — SanitizedPriceEstimator

**Amaç:** `PriceEstimating` wrapperidir; query'yi temizler:
1. Deny-listed token'lari reddeder
2. `BUY_ETH_ADDRESS` (0xEEEE...EEE) -> WETH donusumunu yapar
3. WETH<->ETH trivial tahminleri kendi uretir

#### Trivial Tahminler:

| sell_token | buy_token | sonuc |
|---|---|---|
| WETH | BUY_ETH_ADDRESS | 1:1, gas = UNWRAP + SETTLEMENT_OVERHEAD |
| BUY_ETH_ADDRESS | WETH | 1:1, gas = WRAP + SETTLEMENT_OVERHEAD |
| token_X | token_X | 1:1, gas = 0, verified = true |

**ETH Gas Sabitleri (gas.rs):**
```
INITIALIZATION_COST  = 32,000   // tx baslatma
SETTLEMENT           = 7,365    // isSolver kontrolu
TRADE                = 41,000   // token transferleri + interaction overhead
ERC20_TRANSFER       = 27,513   // %52 percentil medyan
GAS_REFUNDS          = 13,000   // geri odemeler
SETTLEMENT_SINGLE_TRADE = 94,891 // tek trade icin toplam
SETTLEMENT_OVERHEAD  = 75,891   // overhead (refund haric)
GAS_PER_WETH_UNWRAP  = 9,223
GAS_PER_WETH_WRAP    = 24,038
GAS_PER_ORDER        = 66,315
GAS_PER_UNISWAP      = 90,171
GAS_PER_ZEROEX_ORDER = 66,358
GAS_PER_BALANCER_SWAP= 88,892
```

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield DEX, native ETH emirlerini desteklemek icin ayni ETH->WETH
wrap mekanizmasini uygulamalidir. Gas tahminleri EIP-1559'a gore dinamik
olarak ayarlanmali, ancak bunlar sabit referans olarak kabul edilebilir.

---

### 1.5 src/buffered.rs — BufferedRequest

**Amaç:** Cok sayida native fiyat isteğini otomatik olarak batch haline
getirir. Yuksek yuk altinda N ayrı istegi 1 batch RPC'ye donusturur.

#### Mimari:

```
estimate_native_price(token_A) ──┐
estimate_native_price(token_B) ──┤ mpsc::unbounded channel
estimate_native_price(token_C) ──┘       |
                                          v
                              background_worker
                              (debouncing_time sonra)
                              fetch_native_prices({A,B,C})
                                          |
                              broadcast::channel sonuclari
                                    /    |    \
                              token_A token_B token_C dinleyicileri
```

#### `Configuration` Struct:

```rust
pub struct Configuration {
    pub max_concurrent_requests: Option<NonZeroUsize>,
    pub debouncing_time: Duration,         // ilk istekten bu sure sonra batch gonder
    pub result_ready_timeout: Duration,    // sonuc bekleme timeout'u
    pub broadcast_channel_capacity: usize, // broadcast channel boyutu
}
```

**Kritik Detay:** Subscribe BEFORE send: `rx = results.subscribe()` SONRA
`requests.send(token)`. Boylece token gonderilip sonuc yayin kanalina yazilmasi
arasindaki race condition onlenir.

#### `batched_for_each` Fonksiyonu:

`select_biased!` ile stream chunking:
- debouncing_time dolana VEYA max_batch_size'a ulasana kadar chunk biriktir
- `select_biased!`: debouncing'e oncelik verir (gelen item'a degil), boylece
  batch buyuklugu maximize edilir

**DEX Uygulama Notu — Nova Shield icin:**
Cok sayida token icin native fiyat gerektiren Nova Shield DEX'te bu batch
mekanizmasi zorunludur. CoinGecko gibi dis kaynaklara 1 istek yerine 500
tokenin fiyatini tek cagriyla almak rate limit tasarrufudur.

---

### 1.6 src/native/mod.rs — Native Price Estimation

**Amaç:** Token'in native ETH cinsinden fiyatini tahmin etmek.
Autopilot ve orderbook tarafindan auctiona konan token fiyatlari burada belirlenir.

#### `NativePriceEstimating` Trait:

```rust
pub trait NativePriceEstimating: Send + Sync {
    fn estimate_native_price(
        &self,
        token: Address,
        timeout: Duration,
    ) -> BoxFuture<'_, NativePriceEstimateResult>;
}
```

`NativePrice = f64` — wei per atom cinsinden price.

#### Normalize Etme:

```
to_normalized_price(price: f64) -> Option<U256>:
  price_in_eth = 1e18 * price
  U256::saturating_from(price_in_eth)

from_normalized_price(price: BigDecimal) -> Option<f64>:
  price / 1e18
```

**Price Validity Check:**
```rust
fn is_price_malformed(price: f64) -> bool {
    !price.is_normal()
    || price <= 0.
    || to_normalized_price(price).is_none()
}
```
Normal float degil, sifir veya negatif, ya da U256'ya sigar degil ise malformed.

#### `NativePriceEstimator` Struct:

```rust
pub struct NativePriceEstimator {
    inner: Arc<dyn PriceEstimating>,
    native_token: Address,             // WETH
    price_estimation_amount: NonZeroU256, // sorgu buyuklugu
}
```

Iceride `OrderKind::Buy` kullanir — neden? Derin olmayan havuzlarda Buy emri
daha az slippage uretir, bu da daha guvenilir fiyat tahmini demektir.
(TODO yorumu kodda: "explain why we use BUY order type")

**Test Invarianti:**
`out_amount = 123_456_789_000_000_000` (0.1234... ETH)
`price = buy_amount / sell_amount = 1e18 / 0.1234...e18 = 1 / 0.123456789`

---

### 1.7 src/native/eip4626.rs — EIP-4626 Vault Fiyatlandirma

**Amaç:** EIP-4626 tokenized vault token'larini (ynUSDx, stETH, aUSDC vb.)
native ETH cinsinden fiyatlandirmak. Vault token, alt varligin yield-bearing
wrapperidir.

#### Algoritma (3 adim):

```
1. vault.asset()      → alt_varlik adresi
   vault.decimals()   → vault_decimals
2. vault.convertToAssets(10^vault_decimals) → alt_varlik_atomlari
3. rate = alt_varlik_atomlari / 10^vault_decimals
   vault_price = alt_varlik_native_price * rate
```

#### Matematiksel Turetim (Dokumantasyondan):

```
Ornek: ynUSDx (18 desimal), USDC (6 desimal)
1 tam ynUSDx = 1.1 tam USDC (yield birikmis)

- convertToAssets(10^18) = 1_100_000 (= 1.1 * 10^6 USDC atomu)
- per-atom factor = 1_100_000 / 10^18 = 1.1e-12
- USDC native price = x (wei/atom)
- ynUSDx native price = x * 1.1e-12
```

#### Nested Vault Destegi:

```rust
async fn unwrap_all_layers(&self, token) -> (Address, f64) {
    let mut current = token;
    let mut rate = 1.0;
    while let Some((asset, layer_rate)) = self.unwrap_vault_layer(current)? {
        rate *= layer_rate;   // kumulatif rate
        current = asset;
    }
    (current, rate)
}
```

Vault zinciri: vault1 → vault2 → asset. Her katmanda rate carpilir.

#### Negatif Cache (`non_vault_tokens: DashSet`):

`asset()` revert eden adresler DashSet'e eklenir. Sonraki istekte RPC atlayip
dogrudan inner estimator'a gider. `BUY_ETH_ADDRESS` baslangictan cache'de.

**Guvenlik Invarianti:**
- Contract revert + decimals() basarili => plain ERC20 (cache et)
- Transport hatasi => RETRY (cache etme!)
- Bu iki durumun karistirilmasi kritik hata olurdu.

**Test Invariantlari:**
```
Vault(18 dec) → Asset(18 dec), 1.5x rate: factor = 1.5
Vault(18 dec) → Asset(6 dec),  1.5x rate: factor = 1.5e-12
Vault(6 dec)  → Asset(18 dec), 2x rate:  factor = 2e12
```

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield DEX'in EIP-4626 vault tokenlarini desteklemesi halinde bu
tam algoritmasi kopyalanmalidir. Ozellikle:
- Nested vault desteği (rate carpimi)
- Non-vault negative cache (DashSet)
- Transport vs contract revert ayrimı
- BUY_ETH_ADDRESS'in varsayilan cache'e alinmasi

---

### 1.8 src/native/fallback.rs — FallbackNativePriceEstimator

**Amaç:** Primary estimator defalarca basarisiz olursa (ProtocolInternal),
otomatik olarak fallback'e gecme mekanizmasi.

#### State Machine:

```
               3 consecutive ProtocolInternal
  ┌─────────┐ ─────────────────────────────> ┌──────────┐
  │ Primary │                                │ Fallback │
  └─────────┘ <─────────────────────────── └──────────┘
               probe basarili (60s arayla)
```

#### Sabitler:
```rust
const PROBE_INTERVAL: Duration = Duration::from_secs(60);
const CONSECUTIVE_ERRORS_THRESHOLD: u32 = 3;
```

#### Action Enum:
```
Primary  — normal durum
Fallback — probe suresi dolmamis
Probe    — primary ve fallback AYNI ANDA sorgulanir (racing probe)
```

#### Probe Mantigi:
```rust
fn estimate_with_probe(&self, ...) {
    let (primary, fallback) = join!(primary.estimate(), fallback.estimate());
    if primary is ProtocolInternal {
        state = Fallback { last_probe: now }; // probe suresi sifirla
        return fallback;
    } else {
        state = Primary { consecutive_errors: 0 }; // kurtarildi!
        return primary;
    }
}
```

**Thundering Herd Onlemi:** `last_probe = now()` probe baslamadan oncesi degil,
probe karari verildikten hemen sonra guncellenir. Boylece paralel istekler ayni
anda probe baslatamaz.

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield'da dis price oracle'lar (CoinGecko, 1inch API) gecici olarak
down olabilir. Bu pattern ile primary → fallback geçişi otomatiklestirilmelidir.
Threshold: 3 hata, probe: 60 saniyede bir.

---

### 1.9 src/native_price_cache.rs — NativePriceCache (Ozet)

Cache mimarisi:
- `arc_swap::ArcSwap<HashMap<Address, CachedPrice>>` — lock-free okuma
- Arkaplan gorevi: belirli aralikla sona eren entryleri yenile
- `ApproximationToken`: fiyat tahmininde yardimci token kullanilmasi (peg token)
- Normalization factor: `10^(to_decimals - from_decimals)`

Cache tazelenme stratejisi:
```
evict_fraction: kac % entry silinsin
max_age_without_hit: hit almayan entry ne zaman silinsin
```

Random eviction ile thundering herd onleme: tum cache bir anda sona erip
yenilenmez, rastgele bir alt kume silinir.

---

### 1.10 src/trade_finding/mod.rs — TradeFinding Abstraction

**Amaç:** `PriceEstimating` sadece fiyat dondurur; `TradeFinding` calldata da
uretir. Quote icin calldata gerekli (verification icin simule etmek).

#### `TradeFinding` Trait:

```rust
pub trait TradeFinding: Send + Sync + 'static {
    async fn get_quote(&self, query: &Query) -> Result<Quote, TradeError>;
    async fn get_trade(&self, query: &Query) -> Result<TradeKind, TradeError>;
}
```

#### `TradeKind` Enum:

```
Legacy(LegacyTrade) — eski format: out_amount + interactions
Regular(Trade)      — yeni format: clearing_prices + jit_orders + pre_interactions
```

#### `Trade.out_amount` Formulu:

```
Sell: out_amount = in_amount * sell_price / buy_price (ust yuvarla)
Buy:  out_amount = in_amount * buy_price / sell_price
```

**Ust Yuvarlama (Sell icin):** GPv2Settlement sozlesmesiyle tutarlilik icin
kullanici alacagi miktar `ceil` ile yuvarlanir. Kaynak: GPv2Settlement.sol#389.

#### `TradeError` → `PriceEstimationError` Donusumu:

```
NoLiquidity          → NoLiquidity
UnsupportedOrderType → UnsupportedOrderType
UnsupportedToken     → UnsupportedOrderType(hex_address)
RateLimited          → RateLimited
TradingOutside...    → TradingOutsideAllowedWindow { message }
TokenTemporarily...  → TokenTemporarilySuspended { message }
InsufficientLiq...   → InsufficientLiquidity { message }
CustomSolverError    → CustomSolverError { message }
Other(err)           → Other(err) (EstimatorInternal veya ProtocolInternal)
```

---

### 1.11 src/trade_finding/trade_estimator.rs — TradeEstimator

**Amaç:** `TradeFinding` ve opsiyonel `TradeVerifying` kullanarak
`PriceEstimating` implement eder.

```rust
pub struct TradeEstimator {
    inner: Arc<Inner { finder, verifier }>,
    rate_limiter: Arc<RateLimiter>,
}
```

Akis:
```
verifier varsa:
  finder.get_trade() → verifier.verify() → Estimate { verified: true }
verifier yoksa:
  finder.get_quote() → Estimate { verified: false }
```

---

### 1.12 src/trade_verifier.rs — TradeVerifying (Ozet)

**Amaç:** Bir trade'in gercekten calistigini EVM simulation ile dogrular.

#### `TradeVerifying` Trait:

```rust
pub trait TradeVerifying: Send + Sync + 'static {
    async fn verify(
        &self,
        query: &PriceQuery,
        verification: &Verification,
        trade: TradeKind,
    ) -> Result<Estimate>;
}
```

Simulation akisi:
1. `TradeKind` → calldata
2. `eth_call` (state_override ile)
3. Sonuc `out_amount` beklenenle eslesirse `verified: true`

State override: gercek zincir durumu yerine simule edilmis durum kullanilir.
Bu sayede real balance gerektirmez.

**DEX Uygulama Notu — Nova Shield icin:**
Quote dogrulama (verification) Nova Shield icin kritik guvenlik ozelligi.
Solver'larin mantiksal olarak dogru ama pratik olarak calismaayan quote'lar
vermesini engeller. Her dogrulanmis quote simulate edilmis demektir.

---

### 1.13 src/instrumented.rs — InstrumentedPriceEstimator

Her estimate/estimate_native_price cagrisina Prometheus metrik ekler:

```rust
#[metric(labels("estimator_type", "result"))]
price_estimates: IntCounterVec,

#[metric(labels("estimator_type"))]
price_estimation_times: HistogramVec,
```

Sonuc etiketleri: `success`, `no_liquidity`, `unsupported_token`, `rate_limited`,
`trading_outside_allowed_window`, `token_temporarily_suspended`,
`insufficient_liquidity`, `custom_solver_error`, `estimator_internal_error`,
`protocol_internal_error`

---

### 1.14 src/external.rs — ExternalPriceEstimator

Driver HTTP endpoint'i uzerinden fiyat tahmin eden wrapper:

```rust
pub struct ExternalPriceEstimator(TradeEstimator);

impl ExternalPriceEstimator {
    pub fn new(driver: Url, client, rate_limiter, block_stream) -> Self
    pub fn verified(&self, verifier: Arc<dyn TradeVerifying>) -> Self
}
```

`verified()` ile `TradeVerifying` eklenebilir (quote simulation etkinlestirilir).

---

## 2. CRATE: pool-indexer/

### 2.1 Genel Mimari

`pool-indexer` UniswapV3 havuzlarini bir PostgreSQL veritabanina indeksleyen
bagimsiz bir servisdir. Driver tarafindan CLMM (Concentrated Liquidity Market Maker)
likidite verisi icin sorgulanir.

```
UniswapV3Factory (PoolCreated log) → yeni pool tespiti
UniswapV3Pool (Initialize, Swap, Mint, Burn) → pool state guncellemesi
                |
                v
    PostgreSQL (uniswap_v3_pools, uniswap_v3_pool_states, ticks)
                |
                v
    HTTP API (/api/v1/uniswap_v3/pools, /ticks, /bulk_ticks)
```

### 2.2 src/indexer/uniswap_v3.rs — Ana Indexleyici

#### Temel Tipler:

```rust
pub struct NewPoolData {
    address, token0, token1,
    fee: u32,              // 3000 = 0.3%
    token0_decimals, token1_decimals,
    token0_symbol, token1_symbol,
    created_block: u64,
}

pub struct PoolStateData {
    pool_address, block_number,
    sqrt_price_x96: U160,  // Q64.96 formatinda fiyat
    liquidity: u128,
    tick: i32,
}

pub struct LiquidityUpdateData {
    pool_address, block_number,
    liquidity: u128,  // Swap/Initialize olmayan Mint/Burn icin
}

pub struct TickDeltaData {
    pool_address,
    tick_idx: i32,
    delta: i128,  // Mint: +amount, Burn: -amount
}
```

#### sqrtPriceX96 Formulu:

```
sqrt_price = sqrtPriceX96 / 2^96
price = (sqrtPriceX96 / 2^96)^2
```

Bu Q64.96 fixed-point format (64 bit tamsayi, 96 bit kesir kismi).

#### ChunkChanges Struct:

Tek bir log blogundan elde edilen tum DB degisiklikleri:
```rust
struct ChunkChanges {
    new_pools: Vec<NewPoolData>,
    pool_states: Vec<PoolStateData>,
    liquidity_updates: Vec<LiquidityUpdateData>,
    tick_deltas: Vec<TickDeltaData>,
}
```

Atomik DB yazimi: chunk icerideki tum degisiklikler tek transaction'a yazilir.

#### `BasePoolStates` Optimization:

`Mint`/`Burn` eventi geldiginde ayni chunk'ta daha once `Swap`/`Initialize` yoksa
pool'un mevcut durumu gereklidir. Bu durum onceden DB'den yuklenir (`BasePoolStates`
HashMap), boylece her event icin DB sorgusu yapilmaz.

#### Event Isleme Sirasi:

1. `PoolCreated` → `NewPoolData`
2. `Initialize` → `PoolStateData` (ilk sqrtPrice ve tick)
3. `Swap` → `PoolStateData` (yeni sqrtPrice, tick, liquidity)
4. `Mint` → `TickDeltaData` (tick +) + mevcut liquiditiy guncelleme
5. `Burn` → `TickDeltaData` (tick -) + mevcut liquidity guncelleme

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield DEX, CLMM havuzlarini (UniswapV3 benzeri) destekleyecekse bu
indexleyici mimarisi dogrudan uygulanabilir. Kritik noktalar:
- sqrtPriceX96 / 2^96 formatinin dogru islenmesi
- Tick delta birikimi (Mint+/Burn-)
- Checkpoint/resume mekanizmasi (asagida)

### 2.3 Checkpoint/Resume Mekanizmasi

```
last_indexed_block DB'de saklanir
Servis baslatildiginda: son checkpoint'ten devam
Bootstrap (subgraph seeding) ile ilk yuklemede hizlanma
```

Bu sayede pool-indexer yeniden baslatildiginda sifirdan baslamaz.

### 2.4 HTTP API (src/api/)

| Endpoint | Aciklama |
|---|---|
| GET /api/v1/uniswap_v3/pools | Tum indexlenmis poollar |
| GET /api/v1/uniswap_v3/pools/{id}/ticks | Pool tick'leri |
| POST /api/v1/uniswap_v3/pools/bulk_ticks | Cok pool tick'leri |
| GET /api/v1/uniswap_v3/pools/by_ids | ID listesiyle pool sorgula |

Cursor pagination: `limit + after` parametreleriyle.

---

## 3. CRATE: refunder/

### 3.1 Genel Mimari

`refunder`, EthFlow kontratinda olusturdugu emirleri iptal olan kullanicilara
ETH'lerini iade eden bir servisdir.

```
DB → gecmis/iptal EthFlow emirleri
  → on-chain durum kontrolu (Refunded? Invalid? NotYetRefunded?)
  → NotYetRefunded ise: batch iade TX gonder
```

### 3.2 src/refund_service.rs — RefundService

#### Ana Sabitler:

```rust
const MAX_NUMBER_OF_UIDS_PER_REFUND_TX: usize = 30; // gas limiti
const BASIS_POINT_DENOMINATOR: f64 = 10_000.0;
```

30 UID siniri: bir tek TX'in gas limiti icerisinde kalabilecek maksimum sayı.

#### OrderUID Yapisi:

```
Byte [0..32]:  order hash
Byte [32..52]: EthFlow kontrat adresi (siparis bu kontrat tarafindan olusturuldu)
Byte [52..56]: valid_to timestamp (big-endian u32)
```

`extract_known_ethflow_address`: UID'nin [32..52] byte'larindan kontrat adresini
cikartir ve allowlist'te olup olmadigini kontrol eder.

#### `RefundStatus` Enum:

```
Refunded            — zaten iade edilmis, atla
Invalid             — sahibi ETH alamiyor (EOF kontrat, receive() yok)
NotYetRefunded(owner) — iade bekliyor, owner adresi dondurulur
```

#### `can_receive_eth` Kontrolu:

Owner EOF kontrat ise veya ETH almayan bir kontratsa `RefundStatus::Invalid`
atanir ve batch'den cikarilir. Bu olmadan batch TX revert eder.

Gercek Sepolia test ornegi (koda gomulu):
- `0x66C9152339ce05EE0C8A8eff9EeF8230AbFe8350`: EOF kontrat, ETH alamaz
- `0x5b485e4431853F82d89dba68220A422CC17cE024`: Normal EOA, ETH alabilir

#### Guvensiz Durum (Dokumante Edilmis Bug):

```
test_send_out_refunding_tx_db_error_skips_order:
  - uid1 DB hatasi: order data yok
  - uid2 DB basarili
  - submitter.submit_batch(uids=[uid1,uid2], orders=[order2_data], ...)
```

UIDs ve orders listesi esit uzunlukta olmayabilir! Bu mevcut davranis
dokumante edilmis ama TODO olarak isaretlenmis. Asil fix:
1. Empty orders ise gonderme
2. Basarisiz UID'leri listeden cikar
3. Tamamen basarisiz ise hata dondur

**DEX Uygulama Notu — Nova Shield icin:**
Nova Shield'da native ETH ile yapilan emirlerin iptal ve iade mekanizmasi
gerekli ise bu service dogrudan kullanilabilir. Kritik ozellik: iade oncesi
`can_receive_eth` kontrolu — kontrata ETH gonderilmesi her zaman basarili
olmaz.

#### `RefundService<D, CR, CW>` Generic Yapisı:

```rust
pub struct RefundService<D: DbRead, CR: ChainRead, CW: ChainWrite> {
    pub database: D,
    pub chain: CR,
    pub submitter: CW,
    pub min_validity_duration: i64,
    pub min_price_deviation_bps: i64,
}
```

Full generic: test'te mock injection kolayligi saglayan klasik DI pattern.

#### Test Coverage (rstest parametrik):

| Test | Parametre |
|---|---|
| test_eth_receivability_filtering | EOA (true), ContractRejectingETH (false) |
| test_ethflow_contract_filtering | KNOWN vs UNKNOWN contract |
| test_non_refundable_status_excludes_order | Refunded, Invalid, None (hata) |
| test_send_out_refunding_tx_order_count_boundary | 29, 30, 31, 35, 60 UID |
| test_contract_address_extraction_from_uid | KNOWN_ETHFLOW, KNOWN_ETHFLOW_2, ZERO, 0xFF |

---

## 4. Ortaya Cikan Invariantlar

### 4.1 Fiyat Tahmin Invariantlari

| No | Invariant | Kaynak |
|---|---|---|
| I1 | `is_price_malformed`: sifir, negatif, subnormal, veya U256 tasma fiyat kabul edilmez | native/mod.rs |
| I2 | `NativePriceEstimator` her zaman `OrderKind::Buy` kullanir (shallow liquidity baskisi az) | native/mod.rs |
| I3 | EIP-4626 non_vault_tokens cache'i sadece "contract revert + decimals OK" durumunda | eip4626.rs |
| I4 | FallbackEstimator: `ProtocolInternal` degil `NoLiquidity` fallback tetiklemez | fallback.rs |
| I5 | FallbackEstimator probe: `last_probe = now()` onceden set edilir (thundering herd onleme) | fallback.rs |
| I6 | CompetitionEstimator: Stage icinde paralel, stage'ler arasi sirali | competition/mod.rs |
| I7 | BufferedRequest: subscribe BEFORE send (race condition onleme) | buffered.rs |
| I8 | TradeVerifier'dan gelen Estimate her zaman `verified: true` | trade_verifier.rs |
| I9 | WETH<->ETH trivial tahminleri SanitizedEstimator kendi uretir, iletmez | sanitized.rs |
| I10 | Gas overflow: `GAS_PER_WETH_UNWRAP + u64::MAX` → ProtocolInternal (overflow checked) | sanitized.rs |

### 4.2 Pool Indexer Invariantlari

| No | Invariant | Kaynak |
|---|---|---|
| P1 | sqrtPriceX96 her zaman Q64.96 formatinda (2^96 boleni) | indexer/uniswap_v3.rs |
| P2 | TickDelta: Mint +amount, Burn -amount (imzali i128) | indexer/uniswap_v3.rs |
| P3 | ChunkChanges atomik olarak DB'ye yazilir (tek transaction) | indexer/uniswap_v3.rs |

### 4.3 Refunder Invariantlari

| No | Invariant | Kaynak |
|---|---|---|
| R1 | UID[32..52] = EthFlow kontrat adresi | refund_service.rs |
| R2 | MAX 30 UID per TX (gas limiti) | refund_service.rs |
| R3 | `can_receive_eth` False ise Invalid (batch revert onleme) | refund_service.rs |
| R4 | Unknown EthFlow kontrat adresleri refund edilmez (allowlist) | refund_service.rs |

---

## 5. DEX Uygulama Notu — Nova Shield

### 5.1 Price Estimation Motor Mimarisi

Nova Shield DEX price estimation motoru su katmanlardan olusacak:

```
Kullanici Quote Istegi
       |
SanitizedPriceEstimator (deny-list + native ETH handling)
       |
CompetitionEstimator<T> (stage-based kaynak yarisimi)
       |
TradeEstimator (TradeFinding + opsiyonel verification)
       |
[Nova Solver, External AMM APIs, On-chain AMM direct query]
```

### 5.2 Kritik Implementasyon Maddeleri

1. **Error taxonomy:** PriceEstimationError hiyerarsisi ve error_to_integer_priority
   mantigi Nova Shield'a birebir kopyalanmalidir. Hata onceligi, quote API'sinin
   dogru hata mesajlari dondurmesi icin kritiktir.

2. **Gas constants:** `gas.rs` sabitlerini referans al:
   - WETH unwrap: 9,223 gas
   - WETH wrap: 24,038 gas
   - Single trade settlement: 94,891 gas
   - ERC20 transfer medyani: 27,513 gas

3. **EIP-4626 pricing:** Vault token desteği icin `eip4626.rs` formulu:
   `convertToAssets(10^vault_dec) / 10^vault_dec * asset_native_price`

4. **FallbackEstimator:** Primary down oldugunda otomatik fallback, 60s probe.
   Threshold: 3 ardisik ProtocolInternal hatasi.

5. **Buffered batching:** Yuksek yuk altinda native fiyat isteklerini batch haline
   getiren `BufferedRequest` pattern'i benimsenmeli.

6. **Trade verification:** Sadece out_amount degil, simulate edilmis calldata
   ile dogrulama yapilmali. Verified quote'lar fee discount alabilir.

7. **CLMM pool indexing:** UniswapV3-benzeri havuzlar icin tick-based indexleme,
   sqrtPriceX96 / 2^96 formulu ve atomik DB yazimi uygulanmalidir.

### 5.3 Nova Shield'a Ozgu Gelistirmeler

- **ZK-verified quotes:** Nova Shield ZK proof altyapisi ile quote simulation
  sonuclari zk-snarki olarak dogrulanabilir.
- **MEV-aware pricing:** Solver'lar arasindan en dusuk MEV maruziyetine gore
  siralama (sadece en iyi fiyat degil).
- **Privacy-preserving quotes:** ZK yaklasimi ile kullanicinin trade buyuklugu
  solver'lardan gizlenebilir (batch blindly).

---

## 6. Ozet Istatistikler

| Metrik | Deger |
|---|---|
| Analiz edilen dosya | 54 (price-estimation: 27, pool-indexer: 18, refunder: 9) |
| Tahmini toplam satir | ~10,200 |
| Onemli trait sayisi | 7 (PriceEstimating, StreamingPriceEstimating, NativePriceEstimating, TradeFinding, TradeVerifying, NativePriceBatchFetching, DbRead/ChainRead/ChainWrite) |
| Gas sabitleri belgendi | 10 (gas.rs tum sabitler) |
| Invariant sayisi | 17 (10 price-est, 3 pool-indexer, 4 refunder) |
| DEX uygulama maddeleri | 7 kritik + 3 Nova Shield ozgu |

**Bu agent, CoW Protocol cow-protocol-analysis.md icin planlanan 35+ agentin
21. agentidir. Kalan analizler (eger varsa) ozet/master dosyasinda birlestirilecektir.**
