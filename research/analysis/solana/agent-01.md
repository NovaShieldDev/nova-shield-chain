# Solana SDK Analizi — Agent 01

**Analiz Tarihi:** 2026-07-22
**Kapsam:** sdk/benches/, sdk/macro/src/lib.rs, sdk/build.rs, sdk/gen-headers/src/main.rs

---

## 1. sdk/benches/accounts.rs

**Dosya:** `/research/chains/solana/sdk/benches/accounts.rs`
**Satır Sayısı:** 153

### Genel Amaç

`AccountSharedData::set_data_from_slice` metodunun performansını farklı senaryo ve boyutlarda ölçen benchmark dosyası. Rust'ın nightly `#![feature(test)]` crate'ini kullanır.

### Özellikler (Attributes)

- Satır 1: `#![feature(test)]` — Rust nightly benchmark desteği
- Satır 2: `#![allow(clippy::arithmetic_side_effects)]` — Benchmark kodunda aritmetik taşma uyarısını susturur

### Kullanılan Tipler

- `AccountSharedData` — Solana'nın hesap verisi tipi (Arc tabanlı Copy-on-Write)
- `Pubkey` — 32-byte program/hesap adresi
- `MAX_PERMITTED_DATA_INCREASE` — Bir transaction içinde hesap boyutunun artırılabileceği maksimum miktar

### Fonksiyonlar

#### `fn bench_unchanged(bencher: &mut Bencher, size: usize)` (Satır 9–17)
- Hesap verisi değişmediğinde `set_data_from_slice`'ın maliyetini ölçer.
- `AccountSharedData::new(42, 0, &Pubkey::new_unique())` ile hesap oluşturulur, lamport=42, data_len=0.
- `account.set_data(new_data.clone())` ile ilk veri set edilir.
- Benchmark döngüsünde aynı veriyle `set_data_from_slice` çağrılır; verinin aynı olduğu durumda erken çıkış optimizasyonunu test eder.

#### `fn bench_changed(bencher: &mut Bencher, size: usize)` (Satır 19–37)
- Verinin her iterasyonda küçük bir fark içerdiği durum; 10 farklı varyant hazırlanır, `data[size / 10 * i] = 43` ile tek byte değiştirilir.
- `.iter().cycle()` kullanımı: sonsuz döngü için iterator döngüsü oluşturur.
- Gerçek dünya yazma senaryosunu simüle eder; hafıza ayırma/yeniden kullanım maliyeti ölçülür.

#### `fn bench_grow(bencher: &mut Bencher, size: usize)` (Satır 39–57)
- Veri boyutu her seferinde `MAX_PERMITTED_DATA_INCREASE` kadar artar.
- 10 farklı büyüklükte veri hazırlanır: `size + (i * MAX_PERMITTED_DATA_INCREASE)`.
- Büyüyen hesap verisi için tahsisat maliyeti ölçülür.

#### `fn bench_shrink(bencher: &mut Bencher, size: usize)` (Satır 59–77)
- `bench_grow` ile aynı veri setini kullanır ama `.rev()` ile ters sırada iterate eder — büyük'ten küçüğe gider.
- Küçülen hesap verisi için bellek serbest bırakma maliyetini ölçer.

### Benchmark Fonksiyonları (Satır 79–152)

| Fonksiyon | Senaryo | Boyut |
|---|---|---|
| `bench_set_data_from_slice_unchanged_1k` | Değişmez | 1 KB |
| `bench_set_data_from_slice_unchanged_100k` | Değişmez | 100 KB |
| `bench_set_data_from_slice_unchanged_1mb` | Değişmez | 1 MB |
| `bench_set_data_from_slice_unchanged_10mb` | Değişmez | 10 MB |
| `bench_set_data_from_slice_changed_1k` | Değişir | 1 KB |
| `bench_set_data_from_slice_changed_100k` | Değişir | 100 KB |
| `bench_set_data_from_slice_changed_1mb` | Değişir | 1 MB |
| `bench_set_data_from_slice_changed_10mb` | Değişir | 10 MB |
| `bench_set_data_from_slice_grow_1k` | Büyür | 1 KB |
| `bench_set_data_from_slice_grow_100k` | Büyür | 100 KB |
| `bench_set_data_from_slice_grow_1mb` | Büyür | 1 MB |
| `bench_set_data_from_slice_grow_10mb` | Büyür | 10 MB |
| `bench_set_data_from_slice_shrink_100k` | Küçülür | 100 KB |
| `bench_set_data_from_slice_shrink_1mb` | Küçülür | 1 MB |
| `bench_set_data_from_slice_shrink_10mb` | Küçülür | 10 MB |

### Tasarım Kararları

1. **Shrink 1KB yoktur:** Küçülme senaryosu 100 KB'dan başlar — 1 KB'lık hesapların pratikte küçülmediği varsayımı.
2. **10'lu veri varyantı + cycle():** Her iterasyonu farklı kılmak için 10 varyant + `cycle()` — deterministik ama çeşitli benchmark.
3. **MAX_PERMITTED_DATA_INCREASE kullanımı:** Büyüme testlerinde bu sabiti kullanmak, gerçek on-chain limit davranışını taklit eder.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Hesap verisi okuma/yazma operasyonları state-critical; benzer şekilde `set_data_from_slice` benzeri bir API'de değişim tespiti (dirty check) eklenerek gereksiz kopyalamalardan kaçınılmalıdır.
- Farklı boyutlarda (1KB → 10MB) benchmark yazmak, üretim ortamında karşılaşılabilecek uç durumları ortaya çıkarır.
- `cycle()` ile sonsuz iterator oluşturma benchmarklarda standart bir Rust pattern'idir; Nova Shield Chain'in kendi benchmark suite'inde kullanılmalıdır.

---

## 2. sdk/benches/slot_hashes.rs

**Dosya:** `/research/chains/solana/sdk/benches/slot_hashes.rs`
**Satır Sayısı:** 24

### Genel Amaç

`SlotHashes` sysvar'ının hesaba serileştirilmesi ve geri okunmasının (`create_account_for_test` + `from_account`) performansını ölçer.

### Özellikler

- Satır 1: `#![feature(test)]`

### Kullanılan Tipler

- `SlotHashes` — Slot → Hash eşlemesi tutan sysvar; sonuçları geçmiş slotların hash'lerini içerir.
- `Slot` — `u64` alias.
- `MAX_ENTRIES` — `SlotHashes`'in tutabileceği maksimum giriş sayısı.
- `Hash` — 32-byte SHA256 hash tipi.

### Benchmark Fonksiyonları

#### `fn bench_to_from_account(b: &mut Bencher)` (Satır 14–23)
- `MAX_ENTRIES` kadar slot-hash çifti doldurulur: `slot_hashes.add(i as Slot, Hash::default())`.
- Her iterasyonda `create_account_for_test(&slot_hashes)` ile `AccountSharedData`'ya bincode serializasyonu yapılır.
- `from_account::<SlotHashes, _>(&account).unwrap()` ile geri deserialize edilir.
- Amacı: Sysvar okuma/yazma döngüsünün toplam maliyetini ölçmek.

### Tasarım Kararları

1. **MAX_ENTRIES ile dolu tablo:** En kötü durum senaryosu — sysvar en büyük boyutunda test edilir.
2. **`Hash::default()` kullanımı:** Gerçek hash değil, sıfır dolu placeholder — kripto maliyeti devre dışı bırakılır, sadece serializasyon ölçülür.
3. **Tek benchmark:** SlotHashes için sadece bir kritik yol test edilir; kodun anlaşılması kolay, ama kısmi boyutlar test edilmez.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Sysvar benzeri global durum yapılarını hesaba serialize/deserialize ederken dikkat: büyük sysvar'lar transaction başına önemli overhead ekleyebilir.
- Nova Shield Chain'de benzer "epoch bilgisi" veya "validator set" sysvar'ları için aynı benchmark pattern uygulanmalıdır.

---

## 3. sdk/benches/serialize_instructions.rs

**Dosya:** `/research/chains/solana/sdk/benches/serialize_instructions.rs`
**Satır Sayısı:** 83

### Genel Amaç

Solana'nın instruction serializasyon yollarını karşılaştırır:
1. Bincode ile `Vec<Instruction>` serializasyon/deserializasyon
2. Manuel `construct_instructions_data` + `load_instruction_at` yolu

### Kullanılan Tipler

- `AccountMeta` — Instruction'ın referans ettiği hesap (pubkey + is_signer + is_writable)
- `Instruction` — program_id + accounts + data içeren instruction
- `Message` — Birden fazla instruction'ı birleştiren transaction mesajı
- `SanitizedMessage` — Normalize edilmiş/doğrulanmış mesaj
- `construct_instructions_data` — Sysvar instructions formatında ham byte dizisi oluşturur

### Fonksiyonlar

#### `fn make_instructions() -> Vec<Instruction>` (Satır 15–19)
- 4 adet `AccountMeta` içeren, 10 byte veri içeren, 4 adet `Instruction` oluşturur.
- Test verisi üreticisi; `pubkey::new_rand()` ile her seferinde farklı program ve hesap adresleri.

#### `fn bench_bincode_instruction_serialize(b: &mut Bencher)` (Satır 22–27)
- `serialize(&instructions).unwrap()` — bincode ile tam serializasyon.
- `test::black_box(...)` — derleyicinin sonucu optimize etmesini önler.

#### `fn bench_construct_instructions_data(b: &mut Bencher)` (Satır 30–41)
- `SanitizedMessage::try_from_legacy_message(Message::new(...))` — önce mesaj oluşturulur.
- `message.decompile_instructions()` — mesajdan instruction'lar çıkarılır.
- `construct_instructions_data(...)` — instruction sysvar formatı oluşturulur.
- Bincode'a kıyasla özel format ne kadar hızlı?

#### `fn bench_bincode_instruction_deserialize(b: &mut Bencher)` (Satır 44–50)
- Önceden serialize edilmiş byte'lardan `Vec<Instruction>` deserializasyonu.

#### `fn bench_manual_instruction_deserialize(b: &mut Bencher)` (Satır 53–67)
- Tüm instruction'ları tek tek `load_instruction_at(i, &serialized)` ile okur.
- `#[allow(deprecated)]` not: `load_instruction_at` deprecated bir API; gelecekte kaldırılacak.

#### `fn bench_manual_instruction_deserialize_single(b: &mut Bencher)` (Satır 70–82)
- Sadece 4. instruction'ı (index 3) okur — rastgele erişim maliyetini ölçer.
- Sysvar instructions formatı, belirli bir instruction'a O(n) yerine O(1) erişim sağlar mı? Bu benchmark bunu test eder.

### Tasarım Kararları

1. **Bincode vs Manuel format karşılaştırması:** Bincode genel amaçlı ama ağır; özel instruction sysvar formatı daha hafif ve tek bir instruction'a hızlı erişim sağlar.
2. **`test::black_box` kullanımı:** Tüm benchmark'larda kullanılır; derleyicinin ölçülen kodu "dead code" olarak optimize etmesini önler.
3. **Deprecated API testi:** `load_instruction_at` deprecated olmasına rağmen benchmark'ta tutuluyor — geriye dönük uyumluluk için performans karşılaştırması gerekli.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Transaction instruction'larını serialize ederken özel format geliştirmek, bincode'dan daha iyi performans sağlayabilir. Nova Shield Chain'de özellikle sık okunan sysvar'lar için özel serialization formatları değerlendirilmelidir.
- Deprecated API'leri kaldırmadan önce benchmark karşılaştırması yapmak iyi bir pratiktir — aynı anda hem eski hem yeni API'yi ölçün.

---

## 4. sdk/benches/secp256k1_instructions.rs

**Dosya:** `/research/chains/solana/sdk/benches/secp256k1_instructions.rs`
**Satır Sayısı:** 95

### Genel Amaç

Solana'nın secp256k1 precompile verification'ının farklı mesaj boyutlarında throughput'unu ölçer.

### Sabitler

- Satır 17: `const TX_COUNT: u16 = 5120` — benchmark pool'u 5K transaction

### Kullanılan Tipler

- `libsecp256k1::SecretKey` — Ethereum uyumlu secp256k1 private key
- `new_secp256k1_instruction` — Bir secp256k1 imzasını doğrulayan Solana instruction oluşturur
- `FeatureSet` — Hangi Solana özelliklerinin aktif olduğunu belirler
- `Transaction` — Solana transaction'ı
- `Keypair` / `Signer` — Ed25519 transaction imzalama

### Fonksiyonlar

#### `fn create_test_transactions(message_length: u16) -> Vec<Transaction>` (Satır 20–37)
- 5120 adet benzersiz transaction üretir.
- Her transaction için:
  - `thread_rng()` ile rastgele secp256k1 private key üretilir
  - `message_length` uzunluğunda rastgele mesaj üretilir (`rng.gen_range(0, 255)`)
  - `new_secp256k1_instruction` ile instruction oluşturulur
  - `Transaction::new_signed_with_payer` ile Ed25519 imzalı transaction oluşturulur
- Hash::default() kullanımı — gerçek blockhash yerine sıfır hash, test ortamında yeterli.

#### `fn bench_secp256k1_len_032(b: &mut Bencher)` (Satır 40–51)
- 32-byte mesaj uzunluğu — en kısa durum

#### `fn bench_secp256k1_len_256(b: &mut Bencher)` (Satır 54–65)
- 256-byte mesaj uzunluğu — ortalama kullanım

#### `fn bench_secp256k1_len_32k(b: &mut Bencher)` (Satır 68–79)
- 32 KB mesaj uzunluğu — yüksek yük

#### `fn bench_secp256k1_len_max(b: &mut Bencher)` (Satır 82–94)
- `u16::MAX - 113` byte mesaj uzunluğu — maksimum izin verilen boyut
- Satır 83: `let required_extra_space = 113_u16; // len for pubkey, sig, and offsets`
  - 113 byte = pubkey (64) + imza (64) + offset yapıları için gereken alan
  - Bu sabit, secp256k1 instruction formatının ne kadar overhead koyduğunu belgeler

### Tasarım Kararları

1. **5120 transaction pool + cycle():** Benchmark döngüsü aynı transaction'ı defalarca kullanmak yerine farklı transaction'lar üzerinden geçer; cache etkilerini azaltır, gerçek dünya throughput'unu daha iyi yansıtır.
2. **`FeatureSet::all_enabled()`:** Tüm özelliklerin açık olduğu durumda performans ölçülür — en güncel kod yolu test edilir.
3. **`rand0_7` kullanımı:** Belirli bir rand sürümüne pin — libsecp256k1'in eski rand API'sine uyum için.
4. **`required_extra_space` sabiti:** Magic number yerine açıklayıcı değişken adı + yorum ile belgelenmiş.

### Unsafe Bloklar

Yok (ancak `libsecp256k1` crate'i dahili olarak unsafe kullanır).

### Nova Shield Chain İçin Dersler

- secp256k1 precompile'ın Ethereum'dan farklı olarak Solana'da transaction seviyesinde çalıştığını ve mesaj boyutuna göre farklı performans profili sergilediğini anlamak önemli.
- Nova Shield Chain MPC (cggmp21) ve threshold imzalar kullanacak; bu benchmark pattern'i, imza doğrulama precompile'larının farklı mesaj boyutlarındaki maliyetini ölçmek için doğrudan uygulanabilir.
- `required_extra_space = 113` gibi sabit overhead değerlerini belgelemek, protokol parametrelerini tasarlarken kritik — Nova Shield Chain'de benzer overhead'ler belgelenmelidir.

---

## 5. sdk/benches/ed25519_instructions.rs

**Dosya:** `/research/chains/solana/sdk/benches/ed25519_instructions.rs`
**Satır Sayısı:** 95

### Genel Amaç

Solana'nın Ed25519 precompile verification'ının farklı mesaj boyutlarında throughput'unu ölçer. secp256k1 benchmark ile birebir aynı yapıda.

### Sabitler

- Satır 17: `const TX_COUNT: u16 = 5120`

### Kullanılan Tipler

- `ed25519_dalek::Keypair` — Ed25519 anahtar çifti (dalek kütüphanesi)
- `new_ed25519_instruction` — Ed25519 imzasını doğrulayan Solana instruction
- `FeatureSet`, `Transaction`, `Keypair`, `Signer`, `Hash` — secp256k1 benchmark ile aynı

### Fonksiyonlar

#### `fn create_test_transactions(message_length: u16) -> Vec<Transaction>` (Satır 20–37)
- secp256k1 versiyonuyla özdeş yapı; tek fark:
  - `libsecp256k1::SecretKey::random` yerine `ed25519_dalek::Keypair::generate(&mut rng)`
  - `new_secp256k1_instruction` yerine `new_ed25519_instruction(&privkey, &message)`

#### Benchmark Fonksiyonları (Satır 40–94)

| Fonksiyon | Mesaj Boyutu |
|---|---|
| `bench_ed25519_len_032` | 32 byte |
| `bench_ed25519_len_128` | 128 byte |
| `bench_ed25519_len_32k` | 32 KB |
| `bench_ed25519_len_max` | u16::MAX - 113 byte |

**Not:** secp256k1 benchmark 256-byte test içerirken Ed25519 128-byte test içerir — orta nokta seçimindeki küçük fark. Aynı `required_extra_space = 113` sabiti kullanılır.

### Tasarım Kararları

1. **secp256k1 ile structürel identiklik:** İki precompile aynı API ile benchmark edilir, bu karşılaştırma yapmayı kolaylaştırır.
2. **ed25519_dalek kullanımı:** Solana'nın transaction imzalama (solana_sdk::Keypair) ile Ed25519 precompile testi (ed25519_dalek::Keypair) için farklı kütüphaneler kullanılır. Bu iki farklı amaca hizmet eder: biri Solana transaction imzası, diğeri program içi imza doğrulaması.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Ed25519, Nova Shield Chain'in temel imzalama algoritması olacak; bu benchmark pattern'i doğruca kullanılabilir.
- secp256k1 vs Ed25519 precompile karşılaştırması: Ed25519 genellikle secp256k1'den daha hızlı; Nova Shield Chain'in imzalama kararlarında bu göz önüne alınmalıdır.

---

## 6. sdk/benches/short_vec.rs

**Dosya:** `/research/chains/solana/sdk/benches/short_vec.rs`
**Satır Sayısı:** 35

### Genel Amaç

Solana'nın özel `ShortVec` encoding'ini standart `Vec` ile karşılaştırır.

### Kullanılan Tipler

- `ShortVec<u8>` — Solana'nın kompakt length-prefixed encoding'i; uzunluğu 1-2 byte ile encode eder (< 128 için 1 byte, >= 128 için multi-byte)

### Fonksiyonlar

#### `fn create_encoded_short_vec() -> Vec<u8>` (Satır 7–11)
- 127-elemanlı ShortVec'in binary encoding'i oluşturur.
- `vec![127]` — uzunluk byte'ı (127 = 0x7F, tek byte ile ifade edilebilir)
- Ardından 127 byte veri eklenir.
- Toplam: 128 byte.

#### `fn create_encoded_vec() -> Vec<u8>` (Satır 14–18)
- 127-elemanlı standart `Vec<u8>`'in bincode encoding'i oluşturur.
- `vec![127, 0, 0, 0, 0, 0, 0, 0]` — bincode 8-byte little-endian uzunluk prefix kullanır.
- Ardından 127 byte veri eklenir.
- Toplam: 135 byte.

#### `fn bench_short_vec(b: &mut Bencher)` (Satır 21–26)
- `deserialize::<ShortVec<u8>>(&bytes)` — ShortVec deserializasyonu.

#### `fn bench_vec(b: &mut Bencher)` (Satır 29–34)
- `deserialize::<Vec<u8>>(&bytes)` — Standart Vec deserializasyonu.

### Tasarım Kararları

1. **ShortVec'in amacı:** Solana transaction'larında küçük listelerin (account list, instruction list) kompakt encoding'ini sağlar. Bir transaction'da genellikle < 128 hesap ve instruction olur, dolayısıyla 1 byte yeterli.
2. **8 byte vs 1 byte overhead:** Bincode'un 8-byte uzunluk prefix'i vs ShortVec'in 1-byte prefix'i; transaction boyutunu küçülterek daha fazla instruction sığdırır.
3. **127 seçimi:** 127 tam olarak 1-byte ShortVec encoding'inin sınırındadır — en büyük tek-byte değer.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Kompakt length encoding (ShortVec benzeri) transaction serializasyonunda önemli alan tasarrufu sağlar. Nova Shield Chain'in transaction formatı tasarlanırken değerlendirilmeli.
- Solana bu optimizasyonu bilinçli olarak seçmiş ve benchmark ile doğrulamış — benzer seçimler için benchmark destekli karar alınmalı.

---

## 7. sdk/benches/slot_history.rs

**Dosya:** `/research/chains/solana/sdk/benches/slot_history.rs`
**Satır Sayısı:** 34

### Genel Amaç

`SlotHistory` sysvar'ının iki kritik operasyonunu ölçer: hesaba serialization/deserialization ve yeni slot ekleme.

### Kullanılan Tipler

- `SlotHistory` — Hangi slotların üretildiğini tutan bir bitvector sysvar; rollback ve skip detection için kullanılır.

### Fonksiyonlar

#### `fn bench_to_from_account(b: &mut Bencher)` (Satır 13–20)
- `SlotHistory::default()` ile boş sysvar oluşturulur.
- Her iterasyonda `create_account_for_test(&slot_history)` ile serialize, `from_account::<SlotHistory, _>` ile deserialize edilir.
- `slot_hashes.rs` benchmark'ıyla karşılaştırıldığında: `SlotHistory` boş başlarken `SlotHashes` MAX_ENTRIES ile dolu başlar — farklı boyut senaryoları.

#### `fn bench_slot_history_add_new(b: &mut Bencher)` (Satır 23–33)
- Her iterasyonda 5 slot eklenir: `slot += 100_000` artışlarla.
- `100_000` artışın amacı: SlotHistory tipik olarak bitvector olarak implemente edilir; çok sık atlama (100K gap) yeni segment tahsisatına neden olabilir. Bu en kötü durumu simüle eder.
- Dışarda `let mut slot = 0` — iterasyonlar arasında state tutulur, slot monoton artar.

### Tasarım Kararları

1. **Boş başlangıç:** `bench_to_from_account` boş SlotHistory ile test eder — en iyi durum; üretimde tam dolu sysvar daha yavaş olabilir.
2. **100K gap:** Bit-vector tabanlı uygulamalarda büyük gap'ler yeni blok tahsisatını tetikler. Bu seçim kasıtlı olarak "stres testi" yapar.
3. **5 slot per iter:** Tek slot yerine 5 slot eklemek, döngü overhead'ini azaltır ve daha kararlı ölçüm sağlar.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- "Finalize edilmiş slot" takibi için bitvector tabanlı yapı verimli; Nova Shield Chain'de benzer epoch/block history sysvar'ı için bitvector (örn. `bit_vec` crate) düşünülebilir.
- Sysvar'ların "boş" ve "dolu" durumları farklı performans profillerine sahip olabilir — iki durumu da test etmek gerekir.

---

## 8. sdk/macro/src/lib.rs

**Dosya:** `/research/chains/solana/sdk/macro/src/lib.rs`
**Satır Sayısı:** 448

### Genel Amaç

Solana SDK'nın proc-macro crate'i. Programların ID'lerini declare etmek, pubkey'leri compile-time sabit olarak gömlemek ve özel derive macro'lar sağlamak için kullanılır.

### Bağımlılıklar

- `proc_macro` — Rust proc-macro API
- `proc_macro2` — proc-macro2 (daha kolay manipülasyon)
- `quote` — TokenStream üretimi için DSL
- `syn` — Rust syntax parsing
- `bs58` — Base58 decode (Solana pubkey formatı)
- `rustversion` — Compile-time Rust version kontrolü

### Struct Tanımları

#### `struct SdkPubkey(proc_macro2::TokenStream)` (Satır 99)
- `solana_sdk::pubkey::Pubkey` tipini kullanan pubkey wrapper.
- `Parse` implementasyonu: `parse_id` fonksiyonuna `::solana_sdk::pubkey::Pubkey` tipini geçer.
- `ToTokens` implementasyonu: Token stream'i olduğu gibi yayar.

#### `struct ProgramSdkPubkey(proc_macro2::TokenStream)` (Satır 114)
- `solana_program::pubkey::Pubkey` tipini kullanan pubkey wrapper.
- `solana_program` ve `solana_sdk` arasındaki ayrım: program crate'i BPF/SBF programları için, sdk crate'i client tarafı için.

#### `struct Id(proc_macro2::TokenStream)` (Satır 129)
- `declare_id!` macro'sunun parse tipi.
- `ToTokens`: `id_to_tokens` fonksiyonunu çağırır.

#### `struct IdDeprecated(proc_macro2::TokenStream)` (Satır 143)
- `declare_deprecated_id!` için — eski programların deprecated API'si.

#### `struct ProgramSdkId(proc_macro2::TokenStream)` (Satır 157)
- `program_declare_id!` için — `solana_program` kullanımı.

#### `struct ProgramSdkIdDeprecated(proc_macro2::TokenStream)` (Satır 170)
- `program_declare_deprecated_id!` için.

#### `struct RespanInput` (Satır 184–187)
```rust
struct RespanInput {
    to_respan: Path,
    respan_using: Span,
}
```
- `respan!` macro'sunun input'u.
- `to_respan`: Span'i değiştirilecek path.
- `respan_using`: Yeni span kaynağı.

#### `struct Pubkeys` (Satır 310–314)
```rust
struct Pubkeys {
    method: Ident,
    num: usize,
    pubkeys: proc_macro2::TokenStream,
}
```
- `pubkeys!` macro'sunun parse tipi.
- Tek pubkey veya pubkey array'i üretebilir.

### Fonksiyon Tanımları

#### `fn parse_id(input: ParseStream, pubkey_type: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream>` (Satır 21–38)
- `ParseStream`'den bir pubkey ID parse eder.
- İki yol:
  1. `LitStr`: base58 string literal → `parse_pubkey` ile decode edilir
  2. `Expr`: herhangi bir expression (runtime değer)
- Ekstra token varsa `syn::Error` döner.

#### `fn id_to_tokens(id, pubkey_type, tokens)` (Satır 40–67)
- `declare_id!` çıktısını üretir:
  - `pub const ID: #pubkey_type = #id;`
  - `pub fn check_id(id: &#pubkey_type) -> bool`
  - `pub const fn id() -> #pubkey_type`
  - `#[cfg(test)] fn test_id()` — otomatik test
- **Tasarım Kararı:** `check_id` ve `id()` `const` olarak implemente edilebilir olacak ama `PartialEq` derive'ı const değil; bu yorum satırında belgelenmiş (Satır 50).

#### `fn deprecated_id_to_tokens(id, pubkey_type, tokens)` (Satır 69–97)
- `declare_deprecated_id!` çıktısı:
  - `static ID` yerine `const ID` değil (eski davranış)
  - `check_id` ve `id()` `#[deprecated()]` attribute ile işaretlenir
  - `test_id` testi `#[allow(deprecated)]` ile geçirilir

#### `fn parse_pubkey(id_literal: &LitStr, pubkey_type: &proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream>` (Satır 289–308)
- Base58 string'i decode eder: `bs58::decode(id_literal.value()).into_vec()`
- 32-byte array'e dönüştürür: `<[u8; 32]>::try_from`
- Her byte'ı `LitByte` ile token haline getirir
- `Pubkey::new_from_array([b0, b1, ..., b31])` çağrısı üretir
- **Önemli:** Bu compile-time'da çalışır — program ID'ler binary'e doğrudan gömülür, runtime decode yok.

### Proc-Macro Fonksiyonları

#### `#[proc_macro] fn respan(input: TokenStream) -> TokenStream` (Satır 232–251)
- `#[rustversion::since(1.46.0)]` — `Span::resolved_at` 1.46'da stable oldu
- Bir `Path`'in span'ini başka bir token'ın span'iyle değiştirir.
- Amaç: `$crate::foo` içeren token sequence'ları doğru span'le üretmek için — hata mesajlarında doğru konum gösterilmesi.

#### `#[proc_macro] fn pubkey(input: TokenStream) -> TokenStream` (Satır 254–257)
- Tek bir pubkey'i expression olarak döner (SDK tipi).
- Kullanım: `let my_key = pubkey!("ABC123...");`

#### `#[proc_macro] fn program_pubkey(input: TokenStream) -> TokenStream` (Satır 260–263)
- `pubkey!` ile aynı ama `solana_program::pubkey::Pubkey` tipiyle.

#### `#[proc_macro] fn declare_id(input: TokenStream) -> TokenStream` (Satır 266–269)
- En çok kullanılan macro. Bir programın ID'sini modül seviyesinde declare eder:
  ```rust
  declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
  // Üretir:
  pub const ID: Pubkey = Pubkey::new_from_array([...]);
  pub fn check_id(id: &Pubkey) -> bool { id == &ID }
  pub const fn id() -> Pubkey { ID }
  #[test] fn test_id() { assert!(check_id(&id())); }
  ```

#### `#[proc_macro] fn declare_deprecated_id(input: TokenStream) -> TokenStream` (Satır 272–275)
- Deprecated programlar için; `static` ID ve deprecated fonksiyonlar üretir.

#### `#[proc_macro] fn program_declare_id(input: TokenStream) -> TokenStream` (Satır 278–281)
#### `#[proc_macro] fn program_declare_deprecated_id(input: TokenStream) -> TokenStream` (Satır 284–287)
- `solana_program` crate kullanan versiyonlar.

#### `#[proc_macro] fn pubkeys(input: TokenStream) -> TokenStream` (Satır 377–380)
- Tek pubkey veya pubkey listesi için fonksiyon üretir:
  - Tek: `pub fn method() -> Pubkey { ... }`
  - Çoklu: `pub fn method() -> Vec<Pubkey> { vec![...] }`

#### `#[proc_macro_attribute] fn wasm_bindgen_stub(_attr, item) -> TokenStream` (Satır 385–408)
- WASM için `wasm_bindgen` macro'sunun stub'ı.
- Normal `wasm_bindgen` `.bss` section oluşturur ve bu SBF programlarını bozar.
- Bu stub struct'lardan `#[wasm_bindgen]` attribute'larını temizler, diğer item'leri olduğu gibi geçirir.
- **Tasarım Kararı:** SBF (Solana BPF v2) ve WASM'ın aynı anda desteklenmesi gerektiğinde ortak bir stub kullanmak zorunlu.

#### `#[proc_macro_derive(CloneZeroed)] fn derive_clone_zeroed(input) -> TokenStream` (Satır 413–447)
- Özel `Clone` derive macro'su.
- Amaç: Struct padding byte'larını sıfırlamak.

**Neden gerekli?**
Consensus-critical struct'larda padding byte'ları belirsiz değerlere sahip olabilir. Farklı node'lar aynı struct'ı farklı padding byte'larıyla üretirse, hash'ler eşleşmez → consensus failure.

**Üretilen kod:**
```rust
impl Clone for MyStruct {
    fn clone(&self) -> Self {
        let mut value = std::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            std::ptr::write_bytes(&mut value, 0, 1); // Tüm byte'ları sıfırla (padding dahil)
            let ptr = value.as_mut_ptr();
            std::ptr::addr_of_mut!((*ptr).field1).write(self.field1); // Her field'ı yaz
            std::ptr::addr_of_mut!((*ptr).field2).write(self.field2);
            value.assume_init()
        }
    }
}
```

### Unsafe Bloklar

**`derive_clone_zeroed` içinde** (Satır 434–441):
```rust
unsafe {
    std::ptr::write_bytes(&mut value, 0, 1);
    let ptr = value.as_mut_ptr();
    #(#clone_statements)*
    value.assume_init()
}
```

**Neden unsafe?**
- `MaybeUninit` kullanımı: başlatılmamış belleğe erişim
- `ptr::write_bytes`: raw pointer üzerinden memset
- `ptr::addr_of_mut!`: field'lara raw pointer ile erişim
- `assume_init()`: "bu bellek artık başlatılmış" garantisi

Bu unsafe blok doğru kullanılmış: önce sıfırla, sonra her field'ı yaz, ardından initialize edilmiş sayıl. Padding byte'ları sıfır kalır.

**Kısıtlama:** Sadece `syn::Fields::Named` (isimli field'lar) desteklenir; tuple struct veya unit struct için `unimplemented!()`.

### Tasarım Kararları

1. **Compile-time base58 decode:** Program ID'leri binary'e doğrudan [u8; 32] array olarak gömülür. Runtime decode yok, string parsing yok → sıfır maliyet.
2. **`static` vs `const` ID:** `deprecated` versiyonlar `static`, yeniler `const`. `const` daha iyi optimizasyon sağlar ve inline edilebilir.
3. **Otomatik test üretimi:** Her `declare_id!` kullanımı kendi testini üretir (`fn test_id`). Bu ID'nin doğruluğunu garantiler.
4. **`sdk` vs `program` ayrımı:** İki farklı crate için iki farklı set macro. Client kodu `solana_sdk`, on-chain program kodu `solana_program` kullanır.
5. **`respan` macro'su:** Proc-macro'larda `$crate` resolution doğruluğu için kritik; kullanıcıya gösterilen hata mesajlarının doğru kaynak konumunu göstermesi için.
6. **Padding sıfırlama (CloneZeroed):** Consensus-critical struct'larda padding belirleyici olmalı. Bu derive macro bunu garantiler.

### Nova Shield Chain İçin Dersler

- **Program ID declare pattern'i:** Nova Shield Chain'in built-in programları için aynı `declare_id!` pattern'i uygulanmalıdır. Base58 yerine başka bir encoding kullanılsa bile compile-time decode tercih edilmeli.
- **CloneZeroed — Consensus Safety:** Consensus-critical struct'larda (block header, validator set, epoch info vb.) padding byte'larının deterministik olması zorunlu. Bu derive macro veya benzer bir mekanizma Nova Shield Chain'de kullanılmalıdır.
- **`static` vs `const` geçişi:** Eski API'leri yavaş yavaş `const`'a migrate ederken `deprecated` attribute kullanmak iyi bir geçiş stratejisi.
- **Proc-macro test üretimi:** Kritik değerlerin (örn. genesis hash, system program ID) doğruluğunu garantilemek için otomatik test üretimi değerlendirilmeli.
- **wasm_bindgen_stub fikri:** Nova Shield Chain farklı hedefler için build edilecekse (native, WASM, SBF benzeri) hedef-spesifik stub macro'lar oluşturmak faydalı.

---

## 9. sdk/build.rs

**Dosya:** `/research/chains/solana/sdk/build.rs`
**Satır Sayısı:** 23

### Genel Amaç

Rust compiler kanalını (stable/beta/nightly/dev) tespit eder ve buna göre `RUSTC_WITH_SPECIALIZATION` veya `RUSTC_WITHOUT_SPECIALIZATION` cfg flag'ini ayarlar.

### Bağımlılıklar

- `rustc_version` crate — `version_meta()` ve `Channel` enum

### Fonksiyonlar

#### `fn main()` (Satır 4–22)
- `version_meta().unwrap().channel` ile Rust compiler kanalı alınır.
- `Channel::Stable` veya `Channel::Beta` → `RUSTC_WITHOUT_SPECIALIZATION`
- `Channel::Nightly` veya `Channel::Dev` → `RUSTC_WITH_SPECIALIZATION`
- `println!("cargo:rustc-cfg=...")` ile Cargo'ya cfg flag iletilir.

### Specialization Nedir?

Rust'ın `min_specialization` feature'ı nightly'de mevcut; belirli trait implementasyonlarını daha spesifik tiplere göre özelleştirmeye izin verir. Solana SDK bunu bazı performans-kritik yerlerde kullanır. Stable'da bu feature olmadığı için fallback kod yolu kullanılır.

Bu pattern ile aynı codebase hem stable hem nightly'de derlenir; nightly'de daha optimize kod üretilir.

### Tasarım Kararları

1. **`Channel::Dev` de nightly gibi davranır:** Development build'ler de specialization'a sahip olmalı.
2. **`unwrap()` kullanımı:** Build script'te panic kabul edilebilir; build süreci zaten başarısız olur.
3. **cfg flag adları açıklayıcı:** `RUSTC_WITH_SPECIALIZATION` vs `RUSTC_WITHOUT_SPECIALIZATION` — pozitif ve negatif iki versiyon kod tarafından seçimli kullanılabilir.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Nova Shield Chain'de de nightly-only optimizasyonlar kullanılabilir. `build.rs` üzerinden channel tespiti bu geçişi yönetmenin standart yoludur.
- `cargo:rustc-cfg` ile custom cfg flag'leri, feature flag'lerine alternatif olarak build-time karar verme sağlar.
- `rustc_version` crate build dependency olarak alınmalı (`build-dependencies` bölümüne).

---

## 10. sdk/gen-headers/src/main.rs

**Dosya:** `/research/chains/solana/sdk/gen-headers/src/main.rs`
**Satır Sayısı:** 169

### Genel Amaç

Solana SBF (Solana BPF v2) programları için C header dosyaları üretir. Syscall declaration template'lerini (`@SYSCALL` annotation'lı `.inc` dosyaları) gerçek C header dosyalarına dönüştürür.

`sdk/sbf/c/inc/sol/inc/*.inc` → `sdk/sbf/c/inc/sol/*.h`

### Bağımlılıklar

- `log` — bilgi mesajları
- `regex` — `@SYSCALL` pattern eşleştirme
- `std::ffi::OsStr`, `fs`, `io`, `path::PathBuf` — dosya işlemleri

### Temel Konsept: Syscall Dispatch via Hash

SBFv2 modunda (`#ifndef SOL_SBFV2`) syscall'lar doğrudan declare edilir. `#else` (SOL_SBFV2 aktif) durumunda her syscall için:

1. Fonksiyon adının MurmurHash3-32 hash'i hesaplanır
2. Bu hash değeri fonksiyon pointer'ı olarak kullanılır
3. Pointer üzerinden syscall çağrılır

Bu mekanizma, Solana'nın SBFv2 syscall dispatch sisteminin temelidir.

### Fonksiyonlar

#### `fn main()` (Satır 24–39)
- `sdk/sbf/c/inc/sol/inc` dizinindeki tüm `.inc` dosyalarını bulur.
- Her `.inc` dosyası için `transform(&path)` çağırır.
- Dizin yoksa sessizce devam eder.

#### `fn transform(inc: &PathBuf)` (Satır 45–121)
- `.inc` → `.h` dönüşümü yapar.
- **Çıktı path:** `inc` parent'ının parent'ı + dosya adı + `.h` uzantısı.
  - Örn: `sol/inc/syscalls.inc` → `sol/syscalls.h`
- Regex ile `@SYSCALL` pattern'leri bulur:
  ```
  @SYSCALL ([0-9A-Za-z_*]+)[[:space:]]+(sol_[0-9A-Za-z_]+)\(([^);]*)\);
  ```
  - Grup 1: dönüş tipi
  - Grup 2: fonksiyon adı (`sol_` prefix zorunlu)
  - Grup 3: parametre listesi

**Üretilen çıktı (SBFv2 olmayan durumda):**
```c
/* DO NOT MODIFY... */
#ifndef SOL_SBFV2
return_type sol_function(args);
#else
typedef return_type(*sol_function_pointer_type)(args);
static return_type sol_function(arg1, arg2, ...) {
  sol_function_pointer_type sol_function_pointer = (sol_function_pointer_type) HASH;
  return sol_function_pointer(arg1, arg2, ...);
}
#endif
```

**Argüman adlandırma:** Parametre tiplerinden argüman adları `arg1`, `arg2`, ... olarak üretilir. Virgüller sayılarak argüman sırası belirlenir (Satır 91–99).

#### `const fn sys_hash(name: &str) -> usize` (Satır 123–125)
- Syscall adını `murmur3_32` ile hash'ler, `usize` döner.
- `const fn` — compile-time hesaplanabilir.

#### `#[inline(always)] const fn murmur3_32(buf: &[u8], seed: u32) -> u32` (Satır 127–161)
- MurmurHash3 32-bit implementasyonu.
- **const fn:** Compile-time çalışabilir; `unsafe` yok.
- İşlem adımları:
  1. 4-byte chunk'lar halinde işle: `pre_mix` + rotate + mix (Satır 131–136)
  2. Kalan byte'ları (1-3) işle: (Satır 139–151)
  3. Finalization: uzunluk XOR, 3 kez avalanche karıştırma (Satır 153–160)
- Magic sayılar:
  - `0xcc9e2d51`, `0x1b873593` — MurmurHash3 sabit karıştırma değerleri
  - `0xe6546b64` — MurmurHash3 add sabit
  - `0x85ebca6b`, `0xc2b2ae35` — Final karıştırma sabitleri

#### `const fn pre_mix(buf: [u8; 4]) -> u32` (Satır 163–168)
- 4-byte array'i little-endian `u32`'ye çevirir, ardından iki sabit çarpma/rotate işlemi uygular.
- `u32::from_le_bytes(buf).wrapping_mul(0xcc9e2d51).rotate_left(15).wrapping_mul(0x1b873593)`

### Tasarım Kararları

1. **`@SYSCALL` template sistemi:** Syscall declaration'ları iki kez yazılmak zorunda değil; tek `.inc` dosyasından hem doğrudan declaration hem de hash-dispatch versiyonu üretilir.
2. **Regex yerine tam parser kullanılmamış:** Basit regex yeterli; C declaration syntax'ı karmaşık değil (sadece `sol_` prefix'li fonksiyonlar hedef).
3. **Hash-based dispatch:** SBFv2'de function pointer = hash değeri. Bu yaklaşım, VM'in doğru syscall'ı bulmasını mümkün kılar; ABI olarak function index yerine name hash kullanılır.
4. **`wrapping_mul` kullanımı:** Rust'ta integer overflow by default panic; `wrapping_mul` ve `wrapping_add` ile deliberate modular arithmetic. Hash fonksiyonlarında bu zorunlu.
5. **`const fn` MurmurHash3:** Hem compile-time hem runtime kullanılabilir. Syscall ID'leri program binary'sine compile-time gömülebilir.
6. **Argüman sayımı:** `comm_re.replace_all` ile virgüller sayılarak argüman adları üretilir (Satır 91–99); bu basit ama kırılgan bir yöntem (nested function type'larında virgül varsa yanlış sayar).
7. **`BufWriter` kullanımı:** Dosya yazımı buffered; küçük dosyalarda küçük fayda ama standart pratik.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- **Hash-based syscall dispatch:** Nova Shield Chain SBF/WASM benzeri bir VM tasarlarsa, syscall'ları isim hash'iyle dispatch etmek, function table index'ine göre daha esnek ve refactor-dostu.
- **MurmurHash3 const fn:** Syscall veya opcode ID'lerini compile-time sabit olarak hesaplamak için bu implementasyon doğrudan kullanılabilir.
- **Code generation tool'ları:** `.inc` → `.h` dönüşümü gibi basit transform tool'ları; Nova Shield Chain'in C FFI veya WASM interface üretiminde benzer araç yazılabilir.
- **`wrapping_*` arithmetic:** Hash fonksiyonları ve kriptografik operasyonlarda her zaman `wrapping_mul`, `wrapping_add` kullan — overflow panic'i önler.

---

## Genel Bulgular ve Nova Shield Chain Özeti

### Mimari Insight'lar

| Konu | Solana Yaklaşımı | Nova Shield Chain Önerisi |
|---|---|---|
| Program ID | Compile-time base58 decode, `const ID: Pubkey` | Benzer pattern; compile-time gömme |
| Padding Safety | `CloneZeroed` derive macro | Consensus struct'lar için zorunlu |
| Syscall Dispatch | MurmurHash3 name hash | VM tasarımında değerlendir |
| Serialization | ShortVec compact encoding | Transaction format'ında space optimize |
| Benchmark Stratejisi | Boyut bazlı + senaryo bazlı (unchanged/changed/grow/shrink) | Aynı pattern'i uygula |
| Rust Channel Tespiti | `build.rs` + `rustc_version` | Nightly optimizasyonlar için kullan |
| Ed25519 vs secp256k1 | Her ikisi precompile olarak | Ed25519 tercih et (hız), secp256k1 Eth uyumu için |

### Kritik Kod Kalıpları

1. **`test::black_box`** — Benchmark'larda derleyici optimizasyonunu engelle
2. **`.iter().cycle()`** — Sonsuz benchmark döngüsü için
3. **`#[rustversion::since(1.46.0)]`** — Versiyona göre conditional compilation
4. **`MaybeUninit` + `write_bytes` + `assume_init`** — Zeroed padding pattern
5. **`proc_macro_input!` + `quote!`** — Solana'nın standart proc-macro pattern'i

### Güvenlik Notları

- Padding byte'larının belirsiz bırakılması consensus hatalarına yol açabilir — `CloneZeroed` bu riski ortadan kaldırır.
- Hash-based syscall dispatch, integer index'e göre daha zor manipüle edilir ama hash collision riski (teorik) mevcuttur.
- `wasm_bindgen_stub`, yanlış macro kullanımından kaynaklanacak .bss section bozulmalarını önler.
