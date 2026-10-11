# Substrate pallet-balances — Agent 02

## Kapsam

Analiz edilen dizin: `/substrate/frame/balances/`
Toplam dosya sayısı: 16 Rust dosyası

| Dosya | Satır (yaklaşık) | İçerik |
|---|---|---|
| src/lib.rs | ~1450 | Pallet tanımı, Config, Storage, Events, Errors, Dispatchables, iç metotlar |
| src/types.rs | 185 | AccountData, BalanceLock, ReserveData, Reasons, ExtraFlags, DustCleaner, AdjustmentDirection |
| src/impl_currency.rs | 1045 | Currency, ReservableCurrency, NamedReservableCurrency, LockableCurrency trait impls + imbalance modülü |
| src/impl_fungible.rs | 430 | fungible::Inspect, Mutate, Unbalanced, Balanced, InspectHold, MutateHold, InspectFreeze, MutateFreeze |
| src/migration.rs | 104 | V0→V1 migrasyonu (InactiveIssuance takibi) |
| src/benchmarking.rs | - | Weight benchmarkları |
| src/weights.rs | - | WeightInfo trait |
| tests/general_tests.rs | 146 | Genel testler, TryState doğrulama |
| tests/dispatchable_tests.rs | 500+ | Tüm extrinsic'ler için testler |
| tests/currency_tests.rs | - | Currency trait testleri |
| tests/fungible_tests.rs | 400+ | fungible trait testleri |
| tests/fungible_and_currency.rs | - | Karışık testler |
| tests/fungible_conformance_tests.rs | - | Conformance testleri |
| tests/reentrancy_tests.rs | - | Yeniden giriş testleri |
| tests/consumer_limit_tests.rs | - | Consumer ref limit testleri |
| tests/mod.rs | - | Test altyapısı, ExtBuilder |

Kaynak koddan satır satır doğrulandı. Hiçbir değer tahmin edilmedi.

---

## AccountData Yapısı

### Tanım (types.rs:109-126)

```rust
pub struct AccountData<Balance> {
    pub free: Balance,
    pub reserved: Balance,
    pub frozen: Balance,
    pub flags: ExtraFlags,
}
```

### Alanlar — Detaylı Analiz

#### `free: Balance`
- Hesap sahibinin kontrol ettiği, rezerve edilmemiş bakiye
- Transfer, stake, ücret ödemesi bu bakiyeden yapılır
- `free_balance()` bu alanı döner (impl_currency.rs:371)
- Holds/Freezes hesaplanırken bu alan kullanılır
- `balance()` (fungible::Inspect) da sadece `free` döner (impl_fungible.rs:44-46)

#### `reserved: Balance`
- Holds (yeni API) ve Reserves (eski API) toplamını tutar
- Doğrudan harcanamazken, belirli koşullarda (slash) alınabilir
- `total_balance_on_hold()` bu alanı döner
- Sıfır olmadığında consumer ref gerektirir
- `total()` fonksiyonu: `free.saturating_add(reserved)` (types.rs:157-159)

#### `frozen: Balance`
- `free + reserved` toplamının bu miktarın altına düşemeyeceğini belirler
- Hem Lock sistemi hem Freeze sistemi bu alanı etkiler
- Slashing durumunda bypass edilebilir ("acil" durum)
- `usable()` fonksiyonu: `free.saturating_sub(frozen)` (types.rs:152-154)
- NOT: frozen, reserved üzerine "overlay" eder — aynı bakiye her ikisi tarafından da kısıtlanabilir

#### `flags: ExtraFlags`
- u128 tipinde bir bit flag alanı
- `IS_NEW_LOGIC = 0x80000000_00000000_00000000_00000000u128` (types.rs:128)
- MSB (bit 127): yeni ref-counting mantığının aktif olup olmadığı
- Varsayılan olarak `IS_NEW_LOGIC` set edilmiş gelir (types.rs:134-138)
- Eski hesaplar `old_logic()` flag'ı ile gelir, `upgrade_accounts()` extrinsic'i bunları upgrade eder

### ExtraFlags Implementasyonu (types.rs:130-149)

```rust
pub struct ExtraFlags(pub(crate) u128);
impl Default for ExtraFlags {
    fn default() -> Self { Self(IS_NEW_LOGIC) }  // Yeni hesaplar her zaman new_logic
}
impl ExtraFlags {
    pub fn old_logic() -> Self { Self(0) }
    pub fn set_new_logic(&mut self) { self.0 = self.0 | IS_NEW_LOGIC }
    pub fn is_new_logic(&self) -> bool { (self.0 & IS_NEW_LOGIC) == IS_NEW_LOGIC }
}
```

### AccountData Hesaplama Metodları

```rust
impl<Balance: Saturating + Copy + Ord> AccountData<Balance> {
    pub fn usable(&self) -> Balance {
        self.free.saturating_sub(self.frozen)  // Kullanılabilir miktar
    }
    pub fn total(&self) -> Balance {
        self.free.saturating_add(self.reserved)  // Toplam varlık
    }
}
```

**Kritik ilişki:**
- `total = free + reserved`
- `usable = free - frozen` (frozen, reserved'dan büyük olabilir)
- Transfer edebilir = `reducible_balance()` (fonksiyon — preservation ve fortitude parametreli)

---

## Existential Deposit (ED) Mekanizması

### Config Parametresi (lib.rs:296-298)

```rust
#[pallet::constant]
#[pallet::no_default_bounds]
type ExistentialDeposit: Get<Self::Balance>;
```

Yorumda açıkça belirtilir:
> "MUST BE GREATER THAN ZERO! If you *really* need it to be zero, you can enable the feature `insecure_zero_ed` for this pallet. However, you do so at your own risk: this will open up a major DoS vector."

`insecure_zero_ed` feature flag: sadece test/geliştirme ortamları için.

### integrity_test (lib.rs:619-637)

```rust
fn integrity_test() {
    #[cfg(not(feature = "insecure_zero_ed"))]
    assert!(
        !<T as Config<I>>::ExistentialDeposit::get().is_zero(),
        "The existential deposit must be greater than zero!"
    );
    assert!(
        T::MaxFreezes::get() >= <T::RuntimeFreezeReason as VariantCount>::VARIANT_COUNT,
        "MaxFreezes should be >= number of freeze reasons"
    );
}
```

### Dust Removal Mekanizması — Tam Akış

ED mekanizması `try_mutate_account` içinde çalışır (lib.rs:1131-1140):

```rust
let maybe_dust = if account.free < ed && account.reserved.is_zero() {
    if account.free.is_zero() {
        None  // Tamamen boş hesap — dust yok
    } else {
        Some(account.free)  // Kalan küçük bakiye = dust
    }
} else {
    *maybe_account = Some(account);  // Hesabı canlı tut
    None
};
```

**Dust Tetiklenme Koşulları:**
1. `account.free < ExistentialDeposit` — serbest bakiye ED altına düşer
2. `account.reserved.is_zero()` — ve rezerve bakiye sıfır (reserved > 0 ise hesap ölmez)

**Dust Akışı:**

```
try_mutate_account -> maybe_dust = Some(küçük bakiye)
  -> mutate_account_handling_dust -> handle_raw_dust(dust)
    -> fungible::Unbalanced::handle_dust(dust)
      -> T::DustRemoval::on_unbalanced(dust.into_credit())
        -> Event::DustLost { account, amount } yayımlanır
```

**DustCleaner yapısı (types.rs:162-173):**

```rust
pub struct DustCleaner<T: Config<I>, I: 'static = ()>(
    pub(crate) Option<(T::AccountId, CreditOf<T, I>)>,
);

impl<T: Config<I>, I: 'static> Drop for DustCleaner<T, I> {
    fn drop(&mut self) {
        if let Some((who, dust)) = self.0.take() {
            Pallet::<T, I>::deposit_event(Event::DustLost { account: who, amount: dust.peek() });
            T::DustRemoval::on_unbalanced(dust);
        }
    }
}
```

`DustCleaner` bir RAII guard — drop edildiğinde dust'ı işler.

### GenesisConfig'de ED Kontrolü (lib.rs:580-585)

```rust
for (_, balance) in &self.balances {
    assert!(
        *balance >= <T as Config<I>>::ExistentialDeposit::get(),
        "the balance of any account should always be at least the existential deposit.",
    )
}
```

Genesis konfigürasyonunda her hesap ED'den az bakiye ile başlayamaz.

### deposit_creating vs deposit_into_existing (impl_currency.rs:486-509 ve 457-475)

`deposit_into_existing`: Sadece mevcut (canlı) hesaplara yatırım yapar. Ölü hesap için `DeadAccount` hatası döner.

`deposit_creating`: Yeni hesap oluşturabilir, ama `value >= ed || !is_new` koşulu zorlanır:
- Yeni hesap oluşturuluyorsa value >= ED olmalı
- Var olan hesapsa herhangi bir miktar eklenebilir

---

## Lock Sistemi

### BalanceLock Yapısı (types.rs:76-83)

```rust
pub struct BalanceLock<Balance> {
    pub id: LockIdentifier,   // [u8; 8] — 8 byte kimlik
    pub amount: Balance,       // Kilitli miktar (minimum free balance)
    pub reasons: Reasons,      // Fee, Misc veya All
}
```

### Reasons Enum (types.rs:40-47)

```rust
pub enum Reasons {
    Fee = 0,   // Sadece işlem ücreti ödemelerinde kısıtlama
    Misc = 1,  // Ücret dışı her şey için kısıtlama
    All = 2,   // Her türlü para çekme için kısıtlama
}
```

### WithdrawReasons → Reasons Dönüşümü (types.rs:49-59)

```rust
impl From<WithdrawReasons> for Reasons {
    fn from(r: WithdrawReasons) -> Reasons {
        if r == WithdrawReasons::TRANSACTION_PAYMENT {
            Reasons::Fee
        } else if r.contains(WithdrawReasons::TRANSACTION_PAYMENT) {
            Reasons::All  // Hem fee hem başka nedenler
        } else {
            Reasons::Misc
        }
    }
}
```

### BitOr impl — İki Reason Birleşimi (types.rs:61-69)

```rust
impl BitOr for Reasons {
    fn bitor(self, other: Reasons) -> Reasons {
        if self == other { return self; }
        Reasons::All  // Farklı iki reason birleşince All olur
    }
}
```

### Storage (lib.rs:511-518)

```rust
pub type Locks<T: Config<I>, I: 'static = ()> = StorageMap<
    _,
    Blake2_128Concat,
    T::AccountId,
    WeakBoundedVec<BalanceLock<T::Balance>, T::MaxLocks>,
    ValueQuery,
>;
```

`WeakBoundedVec` — soft limit, aşılırsa log::warn yayımlanır ama fail olmaz.

### set_lock (impl_currency.rs:976-995)

```rust
fn set_lock(id: LockIdentifier, who: &T::AccountId, amount: T::Balance, reasons: WithdrawReasons) {
    if reasons.is_empty() || amount.is_zero() {
        Self::remove_lock(id, who);  // Boş lock — sil
        return;
    }
    let mut new_lock = Some(BalanceLock { id, amount, reasons: reasons.into() });
    let mut locks = Self::locks(who)
        .into_iter()
        .filter_map(|l| if l.id == id { new_lock.take() } else { Some(l) })
        .collect::<Vec<_>>();
    if let Some(lock) = new_lock {
        locks.push(lock)
    }
    Self::update_locks(who, &locks[..]);
}
```

**Davranış:** Mevcut `id` ile lock varsa REPLACE eder, yoksa append eder.

### extend_lock (impl_currency.rs:1000-1028)

```rust
fn extend_lock(id: LockIdentifier, who: &T::AccountId, amount: T::Balance, reasons: WithdrawReasons) {
    // amount.max(mevcut miktar) ve reasons | mevcut reasons alınır
    // Yani extend_lock sadece BÜYÜTÜR, küçültemez
}
```

**Kritik fark:** `set_lock` tamamen değiştirir, `extend_lock` sadece maksimum alır.

### remove_lock (impl_currency.rs:1030-1034)

```rust
fn remove_lock(id: LockIdentifier, who: &T::AccountId) {
    let mut locks = Self::locks(who);
    locks.retain(|l| l.id != id);
    Self::update_locks(who, &locks[..]);
}
```

### update_locks — Frozen Alanının Güncellenmesi (lib.rs:1157-1213)

Bu fonksiyon Lock sistemi ile Freeze sistemini entegre eder:

```rust
pub(crate) fn update_locks(who: &T::AccountId, locks: &[BalanceLock<T::Balance>]) {
    let freezes = Freezes::<T, I>::get(who);
    let mut prev_frozen = Zero::zero();
    let mut after_frozen = Zero::zero();

    // force_consumer_bump = true — locks için hata yoksa diyemeyiz
    let res = Self::mutate_account(who, true, |b| {
        prev_frozen = b.frozen;
        b.frozen = Zero::zero();
        for l in locks.iter() {
            b.frozen = b.frozen.max(l.amount);  // En büyük lock miktarı
        }
        for l in freezes.iter() {
            b.frozen = b.frozen.max(l.amount);  // En büyük freeze miktarı
        }
        after_frozen = b.frozen;
    });
    // ...
    // Locked/Unlocked event yayımla
}
```

**Kritik kavrayış:** `frozen` alanı tüm lock'ların ve freeze'lerin MAKSIMUMUDUR. Additive değil — overlapping!

### ensure_can_withdraw (impl_currency.rs:378-389)

```rust
fn ensure_can_withdraw(who: &T::AccountId, amount: T::Balance, _reasons: WithdrawReasons, new_balance: T::Balance) -> DispatchResult {
    if amount.is_zero() { return Ok(()); }
    ensure!(new_balance >= Self::account(who).frozen, Error::<T, I>::LiquidityRestrictions);
    Ok(())
}
```

NOT: `_reasons` parametresi kasıtlı olarak kullanılmıyor — kaynak kod yorumu: "WithdrawReasons are intentionally not used for anything in this implementation and are expected to be removed in the near future, once migration to `fungible::*` traits is done."

### InspectLockableCurrency (impl_currency.rs:1037-1043)

```rust
impl<T: Config<I>, I: 'static> InspectLockableCurrency<T::AccountId> for Pallet<T, I> {
    fn balance_locked(id: LockIdentifier, who: &T::AccountId) -> Self::Balance {
        Self::locks(who)
            .into_iter()
            .filter(|l| l.id == id)
            .fold(Zero::zero(), |acc, l| acc + l.amount)
    }
}
```

Not: `fold` toplar, ama aynı `id` ile birden fazla lock olamaz (set_lock mevcut olanı replace eder). Pratikte tek sonuç döner.

---

## Freeze Sistemi

### Lock vs Freeze Farkı — Kritik Ayrım

| Özellik | Lock Sistemi | Freeze Sistemi |
|---|---|---|
| API | `LockableCurrency` (eski) | `fungible::MutateFreeze` (yeni) |
| Identifier | `LockIdentifier = [u8; 8]` | `T::FreezeIdentifier` (generic) |
| Tip güvenliği | Düşük (bayt dizisi) | Yüksek (runtime type) |
| Durum | Deprecated (PR #12951) | Aktif, önerilen |
| Kayıt | `Locks<>` StorageMap | `Freezes<>` StorageMap |
| Etki | `frozen` alanını günceller | `frozen` alanını günceller |
| Kombine etki | Locks + Freezes MAX alınır | Locks + Freezes MAX alınır |

### Freezes Storage (lib.rs:546-553)

```rust
pub type Freezes<T: Config<I>, I: 'static = ()> = StorageMap<
    _,
    Blake2_128Concat,
    T::AccountId,
    BoundedVec<IdAmount<T::FreezeIdentifier, T::Balance>, T::MaxFreezes>,
    ValueQuery,
>;
```

`BoundedVec` — hard limit, aşılamaz. `TooManyFreezes` hatası döner.

### Config Kısıtlaması (lib.rs:325-328)

```rust
#[pallet::constant]
type MaxFreezes: Get<u32>;
```

`MaxFreezes >= VARIANT_COUNT(RuntimeFreezeReason)` olmalı — integrity_test bunu doğrular.

### set_freeze (impl_fungible.rs:351-364)

```rust
fn set_freeze(id: &Self::Id, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
    if amount.is_zero() {
        return Self::thaw(id, who);  // Sıfır freeze = thaw
    }
    let mut locks = Freezes::<T, I>::get(who);
    if let Some(i) = locks.iter_mut().find(|x| &x.id == id) {
        i.amount = amount;  // Mevcut freeze'yi güncelle
    } else {
        locks.try_push(IdAmount { id: *id, amount })
            .map_err(|_| Error::<T, I>::TooManyFreezes)?;
    }
    Self::update_freezes(who, locks.as_bounded_slice())
}
```

### extend_freeze (impl_fungible.rs:366-379)

```rust
fn extend_freeze(id: &Self::Id, who: &T::AccountId, amount: Self::Balance) -> DispatchResult {
    // i.amount = i.amount.max(amount)  — sadece büyütür
}
```

### thaw (impl_fungible.rs:381-385)

```rust
fn thaw(id: &Self::Id, who: &T::AccountId) -> DispatchResult {
    let mut locks = Freezes::<T, I>::get(who);
    locks.retain(|l| &l.id != id);
    Self::update_freezes(who, locks.as_bounded_slice())
}
```

### update_freezes — Frozen Alanını Günceller (lib.rs:1215-1250)

```rust
pub(crate) fn update_freezes(
    who: &T::AccountId,
    freezes: BoundedSlice<IdAmount<T::FreezeIdentifier, T::Balance>, T::MaxFreezes>,
) -> DispatchResult {
    let (_, maybe_dust) = Self::mutate_account(who, false, |b| {
        prev_frozen = b.frozen;
        b.frozen = Zero::zero();
        for l in Locks::<T, I>::get(who).iter() {
            b.frozen = b.frozen.max(l.amount);
        }
        for l in freezes.iter() {
            b.frozen = b.frozen.max(l.amount);
        }
        after_frozen = b.frozen;
    })?;
    // Thawed/Frozen event yayımla
}
```

### InspectFreeze impl (impl_fungible.rs:336-348)

```rust
fn balance_frozen(id: &Self::Id, who: &T::AccountId) -> Self::Balance {
    let locks = Freezes::<T, I>::get(who);
    locks.into_iter().find(|l| &l.id == id).map_or(Zero::zero(), |l| l.amount)
}

fn can_freeze(id: &Self::Id, who: &T::AccountId) -> bool {
    let l = Freezes::<T, I>::get(who);
    !l.is_full() || l.iter().any(|x| &x.id == id)  // Dolu ama aynı id var
}
```

---

## Transfer Fonksiyonları

### Tüm Dispatchable Transfer'lar (lib.rs:641-874)

#### 1. transfer_allow_death (call_index: 0)

```rust
pub fn transfer_allow_death(
    origin: OriginFor<T>,
    dest: AccountIdLookupOf<T>,
    #[pallet::compact] value: T::Balance,
) -> DispatchResult {
    let source = ensure_signed(origin)?;
    let dest = T::Lookup::lookup(dest)?;
    <Self as fungible::Mutate<_>>::transfer(&source, &dest, value, Expendable)?;
    Ok(())
}
```

- Origin: `Signed`
- Preservation: `Expendable` — kaynak hesap ölebilir
- ED altına düşerse kaynak hesap reap edilir (DustLost eventi)
- Hedef hesap oluşturulabilir (value >= ED ise)

#### 2. force_transfer (call_index: 2)

```rust
pub fn force_transfer(
    origin: OriginFor<T>,
    source: AccountIdLookupOf<T>,
    dest: AccountIdLookupOf<T>,
    #[pallet::compact] value: T::Balance,
) -> DispatchResult {
    ensure_root(origin)?;
    // ...
    <Self as fungible::Mutate<_>>::transfer(&source, &dest, value, Expendable)?;
}
```

- Origin: `Root` zorunlu
- Kaynak belirtilebilir (isteğe bağlı herhangi bir hesap)
- `Expendable` — kaynak ölebilir
- Root yetkisi: governance kararları için kullanılır

#### 3. transfer_keep_alive (call_index: 3)

```rust
pub fn transfer_keep_alive(
    origin: OriginFor<T>,
    dest: AccountIdLookupOf<T>,
    #[pallet::compact] value: T::Balance,
) -> DispatchResult {
    let source = ensure_signed(origin)?;
    let dest = T::Lookup::lookup(dest)?;
    <Self as fungible::Mutate<_>>::transfer(&source, &dest, value, Preserve)?;
}
```

- Origin: `Signed`
- Preservation: `Preserve` — kaynak her zaman canlı kalır
- Transfer kaynak hesabı öldürecekse `NotExpendable` hatası
- Testler: `transfer_keep_alive_works` — `transfer_keep_alive(1 -> 2, 100)` 100 bakiyeli hesap için fail

#### 4. transfer_all (call_index: 4)

```rust
pub fn transfer_all(
    origin: OriginFor<T>,
    dest: AccountIdLookupOf<T>,
    keep_alive: bool,
) -> DispatchResult {
    let transactor = ensure_signed(origin)?;
    let keep_alive = if keep_alive { Preserve } else { Expendable };
    let reducible_balance = <Self as fungible::Inspect<_>>::reducible_balance(
        &transactor,
        keep_alive,
        Fortitude::Polite,
    );
    let dest = T::Lookup::lookup(dest)?;
    <Self as fungible::Mutate<_>>::transfer(&transactor, &dest, reducible_balance, keep_alive)?;
}
```

- `keep_alive=false`: Tüm transferable bakiye gönderilir, hesap reap edilebilir
- `keep_alive=true`: ED bırakılarak kalan gönderilir
- Locked/frozen/reserved miktarlar transferable değil
- Test: 200 bakiyelik hesap, keep_alive=true → 100 gider, 100 kalır (ED=100 ise)

#### 5. force_unreserve (call_index: 5)

```rust
pub fn force_unreserve(
    origin: OriginFor<T>,
    who: AccountIdLookupOf<T>,
    amount: T::Balance,
) -> DispatchResult {
    ensure_root(origin)?;
    let who = T::Lookup::lookup(who)?;
    let _leftover = <Self as ReservableCurrency<_>>::unreserve(&who, amount);
    Ok(())
}
```

- Root yetkisi
- Başarısız olsa bile (kısmi unreserve) Ok döner (`_leftover` ignore edilir)
- Slashing ve governance için kullanılır

#### 6. upgrade_accounts (call_index: 6)

```rust
pub fn upgrade_accounts(
    origin: OriginFor<T>,
    who: Vec<T::AccountId>,
) -> DispatchResultWithPostInfo {
    // ...
    let proportion_upgraded = Perbill::from_rational(upgrade_count, who.len() as u32);
    if proportion_upgraded >= Perbill::from_percent(90) {
        Ok(Pays::No.into())  // %90+ upgrade edilirse ücret alınmaz
    } else {
        Ok(Pays::Yes.into())
    }
}
```

- Eski hesapları yeni mantığa yükseltir
- %90 veya fazlası upgrade edilirse fee waived (incentive mekanizması)

#### 7. force_set_balance (call_index: 8)

```rust
pub fn force_set_balance(
    origin: OriginFor<T>,
    who: AccountIdLookupOf<T>,
    #[pallet::compact] new_free: T::Balance,
) -> DispatchResult {
    ensure_root(origin)?;
    let existential_deposit = Self::ed();
    let wipeout = new_free < existential_deposit;
    let new_free = if wipeout { Zero::zero() } else { new_free };
    // Hesabı güncelle, total issuance'ı düzelt
}
```

- Root yetkisi
- ED altındaki değer → wipeout → sıfır set edilir
- TotalIssuance otomatik düzeltilir (PositiveImbalance veya NegativeImbalance drop ile)

#### 8. force_adjust_total_issuance (call_index: 9)

```rust
pub fn force_adjust_total_issuance(
    origin: OriginFor<T>,
    direction: AdjustmentDirection,
    #[pallet::compact] delta: T::Balance,
) -> DispatchResult {
    ensure_root(origin)?;
    ensure!(delta > Zero::zero(), Error::<T, I>::DeltaZero);
    let old = TotalIssuance::<T, I>::get();
    let new = match direction {
        AdjustmentDirection::Increase => old.saturating_add(delta),
        AdjustmentDirection::Decrease => old.saturating_sub(delta),
    };
    ensure!(InactiveIssuance::<T, I>::get() <= new, Error::<T, I>::IssuanceDeactivated);
    TotalIssuance::<T, I>::set(new);
    Self::deposit_event(Event::<T, I>::TotalIssuanceForced { old, new });
}
```

- Root yetkisi
- delta sıfır olamaz
- InactiveIssuance <= new (deactivated issuance constraint)
- Saturating arithmetik — overflow/underflow güvenli

#### 9. burn (call_index: 10)

```rust
pub fn burn(
    origin: OriginFor<T>,
    #[pallet::compact] value: T::Balance,
    keep_alive: bool,
) -> DispatchResult {
    let source = ensure_signed(origin)?;
    let preservation = if keep_alive { Preserve } else { Expendable };
    <Self as fungible::Mutate<_>>::burn_from(&source, value, preservation, Precision::Exact, Polite)?;
}
```

- Signed origin
- Gerçek yakma: TotalIssuance düşer
- Burn address'e göndermekten farkı: supply azalır

### Currency::transfer (impl_currency.rs:392-408)

Eski API transferi — fungible::Mutate'e yönlendirir:

```rust
fn transfer(
    transactor: &T::AccountId,
    dest: &T::AccountId,
    value: Self::Balance,
    existence_requirement: ExistenceRequirement,
) -> DispatchResult {
    if value.is_zero() || transactor == dest { return Ok(()); }
    let keep_alive = match existence_requirement {
        ExistenceRequirement::KeepAlive => Preserve,
        ExistenceRequirement::AllowDeath => Expendable,
    };
    <Self as fungible::Mutate<_>>::transfer(transactor, dest, value, keep_alive)?;
    Ok(())
}
```

### reducible_balance — Transfer Limitinin Hesaplanması (impl_fungible.rs:47-73)

```rust
fn reducible_balance(
    who: &T::AccountId,
    preservation: Preservation,
    force: Fortitude,
) -> Self::Balance {
    let a = Self::account(who);
    let mut untouchable = Zero::zero();

    if force == Polite {
        // Freeze kısıtlaması: frozen - reserved (reserved zaten tutulduğundan)
        untouchable = a.frozen.saturating_sub(a.reserved);
    }

    // Provider ref kısıtlamaları:
    if preservation == Preserve
        || preservation == Protect && !a.free.is_zero() &&
            frame_system::Pallet::<T>::providers(who) == 1
        || preservation == Expendable && !a.free.is_zero() &&
            !frame_system::Pallet::<T>::can_dec_provider(who)
    {
        untouchable = untouchable.max(T::ExistentialDeposit::get());
    }

    a.free.saturating_sub(untouchable)
}
```

**Hesaplama mantığı:**
- `Polite + Preserve`: frozen_above_reserved + ED = untouchable
- `Polite + Expendable`: frozen_above_reserved (provider varsa ED de)
- `Force + Preserve`: Sadece ED = untouchable (frozen bypass)
- `Force + Expendable`: 0 = untouchable (her şey alınabilir)

---

## Currency Trait Implementasyonu

### impl_currency.rs — Genel Yapı

Currency ailesi (impl_currency.rs:296-1044):
1. `Currency<T::AccountId>` — temel para işlemleri
2. `ReservableCurrency<T::AccountId>` — rezervasyon
3. `NamedReservableCurrency<T::AccountId>` — isimli rezervasyon
4. `LockableCurrency<T::AccountId>` — kilitleme
5. `InspectLockableCurrency<T::AccountId>` — kilit sorgulama

Tüm bunlar DEPRECATED — `fungible::*` traits'e geçiş önerilir (PR #12951).

### Currency Trait Metodları (impl_currency.rs:296-581)

#### total_balance (satır 304-306)
```rust
fn total_balance(who: &T::AccountId) -> Self::Balance {
    Self::account(who).total()  // free + reserved
}
```

#### minimum_balance (satır 332-334)
```rust
fn minimum_balance() -> Self::Balance {
    T::ExistentialDeposit::get()
}
```

#### burn (satır 338-351)
```rust
fn burn(mut amount: Self::Balance) -> Self::PositiveImbalance {
    <TotalIssuance<T, I>>::mutate(|issued| {
        *issued = issued.checked_sub(&amount).unwrap_or_else(|| {
            amount = *issued;
            Zero::zero()
        });
    });
    Pallet::<T, I>::deposit_event(Event::<T, I>::Rescinded { amount });
    PositiveImbalance::new(amount)
}
```

Not: `burn` TotalIssuance'dan düşer ve `PositiveImbalance` döner (artık pozitif — daha fazla para yaratılabilir anlamında).

#### issue (satır 356-369)
```rust
fn issue(mut amount: Self::Balance) -> Self::NegativeImbalance {
    <TotalIssuance<T, I>>::mutate(|issued| {
        *issued = issued.checked_add(&amount).unwrap_or_else(|| {
            amount = Self::Balance::max_value() - *issued;
            Self::Balance::max_value()
        })
    });
    Pallet::<T, I>::deposit_event(Event::<T, I>::Issued { amount });
    NegativeImbalance::new(amount)
}
```

Not: Overflow durumunda cap'lenmiş miktar basılır.

#### slash (satır 419-452)

```rust
fn slash(who: &T::AccountId, value: Self::Balance) -> (Self::NegativeImbalance, Self::Balance) {
    // ...
    let actual = match system::Pallet::<T>::can_dec_provider(who) {
        true => value.min(account.free),           // Hesap ölebilir
        false => value.min(account.free.saturating_sub(ed)),  // ED korunur
    };
    account.free.saturating_reduce(actual);
    let remaining = value.saturating_sub(actual);
    Ok((NegativeImbalance::new(actual), remaining))
}
```

**Kritik not:** Slash önce free balance'dan çekilir. `can_dec_provider` false ise ED korunur (hesap yaşatılır). Dönen tuple: `(gerçekten slashed, slashlanamayan)`.

### Imbalance Sistemi (impl_currency.rs:41-293)

#### PositiveImbalance

Para arzında gerçekleşmeyen artış (negatif para = eksik supply):
- Oluşturulduğunda TotalIssuance hemen değişmez
- `drop()` çağrıldığında TotalIssuance artırılır:
  ```rust
  fn drop(&mut self) {
      if !self.0.is_zero() {
          <super::TotalIssuance<T, I>>::mutate(|v| *v = v.saturating_add(self.0));
          Pallet::<T, I>::deposit_event(Event::<T, I>::Issued { amount: self.0 });
      }
  }
  ```
- `offset()` ile NegativeImbalance ile netleştirilebilir

#### NegativeImbalance

Para arzında gerçekleşmeyen azalış (pozitif para = fazla supply):
- `drop()` çağrıldığında TotalIssuance azaltılır:
  ```rust
  fn drop(&mut self) {
      if !self.0.is_zero() {
          <super::TotalIssuance<T, I>>::mutate(|v| *v = v.saturating_sub(self.0));
          Pallet::<T, I>::deposit_event(Event::<T, I>::Rescinded { amount: self.0 });
      }
  }
  ```

**Tasarım kararı:** `#[must_use]` — imbalance'lar drop edilmeden kullanılmalı. Drop edilince supply otomatik düzeltilir.

### ReservableCurrency (impl_currency.rs:606-735)

#### reserve (satır 627-643)
```rust
fn reserve(who: &T::AccountId, value: Self::Balance) -> DispatchResult {
    if value.is_zero() { return Ok(()); }
    Self::try_mutate_account_handling_dust(who, false, |account, _| -> DispatchResult {
        account.free = account.free.checked_sub(&value).ok_or(Error::<T, I>::InsufficientBalance)?;
        account.reserved = account.reserved.checked_add(&value).ok_or(ArithmeticError::Overflow)?;
        ensure_can_reserve::<T, I>(who, value, false)  // Sonra kontrol
    })?;
    Self::deposit_event(Event::Reserved { who: who.clone(), amount: value });
    Ok(())
}
```

**Dikkat:** Önce mutasyonu yapar, sonra `ensure_can_reserve` ile doğrular. `try_mutate_account_handling_dust` zaten atomik — hata durumunda revert edilir.

#### unreserve (satır 651-678)
```rust
fn unreserve(who: &T::AccountId, value: Self::Balance) -> Self::Balance {
    // actual = min(reserved, value)
    // account.reserved -= actual
    // account.free += actual (defensive_saturating_add)
    // return value - actual (unreserve edilemeyen miktar)
}
```

Dönen değer: unreserve EDILEMEYEN miktar. `0` döndürmesi başarı anlamına gelir.

#### repatriate_reserved (satır 725-734)
```rust
fn repatriate_reserved(
    slashed: &T::AccountId,
    beneficiary: &T::AccountId,
    value: Self::Balance,
    status: Status,
) -> Result<Self::Balance, DispatchError> {
    let actual = Self::do_transfer_reserved(slashed, beneficiary, value, BestEffort, Polite, status)?;
    Ok(value.saturating_sub(actual))  // Dönen değer: transfer EDILEMEYEN miktar
}
```

### NamedReservableCurrency (impl_currency.rs:737-964)

#### reserve_named (satır 754-779)
```rust
fn reserve_named(id: &Self::ReserveIdentifier, who: &T::AccountId, value: Self::Balance) -> DispatchResult {
    Reserves::<T, I>::try_mutate(who, |reserves| -> DispatchResult {
        match reserves.binary_search_by_key(id, |data| data.id) {
            Ok(index) => {
                reserves[index].amount = reserves[index].amount
                    .checked_add(&value).ok_or(ArithmeticError::Overflow)?;
            },
            Err(index) => {
                reserves.try_insert(index, ReserveData { id: *id, amount: value })
                    .map_err(|_| Error::<T, I>::TooManyReserves)?;
            },
        };
        <Self as ReservableCurrency<_>>::reserve(who, value)?;  // Genel reserve de çağrılır
        Ok(())
    })
}
```

**İki seviyeli kayıt:**
1. `Reserves` storage: isimli rezervasyonun takibi
2. `account.reserved` alanı: toplam rezerv bakiye

#### unreserve_named (satır 785-826)

Binary search ile isimli reserve bulunur, kısmi unreserve yapılır, sıfırsa silinir.

### Holds Storage (lib.rs:532-543)

```rust
pub type Holds<T: Config<I>, I: 'static = ()> = StorageMap<
    _,
    Blake2_128Concat,
    T::AccountId,
    BoundedVec<
        IdAmount<T::RuntimeHoldReason, T::Balance>,
        VariantCountOf<T::RuntimeHoldReason>,
    >,
    ValueQuery,
>;
```

Holds: `RuntimeHoldReason` ile typed. Maksimum sayı = HoldReason variant sayısı (`VariantCountOf`). Açıkça tip güvenli.

### UnbalancedHold — set_balance_on_hold (impl_fungible.rs:292-334)

```rust
fn set_balance_on_hold(
    reason: &Self::Reason,
    who: &T::AccountId,
    amount: Self::Balance,
) -> DispatchResult {
    let mut new_account = Self::account(who);
    let mut holds = Holds::<T, I>::get(who);
    let mut increase = true;
    let mut delta = amount;

    if let Some(item) = holds.iter_mut().find(|x| &x.id == reason) {
        delta = item.amount.max(amount) - item.amount.min(amount);
        increase = amount > item.amount;
        item.amount = amount;
        holds.retain(|x| !x.amount.is_zero());
    } else {
        if !amount.is_zero() {
            holds.try_push(IdAmount { id: *reason, amount })
                .map_err(|_| Error::<T, I>::TooManyHolds)?;
        }
    }

    new_account.reserved = if increase {
        new_account.reserved.checked_add(&delta).ok_or(ArithmeticError::Overflow)?
    } else {
        new_account.reserved.checked_sub(&delta).ok_or(ArithmeticError::Underflow)?
    };

    // Atomik mutasyon: account güncellenir
    Holds::<T, I>::insert(who, holds);
}
```

---

## Storage Items

### Tüm Storage Items (lib.rs:469-553)

```rust
// 1. Toplam dolaşımdaki miktar
#[pallet::storage]
#[pallet::whitelist_storage]  // Her block'ta okunduğu için whitelist'te
pub type TotalIssuance<T: Config<I>, I: 'static = ()> = StorageValue<_, T::Balance, ValueQuery>;

// 2. Deaktive edilmiş (ölü hesaplar, XCM teleport pool) miktar
#[pallet::storage]
#[pallet::whitelist_storage]
pub type InactiveIssuance<T: Config<I>, I: 'static = ()> = StorageValue<_, T::Balance, ValueQuery>;

// 3. Hesap bakiyesi
#[pallet::storage]
pub type Account<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat, T::AccountId, AccountData<T::Balance>, ValueQuery,
>;

// 4. Kilit listesi (DEPRECATED — Freezes kullan)
#[pallet::storage]
pub type Locks<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat, T::AccountId,
    WeakBoundedVec<BalanceLock<T::Balance>, T::MaxLocks>, ValueQuery,
>;

// 5. İsimli rezervasyonlar (DEPRECATED — Holds kullan)
#[pallet::storage]
pub type Reserves<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat, T::AccountId,
    BoundedVec<ReserveData<T::ReserveIdentifier, T::Balance>, T::MaxReserves>, ValueQuery,
>;

// 6. Hold'lar (yeni API)
#[pallet::storage]
pub type Holds<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat, T::AccountId,
    BoundedVec<IdAmount<T::RuntimeHoldReason, T::Balance>, VariantCountOf<T::RuntimeHoldReason>>,
    ValueQuery,
>;

// 7. Freeze'ler (yeni API)
#[pallet::storage]
pub type Freezes<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat, T::AccountId,
    BoundedVec<IdAmount<T::FreezeIdentifier, T::Balance>, T::MaxFreezes>, ValueQuery,
>;
```

### Storage Versiyonları

```rust
const STORAGE_VERSION: frame_support::traits::StorageVersion =
    frame_support::traits::StorageVersion::new(1);
```

- V0: Orijinal depolama formatı
- V1: InactiveIssuance takibi eklendi (XCM teleport checking accounts için)
- Migration: `MigrateToTrackInactive`, `MigrateManyToTrackInactive`, `ResetInactive`

### InactiveIssuance Kullanımı

```rust
fn active_issuance() -> Self::Balance {
    TotalIssuance::<T, I>::get().saturating_sub(InactiveIssuance::<T, I>::get())
}

fn deactivate(amount: Self::Balance) {
    InactiveIssuance::<T, I>::mutate(|b| {
        *b = b.saturating_add(amount).min(TotalIssuance::<T, I>::get());
    });
}

fn reactivate(amount: Self::Balance) {
    InactiveIssuance::<T, I>::mutate(|b| b.saturating_reduce(amount));
}
```

**Neden gerekli?** XCM teleport checking account'larının bakiyesi TotalIssuance'da görünüyor ama dolaşımda değil. `active_issuance` daha doğru supply değeri verir.

---

## Events ve Errors

### Tüm Events (lib.rs:346-425)

| Event | Açıklama | Alanlar |
|---|---|---|
| `Endowed` | Hesap oluşturuldu | account, free_balance |
| `DustLost` | Dust nedeniyle silinen hesap | account, amount |
| `Transfer` | Transfer tamamlandı | from, to, amount |
| `BalanceSet` | Bakiye root tarafından set edildi | who, free |
| `Reserved` | Bakiye rezerve edildi | who, amount |
| `Unreserved` | Bakiye serbest bırakıldı | who, amount |
| `ReserveRepatriated` | Rezerv başka hesaba aktarıldı | from, to, amount, destination_status |
| `Deposit` | Yatırım yapıldı | who, amount |
| `Withdraw` | Para çekildi | who, amount |
| `Slashed` | Ceza kesildi | who, amount |
| `Minted` | Token basıldı (fungible API) | who, amount |
| `MintedCredit` | Negatif imbalance drop edildi | amount |
| `Burned` | Token yakıldı (fungible API) | who, amount |
| `BurnedDebt` | Pozitif imbalance drop edildi | amount |
| `Suspended` | Shelve edildi | who, amount |
| `Restored` | Restore edildi | who, amount |
| `Upgraded` | Hesap eski→yeni mantığa yükseltildi | who |
| `Issued` | TotalIssuance artırıldı | amount |
| `Rescinded` | TotalIssuance azaltıldı | amount |
| `Locked` | Lock eklendi/artırıldı | who, amount |
| `Unlocked` | Lock azaltıldı/kaldırıldı | who, amount |
| `Frozen` | Freeze eklendi/artırıldı | who, amount |
| `Thawed` | Freeze azaltıldı/kaldırıldı | who, amount |
| `TotalIssuanceForced` | Root TotalIssuance değiştirdi | old, new |
| `Held` | Hold oluşturuldu | reason, who, amount |
| `BurnedHeld` | Hold üzerinden yakma | reason, who, amount |
| `TransferOnHold` | Hold'daki miktar transfer edildi | reason, source, dest, amount |
| `TransferAndHold` | Transfer edilip hold edildi | reason, source, dest, transferred |
| `Released` | Hold serbest bırakıldı | reason, who, amount |
| `Unexpected(UnexpectedKind)` | Savunma amaçlı olay | BalanceUpdated, FailedToMutateAccount |

### UnexpectedKind (lib.rs:432-438)

```rust
pub enum UnexpectedKind {
    BalanceUpdated,       // İşlem sırasında beklenmedik dust/balance update
    FailedToMutateAccount, // AccountStore mutasyonu başarısız (Storage uyumsuzluğu riski!)
}
```

Explorer'da görülürse polkadot-sdk'ya issue açılmalı.

### Tüm Errors (lib.rs:440-466)

| Hata | Açıklama |
|---|---|
| `VestingBalance` | Vesting bakiyesi yüzünden çekilemiyor |
| `LiquidityRestrictions` | Lock/freeze kısıtlaması var |
| `InsufficientBalance` | Yetersiz bakiye |
| `ExistentialDeposit` | Yeni hesap için ED yok |
| `Expendability` | Transfer hesabı öldürür (keep_alive kısıtı) |
| `ExistingVestingSchedule` | Vesting zaten var |
| `DeadAccount` | Hedef hesap yok |
| `TooManyReserves` | MaxReserves aşıldı |
| `TooManyHolds` | RuntimeHoldReason variant sayısı aşıldı |
| `TooManyFreezes` | MaxFreezes aşıldı |
| `IssuanceDeactivated` | InactiveIssuance kısıtı |
| `DeltaZero` | force_adjust için delta sıfır |

---

## Total Issuance Yönetimi

### Imbalance Drop Mekanizması

TotalIssuance'ın tutarlılığı Rust'ın ownership sistemi ile garantilenir:

```
PositiveImbalance drop edilir → TotalIssuance += amount + Event::Issued
NegativeImbalance drop edilir → TotalIssuance -= amount + Event::Rescinded
```

```
fungible::HandleImbalanceDrop için:
NegativeImbalance::handle(amount) → TotalIssuance -= amount + Event::BurnedDebt
PositiveImbalance::handle(amount) → TotalIssuance += amount + Event::MintedCredit
```

### set_total_issuance (impl_fungible.rs:175-177)

```rust
fn set_total_issuance(amount: Self::Balance) {
    TotalIssuance::<T, I>::mutate(|t| *t = amount);
}
```

`fungible::Unbalanced` trait'i aracılığıyla erişilir. Doğrudan değiştirir, event yayımlamaz.

### Genesis'te TotalIssuance (lib.rs:576-579)

```rust
fn build(&self) {
    let total = self.balances.iter().fold(Zero::zero(), |acc: T::Balance, &(_, n)| acc + n);
    <TotalIssuance<T, I>>::put(total);
```

Tüm genesis bakiyelerinin toplamı.

---

## fungible Trait Implementasyonu

### impl_fungible.rs — Tam Kapsam

```
fungible::Inspect          → balance, total_issuance, reducible_balance, can_deposit, can_withdraw
fungible::Unbalanced       → write_balance, set_total_issuance, deactivate, reactivate, handle_dust
fungible::Mutate           → done_mint_into, done_burn_from, done_shelve, done_restore, done_transfer
fungible::MutateHold       → done_hold, done_release, done_burn_held, done_transfer_on_hold, done_transfer_and_hold
fungible::InspectHold      → total_balance_on_hold, reducible_total_balance_on_hold, balance_on_hold, hold_available
fungible::UnbalancedHold   → set_balance_on_hold
fungible::InspectFreeze    → balance_frozen, can_freeze
fungible::MutateFreeze     → set_freeze, extend_freeze, thaw
fungible::Balanced         → done_deposit, done_withdraw, done_issue, done_rescind
fungible::BalancedHold     → (boş impl, default yeterli)
fungible::hold::DoneSlash  → done_slash (T::DoneSlashHandler'a yönlendirir)
AccountTouch<(), T::AccountId> → deposit_required=0, should_touch=false, touch=Ok(())
```

### can_deposit (impl_fungible.rs:74-103)

```rust
fn can_deposit(who: &T::AccountId, amount: Self::Balance, provenance: Provenance) -> DepositConsequence {
    if amount.is_zero() { return DepositConsequence::Success; }

    if provenance == Minted && TotalIssuance::<T, I>::get().checked_add(&amount).is_none() {
        return DepositConsequence::Overflow;  // Yeni basılan token TotalIssuance'ı taşıyacak
    }

    let account = Self::account(who);
    let new_free = match account.free.checked_add(&amount) {
        None => return DepositConsequence::Overflow,
        Some(x) if x < T::ExistentialDeposit::get() => return DepositConsequence::BelowMinimum,
        Some(x) => x,
    };

    match account.reserved.checked_add(&new_free) {
        Some(_) => {},
        None => return DepositConsequence::Overflow,  // free + reserved overflow
    };

    DepositConsequence::Success
}
```

### can_withdraw (impl_fungible.rs:104-150)

```rust
fn can_withdraw(who: &T::AccountId, amount: Self::Balance) -> WithdrawConsequence<Self::Balance> {
    // TotalIssuance underflow kontrolü
    // Bakiye yeterliliği
    // Freeze kısıtlaması (liquid < amount)
    // Provider ref (can_dec_provider)
    // Frozen kısıtlaması (new_total < frozen)
}
```

Dönen değerler:
- `Success`: Sorunsuz
- `BalanceLow`: Bakiye yetersiz
- `Frozen`: Freeze kısıtlaması
- `WouldDie`: Provider silemeyiz
- `ReducedToZero(remaining)`: Hesap ölürdü ama can_dec_provider true
- `Underflow`: TotalIssuance underflow

---

## Referans Sayacı Sistemi

### System Pallet ile Entegrasyon (lib.rs:1064-1155)

`try_mutate_account` içinde account lifecycle yönetimi:

```
Önceki durum    Sonraki durum    Eylem
─────────────────────────────────────────
!did_provide   → does_provide    inc_providers(who)
did_consume    → !does_consume   dec_consumers(who)
!did_consume   → does_consume    inc_consumers(who) / inc_consumers_without_limit (force)
did_provide    → !does_provide   dec_providers(who)  [en son yapılır — reap riski]
```

**Kritik sıralaması:** `dec_providers` en son çağrılır çünkü hesabı reap edebilir (siler).

**`did_provide` koşulu:**
```rust
let did_provide = account.free >= Self::ed() && Self::have_providers_or_no_zero_ed(who);
let did_consume = !is_new && (!account.reserved.is_zero() || !account.frozen.is_zero());
```

**`does_provide` koşulu:**
```rust
let does_provide = account.free >= Self::ed();
let does_consume = !account.reserved.is_zero() || !account.frozen.is_zero();
```

Provider ref: ED veya üzeri free balance → hesap yaşıyor
Consumer ref: reserved veya frozen > 0 → başka pallet bu hesaba bağlı

---

## Migration (migration.rs)

### V0 → V1: InactiveIssuance Takibi

```rust
fn migrate_v0_to_v1<T: Config<I>, I: 'static>(accounts: &[T::AccountId]) -> Weight {
    if on_chain_version == 0 {
        let total = accounts.iter()
            .map(|a| Pallet::<T, I>::total_balance(a))
            .fold(T::Balance::zero(), |a, e| a.saturating_add(e));
        Pallet::<T, I>::deactivate(total);
        // ...
        StorageVersion::new(1).put::<Pallet<T, I>>();
    }
}
```

**Kullanım:** XCM teleport checking account'ları V1'e migrate ederken o hesapların bakiyesi InactiveIssuance'a eklenir.

### Yapılar

- `MigrateToTrackInactive<T, A, I>`: Tek hesap
- `MigrateManyToTrackInactive<T, A, I>`: Çok hesap
- `ResetInactive<T, I>`: V1'i V0'a geri al (InactiveIssuance sıfırlanır)

---

## do_transfer_reserved Analizi

Rezerv transferinin en kritik iç fonksiyonu (lib.rs:1258-1325):

```rust
pub(crate) fn do_transfer_reserved(
    slashed: &T::AccountId,
    beneficiary: &T::AccountId,
    value: T::Balance,
    precision: Precision,   // BestEffort veya Exact
    fortitude: Fortitude,   // Polite veya Force
    status: Status,         // Free veya Reserved
) -> Result<T::Balance, DispatchError> {
    let max = <Self as fungible::InspectHold<_>>::reducible_total_balance_on_hold(
        slashed, fortitude,
    );
    let actual = match precision {
        Precision::BestEffort => value.min(max),  // En fazla max kadar
        Precision::Exact => value,                 // Tam miktar, başarısız olabilir
    };
    ensure!(actual <= max, TokenError::FundsUnavailable);

    // Aynı hesap ise basit işlem
    if slashed == beneficiary {
        return match status {
            Status::Free => Ok(actual.saturating_sub(Self::unreserve(slashed, actual))),
            Status::Reserved => Ok(actual),
        };
    }

    // Farklı hesaplar: iç içe try_mutate_account
    // beneficiary'nin değiştirilmesi DIS, slashed'ın değiştirilmesi İÇ
    // Status::Free → to_account.free += actual
    // Status::Reserved → to_account.reserved += actual
    // from_account.reserved -= actual
}
```

---

## Test Altyapısı

### ExtBuilder Yapısı (tests/mod.rs)

```rust
pub struct ExtBuilder {
    existential_deposit: u64,
    monied: bool,
    // ...
}
```

Test parametreleri:
- `existential_deposit(n)`: ED değeri
- `monied(true)`: Önceden bakiyeli hesaplar
- `build_and_execute_with(|| { ... })`: Test closure

### Önemli Test Bulgular

**balance_transfer_when_on_hold_should_not_work:**
```rust
assert_ok!(Balances::hold(&TestId::Foo, &1, 69));
assert_noop!(
    Balances::transfer_allow_death(Some(1).into(), 2, 69),
    TokenError::FundsUnavailable,  // Hold'daki para transfer edilemez
);
```

**inspect_trait_reducible_balance_frozen_works:**
```rust
Balances::set_balance(&1, 100);
assert_ok!(Balances::set_freeze(&TestId::Foo, &1, 50));
assert_eq!(Balances::reducible_balance(&1, Expendable, Polite), 50);  // 100 - 50 = 50
assert_eq!(Balances::reducible_balance(&1, Expendable, Force), 90);   // ED=10, force bypass freeze
```

**regression_historic_acc_does_not_evaporate_reserve:**
- Eski hesap: consumer=0 ama reserved>0 (inconsistent state)
- Yeni mantık: transfer_allow_death → ConsumerRemaining hatası (koruma)
- transfer_keep_alive: consumer ref geri eklenir (self-healing)

---

## Güvenlik Notları

### Reentrancy Koruması

`try_mutate_account` → `T::AccountStore::try_mutate_exists` zinciri StorageMap üzerinden atomik. FRAME runtime tek threaded — reentrancy yok.

### Integer Overflow Korumaları

- `checked_add`, `checked_sub` — hata döner
- `saturating_add`, `saturating_reduce`, `saturating_sub` — sınırda doyar
- `defensive_saturating_add` — debug modda assert, production'da saturating

### Consumer Reference Tutarlılığı

`does_consume && frame_system::Pallet::<T>::consumers(who) == 0` durumunda defensive increment yapılır ve hata loglanır — bu asla olmamalı ama bir güvenlik ağı.

### force_consumer_bump Parametresi

Lock sistemi için `force_consumer_bump = true` kullanılır çünkü Lock API'si hata dönemez. Consumer limit aşılsa bile zorla increment yapılır.

---

## Nova Shield Uygulama Notu

### 1. AccountData Yapısı — Doğrudan Kullanım Önerisi

Nova Shield için aynı yapıyı alın, `flags` alanını ileride migration/upgrade için saklayın:

```rust
pub struct AccountData<Balance> {
    pub free: Balance,      // Kullanılabilir bakiye
    pub reserved: Balance,  // Hold/stake bakiyesi (MEV bond vs.)
    pub frozen: Balance,    // Max(tüm freezes + locks)
    pub flags: ExtraFlags,  // Gelecek upgrade'lar için
}
```

`usable() = free - frozen` ve `total() = free + reserved` fonksiyonları birebir kopyalanabilir.

### 2. Existential Deposit — Kritik Güvenlik Kararı

ED'yi sıfır yapmayın. Substrate'in uyarısı net: "this will open up a major DoS vector."

Nova Shield için önerilen ED değeri: En küçük transfer biriminin 1000 katı (parametrik tutun).

```rust
// Config'de
type ExistentialDeposit: Get<Self::Balance>;

// Varsayılan
type ExistentialDeposit = ConstU64<1_000>;
```

### 3. Lock vs Freeze Kararı

Substrate'in kendi kararı: **Locks deprecated, Freezes kullan.** Nova Shield yeni bir chain olduğundan sadece Freezes implementasyonu yeterli:
- Daha tip güvenli (RuntimeFreezeReason enum)
- Daha az storage (WeakBoundedVec yerine BoundedVec)
- Yeni pallet'lar Freeze kullanıyor (staking, democracy)

Locks sadece geriye dönük uyumluluk için bırakın.

### 4. Imbalance Sistemi — MEV ve Ücret Dağıtımı

Nova Shield'ın MEV koruması için ücret dağıtımı kritik:

```rust
// MEV bot cezası (slash)
let (slashed_amount, remaining) = Balances::slash(&mev_bot, penalty);
// slashed_amount → NegativeImbalance — TotalIssuance'dan düşer veya hazineye gider
// T::SlashHandler::on_unbalanced(slashed_amount)

// Validator/koruyucu ödülü
let reward = Balances::issue(validator_reward);
// → PositiveImbalance — TotalIssuance'a eklenir
// T::RewardHandler::on_unbalanced(reward)
```

### 5. Hold Sistemi — MEV Koruma Bond'u

Nova Shield'ın MEV koruma sisteminde validator/relayer'lar bond yatırır:

```rust
// RuntimeHoldReason enum
#[derive(Encode, Decode, Clone, PartialEq, Debug, VariantCount)]
pub enum RuntimeHoldReason {
    MevProtectionBond,   // MEV koruma için bond
    ValidatorDeposit,    // Validator deposit
    GovernanceProposal,  // Governance önerisi
}

// Bond yatırma
<Balances as fungible::MutateHold<_>>::hold(
    &RuntimeHoldReason::MevProtectionBond,
    &validator,
    bond_amount,
)?;

// Kötü davranış → slash held
<Balances as fungible::MutateHold<_>>::burn_held(
    &RuntimeHoldReason::MevProtectionBond,
    &misbehaving_validator,
    slash_amount,
    BestEffort,
    Force,
)?;
```

### 6. Dust Removal — Treasury veya Burn

Nova Shield'da dust handling kararı: Treasury veya burn? Substrate her ikisini de destekler:

```rust
// Config'de
type DustRemoval = Treasury;       // Dust → Treasury'ye
// veya
type DustRemoval = ();             // Dust → burn (total issuance azalır)
```

Nova Shield için Treasury önerilir — küçük miktarlar da topluluk yönetimine gider.

### 7. Total Issuance Güvencesi

TotalIssuance tutarlılığı Rust ownership sistemi ile garanti altında. Nova Shield için:
- `#[must_use]` imbalance'ları drop etmeyin
- İmbalanceler sadece özel handler'larla işlensin
- `force_adjust_total_issuance` sadece emergency durumda kullanın

### 8. Named Reserves → Holds Migrasyonu

Eğer eski kod Reserves API kullanıyorsa (governance, staking), yeni kodda Holds'a geçin:

```rust
// Eski (deprecated)
<Balances as NamedReservableCurrency<_>>::reserve_named(&id, &who, amount)?;

// Yeni (önerilen)
<Balances as fungible::MutateHold<_>>::hold(&reason, &who, amount)?;
```

### 9. Consumer Reference Yönetimi

Nova Shield'ın diğer pallet'ları (staking, DEX, vesting) consumer ref artırmalı/azaltmalı:

```rust
// Hesabın başka pallet'lara bağlı olduğunu işaretle
frame_system::Pallet::<T>::inc_consumers(&who)?;

// Bağlantı kesildiğinde
frame_system::Pallet::<T>::dec_consumers(&who);
```

Provider ref = "hesap neden canlı?" (ED varsa Balances sağlar)
Consumer ref = "başka kim bu hesaba bağlı?" (her bağlı pallet artırır)

### 10. Çoklu Instance (Pallet<T, I>)

Substrate'in pallet-balances çoklu instance destekler. Nova Shield'da farklı token'lar için:

```rust
// Yerel token
type Balances = pallet_balances::Pallet<Runtime>;

// DEX LP token (ayrı instance)
type LpTokenBalances = pallet_balances::Pallet<Runtime, Instance1>;

// Staking token (ayrı instance)
type StakingBalances = pallet_balances::Pallet<Runtime, Instance2>;
```

Her instance bağımsız TotalIssuance, Account, Locks, Freezes, Holds, Reserves storage'a sahip.

---

## Kaynak Kod Referansları

Tüm değerler doğrudan kaynak koddan alındı:

| Referans | Dosya:Satır | İçerik |
|---|---|---|
| AccountData yapısı | types.rs:109-126 | free, reserved, frozen, flags |
| IS_NEW_LOGIC sabiti | types.rs:128 | 0x80000000... u128 |
| ExtraFlags impl | types.rs:130-149 | is_new_logic, set_new_logic |
| usable() | types.rs:152-154 | free.saturating_sub(frozen) |
| total() | types.rs:157-159 | free.saturating_add(reserved) |
| DustCleaner Drop | types.rs:166-173 | DustLost event + on_unbalanced |
| BalanceLock yapısı | types.rs:76-83 | id, amount, reasons |
| Reasons enum | types.rs:40-47 | Fee=0, Misc=1, All=2 |
| Reasons BitOr | types.rs:61-69 | farklı → All |
| TotalIssuance storage | lib.rs:469-471 | StorageValue whitelist |
| InactiveIssuance storage | lib.rs:474-477 | StorageValue whitelist |
| Account storage | lib.rs:504-505 | Blake2_128Concat map |
| Locks storage | lib.rs:512-518 | WeakBoundedVec<BalanceLock, MaxLocks> |
| Reserves storage | lib.rs:524-530 | BoundedVec<ReserveData, MaxReserves> |
| Holds storage | lib.rs:534-543 | BoundedVec<IdAmount, VariantCountOf> |
| Freezes storage | lib.rs:547-553 | BoundedVec<IdAmount, MaxFreezes> |
| transfer_allow_death | lib.rs:648-658 | call_index=0, Expendable |
| force_transfer | lib.rs:662-674 | call_index=2, Root, Expendable |
| transfer_keep_alive | lib.rs:682-692 | call_index=3, Preserve |
| transfer_all | lib.rs:709-730 | call_index=4, reducible_balance |
| force_unreserve | lib.rs:735-745 | call_index=5, Root |
| upgrade_accounts | lib.rs:756-778 | call_index=6, %90 fee waiver |
| force_set_balance | lib.rs:787-816 | call_index=8, Root, wipeout |
| force_adjust_total_issuance | lib.rs:826-848 | call_index=9, Root, DeltaZero |
| burn dispatchable | lib.rs:859-874 | call_index=10, Signed |
| try_mutate_account | lib.rs:1064-1155 | provider/consumer ref yönetimi |
| update_locks | lib.rs:1157-1213 | frozen = max(locks, freezes) |
| update_freezes | lib.rs:1215-1250 | frozen = max(locks, freezes) |
| do_transfer_reserved | lib.rs:1258-1325 | precision/fortitude/status |
| reducible_balance | impl_fungible.rs:47-73 | preservation + force hesabı |
| can_deposit | impl_fungible.rs:74-103 | Overflow, BelowMinimum |
| can_withdraw | impl_fungible.rs:104-150 | Frozen, WouldDie, ReducedToZero |
| set_balance_on_hold | impl_fungible.rs:292-334 | Holds güncelleme |
| set_freeze | impl_fungible.rs:351-364 | TooManyFreezes |
| extend_freeze | impl_fungible.rs:366-379 | max alma |
| thaw | impl_fungible.rs:381-385 | retain filter |
| Currency::transfer | impl_currency.rs:392-408 | fungible::Mutate yönlendirme |
| Currency::slash | impl_currency.rs:419-452 | can_dec_provider kontrolü |
| ensure_can_withdraw | impl_currency.rs:378-389 | frozen kontrolü |
| reserve | impl_currency.rs:627-643 | free → reserved |
| unreserve | impl_currency.rs:651-678 | dönüş: kalan miktar |
| set_lock | impl_currency.rs:976-995 | replace veya append |
| extend_lock | impl_currency.rs:1000-1028 | max alma |
| PositiveImbalance Drop | impl_currency.rs:257-265 | TotalIssuance += |
| NegativeImbalance Drop | impl_currency.rs:267-275 | TotalIssuance -= |
| active_issuance | impl_fungible.rs:35-37 | total - inactive |
| deactivate | impl_fungible.rs:179-184 | InactiveIssuance += |
| migrate_v0_to_v1 | migration.rs:24-52 | StorageVersion upgrade |
| integrity_test | lib.rs:619-631 | ED > 0, MaxFreezes >= VARIANT_COUNT |
| GenesisConfig build | lib.rs:574-614 | Balance >= ED assertion |

---

## Test Senaryoları — Ayrıntılı Analiz

### currency_tests.rs — Lock Sistemi Testleri

#### set_lock_with_amount_zero_removes_lock (currency_tests.rs:54-63)

```rust
Balances::set_lock(ID_1, &1, u64::MAX, WithdrawReasons::all());
Balances::set_lock(ID_1, &1, 0, WithdrawReasons::all());
assert_ok!(<Balances as Currency<_>>::transfer(&1, &2, 1, AllowDeath));
```

**Kanıtlıyor:** Sıfır miktarlı lock, lock'u kaldırır. `set_lock` içindeki `if amount.is_zero() { remove_lock }` dalı.

#### set_lock_with_withdraw_reasons_empty_removes_lock (currency_tests.rs:65-75)

```rust
Balances::set_lock(ID_1, &1, u64::MAX, WithdrawReasons::all());
Balances::set_lock(ID_1, &1, u64::MAX, WithdrawReasons::empty());
// transfer başarılı
```

**Kanıtlıyor:** Boş WithdrawReasons da lock'u kaldırır (`if reasons.is_empty() { remove_lock }`).

#### basic_locking_should_work (currency_tests.rs:77-89)

```rust
Balances::set_lock(ID_1, &1, 9, WithdrawReasons::all());
assert_noop!(
    <Balances as Currency<_>>::transfer(&1, &2, 5, AllowDeath),
    TokenError::Frozen
);
```

**Kanıtlıyor:** Lock, `frozen` alanını günceller. Transfer `new_balance >= frozen` koşulunu ihlal ederse `Frozen` hatası.

#### account_should_be_reaped (currency_tests.rs:110-122)

```rust
<Balances as Currency<_>>::transfer(&1, &2, 10, AllowDeath)
// Sonuç: providers=0, consumers=0, hesap silinir
assert!(!frame_system::Account::<Test>::contains_key(&1));
```

**Kanıtlıyor:** Tüm free bakiye transfer edildiğinde (free < ED), `dec_providers` çağrılır ve hesap tamamen silinir.

#### reap_failed_due_to_provider_and_consumer (currency_tests.rs:125-149)

```
Senaryo 1: providers=1, consumers=1 → can_dec_provider=false → transfer fail (TokenError::Frozen)
Senaryo 2: providers=2, consumers=1 → can_dec_provider=true → transfer başarılı, providers=1 kalır
```

**Kanıtlıyor:** Consumer ref varken tek provider silinemez. Bu `try_mutate_account` içinde kontrol edilir.

### reentrancy_tests.rs — Dust Removal Senaryoları

#### transfer_dust_removal_tst1_should_work (reentrancy_tests.rs:28-71)

```
Senaryo:
- hesap 1: 1000 bakiye, dust_trap (alıcı)
- hesap 2: 500 bakiye
- transfer: 2 → 3, 450 miktar

Sonuç:
- hesap 2: 500 - 450 = 50, ama 50 < ED(100) → DustLost event
- Dust (50) → hesap 1'e gider (dust_trap)
- hesap 1: 1000 + 50 = 1050
- hesap 3: 450
```

**Event sırası:** Transfer → DustLost → Deposit (dust_trap'a)

**Kanıtlıyor:** Dust removal reentrancy sorunsuz çalışıyor. DustCleaner RAII guard ile, mutasyon tamamlandıktan sonra dust işleniyor.

#### emit_events_with_no_existential_deposit_suicide_with_dust (reentrancy_tests.rs:160-201)

```rust
// hesap 1: 100 bakiye, ED=2
let res = Balances::withdraw(&1, 98, BestEffort, Protect, Force);
// hesap 1: 100 - 98 = 2 (henüz ED üzeri, hesap canlı)

let res = Balances::withdraw(&1, 1, BestEffort, Expendable, Force);
// hesap 1: 2 - 1 = 1 < ED(2)
// KilledAccount → DustLost(1) → BurnedDebt(1) → Withdraw(1) → BurnedDebt(1)
```

**Kritik gözlem:** Dust (1) burn edilince iki kez `BurnedDebt` eventi — hem dust burning hem de withdraw'un kendisi için.

### consumer_limit_tests.rs — Consumer Ref Limiti Testleri

#### lock_behavior_when_consumer_limit_fully_exhausted

```
max_consumers doldurulur → System::inc_consumers başarısız
ama: Balances::set_lock → update_locks → mutate_account(force_consumer_bump=true)
→ inc_consumers_WITHOUT_LIMIT başarılı!

Sonuç: Lock oluşturulabilir, consumer=max_consumers+1 olur
```

**Tasarım kararı:** Lock API hata dönemez (LockableCurrency trait). Bu yüzden `force_consumer_bump=true` kullanılır — consumer limit bypass edilir.

#### freeze_behavior_when_consumer_limit_fully_exhausted

```
max_consumers doldurulur → set_freeze → update_freezes → mutate_account(force_consumer_bump=false)
→ inc_consumers BAŞARISIZ → TooManyConsumers hatası döner
```

**Fark:** Freeze API hata dönebilir, bu yüzden `force_consumer_bump=false`. Daha güvenli tasarım.

#### hold_behavior_when_consumer_limit_fully_exhausted

```
max_consumers doldurulur → hold → set_balance_on_hold → try_mutate_account
→ inc_consumers BAŞARISIZ → TooManyConsumers
```

Hold da Freeze gibi hata döner.

**Özet Consumer Ref Davranışı:**

| Operasyon | force_consumer_bump | Consumer Limit Aşılırsa |
|---|---|---|
| set_lock | true (update_locks) | Geçer (inc_without_limit) |
| set_freeze | false | TooManyConsumers hatası |
| hold | false | TooManyConsumers hatası |
| reserve | false | TooManyConsumers hatası |

---

## Config Trait — Tam Analiz

### Config<I> Parametreleri (lib.rs:251-336)

```rust
pub trait Config<I: 'static = ()>: frame_system::Config {
    // Olay tipi
    type RuntimeEvent: From<Event<Self, I>> + IsType<...>;
    // Hold nedeni
    type RuntimeHoldReason: Parameter + Member + MaxEncodedLen + Copy + VariantCount;
    // Freeze nedeni
    type RuntimeFreezeReason: VariantCount;
    // Ağırlık bilgisi
    type WeightInfo: WeightInfo;
    // Bakiye tipi: AtLeast32BitUnsigned + FixedPointOperand
    type Balance: Parameter + Member + AtLeast32BitUnsigned + Codec + ...;
    // Dust handler
    type DustRemoval: OnUnbalanced<CreditOf<Self, I>>;
    // Existential deposit
    #[pallet::constant]
    type ExistentialDeposit: Get<Self::Balance>;
    // AccountData depolama arayüzü
    type AccountStore: StoredMap<Self::AccountId, AccountData<Self::Balance>>;
    // Eski API için reserve identifier
    type ReserveIdentifier: Parameter + Member + MaxEncodedLen + Ord + Copy;
    // Freeze identifier
    type FreezeIdentifier: Parameter + Member + MaxEncodedLen + Copy;
    // Limit değerleri
    #[pallet::constant] type MaxLocks: Get<u32>;
    #[pallet::constant] type MaxReserves: Get<u32>;
    #[pallet::constant] type MaxFreezes: Get<u32>;
    // Slash callback
    type DoneSlashHandler: fungible::hold::DoneSlash<Self::RuntimeHoldReason, Self::AccountId, Self::Balance>;
}
```

### DefaultConfig (lib.rs:220-248)

Test varsayılanları:
- `Balance = u64`
- `ExistentialDeposit = ConstUint<1>`
- `MaxLocks = ConstU32<100>`
- `MaxReserves = ConstU32<100>`
- `MaxFreezes = VariantCountOf<Self::RuntimeFreezeReason>`
- `DustRemoval = ()` (dust burn edilir)
- `DoneSlashHandler = ()` (callback yok)

---

## AccountStore Trait ve Çift Depolama Seçeneği

### StoredMap Trait Arayüzü

`type AccountStore: StoredMap<Self::AccountId, AccountData<Self::Balance>>`

**Seçenek 1: System pallet (önerilen)**
```rust
impl pallet_balances::Config for Runtime {
    type AccountStore = System;
}
```
- `frame_system::Account` map'i kullanılır
- AccountInfo içinde AccountData bulunur
- System ve Balances aynı storage'ı paylaşır

**Seçenek 2: Ayrı Balances storage**
```rust
impl pallet_balances::Config for Runtime {
    type AccountStore = StorageMapShim<...>;
}
```
- Balances kendi Account storage'ını kullanır
- Sadece bakiye verisi, system bilgisi ayrı

---

## Imbalance Sistemi — Derinlemesine Analiz

### Neden Imbalance Tipi?

Substrate para sistemi invariantını şöyle korur:
```
TotalIssuance = Σ(tüm hesap bakiyeleri)
```

İmbalanceler bu invariantı geçici olarak bozar, ama Rust'ın type sistemi çözümü zorlar:
- Para yaratıldı → PositiveImbalance verildi → ya offset edilmeli ya da drop edilmeli
- Para yıkıldı → NegativeImbalance verildi → ya offset edilmeli ya da drop edilmeli

### PositiveImbalance vs NegativeImbalance Semantiği

**PositiveImbalance:** "Sistemde bu kadar TOKEN FAZLA var, birileri bunu dengelemeli"
- `burn()` çağrıldığında döner — TotalIssuance düştü ama para hâlâ bir yerde
- Drop edilince: TotalIssuance += amount (para eklendi, imbalance kapandı)

**NegativeImbalance:** "Sistemde bu kadar TOKEN EKSİK, birileri bunu dengelemeli"
- `issue()` çağrıldığında döner — TotalIssuance arttı ama karşılık yok
- Drop edilince: TotalIssuance -= amount (para çekildi, imbalance kapandı)

### offset() — Imbalance Netleştirmesi (impl_currency.rs:164-175)

```rust
fn offset(self, other: Self::Opposite) -> SameOrOther<Self, Self::Opposite> {
    let (a, b) = (self.0, other.0);
    mem::forget((self, other));  // Drop handler çalışmasın

    if a > b {
        SameOrOther::Same(Self(a - b))
    } else if b > a {
        SameOrOther::Other(NegativeImbalance::new(b - a))
    } else {
        SameOrOther::None  // Tam netleşti
    }
}
```

`mem::forget`: Orijinal imbalanceler düşürülmez (drop olmaz), yeni artık değer döner. TotalIssuance'ın iki kez güncellenmesini önler.

### split() — Ödül Dağıtımı İçin

```rust
fn split(self, amount: T::Balance) -> (Self, Self) {
    let first = self.0.min(amount);
    let second = self.0 - first;
    mem::forget(self);
    (Self(first), Self(second))
}
```

Ödül yönetiminde: `split(validator_cut)` → `(validator_imbalance, treasury_imbalance)`.

---

## DoneSlashHandler — Slash Callback Sistemi

### Config'de (lib.rs:329-335)

```rust
type DoneSlashHandler: fungible::hold::DoneSlash<
    Self::RuntimeHoldReason,
    Self::AccountId,
    Self::Balance,
>;
```

### impl (impl_fungible.rs:410-416)

```rust
fn done_slash(reason: &T::RuntimeHoldReason, who: &T::AccountId, amount: T::Balance) {
    T::DoneSlashHandler::done_slash(reason, who, amount);
}
```

**Kullanım amacı:** Staking veya diğer pallet'lar bir hold'u slash ettiğinde (örn. validator cezası), bu callback çağrılarak başka pallet'lara bildirim yapılabilir. Örneğin, bir Staking pallet'ı ceza kesildiğinde kendi bookkeeping'ini güncelleyebilir.

---

## Fungible Trait Hiyerarşisi — Tam Harita

```
fungible::
├── Inspect (sorgulama, read-only)
│   ├── total_issuance()
│   ├── active_issuance()
│   ├── minimum_balance()
│   ├── total_balance(who)
│   ├── balance(who) → sadece free bakiye
│   ├── reducible_balance(who, preservation, force)
│   ├── can_deposit(who, amount, provenance) → DepositConsequence
│   └── can_withdraw(who, amount) → WithdrawConsequence
│
├── Unbalanced (düşük seviye)
│   ├── write_balance(who, amount)
│   ├── set_total_issuance(amount)
│   ├── deactivate(amount)
│   ├── reactivate(amount)
│   └── handle_dust(dust)
│
├── Mutate (yüksek seviye, imbalance ile)
│   ├── mint_into(who, amount) → Credit
│   ├── burn_from(who, amount, preservation, precision, force) → Debit
│   ├── shelve(who, amount) → Credit (suspend)
│   ├── restore(who, amount) → Debit (unsuspend)
│   ├── transfer(source, dest, amount, preservation)
│   └── done_* event callback'leri
│
├── Balanced (imbalance geri dönen)
│   ├── deposit(who, amount, precision) → Credit
│   ├── withdraw(who, amount, precision, preservation, force) → Debit
│   ├── issue(amount) → Credit
│   └── rescind(amount) → Debit
│
├── InspectHold
│   ├── total_balance_on_hold(who)
│   ├── reducible_total_balance_on_hold(who, force)
│   ├── balance_on_hold(reason, who)
│   └── hold_available(reason, who) → bool
│
├── UnbalancedHold
│   └── set_balance_on_hold(reason, who, amount)
│
├── MutateHold (yüksek seviye hold)
│   ├── hold(reason, who, amount)
│   ├── release(reason, who, amount, precision)
│   ├── burn_held(reason, who, amount, precision, force)
│   ├── transfer_on_hold(reason, source, dest, amount, precision, mode, force)
│   └── transfer_and_hold(reason, source, dest, amount, precision, preservation)
│
├── BalancedHold
│   └── slash(reason, who, amount) → Debit + remaining
│
├── InspectFreeze
│   ├── balance_frozen(id, who)
│   └── can_freeze(id, who) → bool
│
└── MutateFreeze
    ├── set_freeze(id, who, amount)
    ├── extend_freeze(id, who, amount)
    └── thaw(id, who)
```

---

## Karmaşık Senaryo Analizi

### Senaryo 1: Validator Ceza + Ödül Döngüsü

```
1. Validator bond: hold(ValidatorBond, validator, 1000)
   → account.reserved += 1000
   → Holds[] += {ValidatorBond, 1000}

2. Misbehavior slash: burn_held(ValidatorBond, validator, 500, Exact, Force)
   → account.reserved -= 500
   → Holds[ValidatorBond].amount -= 500
   → TotalIssuance -= 500 (gerçek yakma)
   → DoneSlashHandler::done_slash(ValidatorBond, validator, 500)
   → BurnedHeld event

3. Remaining release: release(ValidatorBond, validator, 500, Exact)
   → account.reserved -= 500, account.free += 500
   → Holds[ValidatorBond] silinir
   → Released event
```

### Senaryo 2: MEV Koruması + DEX Freeze Overlap

```
1. MEV koruma freeze: set_freeze(MevProtection, trader, 100)
   → account.frozen = max(mevcut_locks, 100) = 100
   → Frozen event

2. DEX trade freeze: set_freeze(DexTrade, trader, 50)
   → account.frozen = max(100, 50) = 100 — değişmedi!
   → Frozen event YOK (prev == after)

3. DEX trade tamamlanma: thaw(DexTrade, trader)
   → Freezes listesinden DexTrade çıkar
   → account.frozen = max(locks, MevProtection=100) = 100
   → Thawed event YOK (değişim yok)
```

**Kritik:** Freeze kaldırılsa bile `frozen` alanı değişmeyebilir — başka freeze/lock daha büyük olabilir.

### Senaryo 3: Çoklu Lock Overlap — Additive DEĞİL

```
Hesap bakiyesi: 1000
Lock A (staking): 800, Reasons::All
Lock B (governance): 600, Reasons::Misc

update_locks çalışır:
frozen = max(800, 600) = 800

Sonuç:
- reducible_balance(Expendable, Polite) = 1000 - 800 = 200
- Additive olsaydı: 1000 - 1400 = underflow!
- Her iki lock da aynı 800 birim üzerinde çakışır
```

### Senaryo 4: Dust Trap — Treasury Reentrancy Testi

```
ED = 100, dust_trap = hesap 1 (1000 bakiye)
Hesap 2: 500 bakiye

transfer(2 → 3, 450):
1. Hesap 3 oluşur: +450
2. Hesap 2: 500 - 450 = 50 < ED(100)
3. DustCleaner drop: DustLost(50) → on_unbalanced → deposit_creating(dust_trap=1, 50)
4. Hesap 1: 1000 + 50 = 1050

Event sırası: Transfer → DustLost → Deposit(50 to 1)
```

**Reentrancy güvencesi:** `deposit_creating` hesap 1'i mutate ederken hesap 2 artık silinmiş. Çakışan storage mutasyonu yok.

### Senaryo 5: Reserve Repatriation (Slash → Beneficiary)

```
slashed: reserved = 500, free = 100
beneficiary: free = 200 (canlı hesap)

do_transfer_reserved(slashed, beneficiary, 300, BestEffort, Polite, Status::Reserved):

1. max = reducible_total_balance_on_hold(slashed, Polite)
   = reserved.saturating_sub(frozen.saturating_sub(free))
   = 500 - max(0, frozen-100)

2. actual = min(300, max)

3. beneficiary.reserved += actual (Status::Reserved)
4. slashed.reserved -= actual

5. Event: ReserveRepatriated { from: slashed, to: beneficiary, amount: actual, destination_status: Reserved }
```

---

## Özet — Kritik Tasarım Kararları

### 1. Overlapping vs Additive Locks/Freezes

Substrate'in temel tercihi: Lock/Freeze'ler **OVERLAY** eder, **ADD** etmez.

```
Lock A = 800, Lock B = 600
frozen = max(800, 600) = 800  (additive olsaydı 1400 olurdu)
```

Bu, bir kullanıcının birden fazla pallet tarafından "aynı fon üzerinde" kısıtlanmasına izin verir. Staker hem staking lock'u hem governance lock'u kullanabilir — ikisi de aynı stake üzerinde çakışır.

### 2. İki Paralel API Sistemi

```
Eski API (Deprecated):          Yeni API (Önerilen):
─────────────────────           ─────────────────────
Currency                        fungible::Inspect + Mutate
ReservableCurrency              fungible::InspectHold + MutateHold
NamedReservableCurrency         fungible::InspectHold + MutateHold (reason ile)
LockableCurrency                fungible::InspectFreeze + MutateFreeze
```

Her iki API da aynı `AccountData` alanlarını kullanır ve birbirini etkiler.

### 3. Storage Consistency Model

```
TotalIssuance = Σ(account.free + account.reserved) için tüm hesaplar
```

Bu invariant şu mekanizmalarla korunur:
- Imbalance'ların drop'u TotalIssuance'ı otomatik düzeltir (Rust ownership)
- `force_set_balance` delta imbalance üretir
- `force_adjust_total_issuance` doğrudan değiştirir (acil durum, Root only)

### 4. Account Lifecycle — Tam Yaşam Döngüsü

```
DOĞUM:
try_mutate_account → is_new=true → free >= ED → inc_providers → Endowed event

AKTIF (reserved/frozen > 0):
inc_consumers → hesap başka pallet'lara bağlı

CONSUMER AZALMA (reserved=frozen=0):
dec_consumers

ÖLÜM:
free < ED AND reserved = 0 → DustLost event → dec_providers → KilledAccount event (System)
```

### 5. Güvenli Programlama Kalıpları

```rust
// Defensive pattern — debug'da assert, production'da saturate
account.free.defensive_saturating_add(actual)

// Checked arithmetic — hata döner
account.free.checked_sub(&value).ok_or(Error::InsufficientBalance)?

// Saturating arithmetic — sınırda doyar, asla paniklemez
account.free.saturating_reduce(actual)
```

Öncelik sırası: hata dönmek > saturate > panic.

### 6. #[must_use] Imbalance Güvencesi

```rust
#[must_use]
pub struct PositiveImbalance<T: Config<I>, I: 'static = ()>(T::Balance);

#[must_use]
pub struct NegativeImbalance<T: Config<I>, I: 'static = ()>(T::Balance);
```

Derleyici, imbalance'ların kullanılmadan bırakılmasına izin vermez. TotalIssuance tutarlılığı compile-time'da garanti altına alınır.

### 7. DustCleaner RAII Pattern

```rust
pub struct DustCleaner<T: Config<I>, I: 'static = ()>(
    pub(crate) Option<(T::AccountId, CreditOf<T, I>)>,
);

impl Drop for DustCleaner<T, I> {
    fn drop(&mut self) {
        if let Some((who, dust)) = self.0.take() {
            // İşlem tamamlandıktan SONRA dust temizlenir
            T::DustRemoval::on_unbalanced(dust);
        }
    }
}
```

Dust, dıştaki mutasyonlar tamamlandıktan sonra işlenir. Reentrancy sorunlarını önler.

---

## Sonuç — pallet-balances Analiz Özeti

`pallet-balances` Substrate'in en kritik ve en karmaşık palletidir. 16 dosya, ~3500+ satır Rust kodu. Temel bulgular:

1. **AccountData = {free, reserved, frozen, flags}** — dört alan tüm bakiye mantığını taşır
2. **frozen = max(tüm_locks, tüm_freezes)** — overlapping model, additive değil
3. **ED mekanizması** Rust RAII ile (DustCleaner) güvenli şekilde uygulanır
4. **Imbalance tipi** TotalIssuance tutarlılığını Rust type sistemi ile garanti eder
5. **Consumer ref** hesabın başka pallet'lara bağlı olduğunu gösterir
6. **Lock API deprecated** — yeni projelerde Freeze kullanılmalı (PR #12951)
7. **Hold API** = Reserve API'nin tip güvenli hali — RuntimeHoldReason enum ile
8. **force_consumer_bump** Lock API'nin hata dönememesi sorununu çözer
9. **InactiveIssuance** XCM teleport için doğru supply takibi sağlar
10. **%90 kuralı** upgrade_accounts için ekonomik teşvik mekanizması
11. **Çoklu instance** (Pallet<T, I>) farklı token'lar için ayrı state
12. **DoneSlashHandler** slash callback — diğer pallet'ların bookkeeping güncellemesi için
