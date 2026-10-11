# Penumbra DEX Analizi — Agent DEX-A
## Kapsam: DEX Veri Katmanı, Swap, SwapClaim, LP — Tüm Modüller (35 Dosya)
## 3x Hassasiyet Protokolü Uygulandı

**Analiz Tarihi:** 2026-07-22
**Agent:** DEX-A (LP katmanı + Swap + SwapClaim + üst düzey DEX tipleri)
**Toplam Okunan Satır:** ~3500+ satır

---

## ÖZET — Penumbra DEX Mimarisi

Penumbra DEX, **zk-SNARK tabanlı batch swap** ve **concentrated liquidity** mekanizmalarını birleştiren benzersiz bir sistemdir. İki temel katmana ayrılır:

1. **LP (Liquidity Provider) katmanı** — Pozisyonlar, concentrated liquidity AMM, NFT tabanlı sahiplik
2. **Swap + SwapClaim katmanı** — ZK kanıtlı gizli batch swap, pro-rata çıktı dağıtımı

Temel fark: Swap işlemleri **Groth16 ZK proofs** ile korunur ve swap girdisi şifrelenir. Clearing price (fiyat tespiti) batch olarak yapılır ve her kullanıcı kendi payını ZK kanıtıyla talep eder.

---

## BÖLÜM 1 — LP VERİ KATMANI

---

## [trading_function.rs]

**Dosya:** `/crates/core/component/dex/src/lp/trading_function.rs`
**Satır Sayısı:** 716

### Genel Amaç

Penumbra DEX'in çekirdek AMM matematiksel modelini tanımlar. İki struct içerir:
- `TradingFunction` — pair bilgisiyle birlikte trading function
- `BareTradingFunction` — sadece matematiksel bileşen (fee, p, q)

### Struct/Enum/Type Listesi

#### `TradingFunction`
```rust
pub struct TradingFunction {
    pub component: BareTradingFunction,
    pub pair: TradingPair,
}
```

**Metodlar:**
- `new(pair, fee, p, q) -> Self` — Yeni trading function oluşturur
- `matches_input(input_id) -> bool` — Giriş asset'i pair'in bir tarafıyla eşleşiyor mu
- `fill(input: Value, reserves: &Reserves) -> Result<(Value, Reserves, Value)>` — İki yönlü fill
- `fill_output(reserves, output) -> Result<Option<(Reserves, Value)>>` — Belirli çıktı için gereken giriş
- `orient_end(end) -> Option<BareTradingFunction>` — Çıktı yönüne göre orient
- `orient_start(start) -> Option<BareTradingFunction>` — Giriş yönüne göre orient

#### `BareTradingFunction`
```rust
pub struct BareTradingFunction {
    pub fee: u32,   // basis points, 0 <= fee < 5000
    pub p: Amount,  // asset_1 değerlemesi (max 80-bit)
    pub q: Amount,  // asset_2 değerlemesi (max 80-bit)
}
```

**Metodlar:**
- `new(fee, p, q) -> Self`
- `flip() -> Self` — p ve q'yu yer değiştirir (yön ters çevirme)
- `fill_output(reserves, lambda_2) -> Result<Option<(Reserves, Amount)>>` — Çıktı miktarı için gerekli giriş
- `fill(delta_1, reserves) -> Result<(Amount, Reserves, Amount)>` — Forward fill
- `effective_price_key_bytes() -> [u8; 32]` — Sıralanabilir fiyat anahtarı
- `effective_price_inv() -> U128x128` — Dönüşüm oranı (1→2)
- `effective_price() -> U128x128` — Dönüşüm oranı (2→1)
- `convert_to_lambda_2(delta_1) -> Result<U128x128>`
- `convert_to_delta_1(lambda_2) -> Result<U128x128>`
- `gamma() -> U128x128` — Fee discount faktörü
- `compose()` — DEPRECATED, impl yok

### Matematiksel/AMM Formülleri (KRITIK — SATIR NUMARASILI)

#### Trading Function Tanımı (Satır 196-198)
```
phi(R) = p*R_1 + q*R_2
```
Bu bir **constant-sum CFMM** (sabit toplam). Fiyat oranı `p/q` tarafından belirlenir.

#### Gamma (Fee Discount Faktörü) (Satır 444-446)
```rust
pub fn gamma(&self) -> U128x128 {
    (U128x128::from(10_000 - self.fee) / U128x128::from(10_000u64)).expect("10_000 != 0")
}
```
**Formula:** `gamma = (10_000 - fee) / 10_000`
- fee=0 bps → gamma=1.0 (fee yok)
- fee=30 bps → gamma=0.997
- fee=5000 bps → gamma=0.5 (maksimum)

#### Effective Price (Satır 413-419)
```rust
pub fn effective_price(&self) -> U128x128 {
    let price_ratio = (q / p).expect("p != 0 and p,q <= 2^60");
    price_ratio.checked_div(&self.gamma()).expect("gamma != 0")
}
```
**Formula (2→1 yönü):** `effective_price = (q/p) / gamma`

Bu şu anlama gelir: `lambda_2 * effective_price = delta_1`
Yani 1 birim asset_2 için kaç birim asset_1 gerektiğini söyler (fee dahil).

#### Effective Price Inv (Satır 402-408)
```rust
pub fn effective_price_inv(&self) -> U128x128 {
    let price_ratio = (p / q).expect("q != 0 and p,q <= 2^60");
    (price_ratio * self.gamma()).expect("2^-1 <= gamma <= 1")
}
```
**Formula (1→2 yönü):** `effective_price_inv = (p/q) * gamma`

Bu şu anlama gelir: `delta_1 * effective_price_inv = lambda_2`
Yani 1 birim asset_1 kaç birim asset_2 verir (fee düşüldükten sonra).

#### fill() Algoritması — ÇIFT DALLANMALI (Satır 290-388)

**Durum 1: Yeterli rezerv var (tentative_lambda_2 <= r2)**
```
delta_1_fp = U128x128::from(delta_1)
tentative_lambda_2 = convert_to_lambda_2(delta_1_fp)
                   = effective_price_inv * delta_1
                   = (p/q * gamma) * delta_1

lambda_2 = floor(tentative_lambda_2)      // aşağı yuvarla = alıcı lehine
new_r1 = reserves.r1 + delta_1
new_r2 = reserves.r2 - lambda_2
unfilled = 0
```

**Durum 2: Rezerv yetersiz (tentative_lambda_2 > r2)**
```
r2 = reserves.r2 (tüm rezervi kullan)
fillable_delta_1 = convert_to_delta_1(r2) = effective_price * r2 = (q/p)/gamma * r2
fillable_delta_1_exact = ceil(fillable_delta_1)  // yukarı yuvarla = satıcı lehine
unfilled = delta_1 - fillable_delta_1_exact
new_r1 = reserves.r1 + fillable_delta_1_exact
new_r2 = 0
lambda_2 = reserves.r2  // tam r2
```

**Değer Koruma Garantisi** (Satır 577-606 testlerde):
```
old_r1 + delta_1 == new_r1 + lambda_1 (unfilled)
old_r2 + 0 == new_r2 + lambda_2
```

#### fill_output() — Ters Yön (Satır 241-281)

Belirli miktarda çıktı üretmek için gereken girdiyi hesaplar:
```
if lambda_2 > r2: return None (yetersiz rezerv)
fillable_delta_1 = convert_to_delta_1(lambda_2) = effective_price * lambda_2
fillable_delta_1_exact = ceil(fillable_delta_1)  // yuvarlama: alıcı değil, havuz lehine
new_r1 = r1 + fillable_delta_1_exact
new_r2 = r2 - lambda_2
return Some((new_reserves, fillable_delta_1_exact))
```

#### TradingFunction::fill() — Çift Yönlü Wrapper (Satır 42-85)

```
if input.asset_id == pair.asset_1():
    component.fill(input.amount, reserves)  // doğrudan

if input.asset_id == pair.asset_2():
    flipped_reserves = reserves.flip()       // r1↔r2
    component.flip().fill(input.amount, &flipped_reserves)  // p↔q de flip
    new_reserves.flip()                      // sonucu geri flip
```

### Trait Implementasyonları

- `DomainType for TradingFunction` → `pb::TradingFunction`
- `DomainType for BareTradingFunction` → `pb::BareTradingFunction`
- `TryFrom<pb::TradingFunction>` / `From<TradingFunction>`
- `TryFrom<pb::BareTradingFunction>` / `From<BareTradingFunction>`

### Testler

1. **test_trading_function_to_bytes** (Satır 499-540): Fiyat kodlamasının lexicographic sıralamayı koruduğunu doğrular. `price1 < price2` ise `bytes1 < bytes2`.
2. **fill_conserves_value** (Satır 551-607): Her iki branch'te değer koruma testi.
   - Partial fill: `delta_1=10M, r2=100M, p=1, q=3` → `lambda_2=3_333_333`
   - Full fill: `delta_1=600M` → `lambda_1=300M unfilled, lambda_2=r2`
3. **fill_bad_rounding** (Satır 610-634): `p=12, q=10, r2=120, delta_1=100` → `lambda_2=119` (lossy rounding expected)
4. **test_conversion_helpers** (Satır 636-653): `effective_price == convert_to_delta_1(1)` doğrulaması
5. **test_fill_trading_function** (Satır 656-714): Çift yönlü TradingFunction fill testi

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **Constant-sum CFMM** — Uniswap'ın constant-product yerine. Tick bazlı yoğunlaştırılmış likiditeye izin verir. Her pozisyon kendi `p/q` oranıyla bir limit order gibi davranır.
2. **Tek yönlü yuvarlama** — `lambda_2` aşağı yuvarlanır (Durum 1), `delta_1_exact` yukarı yuvarlanır (Durum 2). Rounding hatası her zaman havuz/satıcı aleyhine değil, protokol güvenliği lehine.
3. **Etkili fiyat key encoding** — `effective_price().to_bytes()` ile elde edilen 32-byte anahtar, lexicographic sıralamada numerik sıralamayı korur. Bu sayede key-value store'dan en iyi fiyatlı pozisyonu O(1) ile bulunabilir.
4. **Flip mekanizması** — `pair.asset_2()` girdi olduğunda `flip()` çağrısı: `p↔q` ve `r1↔r2`. Kod tekrarını önler.
5. **U128x128 fixed-point** — 128-bit integer + 128-bit fraction. Kaygısız bölme; native Rust artimetik taşmaları önler.

### DEX Uygulama Notu — Nova Shield için:

**KRITIK — Hemen Uygulanabilir:**

1. **Constant-sum AMM modeli**: Nova Shield DEX'i için Uniswap v3 concentrated liquidity yerine Penumbra'nın `phi(R) = p*R1 + q*R2` modeli daha şeffaf ve denetlenebilir. Her pozisyon kendi price tick'ini `p/q` oranıyla belirler. Bu model yerleşik limit order desteği sağlar.

2. **Etkili fiyat anahtar kodlaması**: `effective_price().to_bytes()` → lexicographic sıralanabilir fiyat indeksi. Nova Shield'in RocksDB depolamasında pozisyonları fiyat sırasında tutmak için bu yaklaşım kullanılmalı. Key: `[price_bytes || position_id]`.

3. **Çift dallanmalı fill algoritması**: Rounding hatası her zaman belirli tarafa atılır. Nova Shield için kural: Partial fill → `lambda` aşağı yuvarlama; Full fill → `delta_input` yukarı yuvarlama. Bu, değer koruma (conservation) garantisi verir.

4. **Gamma formülü**: `gamma = (10_000 - fee) / 10_000` doğrudan alınabilir. Ancak Nova Shield için `MAX_FEE_BPS = 5000` (50%) yerine MEV koruması senaryolarında farklı bir tavan düşünülebilir.

5. **compose() metodunun eksikliği**: Penumbra `compose()` metodunu implement etmemiş (deprecated). Nova Shield için birden fazla pozisyonu birleştiren composable liquidity, routing engine'i güçlendirecek — bu boşluk doldurulabilir.

---

## [position.rs]

**Dosya:** `/crates/core/component/dex/src/lp/position.rs`
**Satır Sayısı:** 703

### Genel Amaç

LP pozisyonunun merkezi veri yapısı. Değişmez kısım (phi/nonce) ve değişken kısım (state/reserves) birlikte kapsüllenir.

### Struct/Enum/Type Listesi

#### `Position` (Ana struct, Satır 24-38)
```rust
pub struct Position {
    pub state: State,
    pub reserves: Reserves,
    pub phi: TradingFunction,
    pub nonce: [u8; 32],      // İki pozisyonu ayıran rastgele değer
    pub close_on_fill: bool,  // Limit order davranışı
}
```

#### `State` Enum (Satır 368-381)
```rust
pub enum State {
    Opened,
    Closed,
    Withdrawn { sequence: u64 },
}
```

**State Machine Geçişleri:**
```
Opened → Closed          (PositionClose işlemi)
Closed → Withdrawn{0}   (PositionWithdraw, sequence=0)
Withdrawn{n} → Withdrawn{n+1}  (Tekrar withdraw - retroactive rewards için)
```

#### `Id` (Satır 330)
```rust
pub struct Id(pub [u8; 32]);
```
Bech32m encoding: `plpid1...` prefix.

#### `Flows` (Satır 229-238)
```rust
pub struct Flows {
    pair: DirectedTradingPair,
    delta_1: Amount,
    delta_2: Amount,
    lambda_1: Amount,
    lambda_2: Amount,
    fee_1: Amount,
    fee_2: Amount,
}
```
Bir pozisyonun önceki durumuna göre akışlarını hesaplar.

### Sabitler (Satır 14-18)
```rust
pub const MAX_RESERVE_AMOUNT: u128 = (1 << 80) - 1;  // 80-bit maks rezerv
pub const MAX_FEE_BPS: u32 = 5000;                    // Maks %50 fee
```

### Fonksiyonlar

#### `Position::new()` (Satır 58-100)
Yeni pozisyon oluşturur:
1. RNG ile 32-byte nonce üretir
2. `DirectedTradingPair → TradingPair` canonical dönüşümü
3. Eğer `pair.start != canonical_tp.asset_1()` ise `p/q` ve `r1/r2` swap edilir
4. `State::Opened` ile başlar, `close_on_fill: false`

#### `Position::id()` (Satır 145-161) — KRITIK
```rust
let mut state = blake2b_simd::Params::default()
    .personal(b"penumbra_lp_id")
    .to_state();

state.update(&self.nonce);
state.update(&self.phi.pair.asset_1().to_bytes());
state.update(&self.phi.pair.asset_2().to_bytes());
state.update(&self.phi.component.fee.to_le_bytes());
state.update(&self.phi.component.p.to_le_bytes());
state.update(&self.phi.component.q.to_le_bytes());
```
**Formula:** `id = blake2b_personal("penumbra_lp_id", nonce || asset_1 || asset_2 || fee_le || p_le || q_le)`

#### `Position::check_stateless()` (Satır 163-189)
Hata koşulları:
- `r1 > MAX_RESERVE_AMOUNT` veya `r2 > MAX_RESERVE_AMOUNT`
- `r1 == 0 && r2 == 0` (her ikisi de boş)
- `p == 0` veya `q == 0` (sıfır katsayı)
- `p > MAX_RESERVE_AMOUNT` veya `q > MAX_RESERVE_AMOUNT`
- `asset_1 == asset_2` (cyclical pair)
- `fee > MAX_FEE_BPS`

#### `Flows::from_phi_and_reserves()` (Satır 269-302)
```
if current.r1 > prev.r1:
    delta_1 = current.r1 - prev.r1   // asset_1 girdi
    lambda_2 = prev.r2 - current.r2  // asset_2 çıktı
else:
    delta_2 = current.r2 - prev.r2   // asset_2 girdi
    lambda_1 = prev.r1 - current.r1  // asset_1 çıktı

fee_1 = (delta_1 * fee_bps) / 10_000
fee_2 = (delta_2 * fee_bps) / 10_000
```

### Trait Implementasyonları

- `DomainType for Position` → `pb::Position`
- `DomainType for Id` → `pb::PositionId`
- `DomainType for State` → `pb::PositionState`
- `FromStr for State` — "opened", "closed", "withdrawn_N" formatı
- `Display for State` — aynı format
- `TryFrom<pb::PositionId> for Id` — `inner` veya `alt_bech32m` mutually exclusive

### Testler

1. **position_state_fromstr** (Satır 547-563): FromStr parsing + hata durumları
2. **test_position** (Satır 581-701): Pair yönüne göre p/q/r1/r2 swap mantığı doğrulaması

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **Nonce tabanlı benzersizlik**: Aynı trading function ile iki pozisyon oluşturmak nonce sayesinde mümkün. Chain, duplicate position ID'leri reddeder.
2. **close_on_fill**: True olduğunda pozisyon doldurulduktan sonra otomatik kapanır → limit order davranışı.
3. **State machine + NFT**: Her state geçişi bir NFT yakıp yeni bir NFT basar. Pozisyon state'i on-chain izlenir ama sahiplik ZK shielded pool'da tutulabilir.
4. **80-bit rezerv sınırı**: `(1 << 80) - 1 ≈ 1.2 × 10^24`. U128x128 hesaplamalarında taşmayı önlemek için.
5. **Retroactive rewards**: `Withdrawn{sequence}` state'i tekrar withdraw'a izin verir — gelecekte retroaktif ödül dağıtımı için yer bırakılmış.

### DEX Uygulama Notu — Nova Shield için:

1. **Pozisyon ID tasarımı**: Nova Shield için `blake2b_personal("nova_shield_lp_id", nonce || asset_1 || asset_2 || fee || p || q)` aynı mantıkla kullanılabilir. Nonce, duplicate güvenliğini sağlar.

2. **State machine NFT modeli**: Penumbra'nın `Opened → Closed → Withdrawn{n}` state geçişini NFT burning/minting ile modellemesi, ZK uyumlu sahiplik için ideal. Nova Shield'in kendi shielded pool'u varsa aynı pattern uygulanmalı.

3. **close_on_fill**: Limit order desteği için bu boolean yeterli. Nova Shield'in DEX'inde limit orderlar için ayrı bir sistem gereksiz.

4. **MAX_RESERVE_AMOUNT = 80-bit**: Nova Shield için da bu değer korunmalı. U128x128 ile 80-bit değerlerinin çarpımı 160-bit → U256 gerekmez.

5. **Flows struct**: Pozisyon geçmişini ve fee akışlarını takip etmek için bu struct Nova Shield'in analitik katmanında kullanılabilir.

---

## [reserves.rs]

**Dosya:** `/crates/core/component/dex/src/lp/reserves.rs`
**Satır Sayısı:** 104

### Struct/Enum/Type Listesi

#### `Reserves`
```rust
pub struct Reserves {
    pub r1: Amount,
    pub r2: Amount,
}
```

### Fonksiyonlar

- `check_bounds()` — `r1 > MAX_RESERVE_AMOUNT`, `r2 > MAX_RESERVE_AMOUNT`, veya ikisi de 0 hatası
- `balance(pair: &TradingPair) -> Balance` — r1 ve r2'yi typed Balance'a çevirir
- `flip() -> Reserves` — `{r1: self.r2, r2: self.r1}` yön ters çevirme
- `zero() -> Self` — sıfır rezervler
- `Default` → `zero()`

### DEX Uygulama Notu — Nova Shield için:

`Reserves` minimal ve yeniden kullanılabilir. Nova Shield da aynı yapıyı kullanabilir. `flip()` metodu pair yönünü değiştirirken çok kritik — unutulursa matematik hatalı olur.

---

## [nft.rs]

**Dosya:** `/crates/core/component/dex/src/lp/nft.rs`
**Satır Sayısı:** 207

### Genel Amaç

LP pozisyonunu temsil eden NFT. Her pozisyon durumu için benzersiz bir token: `lpnft_opened_<id>`, `lpnft_closed_<id>`, `lpnft_withdrawn_N_<id>`.

### Struct/Enum/Type Listesi

#### `LpNft`
```rust
pub struct LpNft {
    position_id: Id,
    state: State,
    base_denom: asset::Metadata,
}
```

### Fonksiyonlar

- `new(position_id, state) -> Self` — Denom formatı: `lpnft_{state}_{position_id}`
- `denom() -> asset::Metadata`
- `asset_id() -> asset::Id` — Denom'dan türetilen asset ID
- `position_id() -> Id`
- `state() -> State`

### Trait Implementasyonları

- `TryFrom<asset::Metadata> for LpNft` — Regex ile parse: `^lpnft_(?P<state>[a-z_0-9]+)_(?P<id>plpid1[a-zA-HJ-NP-Z0-9]+)$`
- `FromStr for LpNft`
- `PartialEq` — `position_id == other.position_id && state == other.state`

### Testler

1. **lpnft_denom_parsing_roundtrip** (Satır 158-205): opened/closed/withdrawn_0/withdrawn_1 round-trip

### Tasarım Kararları

State geçişi = NFT yakıp mint. Bu yaklaşım LP mekanizmasını ZK spending'e bağlar; herhangi bir auth mekanizmasıyla uyumlu (shielded pool veya on-chain hesap).

### DEX Uygulama Notu — Nova Shield için:

Nova Shield LP sistemi için NFT-based state tracking zorunlu değil, ancak ZK uyumluluk istiyorsa bu pattern kullanılmalı. Denom formatı `novalp_{state}_{id}` olarak uyarlanabilir.

---

## [action.rs] (LP)

**Dosya:** `/crates/core/component/dex/src/lp/action.rs`
**Satır Sayısı:** 205

### Struct/Enum/Type Listesi

#### `PositionOpen`
```rust
pub struct PositionOpen {
    pub position: Position,
    pub encrypted_metadata: Option<Vec<u8>>,  // 50 byte: 24-byte nonce + 26-byte ciphertext
}
```

**balance():** `+opened_position_nft - reserves`
→ İşlem rezervleri tüketir ve opened NFT üretir.

#### `PositionClose`
```rust
pub struct PositionClose {
    pub position_id: position::Id,
}
```

**balance():** `+closed_position_nft - opened_position_nft`
→ Sadece NFT'leri değiştirir; rezervler aynı kalır (erken binding).

#### `PositionWithdraw`
```rust
pub struct PositionWithdraw {
    pub position_id: position::Id,
    pub reserves_commitment: balance::Commitment,  // şeffaf commitment (blinding=0)
    pub sequence: u64,
}
```

**Açıklama:** `reserves_commitment` chain tarafından doğrulanır. Sequence withdraw sayısını takip eder (retroaktif ödüller için).

### Tasarım Kararları

**PositionClose neden rezervleri release etmez?** ZK modelde transaction'lar "early-binding"dir — prover son rezervleri önceden bilemez. Kapanma ve çekilme iki ayrı işlemdir.

### DEX Uygulama Notu — Nova Shield için:

Nova Shield'in LP işlem seti de bu üç primitive üzerine kurulabilir: Open/Close/Withdraw. Close → final reserves belirsiz olduğundan ayrı işlem zorunlu. Bu, ZK model için kritik bir kısıtlamadır.

---

## [order.rs]

**Dosya:** `/crates/core/component/dex/src/lp/order.rs`
**Satır Sayısı:** 431

### Genel Amaç

`BuyOrder` ve `SellOrder` yardımcı tipleri. Human-readable order string'lerinden `Position` oluşturma.

### Struct/Enum/Type Listesi

#### `BuyOrder`
```rust
pub struct BuyOrder {
    pub desired: Value,
    pub offered: Value,
    pub fee: u32,
}
```

#### `SellOrder`
```rust
pub struct SellOrder {
    pub offered: Value,
    pub desired: Value,
    pub fee: u32,
}
```

### Fonksiyonlar

#### `parse_parts(input: &str) -> Result<(&str, &str, u32)>` (Satır 33-50)
Format: `<amount><denom>@<price><denom>/Nbps`
- Slash + "bps" suffix varsa fee parse edilir; yoksa fee=0
- `@` ile iki parçaya ayrılır: trade_part ve fee_part

#### `into_position_inner(offered, desired, fee, rng) -> Position` (Satır 236-263) — KRITIK

```rust
let p = desired.amount;
let q = offered.amount;

Position::new(rng, DirectedTradingPair {
    start: offered.asset_id,
    end: desired.asset_id,
}, fee, p, q,
Reserves { r1: offered.amount, r2: 0u64.into() })
```

**Math:** `p = desired.amount, q = offered.amount`
Neden?
```
k = p*R1 + q*R2
k(r1=offered, r2=0) = p*offered = desired*offered
k(r1=0, r2=desired) = q*desired = offered*desired
```
Eşit! Yani `p=desired, q=offered` seçimi constant-sum'u sağlar.

#### `BuyOrder::parse_str()` (Satır 64-96)
```
"100mpenumbra@1.2gm" → 100 mpenumbra almak istiyorum, 1.2 gm/mpenumbra fiyatından
offered_amount = ceil(price * desired / desired_unit)
```

#### `Position::interpret_as_buy/sell/mixed()` (Satır 281-353)
Sadece bir tarafın sıfır olduğu rezervleri buy/sell order olarak yorumlar.

### Testler

1. **parse_buy_order_basic**: "123.444gm@2gn/10bps" parse + round-trip
2. **parse_sell_order_basic**: aynı format sell için

### DEX Uygulama Notu — Nova Shield için:

`into_position_inner` formülü altın değerinde: `p=desired, q=offered`. Nova Shield'in CLI araçları için aynı buy/sell order soyutlaması kullanılabilir. Rounding: `ceil()` kullanılır — kullanıcı her zaman biraz fazla öder.

---

## [metadata.rs]

**Dosya:** `/crates/core/component/dex/src/lp/metadata.rs`
**Satır Sayısı:** 290

### Genel Amaç

Bir pozisyon hakkında şifreli metadata (UIP-9 protokolü). Strategy tag + identifier.

### Struct/Enum/Type Listesi

#### `PositionMetadata`
```rust
pub struct PositionMetadata {
    pub strategy: NonZeroU32,
    pub identifier: NonZeroU32,
}
```

### Kriptografik Yapılar

- **Şifreleme**: `PositionMetadataKey` (OutgoingViewingKey'den türetilir)
- **Format**: `plaintext: [u8; 10]` → `ciphertext: [u8; 26]` (10 byte + 16 byte auth tag)
- **Toplam**: nonce (24 byte) + ciphertext (26 byte) = 50 byte `encrypted_metadata`
- **Anahtar türetme**: `PositionMetadataKey::derive(fvk.outgoing())`

### Fonksiyonlar

- `encrypt(pmk, nonce) -> Vec<u8>` — AED şifreleme
- `decrypt(pmk, ciphertext) -> Result<Option<Self>>` — AED şifre çözme

### Testler

8 test: wire size doğrulama, şifreleme/çözme round-trip, NonZeroU32 sınır testleri.

**Kritik Bulgu — Sabit Wire Size:**
`strategy: NonZeroU32::new(u32::MAX)` ve `strategy: NonZeroU32::new(1)` ikisi de aynı boyutta protobuf encode edilir: `POSITION_METADATA_SIZE_BYTES = 10`.

### DEX Uygulama Notu — Nova Shield için:

Nova Shield'in LP pozisyonlarına metadata eklemek istiyorsa aynı şifreli metadata yapısı kullanılabilir. Özellikle strateji bazlı gruplama (hangi strateji kaç pozisyon açtı) için `identifier` alanı kullanışlı.

---

## [plan.rs] (LP)

**Dosya:** `/crates/core/component/dex/src/lp/plan.rs`
**Satır Sayısı:** 201

### Struct/Enum/Type Listesi

#### `PositionOpenPlan`
```rust
pub struct PositionOpenPlan {
    pub position: Position,
    pub metadata: Option<PositionMetadata>,
}
```

`position_open(fvk, nonce)` → `PositionOpen` (şifreli metadata ile)

#### `PositionWithdrawPlan`
```rust
pub struct PositionWithdrawPlan {
    pub reserves: Reserves,
    pub position_id: position::Id,
    pub pair: TradingPair,
    pub sequence: u64,
    pub rewards: Vec<Value>,
}
```

**reserves_commitment():** `reserves.balance(pair) + rewards commit(Fr::zero())`
Transparent commitment (blinding=0) → chain doğrulayabilir.

**balance() mantığı (Satır 120-157):**
```
// sequence=0: Closed NFT tüket
balance -= closed_position_nft

// sequence=N: Withdrawn{N-1} NFT tüket
balance -= withdrawn_{N-1}_nft

// Her zaman: Withdrawn{N} NFT bas
balance += withdrawn_{N}_nft

// Rezervleri ekle
balance += reserves + rewards
```

### DEX Uygulama Notu — Nova Shield için:

`reserves_commitment()` transparent commitment (blinding=0 ile) tekniği Nova Shield için de kritik: chain rezerv doğrulaması için blinding faktörü 0 seçmek şeffaflık sağlar ama ZK gizliliğini kaybetmez.

---

## [view.rs] (LP)

**Dosya:** `/crates/core/component/dex/src/lp/view.rs`
**Satır Sayısı:** 87

### Struct/Enum/Type Listesi

#### `PositionOpenView`
```rust
pub enum PositionOpenView {
    Visible { action: PositionOpen, metadata: PositionMetadata },
    Opaque  { action: PositionOpen },
}
```

FVK sahibi metadata'yı görebilir (Visible); diğerleri sadece açık action'ı görür (Opaque).

---

## BÖLÜM 2 — SWAP (BATCH ZK SWAP)

---

## [swap.rs] (modül)

**Dosya:** `/crates/core/component/dex/src/swap.rs`
**Satır Sayısı:** 27

### Sabitler
```rust
pub const SWAP_CIPHERTEXT_BYTES: usize = 272;  // ciphertext uzunluğu
pub const SWAP_LEN_BYTES: usize = 256;          // plaintext uzunluğu
```

### Domain Separator (Satır 25-26)
```rust
pub static DOMAIN_SEPARATOR: Lazy<Fq> =
    Lazy::new(|| Fq::from_le_bytes_mod_order(blake2b_simd::blake2b(b"penumbra.swap").as_bytes()));
```

---

## [plaintext.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/plaintext.rs`
**Satır Sayısı:** 470

### Genel Amaç

Swap işleminin gizli içeriği. ZK circuit'e girdi olur. Şifrelenir ve zincirde ciphertext olarak saklanır.

### Struct/Enum/Type Listesi

#### `SwapPlaintext`
```rust
pub struct SwapPlaintext {
    pub trading_pair: TradingPair,
    pub delta_1_i: Amount,      // asset_1 girdi miktarı
    pub delta_2_i: Amount,      // asset_2 girdi miktarı (genelde 0 — tek yön swap)
    pub claim_fee: Fee,         // SwapClaim için ön ödenmiş fee
    pub claim_address: Address, // Çıktıların alınacağı adres
    pub rseed: Rseed,           // Çıktı notları için rastgele seed
}
```

#### Blinding Domain Separators (Satır 43-51)
```rust
pub static OUTPUT_1_BLINDING_DOMAIN_SEPARATOR: Lazy<Fq> = Lazy::new(||
    Fq::from_le_bytes_mod_order(blake2b_simd::blake2b(b"penumbra.swapclaim.output1.blinding").as_bytes())
);
pub static OUTPUT_2_BLINDING_DOMAIN_SEPARATOR: Lazy<Fq> = Lazy::new(||
    Fq::from_le_bytes_mod_order(blake2b_simd::blake2b(b"penumbra.swapclaim.output2.blinding").as_bytes())
);
```

#### `SwapPlaintextVar` (R1CS değişken versiyonu)
```rust
pub struct SwapPlaintextVar {
    pub claim_fee: ValueVar,
    pub delta_1_i: AmountVar,
    pub trading_pair: TradingPairVar,
    pub delta_2_i: AmountVar,
    pub claim_address: AddressVar,
    pub rseed: FqVar,
}
```

### Fonksiyonlar

#### `output_rseeds()` (Satır 55-63) — ZK Kriptografik
```rust
let fq_rseed = Fq::from_le_bytes_mod_order(&self.rseed.to_bytes()[..]);
let rseed_1_hash = hash_1(&OUTPUT_1_BLINDING_DOMAIN_SEPARATOR, fq_rseed);
let rseed_2_hash = hash_1(&OUTPUT_2_BLINDING_DOMAIN_SEPARATOR, fq_rseed);
```
**Formula:** Poseidon hash ile iki çıktı notu için deterministik seed türetme.

#### `output_notes()` (Satır 65-92)
```
(lambda_1_i, lambda_2_i) = batch_data.pro_rata_outputs((delta_1_i, delta_2_i))

output_1_note = Note(claim_address, {amount: lambda_1_i, asset_id: asset_1}, output_1_rseed)
output_2_note = Note(claim_address, {amount: lambda_2_i, asset_id: asset_2}, output_2_rseed)
```

#### `swap_commitment()` (Satır 97-122) — ZK KRITIK

**Formula:** Poseidon hash ile swap commitment:
```
inner = hash_7(DOMAIN_SEPARATOR, (
    rseed,
    claim_fee.amount,
    claim_fee.asset_id,
    compressed_g_d,          // claim_address.diversified_generator
    transmission_key_s,      // claim_address.transmission_key_s
    clue_key,                // claim_address.clue_key
    hash_4(DOMAIN_SEPARATOR, (asset_1, asset_2, delta_1_i, delta_2_i))
))
commitment = StateCommitment(inner)
```

Bu commitment, Tiered Commitment Tree (TCT) içinde saklanır.

#### `encrypt(ovk) -> SwapPayload` (Satır 132-146)
```
commitment = swap_commitment()
key = PayloadKey::derive_swap(ovk, commitment)
swap_plaintext_bytes: [u8; 256] = self.into()
ciphertext: [u8; 272] = key.encrypt_swap(bytes)
```

#### Serialization Layout — [u8; 256] (Satır 338-350)
```
bytes[0..64]   = trading_pair.to_bytes()        // 32+32 = 64 byte
bytes[64..80]  = delta_1_i.to_le_bytes()        // 16 byte
bytes[80..96]  = delta_2_i.to_le_bytes()        // 16 byte
bytes[96..112] = claim_fee.amount.to_le_bytes() // 16 byte
bytes[112..144] = claim_fee.asset_id.to_bytes() // 32 byte
bytes[144..224] = claim_address.inner           // 80 byte
bytes[224..256] = rseed.to_bytes()              // 32 byte
```

#### `SwapPlaintextVar::commit()` — R1CS Kısıtı (Satır 207-243)
ZK circuit içinde, plaintext'in commitment'a eşitliğini enforce eder:
```
inner_hash4 = poseidon_r1cs::hash_4(ds, (asset_1, asset_2, delta_1, delta_2))
inner = poseidon_r1cs::hash_7(ds, (rseed, fee.amount, fee.asset_id, compressed_g_d, tk_s, clue_key, inner_hash4))
```

### ZK/Kriptografik Yapılar

- **Commitment**: Poseidon hash (ZK-friendly, BLS12-377 field üzerinde)
- **Şifreleme**: `PayloadKey::derive_swap(ovk, commitment)` → AED şifreleme
- **R1CS**: `AllocVar<SwapPlaintext, Fq>` — Groth16 circuit'e witness olarak verilir
- **Nonce türetme**: `rseed` → `hash_1(DS_1, rseed)` ve `hash_1(DS_2, rseed)` → deterministik iki çıktı notu

### Testler

1. **swap_encryption_and_decryption** (Satır 426-468): OVK ile şifreleme/çözme round-trip

### DEX Uygulama Notu — Nova Shield için:

1. **swap_commitment yapısı**: `hash_7` + iç `hash_4` nested Poseidon yapısı Nova Shield için de kullanılabilir. Kritik: `rseed` hem commitment'a hem çıktı notlarına girdiği için tek bir entropi kaynağından tüm determinizm sağlanır.

2. **Plaintext layout (256 byte)**: Compact serialization. Nova Shield da aynı layout'u benimseyebilir. 80-byte adres alanı Penumbra'ya özgü; Nova Shield kendi adres formatına göre ayarlayabilir.

3. **ZK uyumluluk**: Swap commitment'ın ZK circuit içinde yeniden hesaplanması (enforce_equal) critical. Nova Shield'in ZK swap'ları için aynı yaklaşım: commitment integrity check circuit içinde yapılmalı.

4. **Gizli swap**: Swap girdisi (delta_1_i, delta_2_i, address) on-chain şifrelidir. Nova Shield'in MEV koruması bu modelden ilham alabilir: işlem içeriği şifreli, ZK kanıtı ile doğrulama.

---

## [proof.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/proof.rs`
**Satır Sayısı:** 431

### Genel Amaç

Swap işleminin Groth16 ZK kanıtı. Bir kullanıcının geçerli swap girdileri sağladığını (bakiye taahhüdü, swap taahhüdü, fee taahhüdü) şifreli biçimde kanıtlar.

### Struct/Enum/Type Listesi

#### `SwapProofPublic`
```rust
pub struct SwapProofPublic {
    pub balance_commitment: balance::Commitment,
    pub swap_commitment: tct::StateCommitment,
    pub fee_commitment: balance::Commitment,
}
```

#### `SwapProofPrivate`
```rust
pub struct SwapProofPrivate {
    pub fee_blinding: Fr,           // Fee commitment blinding faktörü
    pub swap_plaintext: SwapPlaintext,
}
```

#### `SwapCircuit` (R1CS Circuit)
```rust
pub struct SwapCircuit {
    public: SwapProofPublic,
    private: SwapProofPrivate,
}
```

#### `SwapProof([u8; GROTH16_PROOF_LENGTH_BYTES])`

### ZK Circuit Kısıtları (Satır 106-144) — KRITIK

```
// 1. Swap commitment integrity
swap_commitment_computed = swap_plaintext_var.commit()
enforce_equal(claimed_swap_commitment, swap_commitment_computed)

// 2. Fee commitment integrity
fee_balance = -Balance::from(claim_fee)
fee_commitment_computed = fee_balance.commit(fee_blinding)
enforce_equal(claimed_fee_commitment, fee_commitment_computed)

// 3. Balance commitment integrity
balance_1 = -Balance::from(delta_1_value)
balance_2 = -Balance::from(delta_2_value)
transparent_balance = balance_1.commit(0) + balance_2.commit(0)  // blinding=0 → şeffaf
total_balance = transparent_balance + fee_commitment
enforce_equal(claimed_balance_commitment, total_balance)
```

**Yani kullanıcı şunları kanıtlar:**
- Swap plaintext'inin commit'e uygun olduğunu (gizli bilgi ile public commitment tutarlı)
- Fee'nin doğru taahhüt edildiğini
- Balance commitment'ının delta_1 + delta_2 + fee tutarından oluştuğunu

**Önemli:** `delta_1` ve `delta_2` transparent commitment ile (blinding=0) taahhüt edilir — çünkü bunlar zaten public (Body'de açık).

### Fonksiyonlar

#### `SwapProof::prove()` (Satır 199-214)
```rust
Groth16::<Bls12_377, LibsnarkReduction>::create_proof_with_reduction(
    circuit, pk, blinding_r, blinding_s
)
```
ZK kanıt üretimi: Bls12-377 eğrisi, LibsnarkReduction QAP.

#### `SwapProof::verify()` (Satır 227-270)
Public inputs sırası: `[balance_commitment, swap_commitment, fee_commitment]`

### ZK/Kriptografik Yapılar

- **Eğri**: BLS12-377 (Bls12_377 — Penumbra'nın ana eğrisi)
- **ZK Sistemi**: Groth16
- **QAP Redüksiyon**: LibsnarkReduction
- **Field**: Fq (BLS12-377'nin temel field'ı)
- **Proof Boyutu**: `GROTH16_PROOF_LENGTH_BYTES` (genellikle 192 byte)

### Testler

1. **swap_proof_happy_path** (proptest) — rastgele geçerli statement'lar için circuit satisfaction
2. **swap_proof_invalid_fee_commitment** (proptest) — hatalı fee blinding tespiti

### DEX Uygulama Notu — Nova Shield için:

1. **ZK sisteminin bütünü**: Nova Shield'in ZK zinciri `arkworks + Groth16` kullanıyor (MEMORY.md). Bu, Penumbra ile aynı. Swap proof circuit'i doğrudan uyarlanabilir.

2. **3 public input**: `balance_commitment, swap_commitment, fee_commitment` — Nova Shield'in swap ZK'sı için minimum public input seti bu.

3. **Transparent delta**: `delta_1` ve `delta_2` public balance'a yansır (blinding=0). Bu gizliliği azaltır ama batch işlemi şeffaf tutar. Nova Shield için aynı trade-off geçerli.

4. **BLS12-377**: Nova Shield MEMORY'de BLS12-381 kullanıyor. Penumbra BLS12-377 kullanıyor. Fark: circuit içi Poseidon hash parametreleri farklı. Nova Shield için tüm hash parametrelerini BLS12-381'e uyarlamak gerekir.

---

## [action.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/action.rs`
**Satır Sayısı:** 143

### Struct/Enum/Type Listesi

#### `Swap`
```rust
pub struct Swap {
    pub proof: SwapProof,
    pub body: Body,
}
```

#### `Body`
```rust
pub struct Body {
    pub trading_pair: TradingPair,
    pub delta_1_i: Amount,
    pub delta_2_i: Amount,
    pub fee_commitment: balance::Commitment,
    pub payload: SwapPayload,  // encrypted swap + commitment
}
```

### Fonksiyonlar

#### `Swap::balance_commitment_inner()` (Satır 24-39)
```
input_1 = -Value{delta_1_i, asset_1}
input_2 = -Value{delta_2_i, asset_2}
commitment = input_1.commit(0) + input_2.commit(0) + fee_commitment
```

**Önemli:** `delta_1_i` ve `delta_2_i` hem `Body`'de plain görünür hem de ZK kanıtında kullanılır.

### DEX Uygulama Notu — Nova Shield için:

Swap action formatı compact: sadece ZK proof + public body. Gizli bilgi `payload` içinde şifreli. Nova Shield'in transaction formatı için bu şema referans alınabilir.

---

## [payload.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/payload.rs`
**Satır Sayısı:** 79

### Struct/Enum/Type Listesi

#### `SwapPayload`
```rust
pub struct SwapPayload {
    pub commitment: tct::StateCommitment,
    pub encrypted_swap: SwapCiphertext,
}
```

### Fonksiyonlar

#### `trial_decrypt(fvk) -> Option<SwapPlaintext>` (Satır 16-48)
1. `encrypted_swap.decrypt(fvk.outgoing(), commitment)`
2. Integrity check: `swap.swap_commitment() == self.commitment`
3. Sahiplik check: `fvk.incoming().views_address(&swap.claim_address)`
4. Başarısız ise `None` (hata değil — "REJECT" saldırılarına karşı)

### DEX Uygulama Notu — Nova Shield için:

`trial_decrypt` yaklaşımı (hata yerine None dönüş) scanning sırasında güvenlik sağlar. Nova Shield'in swap detection mekanizması için aynı pattern kullanılmalı.

---

## [ciphertext.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/ciphertext.rs`
**Satır Sayısı:** 55

### Struct/Enum/Type Listesi

#### `SwapCiphertext([u8; 272])`

### Fonksiyonlar

- `decrypt(ovk, commitment) -> Result<SwapPlaintext>`
  - `key = PayloadKey::derive_swap(ovk, commitment)`
  - `payload_key.decrypt_swap(ciphertext)` → `[u8; 256]` → `SwapPlaintext`
- `decrypt_with_payload_key(key) -> Result<SwapPlaintext>`

---

## [plan.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/plan.rs`
**Satır Sayısı:** 162

### Struct/Enum/Type Listesi

#### `SwapPlan`
```rust
pub struct SwapPlan {
    pub swap_plaintext: SwapPlaintext,
    pub fee_blinding: Fr,
    pub proof_blinding_r: Fq,
    pub proof_blinding_s: Fq,
}
```

### Fonksiyonlar

- `new(rng, swap_plaintext)` — `fee_blinding`, `r`, `s` rastgele üretir
- `swap(fvk) -> Swap` — Body + Proof oluşturur
- `swap_body(fvk) -> Body` — Payload şifreler
- `swap_proof() -> SwapProof` — Groth16 kanıtı üretir
- `fee_commitment() -> balance::Commitment` — `claim_fee.commit(fee_blinding)`
- `transparent_balance() -> Balance` — `-delta_1 - delta_2`
- `balance() -> Balance` — `transparent_balance - fee`

### DEX Uygulama Notu — Nova Shield için:

Plan pattern (SwapPlan → Swap) Nova Shield'in transaction builder'ı için model alınabilir. Tüm randomness (fee_blinding, proof_blinding) plan aşamasında üretilir; proof deterministik olarak hesaplanır.

---

## [view.rs] (Swap)

**Dosya:** `/crates/core/component/dex/src/swap/view.rs`
**Satır Sayısı:** 138

### Struct/Enum/Type Listesi

#### `SwapView`
```rust
pub enum SwapView {
    Visible {
        swap: Swap,
        swap_plaintext: SwapPlaintext,
        output_1: Option<NoteView>,
        output_2: Option<NoteView>,
        claim_tx: Option<TransactionId>,
        asset_1_metadata: Option<Metadata>,
        asset_2_metadata: Option<Metadata>,
        batch_swap_output_data: Option<BatchSwapOutputData>,
    },
    Opaque {
        swap: Swap,
        batch_swap_output_data: Option<BatchSwapOutputData>,
        output_1: Option<ValueView>,
        output_2: Option<ValueView>,
        asset_1_metadata: Option<Metadata>,
        asset_2_metadata: Option<Metadata>,
    },
}
```

FVK sahibi tam içeriği görür (Visible); diğerleri sadece public kısmı (Opaque).

---

## BÖLÜM 3 — BATCH SWAP OUTPUT DATA

---

## [batch_swap_output_data.rs]

**Dosya:** `/crates/core/component/dex/src/batch_swap_output_data.rs`
**Satır Sayısı:** 500

### Genel Amaç

Bir bloktaki bir trading pair için tüm batch swap sonuçlarını tutar. Pro-rata çıktı dağıtımı ve ZK circuit implementasyonu içerir.

### Struct/Enum/Type Listesi

#### `BatchSwapOutputData`
```rust
pub struct BatchSwapOutputData {
    pub delta_1: Amount,   // Toplam asset_1 girdi
    pub delta_2: Amount,   // Toplam asset_2 girdi
    pub lambda_1: Amount,  // Toplam asset_1 çıktı (2→1 takasları için)
    pub lambda_2: Amount,  // Toplam asset_2 çıktı (1→2 takasları için)
    pub unfilled_1: Amount, // Doldurulmayan asset_1
    pub unfilled_2: Amount, // Doldurulmayan asset_2
    pub height: u64,
    pub trading_pair: TradingPair,
    pub sct_position_prefix: Position,  // commitment.must == 0
}
```

#### `BatchSwapOutputDataVar` (R1CS versiyonu)
```rust
pub struct BatchSwapOutputDataVar {
    pub delta_1: U128x128Var,
    pub delta_2: U128x128Var,
    pub lambda_1: U128x128Var,
    pub lambda_2: U128x128Var,
    pub unfilled_1: U128x128Var,
    pub unfilled_2: U128x128Var,
    pub trading_pair: TradingPairVar,
    pub epoch: FqVar,
    pub block_within_epoch: FqVar,
}
```

### Fonksiyonlar

#### `pro_rata_outputs()` — MERKEZI FORMÜL (Satır 47-83) — KRITIK

**Formula (out-of-circuit):**
```
pro_rata_input_1 = delta_1_i / delta_1   (veya 0 eğer delta_1 = 0)
pro_rata_input_2 = delta_2_i / delta_2   (veya 0 eğer delta_2 = 0)

lambda_2_i = pro_rata_input_1 * lambda_2 + pro_rata_input_2 * unfilled_2
lambda_1_i = pro_rata_input_1 * unfilled_1 + pro_rata_input_2 * lambda_1

// Her iki miktar da aşağı yuvarlanır (round_down)
```

**Yorumu:**
- Kullanıcı delta_1_i girdi sağladıysa: hem lambda_2'den pay alır hem de unfilled_1'den (doldurulmayan kısmı geri alır)
- Kullanıcı delta_2_i girdi sağladıysa: hem lambda_1'den pay alır hem de unfilled_2'den

**Neden iki terim var?** Aynı blokta hem 1→2 hem 2→1 swap'lar olabilir. Her kullanıcı sadece kendi yönünden pay alır.

#### `BatchSwapOutputDataVar::pro_rata_outputs()` — R1CS Versiyonu (Satır 230-277)

ZK circuit içinde aynı formülü enforce eder:
```
// delta_1 = 0 koruması (ZK-friendly):
delta_1_is_zero = self.delta_1.is_eq(&zero)
divisor_1 = conditionally_select(delta_1_is_zero, one, self.delta_1)  // sıfır bölme önleme
division_result_1 = delta_1_i.checked_div(divisor_1)
pro_rata_input_1 = conditionally_select(delta_1_is_zero, zero, division_result_1)
// (aynısı delta_2 için)

lambda_2_i = pro_rata_input_1 * lambda_2 + pro_rata_input_2 * unfilled_2
lambda_1_i = pro_rata_input_1 * unfilled_1 + pro_rata_input_2 * lambda_1

// round_down → AmountVar'a çevir
```

#### `AllocVar<BatchSwapOutputData>` (Satır 151-202)

Epoch ve block için özel kısıt:
```rust
bit_constrain(epoch.clone(), 16)?;           // epoch < 2^16
bit_constrain(block_within_epoch.clone(), 16)?;  // block < 2^16
```

#### `ToConstraintField<Fq>` (Satır 86-137)

Public input sırası: `delta_1, delta_2, lambda_1, lambda_2, unfilled_1, unfilled_2, asset_1, asset_2, epoch, block`

### Testler

1. **pasiphae_inflation_bug** (Satır 356-388): Gerçek chain verisiyle regression test. Önemli: "inflation bug" adlı bir hata giderilmiş.
2. **happy_path_bsod_pro_rata** (Satır 451-498): Pro-rata hesaplama + Groth16 kanıtı ile doğrulama

### Tasarım Kararları

1. **Batch clearing**: Tüm aynı blokta gelen aynı pair swap'ları tek bir batch'te işlenir. Bu, MEV koruması sağlar — sıra farkı yoktur.
2. **Pro-rata dağıtım**: Herkes aynı clearing fiyatından işlem yapar. Büyük trader avantajı yoktur.
3. **sct_position_prefix.commitment() == 0**: TCT position'ının commitment kısmının sıfır olması zorunlu — bu swap zamanlamasını blok'a sabitleyen kısıt.
4. **Pasiphae inflation bug**: Sıfır bölme koruması gerekliydi. Test bunu belgeler.

### DEX Uygulama Notu — Nova Shield için:

**SON DERECE KRITIK — BATCH SWAP TASARIMI:**

1. **Pro-rata formülü**: `lambda_2_i = (delta_1_i/delta_1)*lambda_2 + (delta_2_i/delta_2)*unfilled_2`
   Nova Shield'in MEV-korumalı DEX'i için bu formül doğrudan kullanılabilir. Batch içinde herkes eşit muamele görür.

2. **ZK-içi pro-rata**: `BatchSwapOutputDataVar::pro_rata_outputs()` ZK circuit'e dahil edilir. Bu, kullanıcının kendi payını doğru hesapladığını kanıtlar. Nova Shield'in ZK swap claim'leri için bu yaklaşım şart.

3. **Sıfır bölme koruması**: ZK içinde `conditionally_select(is_zero, one, divisor)` → `conditionally_select(is_zero, zero, result)`. Bu pattern tüm ZK bölme işlemlerinde kullanılmalı.

4. **unfilled miktarlar**: `unfilled_1` ve `unfilled_2` iade edilmelidir. Nova Shield'in batch swap'ında da kısmen doldurulmuş swaplar için bu iade mekanizması tasarlanmalı.

5. **Inflation bug koruması**: Sıfır delta'yla bölme, potansiyel inflation vektörüdür. Nova Shield'de her bölme işleminde sıfır kontrolü zorunlu.

---

## BÖLÜM 4 — SWAP CLAIM

---

## [swap_claim/action.rs]

**Dosya:** `/crates/core/component/dex/src/swap_claim/action.rs`
**Satır Sayısı:** 133

### Struct/Enum/Type Listesi

#### `SwapClaim`
```rust
pub struct SwapClaim {
    pub proof: SwapClaimProof,
    pub body: Body,
    pub epoch_duration: u64,
}
```

**balance():** Sadece claim_fee döndürür → pre-paid fee'yi geri alır.

#### `Body`
```rust
pub struct Body {
    pub nullifier: Nullifier,                     // Swap'ı harcadığını kanıtlar
    pub fee: Fee,                                 // Pre-paid fee
    pub output_1_commitment: tct::StateCommitment,
    pub output_2_commitment: tct::StateCommitment,
    pub output_data: BatchSwapOutputData,
}
```

### DEX Uygulama Notu — Nova Shield için:

SwapClaim'in Nullifier içermesi kritik: aynı swap iki kez claim edilemez. Nova Shield'in nullifier mekanizması bu şemaya göre tasarlanmalı.

---

## [swap_claim/proof.rs]

**Dosya:** `/crates/core/component/dex/src/swap_claim/proof.rs`
**Satır Sayısı:** 917

### Genel Amaç

SwapClaim'in Groth16 ZK kanıtı. Bir kullanıcının gerçekten o swap'ı yaptığını, meşru çıktıları hesapladığını ve nullifier'ı doğru türettiğini kanıtlar.

### Struct/Enum/Type Listesi

#### `SwapClaimProofPublic`
```rust
pub struct SwapClaimProofPublic {
    pub anchor: tct::Root,                       // TCT kök hash
    pub nullifier: Nullifier,
    pub claim_fee: Fee,
    pub output_data: BatchSwapOutputData,
    pub note_commitment_1: note::StateCommitment,
    pub note_commitment_2: note::StateCommitment,
}
```

#### `SwapClaimProofPrivate`
```rust
pub struct SwapClaimProofPrivate {
    pub swap_plaintext: SwapPlaintext,
    pub state_commitment_proof: tct::Proof,  // TCT Merkle path
    pub ak: VerificationKey<SpendAuth>,      // Spend authorization key
    pub nk: NullifierKey,
    pub lambda_1: Amount,
    pub lambda_2: Amount,
    pub note_blinding_1: Fq,
    pub note_blinding_2: Fq,
}
```

### ZK Circuit Kısıtları — `SwapClaimCircuit::generate_constraints()` (Satır 190-301)

**9 KRİTİK KISIT:**

```
1. Swap commitment integrity:
   computed = swap_plaintext_var.commit()
   enforce_equal(claimed_swap_commitment, computed)

2. Merkle path integrity (TCT):
   merkle_path_var.verify(cs, TRUE, position_bits, anchor_var, swap_commitment.inner())

3. Nullifier integrity:
   nullifier_computed = NullifierVar::derive(nk, position, swap_commitment)
   enforce_equal(nullifier_computed, claimed_nullifier)

4. IVK-address connection:
   ivk = IncomingViewingKeyVar::derive(nk, ak)
   computed_tk = ivk.diversified_public(swap_plaintext.claim_address.g_d)
   enforce_equal(computed_tk, swap_plaintext.claim_address.transmission_key)

5. Fee consistency:
   enforce_equal(claimed_fee, swap_plaintext.claim_fee)

6. Height match (block):
   enforce_equal(output_data.block_within_epoch, position.block())

7. Height match (epoch):
   enforce_equal(output_data.epoch, position.epoch())

8. Trading pair match:
   enforce_equal(output_data.trading_pair, swap_plaintext.trading_pair)

9. Output amounts:
   (lambda_1_i, lambda_2_i) = output_data.pro_rata_outputs(delta_1_i, delta_2_i)
   enforce_equal(lambda_1_i, private.lambda_1)
   enforce_equal(lambda_2_i, private.lambda_2)

10. Output note commitments:
    note_1 = NoteVar{address, {lambda_1, asset_1}, note_blinding_1}
    note_commitment_1 = note_1.commit()
    enforce_equal(note_commitment_1, claimed_note_commitment_1)
    (aynısı note_2 için)
```

### ZK/Kriptografik Yapılar

- **Eğri**: BLS12-377
- **ZK**: Groth16
- **TCT**: Tiered Commitment Tree (Merkle path doğrulaması)
- **Nullifier**: `Nullifier::derive(nk, position, swap_commitment)`
- **IVK türetme**: `IncomingViewingKeyVar::derive(nk, ak)` — ZK içinde
- **Not commitment**: Poseidon hash (ZK içinde NoteVar::commit())

### Testler

4 proptest:
1. **swap_claim_proof_happy_path_filled** — filled BSOD
2. **swap_claim_proof_happy_path_unfilled** — unfilled BSOD
3. **swap_claim_proof_invalid_fee** — hatalı fee tespiti
4. **swap_claim_proof_invalid_swap_commitment_height** — height mismatch tespiti

### DEX Uygulama Notu — Nova Shield için:

**ULTRA KRITIK — SWAP CLAIM ZK TASARIMI:**

1. **10 kısıt seti**: SwapClaim circuit'i Nova Shield için model. Özellikle kısıt 6-7 (height match): swap'ın yapıldığı blok ile batch output data'nın aynı bloğa ait olduğunu enforce etmek, farklı dönemlerin karışmasını önler.

2. **Merkle path doğrulaması**: TCT Merkle path ZK içinde doğrulanır. Nova Shield'in kendi commitment tree'si (arkworks SCT) için aynı yaklaşım.

3. **Nullifier = ZK double-spend koruması**: `Nullifier::derive(nk, position, commitment)` formülü, aynı swap'ın iki kez claim edilmesini önler. Nova Shield'in MEV-korumalı DEX'inde bu zorunlu.

4. **IVK-adres bağlantısı**: ZK içinde `ivk.diversified_public(g_d) == transmission_key` kısıtı, yalnızca meşru adres sahibinin claim yapabilmesini garanti eder.

5. **note_blinding**: Çıktı notları deterministik rseed'den türetilir ama blinding faktörü ZK'ya gizli witness olarak girer. Bu, çıktı notların gizliliğini garanti eder.

---

## [swap_claim/plan.rs]

**Dosya:** `/crates/core/component/dex/src/swap_claim/plan.rs`
**Satır Sayısı:** 185

### Struct/Enum/Type Listesi

#### `SwapClaimPlan`
```rust
pub struct SwapClaimPlan {
    pub swap_plaintext: SwapPlaintext,
    pub position: Position,              // TCT pozisyonu
    pub output_data: BatchSwapOutputData,
    pub epoch_duration: u64,
    pub proof_blinding_r: Fq,
    pub proof_blinding_s: Fq,
}
```

### Fonksiyonlar

- `swap_claim(fvk, state_commitment_proof) -> SwapClaim` — Body + Proof
- `swap_claim_proof()` — Groth16 üretimi (SWAPCLAIM_PROOF_PROVING_KEY kullanır)
- `swap_claim_body()` — nullifier + note commitments + output data
- `is_viewed_by(ivk) -> bool`
- `balance()` → sadece pre-paid fee (değer geri döner)

---

## BÖLÜM 5 — ÜST DÜZEY DEX TIPLERI

---

## [trading_pair.rs]

**Dosya:** `/crates/core/component/dex/src/trading_pair.rs`
**Satır Sayısı:** 330

### Struct/Enum/Type Listesi

#### `DirectedTradingPair`
```rust
pub struct DirectedTradingPair {
    pub start: asset::Id,
    pub end: asset::Id,
}
```
Yönlü çift. `flip()`, `to_canonical()` metodları.

#### `TradingPair` (Canonical)
```rust
pub struct TradingPair {
    pub(crate) asset_1: asset::Id,
    pub(crate) asset_2: asset::Id,
}
```

**new(a, b):** `a < b` ise `{a, b}`; `a >= b` ise `{b, a}`. Her zaman sıralı.

**to_bytes() → [u8; 64]:** `asset_1(32) || asset_2(32)`

**Validasyon:** `TryFrom<[u8; 64]>` canonical sırayı enforce eder.

#### `TradingPairVar` (R1CS)
```rust
pub struct TradingPairVar {
    pub asset_1: asset::AssetIdVar,
    pub asset_2: asset::AssetIdVar,
}
```
`new_variable_unchecked` — canonical sıralama enforce edilmez (güvenlik notu: commitment integrity zaten enforce eder).

#### `DirectedUnitPair`
Human-readable format: `Unit` tiplerini tutar. CLI'da kullanıcıya gösterim için.

### Trait Implementasyonları

- `ToConstraintField<Fq> for TradingPair` — ZK public input
- `EqGadget<Fq> for TradingPairVar` — ZK equality check
- `FromStr for TradingPair` — "DENOM1:DENOM2" format
- `Display for TradingPair` — "ASSET_ID1:ASSET_ID2"

### DEX Uygulama Notu — Nova Shield için:

`TradingPair::new()` ile canonical sıralama Nova Shield'de de zorunlu. Her çift için `asset_1 < asset_2` kuralı hashmap anahtarlarında tutarlılık sağlar. Nova Shield'in DEX'i için bu primitive doğrudan kopyalanabilir.

---

## [batch_swap_output_data.rs] — yukarıda analiz edildi

---

## [candlestick.rs]

**Dosya:** `/crates/core/component/dex/src/candlestick.rs`
**Satır Sayısı:** 57

### Struct/Enum/Type Listesi

#### `CandlestickData`
```rust
pub struct CandlestickData {
    pub height: u64,
    pub open: f64,
    pub close: f64,
    pub high: f64,
    pub low: f64,
    pub direct_volume: f64,   // Bireysel pozisyon işlemleri
    pub swap_volume: f64,     // Multi-route swap'lar
}
```

### DEX Uygulama Notu — Nova Shield için:

`f64` kullanımı dikkat çekici — consensus-critical veri değil, client tarafı görüntüleme için. Nova Shield'in analitik katmanında aynı yapı kullanılabilir ama consensus verisinde `f64` kullanmaktan kaçınılmalı.

---

## [swap_execution.rs]

**Dosya:** `/crates/core/component/dex/src/swap_execution.rs`
**Satır Sayısı:** 79

### Struct/Enum/Type Listesi

#### `SwapExecution`
```rust
pub struct SwapExecution {
    pub traces: Vec<Vec<Value>>,  // Her hop'un değerleri
    pub input: Value,
    pub output: Value,
}
```

**Trace formatı:** `[asset_A_amount, hop1_amount, hop2_amount, ..., asset_B_amount]`

**max_price():** `traces.last()` → son trace'in input/output oranı (en kötü fiyat = limit fiyatı).

### DEX Uygulama Notu — Nova Shield için:

Routing trace'leri müşteri tarafı analitik için kritik. Nova Shield'in router'ı her hop'u kaydetmeli; bu trace, kullanıcıya "neden bu fiyattan aldın" açıklaması sunar.

---

## [event.rs]

**Dosya:** `/crates/core/component/dex/src/event.rs`
**Satır Sayısı:** 739

### Event Tipleri

| Event | İçerik |
|-------|--------|
| `EventSwap` | trading_pair, delta_1_i, delta_2_i, swap_commitment |
| `EventSwapClaim` | trading_pair, output_1/2 commitment, nullifier |
| `EventPositionOpen` | position_id, trading_pair, reserves_1/2, trading_fee, position |
| `EventPositionClose` | position_id |
| `EventQueuePositionClose` | position_id |
| `EventPositionWithdraw` | position_id, trading_pair, reserves_1/2, sequence |
| `EventPositionExecution` | position_id, trading_pair, reserves (current+prev), context |
| `EventBatchSwap` | batch_swap_output_data, swap_execution_1_for_2/2_for_1 |
| `EventArbExecution` | height, swap_execution |
| `EventValueCircuitBreakerCredit` | asset_id, previous_balance, new_balance |
| `EventValueCircuitBreakerDebit` | asset_id, previous_balance, new_balance |
| `EventCandlestickData` | pair, stick |
| `EventLqtPositionVolume` | epoch_index, asset_id, position_id, volume, total_volume, staking_token_in, asset_in, staking_fees, asset_fees |

### Önemli Event'ler

**`EventPositionExecution`** — hem önceki hem mevcut rezervleri içerir. Zincir dışı analiz için altın değer.

**`EventValueCircuitBreakerCredit/Debit`** — Değer devresinin kırılma noktalarını izler. DEX overflow koruması.

**`EventLqtPositionVolume`** — LQT (Liquidity Mining) mekanizması. Volume bazlı ödüller.

### DEX Uygulama Notu — Nova Shield için:

Nova Shield DEX event sistemi bu listeyi referans alabilir. Özellikle `EventPositionExecution` (önceki + mevcut rezerv), on-chain analitik için kritik. `EventValueCircuitBreakerCredit/Debit` değer koruma auditlemesi için zorunlu.

---

## [genesis.rs]

**Dosya:** `/crates/core/component/dex/src/genesis.rs`
**Satır Sayısı:** 46

### Struct/Enum/Type Listesi

#### `Content`
```rust
pub struct Content {
    pub dex_params: DexParameters,
}
```

Default değerler: `DexParameters::default()`.

---

## [params.rs]

**Dosya:** `/crates/core/component/dex/src/params.rs`
**Satır Sayısı:** 78

### Struct/Enum/Type Listesi

#### `DexParameters`
```rust
pub struct DexParameters {
    pub is_enabled: bool,
    pub fixed_candidates: Vec<asset::Id>,
    pub max_hops: u32,
    pub max_positions_per_pair: u32,
    pub max_execution_budget: u32,
}
```

### Default Değerler (Satır 56-77)
```rust
is_enabled: true,
fixed_candidates: [STAKING_TOKEN, test_usd, gm, gn, test_atom, test_osmo, test_btc],
max_hops: 4,                 // Maksimum routing hop sayısı
max_positions_per_pair: 1000, // Her pair için max pozisyon
max_execution_budget: 64,     // Her blokta max işlem birimi
```

### DEX Uygulama Notu — Nova Shield için:

**KRITIK — Routing Parametreleri:**

1. **max_hops = 4**: Routing algoritması derinliğini sınırlar. 4 hop = O(n^4) karmaşıklık potansiyeli. Nova Shield için bu değer dikkatli seçilmeli.

2. **max_positions_per_pair = 1000**: Her trading pair için 1000 pozisyon limiti. Bu, state büyümesini sınırlar. Nova Shield'de benzer bir limit zorunlu.

3. **max_execution_budget = 64**: Her blokta işlenebilen "birim" sayısı. Bu parametrenin semantiği routing/execution engine'de belirlenir.

4. **fixed_candidates**: Routing hub'ı olarak kullanılan sabit asset'ler. Nova Shield'in routing için `NOVA` tokenını hub olarak belirlemeyi düşünebilir.

---

## [state_key.rs]

**Dosya:** `/crates/core/component/dex/src/state_key.rs`
**Satır Sayısı:** 337

### Genel Amaç

DEX state'in key-value store'da nasıl organize edildiğini tanımlar.

### Key Hiyerarşisi

```
dex/config/dex_params
dex/value_balance/{asset_id}
dex/positions/{trading_pair}/opened/{position_id}
dex/position/{id}           → tüm pozisyonlar
dex/position/               → prefix scan
dex/output/{height:020}/{asset_1}/{asset_2}   → BatchSwapOutputData
dex/swap_execution/{height:020}/{start}/{end}
dex/arb_execution/{height:020}
dex/swap_flows
dex/pending_position_closures
dex/recently_accessed_assets
dex/pending_payloads
dex/pending_outputs
dex/aggregate_value
```

### Price Index (Satır 311-335) — KRITIK

```
key = "dex/pi/" || start_bytes(32) || end_bytes(32) || effective_price_bytes(32) || position_id(32)
     = [u8; 7] + [u8; 32] + [u8; 32] + [u8; 32] + [u8; 32] = 135 bytes
```

Bu encoding ile **prefix scan + lexicographic sıra = price-ordered scan**. En düşük fiyattan başlayarak tüm pozisyonları sırayla gezmek O(N) scan'dir.

### Routable Assets Index (Satır 265-308)

```
"dex/ra/" || from_asset(32) → başlangıç prefix
"dex/ra/" || from_asset(32) || BE(!aggregate_liquidity)(16) → primary key
"dex/ab/" || start_asset(32) || end_asset(32) → lookup key
```

`!aggregate_liquidity` (bitwise NOT + big-endian) trick: Likit varlıklar önce gelsin diye ters sıralama.

### LQT Volume Index (Satır 122-237)

```
"dex/lqt/v1/lp/by_volume/{epoch:020}/" || asset(32) || BE(!volume)(16) || position_id(32)
```
Total 125 bytes. Volume'ün bitwise NOT'u ile en yüksek volume en başa gelir.

### Eviction Queue (Satır 338-373)

```
"dex/internal/eviction_queue/inventory_index" || start(32) || end(32) || inventory(16) || position_id(32)
= 155 bytes
```

Düşük inventory'li pozisyonları evict etmek için sıralı index.

### DEX Uygulama Notu — Nova Shield için:

**KRITIK — Key-Value Store Tasarımı:**

1. **Price index (135 byte key)**: `price_bytes(32) || position_id(32)` soneki ile lexicographic = price sırası. Nova Shield'in RocksDB DEX indexi için doğrudan kopyalanabilir. Bu, routing engine'inin en ucuz pozisyonu O(1) bulmasını sağlar.

2. **`!aggregate_liquidity` trick**: Büyük değerlerin küçük görünmesi için bitwise NOT + big-endian. Nova Shield'in herhangi bir "en yüksek X" indekslemesinde bu trick kullanılabilir.

3. **Height-padded keys**: `{height:020}` → 20 karakter ile sıfır dolgulu yükseklik. Bu, belirli blok aralığında scan'i mümkün kılar. Nova Shield'in her blok çıktısını key'e gömmesi önerilir.

4. **Prefix scan pattern**: `dex/position/` prefix'i ile tüm pozisyonları scan etmek mümkün. Nova Shield'in KV store'u bu pattern'ı desteklemeli.

5. **LQT volume index**: Nova Shield token'ının likidite madenciliği programı için bu index yapısı kullanılabilir.

---

## [lib.rs]

**Dosya:** `/crates/core/component/dex/src/lib.rs`
**Satır Sayısı:** 29

### Re-exportlar

```rust
pub use batch_swap_output_data::BatchSwapOutputData;
pub use candlestick::CandlestickData;
pub use params::DexParameters;
pub use swap_execution::SwapExecution;
pub use trading_pair::{DirectedTradingPair, DirectedUnitPair, TradingPair, TradingPairVar};
pub use lp::action::{PositionClose, PositionOpen, PositionWithdraw};
pub use swap::Swap;
pub use swap_claim::SwapClaim;
```

### Özellikler

- `#![deny(clippy::unwrap_used)]` — `.unwrap()` kullanımı yasak!
- `#[cfg(feature = "component")]` — Execution engine ayrı feature

---

## GENEL SENTEZ

### Penumbra DEX'in Ana Bileşenleri

```
┌─────────────────────────────────────────────────────────────┐
│                    PENUMBRA DEX                              │
├─────────────────────────────────────────────────────────────┤
│  LP Katmanı                                                  │
│  ├── TradingFunction: phi(R) = p*R1 + q*R2 (constant-sum)   │
│  ├── Position: state machine (Opened→Closed→Withdrawn{n})    │
│  ├── LpNft: NFT tabanlı sahiplik ve state geçişi             │
│  └── BuyOrder/SellOrder: p=desired, q=offered formülü       │
├─────────────────────────────────────────────────────────────┤
│  Batch Swap Katmanı                                          │
│  ├── SwapPlaintext: şifreli swap içeriği                     │
│  │   ├── swap_commitment = Poseidon hash_7(rseed, fee, addr, hash_4(pair, deltas))
│  │   └── Serialized: 256 byte sabit format                   │
│  ├── SwapProof (Groth16): 3 kısıt                            │
│  │   ├── swap commitment integrity                            │
│  │   ├── fee commitment integrity                             │
│  │   └── balance commitment integrity                         │
│  └── BatchSwapOutputData: pro-rata dağıtım                   │
│      └── lambda_i = (delta_i/delta)*lambda + (delta_j/delta_j)*unfilled_j
├─────────────────────────────────────────────────────────────┤
│  SwapClaim Katmanı                                           │
│  ├── SwapClaimProof (Groth16): 10 kısıt                      │
│  │   ├── swap commitment integrity                            │
│  │   ├── Merkle path (TCT)                                    │
│  │   ├── nullifier integrity                                  │
│  │   ├── IVK-address bağlantısı                               │
│  │   ├── fee consistency                                      │
│  │   ├── height match (epoch + block)                         │
│  │   ├── trading pair match                                   │
│  │   ├── pro-rata output amounts                              │
│  │   └── output note commitments                              │
│  └── Nullifier: double-spend koruması                        │
└─────────────────────────────────────────────────────────────┘
```

### Teknik Değerler Özeti

| Parametre | Değer | Kaynak |
|-----------|-------|--------|
| MAX_RESERVE_AMOUNT | `(1 << 80) - 1` | position.rs:15 |
| MAX_FEE_BPS | 5000 (50%) | position.rs:18 |
| gamma formula | `(10_000 - fee) / 10_000` | trading_function.rs:444 |
| effective_price (2→1) | `(q/p) / gamma` | trading_function.rs:417 |
| effective_price_inv (1→2) | `(p/q) * gamma` | trading_function.rs:406 |
| Position ID | `blake2b_personal("penumbra_lp_id", nonce\|\|asset1\|\|asset2\|\|fee\|\|p\|\|q)` | position.rs:146 |
| Swap commitment | `Poseidon hash_7(DS, rseed, fee.amount, fee.asset_id, g_d, tk_s, clue_key, hash_4(DS, asset1, asset2, delta1, delta2))` | plaintext.rs:98 |
| SWAP_LEN_BYTES | 256 | swap.rs:23 |
| SWAP_CIPHERTEXT_BYTES | 272 | swap.rs:21 |
| Pro-rata formula | `lambda_2_i = (delta_1_i/delta_1)*lambda_2 + (delta_2_i/delta_2)*unfilled_2` | batch_swap_output_data.rs:47 |
| max_hops (default) | 4 | params.rs:71 |
| max_positions_per_pair | 1000 | params.rs:73 |
| max_execution_budget | 64 | params.rs:74 |
| Encrypted metadata size | 50 bytes (24 nonce + 26 ciphertext) | action.rs:23 |
| ZK System | Groth16 on BLS12-377 | proof.rs:7 |
| Price index key | 135 bytes | state_key.rs:329 |

### Nova Shield DEX — Direktif Eylem Planı

#### ANINDA UYGULANABİLİR (Copy-adapt)

1. **`TradingPair::new()`** ile canonical sıralama (satır 83-95 trading_pair.rs)
2. **`BareTradingFunction::fill()`** çift-dallanmalı algoritması (satır 290-388 trading_function.rs)
3. **`BatchSwapOutputData::pro_rata_outputs()`** formülü (satır 47-83 batch_swap_output_data.rs)
4. **Price index key design** (satır 311-335 state_key.rs) — RocksDB DEX indeksi için
5. **Position ID: blake2b** tabanlı hash (satır 146-161 position.rs)
6. **State machine: Opened→Closed→Withdrawn{n}** + NFT yakıp mint pattern
7. **`!aggregate_liquidity` + big-endian trick** (satır 288-293 state_key.rs) — sıralı routing index

#### UYARLANMASI GEREKEN

1. **BLS12-377 → BLS12-381**: Poseidon hash parametreleri değişmeli. ZK circuit'leri yeniden derlenmeli.
2. **Swap commitment**: Penumbra adresi 80 byte → Nova Shield adres formatı farklı.
3. **max_hops parametresi**: Nova Shield'in token ekonomisi ve gas limitlerine göre ayarlanmalı.
4. **`compose()` metodu**: Penumbra implement etmedi — Nova Shield için bu boşluğu doldur (multi-position composable liquidity).

#### TASARIM RİSKLERİ

1. **Constant-sum vs Constant-product**: Penumbra her pozisyon için constant-sum kullanır ama aggregate davranış constant-product'a yaklaşır. Tek pozisyon için büyük swap slipage yüksek olabilir. Nova Shield, farklı pozisyon boyutları için slipage hesaplamasını test etmeli.

2. **ZK kanıt süresi**: Groth16 kanıt üretimi client-side. Nova Shield'in kullanıcı deneyimi için kanıt üretim süresini minimize etmek gerekebilir.

3. **`f64` in CandlestickData**: Consensus-critical olmayan ama dikkatli olunmalı. Nova Shield'in herhangi bir consensus hesaplamasında floating point kullanılmamalı.

---

## UNSAFE BLOKLAR

Tüm analiz edilen 35 dosyada sıfır `unsafe` blok. Penumbra'nın veri katmanı tamamen safe Rust.

---

## TEST KAPSAMLARI

| Dosya | Test Sayısı | Test Türü |
|-------|-------------|-----------|
| trading_function.rs | 5 | Unit |
| position.rs | 2 | Unit |
| nft.rs | 1 | Unit |
| order.rs | 2 | Unit |
| metadata.rs | 8 | Unit |
| plaintext.rs | 1 | Unit |
| proof.rs (swap) | 2 | Proptest |
| batch_swap_output_data.rs | 2 | Unit + ZK |
| proof.rs (swapclaim) | 4 | Proptest |

**Toplam:** ~27 test. Proptest kullanımı (özellikle proof dosyalarında) yüksek kapsamlı özellik testlemesi sağlar.

---

*Agent DEX-A tamamlandı. Toplam 35 dosya, ~3500+ satır analiz edildi.*
*Bir sonraki agent (DEX-B) routing engine, component implementasyonu ve execution katmanını analiz etmelidir.*
