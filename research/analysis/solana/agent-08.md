# Solana Agent-08 Analizi
## Stake, Config, Address Lookup Table, Compute Budget, Loader-v4 Programları

**Analiz Tarihi:** 2026-07-22
**Kapsam:** 5 program dizini, toplam ~15 dosya

---

## 1. programs/stake/src/ — Stake Programı

### Dosya Listesi
- `lib.rs` (38 satır)
- `config.rs` (43 satır)
- `points.rs` (250 satır)
- `rewards.rs` (648 satır)
- `stake_state.rs` (~1200+ satır)
- `stake_instruction.rs` (~500+ satır + kapsamlı testler)

---

### 1.1 lib.rs

**Satır 21-23:** `add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64`
- Genesis'e stake config account ekler
- Stake programının genesis başlangıç noktası

**Satır 29-37:** `get_minimum_delegation(feature_set: &FeatureSet) -> u64`
- Feature flag `stake_raise_minimum_delegation_to_1_sol` aktifse → 1 SOL (1_000_000_000 lamport)
- Değilse → legacy `MINIMUM_STAKE_DELEGATION` (1 lamport)
- `#[inline(always)]` ile optimize edilmiş

**Tasarım Kararı:** Minimum delegation miktarı feature flag ile kontrol ediliyor. Bu sayede consensus bozulmadan ağ parametresi değiştirilebiliyor.

---

### 1.2 config.rs — Stake Config

**Satır 21-25:** `from(account: &BorrowedAccount) -> Option<Config>`
- Config program datasından Config deserialize eder
- `get_config_data` + `bincode::deserialize` pipeline

**Satır 28-30:** `create_account(lamports: u64, config: &Config) -> AccountSharedData`
- Genesis veya test için stake config account oluşturur

**Satır 33-42:** `add_genesis_account(genesis_config: &mut GenesisConfig) -> u64`
- Default Config ile genesis account oluşturur
- Rent exempt minimum balance'ı hesaplar, en az 1 lamport ayarlar

**Config İçeriği (solana_sdk::stake::config::Config):**
- `warmup_cooldown_rate: f64` — epoch başına maksimum aktif stake değişim oranı (varsayılan 0.25, yani %25)
- `slash_penalty: u8` — slash cezası (kullanılmamış, gelecek rezerv)

---

### 1.3 points.rs — Puan Hesaplama

#### Struct ve Enum Tanımları

**Satır 21-24:** `struct PointValue`
```rust
pub struct PointValue {
    pub rewards: u64, // dağıtılacak toplam lamport
    pub points: u128, // bu lamportların dağıtılacağı toplam puan
}
```
- Ödül oranı = rewards / points (tamsayı bölme, kesirli lamport kayıp)

**Satır 27-31:** `struct CalculatedStakePoints` (pub(crate))
```rust
pub(crate) struct CalculatedStakePoints {
    pub(crate) points: u128,
    pub(crate) new_credits_observed: u64,
    pub(crate) force_credits_update_with_skipped_reward: bool,
}
```

**Satır 34-43:** `enum InflationPointCalculationEvent`
- Debug/tracing için kullanılan event enum'u
- Variants: `CalculatedPoints`, `SplitRewards`, `EffectiveStakeAtRewardedEpoch`, `RentExemptReserve`, `Delegation`, `Commission`, `CreditsObserved`, `Skipped`

**Satır 50-60:** `enum SkippedReason`
- `DisabledInflation` — enflasyon kapalı
- `JustActivated` — stake bu epoch'ta aktifleşti
- `TooEarlyUnfairSplit` — validator ve staker'dan birine 0 lamport düşüyor
- `ZeroPoints` — kazanılan puan yok
- `ZeroPointValue` — PointValue.points == 0
- `ZeroReward` — hesaplanan ödül 0 lamport
- `ZeroCreditsAndReturnZero/Current/Rewinded` — kredi durumları

#### Fonksiyonlar

**Satır 70-87:** `calculate_points(stake_state, vote_state, stake_history, new_rate_activation_epoch) -> Result<u128>`
- Runtime tarafından kullanılan public utility
- StakeStateV2::Stake değilse InstructionError::InvalidAccountData

**Satır 89-104:** `calculate_stake_points(...)` (private)
- `calculate_stake_points_and_credits`'i çağırır, sadece points field'ını döner

**Satır 109-211:** `calculate_stake_points_and_credits(stake, new_vote_state, stake_history, tracer, new_rate_activation_epoch) -> CalculatedStakePoints` (pub(crate))

**PUAN HESAPLAMA ALGORİTMASI:**

```
1. credits_in_vote = vote_state.credits()
2. credits_in_stake = stake.credits_observed

3. Eğer credits_in_vote < credits_in_stake:
   → force_credits_update = true (vote account yeniden yaratıldı)
   → points = 0, new_credits = credits_in_vote

4. Eğer credits_in_vote == credits_in_stake:
   → delinquent validator, force_credits_update = false
   → points = 0, new_credits = credits_in_stake

5. Eğer credits_in_vote > credits_in_stake:
   → Her epoch için iterate et:
     - stake_amount = stake.delegation.stake(epoch, stake_history)
     - earned_credits = (credits_in_stake < initial_epoch_credits)?
         full epoch: final - initial
       (credits_in_stake < final_epoch_credits)?
         partial: final - credits_observed
       else: 0
     - earned_points = stake_amount * earned_credits  (u128 * u128)
     - total_points += earned_points
```

**Kritik tasarım:** `earned_points = stake_amount * earned_credits` — bu basit çarpım Solana'nın tüm ödül sisteminin temelidir. Küçük stake'ler az kredi ile az puan alır.

**Test (satır 218-248):** 10M SOL stake, 193,536,000 kredi → overflow yok (u128 kullanımı)

---

### 1.4 rewards.rs — Ödül Dağıtımı

#### Struct Tanımları

**Satır 24-28:** `struct CalculatedStakeRewards` (private)
```rust
struct CalculatedStakeRewards {
    staker_rewards: u64,
    voter_rewards: u64,
    new_credits_observed: u64,
}
```

#### Fonksiyonlar

**Satır 33-79:** `pub fn redeem_rewards(rewarded_epoch, stake_state, stake_account, vote_state, point_value, stake_history, tracer, new_rate_activation_epoch) -> Result<(u64, u64)>`
- Runtime tarafından her epoch sonunda çağrılır
- StakeStateV2::Stake değilse hata
- Başarılı olursa stake account'a lamport ekler ve state günceller
- (staker_reward, voter_reward) tuple döner

**Satır 81-119:** `fn redeem_stake_rewards(...)` (private)
- `calculate_stake_rewards` → bazı durumlarda `None` dönebilir
- None ise `NoCreditsToRedeem` hatası
- Başarı durumunda:
  - `stake.credits_observed = new_credits_observed`
  - `stake.delegation.stake += staker_rewards` (compound interest!)

**Satır 127-225:** `fn calculate_stake_rewards(rewarded_epoch, stake, point_value, vote_state, stake_history, tracer, new_rate_activation_epoch) -> Option<CalculatedStakeRewards>` (private)

**ÖDÜL HESAPLAMA FORMÜLÜ:**

```
points = calculate_stake_points_and_credits(...)

# Eğer force_credits_update (enflasyon kapalı veya just-activated):
→ staker_rewards = 0, voter_rewards = 0, new_credits = current

# Eğer points == 0:
→ None (ödül yok)

# Eğer point_value.points == 0:
→ None (geçersiz)

# Ödül hesapla:
total_rewards = (points * point_value.rewards) / point_value.points
             = (u128 * u64) / u128

(voter_rewards, staker_rewards, is_split) = vote_state.commission_split(total_rewards)

# Adil bölme kontrolü:
if is_split && (voter_rewards == 0 || staker_rewards == 0):
    → None (TooEarlyUnfairSplit — birileri 0 alıyorsa iptal)
```

**Komisyon formülü** (`commission_split` — vote_state.rs'de):
- `voter_share = (total * commission as u64 + 99) / 100` (ceiling division)
- `staker_share = total - voter_share`

**Kritik test senaryoları (satır 236-615):**
- Sıfır kredi → None
- 2 kredi → stake * 2 ödül
- Kısmi kredi gözlemi (credits_observed = 1, 2 kredi varsa → 1 kredi ödül)
- Enflasyon kapalı → 0 ödül ama credits_observed güncellenir
- credits_observed > vote credits → force_credits_update = true
- activation_epoch == rewarded_epoch → 0 ödül (just activated)
- commission = 1 → TooEarlyUnfairSplit (küçük miktarlarda)
- commission = 99 → TooEarlyUnfairSplit

---

### 1.5 stake_state.rs — Stake State Machine

#### Utility Fonksiyonları (satır 37-59)

**Satır 37:** `pub fn from<T: ReadableAccount + StateMut<StakeStateV2>>(account: &T) -> Option<StakeStateV2>`
**Satır 41:** `pub fn stake_from<T>(...) -> Option<Stake>`
**Satır 45:** `pub fn delegation_from(account: &AccountSharedData) -> Option<Delegation>`
**Satır 49:** `pub fn authorized_from(account: &AccountSharedData) -> Option<Authorized>`
**Satır 53:** `pub fn lockup_from<T>(...) -> Option<Lockup>`
**Satır 57:** `pub fn meta_from(account: &AccountSharedData) -> Option<Meta>`

#### Warmup/Cooldown

**Satır 61-69:** `pub(crate) fn new_warmup_cooldown_rate_epoch(invoke_context: &InvokeContext) -> Option<Epoch>`
- `reduce_stake_warmup_cooldown` feature aktifse epoch döner
- Bu epoch'tan itibaren warmup/cooldown oranı %100'e çıkar (neredeyse anlık aktivasyon)

**Satır 71-82:** `fn get_stake_status(invoke_context, stake, clock) -> Result<StakeActivationStatus>`
- `stake_history` sysvar'dan okur
- `stake.delegation.stake_activating_and_deactivating(epoch, history, new_rate_epoch)` çağırır
- Sonuç: `{effective, activating, deactivating}` lamport değerleri

#### Stake State Machine — Ana Durumlar (StakeStateV2 enum, solana_sdk'da)

```
Uninitialized
    ↓ initialize()
Initialized(Meta)
    ↓ delegate()
Stake(Meta, Stake, StakeFlags)
    - activation_epoch = current
    - deactivation_epoch = u64::MAX

Stake → cooldown:
    deactivate()
    - deactivation_epoch = current

Stake → tam deaktif:
    withdraw() all lamports
    → Uninitialized
```

**Warmup mekanizması:**
- Yeni stake `Delegation.activation_epoch = clock.epoch` ile kaydedilir
- Her epoch: `stake_history[epoch]` entry'si güncellenir
- Aktif stake = `effective` = min(delegated, history'deki warmup_rate * total_stake)
- Eski oran: %25/epoch, yeni oran (feature sonrası): ~%100 (neredeyse anlık)

**Cooldown mekanizması:**
- `deactivation_epoch` set edilir
- Sonraki epoch(larda) stake azalır: `deactivating = effective * cooldown_rate`
- Tamamen deaktif olunca `effective = 0` ve withdraw yapılabilir

#### Fonksiyonlar

**Satır 84-133:** `fn redelegate_stake(invoke_context, stake, stake_lamports, voter_pubkey, vote_state, clock, stake_history) -> Result<(), StakeError>`
- Eğer stake hâlâ aktifse:
  - Aynı validator'a, bu epoch'ta deactivate edilmişse → deactivation_epoch = u64::MAX (iptal)
  - Farklı validator'a → `TooSoonToRedelegate` hatası
- Deaktif ise → yeni delegation ayarla, activation_epoch = current

**Satır 135-145:** `pub(crate) fn new_stake(stake, voter_pubkey, vote_state, activation_epoch) -> Stake`
```rust
Stake {
    delegation: Delegation::new(voter_pubkey, stake, activation_epoch),
    credits_observed: vote_state.credits(),
}
```

**Satır 147-171:** `pub fn initialize(stake_account, authorized, lockup, rent) -> Result<(), InstructionError>`
- Account boyutu `StakeStateV2::size_of()` olmalı
- State `Uninitialized` olmalı
- Lamport >= rent_exempt_reserve olmalı
- Başarılı → `StakeStateV2::Initialized(Meta { rent_exempt_reserve, authorized, lockup })`

**Satır 176-205:** `pub fn authorize(stake_account, signers, new_authority, stake_authorize, clock, custodian)`
- Hem `Stake` hem `Initialized` state'de çalışır
- `meta.authorized.authorize(signers, ...)` → lockup kontrolü dahil

**Satır 208-240:** `pub fn authorize_with_seed(...)`
- Base pubkey + seed + owner'dan türetilmiş PDA ile yetkilendirme

**Satır 243-295:** `pub fn delegate(invoke_context, ..., stake_account_index, vote_account_index, clock, stake_history, signers, feature_set)`
- Vote account owner kontrolü (solana_vote_program::id())
- `Initialized` → yeni `Stake` oluştur (`new_stake`)
- `Stake` → `redelegate_stake` çağır

**Satır 297-336:** `fn deactivate_stake(invoke_context, stake, stake_flags, epoch)`
- Feature `stake_redelegate_instruction` aktifse:
  - `MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED` flag varsa → activating == 0 kontrolü
- `stake.deactivate(epoch)` → `deactivation_epoch = epoch`

**Satır 338-351:** `pub fn deactivate(invoke_context, stake_account, clock, signers)`

**Satır 353-370:** `pub fn set_lockup(stake_account, lockup, signers, clock)`
- Hem `Initialized` hem `Stake` state'de çalışır

**Satır 372-523:** `pub fn split(invoke_context, ..., stake_account_index, lamports, split_index, signers)`
- Split hedef account: `Uninitialized`, `StakeStateV2::size_of()` boyutunda, stake programına ait
- `Stake` state:
  - `validate_split_amount` → kaynak/hedef bakiye kontrolü
  - `stake.split(remaining_delta, split_stake_amount)` → stake azalt
  - Hedef account'a `StakeStateV2::Stake` yaz
- `Initialized` state: sadece meta kopyalanır
- `Uninitialized`: signer kontrolü, lamport transferi

**Satır 525-588:** `pub fn merge(invoke_context, ..., stake_account_index, source_account_index, clock, stake_history, signers)`
- Her iki account da stake programına ait olmalı
- `MergeKind::get_if_mergeable` ile her ikisinin durumu alınır
- `stake_merge_kind.merge(source)` → None veya yeni state
- Source → `Uninitialized` ve bakiyesi destination'a aktarılır

**Satır 590-705:** `pub fn redelegate(invoke_context, ..., stake_account, uninitialized_stake_account_index, vote_account_index, signers)`
- Kaynak stake: fully active, activating veya deactivating olmamalı
- Aynı vote account'a redelegate → `RedelegateToSameVoteAccount` hatası
- 1. Kaynak stake'i deactivate et
- 2. effective_stake lamport'u uninitialized account'a transfer et
- 3. Uninitialized account'ı yeni delegation ile `Stake` state'e getir
- `StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED` flag set edilir

**Satır 707-812:** `pub fn withdraw(transaction_context, ..., lamports, clock, stake_history, withdraw_authority_index, custodian_index, new_rate_activation_epoch)`
- `Stake` state: deactivating ise hesaplanan stake kadar reserve, değilse full stake + rent
- `Initialized` state: sadece rent_exempt_reserve
- `Uninitialized`: self-signed, hiçbir kısıtlama yok
- Lockup kontrol edilir (custodian olmadan)
- Tam çekim → `Uninitialized`'a dönüş

**Satır 814-865:** `pub(crate) fn deactivate_delinquent(...)`
- Herhangi biri çağırabilir (permissionless deactivation)
- Delinquent vote account: son N epoch'ta hiç vote atmamış
- Reference vote account: yakın zamanda aktif (son 2 epoch'ta vote atmış)
- `eligible_for_deactivate_delinquent` kontrolü
- Başarılı → `deactivate_stake` çağrısı

#### ValidatedDelegatedInfo ve ValidatedSplitInfo

**Satır 869-890:** `struct ValidatedDelegatedInfo { stake_amount: u64 }`
**Satır 875-890:** `fn validate_delegated_amount(account, meta, feature_set)`
- `stake_amount = lamports - rent_exempt_reserve`
- `stake_amount < minimum_delegation` → `InsufficientDelegation` hatası

**Satır 894-983:** `struct ValidatedSplitInfo { source_remaining_balance: u64, destination_rent_exempt_reserve: u64 }`
**Satır 904-983:** `fn validate_split_amount(...)`
- lamports == 0 → `InsufficientFunds`
- lamports > source → `InsufficientFunds`
- source_remaining < source_minimum (rent + min_delegation) → `InsufficientFunds`
- source_is_active && source_remaining != 0 && dest < dest_rent_exempt → `InsufficientFunds`
- split < destination_balance_deficit → `InsufficientFunds`

#### MergeKind Enum

**Satır 985-989:**
```rust
enum MergeKind {
    Inactive(Meta, u64, StakeFlags),        // effective == 0, activating == 0, deactivating == 0
    ActivationEpoch(Meta, Stake, StakeFlags), // effective == 0, activating > 0
    FullyActive(Meta, Stake),               // effective > 0, activating == 0, deactivating == 0
}
```

**Satır 1009-1042:** `get_if_mergeable` — transient stake (activating/deactivating) merge edilemez

**Satır 1044-1064:** `metas_can_merge` — authorized ve lockup uyumlu olmalı

**Satır 1066-1081:** `active_delegations_can_merge` — aynı voter, her ikisi de deactivate edilmemiş

**Satır 1083-1148:** `merge` — merge logic:
- Inactive + Inactive → None (sadece lamport transfer)
- Inactive + ActivationEpoch → None
- ActivationEpoch + Inactive → stake.delegation.stake += source_lamports
- ActivationEpoch + ActivationEpoch → weighted average credits_observed
- FullyActive + FullyActive → weighted credits, source.rent_exempt_reserve eklenmez (magic activation koruması)
- Diğer kombinasyonlar → `MergeMismatch`

**Satır 1151-1161:** `fn merge_delegation_stake_and_credits_observed(stake, absorbed_lamports, absorbed_credits_observed)`

**Satır 1188-son:** `fn stake_weighted_credits_observed(stake, absorbed_lamports, absorbed_credits_observed) -> Option<u64>`
- Matematiksel türetme (satır 1163-1187'deki yorumda):
  ```
  a3.c = (a1.c * a1.s + a2.c * a2.s) / (a1.s + a2.s)
  ```
- Overflow koruması için u128 kullanımı

---

### 1.6 stake_instruction.rs — Instruction Dispatcher

**Satır 54:** `pub const DEFAULT_COMPUTE_UNITS: u64 = 750;`

**Satır 28-52:** `fn get_optional_pubkey(...)` — opsiyonel 4. hesap için pubkey okuma

#### Desteklenen Instructions

| Instruction | Satır | Açıklama |
|-------------|-------|----------|
| `Initialize` | 73 | Authorized + Lockup ile başlat |
| `Authorize` | 78 | Staker/Withdrawer değiştir |
| `AuthorizeWithSeed` | 95 | PDA ile yetkilendir |
| `DelegateStake` | 116 | Vote account'a delegate et |
| `Split` | 153 | Stake'i ikiye böl |
| `Merge` | 167 | İki stake'i birleştir |
| `Withdraw` | 189 | Lamport çek |
| `Deactivate` | 218 | Cooldown başlat |
| `SetLockup` | 224 | Lockup güncelle |
| `InitializeChecked` | 229 | Withdrawer imzası zorunlu versiyonu |
| `AuthorizeChecked` | 250 | Yeni authority imzası zorunlu |
| `AuthorizeCheckedWithSeed` | 273 | Seed + imza zorunlu |
| `SetLockupChecked` | 301 | Custodian imzası kontrollü |
| `GetMinimumDelegation` | 314 | Return data ile minimum delegation döner |
| `DeactivateDelinquent` | 322 | Delinquent stake deactivate et |
| `Redelegate` | 337 | Farklı validator'a redelegate |

---

## 2. programs/config/src/ — Config Programı

### 2.1 lib.rs

**Satır 19-22:** `pub trait ConfigState: serde::Serialize + Default`
```rust
pub trait ConfigState: serde::Serialize + Default {
    fn max_space() -> u64;
}
```

**Satır 26-30:** `impl ConfigState for StakeConfig`
- `max_space() = serialized_size(&StakeConfig::default())`

**Satır 33-39:** `pub struct ConfigKeys`
```rust
pub struct ConfigKeys {
    #[serde(with = "short_vec")]
    pub keys: Vec<(Pubkey, bool)>,  // (pubkey, is_signer)
}
```
- Config account'ın başında tutulan "signer listesi"
- short_vec: compact encoding (1-3 byte length prefix)

**Satır 41-44:** `impl ConfigKeys { pub fn serialized_size(keys) -> u64 }`

**Satır 47-51:** `pub fn get_config_data(bytes: &[u8]) -> Result<&[u8], bincode::Error>`
- ConfigKeys'i parse eder, offset'i hesaplar, geriye kalan data slice'ı döner
- Veri formatı: `[ConfigKeys bincode | actual config data bincode]`

**Satır 54-67:** `pub fn create_config_account<T: ConfigState>(keys, config_data, lamports) -> AccountSharedData`

### 2.2 config_instruction.rs

**Satır 10-14:** `fn initialize_account<T: ConfigState>(config_pubkey) -> Instruction` (private)

**Satır 17-34:** `pub fn create_account<T: ConfigState>(from, config, lamports, keys) -> Vec<Instruction>`
- system_instruction::create_account → initialize_account
- 2 instruction döner

**Satır 37-51:** `pub fn store<T: ConfigState>(config_pubkey, is_config_signer, keys, data) -> Instruction`
- Güncelleme instruction'ı
- Signer olan key'ler için AccountMeta::new(..., true) ekler
- Config account kendisi de signer olabilir

### 2.3 config_processor.rs

**Satır 14:** `pub const DEFAULT_COMPUTE_UNITS: u64 = 450;`

**İşlem Akışı (satır 16-132):**

1. `key_list: ConfigKeys = limited_deserialize(data)?` — gelen instruction verisini parse et
2. Config account'ın owner'ı doğru mu? → `InvalidAccountOwner`
3. Mevcut `current_data: ConfigKeys = deserialize(config_account.get_data())?`
4. `current_signer_keys` — mevcut config'daki signer pubkey listesi
5. Eğer `current_signer_keys.is_empty()`:
   - Config account kendisi signer olmalı (initialization)
6. Her signer için:
   - Account listesinde var mı?
   - `signer_account.is_signer()` doğru mu?
   - `signer_account.get_key() == signer` doğru mu?
   - Mevcut signer listesinde var mı? (önceden izin verilmiş mi?)
7. Duplicate key kontrolü (`BTreeSet`)
8. `current_signer_keys.len() > counter` → tüm signerlar geldi mi kontrolü
9. Config account data boyutu yeterli mi?
10. `config_account.data_mut()[..data.len()].copy_from_slice(data)` — direkt yaz

**Güvenlik özellikleri:**
- Mevcut signer listesindeki tüm pubkey'lerin imzası gerekli (M-of-M)
- Yeni config'daki signer listesi mevcut listeden farklıysa tüm eski signerlar onaylamalı
- Duplicate key → `InvalidArgument`
- Veri büyük gelirse → `InvalidInstructionData`

### 2.4 date_instruction.rs — Tarih Oracle Örneği

**Satır 16-27:** `pub struct DateConfig`
```rust
pub struct DateConfig {
    #[serde(with = "ts_seconds")]
    pub date_time: DateTime<Utc>,
}
```
- `ConfigState` implementasyonu olan örnek bir config tipi
- Zaman damgası oracle'ı olarak kullanılabilir

**Satır 45-51:** `pub fn create_account(payer, date_pubkey, lamports) -> Vec<Instruction>`
**Satır 55-58:** `pub fn store(date_pubkey, date_time) -> Instruction`

---

## 3. programs/address-lookup-table/src/ — Adres Arama Tablosu

### 3.1 lib.rs (satır 1-15)

- `cfg(not(target_os = "solana"))` ile `processor` modülü açılır
- v1.17.0'dan itibaren `solana_program::address_lookup_table`'a yönlendirme

### 3.2 processor.rs

**Satır 22:** `pub const DEFAULT_COMPUTE_UNITS: u64 = 750;`

#### Desteklenen Instructions

| Instruction | Satır |
|-------------|-------|
| `CreateLookupTable { recent_slot, bump_seed }` | 29 |
| `FreezeLookupTable` | 33 |
| `ExtendLookupTable { new_addresses }` | 34 |
| `DeactivateLookupTable` | 37 |
| `CloseLookupTable` | 40 |

#### Processor Struct

**Satır 48:** `pub struct Processor;`

**Satır 50-170:** `fn create_lookup_table(invoke_context, untrusted_recent_slot, bump_seed)`
- Account[0]: yeni lookup table (PDA)
- Account[1]: authority (signer)
- Account[2]: payer (signer)
- PDA derivation: `create_program_address([authority_key, slot_le_bytes, bump_seed], &id())`
- `recent_slot` slot_hashes sysvar'dan doğrulanır (replay attack koruması)
- Zaten var olan ama programın sahibi olduğu account varsa idempotent OK
- `LookupTableMeta::new(authority_key)` ile state yazılır

**Satır 172-221:** `fn freeze_lookup_table(invoke_context)`
- `meta.authority = None` → immutable hale getirir
- Deaktif veya boş tablo freeze edilemez

**Satır 223-339:** `fn extend_lookup_table(invoke_context, new_addresses: Vec<Pubkey>)`
- Maksimum 256 adres (LOOKUP_TABLE_MAX_ADDRESSES = u8::MAX + 1)
- Her slot için `last_extended_slot` ve `last_extended_slot_start_index` güncellenir
- Visibility delay: o slot'taki değişiklikler bir sonraki slot'tan itibaren görünür
- Rent hesaplama: yeni boyut için gerekli lamport payer'dan alınır

**Satır 342-389:** `fn deactivate_lookup_table(invoke_context)`
- `meta.deactivation_slot = clock.slot`
- Frozen tablo deaktif edilemez

**Satır 391-468:** `fn close_lookup_table(invoke_context)`
- Deaktivasyon tamamlanmış olmalı (slot_hashes'ten kontrol)
- `LookupTableStatus::Activated` → hata
- `LookupTableStatus::Deactivating { remaining_blocks }` → hata
- `LookupTableStatus::Deactivated` → OK
- Recipient account[2] olmalı (tablo kendisi olamaz)
- `lookup_table_account.set_data_length(0)?; set_lamports(0)?;`

**Tablo Yaşam Döngüsü:**
```
[PDA oluşturulur]
    ↓ CreateLookupTable
Active (authority var, deactivation_slot = u64::MAX)
    ↓ ExtendLookupTable (adres ekle)
Active (dolu olabilir, maks 256)
    ↓ FreezeLookupTable (opsiyonel)
Active + Frozen (authority = None)
    ↓ DeactivateLookupTable (sadece frozen olmayan için)
Deactivating (slot_hashes'te çözülene kadar)
    ↓ slot_hashes'ten düşünce
Deactivated
    ↓ CloseLookupTable
[Account silinir, lamport geri alınır]
```

---

## 4. programs/compute-budget/src/lib.rs

**Satır 3:** `pub const DEFAULT_COMPUTE_UNITS: u64 = 150;`

**Satır 5-8:**
```rust
declare_process_instruction!(Entrypoint, DEFAULT_COMPUTE_UNITS, |_invoke_context| {
    // Do nothing, compute budget instructions handled by the runtime
    Ok(())
});
```

- Compute budget instruction'ları runtime düzeyinde transaction parse sırasında işlenir
- On-chain program hiçbir şey yapmaz
- Compute cost: 150 CU (en ucuz built-in program)

**Compute Budget Instructions (runtime tarafından):**
- `SetComputeUnitLimit(u32)` — transaction'ın CU limiti
- `SetComputeUnitPrice(u64)` — priority fee (micro-lamport/CU)
- `SetLoadedAccountsDataSizeLimit(u32)` — yüklenen account data boyutu

---

## 5. programs/loader-v4/src/lib.rs — Loader-v4 Programı

### Sabitler ve Struct'lar

**Satır 39:** `pub const DEFAULT_COMPUTE_UNITS: u64 = 2_000;`

**LoaderV4State (solana_sdk::loader_v4):**
```rust
struct LoaderV4State {
    slot: Slot,                    // son deployment slot'u
    authority_address: Pubkey,     // authority
    status: LoaderV4Status,        // Retracted | Deployed | Finalized
}
```

**LoaderV4Status:**
- `Retracted` — yazılabilir, deploy edilmemiş
- `Deployed` — çalışabilir, retract edilebilir
- `Finalized` — immutable, authority transfer yapılamaz

### Unsafe Bloklar

**Satır 41-53:** `pub fn get_state(data: &[u8]) -> Result<&LoaderV4State, InstructionError>`
```rust
unsafe {
    Ok(std::mem::transmute::<
        &[u8; LoaderV4State::program_data_offset()],
        &LoaderV4State,
    >(data))
}
```
- Zero-copy deserialization: raw byte slice'ı struct reference'ına cast eder
- Safety: slice boyutu kontrol edildikten sonra transmute

**Satır 55-67:** `fn get_state_mut(data: &mut [u8]) -> Result<&mut LoaderV4State, InstructionError>`
- Aynı pattern, mut versiyonu

### eBPF VM Altyapısı

**Satır 69-95:** `pub fn create_program_runtime_environment_v2<'a>(compute_budget, debugging_features) -> BuiltinProgram<InvokeContext<'a>>`

**Config parametreleri:**
- `enable_sbpf_v1: false` — eski format devre dışı
- `enable_sbpf_v2: true` — yeni SBF format
- `optimize_rodata: true` — read-only data optimization
- `new_elf_parser: true` — geliştirilmiş ELF parser
- `aligned_memory_mapping: true` — hizalı bellek mapping
- `reject_broken_elfs: true` — bozuk ELF reddet
- `external_internal_function_hash_collision: true` — hash çakışması koruması
- `reject_callx_r10: true` — güvenlik: r10 üzerinden dolaylı call yasaklı

**Satır 97-106:** `fn calculate_heap_cost(heap_size: u32, heap_cost: u64) -> u64`
```
cost = (heap_size / 32KB - 1) * heap_cost
```
- 32KB baseline ücretsiz, her ek 32KB sayfa ücretli

**Satır 108-140:** `pub fn create_vm<'a, 'b>(...) -> Result<EbpfVm<...>>`
- Stack: `AlignedMemory::zero_filled(config.stack_size())`
- Heap: `AlignedMemory::zero_filled(compute_budget.heap_size)`
- MemoryRegions: [ro_region, stack_region, heap_region]
- `MemoryMapping::new(regions, config, sbpf_version)`
- `EbpfVm::new(...)`

**Satır 142-190:** `fn execute<'a, 'b: 'a>(invoke_context, executable)`
- Lifetime trick: `'static` ile dropped lifetime yeniden ekleniyor (`unsafe transmute`)
- JIT: x86_64 && !windows → `executable.get_compiled_program().is_some()`
- `vm.execute_program(executable, !use_jit)` → interpreter veya JIT

### Instruction Handler'lar

**Satır 192-224:** `fn check_program_account(log_collector, instruction_context, program, authority_address) -> Result<LoaderV4State, InstructionError>`
- Owner kontrolü, boş data kontrolü, writable kontrolü, signer kontrolü, authority eşleşme kontrolü, Finalized kontrolü

**Satır 226-261:** `pub fn process_instruction_write(invoke_context, offset: u32, bytes: Vec<u8>)`
- Sadece `Retracted` state'de yazılabilir
- `program.data_mut()[program_data_offset + offset..end_offset].copy_from_slice(&bytes)`

**Satır 263-347:** `pub fn process_instruction_truncate(invoke_context, new_size: u32)`
- Initialization (new_size > 0 && data < header_size): program + authority imzası
- Truncation: `Retracted` state gerekli
- Lamport azaldıysa → recipient account'a gönder
- new_size == 0 → `program.set_data_length(0)` (account'ı sil)
- Initialization sırasında: `state.status = LoaderV4Status::Retracted; state.authority_address = *authority`

**Satır 349-457:** `pub fn process_instruction_deploy(invoke_context)`
- Cooldown kontrolü: `state.slot + DEPLOYMENT_COOLDOWN_IN_SLOTS > current_slot` → hata
- Source program (opsiyonel): buffer'dan deploy
  - Source da `Retracted` olmalı
  - Source data program account'a kopyalanır, source silinir
- ELF parse ve verify: `LoadedProgram::new(...)`
- `state.slot = current_slot; state.status = LoaderV4Status::Deployed`
- `programs_modified_by_tx.replenish(key, executor)` — program cache güncellenir
- Usage counter'lar eski entry'den kopyalanır

**Satır 459-491:** `pub fn process_instruction_retract(invoke_context)`
- Cooldown kontrolü (retract için de geçerli)
- `Retracted` → hata (zaten retracted)
- `state.status = LoaderV4Status::Retracted`

**Satır 493-528:** `pub fn process_instruction_transfer_authority(invoke_context)`
- Yeni authority varsa: authority değişir (new_authority da imzalamalı)
- Yeni authority yoksa:
  - `Deployed` ise → `Finalized` (kalıcı immutable)
  - `Retracted` ise → hata

**Satır 530-543:** `declare_builtin_function!(Entrypoint, ...)`
- `process_instruction_inner(invoke_context)` çağrısı

**Satır 545-son:** `pub fn process_instruction_inner(invoke_context)`
- `loader_v4::check_id(program_id)` ise: loader olarak çağrılıyor → instruction dispatch
- Değilse: user program olarak çağrılıyor:
  - `Retracted` → hata
  - Program cache'den `loaded_program` bul
  - `ix_usage_counter.fetch_add(1)`
  - `execute(invoke_context, executable)`

**Program Yaşam Döngüsü:**
```
[Account oluşturulur]
    ↓ Truncate (new_size > 0)
Retracted (initialized)
    ↓ Write (ELF bytecode yükle)
Retracted (dolu)
    ↓ Deploy (ELF doğrula + cache)
Deployed (çalışabilir)
    ↓ Retract (değiştir)
Retracted
    ↓ Deploy (tekrar)
Deployed
    ↓ TransferAuthority (new_authority = None)
Finalized (değiştirilemez)
```

---

## 6. Nova Shield Chain İçin Dersler

### 6.1 Stake/Delegation Tasarımı

**Ders 1: Warmup/Cooldown Oranı**
- Solana %25/epoch oranını feature flag ile kaldırmış (instantaneous)
- Nova Shield için: başlangıçta %33/epoch (3 epoch warmup), sonra governance ile değiştirilebilir
- `StakeHistory` sysvar pattern — her epoch'un toplam aktif/activating/deactivating stake'ini sakla

**Ders 2: Reward Hesaplama — Points Sistemi**
- `points = stake_amount * earned_credits` (u128 — overflow yok)
- `reward = (stake_points * total_epoch_rewards) / total_epoch_points`
- Bu formül proportional distribution sağlar
- Nova Shield'da: `validator_points = effective_stake * blocks_produced_this_epoch`

**Ders 3: Credits Observed Mekanizması**
- Her stake account `credits_observed` tutar
- Validator yeni kredi kazandıkça fark hesaplanır
- Delinquent validator durumu: `credits_in_vote == credits_in_stake` → puan yok ama credits ilerlemez
- Vote account recreate edilirse: `credits < credits_observed` → force rewind

**Ders 4: Merge için Weighted Average Credits**
```
merged_credits = (a1.credits * a1.stake + a2.credits * a2.stake) / (a1.stake + a2.stake)
```
Bu formül merge sonrasında ödüllerin değişmemesini garanti eder.

**Ders 5: Split Güvenliği**
- Magic activation loophole: önceden funded split account'a gereksiz stake aktifleşmesini engelle
- `FullyActive` merge'de source'un `rent_exempt_reserve`'i stake'e dahil edilmez

**Ders 6: Redelegate — MUST_FULLY_ACTIVATE Flag**
- Redelegate sonrası yeni stake'in tamamen aktifleşmeden önce deactivate edilmesi yasak
- Bu, warmup/cooldown sistemini bypass etmeyi engeller

### 6.2 Config Program Tasarımı

**Ders 7: Multi-Signer Config Pattern**
- On-chain config: `ConfigKeys { keys: Vec<(Pubkey, bool)> }` + data
- Tüm mevcut signerlar yeni config için onay vermeli (M-of-M)
- Duplicate signer tespiti (BTreeSet)
- Nova Shield için: chain parametrelerini (epoch uzunluğu, reward oranı) bu pattern ile yönet

**Ders 8: ConfigState Trait**
```rust
pub trait ConfigState: serde::Serialize + Default {
    fn max_space() -> u64;
}
```
Account boyutunu önceden bilmek için `max_space()` critical.

### 6.3 Address Lookup Table

**Ders 9: Transaction Sıkıştırma**
- ALT: 32-byte pubkey yerine 1-byte index
- Maks 256 adres per tablo
- `last_extended_slot_start_index`: o slot'taki eklemeyi bir öncekinden ayırt etmek için
- Nova Shield için: sık kullanılan program/system account pubkey'leri için LUT

**Ders 10: Deactivation Delay**
- Tablo deactivate edilince hemen kapatılamaz (slot_hashes düşene kadar bekle)
- In-flight transactionlar tablo kullanıyor olabilir
- Nova Shield: benzer "cooldown before close" mekanizması

**Ders 11: PDA Derivation ile Unique Table Adresi**
```
table_address = PDA([authority, recent_slot_bytes, bump], program_id)
```
Aynı address'te iki kez tablo oluşturulamaz (replay protection).

### 6.4 Loader-v4

**Ders 12: Deployment Cooldown**
- `DEPLOYMENT_COOLDOWN_IN_SLOTS = 750` slot (yaklaşık 5 dakika)
- Deploy'dan retract'a kadar bu süre geçmeli
- Nova Shield için: kritik sistem program güncellemeleri için minimum bekleme süresi

**Ders 13: Retract/Deploy Lifecycle**
- Deployed program değiştirilemez → önce retract, sonra write, sonra deploy
- Bu, "hot upgrade" sırasında yarım kalan write'ların deploy edilmesini engeller
- Nova Shield'da: program hash'i + deployment slot'u kaydet

**Ders 14: Zero-Copy State Erişimi (unsafe)**
```rust
std::mem::transmute::<&[u8; SIZE], &LoaderV4State>(data)
```
- Küçük, fixed-size state için `bincode` yerine raw cast
- Nova Shield'da: consensus-critical hot path state için kullanılabilir (ör. validator set)

**Ders 15: Program Cache (LoadedProgram)**
- Program deploy edilince in-memory cache'e alınır
- TX tarafından modified → `programs_modified_by_tx`
- Usage counter (tx + ix) → program ısınma metrikleri için
- Nova Shield: WASM/eBPF runtime için benzer program cache sistemi

**Ders 16: Finalized Status**
- Authority transfer + new_authority = None → Finalized
- Finalized program değiştirilemez, authority'si yoktur
- Nova Shield: genesis programs için kullanılabilir (system program vb.)

### 6.5 Genel Mimari Dersler

**Ders 17: Feature Flag Sistemi**
- Her davranış değişikliği bir feature flag arkasında
- `FeatureSet::is_active(&feature_id)` — consensus-safe upgrade
- Nova Shield: governance yönetilen feature set

**Ders 18: Compute Unit (CU) Maliyetleri**
```
compute-budget: 150 CU
config: 450 CU
stake: 750 CU
address-lookup-table: 750 CU
loader-v4: 2000 CU
```
Karmaşıklık arttıkça maliyet artar. Nova Shield'da: gas model.

**Ders 19: Sysvar Cache Pattern**
- `invoke_context.get_sysvar_cache().get_clock()` → `Arc<Clock>`
- Cache miss olursa sysvar account'tan okur
- Nova Shield: epoch, timestamp, validator set gibi sık kullanılan state'ler için cache

**Ders 20: checked_add / saturating_sub Kullanımı**
- Her aritmetik işlemde overflow kontrolü
- `checked_add` → None/Err durumunda `ArithmeticOverflow`
- `saturating_sub` → 0'a doyar, panic olmaz
- Nova Shield: tüm token/lamport işlemlerinde zorunlu

---

## 7. Özet Tablo

| Program | Dosyalar | Satır | Compute Units | Ana Özellik |
|---------|----------|-------|---------------|-------------|
| stake | 6 | ~2600+ | 750 | Delegasyon, warmup/cooldown, ödül dağıtımı |
| config | 4 | ~850 | 450 | M-of-M multi-signer on-chain config |
| address-lookup-table | 2 | ~470 | 750 | Transaction boyutu azaltma (32B→1B) |
| compute-budget | 1 | 8 | 150 | Runtime-level fee parametreleri |
| loader-v4 | 1 | ~620 | 2000 | SBF v2 program deployment + execution |

---

## 8. Kritik Kod Referansları

- **Points formülü:** `programs/stake/src/points.rs:193` — `let earned_points = stake_amount * earned_credits;`
- **Reward formülü:** `programs/stake/src/rewards.rs:185-189` — `rewards = (points * point_value.rewards) / point_value.points`
- **Unfair split koruması:** `programs/stake/src/rewards.rs:210-218`
- **Weighted credits merge:** `programs/stake/src/stake_state.rs:1188-1209`
- **Magic activation loophole fix:** `programs/stake/src/stake_state.rs:1133-1143`
- **ALT PDA derivation:** `programs/address-lookup-table/src/processor.rs:111-117`
- **Deployment cooldown:** `programs/loader-v4/src/lib.rs:373-379`
- **Zero-copy state:** `programs/loader-v4/src/lib.rs:41-53`
