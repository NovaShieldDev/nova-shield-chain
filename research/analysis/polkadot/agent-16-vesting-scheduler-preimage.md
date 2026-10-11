# pallet-vesting + pallet-scheduler + pallet-preimage — Tam Analiz
**Agent**: 16 | **Tarih**: 2026-07-22

---

## BÖLÜM 1: pallet-vesting

### 1.1 VestingInfo Yapısı

**Dosya**: `substrate/frame/vesting/src/vesting_info.rs`

```rust
pub struct VestingInfo<Balance, BlockNumber> {
    locked: Balance,       // Başlangıçta kilitlenen toplam miktar
    per_block: Balance,    // Her blokta açılan miktar
    starting_block: BlockNumber,  // Vesting'in başladığı blok
}
```

Kaynak: `vesting_info.rs:35-42`

**Erişim yöntemleri:**
- `locked()` → `self.locked` (kaynak: satır 65-67)
- `per_block()` → `self.per_block.max(One::one())` — 0 olursa 1'e yukarı sabitlenir (kaynak: satır 72-74)
- `raw_per_block()` → düzeltilmemiş `self.per_block` — yalnızca validasyon için (kaynak: satır 78-80)
- `starting_block()` → `self.starting_block` (kaynak: satır 83-85)

**is_valid()**: `!self.locked.is_zero() && !self.raw_per_block().is_zero()` — hem `locked` hem `per_block` sıfır olmamak zorunda (kaynak: satır 60-62)

**Önemli not**: Cliff vesting yoktur. Sadece doğrusal (linear) vesting desteklenmektedir. `starting_block`'tan önce hiçbir şey serbest bırakılmaz.

---

### 1.2 Vesting Unlock Formülü

**Dosya**: `substrate/frame/vesting/src/vesting_info.rs:88-101`

```rust
pub fn locked_at<BlockNumberToBalance: Convert<BlockNumber, Balance>>(
    &self,
    n: BlockNumber,
) -> Balance {
    // starting_block'tan bu yana geçen blok sayısı (saturating: n < starting_block => 0)
    let vested_block_count = n.saturating_sub(self.starting_block);
    let vested_block_count = BlockNumberToBalance::convert(vested_block_count);
    // Hâlâ kilitli olan miktar
    vested_block_count
        .checked_mul(&self.per_block())
        .map(|to_unlock| self.locked.saturating_sub(to_unlock))
        .unwrap_or(Zero::zero())
}
```

**Formül** (özet):
```
locked_at(n) = locked - ((n - starting_block) * per_block)
             = 0  eğer overflow veya fully vested
```

**Tam vesting anı** (ending_block hesabı):
```rust
// vesting_info.rs:104-124
duration = if per_block >= locked {
    1  // tek blokta biter
} else {
    locked / per_block + (locked % per_block != 0 ? 1 : 0)
}
ending_block_as_balance = starting_block + duration
```

Yani: `ending_block = starting_block + ceil(locked / per_block)`.

**vesting_balance()** (lib.rs:756-766):
Tüm schedule'ların `locked_at(now)` değerlerinin toplamı, `min(free_balance, total_locked_now)` ile sınırlanır.

---

### 1.3 Schedule Yönetimi

**Depolama**: `StorageMap<Blake2_128Concat, AccountId, BoundedVec<VestingInfo, MaxVestingSchedulesGet>>`
Kaynak: `lib.rs:228-233`

**Maksimum schedule sayısı**: `T::MAX_VESTING_SCHEDULES` — `const` olarak Config'de tanımlı.
Test değeri: `const MAX_VESTING_SCHEDULES: u32 = 3` (mock.rs:56)

**Schedule eklenirken**:
1. `try_push` ile BoundedVec'e eklenir — kapasite doluysa `AtMaxVestingSchedules` hatası (lib.rs:800)
2. Eklendikten sonra `exec_action(Passive)` ile mevcut kilitleme tutarı güncellenir
3. `add_vesting_schedule` sırasında event yayılır: `VestingCreated { account, schedule_index }` (lib.rs:804)

**Schedule çıkarılırken**:
- `VestingAction::Remove { index }` → belirli index kaldırılır
- `VestingAction::Merge { index1, index2 }` → ikisi kaldırılır, yeni birleşik schedule oluşturulabilir

**Tamamlanan schedulelar**: `report_schedule_updates` fonksiyonu `locked_at(now) == 0` olan scheduleları filtreler (lib.rs:598-618).

---

### 1.4 merge_schedules Mekanizması

**Dispatchable**: `merge_schedules(schedule1_index, schedule2_index)` — Signed origin, call_index(4).

**Birleştirme kuralları** (lib.rs:505-550):
```
merged.locked = schedule1.locked_at(now) + schedule2.locked_at(now)
merged.ending_block = MAX(schedule1.ending_block, schedule2.ending_block)
merged.starting_block = MAX(schedule1.starting_block, schedule2.starting_block, now)
merged.per_block = max(1, locked / max(1, ending_block - starting_block))
```

**Edge case'ler**:
- Her iki schedule da bitmiş → None (yeni schedule oluşturulmaz, ikisi de silinir)
- Biri bitmiş, diğeri devam ediyor → bitmemiş schedule aynen döner (birleştirme olmaz)
- `schedule1_index == schedule2_index` → no-op (lib.rs:444-446)

**force_remove_vesting_schedule**: Root origin ile belirli bir schedule'ı zorla siler (call_index(5), lib.rs:472-490).

---

### 1.5 Sabitler ve Konfigürasyon

| Sabit/Parametre | Değer (Test) | Kaynak |
|---|---|---|
| `MAX_VESTING_SCHEDULES` | `3` | mock.rs:56 |
| `MinVestedTransfer` | `256 * 2 = 512` | mock.rs:47 |
| `VESTING_ID` | `b"vesting "` (8 byte) | lib.rs:94 |
| `UnvestedFundsAllowedWithdrawReasons` | `except(TRANSFER | RESERVE)` | mock.rs:48-50 |

Lock `WithdrawReasons::except(UnvestedFundsAllowedWithdrawReasons)` ile set edilir (lib.rs:626-627). Bu, allowlist değil denylist mantığıdır.

---

### 1.6 Vest / Vest_other Mekanizması

**do_vest(who)** akışı (lib.rs:655-665):
1. `Vesting::get(&who)` → schedules yoksa `NotVesting` hatası
2. `exec_action(schedules, VestingAction::Passive)` → tamamlananlar filtrelenir, kalan locked hesaplanır
3. `write_vesting(&who, schedules)` → schedules boşsa storage'dan silinir
4. `write_lock(&who, locked_now)` → sıfırsa lock kaldırılır + `VestingCompleted` eventi, değilse `set_lock` + `VestingUpdated` eventi

**Önemli**: `vest()` / `vest_other()` manuel olarak çağrılmak ZORUNDADIR. Vesting otomatik değil, kullanıcı tetikler.

---

### 1.7 VestedTransfer ve VestedPayout Trait Implementasyonları

**VestedTransfer** (lib.rs:854-879): Başka palletler tarafından kullanılır. Transactional wrapper (`with_transaction`) içinde `do_vested_transfer` çağrılır — hata durumunda rollback yapılır.

**VestedPayout.vested_transfer** (lib.rs:719-746):
- `duration == 0` → direkt liquid transfer, vesting yok
- `per_block` formülü: `ceil(amount / duration)` = `(amount + duration - 1) / duration` — vesting tam `duration` blok içinde biter, daha uzun sürmez

---

### 1.8 Test ve Edge Case'ler

**check_vesting_status** (tests.rs:45-101): Temel vesting formülü doğrulaması. Block 1'de user1 `128*9 = 1152` kilide sahip. Block 10'da tam vesting.

**check_vesting_status_for_multi_schedule_account** (tests.rs:104-178): 3 aynı anda aktif schedule, her biri bağımsız hesaplanır, toplamları birleşir.

**unvested_balance_should_not_transfer** (tests.rs:182-190): `TokenError::Frozen` — kullanıcı kilitlenen bakiyeyi transfer edemez.

**Görülemeyen cliff**: `starting_block`'a kadar tüm `locked` miktarı kilitli. Cliff mekanizması yoktur; sadece "başlangıç bloğu" gecikmesi vardır.

**Storage migration**: V0 → V1: tek schedule → BoundedVec. `migrations.rs:24-96`.

---

## BÖLÜM 2: pallet-scheduler

### 2.1 Agenda Yapısı

**Depolama** (lib.rs:344-350):
```rust
pub type Agenda<T: Config> = StorageMap<
    _,
    Twox64Concat,
    BlockNumberFor<T>,
    BoundedVec<Option<ScheduledOf<T>>, T::MaxScheduledPerBlock>,
    ValueQuery,
>;
```

`Option<ScheduledOf<T>>` — silinmiş görevler `None` bırakır ("slot"), BoundedVec sıkıştırılmaz (kaynak: lib.rs:1004-1013).

**Scheduled struct** (lib.rs:161-174):
```rust
pub struct Scheduled<Name, Call, BlockNumber, PalletsOrigin, AccountId> {
    pub maybe_id: Option<Name>,           // Opsiyonel isim (32 byte Blake2-256 hash)
    pub priority: schedule::Priority,     // u8, düşük = yüksek öncelik
    pub call: Call,                       // BoundedCallOf<T> (Inline veya Lookup)
    pub maybe_periodic: Option<schedule::Period<BlockNumber>>,  // (period, repetitions_remaining)
    pub origin: PalletsOrigin,
    pub _phantom: PhantomData<AccountId>,
}
```

**Named task lookup** (lib.rs:367-368):
```rust
pub type Lookup<T: Config> =
    StorageMap<_, Twox64Concat, TaskName, TaskAddress<BlockNumberFor<T>>>;
```
`TaskName = [u8; 32]` — v4'te Blake2-256 hash (onceden `Vec<u8>` idi).
`TaskAddress<BlockNumber> = (BlockNumber, u32)` — (blok_numarası, agenda_index).

**Retry depolama** (lib.rs:354-360):
```rust
pub type Retries<T: Config> = StorageMap<
    _, Blake2_128Concat,
    TaskAddress<BlockNumberFor<T>>,
    RetryConfig<BlockNumberFor<T>>,
    OptionQuery,
>;
```

**RetryConfig** (lib.rs:139-146):
```rust
pub struct RetryConfig<Period> {
    pub total_retries: u8,
    pub remaining: u8,
    pub period: Period,
}
```

---

### 2.2 Schedule Mekanizmaları

**6 dispatchable** (tümü `ScheduleOrigin` gerektirir):

| Call Index | Fonksiyon | Açıklama |
|---|---|---|
| 0 | `schedule(when, periodic, priority, call)` | Anonim, belirli blok |
| 1 | `cancel(when, index)` | Anonim/named iptal |
| 2 | `schedule_named(id, when, periodic, priority, call)` | İsimli, belirli blok |
| 3 | `cancel_named(id)` | İsimli iptal |
| 4 | `schedule_after(after, periodic, priority, call)` | Anonim, gecikme sonrası |
| 5 | `schedule_named_after(id, after, ...)` | İsimli, gecikme sonrası |
| 6 | `set_retry(task, retries, period)` | Retry config ayarla |
| 7 | `set_retry_named(id, retries, period)` | İsimli retry config |
| 8 | `cancel_retry(task)` | Retry config sil |
| 9 | `cancel_retry_named(id)` | İsimli retry config sil |

**schedule_after** zaman çözümü (lib.rs:963-979):
```rust
DispatchTime::After(x) => now.saturating_add(x).saturating_add(One::one())
```
`schedule_after(0)` → `now + 1` (bir sonraki blok).
`schedule_after(3)` at block 2 → block `2 + 3 + 1 = 6`.

**do_schedule periodic sanitization** (lib.rs:1049-1052):
```rust
let maybe_periodic = maybe_periodic
    .filter(|p| p.1 > 1 && !p.0.is_zero())  // count > 1 ve period > 0 zorunlu
    .map(|(p, c)| (p, c - 1));               // ilk çalışma sayılır, count-1 kalır
```

---

### 2.3 Periodic Task Mekanizması

**Period = (period_blocks, count_remaining)**.

**service_task içinde periodic yönetimi** (lib.rs:1404-1429):
```rust
if let &Some((period, count)) = &task.maybe_periodic {
    if count > 1 {
        task.maybe_periodic = Some((period, count - 1));
    } else {
        task.maybe_periodic = None;  // son çalışma, periodic bitir
    }
    let wake = now.saturating_add(period);
    match Self::place_task(wake, task) {
        Ok(new_address) => { /* retry config da taşınır */ },
        Err((_, task)) => {
            T::Preimages::drop(&task.call);
            Self::deposit_event(Event::PeriodicFailed { ... });
        },
    }
}
```

**Önemli**: Periodic task başarıyla çalıştıktan sonra `now + period` bloğuna yeniden eklenir. Agenda doluysa `PeriodicFailed` eventi yayılır, görev **kaybolur** (manuel reschedule gerekir — kod içinde TODO notu var: lib.rs:1418).

**Test**: `periodic_scheduling_works` (tests.rs:152-181): `(3, 3)` = her 3 blokta, 3 kez. Block 4, 7, 10'da çalışır.

---

### 2.4 Weight Yönetimi

**on_initialize** (lib.rs:423-429):
```rust
fn on_initialize(_now: SystemBlockNumberFor<T>) -> Weight {
    let now = T::BlockNumberProvider::current_block_number();
    let mut weight_counter = frame_system::Pallet::<T>::remaining_block_weight()
        .limit_to(T::MaximumWeight::get());
    Self::service_agendas(&mut weight_counter, now, u32::MAX);
    weight_counter.consumed()
}
```

**service_agendas döngüsü** (lib.rs:1233-1264):
- `IncompleteSince` değerinden başlar (geçmiş tamamlanmamış agendaları yakalamak için)
- `now` bloğuna kadar tüm agendaları işler
- Her agenda için `service_agenda_base` weight tüketilir
- Weight biter → döngü kırılır, `IncompleteSince` güncellenir

**service_agenda priority sıralaması** (lib.rs:1276-1283):
```rust
ordered.sort_by_key(|k| k.1);  // priority u8, düşük değer = önce çalışır
```

**Overweight handling** (lib.rs:1379-1387):
- Eğer görev `is_first == true` ve overweight → `PermanentlyOverweight` eventi + `Unavailable` olarak işaretlenir
- `is_first == false` ise `Overweight` olarak ertelenir (bir sonraki blok/döngüde tekrar denenebilir)

**MaximumSchedulerWeight** (test mock): `Perbill::from_percent(80) * BlockWeights::max_block` (mock.rs:218-219).

---

### 2.5 Named Task

**Ekleme** (lib.rs:1129-1131):
```rust
if Lookup::<T>::contains_key(&id) {
    return Err(Error::<T>::FailedToSchedule.into());
}
```
İsim benzersizliği zorunlu.

**Arama** (lib.rs:1606-1610):
```rust
fn next_dispatch_time(id: TaskName) -> Result<...> {
    Lookup::<T>::get(id)
        .and_then(|(when, index)| Agenda::<T>::get(when).get(index as usize).map(|_| when))
        .ok_or(DispatchError::Unavailable)
}
```

**TaskName**: `[u8; 32]` = Blake2-256 hash. V1/V2/V3'ten migration'da: `blake2_256(&old_name)`. (lib.rs:727, 779, 898)

**İptal** (do_cancel_named, lib.rs:1162-1184):
1. Lookup'tan sil
2. Agenda'da None'a set
3. cleanup_agenda çağır (trailing None'lar temizlenir)
4. Retries temizle

**OriginPrivilegeCmp**: İptal/retry için origin aynı ya da daha yüksek ayrıcalıklı olmak zorunda (lib.rs:1526-1535).

---

### 2.6 Sabitler

| Parametre | Değer (Test) | Kaynak |
|---|---|---|
| `MaxScheduledPerBlock` | `10` | mock.rs:230 |
| `MaximumWeight` | `80% * max_block` | mock.rs:218-219 |
| `STORAGE_VERSION` | `4` | lib.rs:247 |
| `PeriodicIndex` | `u32` | lib.rs:113 |
| Priority range | `u8` (0 = en yüksek) | schedule trait |

**IncompleteSince**: `StorageValue<_, BlockNumberFor<T>>` — işlenemeyen agenda'lar için geri dönüş noktası (lib.rs:340).

---

### 2.7 Retry Mekanizması

**set_retry** (call_index 6, lib.rs:584-607):
- Anonim task için retry ayarlar
- `total_retries` ve `remaining` ikisi de `retries` ile başlar

**schedule_retry** (lib.rs:1480-1521):
1. `remaining - 1` azaltılır, 0 olursa retry biter
2. `now + period` bloğuna `task.as_retry()` (anonim, non-periodic klon) eklenir
3. Başarılıysa yeni adrese `RetryConfig { total_retries, remaining: remaining-1, period }` eklenir
4. Agenda doluysa `RetryFailed` eventi, preimage drop edilir (TODO: kaybolur)

---

### 2.8 Preimage Entegrasyonu (Scheduler tarafı)

**do_schedule** içinde (lib.rs:1063-1066):
```rust
if let Some(hash) = lookup_hash {
    T::Preimages::request(&hash);  // hash varlığı talep edilir
}
```

**service_task** içinde (lib.rs:1349-1370): `T::Preimages::peek(&task.call)` ile call okunur.
- Başarılı → call execute edilir
- `CallUnavailable` hatası → `T::Preimages::drop` + event

**Görev tamamlandığında** (lib.rs:1428): `T::Preimages::drop(&task.call)` — preimage talep kaydı temizlenir.

**BoundedCallOf türleri** (Bounded enum):
- `Inline(BoundedInline)` — küçük call'lar doğrudan saklanır, lookup gerektirmez
- `Lookup { hash, len }` — büyük call'lar hash ile referenced, preimage'tan çekilir
- `Legacy { hash }` — eski V3 migration için

---

## BÖLÜM 3: pallet-preimage

### 3.1 RequestStatus Enum

**OldRequestStatus** (lib.rs:71-79) — deprecated, migration için:
```rust
pub enum OldRequestStatus<AccountId, Balance> {
    Unrequested { deposit: (AccountId, Balance), len: u32 },
    Requested { deposit: Option<(AccountId, Balance)>, count: u32, len: Option<u32> },
}
```

**RequestStatus** (lib.rs:85-93) — mevcut:
```rust
pub enum RequestStatus<AccountId, Ticket> {
    Unrequested { ticket: (AccountId, Ticket), len: u32 },
    Requested { maybe_ticket: Option<(AccountId, Ticket)>, count: u32, maybe_len: Option<u32> },
}
```

**Fark**: `Balance` yerine `Ticket` (Consideration trait) kullanılıyor — yeni hold mekanizması.

**Depolama** (lib.rs:184-186):
```rust
pub type RequestStatusFor<T: Config> =
    StorageMap<_, Identity, T::Hash, RequestStatus<T::AccountId, TicketOf<T>>>;
```

**PreimageFor depolama** (lib.rs:188-190):
```rust
pub type PreimageFor<T: Config> =
    StorageMap<_, Identity, (T::Hash, u32), BoundedVec<u8, ConstU32<MAX_SIZE>>>;
```
Anahtar = `(hash, len)` çifti.

---

### 3.2 Deposit Sistemi

**Eski sistem (deprecated StatusFor)**: `ReservableCurrency` ile `T::Currency::reserve`.

**Yeni sistem (RequestStatusFor)**: `T::Consideration` trait ile `HoldConsideration`.

**Test mock deposit formülü** (mock.rs:66-70):
```rust
impl Convert<Footprint, u64> for ConvertDeposit {
    fn convert(a: Footprint) -> u64 {
        a.count * 2 + a.size  // count=1, size=len => deposit = 2 + len
    }
}
```

Yani: `deposit = 2 + byte_length`. Test: 1 byte preimage → `deposit = 3` (tests.rs:61).

**Kim öder**:
- `note_preimage(signed)` → `maybe_depositor = Some(sender)` → Consideration alınır → `Unrequested { ticket }`
- `note_preimage(manager_origin)` → `maybe_depositor = None` → depozito yok → `Requested { count: 1 }`
- Önceden `request_preimage` çağrılmışsa → `note_preimage` depozito almaz, tx fee de alınmaz

**Depozito iade** (do_unnote_preimage, lib.rs:423-429):
```rust
let _ = ticket.drop(&owner);  // Consideration drop = hold release
```

---

### 3.3 Size Limiti

```rust
pub const MAX_SIZE: u32 = 4 * 1024 * 1024;  // 4 MB
```
Kaynak: `lib.rs:100`

`StorePreimage::MAX_LENGTH = MAX_SIZE as usize = 4194304` (lib.rs:572).

**Önemli**: SCALE encoding eklenince gerçek limit daha küçüktür. Vec için SCALE 4 byte prefix ekler. Test (tests.rs:490-497): `MAX_LENGTH` boyutunda `Vec<u8>` SCALE-encode edilince `MAX_LENGTH + 4 > MAX_SIZE` → `TooBig` hatası. `MAX_LENGTH - 4` boyutunda Vec çalışır.

**TooBig hatası** `DispatchError::Exhausted` olarak map'lenir (lib.rs:578-582).

---

### 3.4 Preimage Operasyonları

**note_preimage** (call_index 0, lib.rs:200-210):
- Signed veya ManagerOrigin kabul eder
- `note_bytes(bytes, maybe_sender)` çağrılır
- `system_requested` ise veya Manager origin → `Pays::No` (ücretsiz)

**unnote_preimage** (call_index 1, lib.rs:219-222):
- Sahibi veya ManagerOrigin kaldırabilir
- `Requested` durumundaki preimage → sahiplik bırakılır ama veri kalır
- `Unrequested` durumu → tamamen silinir

**request_preimage** (call_index 2, lib.rs:229-233): Yalnızca `ManagerOrigin`. Counter `count++`.

**unrequest_preimage** (call_index 3, lib.rs:239-242): Yalnızca `ManagerOrigin`. Counter `count--`.
- `count == 1` ve `maybe_len == None` → entry kaldırılır (veri hiç gelmedi)
- `count == 1` ve `maybe_len == Some` → veri silinir
- `count == 1` ve `maybe_ticket == Some` → `Unrequested`'a geçiş (sahibi geri alabilsin)

---

### 3.5 Scheduler Entegrasyonu

**Akış** (tam döngü):
1. Scheduler `do_schedule(Lookup { hash, len })` çağrısında `T::Preimages::request(&hash)` çağrır
2. Kullanıcı `preimage.note_preimage(bytes)` ile veriyi yükler — önce request geldiyse deposit alınmaz
3. Scheduler `on_initialize`'da `service_task` → `T::Preimages::peek(&task.call)` ile call okunur
4. Call başarıyla execute edilirse `T::Preimages::drop(&task.call)` → `count--`
5. Eğer preimage hâlâ `Unrequested`'a dönmüşse, sahibi `unnote_preimage` ile depozitosunu geri alabilir

**QueryPreimage trait** (lib.rs:545-569): `request`, `unrequest`, `len`, `fetch`, `is_requested` metodları.

**StorePreimage trait** (lib.rs:571-591): `note(bytes)`, `unnote(hash)`, `bound(value)` metodları. `bound` = encode + note + `Bounded::Lookup` döner.

**ensure_updated** (call_index 4, lib.rs:248-262): Eski `StatusFor` → yeni `RequestStatusFor` geçişi. Batch migration. `>= 90%` başarı → ücretsiz.

---

### 3.6 Traits

Pallet üç farklı trait implementasyonu sunar (eski + yeni API uyumluluğu):
- `PreimageProvider<T::Hash>` — eski API (lib.rs:505-527)
- `PreimageRecipient<T::Hash>` — eski runtime-internal API (lib.rs:529-543)
- `QueryPreimage` + `StorePreimage` — yeni modern API (lib.rs:545-591)

---

## Kritik Bulgular Özeti

1. **Vesting unlock formülü**: `locked_at(n) = locked - ((n - starting_block) * per_block)`, saturating_sub (lib.rs vesting_info.rs:97-100)

2. **per_block minimum 1**: `per_block()` metodu `max(1, raw_per_block)` döner — sonsuz vesting imkansız (vesting_info.rs:73)

3. **Vest MANUEL**: `vest()` / `vest_other()` çağrılmadan lock azalmaz. Otomatik güncelleme yok. (lib.rs:329-332)

4. **MAX_VESTING_SCHEDULES**: Aynı anda maksimum N schedule. Test mock'ta 3 (mock.rs:56). Config const olarak tanımlı (lib.rs:208)

5. **Cliff vesting yok**: `starting_block` delay vardır ama gerçek cliff (belirli süre sonra toplu unlock) implementasyonu yoktur.

6. **Merge formülü**: `merged.locked = s1.locked_at(now) + s2.locked_at(now)` — hem sayım hem birleştirme şu anki blokta yapılır (lib.rs:526-528)

7. **Scheduler STORAGE_VERSION = 4**: Dört farklı storage versiyonu geçilmiş (V1→V4). TaskName artık `[u8; 32]` Blake2-256 hash (lib.rs:247)

8. **Priority sıralaması**: `u8`, küçük değer = önce çalışır. `sort_by_key(|k| k.1)` (lib.rs:1283)

9. **Periodic task kaybolabilir**: Agenda doluysa `PeriodicFailed` + preimage drop. `TODO: Leave task in storage` yorumu var (lib.rs:1418)

10. **Retry mekanizması**: `u8` (max 255 deneme), period bloklarda. Her deneme yeni anonim unnamed task (lib.rs:184-193)

11. **Preimage MAX_SIZE = 4 MB**: `4 * 1024 * 1024` bytes (preimage/lib.rs:100)

12. **Preimage deposit formülü**: `count * 2 + size` (mock.rs:68). Gerçek üretimde `HoldConsideration` ile konfigüre edilir.

13. **Request/Note sırasının önemi yok**: `request_note_order_makes_no_difference` testi bunu doğrular (preimage/tests.rs:175-193)

14. **ensure_updated ücretsiz eşiği**: `>= 90%` başarı oranı → `Pays::No` (preimage/lib.rs:260)

15. **Scheduler Overweight**: İlk task overweight ise `PermanentlyOverweight` eventi ve kalıcı olarak düşürülür. Sonraki tasklar ise sadece ertelenir (lib.rs:1379-1387)

---

## Nova Shield Chain Notları

### Vesting için Tavsiyeler

1. **`MAX_VESTING_SCHEDULES` seçimi**: 3-5 arası makul. Çok fazla schedule = her vest() çağrısında O(N) işlem. Polkadot testinde 3 kullanılmış.

2. **Cliff vesting gereksinimi**: Polkadot pallet-vesting cliff desteklemiyor. Nova Shield'da cliff gerekiyorsa özel bir `cliff_block` alanı eklenmelidir. `locked_at` şu an `max(0, locked - (n - start) * per_block)` şeklinde doğrusal; cliff için `if n < cliff { return locked; }` eklenmeli.

3. **`per_block` hesabı**: Genesis'te `per_block = locked / length_as_balance` kullanılır (lib.rs:269). Integer bölme → son blokta küçük bir miktar kalabilir. `VestedPayout`'ta `ceil` kullanılır (lib.rs:739). Tutarsızlık: genesis `floor`, payout `ceil`. Nova Shield'da `ceil` kullanmak önerilir.

4. **Vest trigger**: Kullanıcıların `vest()` çağırması için on-chain teşvik veya fee-free mekanizması düşünülmeli. Yoksa kilit asla kaldırılmaz (kullanıcı farkında olmayabilir).

5. **`UnvestedFundsAllowedWithdrawReasons`**: Staking için stake edilebilmesine izin verilmek isteniyorsa `WithdrawReasons::except(TRANSFER)` → sadece transfer yasakla, reserve/stake serbest.

### Scheduler için Tavsiyeler

6. **`MaxScheduledPerBlock` seçimi**: 10-50 arası öneri. Çok fazla → on_initialize ağır. Üretimde ağırlık kısıtlaması da korunur; MaximumWeight genellikle `%80 * max_block`.

7. **Periodic task güvenilirliği**: Agenda doluysa periodic task silinir (`PeriodicFailed`). Kritik periodic işlemler için redundant schedule veya off-chain watcher önerilir. Polkadot'ta henüz çözülmemiş bir TODO (lib.rs:1418).

8. **TaskName v4**: 32-byte Blake2-256. Migration'larda eski string isimler hash'lenir — çakışma riski teorik ama pratikte ihmal edilebilir.

9. **`schedule_after(0)` = next block**: `DispatchTime::After(0)` → `now + 1`. Aynı blokta çalıştırma imkansız.

10. **Origin privilege check**: Cancel için caller, creator ile aynı veya daha yüksek privilege'a sahip olmalı. Yanlış config → kimse cancel edemez (lock). `EqualPrivilegeOnly` en güvenli seçenek.

### Preimage için Tavsiyeler

11. **4 MB limit**: Büyük governance proposal'ları için yeterli. Ancak her preimage on-chain storage'da saklanır — Node'lar için disk baskısı. Nova Shield'da limit küçültülmesi (örn. 1 MB) değerlendirilebilir.

12. **Deposit modeli**: `HoldConsideration` (yeni hold API) kullan, `ReservableCurrency` deprecated. Hold, hesabın toplam bakiyesini etkilemez — staking ile çakışmaz.

13. **ManagerOrigin**: Kim preimage talep edebilir? Genellikle governance. Yanlış config → kötü aktörler sonsuz preimage talep edebilir (DoS).

14. **Scheduler + Preimage entegrasyonu**: Scheduler `T::Preimages` type'ı olarak pallet-preimage kullanır. Large call'lar için `Bounded::Lookup` zorunlu. Bu iki pallet birlikte deploy edilmeli.

15. **ensure_updated**: Migration extrinsic — eski `StatusFor`'daki entryleri `RequestStatusFor`'a taşır. Yeni deploy'da bu gereksiz; sadece upgrade senaryosunda kullanılır.
