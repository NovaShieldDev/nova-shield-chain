# Solana Agent-04 Analizi
## Analiz Edilen Dosyalar
- `sdk/program/src/hash.rs` (235 satir)
- `sdk/program/src/instruction.rs` (749 satir)
- `sdk/program/src/pubkey.rs` (995 satir)
- `sdk/program/src/lamports.rs` (23 satir)
- `sdk/program/src/loader_instruction.rs` (57 satir)
- `sdk/program/src/log.rs` (167 satir)

---

## 1. hash.rs — SHA-256 Hash Modulu

### Sabitler
| Satir | Tanim | Deger |
|-------|-------|-------|
| 16 | `HASH_BYTES: usize` | 32 |
| 18 | `MAX_BASE58_LEN: usize` | 44 (private) |

### Struct: `Hash` (satir 31-51)
```rust
#[repr(transparent)]
pub struct Hash(pub(crate) [u8; HASH_BYTES]);
```
- `#[repr(transparent)]` — bellekte tek bir `[u8; 32]` olarak temsil edilir, ABI uyumlu
- Derive'lar: `Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash, AbiExample, Pod, Zeroable`
- `Pod + Zeroable` (bytemuck) — sifir-kopya serializasyon icin, unsafe-safe araclari
- `pub(crate)` ic alan — dis erisim kontrol altinda
- `#[wasm_bindgen]` — WASM hedefi icin

### Struct: `Hasher` (satir 53-70)
```rust
pub struct Hasher {
    hasher: Sha256,
}
```
- `sha2::Sha256` sargisi (SHA-256 donanim/yazilim implementasyonu)
- **Metotlar:**
  - `hash(&mut self, val: &[u8])` (satir 59) — tek dilim hash'le
  - `hashv(&mut self, vals: &[&[u8]])` (satir 62) — coklu dilim, sirali isleme
  - `result(self) -> Hash` (satir 67) — finalize, hasher'i tuketir (move)

### Trait Implementasyonlari
| Satir | Trait | Notlar |
|-------|-------|--------|
| 72 | `Sanitize for Hash` | Bos impl — icerik dogrulamasi beklenmez |
| 74-78 | `From<[u8; 32]> for Hash` | Dogrudan donusum |
| 80-84 | `AsRef<[u8]> for Hash` | Dilim referansi |
| 86-96 | `fmt::Debug + fmt::Display` | bs58 encode ile gosterim |
| 106-122 | `FromStr for Hash` | base58 parse, MAX_BASE58_LEN kontrolu |

### Enum: `ParseHashError` (satir 98-104)
```rust
pub enum ParseHashError {
    WrongSize,   // decode sonrasi boyut yanlis
    Invalid,     // gecersiz base58 karakteri
}
```

### Metotlar — `impl Hash` (satir 124-147)
| Satir | Metot | Aciklama |
|-------|-------|---------|
| 125 | `new(hash_slice: &[u8]) -> Self` | Dilimden olustur, panic eder yanlis boyutta |
| 129 | `const fn new_from_array(...)` | const context'te kullanilabilir |
| 134 | `new_unique() -> Self` | Test/benchmark icin atomik sayac kullanir |
| 144 | `to_bytes(self) -> [u8; 32]` | Ic byte dizisini dondurur |

#### `new_unique()` Tasarim Karari
```rust
static I: AtomicU64 = AtomicU64::new(1);
let i = I.fetch_add(1);
b[0..8].copy_from_slice(&i.to_le_bytes());
```
- Global atomik sayac — thread-safe unique degerler
- Little-endian kullanir (Pubkey'den farkli — orada big-endian)

### Fonksiyonlar
| Satir | Fn | Aciklama |
|-------|-----|---------|
| 150-172 | `hashv(vals: &[&[u8]]) -> Hash` | Ana hash fonksiyonu, platform uyumlu |
| 175-177 | `hash(val: &[u8]) -> Hash` | `hashv` sargisi |
| 180-184 | `extend_and_hash(id: &Hash, val: &[u8]) -> Hash` | Mevcut hash'e veri ekleyip yeniden hash'le |

### UNSAFE BLOK — `hashv` (satir 163-170)
```rust
#[cfg(target_os = "solana")]
{
    let mut hash_result = [0; HASH_BYTES];
    unsafe {
        crate::syscalls::sol_sha256(
            vals as *const _ as *const u8,
            vals.len() as u64,
            &mut hash_result as *mut _ as *mut u8,
        );
    }
    Hash::new_from_array(hash_result)
}
```
- **Platform uyumlu cift implementasyon:** `#[cfg(not(target_os = "solana"))]` normal SHA-256, `#[cfg(target_os = "solana")]` syscall
- Ham pointer kullanimlari: fat pointer olan `&[&[u8]]` raw `*const u8`'e cast ediliyor
- BPF VM syscall arayuzu bu sekilde calisir

### Testler (satir 186-235)
- `test_new_unique`: Ardinisik `new_unique()` cagrilerinin farkli deger urettigi kontrol edilir
- `test_hash_fromstr`: base58 encode/decode round-trip, boyut hatalari, gecersiz karakter

---

## 2. instruction.rs — Instruction ve InstructionError

### Enum: `InstructionError` (satir 33-266)
**Solana runtime'in program hata kodlari. 50+ variant — hepsinin sabit discriminant degeri var, backward-compatible.**

| # | Variant | Aciklama |
|---|---------|---------|
| 1 | `GenericError` | Eski, deprecated. `Custom` kullanin |
| 2 | `InvalidArgument` | Program argumanlar gecersiz |
| 3 | `InvalidInstructionData` | Instruction data icerigi gecersiz |
| 4 | `InvalidAccountData` | Hesap verisi gecersiz |
| 5 | `AccountDataTooSmall` | Hesap verisi cok kucuk |
| 6 | `InsufficientFunds` | Yetersiz bakiye |
| 7 | `IncorrectProgramId` | Yanlis program ID |
| 8 | `MissingRequiredSignature` | Gerekli imza eksik |
| 9 | `AccountAlreadyInitialized` | Hesap zaten baslatilmis |
| 10 | `UninitializedAccount` | Baslatilmamis hesap |
| 11 | `UnbalancedInstruction` | Toplam lamport dengesi bozuldu |
| 12 | `ModifiedProgramId` | Program ID yasadisi degistirildi |
| 13 | `ExternalAccountLamportSpend` | Sahip olunmayan hesaptan harcama |
| 14 | `ExternalAccountDataModified` | Sahip olunmayan hesap verisi degistirildi |
| 15 | `ReadonlyLamportChange` | Read-only hesabin bakiyesi degisti |
| 16 | `ReadonlyDataModified` | Read-only hesabin verisi degisti |
| 17 | `DuplicateAccountIndex` | Duplicate hesap indexi (deprecated) |
| 18 | `ExecutableModified` | Executable biti degisti |
| 19 | `RentEpochModified` | Rent epoch degisti |
| 20 | `NotEnoughAccountKeys` | Yetersiz hesap sayisi |
| 21 | `AccountDataSizeChanged` | Hesap boyutu sahip olmayan tarafindan degistirildi |
| 22 | `AccountNotExecutable` | Executable hesap bekleniyor |
| 23 | `AccountBorrowFailed` | Hesap referansi alinamamadi (borrow checker) |
| 24 | `AccountBorrowOutstanding` | Program cikisinda borrow hala aktif |
| 25 | `DuplicateAccountOutOfSync` | Ayni hesap farkli sekillerde degistirildi |
| 26 | `Custom(u32)` | Program-ozel hata kodu |
| 27 | `InvalidError` | Gecersiz hata kodu |
| 28 | `ExecutableDataModified` | Executable hesap verisi degisti |
| 29 | `ExecutableLamportChange` | Executable hesap bakiyesi degisti |
| 30 | `ExecutableAccountNotRentExempt` | Executable hesap rent-exempt olmali |
| 31 | `UnsupportedProgramId` | Desteklenmeyen program ID |
| 32 | `CallDepth` | CPI derinlik limiti asildi |
| 33 | `MissingAccount` | Gerekli hesap eksik |
| 34 | `ReentrancyNotAllowed` | CPI yeniden giris yasak |
| 35 | `MaxSeedLengthExceeded` | Seed uzunlugu limiti asildi |
| 36 | `InvalidSeeds` | Gecersiz seed kombinasyonu |
| 37 | `InvalidRealloc` | Gecersiz veri yeniden tahsisi |
| 38 | `ComputationalBudgetExceeded` | Hesaplama butcesi asildi |
| 39 | `PrivilegeEscalation` | CPI'de yetkisiz signer/writable |
| 40 | `ProgramEnvironmentSetupFailure` | Program ortami kurulamadi |
| 41 | `ProgramFailedToComplete` | Program tamamlanamadi |
| 42 | `ProgramFailedToCompile` | Program derlenemedi |
| 43 | `Immutable` | Hesap degistirilemez |
| 44 | `IncorrectAuthority` | Yanlis yetkili |
| 45 | `BorshIoError(String)` | Borsh serializasyon hatasi (string icerir — dikkat!) |
| 46 | `AccountNotRentExempt` | Hesap rent-exempt degil |
| 47 | `InvalidAccountOwner` | Gecersiz hesap sahibi |
| 48 | `ArithmeticOverflow` | Aritmetik tasma |
| 49 | `UnsupportedSysvar` | Desteklenmeyen sysvar |
| 50 | `IllegalOwner` | Yasadisi sahip |
| 51 | `MaxAccountsDataAllocationsExceeded` | Transaction basi veri tahsis limiti |
| 52 | `MaxAccountsExceeded` | Maksimum hesap sayisi asildi |
| 53 | `MaxInstructionTraceLengthExceeded` | Instruction trace limiti |
| 54 | `BuiltinProgramsMustConsumeComputeUnits` | Builtin programlar CU tuketmeli |

**Tasarim Notu (satir 26-30):**
> "Members of this enum must not be removed, but new ones can be added."
> "it is dangerous to include error strings from 3rd party crates"

- `BorshIoError(String)` bu kurali ihlal ediyor — yalnizca on-chain programlarda kullanilmali, runtime'da degil

### Struct: `Instruction` (satir 328-339)
```rust
pub struct Instruction {
    pub program_id: Pubkey,
    pub accounts: Vec<AccountMeta>,
    pub data: Vec<u8>,
}
```
- `#[wasm_bindgen]` — WASM icin, alanlari `skip` ile gizlenmis
- `data: Vec<u8>` — tamamen opaque, program yorumlar

### `impl Instruction` Metotlari (satir 341-513)
| Satir | Metot | Aciklama |
|-------|-------|---------|
| 389-400 | `new_with_borsh<T: BorshSerialize>` | Borsh kodlamali instruction |
| 441-452 | `new_with_bincode<T: Serialize>` | Bincode kodlamali instruction |
| 498-504 | `new_with_bytes(program_id, data, accounts)` | Ham byte instruction |
| 510-512 | `new<T: Serialize>` | **Deprecated** v1.6.0'dan beri, bincode delegate eder |

**Tasarim Karari:** Birden fazla kodlama formatini desteklemek icin ayri konstruktorlar. Borsh'un stabil spesifikasyonu ve JS implementasyonu nedeniyle tercih edilmesi oneriliyor.

### Fn: `checked_add` (satir 519-521)
```rust
pub fn checked_add(a: u64, b: u64) -> Result<u64, InstructionError> {
    a.checked_add(b).ok_or(InstructionError::InsufficientFunds)
}
```
- Internal utility — overflow'u `InsufficientFunds` hatasi olarak dondurur
- `#[doc(hidden)]` — public API'nin partsai degil

### Struct: `AccountMeta` (satir 537-546)
```rust
#[repr(C)]
pub struct AccountMeta {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}
```
- `#[repr(C)]` — C ABI uyumlu layout, `ProcessedSiblingInstruction` ile birlikte syscall'da kullanilir
- **Metotlar:**
  - `new(pubkey, is_signer) -> Self` (satir 577) — writable=true
  - `new_readonly(pubkey, is_signer) -> Self` (satir 615) — writable=false

### Struct: `CompiledInstruction` (satir 631-642)
```rust
pub struct CompiledInstruction {
    pub program_id_index: u8,
    pub accounts: Vec<u8>,  // Transaction-level key array indeksleri
    pub data: Vec<u8>,
}
```
- `Message` icinde kullanilir, `Instruction`'in kompakt versiyonu
- Hesap adresleri `u8` indeksler olarak saklanir (transaction key array'ine referans)
- `#[serde(with = "short_vec")]` — kompakt VLQ benzeri kodlama

### Struct: `ProcessedSiblingInstruction` (satir 671-678)
```rust
#[repr(C)]
pub struct ProcessedSiblingInstruction {
    pub data_len: u64,
    pub accounts_len: u64,
}
```
- `sol_get_processed_sibling_instruction` syscall'u icin buffer boyutu bilgisi

### Fn: `get_processed_sibling_instruction` (satir 692-730)

**UNSAFE BLOK — Cift cagri deseni:**
```rust
// Once boyutlari al
unsafe {
    crate::syscalls::sol_get_processed_sibling_instruction(
        index as u64, &mut meta, &mut program_id,
        &mut u8::default(), &mut AccountMeta::default(),
    )
}
// Sonra doldurulan bufferlarla tekrar cagir
unsafe {
    crate::syscalls::sol_get_processed_sibling_instruction(
        index as u64, &mut meta, &mut program_id,
        data.as_mut_ptr(), accounts.as_mut_ptr(),
    )
}
```
- Klasik C tarzinda two-pass pattern: once boyutu sor, sonra doldur
- `meta.data_len` ve `meta.accounts_len` ile `Vec::resize_with` kullanir

### Fn: `get_stack_height` (satir 738-748)

**UNSAFE BLOK:**
```rust
#[cfg(target_os = "solana")]
unsafe {
    crate::syscalls::sol_get_stack_height() as usize
}
```
- CPI derinligini sorgular
- `TRANSACTION_LEVEL_STACK_HEIGHT = 1` sabiti ile karsilastirilir

---

## 3. pubkey.rs — Hesap Adresleri ve PDA

### Sabitler
| Satir | Sabit | Deger | Aciklama |
|-------|-------|-------|---------|
| 21 | `PUBKEY_BYTES` | 32 | Pubkey byte boyutu |
| 23 | `MAX_SEED_LEN` | 32 | Tek seed max uzunlugu |
| 25 | `MAX_SEEDS` | 16 | Toplam seed sayisi max |
| 27 | `MAX_BASE58_LEN` | 44 | private |
| 29 | `PDA_MARKER` | `b"ProgramDerivedAddress"` | 21 byte, PDA guvenlik markeri |

### Enum: `PubkeyError` (satir 31-54)
```rust
pub enum PubkeyError {
    MaxSeedLengthExceeded,  // Seed uzunluk limiti
    InvalidSeeds,            // Gecersiz seed kombinasyonu
    IllegalOwner,            // Yasadisi sahip
}
```
- `FromPrimitive + ToPrimitive` — hata kodlari u64'e donusturulebilir
- `From<u64> for PubkeyError` (satir 46-54): syscall donus degeri cevirmek icin, `_` kolu panic eder

### Struct: `Pubkey` (satir 70-92)
```rust
#[repr(transparent)]
pub struct Pubkey(pub(crate) [u8; 32]);
```
- Hash gibi `#[repr(transparent)]` ve `Pod + Zeroable`
- `#[cfg_attr(test, derive(Arbitrary))]` — fuzzing icin conditional derive
- Tum Hash trait'leriyle ayni derive seti + `AsMut<[u8]>`

### Enum: `ParsePubkeyError` (satir 96-102)
- `WrongSize`, `Invalid` — Hash ile ayni yapi
- `From<Infallible>` impl: `unreachable!()` — Infallible type deger alamaz, typesystem garantisi

### `impl Pubkey` Metotlari

#### `create_with_seed` (satir 211-229)
```rust
pub fn create_with_seed(base: &Pubkey, seed: &str, owner: &Pubkey) -> Result<Pubkey, PubkeyError>
```
**Algoritma:**
1. `seed.len() > MAX_SEED_LEN` kontrolu (32 byte limit)
2. Owner'in son 21 byte'i `PDA_MARKER` ile karsilastirilir
   - Eger `owner` adresi `"ProgramDerivedAddress"` ile bitiyorsa `IllegalOwner` hatasi
   - Bu, PDA'nin bir program sahibi olarak kullanilmasini engeller
3. `SHA256(base || seed || owner)` hesapla
4. Sonucu `Pubkey`'e donustur

**Guvenlik:** PDA_MARKER kontrolu, sahte PDA owner'lar olusturmayi engeller.

#### `find_program_address` (satir 484-487)
```rust
pub fn find_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> (Pubkey, u8)
```
- `try_find_program_address`'i sarar, `None` durumunda panic eder
- Off-chain'de kullanilmasi tavsiye edilir (compute budget yok)

#### `try_find_program_address` (satir 502-541) — PDA Arama Algoritmasi

**Off-chain implementasyon (satir 505-521):**
```rust
let mut bump_seed = [std::u8::MAX];  // 255'ten basla
for _ in 0..std::u8::MAX {           // 255 iterasyon max
    let mut seeds_with_bump = seeds.to_vec();
    seeds_with_bump.push(&bump_seed);
    match Self::create_program_address(&seeds_with_bump, program_id) {
        Ok(address) => return Some((address, bump_seed[0])),
        Err(PubkeyError::InvalidSeeds) => (),  // Curve uzerinde, devam et
        _ => break,                              // Diger hata, dur
    }
    bump_seed[0] -= 1;  // 255 -> 254 -> ... -> 0
}
None
```

**On-chain implementasyon (satir 523-540):**

**UNSAFE BLOK:**
```rust
let result = unsafe {
    crate::syscalls::sol_try_find_program_address(
        seeds as *const _ as *const u8,
        seeds.len() as u64,
        program_id as *const _ as *const u8,
        &mut bytes as *mut _ as *mut u8,
        &mut bump_seed as *mut _ as *mut u8,
    )
};
```
- Ayni is syscall ile tek seferde yapilir

**Onemli Tasarim:**
- 255'ten 0'a dogru azalma — en yuksek bump seed tercih edilir
- Bu standartlastirilmis davraniş — herkes ayni `bump_seed` degeri uretir
- Istatistiksel olarak ~%50 ihtimalle ilk denemede basarilir (curve disinda olmak)

#### `create_program_address` (satir 585-632) — Tek PDA Dogrulama

**Off-chain algoritma (satir 600-614):**
```rust
let mut hasher = crate::hash::Hasher::default();
for seed in seeds.iter() {
    hasher.hash(seed);       // Her seed ayri hash'lenir
}
hasher.hashv(&[program_id.as_ref(), PDA_MARKER]);  // program_id + marker eklenir
let hash = hasher.result();

if bytes_are_curve_point(hash) {
    return Err(PubkeyError::InvalidSeeds);  // Curve uzerinde — gecersiz PDA
}
Ok(Pubkey::from(hash.to_bytes()))
```

**Hash Carpısma Uyarisi (satir 282-288):**
> Seeds `{"abcdef"}`, `{"abc", "def"}`, `{"ab", "cd", "ef"}` ayni adresi uretir!
> Cunku seed'ler sirali hashlenip birlestiriliyor.
> Cozum: Seed'ler arasina ayirici ekle, ornegin `{"abc", "-", "def"}`

**On-chain implementasyon (satir 616-631):**

**UNSAFE BLOK:**
```rust
let result = unsafe {
    crate::syscalls::sol_create_program_address(
        seeds as *const _ as *const u8,
        seeds.len() as u64,
        program_id as *const _ as *const u8,
        &mut bytes as *mut _ as *mut u8,
    )
};
```

#### `is_on_curve` (satir 638-640)
```rust
pub fn is_on_curve(&self) -> bool {
    bytes_are_curve_point(self)
}
```
- PDA'nin gecerli olmasi icin `false` donmeli

#### `log` (satir 643-651)

**UNSAFE BLOK:**
```rust
#[cfg(target_os = "solana")]
unsafe {
    crate::syscalls::sol_log_pubkey(self.as_ref() as *const _ as *const u8)
};
```
- Pubkey'i verimli sekilde log'a yazar (format! olmadan)

### `bytes_are_curve_point` (satir 166-176)
```rust
pub fn bytes_are_curve_point<T: AsRef<[u8]>>(_bytes: T) -> bool {
    #[cfg(not(target_os = "solana"))]
    {
        curve25519_dalek::edwards::CompressedEdwardsY::from_slice(_bytes.as_ref())
            .decompress()
            .is_some()
    }
    #[cfg(target_os = "solana")]
    unimplemented!();
}
```
- On-chain'de `unimplemented!()` — sadece off-chain kullanilir
- ed25519 egrisinde bir nokta olup olmadigini kontrol eder
- PDA'lar curve DISINDA olmali

### Borsh Macro'lari (satir 693-741)
```rust
macro_rules! impl_borsh_schema { ... }
macro_rules! impl_borsh_serialize { ... }
```
- `borsh0_9` ve `borsh0_10` icin cift implementasyon
- Geriye donuk uyumluluk icin

### Testler (satir 743-994)
| Satir | Test | Ne Test Eder |
|-------|------|-------------|
| 748 | `test_new_unique` | Unique pubkey olusturma |
| 752 | `pubkey_fromstr` | base58 parsing, boundary cases |
| 793 | `test_create_with_seed` | Seed uzunluk limiti, UTF-8 seeds, bilinen vektorler |
| 848 | `test_create_program_address` | MAX_SEEDS, MAX_SEED_LEN, bilinen adresler |
| 936 | `test_pubkey_off_curve` | 1000 rastgele PDA — hepsi curve disinda olmali |
| 960 | `test_find_program_address` | 1000 rastgele — find=create(bump) dogrulama |
| 987 | `test_create_with_seed_rejects_illegal_owner` | PDA_MARKER ile biten owner reddedilmeli |

**Test Vektorleri (bilinen adresler):**
- `create_with_seed(default, "limber chicken: 4/45", default)` → `"9h1HyLCW5dZnBVap8C5egQ9Z6pHyjsh5MNy83iPqqRuq"`
- `create_program_address([b"", &[1]], BPFLoader)` → `"BwqrghZA2htAcqq8dzP1WDAhTXYTYWj7CHxF5j7TDBAe"`

---

## 4. lamports.rs — Lamport Aritmetik Hatalari

### Enum: `LamportsError` (satir 5-14)
```rust
pub enum LamportsError {
    ArithmeticUnderflow,  // u64 underflow
    ArithmeticOverflow,   // u64 overflow
}
```
- `From<LamportsError> for InstructionError` (satir 16-23)

**DIKKAT EDILMESI GEREKEN BUG:**
```rust
impl From<LamportsError> for InstructionError {
    fn from(error: LamportsError) -> Self {
        match error {
            LamportsError::ArithmeticOverflow  => InstructionError::ArithmeticOverflow,
            LamportsError::ArithmeticUnderflow => InstructionError::ArithmeticOverflow,  // BUG!
        }
    }
}
```
- `ArithmeticUnderflow` de `ArithmeticOverflow` olarak donuyor
- Bu muhtemelen kasitli (InstructionError'da UnderFlow variant yok) ya da bir bug
- Sonuc: InstructionError seviyesinde underflow/overflow ayirt edilemiyor

---

## 5. loader_instruction.rs — BPF Loader Instruktionlari

### Enum: `LoaderInstruction` (satir 12-35)
```rust
pub enum LoaderInstruction {
    Write {
        offset: u32,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    Finalize,
}
```
**Variant'lar:**
- `Write { offset, bytes }` — Program bytecodunu hesaba yazar, bolum bolum
  - Hesap: [0] WRITE + SIGNER
  - `serde_bytes` kullanilir — verimsiz `Vec<u8>` serializasyonunu onler
- `Finalize` — Hesabi executable yapar
  - Hesap: [0] WRITE + SIGNER, [1] Rent sysvar (read-only)

### Fonksiyonlar
| Satir | Fn | Imza |
|-------|-----|------|
| 37-49 | `write(...)` | `account_pubkey, program_id, offset, bytes -> Instruction` |
| 51-57 | `finalize(...)` | `account_pubkey, program_id -> Instruction` |

**Deploy akisi:**
1. `Write` ile program bytecodunu parcalar halinde yukle (offset arttirarak)
2. `Finalize` ile executable bit'i set et

---

## 6. log.rs — Program Loglama

### Makrolar

#### `info!` (satir 40-54) — Deprecated
- v1.4.14'den beri deprecated, `msg!` kullanin

#### `msg!` (satir 83-89) — Ana Log Makrosu
```rust
macro_rules! msg {
    ($msg:expr) => {
        $crate::log::sol_log($msg)   // Tek string — format! yok, hizli
    };
    ($($arg:tt)*) => ($crate::log::sol_log(&format!($($arg)*)));  // Formatli
}
```
- Tek string ise `format!` atlanir — onemli performans optimizasyonu
- Formatli ise `format!()` + `sol_log()` — CPU yogun, dikkatli kullanin

### Fonksiyonlar

#### `sol_log(message: &str)` (satir 93-101)

**UNSAFE BLOK:**
```rust
#[cfg(target_os = "solana")]
unsafe {
    crate::syscalls::sol_log_(message.as_ptr(), message.len() as u64);
}
```
- String pointer + uzunluk syscall'a gonderilir

#### `sol_log_64(arg1..arg5: u64)` (satir 105-113)

**UNSAFE BLOK:**
```rust
#[cfg(target_os = "solana")]
unsafe {
    crate::syscalls::sol_log_64_(arg1, arg2, arg3, arg4, arg5);
}
```
- 5 u64 deger hex formatinda log'a yazilir
- Eski `info!` makrosu bu fonksiyonu kullaniyordu

#### `sol_log_data(data: &[&[u8]])` (satir 116-124)

**UNSAFE BLOK:**
```rust
#[cfg(target_os = "solana")]
unsafe {
    crate::syscalls::sol_log_data(data as *const _ as *const u8, data.len() as u64)
};
```
- Base64 formatinda yazmak icin
- Yapısal veri log'lama icin kullanilir

#### `sol_log_slice(slice: &[u8])` (satir 128-132)
```rust
pub fn sol_log_slice(slice: &[u8]) {
    for (i, s) in slice.iter().enumerate() {
        sol_log_64(0, 0, 0, i as u64, *s as u64);
    }
}
```
- Her byte'i ayri satir olarak yazar (index, deger)
- Compute-intensive, debug amacli

#### `sol_log_params(accounts, data)` (satir 139-156)
- Tum AccountInfo'lari ve instruction data'yi log'a yazar
- Debug icin; production'da compute budget'i tuketir

#### `sol_log_compute_units()` (satir 160-167)

**UNSAFE BLOK:**
```rust
#[cfg(target_os = "solana")]
unsafe {
    crate::syscalls::sol_log_compute_units_();
}
```
- Kalan compute unit sayisini log'a yazar
- Profiling ve optimizasyon icin kullanilir

---

## Genel UNSAFE Ozeti

| Dosya | UNSAFE Blok | Amac |
|-------|-------------|------|
| hash.rs:163 | `sol_sha256` syscall | SHA-256 hesaplama (BPF) |
| instruction.rs:698 | `sol_get_processed_sibling_instruction` | Boyut sorgusu |
| instruction.rs:712 | `sol_get_processed_sibling_instruction` | Veri dolumu |
| instruction.rs:740 | `sol_get_stack_height` | CPI derinlik sorgusu |
| pubkey.rs:527 | `sol_try_find_program_address` | PDA arama (BPF) |
| pubkey.rs:619 | `sol_create_program_address` | PDA olusturma (BPF) |
| pubkey.rs:644 | `sol_log_pubkey` | Pubkey log'lama |
| log.rs:95 | `sol_log_` | String log |
| log.rs:107 | `sol_log_64_` | 64-bit hex log |
| log.rs:119 | `sol_log_data` | Base64 data log |
| log.rs:163 | `sol_log_compute_units_` | CU sayaci log |

**Ortak Pattern:**
- `&[T]` → `*const u8` cast: fat pointer'in data pointer'i BPF syscall'a verilir
- `&mut T` → `*mut u8` cast: output buffer icin
- `#[cfg(target_os = "solana")]` guard — unsafe bloklar sadece BPF'de derlenir

---

## Nova Shield Chain Icin Dersler

### 1. PDA Tasarim Prensipleri
- **Ed25519 curve disinda olmak zorunda** — her PDA adayini `bytes_are_curve_point` ile kontrol et
- **Bump seed 255'ten asagi** — en buyuk gecerli bump secilir, deterministik davranis icin standart
- **Seed carpismasi riski:** `hash(a, b)` ile `hash(ab)` farkli olmak zorunda degil. Nova Shield icin ya sabit uzunluklu seed kullan ya da ayirici byte ekle
- **PDA_MARKER** gibi bir domain separator kullan — `b"NovaShieldPDA"` gibi

### 2. InstructionError Tasarimi
- Hata kodlarini **hicbir zaman silme, sadece ekle** — backward compat kritik
- **`Custom(u32)`** pattern: program-spesifik hatalar icin ayri bir enum + u32 cast
- **String iceren variant'lardan kacin** runtime seviyesinde (`BorshIoError` sorunu gibi)
- Underflow/overflow icin **ayri variant** yap (`LamportsError` bug'i gibi durum olusmasin)

### 3. Hash Tasarimi
- `Hash` ve `Pubkey` ayni `[u8; 32]` layout ama farkli tipler — type safety
- `#[repr(transparent)]` + `Pod + Zeroable` — sifir-kopya serialization pattern
- `new_unique()` test yardimcisi — `AtomicU64` ile hizli, thread-safe

### 4. Platform Uyumlu Cift Implementasyon
Solana'nin kullandigi pattern Nova Shield'de de uygulanabilir:
```rust
pub fn my_hash(data: &[u8]) -> Hash {
    #[cfg(not(target_os = "nova-shield"))]
    { /* native Rust impl */ }
    #[cfg(target_os = "nova-shield")]
    { /* syscall impl */ }
}
```

### 5. Loglama Stratejisi
- `msg!` makrosu — tek string versiyonu `format!` atlayarak verimli
- `sol_log_compute_units()` pattern — profiling icin kritik
- Debug logging production'da **conditional compile** ile kaldir

### 6. Instruction Mimarisi
- `AccountMeta { pubkey, is_signer, is_writable }` — minimal, net
- `#[repr(C)]` AccountMeta — syscall arayuzleri icin ABI stabil
- `CompiledInstruction` — transaction seviyesinde indeks tabanli, boyut tasarrufu

### 7. Loader Instruction Pattern (iki adimli deploy)
- Buyuk veri (bytecode) parcalar halinde yükle: `Write(offset, bytes)`
- Son adimda commit et: `Finalize`
- Bu pattern buyuk on-chain veri yuklemeleri icin genel gecerli

### 8. Seed Hashleme Uyarisi (Hash Carpismasi)
Solana'da `create_program_address(["abc", "def"], id)` ile `create_program_address(["abcdef"], id)` **ayni sonucu verir**.
Nova Shield'de bu sorunu cozmek icin:
- Her seed'e uzunlugu prefix olarak ekle: `len(seed) || seed`
- Ya da sabit uzunluklu seed'ler kullan
- Ya da seed'ler arasi bilinen bir ayirici kullan

---

## Dosya Istatistikleri

| Dosya | Satir | Struct/Enum | Fn/Metot | Unsafe Blok | Test |
|-------|-------|-------------|----------|-------------|------|
| hash.rs | 235 | 3 (Hash, Hasher, ParseHashError) | 7 | 1 | 2 |
| instruction.rs | 749 | 4 (InstructionError, Instruction, AccountMeta, CompiledInstruction, ProcessedSiblingInstruction) | 12 | 3 | 0 |
| pubkey.rs | 995 | 4 (Pubkey, PubkeyError, ParsePubkeyError + makrolar) | 14 | 4 | 7 |
| lamports.rs | 23 | 1 (LamportsError) | 1 (From impl) | 0 | 0 |
| loader_instruction.rs | 57 | 1 (LoaderInstruction) | 2 | 0 | 0 |
| log.rs | 167 | 0 | 6 | 4 | 0 |
| **TOPLAM** | **2226** | **13** | **42** | **12** | **9** |
