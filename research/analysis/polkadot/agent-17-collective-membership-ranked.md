# pallet-collective + pallet-membership + pallet-ranked-collective — Tam Analiz
**Agent**: 17 | **Tarih**: 2026-07-22

---

## BÖLÜM 1: pallet-collective

### 1.1 Proposal Lifecycle

Proposal yaşam döngüsü iki ana kola ayrılır:

**Kol A — Doğrudan Yürütme (threshold < 2):**
```
propose(threshold < 2) → do_propose_execute()
  → proposal.dispatch(RawOrigin::Members(1, seats).into())
  → Event::Executed emitted
```
- Tek üye (proposer) yeterli, threshold=1 ise anında execute edilir
- `do_propose_execute()`: lib.rs:913-935

**Kol B — Oylama (threshold >= 2):**
```
propose(threshold >= 2) → do_propose_proposed()
  → Proposals BoundedVec'e hash eklenir
  → Consideration::new() — deposit alınır (isteğe bağlı)
  → ProposalCount arttırılır
  → ProposalOf'a call eklenir
  → Voting storage'a oluşturulur: {index, threshold, ayes:[], nays:[], end: block + MotionDuration}
  → Event::Proposed emitted

vote(approve) → do_vote()
  → Üye ayes veya nays listesine eklenir
  → İlk oy → Pays::No (fee muafiyet!)
  → Event::Voted emitted

close(proposal_hash, index, weight_bound, length_bound) → do_close()
  → ERKEN ONAY: yes_votes >= threshold ise anında execute
  → ERKEN RED: (seats - no_votes) < threshold ise anında disapprove
  → Süre dolmuşsa: abstention sayılır, DefaultVote uygulanır
  → do_approve_proposal() veya do_disapprove_proposal()
```

**do_propose_proposed() — satır numarasıyla:**
- lib.rs:938-983
- `end = frame_system::Pallet::<T>::block_number() + T::MotionDuration::get()` — lib.rs:971

**do_close() — satır numarasıyla (persisted file):**
- `approved = yes_votes >= voting.threshold` — line 1050
- `disapproved = seats.saturating_sub(no_votes) < voting.threshold` — line 1051
- Abstentions hesabı: `abstentions = seats - (yes_votes + no_votes)` — line 1088
- DefaultVote'a göre abstentions ya yes ya no'ya eklenir — lines 1090-1092

---

### 1.2 Voting Threshold Tipleri

pallet-collective'de "threshold" doğrudan bir integer sayıdır (MemberCount), **enum tipi yoktur**. Threshold, `propose` çağrısında proposer tarafından belirlenir.

Farklı eşik tipleri, **EnsureOrigin implementasyonları** üzerinden ifade edilir:

| Tip | Struct | Koşul (kaynak) |
|-----|--------|----------------|
| En az N üye | `EnsureMembers<_, I, N>` | `n >= N` — lib.rs:1433 |
| N/D'den fazla | `EnsureProportionMoreThan<_, I, N, D>` | `n * D > N * m` — lib.rs:1472 |
| En az N/D | `EnsureProportionAtLeast<_, I, N, D>` | `n * D >= N * m` — lib.rs:1517 |
| Tek üye | `EnsureMember<_, I>` | `RawOrigin::Member(id)` — lib.rs:1393 |

---

### 1.3 Threshold Hesap Formülleri

**Karar anında (do_close içi):**

```
// Erken onay
approved = yes_votes >= threshold          // (persisted file line 1050)

// Erken red (imkansızlık)
disapproved = (seats - no_votes) < threshold  // (persisted file line 1051)

// Süre dolmuşsa abstentionları say
abstentions = seats - (yes_votes + no_votes)  // (persisted file line 1088)
if default_vote == true  => yes_votes += abstentions
if default_vote == false => no_votes += abstentions
approved = yes_votes >= threshold              // (persisted file line 1093)
```

**EnsureProportionMoreThan (n/m > N/D):**
```
n * D > N * m   // cross-multiply, integer aritmetik
```
Örnek: %50'den fazla için N=1, D=2: `n * 2 > 1 * m` yani `2n > m`

**EnsureProportionAtLeast (n/m >= N/D):**
```
n * D >= N * m
```
Örnek: 3/5 için N=3, D=5: `n * 5 >= 3 * m`

---

### 1.4 Prime Üye Mekanizması

**İki farklı strateji** — `DefaultVote` trait implementasyonu:

**PrimeDefaultVote** (lib.rs:112-121):
```rust
fn default_vote(prime_vote: Option<bool>, ...) -> bool {
    prime_vote.unwrap_or(false)
}
```
- Prime üye oy kullandıysa o yönde, kullanmadıysa `false` (nay) default.

**MoreThanMajorityThenPrimeDefaultVote** (lib.rs:127-137):
```rust
fn default_vote(prime_vote, yes_votes, _, len) -> bool {
    let more_than_majority = yes_votes * 2 > len;
    more_than_majority || prime_vote.unwrap_or(false)
}
```
- ÖNCE: evet oyu mutlak çoğunluğu geçtiyse (yes*2 > len) evet
- SONRA: prime üyenin oyu
- Sonuç: prime üye "tie-breaking" değil, "second-level default"

**Kullanım yeri (do_close içi):**
```rust
let prime_vote = Prime::<T, I>::get().map(|who| voting.ayes.iter().any(|a| a == &who));
let default = T::DefaultVote::default_vote(prime_vote, yes_votes, no_votes, seats);
```
(persisted file lines 1083-1086)

**Prime üye ayarlanınca ne olur:**
- `change_members_sorted()` çağrıldığında `Prime::kill()` çalışır (lib.rs:1346)
- Prime üye çıkarılırsa veya membership sıfırlanırsa prime silinir

---

### 1.5 MotionDuration ve Sabitler

Sabitler `Config` trait'i üzerinden runtime'a bırakılır (hardcoded değil):

| Sabit | Tip | Açıklama | Kaynak |
|-------|-----|----------|--------|
| `MotionDuration` | `BlockNumberFor<T>` | Oylama bitiş süresi (blok sayısı) | lib.rs:346 |
| `MaxProposals` | `ProposalIndex` (u32) | Paralel maksimum proposal sayısı | lib.rs:349 |
| `MaxMembers` | `MemberCount` (u32) | Maksimum üye sayısı (weight tahmini için) | lib.rs:356 |
| `MaxProposalWeight` | `Weight` | Proposal call'ının max ağırlığı | lib.rs:369 |

**Not:** `MaxMembers` weight tahmini içindir, kod bunu zorla uygulamaz. Gerçek üye listesi üzerinde hard limit yoktur. — lib.rs:354-356

**Oylama süresi hesabı:**
```rust
end = frame_system::Pallet::<T>::block_number() + T::MotionDuration::get()
```
(persisted file line 971)

---

### 1.6 Execution Mekanizması

**Başarılı proposal execute akışı (do_approve_proposal):**
```
do_approve_proposal(seats, yes_votes, proposal_hash, proposal)
  1. Event::Approved emitted
  2. origin = RawOrigin::Members(yes_votes, seats)
  3. proposal.dispatch(origin)
  4. Event::Executed emitted (result: Ok/Err)
  5. remove_proposal(hash):
     - ProposalOf::remove(hash)
     - Voting::remove(hash)
     - Proposals vec'ten hash çıkarılır
     - CostOf varsa serbest bırakılır (release_proposal_cost ayrı extrinsic)
```
(persisted file lines 1153-1173)

**Origin formatı:** `RawOrigin::Members(yes_votes, seats)` — dispatcher hangi oranla kabul edildiğini bilir.

**Disapprove akışı:**
- `do_disapprove_proposal(hash)` → Event::Disapproved → remove_proposal
- Close fee: disapprove ise `Pays::No` (ücretsiz!) — persisted file line 1075

**Kill (kötü niyetli proposal):**
- `kill()`: KillOrigin gerektirir, deposit yakar (burn, iade etmez) — lib.rs:850-856

---

### 1.7 Motion Hash Hesabı

```rust
let proposal_hash = T::Hashing::hash_of(&proposal);
```
- `T::Hashing` genellikle `BlakeTwo256`
- Tüm encode edilmiş proposal bytes'ının hash'i
- Hash key olarak kullanılır: ProposalOf, Voting, CostOf storage'larında
- Duplicate hash kontrolü: `ensure!(!ProposalOf::contains_key(proposal_hash), DuplicateProposal)` — persisted file line 953

---

### 1.8 Deposit / Consideration Sistemi (Yeni)

v4'te eklenen yenilik: `type Consideration: MaybeConsideration<AccountId, u32>`.

Proposal count'a göre ayarlanan deposit stratejileri (lib.rs deposit modülü):

| Strateji | Formül | Kaynak |
|----------|--------|--------|
| `Constant<D>` | `D` | lib.rs:205-208 |
| `Linear<Slope, Offset>` | `Slope * proposal_count + Offset` | lib.rs:219-222 |
| `Geometric<Ratio, Base>` | `Base * Ratio^proposal_count` | lib.rs:234-238 |
| `Round<Precision, D>` | round D to Precision digits | lib.rs:251-264 |
| `Stepped<Period, Step>` | step per Period proposals | lib.rs:275-278 |
| `Delayed<Delay, D>` | 0 until Delay, then D | lib.rs:289-295 |
| `WithCeil<Ceil, D>` | min(D, Ceil) | lib.rs:307-309 |

---

## BÖLÜM 2: pallet-membership

### 2.1 Üye Yönetimi

Tüm işlemler `Members<T, I>: StorageValue<_, BoundedVec<T::AccountId, T::MaxMembers>, ValueQuery>` üzerinde çalışır. Liste her zaman sıralı tutulur.

**add_member (call_index=0):**
```rust
binary_search(&who).err() → sıralı konum bulunur
try_insert(location, who) → MaxMembers sınırı
MembershipChanged::change_members_sorted(&[who], &[], &members)
```
lib.rs:177-198

**remove_member (call_index=1):**
```rust
binary_search(&who).ok() → konum bulunur
members.remove(location)
MembershipChanged::change_members_sorted(&[], &[who], &members)
Self::rejig_prime(&members)  // prime hala üye mi kontrol
```
lib.rs:205-223

**swap_member (call_index=2):**
- remove == add ise early return (noop)
- remove yoksa NotMember, add varsa AlreadyMember
- `members[location] = add; members.sort()` — lib.rs:249-250
- Prime swap ile transfer olmaz (comment'te belirtilmiş) — lib.rs:230
- `rejig_prime(&members)` ile prime kontrol edilir

**reset_members (call_index=3):**
```rust
BoundedVec::try_from(members) → TooManyMembers check
members.sort()
MembershipChanged::set_members_sorted(&members, old_m)
rejig_prime(&members)
```
lib.rs:267-280

**change_key (call_index=4):**
- Mevcut üye kendi anahtarını değiştirir (Signed origin)
- Prime ise yeni key'e prime taşınır — lib.rs:316-319

**set_prime (call_index=5) / clear_prime (call_index=6):**
- PrimeOrigin gerektirir
- set: üye olma kontrolü yapılır
- MembershipChanged::set_prime çağrılır — lib.rs:339/351

**rejig_prime (internal):**
```rust
fn rejig_prime(members: &[T::AccountId]) {
    if let Some(prime) = Prime::get() {
        match members.binary_search(&prime) {
            Ok(_) => MembershipChanged::set_prime(Some(prime)),
            Err(_) => Prime::kill(),  // prime artık üye değil
        }
    }
}
```
lib.rs:406-413

---

### 2.2 MembershipChanged Hook

`type MembershipChanged: ChangeMembers<Self::AccountId>` — lib.rs:95

**Ne zaman çağrılır:**

| Extrinsic | Çağrı |
|-----------|-------|
| add_member | `change_members_sorted(&[who], &[], &members)` |
| remove_member | `change_members_sorted(&[], &[who], &members)` |
| swap_member | `change_members_sorted(&[add], &[remove], &members)` |
| reset_members | `set_members_sorted(&members, old)` |
| change_key | `change_members_sorted(&[new], &[old], &members)` |
| set_prime | `set_prime(Some(who))` |
| clear_prime | `set_prime(None)` |

`ChangeMembers` trait'i 3 method tanımlar:
- `change_members_sorted(incoming, outgoing, new_sorted)` — eklenen/çıkarılanlar + yeni liste
- `set_members_sorted(new, old)` — tam sıfırlama
- `set_prime(prime: Option<AccountId>)`

---

### 2.3 Collective Entegrasyonu

**Tipik kullanım pattern'i:**

```toml
# Runtime konfigürasyonu
impl pallet_membership::Config for Runtime {
    type MembershipInitialized = TechnicalCommittee;   // genesis'te
    type MembershipChanged = TechnicalCommittee;        // her değişimde
    ...
}
```

pallet-collective, `ChangeMembers<T::AccountId>` trait'ini implement eder:

```rust
impl<T: Config<I>, I: 'static> ChangeMembers<T::AccountId> for Pallet<T, I> {
    fn change_members_sorted(_incoming, outgoing, new) {
        // 1. Outgoing üyelerin tüm aktif proposal oyları silinir
        for h in Proposals::get() {
            Voting::mutate(h, |v| {
                votes.ayes = votes.ayes.filter(|i| not in outgoing)
                votes.nays = votes.nays.filter(|i| not in outgoing)
            });
        }
        // 2. Members güncellenir
        Members::put(new);
        // 3. Prime sıfırlanır (!)
        Prime::kill();
    }
}
```
(persisted file lines 1312-1347)

**Kritik:** Üye değişiminde prime her zaman sıfırlanır. `pallet-membership`'in `rejig_prime` ile prime'ı koruduğu yerde collective pallet Prime'ı siler. Bu tasarımsal bir kopukluk değil: collective'in kendi `set_prime` hook'u membership'in `set_prime` çağrısıyla sonradan düzeltilir.

---

## BÖLÜM 3: pallet-ranked-collective

### 3.1 Rank Sistemi

**Temel tipler:**
```rust
pub type Rank = u16;          // lib.rs:78 — maksimum 65535 rank
pub type MemberIndex = u32;   // lib.rs:75
pub type Votes = u32;         // lib.rs:81
```

**MemberRecord:**
```rust
pub struct MemberRecord {
    rank: Rank,   // 0 = en düşük, artış birer birer
}
```
lib.rs:178-182

**Storage haritası:**
- `Members: StorageMap<AccountId, MemberRecord>` — lib.rs:474-476
- `MemberCount: StorageMap<Rank, MemberIndex>` — rank'ın üstündeki toplam üye sayısı — lib.rs:470-471
- `IdToIndex: StorageDoubleMap<Rank, AccountId, MemberIndex>` — O(1) random seçim için — lib.rs:480-481
- `IndexToId: StorageDoubleMap<Rank, MemberIndex, AccountId>` — reverse lookup — lib.rs:486-487

**Rank privilege kuralı:** Daha yüksek rank, daha düşük rank'taki tüm yoklamalara da oy kullanabilir (kümülatif ayrıcalık). — lib.rs:29-31

---

### 3.2 Oy Ağırlığı Formülü

**Temel fonksiyon:**
```rust
fn rank_to_votes(rank: Rank, min: Rank) -> Result<Votes, DispatchError> {
    let excess = rank.checked_sub(min).ok_or(Error::RankTooLow)?;
    Ok(T::VoteWeight::convert(excess))
}
```
lib.rs:762-765

`excess = voter_rank - min_rank_of_class` — yoklamanın gerektirdiği minimum rank'ın üzerindeki faz.

**Üç hazır VoteWeight şeması:**

**Unit (lib.rs:221-226):**
```rust
impl Convert<Rank, Votes> for Unit {
    fn convert(_: Rank) -> Votes { 1 }
}
```
Herkes 1 oy — rank fark etmez.

**Linear (lib.rs:236-241):**
```rust
impl Convert<Rank, Votes> for Linear {
    fn convert(r: Rank) -> Votes { (r + 1) as Votes }
}
```
- excess=0 → 1 oy
- excess=1 → 2 oy
- excess=2 → 3 oy
- excess=k → (k+1) oy

**Geometric (lib.rs:251-257):**
```rust
impl Convert<Rank, Votes> for Geometric {
    fn convert(r: Rank) -> Votes {
        let v = (r + 1) as Votes;
        v * (v + 1) / 2
    }
}
```
Triangular numbers:
- excess=0 → 1*(1+1)/2 = 1 oy
- excess=1 → 2*(2+1)/2 = 3 oy
- excess=2 → 3*(3+1)/2 = 6 oy
- excess=3 → 4*(4+1)/2 = 10 oy
- excess=4 → 5*(5+1)/2 = 15 oy

**Test'ten doğrulama** (tests.rs:419-432):
```
rank=1, min=1, excess=0 → Geometric → 1 oy → tally: (1, 1, 0) ✓
rank=2, min=1, excess=1 → Geometric → 3 oy → tally: (1, 3, 0) ✓
rank=3, min=1, excess=2 → Geometric → 6 oy → tally: (1, 6, 0) ✓
```

---

### 3.3 Promote/Demote Mekanizması

**Promote (do_promote_member, lib.rs:810-834):**
```rust
let rank = record.rank.checked_add(1).ok_or(Overflow)?;   // +1 birer birer
if let Some(max_rank) = maybe_max_rank {
    ensure!(max_rank >= rank, Error::NoPermission);         // yetki sınırı
}
let count = MemberCount::get(rank).checked_add(1).ok_or(Overflow)?;
if let Some(max) = T::MaxMemberCount::maybe_convert(rank) {
    ensure!(count <= max, Error::TooManyMembers);           // rank başına üst sınır
}
// Storage güncelleme: MemberCount, IdToIndex, IndexToId, Members
Members::insert(&who, MemberRecord { rank });
```

**Demote (do_demote_member, lib.rs:840-861):**
```rust
// rank=0'dan demote → tamamen kaldırılır
Self::remove_from_rank(&who, rank)?;
match rank.checked_sub(1) {
    None => {
        Members::remove(&who);
        Event::MemberRemoved { rank: 0 }
    },
    Some(new_rank) => {
        record.rank = new_rank;
        Members::insert(&who, &record);
        Event::RankChanged { rank: new_rank }
    }
}
```

**Yetki modeli (tests.rs'ten):**
- `PromoteOrigin` Success = max rank to which can promote
- `DemoteOrigin` Success = max rank from which can demote
- Üye kendi rank'ının 2 altındaki üyeleri promote edebilir (test config: `ReduceBy<ConstU16<2>>`)
- Üye kendi rank'ının 3 altındaki üyeleri demote edebilir

**remove_member (lib.rs:887-893):**
```rust
pub fn do_remove_member_from_rank(who: &T::AccountId, rank: Rank) -> DispatchResult {
    for r in 0..=rank {          // TÜM rank'lardan temizle
        Self::remove_from_rank(&who, r)?;
    }
    Members::remove(&who);
    Ok(())
}
```
Bu O(rank) — tek O(rank) olmayan operasyon budur (diğerleri O(1)).

**do_add_member_to_rank (lib.rs:865-875) — convenience method:**
```rust
pub fn do_add_member_to_rank(who, rank, emit_event) {
    Self::do_add_member(who.clone(), emit_event)?;   // rank 0'dan başla
    for _ in 0..rank {
        Self::do_promote_member(who.clone(), None, emit_event)?;
    }
}
```

---

### 3.4 Tally Yapısı

```rust
pub struct Tally<T, I, M: GetMaxVoters> {
    bare_ayes: MemberIndex,  // oy kullanan aye üye SAYISI (weight'siz)
    ayes: Votes,             // ağırlıklı toplam aye oyu
    nays: Votes,             // ağırlıklı toplam nay oyu
    dummy: PhantomData<...>,
}
```
lib.rs:97-102

**VoteTally trait implementasyonu:**
```rust
fn ayes(&self, _: Class) -> Votes { self.bare_ayes }          // RAW sayı (support için)
fn support(&self, class: Class) -> Perbill {
    Perbill::from_rational(self.bare_ayes, M::get_max_voters(class))  // destek oranı
}
fn approval(&self, _: Class) -> Perbill {
    Perbill::from_rational(self.ayes, 1.max(self.ayes + self.nays))   // onay oranı
}
```
lib.rs:128-136

**İki farklı metrik:**
- **Support** = `bare_ayes / max_voters` — kaç üye oy kullandı (weight'siz)
- **Approval** = `ayes / (ayes + nays)` — kullananların ağırlıklı onay yüzdesi

**GetMaxVoters:**
```rust
impl GetMaxVoters for Pallet<T, I> {
    fn get_max_voters(c: Class) -> MemberIndex {
        MemberCount::get(T::MinRankOfClass::convert(c))
    }
}
```
lib.rs:266-271 — Yoklamanın class'ının minimum rank'ına sahip üye sayısı.

**Oy silme işlemi (vote fonksiyonu):**
```rust
match Voting::get(&poll, &who) {
    Some(Aye(votes)) => {
        tally.bare_ayes.saturating_dec();
        tally.ayes.saturating_reduce(votes);
    },
    Some(Nay(votes)) => tally.nays.saturating_reduce(votes),
    None => pays = Pays::No,   // ilk oy → ücretsiz
}
```
lib.rs:650-657

---

### 3.5 Polls Entegrasyonu

```rust
type Polls: Polling<TallyOf<Self, I>, Votes = Votes, Moment = BlockNumberFor<Self>>;
```
lib.rs:435

**Polling trait:** External yoklama sistemi (örn. pallet-referenda) ile entegrasyon.

**MinRankOfClass:**
```rust
type MinRankOfClass: Convert<ClassOf<Self, I>, Rank>;
```
lib.rs:440 — Yoklama class'ını minimum rank'a dönüştürür.

**Örnek (tests.rs:144-149):**
```rust
pub struct MinRankOfClass<Delta>(PhantomData<Delta>);
impl<Delta: Get<Rank>> Convert<Class, Rank> for MinRankOfClass<Delta> {
    fn convert(a: Class) -> Rank {
        a.saturating_sub(Delta::get())
    }
}
```
`MinRank(Class) = Class - Delta` — delta ile ayarlanabilir.

**Oy kullanma akışı:**
```
vote(poll_index, aye)
  1. ensure_member(&who) → MemberRecord{rank}
  2. T::Polls::try_access_poll(poll) → tally mutable ref
  3. Eski oy varsa geri al (tally'den düş)
  4. min_rank = T::MinRankOfClass::convert(class)
  5. votes = rank_to_votes(rank, min_rank) → T::VoteWeight::convert(excess)
  6. Tally güncelle
  7. Voting storage'a ekle
  8. Event::Voted emitted
```

---

### 3.6 Storage Yapısı — Rank Index Detayı

**remove_from_rank (lib.rs:767-783) — swap-and-pop O(1):**
```rust
MemberCount::try_mutate(rank, |last_index| {
    last_index.saturating_dec();
    let index = IdToIndex::get(rank, &who)?;
    if index != *last_index {
        let last = IndexToId::get(rank, *last_index)?;
        IdToIndex::insert(rank, &last, index);   // son elemanı boşluğa taşı
        IndexToId::insert(rank, index, &last);
    }
    IdToIndex::remove(rank, who);
    IndexToId::remove(rank, last_index);
})
```
Bu yapı random sampling için kritik: O(1)'de rastgele üye seçimi yapılabilir.

**MaxMemberCount (lib.rs:460-461):**
```rust
type MaxMemberCount: MaybeConvert<Rank, MemberIndex>;
```
Her rank için ayrı maksimum üye sayısı tanımlanabilir. `None` = sınırsız.

---

### 3.7 exchange_member (lib.rs:722-746)

```rust
pub fn exchange_member(origin, who, new_who) {
    T::ExchangeOrigin::ensure_origin(origin)?;
    ensure!(who != new_who, Error::SameMember);
    let MemberRecord { rank, .. } = Self::ensure_member(&who)?;
    Self::do_remove_member_from_rank(&who, rank)?;
    Self::do_add_member_to_rank(new_who.clone(), rank, false)?;
    T::MemberSwappedHandler::swapped(&who, &new_who, rank);
}
```
Hesap değişimi, rank korunur. `emit_event=false` çünkü `MemberExchanged` özel event.

---

## Kritik Bulgular Özeti

1. **Threshold integer'dır, enum değil** — Collective'de threshold `MemberCount` (u32), hardcoded threshold type yok. Eşik politikası runtime konfigürasyonuna bırakılmış. — collective lib.rs:697

2. **Erken kapatma formülü** — Onay: `yes >= threshold`. Red: `(seats - no_votes) < threshold` (yani evet ulaşılamaz). Bu ikisi `close` dönemini beklemeden çalışır. — persisted file lines 1050-1051

3. **PrimeDefaultVote vs MoreThanMajorityThenPrimeDefaultVote** — İki hazır strateji. Prime üye mutlak yoklama çoğunluğunu geçemiyorsa belirleyici, geçiyorsa çoğunluk kazanır. — collective lib.rs:112-137

4. **Abstentions prime'a göre dağıtılır** — Oylamadan çekinen üyelerin oyları DefaultVote tarafından atanır. `abstentions = seats - yes - no`. — persisted file lines 1088-1092

5. **İlk oy ücretsiz** — `is_account_voting_first_time=true` ise `Pays::No`. Hem collective hem ranked-collective'de aynı kural. — collective lib.rs:754-755, ranked lib.rs:656

6. **Collective MaxMembers zorlanmaz** — Sadece weight tahmini için kullanılır. Gerçek listenin sınırı yoktur (log::error yazılır). — collective lib.rs:585-592

7. **Geometric vote weight formülü** — `v*(v+1)/2` (triangular numbers), `v = excess + 1`. Excess=2 → 6 oy, excess=4 → 15 oy. — ranked lib.rs:254-256

8. **Support vs Approval ayrımı** — `bare_ayes/max_voters` ve `ayes/(ayes+nays)`. Support oy kullanım oranını, approval ağırlıklı onay oranını verir. — ranked lib.rs:131-136

9. **remove_from_rank swap-and-pop** — O(1) silme, son eleman boşluğa taşınır. Bu yapı `get_max_voters` ile birlikte O(1) random member selection sağlar. — ranked lib.rs:767-783

10. **Membership'te prime swap ile taşınmaz** — `swap_member` prime'ı korumaz (remove üye swap edilince prime silinebilir). `change_key` ise prime'ı taşır. — membership lib.rs:230, 316-319

11. **Collective ChangeMembers'da Prime::kill()** — Üye listesi değiştiğinde prime her zaman sıfırlanır. Ardından MembershipChanged hook yeniden set edebilir. — collective lib.rs:1346

12. **Deposit/Consideration sistemi** — Proposal sayısına göre lineer/geometrik/stepped deposit stratejileri. Kill: deposit yakar; Normal close: serbest bırakır. — collective lib.rs:192-311

13. **Storage version 4** — Her iki pallet (collective, membership) STORAGE_VERSION=4. — collective lib.rs:320, membership lib.rs:61

14. **Promote/demote birer birer** — Bir işlemde sadece ±1 rank. `do_add_member_to_rank` loop ile N kez promote eder. — ranked lib.rs:816, 870-874

15. **MemberExchanged yeni event** — `exchange_member` ile hesap değişimi; rank korunur, eski hesap tamamen silinir ve tüm rank storage'larında yeni hesap ile değiştirilir. — ranked lib.rs:722-746

---

## Nova Shield Chain Notları

### Governance Kurulu Tasarımı İçin Tavsiyeler

**1. pallet-collective — Basit Komite Yapısı**
- Teknik Komite, Güvenlik Komitesi gibi sabit üyeli orglar için ideal
- `MoreThanMajorityThenPrimeDefaultVote` ile prime üye tie-breaking sağlar
- `MaxProposals=20-50`, `MotionDuration=7200 blok (~1 gün @ 12s)` gibi değerler önerilir
- `Consideration` ile kötü niyetli proposal spam'i önlemek için deposit sistemi kurulabilir
- `DisapproveOrigin` ile Root/Council belirli proposal'ları zaman aşımına uğramadan temizleyebilir
- `KillOrigin` ile spam veya zararlı proposal'ların depositi yakılabilir

**2. pallet-membership — Dinamik Üyelik**
- pallet-collective ile birlikte kullanılır: membership değişikliklerini collective'e aktarır
- `AddOrigin`, `RemoveOrigin`, `SwapOrigin`, `ResetOrigin` ayrı origin'ler — granüler yetki denetimi
- `PrimeOrigin` ile prime üye atama yetkisi ayrılabilir (örn. sadece Root veya SuperMajority)
- `MaxMembers` bounded, genesis'te duplicate kontrolü zorunlu

**3. pallet-ranked-collective — Teknik Liyakat Sistemi**
- RFC (Request for Comments) tarzı teknik governance için ideal
- `Geometric` VoteWeight → Üst rank'ların etkisi üstel artar — "az sayıda uzman çok daha etkili"
- `Linear` VoteWeight → Daha dengeli güç dağılımı
- `Unit` VoteWeight → Tüm ranklar eşit → pure headcount voting
- `MinRankOfClass` ile "hangi rank hangi kararı oy kullanabilir" konfigüre edilir
- `MaxMemberCount` per-rank ile her katmanda üye sayısı kontrol edilir (piramit yapı)

**4. Önerilen Nova Shield Mimarisi:**
```
Root (Sudo / Multisig başlangıçta)
  ↓
TechnicalCommittee (pallet-collective + pallet-membership)
  → Ağ parametresi değişiklikleri
  → Acil pause/resume
  ↓
SecurityCouncil (pallet-ranked-collective, Geometric weight)
  → Validator rotasyonu önerileri
  → Protokol güncellemeleri (RFC style)
  ↓
PublicReferenda (pallet-referenda + pallet-conviction-voting)
  → Token ağırlıklı genel kararlar
```

**5. Kritik Tasarım Uyarıları:**
- pallet-collective'deki `change_members_sorted` tüm aktif oyları günceller → Üye değişiminde büyük O(P*M) maliyet
- pallet-ranked-collective `remove_member` O(rank) → Yüksek rank üyeleri kaldırmak pahalı
- Ranked-collective'de oy kayıtları poll bitince temizlenmez; `cleanup_poll` extrinsic çağrılmalı
- Her iki pallet de instance desteği sunar (I: 'static) → Aynı chain'de birden fazla collective mümkün
