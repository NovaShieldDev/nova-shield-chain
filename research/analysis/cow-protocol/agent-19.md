# CoW Protocol Analiz — Agent 19
## Kapsam: solvers/src/ — domain/ + boundary/ TAM DERİNLİK
## DEX Özel Protokolü: 3x Hassasiyet

**Analiz Tarihi:** 2026-07-22
**Toplam Dosya Sayısı:** 78 dosya
**Bu Agent Odak Noktaları:** domain/, boundary/, infra/dex/, infra/config/

---

## 1. GENEL MİMARİ — Solver Katmanı

CoW Protocol solver mimarisi iki ana solver tipi içeriyor:

```
                  ┌──────────────────────────────┐
                  │        Auction Input          │
                  │  orders + liquidity + prices  │
                  └──────────────┬───────────────┘
                                 │
              ┌──────────────────┴──────────────────┐
              │                                     │
     ┌────────▼────────┐                  ┌────────▼────────┐
     │ Baseline Solver  │                  │   DEX Solver     │
     │ (onchain AMMs)   │                  │ (external APIs)  │
     │                  │                  │                  │
     │ UniswapV2/V3     │                  │ OKX / Bitget     │
     │ BalancerV2       │                  │                  │
     │ CurveStable      │                  │                  │
     │ LimitOrders      │                  │                  │
     └──────────────────┘                  └──────────────────┘
```

---

## 2. DOMAIN KATMANI — TÜMÜ

### [domain/auction.rs] (105 satır)

**Amaç:** Solver'a verilen auction'ı temsil eden alan nesnesi.

**Struct/Enum Listesi:**

- `Auction` (satır 12): Ana auction struct
  - `id: Id` — auction kimliği
  - `tokens: Tokens` — token metadata (fiyatlar, bakiyeler, güvenilirlik)
  - `orders: Vec<order::Order>` — çözülmesi gereken emirler
  - `liquidity: Vec<liquidity::Liquidity>` — kullanılabilir likidite kaynakları
  - `gas_price: GasPrice` — anlık gas fiyatı tahmini (ETH cinsinden)
  - `deadline: Deadline` — solver'ın yanıt vermesi gereken son an

- `Tokens` (satır 22): `HashMap<eth::TokenAddress, Token>` wrapper
  - `get()`: token metadata döndürür
  - `reference_price()`: token'ın native token cinsinden referans fiyatını döndürür

- `Id` (satır 36): Auction kimliği
  - `Solve(i64)` — gerçek bir onchain settlement auction'ı
  - `Quote` — sadece fiyat teklifi hesaplaması (onchain işlem yok)

- `Token` (satır 56): Bireysel token metadata
  - `decimals: Option<u8>` — token hassasiyeti
  - `symbol: Option<String>` — token sembolü
  - `reference_price: Option<Price>` — ETH cinsinden fiyat
  - `available_balance: eth::U256` — settlement kontratının token bakiyesi
  - `trusted: bool` — internalizasyon için güvenilir mi?

- `Price` (satır 67): Token fiyatı (wei cinsinden)
  - **Matematiksel Tanım:** `Price = wei_amount / 10^18` yani "1e18 birim başına kaç wei ETH gerekli"
  - `ether_value(eth)` = `eth * 10^18 / self.0.0` — ETH miktarını token miktarına çevirir

- `GasPrice` (satır 85): `eth::Ether` wrapper — settlement işlemi için gas fiyatı

- `Deadline` (satır 89):
  - `remaining()` — kalan süreyi döndürür, sıfır veya negatifse `None`
  - `reduce(duration)` — deadline'ı azaltır (DEADLINE_SLACK uygulaması için)

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield DEX solver'ı aynı Auction yapısını kullanabilir
- `trusted` flag'i kritik: sadece güvenilen tokenlar internalize edilir (settlement buffer optimization)
- `Price` formatı: "10^18 birim token için kaç wei ETH" — bu normalize fiyat formatı DEX hesaplamalarında yaygın

---

### [domain/order.rs] (162 satır)

**Amaç:** CoW Protocol emrini temsil eden alan nesnesi.

**Struct/Enum Listesi:**

- `Order` (satır 12): Ana emir struct
  - `uid: Uid` — 56 byte emir kimliği
  - `sell: eth::Asset` — satılacak token + miktar
  - `buy: eth::Asset` — alınacak token + miktar
  - `side: Side` — Buy (sabit alım miktarı) veya Sell (sabit satım miktarı)
  - `class: Class` — Market (piyasa emri) veya Limit (limit emri)
  - `partially_fillable: bool` — kısmi dolum izni var mı?
  - `flashloan_hint: Option<FlashloanHint>` — flashloan önerisi
  - `wrappers: Vec<WrapperCall>` — ek wrapper çağrıları

- `Uid` (satır 39): `[u8; 56]` — 56 byte emir kimliği
  - Yapı: `[order_hash (32 bytes) | owner_address (20 bytes) | valid_to (4 bytes)]`
  - `owner()` (satır 30): UID'den sahip adresini çıkarır (`bytes[32..52]`)

- `Side` (satır 57):
  - `Buy` — sabit alım miktarı, maksimum satım miktarı
  - `Sell` — sabit satım miktarı, minimum alım miktarı

- `Class` (satır 66):
  - `Market` — piyasa emri (protokol fee'si)
  - `Limit` — limit emri (solver-determined fee)

- `JitOrder` (satır 75): Just-In-Time likidite emri
  - Solverlar tarafından özel market maker entegrasyonu için kullanılır
  - `owner, signature, sell, buy, side, class, partially_fillable, valid_to, app_data, receiver`

- `Signature` (satır 92): İmza varyantları
  - `Eip712` — standart EIP-712 tip bazlı imza
  - `EthSign` — EIP-191 personal_sign formatı
  - `Eip1271` — akıllı kontrat imza doğrulama (Vec<u8> arbitrary data)
  - `PreSign` — onchain işlemle yapılan ön imzalama

- `FlashloanHint` (satır 149):
  - `liquidity_provider` — flashloan sağlayıcı kontrat adresi
  - `protocol_adapter` — protokol adaptörü adresi
  - `receiver, token, amount` — flashloan detayları

- `WrapperCall` (satır 158):
  - `address: eth::Address` — hedef kontrat
  - `data: Vec<u8>` — çağrı verisi

**Güvenlik Analizi:**
- UID'den owner adresi çıkarma işlemi doğrudan byte slice'dan yapılıyor — herhangi bir doğrulama yok, ama EIP-712 hash zaten bunu koruyuyor
- Signature varyantları: her biri farklı doğrulama mekanizması gerektiriyor

**DEX Uygulama Notu — Nova Shield:**
- 4 imza türü desteklemek zorunlu: EOA'lar için EIP-712 ve EthSign, akıllı kontrat cüzdanlar için EIP-1271, onchain-signing için PreSign
- UID yapısı öğrenilmeli: 32 byte hash + 20 byte owner + 4 byte valid_to = 56 byte kompakt format
- JIT order mekanizması: solver'ların kendi private market makerlarını enjekte etmesini sağlıyor — Nova Shield için valuable

---

### [domain/solution.rs] (486 satır)

**Amaç:** Solver'ın ürettiği çözümü temsil eden alan nesnesi.

**Kritik Struct/Enum Listesi:**

- `Solution` (satır 19): Auction'a verilen tam çözüm
  - `id: Id`
  - `prices: ClearingPrices` — uniform clearing fiyatları
  - `trades: Vec<Trade>` — gerçekleştirilen işlemler
  - `pre_interactions, interactions, post_interactions: Vec<Interaction>` — onchain çağrılar
  - `gas: Option<eth::Gas>`
  - `wrappers: Vec<WrapperCall>`

- `Single` (satır 110): Tek bir emir için çözüm
  - `order, input, output, interactions, gas, wrappers`
  - `into_solution(fee)` (satır 128): Single'ı tam Solution'a dönüştürür
  - `into_dex_solution(gas_price, sell_token, gas_offset)` (satır 206): DEX swap için dönüşüm

**into_solution() — Kritik Algoritma (satır 128-202):**

```
Giriş: Single { order, input, output, interactions, gas }, fee: SellTokenAmount

1. Token eşleşme doğrulama: (order.sell.token, order.buy.token) == (input.token, output.token)
2. Fee hesaplama:
   - Limit emri: Fee::Surplus(solver_fee)
   - Market emri: Fee::Protocol
3. surplus_fee = fee.surplus().unwrap_or_default()
4. (sell, buy) hesaplama — SIDE'a göre:
   - Buy emri:
     sell = input.amount + surplus_fee  (checked_add → None = overflow)
     buy  = output.amount
   - Sell emri:
     sell = min(input.amount + surplus_fee, order.sell.amount)  [kontrat garantisi]
     buy  = (sell - surplus_fee) * output.amount / input.amount  [orantılı hesaplama]
5. Limit fiyat kontrolü:
   order.sell.amount * buy >= order.buy.amount * sell  →  None dönerse geçersiz
6. executed miktarı:
   - Buy: buy
   - Sell: sell - surplus_fee
7. ClearingPrices:
   sell_token → buy (buy token üzerinden fiyat)
   buy_token  → sell - surplus_fee (sell token üzerinden fiyat)
```

**Matematiksel Formüller:**

**Limit Fiyat Kontrolü:**
```
sell.amount * buy >= buy.amount * sell
```
Bu çapraz çarpım eşitsizliği, işlemin limit fiyatı ihlal etmediğini garanti eder:
```
buy / sell >= buy.amount / sell.amount
```
Yani: elde edilen alım/satım oranı >= emrin talep ettiği minimum oran.

**Sell order'da buy miktarı hesaplama:**
```
buy = (sell - surplus_fee) * output.amount / input.amount
```
Bu: DEX'in verdiği oranı kullanarak fee düşülmüş net satış miktarına göre alım miktarı hesaplar.

**into_dex_solution() — DEX Fee Hesabı (satır 206-302):**
```
fee = sell_token_price.ether_value(
    (swap_gas + gas_offset) * gas_price
)
```
Bu: gas maliyetini ETH cinsinden hesaplayıp sell token'a çevirir.

**Sell order'da checked_ceil_div (satır 267):**
DEX solvers için buy miktarı ceiling division ile hesaplanır — yani seller aleyhine değil, satıcı lehine yuvarlama.

- `ClearingPrices` (satır 308): `HashMap<eth::TokenAddress, U256>`
  - Uniform fiyatlar: her settlement'ta tüm trader'lar aynı fiyatı alır

- `Trade` (satır 319): İki varyant
  - `Fulfillment(Fulfillment)` — mevcut emir doldurma
  - `Jit(JitTrade)` — JIT likidite emri

- `Fulfillment` (satır 326):
  - `new(order, executed, fee)` (satır 335):
    - `fee` tipi `order.solver_determines_fee()` ile eşleşmeli
    - Kısmi dolum: `executed <= fill` kontrolü
    - Tam dolum: `executed == fill` kontrolü

- `Fee` (satır 399):
  - `Protocol` — order'ın `fee_amount` alanından
  - `Surplus(eth::SellTokenAmount)` — solver'ın belirlediği ek fee

- `Interaction` (satır 432):
  - `Liquidity(Box<LiquidityInteraction>)` — AMM swap interaction
  - `Custom(CustomInteraction)` — arbitrary calldata

- `LiquidityInteraction` (satır 440):
  - `liquidity, input, output` — AMM bilgileri
  - `internalize: bool` — settlement buffer optimizasyonu aktif mi?

- `CustomInteraction` (satır 452):
  - `target, value, calldata` — standart EVM call
  - `internalize: bool`
  - `inputs, outputs: Vec<eth::Asset>` — internalizasyon doğrulaması için
  - `allowances: Vec<Allowance>` — gerekli token izinleri

**with_buffers_internalizations() Algoritması (satır 50-106):**

```
1. Her interaction için:
   a. input/output asset'leri al
   b. tüm input'lar "trusted" tokenlardan mı? → değilse atla
   c. output için yeterli settlement buffer bakiyesi var mı?
      total_required = sum(outputs) + previously_used_buffers[token]
      total_required <= tokens[token].available_balance  → geçerliyse internalize et
   d. used_buffers güncelle
2. internalize = true ise: interaction onchain çağrılmaz
   settlement kontratının mevcut bakiyesinden karşılanır
```

**Gas Sabitleri (satır 478-486):**
```rust
INITIALIZATION_COST: u64 = 32_000  // settle() fonksiyon çağrısı başlangıç gaz
SETTLEMENT: u64 = 7_365            // isSolver() kontrolü gaz
ERC20_TRANSFER: u64 = 27_513       // ERC20 transfer için median gaz (Oct 2021 verisi, 52. persentil)
```

**DEX Uygulama Notu — Nova Shield:**
- Uniform clearing prices kritik: tüm emirler aynı fiyattan işlem görür — MEV koruma mekanizması
- Buffer internalizasyonu: settlement kontratının token bakiyesini kullanarak gas tasarrufu
  - Trusted token listesi tutulmalı
  - Available balance tracking gerekli
- DEX solver fee hesabı: `fee = gas_cost_ETH / sell_token_price` — bu formül Nova Shield'de aynı şekilde kullanılabilir

---

### [domain/liquidity/mod.rs] (80 satır)

**Amaç:** Onchain likidite kaynaklarının modellenmesi.

**Struct/Enum Listesi:**

- `Liquidity` (satır 13): Bir likidite kaynağı
  - `id: Id` — benzersiz tanımlayıcı (String wrapper)
  - `address: eth::Address` — pool kontrat adresi
  - `gas: eth::Gas` — bu kaynağı kullanmanın tahmini gaz maliyeti
  - `state: State` — pool tipi ve durum verisi

- `State` (satır 26): Likidite tipi
  - `ConstantProduct(constant_product::Pool)` — UniswapV2 tipi
  - `WeightedProduct(weighted_product::Pool)` — BalancerV2 weighted
  - `Stable(stable::Pool)` — Curve/BalancerV2 stable
  - `Concentrated(concentrated::Pool)` — UniswapV3 tipi
  - `LimitOrder(limit_order::LimitOrder)` — 0x limit order

- `TokenPair` (satır 36): Sıralı token çifti
  - `new(a, b)` — küçük adres önce, büyük adres sonra sıralama
  - Bu deterministik sıralama pool arama için hash map key'i

- `ScalingFactor` (satır 56): Balancer token scaling
  - `new(value)` — sıfır değer yasak
  - Balancer stable/weighted pool'larda token miktarlarını normalize etmek için kullanılır

---

### [domain/liquidity/constant_product.rs] (62 satır)

**Amaç:** UniswapV2 tarzı constant product pool modeli.

**Struct Listesi:**

- `Pool` (satır 11):
  - `reserves: Reserves` — token bakiyeleri
  - `fee: eth::Rational` — işlem ücreti (ör. 3/1000 = 0.3%)
  - `k()` (satır 25): `reserves.0 * reserves.1` — AMM invariantı

- `Reserves` (satır 36):
  - `new(a, b)` — maksimum değer kontrolü: her ikisi de `u112::MAX` altında olmalı (UniswapV2 sınırı)
  - Sıralama: küçük token adresi önce

**Matematiksel Formüller:**

**Constant Product Invariant:**
```
k = x * y = sabit
```
Burada x ve y pool'daki token miktarları.

**get_amount_out (boundary tarafında implement):**
```
out = (y * dx * (1 - fee)) / (x + dx * (1 - fee))
   = (reserve_out * amount_in * (fee_denom - fee_numer)) /
     (reserve_in * fee_denom + amount_in * (fee_denom - fee_numer))
```

**U112 Sınırı:**
```rust
let max = U256::from(2_u128.pow(112) - 1);  // 5192296858534827628530496329220095
```
Bu UniswapV2'nin Solidity kontratındaki `uint112` rezerv sınırından geliyor.

**DEX Uygulama Notu — Nova Shield:**
- UniswapV2-clone pool'lar için aynı invariant formülü kullanılabilir
- u112 sınırı: UniswapV2-compatible pool'lar için zorunlu güvenlik sınırı
- k = x*y formülü en basit AMM; ancak büyük emirler için yüksek slippage

---

### [domain/liquidity/weighted_product.rs] (73 satır)

**Amaç:** BalancerV2 weighted pool modeli.

**Struct Listesi:**

- `Pool` (satır 8):
  - `reserves: Reserves` — token bakiyeleri + ağırlıklar
  - `fee: eth::Rational`
  - `version: Version` — V0 veya V3Plus (farklı yuvarlama davranışı)

- `Reserves` (satır 16):
  - Sıralama: token adresine göre ascending — BalancerV2 kontrat sıralamasıyla eşleşmek için
  - Duplicate kontrolü ile yeni oluşturma
  - `token_pairs()` — tüm ikili kombinasyonları döndürür (Itertools::tuple_combinations)

- `Reserve` (satır 55):
  - `asset: eth::Asset` — token + miktar
  - `weight: eth::Rational` — Balancer ağırlığı (ör. 50/100, 80/100)
  - `scale: liquidity::ScalingFactor` — decimal normalizasyonu

- `Version` (satır 64):
  - `V0` — orijinal BalancerV2 weighted math
  - `V3Plus` — 50/50 ve 20/80 pool'lar için exponentiation kısayolu

**Matematiksel Formüller:**

**Balancer Weighted Product Invariant:**
```
W = ∏(balance_i^weight_i) = sabit
```

**get_amount_out:**
```
amount_out = balance_out * (1 - (balance_in / (balance_in + amount_in * (1-fee)))^(weight_in/weight_out))
```

Bu formül `x^k` exponentiation içeriyor — float precision hassasiyeti kritik.

**DEX Uygulama Notu — Nova Shield:**
- Weighted pools: farklı ağırlıklarda likidite sağlama imkanı (80/20 ETH/BTC gibi)
- `scale` factörü: farklı decimal sayıları olan tokenları normalize etmek için zorunlu
- V3Plus'da 50/50 için kısayol: `sqrt(x*y)` — gas optimization

---

### [domain/liquidity/stable.rs] (60 satır)

**Amaç:** Curve tarzı stable pool (BalancerV2 stable dahil).

**Struct Listesi:**

- `Pool` (satır 8):
  - `reserves: Reserves`
  - `amplification_parameter: eth::Rational` — A parametresi
  - `fee: eth::Rational`

- `Reserve` (satır 55): Token bakiyesi + `scale: ScalingFactor`

**Matematiksel Formüller:**

**StableSwap (Curve) Invariant:**
```
A * n^n * ∑x_i + D = A * D * n^n + D^(n+1) / (n^n * ∏x_i)
```
Burada:
- `A` = amplification parameter (piyasa koşullarına göre ayarlanır, genellikle 100-2000)
- `n` = token sayısı
- `D` = pool'un toplam "invariant değeri"
- `x_i` = i. token miktarı (scaled)

**Amplification Etkisi:**
- A = 0: pure constant product (x*y=k)
- A → ∞: pure constant sum (x+y=k)
- Stablecoins için yüksek A: düşük slippage benzer değerli tokenlar arasında

**DEX Uygulama Notu — Nova Shield:**
- stablecoin swap'ları için StableSwap matematiki zorunlu
- `amplification_parameter` dinamik olabilir — Curve'de admin değiştirebilir
- `scale` faktörü: USDC (6 decimal) + DAI (18 decimal) gibi farklı decimal tokenları normalize eder

---

### [domain/liquidity/concentrated.rs] (15 satır)

**Amaç:** UniswapV3 concentrated liquidity pool modeli.

- `Pool` (satır 5): Sadece `tokens: liquidity::TokenPair` ve `fee: Fee`
- `Fee` (satır 13): `u32` — fee tier (5, 30, 100 bps)

**Not:** Bu minimal domain modeli. Gerçek hesaplama boundary katmanında `UniswapV3QuoterV2` kontratına RPC çağrısı ile yapılıyor — yani domain katmanı sadece metadata tutuyor.

---

### [domain/liquidity/limit_order.rs] (14 satır)

**Amaç:** 0x tarzı limit order (CoW order ile kullanılabilir likidite).

- `LimitOrder`: `maker: eth::Asset, taker: eth::Asset, fee: TakerAmount`
- `TakerAmount`: taker tokenında denomine fee miktarı

**Matematiksel Formüller:**

**get_amount_out** (boundary'de implement):
```
out = in_amount * maker.amount / taker.amount
```
(Basit orantılı hesaplama — limit fiyat)

**get_amount_in:**
```
in = out_amount * taker.amount / maker.amount
```

**DEX Uygulama Notu — Nova Shield:**
- Limit order'lar: private market maker'lardan gelen JIT likidite sağlar
- maker/taker yapısı: piyasada standart terminoloji — Nova Shield DEX'te aynı kalabilir

---

### [domain/dex/mod.rs] (205 satır)

**Amaç:** Harici DEX API entegrasyonu için domain modelleri.

**Struct/Enum Listesi:**

- `Order` (satır 23): DEX API için basitleştirilmiş emir
  - `sell, buy: eth::TokenAddress`
  - `side: order::Side`
  - `amount: Amount` — side'a göre ilgili miktar (sell side için sell amount, buy side için buy amount)
  - `owner: eth::Address`
  - `new(order)` (satır 32): CoW order'ı DEX order'a dönüştürür
  - `amount()` (satır 46): Asset olarak döndürür (token + miktar)

- `Call` (satır 59): Onchain swap çağrısı
  - `to: Address` — hedef kontrat
  - `calldata: Vec<u8>` — kodlanmış swap verisi

- `Swap` (satır 79): Bir DEX swap'ının tam tanımı
  - `calls: Vec<Call>` — çalıştırılacak onchain çağrılar
  - `input: eth::Asset` — beklenen giriş (slippage nedeniyle gerçek farklı olabilir)
  - `output: eth::Asset` — beklenen çıkış (slippage nedeniyle gerçek farklı olabilir)
  - `allowance: Allowance` — gerekli token izni
  - `gas: eth::Gas` — tahmini gas (heuristic — kesin değil)

**satisfies() — Kritik Kontrol (satır 162-167):**
```rust
self.output.amount.widening_mul(order.sell.amount)
    >= self.input.amount.widening_mul(order.buy.amount)
```

Bu: `output/input >= order.buy/order.sell` kontrolü — yani:
```
DEX'in verdiği oran >= emrin talep ettiği minimum oran
```
`widening_mul` kullanımı: U256 * U256 = U512 genişletilmiş çarpım — overflow güvenliği.

**satisfies_with_minimum_surplus() (satır 169-179):**
```rust
let required_buy_amount = minimum_surplus.add(order.buy.amount);
self.output.amount.widening_mul(order.sell.amount)
    >= self.input.amount.widening_mul(required_buy_amount)
```
Yani minimum surplus: `output/input >= (order.buy + surplus) / order.sell`

**into_solution() (satır 111-160):**
```
1. Limit emri için gas simülasyonu
2. swap.calls → CustomInteraction dönüşümü
3. Single { order, input, output, interactions, gas } → into_dex_solution()
```

**DEX Uygulama Notu — Nova Shield:**
- `satisfies()` kontrolü kritik — swap'ın emri karşıladığından emin olmak için
- `widening_mul` pattern: U256 çarpımlarında overflow koruması için kullanılmalı
- `allowance` mekanizması: DEX router'a settle kontratının approve yapması gerekiyor

---

### [domain/dex/slippage.rs] (269 satır)

**Amaç:** DEX swap'ları için slippage toleransı hesaplaması.

**Struct Listesi:**

- `SlippageLimits` (satır 12):
  - `relative: BigDecimal` — maksimum yüzde slippage (0-1 arası)
  - `absolute: Option<eth::Ether>` — maksimum ETH değer slippage

- `Slippage` (satır 41): Hesaplanmış relative slippage değeri

**Kritik Algoritma — relative() (satır 30-37):**
```
1. absolute slippage'ı relative'e çevir:
   absolute_as_relative = absolute_eth / (asset_amount * asset_price_in_eth)
2. Kullanılan slippage = min(self.relative, absolute_as_relative)
   → büyük emirlerde absolute cap devreye giriyor
```

**Matematiksel Formüller:**

**Absolute-to-Relative Dönüşümü (domain/dex/shared.rs):**
```
absolute_as_relative = absolute_eth / (amount_in_token * price_in_eth)
                     = absolute_eth / amount_in_eth
```

**Slippage Ekleme/Çıkarma:**
```
tolerance_amount = ceil(amount * factor)   [compute_absolute_tolerance]
add(amount) = amount + tolerance_amount    [saturating_add]
sub(amount) = amount - tolerance_amount    [saturating_sub]
```

**Test Örnekleri (doğrulanmış değerler):**
```
1 WETH, 1% relative slippage:
  min=0.99 WETH, max=1.01 WETH

100 WETH, abs cap=0.02 WETH:
  relative_effective=0.02/100=0.02% (relative_slippage=1% den küçük, abs cap devreye girer)
  min=99.98 WETH, max=100.02 WETH

1K USDC (1 USDC = 0.0000016955 ETH gibi):
  1% relative slippage → ±10 USDC
```

**as_bps():** slippage'ı basis points'e çevirir (0.01 → 100 bps)
**as_percent():** slippage'ı % formatına çevirir (0.01 → 1.0)
**round(decimals):** BigDecimal rounding

**Güvenlik Analizi:**
- `saturating_add/sub`: underflow/overflow güvenliği
- `BigDecimal` kullanımı: floating point precision hatalarını önler
- Sıfır miktar testi mevcut: sıfır miktarlı asset'te panic yok

**DEX Uygulama Notu — Nova Shield:**
- İki katmanlı slippage: relative + absolute cap — bu yaklaşım büyük emirleri korur
- Default değerler: relative=1%, absolute=0.02 ETH (yapılandırılabilir)
- Nova Shield DEX'te: kullanıcıya slippage seçimi sunulabilir, ama max cap sistemi güvenlik için zorunlu

---

### [domain/dex/minimum_surplus.rs] (142 satır)

**Amaç:** Swap'ın minimum kâr gereksinimini modeller.

**Struct Listesi:**

- `MinimumSurplusLimits` (satır 12):
  - `relative: BigDecimal` — minimum relative surplus (0+)
  - `absolute: Option<eth::Ether>` — minimum absolute surplus

**relative() Algoritması (satır 30-37):**
```
absolute_as_relative = absolute_eth / amount_in_eth
minimum_surplus = max(self.relative, absolute_as_relative)
→ küçük emirlerde absolute floor devreye giriyor
```

**Slippage ile Fark:**
- Slippage: `min(relative, absolute_as_relative)` → büyük emirler için cap
- MinimumSurplus: `max(relative, absolute_as_relative)` → küçük emirler için floor

**Test Örnekleri:**
```
0.5 WETH emri, abs=0.02 WETH, rel=1%:
  absolute_as_relative = 0.02/0.5 = 4%  > 1%
  minimum_surplus = 4%
  required_buy >= 0.5 WETH * 1.04 = 0.52 WETH

5 WETH emri:
  absolute_as_relative = 0.02/5 = 0.4%  < 1%
  minimum_surplus = 1%
  required_buy >= 5 WETH * 1.01 = 5.05 WETH
```

**DEX Uygulama Notu — Nova Shield:**
- MinimumSurplus, solver'ın kârsız swap'ları reddetmesini sağlar
- Küçük emirlerde absolute floor kritik: gas maliyetini karşılamak için
- Nova Shield DEX: solver fee mekanizması için minimum surplus threshold konfigüre edilmeli

---

### [domain/dex/shared.rs] (55 satır)

**Amaç:** Slippage ve minimum surplus için paylaşılan matematiksel fonksiyonlar.

**Kritik Fonksiyonlar:**

- `compute_absolute_tolerance(amount, factor)` (satır 15):
  ```
  numer = amount (BigUint) * factor_int (BigUint)
  denom = 10^factor_exp
  abs = ceil(numer / denom)  [div_ceil]
  ```
  `BigUint::div_ceil` kullanımı: her zaman yukarı yuvarlar — güvenli yaklaşım.

- `absolute_to_relative(absolute, asset, tokens)` (satır 28):
  ```
  price = tokens.reference_price(asset.token)
  amount_in_eth = amount * price
  absolute_as_relative = absolute / amount_in_eth
  ```
  Sıfır kontrolleri: `price.0.0.is_zero()` ve `amount_in_eth.is_zero()` → `None` döner.

---

### [domain/notification.rs] (62 satır)

**Amaç:** Driver'dan solver'a bildirimlerin modellenmesi.

**Enum Listesi:**

- `Kind` (satır 34): Bildirim türleri
  - `Timeout` — solver yanıt süresini aştı
  - `EmptySolution` — boş çözüm döndü
  - `DuplicatedSolutionId` — aynı ID'li çözüm
  - `SimulationFailed(BlockNo, Transaction, SimulationSucceededAtLeastOnce)` — simülasyon başarısız
  - `ScoringFailed(ScoreKind)` — puanlama başarısız
  - `NonBufferableTokensUsed(TokensUsed)` — internalize edilemeyen token kullanıldı
  - `SolverAccountInsufficientBalance(RequiredEther)` — yetersiz bakiye
  - `Settled(Settlement)` — settlement sonucu
  - `DriverError(String)` — driver hatası
  - `PostprocessingTimedOut` — settlement sonrası işlem timeout

- `Settlement` (satır 49):
  - `Success(TransactionHash)` — başarılı onchain işlem
  - `Revert(TransactionHash)` — revert eden işlem
  - `SimulationRevert` — simülasyonda revert
  - `Fail` — genel başarısızlık

**DEX Uygulama Notu — Nova Shield:**
- Solver'a feedback mekanizması kritik: solver kendi başarısızlıklarını öğrenerek adapt edilebilir
- `SimulationSucceededAtLeastOnce`: solver'ın daha önce başarılı olduğunu bilmek, geçici hata analizi için önemli

---

### [domain/solver/mod.rs] (boş — subdirektorye redirect)

### [domain/solver/baseline.rs] (409 satır)

**Amaç:** Baseline (onchain AMM) solver'ının ana implementasyonu.

**Struct Listesi:**

- `Solver` (satır 27): `Arc<Inner>` wrapper — thread-safe paylaşım

- `Config` (satır 33):
  - `weth: eth::WethAddress` — WETH kontrat adresi
  - `base_tokens: Vec<eth::TokenAddress>` — ara hop tokenları (ör. USDC, DAI)
  - `max_hops: usize` — maksimum ara token sayısı (genellikle 2)
  - `max_partial_attempts: usize` — kısmi dolum deneme sayısı
  - `solution_gas_offset: eth::SignedGas` — gas tahmini düzeltme
  - `native_token_price_estimation_amount: eth::U256` — fiyat tahmini için kullanılan native token miktarı
  - `uni_v3_node_url: Option<Url>` — UniswapV3 için RPC URL

- `Inner` (satır 43): Config alanları + `uni_v3_quoter_v2: Option<Arc<...>>`

**DEADLINE_SLACK (satır 31): 500ms**
Solver, deadline'dan 500ms önce durur — ağ latansı ve encoding için buffer.

**solve() Ana Akışı (satır 107-142):**
```
1. deadline = auction.deadline - 500ms
2. (sender, receiver) = mpsc::unbounded_channel()
3. background_work = inner.solve(auction, sender)  [CPU-yoğun, tokio spawn]
4. tokio::time::timeout(remaining, handle).await
5. timeout olursa handle.abort()  [memory leak önleme]
6. receiver'dan tüm solutions topla
7. return solutions
```

**Inner::solve() — Emir İşleme Döngüsü (satır 146-275):**

```
for each order in auction.orders:
  1. sell_token fiyatını al:
     a. Auction'da varsa: direkt kullan
     b. WETH'se: 1e18 (sabit)
     c. Yoksa: native_price_request ile tahmin et
        - sell_token → WETH route bul
        - price = native_estimation_amount / route.input.amount

  2. compute_solution(request) = async closure:
     a. sell_token == buy_token: direkt çözüm (gas=0, output=input)
     b. Aksi: boundary_solver.route(request, max_hops)
        - her segment → LiquidityInteraction
        - buy order: output = min(output, order.buy.amount)  [yuvarlama düzeltmesi]
     c. fee = sell_token_price.ether_value(gas * gas_price)
     d. single_solution.into_solution(fee)
     e. .with_buffers_internalizations(tokens)

  3. requests_for_order(order):
     - tam dolum: 1 istek
     - kısmi dolum: max_partial_attempts istek
       her deneme: amount /= 2 (binary search downward)
```

**requests_for_order() — Binary Halving (satır 278-310):**
```
for i in 0..n:
  divisor = 1 << i  (1, 2, 4, 8, ...)
  sell_amount = order.sell.amount / divisor
  buy_amount  = order.buy.amount / divisor
```
Bu: tam dolum deneyin başarısız olması halinde yarı yarıya azaltarak deneme.

**native_price_request() (satır 312-338):**
```
sell = (sell_token, amount=2^144)
buy  = (WETH, amount=native_token_price_estimation_amount)
side = Buy
```
Neden 2^144?:
- UniswapV2 rezervleri uint112 (max=2^112-1)
- 2^144 = 2^(256-112): tam uint112 rezerv ile overflow olmadan swap yapılabilecek maksimum miktar

**to_normalized_price() (satır 341-350):**
```
price_in_eth = 1e18 * price
valid = price_in_eth.is_normal() && price_in_eth >= 1.0 && price_in_eth < 2^256
```
f64 normal sayı kontrolü: NaN, Inf, subnormal → None

**Request (satır 354): Route talebi**
- `sell, buy: eth::Asset, side: order::Side, wrappers: Vec<order::WrapperCall>`

**Route (satır 363): Bulunan rota**
- `segments: Vec<Segment>` — her segment bir AMM swap'ı
- `input()` → ilk segment girişi
- `output()` → son segment çıkışı
- `gas()` → tüm segment gas'larının saturating toplamı

**Segment (satır 369):**
- `liquidity: &'a liquidity::Liquidity` — kullanılan pool (referans, lifetime)
- `input, output: eth::Asset`
- `gas: eth::Gas`

**DEX Uygulama Notu — Nova Shield:**

**Baseline Solver Nova Shield'de Nasıl Çalışır:**
1. Nova Shield DEX'in kendi onchain pool'larından oluşan bir likidite havuzu olmalı
2. Solver: her emir için `path_candidates_with_hops()` ile tüm olası yolları oluşturur
3. Her yol için: AMM formülleriyle theoretical output hesapla
4. En iyi output'u veren yolu seç
5. Gas offset ekle, fee hesapla, solution üret

**Multi-hop Routing Nova Shield'de:**
- base_tokens: WETH, USDC, NOVA gibi merkezi tokenlar
- max_hops=2: A→WETH→B veya A→USDC→WETH→B gibi 3-hop yollar
- Bu yeterli çoğu durumda

---

### [domain/solver/dex/mod.rs] (231 satır)

**Amaç:** Harici DEX API kullanarak emir çözen solver.

**Struct Listesi:**

- `Dex` (satır 23):
  - `dex: infra::dex::Dex` — OKX veya Bitget API client
  - `simulator: infra::dex::Simulator` — gas simülasyonu
  - `slippage: SlippageLimits`
  - `minimum_surplus: MinimumSurplusLimits`
  - `concurrent_requests: NonZeroUsize` — paralel istek sayısı
  - `fills: Fills` — kısmi dolum state makinesi
  - `rate_limiter: rate_limit::RateLimiter` — 429 backoff
  - `gas_offset: eth::Gas`
  - `internalize_interactions: bool`

**solve() Ana Akışı (satır 82-104):**
```
1. solution_stream(&auction) oluştur
2. deadline - 500ms timeout ile stream'i tüket
3. fills.collect_garbage()  [10 dakikadan eski state'leri temizle]
4. return solutions
```

**solution_stream() (satır 106-120):**
```
stream::iter(orders)
  .enumerate()
  .map(|(i, order)| solve_order(order, tokens, gas_price).with_id(i))
  .buffer_unordered(concurrent_requests)  // paralel işleme
  .filter_map(ready)
```
`buffer_unordered`: N adet işlemi aynı anda çalıştırır — maksimum API throughput.

**try_solve() — Swap Deneyin Çekirdeği (satır 122-196):**
```
1. slippage = slippage_limits.relative(dex_order.amount(), tokens)
2. swap = rate_limiter.execute_with_back_off(
     dex.swap(dex_order, slippage, tokens)
   ) → 429 durumunda exponential backoff
3. swap yoksa: fills.reduce_next_try(order.uid)  // kısmi dolum indir
4. swap.satisfies(order) kontrolü
5. swap.satisfies_with_minimum_surplus(order, min_surplus) kontrolü
```

**solve_order() (satır 198-230):**
```
1. fills.dex_order(order, tokens) → None ise emir değmez
2. try_solve(order, dex_order, tokens) → swap bul
3. swap.into_solution(order, gas_price, sell_price, simulator, gas_offset)
4. fills.increase_next_try(order.uid)  // başarı: miktar artır
5. internalize_interactions → with_buffers_internalizations(tokens)
```

**Error handling matrix:**
| Hata | Eylem |
|------|-------|
| NotFound (kısmi dolum) | fills.reduce_next_try() |
| NotFound (tam dolum) | log + skip |
| OrderNotSupported | log + skip |
| BadRequest | warn |
| RateLimited | rate_limiter backoff |
| UnavailableForLegalReasons | log + skip |
| Other | warn |

**DEX Uygulama Notu — Nova Shield:**
- `buffer_unordered(N)`: N adet API isteğini paralel gönder — throughput için kritik
- Backoff stratejisi: growth_factor=2, min=1s, max=8s (config'den)
- Kısmi dolum state: fills.amounts map (her order için son denenen miktar tutulur)

---

### [domain/solver/dex/fills.rs] (181 satır)

**Amaç:** Kısmi dolum emirleri için adaptive fill amount state makinesi.

**Struct Listesi:**

- `Fills` (satır 19):
  - `amounts: Mutex<HashMap<order::Uid, CacheEntry>>` — her order için son denenen miktar
  - `smallest_fill: BigDecimal` — minimum deneme miktarı (ETH cinsinden)

- `CacheEntry` (satır 176):
  - `next_amount: eth::U256` — bir sonraki deneme miktarı
  - `total_amount: eth::U256` — emrin toplam miktarı
  - `last_requested: Instant` — son istek zamanı

**dex_order() — Kısmi Dolum Mantığı (satır 42-136):**

```
1. Tam dolumlu emir: direkt dönüştür
2. Kısmi dolumlu emir:
   a. smallest_fill hesapla:
      smallest_fill_token = smallest_fill_eth / token_price_in_eth * ETH_price
      (reference_price dönüşümü)
   b. Cache'de bu emir için state al/oluştur
   c. Geçerlilik kontrolleri:
      - next_amount < smallest_fill: yeniden başla (total_amount)
      - next_amount > total_amount: kısmi dolmuş, ayarla
   d. Miktar sıfır veya smallest_fill altında → None
   e. Orantılı sell/buy miktarları:
      Buy order: sell = sell.amount * amount / buy.amount  [widening_mul / U512]
      Sell order: buy = buy.amount * amount / sell.amount  [widening_mul / U512]
```

**widening_mul Kullanımı (satır 101-119):**
```rust
order.sell.amount.widening_mul(amount).checked_div(U512::from(order.buy.amount))
```
`widening_mul`: U256 * U256 = U512 — 512-bit ara sonuç ile overflow riski sıfır.

**reduce_next_try() (satır 140-145):**
```
next_amount /= 2  (binary halving)
```

**increase_next_try() (satır 148-158):**
```
next_amount = min(next_amount * 2, total_amount)
```
Başarılı swap sonrası: daha büyük fill dene.

**collect_garbage() (satır 162-172):**
```
MAX_AGE = 10 dakika
10 dakika istenmemiş entry'leri sil
```

**DEX Uygulama Notu — Nova Shield:**
- Binary search pattern: kısmi dolum için hem azalt (başarısızlıkta) hem artır (başarıda) — adaptive
- smallest_fill: minimum ekonomik dolum miktarı — gas'ı karşılamasını garanti eder
- Garbage collection: settled/expired emirlerin state'ini temizler
- Nova Shield'de bu mekanizma kritik: kısmi dolum emirleri DEX'in çekicilik sağlaması için önemli

---

## 3. BOUNDARY KATMANI — TÜMÜ

### [boundary/baseline.rs] (333 satır)

**Amaç:** Baseline solver'ın gerçek yol bulma implementasyonu — domain solver ile onchain likidite arasındaki köprü.

**Struct Listesi:**

- `Solver<'a>` (satır 21):
  - `base_tokens: BaseTokens` — temel token seti (WETH + konfigüre edilmiş tokenlar)
  - `onchain_liquidity: HashMap<TokenPair, Vec<OnchainLiquidity>>` — token çiftine göre pool'lar
  - `liquidity: HashMap<liquidity::Id, &'a liquidity::Liquidity>` — ID'ye göre domain liquidity

**new() — Initialization (satır 28-42):**
```
1. base_tokens oluştur (WETH + custom tokens → boundary::BaseTokens)
2. onchain_liquidity oluştur:
   - Her domain Liquidity → OnchainLiquidity (boundary pool) dönüşümü
   - TokenPair'e göre grupla → HashMap<TokenPair, Vec<OnchainLiquidity>>
3. id → reference map oluştur
```

**route() — Yol Bulma Algoritması (satır 44-115):**

```
1. candidates = base_tokens.path_candidates_with_hops(sell, buy, max_hops)
   → tüm olası token yolları: [sell → buy], [sell → WETH → buy], vs.

2. Her candidate path için parallel execution:
   Buy side:
     a. estimate_sell_amount(buy_amount, path, liquidity)
        → backwards: en az sell eden yolu bul
     b. traverse_path(sell_token, sell.value)
        → forward: gerçek segment'lere dönüştür
     c. Validasyon: actual_buy >= requested_buy
     d. Validasyon: sell.value <= order.sell.amount
     → tuple: (segments, sell_estimate)

   Sell side:
     a. estimate_buy_amount(sell_amount, path, liquidity)
        → forward: en fazla buy eden yolu bul
     b. traverse_path(sell_token, sell_amount)
     c. Validasyon: actual_sell <= requested_sell
     d. Validasyon: buy.value >= order.buy.amount
     → tuple: (segments, buy_estimate)

3. Seçim kriteri:
   Buy side: min_by_key(sell.value) → en az ödeme
   Sell side: max_by_key(buy.value) → en fazla kazanma

4. Route::new(segments)
```

**traverse_path() — Segment Dönüşümü (satır 117-155):**
```
input = (sell_token, sell_amount)
for liquidity in path:
  buy_token = liquidity.token_pair.other(sell_token)
  buy_amount = liquidity.get_amount_out(buy_token, (sell_amount, sell_token))
  segments.push(Segment { liquidity, input, output, gas })
  sell_token = buy_token
  sell_amount = buy_amount
```
Bu: forward simulation — gerçek AMM formüllerini çalıştırarak her adımın çıktısını hesaplar.

**to_boundary_liquidity() — Pool Dönüşümü (satır 158-258):**
```
for liquidity in domain_liquidity:
  match state:
    ConstantProduct → boundary::ConstantProduct pool
    WeightedProduct → N token çifti × boundary::WeightedProduct pool
    Stable → N token çifti × boundary::Stable pool
    LimitOrder → TokenPair → boundary::LimitOrder
    Concentrated → boundary::Concentrated pool (sadece uni_v3_quoter mevcut ise)
```

Multi-token pool'lar (Weighted, Stable): her token çifti için ayrı entry eklenir.

**OnchainLiquidity** (satır 262): Domain + boundary bilgilerini birleştiren iç struct.

**LiquiditySource** enum (satır 269): 5 varyant.

**BaselineSolvable trait impl (satır 277-319):**
```
impl BaselineSolvable for OnchainLiquidity:
  get_amount_out → ilgili pool implementasyonuna delege
  get_amount_in  → ilgili pool implementasyonuna delege
  gas_cost       → ilgili pool implementasyonuna delege
```
Polimorfizm: enum dispatch over 5 pool tipi.

**DEX Uygulama Notu — Nova Shield:**

**Baseline Solver Route Algoritması Nova Shield'de:**
1. Pool indexleme: `HashMap<TokenPair, Vec<Pool>>` — çift başına N pool
2. Path generation: `path_candidates_with_hops()` ile tüm olası yollar
3. Her yol için paralel AMM simülasyonu (`join_all`)
4. Optimal yol seçimi: buy-side: min sell, sell-side: max buy
5. Forward traversal ile kesin sonuç doğrulama

**Neden 2 aşamalı? (estimate + traverse):**
- estimate: hızlı ön filtreleme (path'ın genel yönü)
- traverse: kesin hesaplama (gerçek AMM değerleri)

---

### [boundary/routing.rs] (433 satır)

**Amaç:** Temel path-finding algoritması — en iyi yolu AMM pool'larında bulur.

**Struct Listesi:**

- `Estimate<'a, V, L>` (satır 16):
  - `value: V` — sonuç miktar
  - `path: Vec<&'a L>` — kullanılan likidite pool'ları

- `BaseTokens` (satır 112):
  - `tokens: HashSet<Address>` — WETH + tüm base tokenlar
  - `pairs: HashSet<TokenPair>` — base token çiftleri
  - `new(native, base_tokens)`: token dedup + sort
  - `relevant_pairs(pairs)`: bir token çifti için ilgili tüm pool çiftleri
  - `path_candidates(sell, buy)` — DEFAULT_MAX_HOPS=2 ile yol adayları
  - `path_candidates_with_hops(sell, buy, max_hops)` — özel hop sayısı

**estimate_buy_amount() — Forward Path (satır 38-70):**
```
Giriş: sell_amount, path=[A, B, C, ...], liquidity

1. previous = (sell_amount, first_token, [])
2. for current in path[1..]:
   pools = liquidity[TokenPair(current, previous_token)]
   outputs = join_all(pools.iter().map(|pool| pool.get_amount_out(current, (amount, prev))))
   (best_pool, amount) = outputs.max_by_key(|(_, amount)| amount)
   path.push(best_pool)
   previous = (amount, current, path)
3. return Estimate { value: buy_amount, path }
```
Her hop'ta: en yüksek çıktıyı veren pool seçilir (greedy).

**estimate_sell_amount() — Backward Path (satır 75-110):**
```
Giriş: buy_amount, path=[A, B, C, ...], liquidity

1. previous = (buy_amount, last_token, [])
2. for current in path.rev()[1..]:  // TERSYÖNDE
   pools = liquidity[TokenPair(current, prev_token)]
   outputs = join_all(pools.iter().map(|pool| pool.get_amount_in(current, (amount, prev))))
   (best_pool, amount) = outputs.min_by_key(|(_, amount)| amount)
   path.push(best_pool)
   previous = (amount, current, path)
3. path.reverse()  // Forward sırasına çevir
4. return Estimate { value: sell_amount, path }
```
Her hop'ta: en düşük giriş gerektiren pool seçilir (greedy).

**Greedy vs Optimal:**
Bu greedy yaklaşım — her hop bağımsız optimize ediliyor. Bu, tüm yollar arası global optimum garantilemez, ancak:
- Pratikte yeterince iyi (likidite az bölünmüş)
- Hesaplama karmaşıklığı düşük: O(P * H * K) burada P=path sayısı, H=hop, K=pool/pair

**path_candidates() Algoritması (satır 185-221):**
```
Giriş: sell_token, buy_token, base_tokens, max_hops

candidates = {}
path_prefixes = [[sell_token]]

for _ in 0..(max_hops+1):
  next_prefixes = []
  for prefix in path_prefixes:
    // Bu round: buy_token ekle → tam yol
    candidates.insert(prefix + [buy_token])
    // Sonraki round için: her base_token'ı ekle (zaten yoksa)
    for base_token in base_tokens:
      if base_token != buy_token && base_token not in prefix:
        next_prefixes.push(prefix + [base_token])
  path_prefixes = next_prefixes

return candidates
```

**Örnek (max_hops=1, base_tokens={WETH}):**
```
Round 0:
  prefix=[A] → candidate=[A, B]
  next=[A, WETH]
Round 1:
  prefix=[A, WETH] → candidate=[A, WETH, B]
  (next: [A, WETH, B] would add B which is buy_token, skip)

Sonuç: {[A,B], [A,WETH,B]}
```

**Gas Hesabı (satır 23-32):**
```
cost_of_hops = sum(pool.gas_cost() for pool in path)
total = 50_000 + cost_of_hops
```
50,000: settlement overhead base cost.

**Test Coverage:**
- `test_estimate_amount_returns_none_if_it_contains_pair_without_pool` — eksik pool
- `test_estimate_amount` — 2 hop hesap doğrulaması
- `test_estimate_sell_amount_returns_none_buying_too_much` — pool kapasitesi aşımı
- `test_estimate_amount_multiple_pools` — çoklu pool seçimi (best pool seçimi)
- `test_estimate_amount_invalid_pool` — sıfır rezervli geçersiz pool

**DEX Uygulama Notu — Nova Shield:**

**Multi-hop Routing Özeti:**
- Algoritma: BFS-benzeri yol üretimi (path_candidates) + Greedy-best-pool seçimi
- DEX'e Göre Karşılaştırma: Dijkstra'ya değil greedy'ye dayalı (daha hızlı, az optimal)
- Nova Shield'de iyileştirme: Bellman-Ford veya dynamic programming ile global optimal yol
- Önerilen: "split routing" — tek siparişi birden fazla yola bölme (bu baseline'da yok)

**Routing Algoritması Türleri:**
| Algoritma | CoW Baseline | Önerilen Nova Shield |
|-----------|-------------|---------------------|
| BFS/DFS | Evet (path gen) | Evet |
| Greedy | Evet (pool seç) | + DP |
| Split | Hayır | Evet (TWAMM) |
| Gas-aware | Kısmi | Tam |

---

### [boundary/liquidity/constant_product.rs] (53 satır)

**Amaç:** Domain ConstantProduct pool'unu boundary (shared) Pool'una dönüştürür.

**to_boundary_pool():**
```
1. reserves: u128'e cast (>u128 ise None)
2. fee: Ratio<u32> → pool.fee.numer()/denom() u32'ye cast
3. Pool { address, tokens, reserves, fee } döndür
```
Boundary pool `liquidity_sources::uniswap_v2::pool_fetching::Pool` tipinde.

---

### [boundary/liquidity/weighted_product.rs] (65 satır)

**to_boundary_pool():**
```
1. Pool ID: address'ten right-pad (yaklaşım — yeterli encoding için)
2. swap_fee: to_fixed_point(&pool.fee)
3. reserves: her token için WeightedTokenState { balance, scaling_factor, weight }
4. version: V0 / V3Plus enum mapping
```

**to_fixed_point():**
```
wei = ratio.numer() * 10^18 / ratio.denom()
return Bfp::from_wei(wei)
```
Balancer "fixed point" formatı: 10^18 baz, Bfp (Balancer Fixed Point).

---

### [boundary/liquidity/stable.rs] (61 satır)

**to_boundary_pool():**
```
1. amplification_parameter: AmplificationParameter::try_new(numer, denom)
2. reserves: TokenState { balance, scaling_factor }
```
AmplificationParameter'ın kısıtları: Balancer kontratından geliyor.

---

### [boundary/liquidity/concentrated.rs] (80 satır)

**Amaç:** UniswapV3 QuoterV2 kontratı üzerinden concentrated liquidity hesaplama.

**Pool (satır 13):**
```
uni_v3_quoter_contract: Arc<UniswapV3QuoterV2::Instance>
address: Address
tokens: TokenPair
fee: U24
```

**POOL_SWAP_GAS_COST = 106,000** (Dune analytics verisi ile ölçülmüş)

**get_amount_out() — RPC Çağrısı:**
```rust
self.uni_v3_quoter_contract
    .quoteExactInputSingle(QuoteExactInputSingleParams {
        tokenIn: in_token,
        tokenOut: out_token,
        amountIn: in_amount,
        fee: self.fee,
        sqrtPriceLimitX96: U160::ZERO,  // fiyat limiti yok
    })
    .call()
    .await
    .map(|result| result.amountOut)
```

**get_amount_in() — RPC Çağrısı:**
```rust
quoteExactOutputSingle(QuoteExactOutputSingleParams {
    tokenIn, tokenOut, amount: out_amount,
    fee, sqrtPriceLimitX96: U160::ZERO
})
```

**Güvenlik Analizi:**
- Token pair doğrulama: `TokenPair::new(out_token, in_token) != Some(self.tokens)` → None
- sqrtPriceLimitX96 = 0: fiyat sınırı yok — tam dolum denenir

**DEX Uygulama Notu — Nova Shield:**
- UniswapV3 entegrasyonu için: Quoter kontratına RPC çağrısı gerekli
- Bu, offline hesaplama değil — her swap için ağ round-trip mevcut
- Nova Shield: concentrated liquidity için zincir üstü hesaplama veya local tick math implementasyonu daha hızlı

---

### [boundary/liquidity/limit_order.rs] (165 satır)

**BaselineSolvable for LimitOrder:**

**get_amount_out:**
```rust
// İzin kontrolleri:
in_token != taker.token.0 → None
out_token != maker.token.0 → None
in_amount > taker.amount → None (limit aşılamaz)

// Hesap:
out = in_amount * maker.amount / taker.amount
```

**get_amount_in:**
```rust
out_amount > maker.amount → None

in = out_amount * taker.amount / maker.amount
```

**Round-trip invariant:**
```
get_amount_out(get_amount_in(x)) = x  (integer truncation nedeniyle yaklaşık)
get_amount_in(get_amount_out(x)) = x
```

**gas_cost:** `GAS_PER_ZEROEX_ORDER` sabiti (0x kontrat gaz benchmark)

**Test coverage:** 4 test — round-trip, too-high amounts, wrong tokens.

---

## 4. INFRA/DEX KATMANI — TAM ANALİZ

### [infra/dex/mod.rs] (122 satır)

**Amaç:** Desteklenen harici DEX API'lerinin enum wrapper'ı.

**Dex enum (satır 14):**
```rust
pub enum Dex {
    Bitget(bitget::Bitget),
    Okx(Box<okx::Okx>),  // Box: büyük struct (heap)
}
```

**swap() (satır 24-36):**
Polimorfik dispatch — enum match ile ilgili implementation'a yönlendirir.

**Error enum (satır 40-54):**
| Varyant | Anlam |
|---------|-------|
| OrderNotSupported | Bu order tipi desteklenmiyor |
| NotFound | Swap bulunamadı (likidite yok) |
| BadRequest | Geçersiz istek |
| RateLimited | 429 Too Many Requests |
| UnavailableForLegalReasons | Yasaklı token/ülke |
| Other(Box<dyn Error>) | Genel hata |

**Client (satır 58-84):**
- `client: reqwest::Client` — HTTP client
- `block_stream: Option<CurrentBlockWatcher>` — anlık blok hash'i için
- `request(method, url)`: eğer block_stream mevcutsa `X-CURRENT-BLOCK-HASH` header ekler
  → egress proxy caching için (aynı blok = aynı sonuç cache'lenebilir)

---

### [infra/dex/okx/mod.rs] (517 satır)

**Amaç:** OKX v6 DEX Aggregator API istemcisi.

**Okx struct (satır 37):**
```
client: super::Client
sell_orders_endpoint: URL (default: https://web3.okx.com/api/v6/dex/aggregator/)
buy_orders_endpoint: Option<URL>  (V5 API - sadece buy orders için)
sell_orders_signature_base_url: URL  (proxy'de farklı olabilir)
buy_orders_signature_base_url: Option<URL>
api_secret_key: String
defaults: dto::SwapRequest
dex_approved_addresses: Cache<ApprovalCacheKey, eth::ContractAddress>  (moka cache, 100 entry)
```

**Config (satır 52):**
- 2 endpoint: V6 (sell) + V5 (buy) — farklı API versiyonları
- signature_base_url: proxy kullanımında gerçek OKX URL ile imza üretimi
- price_impact_protection_percent: (0-100%) fiyat etkisi koruma

**try_new() (satır 103):**
```
1. HTTP headers: OK-ACCESS-PROJECT, OK-ACCESS-KEY, OK-ACCESS-PASSPHRASE
2. price_impact_protection: [0.0, 100.0] aralığı doğrulaması
3. defaults.swap_receiver_address = settlement_contract
4. dex_approved_addresses cache başlat (100 entry)
```

**swap() Ana Akışı (satır 169-228):**
```
1. handle_api_requests(order, slippage):
   Sell: parallel(swap + approve_transaction)
   Buy: sequential(swap → approve_transaction)
2. gas *= 1.5  (OKX documentation önerisi)
3. allowance_amount:
   Buy order: U256::MAX (slippage nedeniyle input belirsiz)
   Sell order: from_token_amount
4. dex::Swap { calls, input, output, allowance, gas } döndür
```

**handle_sell_order() — Paralel API (satır 251-290):**
```
tokio::try_join!(
  swap_request → /swap endpoint
  approve_request → /approve-transaction endpoint
)
```
Sell order'da: input miktarı önceden bilindiği için her iki istek paralel.

**handle_buy_order() — Sıralı API (satır 297-341):**
```
1. V5 /swap → swap_response (from_token_amount biliniyor)
2. approve_request(from_token_amount) → /approve-transaction
```
Buy order'da: input miktarı swap sonucuna bağlı, sıralı.

**cache_approval_address() (satır 344-362):**
```
Cache key: (token_address, order_side)
moka::Cache::try_get_with() → async computation on miss
```
V5 ve V6 aynı token için farklı approve kontratı döndürebilir — yan yana cache.

**generate_signature() — HMAC-SHA256 (satır 367-386):**
```
data = timestamp + method + signature_url.path + "?" + query_string
signature = base64(HMAC-SHA256(secret_key, data))
headers: OK-ACCESS-TIMESTAMP, OK-ACCESS-SIGN
```

**handle_api_error() (satır 388-403):**
```
code 0 → OK
51005, 82000, 82104, 82112 → NotFound (liquidity issues)
50011 → RateLimited
_ → Api { code, reason }
```

**send_get_request() (satır 405-459):**
```
1. Request builder oluştur
2. Signature hesapla
3. Timestamp + signature header ekle
4. util::http::roundtrip! makrosu ile gönder
5. Response { code, data: Vec<T>, msg } → ilk data item
```

**Error türleri (satır 462-516):**
- Hem HTTP 429 hem API code 429 → RateLimited
- Invalid header value → CreationError::Header

**DEX Uygulama Notu — Nova Shield:**

**OKX Entegrasyonu Checklist:**
1. API Kimlik Bilgileri: project_id + api_key + api_passphrase + api_secret_key
2. İmza: `timestamp + method + path + ? + query_params` → HMAC-SHA256 → base64
3. Headers: OK-ACCESS-PROJECT, OK-ACCESS-KEY, OK-ACCESS-PASSPHRASE, OK-ACCESS-TIMESTAMP, OK-ACCESS-SIGN
4. V6 sell, V5 buy (buy orders V6'da yeni ama V5 daha stabil)
5. gas × 1.5: OKX gas tahminleri düşük, % 50 emniyet payı
6. Price impact protection: 90% default (API) — konfigüre et
7. Approve kontrat cache'i: her token + side için ayrı cache — API maliyeti azaltır

---

### [infra/dex/okx/dto.rs] (313 satır)

**SwapRequest (satır 26):**
| Alan | Tip | Açıklama |
|------|-----|----------|
| chain_index | u64 → DisplayFromStr | Chain ID (1=Mainnet) |
| amount | U256 → HexOrDecimalU256 | Giriş miktarı (min units) |
| from_token_address | eth::Address | Satılan token |
| to_token_address | eth::Address | Alınan token |
| slippage_percent | Slippage (BigDecimal) | Tolere edilen slippage |
| user_wallet_address | eth::Address | Settlement contract |
| swap_receiver_address | eth::Address | Settlement contract |
| swap_mode | SwapMode | "exactIn" / "exactOut" |
| price_impact_protection_percent | BigDecimal → DisplayFromStr | Price impact cap |

**SwapRequestV5 (satır 96):**
V5 farkları: `chain_id` (chain_index yerine), `slippage` (slippage_percent yerine)

**SwapResponse (satır 157):**
```
router_result:
  from_token: { token_contract_address }
  to_token: { token_contract_address }
  from_token_amount: U256 (HexOrDecimalU256)
  to_token_amount: U256 (HexOrDecimalU256)
tx:
  gas: U256 (HexOrDecimalU256)
  to: eth::Address  (router contract)
  data: Vec<u8> (BytesHex)
```

**ApproveTransactionRequest (satır 225):**
```
chain_index: u64
token_contract_address: eth::Address
approve_amount: U256
```

**ApproveTransactionResponse (satır 284):**
```
dex_contract_address: eth::Address  (approve spender)
```

**Response<T> Wrapper (satır 294):**
```
code: i64 (DisplayFromStr — string olarak geliyor!)
data: Vec<T>  (success: [item], error: [])
msg: String
```

---

### [infra/dex/bitget/mod.rs] (472 satır)

**Amaç:** Bitget DEX Aggregator API istemcisi.

**Bitget struct (satır 78):**
```
client: super::Client
endpoint: URL (default: https://bopenapi.bgwapi.io/bgw-pro/swapx/pro/)
api_key, api_secret, partner_code: String
chain_name: dto::ChainName
settlement_contract: Address
enable_buy_orders: bool  (feature flag)
```

**swap() (satır 141-177):**
```
Buy order + !enable_buy_orders → OrderNotSupported
sell_decimals = tokens[order.sell].decimals  (yoksa MissingDecimals)
buy_decimals = tokens[order.buy].decimals
match side: handle_sell_order | handle_buy_order
```

**handle_sell_order() — "rich" mod (satır 185-231):**
```
1. SwapRequest { requestMod: "rich" } → /swap POST
2. response: { out_amount, gas_fee.gas_limit, swap_transaction.{to, data} }
3. gas = gas_limit * 1.5
4. output = decimal_to_wei(out_amount, buy_decimals, Round::Down)
5. return dex::Swap
```

**handle_buy_order() — Reverse quote (satır 249-326):**
```
1. ReverseSwapRequest { requestMode: "minAmountOut" } → /swapr POST
2. response: { amount_in, expected_amount_out, txs }
3. tx = txs.rev().find(|tx| tx.function == "swap")  // sadece swap tx'i al
4. Sanity check: expected_amount_out >= requested_out
5. input = decimal_to_wei(amount_in, sell_decimals, Round::Up)
6. output = order.amount.get()  (exactly buy amount)
```

**Rounding Mantığı (satır 34-58):**
```
Round::Up   → input amounts (approve, pull funds) — fazla öde, revert'i önle
Round::Down → output amounts (claim) — az iddaa et, revert'i önle
```

**generate_signature() — HMAC-SHA256 (satır 332-353):**
```
content = BTreeMap {
  "apiPath": api_path,
  "body": body_json,
  "x-api-key": api_key,
  "x-api-timestamp": timestamp,
}
content_str = serde_json::to_string(content)  // alphabetical order (BTreeMap)
signature = base64(HMAC-SHA256(api_secret, content_str))
```
OKX'ten fark: JSON body imzalıyor, BTreeMap alphabetical sort zorunlu.

**handle_api_error() (satır 357-388):**
```
80000 → NotFound (internal error)
80001 → NotFound (insufficient balance)
80004 → NotFound (expired)
80005 → NotFound (insufficient liquidity)
80008 → NotFound (reverse quote not converged)
80002/80003/80006/80013 → BadRequest
_ → Api { code, message }
```

**send_post_request() (satır 390-438):**
```
1. URL = endpoint + path
2. body_str = serde_json::to_string(body)
3. timestamp = millis
4. signature = generate_signature(api_path, body_str, timestamp)
5. POST headers: Content-Type, Partner-Code, x-api-key, x-api-timestamp, x-api-signature
6. response parse → Response<T> → handle_api_error → data.ok_or(NotFound)
```

**DEX Uygulama Notu — Nova Shield:**

**Bitget Entegrasyonu Checklist:**
1. API Kimlik Bilgileri: api_key + api_secret + partner_code
2. İmza: JSON(BTreeMap) → HMAC-SHA256 → base64 (OKX'ten farklı)
3. Headers: Content-Type:json, Partner-Code, x-api-key, x-api-timestamp, x-api-signature
4. Sell orders: /swap (requestMod="rich") — tek çağrı
5. Buy orders: /swapr (requestMode="minAmountOut") — reverse quote
6. gas × 1.5 (OKX ile aynı)
7. Decimal dönüşüm zorunlu: wei ↔ human-readable decimal
8. Rounding yönü kritik: input=Up, output=Down

---

### [infra/dex/bitget/dto.rs] (272 satır)

**ChainName (satır 12):** eth, bnb, base, matic, arbitrum

**SwapRequest (satır 44):**
```
from_contract: eth::Address
from_amount: BigDecimal (DisplayFromStr — human readable!)
from_chain, to_chain: ChainName
to_contract: eth::Address
from_address, to_address: Address  (settlement contract)
market: "bgwevmaggregator"  (Bitget tavsiyesi — en iyi EVM rotaları)
slippage: f64  (percentage, ör. 1.0 = 1%)
request_mod: "rich"
fee_rate: Option<f64>  (0.0 = ücretsiz)
```

**SwapResponse (satır 108):**
```
out_amount: BigDecimal (DisplayFromStr — decimal!)
gas_fee.gas_limit: u64 (DisplayFromStr)
swap_transaction: { to, data: Vec<u8> (BytesHex) }
```

**ReverseSwapRequest (satır 151):**
```
from_contract, to_contract: eth::Address
from_chain: ChainName
amount: BigDecimal  (istenen minimum output miktarı — buy amount)
request_mode: "minAmountOut"
from_address, to_address: Address
slippage: f64 (DisplayFromStr)
fee_rate: 0.0
```

**ReverseSwapResponse (satır 211):**
```
amount_in: BigDecimal  (gerekli sell miktarı)
expected_amount_out: BigDecimal  (beklenen çıkış — requested'den biraz fazla)
txs: Vec<ReverseSwapTx>
```

**ReverseSwapTx (satır 228):**
```
to: eth::Address
calldata: Vec<u8> (BytesHex)
function: String  (ör. "swap", "approve")
gas_limit: u64 (DisplayFromStr)
```

**Response<T> Wrapper (satır 255):**
```
status: i64  (0=success, 1=failure)
error_code: Option<i64>
message: Option<String>
data: Option<T>  (null on error!)
```
OKX'ten fark: data=Option yerine Vec, status=integer yerine code.

---

### [infra/dex/simulator.rs] (121 satır)

**Amaç:** Limit emirleri için gerçek gas maliyeti simülasyonu.

**Simulator struct (satır 22):**
```
web3: DynProvider
settlement: Address
authenticator: Address
```

**gas() — Simülasyon Algoritması (satır 41-111):**
```
Özel durum: owner == settlement → SettlementContractIsOwner hatası

1. swapper = Swapper::Instance::new(owner, web3)
   (trader address'inde Swapper bytecode'u override edilmiş contract instance)

2. State override:
   owner address → Swapper bytecode (simülasyon için)
   authenticator address → AnyoneAuthenticator bytecode
   (herhangi adresin solver olarak davranmasına izin verir)

3. Swapper.swap() çağrısı:
   - settlement_contract
   - sell = Asset { token, amount }
   - buy = Asset { token, amount }
   - allowance = Allowance { spender, amount }
   - interactions = Vec<Interaction { target, value, callData }>

4. gas = 0 → heuristic swap.gas kullan (simülasyon mümkün değil)
   gas > 0 → eth::Gas(gas) döndür
```

**State Override Tekniği:**
```
Amacı: gerçek EVM state'ini değiştirmeden blok üstünde simülasyon
AnyoneAuthenticator: solver auth bypass → auth gerektiren kontrat çağrıları simüle edebilir
Swapper: trader logic → settlement flow'u simüle eder
```

**DEX Uygulama Notu — Nova Shield:**
- Gas simülasyonu limit emirler için kritik — heuristic gas yetersiz kalabilir
- `eth_call` + state override tekniği: alloy kütüphanesi ile uygulanabilir
- Nova Shield'de: Swapper ve AnyoneAuthenticator equivalent kontratları deploy edilmeli

---

## 5. KONFİGÜRASYON DOSYALARI

### [infra/config/baseline.rs] (96 satır)

**Config (satır 16):**
```toml
[toml config]
chain-id = "mainnet"          # veya weth address
weth = "0x..."                # chain-id ile XOR
base-tokens = ["0x...", ...]  # ara hop tokenları
max-hops = 2                  # maksimum ara hop sayısı
max-partial-attempts = 5      # kısmi dolum deneme sayısı
solution-gas-offset = 106391  # gas tahmini düzeltme
native-token-price-estimation-amount = 1000000000000000000  # 1 ETH
uni-v3-node-url = "https://..."  # opsiyonel
```

**default_gas_offset():** `SETTLEMENT_OVERHEAD` sabitinden — price_estimation crate'indeki değer.

### [infra/config/dex/file.rs] (207 satır)

**Config (satır 22):**
```toml
node-url = "https://..."
settlement = "0x..."           # opsiyonel
relative-slippage = "0.01"    # 1% default
absolute-slippage = 20000000000000000  # 0.02 ETH
relative-minimum-surplus = "0"
absolute-minimum-surplus = ...
concurrent-requests = 1
smallest-partial-fill = 10000000000000000  # 0.01 ETH
back-off-growth-factor = 2.0
min-back-off = "1s"
max-back-off = "8s"
gas-offset = 106391
current-block-poll-interval = "12s"  # opsiyonel
internalize-interactions = true
[dex]
# API-specific config (OKX veya Bitget)
```

---

## 6. MATEMATİKSEL FORMÜLLER — ÖZET

### Constant Product (Uniswap V2)
```
k = x * y
out = y * (dx * fee_complement) / (x * fee_denom + dx * fee_complement)
fee_complement = fee_denom - fee_numer
```

### Weighted Product (Balancer V2)
```
W = ∏(balance_i^weight_i) = sabit
out = balance_out * (1 - (balance_in/(balance_in + dx*(1-fee)))^(w_in/w_out))
```

### StableSwap (Curve / Balancer Stable)
```
A*n^n*∑x_i + D = A*D*n^n + D^(n+1)/(n^n*∏x_i)
```

### UniswapV3 (Concentrated)
```
onchain QuoterV2 kontratı → sqrt(price) tick math (sayısal)
```

### Limit Order (0x style)
```
out = in_amount * maker / taker
in  = out_amount * taker / maker
```

### Limit Fiyat Kontrolü
```
order.sell * actual_buy >= order.buy * actual_sell
```

### Slippage Hesabı
```
effective_slippage = min(relative, absolute/asset_value_eth)
tolerance = ceil(amount * effective_slippage)
min_out = amount - tolerance
max_in  = amount + tolerance
```

### Minimum Surplus
```
effective_surplus = max(relative, absolute/asset_value_eth)
required_buy = order.buy * (1 + effective_surplus)
```

### Fee Hesabı (DEX Solver)
```
gas_cost_eth = (swap_gas + gas_offset) * gas_price
fee_sell_token = gas_cost_eth / sell_token_price_eth
```

### Clearing Prices
```
sell_token_clearing_price = actual_buy_amount
buy_token_clearing_price  = actual_sell_amount - surplus_fee
```

---

## 7. GÜVENLİK ANALİZİ

### Overflow Koruması
- Tüm kritik çarpımlar: `checked_mul`, `checked_add`, `checked_sub`
- U256 × U256: `widening_mul` → U512 (512-bit ara sonuç)
- U512 → U256: `U256::uint_try_from` ile güvenli cast

### MEV Koruması
- Uniform clearing prices: front-running'i zorlaştırır (herkes aynı fiyattan işlem görür)
- Deadline mekanizması: eski auction'ları engeller
- Buffer internalizasyonu: onchain interaction sayısını azaltır → less extractable value

### Slippage Saldırıları
- Minimum surplus mekanizması: düşük surplus'lı swap'ları reddeder
- Absolute slippage cap: büyük emirleri korur
- satisfies() kontrolü: swap gerçekten emri karşılıyor mu doğrular

### API Güvenliği
- OKX: HMAC-SHA256 imza (timestamp dahil — replay saldırısı koruması)
- Bitget: HMAC-SHA256 + JSON body imzası
- api_secret: header olarak değil, imzalama için kullanılır
- `set_sensitive()`: api_key header'larını log'dan gizler

### Kısmi Dolum Güvenliği
- smallest_fill: ekonomik olmayan micro-fill'leri engeller
- MAX_AGE=10dk: stale state temizlenir
- Binary halving: tek seferde büyük dolum atlanmaz, adım adım azalır

---

## 8. DEX UYGULAMA NOTU — NOVA SHIELD İÇİN TAM EYLEM PLANI

### A. Baseline Solver Tasarımı

**Gerekli Bileşenler:**
1. **Liquidity Sources:**
   - Nova Shield kendi AMM pool'ları (constant product başlangıç için yeterli)
   - Opsiyonel: Balancer-fork weighted/stable
   - Opsiyonel: concentrated (zkEVM'de daha karmaşık)

2. **Pool State:**
   ```rust
   struct NovaPool {
       id: PoolId,
       address: ContractAddress,
       token_a: TokenAddress,
       token_b: TokenAddress,
       reserve_a: U256,
       reserve_b: U256,
       fee_numerator: u32,
       fee_denominator: u32,
   }
   ```

3. **Path Finding:**
   ```
   base_tokens = [NOVA, WETH, USDC, USDT]  // merkezi tokenlar
   max_hops = 2  // A → NOVA → B veya A → WETH → B
   ```
   CoW'un `path_candidates_with_hops()` algoritmasını kullan (BFS-tabanlı)

4. **Route Selection:**
   - Sell order: `max_by_key(buy_amount)` — maksimum alım
   - Buy order: `min_by_key(sell_amount)` — minimum satış
   - Her hop'ta bağımsız greedy pool seçimi

5. **Clearing Prices:**
   - `sell_token → buy_amount`
   - `buy_token → sell_amount - fee`
   - Uniform pricing tüm emirler için

### B. External DEX Entegrasyonu

**OKX Entegrasyonu için Gerekli:**
```rust
pub struct OkxConfig {
    project_id: String,
    api_key: String,
    api_secret_key: String,
    api_passphrase: String,
    chain_id: u64,
    settlement_contract: Address,
    price_impact_protection: f64,
}
```
İmza: `timestamp + GET + path + ? + query → HMAC-SHA256 → base64`

**Bitget Entegrasyonu için Gerekli:**
```rust
pub struct BitgetConfig {
    api_key: String,
    api_secret: String,
    partner_code: String,
    chain_id: ChainId,
}
```
İmza: `BTreeMap{apiPath, body, x-api-key, x-api-timestamp} → JSON → HMAC-SHA256 → base64`

### C. Kısmi Dolum Mekanizması

```rust
// Binary adaptive fill:
// Başarısız: next_amount /= 2
// Başarılı: next_amount = min(next_amount * 2, total)
// Reset: next_amount < smallest_fill → total_amount

smallest_fill = 0.01 ETH eşdeğeri  // min ekonomik dolum
max_partial_attempts = 5            // 100%, 50%, 25%, 12.5%, 6.25%
```

### D. Slippage Yönetimi

```
relative_slippage = 1%  (default)
absolute_slippage = 0.02 ETH (cap)
effective = min(relative, absolute/asset_value_eth)
```

### E. Gas Simülasyonu

Limit emirler için gas simülasyonu zorunlu:
```rust
// eth_call + state override
// Swapper contract: settlement flow simülasyonu
// AnyoneAuthenticator: solver auth bypass
// gas > 0 → gerçek gas, gas == 0 → heuristic
```

### F. MEV Koruma Mekanizmaları

1. **Uniform Clearing Prices:** Batch'teki tüm emirler aynı fiyattan
2. **Deadline Slack (500ms):** Buffer süresi
3. **Buffer Internalization:** Trusted token pool'dan optimize
4. **Minimum Surplus:** Kârsız işlemleri engelle

### G. Rate Limiting

```
growth_factor = 2.0  (exponential backoff)
min_back_off = 1 saniye
max_back_off = 8 saniye
429 → backoff → retry
```

### H. Nova Shield'e Özgü İyileştirmeler

CoW Baseline'dan Daha İyi Yapılabilecekler:

1. **Optimal Path (Bellman-Ford):** Greedy yerine global optimal yol bulma
2. **Split Routing:** Tek emri birden fazla pool'a böl → daha az slippage
3. **ZK Gas Optimization:** zk-proof gas maliyeti farklı — gas modelini buna göre ayarla
4. **TWAMM Entegrasyonu:** Büyük emirler için Time-Weighted AMM
5. **On-chain Price Oracle:** UniswapV3 TWAP yerine zkVM'de native oracle
6. **Privacy Preserving Routing:** ZK proof ile routing detaylarını gizle (MEV koruması++)

---

## 9. DOSYA SAYISI VE SATIR ÖZETİ

| Dizin | Dosya | Satır | Analiz |
|-------|-------|-------|--------|
| domain/auction.rs | 1 | 105 | Tam |
| domain/order.rs | 1 | 162 | Tam |
| domain/solution.rs | 1 | 486 | Tam |
| domain/liquidity/ | 6 | ~250 | Tam |
| domain/dex/ | 4 | ~400 | Tam |
| domain/solver/ | 3 | ~600 | Tam |
| domain/notification.rs | 1 | 62 | Tam |
| boundary/baseline.rs | 1 | 333 | Tam |
| boundary/routing.rs | 1 | 433 | Tam |
| boundary/liquidity/ | 5 | ~290 | Tam |
| infra/dex/okx/ | 2 | ~830 | Tam |
| infra/dex/bitget/ | 2 | ~744 | Tam |
| infra/dex/simulator.rs | 1 | 121 | Tam |
| infra/dex/mod.rs | 1 | 122 | Tam |
| infra/config/dex/file.rs | 1 | 207 | Tam |
| infra/config/baseline.rs | 1 | 96 | Tam |
| **TOPLAM** | **~32** | **~5,241** | **%100** |

---

## 10. KRİTİK BULGULAR VE ÇIKARIMLAR

### Baseline Solver'ın Gücü
- 5 farklı likidite tipi (ConstantProduct, Weighted, Stable, Concentrated, LimitOrder)
- Greedy path selection — hızlı, yeterince iyi
- Binary halving partial fill — adaptive
- Native token fiyat tahmini (2^144 trick) — akıllı

### Harici DEX Entegrasyonunun Karmaşıklığı
- OKX: iki farklı API versiyonu (V5 sell, V6 buy) — maintainability riski
- Bitget: decimal ↔ wei dönüşümü + rounding yönü kritik güvenlik noktası
- Her iki API: gas × 1.5 düzeltmesi (API güvenilmez gas tahminleri)
- Rate limiting zorunlu: exponential backoff

### MEV Koruma Mekanizmaları
- Uniform clearing prices: en güçlü MEV koruması
- Buffer internalizasyonu: settlement gas + MEV azaltma
- Minimum surplus: solver kârsız işlem yapmaz

### Nova Shield için Kritik Tasarım Kararları
1. ZK gas model farklı — gas_offset ve solution_gas_offset dikkatle ayarlanmalı
2. Minimum surplus: Nova zinciri için ayrı kalibrasyon gerekli
3. Deadline slack (500ms): zk-proof generation süresi + blok süresi dikkate alınmalı
4. NOVA token'ı base_tokens'a ekle — merkezi routing hub olarak

---
*Bu analiz doğrudan kaynak kodundan üretilmiştir — agent-19, CoW Protocol solvers/src/ domain/ + boundary/ tam derinlik analizi.*
