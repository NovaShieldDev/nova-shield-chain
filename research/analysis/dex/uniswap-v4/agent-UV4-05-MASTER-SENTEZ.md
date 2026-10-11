# UV4-05: MASTER SENTEZ — Uniswap V3+V4 Tam Karşılaştırması + Nova Shield DEX Mimari Kararları

**Analiz Tarihi:** 2026-07-22
**Kaynak Repolar:**
- `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v3-core/` (159 dosya, 48,671 satır)
- `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v3-periphery/` (183 dosya, 32,891 satır)
- `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v4-core/` (232 dosya, 222,384 satır)

**Bu dosya:** Tüm analizlerin (UV3-01 ~ UV4-04) sentezi + Nova Shield DEX implementasyon rehberi

---

## 1. Uniswap V3 Tam Özeti

### 1.1 Temel Tasarım Prensipleri

**Concentrated Liquidity (Yoğunlaştırılmış Likidite):**
```
V2: tüm fiyat aralığında [0, ∞) likidite
V3: [tickLower, tickUpper] aralığında likidite → sermaye verimliliği ↑10-4000x
```

**Sanal Rezervler:**
```
x_virtual = L / sqrt(P)
y_virtual = L × sqrt(P)
x_virtual × y_virtual = L²  (AMM invariant)
```

**Price ↔ Tick ↔ SqrtPrice ilişkisi:**
```
Price(tick) = 1.0001^tick
SqrtPrice(tick) = sqrt(1.0001^tick) × 2^96   [Q64.96 fixed-point]
```

### 1.2 V3 Kritik Matematik Bileşenleri

| Bileşen | Formül | Dosya |
|---------|--------|-------|
| 512-bit mulDiv | `(a×b mod 2^256, a×b mod (2^256-1)) + CRT` | FullMath.sol |
| Tick→Price | `∏(C_i^{bit_i}) × 2^128` (20 sabit) | TickMath.sol |
| Price→Tick | MSB + 14 iterasyon log_2 | TickMath.sol |
| Amount0 delta | `L × (sqrtB-sqrtA) / (sqrtA×sqrtB)` | SqrtPriceMath.sol |
| Amount1 delta | `L × (sqrtB-sqrtA)` | SqrtPriceMath.sol |
| Swap step | `computeSwapStep(sqrtCur, sqrtTarget, L, amount, fee)` | SwapMath.sol |
| Fee birikimi | `feeGrowthGlobal += fee × Q128 / liquidity` | UniswapV3Pool.sol |
| TWAP | `TWAP = (tickCumulative[t2] - tickCumulative[t1]) / (t2-t1)` | Oracle.sol |
| Fee inside | `feeGlobal - feeBelow - feeAbove` (overflow-safe) | Tick.sol |

### 1.3 V3 Güvenlik Modeli

| Açık | Mekanizma |
|------|-----------|
| Reentrancy | `slot0.unlocked` storage flag |
| Tick taşması | `tickNext = clamp(tickNext, MIN_TICK, MAX_TICK)` |
| Likidite taşması | `liquidityGross <= maxLiquidityPerTick` |
| Fiyat sınırı | `sqrtPriceLimitX96` zorunlu parametre |
| delegatecall | `noDelegateCall` modifier |
| Optimistik transfer | `balance_after >= balance_before + amount` |

---

## 2. Uniswap V4 Tam Özeti

### 2.1 V4'ün 5 Temel Yeniliği

**1. Singleton PoolManager:**
- V3: N pool = N contract, her swap N×2 token transfer
- V4: Tüm pool'lar tek PoolManager, flash accounting ile net settle

**2. Flash Accounting (Transient Storage):**
```
unlock() → callback → işlemler → settle()
NonzeroDeltaCount = 0 garantisi olmadan lock() açılmaz
```

**3. Hook Sistemi:**
```
14-bit hook permission bitmap → CREATE2 adresine gömülü
beforeSwap / afterSwap / beforeModifyLiquidity / afterModifyLiquidity / ...
beforeSwapReturnsDelta → pool math bypass mümkün
```

**4. ERC-6909 Multi-Token:**
```
Her currency için claim token
mint(to, currencyId, amount) → ERC6909 bas, delta sıfırla
burn(from, currencyId, amount) → ERC6909 yak, delta artır
```

**5. Native ETH Desteği:**
```
Currency = address (ERC20 veya address(0) = ETH)
settle() içinde msg.value ile ETH ödenebilir
```

### 2.2 V4 Kaldırılan Özellikler

| V3 Özelliği | V4 Durumu | Neden |
|-------------|-----------|-------|
| Gömülü TWAP Oracle | Kaldırıldı | Hook üzerinden implement edilebilir |
| `collect()` fonksiyonu | Kaldırıldı | `feeDelta` döndürülüyor |
| `mint()` / `burn()` ayrı | Birleşti → `modifyLiquidity` | Basitlik |
| `flash()` fonksiyonu | `take() + settle()` ile değiştirildi | Flash accounting yeterli |
| `NoDelegateCall` (pool'da) | PoolManager'da korundu | Singleton farklı risk |
| `ProtocolFees` pool'da | Ayrı contract | Separation of concerns |

### 2.3 V4 Matematiksel Farklar

| Formül | V3 | V4 |
|--------|-----|-----|
| exactIn işareti | `amountSpecified > 0` | `amountSpecified < 0` |
| feeGrowthGlobal | `FullMath.mulDiv` | `UnsafeMath.simpleMulDiv` |
| Protocol fee | `1/N` (4-10) | pips (0-1000) |
| getSqrtPriceTarget | ternary | branchless XOR/MUL |
| price bounds | 2 compare | arithmetic trick (1 op) |

---

## 3. V3 vs V4 Karşılaştırma Tablosu (Tam)

| Kategori | Uniswap V3 | Uniswap V4 |
|----------|-----------|-----------|
| **Mimari** | Multi-contract | Singleton |
| **Likidite** | Concentrated [tL, tU] | Concentrated [tL, tU] |
| **Fiyat Formatı** | Q64.96 (uint160) | Q64.96 (uint160) |
| **Fee Format** | 100/500/3000/10000 bps | Pips (0-1,000,000) |
| **Dinamik Fee** | Yok | Hook üzerinden |
| **Oracle** | Circular buffer (65535) | Yok (hook'ta) |
| **Reentrancy** | storage flag (5000 gas) | transient (100 gas) |
| **Token Std** | ERC-20 | ERC-6909 + ERC-20 |
| **Native ETH** | Yok | Var |
| **Flash Loan** | `pool.flash()` | `take() + settle()` |
| **Position Salt** | Yok | bytes32 |
| **Hooks** | Yok | 14 hook noktası |
| **Tick Info** | 7 alan | 4 alan |
| **Deploy Cost** | Her pool için | Sadece PoolManager |
| **Multi-hop** | N×2 transfer | 2 transfer (net) |
| **exactIn Sign** | +positive | -negative |
| **Slot0 Tipi** | Struct | bytes32 wrapper |
| **Solidity Versiyon** | 0.7.6 | 0.8.26 |

---

## 4. Nova Shield Chain — DEX Mimari Kararları

### 4.1 Temel Prensipler

Nova Shield native L1 DEX'i (Hyperliquid benzeri) şu prensiplere dayalı:

1. **Protokol Seviyesi:** DEX, smart contract değil — kernel modülü
2. **Native Varlıklar:** EVM token callback'i yok, native asset transfer
3. **Rust Sahipliği:** Reentrancy Rust ownership ile imkansız
4. **RocksDB:** Pool state persistent — memory + disk hybrid
5. **Tokio Async:** Paralel tick bitmap taraması mümkün

### 4.2 Neyi V3'ten Alıyoruz

```
✅ Concentrated liquidity algoritması (aynen)
✅ TickMath magic constants (aynen, compile-time const)
✅ SqrtPriceMath formülleri (aynen)
✅ SwapMath.computeSwapStep (aynen)
✅ FullMath 512-bit mulDiv (Rust U512 ile)
✅ TickBitmap O(1) next tick (Rust native bit ops)
✅ feeGrowthGlobal / feeGrowthInside / feeGrowthBelow+Above pattern
✅ liquidityNet invariant (∑ = 0)
✅ Tick flip mechanism
✅ Position.tokensOwed (V3 semantiği, V4'te kaldırıldı ama biz tutarız)
```

### 4.3 Neyi V4'ten Alıyoruz

```
✅ exactIn/exactOut işaret semantiği (-/+ yerine +/- DEĞİL, V4: -=exactIn)
✅ Position salt
✅ BalanceDelta konsepti (Rust tuple olarak)
✅ Flash accounting prensibini (native tx context olarak)
✅ Protocol fee pips format
✅ Hook sistemi prensibini (Rust trait olarak)
✅ Slot0 bit-packing (bytes32 yerine Rust packed struct)
✅ getSqrtPriceTarget branchless (LLVM optimize eder)
✅ Singleton pool mapping (PoolId → PoolState HashMap)
✅ Salt eklenmesiyle çoklu pozisyon
```

### 4.4 Nova Shield'de Yapılmayacaklar

```
❌ EVM callback pattern (mint/swap/flash callback) → native asset transfer
❌ SLOAD/SSTORE gas optimization → RocksDB I/O optimization
❌ assembly optimizasyonlar → Rust unsafe + SIMD (gerekirse)
❌ delegatecall koruması → Rust'ta mevcut değil
❌ ERC-6909 → native multi-asset modülü
❌ Solidity custom error → Rust Result<T, DexError>
❌ NoDelegateCall modifier → gereksiz
```

### 4.5 Nova Shield'e Özgü Eklemeler

```
✅ ZK Proof Entegrasyonu: Her swap'ın fiyat değişimini ZK kanıtla doğrula
✅ MPC Controlled LP: cggmp21 ile çoklu imza gerektiren büyük LP pozisyonları
✅ Cross-chain Liquidity: libp2p üzerinden başka zincirlerle likidite köprüsü
✅ MEV Koruması: Sequencer'da commit-reveal ile front-running önlemi
✅ Native Order Book: DEX + CLOB hibrit (Uniswap + dYdX)
```

---

## 5. Nova Shield DEX Rust Crate Yapısı

```
nova-shield-chain/crates/dex/
├── src/
│   ├── lib.rs
│   ├── pool/
│   │   ├── mod.rs          (Pool.State)
│   │   ├── state.rs        (PoolState struct)
│   │   ├── swap.rs         (swap loop)
│   │   ├── liquidity.rs    (modify_liquidity)
│   │   └── donate.rs       (donate)
│   ├── math/
│   │   ├── full_math.rs    (mulDiv 512-bit)
│   │   ├── tick_math.rs    (getSqrtPriceAtTick)
│   │   ├── sqrt_price_math.rs  (amount deltas)
│   │   ├── swap_math.rs    (computeSwapStep)
│   │   ├── bit_math.rs     (MSB/LSB)
│   │   └── fixed_point.rs  (Q96, Q128 constants)
│   ├── tick/
│   │   ├── mod.rs          (Tick.Info)
│   │   ├── bitmap.rs       (TickBitmap)
│   │   └── fee.rs          (getFeeGrowthInside)
│   ├── position/
│   │   ├── mod.rs          (Position.State)
│   │   └── update.rs       (fee claim)
│   ├── oracle/
│   │   ├── mod.rs          (TWAP)
│   │   └── circular_buf.rs (Observation ring buffer)
│   ├── hooks/
│   │   ├── mod.rs          (Hook trait)
│   │   └── registry.rs     (HookRegistry)
│   ├── pool_manager/
│   │   ├── mod.rs          (PoolManager)
│   │   └── flash_accounting.rs (TransactionContext)
│   └── types/
│       ├── balance_delta.rs (BalanceDelta)
│       ├── pool_key.rs     (PoolKey)
│       ├── pool_id.rs      (PoolId)
│       └── currency.rs     (CurrencyId)
```

---

## 6. Nova Shield DEX Swap Akışı (End-to-End)

```rust
pub async fn execute_swap(
    pool_manager: &mut PoolManager,
    key: PoolKey,
    params: SwapParams,
    caller: AccountId,
) -> Result<BalanceDelta, DexError> {

    // 1. Pool'u bul
    let pool_id = key.pool_id();
    let pool = pool_manager.get_pool_mut(&pool_id)?;

    // 2. Hook: beforeSwap
    let (amount_to_swap, lp_fee_override) = if let Some(hook) = &pool.hook {
        hook.before_swap(&key, &params, &[]).await?
    } else {
        (params.amount_specified, None)
    };

    // 3. Swap hesapla
    let (swap_delta, protocol_fee, swap_fee, result) = pool.state.swap(
        SwapParams {
            amount_specified: amount_to_swap,
            lp_fee_override,
            ..params
        }
    )?;

    // 4. Protocol fee güncelle
    if protocol_fee > 0 {
        pool_manager.update_protocol_fees(&key.currency_input(), protocol_fee);
    }

    // 5. Flash accounting — delta kaydet
    let ctx = pool_manager.transaction_context();
    ctx.account_delta(key.currency0, caller, swap_delta.amount0);
    ctx.account_delta(key.currency1, caller, swap_delta.amount1);

    // 6. Hook: afterSwap
    let (final_delta, hook_delta) = if let Some(hook) = &pool.hook {
        hook.after_swap(&key, &params, swap_delta, &[]).await?
    } else {
        (swap_delta, BalanceDelta::zero())
    };

    // 7. Asset transfer (flash accounting çözümü)
    ctx.settle_all(&pool_manager.asset_module)?;

    Ok(final_delta)
}
```

---

## 7. Nova Shield Oracle Tasarımı

V4 oracle kaldırdı, Nova Shield ayrı oracle modülü:

```rust
pub struct OracleModule {
    observations: HashMap<PoolId, CircularBuffer<Observation>>,
    cardinality: HashMap<PoolId, u16>,
}

pub struct Observation {
    pub block_timestamp: u32,
    pub tick_cumulative: i64,       // tick × delta zaman
    pub seconds_per_liquidity_x128: U256,
    pub initialized: bool,
}

impl OracleModule {
    // Swap sonrası çağrılır (event-based, not inline)
    pub fn record_swap(&mut self, pool_id: PoolId, tick: i32, liquidity: u128, timestamp: u32) {
        let obs = self.observations.entry(pool_id).or_default();
        obs.write(timestamp, tick, liquidity);
    }

    // TWAP hesabı
    pub fn get_twap(&self, pool_id: PoolId, seconds_ago: u32) -> Result<i32, OracleError> {
        let obs = self.observations.get(&pool_id).ok_or(OracleError::NotInitialized)?;
        let (tc1, tc2) = obs.observe(seconds_ago)?;
        Ok(((tc2 - tc1) / seconds_ago as i64) as i32)
    }
}
```

---

## 8. Kritik Bileşen Rust Implementasyonu — FullMath

```rust
// crates/dex/src/math/full_math.rs
use uint::U512;

pub fn mul_div(a: u256, b: u256, denominator: u256) -> u256 {
    assert!(denominator > u256::ZERO, "denominator must be nonzero");

    let product = U512::from(a) * U512::from(b);

    assert!(
        U512::from(denominator) > (product >> 256),
        "denominator must be greater than prod1 (result would overflow)"
    );

    (product / U512::from(denominator)).try_into().expect("result fits in u256")
}

pub fn mul_div_rounding_up(a: u256, b: u256, denominator: u256) -> u256 {
    let result = mul_div(a, b, denominator);
    let remainder = (U512::from(a) * U512::from(b)) % U512::from(denominator);
    if remainder > U512::ZERO {
        assert!(result < u256::MAX, "overflow");
        result + u256::ONE
    } else {
        result
    }
}
```

---

## 9. Kritik Bileşen Rust Implementasyonu — TickMath

```rust
// crates/dex/src/math/tick_math.rs

const TICK_MAGIC: [u128; 20] = [
    0xfffcb933bd6fad37aa2d162d1a594001,
    0xfff97272373d413259a46990580e213a,
    0xfff2e50f5f656932ef12357cf3c7fdcc,
    0xffe5caca7e10e4e61c3624eaa0941cd0,
    0xffcb9843d60f6159c9db58835c926644,
    0xff973b41fa98c081472e6896dfb254c0,
    0xff2ea16466c96a3843ec78b326b52861,
    0xfe5dee046a99a2a811c461f1969c3053,
    0xfcbe86c7900a88aedcffc83b479aa3a4,
    0xf987a7253ac413176f2b074cf7815e54,
    0xf3392b0822b70005940c7a398e4b70f3,
    0xe7159475a2c29b7443b29c7fa6e889d9,
    0xd097f3bdfd2022b8845ad8f792aa5825,
    0xa9f746462d870fdf8a65dc1f90e061e5,
    0x70d869a156d2a1b890bb3df62baf32f7,
    0x31be135f97d08fd981231505542fcfa6,
    0x9aa508b5b7a84e1c677de54f3e99bc9,
    0x5d6af8dedb81196699c329225ee604,
    0x2216e584f5fa1ea926041bedfe98,
    0x48a170391f7dc42444e8fa2,
];

pub const MIN_TICK: i32 = -887272;
pub const MAX_TICK: i32 = 887272;
pub const MIN_SQRT_PRICE: u160 = 4295128739;
pub const MAX_SQRT_PRICE: u160 = 1461446703485210103287273052203988822378723970342;

pub fn get_sqrt_price_at_tick(tick: i32) -> u160 {
    let abs_tick = tick.unsigned_abs();
    assert!(abs_tick <= MAX_TICK as u32, "tick out of bounds");

    let mut ratio: U256 = if abs_tick & 1 != 0 {
        U256::from(TICK_MAGIC[0])
    } else {
        U256::one() << 128
    };

    for i in 1..20_usize {
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
        (shifted + 1).as_u128() as u160  // ceiling
    } else {
        shifted.as_u128() as u160
    }
}
```

---

## 10. Nova Shield vs Hyperliquid Karşılaştırması

| Özellik | Hyperliquid | Nova Shield DEX |
|---------|-------------|-----------------|
| Temel | CLOB (order book) | CLMM + CLOB Hibrit |
| Likidite modeli | Off-chain book | V3/V4 konsantre |
| Clearing | On-chain settle | Native L1 |
| ZK | Yok | Groth16/PLONK |
| MPC | Yok | cggmp21 |
| P2P | Tendermint | libp2p |
| Oracle | Internal | Ayrı modül (TWAP + external) |
| Fee | Maker-Taker | AMM fee + dinamik |

---

## 11. Güvenlik Öncelik Listesi (Nova Shield DEX için)

### Yüksek Öncelik (Implement etmeden önce audit)

1. **FullMath 512-bit:** Newton-Raphson seed doğruluğu, U512 overflow edge case
2. **TickMath sınırlar:** MIN_TICK/MAX_TICK clamp, tick geçiş edge case
3. **liquidityNet invariant:** ∑ = 0 olduğunun matematiksel garantisi
4. **feeGrowthInside overflow:** uint256 wrap-around her koşulda doğru mu?
5. **swap loop termination:** Sonsuz döngü riski (tick sıkıştırma yeterli mi?)

### Orta Öncelik

6. **Protocol fee precision:** Pips rounding yönü
7. **Position salt collision:** keccak256 preimage resistance
8. **Hook trust model:** Yetkisiz hook kaydı engellenmiş mi?
9. **Flash accounting balance:** settle edilmeden işlem bitirilemiyor mu?

### Düşük Öncelik

10. **Oracle manipulation:** TWAP tek kaynak değil (multi-source)
11. **donate() inflation:** Single LP pool'da manipülasyon
12. **ERC-6909 approve:** Allowance integer underflow

---

## 12. Tüm Analiz Dosyaları Özeti

| Dosya | Konu | Satır |
|-------|------|-------|
| UV3-01.md | FullMath + TickMath + SqrtPriceMath + SwapMath + BitMath | ~800 |
| UV3-02.md | UniswapV3Pool.sol (mint/burn/swap/flash/collect) | ~850 |
| UV3-03.md | Tick + TickBitmap + Position + Fee Growth | ~800 |
| UV3-04.md | Oracle (TWAP, circular buffer, Echidna) | ~800 |
| UV3-05.md | Periphery (SwapRouter, NPM, QuoterV2) | ~800 |
| UV4-01.md | PoolManager + Flash Accounting + ERC-6909 + BalanceDelta | ~900 |
| UV4-02.md | Hooks sistemi + 14-bit bitmap + Dinamik fee | ~850 |
| UV4-03.md | Pool.sol V4 + Slot0 custom type + donate() + V3 farkları | ~850 |
| UV4-04.md | V3 vs V4 math karşılaştırması + BitMath + SwapMath V4 | ~800 |
| UV4-05.md | Bu dosya — Master Sentez + Nova Shield mimari | ~1000 |

**Toplam analiz:** ~8450 satır Türkçe teknik dokümantasyon

---

## Özet: 3 Ana Bulgu

1. **V4 Flash Accounting > V3 Multi-Transfer:** Multi-hop swap'ta V4 net settle mekanizması yüzlerce gas tasarrufu sağlar. Nova Shield'de bu pattern native tx context olarak uygulanır — hiçbir token transfer callback yoktur.

2. **Concentrated Liquidity Rust'ta Karmaşıklık:** TickMath'ın 20 magic constant'ı, FullMath'ın 512-bit Newton-Raphson'ı ve Tick'in döngüsel overflow aritmetiği Rust'a taşınırken `wrapping_*` operasyonları kritik. Yanlış `checked_*` kullanımı doğru sonuç vermez.

3. **Nova Shield Hibrit Model:** Uniswap V3/V4'ün CLMM (Concentrated Liquidity Market Maker) algoritması + dYdX V4'ün order book yapısı birleştirilir. AMM likidite pasif LP'leri desteklerken, CLOB aktif trader'lara hizmet eder. ZK proof her swap/order execution'ı on-chain doğrular.
