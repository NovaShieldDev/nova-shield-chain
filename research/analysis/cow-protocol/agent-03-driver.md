# CoW Protocol Analiz — Agent 03 (Driver)
## Kapsam: driver/src/ (competition dışı) — Driver Altyapısı
## DEX Özel Protokolü: 3x Hassasiyet

Analiz tarihi: 2026-07-22
Kaynak: `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/libs/cow-protocol/crates/driver/src/`

---

## Genel Mimari: Domain-Driven Design

Driver, klasik DDD katmanlamasını uygular:

```
infra/   — dış dünya (blockchain RPC, solver HTTP, mempool, config)
domain/  — iş mantığı (saf Rust, dış bağımlılık yok)
boundary/ — anti-corruption layer (infra'dan domain'e çeviri)
```

Eric Evans'ın "Domain-Driven Design" kitabından alıntı boundary/mod.rs yorumuna konulmuş:
> "When you find your system needing to communicate with another system … create an isolating layer to provide clients with functionality in terms of their own domain model."

Bu, shared/crates gibi shared kütüphanelerin domain objelerine sızmadan sadece boundary üzerinden geçmesini zorunlu kılar.

---

## 1. lib.rs (16 satır)

**Amaç:** Driver crate'ini hem binary (main.rs) hem de e2e test kütüphanesi olarak kullanılabilir kılar.

**Kritik Detay:**
```rust
#![forbid(unsafe_code)]
```
Tüm driver unsafe'den muaf. Güvenlik için tasarım kararı.

**Modüller:**
- `pub mod boundary` — anti-corruption layer
- `pub mod domain` — iş mantığı
- `pub mod infra` — altyapı
- `pub mod util` — yardımcı araçlar
- `pub use self::run::{run, start}` — e2e testlerin `driver::run(...)` diyebilmesi için

**DEX Uygulama Notu:**
Nova Shield DEX modülünü da `#![forbid(unsafe_code)]` ile korumalı. Test framework'ünün ayrı binary başlatmadan kütüphane olarak import edebilmesi için DEX engine'i lib crate olarak tasarla.

---

## 2. main.rs (13 satır)

**Amaç:** Binary entry point. Minimal — sadece allocator seçimi ve start() çağrısı.

**Allocator seçimi:**
```rust
#[cfg(feature = "mimalloc-allocator")]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(not(feature = "mimalloc-allocator"))]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
```

Varsayılan: **jemalloc** (heap profiling destekli, MALLOC_CONF ile runtime aktifleştirilir).
Opsiyonel: **mimalloc** (Windows-friendly, daha düşük overhead bazı workload'larda).

**DEX Uygulama Notu:**
Nova Shield'da da jemalloc varsayılan olmalı. Özellikle order book gibi yoğun allocation yapan yapılarda jemalloc fragmentation'ı azaltır.

---

## 3. run.rs (259 satır)

**Amaç:** Driver'ın tüm bileşenlerini bir araya getirir ve serve eder.

### Başlatma Akışı (run_with)

```
observe::panic_hook::install()
    ↓
cli::Args::parse_from()
    ↓
infra::observe::init() — logging + metrics
    ↓
ethrpc() — blockchain RPC bağlantısı
    ↓
config::file::load() — TOML config
    ↓
Ethereum::new() — gas estimator, contracts, current_block
    ↓
simulator::Ethereum::new() — simulation katmanı
    ↓
AppDataRetriever::new() — orderbook metadata cache
    ↓
solvers() — join_all(Solver::try_new) — tüm solver'lar paralel başlar
    ↓
EIP-7702 setup() — delegation ve deployment (bu tamamlanmadan HTTP server başlamaz)
    ↓
Api::serve() — HTTP server
```

**Kritik Sıralama:** EIP-7702 setup, Http server'dan ÖNCE tamamlanmalı. Aksi takdirde /settle endpoint'i düzgün çalışmaz.

### Graceful Shutdown

```rust
tokio::select! {
    result = &mut serve => panic!("serve task exited: {result:?}"),
    _ = shutdown_signal() => {
        shutdown_sender.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(20), serve).await
    }
}
```

SIGTERM veya SIGINT alındığında 20 saniye beklenir. Bu değer auction deadline'ından büyük olmalı — çünkü aktif bir auction'ın bitmesi beklenmeli.

### Simulator Konfigürasyonu

```rust
if config.disable_access_list_simulation { simulator.disable_access_lists(); }
if let Some(gas) = config.disable_gas_simulation { simulator.disable_gas(gas); }
if let Some(cfg) = &config.simulator.state_override_stream { ... }
```

Access list ve gas simulation'ı ayrı ayrı devre dışı bırakılabilir. State override stream: blok başına state overrides inject eder (MEV protection testi için kullanışlı).

**DEX Uygulama Notu:**
Nova Shield DEX'te de aynı graceful shutdown pattern kullanılmalı. Aktif swap işlemi varsa 20-30s beklenmeli. Panic hook kurulumu kritik — production'da panikler sessizce kaybolmamalı.

---

## 4. domain/blockchain.rs (84 satır)

**Amaç:** Blockchain işlem kavramları için domain tipleri.

### Tipler

```rust
pub struct TxId(B256);  // transaction hash

pub enum TxStatus {
    Executed { block_number: u64 },
    Reverted { block_number: u64 },
    Pending,
}

pub struct Tx {
    pub from: eth::Address,
    pub to: eth::Address,
    pub value: eth::Ether,
    pub input: eth::Calldata,
    pub access_list: eth::AccessList,
}

pub struct CodeDigest(B256);  // CREATE2 init code hash için
```

### Kritik Köprü

```rust
impl From<Tx> for TransactionRequest {
    fn from(tx: Tx) -> Self {
        // domain Tx → alloy TransactionRequest dönüşümü
    }
}
```

Bu `From` impl, domain katmanının infra'ya sızmadan kullanılmasını sağlar. Domain bilmez ki alloy kullanılıyor.

**DEX Uygulama Notu:**
Nova Shield'da da `Tx`, `TxId`, `TxStatus` gibi domain tipler tanımlanmalı. `From<DomainTx> for ChainTx` pattern'i infra değiştiğinde (alloy başka kütüphane) domain'i korur.

---

## 5. domain/cow_amm.rs (133 satır)

**Amaç:** CoW AMM havuzlarını on-chain'den çekerek cache'ler.

### Struct

```rust
pub struct Cache {
    inner: RwLock<HashMap<Address, Arc<Amm>>>,
    web3: DynProvider,
    helper_by_factory: HashMap<Address, CowAmmLegacyHelper::Instance>,
}
```

### Kritik Algoritma: get_or_create_amms()

```
1. read lock → hangi address'ler cache'de var?
2. partition: cached vs missing
3. join_all(fetch_missing) — paralel on-chain çekme
4. write lock → yeni AMM'leri ekle
5. cached + yeni = toplam result
```

Optimistic read → paralel fetch → minimal write lock. Bu pattern yüksek eşzamanlılıkta verimli.

### CoW AMM vs Normal AMM Farkı

| Özellik | Normal AMM | CoW AMM |
|---------|-----------|---------|
| Order tipi | Reaktif (swap tetikler) | JIT orders (batch'e eklenir) |
| Surplus | Trader alır | Protocol/AMM alır |
| Factory | Tek tip | Factory+Helper indirection (legacy desteği) |
| Discovery | Sabit adres | factory → address lookup on-chain |

```rust
async fn fetch_amm_factory_address(
    helper: &CowAmmLegacyHelper::Instance,
    address: Address,
) -> Result<Address> {
    // CowAmmFactoryGetter::FACTORY() view call
}
```

**Matematiksel Not:**
CoW AMM JIT orders: settlement'a dahil edilip trader ile karşı taraf olarak hareket eden AMM emirleri. Normal AMM'de `x*y=k` constraint solver tarafından hesaplanır; CoW AMM'de AMM kendi emrini oluşturur.

**DEX Uygulama Notu:**
Nova Shield DEX'te AMM türleri ayrı olmalı: pasif (CFMM) vs aktif (JIT-order AMM). Cache pattern doğrudan kullanılabilir: `RwLock<HashMap<PoolId, Arc<Pool>>>` + `join_all` fetch.

---

## 6. domain/flashloan.rs (52 satır)

**Amaç:** Flash loan parametrelerini protocol-agnostik şekilde temsil eder.

### Struct

```rust
pub struct Flashloan {
    pub liquidity_provider: ContractAddress,  // flash loan veren (ör: Aave Pool)
    pub protocol_adapter: ContractAddress,    // flash loan callback adapter
    pub receiver: Address,                    // flash loan alıcı
    pub token: TokenAddress,                  // ödünç alınan token
    pub amount: TokenAmount,                  // miktar
}
```

### İki Dönüşüm

```rust
impl Into<solvers_dto::solution::Flashloan> for Flashloan { ... }
impl Into<FlashloanHint> for Flashloan { ... }
```

1. `FlashloanHint` — solver'a auction request'te gönderilir (solver ne istiyor öğrensin)
2. `solvers_dto::solution::Flashloan` — solver'ın çözümünde döndürdüğü

### Güvenlik

`liquidity_provider` + `protocol_adapter` ayrımı kritik: farklı protokollerin (Aave v2, v3, Balancer) farklı callback interface'leri var. Adapter pattern bu farkı soyutlar.

**DEX Uygulama Notu:**
Nova Shield DEX'te flash loan entegrasyonu için aynı struct tasarımı: provider adresi + adapter adresi + receiver + token + amount. Bu 5 alan herhangi bir flash loan protokolünü destekler.

---

## 7. domain/interaction.rs (33 satır)

**Amaç:** Genel akıllı kontrat çağrısı temsili.

```rust
pub struct Interaction {
    pub target: Address,
    pub value: Ether,
    pub call_data: Bytes,
}
```

Bidirectional conversion: `model::interaction::InteractionData` ↔ `Interaction`

Pre/post/main interactions hepsi bu tipi kullanır. Herhangi bir kontrat çağrısını encode edebilir.

**DEX Uygulama Notu:**
Nova Shield DEX'te de settlement öncesi/sonrası işlemler (pre-hook, post-hook) bu formatta temsil edilmeli.

---

## 8. domain/mempools.rs (749 satır) — KRİTİK

**Amaç:** Transaction submission, izleme, replacement ve iptal mekanizması.

### Sabitler

```rust
const GAS_PRICE_BUMP_PCT: u64 = 13;  // EIP-1559 replacement minimum %12.5, biz %13 kullanıyoruz
const CANCELLATION_GAS_AMOUNT: u64 = 21_000;  // ETH self-transfer
```

Neden 13%? Ethereum protokolü minimum %12.5 artış gerektiriyor. %13 bu minimum'u güvenli geçer.

### Tipler

```rust
pub enum SubmissionMode {
    Direct(eth::Address),           // normal: solver hesabından direkt gönder
    Delegated {                     // EIP-7702: submission_account gönderir, solver adına
        submitter_eoa: eth::Address,
        solver_eoa: eth::Address,
    },
}

pub struct Mempools {
    mempools: Vec<infra::Mempool>,
    ethereum: Ethereum,
}
```

### Ana Akış: execute()

```rust
// Tüm mempoolları eş zamanlı dene — ilk başarılı kazanır
select_ok(mempools.iter().map(|m| submit(m, tx, ...)))
```

`futures::future::select_ok` ilk başarıyı döner, kalanları drop eder. Drop edilenler "Superseded" olarak işaretlenir.

### submit() İç Akışı

```
1. simulate(tx) — revert edecek mi?
2. get_nonce(signer) — mevcut nonce
3. gas_price_estimate() — base fee + tip
4. minimum_replacement_gas_price() — eski tx varsa minimum bump
5. apply_gas_fee_override() — solver'ın istediği gaz fiyatı (clamp ile)
6. final_price = max(estimate, replacement_minimum)
7. mempool.submit(tx_with_price)
8. Poll her blokta: Executed/Reverted/Pending?
9. Eğer simülasyon revert → requires_cancellation() → cancel()
```

### Replacement Mekanizması

```rust
fn minimum_replacement_gas_price(
    last_submissions: &DashMap<Address, Submission>,
    signer: Address,
    ...
) -> Option<GasPrice> {
    // 1. DashMap'ten son submission'ı bul
    // 2. Eğer yoksa txpool_content_from RPC çağrısı
    // 3. Bulunan fiyatı %13 artır
}
```

`DashMap`: thread-safe HashMap (dashmap crate). Her signer için son submission saklanır.

### İptal Mekanizması

```rust
fn requires_cancellation(
    pending_nonce: U256,
    submission_nonce: U256,
    resimulated: bool,
) -> bool {
    // SADECE: resimülasyon revert ETTİ ve tx hala mempool'da
    resimulated && pending_nonce <= submission_nonce
}
```

Önemli: sadece revert EDECEK bir tx iptal edilir. Gecikmiş ama başarılı tx'ler bekletilir.

```rust
async fn cancel(nonce: U256, gas_price: GasPrice, signer: Address) {
    // to == from (self-transfer), value == 0, calldata boş
    // aynı nonce, bump'lı gas price
}
```

### EIP-7702 Calldata Rewriting

```rust
fn prepare_submission(tx: Tx, mode: SubmissionMode) -> Tx {
    match mode {
        SubmissionMode::Delegated { submitter_eoa, solver_eoa } => {
            // tx.to = solver_eoa (delegate hesap)
            // tx.from = submitter_eoa
            // tx.input = solver_eoa (20 bytes) || original_calldata
        }
        SubmissionMode::Direct(_) => tx  // değişiklik yok
    }
}
```

`delegated_calldata()`: 20-byte target address'i calldata'nın başına ekler. `Solver7702Delegate` kontratı bu prefix'i okuyup doğru hedefe forward eder.

### Gas Fiyatı Ayarlaması

```rust
fn apply_gas_fee_override(
    estimate: GasPrice,
    override_val: Option<GasPrice>,
    replacement_minimum: Option<GasPrice>,
) -> GasPrice {
    let base = override_val.unwrap_or(estimate);
    let min = replacement_minimum.unwrap_or(GasPrice::ZERO);
    max(base, min)
}
```

Solver kendi gas fiyatını önerebilir; driver replacement minimum ile clamp eder.

### Metrik Güncelleme

```rust
fn update_metrics(results: &[SubmissionResult]) {
    // Eğer bir "winner" varsa (Executed), diğer Failed'lar → Superseded olarak loglanır
}
```

Bu sayede "kaybeden" mempoollar failure olarak değil, doğal yarış kaybı olarak raporlanır.

### Tests

```rust
#[test] fn delegated_submission_rewrites_transaction()
#[test] fn cancels_only_when_reverted_and_tx_left_the_mempool()
#[test] fn failed_nonce_lookup_never_cancels()
```

**Matematiksel Formüller:**
- Replacement minimum: `old_max_fee * 1.13` ve `old_priority_fee * 1.13`
- Cancel gas: `21_000 * replacement_gas_price`

**Güvenlik Analizi:**
- Simülasyon başarısız → submit edilmez (false positive tx önlenir)
- Revert protection: özel mempool'a gönderilir, public mempool'a değil
- `requires_cancellation` koşulu konservative: sadece kesin revert durumunda iptal

**DEX Uygulama Notu:**
Nova Shield DEX'te de çok-mempool yarışı kritik. `select_ok` pattern + `DashMap<Address, LastSubmission>` + `%13 replacement bump` doğrudan uygulanabilir. EIP-7702 support: paralel hesap submission için zorunlu (MEV bot yarışında hız kazancı).

---

## 9. domain/quote.rs (401 satır) — KRİTİK

**Amaç:** Kullanıcıya fiyat teklifi üretir. Gerçek auction mekanizmasını simüle eder.

### Struct

```rust
pub struct Quote {
    clearing_prices: HashMap<eth::Address, eth::U256>,
    pre_interactions: Vec<Interaction>,
    interactions: Vec<Interaction>,
    solver: eth::Address,
    gas: eth::Gas,
    tx_origin: Option<eth::Address>,
    jit_orders: Vec<order::Jit>,
}
```

### Kritik Sabit

```rust
const DEFAULT_QUOTE_SLIPPAGE_BPS: u32 = 100;  // 1% varsayılan slippage
```

### fake_auction() — Sahte Açık Artırma

```rust
fn fake_auction(order: &Order, liquidity: Vec<Liquidity>) -> competition::Auction {
    // Tek emirli sentetik auction
    // uid = default (sıfır)
    // valid_to = u32::MAX (asla expire olmaz)
    // sell order: buy = U256::ONE (minimum)
    // buy order: sell = 2^144 (maksimum)
}
```

Neden `2^144`? Uniswap V2 rezervleri `uint112` (max 2^112). Satış miktarı her zaman rezervden büyük olacak şekilde 256-112=144 bit ile ayarlanır → solver hangi tarafın ne kadar alacağını hesaplayabilir.

### compute_clearing_prices() — Fiyat Hesabı

```rust
fn compute_clearing_prices(
    fulfillment: &Fulfillment,
    haircut_bps: u32,
) -> Result<HashMap<Address, U256>> {
    // 1. uniform_prices ile başla (solver çıktısından)
    // 2. haircut_fee != 0 ise fiyatları konservatif ayarla
}
```

**Haircut Mekanizması:**
Quoting'de gerçek settlement'tan daha iyi fiyat göstermemek için fiyatları biraz kötüleştirir. Bu kullanıcının aldığı fiyatın quote'dan daha iyi çıkmasını sağlar (negatif slippage).

### Onay Sıfırlama (USDT Uyumluluğu)

```rust
// ERC20 approve: önce 0'a çek, sonra max'a set et
approvals = [Approval { amount: 0 }, Approval { amount: MAX }]
```

USDT'nin "double-spend protection" özelliği: mevcut allowance > 0 iken yeni approve reverts. Bu yüzden önce 0'a çekilir.

### buy() ve sell() Mantığı

```rust
fn buy(order: &Order) -> eth::TokenAmount {
    match order.side {
        Side::Sell => U256::ONE,          // satış emri: 1 unit al
        Side::Buy  => order.amount,       // alış emri: tam miktar
    }
}

fn sell(order: &Order) -> eth::TokenAmount {
    match order.side {
        Side::Sell => order.amount,       // satış emri: tam miktar
        Side::Buy  => 2u256.pow(144),     // alış emri: çok büyük → solver hesaplar
    }
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te quote sistemi `fake_auction` pattern'iyle implement edilebilir. Tek emirli sentetik auction çok elegant — gerçek solver mekanizmasını test ederek fiyat üretir, ayrı pricing engine gerektirmez. Haircut mekanizması şart: kullanıcıya verilen fiyat gerçek settlement'tan daha iyi değilse complaint riski yüksek.

---

## 10. domain/time.rs (77 satır)

**Amaç:** Üç kademeli deadline yönetimi.

### Struct

```rust
pub struct Deadline {
    pub auction: chrono::DateTime<Utc>,   // autopilot'tan gelen son tarih
    pub driver: chrono::DateTime<Utc>,    // auction - http_delay
    pub solvers: chrono::DateTime<Utc>,   // şimdiki zaman + (driver-şimdi)*solving_share
}
```

### Hesaplama

```rust
fn new(auction: DateTime, http_delay: Duration, solving_share: f64) -> Self {
    let driver = auction - http_delay;
    let remaining = (driver - now()).max(Duration::ZERO);
    let solvers = now() + remaining * solving_share;
    // solving_share varsayılan: 0.8 (konfigürasyon dosyasından)
}
```

**Örnek:** Auction deadline 60s, http_delay 2s, solving_share 0.8:
- driver = 58s kalan
- solvers = 0.8 * 58 = 46.4s kalan

Solver'a %80 zaman verilir, kalan %20 encoding + submission için.

### Remaining Trait

```rust
trait Remaining {
    fn remaining(self) -> Result<Duration, DeadlineExceeded>;
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te de bu 3-tier deadline modeli kullanılmalı: network_deadline → driver_deadline → solver_deadline. `solving_share` konfigürasyon parametresi olmalı.

---

## 11. domain/liquidity/mod.rs (97 satır)

**Amaç:** Tüm likidite kaynaklarının ortak domain temsili.

### Tipler

```rust
pub struct Liquidity {
    pub id: Id,
    pub gas: eth::Gas,     // bu kaynağı kullanmanın gas maliyeti
    pub kind: Kind,
}

pub enum Kind {
    UniswapV2(uniswap::v2::Pool),
    UniswapV3(uniswap::v3::Pool),
    BalancerV2Stable(balancer::v2::stable::Pool),
    BalancerV2Weighted(balancer::v2::weighted::Pool),
    Swapr(swapr::Pool),
    ZeroEx(zeroex::LimitOrder),
}

pub struct TokenPair(eth::TokenAddress, eth::TokenAddress);  // her zaman küçük adres önce
```

### TokenPair Sıralama

```rust
fn try_new(a: TokenAddress, b: TokenAddress) -> Result<Self, SameTokens> {
    if a == b { return Err(SameTokens); }
    if a < b { Ok(Self(a, b)) } else { Ok(Self(b, a)) }
}
```

Normalize edilmiş çift: (USDC, WETH) her zaman (küçük_addr, büyük_addr) formatında. Bu pool lookup için canonical key sağlar.

**DEX Uygulama Notu:**
Nova Shield DEX'te tüm likidite kaynaklarını saran tek `Liquidity` enum kritik tasarım kararı. Solver/routing engine tek tip üzerinde çalışır, source-specific logic boundary'de kalır.

---

## 12. domain/liquidity/uniswap/v2.rs (85 satır)

**Amaç:** UniswapV2 havuz domain modeli.

### Struct

```rust
pub struct Pool {
    pub address: eth::Address,
    pub router: eth::ContractAddress,
    pub reserves: Reserves,
}

pub struct Reserves(eth::Asset, eth::Asset);  // token + amount çifti, adrese göre sıralı
```

### AMM Formülü

Uniswap V2: **x * y = k** (constant product formula)

- x: token0 rezervi
- y: token1 rezervi
- k: sabit (swap sırasında değişmez)

Swap çıktısı: `dy = y * dx / (x + dx)` (fee öncesi)
Fee sonrası: `dy = y * (dx * 997) / (x * 1000 + dx * 997)` (0.3% fee)

Reserve kısıtı: `max 2^112` (uint112 — boundary'de kontrol edilir)

### swap()

```rust
fn swap(&self, input: eth::Asset) -> Result<Interaction> {
    // validate: input token reserves'te var mı?
    // delegate: boundary::liquidity::uniswap::v2::to_interaction(pool, input)
}
```

Domain swap() boundary'e delege eder. Domain sadece doğrulama yapar.

**DEX Uygulama Notu:**
Nova Shield DEX native AMM için x*y=k başlangıç noktası. Reserve limit uint112 (2^112 yaklaşık 5.2e33) üstü miktarlarda overflow olur — kontrol zorunlu.

---

## 13. domain/liquidity/uniswap/v3.rs (80 satır)

**Amaç:** UniswapV3 concentrated liquidity havuz modeli.

### Struct

```rust
pub struct Pool {
    pub router: eth::ContractAddress,
    pub address: eth::Address,
    pub tokens: TokenPair,
    pub sqrt_price: SqrtPrice,       // Q64.96 format current price
    pub liquidity: Liquidity(u128),  // aktif likidite
    pub tick: Tick(i32),             // mevcut tick indeksi
    pub liquidity_net: Arc<BTreeMap<i32, i128>>,  // tick → net likidite değişimi
    pub fee: Fee,
}
```

### Arc Kullanım Nedeni

`liquidity_net: Arc<BTreeMap<i32, i128>>` — tick map klonlamak pahalı. `Arc` ile her /solve isteğinde referans kopyalanır, veri kopyalanmaz.

### V3 Fiyat Formülü

- Fiyat aralıklar (tick'ler) halinde. Her tick: `sqrt(1.0001)^tick` fiyat oranına karşılık gelir
- `sqrt_price`: Q64.96 fixed-point (64 bit tamsayı, 96 bit kesir)
- Concentrated liquidity: sadece aktif tick'te likidite sunulur

**Matematiksel Formül:**
```
price_at_tick(i) = 1.0001^i
sqrt_price_x96 = sqrt(price) * 2^96
liquidity_delta = liquidity_net[tick]  // tick geçilince likidite değişimi
```

**DEX Uygulama Notu:**
Nova Shield DEX'te concentrated liquidity için `BTreeMap<i32, i128>` (tick index → net delta) temel veri yapısı. Arc wrap + Q64.96 format doğrudan kullanılabilir.

---

## 14. domain/liquidity/balancer/v2/mod.rs (73 satır)

**Amaç:** Balancer V2 ortak tipleri.

### Pool ID Yapısı

```rust
pub struct Id(B256);
// bytes 0..20:  pool address
// bytes 20..22: pool specialization (0=General, 1=MinimalSwapInfo, 2=TwoToken)
// bytes 22..32: nonce (her pool için benzersiz)
```

Balancer pool ID'si compound: adres + tip + nonce tek 32 byte'ta.

### Fee ve ScalingFactor

```rust
pub struct Fee(U256);           // f * 1e18 (ör: 0.003 → 3_000_000_000_000_000)
pub struct ScalingFactor(U256); // f * 1e18, sıfır olamaz
```

ScalingFactor: token decimals farkını normalize eder. USDC (6 decimal) + WETH (18 decimal) denklemi için scaling factor kullanılır.

**DEX Uygulama Notu:**
Nova Shield DEX multi-token pool destekleyecekse Balancer V2'nin pool ID yapısı (compound bytes) ve scaling factor mekanizması iyi referans.

---

## 15. domain/liquidity/balancer/v2/stable.rs (143 satır)

**Amaç:** StableSwap (Curve.fi algoritması) Balancer V2 üzerinde.

### AmplificationParameter

```rust
pub struct AmplificationParameter {
    pub factor: U256,
    pub precision: U256,
}

impl AmplificationParameter {
    pub fn as_big_uint(&self) -> Option<BigUint> {
        // overflow-checked: factor * precision
    }
}
```

### StableSwap Formülü

Curve.fi invariant:
```
A * n^n * sum(x_i) + D = A * D * n^n + D^(n+1) / (n^n * prod(x_i))
```

- A: amplification parameter (büyük A → daha fazla likidite konsantrasyonu)
- n: token sayısı
- D: toplam likidite invariant
- x_i: her token rezervi

StableSwap x*y=k'ya göre: stablecoin swap'larında çok daha az slippage, çünkü fiyat sabit aralıkta tutulur.

### Reserves

```rust
pub struct Reserves(Vec<Reserve>);

fn all_unique(tokens: &[TokenAddress]) -> bool {
    // HashMap ile O(n) duplicate kontrol
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te stablecoin swap pool'u için StableSwap şart. A parametresi governance'la ayarlanabilir olmalı. Amplification faktörü ne kadar yüksek, pegged varlıklar arasında slippage o kadar düşük.

---

## 16. domain/liquidity/balancer/v2/weighted.rs (142 satır)

**Amaç:** Balancer V2 ağırlıklı havuz (Weighted Pool).

### Weighted Pool Formülü

```
invariant = prod(balance_i ^ weight_i)
```

Özel durum 2 token, eşit ağırlık: x*y=k (UniswapV2 ile aynı)
Genel: n token, farklı ağırlıklar mümkün (ör: 80/20 WETH/USDC)

### Version Farkı

```rust
pub enum Version {
    V0,
    V3Plus,  // 50/50 ve 20/80 için optimize edilmiş üs hesabı
}
```

V3Plus: 50/50 havuzda `sqrt()` kullanır (üs hesabından çok daha hızlı).

### Weight

```rust
pub struct Weight(U256);  // w * 1e18 (ör: 0.5 → 500_000_000_000_000_000)
```

**Matematiksel Not:**
Weighted swap çıktısı:
```
out = balance_out * (1 - (balance_in / (balance_in + amount_in))^(weight_in/weight_out))
```

**DEX Uygulama Notu:**
Nova Shield DEX'te çoklu token weight desteği (Balancer tarzı) eklenirse bu formül referans. 80/20 pool'lar: impermanent loss'u azaltır (majör varlık yüksek ağırlık).

---

## 17. domain/liquidity/swapr.rs (59 satır)

**Amaç:** Swapr (UniswapV2 fork, değişken fee) domain modeli.

```rust
pub struct Pool {
    pub inner: v2::Pool,
    pub fee: Fee,
}

pub struct Fee(u32);  // basis points, 0..=1000 (max %10)

impl Fee {
    pub fn try_new(bps: u32) -> Result<Self> {
        if bps > 1000 { Err(InvalidFee) } else { Ok(Self(bps)) }
    }
}
```

UniswapV2'den tek farkı: fee pool başına ayarlanabilir (0-10% arası). Inner Pool olarak v2::Pool kullanır.

**DEX Uygulama Notu:**
Nova Shield DEX'te pool başına ayarlanabilir fee: `Fee(u32)` + validation (0..=MAX_BPS) doğrudan kullanılabilir.

---

## 18. domain/liquidity/zeroex.rs (91 satır)

**Amaç:** 0x protokolü limit emirleri için domain modeli.

### Struct

```rust
pub struct LimitOrder {
    pub order: Order,
    pub fillable: Amounts,
    pub zeroex: Arc<IZeroex::Instance>,  // shared kontrat instance
}

pub struct ZeroExSignature {
    pub r: B256,
    pub s: B256,
    pub v: u8,
    pub signature_type: u8,  // EIP-712, EthSign, vb.
}
```

### to_interaction()

```rust
fn to_interaction(&self, fill_amount: TokenAmount) -> Interaction {
    // fillOrKillLimitOrder(order, signature, fill_amount)
    // takerAmount = fill_amount
}
```

"fillOrKill": tam doldurulmazsa revert. Bu slippage kontrolü sağlar.

**DEX Uygulama Notu:**
Nova Shield DEX'te off-chain limit order entegrasyonu: 0x v4 order struct + `fillOrKillLimitOrder` pattern kullanılabilir. `Arc<ContractInstance>` sharing ile aynı kontrat için binlerce order'da tek instance.

---

## 19. boundary/mod.rs (50 satır)

**Amaç:** Anti-corruption layer tanımı ve ortak export'lar.

```rust
pub fn unbuffered_web3(url: &Url, ...) -> Web3 {
    // max_batch_size = 0, max_concurrent_requests = 0
    // batch olmadan doğrudan RPC çağrıları (bazı liquidity fetch'ler için gerekli)
}
```

Re-exports: `anyhow::Error`, `contracts`, `model::order::OrderData`, `shared::web3::Web3`

**DEX Uygulama Notu:**
Nova Shield DEX'te de boundary/anti-corruption layer ayrı tutulmalı. External library tipleri domain'e sızmadan boundary'de wrapper'lanmalı.

---

## 20. boundary/liquidity/mod.rs (190 satır) — KRİTİK

**Amaç:** Tüm likidite kaynaklarını başlatır ve domain tiplerine çevirir.

### Cache Konfigürasyonu

```rust
fn cache_config() -> CacheConfig {
    CacheConfig {
        number_of_blocks_to_cache: 2,
        number_of_entries_to_auto_update: 1_000,
        maximum_recent_block_age: 4,
        max_retries: 5,
        delay_between_retries: Duration::from_secs(1),
        ..Default::default()
    }
}
```

5-dakika TTL, 2-blok cache, 1000 entry auto-update, 4-blok max yaş.

### try_new() — Kaynak Başlatma

```rust
let collectors = vec![
    uniswap_v2_collectors(&config, eth, &http_factory)?,
    swapr_collector(&config, eth)?,
    balancer_v2_collector(&config, eth, &http_factory)?,
    uniswap_v3_collector(&config, eth, &http_factory)?,
    zeroex_collector(&config, eth, &http_factory)?,
];
```

Her kaynak ayrı collector. Hepsi `LiquidityCollector` trait'ini implement eder.

### fetch() — Dispatch

```rust
fn fetch(&self, pairs: &HashSet<TokenPair>) -> Vec<Liquidity> {
    let raw = self.inner.get_liquidity(pairs, block);
    raw.iter().filter_map(|entry| match entry.settlement_handling.as_ref() {
        // UniswapV2LikeHandler → boundary::uniswap::v2::to_domain()
        // UniswapV3Handler    → boundary::uniswap::v3::to_domain()
        // BalancerV2Handler   → boundary::balancer::v2::to_domain()
        // SwaprHandler        → boundary::swapr::to_domain() (eğer router swapr_routers'da)
        // ZeroExHandler       → boundary::zeroex::to_domain()
    }).collect()
}
```

Swapr disambiguation: `pool.router ∈ swapr_routers_set` kontrolü. UniswapV2 ve Swapr aynı interface'i paylaşır, sadece fee hesabı farklı.

**DEX Uygulama Notu:**
Nova Shield DEX liquidity aggregation: her source için ayrı collector + shared cache (2-blok, TTL-based) + dispatch pattern. Swapr/UniswapV2 ayrımı gibi fork detection için router address whitelist yeterli.

---

## 21. boundary/liquidity/uniswap/v2.rs (153 satır)

**Amaç:** UniswapV2 boundary: domain çevirisi ve interaction encoding.

### Gas Sabiti

```rust
pub const GAS_PER_SWAP: u64 = 90_171;
// Kaynak: Dune Analytics query 640717 (gerçek on-chain ölçüm)
```

### to_domain()

```rust
fn to_domain(pool: &PoolEntry) -> Option<liquidity::uniswap::v2::Pool> {
    assert_eq!(fee, Rational::new(3, 1000));  // sadece 0.3% destekleniyor
    assert!(reserve0 < 2^112 && reserve1 < 2^112);  // uint112 bound
    ...
}
```

### to_interaction()

```rust
fn to_interaction(pool: &Pool, input: Asset) -> Interaction {
    // UniswapV2Inner::settle(input, output, direction)
    // encode_swap() → calldata
}
```

### collector()

```rust
fn collector(config, eth) -> PoolCache<UniswapLikeLiquidity> {
    PoolFetcher → PoolCache(cache_config()) → UniswapLikeLiquidity
}
```

**DEX Uygulama Notu:**
Gas per swap sabitini on-chain ölçümden al (Dune gibi). 90,171 gas rakamı yönlendirme kararlarında kullanılır — daha ucuz kaynak öncelik alır.

---

## 22. boundary/liquidity/uniswap/v3.rs (176 satır)

**Amaç:** UniswapV3 boundary: concentrated liquidity tick verisini domain'e çevirir.

### to_domain()

```rust
fn to_domain(entry: &PoolEntry) -> Option<liquidity::uniswap::v3::Pool> {
    assert_eq!(entry.tokens.len(), 2);
    let handler = entry.settlement_handling
        .downcast::<UniswapV3SettlementHandler>()?;
    Pool {
        sqrt_price: handler.sqrt_price,
        liquidity_net: Arc::new(handler.tick_data.liquidity_net.clone()),
        ...
    }
}
```

### Pool Kaynağı Seçimi

```rust
match config.pool_source {
    PoolSource::PoolIndexer => indexer_source(config),  // CoW'un kendi indexer'ı
    PoolSource::Subgraph => subgraph_source(config),    // Goldsky subgraph
}
```

BackgroundInitLiquiditySource: V3 pool listesi başlangıçta yüklenmesi zaman alır (10 dakika timeout). Background task ile initialize edilir.

**DEX Uygulama Notu:**
Nova Shield DEX'te V3 tarzı concentrated liquidity için tick data `BTreeMap<i32, i128>` ve `Arc` wrap şart. Pool indexer için Graph Protocol subgraph veya özel RPC-based indexer seçilmeli.

---

## 23. boundary/liquidity/balancer/v2/mod.rs (193 satır)

**Amaç:** Balancer V2 likidite boundary.

### Gas Sabiti

```rust
pub const GAS_PER_SWAP: u64 = 88_892;  // Balancer V2 biraz daha verimli (UniV2: 90,171)
```

### 5 Factory Tipi

```rust
enum FactoryKind {
    Weighted,
    WeightedV3,
    StableV2,
    LiquidityBootstrapping,
    ComposableStable,
}
```

Her factory farklı pool tipi üretir. Balancer V2 aynı vault üzerinde 5 farklı pool mekaniğini destekler.

### to_interaction()

```rust
fn to_interaction(pool: &Pool, swap: Swap) -> Interaction {
    // receiver == sender assumption (settlement contract hepsini halleder)
    vault.swap(swap, funds, limit, deadline)
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te Vault pattern (Balancer tarzı): token transferlerini merkezi vault'ta tutar, pool'lar sadece matematik yapar. Gas optimize — her swap'ta token transfer yok, sadece internal accounting.

---

## 24. boundary/liquidity/swapr.rs (56 satır)

**Amaç:** Swapr fee'sini boundary'de doğrular ve domain tipine çevirir.

```rust
const BPS_BASE: u32 = 10_000;

fn to_domain(pool: &PoolEntry) -> Option<swapr::Pool> {
    // fee = numer/denom (Rational)
    // Doğrulama: numer * BPS_BASE % denom == 0 (tam bölünebilir olmalı)
    let bps = numer * BPS_BASE / denom;
    swapr::Fee::try_new(bps)?
}
```

Exact BPS representation kontrolü: 0.3% = 3/1000 = 30 BPS. Eğer tam bölünmüyorsa pool atlanır.

---

## 25. boundary/liquidity/zeroex.rs (102 satır)

**Amaç:** 0x limit order'larını domain'e çevirir.

```rust
fn to_domain(entry: &LiquidityEntry) -> Option<zeroex::LimitOrder> {
    let handler = entry.settlement_handling
        .downcast::<zeroex::OrderSettlementHandler>()?;
    LimitOrder {
        order: handler.order.clone(),
        fillable: handler.fillable_amounts(),
        zeroex: Arc::clone(&handler.contract),  // shared kontrat instance
    }
}
```

`collector()`: `DefaultZeroExApi` + `HttpClientFactory` → `ZeroExLiquidity`

**DEX Uygulama Notu:**
Nova Shield DEX off-chain order book için 0x pattern: off-chain sign → on-chain fill. `Arc<Contract>` sharing ile RPC bağlantısı minimize.

---

## 26. infra/mempool/mod.rs (225 satır)

**Amaç:** Fiziksel mempool bağlantısı ve transaction gönderme.

### Config

```rust
pub struct Config {
    pub min_priority_fee: eth::U256,
    pub gas_price_cap: eth::U256,          // default: 1000 gwei
    pub target_confirm_time: Duration,     // default: 30s
    pub retry_interval: Duration,          // default: 2s
    pub nonce_block_number: BlockNumberKind,
    pub url: Url,
    pub name: String,
    pub revert_protection: RevertProtection,
    pub max_additional_tip: eth::U256,     // default: 3 gwei
    pub additional_tip_percentage: f64,   // default: 5%
}
```

### get_nonce()

```rust
async fn get_nonce(&self, signer: Address) -> Result<U256> {
    match self.config.nonce_block_number {
        BlockNumberKind::Pending => eth_getTransactionCount(signer, "pending"),
        BlockNumberKind::Latest  => eth_getTransactionCount(signer, "latest"),
        // ...
    }
}
```

"pending" nonce: mempool'daki bekleyen tx'leri sayar. "latest" nonce: sadece on-chain finalized.

### submit()

```rust
async fn submit(&self, tx: TransactionRequest) -> Result<TxId> {
    let signed = tx.sign(self.signer).await?;
    let tx_id = self.transport.send_raw_transaction(signed).await?;
    self.last_submissions.insert(signer, Submission { tx_id, gas_price, nonce });
    Ok(tx_id)
}
```

### reverts_can_get_mined()

```rust
fn reverts_can_get_mined(&self) -> bool {
    self.config.revert_protection == RevertProtection::Disabled
}
```

RevertProtection::Enabled → sadece private mempool'a gönder.
RevertProtection::Disabled → public mempool'a da gönder.

**DEX Uygulama Notu:**
Nova Shield DEX'te mempool abstraction: `Config` struct + `DashMap` last_submissions. Public + private mempool yarışı için `Vec<Mempool>` + `select_ok`. Nonce block tag konfigürasyonu MEV protection seçeneği.

---

## 27. infra/solver/mod.rs (715 satır) — KRİTİK

**Amaç:** External solver API istemcisi.

### Struct

```rust
pub struct Solver {
    client: reqwest::Client,
    config: Config,
    eth: Ethereum,
    persistence: Persistence,
}

pub enum Account {
    PrivateKey(PrivateKeySigner),
    Kms(AwsSigner),
    Address(Address),
}
```

### Config'den Önemli Alanlar

| Alan | Varsayılan | Açıklama |
|------|-----------|---------|
| solving_share_of_deadline | 0.8 | Solver'a verilen zaman oranı |
| response_size_limit_max_bytes | 30MB | Maksimum solver yanıt boyutu |
| settle_queue_size | 2 | Paralel settlement kuyruğu |
| max_solutions_to_propose | 1 | Birden fazla çözüm önerisi |
| haircut_bps | 0 | Fiyat haircut (baz puan) |
| flashloans_enabled | false | Flash loan desteği |
| fetch_liquidity_at_block | Latest | Hangi blok için likidite |

### solve() İç Akışı

```
1. assemble_flashloan_hints(auction)
2. Build wrappers (buy_eth_address handling)
3. auction → solvers_dto::auction::Auction DTO
4. POST /solve (streaming JSON body)
5. Optional: gzip compress + S3 archive (fire-and-forget)
6. Parse response: SolverResponse → Vec<Solution>
7. into_domain(): DTO → domain Solution
```

### Streaming Body

```rust
let (sender, body) = channel(1);
tokio::spawn(async move {
    sender.send(chunk).await
});
```

Büyük auction verisi için streaming: tüm JSON belleğe yüklenmez, stream edilir.

### Validasyon

```rust
if config.max_solutions_to_propose > 1 && config.submission_accounts.is_empty() {
    return Err("multiple solutions require submission accounts");
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te solver API istemcisi: streaming POST body (büyük veri için), S3 arşivleme (audit trail), AWS KMS signing (production key management).

---

## 28. infra/solver/eip7702.rs (585 satır) — KRİTİK

**Amaç:** EIP-7702 delegation kurulumu — paralel settlement için.

### Sabitler

```rust
pub const DELEGATION_PREFIX: [u8; 3] = [0xef, 0x01, 0x00];
pub const CREATE2_DEPLOYER: Address = address!("4e59b44847b379578588920cA78FbF26c0B4956C");
pub const MAX_APPROVED_CALLERS: usize = 5;
```

### Amaç

Normal EIP-7702: bir EOA, bir akıllı kontratın kodunu "borrow" eder.
Driver'ın kullanımı: `submission_account` (yardımcı EOA) `Solver7702Delegate` kontratının kodunu alır → solver adına settlement gönderebilir.

### delegate_deployment()

```rust
fn delegate_deployment(approved_callers: Vec<Address>, salt: B256) -> Result<(Bytes, Address)> {
    // Validasyonlar: hiçbiri sıfır, hepsi unique, max 5
    let init_code = bytecode + abi_encode(callers_array_padded_to_5);
    let code_digest = keccak256(init_code);
    let address = create2_address(CREATE2_DEPLOYER, salt, code_digest);
    (init_code, address)
}
```

CREATE2 formülü: `address = keccak256(0xff || deployer || salt || keccak256(init_code))[12:]`

### setup_solver() Durum Makinesi

| Mevcut Durum | Action |
|-------------|--------|
| DelegatedTo(başka) | ERROR bail |
| OtherCode | ERROR bail |
| DelegatedTo(doğru) + kontrat missing | Sadece deploy |
| DelegatedTo(doğru) + kontrat exists | Skip |
| Empty + kontrat missing | Deploy + Delegate (tek tx) |
| Empty + kontrat exists | Sadece Delegate |

### sign_authorization()

```rust
fn sign_authorization(solver_signer: &Signer, delegate_address: Address) -> Result<Authorization> {
    // auth_nonce = solver_nonce + 1 (tahmin: auth işlenirken nonce artacak)
    let auth = Authorization { chain_id, address: delegate_address, nonce: auth_nonce };
    solver_signer.sign_authorization(auth)
}
```

### Tests

```rust
#[test] fn delegate_target_stable_and_caller_order_sensitive()
// CREATE2 adresi caller sırasına DUYARLI

#[test] fn pads_approved_callers_to_5()
// Her zaman tam 5 caller encode (trailing zeros)

#[test] fn rejects_more_than_5_callers()
#[test] fn rejects_zero_and_duplicate_accounts()
```

**Güvenlik Analizi:**
- Max 5 caller: kontrat boyutunu sınırlar
- Sıra duyarlılığı: caller sırasını değiştirme = yeni kontrat deploy = maliyet
- Nonce tahmin: `+1` nonce pending tx varsa
- Zero adres reddi: yanlış approved_caller önlenir

**DEX Uygulama Notu:**
Nova Shield DEX'te paralel settlement için EIP-7702: `approved_callers` listesi + CREATE2 deployment + delegation. Max 5 caller limiti ve sıra duyarlılığı dikkat gerektirir.

---

## 29. infra/blockchain/mod.rs (407 satır)

**Amaç:** Ethereum blockchain ile tüm etkileşimler.

### Struct

```rust
pub struct Ethereum {
    web3: Web3,
    inner: Arc<Inner>,
}

struct Inner {
    chain: eth::ChainId,
    contracts: Contracts,
    gas: GasPriceEstimator,
    current_block: CurrentBlockWatcher,
    balance_simulator: BalanceOverrideSimulator,
    state_overrider: StateOverrider,
    tx_gas_limit: eth::Gas,
}
```

### estimate_gas()

Pending block üzerinde tahmin: mempool'daki tx'leri de hesaba katar.

### simulation_gas_price()

```rust
fn simulation_gas_price(&self) -> Result<eth::GasPrice> {
    // "sneaky token" saldırısını önlemek için
    // Gas price = 0 olursa bazı token'lar farklı davranır (gas refund exploit)
    let base_fee = self.current_block().base_fee_per_gas?;
    Ok(eth::GasPrice::new(base_fee, eth::U256::ZERO))
}
```

### successful_settlement_block()

```rust
async fn successful_settlement_block(&self, tx_id: TxId) -> Result<Option<u64>> {
    // eth_getLogs(Settlement event, from=submission_block, to=Latest)
    // Log indexing receipt'ten daha güvenilir
}
```

### Error Sınıflandırması

```rust
impl Error {
    pub fn is_revert(&self) -> bool {
        match self {
            Error::ContractRpc(_) => true,
            Error::Rpc(e) => e.is_error_resp(),
            Error::GasPrice(_) => false,
            Error::AccessList(_) => true,
        }
    }
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te `Ethereum` struct aynı pattern ile: `Arc<Inner>` ile paylaşılan blockchain state, simulation gas price trick (sneaky token saldırı önlemi), Settlement event log indexing.

---

## 30. infra/blockchain/gas.rs (150 satır)

**Amaç:** EIP-1559 gas fiyat tahmini ve adjustment.

### estimate() Algoritması

```
1. base_estimate = gas_estimator.estimate()
2. additional_tip = min(max_additional_tip, priority_fee * tip_pct)
3. adjusted_priority = max(min_priority_fee, priority_fee + additional_tip)
4. if adjusted_priority > max_fee_per_gas → Err(GasPriceTooHigh)
5. return GasPrice { max_fee: max_fee_per_gas, priority_fee: adjusted_priority }
```

**Örnek Hesap:**
- base_fee = 20 gwei, priority_fee = 2 gwei
- additional_tip = min(3 gwei, 2 * 0.05) = 0.1 gwei
- adjusted = max(1 gwei min, 2 + 0.1) = 2.1 gwei
- Final: maxFeePerGas=1000 gwei, maxPriorityFeePerGas=2.1 gwei

### Estimator Tipleri

```rust
enum GasEstimatorKind {
    Web3,                         // eth_gasPrice
    Alloy {
        past_blocks: u64,         // default: 10
        reward_percentile: f64,   // default: 20.0
    },
}
```

Alloy fee history: son 10 bloktaki ödüllerin 20. persentilini kullanır. Konservatif ama güvenilir.

### adjustments()

```rust
fn adjustments(mempool_configs: &[MempoolConfig]) -> GasPriceParameters {
    GasPriceParameters {
        max_additional_tip: configs.iter().map(|c| c.max_additional_tip).max(),
        min_priority_fee: configs.iter().map(|c| c.min_priority_fee).max(),
        max_fee_per_gas: configs.iter().map(|c| c.gas_price_cap).min(),
        // Çoklu mempool: en agresif tip + en düşük cap
    }
}
```

**DEX Uygulama Notu:**
Nova Shield DEX'te EIP-1559 gas: fee history percentile tabanlı tahmin (Alloy tarzı) + minimum priority fee + additional tip. Çoklu mempool için "max tip, min cap" kombinasyonu.

---

## 31. infra/blockchain/contracts.rs (154 satır)

**Amaç:** Smart contract instance'larını initialize eder.

### İnit Akışı

```rust
// 1. Chain'e göre varsayılan adresler yükle (network registry)
// 2. Config override'larını uygula
// 3. vault_relayer = settlement.vaultRelayer().call() (on-chain okuma)
// 4. settlement_domain_separator = settlement.domainSeparator().call()
// 5. CoW AMM helper'larını factory adreslerine göre map'le
```

vault_relayer ve domain_separator on-chain'den runtime'da yüklenir — hardcode değil.

---

## 32. infra/blockchain/token.rs (81 satır)

**Amaç:** ERC20 token metadata ve balance.

```rust
pub async fn decimals(&self) -> Option<u8>    // None: non-standard token
pub async fn symbol(&self) -> Option<String>  // None: non-standard token
```

`Option` dönüşleri: bazı token'lar (ör: eski USDT) `decimals()` veya `symbol()` implement etmez.

---

## 33. infra/config/file/mod.rs (1217 satır)

**Amaç:** TOML konfigürasyonu parse eder ve doğrular.

### Kritik Varsayılanlar

| Parametre | Varsayılan | Açıklama |
|-----------|-----------|---------|
| solving_share_of_deadline | 0.8 | Solver'a %80 zaman |
| http_time_buffer | 500ms | HTTP gecikmesi buffer |
| max_solutions_to_propose | 1 | Tekil çözüm |
| settle_queue_size | 2 | Paralel settlement kuyruğu |
| response_size_limit_max_bytes | 30MB | Max solver yanıt |
| gas_price_cap | 1000 gwei | Max gas fiyatı |
| target_confirm_time | 30s | İstenen onay süresi |
| retry_interval | 2s | Tekrar deneme aralığı |
| max_additional_tip | 3 gwei | Max ekstra tip |
| additional_tip_percentage | 5% | Tip artış yüzdesi |

### OrderPriority Stratejileri

Varsayılan sıra: `[OwnQuotes, CreationTimestamp, ExternalPrice]` (5-dakika yaş ile)

### BadOrderDetection

```rust
pub struct MetricsStrategy {
    failure_ratio: 0.9,         // %90 başarısızlık → kötü emir
    required_measurements: 20,  // minimum ölçüm sayısı
}
```

**DEX Uygulama Notu:**
Nova Shield DEX konfigürasyonu: `deny_unknown_fields` (typo koruması), kebab-case TOML, runtime validasyon.

---

## 34. infra/liquidity/config.rs (320 satır)

**Amaç:** Likidite kaynaklarını konfigürasyon üzerinden tanımlar.

### UniswapV2 Presets

```rust
pub enum UniswapV2Preset {
    UniswapV2,       // Ethereum Mainnet
    SushiSwap,       // Multi-chain
    Honeyswap,       // Gnosis Chain
    Baoswap,         // Gnosis Chain
    PancakeSwap,     // BNB Chain (farklı init hash)
    TestnetUniswapV2,
}
```

Her preset: factory address + router address + init code hash (chain'e özel).

**DEX Uygulama Notu:**
Nova Shield DEX multi-chain desteği için her chain'e özel factory adresleri + init code hash mapping. Preset enum pattern güzel — yeni chain eklemek = yeni variant.

---

## 35. infra/liquidity/fetcher.rs (73 satır)

**Amaç:** Boundary Fetcher'ı infra katmanında wrap eder.

```rust
pub enum AtBlock {
    Recent,    // cache'den (2 blok eski olabilir)
    Latest,    // mevcut blok
    Finalized, // finalized blok
}
```

`fetch()`: boundary fetch → observe log → error swallow → return

Hataları sessizce yutar (likidite eksikliği = daha az çözüm, kritik hata değil).

---

## 36. infra/observe/mod.rs (469 satır)

**Amaç:** Tüm observable event'leri merkezi bir yerde tanımlar.

### Temel Event'ler

```rust
pub fn auction(auction_id, order_count)
pub fn fetching_liquidity()
pub fn fetched_liquidity(liquidity: &[Liquidity])  // type bazlı gruplama
pub fn solutions(solutions: &[competition::Solution])
pub fn scoring(score: &Score)
pub fn settlement(settlement_id, tx_hash)
pub fn mempool_log(result: &SubmissionResult)
pub fn mempool_submission_result(results, winner: Option<TxId>)
```

### Superseded Reclassification

```rust
pub fn mempool_submission_result(results: &[SubmissionResult], winner: Option<TxId>) {
    for result in results {
        let label = if result.tx_id == winner {
            "winner"
        } else if matches!(result.status, Failed) && winner.is_some() {
            "superseded"  // kaybeden → superseded (başarısız değil)
        } else {
            result.status.as_str()
        };
    }
}
```

**DEX Uygulama Notu:**
Nova Shield DEX observe: Superseded vs Failed ayrımı Grafana dashboardlarında "yarış kaybı" vs "gerçek hata" ayrımı için kritik.

---

## 37. infra/notify/notification.rs (77 satır)

**Amaç:** Notification payload tipleri.

```rust
pub enum Kind {
    Timeout,
    EmptySolution,
    DuplicatedSolutionId(SolutionId),
    SimulationFailed(BlockNo, Tx, bool),    // bool: succeeded_after_revert
    ScoringFailed(ScoreKind),
    NonBufferableTokensUsed(BTreeSet<TokenAddress>),
    SolverAccountInsufficientBalance(Ether),
    SettlementStarted { auction_id },
    Settled(Settlement),
    DriverError(String),
    PostprocessingTimedOut,
    DeserializationError(String),
}

pub enum Settlement {
    Success(TxId),
    Revert(TxId),
    SimulationRevert,
    Expired,
    Fail,
}
```

`SimulationFailed(..., true)`: simülasyon revert etti ama revert sonrası blokta başarılı oldu → sandwich saldırısı veya race condition.

**DEX Uygulama Notu:**
Nova Shield DEX'te solver feedback loop: her hata kategorisi için ayrı notification. `succeeded_after_revert` flag sandwich saldırısı tespiti için.

---

## 38. infra/persistence/mod.rs (89 satır)

**Amaç:** Auction verilerini S3'e arşivler.

```rust
pub fn archive_auction_gzipped(&self, bytes: oneshot::Receiver<Bytes>) {
    tokio::spawn(async move {
        let data = bytes.await.ok()?;
        uploader.upload(data).await;
    });
}
```

Fire-and-forget: upload başarısız olsa bile driver çalışmaya devam eder.

---

## 39. infra/tokens.rs (244 satır)

**Amaç:** ERC20 token metadata cache'i.

### Struct

```rust
struct Inner {
    cache: RwLock<HashMap<TokenAddress, Token>>,
    sharing: BoxRequestSharing<TokenAddress, Token>,
    ethereum: Ethereum,
}
```

`BoxRequestSharing`: aynı token için eş zamanlı 10 request gelirse sadece 1 RPC çağrısı yapılır.

### Algoritma

```
1. read lock → cache'de var mı?
2. BoxRequestSharing: başka task aynı token'ı fetch ediyorsa bekle
3. try_join3(decimals, symbol, balance) — paralel RPC
4. write lock → cache'e ekle
```

`BUY_ETH_ADDRESS` native ETH: ERC20 değil, cache'e giremez.

**DEX Uygulama Notu:**
Nova Shield DEX token cache: `BoxRequestSharing` (deduplication) + `RwLock<HashMap>` + per-block balance update. Native token pseudo-address özel case.

---

## 40. infra/time.rs (14 satır)

**Amaç:** Test/production zaman ayrımı.

```rust
#[cfg(test)]
pub fn now() -> chrono::DateTime<chrono::Utc> {
    use std::sync::LazyLock;
    static TIME: LazyLock<chrono::DateTime<chrono::Utc>> = LazyLock::new(chrono::Utc::now);
    *TIME
}
```

Test ortamında zaman ilk çağrıda dondurulur. Deadline testleri deterministic.

**DEX Uygulama Notu:**
Nova Shield DEX test setup'ı: `LazyLock` ile frozen time. Order expiry, auction deadline gibi time-sensitive test senaryoları için kritik.

---

## 41. infra/api/mod.rs (219 satır)

**Amaç:** HTTP API sunucusu.

### Endpoint Yapısı

```
/metrics        — Prometheus metrics
/healthz        — Health check
/gasprice       — Mevcut gas fiyatı

Per-solver prefix (/{solver_name}/):
  /solve        — POST: auction → solutions
  /quote        — POST: order → quote
  /reveal       — POST: çözümü reveal et
  /settle       — POST: settlement başlat
  /info         — GET: solver bilgisi
```

### Middleware

```rust
let app = Router::new()
    .layer(axum::DefaultBodyLimit::disable())
    .layer(tower_http::decompression::RequestDecompressionLayer::new())
    .layer(tower_http::trace::TraceLayer::new_for_http()
        .make_span_with(|request| {
            tracing::info_span!("request", id = request_id)
        }))
```

### Competition Mimarisi

Her solver için ayrı `domain::Competition` instance oluşturulur.

**DEX Uygulama Notu:**
Nova Shield DEX API: per-solver prefix + ayrı Competition instance + Prometheus metrics + distributed trace. `DefaultBodyLimit::disable()` önemli — büyük auction response'lar için.

---

## 42. infra/cli.rs (53 satır)

**Amaç:** Komut satırı argümanları.

```rust
pub struct Args {
    pub addr: SocketAddr,           // default: "0.0.0.0:11088"
    pub log: String,                // default: "warn,driver=debug,..."
    pub ethrpc: Url,
    pub ethrpc_max_batch_size: usize,          // default: 20
    pub ethrpc_max_concurrent_requests: usize, // default: 10
    pub config: PathBuf,
}
```

RPC batch: max 20 request/batch, max 10 concurrent batch = max 200 concurrent RPC call.

---

## Genel DEX Uygulama Özeti

### Nova Shield DEX için Doğrudan Uygulanabilir Pattern'ler

1. **Multi-Mempool Yarışı:** `select_ok` + `Vec<Mempool>` + `DashMap<Address, LastSubmission>`
2. **Gas Replacement:** `%13 bump` (Ethereum minimum %12.5 üstü)
3. **Quote via Fake Auction:** tek emirli sentetik auction → solver'ı test eder
4. **EIP-7702 Paralel Submission:** `Solver7702Delegate` + CREATE2 + approved_callers
5. **Likidite Cache:** 2-blok, 1000 entry, 5-min TTL, 4-blok max yaş
6. **Haircut Mekanizması:** Quote fiyatları gerçek settlement'tan muhafazakâr
7. **BoxRequestSharing:** Duplicate RPC call deduplication
8. **Domain-Boundary-Infra Katmanlama:** DDD anti-corruption layer
9. **Streaming Solver Request:** büyük veri için streaming POST body
10. **Frozen Time in Tests:** `LazyLock<DateTime<Utc>>` test determinizmi

### Matematiksel Formüller Özeti

| Formül | Bağlam |
|--------|--------|
| `x*y=k` | UniswapV2 CFMM |
| `dy = y*(dx*997)/(x*1000+dx*997)` | UniswapV2 swap output (0.3% fee) |
| `price = 1.0001^tick` | UniswapV3 tick fiyatı |
| `sqrt_price_x96 = sqrt(price) * 2^96` | UniswapV3 Q64.96 format |
| `invariant = prod(x_i^w_i)` | Balancer Weighted |
| `A*n^n*sum(x_i)+D = A*D*n^n + D^(n+1)/(n^n*prod(x_i))` | StableSwap (Curve) |
| `new_price = old_price * 1.13` | Gas replacement minimum |
| `solver_time = (driver_deadline - now) * 0.8` | Solver deadline hesabı |
| `create2_addr = keccak256(0xff || deployer || salt || keccak256(init_code))[12:]` | EIP-7702 delegate deployment |

### Güvenlik Kriterleri

1. **MEV Koruması:** RevertProtection::Enabled → private mempool
2. **Sneaky Token:** simulation gas price != 0 (base_fee kullanılır)
3. **EIP-7702 Caller Check:** max 5, no zero, no duplicate
4. **USDT Double-Spend:** approve(0) → approve(max) sıralaması
5. **Flash Loan:** provider + adapter indirection (protocol-agnostic)
6. **`#![forbid(unsafe_code)]`:** tüm driver unsafe-free
7. **Nonce Yönetimi:** pending nonce + DashMap last_submission
8. **requires_cancellation:** sadece revert edecek AND mempool'da olan tx'ler iptal
