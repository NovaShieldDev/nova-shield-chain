# UV3-02: UniswapV3Pool.sol — Pool Çekirdeği Kapsamlı Analiz

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v3-core/`
**Kapsanan Dosyalar:**
- `contracts/UniswapV3Pool.sol` (846 satır — tam analiz)
- `contracts/UniswapV3Factory.sol`
- `contracts/NoDelegateCall.sol`
- `contracts/interfaces/IUniswapV3Pool.sol`
- `contracts/interfaces/IUniswapV3PoolDeployer.sol`
- `contracts/interfaces/callback/IUniswapV3MintCallback.sol`
- `contracts/interfaces/callback/IUniswapV3SwapCallback.sol`
- `contracts/interfaces/callback/IUniswapV3FlashCallback.sol`

---

## 1. Slot0 — 248-bit Sıkıştırılmış Pool Durumu

```solidity
struct Slot0 {
    uint160 sqrtPriceX96;           // Bit 0-159:   Mevcut sqrt fiyatı (Q64.96)
    int24   tick;                    // Bit 160-183: Mevcut tick
    uint16  observationIndex;        // Bit 184-199: Son oracle yazım indeksi
    uint16  observationCardinality;  // Bit 200-215: Dolu oracle slot sayısı
    uint16  observationCardinalityNext; // Bit 216-231: Hedef oracle kapasitesi
    uint8   feeProtocol;             // Bit 232-239: Protocol fee (4 bit token0 + 4 bit token1)
    bool    unlocked;                // Bit 240:     Reentrancy kilidi
}
```

**Toplam bit kullanımı:** 160 + 24 + 16 + 16 + 16 + 8 + 1 = **241 bit** → tek bir `bytes32` (256 bit) storage slotuna sığar.

**Bu kritik:** Tek bir `SLOAD` ile tüm pool durumu okunur. Her swap başında `Slot0 memory slot0Start = slot0;` ile sadece bir SLOAD → ~2100 gas yerine tek seferlik ödeme.

### 1.1 feeProtocol Paketi

```
feeProtocol = feeProtocol0 | (feeProtocol1 << 4)
```

Token0 için: `feeProtocol0 = slot0.feeProtocol % 16` (alt 4 bit)
Token1 için: `feeProtocol1 = slot0.feeProtocol >> 4`  (üst 4 bit)

**Geçerli değerler:** 0 (devre dışı) veya 4-10 (1/4 ile 1/10 arası)

Protokol fee oranı: `delta = feeAmount / feeProtocol` → **fee'nin 1/feeProtocol'u**

Örnek: `feeProtocol=4` → protocol fee = swap fee'nin %25'i

---

## 2. Immutable Değişkenler ve Constructor

```solidity
address public immutable factory;   // Factory adresi
address public immutable token0;    // Token0 (her zaman < token1)
address public immutable token1;    // Token1
uint24  public immutable fee;       // Fee tier (100/500/3000/10000)
int24   public immutable tickSpacing; // Tick aralığı
uint128 public immutable maxLiquidityPerTick; // Tick başına max likidite
```

### 2.1 Constructor CREATE2 Deseni

```solidity
constructor() {
    (factory, token0, token1, fee, _tickSpacing) = IUniswapV3PoolDeployer(msg.sender).parameters();
    tickSpacing = _tickSpacing;
    maxLiquidityPerTick = Tick.tickSpacingToMaxLiquidityPerTick(_tickSpacing);
}
```

**Factory şöyle deploy eder:**
1. Factory, parametreleri geçici depolama alanına yazar
2. `new UniswapV3Pool{salt: salt}()` ile Pool deploy eder
3. Pool constructor'da Factory'ye geri döner parametreler için
4. Parametreler silindikten sonra Pool adresi döner

**CREATE2 tuz (salt):**
```
salt = keccak256(abi.encode(token0, token1, fee))
```

Pool adresi deterministiktir → herhangi biri hesaplayabilir.

### 2.2 maxLiquidityPerTick Formülü

```
maxLiquidityPerTick = uint128.MAX / (MAX_TICK - MIN_TICK + 1) / tickSpacing
                    = (2^128 - 1) / (887272 - (-887272) + 1) / tickSpacing
```

| Fee | tickSpacing | maxLiquidityPerTick |
|-----|-------------|---------------------|
| 0.01% | 1 | ~1.461 × 10^35 |
| 0.05% | 10 | ~1.461 × 10^36 |
| 0.30% | 60 | ~8.767 × 10^36 |
| 1.00% | 200 | ~2.922 × 10^37 |

---

## 3. Lock Modifier — Reentrancy Koruması

```solidity
modifier lock() {
    require(slot0.unlocked, 'LOK');
    slot0.unlocked = false;
    _;
    slot0.unlocked = true;
}
```

**Kritik Gözlem:** `swap()` fonksiyonu `lock` modifier kullanmaz — bunun yerine manuel kilit yönetimi yapar:

```solidity
// Satır 615: Manuel kilit al
slot0.unlocked = false;

// ... swap işlemleri ...

// Satır 787: Manuel kilit bırak
slot0.unlocked = true;
```

**Neden?** `swap()` içinde callback çağrısından ÖNCE output token gönderilir. Bu optimistik transfer pattern'ı Checks-Effects-Interactions sırasını bozar. Manuel kilit bu güvensizliği kontrol altına alır.

---

## 4. `initialize()` — Pool Başlatma

```solidity
function initialize(uint160 sqrtPriceX96) external {
    require(slot0.sqrtPriceX96 == 0, 'AI');  // Already Initialized kontrolü

    int24 tick = TickMath.getTickAtSqrtRatio(sqrtPriceX96);

    (uint16 cardinality, uint16 cardinalityNext) = observations.initialize(_blockTimestamp());

    slot0 = Slot0({
        sqrtPriceX96: sqrtPriceX96,
        tick: tick,
        observationIndex: 0,
        observationCardinality: cardinality,   // = 1
        observationCardinalityNext: cardinalityNext, // = 1
        feeProtocol: 0,
        unlocked: true
    });
}
```

**Not:** `lock` modifier yok çünkü başlangıçta `unlocked` değeri false (sıfır). Modifier başlangıçta bloklanırdı.

---

## 5. `mint()` — Likidite Ekleme

```solidity
function mint(
    address recipient,
    int24 tickLower,
    int24 tickUpper,
    uint128 amount,      // Eklenecek likidite miktarı
    bytes calldata data  // Callback data
) external lock returns (uint256 amount0, uint256 amount1)
```

### 5.1 Akış

```
1. require(amount > 0)
2. _modifyPosition(owner, tickLower, tickUpper, +liquidityDelta)
   → amount0, amount1 hesaplanır
3. balance0Before, balance1Before kaydedilir
4. IUniswapV3MintCallback(msg.sender).uniswapV3MintCallback(amount0, amount1, data)
   → CALLER token göndermek zorunda!
5. require(balance0Before + amount0 <= balance0())  // Token kontrolü
6. require(balance1Before + amount1 <= balance1())
```

**Güvenlik Modeli:** Optimistik ödeme bekleme. Çağıran `uniswapV3MintCallback` içinde token0 ve token1'i Pool'a göndermelidir. Göndermediyse `require` fail eder → revert.

### 5.2 `_modifyPosition` — Pozisyon Güncelleme

**Tick'in konumuna göre token ihtiyacı:**

```
tick < tickLower:  Sadece token0 gerekli (fiyat range altında)
  amount0 = getAmount0Delta(sqrtAt(tickLower), sqrtAt(tickUpper), liquidityDelta)

tickLower ≤ tick < tickUpper:  Her iki token gerekli (fiyat range içinde)
  amount0 = getAmount0Delta(sqrtCurrent, sqrtAt(tickUpper), liquidityDelta)
  amount1 = getAmount1Delta(sqrtAt(tickLower), sqrtCurrent, liquidityDelta)
  liquidity += liquidityDelta  // Aktif likiditeyi güncelle

tick ≥ tickUpper:  Sadece token1 gerekli (fiyat range üstünde)
  amount1 = getAmount1Delta(sqrtAt(tickLower), sqrtAt(tickUpper), liquidityDelta)
```

### 5.3 `_updatePosition` — Tick ve Fee Güncellemesi

```
1. ticks.update(tickLower, ...) → flippedLower (0→non-zero veya non-zero→0 geçişi)
2. ticks.update(tickUpper, ...) → flippedUpper
3. flippedLower → tickBitmap.flipTick(tickLower, tickSpacing)
4. flippedUpper → tickBitmap.flipTick(tickUpper, tickSpacing)
5. getFeeGrowthInside(tickLower, tickUpper, tick, feeGrowth0, feeGrowth1)
6. position.update(liquidityDelta, feeGrowthInside0, feeGrowthInside1)
7. liquidityDelta < 0 && flipped → ticks.clear() (boşaltılan tick temizle)
```

---

## 6. `burn()` — Likidite Çıkarma

```solidity
function burn(int24 tickLower, int24 tickUpper, uint128 amount)
    external lock returns (uint256 amount0, uint256 amount1)
```

```
1. _modifyPosition(owner, tickLower, tickUpper, -liquidityDelta)
   → amount0Int, amount1Int hesaplanır (negatif)
2. amount0 = uint256(-amount0Int)
3. amount1 = uint256(-amount1Int)
4. position.tokensOwed0 += amount0
5. position.tokensOwed1 += amount1
```

**Kritik:** `burn()` token GÖNDERMEZ. Sadece `tokensOwed` artırır. Token almak için ayrıca `collect()` çağrılmalıdır. Bu 2 adımlı pattern MEV koruması sağlar.

---

## 7. `collect()` — Hakediş Toplama

```solidity
function collect(
    address recipient,
    int24 tickLower, int24 tickUpper,
    uint128 amount0Requested,
    uint128 amount1Requested
) external lock returns (uint128 amount0, uint128 amount1)
```

```
amount0 = min(amount0Requested, position.tokensOwed0)
amount1 = min(amount1Requested, position.tokensOwed1)

position.tokensOwed0 -= amount0
position.tokensOwed1 -= amount1

safeTransfer(token0, recipient, amount0)
safeTransfer(token1, recipient, amount1)
```

**Dikkat:** `checkTicks()` çağrılmaz. Geçersiz tick range'lerde hiç tokensOwed olmaz → güvenli.

---

## 8. `swap()` — Tam Analiz (Satır 596-788)

Bu, V3'ün en karmaşık fonksiyonu. Satır satır analiz:

### 8.1 Başlangıç Kontrolleri (Satır 603-625)

```solidity
require(amountSpecified != 0, 'AS');

// Slot0'ı tek SLOAD ile al
Slot0 memory slot0Start = slot0;

require(slot0Start.unlocked, 'LOK');
require(
    zeroForOne
        ? sqrtPriceLimitX96 < slot0Start.sqrtPriceX96 && sqrtPriceLimitX96 > MIN_SQRT_RATIO
        : sqrtPriceLimitX96 > slot0Start.sqrtPriceX96 && sqrtPriceLimitX96 < MAX_SQRT_RATIO,
    'SPL'
);

slot0.unlocked = false;  // Manuel lock
```

**sqrtPriceLimitX96 mantığı:**
- zeroForOne (token0→token1): Fiyat düşer, limit daha düşük olmalı
- oneForZero (token1→token0): Fiyat yükselir, limit daha yüksek olmalı

### 8.2 SwapCache ve SwapState Yapıları

```
SwapCache: Swap boyunca sabit kalan değerler
  - liquidityStart: Başlangıç likiditesi
  - blockTimestamp: Zaman damgası
  - feeProtocol: Protocol fee oranı

SwapState: Swap boyunca değişen değerler (memory'de biriktirilir)
  - amountSpecifiedRemaining: Kalan işlem miktarı
  - amountCalculated: Hesaplanan output
  - sqrtPriceX96: Mevcut fiyat
  - tick: Mevcut tick
  - feeGrowthGlobalX128: Fee birikimi (memory'de, sonunda storage'a yazılır)
  - protocolFee: Biriken protocol fee
  - liquidity: Aktif likidite
```

### 8.3 Ana Swap Döngüsü (Satır 641-730)

```
while (amountSpecifiedRemaining != 0 && sqrtPriceX96 != sqrtPriceLimitX96):

  Adım 1: Bir sonraki başlatılmış tick'i bul
    (tickNext, initialized) = tickBitmap.nextInitializedTickWithinOneWord(tick, tickSpacing, zeroForOne)
    tickNext = clamp(tickNext, MIN_TICK, MAX_TICK)

  Adım 2: Tick fiyatını hesapla
    sqrtPriceNextX96 = getSqrtRatioAtTick(tickNext)

  Adım 3: Target fiyatı belirle (tick fiyatı mı limit fiyatı mı?)
    target = zeroForOne
      ? max(sqrtPriceLimitX96, sqrtPriceNextX96)   // En büyük "alt sınır"
      : min(sqrtPriceLimitX96, sqrtPriceNextX96)    // En küçük "üst sınır"

  Adım 4: Bu tick adımında hesapla
    (newSqrtPrice, amountIn, amountOut, feeAmount) = computeSwapStep(
        sqrtPriceX96, target, liquidity, amountSpecifiedRemaining, fee)

  Adım 5: State güncelle
    amountSpecifiedRemaining -= (amountIn + feeAmount)  [exactIn]
    amountCalculated -= amountOut                        [exactIn]

  Adım 6: Protocol fee (varsa)
    delta = feeAmount / feeProtocol
    feeAmount -= delta
    protocolFee += delta

  Adım 7: Global fee güncelle
    feeGrowthGlobalX128 += feeAmount × Q128 / liquidity

  Adım 8: Tick geçişi (tick crossed)
    if sqrtPriceX96 == sqrtPriceNextX96:
      if initialized:
        if !computedLatestObservation:
          tickCumulative, secondsPerLiq = observeSingle(...)
          computedLatestObservation = true

        liquidityNet = ticks.cross(tickNext, feeGrowth0, feeGrowth1, secondsPerLiq, tickCumul, time)
        if zeroForOne: liquidityNet = -liquidityNet
        liquidity = addDelta(liquidity, liquidityNet)

      tick = zeroForOne ? tickNext - 1 : tickNext
    else if sqrtPriceX96 != sqrtPriceStartX96:
      tick = getTickAtSqrtRatio(sqrtPriceX96)
```

### 8.4 Swap Sonrası Storage Yazma (Satır 733-765)

```
if tick changed:
  (observationIndex, observationCardinality) = observations.write(...)
  slot0 = {sqrtPriceX96, tick, observationIndex, observationCardinality}
else:
  slot0.sqrtPriceX96 = state.sqrtPriceX96   (sadece fiyat güncellemesi)

if liquidity changed:
  liquidity = state.liquidity

feeGrowthGlobal{0|1}X128 = state.feeGrowthGlobalX128
protocolFees.{token0|token1} += state.protocolFee
```

**Gas optimizasyonu:** Değişmeyen değerler storage'a yazılmaz.

### 8.5 Optimistik Transfer Pattern (Satır 772-784)

```
if zeroForOne:
  // Önce output gönder
  if amount1 < 0: safeTransfer(token1, recipient, -amount1)

  // Sonra callback → caller token0 gönderir
  balance0Before = balance0()
  IUniswapV3SwapCallback(msg.sender).uniswapV3SwapCallback(amount0, amount1, data)

  // Ödeme kontrolü
  require(balance0Before + amount0 <= balance0(), 'IIA')
```

**Güvenlik:** Callback çağrısından önce lock aktif. Reentrant swap girişimi `slot0.unlocked = false` sebebiyle fail olur.

### 8.6 feeGrowthGlobal Formülü

```
feeGrowthGlobalX128 += feeAmount × Q128 / liquidity
```

Bu formülün tam analizi UV3-04'te (Oracle) yapıldı. Özet:
- Her swap adımında fee, aktif likiditeye orantılı dağıtılır
- Q128.128 formatında biriktirilir
- Pozisyon fee claim'i: `(feeGrowthInside_current - feeGrowthInside_entry) × liquidity / Q128`

---

## 9. `flash()` — Flash Loan

```solidity
function flash(address recipient, uint256 amount0, uint256 amount1, bytes calldata data)
    external lock noDelegateCall
```

```
fee0 = ceil(amount0 × fee / 1e6)
fee1 = ceil(amount1 × fee / 1e6)

balance0Before, balance1Before = kaydedilir

// Önce gönder
safeTransfer(token0, recipient, amount0)
safeTransfer(token1, recipient, amount1)

// Callback
IUniswapV3FlashCallback(msg.sender).uniswapV3FlashCallback(fee0, fee1, data)

// Geri ödeme kontrolü
require(balance0Before + fee0 <= balance0After, 'F0')
require(balance1Before + fee1 <= balance1After, 'F1')

// Fee dağıt (LP'ye ve protocol'e)
paid0 = balance0After - balance0Before
fees0 = feeProtocol0 == 0 ? 0 : paid0 / feeProtocol0
protocolFees.token0 += fees0
feeGrowthGlobal0X128 += (paid0 - fees0) × Q128 / liquidity
```

**Flash loan fee:** Swap fee'si ile aynı oranı kullanır (100/500/3000/10000 bps).

---

## 10. `setFeeProtocol()` — Protocol Fee Yönetimi

```solidity
function setFeeProtocol(uint8 feeProtocol0, uint8 feeProtocol1)
    external lock onlyFactoryOwner
```

```
require((feeProtocol0 == 0 || (feeProtocol0 >= 4 && feeProtocol0 <= 10)) &&
        (feeProtocol1 == 0 || (feeProtocol1 >= 4 && feeProtocol1 <= 10)))

slot0.feeProtocol = feeProtocol0 + (feeProtocol1 << 4)
```

**Geçerli değerler:** 4, 5, 6, 7, 8, 9, 10 (veya 0=kapalı)
- 4 → Swap fee'nin %25'i
- 10 → Swap fee'nin %10'u

---

## 11. `snapshotCumulativesInside()` — Range Bazlı TWAP

```
Tick range [tickLower, tickUpper] için TWAP alma:

if currentTick < tickLower:
  // Fiyat range'in altında
  return (lower.outside - upper.outside, ...)

if currentTick < tickUpper:
  // Fiyat range'in içinde
  global = observeSingle(time, 0, ...)
  return (global - lower.outside - upper.outside, ...)

else:
  // Fiyat range'in üstünde
  return (upper.outside - lower.outside, ...)
```

Bu "inside/outside" terimleri Tick.sol'daki `feeGrowthOutside` mekanizmasından gelir — ayrıntılar UV3-03'te.

---

## 12. NoDelegateCall Koruması

```solidity
contract NoDelegateCall {
    address private immutable original;

    constructor() {
        original = address(this);
    }

    modifier noDelegateCall() {
        require(address(this) == original);
        _;
    }
}
```

`delegatecall` ile çağrılırsa `address(this)` farklı olur → revert.
Hangi fonksiyonlarda kullanılır: `observe()`, `snapshotCumulativesInside()`, `_modifyPosition()`, `flash()`

**Neden?** delegatecall ile başka bir contract'ın storage'ına pool state yazılmasını engeller.

---

## 13. UniswapV3Factory.sol — Özet

```solidity
function createPool(address tokenA, address tokenB, uint24 fee)
    external returns (address pool)
```

```
(token0, token1) = tokenA < tokenB ? (tokenA, tokenB) : (tokenB, tokenA)

require(token0 != address(0))
require(feeAmountTickSpacing[fee] != 0)  // Geçerli fee tier mi?
require(getPool[token0][token1][fee] == address(0))  // Zaten var mı?

// Geçici parameters yaz
parameters = Parameters({factory, token0, token1, fee, tickSpacing})

// CREATE2 ile deploy
pool = address(new UniswapV3Pool{salt: keccak256(abi.encode(token0, token1, fee))}())

delete parameters

getPool[token0][token1][fee] = pool
getPool[token1][token0][fee] = pool  // Her iki yönde kayıt
```

**Varsayılan fee tier → tickSpacing eşlemesi:**
| Fee | tickSpacing |
|-----|-------------|
| 500 | 10 |
| 3000 | 60 |
| 10000 | 200 |

0.01% (100 bps, spacing=1) sonradan `enableFeeAmount(100, 1)` ile eklendi.

---

## 14. Güvenlik Analizi

### 14.1 Reentrancy

| Saldırı Vektörü | Koruma |
|-----------------|--------|
| mint() callback'ten reentrant swap | lock modifier |
| swap() callback'ten reentrant swap | Manuel lock (slot0.unlocked) |
| flash() callback'ten reentrant mint | lock modifier |
| flash() callback'ten reentrant flash | lock modifier |
| delegatecall ile state manipülasyonu | noDelegateCall modifier |

### 14.2 Sandwich Attack Koruması

`sqrtPriceLimitX96` parametresi zorunlu. Sıfır verilemez:
```
zeroForOne ? limit > MIN_SQRT_RATIO : limit < MAX_SQRT_RATIO
```
Kullanıcı fiyat limiti belirler → sandwich tamamen önlenemez ama slippage kontrolü mümkün.

### 14.3 Tick Taşma Koruması

Döngüde:
```solidity
if (step.tickNext < TickMath.MIN_TICK) step.tickNext = TickMath.MIN_TICK;
else if (step.tickNext > TickMath.MAX_TICK) step.tickNext = TickMath.MAX_TICK;
```

TickBitmap sınırları MAX_TICK'i aşabilir (bitmap bit hesapları nedeniyle), bu clamp onu düzeltir.

### 14.4 Integer Overflow

- `feeGrowthGlobalX128` overflow'a izin verilir (kasıtlı): `// overflow is acceptable`
- Protocol fee `type(uint128).max`'a ulaşmadan çekilmeli
- `tokensOwed` uint128 — taşarsa `collect()` sonsuza gider → protokol tasarımı gereği uyarı

---

## 15. Nova Shield Chain — Rust Implementasyon Notları

### 15.1 Pool State Yapısı

```rust
// Nova Shield'de pool state — Rust struct alignment önemli
#[derive(Debug, Clone)]
pub struct PoolState {
    // Slot0 eşdeğeri
    pub sqrt_price_x96: U160,
    pub tick: i32,
    pub observation_index: u16,
    pub observation_cardinality: u16,
    pub observation_cardinality_next: u16,
    pub fee_protocol: u8,  // 4 bit token0 + 4 bit token1
    pub unlocked: bool,

    // Ayrı storage
    pub fee_growth_global_0_x128: U256,
    pub fee_growth_global_1_x128: U256,
    pub protocol_fees: (u128, u128),  // (token0, token1)
    pub liquidity: u128,
}
```

### 15.2 Nova Shield DEX Mimarisi Farkı

Nova Shield'de pool EVM smart contract değil, native L1 modülü:

```rust
// crates/dex/src/pool.rs
pub struct Pool {
    pub id: PoolId,
    pub state: PoolState,
    pub ticks: BTreeMap<i32, TickInfo>,
    pub tick_bitmap: HashMap<i16, u256>,
    pub positions: HashMap<PositionKey, PositionInfo>,
    pub observations: CircularBuffer<Observation>,
}

impl Pool {
    pub fn swap(
        &mut self,
        zero_for_one: bool,
        amount_specified: i256,
        sqrt_price_limit_x96: U160,
    ) -> Result<SwapResult, PoolError> {
        // Uniswap V3 algoritmasını birebir Rust'ta uygula
        // Callback yok — native transfer
        // reentrancy yok — Rust ownership garantisi
    }
}
```

### 15.3 Kritik Farklar: Smart Contract vs Native

| Özellik | Uniswap V3 (Solidity) | Nova Shield (Rust) |
|---------|----------------------|-------------------|
| Reentrancy | lock modifier gerekli | Rust ownership → imkansız |
| Callback | uniswapV3SwapCallback | Direkt asset transfer |
| Storage | EVM SLOAD/SSTORE | RocksDB / Memory |
| Gas | Her opcode ücretli | İşlem bazlı fee |
| Overflow | Manuel SafeMath | Rust checked_* |
| Parallelism | Tek thread EVM | Tokio async |

### 15.4 Swap Döngüsü Rust'ta

```rust
loop {
    let (tick_next, initialized) = tick_bitmap.next_initialized_tick_within_one_word(
        state.tick, tick_spacing, zero_for_one
    );
    let tick_next = tick_next.clamp(MIN_TICK, MAX_TICK);
    let sqrt_price_next = tick_math::get_sqrt_ratio_at_tick(tick_next);

    let step = swap_math::compute_swap_step(
        state.sqrt_price_x96,
        if zero_for_one {
            sqrt_price_next.max(sqrt_price_limit_x96)
        } else {
            sqrt_price_next.min(sqrt_price_limit_x96)
        },
        state.liquidity,
        state.amount_specified_remaining,
        fee_pips,
    );

    // ... state güncelle ...

    if state.amount_specified_remaining == 0 || state.sqrt_price_x96 == sqrt_price_limit_x96 {
        break;
    }
}
```

---

## Özet: 3 Kritik Bulgu

1. **Slot0 248-bit Paketi:** Tüm pool state tek storage slot'a sığdırılmış. Nova Shield'de bu Rust struct'a 1:1 map edilecek — ancak RocksDB'de değil, in-memory state olarak tutulacak (hız için).

2. **Optimistik Transfer + Callback Deseni:** Swap/Mint token önce çıkar, sonra callback'te ödeme istenir. Nova Shield'de bu desen anlamsız çünkü native DEX kendi token transfer modülüne sahip — direkt `assets::transfer()` çağrısı yapılır.

3. **Tick Geçişi Lazy Hesabı:** Oracle sadece tick geçildiğinde hesaplanır (`computedLatestObservation` flag). Nova Shield'de bu yaklaşım kopyalanmalı — gereksiz oracle güncellemesinden kaçınmak için.
