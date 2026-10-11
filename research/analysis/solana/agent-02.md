# Solana SDK Program — Agent 02 Analizi

**Analiz Tarihi:** 2026-07-21
**Analiz Eden:** Agent 02
**Kapsam:** sdk/program/src/ — account_info, address_lookup_table/, alt_bn128/, blake3, big_mod_exp, bpf_loader, bpf_loader_upgradeable, borsh*

---

## 1. account_info.rs

**Dosya:** `sdk/program/src/account_info.rs`
**Satır Sayısı:** 532

### Struct Tanımları

#### `AccountInfo<'a>` (satır 19-36)
```rust
#[repr(C)]
pub struct AccountInfo<'a> {
    pub key: &'a Pubkey,
    pub lamports: Rc<RefCell<&'a mut u64>>,
    pub data: Rc<RefCell<&'a mut [u8]>>,
    pub owner: &'a Pubkey,
    pub rent_epoch: Epoch,
    pub is_signer: bool,
    pub is_writable: bool,
    pub executable: bool,
}
```

- `#[repr(C)]`: C uyumlu bellek düzeni. Runtime bu struct'ı ham bellekte oluşturup pointer geçirir; Rust tarafının tam offsetleri bilmesi gerekir.
- `Rc<RefCell<...>>`: Tek iş parçacıklı ortamda (BPF/SBF) runtime borrow checking. Aynı hesabın birden fazla kez ödünç alınmasını engeller. `Arc` yerine `Rc` kullanılması, BPF'in single-threaded olmasından dolayıdır (senkronizasyon yükü sıfır).
- `lamports` ve `data` alanları `&'a mut` olarak tutulur ama `RefCell` içine sarılır: bu sayede `AccountInfo` kopyalanabilir (`Clone`) iken, mutasyon sadece borrow kontrolü altında gerçekleşir.

### Trait Tanımları

#### `IntoAccountInfo<'a>` (satır 231-238)
```rust
pub trait IntoAccountInfo<'a> {
    fn into_account_info(self) -> AccountInfo<'a>;
}
```
Farklı tuple/referans tiplerinin `AccountInfo`'ya dönüştürülmesi için standart konversiyon noktası. `From<T>` implementasyonu bu trait'e delege eder.

#### `Account` (satır 242-244)
```rust
pub trait Account {
    fn get(&mut self) -> (&mut u64, &mut [u8], &Pubkey, bool, Epoch);
}
```
Ham hesap verilerini tek bir tuple olarak döndüren yardımcı trait. Test ve simülasyon kodları için kullanılır; runtime kodu bu trait'i çağırmaz.

### Fonksiyon Tanımları

| Satır | Fonksiyon | Açıklama |
|-------|-----------|----------|
| 57 | `signer_key()` | is_signer true ise key döner |
| 65 | `unsigned_key()` | Her zaman key döner |
| 69 | `lamports()` | RefCell borrow ile lamport okur |
| 73 | `try_lamports()` | Hata döndüren versiyon |
| 84 | `unsafe original_data_len()` | Pointer aritmetiği ile serialized veri |
| 90 | `data_len()` | Borrow ile veri uzunluğu |
| 148 | `realloc()` | Veri alanını yeniden boyutlandırır |
| 186 | `assign()` | Sahip programı değiştirir |
| 196 | `new()` | Constructor |
| 218 | `deserialize_data()` | Bincode deserializasyonu |
| 222 | `serialize_data()` | Bincode serializasyonu |
| 326 | `next_account_info()` | Iterator'dan sonraki AccountInfo |
| 378 | `next_account_infos()` | Iterator'dan N tane AccountInfo |

### Unsafe Bloklar

#### `original_data_len()` (satır 84-88)
```rust
pub unsafe fn original_data_len(&self) -> usize {
    let key_ptr = self.key as *const _ as *const u8;
    let original_data_len_ptr = key_ptr.offset(-4) as *const u32;
    *original_data_len_ptr as usize
}
```
**Neden unsafe?**
Runtime tarafından serialized edilmiş hesabın bellekte, `key` alanından 4 byte önce orijinal veri uzunluğunun u32 olarak yazıldığı varsayılır. Bu, Solana'nın BPF entry serialization formatının implicit bir özelliğidir. Rust borrow checker bu bellek bölgesini bilmez, dolayısıyla ham pointer okuma zorunludur.

**Risk:** Bu fonksiyonu doğrudan oluşturulmuş (runtime dışı) `AccountInfo` üzerinde çağırmak undefined behavior üretir. Dokümantasyonda "only for instances created by the runtime" notu mevcuttur.

#### `realloc()` (satır 148-183)
```rust
unsafe {
    let data_ptr = data.as_mut_ptr();
    *(data_ptr.offset(-8) as *mut u64) = new_len as u64;
    *data = from_raw_parts_mut(data_ptr, new_len)
}
```
**Neden unsafe?**
1. `data_ptr.offset(-8)`: Serialized hesap tamponunda, data slice'ından 8 byte önce uzunluk alanı tutulur. Bu pointer aritmetiği Rust'ın slice bound'larının dışındadır.
2. `from_raw_parts_mut`: Yeni uzunlukla yeni bir slice oluşturur. Allocator çağrılmaz; bellek zaten runtime tarafından pre-allocated olarak `MAX_PERMITTED_DATA_INCREASE` kadar geniş ayrılmıştır.
3. Güvenlik sınırı: `new_len - original_data_len > MAX_PERMITTED_DATA_INCREASE` kontrolü (satır 160) runtime'ın önceden ayırdığı sınırı aşmayı önler.

#### `assign()` (satır 186-194)
```rust
unsafe {
    std::ptr::write_volatile(
        self.owner as *const Pubkey as *mut [u8; 32],
        new_owner.to_bytes(),
    );
}
```
**Neden unsafe?**
`owner` alanı `&'a Pubkey` olarak tanımlıdır (immutable referans), ancak program sahipliği değiştirmek için bu alana yazılması gerekir. `write_volatile` hem Rust'ın immutable referans kuralını bypass eder hem de derleyicinin bu yazımı optimize etmesini engeller (volatile = gerçek bellek yazımı garantisi). `#[rustversion::attr(since(1.72), allow(invalid_reference_casting))]` bu cast'i kabul ettirir.

### Bellek Güvenliği Analizi

- `Rc<RefCell>` kombinasyonu aynı `AccountInfo`'nun aynı anda hem okunup hem yazılmasını (`borrow + borrow_mut`) çalışma zamanında engeller → `AccountBorrowFailed` hatası döner.
- `realloc` zero-init seçeneği (`zero_init: true`) stale data sızıntısını önler. Büyüme → küçülme → tekrar büyüme senaryosunda eski veriler kalabilir; bu durum dökümantasyonda açıkça belirtilmiştir.
- `next_account_infos()` bound check yapar (`accounts.len() < count`) → `NotEnoughAccountKeys`.

### Test Kapsamı

| Test | Satır | Ne Test Eder |
|------|-------|--------------|
| `test_next_account_infos` | 402 | Tekli ve çoklu iterator erişimi |
| `test_account_info_as_ref` | 438 | `AsRef` implementasyonu |
| `test_account_info_debug_data` | 447 | Debug format, >64 byte, <64 byte, 0 byte data |

### Nova Shield Chain İçin Dersler

1. **Hesap modeli tasarımı:** `Rc<RefCell<&mut T>>` pattern'i BPF'e özel olmakla birlikte, borrow-checked mutable paylaşım için iyi bir model. Nova Shield Chain'de WASM/native program ortamında benzer bir pattern kullanılabilir.
2. **Unsafe serialization layout:** Runtime'ın implicit bellek düzenine bağımlılık (offset -4, offset -8) kırılgan bir tasarımdır. Nova Shield'da programmatic realloc varsa, uzunluğu explicit bir header field olarak encode edin.
3. **Volatil yazım:** `write_volatile` yerine mümkünse alan mutability'sini `UnsafeCell` veya açık `*mut` olarak tanımlamak daha okunabilirdir.

---

## 2. address_lookup_table/ (tüm dosyalar)

### 2a. mod.rs (satır 1-20)

**Program ID:** `AddressLookupTab1e1111111111111111111111111` (satır 10)

#### `AddressLookupTableAccount` (satır 17-20)
```rust
pub struct AddressLookupTableAccount {
    pub key: crate::pubkey::Pubkey,
    pub addresses: Vec<crate::pubkey::Pubkey>,
}
```
Program dışı (client-side) kullanım için adres tablosu account temsilcisi. Adresler `Vec` içinde tutulur; on-chain versiyonu `Cow<'a, [Pubkey]>` kullanır (zero-copy için).

### 2b. error.rs (satır 1-20)

#### `AddressLookupError` enum (satır 4-20)
```rust
pub enum AddressLookupError {
    LookupTableAccountNotFound,
    InvalidAccountOwner,
    InvalidAccountData,
    InvalidLookupIndex,
}
```
4 hata varyantı. `thiserror` ile `Display` türetilir. Basit, non-recoverable hata türleri — her biri farklı bir başarısızlık modunu ifade eder.

### 2c. state.rs (satır 1-479)

#### Sabitler (satır 15-18)
```rust
pub const LOOKUP_TABLE_MAX_ADDRESSES: usize = 256;
pub const LOOKUP_TABLE_META_SIZE: usize = 56;
```
256 adres sınırı: 8-bit index (u8) ile adreslenebilir maksimum sayı. Meta boyutu 56 byte: bincode ile serialize edilmiş `ProgramState::LookupTable(LookupTableMeta)` boyutu (test satır 248 ile doğrulanmış).

#### `LookupTableStatus` enum (satır 22-26)
```rust
pub enum LookupTableStatus {
    Activated,
    Deactivating { remaining_blocks: usize },
    Deactivated,
}
```
Üç aşamalı yaşam döngüsü. `Deactivating` varyantı `remaining_blocks` taşır → kullanıcıya ne kadar süre kaldığını gösterir.

#### `LookupTableMeta` struct (satır 30-47)
```rust
pub struct LookupTableMeta {
    pub deactivation_slot: Slot,       // Slot::MAX = aktif
    pub last_extended_slot: Slot,
    pub last_extended_slot_start_index: u8,
    pub authority: Option<Pubkey>,     // None = frozen (immutable)
    pub _padding: u16,                 // 8-byte alignment için
}
```
Önemli tasarım kararları:
- `deactivation_slot: Slot::MAX` → aktif (sentinel değer). Explicit boolean yerine sentinel kullanımı alan sayısını azaltır.
- `authority: None` → tablo dondurulmuş (freeze edilmiş), artık kimse değiştiremez. `Option<Pubkey>` ile iki anlam tek alanda.
- `_padding: u16` → ardından gelen adres listesinin 8-byte hizalı kalması için.

#### `ProgramState` enum (satır 106-113)
```rust
pub enum ProgramState {
    Uninitialized,
    LookupTable(LookupTableMeta),
}
```
Account state için `#[allow(clippy::large_enum_variant)]` notu: `LookupTable` varyantı büyük olduğundan Clippy uyarısı bastırılmış. Bu, boxing yapmak yerine inline tutma tercihidir (performans öncelikli).

#### `AddressLookupTable<'a>` struct (satır 115-119)
```rust
pub struct AddressLookupTable<'a> {
    pub meta: LookupTableMeta,
    pub addresses: Cow<'a, [Pubkey]>,
}
```
`Cow<'a, [Pubkey]>`: On-chain deserializasyonda `Cow::Borrowed` (sıfır kopya), test/client tarafında `Cow::Owned`. Bu pattern hem performansı hem ergonomiyi optimize eder.

#### Fonksiyon Tanımları (state.rs)

| Satır | Fonksiyon | Açıklama |
|-------|-----------|----------|
| 70 | `is_active()` | status() sonucuna göre bool döner |
| 79 | `status()` | SlotHashes ile deactivation durumu hesaplar |
| 124 | `overwrite_meta_data()` | Meta alanı sıfırlayıp yeniden yazar |
| 138 | `get_active_addresses_len()` | Şu anda aranabilir adres sayısı |
| 165 | `lookup()` | Index listesini Pubkey listesine çevirir |
| 181 | `serialize_for_tests()` | Test amaçlı tam serialization |
| 192 | `deserialize()` | Zero-copy deserialization |

#### `status()` Algoritması (satır 79-102)
```
deactivation_slot == Slot::MAX    → Activated
deactivation_slot == current_slot → Deactivating { remaining: MAX_ENTRIES + 1 }
deactivation_slot in slot_hashes  → Deactivating { remaining: MAX_ENTRIES - position }
else                               → Deactivated
```
SlotHashes sysvar kullanımı: deactivation cool-down dönemini slot hash listesi üzerinden hesaplar. Tablo kapandıktan sonra aynı adreste yeniden tablo oluşturmayı da önler (recent slot requirement).

#### `deserialize()` (satır 192-216)
```rust
let addresses: &[Pubkey] = bytemuck::try_cast_slice(raw_addresses_data)...;
Ok(Self { meta, addresses: Cow::Borrowed(addresses) })
```
`bytemuck::try_cast_slice`: Alignment ve boyut kontrolü yapar, güvenli cast. `Pubkey` Pod-safe olduğu için sıfır kopyalama ile slice olarak döner.

#### `lookup()` (satır 165-178)
```rust
indexes.iter()
    .map(|idx| active_addresses.get(*idx as usize).cloned())
    .collect::<Option<_>>()
    .ok_or(AddressLookupError::InvalidLookupIndex)
```
`collect::<Option<_>>()`: Iterator'daki herhangi bir `None` tüm koleksiyonu `None`'a çevirir → tek bir `InvalidLookupIndex` hatası döner. Elegant kısa devre mantığı.

### 2d. instruction.rs (satır 1-215)

#### `ProgramInstruction` enum (satır 13-68)
```rust
pub enum ProgramInstruction {
    CreateLookupTable { recent_slot: Slot, bump_seed: u8 },
    FreezeLookupTable,
    ExtendLookupTable { new_addresses: Vec<Pubkey> },
    DeactivateLookupTable,
    CloseLookupTable,
}
```
5 instruction varyantı. `recent_slot` + `bump_seed` ile PDA türetimi: güvenlik için yakın slot gerekliliği, replay/malicious table reuse'u önler.

#### `derive_lookup_table_address()` (satır 71-79)
```rust
pub fn derive_lookup_table_address(authority_address: &Pubkey, recent_block_slot: Slot) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[authority_address.as_ref(), &recent_block_slot.to_le_bytes()],
        &id(),
    )
}
```
PDA türetimi: `[authority | recent_slot_le]`. Little-endian slot bytes kullanımı tuple'ı deterministic yapar.

#### Instruction Builder Fonksiyonları

| Satır | Fonksiyon |
|-------|-----------|
| 116 | `create_lookup_table_signed()` — authority signer |
| 132 | `create_lookup_table()` — authority signer değil |
| 143 | `freeze_lookup_table()` |
| 156 | `extend_lookup_table()` — optional payer |
| 184 | `deactivate_lookup_table()` |
| 201 | `close_lookup_table()` |

`create_lookup_table_signed` vs `create_lookup_table`: v1.12'de authority'nin signer olması zorunluluğu kaldırılmış. İkisi de `create_lookup_table_common()` private fonksiyonuna delege eder, sadece `authority_is_signer` flag'i farklı.

### Nova Shield Chain İçin Dersler

1. **PDA ile tablo adresi türetme:** `[authority | recent_slot]` pattern'i replay saldırılarına karşı etkili. Nova Shield'da da account/table oluşturma işlemleri yakın slot/block hash bağımlılığı kullanmalı.
2. **Cow<'a, T> ile zero-copy deserialization:** On-chain hesap verisi sık okunuyorsa `Cow::Borrowed` allocation'ı önler. Storage katmanından veri okurken benzer pattern kullanılabilir.
3. **Yaşam döngüsü state machine:** `Activated → Deactivating → Deactivated` üç durumlu model. Cool-down periodu için slot hash listesi kullanımı öğretici; Nova Shield'da hesap/kayıt kapatma mekanizmaları için benzer bir deactivation guard implement edilebilir.
4. **Sentinel değer:** `deactivation_slot: Slot::MAX` = aktif. Ek boolean field yerine sentinel kullanımı bellekten tasarruf sağlar.
5. **bytemuck::try_cast_slice:** Alignment-checked sıfır kopya cast. Nova Shield'da fixed-size veri listeleri için `bytemuck` kullanımı önerilir.

---

## 3. alt_bn128/ (tüm dosyalar)

### 3a. mod.rs — Ana Modül

**Dosya:** `sdk/program/src/alt_bn128/mod.rs`

#### Sabitler (satır 12-42, `mod consts`)
```
ALT_BN128_ADDITION_INPUT_LEN    = 128 (2 G1 noktası)
ALT_BN128_MULTIPLICATION_INPUT_LEN = 128 (1 G1 + 1 scalar)
ALT_BN128_PAIRING_ELEMENT_LEN  = 192 (1 G1 + 1 G2)
ALT_BN128_ADDITION_OUTPUT_LEN  = 64
ALT_BN128_MULTIPLICATION_OUTPUT_LEN = 64
ALT_BN128_PAIRING_OUTPUT_LEN   = 32
ALT_BN128_FIELD_SIZE            = 32
ALT_BN128_POINT_SIZE            = 64
ALT_BN128_ADD = 0, SUB = 1, MUL = 2, PAIRING = 3
```
Bu sabitler EVM'nin precompile adresleri (0x06, 0x07, 0x08) ile uyumlu alt_bn128 operasyonlarını tanımlar.

#### `AltBn128Error` enum (satır 44-58)
```rust
pub enum AltBn128Error {
    InvalidInputData,
    GroupError,
    SliceOutOfBounds,
    UnexpectedError,
    TryIntoVecError(Vec<u8>),
    ProjectiveToG1Failed,
}
```
`From<u64>` ve `Into<u64>` implementasyonları (satır 60-84): syscall'dan dönen u64 hata kodunu Rust enum'a dönüştürür. Kod eşlemesi: 1→InvalidInputData, 2→GroupError, 3→SliceOutOfBounds, 4→TryIntoVecError, 5→ProjectiveToG1Failed, 0/diğer→UnexpectedError.

#### Pod Wrapper Struct'ları (satır 86-92)
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodG1(pub [u8; 64]);   // G1 affine point (x,y her biri 32 byte)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
#[repr(transparent)]
pub struct PodG2(pub [u8; 128]);  // G2 affine point (x0,x1,y0,y1 her biri 32 byte)
```
`#[repr(transparent)]`: Bellek düzeni iç array ile özdeş. `Pod + Zeroable`: bytemuck safe cast. Bu wrapper'lar ham byte dizilerini typed G1/G2 noktalarına dönüştürmenin güvenli yoludur.

#### `TryFrom<PodG1> for G1` (satır 107-131, native arch)
```rust
impl TryFrom<PodG1> for G1 {
    fn try_from(bytes: PodG1) -> Result<Self, AltBn128Error> {
        if bytes.0 == [0u8; 64] { return Ok(G1::zero()); }
        let g1 = Self::deserialize_with_mode(..., Compress::No, Validate::Yes);
        if !g1.is_on_curve() { return Err(AltBn128Error::GroupError); }
        Ok(g1)
    }
}
```
Önemli güvenlik kontrolleri:
1. All-zero → identity element (infinity point). Bu EVM precompile davranışıyla uyumlu.
2. `Validate::Yes` ile deserialization → arkworks kütüphanesi curve üzerinde olup olmadığını kontrol eder.
3. Ek `is_on_curve()` double-check → subgroup attack'a karşı savunma.

#### `alt_bn128_addition()` (satır 159-195, native)
```rust
pub fn alt_bn128_addition(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
    // input padding to 128 bytes
    let p: G1 = PodG1(convert_endianness_64(...).try_into()?).try_into()?;
    let q: G1 = PodG1(convert_endianness_64(...).try_into()?).try_into()?;
    let result_point = p + q;  // arkworks G1 addition
    // serialize back with endianness conversion
}
```
Big-endian ↔ little-endian dönüşümü: Solana'nın dış API'si big-endian (Ethereum uyumlu), arkworks kütüphanesi little-endian çalışır. `convert_endianness_64` her 32 byte chunk'ı ters çevirir.

#### `alt_bn128_pairing()` (satır 235-286, native)
```rust
let res = <Bn<Config> as Pairing>::multi_pairing(
    vec_pairs.iter().map(|pair| pair.0),
    vec_pairs.iter().map(|pair| pair.1),
);
if res.0 == ark_bn254::Fq12::one() {
    result = BigInteger256::from(1u64);
}
```
Pairing sonucu Fq12'de, kimlik elementle karşılaştırılır. `1` döner ise pairing check başarılı. Bu, groth16 ve diğer pairing-tabanlı ZK sistemlerinin temel doğrulama adımıdır.

#### Syscall Versiyonu (satır 303-369, `#[cfg(target_os = "solana")]`)
```rust
pub fn alt_bn128_addition(input: &[u8]) -> Result<Vec<u8>, AltBn128Error> {
    let mut result_buffer = [0; ALT_BN128_ADDITION_OUTPUT_LEN];
    let result = unsafe {
        crate::syscalls::sol_alt_bn128_group_op(
            ALT_BN128_ADD,
            input as *const _ as *const u8,
            input.len() as u64,
            &mut result_buffer as *mut _ as *mut u8,
        )
    };
    match result { 0 => Ok(result_buffer.to_vec()), error => Err(error.into()) }
}
```

**Neden unsafe?**
SBF (Solana BPF) ortamında kriptografik hesaplamalar, doğrudan SBF VM syscall'larına devredilir. `sol_alt_bn128_group_op` bir foreign function'dır; Rust bağlayıcısı bu fonksiyonun mevcudiyetini veya güvenliğini garanti edemez. Raw pointer'lar ile çalışmak unsafe gerektirir.

**Tasarım kararı:** Aynı public API hem native hem de on-chain çalışır. `#[cfg(target_os = "solana")]` ile platforma özgü implementasyon seçilir. Bu, programları her iki ortamda da test etmeye izin verir.

### 3b. compression.rs

**Dosya:** `sdk/program/src/alt_bn128/compression.rs`

#### `AltBn128CompressionError` enum (satır 23-37)
```rust
pub enum AltBn128CompressionError {
    UnexpectedError,
    G1DecompressionFailed,
    G2DecompressionFailed,
    G1CompressionFailed,
    G2CompressionFailed,
    InvalidInputSize,
}
```
Ayrı bir compression modülü: nokta sıkıştırma/genişletme işlemleri ana mod'dan bağımsız. `From<u64>` / `Into<u64>` hata kodu eşlemesi aynı pattern'ı izler.

#### Boyut Sabitleri (`mod alt_bn128_compression_size`, satır 16-21)
```
G1 = 64, G1_COMPRESSED = 32
G2 = 128, G2_COMPRESSED = 64
```
G1 sıkıştırma: 64 → 32 byte (%50 tasarruf), y koordinatını çıkar, x + sign bit sakla.
G2 sıkıştırma: 128 → 64 byte (%50 tasarruf).

#### `alt_bn128_g1_decompress()` (satır 77-102, native)
```rust
pub fn alt_bn128_g1_decompress(g1_bytes: &[u8]) -> Result<[u8; G1], AltBn128CompressionError> {
    // all-zero check → identity
    let decompressed_g1 = G1::deserialize_with_mode(
        convert_endianness::<32, 32>(&g1_bytes).as_slice(),
        Compress::Yes,   // ← sıkıştırılmış format
        Validate::No,    // ← validation sonra yapılacak
    )?;
    // x ve y serialize et, endianness dönüştür
}
```
`Validate::No` kullanımı: arkworks validation pahalı olabilir, burada kütüphaneye güvenilir. Serileştirme hatası zaten yakalanır.

#### `convert_endianness<const CHUNK_SIZE, const ARRAY_SIZE>()` (satır 170-182)
```rust
pub fn convert_endianness<const CHUNK_SIZE: usize, const ARRAY_SIZE: usize>(
    bytes: &[u8; ARRAY_SIZE],
) -> [u8; ARRAY_SIZE] {
    let reversed: [_; ARRAY_SIZE] = bytes
        .chunks_exact(CHUNK_SIZE)
        .flat_map(|chunk| chunk.iter().rev().copied())
        .enumerate()
        .fold([0u8; ARRAY_SIZE], |mut acc, (i, v)| { acc[i] = v; acc });
    reversed
}
```
Const generics kullanımı: derleme zamanında chunk ve array boyutu belirlenir, tip güvenli endianness dönüşümü. `fold` ile allocation'sız yığın üzerinde çalışır.

#### Syscall Versiyonu (satır 185-265, `#[cfg(target_os = "solana")]`)
Aynı `unsafe { crate::syscalls::sol_alt_bn128_compression(...) }` pattern'i. Tüm dört işlem (g1/g2 compress/decompress) aynı syscall ile farklı opcode (0,1,2,3) üzerinden yapılır.

### Nova Shield Chain İçin Dersler

1. **BN254 (alt_bn128) entegrasyonu:** Nova Shield'ın ZK katmanı için bu kütüphane doğrudan kullanılabilir. arkworks'ün `ark_bn254` crate'i hem G1/G2 point işlemlerini hem pairing'i destekler.
2. **Dual-target pattern:** `#[cfg(not(target_os = "solana"))]` / `#[cfg(target_os = "solana")]` ayırımı; Nova Shield'da native test ortamı ve WASM/native runtime için aynı pattern uygulanabilir.
3. **Pod wrapper + bytemuck:** Kriptografik noktaları ham byte'lardan güvenle dönüştürmek için `PodG1/PodG2` tipi wrapper struct'lar önerilebilir.
4. **Endianness yönetimi:** Ethereum ekosistemiyle uyumluluk gerekiyorsa big-endian giriş/çıkış, iç hesaplama için little-endian dönüşümü zorunlu. `convert_endianness` fonksiyonu const-generics ile tip güvenli şekilde çözülmüş.
5. **Pairing check:** `res.0 == Fq12::one()` kontrolü Groth16 verifier'ın çekirdeğidir. Nova Shield'ın ZK doğrulama adımında bu yaklaşım benimsenebilir.
6. **Nokta doğrulama:** TryFrom'da `is_on_curve()` + `Validate::Yes` çift kontrol. Nova Shield'daki ZK input validation'da her dış G1/G2 noktasına bu kontrol uygulanmalı.

---

## 4. blake3.rs

**Dosya:** `sdk/program/src/blake3.rs`
**Satır Sayısı:** 224

### Struct Tanımları

#### `Hash` struct (satır 36)
```rust
#[repr(transparent)]
pub struct Hash(pub [u8; HASH_BYTES]);  // HASH_BYTES = 32
```
`#[repr(transparent)]`: İç array ile aynı bellek düzeni. Derive: Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash, AbiExample.

#### `Hasher` struct (satır 38-41)
```rust
pub struct Hasher {
    hasher: blake3::Hasher,
}
```
`blake3` crate'e thin wrapper. `Default` implement edilmiş, yeni bir hasher oluşturur.

### Fonksiyon Tanımları

| Satır | Fonksiyon | Açıklama |
|-------|-----------|----------|
| 44 | `Hasher::hash()` | Tek slice ekle |
| 47 | `Hasher::hashv()` | Birden fazla slice ekle |
| 52 | `Hasher::result()` | Hash'i sonuçlandır |
| 104 | `Hash::new()` | Slice'tan oluştur |
| 108 | `Hash::new_from_array()` | Array'dan oluştur (const) |
| 113 | `Hash::new_unique()` | Test için atomik sayaç tabanlı |
| 123 | `Hash::to_bytes()` | İç array döner |
| 129 | `hashv()` | Global hash fonksiyonu (native + syscall) |
| 154 | `hash()` | Tek slice hash |
| 159 | `extend_and_hash()` | Hash + veri konkatenasyonu hash |

### Unsafe Blok

#### `hashv()` syscall versiyonu (satır 139-150)
```rust
#[cfg(target_os = "solana")]
{
    let mut hash_result = [0; HASH_BYTES];
    unsafe {
        crate::syscalls::sol_blake3(
            vals as *const _ as *const u8,
            vals.len() as u64,
            &mut hash_result as *mut _ as *mut u8,
        );
    }
    Hash::new_from_array(hash_result)
}
```
**Neden unsafe?** `sol_blake3` SBF syscall'dır. `vals` (bir `&[&[u8]]`) ham pointer'a cast edilir; SBF VM bu pointer'ı interpret eder. Native platformda ise tamamen güvenli `blake3::Hasher` kullanılır.

### Tasarım Kararları

- **`Hash::new_unique()`** (satır 113-121): Atomik u64 sayaç, LE bytes olarak hash'in ilk 8 byte'ına yazılır. Test izolasyonu için her çağrıda farklı hash garantisi. Global `AtomicU64` — thread-safe.
- **`FromStr for Hash`**: base58 decode + boyut kontrolü. `MAX_BASE58_LEN = 44` üst sınır ile erken ret.
- **`extend_and_hash()`**: `id_bytes || val_bytes` → hash. Basit domain separation için kullanılır (ör. PoH hesaplaması).
- Blake3, SHA256'ya göre ~3-4x daha hızlı. Solana'nın PoH zinciri için ideal.

### Nova Shield Chain İçin Dersler

1. **Blake3 hash kullanımı:** Nova Shield'ın internal hash ihtiyaçları için `blake3` crate doğrudan kullanılabilir. SHA256 yerine blake3 seçimi performans açısından avantajlıdır.
2. **`Hash::new_unique()` pattern:** Test ortamında deterministik ama birbirinden farklı değerler üretmek için atomik sayaç + LE bytes pattern'i kullanılabilir.
3. **Dual-target hash:** Native'de kütüphane, on-chain'de syscall — aynı API. Nova Shield'ın program/runtime ayrımında bu pattern faydalıdır.

---

## 5. big_mod_exp.rs

**Dosya:** `sdk/program/src/big_mod_exp.rs`
**Satır Sayısı:** 141

### Struct Tanımları

#### `BigModExpParams` (satır 1-9)
```rust
#[repr(C)]
pub struct BigModExpParams {
    pub base: *const u8,
    pub base_len: u64,
    pub exponent: *const u8,
    pub exponent_len: u64,
    pub modulus: *const u8,
    pub modulus_len: u64,
}
```
`#[repr(C)]`: SBF syscall için C ABI uyumlu parametre yapısı. Raw pointer'lar (`*const u8`) ve uzunluklar ayrı field'lar olarak tanımlanmış — C'deki `(ptr, len)` pair convention'ı.

### Fonksiyon Tanımları

#### `big_mod_exp()` (satır 12-57)
```rust
pub fn big_mod_exp(base: &[u8], exponent: &[u8], modulus: &[u8]) -> Vec<u8>
```
İki implementasyon:

**Native (`#[cfg(not(target_os = "solana"))]`):**
```rust
use { num_bigint::BigUint, num_traits::{One, Zero} };
let base = BigUint::from_bytes_be(base);
let ret_int = base.modpow(&exponent, &modulus);
// zero-pad to modulus length
```
`num_bigint::BigUint::modpow()` kullanır. Modülün sıfır veya bir olduğu edge case'leri: sıfır döner.

**SBF (`#[cfg(target_os = "solana")]`):**
```rust
unsafe {
    crate::syscalls::sol_big_mod_exp(
        &param as *const _ as *const u8,
        return_value.as_mut_slice() as *mut _ as *mut u8,
    )
};
```

### Unsafe Blok (satır 48-53)

**Neden unsafe?**
`BigModExpParams` struct'ı ham pointer içerir. SBF VM'e pointer tabanlı struct geçirilmesi doğası gereği unsafe. `&param as *const _ as *const u8` cast'i Rust'ın referans garantilerini bypass eder.

### Kriptografik Tasarım Kararları

- Big integer modular exponentiation (modpow): RSA, Diffie-Hellman, bazı ZK sistemlerinde kullanılır.
- Çıktı her zaman `modulus.len()` uzunluğunda (satır 31): sonuç sıfır padded → sabit uzunluk, timing saldırısı azaltma.
- `modulus == 0` veya `modulus == 1` → sıfır döner (matematiksel doğruluk: a mod 0 tanımsız, a mod 1 = 0).

### Test Kapsamı (satır 59-140)

9 test vektörü, JSON inline. RSA-benzeri büyük sayılar ve basit modpow örnekleri. `array_bytes::hex2bytes_unchecked` ile hex string'den byte array dönüşümü.

### Nova Shield Chain İçin Dersler

1. **Modpow syscall pattern:** Büyük sayı işlemleri (RSA benzeri) için syscall + native dual implementation pattern, Nova Shield'da precompile katmanı tasarımında kullanılabilir.
2. **`#[repr(C)]` parametre struct:** Syscall'lara çok argümanlı geçiş yerine, tüm argümanları bir struct'ta toplayıp pointer geçirmek daha temiz bir ABI.
3. **Sabit uzunluk çıktı:** Kriptografik fonksiyon çıktılarını her zaman beklenen uzunluğa pad'lemek, çağıran tarafın boyut kontrolü yapmasını gereksiz kılar.

---

## 6. bpf_loader.rs

**Dosya:** `sdk/program/src/bpf_loader.rs`
**Satır Sayısı:** 24

Sadece program ID tanımı:
```rust
crate::declare_id!("BPFLoader2111111111111111111111111111111111");
```
BPF Loader v2 (güncel olmayan, kullanımdan kalkmakta). Bu loader'da deploy edilen programlar upgrade edilemez. `declare_id!` makrosu statik `id()` fonksiyonu ve `check_id()` fonksiyonu üretir.

---

## 7. bpf_loader_upgradeable.rs

**Dosya:** `sdk/program/src/bpf_loader_upgradeable.rs`
**Satır Sayısı:** 534

**Program ID:** `BPFLoaderUpgradeab1e11111111111111111111111` (satır 25)

### Enum Tanımları

#### `UpgradeableLoaderState` (satır 29-53)
```rust
pub enum UpgradeableLoaderState {
    Uninitialized,
    Buffer { authority_address: Option<Pubkey> },
    Program { programdata_address: Pubkey },
    ProgramData { slot: u64, upgrade_authority_address: Option<Pubkey> },
}
```

4 state:
- `Uninitialized`: 4 byte (sadece enum discriminant)
- `Buffer`: Program yüklemesi sırasında geçici buffer. `authority_address: None` = kim kontrol edemez.
- `Program`: 36 byte. Sadece `programdata_address` referansı saklar; asıl bytecode başka account'ta.
- `ProgramData`: 45 byte metadata + bytecode. `upgrade_authority_address: None` = immutable program.

Bu tasarım kritik: Program ve ProgramData'yı ayırarak;
- Program account'ı küçük ve sabit tutulur
- Upgrade: sadece ProgramData'nın `slot` ve bytecode kısmı güncellenir
- Program address değişmez → kullanıcı adresi sabit kalır

#### Sabit boyut fonksiyonları (satır 56-84)
```rust
pub const fn size_of_uninitialized() -> usize { 4 }
pub const fn size_of_buffer_metadata() -> usize { 37 }
pub const fn size_of_programdata_metadata() -> usize { 45 }
pub const fn size_of_program() -> usize { 36 }
pub const fn size_of_buffer(program_len: usize) -> usize { 37 + program_len }
pub const fn size_of_programdata(program_len: usize) -> usize { 45 + program_len }
```
`const fn`: Derleme zamanında hesaplanabilir. Account boyutunu hesaplamak için runtime'da allocation yapmadan kullanılabilir. Test'ler bu değerleri `bincode::serialized_size()` ile doğrular.

### Fonksiyon Tanımları

| Satır | Fonksiyon | Açıklama |
|-------|-----------|----------|
| 123 | `get_program_data_address()` | PDA türetimi |
| 128 | `create_buffer()` | Buffer account oluştur + initialize |
| 156 | `write()` | Buffer'a program chunk yaz |
| 175 | `deploy_with_max_program_len()` | Program deploy et |
| 210 | `upgrade()` | Program upgrade et |
| 232 | `is_upgrade_instruction()` | Opcode 3 kontrolü |
| 236 | `is_set_authority_instruction()` | Opcode 4 |
| 240 | `is_close_instruction()` | Opcode 5 |
| 244 | `is_set_authority_checked_instruction()` | Opcode 7 |
| 249 | `set_buffer_authority()` | Buffer authority değiştir |
| 267 | `set_buffer_authority_checked()` | Yeni authority da sign eder |
| 284 | `set_upgrade_authority()` | Program upgrade authority değiştir |
| 301 | `set_upgrade_authority_checked()` | Yeni authority da sign eder |
| 323 | `close()` | Buffer kapat |
| 337 | `close_any()` | Program/buffer/uninitialized kapat |
| 358 | `extend_program()` | ProgramData account boyutunu genişlet |

#### `is_*_instruction()` fonksiyonları (satır 232-246)
```rust
pub fn is_upgrade_instruction(instruction_data: &[u8]) -> bool {
    !instruction_data.is_empty() && 3 == instruction_data[0]
}
```
Bincode'un enum discriminant'ı: `Uninitialized=0, InitializeBuffer=1, Write=2, DeployWithMaxDataLen=3(?)...` — ancak burada opcode mapping farklı. İlk byte kontrolü, tam deserialization olmadan hızlı filtreleme sağlar. Validator bu fonksiyonları hot path'te kullanır.

`checked` versiyonları (satır 267-319): `SetAuthority` vs `SetAuthorityChecked` farkı — checked versiyonunda yeni authority'nin de imzalaması gerekir, bu single authority değişimlerindeki human error'ı önler.

### Upgrade Mekanizması Akışı

```
1. create_buffer(): Buffer account oluştur
2. write() × N: Program bytecode'u chunk'lar halinde yükle
3. deploy_with_max_program_len(): Buffer → ProgramData deploy
4. upgrade(): Yeni buffer → ProgramData replace
```

Deploy'da `max_data_len` parametresi: Future upgrade'lerin sığabileceği maksimum boyut. Account'u baştan büyük ayırmak, küçük → büyük geçişte rent sorunlarını önler.

### Test Kapsamı (satır 382-534)

- `test_state_size_of_*`: `bincode::serialized_size()` ile const boyutları doğrular
- `test_account_lengths`: Deprecated fonksiyonları eski API ile test eder
- `assert_is_instruction`: Her instruction enum varyantı için `is_*_instruction` fonksiyonlarını doğrular

### Nova Shield Chain İçin Dersler

1. **Program/data ayrımı:** Nova Shield'da akıllı kontrat deploy mekanizması tasarlanırken, kontrat mantığını küçük bir "pointer" account'ta + ayrı bytecode account'ta saklama modeli hem upgrade esnekliği hem de sabit kontrat adresi sağlar.
2. **Upgrade authority:** `Option<Pubkey>` ile `None = immutable` pattern'i. Nova Shield'da sistem programlarını immutable yapmak için authority'yi None'a çekme mekanizması implement edilebilir.
3. **`checked` authority transfer:** Yeni sahibin de imzalaması şartı, accidental ownership transfer'ı önler. Kritik izin değişikliklerinde bu pattern uygulanmalı.
4. **Hızlı instruction type detection:** Tam deserialization yerine ilk byte kontrolü, validator/runtime'da hot path optimizasyonu. Nova Shield instruction dispatcher'ında benzer hızlı filtreleme kullanılabilir.
5. **const fn boyut hesabı:** Account allocation boyutlarını `const fn` ile belirlemek, runtime'da gereksiz hesaplamayı önler ve derleme zamanında doğrulanabilir.

---

## 8. borsh.rs (ve borsh0_9.rs, borsh0_10.rs, borsh1.rs)

**Dosya:** `sdk/program/src/borsh.rs`, `borsh0_9.rs`, `borsh0_10.rs`, `borsh1.rs`

### Genel Yapı

`borsh.rs` bir "şablon" modüldür: 4 macro tanımlar ve bunları versiyona özgü modüller (`borsh0_9`, `borsh0_10`, `borsh1`) kendi `borsh` crate'leri ile instantiate eder.

### Macro Tanımları

#### `impl_get_packed_len_v0!` (satır 59-118)
BorshSchema'dan worst-case serializasyon boyutunu hesaplar. Definition türleri:
- `Array { length, elements }` → length × element_size
- `Enum { variants }` → 1 (discriminant) + max(variant_sizes)
- `Struct { NamedFields }` → toplam field boyutu
- `Sequence` → `panic!` (desteklenmiyor)
- Primitive'ler → sabit boyutlar (bool/u8=1, u16=2, u32=4, u64=8, u128=16)

`panic!("Missing support for Definition::Sequence")`: Dinamik uzunluklu diziler (Vec) şema üzerinden hesaplanamaz; bu durumda `get_instance_packed_len` kullanılmalı.

#### `impl_get_packed_len_v1!` (satır 121-180)
Borsh 1.x API'si ile güncellenmiş versiyon. `BorshSchemaContainer` + `get_definition()` API'si farklı. `Definition::Sequence { length_width, length_range }` → `length_width == 0` ise fixed array gibi davranır.

#### `impl_try_from_slice_unchecked!` (satır 183-201)
```rust
pub fn try_from_slice_unchecked<T: BorshDeserialize>(data: &[u8]) -> Result<T, Error> {
    let mut data_mut = data;
    let result = T::deserialize(&mut data_mut)?;
    Ok(result)
}
```
Tüketilmemiş byte kontrolü yapılmaz. Güvenlik notu (satır 32-34): Hedef tipten büyük buffer geçirilirse hata yakalanmaz. Bu fonksiyonun bilinçli kullanımı gerekir.

#### `impl_get_instance_packed_len!` (satır 204-237)
```rust
struct WriteCounter { count: usize }
impl Write for WriteCounter { fn write(&mut self, data: &[u8]) -> ... { self.count += data.len(); Ok(data.len()) } }
pub fn get_instance_packed_len<T: BorshSerialize>(instance: &T) -> Result<usize, Error> {
    let mut counter = WriteCounter::default();
    instance.serialize(&mut counter)?;
    Ok(counter.count)
}
```
Null writer pattern: `WriteCounter` gerçek bir buffer'a yazmaz, sadece byte sayısını tutar. Instance serialize edilip boyut öğrenilir — allocation gerektirmez.

#### `impl_tests!` (satır 241-357, cfg(test))
Test macro'su: 4 test fonksiyonu tanımlar (unchecked_deserialization, packed_len, instance_packed_len_matches_packed_len, instance_packed_len_with_vec, instance_packed_len_with_varying_sizes_in_hashmap). Her versiyona özgü modül bu macro'yu kendi tiplerle çağırır.

### Versiyon Modülleri

| Dosya | Borsh Versiyonu | Durum |
|-------|-----------------|-------|
| `borsh0_9.rs` | 0.9.x | deprecated since 1.17.0 |
| `borsh0_10.rs` | 0.10.x | deprecated since 1.18.0 |
| `borsh1.rs` | 1.x (current) | aktif |
| `borsh.rs` | 0.10.x re-export | deprecated since 1.17.0 |

`borsh0_9` ve `borsh0_10`: Backwards compatibility için korunmuş. Deprecation notice'ları ilgili `impl_*` macro çağrılarına `#[deprecated]` attribute ile eklenir.

### Nova Shield Chain İçin Dersler

1. **Borsh 1.x kullanımı:** Nova Shield'da account/mesaj serializasyonu için Borsh 1.x tercih edilmeli. Hem schema introspection hem de compact binary format avantajları var.
2. **Null writer boyut tespiti:** `WriteCounter` pattern'i, buffer allocate etmeden serializasyon boyutunu öğrenme için elegant bir yaklaşım. Nova Shield'da account storage tahsisatı öncesinde kullanılabilir.
3. **`try_from_slice_unchecked` güvenlik notu:** Eğer bu pattern kullanılırsa, buffer boyutunun tipten büyük olabileceği göz önünde bulundurulmalı; kritik path'lerde `try_from_slice` (tüketilen byte kontrolü yapar) tercih edilmeli.
4. **Makro ile çoklu versiyon desteği:** Aynı mantığı farklı crate API'leri için instant etmek istiyorsak macro tabanlı code generation iyi bir yaklaşım. Nova Shield'da protocol upgrade'leri sırasında benzer bir compatibility layer gerekebilir.

---

## Genel Özet ve Nova Shield Chain İçin Kritik Çıkarımlar

### unsafe Kullanım Patternleri (Özet)

| Dosya | unsafe nedeni | Risk seviyesi |
|-------|---------------|---------------|
| account_info.rs:84 | Pointer aritmetiği, serialization layout assumption | Yüksek |
| account_info.rs:165 | Slice realloc, raw pointer length update | Yüksek |
| account_info.rs:188 | Immutable ref'e write_volatile | Orta |
| alt_bn128/mod.rs:311+ | SBF syscall foreign fn | Orta |
| alt_bn128/compression.rs:197+ | SBF syscall foreign fn | Orta |
| blake3.rs:142 | SBF syscall foreign fn | Orta |
| big_mod_exp.rs:48 | SBF syscall foreign fn | Orta |

### Kriptografik Ekosistem Bulguları

1. **alt_bn128 (BN254)** tam implementasyon: addition, multiplication, pairing, G1/G2 compression. arkworks `ark_bn254` crate kullanılır.
2. **Blake3** hashing: PoH ve genel hash ihtiyaçları için.
3. **BigModExp**: RSA/DH tipi büyük sayı modüler üs alma.
4. Bu üç precompile EVM'dekine benzer; Solana'nın Ethereum cross-chain uyumluluğu hedefine işaret eder.

### Mimari Kararlar

1. **Dual-target pattern** (native lib / SBF syscall): Aynı public API, platform'a göre implementasyon seçimi. Test edilebilirlik + performans.
2. **Cow<'a, T>** zero-copy deserialization: On-chain performans kritik.
3. **Sentinel değerler** (Slot::MAX = aktif, None = frozen): Ek boolean field yerine alan tasarrufu.
4. **PDA güvenliği**: Recent slot + authority seed kombinasyonu, replay ve malicious table attacks'ı önler.
5. **const fn boyutlar**: Compile-time hesaplama, runtime overhead sıfır.
