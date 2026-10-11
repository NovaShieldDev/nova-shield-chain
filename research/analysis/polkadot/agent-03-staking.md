# Substrate pallet-staking — Agent 03

## Kapsam

Analiz edilen dizin: `/substrate/frame/staking/`

Toplam analiz edilen dosyalar ve satır sayıları:

| Dosya | Satır |
|---|---|
| src/lib.rs | 1394 |
| src/pallet/mod.rs | 2338 |
| src/pallet/impls.rs | 2563 |
| src/slashing.rs | 821 |
| src/ledger.rs | 310 |
| src/inflation.rs | 108 |
| src/asset.rs | 157 |
| src/election_size_tracker.rs | 259 |
| src/migrations.rs | 439 |
| reward-fn/src/lib.rs | 224 |
| reward-curve/src/lib.rs | 452 |
| **TOPLAM** | **~9.065** |

**UYARI**: Pallet başlığı şunu belirtir:
```
//! NOTE: This pallet is deprecated and no longer maintained in favour of `pallet-staking-async`.
```
Yani bu pallet devre dışı bırakılma sürecindedir. Ancak Polkadot ana ağında hâlâ kullanılan referans implementasyondur ve tüm design kararları `pallet-staking-async` için de geçerlidir.

Storage versiyonu: `StorageVersion::new(16)` — 16. versiyon, kayda değer migration geçmişi var.

---

## Validator/Nominator State Machine

### Kavramsal Model

Pallet-staking, Nominated Proof-of-Stake (NPoS) mantığını uygular. İki ana aktör vardır:

- **Validator**: Blok üreten node. Kendi payını stake eder, ek olarak nominatörlerin payına göre election'a girer.
- **Nominator**: Validator seçen ve stake'ini onların arkasına koyan hesap.

### Temel Durum Geçişleri

```
UNBONDED ──bond()──> BONDED
                        |
              validate() / nominate()
                        |
              ┌─────────┴──────────┐
              V                    V
         VALIDATOR            NOMINATOR
              |                    |
              └─────────┬──────────┘
                    chill()
                        |
                        V
                  CHILLED (bonded fakat rol yok)
                        |
                    unbond()
                        |
                        V
                  UNBONDING (UnlockChunk'lar)
                        |
              withdraw_unbonded() (era dolunca)
                        |
                        V
                  UNBONDED (serbest balance)
```

### StakingAccount Enum

```rust
// sp_staking içinde
pub enum StakingAccount<AccountId> {
    Stash(AccountId),
    Controller(AccountId),
}
```

Controller hesabı deprecated edilmekte; artık stash == controller (1:1 eşleme). `set_controller_to_stash()` ile migration yapılır.

### VirtualStakers

```rust
pub type VirtualStakers<T: Config> = CountedStorageMap<_, Twox64Concat, T::AccountId, ()>;
```

Pool paleti ve diğer pallet'lar tarafından yönetilen "sanal" stake'ler. Bu hesaplara lock uygulanmaz, balance hold sistemiyle takip edilir. Slash olursa ilgili pallet handle eder.

---

## Era Sistemi

### Era-Session-Block İlişkisi

```
Block ──on_finalize──> Session ──end_session──> Era
                                ──start_session──> Era
```

- Bir **era** = `T::SessionsPerEra` kadar session
- Test default: `SessionsPerEra = 3`
- Polkadot production: SessionsPerEra = 6, BondingDuration = 28 era
- Session süresi ağ konfigürasyonuna bağlı (tipik 4 saat = 6 session × 4 saat = 24 saatlik era)

### Era Transition Flow (satır satır)

`pallet/impls.rs` — `new_session()` fonksiyonu (satır 452–494):

```rust
fn new_session(session_index: SessionIndex, is_genesis: bool)
    -> Option<BoundedVec<T::AccountId, MaxWinnersOf<T>>> {
    if let Some(current_era) = CurrentEra::<T>::get() {
        let era_length = session_index - current_era_start_session_index;
        match ForceEra::<T>::get() {
            Forcing::ForceNew => (),      // yeni era zorla
            Forcing::ForceAlways => (),   // her session'da yeni era
            Forcing::NotForcing if era_length >= T::SessionsPerEra::get() => (),  // normal
            _ => return None,             // henüz değil
        }
        Self::try_trigger_new_era(session_index, is_genesis)
    }
}
```

`start_session()` (satır 497–513): `next_active_era` için `ErasStartSessionIndex` kaydına bakılır, match varsa `start_era()` çağrılır.

`end_session()` (satır 517–527): Aktif era'nın bir sonraki session başlangıcı mevcut session+1'e eşleşiyorsa `end_era()` çağrılır.

### start_era() Detayı (satır 533–568)

```rust
fn start_era(start_session: SessionIndex) {
    let active_era = ActiveEra::<T>::mutate(|active_era| {
        let new_index = active_era.as_ref().map(|info| info.index + 1).unwrap_or(0);
        *active_era = Some(ActiveEraInfo { index: new_index, start: None });
        new_index
    });

    let bonding_duration = T::BondingDuration::get();
    BondedEras::<T>::mutate(|bonded| {
        bonded.push((active_era, start_session));

        if active_era > bonding_duration {
            let first_kept = active_era - bonding_duration;
            // prune old eras
            let n_to_prune = bonded.iter().take_while(|&&(era_idx, _)| era_idx < first_kept).count();
            for (pruned_era, _) in bonded.drain(..n_to_prune) {
                slashing::clear_era_metadata::<T>(pruned_era);
            }
            T::SessionInterface::prune_historical_up_to(first_session);
        }
    });

    Self::apply_unapplied_slashes(active_era);  // deferred slash'ları uygula
}
```

### end_era() ve Era Payout Hesabı (satır 571–601)

```rust
fn end_era(active_era: ActiveEraInfo, _session_index: SessionIndex) {
    if let Some(active_era_start) = active_era.start {
        let era_duration = now_as_millis_u64 - active_era_start;  // millisaniye
        let staked = ErasTotalStake::<T>::get(&active_era.index);
        let issuance = asset::total_issuance::<T>();

        let (validator_payout, remainder) =
            T::EraPayout::era_payout(staked, issuance, era_duration);

        // MaxStakedRewards cap uygulaması
        let total_payout = validator_payout.saturating_add(remainder);
        let max_staked_rewards = MaxStakedRewards::<T>::get()
            .unwrap_or(Percent::from_percent(100));

        let validator_payout = validator_payout.min(max_staked_rewards * total_payout);
        let remainder = total_payout.saturating_sub(validator_payout);

        ErasValidatorReward::<T>::insert(&active_era.index, validator_payout);
        T::RewardRemainder::on_unbalanced(asset::issue::<T>(remainder));
    }
}
```

**Kritik not**: `era_duration` millisaniye cinsinden. `MILLISECONDS_PER_YEAR = 1000 * 3600 * 24 * 36525 / 100` (Julian yıl = 365.25 gün).

### Forcing Enum (mod.rs satır 1022–1034)

```rust
pub enum Forcing {
    NotForcing,        // normal davranış
    ForceNew,          // bir sonraki session'da yeni era (sonra NotForcing'e döner)
    ForceNone,         // yeni era hiçbir zaman (governance müdahalesi)
    ForceAlways,       // her session'da yeni era (test/emergency)
}
```

---

## Reward Hesabı

### Toplam Era Payout Formülü

`src/inflation.rs` — `compute_total_payout()` (satır 32–50):

```rust
pub fn compute_total_payout<N>(
    yearly_inflation: &PiecewiseLinear<'static>,
    npos_token_staked: N,
    total_tokens: N,
    era_duration: u64,   // millisaniye
) -> (N, N)
{
    const MILLISECONDS_PER_YEAR: u64 = 1000 * 3600 * 24 * 36525 / 100;
    let portion = Perbill::from_rational(era_duration, MILLISECONDS_PER_YEAR);
    let payout = portion * yearly_inflation
        .calculate_for_fraction_times_denominator(npos_token_staked, total_tokens.clone());
    let maximum = portion * (yearly_inflation.maximum * total_tokens);
    (payout, maximum)
}
```

**Formül özeti**:
```
staker_payout = (era_duration / year_ms) × inflation(staked_ratio) × total_issuance
maximum      = (era_duration / year_ms) × max_inflation × total_issuance
```

### Inflation Eğrisi (reward-fn)

`reward-fn/src/lib.rs` — `compute_inflation()` (satır 54–101):

```
I(x) = x / x_ideal                    eğer x <= x_ideal
I(x) = 2^((x_ideal - x) / decay_rate) eğer x > x_ideal
```

Parametreler (örnek test değerleri):
- `min_inflation = 0.025` (2.5%)
- `max_inflation = 0.100` (10%)
- `ideal_stake = 0.500` (%50 stake oranı)
- `falloff = 0.050` (5% decay)

Bu formül Taylor serisi ile hesaplanır (maksimum 300 iterasyon). Sonuç `PiecewiseLinear` struct'ına dönüştürülür.

### payout_stakers — Tam Dağıtım Formülü

`pallet/impls.rs` — `do_payout_stakers_by_page()` (satır 253–398):

**1. Adım: Validator'ın reward payını bul**
```rust
let validator_total_reward_part =
    Perbill::from_rational(validator_reward_points, total_reward_points);

let validator_total_payout = validator_total_reward_part * era_payout;
```

**2. Adım: Validator komisyonu hesapla**
```rust
let validator_commission = EraInfo::<T>::get_validator_commission(era, &ledger.stash);
let validator_total_commission_payout = validator_commission * validator_total_payout;
let validator_leftover_payout =
    validator_total_payout - validator_total_commission_payout;
```

**3. Adım: Validator'ın kendi stake payı (komisyon hariç)**
```rust
let validator_exposure_part = Perbill::from_rational(exposure.own(), exposure.total());
let validator_staking_payout = validator_exposure_part * validator_leftover_payout;
```

**4. Adım: Sayfalı komisyon dağıtımı**
```rust
let page_stake_part = Perbill::from_rational(exposure.page_total(), exposure.total());
let validator_commission_payout = page_stake_part * validator_total_commission_payout;
// Validator: kendi stake payı + bu sayfanın komisyon payı
// Toplam: validator_staking_payout + validator_commission_payout
```

**5. Adım: Nominatör ödemeleri**
```rust
for nominator in exposure.others().iter() {
    let nominator_exposure_part = Perbill::from_rational(nominator.value, exposure.total());
    let nominator_reward = nominator_exposure_part * validator_leftover_payout;
    Self::make_payout(&nominator.who, nominator_reward);
}
```

**Tam Formül Özeti**:
```
era_payout                 — o era için toplam validator havuzu
validator_reward_part      = validator_points / total_points
validator_total_payout     = validator_reward_part × era_payout
commission_payout          = commission × validator_total_payout
leftover                   = validator_total_payout - commission_payout
validator_own_payout       = (validator_own / total_exposure) × leftover
nominator_i_payout         = (nominator_i_stake / total_exposure) × leftover
```

### Reward Destinations

`make_payout()` fonksiyonu (satır 411–448) şu destinasyonları destekler:

| RewardDestination | Davranış |
|---|---|
| `Staked` | Balance stake'e eklenir (ledger.active++ , ledger.total++) |
| `Stash` | Balance serbest olarak stash'a eklenir |
| `Account(dest)` | Belirtilen hesaba mint edilir |
| `None` | Ödeme yapılmaz |
| `Controller` (deprecated) | Eski controller hesabına |

### Reward Points Sistemi

`reward_by_ids()` (satır 849–858):
```rust
fn reward_by_ids(validators_points: impl IntoIterator<Item = (T::AccountId, u32)>) {
    ErasRewardPoints::<T>::mutate(active_era.index, |era_rewards| {
        for (validator, points) in validators_points.into_iter() {
            *era_rewards.individual.entry(validator).or_default() += points;
            era_rewards.total += points;
        }
    });
}
```

`pallet_authorship::EventHandler` impl'ı (satır 1730–1736):
```rust
fn note_author(author: T::AccountId) {
    <Self as RewardsReporter<T::AccountId>>::reward_by_ids(vec![(author, 20)])
}
```

**Blok başına 20 puan** — validator blok üretince kazanır. Toplam puanlara göre era ödülü oransal bölüşülür.

---

## Slashing Mekanizması

### Temel Prensip

`slashing.rs` açılış yorumu:
> Nominatörler birden fazla validator'ı nominate edebilir ve herhangi biriyle slash yiyebilir. Stake era'dan era'ya yeniden kullanılır (N × E değil, sadece N). Slash olayları geç veya sıra dışı bildirilecek.
>
> Çözüm: Bir **slashing span** içindeki maksimum slash'ı uygula (toplam değil). Bu over-slash'a karşı korur.

### Slashing Formülü

`compute_slash()` (satır 241–307):

```rust
pub(crate) fn compute_slash<T: Config>(params: SlashParams<T>)
    -> Option<UnappliedSlash<T::AccountId, BalanceOf<T>>>
{
    // 1. Validator'ın kendi slash'ı
    let own_slash = params.slash * params.exposure.own;  // Perbill çarpımı

    // 2. Toplam sıfırsa sadece kick et
    if params.slash * params.exposure.total == Zero::zero() {
        kick_out_if_recent::<T>(params);
        return None;
    }

    // 3. Önceki era slash'ıyla karşılaştır (max prensibi)
    let prior_slash_p = ValidatorSlashInEra::<T>::get(&params.slash_era, params.stash)
        .map_or(Zero::zero(), |(prior_slash_proportion, _)| prior_slash_proportion);

    if params.slash.deconstruct() > prior_slash_p.deconstruct() {
        // Yeni max — storage'ı güncelle
        ValidatorSlashInEra::<T>::insert(&params.slash_era, params.stash, &(params.slash, own_slash));
    } else {
        // Eski slash daha büyük — işlem yok
        return None;
    }

    // 4. Slashing span'ı güncelle
    let mut spans = fetch_spans::<T>(params.stash, params.window_start, ...);
    let target_span = spans.compare_and_update_span_slash(params.slash_era, own_slash);
    if target_span == Some(spans.span_index()) {
        spans.end_span(params.now);  // yeni span başlat
    }

    // 5. Nominatörleri slash et
    let mut nominators_slashed = Vec::new();
    reward_payout += slash_nominators::<T>(params.clone(), prior_slash_p, &mut nominators_slashed);

    Some(UnappliedSlash {
        validator: params.stash.clone(),
        own: val_slashed,
        others: nominators_slashed,
        reporters: Vec::new(),
        payout: reward_payout,
    })
}
```

**Temel Formül**: `actual_slash = Perbill * stake`

### Slashing Span Sistemi

`SlashingSpans` struct (satır 92–104):
```rust
pub struct SlashingSpans {
    pub span_index: SpanIndex,
    pub last_start: EraIndex,
    pub last_nonzero_slash: EraIndex,
    pub prior: Vec<EraIndex>,  // eski span'ların start era'ları (offset olarak)
}
```

- Her stash'ın kendi span geçmişi var
- Slash gelince: o era hangi span'a düşüyor? O span'ın max slash'ını güncelle
- Mevcut aktif span'da slash varsa yeni span başlat (eski span kapandı)
- `prune()` ile eski span'lar temizlenir (window_start'tan eskiler)

### Slash Reward Formülü

`compare_and_update_span_slash()` (satır 452–494):
```rust
let reward = if span_record.slashed < slash {
    // Yeni max slash, fark kadar reward hesapla
    let difference = slash - span_record.slashed;
    span_record.slashed = slash;

    // reward = REWARD_F1 × (reward_proportion × slash - zaten_ödenen)
    REWARD_F1 * (self.reward_proportion * slash).saturating_sub(span_record.paid_out)
} else if span_record.slashed == slash {
    REWARD_F1 * (self.reward_proportion * slash).saturating_sub(span_record.paid_out)
} else {
    Zero::zero()
};
```

Sabitler:
- `REWARD_F1 = Perbill::from_percent(50)` — ilk bildirimin ödül oranı (f1 parametresi, paper'dan)
- `reward_proportion` = `SlashRewardFraction` storage değeri

### Nominatör Slash Formülü

`slash_nominators()` (satır 332–380):
```rust
for nominator in &params.exposure.others {
    let own_slash_prior = prior_slash_p * nominator.value;
    let own_slash_by_validator = params.slash * nominator.value;
    let own_slash_difference = own_slash_by_validator.saturating_sub(own_slash_prior);

    let mut era_slash = NominatorSlashInEra::<T>::get(&params.slash_era, stash)
        .unwrap_or_else(Zero::zero);
    era_slash += own_slash_difference;
    NominatorSlashInEra::<T>::insert(&params.slash_era, stash, &era_slash);
    // sonra: era_slash ile span max karşılaştırması
}
```

**Formül**:
```
nominator_era_slash_increase = (new_slash_perbill - prior_slash_perbill) × nominator.stake
nominators era_slash          = previous_era_slash + increase
(span max prensibi uygulanır)
```

### do_slash() — Gerçek Balance İşlemi

```rust
pub fn do_slash<T: Config>(
    stash: &T::AccountId,
    value: BalanceOf<T>,
    reward_payout: &mut BalanceOf<T>,
    slashed_imbalance: &mut NegativeImbalanceOf<T>,
    slash_era: EraIndex,
) {
    let mut ledger = Pallet::<T>::ledger(StakingAccount::Stash(stash.clone()))?;
    let value = ledger.slash(value, asset::existential_deposit::<T>(), slash_era);

    if !Pallet::<T>::is_virtual_staker(stash) {
        let (imbalance, missing) = asset::slash::<T>(stash, value);
        slashed_imbalance.subsume(imbalance);
        if !missing.is_zero() {
            *reward_payout = reward_payout.saturating_sub(missing);
        }
    }
    ledger.update();
}
```

Virtual staker'lar için balance slash atlanır — ilgili pallet (pool pallet) halleder.

### Slash Deferral

Offence raporu gelince:
- `SlashDeferDuration == 0` → anında uygula
- `SlashDeferDuration > 0` → `UnappliedSlashes[era + defer + 1]` storage'ına yaz
- `start_era()` çağrıldığında `apply_unapplied_slashes(active_era)` çağrılır

```rust
// apply zamanı
let slash_era = active_era - T::SlashDeferDuration::get();
slashing::apply_slash::<T>(slash, slash_era);
```

Governance `cancel_deferred_slash()` ile slash'ı iptal edebilir (sadece henüz uygulanmamışsa).

### StakingLedger::slash() — Proportional Slash

Ledger içindeki proportional slash mantığı (lib.rs satır 671–790):

```rust
let slashable_chunks_start = slash_era + T::BondingDuration::get();

// Eğer slashable_chunks_start'tan sonra unlock chunk varsa: proportional slash
let ratio = Perquintill::from_rational_with_rounding(
    slash_amount,
    affected_balance,  // active + slashable_unlock_chunks
    Rounding::Up,
);
// Her hedef için: slash_from_target = ratio × target
// Eğer proportional değilse: active'den full slash alınır
```

---

## Bonding/Unbonding

### bond() (mod.rs satır 1182–1214)

```rust
pub fn bond(origin, value: BalanceOf<T>, payee: RewardDestination<T::AccountId>) {
    let stash = ensure_signed(origin)?;
    ensure!(!T::Filter::contains(&stash), Restricted);       // pool staker block
    ensure!(!StakingLedger::is_bonded(Stash(stash)), AlreadyBonded);
    ensure!(!StakingLedger::is_bonded(Controller(stash)), AlreadyPaired);
    ensure!(value >= ED, InsufficientBond);

    let value = value.min(asset::free_to_stake::<T>(&stash));
    let ledger = StakingLedger::<T>::new(stash.clone(), value);
    ledger.bond(payee)?;  // Bonded, Payee, Ledger storage'larını set et
    Event::Bonded { stash, amount: value }
}
```

### bond_extra() (mod.rs satır 1232–1240)

```rust
pub fn bond_extra(origin, max_additional: BalanceOf<T>) {
    let stash = ensure_signed(origin)?;
    Self::do_bond_extra(&stash, max_additional)
}
```

`do_bond_extra()` (impls.rs satır 164–191):
```rust
let extra = additional.min(asset::free_to_stake::<T>(stash));
ledger.total += extra;
ledger.active += extra;
ensure!(ledger.active >= ED, InsufficientBond);
ledger.update()?;
T::VoterList::on_update(stash, Self::weight_of(stash));
```

### unbond() (mod.rs satır 1267–1287)

```rust
pub fn unbond(origin, value: BalanceOf<T>) {
    let controller = ensure_signed(origin)?;
    let ledger = Self::ledger(Controller(controller))?;

    // Tüm balance unbond ediliyorsa chill et
    if value >= ledger.total {
        Self::chill_stash(&ledger.stash);
    }

    Self::do_unbond(controller, value)?;
}
```

`do_unbond()` iç mantığı (impls.rs satır 1386–1469):
```rust
// Eğer unlocking chunk doluysa önce withdraw dene
if unlocking == T::MaxUnlockingChunks::get() as usize {
    Self::do_withdraw_unbonded(&controller, real_num_slashing_spans)?;
}

let mut value = value.min(ledger.active);
ledger.active -= value;

// Dust kontrolü
if ledger.active < ED {
    value += ledger.active;
    ledger.active = Zero::zero();
}

// Min bond kontrolü
ensure!(ledger.active >= min_active_bond, InsufficientBond);

// UnlockChunk ekle veya mevcut era chunk'una ekle
let era = CurrentEra + T::BondingDuration::get();
if let Some(chunk) = ledger.unlocking.last_mut().filter(|c| c.era == era) {
    chunk.value += value;  // aynı era'ya birleştir (FiFo chunk mantığı)
} else {
    ledger.unlocking.try_push(UnlockChunk { value, era })?;
}
```

### withdraw_unbonded() (mod.rs satır 1314–1322)

```rust
pub fn withdraw_unbonded(origin, num_slashing_spans: u32) {
    let controller = ensure_signed(origin)?;
    Self::do_withdraw_unbonded(&controller, num_slashing_spans)?;
}
```

`do_withdraw_unbonded()` (impls.rs satır 193–233):
```rust
let (stash, old_total) = (ledger.stash.clone(), ledger.total);
if let Some(current_era) = CurrentEra::<T>::get() {
    ledger = ledger.consolidate_unlocked(current_era);
    // consolidate_unlocked: chunk.era <= current_era ise çıkar, total'dan düş
}

if ledger.unlocking.is_empty() && (ledger.active < ED || ledger.active.is_zero()) {
    // Tamamen unbonded — stash'ı öldür
    Self::kill_stash(&ledger.stash, num_slashing_spans)?;
} else {
    ledger.update()?;
}

// Event: Withdrawn { stash, amount: old_total - new_total }
```

### rebond() (mod.rs satır 1748–1785)

```rust
pub fn rebond(origin, value: BalanceOf<T>) {
    let controller = ensure_signed(origin)?;
    let ledger = Self::ledger(Controller(controller))?;
    ensure!(!ledger.unlocking.is_empty(), NoUnlockChunk);

    let (ledger, rebonded_value) = ledger.rebond(value);
    ensure!(ledger.active >= ED, InsufficientBond);
    ledger.update()?;
    T::VoterList::on_update(&stash, Self::weight_of(&stash));
}
```

`StakingLedger::rebond()` (lib.rs satır 624–646):
```rust
fn rebond(mut self, value: BalanceOf<T>) -> (Self, BalanceOf<T>) {
    let mut unlocking_balance = BalanceOf::<T>::zero();
    while let Some(last) = self.unlocking.last_mut() {
        // Unlocking queue'dan sondan başlayarak al (LIFO rebond = en yeni chunk önce)
        if unlocking_balance + last.value <= value {
            unlocking_balance += last.value;
            self.active += last.value;
            self.unlocking.pop();
        } else {
            let diff = value - unlocking_balance;
            unlocking_balance += diff;
            self.active += diff;
            last.value -= diff;
        }
        if unlocking_balance >= value { break; }
    }
    (self, unlocking_balance)
}
```

**Not**: rebond LIFO (en yeni chunk önce), unbond FiFo (chunk ekleme). Withdraw FIFO (en eski çıkar).

### Bonding Döngüsü Özeti

```
bond(value)
  └─> ledger.active = ledger.total = value
      lock uygulanır (HoldReason::Staking)

bond_extra(additional)
  └─> ledger.active += min(additional, free_balance)
      ledger.total += same

unbond(value)
  └─> ledger.active -= value
      unlocking.push(UnlockChunk { value, era: current + bonding_duration })

rebond(value)
  └─> unlocking'den çıkar (LIFO), active'e ekle

withdraw_unbonded()
  └─> unlocking'de era <= current_era olanları çıkar
      total azalt, hold miktarını güncelle

// Tamamen unbonded ise:
kill_stash()
  └─> Bonded, Ledger, Payee sil
      hold release et
      Nominators/Validators'dan çıkar
      SlashingSpans metadata temizle
```

---

## Election Provider

### ElectionProvider Trait Entegrasyonu

`Config` içinde tanımlıdır (mod.rs satır 146–158):

```rust
type ElectionProvider: ElectionProvider<
    AccountId = Self::AccountId,
    BlockNumber = BlockNumberFor<Self>,
    DataProvider = Pallet<Self>,  // staking pallet data provider görevi yapar
>;
type GenesisElectionProvider: ElectionProvider<...>;
```

### ElectionDataProvider Implementasyonu

`pallet/impls.rs` satır 1496–1572 — pallet-staking, `ElectionDataProvider` trait'ini implement eder:

```rust
impl<T: Config> ElectionDataProvider for Pallet<T> {
    type AccountId = T::AccountId;
    type BlockNumber = BlockNumberFor<T>;
    type MaxVotesPerVoter = MaxNominationsOf<T>;

    fn desired_targets() -> Result<u32> {
        Ok(ValidatorCount::<T>::get())
    }

    fn electing_voters(bounds: DataProviderBounds, _page: PageIndex)
        -> Result<Vec<VoterOf<Self>>> {
        let voters = Self::get_npos_voters(bounds);
        Ok(voters)
    }

    fn electable_targets(bounds: DataProviderBounds, _page: PageIndex)
        -> Result<Vec<T::AccountId>> {
        let targets = Self::get_npos_targets(bounds);
        Ok(targets)
    }

    fn next_election_prediction(now: BlockNumberFor<T>) -> BlockNumberFor<T> {
        // era ilerlemesine göre tahmini sonraki election bloğu
    }
}
```

### get_npos_voters() — Voter Listesi (satır 889–999)

```rust
pub fn get_npos_voters(bounds: DataProviderBounds) -> Vec<VoterOf<Self>> {
    let weight_of = Self::weight_of_fn();  // total_issuance cache'le
    let mut sorted_voters = T::VoterList::iter();

    while all_voters.len() < final_predicted_len &&
        voters_seen < (NPOS_MAX_ITERATIONS_COEFFICIENT * final_predicted_len)
    {
        let voter = sorted_voters.next()?;
        let voter_weight = weight_of(&voter);

        if voter_weight.is_zero() { continue; }  // sıfır stake skip

        if let Some(nominations) = Nominators::<T>::get(&voter) {
            // Nominatör voter olarak ekle
            all_voters.push((voter, voter_weight, nominations.targets));
        } else if Validators::<T>::contains_key(&voter) {
            // Validator kendi kendini nominate eder
            all_voters.push((voter, voter_weight, vec![voter.clone()]));
        }
    }

    MinimumActiveStake::<T>::put(min_active_stake);
    all_voters
}
```

`NPOS_MAX_ITERATIONS_COEFFICIENT = 2` — maksimum N voter için 2×N iterasyon yapılır (geçersiz voter'lar için tolerans).

### Validator Seçim Süreci

1. `try_trigger_new_era()` çağrılır (impls.rs satır 641–694)
2. `T::ElectionProvider::elect(0)` çağrılır (tek sayfa, page=0)
3. Sonuç `BoundedSupports` olarak gelir (validator + support ağırlıkları)
4. `collect_exposures()` ile `Exposure` struct'larına dönüştürülür
5. `trigger_new_era()` → `store_stakers_info()` ile storage'a yazılır

### collect_exposures() — Exposure Yapısı (satır 744–778)

```rust
fn collect_exposures(supports: BoundedSupportsOf<T::ElectionProvider>)
    -> BoundedVec<(T::AccountId, Exposure<..>), MaxWinnersOf<T>>
{
    let total_issuance = asset::total_issuance::<T>();
    let to_currency = |e: ExtendedBalance| T::CurrencyToVote::to_currency(e, total_issuance);

    supports.into_iter().map(|(validator, support)| {
        let mut own: BalanceOf<T> = Zero::zero();
        let mut total: BalanceOf<T> = Zero::zero();
        let mut others = Vec::new();

        support.voters.into_iter()
            .map(|(nominator, weight)| (nominator, to_currency(weight)))
            .for_each(|(nominator, stake)| {
                if nominator == validator {
                    own += stake;       // validator'ın kendi stake'i
                } else {
                    others.push(IndividualExposure { who: nominator, value: stake });
                }
                total += stake;
            });

        (validator, Exposure { own, others, total })
    }).collect()
}
```

### VoterList ve TargetList

```rust
type VoterList: SortedListProvider<Self::AccountId, Score = VoteWeight>;
type TargetList: SortedListProvider<Self::AccountId, Score = BalanceOf<Self>>;
```

- `VoterList`: tipik olarak `pallet-bags-list` kullanılır (weighted bags ile sıralı liste)
- Nominatör veya validator eklenince/çıkarınca güncellenir
- Score = `slashable_balance_of_vote_weight()` = aktif stake / total issuance * u64

---

## Storage Items

Tam depolama envanteri (`pallet/mod.rs`):

### Temel Staker Durumu

| Storage | Tip | Açıklama |
|---|---|---|
| `Bonded<T>` | `Map<AccountId, AccountId>` | stash → controller eşlemesi |
| `Ledger<T>` | `Map<AccountId, StakingLedger<T>>` | controller → ledger |
| `Payee<T>` | `Map<AccountId, RewardDestination>` | stash → reward dest |
| `Validators<T>` | `CountedMap<AccountId, ValidatorPrefs>` | aktif validator'lar |
| `Nominators<T>` | `CountedMap<AccountId, Nominations<T>>` | aktif nominatörler |
| `VirtualStakers<T>` | `CountedMap<AccountId, ()>` | pool/dış staker'lar |

### Era Tracking

| Storage | Tip | Açıklama |
|---|---|---|
| `CurrentEra<T>` | `Option<EraIndex>` | planlanmış era |
| `ActiveEra<T>` | `Option<ActiveEraInfo>` | ödüllendirilen era (index + start ms) |
| `ErasStartSessionIndex<T>` | `Map<EraIndex, SessionIndex>` | era başlangıç session'ı |
| `BondedEras<T>` | `Vec<(EraIndex, SessionIndex)>` | bonding penceresi içindeki eralar |
| `CurrentPlannedSession<T>` | `SessionIndex` | son planlanan session |

### Exposure / Stake Bilgisi

| Storage | Tip | Açıklama |
|---|---|---|
| `ErasStakers<T>` | `DoubleMap<EraIndex, AccountId, Exposure>` | **deprecated** (v14) |
| `ErasStakersClipped<T>` | `DoubleMap<EraIndex, AccountId, Exposure>` | **deprecated** (v14) |
| `ErasStakersOverview<T>` | `DoubleMap<EraIndex, AccountId, PagedExposureMetadata>` | yeni (v14+) |
| `ErasStakersPaged<T>` | `NMap<(EraIndex, AccountId, Page), ExposurePage>` | sayfalı nominatör exposure |
| `ErasTotalStake<T>` | `Map<EraIndex, Balance>` | era toplam stake |

### Reward Bilgisi

| Storage | Tip | Açıklama |
|---|---|---|
| `ErasValidatorReward<T>` | `Map<EraIndex, Balance>` | era toplam validator ödemesi |
| `ErasRewardPoints<T>` | `Map<EraIndex, EraRewardPoints>` | era puan dağılımı |
| `ErasValidatorPrefs<T>` | `DoubleMap<EraIndex, AccountId, ValidatorPrefs>` | snapshot prefs |
| `ClaimedRewards<T>` | `DoubleMap<EraIndex, AccountId, Vec<Page>>` | hangi sayfalar ödendi |

### Slashing

| Storage | Tip | Açıklama |
|---|---|---|
| `ValidatorSlashInEra<T>` | `DoubleMap<EraIndex, AccountId, (Perbill, Balance)>` | era max slash |
| `NominatorSlashInEra<T>` | `DoubleMap<EraIndex, AccountId, Balance>` | nominatör era slash |
| `SlashingSpans<T>` | `Map<AccountId, SlashingSpans>` | stash başına span geçmişi |
| `SpanSlash<T>` | `Map<(AccountId, SpanIndex), SpanRecord>` | span içi slash kayıtları |
| `UnappliedSlashes<T>` | `Map<EraIndex, Vec<UnappliedSlash>>` | deferred slash kuyruğu |

### Yönetim/Config

| Storage | Tip | Açıklama |
|---|---|---|
| `ValidatorCount<T>` | `u32` | hedef validator sayısı |
| `MinimumValidatorCount<T>` | `u32` | minimum geçerli validator |
| `Invulnerables<T>` | `Vec<AccountId>` | slash edilemeyen validator'lar |
| `ForceEra<T>` | `Forcing` | era zorlama modu |
| `MinNominatorBond<T>` | `Balance` | min nominatör bond |
| `MinValidatorBond<T>` | `Balance` | min validator bond |
| `MaxValidatorsCount<T>` | `Option<u32>` | max validator cap |
| `MaxNominatorsCount<T>` | `Option<u32>` | max nominatör cap |
| `MinimumActiveStake<T>` | `Balance` | son election'ın min aktif stake'i |
| `MinCommission<T>` | `Perbill` | min komisyon oranı |
| `SlashRewardFraction<T>` | `Perbill` | slash ödül fraksiyonu |
| `CanceledSlashPayout<T>` | `Balance` | iptal edilen slash ödemeleri |
| `ChillThreshold<T>` | `Option<Percent>` | chill_other eşiği |
| `MaxStakedRewards<T>` | `Option<Percent>` | era payout cap |

---

## Kritik Sabitler

### Test Default Konfigürasyonundan (mod.rs satır 355–387)

```rust
// SessionsPerEra
pub const SessionsPerEra: SessionIndex = 3;     // test default

// BondingDuration
pub const BondingDuration: EraIndex = 3;         // test default

// Config defaults:
type HistoryDepth = ConstU32<84>;                // 84 era geçmiş
type MaxExposurePageSize = ConstU32<64>;          // sayfa başına max 64 nominatör
type MaxUnlockingChunks = ConstU32<32>;           // max 32 unlock chunk
type MaxValidatorSet = ConstU32<100>;             // max 100 validator
type MaxControllersInDeprecationBatch = ConstU32<100>;
type NominationsQuota = crate::FixedNominationsQuota<16>;  // max 16 nomination
```

### Slashing Sabitleri (slashing.rs satır 72)

```rust
const REWARD_F1: Perbill = Perbill::from_percent(50);  // slash ödülünün %50'si ilk bildirimine
```

### Diğer Sabitler (pallet/mod.rs satır 62)

```rust
pub(crate) const SPECULATIVE_NUM_SPANS: u32 = 32;
// unbond weight tahmini için spekülatif span sayısı
```

### STAKING_ID (lib.rs satır 341)

```rust
pub(crate) const STAKING_ID: LockIdentifier = *b"staking ";
// eski lock sisteminde kullanılan 8 byte identifier
// yeni sistemde HoldReason::Staking kullanılıyor
```

### inflation.rs Sabiti (satır 42)

```rust
const MILLISECONDS_PER_YEAR: u64 = 1000 * 3600 * 24 * 36525 / 100;
// = 31_557_600_000 ms = Julian yıl (365.25 gün)
```

### reward-fn Taylor Serisi

```rust
const LN2: u64 = 0_693_147_180_559_945_309;  // ln(2) × 10^18
// Taylor serisi 300 iterasyona kadar çalışır
```

### NPOS_MAX_ITERATIONS_COEFFICIENT (impls.rs satır 73)

```rust
const NPOS_MAX_ITERATIONS_COEFFICIENT: u32 = 2;
// N voter için 2N voter iterate edilir (geçersizler için tolerans)
```

---

## Events ve Errors

### Events (mod.rs satır 824–882)

```rust
pub enum Event<T: Config> {
    EraPaid { era_index, validator_payout, remainder },
    // Era bitti, toplam ödemeler loglandı

    Rewarded { stash, dest, amount },
    // Bir staker ödüllendirildi

    Slashed { staker, amount },
    // Slash gerçekleşti

    SlashReported { validator, fraction, slash_era },
    // Slash raporu alındı

    OldSlashingReportDiscarded { session_index },
    // Eski/geçersiz slash raporu atıldı

    StakersElected,
    // Yeni validator seti seçildi

    Bonded { stash, amount },
    // Stake bond edildi

    Unbonded { stash, amount },
    // Stake unbond başlatıldı (hâlâ kilitli)

    Withdrawn { stash, amount },
    // Unbonding tamamlandı, para serbest

    Kicked { nominator, stash },
    // Nominatör validator tarafından çıkarıldı

    StakingElectionFailed,
    // Election başarısız (yeterli aday yok)

    Chilled { stash },
    // Staker rolü bıraktı

    PayoutStarted { era_index, validator_stash, page, next },
    // Ödeme başladı, next: sonraki sayfa index

    ValidatorPrefsSet { stash, prefs },
    // Validator tercihleri güncellendi

    SnapshotVotersSizeExceeded { size },
    SnapshotTargetsSizeExceeded { size },
    // Election snapshot boyutu aşıldı

    ForceEra { mode },
    // Era zorlama modu değişti

    ControllerBatchDeprecated { failures },
    // Controller batch migration raporu

    CurrencyMigrated { stash, force_withdraw },
    // Staking balance lock → hold migration
}
```

### Errors (mod.rs satır 884–960)

```rust
pub enum Error<T> {
    NotController,              // hesap controller değil
    NotStash,                   // hesap stash değil
    AlreadyBonded,              // zaten bond edilmiş
    AlreadyPaired,              // controller başka stash'e bağlı
    EmptyTargets,               // nominasyon hedefleri boş
    DuplicateIndex,             // tekrar slash index
    InvalidSlashIndex,          // geçersiz slash index
    InsufficientBond,           // minimum bond altı
    NoMoreChunks,               // MaxUnlockingChunks dolu
    NoUnlockChunk,              // rebond için unlock chunk yok
    FundedTarget,               // reap_stash için hâlâ fon var
    InvalidEraToReward,         // geçersiz era (history_depth dışı)
    InvalidNumberOfNominations, // nomination sayısı limiti
    NotSortedAndUnique,         // cancel_deferred_slash için
    AlreadyClaimed,             // ödül zaten alınmış
    InvalidPage,                // geçersiz sayfa indexi
    IncorrectHistoryDepth,      // tarih derinliği uyumsuzluğu
    IncorrectSlashingSpans,     // span sayısı uyumsuz
    BadState,                   // internal inconsistency
    TooManyTargets,             // nomination quota aşımı
    BadTarget,                  // blocked veya geçersiz validator
    CannotChillOther,           // chill_other koşulları sağlanmıyor
    TooManyNominators,          // MaxNominatorsCount aşıldı
    TooManyValidators,          // MaxValidatorsCount aşıldı
    CommissionTooLow,           // MinCommission altı
    BoundNotMet,                // genel bound ihlali
    ControllerDeprecated,       // eski controller kullanımı
    CannotRestoreLedger,        // restore_ledger koşulları yok
    RewardDestinationRestricted,// kısıtlı reward destination
    NotEnoughFunds,             // hold için yeterli fon yok
    VirtualStakerNotAllowed,    // virtual staker işlemi yasak
    CannotReapStash,            // başka pallet tüketici referansı var
    AlreadyMigrated,            // currency migration zaten yapılmış
    Restricted,                 // Filter trait tarafından engellendi
}
```

---

## Paged Exposure Sistemi (v14+)

V14 migration ile birlikte nominatör exposure artık sayfalıdır. Bu kritik bir değişikliktir.

### Yeni Yapı

```rust
// Metadata (genel bilgi)
pub struct PagedExposureMetadata<Balance> {
    pub own: Balance,        // validator'ın kendi stake'i
    pub total: Balance,      // toplam (own + tüm nominatörler)
    pub nominator_count: u32,
    pub page_count: Page,    // kaç sayfa var
}

// Tek sayfa
pub struct ExposurePage<AccountId, Balance> {
    pub page_total: Balance,  // bu sayfadaki nominatörlerin toplam stake'i
    pub others: Vec<IndividualExposure<AccountId, Balance>>,
}

// Storage
ErasStakersOverview<T>:  (EraIndex, AccountId) -> PagedExposureMetadata
ErasStakersPaged<T>:     (EraIndex, AccountId, Page) -> ExposurePage
```

### Sayfa Boyutu Hesabı (lib.rs satır 1333–1360)

```rust
pub fn set_exposure(era: EraIndex, validator: &T::AccountId, exposure: Exposure<...>) {
    let page_size = T::MaxExposurePageSize::get().defensive_max(1);

    // Nominatörleri sayfalara böl
    let (others, own, total, nominator_count) = ...;

    let num_exposures = others.chunks(page_size as usize);
    for (page_index, page_exposures) in num_exposures.enumerate() {
        let page_total = page_exposures.iter().map(|e| e.value).sum();
        ErasStakersPaged::<T>::insert(
            (era, validator, page_index as Page),
            ExposurePage { page_total, others: page_exposures.to_vec() },
        );
    }

    ErasStakersOverview::<T>::insert(era, validator, PagedExposureMetadata {
        own, total,
        nominator_count: nominator_count as u32,
        page_count: num_exposures.count() as Page,
    });
}
```

### Ödeme Süreci (Paged)

Her sayfa için `payout_stakers_by_page()` ayrı çağrılır. `ClaimedRewards` storage'ı hangi sayfaların ödülünün alındığını tutar.

```rust
// Sıradaki claimable sayfayı bul
pub fn get_next_claimable_page(era, validator, ledger) -> Option<Page> {
    let page_count = ErasStakersOverview::get(era, validator)?.page_count;
    let claimed = ClaimedRewards::get(era, validator);
    // Legacy fallback: eski ledger.claimed_rewards kontrolü
    (0..page_count).find(|page| !claimed.contains(page))
}
```

---

## Diğer Önemli Mekanizmalar

### chill_other() Mekanizması (mod.rs satır 1955–2017)

Başkasının staker'ını chill etme koşulları:
1. Nominatör artık decode edilemiyorsa (quota değişiminden) → anında chill
2. Yoksa:
   - `ChillThreshold` ayarlı olmalı
   - `MaxNominatorsCount` veya `MaxValidatorsCount` ayarlı olmalı
   - `threshold × max_count < current_count` (dolu yeterli)
   - `ledger.active < MinNominatorBond` veya `MinValidatorBond` (yetersiz bond)

### reap_stash() Mekanizması (mod.rs satır 1806–1831)

Toz haline gelen stash'ı temizle:
- `total_balance < ED` VEYA `ledger.total < ED` → reapable
- Virtual staker'lar için yasak
- Başarılı → işlem ücreti yok (`Pays::No`)

### Validator Komisyon Sistemi

```rust
pub struct ValidatorPrefs {
    pub commission: Perbill,  // 0 ile 1 arası
    pub blocked: bool,        // yeni nominasyon kabul etmiyor mu
}
```

`MinCommission` governance tarafından ayarlanabilir. `force_apply_min_commission()` ile herkes bir validator'ın komisyonunu minimuma yükseltebilir.

### on_offence Handler (impls.rs satır 1249–1383)

`OnOffenceHandler` trait implementation:

```rust
pub fn on_offence(offenders, slash_fractions, slash_session) {
    let window_start = active_era - T::BondingDuration::get();
    let slash_era = /* session -> era dönüşümü */;

    for (details, slash_fraction) in offenders.zip(slash_fractions) {
        // Invulnerable validator skip
        if invulnerables.contains(stash) { continue; }

        // Aktif era offence'ı: validator'ı session'a bildir (disable et)
        if slash_era == active_era {
            T::SessionInterface::report_offence(stash, OffenceSeverity(*slash_fraction));
        }

        // Slash hesapla (defer edilebilir)
        let unapplied = slashing::compute_slash::<T>(SlashParams { ... });

        if slash_defer_duration == 0 {
            slashing::apply_slash::<T>(unapplied, slash_era);
        } else {
            // defer: era + defer_duration + 1'de uygula
            UnappliedSlashes::<T>::mutate(
                slash_era + slash_defer_duration + 1,
                |for_later| for_later.push(unapplied),
            );
        }
    }
}
```

### Asset Modülü

`src/asset.rs` — tüm currency işlemleri buradan yapılır (eski `LockableCurrency` değil):

```rust
pub fn staked<T: Config>(who: &T::AccountId) -> BalanceOf<T> {
    T::Currency::balance_on_hold(&HoldReason::Staking.into(), who)
}

pub fn update_stake<T: Config>(who: &T::AccountId, amount: BalanceOf<T>) -> DispatchResult {
    T::Currency::set_on_hold(&HoldReason::Staking.into(), who, amount)
}

pub fn slash<T: Config>(who: &T::AccountId, value: BalanceOf<T>)
    -> (NegativeImbalanceOf<T>, BalanceOf<T>) {
    T::Currency::slash(&HoldReason::Staking.into(), who, value)
}

pub fn mint_into_existing<T: Config>(who: &T::AccountId, value: BalanceOf<T>)
    -> Option<PositiveImbalanceOf<T>> {
    T::Currency::deposit(who, value, Precision::Exact).ok()
}
```

**Önemli tasarım kararı**: Staking hold, `Precision::Exact` ile deposit edilir — hesap varsa ED altına bile düşebilir. Yeni hesap için `BestEffort` kullanılır.

---

## Nova Shield Uygulama Notu

### 1. Staking Mimarisi

Nova Shield, Substrate pallet-staking'in tasarımını referans almalı fakat şu iyileştirmeleri içermeli:

**A. Controller Hesabını Kaldır**
Substrate controller kavramını yıllarca `deprecated` olarak sürükledi. Nova Shield baştan stash == controller yap. Ledger:
```rust
pub struct NovaStkLedger<T: Config> {
    pub stash: T::AccountId,   // stash == controller == hesap
    pub total: BalanceOf<T>,
    pub active: BalanceOf<T>,
    pub unlocking: BoundedVec<UnlockChunk<BalanceOf<T>>, T::MaxUnlockingChunks>,
}
// controller alanı yok — her zaman stash
```

**B. Hold Sistemi**
Substrate v14'te `LockableCurrency` → `FunHoldMutate` migrasyonu yapıldı. Nova Shield baştan `Hold` kullan:
```rust
type Currency: FunHoldMutate<AccountId, Reason = RuntimeHoldReason>
             + FunMutate<AccountId>;
```

### 2. Inflation Eğrisi Parametreleri

Substrate inflation formülü doğrudan alınabilir. Önerilen Nova Shield parametreleri:

```
min_inflation:  2.5%   (I_0 = 0.025)
max_inflation:  10%    (I_max = 0.10)
ideal_stake:    50%    (x_ideal = 0.50)
falloff:        5%     (d = 0.05)
```

Bu parametrelerle 50% stake oranında maksimum inflation, altında lineer artış, üstünde eksponansiyel düşüş sağlanır.

`compute_total_payout()` kütüphanesini doğrudan kullanabilirsin:
```rust
use pallet_staking_reward_fn::compute_inflation;
use sp_runtime::curve::PiecewiseLinear;
```

### 3. Slashing Stratejisi

Substrate'in slashing tasarımı oldukça iyi:
- **Max-in-era prensibi**: Bir era'da birden fazla slash raporu gelirse yalnızca maksimum uygulanır. Over-slash'a karşı korur.
- **Slashing span**: Her stash için ayrı span geçmişi. Slash sonrası yeni span başlar.
- **Proportional slash**: Unlocking chunk'lar da proportional olarak slash edilir.

Nova Shield için öneri — slashing şiddet skalası:

| Offence | Slash Oranı |
|---|---|
| Equivocation (oy çifte imzalama) | %1 (Substrate default) |
| BFT çifte imzalama | %30 |
| Downtime (sık offline) | %0.01 (küçük) |
| Uzun süreli offline | kademeli artış |

**Defer duration**: 7 era önerilir (governance müdahalesi için zaman). `SlashDeferDuration = 7` veya 0 (hemen uygula, governance sonraya bırakma).

### 4. Paged Exposure

Substrate v14'te paged exposure eklendi. Nova Shield baştan paged başlatmalı:
- `MaxExposurePageSize = 256` önerisi (Substrate'in 64'ten büyük)
- Her validator için bir snapshot alınır, sayfalara bölünür
- `payout_stakers_by_page()` her sayfa için ayrı çağrılır

### 5. NominationsQuota

Substrate'in `NominationsQuota` trait'i kullanışlı:
```rust
pub trait NominationsQuota<Balance> {
    type MaxNominations: Get<u32>;
    fn curve(balance: Balance) -> u32 { ... }  // stake'e göre dinamik quota
    fn get_quota(balance: Balance) -> u32 {
        Self::curve(balance).clamp(1, Self::MaxNominations::get())
    }
}
```

Nova Shield önerisi: Sabit 16 yerine `StakeBasedNominationsQuota` implement et:
- Az stake → 4 nomination
- Orta stake → 8 nomination
- Yüksek stake → 16 nomination
Bu, küçük nominatörlerin spam nomination yapmasını engeller.

### 6. VirtualStakers / Pool Entegrasyonu

Eğer Nova Shield'da staking pool (havuz) sistemi olacaksa, `VirtualStakers` mekanizması kritik:
- Pool pallet kendi hold mekanizmasını yönetir
- Staking pallet sadece logic (election, reward, slash) yapar
- `is_virtual_staker()` kontrolleri her işlemde gerekli

### 7. Election Provider Seçimi

Substrate'de kullanılabilir election provider'lar:
1. **pallet-election-provider-multi-phase** (SEQ-Phragmen) — production
2. **pallet-fast-unstake** — ek yardımcı
3. Basit **BagsList** tabanlı çözüm

Nova Shield için öneri: İlk fazda sequential Phragmén (TEE destekli), daha sonra onchain phragmen+.

### 8. Era Uzunluğu

Polkadot üretimde:
- Session = 10 dakika (600 blok × 6s)
- Era = 6 session = 1 saat (test) veya 4 saat (prod)

Nova Shield için önerilen:
- Session = 6 dakika (72 blok × 5s)
- SessionsPerEra = 10 → Era = 60 dakika
- BondingDuration = 14 era = 14 saat (veya 28 era = 1 gün)

### 9. Reward Destination Kısıtlaması

Substrate `Restricted` error ve `T::Filter` ekleyerek pool içindeki hesapların direkt staking yapmasını engeller. Nova Shield'da benzer bir mekanizma gerekir:
```rust
type Filter: Contains<Self::AccountId>;
```

### 10. Storage Migration Planı

Substrate'in 16 migration versiyonu geçmişi, başlangıçtan doğru tasarım yapmayı öğretiyor:
- V1–V12: controller deprecation, lock → hold
- V13: paged exposure
- V14: PagedExposureMetadata ayrı storage
- V15–V16: VirtualStakers, currency migration

**Önerim**: Nova Shield baştan paged exposure ve hold sistemi ile başlasın. Gereksiz migration yükü oluşturma.

---

## Kaynak Kod Referansları

### Temel Dosyalar

| Dosya | İçerik |
|---|---|
| `/substrate/frame/staking/src/lib.rs` | Tüm tip tanımları, Exposure, Ledger, Nominations vb |
| `/substrate/frame/staking/src/pallet/mod.rs` | Config trait, Storage items, Events, Errors, Extrinsics |
| `/substrate/frame/staking/src/pallet/impls.rs` | Tüm impl'lar: payout, election, session manager |
| `/substrate/frame/staking/src/slashing.rs` | SlashingSpan, compute_slash, apply_slash |
| `/substrate/frame/staking/src/ledger.rs` | StakingLedger CRUD ops |
| `/substrate/frame/staking/src/inflation.rs` | compute_total_payout |
| `/substrate/frame/staking/src/asset.rs` | Currency abstraction |
| `/substrate/frame/staking/reward-fn/src/lib.rs` | compute_inflation (Taylor serisi) |
| `/substrate/frame/staking/reward-curve/src/lib.rs` | PiecewiseLinear curve proc macro |

### Kritik Satır Referansları

| Fonksiyon/Sabit | Dosya:Satır |
|---|---|
| `REWARD_F1 = 50%` | slashing.rs:72 |
| `compute_slash()` | slashing.rs:241-307 |
| `slash_nominators()` | slashing.rs:332-380 |
| `compare_and_update_span_slash()` | slashing.rs:452-494 |
| `do_slash()` | slashing.rs:554-590 |
| `apply_slash()` | slashing.rs:593-619 |
| `pay_reporters()` | slashing.rs:622-651 |
| `compute_total_payout()` | inflation.rs:32-50 |
| `MILLISECONDS_PER_YEAR` | inflation.rs:42 |
| `do_payout_stakers_by_page()` | impls.rs:253-398 |
| `end_era()` / payout hesabı | impls.rs:571-601 |
| `on_offence()` | impls.rs:1249-1383 |
| `get_npos_voters()` | impls.rs:889-999 |
| `collect_exposures()` | impls.rs:744-778 |
| `StakingLedger::slash()` proportional | lib.rs:671-790 |
| `StakingLedger::rebond()` | lib.rs:624-646 |
| `SPECULATIVE_NUM_SPANS = 32` | pallet/mod.rs:62 |
| `HistoryDepth = 84` (test) | pallet/mod.rs:369 |
| `MaxExposurePageSize = 64` (test) | pallet/mod.rs:378 |
| `MaxUnlockingChunks = 32` (test) | pallet/mod.rs:379 |
| `SessionsPerEra = 3` (test) | pallet/mod.rs:356 |
| `BondingDuration = 3` (test) | pallet/mod.rs:357 |
| `NPOS_MAX_ITERATIONS_COEFFICIENT = 2` | impls.rs:73 |
| `note_author` → 20 puan | impls.rs:1735 |
| `LN2 = 0_693_147_180_559_945_309` | reward-fn/src/lib.rs:76 |

---

## Özet ve Kritik Bulgular

1. **Slashing Max Prensibi**: Bir era'da birden fazla slash gelse bile sadece maksimum uygulanır. `ValidatorSlashInEra` Perbill karşılaştırmasıyla çalışır — bal değil oran karşılaştırması yapılır (rounding güvenliği için).

2. **Proportional Slash**: Slash zamanında `bonding_duration` öteye kadar olan unlock chunk'lar da proportional slashlanır. Sadece `active` değil.

3. **Paged Exposure + Commission Bölüşümü**: Komisyon her sayfada `page_stake / total_stake` oranında ödenir. Büyük validator'lar için çok sayfalı ödeme gerekir.

4. **Reward Points = Blok Başına 20 Puan**: `pallet_authorship::EventHandler` integration ile validator blok ürettikçe puan kazanır. Dönem sonunda puana göre oransal bölüşüm.

5. **Deferred Slash**: `SlashDeferDuration > 0` ise slash `apply_unapplied_slashes()` ile era başında uygulanır. Governance bu pencerede cancel edebilir.

6. **Virtual Staker Exception**: Pool staker'lar için ledger lock atlanır, slash handling pool pallet'a devredilir.

7. **Inflation Formülü**: `portion × inflation(staked_ratio) × total_issuance` — era süresi ms cinsinden Julian yıla oranlanır.

8. **Controller Deprecation**: v16'ya kadar süren controller deprecation süreci tamamlandı. Artık stash == controller. Nova Shield baştan bu şekilde başlamalı.

9. **Hold vs Lock Migration**: v14+ sistemde staking artık `FunHoldMutate` kullanıyor (`LockableCurrency` değil). `STAKING_ID = *b"staking "` artık sadece legacy migration için.

10. **HistoryDepth = 84 default**: Bu, yaklaşık 84 era geçmişi anlamına gelir (~84 gün production'da). Payout bunun dışında kalırsa kaybolur.

---

## StakingInterface Trait — Dış Pallet Entegrasyonu

`impls.rs` satır 1971–2159 — `StakingInterface` tam implementasyonu:

```rust
impl<T: Config> StakingInterface for Pallet<T> {
    type AccountId = T::AccountId;
    type Balance = BalanceOf<T>;
    type CurrencyToVote = T::CurrencyToVote;

    fn minimum_nominator_bond() -> Self::Balance;
    fn minimum_validator_bond() -> Self::Balance;
    fn stash_by_ctrl(controller) -> Result<AccountId>;
    fn bonding_duration() -> EraIndex;
    fn current_era() -> EraIndex;
    fn stake(who) -> Result<Stake<Balance>>;  // { total, active }
    fn bond_extra(who, extra) -> DispatchResult;
    fn unbond(who, value) -> DispatchResult;
    fn set_payee(stash, reward_acc) -> DispatchResult;
    fn chill(who) -> DispatchResult;
    fn withdraw_unbonded(who, num_slashing_spans) -> Result<bool>;
    fn bond(who, value, payee) -> DispatchResult;
    fn nominate(who, targets) -> DispatchResult;
    fn desired_validator_count() -> u32;
    fn election_ongoing() -> bool;
    fn force_unstake(who) -> DispatchResult;
    fn is_exposed_in_era(who, era) -> bool;
    fn status(who) -> Result<StakerStatus>;
    fn is_virtual_staker(who) -> bool;
    fn slash_reward_fraction() -> Perbill;
}
```

Bu trait pool pallet ve diger runtime palletler tarafindan kullanilir. Nova Shield'da benzer bir trait soyutlamasi gerekli olacak.

### StakingUnchecked — Ileri Duzey Islemler

```rust
impl<T: Config> sp_staking::StakingUnchecked for Pallet<T> {
    fn migrate_to_virtual_staker(who: &AccountId) -> DispatchResult {
        asset::kill_stake::<T>(who)?;  // hold kaldir
        VirtualStakers::<T>::insert(who, ());  // virtual olarak isaretl
        Ok(())
    }

    fn virtual_bond(keyless_who, value, payee) -> DispatchResult {
        // keyless_who: pool hesabi (imzalayamaz)
        // payee: odullerin gidecegi hesap (keyless_who != payee zorunlu)
        VirtualStakers::<T>::insert(keyless_who, ());
        let ledger = StakingLedger::new(keyless_who, value);
        ledger.bond(RewardDestination::Account(payee))?;
    }
}
```

### is_virtual_staker() Kontrolu

```rust
fn is_virtual_staker(who: &T::AccountId) -> bool {
    frame_system::Pallet::<T>::account_nonce(who).is_zero()
        && VirtualStakers::<T>::contains_key(who)
}
```

**Kritik**: Nonce sifir VE VirtualStakers'ta kayitli. Iki kosul birden aranir. Eger hesap herhangi bir extrinsic imzaladiysa (nonce > 0) virtual staker olamaz.

---

## Election Size Tracker

`src/election_size_tracker.rs` — alloc gerektirmeden election snapshot boyutunu hesaplama:

```rust
pub struct StaticTracker<DataProvider> {
    pub size: usize,    // toplam tahmini byte sayisi
    pub counter: usize, // voter sayisi
}

impl StaticTracker {
    pub fn try_register_voter(&mut self, voter: &VoterOf<DataProvider>, bounds) -> Result<(), ()> {
        let voter_hint = Self::voter_size_hint(voter);
        let tracker_size_after = Self::final_byte_size_of(
            self.counter + 1, self.size + voter_hint
        );
        if bounds.size_exhausted(SizeBound(tracker_size_after as u32)) {
            Err(())  // limit asildi, snapshot artik voter alamaz
        } else {
            self.size = tracker_size_after;
            self.counter += 1;
            Ok(())
        }
    }

    fn voter_size_hint(voter: &VoterOf<DataProvider>) -> usize {
        let (voter_account, vote_weight, targets) = voter;
        // AccountId hint + VoteWeight hint + (num_votes x AccountId hint)
        voter_account.size_hint()
            .saturating_add(vote_weight.size_hint())
            .saturating_add(voter_account.size_hint() * targets.len())
    }

    fn final_byte_size_of(num_voters: usize, size: usize) -> usize {
        Self::length_prefix(num_voters).saturating_add(size)
        // SCALE compact length prefix dahil
    }
}
```

`SnapshotVotersSizeExceeded` event'ini tetikler. Election snapshot boyutu asilinca yeni voter eklenmez — snapshot'ta yalnizca en onemli (en yuksek stake'li) voter'lar kalir. `NPOS_MAX_ITERATIONS_COEFFICIENT = 2` ile N voter icin 2N iterasyon yapilir.

---

## UseNominatorsAndValidatorsMap — Basit Voter List

Production'da `pallet-bags-list` kullanilir. Test/basit konfigurasyon icin `UseNominatorsAndValidatorsMap`:

```rust
pub struct UseNominatorsAndValidatorsMap<T>;

impl SortedListProvider<T::AccountId> for UseNominatorsAndValidatorsMap<T> {
    fn iter() -> Box<dyn Iterator<Item = T::AccountId>> {
        Box::new(
            Validators::<T>::iter().map(|(v, _)| v)
                .chain(Nominators::<T>::iter().map(|(n, _)| n))
        )
    }
    fn get_score(id) -> VoteWeight { Pallet::<T>::weight_of(id) }
    // insert/update/remove: noop (dogrudan storage takip eder)
}
```

`pallet-bags-list` agirliga gore sirali bucket sistemi kullanir; buyuk stake'ler one gecer. Election snapshot icin daha adil secim saglar.

---

## Migrations — Tarihsel Degisiklikler

`src/migrations.rs` — 439 satir, v1'den v16'ya kadar migration gecmisi.

### V7: RewardDestination::Controller Kaldirma
Payee storage'daki tum `Controller` variantlar `Account(ctrl)` olarak guncellenir.

### V13: Paged Exposure
`ErasStakers` (tam liste) => `ErasStakersOverview` + `ErasStakersPaged`. Buyuk exposure'lar bolundu, sayfalara ayrildi. `ErasStakersClipped` deprecated edildi.

### V14: Currency Hold Migration
- `LockableCurrency::lock()` => `FunHoldMutate::set_on_hold()`
- Her stash icin lock kaldir, hold ekle
- Balance yetersizse `force_withdraw` ile ledger kucultulur
- `CurrencyMigrated { stash, force_withdraw }` event emit edilir

### V15-V16: VirtualStakers ve Son Migration
`consumer count` inconsistency duzeltmesi. `OldCurrency` lock => `Currency` hold tam gecis.

---

## EraInfo Helper — Paged Exposure Facade

`lib.rs` satir 1145–1360 — `EraInfo<T>` struct tum exposure islemlerini soyutlar:

```rust
pub struct EraInfo<T>(core::marker::PhantomData<T>);

impl<T: Config> EraInfo<T> {
    // Legacy + paged birlikte okuma
    pub fn get_paged_exposure(era, validator, page) -> Option<PagedExposure<..>> {
        if let Some(overview) = ErasStakersOverview::<T>::get(era, validator) {
            let exposure_page = ErasStakersPaged::<T>::get((era, validator, page))?;
            Some(PagedExposure { exposure_metadata: overview, exposure_page })
        } else {
            // Fallback: legacy ErasStakersClipped
            let clipped = ErasStakersClipped::<T>::get(era, validator);
            Some(PagedExposure::from_clipped(clipped))
        }
    }

    // Tam exposure (legacy + paged birlesimi)
    pub fn get_full_exposure(era, validator) -> Exposure<AccountId, Balance> {
        if let Some(overview) = ErasStakersOverview::<T>::get(era, validator) {
            let mut others = vec![];
            for page in 0..overview.page_count {
                if let Some(page_data) = ErasStakersPaged::<T>::get((era, validator, page)) {
                    others.extend(page_data.others);
                }
            }
            Exposure { total: overview.total, own: overview.own, others }
        } else {
            ErasStakers::<T>::get(era, validator)
        }
    }

    // Validator komisyonunu bul
    pub fn get_validator_commission(era, validator) -> Perbill {
        ErasValidatorPrefs::<T>::get(&era, validator_stash).commission
    }

    // Sayfa sayisini dondur
    pub fn get_page_count(era, validator) -> Page {
        ErasStakersOverview::<T>::get(era, validator)
            .map_or(
                if ErasStakersClipped::<T>::get(era, validator).others.is_empty() { 0 } else { 1 },
                |overview| overview.page_count,
            )
    }
}
```

---

## Guvenlik Analizi: Kritik Riskler

### 1. Slash Rounding Guvenligi

```rust
// Perbill karsilastirmasi — balance degil!
if params.slash.deconstruct() > prior_slash_p.deconstruct() { ... }
```

Neden? `slash * exposure.own` degerini karsilastirma yapilsaydi, kucuk exposure'larda rounding nedeniyle yanlis kararlar verilebilirdi. Perbill (parts per billion) precision yeterli.

### 2. Overslash Korumasi

```rust
let (imbalance, missing) = asset::slash::<T>(stash, value);
if !missing.is_zero() {
    *reward_payout = reward_payout.saturating_sub(missing);
}
```

Slash edilecek miktardan fazlasi yoksa, fazla eksiklik reporter odulunden dusulur.

### 3. Dust Attack Korumasi

```rust
// unbond'da dust kontrolu
if ledger.active < ED {
    value += ledger.active;  // dust'i da unbond'a ekle
    ledger.active = Zero::zero();
}
```

`slash_out_of` icinde de ayni mantik: hedef ED alti duserse kesin sifirla.

### 4. Slashing Span Limitasyon

Slashing span'larinin sayisi teorik olarak sinirsiz buyuyebilir. `prune()` ile eski span'lar temizlenir ama `withdraw_unbonded_kill()` icin `num_slashing_spans` kullanici tarafindan saglanir. Yanlis deger => `IncorrectSlashingSpans` hatasi.

**Nova Shield onerisi**: Slashing span sayisini sinirla (`MaxSlashingSpans` const).

### 5. Election Provider Guven Siniri

```rust
type ElectionProvider: ElectionProvider<DataProvider = Pallet<Self>>;
```

Staking pallet election sonucuna guvenilir — herhangi bir validation yapmaz. `MinimumValidatorCount` enforse edilir:

```rust
if (exposures.len() as u32) < MinimumValidatorCount::<T>::get().max(1) {
    Self::deposit_event(Event::StakingElectionFailed);
    return None;  // hicbir sey degismez, eski set devam eder
}
```

---

## Pallet Cagri Indeksleri (Call Index)

| Index | Extrinsic | Orijin |
|---|---|---|
| 0 | `bond` | Signed |
| 1 | `bond_extra` | Signed |
| 2 | `unbond` | Signed |
| 3 | `withdraw_unbonded` | Signed |
| 4 | `validate` | Signed |
| 5 | `nominate` | Signed |
| 6 | `chill` | Signed |
| 7 | `set_payee` | Signed |
| 8 | `set_controller` | Signed |
| 9 | `set_validator_count` | Root |
| 10 | `increase_validator_count` | Root |
| 11 | `scale_validator_count` | Root |
| 12 | `force_no_eras` | Root |
| 13 | `force_new_era` | Root |
| 14 | `set_invulnerables` | Root |
| 15 | `force_unstake` | Root |
| 16 | `force_new_era_always` | Root |
| 17 | `cancel_deferred_slash` | AdminOrigin |
| 18 | `payout_stakers` | Signed (anyone) |
| 19 | `rebond` | Signed |
| 20 | `reap_stash` | Signed (anyone) |
| 21 | `kick` | Signed |
| 22 | `set_staking_configs` | Root |
| 23 | `chill_other` | Signed (anyone) |
| 24 | `force_apply_min_commission` | Signed (anyone) |
| 25 | `set_min_commission` | AdminOrigin |
| 26 | `payout_stakers_by_page` | Signed (anyone) |
| 27 | `update_payee` | Signed (free) |
| 28 | `deprecate_controller_batch` | AdminOrigin |
| 29 | `restore_ledger` | AdminOrigin |
| 30 | `migrate_currency` | Signed (anyone) |

---

## Gercek Polkadot Uretim Parametreleri (Referans)

| Parametre | Polkadot Mainnet | Kusama |
|---|---|---|
| SessionsPerEra | 6 | 6 |
| BondingDuration | 28 era (~28 gun) | 28 era (~7 gun) |
| SlashDeferDuration | 27 era | 27 era |
| HistoryDepth | 84 | 84 |
| MaxValidatorSet | 297 | 1000 |
| MaxExposurePageSize | 512 | 512 |
| MaxUnlockingChunks | 32 | 32 |
| MaxNominations | 16 | 24 |
| Session suresi | ~24 dakika | ~6 dakika |
| Era suresi | ~24 saat | ~6 saat |

---

## Tam Tip Referanslari

### StakingLedger Alanlari

```
stash          : AccountId        — kim icin
total          : Balance          — active + sum(unlocking)
active         : Balance          — election'a katilir, slashable
unlocking      : Vec<UnlockChunk> — FiFo, bonding_duration sonra unlock
legacy_claimed : Vec<EraIndex>    — deprecated v14
controller     : Option<AccountId> — storage'a yazilmaz
```

### Exposure Alanlari

```
total          : Balance               — validator + tum nominatorler
own            : Balance               — validator'in kendi stake'i
others         : Vec<IndividualExposure> — [{who: AccountId, value: Balance}]
```

### UnappliedSlash Alanlari

```
validator      : AccountId             — slashlanacak
own            : Balance               — validator'in slash miktari
others         : Vec<(AccountId, Balance)> — nominatorler
reporters      : Vec<AccountId>        — ihbarcilara odeme
payout         : Balance               — toplam ihbarci odulu
```

### SpanRecord Alanlari

```
slashed        : Balance   — bu span'da toplam slashlanan
paid_out       : Balance   — bu span'dan odenen ihbarci odulu
```

---

## Sonuc: Nova Shield Icin Eylem Plani

### Acil (v0.1 oncesi)

1. `StakingLedger` tasarla: stash == controller, hold-based, controller alani yok
2. Inflation egrisini implement et: `compute_inflation()` + `compute_total_payout()` formulu
3. Slashing mekanizmasi: Max-in-era prensibi, proportional chunk slash
4. `payout_stakers_by_page()` logic: points-based, komisyon once, then proportional
5. `ElectionDataProvider` trait: voter/target listesi, bounds kontrolu

### Orta Vadeli (v0.2-v0.3)

6. Paged exposure: Bastan `ErasStakersOverview` + `ErasStakersPaged`
7. VirtualStakers: Pool pallet entegrasyonu icin
8. Dynamic NominationsQuota: Stake bazli kota
9. `chill_other()` mekanizmasi: Governance onay sistemi
10. Election size tracker: SCALE boyut hesabi

### Uzun Vadeli

11. `pallet-bags-list` entegrasyonu veya ozel sirali voter list
12. `MaxSlashingSpans` const ekle (guvenlik)
13. Governance parametreleri: BondingDuration, SlashDeferDuration ayarlanabilir
14. Runtime API: nominations_quota, pending_rewards, page_count
15. `pallet-staking-async` mimarisine gecis plani
