# CoW Protocol Analiz — Agent 05
## Kapsam: solver/src/ — Eski Solver ve Settlement Encoder
## DEX Özel Protokolü: 3x Hassasiyet

**Analiz Tarihi:** 2026-07-22
**Toplam Dosya:** 18 Rust dosyası
**Dosya Listesi:**
- `lib.rs` (6 satır)
- `solver.rs` (79 satır)
- `settlement/mod.rs` (462 satır)
- `settlement/settlement_encoder.rs` (1144 satır)
- `liquidity/mod.rs` (479 satır)
- `liquidity/slippage.rs` (248 satır)
- `liquidity/uniswap_v2.rs` (113 satır)
- `liquidity/uniswap_v3.rs` (165 satır)
- `liquidity/balancer_v2.rs` (477 satır)
- `liquidity/zeroex.rs` (383 satır)
- `liquidity_collector.rs` (256 satır)
- `interactions/mod.rs` (15 satır)
- `interactions/balancer_v2.rs` (135 satır)
- `interactions/erc20.rs` (65 satır)
- `interactions/uniswap_v2.rs` (89 satır)
- `interactions/uniswap_v3.rs` (32 satır)
- `interactions/weth.rs` (111 satır)
- `interactions/zeroex.rs` (49 satır)

---

## Genel Mimari Özeti

`solver` crate, CoW Protocol'ün eski (legacy) baseline solver implementasyonudur. Yeni `solvers` crate ile birlikte production'da kullanılmaktadır. Bu crate'in temel görevi:

1. **Likidite toplama** — UniV2, UniV3, Balancer V2, ZeroEx gibi kaynaklardan AMM havuzlarını fetch etmek
2. **Likidite modelleme** — Bu havuzları solver'ın anlayabileceği `Liquidity` enum'una çevirmek
3. **Settlement encoding** — Solver'ın bulduğu çözümü on-chain calldata formatına çevirmek
4. **Slippage yönetimi** — AMM swap'larına slippage toleransı eklemek

Önemli tasarım kararı: **Solver algoritması bu crate içinde değildir.** Bu crate, solver'ın kullanacağı veri modelini ve settlement encoding altyapısını sağlar. Asıl route-finding mantığı başka yerdedir (baseline_solvable trait üzerinden uniswap_v2 pool_fetching'e delege edilir).

---

## DEX Analizi — Dosya Bazında

---

### [lib.rs] (6 satır)

**Amaç:** Crate'in public modüllerini tanımlar ve `BaselineSolvable` trait implementasyonlarını sağlar.

**Struct/Enum/Trait Listesi:**
- `Arn` (satır 15): AWS KMS ARN identifier'ı için wrapper. Field: `pub String`. Solver'ın remote signing için AWS KMS kullandığını gösterir — MEV koruması için önemli (private key asla sunucuda durmuyor).

**Kritik Fonksiyonlar:**
- `impl BaselineSolvable for ConstantProductOrder` (satır 31-43): UniV2 AMM'leri için fiyat hesaplama delege eder.
  - `get_amount_out(out_token, input: (U256, Address)) -> Option<U256>`: AMM'den çıkacak token miktarını hesaplar.
  - `get_amount_in(in_token, output: (U256, Address)) -> Option<U256>`: AMM'e girecek token miktarını hesaplar.
  - `gas_cost() -> usize`: Bu liquidity'yi kullanmanın gas maliyetini döndürür.

- `impl BaselineSolvable for WeightedProductOrder` (satır 45-61): Balancer V2 weighted pool'lar için aynı trait'i implemente eder.

- `amm_to_pool(amm: &ConstantProductOrder) -> Pool` (satır 63-70): `ConstantProductOrder` → `Pool` dönüşümü. `Pool` struct'ı: `{address, tokens, reserves, fee}`.

- `amm_to_weighted_pool(amm: &WeightedProductOrder) -> WeightedPoolRef<'_>` (satır 72-78): `WeightedProductOrder` → `WeightedPoolRef<'_>` dönüşümü. `WeightedPoolRef`: `{reserves, swap_fee, version}`.

**Matematiksel Formüller:**
- UniV2 için `get_amount_out` ve `get_amount_in` hesapları `Pool` struct'ının metodlarına delege edilir — asıl formüller `liquidity_sources::uniswap_v2::pool_fetching::Pool` içinde tanımlanmıştır.

**Güvenlik Analizi:**
- `Arn::from_str`: Prefix kontrolü (`arn:aws:kms:`) ile kötü konfigürasyon yakalanır. Ancak tam ARN format validation yapılmaz — yeterli seviyede.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın solver da AMM havuzlarını `BaselineSolvable` benzeri bir trait üzerinden soyutlamalı. Bu sayede farklı AMM tiplerini (UniV2 clone'lar, stable pool'lar) aynı solver mantığı ile kullanabiliriz.
- AWS KMS yerine Nova Shield'ın MPC (cggmp21) kullanması gerekiyor — bu crate, bu pattern'i `Arn` wrapper ile gösteriyor.

---

### [solver.rs] (79 satır)

**Amaç:** `BaselineSolvable` trait implementasyonlarını AMM tiplerinden pool wrapper tiplerine delege eder; `Arn` tanımlar.

**NOT:** Bu dosyanın adı "solver.rs" olmakla birlikte, asıl solver algoritması içermez. Sadece `lib.rs`'in bir continuation'ıdır — aynı dosyada olabilirdi. Asıl `Solver` trait'i başka bir crate'dedir (`solvers` veya `driver`).

**Struct/Enum/Trait Listesi:**
- `Arn(pub String)` (satır 15): Wrapper tip. `FromStr` impl var, AWS ARN format validation yapar.

**Kritik Fonksiyonlar:**
- `Arn::from_str` (satır 20-28):
  - `s.starts_with("arn:aws:kms:")` kontrolü
  - Başarısız: `Err(anyhow!("Invalid ARN identifier: {}", s))`
  - Başarılı: `Ok(Self(s.to_string()))`

**Güvenlik Analizi:**
- Minimal validation: Prefix kontrolü yeterli pratik hatalara karşı. Bir saldırgan geçerli prefix ile hatalı ARN oluşturabilir, ancak gerçek KMS çağrısı başarısız olacaktır.

**DEX Uygulama Notu — Nova Shield için:**
- Bu solver.rs gerçekte bir "solver" içermiyor. Nova Shield için gerçek solver ayrı bir modülde olmalı ve `Solver` trait'i şunları içermeli: `solve(orders: Vec<LimitOrder>, liquidity: Vec<Liquidity>) -> Vec<Settlement>`.

---

### [settlement/mod.rs] (462 satır)

**Amaç:** On-chain settlement için `Trade`, `TradeExecution`, `Settlement` veri modelini tanımlar ve encoding pipeline'ını yönetir.

**Struct/Enum/Trait Listesi:**

- `Trade` (satır 17-21):
  - `order: Order` — kullanıcının imzaladığı orijinal emir
  - `executed_amount: U256` — gerçekte execute edilen miktar (kısmi doldurma desteği)
  - `fee: U256` — alınan ücret miktarı

- `TradeExecution` (satır 23-30): Execution'ın hesaplanan sonuçları.
  - `sell_token: Address`
  - `buy_token: Address`
  - `sell_amount: U256` — gerçekte satılan miktar
  - `buy_amount: U256` — gerçekte alınan miktar
  - `fee_amount: U256` — gerçekte alınan ücret

- `Revertable` (satır 104-108): Settlement'ın revert riski.
  - `NoRisk` — güvenli settlement
  - `HighRisk` — revert edebilir

- `Settlement` (satır 99-101): Ana settlement tipi.
  - `encoder: SettlementEncoder` — tek field, encoding logic'i encapsulate eder.

**Kritik Fonksiyonlar:**

- `Trade::executed_fee(&self) -> Option<U256>` (satır 35-37):
  - `executed_amount` oranında `fee_amount`'u ölçekler.
  - Kısmi doldurma için: fee prorata hesaplanır.
  - GPv2Settlement.sol#L383-L385'e atıfta bulunur (on-chain aynı mantık).

- `Trade::scale_amount(&self, amount: U256) -> Option<U256>` (satır 40-48):
  - **Sell order:** `amount * executed_amount / sell_amount`
  - **Buy order:** `amount * executed_amount / buy_amount`
  - `checked_mul` ve `checked_div` ile overflow koruması.

- `Trade::executed_amounts(&self, sell_price: U256, buy_price: U256) -> Option<TradeExecution>` (satır 52-81):
  - **Sell order:**
    - `sell_amount = executed_amount`
    - `buy_amount = sell_amount * sell_price / buy_price` (CEILING division — alıcıyı korur)
  - **Buy order:**
    - `buy_amount = executed_amount`
    - `sell_amount = buy_amount * buy_price / sell_price` (FLOOR division — satıcı zaten en kötüde kalmaz)
  - `checked_ceil_div` kullanımı kritik: Sell order'da alıcı her zaman en az limit price'ı alır.

- `Trade::encode(&self, sell_token_index: usize, buy_token_index: usize) -> EncodedTrade` (satır 87-96):
  - `encode_trade()` helper'ına delege eder — on-chain ABI formatı.
  - `sell_token_index` ve `buy_token_index`, clearing price vektöründeki konumlar.

- `Settlement::new(clearing_prices: HashMap<Address, U256>) -> Self` (satır 112-116):
  - Yeni settlement oluşturur. Tüm clearing fiyatları önceden verilmeli.
  - Bu tasarım kararı önemli: Solver, fiyatları bitiş noktasında bilmeli — incremental değil.

- `Settlement::with_liquidity<L>(&mut self, liquidity: &L, execution: L::Execution) -> Result<()>` (satır 118-125):
  - Generic: `L: Settleable`. Her liquidity tipi kendi encoding'ini sağlar.
  - `liquidity.settlement_handling().encode(execution, &mut self.encoder)` çağrısı.

- `Settlement::encode(self, internalization_strategy: InternalizationStrategy) -> EncodedSettlement` (satır 147-149):
  - Son on-chain calldata'yı üretir.

**Matematiksel Formüller:**

- **Sell order buy amount (ceiling):**
  `buy_amount = ceil(executed_sell_amount * sell_price / buy_price)`
  Ceiling rounding: Alıcı her zaman `sell_amount * sell_price / buy_price`'tan en az bu kadar alır.

- **Buy order sell amount (floor):**
  `sell_amount = floor(executed_buy_amount * buy_price / sell_price)`
  Floor rounding: Satıcı hiçbir zaman gereğinden fazla ödemez.

- **Fee prorata (sell order):**
  `executed_fee = fee_amount * executed_amount / sell_amount`

- **Fee prorata (buy order):**
  `executed_fee = fee_amount * executed_amount / buy_amount`

**Güvenlik Analizi:**

- **Overflow koruması:** Her aritmetik işlem `checked_*` variant kullanır, `None` döner overflow durumunda.
- **Division by zero:** `checked_div` sıfır bölme koruması sağlar — `None` döner.
- **Kısmi doldurma:** `executed_amount` orijinal emirden az olabilir — scale_amount bu durumu doğru işler.
- **Test: `test_constructing_settlement_with_zero_prices`:** Clearing price `U256::ZERO` olan token'a trade ekleme denemesi `Err` döner — sıfır fiyat koruması.

**Test Analizi:**
- `sell_order_executed_amounts()`: sell=5, sell_price=3, buy_price=4 → buy=ceil(5*3/4)=ceil(3.75)=4. DOĞRU.
- `buy_order_executed_amounts()`: buy=5, buy_price=4, sell_price=3 → sell=floor(5*4/3)=floor(6.67)=6. DOĞRU.
- `test_trade_fee()`: Tam doldurmada fee=fee_amount, yarım doldurmada fee=fee_amount/2. DOĞRU.
- `includes_limit_order_ucp()`: Limit order için token listesi: `[sell_token, buy_token, sell_token, buy_token]` — ilk 2'si UCP, son 2'si limit order custom price. Bu kritik: On-chain contract limit order'ı ayrıca işler.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın settlement katmanı da `Trade` + `TradeExecution` ayrımını kullanmalı. Execution, çözüm zamanında hesaplanır; Trade, imzalanmış emir verisini tutar.
- Ceiling division sell order'larda, floor division buy order'larda kullanılmalı. Bu, kullanıcıyı korur ve CoW Protocol'ün standard'ıdır.
- Limit order'ların ayrı fiyat vektörü gerektirmesi önemli tasarım kararı: Uniform clearing price sistemi ile limit order'ların coexistence'ı.
- `Revertable` enum Nova Shield için genişletilebilir: `NoRisk | SoftRisk | HighRisk`.

---

### [settlement/settlement_encoder.rs] (1144 satır) — ÇOK KRİTİK

**Amaç:** Settlement'ın on-chain calldata'ya dönüştürülmesi için incremental builder. Token fiyat vektörü, trade array ve interaction listesi yönetimi.

**Struct/Enum/Trait Listesi:**

- `MaybeInternalizableInteraction` (satır 23): `type alias = (Arc<dyn Interaction>, bool)`. Bool = "internalizable" bayrağı.

- `SettlementEncoder` (satır 34-48): Ana encoder struct.
  - `tokens: Vec<Address>` — token array (sorted invariant korunur!)
  - `clearing_prices: HashMap<Address, U256>` — token adresi → fiyat map'i
  - `trades: Vec<EncoderTrade>` — settlement'a eklenen tradeler
  - `execution_plan: Vec<MaybeInternalizableInteraction>` — on-chain interaction'lar (swap calldata'ları)
  - `pre_interactions: Vec<InteractionData>` — trade'den önce çalışacak interaction'lar
  - `post_interactions: Vec<InteractionData>` — trade'den sonra çalışacak interaction'lar
  - `unwraps: Vec<UnwrapWethInteraction>` — WETH→ETH unwrap'leri (gas optimizasyonu: merge edilir)

- `TokenReference` (satır 52-68): Trade'in token fiyatlarına referans.
  - `Indexed { sell_token_index: usize, buy_token_index: usize }`: UCP vektöründeki index.
  - `CustomPrice { sell_token_price: U256, buy_token_price: U256 }`: Liquidity order ve limit order'lar için custom fiyat — UCP'den farklılaşabilir.

- `EncoderTrade` (satır 73-76): Encoder içindeki trade temsili.
  - `data: Trade`
  - `tokens: TokenReference`

- `PricedTrade<'a>` (satır 80-84): Fiyatı hesaplanmış trade.
  - `data: &'a Trade`
  - `sell_token_price: U256`
  - `buy_token_price: U256`

**Kritik Fonksiyonlar:**

- `SettlementEncoder::new(clearing_prices: HashMap<Address, U256>) -> Self` (satır 91-112):
  - Token array'i hashmap'ten derive eder ve **sort eder** (determinism için).
  - Invariant: `tokens == clearing_prices.keys().sorted()`.
  - Bu determinism, aynı settlement'ın her seferinde aynı calldata üretmesini sağlar.

- `SettlementEncoder::all_trades(&self) -> impl Iterator<Item = PricedTrade<'_>>` (satır 129-133):
  - Her trade için `compute_trade_token_prices` çağırır.

- `SettlementEncoder::compute_trade_token_prices<'a>(&'a self, trade: &'a EncoderTrade) -> PricedTrade<'a>` (satır 135-155):
  - `TokenReference::Indexed`: UCP vektöründen fiyat al.
  - `TokenReference::CustomPrice`: Doğrudan stored fiyatı kullan.

- `SettlementEncoder::add_market_trade(&mut self, order, executed_amount, fee) -> Result<TradeExecution>` (satır 160-201):
  - Market order'lar için trade ekleme.
  - Her iki token için clearing price kontrolü (yoksa hata).
  - `verify_executed_amount` çağrısı — kısmi doldurma validasyonu.
  - `TokenReference::Indexed` kullanır.

- `SettlementEncoder::add_trade(&mut self, order, executed_amount, fee) -> Result<TradeExecution>` (satır 205-240): **ANA ENTRY POINT**
  - **Market order:** `add_market_trade` → `TokenReference::Indexed`.
  - **Liquidity order:** Custom price = `(order.buy_amount, order.sell_amount)` (inverted). Liquidity order kendi limit price'ından settle edilir.
  - **Limit order:** `custom_price_for_limit_order` → `TokenReference::CustomPrice`. Fee hesabı özel.
  - `pre_interactions` ve `post_interactions` kayıt altına alınır.

- `SettlementEncoder::custom_price_for_limit_order(&self, order, executed_amount, fee) -> Result<(U256, U256)>` (satır 248-319): **ÇOK KRİTİK**
  - Limit order'lar için custom clearing fiyatı hesaplar.
  - Amaç: Solver `sell_amount - surplus_fee` üzerinden fiyat hesapladı. Gerçek execution `sell_amount` üzerinden — fark `surplus_fee`.
  - **Sell limit order:**
    1. `sell_amount = executed_amount - fee` (solver'ın kullandığı miktar)
    2. `buy_amount = ceil(sell_amount * uniform_sell_price / uniform_buy_price)`
    3. Döndürülen: `sell_token_price = buy_amount`, `buy_token_price = executed_amount`
  - **Buy limit order:**
    1. `buy_amount = executed_amount`
    2. `sell_amount = buy_amount * uniform_buy_price / uniform_sell_price + fee`
    3. Döndürülen: `sell_token_price = buy_amount`, `buy_token_price = sell_amount`
  - Bu fiyat manipülasyonu, settlement contract'ına limit order'ın gerçek execution fiyatını söyler.

- `SettlementEncoder::add_custom_price_trade(...)` (satır 321-348): Custom fiyatlı trade ekleme. `TokenReference::CustomPrice` oluşturur.

- `SettlementEncoder::append_to_execution_plan(&mut self, interaction: Arc<dyn Interaction>)` (satır 350-352):
  - AMM swap calldata'larını execution listesine ekler. `internalizable=false`.

- `SettlementEncoder::append_to_execution_plan_internalizable(...)` (satır 354-360):
  - `internalizable=true` ise ve strateji `SkipInternalizableInteraction` ise, final calldata'ya dahil edilmez.
  - **Gas optimizasyonu:** CoW protokolü, bazı token transferleri için internal balance kullanabilir (Balancer vault). Bu interaction'lar "internalizable" işaretlenir.

- `SettlementEncoder::add_unwrap(&mut self, unwrap: UnwrapWethInteraction)` (satır 363-373):
  - Aynı WETH contract'ı için unwrap'leri merge eder (tek transaction).
  - **Gas optimizasyonu:** 3 WETH unwrap → 1 WETH unwrap calldata.

- `SettlementEncoder::sort_tokens_and_update_indices(&mut self)` (satır 410-426):
  - Token eklendikten sonra token array'ini yeniden sıralar ve trade token indekslerini günceller.
  - `TokenReference::CustomPrice` olan trade'lerin indeksleri değişmez (custom price zaten inline).
  - Binary search için invariant korunur.

- `SettlementEncoder::token_index(&self, token: Address) -> Option<usize>` (satır 428-430):
  - `self.tokens.binary_search(&token).ok()` — O(log n) lookup.

- `SettlementEncoder::drop_unnecessary_tokens_and_prices(&mut self)` (satır 432-458):
  - Trade'lerde kullanılmayan token'ları ve fiyatlarını kaldırır.
  - **User order'lar:** Her iki token (sell ve buy) tutulur.
  - **Liquidity order'lar:** Sadece sell token tutulur (buy token sadece internal kullanım için).
  - **Gas optimizasyonu:** Gereksiz token → clearing price vektörünü küçültür → calldata gaz maliyeti düşer.

- `SettlementEncoder::finish(mut self, internalization_strategy: InternalizationStrategy) -> EncodedSettlement` (satır 460-559): **ON-CHAIN CALLDATA ÜRETİMİ**

  Adım adım algoritma:
  1. `drop_unnecessary_tokens_and_prices()` — token listesini temizle.
  2. `uniform_clearing_price_vec_length = self.tokens.len()` — UCP vektörünün uzunluğu.
  3. UCP token array'i ve fiyat array'i oluştur (sorted order'da).
  4. `CustomPrice` olan trade'lerin token ve fiyatlarını **ek olarak** UCP array'inin sonuna ekle.
  5. Trade'leri encode et:
     - `Indexed` trade: UCP array'deki `sell_token_index` ve `buy_token_index` kullan.
     - `CustomPrice` trade: `custom_price_index` ve `custom_price_index + 1` kullan. `custom_price_index` her custom price trade için 2 artar.
  6. Interactions encode et:
     - `pre`: pre_interactions list.
     - `main`: execution_plan (internalizable filter) + unwraps (son sırada!).
     - `post`: post_interactions list.

**Matematiksel Formüller:**

- **Uniform Clearing Price (UCP):** Tüm user order'lar aynı fiyat vektörüne göre settle edilir. `clearing_price[sell_token] / clearing_price[buy_token]` oranı her trade için aynı.

- **Limit Order Custom Price (sell):**
  `adjusted_sell_price = ceil((executed_sell - fee) * ucp_sell / ucp_buy)`
  `adjusted_buy_price = executed_sell` (tüm satılan miktar, fee dahil)

- **Limit Order Custom Price (buy):**
  `adjusted_sell_price = executed_buy` (alınan miktar)
  `adjusted_buy_price = floor(executed_buy * ucp_buy / ucp_sell) + fee`

- **Liquidity Order Custom Price:**
  `sell_price = order.buy_amount`
  `buy_price = order.sell_amount`
  (Inverted: limit price olarak kullanılır, surplus yasak)

**On-Chain Calldata Formatı (GPv2Settlement.settle() ABI):**
```
tokens:          address[]   — sorted token listesi (UCP + custom price)
clearing_prices: uint256[]   — token başına fiyat (aynı uzunluk)
trades:          tuple[]     — (sell_token_idx, buy_token_idx, ..., executed_amount, flags)
interactions[0]: bytes[][]   — pre-interactions
interactions[1]: bytes[][]   — main-interactions (AMM swapları + WETH unwrapları)
interactions[2]: bytes[][]   — post-interactions
```

**Güvenlik Analizi:**

- **Deterministic encoding:** Token array sort edilir → aynı settlement daima aynı calldata üretir → on-chain replay koruması için önemli.
- **Index doğruluğu:** `sort_tokens_and_update_indices()` her token array değişikliğinden sonra çağrılır — eski index'lerin yanlış kalması engellenir.
- **Zero clearing price:** `add_market_trade` clearing price yoksa `Err` döner — sıfır fiyatla settlement imkansız.
- **Custom price validation:** `add_custom_price_trade` → `verify_executed_amount` → `executed_amounts` zinciri. Herhangi bir adımda overflow/underflow → `None`/`Err`.
- **Internalizable pattern:** `SkipInternalizableInteraction` strateji seçilirse gas tasarrufu sağlanır ama on-chain durumu değişmez — sadece calldata kısalır.
- **Unwrap merge gas optimizasyonu:** Aynı WETH contract için birden fazla unwrap tek calldata'ya merge edilir.
- **Limit order fee extraction:** `surplus_fee` hesabı checked_add/sub kullanır — taşma kontrolü var.

**Tasarım Kararı — Neden CustomPrice?**
Liquidity order'lar kendi limit price'larında settle edilmeli (artı kalmaz). Limit order'lar ise `surplus_fee`'yi protokole bırakır. Her iki durum da UCP'den sapar, bu yüzden CustomPrice token reference tipi gerekli. UCP vektörü sadece market order'lar için yeterli.

**Test Analizi (kapsamlı):**
- `encode_trades_finds_token_index`: Temel trade ekleme, index bulma.
- `settlement_merges_unwraps_for_same_token`: 2 WETH unwrap → 1 merge.
- `settlement_reflects_different_price_for_normal_and_liquidity_order`: UCP vs custom price farkı. Tokens vektörü: `[t0, t1, t1, t0]` — son 2 liquidity order için.
- `settlement_inserts_sell_price_for_new_liquidity_order_if_price_did_not_exist`: Liquidity order, UCP'de olmayan token kullanabilir.
- `settlement_encoder_add_token_equivalency`: WETH/ETH equivalency. Aynı fiyatla iki token → birini yeni ekle.
- `trades_add_interactions_to_the_encoded_and_later_get_encoded`: Pre/post interaction encoding.
- `encoding_strips_unnecessary_tokens_and_prices`: Trade'lerde kullanılmayan token 4 → son token 2 kalır.
- `optionally_encodes_internalizable_transactions`: SkipInternalizable → 1 tx, EncodeAll → 2 tx.
- `computes_custom_price_for_sell_limit_order_correctly`: 1.01 WETH → 1000 USDC. Fee = 0.01 WETH. Custom price = (1000 USDC, 1.01 WETH). Bu: "1.01 WETH satarak 1000 USDC aldım" demek.
- `computes_custom_price_for_buy_limit_order_correctly`: 1 WETH almak için 1010 USDC sat (1000 UCP + 10 fee). Custom price = (1 WETH, 1010 USDC).

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın settlement encoder'ı aynı pattern'i kullanmalı: `tokens` sorted, `clearing_prices` parallel array.
- **Kritik:** UCP vektörü ve custom price vektörünün ayrılması, uniform settlement ile liquidity order settlement'ını tek transaction'da mümkün kılar.
- **Gas optimizasyonu pattern'leri Nova Shield'a uygulanmalı:**
  1. Unwrap merge (native token unwrap'leri birleştir)
  2. Internalizable interactions (internal balance kullanan işlemleri skip et)
  3. Unnecessary token pruning (kullanılmayan token/fiyat çiftlerini kaldır)
- **Geliştirme önerisi:** CoW, `execution_plan`, `pre_interactions`, `post_interactions` ayrı vector'larda tutuyor. Nova Shield, bu üç faz arasındaki bağımlılıkları da modellemeli — özellikle flashloan bazlı settlement için önceki interaction'ların sonucunu sonraki'ye geçirebilmek.
- **CustomPrice ve TokenReference ayrımı kesinlikle uygulanmalı.** Çünkü Nova Shield da limit order ve market order'ı aynı settlement'da settle etmek isteyecek.

---

### [liquidity/mod.rs] (479 satır) — KRİTİK

**Amaç:** Tüm liquidity tiplerine ait veri modellerini ve `Settleable`/`SettlementHandling` trait'lerini tanımlar.

**Struct/Enum/Trait Listesi:**

- `Liquidity` (satır 37-43): Ana liquidity enum.
  - `ConstantProduct(ConstantProductOrder)` — UniV2, SushiSwap, vb.
  - `BalancerWeighted(WeightedProductOrder)` — Balancer V2 weighted pool
  - `BalancerStable(StablePoolOrder)` — Balancer V2 stable pool (Curve benzeri)
  - `LimitOrder(LimitOrder)` — CoW Protocol limit orderbook
  - `Concentrated(ConcentratedLiquidity)` — UniV3 concentrated liquidity

- `Settleable` trait (satır 48-52):
  - `type Execution` — bu liquidity için execution parametresi
  - `fn settlement_handling(&self) -> &dyn SettlementHandling<Self>`

- `SettlementHandling<L>` trait (satır 55-72):
  - `fn as_any(&self) -> &dyn std::any::Any` — downcasting için (hack olarak belirtilmiş: "should eventually be purged with fire")
  - `fn encode(&self, execution: L::Execution, encoder: &mut SettlementEncoder) -> Result<()>` — liquidity execution'ı calldata'ya çevir

- `Exchange` (satır 74-78):
  - `GnosisProtocol` — CoW Protocol orderbook'tan gelen order
  - `ZeroEx` — 0x protocol'dan gelen order

- `LimitOrderId` (satır 85-89):
  - `Market(OrderUid)` — market order
  - `Limit(OrderUid)` — limit order
  - `Liquidity(LiquidityOrderId)` — liquidity sağlayan order

- `LiquidityOrderId` (satır 100-109):
  - `Protocol(OrderUid)` — CoW Protocol ve foreign liquidity
  - `ZeroEx(String)` — 0x order hash

- `LimitOrder` (satır 127-141): Kullanıcı limit emri modeli.
  - `id: LimitOrderId`
  - `sell_token: Address`
  - `buy_token: Address`
  - `sell_amount: U256` — max satılabilecek miktar
  - `buy_amount: U256` — min alınacak miktar
  - `kind: OrderKind` — Sell veya Buy
  - `partially_fillable: bool`
  - `user_fee: U256` — kısmi fill için ölçekleme dahil ücret
  - `settlement_handling: Arc<dyn SettlementHandling<Self>>`
  - `exchange: Exchange`

- `LimitOrderExecution` (satır 174-184):
  - `filled: U256` — doldurulacak miktar
  - `fee: U256` — limit order için solver tarafından hesaplanan ücret

- `BalancedOrder` (satır 205-216): Balance kontrolü geçmiş order.
  - `order: Order`
  - `available_sell_token_balance: U256`

- `ConstantProductOrder` (satır 247-254): UniV2 AMM modeli.
  - `address: Address` — pool contract adresi
  - `tokens: TokenPair`
  - `reserves: (u128, u128)` — token0 ve token1 rezervleri
  - `fee: Ratio<u32>` — ör. 3/1000 = 0.3%
  - `settlement_handling: Arc<dyn SettlementHandling<Self>>`

- `WeightedProductOrder` (satır 295-302): Balancer V2 weighted pool modeli.
  - `address: Address`
  - `reserves: BTreeMap<Address, WeightedTokenState>` — her token için ağırlıklı rezerv
  - `fee: Bfp` — Balancer fixed-point fee (1e18 = 100%)
  - `version: WeightedPoolVersion`
  - `settlement_handling: Arc<dyn SettlementHandling<Self>>`

- `StablePoolOrder` (satır 320-327): Balancer V2 stable pool (Curve tarzı).
  - `address: Address`
  - `reserves: BTreeMap<Address, TokenState>`
  - `fee: Bfp`
  - `amplification_parameter: AmplificationParameter` — StableSwap A parametresi
  - `settlement_handling: Arc<dyn SettlementHandling<Self>>`

- `AmmOrderExecution` (satır 345-349): AMM execution parametreleri.
  - `input_max: TokenAmount` — max girilecek token (slippage dahil)
  - `output: TokenAmount` — alınacak token (kesin)
  - `internalizable: bool` — internal balance kullanılabilir mi

- `ConcentratedLiquidity` (satır 377-382): UniV3 concentrated liquidity.
  - `tokens: TokenPair`
  - `pool: Arc<PoolInfo>` — pool state (tick data dahil)
  - `settlement_handling: Arc<dyn SettlementHandling<Self>>`

**Kritik Fonksiyonlar:**

- `LimitOrder::full_execution_amount(&self) -> U256` (satır 160-164):
  - Sell: `sell_amount`, Buy: `buy_amount`. Tam doldurma miktarı.

- `ConstantProductOrder::for_pool(pool: Pool, settlement_handling: Arc<...>) -> Self` (satır 269-278):
  - Factory method — `Pool` struct'tan `ConstantProductOrder` oluşturur.

**Settleable Implementasyonları:**
- `ConstantProductOrder: Settleable<Execution = AmmOrderExecution>`
- `WeightedProductOrder: Settleable<Execution = AmmOrderExecution>`
- `StablePoolOrder: Settleable<Execution = AmmOrderExecution>`
- `LimitOrder: Settleable<Execution = LimitOrderExecution>`
- `ConcentratedLiquidity: Settleable<Execution = AmmOrderExecution>`

**Tasarım Analizi:**
- `Arc<dyn SettlementHandling<Self>>` pattern: Her liquidity objesi kendi encoding handler'ını içerir. Bu "strategy pattern" — farklı encoding mantığı aynı liquidity tipi için inject edilebilir.
- `as_any()` hack'i: Rust'ta trait object downcasting için geçici çözüm. Temiz tasarım değil. Uzun vadede kaldırılacak.
- `BTreeMap` kullanımı: `WeightedProductOrder.reserves` ve `StablePoolOrder.reserves` `BTreeMap` kullanır — deterministik iterasyon sırası sağlar. Token sırasına bağımlı encoding için kritik.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın `Liquidity` enum'u CoW'dakine benzer olmalı ama genişletilmeli:
  - `Nova(NovaNativeOrder)` — Nova Shield kendi native AMM'i
  - `CrossChain(CrossChainLiquidity)` — köprü likiditeleri
- `Settleable` trait pattern'i mükemmel. Nova Shield da aynı pattern'i uygulayabilir: Her liquidity tipi kendi `encode_to_settlement()` metodunu implemente eder.
- `AmmOrderExecution`'daki `internalizable` bayrağı Nova Shield için önemli: Nova'nın kendi settlement contract'ı internal balance tutabilirse, gas tasarrufu sağlanır.
- `BalancedOrder.available_sell_token_balance`: Nova Shield'ın solver'ı da kullanıcının gerçek bakiyesini kontrol etmeli — settle edilemeyen order'ları filtrelemek için.
- `StablePoolOrder` için `AmplificationParameter`: Nova Shield stable pool desteği için A parametresini takip etmeli. A büyüdükçe pool "daha stabil" — slippage azalır.

---

### [liquidity/slippage.rs] (248 satır) — ÇOK KRİTİK

**Amaç:** AMM liquidity kullanımı sırasında negative slippage limitini hesaplar ve uygular.

**Struct/Enum/Trait Listesi:**

- Sabitler (satır 13-17):
  - `DEFAULT_MAX_SLIPPAGE_BPS: u32 = 10` — varsayılan max slippage = 10 BPS = 0.1%
  - `BPS_BASE: u32 = 10000` — 100% = 10000 BPS

- `SlippageContext<'a>` (satır 20-23): Per-auction slippage computation bağlamı.
  - `prices: &'a ExternalPrices` — external oracle fiyatları
  - `calculator: &'a SlippageCalculator`

- `SlippageCalculator` (satır 104-109): Slippage hesaplama bileşeni.
  - `relative: BigRational` — max relative slippage oranı (örn. 10/10000)
  - `absolute: Option<BigInt>` — max absolute slippage (native token cinsinden). `None` ise sadece relative kullanılır.

- `SlippageAmount` (satır 167-172): Hesaplanan slippage sonucu.
  - `relative: f64` — hesaplanan relative slippage
  - `absolute: U256` — hesaplanan absolute slippage (token cinsinden)

**Kritik Fonksiyonlar:**

- `SlippageContext::apply_to_amm_execution(&self, mut execution: AmmOrderExecution) -> Result<AmmOrderExecution>` (satır 27-88): **ANA SLIPPAGE UYGULAMA METODU**

  Algoritma:
  1. Input token için external price al ve relative slippage hesapla.
  2. Input token fiyatı yoksa, output token fiyatıyla dene.
  3. Her iki fiyat da yoksa: Default relative slippage kullan (absolute capping olmadan).
  4. Absolute slippage miktarını hesapla: `absolute = ceil(relative * input_amount)`.
  5. `execution.input_max.amount += slippage.absolute` — max input'u artır.
  6. Değiştirilmiş execution'ı döndür.

  Tracing: Her adımda hangi token'ın kullanıldığı loglanır.

- `SlippageCalculator::from_bps(relative_bps: u32, absolute: Option<U256>) -> Self` (satır 112-117):
  - `relative = BigRational::new(relative_bps, 10000)`
  - `absolute = absolute.map(|v| u256_to_big_int(v))`

- `SlippageCalculator::context<'a>(&'a self, prices: &'a ExternalPrices) -> SlippageContext<'a>` (satır 119-124):
  - Per-auction bağlam oluşturur.

- `SlippageCalculator::compute(&self, price: Option<&BigRational>, amount: BigInt) -> Result<(Cow<'_, BigRational>, BigInt)>` (satır 128-155): **SLIPPAGE CORE HESAPLAMA**

  Algoritma:
  1. Eğer `absolute` maksimum tanımlıysa:
     - `max_absolute_slippage = max_absolute_native_token / token_price`
     - `max_relative_slippage_respecting_absolute = max_absolute_slippage / amount`
     - `effective_relative = min(max_relative_slippage_respecting_absolute, self.relative)`
  2. Yoksa: `effective_relative = self.relative`
  3. `absolute = ceil(effective_relative * amount)`
  4. Döndür: `(effective_relative, absolute)`

- `SlippageAmount::from_num(relative: &BigRational, absolute: &BigInt) -> Result<Self>` (satır 176-183):
  - `relative` → `f64` dönüşümü (sadece display için)
  - `absolute` → `U256` dönüşümü

- `SlippageAmount::add_to_amount(&self, amount: U256) -> U256` (satır 186-188):
  - `amount.saturating_add(self.absolute)` — saturating! Taşma imkansız.

- `absolute_slippage_amount(relative: &BigRational, amount: &BigInt) -> BigInt` (satır 191-195):
  - `ratio = relative * amount`
  - `ceil(numer / denom)` — ceiling division. AMM input için slippage her zaman yukarı yuvarlanır.

**Matematiksel Formüller:**

- **Relative Slippage:** `s_rel = relative_bps / 10000`
  Örnek: 10 BPS → `s_rel = 0.001` (0.1%)

- **Absolute Slippage (capped):**
  ```
  max_abs_native = max_absolute_limit (native token)
  token_price = native_per_token (oracle)
  max_abs_token = max_abs_native / token_price
  s_rel_capped = min(max_abs_token / input_amount, s_rel_default)
  s_abs = ceil(s_rel_capped * input_amount)
  ```

- **Final Input Max:**
  `input_max_with_slippage = input_amount + s_abs`

- **Örnek (test'ten):**
  - Input: 1 WETH (1e18 wei), max_absolute = 1 ETH, relative = 100 BPS (1%)
  - `max_abs_token = 1e18 / (WETH price in ETH) = 1e18` (WETH price = 1 ETH → max_abs = 1 WETH)
  - `s_rel_capped = min(1e18 / 1e18, 0.01) = min(1.0, 0.01) = 0.01` (1% < 100% → relative capping aktif)
  - `s_abs = ceil(0.01 * 1e18) = 1e16`
  - `input_max = 1e18 + 1e16 = 1.01e18` ✓

- **Capping mekanizması:** Büyük trade'lerde absolute limit devreye girer. Küçük trade'lerde relative limit daha kısıtlayıcı olabilir.

**Güvenlik Analizi:**

- **Slippage bounding:** Absolute limit, büyük trade'lerin aşırı slippage ödememesini sağlar. Küçük trade'ler relative limit ile korunur.
- **Fiyat yokluğu fallback:** Token fiyatı oracle'da yoksa uncapped relative slippage kullanılır — `warn` log atılır.
- **Ceil division:** Slippage her zaman yukarı yuvarlanır (kullanıcı aleyhine değil, protokol aleyhine) — AMM'in slippage toleransı aşmayan swap yapması için.
- **BigRational kullanımı:** Büyük sayı aritmetiği için `BigRational` — `f64` hassasiyet kayıpları önlenir. Sadece display için `f64`.
- **saturating_add:** `add_to_amount` → `U256::MAX` aşılmaz.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın AMM entegrasyonunda aynı slippage pattern'i kullanmalı: `relative` ve `absolute` capping.
- **Geliştirme önerisi:** CoW, slippage'ı sadece input'a uygular. Nova Shield, output'a da uygulayabilir (min output garantisi) — daha güçlü koruma.
- **Oracle bağımlılığı riski:** Fiyat oracle yoksa slippage uncapped. Nova Shield için: Oracle yoksa swap reddet veya çok yüksek default (örn. 0.3%) kullan.
- **Per-auction context:** `SlippageContext` immutable referans tutar — bir auction içinde tüm AMM swap'ları tutarlı fiyatlarla hesaplanır. Nova Shield da aynı pattern'i uygulayabilir.
- **DEFAULT_MAX_SLIPPAGE_BPS = 10 (0.1%):** Çok düşük. Nova Shield için başlangıç değer olarak 10-30 BPS arası düşünülebilir; high-volatility dönemlerinde dinamik ayarlama gerekebilir.

---

### [liquidity/uniswap_v2.rs] (113 satır)

**Amaç:** UniV2-uyumlu AMM havuzlarından likidite toplama ve settlement encoding.

**Struct/Enum/Trait Listesi:**

- `UniswapLikeLiquidity` (satır 18-21): Liquidity collector.
  - `inner: Arc<Inner>` — router ve settlement adresleri
  - `pool_fetcher: Arc<dyn PoolFetching>` — pool verilerini fetch eder

- `Inner` (satır 23-26): Immutable config.
  - `router: Address` — UniV2 router contract adresi
  - `gpv2_settlement: Address` — CoW settlement contract adresi

- `UniswapV3SettlementHandler` (misnomer, gerçekte V2 için): Yok bu dosyada.

**Kritik Fonksiyonlar:**

- `UniswapLikeLiquidity::get_liquidity(&self, pairs, at_block) -> Result<Vec<Liquidity>>` (satır 49-70): `LiquidityCollecting` implementasyonu.
  - `pool_fetcher.fetch(pairs, at_block).await?` — belirli block'ta pool verilerini çek.
  - Her pool için `Liquidity::ConstantProduct(ConstantProductOrder { ... })` oluştur.
  - Token setini topla.

- `Inner::settle(&self, token_amount_in_max, token_amount_out) -> UniswapInteraction` (satır 80-93):
  - `UniswapInteraction` struct'ı oluşturur: router, settlement, amount_out, amount_in_max, token_in, token_out.
  - **DİKKAT:** `amount_out` kesin (exact output), `amount_in_max` slippage dahil maksimum.

- `Inner::router(&self) -> Address` (satır 95-97): Router getter.

- `impl SettlementHandling<ConstantProductOrder> for Inner` (satır 100-112):
  - `encode(execution, encoder)`:
    - `settle(execution.input_max, execution.output)` → `UniswapInteraction`
    - `encoder.append_to_execution_plan_internalizable(swap, execution.internalizable)`

**Matematiksel Formüller:**
- Asıl AMM math `Pool.get_amount_out` ve `Pool.get_amount_in` metodlarında — `liquidity_sources::uniswap_v2` crate'inde.
- UniV2 formül (referans): `amount_out = (amount_in * fee_factor * reserve_out) / (reserve_in * 1000 + amount_in * fee_factor)`
  Burada `fee_factor = 1000 - fee_bps/10`.

**Güvenlik Analizi:**
- `swapTokensForExactTokens` kullanımı: Exact output, max input — kullanıcı fazla ödemez ama işlem input limit aşılırsa revert eder.
- `deadline = U256::MAX`: Transaction'ın deadline'ı yok — settlement contract'ı zaten 2-3 block içinde submit eder, bu yüzden kabul edilebilir. Ancak teorik risk: mempool'da bekleyen transaction eski fiyatla execute edilebilir.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield, `UniswapLikeLiquidity` pattern'ini kendi AMM havuzları için kullanabilir.
- `swapTokensForExactTokens` yerine `swapExactTokensForTokens` da kullanılabilir — sell order'lar için daha doğal.
- Pool fetching `at_block` parametresi kritik: Aynı block'taki fiyatları kullan, reorg koruması.

---

### [liquidity/uniswap_v3.rs] (165 satır)

**Amaç:** UniV3 concentrated liquidity pool'larından likidite toplama ve settlement encoding.

**Struct/Enum/Trait Listesi:**

- `UniswapV3Liquidity` (satır 20-23):
  - `inner: Arc<Inner>`
  - `pool_fetcher: Arc<dyn PoolFetching>`

- `Inner` (satır 24-27):
  - `router: Address`
  - `gpv2_settlement: Address`

- `UniswapV3SettlementHandler` (satır 29-32):
  - `inner: Arc<Inner>`
  - `fee: u32` — pool'un fee tier'ı (100, 500, 3000, 10000)

**Kritik Fonksiyonlar:**

- `ratio_to_u32(ratio: Ratio<u32>) -> Result<u32>` (satır 52-57): Fee dönüşümü.
  - `1/100 → 10000` (1%)
  - `3/1000 → 3000` (0.3%)
  - `5/10000 → 500` (0.05%)
  - `1/10000 → 100` (0.01%)
  - Hesaplama: `ratio * 1_000_000 |> to_integer`

- `UniswapV3Liquidity::get_liquidity(&self, pairs, block) -> Result<Vec<Liquidity>>` (satır 80-109):
  - `pool_fetcher.fetch(&pairs, block).await?`
  - Her pool için: `ensure!(pool.tokens.len() == 2, ...)` — sadece 2-token pool
  - `Liquidity::Concentrated(ConcentratedLiquidity { tokens, settlement_handling, pool })`

- `UniswapV3SettlementHandler::settle(&self, input_max, output) -> UniswapV3Interaction` (satır 112-131):
  - `ExactOutputSingleParams { tokenIn, tokenOut, fee, recipient, amountOut, amountInMaximum, sqrtPriceLimitX96: U160::ZERO }`
  - `sqrtPriceLimitX96 = 0`: Fiyat limiti yok — settlement contract'ı zaten slippage uyguladı.

- `impl SettlementHandling<ConcentratedLiquidity> for UniswapV3SettlementHandler` (satır 134-146):
  - `encode(execution, encoder)`:
    - `settle(execution.input_max, execution.output)` → `UniswapV3Interaction`
    - `encoder.append_to_execution_plan_internalizable(swap, execution.internalizable)`

**Matematiksel Formüller:**
- UniV3 swap math (Tick bazlı):
  `sqrt_price_X96` üzerinden Q64.96 fixed-point arithmetic.
  `amount_out = liquidity * (sqrt_price_upper - sqrt_price_lower)` (basit hali)
  Asıl hesaplama `liquidity_sources::uniswap_v3` crate'inde, pool tick'leri üzerinden.

- Fee Dönüşümü:
  `fee_u32 = rational_fee * 1_000_000`
  Bu UniV3-specific format: 1% = 10000, 0.3% = 3000, 0.05% = 500.

**Güvenlik Analizi:**
- `sqrtPriceLimitX96 = 0`: Fiyat limiti olmadan swap. CoW settlement level'da slippage uygulandığı için (input_max slippage dahil), bu kabul edilebilir. Ancak büyük single-hop swap'larda price impact koruması yoktur.
- `ensure!(pool.tokens.len() == 2)`: Multi-token pool hataları yakalanır.
- `fee.try_into().expect("fee < (1 << 24)")`: u32'den u24'e dönüşüm — UniV3 fee 24-bit.

**DEX Uygulama Notu — Nova Shield için:**
- UniV3 entegrasyonu için `ExactOutputSingle` kullanımı doğru: Exact output guarantee AMM'ler için tercih edilmeli.
- Nova Shield'ın kendi concentrated liquidity AMM'si için tick hesaplamalar `liquidity_sources` katmanında izole edilmeli.
- `sqrtPriceLimitX96 = 0` riski: Nova Shield büyük swap'lar için price limit belirlemeyi düşünmeli, özellikle ince likidite havuzlarında.

---

### [liquidity/balancer_v2.rs] (477 satır)

**Amaç:** Balancer V2 weighted pool ve stable pool likidite sağlayıcısı.

**Struct/Enum/Trait Listesi:**

- `BalancerV2Liquidity` (satır 29-33):
  - `settlement: Address`
  - `vault: Address` — Balancer V2 Vault contract
  - `pool_fetcher: Arc<dyn BalancerPoolFetching>`

- `SettlementHandler` (satır 113-116):
  - `pool_id: B256` — 32-byte pool identifier
  - `inner: Arc<Inner>`

- `Inner` (satır 118-121):
  - `settlement: Address`
  - `vault: Address`

**Kritik Fonksiyonlar:**

- `BalancerV2Liquidity::get_orders(&self, pairs, block) -> Result<(Vec<StablePoolOrder>, Vec<WeightedProductOrder>)>` (satır 48-90): Internal helper.
  - Weighted pool'lar → `WeightedProductOrder` listesi
  - Stable pool'lar → `StablePoolOrder` listesi
  - Her pool için ayrı `SettlementHandler` (Arc shared inner)

- `LiquidityCollecting::get_liquidity` (satır 98-111):
  - `get_orders` → `Liquidity::BalancerStable` + `Liquidity::BalancerWeighted` chain

- `SettlementHandler::swap(&self, input_max, output) -> BalancerSwapGivenOutInteraction` (satır 139-155):
  - **GivenOut swap:** Tam çıktı verilir, input flexible. `input_max.amount` = limit.
  - `user_data = Default::default()` — weighted pool'da kullanılmaz.

- `SettlementHandling<WeightedProductOrder>::encode` ve `SettlementHandling<StablePoolOrder>::encode` (satır 158-176):
  - İkisi de `inner_encode` çağırır.

- `SettlementHandler::inner_encode(&self, execution, encoder)` (satır 179-190):
  - `encoder.append_to_execution_plan_internalizable(Arc::new(self.swap(...)), execution.internalizable)`

**Matematiksel Formüller:**
- **Balancer Weighted Pool (Weighted AMM):**
  `V = prod(B_i^W_i)` — invariant
  `swap: (B_i / W_i) * (1 - (B_i / (B_i + amount_in))^W_i) = amount_out / B_o^W_o` (basitleştirilmiş)
  Asıl: `amount_out = B_o * (1 - (B_i / (B_i + amount_in * (1 - fee)))^(W_i / W_o))`

- **Balancer Stable Pool (StableSwap):**
  `A * n^n * sum(x_i) + D = A * D * n^n + D^(n+1) / (n^n * prod(x_i))`
  `A` = amplification parameter. Yüksek A → düşük slippage (sabit değerli varlıklar için ideal).

- **GivenOut swap kind = 1:** Input miktarı hesaplanır, output sabit tutulur. CoW Protocol bu yaklaşımı tercih eder: Kullanıcının alacağı miktar kesin.

**Güvenlik Analizi:**
- `paused: true` olan pool'lar test'te var — `pool_fetcher` paused pool'ları filtreler mi? Bu soruyu test verisi ile analiz: Test'te `paused: true` olan pool'lar hâlâ döndürülüyor. Filtre `pool_fetcher` crate'inde olmalı.
- `user_data = Default::default()`: Weighted pool'da bu OK. Stable pool için bazı durumlarda `user_data` gerekebilir — bu potansiyel eksiklik.
- `pool_id: B256` — 32-byte havuz kimliği, çarpışma riski yok (Balancer tarafından deterministik üretilir).

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın kendi multi-token AMM'si Balancer'a benzer olabilir — weighted product invariant güçlü bir başlangıç.
- Stable pool (StableSwap) USDC/USDT/DAI gibi stable coin havuzları için kritik. Amplification parameter A'nın governance ile ayarlanabilir olması önemli.
- `GivenOut` swap tercihi: Nova Shield da "exact output" swap'ı tercih etmeli — settlement kesinliği için.
- Balancer vault pattern'i (merkezi settlement için `vault`): Nova Shield kendi vault'unu implement edebilir — tüm token transferleri vault üzerinden, atomik settlement.

---

### [liquidity/zeroex.rs] (383 satır)

**Amaç:** 0x Protocol orderbook'undan limit order likidite toplama ve settlement encoding.

**Struct/Enum/Trait Listesi:**

- `type OrderBuckets = HashMap<(Address, Address), Vec<OrderRecord>>` (satır 28): Token çifti → order listesi haritası.
- `type OrderbookCache = ArcSwap<OrderBuckets>` (satır 29): Lock-free cache.

- `ZeroExLiquidity` (satır 31-35):
  - `zeroex: Arc<IZeroex::Instance>` — 0x contract client
  - `orderbook_cache: Arc<OrderbookCache>` — lock-free cache (`arc_swap` crate)

- `OrderSettlementHandler` (satır 188-191):
  - `order_record: OrderRecord` — 0x order + metadata
  - `zeroex: Arc<IZeroex::Instance>`

**Kritik Fonksiyonlar:**

- `ZeroExLiquidity::new(api, zeroex, gpv2, blocks_stream) -> Self` (satır 38-54):
  - Background task spawn: `run_orderbook_fetching` her yeni block'ta 0x API'yi çeker.
  - `arc_swap::ArcSwap` kullanımı: Lock-free read, write atomik.

- `ZeroExLiquidity::record_into_liquidity(&self, record: OrderRecord) -> Option<Liquidity>` (satır 57-82):
  - `sell_amount = U256::from(record.remaining_maker_amount().ok()?)` — kalan maker amount
  - Sıfır amount filtresi
  - `LimitOrder` oluştur: `kind = OrderKind::Buy` (0x order'ı: maker sell → taker buy)
  - `partially_fillable = true` — 0x order'lar her zaman kısmi doldurulabilir

- `ZeroExLiquidity::run_orderbook_fetching(api, blocks_stream, cache, gpv2)` (satır 84-113): **ARKA PLAN GÖREVI**
  - Her block'ta iki sorgu:
    1. `OrdersQuery::default()` — herkese açık order'lar
    2. `OrdersQuery { sender: Some(gpv2_address) }` — sadece settlement contract'ın fill edebileceği order'lar
  - `group_by_token_pair()` → cache güncelleme (`orderbook_cache.store`)
  - Hata durumunda: `tracing::error!` ve boş liste (`unwrap_or_else`)

- `LiquidityCollecting::get_liquidity` (satır 117-134):
  - Cache'den load: `self.orderbook_cache.load()` — lock-free read.
  - `get_useful_orders(cache, &pairs, 5)` — her çift için max 5 order
  - `record_into_liquidity` map ile filtrele

- `group_by_token_pair(orders) -> HashMap<(Address, Address), Vec<OrderRecord>>` (satır 136-149):
  - Token pair → order listesi grupla.
  - `TokenPair::new(taker_token, maker_token)` — geçersiz pair'leri filtrele (aynı iki token gibi).

- `get_useful_orders(buckets, pairs, orders_per_type) -> Vec<OrderRecord>` (satır 152-185): **ORDER SEÇİMİ**
  - Her token çifti için:
    1. Eğer order sayısı `2 * orders_per_type`'tan azsa → hepsini al.
    2. Yoksa iki kritere göre seç:
       - **Fiyat bazlı:** `maker_amount / taker_amount` en yüksek `orders_per_type` adet. Sort ascending → son `orders_per_type` elementi al.
       - **Hacim bazlı:** `remaining_fillable_taker_amount` en yüksek `orders_per_type` adet. Sort ascending → reverse, take.

- `OrderSettlementHandler::encode(execution, encoder)` (satır 199-213):
  - `u128::try_from(execution.filled)?` — 0x sadece u128 amount destekler.
  - `ZeroExInteraction { taker_token_fill_amount, order, zeroex }` → `encoder.append_to_execution_plan`.

**Matematiksel Formüller:**
- **0x Order Fiyatı:** `price = maker_amount / taker_amount`
  - maker_token = satılan token, taker_token = alınan token
  - Yüksek oran = daha iyi fiyat (maker daha fazla veriyor)

- **Best Price Selection:**
  `sort_by(price_1.total_cmp(price_2))` → `drain(len - N..)` son N = en iyi N fiyat

- **Best Volume Selection:**
  `sort_by_key(remaining_fillable_taker_amount)` → `rev().take(N)` = en yüksek N hacim

**Güvenlik Analizi:**
- **Lock-free cache:** `arc_swap::ArcSwap` — reader'lar block etmiyor. Background task günceller, reader'lar eski versiyonu tutabilir (eventual consistency).
- **Block başına güncelleme:** Her yeni block'ta 0x API fetch. 12 saniyede bir. Stale order riski minimal.
- **Zero amount filtresi:** Sıfır amount order'lar `record_into_liquidity`'de filtrelenir — solver hatası engellenir.
- **u128 limit:** 0x order amount'ları u128 ile sınırlı — çok büyük order'lar için sorun olabilir (ancak pratikte U256 seviyesinde miktarlar zaten 0x orderbook'ta bulunmaz).
- **Hata yönetimi:** API hatası → `error!` log, boş liste. Circuit breaker yok — sürekli hata durumunda sessizce boş likidite sağlanır.
- **Sadece 2 sorgu:** `default` + `sender=gpv2`. MEV bot'lar başka `sender` ile gelirse görünmez. Kasıtlı tasarım.

**DEX Uygulama Notu — Nova Shield için:**
- 0x entegrasyonu, Nova Shield'ın başlangıç döneminde dış likidite kaynağı olarak kullanılabilir.
- `orders_per_type = 5` limiti: Nova Shield için başlangıç değeri. Yüksek volatilite dönemlerinde daha fazla order sample edilebilir.
- **Price + Volume dual selection** (hem en iyi fiyat hem en büyük hacim): İki boyutlu optimizasyon. Nova Shield için 3. kriter eklenebilir: order expiry (yakında sona erecek order'lara öncelik).
- `arc_swap` pattern: Nova Shield'ın cache katmanı da lock-free olmalı. Yüksek frekanslı trading ortamında mutex lock'lar bottleneck oluşturur.
- **Background goroutine pattern:** Her yeni block'ta dış API refresh. Nova Shield için WebSocket subscription ile block-triggered fetch daha verimli.

---

### [liquidity_collector.rs] (256 satır)

**Amaç:** Birden fazla liquidity source'u tek bir `LiquidityCollecting` interface'i altında toplar; lazy initialization desteği.

**Struct/Enum/Trait Listesi:**

- `LiquidityCollecting` trait (satır 12-18):
  - `async fn get_liquidity(pairs: HashSet<TokenPair>, at_block: Block) -> Result<Vec<Liquidity>>`

- `LiquidityCollector` (satır 20-23): Aggregator.
  - `liquidity_sources: Vec<Box<dyn LiquidityCollecting>>`
  - `base_tokens: Arc<BaseTokens>`

- `BackgroundInitLiquiditySource<L>` (satır 54-56): Lazy init wrapper.
  - `liquidity_source: Arc<RwLock<Option<L>>>`

- `Metrics` (satır 131-135): Prometheus metric.
  - `liquidity_enabled: IntGaugeVec` — her source için binary (0/1) enabled/disabled

**Kritik Fonksiyonlar:**

- `LiquidityCollector::get_liquidity` (satır 27-46):
  - `pairs = base_tokens.relevant_pairs(pairs)` — sadece base token üzerinden geçen pair'leri filtrele.
  - `futures::future::join_all(futures)` — TÜM sources paralel fetch.
  - Flatten + collect — tüm AMM'leri tek `Vec<Liquidity>` içinde birleştir.

- `BackgroundInitLiquiditySource::new(label, init, retry_init_timeout, reinit_interval) -> Self` (satır 62-112):
  - Arka planda init task spawn eder.
  - Başarısız init: `retry_init_timeout` bekle, tekrar dene.
  - Başarılı init: `inner.write().await = Some(source)`.
  - `reinit_interval: Some(duration)`: Periyodik re-init (bazı source'lar zaman zaman refresh gerektirir).
  - Metric: `liquidity_enabled.with_label_values(&[label]).inc()` — init başarılı.

- `BackgroundInitLiquiditySource::get_liquidity` (satır 119-128):
  - `liquidity_source.read().await` → `Some(source)` → `source.get_liquidity(...)`.
  - `None` → `Ok(vec![])` — initialize olmadan boş döner.

**Güvenlik Analizi:**
- `RwLock` kullanımı: Çoklu reader, tek writer. Init tamamlandıktan sonra sadece read. Tokio async RwLock.
- Hata durumunda: `warn!` log + sleep. Panic yok. Graceful degradation.
- `reinit_interval`: Periyodik refresh bazı kaynaklarda gerekli (Örn: graph protokol indexing gecikmesi).
- Metric tracking: Hangi source'un enabled/disabled olduğu izlenebilir. Production monitoring için kritik.

**Test Analizi:**
- `delayed_init`: 3 attempt ile init. İlk 2 başarısız, 3. başarılı. Counter doğrulanır.
- `reinit`: 50ms içinde 5-6 init cycle. Periyodik re-init testi.

**DEX Uygulama Notu — Nova Shield için:**
- `LiquidityCollector` pattern'i Nova Shield için doğrudan uygulanabilir. Farklı DEX'lerden gelen likidite tek interface altında.
- `base_tokens.relevant_pairs()`: Nova Shield'ın kendi "routing token" (WETH/USDC/ETH benzeri) konsepti olmalı. Direkt pair yoksa routing token üzerinden path hesapla.
- `BackgroundInitLiquiditySource`: Graph subgraph veya RPC-bazlı likidite kaynakları için ideal — bağlantı kopuksa sistem ayakta kalır.
- **Paralel fetch:** `join_all` tüm source'ları paralel çeker. Nova Shield da bunu yapmalı. Sıralı fetch yerine paralel → latency düşer.

---

### [interactions/mod.rs] (15 satır)

**Amaç:** Interaction tiplerini re-export eder.

Public re-export'lar:
- `BalancerSwapGivenOutInteraction`
- `Erc20ApproveInteraction`
- `UniswapInteraction`
- `UniswapV3Interaction`
- `UnwrapWethInteraction`
- `ZeroExInteraction`

---

### [interactions/balancer_v2.rs] (135 satır)

**Amaç:** Balancer V2 Vault'a `swap()` calldata üretir (GivenOut tipinde).

**Struct/Enum/Trait Listesi:**

- `BalancerSwapGivenOutInteraction` (satır 15-22):
  - `settlement: Address`
  - `vault: Address`
  - `pool_id: B256`
  - `asset_in_max: TokenAmount` — max girilecek token + miktar
  - `asset_out: TokenAmount` — kesin çıkacak token + miktar
  - `user_data: Bytes` — pool-spesifik ek veri

- `NEVER: LazyLock<U256>` (satır 27): `U256::from(1) << 255` — `0x8000...0000`. Büyük deadline. Gas optimizasyonu: Çoğu byte 0.

**Kritik Fonksiyonlar:**

- `BalancerSwapGivenOutInteraction::encode_swap(&self) -> EncodedInteraction` (satır 30-56):
  - `IVault::SingleSwap { poolId, kind: 1 (GivenOut), assetIn, assetOut, amount (output), userData }`
  - `IVault::FundManagement { sender: settlement, fromInternalBalance: false, recipient: settlement, toInternalBalance: false }`
  - `swapCall { singleSwap, funds, limit: asset_in_max.amount, deadline: *NEVER }`
  - Döndür: `(vault, U256::ZERO, calldata)`

**Matematiksel Formüller:**
- `NEVER = 2^255`: Maximum timestamp. Calldata'da: `0x80 00 00 ... 00` — ilk byte hariç sıfır, calldata gaz maliyeti düşük.
- `kind = 1` (GivenOut): Vault'a "bu kadar output ver, gerekeni al" demek. `kind = 0` (GivenIn) olsaydı: "bu kadar input ver, ne çıkarsa çıksın".

**Güvenlik Analizi:**
- `fromInternalBalance = false`, `toInternalBalance = false`: Basit external balance kullanımı. `true` olursa gas tasarrufu ama karmaşıklık artar.
- `limit = asset_in_max.amount`: Slippage protection. Bu limit aşılırsa Balancer revert eder.
- Test: ABI encoding byte-level doğrulanmış (Ethers.js referans encoding ile karşılaştırma).

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield kendi vault'u olursa `toInternalBalance = true` ile internal balance optimizasyonu eklenebilir. Bu aynı settlement'daki ardışık swap'larda token transferini elimine eder.
- `NEVER` deadline pattern'i: Gas optimizasyonu için akıllıca. Nova Shield da aynı tekniği kullanabilir.

---

### [interactions/erc20.rs] (65 satır)

**Amaç:** ERC20 `approve()` calldata üretir.

**Struct/Enum/Trait Listesi:**

- `Erc20ApproveInteraction` (satır 13-17):
  - `token: Address`
  - `spender: Address`
  - `amount: U256`

**Kritik Fonksiyonlar:**

- `Erc20ApproveInteraction::as_encoded(&self) -> EncodedInteraction` (satır 20-31):
  - `ERC20::approveCall { spender, amount }.abi_encode()`
  - Döndür: `(token, U256::ZERO, calldata)`

**Güvenlik Analizi:**
- Sonsuz approve (`U256::MAX`) riski: Mevcut kod `amount` parametresi ile kontrol edilir. Kullanım noktasında dikkat gerekir.
- `0x095ea7b3` function selector: ERC20 `approve` standard selector.

---

### [interactions/uniswap_v2.rs] (89 satır)

**Amaç:** UniV2 `swapTokensForExactTokens` calldata üretir.

**Struct/Enum/Trait Listesi:**

- `UniswapInteraction` (satır 11-18):
  - `router: Address`
  - `settlement: Address`
  - `amount_out: U256` — kesin alınacak miktar
  - `amount_in_max: U256` — max ödenecek miktar (slippage dahil)
  - `token_in: Address`
  - `token_out: Address`

**Kritik Fonksiyonlar:**

- `UniswapInteraction::encode_swap(&self) -> EncodedInteraction` (satır 27-37):
  - `swapTokensForExactTokens { amountOut, amountInMax, path: [token_in, token_out], to: settlement, deadline: U256::MAX }`
  - Selector: `0x8803dbee`
  - `deadline = U256::MAX` — sonsuz deadline (settlement contract hızlı submit eder)

**ABI Calldata Formatı (test'ten doğrulanmış):**
```
bytes[0-3]:    0x8803dbee (selector)
bytes[4-35]:   amountOut (uint256)
bytes[36-67]:  amountInMax (uint256)
bytes[68-99]:  path offset (uint256 = 160)
bytes[100-131]:to (address, 12 byte padding)
bytes[132-163]:deadline (uint256)
bytes[164-195]:path length (uint256 = 2)
bytes[196-227]:token_in (address, 12 byte padding)
bytes[228-259]:token_out (address, 12 byte padding)
```

**Güvenlik Analizi:**
- `deadline = U256::MAX`: MEV koruması açısından zayıf — settlement aynı block'ta yapılmasa bile transaction geçerli kalır. Pratikte settlement 2-3 block içinde yapılır, risk düşük.
- Direkt path (tek hop): `[token_in, token_out]` — multi-hop routing yok. Daha karmaşık route'lar için `liquidity_sources` katmanında path aggregation yapılmalı.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield için `swapTokensForExactTokens` yerine kendi AMM'inin `exactOutputSwap` interface'i kullanılmalı.
- Multi-hop path desteği: `path` array'ini dinamik oluşturabilecek encoding katmanı gerekli.

---

### [interactions/uniswap_v3.rs] (32 satır)

**Amaç:** UniV3 `exactOutputSingle` calldata üretir.

**Struct/Enum/Trait Listesi:**

- `UniswapV3Interaction` (satır 14-17):
  - `router: Address`
  - `params: ExactOutputSingleParams`

**Kritik Fonksiyonlar:**

- `Interaction::encode(&self) -> EncodedInteraction` (satır 19-31):
  - `exactOutputSingleCall { params }.abi_encode()`
  - Döndür: `(router, U256::ZERO, calldata)`

**ExactOutputSingleParams alanları (contracts crate'den):**
- `tokenIn`, `tokenOut`, `fee`, `recipient`, `amountOut`, `amountInMaximum`, `sqrtPriceLimitX96`

**Güvenlik Analizi:**
- `sqrtPriceLimitX96 = 0`: Fiyat limit yok. Settlement-level slippage koruması (`amountInMaximum`) yeterli.

---

### [interactions/weth.rs] (111 satır)

**Amaç:** WETH'i ETH'e unwrap eder (`withdraw` calldata üretir). Birden fazla unwrap'i merge eder.

**Struct/Enum/Trait Listesi:**

- `UnwrapWethInteraction` (satır 9-12):
  - `weth: WETH9::Instance` — WETH9 contract client
  - `amount: U256`

**Kritik Fonksiyonlar:**

- `UnwrapWethInteraction::merge(&mut self, other: &Self) -> Result<()>` (satır 19-30):
  - Adres uyumu kontrolü: `self.weth.address() == other.weth.address()`
  - `self.amount = self.amount.checked_add(other.amount).expect("no one is that rich")`
  - Hata: farklı WETH contract adresi

- `Interaction::encode(&self) -> EncodedInteraction` (satır 33-40):
  - `weth.withdraw(self.amount).calldata()` → selector `0x2e1a7d4d`
  - Döndür: `(*weth.address(), U256::ZERO, calldata)`

**Güvenlik Analizi:**
- `checked_add(...).expect(...)`: Taşma halinde panic. "No one is that rich" yorumu — pratikte U256::MAX WETH imkansız. Ancak production'da `expect` yerine `checked_add().context(...)` daha güvenli olurdu.
- Adres kontrolü: Aynı WETH contract'ı olması zorunlu — farklı chain'lerde farklı WETH adresleri var.

**Test Analizi:**
- `encode_unwrap_weth`: Calldata format doğrulaması. 36 byte: 4 (selector) + 32 (amount).
- `merge_same_native_token`: 1 + 2 = 3. OK.
- `merge_different_native_token`: Farklı adres → Err.
- `merge_u256_overflow`: `1 + U256::MAX` → panic (should_panic test).

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'ın native token sistemi için benzer pattern: "Wrap/Unwrap native token" modülü.
- Merge optimizasyonu: Aynı settlement içindeki tüm unwrap'leri merge etmek önemli gas tasarrufu sağlar. Nova Shield da bu pattern'i uygulamalı.

---

### [interactions/zeroex.rs] (49 satır)

**Amaç:** 0x Protocol `fillOrKillLimitOrder` calldata üretir.

**Struct/Enum/Trait Listesi:**

- `ZeroExInteraction` (satır 9-13):
  - `order: Order` — 0x limit order
  - `taker_token_fill_amount: u128` — fill edilecek taker token miktarı (u128!)
  - `zeroex: Arc<IZeroex::Instance>`

**Kritik Fonksiyonlar:**

- `Interaction::encode(&self) -> EncodedInteraction` (satır 16-47):
  - `fillOrKillLimitOrder(LimitOrder, Signature, taker_fill_amount)`
  - `LimitOrder` fields: makerToken, takerToken, makerAmount, takerAmount, takerTokenFeeAmount, maker, taker, sender, feeRecipient, pool, expiry, salt
  - `Signature` fields: signatureType, v, r, s
  - Döndür: `(*zeroex.address(), U256::ZERO, calldata)`

**Güvenlik Analizi:**
- `fillOrKillLimitOrder`: "Kill" = eğer tam fill edilemezse revert et. CoW Protocol bunu tercih eder — partial fill yerine fail-safe.
- `u128` fill amount: 0x Protocol kısıtlaması. U256 değerler bu kadar büyük olamaz.
- `sender` field: Belirli sender gerektiren order'lar için. `sender = gpv2_settlement` ise sadece CoW settle edebilir.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield, 0x gibi external protocol'lerle entegrasyon için bu pattern'i kullanabilir.
- `fillOrKillLimitOrder` vs `fillLimitOrder`: Nova Shield partial fill semantiği istiyorsa `fillLimitOrder` tercih edilmeli.

---

## Genel Mimari Değerlendirmesi

### CoW Protocol solver/ crate'inin Güçlü Yanları

1. **Trait abstraction:** `Settleable` + `SettlementHandling` pattern her liquidity tipini aynı encoding pipeline'a dahil eder. Yeni liquidity tipi eklemek minimum değişiklik gerektirir.

2. **Deterministic encoding:** Token array sort invariantı, aynı settlement'ın her seferinde aynı calldata üretmesini sağlar.

3. **Gas optimizasyonları:**
   - WETH unwrap merge
   - Unnecessary token/price pruning
   - Internalizable interaction skipping
   - NEVER deadline (mostly-zero bytes)

4. **Overflow koruması:** `checked_*` aritmetik her yerde kullanılmış. `BigRational` hassas slippage hesabı için.

5. **Liquidity order custom pricing:** UCP ile limit order'ların aynı settlement'da coexistence — elegant tasarım.

6. **Background init pattern:** Likidite source'ları delayed init + retry + periyodik reinit destekler.

### Zayıf Yanlar / Teknik Borç

1. **`as_any()` hack:** `SettlementHandling` trait'indeki downcasting. "Should eventually be purged with fire" yorumu — trait object sistemi kısıtlaması.

2. **`deadline = U256::MAX`:** UniV2 interaction'da sonsuz deadline. Settlement 2-3 block içinde yapılır ama uzun bekleme halinde risk var.

3. **`checked_add(...).expect("no one is that rich")`:** WETH unwrap merge'de panic potansiyeli. Daha iyi: `Result<()>`.

4. **Solver algoritması yok:** `solver.rs` adıyla anılan dosya aslında solver algorithm içermiyor — isim yanıltıcı. Gerçek routing ve CoW matching başka crate'te.

5. **0x single-hop limit:** `interactions/uniswap_v2.rs`'de path `[token_in, token_out]` — multi-hop yok. Derin likidite için multi-hop routing gerekli.

6. **Paused pool filtresi belirsizliği:** `paused: true` olan Balancer pool'lar `pool_fetcher` crate'inde filtrelenmeli ama bu dosyada açık değil.

---

## Nova Shield DEX — Direkt Eylem Planı

### Öncelik 1: Settlement Encoder (Kritik Altyapı)

`settlement/settlement_encoder.rs`'deki pattern **direkt uyarlanmalı:**

1. `tokens: Vec<Address>` sorted invariant koru.
2. `TokenReference::Indexed` (UCP trade'ler) ve `TokenReference::CustomPrice` (limit order'lar) ayrımı uygula.
3. `finish()` metodundaki 6 adımlı encoding süreci:
   - Drop unnecessary → UCP vec → Custom price append → Trade encode → Interaction encode

4. **Geliştirme:** CoW, custom price'ı sadece limit order ve liquidity order için kullanıyor. Nova Shield, "maker rebate" veya "taker discount" için ek token reference tipi ekleyebilir.

### Öncelik 2: Slippage Calculator

`liquidity/slippage.rs`'deki sistem **direkt kullanılabilir:**

1. `SlippageCalculator::from_bps(relative_bps, absolute)` — konfigürasyondan parametreler.
2. `SlippageContext` per-auction bağlamı.
3. Input token → output token → default fallback cascadesi.

**Nova Shield geliştirmesi:** Volatilite göstergesi (örn. 24h price range) ile dinamik `relative_bps` hesabı.

### Öncelik 3: Liquidity Abstraction

`liquidity/mod.rs`'deki trait sistemi **direkt uyarlanmalı:**

```rust
// Nova Shield Liquidity enum örneği
pub enum Liquidity {
    ConstantProduct(ConstantProductOrder),  // UniV2 klon'lar
    WeightedProduct(WeightedProductOrder),   // Balancer klon
    StableSwap(StablePoolOrder),             // Curve klon
    LimitOrder(LimitOrder),                  // Orderbook
    Concentrated(ConcentratedLiquidity),     // UniV3 klon
    NativeNova(NovaAMMOrder),               // Nova'nın kendi AMM'i
}
```

### Öncelik 4: Liquidity Collector

`LiquidityCollector` + `BackgroundInitLiquiditySource` pattern'i:

1. Farklı DEX ve pool tiplerini tek interface altında topla.
2. Background init + retry ile güvenilir başlatma.
3. Prometheus metric ile kaynak durumu izleme.

### Öncelik 5: Interaction Encoding

Her liquidity tipi için ayrı interaction struct:

- `NativeNovaSwapInteraction` (Nova Shield native AMM)
- `CrossChainSwapInteraction` (köprü protokolü)
- `FlashloanInteraction` (flashloan pre-interaction)

### Geliştirme Fırsatları (CoW'dan Daha İyi)

1. **Çok boyutlu order seçimi (ZeroEx'ten):** Price + Volume + Expiry üç kriterli seçim.

2. **Internal balance optimizasyonu (Balancer'dan):** `toInternalBalance = true` ile ardışık swap'larda token transfer elimine edilir. Nova Shield vault'u bunu desteklemeli.

3. **Dinamik slippage:** Oracle volatilite verisiyle `relative_bps` dinamik hesaplama.

4. **Multi-hop routing:** UniV2 interaction'da direkt path. Nova Shield için graph-based multi-hop path finding.

5. **Revert risk sınıflandırması:** `Revertable::NoRisk | HighRisk` ikileminden çıkıp `Risk::Score(f64)` veya `Risk::Probability(BPS)` gibi granüler.

6. **Settlement simulation:** CoW encoding oluşturur, simülasyon başka crate'te. Nova Shield encode + simulate pipeline'ı daha entegre yapabilir.

---

## Özet İstatistikler

| Dosya | Satır | Test Sayısı | Kritik Yapı |
|-------|-------|-------------|-------------|
| lib.rs | 6 | 0 | BaselineSolvable impl |
| solver.rs | 79 | 0 | Arn wrapper |
| settlement/mod.rs | 462 | 9 | Trade, Settlement, TradeExecution |
| settlement/settlement_encoder.rs | 1144 | 13 | SettlementEncoder, TokenReference |
| liquidity/mod.rs | 479 | 1 | Liquidity enum, Settleable trait |
| liquidity/slippage.rs | 248 | 1 | SlippageCalculator, SlippageContext |
| liquidity/uniswap_v2.rs | 113 | 0 | UniswapLikeLiquidity |
| liquidity/uniswap_v3.rs | 165 | 1 | UniswapV3Liquidity, ratio_to_u32 |
| liquidity/balancer_v2.rs | 477 | 2 | BalancerV2Liquidity, SettlementHandler |
| liquidity/zeroex.rs | 383 | 5 | ZeroExLiquidity, get_useful_orders |
| liquidity_collector.rs | 256 | 2 | LiquidityCollector, BackgroundInit |
| interactions/mod.rs | 15 | 0 | Re-exports |
| interactions/balancer_v2.rs | 135 | 1 | BalancerSwapGivenOutInteraction |
| interactions/erc20.rs | 65 | 1 | Erc20ApproveInteraction |
| interactions/uniswap_v2.rs | 89 | 1 | UniswapInteraction |
| interactions/uniswap_v3.rs | 32 | 0 | UniswapV3Interaction |
| interactions/weth.rs | 111 | 3 | UnwrapWethInteraction |
| interactions/zeroex.rs | 49 | 0 | ZeroExInteraction |
| **TOPLAM** | **~3.907** | **~40** | |
