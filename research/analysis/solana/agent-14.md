# Solana Consensus / Tower BFT Analizi — Agent 14

**Analiz Tarihi:** 2026-07-22
**Kapsam:** core/src/consensus/, consensus.rs, cluster_slots_service/
**Analiz Eden:** Agent 14
**Toplam Dosya Sayisi:** 11 dosya

---

## Ozet

Bu analiz Solana'nin Tower BFT consensus mekanizmasinin tam implementasyonunu kapsar. Tower BFT; PBFT turevi olmayan, Proof of History ile entegre, lockout bazli bir BFT varyantdir. Cekirdek yapi sudur:

1. `consensus.rs` — Tower struct, lockout hesaplama, switch threshold, TowerError
2. `consensus/fork_choice.rs` — ForkChoice trait
3. `consensus/heaviest_subtree_fork_choice.rs` — agirlik bazli fork secimi
4. `consensus/vote_stake_tracker.rs` — stake agirlikli oy takibi
5. `consensus/latest_validator_votes_for_frozen_banks.rs` — gossip/replay oy takibi
6. `consensus/progress_map.rs` — fork ilerleme haritasi
7. `consensus/tree_diff.rs` — agac fark hesaplama
8. `consensus/tower_storage.rs` — Tower kalici depolama
9. `consensus/tower1_14_11.rs` — Tower v1.14.11 formatı
10. `consensus/tower1_7_14.rs` — Tower v1.7.14 formati (eski)
11. `cluster_slots_service.rs` + `cluster_slots_service/cluster_slots.rs` — cluster slot takip servisi

---

## 1. consensus.rs — Ana Consensus Modulu

**Konum:** `core/src/consensus.rs`
**Satir Sayisi:** ~1700+ satir

### Sabitler (satır 146-148)

```rust
const VOTE_THRESHOLD_DEPTH_SHALLOW: usize = 4;
pub const VOTE_THRESHOLD_DEPTH: usize = 8;
pub const SWITCH_FORK_THRESHOLD: f64 = 0.38;
```

**Aciklama:**
- `VOTE_THRESHOLD_DEPTH_SHALLOW = 4`: Sığ lockout kontrolü icin kullanilir (4. derinlik, SWITCH_FORK_THRESHOLD=%38 ile kontrol edilir)
- `VOTE_THRESHOLD_DEPTH = 8`: Ana threshold derinligi. Tower'in 8 derinligindeki oyun %66.7 stake gormuş olmasi gerekir
- `SWITCH_FORK_THRESHOLD = 0.38`: Fork gecisi icin gereken minimum locked-out stake orani (%38)

### Type Tanimlari (satir 150-154)

```rust
pub type Result<T> = std::result::Result<T, TowerError>;
pub type Stake = u64;
pub type VotedStakes = HashMap<Slot, Stake>;
pub type PubkeyVotes = Vec<(Pubkey, Slot)>;
```

---

### Enum: ThresholdDecision (satir 53-64)

```rust
#[derive(PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum ThresholdDecision {
    #[default]
    PassedThreshold,
    FailedThreshold(/* vote depth */ u64, /* Observed stake */ u64),
}
```

**Aciklama:**
- Vote threshold testinin sonucunu temsil eder
- `PassedThreshold`: Yeterli stake var, oy verilebilir
- `FailedThreshold(depth, stake)`: Hangi derinlikte ve ne kadar stake goruldugunu bildirir
- `.passed()` metodu: `PassedThreshold` mi diye kontrol eder

---

### Enum: SwitchForkDecision (satir 66-75)

```rust
#[derive(PartialEq, Eq, Clone, Debug, AbiExample)]
pub enum SwitchForkDecision {
    SwitchProof(Hash),
    SameFork,
    FailedSwitchThreshold(/* Switch proof stake */ u64, /* Total stake */ u64),
    FailedSwitchDuplicateRollback(Slot),
}
```

**Aciklama:**
- `SwitchProof(Hash)`: Baska bir fork'a gecis icin yeterli locked-out stake ispatlanmis
- `SameFork`: Ayni fork'ta kaliniyor, ispat gerekmez
- `FailedSwitchThreshold(u64, u64)`: %38 locked-out orani saglanamadi
- `FailedSwitchDuplicateRollback(Slot)`: Duplicate blok nedeniyle geri alma gerektiriyor

**Onemli Metod — to_vote_instruction() (satir 78-134):**
Karar tipine gore uygun vote instruction uretir:
- `Vote` icin `vote_instruction::vote()` veya `vote_instruction::vote_switch()`
- `VoteStateUpdate` icin `update_vote_state()` veya `update_vote_state_switch()`
- `CompactVoteStateUpdate` icin `compact_update_vote_state()` veya `compact_update_vote_state_switch()`
- `FailedSwitchThreshold` ve `FailedSwitchDuplicateRollback` — `None` doner, oy verilmez

---

### Enum: TowerVersions (satir 166-171)

```rust
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum TowerVersions {
    V1_17_14(Tower1_7_14),
    V1_14_11(Tower1_14_11),
    Current(Tower),
}
```

**Aciklama:** Geriye donuk uyumluluk icin versiyon kontrolu. `convert_to_current()` metodu ile eski versiyonlar mevcut versiyona donusturulebilir.

Donusum sureci:
- `V1_17_14` → `VoteState1_14_11` → `VoteStateVersions::V1_14_11` → current
- `V1_14_11` → dogrudan current
- `Current` → kendisi

---

### Enum: BlockhashStatus (satir 213-222)

```rust
#[derive(PartialEq, Eq, Debug, Default, Clone, Copy, AbiExample)]
pub(crate) enum BlockhashStatus {
    #[default]
    Uninitialized,  // restart'tan bu yana oy yok
    NonVoting,      // oy kullanmayan validator
    Blockhash(Hash),// basarili sekilde vote tx olusturuldu
}
```

---

### Struct: Tower (satir 224-250) — ANA YAPI

```rust
#[frozen_abi(digest = "iZi6s9BvytU3HbRsibrAD71jwMLvrqHdCjVk6qKcVvd")]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
pub struct Tower {
    pub node_pubkey: Pubkey,
    threshold_depth: usize,           // privat — default 8
    threshold_size: f64,              // privat — default 2/3
    pub(crate) vote_state: VoteState, // lockout gecmisi
    last_vote: VoteTransaction,       // son oy islemi
    #[serde(skip)]
    last_vote_tx_blockhash: BlockhashStatus,
    last_timestamp: BlockTimestamp,
    #[serde(skip)]
    stray_restored_slot: Option<Slot>,      // validator restart edge case
    #[serde(skip)]
    pub last_switch_threshold_check: Option<(Slot, SwitchForkDecision)>,
}
```

**Tasarim Notlari:**
- `#[frozen_abi]`: ABI dondurulmust, degistirilirse derleme hatasi olusur
- `#[serde(skip)]` alanlar: Yeniden hesaplanabilir bilgiler, disk'e yazilmaz
- `stray_restored_slot`: SlotHistory'de bulunmayan eski oylar icin edge case; slashing olmaksizin yeniden baslama saglar
- `last_switch_threshold_check`: Ayni switch slot icin tekrar hesaplamadan onlemek amaciyla cache'lenir

---

### Struct: ComputedBankState (satir 156-164) — crate-internal

```rust
pub(crate) struct ComputedBankState {
    pub voted_stakes: VotedStakes,
    pub total_stake: Stake,
    pub fork_stake: Stake,
    pub lockout_intervals: LockoutIntervals,
    pub my_latest_landed_vote: Option<Slot>,
}
```

**Aciklama:** Bir banka (slot) icin hesaplanan stake bilgileri. `collect_vote_lockouts()` tarafindan doldurulur.

---

### Tower Impl — Onemli Metodlar

#### new() (satir 272-284)
```rust
pub fn new(
    node_pubkey: &Pubkey,
    vote_account_pubkey: &Pubkey,
    root: Slot,
    bank: &Bank,
) -> Self
```
Banka'dan mevcut oy durumu okunarak Tower olusturulur. `initialize_lockouts_from_bank()` cagirilir.

#### collect_vote_lockouts() (satir 343-492) — KRITIK FONKSIYON

```rust
pub(crate) fn collect_vote_lockouts(
    vote_account_pubkey: &Pubkey,
    bank_slot: Slot,
    vote_accounts: &VoteAccountsHashMap,
    ancestors: &HashMap<Slot, HashSet<Slot>>,
    get_frozen_hash: impl Fn(Slot) -> Option<Hash>,
    latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
) -> ComputedBankState
```

**Islem Akisi:**
1. Tum vote account'lari iterate eder (stake = 0 olanlar atlanir)
2. Her oy icin `lockout_intervals` agaci guncellenir: `[vote_slot, vote_slot + lockout]`
3. Kendi vote account pubkey'i icin `my_latest_landed_vote` belirlenir
4. `process_slot_vote_unchecked()` ile `bank_slot` simule edilir
5. Simule oydan onceki son oy'un atasi olarak `update_ancestor_voted_stakes()` cagirilir
6. `populate_ancestor_voted_stakes()` ile `voted_stakes` tamamlanir
7. `fork_stake` = bank_slot'un en buyuk atasindaki stake

#### is_slot_confirmed() (satir 494-504)
```rust
pub(crate) fn is_slot_confirmed(
    &self, slot: Slot, voted_stakes: &VotedStakes, total_stake: Stake,
) -> bool {
    voted_stakes.get(&slot)
        .map(|stake| (*stake as f64 / total_stake as f64) > self.threshold_size)
        .unwrap_or(false)
}
```
Bir slot'un supermajority (>%66.7) stake aldiysa confirmed sayilir.

#### is_slot_duplicate_confirmed() (satir 506-516)
Benzer mantik, ancak `DUPLICATE_THRESHOLD` kullanir (replay_stage'den gelir, daha yuksek oran).

#### record_bank_vote() (satir 579-583)
```rust
pub fn record_bank_vote(&mut self, bank: &Bank) -> Option<Slot>
```
Banka slot'u icin oy kaydeder, yeni root olusursa `Some(new_root)` doner.

#### record_bank_vote_and_update_lockouts() (satir 602-632) — LOCKOUT GUNCELLEME
```rust
fn record_bank_vote_and_update_lockouts(
    &mut self, vote_slot: Slot, vote_hash: Hash,
) -> Option<Slot>
```
1. `Vote::new(vec![vote_slot], vote_hash)` olusturur
2. `process_vote_unchecked()` ile VoteState guncellenir (lockout'lar ilerletilir)
3. `update_last_vote_from_vote_state()` ile `last_vote` guncellenir
4. Yeni root olusursa doner

**Not:** Hata durumunda `panic!` atar — bu kasitlidir. Yerel tower'a oy kaydi asla basarisiz olmamalidir.

#### make_check_switch_threshold_decision() (satir 766-1069) — EN UZUN FONKSIYON

```rust
fn make_check_switch_threshold_decision(
    &self,
    switch_slot: Slot,
    ancestors: &HashMap<Slot, HashSet<u64>>,
    descendants: &HashMap<Slot, HashSet<u64>>,
    progress: &ProgressMap,
    total_stake: u64,
    epoch_vote_accounts: &VoteAccountsHashMap,
    latest_validator_votes_for_frozen_banks: &LatestValidatorVotesForFrozenBanks,
    heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice,
) -> SwitchForkDecision
```

**Switch Fork Karar Algoritmasi — Adim Adim:**

1. Son oylanmis slot hash alinamiyorsa → `SameFork` (hic oy yok)
2. Duplicate ancestor kontrolu (heaviest_subtree_fork_choice.latest_invalid_ancestor):
   - Eger switch_slot gecersiz ancestorin strict atasiysa → `FailedSwitchDuplicateRollback`
   - Eger son oyumuzu icerecek mevcut slot purge edilmisse → `SwitchProof(Hash::default())` (serbest oy)
3. Ancestor/descendant haritasinda last_vote_ancestors alinamiyorsa:
   - `is_stray_last_vote()` → bos ancestors ile devam
   - Diger → `panic!`
4. `switch_slot == last_voted_slot || switch_slot_ancestors.contains(last_voted_slot)` → `SameFork`
5. `last_vote_ancestors.contains(switch_slot)` → hata ya da `suspended_decision`
6. Buraya ulasildiysa baska fork'a gecis gerekiyor, switching proof hesaplanir:
   - Her `candidate_slot` icin (descendants haritasindan):
     - Dondurulan/hesaplanan bankalar
     - `last_voted_slot`'un descendant'i olmayan
     - Root'tan buyuk slotlar
   - `lockout_intervals` icinde `last_voted_slot >= interval_end` olanlar aranir
   - Locked-out stake biriktirilir, `> SWITCH_FORK_THRESHOLD (%38)` → `SwitchProof`
7. Gossip oylari da kontrol edilir (`max_gossip_frozen_votes`)
8. Yeterli stake yoksa → `FailedSwitchThreshold(locked_out_stake, total_stake)`

#### check_vote_stake_thresholds() (satir 1158-1191) — VOTE THRESHOLD KONTROLU

```rust
pub fn check_vote_stake_thresholds(
    &self, slot: Slot, voted_stakes: &VotedStakes, total_stake: Stake,
) -> ThresholdDecision
```

**Kontroller:**
1. Sığ kontrol: derinlik=4, esik=%38 (SWITCH_FORK_THRESHOLD)
2. Derin kontrol: derinlik=8, esik=%66.7 (threshold_size)

Her ikisi de gececek `PassedThreshold`, herhangi biri basarisiz olursa `FailedThreshold(depth, stake)`.

#### check_vote_stake_threshold() — TEKIL THRESHOLD KONTROLU (satir 1110-1155)

```rust
fn check_vote_stake_threshold(
    threshold_vote: Option<&Lockout>,
    vote_state_before_applying_vote: &VoteState,
    threshold_depth: usize,
    threshold_size: f64,
    slot: Slot,
    voted_stakes: &HashMap<Slot, u64>,
    total_stake: u64,
) -> ThresholdDecision
```

**Algoritma:**
- `threshold_vote` yoksa tower yetince derin degil → `PassedThreshold`
- `voted_stakes` icinde threshold_vote slot'u yoksa → `FailedThreshold(depth, 0)`
- `lockout > threshold_size` → `PassedThreshold`
- Ozel durum: Bir slot birden fazla confirmation count yukselmesi yasadiysa (eski oylarda eslesen slot varsa) → `PassedThreshold` (bounce-back logic)

#### adjust_lockouts_after_replay() (satir 1244-1326) — RESTART DURUMUNDA LOCKOUT DUZELTME

```rust
pub fn adjust_lockouts_after_replay(
    mut self, replayed_root: Slot, slot_history: &SlotHistory,
) -> Result<Self>
```

**Durum Analizi:**
- `tower_root <= replayed_root` (normal durum): Son oy TooOld mu? Evet → hata. Hayir → `adjust_lockouts_with_slot_history()` + `initialize_root()`
- `tower_root > replayed_root` (gelecek tower): Sentetik SlotHistory ile devam edilir, `stray_restored_slot` atamasiz root guncellenmez

#### Enum: TowerError (satir 1482-1507)

```rust
#[derive(Error, Debug)]
pub enum TowerError {
    IoError(#[from] std::io::Error),
    SerializeError(#[from] bincode::Error),
    InvalidSignature,               // imza gecersiz
    WrongTower(String),             // yanlis validator icin tower
    TooOldTower(Slot, Slot),        // tower cok eski
    FatallyInconsistent(&'static str), // blockstore ile uyumsuzluk
    HardFork(Slot),                 // hard fork nedeniyle gecersiz
}
```

#### Enum: ExternalRootSource (satir 1541-1554)
```rust
pub enum ExternalRootSource {
    Tower(Slot),
    HardFork(Slot),
}
```

#### reconcile_blockstore_roots_with_external_source() (satir 1561-1615)

```rust
pub fn reconcile_blockstore_roots_with_external_source(
    external_source: ExternalRootSource,
    blockstore: &Blockstore,
    last_blockstore_root: &mut Slot,
) -> blockstore_db::Result<()>
```

Blockstore root'u tower/hardfork root ile uyumlu hale getirir. Beklenmedik kilitlenme sonrasi tutarlilik saglar.

---

### Test Fonksiyonlari (consensus.rs satir 1617+)

Test modulu cok buyuk, asagidaki onemli test fonksiyonlari icermektedir:

- `gen_stakes()`: Test icin VoteAccountsHashMap olusturur
- `test_to_vote_instruction()`: SwitchForkDecision → instruction donusumu
- `test_collect_vote_lockouts_no_epoch_stakes_missing()`: Lockout toplama
- `test_is_slot_confirmed_not_enough_stake_failure()`: Threshold kontrolu
- `test_check_vote_stake_thresholds_*`: Cesitli threshold senaryolari
- `test_switch_threshold_*`: Switch fork karar senaryolari
- `test_adjust_lockouts_after_replay_*`: Restart sonrasi duzeltme

---

## 2. consensus/fork_choice.rs — ForkChoice Trait

**Konum:** `core/src/consensus/fork_choice.rs`
**Satir Sayisi:** 52 satir

### Struct: SelectVoteAndResetForkResult (satir 16-20)

```rust
pub struct SelectVoteAndResetForkResult {
    pub vote_bank: Option<(Arc<Bank>, SwitchForkDecision)>,
    pub reset_bank: Option<Arc<Bank>>,
    pub heaviest_fork_failures: Vec<HeaviestForkFailures>,
}
```

**Aciklama:**
- `vote_bank`: Oy verilecek bank ve hangi karar tipiyle (SwitchProof/SameFork)
- `reset_bank`: PoH sıfırlanacak bank (oy verilmeden)
- `heaviest_fork_failures`: Neden oy verilemediginin listesi

### Trait: ForkChoice (satir 22-52)

```rust
pub trait ForkChoice {
    type ForkChoiceKey;

    fn compute_bank_stats(
        &mut self,
        bank: &Bank,
        tower: &Tower,
        latest_validator_votes_for_frozen_banks: &mut LatestValidatorVotesForFrozenBanks,
    );

    fn select_forks(
        &self,
        frozen_banks: &[Arc<Bank>],
        tower: &Tower,
        progress: &ProgressMap,
        ancestors: &HashMap<u64, HashSet<u64>>,
        bank_forks: &RwLock<BankForks>,
    ) -> (Arc<Bank>, Option<Arc<Bank>>);

    fn mark_fork_invalid_candidate(&mut self, invalid_slot: &Self::ForkChoiceKey);

    fn mark_fork_valid_candidate(
        &mut self, valid_slot: &Self::ForkChoiceKey,
    ) -> Vec<Self::ForkChoiceKey>;
}
```

**select_forks() donus degerleri:**
1. En agir genel bank (heaviest overall bank)
2. Son oyun oldugu fork'taki en agir bank (switching proof gerekmeden oylanabilir)

---

## 3. consensus/heaviest_subtree_fork_choice.rs — Agirlik Bazli Fork Secimi

**Konum:** `core/src/consensus/heaviest_subtree_fork_choice.rs`
**Satir Sayisi:** ~1300+ satir

### Tip Tanimlari (satir 28-32)

```rust
pub type ForkWeight = u64;
pub type SlotHashKey = (Slot, Hash);
type UpdateOperations = BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation>;

const MAX_ROOT_PRINT_SECONDS: u64 = 30;
```

### Enum: UpdateLabel (satir 34-50)

```rust
#[derive(PartialEq, Eq, Clone, Debug, PartialOrd, Ord)]
enum UpdateLabel {
    Aggregate,
    Add,
    MarkValid(Slot),    // kucukten buyuge siralama icin slot dahil
    MarkInvalid(Slot),  // kucukten buyuge siralama icin slot dahil
    Subtract,
}
```

**Not:** `BTreeMap` anahtari olarak kullanilir. `PartialOrd + Ord` implementasyonu siralama icin kritik — islemler onem sirasina gore uygulanir.

### Enum: UpdateOperation (satir 68-77)

```rust
#[derive(PartialEq, Eq, Clone, Debug)]
enum UpdateOperation {
    Add(u64),
    MarkValid(Slot),
    MarkInvalid(Slot),
    Subtract(u64),
    Aggregate,
}
```

`update_stake()` metodu Add/Subtract stake biriktirir, Aggregate/MarkValid/MarkInvalid icin panic!.

### Struct: ForkInfo (satir 91-115) — TEMEL AGAC DUGUMU

```rust
#[derive(Clone, Debug)]
struct ForkInfo {
    stake_voted_at: ForkWeight,          // tam bu slot icin oy veren stake
    stake_voted_subtree: ForkWeight,     // bu slot + alt agac icin toplam stake
    height: usize,                       // bu nodun agac yuksekligi
    best_slot: SlotHashKey,              // alt agactaki en agir slot
    deepest_slot: SlotHashKey,           // alt agactaki en derin slot
    parent: Option<SlotHashKey>,
    children: BTreeSet<SlotHashKey>,
    latest_invalid_ancestor: Option<Slot>, // duplicate olarak isaretlenen en yeni ata
    is_duplicate_confirmed: bool,          // duplicate confirmed mi?
}
```

**ForkInfo Metodlari:**

- `is_unconfirmed_duplicate(my_slot)`: Bu slot kendisi duplicate olarak mi isaretlendi?
- `is_candidate()`: `latest_invalid_ancestor.is_none()` — fork seciminde yer aliyor mu?
- `is_duplicate_confirmed()`: Duplicate confirmed mi?
- `set_duplicate_confirmed()`: Duplicate confirmed yap, `latest_invalid_ancestor = None`
- `update_with_newly_valid_ancestor(key, slot)`: Ata valid yapilinca alt dala da yansitilar
- `update_with_newly_invalid_ancestor(key, slot)`: Ata invalid yapilinca en yeni invalid ata guncellenir

### Trait GetSlotHash (satir 52-66)

```rust
pub trait GetSlotHash {
    fn slot_hash(&self) -> SlotHashKey;
}
impl GetSlotHash for SlotHashKey { ... }  // (slot, hash) icin dogrudan
impl GetSlotHash for Slot { ... }          // (slot, Hash::default()) ile
```

### Struct: HeaviestSubtreeForkChoice (satir 182-188)

```rust
#[derive(Debug, Clone)]
pub struct HeaviestSubtreeForkChoice {
    fork_infos: HashMap<SlotHashKey, ForkInfo>,
    latest_votes: HashMap<Pubkey, SlotHashKey>,
    tree_root: SlotHashKey,
    last_root_time: Instant,
}
```

**Aciklama:**
- `fork_infos`: (slot, hash) → ForkInfo haritasi; agacin tum dugumlerini icerer
- `latest_votes`: Her validator icin en son oyladigi (slot, hash)
- `tree_root`: Agacin koku (en eski bilinen slot)
- `last_root_time`: Log throttle icin

**Onemli Metodlar:**

#### new() ve new_from_frozen_banks() (satir 215-261)
- `new(tree_root)`: Yeni agac, sadece root dugumu ile baslar
- `new_from_frozen_banks(root, frozen_banks)`: Mevcut donuk bankalardan agac insasi (sirali, kucukten buyuge)
- `new_from_bank_forks(bank_forks)`: BankForks'dan olustur

#### best_overall_slot() ve deepest_overall_slot() (satir 301-307)
Root'un `best_slot` ve `deepest_slot` bilgisini doner.

#### max_by_weight() (satir 323-331)
```rust
pub fn max_by_weight(&self, slot1: SlotHashKey, slot2: SlotHashKey) -> std::cmp::Ordering
```
Iki fork'u stake'e gore karsilastirir. Esit stakede kucuk slot kazanir (daha eski = daha guvenilir).

#### add_votes() (satir 334-348)
```rust
pub fn add_votes<'a, 'b>(
    &'a mut self,
    pubkey_votes: impl Iterator<Item = impl Borrow<(Pubkey, SlotHashKey)> + 'b>,
    epoch_stakes: &HashMap<Epoch, EpochStakes>,
    epoch_schedule: &EpochSchedule,
) -> SlotHashKey
```
Yeni oylar geldiginde agaci gunceller ve en iyi slotu doner.

#### set_tree_root() (satir 354-370)
Yeni root atanir, eski root'a ulasilan ama yeni root'a ulasilamayan tum dugumler silinir.

#### purge_prune() (satir 375-415) — AGAC BUDAMA
```rust
pub fn purge_prune(&mut self, new_root: SlotHashKey) -> (Vec<SlotHashKey>, Vec<Self>)
```
- `< new_root` olan slotlar purge edilir
- `> new_root` olan ama `new_root`'un descendant'i olmayan alt agaclar ayrilir
- Donus: (purge edilen slotlar, ayirilan alt agaclar)

#### add_new_leaf_slot() (satir 441-489) — YAPRAK EKLEME
```rust
pub fn add_new_leaf_slot(&mut self, slot_hash_key: SlotHashKey, parent: Option<SlotHashKey>)
```
1. `fork_infos` haritasina yeni ForkInfo eklenir
2. Ebeveynin `children` setine eklenir
3. `propagate_new_leaf()` ile yukari dogru best_slot ve deepest_slot yayilir

#### is_best_child() (satir 491-518)
Cevap: Bu slot kardes nodlar arasinda en agir mi? Gecersiz (invalid) kardesler hesaba katilmaz.

#### is_deepest_child() (satir 520-553)
Yukseklik sonra stake sonra slot numarasina gore karsilastirir.

#### split_off() (satir 570-636) — ALT AGAC AYIRMA
```rust
pub fn split_off(&mut self, slot_hash_key: &SlotHashKey) -> Self
```
`slot_hash_key`'den baslayan alt agaci ayirir, ana agactan cikarir. Update operasyonlari uygulanir.

#### merge() (satir 643-669) — ALT AGAC BIRLESTIRME
`other` agaci `merge_leaf`'a bagli olarak ana agaca birlestir.

#### aggregate_slot() (satir 844-939) — AGIRLIK HESAPLAMA

```rust
fn aggregate_slot(&mut self, slot_hash_key: SlotHashKey)
```

**Algoritma:**
1. Tum cocuk nodelar iterate edilir
2. Her cocugun `stake_voted_subtree` toplanir (gecersiz cocuklar da dahil — onemli!)
3. En iyi cocuk: `is_candidate()` olan, en yuksek stake'li; esitlik durumunda kucuk slot
4. En derin cocuk: yukseklik > stake > kucuk slot
5. `is_duplicate_confirmed`: Herhangi bir cocuk duplicate confirmed ise parent da olur

**Onemli Tasarim Karari:** Gecersiz (duplicate) fork stake'i sayilir, ama `best_slot` hesabinda atlanir. Bu sayede o fork'a cok stake yatirmis validatorlerin fork degistirmesi engellenmez.

#### generate_update_operations() (satir 963-1079) — UPDATE OPERASYONU URETME

```rust
fn generate_update_operations<'a, 'b>(
    &'a mut self,
    pubkey_votes: impl Iterator<Item = impl Borrow<(Pubkey, SlotHashKey)> + 'b>,
    epoch_stakes: &HashMap<Epoch, EpochStakes>,
    epoch_schedule: &EpochSchedule,
) -> UpdateOperations
```

**Islem Akisi:**
1. Her (pubkey, yeni_slot) ciftini isle
2. Root'tan kucuk yeni oy slotlari → atla
3. Ayni batch'te ayni pubkey tekrar gelmesi → panic! (hata)
4. Eski oydan yeni oya gecis:
   - Eski slot icin `Subtract(stake)` + aggregate yukari dogru
   - Yeni slot icin `Add(stake)` + aggregate yukari dogru
5. Ayni slot, farkli hash (duplicate): kucuk hash kazanir

#### process_update_operations() (satir 1082-1097)

```rust
fn process_update_operations(&mut self, update_operations: UpdateOperations)
```

En buyukten en kucuge siralayarak (rev()) islemler uygulanir:
- `MarkValid` → `mark_fork_valid()`
- `MarkInvalid` → `mark_fork_invalid()`
- `Aggregate` → `aggregate_slot()`
- `Add(stake)` → `add_slot_stake()`
- `Subtract(stake)` → `subtract_slot_stake()`

#### ForkChoice impl on HeaviestSubtreeForkChoice (satir 1238-1310)

**compute_bank_stats():**
1. `take_votes_dirty_set(root)` ile kirli oylari al
2. `add_votes()` ile agaci guncelle
3. Datapoint ile en iyi slotu logla

**select_forks():**
1. `best_overall_slot()` → en agir bank
2. `heaviest_slot_on_same_voted_fork()` → ayni fork'taki en agir bank
3. Hash eslesmesi kontrol edilir (gecersiz duplicate durumunda `None` doner)

#### heaviest_slot_on_same_voted_fork() (satir 1131-1203)
```rust
fn heaviest_slot_on_same_voted_fork(&self, tower: &Tower) -> Option<SlotHashKey>
```
- Son oyun `is_candidate()` → `best_slot()` doner
- Son oy gecersiz fork'taysa → `deepest_slot()` doner (duplicate confirmation icin derin bloklara ihtiyac var)
- Stray slot ise → `None`

#### is_strict_ancestor() (satir 707-724)
```rust
pub fn is_strict_ancestor(
    &self, maybe_ancestor_key: &SlotHashKey, node_key: &SlotHashKey,
) -> bool
```
`maybe_ancestor_key == node_key` ise false (katı ata). Ata iterator ile kontrol edilir.

#### TreeDiff impl (satir 1223-1236)
`TreeDiff` trait implementasyonu: `children()` ve `contains_slot()` saglar.

---

## 4. consensus/vote_stake_tracker.rs — Stake Agirlikli Oy Takibi

**Konum:** `core/src/consensus/vote_stake_tracker.rs`
**Satir Sayisi:** 100 satir

### Struct: VoteStakeTracker (satir 3-7)

```rust
#[derive(Default)]
pub struct VoteStakeTracker {
    voted: HashSet<Pubkey>,  // oy kullananlar
    stake: u64,              // toplam stake
}
```

### Metodlar

#### add_vote_pubkey() (satir 14-38) — TEMEL METOD

```rust
pub fn add_vote_pubkey(
    &mut self,
    vote_pubkey: Pubkey,
    stake: u64,
    total_stake: u64,
    thresholds_to_check: &[f64],
) -> (Vec<bool>, bool)
```

**Donus Degeri:** `(reached_threshold_results, is_new)`
- `is_new`: Bu pubkey daha once gozlemlenmemisse true
- `reached_threshold_results[i]`: i'nci threshold bu ek ile YENi gecildiyse true

**Threshold Gecilme Kontrolu:**
```rust
let threshold_stake = (total_stake as f64 * threshold) as u64;
old_stake <= threshold_stake && threshold_stake < new_stake
```
Eski stake threshold'un altindaysa VE yeni stake threshold'u astiysa yeni gecildi sayilir.

**Onemli Nokta:** Ayni pubkey tekrar eklenirse hic bir sey degismez (`is_new = false`, tum thresholds = false).

**Kullanim:** Optimistic confirmation icin kullanilir. Genellikle iki threshold birlikte kontrol edilir:
- `VOTE_THRESHOLD_SIZE = 2/3` (supermajority)
- `0.0` (ilk oy)

### Test (satir 53-99)
- 10 validator, her biri 1 stake
- i=6'da supermajority (%70) gecilir → `is_confirmed_thresholds[0] = true`
- i=0'da 0.0 threshold gecilir → `is_confirmed_thresholds[1] = true`

---

## 5. consensus/latest_validator_votes_for_frozen_banks.rs

**Konum:** `core/src/consensus/latest_validator_votes_for_frozen_banks.rs`
**Satir Sayisi:** ~524 satir

### Struct: LatestValidatorVotesForFrozenBanks (satir 7-15)

```rust
#[derive(Default)]
pub struct LatestValidatorVotesForFrozenBanks {
    max_gossip_frozen_votes: HashMap<Pubkey, (Slot, Vec<Hash>)>,
    max_replay_frozen_votes: HashMap<Pubkey, (Slot, Vec<Hash>)>,
    fork_choice_dirty_set: HashMap<Pubkey, (Slot, Vec<Hash>)>,
}
```

**Aciklama:**
- `max_gossip_frozen_votes`: Gossip'ten gelen oy; tutarsiz olabilir
- `max_replay_frozen_votes`: Replay'den gelen oy; daha guvenilir
- `fork_choice_dirty_set`: Fork choice'in guncellenmesi gereken degisiklikler

**Desen:** Bir validator icin birden fazla hash olabilir (duplicate bloklar). `Vec<Hash>` bu durumu destekler.

### check_add_vote() (satir 20-89)

```rust
pub fn check_add_vote(
    &mut self,
    vote_pubkey: Pubkey,
    vote_slot: Slot,
    frozen_hash: Option<Hash>,
    is_replay_vote: bool,
) -> (bool, Option<Slot>)
```

**Donus:** `(vote_eklendi_mi, en_son_frozen_oy_slotu)`

**Karar Mantigi:**
1. `frozen_hash` `None` ise → sadece mevcut en yeni oy slotunu doner, ekleme yapilmaz
2. `frozen_hash` `Some` ise:
   - Mevcut oydan buyuk slot → guncelle, `fork_choice_dirty_set`'e ekle (sadece replay)
   - Ayni slot, farkli hash → duplicate olarak ekle, dirty set'e ekle
   - Eski/ayni slot, ayni hash → atla

### take_votes_dirty_set() (satir 91-103)

```rust
pub fn take_votes_dirty_set(&mut self, root: Slot) -> Vec<(Pubkey, SlotHashKey)>
```

`fork_choice_dirty_set`'i alir, root'tan kucuk oylari filtreler, (pubkey, (slot, hash)) listesi doner.

### max_gossip_frozen_votes() (satir 105-107)

Switch threshold hesaplamasinda gossip oylarini kullanmak icin kullanilir.

### Test Fonksiyonlari (satir 122-523)

7 senaryo test edilir:
1. Non-frozen bank → eklenmez
2. Frozen vote eklenir, tekrar eklenmez
3. Duplicate vote (ayni slot, farkli hash) → eklenir
4. Duplicate non-frozen → eklenmez
5. Yeni yuksek slot non-frozen → eklenmez
6. Yeni yuksek slot frozen → eklenir
7. Yeni pubkey → eklenir

---

## 6. consensus/progress_map.rs — Fork Ilerleme Haritasi

**Konum:** `core/src/consensus/progress_map.rs`
**Satir Sayisi:** ~600 satir

### Sabitler (satir 166-167)
```rust
pub const RETRANSMIT_BASE_DELAY_MS: u64 = 5_000;
pub const RETRANSMIT_BACKOFF_CAP: u32 = 6;
```

### Struct: ReplaySlotStats (satir 25-137)

```rust
#[derive(Default)]
pub struct ReplaySlotStats(ConfirmationTiming);
```

`ConfirmationTiming`'e Deref uygular. `report_stats()` metodu cok detayli datapoint uretir:
- `fetch_entries_time`, `fetch_entries_fail_time`
- `entry_poh_verification_time`, `entry_transaction_verification_time`
- `replay_time`, `execute_batches_us`
- Program bazinda (`per_program_timings`) istatistikler
- En yavas 5 program loglanir

### Struct: ValidatorStakeInfo (satir 140-154)

```rust
#[derive(Debug)]
pub struct ValidatorStakeInfo {
    pub validator_vote_pubkey: Pubkey,
    pub stake: u64,
    pub total_epoch_stake: u64,
}
```

### Struct: RetransmitInfo (satir 169-186)

```rust
#[derive(Debug)]
pub struct RetransmitInfo {
    pub(crate) retry_time: Instant,
    pub(crate) retry_iteration: u32,
}
```

**reached_retransmit_threshold():** Exponential backoff: `(1 << min(retry, 6)) * 5000 ms`

### Struct: ForkProgress (satir 188-201) — FORK DURUMU

```rust
pub struct ForkProgress {
    pub is_dead: bool,
    pub fork_stats: ForkStats,
    pub propagated_stats: PropagatedStats,
    pub replay_stats: Arc<RwLock<ReplaySlotStats>>,
    pub replay_progress: Arc<RwLock<ConfirmationProgress>>,
    pub retransmit_info: RetransmitInfo,
    pub num_blocks_on_fork: u64,
    pub num_dropped_blocks_on_fork: u64,
}
```

**new_from_bank():** Eger bu validatorun leader slot'uysa `ValidatorStakeInfo` doldurulur, propagation tracking baslar.

### Struct: ForkStats (satir 294-310)

```rust
#[derive(Debug, Clone, Default)]
pub struct ForkStats {
    pub fork_stake: Stake,
    pub total_stake: Stake,
    pub block_height: u64,
    pub has_voted: bool,
    pub is_recent: bool,
    pub is_empty: bool,
    pub vote_threshold: ThresholdDecision,
    pub is_locked_out: bool,
    pub voted_stakes: VotedStakes,
    pub is_supermajority_confirmed: bool,
    pub computed: bool,
    pub lockout_intervals: LockoutIntervals,
    pub bank_hash: Option<Hash>,
    pub my_latest_landed_vote: Option<Slot>,
}
```

`fork_weight()` = `fork_stake / total_stake`

### Struct: PropagatedStats (satir 319-330)

```rust
#[derive(Clone, Default)]
pub struct PropagatedStats {
    pub propagated_validators: HashSet<Pubkey>,
    pub propagated_node_ids: HashSet<Pubkey>,
    pub propagated_validators_stake: u64,
    pub is_propagated: bool,
    pub is_leader_slot: bool,
    pub prev_leader_slot: Option<Slot>,
    pub slot_vote_tracker: Option<Arc<RwLock<SlotVoteTracker>>>,
    pub cluster_slot_pubkeys: Option<Arc<RwLock<SlotPubkeys>>>,
    pub total_epoch_stake: u64,
}
```

Leader slot'un yeterince yayilip yayilmadigini takip eder. `add_node_pubkey()` node'un tum vote account'larini ekler.

### Struct: ProgressMap (satir 373-376)

```rust
#[derive(Default)]
pub struct ProgressMap {
    progress_map: HashMap<Slot, ForkProgress>,
}
```

`Deref + DerefMut` to `HashMap<Slot, ForkProgress>`.

**Onemli Metodlar:**
- `get_propagated_stats(slot)`: Slot'un propagation istatistikleri
- `get_fork_stats(slot)`: Slot'un fork istatistikleri
- `get_retransmit_info(slot)`: Yeniden iletim bilgisi
- `get_latest_leader_slot_must_exist(slot)`: En yakin leader slot
- `get_leader_propagation_slot_must_exist(slot)`: Leader propagation durumu
- `handle_new_root(bank_forks)`: Yeni root sonrasi eski slotlari temizle
- `set_supermajority_confirmed_slot(slot)`: Supermajority confirmed isaretle

---

## 7. consensus/tree_diff.rs — Agac Fark Hesaplama

**Konum:** `core/src/consensus/tree_diff.rs`
**Satir Sayisi:** 33 satir

### Trait: TreeDiff (satir 3-33)

```rust
pub trait TreeDiff<'a> {
    type TreeKey: 'a + Hash + PartialEq + Eq + Copy;
    type ChildIter: Iterator<Item = &'a Self::TreeKey>;

    fn children(&self, key: &Self::TreeKey) -> Option<Self::ChildIter>;
    fn contains_slot(&self, slot: &Self::TreeKey) -> bool;

    fn subtree_diff(&self, root1: Self::TreeKey, root2: Self::TreeKey) -> HashSet<Self::TreeKey>
}
```

**subtree_diff algoritması:**
`root1`'den ulasilabilen tum nodlari bul, ama `root2`'ye ulasinca dur (onu ve alt agacini dahil etme). `root1` mevcut degilse bos set doner.

**Kullanim:** `set_tree_root()` icinde eski root'tan yeni root'a ulasmayan nodlari bulmak icin kullanilir.

---

## 8. consensus/tower_storage.rs — Tower Kalici Depolama

**Konum:** `core/src/consensus/tower_storage.rs`
**Satir Sayisi:** ~423 satir

### Enum: SavedTowerVersions (satir 17-21)

```rust
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, AbiExample)]
pub enum SavedTowerVersions {
    V1_17_14(SavedTower1_7_14),
    Current(SavedTower),
}
```

**try_into_tower():**
1. imza dogrulama (`!t.signature.verify(...)`)
2. Bincode deserialize
3. `node_pubkey` kontrolu
4. `TowerVersions::convert_to_current()` ile mevcut versiyona ceviri

### Struct: SavedTower (satir 78-108)

```rust
#[frozen_abi(digest = "Gaxfwvx5MArn52mKZQgzHmDCyn5YfCuTHvp5Et3rFfpp")]
#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq, Eq, AbiExample)]
pub struct SavedTower {
    signature: Signature,
    data: Vec<u8>,
    #[serde(skip)]
    node_pubkey: Pubkey,
}
```

**new():** Tower → bincode serialize → keypair ile imzala. `Tower1_14_11` formatinda kaydeder.

### Trait: TowerStorage (satir 110-113)

```rust
pub trait TowerStorage: Sync + Send {
    fn load(&self, node_pubkey: &Pubkey) -> Result<Tower>;
    fn store(&self, saved_tower: &SavedTowerVersions) -> Result<()>;
}
```

### Struct: NullTowerStorage (satir 115-129)
Test icin null implementasyon. `load()` hep hata, `store()` hep OK.

### Struct: FileTowerStorage (satir 131-171)

```rust
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FileTowerStorage {
    pub tower_path: PathBuf,
}
```

**Dosya Adlari:**
- Yeni format: `tower-1_9-{node_pubkey}.bin`
- Eski format: `tower-{node_pubkey}.bin`

**store():**
1. `.bin.new` geçici dosyasi olustur
2. Bincode serialize et
3. `fs::rename()` ile atomik olarak yeniden adlandir

**NOT:** `file.sync_all()` kasitli olarak kullanilmamis — performans kritik, oy pipeline'i engellememek icin.

### Struct: EtcdTowerStorage (satir 218-367)

```rust
pub struct EtcdTowerStorage {
    client: tokio::sync::Mutex<etcd_client::Client>,
    instance_id: [u8; 8],
    runtime: tokio::runtime::Runtime,
}

pub struct EtcdTlsConfig {
    pub domain_name: String,
    pub ca_certificate: Vec<u8>,
    pub identity_certificate: Vec<u8>,
    pub identity_private_key: Vec<u8>,
}
```

**Distributed Locking Mekanizmasi:**
- `load()`:
  1. `{pubkey}/instance` anahtarina `instance_id` yaz (sahiplik iddiasi)
  2. `instance` == `instance_id` ise `{pubkey}/tower` anahtarini oku (atomic txn)
  3. Eger txn basarisiz → kilit kaybi hatasi
- `store()`:
  1. `instance` == `instance_id` ise `{pubkey}/tower` anahtarini yaz (atomic txn)
  2. Basarisizsa → kilit kaybi hatasi

**Tasarim:** Tek bir validator örneğinin etcd'ye yazmasını garanti eder. instance_id = Unix timestamp (8 byte LE).

---

## 9. consensus/tower1_14_11.rs ve tower1_7_14.rs — Eski Versiyon Formatlari

### Tower1_14_11 (satir 9-34)

```rust
#[frozen_abi(digest = "F83xHQA1wxoFDy25MTKXXmFXTc9Jbp6SXRXEPcehtKbQ")]
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, AbiExample)]
pub struct Tower1_14_11 {
    pub(crate) node_pubkey: Pubkey,
    pub(crate) threshold_depth: usize,
    pub(crate) threshold_size: f64,
    pub(crate) vote_state: VoteState1_14_11,
    pub(crate) last_vote: VoteTransaction,
    #[serde(skip)] pub(crate) last_vote_tx_blockhash: BlockhashStatus,
    pub(crate) last_timestamp: BlockTimestamp,
    #[serde(skip)] pub(crate) stray_restored_slot: Option<Slot>,
    #[serde(skip)] pub(crate) last_switch_threshold_check: Option<(Slot, SwitchForkDecision)>,
}
```

Mevcut `Tower` yapisina cok benzer, fark: `VoteState1_14_11` kullanir.

### Tower1_7_14 (satir 13-36)

```rust
#[frozen_abi(digest = "9Kc3Cpak93xdL8bCnEwMWA8ZLGCBNfqh9PLo1o5RiPyT")]
pub struct Tower1_7_14 {
    // ayni alanlar, ancak last_vote: Vote (eski tip)
    pub(crate) last_vote: Vote,  // Tower1_14_11: VoteTransaction
    ...
}
```

### SavedTower1_7_14 (satir 38-65)
Imza + veri icerir. `SavedTower` ile ayni desen.

---

## 10. cluster_slots_service.rs — Cluster Slot Takip Servisi

**Konum:** `core/src/cluster_slots_service.rs`
**Satir Sayisi:** 204 satir

### Tip Tanimlari (satir 20-21)

```rust
pub type ClusterSlotsUpdateReceiver = Receiver<Vec<Slot>>;
pub type ClusterSlotsUpdateSender = Sender<Vec<Slot>>;
```

### Struct: ClusterSlotsServiceTiming (satir 23-27)

```rust
#[derive(Default, Debug)]
struct ClusterSlotsServiceTiming {
    pub lowest_slot_elapsed: u64,
    pub process_cluster_slots_updates_elapsed: u64,
}
```

Her 2 saniyede bir `datapoint_info!("cluster_slots_service-timing", ...)` loglanir.

### Struct: ClusterSlotsService (satir 36-38)

```rust
pub struct ClusterSlotsService {
    t_cluster_slots_service: JoinHandle<()>,
}
```

**new() (satir 41-68):**
1. `initialize_lowest_slot()`: Blockstore'dan en dusuk slotu gossip'e bildir
2. `initialize_epoch_slots()`: Donuk bankalardaki slotlari gossip'e bildir
3. `"solClusterSlots"` thread'i baslatilir

**run() dongu mantigi (satir 74-137):**
1. `exit.load(Relaxed)` kontrol edilir
2. `cluster_slots_update_receiver.recv_timeout(200ms)` ile slot guncellemeleri beklenir
3. `update_lowest_slot()`: Her dongude en dusuk slot guncellenir
4. `process_cluster_slots_updates()`: Yeni slotlar gossip'e iletilir
5. `cluster_slots.update()`: Gossip'ten gelen slotlar islenir
6. Zamanlama istatistikleri loglanir

**process_cluster_slots_updates() (satir 139-153):**
- Mevcut slot listesini al
- Kanaldan kalan tum mesajlari bosal (try_recv loop)
- Sirala (stabil sort: `#[allow(clippy::stable_sort_primitive)]`)
- Bos degilse `cluster_info.push_epoch_slots()` ile gossip'e gonder

**initialize_lowest_slot() (satir 155-161):**
```
// Safe to set into gossip because by this time, the leader schedule cache should
// also be updated with the latest root (done in blockstore_processor)...
```
Blockstore'un en dusuk slotunu gossip'e gonderir.

**initialize_epoch_slots() (satir 167-178):**
Donuk bankalardaki slot numaralarini gossip'e bildirir. Henuz replay edilmemis slotlar dahil edilmez.

---

## 11. cluster_slots_service/cluster_slots.rs — Cluster Slot Veri Yapisi

**Konum:** `core/src/cluster_slots_service/cluster_slots.rs`
**Satir Sayisi:** ~200+ satir

### Sabit (satir 21)

```rust
const CLUSTER_SLOTS_TRIM_SIZE: usize = 524_288; // 512K
```

### Tip Tanimlari (satir 23)

```rust
pub(crate) type SlotPubkeys = HashMap</*node:*/ Pubkey, /*stake:*/ u64>;
```

### Struct: ClusterSlots (satir 25-32)

```rust
#[derive(Default)]
pub struct ClusterSlots {
    cluster_slots: RwLock<BTreeMap<Slot, Arc<RwLock<SlotPubkeys>>>>,
    validator_stakes: RwLock<Arc<NodeIdToVoteAccounts>>,
    epoch: RwLock<Option<u64>>,
    cursor: Mutex<Cursor>,
    last_report: AtomicInterval,
}
```

**Aciklama:**
- `cluster_slots`: Hangi node'un hangi slot'u gosterdigi bilgisi (gossip'ten)
- `validator_stakes`: Epoch bazinda node stake bilgisi
- `epoch`: Son guncellenen epoch (degisince validator_stakes guncellenir)
- `cursor`: Gossip CRDS cursor — yeni EpochSlots verilerini sirmalar
- `last_report`: Boyut istatistigi loglama throttle

**update() (satir 39-47):**
1. `update_peers()`: Epoch degistiyse validator stake'lerini guncelle
2. Gossip'ten `epoch_slots` listesini al (cursor ile)
3. `update_internal()` cagirilir

**update_internal() (satir 49-106):**
1. Her EpochSlots icin validator stake'ini ekle
2. Gecerli aralik: `(root+1)..root+2*epoch_slots`
3. Aralikta olmayan slotlari filtrele
4. Slot → [(node, stake)] haritasi olustur
5. Mevcut `cluster_slots` haritasina ekle
6. Root'tan onceki slotlari temizle (split_off)
7. `> 1.1 * 512K` ise uzak slotlari at (trimle)

**compute_weights() (satir 172-199) — ONARIM AGIRLIK HESABI**

```rust
pub(crate) fn compute_weights(&self, slot: Slot, repair_peers: &[ContactInfo]) -> Vec<u64>
```

Her repair peer icin agirlik hesaplar:
1. Oncelikle `validator_stakes`'ten peer stake'i al (+1 minimum)
2. `cluster_slots`'tan bu slot icin peer stake'i al
3. Agirlik = `(slot_peer_stake / 2 + total_stake / 2).max(1)`

Bu, hem genel stake hem de o slotu aktif olarak gosteren peerleri onceliklendirir.

**update_peers() (satir 156-170):**
Epoch degistiyse `root_bank.epoch_stakes(epoch).node_id_to_vote_accounts()` ile guncellenir.

### Test (cluster_slots_service.rs satir 181-203)

```rust
#[test]
pub fn test_update_lowest_slot() {
    // ClusterSlotsService::update_lowest_slot(5, &cluster_info) ile
    // gossip CRDS'de LowestSlot.lowest == 5 oldugunu dogrular
}
```

---

## Mimari Degerlendirmesi

### Tower BFT Temel Prensipleri

**Lockout Mekanizmasi:**
- Her oy N dogrulama sayisina ulasinceya kadar `2^N` slot kilitlenir
- 8 derinlikli tower = 2^8 = 256 slot lockout (maksimum)
- Kilitli slot farkli bir fork'ta ise o fork'a oy verilemez

**Threshold Kontrolleri:**
1. `VOTE_THRESHOLD_DEPTH = 8` + `threshold_size = 2/3`: Derin oylarin %66.7 stake almasi
2. `VOTE_THRESHOLD_DEPTH_SHALLOW = 4` + `SWITCH_FORK_THRESHOLD = 0.38`: 4. derinlik icin %38 yeterli

**Switch Fork:**
- Farkli bir fork'a gecmek icin %38 locked-out stake ispatlanmali
- Ispat: `descendants` haritasinda `[last_voted_slot, last_voted_slot + lockout]` araligindaki kilitli oylar
- Gossip oyları da switch proof icin kullanılabilir

### Performans Tasarim Kararlari

1. **Dosya sync yok:** `FileTowerStorage` `file.sync_all()` KULLANMAZ. Oy pipeline'ini bloklamamak icin bilinçli karar.

2. **BTreeMap UpdateOperations:** Islemler slot'a gore siralı uygulanır (buyukten kucuge). Bu aggregate operasyonlarinin dogru sirada uygulanmasini saglar.

3. **Lazy invalidation:** Gecersiz forklarin stake'i toplam `stake_voted_subtree`'e dahil edilir. Bu, gecersiz fork'taki validatorlerin fork degistirmesine olanak taniyan critical tasarim.

4. **Cursor tabanlı gossip okuma:** `Cluster slots` yalnizca yeni EpochSlots mesajlarini islenir (cursor ile), tum gecmisi tekrar taramaz.

5. **AtomicBool exit:** Thread kapatma kosulsuz `Relaxed` ordering ile kontrol edilir.

---

## Unsafe Bloklar

Bu dosyalarda **unsafe blok bulunmamaktadir.** Solana consensus katmani tamamen safe Rust ile yazilmistir.

---

## Nova Shield Chain icin Onemli Dersler

### 1. Tower BFT Uyarlamasi
Nova Shield icin ayni lockout mekanizmasi uygulanabilir:
- `VOTE_THRESHOLD_DEPTH = 8` ve `threshold_size = 2/3` ideal baslangic noktasi
- `SWITCH_FORK_THRESHOLD = 0.38` matematiksel olarak turetilmistir: `1 - 2/3 / 1.75 ≈ 0.38`

### 2. Dual-Track Oy Takibi
`LatestValidatorVotesForFrozenBanks`'in gossip/replay ikili yapisi:
- Gossip oylari switch proof icin kullanilir
- Replay oylari fork choice icin kullanilir
- Bu ayrim onemli: Gossip tutarsiz olabilir, replay guclu garanti verir

### 3. Versiyon Yonetimi
`TowerVersions` + `SavedTowerVersions` desen:
- Her buyuk versiyon icin ayri struct
- `convert_to_current()` ile kademeli donusum
- `#[frozen_abi]` ile ABI kilitleme

### 4. Distributed Tower Storage
`EtcdTowerStorage` kaliplari:
- Instance ID ile sahiplik kilitleme
- Atomic txn ile okuma/yazma tutarliligi
- Tek validator instance garantisi (cift-imzalama onlemi)

### 5. HeaviestSubtreeForkChoice Veri Yapisi
- `(Slot, Hash)` anahtari — Hash farkli duplicate bloklari ayirt eder
- Lazy propagation: `propagate_new_leaf()` sadece etkilenen dallari gunceller
- Separe best_slot ve deepest_slot: Konsensus icin en agir, blok uretimi icin en derin

### 6. Switch Threshold Hesaplama Optimizasyonu
`last_switch_threshold_check` cache'i:
- Ayni switch_slot icin hesaplama tekrarlanmaz
- `FailedSwitchThreshold` durumunda gercek zamanda kontrol edilir

### 7. Cluster Slots Boyut Kontrolu
`CLUSTER_SLOTS_TRIM_SIZE = 512K + %10 overshoot`:
- `10 * len > 11 * MAX` kontrolu ile amortize temizleme
- Her temizlemede tam sinire getirmek yerine overshoot ile temizleme sikligi azaltilir

### 8. Propagation Tracking
`PropagatedStats`:
- Leader slot'un %15 (SUPERMINORITY_THRESHOLD) stake'e ulasana kadar propagated sayilmamasini
- Hem vote pubkey hem node pubkey takibi
- Kendi stake'iniz propagated_validators_stake'e dahil

---

## Dosya Ozeti

| Dosya | Satir | Temel Amac |
|-------|-------|-----------|
| `consensus.rs` | ~1700 | Tower struct, lockout/switch hesaplama, TowerError |
| `consensus/fork_choice.rs` | 52 | ForkChoice trait tanımı |
| `consensus/heaviest_subtree_fork_choice.rs` | ~1350 | Ağırlık bazlı fork ağacı |
| `consensus/vote_stake_tracker.rs` | 100 | Threshold geçiş takibi |
| `consensus/latest_validator_votes_for_frozen_banks.rs` | ~524 | Gossip/replay oy kaydı |
| `consensus/progress_map.rs` | ~600 | Fork ilerleme + propagation istatistikleri |
| `consensus/tree_diff.rs` | 33 | Ağaç fark algoritması |
| `consensus/tower_storage.rs` | ~423 | Dosya + etcd tower depolama |
| `consensus/tower1_14_11.rs` | 35 | v1.14.11 uyumluluk formatı |
| `consensus/tower1_7_14.rs` | 65 | v1.7.14 uyumluluk formatı |
| `cluster_slots_service.rs` | 204 | Cluster slot güncelleme servisi |
| `cluster_slots_service/cluster_slots.rs` | ~200+ | Gossip tabanlı slot takibi |
