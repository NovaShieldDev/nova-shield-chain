# pallet-democracy + pallet-referenda — Agent 10

**Analiz tarihi:** 2026-07-22
**Kaynak dizinler:**
- `/substrate/frame/democracy/src/` — 21 .rs dosyası
- `/substrate/frame/referenda/src/` — 8 .rs dosyası
**Toplam satır:** ~4.200 satır birincil kod (benchmarking, test ve migration dahil)

---

## KISIM 1: pallet-democracy (Eski Sistem — v1 Governance)

### 1.1 Genel Mimari ve Tasarım Felsefesi

pallet-democracy, Polkadot/Substrate'in **birinci nesil yönetişim sistemidir**. OpenGov (pallet-referenda) ile değiştirilmeden önce Polkadot, Kusama ve tüm Substrate tabanlı zincirler bu pallet'i kullandı. Şu anda "deprecated" (kullanım dışı) sayılsa da kavramsal olarak hâlâ kritik bir referans noktasıdır.

Temel felsefe: **Adaptif Quorum Biasing (Uyarlanabilir Çoğunluk Eğilimi)** — Bir referandum nasıl önerildiğine göre farklı onay eşikleri uygular.

---

### 1.2 Öneri Pipeline — Tüm Akış

#### İki Tür Öneri Kuyruğu

**Kuyruk 1: Public Proposal Queue (Halk Önerisi Kuyruğu)**

```
Herhangi bir hesap
  → propose(proposal_hash, value >= MinimumDeposit)
  → PublicProps storage'a eklenir
  → Diğer hesaplar second() ile destek ekler (eşit depozito gerekir)
  → Her LaunchPeriod'da, en yüksek "backing_for" değerine sahip öneri seçilir
  → inject_referendum() ile referanduma dönüştürülür
  → Threshold: SuperMajorityApprove (her zaman)
  → Depozitolar iade edilir (tablo anında)
```

**Kuyruk 2: External Proposal Queue (Harici Öneri Kuyruğu)**

```
ExternalOrigin (genellikle Council)
  → external_propose(proposal) → NextExternal = (proposal, SuperMajorityApprove)
  → ExternalMajorityOrigin
  → external_propose_majority(proposal) → NextExternal = (proposal, SimpleMajority)
  → ExternalDefaultOrigin
  → external_propose_default(proposal) → NextExternal = (proposal, SuperMajorityAgainst)
```

#### Alternating Launch Mekanizması

`lib.rs:1538-1543` — Her `LaunchPeriod` dolduğunda `begin_block()` çalışır:

```rust
fn launch_next(now: BlockNumberFor<T>) -> DispatchResult {
    if LastTabledWasExternal::<T>::take() {
        Self::launch_public(now).or_else(|_| Self::launch_external(now))
    } else {
        Self::launch_external(now).or_else(|_| Self::launch_public(now))
    }
    .map_err(|_| Error::<T>::NoneWaiting.into())
}
```

Bu mekanizma şunu garanti eder: Eğer son sefer external proposal geldiyse, bu sefer public proposal önceliklidir; ve tam tersi. Eğer öncelikli kuyruk boşsa fallback olarak diğer kuyruk kullanılır.

#### Referanduma Dönüşüm

`inject_referendum()` — lib.rs:1519-1534:

```rust
fn inject_referendum(
    end: BlockNumberFor<T>,        // şu_an + VotingPeriod
    proposal: BoundedCallOf<T>,
    threshold: VoteThreshold,
    delay: BlockNumberFor<T>,      // EnactmentPeriod
) -> ReferendumIndex {
    let ref_index = ReferendumCount::<T>::get();
    ReferendumCount::<T>::put(ref_index + 1);
    let status = ReferendumStatus { end, proposal, threshold, delay, tally: Default::default() };
    let item = ReferendumInfo::Ongoing(status);
    ReferendumInfoOf::<T>::insert(ref_index, item);
    Self::deposit_event(Event::<T>::Started { ref_index, threshold });
    ref_index
}
```

- `end` = `now + VotingPeriod` bloğu
- `delay` = `EnactmentPeriod` — onaylandıktan sonra kaç blok daha beklenecek

#### Baking (Sonuç Değerlendirme)

`bake_referendum()` — lib.rs:1597-1627:

```rust
fn bake_referendum(now, index, status) -> bool {
    let total_issuance = T::Currency::total_issuance();  // seçmen kütlesi
    let approved = status.threshold.approved(status.tally, total_issuance);
    if approved {
        // schedule_named() ile EnactmentPeriod sonrasına programlan
        let when = now + status.delay.max(One::one());
        T::Scheduler::schedule_named(...)
    }
    approved
}
```

`begin_block()` her blokta `maturing_referenda_at_inner()` ile `end == now` olan tüm referandaları tarar ve `bake_referendum()` çağırır.

---

### 1.3 Oy Ağırlığı ve Conviction — Tam Formüller

#### Conviction Enum (conviction.rs:44-59)

```rust
pub enum Conviction {
    None,       // 0.1x oy gücü, kilit yok
    Locked1x,   // 1x oy gücü, 1  EnactmentPeriod kilitli
    Locked2x,   // 2x oy gücü, 2  EnactmentPeriod kilitli
    Locked3x,   // 3x oy gücü, 4  EnactmentPeriod kilitli
    Locked4x,   // 4x oy gücü, 8  EnactmentPeriod kilitli
    Locked5x,   // 5x oy gücü, 16 EnactmentPeriod kilitli
    Locked6x,   // 6x oy gücü, 32 EnactmentPeriod kilitli
}
```

#### Lock Periods (conviction.rs:100-110)

```rust
pub fn lock_periods(self) -> u32 {
    match self {
        Conviction::None     => 0,
        Conviction::Locked1x => 1,
        Conviction::Locked2x => 2,   // 2^1
        Conviction::Locked3x => 4,   // 2^2
        Conviction::Locked4x => 8,   // 2^3
        Conviction::Locked5x => 16,  // 2^4
        Conviction::Locked6x => 32,  // 2^5
    }
}
```

**ÖNEMLİ PATTERN:** Conviction değeri N için kilit = `2^(N-1)` periyot (None hariç). Conviction artışı 1 birimdir ama kilit süresi **ikiyle çarpılır**.

#### votes() Fonksiyonu — Oy Gücü Hesabı (conviction.rs:112-122)

```rust
pub fn votes<B: From<u8> + Zero + Copy + CheckedMul + CheckedDiv + Bounded>(
    self,
    capital: B,
) -> Delegations<B> {
    let votes = match self {
        Conviction::None => capital.checked_div(&10u8.into()).unwrap_or_else(Zero::zero),
        x => capital.checked_mul(&u8::from(x).into()).unwrap_or_else(B::max_value),
    };
    Delegations { votes, capital }
}
```

**Formül özeti:**
- `None`: `oy_gücü = balance / 10` (yani 0.1x)
- `Locked1x..Locked6x`: `oy_gücü = balance * conviction_sayısı`

| Conviction | Oy Gücü      | Kilit Süre      | Kilit = EnactmentPeriod × N |
|------------|-------------|-----------------|------------------------------|
| None       | 0.1 × bakiye | 0 periyot       | Kilit yok                   |
| Locked1x   | 1 × bakiye   | 1 periyot       | 1 × EnactmentPeriod          |
| Locked2x   | 2 × bakiye   | 2 periyot       | 2 × EnactmentPeriod          |
| Locked3x   | 3 × bakiye   | 4 periyot       | 4 × EnactmentPeriod          |
| Locked4x   | 4 × bakiye   | 8 periyot       | 8 × EnactmentPeriod          |
| Locked5x   | 5 × bakiye   | 16 periyot      | 16 × EnactmentPeriod         |
| Locked6x   | 6 × bakiye   | 32 periyot      | 32 × EnactmentPeriod         |

#### Unlock Bloğu Hesabı (lib.rs:1347-1349)

```rust
let unlock_at = end.saturating_add(
    T::VoteLockingPeriod::get().saturating_mul(lock_periods.into()),
);
```

Kilit kalkma zamanı = `referendum_end + VoteLockingPeriod × lock_periods`

**NOT:** `VoteLockingPeriod` en az `EnactmentPeriod` kadar olmalıdır. Bu config.rs belgesinde şu şekilde ifade edilir:
> "It should be no shorter than enactment period to ensure that in the case of an approval, those successful voters are locked into the consequences that their votes entail."

#### Tally Yapısı (types.rs:36-43)

```rust
pub struct Tally<Balance> {
    pub ayes: Balance,     // Post-conviction oy gücü (ağırlıklı)
    pub nays: Balance,     // Post-conviction oy gücü (ağırlıklı)
    pub turnout: Balance,  // Pre-conviction ham bakiye toplamı
}
```

`ayes` ve `nays` conviction ile çarpılmış değerleri tutar; `turnout` ham bakiyeleri tutar. Bu ayrım SuperMajority hesaplamalarında kritiktir.

#### Test Doğrulaması (lock_voting.rs:50-55)

```rust
assert_ok!(Democracy::vote(RuntimeOrigin::signed(1), r, nay(5, 10)));   // 5x conviction, 10 bakiye
assert_ok!(Democracy::vote(RuntimeOrigin::signed(2), r, aye(4, 20)));   // 4x conviction, 20 bakiye
assert_ok!(Democracy::vote(RuntimeOrigin::signed(3), r, aye(3, 30)));   // 3x conviction, 30 bakiye
assert_ok!(Democracy::vote(RuntimeOrigin::signed(4), r, aye(2, 40)));   // 2x conviction, 40 bakiye
assert_ok!(Democracy::vote(RuntimeOrigin::signed(5), r, nay(1, 50)));   // 1x conviction, 50 bakiye
assert_eq!(tally(r), Tally { ayes: 250, nays: 100, turnout: 150 });
```

Doğrulama:
- `ayes = 4×20 + 3×30 + 2×40 = 80 + 90 + 80 = 250` ✓
- `nays = 5×10 + 1×50 = 50 + 50 = 100` ✓
- `turnout = 10 + 20 + 30 + 40 + 50 = 150` ✓

#### Split Vote

```rust
AccountVote::Split { aye: Balance, nay: Balance }
```

Split oy her zaman `Conviction::None` ile işlenir (conviction.rs değil, types.rs:115-119):

```rust
AccountVote::Split { aye, nay } => {
    let aye = Conviction::None.votes(aye);
    let nay = Conviction::None.votes(nay);
    // her iki taraf da 0.1x oy gücü
}
```

---

### 1.4 Çoğunluk Algoritmaları — Tam Formüller

**Kaynak:** `vote_threshold.rs`

#### Üç VoteThreshold Varyantı

```rust
pub enum VoteThreshold {
    SuperMajorityApprove,   // Pozitif turnout bias
    SuperMajorityAgainst,   // Negatif turnout bias
    SimpleMajority,         // 50%+1
}
```

#### approved() Implementasyonu (vote_threshold.rs:103-118)

```rust
fn approved(&self, tally: Tally<Balance>, electorate: Balance) -> bool {
    let sqrt_voters = tally.turnout.integer_sqrt();        // √turnout
    let sqrt_electorate = electorate.integer_sqrt();       // √total_issuance
    if sqrt_voters.is_zero() {
        return false;
    }
    match *self {
        VoteThreshold::SuperMajorityApprove => {
            compare_rationals(tally.nays, sqrt_voters, tally.ayes, sqrt_electorate)
            // nays/√turnout < ayes/√electorate → geçer
        },
        VoteThreshold::SuperMajorityAgainst => {
            compare_rationals(tally.nays, sqrt_electorate, tally.ayes, sqrt_voters)
            // nays/√electorate < ayes/√turnout → geçer
        },
        VoteThreshold::SimpleMajority => tally.ayes > tally.nays,
    }
}
```

#### SuperMajorityApprove (Pozitif Bias — Halk Önerisi Varsayılanı)

**Geçiş koşulu:**
```
nays / √turnout < ayes / √electorate
```

Yeniden düzenlenirse:
```
ayes / nays > √electorate / √turnout
```

Bu formülün özellikleri:
- **Düşük katılım** → gerekli ayes/nays oranı yüksek (daha zor onay)
- **Yüksek katılım** → gerekli oran azalır (daha kolay onay)
- Katılım toplamla eşitlenirse (turnout = electorate): basit çoğunluk yeterli

**Örnek (test dosyasından):**
```rust
// ONAYLANMAZ: ayes=60, nays=50, turnout=110, electorate=210
// Koşul: 50/√110 < 60/√210 → 4.77 < 4.14 → FALSE
assert!(!VoteThreshold::SuperMajorityApprove
    .approved(Tally { ayes: 60, nays: 50, turnout: 110 }, 210));

// ONAYLANIR: ayes=100, nays=50, turnout=150, electorate=210
// Koşul: 50/√150 < 100/√210 → 4.08 < 6.90 → TRUE
assert!(VoteThreshold::SuperMajorityApprove
    .approved(Tally { ayes: 100, nays: 50, turnout: 150 }, 210));
```

#### SuperMajorityAgainst (Negatif Bias — ExternalDefault)

**Geçiş koşulu:**
```
nays / √electorate < ayes / √turnout
```

Yeniden düzenlenirse:
```
ayes / nays > √turnout / √electorate
```

Özellikleri:
- **Düşük katılım** → gerekli oran düşük → kolay onay (default geçer)
- **Yüksek katılım** → gerekli oran artar → daha zor onay
- Turnout = 0 olsa bile nays = 0 ise geçer (gerçek "negatif bias")

#### SimpleMajority (Basit Çoğunluk)

```rust
tally.ayes > tally.nays
```

Sadece conviction-ağırlıklı oy sayısına bakar; turnout ve electorate dikkate alınmaz.

#### compare_rationals() — Overflow-safe Karşılaştırma (vote_threshold.rs:58-90)

```rust
fn compare_rationals<T: ...>(mut n1: T, mut d1: T, mut n2: T, mut d2: T) -> bool {
    // n1/d1 < n2/d2 mi?
    // Devam kesrinde çarpma olmadan karşılaştırır
    loop {
        let q1 = n1 / d1;
        let q2 = n2 / d2;
        if q1 < q2 { return true; }
        if q2 < q1 { return false; }
        let r1 = n1 % d1;
        let r2 = n2 % d2;
        if r2.is_zero() { return false; }
        if r1.is_zero() { return true; }
        n1 = d2; n2 = d1; d1 = r2; d2 = r1;
    }
}
```

Bu algoritma hiç çarpma yapmaz, sadece bölme ve mod kullanır — bu sayede büyük Balance değerlerinde taşma olmaz. Kaynak: https://janmr.com/blog/2014/05/comparing-rational-numbers-without-overflow/

---

### 1.5 Kritik Sabitler (Config Parametreleri)

Tüm sabitler runtime'da tanımlanır; pallet'in kendisinde hardcode değer yoktur.

```rust
// lib.rs:248-302 — Config trait'inden alınan sabitler

// Referandumun onaylanması ile yürürlüğe girmesi arasındaki süre
type EnactmentPeriod: Get<BlockNumberFor<Self>>;

// Yeni public referandumların ne sıklıkla başlatılacağı
type LaunchPeriod: Get<BlockNumberFor<Self>>;

// Oy verme süresi
type VotingPeriod: Get<BlockNumberFor<Self>>;

// Minimum kilit süresi (en az EnactmentPeriod kadar olmalı)
type VoteLockingPeriod: Get<BlockNumberFor<Self>>;

// Public öneri için minimum depozito
type MinimumDeposit: Get<BalanceOf<Self>>;

// Instant referendum'un izin verilip verilmediği
type InstantAllowed: Get<bool>;

// Fast-track için minimum oylama süresi
type FastTrackVotingPeriod: Get<BlockNumberFor<Self>>;

// Veto sonrasında yeniden öneri için bekleme süresi
type CooloffPeriod: Get<BlockNumberFor<Self>>;

// Bir hesabın maksimum oy sayısı
type MaxVotes: Get<u32>;

// Maksimum public öneri sayısı
type MaxProposals: Get<u32>;

// Bir önerinin maksimum depozitör sayısı
type MaxDeposits: Get<u32>;

// Kara listedeki maksimum öğe sayısı
type MaxBlacklisted: Get<u32>;
```

**Polkadot mainnet değerleri** (referans için, pallet içinde değil runtime'da):
- `EnactmentPeriod`: 28 gün (~403,200 blok @ 6s)
- `LaunchPeriod`: 28 gün
- `VotingPeriod`: 28 gün
- `VoteLockingPeriod`: 28 gün
- `MinimumDeposit`: 100 DOT
- `CooloffPeriod`: 7 gün
- `FastTrackVotingPeriod`: 3 saat

---

### 1.6 Storage Items

```rust
// PublicPropCount: Toplam kaç öneri yapıldı (monoton artan)
StorageValue<_, PropIndex, ValueQuery>

// PublicProps: Aktif public öneriler listesi
StorageValue<_, BoundedVec<(PropIndex, BoundedCallOf<T>, AccountId), MaxProposals>, ValueQuery>

// DepositOf: Her öneri için depozitörler ve miktar
StorageMap<Twox64Concat, PropIndex, (BoundedVec<AccountId, MaxDeposits>, Balance)>

// ReferendumCount: Toplam referandum sayısı
StorageValue<_, ReferendumIndex, ValueQuery>

// LowestUnbaked: Henüz "baked" olmayan en düşük referandum indeksi
StorageValue<_, ReferendumIndex, ValueQuery>

// ReferendumInfoOf: Referandum detayları
StorageMap<Twox64Concat, ReferendumIndex, ReferendumInfo<...>>

// VotingOf: Hesap başına oy bilgisi (Direct veya Delegating)
StorageMap<Twox64Concat, AccountId, Voting<Balance, AccountId, BlockNumber, MaxVotes>>

// LastTabledWasExternal: Son tablo edilen external mi yoksa public mıydı
StorageValue<_, bool, ValueQuery>

// NextExternal: Bekleyen external öneri
StorageValue<_, (BoundedCallOf<T>, VoteThreshold)>

// Blacklist: Kara liste (hash → (until_block, vetoers))
StorageMap<Identity, Hash, (BlockNumber, BoundedVec<AccountId, MaxBlacklisted>)>

// Cancellations: Emergency cancel yapılan öneriler
StorageMap<Identity, Hash, bool, ValueQuery>

// MetadataOf: Öneri/referandum metadata
StorageMap<Blake2_128Concat, MetadataOwner, Hash>
```

---

### 1.7 Delegasyon Mekanizması

**Voting enum (vote.rs:166-187):**

```rust
pub enum Voting<Balance, AccountId, BlockNumber, MaxVotes: Get<u32>> {
    Direct {
        votes: BoundedVec<(ReferendumIndex, AccountVote<Balance>), MaxVotes>,
        delegations: Delegations<Balance>,  // Bana delege edilenlerin toplamı
        prior: PriorLock<BlockNumber, Balance>,
    },
    Delegating {
        balance: Balance,
        target: AccountId,
        conviction: Conviction,
        delegations: Delegations<Balance>,
        prior: PriorLock<BlockNumber, Balance>,
    },
}
```

**Kritik kısıtlamalar:**
1. Tek seviye delegasyon — zincirleme delegasyon çalışmaz (test ile doğrulandı)
2. Delegating hesap aktif oy kullanamaz
3. Direct hesap delegasyon kabul edebilir ve kendi oyunu da kullanabilir
4. Delegasyonun conviction'ı, delegate eden hesabın tokenleri için kullanılır

**try_delegate() akışı (lib.rs:1421-1470):**

```
1. who != target doğrulaması (self-delegation yasak)
2. balance <= free_balance doğrulaması
3. VotingOf[who] = Delegating { balance, target, conviction, ... }
4. increase_upstream_delegation(&target, conviction.votes(balance)) çağrılır
5. target.votes içindeki HER aktif referanduma delegasyon eklenir
6. extend_lock(DEMOCRACY_ID, who, balance, ...)
```

**Delegasyon oy etkisi (test, delegation.rs:34-35):**
```rust
// Hesap 1 oy kullanır (1 bakiye)
// Hesap 2, hesap 1'e None conviction ile 20 bakiye delege eder
// Tally: ayes = 3 (1*1 + 20*0.1 = 1+2 = 3), turnout = 30 (1+20=21?
// Aslında: 1'in oyu 1*1=1 ayes; 2'nin delegasyonu 20/10=2 ayes; turnout=1+20=21)
```

`assert_eq!(tally(r), Tally { ayes: 3, nays: 0, turnout: 30 });`

Test burada: hesap 1'in 10 bakiyesi + hesap 2'nin 20 bakiyesi + hesap 3'ün 30 bakiyesi = turnout 60 durumunda geliyor. Bu gerçek test değerlerini almak için delegasyon testi tam bağlam gerektiriyor.

**PriorLock (vote.rs:141-160):**

```rust
pub struct PriorLock<BlockNumber, Balance>(BlockNumber, Balance);

impl<...> PriorLock<...> {
    pub fn accumulate(&mut self, until: BlockNumber, amount: Balance) {
        self.0 = self.0.max(until);   // en geç kilit bitiş zamanı
        self.1 = self.1.max(amount);  // en yüksek miktarı sakla
    }

    pub fn rejig(&mut self, now: BlockNumber) {
        if now >= self.0 {
            // Kilit süresi dolmuş, temizle
            self.0 = Zero::zero();
            self.1 = Zero::zero();
        }
    }
}
```

PriorLock, geçmiş oylardan kalan kilit bilgisini (delegasyon değiştirme veya oy kaldırma sonrası) saklar. Kilit hiçbir zaman "aniden azalmaz"; en uzun süre ve en yüksek miktarı tutar.

---

### 1.8 External Proposal ve Veto Mekanizması

#### Üç External Threshold Türü

| Extrinsic | Origin | Threshold | Semantik |
|---|---|---|---|
| `external_propose` | ExternalOrigin | SuperMajorityApprove | Normal harici öneri |
| `external_propose_majority` | ExternalMajorityOrigin | SimpleMajority | Çoğunluk harici öneri |
| `external_propose_default` | ExternalDefaultOrigin | SuperMajorityAgainst | Varsayılan harici öneri |

**lib.rs:709,731,753:**
```rust
// external_propose:
NextExternal::<T>::put((proposal, VoteThreshold::SuperMajorityApprove));

// external_propose_majority:
NextExternal::<T>::put((proposal, VoteThreshold::SimpleMajority));

// external_propose_default:
NextExternal::<T>::put((proposal, VoteThreshold::SuperMajorityAgainst));
```

#### FastTrack (lib.rs:775-813)

FastTrack sadece `SimpleMajority` veya `SuperMajorityAgainst` threshold'una sahip harici öneriler için çalışır — SuperMajorityApprove için çalışmaz (lib.rs:799-801):

```rust
ensure!(
    threshold != VoteThreshold::SuperMajorityApprove,
    Error::<T>::NotSimpleMajority,
);
```

FastTrack, önerinin oylama süresini kısaltır:
- `FastTrackOrigin` için: `voting_period >= FastTrackVotingPeriod`
- `InstantOrigin` için: herhangi bir süre (InstantAllowed = true ise)

#### Veto Mekanizması (lib.rs:827-851)

```rust
fn veto_external(origin, proposal_hash: T::Hash) -> DispatchResult {
    let who = T::VetoOrigin::ensure_origin(origin)?;
    // ...
    let insert_position = existing_vetoers
        .binary_search(&who)
        .err()
        .ok_or(Error::<T>::AlreadyVetoed)?;  // Aynı kişi iki kez veto yapamaz
    existing_vetoers.try_insert(insert_position, who.clone())?;

    let until = now + T::CooloffPeriod::get();
    Blacklist::<T>::insert(&proposal_hash, (until, existing_vetoers));
    NextExternal::<T>::kill();  // Harici öneriyi kuyruğdan çıkar
}
```

**Blacklist çalışması:**
- `(until_block, vetoers_list)` tuple olarak saklanır
- `until` bloğuna kadar aynı hash tekrar önerilemez
- `veto_external`: CooloffPeriod kadar, `blacklist` (Root): `BlockNumber::max_value()`'ye kadar

---

### 1.9 on_initialize / begin_block Akışı

```rust
fn begin_block(now: BlockNumberFor<T>) -> Weight {
    // 1. Her LaunchPeriod'da yeni referandum başlat
    if (now % T::LaunchPeriod::get()).is_zero() {
        if Self::launch_next(now).is_ok() {
            weight = max_block_weight;
        }
    }

    // 2. Süresi dolan referandaları değerlendir
    for (index, info) in Self::maturing_referenda_at_inner(now, next..last) {
        let approved = Self::bake_referendum(now, index, info);
        ReferendumInfoOf::<T>::insert(index, ReferendumInfo::Finished { end: now, approved });
        weight = max_block_weight;
    }

    // 3. LowestUnbaked'i güncelle
    LowestUnbaked::<T>::mutate(|ref_index| {
        while *ref_index < last && ReferendumInfoOf::<T>::get(*ref_index)
            .map_or(true, |info| matches!(info, ReferendumInfo::Finished { .. }))
        {
            *ref_index += 1
        }
    });

    weight
}
```

`LowestUnbaked` izleme, tüm referandumları her blokta taramaktan kaçınmak için kullanılır — O(R) yerine baked'ların üzerinden atlayarak verimlilik sağlar.

---

## KISIM 2: pallet-referenda (OpenGov — Yeni Sistem)

### 2.1 OpenGov Felsefesi ve pallet-democracy ile Fark

**pallet-democracy kısıtlamaları:**
- Aynı anda sadece bir referandum aktif
- Tüm token sahipleri her şeyde oy kullanır
- Council (Technical Committee) veto yetkisi
- Tek tip oylama süresi ve eşik

**pallet-referenda (OpenGov) çözümleri:**
- Paralel referandumlar: Her track'te birden fazla aktif referandum
- Origin tabanlı ayrım: Farklı yetkiler farklı track'lerde
- Her track'in kendi eğrileri: Süreye göre değişen onay/destek eşiği
- Confirmation period: Kararlı çoğunluk gereksinimi
- Council kaldırıldı: Herkes istediği başlatabilir

---

### 2.2 Tracks Sistemi

#### TrackDetails Yapısı (types.rs:177-199)

```rust
pub struct TrackDetails<Balance, Moment, Name> {
    pub name: Name,
    pub max_deciding: u32,           // Bu track'te aynı anda kaç referandum kararlaştırılabilir
    pub decision_deposit: Balance,   // Karar depositosu miktarı
    pub prepare_period: Moment,      // Submission'dan deciding'e geçmeden önce bekleme süresi
    pub decision_period: Moment,     // Karar alma süresi (deciding başlangıcından)
    pub confirm_period: Moment,      // Onay süresi (passing state'te tutulması gereken süre)
    pub min_enactment_period: Moment,// Onaydan yürürlüğe geçişe kadar minimum süre
    pub min_approval: Curve,         // Approval için minimum eşik eğrisi
    pub min_support: Curve,          // Support için minimum eşik eğrisi
}
```

#### TracksInfo Trait (types.rs:211-265)

```rust
pub trait TracksInfo<Balance, Moment, const N: usize = DEFAULT_MAX_TRACK_NAME_LEN> {
    type Id: Copy + Parameter + Ord + ...;
    type RuntimeOrigin;

    fn tracks() -> impl Iterator<Item = Cow<'static, Track<Self::Id, Balance, Moment, N>>>;
    fn track_for(origin: &Self::RuntimeOrigin) -> Result<Self::Id, ()>;
    fn track_ids() -> impl Iterator<Item = Self::Id> { ... }
    fn info(id: Self::Id) -> Option<Cow<'static, TrackInfo<...>>> { ... }
    fn check_integrity() -> Result<(), &'static str> { ... }  // Track'ler sıralı mı?
}
```

Track ID'leri **sıralı** olmalıdır — `check_integrity()` bunu runtime başlangıcında doğrular.

#### Storage Items (lib.rs:257-292)

```rust
// ReferendumInfoFor: Referandum detayları
StorageMap<Blake2_128Concat, ReferendumIndex, ReferendumInfoOf<T, I>>

// TrackQueue: Her track için sıralanmış bekleyen referandumlar
// Oy sayısına göre sıralı (en fazla oylu en önde)
StorageMap<Twox64Concat, TrackId, BoundedVec<(ReferendumIndex, Votes), MaxQueued>, ValueQuery>

// DecidingCount: Her track'te kaç referandum şu anda kararlaştırma aşamasında
StorageMap<Twox64Concat, TrackId, u32, ValueQuery>

// MetadataOf: Referandum metadata
StorageMap<Blake2_128Concat, ReferendumIndex, Hash>

// ReferendumCount: Toplam referandum sayısı
StorageValue<_, ReferendumIndex, ValueQuery>
```

---

### 2.3 Referandum Yaşam Döngüsü

#### 5 Aşama

```
[Submission] → [Preparation] → [Deciding] → [Confirmation] → [Enactment]
                     ↓
              [TimedOut — UndecidingTimeout geçerse]
```

#### Detaylı Akış

**1. Submission:**
```rust
// lib.rs:474-520
pub fn submit(
    origin,
    proposal_origin: Box<PalletsOriginOf<T>>,
    proposal: BoundedCallOf<T, I>,
    enactment_moment: DispatchTime<BlockNumber>,
) -> DispatchResult {
    // 1. Origin kontrol → track_for() ile track belirlenir
    let track = T::Tracks::track_for(&proposal_origin)?;
    // 2. SubmissionDeposit alınır
    let submission_deposit = Self::take_deposit(who, T::SubmissionDeposit::get())?;
    // 3. ReferendumStatus oluşturulur
    let status = ReferendumStatus {
        track, origin: proposal_origin, proposal,
        submitted: now, submission_deposit,
        decision_deposit: None, deciding: None,
        tally: TallyOf::<T, I>::new(track),
        in_queue: false,
        alarm: Self::set_alarm(nudge_call, now + T::UndecidingTimeout::get()),
    };
    // 4. ReferendumInfo::Ongoing olarak saklanır
}
```

**2. Decision Deposit (Karar Depositosu):**

```rust
// lib.rs:533-550
pub fn place_decision_deposit(origin, index: ReferendumIndex) -> DispatchResultWithPostInfo {
    ensure!(status.decision_deposit.is_none(), Error::<T, I>::HasDeposit);
    let track = T::Tracks::info(status.track)?;
    status.decision_deposit = Some(Self::take_deposit(who, track.decision_deposit)?);
    // service_referendum() çağrılır → geçişler kontrol edilir
    let (info, _, branch) = Self::service_referendum(now, index, status);
}
```

Decision deposit herkes tarafından ödenebilir (proposer olmak zorunda değil).

**3. Preparation Tamamlanması:**

`service_referendum()` her `nudge_referendum` çağrısında çalışır:

```rust
// Prepare koşulları (lib.rs:1148-1165):
if status.decision_deposit.is_some() {
    let prepare_end = status.submitted + track.prepare_period;
    if now >= prepare_end {
        // ready_for_deciding() çağrılır
    } else {
        alarm = prepare_end;  // prepare_end'de uyandır
        ServiceBranch::Preparing
    }
}
```

**4. Deciding:**

`ready_for_deciding()` (lib.rs:1003-1022):

```rust
fn ready_for_deciding(now, track, index, status) -> (Option<BlockNumber>, ServiceBranch) {
    let deciding_count = DecidingCount::<T, I>::get(status.track);
    if deciding_count < track.max_deciding {
        // Kapasitesi var → deciding başlar
        DecidingCount::<T, I>::insert(status.track, deciding_count + 1);
        Self::begin_deciding(status, index, now, track)
    } else {
        // Dolu → sıraya al (vote sayısına göre)
        let item = (index, status.tally.ayes(status.track));
        TrackQueue::mutate(status.track, |q| q.insert_sorted_by_key(item, |x| x.1));
        status.in_queue = true;
        (None, ServiceBranch::Queued)
    }
}
```

**5. Confirmation:**

```rust
// lib.rs:1183-1248 — service_referendum deciding branch'i
let is_passing = Self::is_passing(&status.tally, elapsed, track.decision_period, ...);
if is_passing {
    match deciding.confirming {
        Some(t) if now >= t => {
            // Onay süresi doldu → ONAYLANDI
            Self::schedule_enactment(index, &track, desired, origin, call);
            return (ReferendumInfo::Approved(...), true, ServiceBranch::Approved);
        },
        Some(_) => ServiceBranch::ContinueConfirming,
        None => {
            // Confirmation başlat
            deciding.confirming = Some(now + track.confirm_period);
            ServiceBranch::BeginConfirming
        },
    }
} else {
    if now >= deciding.since + track.decision_period {
        // Karar süresi doldu, passing değil → REDDEDİLDİ
        return (ReferendumInfo::Rejected(...), true, ServiceBranch::Rejected);
    }
    if deciding.confirming.is_some() {
        // Confirmation kesintiye uğradı
        deciding.confirming = None;
        ServiceBranch::EndConfirming
    }
}
```

**ÖNEMLİ:** Confirmation kesilirse sayaç sıfırlanır ve yeniden confirmation period kadar passing state'te kalması gerekir. Confirmation "kümülatif" değil "kesintisiz" olmalıdır.

---

### 2.4 Approval ve Support Eğrileri — Matematiksel Formüller

#### Curve Enum (types.rs:390-401)

```rust
pub enum Curve {
    LinearDecreasing { length: Perbill, floor: Perbill, ceil: Perbill },
    SteppedDecreasing { begin: Perbill, end: Perbill, step: Perbill, period: Perbill },
    Reciprocal { factor: FixedI64, x_offset: FixedI64, y_offset: FixedI64 },
}
```

#### threshold(x: Perbill) → Perbill (types.rs:537-549)

`x` = elapsed / decision_period (0.0 başlangıç, 1.0 bitiş)
Dönüş değeri = o anda gerekli minimum eşik (0.0 - 1.0)

```rust
pub fn threshold(&self, x: Perbill) -> Perbill {
    match self {
        Self::LinearDecreasing { length, floor, ceil } => {
            // x=0'da ceil'den başlar, x=length'de floor'a düşer, sonra sabit kalır
            *ceil - (x.min(*length).saturating_div(*length, Down) * (*ceil - *floor))
        },
        Self::SteppedDecreasing { begin, end, step, period } => {
            // Her 'period' geçişinde 'step' kadar düşer, 'end'de durur
            (*begin - (step.int_mul(x.int_div(*period))).min(*begin)).max(*end)
        },
        Self::Reciprocal { factor, x_offset, y_offset } => {
            // K/(x+S) - T şeklinde hiperbolik eğri
            factor.checked_rounding_div(FixedI64::from(x) + *x_offset, Low)
                .map(|yp| (yp + *y_offset).into_clamped_perthing())
                .unwrap_or_else(Perbill::one)
        },
    }
}
```

**LinearDecreasing formülü detay:**
```
y(x) = ceil - min(x, length) / length × (ceil - floor)

- x ∈ [0, length]: lineer düşüş ceil'den floor'a
- x ∈ [length, 1]: floor sabit kalır
```

**SteppedDecreasing formülü detay:**
```
steps = floor(x / period)
y(x) = max(begin - steps × step, end)

- Her 'period' fraksiyonu geçildiğinde 'step' kadar düşer
- Minimum değer 'end'dir
```

**Reciprocal (Hiperbolik) formülü detay:**
```
y(x) = K/(x + S) - T

Burada K=factor, S=x_offset, T=y_offset

- x=0'da maksimum değer: K/S - T
- x→∞: -T'ye yaklaşır (ama genellikle 0 ile kısıtlanır)
- Başlangıçta çok hızlı düşer, sona doğru yavaşlar
```

#### passing(x, y) (types.rs:610-612)

```rust
pub fn passing(&self, x: Perbill, y: Perbill) -> bool {
    y >= self.threshold(x)   // Gerçek değer >= gerekli eşik mi?
}
```

#### is_passing() (lib.rs:1318-1329)

```rust
fn is_passing(tally, elapsed, period, support_needed, approval_needed, id) -> bool {
    let x = Perbill::from_rational(elapsed.min(period), period);
    support_needed.passing(x, tally.support(id)) &&
        approval_needed.passing(x, tally.approval(id))
}
```

**İki boyutlu geçiş koşulu:** Hem support hem approval eşzamanlı olarak eşiklerini karşılamalı.

#### decision_time() — Alarm Hesabı (lib.rs:1263-1278)

```rust
fn decision_time(deciding, tally, track_id, track) -> BlockNumber {
    deciding.confirming.unwrap_or_else(|| {
        let approval = tally.approval(track_id);
        let support = tally.support(track_id);
        let until_approval = track.min_approval.delay(approval);  // Eğri üzerinde geriye doğru hesap
        let until_support = track.min_support.delay(support);
        let offset = until_support.max(until_approval);  // En kısıtlayıcı olanı al
        deciding.since + offset.mul_ceil(track.decision_period)
    })
}
```

`delay(y)` fonksiyonu: Mevcut `y` değeri eşiği geçmek için kaç `x` geçmesi gerektiğini hesaplar.

#### delay() Fonksiyonu (types.rs:581-607)

```rust
pub fn delay(&self, y: Perbill) -> Perbill {
    match self {
        Self::LinearDecreasing { length, floor, ceil } => {
            if y < *floor { Perbill::one() }  // Hiç geçmez
            else if y > *ceil { Perbill::zero() }  // Zaten geçiyor
            else {
                (*ceil - y).saturating_div(*ceil - *floor, Up).saturating_mul(*length)
            }
        },
        Self::SteppedDecreasing { begin, end, step, period } => {
            if y < *end { Perbill::one() }  // Hiç geçmez
            else {
                period.int_mul((*begin - y.min(*begin) + step.less_epsilon()).int_div(*step))
            }
        },
        Self::Reciprocal { factor, x_offset, y_offset } => {
            let y = FixedI64::from(y);
            let maybe_term = factor.checked_rounding_div(y - *y_offset, High);
            maybe_term
                .and_then(|term| (term - *x_offset).try_into_perthing().ok())
                .unwrap_or_else(Perbill::one)
        },
    }
}
```

**Gerçek kullanım örnekleri (types.rs test bölümünden):**

```rust
// ROOT track için örnek eğriler (28 günlük dönem)
const ROOT_APP: Curve = Curve::make_reciprocal(4, 28, percent(80), percent(50), percent(100));
// Anlam: 4 günde %80 approval eşiği, %50-100 aralığında hiperbolik

const ROOT_SUP: Curve = Curve::make_linear(28, 28, percent(0), percent(50));
// Anlam: 0-28 günde %50'den %0'a lineer azalma (support kolay olsun)

const WHITE_APP: Curve = Curve::make_reciprocal(16, 28*24, percent(96), percent(50), percent(100));
// Whitelist için çok yüksek ilk eşik (16 saat @ 28*24 saatlik dönem)
```

---

### 2.5 Queueing Mekanizması

#### TrackQueue Yapısı

```rust
StorageMap<Twox64Concat, TrackId, BoundedVec<(ReferendumIndex, Votes), MaxQueued>, ValueQuery>
```

Sıralama: `Votes` (ayes sayısı) artan sırada — en yüksek ayes en sonda, `pop()` ile alınır.

#### Queue'ya Ekleme ve Konumlandırma (lib.rs:1016-1019)

```rust
let item = (index, status.tally.ayes(status.track));
TrackQueue::<T, I>::mutate(status.track, |q| q.insert_sorted_by_key(item, |x| x.1));
```

`insert_sorted_by_key`: Binary search ile doğru pozisyona insert eder; `force_insert_keep_right` ile taşma durumunda en düşük priority olan (en az ayes'li) eleman düşer.

#### Queue Güncelleme (service_referendum içinde)

Bir referandum sıradayken oy aldığında (`in_queue == true`):

```rust
// lib.rs:1129-1145
let ayes = status.tally.ayes(status.track);
let mut queue = TrackQueue::<T, I>::get(status.track);
let maybe_old_pos = queue.iter().position(|(x, _)| *x == index);
let new_pos = queue.binary_search_by_key(&ayes, |x| x.1).unwrap_or_else(|x| x);
if maybe_old_pos.is_none() && new_pos > 0 {
    queue.force_insert_keep_right(new_pos, (index, ayes));  // Yeni ekleme
    ServiceBranch::RequeuedInsertion
} else if let Some(old_pos) = maybe_old_pos {
    queue[old_pos].1 = ayes;
    queue.slide(old_pos, new_pos);  // Kayma
    ServiceBranch::RequeuedSlide
}
```

#### next_for_deciding() (lib.rs:1026-1036)

```rust
fn next_for_deciding(track_queue) -> Option<(ReferendumIndex, ReferendumStatusOf<T, I>)> {
    loop {
        let (index, _) = track_queue.pop()?;  // En yüksek ayes'li al
        match Self::ensure_ongoing(index) {
            Ok(s) => return Some((index, s)),
            Err(_) => {},  // Cancelled/timed-out — atla
        }
    }
}
```

#### one_fewer_deciding() (lib.rs:668-692)

Bir referandum deciding aşamasını terk ettiğinde (onay veya red), bu extrinsic çağrılır:

```rust
fn one_fewer_deciding(origin, track) -> DispatchResultWithPostInfo {
    let track_info = T::Tracks::info(track)?;
    let mut track_queue = TrackQueue::<T, I>::get(track);
    if let Some((index, mut status)) = Self::next_for_deciding(&mut track_queue) {
        // Queue'dan bir sonrakini al ve deciding'e başlat
        let (maybe_alarm, branch) = Self::begin_deciding(&mut status, index, now, &track_info);
        // Alarm kur
        TrackQueue::<T, I>::insert(track, track_queue);
        branch.into()
    } else {
        // Queue boş → DecidingCount azalt
        DecidingCount::<T, I>::mutate(track, |x| x.saturating_dec());
        OneFewerDecidingBranch::QueueEmpty
    }
}
```

---

### 2.6 Alarm Sistemi

Referandum durumu değişikliklerini otomatikleştiren scheduler tabanlı wake-up mekanizması.

#### set_alarm() (lib.rs:929-955)

```rust
fn set_alarm(call: BoundedCallOf<T, I>, when: BlockNumber) -> Option<(BlockNumber, ScheduleAddress)> {
    let alarm_interval = T::AlarmInterval::get().max(One::one());
    // when'i alarm_interval'ın bir sonraki katına yuvarla (yukarı)
    let when = (when + alarm_interval - 1) / alarm_interval * alarm_interval;
    let result = T::Scheduler::schedule(
        DispatchTime::At(when),
        None,
        128u8,  // öncelik
        frame_system::RawOrigin::Root.into(),
        call,
    );
    result.ok().map(|x| (when, x))
}
```

`AlarmInterval`: Küçük bir değer daha hassas zamanlama ama daha fazla storage I/O.

#### ensure_alarm_at() (lib.rs:1062-1086)

Mevcut alarm farklı bir zamana işaretliyse: eski alarmı iptal et, yeni alarm kur.

```rust
fn ensure_alarm_at(status, index, alarm) -> bool {
    if status.alarm.as_ref().map_or(true, |&(when, _)| when != alarm) {
        Self::ensure_no_alarm(status);  // Eski alarmı iptal et
        // nudge_referendum call'ı bağla
        status.alarm = Self::set_alarm(call, alarm);
        true
    } else {
        false  // Değişiklik yok
    }
}
```

---

### 2.7 Polling Trait Implementasyonu

pallet-referenda, voting logic'i pallet-conviction-voting gibi dışarıdaki palletlere bırakmak için `Polling` trait'ini implement eder:

```rust
impl<T: Config<I>, I: 'static> Polling<T::Tally> for Pallet<T, I> {
    type Index = ReferendumIndex;
    type Votes = VotesOf<T, I>;
    type Moment = BlockNumberFor<T, I>;
    type Class = TrackIdOf<T, I>;

    fn access_poll<R>(index: Self::Index, f: impl FnOnce(PollStatus<...>) -> R) -> R {
        match ReferendumInfoFor::get(index) {
            Some(ReferendumInfo::Ongoing(mut status)) => {
                let result = f(PollStatus::Ongoing(&mut status.tally, status.track));
                // Bir sonraki blokta alarm kur (oy geldi)
                Self::ensure_alarm_at(&mut status, index, now + One::one());
                ReferendumInfoFor::insert(index, ReferendumInfo::Ongoing(status));
                result
            },
            Some(ReferendumInfo::Approved(end, ..)) => f(PollStatus::Completed(end, true)),
            Some(ReferendumInfo::Rejected(end, ..)) => f(PollStatus::Completed(end, false)),
            _ => f(PollStatus::None),
        }
    }
}
```

Her oy geldiğinde `ensure_alarm_at(now + 1)` çağrılır → bir sonraki blokta `nudge_referendum` çalışır.

---

### 2.8 ReferendumInfo Sonuç Durumları

```rust
pub enum ReferendumInfo<...> {
    Ongoing(ReferendumStatus<...>),

    // Depozitolar: (submission_deposit, decision_deposit)
    Approved(Moment, Option<Deposit<...>>, Option<Deposit<...>>),   // İade edilebilir
    Rejected(Moment, Option<Deposit<...>>, Option<Deposit<...>>),   // Submission iade edilmez
    Cancelled(Moment, Option<Deposit<...>>, Option<Deposit<...>>),  // Her ikisi de iade edilir
    TimedOut(Moment, Option<Deposit<...>>, Option<Deposit<...>>),   // Decision deposit iade olmaz
    Killed(Moment),                                                   // Her ikisi de yakılır
}
```

| Sonuç | Submission Deposit | Decision Deposit | Açıklama |
|-------|-------------------|-----------------|-----------|
| Approved | İade edilir | İade edilir | Başarılı onay |
| Rejected | İade edilmez | İade edilir | Oy kaybedildi |
| Cancelled | İade edilir | İade edilir | CancelOrigin tarafından |
| TimedOut | İade edilmez | İade edilir | UndecidingTimeout geçti |
| Killed | Her ikisi yakılır | Her ikisi yakılır | KillOrigin tarafından |

**Submission deposit'in neden iade edilmediği (Rejected/TimedOut):** Bu tasarım, spam önerileri caydırmak içindir. Reddedilen bir öneri için depozito slashing yoktur (sadece "iade edilmez"); Killed durumunda ise tam slashing olur.

---

### 2.9 ServiceBranch — State Machine

`branch.rs` dosyasındaki `ServiceBranch` enum'u, `service_referendum()` fonksiyonunun olası sonuçlarını modelleyerek farklı weight değerlerini hesaplamak için kullanılır:

```rust
pub enum ServiceBranch {
    Fail,                    // Track bulunamadı
    NoDeposit,              // Decision deposit yok, alarm timeout'a set
    Preparing,              // prepare_period henüz bitmedi
    Queued,                 // Track doluydu, sıraya alındı
    NotQueued,              // Sıralamada güncelleme gerekmiyor
    RequeuedInsertion,      // Yeni sıra pozisyonu insert edildi
    RequeuedSlide,          // Sıra içinde kaydırıldı
    BeginDecidingPassing,   // Deciding başladı ve şu an passing
    BeginDecidingFailing,   // Deciding başladı ama şu an failing
    BeginConfirming,        // Confirmation başladı
    ContinueConfirming,     // Confirmation devam ediyor
    EndConfirming,          // Confirmation kesildi (artık failing)
    ContinueNotConfirming,  // Failing olmaya devam ediyor
    Approved,               // Confirmation tamamlandı → Onaylandı
    Rejected,               // Decision period doldu, failing
    TimedOut,               // UndecidingTimeout geçti
}
```

Her branch için ayrı weight hesaplanır; bu, Substrate'in weight-based fee sisteminin doğru çalışması için gereklidir.

---

## KISIM 3: pallet-democracy vs pallet-referenda Karşılaştırması

### 3.1 Temel Mimari Farklar

| Özellik | pallet-democracy | pallet-referenda (OpenGov) |
|---------|-----------------|---------------------------|
| Eş zamanlı referandum | 1 adet (sıralı) | Track başına max_deciding |
| Öneri kaynağı | Public + External (2 kuyruk) | Herhangi bir Origin |
| Oy sistemi | Conviction + Lock | Conviction-voting (ayrı pallet) |
| Onay eşiği | Sabit (turnout bazlı) | Süreye göre değişen eğri |
| Onay tamamlama | VotingPeriod dolunca | Confirmation period |
| Track sistemi | Yok | Origin → Track mapping |
| Council veto | Var (VetoOrigin) | Yok |
| Depozito | Minimum deposit | Submission + Decision (ayrı) |
| Metadata | Var (MetadataOwner) | Var (MetadataOf) |
| Alarm sistemi | Yok (her blokta tarama) | Scheduler-based alarm |
| Emergency cancel | AlreadyCanceled kontrolü | CancelOrigin + KillOrigin |

### 3.2 Voting Eşik Karşılaştırması

**pallet-democracy:** Sabit formüller
```
SuperMajorityApprove: nays/√turnout < ayes/√electorate
SuperMajorityAgainst: nays/√electorate < ayes/√turnout
SimpleMajority: ayes > nays
```

**pallet-referenda:** Zamanla değişen eğriler
```
is_passing = support_curve.passing(elapsed/period, actual_support)
           && approval_curve.passing(elapsed/period, actual_approval)
```

Democracy'de electorate = total_issuance; referenda'da support = aye_votes / total_issuance (pre-conviction).

### 3.3 Proposal Queue vs Track Queue

**democracy PublicProps:** Depozito toplamına göre sıralanmış (en yüksek backing önce)
```rust
.max_by_key(|x| Self::backing_for((x.1).0).defensive_unwrap_or_else(Zero::zero))
```

**referenda TrackQueue:** Ayes sayısına göre sıralanmış (en yüksek ayes önce)
```rust
q.insert_sorted_by_key(item, |x| x.1)  // x.1 = ayes count
```

### 3.4 Lock Mekanizması Farkı

**democracy:** Pallet kendi lock'unu yönetir (`DEMOCRACY_ID`)
```rust
T::Currency::extend_lock(DEMOCRACY_ID, who, vote.balance(), WithdrawReasons::...)
```

**referenda:** Voting logic ayrı pallet'te (pallet-conviction-voting); referenda sadece tally günceller.

### 3.5 Yaşam Döngüsü Uzunluğu

**democracy:**
```
Öneri → LaunchPeriod → VotingPeriod → EnactmentPeriod
         (28 gün)       (28 gün)        (28 gün)
Toplam: ~3 ay (tipik mainnet)
```

**referenda:**
```
Submission → PreparePeriod → DecisionPeriod → ConfirmPeriod → EnactmentPeriod
              (track bazlı)    (track bazlı)    (track bazlı)   (track bazlı)
Whitelist Track: çok hızlı (saatler)
Root Track: çok yavaş (haftalar)
```

---

## KISIM 4: Ek Teknik Detaylar

### 4.1 Vote Encoding

`vote.rs:36-59` — Tek byte'a sığdırma:

```rust
// Encoding:
output.push_byte(u8::from(self.conviction) | if self.aye { 0b1000_0000 } else { 0 });

// Decoding:
Vote {
    aye: (b & 0b1000_0000) == 0b1000_0000,  // MSB = aye/nay
    conviction: Conviction::try_from(b & 0b0111_1111)?  // Alt 7 bit = conviction
}
```

| Bit | Anlam |
|-----|-------|
| 7 (MSB) | 1 = aye, 0 = nay |
| 6-3 | Kullanılmıyor |
| 2-0 | Conviction (0-6) |

### 4.2 backing_for() — Halk Önerisi Öncelik Skoru

```rust
pub fn backing_for(proposal: PropIndex) -> Option<BalanceOf<T>> {
    DepositOf::<T>::get(proposal)
        .map(|(l, d)| d.saturating_mul((l.len() as u32).into()))
}
```

**Formül:** `toplam_backing = depositor_count × per_deposit_amount`

Tüm depozitörler aynı miktarı yatırmak zorunda olduğu için bu: `kişi_sayısı × depozito_miktarı`. Dolayısıyla en popüler öneri (en fazla seconded) en öncelikli olur.

### 4.3 LowestUnbaked Optimizasyonu

```rust
// Her blokta begin_block() çalışır:
LowestUnbaked::<T>::mutate(|ref_index| {
    while *ref_index < last &&
        ReferendumInfoOf::<T>::get(*ref_index)
            .map_or(true, |info| matches!(info, ReferendumInfo::Finished { .. }))
    {
        *ref_index += 1
    }
});
```

Bu sayede baked (tamamlanmış) referandumlar bir daha taranmaz — pointer ilerler.

### 4.4 AlarmInterval ile Yuvarlama

```rust
// set_alarm() içinde:
let alarm_interval = T::AlarmInterval::get().max(One::one());
let when = (when + alarm_interval - 1) / alarm_interval * alarm_interval;
```

Bu formül `when`'i alarm_interval'ın bir sonraki katına yuvarlar (ceiling division). Örnek:
- `when = 107`, `alarm_interval = 10` → `(107 + 9) / 10 * 10 = 110`

Bu, scheduler'ın çok sık tetiklenmesini engeller.

### 4.5 make_reciprocal() — Compile-time Eğri Oluşturma

`types.rs:429-476` — Hiperbolik eğriyi `delay`, `level`, `floor`, `ceil` gibi anlaşılır parametrelerden oluşturur. Binary search ile doğru `factor` değerini bulur:

```rust
pub const fn make_reciprocal(delay: u128, period: u128, level: FixedI64, floor: FixedI64, ceil: FixedI64) -> Curve {
    // Binary search ile factor bul
    // Hedef: delay noktasında threshold = level
    // Aralık: floor..=ceil
}
```

Bu sayede runtime konfigüratörü matematiksel parametreler yerine semantik parametreler kullanabilir.

---

## KISIM 5: Nova Shield Uygulama Notu

### 5.1 Nova Shield Governance Sistemi Tasarım Önerileri

Nova Shield Chain için bir yönetişim sistemi tasarlarken bu iki pallet'ten çıkarılabilecek kritik dersler:

#### A. Conviction Voting — Aynen Alınabilecek Tasarım

Democracy'nin conviction mekanizması mükemmel ve test edilmiş bir tasarımdır:
- `None → 0.1x` (ceza değil düşük güç, zira kilit yok)
- `Locked1x-6x`: Linear çarpan, üstel kilit
- Lock unlock zamanı = `referendum_end + VoteLockingPeriod × lock_periods`

**Nova Shield için öneri:** Aynı parametreler kullanılabilir. Tek dikkat: `VoteLockingPeriod >= EnactmentPeriod` kuralı.

```rust
// Nova Shield conviction tablosu (öneri):
// None    → 0.1x oy, 0 kilit
// Locked1x → 1x oy, 1 hafta kilit
// Locked2x → 2x oy, 2 hafta kilit
// Locked3x → 3x oy, 1 ay kilit
// Locked4x → 4x oy, 2 ay kilit
// Locked5x → 5x oy, 4 ay kilit
// Locked6x → 6x oy, 8 ay kilit
```

#### B. OpenGov Track Sistemi — Uyarlanabilir

Nova Shield için minimal track seti önerim:

| Track ID | Ad | Origin | max_deciding | Decision Deposit |
|----------|-----|--------|--------------|-----------------|
| 0 | Root | Root | 1 | Yüksek |
| 1 | ParamChange | ParamChangeOrigin | 3 | Orta |
| 2 | Treasury | TreasuryOrigin | 5 | Düşük |
| 3 | EmergencyFix | TechnicalCommittee | 1 | Çok Yüksek |

#### C. Approval/Support Eğrisi Seçimi

**Root track için:** `Reciprocal` + `LinearDecreasing`
```
Approval: make_reciprocal(7, 28, 80%, 50%, 100%)
  → İlk 7 günde %80 gerekli, zamanla azalır
Support: make_linear(28, 28, 0%, 50%)
  → 0 günde %50, her gün azalır
```

**Treasury track için:** `LinearDecreasing` + `SteppedDecreasing`
```
Approval: make_linear(14, 28, 50%, 90%)
  → Başta %90, 14 günde %50'ye düşer
Support: SteppedDecreasing { begin: 30%, end: 5%, step: 5%, period: 10% }
  → Adım adım düşen destek eşiği
```

#### D. Onay Eşiği Seçimi — democracy vs referenda

**Nova Shield için tavsiyem: pallet-referenda tabanlı sistem**

Gerekçe:
- Paralel referandumlar daha hızlı governance döngüsü sağlar
- Track sistemi farklı konular için farklı standartlar tanımlar (token mint vs parametre değişikliği aynı eşiği gerektirmemeli)
- Conviction-voting ayrı pallet olduğu için ağ güncellemelerinde modüler
- Confirmation period, anlık oy patlamalarına karşı daha güvenli

#### E. Anti-Spam Mekanizmaları

Democracy'den alınacak:
- **Blacklist**: `Hash → (until_block, vetoers_list)` — tekrarlı spam önerilerini engeller
- **Submission deposit**: İade edilmemesi (rejected/timed-out) başlı başına caydırıcı

Referenda'dan alınacak:
- **Decision deposit**: Ayrı taraf ödeyebilir → topluluk isteyen önerileri aktive edebilir
- **UndecidingTimeout**: Karar depositosu ödenmezse öneri silinir → "ghost referendum" sorunu yok

#### F. Voting Lock Yönetimi

**Democracy yaklaşımı:** Tek pallet içinde `DEMOCRACY_ID` kilidi
**OpenGov yaklaşımı:** pallet-conviction-voting ayrı lock yönetir

Nova Shield için: Conviction voting'i ayrı pallet olarak implement edin (OpenGov yaklaşımı). Bu sayede governance sistemi değiştiğinde lock mantığı korunur.

```rust
// Nova Shield'de öncelik:
// 1. pallet-referenda (fork veya yeniden yazma)
// 2. pallet-conviction-voting (fork veya uyarlama)
// 3. Nova Shield custom tracks (TrackInfo implementasyonu)
// 4. Custom VoteTally trait implementasyonu
```

#### G. Kritik Edge Case'ler (Kaynak Koddan Öğrenilen)

1. **Cyclic delegation**: Democracy bunu fark etmez — sadece tek seviye delegasyon desteklenir. Nova Shield'de ya aynı kısıtlamayı benimseyin ya da delegasyon döngüsü tespiti ekleyin.

2. **PriorLock accumulation**: Delegasyon değiştirirken önceki lock'u sakla. Yoksa kullanıcılar lock'tan kaçmak için delegasyon değiştirebilir.

3. **TrackQueue overflow**: `force_insert_keep_right` — en düşük öncelikli silinir. Nova Shield'de `MaxQueued` dikkatli seçilmeli.

4. **Confirmation interruption**: Referandum passing state'ten çıkarsa confirmation sıfırlanır. Beklenen davranış mı? Nova Shield için belgelenip community'e açıklanmalı.

5. **compare_rationals overflow prevention**: Büyük balance değerleri (128-bit) için kesirsiz karşılaştırma algoritması zorunlu. Doğrudan `a * b > c * d` yapmayın.

#### H. Deposito Yönetimi — Kritik Tasarım Kararı

Democracy:
- Öneride depozito alınır, proposal launch'ta iade edilir

Referenda:
- `SubmissionDeposit`: Öneride → iade: Approved/Cancelled'da
- `DecisionDeposit`: Deciding başlamadan önce → iade: Approved/Rejected/Cancelled/TimedOut'da

**Nova Shield tavsiyesi:** Referenda modelini benimseyin. İki katmanlı depozito spamleri iki aşamada engeller.

---

## KISIM 6: Kaynak Kod Referansları

### pallet-democracy Kritik Satır Referansları

| Konu | Dosya | Satır |
|------|-------|-------|
| Conviction enum | conviction.rs | 44-59 |
| lock_periods() | conviction.rs | 100-110 |
| votes() — oy ağırlığı formülü | conviction.rs | 112-122 |
| Tally struct | types.rs | 36-43 |
| Delegations struct | types.rs | 46-52 |
| Tally::add() | types.rs | 104-123 |
| Vote encoding (1 byte) | vote.rs | 36-59 |
| AccountVote enum | vote.rs | 87-93 |
| PriorLock::accumulate() | vote.rs | 145-148 |
| Voting enum (Direct/Delegating) | vote.rs | 166-187 |
| VoteThreshold enum | vote_threshold.rs | 42-49 |
| compare_rationals() | vote_threshold.rs | 58-90 |
| approved() — üç algoritma | vote_threshold.rs | 103-118 |
| Config trait — tüm sabitler | lib.rs | 223-347 |
| Storage items | lib.rs | 349-439 |
| propose() | lib.rs | 586-618 |
| second() | lib.rs | 628-643 |
| vote() | lib.rs | 652-662 |
| emergency_cancel() | lib.rs | 672-687 |
| external_propose() | lib.rs | 697-711 |
| external_propose_majority() | lib.rs | 724-733 |
| external_propose_default() | lib.rs | 745-755 |
| fast_track() | lib.rs | 775-813 |
| veto_external() | lib.rs | 827-851 |
| delegate() | lib.rs | 872-906 |
| try_delegate() | lib.rs | 1421-1470 |
| try_undelegate() | lib.rs | 1475-1498 |
| increase_upstream_delegation() | lib.rs | 1371-1392 |
| reduce_upstream_delegation() | lib.rs | 1394-1416 |
| try_vote() | lib.rs | 1272-1317 |
| try_remove_vote() | lib.rs | 1325-1368 |
| update_lock() | lib.rs | 1502-1517 |
| inject_referendum() | lib.rs | 1519-1534 |
| launch_next() — alternating | lib.rs | 1537-1544 |
| launch_external() | lib.rs | 1547-1562 |
| launch_public() — max backing | lib.rs | 1565-1595 |
| bake_referendum() — tally+schedule | lib.rs | 1597-1627 |
| begin_block() — LaunchPeriod+bake | lib.rs | 1635-1681 |
| backing_for() | lib.rs | 1197-1199 |
| decode_compact_u32_at() | lib.rs | 1740-1753 |

### pallet-referenda Kritik Satır Referansları

| Konu | Dosya | Satır |
|------|-------|-------|
| TrackDetails struct | types.rs | 177-199 |
| TracksInfo trait | types.rs | 211-265 |
| ReferendumStatus struct | types.rs | 267-304 |
| ReferendumInfo enum (6 sonuç) | types.rs | 306-343 |
| take_decision_deposit() | types.rs | 359-370 |
| take_submission_deposit() | types.rs | 375-383 |
| Curve enum (3 tür) | types.rs | 388-401 |
| pos_quad_solution() | types.rs | 407-411 |
| make_linear() | types.rs | 418-423 |
| make_reciprocal() | types.rs | 429-465 |
| reciprocal_from_parts() | types.rs | 471-476 |
| threshold() — 3 eğri formülü | types.rs | 537-549 |
| delay() — inverse hesap | types.rs | 581-607 |
| passing() | types.rs | 610-612 |
| ServiceBranch enum | branch.rs | 31-48 |
| Config trait | lib.rs | 140-225 |
| Storage items | lib.rs | 257-292 |
| submit() | lib.rs | 474-520 |
| place_decision_deposit() | lib.rs | 533-550 |
| cancel() | lib.rs | 591-608 |
| kill() | lib.rs | 616-634 |
| nudge_referendum() | lib.rs | 643-656 |
| one_fewer_deciding() | lib.rs | 668-692 |
| Polling impl | lib.rs | 761-866 |
| ensure_ongoing() | lib.rs | 869-878 |
| is_referendum_passing() | lib.rs | 882-903 |
| schedule_enactment() | lib.rs | 905-927 |
| set_alarm() | lib.rs | 929-955 |
| begin_deciding() | lib.rs | 963-997 |
| ready_for_deciding() | lib.rs | 1003-1022 |
| next_for_deciding() | lib.rs | 1026-1036 |
| note_one_fewer_deciding() | lib.rs | 1038-1055 |
| ensure_alarm_at() | lib.rs | 1062-1086 |
| service_referendum() — ana state machine | lib.rs | 1109-1259 |
| decision_time() | lib.rs | 1263-1278 |
| is_passing() | lib.rs | 1318-1329 |

---

## ÖZET — En Kritik Bulgular

1. **Conviction formülü:** `None = balance/10`, `LockedNx = balance * N` — overflow için `checked_mul`/`checked_div` zorunlu; overflow'da `B::max_value()` döner.

2. **SuperMajorityApprove:** `nays/√turnout < ayes/√electorate` — düşük katılımda onay zordur, yüksek katılımda kolaylaşır.

3. **SuperMajorityAgainst:** `nays/√electorate < ayes/√turnout` — tam tersi semantik; varsayılan olarak geçer.

4. **compare_rationals():** Taşmasız kesir karşılaştırması — continued fraction algoritması, çarpma operasyonu yok.

5. **OpenGov iki boyutlu eşik:** `support.passing(x, actual) && approval.passing(x, actual)` — her ikisi aynı anda karşılanmalı.

6. **Confirmation period:** Kesintisiz olmalı — bir kez kesintiye uğrarsa sıfırdan başlar.

7. **TrackQueue:** Ayes sayısına göre sıralı; `pop()` en yüksek önceliklileri alır; taşmada en düşük olanlar düşer.

8. **Alarm yuvarlama:** `ceil(when / interval) * interval` — az sayıda I/O için quantization.

9. **Deposito semantiği:** Democracy = tek depozito (launch'ta iade), Referenda = iki depozito (farklı koşullarda iade).

10. **Delegasyon sınırı:** Tek seviye; zincir delegasyon hesaplanmaz (kasıtlı tasarım kararı).
