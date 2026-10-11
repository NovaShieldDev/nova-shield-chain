# pallet-asset-conversion — DEX Analiz B (Güvenlik & Testler)
**Agent**: 12-B | **Tarih**: 2026-07-22 | **Hassasiyet**: 3X (DEX)

---

## 1. Test Coverage Analizi

### Ana Test Dosyası: `src/tests.rs` (~3100 satır)

#### Pool Locator Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `validate_with_first_asset_pool_id_locator` | WithFirstAsset locator'ın doğru sıralama yapıp yapmadığı | Native her zaman önce gelir; Native/Native ve NonNative/NonNative hata verir |
| `validate_ascending_pool_id_locator` | Ascending locator sıralama kuralları | asset1 < asset2 sırası; aynı asset pair hata verir |
| `validate_native_or_with_id_sorting` | NativeOrWithId sıralama karşılaştırması | Native < WithId(1) < WithId(2) |
| `check_pool_accounts_dont_collide` | 1.000.000 farklı pool ID için account ID çakışmaması | HashSet'te çakışma olmaz |

#### Pool Oluşturma Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `check_max_numbers` | U128::MAX ile quote/get_amount_out/get_amount_in | Overflow olmadan sonuç döner; aşırı durumlar hata verir |
| `can_create_pool` | Normal pool oluşturma + setup fee | Event emit, LP token ID artımı, fee kesimi |
| `create_same_pool_twice_should_fail` | Aynı pool iki kez oluşturma | `PoolExists` hatası; token sırası farklı olsa da |
| `different_pools_should_have_different_lp_tokens` | Farklı poolların farklı LP tokenları | LP token ID'ler eşit değil |

#### Likidite Yönetimi Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `can_add_liquidity` | Normal likidite ekleme; token sırası değişik verilse de | LP minted = 216 (10000*10 sqrt - min) |
| `add_tiny_liquidity_leads_to_insufficient_liquidity_minted_error` | Çok küçük miktar | `AmountOneLessThanMinimal` veya `InsufficientLiquidityMinted` |
| `add_tiny_liquidity_directly_to_pool_address` | Pool hesabına doğrudan token gönderilmişse | Hâlâ likidite eklenebilir |
| `can_remove_liquidity` | Withdrawal fee ile LP token yakma | 10% fee ile orantılı çıkış; pool hesabında min_liquidity kalır |
| `can_not_redeem_more_lp_tokens_than_were_minted` | Sahip olunandan fazla LP token yakma | `FundsUnavailable` token hatası |

#### Price Quote Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `quote_price_returns_none_for_zero_amount` | Sıfır input için quote | `None` döner |
| `quote_price_returns_none_for_zero_output` | Skewed pool'da küçük input | Integer division sıfır üretir → `None` |
| `can_quote_price` | Normal quote; ücret dahil/hariç | fee=0.3%, beklenen değerler doğrulama |
| `quote_price_ignores_unrelated_non_sufficient_assets_in_pool_account` | Pool hesabına alakasız token gönderilmesi | Quote değişmez |
| `quote_price_exact_tokens_for_tokens_matches_execution` | Quote ile gerçek swap tutarlılığı | Quoted amount = actual received |
| `quote_price_tokens_for_exact_tokens_matches_execution` | Benzer; ters yön | Quoted price = actual spent |
| `quote_price_returns_none_when_output_exceeds_pool_withdrawable` | Pool'dan çekilebilir max'ı aşan talep | `None` döner; sınırda `Some` döner |
| `quote_price_returns_none_swap_fails` | AMM formülü sonuç verse de withdraw başarısız olacaksa | `None` döner; gerçek swap de başarısız |

#### Swap Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `can_swap_with_native` | Native ile basit swap | `get_amount_out` formülü ile beklenen çıkış |
| `can_swap_with_realistic_values` | Gerçekçi DOT/USD değerleri (200k DOT, 1M USD) | 10 USD → ~1.994 DOT |
| `can_not_swap_in_pool_with_no_liquidity_added_yet` | Boş pool swapı | `PoolEmpty` hatası |
| `check_no_panic_when_try_swap_close_to_empty_pool` | Neredeyse boş pool | Son reserve'e ulaşmak `AmountOutTooHigh`; ED ihlali `NotExpendable` |
| `swap_should_not_work_if_too_much_slippage` | Slippage limiti aşıldığında | `ProvidedMinimumNotSufficientForSwap` |
| `can_swap_tokens_for_exact_tokens` | Exact output swap | Toplam token miktarı korunmuş |
| `can_swap_tokens_for_exact_tokens_when_not_liquidity_provider` | LP olmayan kullanıcı swap | Balanslar doğru değişmiş |
| `swap_when_existential_deposit_would_cause_reaping_but_keep_alive_set` | keep_alive=true ile ED ihlali | `NotExpendable` |
| `swap_when_existential_deposit_would_cause_reaping_pool_account` | Pool hesabının ED'si ihlal edilecekse | `NotExpendable`; multi-hop da dahil |
| `swap_tokens_for_exact_tokens_should_not_work_if_too_much_slippage` | Max input aşılması | `ProvidedMaximumNotSufficientForSwap` |
| `swap_exact_tokens_for_tokens_in_multi_hops` | 3-token multi-hop (A→B→C) | Her pool doğru güncellendi; tek hop path hata; döngüsel path hata |
| `swap_tokens_for_exact_tokens_in_multi_hops` | Multi-hop ters hesap | Backward hesap `get_amount_in` ile |
| `can_not_swap_same_asset` | A→A swap | `InvalidAssetPair` |
| `cannot_block_pool_creation` | Attacker pool hesabına erken token gönderirse | Kullanıcı hâlâ pool oluşturabilir ve likidite ekleyebilir |
| `swap_transactional` | Multi-hop'ta son adım başarısız → tüm state rollback | `assert_storage_noop!` ile doğrulanmış |
| `swap_credit_returns_change` | SwapCredit değişikliği | (credit_out, credit_change) tuple |
| `swap_credit_insufficient_amount_bounds` | Yetersiz credit ile swap | Hata + credit_in geri döner |
| `swap_credit_zero_amount` | Sıfır credit veya sıfır amount_out_min | `ZeroAmount` hatası |
| `swap_credit_invalid_path` | credit_in.asset != path[0] | `InvalidPath` hatası |

#### Genesis Pool Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `genesis_pool_with_liquidity` | Genesis'te likidite ile pool | Balanslar doğru; setup fee yok |
| `genesis_pool_without_liquidity` | Genesis'te boş pool | Balanslar değişmedi |
| `genesis_duplicate_pool_panics` | Aynı pool iki kez genesis | `panic!("PoolExists")` |
| `genesis_same_assets_panics` | Aynı asset çifti genesis | `panic!("InvalidAssetPair")` |

#### Per-Pool Fee Testleri
| Test | Açıklama | Başarı Koşulu |
|------|----------|---------------|
| `pool_fee_falls_back_to_global_lp_fee` | Override yoksa global LPFee kullanılır | `PoolFees` → None, fee = global |
| `create_pool_with_fee_sets_override` | AdminOrigin ile fee override | BadOrigin; FeeTooHigh; doğru fee set |
| `set_pool_fee_requires_admin_origin_and_overrides` | set_pool_fee AdminOrigin gerektirir | BadOrigin; MaxSwapFee aşımı; başarı |
| `set_pool_fee_on_missing_pool_fails` | Var olmayan pool için fee | `PoolNotFound` |
| `per_pool_fee_is_applied_to_swaps` | Pool-specific fee gerçek swap'a yansır | 10% fee → 0.3%'den az çıkış |

### Ops Test Dosyası: `ops/src/tests.rs`
| Test | Açıklama |
|------|----------|
| `migrate_pool_account_id_with_native` | Native içeren pool migration; total issuance değişmez |
| `migrate_pool_account_id_with_insufficient_assets` | Insufficient asset tipi; migration başarılı |
| `migrate_pool_account_id_with_sufficient_assets` | Sufficient asset tipi; deposit yönetimi |

---

## 2. Mock Kurulumu

### Dosya: `src/mock.rs`

```rust
// Runtime bileşenleri
System: frame_system
Balances: pallet_balances       // Native token
Assets: pallet_assets<Instance1> // Normal token'lar
PoolAssets: pallet_assets<Instance2> // LP token'lar
AssetConversion: pallet_asset_conversion
```

**Kritik Konfigürasyon Değerleri (mock.rs):**
- `ExistentialDeposit = ConstU128<100>` — Balances için (satır 67)
- `AssetDeposit = ConstU128<1>` — Instance1 için (satır 81)
- `AssetAccountDeposit = ConstU128<10>` — Instance1 için (satır 82)
- `AssetDeposit = ConstU128<0>` — Instance2 (PoolAssets) için (satır 108) — sıfır depozit
- `LpFee = Permill::from_rational(3u32, 1_000u32)` → **0.3%** (satır 127)
- `MaxSwapFee = Permill::from_percent(10)` — %10 üst sınır (satır 128)
- `LiquidityWithdrawalFee = Permill::from_percent(0)` — varsayılan sıfır; testlerde değiştirilebilir `storage` anahtar kelimesiyle (satır 129)
- `PoolSetupFee = ConstU128<100>` — pool oluşturma ücr. (satır 153)
- `MaxSwapPathLength = ConstU32<4>` — max 4-asset path (satır 162)
- `MintMinLiquidity = ConstU128<100>` — min LP mint miktarı (satır 163)
- `HigherPrecisionBalance = sp_core::U256` — overflow koruması için (satır 146)

**Test Hesapları (new_test_ext):**
```rust
balances: vec![(1, 10000), (2, 20000), (3, 30000), (4, 40000)]
```

**Pool Account ID Türetimi:**
```rust
pub type PoolIdToAccountId = AccountIdConverter<AssetConversionPalletId, (NativeOrWithId<u32>, NativeOrWithId<u32>)>;
// Blake2_256(PalletId || pool_id) → AccountId
```

**Pool Locator Zinciri:**
```rust
type PoolLocator = Chain<WithFirstAssetLocator, AscendingLocator>;
// Önce Native içeren pair; başarısız olursa ascending sıra
```

---

## 3. Benchmark Analizi

### Dosya: `src/benchmarking.rs` ve `src/weights.rs`

| Extrinsic | Weight (picosecond) | DB Reads | DB Writes | Parametre |
|-----------|---------------------|----------|-----------|-----------|
| `create_pool` | 105_415_000 | 9 | 5 | - |
| `create_pool_with_fee` | 108_930_000 | 9 | 6 | - |
| `set_pool_fee` | 17_732_000 | 1 | 1 | - |
| `add_liquidity` | 164_075_000 | 11 | 10 | - |
| `remove_liquidity` | 145_676_000 | 9 | 8 | - |
| `swap_exact_tokens_for_tokens` | 103_421_000 + 13_428_385*n | 7+n | 3*n | n=[2,4] |
| `swap_tokens_for_exact_tokens` | 103_507_000 + 13_470_638*n | 7+n | 3*n | n=[2,4] |
| `touch` | 57_344_527 + 21_340_234*n | 8 | 2*n | n=[0,3] |
| `get_reserves` | 25_001_000 | 4 | 0 | - |

**Dikkat Noktaları:**
- Swap ağırlığı path uzunluğuna (`n`) doğrusal bağlı — `Linear<2, MaxSwapPathLength>` benchmark
- `touch` extrinsic, weight `3` ile çağrılır ama actual refund sayısına göre sonuç düzenlenir (`DispatchResultWithPostInfo`)
- Benchmark helper `NativeOrWithIdFactory`: çift sayı seed → Native önce; tek sayı → Native ikinci

### `valid_liquidity_amount` Fonksiyonu:
```rust
let l = ed1.max(ed2) + T::MintMinLiquidity::get() + T::MintMinLiquidity::get() + T::Balance::one();
```
İki kez `MintMinLiquidity` eklenir → ilk likidite testi için yeterli LP üretimi garanti edilir.

---

## 4. Güvenlik Analizi

### 4.1 Overflow/Underflow

**Tüm kritik hesaplamalar `HigherPrecisionBalance` (U256) üzerinden yapılır:**

`get_amount_out` (lib.rs:1388-1419):
```rust
// Tüm aritmetik HigherPrecisionBalance'a yükseltilir
let amount_in_with_fee = amount_in.checked_mul(&fee_complement).ok_or(Error::Overflow)?;
let numerator = amount_in_with_fee.checked_mul(&reserve_out).ok_or(Error::Overflow)?;
let denominator = reserve_in.checked_mul(&ACCURACY).ok_or(Error::Overflow)?.checked_add(&amount_in_with_fee).ok_or(Error::Overflow)?;
let result = numerator.checked_div(&denominator).ok_or(Error::Overflow)?;
result.try_into().map_err(|_| Error::Overflow)  // U256 → Balance dönüşüm overflow kontrolü
```

`get_amount_in` (lib.rs:1425-1463):
```rust
let numerator = reserve_in.checked_mul(&amount_out)?.checked_mul(&ACCURACY)?;
let denominator = reserve_out.checked_sub(&amount_out)?.checked_mul(&fee_complement)?;
let result = (numerator / denominator).checked_add(&One::one())?;  // +1 yukarı yuvarlama
```

`calc_lp_amount_for_zero_supply` (lib.rs:1353-1368):
```rust
// U256 ile checked_mul → integer_sqrt → checked_sub(MintMinLiquidity) → try_into Balance
let result = amount1.checked_mul(&amount2).ok_or(Overflow)?.integer_sqrt().checked_sub(MintMinLiquidity).ok_or(InsufficientLiquidityMinted)?;
```

`mul_div` (lib.rs:1370-1382):
```rust
// a*b/c — her adım checked
a.checked_mul(&b)?.checked_div(&c)?.try_into()?
```

`withdrawal_fee_amount` (lib.rs:916):
```rust
let withdrawal_fee_amount = T::LiquidityWithdrawalFee::get() * lp_token_burn; // Permill çarpımı — dahili saturation
let lp_redeem_amount = lp_token_burn.saturating_sub(withdrawal_fee_amount);  // saturating
```

**Sonuç:** Kritik yollarda ham aritmetik yok. Tüm checked. U256 kullanımı yeterli headroom sağlar.

**Tek Dikkat Noktası:** `balance_path_from_amount_out`/`balance_path_from_amount_in` içinde her adım `get_amount_in`/`get_amount_out` çağrısı yapılır — multi-hop'ta ardışık U256 dönüşümleri. Her birinde `try_into` overflow kontrolü mevcut.

### 4.2 Reentrancy

Polkadot Substrate mimarisinde reentrancy riski WASM runtime seviyesinde bulunmaz (tek-thread, deterministic execution). Ancak:

**Swap fonksiyonları `#[transactional]` ile sarılmıştır:**
- `Swap::swap_exact_tokens_for_tokens` (swap.rs:155) — `#[transactional]`
- `Swap::swap_tokens_for_exact_tokens` (swap.rs:174) — `#[transactional]`
- `SwapCredit::swap_exact_tokens_for_tokens` (swap.rs:203) — `with_transaction`
- `SwapCredit::swap_tokens_for_exact_tokens` (swap.rs:222) — `with_transaction`
- `MutateLiquidity` trait impl'ları (liquidity.rs:90, 99, 118) — hepsi `#[transactional]`

**İç `do_swap_*` fonksiyonları için uyarı komutu:**
```
WARNING: This may return an error after a partial storage mutation.
It should be used only inside a transactional storage context.
```
(lib.rs:978, 1025) — Bu design intentional; callerın transactional wrapper sağlaması zorunlu.

`swap_transactional` testi (tests.rs:2393) bu davranışı `assert_storage_noop!` ile doğrular.

### 4.3 Slippage Koruması

**`swap_exact_tokens_for_tokens` (extrinsic, lib.rs:527):**
- `amount_out_min: T::Balance` — sıfır değer girilirse `ZeroAmount` hatası
- Hesaplanan `amount_out >= amount_out_min` kontrolü (lib.rs:997-1001)
- Hata: `ProvidedMinimumNotSufficientForSwap`

**`swap_tokens_for_exact_tokens` (extrinsic, lib.rs:555):**
- `amount_in_max: T::Balance` — sıfır değer girilirse `ZeroAmount` hatası
- `amount_in <= amount_in_max` kontrolü (lib.rs:1046-1050)
- Hata: `ProvidedMaximumNotSufficientForSwap`

**`Swap` trait metodları (swap.rs:43-69):**
- `amount_out_min: Option<T::Balance>` — None ise minimum kontrol yok (dikkat!)
- `amount_in_max: Option<T::Balance>` — None ise maksimum kontrol yok (dikkat!)

**`add_liquidity`:**
- `amount1_min`, `amount2_min` — min deposit miktarı kontrolü
- `AssetOneDepositDidNotMeetMinimum`, `AssetTwoDepositDidNotMeetMinimum` hataları

**`remove_liquidity`:**
- `amount1_min_receive`, `amount2_min_receive` — min çıkış kontrolü
- `AssetOneWithdrawalDidNotMeetMinimum`, `AssetTwoWithdrawalDidNotMeetMinimum` hataları

**Deadline parametresi YOK.** Polkadot'ta transaction validity kontrolü block-level yapılır; ayrıca deadline mekanizması eklenmemiş.

### 4.4 Pool Drain / Manipulation

**Pool Drain Koruması:**
1. `amount_out >= reserve_out` kontrolü `get_amount_in`'de (lib.rs:1439-1441) — `AmountOutTooHigh`
2. `get_reserves` fonksiyonu reserve sıfırsa `PoolEmpty` döner (lib.rs:1509-1511)
3. `do_remove_liquidity` içinde:
   ```rust
   ensure!(reserve1_left >= T::Assets::minimum_balance(asset1.clone()), Error::ReserveLeftLessThanMinimal);
   ensure!(reserve2_left >= T::Assets::minimum_balance(asset2.clone()), Error::ReserveLeftLessThanMinimal);
   ```
   (lib.rs:932-939) — ED altına inmez
4. `calc_lp_amount_for_zero_supply` — initial LP miktarı, `MintMinLiquidity` pool hesabına kilitlenir (lib.rs:863-867) — bu likidite çekilemez, pool'u boşaltmayı engeller
5. `swap` fonksiyonunda `withdraw` ile `Preserve` kullanılır (lib.rs:1217) — pool hesabında ED korunur

**Frontrunning:** Deadline yoktur. `amount_out_min`/`amount_in_max` parametreleri slippage koruması sağlar ama zaman bazlı koruma yok. Substrate block production modeli nedeniyle klasik mempool frontrunning daha az etkilidir.

**Pool Hesabı Kirlenmesi Saldırısı (cannot_block_pool_creation testi):**
- Saldırgan pool hesabına önceden token gönderebilir
- Pool oluşturma bu durumda başarılı olur (test satır 2365)
- `add_liquidity`'de `reserve > 0` ise formüle girer ama initial liquidity için sıfır reserve kontrolü ayrı — test bu senaryoyu doğrular

**Sandwich Attack:** Tek blokta sandwich — blok üreticisi yapabilir. Kullanıcı kontrolü: `amount_out_min`.

---

## 5. Edge Case'ler

### 5.1 Sıfır Liquidity
- `get_reserves` → `PoolEmpty` (lib.rs:1509)
- `get_amount_out` / `get_amount_in` → `ZeroLiquidity` (lib.rs:1398-1400, 1435-1437)
- Boş pool'da swap → test `can_not_swap_in_pool_with_no_liquidity_added_yet` doğrular

### 5.2 Minimum Miktarlar
- `amount_out >= reserve_out` → `AmountOutTooHigh` (lib.rs:1439)
- `check_max_numbers` testi: `quote(MAX, MAX, MAX)` → overflow yok
- `quote(MAX, 3, MAX)` → overflow hatası (satır 158)

### 5.3 Integer Division Sıfır Çıkışı
- Çok küçük input skewed pool'da → `get_amount_out` sıfır döner
- `quote_price_*` fonksiyonları `amount_out.is_zero()` kontrolü yapar → `None` (lib.rs:1550-1552)
- Test: `quote_price_returns_none_for_zero_output` (tests.rs:700)

### 5.4 Asymmetric Pool (Tek Taraflı Likidite)
- `reserve1.is_zero() || reserve2.is_zero()` → initial liquidity branch (lib.rs:818)
- Sonraki ekleme optimal miktarla yapılır

### 5.5 LP Token Overflow (NextPoolAssetId)
- `lp_token.increment().ok_or(Error::IncorrectPoolAssetId)` (lib.rs:687) — tükenirse hata

### 5.6 withdraw() Fonksiyonu Workaround
```rust
// TODO drop the ensure! when this issue addressed
// https://github.com/paritytech/polkadot-sdk/issues/1698
let free = T::Assets::reducible_balance(asset, who, preservation, Polite);
ensure!(free >= value, TokenError::NotExpendable);
```
(lib.rs:1257-1261) — Manuel pre-check; SDK bug'ı için geçici çözüm. Teorik TOCTOU riski var.

### 5.7 Pool Oluşturmada keep_alive Davranışı
`do_create_pool` içinde `withdraw` çağrısı `keep_alive=true` (lib.rs:750):
```rust
let fee = Self::withdraw(T::PoolSetupFeeAsset::get(), creator, T::PoolSetupFee::get(), true)?;
```

### 5.8 Withdrawal Fee Hesabı
```rust
let withdrawal_fee_amount = T::LiquidityWithdrawalFee::get() * lp_token_burn;
let lp_redeem_amount = lp_token_burn.saturating_sub(withdrawal_fee_amount);
```
(lib.rs:916-917) — Fee, yakılan LP'den düşülür. Gerçek çekilecek miktar `lp_redeem_amount` üzerinden hesaplanır. Fee miktarı pool'da kalır (yakılmaz, transfer edilmez).

### 5.9 Genesis Pool Bypass
- `setup_pool_from_genesis` setup fee almaz (lib.rs:667)
- Bu konfigürasyona bağlıdır; genesis block'ta ekonomik teşvik mekanizması çalışmaz

---

## 6. Trait İmplementasyonları

### `Swap<AccountId>` (swap.rs:147-192)
```rust
fn swap_exact_tokens_for_tokens(
    sender: AccountId, path: Vec<AssetKind>, amount_in: Balance,
    amount_out_min: Option<Balance>, send_to: AccountId, keep_alive: bool,
) -> Result<Balance, DispatchError>

fn swap_tokens_for_exact_tokens(
    sender: AccountId, path: Vec<AssetKind>, amount_out: Balance,
    amount_in_max: Option<Balance>, send_to: AccountId, keep_alive: bool,
) -> Result<Balance, DispatchError>
```
Her ikisi de `#[transactional]` ile sarılmış.

### `SwapCredit<AccountId>` (swap.rs:194-240)
```rust
type Credit = CreditOf<T>; // = Credit<AccountId, Assets>

fn swap_exact_tokens_for_tokens(
    path: Vec<AssetKind>, credit_in: Credit, amount_out_min: Option<Balance>,
) -> Result<Credit, (Credit, DispatchError)>  // Hata durumunda credit_in geri döner

fn swap_tokens_for_exact_tokens(
    path: Vec<AssetKind>, credit_in: Credit, amount_out: Balance,
) -> Result<(Credit, Credit), (Credit, DispatchError)>  // (credit_out, credit_change)
```
`with_transaction` içinde çalışır. Rollback durumunda credit_in geri döner.

### `QuotePrice` (swap.rs:242-261)
```rust
fn quote_price_exact_tokens_for_tokens(asset1, asset2, amount, include_fee) -> Option<Balance>
fn quote_price_tokens_for_exact_tokens(asset1, asset2, amount, include_fee) -> Option<Balance>
```
Read-only; state değiştirmez.

### `MutateLiquidity<AccountId>` (liquidity.rs:37-138)
```rust
fn create_pool(creator: &AccountId, asset1, asset2) -> Result<PoolId, DispatchError>
fn add_liquidity(who: &AccountId, asset1: AddLiquidityAsset, asset2: AddLiquidityAsset, mint_to: &AccountId) -> Result<Balance, DispatchError>
fn remove_liquidity(who: &AccountId, asset1, asset2, lp_token_burn, amount1_min, amount2_min, withdraw_to: &AccountId) -> Result<(Balance, Balance), DispatchError>
```
Hepsi `#[transactional]`.

### `PoolLocator<AccountId, AssetKind, PoolId>` (types.rs:49-66)
```rust
fn address(id: &PoolId) -> Result<AccountId, ()>
fn pool_id(asset1, asset2) -> Result<PoolId, ()>
fn pool_address(asset1, asset2) -> Result<AccountId, ()>  // default impl
```

**Implementasyonlar:**
- `WithFirstAsset`: FirstAsset her zaman ilk; diğer asset ikinci
- `Ascending`: Küçük asset önce (Ord)
- `Chain<First, Second>`: First başarısız → Second dene
- `AccountIdConverter`: `blake2_256(PalletId || pool_id)` → AccountId
- `AccountIdConverterNoSeed`: `blake2_256(pool_id)` → AccountId (seed olmadan)

---

## 7. Extrinsic Listesi ve Parametreleri

| Call Index | Extrinsic | Parametreler | Kısıtlamalar |
|------------|-----------|-------------|--------------|
| 0 | `create_pool` | asset1, asset2 (Box) | asset1 ≠ asset2; pool yok; setup fee ödenir |
| 1 | `add_liquidity` | asset1, asset2, amount1_desired, amount2_desired, amount1_min, amount2_min, mint_to | amount > 0; pool var; optimal hesap |
| 2 | `remove_liquidity` | asset1, asset2, lp_token_burn, amount1_min, amount2_min, withdraw_to | lp_burn > 0; ED kontrolü |
| 3 | `swap_exact_tokens_for_tokens` | path (Vec<Box>), amount_in, amount_out_min, send_to, keep_alive | amount_in > 0; amount_out_min > 0; path ≥ 2; max path length |
| 4 | `swap_tokens_for_exact_tokens` | path (Vec<Box>), amount_out, amount_in_max, send_to, keep_alive | amount_out > 0; amount_in_max > 0; path ≥ 2 |
| 5 | `touch` | asset1, asset2 | Pool var; touch gerekli olmalı |
| 6 | `create_pool_with_fee` | creator, asset1, asset2, fee | AdminOrigin; fee ≤ MaxSwapFee |
| 7 | `set_pool_fee` | pool_id, fee | AdminOrigin; pool var; fee ≤ MaxSwapFee |

**Weight parametrizasyonu:**
- Swap extrinsics: `path.len() as u32` ile dinamik weight
- `touch`: `3` ile çağrılır, actual refund sayısına göre indirim

---

## 8. Integer Precision Sorunları

### 8.1 `get_amount_out` Yuvarlama
```rust
// Sonuç: floor(amount_in * (1-fee) * reserve_out / (reserve_in * ACCURACY + amount_in * (1-fee)))
// Integer division floor → kullanıcı beklediğinden az alır
// Bu Uniswap V2 standart davranışıdır (LP lehine yuvarlama)
```

### 8.2 `get_amount_in` Yuvarlama
```rust
// +1 ile yukarı yuvarlama
let result = (numerator / denominator).checked_add(&One::one())?;
// Kullanıcı beklediğinden biraz fazla gönderir — LP lehine
```

### 8.3 `mul_div(a, b, c)` = `a * b / c`
- Intermediate `a * b` çarpımı U256'da — overflow riski minimumdur
- Final `/c` integer division floor
- LP token hesabında kullanılır: `min(amount1 * total_supply / reserve1, amount2 * total_supply / reserve2)`
- `min` alınması küçük tarafı seçer → LP lehine (küçük taraftaki likidite sağlayıcı az LP alır)

### 8.4 Fee Hesabı
```rust
let fee_complement = fee.left_from_one().deconstruct(); // ACCURACY - fee
// Örnek: 0.3% için → (1_000_000 - 3_000) = 997_000
// Permill::ACCURACY = 1_000_000
```
`left_from_one()` çağrısı — tam tersini alır. 997_000 değeri hardcoded değil, dinamik.

### 8.5 `calc_lp_amount_for_zero_supply` Precision
```rust
let result = sqrt(amount1 * amount2) - MintMinLiquidity;
// sqrt floor alır
// Büyük amount1/amount2 farkı varsa sqrt'ın precision kaybı olabilir
// Örnek: amount1=1, amount2=1e18 → sqrt ≈ 1e9; MintMinLiquidity=100 → net=1e9-100
```

### 8.6 Quote vs Execution Uyumsuzluğu Riski
`quote_price_*` read-only, `get_reserves` anlık değer kullanır. Bloklar arası pool değişirse quote ile execution arasında fark oluşur. Testlerde aynı blokta çalıştırılarak doğrulanmış — production'da kullanıcı aynı tx'te swap yapmalı.

---

## 9. Hata Yönetimi

| Error Varyantı | Tetiklenme Koşulu | Kaynak |
|----------------|------------------|--------|
| `InvalidAssetPair` | asset1 == asset2; pool_id türetme başarısız | lib.rs:735, 801 |
| `PoolExists` | Aynı pool iki kez | lib.rs:743 |
| `WrongDesiredAmount` | amount_desired = 0 | lib.rs:804-807 |
| `AmountOneLessThanMinimal` | reserve1 + amount1 < ED | lib.rs:846-849 |
| `AmountTwoLessThanMinimal` | reserve2 + amount2 < ED | lib.rs:850-853 |
| `ReserveLeftLessThanMinimal` | Kaldıktan sonra ED altı | lib.rs:932-939 |
| `AmountOutTooHigh` | amount_out >= reserve_out | lib.rs:1439-1441 |
| `PoolNotFound` | Pool storage'da yok | lib.rs:809 |
| `Overflow` | Checked arithmetic başarısız | lib.rs:1362, 1377, 1405 |
| `AssetOneDepositDidNotMeetMinimum` | amount1_optimal < amount1_min | lib.rs:838 |
| `AssetTwoDepositDidNotMeetMinimum` | amount2_optimal < amount2_min | lib.rs:826-828 |
| `AssetOneWithdrawalDidNotMeetMinimum` | Çıkış miktarı < min; veya sıfır | lib.rs:922-925 |
| `AssetTwoWithdrawalDidNotMeetMinimum` | Çıkış miktarı < min; veya sıfır | lib.rs:926-929 |
| `OptimalAmountLessThanDesired` | Hesaplanan optimal < desired | lib.rs:834-836 |
| `InsufficientLiquidityMinted` | LP mint < MintMinLiquidity | lib.rs:874-877; 1364 |
| `ZeroLiquidity` | lp_token_burn = 0; reserve sıfır | lib.rs:907; 1399 |
| `ZeroAmount` | amount_in/out = 0 | lib.rs:988-991; 1036-1039 |
| `ProvidedMinimumNotSufficientForSwap` | amount_out < amount_out_min | lib.rs:998-1001 |
| `ProvidedMaximumNotSufficientForSwap` | amount_in > amount_in_max | lib.rs:1046-1049 |
| `InvalidPath` | path.len < 2 veya > max | lib.rs:1467-1468 |
| `NonUniquePath` | Aynı pool iki kez kullanılmış | lib.rs:1478-1480 |
| `IncorrectPoolAssetId` | LP ID overflow veya None | lib.rs:762-764 |
| `BelowMinimum` | Hedef hesap resolve başarısız | lib.rs:1178 |
| `PoolEmpty` | Reserve sıfır | lib.rs:1509-1511 |
| `FeeTooHigh` | fee > MaxSwapFee | lib.rs:653; 737 |

**TokenError'lar (framework level):**
- `TokenError::NotExpendable` — keep_alive ile ED ihlali
- `TokenError::FundsUnavailable` — yetersiz bakiye (LP yakma)

---

## DEX Uygulama Notu (ZORUNLU)

### Nova Shield Chain İçin Güvenlik Tavsiyeleri

#### A. AMM Formülleri — Doğrulanmış Değerler

**get_amount_out formülü (Uniswap V2 standart):**
```
amount_out = (amount_in * (ACCURACY - fee) * reserve_out) / (reserve_in * ACCURACY + amount_in * (ACCURACY - fee))
```
Kaynak: lib.rs:1388-1419

**get_amount_in formülü:**
```
amount_in = (reserve_in * amount_out * ACCURACY) / ((reserve_out - amount_out) * (ACCURACY - fee)) + 1
```
Kaynak: lib.rs:1425-1463 (`+1` yukarı yuvarlama!)

**fee_complement:** `Permill::left_from_one().deconstruct()` → `ACCURACY - fee_parts` (dinamik, hardcoded değil)

**İlk LP miktarı:** `sqrt(amount1 * amount2) - MintMinLiquidity`
- MintMinLiquidity: kalıcı pool lock'u; boşaltmayı engeller

**LP redemption:**
```
amount_out = (lp_redeem_amount * reserve) / total_supply
lp_redeem_amount = lp_burn - (LiquidityWithdrawalFee * lp_burn)
```

#### B. Güvenlik Öncelik Listesi

1. **U256 HigherPrecisionBalance zorunlu** — `u128 * u128` overflow riskini önler; Nova Shield Chain'de de kullanılmalı
2. **`#[transactional]` her public entry point'te** — Partial state mutation'ı önler
3. **MintMinLiquidity pool'da kilitli tutulmalı** — ilk LP hesabında; pool drain saldırısını engeller
4. **ED kontrolü reserve'de zorunlu** — `ReserveLeftLessThanMinimal` benzeri kontrol kritik
5. **`amount_out_min`/`amount_in_max` her zaman required** — `Option<Balance>` yerine `Balance` ile zorunlu kılmak daha güvenli (Swap trait ikisi de Option kabul ediyor — zayıflık)

#### C. Kritik Test Stratejisi

Nova Shield Chain DEX implementasyonu için şu testler zorunlu:

1. **Max value testleri:** `check_max_numbers` benzeri — tüm U128::MAX kombinasyonları
2. **Pool account çakışma testi:** `check_pool_accounts_dont_collide` — 1M pool ID hash çakışması yok mu
3. **Transactional rollback testi:** `swap_transactional` benzeri — `assert_storage_noop!` ile
4. **ED koruma testi:** `swap_when_existential_deposit_would_cause_reaping_pool_account` — pool'un ED altına düşmesini önle
5. **Skewed pool testi:** `quote_price_returns_none_for_zero_output` — büyük imbalance'da küçük input
6. **Pool drain testi:** `check_no_panic_when_try_swap_close_to_empty_pool` — neredeyse boş pool
7. **Fee precision testi:** `per_pool_fee_is_applied_to_swaps` — farklı fee seviyelerinde doğru hesap
8. **Multi-hop invariant testi:** Token toplam arzı swap sonrası korunmalı

#### D. Potansiyel Riskler

**Risk 1: `withdraw()` TOCTOU (lib.rs:1256-1262)**
```rust
// TODO: SDK issue #1698
let free = T::Assets::reducible_balance(asset, who, Preserve, Polite);
ensure!(free >= value, TokenError::NotExpendable);  // ← manual pre-check
T::Assets::withdraw(asset, who, value, Exact, Preserve, Polite)  // ← actual withdrawal
```
Bu arada başka bir extrinsic çalışamaz (Substrate single-threaded), ama gelecekte async execution gelirse risk var.

**Risk 2: Deadline Yok**
Polkadot ekosisteminde transaction timeout block validity süresiyle yönetilir. DEX için eksik; uzun süreli pending tx'te price drift olabilir.

**Risk 3: `Swap` Trait'te Opsiyonel Slippage**
`Option<amount_out_min>` ve `Option<amount_in_max>` → programmatic kullanıcılar (`None` geçerse) slippage koruması olmadan swap yapabilir.

**Risk 4: Pool Fee Governance**
`AdminOrigin` (Root) fee'yi istediği zaman değiştirebilir. Eski fee ile quote alan kullanıcı farklı fee ile swap yapabilir. Event emit ediliyor ama on-chain timing garantisi yok.

**Risk 5: `cannot_block_pool_creation` — Dikkat Edilmesi Gereken Senaryo**
Saldırgan pool hesabına 14 token göndermiş → oluşturma başarılı olmuş. Ancak bu tokenlar pool'un reserve'ine dahil edilmez — sadece native+target token reserve sayılır. Test bu durumu doğrular. Nova Shield'da da aynı mantık korunmalı.

---

## Kritik Bulgular Özeti

| # | Bulgu | Dosya:Satır | Önem |
|---|-------|-------------|------|
| 1 | `withdraw()` TOCTOU workaround: SDK bug #1698 nedeniyle manuel pre-check var | lib.rs:1256-1262 | ORTA |
| 2 | `Swap` trait `amount_out_min`/`amount_in_max` Optional — slippage koruması atlanabilir | swap.rs:48, 66 | ORTA |
| 3 | `mul_div` ile LP hesabı `min(side1, side2)` → her iki taraf aynı değilse küçük taraf kaybeder | lib.rs:869-871 | DÜŞÜK |
| 4 | `get_amount_in` +1 yukarı yuvarlama — kullanıcı fazla öder, LP lehine | lib.rs:1459-1460 | BİLGİ |
| 5 | `LiquidityWithdrawalFee` fee'nin değeri yakılmaz, pool'da kalır — LP'ler artık daha fazla çekemez; bu bir reward mekanizması değil | lib.rs:916-952 | BİLGİ |
| 6 | `MintMinLiquidity` (100) pool hesabında kalıcı lock → pool drain saldırısı engellenir | lib.rs:863-867 | KRİTİK DOĞRU TASARIM |
| 7 | `check_pool_accounts_dont_collide`: 1M pool için hash çakışması test edilmiş — Blake2_256 güvenliği | tests.rs:137-152 | POZİTİF |
| 8 | `swap_transactional` test: multi-hop rollback `assert_storage_noop!` ile kesinleştirilmiş | tests.rs:2393-2531 | POZİTİF |
| 9 | `cannot_block_pool_creation`: Pool hesabına önceden token gönderilmesi engellenemez ama pool oluşturmayı da engelleyemez | tests.rs:2334-2391 | POZİTİF |
| 10 | Deadline parametresi eksik — uzun süre pending tx'lerde price manipulation riski teorik | lib.rs genel | DÜŞÜK |

---

## Ekstra: Precompile (EVM Interface)

`precompiles/src/lib.rs` — pallet-revive üzerinden Solidity interface:
- Delegate call reddedilir (satır 197-199)
- Read-only modda write işlemleri reddedilir (satır 202-209)
- Path uzunluğu `MaxSwapPathLength` ile sınırlı (satır 265-272)
- U256 → Balance dönüşümü `try_into` ile — overflow güvenli (satır 274-287)
- SCALE-decode başarısız olursa `ERR_INVALID_ASSET_ENCODING` (satır 260-262)
- Weight charging: Her çağrıda önce weight charge edilir (satır 294-298)

Bu precompile Nova Shield Chain'de EVM uyumlu DEX erişimi için model alınabilir.

---

*Tüm değerler kaynak koddan doğrulanmıştır. Satır numaraları polkadot-sdk ana repo'su (2026-07 state) için geçerlidir.*
