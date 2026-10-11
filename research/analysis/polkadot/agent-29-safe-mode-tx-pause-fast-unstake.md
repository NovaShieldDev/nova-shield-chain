# Agent-29: pallet-safe-mode + pallet-tx-pause + pallet-fast-unstake + Diger Analizi

## Kapsanan Dosyalar

| Pallet | Dosyalar |
|---|---|
| pallet-safe-mode | src/lib.rs, src/mock.rs, src/tests.rs, src/benchmarking.rs, src/weights.rs |
| pallet-tx-pause | src/lib.rs, src/mock.rs, src/tests.rs, src/benchmarking.rs, src/weights.rs |
| pallet-fast-unstake | src/lib.rs, src/types.rs, src/migrations.rs, src/mock.rs, src/tests.rs, src/weights.rs |
| pallet-whitelist | src/lib.rs, src/mock.rs, src/tests.rs |
| pallet-parameters | src/lib.rs, src/tests/mock.rs, src/tests/unit.rs |
| pallet-state-trie-migration | src/lib.rs, src/weights.rs |
| pallet-migrations | src/lib.rs, src/migrations.rs, src/mock.rs, src/mock_helpers.rs |

Toplam: 7 pallet, tum kaynak dosyalar okundu.

---

## 1. pallet-safe-mode

**Kaynak:** `substrate/frame/safe-mode/src/lib.rs`

### 1.1 Genel Amac

Safe mode, runtime'daki belirli bir whitelist disindaki TUM extrinsic'leri bloke eden bir acil durum mekanizmasidir. `BaseCallFilter` uzerinden `Contains<RuntimeCall>` implement ederek calisir. Kullanim sekli:

```rust
// frame_system::Config'de:
type BaseCallFilter = InsideBoth<DefaultFilter, SafeMode>;
```

Safe mode aktifken pallet kendi cagrilarini HIC ZAMAN bloklayamaz (lib.rs:588-591).

### 1.2 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `EnteredUntil<T>` | `StorageValue<_, BlockNumberFor<T>, OptionQuery>` | Safe mode'un aktif oldugu son block. `None` = deaktif. (lib.rs:255) |
| `Deposits<T>` | `StorageDoubleMap<_, Twox64Concat, AccountId, Twox64Concat, BlockNumber, Balance, OptionQuery>` | Hesap + block numarasina gore tutulan deposit'ler (lib.rs:262-270) |

### 1.3 Config Trait - Kritik Parametreler

| Config | Aciklama |
|---|---|
| `WhitelistedCalls: Contains<RuntimeCall>` | Safe mode'da gecen cagrilar (lib.rs:114) |
| `EnterDuration: Get<BlockNumber>` | `enter()` ile kac block gireceği (lib.rs:118) |
| `ExtendDuration: Get<BlockNumber>` | `extend()` ile kac block uzatilacagi (lib.rs:124) |
| `EnterDepositAmount: Get<Option<Balance>>` | Permissionless giris icin deposit (`None` = devre disi) (lib.rs:130) |
| `ExtendDepositAmount: Get<Option<Balance>>` | Permissionless uzatma icin deposit (`None` = devre disi) (lib.rs:136) |
| `ForceEnterOrigin: EnsureOrigin<_, Success=BlockNumber>` | Zorunlu giris koyeni - **Success degeri = kac block girilecegi** (lib.rs:141) |
| `ForceExtendOrigin: EnsureOrigin<_, Success=BlockNumber>` | Zorunlu uzatma koyeni - **Success degeri = kac block uzatilacagi** (lib.rs:146) |
| `ForceExitOrigin` | Zorunlu cikis koyeni (lib.rs:149) |
| `ForceDepositOrigin` | Deposit slash/release koyeni (lib.rs:152) |
| `ReleaseDelay: Get<Option<BlockNumber>>` | Deposit serbest birakmak icin bekleme suresi (`None` = devre disi) (lib.rs:166) |
| `Notify: SafeModeNotify` | Safe mode girisi/cikisi bildirimi (lib.rs:155) |

**KRITIK TASARIM:** `ForceEnterOrigin::Success = BlockNumber` — origin'in success type'i kac block girilebilecegini belirler. Bu sayede farkli yetkili originler farkli sure icin giris yapabilir (mock'ta: Weak=3 block, Strong=5 block).

### 1.4 Hold Reason

```rust
pub enum HoldReason {
    #[codec(index = 0)]
    EnterOrExtend,  // lib.rs:293
}
```

Tek bir hold reason; hem giris hem uzatma depositleri ayni reason altinda tutulur.

### 1.5 Extrinsics

#### `enter(origin)` — call_index(0)
- Origin: `ensure_signed` (herhangi bir hesap)
- `EnterDepositAmount::get()` `None` ise `Error::NotConfigured`
- `EnterDepositAmount` kadar hold alir
- `do_enter(Some(who), EnterDuration::get())` cagirir
- `until = current_block + EnterDuration` — **until hesaplama: lib.rs:477**
- Ayni hesap iki kez deposit koyamaz: `AlreadyDeposited` (lib.rs:565-567)

#### `force_enter(origin)` — call_index(1)
- Origin: `ForceEnterOrigin::ensure_origin` → success = duration
- Deposit yok
- `do_enter(None, duration)` cagirir

#### `extend(origin)` — call_index(2)
- Origin: `ensure_signed`
- Safe mode aktif degilse: `Error::Exited`
- `ExtendDepositAmount::get()` `None` ise `Error::NotConfigured`
- `until.saturating_accrue(duration)` — mevcut sure uzerine eklenir (lib.rs:496)

#### `force_extend(origin)` — call_index(3)
- Origin: `ForceExtendOrigin::ensure_origin` → success = duration
- Deposit yok

#### `force_exit(origin)` — call_index(4)
- Origin: `ForceExitOrigin`
- `EnteredUntil::take()` ile safe mode'u kaldirir
- `ExitReason::Force` event'i emit eder

#### `force_slash_deposit(origin, account, block)` — call_index(5)
- Origin: `ForceDepositOrigin`
- `Deposits::take(&account, block)` — deposit'i siler
- `Currency::burn_held(Precision::BestEffort, Fortitude::Force)` — hold'u yakar (lib.rs:547-554)
- `defensive_assert!(burned == amount)` — tam miktar yakildigi dogrulanir (lib.rs:555)

#### `release_deposit(origin, account, block)` — call_index(6)
- Origin: `ensure_signed` (herhangi biri)
- Kaldirma kosullari (lib.rs:521-526):
  1. Safe mode DEAKTIF olmali
  2. `ReleaseDelay` `None` degilse `now > block + ReleaseDelay` olmali
- `Currency::release(Precision::BestEffort)` ile hold birakilir

#### `force_release_deposit(origin, account, block)` — call_index(7)
- Origin: `ForceDepositOrigin`
- Safe mode aktifken bile calisabilir (`force=true` ile `do_release` cagirir, lib.rs:441)
- Delay kontrolu yapilmaz

### 1.6 Hook: `on_initialize`

```rust
fn on_initialize(current: BlockNumberFor<T>) -> Weight {
    let Some(limit) = EnteredUntil::<T>::get() else {
        return T::WeightInfo::on_initialize_noop();
    };
    if current > limit {
        let _ = Self::do_exit(ExitReason::Timeout)...
        T::WeightInfo::on_initialize_exit()
    } else {
        T::WeightInfo::on_initialize_noop()
    }
}
// lib.rs:449-461
```

Safe mode'dan otomatik cikis: `current > limit` — yani limit block'tan SONRAKI blokta cikis olur.

### 1.7 Call Filter Mantigi

```rust
pub fn is_allowed(call: &T::RuntimeCall) -> bool {
    let CallMetadata { pallet_name, .. } = call.get_call_metadata();
    // SafeMode pallet'i her zaman gecilir
    if pallet_name == <Pallet<T> as PalletInfoAccess>::name() {
        return true;
    }
    if Self::is_entered() {
        T::WhitelistedCalls::contains(call)
    } else {
        true
    }
}
// lib.rs:583-598
```

**Kritik:** SafeMode pallet'inin kendi cagrilari her zaman izin verilir. DB maliyet: `EnteredUntil` ilk extrinsic'te 1 DB oku, sonrasi overlay'den.

### 1.8 SafeMode Trait Implementasyonu

lib.rs:611-636 - `frame::traits::SafeMode` trait'ini implement eder, dis kodun safe mode'u programatik kontrol etmesine olanak tanir.

### 1.9 Events

| Event | Aciklama |
|---|---|
| `Entered { until }` | Safe mode'a girildi |
| `Extended { until }` | Safe mode uzatildi |
| `Exited { reason: ExitReason }` | Cikis (Timeout veya Force) |
| `DepositPlaced { account, amount }` | Deposit yatirild |
| `DepositReleased { account, amount }` | Deposit serbest birakildi |
| `DepositSlashed { account, amount }` | Deposit slash edildi |
| `CannotDeposit` | Hold alinamamasi durumu |
| `CannotRelease` | Release edilememesi durumu |

### 1.10 Errors

`Entered`, `Exited`, `NotConfigured`, `NoDeposit`, `AlreadyDeposited`, `CannotReleaseYet`, `CurrencyError`

### 1.11 Genesis Config

```rust
pub struct GenesisConfig<T: Config> {
    pub entered_until: Option<BlockNumberFor<T>>,
}
// lib.rs:275-285
```

Genesis'te safe mode aktif baslatilabilir.

### 1.12 Mock Degerler (Testlerden)

```
EnterDuration = 7 block          (mock.rs:151)
ExtendDuration = 10 block         (mock.rs:152)
EnterDepositAmount = 100         (mock.rs:153)
ExtendDepositAmount = 100         (mock.rs:154)
ReleaseDelay = 20 block           (mock.rs:155)
ForceEnterWeak = 3 block          (mock.rs:158)
ForceEnterStrong = 5 block        (mock.rs:159)
ForceExtendWeak = 11 block        (mock.rs:160)
ForceExtendStrong = 15 block      (mock.rs:161)
ForceExitOrigin = account 100     (mock.rs:187)
ForceDepositOrigin = account 200  (mock.rs:188)
```

---

## 2. pallet-tx-pause

**Kaynak:** `substrate/frame/tx-pause/src/lib.rs`

### 2.1 Genel Amac

Safe mode'dan DAHA ince granularite: belirli bir pallet veya belirli bir call'u duraksatma. String tabanli (pallet adi + call adi). `BaseCallFilter` uzerinden calisir:

```rust
type BaseCallFilter = InsideBoth<DefaultFilter, TxPause>;
```

### 2.2 Type Tanimlari

```rust
pub type PalletNameOf<T> = BoundedVec<u8, <T as Config>::MaxNameLen>;
pub type PalletCallNameOf<T> = BoundedVec<u8, <T as Config>::MaxNameLen>;
pub type RuntimeCallNameOf<T> = (PalletNameOf<T>, PalletCallNameOf<T>);
// lib.rs:86-94
```

Pallet adi + call adi cifti ile tek bir extrinsic veya tum pallet durdurulabilir.

### 2.3 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `PausedCalls<T>` | `StorageMap<_, Blake2_128Concat, RuntimeCallNameOf<T>, (), OptionQuery>` | Duraklatilmis cagrilarin seti (lib.rs:142-143) |

**Performans notu:** Her extrinsic'te 1 DB okuma. `EnteredUntil`'den farkli olarak overlay'e tasimak daha zor cunku her farkli call icin ayri key.

### 2.4 Config Trait

| Config | Aciklama |
|---|---|
| `PauseOrigin: EnsureOrigin` | Pause yapabilecek origin (lib.rs:119) |
| `UnpauseOrigin: EnsureOrigin` | Unpause yapabilecek origin (lib.rs:122) |
| `WhitelistedCalls: Contains<RuntimeCallNameOf<Self>>` | Hic pause edilemeyecek cagrilar (lib.rs:128) |
| `MaxNameLen: Get<u32>` | **KRITIK: Bu limitin asilmasi durumunda call PAUSED sayilir** (lib.rs:133-134) |

**GUVENLK UYARISI:** `MaxNameLen` asiminda davranis: `is_paused_unbound` false donerken try_from hatalari `true` (paused) olarak yorumlanir (lib.rs:243-249). Bu tasarimsal bir ozellik: uzun isimler = paused.

### 2.5 Extrinsics

#### `pause(origin, full_name)` — call_index(0)
- Origin: `PauseOrigin`
- `ensure_can_pause` kontrolu:
  1. TxPause pallet'inin kendi cagrilari durduralamaz (lib.rs:255)
  2. `WhitelistedCalls`'ta olan cagrilar durduralamaz (lib.rs:259-261)
  3. Zaten pause edilmisse: `Error::IsPaused` (lib.rs:262-264)
- `PausedCalls::insert(ident, ())`

#### `unpause(origin, ident)` — call_index(1)
- Origin: `UnpauseOrigin`
- `ensure_can_unpause`: Durdurulmamissa `Error::IsUnpaused` (lib.rs:269-276)
- `PausedCalls::remove(ident)`

### 2.6 Call Filter Mantigi

```rust
impl<T: pallet::Config> Contains<RuntimeCall> for Pallet<T> {
    fn contains(call: &RuntimeCall) -> bool {
        let CallMetadata { pallet_name, function_name } = call.get_call_metadata();
        !Pallet::<T>::is_paused_unbound(pallet_name.into(), function_name.into())
    }
}
// lib.rs:283-288
```

`is_paused_unbound` hem exact call hem de pallet-level pause'u kontrol eder: eger `(pallet_name, "")` map'te varsa tum pallet duraklatilmistir (bu implicit bir ozellik, kodu okuyarak cikarilan sonuc).

### 2.7 Pallet Seviyesi Pause

Pallet-level pause icin `full_name.1` (call name) bostur (`b""`). `StorageMap` key'i olarak `(pallet_name, "")` tutulur. `contains` kontrolunde belirli call kontrol edildiginden, pallet-level pause'u yakalamak icin `WhitelistedCalls` implementasyonu ozenle yapilmalidir.

**Not:** Mock'ta `Balances::transfer_keep_alive` whitelist'te (mock.rs:120-123) — pallet-level pause bile bunu engelleyemez.

### 2.8 TransactionPause Trait

lib.rs:290-308: `frame::traits::TransactionPause` trait'ini implement eder — `is_paused`, `can_pause`, `pause`, `unpause` metodlari.

### 2.9 Events

| Event | Aciklama |
|---|---|
| `CallPaused { full_name }` | Belirtilen call/pallet durduruldu |
| `CallUnpaused { full_name }` | Belirtilen call/pallet yeniden aktif edildi |

### 2.10 Errors

`IsPaused`, `IsUnpaused`, `Unpausable`, `NotFound`

### 2.11 Genesis Config

```rust
pub struct GenesisConfig<T: Config> {
    pub paused: Vec<RuntimeCallNameOf<T>>,
}
```

Genesis'te bazi cagrilar pause edilmis baslatilabilir (lib.rs:172-185).

### 2.12 Mock Degerler

```
MaxNameLen = 50 byte                (mock.rs:107)
PauseOrigin = account 1             (mock.rs:111)
UnpauseOrigin = account 2           (mock.rs:112)
WhitelistedCalls: Balances::transfer_keep_alive  (mock.rs:120-123)
```

---

## 3. pallet-fast-unstake

**Kaynak:** `substrate/frame/fast-unstake/src/lib.rs`, `src/types.rs`, `src/migrations.rs`

### 3.1 Neden "Fast" Unstake?

Normal unstake: Bonding duration (Polkadot'ta 28 gun/era) beklemek gerekir. Bu sure, slashing riskinin onceden kovulmasi icindir. Eger bir nominator son `bonding_duration` era'da HICBIR validator'a expose olmamissa, bu bekleme gereksizdir. Fast unstake tam olarak bunu kontrol eder ve uygunlari hizla serbest birakir.

**lib.rs:37-67** — Detayli aciklama.

### 3.2 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `Head<T>` | `StorageValue<_, UnstakeRequest<T>, OptionQuery>` | Simdi islenen batch (lib.rs:211) |
| `Queue<T>` | `CountedStorageMap<_, Twox64Concat, AccountId, Balance>` | Bekleme kuyrugu — stash → deposit (lib.rs:218) |
| `ErasToCheckPerBlock<T>` | `StorageValue<_, u32, ValueQuery>` | Block basina kontrol edilecek era sayisi (lib.rs:229) |

**Onemli:** `Queue` `CountedStorageMap` — `Queue::count()` O(1) ile eleman sayisini verir.

### 3.3 Tip Tanimlari

```rust
// types.rs:44
pub type BalanceOf<T> = <<T as Config>::Currency as Currency<AccountId>>::Balance;

// types.rs:52-57
pub struct UnstakeRequest<T: Config> {
    pub stashes: BoundedVec<(T::AccountId, BalanceOf<T>), T::BatchSize>,
    pub checked: BoundedVec<EraIndex, MaxChecking<T>>,
}

// types.rs:31-38
pub struct MaxChecking<T: Config>(PhantomData<T>);
impl Get<u32> for MaxChecking<T> {
    fn get() -> u32 {
        T::Staking::bonding_duration() + 1  // bonding_duration + 1 era
    }
}
```

### 3.4 Config Trait

| Config | Aciklama |
|---|---|
| `Currency: ReservableCurrency` | Deposit icin (lib.rs:179) |
| `Deposit: Get<Balance>` | Kayit icin gerekli deposit (lib.rs:184) |
| `ControlOrigin` | `ErasToCheckPerBlock` ayarlayan origin (lib.rs:187) |
| `BatchSize: Get<u32>` | Bir batch'te kac stash (lib.rs:192) |
| `Staking: StakingInterface` | Staking sistemi erisimi (lib.rs:195) |
| `MaxErasToCheckPerBlock: Get<u32>` | `ErasToCheckPerBlock` icin ust sinir (lib.rs:201) |

### 3.5 Extrinsics

#### `register_fast_unstake(origin)` — call_index(0)
- Origin: `ensure_signed` (controller hesabi)
- Kontroller (lib.rs:333-338):
  1. `ErasToCheckPerBlock != 0` (pallet aktif olmali)
  2. `stash_by_ctrl(&ctrl)` — bonded olmali
  3. `Queue::contains_key(&stash)` yok olmali (AlreadyQueued)
  4. `is_head(&stash)` = false olmali (AlreadyHead)
  5. `is_unbonding(&stash)` = false olmali (NotFullyBonded — unlocking chunk yokmali)
- Chill + fully_unbond: lib.rs:341-342
- Deposit rezerve: `Currency::reserve(&stash, Deposit::get())` (lib.rs:344)
- `Queue::insert(stash, deposit)` (lib.rs:347)

**KRITIK:** Kayit basarili oldugunda stash HEMEN chill edilir ve tamamen unbond edilir. Deregister edilse bile bu kalicidir — rebond etmek gerekir.

#### `deregister(origin)` — call_index(1)
- Origin: `ensure_signed`
- `ErasToCheckPerBlock != 0` (aktif olmali)
- `Queue::contains_key(&stash)` olmali (NotQueued)
- `!is_head(&stash)` — Head'deyse deregister EDILEMEZ (lib.rs:379)
- `Queue::take` + `Currency::unreserve` (lib.rs:380-387)

#### `control(origin, eras_to_check)` — call_index(2)
- Origin: `ControlOrigin`
- `eras_to_check <= MaxErasToCheckPerBlock` kontrolu (lib.rs:409)
- `ErasToCheckPerBlock::put(eras_to_check)`
- Pallet'i `eras_to_check=0` ile durdurabilir

### 3.6 on_idle Hook — Temel Islem Mantigi

```rust
fn on_idle(_: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
    if remaining_weight.any_lt(T::DbWeight::get().reads(2)) {
        return Weight::from_parts(0, 0);
    }
    Self::do_on_idle(remaining_weight)
}
// lib.rs:270-276
```

`on_idle` calisir — yani GARANTILI calismaz, block dolduysa atlanabilir.

### 3.7 do_on_idle Algoritma Detayi

**lib.rs:443-625** — Tam akis:

1. `ErasToCheckPerBlock` okur; sifirsa erken donus (lib.rs:448-449)
2. `validator_count` ve `next_batch_size` hesaplar (lib.rs:454-458)
3. Agirlik kontrolu: `max_weight(v, b) > remaining_weight` ise erken donus (lib.rs:473-476)
4. **Election kontrolu**: Aktif election varsa ATLA (lib.rs:478-484)
   - Sebep: Snapshot alinmis olabilir, unstake tehlikeli
5. `Head::take()` veya `Queue::drain()` ile batch al (lib.rs:486-507)
6. `checked.retain(|e| *e >= current_era - bonding_duration)` — eski eralar temizlenir (lib.rs:524)
7. Kontrol edilecek eralar: `(current_era - bonding_duration ..= current_era).rev()` (lib.rs:529-532)
8. Already checked eralar filtrelenir, max `eras_to_check_per_block` alinir (lib.rs:540-546)
9. `unchecked_eras_to_check.is_empty()`:
   - EVET: Tum eralar kontrol edildi, unstake_stash (lib.rs:582-586)
   - HAYIR: check_stash cagir, expose edilmisleri slash et (lib.rs:587-623)

**check_stash mantigi** (lib.rs:566-578):
```rust
let is_exposed = unchecked_eras_to_check.iter()
    .any(|e| T::Staking::is_exposed_in_era(&stash, e));

if is_exposed {
    let _ = T::Currency::slash_reserved(&stash, deposit);  // slash
    Self::deposit_event(Event::Slashed { stash, amount: deposit });
    false  // stash'i batch'ten cikar
} else {
    true   // devam et
}
```

**unstake_stash mantigi** (lib.rs:555-563):
```rust
let result = T::Staking::force_unstake(stash.clone());
let remaining = T::Currency::unreserve(&stash, deposit);
if !remaining.is_zero() {
    Self::halt("not enough balance to unreserve");
} else {
    Self::deposit_event(Event::Unstaked { stash, result });
}
```

### 3.8 Hata Durumu — halt()

```rust
pub(crate) fn halt(reason: &'static str) {
    frame_support::defensive!(reason);
    ErasToCheckPerBlock::<T>::put(0);   // pallet'i durdur
    Self::deposit_event(Event::InternalError)
}
// lib.rs:424-428
```

Herhangi bir ic hata: `ErasToCheckPerBlock=0` ile pallet TAMAMEN DURDURULUR. Manuel mudahale gerekir.

### 3.9 Events

| Event | Aciklama |
|---|---|
| `Unstaked { stash, result }` | Stash basariyla unstake edildi (result = force_unstake sonucu) |
| `Slashed { stash, amount }` | Stash expose edilmis, deposit slash edildi |
| `BatchChecked { eras }` | Bu block'ta kontrol edilen eralar (tamamlanmadi) |
| `BatchFinished { size }` | Batch tamamlandi (size kac stash) |
| `InternalError` | Pallet ic hatasi, durduruldu |

### 3.10 Errors

`NotController`, `AlreadyQueued`, `NotFullyBonded`, `NotQueued`, `AlreadyHead`, `CallNotAllowed`

### 3.11 Storage Version ve Migration

```rust
const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);  // lib.rs:164
```

**migrations.rs - v0 → v1:** Eger Head varsa (eski format: `(AccountId, Vec<EraIndex>, Balance)`), Queue'ya geri koyar. Head bos birakilir.

### 3.12 Testte Dogrulanan Kosullar

- `bonding_duration = 3` (tests.rs:39)
- Unlock chunks varsa kayit engellenir (tests.rs:130-141)
- Head'deyse deregister edilemez (tests.rs:113-127)
- `ErasToCheckPerBlock=0` iken kayit engellenir (tests.rs:72-80)
- Deposit yetersizse hata (tests.rs:54-70)

---

## 4. pallet-whitelist

**Kaynak:** `substrate/frame/whitelist/src/lib.rs`

### 4.1 Genel Amac

Bir "council" koyeni belirli call'lari whitelist'e alir. Ayri bir `DispatchWhitelistedOrigin` bu call'lari ROOT yetkisiyle calistirabilir. Temel guvenlik modeli: **iki farkli yetki grubu gerektirir** — biri whitelist ekleme, digeri calistirma.

### 4.2 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `WhitelistedCall<T>` | `StorageMap<_, Twox64Concat, Hash, (), OptionQuery>` | Whitelist'teki call hash'leri (lib.rs:159) |
| `DeferredDispatch<T>` | `StorageMap<_, Twox64Concat, Hash, ProvidedBlockNumber, OptionQuery>` | Ertelenmi dispatch'lerin son gecerlilik blogu (lib.rs:164-165) |

### 4.3 Config Trait

| Config | Aciklama |
|---|---|
| `WhitelistOrigin` | Whitelist'e ekleyebilen origin (lib.rs:83) |
| `DispatchWhitelistedOrigin` | Whitelisted call'lari calistirabilen origin (lib.rs:86) |
| `Preimages: QueryPreimage + StorePreimage` | Preimage yoneticisi (lib.rs:89) |
| `DeferredDispatchExpiration: Get<BlockNumber>` | Deferred dispatch ne kadar surer (lib.rs:92) |
| `BlockNumberProvider` | Blok numarasi saglayicisi (parachain uyumu icin) (lib.rs:95) |

### 4.4 Extrinsics

#### `whitelist_call(origin, call_hash)` — call_index(0)
- Origin: `WhitelistOrigin`
- Zaten varsa: `CallAlreadyWhitelisted`
- `WhitelistedCall::insert(call_hash, ())`
- `Preimages::request(&call_hash)` — preimage sistemi bu hash'in preimage'ini saklamasini ister (lib.rs:180)

#### `remove_whitelisted_call(origin, call_hash)` — call_index(1)
- Origin: `WhitelistOrigin`
- `WhitelistedCall::take(call_hash)` — yoksa `CallIsNotWhitelisted`
- `Preimages::unrequest(&call_hash)` — preimage gereksinimleri azaltilir (lib.rs:193)

#### `dispatch_whitelisted_call(origin, call_hash, call_encoded_len, call_weight_witness)` — call_index(2)
Kompleks akis (lib.rs:205-248):

```
origin = DispatchWhitelistedOrigin?
    YES + call whitelisted → clean_and_dispatch
    YES + call NOT whitelisted → defer_dispatch (ertelendi)
origin = signed account?
    → ensure_signed_deferred_dispatch (deferred kontrolu)
    → clean_and_dispatch
```

- Weight: `WeightInfo::dispatch_whitelisted_call(call_encoded_len) + call_weight_witness`
- `call.get_dispatch_info().call_weight <= call_weight_witness` zorunlu (lib.rs:231-234)
- **Relayer icin fee yok** (lib.rs:237: `pays_fee = if relayer.is_some() { Pays::No } else { Pays::Yes }`)

#### `dispatch_whitelisted_call_with_preimage(origin, call)` — call_index(3)
- Preimage'i hash'e donusturur, ayni akisi izler (lib.rs:257-287)

#### `remove_deferred_dispatch(origin, call_hash)` — call_index(4)
- Origin: `ensure_signed` (herhangi biri)
- `expire_at <= now` olmali (lib.rs:302-303)
- `Pays::No` ile calisir — ucret almaz (lib.rs:308)

### 4.5 clean_and_dispatch Guvenlik Modeli

```rust
fn clean_and_dispatch(call_hash: T::Hash, call: RuntimeCall) -> Option<Weight> {
    WhitelistedCall::<T>::remove(call_hash);    // whitelist'ten kaldir
    T::Preimages::unrequest(&call_hash);         // preimage unrequest
    DeferredDispatch::<T>::remove(call_hash);    // deferred kaldir
    let result = call.dispatch(frame_system::Origin::<T>::Root.into()); // ROOT ile calistir
    // ...
}
// lib.rs:367-381
```

**Guvenlik:** Call ROOT origin ile calistirilir, whitelist temizlenir (replay yok).

### 4.6 Deferred Dispatch Mekanizmasi

Yeni ozellik: `DispatchWhitelistedOrigin` call whitelisted ama preimage henuz yok veya zamanlama gerekiyorsa erteleyebilir:
- `defer_dispatch`: `expire_at = now + DeferredDispatchExpiration`, `DeferredDispatch::insert(call_hash, expire_at)`
- Herhangi bir signed hesap `expire_at`'den ONCE calistirabileir
- `expire_at` sonrasi `remove_deferred_dispatch` ile temizlenebilir
- Whitelist revoke edilirse ertelenmi dispatch calistirilamaz (lib.rs:359)

### 4.7 Events

| Event | Aciklama |
|---|---|
| `CallWhitelisted { call_hash }` | Whitelist'e eklendi |
| `WhitelistedCallRemoved { call_hash }` | Whitelist'ten kaldirildi |
| `WhitelistedCallDispatched { call_hash, result }` | Calistirildi (result = basari/basarisizlik) |
| `DispatchDeferred { call_hash }` | Dispatch ertelendi |
| `DeferredDispatchRemoved { call_hash }` | Ertelenmis dispatch kaldirildi |
| `DeferredDispatchExecuted { call_hash, who }` | Signed account tarafindan calistirildi |

---

## 5. pallet-parameters

**Kaynak:** `substrate/frame/parameters/src/lib.rs`

### 5.1 Genel Amac

Runtime upgrade gerektirmeden on-chain parametreleri degistirme mekanizmasi. `dynamic_params` macro sistemi ile entegre. Her parametre `Get<T>` implement eder ve storage'dan okur.

### 5.2 Tasarim Felsefesi

- **Avantaj:** Governance kararıyla runtime upgrade olmadan parametre degisimi
- **Dezavantaj:** Her `Get<T>` cagrisinda storage okuma — performans kritik yerlerde KULLANILMAMALI
- **Kullanim yeri:** Nadiren erisilen ama sık degisen parametreler (lib.rs:63)

### 5.3 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `Parameters<T>` | `StorageMap<_, Blake2_128Concat, KeyOf<T>, ValueOf<T>, OptionQuery>` | Parametre key-value store (lib.rs:177-178) |

### 5.4 Config Trait

```rust
pub trait Config: frame_system::Config {
    type RuntimeParameters: AggregatedKeyValue;  // parametrelerin aggregate edilmis tipi
    type AdminOrigin: EnsureOriginWithArg<RuntimeOrigin, KeyOf<Self>>;  // key bazli origin kontrolu
}
// lib.rs:136-157
```

**KRITIK:** `AdminOrigin: EnsureOriginWithArg<_, KeyOf<Self>>` — her KEY icin ayri origin kontrolu yapiabilir. Ornegin: Pallet2 parametrelerini signed, Pallet1 parametrelerini sadece root degistirebilir.

### 5.5 Extrinsics

#### `set_parameter(origin, key_value)` — call_index(0)

```rust
pub fn set_parameter(origin, key_value: T::RuntimeParameters) -> DispatchResult {
    let (key, new) = key_value.into_parts();
    T::AdminOrigin::ensure_origin(origin, &key)?;  // key-bazli yetki kontrolu
    let mut old = None;
    Parameters::<T>::mutate(&key, |v| {
        old = v.clone();
        *v = new.clone();
    });
    Self::deposit_event(Event::Updated { key, old_value: old, new_value: new });
    Ok(())
}
// lib.rs:191-207
```

- `None` set edilirse parametre silinir (varsayilan kullanilir)
- Ayni degere set edilse bile `Updated` event emit edilir (lib.rs:88)

### 5.6 RuntimeParameterStore

```rust
impl<T: Config> RuntimeParameterStore for Pallet<T> {
    fn get<KV, K>(key: K) -> Option<K::Value> {
        let key: <KV as AggregatedKeyValue>::Key = key.into();
        let val = Parameters::<T>::get(key.into_key());
        val.and_then(|v| {
            let val: <KV as AggregatedKeyValue>::Value = v.try_into_key().ok()?;
            let val: K::WrappedValue = val.try_into().ok()?;
            Some(val.into())
        })
    }
}
// lib.rs:236-259
```

Type-safe parametre okuma. Yoksa `None` → caller varsayilan kullanmali.

### 5.7 macro ile Parametre Tanimi

```rust
// mock.rs:44-83
#[dynamic_params(RuntimeParameters, pallet_parameters::Parameters::<Runtime>)]
pub mod dynamic_params {
    #[dynamic_pallet_params]
    #[codec(index = 3)]
    pub mod pallet1 {
        #[codec(index = 0)]
        pub static Key1: u64 = 0;  // varsayilan = 0
        #[codec(index = 2)]
        pub static Key3: u128 = 2; // varsayilan = 2
    }
}
```

`Key3::get()` cagrisinda: onchain deger varsa onu, yoksa `2` (varsayilan) doner.

### 5.8 Events

| Event | Aciklama |
|---|---|
| `Updated { key, old_value, new_value }` | Parametre guncellendi (ayni degerde bile) |

---

## 6. pallet-state-trie-migration

**Kaynak:** `substrate/frame/state-trie-migration/src/lib.rs`

### 6.1 Genel Amac

Tum state'i sistematik olarak okuyup yazan pallet. `sp_core::StateVersion::V1`'e gecis icin gerekli: tum key'lere dokunulmasi gerekir. Iki mod:
1. **Auto migration:** `on_initialize`'da otomatik
2. **Signed migration:** Signed transactions ile

### 6.2 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `MigrationProcess<T>` | `StorageValue<_, MigrationTask<T>, ValueQuery>` | Mevcut migrasyon gorevinin ilerleme durumu (lib.rs:572-573) |
| `AutoLimits<T>` | `StorageValue<_, Option<MigrationLimits>, ValueQuery>` | Otomatik migrasyon limitleri; `None` = devre disi (lib.rs:579-581) |
| `SignedMigrationMaxLimits<T>` | `StorageValue<_, MigrationLimits, OptionQuery>` | Signed migrasyon icin max limit (lib.rs:585-587) |

### 6.3 MigrationTask Yapisi

```rust
pub struct MigrationTask<T: Config> {
    pub(crate) progress_top: ProgressOf<T>,    // top trie ilerleme
    pub(crate) progress_child: ProgressOf<T>,  // child trie ilerleme
    dyn_top_items: u32,    // bu calistirmadaki top item sayisi (storage'a yazilmaz)
    dyn_child_items: u32,  // bu calistirmadaki child item sayisi (storage'a yazilmaz)
    dyn_size: u32,         // bu calistirmadaki byte boyutu (storage'a yazilmaz)
    pub(crate) size: u32,       // toplam boyut (bookkeeping)
    pub(crate) top_items: u32,  // toplam top key sayisi
    pub(crate) child_items: u32,// toplam child key sayisi
}
// lib.rs:142-186
```

`Progress<MaxKeyLen>`: `ToStart | LastKey(BoundedVec<u8, MaxKeyLen>) | Complete`

### 6.4 Config Ozeti

- `ControlOrigin`: auto migration kontrolu
- `SignedFilter: EnsureOrigin<_, Success=AccountId>`: signed migration icin
- `MaxKeyLen: Get<u32>`: max key uzunlugu — DIKKAT: `#[pallet::no_default]` ile isaretli (lib.rs:547-548), her runtime kendi degerini tanimlamak ZORUNDA, pallet icinde varsayilan YOK. 512 byte tipik bir runtime tercihi olabilir ancak hardcoded degil.
- `SignedDepositPerItem: Get<Balance>`: item basina deposit
- `SignedDepositBase: Get<Balance>`: baz deposit
- `deposit = items * SignedDepositPerItem + SignedDepositBase`

### 6.5 Extrinsics

#### `control_auto_migration(origin, maybe_config)` — call_index(0)
- Origin: `ControlOrigin`
- `AutoLimits::put(maybe_config)`

#### `continue_migrate(origin, limits, real_size_upper, witness_task)` — call_index(1)
- Origin: `SignedFilter`
- Deposit kontrolu: `can_hold(&who, deposit)`
- **Witness kontrolu:** `task == witness_task` olmali (BadWitness korumasi)
- `task.migrate_until_exhaustion(limits)` calistirir
- `real_size_upper < task.dyn_size` ise deposit slash (lib.rs:692-695)
- Basarida `Pays::No` — fee iade edilir! (lib.rs:710)

#### `migrate_custom_top(origin, keys, witness_size)` — call_index(2)
- `MigrationProcess`'i etkilemez
- Belirli top key'leri manuel olarak migrate eder
- Witness hatasi → deposit slash

#### `migrate_custom_child(origin, ...)` — sonraki extrinsic
- Child key'ler icin ayni

#### `force_set_migration_process(origin, task)` — ControlOrigin ile emergency override

### 6.6 migrate_tick Core Mantigi

```rust
fn migrate_tick(&mut self) -> Result<(), Error<T>> {
    match (&self.progress_top, &self.progress_child) {
        (Progress::ToStart, _) => self.migrate_top(),
        (Progress::LastKey(_), Progress::LastKey(_)) => self.migrate_child(),
        (Progress::LastKey(top_key), Progress::ToStart) => {
            if !top_key.starts_with(DEFAULT_CHILD_STORAGE_KEY_PREFIX) {
                self.migrate_top()
            } else {
                self.migrate_child()
            }
        },
        (Progress::LastKey(_), Progress::Complete) => {
            self.migrate_top()?;
            self.progress_child = Progress::ToStart;
            Ok(())
        },
        (Progress::Complete, _) => Ok(()),
    }
}
// lib.rs:283-317
```

**Kritik not:** Paralif `dyn_*` alanlar storage'a yazilmaz — sadece calistirma suresi icin sayac.

### 6.7 Hold Reason

```rust
pub enum HoldReason {
    #[codec(index = 0)]
    SlashForMigrate,  // lib.rs:494
}
```

### 6.8 Events

| Event | Aciklama |
|---|---|
| `Migrated { top, child, compute }` | Kac item migrate edildi ve nasil (Signed/Auto) |
| `Slashed { who, amount }` | Yanlis witness — slash |
| `AutoMigrationFinished` | Otomatik migrasyon tamamlandi |
| `Halted { error }` | Migrasyon durdu (key uzunlugu asilamazsa) |

---

## 7. pallet-migrations (Multi-Block Migrations)

**Kaynak:** `substrate/frame/migrations/src/lib.rs`

### 7.1 Genel Amac

FRAME runtime upgrade'lerinde coklu-block migrasyonlari (MBM). Runtime upgrade'i sırasında normal ve operational TX'ler duraklatilir, migrasyonlar `on_initialize` uzerinden yapilir.

### 7.2 Tasarim Hedefleri (lib.rs:52-99)

1. Migrasyonlar coklu block'ta otomatik
2. Devam eden migrasyon bilgisi
3. Pessimistic agirlik sinirlarına saygi
4. Sirali calistirma — atlamaya izin yok
5. Gecmis migrasyonlarin yeniden calismasini engelle
6. Transactional storage semantigi
7. Ilerleme garantisi (max_steps ile)

### 7.3 Storage Items

| Storage | Tip | Aciklama |
|---|---|---|
| `Cursor<T>` | `StorageValue<_, CursorOf<T>, OptionQuery>` | Aktif veya stuck migrasyon cursoru (lib.rs:463) |
| `Historic<T>` | `StorageMap<_, Twox64Concat, IdentifierOf<T>, (), OptionQuery>` | Tamamlanan migrasyonlar blacklist'i (lib.rs:470) |

```rust
pub enum MigrationCursor<Cursor, BlockNumber> {
    Active(ActiveCursor<Cursor, BlockNumber>),
    Stuck,  // governance mudahalesi gerekli
}

pub struct ActiveCursor<Cursor, BlockNumber> {
    pub index: u32,          // mevcut migrasyon indeksi
    pub inner_cursor: Option<Cursor>,
    pub started_at: BlockNumber,
}
```

### 7.4 on_runtime_upgrade Hook

```rust
fn on_runtime_upgrade() -> Weight {
    Self::onboard_new_mbms()  // lib.rs:537
}
```

`onboard_new_mbms`: Aktif migration varsa → UpgradeFailed → Stuck. Yoksa `Cursor` set edilir, `UpgradeStarted` emit edilir.

### 7.5 Extrinsics (Hepsi Root)

#### `force_set_cursor(origin, cursor)` — call_index(0)
- Acil durum: cursoru manuel set et

#### `force_set_active_cursor(origin, index, inner_cursor, started_at)` — call_index(1)
- `started_at=None` ise `current_block + 1` kullanilir

#### `force_onboard_mbms(origin)` — call_index(2)
- `Cursor` `None` olmali (Ongoing error)
- Migrations'ı manuel baslatir

#### `clear_historic(origin, selector)` — call_index(3)
- `HistoricCleanupSelector::Specific(ids)` veya `Wildcard { limit, previous_cursor }`
- Varsayilan wildcard limit: 128 (lib.rs:269)

### 7.6 exec_migration Akis

```rust
fn exec_migration(cursor, is_first, meter) -> Option<ControlFlow<...>> {
    // agirlik kontrolu
    if meter.try_consume(max_weight).is_err() {
        if is_first { panic! }
        return Some(ControlFlow::Break(cursor));
    }
    // all migrations done?
    if cursor.index >= T::Migrations::len() {
        UpgradeCompleted; Cursor::kill(); return None;
    }
    // already run?
    if Historic::contains_key(&bounded_id) {
        MigrationSkipped; cursor.goto_next_migration(); return Continue;
    }
    // calistir
    match T::Migrations::nth_transactional_step(...) {
        Ok(Some(next_cursor)) => {
            if max_steps exceeded { MigrationFailed; upgrade_failed; None }
            else { MigrationAdvanced; Break(cursor) }
        },
        Ok(None) => {
            // done!
            MigrationCompleted; Historic::insert; goto_next; Continue
        },
        Err(InsufficientWeight) => {
            if is_first || required > limit { MigrationFailed; None }
            else { Break(cursor) }  // bir sonraki block'ta tekrar
        },
        Err(_) => { MigrationFailed; upgrade_failed; None }
    }
}
// lib.rs:815-925
```

### 7.7 View Functions

```rust
pub fn ongoing_status() -> MbmIsOngoing  // lib.rs:679
pub fn progress() -> Option<MbmProgress>  // lib.rs:691
pub fn affected_prefixes() -> Vec<Vec<u8>>  // lib.rs:712
pub fn status() -> MbmStatus  // lib.rs:724
```

### 7.8 Events

| Event | Aciklama |
|---|---|
| `UpgradeStarted { migrations }` | Kac migrasyon var |
| `UpgradeCompleted` | Hepsi tamam |
| `UpgradeFailed` | Basarisiz — governance gerekli |
| `MigrationSkipped { index }` | Gecmiste calistirilmis, atlandi |
| `MigrationAdvanced { index, took }` | Devam ediyor |
| `MigrationCompleted { index, took }` | Bu migrasyon bitti |
| `MigrationFailed { index, took }` | Bu migrasyon basarisiz |
| `HistoricCleared { next_cursor }` | Gecmis temizlendi |

### 7.9 MultiStepMigrator Implementasyonu

```rust
impl<T: Config> MultiStepMigrator for Pallet<T> {
    fn ongoing() -> bool { Cursor::<T>::exists() }
    fn step() -> Weight { Self::progress_mbms(System::<T>::block_number()) }
}
// lib.rs:956-964
```

Bunu `frame_system::Config::MultiBlockMigrator` olarak konfigure etmek zorunlu.

---

## 8. Kritik Sayisal Degerler

| Deger | Kaynak | Aciklama |
|---|---|---|
| `safe-mode EnterDuration = 7` (mock) | mock.rs:151 | Production'da konfigurable |
| `safe-mode ExtendDuration = 10` (mock) | mock.rs:152 | Production'da konfigurable |
| `safe-mode EnterDepositAmount = 100` (mock) | mock.rs:153 | Production'da konfigurable |
| `safe-mode ReleaseDelay = 20 block` (mock) | mock.rs:155 | Production'da konfigurable |
| `tx-pause MaxNameLen = 50 byte` (mock) | mock.rs:107 | ASIM = PAUSED sayilir! |
| `fast-unstake MaxChecking = bonding_duration + 1` | types.rs:37 | Kontrol edilecek max era |
| `fast-unstake deposit slash` | lib.rs:572 | `slash_reserved(deposit)` — tam deposit |
| `pallet-migrations DEFAULT_HISTORIC_BATCH_CLEAR_SIZE = 128` | lib.rs:269 | Wildcard temizleme limiti |
| `pallet-migrations max MBM per block = 8` | lib.rs:791 | Savunmaci sinir |
| `state-trie MaxKeyLen` — NO DEFAULT (`#[pallet::no_default]`) | lib.rs:547-548 | Her runtime kendisi tanimlamali; 512 byte pallet'te hardcoded DEGIL |
| `state-trie deposit = items * PerItem + Base` | lib.rs:672 | Signed migration deposit |

---

## 9. Test Analizi

### 9.1 pallet-safe-mode Onemli Testler

- `fails_to_filter_calls_to_safe_mode_pallet`: SafeMode kendi cagrilarini engelleyemez (tests.rs:28)
- `can_filter_balance_calls_when_activated`: Balances engellenebilir (tests.rs:139)
- `can_filter_balance_in_batch_when_activated`: Batch icindeki cagrilar da filtrelenir (tests.rs:151)
- `can_filter_balance_in_proxy_when_activated`: Proxy cagrilari da filtrelenir (tests.rs:172)
- `can_activate`: Deposit block numarasina gore kaydedilir (tests.rs:193)
- `cannot_extend`: Zaten deposit varsa AlreadyDeposited (tests.rs:223)
- `fails_to_release_deposits_too_early`: `now > block + ReleaseDelay` gerekli (tests.rs:111)
- `can_release_deposit_while_entered`: force_release safe mode aktifken calisir (tests.rs:363)
- `can_slash_deposit_while_entered`: force_slash safe mode aktifken calisir (tests.rs:401)
- `notify_works`: SafeModeNotify enter/exit sirayla tetiklenir (tests.rs:209)

**Kritik test:** `fails_to_release_deposits_too_early` dogruluyor: `now > block + ReleaseDelay` (lib.rs:526 — strict greater than, not >=).

### 9.2 pallet-tx-pause Onemli Testler

- `can_pause_specific_call`: Sadece belirtilen call duraklatilir (tests.rs:27)
- `can_unpause_specific_call`: Unpause sonrasi calistirilabilir (tests.rs:67)
- `can_pause_all_calls_in_pallet_except_on_whitelist`: Whitelist atlar (tests.rs:46)
- `can_filter_balance_in_batch_when_paused`: Batch'te de filtrelenir (tests.rs:88)

### 9.3 pallet-fast-unstake Onemli Testler

- `register_works`: ErasToCheckPerBlock=1 gerekli (tests.rs:44)
- `register_insufficient_funds_fails`: Deposit yetersizse hata (tests.rs:54)
- `register_disabled_fails`: ErasToCheckPerBlock=0 iken hata (tests.rs:72)
- `cannot_register_if_has_unlocking_chunks`: Unlock chunks aktifken hata (tests.rs:129)
- `cannot_register_if_head`: Head'deyse hata (tests.rs:113)
- Whitelist tests: tum akis call/remove/dispatch uzerinden dogrulanir

### 9.4 pallet-parameters Onemli Testler

- `set_parameters_example`: Default calisiyor, root set edebiliyor, signed edemiyor (unit.rs:34)
- `set_parameters_same_is_noop`: Ayni degere set de Updated event emit eder (unit.rs:67)
- Per-key origin: Pallet2 icin signed yeterli, Pallet1 icin root gerekli (mock.rs:114-119)

---

## 10. Nova Shield Chain Icin Cikarsimlar

### 10.1 Acil Durum Altyapisi (Gereklilik: YUKSEK)

**pallet-safe-mode** veya **pallet-tx-pause** mutlaka entegre edilmeli:

**Safe Mode** — tercih edilir senaryo: Bir hack/istismar aninda HIZLA tum transkasyonlari durdurabilmek.
```rust
// frame_system konfigurasyonu:
type BaseCallFilter = InsideBoth<DefaultFilter, SafeMode>;
// veya ikisi birden:
type BaseCallFilter = InsideBoth<InsideBoth<DefaultFilter, SafeMode>, TxPause>;
```

**Onerilen konfigurasyonlar:**
- `EnterDepositAmount = Some(yeterli_miktar)` — sahte aktivasyonu engelle
- `EnterDepositAmount = None` — sadece privileged enter (daha guvenli baslangiç)
- `ForceEnterOrigin` = Council veya Technical Committee
- `ForceExitOrigin` = Root veya sudo
- `ForceDepositOrigin` = Root
- `WhitelistedCalls` = SafeMode + governance + sudo cagrilari

**TxPause** — ince granularity icin: Tek bir pallet veya cagri durdurulabilir. DEX exploit aninda sadece swap cagrilarini durdurabilirsiniz.

### 10.2 Whitelist Modeli (Gereklilik: ORTA)

pallet-whitelist, iki-katli guvenlik icin uygundur:
- Governance (council) bir cagriyi whitelist'e ekliyebilir
- Teknik ekip o cagriyi ROOT ile calistirabiliyor

Zaruri kosula: `Preimages` bağlantısı.

### 10.3 Fast Unstake (Gereklilik: ORTA-DUSUK)

Nova Shield Chain staking sistemi uyguluyorsa ve bonding suresi uzunsa (14+ gun), fast-unstake nominatorlara iyi bir UX saglar.

**Dikkat edilmesi gerekenler:**
1. `ErasToCheckPerBlock` degerini dikkatli ayarla — `on_idle` tam olarak harcanan block agirligini olmeli
2. Election sirasinda otomatik olarak atlanir — guvenli
3. `InternalError` event'ini izle — pallet kendini durdurur
4. Deposit mekanizması kotu niyet icin caydirici
5. `MaxErasToCheckPerBlock` benchmarking amacli biraz buyuk tutulmali

### 10.4 Dynamic Parameters (Gereklilik: ORTA)

pallet-parameters, runtime upgrade gerektiren degerler icin guçlu alternatif:
- DEX swap fee'si
- Staking odul orani
- Minimum bond miktari
- Parachain slot ucretleri

`AdminOrigin: EnsureOriginWithArg` ile her parametre icin farkli yetki seviyesi konfigüre edilebilir.

**Performans uyarisi:** Her `Get<T>` cagrisinda DB okuma. Weight kritik kodu icin dikkat.

### 10.5 Multi-Block Migrations (Gereklilik: YUKSEK — uzun vadeli)

pallet-migrations, coklu-block migrasyon altyapisi:
- Buyuk veri setlerinde runtime upgrade'leri guvenli yapar
- `FailedMigrationHandler = FreezeChainOnFailedMigration` varsayilan — basarisizlikta zinciri dondurur
- `UpgradeFailed` event izlenmeli

### 10.6 State Trie Migration (Duruma Gore)

Eger Nova Shield V1 state'e gecis yapilacaksa gerekli. Genellikle bir kez kullanilir.

### 10.7 Guvenlik Ozeti

| Pallet | Kritik Risk | Onlem |
|---|---|---|
| safe-mode | Yanlis deposit slash (AlreadyDeposited koruyor) | ForceDepositOrigin dikkatli sec |
| safe-mode | `extend()` — kotu niyet ile uzatma | ExtendDepositAmount yeterli yuksek tut |
| tx-pause | MaxNameLen asimi = call PAUSED (istenmeyebilir) | MaxNameLen yeterince buyuk tut |
| tx-pause | Pause/unpause origin ayri olmali | PauseOrigin != UnpauseOrigin |
| fast-unstake | Election sirasinda otomatik atlanir — guvenli | — |
| fast-unstake | `halt()` — pallet kendi kendini durdurur | InternalError event izle |
| whitelist | Deferred dispatch — whitelist revoke edilirse iptal | DeferredDispatchExpiration kisa tut |
| parameters | Her Get = DB okuma | Performans kritik yerlerde kullanma |
| migrations | Stuck state — governance mudahalesi | Monitoring zorunlu |

### 10.8 Entegrasyon Sirasi Onerisi

1. pallet-safe-mode (acil durum gerekliligi)
2. pallet-tx-pause (ince kontrol)
3. pallet-parameters (governance kolayligi)
4. pallet-whitelist (iki-katli guvenlik)
5. pallet-migrations (buyuk runtime upgrade'leri icin)
6. pallet-fast-unstake (staking sistemi tamamlandiktan sonra)
