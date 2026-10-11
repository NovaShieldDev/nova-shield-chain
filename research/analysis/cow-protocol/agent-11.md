# CoW Protocol — Agent 11: driver/src/ — Kalan Modüller
## Kapsam: solution/scoring, solution/fee, solution/slippage, solution/settlement, domain/mempools, domain/sorting, domain/liquidity, infra/
## DEX Özel Protokolü: 3x Hassasiyet
## Tarih: 2026-07-22

---

## 1. domain/competition/solution/scoring.rs — Skor Hesaplama Motoru (KRİTİK)

### Genel Felsefe

CIP-38 + Draft CIP tabanlı: `score = (surplus + protocol_fees)` önce buy token'a, sonra native token'a dönüştürülür.

Bir önceki yaklaşımdan fark: Sell order surplus zaten buy token'da; buy order surplus sell token'da — ikisi farklı dönüşüm gerektirir.

### Trade Struct (Scoring için)

```rust
pub struct Trade {
    signed_sell: eth::Asset,   // limit price sell token/amount
    signed_buy: eth::Asset,    // limit price buy token/amount
    side: Side,                // Sell | Buy
    executed: order::TargetAmount,  // gerçekleşen miktar
    custom_price: CustomClearingPrices,  // fee dahil clearing fiyatı
    policies: Vec<FeePolicy>,   // protokol fee politikaları
}
```

### score() — Ana Skor Hesabı

```rust
fn score(&self, native_prices: &auction::Prices) -> Result<eth::Ether, Error> {
    let native_price_buy = native_prices.get(&self.signed_buy.token)?;

    let surplus_in_surplus_token = self.user_surplus()?.0 + self.fees()?.0;

    match self.side {
        Side::Sell => {
            // Sell order surplus zaten buy token'da
            native_price_buy.in_eth(eth::TokenAmount(surplus_in_surplus_token))
        }
        Side::Buy => {
            // Buy order surplus sell token'da → buy token'a çevir
            // buy_amount = surplus * buy_price / sell_price
            let surplus_in_buy_tokens = surplus_in_surplus_token
                .widening_mul(self.signed_buy.amount.0)  // U512 genişletme
                .checked_div(U512::from(self.signed_sell.amount.0))?;
            let surplus_in_buy_tokens = eth::U256::uint_try_from(surplus_in_buy_tokens)?;
            native_price_buy.in_eth(surplus_in_buy_tokens.into())
        }
    }
}
```

**Neden widening_mul (U512)?**
Buy order surplus dönüşümü: `surplus * buy_price / sell_price`. Limit price değerleri büyük sayılar olabilir — U256 * U256 overflow verebilir. U512 genişletme ile güvenli hesaplama.

**Tarihsel bağlam:** Test case'in yorumunda gerçek bir Base chain auction'ı referans alınmış. Eski mekanizma bazı buy order'lar için unreasonably yüksek skor veriyordu; yeni formül bunu düzeltiyor.

### surplus_over() — Limit Üzeri Fazlalık

```rust
fn surplus_over(&self, price_limits: PriceLimits) -> Result<eth::SurplusTokenAmount, Math> {
    match self.side {
        Side::Buy => {
            // Sell token'da surplus
            let limit_sell = price_limits.sell.0 * self.executed / price_limits.buy.0;
            let sold = self.executed.0 * self.custom_price.buy / self.custom_price.sell;
            limit_sell.checked_sub(sold)  // limit'ten az sattıysa surplus
        }
        Side::Sell => {
            // Buy token'da surplus
            let limit_buy = (self.executed.0 * price_limits.buy.0).checked_ceil_div(&price_limits.sell.0)?;
            let bought = (self.executed.0 * self.custom_price.sell).checked_ceil_div(&self.custom_price.buy)?;
            bought.checked_sub(limit_buy)  // limit'ten fazla aldıysa surplus
        }
    }
}
```

**Dikkat:** `checked_ceil_div` kullanımı — settlement contract ile tutarlılık. Settlement contract da ceil div kullanır, dolayısıyla skor hesabı da aynı yuvarlama yöntemini kullanmalı.

### fees() — Çok Katmanlı Fee Hesabı

```rust
fn fees(&self) -> Result<eth::SurplusTokenAmount, Error> {
    let mut current_trade = self.clone();
    let mut total = eth::SurplusTokenAmount::default();
    for protocol_fee in self.policies.iter().rev() {
        total += current_trade.protocol_fee(protocol_fee)?;
        current_trade.custom_price = self.calculate_custom_prices(total)?;  // fiyatları güncelle
    }
    Ok(total)
}
```

Fee politikaları ters sırada işlenir. Her fee hesabından sonra custom_price güncellenerek bir sonraki fee hesabının doğru temel üzerinden yapılması sağlanır.

### protocol_fee() — Üç Fee Politikası

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

### fee() — Surplus'tan Fee Hesabı

Fee post-fee surplus üzerinden hesaplanır. Matematiksel türetme:

```
fee_before = surplus_before * factor
surplus_after = surplus_before - fee_before = fee_before/factor - fee_before
factor' = fee_before / surplus_after = factor / (1 - factor)

Sonuç: fee = surplus_after * factor / (1 - factor)
```

```rust
fn fee(&self, surplus: eth::SurplusTokenAmount, factor: f64) -> Result<eth::SurplusTokenAmount, Error> {
    surplus.0.checked_mul_f64(factor / (1.0 - factor))
}
```

### volume_fee() — Hacimden Fee Hesabı

Sell order için buy amount'tan, Buy order için sell amount'tan fee:

```
Sell: fee = buy_amount' * factor / (1 - factor)
Buy:  fee = sell_amount' * factor / (1 + factor)
```

Fark: Sell order'da fee buy amount'u düşürür (bölücüde eksi), Buy order'da fee sell amount'u artırır (bölücüde artı).

---

## 2. domain/competition/solution/fee.rs — Fee Uygulama Motoru

### with_protocol_fees() — Fee Zinciri

```rust
impl Fulfillment {
    pub fn with_protocol_fees(&self, prices: ClearingPrices) -> Result<Self, Error> {
        let mut current = self.clone();
        for protocol_fee in &self.order().protocol_fees {
            current = current.with_protocol_fee(prices, protocol_fee)?;
        }
        Ok(current)
    }
```

### with_protocol_fee() — Tek Fee Uygulaması

```rust
fn with_protocol_fee(&self, prices: ClearingPrices, protocol_fee: &FeePolicy) -> Result<Self, Error> {
    let protocol_fee = self.protocol_fee_in_sell_token(prices, protocol_fee)?;

    // Fee'yi artır
    let fee = Fee::Dynamic((surplus_fee + protocol_fee.0).into());

    // Executed amount'u azalt (sell order için)
    let executed = match order.side {
        Side::Buy => self.executed(),  // Buy: executed değişmez
        Side::Sell => order::TargetAmount(self.executed().0 - protocol_fee.0),  // Sell: azal
    };

    Fulfillment::new(order, executed, fee, self.haircut_fee())
}
```

**Neden Buy order executed değişmiyor?**
Buy order'da hedef buy amount sabit, fee sell token'dan alınır. Sell order'da ise hedef sell amount'tan fee çıkarılır.

### adjust_quote_to_order_limits() — Quote/Order Fiyat Karşılaştırması (KRİTİK)

Solver'ın verdiği quote ile kullanıcının emri doğrudan karşılaştırılamayabilir çünkü:
1. Quote farklı miktarlar içerebilir
2. Quote gas fee içerir (order içermez)

**Sell Order için:**
```rust
// Fee'yi quote buy amount'dan çıkar
let quote_buy_amount = quote.buy_amount - (quote.fee_amount * quote.buy_amount / quote.sell_amount);
// Order sell amount'a ölçekle
let scaled_buy_amount = quote_buy_amount * order.sell_amount / quote.sell_amount;
// Order limit ile karşılaştır (better of two)
let buy_amount = max(order.buy_amount, scaled_buy_amount);
```

**Buy Order için:**
```rust
// Fee'yi quote sell amount'a ekle
let quote_sell_amount = quote.sell_amount + quote.fee_amount;
// Order buy amount'a ölçekle
let scaled_sell_amount = quote_sell_amount * order.buy_amount / quote.buy_amount;
// Order limit ile karşılaştır (worse of two, sell order perspektifinden)
let sell_amount = min(order.sell_amount, scaled_sell_amount);
```

**Test örnekleri:**
- Out-of-market sell: `buy_amount = max(order.buy_amount=19, scaled=17) = 19`
- In-market sell: `buy_amount = max(order.buy_amount=10, scaled=20) = 20`
- In-market buy: `sell_amount = min(order.sell_amount=20, scaled=18) = 18`

---

## 3. domain/competition/solution/slippage.rs — Slippage Yönetimi

### Parameters Struct

```rust
pub struct Parameters {
    pub relative: BigRational,      // Relatif slippage (ör. 0.5%)
    pub max: Option<eth::U256>,     // Maksimum native token cinsinden
    pub min: Option<eth::U256>,     // Minimum native token cinsinden
    pub prices: Prices,             // Token fiyatları
}
```

### apply_to() — Slippage Uygulama Akışı

Üç durum sırasıyla:

1. **Input token fiyatı varsa:**
   - Input amount → native token değerine çevir
   - Relatif slippage uygula
   - Min/max clamp
   - Tekrar input token'a çevir

2. **Output token fiyatı varsa:**
   - Output amount → native token → slippage hesapla
   - Input/output oranı ile input token slippage'ına çevir

3. **Hiç fiyat yoksa:**
   - Min/max cap uygulanmadan salt relatif slippage

**Mantık:** MEV koruması için slippage kısıtlanır. Çok büyük slippage limit front-running fırsatı yaratır.

```rust
let slippage = relative.clamp(
    self.min.unwrap_or_default(),
    self.max.unwrap_or(eth::U256::MAX),
);
```

---

## 4. domain/competition/solution/settlement.rs — Settlement Objesi

### Settlement Struct

```rust
pub struct Settlement {
    pub auction_id: auction::Id,
    transaction: SettlementTx,   // Gönderilecek tx
    pub gas: Gas,
    solution: Solution,
}

struct SettlementTx {
    internalized: eth::Tx,     // Internalize edilmiş tx (gas optimizasyonu)
    uninternalized: eth::Tx,   // Tüm interaction'ları içeren tx
    may_revert: bool,          // Revert riski var mı?
}
```

**Internalization:** Güvenilir token'lar için bazı interaction'lar settlement vault'a bırakılmadan halledilebilir — gas tasarrufu. `may_revert=true` ise pool yalnızca internalized tx gönderir.

### encode() — Solution → Settlement Tx

`#[instrument]` attribute ile tracing. Paralel operasyonlar için `try_join_all` kullanılır.

### Access List Optimizasyonu

`set_access_list()`: EIP-2930 access list ile state access gas maliyetleri önceden ödenir, warm access daha ucuz olur.

---

## 5. domain/mempools.rs — Çok Mempool Yönetimi

### SubmissionMode

```rust
pub enum SubmissionMode {
    Direct(eth::Address),         // EOA doğrudan settlement contract'a imzalar
    Delegated {
        submitter_eoa: eth::Address,  // Tx'yi imzalar ve nonce kullanır
        solver_eoa: eth::Address,     // EIP-7702 delegate target
    },
}
```

**EIP-7702 desteği:** Submitter EOA gas öder, solver EOA'sı EIP-7702 ile Solver7702Delegate'e delege eder. Bu MEV koruması ve fee management için kullanılır.

### execute() — Paralel Mempool Yarışması

```rust
pub async fn execute(&self, settlement, submission_deadline, mode) -> Result<eth::TxId, Error> {
    let res = select_ok(
        self.mempools.iter().map(|(mempool, stat)| async {
            self.submit(mempool, settlement, deadline, mode).await
        })
    ).await;
    // İlk başarılı olan kazanır, diğerleri "Superseded" olarak işaretlenir
}
```

`select_ok`: Futures'ı paralel çalıştırır, ilk başarılı olana kadar bekler. Diğer pending future'lar drop edilir.

### Gas Price Bump

```rust
const GAS_PRICE_BUMP_PCT: u64 = 13;  // %12.5 gerekli, %13 kullanılıyor (yuvarlama)
const CANCELLATION_GAS_AMOUNT: u64 = 21000;  // ETH transfer için min gas
```

Aynı nonce'da bekleyen tx'i geçersiz kılmak için gas fiyatı en az %12.5 artırılmalı. Düşük gas fiyatlı zincirler için hafif daha fazla artırım.

---

## 6. domain/competition/sorting.rs — Order Sıralama Stratejileri

### SortingStrategy Trait

```rust
pub trait SortingStrategy: Send + Sync + Debug {
    fn key(&self, order: &order::Order, tokens: &Tokens, solver: &eth::Address, now: DateTime<Utc>) -> SortingKey;
}
```

### ExternalPrice Stratejisi

```rust
let chance_to_settle = match (buy_price, sell_price) {
    (Some(bp), Some(sp)) => {
        let buy = f64::from(bp.in_eth(order.buy.amount).0);
        let sell = f64::from(sp.in_eth(order.sell.amount).0);
        if buy.is_subnormal() { 0. } else { sell / buy }
    }
    _ => 0.
};
```

**Oran:** `sell_value / buy_value`. Bu oran > 1 ise order kârlı, çözülebilir. Büyük oran = yüksek öncelik.

**OrdFloat wrapper:** `f64::total_cmp()` kullanımı — NaN handling dahil deterministic sıralama. Daha önce `BigRational` kullanılıyordu, `f64::total_cmp()` çok daha hızlı.

### CreationTimestamp Stratejisi

```rust
SortingKey::Timestamp(match max_order_age {
    Some(max_age) => {
        let earliest = (now - max_age).timestamp() as u32;
        (order.created.0 >= earliest).then_some(order.created)
    }
    None => Some(order.created),
})
```

`None` → order yok sayılır (Option<Timestamp> küçük = sıralamada düşük).

### OwnQuotes Stratejisi

```rust
let is_own_quote = order.quote.as_ref().is_some_and(|q| &q.solver == solver);
SortingKey::Bool(!is_order_outdated && is_own_quote)
```

Kendi quote'u verdiği order'ları önce çözer — rekabetçi avantaj.

### sort_orders()

```rust
orders.sort_by_cached_key(|order| {
    std::cmp::Reverse(
        comparators.iter()
            .map(|cmp| cmp.key(order, tokens, solver, now))
            .collect::<Vec<_>>()
    )
});
```

`sort_by_cached_key` → her order için key bir kez hesaplanır. Reverse → en yüksek key öne gelir. Birden fazla comparator varsa: lexicographic sıralama (ilk comparator birincil, ikinci comparator ikincil, vs.).

---

## 7. domain/liquidity/ — Driver Domain Liquidity Tipleri

### UniV2 Domain Tipi

```rust
pub struct UniswapV2(pub liquidity_sources::uniswap_v2::pool_fetching::Pool);
```

`Pool` struct'ını wrapper ile sarmallar. Boundary katman bu domain type'ı kullanır.

### Balancer V2 Domain Tipleri

- `WeightedPool`: `(tokens: Vec<(Address, Bfp)>, balances: Vec<Bfp>)`
- `StablePool`: `(tokens: Vec<Address>, balances: Vec<Bfp>, amplification: U256, scaling_factors: Vec<Bfp>)`

Her tip kendi AMM matematigini wrap eder.

### ZeroEx Domain Tipi

```rust
pub struct ZeroEx {
    pub sell_token: Address,
    pub buy_token: Address,
    pub sell_amount: U256,
    pub buy_amount: U256,
    pub data: Bytes,      // on-chain execution calldata
}
```

Off-chain API'dan alınan liquidiyti verisi. `data` alanı settlement sırasında direkt kullanılır.

---

## 8. boundary/liquidity/ — Boundary Katman (External → Domain Dönüşüm)

### UniV2 Boundary

```rust
pub fn to_domain(pool: liquidity_sources::Pool) -> domain::liquidity::uniswap::v2::UniswapV2 {
    domain::liquidity::uniswap::v2::UniswapV2(pool)
}
```

### Balancer V2 Boundary

Balancer pool data'yı iki katmandan geçirir:
1. Raw pool state (balances, amplification, tokens)
2. Scaling: token decimal normalizasyonu (`scaling_factors`)

```rust
pub fn to_domain(pool: BalancerWeightedPool) -> domain::liquidity::balancer::v2::weighted::Pool {
    domain::liquidity::balancer::v2::weighted::Pool {
        tokens: pool.tokens.iter().map(|(addr, weight)| (*addr, *weight)).collect(),
        balances: pool.balances.clone(),
    }
}
```

---

## 9. infra/api/routes/ — HTTP API Endpoint'leri

### Solve Endpoint (POST /solve)

Solver'dan çözüm alır → competition domain'ine iletir → settlement hazırlar → score döner.

**DTO:** `solve_request.rs` — `SolveRequest` JSON struct'ı (auction ID, orders, tokens, prices, deadline).

### Reveal Endpoint (POST /reveal)

Solver'ın çözümünü (solution) açıklar. CoW Protocol'ün commit-reveal mekanizması için kullanılır.

**DTO:** `reveal_request.rs` / `reveal_response.rs`.

### Settle Endpoint (POST /settle)

Winner seçildikten sonra settlement çağrısı. Settlement tx'i zincire gönderir.

**DTO:** `settle_request.rs`.

### Quote Endpoint (GET /quote)

Kullanıcıya fiyat teklifi döner. Price estimation katmanına delege eder.

### Metrics / Healthz / Info Endpoints

`/metrics` — Prometheus metrics scraping
`/healthz` — Kubernetes readiness/liveness probe
`/info` — Versiyon ve konfigürasyon bilgisi

---

## 10. infra/solver/eip7702.rs — EIP-7702 Delegasyon

EIP-7702: EOA hesaplar geçici olarak smart contract kodu çalıştırabilir. Solver EOA'sı `Solver7702Delegate` kontratı delegasyon olarak kullanır.

Bu mekanizma:
1. Settlement tx gas ödemesini ayırır
2. Çok imzalı sözdizimi destekler
3. Future MEV koruma mekanizmaları için zemin hazırlar

---

## 11. infra/notify/liquidity_sources/ — Likidite Bildirim Sistemi

### Liquorice Client

Dış servis bildirimleri: Solver kullandığı pool'ları Liquorice servisine bildirir. Bu servis likidite sağlayıcılarına pozisyonlarını optimize etme bilgisi verir.

```rust
pub struct LiquoriceNotifier {
    client: Client,
    url: Url,
}
impl LiquoriceNotifier {
    pub async fn notify(&self, pools: &[PoolAddress]) -> Result<()> { ... }
}
```

---

## 12. infra/mempool/ — Mempool Implementasyonları

### Mempool Türleri

1. **Public mempool:** Ethereum P2P ağına direkt gönderim. Hızlı ama MEV saldırısına açık.
2. **Private mempool (Flashbots/MEV Blocker):** Bundle submission. MEV koruması var ama latency yüksek.
3. **Custom mempool:** Konfigürasyon üzerinden tanımlanan custom endpoint.

---

## Tasarım Kararları ve Patterns

### 1. CIP-38 Skor Formülü
Surplus + protocol_fees → buy token → native token. Her konversiyonda integer taşması önlemek için U512 genişletme.

### 2. Fee-after-fee Matematği
Post-fee surplus üzerinden hesaplama: `factor' = factor / (1 - factor)`. Sell order için buy amount düşer, buy order için sell amount artar.

### 3. Paralel Mempool Yarışması
`select_ok` pattern: N mempool paralel başlatılır, ilk başarılı kazanır. İki avantaj:
- Flashbots başarısız olursa public mempool yedek olarak çalışır
- Latency minimize edilir

### 4. f64::total_cmp() Performans Seçimi
`BigRational`'dan `f64::total_cmp()`'ye geçiş. Ticaret-offs: hafif hassasiyet kaybı vs. büyük hız kazanımı (sıralama kritik yolda).

### 5. Internalization/Uninternalization İkilisi
Her settlement iki tx versiyonu taşır:
- `internalized`: Güvenilir token interaction'ları vault'tan sağlanır, gas düşük
- `uninternalized`: Tüm interaction'lar explicit, güvenlik için gerektiğinde kullanılır

---

## DEX Uygulama Notu — Nova Shield DEX

### Skor Formülü (Nova Shield Batch Auction için)

Nova Shield DEX batch auction (CoW benzeri) için CIP-38 skor:

```
score = (user_surplus + protocol_fees) → buy_token → native_token
```

Sell order:
```
surplus_buy = executed * clearing_price_sell / clearing_price_buy - limit_buy
score = surplus_buy * native_price_buy
```

Buy order:
```
surplus_sell = limit_sell_scaled - executed * clearing_price_buy / clearing_price_sell
surplus_buy = surplus_sell * buy_price / sell_price  // U512 genişletme gerekli!
score = surplus_buy * native_price_buy
```

### Protocol Fee Yapısı — Üç Seçenek

Nova Shield için önerilen fee mimarisi:
1. **Surplus fee:** `factor * surplus / (1 - factor)` — solver kalitesini ödüllendirir
2. **Volume fee:** `factor * volume / (1 ± factor)` — basit ve öngörülebilir
3. **Price improvement fee:** Quote'tan iyileştirme oranında — kullanıcıyı teşvik eder

Her fee için `max_volume_factor` cap'i ekle: excessive fee önlenir.

### Slippage Parametresi Nova Shield DEX için

```
Default: relative=0.5%, min=0.001 ETH, max=0.05 ETH
High-value orders: max artırılabilir
Stable pairs: relative=0.1%
```

Fiyatsız token'lar için fallback: salt relatif slippage, cap uygulanmaz.

### Order Sorting Stratejisi Nova Shield için

Üç katman halinde sırala:
1. `ExternalPrice`: Çözülebilir order'ları öne al (`sell_value / buy_value > 1`)
2. `OwnQuotes`: Kendi quote verdiği order'ları ödüllendir (kalite teşvik)
3. `CreationTimestamp`: Yeni order'lar önce (fairness)

```rust
let strategies: Vec<Arc<dyn SortingStrategy>> = vec![
    Arc::new(ExternalPrice),
    Arc::new(OwnQuotes { max_order_age: Some(Duration::minutes(5)) }),
    Arc::new(CreationTimestamp { max_order_age: None }),
];
```

### EIP-7702 + Paralel Mempool

Nova Shield'in L1 blockchain'inde:
- EIP-7702 eşdeğeri: Validator EOA'ları settlement executor rolü delegasyon ile alabilir
- Paralel submission: Ana P2P ağ + builder/sequencer (MEV koruması)

### adjust_quote_to_order_limits() Nova Shield DEX için

Nova Shield quoting sistemi kullanıcı emirlerini quote ile karşılaştırırken:

Sell order:
```
adjusted_buy = quote.buy - (fee * quote.buy / quote.sell)  // fee'yi çıkar
scaled_buy = adjusted_buy * order.sell / quote.sell         // ölçekle
final_buy = max(order.buy, scaled_buy)                      // kullanıcı limitini koru
```

Buy order:
```
adjusted_sell = quote.sell + fee                            // fee ekle
scaled_sell = adjusted_sell * order.buy / quote.buy         // ölçekle
final_sell = min(order.sell, scaled_sell)                   // kullanıcı limitini koru
```

### Nasıl Daha İyi Yapılır?

1. **Skor Normalizasyonu:** Farklı decimal'lı token'larda skor karşılaştırması bazen yanıltıcı. Nova Shield'de her token için native price ile normalize edilmiş bir "score unit" (örn. 1e-18 ETH = 1 gwei unit) tanımlanabilir.

2. **Dinamik Gas Bump:** Sabit %13 yerine, mempool congestion izlenerek adaptif bump. Düşük congestion'da %10, yüksek congestion'da %20.

3. **Multi-objective Sorting:** Mevcut uygulama lexicographic. Nova Shield'de pareto-optimality tabanlı sıralama: birden fazla kriterde aynı anda optimal olanı seç.

4. **Fee Accumulation Account:** Protocol fee'ler doğrudan DAO treasury'e gitmek yerine bir Accumulation Account'a toplanabilir, governance kararıyla dağıtılır.

5. **Real-time Surplus Monitoring:** Settlement sonrası gerçekleşen surplus'u kullanıcıya bildirim sistemi — DEX UX differentiator.
