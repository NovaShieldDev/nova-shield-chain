# pallet-assets — Analiz A (Core Operations)
**Agent**: 13-A | **Tarih**: 2026-07-22 | **Hassasiyet**: 3X

---

## 1. Dizin Yapısı

### pallet-assets (`substrate/frame/assets/`)
| Dosya | Açıklama |
|---|---|
| `src/lib.rs` | Ana pallet — Config trait, Storage, Call enum (~1966 satır) |
| `src/types.rs` | Core tipler: AssetDetails, AssetAccount, ExistenceReason, TransferFlags vb. (362 satır) |
| `src/functions.rs` | İç implementasyon — do_mint, do_burn, do_transfer, can_increase, can_decrease vb. (1155 satır) |
| `src/impl_fungibles.rs` | fungibles trait implementasyonları (395 satır) |
| `src/extra_mutator.rs` | ExtraMutator — sidecar data mutasyonu (99 satır) |
| `src/impl_stored_map.rs` | StoredMap implementasyonu |
| `src/migration.rs` | Migrasyon kodu |
| `src/weights.rs` | Ağırlık tanımları |
| `src/benchmarking.rs` | Benchmark kodu |
| `src/mock.rs` | Test mock |
| `src/tests.rs` / `src/tests/sets.rs` | Test dosyaları |
| `precompiles/src/lib.rs` | EVM precompile (permit, foreign assets) |

### pallet-assets-freezer (`substrate/frame/assets-freezer/`)
| Dosya | Açıklama |
|---|---|
| `src/lib.rs` | Config, Storage (Freezes, FrozenBalances), Event, Error (185 satır) |
| `src/impls.rs` | FrozenBalance impl, InspectFreeze, MutateFreeze impl (173 satır) |

### pallet-assets-holder (`substrate/frame/assets-holder/`)
| Dosya | Açıklama |
|---|---|
| `src/lib.rs` | Config, Storage (Holds, BalancesOnHold), Event, Error (179 satır) |
| `src/impl_fungibles.rs` | BalanceOnHold impl, InspectHold, MutateHold, UnbalancedHold impl (291 satır) |

---

## 2. Storage Yapıları

### pallet-assets

```
Asset<T, I>: StorageMap<Blake2_128Concat, T::AssetId, AssetDetails<...>>
```
- `lib.rs:438-443`
- Key: `T::AssetId`
- Value: `AssetDetails<T::Balance, T::AccountId, DepositBalanceOf<T,I>>`

```
Account<T, I>: StorageDoubleMap<Blake2_128Concat, T::AssetId, Blake2_128Concat, T::AccountId, AssetAccountOf<T,I>>
```
- `lib.rs:447-454`
- Key1: `T::AssetId`, Key2: `T::AccountId`
- Value: `AssetAccount<Balance, DepositBalance, Extra, AccountId>`

```
Approvals<T, I>: StorageNMap<(AssetId, AccountId_owner, AccountId_delegate), Approval<Balance, DepositBalance>>
```
- `lib.rs:460-468`
- 3-key map: (asset_id, owner, delegate)

```
Metadata<T, I>: StorageMap<Blake2_128Concat, T::AssetId, AssetMetadata<...>, ValueQuery>
```
- `lib.rs:472-478`
- ValueQuery: default döner (empty metadata)

```
Reserves<T, I>: StorageMap<Blake2_128Concat, T::AssetId, BoundedVec<T::ReserveData, ConstU32<MAX_RESERVES>>, ValueQuery>
```
- `lib.rs:482-488`
- MAX_RESERVES = 5 (`lib.rs:264`)

```
NextAssetId<T, I>: StorageValue<_, T::AssetId, OptionQuery>
```
- `lib.rs:500`
- Opsiyonel; ayarlandığında auto-increment veya ID kısıtlaması için kullanılır

### pallet-assets-freezer

```
Freezes<T, I>: StorageDoubleMap<Blake2_128Concat, AssetId, Blake2_128Concat, AccountId,
    BoundedVec<IdAmount<RuntimeFreezeReason, Balance>, VariantCountOf<RuntimeFreezeReason>>, ValueQuery>
```
- `freezer/src/lib.rs:109-120`

```
FrozenBalances<T, I>: StorageDoubleMap<Blake2_128Concat, AssetId, Blake2_128Concat, AccountId, Balance>
```
- `freezer/src/lib.rs:124-131`
- Toplam dondurulmuş bakiye önbelleği (her freeze listesi max değeri)

### pallet-assets-holder

```
Holds<T, I>: StorageDoubleMap<Blake2_128Concat, AssetId, Blake2_128Concat, AccountId,
    BoundedVec<IdAmount<RuntimeHoldReason, Balance>, VariantCountOf<RuntimeHoldReason>>, ValueQuery>
```
- `holder/src/lib.rs:119-130`

```
BalancesOnHold<T, I>: StorageDoubleMap<Blake2_128Concat, AssetId, Blake2_128Concat, AccountId, Balance>
```
- `holder/src/lib.rs:134-141`
- Hold bakiyelerinin toplamı (sum, freeze'deki max'ın aksine)

---

## 3. Core Tipler

### AssetDetails (`types.rs:52-78`)
```rust
pub struct AssetDetails<Balance, AccountId, DepositBalance> {
    pub owner: AccountId,       // asset sahibi (destroy, transfer_ownership, set_team)
    pub issuer: AccountId,      // mint yetkisi
    pub admin: AccountId,       // burn, force_transfer, thaw yetkisi
    pub freezer: AccountId,     // freeze, block yetkisi
    pub supply: Balance,        // toplam arz
    pub deposit: DepositBalance,// asset oluşturma depositi
    pub min_balance: Balance,   // minimum bakiye (ED)
    pub is_sufficient: bool,    // consumer ref gerekmez mi?
    pub accounts: u32,          // toplam hesap sayısı
    pub sufficients: u32,       // sufficient ref count
    pub approvals: u32,         // toplam approval sayısı
    pub status: AssetStatus,    // Live | Frozen | Destroying
}
```

### AssetAccount (`types.rs:176-188`)
```rust
pub struct AssetAccount<Balance, DepositBalance, Extra, AccountId> {
    pub balance: Balance,              // transferable bakiye (hold hariç)
    pub status: AccountStatus,         // Liquid | Frozen | Blocked
    pub reason: ExistenceReason<...>,  // neden var bu hesap?
    pub extra: Extra,                  // sidecar data
}
```

### ExistenceReason (`types.rs:97-115`)
```rust
pub enum ExistenceReason<Balance, AccountId> {
    Consumer,                            // consumer ref tüketti
    Sufficient,                          // asset is_sufficient=true
    DepositHeld(Balance),                // kendi depositi var
    DepositRefunded,                     // deposit geri alındı ama hesap hala var
    DepositFrom(AccountId, Balance),     // başka biri depozit koydu (touch_other)
}
```

### AssetStatus (`types.rs:41-49`)
```rust
pub enum AssetStatus {
    Live,       // normal çalışır
    Frozen,     // transfer yasak (admin yetkilileri hariç)
    Destroying, // yıkım sürecinde, GERİ DÖNÜŞ YOK
}
```

### AccountStatus (`types.rs:156-173`)
```rust
pub enum AccountStatus {
    Liquid,   // normal
    Frozen,   // transferler yasak (gönderemez, sadece alabilir)
    Blocked,  // hem gönderemez hem alamaz
}
// is_frozen() = Frozen | Blocked
// is_blocked() = Blocked only
```

### TransferFlags / DebitFlags (`types.rs:278-305`)
```rust
pub struct TransferFlags {
    pub keep_alive: bool,   // kaynak hesap canlı kalmalı
    pub best_effort: bool,  // kısmi transfer kabul edilir
    pub burn_dust: bool,    // minimum bakiyeden düşen fark yakılır
}
```

---

## 4. Config Trait

**Dosya**: `lib.rs:319-434`

| Associated Type | Açıklama | Kısıt | TestDefault |
|---|---|---|---|
| `RuntimeEvent` | Event tipi | — | () |
| `Balance` | Bakiye tipi | `AtLeast32BitUnsigned + HasCompact + Default + Copy` | `u64` |
| `RemoveItemsLimit` | destroy_accounts/approvals batch limiti | `Get<u32>` | `ConstU32<5>` |
| `AssetId` | Asset kimliği | `Member + Parameter + Clone + MaybeSerializeDeserialize` | `u32` |
| `AssetIdParameter` | Dispatch call için wrapper | `Parameter + From<AssetId> + Into<AssetId>` | `u32` |
| `ReserveData` | Reserve lokasyon bilgisi | `Debug + Parameter + MaybeSerializeDeserialize` | `()` |
| `Currency` | Deposit için currency | `ReservableCurrency<AccountId>` | no_default |
| `CreateOrigin` | Asset oluşturma origin | `EnsureOriginWithArg<RuntimeOrigin, AssetId, Success=AccountId>` | no_default |
| `ForceOrigin` | Zorla işlem origin | `EnsureOrigin<RuntimeOrigin>` | no_default |
| `AssetDeposit` | Asset oluşturma depositi | `Get<DepositBalanceOf<Self,I>>` | `ConstUint<1>` |
| `AssetAccountDeposit` | Hesap dokunma depositi | `Get<DepositBalanceOf<Self,I>>` | `ConstUint<10>` |
| `MetadataDepositBase` | Metadata base depositi | `Get<DepositBalanceOf<Self,I>>` | `ConstUint<1>` |
| `MetadataDepositPerByte` | Metadata byte başına | `Get<DepositBalanceOf<Self,I>>` | `ConstUint<1>` |
| `ApprovalDeposit` | Approval depositi | `Get<DepositBalanceOf<Self,I>>` | `ConstUint<1>` |
| `StringLimit` | Ad/symbol max uzunluk | `Get<u32>` | `ConstU32<50>` |
| `Freezer` | Dondurma hook'u | `FrozenBalance<AssetId, AccountId, Balance>` | `()` |
| `Holder` | Hold hook'u | `BalanceOnHold<AssetId, AccountId, Balance>` | `()` |
| `Extra` | Sidecar data | `Member + Parameter + Default + MaxEncodedLen` | `()` |
| `CallbackHandle` | Created/destroyed callback | `AssetsCallback<AssetId, AccountId>` | `()` |
| `WeightInfo` | Ağırlık | `WeightInfo` | `()` |

**Sabitler** (`#[pallet::constant]`):
- `RemoveItemsLimit` — destroy batch büyüklüğü
- `AssetDeposit` — asset oluşturma zorunlu rezerv
- `AssetAccountDeposit` — non-sufficient hesap dokunma zorunlu rezerv
- `MetadataDepositBase` — metadata base zorunlu rezerv
- `MetadataDepositPerByte` — metadata byte başına zorunlu rezerv
- `ApprovalDeposit` — approve zorunlu rezerv
- `StringLimit` — string max uzunluğu

---

## 5. Admin Rol Sistemi

**Kaynak**: `types.rs:52-78`, `lib.rs`, `functions.rs`

| Rol | Kimde | Yetkisi |
|---|---|---|
| **owner** | `AssetDetails.owner` | transfer_ownership, set_team, set_metadata, clear_metadata, set_min_balance, set_reserves, start_destroy |
| **issuer** | `AssetDetails.issuer` | mint (do_mint check_issuer) |
| **admin** | `AssetDetails.admin` | burn (do_burn check_admin), force_transfer, thaw (hesap), thaw_asset, force_cancel_approval |
| **freezer** | `AssetDetails.freezer` | freeze (hesap), freeze_asset, block |

**İzin Matrisi (Satır=Rol, Sütun=İşlem)**:

```
                    mint  burn  force_transfer  freeze/block  thaw  thaw_asset  destroy
owner               -     -     -               -             -     -           YES
issuer              YES   -     -               -             -     -           -
admin               -     YES   YES             -             YES   YES         -
freezer             -     -     -               YES           -     -           -
```

**Kritik Noktalar**:
- `create()`: owner = CreateOrigin döndüreni; admin/issuer/freezer = `admin` parametresi (`lib.rs:803-817`)
- `force_create()`: owner/issuer/admin/freezer TÜMÜ = `owner` parametresi (`functions.rs:772-786`)
- Roller bağımsız hesaplar olabilir; `set_team` sadece issuer/admin/freezer değiştirir, owner değil
- force_cancel_approval: ForceOrigin VEYA admin yapabilir (`lib.rs:1602-1609`)

---

## 6. Asset Oluşturma

### `create` (call_index=0, `lib.rs:782-827`)
- **Origin**: `CreateOrigin` (tipik olarak Signed + bazı kısıtlamalar)
- **Parametreler**: `id`, `admin`, `min_balance`
- **Deposit**: `T::AssetDeposit::get()` — Currency::reserve ile kilitlenir
- **is_sufficient**: DAİMA `false` (permissionless create'te hardcoded)
- **Kısıtlamalar**:
  - `id` kullanımda olmamalı (`InUse` hatası)
  - `min_balance > 0` (`MinBalanceZero` hatası)
  - `NextAssetId` ayarlıysa `id == NextAssetId` olmalı (`BadAssetId` hatası)
- **Sonuç**: owner = CreateOrigin çıktısı; issuer/admin/freezer = `admin` param

### `force_create` (call_index=1, `lib.rs:849-860` → `functions.rs:758-790`)
- **Origin**: `ForceOrigin`
- **Parametreler**: `id`, `owner`, `is_sufficient`, `min_balance`
- **Deposit**: YOK (zero deposit)
- **is_sufficient**: Ayarlanabilir
- **Sonuç**: owner/issuer/admin/freezer = `owner` parametresi

### Deposit Hesabı
```
metadata_deposit = MetadataDepositBase + MetadataDepositPerByte * (name.len + symbol.len)
// functions.rs:1100-1104
```

---

## 7. Mint / Force Mint

### `mint` (call_index=6, `lib.rs:962-973`)
- **Origin**: Signed — çağıran **issuer** olmalı
- **Parametreler**: `id`, `beneficiary`, `amount`
- **İzin kontrolü**: `check_issuer == details.issuer` (`functions.rs:464-466`)
- **Limit**: Supply limiti YOK — sadece overflow kontrolü (`checked_add` başarısız olursa Overflow)
- **Akış**:
  1. `can_increase()` → DepositConsequence::Success kontrolü
  2. `Asset::try_mutate` → `details.supply.saturating_add(amount)`
  3. Hesap yoksa yeni hesap oluştur (`new_account()`)
  4. `Issued` event yayınla
- **Minimum balance**: Yeni hesap oluşturuyorsa `amount >= details.min_balance` şart (`functions.rs:511`)

### Force Mint (programmatic, `impl_fungibles.rs:222-231`)
```rust
fn create(id, admin, is_sufficient, min_balance) -> DispatchResult {
    Self::do_force_create(id, admin, is_sufficient, min_balance)
}
```
`fungibles::Create` trait üzerinden — deposit almadan mint API'si

---

## 8. Burn / Force Burn

### `burn` (call_index=7, `lib.rs:991-1003`)
- **Origin**: Signed — çağıran **admin** olmalı
- **Parametreler**: `id`, `who`, `amount`
- **Flags**: `{ keep_alive: false, best_effort: true }` (`lib.rs:1001`)
  - `best_effort: true` → bakiye yetersiz olsa bile mümkün olanı yakar
- **İzin kontrolü**: `check_admin == details.admin` (`functions.rs:550`)
- **Akış** (`functions.rs:534-559`):
  1. Asset Live veya Frozen olmalı (Destroying değil)
  2. `decrease_balance()` → `prep_debit()` ile gerçek miktar hesaplanır
  3. `details.supply.saturating_sub(actual)`
  4. Bakiye min_balance altına düşerse hesap kapatılır
  5. `Burned` event yayınla
- **Dust removal**: Bakiye `< min_balance` → `ensure_account_can_die()` → hesap silinir

### Burn sonrası hesap silme (`functions.rs:598-619`)
```rust
if account.balance < details.min_balance {
    // bakiye min_balance altına düştü
    Self::ensure_account_can_die(id, target)?;  // hold/freeze yoksa
    target_died = Some(Self::dead_account(...));
    // Remove ise Account storage'dan sil
}
// Hook'lar mutate dışında çağrılır:
T::Freezer::died(id, target);
T::Holder::died(id, target);
```

---

## 9. Transfer Mekanizmaları

### `transfer` (call_index=8, `lib.rs:1025-1037`)
```rust
let f = TransferFlags { keep_alive: false, best_effort: false, burn_dust: false };
```
- Kaynak hesap ölmesine izin verilir
- Kesin miktar (best_effort değil)
- Dust yakılmaz

### `transfer_keep_alive` (call_index=9, `lib.rs:1058-1070`)
```rust
let f = TransferFlags { keep_alive: true, best_effort: false, burn_dust: false };
```
- Kaynak hesabın canlı kalması zorunlu
- `WouldDie` hatası döner eğer transfer kaynağı öldürecekse

### `force_transfer` (call_index=10, `lib.rs:1092-1106`)
```rust
let f = TransferFlags { keep_alive: false, best_effort: false, burn_dust: false };
// maybe_need_admin = Some(origin) — admin kontrolü yapar
```
- Origin **admin** olmalı
- Keyfi iki hesap arasında transfer

### Transfer İç Akışı (`functions.rs:631-748`)
1. `prep_debit()` → kaynaktan düşülecek miktarı hesapla
2. `prep_credit()` → hedefe eklenecek miktarı hesapla (dust burn varsa)
3. `source_account.balance -= debit`
4. Kaynak `< min_balance` ise `ensure_account_can_die()` + `dead_account()`
5. `dest_account.balance += credit` (yoksa yeni hesap)
6. `Transferred` event

### transfer_approved (`functions.rs:1005-1049`)
- Delegate'in approval'ını kontrol eder
- `approved.amount.checked_sub(&amount)` → Unapproved hatası
- Transfer başarılıysa ve `remaining.is_zero()` → approval deposit unreserve

### Untouchable Balance Hesabı (`functions.rs:258-274`)
```rust
let untouchable = match (held, frozen, keep_alive) {
    (None, None, true) => details.min_balance,
    (None, None, false) => Zero::zero(),
    (maybe_held, maybe_frozen, _) => {
        let frozen = maybe_frozen.unwrap_or_default();
        let held = maybe_held.unwrap_or_default();
        frozen.saturating_sub(held).max(details.min_balance)
    }
};
```
**KRITIK**: `untouchable = max(frozen - held, min_balance)` — dokunulamaz tutar

---

## 10. Freeze / Thaw Sistemi

### Hesap Seviyesi Dondurma

**`freeze`** (call_index=11, `lib.rs:1121-1145`)
- Origin: **freezer** olmalı
- `AccountStatus::Liquid → Frozen`
- Hesap var olmalı (NoAccount)
- Asset Live veya Frozen olabilir

**`thaw`** (call_index=12, `lib.rs:1158-1182`)
- Origin: **admin** olmalı (freezer değil!)
- `AccountStatus::Frozen → Liquid`
- Asset Live veya Frozen olabilir

**`block`** (call_index=31, `lib.rs:1797-1821`)
- Origin: **freezer** olmalı
- `AccountStatus → Blocked`
- Blocked hesap: hem gönderemez, hem alamaz
- `is_blocked()` check: `can_increase()` fonksiyonunda `DepositConsequence::Blocked` döner

### Asset Seviyesi Dondurma

**`freeze_asset`** (call_index=13, `lib.rs:1194-1208`)
- Origin: **freezer** olmalı
- `AssetStatus::Live → Frozen`
- Tüm transferler yasak (admin işlemleri dahil değil)
- `can_decrease()` → `Frozen` sonucu

**`thaw_asset`** (call_index=14, `lib.rs:1220-1234`)
- Origin: **admin** olmalı
- `AssetStatus::Frozen → Live`

### assets-freezer (pallet-assets extension)
- `MutateFreeze::set_freeze()` (`freezer/src/impls.rs:127-145`)
  - Frozen amount max değer kullanır: `after_frozen = freezes.iter().map(|f| f.amount).max()`
  - `freezer/src/lib.rs:152`
- `extend_freeze()` → mevcut freeze'den büyükse günceller (`impls.rs:148-165`)
- `thaw()` → o reason'ı kaldırır (`impls.rs:167-171`)

---

## 11. Minimum Balance ve Dust Removal

### Minimum Balance Mekanizması
- `AssetDetails.min_balance` — her hesabın tutması gereken minimum
- `can_increase()` → yeni hesap için `amount >= min_balance` (`functions.rs:161`)
- Var olan hesap için overflow kontrolü yeterli

### Dust Removal Akışı (`functions.rs:598-619`)
```rust
account.balance = account.balance.saturating_sub(actual);
if account.balance < details.min_balance {
    debug_assert!(account.balance.is_zero(), "checked in prep; qed");
    Self::ensure_account_can_die(id.clone(), target)?;
    target_died = Some(Self::dead_account(target, details, &account.reason, false));
    if let Some(Remove) = target_died {
        return Ok(());  // Account::remove çağrılır
    }
}
```

### ensure_account_can_die (`functions.rs:99-106`)
```rust
pub(super) fn ensure_account_can_die(id: T::AssetId, who: &T::AccountId) -> DispatchResult {
    ensure!(T::Holder::balance_on_hold(id.clone(), who).is_none(), Error::ContainsHolds);
    ensure!(T::Freezer::frozen_balance(id, who).is_none(), Error::ContainsFreezes);
    Ok(())
}
```
Hesap silinebilmesi için: hold VE freeze YOK olmalı.

### ExistenceReason ile Deposit Geri Alma
- `DepositHeld(Balance)` → `Currency::unreserve` ile geri alınır
- `DepositFrom(AccountId, Balance)` → depositor'a geri alınır
- `Consumer` → `dec_consumers`
- `Sufficient` → `dec_sufficients`
- `DepositRefunded` → hiçbir şey yapılmaz

### set_min_balance (call_index=28, `lib.rs:1703-1734`)
- Sadece **owner** yapabilir
- `is_sufficient == true` ise min_balance değiştirilemez (`NoPermission`)
- `min_balance < old_min_balance OR accounts == 0` — aksi takdirde izin yok

---

## 12. Total Supply Tracking

### Mint sırasında (`functions.rs:463-477`)
```rust
details.supply = details.supply.saturating_add(amount);  // line 469
```
- `increase_balance()` çağrısı içindeki closure `do_mint()`'ten gelir
- `can_increase()` öncesinde `supply.checked_add(&amount).is_some()` kontrolü

### Burn sırasında (`functions.rs:547-557`)
```rust
details.supply = details.supply.saturating_sub(actual);  // line 554
```
- Gerçek yakılan miktar (best_effort nedeniyle `amount` farklı olabilir)

### Transfer sırasında (`functions.rs:687-692`)
```rust
// Dust burn varsa:
details.supply = details.supply.saturating_sub(burn);
```
- Normal transfer'de supply değişmez
- `burn_dust: true` ise kaynak → hedef minimum altı kalan dust yakılır ve supply düşer

### do_refund sırasında (`functions.rs:386-390`)
```rust
details.supply = details.supply.saturating_sub(account.balance);
```
- `allow_burn: true` ve bakiye varsa, deposit iade + bakiye yakılır + supply düşer

### set_total_issuance (programmatic, `impl_fungibles.rs:188-193`)
```rust
fn set_total_issuance(id: T::AssetId, amount: Self::Balance) {
    Asset::<T, I>::mutate_exists(id, |maybe_asset| {
        if let Some(ref mut asset) = maybe_asset {
            asset.supply = amount
        }
    });
}
```
`fungibles::Unbalanced` trait üzerinden — doğrudan supply güncelleme

---

## 13. Sabitler

| Sabit | Değer | Kaynak |
|---|---|---|
| `MAX_RESERVES` | `5` | `lib.rs:264` |
| `STORAGE_VERSION` | `StorageVersion::new(1)` | `lib.rs:261` |
| `LOG_TARGET` | `"runtime::assets"` | `lib.rs:203` |
| `#[recursion_limit]` | `"1024"` | `lib.rs:150` (benchmark nedeniyle) |

**Config sabitleri** (runtime'da belirlenir, kodda varsayılan yok):
- `AssetDeposit`, `AssetAccountDeposit`, `MetadataDepositBase`, `MetadataDepositPerByte`, `ApprovalDeposit`, `StringLimit`, `RemoveItemsLimit`

**TestDefaultConfig sabitleri** (`lib.rs:303-313`):
```
Balance = u64
RemoveItemsLimit = ConstU32<5>
AssetId = u32
AssetDeposit = ConstUint<1>
AssetAccountDeposit = ConstUint<10>
MetadataDepositBase = ConstUint<1>
MetadataDepositPerByte = ConstUint<1>
ApprovalDeposit = ConstUint<1>
StringLimit = ConstU32<50>
```

---

## 14. assets-freezer Özeti

**Amaç**: `pallet-assets`'e freeze yeteneği ekleyen extension pallet.

**Storage**:
- `Freezes`: Her (asset, account) için freeze listesi — BoundedVec, max = `VariantCount(RuntimeFreezeReason)`
- `FrozenBalances`: Toplam dondurulmuş bakiye önbelleği

**Kritik Davranış** (`freezer/src/lib.rs:152`):
```rust
let after_frozen = freezes.iter().map(|f| f.amount).max().unwrap_or_default();
```
**Freeze bakiyesi = Tüm reason'ların MAXIMUM'u** (toplam değil!)

**FrozenBalance implementasyonu** (`freezer/src/impls.rs:34-55`):
- `frozen_balance()` → `FrozenBalances` storage'dan okur
- `died()` → Freezes ve FrozenBalances temizlenir
- `contains_freezes()` → `Freezes::contains_prefix(asset)`

**MutateFreeze** (`freezer/src/impls.rs:126-172`):
- `set_freeze()` → belirli reason için miktar belirle (0 ise thaw çağır)
- `extend_freeze()` → mevcut ile `max` al
- `thaw()` → o reason'ı kaldır, update_freezes

---

## 15. assets-holder Özeti

**Amaç**: `pallet-assets`'e hold yeteneği ekleyen extension pallet.

**Storage**:
- `Holds`: Her (asset, account) için hold listesi — BoundedVec, max = `VariantCount(RuntimeHoldReason)`
- `BalancesOnHold`: Toplam hold bakiyesi

**Kritik Davranış** (`holder/src/impl_fungibles.rs`):
- `balance_on_hold()` → `BalancesOnHold` storage'dan okur
- Hold bakiyesi = **TOPLAM** (freeze'deki max aksine SUM)
- `set_balance_on_hold()` → delta hesaplayarak total günceller (checked_add/checked_sub)

**Freeze vs Hold Farkı**:
- Freeze: `untouchable = frozen - held` (hold, freeze'den düşülür)
- Hold: hold bakiyesi `AssetAccount.balance`'a dahil DEĞİL (ayrı tracking)
- `total_balance = balance + hold` (`impl_fungibles.rs:49-52`)

---

## DEX Uygulama Notu (ZORUNLU)

### pallet-asset-conversion ile Entegrasyon

**1. AccountTouch Implementasyonu** (`lib.rs:1923-1947`)
```rust
impl<T: Config<I>, I: 'static> AccountTouch<T::AssetId, T::AccountId> for Pallet<T, I> {
    fn deposit_required(_: T::AssetId) -> Self::Balance {
        T::AssetAccountDeposit::get()
    }
    fn should_touch(asset: T::AssetId, who: &T::AccountId) -> bool {
        match Asset::<T, I>::get(&asset) {
            Some(info) if info.is_sufficient => false,  // sufficient asset: touch gerekmez
            Some(_) if frame_system::Pallet::<T>::can_accrue_consumers(who, 2) => false,
            Some(_) => !Account::<T, I>::contains_key(asset, who),
            _ => true,
        }
    }
}
```
DEX, havuz hesabı oluşturmadan önce `should_touch` kontrol etmeli!

**2. is_sufficient Flag — DEX İçin Kritik**
- `is_sufficient = true`: Hesap yaratmak için consumer ref gerekmez
- DEX pool hesapları için `is_sufficient` asset kullanmak veya touch mekanizması şart
- `force_create()` ile `is_sufficient: true` yapılabilir ama sonradan `set_min_balance` çalışmaz!

**3. Transfer Önce Freeze Kontrolü**
- DEX transfer çağrısı `can_decrease()` çağırır
- `can_decrease()`: hesap status frozen/blocked ise `WithdrawConsequence::Frozen`
- Asset-level freeze de transfer'i engeller

**4. Dust ve Minimum Balance — DEX Pool Güvenliği**
- `TransferFlags { burn_dust: false }` → dust yakılmaz, transfer başarısız olur
- DEX implementation: eğer kaynak hesap minimum altına düşecekse transfer reject
- Pool bakiyesinin min_balance üzerinde olduğunu garanti etmek için min_balance'ı 1 set et

**5. Approval API — DEX İzin Mekanizması**
- `approve_transfer()` → `ApprovalDeposit` rezerv gerektirir
- DEX: kullanıcı önce `approve_transfer`, sonra DEX `transfer_approved` çağırır
- `approved.amount.saturating_add(amount)` — kümülatif! Mevcut approval üstüne eklenir

**6. BalanceToAssetBalance Dönüşümü** (`types.rs:330-361`)
```rust
// balance * asset.min_balance / min_balance
Ok(FixedU128::saturating_from_rational(asset.min_balance, min_balance)
    .saturating_mul_int(balance))
```
Yalnızca `is_sufficient == true` asset'ler için çalışır (DEX fiyat hesabı için kritik)

**7. Destroy Guard — DEX Asset Güvenliği**
- `start_destroy()` başlamadan önce: `contains_holds` ve `contains_freezes` kontrolü
- Eğer DEX aktif pool'u varken asset destroy edilirse sorun çıkar
- DEX callback ile `AssetsCallback::destroyed` hook'u implement etmeli

**8. Dikkat Edilecek Sorunlar**:
- Asset `Destroying` durumunda tüm işlemler başarısız (UnknownAsset)
- `AssetStatus::Frozen`: admin işlemleri ÇALIŞIR, normal transfer ÇALIŞMAZ
- `AccountStatus::Blocked`: hem transfer hem de receive engeli — DEX pool hesabı blocked olamaz
- `NextAssetId` ayarlıysa ID sekansı zorunlu — DEX asset ID rezervasyonu dikkatli yapılmalı
- Hold ve freeze varken hesap silinemez — DEX hesap temizlemeden önce hold/freeze temizlemeli

---

## Kritik Bulgular Özeti

| # | Bulgu | Kaynak |
|---|---|---|
| 1 | **create() vs force_create() farkı**: create → is_sufficient=false hardcoded, deposit alır; force_create → is_sufficient ayarlanabilir, deposit yok | `lib.rs:800,859` |
| 2 | **Admin rolleri bağımsız**: thaw = admin, freeze = freezer (aynı hesap olmak zorunda değil) | `lib.rs:1134,1171` |
| 3 | **Untouchable bakiye**: `frozen.saturating_sub(held).max(min_balance)` — hold, freeze'den çıkarılır | `functions.rs:228` |
| 4 | **Freeze bakiyesi = MAX**: `FrozenBalances = freezes.iter().map(|f| f.amount).max()` | `freezer/lib.rs:152` |
| 5 | **Hold bakiyesi = SUM**: `BalancesOnHold` tüm hold'ların toplamı | `holder/impl.rs:192-229` |
| 6 | **Dust removal koşulu**: hesap min_balance altına düşerse ancak hold/freeze yoksa silinir; varsa ContainsHolds/ContainsFreezes hatası | `functions.rs:99-106` |
| 7 | **Supply tracking**: mint'te `saturating_add`, burn'de `saturating_sub`, dust burn'de `saturating_sub` | `functions.rs:469,554,691` |
| 8 | **MAX_RESERVES = 5**: asset başına maksimum 5 reserve lokasyonu | `lib.rs:264` |
| 9 | **Destroying geri dönüşsüz**: `AssetStatus::Destroying` bir kez set edilince `Live`'a dönüş yok | `types.rs:46-48` |
| 10 | **touch() depositi**: `T::AssetAccountDeposit::get()` rezerv gerektirir; non-sufficient asset için zorunlu | `functions.rs:349` |
