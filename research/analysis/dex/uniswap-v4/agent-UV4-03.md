# UV4-03: Uniswap V4 Pool.sol — BalanceDelta + Slot0 Yeniden Tasarımı + donate() + V3 Farkları

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v4-core/`
**Kapsanan Dosyalar:**
- `src/libraries/Pool.sol` (~600 satır — tam analiz)
- `src/types/Slot0.sol`
- `src/types/BalanceDelta.sol`
- `src/libraries/Position.sol` (V4 versiyonu)
- `src/libraries/ProtocolFeeLibrary.sol`

---

## 1. V4 Pool.State — V3'ten Yapısal Farklar

### 1.1 Pool.State Struct

```solidity
struct State {
    Slot0 slot0;
    uint256 feeGrowthGlobal0X128;
    uint256 feeGrowthGlobal1X128;
    uint128 liquidity;
    mapping(int24 tick => TickInfo) ticks;
    mapping(int16 wordPos => uint256) tickBitmap;
    mapping(bytes32 positionKey => Position.State) positions;
}
```

**V3'ten farklar:**
- `protocolFees` ayrı mapping yok — ProtocolFees contract'ta tutulur
- `observations` array yok — V4 yerleşik TWAP oracle kaldırıldı
- `Slot0` custom type oldu (bytes32 wrapper)
- `TickInfo` basitleşti — `tickCumulativeOutside`, `secondsPerLiquidityOutsideX128`, `secondsOutside` kaldırıldı
- `Position.State` içinde `salt` parametresi eklendi

### 1.2 V4 TickInfo — Sadeleşme

```solidity
struct TickInfo {
    uint128 liquidityGross;
    int128 liquidityNet;
    uint256 feeGrowthOutside0X128;
    uint256 feeGrowthOutside1X128;
    // V3'teki şunlar KALDIRILDI:
    // int56 tickCumulativeOutside         ← Oracle kaldırıldı
    // uint160 secondsPerLiquidityOutsideX128  ← Oracle kaldırıldı
    // uint32 secondsOutside               ← Oracle kaldırıldı
    // bool initialized                    ← liquidityGross != 0 ile check ediliyor
}
```

### 1.3 Slot0 — Custom Type Wrapper

```solidity
type Slot0 is bytes32;
```

Slot0 artık bir struct değil, `bytes32` üzerine inşa edilmiş custom type. Her alan accessor fonksiyonu ile okunur/yazılır:

```solidity
function sqrtPriceX96(Slot0 _packed) internal pure returns (uint160 _sqrtPriceX96) {
    assembly { _sqrtPriceX96 := shr(96, _packed) }
    // Üst 160 bit = sqrtPriceX96 (160 bit >> 96 → 0'a normalize)
}

function tick(Slot0 _packed) internal pure returns (int24 _tick) {
    assembly { _tick := signextend(2, shr(72, _packed)) }
    // Bit 72-95 = tick (24 bit)
}

function lpFee(Slot0 _packed) internal pure returns (uint24 _lpFee) {
    assembly { _lpFee := and(shr(48, _packed), 0xFFFFFF) }
    // Bit 48-71 = lpFee (24 bit)
}

function protocolFee(Slot0 _packed) internal pure returns (uint24 _protocolFee) {
    assembly { _protocolFee := and(shr(24, _packed), 0xFFFFFF) }
    // Bit 24-47 = protocolFee (24 bit)
}
```

**Bit düzeni (bytes32 = 256 bit):**
```
[255..96]  = sqrtPriceX96  (160 bit)
[95..72]   = tick          (24 bit, signed)
[71..48]   = lpFee         (24 bit)
[47..24]   = protocolFee   (24 bit)
[23..0]    = (gelecek kullanım)
```

**Setter örneği:**
```solidity
function setTick(Slot0 _packed, int24 _tick) internal pure returns (Slot0 _result) {
    assembly {
        _result := or(
            and(_packed, not(shl(72, 0xFFFFFF))),  // tick alanını temizle
            shl(72, and(_tick, 0xFFFFFF))           // yeni tick yaz
        )
    }
}
```

---

## 2. Pool.initialize() — V4 Versiyonu

```solidity
function initialize(State storage self, uint160 sqrtPriceX96, uint24 lpFee) internal returns (int24 tick) {
    if (self.slot0.sqrtPriceX96() != 0) PoolAlreadyInitialized.selector.revertWith();

    tick = TickMath.getTickAtSqrtPrice(sqrtPriceX96);

    self.slot0 = Slot0.wrap(bytes32(0))
        .setSqrtPriceX96(sqrtPriceX96)
        .setTick(tick)
        .setLpFee(lpFee);
    // protocolFee başlangıçta 0, observations yok
}
```

**V3 farkı:** V3'te `observations.initialize()` de çağrılıyordu. V4'te oracle yok.

---

## 3. Pool.modifyLiquidity() — V4 Versiyonu

### 3.1 Salt Parametresi

```solidity
struct ModifyLiquidityParams {
    address owner;
    int24 tickLower;
    int24 tickUpper;
    int128 liquidityDelta;
    int24 tickSpacing;
    bytes32 salt;  // YENİ: aynı owner, aynı range'de birden fazla pozisyon!
}
```

V3'te `(owner, tickLower, tickUpper)` benzersiz pozisyonu tanımlıyordu. V4'te `salt` eklenerek aynı range'de birden fazla pozisyon tutulabilir.

### 3.2 Pozisyon Key

```solidity
Position.State storage position = self.positions.get(params.owner, tickLower, tickUpper, params.salt);
```

```solidity
// Position.sol V4:
function get(
    mapping(bytes32 => State) storage self,
    address owner,
    int24 tickLower,
    int24 tickUpper,
    bytes32 salt
) internal view returns (State storage state) {
    bytes32 key;
    assembly {
        mstore(0, owner)
        mstore(32, tickLower)
        mstore(64, tickUpper)
        mstore(96, salt)
        key := keccak256(0, 128)  // 4 parametre hash
    }
    state = self[key];
}
```

V3'te 3 parametre hash (96 byte), V4'te 4 parametre hash (128 byte).

### 3.3 updateTick — Assembly Optimizasyonu

```solidity
// liquidityGrossAfter ve liquidityNet aynı slot'a birlikte yazılır (SSTORE sayısını azaltır)
assembly ("memory-safe") {
    sstore(
        info.slot,
        or(
            shl(128, liquidityNet),     // üst 128 bit = liquidityNet
            and(liquidityGrossAfter, 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF)  // alt 128 bit = liquidityGross
        )
    )
}
```

**V3 farkı:** V3'te `info.liquidityGross` ve `info.liquidityNet` ayrı SSTORE. V4'te tek SSTORE.

### 3.4 V4'te updateTick — V3'te tick.update() ile Fark

V3'te `tick.update()` içinde Oracle için `tickCumulative` ve `secondsPerLiquidity` alınıyordu. V4'te bunlar yok:

```solidity
// V3 tick.update() → Oracle verisi al:
(int56 tickCumulative, uint160 secondsPerLiquidityCumulativeX128) = observations.observeSingle(...)

// V4 Pool.updateTick() → Oracle verisi YOK
if (liquidityGrossBefore == 0) {
    if (tick <= self.slot0.tick()) {
        info.feeGrowthOutside0X128 = self.feeGrowthGlobal0X128;
        info.feeGrowthOutside1X128 = self.feeGrowthGlobal1X128;
        // tickCumulativeOutside, secondsOutside vs. → YOK
    }
}
```

### 3.5 modifyLiquidity Dönüş Değerleri

```solidity
returns (BalanceDelta delta, BalanceDelta feeDelta)
```

V3: `(Position.Info storage, int256, int256)` — ayrı değerler
V4: `(BalanceDelta, BalanceDelta)` — packed int256 tuples

`feeDelta` biriken fee'yi temsil eder. Caller `callerDelta = principalDelta + feeDelta` alır.

---

## 4. Pool.swap() — V3 vs V4 Detaylı Karşılaştırma

### 4.1 Fee Hesabı Farkı

**V3:**
```solidity
feeProtocol = slot0.feeProtocol % 16 veya >> 4  (4 bit)
delta = feeAmount / feeProtocol  (bölme)
```

**V4:**
```solidity
// ProtocolFeeLibrary
uint24 protocolFee = slot0Start.protocolFee().getZeroForOneFee();
// protocolFee Pips cinsinden (0-1000)

// calculateSwapFee: LP fee + protocol fee birleşimi
swapFee = protocolFee == 0 ? lpFee : uint16(protocolFee).calculateSwapFee(lpFee);

// Protocol fee katkısı:
uint256 delta = (swapFee == protocolFee)
    ? step.feeAmount
    : (step.amountIn + step.feeAmount) * protocolFee / PIPS_DENOMINATOR;
```

**calculateSwapFee formülü:**
```
swapFee = lpFee + protocolFee - (lpFee × protocolFee / MAX_FEE)
```
Bu, her iki fee'nin input token üzerinden bileşik uygulanmasını sağlar.

### 4.2 exactIn/exactOut Semantiği Değişimi

**V3:**
```
amountSpecified > 0 → exactInput (input miktarı verilmiş)
amountSpecified < 0 → exactOutput (output miktarı verilmiş)
```

**V4:**
```
amountSpecified < 0 → exactInput (negatif = giriyor)
amountSpecified > 0 → exactOutput (pozitif = çıkıyor)
```

Bu tam ters! V4'te işaret değişti. Kodda:
```solidity
// V4 Pool.swap():
if (params.amountSpecified > 0) {  // exactOutput
    amountSpecifiedRemaining -= step.amountOut.toInt256();
    amountCalculated -= (step.amountIn + step.feeAmount).toInt256();
} else {  // exactInput
    amountSpecifiedRemaining += (step.amountIn + step.feeAmount).toInt256();
    amountCalculated += step.amountOut.toInt256();
}
```

### 4.3 SwapMath.getSqrtPriceTarget() — Yeni Yardımcı

```solidity
// V4 SwapMath'e eklendi:
function getSqrtPriceTarget(
    bool zeroForOne,
    uint160 sqrtPriceNextX96,
    uint160 sqrtPriceLimitX96
) internal pure returns (uint160 sqrtPriceTarget) {
    assembly {
        // if zeroForOne, minimum of nextX96 and limitX96
        // if oneForZero, maximum of nextX96 and limitX96
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

**Formül açıklaması:**
- `zeroForOne=true`: `min(sqrtPriceNextX96, sqrtPriceLimitX96)` — kümülatif
- `zeroForOne=false`: `max(sqrtPriceNextX96, sqrtPriceLimitX96)` — kümülatif

Branchless (dallanmasız) implementasyon: `xor + mul` kombinasyonu.

### 4.4 Slot0 Güncelleme

**V3:** `(slot0.sqrtPriceX96, slot0.tick, slot0.observationIndex, slot0.observationCardinality)` — partial update

**V4:**
```solidity
self.slot0 = slot0Start.setTick(result.tick).setSqrtPriceX96(result.sqrtPriceX96);
```

Tek SSTORE — `setTick().setSqrtPriceX96()` zincirleme method pattern.

### 4.5 Oracle Güncellemesi — V4'te YOK

V3 swap sonunda oracle yazıyordu:
```solidity
// V3:
if (state.tick != slot0Start.tick) {
    (observationIndex, observationCardinality) = observations.write(...)
}
```

V4'te bu tamamen kaldırıldı. V4 oracle EIP-4626 benzeri ayrı ERC4626 contract olarak hook üzerinden implement edilebilir.

### 4.6 feeGrowthGlobal Güncelleme Farkı

**V3:**
```solidity
state.feeGrowthGlobalX128 += FullMath.mulDiv(step.feeAmount, FixedPoint128.Q128, state.liquidity);
```

**V4:**
```solidity
step.feeGrowthGlobalX128 += UnsafeMath.simpleMulDiv(step.feeAmount, FixedPoint128.Q128, result.liquidity);
// UnsafeMath.simpleMulDiv = (a * b) / c  (no FullMath, assume no overflow)
```

V4'te `FullMath.mulDiv` yerine `UnsafeMath.simpleMulDiv` kullanılır çünkü:
- `feeAmount` ≤ `amountSpecified` ≤ `type(int128).max` ≈ 2^127
- `Q128` = 2^128
- `liquidity` ≥ 1
- `feeAmount × Q128` ≤ 2^127 × 2^128 = 2^255 < 2^256 → overflow olmaz

---

## 5. donate() — Doğrudan LP Fee Dağıtımı

```solidity
function donate(State storage state, uint256 amount0, uint256 amount1) internal returns (BalanceDelta delta) {
    uint128 liquidity = state.liquidity;
    if (liquidity == 0) NoLiquidityToReceiveFees.selector.revertWith();
    unchecked {
        delta = toBalanceDelta(-(amount0.toInt128()), -(amount1.toInt128()));
        if (amount0 > 0) {
            state.feeGrowthGlobal0X128 += UnsafeMath.simpleMulDiv(amount0, FixedPoint128.Q128, liquidity);
        }
        if (amount1 > 0) {
            state.feeGrowthGlobal1X128 += UnsafeMath.simpleMulDiv(amount1, FixedPoint128.Q128, liquidity);
        }
    }
}
```

**Formül:**
```
feeGrowthGlobal0X128 += amount0 × Q128 / liquidity
feeGrowthGlobal1X128 += amount1 × Q128 / liquidity
```

LP'ler sonraki `modifyLiquidity` veya `collect` çağrısında bu artışı tokensOwed olarak alır.

**Geri dönüş:** `BalanceDelta` negatif → donör caller delta borçlanır (token ödemelidir).

**Uyarı (kaynak kodu notu):** Single LP pozisyonu olan pool'larda `donate()` + anında fee toplama ile `feeGrowthGlobal` şişirilebilir. Bu bir design limitation, security açığı değil.

---

## 6. getFeeGrowthInside — V3 vs V4 Farkı

**V3:**
```solidity
// Üç durum: tick < tickLower, tickLower ≤ tick < tickUpper, tick ≥ tickUpper
if (tickCurrent >= tickLower) feeGrowthBelow = lower.feeGrowthOutside;
else feeGrowthBelow = global - lower.feeGrowthOutside;

if (tickCurrent < tickUpper) feeGrowthAbove = upper.feeGrowthOutside;
else feeGrowthAbove = global - upper.feeGrowthOutside;

feeGrowthInside = global - feeGrowthBelow - feeGrowthAbove;
```

**V4:**
```solidity
// Daha direkt formüller:
if (tickCurrent < tickLower) {
    feeGrowthInside0 = lower.feeGrowthOutside0 - upper.feeGrowthOutside0;
} else if (tickCurrent >= tickUpper) {
    feeGrowthInside0 = upper.feeGrowthOutside0 - lower.feeGrowthOutside0;
} else {
    feeGrowthInside0 = global0 - lower.feeGrowthOutside0 - upper.feeGrowthOutside0;
}
```

**Matematiksel Eşdeğerlik İspatı:**

`tick < tickLower` durumunda:
- V3: `global - (global - lower.outside) - upper.outside = lower.outside - upper.outside`
- V4: `lower.outside - upper.outside` ✓ aynı

`tick >= tickUpper` durumunda:
- V3: `global - lower.outside - (global - upper.outside) = upper.outside - lower.outside`
- V4: `upper.outside - lower.outside` ✓ aynı

Matematiksel olarak eşdeğer ama V4 formülü iki durum için gereksiz `global` kullanmıyor — daha temiz.

---

## 7. crossTick — V4 Versiyonu (V3 cross() Farkı)

```solidity
function crossTick(
    State storage self,
    int24 tick,
    uint256 feeGrowthGlobal0X128,
    uint256 feeGrowthGlobal1X128
) internal returns (int128 liquidityNet) {
    unchecked {
        self.ticks[tick].feeGrowthOutside0X128 = feeGrowthGlobal0X128 - self.ticks[tick].feeGrowthOutside0X128;
        self.ticks[tick].feeGrowthOutside1X128 = feeGrowthGlobal1X128 - self.ticks[tick].feeGrowthOutside1X128;
    }
    liquidityNet = self.ticks[tick].liquidityNet;
}
```

**V3 cross() vs V4 crossTick():**
- V3: `tickCumulativeOutside`, `secondsPerLiquidityOutsideX128`, `secondsOutside` da güncelleniyor
- V4: Sadece `feeGrowthOutside` güncelleniyor (oracle alanları kaldırıldı)

---

## 8. Protocol Fee Sistemi — V4'e Özgü

### 8.1 Protocol Fee Format

V4'te protocol fee "pips" (binde bir) cinsinden:

```solidity
// ProtocolFeeLibrary
uint16 internal constant PIPS_DENOMINATOR = 1000;  // 0.1% = 1 pip

// protocolFee (uint24) = (oneForZero fee << 12) | zeroForOne fee
// Her 12 bit = 0-1000 (max ~0.1%)

function getZeroForOneFee(uint24 self) internal pure returns (uint16) {
    return uint16(self & 0xFFF);       // alt 12 bit
}

function getOneForZeroFee(uint24 self) internal pure returns (uint16) {
    return uint16(self >> 12);         // üst 12 bit
}
```

**V3 vs V4 Protocol Fee:**
| V3 | V4 |
|----|-----|
| `1/feeProtocol` (swap fee'nin katı) | Doğrudan pips (0-1000) |
| Max %25 | Max %0.1% |
| uint8 (0-10) | uint24 (12+12 bit) |

### 8.2 calculateSwapFee

```solidity
function calculateSwapFee(uint16 self, uint24 lpFee) internal pure returns (uint24 swapFee) {
    // self = protocolFee in pips (0-1000)
    // result: LP fee + protocol fee, combined
    return uint24(uint256(self) + uint256(lpFee) - uint256(self) * uint256(lpFee) / uint256(PIPS_DENOMINATOR));
}
```

**Formül:** `swapFee = protocolFee + lpFee - (protocolFee × lpFee / 1000)`

Bu bileşik faiz benzeri bir formül — iki ayrı fee uygulamak yerine tek birleşik fee hesaplanır.

---

## 9. V4 Position.State — salt Eklendi

```solidity
// V4 Position.State:
struct State {
    uint128 liquidity;
    uint256 feeGrowthInside0LastX128;
    uint256 feeGrowthInside1LastX128;
    // tokensOwed0, tokensOwed1 KALDIRILDI!
}
```

V4'te `tokensOwed` pozisyon struct'ında yok. Fee birikimi `feeDelta` olarak `modifyLiquidity` dönüş değerinde alınır ve caller delta'sına eklenir. `collect()` fonksiyonu yok — fee anında flash accounting'e girar.

---

## 10. V3 vs V4 Tam Karşılaştırma Tablosu

| Özellik | V3 | V4 |
|---------|-----|-----|
| Pool yapısı | Ayrı contract | Pool.State library |
| Slot0 tipi | Struct | bytes32 wrapper |
| Oracle | Gömülü circular buffer | Kaldırıldı (hook'a taşındı) |
| Protocol fee | 1/N (N=4-10) | Doğrudan pips (0-1000) |
| exactIn/exactOut | amountSpecified > 0 = exactIn | amountSpecified < 0 = exactIn |
| collect() | Ayrı fonksiyon | Yok (feeDelta döner) |
| Position salt | Yok | bytes32 salt |
| tokensOwed | Pozisyon'da saklanır | Döndürülür (anında) |
| TickInfo boyutu | 7 field | 4 field |
| feeGrowthGlobal | FullMath.mulDiv | UnsafeMath.simpleMulDiv |
| Swap return | (int256, int256) | BalanceDelta |
| Token transfer | V3Pool.mint/burn callback | PoolManager flash accounting |

---

## 11. Nova Shield Chain — Rust Implementasyon Notları

### 11.1 Pool State

```rust
pub struct PoolState {
    pub slot0: Slot0Packed,          // Bit-packed struct
    pub fee_growth_global_0_x128: U256,
    pub fee_growth_global_1_x128: U256,
    pub liquidity: u128,
    pub ticks: BTreeMap<i32, TickInfo>,
    pub tick_bitmap: HashMap<i16, u256>,
    pub positions: HashMap<[u8; 32], PositionState>,
}

pub struct Slot0Packed {
    pub sqrt_price_x96: u160,
    pub tick: i32,
    pub lp_fee: u32,
    pub protocol_fee: u32,
}
```

### 11.2 donate() → Nova Shield

```rust
pub fn donate(
    state: &mut PoolState,
    amount0: u128,
    amount1: u128,
) -> Result<BalanceDelta, PoolError> {
    if state.liquidity == 0 {
        return Err(PoolError::NoLiquidityToReceiveFees);
    }

    if amount0 > 0 {
        state.fee_growth_global_0_x128 = state.fee_growth_global_0_x128
            .wrapping_add(
                U256::from(amount0) * Q128 / U256::from(state.liquidity)
            );
    }
    if amount1 > 0 {
        state.fee_growth_global_1_x128 = state.fee_growth_global_1_x128
            .wrapping_add(
                U256::from(amount1) * Q128 / U256::from(state.liquidity)
            );
    }

    Ok(BalanceDelta {
        amount0: -(amount0 as i128),
        amount1: -(amount1 as i128),
    })
}
```

### 11.3 Nova Shield'de exactIn/exactOut — V4 Semantiği

Nova Shield V4 semantiğini benimser (negatif = exactIn):

```rust
pub fn is_exact_input(amount_specified: i256) -> bool {
    amount_specified < I256::ZERO
}
```

### 11.4 Slot0 Bit Packing — Rust'ta

```rust
// Rust'ta bit packing: bytemuck veya manual
#[repr(C)]
pub struct Slot0 {
    pub sqrt_price_x96: U160,  // 20 bytes
    pub tick: i32,              // 4 bytes (24 bit kullanılır)
    pub lp_fee: u32,            // 4 bytes (24 bit kullanılır)
    pub protocol_fee: u32,      // 4 bytes (24 bit kullanılır)
    _padding: [u8; 0],
}
// Toplam: <32 byte → tek RocksDB slot'a sığar
```

---

## Özet: 3 Kritik Bulgu

1. **V4 Oracle'ı Kaldırdı:** V3'teki gömülü TWAP oracle V4'te tamamen çıkarıldı. Kaynak: `TickInfo`'dan `tickCumulativeOutside`, `secondsPerLiquidityOutsideX128`, `secondsOutside` kaldırıldı. Nova Shield'de oracle ayrı bir TWAP modülü olarak implement edilecek — pool state'ini event'ler üzerinden takip eder.

2. **exactIn/exactOut İşareti Değişti:** V3'te `amountSpecified > 0 = exactIn`. V4'te `amountSpecified < 0 = exactIn`. Bu V3→V4 geçişinde kolayca kaçırılabilecek kritik bir anlam değişikliği. Nova Shield V4 standardını benimser.

3. **Position salt + Anında Fee:** V4'te aynı pozisyon (owner+range) için `bytes32 salt` ile birden fazla pozisyon tutulabilir. `tokensOwed` struct'tan kaldırılıp `feeDelta` olarak döndürülüyor — fee anında flash accounting'e entegre. Nova Shield'de her pozisyon için hash key `(owner, tickLower, tickUpper, salt)`.
