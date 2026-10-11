# pallet-assets — Analiz B (Approvals, Metadata, Tests)
**Agent**: 14-B | **Tarih**: 2026-07-22 | **Hassasiyet**: 3X

---

## 1. Approval Sistemi

### Approval Struct (types.rs:82-88)
```rust
pub struct Approval<Balance, DepositBalance> {
    pub amount: Balance,       // onaylanan transfer miktarı
    pub deposit: DepositBalance, // storage deposit (reserved)
}
```

### Approvals Storage (lib.rs:460-468)
```rust
pub type Approvals<T: Config<I>, I: 'static = ()> = StorageNMap<
    _,
    (
        NMapKey<Blake2_128Concat, T::AssetId>,
        NMapKey<Blake2_128Concat, T::AccountId>, // owner
        NMapKey<Blake2_128Concat, T::AccountId>, // delegate
    ),
    Approval<T::Balance, DepositBalanceOf<T, I>>,
>;
```
3-tuple anahtarlı NMap: `(asset_id, owner, delegate)`. MaxEncodedLen = 148 byte.

### do_approve_transfer (functions.rs:931-970)
Tam akış:
1. `Asset::<T,I>::get(&id)` — asset var mı kontrol, `Live` olmalı
2. `Approvals::try_mutate((id, owner, delegate))`:
   - Mevcut approval yoksa: `d.approvals.saturating_inc()` → yeni slot
   - `deposit_required = T::ApprovalDeposit::get()`
   - Mevcut deposit < deposit_required ise: eksik kısmı reserve et: `T::Currency::reserve(owner, deposit_required - approved.deposit)`
   - `approved.amount = approved.amount.saturating_add(amount)` — **ADDITIVE** (mevcut üzerine eklenir)
3. `Asset::<T,I>::insert(&id, d)` — approvals sayısını güncelle
4. Event: `ApprovedTransfer { asset_id, source, delegate, amount }` (sadece eklenen miktar emit edilir, toplam değil)

**Kritik**: approve_transfer ADDITIVE çalışır. Her çağrı mevcut onayı artırır, sıfırlamaz.
**Kritik**: Deposit bir kez alınır; sonraki approve_transfer çağrılarında yeterli deposit varsa ek reserve yapılmaz (functions.rs:952-955).

### do_cancel_approval (functions.rs:975-996)
1. Asset `Live` olmalı
2. `Approvals::take((id, owner, delegate))` — yoksa `Error::Unknown`
3. `T::Currency::unreserve(owner, approval.deposit)`
4. `asset_details.approvals.saturating_dec()`
5. Event: `ApprovalCancelled`

**Kritik**: `cancel_approval` yalnızca owner çağırabilir (call_index 23).
**force_cancel_approval** (call_index 24): ForceOrigin veya Admin çağırabilir.

### do_transfer_approved (functions.rs:1005-1049)
Tam akış:
1. Asset `Live` kontrol
2. `Approvals::try_mutate_exists((id, owner, delegate))`:
   - `approved.amount.checked_sub(&amount).ok_or(Error::Unapproved)` — yetersizse hata
   - `transfer_and_die(id, owner, destination, amount, None, f)` (f: keep_alive=false, best_effort=false, burn_dust=false)
   - Kalan miktar sıfır ise: deposit unreserve edilir, approvals sayısı azaltılır
   - Kalan > 0 ise: `approved.amount = remaining` (kısmi kullanım destekleniyor)
3. Hook: owner account ölmüşse `Freezer::died` ve `Holder::died` çağrılır

**Kritik**: transfer_approved, do_transfer değil, doğrudan transfer_and_die kullanır (deposit unreserve için try_mutate içinde kalmak gerektiğinden).

---

## 2. Metadata Sistemi

### AssetMetadata Struct (types.rs:190-204)
```rust
pub struct AssetMetadata<DepositBalance, BoundedString> {
    pub deposit: DepositBalance,
    pub name: BoundedString,       // BoundedVec<u8, StringLimit>
    pub symbol: BoundedString,     // BoundedVec<u8, StringLimit>
    pub decimals: u8,
    pub is_frozen: bool,           // non-Force origin değiştiremez
}
```

### Metadata Storage (lib.rs:471-478)
```rust
pub type Metadata<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat, T::AssetId,
    AssetMetadata<DepositBalanceOf<T, I>, BoundedVec<u8, T::StringLimit>>,
    ValueQuery,    // varsayılan: boş struct
>;
```
`ValueQuery` kullanıldığından, metadata olmayan asset için de `Metadata::get` boş struct döndürür (None değil!).

### do_set_metadata (functions.rs:1051-1097)
```
deposit = MetadataDepositBase + MetadataDepositPerByte * (name.len + symbol.len)
```
1. `name/symbol` → `BoundedVec::try_into()` → başarısız ise `Error::BadMetadata`
2. Asset `Live` olmalı
3. `from == &d.owner` — sadece owner ayarlayabilir (force olmadan)
4. `metadata.is_frozen` kontrol: frozen metadata değiştirilemez → `Error::NoPermission`
5. Eski deposit alınır, yeni deposit hesaplanır
6. Fark: reserve/unreserve uygulanır (delta-tabanlı, tam tutarı değil)
7. is_frozen = false set edilir (set_metadata her zaman unfreeze yapar)

### calc_metadata_deposit (functions.rs:1100-1104)
```rust
T::MetadataDepositPerByte::get()
    .saturating_mul(((name.len() + symbol.len()) as u32).into())
    .saturating_add(T::MetadataDepositBase::get())
```
Test değerleri: MetadataDepositBase=1, MetadataDepositPerByte=1.
Örnek: name=10, symbol=10 → deposit = 1 + 20 = 21. Test'te balance=30 iken reserved=21 beklenir.

### set_metadata (call_index 17, lib.rs:1341-1352)
- `#[pallet::weight(T::WeightInfo::set_metadata(name.len() as u32, symbol.len() as u32))]`
- Weight runtime'da `n+s` parametresiyle linear scaling kullanır

### force_set_metadata (call_index 19, lib.rs:1397-1435)
- ForceOrigin zorunlu
- Mevcut deposit korunur (üzerine yazılır ama değiştirilmez)
- `is_frozen` parametresi explicit olarak set edilir
- Metadata `is_frozen=true` set edilirse, sonraki `set_metadata` çağrısı `NoPermission` verir

### clear_metadata (call_index 18, lib.rs:1366-1380)
- Sadece owner çağırabilir
- Metadata yoksa `Error::Unknown`
- Deposit unreserve edilir

### force_clear_metadata (call_index 20, lib.rs:1448-1463)
- ForceOrigin zorunlu
- Deposit unreserve edilir (owner'a gider)

---

## 3. Fungible Trait Implementasyonları

Tüm implementasyonlar `impl_fungibles.rs` dosyasındadır.

### fungibles::Inspect (impl_fungibles.rs:33-84)
| Metod | Açıklama |
|-------|----------|
| `total_issuance(asset)` | `Asset::get(asset).map(x => x.supply)` |
| `minimum_balance(asset)` | `Asset::get(asset).map(x => x.min_balance)` |
| `balance(asset, who)` | `Pallet::balance(asset, who)` — sadece "free" balance |
| `total_balance(asset, who)` | `balance + Holder::balance_on_hold` — **held dahil** |
| `reducible_balance(asset, who, preservation, fortitude)` | `preservation==Expendable` ise `keep_alive=false` |
| `can_deposit(asset, who, amount, provenance)` | `provenance==Minted` ise supply artış kontrol |
| `can_withdraw(asset, who, amount)` | `can_decrease(asset, who, amount, keep_alive=false)` |
| `asset_exists(asset)` | `Asset::contains_key(asset)` |

**Kritik**: Fortitude parametresi şu an dikkate alınmıyor! (impl_fungibles.rs:59 — `_: Fortitude`). Bu bir TODO olabilir.

### fungibles::Mutate (impl_fungibles.rs:86-116)
Sadece event hook'ları implement eder:
- `done_mint_into` → `Event::Issued`
- `done_burn_from` → `Event::Burned`
- `done_transfer` → `Event::Transferred`

Asıl iş `Unbalanced::increase_balance` ve `decrease_balance` üzerinden yapılır.

### fungibles::Unbalanced (impl_fungibles.rs:175-220)
```rust
fn write_balance(...) -> ... {
    defensive!("write_balance is not used if other functions are impl'd");
    Err(DispatchError::Unavailable)
}
```
`write_balance` kasıtlı olarak çalışmıyor — bu pallet kendi `increase_balance` / `decrease_balance` impl'lerini sunar.

`handle_dust` da defensive! ile devre dışı: "decrease_balance ve increase_balance non-default impl var, buraya gelinmez."

### fungibles::Balanced (impl_fungibles.rs:144-173)
- `OnDropCredit`: `DecreaseIssuanceWithEvent` — kredi düştüğünde supply azaltılır + event
- `OnDropDebt`: `IncreaseIssuanceWithEvent` — borç düştüğünde supply artırılır + event
- `done_deposit` → `Event::Deposited`
- `done_withdraw` → `Event::Withdrawn`

### fungibles::Create (impl_fungibles.rs:222-231)
```rust
fn create(id, admin, is_sufficient, min_balance) -> DispatchResult {
    Self::do_force_create(id, admin, is_sufficient, min_balance)
}
```

### fungibles::Destroy (impl_fungibles.rs:233-249)
4 adımlı yıkım delegasyonu: `start_destroy`, `destroy_accounts`, `destroy_approvals`, `finish_destroy`

### fungibles::metadata::Inspect (impl_fungibles.rs:251-265)
- `name(asset)` → `Metadata::get(asset).name.to_vec()`
- `symbol(asset)` → `Metadata::get(asset).symbol.to_vec()`
- `decimals(asset)` → `Metadata::get(asset).decimals`

**Kritik**: `Metadata::ValueQuery` olduğundan, mevcut olmayan asset için boş string ve 0 döner (hata vermez).

### fungibles::metadata::Mutate (impl_fungibles.rs:267-279)
- `set(asset, from, name, symbol, decimals)` → `Self::do_set_metadata(...)`

### fungibles::metadata::MetadataDeposit (impl_fungibles.rs:281-292)
- `calc_metadata_deposit(name, symbol)` → public hesaplama fonksiyonu

### fungibles::approvals::Inspect (impl_fungibles.rs:294-307)
```rust
fn allowance(asset, owner, delegate) -> T::Balance {
    Approvals::get((asset, owner, delegate))
        .map(|x| x.amount)
        .unwrap_or_else(Zero::zero)
}
```

### fungibles::approvals::Mutate (impl_fungibles.rs:309-331)
- `approve(asset, owner, delegate, amount)` → `do_approve_transfer`
- `transfer_from(asset, owner, delegate, dest, amount)` → `do_transfer_approved`

### fungibles::roles::Inspect (impl_fungibles.rs:333-351)
- `owner`, `issuer`, `admin`, `freezer` → `Asset::get(asset).map(...)`, `Option<AccountId>` döner

### fungibles::InspectEnumerable (impl_fungibles.rs:353-362)
- `asset_ids()` → `Asset::iter_keys()` (her item için storage read!)

### fungibles::roles::ResetTeam (impl_fungibles.rs:364-374)
- `reset_team(id, owner, admin, issuer, freezer)` → `do_reset_team`

### fungibles::Refund (impl_fungibles.rs:376-394)
```rust
fn deposit_held(id, who) -> Option<(AccountId, Balance)> {
    match account.reason {
        DepositHeld(b) => Some((who, b)),
        DepositFrom(d, b) => Some((d, b)),
        _ => None,
    }
}
fn refund(id, who) -> DispatchResult {
    match Self::deposit_held(...) {
        Some((d, _)) if d == who => Self::do_refund(id, who, false),  // kendi depositi
        Some(..) => Self::do_refund_other(id, &who, None),             // başkasının depositi
        None => Err(Error::NoDeposit.into()),
    }
}
```

---

## 4. Touch / Refund Mekanizması

### do_touch (functions.rs:342-367)
Deposit: `T::AssetAccountDeposit::get()` (test: 10 birim)
Akış:
1. `!Account::contains_key(&id, &who)` — zaten varsa `Error::AlreadyExists`
2. `details.status == Live` kontrol
3. `new_account(who, details, Some((depositor, deposit)))` → ExistenceReason belirlenir
   - `depositor == who` ise: `ExistenceReason::DepositHeld(deposit)`
   - `depositor != who` ise: `ExistenceReason::DepositFrom(depositor, deposit)`
4. `T::Currency::reserve(&depositor, deposit)` — depositor'ın balance'ından reserve
5. Account sıfır bakiyeyle (`Zero::zero()`, `Liquid`) oluşturulur
6. Event: `Touched { asset_id, who, depositor }`

**Kritik**: touch; `is_sufficient=false` asset'ler için consumer reference olmadan hesap açmanın yoludur. Non-sufficient asset'te `touch` ile oluşturulan hesap tüketici referansı kullanmaz, deposit kullanır.

### do_refund (functions.rs:370-412)
Koşullar:
- Account `Consumer` veya `DepositHeld` reason'ına sahip olmalı (Sufficient/DepositFrom/DepositRefunded hata verir)
- Asset `Live` veya `Frozen` olmalı (Destroying hata verir)
- `allow_burn=false` ise balance sıfır olmalı (yoksa `Error::WouldBurn`)
- `ensure_account_can_die` — freeze veya hold varsa hata

Akış:
1. Deposit unreserve
2. `dead_account` çağrısı
3. Eğer Remove: Account storage'dan kaldırılır, bakiye varsa supply'dan düşülür, `Burned` event emit edilir
4. `Freezer::died` ve `Holder::died` hook çağrılır

### do_refund_other (functions.rs:414-450)
- `DepositFrom(depositor, deposit)` reason'ı gerektirir
- Balance sıfır olmalı (yoksa `Error::WouldBurn`)
- `maybe_check_caller`: caller == depositor veya caller == admin olmalı
- Asset `Live` olmalı (Frozen/Destroying hata)
- Account frozen olmamalı

---

## 5. Destroy / Wipe

### do_start_destroy (functions.rs:794-812)
1. Owner kontrolü (ForceOrigin ise None)
2. `ensure!(!T::Holder::contains_holds(id))` → `Error::ContainsHolds`
3. `ensure!(!T::Freezer::contains_freezes(id))` → `Error::ContainsFreezes`
4. `details.status = AssetStatus::Destroying`
5. Event: `DestructionStarted`

**Kritik**: destroy başlatmadan önce TÜM freeze ve hold'ların temizlenmesi gerekir. Bu sıralamanın bozulması demek, içinde freeze/hold olan bir asset'in yıkılamaması demek.

### do_destroy_accounts (functions.rs:818-866)
- Sadece `Destroying` state'inde çalışır
- `T::RemoveItemsLimit` kadar account kaldırır (varsayılan 5, production'da 1000)
- Her account için: deposit/deposit_from unreserve edilir, `dead_account` çağrılır
- Hold veya freeze olan account'lar atlanır (`ensure_account_can_die` başarısız olanlar)
- Batch sonrası: `Freezer::died` ve `Holder::died` hooklarını çağırır
- Weight: `destroy_accounts(c: u32)` — c ile linear

### do_destroy_approvals (functions.rs:872-902)
- Sadece `Destroying` state'inde çalışır
- `Approvals::drain_prefix((id,))` ile iteration
- Her approval için deposit unreserve
- `T::RemoveItemsLimit` kadar kaldırır

### do_finish_destroy (functions.rs:907-925)
Koşullar (hepsi sıfır olmalı):
1. `details.accounts == 0` → `Error::InUse`
2. `details.approvals == 0` → `Error::InUse`
3. `T::CallbackHandle::destroyed(&id).is_ok()` → `Error::CallbackFailed`

Akış:
1. `maybe_details.take()` — storage'dan kaldır
2. Metadata alınır ve `metadata.deposit + details.deposit` birlikte unreserve edilir
3. `Reserves::remove(&id)` — reserve bilgisi silinir
4. Event: `Destroyed`

**DEX İçin Kritik**: Destroy süreci 4 ayrı TX gerektirir: `start_destroy` → `destroy_accounts*` → `destroy_approvals*` → `finish_destroy`. Her biri ayrı block'ta olabilir. DEX'in pool kapanma mantığı buna benzer incremental yaklaşım kullanabilir.

---

## 6. Test Coverage

### tests.rs — Ana Test Listesi

| Test Adı | Test Ediyor | Başarı Koşulu |
|----------|-------------|---------------|
| `transfer_should_never_burn` | Consumer ref tükenmişken transfer | Transferred event, bakiye korunur |
| `basic_minting_should_work` | Çoklu asset mint | Bakiyeler doğru, asset_ids listesi |
| `minting_too_many_insufficient_assets_fails` | 2 consumer limitiyle 3. mint fail | `TokenError::CannotCreate` |
| `minting_insufficient_asset_with_deposit` | touch sonrası 3. mint | touch ile deposit koy, mint başar |
| `minting_insufficient_assets_with_deposit_without_consumer` | Depositsiz mint fail | touch ile çöz |
| `refunding_asset_deposit_with_burn_should_work` | allow_burn=true refund | Bakiye yakılır, reserved=0 |
| `refund_with_zero_balance_does_not_emit_burned` | Sıfır bakiyle refund | Burned event YOK |
| `refunding_asset_deposit_with_burn_disallowed_should_fail` | allow_burn=false, bakiye var | `Error::WouldBurn` |
| `refunding_asset_deposit_without_burn_should_work` | Önce transfer, sonra refund | Başarılı |
| `refunding_calls_died_hook` | refund sonrası died hook | `Hook::Died` 2 kez (Holder+Freezer) |
| `refunding_with_sufficient_existence_reason_should_fail` | Sufficient account refund | `Error::NoDeposit` |
| `refunding_with_deposit_from_should_fail` | DepositFrom ile refund | `Error::NoDeposit` (do_refund_other gerekir) |
| `refunding_frozen_with_consumer_ref_works` | Frozen account refund | Başarılı |
| `refunding_frozen_with_deposit_works` | Frozen+deposit account refund | Başarılı |
| `approval_lifecycle_works` | Tam approval döngüsü | Approve → partial transfer → cancel |
| `transfer_approved_all_funds` | Tüm approved miktarı transfer | Approval otomatik silinir |
| `approval_deposits_work` | Deposit reserve/unreserve | Deposit alınır ve geri verilir |
| `cannot_transfer_more_than_approved` | Onaylanan üstü transfer | `Error::Unapproved` |
| `cannot_transfer_more_than_exists` | Balance üstü approved transfer | `Error::BalanceLow` |
| `cancel_approval_works` | Cancel akışı + hata durumları | Yanlış asset/owner/delegate → Unknown |
| `force_cancel_approval_works` | Admin cancel | NoPermission → Unknown hataları |
| `lifecycle_should_work` | Tam yaşam döngüsü (2x) | create→mint→destroy→tekrar |
| `destroy_should_refund_approvals` | Destroy sırasında approval deposit iadesi | reserved=0 |
| `partial_destroy_should_work` | RemoveItemsLimit=5, 7 account | 2 çağrı gerekiyor |
| `non_providing_should_work` | is_sufficient=false, consumer yoksa fail | CannotCreate |
| `min_balance_should_work` | min_balance altına düşünce reap | 4 farklı death yolu |
| `querying_total_supply_should_work` | Mint+transfer+burn sonrası supply | Doğru hesaplama |
| `transferring_frozen_user_should_not_work` | Frozen account transfer | `Error::Frozen` |
| `transferring_frozen_asset_should_not_work` | Frozen asset transfer | `Error::AssetNotLive` |
| `approve_transfer_frozen_asset_should_not_work` | Frozen asset approve | `Error::AssetNotLive` |
| `transferring_from_blocked_account_should_not_work` | Block → Frozen gibi davranır | `Error::Frozen` |
| `transferring_to_blocked_account_should_not_work` | Blocked account'a gönderme | `TokenError::Blocked` |
| `transfer_all_works_1/2/3` | transfer_all allow_death=false/true, frozen | Doğru bakiyeler |
| `touch_other_works_legacy` | Admin/freezer/herkes touch_other | Başarılı |
| `touch_other_works` | Fonsuz hesap touch_other fail | InsufficientBalance |
| `account_with_deposit_not_destroyed` | Sıfır bakiye ama deposit var, account korunur | Account silinmez |
| `refund_other_should_fails` | Birden çok fail durumu | NoDeposit, AssetNotLive, Frozen, NoPermission, WouldBurn |
| `refund_other_works` | Depositor=freezer/admin başarılı | reserved sıfırlanır |
| `set_metadata_should_work` | Metadata ayarla+güncelle+temizle | Deposit delta doğru |
| `calling_dead_account_fails_1/2` | Freeze/hold varken dead account | ContainsFreezes, ContainsHolds |
| `destroy_accounts_calls_died_hooks` | Destroy sonrası hook | Died x2 per account |
| `finish_destroy_asset_destroys_asset` | Asset tamamen silinir | Asset::get(0).is_none() |
| `freezer_should_work` | frozen_balance etkisi, untouchable = max(ed, frozen) | Doğru reducible |
| `freezing_and_holds_work` | Hold+freeze birlikte | untouchable = max(ed, frozen-held) |
| `imbalances_should_work` | issue/split/resolve | Supply doğru |
| `force_metadata_should_work` | force_set_metadata, force_clear_metadata | is_frozen destekleniyor |
| `force_asset_status_should_work` | min_balance değişimi | Düşük bakiyeli account'lar temizlenir |
| `set_min_balance_should_work` | set_min_balance kısıtları | Sadece is_sufficient=false ve 0 holder |
| `transfer_ownership_fails_when_deposit_is_locked` | Lock'lu balance ile ownership transfer | `Error::IncompleteDepositTransfer` |

### sets.rs (ayrı modül)
Muhtemelen ek set operasyonu testleri içeriyor (okunan dosyada görüldü: `mod sets;`).

### mock.rs Kritik Ayarlar
```rust
type MaxConsumers = ConstU32<3>;       // 3 consumer limit
AssetAccountDeposit = ConstUint<10>;   // touch deposit = 10
ApprovalDeposit = ConstUint<1>;        // approval deposit = 1
MetadataDepositBase = ConstUint<1>;    // metadata base = 1
MetadataDepositPerByte = ConstUint<1>; // metadata per byte = 1
RemoveItemsLimit = ConstU32<5>;        // destroy batch size = 5
StringLimit = ConstU32<50>;
```

TestHolder ve TestFreezer her ikisi de `Hook::Died` emit eder → died hook'lar 2 kez çağrılır (hem Holder hem Freezer'dan).

---

## 7. Benchmark Analizi

### Parametrik Benchmarklar (src/benchmarking.rs)

| Extrinsic | Parametreler | Açıklama |
|-----------|-------------|----------|
| `create` | - | O(1) — 3 storage read, 2 write |
| `force_create` | - | O(1) — 2 read, 1 write |
| `start_destroy` | - | O(1) — 1 read+write |
| `destroy_accounts` | `c in 0..RemoveItemsLimit` | O(c) — 2c read, 2c write |
| `destroy_approvals` | `a in 0..RemoveItemsLimit` | O(a) — a read+write |
| `finish_destroy` | - | O(1) — 2 read, 2 write |
| `mint` | - | O(1) |
| `burn` | - | O(1) |
| `transfer` | - | O(1) — 4 read, 4 write |
| `transfer_keep_alive` | - | O(1) |
| `approve_transfer` | - | O(1) |
| `transfer_approved` | - | O(1) |
| `cancel_approval` | - | O(1) |
| `force_cancel_approval` | - | O(1) |
| `set_metadata` | `n in 0..StringLimit, s in 0..StringLimit` | O(n+s) — linear |
| `force_set_metadata` | `n in 0..StringLimit, s in 0..StringLimit` | O(n+s) |
| `clear_metadata` | - | O(1) |
| `touch` | - | O(1) |
| `touch_other` | - | O(1) |
| `refund` | - | O(1) |
| `refund_other` | - | O(1) |
| `set_reserves` | `n in 0..MAX_RESERVES` | O(n) — çok hafif |

### Ölçülen Weight Değerleri (weights.rs — Production)
Kitchensink runtime, 2026-02-10, Intel Xeon @ 2.60GHz, steps=50, repeat=20:

| Extrinsic | Min Süre | DB Reads | DB Writes |
|-----------|----------|----------|-----------|
| `create` | 37,320,000 ps | 3 | 2 |
| `force_create` | 15,517,000 ps | 2 | 1 |
| `start_destroy` | 18,207,000 ps | 1 | 1 |
| `destroy_accounts(c)` | base 20,589,000 ps + 16,635,717*c | 2+2c | 1+2c |
| `destroy_approvals(a)` | base 25,341,000 ps + 18,802,362*a | 2+a | 1+a |
| `finish_destroy` | 18,216,000 ps | 2 | 2 |
| `mint` | 28,835,000 ps | 2 | 2 |
| `burn` | 40,615,000 ps | 2 | 2 |
| `transfer` | 56,309,000 ps | 4 | 4 |
| `transfer_keep_alive` | 50,789,000 ps | 4 | 4 |
| `approve_transfer` | ? | 3 | 2 |
| `set_metadata(n,s)` | base 36,344,000 ps + 6,447*n + 6,609*s | 2 | 1 |
| `clear_metadata` | 36,302,000 ps | 2 | 1 |

**destroy_accounts(c)**: Her account için ~16.6M ps ek — 1000 account için ~16.6 saniye, block sınırını aşar. Bu yüzden RemoveItemsLimit ile batch yapılır.

---

## 8. Hook Sistemi

### FrozenBalance<AssetId, AccountId, Balance> (types.rs:209-240)
```rust
pub trait FrozenBalance<AssetId, AccountId, Balance> {
    fn frozen_balance(asset: AssetId, who: &AccountId) -> Option<Balance>;
    fn died(asset: AssetId, who: &AccountId);
    fn contains_freezes(asset: AssetId) -> bool;
}
```
- `frozen_balance`: account için minimum tutulması gereken frozen miktar
- `died`: account yok edildiğinde callback — harici state temizliği için
- `contains_freezes`: asset genelinde freeze var mı — `start_destroy` öncesi kontrol için

### BalanceOnHold<AssetId, AccountId, Balance> (types.rs:247-275)
```rust
pub trait BalanceOnHold<AssetId, AccountId, Balance> {
    fn balance_on_hold(asset: AssetId, who: &AccountId) -> Option<Balance>;
    fn died(asset: AssetId, who: &AccountId);
    fn contains_holds(asset: AssetId) -> bool;
}
```
- `balance_on_hold`: account için held/locked tutar — `can_decrease` hesabında kullanılır
- Hold edilen miktar, balance'dan zaten düşülmüş olarak takip edilir (mock'ta decrease_balance çağrılıyor)
- `contains_holds`: `start_destroy` öncesi kontrol

### can_increase Hook Kullanımı (functions.rs:137-173)
```rust
pub(super) fn can_increase(id, who, amount, increase_supply) -> DepositConsequence {
    // 1. Asset var mı, Destroying değil mi
    // 2. supply overflow
    // 3. Account blocked mu
    // 4. Account balance overflow
    // 5. Yeni account: min_balance kontrol
    // 6. can_accrue_consumers(who, 2) — 2 consumer slot gerekiyor
    // 7. sufficient: sufficients overflow
}
```

### can_decrease Hook Kullanımı (functions.rs:176-243)
```rust
let untouchable = frozen.saturating_sub(held).max(details.min_balance);
```
**Kritik formül**: `untouchable = max(min_balance, frozen - held)`
- Held miktar freeze'den çıkarılır (çünkü hold zaten freeze'in bir parçasıdır)
- Eğer sadece freeze: untouchable = max(ed, frozen)
- Eğer hold > frozen: untouchable = max(ed, 0) = ed

Bu formül PR #4530 ile değişmiştir: eski formül `untouchable = ed + frozen` idi, yeni = `max(ed, frozen - held)`.

---

## 9. Approvals Storage

### Struct Detayı (types.rs:82-88)
```rust
pub struct Approval<Balance, DepositBalance> {
    pub amount: Balance,
    pub deposit: DepositBalance,
}
```
Varsayılan: `#[derive(Default)]` — Default impl mevcuttur.
MaxEncodedLen: Balance + DepositBalance = genelde 8+8 = 16 byte + overhead.

### NMapKey Şeması (lib.rs:460-468)
3 ayrı key: AssetId + AccountId(owner) + AccountId(delegate)
- Tüm blake2_128_concat hashleme → prefix iteration destekli
- `drain_prefix((id,))` ile tüm asset'in approvalleri silinebilir

### Approvals Count Tracking
`AssetDetails.approvals: u32` — her add/remove'da güncellenir.
- approve_transfer (yeni): `d.approvals.saturating_inc()`
- cancel_approval: `asset_details.approvals.saturating_dec()`
- transfer_approved (remaining==0): `details.approvals.saturating_dec()`
- do_destroy_approvals: her silinen için `details.approvals.saturating_sub(1)`

**Kritik**: `saturating_inc/dec` kullanılıyor — overflow/underflow güvenli. Ama teorik olarak approvals sayısı u32::MAX'e ulaşabilirse yeni approval açılmaz (saturating_inc durur). Bu edge case test edilmemiş görünüyor.

---

## 10. Integer Precision ve Overflow

### Tüm Aritmetik İşlemler

| Konum | İşlem | Yöntem | Güvenli mi? |
|-------|--------|---------|-------------|
| `new_account` (functions.rs:73) | `d.accounts.checked_add(1)` | checked | Evet |
| `increase_balance` (functions.rs:469) | `details.supply.saturating_add(amount)` | saturating | Evet (debug_assert öncesiyle garanti) |
| `decrease_balance` (functions.rs:554) | `details.supply.saturating_sub(actual)` | saturating | Evet (debug_assert öncesiyle garanti) |
| `can_increase` (functions.rs:150-151) | `details.supply.checked_add(&amount).is_none()` | checked | Evet |
| `can_increase` (functions.rs:157-158) | `account.balance.checked_add(&amount).is_none()` | checked | Evet |
| `can_decrease` (functions.rs:188) | `details.supply.checked_sub(&amount).is_none()` | checked | Evet |
| `can_decrease` (functions.rs:206) | `account.balance.checked_sub(&amount)` | checked | Evet |
| `reducible_balance` (functions.rs:268) | `frozen.saturating_sub(held).max(min_balance)` | saturating | Evet |
| `reducible_balance` (functions.rs:271) | `account.balance.saturating_sub(untouchable)` | saturating | Evet |
| `do_approve_transfer` (functions.rs:956) | `approved.amount.saturating_add(amount)` | saturating | Evet — ama approval miktarı overflow olursa sessizce u64::MAX olur! |
| `do_transfer_approved` (functions.rs:1022) | `approved.amount.checked_sub(&amount)` | checked | Evet |
| `calc_metadata_deposit` (functions.rs:1102) | `MetadataDepositPerByte * (name+symbol len)` | saturating | Evet |
| `AssetDetails.accounts` | `saturating_inc/dec` | saturating | Evet |
| `AssetDetails.sufficients` | `saturating_inc/dec` | saturating | Evet |
| `AssetDetails.approvals` | `saturating_inc/dec` | saturating | Evet |

**Potansiyel Sorun**: `do_approve_transfer`'de `approved.amount.saturating_add(amount)` kullanılıyor. Eğer bir kullanıcı aynı delegate için çok sayıda küçük approve_transfer çağrısı yaparsa, amount u64::MAX'e saturate olur. Bu "onayı iptal edememe" sorununa yol açmaz (cancel her zaman çalışır), ama anlamsal bir tuhaflık yaratır.

### Balance Türü Kısıtı
`T::Balance: AtLeast32BitUnsigned` — en az 32-bit unsigned. Genelde u128 kullanılır.

---

## 11. Error Listesi

| Error | Konum | Tetikleme Koşulu |
|-------|-------|------------------|
| `BalanceLow` | can_decrease, prep_debit | Bakiye yetersiz |
| `NoAccount` | decrease_balance, transfer | Account mevcut değil |
| `NoPermission` | çeşitli | Yetki eksikliği |
| `Unknown` | Asset/Approval bulunamadı | get() → None |
| `Frozen` | can_decrease | Account veya asset frozen |
| `InUse` | do_force_create, do_finish_destroy | Asset id kullanımda; veya accounts/approvals > 0 |
| `BadWitness` | (eski API kalıntısı) | Artık kullanılmıyor |
| `MinBalanceZero` | create, force_create | min_balance == 0 |
| `UnavailableConsumer` | new_account | Consumer ref artırılamıyor |
| `BadMetadata` | do_set_metadata, force_set_metadata | Name/symbol StringLimit aşıyor |
| `Unapproved` | do_transfer_approved | Approval yok veya miktar aşıldı |
| `WouldDie` | can_decrease | keep_alive=true, balance minimum altına düşer |
| `AlreadyExists` | do_touch | Account zaten mevcut |
| `NoDeposit` | do_refund, do_refund_other | DepositHeld/Consumer reason yok |
| `WouldBurn` | do_refund, do_refund_other | allow_burn=false ama bakiye > 0 |
| `LiveAsset` | start_destroy'u Live'dan başlatma hatası | (kullanılmıyor gibi, kontrol edilmeli) |
| `AssetNotLive` | Live gereken işlemlerde Frozen/Destroying asset | |
| `IncorrectStatus` | Destroying'de Live beklenen işlemler | |
| `NotFrozen` | thaw_asset: asset frozen değil | |
| `CallbackFailed` | CallbackHandle::created/destroyed hata | |
| `BadAssetId` | NextAssetId mevcutken farklı id | |
| `ContainsFreezes` | start_destroy | Asset'te aktif freeze var |
| `ContainsHolds` | start_destroy | Asset'te aktif hold var |
| `TooManyReserves` | set_reserves | MAX_RESERVES (5) aşıldı |
| `IncompleteDepositTransfer` | transfer_ownership | Lock/freeze deposit transferi engelledi |

---

## 12. Event Listesi

| Event | Ne Zaman |
|-------|----------|
| `Created` | create() başarılı |
| `ForceCreated` | force_create() başarılı |
| `Issued` | do_mint(), done_mint_into() |
| `Transferred` | transfer_and_die(), done_transfer() |
| `Burned` | do_burn(), do_refund() (bakiye varsa), done_burn_from() |
| `TeamChanged` | set_team() |
| `OwnerChanged` | transfer_ownership() |
| `Frozen` | freeze() |
| `Thawed` | thaw() |
| `AssetFrozen` | freeze_asset() |
| `AssetThawed` | thaw_asset() |
| `AccountsDestroyed` | do_destroy_accounts() |
| `ApprovalsDestroyed` | do_destroy_approvals() |
| `DestructionStarted` | do_start_destroy() |
| `Destroyed` | do_finish_destroy() |
| `MetadataSet` | do_set_metadata(), force_set_metadata() |
| `MetadataCleared` | clear_metadata(), force_clear_metadata() |
| `ApprovedTransfer` | do_approve_transfer() |
| `ApprovalCancelled` | do_cancel_approval() |
| `TransferredApproved` | (tanımlı ama emit edilmiyor?) |
| `AssetStatusChanged` | force_asset_status() |
| `AssetMinBalanceChanged` | set_min_balance() |
| `Touched` | do_touch() |
| `Blocked` | block() |
| `Deposited` | done_deposit() (Balanced trait) |
| `Withdrawn` | done_withdraw() (Balanced trait) |
| `ReservesUpdated` | unchecked_update_reserves() — not empty |
| `ReservesRemoved` | unchecked_update_reserves() — empty |
| `IssuedCredit` | done_issue() (Balanced trait) |
| `BurnedCredit` | DecreaseIssuanceWithEvent::handle() |
| `IssuedDebt` | done_rescind() (Balanced trait) |
| `BurnedDebt` | IncreaseIssuanceWithEvent::handle() |

**Dikkat**: `TransferredApproved` event'i struct olarak tanımlı (lib.rs:662-668) ama kaynak kodda emit edildiği yer bulunamadı. `do_transfer_approved` içinde `transfer_and_die` → `Transferred` event emit edilir. Bu bir bug ya da tasarım kararı olabilir.

---

## DEX Uygulama Notu (ZORUNLU)

### Approval Sistemi DEX Entegrasyonu

pallet-assets'in approval sistemi, ERC-20 tarzı approve/transferFrom mekanizmasına birebir karşılık gelir. Ancak önemli farklar ve dikkate alınması gereken noktalar:

**1. ADDITIVE Approval Yapısı**
`do_approve_transfer` mevcut approval'a ekler, sıfırlamaz. ERC-20'de `approve(0)` → sonra `approve(amount)` yaygın bir race condition güvenliği olarak kullanılır. Bu pallet'te `cancel_approval` + `approve_transfer` gerekir. DEX'in spender kontratı, çift ödeme riskini önlemek için bu farkı anlamalıdır.

**2. Deposit Gereksinimi**
Her approval için `T::ApprovalDeposit` miktarında reserve gerekir. DEX router bu deposit'i karşılayabilmek için yeterli native balance'a ihtiyaç duyar. Kullanıcı arayüzü bunu UX'e yansıtmalıdır.

**3. fungibles::approvals::Inspect/Mutate Kullanımı**
DEX pallet'i `fungibles::approvals::Inspect::allowance()` ile herhangi bir asset için izin miktarını kontrol edebilir. `fungibles::approvals::Mutate::transfer_from()` ile de transfer gerçekleştirebilir. Bu soyutlamalar sayesinde DEX, asset tipine bağımsız çalışabilir.

**4. Transfer Onaylarının Sıfırlanması**
`transfer_approved` ile tüm onaylı miktar kullanılırsa otomatik temizleme yapılır (deposit unreserve + approvals count güncelleme). DEX yönlendirici mantığı bu davranışı varsayabilir: tam kullanım sonrası `allowance` == 0 olur.

### Metadata DEX Entegrasyonu

**1. Token Bilgisi Okuma**
DEX arayüzü, `fungibles::metadata::Inspect` trait'i üzerinden `name()`, `symbol()`, `decimals()` okuyabilir. `ValueQuery` kullanıldığından, metadata olmayan token için boş string ve 0 decimal döner. DEX UI bu durumu ele almalıdır.

**2. Decimal Normalizasyon**
`decimals: u8` (0-255). DEX fiyat hesaplaması için decimal normalizasyonu şarttır:
```
normalized_price = price * 10^(decimals_A - decimals_B)
```
Örnek: Asset A = 6 decimal, Asset B = 18 decimal → fiyat 10^12 faktörüyle çarpılmalı.

**3. is_frozen Metadata**
`force_set_metadata` ile metadata dondurulabilir. DEX'in kendi pool verileri için pallet-assets metadata'sını kullanması durumunda, `is_frozen=true` olan metadata alanları güncelleme yapılamaz. Pool konfigürasyonu için metadata'ya bağımlı olmak risklidir.

### Pool Token Hesap Yönetimi

**touch() ve refund() Pattern'i**
DEX'in LP token'ları (pallet-assets kullanıyorsa) için, LP almak isteyen kullanıcıların önce `touch()` çağırması gerekebilir (is_sufficient=false token'lar için). Bu ekstra TX demektir. Alternatif:
- `touch_other()` ile DEX pallet'i, kullanıcı adına hesap açabilir (deposit DEX'ten alınır)
- `refund_other()` ile deposit geri alınabilir

### Destroy Prosedürünün DEX Etkisi

Bir asset destroy edilirken (`Destroying` state), tüm işlemler başarısız olur:
- `transfer` → `AssetNotLive`
- `approve_transfer` → `AssetNotLive`
- `mint` → `AssetNotLive`

DEX, işlem öncesi `Asset::get(id).status == Live` kontrolü yapmalıdır. `fungibles::Inspect::asset_exists()` yeterli değildir — `Destroying` state'inde asset var ama işlemler başarısız olur.

### Kritik Güvenlik Notu

`do_transfer_approved` içinde `transfer_and_die` çağrısı, `try_mutate_exists` içinde yapılır. Bu kritik: transfer ve approval güncelleme atomik. Kısmi transfer durumunda (başarısız transfer), approval da değişmez. DEX için re-entrancy riski minimumdur.

---

## Kritik Bulgular Özeti

1. **ADDITIVE Approval** (functions.rs:956): `approve_transfer` mevcut onayı artırır, sıfırlamaz. ERC-20'den farklı davranış. DEX'in yarış koşulunu önlemek için `cancel_approval` → `approve_transfer` sırasını kullanması gerekir.

2. **Frozen Metadata Kilidi** (functions.rs:1069): `force_set_metadata(is_frozen=true)` sonrası `set_metadata` çağrısı `NoPermission` verir. Sadece `force_set_metadata` ile güncelleme yapılabilir. Admin olmayan DEX pool'ları için tehlikeli.

3. **TransferredApproved Event Eksikliği**: `TransferredApproved` event tanımlı (lib.rs:662) ama `do_transfer_approved` içinde emit edilmiyor. Transfer event `Transferred` olarak emit ediliyor. Off-chain indexer'lar için semantic sorun.

4. **untouchable Formülü Değişikliği** (functions.rs:228): `untouchable = max(min_balance, frozen - held)` — eski `ed + frozen` değil. Bu PR #4530 ile değişti. Harici hesaplama yapan sistemler eski formülü kullanıyorsa yanlış sonuç verir.

5. **ValueQuery Metadata** (lib.rs:478): `Metadata` storage `ValueQuery` kullanıyor. `metadata::Inspect::decimals()` için asset yoksa 0 döner — hata vermez. DEX decimal normalizasyonu sırasında gizli hatalara yol açabilir.

6. **Destroy'da Hold/Freeze Engelleyici** (functions.rs:804-805): `start_destroy` öncesinde tüm account'lardaki hold ve freeze temizlenmelidir. DEX, bir token'i yok etmeden önce tüm hold'ları (`BalanceOnHold`) ve freeze'leri (`FrozenBalance`) temizlemek zorundadır.

7. **4-TX Destroy Süreci** (functions.rs:794-925): Asset yok etme 4 ayrı dispatch gerektirir ve birden fazla blokta yayılabilir. `Destroying` state'inde transfer yapılamaz. DEX, bu geçiş durumunu açıkça yönetmelidir.

8. **Deposit Zinciri: touch→refund** (functions.rs:342-450): `touch_other` ile açılan hesaplar sadece depositor veya admin tarafından `refund_other` ile kapatılabilir. DEX LP hesabı açtıysa (touch_other), LP çıkarıldıktan sonra refund_other ile deposit geri almalıdır.

9. **approve_transfer Saturation Riski** (functions.rs:956): `saturating_add` kullanıldığından, tekrarlı approve çağrıları amount'u u64::MAX'e saturate edebilir. Cancel her zaman çalışır ama miktarı anlamsızlaşır.

10. **MAX_RESERVES = 5** (lib.rs:264): Her asset için maksimum 5 reserve lokasyonu. DEX pool'ları için reserve bilgisi saklanmak isteniyorsa bu limit dikkate alınmalıdır. `set_reserves` weight O(n) olup n=5'te ihmal edilebilir düzeyde.
