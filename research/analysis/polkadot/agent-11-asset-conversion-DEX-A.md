# pallet-asset-conversion — DEX Analiz A (AMM Math & Core)
**Agent**: 11-A | **Tarih**: 2026-07-22 | **Hassasiyet**: 3X (DEX)

---

## 1. Dizin Yapısı ve Dosya Listesi

```
substrate/frame/asset-conversion/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs          (~1643 satır) — ANA PALLET: tüm AMM math, extrinsics, storage
│   ├── types.rs        (172 satır)   — PoolInfo, PoolLocator trait, AccountIdConverter
│   ├── swap.rs         (261 satır)   — Swap/SwapCredit/QuotePrice trait impl
│   ├── liquidity.rs    (139 satır)   — MutateLiquidity trait impl
│   ├── mock.rs         (182 satır)   — Test ortamı konfigürasyonu
│   ├── tests.rs        (~2000+ satır) — Kapsamlı test seti
│   ├── benchmarking.rs (~200 satır)  — Benchmark helper'ları
│   └── weights.rs      (otomatik üretilmiş)
├── ops/src/
│   ├── lib.rs          (337 satır)   — Migrasyon operasyonları (pool AccountId değişimi)
│   ├── mock.rs         — Ops test ortamı
│   ├── tests.rs        — Ops testleri
│   ├── benchmarking.rs — Ops benchmark'ları
│   └── weights.rs
└── precompiles/src/
    ├── lib.rs          (551 satır)   — Solidity precompile (EVM entegrasyonu)
    ├── mock.rs         — Precompile test ortamı
    └── tests.rs        — Precompile testleri
```

**Toplam**: 16 .rs dosyası

---

## 2. Storage Yapıları

### `Pools<T>` — lib.rs:215-217
```rust
pub type Pools<T: Config> =
    StorageMap<_, Blake2_128Concat, T::PoolId, PoolInfo<T::PoolAssetId>, OptionQuery>;
```
- **Key**: `T::PoolId` (genellikle `(AssetKind, AssetKind)` tuple)
- **Value**: `PoolInfo { lp_token: PoolAssetId }` — sadece LP token ID'si tutar
- **Hasher**: Blake2_128Concat
- **Açıklama**: Var olan pool'ları kaydeder. Bir pool'un "resmi olarak oluşturulmuş" olup olmadığını belirler. Pool reserve'lerini SAKLAMAZ — bunlar doğrudan Assets pallet'inde tutulur.

### `NextPoolAssetId<T>` — lib.rs:221-222
```rust
pub type NextPoolAssetId<T: Config> = StorageValue<_, T::PoolAssetId, OptionQuery>;
```
- **Type**: StorageValue
- **Value**: `Option<T::PoolAssetId>`
- **Açıklama**: Sıradaki LP token asset ID'sini tutar. Her yeni pool oluşturulduğunda increment edilir.

### `PoolFees<T>` — lib.rs:229-230
```rust
pub type PoolFees<T: Config> = StorageMap<_, Blake2_128Concat, T::PoolId, Permill, OptionQuery>;
```
- **Key**: `T::PoolId`
- **Value**: `Permill` (per-pool fee override)
- **Açıklama**: Per-pool swap fee override'ları. Entry yoksa global `Config::LPFee` kullanılır. Salt additive: mevcut pool'lar etkilenmez (migration gereksiz).

---

## 3. AMM Formülü (KAYNAK DOĞRULAMALI)

Pallet **Uniswap V2** x*y=k constant-product formülüne dayanır (lib.rs:20).

### Temel İlişki
Pool her zaman şu değişmezi korur:
```
reserve_in * reserve_out = k (constant product)
```

### quote() — Spot Fiyat Hesabı (FEE YOK)
**lib.rs:1344-1351**
```rust
pub fn quote(amount: &Balance, reserve1: &Balance, reserve2: &Balance) -> Result<Balance, Error<T>> {
    // (amount * reserve2) / reserve1
    Self::mul_div(amount, reserve2, reserve1)
}
```
**Formül**: `amount_out = (amount_in * reserve_out) / reserve_in`

Bu "spot price" hesabıdır — fee dahil değildir. Sadece mevcut rezerv oranından fiyat verir.

### mul_div() — Yardımcı Fonksiyon
**lib.rs:1370-1382**
```rust
fn mul_div(a: &Balance, b: &Balance, c: &Balance) -> Result<Balance, Error<T>> {
    let a = T::HigherPrecisionBalance::from(*a);
    let b = T::HigherPrecisionBalance::from(*b);
    let c = T::HigherPrecisionBalance::from(*c);
    let result = a.checked_mul(&b)?.checked_div(&c)?;
    result.try_into().map_err(|_| Error::<T>::Overflow)
}
```
Overflow önlemek için `HigherPrecisionBalance` (mock'da `U256`) kullanılır.

---

## 4. Fee Hesabı (KAYNAK DOĞRULAMALI)

### Global LPFee Config
**lib.rs:163-168**
```rust
#[pallet::constant]
type LPFee: Get<Permill>;
```
**Mock değeri** — mock.rs:127:
```rust
pub LpFee: Permill = Permill::from_rational(3u32, 1_000u32); // 0.3%
```

### MaxSwapFee
**lib.rs:175-176**
```rust
#[pallet::constant]
type MaxSwapFee: Get<Permill>;
```
**Mock değeri** — mock.rs:128: `Permill::from_percent(10)` → max %10

### LiquidityWithdrawalFee
**lib.rs:190-191**
```rust
#[pallet::constant]
type LiquidityWithdrawalFee: Get<Permill>;
```
**Mock değeri** — mock.rs:129: `Permill::from_percent(0)` (default 0)

### Per-Pool Fee Çözümleme — pool_fee()
**lib.rs:1275-1277**
```rust
pub fn pool_fee(pool_id: &T::PoolId) -> Permill {
    PoolFees::<T>::get(pool_id).unwrap_or_else(T::LPFee::get)
}
```
**Öncelik**: PoolFees storage override > global LPFee

### get_amount_out() — Fee ile amount_out hesabı
**lib.rs:1388-1419**
```rust
pub fn get_amount_out(fee: Permill, amount_in: &Balance, reserve_in: &Balance, reserve_out: &Balance) -> Result<Balance, Error<T>> {
    let fee_complement = fee.left_from_one().deconstruct(); // = Permill::ACCURACY - fee_parts
    let amount_in_with_fee = amount_in * fee_complement;
    let numerator = amount_in_with_fee * reserve_out;
    let denominator = reserve_in * Permill::ACCURACY + amount_in_with_fee;
    numerator / denominator
}
```

**Matematiksel formül** (fee = Permill türü, ACCURACY = 1_000_000):
```
fee_complement = ACCURACY - fee_raw   // örn. 0.3% için: 1_000_000 - 3_000 = 997_000
amount_in_with_fee = amount_in * fee_complement
numerator = amount_in_with_fee * reserve_out
denominator = reserve_in * ACCURACY + amount_in_with_fee
amount_out = numerator / denominator
```

**Basitleştirilmiş** (fee = f, ACCURACY = A):
```
amount_out = (amount_in * (A-f) * reserve_out) / (reserve_in * A + amount_in * (A-f))
```
Bu Uniswap V2 standart formülüdür (`(x * 997 * dy) / (y * 1000 + x * 997)` gibi fakat Permill ile).

**Test doğrulaması** — tests.rs:161-167:
```rust
assert_eq!(
    AssetConversion::get_amount_out(LpFee::get(), &100u128, &u128::MAX, &u128::MAX).ok().unwrap(),
    99
);
```
100 birim girdi, 0.3% fee ile 99 birim çıktı → Uniswap formülüyle tutarlı.

### get_amount_in() — Fee ile amount_in hesabı
**lib.rs:1425-1463**
```rust
pub fn get_amount_in(fee: Permill, amount_out: &Balance, reserve_in: &Balance, reserve_out: &Balance) -> Result<Balance, Error<T>> {
    if amount_out >= reserve_out { Err(AmountOutTooHigh) }

    let fee_complement = fee.left_from_one().deconstruct();
    let numerator = reserve_in * amount_out * ACCURACY;
    let denominator = (reserve_out - amount_out) * fee_complement;
    numerator / denominator + 1  // +1: ceiling division
}
```

**Matematiksel formül**:
```
amount_in = (reserve_in * amount_out * ACCURACY) / ((reserve_out - amount_out) * fee_complement) + 1
```

**Kritik**: `+1` (One::one() ekleme) — ceiling division. Alıcı her zaman istenen miktarı alır. Test — tests.rs:168-173: `get_amount_in(&100, MAX, MAX) = 101`

---

## 5. add_liquidity Matematği

**lib.rs:790-892**

### İlk Likidite (Sıfırdan)

```rust
lp_token_amount = Self::calc_lp_amount_for_zero_supply(&amount1, &amount2)?;
T::PoolAssets::mint_into(pool.lp_token.clone(), &pool_account, T::MintMinLiquidity::get());
T::PoolAssets::mint_into(lp_token, mint_to, lp_token_amount)?;
```

#### calc_lp_amount_for_zero_supply() — lib.rs:1353-1368
```rust
pub(super) fn calc_lp_amount_for_zero_supply(amount1: &Balance, amount2: &Balance) -> Result<Balance, Error<T>> {
    let result = (amount1 * amount2).integer_sqrt()
        .checked_sub(&T::MintMinLiquidity::get().into())
        .ok_or(Error::<T>::InsufficientLiquidityMinted)?;
    result.try_into()
}
```

**Formül**: `lp_minted_to_user = sqrt(amount1 * amount2) - MintMinLiquidity`

**Toplam mint edilen**: `sqrt(amount1 * amount2)` (= user aldığı + pool_account'a kilitlenen)

**Locked liquidity**: `MintMinLiquidity` pool_account'a mint edilir (= Uniswap'ın 1000 MINIMUM_LIQUIDITY'si gibi, sadece farklı değer)

**Mock değeri**: `MintMinLiquidity = 100` (mock.rs:163)

**Test doğrulaması**: tests.rs:382-383 — `(10000, 10)` için: `sqrt(10000*10) - 100 = sqrt(100000) - 100 = 316 - 100 = 216`. Event'te `lp_token_minted: 216` görülür.

### Mevcut Likiditeye Ekleme

**Optimal miktar hesabı** — lib.rs:822-843:
```rust
let amount2_optimal = Self::quote(&amount1_desired, &reserve1, &reserve2)?;
// = amount1_desired * reserve2 / reserve1

if amount2_optimal <= amount2_desired {
    amount1 = amount1_desired;
    amount2 = amount2_optimal;
} else {
    let amount1_optimal = Self::quote(&amount2_desired, &reserve2, &reserve1)?;
    amount1 = amount1_optimal;
    amount2 = amount2_desired;
}
```
**Amaç**: Mevcut rezerv oranına en yakın miktarları bul.

**LP token miktarı** — lib.rs:869-871:
```rust
let side1 = Self::mul_div(&amount1, &total_supply, &reserve1)?;
let side2 = Self::mul_div(&amount2, &total_supply, &reserve2)?;
lp_token_amount = side1.min(side2);
```

**Formül**:
```
side1 = amount1 * total_lp_supply / reserve1
side2 = amount2 * total_lp_supply / reserve2
lp_minted = min(side1, side2)
```

Bu Uniswap V2'nin standart LP token hesabıyla aynı. `min` alınır çünkü her iki varlığın oranıyla tutarlı miktar hesaplanır; az olanı alınır (price manipulation önleme).

### Minimum LP Kontrolü — lib.rs:874-877:
```rust
ensure!(
    lp_token_amount > T::MintMinLiquidity::get(),
    Error::<T>::InsufficientLiquidityMinted
);
```
`lp_token_amount > MintMinLiquidity` olmalı (strict greater than). Mock'ta 100.

---

## 6. remove_liquidity Matematği

**lib.rs:894-966**

```rust
let withdrawal_fee_amount = T::LiquidityWithdrawalFee::get() * lp_token_burn;
let lp_redeem_amount = lp_token_burn.saturating_sub(withdrawal_fee_amount);

let amount1 = Self::mul_div(&lp_redeem_amount, &reserve1, &total_supply)?;
let amount2 = Self::mul_div(&lp_redeem_amount, &reserve2, &total_supply)?;
```

**Formül**:
```
withdrawal_fee_amount = LiquidityWithdrawalFee * lp_token_burn
lp_redeem_amount = lp_token_burn - withdrawal_fee_amount
amount1 = lp_redeem_amount * reserve1 / total_supply
amount2 = lp_redeem_amount * reserve2 / total_supply
```

**Önemli**: `lp_token_burn` TAM OLARAK yakılır (fee dahil). Ama hesaplamada `lp_redeem_amount` kullanılır.
`withdrawal_fee_amount` yakılır ama token karşılığı verilmez — bu LP için efektif bir "çekim cezası"dır.

**LP Token Yakma** — lib.rs:941-949:
```rust
T::PoolAssets::burn_from(
    pool.lp_token.clone(),
    who,
    lp_token_burn,  // tam miktar yakılır (fee dahil)
    Expendable, Exact, Polite,
)?;
```

**Test doğrulaması** — tests.rs:562-583:
LiquidityWithdrawalFee = %10 ile:
- `reserve1=1_000_000_000, reserve2=100_000, total_lp=total_lp_received`
- Yakılan: `total_lp_received` (fee dahil)
- `lp_redeem = total_lp * 0.9`
- `amount1 = lp_redeem * 1_000_000_000 / total_lp`
- Olay çıktısı: `amount1: 899_991_000, amount2: 89_999`

---

## 7. swap_exact_tokens_for_tokens

**lib.rs:980-1013 (do_swap_exact_tokens_for_tokens)**

### Akış:
1. `amount_in > 0` ve `amount_out_min > 0` kontrolleri
2. `validate_swap_path(&path)` — path uzunluğu, unique pool kontrolleri
3. `balance_path_from_amount_in(amount_in, path)` — her hop için get_amount_out() çağrılır
4. Son elemandaki `amount_out` alınır
5. `amount_out >= amount_out_min` kontrolü
6. `swap(&sender, &path, &send_to, keep_alive)` — token transferleri

### balance_path_from_amount_in() — lib.rs:1319-1341
```rust
// İleri yönde: amount_in → amount_out for each hop
let mut amount_out = amount_in;
for each consecutive pair (asset1, asset2) in path:
    fee = pool_fee_for(asset1, asset2)
    (reserve_in, reserve_out) = get_reserves(asset1, asset2)
    amount_out = get_amount_out(fee, &amount_out, &reserve_in, &reserve_out)
```

**Dönen değer**: `amount_out` (son aldığımız miktar)

### credit_swap() — Multi-hop transfer mekanizması — lib.rs:1194-1243
```rust
// asset_in → pool_12
// asset2 → pool_23 (intermediate transfer)
// asset_out ← pool_last
```
Her hop için aralarındaki pool'lar arasında transfer yapılır. Son hop'ta `credit_out` elde edilir.

---

## 8. swap_tokens_for_exact_tokens

**lib.rs:1028-1063 (do_swap_tokens_for_exact_tokens)**

### Akış:
1. `amount_out > 0` ve `amount_in_max > 0` kontrolleri
2. `validate_swap_path(&path)`
3. `balance_path_from_amount_out(amount_out, path)` — **geriye doğru** her hop için get_amount_in() çağrılır
4. İlk elemandaki `amount_in` alınır
5. `amount_in <= amount_in_max` kontrolü
6. `swap(&sender, &path, &send_to, keep_alive)` — token transferleri

### balance_path_from_amount_out() — lib.rs:1292-1316
```rust
// Geriye yönde: amount_out → required amount_in for each hop
let mut amount_in = amount_out;
for each consecutive pair (asset1, asset2) in REVERSED path:
    fee = pool_fee_for(asset1, asset2)
    (reserve_in, reserve_out) = get_reserves(asset1, asset2)
    amount_in = get_amount_in(fee, &amount_in, &reserve_in, &reserve_out)
```

**Dönen değer**: `amount_in` (ödememiz gereken miktar, ceiling division ile)

### Fark: exact_in vs exact_out
| Özellik | swap_exact_tokens_for_tokens | swap_tokens_for_exact_tokens |
|---------|------------------------------|------------------------------|
| Sabit | Girdi miktarı (amount_in) | Çıktı miktarı (amount_out) |
| Path traversal | İleri → | Geriye ← |
| Math | get_amount_out() | get_amount_in() |
| Slippage kontrolü | amount_out_min | amount_in_max |
| Dönen değer | amount_out | amount_in |
| Rounding | Truncating (kullanıcı aleyhine) | Ceiling (+1, kullanıcı aleyhine) |

---

## 9. Pool Oluşturma

**lib.rs:726-788 (do_create_pool)**

### Adımlar:
1. `asset1 != asset2` kontrolü (lib.rs:735)
2. Per-pool fee varsa `fee <= MaxSwapFee` kontrolü (lib.rs:736-738)
3. `PoolLocator::pool_id()` ile pool_id hesapla
4. Pool zaten var mı? → `PoolExists` hatası (lib.rs:743)
5. **PoolSetupFee ödenir**: `withdraw(PoolSetupFeeAsset, creator, PoolSetupFee, keep_alive=true)` (lib.rs:749-751)
6. Pool account için asset account'lar oluşturulur (touch) — creator deposit ödeyebilir (lib.rs:753-759)
7. LP token ID alınır ve inkrementlenir (lib.rs:761-765)
8. LP token asset yaratılır: `PoolAssets::create(lp_token, pool_account, false, 1u32.into())` (lib.rs:767)
9. `Pools::insert` ile kayıt (lib.rs:773)
10. Per-pool fee varsa `PoolFees::insert` (lib.rs:782-785)

### Sabitler:
- `PoolSetupFee`: Yapılandırılabilir, mock'da `100` (mock.rs:153)
- `PoolSetupFeeAsset`: Hangi varlıkla ödeneceği — mock'da `Native` (mock.rs:154)
- Setup fee tamamen `PoolSetupFeeTarget::on_unbalanced()` ile işlenir (yakılabilir veya hazineye)

### Genesis pool oluşturma (setup_pool_from_genesis) — lib.rs:662-724:
- Setup fee alınmaz
- İlk likidite otomatik eklenir (miktarlar > 0 ise)

---

## 10. Sabitler ve Magic Numberlar

| Sabit | Değer (Mock) | Kaynak | Açıklama |
|-------|-------------|--------|----------|
| `MintMinLiquidity` | 100 | mock.rs:163 | İlk LP'de pool_account'a kilitlenen miktar |
| `PoolSetupFee` | 100 | mock.rs:153 | Pool oluşturma ücreti (ED'e eşit veya büyük olmalı) |
| `MaxSwapPathLength` | 4 | mock.rs:162 | Max hop sayısı (>1 olmalı — integrity_test) |
| `LPFee` | Permill::from_rational(3, 1000) = %0.3 | mock.rs:127 | Default LP fee |
| `MaxSwapFee` | Permill::from_percent(10) = %10 | mock.rs:128 | Max izin verilen fee |
| `LiquidityWithdrawalFee` | Permill::from_percent(0) = %0 | mock.rs:129 | LP çekim cezası |
| `PalletId` | `b"py/ascon"` | mock.rs:125 | Pool AccountId türetmek için seed |
| `Permill::ACCURACY` | 1_000_000 | sp_arithmetic | Fee hesaplarında baz oran |
| LP min issuance | `1u32.into()` | lib.rs:691,767 | LP asset yaratırken min_balance |
| Existential deposit (Native) | 100 | mock.rs:67 | `ConstU128<100>` |
| Existential deposit (Assets) | 1 | mock.rs:81 | `ConstU128<1>` |

### Integrity Test Koşulları — lib.rs:421-430:
```rust
assert!(T::MaxSwapPathLength::get() > 1, "MaxSwapPathLength should be greater than 1");
assert!(T::MaxSwapFee::get() < Permill::one(), "MaxSwapFee should be less than 100%");
```

### AccountId Türetme (AccountIdConverter) — types.rs:147-158:
```rust
// PoolId → AccountId
blake2_256(encode(PalletId, pool_id)) → TrailingZeroInput → Decode → AccountId
```
Seed: `(PalletId, pool_id)` encode edilip hash'lenir.

---

## 11. Event ve Error Listesi

### Events (lib.rs:265-360)
| Event | Tetiklenme |
|-------|-----------|
| `PoolCreated { creator, pool_id, pool_account, lp_token }` | create_pool() başarı |
| `PoolFeeSet { pool_id, fee }` | create_pool_with_fee() veya set_pool_fee() |
| `LiquidityAdded { who, mint_to, pool_id, amount1_provided, amount2_provided, lp_token, lp_token_minted }` | add_liquidity() başarı |
| `LiquidityRemoved { who, withdraw_to, pool_id, amount1, amount2, lp_token, lp_token_burned, withdrawal_fee }` | remove_liquidity() başarı |
| `SwapExecuted { who, send_to, amount_in, amount_out, path }` | Her iki swap başarı |
| `SwapCreditExecuted { amount_in, amount_out, path }` | Credit-based swap başarı |
| `Touched { pool_id, who }` | touch() çağrısı |

### Errors (lib.rs:362-417)
| Error | Açıklama |
|-------|---------|
| `InvalidAssetPair` | Desteklenmeyen asset çifti |
| `PoolExists` | Pool zaten var |
| `WrongDesiredAmount` | amount_desired = 0 |
| `AmountOneLessThanMinimal` | reserve + amount < ED |
| `AmountTwoLessThanMinimal` | reserve + amount < ED |
| `ReserveLeftLessThanMinimal` | Çekimden sonra reserve < ED |
| `AmountOutTooHigh` | amount_out >= reserve_out |
| `PoolNotFound` | Pool yok |
| `Overflow` | Aritmetik taşma |
| `AssetOneDepositDidNotMeetMinimum` | Deposit < amount_min |
| `AssetTwoDepositDidNotMeetMinimum` | Deposit < amount_min |
| `AssetOneWithdrawalDidNotMeetMinimum` | Çekim < amount_min_receive |
| `AssetTwoWithdrawalDidNotMeetMinimum` | Çekim < amount_min_receive |
| `OptimalAmountLessThanDesired` | Optimal < desired (slippage) |
| `InsufficientLiquidityMinted` | LP mint ≤ MintMinLiquidity |
| `ZeroLiquidity` | LP burn = 0 |
| `ZeroAmount` | amount = 0 |
| `ProvidedMinimumNotSufficientForSwap` | amount_out < amount_out_min |
| `ProvidedMaximumNotSufficientForSwap` | amount_in > amount_in_max |
| `InvalidPath` | Path uzunluğu <2 veya >max |
| `NonUniquePath` | Aynı pool iki kez kullanıldı |
| `IncorrectPoolAssetId` | LP token ID alınamadı |
| `BelowMinimum` | Hedef hesap ED altında kalır |
| `PoolEmpty` | Pool var ama reserve sıfır |
| `FeeTooHigh` | fee > MaxSwapFee |

### ops/src/lib.rs Errors (ops/src/lib.rs:132-141)
| Error | Açıklama |
|-------|---------|
| `InvalidAssetPair` | Geçersiz asset çifti |
| `PoolNotFound` | Pool yok |
| `ZeroBalance` | Migrate edilecek bakiye sıfır |
| `PartialTransfer` | Transfer kısmen gerçekleşti |

---

## 12. Config Trait

**lib.rs:114-211**

```rust
pub trait Config: frame_system::Config {
    type RuntimeEvent;
    type Balance: Balance;
    type HigherPrecisionBalance: IntegerSquareRoot + One + Ensure + Unsigned
        + From<u32> + From<Self::Balance> + TryInto<Self::Balance>;
    type AssetKind: Parameter + MaxEncodedLen + MaybeSerializeDeserialize;
    type Assets: Inspect<AccountId, AssetId=AssetKind, Balance=Balance>
        + Mutate<AccountId>
        + AccountTouch<AssetKind, AccountId, Balance=Balance>
        + Balanced<AccountId>
        + Refund<AccountId, AssetId=AssetKind>;
    type PoolId: Parameter + MaxEncodedLen + Ord;
    type PoolLocator: PoolLocator<AccountId, AssetKind, PoolId>;
    type PoolAssetId: AssetId + PartialOrd + Incrementable + From<u32>;
    type PoolAssets: Inspect<AccountId, AssetId=PoolAssetId, Balance=Balance>
        + Create<AccountId>
        + Mutate<AccountId>
        + AccountTouch<PoolAssetId, AccountId, Balance=Balance>
        + Refund<AccountId, AssetId=PoolAssetId>;
    #[pallet::constant] type LPFee: Get<Permill>;
    type AdminOrigin: EnsureOrigin<RuntimeOrigin>;
    #[pallet::constant] type MaxSwapFee: Get<Permill>;
    #[pallet::constant] type PoolSetupFee: Get<Balance>;
    #[pallet::constant] type PoolSetupFeeAsset: Get<AssetKind>;
    type PoolSetupFeeTarget: OnUnbalanced<CreditOf<Self>>;
    #[pallet::constant] type LiquidityWithdrawalFee: Get<Permill>;
    #[pallet::constant] type MintMinLiquidity: Get<Balance>;
    #[pallet::constant] type MaxSwapPathLength: Get<u32>;
    #[pallet::constant] type PalletId: Get<PalletId>;
    type WeightInfo: WeightInfo;
}
```

### Mock Konfigürasyonu (mock.rs:143-166)
| Config | Değer |
|--------|-------|
| `Balance` | `u128` |
| `HigherPrecisionBalance` | `sp_core::U256` |
| `AssetKind` | `NativeOrWithId<u32>` |
| `Assets` | `UnionOf<Balances, Assets, NativeFromLeft, NativeOrWithId<u32>, u128>` |
| `PoolId` | `(AssetKind, AssetKind)` |
| `PoolLocator` | `Chain<WithFirstAssetLocator, AscendingLocator>` |
| `PoolAssetId` | `u32` |
| `PoolSetupFee` | `100` |
| `MintMinLiquidity` | `100` |
| `MaxSwapPathLength` | `4` |
| `PalletId` | `b"py/ascon"` |

---

## Ekstra: PoolLocator Türleri (types.rs)

### WithFirstAsset — types.rs:72-100
- Her pool'da belirli bir "first asset" zorunludur (örn. Native)
- `pool_id = (FirstAsset, diğer_asset)` — her zaman FirstAsset önce

### Ascending — types.rs:103-126
- Asset'ler Ord sırasına göre sıralanır
- `pool_id = (min(asset1, asset2), max(asset1, asset2))`

### Chain — types.rs:131-144
- İki locator'ı zincirler: önce First'ü dener, başarısız olursa Second
- Mock: `Chain<WithFirstAssetLocator, AscendingLocator>`

### AccountIdConverter — types.rs:147-158
```rust
blake2_256(encode(PalletId, pool_id)) → AccountId
```

### AccountIdConverterNoSeed — types.rs:161-171
```rust
blake2_256(encode(pool_id)) → AccountId
```
Seed olmadan (migrasyonlarda kullanılır).

---

## Ekstra: Precompile (Solidity/EVM Entegrasyonu)

**precompiles/src/lib.rs** — Solidity smart contract'larının DEX'i kullanabilmesi için.

### IAssetConversion Solidity Interface (precompiles/src/lib.rs:50-167):
- `swapExactTokensForTokens(path[], amountIn, amountOutMin, sendTo, keepAlive) → amountOut`
- `swapTokensForExactTokens(path[], amountOut, amountInMax, sendTo, keepAlive) → amountIn`
- `quoteExactTokensForTokens(asset1, asset2, amount, includeFee) → amount`
- `quoteTokensForExactTokens(asset1, asset2, amount, includeFee) → amount`
- `createPool(asset1, asset2)`
- `addLiquidity(asset1, asset2, amount1Desired, amount2Desired, amount1Min, amount2Min, mintTo) → lpTokensMinted`
- `removeLiquidity(asset1, asset2, lpTokenBurn, amount1MinReceive, amount2MinReceive, withdrawTo) → (amount1, amount2)`
- `getReserves(asset1, asset2) → (reserve1, reserve2)`

Asset'ler SCALE-encoded bytes olarak iletilir. `AssetConversion<ADDRESS, Runtime>` yapısı ADDRESS ile sabitlenir (u16 ID, H160 adresinin [16..18] byte'larında).

---

## DEX Uygulama Notu (ZORUNLU)

### Nova Shield Chain'e Entegrasyon için Kritik Bulgular

#### 1. AMM Seçimi ve Formül Uyarlaması
Bu pallet **pure Uniswap V2** constant-product AMM'dir. `x*y=k` değişmezi korunur. Nova Shield Chain kendi DEX'ini tasarlarken bu formülleri doğrudan kullanabilir:

```
// get_amount_out (Rust pseudocode)
let fee_complement = 1_000_000u128 - fee_ppm; // ppm = parts per million
let amount_in_with_fee = amount_in * fee_complement;
let numerator = amount_in_with_fee * reserve_out;
let denominator = reserve_in * 1_000_000 + amount_in_with_fee;
let amount_out = numerator / denominator;

// get_amount_in (Rust pseudocode)
let numerator = reserve_in * amount_out * 1_000_000;
let denominator = (reserve_out - amount_out) * fee_complement;
let amount_in = numerator / denominator + 1; // ceiling!
```

**UYARI**: `get_amount_in` daima `+1` ekler (ceiling division). Nova Shield Chain implementasyonunda da bu şekilde yapılmalı, aksi takdirde pool'da her "exact out" swap'ında küçük bir kayıp oluşur.

#### 2. HigherPrecisionBalance Zorunludur
Fee hesaplarında `amount_in_with_fee * reserve_out` işlemi overflow yapabilir. U256 gibi yüksek hassasiyetli tip zorunludur. `u128` max ~3.4e38 olduğundan büyük rezervlerde `u128*u128` taşar. Nova Shield Chain'de de `U256` veya en az `u128` için `u128` HigherPrecisionBalance ile 64-bit balance kullanılmalı.

#### 3. Minimum Liquidity Lock Mekanizması
İlk likidite eklenirken `sqrt(amount1 * amount2)` LP token mint edilir, bunun `MintMinLiquidity` kadarı **pool_account'a** (kullanıcıya değil) mint edilir ve bu tokenlar **hiçbir zaman geri alınamaz**. Bu "donation attack"ı önler. Nova Shield Chain'de:
- `MintMinLiquidity` değeri dikkatli seçilmeli: çok küçükse donation attack; çok büyükse küçük pool'lar yaratılamaz
- Mock'daki 100 değeri "ana para 12 decimal için yeterli" notu ile seçilmiş

#### 4. Pool Hesap Sistemi
Pool rezervleri ayrı bir "pool account" içinde tutulur — ayrı bir storage mapping'de değil. Bu yaklaşım:
- Herhangi biri doğrudan token transfer edebilir (MANIPÜLASYON AÇIĞI! — pallet bunu yönetir ancak öngörülmeyen akışlar olabilir)
- Nova Shield için dedicated reserve storage daha güvenli olabilir

#### 5. Fee Sistemi — 2 Katman
1. **Global fee**: `Config::LPFee` (Permill)
2. **Per-pool fee**: `PoolFees` storage (AdminOrigin gerektir)

Nova Shield Chain'de benzer 2 katman implementasyonu önerilir. `AdminOrigin` governance-controlled olmalı.

#### 6. LiquidityWithdrawalFee
Çekim cezası tamamen yakılır (burn), LP'lere dağıtılmaz. Bu, "sticky liquidity" mekanizması olarak kullanılabilir. Nova Shield Chain'de bu fee'yi hazineye veya başka bir fonda yönlendirmek istenirse `PoolSetupFeeTarget` benzeri bir trait ile yapılabilir.

#### 7. Multi-hop Swap Path
`MaxSwapPathLength = 4` (2 hop max = 3 asset). Path validation:
- `path.len() >= 2` zorunlu
- Tüm pool'lar unique olmalı (`BTreeSet` kontrolü)
- Aynı pool iki kez kullanılamaz (`NonUniquePath` hatası)

#### 8. Swap İşleminin Atomikliği
`do_swap_*` fonksiyonları "partial storage mutation" uyarısı içeriyor. `#[transactional]` macro zorunlu. `credit_swap` internal fonksiyonu transactional context gerektiriyor — Nova Shield Chain'de aynı pattern uygulanmalı.

#### 9. Genesis Pool Desteği
`GenesisConfig` ile setup fee ödemeden pool oluşturulabiliyor. Hem pool oluşturma hem de başlangıç likidite ekleme tek adımda yapılıyor. Bu özellik ağ başlangıcında önemli.

#### 10. EVM/Solidity Precompile
`precompiles/src/lib.rs` doğrudan Solidity ABI uyumlu bir precompile sunuyor. EVM uyumlu zincirler için bu pattern değerli: SCALE-encoded AssetKind bytes olarak geçiyor. Nova Shield Chain'de EVM uyumu planlanıyorsa bu pattern referans alınmalı.

---

## Kritik Bulgular Özeti

| # | Bulgu | Kaynak |
|---|-------|--------|
| 1 | **AMM tipi**: Uniswap V2 constant-product (x*y=k) | lib.rs:20 |
| 2 | **get_amount_out formülü**: `(amount_in*(A-fee)*reserve_out) / (reserve_in*A + amount_in*(A-fee))` where A=1_000_000 | lib.rs:1388-1419 |
| 3 | **get_amount_in formülü**: `(reserve_in*amount_out*A) / ((reserve_out-amount_out)*(A-fee)) + 1` (ceiling!) | lib.rs:1425-1463 |
| 4 | **İlk LP formülü**: `lp_user = sqrt(amount1 * amount2) - MintMinLiquidity` | lib.rs:1353-1368 |
| 5 | **Sonraki LP formülü**: `lp = min(amount1*supply/reserve1, amount2*supply/reserve2)` | lib.rs:869-871 |
| 6 | **remove_liquidity**: `amount = (lp_burn * (1-withdrawal_fee)) * reserve / total_supply` | lib.rs:916-920 |
| 7 | **Per-pool fee override**: `PoolFees::get(pool_id).unwrap_or(LPFee::get())` — per-pool fee, global'dan önce gelir | lib.rs:1275-1277 |
| 8 | **MintMinLiquidity = 100** mock'da (pool_account'a kilitlenir, asla serbest bırakılmaz) | mock.rs:163 |
| 9 | **Default fee = 0.3%** (`Permill::from_rational(3, 1000)`) | mock.rs:127 |
| 10 | **HigherPrecisionBalance zorunlu**: tüm fee hesaplarında U256 kullanılır, overflow önlemek için | mock.rs:146, lib.rs:1394-1396 |
