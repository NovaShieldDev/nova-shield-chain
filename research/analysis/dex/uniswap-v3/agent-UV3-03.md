# UV3-03: Tick + TickBitmap + Position + Fee Growth İçi Matematigi

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v3-core/`
**Kapsanan Dosyalar:**
- `contracts/libraries/Tick.sol` (185 satır — tam analiz)
- `contracts/libraries/TickBitmap.sol` (78 satır — tam analiz)
- `contracts/libraries/Position.sol` (88 satır — tam analiz)

---

## 1. Tick.Info Struct — 256 Bit'ten Fazla Storage

```solidity
struct Tick.Info {
    uint128 liquidityGross;                   // Tick başvuran toplam likidite
    int128  liquidityNet;                     // Tick geçilince eklenen/çıkarılan net likidite
    uint256 feeGrowthOutside0X128;            // Token0 fee birikimi (diğer taraf)
    uint256 feeGrowthOutside1X128;            // Token1 fee birikimi (diğer taraf)
    int56   tickCumulativeOutside;            // TWAP kümülatif (diğer taraf)
    uint160 secondsPerLiquidityOutsideX128;   // Saniye/likidite (diğer taraf)
    uint32  secondsOutside;                   // Geçen süre (diğer taraf)
    bool    initialized;                      // Tick aktif mi?
}
```

**Storage cost:** ~4 slot (128 büyüklüğüne bağlı olarak)

---

## 2. liquidityNet ve liquidityGross — İnvariant Analizi

### 2.1 liquidityNet

`liquidityNet`, tick sol-sağ geçilince aktif likiditeye eklenir:
- Pozisyon açıldığında lower tick: `liquidityNet += liquidityDelta`
- Pozisyon açıldığında upper tick: `liquidityNet -= liquidityDelta`

**Kritik İnvariant:** Tüm başlatılmış tick'lerin liquidityNet toplamı sıfır olmalıdır:
```
∑ liquidityNet[i] = 0    (tüm başlatılmış tick'ler için)
```

Bu matematiksel olarak zorunlu — her pozisyon bir lower ve upper tick'e eşit ama zıt işaretli katkı yapar.

**Geçiş mantığı (swap'ta):**
```
zeroForOne (sağdan sola, tick azalır):
  liquidity -= liquidityNet   (çünkü liquidityNet = -liquidityNet_tersi)

oneForZero (soldan sağa, tick artar):
  liquidity += liquidityNet
```

Kod:
```solidity
// Pool.sol satır 719-720:
if (zeroForOne) liquidityNet = -liquidityNet;
state.liquidity = LiquidityMath.addDelta(state.liquidity, liquidityNet);
```

### 2.2 liquidityGross

`liquidityGross`, o tick'e başvuran TÜM pozisyonların mutlak likidite toplamı:
```
liquidityGross = ∑ |liquidityDelta_i|  (o tick'teki tüm pozisyonlar)
```

**Neden gerekli?**
- `liquidityGross > 0` → tick başlatılmış (aktif)
- `liquidityGross == 0` → tick boş → `tickBitmap.flipTick()` çağrılır
- `liquidityGross` max kontrolü: `require(liquidityGrossAfter <= maxLiquidity, 'LO')`

---

## 3. feeGrowthOutside — Döngüsel Aritmetik ile Fee Takibi

Bu, V3'ün en zekice kısımlarından biri. "Outside" terimi kasıtlı belirsiz — görecelidir.

### 3.1 Sözleşme (Convention)

Tick başlatıldığında (`liquidityGross == 0` → non-zero geçişi):

```solidity
if (tick <= tickCurrent) {
    // Fiyat bu tick'in üzerindeyken başlatıldı
    info.feeGrowthOutside0X128 = feeGrowthGlobal0X128;
    info.feeGrowthOutside1X128 = feeGrowthGlobal1X128;
    info.secondsPerLiquidityOutsideX128 = secondsPerLiquidityCumulativeX128;
    info.tickCumulativeOutside = tickCumulative;
    info.secondsOutside = time;
} else {
    // Fiyat bu tick'in altındayken başlatıldı → outside = 0 (tümü "içeride")
    // Zaten varsayılan sıfır, SSTORE maliyetinden kaçınılır
}
```

**Semantik:** `feeGrowthOutside` = "bu tick'in mevcut tick'e göre karşı tarafındaki fee birikimi".

Mevcut tick bu tick'in:
- Üzerindeyse: `feeGrowthOutside` = **altında** biriken fee
- Altındaysa: `feeGrowthOutside` = **üzerinde** biriken fee

### 3.2 cross() — Tick Geçişinde Döngüsel Güncelleme

```solidity
function cross(...) returns (int128 liquidityNet) {
    info.feeGrowthOutside0X128 = feeGrowthGlobal0X128 - info.feeGrowthOutside0X128;
    info.feeGrowthOutside1X128 = feeGrowthGlobal1X128 - info.feeGrowthOutside1X128;
    info.secondsPerLiquidityOutsideX128 = secondsPerLiquidityCumulativeX128 - info.secondsPerLiquidityOutsideX128;
    info.tickCumulativeOutside = tickCumulative - info.tickCumulativeOutside;
    info.secondsOutside = time - info.secondsOutside;
    liquidityNet = info.liquidityNet;
}
```

**Formül:** `feeGrowthOutside_new = feeGrowthGlobal - feeGrowthOutside_old`

Bu flip işlemi şu anlama gelir: "Karşı tarafın birikimi = Global - Bu tarafın birikimi"

**Overflow kasıtlı:** `feeGrowthGlobal - feeGrowthOutside` uint256 underflow olabilir. Solidity 0.7'de bu wrap-around yapar. Fark hesabında bu iptal olur → her zaman doğru sonuç.

### 3.3 getFeeGrowthInside — Pozisyon Fee Hesabı

```
Koşul 1: currentTick >= tickLower
  feeGrowthBelow = lower.feeGrowthOutside  (alt kısım, doğrudan)

Koşul 2: currentTick < tickLower
  feeGrowthBelow = feeGrowthGlobal - lower.feeGrowthOutside  (flip gerekli)

Koşul 3: currentTick < tickUpper
  feeGrowthAbove = upper.feeGrowthOutside  (üst kısım, doğrudan)

Koşul 4: currentTick >= tickUpper
  feeGrowthAbove = feeGrowthGlobal - upper.feeGrowthOutside  (flip gerekli)

SONUÇ:
  feeGrowthInside = feeGrowthGlobal - feeGrowthBelow - feeGrowthAbove
```

**Görsel açıklama:**

```
tickLower              tickUpper
    |                      |
----[=====INSIDE===========]----
<feeBelow><--feeInside--><feeAbove>

feeInside = feeGlobal - feeBelow - feeAbove
```

**Neden bu çalışır?**

Tick geçişlerinde `cross()` çağrıldığında `feeGrowthOutside` flip olur. Bu flip, hesaplama sırasındaki "mirror" etkisini otomatik olarak dengeler. Uint256 overflow da bu sayede sorunsuz çalışır çünkü:

```
(A - B) - (A - C) = C - B  (mod 2^256)
```

### 3.4 tick.update() — Flipped Durumu

```solidity
flipped = (liquidityGrossAfter == 0) != (liquidityGrossBefore == 0)
```

**Flipped durumları:**
- `0 → 1`: Tick yeni başlatıldı → TickBitmap'te bit set edilir
- `N → 0`: Tick boşaltıldı → TickBitmap'te bit silinir
- `N → M (ikisi de ≠ 0)`: flipped = false → Bitmap değişmez

---

## 4. TickBitmap.sol — O(1) Tick Keşfi

### 4.1 Veri Yapısı

```
mapping(int16 => uint256) tickBitmap
```

Her uint256 word: 256 tick'i temsil eder (tick spacing'e bölünmüş)

**Toplam kapasite:**
- int16 key: 65536 kelime
- 256 bit per kelime: 65536 × 256 = 16.7M olası tick pozisyonu
- V3'ün gerçek tick aralığı: ±887272 / tickSpacing

### 4.2 `position()` — Tick'i Bitmap Konumuna Çevir

```solidity
function position(int24 tick) private pure returns (int16 wordPos, uint8 bitPos) {
    wordPos = int16(tick >> 8);   // Hangi kelimede? (tick / 256)
    bitPos = uint8(tick % 256);   // Kelime içinde hangi bit? (tick mod 256)
}
```

**Örnek:**
- tick = 600 → wordPos = 2 (600 >> 8 = 2), bitPos = 88 (600 % 256 = 88)
- tick = -1  → wordPos = -1 (EVM: int16(-1) = 0xFFFF), bitPos = 255

**Negatif tick'te dikkat:**
```
tick = -1 → tick >> 8 = -1 (int24 sign extend)
tick % 256 = -1 % 256 = 255 (Solidity'de negatif modulo sonucu pozitif değil)
```

Aslında kod `int24 tick / tickSpacing` sonrası `position(compressed)` çağrılır.

### 4.3 `flipTick()` — Tick Durumunu Değiştir

```solidity
function flipTick(mapping(int16 => uint256) storage self, int24 tick, int24 tickSpacing) {
    require(tick % tickSpacing == 0);  // Sadece geçerli tick'ler
    (int16 wordPos, uint8 bitPos) = position(tick / tickSpacing);
    uint256 mask = 1 << bitPos;
    self[wordPos] ^= mask;  // XOR ile toggle
}
```

**XOR toggle:** Bit 0 ise → 1, 1 ise → 0. Tek SSTORE.

### 4.4 `nextInitializedTickWithinOneWord()` — Swap'ta Sonraki Tick

#### lte = true (zeroForOne, sola git — düşük tick'e)

```
compressed = tick / tickSpacing  (negatif için floor division)

wordPos, bitPos = position(compressed)

// Tüm bitler bu pozisyon ve solundaki (daha düşük değerlere doğru):
mask = (1 << bitPos) - 1 + (1 << bitPos)
     = (1 << (bitPos + 1)) - 1

masked = self[wordPos] & mask

if masked != 0:
    // En soldaki set biti bul (en yüksek değerli)
    msb = BitMath.mostSignificantBit(masked)
    next = (compressed - (bitPos - msb)) * tickSpacing
else:
    // Bu kelimede hiç başlatılmış tick yok → kelime başına git
    next = (compressed - bitPos) * tickSpacing
```

**Formül:** `next = (compressed - (bitPos - MSB(masked))) * tickSpacing`

#### lte = false (oneForZero, sağa git — yüksek tick'e)

```
compressed + 1  (current tick dahil edilmez)

wordPos, bitPos = position(compressed + 1)

// Tüm bitler bu pozisyon ve sağındaki (daha yüksek değerlere doğru):
mask = ~((1 << bitPos) - 1)  // Bu pozisyon ve üzeri

masked = self[wordPos] & mask

if masked != 0:
    lsb = BitMath.leastSignificantBit(masked)
    next = (compressed + 1 + (lsb - bitPos)) * tickSpacing
else:
    next = (compressed + 1 + (255 - bitPos)) * tickSpacing
```

### 4.5 Negatif Tick İçin Floor Division

```solidity
int24 compressed = tick / tickSpacing;
if (tick < 0 && tick % tickSpacing != 0) compressed--;
```

EVM'de integer bölme sıfıra doğru yuvarlar (trunc), ama tick sınırları için negatif sonsuz yönünde (floor) yuvarlama gerekir:

```
-5 / 10 = 0 (EVM)  →  -1 (floor)
-10 / 10 = -1 (EVM) → -1 (doğru, düzeltme yok)
```

Düzeltme: `tick < 0 && tick % tickSpacing != 0` durumunda `-1` eksilt.

---

## 5. Position.sol — Kullanıcı Pozisyon Yönetimi

### 5.1 Position.Info Struct

```solidity
struct Position.Info {
    uint128 liquidity;                    // Pozisyonun likiditesi
    uint256 feeGrowthInside0LastX128;    // Son güncelleme anındaki feeGrowthInside (token0)
    uint256 feeGrowthInside1LastX128;    // Son güncelleme anındaki feeGrowthInside (token1)
    uint128 tokensOwed0;                 // Birikmiş token0 fee hakediş
    uint128 tokensOwed1;                 // Birikmiş token1 fee hakediş
}
```

**Storage:** 4 slot (32 + 32 + 32 + 8 + 8 + 4 + 4 byte)
Aslında: `liquidity(16) + tokensOwed0(16)` = 32 byte → 1 slot, `feeGrowthInside0(32)`, `feeGrowthInside1(32)` → toplamda 3 slot

### 5.2 Position Key — keccak256 Hash

```solidity
function get(mapping, address owner, int24 tickLower, int24 tickUpper)
    returns (Position.Info storage)
{
    return self[keccak256(abi.encodePacked(owner, tickLower, tickUpper))];
}
```

**Neden hash?** EVM mapping key olarak sadece 32-byte değer kullanabilir. Üç parametre (address=20B + int24=3B + int24=3B = 26B) sığdırılmış — hash çakışma ihtimali astronomik derecede düşük (2^-256).

**Nova Shield'de alternatif:**
```rust
#[derive(Hash, Eq, PartialEq)]
pub struct PositionKey {
    pub owner: [u8; 32],   // AccountId
    pub tick_lower: i32,
    pub tick_upper: i32,
}
// HashMap<PositionKey, PositionInfo> — hash yerine native struct
```

### 5.3 `position.update()` — Fee Hesabı Formülü

```
tokensOwed0 = (feeGrowthInside0_current - feeGrowthInside0_last) × liquidity / Q128
tokensOwed1 = (feeGrowthInside1_current - feeGrowthInside1_last) × liquidity / Q128
```

**Tam kod:**
```solidity
uint128 tokensOwed0 = uint128(
    FullMath.mulDiv(
        feeGrowthInside0X128 - _self.feeGrowthInside0LastX128,  // Fark (overflow safe)
        _self.liquidity,
        FixedPoint128.Q128
    )
);
```

**Overflow analizi:**
- `feeGrowthInside0X128 - feeGrowthInside0LastX128` → uint256 underflow kasıtlı (wrap)
- Bu fark, pozisyon aktifken biriken fee'yi gösterir (doğru)
- `mulDiv` → tam 512-bit hassasiyet
- `uint128(...)` → sonuç her zaman < 2^128 (ispat: `liquidity < 2^128, fark < Q128`)

**Sıfır pozisyon güncelleme (poke):**
```solidity
if (liquidityDelta == 0) {
    require(_self.liquidity > 0, 'NP'); // Sıfır likiditeli pozisyon update edilemez
    liquidityNext = _self.liquidity;
}
```

"Poke" — likidite değişmeksizin fee tahakkuk ettirir.

---

## 6. tickSpacingToMaxLiquidityPerTick Formülü

```solidity
function tickSpacingToMaxLiquidityPerTick(int24 tickSpacing) returns (uint128) {
    int24 minTick = (TickMath.MIN_TICK / tickSpacing) * tickSpacing;
    int24 maxTick = (TickMath.MAX_TICK / tickSpacing) * tickSpacing;
    uint24 numTicks = uint24((maxTick - minTick) / tickSpacing) + 1;
    return type(uint128).max / numTicks;
}
```

**Mantık:** Tüm tick'lerdeki toplam likiditeler `liquidity` (uint128) aşamaz. Her tick eşit paylaşırsa max = `uint128.MAX / numTicks`.

**Hesaplama örnekleri:**

| tickSpacing | numTicks | maxLiqPerTick |
|-------------|----------|---------------|
| 1 | 1,774,545 | ~1.91 × 10^32 |
| 10 | 177,455 | ~1.91 × 10^33 |
| 60 | 29,575 | ~1.15 × 10^34 |
| 200 | 8,869 | ~3.84 × 10^34 |

---

## 7. Fee Birikimi: Uçtan Uca Akış

### 7.1 Global Fee Birikimi (Swap sırasında)

```
feeGrowthGlobal0X128 += feeAmount × Q128 / liquidity
```

Bu değer her swap adımında artar. Teknik olarak "likidite birimi başına toplam biriken fee" dir.

### 7.2 Tick Level (feeGrowthOutside)

Her tick, kendi "dış tarafında" biriken fee'yi saklar. `cross()` geçişinde flip olur.

### 7.3 Pozisyon Level (feeGrowthInside)

```
feeGrowthInside = feeGrowthGlobal - feeGrowthBelow - feeGrowthAbove
```

Son mint/burn/poke anından bu yana artış = ödenmemiş fee.

### 7.4 Kullanıcı Hakediş (tokensOwed)

```
tokensOwed += (feeGrowthInside_current - feeGrowthInside_last) × liquidity / Q128
```

`collect()` çağrısında bu tutar kullanıcıya transfer edilir.

### 7.5 Örnek Akış

```
T=0: Pool başlatılır, feeGrowthGlobal = 0
T=1: Alice mint: tickLower=1000, tickUpper=2000, liquidity=1000
     → feeGrowthInside0Last = 0 (henüz fee yok)
T=2: Swap 0.03% fee, 100 token0 swap edildi, feeAmount=0.03 token0
     liquidity=1000, feeGrowthGlobal0 += 0.03 × Q128 / 1000
     = 0.00003 × Q128
T=3: Bob da mint yaptıysa ve aktif tick ikisinde de: fee paylaşılır orantılı
T=4: Alice burn eder:
     feeGrowthInside0 = feeGrowthGlobal - 0 - 0 = 0.00003 × Q128
     tokensOwed0 = (0.00003 × Q128 - 0) × 1000 / Q128 = 0.03 token0
```

---

## 8. Güvenlik Analizi

### 8.1 liquidityNet Taşma Koruması

```solidity
info.liquidityNet = upper
    ? int256(info.liquidityNet).sub(liquidityDelta).toInt128()
    : int256(info.liquidityNet).add(liquidityDelta).toInt128();
```

`int256` üzerinde SafeMath ile hesap yapılır, sonra `toInt128()` ile güvenli cast. `type(int128).min` durumu özel not: "safe because liquidityNet cannot be type(int128).min" — cross() içinde `-liquidityNet` yapılır, min değerde bu overflow olur.

### 8.2 TickBitmap Sınır Taşması

`nextInitializedTickWithinOneWord()` sadece bir kelime (256 tick aralığı) içinde arar. Pool swap döngüsü tick sınırı kontrolü yapar:

```solidity
if (step.tickNext < TickMath.MIN_TICK) step.tickNext = TickMath.MIN_TICK;
else if (step.tickNext > TickMath.MAX_TICK) step.tickNext = TickMath.MAX_TICK;
```

### 8.3 tokensOwed Overflow

```solidity
// overflow is acceptable, have to withdraw before you hit type(uint128).max fees
self.tokensOwed0 += tokensOwed0;
self.tokensOwed1 += tokensOwed1;
```

Kasıtlı overflow — kullanıcı `type(uint128).max` önce çekmelidir. Pratikte imkansız miktarda fee gerektirir.

---

## 9. Nova Shield Chain — Rust Implementasyon Notları

### 9.1 Tick Struct

```rust
#[derive(Debug, Clone, Default)]
pub struct TickInfo {
    pub liquidity_gross: u128,
    pub liquidity_net: i128,
    pub fee_growth_outside_0_x128: U256,
    pub fee_growth_outside_1_x128: U256,
    pub tick_cumulative_outside: i64,
    pub seconds_per_liquidity_outside_x128: U256,
    pub seconds_outside: u32,
    pub initialized: bool,
}

impl TickInfo {
    pub fn cross(
        &mut self,
        fee_growth_global_0: U256,
        fee_growth_global_1: U256,
        seconds_per_liq_cumulative: U256,
        tick_cumulative: i64,
        time: u32,
    ) -> i128 {
        // Wrap-around aritmetiği — Rust'ta wrapping_sub kullan
        self.fee_growth_outside_0_x128 = fee_growth_global_0.wrapping_sub(self.fee_growth_outside_0_x128);
        self.fee_growth_outside_1_x128 = fee_growth_global_1.wrapping_sub(self.fee_growth_outside_1_x128);
        self.seconds_per_liquidity_outside_x128 = seconds_per_liq_cumulative
            .wrapping_sub(self.seconds_per_liquidity_outside_x128);
        self.tick_cumulative_outside = tick_cumulative.wrapping_sub(self.tick_cumulative_outside);
        self.seconds_outside = time.wrapping_sub(self.seconds_outside);
        self.liquidity_net
    }
}
```

### 9.2 TickBitmap → Rust HashMap + Native Bit ops

```rust
pub struct TickBitmap {
    inner: HashMap<i16, u256>,
}

impl TickBitmap {
    fn position(tick: i32) -> (i16, u8) {
        let word_pos = (tick >> 8) as i16;
        let bit_pos = (tick & 0xFF) as u8;  // mod 256 (tick pozitifse)
        (word_pos, bit_pos)
    }

    pub fn flip_tick(&mut self, tick: i32, tick_spacing: i32) {
        assert_eq!(tick % tick_spacing, 0);
        let (word_pos, bit_pos) = Self::position(tick / tick_spacing);
        let mask = 1u256 << bit_pos;
        *self.inner.entry(word_pos).or_insert(0) ^= mask;
    }

    pub fn next_initialized_tick_within_one_word(
        &self, tick: i32, tick_spacing: i32, lte: bool
    ) -> (i32, bool) {
        let mut compressed = tick / tick_spacing;
        if tick < 0 && tick % tick_spacing != 0 {
            compressed -= 1;  // Floor division
        }

        if lte {
            let (word_pos, bit_pos) = Self::position(compressed);
            let mask = (1u256 << (bit_pos + 1)).wrapping_sub(1);
            let masked = self.inner.get(&word_pos).copied().unwrap_or(0) & mask;
            let initialized = masked != 0;
            let next = if initialized {
                let msb = 255 - masked.leading_zeros() as i32;
                (compressed - (bit_pos as i32 - msb)) * tick_spacing
            } else {
                (compressed - bit_pos as i32) * tick_spacing
            };
            (next, initialized)
        } else {
            let (word_pos, bit_pos) = Self::position(compressed + 1);
            let mask = !((1u256 << bit_pos).wrapping_sub(1));
            let masked = self.inner.get(&word_pos).copied().unwrap_or(0) & mask;
            let initialized = masked != 0;
            let next = if initialized {
                let lsb = masked.trailing_zeros() as i32;
                (compressed + 1 + (lsb - bit_pos as i32)) * tick_spacing
            } else {
                (compressed + 1 + (255 - bit_pos as i32)) * tick_spacing
            };
            (next, initialized)
        }
    }
}
```

### 9.3 Fee Growth Inside — Rust'ta Wrapping Arithmetic

```rust
pub fn get_fee_growth_inside(
    ticks: &HashMap<i32, TickInfo>,
    tick_lower: i32,
    tick_upper: i32,
    tick_current: i32,
    fee_growth_global_0: U256,
    fee_growth_global_1: U256,
) -> (U256, U256) {
    let lower = ticks.get(&tick_lower).expect("lower not initialized");
    let upper = ticks.get(&tick_upper).expect("upper not initialized");

    // Fee aşağıda
    let (fee_below_0, fee_below_1) = if tick_current >= tick_lower {
        (lower.fee_growth_outside_0_x128, lower.fee_growth_outside_1_x128)
    } else {
        (
            fee_growth_global_0.wrapping_sub(lower.fee_growth_outside_0_x128),
            fee_growth_global_1.wrapping_sub(lower.fee_growth_outside_1_x128),
        )
    };

    // Fee yukarıda
    let (fee_above_0, fee_above_1) = if tick_current < tick_upper {
        (upper.fee_growth_outside_0_x128, upper.fee_growth_outside_1_x128)
    } else {
        (
            fee_growth_global_0.wrapping_sub(upper.fee_growth_outside_0_x128),
            fee_growth_global_1.wrapping_sub(upper.fee_growth_outside_1_x128),
        )
    };

    (
        fee_growth_global_0.wrapping_sub(fee_below_0).wrapping_sub(fee_above_0),
        fee_growth_global_1.wrapping_sub(fee_below_1).wrapping_sub(fee_above_1),
    )
}
```

### 9.4 Position Update

```rust
pub fn update_position(
    pos: &mut PositionInfo,
    liquidity_delta: i128,
    fee_growth_inside_0: U256,
    fee_growth_inside_1: U256,
) {
    let tokens_owed_0 = full_math::mul_div(
        fee_growth_inside_0.wrapping_sub(pos.fee_growth_inside_0_last_x128),
        U256::from(pos.liquidity),
        Q128,
    );
    let tokens_owed_1 = full_math::mul_div(
        fee_growth_inside_1.wrapping_sub(pos.fee_growth_inside_1_last_x128),
        U256::from(pos.liquidity),
        Q128,
    );

    if liquidity_delta != 0 {
        pos.liquidity = liquidity_math::add_delta(pos.liquidity, liquidity_delta);
    }
    pos.fee_growth_inside_0_last_x128 = fee_growth_inside_0;
    pos.fee_growth_inside_1_last_x128 = fee_growth_inside_1;
    pos.tokens_owed_0 = pos.tokens_owed_0.wrapping_add(tokens_owed_0 as u128);
    pos.tokens_owed_1 = pos.tokens_owed_1.wrapping_add(tokens_owed_1 as u128);
}
```

---

## Özet: 3 Kritik Bulgu

1. **cross() Döngüsel Flip Aritmetiği:** `feeGrowthOutside = feeGrowthGlobal - feeGrowthOutside` formülü kasıtlı uint256 underflow kullanır. Fark hesaplamalarında bu sıfırlanır. Nova Shield'de Rust'ta `wrapping_sub` zorunlu — checked_sub kullanılırsa panic.

2. **TickBitmap O(1) Sonraki Tick:** Swap döngüsünün her adımında `nextInitializedTickWithinOneWord` MSB/LSB operasyonları ile sabit sürede çalışır. Nova Shield'de Rust native `leading_zeros()` / `trailing_zeros()` kullanılacak.

3. **liquidityNet ∑ = 0 İnvariantı:** Tüm aktif tick'lerin net likidite toplamı her zaman sıfırdır. Nova Shield'de bu invariant debug modda `assert!` ile doğrulanacak.
