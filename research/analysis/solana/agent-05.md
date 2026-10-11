# Solana Message Sistemi — Kapsamlı Analiz (Agent-05)

**Analiz edilen dizin:** `sdk/program/src/message/`
**Analiz tarihi:** 2026-07-22
**Toplam dosya:** 9 (mod.rs, legacy.rs, sanitized.rs, compiled_keys.rs, account_keys.rs, address_loader.rs, versions/mod.rs, versions/sanitized.rs, versions/v0/mod.rs, versions/v0/loaded.rs)

---

## 1. GENEL MİMARİ HARİTASI

```
message/
├── mod.rs                    — MessageHeader, modül re-export'ları, PUBLIC API
├── compiled_keys.rs          — CompiledKeys (iç yardımcı, pub(crate))
├── legacy.rs                 — Legacy Message formatı (Solana 1.0'dan beri)
├── account_keys.rs           — AccountKeys (static + dynamic birleşimi)
├── address_loader.rs         — AddressLoader trait + AddressLoaderError
├── sanitized.rs              — SanitizedMessage enum (Legacy + V0 sarmalayıcı)
└── versions/
    ├── mod.rs                — VersionedMessage enum, özel serde impl
    ├── sanitized.rs          — SanitizedVersionedMessage (sanitize wrapper)
    └── v0/
        ├── mod.rs            — v0::Message (Address Lookup Table destekli)
        └── loaded.rs         — LoadedMessage, LoadedAddresses
```

### Katmanlı Sanitizasyon Zinciri

```
Raw bytes
  → VersionedMessage (deserialize via custom serde)
  → SanitizedVersionedMessage (sanitize() çağrısı)
  → SanitizedMessage (AddressLoader ile ALT lookup)
    ├── SanitizedMessage::Legacy(LegacyMessage)
    └── SanitizedMessage::V0(v0::LoadedMessage)
```

---

## 2. mod.rs — Üst Düzey Modül

**Dosya:** `sdk/program/src/message/mod.rs`
**Satırlar:** 111

### Sabit

```rust
// Satır 59
pub const MESSAGE_HEADER_LENGTH: usize = 3;
```
MessageHeader'ın binary boyutu = 3 byte. Bunu `test_message_header_len_constant` testi doğrular (bincode::serialized_size ile karşılaştırır).

### Struct: MessageHeader (Satır 96-110)

```rust
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, Copy, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    pub num_required_signatures: u8,        // Satır 101
    pub num_readonly_signed_accounts: u8,   // Satır 105
    pub num_readonly_unsigned_accounts: u8, // Satır 109
}
```

**Tasarım Kararı:** Sadece 3 byte, fakat tüm account_keys dizisinin organize yapısını tarif eder. Hesap anahtarları sıralı olarak:
1. Writable + Signer (ilk `num_required_signatures - num_readonly_signed_accounts` adet)
2. Readonly + Signer (sonraki `num_readonly_signed_accounts` adet)
3. Writable + Unsigned (sonraki kısım)
4. Readonly + Unsigned (son `num_readonly_unsigned_accounts` adet)

**BPF/non-BPF ayrımı (Satır 43-55):**
```rust
#[cfg(not(target_os = "solana"))]
#[path = ""]
mod non_bpf_modules {
    mod account_keys;
    mod address_loader;
    mod sanitized;
    mod versions;
    pub use {account_keys::*, address_loader::*, sanitized::*, versions::*};
}
```
On-chain program ortamında (BPF) yalnızca `legacy::Message` ve `CompileError` erişilebilir. Diğer tüm tipler host-side'a özeldir. Bu backward compat sağlar.

---

## 3. legacy.rs — Legacy Message Formatı

**Dosya:** `sdk/program/src/message/legacy.rs`
**Satırlar:** 903

### Sabit Diziler (Satır 28-59)

```rust
// Satır 30 — 10 built-in program
pub static ref BUILTIN_PROGRAMS_KEYS: [Pubkey; 10] = { ... }

// Satır 53 — 256 elemanlı O(1) lookup tablosu
pub static ref MAYBE_BUILTIN_KEY_OR_SYSVAR: [bool; 256] = { ... }
```

**Önemli Optimizasyon (Satır 47-58):**
Built-in program ve sysvar kontrolü için iki aşamalı hızlı yol:
1. `MAYBE_BUILTIN_KEY_OR_SYSVAR[key.0[0] as usize]` — O(1) ilk byte kontrolü
2. Sadece true ise tam karşılaştırma yap
Bu, yaygın olmayan hesap anahtarlarında tam karşılaştırma maliyetini önler.

### fn is_builtin_key_or_sysvar (Satır 61-66)
```rust
pub fn is_builtin_key_or_sysvar(key: &Pubkey) -> bool {
    if MAYBE_BUILTIN_KEY_OR_SYSVAR[key.0[0] as usize] {
        return sysvar::is_sysvar_id(key) || BUILTIN_PROGRAMS_KEYS.contains(key);
    }
    false
}
```

### Private fn'ler (Satır 68-88)

```rust
fn position(keys: &[Pubkey], key: &Pubkey) -> u8          // Satır 68
fn compile_instruction(ix: &Instruction, keys: &[Pubkey]) -> CompiledInstruction  // Satır 72
fn compile_instructions(ixs: &[Instruction], keys: &[Pubkey]) -> Vec<CompiledInstruction>  // Satır 86
```

### Struct: Message (Satır 110-129)

```rust
#[wasm_bindgen]
#[frozen_abi(digest = "2KnLEqfLcTBQqitE22Pp8JYkaqVVbAkGbCfdeHoyxcAU")]
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub header: MessageHeader,              // Satır 114
    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,          // Satır 119
    pub recent_blockhash: Hash,             // Satır 122
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,  // Satır 128
}
```

**Tasarım Kararları:**
- `#[frozen_abi]` — binary formatın değişmezliğini garanti eder, consensus-breaking değişiklikleri önler
- `short_vec` — compact u16 encoding (standart u32 yerine), alan tasarrufu
- `#[wasm_bindgen]` — WASM ile erişilebilirlik
- fee_payer her zaman `account_keys[0]`

### impl Sanitize for Message (Satır 131-165)

```rust
fn sanitize(&self) -> Result<(), SanitizeError> {
    // (1) Satır 134-139: imzalı + unsigned readonly alanları çakışmamalı
    if self.header.num_required_signatures + self.header.num_readonly_unsigned_accounts
        > self.account_keys.len()
    { return Err(SanitizeError::IndexOutOfBounds); }

    // (2) Satır 141-144: En az 1 RW fee-payer olmalı
    if self.header.num_readonly_signed_accounts >= self.header.num_required_signatures {
        return Err(SanitizeError::IndexOutOfBounds);
    }

    // (3) Satır 146-158: Her instruction'ın program_id ve account indexleri geçerli olmalı
    for ci in &self.instructions {
        if ci.program_id_index >= account_keys.len() { ... }
        if ci.program_id_index == 0 { ... }  // Program fee-payer olamaz!
        for ai in &ci.accounts {
            if *ai >= account_keys.len() { ... }
        }
    }
    // (4) Alt bileşenleri de sanitize et
    self.account_keys.sanitize()?;
    self.recent_blockhash.sanitize()?;
    self.instructions.sanitize()?;
    Ok(())
}
```

**Kritik Kural:** `program_id_index == 0` yasak — index 0 her zaman fee-payer (Satır 150-153). Bu hem güvenlik kısıtlaması hem de protokol kuralı.

### impl Message — Constructor'lar (Satır 167-468)

#### new (Satır 236-238)
```rust
pub fn new(instructions: &[Instruction], payer: Option<&Pubkey>) -> Self {
    Self::new_with_blockhash(instructions, payer, &Hash::default())
}
```

#### new_with_blockhash (Satır 311-329)
```rust
pub fn new_with_blockhash(...) -> Self {
    let compiled_keys = CompiledKeys::compile(instructions, payer.cloned());
    let (header, account_keys) = compiled_keys.try_into_message_components()
        .expect("overflow when compiling message keys");
    let instructions = compile_instructions(instructions, &account_keys);
    Self::new_with_compiled_instructions(...)
}
```

**Derleme akışı:**
1. `CompiledKeys::compile()` — tüm instruction'lardan pubkey'leri topla
2. `try_into_message_components()` — sıralı header + account_keys üret
3. `compile_instructions()` — instruction'ları index tabanlı `CompiledInstruction`'a çevir

#### new_with_nonce (Satır 438-448)
```rust
pub fn new_with_nonce(...) -> Self {
    let nonce_ix = system_instruction::advance_nonce_account(...);
    instructions.insert(0, nonce_ix);  // İlk instruction olmalı
    Self::new(&instructions, payer)
}
```
Durable nonce için AdvanceNonceAccount her zaman instructions[0] olmalı.

#### new_with_compiled_instructions (Satır 450-468)
```rust
pub fn new_with_compiled_instructions(
    num_required_signatures: u8,
    num_readonly_signed_accounts: u8,
    num_readonly_unsigned_accounts: u8,
    account_keys: Vec<Pubkey>,
    recent_blockhash: Hash,
    instructions: Vec<CompiledInstruction>,
) -> Self
```
Ham constructore doğrudan erişim — test ve düşük seviye kullanım için.

### impl Message — Utility Metodlar (Satır 470-637)

| Metod | Satır | Açıklama |
|-------|-------|----------|
| `hash(&self) -> Hash` | 472 | blake3 hash, `b"solana-tx-message-v1"` prefix |
| `hash_raw_message(bytes) -> Hash` | 479 | Ham bytes üzerinden hash |
| `compile_instruction` | 487 | Tek instruction derle |
| `serialize(&self) -> Vec<u8>` | 491 | bincode serialize |
| `program_id(idx)` | 495 | idx'inci instruction'ın program ID'si |
| `program_ids()` | 505 | Tüm program ID'leri |
| `is_key_passed_to_program(idx)` | 512 | Hesap instruction'lara parametre mi? |
| `is_key_called_as_program(idx)` | 522 | Hesap program olarak çağrılıyor mu? |
| `is_non_loader_key(idx)` | 532 | Program değil veya parametre olarak geçirilmiş |
| `is_writable_index(i)` | 553 | Ham writable index kontrolü |
| `is_writable(i)` | 565 | Gerçek writable: sysvar/builtin hariç |
| `is_maybe_writable(i)` | 575 | Runtime dışı writable tahmini |
| `is_signer(i)` | 581 | `i < num_required_signatures` |
| `signer_keys()` | 608 | İlk N anahtar (N = num_required_signatures) |
| `has_duplicates()` | 618 | O(n²) duplicate arama, heap allocation yok |
| `is_upgradeable_loader_present()` | 632 | BPF upgradeable loader var mı? |

**Kritik Performans Notu (Satır 619-629):**
```rust
pub fn has_duplicates(&self) -> bool {
    // O(n^2) ama kısa diziler için HashSet'ten ~50x hızlı
    for i in 1..self.account_keys.len() {
        if self.account_keys[i..].contains(&self.account_keys[i - 1]) {
            return true;
        }
    }
    false
}
```
Benchmark'a dayalı karar: işlem boyutu küçük olduğu için hashset overhead'i geri tepkir.

### Message Hash Tasarımı (Satır 479-485)
```rust
pub fn hash_raw_message(message_bytes: &[u8]) -> Hash {
    use blake3::traits::digest::Digest;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solana-tx-message-v1");  // domain separation prefix
    hasher.update(message_bytes);
    Hash(hasher.finalize().into())
}
```
`solana-tx-message-v1` domain prefix — farklı context'lerdeki hash'lerin çakışmasını önler. Test bunu `test_message_hash` ile sabitler (beklenen hash: `7VWCF4quo2CcWQFNUayZiorxpiR5ix8YzLebrXKf3fMF`).

---

## 4. compiled_keys.rs — Anahtar Derleme Motoru

**Dosya:** `sdk/program/src/message/compiled_keys.rs`
**Satırlar:** 649

### Struct: CompiledKeys (Satır 14-17) — pub(crate)

```rust
pub(crate) struct CompiledKeys {
    payer: Option<Pubkey>,
    key_meta_map: BTreeMap<Pubkey, CompiledKeyMeta>,
}
```
**Tasarım:** `BTreeMap` (deterministic sıralama) kullanımı kritik — aynı instruction kümesinden her zaman aynı `account_keys` sırası üretilmeli.

### Enum: CompileError (Satır 20-28)

```rust
pub enum CompileError {
    AccountIndexOverflow,            // u8 overflow (max 256 hesap)
    AddressTableLookupIndexOverflow, // ALT index u8 overflow
    UnknownInstructionKey(Pubkey),   // Bilinmeyen hesap anahtarı
}
```

### Struct: CompiledKeyMeta (Satır 31-35) — private

```rust
struct CompiledKeyMeta {
    is_signer: bool,
    is_writable: bool,
    is_invoked: bool,
}
```
Bir pubkey'in tüm kullanım niteliklerini tek yerde toplar.

### fn compile (Satır 40-60)

```rust
pub(crate) fn compile(instructions: &[Instruction], payer: Option<Pubkey>) -> Self {
    let mut key_meta_map = BTreeMap::new();
    for ix in instructions {
        // program_id: is_invoked = true
        let meta = key_meta_map.entry(ix.program_id).or_default();
        meta.is_invoked = true;
        // her account meta: is_signer ve is_writable birleşimi
        for account_meta in &ix.accounts {
            let meta = key_meta_map.entry(account_meta.pubkey).or_default();
            meta.is_signer |= account_meta.is_signer;    // OR semantiği
            meta.is_writable |= account_meta.is_writable; // OR semantiği
        }
    }
    // payer: her zaman signer + writable
    if let Some(payer) = &payer {
        let meta = key_meta_map.entry(*payer).or_default();
        meta.is_signer = true;
        meta.is_writable = true;
    }
    Self { payer, key_meta_map }
}
```
**OR semantiği:** Bir hesap birden fazla instruction'da farklı niteliklerle geçerse, en geniş izin kazanır (writable olan bir yerde writable, signer olan bir yerde signer).

### fn try_into_message_components (Satır 62-117)

```rust
pub(crate) fn try_into_message_components(self) -> Result<(MessageHeader, Vec<Pubkey>), CompileError> {
    // Payer'ı çıkar (her zaman ilk sırada olacak)
    if let Some(payer) = &payer { key_meta_map.remove_entry(payer); }

    // 4 grubu filtrele
    let writable_signer_keys: Vec<Pubkey> = payer.into_iter().chain(
        key_meta_map.iter().filter_map(|(key, meta)| (meta.is_signer && meta.is_writable).then_some(*key))
    ).collect();
    let readonly_signer_keys: Vec<Pubkey> = ...;
    let writable_non_signer_keys: Vec<Pubkey> = ...;
    let readonly_non_signer_keys: Vec<Pubkey> = ...;

    let header = MessageHeader {
        num_required_signatures: try_into_u8(writable+readonly signer count)?,
        num_readonly_signed_accounts: try_into_u8(readonly_signer_keys.len())?,
        num_readonly_unsigned_accounts: try_into_u8(readonly_non_signer_keys.len())?,
    };

    // Deterministik sıra: [ws, rs, wu, ru]
    let static_account_keys = [writable_signer, readonly_signer, writable_non_signer, readonly_non_signer].concat();
    Ok((header, static_account_keys))
}
```

**Hesap sıralama kuralı:**
```
[WS₀(payer), WS₁, ..., RS₀, ..., WU₀, ..., RU₀, ...]
  ↑ fee payer her zaman ilk
```

### fn try_extract_table_lookup (Satır 119-149) — Sadece non-BPF

```rust
pub(crate) fn try_extract_table_lookup(
    &mut self,
    lookup_table_account: &AddressLookupTableAccount,
) -> Result<Option<(MessageAddressTableLookup, LoadedAddresses)>, CompileError>
```
- Signer veya invoked olmayan writable hesapları lookup table'dan çeker
- Signer veya invoked olmayan readonly hesapları lookup table'dan çeker
- Her iki liste de boşsa `None` döner (etkili lookup yok)

**ALT Seçim Kriterleri:**
```rust
// Writable: NOT signer AND NOT invoked AND writable
|meta| !meta.is_signer && !meta.is_invoked && meta.is_writable

// Readonly: NOT signer AND NOT invoked AND NOT writable
|meta| !meta.is_signer && !meta.is_invoked && !meta.is_writable
```
Signer ve invoked (program) hesaplar asla lookup table üzerinden yüklenemez — bunlar statik listede kalır.

### fn try_drain_keys_found_in_lookup_table (Satır 151-182) — private

ALT'ta bulunan anahtarları `key_meta_map`'ten çıkarır ve karşılık gelen indexes listesini döner. Bu mutasyonlu çalışır (`&mut self`).

---

## 5. account_keys.rs — AccountKeys

**Dosya:** `sdk/program/src/message/account_keys.rs`
**Satırlar:** 339

### Struct: AccountKeys<'a> (Satır 13-16)

```rust
pub struct AccountKeys<'a> {
    static_keys: &'a [Pubkey],
    dynamic_keys: Option<&'a LoadedAddresses>,
}
```
Lifetime parametreli — sahiplik almaz, borrow eder.

### Segment Sırası (Satır 36-48)

```rust
fn key_segment_iter(&self) -> impl Iterator<Item = &'a [Pubkey]> {
    if let Some(dynamic_keys) = self.dynamic_keys {
        [self.static_keys, &dynamic_keys.writable, &dynamic_keys.readonly].into_iter()
    } else {
        [self.static_keys, &[], &[]].into_iter()
    }
}
```
**Kesinleşmiş sıra:** `static | dynamic_writable | dynamic_readonly`
Bu sıra instruction account index'lerin çözümlenmesinde kritik — değiştirilmemeli.

### Metodlar

| Metod | Satır | Açıklama |
|-------|-------|----------|
| `new(static, dynamic)` | 26 | Constructor |
| `get(index)` | 54 | Segment üzerinden arama |
| `len()` | 66 | Toplam uzunluk |
| `is_empty()` | 75 | |
| `iter()` | 80 | Tüm segmentler üzerinde iterator |
| `compile_instructions()` | 91 | Panic'li compile |
| `try_compile_instructions()` | 106 | Result dönen compile |

### Index impl (Satır 18-23)
```rust
impl Index<usize> for AccountKeys<'_> {
    type Output = Pubkey;
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("index is invalid")
    }
}
```

### try_compile_instructions (Satır 106-139)
```rust
pub fn try_compile_instructions(&self, instructions: &[Instruction]) -> Result<Vec<CompiledInstruction>, CompileError> {
    // BTreeMap: her Pubkey -> u8 index
    let mut account_index_map = BTreeMap::<&Pubkey, u8>::new();
    for (index, key) in self.iter().enumerate() {
        let index = u8::try_from(index).map_err(|_| CompileError::AccountIndexOverflow)?;
        account_index_map.insert(key, index);
    }
    // Her instruction'ı derle
    instructions.iter().map(|ix| {
        let accounts: Vec<u8> = ix.accounts.iter()
            .map(|meta| account_index_map.get(&meta.pubkey).cloned()
                .ok_or(CompileError::UnknownInstructionKey(meta.pubkey)))
            .collect::<Result<_,_>>()?;
        Ok(CompiledInstruction { program_id_index: ..., data: ..., accounts })
    }).collect()
}
```
u8 overflow: max 256 toplam hesap (statik + dinamik).

---

## 6. address_loader.rs — AddressLoader Trait

**Dosya:** `sdk/program/src/message/address_loader.rs`
**Satırlar:** 57

### Enum: AddressLoaderError (Satır 7-31)

```rust
pub enum AddressLoaderError {
    Disabled,                  // ALT lookup devre dışı
    SlotHashesSysvarNotFound,  // Slot hashes sysvar yok
    LookupTableAccountNotFound,// Tablo hesabı yok
    InvalidAccountOwner,       // Yanlış program sahibi
    InvalidAccountData,        // Geçersiz hesap verisi
    InvalidLookupIndex,        // Geçersiz index
}
```

### Trait: AddressLoader (Satır 33-38)

```rust
pub trait AddressLoader: Clone {
    fn load_addresses(
        self,
        lookups: &[MessageAddressTableLookup],
    ) -> Result<LoadedAddresses, AddressLoaderError>;
}
```
`Clone` bound: runtime farklı bağlamlarda loader'ı klonlayabilmeli.

### Enum: SimpleAddressLoader (Satır 40-56)

```rust
pub enum SimpleAddressLoader {
    Disabled,
    Enabled(LoadedAddresses),
}
```
Test ve devre dışı bırakma senaryoları için basit impl. `Disabled` hata döner, `Enabled` önceden yüklenmiş adresleri verir.

---

## 7. versions/mod.rs — VersionedMessage

**Dosya:** `sdk/program/src/message/versions/mod.rs`
**Satırlar:** 403

### Sabit (Satır 24)

```rust
pub const MESSAGE_VERSION_PREFIX: u8 = 0x80;  // 0b10000000
```
İlk bit set = versioned message, clear = legacy.

### Enum: VersionedMessage (Satır 36-39)

```rust
#[frozen_abi(digest = "G4EAiqmGgBprgf5ePYemLJcoFfx4R7rhC1Weo2FVJ7fn")]
pub enum VersionedMessage {
    Legacy(LegacyMessage),
    V0(v0::Message),
}
```

### Serialization Mantığı (Satır 161-180)

```rust
impl Serialize for VersionedMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Legacy(message) => {
                // Legacy: prefix yok, doğrudan serialize
                let mut seq = serializer.serialize_tuple(1)?;
                seq.serialize_element(message)?;
                seq.end()
            }
            Self::V0(message) => {
                // V0: MESSAGE_VERSION_PREFIX (0x80) + versiyon (0) + message
                let mut seq = serializer.serialize_tuple(2)?;
                seq.serialize_element(&MESSAGE_VERSION_PREFIX)?;  // 0x80 | 0 = 0x80
                seq.serialize_element(message)?;
                seq.end()
            }
        }
    }
}
```

### Deserialize — Prefix Tespiti (Satır 182-305)

```rust
enum MessagePrefix {
    Legacy(u8),    // ilk byte = num_required_signatures
    Versioned(u8), // ilk byte'ın alt 7 biti = versiyon numarası
}
```

**Deserialization akışı:**
1. İlk byte oku
2. `byte & 0x80 != 0` → Versioned: alt 7 bit versiyon numarasını verir
3. Aksi hâlde → Legacy: bu byte `num_required_signatures`'dır
4. Versiyon 0 → V0 message deserialize et
5. Versiyon 127 (0x7F) → off-chain message, reddedilir (Satır 287-292)
6. Diğer → geçersiz, hata

**Version 127 özel durumu (Satır 287-292):**
```rust
127 => {
    // 0xff is used as the first byte of the off-chain messages
    // which corresponds to version 127 of the versioned messages.
    Err(de::Error::custom("off-chain messages are not accepted"))
}
```
Off-chain mesajlar (0xFF prefix) runtime'a hiç kabul edilmez.

### VersionedMessage Metodları (Satır 41-153)

| Metod | Satır | Açıklama |
|-------|-------|----------|
| `sanitize()` | 42 | Versiyona göre sanitize et |
| `header()` | 49 | MessageHeader referansı |
| `static_account_keys()` | 56 | Statik account_keys |
| `address_table_lookups()` | 63 | Legacy: None, V0: Some |
| `is_signer(i)` | 72 | |
| `is_maybe_writable(i)` | 80 | ALT yüklemeden önce |
| `is_invoked(i)` | 99 | Program olarak çağrılıyor mu |
| `is_non_loader_key(i)` | 108 | |
| `recent_blockhash()` | 112 | |
| `set_recent_blockhash()` | 119 | Mutable setter |
| `instructions()` | 128 | |
| `serialize()` | 135 | bincode |
| `hash()` | 140 | blake3 |
| `hash_raw_message()` | 146 | `b"solana-tx-message-v1"` prefix |

---

## 8. versions/sanitized.rs — SanitizedVersionedMessage

**Dosya:** `sdk/program/src/message/versions/sanitized.rs`
**Satırlar:** 46

### Struct: SanitizedVersionedMessage (Satır 8-10)

```rust
pub struct SanitizedVersionedMessage {
    pub message: VersionedMessage,
}
```

Wrapper struct — sanitize edilmiş olduğunu type-level'da garanti eder.

### TryFrom impl (Satır 12-17)
```rust
impl TryFrom<VersionedMessage> for SanitizedVersionedMessage {
    type Error = SanitizeError;
    fn try_from(message: VersionedMessage) -> Result<Self, Self::Error> {
        Self::try_new(message)
    }
}
```

### try_new (Satır 20-23)
```rust
pub fn try_new(message: VersionedMessage) -> Result<Self, SanitizeError> {
    message.sanitize()?;
    Ok(Self { message })
}
```

### Metodlar
- `instructions()` — CompiledInstruction dilimi
- `program_instructions_iter()` — (program_id, instruction) iterator

**Not:** `program_instructions_iter` yalnızca statik anahtarları kullanır (`static_account_keys()`) — ALT adresler henüz yüklenmedi, bu yüzden program_id her zaman statik listede olmalı (v0 sanitize'da garanti altına alınır).

---

## 9. versions/v0/mod.rs — V0 Message

**Dosya:** `sdk/program/src/message/versions/v0/mod.rs`
**Satırlar:** 689

### Struct: MessageAddressTableLookup (Satır 34-43)

```rust
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, AbiExample)]
pub struct MessageAddressTableLookup {
    pub account_key: Pubkey,           // ALT hesabının adresi
    #[serde(with = "short_vec")]
    pub writable_indexes: Vec<u8>,     // ALT'ta writable olarak yüklenecek indexler
    #[serde(with = "short_vec")]
    pub readonly_indexes: Vec<u8>,     // ALT'ta readonly olarak yüklenecek indexler
}
```

### Struct: v0::Message (Satır 55-88)

```rust
pub struct Message {
    pub header: MessageHeader,
    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,               // Satır 63 — sadece statik
    pub recent_blockhash: Hash,                  // Satır 66
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,  // Satır 82
    #[serde(with = "short_vec")]
    pub address_table_lookups: Vec<MessageAddressTableLookup>,  // Satır 87
}
```

**Legacy vs V0 Temel Farkı:**
- Legacy: `account_keys` — tüm hesaplar
- V0: `account_keys` (statik) + `address_table_lookups` (dinamik referanslar)

### impl Message::sanitize (Satır 92-172)

**Ek V0-özgü kontroller:**

```rust
// (1) Satır 106-122: Her ALT lookup en az 1 hesap yüklemeli
for lookup in &self.address_table_lookups {
    let num_lookup_indexes = lookup.writable_indexes.len() + lookup.readonly_indexes.len();
    if num_lookup_indexes == 0 { return Err(SanitizeError::InvalidValue); }
    total_lookup_keys += num_lookup_indexes;
}

// (2) Satır 135-138: Toplam hesap sayısı <= 256
let total_account_keys = num_static + num_dynamic;
if total_account_keys > 256 { return Err(SanitizeError::IndexOutOfBounds); }

// (3) Satır 148-154: Program ID yalnızca STATIK listeden gelebilir
let max_program_id_ix = num_static_account_keys - 1;
for ci in &self.instructions {
    if ci.program_id_index > max_program_id_ix {
        return Err(SanitizeError::IndexOutOfBounds);  // Dinamik program yasak
    }
}
```

**Kritik Kural:** Programlar (invoked hesaplar) ALT üzerinden yüklenemez. Bu statik analiz yapılabilmesi için zorunlu — program kodu on-chain yüklenmeden instruction analizi yapılabilmeli.

### impl Message::try_compile (Satır 254-285)

```rust
pub fn try_compile(
    payer: &Pubkey,
    instructions: &[Instruction],
    address_lookup_table_accounts: &[AddressLookupTableAccount],
    recent_blockhash: Hash,
) -> Result<Self, CompileError>
```

**Akış:**
1. `CompiledKeys::compile()` — tüm anahtarları topla
2. Her lookup table için `try_extract_table_lookup()` — dinamik hesapları çıkar
3. `try_into_message_components()` — kalan statik hesapları derle
4. `AccountKeys::try_compile_instructions()` — instructions'ı derle

### fn is_writable_index (Satır 305-328)

```rust
fn is_writable_index(&self, key_index: usize) -> bool {
    let num_account_keys = self.account_keys.len();
    let num_signed_accounts = self.header.num_required_signatures as usize;

    if key_index >= num_account_keys {
        // Dinamik hesap — writable_indexes toplamından hesapla
        let loaded_addresses_index = key_index - num_account_keys;
        let num_writable_dynamic = self.address_table_lookups.iter()
            .map(|l| l.writable_indexes.len()).sum();
        loaded_addresses_index < num_writable_dynamic
    } else if key_index >= num_signed_accounts {
        // Unsigned statik hesap
        let num_writable_unsigned = (num_account_keys - num_signed_accounts)
            - self.header.num_readonly_unsigned_accounts as usize;
        (key_index - num_signed_accounts) < num_writable_unsigned
    } else {
        // Signed hesap
        let num_writable_signed = num_signed_accounts
            - self.header.num_readonly_signed_accounts as usize;
        key_index < num_writable_signed
    }
}
```

**V0 özelliği:** `key_index >= num_account_keys` ise dinamik (ALT'tan yüklenen) hesap. Bu dalda writable sayısı `address_table_lookups`'dan hesaplanır.

---

## 10. versions/v0/loaded.rs — LoadedMessage ve LoadedAddresses

**Dosya:** `sdk/program/src/message/versions/v0/loaded.rs`
**Satırlar:** 329

### Struct: LoadedAddresses (Satır 25-30)

```rust
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedAddresses {
    pub writable: Vec<Pubkey>,  // ALT'tan yüklenen writable hesaplar
    pub readonly: Vec<Pubkey>,  // ALT'tan yüklenen readonly hesaplar
}
```

### FromIterator<LoadedAddresses> (Satır 32-43)

```rust
impl FromIterator<LoadedAddresses> for LoadedAddresses {
    fn from_iter<T: IntoIterator<Item = LoadedAddresses>>(iter: T) -> Self {
        let (writable, readonly): (Vec<Vec<Pubkey>>, Vec<Vec<Pubkey>>) = iter
            .into_iter()
            .map(|a| (a.writable, a.readonly))
            .unzip();
        LoadedAddresses {
            writable: writable.into_iter().flatten().collect(),
            readonly: readonly.into_iter().flatten().collect(),
        }
    }
}
```
Birden fazla ALT'tan gelen adresleri tek bir `LoadedAddresses` listesinde birleştirir. `unzip()` + `flatten()` elegantı.

### Struct: LoadedMessage<'a> (Satır 12-20)

```rust
pub struct LoadedMessage<'a> {
    pub message: Cow<'a, v0::Message>,
    pub loaded_addresses: Cow<'a, LoadedAddresses>,
    pub is_writable_account_cache: Vec<bool>,  // Ön-hesaplanmış cache
}
```

**`Cow<'a, T>` Kullanımı:** Hem `Owned` hem `Borrowed` olabilir. Runtime'da borrow, test'te owned. Gereksiz kopyalamayı önler.

**`is_writable_account_cache`:** Writable durumu her erişimde hesaplamak pahalı olduğu için constructor'da bir kez hesaplanıp cache'lenir.

### Constructor'lar (Satır 57-89)

```rust
// Owned (test/standalone)
pub fn new(message: v0::Message, loaded_addresses: LoadedAddresses) -> Self {
    let mut loaded_message = Self { message: Cow::Owned(message), ... };
    loaded_message.set_is_writable_account_cache();
    loaded_message
}

// Borrowed (runtime — kopyalama yok)
pub fn new_borrowed(message: &'a v0::Message, loaded_addresses: &'a LoadedAddresses) -> Self {
    let mut loaded_message = Self { message: Cow::Borrowed(message), ... };
    loaded_message.set_is_writable_account_cache();
    loaded_message
}
```

### set_is_writable_account_cache (Satır 78-89)

```rust
fn set_is_writable_account_cache(&mut self) {
    let is_writable_account_cache = self.account_keys().iter().enumerate()
        .map(|(i, _key)| self.is_writable_internal(i))
        .collect::<Vec<_>>();
    let _ = std::mem::replace(&mut self.is_writable_account_cache, is_writable_account_cache);
}
```
`mem::replace` — eski cache'i drop eder, yenisini atar. `std::mem::replace` dönüş değeri `_` ile görmezden gelinir.

### account_keys (Satır 92-94)

```rust
pub fn account_keys(&self) -> AccountKeys {
    AccountKeys::new(&self.message.account_keys, Some(&self.loaded_addresses))
}
```
Static + Dynamic anahtarları birleştirerek `AccountKeys` üretir.

### is_writable_internal (Satır 130-137)

```rust
fn is_writable_internal(&self, key_index: usize) -> bool {
    if self.is_writable_index(key_index) {
        if let Some(key) = self.account_keys().get(key_index) {
            return !(is_builtin_key_or_sysvar(key) || self.demote_program_id(key_index));
        }
    }
    false
}
```
**Demote program id:** Bir hesap writable olarak işaretlense bile, program olarak çağrılıyorsa (ve upgradeable loader yoksa) write lock düşürülür.

### is_writable (Satır 139-143)

```rust
pub fn is_writable(&self, key_index: usize) -> bool {
    *self.is_writable_account_cache.get(key_index).unwrap_or(&false)
}
```
Cache'den O(1) okuma.

### has_duplicates (Satır 102-105)

```rust
pub fn has_duplicates(&self) -> bool {
    let mut uniq = HashSet::new();
    self.account_keys().iter().any(|x| !uniq.insert(x))
}
```
Legacy'nin O(n²)'sine karşı burada `HashSet` kullanılır — dinamik adresler dahil olduğu için daha büyük küme bekleniyor.

---

## 11. sanitized.rs — SanitizedMessage

**Dosya:** `sdk/program/src/message/sanitized.rs`
**Satırlar:** 661

### Struct: LegacyMessage<'a> (Satır 25-31)

```rust
pub struct LegacyMessage<'a> {
    pub message: Cow<'a, legacy::Message>,
    pub is_writable_account_cache: Vec<bool>,
}
```
LoadedMessage ile paralel yapı — legacy için de writable cache.

### Enum: SanitizedMessage (Satır 72-77)

```rust
pub enum SanitizedMessage {
    Legacy(LegacyMessage<'static>),
    V0(v0::LoadedMessage<'static>),
}
```
`'static` lifetime — sanitize edilmiş mesaj sahibi olduğu veriyi taşır.

### Enum: SanitizeMessageError (Satır 79-89)

```rust
pub enum SanitizeMessageError {
    IndexOutOfBounds,
    ValueOutOfBounds,
    InvalidValue,
    AddressLoaderError(#[from] AddressLoaderError),
}
```
`#[from]` ile `AddressLoaderError` otomatik dönüşüm.

### From<SanitizeError> (Satır 91-99)

```rust
impl From<SanitizeError> for SanitizeMessageError {
    fn from(err: SanitizeError) -> Self {
        match err {
            SanitizeError::IndexOutOfBounds => Self::IndexOutOfBounds,
            SanitizeError::ValueOutOfBounds => Self::ValueOutOfBounds,
            SanitizeError::InvalidValue => Self::InvalidValue,
        }
    }
}
```

### SanitizedMessage::try_new (Satır 105-119)

```rust
pub fn try_new(
    sanitized_msg: SanitizedVersionedMessage,
    address_loader: impl AddressLoader,
) -> Result<Self, SanitizeMessageError> {
    Ok(match sanitized_msg.message {
        VersionedMessage::Legacy(message) => {
            SanitizedMessage::Legacy(LegacyMessage::new(message))
        }
        VersionedMessage::V0(message) => {
            let loaded_addresses = address_loader.load_addresses(&message.address_table_lookups)?;
            SanitizedMessage::V0(v0::LoadedMessage::new(message, loaded_addresses))
        }
    })
}
```
**İki aşamalı işlem:**
1. `SanitizedVersionedMessage` — statik sanitize
2. `try_new` — dinamik adres yükleme (ALT lookup)

### try_from_legacy_message (Satır 122-125)

```rust
pub fn try_from_legacy_message(message: legacy::Message) -> Result<Self, SanitizeMessageError> {
    message.sanitize()?;
    Ok(Self::Legacy(LegacyMessage::new(message)))
}
```
Legacy için ALT yoktur, direkt sanitize + wrap.

### SanitizedMessage Metodları

| Metod | Satır | Açıklama |
|-------|-------|----------|
| `has_duplicates()` | 128 | Her iki variant için |
| `header()` | 137 | MessageHeader referansı |
| `legacy_message()` | 145 | Option<&legacy::Message> |
| `fee_payer()` | 153 | account_keys[0] |
| `recent_blockhash()` | 161 | |
| `instructions()` | 170 | |
| `program_instructions_iter()` | 179 | (program_id, ix) iterator |
| `account_keys()` | 193 | AccountKeys (static+dynamic) |
| `message_address_table_lookups()` | 201 | Legacy: &[], V0: slice |
| `is_key_passed_to_program()` | 210 | |
| `is_invoked()` | 222 | |
| `is_non_loader_key()` | 231 | |
| `is_writable()` | 237 | Cache'den |
| `is_signer()` | 246 | |
| `loaded_lookup_table_addresses()` | 251 | V0'a özgü |
| `num_readonly_accounts()` | 259 | Legacy + ALT readonly sayısı |
| `decompile_instructions()` | 270 | BorrowedInstruction listesi |
| `is_upgradeable_loader_present()` | 297 | |
| `get_ix_signers(ix_index)` | 305 | Bir instruction'ın imzacıları |
| `get_durable_nonce()` | 320 | Durable nonce pubkey |
| `num_signatures()` | 347 | Toplam imza sayısı |
| `num_write_locks()` | 353 | Write lock sayısı |
| `get_signature_details()` | 360 | TransactionSignatureDetails |

### TransactionSignatureDetails (Satır 389-420)

```rust
pub struct TransactionSignatureDetails {
    num_transaction_signatures: u64,       // Ed25519 tx imzaları
    num_secp256k1_instruction_signatures: u64, // secp256k1 precompile
    num_ed25519_instruction_signatures: u64,   // ed25519 precompile
}
```

**İmza sayımı (Satır 360-386):**
```rust
pub fn get_signature_details(&self) -> TransactionSignatureDetails {
    let mut details = TransactionSignatureDetails {
        num_transaction_signatures: self.header().num_required_signatures as u64,
        ..Default::default()
    };
    for (program_id, instruction) in self.program_instructions_iter() {
        if secp256k1_program::check_id(program_id) {
            // İlk data byte'ı verify sayısını belirtir
            details.num_secp256k1_instruction_signatures += instruction.data.first().copied() as u64;
        } else if ed25519_program::check_id(program_id) {
            details.num_ed25519_instruction_signatures += instruction.data.first().copied() as u64;
        }
    }
    details
}
```

### get_durable_nonce (Satır 320-345)

```rust
pub fn get_durable_nonce(&self) -> Option<&Pubkey> {
    self.instructions()
        .get(NONCED_TX_MARKER_IX_INDEX as usize)  // İlk instruction
        .filter(|ix| system_program::check_id(program_id))
        .filter(|ix| matches!(
            limited_deserialize(&ix.data),
            Ok(SystemInstruction::AdvanceNonceAccount)
        ))
        .and_then(|ix| {
            // Nonce hesabı writable olmalı
            let idx = ix.accounts.first().copied() as usize;
            if !self.is_writable(idx) { None } else { self.account_keys().get(idx) }
        })
}
```
3 koşul: sistem programı + AdvanceNonceAccount + writable nonce hesabı.

---

## 12. LEGACY vs V0 KARŞILAŞTIRMA

| Özellik | Legacy | V0 |
|---------|--------|----|
| Struct | `legacy::Message` | `v0::Message` |
| account_keys | Tüm hesaplar | Yalnızca statik |
| Address Lookup | Yok | `address_table_lookups: Vec<MessageAddressTableLookup>` |
| Sanitized wrapper | `SanitizedMessage::Legacy(LegacyMessage)` | `SanitizedMessage::V0(LoadedMessage)` |
| Max hesap | u8 (~128 pratikte) | 256 (statik+dinamik toplam) |
| Serialization prefix | Yok (ilk byte = num_required_signatures) | `0x80` |
| Writable index | 3 bölge (header'dan) | 4 bölge (header + ALT writable counts) |
| Program ID kaynağı | Herhangi bir statik hesap | Yalnızca statik hesaplar |
| has_duplicates | O(n²) no-heap | HashSet kullanır |
| Loaded wrapper | Yok | `LoadedMessage` + `LoadedAddresses` |

---

## 13. ADRES LOOKUP TABLE (ALT) ENTEGRASYONUNun AKIŞI

```
1. Kullanıcı: v0::Message::try_compile(payer, instructions, &lookup_table_accounts, blockhash)
2. CompiledKeys::compile() → tüm pubkey'leri topla
3. Her lookup_table için:
   try_extract_table_lookup() → writable/readonly indexleri çıkar + compiled_keys'ten sil
4. Kalan statik hesaplar → try_into_message_components() → header + static account_keys
5. AccountKeys::try_compile_instructions() → instruction'ları derle
6. v0::Message { header, static_keys, instructions, address_table_lookups }

--- Ağ üzerinden gönderilir ---

7. Alıcı: VersionedMessage::deserialize() → 0x80 prefix tespiti → V0 parse
8. SanitizedVersionedMessage::try_new() → sanitize kontrolü
9. SanitizedMessage::try_new(msg, address_loader) →
   address_loader.load_addresses(&address_table_lookups) → LoadedAddresses
10. v0::LoadedMessage::new(message, loaded_addresses)
    → is_writable_account_cache hesapla
11. Runtime: SanitizedMessage üzerinden AccountKeys ile instruction'ları çalıştır
```

---

## 14. GÜVENLİK MEKANİZMALARI

### Sanitize Katmanları

1. **Format Sanitize** (`VersionedMessage::sanitize`): Index'ler ve header tutarlılığı
2. **V0 Ek Sanitize**: ALT boş değil, toplam ≤256, program ID statik
3. **Writable Demote**: Sysvar/builtin ve program ID'ler otomatik readonly
4. **Fee Payer Koruma**: index 0 her zaman writable signer, program olamaz
5. **Program ID Kısıtlaması**: V0'da program ID yalnızca statik listeden — on-chain yüklemeden analiz edilebilir

### Kritik Güvenlik Kontrolü Listesi

```
SanitizeError::IndexOutOfBounds durumları:
  ✓ header.num_required_signatures + num_readonly_unsigned > account_keys.len()
  ✓ num_readonly_signed >= num_required_signatures (fee payer yoksa)
  ✓ program_id_index == 0 (fee payer program olamaz)
  ✓ program_id_index >= account_keys.len()
  ✓ account_index >= total_account_keys (256 sınırı)
  ✓ V0: program_id_index >= num_static_account_keys (dinamik program yasak)
  ✓ V0: total_account_keys > 256

SanitizeError::InvalidValue durumları:
  ✓ V0: ALT entry'nin lookup indexleri boş (en az 1 hesap yüklemeli)
  ✓ num_readonly_signed >= num_required_signatures (V0 variant)
  ✓ num_static_account_keys == 0 (V0: en az 1 statik hesap)
```

---

## 15. NOVA SHIELD CHAIN İÇİN DERSLER

### 1. MessageHeader Tasarımı — Minimal Descriptor Pattern

Sadece 3 byte ile tüm hesap listesinin organize yapısını tarif etme. Nova Shield Chain için transaction header tasarımında benzer yaklaşım kullanılabilir:
```rust
pub struct NsMessageHeader {
    pub num_required_signatures: u8,
    pub num_readonly_signed_accounts: u8,
    pub num_readonly_unsigned_accounts: u8,
    // MPC threshold bilgisi için ek alan:
    pub mpc_threshold_signatures: Option<u8>,
}
```

### 2. İki Aşamalı Sanitizasyon

Statik + Dinamik ayrımı:
- `SanitizedVersionedMessage` — off-chain, veritabanı sorgusu gerektirmez
- `SanitizedMessage` — on-chain data ile dinamik doğrulama

Nova Shield Chain için:
- Layer 1: Statik format kontrolü (header tutarlılığı, index sınırları)
- Layer 2: MPC imza doğrulama (threshold check)
- Layer 3: ZK kanıt doğrulama (varsa)

### 3. Writable Cache Mimarisi

`is_writable_account_cache: Vec<bool>` — constructor'da bir kez hesapla, her erişimde O(1) oku. Nova Shield Chain'de de hesap durumu cache'i benzer şekilde implement edilebilir:
```rust
pub struct NsLoadedMessage {
    pub message: v0::Message,
    pub loaded_addresses: LoadedAddresses,
    pub is_writable_account_cache: Vec<bool>,
    pub mpc_signer_cache: Vec<bool>,  // MPC imza durumu cache'i
}
```

### 4. AddressLookupTable → Nova Shield Chain Address Registry

Solana ALT'ın Nova Shield Chain eşdeğeri — zincir üzerinde bilinen kontrat/hesap adreslerini referansla çek:
```rust
pub struct NsAddressRegistry {
    pub registry_account: Pubkey,
    pub mpc_validator_indexes: Vec<u8>,   // Writable validators
    pub readonly_contract_indexes: Vec<u8>, // Read-only contracts
}
```

### 5. Version Prefix Protokolü

`MESSAGE_VERSION_PREFIX = 0x80` ile geriye dönük uyumluluk. Nova Shield Chain için benzer bir scheme:
```
0x00-0x7F = Legacy format (backwards compat)
0x80 | version = Versioned format
0xFF = Off-chain / reserved
```

### 6. CompiledKeys BTreeMap Tasarımı

`BTreeMap` kullanımı determinism için kritik. Aynı instruction seti her zaman aynı account_keys sırası üretmeli. Nova Shield Chain'de transaction derleme aşamasında `BTreeMap` kullanmak, farklı validator'ların aynı mesajı üretmesini garantileyebilir.

### 7. Domain Separation in Hashing

```rust
hasher.update(b"solana-tx-message-v1");
hasher.update(message_bytes);
```
Nova Shield Chain'de tüm hash işlemlerinde domain prefix kullanılmalı:
- `b"nova-tx-message-v1"` — transaction hash
- `b"nova-mpc-share-v1"` — MPC share hash
- `b"nova-block-v1"` — block hash

### 8. Sysvar/Builtin Fast Path

`MAYBE_BUILTIN_KEY_OR_SYSVAR[key.0[0] as usize]` — 256-element bool array ile ilk byte'a göre erken çıkış. Nova Shield Chain'de reserved/system account kontrolü için aynı pattern:
```rust
static MAYBE_SYSTEM_ACCOUNT: [bool; 256] = compute_at_startup();
fn is_system_account(key: &NsPublicKey) -> bool {
    if MAYBE_SYSTEM_ACCOUNT[key.bytes[0] as usize] {
        return SYSTEM_ACCOUNTS.contains(key);
    }
    false
}
```

### 9. has_duplicates Benchmark Dersi

O(n²) vs HashSet: `~50x hızlı kısa diziler için`. Nova Shield Chain'de küçük koleksiyonlar (≤ 20 eleman) için HashSet overhead'inden kaçın. Benchmark yaz, karar ver.

### 10. Cow<'a, T> Lifecycle Yönetimi

Runtime borrow, test owned:
```rust
pub struct NsLoadedTransaction<'a> {
    pub message: Cow<'a, NsMessage>,
    pub loaded_state: Cow<'a, LoadedState>,
}
```
Runtime'da zero-copy borrow, unit test'lerde owned değer. Aynı struct her iki senaryo için çalışır.

### 11. Program ID Statik Kısıtlaması

V0'ın en önemli güvenlik kararı: program ID dinamik olarak yüklenemez. Bu:
- Static analysis mümkün kılar
- Off-chain transaction simulation yapılabilir
- Fee estimation güvenilir olur

Nova Shield Chain'de validator/program referansları da statik listeden gelmeli.

### 12. Fee Payer Tasarımı

`account_keys[0]` her zaman fee payer, her zaman writable signer, asla program olamaz. Nova Shield Chain'de benzer convention:
- `tx.accounts[0]` = fee payer
- Header'da özel bit gereksiz — pozisyon yeterli
- Sanitize: index 0 program_id_index olamaz

---

## 16. ÖZET TABLO — Tüm Public Tipler

| Tip | Tür | Dosya | Açıklama |
|-----|-----|-------|----------|
| `MessageHeader` | struct | mod.rs | 3-byte header |
| `MESSAGE_HEADER_LENGTH` | const | mod.rs | = 3 |
| `Message` (legacy) | struct | legacy.rs | Legacy format |
| `BUILTIN_PROGRAMS_KEYS` | static | legacy.rs | 10 built-in |
| `MAYBE_BUILTIN_KEY_OR_SYSVAR` | static | legacy.rs | 256-bool hızlı tablo |
| `is_builtin_key_or_sysvar` | fn | legacy.rs | İki aşamalı kontrol |
| `CompiledKeys` | struct (pub(crate)) | compiled_keys.rs | Derleme yardımcısı |
| `CompileError` | enum | compiled_keys.rs | 3 varyant |
| `AccountKeys<'a>` | struct | account_keys.rs | Static+dynamic birleşimi |
| `AddressLoaderError` | enum | address_loader.rs | 6 hata varyantı |
| `AddressLoader` | trait | address_loader.rs | load_addresses |
| `SimpleAddressLoader` | enum | address_loader.rs | Disabled/Enabled |
| `MESSAGE_VERSION_PREFIX` | const | versions/mod.rs | = 0x80 |
| `VersionedMessage` | enum | versions/mod.rs | Legacy/V0 |
| `SanitizedVersionedMessage` | struct | versions/sanitized.rs | Sanitized wrapper |
| `MessageAddressTableLookup` | struct | versions/v0/mod.rs | ALT referansı |
| `v0::Message` | struct | versions/v0/mod.rs | V0 format |
| `LoadedAddresses` | struct | versions/v0/loaded.rs | Dinamik adresler |
| `LoadedMessage<'a>` | struct | versions/v0/loaded.rs | V0 + loaded |
| `LegacyMessage<'a>` | struct | sanitized.rs | Sanitized legacy |
| `SanitizedMessage` | enum | sanitized.rs | Son kullanım tipi |
| `SanitizeMessageError` | enum | sanitized.rs | 4 varyant |
| `TransactionSignatureDetails` | struct | sanitized.rs | İmza sayıları |

---

*Agent-05 analizi tamamlandı. Toplam 9 dosya, ~1800+ satır kod analiz edildi.*
