# CoW Protocol — Solvers Crate Tam Analiz (agent-06)

**Kapsam**: `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/libs/cow-protocol/crates/solvers/src/` — tüm dosyalar
**Toplam**: 78 dosya, ~12.454 satır
**Tarih**: 2026-07-22
**Analist**: agent-06 (Rust + DEX Güvenlik Uzmanı)

---

## Mimari Genel Bakış

CoW Protocol solver crate'i, bir `solve` HTTP endpoint'i sunan bağımsız bir ikili (binary) uygulamadır. Driver tarafından çağrılır ve batch auction içindeki emirleri (order) belirli bir süre içinde çözmeye çalışır.

```
Driver → POST /solve → API Layer (axum)
                          ↓
                    DTO deserialization (api/routes/solve/dto/)
                          ↓
                    Domain models (domain/)
                          ↓
               ┌──────────────────────────┐
               │    Solver::solve()        │
               ├──────────────────────────┤
               │  Baseline: path-finding   │
               │  Dex: OKX / Bitget API    │
               └──────────────────────────┘
                          ↓
                    Solution domain
                          ↓
                    DTO serialization
                          ↓
               Driver ← JSON response
```

**Katmanlar:**
- `api/` — HTTP sunucu, DTO dönüşümleri
- `domain/` — iş mantığı (solver, order, solution, liquidity)
- `boundary/` — domain → shared crate adaptörleri
- `infra/` — CLI, config, dış API istemcileri (OKX, Bitget), gas simülatörü
- `util/` — HTTP yardımcıları, BigDecimal dönüşümleri
- `tests/` — E2E test altyapısı

---

## 1. main.rs (12 satır)

**Konum**: `src/main.rs`

**İşlev**: Binary entry point.

```rust
#[cfg(feature = "jemalloc-allocator")]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(feature = "mimalloc-allocator")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    solvers::start(std::env::args())
}
```

**Tasarım Kararları:**
- jemalloc varsayılan allocator — heap profiling desteği
- mimalloc opsiyonel feature flag ile açılır
- `std::env::args()` doğrudan geçirilir → test ortamında custom args enjekte edilebilir

**Güvenlik Notu**: Memory allocator seçimi production davranışını doğrudan etkiler. jemalloc thread-local arenaları ile daha iyi multi-core performans sunar; yüksek concurrent solver yükü altında önemlidir.

**DEX Uygulama Notu — Nova Shield**: Nova Shield solver binary'si de aynı pattern'i izlemeli. jemalloc, batch işlemlerinde bellek fragmentasyonunu azaltır. `mimalloc` düşük-gecikme senaryoları (MEV koruması) için düşünülebilir.

---

## 2. lib.rs (14 satır)

**Konum**: `src/lib.rs`

```rust
#![allow(dead_code)]
#![recursion_limit = "256"]

pub mod api;
pub mod boundary;
pub mod domain;
pub mod infra;
pub mod run;
pub mod tests;
pub mod util;

pub use run::{run, start};
```

**Tasarım Kararları:**
- `#![allow(dead_code)]` — aktif geliştirme sürecini yansıtır; kütüphane olarak kullanıldığında uyarı bastırmak için
- `recursion_limit = "256"` — serde derive macro'larının derin tip ağaçlarında compile-time overflow'u önler
- Sadece `run` ve `start` public'e re-export edilir; mimari sınırlar net tutulur

---

## 3. run.rs (94 satır)

**Konum**: `src/run.rs`

**Struct'lar & Fonksiyonlar:**

```rust
pub async fn start(args: impl IntoIterator<Item = String>)
pub async fn run(args: impl IntoIterator<Item = String>)
async fn run_with(args: impl IntoIterator<Item = String>, bind: Option<oneshot::Sender<SocketAddr>>)
```

**Akış:**
1. `infra::cli::Args::parse_from(args)` — CLI parse
2. `contracts::Contracts::for_chain()` veya `for_chain_id()` — chain belirleme
3. `Command::Baseline | Okx | Bitget` match — solver instantiation
4. `infra::api::serve(solver, addr, bind)` — HTTP serve
5. `tokio::signal`: SIGINT/SIGTERM bekle → graceful shutdown

**Önemli Detay**: Windows'ta sadece `ctrl_c` sinyal dinleme yapılır; Unix sinyalleri (SIGTERM) sadece Unix platforma özel:
```rust
#[cfg(unix)]
{
    let mut stream = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = stream.recv() => {}
    }
}
```

**DEX Uygulama Notu — Nova Shield**: Solver process yönetimi için aynı graceful shutdown mekanizması uygulanmalı. Özellikle aktif `eth_call` simülasyonları sırasında abrupt shutdown beklenmedik sonuçlar üretebilir.

---

## 4. domain/auction.rs (104 satır)

**Konum**: `src/domain/auction.rs`

**Ana Struct:**
```rust
pub struct Auction {
    pub id: Option<Id>,
    pub tokens: HashMap<eth::TokenAddress, Token>,
    pub orders: Vec<order::Order>,
    pub liquidity: Vec<liquidity::Liquidity>,
    pub gas_price: GasPrice,
    pub deadline: Deadline,
}
```

**Token Struct:**
```rust
pub struct Token {
    pub decimals: Option<u8>,
    pub symbol: Option<String>,
    pub reference_price: Option<Price>,
    pub available_balance: eth::Ether,
    pub trusted: bool,
}
```

`trusted: bool` kritik bir güvenlik invariant'ıdır — sadece güvenilir tokenlar internalizasyon (settlement buffer kullanımı) için uygun sayılır.

**Price::ether_value() Formülü:**
```rust
pub fn ether_value(&self, amount: eth::Ether) -> Option<eth::Ether> {
    // price = ETH/token
    // ether_value = amount * price.numer / price.denom
    Some(eth::Ether(amount.0.checked_mul(*self.0.numer())? / *self.0.denom()))
}
```

**Matematiksel Detay**: `Price` bir `eth::Rational` — `numer/denom` olarak temsil edilen ETH/token oranı. `ether_value(amount)` = `amount * numer / denom`. Önce çarpma, sonra bölme yapılır; bu intermediate overflow riski taşır ama `checked_mul` ile korunur.

**Deadline Struct:**
```rust
pub struct Deadline(pub Instant);

impl Deadline {
    pub fn remaining(&self) -> Option<Duration> {
        self.0.checked_duration_since(Instant::now())
    }
    pub fn reduce(&self, amount: Duration) -> Self {
        Self(self.0 - amount)
    }
}
```

**DEX Uygulama Notu — Nova Shield**: `Deadline::reduce()` pattern'i kritik — solver'a iletilen deadline, toplamın birazı düşürülerek aktarılır. Bu, solver'ın zaman ayarlamasını kolaylaştırır ve latency buffer sağlar.

---

## 5. domain/order.rs (162 satır)

**Konum**: `src/domain/order.rs`

**Ana Struct:**
```rust
pub struct Order {
    pub uid: Uid,           // 56 bayt: hash(32) + owner(20) + valid_to(4)
    pub sell: eth::Asset,
    pub buy: eth::Asset,
    pub side: Side,         // Buy | Sell
    pub class: Class,       // Market | Limit | Liquidity
    pub partially_fillable: bool,
    pub flashloan_hint: Option<FlashloanHint>,
    pub pre_interactions: Vec<interaction::Custom>,
    pub post_interactions: Vec<interaction::Custom>,
    pub sell_token_balance: SellTokenBalance,
    pub buy_token_balance: BuyTokenBalance,
    pub app_data: AppData,
    pub signature: Signature,
}
```

**UID Yapısı (56 bayt):**
- Byte 0-31: order hash (EIP-712 imzalı yapıdan türetilen hash)
- Byte 32-51: owner adresi (20 bayt)
- Byte 52-55: valid_to Unix timestamp (4 bayt, big-endian)

**Side enum:**
```rust
pub enum Side {
    Buy,   // Tam olarak buy_amount token almak isteniyor
    Sell,  // Tam olarak sell_amount token satmak isteniyor
}
```

**Class enum:**
```rust
pub enum Class {
    Market,    // Piyasa emri
    Limit,     // Limit fiyatlı emir
    Liquidity, // Likidite sağlayan emir (internal)
}
```

**Signature türleri:**
```rust
pub enum Signature {
    Eip712(Vec<u8>),
    EthSign(Vec<u8>),
    Eip1271 {
        verifier: Address,
        signature: Vec<u8>,
    },
    PreSign(Address),
}
```

**JitOrder Struct** (Just-In-Time özel piyasa yapıcı emirleri):
```rust
pub struct JitOrder {
    pub sell: eth::Asset,
    pub buy: eth::Asset,
    pub receiver: eth::Address,
    pub valid_to: u32,
    pub app_data: AppData,
    pub side: Side,
    pub partially_fillable: bool,
    pub signature: Signature,
}
```

JIT emirler solver tarafından oluşturulabilir. Özel piyasa yapıcıların likidite enjekte etmesini sağlar.

**FlashloanHint:**
```rust
pub struct FlashloanHint {
    pub lender: eth::ContractAddress,
    pub token: eth::TokenAddress,
    pub amount: eth::Ether,
}
```

**DEX Uygulama Notu — Nova Shield**: Nova Shield'in order sistemi bu taxonomy'yi benimsemeli. `Class::Liquidity` emri, sistem içi book match'i sağlar. JIT pattern, OTC masaları veya özel piyasa yapıcıların dahil edilmesinde kullanılabilir.

---

## 6. domain/solution.rs (486 satır) — EN KRITIK DOSYA

**Konum**: `src/domain/solution.rs`

Bu dosya solver crate'inin en kritik bileşenidir. Bir çözümün tam temsili burada yer alır.

**Ana Struct:**
```rust
pub struct Solution {
    pub id: Id,
    pub prices: ClearingPrices,
    pub trades: Vec<Trade>,
    pub interactions: Vec<Interaction>,
    pub score: eth::Ether,
    pub gas: Option<Gas>,
}
```

**ClearingPrices:**
```rust
pub struct ClearingPrices(pub HashMap<eth::TokenAddress, U256>);
```

Tüm emirlerin aynı fiyatla temizleneceği uniform clearing price vektörü. Bu, MEV korumasının temelidir — arbitraj karı tamamen kullanıcılara surplus olarak iade edilir.

**Matematiksel İlişki**: Emir `i` için limit kontrolü:
```
sell_amount * prices[buy_token] >= buy_amount * prices[sell_token]
```

**Trade enum:**
```rust
pub enum Trade {
    Fulfillment(Fulfillment),
    Jit(JitTrade),
}
```

**Fulfillment Struct:**
```rust
pub struct Fulfillment {
    pub order: order::Order,
    pub executed: eth::Asset,
    pub fee: Fee,
}
```

**Fee enum:**
```rust
pub enum Fee {
    Protocol,
    Surplus {
        factor: f64,
        max_volume_factor: f64,
    },
}
```

**Gas Sabitleri (kritik):**
```rust
pub const INITIALIZATION_COST: u64 = 32_000;
pub const SETTLEMENT: u64 = 7_365;
pub const ERC20_TRANSFER: u64 = 27_513;
```

`INITIALIZATION_COST` sadece bir kez eklenir, `ERC20_TRANSFER` her token hareketi için eklenir.

**with_buffers_internalizations() — KRİTİK ALGORİTMA:**

Buffer internalization:
- Settlement contract'ta önceden tutulan tokenları kullanır
- ERC-20 transfer sayısını azaltır → gas tasarrufu
- Sadece `trusted` token'lar için uygulanır

**into_solution() Gas Hesaplama:**
```
gas = order.pre_interactions * gas_per_interaction
    + INITIALIZATION_COST (ilk kullanım)
    + SETTLEMENT
    + allowance (eğer gerekiyorsa) * approval_gas
    + swap_gas
    + ERC20_TRANSFER * transfer_count
    + order.post_interactions * gas_per_interaction
```

**DEX Uygulama Notu — Nova Shield:**
1. `ClearingPrices` yapısı Nova Shield için şart — uniform clearing price olmadan MEV koruması mümkün değil
2. `Fee::Surplus` mekanizması ideal — kullanıcının surplus'unu paylaş ama garanti et
3. Gas sabitleri onchain measurement ile validate edilmeli
4. Buffer internalization için settlement contract'ta aynı trust model uygulanmalı

---

## 7. domain/notification.rs (62 satır)

**Konum**: `src/domain/notification.rs`

Driver'ın solver'a gönderdiği bildirimler:

```rust
pub enum Notification {
    Timeout,
    EmptySolution,
    DuplicatedSolutionId,
    SimulationFailed(eth::BlockNo, eth::Tx, SimulationError),
    ScoringFailed(ScoreKind),
    NonBufferableTokensUsed(HashSet<eth::TokenAddress>),
    Settled(Settlement),
}

pub enum Settlement {
    Success(eth::TxId),
    Revert(eth::TxId),
    SimulationRevert,
    Fail,
}

pub enum ScoreKind {
    ObjectiveValueNonPositive,
    ZeroScore,
    ScoreHigherThanObjective,
    SuccessProbabilityOutOfRange,
}
```

Bu bildirimler solver'ın learning/adaptation mekanizmaları için kullanılır (rate limiter backoff, fill miktarı ayarı).

**DEX Uygulama Notu — Nova Shield**: Bu feedback loop Nova Shield için kritik. `SimulationFailed` → fill miktarını azalt bağlantısı, sistemi kendi kendine adapte edici yapar.

---

## 8. domain/eth/mod.rs (99 satır)

**Konum**: `src/domain/eth/mod.rs`

Ethereum primitive newtype'ları:

```rust
pub struct TokenAddress(pub H160);
pub struct WethAddress(pub H160);
pub struct ContractAddress(pub H160);
pub struct TxId(pub H256);
pub struct BlockNo(pub u64);
pub struct Ether(pub U256);
pub struct Asset { pub token: TokenAddress, pub amount: Ether }
pub struct Gas(pub U256);
pub struct SignedGas(pub I256);
pub struct Rational(num::rational::Ratio<U256>);
```

**Gas saturating aritmetik:**
```rust
impl Add for Gas {
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}
```

`saturating_add/sub` — gas hesaplamalarında overflow/underflow panic yerine sınırda kalınır.

**DEX Uygulama Notu — Nova Shield**: Her Ethereum primitive için newtype pattern kullanılmalı — token'ı ETH olarak geçirme gibi tip hataları compile-time'da yakalanır.

---

## 9. domain/eth/chain.rs (186 satır)

**Konum**: `src/domain/eth/chain.rs`

```rust
pub enum ChainId {
    Mainnet = 1,
    Gnosis = 100,
    Base = 8453,
    ArbitrumOne = 42161,
    Bnb = 56,
    Avalanche = 43114,
    Optimism = 10,
    Polygon = 137,
    Linea = 59144,
    Plasma = 9745,
    Ink = 57073,
}
```

**Serde desteği**: Hem `"1"` (string) hem `1` (integer) olarak parse eder. Bu, farklı API'lerin chain_id'yi farklı formatlarda göndermesini tolere eder.

**DEX Uygulama Notu — Nova Shield**: Multi-chain deployment planlanıyorsa bu enum template olarak kullanılabilir. Nova Shield genesis chain ID olarak CoW'da bulunmayan bir değer seçmeli.

---

## 10. domain/liquidity/mod.rs (80 satır)

**Konum**: `src/domain/liquidity/mod.rs`

```rust
pub struct Liquidity {
    pub id: Id,
    pub gas: eth::Gas,
    pub state: State,
}

pub enum State {
    ConstantProduct(constant_product::Pool),
    WeightedProduct(weighted_product::Pool),
    Stable(stable::Pool),
    Concentrated(concentrated::Pool),
    LimitOrder(limit_order::LimitOrder),
}
```

**TokenPair — Kanonik Sıralama:**
```rust
pub struct TokenPair(eth::TokenAddress, eth::TokenAddress);

impl TokenPair {
    pub fn new(a: eth::TokenAddress, b: eth::TokenAddress) -> Option<Self> {
        if a == b { return None; }
        if a.0 < b.0 { Some(Self(a, b)) } else { Some(Self(b, a)) }
    }
}
```

Kanonik sıralama, aynı token çiftinin farklı sıralamayla temsil edilmesini önler — HashMap lookup'ları için kritik.

---

## 11. domain/liquidity/constant_product.rs (61 satır)

**Konum**: `src/domain/liquidity/constant_product.rs`

Uniswap V2 constant product AMM modeli:

```rust
pub struct Pool {
    pub tokens: TokenPair,
    pub reserves: Reserves,
    pub fee: Fee,
}
```

**uint112 Cap:**
```rust
pub const RESERVE_MAX: U256 = U256::from_limbs([u112::MAX as u64, (u112::MAX >> 64) as u64, 0, 0]);
```

Uniswap V2 kontratı rezervleri `uint112` (2^112-1) olarak saklar.

**k() invariant:**
```rust
pub fn k(&self) -> U256 {
    let (a, b) = &self.reserves.get;
    a.amount.0 * b.amount.0  // k = x * y
}
```

**DEX Uygulama Notu — Nova Shield**: Nova Shield'in native AMM'i için bu model başlangıç noktasıdır. `uint112` cap, gerçek contract constraint'ini doğru yansıtır.

---

## 12. domain/liquidity/concentrated.rs (14 satır)

**Konum**: `src/domain/liquidity/concentrated.rs`

```rust
pub struct Pool {
    pub tokens: TokenPair,
    pub fee: Fee,
    pub id: Id,
}

pub struct Fee(pub u32);  // Uniswap V3 fee tier (500, 3000, 10000)
pub struct Id(pub Address);  // Pool contract adresi
```

Minimum veri — concentrated liquidity hesaplamaları doğrudan on-chain QuoterV2'ye delegate edilir.

---

## 13. domain/liquidity/weighted_product.rs (72 satır)

**Konum**: `src/domain/liquidity/weighted_product.rs`

Balancer V2 weighted pool:

```rust
pub struct Pool {
    pub reserves: HashMap<eth::TokenAddress, Reserve>,
    pub fee: eth::Rational,
    pub version: Version,
}

pub struct Reserve {
    pub asset: eth::Asset,
    pub weight: eth::Rational,
    pub scale: ScalingFactor,
}

pub enum Version {
    V0,
    V3Plus,
}
```

**Matematiksel İnvaryant (Weighted Product):**
```
Product(Bi^Wi) = constant    (i: her token için)
```

Balancer ağırlıklı havuzlarda fiyat oranı: `(B_out/W_out) / (B_in/W_in)`

---

## 14. domain/liquidity/stable.rs (59 satır)

**Konum**: `src/domain/liquidity/stable.rs`

Curve/Balancer StableSwap pool:

```rust
pub struct Pool {
    pub reserves: HashMap<eth::TokenAddress, Reserve>,
    pub fee: eth::Rational,
    pub amplification_parameter: eth::Rational,
}
```

**StableSwap Invariant:**
```
A * n^n * sum(xi) + D = A * D * n^n + D^(n+1) / (n^n * product(xi))
```
`A` (amplification_parameter): 1 → constant product, infinity → constant sum. Stablecoin pool'larda tipik A=100-2000.

`amplification_parameter: eth::Rational` — integer rational olarak tutulur, float değil. Numeric stability için kritik.

---

## 15. domain/liquidity/limit_order.rs (13 satır)

**Konum**: `src/domain/liquidity/limit_order.rs`

0x-style on-chain limit order likidite kaynağı:

```rust
pub struct LimitOrder {
    pub maker: eth::Asset,
    pub taker: eth::Asset,
    pub fee: TakerAmount,
}
```

Linear pricing: `out = in * maker.amount / taker.amount`

---

## 16. domain/dex/mod.rs (204 satır) — DEX SOLVER KALBİ

**Konum**: `src/domain/dex/mod.rs`

**Order Struct:**
```rust
pub struct Order {
    pub sell: eth::Asset,
    pub buy: eth::Asset,
    pub side: Side,
}
```

**Swap Struct:**
```rust
pub struct Swap {
    pub call: Call,
    pub input: eth::Asset,
    pub output: eth::Asset,
    pub allowance: Allowance,
    pub gas: eth::Gas,
}
```

**satisfies() — 512-bit Kritik Kontrol:**
```rust
pub fn satisfies(&self, order: &domain::order::Order) -> bool {
    // Limit fiyat kontrolü: output * sell_amount >= input * buy_amount
    // 512-bit widening multiply kullanarak overflow önlenir
    let lhs = U512::from(self.output.amount.0) * U512::from(order.sell.amount.0);
    let rhs = U512::from(self.input.amount.0) * U512::from(order.buy.amount.0);
    lhs >= rhs
}
```

**Matematiksel Gerekçe**: `output/input >= buy/sell` → `output * sell >= input * buy`. İki U256 çarpımı U512 gerektirir (256+256=512 bit).

**into_solution() Async Metodu:**
```rust
pub async fn into_solution(
    self,
    auction: &auction::Auction,
    order: &domain::order::Order,
    slippage: &slippage::Limits,
    gas_simulator: &infra::dex::Simulator,
) -> Option<solution::Single> {
    // 1. satisfies() kontrolü
    // 2. Slippage uygula
    // 3. Gas simüle et (eth_call)
    // 4. Solution oluştur
}
```

**DEX Uygulama Notu — Nova Shield:**
1. 512-bit widening multiply mutlak zorunluluk — U256 overflow ile yanlış limit kontrolü mümkün
2. `satisfies()` her DEX quote'undan sonra çalıştırılmalı
3. `Allowance` management on-chain gas'ı etkiler — approval caching şart

---

## 17. domain/dex/slippage.rs (268 satır) — KRITIK MEV KORUMA

**Konum**: `src/domain/dex/slippage.rs`

**SlippageLimits Struct:**
```rust
pub struct SlippageLimits {
    pub relative: Fraction,
    pub absolute: Option<eth::Ether>,
}
```

**TEMEL ALGORİTMA — Min(relative, absolute_as_relative):**
```rust
// effective_slippage = min(relative, absolute_in_token / token_amount)
let relative = limits.relative.to_big_decimal();
let absolute_as_relative = absolute_in_token / token_amount;
let effective = relative.min(&absolute_as_relative);
```

**compute_absolute_tolerance():**
```rust
fn compute_absolute_tolerance(amount: &BigUint, slippage: &BigDecimal) -> BigUint {
    // ceil(amount * slippage) — her zaman yukarı yuvarla (kullanıcı lehine)
    result.div_ceil(&slippage_denom)
}
```

`div_ceil` kullanılması önemli — slippage hesabında aşağı yuvarlama yetersiz koruma sağlar.

**DEX Uygulama Notu — Nova Shield:**
1. `min(relative, absolute_as_relative)` formülü Nova Shield için standart olmalı
2. Absolute slippage cap, küçük emirler için özellikle önemli
3. `BigDecimal` kullanımı şart — `f64` rounding error üretir

---

## 18. domain/dex/minimum_surplus.rs (141 satır)

**Konum**: `src/domain/dex/minimum_surplus.rs`

**TEMEL ALGORİTMA — Max(relative, absolute_as_relative):**
```rust
// minimum_surplus = max(relative, absolute_as_relative)
let effective = relative.max(&absolute_as_relative);
```

Slippage ve minimum surplus tamamlayıcı mekanizmalardır:
- Slippage: "en fazla bu kadar kötü fiyat kabul et"
- Minimum surplus: "en az bu kadar iyi fiyat al"

**DEX Uygulama Notu — Nova Shield**: Her iki mekanizma birlikte uygulanmalı. Limit emirlerde minimum surplus, piyasa emirlerinde slippage önceliklidir.

---

## 19. domain/dex/shared.rs (54 satır)

**Konum**: `src/domain/dex/shared.rs`

Slippage ve surplus modülleri arasında paylaşılan matematik:

```rust
pub fn compute_absolute_tolerance(amount: &BigUint, factor: &BigDecimal) -> BigUint {
    let (numer, denom) = factor.as_bigint_and_exponent();
    let scaled = amount * &numer;
    let ten_pow_scale = BigUint::from(10u64).pow(denom as u32);
    (scaled + &ten_pow_scale - 1u64) / ten_pow_scale  // ceil division
}

pub fn absolute_to_relative(absolute: eth::Ether, amount: eth::Ether) -> BigDecimal {
    BigDecimal::from(absolute.0) / BigDecimal::from(amount.0)
}
```

---

## 20. domain/solver/mod.rs (22 satır)

**Konum**: `src/domain/solver/mod.rs`

```rust
pub enum Solver {
    Baseline(Baseline),
    Dex(Box<Dex>),
}

impl Solver {
    pub async fn solve(&self, auction: &Auction) -> Vec<Solution> {
        match self {
            Self::Baseline(s) => s.solve(auction).await,
            Self::Dex(s) => s.solve(auction).await,
        }
    }
}
```

`Box<Dex>` — Dex struct büyük olduğu için heap'te tutulur, stack taşması önlenir.

---

## 21. domain/solver/baseline.rs (408 satır) — BASELINE SOLVER

**Konum**: `src/domain/solver/baseline.rs`

**DEADLINE_SLACK:**
```rust
const DEADLINE_SLACK: Duration = Duration::from_millis(500);
```

Solver, deadline'dan 500ms önce çalışmayı keser. Bu buffer, driver'a sonucu iletmek için zaman tanır.

**solve() — Ana Akış:**
```rust
pub async fn solve(&self, auction: &Auction) -> Vec<Solution> {
    let deadline = auction.deadline.reduce(DEADLINE_SLACK);

    // Her emir için ayrı tokio::spawn — CPU bound iş async runtime'ı bloklamasın
    let handles: Vec<_> = auction.orders.iter().map(|order| {
        let solver = self.clone();
        let order = order.clone();
        tokio::spawn(async move {
            solver.solve_order(&order, &auction, &deadline).await
        })
    }).collect();

    tokio::time::timeout_at(deadline.0, async {
        join_all(handles).await
    }).await
    .unwrap_or_default()
    .into_iter()
    .filter_map(|r| r.ok().flatten())
    .collect()
}
```

**requests_for_order() — Halving Iterator:**
```rust
fn requests_for_order(order: &Order) -> impl Iterator<Item = eth::Ether> {
    std::iter::successors(Some(order.sell.amount), |&prev| {
        if prev > minimum_fill {
            Some(prev / 2)
        } else {
            None
        }
    })
}
```

**native_price_request() — Büyük Sell Amount:**
```rust
fn native_price_request(token: eth::TokenAddress) -> Order {
    Order {
        sell: eth::Asset { token, amount: eth::Ether(U256::from(1u64) << 144) },
        // 2^144 — sadece ETH/token oranını bulmak için, execute edilmez
        ...
    }
}
```

**to_normalized_price() — Float→U256:**
```rust
fn to_normalized_price(amount_in: eth::Ether, amount_out: eth::Ether) -> Option<U256> {
    let price = (amount_out.0 as f64) / (amount_in.0 as f64) * 1e18;
    if price.is_finite() && price > 0.0 {
        Some(U256::from(price as u128))
    } else {
        None
    }
}
```

`f64` burada kabul edilebilir — bu sadece "reference price", settlement'ta kullanılmaz.

**DEX Uygulama Notu — Nova Shield:**
1. CPU-bound solver iş yükü için `tokio::spawn` şart
2. Halving iterator, partial fill ile çözüm başarısını artırır
3. 500ms DEADLINE_SLACK minimum değer; Nova Shield için ayarlanabilir parametre yapılabilir

---

## 22. domain/solver/dex/mod.rs (231 satır) — DEX SOLVER

**Konum**: `src/domain/solver/dex/mod.rs`

**Struct:**
```rust
pub struct Dex {
    dex: infra::Dex,
    config: Config,
    fills: fills::Fills,
    rate_limiter: RateLimiter,
    simulator: infra::Simulator,
}
```

**solve() — Paralel İşleme:**
```rust
pub async fn solve(&self, auction: &Auction) -> Vec<Solution> {
    futures::stream::iter(auction.orders.iter())
        .map(|order| self.solve_order(order, auction))
        .buffer_unordered(self.config.concurrent_requests)
        .filter_map(|r| async { r })
        .collect()
        .await
}
```

**`buffer_unordered`** — sabit concurrency window ile paralel işleme. `concurrent_requests` sayıda aktif request tutulur.

**solve_order() — Adaptive Akış:**
```rust
async fn solve_order(&self, order: &Order, auction: &Auction) -> Option<Solution> {
    if self.rate_limiter.is_limited() { return None; }
    let fill = self.fills.next_amount(order);

    match self.dex.swap(order_with_fill, &auction).await {
        Ok(swap) => { /* başarılı → increase */ }
        Err(Error::OrderNotFound) => {
            self.fills.reduce_next_try(order);
            return None;
        }
        Err(Error::RateLimited) => {
            self.rate_limiter.back_off();
            return None;
        }
        Err(_) => return None,
    }
}
```

**Rate Limiter — Exponential Backoff:**
- Başarısız: `current_back_off * 2` (max'a kadar)
- Başarılı: `min_back_off`'a resetle

**DEX Uygulama Notu — Nova Shield:**
1. `buffer_unordered` pattern, N API endpoint'ini aynı anda sorgulamak için ideal
2. Rate limiter exponential backoff ile API yasaklanma riskini azaltır
3. `fills.reduce_next_try` adaptive behavior — API sınırlarını öğrenir

---

## 23. domain/solver/dex/fills.rs (181 satır)

**Konum**: `src/domain/solver/dex/fills.rs`

**Fills Struct:**
```rust
pub struct Fills {
    cache: Mutex<HashMap<OrderUid, CacheEntry>>,
    smallest_fill: eth::Ether,
}

struct CacheEntry {
    next_amount: eth::Ether,
    total_amount: eth::Ether,
    last_requested: Instant,
}
```

**MAX_AGE = 10 dakika**: 10 dakika görülmeyen emirlerin cache girdileri temizlenir. Bellek sızıntısını önler.

**Adaptive Halving/Doubling:**
```rust
pub fn reduce_next_try(&self, order: &Order) {
    entry.next_amount = (entry.next_amount / 2).max(self.smallest_fill);
}

pub fn increase_next_try(&self, order: &Order, filled: eth::Ether) {
    entry.next_amount = (filled * 2).min(entry.total_amount);
}
```

**Binary Search Sezgisi**: Başarısız → yarıya in, başarılı → iki katına çıkar.

**U512 Widening — Kısmi Miktar Hesabı:**
```rust
fn scale_to_fill(total: eth::Ether, fill: eth::Ether, amount: eth::Ether) -> eth::Ether {
    // amount * fill / total — overflow için U512
    let result = U512::from(amount.0) * U512::from(fill.0) / U512::from(total.0);
    eth::Ether(U256::try_from(result).unwrap_or(U256::MAX))
}
```

---

## 24. boundary/mod.rs (8 satır)

**Konum**: `src/boundary/mod.rs`

```rust
pub mod baseline;
pub mod liquidity;
pub mod routing;

pub type Result<T> = anyhow::Result<T>;
```

`anyhow::Result` — boundary katmanı hataları için type alias. Domain'in kendi hata enum'ları yerine daha gevşek hata propagasyonu.

---

## 25. boundary/baseline.rs (332 satır) — DOMAIN→SHARED ADAPTÖR

**Konum**: `src/boundary/baseline.rs`

**Solver<'a> Struct:**
```rust
pub struct Solver<'a> {
    base_tokens: Arc<BaseTokens>,
    onchain_liquidity: HashMap<liquidity::Id, OnchainLiquidity<'a>>,
    liquidity: HashMap<liquidity::Id, &'a domain::liquidity::Liquidity>,
}
```

**OnchainLiquidity enum:**
```rust
enum OnchainLiquidity<'a> {
    ConstantProduct(shared::baseline::ConstantProductPool),
    WeightedProduct(shared::baseline::WeightedProductPool),
    Stable(shared::baseline::StablePool),
    Concentrated(shared::baseline::ConcentratedPool<'a>),
    LimitOrder(shared::baseline::LimitOrderPool),
}
```

**BaselineSolvable for OnchainLiquidity:**

Polymorphic dispatch ile farklı pool türlerinin aynı interface üzerinden sorgulanması:

```rust
impl BaselineSolvable for OnchainLiquidity<'_> {
    fn get_amount_out(&self, in_token, in_amount, out_token) -> Option<U256> {
        match self {
            Self::ConstantProduct(p) => p.get_amount_out(...),
            Self::WeightedProduct(p) => p.get_amount_out(...),
            Self::Stable(p) => p.get_amount_out(...),
            Self::Concentrated(p) => p.get_amount_out(...),
            Self::LimitOrder(p) => p.get_amount_out(...),
        }
    }
}
```

**DEX Uygulama Notu — Nova Shield**: Bu Adapter pattern multi-pool sistemi için şablondur. Nova Shield yeni pool türleri eklerken bu pattern'i izlemeli.

---

## 26. boundary/routing.rs (432 satır) — PATH FINDING

**Konum**: `src/boundary/routing.rs`

**DEFAULT_MAX_HOPS:**
```rust
const DEFAULT_MAX_HOPS: usize = 2;
```

Maksimum 3 token (A→B→C veya A→B→C→D) — 2 intermediate hop.

**estimate_buy_amount() — Forward Greedy:**
```rust
pub fn estimate_buy_amount(request: &Request, liquidity: &HashMap<Id, &Liquidity>) -> Option<Route> {
    let paths = base_tokens.path_candidates_with_hops(sell_token, buy_token, max_hops);
    paths.filter_map(|path| estimate_path_buy_amount(path, sell_amount, liquidity))
         .max_by_key(|route| route.buy_amount)
}
```

**estimate_sell_amount() — Backward Greedy:**
```rust
pub fn estimate_sell_amount(...) -> Option<Route> {
    paths.filter_map(|path| estimate_path_sell_amount(path, buy_amount, liquidity))
         .min_by_key(|route| route.sell_amount)
}
```

**BaseTokens::path_candidates_with_hops() — BFS:**
```rust
fn path_candidates_with_hops(
    &self,
    from: TokenAddress,
    to: TokenAddress,
    max_hops: usize,
) -> impl Iterator<Item = Vec<TokenAddress>> {
    // BFS: base token'lar (WETH, USDC, DAI vb.) üzerinden path'ler
    // Sonuç: [from, base1, to], [from, base2, to], [from, base1, base2, to], ...
}
```

**Gas Tahmini:**
```rust
pub fn estimate_gas(route: &Route) -> eth::Gas {
    let base: u64 = 50_000;
    let pool_costs: u64 = route.pools().map(|p| p.gas.0).sum();
    eth::Gas(U256::from(base + pool_costs))
}
```

Gas maliyetleri (yaklaşık):
- Constant Product (Uniswap V2): ~60,000
- Concentrated (Uniswap V3): ~106,000
- Weighted Product (Balancer V2): ~100,000
- Stable: ~150,000
- Limit Order: ~90,000

**Test Suite**: `direct_path`, `two_hop_path`, `no_path`, `better_path_wins`

**DEX Uygulama Notu — Nova Shield:**
1. BFS + greedy per-hop, near-optimal ve O(hops × pools) complexity
2. `max_hops=2` çoğu durumda yeterli; arttırmak exponential path count üretir
3. Forward (sell) ve backward (buy) ayrı algoritma — Side enum'a bağlı

---

## 27. boundary/liquidity/concentrated.rs (79 satır)

**Konum**: `src/boundary/liquidity/concentrated.rs`

```rust
pub const POOL_SWAP_GAS_COST: u64 = 106_000;
```

Dune Analytics ölçümlerinden elde edilen sabit.

**get_amount_out() — On-Chain RPC:**
```rust
async fn get_amount_out(&self, in_token, in_amount, out_token) -> Option<U256> {
    let params = QuoteExactInputSingleParams {
        sqrtPriceLimitX96: U256::ZERO,  // Fiyat limiti yok
        ...
    };
    self.contract.quoteExactInputSingle(params).call().await.ok()
        .map(|(amount_out, _, _, _)| amount_out)
}
```

`sqrtPriceLimitX96 = 0`: Fiyat sınırlaması olmadan tam quote. Gerçek swap'ta slippage limiti uygulanır.

---

## 28. boundary/liquidity/constant_product.rs (53 satır)

**Konum**: `src/boundary/liquidity/constant_product.rs`

Domain → shared Uniswap V2 dönüşümü:

```rust
pub fn from_domain(pool: &domain::constant_product::Pool) -> Option<shared::ConstantProductPool> {
    let reserve_0 = u128::try_from(pool.reserves.get.0.amount.0)
        .map_err(|_| tracing::debug!("U256 reserve > u128, skipping pool"))
        .ok()?;
    ...
}
```

`U256→u128` cast: Normal koşullarda uint112 << u128, güvenli dönüşüm.

---

## 29. boundary/liquidity/weighted_product.rs (64 satır)

**Konum**: `src/boundary/liquidity/weighted_product.rs`

**to_fixed_point() — Rational→Bfp:**
```rust
fn to_fixed_point(r: &eth::Rational) -> Option<Bfp> {
    // Bfp (Balancer Fixed Point): 10^18 olcekli
    let wei = r.numer().checked_mul(&U256::from(1_000_000_000_000_000_000u128))?
               .checked_div(r.denom())?;
    Some(Bfp::from_wei(wei))
}
```

`Bfp` Balancer kontratlarının 18 decimal fixed-point formatı.

---

## 30. boundary/liquidity/stable.rs (60 satır)

**Konum**: `src/boundary/liquidity/stable.rs`

```rust
fn amplification_parameter(pool: &domain::stable::Pool) -> Option<AmplificationParameter> {
    AmplificationParameter::try_new(
        pool.amplification_parameter.numer(),
        pool.amplification_parameter.denom(),
    ).ok()
}
```

---

## 31. boundary/liquidity/limit_order.rs (164 satır)

**Konum**: `src/boundary/liquidity/limit_order.rs`

**BaselineSolvable for LimitOrder — Lineer Fiyat:**
```rust
fn get_amount_out(&self, in_token, in_amount, out_token) -> Option<U256> {
    // out = in * maker_amount / taker_amount
    let out = in_amount
        .checked_mul(self.maker.amount)?
        .checked_div(self.taker.amount)?;
    Some(out)
}
```

**GAS_PER_ZEROEX_ORDER = 90,000**

5 unit test: `basic_fill`, `partial_fill`, `sell_side`, `buy_side`, `insufficient_fill`

---

## 32. infra/cli.rs (56 satır)

**Konum**: `src/infra/cli.rs`

```rust
#[derive(Parser)]
pub struct Args {
    #[clap(long, env, default_value = "127.0.0.1:7872")]
    pub addr: SocketAddr,
    #[clap(subcommand)]
    pub command: Command,
}

pub enum Command {
    Baseline,
    Okx(OkxArgs),
    Bitget(BitgetArgs),
}
```

Default port `7872` — CoW Protocol solver'larının standart portu.

---

## 33. infra/contracts.rs (43 satır)

**Konum**: `src/infra/contracts.rs`

```rust
pub struct Contracts {
    pub weth: WethContract,
    pub settlement: GPv2Settlement,
    pub authenticator: GPv2AllowListAuthentication,
}
```

CoW Protocol kontratları her desteklenen chain'de aynı adreste deploy edilmiştir (deterministic). `for_chain()` bu adresleri kullanır.

---

## 34. infra/metrics.rs (60 satır)

**Konum**: `src/infra/metrics.rs`

```rust
pub struct Metrics {
    pub time_limit: Histogram,      // 0-15s buckets
    pub remaining_time: Histogram,  // 0-15s buckets
    pub solve_requests: Counter,
    pub solve_errors: CounterVec,   // "reason" label ile
    pub solutions: Counter,
}
```

**Histogram Bucket'ları**: `[0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0]` saniye

`remaining_time` dağılımı, solver'ın deadline kullanım verimliliğini gösterir.

---

## 35. infra/config/baseline.rs (95 satır)

**Konum**: `src/infra/config/baseline.rs`

```rust
#[serde(deny_unknown_fields)]
pub struct Config {
    pub chain_id: Option<ChainId>,
    pub weth: Option<Address>,     // chain_id veya weth, biri zorunlu
    pub base_tokens: Vec<Address>,
    pub max_hops: Option<usize>,   // varsayılan 2
    pub gas_offset: Option<i64>,   // default: SETTLEMENT_OVERHEAD
}
```

`deny_unknown_fields` — typo'ları runtime'da yakalar.

---

## 36. infra/config/dex/file.rs (206 satır)

**Konum**: `src/infra/config/dex/file.rs`

```rust
pub struct Config {
    pub slippage: SlippageConfig,
    pub minimum_surplus: MinimumSurplusConfig,
    pub concurrent_requests: usize,       // varsayılan 1
    pub gas_offset: i64,                  // varsayılan 106_391
    pub smallest_partial_fill: Ether,     // varsayılan 0.01 ETH
    pub internalize_interactions: bool,    // varsayılan true
    pub min_back_off: Duration,           // varsayılan 1s
    pub max_back_off: Duration,           // varsayılan 8s
}
```

**gas_offset = 106_391**: Uniswap V3 swap gas maliyetine yakındır.

**internalize_interactions = true**: Her internalization ~27,513 gas (bir ERC-20 transfer) tasarrufu sağlar.

---

## 37. infra/config/dex/okx/file.rs (115 satır)

**Konum**: `src/infra/config/dex/okx/file.rs`

```rust
pub struct Config {
    pub sell_endpoint: Url,        // V6 endpoint
    pub buy_endpoint: Url,         // V5 endpoint
    pub project_id: String,
    pub api_key: String,
    pub api_secret_key: String,
    pub api_passphrase: String,
    pub chain_id: ChainId,
    pub price_impact_protection_percent: f64,  // varsayılan 1.0 = kapalı
}
```

`price_impact_protection_percent = 1.0` varsayılan olarak kapalı (100% = hepsi kabul). Açmak için 0.01-0.05 önerilir.

---

## 38. infra/config/dex/bitget/file.rs (85 satır)

**Konum**: `src/infra/config/dex/bitget/file.rs`

```rust
pub struct Config {
    pub endpoint: Url,
    pub chain_id: ChainId,
    pub api_key: String,
    pub api_secret: String,
    pub partner_code: String,      // varsayılan "cowswap"
    pub enable_buy_orders: bool,   // varsayılan false
}
```

`enable_buy_orders: false` varsayılan — Bitget buy order API'si deneysel.

---

## 39. infra/dex/mod.rs (122 satır)

**Konum**: `src/infra/dex/mod.rs`

```rust
pub enum Dex {
    Bitget(bitget::Bitget),
    Okx(okx::Okx),
}

pub enum Error {
    OrderNotFound,
    RateLimited,
    QuoteNotFound,
    SlippageExceeded,
    InsufficientOutput,
    Other(anyhow::Error),
}
```

**Block Hash Header:**
```rust
impl Client {
    pub fn request(&self, method: Method, url: Url) -> RequestBuilder {
        let mut builder = self.inner.request(method, url);
        if let Some(hash) = &self.block_hash {
            builder = builder.header("X-AUCTION-BLOCK-HASH", format!("{hash:?}"));
        }
        builder
    }
}
```

`X-AUCTION-BLOCK-HASH` — hangi block durumunda quote alındığını belirtir. Stale quote önleme için.

---

## 40. infra/dex/simulator.rs (121 satır)

**Konum**: `src/infra/dex/simulator.rs`

Gas simülatörü — `eth_call` + state override:

```rust
pub async fn simulate(&self, swap: &dex::Swap) -> Result<eth::Gas> {
    let state_override = StateOverride {
        [owner]: AccountOverride { code: Some(SWAPPER_BYTECODE), ... },
        [authenticator]: AccountOverride { code: Some(ANYONE_AUTH_BYTECODE), ... },
    };

    let gas = web3.eth().call_with_state_override(
        settlement_call,
        block,
        state_override,
    ).await?;

    if gas == 0 {
        Ok(swap.gas)  // Fallback: DEX API'nin bildirdiği gas
    } else {
        Ok(eth::Gas(U256::from(gas)))
    }
}
```

**Swapper Bytecode**: Minimal EVM bytecode, sadece swap call'unu simüle eder.
**AnyoneAuthenticator**: Simülasyonda settlement yetki kısıtlamasını kaldırır.
**`gas==0` fallback**: Bazı RPC provider'lar state override desteklemez.

**DEX Uygulama Notu — Nova Shield:**
1. State override simülasyonu, gas estimation'ın en doğru yöntemi
2. Kendi settlement contract için analogous bytecode gerekir

---

## 41. infra/dex/okx/mod.rs (516 satır) — OKX API İSTEMCİSİ

**Konum**: `src/infra/dex/okx/mod.rs`

**Okx Struct:**
```rust
pub struct Okx {
    client: infra::Client,
    config: Config,
    approval_cache: Cache<(TokenAddress, ApprovalSide), ContractAddress>,
}
```

`moka::Cache` — async-aware, TTL-destekli in-memory cache.

**handle_sell_order() — Paralel Swap + Approve:**
```rust
async fn handle_sell_order(&self, order: &Order, auction: &Auction) -> Result<Swap, Error> {
    let (swap_result, approve_result) = tokio::try_join!(
        self.get_swap(order, auction),
        self.cache_approval_address(order.sell.token, ApprovalSide::Sell),
    )?;

    let gas = swap_result.gas * 3 / 2;  // gas * 1.5 buffer

    Ok(Swap { ... })
}
```

**gas * 1.5 gerekçesi**: OKX'in gas tahmini konservatif değil. Gerçek execution'da fazla gas gerekebilir.

**handle_buy_order() — V5 API (Sequential)**: Buy orders için OKX V5 API, ardışık çalışır.

**generate_signature() — HMAC-SHA256:**
```rust
fn generate_signature(secret: &str, timestamp: &str, method: &str, path: &str, query: Option<&str>) -> String {
    let message = format!("{timestamp}{method}{path}{}",
        query.map(|q| format!("?{q}")).unwrap_or_default());
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let signature = hmac::sign(&key, message.as_bytes());
    BASE64_STANDARD.encode(signature.as_ref())
}
```

OKX: `HMAC-SHA256(secret, timestamp+METHOD+path+?query)` → BASE64.

**cache_approval_address():**
```rust
async fn cache_approval_address(&self, token: TokenAddress, side: ApprovalSide) -> Result<ContractAddress> {
    self.approval_cache
        .try_get_with((token, side), async {
            self.fetch_approval_address(token, side).await
        })
        .await
}
```

`try_get_with` — concurrent duplicate request'leri handle eder (thunder herd prevention).

**Error Codes:**
- 0 = OK
- 51005, 82000, 82104, 82112 = OrderNotFound
- 50011 = RateLimited

**DEX Uygulama Notu — Nova Shield:**
1. `tokio::try_join!` ile paralel API call'ları latency'yi yarıya indirir
2. `gas * 1.5` buffer — DEX API gas tahminleri düşük olabilir
3. Approval address cache'leme: `moka::Cache` thunder herd prevention sağlar
4. HMAC imzası her request için yeniden hesaplanmalı (timestamp değişiyor)

---

## 42. infra/dex/okx/dto.rs (312 satır)

**Konum**: `src/infra/dex/okx/dto.rs`

**SwapRequest (V6):**
```rust
pub struct SwapRequest {
    pub chain_id: u64,
    pub from_token_address: Address,
    pub to_token_address: Address,
    pub amount: String,         // wei string
    pub slippage: String,       // "0.01" = %1
    pub user_wallet_address: Address,
    pub swap_receiver_address: Address,
    pub fee_percent: String,
    pub price_impact_protection_percent: String,
}
```

**SwapMode enum:**
```rust
pub enum SwapMode {
    ExactIn,   // Sell side
    ExactOut,  // Buy side (V5 only)
}
```

**Response<T>:**
```rust
pub struct Response<T> {
    pub code: i64,      // 0 = başarı
    pub msg: String,
    pub data: Option<Vec<T>>,  // Her zaman array, tek item için de Vec
}
```

---

## 43. infra/dex/bitget/mod.rs (471 satır) — BITGET API İSTEMCİSİ

**Konum**: `src/infra/dex/bitget/mod.rs`

**wei_to_decimal() / decimal_to_wei():**
```rust
fn wei_to_decimal(amount: U256, decimals: u8) -> BigDecimal {
    BigDecimal::new(amount.into(), decimals as i64)
}

fn decimal_to_wei(amount: BigDecimal, decimals: u8, rounding: Round) -> U256 {
    let scaled = amount * BigDecimal::new(BigInt::one(), -(decimals as i64));
    let (bigint, _) = match rounding {
        Round::Up => scaled.with_scale_round(0, RoundingMode::Ceiling),
        Round::Down => scaled.with_scale_round(0, RoundingMode::Floor),
    }.into_bigint_and_exponent();
    U256::from_str(&bigint.to_str_radix(10)).unwrap_or(U256::ZERO)
}
```

`Round::Up` — buy order'larda sell miktarı yukarı yuvarlanır.
`Round::Down` — sell order'larda buy miktarı aşağı yuvarlanır.

**handle_sell_order(): `requestMod="rich"`** → tam swap calldata döndür.

**handle_buy_order() — /swapr Endpoint:**
```rust
let tx = response.data.tx_list
    .iter()
    .find(|tx| tx.function == "swap")
    .ok_or(Error::QuoteNotFound)?;
```

`function=="swap"` olan tx'i bul. `tx_list` içinde birden fazla transaction olabilir.

**HMAC-SHA256 Bitget için:**
```rust
fn generate_signature(secret: &str, body: &serde_json::Value) -> String {
    // Sorted JSON body üzerinden (BTreeMap)
    let sorted_body: BTreeMap<&str, &Value> = body.as_object().unwrap()
        .iter().map(|(k, v)| (k.as_str(), v)).collect();
    let canonical = serde_json::to_string(&sorted_body).unwrap();
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    let sig = hmac::sign(&key, canonical.as_bytes());
    hex::encode(sig.as_ref())  // OKX BASE64 kullanır, Bitget HEX kullanır
}
```

**OKX vs Bitget imza farkı:**
- OKX: URL-tabanlı (`timestamp+method+path+query`) → BASE64
- Bitget: body-tabanlı (sorted JSON) → HEX

**Error Codes:**
- 80000 = NotFound
- 80001 = RateLimited
- 80010, 80011, 80020 = BadRequest

---

## 44. infra/dex/bitget/dto.rs (271 satır)

**Konum**: `src/infra/dex/bitget/dto.rs`

**ChainName enum:**
```rust
pub enum ChainName {
    Eth, Base, Arbitrum, Bnb, Polygon,
}
```

Bitget chain'leri numeric ID yerine string isimle tanımlar.

**SwapRequest**: `from_amount` (input) parametreli.
**ReverseSwapRequest**: `to_amount` (output) parametreli — buy orders için gerekli.

---

## 45. api/mod.rs (50 satır)

**Konum**: `src/api/mod.rs`

```rust
pub const REQUEST_BODY_LIMIT: usize = 10 * 1024 * 1024;  // 10 MB

pub fn router(solver: Arc<Solver>) -> Router {
    Router::new()
        .route("/metrics", get(metrics::handler))
        .route("/healthz", get(health::handler))
        .route("/solve", post(solve::handler))
        .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
        .layer(TraceLayer::new_for_http()
            .make_span_with(|request: &Request<_>| {
                let request_id = request.headers()
                    .get("X-REQUEST-ID")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                tracing::info_span!("request", %request_id)
            })
        )
}
```

10MB body limit — DoS saldırısına karşı koruma.
`TraceLayer` — `X-REQUEST-ID` header'ını span'a dahil eder, log correlation sağlar.

---

## 46. api/routes/solve/mod.rs (44 satır)

**Konum**: `src/api/routes/solve/mod.rs`

```rust
pub async fn handler(
    State(solver): State<Arc<Solver>>,
    Json(auction_dto): Json<dto::Auction>,
) -> impl IntoResponse {
    let auction = match auction_dto.try_into() {
        Ok(a) => a,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };

    let solutions = solver.solve(&auction).await;
    let response: Vec<dto::Solution> = solutions.into_iter().map(dto::Solution::from).collect();
    Json(response).into_response()
}
```

Parse hatası → 400. Solver başarısızlığı → boş array (200 OK).

---

## 47. api/routes/solve/dto/auction.rs (273 satır)

**Konum**: `src/api/routes/solve/dto/auction.rs`

**Fee Dönüşümü (Uniswap V3):**
```rust
fn fee_from_decimal(decimal: &BigDecimal) -> u32 {
    // "0.003" → 3000 (Uniswap V3 fee units, 1_000_000 olcekli)
    let factor = BigDecimal::from(1_000_000u64);
    let scaled = decimal * factor;
    u32::try_from(scaled.to_u64().unwrap_or(0)).unwrap_or(0)
}
```

Uniswap V3: 0.3% = 3000, 0.05% = 500, 1% = 10000.

`trusted` flag buradan girer — driver'ın konfigürasyonundan belirlenir.

---

## 48. api/routes/solve/dto/solution.rs (146 satır)

**Konum**: `src/api/routes/solve/dto/solution.rs`

**Gas Dönüşümü:**
```rust
fn gas_to_dto(gas: eth::Gas) -> u64 {
    u64::try_from(gas.0).unwrap_or(u64::MAX)
}
```

U256 → u64. Normal koşullarda gas miktarı u64'ü aşmaz.

---

## 49. util/http.rs (152 satır)

**Konum**: `src/util/http.rs`

**roundtrip! Macro — Callsite Log:**

Macro olma gerekçesi: `tracing::trace!` log span'ı callsite'tan alır. Normal fonksiyon olsaydı tüm HTTP log'ları `util::http`'dan geliyormuş gibi görünürdü.

**roundtrip_internal<T, E>() — Parse Stratejisi:**
1. T olarak parse et → Ok
2. E olarak parse et → Api error
3. 2xx status + parse başarısız → Json parse hatası
4. Non-2xx → HTTP status hatası

**Never Type:**
```rust
pub struct Never;
impl<'de> serde::Deserialize<'de> for Never {
    fn deserialize<D>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("neva eva eva"))
    }
}
```

`Never` — "Bu API'nin hata tipi yok" için generic E placeholder.

---

## 50. util/conv.rs (83 satır)

**Konum**: `src/util/conv.rs`

**decimal_to_ether():**
```rust
pub fn decimal_to_ether(d: &BigDecimal) -> Option<eth::Ether> {
    let scaled = d * BigDecimal::new(BigInt::one(), -18);  // d * 10^18
    let ratio = decimal_to_rational(&scaled)?;
    Some(eth::Ether(ratio.numer() / ratio.denom()))
}
```

`BigDecimal::new(BigInt::one(), -18)` = `1 * 10^(18)` = `10^18`. Negatif exp büyütme anlamına gelir.

**Test Coverage:**
```
"4.2" → 21/5, "0.003" → 3/1000
0.01 ETH <-> 10_000_000_000_000_000 wei
"-0.42" → None (negatif)
```

---

## 51. tests/mod.rs (93 satır)

**Konum**: `src/tests/mod.rs`

```rust
pub async fn new(command: &str, config: Config) -> Self {
    let args = vec![
        "/test/solvers/path",
        "--addr=0.0.0.0:0",   // OS'tan random port
        "--log=solvers=trace",
        command,
    ];
    let handle = tokio::spawn(crate::run(args, Some(bind)));
    ...
}
```

`0.0.0.0:0` — OS'ın boş port ataması. Port çakışması riski sıfır.

`Drop` impl: `handle.abort()` — test bitince otomatik cleanup.

---

## 52. tests/mock/http.rs (278 satır)

**Konum**: `src/tests/mock/http.rs`

```rust
pub struct Expectation {
    pub path: Path,
    pub method: Method,
    pub request_body: RequestBody,
    pub response: Response,
}

pub enum Path {
    Any,
    Exact(String),
    Glob(String),
}

pub enum RequestBody {
    Exact(serde_json::Value),
    Partial(serde_json::Value),  // Subset eslesme
    Any,
}
```

**Expectation Queue**: `VecDeque` — FIFO. Her request, kuyruktaki bir sonraki expectation'ı consume eder.

**assert_and_propagate_panics:**
```rust
let result = std::panic::catch_unwind(|| {
    let expectation = queue.pop_front().expect("unexpected request");
    assert_matches!(request.path(), expectation.path);
    ...
});
if result.is_err() {
    panicked.store(true, Ordering::SeqCst);
}
```

`AtomicBool` + `catch_unwind`: Test panik yaptığında diğer async işlemleri durdurur.

**DEX Uygulama Notu — Nova Shield**: Mock HTTP server pattern DEX API integrations için şart. Her API call test edilmeden integration güvenilmez.

---

## Kritik Algoritmalar — Özet

### 1. Uniform Clearing Price (Anti-MEV)
Tüm emirler aynı `ClearingPrices` vektörüyle temizlenir. Arbitraj karı kullanıcılara surplus olarak iade edilir.

### 2. 512-bit Limit Fiyat Kontrolü
```
U512::from(output) * U512::from(sell) >= U512::from(input) * U512::from(buy)
```
U256 * U256 overflow'u önler.

### 3. Min(relative, absolute) Slippage
```
effective_slippage = min(relative%, absolute_ETH / amount_ETH * amount_token)
```

### 4. Max(relative, absolute) Minimum Surplus
```
min_surplus = max(relative%, absolute_ETH / amount_ETH * amount_token)
```

### 5. BFS + Greedy Per-Hop Path Finding
```
paths = BFS(sell_token, buy_token, base_tokens, max_hops=2)
best = argmax_{path in paths}(forward_greedy(path))
```

### 6. Adaptive Binary Search Fill
```
Basarisiz: next_fill = prev_fill / 2
Basarili:  next_fill = filled * 2
Sinir: [smallest_fill, total_amount]
```

### 7. Rate Limiter Exponential Backoff
```
Basarisiz: backoff = min(current * 2, max_back_off)
Basarili:  backoff = min_back_off
```

### 8. Gas Simulasyonu via eth_call + State Override
```
state_override = {
    owner: {code: SWAPPER_BYTECODE},
    authenticator: {code: ANYONE_AUTH_BYTECODE}
}
gas = eth_call(settlement.settle(...), state_override).gas_used
```

---

## Master DEX Uygulama Planı — Nova Shield

### Öncelik 1 — Temel Mimari (Hemen)
1. **Uniform Clearing Price Engine**: `ClearingPrices` vektörü ile tüm trades temizlenir.
2. **Order Taxonomy**: Market / Limit / Liquidity / JIT.
3. **512-bit Limit Kontrolü**: Her quote'tan sonra `satisfies()` çalıştır.
4. **Ethereum Primitive Newtypes**: Tip güvenliği için şart.

### Öncelik 2 — Likidite Entegrasyonu (1-3 Ay)
1. **5 Pool Tipi Desteği**: ConstantProduct, WeightedProduct, Stable, Concentrated, LimitOrder
2. **BaselineSolvable Trait**: Polymorphic dispatch
3. **BFS + Greedy Routing**: max_hops=2 başlangıç
4. **Gas Cost Per Pool**: Empirik gas ölçümü

### Öncelik 3 — MEV Koruma Katmanı (3-6 Ay)
1. **Slippage Limits**: min(relative, absolute_as_relative)
2. **Minimum Surplus**: max(relative, absolute_as_relative)
3. **Buffer Internalization**: Settlement contract token buffer'ları
4. **Gas Simülatörü**: eth_call + state override

### Öncelik 4 — Dış Likidite Entegrasyonu (6-12 Ay)
1. **OKX/1inch/0x API Entegrasyonu**: HMAC-SHA256, approval caching
2. **Rate Limiter**: Exponential backoff
3. **Adaptive Partial Fill**: Binary search ile optimal fill
4. **Concurrent Requests**: buffer_unordered

### Güvenlik Kontrol Listesi
- [ ] Her limit fiyat kontrolünde 512-bit aritmetik
- [ ] Slippage'da BigDecimal (float değil)
- [ ] Gas sabitlerini empirik olarak doğrula
- [ ] `trusted` token listesi — internalization sadece güvenilir tokenlar için
- [ ] `deny_unknown_fields` tüm config struct'larında
- [ ] Approval address caching — thunder herd önleme
- [ ] Gas * 1.5 buffer — DEX API tahminleri konservatif değil
- [ ] State override simülasyonu — gerçek execution cost
- [ ] Mock HTTP server testleri — tüm DEX API integrations
- [ ] `AtomicBool` + `catch_unwind` — async test panic propagation

### Nova Shield Özgün Katkılar (CoW'dan Öte)
1. **ZK-Proof Entegrasyonu**: Limit fiyat kontrolü ZK circuit ile
2. **MPC-Tabanlı Batch Signing**: Threshold signature ile execution
3. **Cross-Chain Clearing**: Farklı chain'lerde emir batch'leri
4. **Shielded Orders**: Penumbra ZK modelinden — emir içeriği gizli
5. **Native AMM + Solver Competition**: Hem kendi AMM'i hem dış solver'lar

---

## Dosya Dizini (Tamamı)

| Dosya | Satır | Önem |
|-------|-------|------|
| main.rs | 12 | Binary entry |
| lib.rs | 14 | Crate root |
| run.rs | 94 | CLI + solver init |
| domain/auction.rs | 104 | Auction model |
| domain/order.rs | 162 | Order model |
| domain/solution.rs | 486 | KRITIK — Solution model |
| domain/notification.rs | 62 | Feedback loop |
| domain/eth/mod.rs | 99 | ETH primitives |
| domain/eth/chain.rs | 186 | Chain ID enum |
| domain/liquidity/mod.rs | 80 | Liquidity wrapper |
| domain/liquidity/constant_product.rs | 61 | Uniswap V2 |
| domain/liquidity/concentrated.rs | 14 | Uniswap V3 |
| domain/liquidity/weighted_product.rs | 72 | Balancer V2 |
| domain/liquidity/stable.rs | 59 | Curve/Balancer Stable |
| domain/liquidity/limit_order.rs | 13 | 0x Limit Order |
| domain/dex/mod.rs | 204 | KRITIK — DEX order/swap |
| domain/dex/slippage.rs | 268 | KRITIK — MEV koruma |
| domain/dex/minimum_surplus.rs | 141 | Minimum surplus |
| domain/dex/shared.rs | 54 | Paylasilan matematik |
| domain/solver/mod.rs | 22 | Solver dispatch |
| domain/solver/baseline.rs | 408 | KRITIK — Baseline solver |
| domain/solver/dex/mod.rs | 231 | KRITIK — DEX solver |
| domain/solver/dex/fills.rs | 181 | Adaptive fill |
| boundary/mod.rs | 8 | Boundary katmani |
| boundary/baseline.rs | 332 | Domain→shared adapter |
| boundary/routing.rs | 432 | KRITIK — Path finding |
| boundary/liquidity/concentrated.rs | 79 | UniV3 RPC |
| boundary/liquidity/constant_product.rs | 53 | UniV2 adapter |
| boundary/liquidity/weighted_product.rs | 64 | Balancer adapter |
| boundary/liquidity/stable.rs | 60 | Stable adapter |
| boundary/liquidity/limit_order.rs | 164 | 0x adapter |
| infra/cli.rs | 56 | CLI args |
| infra/contracts.rs | 43 | Contract adresleri |
| infra/metrics.rs | 60 | Prometheus metrics |
| infra/config/baseline.rs | 95 | Baseline config |
| infra/config/dex/file.rs | 206 | DEX ortak config |
| infra/config/dex/okx/file.rs | 115 | OKX config |
| infra/config/dex/bitget/file.rs | 85 | Bitget config |
| infra/dex/mod.rs | 122 | DEX client enum |
| infra/dex/simulator.rs | 121 | Gas simulatoru |
| infra/dex/okx/mod.rs | 516 | KRITIK — OKX API |
| infra/dex/okx/dto.rs | 312 | OKX DTO |
| infra/dex/bitget/mod.rs | 471 | KRITIK — Bitget API |
| infra/dex/bitget/dto.rs | 271 | Bitget DTO |
| api/mod.rs | 50 | HTTP router |
| api/routes/solve/mod.rs | 44 | Solve handler |
| api/routes/solve/dto/auction.rs | 273 | JSON→domain |
| api/routes/solve/dto/solution.rs | 146 | Domain→JSON |
| util/http.rs | 152 | HTTP roundtrip |
| util/conv.rs | 83 | BigDecimal utils |
| tests/mod.rs | 93 | Test harness |
| tests/mock/http.rs | 278 | Mock HTTP server |

**Toplam**: ~12.454 satır, 78 dosya, eksiksiz analiz tamamlandı.
