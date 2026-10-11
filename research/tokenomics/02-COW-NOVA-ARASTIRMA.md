# Nova Shield Chain — CoW Protocol Araştırması
## CoW Protocol'den Öğrenilecekler: Tam Uygulama Rehberi
**Tarih:** 2026-07-22
**Kaynak:** 30 analiz dosyası, 828+ dosya, ~354.000 satır kaynak kod
**Hazırlayan:** CoW Protocol araştırma agentleri + master sentez

---

## A. TOKEN EKONOMİSİ (COW TOKEN)

### COW Token Utility ve Değer Yakalama Mekanizması

CoW Protocol'ün COW token'ı:
- **Governance:** Protocol parametrelerini (fee oranları, solver whitelist, partner fee cap'leri) DAO oylamasıyla belirler.
- **Solver Bonding:** Solver olmak için COW stake edilmesi gerekir. Stake miktarı solver'ın "güvenilirlik teminatı" işlevi görür. Miktarlar governance ile belirlenir.
- **Protocol Fee Sink:** Protocol fee'leri toplanır ve belirli orana göre COW buyback/burn için kullanılır.

Kaynak: `configs/src/autopilot/` ve `model/src/fee_policy.rs`

COW'da değer yakalama mekanizması şu döngüdür:
```
Kullanıcı trade yapar
  → Protocol fee (surplus/volume/price_improvement) kesilir
  → Fee havuzda birikir
  → Governance kararıyla: buyback + burn veya treasury
```

### Protocol Fee Akışı — CoW'daki Gerçek Mekanizma

CoW Protocol 3 fee politikası uygular (kaynak: `model/src/fee_policy.rs`, `scoring.rs`):

**1. Surplus Fee:**
```
fee = surplus_after_fee * (factor / (1 - factor))
```
- `factor` tipik değeri: 0.5 (surplus'un %50'si)
- Max volume cap: `min(fee_from_surplus, volume * max_volume_factor)`

**2. Volume Fee:**
```
Sell order: fee = traded_buy_amount * factor / (1 - factor)
Buy order:  fee = traded_sell_amount * factor / (1 + factor)
```

**3. Price Improvement Fee:**
```
quote_price   = quote.buy_amount / (quote.sell_amount + quote.fee_amount)
actual_price  = actual_buy / actual_sell
improvement   = actual_price - quote_price
protocol_fee  = min(improvement * actual_sell * factor,
                    trade_volume * max_volume_factor)
```

Fee politikaları ters sırada işlenir (`scoring.rs` satır 175). Her fee sonrası custom price yeniden hesaplanır.

**Partner Fee (app_data üzerinden):**
```json
{
  "metadata": {
    "partnerFee": [
      {"bps": 15, "recipient": "0x1234..."},
      {"bps": 5,  "recipient": "0xabcd..."}
    ]
  }
}
```
1 bps = 0.01%. Birden fazla recipient desteklenir.

### COW Token Burn Mekanizması

CoW Protocol'de gerçek on-chain burn yok; surplus settlement kontratında kalır ve solver/protocol arasında paylaşılır. Buyback governance kararıyla gerçekleşir.

### Solver Bonding ve Slashing

CoW Protocol'de solver whitelist modeli:
```solidity
// GPv2AllowListAuthentication.sol
function addSolver(address solver) onlyManager
function removeSolver(address solver) onlyManager
function isSolver(address) view returns (bool)
```

Manager → solver ekler/çıkarır. Slashing yok, whitelist removal var. Kötü davranışlı solver'lar jail edilir (`circuit_breaker` mekanizması). Jail: geçici banlama, sonra removal.

Autopilot config'den doğrulanan değerler:
```rust
// configs/src/autopilot/run_loop.rs (doğrulandı)
max_winners_per_auction:  20
max_solutions_per_solver:  3
submission_deadline:        5  // blok
min_solve_time:            10  // saniye
```

### NSC için Exact Öneriler: Fee Dağılımı

Nova Shield'ın mevcut fee modeli (`NOVA_SHIELD_CONTEXT.md`):
```
Base fee:     %50 burn + %50 validator
Protocol fee: %0.25 DEX/Auction
Protocol fee dağılımı: Validator %30, Staker %30, Ekosistem %25, Hazine %15
```

**CoW Protocol'den alınan derslerle iyileştirme:**

```
DEX Batch Auction fee akışı önerisi:
  Toplanan surplus/protocol_fee:
    %30 → NSC burn (deflasyonist baskı, long-term holder teşviki)
    %25 → Validator/Staker ödülü (consensus güvenlik)
    %25 → Solver ödülü (batch kalitesi teşviki)
    %20 → Hazine (protocol büyümesi)

Solver ödülü = marjinal katkı (Vickrey mantığı):
  reward_solver = total_score_with_solver - total_score_without_solver
```

CoW'dan alınan kritik ders: Solver'ları sabit ücretle değil, **marjinal surplus katkısına göre** öde. Bu truthful revelation sağlar.

### Governance Token Olarak NSC — CoW'dan Alınan Dersler

CoW Protocol'de governance değişkenler:
- Fee politikası (surplus_factor, volume_factor, max_volume_factor değerleri)
- Solver whitelist yönetimi (manager adresi)
- Partner fee cap (bps üst sınırı)
- Token güven listesi (trusted token listesi)

**Nova Shield için governance scope:**
- DEX fee oranları (surplus_factor, volume_factor)
- Solver stake gereksinimleri
- Guardian trust score ağırlıkları
- Cross-chain fee oranları
- Slashing parametreleri

NSC minimum governance deposit: mevcut kodda 1,000 NSC (doğrulandı `NOVA_SHIELD_CONTEXT.md`).

---

## B. BATCH AUCTION — TAM PROTOKOL

### Uniform Directional Clearing Price (UDCP) — Tam Açıklama

**Kritik Düzeltme (master sentez acımasız incelemeden):** CoW Protocol "uniform clearing price" değil, **"uniform DIRECTIONAL clearing price"** uygular.

Kaynak: `winner_selection.rs` başlık yorumu (doğrulandı):
```
"...enforcing uniform directional clearing prices. That means all orders
selling the same token must get executed at the same price for that token.
But orders buying that same token may all be settled at a different (but
still uniform) price."
```

Yani her token için **2 fiyat** olabilir: satma fiyatı ve alma fiyatı farklıdır.

**Settlement Kontratı Formülleri (on-chain, `ISettlement.sol` + `encoding.rs`):**

Sell order için buy amount:
```
buyAmount = ceil(executedSellAmount * clearingPrices[sellTokenIndex]
                 / clearingPrices[buyTokenIndex])
```

Buy order için sell amount:
```
sellAmount = executedBuyAmount * clearingPrices[buyTokenIndex]
             / clearingPrices[sellTokenIndex]
```

**Price Vector Kodlaması:**
```
tokens[] = [USDC, WETH, DAI, ...]  (adrese göre sıralı)
clearingPrices[] = [fiyat_usdc, fiyat_weth, fiyat_dai, ...]
// Her fiyat: native token (ETH) cinsinden 1 token atomu değeri
// USDC(6 dec): ~1_000_000_000_000 (= 1e-6 ETH/USDC)
// WETH(18 dec): 1_000_000_000_000_000_000 (= 1 ETH/WETH)
```

### Clearing Price Algoritması — Tam Adımlar

Kaynak: `encoding.rs` (471 satır), `trade.rs` (557 satır), `scoring.rs` (450 satır)

**Adım 1 — Token-Fiyat Vektörü İnşası:**
```rust
// encoding.rs satır 57-65 (doğrulandı):
for (token, amount) in solution.clearing_prices()
    .into_iter()
    .sorted_by_cached_key(|(token, _)| *token)
{
    tokens.push(token);
    clearing_prices.push(amount);
}
```
Token adresi lexicographic sıralı — deterministik.

**Adım 2 — Per-Trade Custom Price Hesabı:**
```rust
// trade.rs satır 292-297 (doğrulandı):
custom_prices.sell = actual_buy_amount   // "cross-term": sell slot'una buy amount
custom_prices.buy  = actual_sell_amount  // "cross-term": buy slot'una sell amount
```
Bu tersine çevrilmiş encoding settlement kontratının ratio hesabını doğru yapar.

**Adım 3 — Limit Price On-Chain Enforcement (GPv2Settlement.sol):**
```
Sell order: actual_buy >= order.buyAmount * executed / order.sellAmount
Buy order:  actual_sell <= order.sellAmount * executed / order.buyAmount
```

**Adım 4 — Fee Zinciri:**
```rust
// fee.rs: Politikalar TERS sırada işlenir
for protocol_fee in self.policies.iter().rev() {
    total += current_trade.protocol_fee(protocol_fee)?;
    current_trade.custom_price = recalculate(total)?;
}
```

**Adım 5 — Settlement Execute:**
```
interactions[0] (pre-hooks) → sell token pull → interactions[1] (main) → buy token pay → interactions[2] (post-hooks)
```

**Edge Case'ler:**
- Fill-or-Kill: `executed + fee == order.target()` tam eşit olmalı
- Partial fill: `executed + fee <= available`
- USDT-like token (approve sıfırlama): `approve(0)` → `approve(amount)` sırası simulate edilir
- Native ETH buy: WETH → ETH unwrap interaction otomatik eklenir
- Flashloan + wrapper: Aynı settlement'ta kullanılamaz (`FlashloanWrappersIncompatible`)

### Pro-Rata Fill Hesabı

Kaynak: `scoring.rs` `surplus_over()` fonksiyonu

```
Partial fill için limit price scaling:

Sell order:
  limit_buy_scaled = order.buy_amount * executed_sell / order.sell_amount
  actual_buy = ceil(executed_sell * price_sell / price_buy)
  surplus = actual_buy - limit_buy_scaled

Buy order:
  limit_sell_scaled = order.sell_amount * executed_buy / order.buy_amount
  actual_sell = executed_buy * price_buy / price_sell
  surplus = limit_sell_scaled - actual_sell
```

`checked_ceil_div` kullanımı kritik — settlement kontratı da ceil div kullanır, tutarsızlık olursa fee hesabı bozulur.

### Surplus Hesabı

**Kullanıcı Surplus (Sell Order):**
```
surplus_in_buy_token = actual_buy - limit_buy_scaled
user_benefit = surplus_in_buy_token * (1 - protocol_fee_factor)
```

**Kullanıcı Surplus (Buy Order):**
```
surplus_in_sell_token = limit_sell_scaled - actual_sell
surplus_in_buy_tokens = surplus_sell * signed_buy / signed_sell  (U512 genişletme!)
user_benefit = surplus_in_buy_tokens * (1 - protocol_fee_factor)
```

**Solver Surplus:**
```
settlement contract'ta kalan = gross_surplus - user_benefit - protocol_fee
solver_reward = marjinal katkı (Vickrey)
```

### OrderUid 56-Byte Format

Kaynak: `model/src/order.rs` satır 743-758 (doğrulandı)

```
[0..32]  = keccak256(0x19 0x01 || domain_separator[32] || hash_struct[32])
[32..52] = owner address (20 byte)
[52..56] = valid_to (u32, big-endian, 4 byte)
```

`hash_struct` 416-byte buffer (doğrulandı `model/src/order.rs` satır 237):
```
[0..32]   = TYPE_HASH = 0xd5a25ba2e97094ad7d83dc28a6572da797d6b3e7fc6663bd93efb789fc17e489
[44..64]  = sell_token
[76..96]  = buy_token
[108..128]= receiver
[128..160]= sell_amount (U256 big-endian)
[160..192]= buy_amount
[220..224]= valid_to (u32, 28 byte padding)
[224..256]= app_data
[256..288]= fee_amount (artık 0)
[288..320]= kind hash
[351]     = partially_fillable (1 byte)
[352..384]= sell_token_balance hash
[384..416]= buy_token_balance hash
result    = keccak256(buf[0..416])
```

**Nova Shield için UID uyarlama:**
- `valid_to`: u32 (2106 limiti) yerine Nova Shield'da slot numarası (u64) kullanılabilir
- `owner` yerine `sender_pubkey [32 byte]` (Ed25519/sr25519 için)
- Böylece UID = 32 + 32 + 8 = 72 byte

### OrderClass Türleri

Kaynak: `model/src/order.rs` satır 886 (doğrulandı)

| Class | Açıklama | Fee |
|-------|----------|-----|
| `Market` | Anında gerçekleşmesi beklenen normal order | Volume fee |
| `Limit` | Belirli fiyata ulaşınca çalışır | Surplus fee |
| `Liquidity` | Solver-internal, sıfır fee, whitelist only | Sıfır |

**Nova Shield için ek class'lar:**
- `ShieldedLimit`: ZK masked amount, limit semantiği
- `CrossChain`: BTC/ETH varlıkları için cross-chain order
- `GuardianApproved`: Guardian threshold'u gerektiren order

---

## C. COİNCİDENCE OF WANTS (COW) — EN KRİTİK BÖLÜM

### CoW Tam Algoritması

CoW (Coincidence of Wants): Aynı batch'te ters yönde emir veren kullanıcıları doğrudan eşleştirme. Hiçbir on-chain AMM çağrısı gerekmez.

**Temel Örnek (master sentez'den, doğrulandı):**
```
Kullanıcı A: "100 USDC ver, en az 0.05 ETH al"
Kullanıcı B: "0.05 ETH ver, en az 95 USDC al"

CoW settlement (uniform clearing price ile):
  A: 100 USDC → 0.0505 ETH (surplus = 0.0005 ETH)
  B: 0.05 ETH → 97 USDC  (surplus = 2 USDC)

Sonuç:
  - Hiçbir AMM swap yok → gas minimal
  - MEV açığı sıfır (mempool'a çıkmadan netting)
  - İki kullanıcı da fazla aldı
```

### A→B ve B→A Emirlerini Eşleştirme Algoritması

Nova Shield'ın batch auction'ına CoW ekleme için gerekli adımlar:

**Adım 1 — Emir Sınıflandırması:**
```rust
struct BatchOrder {
    uid: OrderUid,
    sell_token: TokenId,
    buy_token: TokenId,
    sell_amount: u128,    // maksimum verebileceği
    buy_amount: u128,     // minimum alacağı
    kind: OrderKind,      // Sell | Buy
    partially_fillable: bool,
}

// Token çifti indexi:
type TokenPair = (TokenId, TokenId);  // (küçük, büyük) adres sıralı
let mut pair_map: HashMap<TokenPair, Vec<BatchOrder>> = HashMap::new();
```

**Adım 2 — CoW Tespit (Çapraz Çift Netting):**
```rust
fn find_cow_match(
    a_to_b: &[BatchOrder],  // A token satan, B token alan
    b_to_a: &[BatchOrder],  // B token satan, A token alan
    clearing_price_a: u128,  // A'nın clearing price'ı
    clearing_price_b: u128,  // B'nin clearing price'ı
) -> Option<CowMatch> {
    // Toplam A satışı ve toplam B satışını bul
    let total_a_sell: u128 = a_to_b.iter().map(|o| o.sell_amount).sum();
    let total_b_sell: u128 = b_to_a.iter().map(|o| o.sell_amount).sum();

    // Clearing price oranını kontrol et:
    // total_a_sell * price_a == total_b_sell * price_b ise tam CoW
    // Kısmi eşleşme: min(total_a_sell * price_a, total_b_sell * price_b)

    let value_a = total_a_sell as u256 * clearing_price_a as u256;
    let value_b = total_b_sell as u256 * clearing_price_b as u256;

    if value_a == value_b {
        // Tam CoW: AMM gerekmez
        Some(CowMatch::Full { a_fill: total_a_sell, b_fill: total_b_sell })
    } else {
        // Kısmi CoW: fark AMM'den tamamlanır
        let cow_value = value_a.min(value_b);
        Some(CowMatch::Partial { cow_value, residual_to_amm: value_a.abs_diff(value_b) })
    }
}
```

**Adım 3 — Clearing Price Belirleme:**
CoW eşleşmesi oluştuğunda clearing price şu kısıtları sağlamalı:
```
clearing_price_A / clearing_price_B:
  >= order_A.buy_amount / order_A.sell_amount  (A'nın limit'i)
  >= order_B.buy_amount / order_B.sell_amount  (B'nin limit'i geçmeli)
  ve bu iki sınır arasında bir değer
```

Solver bu aralıkta bir clearing price seçer. Uniform directional: tüm A satanlar aynı price_A'yı görür.

### CoW Surplus Hesabı

```
A'nın surplusu (ETH cinsinden):
  a_surplus_buy_token = actual_eth_received - limit_eth
  a_surplus_ETH = a_surplus_buy_token * native_price_ETH

B'nin surplusu (USDC cinsinden):
  b_surplus_buy_token = actual_usdc_received - limit_usdc
  b_surplus_ETH = b_surplus_buy_token * native_price_USDC

Toplam batch surplus = a_surplus_ETH + b_surplus_ETH
CoW solver reward = toplam surplus - protocol_fee
```

### Nova'nın Batch Auction'ına CoW Ekleme — Sıralı Adımlar

Nova Shield'ın mevcut `nova-auction` crate'inde CoW feature eksik (`NOVA_SHIELD_CONTEXT.md` bug #16: "CoW feature (çapraz çift netting) yok").

**Implementasyon Sırası:**

1. **TokenPair indeksi oluştur:**
```rust
// nova-auction/src/cow.rs
pub struct CowEngine {
    pair_orders: HashMap<TokenPair, PairOrderbook>,
}

pub struct PairOrderbook {
    buy_a_sell_b: Vec<PendingOrder>,  // A alıp B veren
    sell_a_buy_b: Vec<PendingOrder>,  // A verip B alan
}
```

2. **Batch başlangıcında CoW fırsatı tara:**
```rust
pub fn find_cow_opportunities(&self) -> Vec<CowOpportunity> {
    let mut opps = Vec::new();
    for (pair, orderbook) in &self.pair_orders {
        if !orderbook.buy_a_sell_b.is_empty()
            && !orderbook.sell_a_buy_b.is_empty()
        {
            if let Some(opp) = self.compute_cow_match(pair, orderbook) {
                opps.push(opp);
            }
        }
    }
    opps
}
```

3. **CoW fill uygula (limit kontrolüyle):**
```rust
fn apply_cow_fill(
    order: &PendingOrder,
    clearing_price_sell: u128,
    clearing_price_buy: u128,
) -> Result<Fill, CowError> {
    let actual_buy = order.executed_sell
        .checked_mul(clearing_price_sell)?
        .checked_div(clearing_price_buy)?;

    // Limit kontrolü:
    let min_buy_scaled = order.limit_buy
        .checked_mul(order.executed_sell)?
        .checked_div(order.limit_sell)?;

    if actual_buy < min_buy_scaled {
        return Err(CowError::LimitViolation);
    }
    Ok(Fill { buy: actual_buy, sell: order.executed_sell })
}
```

4. **Artık miktarı (residual) solver'a ilet:**
Kısmi CoW fill sonrası kalan fark solver'ın AMM çağrısıyla kapatılır.

5. **Fairness invariant'ı kontrol et:**
```rust
// Her order'ın batch'teki fiyatı tek-order execution'dan iyi olmalı
fn check_fairness(order: &Order, batch_fill: &Fill, single_fill: &Fill) -> bool {
    // Sell order: batch_fill.buy >= single_fill.buy
    // Buy order: batch_fill.sell <= single_fill.sell
}
```

### Multi-Hop CoW: A→B→C→A Zinciri

Üç kullanıcı döngüsü:
```
A: 1 ETH → 3000 USDC (limit: min 2900 USDC)
B: 3000 USDC → 1 SOL (limit: min 0.9 SOL)
C: 1 SOL → 1.02 ETH (limit: min 0.95 ETH)

Ring CoW settlement:
  A → 1 ETH → B (A: 3050 USDC alır, surplus 150 USDC)
  B → 3050 USDC → C (B: 1.02 SOL alır, surplus 0.12 SOL)
  C → 1.02 SOL → A (C: 1.04 ETH alır, surplus 0.09 ETH)

Üç atomik transfer, sıfır AMM, sıfır MEV.
```

Implementasyon: Graph cycle detection — Bellman-Ford veya DFS ile döngü bul, clearing price'ı ring constraint'i sağlayacak şekilde ayarla.

### Kısmi CoW Fill

```rust
// Eşleşen iki order'da biri daha küçükse:
let cow_fill = match (a.sell_amount, b.buy_amount_in_a_terms) {
    (a_sell, b_needs) if a_sell <= b_needs => {
        // A tamamen doldurulur, B kısmen
        CowFill::FullA_PartialB { a_fill: a_sell }
    }
    (a_sell, b_needs) => {
        // A kısmen, B tamamen
        CowFill::PartialA_FullB { cow_portion: b_needs }
    }
};
```

Kısmi fill: `partially_fillable: true` olan order'lar için. Fill-or-Kill order'lar kısmen doldurulamaz — ya tam doldurul ya da atlat.

---

## D. SOLVER KOMPETİSYONU

### Solver Çözüm Hazırlama Süreci

Kaynak: `crates/solvers/src/` (78 dosya, 12,454 satır)

```
Driver → POST /solve → Solver Binary

Solver'ın iç akışı:
  1. Auction deserialize et (orders, tokens, prices, deadline)
  2. Liquidity fetch: UniV2/V3/Balancer pool'larını pull et
  3. Order sıralama:
     - ExternalPrice: sell_value/buy_value > 1 olanlar önce
     - OwnQuotes: solver'ın kendi quote'u varsa önce
     - CreationTimestamp: yeni orderlar önce
  4. Path-finding (baseline solver):
     - Graph: token'lar node, pool'lar edge
     - Bellman-Ford / Dijkstra ile en iyi route
  5. DEX API (varsa): OKX, Bitget API'ya sorgu
  6. Solution encode: clearing_prices + trades + interactions
  7. CIP-38 score hesapla
  8. HTTP response → driver'a gönder
```

Solver timeout: `min_solve_time = 10s` (doğrulandı `configs/src/autopilot/run_loop.rs`)

### Solver Değerlendirme Metrikleri

**CIP-38 Score Formülü (doğrulandı `scoring.rs`):**

Sell order:
```
score = (surplus_in_buy_token + protocol_fees_in_buy_token)
        * native_price_buy_token
```

Buy order (U512 genişletme zorunlu):
```
surplus_in_sell → buy_token:
  buy_equiv = surplus_sell * signed_buy_amount / signed_sell_amount
  // widening_mul: U256 * U256 → U512, sonra U256'ya truncate

score = buy_equiv * native_price_buy_token
```

Solver sıralama:
1. Fairness filtresi: `partition_unfair_solutions()` — baseline score altındaki çözümler elenir
2. Score = 0 olan çözümler elenir
3. CIP-38 score'a göre azalan sıra
4. Max 20 kazanan seçilir (`max_winners_per_auction = 20`)

### VCG (Vickrey-Clarke-Groves) Payment Mekanizması — Tam Formül

CoW Protocol'de **Vickrey benzeri** ödeme mekanizması uygulanır (kaynak: `winner_selection.rs`, `arbitrator.rs`):

```
Solver S için reward hesabı (doğrulandı arbitrator.rs satır 607-644):

1. S'in tüm çözümleri ranking'den çıkar
2. Kalan çözümlerle pick_winners() çalıştır
3. Bu senaryodaki toplam score = reference_score_S
4. Gerçek toplam score = total_score_with_S

reward_S = total_score_with_S - reference_score_S
         = S'in marjinal katkısı
```

Bu Vickrey mantığı: Solver gerçek değerini açıklamak zorunda, fazladan surplus iddia edemez. Çünkü ödülü yalnızca marjinal katkısıdır, toplam surplus değil.

**Nova Shield için VCG Formülü:**
```
Nova Shield'da uyarlama:
  total_batch_surplus = CoW surplus + AMM routing surplus
  reference_surplus_without_S = solver S olmadan elde edilecek toplam surplus

  solver_S_reward = total_batch_surplus - reference_surplus_without_S

  Bu miktarın dağılımı:
    %60 → Solver S (NSC cinsinden ödeme veya fee retention)
    %40 → Protocol burn/treasury
```

### Solver Slashing Koşulları

CoW Protocol'de jail mekanizması (`run_loop.rs` circuit_breaker):
- Settlement başarısız: Jail
- Slippage aşımı: Jail
- Fairness ihlali (kullanıcı tek-order'dan kötü fiyat aldı): Jail
- On-chain settlement failure: Jail → whitelist removal

Jail, geçici banlamadır. Kalıcı removal governance kararı gerektirir.

### Nova Shield için Solver Parametreleri

```
Solver Stake Gereksinimleri (öneri):
  Minimum stake: 10,000 NSC
  Maksimum settlement değeri: stake * 10 (leverage cap)

  Tier sistemi:
    Basic Solver:      10,000 NSC stake → max 100,000 NSC settlement
    Advanced Solver:   50,000 NSC stake → max 500,000 NSC settlement
    Institutional:    100,000 NSC stake → max 1,000,000 NSC settlement

Slashing Koşulları (öneri):
  Wrong clearing price (kullanıcı zarar etti): %50 slash
  Fake solution (simulation'dan geçmeyen): %25 slash
  Extended unresponsive: %10 slash
  Double settlement: %100 slash (tombstone)

Jailing:
  3 başarısız settlement → 1 epoch jail (216,000 slot = 24 saat)
  5 başarısız → 1 hafta jail
  10 başarısız → governance removal
```

---

## E. MEV KORUMASIYLA ENTEGRASYON

### CoW'un MEV'e Karşı Yaklaşımı

CoW Protocol çok katmanlı MEV savunması (kaynak: `mempools.rs`, `settlement.rs`, `slippage.rs`):

**1. Batch Auction (Yapısal Koruma):**
- Order'lar anında execute edilmez, biriktirilir
- Batch içi CoW netting mempool'a hiç çıkmaz
- Frontrunning hedefi yok — tek bir "büyük" tx

**2. Uniform Clearing Price:**
- Sandwich attack fiziksel olarak imkânsız
- İki clearing price = iki ayrı settlement gerektirir
- Aynı batch'te aynı token'ı satan tüm kullanıcılar aynı fiyatı görür

**3. Private Mempool:**
```rust
// mempools.rs satır 72 (doğrulandı select_ok):
select_ok([
    public_mempool.submit(tx),
    flashbots.submit(bundle),
    mev_blocker.submit(tx),
])
// İlk başarılı kazanır
```

**4. Slippage Kısıtlaması:**
```rust
// slippage.rs (doğrulandı):
const DEFAULT_RELATIVE: BigRational = 0.005;  // 0.5%
const DEFAULT_MIN: U256 = 0.001 ETH;
const DEFAULT_MAX: U256 = 0.05 ETH;

let slippage = relative.clamp(self.min, self.max);
```

**5. Gas Price Bump (%13):**
```rust
// mempools.rs (doğrulandı):
const GAS_PRICE_BUMP_PCT: u64 = 13;  // EIP-1559 min %12.5, +buffer
```

**6. Gas Limit:**
```rust
// settlement.rs (doğrulandı):
const GAS_LIMIT_FACTOR: f64 = 2.0;
// EIP-7825 Mainnet Fusaka cap: 2^24 - 1 = 16,777,215 gas/tx
let max_gas = min(block_limit / 2, EIP_7825_CAP);
let limit = min(max_gas, estimate * GAS_LIMIT_FACTOR);
```

### Pre-Confirmation Şifreleme — CoW vs Nova'nın IBE Sistemi

CoW Protocol'de şifreleme yok — MEV koruması mimari (batch auction) ve private mempool ile sağlanır.

Nova Shield'da IBE (Identity-Based Encryption) sistemi `NOVA_SHIELD_CONTEXT.md`'den:
- Emirler IBE ile şifreli: `commitment_hash lexicographic` sıralama
- Eon key: Epoch başında threshold key paylaşımı (Rolling Shutter pattern)
- Validator emirleri göremez — sıralama şifreli hash üzerinden

**İki yaklaşımın karşılaştırması:**
```
CoW Protocol:
  + Batch auction yapısal MEV önler
  + Private mempool Flashbots entegrasyonu
  - Order içerikleri validator'lara görünür
  - Submission anında frontrun riski

Nova Shield IBE:
  + Validator emirleri göremez → sıralama manipülasyonu imkânsız
  + Decryption sadece slot kapanışında
  - Şifreleme/deşifreleme overhead (eon key yönetimi)
  - Eon key threshold paylaşımı karmaşıklığı

Nova Shield'da ikisi birlikte:
  IBE şifreleme (validator göremez) + Batch auction (CoW netting) + Native L1 (private mempool gereksiz)
```

### Açık Artırma Öncesi Gizlilik

CoW Pattern → Nova Shield uyarlama:

```rust
// Nova Shield batch auction order submission:
struct ShieldedOrder {
    // IBE ile şifrelenmiş:
    encrypted_payload: IbeCiphertext,  // sell_token, buy_token, amounts, limit_price
    // Açık (MEV için anlamsız):
    commitment_hash: B256,             // lexicographic sıralama için
    valid_to_slot: u64,               // zaman aşımı
    fee_amount: u128,                 // gas ödeme
}

// Slot kapandığında:
// 1. Eon key ile tüm emirler deşifre edilir (threshold decrypt)
// 2. Batch auction çalışır
// 3. CoW netting ve solver competition
// 4. Settlement
```

### Backrunning, Sandwiching, Frontrunning — CoW Nasıl Engeller

**Frontrunning:**
- CoW'da mümkün değil: Batch auction'da hedef tx mempool'da beklemiyor.
- Emir geldiğinde orderbook'ta bekler, batch'te çözülür.
- Nova Shield IBE: İçerik şifreli, frontrunner hangi emri hedef alacağını bilemez.

**Sandwiching:**
- CoW'da: Uniform clearing price → sandwich'te "önce al, sonra sat" iki ayrı settlement gerektirir → kârsız.
- Nova Shield: Aynı avantaj + IBE ile fiyat bilgisi gizli.

**Backrunning:**
- CoW'da: Private mempool ile settlement tx'i görünmez yapılabilir.
- Nova Shield L1 avantajı: Validator = block producer. Settlement tx'ini bloka önce koymak için external private mempool gerekmez.

### Nova'nın Threshold ECDH Sistemiyle Entegrasyon

Mevcut kod (`nova-mempool`): Threshold ECDH MEV koruması, ancak `NOVA_SHIELD_CONTEXT.md` Bug #1: "Threshold encrypt/decrypt simetrisi bozuk".

CoW'dan alınan ders ile düzeltme yolu:
```rust
// nova-mempool/src/mev_protection.rs

// Sorun: IBE sisteminde eon_key secp256k1[33] vs Ristretto255[32] uyumsuzluğu (Bug #19)
// Çözüm: Tek tip seç

// Öneri: Ristretto255 kullan (Nova ZK sistemiyle uyumlu):
pub struct EonKey {
    public: ristretto255::PublicKey,   // 32 byte
    threshold_shares: Vec<SecretShare>, // threshold dağıtılmış
}

// Rolling Shutter pattern (CoW'un referansı):
// Epoch başında: threshold key generation ceremony
// Epoch boyunca: emirler bu key ile IBE şifreli
// Epoch kapanışında: threshold decrypt
```

---

## F. AMM LİKİDİTE ENTEGRASYONU

### CoW Batch Auction + AMM Entegrasyonu

Kaynak: `crates/liquidity-sources/src/` (~40 dosya), `solver/src/liquidity/`

**Desteklenen AMM Kaynakları:**
| Kaynak | Matematik | Nova Uyarlaması |
|--------|-----------|-----------------|
| UniswapV2 | Constant product x*y=k | Nova iç havuz |
| UniswapV3 | CLMM tick-based | Gelecek sürüm |
| Balancer V2 Weighted | Ağırlıklı AMM | Çoklu token havuzu |
| Balancer V2 Stable | StableSwap amplification | Stable pair |
| ZeroEx | Off-chain API | Dış bridge |

**UniswapV2 Formülü (doğrulandı `pool_fetching.rs`):**
```rust
// fee = Rational { numer, denom }  örnek: numer=3, denom=1000 (= 0.3%)
// fee_factor = denom - numer = 997

fn get_amount_out(
    amount_in: U256,
    reserve_in: U256,
    reserve_out: U256,
    fee: Rational,
) -> U256 {
    let fee_factor = fee.denom - fee.numer;  // 997 for UniV2
    let amount_in_with_fee = amount_in * fee_factor;
    let numerator = amount_in_with_fee * reserve_out;
    let denominator = reserve_in * fee.denom + amount_in_with_fee;
    numerator / denominator
}
// Doğrulanmış formül: amount_out = (in * fee_factor * R_out) / (R_in * denom + in * fee_factor)
```

**Balancer Stable Pool (StableSwap amplification):**
```
Amplification parametresi: A (büyük A → daha flat curve, daha az slippage)
Newton-Raphson iteration ile invariant D hesabı
Decimal normalization: scaling_factors = [10^(18-dec0), 10^(18-dec1), ...]
```

**Gas Maliyetleri (doğrulandı `gas.rs`):**
```
INITIALIZATION_COST  = 32,000
SETTLEMENT_OVERHEAD  = 75,891
TRADE                = 41,000
ERC20_TRANSFER       = 27,513  (medyan %52 percentil)
GAS_PER_WETH_UNWRAP  = 9,223
GAS_PER_WETH_WRAP    = 24,038
GAS_PER_UNISWAP      = 90,171
GAS_PER_ZEROEX_ORDER = 66,358
GAS_PER_BALANCER_SWAP= 88,892
SETTLEMENT_SINGLE_TRADE = 94,891
```

### Liquidity Aggregation — Nova-Auction için

```
Nova Shield liquidity kaynakları (öncelik sırasına göre):

1. CoW Matching (iç): Aynı batch'teki ters emirler — sıfır fee, sıfır gas
2. Nova AMM (iç): Nova Shield iç havuzu — düşük fee
3. Cross-chain vault (dış): BTC/ETH/SOL varlıkları — yıllık %0.05 fee
4. External bridges (dış): IBC, BEEFY — yüksek fee, son çare
```

**Liquidity Aggregation Implementasyonu:**
```rust
// nova-auction/src/liquidity.rs
pub trait LiquiditySource: Send + Sync {
    fn fetch_quote(
        &self,
        sell_token: TokenId,
        buy_token: TokenId,
        sell_amount: u128,
    ) -> BoxFuture<'_, Result<Quote>>;

    fn execution_cost_estimate(&self) -> u64;  // gas/fee equivalent
}

pub struct LiquidityAggregator {
    sources: Vec<Arc<dyn LiquiditySource>>,
}

impl LiquidityAggregator {
    pub async fn best_route(
        &self,
        query: &SwapQuery,
    ) -> Result<Route> {
        // CompetitionEstimator pattern (CoW'dan):
        // Stage 0: iç kaynaklar paralel
        // Stage 1: dış kaynaklar (sadece Stage 0 yetersizse)
        // Ranking: net_value = out_amount - execution_cost
    }
}
```

### Nova'nın Dutch Auction'ı ile Batch Auction Entegrasyonu

Nova Shield'da Dutch auction (NSC/kripto satışı için) + Batch auction (DEX) birlikte çalışmalı:

```
Dutch Auction (NSC emisyon satışı):
  Lineer fiyat düşüşü: price(t) = start_price - (start_price - end_price) * t / duration
  Kullanıcı "al" dediği anda clearing price = o anki Dutch price
  Bu emirler batch auction'a CoW orderi olarak girebilir!

Entegrasyon:
  Dutch auction → order üretir → batch auction'a gönderir
  Batch auction'daki karşı emirlerle CoW netting olabilir
  Kalan kısım on-chain emission'a gönderilir
```

### İç Likidite vs Dış Likidite Kararı

CoW Protocol kararı (trusted token konsepti):
- `token.trusted = true` ise: settlement vault'tan internalize edilebilir (gas tasarrufu)
- `token.trusted = false` ise: tüm interaction'lar explicit (güvenlik)

Nova Shield için:
```rust
pub enum LiquidityDecision {
    CowNetting,          // İç eşleşme — en iyi
    NovaAmmInternal,     // Nova iç havuz — iyi
    NovaAmmExternal,     // Cross-chain vault — orta
    BridgeToExternal,    // Dış bridge — son çare
}

fn decide_liquidity(
    cow_opportunity: Option<CowMatch>,
    internal_amm_quote: Option<Quote>,
    external_quote: Option<Quote>,
) -> LiquidityDecision {
    if cow_opportunity.is_some() {
        return CowNetting;  // Her zaman CoW tercih et
    }
    // net_value = quote.out_amount - quote.execution_cost
    // En yüksek net_value'yu seç
}
```

---

## G. SİPARİŞ YAŞAM DÖNGÜSÜ

### Order Submission → Settlement Tam Akışı

**Adım 1 — Order Gönderme:**
```
POST /api/v1/orders → orderbook (axum server)

Validation pipeline (orderbook.rs + order_validation):
  1. JSON parse → OrderCreation struct
  2. app_data hash doğrula (keccak256 → IPFS CID)
  3. EIP-712 imza doğrula → verify_owner()
     - EIP-712: ECDSA recovery → owner == expected
     - EthSign: EIP-191 personal_sign
     - EIP-1271: simulateDelegatecall → isValidSignature() → 0x1626ba7e
     - PreSign: on-chain preSignature[] kaydı
  4. Token ban kontrolü:
     - deny list (HashSet)
     - Chainalysis Oracle (on-chain)
     - Hermod service (off-chain)
  5. Token kalite: trace_callMany → 8 adım simülasyon (fee-on-transfer tespiti)
  6. Balance simülasyon: simulateDelegatecall → can_transfer?
  7. App data hooks → gas limit hesabı
  8. Quote doğrulama: within_market() kontrolü
  9. DB INSERT
  10. HTTP 201 + OrderUid
```

**Nova Shield için adaptasyon:**
```
POST /rpc/order → nova-rpc
  1. Decode (JSON/Protobuf)
  2. Schnorr imzası doğrula (FROST threshold veya single-key)
  3. Guardian approval kontrolü (shielded order'lar için)
  4. Balance check (ZK shielded: nullifier kontrolü)
  5. IBE ile şifrele (MEV koruması)
  6. Mempool'a ekle (nova-mempool)
  7. OrderUid döner
```

**Adım 2 — Solvable Order Tespiti:**
```sql
-- autopilot solvable_orders.rs SQL (konseptüel):
WHERE valid_to >= now
  AND cancellation IS NULL
  AND class != 'liquidity'
  AND (kind='sell' AND filled < sell_amount)
     OR (kind='buy' AND filled < buy_amount)
  AND NOT EXISTS (SELECT 1 FROM invalidations WHERE uid = order.uid)
```

**Adım 3 — Auction oluşturma → Solver'a gönder (min 10s):**

**Adım 4 — Solver Rekabeti → CIP-38 scoring:**

**Adım 5 — Winner Seçimi → Settlement:**

**Adım 6 — On-chain Settlement:**
```
settle(tokens[], clearingPrices[], trades[], interactions[3])
calldata sonuna auction_id eklenir (off-chain indexing)
```

**Adım 7 — Trade Event İndeksleme → Protocol fee hesabı → Observer.**

### Zaman Aşımı Mekanizması

CoW Protocol'de `valid_to`:
```rust
// order.rs
valid_to: u32  // Unix timestamp saniye cinsinden
// Kontrat: validTo >= block.timestamp
// Orderbook: ValidToTooFarInTheFuture kontrolü var (max ~1 yıl)
```

**Nova Shield için slot-based zaman aşımı:**
```rust
pub struct OrderValidity {
    valid_to_slot: u64,       // slot numarası (400ms * slot = absolute time)
    // Veya:
    valid_for_slots: u32,     // şimdiden itibaren kaç slot
}

// Slot süresi: 400ms
// 1 saat = 9,000 slot
// 1 gün = 216,000 slot (1 epoch)
// Maksimum: 30 gün = 6,480,000 slot
```

### Carryover (Kısmi Fill → Sonraki Batch)

CoW Protocol yaklaşımı:
- Kısmi fill: `filledAmounts[orderUid]` güncellenir
- Sonraki batch'te kalan miktar available: `available = total - filled`
- Order `valid_to` dolana kadar her batch'te tekrar denenir

```rust
// driver/domain/competition/solution/trade.rs (doğrulandı):
match order.partial {
    Partial::Yes { available } => executed_with_fee <= available,
    Partial::No => executed_with_fee == order.target(),  // Fill-or-Kill
}
```

**Nova Shield uyarlama:**
```rust
pub struct OrderState {
    filled_amount: u128,      // kümülatif
    invalidated: bool,
    valid_to_slot: u64,
}

// RocksDB key: order_uid (72 byte) → OrderState
// Her settlement sonrası güncelle
// Expire: valid_to_slot < current_slot → silinebilir (storage geri kazanımı)
```

### Signature Scheme

CoW Protocol'de dört scheme (kaynak: `model/src/signature.rs`, doğrulandı):

| Scheme | Kullanım | Byte |
|--------|----------|------|
| EIP-712 | Standard ECDSA | 65 |
| EthSign | MetaMask legacy | 65 |
| EIP-1271 | Smart contract | arbitrary |
| PreSign | On-chain tx ile | 0/20 |

EIP-712 mesaj hash'i (doğrulandı satır 270):
```
message = [0x19, 0x01] || domain_separator[32] || struct_hash[32]  // 66 byte
eip712_hash = keccak256(message)
```

**Nova Shield için imza scheme'i:**
- Ana scheme: **Schnorr** (FROST threshold için uyumlu, sr25519 veya Ed25519)
- Guardian imzaları: FROST 3/5 threshold Schnorr
- ZK shielded orders: Groth16 spending proof (imza yerine)
- EVM uyumlu işlemler: EIP-712 ECDSA desteği (köprü işlemleri için)

---

## H. FEE YAPISI

### Protocol Fee — Sabit mi, Dinamik mi?

CoW Protocol: Governance kararıyla belirlenen sabit parametreler, dinamik uygulama.

`fee_policy.rs` / `fee_factor.rs` (doğrulandı):
```rust
// FeeFactor: [0.0, 1.0) aralığı, f64
// Ancak on-chain hesaplamada integer kullanılır

// Sabitler (governance default'lar):
surplus_factor: 0.5         // surplus'un %50'si (max)
max_volume_factor: 0.01     // hacmin %1'i (cap)
volume_factor: 0.005        // hacmin %0.5'i
price_improvement_factor: 0.5
```

**Nova Shield için:**

Mevcut kodda `%0.25 DEX/Auction fee` var. CoW'dan öğrenerek:
```
Nova Shield Fee Tablosu (öneri):

Market order (immediate fill):
  Volume fee: 0.20% (20 bps)
  Max: hacmin %0.5'i

Limit order (delayed fill):
  Surplus fee: surplus'un %40'ı
  Max volume cap: hacmin %0.3'ü

Price improvement (quote'dan iyi fill):
  Factor: 0.30 (iyileştirmenin %30'u)
  Max volume cap: hacmin %0.2'si

Shielded order (ZK):
  Base fee + ZK proof surcharge
  ZK proof cost: ~200,000 gas ekivalanı NSC
```

### Solver Ödülü Finansmanı

CoW Protocol: Solver settlement tx'inin gas maliyetini öder, ödülü settlement surplus'tan alır.

Ödül akışı:
```
Settlement surplus = kullanıcı limit'i aşan miktar
  → Protocol fee kesilir
  → Kalan = solver reward (gas + marjinal katkı)

Solver gas ödemesi:
  required_eth = gas_estimate * max_fee_per_gas
  // settlement.rs satır 440: driver kendi tahminini 2x kullanır
  override = estimated * 2  (gas climbing buffer)
```

**Nova Shield'da solver ödülü:**
```
Solver NSC ödülü per batch = batch_surplus * solver_share_factor - gas_cost_nsc
  solver_share_factor = 0.25 (önerilen)
  gas_cost_nsc = zk_proof_cost + settlement_tx_cost (NSC cinsinden)

Negatif olursa solver katılmaz (ekonomik rasyon)
→ Minimum batch büyüklüğü: ekonomik sürdürülebilirlik için
```

### Gas Maliyeti ve Meta-Transaction

CoW Protocol: Kullanıcı gas ödemez — solver öder. Bu "meta-transaction" pattern:
```
Kullanıcı: Order imzalar, gas ödemez (EIP-712 signed intent)
Solver: Settlement tx'i imzalar, gas öder
Solver: Gas maliyetini surplus/fee'den geri alır
```

**Nova Shield için:**
- Kullanıcı: NSC cinsinden order fee belirtir (fee_amount field)
- Solver: Settlement yürütür, fee_amount alır
- ZK proof cost: Fee'ye eklenir veya protocol subsidize eder

### ZK Proof Maliyetinin Fee'ye Yansıtılması

```rust
// nova-auction/src/fee.rs

pub struct ZkOrderFee {
    base_protocol_fee: u128,    // temel DEX fee
    zk_proof_surcharge: u128,   // Groth16 proof verify maliyeti
    shield_complexity_fee: u128, // FullShield vs SemiShield farkı
}

// Gas ekivalanları (NOVA_SHIELD_CONTEXT.md'den):
// FullShield:  200,000 gas ekivalanı
// SemiShield:  80,000 gas ekivalanı
// Standard:    21,000 gas ekivalanı

// ZK fee = proof_gas_ekivalanı * nsc_per_gas_unit
// nsc_per_gas_unit = current_nsc_price / block_base_fee

pub fn calculate_zk_fee(shield_mode: ShieldMode, nsc_per_gas: u128) -> u128 {
    let gas_ekivalani = match shield_mode {
        ShieldMode::Full => 200_000,
        ShieldMode::Semi => 80_000,
        ShieldMode::Standard => 21_000,
    };
    gas_ekivalani * nsc_per_gas
}
```

---

## I. NOVA SHIELD'E ÖZEL UYARLAMA

### ZK Shielded Orders ile Batch Auction Uyumu

Mevcut Nova Shield'da: ZK proof sistemi tamamen mock (`nova-zk`, Bug listesi: "Hiç gerçek ZK kanıtı yok").

CoW Protocol analizi ile shielded order entegrasyonu:

```rust
// nova-auction/src/shielded_order.rs

pub enum OrderKind {
    // Standart (CoW'dan doğrudan alınan):
    Standard(StandardOrder),

    // Shielded: ZK masked amount
    Shielded(ShieldedOrder),
}

pub struct ShieldedOrder {
    // Görünür (MEV için anlamsız, sıralama için gerekli):
    commitment_hash: B256,          // note commitment
    valid_to_slot: u64,
    fee_amount: u128,               // gas ödemesi için açık

    // Gizli (IBE şifreli, batch kapandığında açılır):
    encrypted_note: IbeCiphertext,  // sell_token, buy_token, amounts, limit

    // ZK proof (settlement anında doğrulanır):
    spend_proof: Option<Groth16Proof>,    // nullifier doğrulaması
    balance_proof: Option<Groth16Proof>,  // yeterli bakiye
}

// CoW batch auction'da shielded order handling:
pub fn process_shielded_batch(
    batch: &BatchSlot,
    eon_decryption_key: &EonKey,
) -> Result<Vec<DecryptedOrder>> {
    // 1. Threshold decrypt: IBE → plain orders
    let decrypted = batch.orders.iter()
        .filter_map(|o| decrypt_order(o, eon_decryption_key).ok())
        .collect::<Vec<_>>();

    // 2. ZK proof verify (arkworks/Groth16)
    let verified = decrypted.iter()
        .filter(|o| verify_spend_proof(&o.spend_proof, &o.nullifier).is_ok())
        .cloned()
        .collect::<Vec<_>>();

    // 3. Standart batch auction (CoW netting, solver competition)
    Ok(verified)
}
```

**CoW'dan alınan ders:** Settlement öncesi simulation yap. ZK proof'un geçerli olduğunu settlement sırasında değil, batch kapandığında kontrol et. Geçersiz proof = order düşür.

### Guardian-Protected Accounts için Özel Order Flow

```rust
// nova-auction/src/guardian_order.rs

pub struct GuardianOrder {
    inner: StandardOrder,

    // Threshold approval gereksinimleri:
    required_threshold: u8,          // kaç guardian onayı (örn. 2/3)
    guardian_signatures: Vec<FrostSignatureShare>,
    aggregated_signature: Option<FrostSignature>,  // 3/5 toplandığında
}

// Order validation pipeline'a guardian adımı ekle:
pub fn validate_guardian_order(
    order: &GuardianOrder,
    account: &GuardianAccount,
) -> Result<ValidatedOrder> {
    // 1. Guardian set'i al
    let guardians = account.guardians.iter().collect::<Vec<_>>();

    // 2. Threshold kontrolü
    let valid_sigs = order.guardian_signatures.iter()
        .filter(|sig| guardians.contains(&sig.guardian_pubkey))
        .count();

    if valid_sigs < order.required_threshold as usize {
        return Err(ValidationError::InsufficientGuardianApprovals);
    }

    // 3. FROST aggregate verify
    let agg_sig = frost_aggregate(&order.guardian_signatures)?;
    frost_verify(&account.group_pubkey, &order.inner.hash(), &agg_sig)?;

    // 4. Standart validation devam eder
    validate_standard_order(&order.inner)
}
```

**CoW'dan alınan ders:** `EIP-1271` pattern'i (kontrat imzası). Guardian threshold imzası aynı pattern: off-chain signature aggregation, on-chain benzeri verify. Simulation ile test et.

### Cross-Chain Batch Auction

Nova Shield'ın cross-chain varlıkları (BTC/ETH/SOL) için:

```rust
// nova-auction/src/cross_chain.rs

pub struct CrossChainOrder {
    // Kaynak zincir varlığı:
    source_chain: ChainId,           // Bitcoin, Ethereum, Solana
    source_asset: AssetId,           // BTC, ETH, SOL
    source_amount: u128,

    // Hedef (Nova Shield'da ne alacak):
    nova_token: TokenId,             // NSC veya Nova'nın diğer tokenleri
    nova_amount_min: u128,           // minimum

    // MPC vault bilgisi:
    vault_address: CrossChainVault,  // guardian threshold imzalı vault
    mpc_signature: FrostSignature,   // cross-chain tx için
}

// Batch auction entegrasyonu:
// 1. CrossChainOrder → Nova'daki StandardOrder olarak temsil et
//    (vault'taki varlık sanki Nova token'ıymış gibi)
// 2. CoW netting mümkün:
//    BTC→NSC isteyenler + NSC→BTC isteyenler → cross-chain CoW!
// 3. Settlement:
//    Nova kısmı: normal settlement
//    Cross-chain kısmı: MPC vault TX (guardian threshold imzası)
```

**Fee bilgisi (NOVA_SHIELD_CONTEXT.md'den doğrulandı):**
```
Yıllık koruma ücreti: %0.05 (5 bps)
BTC/ETH çarpanı: %100 (tam oran)
SOL çarpanı: %80
Cosmos çarpanı: %60
Fee dağılımı: Staker %60, Guardian %40
```

**Batch auction ek fee:**
```
Cross-chain order için DEX fee:
  Standard DEX fee + cross-chain routing premium
  Routing premium: %0.10-%0.15 ek (MPC signing maliyeti)
```

### NSC Native Settlement

```rust
// nova-auction/src/settlement.rs

pub struct NovaBatchSettlement {
    auction_id: u64,
    slot_number: u64,

    // CoW Protocol'den doğrudan alınan alanlar:
    tokens: Vec<TokenId>,
    clearing_prices: Vec<u128>,      // tokens[i] için clearing price
    trades: Vec<TradeExecution>,
    interactions: Vec<ProtocolInteraction>,  // pre/main/post

    // Nova Shield özgü:
    zk_proofs: Vec<Option<Groth16Proof>>,    // shielded order'lar için
    guardian_sigs: Vec<Option<FrostSignature>>, // guardian order'lar için
    cross_chain_txs: Vec<Option<MpcTx>>,     // cross-chain order'lar için

    // Off-chain indexing (CoW'dan alınan pattern):
    solver_address: Address,
    // calldata sonuna auction_id ekle — CoW'dan alınan:
    // settle_calldata.extend(auction_id.to_be_bytes())
}

// L1 protokol seviyesinde validation:
pub fn validate_batch_settlement(
    settlement: &NovaBatchSettlement,
    state: &ChainState,
) -> Result<(), SettlementError> {
    // 1. Solver stake kontrolü
    check_solver_stake(settlement.solver_address, state)?;

    // 2. Her trade için limit price kontrolü (CoW'dan):
    for trade in &settlement.trades {
        validate_limit_price(trade, &settlement.clearing_prices)?;
    }

    // 3. ZK proof'ları doğrula (shielded order'lar)
    for (trade, proof) in settlement.trades.iter().zip(&settlement.zk_proofs) {
        if trade.is_shielded {
            verify_groth16_proof(proof.as_ref().ok_or(MissingProof)?)?;
        }
    }

    // 4. Fairness invariant (CoW'dan):
    // Her order, tek-order execution'dan kötü fiyat alamaz
    check_fairness_invariant(settlement, state)?;

    // 5. Replay protection (CoW filledAmounts eşdeğeri):
    for trade in &settlement.trades {
        let state = state.order_states.get(&trade.order_uid)
            .ok_or(OrderNotFound)?;
        if state.invalidated { return Err(OrderInvalidated); }
        let new_filled = state.filled_amount + trade.executed_amount;
        if new_filled > trade.order_limit { return Err(Overfill); }
    }

    Ok(())
}
```

---

## J. RUST İMPLEMENTASYON REHBERİ

### Kritik Crate'ler

**CoW Protocol'den doğrudan kullanılabilecek Rust pattern'leri:**

```toml
# Cargo.toml
[dependencies]
# Arithmetic (CoW'un kullandığı):
alloy-primitives = "0.8"         # U256, U512, Address, B256
num-bigint = "0.4"               # BigUint, BigInt
num-rational = "0.4"             # BigRational (price hesabı)
bigdecimal = "0.4"               # BigDecimal (fee display)

# Serialization:
serde = { version = "1", features = ["derive"] }
serde_json = "1"
const-hex = "0.2"                # SIMD-hızlı hex encode/decode (CoW kullanıyor)

# Async:
tokio = { version = "1", features = ["full"] }
futures = "0.3"
itertools = "0.12"

# Crypto (Nova Shield'a özgü):
ark-bn254 = "0.4"               # Groth16 için BN254
ark-groth16 = "0.4"             # ZK proof
frost-core = "1"                 # FROST threshold Schnorr
```

### Sayısal Güvenlik Kuralları (CoW'dan öğrenilen)

```rust
// 1. U512 widening_mul — buy order surplus hesabı (doğrulandı scoring.rs satır 109):
let surplus_in_buy = surplus_sell
    .widening_mul(signed_buy_amount)  // U256 * U256 → U512
    .checked_div(U512::from(signed_sell_amount))?;
let result = U256::uint_try_from(surplus_in_buy)?;

// 2. checked_ceil_div — settlement kontratıyla tutarlılık (doğrulandı):
fn checked_ceil_div(numerator: U256, denominator: U256) -> Option<U256> {
    (!denominator.is_zero()).then(|| numerator.div_ceil(denominator))
}

// 3. Fee faktörü — ASLA f64 kullanma on-chain'de:
// BAD:  let fee = surplus * 0.5;
// GOOD: let fee = surplus * FACTOR_BPS / 10_000;  (basis points)
// CoW f64 kullanıyor ama off-chain simulation için, biz on-chain yapacağız

// 4. NonZeroU256 — bölüme denominatörü için:
let price = NonZeroU256::new(buy_price).ok_or(DivisionByZero)?;
let amount = sell_amount.checked_div(price.get()).ok_or(Overflow)?;

// 5. Price fee formülü (doğrulandı scoring.rs satır 322-338):
// fee = surplus * factor / (1 - factor)
// Integer: fee = surplus_bps * FACTOR_BPS / (10_000 - FACTOR_BPS)
fn surplus_fee_bps(surplus: u128, factor_bps: u64) -> Option<u128> {
    surplus
        .checked_mul(factor_bps as u128)?
        .checked_div((10_000 - factor_bps) as u128)
}
```

### DomainSeparator Implementasyonu

```rust
// nova-auction/src/domain.rs (CoW model/src/lib.rs'ten alındı)

pub struct NovaDomainSeparator([u8; 32]);

impl NovaDomainSeparator {
    pub fn new(chain_id: u64, settlement_program_id: [u8; 32]) -> Self {
        // EIP-712 benzeri ama Nova'ya özgü:
        let domain = [
            keccak256(b"NovaDomain(string name,string version,uint64 chainId,bytes32 programId)"),
            keccak256(b"Nova Shield"),
            keccak256(b"v1"),
            chain_id.to_be_bytes(),
            settlement_program_id,
        ].concat();
        Self(keccak256(&domain))
    }
}

// Replay protection: chain_id + program_id birlikte
// Cross-chain replay imkânsız (farklı chain_id)
// Program upgrade replay imkânsız (farklı program_id)
```

### Token Kalite Kontrolü

CoW Protocol'den doğrudan alınan pattern (`bad-tokens/src/trace_call.rs`):

```rust
// nova-dex/src/token_quality.rs
pub async fn test_token_quality(
    token: Address,
    amount: U256,
    web3: &Web3,
) -> TokenQuality {
    // 8 trace call (CoW'dan doğrudan alınan):
    // [0] balanceOf(settlement) başlangıç
    // [1] transfer(settlement, amount)
    // [2] balanceOf(settlement) sonra — fee-on-transfer tespiti
    // [3] balanceOf(random_recipient) başlangıç
    // [4] transfer(random_recipient, amount) from settlement
    // [5] balanceOf(settlement) sonra (sıfırlanmalı)
    // [6] balanceOf(random_recipient) sonra
    // [7] approve(random_recipient, U256::MAX) — max approve

    // Fee-on-transfer kontrolü (1 wei tolerans):
    // balance_after >= balance_before + amount - 1

    // CoW'dan alınan kritik kural:
    // U256::MAX approve desteklenmelidir
    // Wyper compiler bug: balanceOf 32 byte değil farklı encoding dönebilir
}
```

### Cache Stratejileri

```rust
// nova-dex/src/cache.rs

// 1. Balance cache (CoW cached.rs'ten):
const EVICTION_TIME: u64 = 5;  // 5 slot (CoW'da 5 blok)
// Error'ları cache'leme — geçici node hatası kalıcı olmaz

// 2. Token info cache (CoW CachedTokenInfoFetcher'dan):
// Shared future: aynı token için paralel istekler tek RPC çağrısı
// Error eviction: hatalı entry'yi temizle, retry'a izin ver

// 3. Native price cache:
// arc_swap::ArcSwap<HashMap> — lock-free okuma
// Random eviction — thundering herd önleme
// max_age_without_hit ile stale entry temizleme
```

### Batch RPC (CoW'un Nagle pattern'i)

```rust
// nova-network/src/rpc_client.rs

// CoW'dan alınan batch layer (ethrpc/src/alloy/buffering.rs):
// chunks_timeout(max_batch_size=20, batch_delay=0) pattern
// Nova Shield'da slot süresi 400ms → batch_delay = 50ms öneri

pub struct BatchRpcConfig {
    pub max_batch_size: usize,         // default: 20 (CoW'dan)
    pub max_concurrent_requests: usize, // default: 10 (CoW'dan)
    pub batch_delay: Duration,          // default: 50ms (Nova 400ms slot için)
}
```

---

## K. ÖNEMLİ TASARIM KARARLARI VE GÜVENLİK

### CoW Protocol'ün Kritik Güvenlik Mekanizmaları

**1. Replay Protection (`filledAmounts`):**
```
// GPv2Settlement'da:
filledAmounts[orderUid] = filled_sell_amount  // Sell order
filledAmounts[orderUid] = filled_buy_amount   // Buy order

// filledAmounts >= order.amount → order tamamlandı
// filledAmounts = U256::MAX → order iptal edildi (invalidateOrder)
```

Nova Shield: RocksDB'de `order_uid → OrderState` mapping.

**2. Atomic Settlement:**
Tüm batch ya başarılı ya da tümü başarısız (all-or-nothing). Nova Shield'da L1 transaction atomicity bunu sağlar.

**3. Auction ID Calldata Trailing Bytes (CoW'dan alınan kritik pattern):**
```rust
// encoding.rs satır 226 (doğrulandı):
settle_calldata.extend(auction_id.to_be_bytes());
```
Off-chain indexer hangi settlement'ın hangi auction'a ait olduğunu bilir.
Nova Shield'da: `settlement_tx_body.extend(auction_id.to_be_bytes())`.

**4. Solution Merging:**
İki solver farklı token çiftleri için aynı auction'ı çözebilir, sonuçlar birleştirilir:
```rust
// mod.rs satır 361-448 (doğrulandı):
// scaling_factor ile fiyatlar normalize edilir
// Token fiyatları congruent ise merge mümkün
```
Nova Shield: Multi-solver batch için aynı mantık.

**5. USDT Approve Pattern:**
```rust
// Settlement öncesi approve simülasyonu:
if regular_approve_fails {
    // approve(0) → approve(amount)  // USDT-like token handling
}
```

**6. Internalization — Trusted Token Optimizasyonu:**
```rust
// settlement.rs:
// Güvenilir token'lar settlement vault'tan sağlanabilir
// Gas tasarrufu: interaction'lar atlanır
pub token.trusted: bool,
```
Nova Shield: WhitelistedToken flag ile aynı optimizasyon.

### Kritik Bug'lar ve Önlemleri

CoW Protocol'de tespit edilen ve Nova Shield'ın kaçınması gereken sorunlar:

| Sorun | Kaynak | Nova Shield Önlemi |
|-------|--------|--------------------|
| USDT approve = sıfırlama gerektirir | `settlement.rs` | Approve simulation zorunlu |
| EthFlow UID'de kontrat adresi | `refund_service.rs` satır 829 | UID formatında güvenli encoding |
| UIDs/orders listesi boyut uyumsuzluğu | `refund_service.rs` (bug) | Strict zip ile işle |
| Gas overflow: `GAS_PER_WETH_UNWRAP + u64::MAX` | `sanitized.rs` | Checked arithmetic her yerde |
| Thundering herd: probe'da race condition | `fallback.rs` | `last_probe = now()` önce set et |
| `subscribe_before_send` race | `buffered.rs` | Subscribe BEFORE channel send |
| f64 fee factor off-chain/on-chain uyumsuzluğu | `fee_policy.rs` | On-chain'de integer/bps kullan |

### Master Sentez'den Doğrulanan Kritik Değerler

Tüm değerler kaynak koddan satır satır doğrulandı (`agent-25-MASTER-SENTEZ.md`):

```
OrderUid format: 32+20+4 = 56 byte              ✅ model/src/order.rs satır 754-758
hash_struct buffer: 416 byte                     ✅ model/src/order.rs satır 237
GPv2Order TYPE_HASH: 0xd5a25ba2...               ✅ model/src/order.rs satır 230
CIP-38 widening_mul (U256→U512) buy order        ✅ scoring.rs satır 109
Fee = surplus * factor/(1-factor)                ✅ scoring.rs satır 322-338
Volume fee Sell: factor/(1-factor)               ✅ scoring.rs satır 376-378
Volume fee Buy: factor/(1+factor)                ✅ scoring.rs satır 376-378
Fee policies ters sırada işlenir                 ✅ scoring.rs satır 175
select_ok paralel mempool submission             ✅ driver/domain/mempools.rs satır 72
Reference score = total_score_without_S          ✅ winner_selection/arbitrator.rs satır 607-644
max_winners_per_auction = 20                     ✅ configs/src/autopilot/run_loop.rs satır 11
max_solutions_per_solver = 3                     ✅ configs/src/autopilot/run_loop.rs satır 15
min_solve_time = 10s                             ✅ configs/src/autopilot/run_loop.rs
submission_deadline = 5 blok                     ✅ configs/src/autopilot/run_loop.rs
GAS_PRICE_BUMP_PCT = 13                         ✅ mempools.rs satır 22
EIP-7825 cap = 2^24-1 = 16,777,215 gas/tx       ✅ settlement.rs satır 460
GAS_LIMIT_FACTOR = 2.0                          ✅ settlement.rs
SETTLEMENT_SINGLE_TRADE = 94,891 gas            ✅ gas.rs
EIP-1271 magic = 0x1626ba7e                     ✅ signature-validator/src/lib.rs
Uniform DIRECTIONAL clearing price (2 fiyat)    ✅ winner_selection.rs başlık yorumu
Ban sistemi: HashSet+Chainalysis+Hermod          ✅ order-validation/src/banned/mod.rs
```

---

## L. UYGULAMA YOL HARİTASI

### Nova Shield'a Derhal Uygulanabilecekler (Öncelik 1)

1. **CoW Netting Engine** (`nova-auction/src/cow.rs`):
   - Bug #16'yı düzelt
   - TokenPair index, CoW fırsatı tarama, limit kontrolüyle fill
   - Test: 2 ters order → CoW settlement, AMM kullanılmadı

2. **OrderUid Format Güncelleme** (`nova-auction/src/order.rs`):
   - 56-byte CoW formatı veya Nova'ya özgü 72-byte
   - valid_to_slot (u64), sender_pubkey (32 byte)

3. **Fee Engine** (`nova-auction/src/fee.rs`):
   - Surplus fee: `surplus * factor_bps / (10_000 - factor_bps)`
   - Volume fee: hacim * factor_bps / 10_000
   - Max volume cap
   - Partner fee (referral/affiliate)

4. **CIP-38 Scoring** (`nova-auction/src/scoring.rs`):
   - Sell order: `surplus_buy * native_price_buy`
   - Buy order: U512 widening_mul ile
   - VCG reward: marjinal katkı hesabı

5. **Fairness Invariant** (`nova-auction/src/winner.rs`):
   - Her order'ın batch fiyatı ≥ single-order fiyatı

6. **Solver Competition Loop** (`nova-auction/src/auction.rs`):
   - Paralel solver sorgulaması
   - Deadline yönetimi (10s minimum)
   - Max 20 winner, solver başına max 3 çözüm

### Nova Shield'a Orta Vadede Uygulanacaklar (Öncelik 2)

7. **Token Kalite Kontrolü**: TraceCallDetector benzeri 8-adım simülasyon
8. **Balance Simülasyonu**: Pre-interaction aware simülasyon
9. **Price Estimation Motoru**: Stage-based competition, BestBangForBuck ranking
10. **ZK Order Entegrasyonu**: Shielded order decrypt → batch auction pipeline
11. **Cross-Chain Batch**: BTC/ETH/SOL order'ları için cross-chain CoW

### Nova Shield'a Uzun Vadede Uygulanacaklar (Öncelik 3)

12. **EIP-4626 Vault Fiyatlandırma**: Yield-bearing token desteği
13. **Multi-Hop CoW**: 3+ token ring netting
14. **CLMM Pool İndexleme**: UniV3-benzeri tick bazlı pool desteği
15. **ZK-Verified Quotes**: Simulation sonuçları ZK kanıtla doğrulama
16. **Privacy-Preserving Batch**: Solver'lardan batch büyüklüğü gizleme

---

## ÖZET: NOVA SHIELD'E COW PROTOCOL'DEN ALINACAK 20 KRİTİK ÖĞRENIM

| # | Öğrenim | Kaynak | Nova Önemi |
|---|---------|--------|------------|
| 1 | Uniform DIRECTIONAL clearing price (her token 2 fiyat) | winner_selection.rs | YÜKSEK |
| 2 | VCG reward: marjinal katkı = total_S - total_without_S | arbitrator.rs | YÜKSEK |
| 3 | Fee = surplus * factor/(1-factor) post-fee hesap | scoring.rs | YÜKSEK |
| 4 | U512 widening_mul — buy order surplus overflow önleme | scoring.rs satır 109 | YÜKSEK |
| 5 | Fee politikaları ters sırada işle, her sonra price güncelle | fee.rs | YÜKSEK |
| 6 | CoW netting: ters emirler AMM'siz netting → sıfır MEV | batch_auction | YÜKSEK |
| 7 | OrderUid = 32 hash + 20 owner + 4 valid_to | order.rs | YÜKSEK |
| 8 | Fairness invariant: batch ≥ single-order | winner_selection.rs | YÜKSEK |
| 9 | Replay protection: filledAmounts[uid] on-chain | GPv2Settlement | YÜKSEK |
| 10 | Auction ID trailing bytes (off-chain indexing) | encoding.rs satır 226 | ORTA |
| 11 | select_ok paralel mempool yarışması | mempools.rs | ORTA |
| 12 | Gas limit: min(block/2, EIP-7825, estimate*2) | settlement.rs | ORTA |
| 13 | Token kalite testi: 8-adım trace simülasyon | trace_call.rs | ORTA |
| 14 | Balance cache: 5 slot eviction, error'ı cache'leme | cached.rs | ORTA |
| 15 | Fee-on-transfer: 1 wei tolerans | trace_call.rs | ORTA |
| 16 | Stage-based price competition: paralel kaynak | competition/mod.rs | ORTA |
| 17 | BestBangForBuck: out_amount - gas_cost (net value) | competition/mod.rs | ORTA |
| 18 | EIP-4626: convertToAssets nested rate hesabı | native/eip4626.rs | DÜŞÜK |
| 19 | USDT approve sıfırlama simülasyonu | settlement.rs | DÜŞÜK |
| 20 | app_data 8KB IPFS → sadece hash imzala | app_data.rs | DÜŞÜK |
