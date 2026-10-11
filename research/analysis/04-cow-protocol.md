# CoW Protocol — Kapsamlı Teknik Analiz

**Repo:** `research/libs/cow-protocol`
**Dil:** Rust 2024 Edition
**Runtime:** Tokio async
**Web3:** Alloy
**HTTP:** Axum
**DB:** PostgreSQL + sqlx
**Amaç:** Batch auction (toplu ihale) tabanlı merkeziyetsiz takas protokolü

---

## 1. Genel Mimari

CoW Protocol şu servisleri içerir:

```
User
  │
  ▼
Orderbook (HTTP API — order submit, quote, cancel)
  │  (PostgreSQL'de sakla)
  ▼
Autopilot (merkezi açık artırma yöneticisi)
  │  her ~12-15 saniyede bir auction döngüsü
  ├──▶ Driver 1 (Colocated solver)
  ├──▶ Driver 2 (Non-colocated: bizim driver + external solver API)
  └──▶ Driver N ...
         │ (her driver kendi solver'ına yönlendirir)
         ▼
      Solver Engine (rota bul, çöz)
         │
         ▼
      Driver (encode calldata, simüle et, submit et)
         │
         ▼
      Settlement Contract (on-chain execute)
         1. Pre-interactions (user pre-hooks)
         2. Sell token transfer
         3. Main interactions (swap/routing)
         4. Buy token payout
         5. Post-interactions (user post-hooks)
         │
         ▼
      Circuit Breaker (on-chain vs off-chain uyum izle)
```

**Solver türleri:**
- **Colocated:** External partner kendi driver+solver'ını çalıştırır. Tam kontrol, tam sorumluluk.
- **Non-colocated:** Biz driver çalıştırırız, partner'ın solver API endpoint'ine bağlanırız. Simülasyon ve submission bizden.

---

## 2. Crate Yapısı

```
crates/
├── orderbook/       — HTTP API, sipariş doğrulama ve depolama
├── autopilot/       — Merkezi auctioneer, döngü yöneticisi
├── driver/          — Likidite toplama, çözüm kodlama, zincire gönderme
├── solvers/         — Internal solver engine (baseline, dex)
├── model/           — API serialization modelleri
├── shared/          — Ortak: pricing, liquidity, gas, order_validation
├── database/        — PostgreSQL soyutlaması, migration'lar
├── contracts/       — Smart contract bindings
├── ethrpc/          — Extended Ethereum RPC, batching katmanı
├── chain/           — Blockchain interaction utilities
├── number/          — 256-bit integer uzantıları
├── app-data/        — Order metadata doğrulaması (8KB limit)
├── alerter/         — Order monitoring (çözülmesi gereken ama çözülmeyen)
├── testlib/         — Unit ve E2E test yardımcıları
└── observe/         — Logging ve metrics başlatma
```

---

## 3. Order Modeli

### 3.1 OrderUid — 56-byte Benzersiz Kimlik

`crates/model/src/order.rs`:

```rust
pub struct OrderUid(pub [u8; 56]);
```

OrderUid bileşimi:
- **Bytes 0-31:** EIP-712 order hash (32 byte)
- **Bytes 32-51:** Owner adresi (20 byte)
- **Bytes 52-55:** `valid_to` Unix timestamp (4 byte, u32)

Bu tasarım sayesinde OrderUid hem unique identifier hem de expire kontrolü için yeterlidir. Ayrı bir index gerekmez.

### 3.2 OrderData Struct

```rust
pub struct OrderData {
    pub sell_token: Address,
    pub buy_token: Address,
    pub receiver: Option<Address>,   // alıcı farklıysa
    pub sell_amount: U256,
    pub buy_amount: U256,
    pub valid_to: u32,               // Unix timestamp
    pub app_data: AppDataHash,       // 32-byte metadata hash
    pub fee_amount: U256,            // SIFIR — protocol fees off-chain
    pub kind: OrderKind,             // Buy veya Sell
    pub partially_fillable: bool,    // kısmi dolum
    pub sell_token_balance: SellTokenSource,
    pub buy_token_balance: BuyTokenDestination,
}
```

**Önemli:** `fee_amount` SIFIR olarak gönderilir. Protokol ücretleri off-chain hesaplanır ve clearing price'a dahil edilir. Bu MEV açısından kritik: fee bilgisi transparent değildir.

### 3.3 OrderKind (Side)

```rust
pub enum OrderKind { Sell, Buy }
```

- **Sell order:** Belirli miktarda sell_token sat, mümkün olduğu kadar buy_token al
- **Buy order:** Belirli miktarda buy_token al, mümkün olduğu kadar az sell_token harca

### 3.4 Signature Türleri

```rust
pub enum Scheme {
    Eip712,       // EIP-712 structured data signature
    EthSign,      // eth_sign ile imzalanmış (Metamask legacy)
    Eip1271,      // Smart contract wallet (ERC-1271)
    PreSign,      // On-chain pre-signature (EIP-1271 alternatifi)
}
```

EIP-1271 destekli smart contract wallet'lar sözleşme imzası kullanabilir. Bu, MPC cüzdanlar için kritik bir özellik; threshold imza doğrulaması contract seviyesinde yapılabilir.

### 3.5 SellTokenSource / BuyTokenDestination

```rust
pub enum SellTokenSource {
    Erc20,          // Normal ERC-20 balance
    External,       // Vault (Balancer) external balance
    Internal,       // Vault internal balance (settlement contract buffer)
}

pub enum BuyTokenDestination {
    Erc20,
    Internal,
}
```

Internal balance kullanımı gas tasarrufu sağlar: token'lar settlement contract'ın kendi buffer'ında tutulur.

### 3.6 Order Statüleri

```rust
pub enum OrderStatus {
    Open,                   // Aktif, çözüm bekliyor
    Fulfilled,              // Tamamlandı
    Cancelled,              // İptal edildi
    Expired,                // valid_to geçti
    PresignaturePending,    // On-chain pre-sign bekleniyor
}
```

OrderEventLabel (lifecycle tracking):
```rust
pub enum OrderEventLabel {
    Created,      // DB'ye eklendi
    Ready,        // Auction'a dahil edildi
    Filtered,     // Auction'dan çıkarıldı
    Considered,   // Solver tarafından değerlendirildi
    Executing,    // Kazanan çözüme dahil
    Traded,       // On-chain execute edildi
    Cancelled,    // İptal edildi
    Invalid,      // Geçersiz
}
```

---

## 4. Orderbook Servisi

### 4.1 Orderbook Struct

`crates/orderbook/src/orderbook.rs`:

```rust
pub struct Orderbook {
    domain_separator: DomainSeparator,        // EIP-712 domain
    settlement_contract: Address,             // settlement contract adresi
    database: crate::database::Postgres,      // yazma DB
    database_replica: crate::database::Postgres, // okuma DB (read replica)
    order_validator: Arc<dyn OrderValidating>,
    app_data: Arc<crate::app_data::Registry>,
    active_order_competition_threshold: u32,  // son N auction için kontrol
    order_simulator: Option<SettlementSimulator>, // Tenderly simülasyon
}
```

### 4.2 add_order() — Sipariş Ekleme Akışı

```rust
pub async fn add_order(
    &self,
    payload: OrderCreation,
) -> Result<(OrderUid, Option<QuoteMetadata>), AddOrderError>
```

Adımlar:
1. `app_data`: Hash ise DB'den tam veriyi getir
2. `get_replaced_order()`: Eğer app_data'da `replacedOrder` metadata varsa eski siparişi bul
3. `order_validator.validate_and_construct_order()`: Tam doğrulama
   - İmza doğrulama (EIP-712, EIP-1271, PreSign)
   - Balance ve allowance kontrolü
   - Token desteklenebilirlik
   - valid_to aralık kontrolü
   - Quote doğrulama
4. Eğer replaced order varsa `replace_order()` çağır, yoksa `insert_order()`
5. Metrik kaydet, OrderUid döndür

### 4.3 replace_order() — Order Değiştirme

```rust
pub async fn replace_order(
    &self,
    validated_new_order: Order,
    old_order: Order,
) -> Result<(), AddOrderError>
```

Güvenlik kontrolleri:
1. Yeni order ECDSA imzalı OLMALI (PreSign ile başkasının siparişi iptal edilemez)
2. Eski ve yeni siparişin sahibi AYNI OLMALI
3. Eski sipariş son N auction'da bid üzerinde OLMAMALI (`order_is_actively_bid_on`)

"Actively bid on" kontrolü: Son N competition'da hangi orderlar çözüme dahil edilmiş? Eğer eski order aktif ise double-spend riski var.

### 4.4 Error Hiyerarşisi

```rust
pub enum AddOrderError {
    OrderNotFound(OrderCancellationError),
    DuplicatedOrder,
    OrderValidation(ValidationError),
    Database(anyhow::Error),
    InvalidAppData(anyhow::Error),
    InvalidReplacement(OrderReplacementError),
    AppDataMismatch { contract_app_data, provided, existing },
    MetadataSerializationFailed(serde_json::Error),
}

pub enum OrderCancellationError {
    InvalidSignature,
    WrongOwner,
    OrderNotFound,
    AlreadyCancelled,
    OrderFullyExecuted,
    OrderExpired,
    OnChainOrder,      // PreSign order off-chain ile iptal edilemez
    Other(anyhow::Error),
}

pub enum ValidationError {
    Partial(PartialValidationError),
    AppData(AppDataValidationError),
    PriceForQuote(PriceEstimationError),
    MissingFrom,
    AppdataFromMismatch { from, app_data_signer },
    WrongOwner(signature::Recovered),
    InvalidEip1271Signature(hash),
    InsufficientBalance,
    InsufficientAllowance,
    InvalidSignature,
    NonZeroFee,           // Artık zero-fee protokolü
    SellAmountOverflow,
    TransferSimulationFailed(revert_data),
    QuoteNotVerified,
    ZeroAmount,
    IncompatibleSigningScheme,
    TooManyLimitOrders,
    TooMuchGas,
    Other(anyhow::Error),
}
```

### 4.5 Order Simülasyonu (Tenderly Entegrasyonu)

```rust
pub async fn simulate_order(
    &self,
    uid: &OrderUid,
    block_number: Option<u64>,
) -> Result<Option<OrderSimulationResult>, OrderSimulationError>
```

`SettlementSimulator` üzerinden Tenderly API'ı kullanılır:
- `simulation_builder::Order::new()` ile order construct edilir
- `.fill_at(ExecutionAmount::Remaining, PriceEncoding::LimitPrice)` — limit price ile doldur
- `.from_solver(Solver::Fake(None))` — sahte solver
- `.simulate_with_tenderly_report()` — Tenderly trace URL döndürür

Bu debug endpoint, kullanıcıların neden order'larının çalışmadığını anlamalarına yardımcı olur.

---

## 5. Autopilot — Run Loop

### 5.1 RunLoop Struct

`crates/autopilot/src/run_loop.rs`:

```rust
pub struct RunLoop {
    config: Config,
    eth: infra::Ethereum,
    persistence: infra::Persistence,
    drivers: Vec<Arc<infra::Driver>>,
    solvable_orders_cache: Arc<SolvableOrdersCache>,
    trusted_tokens: AutoUpdatingTokenList,
    probes: Probes,
    maintenance: MaintenanceSync,
    winner_selection: winner_selection::Arbitrator,
    wake_notify: Arc<tokio::sync::Notify>,  // yeni blok veya order gelince uyandır
}

pub struct Config {
    pub submission_deadline: u64,               // kaç blok sonra submit süresi dolar
    pub max_settlement_transaction_wait: Duration,
    pub min_solve_time: Duration,
    pub sync_solve_deadline_to_blockchain: Option<SlotConfig>,
    pub max_run_loop_delay: Duration,
    pub max_winners_per_auction: NonZeroUsize,  // birden fazla kazanan
    pub max_solutions_per_solver: NonZeroUsize,
    pub enable_leader_lock: bool,               // distributed leader election
    pub compress_solve_request: bool,
}
```

### 5.2 run_forever() — Ana Döngü

```rust
pub async fn run_forever(self, mut control: ShutdownController) {
    loop {
        leader_lock_tracker.try_acquire().await; // distributed leader lock
        self_arc.wake_notify.notified().await;   // yeni blok veya order
        let start_block = self_arc.update_caches(&mut last_block, is_leader).await;

        if !leader_lock_tracker.is_leader() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }

        if let Some(auction) = self_arc.next_auction(start_block, &mut last_auction, ...).await {
            self_arc.single_run(auction).await;
        }
    }
}
```

**Leader Lock:** Birden fazla autopilot instance çalışabilir (HA), ama sadece lider auction yönetir. Diğerleri cache günceller ve lider yoksa devre alır.

### 5.3 single_run() — Tek Auction Çalışması

```rust
async fn single_run(self: &Arc<Self>, auction: domain::Auction) {
    // 1. Tüm auction order'larını "Ready" olarak işaretle
    self.persistence.store_order_events(auction.orders.iter().map(|o| o.uid), OrderEventLabel::Ready);

    // 2. Tüm driver'lardan çözüm topla (paralel)
    let solutions = self.fetch_solutions(&auction).await;
    if solutions.is_empty() { return; }

    // 3. Kazananları seç (winner_selection::Arbitrator)
    let ranking = self.winner_selection.arbitrate(solutions, &auction);

    // 4. DB ve S3'e kaydet, competition verilerini sakla
    self.post_processing(&auction, competition_simulation_block, &ranking, block_deadline).await;

    // 5. Kazanan order'ları "Executing", diğerlerini "Considered" olarak işaretle
    // ...

    // 6. Her kazanan driver'a reveal + settle komutları gönder
    for (solution_uid, winner) in ranking.enumerated().filter(|(_, bid)| bid.is_winner()) {
        let (driver, solution) = (winner.driver(), winner.solution());
        // reveal calldata → settle
    }
}
```

---

## 6. Autopilot Domain — Auction

### 6.1 Auction Struct

`crates/autopilot/src/domain/auction/mod.rs`:

```rust
pub struct Auction {
    pub id: Option<Id>,
    pub orders: Vec<order::Order>,
    pub prices: Prices,   // HashMap<TokenAddress, Price>
    pub surplus_capturing_jit_order_owners: Vec<Address>,
}

// Token fiyatı: native token cinsinden
// price(token) = amount_of_native_token / amount_of_token
pub type Price = eth::Ether;
pub type Prices = HashMap<eth::TokenAddress, Price>;
```

### 6.2 Autopilot Order

`crates/autopilot/src/domain/auction/order.rs`:

```rust
pub struct Order {
    pub uid: OrderUid,              // 56-byte
    pub sell: eth::Asset,           // sell_token + sell_amount
    pub buy: eth::Asset,            // buy_token + buy_amount
    pub side: Side,                 // Sell | Buy
    pub class: Class,               // Market | Limit | Liquidity
    pub valid_to: u32,
    pub receiver: Option<eth::Address>,
    pub signature: Signature,
    pub sell_token_balance: SellTokenSource,
    pub buy_token_balance: BuyTokenDestination,
    pub partially_fillable: bool,
    pub fee: Fee,                   // protocol fee miktarı (off-chain hesaplanmış)
    pub protocol_fees: Vec<FeePolicy>,
    pub app_data: AppDataHash,
    pub pre_interactions: Vec<Interaction>,
    pub post_interactions: Vec<Interaction>,
    pub quote: Option<Quote>,
}

pub enum Class {
    Market,    // Limit price yakın, hızlı execute
    Limit,     // Belirli limit price bekler
    Liquidity, // JIT (Just-In-Time) likidite siparişleri
}
```

---

## 7. Fee Politikaları

`crates/autopilot/src/domain/fee/policy.rs`:

```rust
pub enum Policy {
    // Surplus'ın belirli bir yüzdesini al, max volume cap ile
    Surplus {
        factor: f64,             // örn: 0.5 = surplus'ın %50'si
        max_volume_factor: f64,  // örn: 0.01 = trade volume'un %1'i
    },

    // Quote'a göre fiyat iyileştirmesinin belirli bir yüzdesini al
    PriceImprovement {
        factor: f64,
        max_volume_factor: f64,
        quote: Quote,
    },

    // Trade hacminin sabit bir yüzdesi
    Volume {
        factor: f64,  // örn: 0.001 = %0.1
    },
}
```

**Surplus = kullanıcının limit fiyatının üzerinde aldığı değer.**

Örnek:
- Kullanıcı 100 USDC için en az 0.05 ETH almasını istiyor (limit price)
- Solver 0.06 ETH buluyor
- Surplus = 0.01 ETH
- Eğer Surplus policy factor=0.5 ise: fee = 0.005 ETH

Bu "CoW Improvement" modeli: kullanıcı her zaman kazanır, sadece ekstra kazancın bir kısmı fee olarak alınır.

---

## 8. Winner Selection — Uniform Directional Clearing Price

`crates/autopilot/src/domain/competition/winner_selection.rs`:

Bu CoW Protocol'ün en kritik bileşeni — MEV önlemenin kalbi.

### 8.1 Kavram: Uniform Clearing Price

Bir batch'te aynı yönde (örn: A→B) birden fazla sipariş varsa, hepsi **aynı clearing price** üzerinden execute edilir. Fiyat avantajı kullanıcıya surplus olarak yansır.

**Matematiksel özellik:**
- Tüm siparişler batch içinde tek bir fiyat üzerinden execute edilir
- Bu fiyat, mevcut market fiyatından daha iyi veya eşit olmalıdır
- Fiyat farkı (surplus) kullanıcılara dağıtılır

**MEV özelliği:** Tek clearing price olduğundan sandwich attack imkansızdır. Miner/validator "içeriden bilgi" ile order'ları reorder edemez; tüm batch aynı fiyattan execute edilir.

### 8.2 Scoring (CIP-38)

Her çözüm için score = surplus + protocol fees

```
score = Σ(trade_i in solution) [surplus_i + fees_i]
```

Her şey native token (ETH) cinsine çevrilir. En yüksek score kazanır.

### 8.3 Birden Fazla Kazanan

`max_winners_per_auction` > 1 olabilir. Farklı order kümelerini çözen solver'lar aynı anda kazanabilir (çakışmayan order set'leri).

---

## 9. Driver — Competition

### 9.1 Competition Struct

`crates/driver/src/domain/competition/mod.rs`:

```rust
pub struct Competition {
    pub solver: Solver,
    pub eth: Ethereum,
    pub liquidity: infra::liquidity::Fetcher,
    pub liquidity_sources_notifier: infra::notify::liquidity_sources::Notifier,
    pub simulator: Simulator,
    pub mempools: Mempools,
    pub settlements: Mutex<VecDeque<Settlement>>,  // son N auction'ın settlement'ları
    pub risk_detector: Arc<risk_detector::Detector>,
    fetcher: Arc<pre_processing::DataAggregator>,
    order_sorting_strategies: Vec<Arc<dyn sorting::SortingStrategy>>,
    submitter_pool: SubmitterPool,  // EIP-7702 paralel submission
}
```

### 9.2 SubmitterPool — EIP-7702 Paralel Submission

```rust
struct SubmitterPool {
    direct_slot: Arc<tokio::sync::Semaphore>,  // Solver EOA (doğrudan)
    delegated: Option<DelegatedSlots>,          // EIP-7702 hesapları
    admission: Arc<tokio::sync::Semaphore>,     // Max in-flight settle sayısı
    solver_address: eth::Address,
}
```

**EIP-7702:** Ethereum Pectra hard fork ile geldi. EOA'lar geçici olarak smart contract davranışı kazanabilir. Bu sayede aynı solver birden fazla settlement'ı paralel olarak submit edebilir.

- İlk olarak `direct_slot` kontrol edilir (solver EOA, gas tasarrufu)
- Direct slot meşgulse EIP-7702 delegated hesaplardan biri alınır
- `futures::future::select_ok` ile race edilir (hangisi önce hazırsa)
- RAII guard drop'ta hesabı pool'a geri iade eder

### 9.3 solve() — Ana Çözüm Akışı

```rust
pub async fn solve(&self, request: Request<Body>) -> Result<Vec<Solved>, Error>
```

Adımlar:
1. **Pre-processing:** `DataAggregator.start_or_get_tasks_for_auction()` — paralel veri toplama
2. **assemble_auction():** Order sıralama + balance kontrolü
3. **Likidite çek:** Solver'ın konfigürasyonuna göre `Fetch` veya `Skip`
4. **Solver'a gönder:** `self.solver.solve(auction, &liquidity)` — HTTP call
5. **Unique ID filtrele:** Tekrarlayan solution ID'leri at
6. **Empty solution filtrele:** İşlem içermeyen çözümleri at
7. **Solution merge:** Birleştirme izinse `merge()` çağır
8. **Encode:** Her solution'ı `solution.encode()` ile calldata'ya çevir
9. **Score:** `settlement.score()` ile her settlement'ı puanla
10. **Sort:** Puana göre azalan sırala
11. **Cache:** `self.settlements` VecDeque'ye ekle (MAX 5 auction × max_solutions)
12. **Resimüle:** Her yeni blokta yeniden simüle et, revert edenler çıkar
13. **Return:** `Vec<Solved>` — autopilot'a bildir

### 9.4 assemble_auction() — Auction Oluşturma

```rust
async fn assemble_auction(&self, tasks: &DataFetchingTasks) -> Auction
```

- **sort_orders():** CPU-yoğun → `spawn_blocking()`. Siparişleri öncelik sırasına koyar
- **update_orders():** Balance'a göre siparişleri filtrele ve scale et

**Balance allocation (update_orders):**
```rust
auction.orders.retain_mut(|order| {
    // CoW AMM, flashloan, wrapper order'lar doğrudan geçer

    let remaining_balance = balances.get_mut(&(trader, sell_token, sell_token_balance));

    // Partial fillable order: bakiyeye göre scale
    // Fill-or-kill order: ya tam bakiye var ya da at
    let allocated_balance = match order.partial {
        Partial::Yes { .. } => max_sell.min(*remaining_balance),
        Partial::No if max_sell <= *remaining_balance => max_sell,
        _ => SellAmount::default(),  // 0 → filtrele
    };

    // Partial order available amount'u scale et (fee'yi de hesaba kat)
    if let Partial::Yes { available } = &mut order.partial {
        *available = TargetAmount(mul_ratio(available.0, allocated_balance.0, max_sell.0));
    }

    remaining_balance.0 -= allocated_balance.0; // bakiyeyi düş
    true
});
```

**Önemli:** Bakiye yalnızca önce sıralanan order'lara tahsis edilir. Sıralama stratejisi kritik!

### 9.5 resimulate_until_revert() — Blok İzleme

```rust
async fn resimulate_until_revert(
    &self,
    scored: &mut Vec<(Solved, Settlement)>,
    auction: &Auction,
)
```

Her yeni blok geldiğinde tüm çözümler yeniden simüle edilir:
- Gas limit aşılıyorsa: `Revert(GasExceeded)` → çözümü çıkar
- Revert eden çözüm varsa: notify gönder, `scored` ve `settlements` cache'den sil
- Tüm çözümler revert ederse: döngüden çık

Bu mekanizma, mempool'daki state değişikliklerinden kaynaklanan revert'ları önler.

### 9.6 reveal() ve settle()

**reveal():** Autopilot kazananı seçince driver reveal çağrısı alır.
```rust
pub async fn reveal(&self, solution_id: u64, auction_id: auction::Id) -> Result<Revealed, Error>
```
- Cache'deki settlement'ı bul
- İki calldata versiyonu döndür:
  - `internalized_calldata`: İnternalize edilmiş (gas optimize, on-chain görünür)
  - `uninternalized_calldata`: Tam versiyon (off-chain doğrulama için)

**settle():** Autopilot'un onayından sonra gerçek submit.
```rust
pub async fn settle(
    self: &Arc<Self>,
    auction_id: auction::Id,
    solution_id: u64,
    submission_deadline: BlockNo,
) -> Result<Settled, Error>
```
- Admission kontrolü (max in-flight sınırı)
- Deadline kontrolü (halihazırda geçmişse reddet)
- `SubmitterPool::acquire()` — direct veya EIP-7702 slot
- `mempools.execute()` — mempool'a gönder
- Transaction hash döndür

---

## 10. Solution — Çözüm Modeli

### 10.1 Solution Struct

`crates/driver/src/domain/competition/solution/mod.rs`:

```rust
pub struct Solution {
    id: Id,
    trades: Vec<Trade>,               // Fulfillment | JIT
    prices: Prices,                   // HashMap<TokenAddress, U256> — clearing prices
    pre_interactions: Vec<domain::Interaction>,
    interactions: Vec<Interaction>,   // AMM swap calls
    post_interactions: Vec<domain::Interaction>,
    solver: Solver,
    weth: eth::WrappedNativeToken,
    gas: Option<eth::Gas>,
    gas_fee_override: Option<GasFeeOverride>,
    flashloans: HashMap<order::Uid, Flashloan>,
    wrappers: Vec<WrapperCall>,
}
```

**Trade türleri:**
- `Trade::Fulfillment` — Normal kullanıcı order'ı
- `Trade::Jit` — Just-In-Time likidite (CoW AMM gibi)

JIT order eğer `surplus_capturing_jit_order_owners` listesindeyse Fulfillment'a dönüştürülür (same scoring logic).

### 10.2 Solution ID Yapısı

```rust
pub struct Id {
    id: u64,                  // Global unique (atomic counter, restart'a kadar)
    merged_solutions: Vec<u64>, // Hangi base solution'ların merge'i
}
```

- `Id::new(solver_id)`: Solver'dan gelen tek solution
- `Id::new_merged(first, second)`: İki solution'ın merge'i

### 10.3 merge() — Solution Birleştirme

```rust
pub fn merge(
    &self,
    other: &Self,
    max_orders_per_merged_solution: usize,
) -> Result<Self, error::Merge>
```

Kontroller:
1. Aynı solver'dan gelmeli
2. Toplam trade sayısı `max_orders_per_merged_solution`'dan az olmalı
3. Aynı order iki kez çözülmemeli (`uids.is_disjoint(&other_uids)`)
4. Clearing price'lar uyumlu olmalı (unique scaling factor)

Uyumlu fiyat kontrolü:
```rust
fn scaling_factor(first: &Prices, second: &Prices) -> Option<BigRational> {
    // Ortak token'lar için price ratio'ları hesapla
    // Eğer tek unique ratio varsa → bu scaling factor
    // Birden fazla farklı ratio varsa → None (uyumsuz)
    // Hiç ortak token yoksa → 1.0
}
```

Merge yapılırken daha küçük olan scale edilir (precision kaybını önlemek için her zaman yukarı scale).

### 10.4 scoring() — Solution Puanlama

```rust
pub fn scoring(
    &self,
    native_prices: &auction::Prices,
    surplus_capturing_jit_order_owners: &HashSet<eth::Address>,
) -> Result<eth::Ether, error::Scoring>
```

Her trade için:
- Sell order: `executed_sell = executed + fee`
- Buy order: `executed_buy = executed`
- `scoring::compute_score(&trades, native_prices)` çağrılır

### 10.5 clearing_prices() — ETH/WETH Fiyat Normalizasyonu

```rust
pub fn clearing_prices(&self) -> Prices {
    // Eğer bir order ETH satın alıyorsa:
    // - ETH'nin clearing price'ı = WETH'nin clearing price'ı
    // - WETH ayrıca trade edilmiyorsa WETH fiyatı kaldırılır (gas tasarrufu)
    // ETH siparişleri WETH üzerinden çözülür, driver unwrap ekler
}
```

### 10.6 approvals() — Token Onayları

```rust
pub async fn approvals(
    &self,
    eth: &Ethereum,
    internalization: settlement::Internalization,
) -> Result<impl Iterator<Item = eth::allowance::Approval>, Error>
```

USDT gibi "reset-first" token'lar için:
1. `erc20.allowance(settlement_contract, spender)` → mevcut allowance
2. Eğer yeterliyse: skip
3. `erc20.approve(spender, amount)` dene (simulate)
4. Başarısızsa: önce `approve(spender, 0)` ekle, sonra gerçek approval

---

## 11. Settlement — On-Chain Encoding

### 11.1 Settlement Struct

`crates/driver/src/domain/competition/solution/settlement.rs`:

```rust
pub struct Settlement {
    pub auction_id: auction::Id,
    transaction: SettlementTx,
    pub gas: Gas,
    solution: Solution,
}

struct SettlementTx {
    internalized: eth::Tx,    // AMM interaction'lar internalize edilmiş (gas optimize)
    uninternalized: eth::Tx,  // Tam versiyon (doğrulama için)
    may_revert: bool,
}
```

**Internalization:** Settlement contract'ın internal buffer'ında yeterli token varsa AMM swap'ı atlayıp direkt buffer'dan kullan. Bu significant gas tasarrufu sağlar. Sadece "trusted" token'larla yapılabilir.

### 11.2 encode() — Çözümü Transaction'a Çevir

```rust
pub(super) async fn encode(
    solution: competition::Solution,
    auction: &competition::Auction,
    eth: &Ethereum,
    simulator: &Simulator,
    solver_native_token: ManageNativeToken,
) -> Result<Self, Error>
```

Adımlar:
1. **Internalization güvenlik kontrolü:** Internalize edilen interaction'lar sadece trusted token kullanabilir. Aksi hâlde solver slashing
2. **Token approvals:** Hem internalized hem uninternalized için paralel hesapla
3. **Transaction encode:** `encoding::tx()` ile her iki versiyon oluştur
4. **Access list hesapla:** ETH satın alan SC wallet'lar için `eth_createAccessList`
5. **Gas simülasyonu:** `simulator.gas(transaction.internalized)` → gas estimate
6. **Gas limit hesapla:** `Gas::new(estimate, block_limit, tx_gas_limit)`
7. **Solver balance kontrolü:** `eth.balance(solver)` ≥ `gas.required_balance(max_fee_per_gas)`

### 11.3 Gas Parametreleri

```rust
pub struct Gas {
    pub estimate: eth::Gas,  // Simülasyondan gelen gerçek tahmin
    pub limit: eth::Gas,     // estimate × 2.0 (buffer için)
}

impl Gas {
    pub fn new(estimate, block_limit, tx_gas_limit) -> Result<Self, Error> {
        // Block limit'in yarısını geçemez (block producer'ların txn kabul etmesi için)
        // Aynı zamanda configurable tx gas limit'i geçemez (EIP-7825 Fusaka için)
        let max_gas = min(block_limit / 2, tx_gas_limit);
        if estimate > max_gas { return Err(GasLimitExceeded) }

        const GAS_LIMIT_FACTOR: f64 = 2.0;
        let limit = min(max_gas, estimate × 2.0);
        Ok(Self { estimate, limit })
    }
}
```

**EIP-7825 (Fusaka):** Mainnet'te per-transaction gas cap = 2^24 - 1 = 16,777,215. Bu, tek transaction'ın blok'un tamamını kullanmasını önler.

---

## 12. Scoring — CIP-38 Puanlama Algoritması

`crates/driver/src/domain/competition/solution/scoring.rs`:

### 12.1 compute_score()

```rust
pub fn compute_score(trades: &[Trade], native_prices: &auction::Prices) -> Result<eth::Ether, Error>
    trades.iter().map(|trade| trade.score(native_prices)).sum()
```

Her trade bağımsız puanlanır, toplamı solution score'u.

### 12.2 Trade::score() — CIP-38 (Güncellendi: CIP-Draft)

```rust
fn score(&self, native_prices: &auction::Prices) -> Result<eth::Ether, Error> {
    let surplus_in_surplus_token = self.user_surplus()?.0 + self.fees()?.0;

    let score = match self.side {
        // Sell order: surplus zaten buy token cinsinden → direkt ETH'ye çevir
        Side::Sell => native_price_buy.in_eth(surplus_in_surplus_token),

        // Buy order: surplus sell token cinsinden → önce buy token'a çevir, sonra ETH'ye
        // buy_amount = surplus × (buy_price / sell_price)
        // to avoid overflow: (surplus × buy_amount) / sell_amount
        Side::Buy => {
            let surplus_in_buy_tokens = (surplus_in_surplus_token * buy_amount) / sell_amount;
            native_price_buy.in_eth(surplus_in_buy_tokens)
        }
    };
    Ok(score)
}
```

**Neden buy order farklı?** Tarihsel bug: Eski formül bazı buy order'lar için unreasonably high score veriyordu (test'te görülen `score = 911` bunun sınırlanmış hâli). Yeni formül limit price oranını kullanarak buy token cinsine çevirir.

### 12.3 surplus_over() — Limit Fiyat Üzerindeki Değer

```rust
fn surplus_over(&self, price_limits: PriceLimits) -> Result<eth::SurplusTokenAmount, Math> {
    match self.side {
        Side::Buy => {
            // Limit sell = executed_buy × (limit_sell / limit_buy) — scale for partial
            // Actual sold = executed_buy × (custom_buy / custom_sell)
            // Surplus = limit_sell - actual_sold (sell token cinsinden surplus)
            let limit_sell = price_limits.sell × executed / price_limits.buy;
            let sold = executed × custom_price.buy / custom_price.sell;
            limit_sell - sold
        }
        Side::Sell => {
            // Limit buy = executed_sell × ceil(limit_buy / limit_sell)
            // Actual bought = executed_sell × custom_price.sell / custom_price.buy
            // Surplus = actual_bought - limit_buy (buy token cinsinden surplus)
            let limit_buy = ceil(executed × price_limits.buy / price_limits.sell);
            let bought = ceil(executed × custom_price.sell / custom_price.buy);
            bought - limit_buy
        }
    }
}
```

**Not:** `checked_ceil_div` — Settlement contract'ın buy amount hesabıyla tutarlılık için tavan bölme kullanılır.

### 12.4 Protocol Fee Hesabı

```rust
fn protocol_fee(&self, fee_policy: &FeePolicy) -> Result<eth::SurplusTokenAmount, Error> {
    match fee_policy {
        FeePolicy::Surplus { factor, max_volume_factor } => {
            let surplus = self.user_surplus()?;
            min(self.fee(surplus, *factor)?, self.volume_fee(*max_volume_factor)?)
        }
        FeePolicy::PriceImprovement { factor, max_volume_factor, quote } => {
            let price_improvement = self.price_improvement(quote)?;
            min(self.fee(price_improvement, *factor)?, self.volume_fee(*max_volume_factor)?)
        }
        FeePolicy::Volume { factor } => self.volume_fee(*factor)?
    }
}
```

### 12.5 Fee Dönüşüm Formülü

Scoring modülü post-fee trade'leri alır (fee zaten uygulanmış). Bu nedenle farklı factor kullanılır:

```
fee_before = surplus_before × factor
fee_after = surplus_after × factor'

surplus_after = surplus_before - fee_before = surplus_before × (1 - factor)

factor' = fee_before / surplus_after = (surplus_before × factor) / (surplus_before × (1-factor))
        = factor / (1 - factor)
```

Benzer mantık volume fee için:
- Sell order: `factor' = factor / (1 - factor)` (buy amount azalır)
- Buy order: `factor' = factor / (1 + factor)` (sell amount artar)

Bu matematiksel dönüşüm, feeler önceden uygulandığında doğru fee recovery sağlar.

---

## 13. Baseline Solver

### 13.1 Yapı ve Config

`crates/solvers/src/domain/solver/baseline.rs`:

```rust
pub struct Config {
    pub weth: eth::WethAddress,
    pub base_tokens: Vec<eth::TokenAddress>,  // ara hop token'ları (WETH, USDC, vb.)
    pub max_hops: usize,                       // A→B→C = 1 hop
    pub max_partial_attempts: usize,           // kısmi fill için kaç deneme
    pub solution_gas_offset: eth::SignedGas,   // routing gas offset
    pub native_token_price_estimation_amount: eth::U256,
    pub uni_v3_node_url: Option<Url>,         // Uniswap V3 desteği
}
```

### 13.2 solve() — Path Finding

```rust
pub async fn solve(&self, auction: auction::Auction) -> Vec<solution::Solution>
```

CPU-yoğun işlem `tokio::spawn()` ile background'a alınır, deadline timeout'ına tabi.

Her order için:
1. Sell token'ın native fiyatını al (ya cache'den ya routing ile tahmin)
2. `requests_for_order()` — order için routing request'leri üret:
   - Fill-or-kill: 1 request (tam miktar)
   - Partially fillable: `max_partial_attempts` request (1/1, 1/2, 1/4, 1/8...)
3. Her request için `boundary_solver.route(request, max_hops)` — graph traversal
4. İlk başarılı route'u çözüm olarak gönder, sonrakini dene

**Path finding:** `boundary::baseline::Solver` — on-chain likidite pool'ları üzerinde Dijkstra-benzeri traversal.
- Uniswap V2/V3, Balancer, Swapr ve diğer AMM'ler desteklenir
- `max_hops` ile yol uzunluğu sınırlandırılır
- `base_tokens` ile ara hop'lar kısıtlanır (WETH, USDC, DAI)

### 13.3 Native Price Tahmini

Sell token'ın native fiyatı bilinmiyorsa:
1. Küçük bir satış simülasyonu: `sell = 2^144` birim sell_token → buy = `native_token_price_estimation_amount` birim WETH
2. Route bulunursa: `price = native_estimation_amount / route.input.amount`
3. Route bulunamazsa: `price = U256::MAX` (fee = 0 etkisi)

**Neden 2^144?** Uniswap V2 rezervleri 112-bit integer. 256 - 112 = 144. Bu, pool overflow olmadan maksimum işlem.

### 13.4 Gas Hesabı

Her route için gas estimate = `sum(segment.gas)` + `solution_gas_offset`

Fee (native token) = `sell_token_price × gas_units × gas_price`

Eğer hesaplama overflow olursa çözüm atlanır.

---

## 14. Likidite Tipleri

`crates/driver/src/domain/liquidity/`:

```
ConstantProduct  — Uniswap V2 tipi (x × y = k)
WeightedProduct  — Balancer V2 (ağırlıklı CPMM)
Stable           — Balancer stable pool (Curve-tipi)
ConcentratedLiquidty — Uniswap V3 (tick-bazlı)
LimitOrder       — On-chain limit order (GPv2 gibi)
Swapr            — Swapr (Gnosis Chain)
ZeroEx           — 0x agregasyon
```

Her likidite tipi için driver boundary katmanında adaptör var. Bu soyutlama yeni likidite kaynaklarının kolayca eklenmesini sağlar.

---

## 15. Risk Detector

`crates/driver/src/domain/competition/risk_detector/`:

### 15.1 Bad Token Detector

```rust
pub struct BadTokenDetector {
    cache: Arc<RwLock<HashMap<TokenAddress, TokenQuality>>>,
    simulation: SimulationBasedDetector,
}

pub enum TokenQuality {
    Good,
    Bad { reason: String },
}
```

Token kalitesi şu şekilde test edilir:
1. Küçük bir miktar transfer simüle edilir
2. Revert varsa: Bad token
3. Transfer miktarı beklenenden azsa: Fee-on-transfer (Bad)
4. Cache'de tutulur

### 15.2 Bad Order Detector

```rust
pub struct BadOrderDetector {
    metrics: Arc<Metrics>,
    threshold: f64,  // fail rate eşiği
}
```

Sık fail eden order'lar işaretlenir ve auction'dan çıkarılır. Metrics:
- `encoding_failed` / `encoding_succeeded` sayacı
- Fail rate eşiği aşılırsa order blacklist'e alınır

---

## 16. Mempool Stratejisi

`crates/driver/src/domain/mempools.rs` ve `infra/mempool/`:

Çoklu mempool stratejisi:
- **Public mempool** (default Ethereum txpool)
- **Flashbots Protect** (MEV korumalı)
- **MEV Blocker** (Gnosis Chain)
- **Eden Network**

Driver, settlement transaction'ı birden fazla mempool'a paralel gönderebilir. Gas price bump mekanizması da desteklenir (replacement transaction).

**Submission Mode:**
```rust
pub enum SubmissionMode {
    Direct(eth::Address),            // Solver EOA doğrudan gönderir
    Delegated {
        submitter_eoa: eth::Address, // EIP-7702 hesabı
        solver_eoa: eth::Address,    // Gerçek solver
    },
}
```

---

## 17. App Data — Order Metadata

`crates/app-data/`:

Order'a 8KB'a kadar JSON metadata eklenebilir (off-chain):

```json
{
  "version": "1.1.0",
  "metadata": {
    "replacedOrder": { "uid": "0x..." },
    "flashloan": { "token": "0x...", "amount": "..." },
    "hooks": {
      "pre": [{ "target": "0x...", "callData": "...", "gasLimit": "..." }],
      "post": [{ "target": "0x...", "callData": "...", "gasLimit": "..." }]
    }
  }
}
```

- `replacedOrder`: Order replacement zinciri
- `flashloan`: Flash loan kaynağı
- `hooks`: Pre/post execution hooks (arbitrary contract calls)
- `wrappers`: ETH wrap/unwrap yönetimi

App data hash'i (32 byte keccak256) on-chain tutulur, tam veri off-chain (IPFS veya orderbook DB'de).

---

## 18. Solvable Orders Cache

`crates/autopilot/src/solvable_orders.rs`:

Orderbook ile Autopilot arasındaki köprü. Her blokta güncellenir:

1. DB'den aktif order'ları al
2. Her order için:
   - `valid_to` kontrolü
   - Balance/allowance cache'i güncelle
   - Fee policy uygula
3. Auction'a hazır order listesi oluştur

Cache iki amaç için: hız (DB round-trip azaltmak) ve tutarlılık (aynı blok snapshot'ı).

---

## 19. Nova Shield Chain için Çıkarımlar

### 19.1 Sealed-Bid Batch Auction Mekanizması

CoW Protocol'ün batch auction modeli Nova Shield'in encrypted mempool + batch execution ihtiyacını doğrudan karşılıyor. Öğrenilecekler:

**Order model:**
- 56-byte OrderUid (hash + owner + valid_to) — çok zarif, ayrı index gereksiz
- `fee_amount = 0` zorunluluğu — fee off-chain clearing price'a dahil edilir
- Bu, fee transparency'i sağlar ve MEV attack surface'ı azaltır

**Batch oluşturma:**
- `SolvableOrdersCache.update()` pattern → her slot'ta cache temizle
- Partial fillable order'lar için balance scaling mantığı alınabilir
- `valid_to` timestamp kontrolü + `OrderStatus` state machine doğrudan uygulanabilir

**Clearing price mekanizması:**
- Uniform directional clearing price = tüm siparişler aynı fiyattan execute
- Bu "MEV = 0" özelliği matematiksel olarak kanıtlanmış
- Nova Shield'e uygulanacak şekil: encrypted slot kapanınca tüm order'lar açılır, batch içinde en iyi uniform price bulunur

### 19.2 Scoring Sistemi (Solver Teşvik)

CIP-38 scoring = `Σ(surplus_i + fees_i)` native token cinsinden

Nova Shield'de bu mekanizma validator/solver teşviki için kullanılabilir:
- Her batch çözücü `score` hesaplar
- En yüksek score'u getiren çözüm seçilir
- Bu, rekabetçi fiyat bulma garantisi verir

Fee politikaları (Surplus, PriceImprovement, Volume) NSC native fee hesaplamasında referans alınabilir.

### 19.3 Çözüm Merge Mekanizması

`Solution::merge()` kritik: iki ayrı çözümü tek settlement'a birleştirir.
- Clearing price uyumluluğu kontrolü (scaling factor)
- Duplicate trade kontrolü
- Gas toplama

Nova Shield batch processor'ı bu mantıkla oluşturulabilir: birden fazla sub-solution → tek settlement.

### 19.4 Gas Yönetimi

- `Gas::new()`: estimate × 2.0 buffer, max = block_limit/2
- EIP-7825 (tx gas cap) uyumluluğu
- Submission fee: `gas_limit × max_fee_per_gas` → solver minimum balance

Nova Shield'in NSC fee hesaplaması için `Gas::required_balance()` pattern'i alınabilir.

### 19.5 EIP-7702 Paralel Submission

`SubmitterPool` pattern: 1 direct slot + N delegated slots. Her slot ayrı bir EOA/account.

Nova Shield'de:
- Validator'lar birden fazla parallel settlement gönderebilir
- `SubmitterPool` pattern ▷ NS validator submission pool

### 19.6 Risk Detector → Bad Token / Bad Transaction Filter

`BadTokenDetector` ve `BadOrderDetector`:
- Simülasyon tabanlı token kalite kontrolü (transfer simülasyonu)
- Order fail rate tracking (encoding_failed / encoding_succeeded)
- Eşik aşılınca blacklist

Nova Shield'de bu, spam filter ve anomaly detection için kullanılabilir.

### 19.7 Order Replacement

CoW Protocol'ün `replace_order()` mekanizması NovaShield'de önemli:
- Eski order zaten bid üzerindeyse replacement yok (double-spend koruması)
- ECDSA imzalı olması zorunlu (PreSign ile başkası iptal edemez)
- Owner aynı olmalı

Bu pattern, NS emergency cancel / order update mekanizması için referans.

### 19.8 App Data / Hooks Mekanizması

Pre/post hooks (`metadata.hooks`) kullanıcıların settlement öncesi/sonrası arbitrary contract call yapmasına izin verir. Bu NovaShield'deki:
- **Native miras protokolü:** Post-execution hook ile heir'a transfer
- **Emergency freeze:** Pre-execution hook ile freeze kontrol
- **MPC guardian approval:** Pre-execution hook ile threshold imza doğrulama

olarak uyarlanabilir.

### 19.9 Internalization — Internal Buffer Optimizasyonu

Settlement contract dahilinde "buffered" token'lar için AMM swap atlama özelliği. Nova Shield'de:
- Validator staking pool buffer
- Batch settlement gas optimizasyonu için kullanılabilir

### 19.10 Baseline Solver — Graph Traversal Referansı

`max_hops` ile sınırlandırılmış path finding:
- `base_tokens` = hub token'lar (WETH, USDC, DAI)
- Her order için binary search (1/1, 1/2, 1/4...) partial fill denemesi
- CPU-yoğun işlemler `spawn_blocking` ile async runtime'dan izole edilir

Nova Shield batch matcher için: Token routing için `max_hops` ile Dijkstra, partial fill için binary search mantığı alınabilir.

---

## 20. Kritik Güvenlik Özellikleri

### 20.1 Slashing Mekanizması

Driver Settlement encoding kuralları:
- İnternalize edilen interaction'lar sadece trusted token kullanabilir → aksi hâlde slashing
- Çözüm simülasyon gerektirmeden submit edilemez → aksi hâlde slashing
- Gas estimate aşılırsa: Revert → slashing

Bu kurallar protokolün güvenliğini garanti eder. Solver'lar kurala uymak zorunda.

### 20.2 Circuit Breaker

Monitör on-chain settlement'ların off-chain auction outcome'larıyla uyuşup uyuşmadığını izler. Uyuşmazlık varsa solver "jailed" edilir.

### 20.3 PreSign Security

PreSign order'lar off-chain iptal edilemez (sadece on-chain). Bu özellik, MPC sisteminde kritik: zaten blockchain'e işlenmiş bir izin, off-chain saldırı ile iptal edilemez.

### 20.4 Quote Verification

Order submission'da quote doğrulaması: Quote'un geçerliliği ve price accuracy kontrolü. Kullanıcının gerçek market fiyatından çok uzakta order girmesini önler (`QuoteNotVerified` hatası).

---

## 21. Özet ve Değerlendirme

### CoW Protocol'ün Başarılı Yaptığı

1. **MEV Koruması:** Uniform clearing price matematiksel olarak sandwich attack imkansız kılar
2. **Surplus Paylaşımı:** Kullanıcılar her zaman limit fiyatlarından daha iyi fiyat alır
3. **Çok Solver:** Rekabetçi solver ekosistemi, en iyi fiyat için incentive
4. **Gas Optimizasyonu:** Internalization, batch execution, access list
5. **Esneklik:** Hook sistemi, partial fill, flashloan, wrapper

### Nova Shield için Kullanılacak Elemanlar

| CoW Bileşen | Nova Shield Karşılığı |
|-------------|----------------------|
| 56-byte OrderUid | Tx kimlik formatı (hash+sender+valid_to) |
| fee_amount=0 | Clearing price içinde implicit fee |
| Batch auction | Encrypted mempool + slot-based batch |
| Uniform clearing price | MEV'siz fiyat eşitleme |
| CIP-38 scoring | Solver/batch builder teşvik mekanizması |
| Solution merge | Multi-sub-solution batch combiner |
| Gas::new() | NSC fee hesabı |
| SubmitterPool (EIP-7702) | Validator parallel submission pool |
| Pre/Post hooks | Native miras + emergency freeze |
| BadTokenDetector | Spam/anomaly filter |
| replace_order() | Order güncelleme protokolü |
| Circuit Breaker | Validator slashing + jailing |
| Internalization | NS settlement buffer optimizasyonu |
| Leader Lock | NS validator leader election |

### Rust Implementasyon Notları

- **Error handling:** `thiserror` + `anyhow` + `#[from]` kombinasyonu çok temiz
- **Async pattern:** CPU-yoğun işlemler `spawn_blocking`, async I/O normal tokio
- **Type safety:** Newtypes her yerde (TokenAddress, Ether, Gas, U256 ayrı tipler)
- **RAII:** SubmitterPool guard drop'ta slot iade eder
- **Metrics:** `prometheus_metric_storage` macro ile otomatik metric registration
- **Tracing:** `#[instrument]` + structured logging her kritik fonksiyonda

Bu pattern'lerin tamamı Nova Shield Rust implementasyonunda doğrudan uygulanabilir.
