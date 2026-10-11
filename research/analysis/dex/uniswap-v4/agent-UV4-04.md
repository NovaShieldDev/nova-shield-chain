# UV4-04: Uniswap V3 vs V4 Matematik Kütüphaneleri Karşılaştırması + BitMath De Bruijn + SwapMath Yenilikler

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v4-core/`
**Kapsanan Dosyalar:**
- `src/libraries/BitMath.sol` (V4 versiyonu)
- `src/libraries/SwapMath.sol` (V4 versiyonu)
- `src/libraries/SqrtPriceMath.sol` (V4 versiyonu)
- `src/libraries/TickMath.sol` (V4 versiyonu)
- `src/libraries/FullMath.sol` (V4 versiyonu)
- `src/libraries/UnsafeMath.sol` (V4)
- `src/libraries/TickBitmap.sol` (V4)
- V3 vs V4 Karşılaştırma

---

## 1. BitMath — De Bruijn'den Binary Search'e Geçiş

### 1.1 V3 BitMath: Binary Search (Yukarı-Aşağı)

```solidity
// V3 mostSignificantBit:
function mostSignificantBit(uint256 x) returns (uint8 r) {
    require(x > 0);
    if (x >= 0x100000000000000000000000000000000) { x >>= 128; r += 128; }
    if (x >= 0x10000000000000000) { x >>= 64; r += 64; }
    // ... 7 daha adım
    if (x >= 0x2) r += 1;
}
```

**Algoritma:** Her adımda yarıya böl. 8 conditional branch.

### 1.2 V4 BitMath: Değişmedi, V3 ile Aynı

V4 BitMath, V3 ile yapısal olarak aynı binary search kullanır. Fark yok — ancak V4 test coverage genişledi.

### 1.3 V4 TickBitmap'e nextInitializedTickWithinOneWord Değişikliği

```solidity
// V4 TickBitmap — tick sınır kontrolü değişti:
if (step.tickNext <= TickMath.MIN_TICK) {  // V3: < MIN_TICK
    step.tickNext = TickMath.MIN_TICK;
}
if (step.tickNext >= TickMath.MAX_TICK) {  // V3: > MAX_TICK
    step.tickNext = TickMath.MAX_TICK;
}
```

V4'te `<=` ve `>=` (inclusive). Bu, MIN_TICK ve MAX_TICK'in tick geçişinde dahil edilmesini sağlar.

---

## 2. SwapMath — V4'teki Önemli Değişiklikler

### 2.1 MAX_SWAP_FEE Sabiti

```solidity
// V4 SwapMath
uint256 internal constant MAX_SWAP_FEE = 1e6;  // 100%
```

V3'te böyle bir sabit yoktu. V4'te `swapFee >= MAX_SWAP_FEE && exactOutput` → revert.

### 2.2 getSqrtPriceTarget() — Branchless Implementation

```solidity
function getSqrtPriceTarget(
    bool zeroForOne,
    uint160 sqrtPriceNextX96,
    uint160 sqrtPriceLimitX96
) internal pure returns (uint160 sqrtPriceTarget) {
    assembly {
        sqrtPriceTarget := xor(
            sqrtPriceLimitX96,
            mul(
                xor(sqrtPriceNextX96, sqrtPriceLimitX96),
                lt(sqrtPriceNextX96, sqrtPriceLimitX96) == zeroForOne
            )
        )
    }
}
```

**Matematiksel analiz:**
```
Durum A: zeroForOne=true, sqrtPriceNextX96 < sqrtPriceLimitX96
  lt(next, limit) = 1, zeroForOne = 1
  (1 == 1) = 1
  mul(..., 1) = xor(next, limit)
  xor(limit, xor(next, limit)) = next
  → min(next, limit) = next ✓

Durum B: zeroForOne=true, sqrtPriceNextX96 >= sqrtPriceLimitX96
  lt(next, limit) = 0, zeroForOne = 1
  (0 == 1) = 0
  mul(..., 0) = 0
  xor(limit, 0) = limit
  → min(next, limit) = limit ✓

Durum C: zeroForOne=false, sqrtPriceNextX96 < sqrtPriceLimitX96
  lt(next, limit) = 1, zeroForOne = 0
  (1 == 0) = 0
  mul(..., 0) = 0
  xor(limit, 0) = limit
  → max(next, limit) = limit ✓

Durum D: zeroForOne=false, sqrtPriceNextX96 >= sqrtPriceLimitX96
  lt(next, limit) = 0, zeroForOne = 0
  (0 == 0) = 1
  mul(..., 1) = xor(next, limit)
  xor(limit, xor(next, limit)) = next
  → max(next, limit) = next ✓
```

**Sonuç:** Branchless `min/max` implementasyonu. V3'te ternary operatör (`? :`) kullanılıyordu.

### 2.3 computeSwapStep — V4 vs V3

**Fark 1: exactOutput Semantiği**

```solidity
// V3: exactInput = amountRemaining >= 0
bool exactIn = amountRemaining >= 0;

// V4: exactInput = amountSpecified < 0 (negatif = exactIn)
bool exactInput = amountSpecified < 0;
```

**Fark 2: amountSpecifiedRemaining işaretinin kullanımı**

V3:
```
exactIn: amountRemainingLessFee = amountRemaining × (1e6 - fee) / 1e6
```

V4:
```
exactInput: amountRemainingLessFee = (-amountSpecifiedRemaining) × (1e6 - fee) / 1e6
// Negatif işaret çevriliyor
```

**Fark 3: Return değerleri**

V3: `(sqrtRatioNextX96, amountIn, amountOut, feeAmount)` — dört ayrı değer

V4: Aynı imza, ama `MAX_SWAP_FEE` kontrolü eklendi.

---

## 3. TickMath — V4 Değişiklikleri

### 3.1 İsimlendirme Değişikliği

```
V3:                          V4:
getSqrtRatioAtTick()    →   getSqrtPriceAtTick()
getTickAtSqrtRatio()    →   getTickAtSqrtPrice()
MIN_SQRT_RATIO          →   MIN_SQRT_PRICE
MAX_SQRT_RATIO          →   MAX_SQRT_PRICE
```

"Ratio" → "Price" terminoloji değişikliği.

### 3.2 TickSpacing Sınırları (V4 Yeni)

```solidity
int24 internal constant MAX_TICK_SPACING = type(int16).max;  // 32767
int24 internal constant MIN_TICK_SPACING = 1;
```

V3'te factory'de kontrol ediliyordu. V4'te TickMath'te sabit.

### 3.3 MAX_SQRT_PRICE_MINUS_MIN_SQRT_PRICE_MINUS_ONE

```solidity
uint160 internal constant MAX_SQRT_PRICE_MINUS_MIN_SQRT_PRICE_MINUS_ONE =
    MAX_SQRT_PRICE - MIN_SQRT_PRICE - 1;
```

Bu sabit, price range doğrulamasında kullanılır:
```solidity
// Hem MIN hem MAX dışında mı?
if (sqrtPriceX96 - MIN_SQRT_PRICE >= MAX_SQRT_PRICE_MINUS_MIN_SQRT_PRICE_MINUS_ONE) {
    revert PriceOutOfBounds();
}
// Eşdeğer: sqrtPriceX96 < MIN_SQRT_PRICE || sqrtPriceX96 >= MAX_SQRT_PRICE
// Ama tek karşılaştırma ile (branchless)
```

**Neden çalışır?**
```
x - MIN < (MAX - MIN - 1)
⟺ MIN ≤ x AND x < MAX - 1
⟺ MIN ≤ x AND x < MAX  (MAX - 1 ve MAX arası fark <1 tick)
```

uint256 underflow-safe çünkü `x >= MIN_SQRT_PRICE` olduğunda `x - MIN` her zaman ≥ 0.

---

## 4. SqrtPriceMath — V4 Değişiklikleri

### 4.1 İsimlendirme

```
V3:                                      V4:
getNextSqrtPriceFromAmount0RoundingUp  → getNextSqrtPriceFromAmount0RoundingUp (aynı)
getNextSqrtPriceFromAmount1RoundingDown → getNextSqrtPriceFromAmount1RoundingDown (aynı)
getAmount0Delta(sqrtA, sqrtB, liq, roundUp) → aynı
getAmount1Delta(sqrtA, sqrtB, liq, roundUp) → aynı
```

### 4.2 Signed Delta Fonksiyonları

```solidity
// V4'te imzalı versiyonlar daha az kullanılıyor, unsigned + toInt128() tercih ediliyor
function getAmount0Delta(
    uint160 sqrtPriceAX96,
    uint160 sqrtPriceBX96,
    int128 liquidity
) internal pure returns (int256 amount0) {
    unchecked {
        return liquidity < 0
            ? -getAmount0Delta(sqrtPriceAX96, sqrtPriceBX96, uint128(-liquidity), false).toInt256()
            : getAmount0Delta(sqrtPriceAX96, sqrtPriceBX96, uint128(liquidity), true).toInt256();
    }
}
```

---

## 5. FullMath — V4 Değişikliği

### 5.1 V3 FullMath vs V4 FullMath

V4 FullMath, V3 FullMath ile özdeş algoritmik olarak. Tek fark:
- V4 `Solidity ^0.8.0` kullanır → `unchecked {}` blokları gerekli
- `prod1 := sub(sub(mm, prod0), lt(mm, prod0))` ifadesi `unchecked` içinde

### 5.2 UnsafeMath.simpleMulDiv — V4'e Özgü

```solidity
// V4 UnsafeMath — YENİ:
function simpleMulDiv(uint256 x, uint256 y, uint256 d) internal pure returns (uint256 z) {
    assembly ("memory-safe") {
        z := div(mul(x, y), d)
    }
}
```

`FullMath.mulDiv` yerine: **overflow kontrolü yok**. Yalnızca `x × y < 2^256` garantili durumlarda kullanılır.

**Nerede kullanılır:**
```
feeGrowthGlobal += UnsafeMath.simpleMulDiv(feeAmount, Q128, liquidity)
```
`feeAmount ≤ 2^127`, `Q128 = 2^128` → `feeAmount × Q128 ≤ 2^255` → overflow yok.

**donate():**
```
amount0 ≤ int128.max ≈ 2^127, Q128 = 2^128 → 2^255 < 2^256 → güvenli
```

---

## 6. V3 vs V4 Gas Karşılaştırması

### 6.1 Tek Swap Gas Tasarrufu

| Operasyon | V3 (gas) | V4 (gas) | Fark |
|-----------|----------|----------|------|
| Reentrancy lock | ~5000 (SSTORE) | ~100 (TSTORE) | -98% |
| Slot0 okuma | ~2100 (SLOAD) | ~100 (TLOAD, lock) | -95% |
| Oracle yazma | ~5000 (SSTORE) | 0 (kaldırıldı) | -100% |
| Token transfer | 2× ~21000 | 0 (net settle) | -100% |
| Pool deploy | ~1.5M | 0 (singleton) | -100% |

### 6.2 Multi-hop Swap Tasarrufu

V3 (3 hop): `6 × token.transfer() + 3 × SSTORE(unlock)`
V4 (3 hop): `2 × token.transfer() + 0 (flash accounting)`

### 6.3 Tick Math Optimizasyonları

| Fonksiyon | V3 | V4 | Değişim |
|-----------|-----|-----|---------|
| getSqrtPriceTarget | ternary `? :` | branchless xor/mul | ~10 gas |
| price bounds check | 2 compare | 1 compare (arithmetic trick) | ~10 gas |
| tick bounds | `<` / `>` | `<=` / `>=` | minimal |

---

## 7. Solidity Versiyonu Farkları: 0.7.x vs 0.8.x

### 7.1 Overflow Davranışı

```
V3 (0.7.x): overflow sessizce wrap-around yapar
  → uint256 kasıtlı overflow (feeGrowthGlobal vb.) için SafeMath YOK

V4 (0.8.x): overflow/underflow otomatik revert
  → kasıtlı wrap için `unchecked {}` bloğu zorunlu
  → V4 kodunda çok sayıda `unchecked { ... }` bloku var
```

### 7.2 Custom Errors

```
V3: require(condition, 'ERR') → 4+ byte string encoding
V4: ErrorName.selector.revertWith(param) → 4 byte selector
```

Gas tasarrufu: revert data küçüldü.

### 7.3 Custom Types

```
V3: struct Slot0 { ... }
V4: type Slot0 is bytes32; → daha az ABI encoding overhead
```

---

## 8. UnsafeMath.sol — Tam Analiz

```solidity
library UnsafeMath {
    /// @notice Returns ceil(x / y). Assumes y != 0
    function divRoundingUp(uint256 x, uint256 d) internal pure returns (uint256 z) {
        assembly ("memory-safe") {
            z := add(div(x, d), gt(mod(x, d), 0))
        }
    }

    /// @notice Returns floor(x * y / d). Assumes result fits in uint256 and d != 0
    function simpleMulDiv(uint256 x, uint256 y, uint256 d) internal pure returns (uint256 z) {
        assembly ("memory-safe") {
            z := div(mul(x, y), d)
        }
    }
}
```

`divRoundingUp`: V3'ten aynı.
`simpleMulDiv`: V4'e özgü — overflow-free varsayımı ile.

---

## 9. Nova Shield Chain — Kritik Tasarım Kararları

### 9.1 Hangi Matematik Kütüphanesi?

```rust
// Tier 1 — Zorunlu implement:
// - FullMath (512-bit mulDiv) → Rust'ta U256+U512 veya custom
// - TickMath (getSqrtPriceAtTick / getTickAtSqrtPrice) → 20 magic constant
// - SqrtPriceMath (amount delta ve next sqrt price) → 4 fonksiyon
// - SwapMath (computeSwapStep) → 1 ana fonksiyon
// - BitMath (MSB/LSB) → Rust builtin leading_zeros/trailing_zeros

// Tier 2 — Optimize edilebilir:
// - UnsafeMath.simpleMulDiv → Rust checked_mul + checked_div
// - LowGasSafeMath → Rust checked_* zaten var
// - SafeCast → Rust as + panic
```

### 9.2 Branchless Optimizasyonlar → Rust'ta

```rust
// getSqrtPriceTarget eşdeğeri — Rust'ta closure:
pub fn get_sqrt_price_target(
    zero_for_one: bool,
    sqrt_price_next: u160,
    sqrt_price_limit: u160,
) -> u160 {
    if zero_for_one {
        sqrt_price_next.min(sqrt_price_limit)
    } else {
        sqrt_price_next.max(sqrt_price_limit)
    }
}
// Rust optimizer büyük olasılıkla bu if'i branchless yapar (CMOV instruction)
```

### 9.3 MAX_SWAP_FEE Kuralı

```rust
const MAX_SWAP_FEE: u32 = 1_000_000;  // 100%

pub fn validate_swap_fee(swap_fee: u32, is_exact_output: bool) -> Result<(), DexError> {
    if swap_fee >= MAX_SWAP_FEE && is_exact_output {
        return Err(DexError::InvalidFeeForExactOut);
    }
    Ok(())
}
```

### 9.4 Nova Shield için Seçilen Yaklaşım

| Konu | V3 | V4 | Nova Shield Seçimi |
|------|-----|-----|-------------------|
| exactIn/exactOut işareti | +/- | -/+ | V4 (negatif=exactIn) |
| Oracle | Gömülü | Hook | Ayrı modül (event-based) |
| Fee format | 1/N | pips | V4 pips |
| Overflow | Silent | unchecked | Rust checked_* (panic) |
| Tick bounds | Exclusive | Inclusive | V4 |
| Position salt | Yok | bytes32 | Desteklenecek |

---

## 10. ProtocolFeeLibrary — Tam Analiz

```solidity
library ProtocolFeeLibrary {
    uint16 public constant MAX_PROTOCOL_FEE = 1000;  // 0.1%
    uint16 internal constant PIPS_DENOMINATOR = 1000;

    // protocolFee: uint24 = (oneForZero << 12) | zeroForOne
    // Her yarısı 0-1000 arası (12 bit yeterli, max 1000)

    function getZeroForOneFee(uint24 self) returns (uint16) {
        return uint16(self & 0xFFF);
    }
    function getOneForZeroFee(uint24 self) returns (uint16) {
        return uint16(self >> 12);
    }

    function calculateSwapFee(uint16 self, uint24 lpFee) returns (uint24) {
        // Combined fee: protocolFee + lpFee - protocolFee*lpFee/DENOM
        return uint24(uint256(self) + uint256(lpFee) - uint256(self) * uint256(lpFee) / uint256(PIPS_DENOMINATOR));
    }

    function isValidProtocolFee(uint24 self) returns (bool) {
        if (self != 0) {
            uint16 fee0 = self.getZeroForOneFee();
            uint16 fee1 = self.getOneForZeroFee();
            if (fee0 > MAX_PROTOCOL_FEE || fee1 > MAX_PROTOCOL_FEE) return false;
        }
        return true;
    }
}
```

---

## Özet: 3 Kritik Bulgu

1. **getSqrtPriceTarget Branchless:** V4 `xor(limit, mul(xor(next, limit), eq(lt(next, limit), zeroForOne)))` → branch'sız `min/max`. Rust'ta LLVM zaten `if/else min/max`'ı CMOV'a optimize eder. Eşdeğer performans.

2. **UnsafeMath.simpleMulDiv — Overflow Varsayımı:** V4 `feeGrowthGlobal += feeAmount × Q128 / liquidity` için `FullMath.mulDiv` yerine `simpleMulDiv` kullanır. Bu güvenli çünkü `feeAmount ≤ int128.max ≈ 2^127` ve `Q128 = 2^128` → çarpım `≤ 2^255`. Nova Shield'de Rust `u128::checked_mul` + `u256::from()` ile eşdeğer implementasyon.

3. **MAX_SQRT_PRICE_MINUS_MIN_SQRT_PRICE_MINUS_ONE Trick:** Tek aritmetik işlemle price range sınır kontrolü. `x - MIN < (MAX - MIN - 1)` → `MIN ≤ x < MAX`. Nova Shield'de Rust `checked_sub` + `<` ile aynı etki sağlanır — iki karşılaştırma gerekiyor ama Rust optimizer'ı birleştirebilir.
