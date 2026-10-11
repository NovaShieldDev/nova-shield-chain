# UV3-01: Uniswap V3 Core Math Libraries — Kapsamlı Analiz

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v3-core/`
**Kapsanan Dosyalar:**
- `contracts/libraries/FullMath.sol` (124 satır)
- `contracts/libraries/TickMath.sol` (205 satır)
- `contracts/libraries/SqrtPriceMath.sol` (227 satır)
- `contracts/libraries/SwapMath.sol` (98 satır)
- `contracts/libraries/BitMath.sol` (94 satır)
- `contracts/libraries/LowGasSafeMath.sol` (46 satır)
- `contracts/libraries/SafeCast.sol` (28 satır)
- `contracts/libraries/FixedPoint96.sol`
- `contracts/libraries/FixedPoint128.sol`
- `contracts/libraries/UnsafeMath.sol`
- `contracts/libraries/LiquidityMath.sol`
- `contracts/libraries/TransferHelper.sol`

---

## 1. FullMath.sol — 512-Bit Phantom Overflow Aritmetiği

### 1.1 Temel Problem: Neden 512-bit?

Uniswap V3'te en sık yapılan işlem: `feeAmount × Q128 / liquidity`. Burada:
- `feeAmount` ≤ 2^128
- `Q128` = 2^128
- `liquidity` ≤ 2^128

`feeAmount × Q128` çarpımı 2^256 sınırını aşar → **phantom overflow**. Solidity'nin standart `uint256` çarpımı bu durumda yanlış sonuç verir. FullMath bunu 512-bit aritmetikle çözer.

### 1.2 `mulDiv(a, b, denominator)` — Kaynak Satır Satır Analiz

```solidity
// Satır 26-30: 512-bit çarpım hesaplama
assembly {
    let mm := mulmod(a, b, not(0))   // a×b mod (2^256-1) → wrap-around davranışı
    prod0 := mul(a, b)               // a×b mod 2^256 → düşük 256 bit
    prod1 := sub(sub(mm, prod0), lt(mm, prod0))  // yüksek 256 bit
}
```

**Matematiksel ispat:**
```
a × b = prod1 × 2^256 + prod0

mulmod(a, b, 2^256-1) hesabı:
  mm = a×b mod (2^256-1)

Eğer a×b = k×2^256 + prod0 ise:
  mm = k×(2^256 mod (2^256-1)) + prod0 mod (2^256-1)
     = k×1 + prod0 (2^256 ≡ 1 mod (2^256-1))
     = k + prod0

prod1 = mm - prod0 - borrow
      = k + prod0 - prod0 - (mm < prod0 ? 1 : 0)
      = k  (çünkü overflow olunca mm < prod0 olur, borrow 1)
```

Bu teknik: Chinese Remainder Theorem (CRT) ile 512-bit sayıyı iki 256-bit parçada saklar.

### 1.3 512-bit Bölme Algoritması (Remco Bloemen)

```
Adım 1: remainder = mulmod(a, b, denominator)
Adım 2: [prod1, prod0] -= remainder  (512-bit çıkarma)
Adım 3: twos = -denominator & denominator  (en düşük 2'nin kuvveti)
Adım 4: denominator /= twos   → tek sayı hale getir
Adım 5: prod0 /= twos         → sağa kaydır
Adım 6: prod0 |= prod1 * (2^256 / twos)  → yüksek bitleri ekle
Adım 7: Modüler ters: inv = (3 * denominator) XOR 2
Adım 8: Newton-Raphson iterasyonları (6 adım, 4→8→16→32→64→128→256 bit doğruluk)
Adım 9: result = prod0 * inv
```

**Newton-Raphson iterasyonu:** `inv_{n+1} = inv_n × (2 - d × inv_n)`

Bu, Hensel'in lifting lemması sayesinde modüler aritmetikte çalışır. Her iterasyonda doğru bit sayısı iki katına çıkar.

### 1.4 `mulDivRoundingUp` — Tavan Bölme

```solidity
function mulDivRoundingUp(uint256 a, uint256 b, uint256 denominator) {
    result = mulDiv(a, b, denominator);
    if (mulmod(a, b, denominator) > 0) {
        require(result < type(uint256).max);
        result++;
    }
}
```

**Formül:** `ceil(a × b / denominator)`

Kalıntı sıfırdan büyükse bir ekle. `mulmod` ile kalıntıyı tekrar hesaplar — ikinci kez tam 512-bit çarpım yapmaktan daha verimli.

### 1.5 Güvenlik Analizi

| Risk | Durum |
|------|-------|
| Denominator == 0 | `require(denominator > prod1)` → prod1 ≥ 0 olduğu için denominator > 0 zorunlu |
| Sonuç 2^256 overflow | `require(denominator > prod1)` → garantilenir |
| mulDivRoundingUp taşma | `require(result < type(uint256).max)` → korunur |
| Assembly'de reentrancy | Pure function, state yok → imkansız |

**Kritik Bulgu:** FullMath tamamen `pure` ve assembly tabanlı. EVM'nin `mulmod` opcode'u bu hesabı native destekler — Gas açısından optimal.

---

## 2. TickMath.sol — Logaritmik Price/Tick Dönüşümü

### 2.1 Temel Formül

```
sqrtPriceX96 = sqrt(1.0001^tick) × 2^96
```

Bu Q64.96 fixed-point formatıdır:
- 64 bit tam kısım (integer part)
- 96 bit ondalık kısım (fractional part)
- Toplam: 160 bit (uint160'a sığar)

### 2.2 Tick Sınırları

```
MIN_TICK = -887272
MAX_TICK =  887272

MIN_SQRT_RATIO = 4295128739           ≈ 2^32 × sqrt(1.0001^-887272)
MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970342  ≈ 2^160 yakını
```

**Neden ±887272?**
```
log_{1.0001}(2^128) = 128 × log_{1.0001}(2) = 128 × ln(2)/ln(1.0001)
                    = 128 × 0.6931 / 0.0001 ≈ 887272
```

### 2.3 `getSqrtRatioAtTick` — 20 Magic Constant ile Binary Decomposition

```solidity
uint256 ratio = absTick & 0x1 != 0 ? 0xfffcb933bd6fad37aa2d162d1a594001
                                    : 0x100000000000000000000000000000000;
```

**Algoritma:** Her bit pozisyonu için Q128.128 formatında precomputed sabit.

Bit 0 (absTick & 1): `C_0 = floor(sqrt(1.0001^1) × 2^128)`
```
sqrt(1.0001^1) = 1.00004999875...
× 2^128 = 340291936...
= 0xfffcb933bd6fad37aa2d162d1a594001
```

Her sabit: `C_i = floor(sqrt(1.0001^{2^i}) × 2^128)`

| Bit | İzin verilenler | Hex Sabit |
|-----|----------------|-----------|
| 0 | absTick & 0x1 | 0xfffcb933bd6fad37aa2d162d1a594001 |
| 1 | absTick & 0x2 | 0xfff97272373d413259a46990580e213a |
| 2 | absTick & 0x4 | 0xfff2e50f5f656932ef12357cf3c7fdcc |
| 3 | absTick & 0x8 | 0xffe5caca7e10e4e61c3624eaa0941cd0 |
| 4 | absTick & 0x10 | 0xffcb9843d60f6159c9db58835c926644 |
| 5 | absTick & 0x20 | 0xff973b41fa98c081472e6896dfb254c0 |
| 6 | absTick & 0x40 | 0xff2ea16466c96a3843ec78b326b52861 |
| 7 | absTick & 0x80 | 0xfe5dee046a99a2a811c461f1969c3053 |
| 8 | absTick & 0x100 | 0xfcbe86c7900a88aedcffc83b479aa3a4 |
| 9 | absTick & 0x200 | 0xf987a7253ac413176f2b074cf7815e54 |
| 10 | absTick & 0x400 | 0xf3392b0822b70005940c7a398e4b70f3 |
| 11 | absTick & 0x800 | 0xe7159475a2c29b7443b29c7fa6e889d9 |
| 12 | absTick & 0x1000 | 0xd097f3bdfd2022b8845ad8f792aa5825 |
| 13 | absTick & 0x2000 | 0xa9f746462d870fdf8a65dc1f90e061e5 |
| 14 | absTick & 0x4000 | 0x70d869a156d2a1b890bb3df62baf32f7 |
| 15 | absTick & 0x8000 | 0x31be135f97d08fd981231505542fcfa6 |
| 16 | absTick & 0x10000 | 0x9aa508b5b7a84e1c677de54f3e99bc9 |
| 17 | absTick & 0x20000 | 0x5d6af8dedb81196699c329225ee604 |
| 18 | absTick & 0x40000 | 0x2216e584f5fa1ea926041bedfe98 |
| 19 | absTick & 0x80000 | 0x48a170391f7dc42444e8fa2 |

**İşlem:** `ratio = (ratio × C_i) >> 128` (her aktif bit için)

Bu, Q128.128 formatında çarpma sonrasını Q128.128'e normalize eder.

**Negatif tick için:** `ratio = MAX_UINT256 / ratio` (tersini al)

**Q128.128 → Q64.96 dönüşüm:**
```solidity
sqrtPriceX96 = uint160((ratio >> 32) + (ratio % (1 << 32) == 0 ? 0 : 1));
```
32 bit sağa kaydırma + yukarı yuvarlama.

### 2.4 `getTickAtSqrtRatio` — MSB Tabanlı Log Hesabı

```
Adım 1: ratio = sqrtPriceX96 << 32  (Q64.96 → Q64.128)
Adım 2: MSB bul (binary search ile, 8 assembly bloğu)
Adım 3: r = ratio / 2^(msb-127)  veya  ratio × 2^(127-msb)  → Q1.127 normalize
Adım 4: 14 iterasyon ile kayan nokta log_2 hesabı
         r = (r × r) >> 127   (karesi)
         f = r >> 128          (integer kısım)
         log_2 |= f << (63 - iterasyon)
Adım 5: log_sqrt10001 = log_2 × 255738958999603826347141
Adım 6: tickLow = (log_sqrt10001 - 3402992956809132418596140100660247210) >> 128
         tickHi  = (log_sqrt10001 + 291339464771989622907027621153398088495) >> 128
Adım 7: tick = tickLow == tickHi ? tickLow
             : getSqrtRatioAtTick(tickHi) <= sqrtPriceX96 ? tickHi : tickLow
```

**255738958999603826347141** = `log_{sqrt(1.0001)}(2) × 2^128 / 2`

**Güvenlik:** Son adımda `getSqrtRatioAtTick(tickHi)` çağrısı yapılıp doğrulanır — yuvarlama hatalarını telafi eder.

### 2.5 Gas Analizi

`getSqrtRatioAtTick`: ~700 gas (20 conditional multiply, hiç branch yok, tümü if-based)
`getTickAtSqrtRatio`: ~900 gas (MSB + 14 iterasyon assembly)

---

## 3. SqrtPriceMath.sol — Fiyat Geçişi Matematigi

### 3.1 Temel Formüller

AMM invariant: `x × y = L^2`

Q64.96 formatında: `sqrtPrice = sqrt(y/x) × 2^96`

Likidite pozisyonu `[sqrtA, sqrtB]` aralığında:

**Token0 miktarı:**
```
amount0 = L × (1/sqrtA - 1/sqrtB)
        = L × (sqrtB - sqrtA) / (sqrtA × sqrtB)
```

**Token1 miktarı:**
```
amount1 = L × (sqrtB - sqrtA)
```

### 3.2 `getAmount0Delta` — Token0 Delta Hesabı

```solidity
// Satır 161-172:
uint256 numerator1 = uint256(liquidity) << FixedPoint96.RESOLUTION;  // L × 2^96
uint256 numerator2 = sqrtRatioBX96 - sqrtRatioAX96;                  // sqrtB - sqrtA

// roundUp = true:
UnsafeMath.divRoundingUp(
    FullMath.mulDivRoundingUp(numerator1, numerator2, sqrtRatioBX96),
    sqrtRatioAX96
)
// = ceil(ceil(L × 2^96 × (sqrtB - sqrtA) / sqrtB) / sqrtA)

// roundUp = false:
FullMath.mulDiv(numerator1, numerator2, sqrtRatioBX96) / sqrtRatioAX96
// = floor(L × 2^96 × (sqrtB - sqrtA) / sqrtB / sqrtA)
// = floor(L × (sqrtB - sqrtA) / (sqrtA × sqrtB) × 2^96 / 2^96)
// Dikkat: 2^96 faktörleri iptal olur çünkü sqrtA, sqrtB zaten Q64.96 formatında
```

**Yuvarlama Asimetrisi:**
- `roundUp=true` (mint): Protokol fazla token alır → güvenli (LP'ye karşı koruma)
- `roundUp=false` (burn): LP daha az token alır → protokol güvenli

### 3.3 `getAmount1Delta` — Token1 Delta Hesabı

```solidity
// roundUp = true:
FullMath.mulDivRoundingUp(liquidity, sqrtRatioBX96 - sqrtRatioAX96, FixedPoint96.Q96)
// = ceil(L × (sqrtB - sqrtA) / 2^96)

// roundUp = false:
FullMath.mulDiv(liquidity, sqrtRatioBX96 - sqrtRatioAX96, FixedPoint96.Q96)
// = floor(L × (sqrtB - sqrtA) / 2^96)
```

### 3.4 `getNextSqrtPriceFromAmount0RoundingUp`

**Eklerken (swap: token0 giriyor, fiyat düşüyor):**

Primary formula:
```
sqrtQ = L × sqrtP / (L + amount × sqrtP)
      = (L << 96) × sqrtP / ((L << 96) + amount × sqrtP)
```

Overflow varsa fallback:
```
sqrtQ = L / (L/sqrtP + amount)
      = (L << 96) / ((L << 96)/sqrtP + amount)
```

Her iki durumda da `mulDivRoundingUp` → tavan yuvarlama.

**Çıkarırken (token0 çıkıyor, fiyat yükseliyor):**
```
sqrtQ = L × sqrtP / (L - amount × sqrtP)
```
`mulDivRoundingUp` → tavan yuvarlama.

### 3.5 `getNextSqrtPriceFromAmount1RoundingDown`

**Eklerken (token1 giriyor, fiyat yükseliyor):**
```
sqrtQ = sqrtP + amount / L
      = sqrtP + (amount << 96) / liquidity    (amount küçükse direkt shift)
      = sqrtP + mulDiv(amount, Q96, liquidity) (amount büyükse FullMath)
```

**Çıkarırken (token1 çıkıyor, fiyat düşüyor):**
```
sqrtQ = sqrtP - amount / L
      = sqrtP - divRoundingUp(amount << 96, liquidity)   (küçük amount)
      = sqrtP - mulDivRoundingUp(amount, Q96, liquidity)  (büyük amount)
```

### 3.6 Yuvarlama Yönü Mantığı

| Operasyon | Token0 | Token1 |
|-----------|--------|--------|
| Mint (add) | RoundUp (protokol daha fazla alır) | RoundUp |
| Burn (remove) | RoundDown (LP daha az alır) | RoundDown |
| Swap exactIn token0 | sqrtNext RoundUp (daha az input gerekir) | - |
| Swap exactIn token1 | - | sqrtNext RoundDown (daha az input) |
| Swap exactOut token0 | sqrtNext RoundDown (daha az output) | - |
| Swap exactOut token1 | - | sqrtNext RoundUp |

---

## 4. SwapMath.sol — Tek Tick Adımı Hesabı

### 4.1 `computeSwapStep` Parametreleri

```
sqrtRatioCurrentX96: Mevcut fiyat
sqrtRatioTargetX96:  Hedef fiyat (tick sınırı veya limit fiyatı)
liquidity:           Aktif likidite
amountRemaining:     > 0: exactIn (pozitif), < 0: exactOut (negatif)
feePips:             Fee tier (100, 500, 3000, 10000 bps)
```

### 4.2 ExactIn Durumu (amountRemaining ≥ 0)

```
Adım 1: amountRemainingLessFee = amountRemaining × (1e6 - feePips) / 1e6
         (fee çıkarılmış input miktarı)

Adım 2: amountIn = getAmount{0|1}Delta(target, current, liquidity, roundUp=true)
         (hedef fiyata ulaşmak için gereken input)

Adım 3a: amountRemainingLessFee >= amountIn → sqrtNext = target (tick geçildi)
Adım 3b: aksi → sqrtNext = getNextSqrtPriceFromInput(current, liquidity, amountRemainingLessFee, dir)

Adım 4: feeAmount hesabı:
         - Tick geçilmediyse: feeAmount = amountRemaining - amountIn
           (kalan input tamamı fee)
         - Tick geçildiyse: feeAmount = mulDivRoundingUp(amountIn, feePips, 1e6 - feePips)
```

### 4.3 ExactOut Durumu (amountRemaining < 0)

```
Adım 1: amountOut = getAmount{1|0}Delta(target, current, liquidity, roundUp=false)
         (hedef fiyata ulaşmak için çıkacak output)

Adım 2a: -amountRemaining >= amountOut → sqrtNext = target (tick geçildi)
Adım 2b: aksi → sqrtNext = getNextSqrtPriceFromOutput(...)

Adım 3: feeAmount = mulDivRoundingUp(amountIn, feePips, 1e6 - feePips)
```

### 4.4 Fee Hesabı Detayı

ExactIn tick geçildiğinde:
```
feeAmount = ceil(amountIn × feePips / (1e6 - feePips))
```

Bu formül: `feeAmount / (amountIn + feeAmount) = feePips / 1e6` ilişkisini korur.

Örnek (0.3% fee, feePips=3000):
```
feeAmount = ceil(amountIn × 3000 / 997000)
           = ceil(amountIn × 0.003009...)
```

ExactIn tick geçilmediğinde:
```
feeAmount = amountRemaining - amountIn
```
Kalan her şey fee olur (zaten fee çıkarılmış amountRemainingLessFee ile çalışıldı).

### 4.5 Fee Tier Tablosu

| feePips | Fee % | Pool Türü |
|---------|-------|-----------|
| 100 | 0.01% | Stablecoin çiftleri |
| 500 | 0.05% | Yakın değer çiftler |
| 3000 | 0.30% | Standard (ETH/USDC) |
| 10000 | 1.00% | Exotic çiftler |

---

## 5. BitMath.sol — MSB/LSB Hesabı

### 5.1 `mostSignificantBit(x)` — Binary Search

```
Başlangıç: r = 0
if x ≥ 2^128: x >>= 128; r += 128   → üst 128 bit bölgesindeiz
if x ≥ 2^64:  x >>= 64;  r += 64    → üst 64 bit
if x ≥ 2^32:  x >>= 32;  r += 32
if x ≥ 2^16:  x >>= 16;  r += 16
if x ≥ 2^8:   x >>= 8;   r += 8
if x ≥ 2^4:   x >>= 4;   r += 4
if x ≥ 2^2:   x >>= 2;   r += 2
if x ≥ 2:     r += 1
```

**Karmaşıklık:** O(log log x) = O(log 256) = O(8) — sabit!

### 5.2 `leastSignificantBit(x)` — İnverse Pattern

```
Başlangıç: r = 255
if x & uint128.MAX > 0: r -= 128  (LSB alt 128 bitte)
else: x >>= 128                    (LSB üst 128 bitte)
... aynı pattern 64, 32, 16, 8, 4, 2, 1 için
```

**TickBitmap'te kullanımı:**
- `mostSignificantBit` → soldaki (düşük tick yönünde) ilk set biti bul
- `leastSignificantBit` → sağdaki (yüksek tick yönünde) ilk set biti bul

### 5.3 Karşılaştırma: V4 De Bruijn BitMath ile

V3 BitMath binary search kullanır: 8 adım, deterministik gas.
V4 BitMath De Bruijn sequence kullanır: 1 adım, hash-based — daha hızlı ama daha az okunabilir.

```rust
// Nova Shield'de önerilecek implementasyon (Rust native):
fn most_significant_bit(x: u256) -> u8 {
    x.leading_zeros() as u8  // Rust'ta u64 için builtin LZCNT/BSR
    // u256 için: üst u128'e bak, sıfırsa alt u128'e geç
}
```

---

## 6. LowGasSafeMath.sol — Minimal Overflow Koruması

### 6.1 uint256 Operasyonlar

```solidity
add(x, y): require((z = x+y) >= x)       // overflow: z küçülür
sub(x, y): require((z = x-y) <= x)       // underflow: z büyür
mul(x, y): require(x == 0 || (z = x*y) / x == y)  // overflow: bölme tutarsız
```

**Gas optimizasyonu:** SafeMath (OpenZeppelin) yerine custom — daha az stack frame.

### 6.2 int256 Operasyonlar

```solidity
add(x, y): require((z = x+y) >= x == (y >= 0))
// y ≥ 0 ise: z ≥ x olmalı (pozitif toplama)
// y < 0 ise: z < x olmalı (negatif toplama)
// Bu iki şartı tek satırda kontrol eder

sub(x, y): require((z = x-y) <= x == (y >= 0))
// y ≥ 0 ise: z ≤ x olmalı (pozitif çıkarma azaltır)
// y < 0 ise: z > x olmalı (negatif çıkarma artırır)
```

**Neden "Low Gas"?** Solidity 0.8+ otomatik overflow revert ekler ama bu kütüphane 0.7 hedefli. Ayrıca require mesajı yok → revert data yok → ~200 gas tasarruf.

---

## 7. SafeCast.sol — Tip Güvenli Dönüşümler

```solidity
toUint160(y): require((z = uint160(y)) == y)
// Kesme olmadığını doğrular

toInt128(y):  require((z = int128(y)) == y)
// Sign extension doğru mu?

toInt256(y):  require(y < 2**255)
// uint256'nın üst bitini korur
```

**Neden gerekli?** Solidity 0.7'de `uint256 → uint160` dönüşümü sessizce keser. SafeCast bu hatayı yakalar.

---

## 8. FixedPoint96 ve FixedPoint128

```solidity
// FixedPoint96.sol
uint8 internal constant RESOLUTION = 96;
uint256 internal constant Q96 = 0x1000000000000000000000000;  // 2^96

// FixedPoint128.sol
uint256 internal constant Q128 = 0x100000000000000000000000000000000;  // 2^128
```

**Q64.96 format:** Fiyatlar için. 64 bit tam kısım, 96 bit kesirli kısım.
**Q128.128 format:** Fee birikimi için. `feeGrowthGlobal × 2^128`.

---

## 9. UnsafeMath.sol

```solidity
function divRoundingUp(uint256 x, uint256 y) internal pure returns (uint256 z) {
    assembly {
        z := add(div(x, y), gt(mod(x, y), 0))
    }
}
```

"Unsafe" çünkü y=0 kontrolü yok (caller'a bırakılmış). `SqrtPriceMath` büyük amount durumunda bu fallback'i kullanır.

---

## 10. LiquidityMath.sol

```solidity
function addDelta(uint128 x, int128 y) internal pure returns (uint128 z) {
    if (y < 0) {
        require((z = x - uint128(-y)) < x, 'LS');
    } else {
        require((z = x + uint128(y)) >= x, 'LA');
    }
}
```

`int128` delta'yı `uint128` likiditeye güvenli uygular. Tick cross sırasında `liquidityNet` eklenir/çıkarılır.

---

## 11. Nova Shield Chain — Rust Implementasyon Notları

### 11.1 FullMath → Rust u256

```rust
// Crate: uint veya primitive-types
use primitive_types::U256;

// 512-bit çarpım için:
fn mul_div(a: U256, b: U256, denominator: U256) -> U256 {
    // Rust'ta U512 kullanılabilir:
    use primitive_types::U512;
    let prod = U512::from(a) * U512::from(b);
    let (result, _) = prod.div_mod(U512::from(denominator));
    result.try_into().expect("overflow")
}

// Veya daha verimli: uint256 + custom 512-bit ops
// Nova Shield'de: crates/dex/src/math/full_math.rs
```

### 11.2 TickMath → Rust

```rust
// Magic sabitler compile-time const olarak:
const TICK_MAGIC: [u128; 20] = [
    0xfffcb933bd6fad37aa2d162d1a594001,
    0xfff97272373d413259a46990580e213a,
    // ... tüm 20 sabit
];

fn get_sqrt_ratio_at_tick(tick: i32) -> u160 {
    let abs_tick = tick.unsigned_abs() as u32;
    assert!(abs_tick <= MAX_TICK as u32);

    let mut ratio: u256 = if abs_tick & 1 != 0 {
        TICK_MAGIC[0].into()
    } else {
        1u128 << 128
    }.into();

    for i in 1..20 {
        if abs_tick & (1 << i) != 0 {
            ratio = (ratio * U256::from(TICK_MAGIC[i])) >> 128;
        }
    }

    if tick > 0 {
        ratio = U256::MAX / ratio;
    }

    // Q128.128 → Q64.96
    let shifted = ratio >> 32;
    if ratio % (U256::one() << 32) != U256::zero() {
        (shifted + 1).try_into().unwrap()
    } else {
        shifted.try_into().unwrap()
    }
}
```

### 11.3 SwapMath → Nova Shield DEX Engine

Nova Shield'in native DEX'inde `computeSwapStep` doğrudan Rust'ta:

```rust
pub struct SwapStep {
    pub sqrt_ratio_next: u160,
    pub amount_in: u256,
    pub amount_out: u256,
    pub fee_amount: u256,
}

pub fn compute_swap_step(
    sqrt_ratio_current: u160,
    sqrt_ratio_target: u160,
    liquidity: u128,
    amount_remaining: i256,
    fee_pips: u32,
) -> SwapStep {
    let zero_for_one = sqrt_ratio_current >= sqrt_ratio_target;
    let exact_in = amount_remaining >= I256::ZERO;
    // ... implementasyon
}
```

### 11.4 Kritik Güvenlik Noktaları

1. **FullMath:** Newton-Raphson seed `(3 × denominator) XOR 2` yalnızca tek sayılar için çalışır. Adım 3'te `twos` ile bölme bu garantiyi sağlar.

2. **TickMath:** `getSqrtRatioAtTick(MAX_TICK)` = MAX_SQRT_RATIO - 1. `getTickAtSqrtRatio` son adımda `< MAX_SQRT_RATIO` check'i yapar (eşit değil).

3. **SqrtPriceMath:** Token0 her zaman RoundUp (mint/swap), Token1 yönüne göre değişir. Bu asimetri arbitraj korumasını sağlar — tek yönlü yuvarlama protokolü sürekli +epsilon kazanır.

4. **SwapMath:** `amountRemainingLessFee` overflow olmaz çünkü `mulDiv(remaining, 1e6-feePips, 1e6)` → her zaman `< remaining`.

### 11.5 Performans Hedefleri (Nova Shield)

| Operasyon | EVM Gas | Rust Hedefi |
|-----------|---------|-------------|
| mulDiv 512-bit | ~150 gas | <100ns |
| getSqrtRatioAtTick | ~700 gas | <200ns |
| getTickAtSqrtRatio | ~900 gas | <300ns |
| computeSwapStep | ~2000 gas | <500ns |

---

## 12. Test Coverage Analizi

Test dosyaları (`contracts/test/`):
- `FullMathTest.sol` — boundary cases, overflow boundary
- `TickMathTest.sol` — min/max tick, roundtrip invariant
- `SqrtPriceMathTest.sol` — rounding direction tests
- `SwapMathTest.sol` — fee computation, exact boundaries
- `BitMathTest.sol` — all bits 0-255

**Kritik invariantlar (Echidna fuzzing):**
- `mulDiv(a, b, d) × d ≤ a × b` (floor garantisi)
- `getSqrtRatioAtTick(getTickAtSqrtRatio(p)) ≤ p` (roundtrip)
- `computeSwapStep` → `amountIn + feeAmount ≤ |amountRemaining|` (exactIn)

---

## Özet: 3 Kritik Bulgu

1. **512-bit Phantom Overflow (FullMath):** CRT + Newton-Raphson kombinasyonu ile overflow olmaksızın `a×b/c` hesaplanır. Nova Shield'de `primitive_types::U512` veya custom assembly ile implementasyon gerekli.

2. **20 Magic Constant (TickMath):** Her sabit `sqrt(1.0001^{2^i}) × 2^128` formülünden gelir. Binary decomposition ile O(20) sabit çarpmada log-to-price dönüşümü. Nova Shield'de compile-time const array olarak saklanacak.

3. **Yuvarlama Asimetrisi (SqrtPriceMath):** Token0 her zaman RoundUp (protokol lehine), Token1 duruma göre. Bu asimetri birikimli olarak protokol güvenliğini sağlar — Nova Shield DEX'inde aynı yaklaşım uygulanacak.
