# Agent-28: pallet-conviction-voting + pallet-elections-phragmen Analizi

## Kapsanan Dosyalar

### pallet-conviction-voting
- `substrate/frame/conviction-voting/src/conviction.rs`
- `substrate/frame/conviction-voting/src/vote.rs`
- `substrate/frame/conviction-voting/src/types.rs`
- `substrate/frame/conviction-voting/src/traits.rs`
- `substrate/frame/conviction-voting/src/lib.rs`
- `substrate/frame/conviction-voting/src/tests.rs`

### pallet-elections-phragmen
- `substrate/frame/elections-phragmen/src/lib.rs` (1300+ satir)

### election-provider-multi-phase
- `substrate/frame/election-provider-multi-phase/src/lib.rs`
- `substrate/frame/election-provider-multi-phase/src/signed.rs`

### pallet-asset-rewards
- `substrate/frame/asset-rewards/src/lib.rs`

---

## 1. pallet-conviction-voting: Conviction Sistemi

**Dosya:** `conviction.rs`

### Conviction Enum (satir 44-59)

```rust
pub enum Conviction {
    None,       // 0
    Locked1x,   // 1
    Locked2x,   // 2
    Locked3x,   // 3
    Locked4x,   // 4
    Locked5x,   // 5
    Locked6x,   // 6
}
```

### Oy Gucü Hesabi (satir 113-122)

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

**KRITIK:** `None` conviction = `capital / 10` (0.1x), diger convictionlar = `capital * multiplier`

### Lock Periyotlari (satir 100-110)

```
Conviction::None     -> lock_periods = 0  (kilitlenme YOK)
Conviction::Locked1x -> lock_periods = 1  (1x enactment period)
Conviction::Locked2x -> lock_periods = 2  (2x enactment period)
Conviction::Locked3x -> lock_periods = 4  (4x enactment period)
Conviction::Locked4x -> lock_periods = 8  (8x enactment period)
Conviction::Locked5x -> lock_periods = 16 (16x enactment period)
Conviction::Locked6x -> lock_periods = 32 (32x enactment period)
```

**KRITIK:** Lock periyotlari dogrusal degil, KATLANIYOR (1->2->4->8->16->32).

### Tam Tablo

| Conviction  | Oy Carpani | Kilit Periyot | Oy Gucü (100 token icin) |
|-------------|-----------|---------------|--------------------------|
| None        | 0.1x      | 0 periyot     | 10 votes, 100 capital    |
| Locked1x    | 1x        | 1 periyot     | 100 votes                |
| Locked2x    | 2x        | 2 periyot     | 200 votes                |
| Locked3x    | 3x        | 4 periyot     | 300 votes                |
| Locked4x    | 4x        | 8 periyot     | 400 votes                |
| Locked5x    | 5x        | 16 periyot    | 500 votes                |
| Locked6x    | 6x        | 32 periyot    | 600 votes                |

---

## 2. pallet-conviction-voting: Oy ve Delegasyon

### Vote Encoding (vote.rs satir 36-53)

Vote tek byte ile encode edilir:
- Bit 7 (MSB): aye=1, nay=0
- Bit 0-6: conviction degeri (0-6)

Ornek: `aye=true, Locked3x` -> `0b1000_0000 | 3 = 0x83`

### AccountVote Tipler (vote.rs satir 81-91)

```rust
pub enum AccountVote<Balance> {
    Standard { vote: Vote, balance: Balance },  // Klasik oy, conviction'li
    Split { aye: Balance, nay: Balance },        // Bolunmus oy, conviction YOK
    SplitAbstain { aye: Balance, nay: Balance, abstain: Balance }, // Cekimser dahil
}
```

**NOT:** Split ve SplitAbstain oylar her zaman `Conviction::None` (0.1x) ile isleme girer.

### Tally Yapisi (types.rs satir 43-52)

```rust
pub struct Tally<Votes, Total> {
    pub ayes: Votes,    // Post-conviction aye oylari
    pub nays: Votes,    // Post-conviction nay oylari
    pub support: Votes, // Pre-conviction ham bakiye (sadece aye + abstain)
    dummy: PhantomData<Total>,
}
```

**KRITIK Fark:** `ayes/nays` conviction sonrasi degerleri, `support` ise ham kapital.

### Tally Hesaplari (types.rs satir 64-79)

```rust
fn support(&self, _: Class) -> Perbill {
    Perbill::from_rational(self.support, Total::get())
}

fn approval(&self, _: Class) -> Perbill {
    let total = self.ayes.saturating_add(self.nays);
    if total.is_zero() { Perbill::zero() }
    else { Perbill::from_rational(self.ayes, total) }
}
```

- **Support** = `support / total_issuance` (katilim orani)
- **Approval** = `ayes / (ayes + nays)` (onay orani)

### Tally'ye Vote Ekleme (types.rs satir 127-157)

Standard vote icin:
- `conviction.votes(balance)` -> `Delegations { votes, capital }`
- Aye ise: `support += capital`, `ayes += votes`
- Nay ise: `nays += votes` (support ARTMIYOR)

SplitAbstain icin:
- `support += aye.capital + abstain.capital` (abstain de support'a eklenifir)
- `ayes += aye.votes`, `nays += nay.votes`

### Storage Items (lib.rs satir 164-185)

```rust
// Her account, her class icin oy durumu
VotingFor<T, I>: StorageDoubleMap<AccountId, ClassOf<T,I>, VotingOf<T,I>>

// Her account'in class bazli kilit miktari
ClassLocksFor<T, I>: StorageMap<AccountId, BoundedVec<(ClassOf, Balance), ClassCount>>
```

### Extrinsic'ler (lib.rs satir 239-421)

| Call Index | Fonksiyon | Aciklama |
|------------|-----------|----------|
| 0 | `vote(poll_index, vote)` | Referendum'a oy ver |
| 1 | `delegate(class, to, conviction, balance)` | Bir class icin oy yetki devri |
| 2 | `undelegate(class)` | Yetki devrini geri al |
| 3 | `unlock(class, target)` | Süresi dolan kilitleri ac |
| 4 | `remove_vote(class?, index)` | Kendi oyunu kaldir |
| 5 | `remove_other_vote(target, class, index)` | Baskasinin oyunu kaldir (sadece suresi dolmus) |

### Config Parametreleri

- `MaxVotes: Get<u32>` - Bir account'in max eş zamanli oy sayisi
- `VoteLockingPeriod: Get<BlockNumber>` - Temel kilit periyotu (1 conviction periyotu = bu deger)
- `MaxTurnout: Get<Balance>` - Maksimum katilim (genellikle total_issuance)
- `VotingHooks` - On_vote, on_remove_vote hook trait'i

---

## 3. pallet-conviction-voting: Kilit Mekanizmasi

### PriorLock (vote.rs satir 169-188)

```rust
pub struct PriorLock<BlockNumber, Balance>(BlockNumber, Balance);

impl PriorLock {
    pub fn accumulate(&mut self, until: BlockNumber, amount: Balance) {
        self.0 = self.0.max(until);  // Daha uzun sure ile guncelle
        self.1 = self.1.max(amount); // Daha yuksek miktar ile guncelle
    }

    pub fn rejig(&mut self, now: BlockNumber) {
        if now >= self.0 {
            self.0 = Zero::zero();
            self.1 = Zero::zero();
        }
    }
}
```

**KRITIK:** PriorLock her zaman MAX suresini ve MAX miktarini tutar (additive degil, MAX).

### Kilit Hesaplama (vote.rs satir 287-294)

```rust
pub fn locked_balance(&self) -> Balance {
    match self {
        Voting::Casting(Casting { votes, prior, .. }) => {
            // tum oy miktarlari + prior lock'un maksimumu
            votes.iter().map(|i| i.1.balance()).fold(prior.locked(), |a, i| a.max(i))
        },
        Voting::Delegating(Delegating { balance, prior, .. }) => {
            // delegasyon miktari ve prior lock'un maksimumu
            *balance.max(&prior.locked())
        },
    }
}
```

**KRITIK:** Toplam kilit `MAX(tum oylar, prior_lock)` seklinde hesaplanir, TOPLAM degil.

### update_lock Akisi (lib.rs satir 733-761)

1. `VotingFor` storage'i guncelle, `rejig(now)` cagir (suresi dolan prior kilitleri temizle)
2. `locked_balance()` hesapla
3. `ClassLocksFor` guncelle
4. Tum class'lar arasindaki MAX kilidi bul
5. 0 ise lock'u kaldir, degil ise `Currency::set_lock` cagir

Lock ID: `b"pyconvot"` (8 byte, `CONVICTION_VOTING_ID`)

### Kilit Süresi Formülü

```
unlock_at = poll_end_block + VoteLockingPeriod * conviction.lock_periods()
```

- Ornek: `VoteLockingPeriod = 7 gun`, `Locked3x` kullandiysa: `7 * 4 = 28 gun` kilit

### remove_vote vs remove_other_vote Farki (lib.rs satir 383-421)

**remove_vote:**
- Yalnizca kendi oyu
- `UnvoteScope::Any` - her zaman yapilabilir
- Suresi dolmamis kilitte: prior.accumulate ile kilit transfer edilir

**remove_other_vote:**
- Hedef != imzalayan ise `UnvoteScope::OnlyExpired`
- Kilit suresi dolmamissa `NoPermissionYet` hatasi

### Delegasyon Kilit Mantigi (lib.rs satir 669-705)

`undelegate` cagrildiginda:
```rust
prior.accumulate(
    now + VoteLockingPeriod * conviction.lock_periods(),
    balance,
)
```
Yani undelegate edildiginde kilit HEMEN baslayan bir PriorLock olusturur.

### Delegasyon Derinligi Limiti (lib.rs satir 573-619)

```rust
fn increase_upstream_delegation(who, class, amount) -> u32 {
    VotingFor::mutate(who, class, |voting| match voting {
        Voting::Delegating(Delegating { delegations, .. }) => {
            // We don't support second level delegating
            *delegations = delegations.saturating_add(amount);
            1
        },
        Voting::Casting(Casting { votes, delegations, .. }) => { ... }
    })
}
```

**KRITIK:** Delegasyon YALNIZCA 1 seviye desteklenmektedir. Bir Delegating account'a delegasyon yapilabilir ama o account'in delegasyonu upstream'e YAYILMAZ (sadece delegations sayaci artar).

---

## 4. pallet-elections-phragmen: Phragmen Algoritmasi

### Algoritma Akisi (lib.rs satir 948-1155, do_phragmen fonksiyonu)

```
1. desired_seats = DesiredMembers
2. desired_runners_up = DesiredRunnersUp
3. num_to_elect = desired_seats + desired_runners_up

4. candidates = Candidates + Members (implicit) + RunnersUp (implicit)

5. voters_and_votes = Voting storage'dan al (MaxVoters limit)
   - total_issuance ile balance -> vote weight cevirisi

6. sp_npos_elections::seq_phragmen(num_to_elect, candidates, voters)

7. Kazananlar: ilk desired_seats -> Members
               geri kalanlar -> RunnersUp (worst-to-best sirali)

8. Prime Member secimi: Borda count ile
   - Ilk oy: MaxVotesPerVoter * weight
   - Ikinci oy: (MaxVotesPerVoter - 1) * weight
   - ...
   - En yuksek agirlikli oy alan uye prime olur

9. Kaybedenler: deposit SLASH edilir (CandidateSlashed eventi)

10. ElectionRounds += 1
```

### Sequential Phragmen Kaynagi

`sp_npos_elections::seq_phragmen` fonksiyonu kullanilir. Bu algoritma:
- Her adimda mevcut "load" en az olan adayi secer
- Her secimden sonra voter loadlari guncellenir
- Sonuc: Minimuma indirgenmus maksimum yuk

---

## 5. pallet-elections-phragmen: Aday ve Uye Yonetimi

### Storage Items (lib.rs satir 685-718)

```rust
Members<T>: StorageValue<Vec<SeatHolder<AccountId, Balance>>>
    // Invariant: account_id'ye gore siralanmis

RunnersUp<T>: StorageValue<Vec<SeatHolder<AccountId, Balance>>>
    // Invariant: stake'e gore sirali (worst to best), son eleman best

Candidates<T>: StorageValue<Vec<(AccountId, Balance)>>
    // Invariant: account_id'ye gore siralanmis

ElectionRounds<T>: StorageValue<u32>
    // Toplam secim turu sayisi

Voting<T>: StorageMap<AccountId, Voter<AccountId, Balance>>
    // Her voter'in oylarini ve stake'ini tutar
```

### Config Sabitleri

| Parametre | Aciklama |
|-----------|----------|
| `CandidacyBond` | Aday olmak icin gerekli depozito (reserve edilir) |
| `VotingBondBase` | Oy kullanmak icin temel depozito |
| `VotingBondFactor` | Oy basina ek depozito (32 byte) |
| `DesiredMembers` | Secilmek istenen uye sayisi |
| `DesiredRunnersUp` | Tutulan yedek sayisi |
| `TermDuration` | Her secim arasi blok sayisi (0 ise secim olmaz) |
| `MaxCandidates` | Maksimum aday sayisi |
| `MaxVoters` | Maksimum secmen sayisi |
| `MaxVotesPerVoter` | Voter basi max oy hedefi |

### Voting Bond Hesabi (lib.rs satir 787-790)

```rust
fn deposit_of(count: usize) -> BalanceOf<T> {
    T::VotingBondBase::get()
        .saturating_add(T::VotingBondFactor::get().saturating_mul((count as u32).into()))
}
```

Toplam depozito = `VotingBondBase + VotingBondFactor * oy_sayisi`

### Secim Tetikleyici (lib.rs satir 294-301)

```rust
fn on_initialize(n: BlockNumberFor<T>) -> Weight {
    let term_duration = T::TermDuration::get();
    if !term_duration.is_zero() && (n % term_duration).is_zero() {
        Self::do_phragmen()
    } else {
        Weight::zero()
    }
}
```

`n % TermDuration == 0` oldugunda secim tetiklenir.

### Extrinsic'ler

| Call | Origin | Aciklama |
|------|--------|----------|
| `vote(votes, value)` | Signed | Aday listesi ver, stake kilitle |
| `remove_voter()` | Signed | Secmen kaydini sil, kilit ve depozito iade |
| `submit_candidacy(candidate_count)` | Signed | Aday ol, CandidacyBond reserve et |
| `renounce_candidacy(Renouncing)` | Signed | Adayliktan cekil (Member/RunnerUp/Candidate) |
| `remove_member(who, slash_bond, rerun_election)` | Root | Uyeyi kaldir, depozito slash/iade |
| `clean_defunct_voters(num_voters, num_defunct)` | Root | Gecersiz secmenlerı temizle |

### Renounce_candidacy Sonuclari

- **Member olarak:** `remove_and_replace_member` cagrilir, en iyi runner-up uye olur
- **RunnerUp olarak:** Listeden cikarilir, depozito iade
- **Candidate olarak:** Listeden cikarilir, depozito iade
- **Her ucunde:** Deposit IADE edilir (slash OLMAZ)

### Kaybeden ve Slash Mantigi (lib.rs satir 1097-1108)

```rust
candidates_and_deposit.iter().for_each(|(c, d)| {
    if new_members_ids_sorted.binary_search(c).is_err() &&
       new_runners_up_ids_sorted.binary_search(c).is_err()
    {
        let (imbalance, _) = T::Currency::slash_reserved(c, *d);
        T::LoserCandidate::on_unbalanced(imbalance);
    }
});
```

**KRITIK:** Kaybeden HERKES (eski uyeler dahil) depozitolarini kaybeder.

### Implicit Candidates

Members ve RunnersUp her turde otomatik olarak adaydirlar (tekrar submit GEREKMIYOR). Ama kazanamadillarsa depozitolarini kaybederler.

---

## 6. election-provider-multi-phase

**Dosya:** `election-provider-multi-phase/src/lib.rs`

### Fazlar (lib.rs satir 341-362)

```rust
pub enum Phase<Bn> {
    Off,
    Signed,
    Unsigned((bool, Bn)),  // (aktif_mi, baslangic_blok)
    Emergency,
}
```

### Zaman Cizgisi

```
elect() cagrisi
       |
<--SignedPhase--> <--UnsignedPhase-->
|                |                  |
Off              Signed             Unsigned --> elect()
```

- `remaining <= SignedPhase + UnsignedPhase`: Signed fazina gec, snapshot al
- `remaining <= UnsignedPhase`: Unsigned fazina gec
- Her ikisi de 0 ise: Direkt Fallback

### Signed Faz

- Cozumler `RawSolution` olarak gonderilir
- Max `SignedMaxSubmissions` cozum kuyruklarda tutulur (en kötüden en iyiye)
- Yeni cozum mevcut kuyruktan birinden daha iyiyse: kotu cozum discard, yeni eklenir
- Faz sonunda: `feasibility_check` ile best'ten basla
  - Gecerse: gonderi odul alir (`SignedRewardBase`)
  - Gecmezse: depozito slash edilir
  - Kalan cozumler: depozito iade edilir, kontrol edilmez

### Unsigned Faz

- Sadece validator OCW worker'lari gonderebilir
- Unsigned (imzasiz) transaction, inherent benzeri
- Mevcut en iyi cozumden `BetterSignedThreshold` kadar iyi olmali
- `OffchainRepeat` blok arayla gonderim

### Emergency Faz

- Signed/Unsigned cozum bulunamazsa VEYA her ikisi basarisiz olursa
- `ForceOrigin` (council/root) acil cozum gonderebilir
- Kontrol yok, direk kabul edilir

### Snapshot Mekanizmasi

Signed faz basinda snapshot alinir:
- `RoundSnapshot { voters: Vec<VoterType>, targets: Vec<AccountId> }`
- `SnapshotMetadata { voters: u32, targets: u32 }` ile boyut tutulur
- `ElectionBounds` ile voter/target sayisi sinirlanir

### Signed Submission Deposito Hesabi

```
deposit = SignedDepositBase(kuyruk_boyutu) + solution_bytes * SignedDepositByte + weight * SignedDepositWeight
```

`SignedDepositBase` kuyruk boyutuna gore dinamik olarak degisir (GeometricDepositBase implementasyonu).

### Config (lib.rs satir 601-744)

Key parametreler:
- `SignedPhase`, `UnsignedPhase`: Faz uzunluklari
- `BetterSignedThreshold`: Cozum ne kadar daha iyi olmali (Perbill)
- `SignedMaxSubmissions`: Max kuyruk boyutu
- `SignedRewardBase`: Odül miktari
- `MaxWinners`, `MaxBackersPerWinner`: Sinirlar
- `Solver`: NposSolver implementasyonu (seq_phragmen vs)

---

## 7. pallet-asset-rewards

**Dosya:** `asset-rewards/src/lib.rs`

### Genel Bakis

- LP token ve benzeri asset'ler icin staking reward sistemi
- Synthetix `StakingRewards.sol` implementasyonu
- O(1) karmasiklik, JIT (just-in-time) hesaplama
- `PRECISION_SCALING_FACTOR = 4096` (satir 118)

### Temel Kavramlar

- **Staking Asset:** Stake edilen token (orn. LP token)
- **Reward Asset:** Odul token'i
- **Pool:** Staked asset + reward asset + rate konfigurasyon birimi
- **Pool Account:** Her pool'a ozgu hesap (PalletId.into_sub_account(pool_id))

### Storage Items (lib.rs satir 288-314)

```rust
PoolStakers<T>: StorageDoubleMap<PoolId, AccountId, PoolStakerInfo<Balance>>
Pools<T>: StorageMap<PoolId, PoolInfoFor<T>>
PoolCost<T>: StorageMap<PoolId, (AccountId, T::Consideration)>  // Depolama maliyeti
NextPoolId<T>: StorageValue<PoolId>  // Otomatik artrimali ID
```

### PoolInfo Yapisi (lib.rs satir 148-167)

```rust
pub struct PoolInfo<AccountId, AssetId, Balance, BlockNumber> {
    staked_asset_id: AssetId,
    reward_asset_id: AssetId,
    reward_rate_per_block: Balance,  // Blok basi odul miktari
    expiry_block: BlockNumber,       // Pool sona erme blok
    admin: AccountId,
    total_tokens_staked: Balance,
    reward_per_token_stored: Balance, // Birikimli odul / staked token
    last_update_block: BlockNumber,   // Son guncelleme bloku
    account: AccountId,              // Pool'un para hesabi
}
```

### PoolStakerInfo Yapisi (lib.rs satir 137-144)

```rust
pub struct PoolStakerInfo<Balance> {
    amount: Balance,                   // Stake edilen miktar
    rewards: Balance,                  // Birikmi, odenmemis oduller
    reward_per_token_paid: Balance,    // Son etkilesimde reward_per_token degeri
}
```

### Reward Algoritmasi (lib.rs satir 787-824)

**reward_per_token hesabi:**
```
rewardable_blocks = min(now, expiry_block) - last_update_block

reward_per_token = reward_per_token_stored +
    (reward_rate_per_block * rewardable_blocks * PRECISION_SCALING_FACTOR) / total_tokens_staked
```

**Staker'in odülü:**
```
new_rewards = staker.amount * (reward_per_token - staker.reward_per_token_paid) / PRECISION_SCALING_FACTOR
            + staker.rewards  (birikimli)
```

**KRITIK:** Hesaplamalar tamamen O(1). Her stake/unstake/harvest'ten once `update_pool_and_staker_rewards` cagrilmali.

### Extrinsic'ler (lib.rs satir 435-729)

| Call Index | Fonksiyon | Origin | Aciklama |
|------------|-----------|--------|----------|
| 0 | `create_pool(staked, reward, rate, expiry, admin)` | CreatePoolOrigin | Yeni pool olustur |
| 1 | `stake(pool_id, amount)` | Signed | Token freeze et, stake et |
| 2 | `unstake(pool_id, amount, staker?)` | Signed (aktif pool ise kendi) | Token unfreeze et |
| 3 | `harvest_rewards(pool_id, staker?)` | Signed (aktif pool ise kendi) | Biriken odulleri al |
| 4 | `set_pool_reward_rate_per_block(pool_id, new_rate)` | Admin | Oran artir (azaltilamaz) |
| 5 | `set_pool_admin(pool_id, new_admin)` | Admin | Admin degistir |
| 6 | `set_pool_expiry_block(pool_id, new_expiry)` | Admin | Bitis blogunu uzat (kisaltilamaz) |
| 7 | `deposit_reward_tokens(pool_id, amount)` | Signed | Pool'a odul token'i yatir |
| 8 | `cleanup_pool(pool_id)` | Admin | Pool'u temizle, kalan odulleri al |

### Kritik Kisitlamalar

- `reward_rate_per_block` yalnizca ARTTIRILABILIR (`RewardRateCut` hatasi)
- `expiry_block` yalnizca UZATIILABILIR (`ExpiryCut` hatasi)
- Pool aktifken sadece kendi unstake/harvest yapabilir
- Staker varken cleanup_pool yapilamaz (`NonEmptyPool` hatasi)
- Pool account'i `PalletId::into_sub_account_truncating(pool_id)` ile turetilir

---

## 8. Kritik Sayisal Degerler

### Conviction-Voting

| Deger | Kaynak |
|-------|--------|
| `Conviction::None` = `capital / 10` (0.1x) | conviction.rs:118 |
| `Conviction::Locked1x` = `1x`, 1 periyot | conviction.rs:103 |
| `Conviction::Locked2x` = `2x`, 2 periyot | conviction.rs:104 |
| `Conviction::Locked3x` = `3x`, 4 periyot | conviction.rs:105 |
| `Conviction::Locked4x` = `4x`, 8 periyot | conviction.rs:106 |
| `Conviction::Locked5x` = `5x`, 16 periyot | conviction.rs:107 |
| `Conviction::Locked6x` = `6x`, 32 periyot | conviction.rs:108 |
| Support = `aye_capital / MaxTurnout` | types.rs:68-70 |
| Approval = `ayes / (ayes + nays)` | types.rs:72-78 |
| Delegasyon max derinligi = 1 | lib.rs:573-577 |
| `CONVICTION_VOTING_ID` = `b"pyconvot"` | lib.rs:67 |
| Lock = `MAX(tum oy miktarlari, prior_lock)` | vote.rs:287-294 |
| PriorLock = `MAX(yeni sure, eski sure)` + `MAX(yeni miktar, eski miktar)` | vote.rs:173-176 |

### Elections-Phragmen

| Deger | Kaynak |
|-------|--------|
| Secim kosulu: `block % TermDuration == 0` | lib.rs:296 |
| num_to_elect = `DesiredMembers + DesiredRunnersUp` | lib.rs:951 |
| `deposit_of(n) = VotingBondBase + VotingBondFactor * n` | lib.rs:787-790 |
| Prime secimi: Borda count, ilk oy `MaxVotesPerVoter` agirlik alir | lib.rs:1051-1075 |
| Kaybeden deposit slash: Her secimde | lib.rs:1097-1108 |
| RunnersUp sirasi: worst to best (son eleman best replacement) | lib.rs:697 |
| StorageVersion = 4 | lib.rs:195 |

### Asset-Rewards

| Deger | Kaynak |
|-------|--------|
| `PRECISION_SCALING_FACTOR = 4096` | lib.rs:118 |
| `reward_per_token += rate * blocks * 4096 / total_staked` | lib.rs:803-809 |
| `staker_rewards = amount * (rpt - rpt_paid) / 4096 + accumulated` | lib.rs:819-823 |
| Pool account: `PalletId.into_sub_account_truncating(pool_id)` | lib.rs:742-744 |
| Pool creation footprint: `Footprint::from_mel::<(PoolId, PoolInfoFor<T>)>()` | lib.rs:738 |

---

## 9. Test Analizi

### Conviction-Voting Tests (tests.rs)

**Temel Oy Testleri:**

`basic_voting_works` testi (satir 242-298) kritik gozlemler:
- `aye(2, 5)` = 2 balance, Locked5x conviction
  - Tally: `ayes = 2*5 = 10`, `support = 2` -> `Tally::from_parts(10, 0, 2)`
- `aye(5, 1)` = 5 balance, Locked1x
  - Tally: `ayes = 5*1 = 5`, `support = 5` -> `from_parts(5, 0, 5)`
- `aye(10, 0)` = 10 balance, None conviction
  - Tally: `ayes = 10/10 = 1`, `support = 10` -> `from_parts(1, 0, 10)`

**Lock Süresi Testi:**

`successful_conviction_vote_balance_stays_locked_for_correct_time` (satir 448-466):
- `VoteLockingPeriod = 3` (test config)
- Referendum block 3'te bitti (approved=true)
- Conviction i icin: `unlock_at = 3 + 3 * (2^(i-1))`
  - i=1: `3 + 3*1 = 6` blok
  - i=2: `3 + 3*2 = 9` blok
  - i=3: `3 + 3*4 = 15` blok
  - vb.
- Test: `block >= (3 << (i-1)) + 3` sartini kontrol ediyor

**Delegasyon Testi:**

`classwise_delegation_works` (satir 470-589):
- Account 1 class 0,1,2 icin farkli hesaplara delegasyon yapiyor
- Her class bagimsiz calistiyor
- Delegasyon hedefi oy kullandiginda, delegator'un bakiyesi de hesaba katiliyor
- Tally: `from_parts(6, 2, 15)` -> delegasyon dahil toplam

Test Config:
- `VoteLockingPeriod = ConstU64<3>`
- `MaxVotes = ConstU32<3>`
- Classes = `[0, 1, 2]`

---

## 10. Nova Shield Chain Icin Cikarsimlar

### Governance Tasarimi

**Conviction Sistemi Benimsemesi:**
- Conviction sistemi Nova Shield icin uygun: token sahibi ne kadar cok kilitlemeye hazirsa o kadar cok oy hakki
- `Conviction::None` (0.1x) cekimserler icin ideal (oy kullanmak isteyenler ama kilitlemek istemeyenler)
- `Locked6x = 32 periyot` cok uzun kilit süresi -- `VoteLockingPeriod` parametresini dikkatli sec

**Kritik Implementasyon Notlari:**
1. `support` = sadece aye + abstain capital (nay dahil degil) -- OpenGov track eşiklerini buna gore ayarla
2. Delegasyon yalnizca 1 seviye -- daha derin zincir gerekiyorsa ozel implementasyon gerekli
3. Lock `MAX` operasyonu: kullanicilara birden fazla oyda lock biriktigini anlat
4. PriorLock `MAX(sure, miktar)`: Kisa süreli yuksek miktarli oy + uzun süreli az miktarli oy -> MAX her ikisinden alinir

### Phragmen Elections

**Ne Zaman Kullanilmali:**
- Counc veya komite sectigi durumlarda (5-20 kisi)
- Staking'den bagimsiz governance uyeleri icin
- Polkadot'taki Techincal Committee, Council benzeri yapilar icin

**Dikkat Noktalari:**
- Her secimde kaybedenlerin depositosu SLASH edilir -- mevduati dikkatli sec
- `TermDuration = 0` ise hic secim olmaz (emergency kapatma icin kullanisli)
- MaxVoters limiti asildiysa electionError --> uygun sinirlar koy
- Prime uye mekanizmasi: en cok oy alan degil, **pozisyon agirlikli** en cok oy alan

**Nova Shield Icin Oneri:**
- `CandidacyBond`: Spam'i engellemek icin yeterince yuksek
- `VotingBondFactor`: Her oy icin kucuk maliyet (32 byte depolama = VotingBondFactor)
- `DesiredRunnersUp >= 2`: Member dusuyor olursa yedegin olmasi icin

### Asset-Rewards (Staking Rewards)

**LP Token Reward Sistemi Icin:**
- DEX LP tokenleri icin incentive programlari
- Synthetix algoritmasinin O(1) ozelligi buyuk avantaj
- `PRECISION_SCALING_FACTOR = 4096` -- overflow riski icin Balance tipini dikkat sec

**Implementasyon Notlari:**
1. Pool aktifken sadece kendi unstake/harvest yapabilir -- kullanici UX'i buna gore tasarla
2. `reward_rate` yalnizca arttirilabilir -- pool yaratirken muhafazakar bas
3. `expiry_block` yalnizca uzatiilabilir -- pool süresi bitince yeni pool acmak gerekir
4. Pool cleanup: tüm stakerler cikmadan cleanup yapilamaz -- forced unstake mekanizmasi olmayabilir

**Nova Shield DEX Notu:**
- AMM LP token'lari icin reward pool mükemmel uyuyor
- `staking_asset = LP token`, `reward_asset = NOVA token`
- Her pool'un kendi hesabi var: auditing kolayligi
- Expiry mekanizmasi: Governance ile kontrollü reward desteklemesi

### Election Provider Multi-Phase

**Ne Zaman Kullanilmali:**
- 100+ validator/nominator ile buyuk validator setleri
- Staking pallet'i ile birlikte kullanilir

**Nova Shield Icin:**
- Erken asamada basit on-chain election yeterli
- Buyume ile birlikte multi-phase migrasyon dusunulebilir
- Emergency phase: Fallback olarak Governance kontrollu cozum

### Genel Mimari Onerileri

1. **Governance Katmani:** conviction-voting + OpenGov tracks (pallet-referenda ile)
2. **Komite/Konsey:** elections-phragmen ile belirli periyotlarda secim
3. **LP Incentive:** asset-rewards ile stake eden LP token sahiplerine NOVA odul
4. **Validator Secimi:** multi-phase election provider (buyudukce)

### Guvensizlik Bulgulari

1. **Conviction-Voting Hook Asimetrisi (traits.rs:52-55):** `on_before_vote` basarisiz olursa oy kaydedilmez ama hook'un yaptigi storage degisiklikleri KALIR. Nova Shield kendi VotingHooks implementasyonunda bunu dikkate almali.

2. **MaxVotes Limiti:** Bir account `MaxVotes` kadar oyda bulunabilir. Bu limitin uzerinde `MaxVotesReached` hatasi -- kullanici deneyimi icin yeterince buyuk tut ama weight hesaplamasini da goz onunde bulundur.

3. **Phragmen Kaybedenler Her Zaman Slash:** Bir onceki donemde uye olan birisi bir sonraki donemde secilemezse depositosunu kaybeder. Bu tasarim kasitli (uzun vadeli bagliligi tesvik etmek icin).

4. **Asset-Rewards Pool Bosu:** Pool aktif olsa bile `harvest_rewards` cagrildiginda pool hesabinda yeterli token olmayabilir. Havuz operatoru bu riski yonetmlidir.
