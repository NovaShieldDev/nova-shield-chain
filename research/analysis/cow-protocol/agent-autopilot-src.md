# CoW Protocol Analiz — Agent 04 (Autopilot/src)
## Kapsam: autopilot/src/ — Protokolün Beyin Merkezi
## DEX Ozel Protokolu: 3x Hassasiyet
## Tarih: 2026-07-22

---

## DOSYA HARITASI

Toplam 62 dosya analiz edildi.

| Dosya | Satir |
|---|---|
| run_loop.rs | 1286 |
| solvable_orders.rs | 1340 |
| domain/settlement/mod.rs | 1570 |
| domain/competition/winner_selection.rs | 1295 |
| domain/fee/mod.rs | 704 |
| domain/settlement/trade/math.rs | 552 |
| infra/persistence/mod.rs | 1051 |
| maintenance.rs | 315 |
| infra/solvers/dto/solve.rs | 371 |
| domain/settlement/transaction/mod.rs | 281 |
| domain/settlement/trade/mod.rs | 192 |
| domain/auction/order.rs | 273 |
| domain/auction/mod.rs | 96 |
| domain/competition/mod.rs | 106 |
| domain/competition/bid.rs | 64 |
| shadow.rs | 301 |
| event_updater.rs | 81 |
| leader_lock_tracker.rs | 130 |
| shutdown_controller.rs | 68 |
| periodic_db_cleanup.rs | 179 |
| domain/settlement/observer.rs | 160 |
| domain/settlement/auction.rs | 26 |
| domain/fee/policy.rs | 107 |
| domain/quote/mod.rs | 38 |
| domain/mod.rs | 30 |
| boundary/mod.rs | 47 |
| infra/solvers/mod.rs | 187 |
| + diger dto/db/infra dosyalari | ~2000+ |
| **TOPLAM** | **~13.000+** |

---

## 1. run_loop.rs (1286 satir) — EN KRITIK

**Amac:** Autopilot'un ana cevrim dongusu — bloklari izler, auction keser, solver yarismasini yurutur, kazananlari settle eder.

### Struct/Enum/Trait Listesi

**`Config` (satir 57-70):**
```rust
pub struct Config {
    pub submission_deadline: u64,           // Kac blok icinde tx zincire girmeli
    pub max_settlement_transaction_wait: Duration, // Driver /settle timeout
    pub min_solve_time: Duration,           // Minimum solver suresi
    pub sync_solve_deadline_to_blockchain: Option<SlotConfig>, // PoS slot senkronizasyonu
    pub max_run_loop_delay: Duration,       // Blok geldikten sonra max gecikme
    pub max_winners_per_auction: NonZeroUsize,  // Ayni anda kac kazanan olabilir
    pub max_solutions_per_solver: NonZeroUsize, // Solver basina max cozum sayisi
    pub enable_leader_lock: bool,           // Coklu autopilot destegi
    pub compress_solve_request: bool,       // Brotli sikistirma aktif mi
}
```

**`SlotConfig` (satir 95-98):**
- `slot_length: Duration` — PoS blok araligi (ETH=12s, Gnosis=5s)
- `tx_propagation_latency: Duration` — Tx yayilma gecikmesi (~2s)

**`RunLoop` (satir 105-119):**
- `config: Config`
- `eth: infra::Ethereum` — blockchain iletisimi
- `persistence: infra::Persistence` — DB/S3 iletisimi
- `drivers: Vec<Arc<infra::Driver>>` — kayitli solver driver'lari
- `solvable_orders_cache: Arc<SolvableOrdersCache>` — cozulebilir order cache
- `trusted_tokens: AutoUpdatingTokenList` — guvenilir token listesi
- `probes: Probes` — liveness/startup health probe
- `maintenance: MaintenanceSync` — bakim senkronizasyonu
- `winner_selection: winner_selection::Arbitrator` — kazanan secici
- `wake_notify: Arc<tokio::sync::Notify>` — blok/order uyandirici

**`Probes` (satir 100-103):**
- `liveness: Arc<Liveness>`
- `startup: Arc<Option<AtomicBool>>`

### Kritik Fonksiyonlar

**`run_forever()` (satir 157-208) — ANA DONGÜ:**

```
DÖNGÜ:
  1. leader_lock_tracker.try_acquire() — leader kontrolu
  2. wake_notify.notified().await — yeni blok veya order bekle
  3. update_caches(prev_block, is_leader) — cache guncelle
  4. startup probe'u set et (true)
  5. leader degilse 200ms bekle -> continue
  6. next_auction(start_block, prev_auction, prev_block) -> Option<Auction>
  7. auction degismemisse (prev == current AND same block) -> skip
  8. single_run(auction).await — TEK TAM KOSIS
  9. shutdown kontrolu
```

**Timing Analizi:**
- Dongü kendisi blok temelli calisir (12s ETH, 5s Gnosis)
- `wake_notify` ile tetiklenir: ya yeni blok ya yeni order
- `max_run_loop_delay` gecilmisse bir sonraki blok beklenir
- Auction aynysa ve blok degismemisse solver tekrar cagrilmaz (deduplikasyon)

**`update_caches()` (satir 237-275):**
```
1. current_block al
2. time_since_last_block hesapla
3. max_run_loop_delay asilmissa -> next_block bekle (UYARI log)
4. maintenance.wait_until_block_processed(PartiallyProcessed)
5. solvable_orders_cache.update(block_number, is_leader)
```

**`cut_auction()` (satir 304-334):**
```
1. solvable_orders_cache.current_auction().await -> None ise return None
2. persistence.get_next_auction_id() — DB'den siradaki auction ID al
3. persistence.replace_current_auction_in_db(id, auction) — DB guncelle
4. persistence.upload_auction_to_s3(id, auction) — S3'e yukle
5. auction.orders.is_empty() -> skip (liveness guncelle ama return None)
6. domain::Auction { id, block, orders, prices, ... } dondur
```

**`single_run()` (satir 337-413):**
```
1. Auction order'larini OrderEventLabel::Ready yap
2. fetch_solutions(&auction) -> Vec<Bid<Unscored>>
3. winner_selection.arbitrate(solutions, &auction) -> Ranking
4. post_processing(...) -> DB'ye kaydet
5. Kazanan orderlar: Executing, kaybedenler: Considered
6. Her kazanan icin start_settlement_execution() -> tokio::spawn background
```

**`fetch_solutions()` (satir 592-640):**
```
1. pick_solve_deadline() -> deadline hesapla
2. solve::Request::new(auction, trusted_tokens, deadline, compress)
3. join_all(drivers.iter().map(|d| solve(d, request))) — TUM DRIVERLAR PARALEL
4. Cozumler filtreleme:
   - solver() == submission_address degilse filtrele
   - Solver basina max_solutions_per_solver siniri
5. bids.shuffle(rng) — esit puanlilar icin rastgele sira (TIE-BREAKER)
```

**`try_solve()` (satir 674-724):**
```
1. Paralel:
   a. driver.solve(request) — /solve HTTP cagrisi
   b. authenticator.isSolver(submission_address) — onchain whitelist
2. Timeout uygula (deadline - now)
3. isSolver=false -> SolverDenyListed
4. solutions.is_empty() -> NoSolutions
```

**`settle()` (satir 729-788):**
```
1. current_block < submission_deadline_latest_block kontrolu
2. store_execution_started() — background task DB kayit
3. driver.settle(&request, timeout) || wait_for_settlement_transaction()
   -> RACE: ikisi paralel, biri bitince diger iptal
4. store_execution_ended() — background task DB kayit
```

**`pick_solve_deadline_impl()` (satir 911-955) — PoS SLOT SENKRONIZASYONU:**

```
minimum_deadline = now + min_solve_time

Eger SlotConfig yoksa -> minimum_deadline dondur

1..10 araliginda delay_in_blocks:
  target = current_block_time + slot_length*N - tx_propagation_latency
  if target >= minimum_deadline:
    return target  // Ilk yeterli blok

return minimum_deadline  // Fallback
```

Matematiksel Formul:
- ETH: slot=12s, latency=2s -> target = block_time + 12*N - 2
- Gnosis: slot=5s, latency=2s -> target = block_time + 5*N - 2
- N arttirilir ta ki min_solve_time saglanana kadar

Test kaniti (satir 1231-1284):
- ETH+1s gecikme -> deadline = now+9s
- ETH+2s gecikme -> deadline = block+22s (N=2 slot)
- Gnosis+1s -> deadline = block+13s (N=3 slot, 5s kisa slot)

### Hata Tipleri

**`SolveError`:** Timeout | NoSolutions | Failure(anyhow) | SolverDenyListed
**`SettleError`:** Other(anyhow) | Timeout

### Metrics

Prometheus metrikleri (satir 977-1047):
- `auction` — son auction ID
- `auction_winners` — kazanan sayisi histogram (0-10 bucket)
- `solve` — /solve request suresi per driver (0-20s)
- `settle` — /settle suresi (0-48s)
- `matched_unsettled` — kazanilip settle edilemeyen order
- `solve_request_body_size` — /solve body boyutu (1KB-4MB)

### Guvenlik Analizi

**Vektor: Kotu Niyetli Driver Solver Adresi**
- Koruma: `bid.solution().solver() == driver.submission_address` (satir 617-628)

**Vektor: Onchain Whitelist Bypass**
- Koruma: `isSolver()` /solve ile paralel; false -> SolverDenyListed
- Kontrol settlement tx blokunda yapilir

**Vektor: Deadline Manipulasyonu**
- Koruma: `submission_deadline_latest_block` asilirsa settle iptal

**Vektor: Replay (Tekrar)**
- Koruma: prev_auction==current AND prev_block==current_block -> skip

**DEX Uygulama Notu — Nova Shield icin:**
- "notify -> update caches -> cut auction -> paralel solve -> rank -> settle" akisi dogrudan uygulanabilir
- Nova Shield 2s blok hedefi: slot_length=2s, latency=0.3s, min_solve_time=1s
- Shuffle tie-breaking: VRF ile degistir (deterministik, audit edilebilir)
- Leader lock: zero-downtime deploy icin Postgres advisory lock pattern
- Submission address check: Nova Shield MPC imzalayicisinda submission_address = MPC public key hash

---

## 2. domain/competition/winner_selection.rs (1295 satir)

**Amac:** Auction arbitrasyonu — fairness filtresi, kazanan secimi, reference score hesabi.

### Cekirdek Algoritma

**Uniform Directional Clearing Price:**
- Her token icin 2 fiyat: "satis fiyati" ve "alim fiyati"
- Ayni tokeni satan tum orderlar ayni fiyattan islem gorur
- Fairness Garantisi: Order, tek basina aldigi fiyattan daha kotu fiyatla batche dahil edilemez

**Reference Score Formulu:**
```
reference_score[S] = total_score(winners WITHOUT solver S)
reward[S] = total_score(all winners) - reference_score[S]
```

Bu, bir solver'in yarismaya kattigi gercek deger olcumu.

### Struct/Enum Listesi

**`Arbitrator` (satir 41-57):**
```rust
pub struct Arbitrator(winsel::Arbitrator {
    max_winners: usize,
    weth: eth::Address,
})
```

**`Ranking` (satir 233-275):**
```rust
pub struct Ranking {
    filtered_out: Vec<Bid<Ranked>>,  // Fairness filtresi gecemediler
    ranked: Vec<Bid<Ranked>>,         // Sirali (winners once, non-winners sonra)
    reference_scores: HashMap<eth::Address, Score>,
}
```

API:
- `all()` — filtered_out + ranked
- `winners()` — sadece kazananlar
- `non_winners()` — kazanmayanlar
- `ranked()` — tum gecenler

### Kritik Fonksiyonlar

**`arbitrate()` (satir 61-114):**
```
1. auction -> winsel::AuctionContext (fee_policies, native_prices, jit_owners)
2. Her bid -> SolutionKey(solver, solution_id) ile HashMap
3. bids -> Vec<winsel::Solution<Unscored>>
4. winsel::Arbitrator::arbitrate(solutions, &context) -> ws_ranking
5. compute_reference_scores(&ws_ranking) -> HashMap<Address, Score>
6. filtered_out + ranked listelerini olustur
7. Ranking { filtered_out, ranked, reference_scores } dondur
```

### Test Analizleri (satir 277-1295)

**`single_bid` testi:**
- Tek solver -> reference_score = 0
- Reward = total_score (tum surplus kendine)

**`compatible_bids` testi:**
- Solver A: Order 1+2 (score=200), Solver B: Order 3 (score=100)
- Her ikisi de WINNER (farkli token cifti)
- reference_score[A] = 100, reference_score[B] = 200
- Reward: A = 300-100=200, B = 300-200=100

**`fairness_filtering` testi (satir 641-701):**
- "Unfair batch": Order 1+2 (score=200)
- "Filtering batch": Sadece Order 1 ama DAHA IYI FIYAT (score=150)
- Unfair batch Order 1'e filtering batch'ten daha kotu fiyat sunuyor
- Unfair batch FILTRELENIR -> sadece Filtering batch kazanir

**`reference_better_than_winners` testi (satir 766-853):**
- Best batch: 3 order, score=300
- Incompat 1: Order 1+2 (score=280), Incompat 2: Order 3 (score=100)
- reference_score[Best] = 280+100 = 380 > 300
- Reward = 300-380 = NEGATIF -> 0'a cap edilir

**Gercek Mainnet Testi (satir 855-):**
- Auction 12825008, ETH mainnet
- Solver 1 (baseline): 21.8M wei score -> WINNER
- Solver 2 (zeroex): 21.0M wei score -> kaybetti
- reference_score[S1] = 21.0M
- Reward = 21.8M - 21.0M = 0.8M wei

### Guvenlik Analizi

**Vektor: Fairness Ihlali**
- Koruma: Tek basina en iyi clearing price hesaplanir, batch daha kotuyse filtered_out

**Vektor: Reference Score Manipulasyonu**
- Koruma: submission_address run_loop'ta dogrulanir, iki solver ayni adres kullanamaz

**DEX Uygulama Notu — Nova Shield icin:**
- Bu algoritma Nova Shield DEX icin DOGRUDAN uygulanabilir
- Directional clearing prices: AMM pool limitleri icinde uniform price garantisi
- Reference score = solver odulunun matematiksel temeli
- Fairness filter = MEV korumasinin bir formu
- `winsel` crate'ini dependency olarak kullanabilirsin
- IMPROVEMENT: VRF-tabanli tie-breaking (shuffle yerine)
- IMPROVEMENT: ZK proof ile off-chain score hesabini dogrula

---

## 3. domain/competition/bid.rs (64 satir)

**Amac:** Type-state pattern ile Bid ilerleyisini compile-time'da zorla.

**`Bid<State>` (satir 23-27):**
```rust
pub struct Bid<State = Ranked> {
    solution: Solution,
    driver: Arc<infra::Driver>,
    state: State,  // Unscored | Scored | Ranked
}
```

State Progression: `Bid<Unscored>` -> `Bid<Scored>` -> `Bid<Ranked>`

**`HasState` trait impl:** `with_state<NewState>()` ile tip degisimi, sifir runtime cost.

**DEX Uygulama Notu — Nova Shield icin:**
- Nova Shield order lifecycle: Submitted -> Validated -> Solvable -> Competing -> Executing -> Settled
- Her state icin ayri tip -> yanlis state'de method cagrisi compile error

---

## 4. domain/competition/mod.rs (106 satir)

**`Solution` (satir 18-34):**
```rust
pub struct Solution {
    id: SolutionId,  // u64
    solver: Address,
    orders: HashMap<domain::OrderUid, TradedOrder>,
}
```

**`TradedOrder` (satir 54-65):**
```rust
pub struct TradedOrder {
    pub side: order::Side,
    pub sell: eth::Asset,          // limit sell token+amount
    pub buy: eth::Asset,           // limit buy token+amount
    pub executed_sell: eth::TokenAmount,
    pub executed_buy: eth::TokenAmount,
}
```

**`Score` (satir 67-106):** `Score(eth::Ether)` — ETH cinsinden puan
- `saturating_add`, `checked_sub` impl
- Display, Add, Sub, Ord, PartialOrd derive

**DEX Uygulama Notu:**
- Score = ETH cinsinden surplus — para birimi bagimsiz olcum
- native_prices gereksinimi: tum tokenlar ETH bazinda

---

## 5. domain/auction/mod.rs (96 satir)

**`RawAuctionData` (satir 16-21):**
```rust
pub struct RawAuctionData {
    pub block: u64,
    pub orders: Vec<Order>,
    pub prices: Prices,  // HashMap<TokenAddress, Price>
    pub surplus_capturing_jit_order_owners: Vec<Address>,
}
```

**`Auction` (satir 25-32):** Ayni + `id: Id (i64)`
PartialEq: ID haric karsilastirir. Kritik: prev_auction==current ID'siz karsilastirir.

**`Price` (satir 45-88):**
```
Price(p) = x ETH per 10^18 token birim

in_eth(amount):
  = amount * p / 10^18

Ornek: 1 USDC = 1/1800 ETH -> p = 10^18/1800 wei/USDC
```

**DEX Uygulama Notu:**
- Price normalizasyonu: tum tokenlar ETH bazinda -> birlestik skor hesabi
- BASE=10^18 -> precision loss riski yok (U256)
- surplus_capturing_jit_order_owners: CoW AMM surplusu LP'lere yonlendirir

---

## 6. domain/auction/order.rs (273 satir)

**`Order` (satir 9-30):**
```rust
pub struct Order {
    pub uid: OrderUid,              // 56 byte: 32 digest + 20 owner + 4 validTo
    pub sell: eth::Asset,           // sell_token + limit sell amount
    pub buy: eth::Asset,            // buy_token + limit buy amount
    pub protocol_fees: Vec<fee::Policy>,
    pub side: Side,
    pub created: u32,
    pub valid_to: u32,
    pub receiver: Option<eth::Address>,
    pub owner: eth::Address,
    pub partially_fillable: bool,
    pub executed: TargetAmount,
    pub pre_interactions: Vec<Interaction>,   // settlement oncesi calllar
    pub post_interactions: Vec<Interaction>,  // settlement sonrasi calllar
    pub sell_token_balance: SellTokenSource,
    pub buy_token_balance: BuyTokenDestination,
    pub app_data: AppDataHash,
    pub signature: Signature,
    pub quote: Option<domain::Quote>,
}
```

**`OrderUid` Layout:**
```
[0..32]  = EIP-712 order hash (B256)
[32..52] = owner address (20 bytes)
[52..56] = validTo (4 bytes, BIG-ENDIAN)
```
Test (satir 222-238): little-endian bug guard.

**`Signature` (satir 113-135):**
- `Eip712(EcdsaSignature)` — standart
- `EthSign(EcdsaSignature)` — personal_sign
- `Eip1271(Vec<u8>)` — smart contract (arbitrary)
- `PreSign` — onchain tx imza

**`SellTokenSource`:** Erc20 | External | Internal
**`BuyTokenDestination`:** Erc20 | Internal

**Guvenlik Analizi:**

**Vektor: OrderUid Endian Bug**
- Koruma: `u32::from_be_bytes` (satir 43)
- Test satir 230: big-endian [12,34,56,78] -> 0x12345678

**Vektor: EIP-1271 Balance Bypass**
- Koruma: Balance check driver'a devredilir
- Pre-interaction ile funds saglanabilir -> intentional design

**DEX Uygulama Notu — Nova Shield icin:**
- OrderUid yapisini dogrudan adapte et: [hash(32) + owner(20) + valid_to(4)]
- Pre/post interactions: DEX'te flashloan, yield farming exit, bridge call
- EIP-1271: yield vault'larindan otomatik siparis

---

## 7. domain/fee/mod.rs (704 satir) — FEE MODELI

**`Policy` enum (satir 323-356):**
```rust
pub enum Policy {
    Surplus { factor: FeeFactor, max_volume_factor: FeeFactor },
    PriceImprovement { factor: FeeFactor, max_volume_factor: FeeFactor, quote: Quote },
    Volume { factor: FeeFactor },
}
```

**`ProtocolFees` (satir 87-120):**
- `fee_policies: Vec<ProtocolFee>`
- `max_partner_fee: FeeFactor`
- `upcoming_fee_policies: Option<UpcomingProtocolFees>` — zamanlanmis degisiklik
- `volume_fee_policy: VolumeFeePolicy` — token bucket fee

### Kritik Fonksiyonlar

**`apply()` (satir 244-268):**
```
1. quote yoksa: buy_amount=0 -> out-of-market sayilir
2. partner_fee = get_partner_fee(order, quote, max_partner_fee)
3. JIT order sahibiyse -> sadece partner_fee uygula
4. apply_policies(order, quote, partner_fees)
```

**`apply_policies()` (satir 270-292):**
```
1. upcoming policy zaman geldiyse -> onu kullan
2. fee_policies filtrele (Limit/Market/Any sinifi)
3. variant_fee_apply() ile Policy uygula
4. + partner_fees ekle
```

**`get_partner_fee()` (satir 123-240) — COMPOUND FEE CAP:**

```
Matematiksel Formul:
remaining_factor = (1 + cap) / (1 + accumulated) - 1
fee_capped = min(fee, remaining_factor)
accumulated += fee_capped

Compound ozelligi:
%10 + %20 = (1.1)(1.2)-1 = %32 (cap %30 ise 2. fee kirilir)

Test kaniti (satir 632):
10% + 25% -> 10% + 18.18...% = %30 cap
factor = 0.18181818181818182
```

Bps donusumu: `fee_factor = bps / 10_000`

### Guvenlik Analizi

**Vektor: Partner Fee Sinir Asimi**
- Koruma: compound hesap ile birden fazla partner fee bile cap'i asamaz

**Vektor: Upcoming Policy Race Condition**
- `now >= effective_from_timestamp` kontrolu (satir 280)
- Zaman drift riski: NTP senkronizasyonu kritik

**Vektor: Market Order Fee Bypass**
- Market orderlara Surplus ve PriceImprovement uygulanmaz (intentional)

**DEX Uygulama Notu — Nova Shield icin:**

Formul referanslari:
```
Surplus fee:
  fee = surplus_after * factor / (1 - factor)

Volume fee (sell):
  fee = buy_amount_after * factor / (1 - factor)

Volume fee (buy):
  fee = sell_amount_after * factor / (1 + factor)

Partner fee compound cap:
  remaining = (1 + cap) / (1 + accum) - 1
```

Oneriler:
- upcoming_fee_policies: Nova Shield governance vote sonrasi fee degisikligi
- Partner fee: entegrasyon protokolleri icin revenue share
- JIT order surplus capture: LP'lere yonlendir
- IMPROVEMENT: on-chain fee parametreleri (governance contract)

---

## 8. domain/fee/policy.rs (107 satir)

**Hangi order class'ina hangi fee uygulanir:**

| Policy | Market | Limit | Liquidity |
|---|---|---|---|
| Surplus | None | Some | None |
| PriceImprovement | None | Some | None |
| Volume | None | Some (bucket) | None |

- Market order -> fee yok (kullanici dostu)
- Liquidity/CoW AMM order -> fee yok (LP tesvik)
- Limit order -> fee uygulanir

`Volume::apply()` token bucket kullanir: token cifti risk profiline gore farkli volume fee.

---

## 9. domain/settlement/trade/math.rs (552 satir) — TEMEL HESAPLAMA

**`Trade` (satir 21-28):**
```rust
pub struct Trade {
    pub uid: domain::OrderUid,
    pub sell: eth::Asset,     // limit sell amount
    pub buy: eth::Asset,      // limit buy amount
    pub side: order::Side,
    pub executed: order::TargetAmount,
    pub prices: Prices,  // uniform (before fee) + custom (after fee)
}
```

### Matematiksel Formuller

**Surplus Formulu (satir 34-83):**

```
SELL ORDER:
  limit_buy_scaled = executed_sell * limit_buy / limit_sell  (CEIL div!)
  bought = executed_sell * clearing_sell / clearing_buy  (CEIL div!)
  surplus = bought - limit_buy_scaled

BUY ORDER:
  limit_sell_scaled = limit_sell * executed_buy / limit_buy
  sold = executed_buy * clearing_buy / clearing_sell
  surplus = limit_sell_scaled - sold
```

NOT: CEIL div kullanilir — settlement contract ile tutarlilik icin.

**surplus_in_ether() (satir 87-95):**
```
surplus_eth = surplus_amount * native_price / 10^18
```

**fee() (satir 134-141):**
```
fee = surplus_before_fee - surplus_after_fee
    = surplus_over(uniform_prices) - surplus_over(custom_prices)
```

**protocol_fee() (satir 249-275):**
```
Surplus policy:
  surplus = surplus_over_limit_price()  // custom prices
  fee = min(surplus_fee(surplus, factor), volume_fee(max_volume_factor))

PriceImprovement policy:
  improvement = max(0, surplus_over_quote())
  fee = min(surplus_fee(improvement, factor), volume_fee(max_volume_factor))

Volume policy:
  fee = volume_fee(factor)
```

**surplus_fee() (satir 330-358) — FAKTOR DONUSUMU:**
```
Turetme:
  fee = surplus_before * factor
  surplus_after = surplus_before - fee = surplus_before * (1-factor)
  fee = surplus_after * factor / (1-factor)

Kod:
  fee = surplus.0 * (factor / (1.0 - factor))
```
Ornek: factor=0.5 -> factor'=1.0 -> fee = surplus_after

**volume_fee() (satir 361-405):**
```
Sell order:
  fee_before = bought * factor
  bought_after = bought - fee
  factor' = factor / (1-factor)
  fee = bought_after * factor / (1-factor)

Buy order:
  fee_before = sold * factor
  sold_after = sold + fee
  factor' = factor / (1+factor)
  fee = sold_after * factor / (1+factor)
```

**calculate_custom_prices() (satir 224-246):**
```
Protocol fee uygulandiktan sonra clearing prices guncelle:

SELL ORDER (fee buy token'dan alinir):
  new_sell_price = buy_amount + protocol_fee
  new_buy_price = sell_amount

BUY ORDER (fee sell token'dan alinir):
  new_sell_price = buy_amount
  new_buy_price = sell_amount - protocol_fee
```

**adjust_quote_to_order_limits() (satir 440-482) — PRICE IMPROVEMENT AYARI:**
```
SELL ORDER:
  quote_buy_net = quote_buy - (quote_fee * quote_buy / quote_sell)  // fee cikar
  scaled_buy = quote_buy_net * order_sell / quote_sell              // order miktarina olcekle
  buy_amount = max(order_buy_limit, scaled_buy)

BUY ORDER:
  quote_sell_total = quote_sell + quote_fee  // fee ekle
  scaled_sell = quote_sell_total * order_buy / quote_buy
  sell_amount = min(order_sell_limit, scaled_sell)
```

### Hata Tipleri

- `MissingPrice(TokenAddress)` — native price bulunamadi
- `Math::Overflow` — U256 tasma
- `Math::DivisionByZero`
- `Math::Negative` — surplus < 0 (order doldurulmadi)

### Guvenlik Analizi

**Vektor: Arithmetic Overflow**
- Koruma: Tum islemler `checked_*` — overflow -> error

**Vektor: Precision Loss**
- `checked_mul_f64` — f64 precision kaybi riski
- Mitigasyon: factor 0-1 araliginda bounded

**Vektor: Custom Price Manipulation**
- Custom prices matematiksel turetme, driver belirleyemiyor

**DEX Uygulama Notu — Nova Shield icin:**

Surplus hesabi direkt kullanim:
```rust
// Sell order surplus (ETH):
let surplus_eth = (executed_buy * clearing_sell / clearing_buy - limit_buy)
                  * native_price / 10^18;

// Fee formulleri (hepsi degistirilen faktor ile):
let surplus_fee = surplus * factor / (1.0 - factor);
let vol_fee_sell = bought * factor / (1.0 - factor);
let vol_fee_buy = sold * factor / (1.0 + factor);
```

IMPROVEMENT: Tum `f64` islemleri fixed-point arithmetic ile:
```rust
let fee = surplus * factor_num / factor_den;  // integer, no float
```

---

## 10. domain/settlement/mod.rs (1570 satir) — SETTLEMENT DOMAIN

**`Settlement` (satir 49-65):**
```rust
pub struct Settlement {
    gas: eth::Gas,
    gas_price: eth::EffectiveGasPrice,
    block: eth::BlockNo,
    solver: eth::Address,
    solution_uid: i64,
    auction: Auction,
    trades: Vec<Trade>,
}
```

**`Settlement::new()` (satir 156-208):**
```
1. Paralel:
   a. get_auction(auction_id, trades) — DB'den auction
   b. get_solver_winning_solutions(auction_id, solver) — kazanan solution'lar
2. settled.block > auction.block + max_settlement_age -> WrongEnvironment
3. find_winning_solution_uid(solutions, trades) — trade set'i ile eslesir
4. trades -> Trade::new() donusum
```

**`find_winning_solution_uid()` (satir 266-276):**
```
settled_keys = settled_trades -> Set<{uid, token, executed}>
solutions.iter().find_map(|s|
  s.orders -> Set<{uid, token, executed}>
  settled_keys == solution_keys -> Some(uid)
)
```

**`trade_to_key()` vs `order_to_key()` (satir 226-261):**
- Partially fillable: executed = gercek miktar
- Not partially fillable: executed = limit amount (SC davranisi)
- 3 boyutlu eslesme: uid + token + executed amount

**`max_settlement_age()` (satir 282-284):**
- TARGET = 6 saat in ms
- ETH ~1800 blok, Gnosis ~4320 blok
- Prod/staging cross-contamination detector

### Guvenlik Analizi

**Vektor: Environment Karisimi**
- 6 saat yas siniri: staging'deki prod auction'i tespit eder

**Vektor: Solution Matching Manipulasyonu**
- 3 boyutlu key: ayni UID, farkli executed amount -> match yok

**DEX Uygulama Notu — Nova Shield icin:**
- Settlement domain = gercek islemle off-chain plan kiyaslamasi
- `find_winning_solution_uid` benzeri: zincirdeki settlement'i dogru auction'a bagla
- `max_settlement_age`: Nova Shield 2s blok -> 6h = ~10800 blok (ETH'den 6x fazla)
- `summarize()`: Nova Shield analytics backend icin temel veri

---

## 11. domain/settlement/transaction/mod.rs (281 satir)

**`Transaction` (satir 49-66):**
- hash, auction_id, block, timestamp, gas, gas_price, solver, trades

**`Transaction::try_new()` (satir 69-177):**
```
1. BFS ile settlement call bul (trace tree)
2. calldata son 8 byte = auction_id (i64 big-endian)
3. GPv2Settlement::settleCall::abi_decode(calldata)
4. Her trade: flags -> side/scheme/balance decode, EIP-712 UID recovery
5. Prices: uniform (genel) + custom (fee dahil) clearing prices
6. find_solver_address(): callers listesinde ilk allow-listed adres
```

**Auction ID Encoding:**
```rust
const META_DATA_LEN: usize = 8;
let auction_id = i64::from_be_bytes(calldata[len-8..]);
```

**`find_settlement_trace_and_callers()` (satir 180-201):**
BFS ile trace tree aramalari: root -> nested calls, callers listesi olusuyor.

**`find_solver_address()` (satir 209-228):**
Callers listesinde `isSolver()` kontrolu — ilk allow-listed = solver.

**Guvenlik Analizi:**

**Vektor: Auction ID Sahteleme**
- Koruma: Settlement contract'tan gelen event'lar (onceden filtreli)
- Eksik/yanlis ID -> hata log, Ok(None)

**Vektor: Smart Contract Solver Bypass**
- BFS callers listesi + authenticator per-caller check

**DEX Uygulama Notu — Nova Shield icin:**
- Auction ID calldata sonuna append: 8 byte, minimal gas overhead
- BFS trace arama: proxy contract solver destegi
- Nova Shield solver = MPC node cluster -> submission_address = MPC pubkey hash

---

## 12. solvable_orders.rs (1340 satir) — ORDER FILTRELEME

**`SolvableOrdersCache` (satir 131-146):**
```rust
pub struct SolvableOrdersCache {
    min_order_validity_period: Duration,
    banned_users: banned::Users,
    balance_fetcher: Arc<dyn BalanceFetching>,
    deny_listed_tokens: DenyListedTokens,
    cache: Mutex<Option<Inner>>,
    native_price_estimator: Arc<NativePriceUpdater>,
    weth: Address,                         // hardcoded price=1
    protocol_fees: domain::ProtocolFees,
    cow_amm_registry: cow_amm::Registry,
    disable_order_balance_filter: bool,
    wrapper_cache: app_data::WrapperCache, // LRU 20_000 cap
}
```

**`update()` (satir 204-370) — FILTRELEME PIPELINE:**
```
1. DB'den order cek (incremental veya full)
2. wrapper app_data analizi -> balance check exempt listesi
3. PARALEL:
   a. fetch_balances(queries)
   b. filter_invalid_orders (ban + imza + token)
   c. cow_amm_registry.amms()
   d. fetch_in_flight_orders(block)
4. In-flight orderlari cikar
5. balance kontrolu:
   - EIP-1271 -> skip (pre-interaction)
   - Flashloan (receiver=settlement) -> skip
   - Partial + balance >= 1 -> gec
   - Normal: balance >= sell + fee
6. Dust order filtresi (partial, remaining sell/buy = 0)
7. Native price filtresi (her iki token icin fiyat olmali)
8. CoW AMM in-flight kontrolu (owner adresine gore)
9. protocol_fees.apply() -> domain::Order
10. Cache guncelle
```

**`get_solvable_orders()` (satir 415-448):**
- Cache bos: `all_solvable_orders(min_valid_to)` — full fetch
- Cache dolu: `solvable_orders_after(...)` — incremental
- Checkpoint 60s geri alinir: race condition mitigation

**`orders_with_balance()` (satir 579-632):**
- Yeni orderlar once (Reverse(creation_date) sort)
- EIP-1271 bypass, flashloan bypass, wrapper bypass
- `balance >= sell_amount + fee_amount` kontrolu

**`filter_dust_orders()` (satir 636-677):**
- Sadece partially_fillable'a uygulanir
- `remaining(sell) == 0 || remaining(buy) == 0` -> dust

**Filtreleme Sebepleri:**
BannedUser | DustOrder | InFlight | InsufficientBalance | InvalidSignature | MissingNativePrice | UnsupportedToken

### Guvenlik Analizi

**Vektor: Double-Spend**
- in_flight orders filtresi: ayni order iki auction'da olamaz
- CoW AMM in-flight: owner adresine gore amm bazli kontrol

**Vektor: Dust Order DoS**
- filter_dust_orders: remaining = 0 -> filtrele

**Vektor: Balance Exhaustion**
- Newest-first sira: yeni orderlar balance'tan oncelikli
- Eski orderlar sikilir

**Vektor: Native Price Oracle**
- Tek estimator -> merkezi bagimlilik
- Timeout: fiyat gelmezse order filtreler
- WETH hardcoded = 1 -> manipulasyon imkansiz

**DEX Uygulama Notu — Nova Shield icin:**
- Filtreleme pipeline'i Nova Shield DEX icin DOGRUDAN uygulanabilir
- `min_order_validity_period`: minimum N blok gecerlilik (front-running koruma)
- Balance check sorting: newest-first -> yeni kullanicilar oncelikli
- `wrapper_cache` LRU 20_000: app_data isleme caching pattern
- CoW AMM in-flight: yield vault rebalance siparisler icin ayni pattern
- IMPROVEMENT: Balance check icin merkle proof (RPC guven sorunu)

---

## 13. maintenance.rs (315 satir)

**`MaintenanceSync`:** two-channel sync: `partially_processed_block` + `fully_processed_block`
**`SyncTarget`:** PartiallyProcessed(block) | FullyProcessed(block)

**`Maintenance::spawn_maintenance_task()` (satir 126-155):**
```
watch channel olustur
tokio::spawn:
  loop:
    block = stream.next()
    index_until_block(block, partial_tx, full_tx)
```

**`index_until_block()` (satir 157-199):**
```
1. run_essential_maintenance() PARALEL:
   - settlement_indexer
   - cow_amm_indexer'lar
   - ethflow_order_indexer'lar
   Hata -> log warn, GERI DON (partial_tx guncellenmez)

2. partial_tx.send(block) -> "essential done"

3. run_optional_maintenance() PARALEL:
   - db_cleanup.remove_expired_quotes()
   - ethflow_refund_indexer'lar
   - settlement_observer.post_process_outstanding...()

4. full_tx.send(block) -> "fully done"
```

**DEX Uygulama Notu — Nova Shield icin:**
- Iki seviyeli sync: essential (hizli, kritik) vs optional (async)
- RunLoop sadece essential'i bekler -> gecikme minimize
- Nova Shield: "essential" = order state indexing; "optional" = analytics, rewards

---

## 14. domain/settlement/observer.rs (160 satir)

**`Observer`:** eth + persistence

**`post_process_outstanding_settlement_transactions()` (satir 45-80):**
```
unprocessed settlements -> MAX_CONCURRENCY=10 paralel
  retry_with_sleep(|| post_process_settlement(settlement))
```

**`fetch_auction_data_for_transaction()` (satir 95-159):**
```
eth.transaction(tx) -> calldata
Transaction::try_new() -> decode
Settlement::new(decoded, persistence, chain)

Hatalar:
  MissingCalldata -> error log (SC bug)
  MissingAuctionId/Decoding/Signature/Solver -> warn, Ok(None)
  Authentication -> Err (RETRY)
  Infra -> Err (RETRY)
  WrongEnvironment -> Ok(None)
```

**DEX Uygulama Notu:**
- MAX_CONCURRENCY=10: ayni blokta cok settlement olabilir
- Authentication retry: node latency toleransi
- WrongEnvironment: graceful ignore

---

## 15. shadow.rs (301 satir)

**Amac:** Shadow mod — gercek settlement YAPMADAN ayni algoritmalar.

**`RunLoop` (satir 35-46):** upstream orderbook'tan auction ceker.

**`run_forever()` (satir 78-97):**
```
loop:
  next_block bekle
  next_auction() -> upstream API'dan auction al
  single_run(auction) — SOLVE et ama SETTLE ETME
```

**`single_run()` (satir 129-180):**
```
competition(auction) -> Vec<Bid<Unscored>>
winner_selection.arbitrate() -> ranking
total_score = winners toplami

for bid in ranking.ranked():
  reward = total_score - reference_score
  tracing::info!(driver, reference_score, reward, is_winner)
  Metrics::performance_rewards[driver] += reward
  Metrics::wins[driver] += is_winner
```

**`participate()` (satir 205-258):**
- /solve + /reveal (ama /settle YOK)
- calldata.ends_with(auction_id.to_be_bytes()) kontrolu

**DEX Uygulama Notu — Nova Shield icin:**
- Shadow mod: yeni solver test etmek icin altin degerinde
- `performance_rewards` metric: gercek para harcamadan performance ol
- `/reveal` calldata check: solver compliance test
- Nova Shield: yeni consensus kurallari shadow'da test et, sonra mainnet

---

## 16. event_updater.rs (81 satir)

**`EventUpdater<DB, W>`:** Generic event indexing wrapper
- `DB: EventStoring<W::Event>`
- `W: EventRetrieving`
- `Maintaining::run_maintenance()` impl

`new_skip_blocks_before()`: Mevcut DB state'den devam (genesis'ten degil).

**DEX Uygulama Notu:**
- Generic pattern: Settlement/EthFlow/CoW AMM -> hepsi ayni wrapper
- Nova Shield: Her kontrakt icin `EventUpdater<DB, Contract>` olustur

---

## 17. leader_lock_tracker.rs (130 satir)

**`LeaderLockTracker`:** Disabled | Enabled { is_leader, was_leader, leader_lock }

**Mekanizma:**
```
try_acquire() -> Postgres advisory lock (non-blocking)
  true: leader
  false: follower (baskasi tutuyor)

is_leader():
  Disabled -> always true
  Enabled -> is_leader field
```

**just_stepped_up():** `is_leader=true AND was_leader=false` -> yeni leader

**DEX Uygulama Notu — Nova Shield icin:**
- Zero-downtime deployment pattern
- Follower cache'i warm tutar -> leader fail -> hizli devralma
- Nova Shield: etcd distributed lock alternatif

---

## 18. shutdown_controller.rs (68 satir)

**`ShutdownController`:** `oneshot::Receiver<()>`
- `new_shutdown_on_signal()`: SIGTERM + SIGINT (Unix)
- `new_manual_shutdown()`: test icin
- `should_shutdown()`: non-blocking try_recv

**Unix:**
```rust
tokio::select! {
    _ = ctrl_c => { ... },
    _ = sigterm.recv() => { ... },
}
```

**DEX Uygulama Notu:**
- Non-blocking shutdown check her loop iterasyonunda
- Leader lock shutdown sirasinda release() cagrisi
- Nova Shield validator: shutdown = consensus participation durdur

---

## 19. periodic_db_cleanup.rs (179 satir)

**`OrderEventsCleanerConfig`:**
- `cleanup_interval: Duration`
- `event_age_threshold: chrono::Duration`

**`run_forever()` loop:**
```
interval.tick()
threshold = now - event_age_threshold
db.delete_order_events_before(threshold)
Metrics::order_events_cleanup_total.inc()
```

**Test senaryosu (satir 86-178):**
- 3 event: 300ms, 100ms, 0ms yas
- cleanup_interval=50ms, threshold=200ms
- 20ms: event_a (300ms) silindi
- 50ms: b ve c genç, korundu
- 150ms: event_b (250ms+) silindi
- 250ms: event_c silindi

**DEX Uygulama Notu:**
- Order events tablosu hizli buyuyebilir (12s'de bir auction)
- Threshold: GDPR/compliance gereksinimi ile uyumlu
- Nova Shield: settlement analytics 90 gun retention

---

## 20. infra/solvers/mod.rs (187 satir)

**`Driver` (satir 21-26):**
```rust
pub struct Driver {
    pub name: String,
    pub url: Url,
    pub submission_address: eth::Address,
    client: Client,  // timeout=60s, tcp_keepalive=60s
}
```

**`try_new()` (satir 37-69):**
- `Account::Kms(key_id)` -> AWS KMS (production)
- `Account::Address(address)` -> direkt (dev)

**`solve()`, `reveal()`, `settle()`:**
- HTTP POST, JSON body
- X-REQUEST-ID: auction_id header (distributed tracing)
- Response size limit: 10MB

**`request_response()` (satir 112-148):**
- Streaming body (Measured metric wrapper)
- non-200 -> anyhow error

**DEX Uygulama Notu — Nova Shield icin:**
- KMS signer: MPC signing key in AWS KMS
- TCP keepalive: uzun yasli driver baglantilari
- Streaming body: buyuk auction request (3-4MB brotli)
- IMPROVEMENT: gRPC/WebSocket -> dusuk latency icin

---

## 21. infra/solvers/dto/solve.rs (371 satir)

**`Request` (satir 35-40):**
```rust
pub struct Request {
    auction_id: i64,
    body: bytes::Bytes,   // pre-serialized JSON
    content_encoding: Option<HeaderValue>,  // "br" veya None
    deadline: DateTime<Utc>,
}
```

**Brotli Sikistirma (satir 79-113):**
```
quality=1: en hizli (gzip-3'ten daha iyi ratio+speed)
lgwin=22: LZ77 window 4MB (auction boyutuna uygun ~3-4MB)
buffer=4096: I/O buffer
Fallback: sikistirma hatasinda sikisilmamis gonder
```

**`RequestHelper` JSON ciktisi:**
```json
{
  "id": "1",
  "tokens": [{"address": "0x...", "price": "1000", "trusted": true}],
  "orders": [...],
  "deadline": "2026-07-22T12:00:00Z",
  "surplusCapturingJitOrderOwners": [...]
}
```

**`Solution` response:**
```rust
pub struct Solution {
    pub solution_id: u64,
    pub submission_address: Address,
    pub orders: HashMap<OrderUid, TradedOrder>,
    pub clearing_prices: HashMap<Address, U256>,  // DEPRECATED
}
```

**HTTP inject (satir 135-163):**
- `X-Auction-Id`: erken protokol tespiti
- Streaming body ile Measured wrapper (metrik)
- content-encoding: br (sikistirilmissa)

**DEX Uygulama Notu — Nova Shield icin:**
- Pre-serialization + Bytes clone: CPU cost tek kez, N driver'a paylas
- X-Auction-Id: driver duplicate detection
- `clearing_prices` deprecated: Nova Shield'tan atlayabilirsin
- IMPROVEMENT: Protobuf binary (JSON'dan 5-10x kucuk)

---

## GENEL MIMARI OZETI

```
RunLoop (blok bazli, 12s ETH)
  |
  +-- wake_notify <-- new block | new order
  |
  +-- update_caches()
  |     +-- maintenance.wait_until_block_processed(Partial)
  |     +-- solvable_orders_cache.update()
  |           +-- DB: incremental/full fetch
  |           +-- PARALEL: balances + filter_invalid + cow_amms + in_flight
  |           +-- filter_out_in_flight
  |           +-- orders_with_balance
  |           +-- filter_dust
  |           +-- get_orders_with_native_prices
  |           +-- protocol_fees.apply() -> domain::Order
  |
  +-- cut_auction()
  |     +-- get_next_auction_id()
  |     +-- replace_current_auction_in_db()
  |     +-- upload_auction_to_s3()
  |
  +-- single_run()
        +-- fetch_solutions()  [PARALEL tum driverlar]
        |     +-- pick_solve_deadline()  [PoS slot sync]
        |     +-- solve::Request::new()  [brotli compress]
        |     +-- isSolver()  [onchain whitelist]
        |     +-- shuffle (tie-breaking)
        |
        +-- winner_selection.arbitrate()
        |     +-- fairness filter
        |     +-- winner marking
        |     +-- reference_scores
        |
        +-- post_processing() -> DB persist
        |
        +-- start_settlement_execution() [tokio::spawn]
              +-- driver.settle() RACE wait_for_settlement_tx()
              +-- store_execution_started/ended
```

---

## DEX UYGULAMASI — NOVA SHIELD ICIN TAM EYLEM PLANI

### Oncelik 1 — Kritik Yol (Ilk Sprint)

**A. RunLoop Timing (Nova Shield 2s blok):**
```rust
SlotConfig {
    slot_length: Duration::from_secs(2),
    tx_propagation_latency: Duration::from_millis(300),
}
// min_solve_time = 1s (2s blok -> 1s solve -> 0.7s buffer)
// submission_deadline = 3 blok
```

**B. Surplus Formulleri (math.rs dogrudan kopyalanabilir):**
```rust
// Sell order surplus:
let limit_buy = executed_sell.ceil_div(limit_sell) * limit_buy;
let bought = executed_sell.ceil_div(clearing_buy) * clearing_sell;
let surplus = bought - limit_buy;
let surplus_eth = surplus * native_price / 10_u256.pow(18);

// Fee formuller (faktor donusumu):
let surplus_fee = surplus * factor / (1.0 - factor);
let vol_fee_sell = bought_after * factor / (1.0 - factor);
let vol_fee_buy = sold_after * factor / (1.0 + factor);

// Partner fee compound cap:
let remaining = (1 + cap) / (1 + accumulated) - 1;
```

**C. Fee Modeli:**
- Surplus fee: off-chain hesap, on-chain enforce
- Volume fee: token bucket per-pair override
- Partner fee: compound cap formulu
- Market order: fee yok; Limit order: fee uygulanir

### Oncelik 2 — Kazanan Secimi

**D. Winner Selection:**
`winner_selection` crate'ini dependency ekle veya port et:
- Directional clearing prices: AMM pool uyumlu
- Fairness filter: user MEV korumasi
- Reference scores: solver odulleri

**E. Multi-Winner:**
- Cakismayan order gruplari paralel settle edilebilir
- Nova Shield DEX: farkli token ciftleri ayni auction'da

### Oncelik 3 — Guvenlik Iyilestirmeleri

**F. VRF Tie-Breaking:**
```rust
// shuffle() yerine:
let permutation = vrf::random_permutation(seed, bids.len());
bids = permutation.iter().map(|i| bids[*i].clone()).collect();
```

**G. Fixed-Point Fee Arithmetic:**
```rust
// f64 yerine:
const FACTOR_DENOM: u256 = 1_000_000;
let factor_num = (factor * FACTOR_DENOM as f64) as u256;
let fee = surplus * factor_num / (FACTOR_DENOM - factor_num);
```

**H. ZK Score Proof:**
- winner_selection sonucunu ZK-SNARK ile prove et
- Solver'lar score'larini on-chain kanitleyabilir

### Oncelik 4 — Infra

**I. Brotli Compression:**
```rust
// quality=1, lgwin=22 (test edilmis en iyi parametre)
let mut encoder = CompressorWriter::new(Vec::new(), 4096, 1, 22);
```

**J. Leader Lock:**
- PostgreSQL advisory lock -> zero-downtime deployment
- `SELECT pg_try_advisory_lock(LOCK_ID)` — non-blocking

**K. Shadow Mode:**
- Yeni solver'lari once shadow'da test et
- `performance_rewards` metric ile gercek data olmadan performance ol

**L. Settlement Observer:**
- MAX_CONCURRENCY=10 paralel post-processing
- Authentication hatasi retry (node latency toleransi)
- WrongEnvironment graceful ignore

---

## SONUC VE KALITE DEGERLENDIRMESI

CoW Protocol autopilot kodu **production-grade** kalitede.

**Guclu noktalar:**
1. Type-state pattern: compile-time state machine (sifir runtime cost)
2. Paralel execution: join_all ile tum driverlar ayni anda
3. Fairness guarantee: matematiksel olarak kanitlanmis (test suite)
4. Incremental DB query: solvable_orders checkpoint ile minimal DB yuk
5. PoS slot sync: blok zamanlamasina gore deadline optimize
6. Brotli compression: buyuk auction payloads icin otomatik
7. Leader lock: zero-downtime deploy destegi
8. Reference score: ekonomik olarak saglamm solver odulleri
9. Shadow mode: production risk almadan algoritma test

**Dogrulanan kritik algoritmalar:**
- Surplus hesabi: ceil division SC ile tutarli (test onaylandi)
- Fee compound formula: test case'lerle satir satir dogrulandi
- PoS slot deadline: unit test ile 3 senaryo dogrulandi
- Fairness filter: 6 farkli senaryo test edildi
- Reference score: single, compatible, incompatible, reference>winner

**Nova Shield icin en kritik fark:**
CoW Protocol 12s ETH blogu -> Nova Shield 2s blok hedefi.
`min_solve_time`, `tx_propagation_latency`, `submission_deadline` yeniden kalibre edilmeli.
2s blok = solver'in sadece ~1s suresinin oldugu senaryo -> solver algoritmalari da optimize edilmeli.
