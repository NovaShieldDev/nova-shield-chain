# Solana Kaynak Kod Analizi — Agent 06

## Analiz Kapsamı

| Dosya/Dizin | Satır | Durum |
|---|---|---|
| sdk/program/src/nonce/mod.rs | 7 | Tamamlandı |
| sdk/program/src/nonce/state/mod.rs | 255 | Tamamlandı |
| sdk/program/src/nonce/state/current.rs | 109 | Tamamlandı |
| sdk/program/src/rent.rs | 228 | Tamamlandı |
| sdk/program/src/sysvar/mod.rs | 300 | Tamamlandı |
| sdk/program/src/sysvar/clock.rs | 137 | Tamamlandı |
| sdk/program/src/sysvar/epoch_rewards.rs | 140 | Tamamlandı |
| sdk/program/src/sysvar/epoch_schedule.rs | 136 | Tamamlandı |
| sdk/program/src/sysvar/fees.rs | 71 | Tamamlandı |
| sdk/program/src/sysvar/instructions.rs | 573 | Tamamlandı |
| sdk/program/src/sysvar/last_restart_slot.rs | 53 | Tamamlandı |
| sdk/program/src/sysvar/recent_blockhashes.rs | 184 | Tamamlandı |
| sdk/program/src/sysvar/rent.rs | 137 | Tamamlandı |
| sdk/program/src/sysvar/rewards.rs | 23 | Tamamlandı |
| sdk/program/src/sysvar/slot_hashes.rs | 85 | Tamamlandı |
| sdk/program/src/sysvar/slot_history.rs | 80 | Tamamlandı |
| sdk/program/src/sysvar/stake_history.rs | 108 | Tamamlandı |
| sdk/program/src/stable_layout/stable_vec.rs | 182 | Tamamlandı |
| sdk/program/src/stable_layout/stable_instruction.rs | 97 | Tamamlandı |
| sdk/program/src/stable_layout/stable_rc.rs | 30 | Tamamlandı |
| sdk/program/src/stable_layout/stable_ref_cell.rs | 26 | Tamamlandı |
| sdk/program/src/stable_layout/stable_slice.rs | 28 | Tamamlandı |
| sdk/program/src/secp256k1_recover.rs | 438 | Tamamlandı |
| sdk/program/src/short_vec.rs | 388 | Tamamlandı |
| sdk/program/src/poseidon.rs | 453 | Tamamlandı |

**Toplam: ~3.753 satır**

---

## 1. Durable Nonce Mekanizması

### 1.1 nonce/mod.rs (satır 1-7)

```
pub const NONCED_TX_MARKER_IX_INDEX: u8 = 0;
```

**Tasarım kararı:** Durable nonce kullanan her transaction, ilk instruction'ı (index 0) mutlaka nonce advance instruction olarak işaretlemek **zorundadır**. Bu kural runtime tarafından zorlanır. Index 0 yoksa transaction invalid sayılır.

Bu sabit tek başına kritik bir güvenlik garantisi sağlar: Nonce-based transaction'lar her zaman nonce'ı ilerletir; dolayısıyla aynı transaction iki kez kullanılamaz (replay attack önleme).

---

### 1.2 nonce/state/current.rs (satır 1-109)

#### Sabitler

```rust
// satır 10
const DURABLE_NONCE_HASH_PREFIX: &[u8] = "DURABLE_NONCE".as_bytes();
```

**Kritik tasarım:** DurableNonce, blockhash ile **aynı domain'i paylaşmaz**. Bu prefix, blockhash'lerle çarpışmayı (collision) önlemek için bilinçli olarak eklenmiştir. `DURABLE_NONCE` prefix'i + blockhash'in hash'i alınarak yeni bir değer elde edilir. Bu "domain separation" (alan ayrımı) prensibidir.

#### Struct'lar

```rust
// satır 12-13
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct DurableNonce(Hash);
```

- `Hash` üzerinde newtype wrapper. Hash ile aynı boyutta (32 byte) ama semantik olarak farklı.
- `Copy` türetilebilir çünkü `Hash` `Copy`'dir.
- `Default` ile sıfır hash döner (uninitializated state).

```rust
// satır 18-26
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Data {
    pub authority: Pubkey,        // Hesabı kontrol eden otorite
    pub durable_nonce: DurableNonce,  // Gerçek nonce değeri
    pub fee_calculator: FeeCalculator, // Bu nonce ile ücret hesaplama
}
```

**Önemli:** `Data` struct'ı hem nonce değerini hem de fee bilgisini taşır. Bu sayede nonce hesabı okunduğunda her iki bilgi de tek seferinde elde edilir.

#### Fonksiyonlar

```rust
// satır 30-40
pub fn new(authority: Pubkey, durable_nonce: DurableNonce, lamports_per_signature: u64) -> Self {
    Data {
        authority,
        durable_nonce,
        fee_calculator: FeeCalculator::new(lamports_per_signature),
    }
}
```

```rust
// satır 45-47
pub fn blockhash(&self) -> Hash {
    self.durable_nonce.0
}
```

`blockhash()` metodu adında "legacy" sözcüğü var. DurableNonce, transaction'ın `recent_blockhash` alanına yerleştirilir ama gerçekte bir blockhash değildir; bu nedenle metot adı yanıltıcı görünebilir. Yorum satırı bunu açıklamaktadır.

```rust
// satır 56-63
impl DurableNonce {
    pub fn from_blockhash(blockhash: &Hash) -> Self {
        Self(hashv(&[DURABLE_NONCE_HASH_PREFIX, blockhash.as_ref()]))
    }
    pub fn as_hash(&self) -> &Hash {
        &self.0
    }
}
```

**Hash zinciri:** `from_blockhash` = `hash("DURABLE_NONCE" || blockhash)`. Bu fonksiyon deterministiktir; aynı blockhash her zaman aynı DurableNonce değerini üretir.

#### State enum

```rust
// satır 70-75
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum State {
    #[default]
    Uninitialized,
    Initialized(Data),
}
```

İki durumlu state machine. `#[default]` Rust 1.62'de gelen özellik; `State::default()` çağrıldığında `Uninitialized` döner.

```rust
// satır 88-90
pub const fn size() -> usize {
    80 // see test_nonce_state_size.
}
```

Nonce hesabının veri alanı tam 80 byte'tır. Bu test ile doğrulanmıştır: `bincode::serialized_size(&Versions::new(State::Initialized(Data::default()))) == 80`.

#### Testler (satır 93-108)

- `default_is_uninitialized`: `State::default()` == `State::Uninitialized` doğrulanıyor.
- `test_nonce_state_size`: 80 byte serialization boyutu runtime'ın hardcode ettiği değerle tutarlı.

---

### 1.3 nonce/state/mod.rs (satır 1-255)

#### Enum: Versions

```rust
// satır 11-16
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum Versions {
    Legacy(Box<State>),
    Current(Box<State>),
}
```

`Box<State>` kullanımı dikkat çekicidir. `State` enum'ı içinde `Data` barındırır ve `Data` 80 byte'lık bir veri yapısıdır. Stack'de taşımak yerine heap'e taşıyarak enum'ın kendisini küçültmek hedeflenmiştir.

**Legacy vs Current farkı:** Legacy nonce'lar, blockhash domain ile aynı alanda çalışırdı. Bu bir güvenlik açığı yaratıyordu: DurableNonce değeri, gerçek bir blockhash ile çakışabilirdi. `Current` variant'ı bunu "DURABLE_NONCE" prefix'i ile önler.

#### Enum: AuthorizeNonceError

```rust
// satır 18-22
#[derive(Debug, Eq, PartialEq)]
pub enum AuthorizeNonceError {
    MissingRequiredSignature(Pubkey),
    Uninitialized,
}
```

`MissingRequiredSignature` içinde `Pubkey` taşır — hangi public key'in imzası eksik olduğunu söyler. Debug mesajları için çok pratik.

#### Impl: Versions

```rust
// satır 25-27
pub fn new(state: State) -> Self {
    Self::Current(Box::new(state))
}
```

Her yeni nonce hesabı artık `Current` variant ile oluşturulur.

```rust
// satır 36-53
pub fn verify_recent_blockhash(&self, recent_blockhash: &Hash) -> Option<&Data> {
    match self {
        Self::Legacy(_) => None,   // Legacy nonce'lar artık geçersiz
        Self::Current(state) => match **state {
            State::Uninitialized => None,
            State::Initialized(ref data) => {
                (recent_blockhash == &data.blockhash()).then_some(data)
            }
        },
    }
}
```

**Kritik güvenlik kararı:** Legacy nonce'lar, `verify_recent_blockhash` üzerinden **hiçbir zaman** geçerli sayılmaz (her zaman `None` döner). Bu backward-incompatible bir değişikliktir; eski nonce hesapları ya upgrade edilmeli ya da terk edilmelidir.

```rust
// satır 56-73
pub fn upgrade(self) -> Option<Self> {
    match self {
        Self::Legacy(mut state) => {
            match *state {
                State::Uninitialized => None,
                State::Initialized(ref mut data) => {
                    data.durable_nonce = DurableNonce::from_blockhash(&data.blockhash());
                    Some(Self::Current(state))
                }
            }
        }
        Self::Current(_) => None,
    }
}
```

Upgrade işlemi: Mevcut `blockhash()` değerini alır, üzerine prefix hash uygular ve yeni `DurableNonce` olarak kaydeder. Böylece eski Legacy nonce, çakışma riski olmayan Current nonce'a dönüştürülür.

```rust
// satır 76-102
pub fn authorize(self, signers: &HashSet<Pubkey>, authority: Pubkey) -> Result<Self, AuthorizeNonceError> {
    let data = match self.state() {
        State::Uninitialized => return Err(AuthorizeNonceError::Uninitialized),
        State::Initialized(data) => data,
    };
    if !signers.contains(&data.authority) {
        return Err(AuthorizeNonceError::MissingRequiredSignature(data.authority));
    }
    let data = Data::new(authority, data.durable_nonce, data.get_lamports_per_signature());
    let state = Box::new(State::Initialized(data));
    Ok(match self {
        Self::Legacy(_) => Self::Legacy,
        Self::Current(_) => Self::Current,
    }(state))
}
```

**Önemli tasarım:** Authorize işlemi, `durable_nonce` değerini **korur** (değiştirmez). Sadece `authority` değiştirilir. Version variant'ı da korunur — Legacy nonce upgrade edilmeden re-authorize edilirse Legacy olarak kalır.

Son satırdaki `}(state)` gösterimi: Enum variant'ı fonksiyon gibi çağırma. `Self::Legacy` veya `Self::Current`, `Box<State>` alan bir fonksiyon gibi kullanılmış.

#### Testler (satır 115-254)

- `test_verify_recent_blockhash` (satır 122-164): Legacy nonce her zaman `None` döndürür. Current nonce yalnızca doğru hash ile eşleştiğinde `Some(&data)` döndürür. DurableNonce, blockhash'in kendisiyle değil; onun hash'i ile eşleşir.
- `test_nonce_versions_upgrade` (satır 166-194): Legacy → Current dönüşümü. Dönüştürülen nonce'ın `durable_nonce` değerinin eski değerden **farklı** olduğu doğrulanır.
- `test_nonce_versions_authorize` (satır 197-253): Uninitialized nonce'a authorize → `Err(Uninitialized)`. Yanlış imza → `Err(MissingRequiredSignature)`. Doğru imza → yeni authority ile güncellenmiş nonce.

---

## 2. Rent (Kira) Mekanizması

### 2.1 rent.rs (satır 1-228)

#### Sabitler

```rust
// satır 34
pub const DEFAULT_LAMPORTS_PER_BYTE_YEAR: u64 = 1_000_000_000 / 100 * 365 / (1024 * 1024);
```

**Hesaplama mantığı:**
- 10^9 lamport = 1 SOL
- Varsayım: 1 SOL = $1
- Depolama maliyeti: $0.01 per megabyte-day = $3.65 per megabyte-year
- 1 megabyte = 1024 * 1024 byte
- `1_000_000_000 / 100` = 10_000_000 lamport (1 SOL'un $3.65 oranında $1/100'lük kısmı değil, formül tam olarak: 1 SOL * 365 gün / (100 megabyte * 1024 * 1024 byte/MB))

Sayısal değer: `1_000_000_000 / 100 * 365 / (1024 * 1024)` = `10_000_000 * 365 / 1_048_576` ≈ 3.484 lamport/byte-year

```rust
// satır 38
pub const DEFAULT_EXEMPTION_THRESHOLD: f64 = 2.0;
```

Rent muafiyeti için hesap, **2 yıllık** rent'i peşin ödemek zorundadır.

```rust
// satır 44
pub const DEFAULT_BURN_PERCENT: u8 = 50;
```

Toplanan rent'in %50'si yakılır, %50'si validator'lara dağıtılır.

```rust
// satır 50
pub const ACCOUNT_STORAGE_OVERHEAD: u64 = 128;
```

Her hesabın, veri olmasa bile 128 byte overhead'i vardır (metadata, pubkey, owner, lamports, flags vb.).

#### Struct: Rent

```rust
// satır 10-25
#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, CloneZeroed, Debug, AbiExample)]
pub struct Rent {
    pub lamports_per_byte_year: u64,
    pub exemption_threshold: f64,
    pub burn_percent: u8,
}
```

`#[repr(C)]` ile C-uyumlu bellek düzeni. `CloneZeroed` özel türev makrosu — `Clone`'u implement eder ama bellek bloğunu önce sıfırlar (padding byte'ları temizler). Bu, potansiyel bilgi sızıntısını önler.

#### Fonksiyonlar

```rust
// satır 63-77
pub fn minimum_balance(&self, data_len: usize) -> u64 {
    let bytes = data_len as u64;
    (((ACCOUNT_STORAGE_OVERHEAD + bytes) * self.lamports_per_byte_year) as f64
        * self.exemption_threshold) as u64
}
```

**Rent muafiyeti formülü:**
```
minimum_balance = (128 + data_len) * lamports_per_byte_year * 2.0
```

Default değerlerle: `(128 + data_len) * 3484 * 2.0` lamport

Örneğin 0 byte'lık bir hesap için: `128 * 3484 * 2 = 892_160 lamport ≈ 0.000893 SOL`

```rust
// satır 80-82
pub fn is_exempt(&self, balance: u64, data_len: usize) -> bool {
    balance >= self.minimum_balance(data_len)
}
```

```rust
// satır 85-91
pub fn due(&self, balance: u64, data_len: usize, years_elapsed: f64) -> RentDue {
    if self.is_exempt(balance, data_len) {
        RentDue::Exempt
    } else {
        RentDue::Paying(self.due_amount(data_len, years_elapsed))
    }
}
```

```rust
// satır 94-98
pub fn due_amount(&self, data_len: usize, years_elapsed: f64) -> u64 {
    let actual_data_len = data_len as u64 + ACCOUNT_STORAGE_OVERHEAD;
    let lamports_per_year = self.lamports_per_byte_year * actual_data_len;
    (lamports_per_year as f64 * years_elapsed) as u64
}
```

Muaf olmayan hesaplar için: `(128 + data_len) * lamports_per_byte_year * years_elapsed`

```rust
// satır 110-122
pub fn with_slots_per_epoch(slots_per_epoch: u64) -> Self {
    let ratio = slots_per_epoch as f64 / DEFAULT_SLOTS_PER_EPOCH as f64;
    let exemption_threshold = DEFAULT_EXEMPTION_THRESHOLD * ratio;
    let lamports_per_byte_year = (DEFAULT_LAMPORTS_PER_BYTE_YEAR as f64 / ratio) as u64;
    Self {
        lamports_per_byte_year,
        exemption_threshold,
        ..Self::default()
    }
}
```

Test yardımcı fonksiyonu. Epoch süresini değiştirerek farklı rent konfigürasyonları test etmek için kullanılır. `lamports_per_byte_year` ve `exemption_threshold` ters orantılı olarak ölçeklenir, böylece toplam maliyet sabit kalır.

#### Enum: RentDue

```rust
// satır 127-132
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RentDue {
    Exempt,
    Paying(u64),
}
```

İki durumlu enum. `Paying(0)` ile `Exempt` farklı anlamlara gelir — `Paying(0)` hesap muaf değildir ama şu an için ödeme yapması gerekmiyor; `Exempt` ise kesin olarak muaftır.

```rust
// satır 134-150
impl RentDue {
    pub fn lamports(&self) -> u64 {
        match self {
            RentDue::Exempt => 0,       // Muaf = 0 lamport ödeme
            RentDue::Paying(x) => *x,
        }
    }
    pub fn is_exempt(&self) -> bool {
        match self {
            RentDue::Exempt => true,
            RentDue::Paying(_) => false, // Paying(0) bile false döner!
        }
    }
}
```

**Test doğrulaması (satır 211-214):**
```rust
assert!(RentDue::Exempt.is_exempt());
assert!(!RentDue::Paying(0).is_exempt()); // Paying(0) muaf DEĞİL
```

---

## 3. Sysvar Sistemi

### 3.1 sysvar/mod.rs (satır 1-300)

#### Genel Mimari

Sysvar'lar, blockchain hakkında dinamik bilgi içeren özel hesaplardır. Hepsi `"Sysvar1111111111111111111111111111111111111"` ID'li özel bir program tarafından sahiplenilir.

İki erişim yöntemi mevcuttur:
1. `Sysvar::get()` — Runtime'dan doğrudan yükler, hesap ID'si gerekli değil, daha verimli.
2. `Sysvar::from_account_info()` — AccountInfo'dan deserialize eder, legacy yöntem.

```rust
// satır 102-118
lazy_static! {
    pub static ref ALL_IDS: Vec<Pubkey> = vec![
        clock::id(),
        epoch_schedule::id(),
        fees::id(),           // deprecated
        recent_blockhashes::id(), // deprecated
        rent::id(),
        rewards::id(),
        slot_hashes::id(),
        slot_history::id(),
        stake_history::id(),
        instructions::id(),
    ];
}
```

`lazy_static!` ile singleton başlatma. `epoch_rewards` ve `last_restart_slot` bu listede YOK — muhtemelen sonradan eklendikleri için buraya eklenmemiş (deprecated list sabit kalacak notu var).

#### Macro: declare_sysvar_id

```rust
// satır 126-141
macro_rules! declare_sysvar_id(
    ($name:expr, $type:ty) => (
        crate::declare_id!($name);

        impl crate::sysvar::SysvarId for $type {
            fn id() -> crate::pubkey::Pubkey { id() }
            fn check_id(pubkey: &crate::pubkey::Pubkey) -> bool { check_id(pubkey) }
        }
    )
);
```

Bu makro iki şey yapar: `declare_id!` ile statik bir pubkey tanımlar + `SysvarId` trait'ini implement eder. Tekrar yazmayı önler.

#### Trait: SysvarId

```rust
// satır 166-173
pub trait SysvarId {
    fn id() -> Pubkey;
    fn check_id(pubkey: &Pubkey) -> bool;
}
```

#### Trait: Sysvar

```rust
// satır 176-217
pub trait Sysvar:
    SysvarId + Default + Sized + serde::Serialize + serde::de::DeserializeOwned
{
    fn size_of() -> usize {
        bincode::serialized_size(&Self::default()).unwrap() as usize
    }

    fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        if !Self::check_id(account_info.unsigned_key()) {
            return Err(ProgramError::InvalidArgument);
        }
        bincode::deserialize(&account_info.data.borrow())
            .map_err(|_| ProgramError::InvalidArgument)
    }

    fn to_account_info(&self, account_info: &mut AccountInfo) -> Option<()> {
        bincode::serialize_into(&mut account_info.data.borrow_mut()[..], self).ok()
    }

    fn get() -> Result<Self, ProgramError> {
        Err(ProgramError::UnsupportedSysvar)  // Default: desteklenmiyor
    }
}
```

`get()` varsayılan implementasyonu her zaman hata döndürür. Alt tipler override etmezse sysvar on-chain programlarda `Sysvar::get()` ile erişilemez.

#### Macro: impl_sysvar_get

```rust
// satır 221-239
macro_rules! impl_sysvar_get {
    ($syscall_name:ident) => {
        fn get() -> Result<Self, ProgramError> {
            let mut var = Self::default();
            let var_addr = &mut var as *mut _ as *mut u8;

            #[cfg(target_os = "solana")]
            let result = unsafe { crate::syscalls::$syscall_name(var_addr) };

            #[cfg(not(target_os = "solana"))]
            let result = crate::program_stubs::$syscall_name(var_addr);

            match result {
                crate::entrypoint::SUCCESS => Ok(var),
                e => Err(e.into()),
            }
        }
    };
}
```

**unsafe blok (satır 228):** `syscalls::$syscall_name(var_addr)` — SBF programları için syscall. `var_addr` bir ham pointer'dır. Runtime bu pointer'ın işaret ettiği belleğe veriyi yazar. `#[cfg(target_os = "solana")]` ile yalnızca SBF ortamında çalışır; test ortamında `program_stubs` kullanılır.

---

### 3.2 Bireysel Sysvar'lar

#### sysvar/clock.rs

```rust
crate::declare_sysvar_id!("SysvarC1ock11111111111111111111111111111111", Clock);
impl Sysvar for Clock {
    impl_sysvar_get!(sol_get_clock_sysvar);
}
```

`Clock` tipi başka bir modülde tanımlanmıştır (`crate::clock::Clock`). Burada sadece sysvar ID'si ve erişim yöntemi implement edilir.

**Syscall:** `sol_get_clock_sysvar` — runtime'a slot, epoch, unix_timestamp bilgisi için doğrudan çağrı.

#### sysvar/rent.rs

```rust
crate::declare_sysvar_id!("SysvarRent111111111111111111111111111111111", Rent);
impl Sysvar for Rent {
    impl_sysvar_get!(sol_get_rent_sysvar);
}
```

`Rent::get()` ile program, hesap açma maliyetini hesaplamak için kullanabileceği `Rent` yapısını alır.

#### sysvar/epoch_schedule.rs

```rust
crate::declare_sysvar_id!("SysvarEpochSchedu1e111111111111111111111111", EpochSchedule);
impl Sysvar for EpochSchedule {
    impl_sysvar_get!(sol_get_epoch_schedule_sysvar);
}
```

Dikkat: `"SysvarEpochSchedu1e..."` → `"l"` yerine `"1"` (rakam bir) kullanılmış. Bu kasıtlı — base58 encoding'de 32 byte sığması için.

#### sysvar/epoch_rewards.rs

```rust
crate::declare_sysvar_id!("SysvarEpochRewards1111111111111111111111111", EpochRewards);
impl Sysvar for EpochRewards {
    impl_sysvar_get!(sol_get_epoch_rewards_sysvar);
}
```

Epoch ödüllerinin dağıtımını takip eder. Dağıtım tamamlanana kadar aktif olan geçici bir sysvar.

#### sysvar/last_restart_slot.rs

```rust
crate::declare_sysvar_id!("SysvarLastRestartS1ot1111111111111111111111", LastRestartSlot);
impl Sysvar for LastRestartSlot {
    impl_sysvar_get!(sol_get_last_restart_slot);
}
```

Hard fork sonrası en son yeniden başlatma slotu. Fork yoksa 0 döner.

#### sysvar/fees.rs (Deprecated)

```rust
// satır 33-41
#[deprecated(since = "1.9.0", note = "...")]
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, CloneZeroed, Default, PartialEq, Eq)]
pub struct Fees {
    pub fee_calculator: FeeCalculator,
}
impl Sysvar for Fees {
    impl_sysvar_get!(sol_get_fees_sysvar);
}
```

1.9.0'dan beri deprecated. `getFeeForMessage` RPC metodu kullanılmalı.

#### sysvar/recent_blockhashes.rs (Deprecated)

```rust
// satır 28-32
#[deprecated(since = "1.9.0")]
pub const MAX_ENTRIES: usize = 150;

// satır 43-48
#[deprecated]
pub struct Entry {
    pub blockhash: Hash,
    pub fee_calculator: FeeCalculator,
}

// satır 89-95
#[deprecated]
pub struct RecentBlockhashes(Vec<Entry>);
```

**Boyut:** 6008 byte hardcoded (MAX_ENTRIES=150 için).

**IntoIterSorted** (satır 124-147): Nightly-only `BinaryHeap::into_iter_sorted()` API'sının manual implementasyonu. Rust'ta `binary_heap_into_iter_sorted` feature stabilize edilene kadar burada tutulmuştur.

#### sysvar/slot_hashes.rs

```rust
impl Sysvar for SlotHashes {
    fn size_of() -> usize {
        20_488 // hard-coded
    }
    fn from_account_info(_account_info: &AccountInfo) -> Result<Self, ProgramError> {
        Err(ProgramError::UnsupportedSysvar) // On-chain KULLANILAMAZ
    }
}
```

**Tasarım kararı:** SlotHashes çok büyük (20,488 byte) — on-chain program compute budget'ını aşar. `from_account_info` override edilerek kasıtlı olarak hata döndürülür. Yalnızca off-chain RPC üzerinden erişilebilir.

#### sysvar/slot_history.rs

```rust
fn size_of() -> usize {
    131_097 // hard-coded
}
fn from_account_info(_account_info: &AccountInfo) -> Result<Self, ProgramError> {
    Err(ProgramError::UnsupportedSysvar)
}
```

131 KB'lık bitvektör. On-chain erişim imkansız. Son epoch'taki slot'ların işlenip işlenmediğini takip eder.

#### sysvar/stake_history.rs

```rust
fn size_of() -> usize {
    16392 // hard-coded
}
```

~16 KB. On-chain `get()` desteklenmez. Stake aktivasyon/deaktivasyon geçmişi.

#### sysvar/rewards.rs

```rust
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Rewards {
    pub validator_point_value: f64,
    pub unused: f64,  // Kullanılmayan alan!
}
impl Sysvar for Rewards {}  // get() override yok
```

Deprecated ve kullanılmıyor. `unused` alan bile kalmış. `Sysvar` trait'i implement edilmiş ama `get()` override yok; çağrıldığında `UnsupportedSysvar` döner.

---

### 3.3 sysvar/instructions.rs (satır 1-573)

Bu sysvar diğerlerinden farklıdır: Bir tip içermez, sadece free function'lardan oluşur.

#### Dummy Tip

```rust
// satır 56-57
pub struct Instructions();
crate::declare_sysvar_id!("Sysvar1nstructions1111111111111111111111111", Instructions);
```

`Instructions` tipi sadece ID için vardır. `Sysvar` trait'ini implement etmez.

#### Serializasyon Formatı

```
// satır 100-112 (yorum)
[0..2]   - num_instructions (u16)
[2..2+2*n] - offset table (her instruction için u16 offset)
Her instruction:
  [0..2]    - num_accounts (u16)
  [2]       - meta_byte: bit0=signer, bit1=writable
  [3..35]   - pubkey (32 bytes)
  [35..67]  - program_id (32 bytes)
  [67..69]  - data_len (u16)
  [69..]    - data
Son 2 byte: current_instruction_index (u16)
```

**Tasarım:** Offset tablosu ile O(1) erişim sağlanır — i'nci instruction'a gitmek için önce `offset_table[i]`'ye bakılır, sonra o offset'e atlanır. Baştan başa parse etmek gerekmez.

#### Kritik Fonksiyonlar

```rust
// satır 64-70 (only non-solana)
pub fn construct_instructions_data(instructions: &[BorrowedInstruction]) -> Vec<u8> {
    let mut data = serialize_instructions(instructions);
    data.resize(data.len() + 2, 0);  // Current index için yer
    data
}
```

```rust
// satır 154-159 (deprecated)
pub fn load_current_index(data: &[u8]) -> u16 {
    let mut instr_fixed_data = [0u8; 2];
    let len = data.len();
    instr_fixed_data.copy_from_slice(&data[len - 2..len]);
    u16::from_le_bytes(instr_fixed_data)
}
```

**Deprecated nedeni:** Sysvar hesabının doğru adres olup olmadığı kontrol edilmiyor. Kötü niyetli program, sahte bir hesap geçirebilir.

```rust
// satır 167-179
pub fn load_current_index_checked(instruction_sysvar_account_info: &AccountInfo) -> Result<u16, ProgramError> {
    if !check_id(instruction_sysvar_account_info.key) {
        return Err(ProgramError::UnsupportedSysvar);
    }
    // ...güvenli erişim
}
```

"Checked" versiyonlar: Önce `check_id()` ile hesap adresini doğrular.

```rust
// satır 270-294
pub fn get_instruction_relative(index_relative_to_current: i64, ...) -> Result<Instruction, ProgramError> {
    // ...
    let current_index = load_current_index(&instruction_sysvar) as i64;
    let index = current_index.saturating_add(index_relative_to_current);
    if index < 0 {
        return Err(ProgramError::InvalidArgument);
    }
    // ...
}
```

`saturating_add` ile taşma koruması. Negatif index → `InvalidArgument`.

**InstructionsSysvarAccountMeta bitflags:**

```rust
// satır 92-98
bitflags! {
    struct InstructionsSysvarAccountMeta: u8 {
        const IS_SIGNER   = 0b00000001;
        const IS_WRITABLE = 0b00000010;
    }
}
```

Her hesap için 1 byte metadata, 2 bit kullanılır.

---

## 4. Stable Layout Modülü

### 4.1 stable_layout/stable_vec.rs (satır 1-182)

#### Problem

Rust'ın `Vec<T>` tipi, spesifikasyon olarak bellek düzeninin garantisini vermez. Farklı Rust versiyonları farklı düzenler kullanabilir. Solana runtime, belleği doğrudan pointer ile eriştiği için bu bir sorun yaratır.

#### Çözüm: StableVec

```rust
// satır 25-31
#[repr(C)]
pub struct StableVec<T> {
    pub ptr: NonNull<T>,  // offset 0,  boyut 8
    pub cap: usize,       // offset 8,  boyut 8
    pub len: usize,       // offset 16, boyut 8
    _marker: PhantomData<T>,
}
```

`#[repr(C)]` ile C düzeni garantisi. Toplam boyut: 24 byte.

**Test doğrulaması (satır 155-161):**
```rust
assert_eq!(offset_of!(StableVec<i32>, ptr), 0);
assert_eq!(offset_of!(StableVec<i32>, cap), 8);
assert_eq!(offset_of!(StableVec<i32>, len), 16);
assert_eq!(size_of::<StableVec<i32>>(), 24);
```

#### Dönüşüm: Vec → StableVec

```rust
// satır 106-122
impl<T> From<Vec<T>> for StableVec<T> {
    fn from(other: Vec<T>) -> Self {
        let mut other = ManuallyDrop::new(other);  // Drop çalışmasın
        Self {
            ptr: unsafe { NonNull::new_unchecked(other.as_mut_ptr()) },
            cap: other.capacity(),
            len: other.len(),
            _marker: PhantomData,
        }
    }
}
```

`ManuallyDrop` kritiktir: `other` scope'tan çıkınca drop edilmesini önler — bellek `StableVec`'e transfer edilir.

#### unsafe blok (satır 116)
`NonNull::new_unchecked(other.as_mut_ptr())` — Vec'in pointer'ı her zaman non-null olduğu **bilindiği** için `unchecked` kullanılmış. Normal `NonNull::new()` `Option<NonNull<T>>` döndürür; burada unwrap overhead'i önlenmiştir.

#### Drop implementasyonu

```rust
// satır 135-143
impl<T> Drop for StableVec<T> {
    fn drop(&mut self) {
        // Vec'e geri dönüştürerek Vec'in drop mekanizmasını kullan
        let _vec = unsafe { Vec::from_raw_parts(self.ptr.as_ptr(), self.len, self.cap) };
    }
}
```

**unsafe blok:** `Vec::from_raw_parts` — raw pointer, len, cap ile Vec oluşturma. Buradaki invariant: `StableVec` yalnızca geçerli bir `Vec`'ten oluşturulabilir, dolayısıyla geri dönüşüm güvenlidir.

#### Deref implementasyonları

```rust
// satır 61-68 (Deref)
impl<T> std::ops::Deref for StableVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(self.as_ptr(), self.len) }
    }
}

// satır 70-74 (DerefMut)
impl<T> std::ops::DerefMut for StableVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { core::slice::from_raw_parts_mut(self.as_mut_ptr(), self.len) }
    }
}
```

**unsafe bloklar:** `slice::from_raw_parts` — pointer ve len'in geçerli olduğu garanti edilmeli. Bu garanti `StableVec`'in yalnızca geçerli `Vec`'ten oluşturulabilmesinden gelir.

PartialEq makrosu (satır 83-104): `StableVec<T>` ile `Vec<U>`, `[U]`, `[U; N]` vb. arasında karşılaştırmaya izin verir. Slice deref üzerinden çalışır.

---

### 4.2 stable_layout/stable_instruction.rs (satır 1-97)

```rust
// satır 31-37
#[derive(Debug, PartialEq)]
#[repr(C)]
pub struct StableInstruction {
    pub accounts: StableVec<AccountMeta>,  // offset 0,  boyut 24
    pub data: StableVec<u8>,               // offset 24, boyut 24
    pub program_id: Pubkey,                // offset 48, boyut 32
}
```

Toplam boyut: 80 byte. Test doğrulaması (satır 58-63):
```rust
assert_eq!(offset_of!(StableInstruction, accounts), 0);
assert_eq!(offset_of!(StableInstruction, data), 24);
assert_eq!(offset_of!(StableInstruction, program_id), 48);
assert_eq!(size_of::<StableInstruction>(), 80);
```

`Instruction` → `StableInstruction` dönüşümü (satır 39-47): `accounts` ve `data` field'ları `Vec<T>` → `StableVec<T>` dönüşümü ile alınır.

**Dikkat:** `StableInstruction`'daki field sırası `Instruction`'dan farklıdır. `Instruction`'da sıra `program_id, accounts, data`; `StableInstruction`'da `accounts, data, program_id`. Runtime bu spesifik sırayı bekler.

#### unsafe bloklar — test içinde (satır 82-94)

```rust
let accounts_ptr = instruction_addr as *const StableVec<AccountMeta>;
assert_eq!(unsafe { &*accounts_ptr }, &accounts);

let data_ptr = (instruction_addr + 24) as *const StableVec<u8>;
assert_eq!(unsafe { &*data_ptr }, &data);

let pubkey_ptr = (instruction_addr + 48) as *const Pubkey;
assert_eq!(unsafe { *pubkey_ptr }, program_id);
```

Testler, raw pointer ile bellek offset'lerini doğrular. Bu yaklaşım runtime'ın yapacağı şeyi simüle eder.

---

### 4.3 stable_layout/stable_rc.rs (satır 1-30)

Sadece test modülü içerir. `Rc<i32>` bellek düzenini doğrular:

```rust
assert_eq!(align_of::<Rc<i32>>(), 8);
assert_eq!(size_of::<Rc<i32>>(), 8);  // Sadece pointer boyutu
```

`Rc<i32>` içindeki `RcBox`'ın yapısı (test içinde raw pointer ile erişilir):
- `addr_rcbox + 0`: strong_count (usize, 8 byte)
- `addr_rcbox + 8`: weak_count (usize, 8 byte)
- `addr_rcbox + 16`: value (T)

**unsafe bloklar (satır 21-27):** Raw pointer ile `Rc`'nin iç yapısına erişim. Bu testler, Rust'ın `Rc` implementasyonu değişirse erken uyarı verecek şekilde tasarlanmıştır.

---

### 4.4 stable_layout/stable_ref_cell.rs (satır 1-26)

`RefCell<i32>` bellek düzeni:

```rust
assert_eq!(size_of::<RefCell<i32>>(), 8 + 4 + 4);  // borrow_count + value + padding
```

- `offset 0`: borrow count (isize, 8 byte)
- `offset 8`: value (i32, 4 byte)
- 4 byte padding

**unsafe blok (satır 22-23):** Raw pointer ile RefCell'in borrow count'una erişim.

---

### 4.5 stable_layout/stable_slice.rs (satır 1-28)

`&[i32]` (fat pointer) bellek düzeni:

```rust
assert_eq!(size_of::<&[i32]>(), 8 + 8);  // ptr + len
```

- `offset 0`: data pointer (8 byte)
- `offset 8`: length (8 byte)

---

## 5. secp256k1_recover.rs (satır 1-438)

### Hata Enum'ı

```rust
// satır 33-41
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Secp256k1RecoverError {
    #[error("The hash provided to a secp256k1_recover is invalid")]
    InvalidHash,
    #[error("The recovery_id provided to a secp256k1_recover is invalid")]
    InvalidRecoveryId,
    #[error("The signature provided to a secp256k1_recover is invalid")]
    InvalidSignature,
}
```

u64 ↔ enum dönüşümü (satır 43-62): Syscall hatayı u64 olarak döndürür; Rust tarafında enum'a çevirilir. 1=InvalidHash, 2=InvalidRecoveryId, 3=InvalidSignature.

### Secp256k1Pubkey Tipi

```rust
// satır 67-82
#[repr(transparent)]
#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Clone, Copy, ...)]
pub struct Secp256k1Pubkey(pub [u8; SECP256K1_PUBLIC_KEY_LENGTH]);  // 64 byte
```

`#[repr(transparent)]` — sadece `[u8; 64]`'ün newtype wrapper'ı. C-uyumlu, zero-cost abstraction.

`SECP256K1_PUBLIC_KEY_LENGTH = 64` — uncompressed public key formatının prefix byte'ı (0x04) çıkarılmış hali. Tam uncompressed format 65 byte'tır.

### secp256k1_recover fonksiyonu

```rust
// satır 402-437
pub fn secp256k1_recover(
    hash: &[u8],          // 32 byte keccak/sha256 hash
    recovery_id: u8,      // [0, 3] aralığında
    signature: &[u8],     // 64 byte (r || s)
) -> Result<Secp256k1Pubkey, Secp256k1RecoverError>
```

#### On-chain davranış (SBF, satır 407-423)

```rust
#[cfg(target_os = "solana")]
{
    let mut pubkey_buffer = [0u8; SECP256K1_PUBLIC_KEY_LENGTH];
    let result = unsafe {
        crate::syscalls::sol_secp256k1_recover(
            hash.as_ptr(),
            recovery_id as u64,
            signature.as_ptr(),
            pubkey_buffer.as_mut_ptr(),
        )
    };
    match result {
        0 => Ok(Secp256k1Pubkey::new(&pubkey_buffer)),
        error => Err(Secp256k1RecoverError::from(error)),
    }
}
```

**unsafe blok:** `sol_secp256k1_recover` syscall. 4 argüman: hash ptr, recovery_id, signature ptr, output pubkey ptr. Runtime bu bellek bölgelerini doğrudan okur/yazar.

**Kritik güvenlik notu (satır 233-243):** On-chain'de `hash` ve `signature` slice uzunlukları runtime tarafından doğrulanmaz. Kısa sliceler yanlış bellek okunmasına neden olabilir. Programcı uzunlukları önce kontrol etmelidir.

#### Off-chain davranış (test/native, satır 425-436)

```rust
#[cfg(not(target_os = "solana"))]
{
    let message = libsecp256k1::Message::parse_slice(hash)
        .map_err(|_| Secp256k1RecoverError::InvalidHash)?;
    let recovery_id = libsecp256k1::RecoveryId::parse(recovery_id)
        .map_err(|_| Secp256k1RecoverError::InvalidRecoveryId)?;
    let signature = libsecp256k1::Signature::parse_standard_slice(signature)
        .map_err(|_| Secp256k1RecoverError::InvalidSignature)?;
    let secp256k1_key = libsecp256k1::recover(&message, &signature, &recovery_id)
        .map_err(|_| Secp256k1RecoverError::InvalidSignature)?;
    Ok(Secp256k1Pubkey::new(&secp256k1_key.serialize()[1..65]))
}
```

`serialize()[1..65]` — `libsecp256k1`'in `serialize()` metodu 65 byte döndürür (0x04 prefix + 64 byte key). Prefix atlanır.

### Signature Malleability Uyarısı

Belgede ayrıntılı açıklanmış: Solana'nın `secp256k1_recover`'ı malleability'e karşı **koruma sağlamaz**. Yüksek-order `S` değerli imzalar da kabul edilir. Güvenlik gerektiren uygulamalar için:

```rust
let signature = libsecp256k1::Signature::parse_standard_slice(&signature_bytes)?;
if signature.s.is_high() {
    return Err(ProgramError::InvalidArgument);
}
```

**Ethereum recovery_id farkı (satır 115-119):** Ethereum'da recovery_id 27 veya 28 olur. Bu API [0, 3] aralığı bekler. Dönüşüm: `eth_recovery_id - 27`.

---

## 6. short_vec.rs (satır 1-388)

### ShortU16: Variable-Length Integer Encoding

```rust
// satır 18-19
#[derive(AbiExample)]
pub struct ShortU16(pub u16);
```

#### Encoding Formatı

```
1 byte  : 0xxxxxxx                   => 00000000 0xxxxxxx :      0 -    127
2 bytes : 1xxxxxxx 0yyyyyyy          => 00yyyyyy yxxxxxxx :    128 - 16,383
3 bytes : 1xxxxxxx 1yyyyyyy 000000zz => zzyyyyyy yxxxxxxx : 16,384 - 65,535
```

MSB (en yüksek bit) continuation flag: 1 = daha fazla byte var, 0 = bu son byte.

#### Serialize (satır 21-44)

```rust
impl Serialize for ShortU16 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_tuple(1)?;
        let mut rem_val = self.0;
        loop {
            let mut elem = (rem_val & 0x7f) as u8;  // Alt 7 bit
            rem_val >>= 7;
            if rem_val == 0 {
                seq.serialize_element(&elem)?;
                break;
            } else {
                elem |= 0x80;  // Continuation bit set
                seq.serialize_element(&elem)?;
            }
        }
        seq.end()
    }
}
```

#### Deserialize ile Güvenlik (satır 84-116)

```rust
enum VisitError {
    TooLong(usize),        // 3'ten fazla byte
    TooShort(usize),       // Continuation bit var ama byte yok
    Overflow(u32),         // u16 sığmaz
    Alias,                 // Canonical olmayan encoding (leading zeros)
    ByteThreeContinues,    // 3. byte'ta continuation bit var
}
```

**Alias koruması (satır 88-89):**
```rust
if elem == 0 && nth_byte != 0 {
    return Err(VisitError::Alias);
}
```

`0x80 0x00` (0'ın 2-byte kodlaması) reddedilir. Sadece canonical (en kısa) encoding kabul edilir. Bu, encoding canonicality'yi sağlar — her değerin sadece bir geçerli temsili vardır.

#### Test örnekleri (satır 291-299):

```
0x0   = [0x0]
0x7f  = [0x7f]
0x80  = [0x80, 0x01]
0xff  = [0xff, 0x01]
0x100 = [0x80, 0x02]
0x7fff = [0xff, 0xff, 0x01]
0xffff = [0xff, 0xff, 0x03]
```

### ShortVec

```rust
// satır 233
pub struct ShortVec<T>(pub Vec<T>);
```

Serde ile `serialize` / `deserialize` fonksiyonları:
- Önce vektör uzunluğunu ShortU16 olarak serialize eder.
- Sonra elemanları sırayla serialize eder.
- Maksimum uzunluk: u16::MAX = 65535

#### decode_shortu16_len (satır 255-266)

```rust
pub fn decode_shortu16_len(bytes: &[u8]) -> Result<(usize, usize), ()> {
    // (decoded_value, bytes_consumed) döndürür
}
```

Serde gerek kalmadan ham byte slice üzerinden decode. Transaction parsing için kullanışlı.

---

## 7. poseidon.rs (satır 1-453)

### Hata Enum'ı

```rust
// satır 11-38
pub enum PoseidonSyscallError {
    InvalidParameters,
    InvalidEndianness,
    InvalidNumberOfInputs,  // Maksimum 12 input
    EmptyInput,
    InvalidInputLength,     // 32 byte olmalı
    BytesToPrimeFieldElement,
    InputLargerThanModulus,
    VecToArray,
    U64Tou8,
    BytesToBigInt,
    InvalidWidthCircom,     // Width 2-16 aralığında
    Unexpected,
}
```

u64 ↔ enum dönüşümü: 1-11 arası hata kodları, diğerleri `Unexpected`.

### Parameters Enum

```rust
// satır 92-105
#[repr(u64)]
pub enum Parameters {
    Bn254X5 = 0,
}
```

Şu an sadece bir seçenek: BN254 eğrisi üzerinde x^5 S-Box. Bu, Circom/Groth16 ile uyumlu olan standarttır.

**BN254 Poseidon parametreleri:**
- S-Box: x^5
- Width: 2 ≤ t ≤ 13 (1-12 input)
- Full rounds: 8
- Partial rounds: width'e göre değişen [56, 57, 56, 60, 60, 63, 64, 63, 60, 66, 60, 65]

### Endianness Enum

```rust
// satır 127-133
#[repr(u64)]
pub enum Endianness {
    BigEndian = 0,
    LittleEndian,
}
```

BigEndian ve LittleEndian desteği. ZK sistemlerinde (özellikle Circom) genellikle BigEndian kullanılır.

### PoseidonHash Tipi

```rust
// satır 157-168
#[repr(transparent)]
pub struct PoseidonHash(pub [u8; HASH_BYTES]);  // 32 byte
```

`HASH_BYTES = 32` — BN254 prime field modulus'u 254 bit ≈ 32 byte.

### hashv Fonksiyonu

```rust
// satır 202-273
pub fn hashv(
    parameters: Parameters,
    endianness: Endianness,
    vals: &[&[u8]],
) -> Result<PoseidonHash, PoseidonSyscallError>
```

#### Off-chain implementasyon (satır 211-253)

```rust
#[cfg(not(target_os = "solana"))]
{
    use { ark_bn254::Fr, light_poseidon::{Poseidon, PoseidonBytesHasher} };
    let mut hasher = Poseidon::<Fr>::new_circom(vals.len())?;
    let res = match endianness {
        Endianness::BigEndian => hasher.hash_bytes_be(vals),
        Endianness::LittleEndian => hasher.hash_bytes_le(vals),
    }?;
    Ok(PoseidonHash(res))
}
```

`ark_bn254::Fr` — arkworks kütüphanesi BN254 prime field. `light_poseidon` — Poseidon hash implementasyonu.

#### On-chain implementasyon (satır 255-272)

```rust
#[cfg(target_os = "solana")]
{
    let mut hash_result = [0; HASH_BYTES];
    let result = unsafe {
        crate::syscalls::sol_poseidon(
            parameters.into(),
            endianness.into(),
            vals as *const _ as *const u8,
            vals.len() as u64,
            &mut hash_result as *mut _ as *mut u8,
        )
    };
    match result {
        0 => Ok(PoseidonHash::new(hash_result)),
        e => Err(PoseidonSyscallError::from(e)),
    }
}
```

**unsafe blok:** `sol_poseidon` syscall. `vals` fat pointer (`*const [&[u8]]`) `*const u8`'e cast edilir. Runtime bu pointer üzerinden tüm input slice'larına erişir. Bu unsafe cast, runtime ile API sözleşmesinin bilinçli bir parçasıdır.

#### Testler (satır 313-451)

Beklenen hash değerleri hardcoded:
- `test_poseidon_input_ones_be` — [1u8; 32] BigEndian hash değeri
- `test_poseidon_input_ones_le` — [1u8; 32] LittleEndian hash değeri
- `test_poseidon_input_ones_twos_be` — [1u8; 32] ve [2u8; 32] BigEndian
- `test_poseidon_input_one` — Tek bir '1' değeri için 12 farklı input sayısında hash tablosu

12 farklı input sayısı için beklenen hash tablosu test edilir, bu Poseidon hash fonksiyonunun farklı width'lerini kapsar.

---

## 8. Nova Shield Chain için Dersler

### 8.1 Durable Nonce Mimarisi

**Ders 1: Domain Separation (Alan Ayrımı)**
Nonce değerini blockhash'ten ayırmak için prefix kullanın:
```rust
const NONCE_HASH_PREFIX: &[u8] = b"NOVA_SHIELD_NONCE";
let nonce = hash(NONCE_HASH_PREFIX, blockhash);
```
Bu, Solana'nın Legacy/Current bug'ını yaşamayı önler.

**Ders 2: Versioned State**
`Versions` enum pattern'i çok güçlüdür. State machine değişikliklerini geriye dönük uyumlu biçimde yönetir:
```rust
pub enum NonceVersions {
    V1(Box<NonceState>),
    V2(Box<NonceState>), // Yeni format
}
```
Legacy state'ler artık geçersiz ama hesaplardaki veriler hala okunabilir.

**Ders 3: Nonce Hesabı Boyutu**
Nonce hesabı tam 80 byte. Bu değer testte doğrulanmış, dokümantasyona hardcode edilmiş. Kendi nonce yapımızda da sabit boyutu test ile doğrulayalım.

**Ders 4: Authority Kontrolü**
Nonce authorize işleminde `HashSet<Pubkey>` kullanımı:
```rust
if !signers.contains(&data.authority) { ... }
```
MPC imzalama senaryosunda bu kontrol eşikli (threshold) bir imza seti ile yapılabilir.

### 8.2 Rent (Depolama Maliyeti) Tasarımı

**Ders 5: Rent Muafiyet Eşiği**
Solana 2 yıllık rent muafiyeti kullanıyor. Nova Shield Chain için önerim:
- Tam muafiyet: Hesap oluşturulurken yeterli bakiye zorunlu.
- Ya da alternatif: Hesap başına yıllık sabit ücret (daha öngörülebilir).

**Ders 6: ACCOUNT_STORAGE_OVERHEAD = 128 byte**
Her hesabın sadece veri değil, metadata da tükettiğini unutmayın. Nova Shield Chain'de de her hesabın sabit overhead'ini tanımlayın:
```rust
pub const ACCOUNT_METADATA_OVERHEAD: u64 = 128; // en az
```

**Ders 7: RentDue Enum Pattern**
`Exempt` vs `Paying(0)` ayrımı önemli. Sadece sayısal değer döndürmek yerine enum kullanın:
```rust
pub enum StorageDue {
    Exempt,
    Owing(u64),
}
```

**Ders 8: CloneZeroed**
`Rent` struct'ında padding byte'larını temizleyen özel `Clone` implementasyonu. Kriptografik bağlamlarda hassas veri içeren struct'larda padding sıfırlaması güvenlik açısından önemlidir.

### 8.3 Sysvar Mimarisi

**Ders 9: Sysvar Erişim Stratejisi**
İki yol — hesap üzerinden veya syscall üzerinden. Syscall her zaman daha verimli:
```
Syscall: O(1), AccountInfo gerekmez, deserialization yok
AccountInfo: O(n) deserialization, transaction'da alan tüketir
```

**Ders 10: Büyük Sysvar'larda Override**
SlotHashes (20,488 byte), SlotHistory (131,097 byte) — bu sysvar'lar `from_account_info` override edilerek kasıtlı olarak hata döndürür. Nova Shield'de de büyük veri yapıları için program içi erişimi kısıtlayın, sadece RPC/off-chain sunun.

**Ders 11: declare_sysvar_id Makrosu**
Bu makro pattern — tek bir yerden ID + trait implementasyonu — çok temiz. Nova Shield'de sistem hesapları için benzer makrolar tanımlayın.

**Ders 12: Hard-coded Size'lar**
```rust
fn size_of() -> usize { 20_488 } // golden value
```
Bu "golden value" yaklaşımı, parametre değişimlerinde derleme zamanı uyarısı verecek testlerle desteklenmiştir. Kritik sabitler için her zaman bağımsız test yazın.

### 8.4 Stable Layout

**Ders 13: #[repr(C)] Zorunluluğu**
Runtime ile FFI yapıyorsanız:
- Bellek düzeninin değişmemesi için `#[repr(C)]` şart.
- Offset testleri zorunlu (memoffset crate).
- `ManuallyDrop` ile bellek transferini güvenli yapın.

**Ders 14: StableVec Pattern**
```
StableVec: ptr(8) + cap(8) + len(8) = 24 byte, #[repr(C)]
```
Nova Shield Chain'de runtime ile paylaşılan veri yapıları için aynı pattern kullanın. Vec/Slice'ların Rust sürüm bağımsızlığını garanti altına almak kritiktir.

**Ders 15: Rc/RefCell Bellek Düzeni Testi**
Solana, Rust'ın standart kütüphane tiplerinin bile bellek düzenini test ediyor. Bu, runtime-level güvenlik garanti için mükemmel bir yaklaşım. Nova Shield'de runtime ile birden fazla dile (Rust, C) arayüz gerekirse bu testler zorunlu olacak.

### 8.5 Kriptografi

**Ders 16: secp256k1_recover — Ethereum Uyumluluğu**
Nova Shield Chain, Ethereum cüzdanlarından imza almayı hedefliyorsa:
```rust
// Ethereum recovery_id dönüşümü:
let recovery_id = eth_v.wrapping_sub(27);
// Malleability önleme:
if signature.s.is_high() { return Err(...); }
```
Malleability'ye karşı korumanın varsayılan olmadığını unutmayın.

**Ders 17: secp256k1 Hash Güvenliği**
Belge net: "Programcı hash'i kendisi yapmalı, başka birinin hash verdiğini varsaymamalı." Nova Shield'de imza doğrulama endpointleri yalnızca hash almalı, ham mesaj değil.

**Ders 18: Poseidon Hash — ZK Uyumluluğu**
`ark_bn254::Fr` + `light_poseidon` kombinasyonu Groth16 ile uyumludur. Nova Shield Chain'de Poseidon kullanımı için:
- Off-chain: `light_poseidon` + `ark_bn254`
- On-chain: Syscall (Solana gibi)
- Parametre: BN254/x5, BigEndian (Circom standardı)

**Ders 19: Poseidon Input Limitleri**
Maksimum 12 input, her biri 32 byte, BN254 modülüsünden küçük olmalı. Merkle tree için 2-3 input tipik. 12 input büyük ihtimalle yeterli.

### 8.6 Short Vec / Compact Encoding

**Ders 20: Variable-Length Integer Encoding**
Transaction boyutunu küçültmek için ShortU16:
- 0-127: 1 byte (en yaygın durum)
- 128-16383: 2 byte
- 16384-65535: 3 byte

Ağ üzerinde küçük transaction boyutu için bu encoding kritik. Alias (canonical) kontrolü: Aynı değerin birden fazla temsili olmamalı — replay ve manipulation saldırılarını önler.

**Ders 21: Serde ile Özel Encoding**
`ShortVec<T>` pattern'i: `#[serde(with = "short_vec")]` ile var olan struct'lara compact encoding eklemek çok temiz:
```rust
#[derive(Serialize, Deserialize)]
pub struct Transaction {
    #[serde(with = "short_vec")]
    pub signatures: Vec<Signature>,
    // ...
}
```

---

## 9. Özet Tablo

| Bileşen | Temel Pattern | Unsafe Blok | Nova Shield Önemi |
|---|---|---|---|
| Durable Nonce | Domain-separated hash, versioned state | Yok | Replay protection için kritik |
| Rent | (128 + data_len) * rate * threshold | Yok | Depolama ekonomisi |
| Sysvar | Trait + macro, syscall erişim | impl_sysvar_get | Sistem bilgisi erişimi |
| Instructions Sysvar | Offset table, bitflags | Yok | Cross-instruction verification |
| StableVec | #[repr(C)], ManuallyDrop transfer | 4 blok | Runtime FFI için zorunlu |
| StableInstruction | #[repr(C)], farklı field sırası | Test'lerde | Runtime FFI için zorunlu |
| Rc/RefCell layout | Test-only doğrulama | Test'lerde | Rust iç yapı garantisi |
| secp256k1_recover | Syscall wrapper, dual-target | 1 blok | Ethereum uyumluluğu |
| ShortU16 | Variable-length, canonical encoding | Yok | Transaction boyutu optimizasyonu |
| Poseidon | BN254/x5, ark+light_poseidon | 1 blok | ZK uyumluluğu |

---

*Agent 06 analizi tamamlandı. Toplam ~3.753 satır kod analiz edildi.*
