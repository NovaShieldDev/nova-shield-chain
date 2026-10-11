# Agent-24: pallet-sudo + pallet-recovery + pallet-society + Misc Pallet Analizi

## Kapsanan Dosyalar

| Pallet | Dosyalar |
|---|---|
| pallet-sudo | src/lib.rs, src/extension.rs, src/tests.rs, src/weights.rs |
| pallet-recovery | src/lib.rs, src/types.rs, src/migrations/v0.rs, src/migrations/v1.rs, src/weights.rs |
| pallet-society | src/lib.rs (~2265 satir), src/migrations.rs |
| pallet-lottery | src/lib.rs, src/tests.rs |
| pallet-root-offences | src/lib.rs |

---

## 1. pallet-sudo

**Kaynak:** `substrate/frame/sudo/src/lib.rs` + `src/extension.rs`

### 1.1 Storage Items

```rust
// substrate/frame/sudo/src/lib.rs:333
pub type Key<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;
```

- `Key<T>`: Tek bir `Option<AccountId>`. Sudo anahtari bu storage'da tutulur. Eger `None` ise sudo hesabi kaldirilmis demektir.
- `GenesisConfig` ile initialize edilir (lib.rs:337-347).

### 1.2 Extrinsicler ve Origin Kontrolu

#### `sudo(origin, call)` — call_index: 0
- **lib.rs:202-213**
- Origin: `ensure_signed_or_root` — imzali VEYA root kabul eder.
- `ensure_sudo()` ile caller'in `Key<T>` ile eslesmesi kontrol edilir.
- Call'i `dispatch_bypass_filter(RawOrigin::Root.into())` ile calistirir — filter bypass edilir, dogrudan Root origin verilir.
- `Pays::No` — sudo hesabi hic fee odemez.
- `Sudid { sudo_result }` eventi emit edilir; call basarili olsa da olmasa da event gelir.

#### `sudo_unchecked_weight(origin, call, weight)` — call_index: 1
- **lib.rs:222-235**
- Origin: `ensure_signed_or_root`
- `weight` parametresi tamamen goz ardi edilir (`let _ = weight;`).
- Kullanimamasi gereken durumlar: cok agir migration call'lari veya chain'in ilk bootstrap asamasinda block weight limitini bypass etmek gerektiginde. Yanlis agirlik gecirilerek blok dolup tikanabilir — tehlikeli.
- `dispatch_bypass_filter(RawOrigin::Root.into())` ile calisir.
- `Pays::No`.

#### `set_key(origin, new)` — call_index: 2
- **lib.rs:241-253**
- Origin: mevcut sudo key (ensure_sudo).
- Eski key **hemen** etkisini kaybeder; `Key<T>` yeni degerle overwrite edilir.
- `KeyChanged { old: Key::<T>::get(), new: new.clone() }` eventi emit edilir.
- Eski key bir sonraki blocktaki transaction'larda `RequireSudo` hatasi alir.
- Eski key kendisini tekrar set edemez (cunku artik sudo key o degil).

#### `sudo_as(origin, who, call)` — call_index: 3
- **lib.rs:267-282**
- Origin: sudo key.
- `dispatch_bypass_filter(RawOrigin::Signed(who).into())` — Root degil, `who` hesabinin Signed origin'i ile calistirir.
- `sudo` farkli: `sudo` root origin verir, `sudo_as` baska bir hesabin Signed origin'ini taklit eder.
- `SudoAsDone { sudo_result }` eventi emit edilir.
- `Pays::No`.

#### `remove_key(origin)` — call_index: 4
- **lib.rs:289-297**
- Origin: mevcut sudo key.
- `Key::<T>::kill()` — sudo storage tamamen silinir. Bu islemi **geri alinamaz** (once remove_key'i once bir sudo call ile geri getiremezsiniz, cunku artik sudo yok).
- `KeyRemoved` eventi emit edilir.

### 1.3 `ensure_sudo` Implementasyonu

```rust
// lib.rs:351-363
pub(crate) fn ensure_sudo(origin: OriginFor<T>) -> DispatchResult {
    let sender = ensure_signed_or_root(origin)?;
    if let Some(sender) = sender {
        if Key::<T>::get().map_or(false, |k| k == sender) {
            Ok(())
        } else {
            Err(Error::<T>::RequireSudo.into())
        }
    } else {
        Ok(()) // Root origin dogrudan gecebilir
    }
}
```

- `ensure_signed_or_root` kullanilir: `None` = Root, `Some(account)` = Signed.
- Root origin, Key kontrolunu bypass eder — bu kasitli bir tasarim.

### 1.4 `CheckOnlySudoAccount` TransactionExtension

**Kaynak:** `src/extension.rs`

- `IDENTIFIER: "CheckOnlySudoAccount"` (extension.rs:77).
- `validate()` metodu:
  - Sadece Signed transaction'lari kabul eder (`BadSigner` hatasi unsigned icin).
  - Sudo key mevcut degilse `UnknownTransaction::CannotLookup` dondurir.
  - `who != sudo_key` ise `InvalidTransaction::BadSigner`.
  - Gecerli ise priority = `info.total_weight().ref_time()` olarak belirlenir.
  - `provides: vec![(who, hash_of(call)).encode()]` — tekrarlayan call'lari mempool'dan filtreler.
- **Amac:** Chain'in ilk asamasinda sadece sudo hesabinin transaction gondermesine izin verir; diger hesaplar mempool'u spam yapamaz.

### 1.5 Hata Tipleri

```rust
// lib.rs:326-329
pub enum Error<T> {
    RequireSudo,  // Sadece tek hata var
}
```

### 1.6 Events

| Event | Ne Zaman |
|---|---|
| `Sudid { sudo_result }` | `sudo` veya `sudo_unchecked_weight` cagrisinin ardindan |
| `KeyChanged { old, new }` | `set_key` sonrasinda |
| `KeyRemoved` | `remove_key` sonrasinda |
| `SudoAsDone { sudo_result }` | `sudo_as` sonrasinda |

### 1.7 Kritik Tasarim Notlari

- Sudo hesabi hicbir call icin fee odemiyor (`Pays::No`).
- `dispatch_bypass_filter` kullanilir — SafeMode, TxPause gibi pallet filtreleri bypass edilir. Bu kasitli.
- Sudo, root origin gerektiren fakat extrinsic olarak cagirilmasi gerekli olan tum call'lar icin bir gecisin kapidir.
- Uretim zincirlerinde sudo kaldirilmali veya governance'a devredilmeli.

### 1.8 Test Coverage

`src/tests.rs` (315 satir):
- `sudo_basics` — dogru/yanlis key ile sudo call
- `sudo_unchecked_weight_basics` — weight bypass
- `set_key_basics` — key degisimi
- `sudo_as_emits_events_correctly` — sudo_as akisi
- `key_removal_works` — key kaldirilmasi
- `sudo_with_none_key_should_fail` — key yokken sudo
- `remove_key_works` — remove_key sonrasi sudo fail

---

## 2. pallet-recovery

**Kaynak:** `substrate/frame/recovery/src/lib.rs` (~1042 satir), `src/types.rs`

**Not:** Bu, orijinal recovery pallet'inin onemli olcude yeniden yazilmis modern versiyonudur. Eski versiyon (v0/v1 migration) cok daha basitti. Yeni versiyon:
- Birden fazla friend group destekler
- Priority sistemi ile inheritance conflict cozumu yapar
- Bitfield ile oylama takibi yapar
- SecurityDeposit sistemi ile malicious recovery'e karsi koruma sunar

### 2.1 Sabitler

```rust
// lib.rs:129
pub const MAX_GROUPS_PER_ACCOUNT: u32 = 10;
```

### 2.2 Storage Items

```rust
// lib.rs:354-383
pub type FriendGroups<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId,
    (FriendGroupsOf<T>, T::FriendGroupsConsideration)>;

pub type Attempt<T: Config> = StorageDoubleMap<_, Blake2_128Concat, T::AccountId,
    Blake2_128Concat, FriendGroupIndex,
    (AttemptOf<T>, AttemptTicketOf<T>, SecurityDepositOf<T>)>;

pub type Inheritor<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId,
    (InheritancePriority, T::AccountId, InheritorTicketOf<T>)>;
```

- `FriendGroups`: `lost_account -> (groups, storage_consideration)`
  - Ongoing attempt varken degistirilemez (`HasOngoingAttempts` hatasi).
  - `MAX_GROUPS_PER_ACCOUNT = 10` limiti.
- `Attempt`: `(lost_account, friend_group_index) -> (attempt_data, ticket, security_deposit)`
  - `security_deposit` initiator'dan hold edilir.
- `Inheritor`: `lost_account -> (priority, inheritor_account, ticket)`
  - Bir hesap en fazla bir inheritor'a sahip olabilir, ancak daha yuksek priority bir grup onu replace edebilir.

### 2.3 Hold Reasons

```rust
// lib.rs:386-402
pub enum HoldReason {
    FriendGroupsStorage,  // index 0 — friend group konfigurasyonu icin storage deposit
    AttemptStorage,       // index 1 — devam eden attempt icin storage deposit
    InheritorStorage,     // index 2 — inheritor kaydi icin storage deposit
    SecurityDeposit,      // index 3 — guvenlik depositi (slash edilebilir)
}
```

### 2.4 FriendGroup Yapisi

```rust
// lib.rs:156-187
pub struct FriendGroup<ProvidedBlockNumber, AccountId, Friends> {
    pub friends: Friends,              // BoundedVec<AccountId, MaxFriendsPerConfig> — sirali, tekrarsiz
    pub friends_needed: u32,           // threshold — kac arkadasin onaylamasi gerekiyor
    pub inheritor: AccountId,          // hesabi kim devraliyor
    pub inheritance_delay: ProvidedBlockNumber,  // minimum bekleme suresi (block olarak)
    pub inheritance_priority: InheritancePriority, // 0 = en yuksek oncelik
    pub cancel_delay: ProvidedBlockNumber, // en az 1 olmali (NoCancelDelay hatasi)
}
```

- `InheritancePriority`: `u32`, dusuk deger = yuksek oncelik (`0` en guclu).
- `cancel_delay`: En az 1 olmali — initiator'in slash'tan kacmak icin mempool izlemesini onler.

### 2.5 Recovery Akisi: Initiate → Approve → Finish

#### `initiate_attempt(origin, lost, friend_group_index)` — call_index: 3
- **lib.rs:687-745**
- Origin: Signed (friend olmali)
- Kontroller:
  - Attempt zaten baslatilmamis olmali (`AlreadyInitiated`).
  - Caller `friend_group.friends` listesinde olmali (`NotFriend`).
  - Eger lost account zaten daha yuksek veya esit oncelikli bir inheritor'a sahipse `HigherPriorityRecovered` hatasi.
- Initiator'in oyu otomatik olarak eklenir (ilk approval).
- `SecurityDeposit::get()` kadar hold edilir.
- Storage ticket + security deposit kaydedilir.
- `AttemptInitiated` + `AttemptApproved` eventi emit edilir.

#### `approve_attempt(origin, lost, friend_group_index)` — call_index: 4
- **lib.rs:754-788**
- Origin: Signed (friend olmali)
- Bitfield ile hangi arkadasin oy verdigi takip edilir.
- `friends_voted >= friends_needed` ise `AlreadyApproved` hatasi — threshold dolduktan sonra ek oy kabul edilmez.
- `last_approval_block` guncellenir — bu `cancel_delay` hesaplamasi icin kullanilir.
- Attempt'in boyutu static oldugu icin ticket guncellenmez.

#### `finish_attempt(origin, lost, friend_group_index)` — call_index: 5
- **lib.rs:794-873**
- Origin: Herhangi bir Signed hesap.
- `AttemptTicket` drop edilir, security deposit serbest birakilir.
- Kontroller:
  - `approvals >= friends_needed` — `NotApproved` hatasi.
  - `now >= init_block + inheritance_delay` — `NotYetInheritable` hatasi.
- Inheritor belirlemesi:
  - `Inheritor` yoksa: yeni inheritor kaydi olusturulur, caller inheritor ticket'i ozer.
  - `Inheritor` varsa ve yeni priority < eski priority (daha yuksek oncelik): mevcut inheritor replace edilir.
  - `Inheritor` varsa ve yeni priority >= eski: `AttemptDiscarded` eventi emit edilir, inheritor degismez.

### 2.6 Diger Extrinsicler

#### `cancel_attempt(origin, lost, friend_group_index)` — call_index: 6
- **lib.rs:881-921**
- Origin: Initiator VEYA lost account kendisi.
- Lost account (kendisi) istediği zaman iptal edebilir.
- Initiator: `last_approval_block + cancel_delay` gecmis olmali (`NotYetCancelable`).
- Security deposit initiator'a iade edilir.

#### `slash_attempt(origin, friend_group_index)` — call_index: 7
- **lib.rs:928-943**
- Origin: Lost account (kendisi).
- Security deposit slashlenir ve `T::Slash` handler'ina verilir (burn veya treasury).
- `AttemptSlashed` eventi emit edilir.

#### `control_inherited_account(origin, recovered, call)` — call_index: 0
- **lib.rs:567-600**
- Origin: Inheritor.
- Reentrancy guard: Recovery pallet call'lari filtrelenir (inheritor recovery konfigurasyonunu bozamaz).
- Call dispatch edilir ve sonuc eventi ile birlikte emit edilir.
- **ONEMLI:** Caller'in izni varsa call basarili olsa da olmasa da `Ok(())` dondurur.

#### `revoke_inheritor(origin)` — call_index: 1
- **lib.rs:608-619**
- Origin: Lost account (kendisi — hesabina tekrar eriştikten sonra).
- Inheritor kaydi silinir ve ticket drop edilir.

#### `set_friend_groups(origin, friend_groups)` — call_index: 2
- **lib.rs:630-673**
- Origin: Lost account (henuz erişimi varken konfigüre eder).
- Bos liste gonderilirse tum friend group'lar silinir.
- Ongoing attempt varken degistirilemez.

### 2.7 Deposit Mekanizmalari

| Deposit Tipi | Kim Oduyor | Ne Zaman Serbest |
|---|---|---|
| `FriendGroupsStorage` | Lost account | `set_friend_groups` ile guncelleme/silme |
| `AttemptStorage` | Initiator | `finish_attempt` veya `cancel_attempt` |
| `InheritorStorage` | `finish_attempt` caller'i | `revoke_inheritor` veya replace |
| `SecurityDeposit` | Initiator | `finish_attempt` (serbest) veya `slash_attempt` (burn) |

- Deposit miktarlari `Consideration` trait'i uzerinden hesaplanir — sabit degerler yerine footprint bazli.
- `T::SecurityDeposit: Get<BalanceOf<Self>>` — sabit bir deger, konfigurasyonla belirlenir.

### 2.8 Bitfield Implementasyonu

```rust
// src/types.rs:33-99
pub struct Bitfield<MaxEntries: Get<u32>>(pub BoundedVec<u16, BitfieldLenOf<MaxEntries>>);
```

- Her u16, 16 arkadas icin 1 bit icerir.
- `MaxFriendsPerConfig` adet arkadasi destekler, boyut = `ceil(MaxFriendsPerConfig / 16)` adet u16.
- `set_if_not_set(index)`: Bit daha once set edilmisse `Err(())` dondurur — double-vote koruması.
- `count_ones()`: Toplam onayli friend sayisi.

### 2.9 Max Friend Sayisi

- `T::MaxFriendsPerConfig: Get<u32>` — runtime konfigurasyonla belirlenir.
- `integrity_test` ile `MaxFriendsPerConfig > 0` ve bitfield en az 1 u16 icerdiginden emin olunur.

### 2.10 Kurtarma Sirasinda Hesap Bakiyesi

- Recovery sureci **bakiyeyi dokunmaz** — sadece `Inheritor` storage kaydi olusturulur.
- Bakiye ancak `control_inherited_account` ile bir call dispatch ederek tasinabilir.
- Staked/locked bakiyeler aninda devredilmez; yalnizca serbest bakiye hemen transfer edilebilir.

### 2.11 Kritik Tasarim Notlari

- **cancel_delay >= 1 zorunlu:** Initiator'in malicious cancel'i (slash'tan kacmak) engellemek icin.
- **Ayni friend seti olan iki group yaratılamaz** (`DuplicateFriendGroups` hatasi) — lib.rs:1024-1027.
- **Lost account kendi friend group'unda olamaz** (`LostAccountInFriendGroup`).
- **Her initiation icin SecurityDeposit:** Sifir maliyetli spam initiation'lari engeller.
- **finish_attempt herkese acik:** Initiator veya inheritor olmak zorunda degil; herhangi biri odemek karsiliginda bitirebilir.

### 2.12 Events

| Event | Ne Zaman |
|---|---|
| `AttemptInitiated` | initiate_attempt sonrasi |
| `AttemptApproved` | initiate_attempt (ilk oy) veya approve_attempt sonrasi |
| `AttemptFinished` | finish_attempt — inheritor belirlendi |
| `AttemptDiscarded` | finish_attempt — daha yuksek oncelikli inheritor zaten var |
| `AttemptSlashed` | slash_attempt sonrasi |
| `AttemptCanceled` | cancel_attempt sonrasi |
| `FriendGroupsChanged` | set_friend_groups — yeni != eski durumunda |
| `InheritorRevoked` | revoke_inheritor sonrasi |
| `RecoveredAccountControlled` | control_inherited_account sonrasi |

### 2.13 Error Listesi

```
AlreadyApproved, AlreadyInitiated, AlreadyVoted, HasOngoingAttempts,
LostAccountInFriendGroup, HigherPriorityRecovered, NoCancelDelay,
NoFriendGroups, NoFriends, NoInheritor, NotApproved, NotAttempt,
NotCanceller, NotFriend, NotFriendGroup, NotInheritor,
NotYetCancelable, NotYetInheritable, TooManyFriendGroups,
TooManyFriendsNeeded, NoFriendsNeeded, FriendsNotSortedOrUnique,
DuplicateFriendGroups
```

---

## 3. pallet-society

**Kaynak:** `substrate/frame/society/src/lib.rs` (~2265 satir)

### 3.1 Genel Bakis

Society pallet, uyelik bazli bir ekonomik oyundur. Oyuncular:
- **Bidder:** Giris teklifi vermis kullanici
- **Candidate:** Oy verilecek aday
- **Member:** Aktif uye
- **Suspended Member:** Cok fazla strike almis uye
- **Head:** Suspension'dan muaf uye
- **Defender:** Uyeligini savunmasi gereken rastgele secilmis uye

Pallet, iki farkli Instance destekler (`I: 'static = ()`).

### 3.2 Config Parametreleri

```rust
// lib.rs:494-552
type PalletId: Get<PalletId>;                    // Pallet hesap ID'si
type Currency: ReservableCurrency;               // Reserve edilebilir para birimi
type Randomness: Randomness;                     // Rastgelelik kaynagi
type GraceStrikes: Get<u32>;                     // Bu kadar strike'ta yarim slash
type PeriodSpend: Get<BalanceOf>;                // Her rotation periyodunda pot'a eklenen miktar
type VotingPeriod: Get<BlockNumber>;             // Oylama suresi (block sayisi)
type ClaimPeriod: Get<BlockNumber>;              // Membership claim suresi
type MaxLockDuration: Get<BlockNumber>;          // Maksimum payout kilit suresi
type FounderSetOrigin: EnsureOrigin;            // found_society icin gerekli origin
type ChallengePeriod: Get<BlockNumber>;          // Membership challenge arasi
type MaxPayouts: Get<u32>;                       // Uye basina max bekleyen odeme
type MaxBids: Get<u32>;                          // Maksimum bid sayisi
type BlockNumberProvider: BlockNumberProvider;   // Block numarasi saglayici
```

### 3.3 Storage Items

```rust
// lib.rs:679-793
Parameters: Option<GroupParams>    // max_members, max_intake, max_strikes, candidate_deposit
Pot: Balance                       // Bu rotation icin ayrilan toplam
Founder: Option<AccountId>         // Toplumu kuran uye
Head: Option<AccountId>            // Mevcut bas uye (suspend muaf)
Rules: Option<Hash>                // Uyelik kurallarinin hash'i (degismez)
Members: Map<AccountId, MemberRecord>  // rank, strikes, vouching, index
Payouts: Map<AccountId, PayoutRecord>  // Ödeme kayitlari (yalnizca rank-0 uyeleri icin)
MemberCount: u32                   // Aktif uye sayisi
MemberByIndex: Map<u32, AccountId> // Rastgele secim icin index-based erisim
SuspendedMembers: Map<AccountId, MemberRecord>
RoundCount: u32                    // Toplam tur sayisi
Bids: BoundedVec<Bid, MaxBids>     // Deger sirasina gore sirali bid listesi
Candidates: Map<AccountId, Candidacy>
Skeptic: Option<AccountId>         // Bu turdaki kuseyci uye
Votes: DoubleMap<Candidate, Voter, Vote>
VoteClearCursor: Map<Candidate, Cursor>   // Temizlik icin imle
NextHead: Option<IntakeRecord>            // Bir sonraki head icin aday
ChallengeRoundCount: u32                  // Challenge tur sayaci
Defending: Option<(defender, skeptic, Tally)>  // Mevcut savunmaci
DefenderVotes: DoubleMap<Round, Voter, Vote>
NextIntakeAt: Option<BlockNumber>         // Bir sonraki intake zamani
NextChallengeAt: Option<BlockNumber>      // Bir sonraki challenge zamani
```

### 3.4 Period (Donem) Hesabisi

```rust
// lib.rs:1548-1561
fn period() -> Period {
    let rotation_period = VotingPeriod + ClaimPeriod;
    let now = current_block_number();
    let phase = now % rotation_period;
    if now >= next_intake_at() {
        Period::Intake { elapsed: now - next_intake_at() }
    } else if phase < VotingPeriod {
        Period::Voting { elapsed: phase, more: VotingPeriod - phase }
    } else {
        Period::Claim { elapsed: phase - VotingPeriod, more: rotation_period - phase }
    }
}
```

- `RotationPeriod = VotingPeriod + ClaimPeriod`
- `NextIntakeAt` kaydedilir; her intake sonrasi bir sonraki `next_intake_at = prev + RotationPeriod`.
- `NextChallengeAt` ayri ve sadece `ChallengePeriod` ile arttirilir.

### 3.5 Uyelik Basvuru Sureci

**1. Bid (Teklif Verme):**
```
bid(value) → candidate_deposit reserve edilir → bids listesine sirali eklenir
```
- Bid'ler kucukten buyuge siralidir.
- `MaxBids` doluysa en yuksek bid atilir (`AutoUnbid` eventi) — lib.rs:1819-1828.
- Sifir bid sadece ilk secim olarak kabul edilir.

**2. Vouch (Oneri):**
```
vouch(who, value, tip) → uyenin vouching statusu Vouching olarak ayarlanir
                        → BidKind::Vouch(voucher, tip) ile bid eklenir
```
- Bir uye ayni anda sadece bir kisiyi onerabilir.

**3. Candidate Secimi (`select_new_candidates`):**
- **lib.rs:1766-1808**
- `max_selections = min(max_intake, max_members - member_count, bids.len())`
- Soldan saga toplam maliyet <= pot olacak sekilde bid'ler alinir.
- Sifir bid sadece ilk secim olabilir.
- Secilen bid'ler `Candidates` storage'ina yazilir; bids listesinden cikarilir.

**4. Voting:**
```rust
// lib.rs:1627-1640
fn do_vote(maybe_old, approve, rank, tally) -> Vote {
    // Eski oyu geri al
    let weight_root = rank + 1;
    let weight = weight_root * weight_root;  // Rank^2 agirlikli oy
    // Yeni oyu ekle
    Vote { approve, weight }
}
```
- **Oy agirligi:** `weight = (rank + 1)^2`
  - Rank 0 uye: weight = 1
  - Rank 1 uye: weight = 4
  - Rank 2 uye: weight = 9
- Uye oyunu degistirebilir (eski agirligi geri alinir).
- Ilk oy bedava (`Pays::No`), degistirme ucretli.

**5. Oylama Sonucu:**

```rust
// lib.rs:379-393
impl Tally {
    fn more_approvals(&self) -> bool { self.approvals > self.rejections }
    fn more_rejections(&self) -> bool { self.rejections > self.approvals }
    fn clear_approval(&self) -> bool { self.approvals >= (2 * self.rejections).max(1) }
    fn clear_rejection(&self) -> bool { self.rejections >= (2 * self.approvals).max(1) }
}
```

- `clear_approval`: Onaylar >= 2*reddler (net onay — herkes tarafindan claim edilebilir).
- `clear_rejection`: Reddler >= 2*onaylar (net red — herkes tarafindan dusurulebilir).
- `more_approvals`: Sadece kucuk cokgunluk.

**6. Membership Claim:**
- `claim_membership()`: Aday `clear_approval` oldugunda cagirabilir.
- `bestow_membership()`: Founder, `clear_rejection` olmayan bir adayi kabul edebilir.
- `kick_candidate()`: Founder, `clear_approval` olmayan adayi atar.

### 3.6 Membership Challenge (Uyelik Savunmasi)

- Her `ChallengePeriod` blocklarinda tetiklenir.
- Founder ve Head asla challenger secilmez.
- Rastgele bir savunmaci + rastgele bir kuseyci secilir.
- Savunmaci kendi lehine oy kullanabilir (tie = basarisizlik, yani reddedilir).
- `more_approvals()` — kati coguluk gerekli (esitlik = suspensiyon).
- Kuseyci yanlis oy kullandiysa veya oy kullanmadiysa bir strike alir ve kendisi sonraki round'da savunmaci olur.

### 3.7 Pot Mekanizmasi

**Birikim:**
- `rotate_intake` sirasinda:
  ```rust
  // lib.rs:1742-1744
  let unaccounted = free_balance(account_id) - pot;
  pot += min(PeriodSpend, unaccounted / 2);
  ```
  - Pot, hesap bakiyesinin yarisi kadar veya `PeriodSpend` kadar artirilir (hangisi kucukse).
- External transfer (slash, tip vb.) geldiginde `OnUnbalanced` ile pot arttirilir.

**Harcama:**
- Adaylar kabul edildiginde: `reserve_payout(bid_value)` ile pot azaltilir, payouts hesabina gecilir.
- Slash: `slash_payout` ile payouts'dan pot'a geri alinir.

**Iki Ayri Hesap:**
- `account_id()` = ana topluluk hazinesi (`PalletId.into_account_truncating()`)
- `payouts()` = odeme bekleme hesabi (`PalletId.into_sub_account_truncating(b"payouts")`)

### 3.8 Payout Mekanizmasi

- Uyeye kabul edildiginde `reward_bidder` cagirilir:
  - Deposit bid: Deposit unreserve edilir.
  - Vouch bid: Tip, voucher'a `maturity` aninda odenir; geri kalani adaya.
- `maturity = now + lock_duration(member_count)`:
  ```rust
  // lib.rs:2250-2251
  fn lock_duration(x: u32) -> BlockNumber {
      let lock_pc = 100 - 50_000 / (x + 500);
      Percent::from_percent(lock_pc as u8) * MaxLockDuration
  }
  ```
  - 1 uye: `100 - 50000/501 ≈ 0.2%` → neredeyse sifir.
  - 500 uye: `100 - 50000/1000 = 50%` → MaxLockDuration'un %50'si.
  - 1000 uye: `100 - 50000/1500 ≈ 67%` → MaxLockDuration'un %67'si.
- Rank 1+ uyeler payout alamaz (`NoPayout` hatasi).
- Payout: Her cagrida sadece bir payout (en erken matureli) alinabilir.

### 3.9 Strike Mekanizmasi

```rust
// lib.rs:1946-1967
fn strike_member(who) {
    record.strikes += 1;
    if record.strikes >= GraceStrikes {
        slash_payout(who, total_payout / 2);  // Yarim slash
    }
    if record.strikes >= params.max_strikes {
        suspend_member(who);  // Tam suspensiyon
    }
}
```

- `GraceStrikes` gecince: bekleyen odemeler yariya indirilir.
- `max_strikes` gecince: uye suspend edilir.

### 3.10 Suspended Member Yargilama

- `judge_suspended_member(who, forgive)`: Sadece Founder cagirir.
- `forgive = true`: Uye geri alinir (rank korunur, ama max_members doluysa basarisiz olur).
- `forgive = false`: Tum payout'lar silinir ve payouts hesabindaki fonlar pot'a geri doner.

### 3.11 Extrinsic Listesi

| Index | Extrinsic | Origin |
|---|---|---|
| 0 | `bid` | Signed (dis kullanici) |
| 1 | `unbid` | Signed (bidder) |
| 2 | `vouch` | Signed (member) |
| 3 | `unvouch` | Signed (vouching member) |
| 4 | `vote` | Signed (member) |
| 5 | `defender_vote` | Signed (member) |
| 6 | `payout` | Signed (rank-0 member) |
| 7 | `waive_repay` | Signed (rank-0 member) |
| 8 | `found_society` | FounderSetOrigin |
| 9 | `dissolve` | Signed (Founder, tek uye) |
| 10 | `judge_suspended_member` | Signed (Founder) |
| 11 | `set_parameters` | Signed (Founder) |
| 12 | `punish_skeptic` | Signed (aday) |
| 13 | `claim_membership` | Signed (aday) |
| 14 | `bestow_membership` | Signed (Founder) |
| 15 | `kick_candidate` | Signed (Founder) |
| 16 | `resign_candidacy` | Signed (aday) |
| 17 | `drop_candidate` | Signed (herhangi) |
| 18 | `cleanup_candidacy` | Signed (herhangi) |
| 19 | `cleanup_challenge` | Signed (herhangi) |
| 20 | `poke_deposit` | Signed (bidder) |
| 21 | `kick_member` | Signed (Founder) |

### 3.12 Kritik Tasarim Notlari

- **Oylama sonucu randomdur:** Kazanan rastgele secilmez; oylama agirligi ile belirlenir. Skeptic mekanizmasinda rastgelelik vardir (ChaCha RNG).
- **Kuseyci (Skeptic):** Her intake turunda rastgele secilen uye. Adaylar icin oy kullanmak zorunda. Oy vermezse ya da yanlis oy verirse strike alir ve sonraki challenge'da savunmaci olabilir.
- **Vouch ban:** Rejected candidate'i oneren uye `VouchingStatus::Banned` olur ve bir daha vouch edemez.
- **AutoUnbid:** Bids listesi MaxBids'e ulasinca, en yuksek degerli bid atilir.
- **Deposit vs. Vouch bid:** Deposit bid'lerde candidate_deposit reserve edilir; vouch bid'lerde deposit yoktur.
- **Head exempt:** Head member suspension challenge'a girmez ve hiçbir extrinsic ile dogrudan kaldirilamaz.

### 3.13 `waive_repay` ile Rank Yükseltme

- Rank 0 uye, aldigi tum odemeleri geri oduyor ve kalan pending payout'ları silerse rank 1'e yükseltilir.
- Rank 1 uyeler payout alamaz, ancak oy agirliklari daha yüksektir.

---

## 4. pallet-lottery

**Kaynak:** `substrate/frame/lottery/src/lib.rs`

### 4.1 Genel Bakis

Lottery, belirli call'lari dispatch etmeyi ticket satin almaya donusturuyor. Kullanici bir valid call'i `buy_ticket` ile dispatch eder; call basarili olursa ticket satin alinir.

### 4.2 Config Parametreleri

```rust
type PalletId: Get<PalletId>;           // Piyango hesap ID'si
type RuntimeCall: Parameter + ...;       // Dispatch edilebilir call tipi
type Currency: ReservableCurrency;       // Para birimi
type Randomness: Randomness;             // Rastgelelik kaynagi
type ManagerOrigin: EnsureOrigin;       // set_calls ve start_lottery icin origin
type MaxCalls: Get<u32>;                // Tek lottery'de max gecerli call sayisi
type ValidateCall: ValidateCall;         // Call gecerliligi kontrolu
type MaxGenerateRandom: Get<u32>;        // Modulus bias giderme icin max deneme sayisi
```

### 4.3 Storage Items

```rust
// lib.rs:207-239
LotteryIndex: u32                        // Her yeni lottery ile artar
Lottery: Option<LotteryConfig>           // Mevcut lottery konfigurasyonu
Participants: Map<AccountId, (u32, BoundedVec<CallIndex, MaxCalls>)>
    // AccountId -> (lottery_index, kullanilan_call_index_listesi)
TicketsCount: u32                        // Toplam satilan ticket sayisi
Tickets: Map<u32, AccountId>             // ticket_index -> sahip
CallIndices: BoundedVec<CallIndex, MaxCalls>  // Gecerli (pallet, call) ciftleri
```

### 4.4 `LotteryConfig` Yapisi

```rust
// lib.rs:84-96
pub struct LotteryConfig<BlockNumber, Balance> {
    price: Balance,      // Ticket fiyati
    start: BlockNumber,  // Lottery baslangic blocku
    length: BlockNumber, // Lottery suresi (start + length = son ticket alim blocku)
    delay: BlockNumber,  // Kazanan secim gecikmesi (start + length + delay = odeme blocku)
    repeat: bool,        // Tamamlaninca otomatik tekrar et
}
```

### 4.5 Ticket Satin Alma Mekanizmasi

```rust
// lib.rs:304-313
pub fn buy_ticket(origin, call) -> DispatchResult {
    let caller = ensure_signed(origin.clone())?;
    call.clone().dispatch(origin)?;     // Once call dispatch edilir
    let _ = Self::do_buy_ticket(&caller, &call);   // Sonra ticket alinir (sessiz basarisizlik)
    Ok(())
}
```

- Call once dispatch edilir; basarisiz olursa ticket alinmaz.
- Ticket alim basarisizligi sessiz kalir (hata dondurmez, event emit etmez).
- `TicketBought` eventi basarili ticket alinminda emit edilir.

**`do_buy_ticket` icindeki kontroller:**
- Lottery mevcut olmali (`NotConfigured`).
- `block_number < start + length` olmali (`AlreadyEnded`).
- Call valid olmali (`InvalidCall`).
- Ayni lottery'de ayni call ile zaten katilmamis olmali (`AlreadyParticipating`).
- `T::Currency::transfer(caller, account_id, price)` — ticket bedeli aktarilir.

### 4.6 CallIndex Mekanizmasi

```rust
// lib.rs:429-435
fn call_to_index(call) -> Result<CallIndex, DispatchError> {
    let encoded_call = call.encode();
    if encoded_call.len() < 2 { return Err(EncodingFailed); }
    Ok((encoded_call[0], encoded_call[1]))  // (pallet_index, call_index)
}
```

- Her runtime call, ilk 2 byte olarak `(pallet_index, call_index)` iceriyor.
- `set_calls` ile valid call'lar storage'a yazilir; validate etmek icin bu indeksler kullanilir.
- **Onemli sinir:** Call parametreleri validate edilemez — sadece hangi pallet/call gecerli belirlenir.

### 4.7 Kazanan Secimi

```rust
// lib.rs:493-508
fn choose_ticket(total: u32) -> Option<u32> {
    if total == 0 { return None; }
    let mut random_number = Self::generate_random_number(0);

    // Modulus bias giderme: u32::MAX'in total'e tam bolunemeyen kismini atlat
    for i in 1..MaxGenerateRandom {
        if random_number < u32::MAX - u32::MAX % total {
            break;
        }
        random_number = Self::generate_random_number(i);
    }

    Some(random_number % total)
}

// lib.rs:517-522
fn generate_random_number(seed: u32) -> u32 {
    let (random_seed, _) = T::Randomness::random(&(PalletId, seed).encode());
    <u32>::decode(&mut random_seed.as_ref()).expect("...")
}
```

- `T::Randomness` (genellikle BABE/pallet-insecure-randomness) kullanilir.
- `MaxGenerateRandom` ile modulus bias giderilmeye calisilir — `u32::MAX - u32::MAX % total` altinda kalan ilk random sayi kullanilir.
- **TODO notu:** Kaynak kodda randomness freshness problemi belirtilmis (lib.rs:515, issue #8311).

### 4.8 Lottery Tamamlanma (`on_initialize`)

```rust
// lib.rs:243-283
fn on_initialize(n) -> Weight {
    if payout_block <= n {
        let winner = choose_account().unwrap_or(lottery_account);
        T::Currency::transfer(&account_id, &winner, lottery_balance, KeepAlive);
        TicketsCount::kill();
        if config.repeat {
            LotteryIndex += 1;
            config.start = n;  // Simdi'den baslatilir
        } else {
            Lottery = None;
        }
        // NOTE: Participants ve Tickets temizlenmez! Residual data kalabilir.
    }
}
```

- `lottery_balance = free_balance(account_id) - minimum_balance` (existential deposit pot'a dahil degildir).
- Kazanan yoksa (hic ticket satilmamissa) `lottery_account`'a iade edilir.
- Katilimcilar ve ticket mapping'leri **kasitli olarak temizlenmez** — buyuk write maliyeti; `LotteryIndex` ile stale data ignore edilir.

### 4.9 Extrinsicler

| Index | Extrinsic | Origin |
|---|---|---|
| 0 | `buy_ticket(call)` | Signed |
| 1 | `set_calls(calls)` | ManagerOrigin |
| 2 | `start_lottery(price, length, delay, repeat)` | ManagerOrigin |
| 3 | `stop_repeat()` | ManagerOrigin |

### 4.10 Events

| Event | Ne Zaman |
|---|---|
| `LotteryStarted` | `start_lottery` sonrasi |
| `CallsUpdated` | `set_calls` sonrasi |
| `Winner { winner, lottery_balance }` | Her lottery tamamlanmasi |
| `TicketBought { who, call_index }` | Basarili ticket alimi |

### 4.11 Kritik Tasarim Notlari

- **Ticket = call dispatch:** Kullanici gereksiz bir call yapmak zorunda kalmaz; halihazirda yapacagi call'lar ticket olarak sayilir.
- **AlreadyParticipating per-call:** Ayni lottery'de ayni call tipini ikinci kez kullanarak ekstra ticket alinamaz; farkli call tipler icin farkli ticketlar alinabilir.
- **Residual storage:** Participants ve Tickets temizlenmez; `LotteryIndex` stale data'yi overlay eder. Bu bilerek yapilmistir (buyuk blok yazim maliyetinden kacmak).
- **Randomness freshness:** Resmi olarak belgelenmis bir eksiklik. pallet-insecure-randomness gibi kaynaklar fresh olmayan randomness uretebilir.

---

## 5. pallet-root-offences

**Kaynak:** `substrate/frame/root-offences/src/lib.rs`

**Not:** Bu pallet test amacli. Uretim zincirlerinde kullanilmamalidir.

### 5.1 Amac

Root origin'e offence olusturbilme ve slash tetikleyebilme kapasitesi saglar. E2E staking/offences pipeline'ini test etmek icin kullanilir.

### 5.2 `TestSpamOffence` Yapisi

```rust
// lib.rs:47-57
pub struct TestSpamOffence<Offender> {
    pub offender: Offender,
    pub session_index: SessionIndex,
    pub time_slot: u128,          // Ayni session'da farkli offence olusturmak icin
    pub slash_fraction: Perbill,  // Uygulancak slash yüzdesi
}

impl Offence<Offender> {
    const ID: Kind = *b"spamspamspamspam";  // 16 byte sabit ID
    type Slot = u128;
}
```

### 5.3 Extrinsicler

#### `create_offence(origin, offenders, maybe_identifications, maybe_session_index)` — call_index: 0
- **lib.rs:137-167**
- Origin: `ensure_root`
- `maybe_identifications`: `Some` ise verilen identification kullanilir; `None` ise `FullIdentificationOf::convert` ile storage'dan alinir.
- `maybe_session_index`: `None` ise mevcut session index kullanilir.
- `OnOffenceHandler::on_offence` dogrudan cagirilir — pallet-offences atlanir.

#### `report_offence(origin, offences)` — call_index: 1
- **lib.rs:181-199**
- Origin: `ensure_root`
- `TestSpamOffence` olusturur ve `T::ReportOffence::report_offence` ile pallet-offences'e iletir.
- Daha gercekci E2E testi icin — offences pipeline'in tamamini cagirir.

### 5.4 Config

```rust
// lib.rs:84-106
type OffenceHandler: OnOffenceHandler<AccountId, IdentificationTuple, Weight>;
type ReportOffence: ReportOffence<...>;
```

### 5.5 Kullanim Notu

- `validator_set_count()` unimplemented olarak birakilmis (`unreachable!()`) — bu test amaciyla kasitlidir.
- Real-world slash oranlarini test etmek icin `slash_fraction` parametresi dogrudan girilir.

---

## 6. Kritik Sayisal Degerler

| Pallet | Deger | Aciklama |
|---|---|---|
| pallet-recovery | `MAX_GROUPS_PER_ACCOUNT = 10` | Bir hesap icin maksimum friend group sayisi |
| pallet-recovery | `cancel_delay >= 1` | Zorunlu; sifir `NoCancelDelay` hatasi |
| pallet-recovery | `friends_needed > 0` | Zorunlu; sifir `NoFriendsNeeded` hatasi |
| pallet-recovery | `friends_needed <= friends.len()` | Zorunlu; `TooManyFriendsNeeded` hatasi |
| pallet-recovery | `inheritance_priority: 0` | En yuksek oncelik |
| pallet-society | `clear_approval = approvals >= max(2 * rejections, 1)` | Net onay esigi |
| pallet-society | `clear_rejection = rejections >= max(2 * approvals, 1)` | Net red esigi |
| pallet-society | `vote_weight = (rank + 1)^2` | Oy agirligi |
| pallet-society | `lock_pc = 100 - 50000 / (x + 500)` | Payout kilit suresi formulu |
| pallet-society | `pot_delta = min(PeriodSpend, unaccounted / 2)` | Her rotation'da pot artisi |
| pallet-society | `strike_slash = total_payout / 2` | GraceStrikes gecince payout slash |
| pallet-lottery | `CallIndex = (encoded[0], encoded[1])` | (pallet_index, call_index) |
| pallet-lottery | `bias_limit = u32::MAX - u32::MAX % total` | Modulus bias giderme limiti |

---

## 7. Nova Shield Chain Icin Cikarsimlar

### 7.1 pallet-sudo Kullanimi

- **Bootstrap fazinda kullanilabilir:** Zincirin ilk asamasinda sudo ile genesis konfigurasyonu, initial token dağıtımı, pallet parametrelerini ayarlama.
- **`CheckOnlySudoAccount` extension:** Eger chain'in ilk fazinda sadece sudo'nun islem yapmasi isteniyorsa bu extension eklenmeli. Mempool spam saldirilarini etkili sekilde engeller.
- **`sudo_unchecked_weight` tehlikesi:** Buyuk migration call'larinda kullanilabilir, ancak block weight'i asmamaya dikkat edilmeli. Gecici olarak block weight'i artirip migration yapip geri alindigi senaryolar var.
- **Kaldirilmasi:** Uretim asamasinda `remove_key()` ile sudo kaldirmak veya governance'a devretmek onerilen yaklasimdir.

### 7.2 pallet-recovery Kullanimi

- **Nova Shield Chain icin cok uygun:** Zincirde sosyal kurtarma mekanizmasi saglar. Kullanicilarin private key'lerini kaybetmesi durumunda topluluk tabanli kurtarma imkani.
- **MAX_GROUPS_PER_ACCOUNT = 10:** Yeterli esneklik.
- **SecurityDeposit:** Spam initiation'lari engellemek icin iyi bir tasarim. Nova Shield'da uygun miktar belirlenmelidir.
- **cancel_delay zorunlulugu:** Malicious recovery onleme icin kritik.
- **Inheritor multisig olabilir:** Chain'de multisig + recovery kombinasyonu ile guclu anahtar yonetimi.
- **BlockNumberProvider:** Parachain ise relay chain block numarasi, solo chain ise local block numarasi onerilir.

### 7.3 pallet-society Kullanimi

- **DAO/topluluk token yonetimi icin ilham kaynagi:** Society'nin voting mekanizmasi (rank^2 agirlikli oy), pot mekanizmasi ve challenge sistemi ilginc DAO primitifleri sunuyor.
- **Dogrudan kullanim zorlu:** Society pallet cok spesifik bir ekonomik oyun. Genel amacli DAO icin daha fazla ozellestirme gerekiyor.
- **Pot mekanizmasi:** `min(PeriodSpend, unaccounted / 2)` formulu treasury/staking icin model alinabilir.
- **Challenge mekanizmasi:** Membership validity kontrolu icin ilham: periyodik challenge ile aktif olmayan uyeler cikarilabiir.

### 7.4 pallet-lottery Kullanimi

- **DeFi katilimi tesvik:** Nova Shield'da staking, governance oylamasina katilim gibi islmleri lottery ticket olarak kullanmak kullanici katilimini artirabilir.
- **Randomness freshness sorunu:** Production'da `pallet-babe` veya VRF tabanli randomness kullanilmali.
- **Residual storage:** Lottery endeksi degisince eski participant/ticket verileri otomatik gecersiz sayilir — temizlik on-demand yapilmali.
- **CallIndex mekanizmasi basit ama guclu:** Parametre bazli validate icin ozel `ValidateCall` implementasyonu gerekir.

### 7.5 pallet-root-offences

- **Test zincirlerinde faydali:** Staking slash/offences entegrasyon testleri icin kullanilabilir.
- **Uretim'de kullanilmamali:** `validator_set_count()` unimplemented; slash oranlari bozulabilir.

### 7.6 Entegrasyon Noktaları

| Kombinasyon | Fayda |
|---|---|
| `pallet-sudo` + `pallet-recovery` | Bootstrap: sudo ile kurulum, sonra recovery ile hesap guvenligini artir |
| `pallet-recovery` + multisig | En guclu anahtar yonetimi: threshold + social recovery |
| `pallet-lottery` + governance voting | Governance katilimini incentivize et |
| `pallet-society` + `pallet-treasury` | DAO benzeri yapilarda society'nin treasury mekanizmasini model al |
