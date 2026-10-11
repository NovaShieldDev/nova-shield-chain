# Solana Programs — Kapsamli Kaynak Kod Analizi

Dizin: `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/`
Analiz tarihi: 2026-07-21

---

## DIZIN YAPISI

```
programs/
  system/                    — Temel hesap ve transfer islemleri
  vote/                      — Validator oylama sistemi
  stake/                     — Stake delegation ve odul sistemi
  config/                    — On-chain konfigürasyon deposu
  address-lookup-table/      — Versiyonlu adres tablosu (v0 tx)
  compute-budget/            — Compute unit / heapmik ayarlari (runtime)
  loader-v4/                 — SBF program deploy/retract/finalize
  bpf_loader/                — BPF syscall kati + CPI + serialization
  sbf/                       — SBF entegrasyon test programlari (40+)
  zk-token-proof/            — ElGamal / ZK ispat dogrulama
  *-tests/                   — Entegrasyon test kasalari
```

---

## 1. SYSTEM PROGRAM

### Dosyalar
- `system/src/lib.rs`
- `system/src/system_processor.rs`   (~540 satir + testler)
- `system/src/system_instruction.rs` (~230 satir + testler)

### SystemInstruction Enum (solana_sdk'dan import edilir)
```
CreateAccount            { lamports, space, owner }
CreateAccountWithSeed    { base, seed, lamports, space, owner }
Assign                   { owner }
AssignWithSeed           { base, seed, owner }
Transfer                 { lamports }
TransferWithSeed         { lamports, from_seed, from_owner }
Allocate                 { space }
AllocateWithSeed         { base, seed, space, owner }
AdvanceNonceAccount
WithdrawNonceAccount     (lamports)
InitializeNonceAccount   (authorized)
AuthorizeNonceAccount    (nonce_authority)
UpgradeNonceAccount
```

### `Address` Yapisal Karar
```rust
struct Address {
    address: Pubkey,
    base: Option<Pubkey>,   // Seed'den turetilen adreste base key
}

impl Address {
    fn is_signer(&self, signers: &HashSet<Pubkey>) -> bool {
        // Seed'li adres icin BASE imzalar, dogrudan adres icin kendisi imzalar
        if let Some(base) = self.base { signers.contains(&base) }
        else { signers.contains(&self.address) }
    }
}
```
Bu yapi seed'li adresler icin PDA-benzeri mantigi saglar: imza kontrolu base key'e yapilir, adresin kendisine degil.

### Fonksiyon Analizi: `allocate`
- Imza kontrolu: `address.is_signer(signers)?`
- Hesap bos mu?: `data.is_empty() && owner == system_program` — degilse `AccountAlreadyInUse`
- Boyut limiti: `space > MAX_PERMITTED_DATA_LENGTH` (10 MiB) — `InvalidAccountDataLength`
- Gecerli olunca: `account.set_data_length(space as usize)`

### Fonksiyon Analizi: `assign`
- Zaten ayni owner'sa erken cikis (no-op, hata yok)
- Imza kontrolu: `address.is_signer(signers)?`
- `account.set_owner(&owner.to_bytes())`

### Fonksiyon Analizi: `create_account`
1. `to` hesabinin lamport bakiyesi > 0 ise `AccountAlreadyInUse`
2. `allocate_and_assign` — once allocate, sonra assign
3. `transfer` — `from`'dan `to`'ya lamport tasima

### Fonksiyon Analizi: `transfer`
- Imza kontrolu: `instruction_context.is_instruction_account_signer(from_account_index)?`
- `transfer_verified` cagrisi:
  - `from.get_data().is_empty()` — `from` veri tasiyamaz (sadece native lamport)
  - `lamports > from.get_lamports()` — `ResultWithNegativeLamports`
  - `from.checked_sub_lamports(lamports)?`
  - `to.checked_add_lamports(lamports)?`

### Fonksiyon Analizi: `transfer_with_seed`
- `from_base` imza kontrolu
- `Pubkey::create_with_seed(base, seed, owner)` ile adres turetilir ve from adresiyle karsilastirilir
- Uyusmazsa `AddressWithSeedMismatch`

### Nonce Fonksiyonlari (system_instruction.rs)

#### `advance_nonce_account`
- `account.is_writable()` kontrolu
- State deserialize: `Versions` -> `State::Initialized(data)`
- `signers.contains(&data.authority)` — otorite imzasi gerekli
- `DurableNonce::from_blockhash(&invoke_context.blockhash)` ile yeni nonce uretilir
- Eger `data.durable_nonce == next_durable_nonce` ise `NonceBlockhashNotExpired` — slot basina bir kez
- `nonce::state::Data::new(authority, next_durable_nonce, lamports_per_signature)` yeni state

#### `withdraw_nonce_account`
- `from.is_writable()` kontrolu
- `State::Uninitialized`: lamport yetersizlik kontrolu, imza for `from_key`
- `State::Initialized`: tam cekim durumunda nonce expire kontrolu; kısmi cekim durumunda `lamports + min_balance <= from.get_lamports()` kontrolu
- `signer` (data.authority) imzasi zorunlu

#### `initialize_nonce_account`
- `is_writable()` kontrolu
- State `Uninitialized` olmali
- Rent exempt kontrolu: `account.get_lamports() >= min_balance`
- `DurableNonce::from_blockhash(...)` ile nonce olusturulur
- `Versions::new(State::Initialized(data))` ile kaydedilir

#### `authorize_nonce_account`
- Mevcut authority imzasi gerekli
- State `Initialized` olmali
- `data.authority = *nonce_authority` guncelleme

#### `UpgradeNonceAccount`
- Owner kontrolu: `system_program::check_id`
- `is_writable()` kontrolu
- `nonce_versions.upgrade()` — None donerse `InvalidArgument`

### DEFAULT_COMPUTE_UNITS: 150

### Test Modulu
- `test_create_account` — lamport ve owner transferi dogrular
- `test_create_account_with_seed` / `test_create_account_with_seed_separate_base_account`
- Nonce testleri: ilerletme, geri cekme, baslatma, yetkilendirme
- Guvensiz senaryolar: imzasiz islemler, yetersiz lamport, tekrar baslama

### SystemAccountKind (lib.rs)
```rust
pub enum SystemAccountKind { System, Nonce }

pub fn get_system_account_kind(account: &AccountSharedData) -> Option<SystemAccountKind> {
    if system_program::check_id(account.owner()) {
        if account.data().is_empty()           { Some(System) }
        else if data.len() == nonce::State::size() { /* Nonce deserialized check */ Some(Nonce) }
        else { None }
    } else { None }
}
```

---

## 2. VOTE PROGRAM

### Dosyalar
- `vote/src/lib.rs`
- `vote/src/vote_processor.rs`  (~234 satir + testler)
- `vote/src/vote_state/mod.rs`  (~900+ satir + testler)
- `vote/src/vote_transaction.rs`

### VoteInstruction Enum (solana_program'dan import edilir)
```
InitializeAccount(VoteInit)
Authorize(Pubkey, VoteAuthorize)
AuthorizeChecked(VoteAuthorize)
AuthorizeWithSeed(VoteAuthorizeWithSeedArgs)
AuthorizeCheckedWithSeed(VoteAuthorizeCheckedWithSeedArgs)
UpdateValidatorIdentity
UpdateCommission(u8)
Vote(Vote)
VoteSwitch(Vote, Hash)
UpdateVoteState(VoteStateUpdate)
UpdateVoteStateSwitch(VoteStateUpdate, Hash)
CompactUpdateVoteState(VoteStateUpdate)
CompactUpdateVoteStateSwitch(VoteStateUpdate, Hash)
Withdraw(u64)
```

### Birinci Guvenslik Kati — Owner Kontrolu
```rust
if *me.get_owner() != id() {
    return Err(InstructionError::InvalidAccountOwner);
}
```
Her instruction'da ilk is bu kontrol. Program kendi sahip olmadigi hesaplara dokunmaz.

### DEFAULT_COMPUTE_UNITS: 2100 (statik)

### Instruction Analizi

#### `InitializeAccount(vote_init)`
- Rent sysvar: `rent.is_exempt(lamports, data_len)` — yeterli lamport yoksa `InsufficientFunds`
- Clock sysvar
- `vote_state::initialize_account(&mut me, &vote_init, &signers, &clock, &feature_set)`

#### `Authorize(voter_pubkey, vote_authorize)`
- Clock sysvar (account[1])
- `vote_state::authorize(...)` — yeni voter veya withdrawer ataması

#### `AuthorizeChecked(vote_authorize)`
- 4 hesap zorunlu
- `instruction_context.is_instruction_account_signer(3)?` — yeni otorite imzalamali
- Ayni `vote_state::authorize` cagrisi

#### `AuthorizeWithSeed(args)`
- `process_authorize_with_seed_instruction` yardimci fonksiyon:
  - `base_pubkey` ile `Pubkey::create_with_seed(base, seed, owner)` -> `expected_authority_keys`
  - `vote_state::authorize` bu setle cagrilir

#### `UpdateValidatorIdentity`
- `node_pubkey` = account[1]
- `vote_state::update_validator_identity` — eski ve yeni node imza zorunlu

#### `UpdateCommission(commission)`
- Epoch schedule + clock sysvar cache'den cekilir
- `vote_state::update_commission` — epoch ortasinda degisiklik kısıtlaması icerir

#### `Vote(vote)` / `VoteSwitch(vote, _)`
- SlotHashes ve Clock sysvar
- `vote_state::process_vote_with_account`

#### `UpdateVoteState(update)` / `CompactUpdateVoteState(update)`
- `process_vote_state_update` — tower tutarliligi tamamen dogrulanan yeni path

#### `Withdraw(lamports)`
- 2 hesap: kaynak[0], hedef[1]
- Rent + clock sysvar cache
- `vote_state::withdraw` — rent exempt alt sinir, komisyon denetimi

### VoteTransaction Enum
```rust
pub enum VoteTransaction {
    Vote(Vote),
    VoteStateUpdate(VoteStateUpdate),
    CompactVoteStateUpdate(VoteStateUpdate),
}
```
Her varyant `slots()`, `slot(i)`, `len()`, `hash()`, `timestamp()` metotlari saglar.

### `check_update_vote_state_slots_are_valid` — Cok Detayli Analiz

Bu fonksiyon vote tower guncelleme guvenliginin cekirdegi:

1. Bos lockout listesi -> `VoteError::EmptySlots`
2. Son slot <= mevcut son oy -> `VoteError::VoteTooOld`
3. Slot hashes bos -> `VoteError::SlotsMismatch`
4. Son slot < en eski slot hash -> `VoteError::VoteTooOld`
5. Onerilen root < en eski slot hash ise: root, vote_state'in root ile ustune yazilir, daha yenisi secilir
6. Cift indisli tarama (vote_state_update_index ile slot_hashes_index):
   - `Less` (slot hash henuz bulunamadi): eger slot tarihi oncesinde ise ve vote state'de yoksa filtrele; aksi halde `SlotsMismatch` veya `RootOnDifferentFork`
   - `Greater`: slot_hashes geriden geliyor, decrement
   - `Equal`: Hem vote hem hash eslesti, ikisini de ilerlet
7. Tum oylarin eslesmesi zorunlu
8. Son slotin hash'i `VoteStateUpdate.hash` ile eslesmeli -> `SlotHashMismatch`

### `process_new_vote_state` — Tower Dogrulama
1. `new_state.len() > MAX_LOCKOUT_HISTORY` -> `TooManyVotes`
2. `new_root < current_root` -> `RootRollBack`
3. Her oy icin:
   - `confirmation_count == 0` -> `ZeroConfirmations`
   - `confirmation_count > MAX_LOCKOUT_HISTORY` -> `ConfirmationTooLarge`
   - `slot <= new_root` -> `SlotSmallerThanRoot`
   - Oncekine gore siralama ve azalan konfirmasyon -> `SlotsNotOrdered`, `ConfirmationsNotOrdered`
   - Lockout cakismasi -> `NewVoteStateLockoutMismatch`
4. Mevcut state ile karsilastirma:
   - `Less`: `current_vote.lockout.last_locked_out_slot() >= new_vote.slot()` -> `LockoutConflict`
   - `Equal`: `new_vote.confirmation_count < current_vote.confirmation_count` -> `ConfirmationRollBack`; latency kopyalanir
5. Timely vote credits: `VoteState::compute_vote_latency(slot, current_slot)` ile latency hesabi
6. Krediler: rooted her slot icin `vote_state.increment_credits(epoch, earned_credits)`

### `authorize` (vote_state)
- `VoteAuthorize::Voter`: Mevcut authorized_withdrawer veya voter imzalamali; `set_new_authorized_voter` epoch+1 icin zamanlanir
- `VoteAuthorize::Withdrawer`: Sadece mevcut withdrawer imzalamali; aninda degistirilir

### `update_commission`
```rust
// Komisyon degisikliklerine kısıtlama:
// feature_set kontrol edilir, epoch'un ikinci yarisinda artis yasaktir (eger aktif)
```

---

## 3. STAKE PROGRAM

### Dosyalar
- `stake/src/lib.rs`
- `stake/src/stake_instruction.rs`  (~372 satir + 500+ satir test)
- `stake/src/stake_state.rs`        (~700+ satir logic + testler)
- `stake/src/rewards.rs`
- `stake/src/points.rs`
- `stake/src/config.rs`

### StakeInstruction Enum (solana_sdk'dan)
```
Initialize(Authorized, Lockup)
InitializeChecked
Authorize(Pubkey, StakeAuthorize)
AuthorizeChecked(StakeAuthorize)
AuthorizeWithSeed(AuthorizeWithSeedArgs)
AuthorizeCheckedWithSeed(AuthorizeCheckedWithSeedArgs)
DelegateStake
Split(u64)
Merge
Withdraw(u64)
Deactivate
DeactivateDelinquent
SetLockup(LockupArgs)
SetLockupChecked(LockupCheckedArgs)
GetMinimumDelegation
Redelegate
```

### DEFAULT_COMPUTE_UNITS: 750

### StakeStateV2 Enum (state machine)
```
Uninitialized
Initialized(Meta)
Stake(Meta, Stake, StakeFlags)
RewardsPool
```

### `Meta` Yapisi
```rust
Meta {
    rent_exempt_reserve: u64,
    authorized: Authorized { staker: Pubkey, withdrawer: Pubkey },
    lockup: Lockup { unix_timestamp, epoch, custodian },
}
```

### `Delegation` Yapisi
```rust
Delegation {
    voter_pubkey: Pubkey,
    stake: u64,
    activation_epoch: Epoch,
    deactivation_epoch: Epoch,  // MAX = aktif, baska = deaktive ediliyor
    warmup_cooldown_rate: f64,
}
```

### Birinci Guvenslik Kati
```rust
let get_stake_account = || {
    let me = instruction_context.try_borrow_instruction_account(transaction_context, 0)?;
    if *me.get_owner() != id() {
        return Err(InstructionError::InvalidAccountOwner);
    }
    Ok(me)
};
```

### `initialize`
- `data.len() != StakeStateV2::size_of()` -> `InvalidAccountData`
- State `Uninitialized` olmali
- `lamports >= rent_exempt_reserve` -> gecerli
- State: `Initialized(Meta { rent_exempt_reserve, authorized, lockup })`

### `InitializeChecked`
- Withdrawer'in account[3] oldugu dogrulanir
- `instruction_context.is_instruction_account_signer(3)?` — withdrawer imzalamali
- `Lockup::default()` (kilitlemesiz) ile `initialize`

### `authorize`
```rust
match stake_account.get_state()? {
    StakeStateV2::Stake(mut meta, stake, stake_flags) => {
        meta.authorized.authorize(signers, new_authority, stake_authorize, Some((&meta.lockup, clock, custodian)))?;
        stake_account.set_state(&StakeStateV2::Stake(meta, stake, stake_flags))
    }
    StakeStateV2::Initialized(mut meta) => { ... }
    _ => Err(InstructionError::InvalidAccountData),
}
```

### `AuthorizeChecked`
- Yeni otorite account[3] olarak saglanir, imzalamali
- Custodian opsiyonel (account[4])

### `DelegateStake`
- Vote account owner'i `solana_vote_program::id()` olmali
- State `Initialized` ise: staker imzasi, `validate_delegated_amount`, `new_stake`, state -> `Stake`
- State `Stake` ise: staker imzasi, `redelegate_stake`
- Feature gated: `reduce_stake_warmup_cooldown` aktif degilse config hesabi da gonderilmeli

### `redelegate_stake`
- Stake aktif (effective > 0) ve ayni voter ise + deactivation_epoch == clock.epoch ise: deactivation_epoch = MAX (rescind)
- Farkli voter'a aktif delegede redelegation yasak: `TooSoonToRedelegate`
- Aktif degilse: yeni aktivasyon parametreleri atanir

### `Split(lamports)`
- Hedef hesap: owner = stake program, bos state, dogru boyut
- `validate_split_amount` — kaynak ve hedef icin rent exempt kontrolu
- Tam cekimde: `remaining_stake_delta = lamports - rent_exempt_reserve`
- Kismi cekimde: `stake.delegation.stake - lamports >= minimum_delegation` kontrolu
- `split_stake_amount < minimum_delegation` -> `InsufficientDelegation`
- Tam cekim olunca kaynak `Uninitialized`'a donusur

### `Merge`
- Source hesap owner = stake program (spoofing engeli)
- Kaynak ve hedef ayni transaction index'te olamaz (loophole engeli)
- `MergeKind::get_if_mergeable` — sadece uyumlu state'ler birlesebilir
- Staker imzasi zorunlu
- Source `Uninitialized`'a cekilir, lamportlar hedefe aktarilir

### `Redelegate`
- Tam aktif stake'i baska validator'e tasir
- `status.activating != 0 || status.deactivating != 0` -> `RedelegateTransientOrInactiveStake`
- Ayni vote account'a redelegation yasak: `RedelegateToSameVoteAccount`
- Kaynak stake deactive yapilir, effective lamportlar uninitialized hesaba aktarilir
- Uninitialized hesap yeni delegasyon ile `Stake` state'ine gecirilir
- `StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED` set edilir

### `deactivate_stake` (internal)
- `MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED` flag varsa: `status.activating == 0` olmali
- Gecerli olunca: `stake.deactivation_epoch = clock.epoch`; flag temizlenir

### `DeactivateDelinquent`
- Validator delinquent kontrolu: reference epoch credits ile karsilastirilir
- `eligible_for_deactivate_delinquent` -> `MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION` (5 epoch)

### `Withdraw(lamports)`
- `StakeAuthorize::Withdrawer` imzasi gerekli (meta.authorized.check)
- Lockup aktifse: custodian imzasi da gerekebilir (lockup bypass)
- Kalan bakiye rent exempt alt sinirinin altina dusmemeli (tam cekim haric)
- `meta.authorized.withdrawer` imzasi

### `GetMinimumDelegation`
- Hesaplama: `crate::get_minimum_delegation(feature_set)`
- `invoke_context.transaction_context.set_return_data(id(), minimum_delegation.to_le_bytes())`

### Rewards Sistemi (rewards.rs + points.rs)

#### `PointValue` yapisi
```rust
pub struct PointValue {
    pub rewards: u64,  // dagitilacak lamport
    pub points: u128,  // bu puanlar uzerinden
}
```

#### `calculate_stake_points_and_credits`
- `credits_in_vote > credits_in_stake` olmali (yeni kredit yoksa sifir puan)
- Kredit'ler vote_state.epoch_credits'ten gecirilir; her epoch icin aktif stake * kredit
- `force_credits_update_with_skipped_reward`: vote hesabi yokken veya yeniden olusturuldugunda true

#### `calculate_stake_rewards`
- `point_value.rewards == 0` -> inflasyon kapali: credits_observed ilerlir ama odeme sifir
- `stake.delegation.activation_epoch == rewarded_epoch` -> `JustActivated`: sifir odeme
- `rewards = points * point_value.rewards / point_value.points`
- `vote_state.commission_split(rewards)` -> `(voter_rewards, staker_rewards, is_split)`
- `is_split && (voter_rewards == 0 || staker_rewards == 0)` -> `TooEarlyUnfairSplit`, None doner

#### `redeem_rewards`
- Ayni hesap uzerinde calisan runtime tarafindan cagrilir
- `stake_account.checked_add_lamports(stakers_reward)`
- Stake delegation.stake artirilir (compound staking)

---

## 4. CONFIG PROGRAM

### Dosyalar
- `config/src/lib.rs`
- `config/src/config_processor.rs` (~132 satir + testler)
- `config/src/config_instruction.rs`
- `config/src/date_instruction.rs`

### Tasarim Kavrami
Config program tek instruction destekler: Store. Instruction data'si `ConfigKeys + config_data` formatidir. Tek bir hesap, birden fazla imzaciya izin veren esnek bir on-chain key-value deposu.

### `ConfigKeys` yapisi
```rust
pub struct ConfigKeys {
    #[serde(with = "short_vec")]
    pub keys: Vec<(Pubkey, bool)>,  // (pubkey, is_signer)
}
```

### `ConfigState` trait'i
```rust
pub trait ConfigState: serde::Serialize + Default {
    fn max_space() -> u64;
}
```

### `get_config_data` fonksiyonu
```rust
pub fn get_config_data(bytes: &[u8]) -> Result<&[u8], bincode::Error> {
    deserialize::<ConfigKeys>(bytes)
        .and_then(|keys| serialized_size(&keys))
        .map(|offset| &bytes[offset as usize..])
}
```
Hesap verisi: `[ConfigKeys (bincode)] + [ConfigData (bincode)]`

### Processor Logigi (config_processor.rs)

1. Instruction data'sini `ConfigKeys` olarak deserialize et
2. config_account[0] owner = `crate::id()` olmali
3. Mevcut `current_data: ConfigKeys` deserialize et
4. `current_signer_keys` = sadece `is_signer == true` olanlar
5. Eger `current_signer_keys.is_empty()`: ilk yaratmada config account'un kendisi imzalamali
6. Her yeni signer icin:
   - Config account key'i ise: `is_config_account_signer` kontrolu
   - Diger: transaction'daki sirasiyla hesap borrow et, imza kontrolu, pubkey eslesmesi
   - Mevcut `current_signer_keys`'te bulunmali (yeni signerlar eski anahtarlarla tutarli olmali)
7. Duplicate kontrolu: `key_list.keys` -> `BTreeSet` -> `len()` eslesmeli
8. Eksik imzaci kontrolu: `current_signer_keys.len() > counter`
9. Instruction data boyutu <= hesap verisi boyutu olmali
10. `config_account.get_data_mut()[..data.len()].copy_from_slice(data)` — yazma

### Guvenlik Bulgulari
- Duplicate key engeli (`BTreeSet` dedupe)
- Signer'lar mevcut listeyle tutarli olmali (eski signerlar yetkisiz eklenemez)
- Owner kontrolu on planda
- Instruction data'si hesap boyutunu asamaz (overflow engeli)

### DEFAULT_COMPUTE_UNITS: 450

### Test Senaryolari
- `test_process_create_ok`, `test_process_store_ok`
- `test_process_store_fail_instruction_data_too_large`
- `test_process_store_fail_account0_not_signer`
- `test_process_store_with_additional_signers`
- `test_process_store_without_config_signer`
- `test_process_store_with_bad_additional_signer`
- `test_config_updates` — eksik imza, yanlis imza, guncelleme
- `test_config_initialize_contains_duplicates_fails`
- `test_config_update_contains_duplicates_fails`
- `test_config_bad_owner`

---

## 5. ADDRESS LOOKUP TABLE PROGRAM

### Dosyalar
- `address-lookup-table/src/lib.rs`
- `address-lookup-table/src/processor.rs` (~469 satir)

### ProgramInstruction Enum
```
CreateLookupTable { recent_slot: Slot, bump_seed: u8 }
FreezeLookupTable
ExtendLookupTable  { new_addresses: Vec<Pubkey> }
DeactivateLookupTable
CloseLookupTable
```

### LookupTableMeta yapisi
```rust
LookupTableMeta {
    deactivation_slot: Slot,    // MAX = aktif
    last_extended_slot: Slot,
    last_extended_slot_start_index: u8,
    authority: Option<Pubkey>,  // None = frozen
    ...
}
```

### `LookupTableStatus` enum'u
```
Activated
Deactivating { remaining_blocks }
Deactivated
```

### `create_lookup_table`
1. Table account bos olmali (feature gated relaxation)
2. Authority signer olmali (feature gated)
3. Payer signer olmali
4. `recent_slot` -> `slot_hashes.get(recent_slot)` ile dogrula
5. PDA turetimi: `Pubkey::create_program_address(&[authority_key, &slot.to_le_bytes(), &[bump_seed]], &id())`
6. Table key turetilen adresle eslesmelidir
7. Native invoke ile: `transfer`, `allocate`, `assign` cagrilir
8. State: `ProgramState::LookupTable(LookupTableMeta::new(authority_key))`

Tasarim notu: PDA kaynakli adres sayesinde ayni adres iki kez initialize edilemez.

### `freeze_lookup_table`
- Owner kontrolu
- Authority signer olmali
- `lookup_table.meta.authority == Some(authority_key)` eslesme
- `deactivation_slot == Slot::MAX` (deaktif tablolar dondurulamaz)
- Adres listesi bos olamaz
- `meta.authority = None` yazilir

### `extend_lookup_table`
- Owner + authority signer kontrolu
- `meta.authority.is_some()` (frozen tabloya ekleme yapılamaz)
- `deactivation_slot == Slot::MAX` (deaktif tabloya ekleme yapılamaz)
- `addresses.len() < LOOKUP_TABLE_MAX_ADDRESSES` (255)
- Toplam yeni uzunluk maksimumu asamaz
- `new_addresses.is_empty()` -> `InvalidInstructionData`
- Clock slotu degistiyse: `last_extended_slot` ve `last_extended_slot_start_index` guncellenir
- Her yeni adres: `lookup_table_account.extend_from_slice(new_address.as_ref())`
- Rent hesabi: `rent.minimum_balance(new_table_data_len)` - mevcut lamport > 0 ise payer'dan native invoke ile transfer

### `deactivate_lookup_table`
- Owner + authority signer + authority eslesme
- Frozen (authority == None) tablo deaktif edilemez
- Zaten deaktif (deactivation_slot != MAX) tablo tekrar deaktif edilemez
- `meta.deactivation_slot = clock.slot`

### `close_lookup_table`
- Owner + authority signer + authority eslesme
- Hedef hesap (recipient) ayni table olamaz (self-reference engeli)
- Frozen tablo kapatilamaz
- `lookup_table.meta.status(clock.slot, &slot_hashes)`:
  - `Activated` -> `InvalidArgument` (once deaktif et)
  - `Deactivating` -> blok sayisi bekleniyor mesaji + `InvalidArgument`
  - `Deactivated` -> devam et
- `recipient.checked_add_lamports(withdrawn_lamports)`
- `lookup_table_account.set_data_length(0)` + `set_lamports(0)`

### DEFAULT_COMPUTE_UNITS: 750

---

## 6. COMPUTE BUDGET PROGRAM

### Dosyalar
- `compute-budget/src/lib.rs` (8 satir)

```rust
pub const DEFAULT_COMPUTE_UNITS: u64 = 150;

declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |_invoke_context| {
    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
});
```

Bu program sadece bir kayit noktasidir. Gercek islem runtime tarafindan transaction'in basinda yapilir:
- `SetComputeUnitLimit(units)` — transaction CU limitini ayarlar
- `SetComputeUnitPrice(micro_lamports)` — oncelik ucreti ayarlar
- `SetLoadedAccountsDataSizeLimit(bytes)` — yuklenen hesap veri limiti

---

## 7. LOADER-V4 PROGRAM

### Dosyalar
- `loader-v4/src/lib.rs` (~620+ satir)

### LoaderV4Instruction Enum
```
Write          { offset: u32, bytes: Vec<u8> }
Truncate       { new_size: u32 }
Deploy
Retract
TransferAuthority
```

### LoaderV4Status Enum
```
Retracted    — yuklendi/degistirildi ama deploy edilmedi
Deployed     — aktif, calistirilab ilir
Finalized    — immutable, authority yok
```

### LoaderV4State yapisi
```rust
LoaderV4State {
    slot: Slot,
    status: LoaderV4Status,
    authority_address: Pubkey,
}
```

### `check_program_account` (ortak dogrulama)
```rust
fn check_program_account(log_collector, instruction_context, program, authority_address)
    -> Result<LoaderV4State, InstructionError>
{
    // 1. Owner = loader_v4::id()
    // 2. Data bos degil
    // 3. state: get_state(data)
    // 4. is_writable()
    // 5. instruction_account[1] signer (authority)
    // 6. state.authority_address == *authority_address
    // 7. !matches!(status, Finalized) -- immutable engeli
}
```

### `process_instruction_write`
- `check_program_account` cagrilir
- `status == Retracted` olmali (deployed programa yazma yapılamaz)
- `data[program_data_offset + offset .. + bytes.len()].copy_from_slice(&bytes)`

### `process_instruction_truncate`
- Yeni boyut > 0 ve data < program_data_offset ise: ilk baslatma
  - Owner, writable, PROGRAM imzasi (programin kendisi), AUTHORITY imzasi
- Diger durum: `check_program_account` + `status == Retracted`
- Lamport hesabi: `rent.minimum_balance(program_data_offset + new_size)`
- Eksik lamport: `InsufficientFunds`
- Fazla lamport: recipient[2]'ye gonderilir (is_writable kontrolu)
- `new_size == 0`: `set_data_length(0)` (silme)
- `is_initialization`: `state.slot = 0`, `status = Retracted`, `authority_address = ...`

### `process_instruction_deploy`
- `check_program_account` cagrilir
- Cooldown kontrolu: `state.slot != 0 && state.slot + DEPLOYMENT_COOLDOWN_IN_SLOTS > current_slot`
- `status == Retracted` olmali
- Source program opsiyonel (account[2]): varsa source da Retracted olmali
- `LoadedProgram::new(...)` ile ELF dogrulama + JIT derleme
- `state.slot = current_slot`, `state.status = Deployed`
- Program cache: `programs_modified_by_tx.replenish()`
- Source varsa: data kopyalanir, source temizlenir, lamportlar dengelenir

### `process_instruction_retract`
- `check_program_account` cagrilir
- Cooldown kontrolu: `state.slot + COOLDOWN > current_slot` ise `InvalidArgument`
- `status != Retracted` olmali (zaten retracted ise hata)
- `state.status = Retracted`

### `process_instruction_transfer_authority`
- `check_program_account` cagrilir
- `new_authority_address.is_some()`: yeni otorite imzalamali (account[2])
- Yeni otorite varsa: `state.authority_address = new_authority_address`
- Yeni otorite yoksa ve `status == Deployed`: `state.status = Finalized`
- `status != Deployed` iken finalise: `InvalidArgument`

### Unsafe Kullanim
```rust
pub fn get_state(data: &[u8]) -> Result<&LoaderV4State, InstructionError> {
    unsafe {
        let data = data.get(0..LoaderV4State::program_data_offset())
            .ok_or(InstructionError::AccountDataTooSmall)?
            .try_into().unwrap();
        Ok(std::mem::transmute::<&[u8; N], &LoaderV4State>(data))
    }
}
```
`transmute` ile sifir kopya state okumasi. Ust sinir kontrolu (`program_data_offset`) oneellikle yapilir.

### VM Kurulumu (`create_vm`)
```rust
pub fn create_vm(invoke_context, program) -> Result<EbpfVm, Error> {
    // Heap maliyeti hesabi (32KB sayfa basi)
    invoke_context.consume_checked(calculate_heap_cost(heap_size, heap_cost))?;
    // Stack: AlignedMemory (HOST_ALIGN)
    // Heap: AlignedMemory
    // Memory regions: [ro_region, stack_region, heap_region]
    // MemoryMapping::new -> EbpfVm::new
}
```

### DEFAULT_COMPUTE_UNITS: 2000

---

## 8. BPF LOADER PROGRAM (Ozet)

### Dosyalar
- `bpf_loader/src/lib.rs`
- `bpf_loader/src/serialization.rs`
- `bpf_loader/src/syscalls/mod.rs`
- `bpf_loader/src/syscalls/cpi.rs`
- `bpf_loader/src/syscalls/logging.rs`
- `bpf_loader/src/syscalls/mem_ops.rs`
- `bpf_loader/src/syscalls/sysvar.rs`

### Temel Sorumluluklar
- Legacy BPF ve upgradeble program loader
- `sol_invoke_signed_c` / `sol_invoke_signed_rust` CPI syscall'lari
- Logging: `sol_log_`, `sol_log_64_`, `sol_log_data`
- Memory: `sol_memcpy_`, `sol_memmove_`, `sol_memset_`, `sol_memcmp_`
- Sysvar: `sol_get_clock_sysvar`, `sol_get_epoch_schedule_sysvar` vb.
- Program serialization: account data'yi sBPF bellek haritasina yazar

---

## 9. ZK-TOKEN-PROOF PROGRAM (Ozet)

### Dosyalar
- `zk-token-proof/src/lib.rs`
- `zk-token-proof/benches/verify_proofs.rs`

### Fonksiyon
Confidential Transfer icin ZK ispat dogrulama:
- ElGamal sifreleme ispatı
- Range proof (Bulletproofs)
- Equality proof
- Validity proof

Her ispat turu ayri instruction olarak islenir. Gercek dogrulama `solana_zk_token_sdk` kullanilarak yapilir.

---

## 10. GENEL MIMARIEL KARARLAR VE DERSLER

### 10.1 `declare_process_instruction!` Makrosu

Tum native program'lar bu makroyu kullanir:
```rust
declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |invoke_context| {
    // program mantigi
});
```
- `DEFAULT_COMPUTE_UNITS`: program'in varsayilan CU maliyeti
- `Entrypoint::vm` metodu: BPF VM icin kullanilan entrypoint

### 10.2 BorrowedAccount Modeli

Hesaplar `BorrowedAccount` olarak borrow edilir (Rust lifetime takibi):
```rust
let mut account = instruction_context.try_borrow_instruction_account(tx_ctx, index)?;
// ... kullan ...
drop(account);  // Sonra baska hesap borrow edilebilir
```
Ayni hesabın iki mutable borrow'u derlenmez — double-spend'i derleme zamaninda engeller.

### 10.3 Sysvar Cekimi

Iki yontem:
1. `get_sysvar_with_account_check::clock(invoke_context, instruction_context, N)` — account[N]'i sysvar olarak dogrular
2. `invoke_context.get_sysvar_cache().get_clock()?` — dogrudan cache'den, hesap yok

### 10.4 `checked_sub_lamports` / `checked_add_lamports`

Hic yerinde overflow olmaz, her zaman checked arithmetic:
```rust
from.checked_sub_lamports(lamports)?;
to.checked_add_lamports(lamports)?;
```
`?` operatoru `InstructionError::ArithmeticOverflow` doner.

### 10.5 `limited_deserialize`

```rust
let instruction = limited_deserialize(instruction_data)?;
```
Maksimum 1232 byte limiti uygular. Bu limit, transaction'in maximum boyutundan gelir. Gossip ve replikasyon katmanlarini buyuk instruction data'lardan korur.

### 10.6 Imza Kontrolu Katmanlari

Solana'da uc katman vardır:
1. **Transaction seviyesi**: `signers_seeds` ile imzalanan hesap listesi runtime tarafindan dogrulanir
2. **Instruction seviyesi**: `instruction_context.is_instruction_account_signer(N)?`
3. **Program seviyesi**: `signers: HashSet<Pubkey>` uzerinden `signers.contains(&authority)` manuel kontrol

### 10.7 Owner Kontrolu Patterns

```rust
// Pattern 1: Kendi programi mi?
if *account.get_owner() != id() { return Err(InvalidAccountOwner); }

// Pattern 2: Baska programin hesabi mi?
if *vote_account.get_owner() != solana_vote_program::id() { return Err(IncorrectProgramId); }

// Pattern 3: System program hesabi mi?
if !system_program::check_id(nonce_account.get_owner()) { return Err(InvalidAccountOwner); }
```

### 10.8 Self-Reference Engeli

```rust
// Merge'de: kaynak ve hedef ayni olamaz
if instruction_context.get_index_of_instruction_account_in_transaction(stake_account_index)?
    == instruction_context.get_index_of_instruction_account_in_transaction(source_account_index)?
{
    return Err(InstructionError::InvalidArgument);
}

// CloseLookupTable'da: tablo kendi recipient'i olamaz
if get_index_of_instruction_account_in_transaction(0)?
    == get_index_of_instruction_account_in_transaction(2)?
{
    return Err(InstructionError::InvalidArgument);
}
```

### 10.9 Feature Gating

```rust
if invoke_context.feature_set.is_active(&feature_set::reduce_stake_warmup_cooldown::id()) {
    // yeni davranis
} else {
    // eski davranis (config hesabi gerekli)
}
```
Runtime upgrade'leri backward-compatible yapar. Her ozellik bir pubkey ile tanimlenir.

### 10.10 Native Invoke (CPI benzeri)

```rust
invoke_context.native_invoke(
    system_instruction::transfer(&payer_key, &table_key, required_lamports).into(),
    &[payer_key],
)?;
```
`address-lookup-table` gibi programlar kendileri lamport yonetimi yapmak yerine system_program'i CPI ile cagirir.

### 10.11 State Machine Gecisleri (Stake)

```
Uninitialized
    -> Initialized    (Initialize, InitializeChecked)
    -> Stake          (buraya dogrudan gecilmez)

Initialized
    -> Stake          (DelegateStake)
    -> Uninitialized  (tam Withdraw)

Stake
    -> Stake          (Authorize, SetLockup, Split, Merge, Redelegate, reward)
    -> Uninitialized  (tam Withdraw)
```

### 10.12 Validator Kimlik Guvenlik Hiyerarsisi (Vote)
- `authorized_withdrawer` — en yuksek yetki (komisyon degistirir, para ceker)
- `authorized_voter` — epoch bazli zamanlanir, oy atar
- `node_pubkey` — validator identity; both withdrawer AND new_node imzasi gerekli

---

## 11. NOVA SHIELD CHAIN ICIN CIKARIMLARI

### 11.1 System Program Mimarisi
- Kendi system program'inizi yazarken `Address { address, base: Option<Pubkey> }` patterniyle seed'li adresleri ilk siniftan destekleyin.
- Hesap yaratimini iki adima bolun: once `allocate` (boyut yaz), sonra `assign` (owner degistir). Bu, atomik olmayan duplicate yaratim saldirilarini engeller.
- `transfer`: `from` hesabinda veri bulunuyorsa reddet — token/program hesabi SOL tasiyabilir ama native transfer sadece bos hesaplar uzerinden yapilmali.
- Nonce mekanizmasi uzun omurlu islemleri saglar. Nova Shield Chain'de MPC transaction akislari icin durable nonce pattern'ini degerlendirin.

### 11.2 Vote/Consensus Mekanizmasi
- Validator oylarinda `SlotHash` dogrulamasini zorunlu kılin — fork algilamasinin temelini olusturur.
- Tower BFT geriyatirma engeli (`ConfirmationRollBack`, `LockoutConflict`) Nova Shield Chain'in BFT modulu icin dogrudan uygulanabilir.
- `timely_vote_credits` + latency hesabi: validator'leri hizli oy atmaya tesvik eden ekonomik model.
- Çift authority: `authorized_voter` (epoch-scheduled) vs `authorized_withdrawer` (anlik). MPC key rotasyonu icin bu ayrimi uygulayabilirsiniz.

### 11.3 Stake/Delegation Sistemi
- `StakeStateV2` state machine kesinlikle kullanin — enum bazli state, gercersiz gecisleri derleme zamaninda engeller.
- Cooldown/warmup mekanizmasi: aktivasyon ve deaktivasyon epoch bazli gecikmeli. Nova Shield'in staking sistemi icin benimsein.
- `PointValue { rewards, points }` pattern'i tamsayi aritmetigi ile odul dagilimini saglar — float kullanmak gerekmez.
- `calculate_stake_rewards` sifir odul durumlarini kapsamli ele alir: inflation kapali, activasyon epoch'u, erken bolunme (TooEarlyUnfairSplit). Bunlari direkt uygulayabilirsiniz.
- `redeem_rewards` compound staking saglar: `stake.delegation.stake += staker_rewards`.

### 11.4 Config Program Modeli
- Genesis parametreleri ve on-chain yonetim icin `ConfigKeys + data` pattern'ini kullanin.
- Multi-signer yetkilendirme: `BTreeSet` ile duplicate engeli + eski imzacilarin tutarlilik zorunlulugu.
- Nova Shield Chain'in validator kümesi yonetimi veya MPC parametre guncelleme akislari icin bu pattern idealdir.

### 11.5 Address Lookup Table
- Cok sayida hesap iceren islemler icin kritik: 256 adrese kadar on-chain tablo.
- PDA tabanlı adres turetimi (`authority + slot + bump`) duplicate initialize'yi engeller.
- Deactivation cooldown mekanizmasi, tablonun canlı işlemlerde kullanilmasini engeller — Nova Shield'in hesap tablolari veya shard mapping'i icin kullanilabilir.
- Freeze mekanizmasi immutable tablo saglar — dogrulama kumeleri veya sistem adresleri icin uygundur.

### 11.6 Loader-V4 Modeli
- Program deployment lifecycle'i: `Retracted -> Deployed -> Finalized`
- Cooldown on yeniden deploy: `DEPLOYMENT_COOLDOWN_IN_SLOTS` — Nova Shield'in program governance'inda benzer cooldown ekleyin.
- `authority_address = None` -> `Finalized`: immutable program pattern'ini kendi akilli sozlesmelerinizde kullanin.
- Unsafe `transmute` sifir kopya okuma: sadece boyut garantisi saglandiktan sonra kullanin. Nova Shield'de benzer low-level serialization gerekirse `zerocopy` crate'i tercih edin.

### 11.7 Genel Guvenlik Prensipleri
1. Her program girisinde owner kontrolu yap.
2. Her `checked_sub_lamports`/`checked_add_lamports` = overflow sifir tolerans.
3. `limited_deserialize` ile instruction boyut siniri.
4. BorrowedAccount'un Rust lifetime sistemi double-spend'i derleme zamaninda engeller — bu mimariyi kopyalayin.
5. Self-reference kontrolu: kaynak == hedef mi? Her hesap transfer/merge operasyonunda zorunlu.
6. Feature gating ile backward-compatible upgrade: eski ve yeni davranisi `is_active()` ile ayirin.
7. Sysvar'lari on-chain hesap olarak gondermeyi zorunlu kilin (account check sysvar) — spoofing engeli.

### 11.8 Compute Budget Modeli
- Her program sabit veya dinamik CU degeri bildirir.
- System: 150 CU (basit transfer)
- Config: 450 CU
- Address Lookup Table: 750 CU
- Stake: 750 CU
- Loader-v4: 2000 CU
- Vote: 2100 CU (statik, gelecekte dinamik olacak)

Nova Shield Chain icin: her modüle uc tip CU maliyeti ayirin:
1. Okuma agirlikli islemler (dusuk CU)
2. Yazma agirlikli islemler (orta CU)
3. Kriptografik/hesaplama agirlikli islemler (yuksek CU)

---

## 12. DOSYA REFERANSLARI

```
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/system/src/lib.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/system/src/system_processor.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/system/src/system_instruction.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/vote/src/vote_processor.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/vote/src/vote_state/mod.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/vote/src/vote_transaction.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/stake/src/stake_instruction.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/stake/src/stake_state.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/stake/src/rewards.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/stake/src/points.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/config/src/lib.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/config/src/config_processor.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/address-lookup-table/src/processor.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/compute-budget/src/lib.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/loader-v4/src/lib.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/bpf_loader/src/lib.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/bpf_loader/src/syscalls/cpi.rs
/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/programs/zk-token-proof/src/lib.rs
```
