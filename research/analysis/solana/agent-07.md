# Solana Agent-07 Analizi: System Program ve Vote Program

**Analiz Edilen Dosyalar:**
- `programs/system/src/lib.rs`
- `programs/system/src/system_instruction.rs`
- `programs/system/src/system_processor.rs`
- `programs/vote/src/lib.rs`
- `programs/vote/src/vote_processor.rs`
- `programs/vote/src/vote_state/mod.rs`
- `programs/vote/src/vote_transaction.rs`

---

## BÖLÜM 1: SYSTEM PROGRAM

### 1.1 `programs/system/src/lib.rs`

**Dosya Boyutu:** 34 satır
**Amaç:** System program'ın kök modülü; `SystemAccountKind` enum'u ve hesap türü tanımlama mantığı.

#### Enum Tanımları

```rust
// Satır 12-16
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SystemAccountKind {
    System,
    Nonce,
}
```

- `System`: Boş data alanına sahip system-owned hesap (normal cüzdan).
- `Nonce`: `nonce::State::size()` uzunluğunda data içeren ve `Initialized` durumundaki nonce hesabı.

#### Fonksiyon Tanımları

**`get_system_account_kind`** (Satır 18-34)
```rust
pub fn get_system_account_kind(account: &AccountSharedData) -> Option<SystemAccountKind>
```
- Owner kontrolü: `system_program::check_id(account.owner())`
- Veri yoksa → `SystemAccountKind::System`
- Veri `nonce::State::size()` uzunluğundaysa → deserialize edip kontrol et
  - `Uninitialized` → `None` (geçersiz)
  - `Initialized(_)` → `SystemAccountKind::Nonce`
- Başka boyutlarda data varsa → `None`

**Güvenlik Notu:** Sahiplik kontrolü (`owner == system_program`) zorunludur. Bu, yabancı program hesaplarının system account olarak sınıflandırılmasını önler.

---

### 1.2 `programs/system/src/system_instruction.rs`

**Dosya Boyutu:** ~700+ satır (test dahil)
**Amaç:** Nonce account'lar için dört temel işlem: `advance`, `withdraw`, `initialize`, `authorize`.

#### Fonksiyon Tanımları

---

**`advance_nonce_account`** (Satır 20-70)
```rust
pub fn advance_nonce_account(
    account: &mut BorrowedAccount,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError>
```

**State Machine:**
```
Uninitialized → Err(InvalidAccountData)
Initialized(data) →
  ├── is_writable? → Err(InvalidArgument) [hayır]
  ├── signers.contains(data.authority)? → Err(MissingRequiredSignature) [hayır]
  ├── durable_nonce == next_durable_nonce? → Err(NonceBlockhashNotExpired) [evet, aynı slot]
  └── OK: State::Initialized(new_data) kaydedilir
```

**Güvenlik Kontrolleri:**
1. `is_writable()` zorunludur.
2. `data.authority` signers içinde olmalı.
3. Nonce aynı blockhash'ten tekrar türetilemez (replay protection).
4. Yeni nonce `DurableNonce::from_blockhash(&invoke_context.blockhash)` ile hesaplanır.

---

**`withdraw_nonce_account`** (Satır 72-151)
```rust
pub fn withdraw_nonce_account(
    from_account_index: IndexOfAccount,
    lamports: u64,
    to_account_index: IndexOfAccount,
    rent: &Rent,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError>
```

**State Machine:**
```
Uninitialized:
  ├── lamports > from.lamports → Err(InsufficientFunds)
  └── signer = from.key (self-sign gerekliliği)

Initialized(data):
  ├── lamports == from.lamports (tam çekim):
  │   ├── durable_nonce == next_durable_nonce? → Err(NonceBlockhashNotExpired) [evet]
  │   └── State → Uninitialized (account deinitialize)
  └── Kısmi çekim:
      ├── lamports + min_balance > from.lamports → Err(InsufficientFunds)
      └── signer = data.authority
```

**Güvenlik Kontrolleri:**
1. Writable kontrolü.
2. Tam çekilirken nonce'ın expired olması şartı (aynı slot replay protection).
3. Kısmi çekilirken rent-exempt minimum'un korunması: `amount = lamports + min_balance`.
4. `from.checked_sub_lamports(lamports)` ve `to.checked_add_lamports(lamports)` — overflow güvenli.
5. Borrow drop sırası kritik: `drop(from)` yapıldıktan sonra `to` borçlanılır.

---

**`initialize_nonce_account`** (Satır 153-198)
```rust
pub fn initialize_nonce_account(
    account: &mut BorrowedAccount,
    nonce_authority: &Pubkey,
    rent: &Rent,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError>
```

**State Machine:**
```
Initialized(_) → Err(InvalidAccountData)  [zaten init]
Uninitialized →
  ├── is_writable? → Err(InvalidArgument) [hayır]
  ├── lamports >= min_balance? → Err(InsufficientFunds) [değilse]
  └── OK: State::Initialized(new_data) kaydedilir
```

**Önemli:** Authority herhangi bir pubkey olabilir; account key olmak zorunda değil. Bu, başka bir key'in nonce'ı kontrol etmesine olanak tanır.

---

**`authorize_nonce_account`** (Satır 200-236)
```rust
pub fn authorize_nonce_account(
    account: &mut BorrowedAccount,
    nonce_authority: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError>
```

**State Machine:**
```
versions.authorize(signers, new_authority):
  ├── Ok(versions) → account.set_state(&versions)
  ├── Err(Uninitialized) → Err(InvalidAccountData)
  └── Err(MissingRequiredSignature(acc_auth)) → Err(MissingRequiredSignature)
```

**Not:** Gerçek yetkilendirme mantığı `nonce::state::Versions::authorize()` içinde; burada sadece hata çevirme yapılır.

#### Test Coverage (Satır 238+)

Test makroları:
- `push_instruction_context!` — mock context kurulumu
- `prepare_mockup!` — standart test hesapları
- `set_invoke_context_blockhash!` — blockhash simülasyonu

Test fonksiyonları:
- `default_is_uninitialized` — varsayılan state kontrolü
- `expected_behavior` — tam lifecycle testi (init → advance × 3 → withdraw)
- `nonce_inx_initialized_account_not_signer_fail` — eksik imza testi
- `nonce_inx_too_early_fail` — aynı slot replay koruması
- `nonce_inx_uninitialized_account_fail` — uninit advance girişimi
- `nonce_inx_independent_nonce_authority_ok` — ayrı authority ok
- `nonce_inx_no_nonce_authority_sig_fail` — wrong authority fail
- `withdraw_inx_unintialized_acc_ok` — uninit çekim

---

### 1.3 `programs/system/src/system_processor.rs`

**Dosya Boyutu:** ~1200+ satır (test dahil)
**Amaç:** System program'ın ana instruction dispatcher'ı. Tüm `SystemInstruction` varyantlarını işler.

#### Struct Tanımları

**`Address`** (Satır 27-31)
```rust
#[derive(PartialEq, Eq, Default, Debug)]
struct Address {
    address: Pubkey,
    base: Option<Pubkey>,  // seed'den türetilmişse Some(base_key)
}
```

- `base: None` → Normal adres, kendi imzasını verir.
- `base: Some(base_key)` → Seed-derived adres, `base_key` imzasını verir.

#### Impl Metotları — `Address`

**`is_signer`** (Satır 34-39)
```rust
fn is_signer(&self, signers: &HashSet<Pubkey>) -> bool
```
- Seed varsa base key'in imzası kontrol edilir.
- Seed yoksa adresin kendi imzası kontrol edilir.

**`create`** (Satır 41-67)
```rust
fn create(
    address: &Pubkey,
    with_seed: Option<(&Pubkey, &str, &Pubkey)>,
    invoke_context: &InvokeContext,
) -> Result<Self, InstructionError>
```
- `with_seed` varsa: `Pubkey::create_with_seed(base, seed, owner)` türetilir ve `address` ile karşılaştırılır.
- Eşleşmezse: `Err(SystemError::AddressWithSeedMismatch)` — deterministic address doğrulaması.

#### İç Fonksiyonlar

**`allocate`** (Satır 70-110)
```rust
fn allocate(
    account: &mut BorrowedAccount,
    address: &Address,
    space: u64,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError>
```

Kontroller:
1. `address.is_signer(signers)` — hesap sahibi imzalamalı.
2. `!account.get_data().is_empty() || !system_program::check_id(account.get_owner())` — hesap zaten kullanımda.
3. `space > MAX_PERMITTED_DATA_LENGTH` — maksimum alan kontrolü (10 MB).

**`assign`** (Satır 112-130)
```rust
fn assign(
    account: &mut BorrowedAccount,
    address: &Address,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
) -> Result<(), InstructionError>
```
- Mevcut owner zaten yenisiyle aynıysa erken çıkış (no-op).
- Aksi takdirde imza kontrolü ve `account.set_owner(&owner.to_bytes())`.

**`allocate_and_assign`** (Satır 132-142)
```rust
fn allocate_and_assign(...) -> Result<(), InstructionError>
```
- `allocate()` + `assign()` zincirleme çağrısı.

**`create_account`** (Satır 144-180)
```rust
fn create_account(
    from_account_index: IndexOfAccount,
    to_account_index: IndexOfAccount,
    to_address: &Address,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &InvokeContext,
    transaction_context: &TransactionContext,
    instruction_context: &InstructionContext,
) -> Result<(), InstructionError>
```

Kontroller:
1. `to.get_lamports() > 0` → `Err(AccountAlreadyInUse)` — pre-funded accounts koruması.
2. `allocate_and_assign()` — alan ve sahiplik ayarla.
3. `transfer()` — lamport transferi.

**`transfer_verified`** (Satır 182-212)
```rust
fn transfer_verified(...) -> Result<(), InstructionError>
```
- `from.get_data().is_empty()` — data taşıyan hesap transfer edemez (program hesabı koruması).
- `lamports > from.get_lamports()` → `Err(ResultWithNegativeLamports)`.
- `from.checked_sub_lamports()` + `to.checked_add_lamports()`.

**`transfer`** (Satır 214-242)
```rust
fn transfer(...) -> Result<(), InstructionError>
```
- `is_instruction_account_signer(from_account_index)` — imza kontrolü.
- `transfer_verified()` çağrısı.

**`transfer_with_seed`** (Satır 244-296)
```rust
fn transfer_with_seed(
    from_account_index: IndexOfAccount,
    from_base_account_index: IndexOfAccount,
    from_seed: &str,
    from_owner: &Pubkey,
    to_account_index: IndexOfAccount,
    lamports: u64,
    ...
) -> Result<(), InstructionError>
```
- Base key imzası kontrol edilir.
- `Pubkey::create_with_seed(base, seed, owner)` türetilir ve `from_key` ile eşleştirilir.
- `transfer_verified()` çağrısı.

#### Ana Entrypoint — `declare_process_instruction!` (Satır 300-538)

`DEFAULT_COMPUTE_UNITS: u64 = 150` (satır 298)

**Dispatch Tablosu:**

| Instruction | Hesap Sayısı | Ana Kontrol |
|---|---|---|
| `CreateAccount` | 2 | `from` imzası, `to` boş |
| `CreateAccountWithSeed` | 2 | seed türetme + from imzası |
| `Assign` | 1 | hesap imzası |
| `Transfer` | 2 | from imzası, from data boş |
| `TransferWithSeed` | 3 | base imzası, seed eşleşme |
| `AdvanceNonceAccount` | 1+ | recent_blockhashes boş değil, authority imzası |
| `WithdrawNonceAccount` | 2+ | recent_blockhashes + rent sysvars |
| `InitializeNonceAccount` | 1+ | recent_blockhashes boş değil |
| `AuthorizeNonceAccount` | 1 | mevcut authority imzası |
| `UpgradeNonceAccount` | 1 | owner system, writable, upgrade mevcut |
| `Allocate` | 1 | hesap imzası |
| `AllocateWithSeed` | 1 | base imzası, seed doğrulama |
| `AssignWithSeed` | 1 | base imzası, seed doğrulama |

**`AdvanceNonceAccount` Güvenlik Detayı (Satır 406-423):**
```rust
#[allow(deprecated)]
let recent_blockhashes = get_sysvar_with_account_check::recent_blockhashes(...)?;
if recent_blockhashes.is_empty() {
    return Err(SystemError::NonceNoRecentBlockhashes.into());
}
```
- Recent blockhashes boşsa nonce advance reddedilir. Bu, ağ donduğunda nonce'ların yanlış ilerlemesini engeller.

**`UpgradeNonceAccount` (Satır 471-486):**
- Owner system programı olmalı.
- Writable olmalı.
- `nonce_versions.upgrade()` — `None` döndürürse zaten en yeni versiyonda, `Err(InvalidArgument)`.

#### Test Coverage (Satır 541+)

Önemli test fonksiyonları:
- `test_create_account` — temel hesap oluşturma
- `test_create_account_with_seed` — seed-derived hesap
- `test_create_account_with_seed_separate_base_account` — ayrı base hesabı
- `test_transfer` — temel transfer
- `test_transfer_with_seed` — seed-derived transfer
- `test_allocate` — alan tahsisi
- `test_assign` — sahiplik değiştirme
- Nonce işlemleri için kapsamlı testler

---

## BÖLÜM 2: VOTE PROGRAM

### 2.1 `programs/vote/src/lib.rs`

**Dosya Boyutu:** 17 satır
**Amaç:** Vote program'ın kök modülü; alt modülleri re-export eder.

```rust
pub mod vote_processor;
pub mod vote_state;
pub mod vote_transaction;

pub use solana_sdk::vote::{
    authorized_voters, error as vote_error, instruction as vote_instruction,
    program::{check_id, id},
};
```

`solana_metrics` ve `solana_frozen_abi_macro` `#[macro_use]` ile yüklenir.

---

### 2.2 `programs/vote/src/vote_processor.rs`

**Dosya Boyutu:** ~1500+ satır (test dahil)
**Amaç:** Vote program instruction dispatcher.

`DEFAULT_COMPUTE_UNITS: u64 = 2_100` (satır 54) — System program'ın 150 biriminin 14 katı; vote işlemlerinin hesaplama ağırlığını yansıtır.

#### Yardımcı Fonksiyon

**`process_authorize_with_seed_instruction`** (Satır 20-50)
```rust
fn process_authorize_with_seed_instruction(
    invoke_context: &InvokeContext,
    instruction_context: &InstructionContext,
    transaction_context: &TransactionContext,
    vote_account: &mut BorrowedAccount,
    new_authority: &Pubkey,
    authorization_type: VoteAuthorize,
    current_authority_derived_key_owner: &Pubkey,
    current_authority_derived_key_seed: &str,
) -> Result<(), InstructionError>
```

- Clock sysvar alınır (saat referansı).
- Hesap indeksi 2 imzacıysa base pubkey'den seed-derived authority türetilir.
- `vote_state::authorize()` çağrısı yapılır.

#### Ana Entrypoint (Satır 56-234)

**İlk kontrol (Satır 64-66):**
```rust
if *me.get_owner() != id() {
    return Err(InstructionError::InvalidAccountOwner);
}
```
Her instruction'dan önce vote account'ın vote program tarafından sahiplenildiği kontrol edilir. Bu temel güvenlik katmanı.

**Dispatch Tablosu:**

| Instruction | Açıklama | Sysvar |
|---|---|---|
| `InitializeAccount(vote_init)` | Vote hesabı başlat | rent, clock |
| `Authorize(pubkey, type)` | Voter/withdrawer yetkilendir | clock |
| `AuthorizeWithSeed(args)` | Seed-derived authority ile yetkilendir | clock |
| `AuthorizeCheckedWithSeed(args)` | Yeni authority imzası zorunlu | clock |
| `UpdateValidatorIdentity` | Node pubkey güncelle | - |
| `UpdateCommission(u8)` | Komisyon güncelle | epoch_schedule, clock |
| `Vote(vote)` / `VoteSwitch(vote, _)` | Oy ver | slot_hashes, clock |
| `UpdateVoteState(update)` / `UpdateVoteStateSwitch` | Toplu oy state güncellemesi | slot_hashes, clock |
| `CompactUpdateVoteState` / `CompactUpdateVoteStateSwitch` | Kompakt format | slot_hashes, clock |
| `Withdraw(lamports)` | Para çek | rent, clock |
| `AuthorizeChecked(type)` | Yeni authority imzası kontrol | clock |

**`InitializeAccount` detayı (Satır 70-84):**
```rust
if !rent.is_exempt(me.get_lamports(), me.get_data().len()) {
    return Err(InstructionError::InsufficientFunds);
}
```
Rent exemption zorunludur — vote hesabı asla rent ödemez, daima exempt olmak zorundadır.

**`AuthorizeChecked` detayı (Satır 214-232):**
```rust
let voter_pubkey = transaction_context.get_key_of_account_at_index(
    instruction_context.get_index_of_instruction_account_in_transaction(3)?,
)?;
if !instruction_context.is_instruction_account_signer(3)? {
    return Err(InstructionError::MissingRequiredSignature);
}
```
`AuthorizeChecked` versiyonu, yeni authority'nin de imzalamasını zorunlu kılar. Bu, "checked" ailesinin standart güvenlik örüntüsü.

**`Vote` / `VoteSwitch` (Satır 153-166):**
- `VoteSwitch`'in ikinci parametresi (`switch_proof_hash`) sessizce görmezden gelinir, ama farklı instruction varyantı olarak saklanır.
- Her ikisi de `process_vote_with_account()` çağırır.

#### Test Yapıları (Satır 270-279)

```rust
struct VoteAccountTestFixtureWithAuthorities {
    vote_account: AccountSharedData,
    vote_pubkey: Pubkey,
    voter_base_key: Pubkey,
    voter_owner: Pubkey,
    voter_seed: String,
    withdrawer_base_key: Pubkey,
    withdrawer_owner: Pubkey,
    withdrawer_seed: String,
}
```

---

### 2.3 `programs/vote/src/vote_state/mod.rs`

**Dosya Boyutu:** ~1500+ satır (test dahil)
**Amaç:** Vote program'ın kalbi; Tower BFT mantığı, lockout hesaplamaları, credit sistemi.

#### Enum Tanımları

**`VoteTransaction`** (Satır 32-38)
```rust
#[frozen_abi(digest = "2AuJFjx7SYrJ2ugCfH1jFh3Lr9UHMEPfKwwk1NcjqND1")]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize, AbiEnumVisitor, AbiExample)]
pub enum VoteTransaction {
    Vote(Vote),
    VoteStateUpdate(VoteStateUpdate),
    #[serde(with = "serde_compact_vote_state_update")]
    CompactVoteStateUpdate(VoteStateUpdate),
}
```

`#[frozen_abi]` — ABI sabitliği garantisi, consensus değişikliklerini engeller.

**`VoteTransaction` metodları (Satır 40-132):**
- `slots()` → `Vec<Slot>`
- `slot(i: usize)` → `Slot`
- `len()` → `usize`
- `is_empty()` → `bool`
- `hash()` → `Hash`
- `timestamp()` → `Option<UnixTimestamp>`
- `set_timestamp(&mut self, ts: Option<UnixTimestamp>)`
- `last_voted_slot()` → `Option<Slot>`
- `last_voted_slot_hash()` → `Option<(Slot, Hash)>`

`From<Vote>` ve `From<VoteStateUpdate>` impl'leri (Satır 122-132).

#### Temel Fonksiyonlar

**`from<T: ReadableAccount>`** (Satır 134-137)
```rust
pub fn from<T: ReadableAccount>(account: &T) -> Option<VoteState>
```
Hesap datasından VoteState deserialize eder.

**`to<T: WritableAccount>`** (Satır 139-142)
```rust
pub fn to<T: WritableAccount>(versioned: &VoteStateVersions, account: &mut T) -> Option<()>
```
VoteState'i hesap datasına serialize eder.

**`set_vote_account_state`** (Satır 146-176)
```rust
fn set_vote_account_state(
    vote_account: &mut BorrowedAccount,
    vote_state: VoteState,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError>
```

Feature flag bağımlı versiyon seçimi:
- `vote_state_add_vote_latency` aktifse → `VoteStateVersions::Current` (yeni format)
  - Hesap yeterince büyük değilse → `V1_14_11` formatına geri düş
- Aktif değilse → `V1_14_11` zorunlu

Bu, canlı ağda backwards compatibility sağlayan kritik bir mekanizma.

#### Vote Slot Doğrulama

**`check_update_vote_state_slots_are_valid`** (Satır 178-413)
```rust
fn check_update_vote_state_slots_are_valid(
    vote_state: &VoteState,
    vote_state_update: &mut VoteStateUpdate,
    slot_hashes: &[(Slot, Hash)],
) -> Result<(), VoteError>
```

Bu fonksiyon, `VoteStateUpdate` instruction'ı için kapsamlı slot doğrulama yapar:

**Ön Kontroller:**
1. `vote_state_update.lockouts.is_empty()` → `Err(EmptySlots)`
2. `last_vote_state_update_slot <= last_vote_slot` → `Err(VoteTooOld)` (güncellik kontrolü)
3. `slot_hashes.is_empty()` → `Err(SlotsMismatch)`
4. `last_vote_state_update_slot < earliest_slot_hash_in_history` → `Err(VoteTooOld)`

**Root İşleme:**
```
proposed_root < earliest_slot_hash_in_history →
  vote_state_update.root = vote_state.root_slot
  (Mevcut vote'lardan proposed_root'tan küçük en büyüğü bul)
```

**İki-Pointer Algoritması (Satır 256-343):**
- `vote_state_update_indexes_to_filter`: geçersiz slot indisleri biriktirilir
- `vote_state_update.lockouts`: küçükten büyüğe sıralı
- `slot_hashes`: büyükten küçüğe sıralı
- Her slot için fork üyeliği kontrol edilir
- `Ordering::Less`: slot tarih dışı veya farklı fork → `Err(SlotsMismatch)` veya `Err(RootOnDifferentFork)`
- `Ordering::Greater`: slot_hashes'i geri sar
- `Ordering::Equal`: eşleşme bulundu, her iki indexi ilerlet

**Son Hash Doğrulama:**
```rust
if slot_hashes[slot_hashes_index].1 != vote_state_update.hash {
    return Err(VoteError::SlotHashMismatch);
}
```

**`check_slots_are_valid`** (Satır 415-500)
```rust
fn check_slots_are_valid(
    vote_state: &VoteState,
    vote_slots: &[Slot],
    vote_hash: &Hash,
    slot_hashes: &[(Slot, Hash)],
) -> Result<(), VoteError>
```

Eski `Vote` instruction'ı için daha basit doğrulama:
1. `vote_state.last_voted_slot() >= vote_slots[i]` → eski slot'ları atla
2. Her slot'u slot_hashes'te bul
3. Son slot'un hash'ini doğrula

Hata durumları:
- `VoteError::VoteTooOld` — tüm slot'lar eskiyse
- `VoteError::SlotsMismatch` — fork dışı slot
- `VoteError::SlotHashMismatch` — hash uyuşmazlığı

#### Tower BFT — `process_new_vote_state` (Satır 540-733)

```rust
pub fn process_new_vote_state(
    vote_state: &mut VoteState,
    mut new_state: VecDeque<LandedVote>,
    new_root: Option<Slot>,
    timestamp: Option<i64>,
    epoch: Epoch,
    current_slot: Slot,
    feature_set: Option<&FeatureSet>,
) -> Result<(), VoteError>
```

**Bu fonksiyon Tower BFT'nin kalbidir.**

**Ön Doğrulamalar:**
1. `new_state.len() > MAX_LOCKOUT_HISTORY` → `Err(TooManyVotes)`
2. Root gerilemesi kontrolü:
   - `new_root < current_root` → `Err(RootRollBack)`
   - `None` (yeni) vs `Some` (mevcut) → `Err(RootRollBack)`

**Her Yeni Vote için (Satır 572-600):**
```
confirmation_count == 0 → Err(ZeroConfirmations)
confirmation_count > MAX_LOCKOUT_HISTORY → Err(ConfirmationTooLarge)
vote.slot() <= new_root (ve new_root != default) → Err(SlotSmallerThanRoot)
slots sırasız → Err(SlotsNotOrdered)
confirmations azalan değil → Err(ConfirmationsNotOrdered)
lockout ihlali → Err(NewVoteStateLockoutMismatch)
```

**Timely Vote Credits (Satır 610-616):**
```rust
let timely_vote_credits = feature_set.map_or(false, |f| {
    f.is_active(&feature_set::timely_vote_credits::id())
});
let mut earned_credits = if timely_vote_credits { 0_u64 } else { 1_u64 };
```
Feature flag ile eski (buggy) ve yeni kredi hesaplama arasında geçiş.

**Rollback Koruması (Satır 661-702):**
Mevcut vote state ile yeni state karşılaştırılır:
- `Ordering::Less`: eski slot yeni state'de yok → lockout süresi dolmuş olmalı, yoksa `Err(LockoutConflict)`
- `Ordering::Equal`: aynı slot → `Err(ConfirmationRollBack)` (confirmation azalıyorsa)
- Latency kopyalama: `new_vote.latency = vote_state.votes[current_vote_state_index].latency`

**Vote Latency (Satır 711-717):**
```rust
if timely_vote_credits {
    for new_vote in new_state.iter_mut() {
        if new_vote.latency == 0 {
            new_vote.latency = VoteState::compute_vote_latency(new_vote.slot(), current_slot);
        }
    }
}
```
Yeni slot'ların latency değeri `current_slot - vote_slot` olarak hesaplanır. Bu manipülasyonu önler: validator kendi latency'sini seçemez.

**Finalization (Satır 719-730):**
```rust
if vote_state.root_slot != new_root {
    vote_state.increment_credits(epoch, earned_credits);
}
if let Some(timestamp) = timestamp {
    vote_state.process_timestamp(last_slot, timestamp)?;
}
vote_state.root_slot = new_root;
vote_state.votes = new_state;
```

#### Oy İşleme Pipeline

**`process_vote_unfiltered`** (Satır 735-756)
```rust
pub fn process_vote_unfiltered(
    vote_state: &mut VoteState,
    vote_slots: &[Slot],
    vote: &Vote,
    slot_hashes: &[SlotHash],
    epoch: Epoch,
    current_slot: Slot,
    timely_vote_credits: bool,
    deprecate_unused_legacy_vote_plumbing: bool,
) -> Result<(), VoteError>
```
1. `check_slots_are_valid()` — slot doğrulama
2. Her slot için `vote_state.process_next_vote_slot()` çağrısı

**`process_vote`** (Satır 758-790)
- Boş slots → `Err(EmptySlots)`
- `earliest_slot_in_history`'den eski slot'ları filtrele
- Tümü filtrelendiyse → `Err(VotesTooOldAllFiltered)`
- `process_vote_unfiltered()` çağrısı

**`process_vote_unchecked`** (Satır 792-808) — test/Tower için
- Slot_hashes synthetic olarak oluşturulur

**`process_slot_vote_unchecked`** (Satır 817-819) — tek slot için unchecked

#### Yetkilendirme Sistemi

**`authorize`** (Satır 821-866)
```rust
pub fn authorize<S: std::hash::BuildHasher>(
    vote_account: &mut BorrowedAccount,
    authorized: &Pubkey,
    vote_authorize: VoteAuthorize,
    signers: &HashSet<Pubkey, S>,
    clock: &Clock,
    feature_set: &FeatureSet,
) -> Result<(), InstructionError>
```

**VoteAuthorize::Voter:**
- Mevcut withdrawer OR mevcut voter imzalayabilir.
- `set_new_authorized_voter()` — `leader_schedule_epoch + 1`'de geçerli olur (epoch önceden planlama).

**VoteAuthorize::Withdrawer:**
- Sadece mevcut withdrawer imzalayabilir.
- Anlık geçerli.

**`update_validator_identity`** (Satır 868-888)
- Withdrawer imzası zorunlu.
- Yeni node key de imzalamalı (çift imza).

**`update_commission`** (Satır 890-937)
- `allow_commission_decrease_at_any_time` aktifse azaltmalar her zaman izinli.
- `commission_updates_only_allowed_in_first_half_of_epoch` aktifse artışlar sadece epoch'un ilk yarısında.
- `is_commission_update_allowed()`: `relative_slot * 2 <= slots_per_epoch`.

**`is_commission_increase`** (Satır 940-942)
```rust
pub fn is_commission_increase(vote_state: &VoteState, commission: u8) -> bool {
    commission > vote_state.commission
}
```

**`is_commission_update_allowed`** (Satır 946-958)
```rust
pub fn is_commission_update_allowed(slot: Slot, epoch_schedule: &EpochSchedule) -> bool {
    if let Some(relative_slot) = slot
        .saturating_sub(epoch_schedule.first_normal_slot)
        .checked_rem(epoch_schedule.slots_per_epoch)
    {
        relative_slot.saturating_mul(2) <= epoch_schedule.slots_per_epoch
    } else {
        true
    }
}
```

**`verify_authorized_signer`** (Satır 960-969)
```rust
fn verify_authorized_signer<S: std::hash::BuildHasher>(
    authorized: &Pubkey,
    signers: &HashSet<Pubkey, S>,
) -> Result<(), InstructionError>
```
Tüm yetkilendirme kontrollerinin alt fonksiyonu.

#### Para Çekme

**`withdraw`** (Satır 971-1030)
- Withdrawer imzası zorunlu.
- Kalan bakiye 0 ise:
  - Son 2 epoch içinde kredi kazanıldıysa → `Err(ActiveVoteAccountClose)`.
  - Aksi takdirde → `VoteState::default()` ile deinitialize.
- Kalan bakiye > 0 ise → rent-exempt minimum korunmalı.

#### Hesap Başlatma

**`initialize_account`** (Satır 1035-1059)
- Data uzunluğu feature flag'a göre doğru versiyona uymalı.
- Zaten init edilmişse → `Err(AccountAlreadyInitialized)`.
- Node key imzalamalı (`verify_authorized_signer(&vote_init.node_pubkey, signers)`).

**`verify_and_get_vote_state`** (Satır 1061-1077)
```rust
fn verify_and_get_vote_state<S: std::hash::BuildHasher>(
    vote_account: &BorrowedAccount,
    clock: &Clock,
    signers: &HashSet<Pubkey, S>,
) -> Result<VoteState, InstructionError>
```
1. Uninit kontrolü.
2. `convert_to_current()` — versiyon yükseltme.
3. `get_and_update_authorized_voter(clock.epoch)` — epoch bazlı voter güncelleme.
4. `verify_authorized_signer()`.

**`process_vote_with_account`** (Satır 1079-1109)
- `verify_and_get_vote_state()` — kimlik doğrulama.
- Feature flags ile `process_vote()`.
- Timestamp işleme: `vote_state.process_timestamp(*slot, timestamp)`.
- `set_vote_account_state()` ile kaydet.

**`process_vote_state_update`** (Satır 1111-1129)
- `verify_and_get_vote_state()`.
- `do_process_vote_state_update()`.
- `set_vote_account_state()`.

**`do_process_vote_state_update`** (Satır 1131-1153)
1. `check_update_vote_state_slots_are_valid()` — slot/hash doğrulama.
2. `process_new_vote_state()` — Tower BFT uygulama.

#### Hesap Oluşturma Fonksiyonları

**`create_account_with_authorized`** (Satır 1159-1185)
```rust
pub fn create_account_with_authorized(
    node_pubkey: &Pubkey,
    authorized_voter: &Pubkey,
    authorized_withdrawer: &Pubkey,
    commission: u8,
    lamports: u64,
) -> AccountSharedData
```
Genesis ve test araçları için kullanılır.

**`create_account`** (Satır 1188-1195) — wrapper (deprecated)

---

### 2.4 `programs/vote/src/vote_transaction.rs`

**Dosya Boyutu:** 105 satır
**Amaç:** Vote transaction'larını oluşturan yardımcı fonksiyonlar.

#### Fonksiyon Tanımları

**`new_vote_transaction`** (Satır 14-44)
```rust
pub fn new_vote_transaction(
    slots: Vec<Slot>,
    bank_hash: Hash,
    blockhash: Hash,
    node_keypair: &Keypair,
    vote_keypair: &Keypair,
    authorized_voter_keypair: &Keypair,
    switch_proof_hash: Option<Hash>,
) -> Transaction
```
- `Vote::new(slots, bank_hash)` ile oy oluşturur.
- `switch_proof_hash` varsa `vote_switch()`, yoksa `vote()` instruction kullanır.
- İki imza: `node_keypair` + `authorized_voter_keypair` (partial sign).

**`new_vote_state_update_transaction`** (Satır 46-74)
```rust
pub fn new_vote_state_update_transaction(
    vote_state_update: VoteStateUpdate,
    blockhash: Hash,
    node_keypair: &Keypair,
    vote_keypair: &Keypair,
    authorized_voter_keypair: &Keypair,
    switch_proof_hash: Option<Hash>,
) -> Transaction
```
`VoteStateUpdate` tipi kullanır; toplu lockout güncelleme.

**`new_compact_vote_state_update_transaction`** (Satır 76-104)
Kompakt format; wire formatı daha küçük, aynı mantık.

**Ortak Pattern:**
```rust
let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));
vote_tx.partial_sign(&[node_keypair], blockhash);
vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
```
Her vote transaction iki ayrı imza gerektirir: node ve authorized voter ayrı key olabilir.

---

## BÖLÜM 3: TOWER BFT STATE MACHINE ANALİZİ

### 3.1 Lockout Mekanizması

Solana'nın Tower BFT'si Tendermint'e benzer ancak daha az mesajlaşma gerektirir.

**Temel Kavramlar:**
- `MAX_LOCKOUT_HISTORY`: Bir validator'ın taşıyabileceği maksimum lockout sayısı.
- `LandedVote { lockout: Lockout, latency: u8 }`: Oylanan slot + latency.
- `Lockout { slot, confirmation_count }`: Belirli bir slot'un ne kadar confirmed olduğu.

**Lockout Kuralları (`process_new_vote_state` içinde):**
```
Her yeni oy (v_i, c_i):
- v_i > v_{i-1}               (sıralı slot'lar)
- c_i < c_{i-1}               (azalan confirmation)
- v_{i+1} <= v_i.last_locked_out_slot() (lockout korunması)
- c_i > 0 ve c_i <= MAX_LOCKOUT_HISTORY
```

**Finalization:**
Bir slot, `root_slot` haline geldiğinde finalize olur. Bu, `increment_credits()` tetikler.

### 3.2 Vote State Versiyonlama

```
VoteStateVersions:
├── V0_23_5  (eski)
├── V1_14_11 (geçiş)
└── Current  (yeni, vote latency içerir)
```

`set_vote_account_state()` içindeki feature-flag logic:
1. `vote_state_add_vote_latency` aktif değil → V1_14_11 zorla
2. Aktif + hesap büyükse → Current
3. Aktif + hesap küçükse + rent-exempt olamıyorsa → V1_14_11'e geri düş

### 3.3 Credit Sistemi

**Timely Vote Credits (öncesi):**
- Her root oluşumunda 1 kredi (buggy: root slot kendisi sayılıyordu)

**Timely Vote Credits (sonrası):**
- `earned_credits` başlar 0'dan
- Her finalized slot için `credits_for_vote_at_index()` kullanılır (latency bazlı)
- Daha erken oy → daha fazla kredi

### 3.4 Authority Sistemi

```
VoteState:
├── node_pubkey (validator kimliği)
├── authorized_voters: BTreeMap<Epoch, Pubkey>  (epoch bazlı voter)
└── authorized_withdrawer: Pubkey              (para çekme yetkisi)
```

**Voter Geçişi:**
`set_new_authorized_voter()` — `leader_schedule_epoch + 1` döneminde geçerli yeni voter. Bu, slot assignment döngüleriyle uyumlu.

---

## BÖLÜM 4: GÜVENLİK ANALİZİ

### 4.1 System Program Güvenlik Modeli

**İzolasyon Katmanları:**
1. **Owner kontrolü**: Sadece system program kendi hesaplarını değiştirebilir.
2. **Signer doğrulama**: `from` için imza, seed-derived için base imzası.
3. **Anti-replay**: Nonce'lar aynı slot'ta iki kez advance edilemez.
4. **Data koruma**: Veri taşıyan hesaplar transfer edemez.
5. **Rent koruması**: Nonce'lar her zaman rent-exempt olmalı.

**Kritik Güvenlik Noktaları:**

`create_account` içinde `to.get_lamports() > 0` kontrolü:
- Pre-funded hesaplara allocate yapılmasını engeller.
- Ancak `lamports == 0` başlangıçlı boş hesaplar oluşturmak mümkündür.

`MAX_PERMITTED_DATA_LENGTH = 10 MB` sınırı:
- Tek bir hesapta sonsuz veri birikimini engeller.
- State bloat saldırılarına karşı koruma.

### 4.2 Vote Program Güvenlik Modeli

**Çok Katmanlı Doğrulama:**
1. Owner kontrolü (`id()` == vote program).
2. Rent-exempt zorunluluğu (init sırasında).
3. Epoch-aware voter kimlik doğrulama.
4. Slot hash doğrulama (fork sahtekarlığı önleme).
5. Lockout ihlali tespiti.
6. Root gerilemesi önleme.

**Fork Sahtekarlığı Koruması:**
`check_update_vote_state_slots_are_valid` içindeki iki-pointer algoritması, her önerilen slot'un bu validator'ın gözlemlediği slot_hashes geçmişinde bulunduğunu garanti eder.

**Manipulation Önleme:**
Vote latency'nin validator tarafından manipüle edilemeyişi kritiktir. Latency, `VoteStateUpdate` işlendiğinde `current_slot - vote_slot` olarak hesaplanır; hiçbir validator kendi latency'sini seçemez.

**Active Vote Account Close Koruması:**
Son 2 epoch içinde kredi kazanmış validator, hesabını kapatamaz. Bu, aktif validator'ların stake'i ve slashing mekanizmasını atlamasını önler.

---

## BÖLÜM 5: NOVA SHIELD CHAIN İÇİN DERSLER

### 5.1 System Program Dersleri

**Ders 1: Seed-Derived Adresler (PDA)**
```rust
// Solana'da deterministik adres türetme
Pubkey::create_with_seed(base, seed, owner)
// Nova Shield Chain için:
// Program Derived Addresses (PDA) pattern'i benimse
// Base key + seed + program_id → deterministic account address
```
Nova Shield Chain'de program hesapları, PDA pattern'i kullanarak öngörülebilir adresler oluşturabilir. Bu, off-chain hesaplama gerektirmez.

**Ders 2: Checked Arithmetic**
```rust
from.checked_sub_lamports(lamports)?;
to.checked_add_lamports(lamports)?;
// NOT: lamports -= x veya += x değil
```
Tüm bakiye işlemleri `checked_*` metodlarıyla yapılmalı. Overflow panics yerine `Result` döndürür.

**Ders 3: Borrow Drop Sırası**
```rust
from.checked_sub_lamports(lamports)?;
drop(from);  // ÖNCE drop et
let mut to = instruction_context.try_borrow_instruction_account(...)?;  // SONRA borrow
to.checked_add_lamports(lamports)?;
```
Rust borrow checker'ın instruction context ile etkileşimi; aynı anda iki mutable borrow imkansız.

**Ders 4: Anti-Replay için Nonce Pattern**
```
Nonce State Machine:
Uninitialized ──initialize──> Initialized(data)
Initialized(data) ──advance──> Initialized(new_data)
Initialized(data) ──withdraw(full)──> Uninitialized
```
Transaction replay koruması için durable nonce şarttır. Nova Shield Chain'de de durable nonce implementasyonu düşünülmeli.

**Ders 5: Allocate/Assign Separation**
Solana'da `Allocate` (alan tahsisi) ve `Assign` (sahip atama) ayrı instruction'lardır. Bu:
- Atomiklik olmadan kademeli kurulum imkanı tanır.
- `AllocateWithSeed` + `AssignWithSeed` → `CreateAccountWithSeed` ile aynı sonuç.

### 5.2 Vote Program Dersleri

**Ders 6: Tower BFT Lockout Invariants**
```
Her vote tower için:
1. Slot'lar sıralı
2. Confirmation'lar azalan
3. Lockout süreleri örtüşmemeli
4. Root asla geri gitmemeli
```
Nova Shield Chain'de validator oylama mekanizması benzer invariant'ları uygulamalı.

**Ders 7: Epoch-Aware Authority**
```rust
// Voter yetkisi hemen değil, gelecek epoch'ta geçerli
vote_state.set_new_authorized_voter(
    authorized,
    clock.epoch,
    clock.leader_schedule_epoch + 1,  // gelecek epoch
    |current_voter| verify_authorized_signer(current_voter, signers)
)?;
```
Nova Shield Chain'de validator key rotations epoch sınırlarına hizalanmalı.

**Ders 8: Feature Flag ile Güvenli Migration**
```rust
if feature_set.is_active(&feature_set::vote_state_add_vote_latency::id()) {
    // Yeni davranış
} else {
    // Eski davranış (backwards compat)
}
```
Runtime upgrade'lar için feature flag sistemi kritik. Solana'nın feature gate mekanizması Nova Shield Chain'de benimsenebilir.

**Ders 9: Timely Vote Credits**
Vote latency hesaplaması:
```rust
new_vote.latency = VoteState::compute_vote_latency(new_vote.slot(), current_slot);
// Latency = current_slot - vote_slot
```
Nova Shield Chain'de validator performans metrikleri için benzer latency tracking uygulanabilir.

**Ders 10: Commission Update Kısıtlaması**
```rust
// Artışlar sadece epoch'un ilk yarısında
relative_slot.saturating_mul(2) <= epoch_schedule.slots_per_epoch
```
Validator komisyon değişikliklerinin zaman kısıtlaması, staker'ları manipülasyondan korur.

**Ders 11: Active Account Close Koruması**
```rust
let reject = vote_state.epoch_credits.last()
    .map(|(last_epoch, _, _)| current_epoch.saturating_sub(*last_epoch) < 2)
    .unwrap_or(false);
if reject {
    return Err(VoteError::ActiveVoteAccountClose.into());
}
```
Aktif validator'ların slashing'den kaçmak için hesabı kapatmasını engelle.

**Ders 12: Slot Hash Fork Doğrulama**
İki-pointer algoritması ile fork üyeliği doğrulama, `O(n + m)` karmaşıklıkta (n: vote slots, m: slot_hashes) ve sıralı listeler sayesinde verimli çalışır. Nova Shield Chain'de consensus oylama doğrulamasında benzer fork-check uygulanmalı.

**Ders 13: Vote Transaction Çift İmza**
```rust
vote_tx.partial_sign(&[node_keypair], blockhash);
vote_tx.partial_sign(&[authorized_voter_keypair], blockhash);
```
Node kimliği ve oy yetkisi ayrı key'lere sahip olabilir. Bu, key güvenliği için önemli: oy yetkisini tehlikeye atmadan node key rotasyonu yapılabilir.

### 5.3 Mimari Dersler

**Ders 14: Compute Unit Farklılaştırması**
- System program: 150 CU
- Vote program: 2,100 CU (14×)

Nova Shield Chain'de farklı instruction türleri için ayrı compute limits belirlenmeli.

**Ders 15: VoteStateVersions Upgrade Pattern**
```
Versiyon geçişi:
1. Yeni versiyon tanımla (Current)
2. Feature flag ekle
3. Okuma her versiyonu destekler
4. Yazma feature flag durumuna göre versiyon seçer
5. Old validators yeni format okuyamaz → epoch'larca geçiş süresi
```

**Ders 16: Sysvar Dependency Injection**
```rust
let clock = get_sysvar_with_account_check::clock(invoke_context, instruction_context, 1)?;
let rent = get_sysvar_with_account_check::rent(invoke_context, instruction_context, 3)?;
```
Sysvar'lar (clock, rent, slot_hashes) hesap olarak instruction'a geçirilir. Bu, testability'yi artırır: mock sysvar'lar enjekte edilebilir.

---

## ÖZET TABLO

### System Program Instruction'ları

| Instruction | Gereken İmzalar | Güvenlik Kontrolleri |
|---|---|---|
| CreateAccount | from + to | to boş, space <= 10MB |
| CreateAccountWithSeed | from + base | seed türetme doğrulama |
| Assign | account | owner değişimi |
| Transfer | from | from data boş, yeterli bakiye |
| TransferWithSeed | base | seed eşleşme |
| AdvanceNonceAccount | authority | blockhash expired, recent_blockhashes dolu |
| WithdrawNonceAccount | authority/self | rent-exempt koruma, tam çekim için expired nonce |
| InitializeNonceAccount | - | writable, rent-exempt |
| AuthorizeNonceAccount | authority | state initialized |
| UpgradeNonceAccount | - | system owner, upgrade available |
| Allocate | account | data boş, space <= 10MB |
| AllocateWithSeed | base | seed doğrulama |
| AssignWithSeed | base | seed doğrulama |

### Vote Program Instruction'ları

| Instruction | Gereken İmzalar | Güvenlik Kontrolleri |
|---|---|---|
| InitializeAccount | node_pubkey | rent-exempt, uninitialized |
| Authorize | voter/withdrawer | epoch-aware voter, withdrawer sınırlaması |
| UpdateValidatorIdentity | withdrawer + node | çift imza |
| UpdateCommission | withdrawer | epoch timing (artışlar için) |
| Vote | authorized_voter | slot_hashes doğrulama |
| UpdateVoteState | authorized_voter | lockout invariants, fork doğrulama |
| Withdraw | withdrawer | active account koruması, rent-exempt |

---

**Analiz Tarihi:** 2026-07-22
**Agent:** 07
**Kaynak:** Solana programs/system + programs/vote
