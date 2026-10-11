# Penumbra core/keys Analizi — Agent 02

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `crates/core/keys/src/` — 20 dosya, ~1850 satir
**Agent:** 02 — Anahtar Yonetimi, Adres Sistemi, Simetrik Kriptografi

---

## GENEL BAKIS

Penumbra'nin `core/keys` crate'i, zincirin butun gizlilik katmaninin temelini olusturuyor.
Bu crate'te iki buyuk tasarim kararinin izleri acikca gorulur:

1. **Zcash Sapling'den turetilmis hiyerarsik anahtar sistemi** (SpendKey → FVK → IVK/OVK → Address).
2. **Decaf377 uzerinde R1CS constraint sistemi** — ZK proof'lari icin anahtarlar dogrudan
   constraint degiskeni (var) olarak modellenebilir.

Bu kombinasyon Penumbra'yi diger privacy chain'lerden ayiran temel ozelliktir. Harcama anahtarindan
adrese kadar her turetim adimi hem off-chain hem de ZK devresi icinde hesaplanabilir.

---

## DOSYA ANALIZI

---

## [lib.rs]

**Dosya:** `/crates/core/keys/src/lib.rs`
**Satir Sayisi:** 22

### Genel Amac

Crate'in kok modulu. Alt modulleri (address, keys, prf, symmetric, test_keys) re-export eder,
deny(clippy::unwrap_used) ile guclu unwrap yasagi koyar.

### Struct/Enum/Type Listesi

Tanimsiz — sadece `pub use` ve modul bildirimleri.

### Fonksiyonlar

#### `fn fmt_hex<T: AsRef<[u8]>>(data: T, f: &mut std::fmt::Formatter) -> std::fmt::Result` (Satir 19–21)
- Gizli/kriptografik alanlarin Debug gosterimi icin ortak yardimci.
- `hex::encode(data)` ile ham byte'lari hex stringe donusturur.
- `derivative` crate'deki `format_with = "crate::fmt_hex"` attribute ile kullanilir.

### Trait Implementasyonlari

Yok.

### Kriptografik Formuller

Yok (sadece re-export).

### Unsafe Bloklar

Yok.

### Testler

Yok.

### Tasarim Kararlari

1. `#![deny(clippy::unwrap_used)]` — butun crate'te `unwrap()` kullanimi yasak. Bu gizlilik
   sisteminin hatalarini sessizce yutmamasi icin kritik bir karar.
2. `decaf377_fmd`, `decaf377_ka`, `decaf377_rdsa` — uclunun hepsi bu crate'in temel bagimliligini
   olusturur; Penumbra'nin ozel e4 egrisinin uzerine insaa edilir.

### Nova Shield Chain Icin Dersler

- `deny(clippy::unwrap_used)` kural olarak benimsenmelidir — ozellikle anahtar ve adres isleyen
  kodlarda sessiz panic yerine `Result` zorunlulugu saglanmali.
- Kriptografi crate'leri icin `crate::fmt_hex` benzeri merkezi bir hex formatter kullanmak
  hem guvenlik (sadece istenen format) hem de tutarlilik saglayacaktir.

---

## [address.rs]

**Dosya:** `/crates/core/keys/src/address.rs`
**Satir Sayisi:** 547

### Genel Amac

Penumbra'nin odeme adresini tanimlar. Adres; diversifier, transmission key ve clue key'den olusur.
80 byte uzunlugunda, F4Jumble ile karistirilerek gorunumunu gizlestirir. Bech32m ile encode edilir.

### Struct/Enum/Type Listesi

#### `struct Address` (Satir 37–55)
```rust
pub struct Address {
    d: Diversifier,            // [u8; 16] — AES-128 ile sifrelenmis adres indeksi
    g_d: OnceLock<decaf377::Element>,  // Lazy hesaplanan diversified base point
    pk_d: ka::Public,          // [u8; 32] — Transmission key (decaf377 noktasi)
    transmission_key_s: Fq,   // pk_d'nin Fq alani temsili (not commitment icin)
    ck_d: fmd::ClueKey,        // [u8; 32] — Fuzzy Message Detection clue key
}
```
- `Serialize/Deserialize`: `pb::Address` proto ile; `inner` veya `alt_bech32m` alanlari.
- `OnceLock<decaf377::Element>`: `g_d` lazy hesaplanir ve cachelenir — gecici kaydedilmez.
- **Onemli Invariant**: `pk_d` byte'lari, gecerli bir `Fq` `s` degerinin kanonik encodelamasi
  olmalidir. Bu kural, `Address::from_components` tarafindan `Fq::deserialize_compressed` ile
  dogrulanir (Satir 118).

#### `struct NobleForwardingAddress` (Satir 252–256)
```rust
pub struct NobleForwardingAddress {
    pub channel: String,   // IBC kanal ID'si (orn. "channel-1")
    pub recipient: String, // Penumbra bech32m adresi
}
```
- IBC forwarding icin Noble chain adresini uretir.

#### `pub const ADDRESS_LEN_BYTES: usize = 80` (Satir 29)
- 16 (diversifier) + 32 (pk_d) + 32 (ck_d) = 80 byte, F4Jumble oncesi.

#### `pub const ADDRESS_NUM_CHARS_SHORT_FORM: usize = 24` (Satir 32)
- Kisaltilmis adres gosterimi icin bech32m karakter sayisi.

### Trait Implementasyonlari

#### `impl PartialEq for Address` (Satir 57–90)
- `OnceLock` icindeki `g_d` karsilastirma oncesi her iki taraf icin de `diversified_generator()`
  cagrilerek olusturulur. Bu, biri g_d'yi olusturmamis biri olusturmus iki esit adresin `!=`
  dondurmesini engeller.
- Tum alanlar karsilastirilir: `d`, `g_d`, `pk_d`, `transmission_key_s`, `ck_d`.

#### `impl PartialOrd / Ord for Address` (Satir 92–102)
- `to_vec()` (80-byte F4Jumble ciktisi) uzerinden baytsal siralama.

#### `impl Hash for Address` (Satir 104–108)
- `to_vec()` uzerinden hashleme — `HashMap<Address, _>` icin guvenli.

#### `impl Display for Address` (Satir 324–333)
- `bech32str::encode(inner, "penumbrav2", Bech32m)` ile bech32m string.

#### `impl FromStr for Address` (Satir 341–384)
- Uc farkli format desteklenir:
  1. `tpenumbra...` — transparent adres (diversifier=0, ck_d=0)
  2. `penumbracompat...` — eski bech32 (non-m) compat format
  3. `penumbrav2...` — standart bech32m format
- F4Jumble ile decode → 80 byte ayristir → `from_components`.

#### `impl TryFrom<&[u8]> for Address` (Satir 403–438)
- Tam decode sureci:
  1. Uzunluk kontrolu (80 byte)
  2. `f4jumble_inv(jumbled_bytes)` — tersi karistirmaci
  3. 16 byte diversifier + 32 byte pk_d + 32 byte ck_d okuma
  4. `Address::from_components(...)`

#### `impl DomainType / TryFrom<pb::Address> for Address` (Satir 287–322)
- Proto: `inner` (bytes) XOR `alt_bech32m` (string); ikisi ayni anda bos ya da dolu olmamali.
- `From<Address>` her zaman `inner` uretir, `alt_bech32m` bos kalir.

### Fonksiyonlar

#### `fn from_components(d: Diversifier, pk_d: ka::Public, ck_d: fmd::ClueKey) -> Option<Self>` (Satir 115–130)
- `Fq::deserialize_compressed(&pk_d.0[..])` ile `pk_d` gecerliligi dogrulanir.
- Basarisiz olursa `None` (gecersiz adres).
- `transmission_key_s: Fq` bu adimda turetilip cachelanir.

#### `fn diversified_generator(&self) -> &decaf377::Element` (Satir 141–144)
- `OnceLock::get_or_init(|| self.diversifier().diversified_generator())` — lazy hesaplama.
- Ilk cagri pahaliysa (BLAKE2b + eliptic curve encode) sonraki cagrilar ucuz.

#### `fn to_vec(&self) -> Vec<u8>` (Satir 162–175)
- Layout: `diversifier(16) || pk_d(32) || ck_d(32)` → toplam 80 byte.
- `f4jumble(bytes)` ile "karistir" — kotu gorunumlu adresler uretir, pattern tanimayı zorlaştirir.

#### `fn dummy<R: CryptoRng + Rng>(rng: &mut R) -> Self` (Satir 178–200)
- Rastgele gecerli adres uretir; `from_components` `Some` donene kadar dongude devam eder.
- Dummy adresler "note commitment" hesabinda kullanilir (gizli alici).

#### `fn display_short_form(&self) -> String` (Satir 203–209)
- `penumbrav2t1` + 24 bech32m karakteri + `...`
- UI icin okunabilir kisaltilmis format.

#### `fn encode_as_transparent_address(&self) -> Option<String>` (Satir 232–249)
- Diversifier sifir VE ck_d sifir ise gecerli transparent adres.
- Sadece `pk_d` `tpenumbra...` prefix ile encode edilir.
- Transparent adresler ZK korumasiz — tam goruntuleme imkani.

#### `fn noble_forwarding_address(&self, channel: &str) -> NobleForwardingAddress` (Satir 223–228)
- IBC forwarding adresi olusturur.

### `impl NobleForwardingAddress` — `fn bytes(&self)`  (Satir 259–272)
- `SHA256("forwarding")` → TAGGED domain separator
- `SHA256(tagged_hash || channel_bytes || recipient_bytes)[12..]` — 20 byte
- Noble chain formatini tam taklit ediyor (Go kaynak referansi satir 260 yorumunda).

### Kriptografik Formuller

**F4Jumble Karistirici (80 byte):**
```
F4Jumble(M) = f4jumble crate'te implemente edilmistir (Zcash ZIP-316'dan).
Adres byte'larini 4 tur BLAKE2b-based permutasyon ile karistirir.
Amac: Adresin sub-field'larini tek tek okunamaz yapmak.
```

**Transparent Adres Kosullari:**
```
d == [0u8; 16]  (null diversifier)
ck_d == [0u8; 32]  (identity clue key)
Yalnizca pk_d encode edilir: tpenumbra + bech32(pk_d.bytes)
```

**Noble Forwarding Hash:**
```
th   = SHA256("forwarding")
hash = SHA256(th || channel_bytes || recipient_bytes)
adres_bytes = hash[12..32]  (20 byte)
```

### Unsafe Bloklar

Yok.

### Testler

#### `test_address_encoding` (Satir 462–494)
- Tam tur testi: SeedPhrase → SpendKey → FVK → IVK → Address → bech32m string → Address
- Proto round-trip: `encode_to_vec` → `decode`
- `alt_bech32m` formati da dogrulanir.

#### `test_compat_encoding` (Satir 496–514)
- Eski bech32 (non-m) formatinin encode/decode edilebilirligini dogrular.

#### `test_bytes_roundtrip` (Satir 517–530)
- `to_vec()` → `TryFrom<Vec<u8>>` round-trip.

#### `test_address_keys_are_diversified` (Satir 532–545)
- Indeks 0 ve 1 icin turetilen adresler farkli `transmission_key`, `clue_key`, `dtk_d`'ye sahiptir.
- Diversification sisteminin calistigi: ayni IVK'den iki farkli adres.

### Tasarim Kararlari

1. **F4Jumble**: Adres byte'larinin duzenini bozarak yan-kanal pattern analizini zorlaştirir.
   Zcash ZIP-316'dan alinmis. Alt alanlari gizler, tamamen gorsel gizlilik amacli.
2. **OnceLock**: `g_d` lazy hesaplanir. Eliptik egri islemi pahali oldugundan tum adresler
   bazi kod yollarinda (orn. karsilastirma) bunu hesaplamak zorunda kalsa da onbellek kullanir.
3. **`pk_d` Fq invariant**: Not commitment `H = ...hash(pk_d_s...)` seklinde hesaplanir.
   `pk_d` byte'lari kanonik Fq temsili olmalidir, aksi halde note commitment tutarsiz olur.
4. **Transparent address**: Gizli adreslerin tam tersi — diversifier=0, ck_d=0. IBC gibi
   zincirler arasi transfer icin gerekli ama ZK gizliligi yok.
5. **Uc format destegi**: Bech32m (yeni standart), bech32 compat (eski), transparent (IBC).
   Geri uyumluluk onemli — alan kullanicilarini kaybetmemek icin.

### Nova Shield Chain Icin Dersler

- **F4Jumble'i benimsemek**: Nova Shield gizli adresleri icin benzer karistirici kullanmak,
  adres incelemesini zorlaştirabilir. ZIP-316 standardi ya da blake2b-tabanli permutasyon.
- **OnceLock ile lazy compute**: Pahali eliptik egri islemlerini lazy hesaplamak ve onbelleye almak
  Nova Shield adres yapisi icin de mantiklidir.
- **Transparent address modu**: L1 IBC-uymlu transfer icin transparent mod dusunulmeli; ama
  varsayilan her zaman gizli olmalidir.
- **`pk_d` kanoniklik denetimi**: `from_components`'ta gecersiz byte encodelamasini reddetmek
  hata satirinin erken bulunmasini saglar — bu pattern Nova Shield adres yapilanmasinda kullanilmali.

---

## [address/r1cs.rs]

**Dosya:** `/crates/core/keys/src/address/r1cs.rs`
**Satir Sayisi:** 87

### Genel Amac

`Address` tipini R1CS (Rank-1 Constraint System) devresine sokar. ZK spend proof'larinda
alici adresinin circuit icinde dogrulanabilmesi icin gereklidir.

### Struct/Enum/Type Listesi

#### `struct AddressVar` (Satir 11–15)
```rust
pub struct AddressVar {
    pub diversified_generator: ElementVar,  // g_d — decaf377 nokta degiskeni
    pub transmission_key: ElementVar,       // pk_d — decaf377 nokta degiskeni
    pub clue_key: FqVar,                    // ck_d — Fq alan degiskeni
}
```
- Diversifier `d` circuit'ta temsil edilmez (yorumda aciklanmis: Satir 72–73).
- `ElementVar`: decaf377 noktasini R1CS'te temsil eden degisken.
- `FqVar`: Fq alan elementini temsil eden degisken.

### Trait Implementasyonlari

#### `impl AllocVar<Address, Fq> for AddressVar` (Satir 31–69)
- `new_variable` metodu: `Address` degerinden `AddressVar` olusturur.
- Adim adim:
  1. `diversified_generator` = `ElementVar::new_variable(cs, || Ok(address.diversified_generator()), mode)` — eliptic curve noktasi
  2. `identity.enforce_not_equal(&diversified_generator)` — **g_d identiy olmamali** (satir 48) — Sapling'den alinmis kural
  3. `decaf377::Encoding(pk_d).vartime_decompress()` — pk_d byte'larindan decaf377 nokta
  4. `FqVar` ile clue_key = `Fq::from_le_bytes_mod_order(ck_d)` — alan elemeni

**Neden `Constant` modu uygulama disi?**
Circuit'ta adresler input veya witness olarak gelir. Sabit adres circuit'ta anlamsiz.

#### `impl ToConstraintField<Fq> for Address` (Satir 75–86)
- Adres dogrudan Fq alani elemenleri listesine donusturulur:
  1. `g_d.to_field_elements()` — nokta to field elements
  2. `pk_d.vartime_compress_to_field()` — transmission key as Fq
  3. `Fq::from_bytes_checked(ck_d)` — clue key as Fq
- Public input olarak ZK proof'una beslenir.

### Kriptografik Formuller

**R1CS Kisiti — g_d ≠ Identity:**
```
g_d_var != identity_element
Bu kisit, diversified_generator'in sifir olmayan gecerli bir eliptic curve noktasi
olmasini garanti eder. Zcash Sapling protokolunden alinmis guvenlik kisiti.
```

### Unsafe Bloklar

Yok. (`vartime_decompress` — sadece zamanlama saldirisindan korunmuyor, guvenli degilse
nonce/key icin kullanilmamali. Ancak burada pk_d public alan, guvenli.)

### Tasarim Kararlari

1. **Diversifier circuit disinda**: `d` (16 byte) circuit'a dahil edilmez. Sadece `g_d`
   (d'den turetilen nokta) devreye girer. Bu, diversifier'in gizli kalmasini saglar.
2. **`vartime_decompress`**: Yalnizca public alanlarda kullanilir (transmission key circuit icin
   sadece input, witness degil). Timing side-channel riski yok.
3. **`enforce_not_equal` for g_d**: Sapling spesifikasyonundan; kimlik noktasi kullanilirsa
   tum diversified_public() hesaplamalari ayni noktaya donusur — bu adres izlenebilirligini arttirir.

### Nova Shield Chain Icin Dersler

- Nova Shield ZK kanit sistemi (arkworks/Groth16) kullanacaksa, adres yapisini
  `AllocVar` trait'i araciligiyla R1CS'te temsil etmek ayni yaklasimdirl.
- `enforce_not_equal(identity)` kisiti: ZK devrelerinde eliptik egri noktalarini
  dogrudan constraint sistemi icinde kontrol etmek yalnizca bu sekilde mumkun.
- `ToConstraintField` implementasyonu: Public input aktariminda standart yol.

---

## [address/view.rs]

**Dosya:** `/crates/core/keys/src/address/view.rs`
**Satir Sayisi:** 253

### Genel Amac

`AddressView` enum'u: bir adresi ya "opak" (FVK olmadan) ya da "cozumlenebilir" (FVK ile birlikte
indeks ve wallet_id iceren) olarak modelleyen view katmani tipi.

### Struct/Enum/Type Listesi

#### `enum AddressView` (Satir 18–27)
```rust
pub enum AddressView {
    Opaque {
        address: Address,
    },
    Decoded {
        address: Address,
        index: AddressIndex,
        wallet_id: WalletId,
    },
}
```
- `Opaque`: Adres bilinir ama cozumlenemez (baska bir cuzdan adresi).
- `Decoded`: Adres + hangi indeks + hangi wallet — sadece FVK sahibi gorebilir.

### Trait Implementasyonlari

#### `impl AddressView` — `fn address(&self) -> Address` (Satir 30–35)
- Her iki variant'tan da ham adresi donduruyor.

#### `impl PartialOrd / Ord for AddressView` (Satir 105–172)
- Siralama kurali: `Opaque < Decoded`.
- Ayni variant icinde: adres → indeks → wallet_id siralamasiyla lexicographic.
- Deterministik JSON ve tree yapilari icin gerekli.

#### Proto donusturuculeri (Satir 42–102)
- `pb::address_view::AddressView::Opaque` / `::Decoded` varyantlariyla birebir esleme.
- Her iki protodan da bos field hatasi uretilir.

### Testler

#### `address_view_basic` (Satir 183–251)
- Iki farkli SpendKey olusturulur (sk1, sk2).
- Her ikisinin fvk'si kendi adreslerini `Decoded` olarak, karsidakini `Opaque` olarak goren.
- 4 adres x 2 FVK = 8 dogrulama.

### Tasarim Kararlari

1. **`Opaque < Decoded` siralamasinin anlami**: Gorsel olarak belirsiz adresler once gelir.
   Wallet UI'de cozumlenemez adresler basit siralama ile ayrilabilir.
2. **`Ord` implementasyonu el ile yazilmis**: derive yerine. Cunku `Address` icin `Ord`
   `to_vec()` ile yapilir, ancak `AddressView` icin `(address, index, wallet_id)` uclusu
   gerektiriyor — derive ile tek alanda yapilabilmez.
3. **FVK bilgisi olmadan `Decoded` olusturulamaz**: `fvk.view_address(addr)` olmadan
   `AddressView::Decoded` yaratmak icin gerekli `index` ve `wallet_id` alinamaz.

### Nova Shield Chain Icin Dersler

- "View layer" pattern'i: Gizli zincirde bir adresin "benim adresim mi?" kontrolu
  FVK gerekirken, adresi kendiniz gormek icin IVK yeterlidir. Bu iki durumu iki farkli
  tip ile temsil etmek — Nova Shield wallet API'si icin ideal.
- `AddressView` API'si wallet servisi icin: gondericiye adres `Opaque`, aliciya `Decoded`
  donebilir. Bu, metadata sizimasini minimize eder.

---

## [keys.rs]

**Dosya:** `/crates/core/keys/src/keys.rs`
**Satir Sayisi:** 30

### Genel Amac

`keys/` alt modullerini re-export eder. Anahtar hiyerarsisinin giris noktasi.

### Re-Export Listesi

```
diversifier::{AddressIndex, Diversifier, DiversifierKey, DIVERSIFIER_LEN_BYTES}
nullifier::{NullifierKey, NullifierKeyVar, NK_LEN_BYTES}
seed_phrase::SeedPhrase
spend::{SpendKey, SpendKeyBytes, SPENDKEY_LEN_BYTES}
bip44::Bip44Path
wallet_id::WalletId
fvk::r1cs::{AuthorizationKeyVar, RandomizedVerificationKey, SpendAuthRandomizerVar}
fvk::FullViewingKey
ivk::{IncomingViewingKey, IncomingViewingKeyVar, IVK_LEN_BYTES}
ovk::{OutgoingViewingKey, OVK_LEN_BYTES}
```

`pub(crate) use fvk::IVK_DOMAIN_SEP` — sadece crate icinde kullanilir, dis API'ye acilmaz.

### Nova Shield Chain Icin Dersler

- Tek giris noktasi ile re-export yapisini benimsemek: `keys::SpendKey`, `keys::FVK` gibi
  temiz API sunar. Nova Shield'in kendi anahtar modulu ayni sekilde organize edilmeli.

---

## [keys/bip44.rs]

**Dosya:** `/crates/core/keys/src/keys/bip44.rs`
**Satir Sayisi:** 104

### Genel Amac

BIP44 hiyerarsik deterministik turetim yolunu modeller. Penumbra'nin kayitli coin type'i: 6532.

### Struct/Enum/Type Listesi

#### `const PENUMBRA_COIN_TYPE: u32 = 6532` (Satir 3)
- SatoshiLabs SLIP-0044'te kayitli: https://github.com/satoshilabs/slips/pull/1592

#### `struct Bip44Path` (Satir 11–17)
```rust
pub struct Bip44Path {
    purpose: u32,            // BIP43: daima 44
    coin_type: u32,          // Penumbra icin 6532
    account: u32,            // Kullanici hesabi (0, 1, 2, ...)
    change: Option<u32>,     // 0=external, 1=change (genellikle None)
    address_index: Option<u32>, // Adres indeksi (genellikle None)
}
```

### Fonksiyonlar

#### `fn new(account: u32) -> Self` (Satir 21–29)
- Standart Penumbra path'ini olusturur: `m/44'/6532'/{account}'`
- `change` ve `address_index` None — Penumbra bunlari kendi diversifier sistemiyle halleder.

#### `fn new_generic(purpose, coin_type, account, change, address_index) -> Self` (Satir 32–46)
- Tam ozellestirilmis path. Test veya baska coinler icin.

#### `fn path(&self) -> String` (Satir 73–86)
- `m/44'/6532'/{account}'[/{change}[/{address_index}]]` formatinda string.
- Hardened derivation: sadece purpose, coin_type, account — apostrof ile.

### Testler

#### `test_bip44_path_full` (Satir 94–97)
- `Bip44Path::new_generic(44, 6532, 0, Some(0), Some(0))` → `"m/44'/6532'/0'/0/0"`

#### `test_bip44_path_account_level` (Satir 99–103)
- `Bip44Path::new(0)` → `"m/44'/6532'/0'"`

### Tasarim Kararlari

1. **Partial BIP44**: `change` ve `address_index` opsiyonel cunku Penumbra bu seviyeleri
   kendi `Diversifier` sistemiyle yonetiyor. BIP44'un 5 seviyesini tamamen kullanmiyor.
2. **Sadece account hardened**: `m/44'/6532'/{account}'` — purpose ve coin_type gibi
   account da hardened (apostrof'lu). Bu child key'leri parent key'den izole eder.
3. **SLIP-0044 kaydeti**: Diger cuzdan yazilimlariyla (Ledger vs.) uyumluluk. Nova Shield
   da kendi coin type'ini kaydetmeli.

### Nova Shield Chain Icin Dersler

- Nova Shield icin BIP44 coin type'i SLIP-0044'te kaydettirilmeli.
- `m/44'/NOVA_COIN_TYPE'/{account}'` standart path; Ledger ve hardware cuzdan destegi icin.
- Penumbra gibi `change` ve `address_index` seviyelerini atlayip kendi indeks sistemi kullanilabilir.

---

## [keys/diversifier.rs]

**Dosya:** `/crates/core/keys/src/keys/diversifier.rs`
**Satir Sayisi:** 318

### Genel Amac

Penumbra'nin coklu-adres sistemini implemente eder. Tek bir IVK'dan milyonlarca farkli
odeme adresi uretmek icin AES-128 tabanli deterministik sifreleme kullanilir.

### Struct/Enum/Type Listesi

#### `pub const DIVERSIFIER_LEN_BYTES: usize = 16`

#### `struct Diversifier([u8; 16])` (Satir 21–23)
- 16 byte opak blok — `AddressIndex`'in AES-128 ile sifrelenmiş hali.
- `Debug`: `fmt_hex` ile hex string.
- `diversified_generator(&self) -> decaf377::Element` (Satir 27–33):
  - BLAKE2b ile hash: `personal = b"Penumbra_Divrsfy"`, `hash(self.0)` → Fq → `encode_to_curve`
  - Her diversifier icin benzersiz, guvenlik parametrelerine gore rastgele gorunen bir base point.

#### `struct DiversifierKey([u8; 16])` (Satir 76–80)
- AES-128 anahtari — IVK'dan turetilir, diversifier<->index donusumu icin kullanilir.
- `derivative(Debug, PartialEq, Eq)` — hex formatli debug.

#### `fn diversifier_for_index(&self, index: &AddressIndex) -> Diversifier` (Satir 83–98)
- `index.to_bytes()` → 16 byte plaintext
- `Aes128::encrypt_block(plaintext)` → 16 byte ciphertext = Diversifier
- AES-128 blok sifreleme — deterministik, geri donusumlü, tek->tek esleme.

#### `fn index_for_diversifier(&self, diversifier: &Diversifier) -> AddressIndex` (Satir 100–131)
- `Aes128::decrypt_block(diversifier)` → 16 byte plaintext = AddressIndex
- **Ozel Durum** (Satir 112–117): Eger `diversifier.0 == [0u8; 16]` (null ciphertext),
  her zaman `AddressIndex { account: 0, randomizer: [0; 12] }` doner.
  Bu transparent adres sistemi icin gerekli — `d=0` her zaman hesap 0'dir.

#### `struct AddressIndex` (Satir 137–140)
```rust
pub struct AddressIndex {
    pub account: u32,         // Hesap numarasi (0, 1, 2, ...)
    pub randomizer: [u8; 12], // Ephemeral adresler icin rastgele
}
```
- Normal adresler: `randomizer = [0; 12]`, `is_ephemeral() = false`
- Ephemeral adresler: `randomizer` rastgele, `is_ephemeral() = true`

#### `fn to_bytes(&self) -> [u8; 16]` (Satir 152–157)
```
bytes[0..4]  = account.to_le_bytes()
bytes[4..16] = randomizer[12]
```

#### `fn new_ephemeral<R: RngCore + CryptoRng>(account: u32, mut rng: R) -> Self` (Satir 167–176)
- 12 byte rastgele randomizer ile ephemeral AddressIndex.
- Bu sayede ephemeral adresler harcama sonrasi tekrar kullanilmaz.

### Proto Donusumleri

- `pb::AddressIndex` ile iki yonlu:
  - `randomizer.is_empty()` → normal adres, randomizer = [0; 12]
  - Dolu randomizer → ephemeral adres

### Testler

#### `diversifier_encryption_roundtrip` (Proptest, Satir 296–305)
- Herhangi key + herhangi index icin: `diversifier_for_index(index)` → `index_for_diversifier()` = orijinal index.
- Simetrik sifreleyici dogrulandi: AES-128 encrypt/decrypt round-trip.

#### `diversifier_encryption_null_ciphertext` (Proptest, Satir 307–317)
- Herhangi key icin: `Diversifier([0; 16])` → `AddressIndex::new(0)`.
- Ozel durum (null ciphertext) testi.

### Kriptografik Formuller

**Diversifier Olusturma:**
```
diversifier = AES-128-ECB-Encrypt(DiversifierKey, AddressIndex.to_bytes())
```

**Diversified Generator (g_d):**
```
hash = BLAKE2b-512(
    personal = "Penumbra_Divrsfy",
    input = diversifier_bytes
)
g_d = decaf377_encode_to_curve(Fq::from_le_bytes_mod_order(hash.as_bytes()))
```

**Null Ciphertext Ozel Durumu:**
```
if diversifier == [0; 16] {
    return AddressIndex { account: 0, randomizer: [0; 12] }
}
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **AES-128-ECB**: Blogu dogrudan sifreler, IV yok — bu bir problem degil cunku:
   - Her `(key, index)` cifti benzersiz bir diversifier uretir.
   - Ayni key ile ayni index her zaman ayni diversifier. Deterministik ve invertible.
   - Privacy: AES ECB normalde guvenli sayilmaz cunku pattern gorunurlugu var. Ancak
     burada her blok farkli plaintext (AddressIndex) — tek blok per key per index.
2. **Randomizer ayrimi (4+12 byte)**: account=4 byte (u32), randomizer=12 byte. Bu, tek
   bir account altinda 2^96 adet ephemeral adres uretmeye izin verir.
3. **Null ciphertext ozel durumu**: Transparent adresler zero diversifier kullanir.
   Bu durumda decrypt yapmak yerine ozel durum: hesap 0. Saydam adres sistemi daha basit.

### Nova Shield Chain Icin Dersler

- **AES-128 tabanli diversifier**: Nova Shield gizli cuzdan sisteminde ayni yaklasimindan
  yararilanilabilir. Her kullanici tek bir IVK ile milyonlarca farkli adres uretebilir.
- **Ephemeral adresler**: Harcama sonrasi yeniden kullanilamayan tek kullanimlik adresler.
  CEX entegrasyonu, DEX order routing icin kritik. Nova Shield wallet bunlari desteklemeli.
- **Proptest roundtrip testleri**: Kriptografik sifreleme/cozme testleri icin property-based
  testing standart olmalidir. `proptest!` makrosu Nova Shield test suitesinde kullanilmali.

---

## [keys/fvk.rs]

**Dosya:** `/crates/core/keys/src/keys/fvk.rs`
**Satir Sayisi:** 223

### Genel Amac

`FullViewingKey` (FVK) — Penumbra'nin "tam goruntuleyici anahtar" yapisi. Harcama yetkisi
vermeksizin tum gelen/giden notlari gormeyi saglar. `SpendKey`'den deterministik turetilir.

### Struct/Enum/Type Listesi

#### `static IVK_DOMAIN_SEP: Lazy<Fq>` (Satir 23–24)
- `Fq::from_le_bytes_mod_order(b"penumbra.derive.ivk")` — IVK turetiminde domain separator.

#### `static ACCOUNT_ID_DOMAIN_SEP: Lazy<Fq>` (Satir 26–27)
- `Fq::from_le_bytes_mod_order(b"Penumbra_HashFVK")` — WalletId hash domain separator.

#### `struct FullViewingKey` (Satir 32–37)
```rust
pub struct FullViewingKey {
    ak: VerificationKey<SpendAuth>,  // Authorization key (decaf377 rdsa verify key)
    nk: NullifierKey,                // Nullifier key (Fq element)
    ovk: OutgoingViewingKey,         // Outgoing viewing key ([u8; 32])
    ivk: IncomingViewingKey,         // Incoming viewing key (ka::Secret + DiversifierKey)
}
```
- `Serialize/Deserialize`: `pb::FullViewingKey` with `inner = ak_bytes || nk_bytes (64 bytes)`.
- `Display`: bech32m ile `penumbrafullviewingkey1...` prefix.

### Fonksiyonlar

#### `fn from_components(ak: VerificationKey<SpendAuth>, nk: NullifierKey) -> Self` (Satir 76–103)
**Tam turetim zinciri:**

```
// OVK turetimi:
hash_result = prf::expand(b"Penumbra_DeriOVK", nk.0.to_bytes(), ak.as_ref())
ovk = hash_result.as_bytes()[0..32]

// DiversifierKey turetimi:
hash_result = prf::expand(b"Penumbra_DerivDK", nk.0.to_bytes(), ak.as_ref())
dk = hash_result.as_bytes()[0..16]

// IVK turetimi:
ak_s = Fq::from_bytes_checked(ak.as_ref())          // ak as Fq field element
ivk_mod_q = poseidon377::hash_2(IVK_DOMAIN_SEP, (nk.0, ak_s))  // Poseidon hash
ivk = ka::Secret::new_from_field(Fr::from_le_bytes_mod_order(ivk_mod_q.to_bytes()))
```

#### `fn wallet_id(&self) -> WalletId` (Satir 135–147)
```
hash = poseidon377::hash_2(ACCOUNT_ID_DOMAIN_SEP, (nk.0, Fq::from_le_bytes_mod_order(ak.to_bytes())))
WalletId(hash.to_bytes()[0..32])
```
Wallet ID = FVK'nin 32-byte Poseidon hash'i.

#### `fn payment_address(&self, index: AddressIndex) -> (Address, fmd::DetectionKey)` (Satir 41–43)
- Delege eder: `self.incoming().payment_address(index)`

#### `fn view_address(&self, address: Address) -> AddressView` (Satir 55–67)
- `self.incoming().views_address(&address)` -> true ise `AddressView::Decoded`
- Aksi halde `AddressView::Opaque`

#### `fn backref_key(&self) -> BackreferenceKey` (Satir 125–127)
- `BackreferenceKey::derive(self.outgoing())`

#### `fn position_metadata_key(&self) -> PositionMetadataKey` (Satir 130–132)
- DEX likidite pozisyon metadata sifreleme icin.

#### TryFrom<&[u8]> (Satir 150–168)
- 64 byte: `ak_bytes[0..32]` + `nk_bytes[32..64]`
- `nk = NullifierKey(Fq::deserialize_compressed(nk_bytes))`

#### Display / FromStr
- Bech32m ile `penumbrafullviewingkey1...` prefix.

### Kriptografik Formuller

**FVK Turetim Zinciri (SpendKey → FVK):**
```
OVK:
  BLAKE2b-512(key=nk.bytes, personal="Penumbra_DeriOVK", input=ak.bytes) → [0..32]

DiversifierKey:
  BLAKE2b-512(key=nk.bytes, personal="Penumbra_DerivDK", input=ak.bytes) → [0..16]

IVK:
  ak_s  = Fq(ak.bytes)
  ivk_q = Poseidon377_hash_2("penumbra.derive.ivk", nk.Fq, ak_s)
  ivk   = Fr::from_le_bytes_mod_order(ivk_q.to_bytes())

WalletId:
  hash  = Poseidon377_hash_2("Penumbra_HashFVK", nk.Fq, Fq(ak.bytes))
  WalletId = hash.to_bytes()[0..32]
```

**FVK Serializasyonu:**
```
inner = ak.to_bytes()[32] || nk.to_bytes()[32]  // Toplam 64 byte
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **Poseidon yerine BLAKE2b OVK/DK icin**: OVK ve DK turetimi BLAKE2b-512 kullanirken
   IVK turetimi Poseidon377 kullanir. Bunun nedeni: IVK devre icinde yeniden hesaplanacak
   (R1CS constraint), Poseidon ZK-dostu (verimli kisitlar). OVK/DK devre disinda — BLAKE2b
   daha hizli ve ucuz.
2. **Poseidon377 icin alan tabanli hash**: Decaf377'nin temel alani Fq'dur. Poseidon377
   bu alanda calisan, ZK devreleri icin optimize hash fonksiyonudur.
3. **ak, nk → FVK**: Serializasyonda yalnizca `ak || nk` (64 byte) saklanir; `ovk`, `ivk`, `dk`
   her seferinde yeniden hesaplanir. Deterministik turetim sayesinde ekstra depolama gereksiz.

### Nova Shield Chain Icin Dersler

- **ZK-dostu hash secimi**: IVK gibi devre icinde hesaplanan buyukluklerde ZK-friendly hash
  (Poseidon, Rescue) kullanilmali. Nova Shield proof sistemi arkworks/Groth16 — Poseidon dogrudan
  `poseidon377` veya `poseidon-permutation` crate'i ile entegre edilebilir.
- **Domain separator pattern'i**: Her turetim amaci icin farkli 16-byte `personal` etiketi.
  Karmasik bir sistemi parcalara ayirir, turetim cakismasini onler.
- **OVK ile outgoing gizliligi**: Gondericinin kendi gonderdigi notlari gorebilmesi icin OVK.
  Nova Shield da "gonderici gorunumu" ve "alici gorunumu" ayri kavramlar olmali.
- **WalletId = FVK hash'i**: Kullanicinin "hesap kimligi" harcama anahtariyla baglantilandirilmaz,
  sadece goruntuleyici anahtar ile. Nova Shield benzer bir wallet kimlik sistemi benimseyebilir.

---

## [keys/fvk/r1cs.rs]

**Dosya:** `/crates/core/keys/src/keys/fvk/r1cs.rs`
**Satir Sayisi:** 182

### Genel Amac

FVK'nin R1CS bilesenlerini tanimlar: `RandomizedVerificationKey`, `AuthorizationKeyVar`,
`SpendAuthRandomizerVar`. Spend proof devresinde harcama yetkisinin ZK olarak dogrulanmasi icin.

### Struct/Enum/Type Listesi

#### `static SPENDAUTH_BASEPOINT: Lazy<Element>` (Satir 14)
- `Element::GENERATOR` — decaf377'nin sabit uretec noktasi, Spend authorization icin kullanilir.

#### `struct RandomizedVerificationKey` (Satir 16–18)
```rust
pub struct RandomizedVerificationKey {
    pub inner: ElementVar,  // Rastgelestirilmis dogrulama anahtari (decaf377 nokta)
}
```

#### `impl AllocVar<VerificationKey<SpendAuth>, Fq> for RandomizedVerificationKey` (Satir 20–41)
- Sadece `AllocationMode::Input` desteklenir (circuit'a dis girdi).
- `decaf377::Encoding(*inner.as_ref()).vartime_decompress()` ile byte'lardan nokta.
- `AllocationMode::Constant / Witness` — `unimplemented!()`.

#### `impl R1CSVar<Fq> for RandomizedVerificationKey` (Satir 43–59)
- `value()`: nokta → bytes → `VerificationKeyBytes<SpendAuth>` → `VerificationKey`

#### `impl RandomizedVerificationKey` — `fn compress_to_field(&self)` (Satir 61–64)
- `ElementVar::compress_to_field()` — nokta → Fq, kisit sistemi icinde.

#### `impl EqGadget<Fq> for RandomizedVerificationKey` (Satir 67–73)
- Her iki key de `compress_to_field()` ile Fq'ya indirgenir, `is_eq` ile karsilastirilir.
- Nokta karsilastirmasini alan elemani karsilastirmasina donusturur — R1CS'te daha verimli.

#### `struct AuthorizationKeyVar` (Satir 75–77)
```rust
pub struct AuthorizationKeyVar {
    pub inner: ElementVar,  // Harcama yetkisi dogrulama anahtari (decaf377 nokta)
}
```

#### `impl AllocVar<VerificationKey<SpendAuth>, Fq> for AuthorizationKeyVar` (Satir 79–107)
- Sadece `AllocationMode::Witness` — circuit icinde gizli bilgi olarak.
- **Kritik Kisit**: `identity.enforce_not_equal(&ak_element_var)` (Satir 99–100)
  Harcama anahtari kimlik noktasi olamaz — Sapling guvenlik kisiti.

#### `impl AuthorizationKeyVar` — `fn randomize()` (Satir 128–138)
**Spend Randomization:**
```
rk = ak + r * G  (r = SpendAuthRandomizer, G = SPENDAUTH_BASEPOINT)
```
- `self.inner + spend_auth_basepoint_var.scalar_mul_le(randomizer.bits())` — R1CS icinde.
- `RandomizedVerificationKey` donduruluyor.

#### `struct SpendAuthRandomizerVar` (Satir 141–143)
```rust
pub struct SpendAuthRandomizerVar {
    inner: Vec<UInt8<Fq>>,  // Fr skaleri, byte vector olarak
}
```

#### `impl AllocVar<Fr, Fq> for SpendAuthRandomizerVar` (Satir 145–165)
- Sadece `AllocationMode::Witness`.
- `Fr.to_bytes()` → 32 byte → `UInt8::new_witness_vec(cs, bytes)`.

#### `impl R1CSVar<Fq> for SpendAuthRandomizerVar` (Satir 167–181)
- `value()`: byte vector → `[u8; 32]` → `Fr::from_bytes_checked`.

### Kriptografik Formuller

**Spend Randomization (R1CS icinde):**
```
rk = ak + r * G
burada:
  ak  = AuthorizationKeyVar.inner (decaf377 Element)
  r   = SpendAuthRandomizerVar (Fr scalar, bit decomposed)
  G   = SPENDAUTH_BASEPOINT = decaf377::Element::GENERATOR
  rk  = RandomizedVerificationKey.inner
```

**ZK Kisit — ak ≠ Identity:**
```
enforce_not_equal(ak_element_var, identity_element)
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **Spend randomization gizliligi**: Her harcamada `ak` randomize edilerek `rk` olusturulur.
   Blok zincirinde `rk` gorunur, `ak` gizlidir. Ayni `ak`'dan farkli `rk`'lar, harcama
   baglantisini koparir — linking saldirisi onlenir.
2. **`compress_to_field` ile EqGadget**: Eliptic curve noktalarini R1CS'te karsilastirmak
   maliyetli. Alan elemanlarina komprese edip karsilastirmak cok daha verimli.
3. **Sadece Input/Witness ayrimi**: `RandomizedVerificationKey` — public input (verifier gorur).
   `AuthorizationKeyVar` — witness (sadece prover bilir).

### Nova Shield Chain Icin Dersler

- **Spend randomization**: Nova Shield gizli harcama sisteminde her harcamanin degisik imza
  goruntulenmesi icin bu pattern kritiktir. `rk = ak + r*G` formulunu Nova Shield spend
  proof'unda aynen uygulayabiliriz (arkworks/Groth16).
- **R1CS'te scalar_mul_le**: Scalar carpim bit decomposition ile yapilir. Bu, Nova Shield'in
  proof devresinde eliptic curve islemlerini dogru implemente etmek icin gerekli teknik.
- **Constant / Input / Witness modlari**: AllocVar implementasyonunda hangi modun desteklenecegi
  protocol semantigine gore belirlenmeli — her mod farkli anlamda.

---

## [keys/ivk.rs]

**Dosya:** `/crates/core/keys/src/keys/ivk.rs`
**Satir Sayisi:** 320

### Genel Amac

`IncomingViewingKey` (IVK) — gelen notlari goren anahtar. Harcama yetkisi vermez.
Adres turetimi, adres tanimasi ve key agreement icin kullanilir.

### Struct/Enum/Type Listesi

#### `pub const IVK_LEN_BYTES: usize = 64`
#### `const MOD_R_QUOTIENT: usize = 4` — Fq'dan Fr'ye indirgeme carpeni

#### `struct IncomingViewingKey` (Satir 24–27)
```rust
pub struct IncomingViewingKey {
    pub(super) ivk: ka::Secret,       // Key agreement skreti (Fr skaleri)
    pub(super) dk: DiversifierKey,    // AES-128 anahtari (diversifier sifreleme icin)
}
```

### Fonksiyonlar

#### `fn payment_address(&self, index: AddressIndex) -> (Address, fmd::DetectionKey)` (Satir 31–45)
**Adres turetim sureci:**
```
d    = dk.diversifier_for_index(index)      // AES-128: index → diversifier
g_d  = d.diversified_generator()            // BLAKE2b + encode_to_curve
pk_d = ivk.diversified_public(g_d)          // pk_d = ivk * g_d (Eliptik carpim)

dtk_d = DetectionKey::from_field(
    Fr::from_le_bytes_mod_order(
        prf::expand(b"PenumbraExpndFMD", ivk.to_bytes(), d.as_ref()).as_bytes()
    )
)
ck_d = dtk_d.clue_key()

Address = { d, pk_d, ck_d }
```

#### `fn transparent_address(&self) -> String` (Satir 51–64)
- `Diversifier([0; 16])` (null diversifier) + `ck_id = ClueKey([0; 32])` ile adres olusturur.
- Sadece string doner — `Address` dondurmek guvenli degil (truncation riski yorumu, Satir 47–49).

#### `fn ephemeral_address<R: RngCore + CryptoRng>(&self, mut rng, mut address_index) -> (Address, DetectionKey)` (Satir 67–79)
- `address_index.randomizer` = 12 byte rastgele.
- Sonra `payment_address(modified_index)` — ephemeral adres.

#### `fn key_agreement_with(&self, pk: &ka::Public) -> Result<ka::SharedSecret, ka::Error>` (Satir 82–84)
- `self.ivk.key_agreement_with(pk)` — ECDH benzeri Diffie-Hellman, decaf377 uzerinde.

#### `fn views_address(&self, address: &Address) -> bool` (Satir 98–100)
```
ivk.diversified_public(address.diversified_generator()) == *address.transmission_key()
```
- Adresin `pk_d`'si `ivk * g_d` ile eslesiyor mu? Eslesiyorsa bu IVK adresi gorebilir.

### `struct IncomingViewingKeyVar` (Satir 114–116)
R1CS constraint degiskeni.

#### `fn derive(nk: &NullifierKeyVar, ak: &AuthorizationKeyVar) -> Result<Self>` (Satir 120–188)
**Devre icinde IVK turetimi — en karmasik R1CS metodu:**

```
// 1. Poseidon hash (devre icinde):
ivk_domain_sep = FqVar::new_constant(IVK_DOMAIN_SEP)
ivk_mod_q = Poseidon377_hash_2(ivk_domain_sep, (nk.inner, ak.inner.compress_to_field()))

// 2. Fq → Fr indirgeme (Out-of-Circuit / OOC):
r_modulus: Fq = Fq::from(Fr::MODULUS)
ivk_mod_r_ooc = Fr::from_le_bytes_mod_order(ivk_mod_q.bytes())

// 3. Kac kez cikarma yapildi? (OOC):
a = ivk_mod_q / r_modulus  (tamsayi bolumu, 0..=4)

// 4. Devre icinde kisat: ivk_mod_q = r * a + ivk_mod_r
rhs = mod_r_var * a_var + ivk_mod_r
ivk_mod_q.enforce_equal(rhs)

// 5. a ∈ {0,1,2,3,4} kisiti (polinom):
a(a-1)(a-2)(a-3)(a-4) = 0
```

**Neden `a <= 4`?**
`Fq` modulus buyuklugu `Fr` modulus buyuklugunun yaklasik `4*Fr + r` kadaridir.
Yani `ivk_mod_q / Fr::MODULUS` en fazla 4 olabilir. Test bunu kantilar: `MOD_R_QUOTIENT = 4`.

#### `fn diversified_public(&self, diversified_generator: &ElementVar) -> Result<ElementVar>` (Satir 191–197)
```
ivk_bits = ivk_mod_r.to_bits_le()
pk_d_var = diversified_generator.scalar_mul_le(ivk_bits.iter())
```
R1CS icinde `pk_d = ivk * g_d` hesaplanir.

### Testler

#### `transparent_address_generation_and_parsing` (Satir 212–246)
- Transparent adres string olarak uretilir, `Address` olarak parse edilir.
- `ivk.views_address(reconstructed)` → true.
- `reconstructed.diversifier() != actual_address.diversifier()` — null diversifier,
  normal adres diversifier'dan farklidir.

#### `views_address_succeeds_on_own_address` (Satir 249–257)
- IVK kendi adreslerini tanimadir.

#### `views_address_fails_on_other_address` (Satir 274–289)
- Baska IVK'nin adresi taninamaz.

#### `enforce_field_assumptions` (Satir 292–319)
- `fq_modulus > fr_modulus` dogrulanir.
- `(fq_modulus - 1) / fr_modulus == 4` (MOD_R_QUOTIENT) numerik olarak dogrulanir.
- Circuit varsayimlari matematiksel olarak test edilmis.

### Kriptografik Formuller

**Adres Turetimi:**
```
d    = AES-128-ECB(dk, index.to_bytes())
g_d  = BLAKE2b("Penumbra_Divrsfy", d) → Fq → encode_to_curve
pk_d = ivk * g_d          (decaf377 scalar multiplication)
dtk  = BLAKE2b-512("PenumbraExpndFMD", key=ivk.bytes, input=d.bytes) → Fr
ck_d = dtk.clue_key()     (FMD clue key from detection key)
```

**IVK Devre Icinde Turetimi (R1CS):**
```
ivk_q = Poseidon("penumbra.derive.ivk", nk.Fq, ak_s.Fq)
a     = floor(ivk_q / Fr::MODULUS)   // 0 <= a <= 4
ivk_r = ivk_q - a * Fr::MODULUS      // ivk mod Fr
Kisitlar:
  a(a-1)(a-2)(a-3)(a-4) = 0           // a ∈ {0,1,2,3,4}
  ivk_r < Fr::MODULUS                 // ivk_r gecerli bir Fr degeri
  eger a=4: ivk_r < Fq - 4*Fr        // tasma kontrolu
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **Fq/Fr modulus indirgeme devresinde**: Poseidon hashi `Fq` alani verir, IVK `Fr` alani
   gerektirir. `Fq > Fr` oldugunda indirgeme gerekli. OOC hesaplama + devre kisiti pattern'i
   burada en guzel ornegi: devre "a kadar cikardin mi?" diye soruyor, prover kanit sagliyor.
2. **Polinom kisit `a(a-1)...(a-4)=0`**: `enforce_cmp` yerine polinom carpimi — 4 carpim,
   5 toplama, yaklasik 5 R1CS satiri. `enforce_cmp` ile gereken kisit sayisindan daha az.
3. **IVK turetiminde TransparentAddress string donuyor**: `Address` degil. Kod yorumunda
   aciklaniyor: arbitrary adresi truncate etmek guvenli degil, bu ozel durum.
4. **FMD clue key turetimi**: `dtk_d = PRF(ivk, d)` — her adres icin benzersiz clue key.
   FMD (Fuzzy Message Detection) alicinin mesajlarini ozel olarak taramamasini saglar.

### Nova Shield Chain Icin Dersler

- **IVK'nin iki kullanimi**: Adres turetimi (g_d ile scalar mul) + Key agreement (ECDH analog).
  Nova Shield'da benzer ayrim: goren anahtar hem adres turetmeli hem de not sifresini acmali.
- **ZK devre icinde modular aritmetik**: Fq → Fr indirgeme kisiti, `Poseidon + OOC + enforce_equal`
  pattern'i, Nova Shield proof sistemine dogrudan aktarilabilir.
- **Polinom kisit pattern'i**: `a ∈ {0,...,4}` icin `a(a-1)...(a-4)=0` — range constraint'e
  alternatif, Nova Shield devrelerinde kullanilabilir.

---

## [keys/nullifier.rs]

**Dosya:** `/crates/core/keys/src/keys/nullifier.rs`
**Satir Sayisi:** 49

### Genel Amac

`NullifierKey` (NK) ve `NullifierKeyVar` tiplerini tanimlar. NK, harcanan notlar icin benzersiz
"nullifier" degeri uretmek icin kullanilir.

### Struct/Enum/Type Listesi

#### `pub const NK_LEN_BYTES: usize = 32`

#### `struct NullifierKey(pub Fq)` (Satir 11)
- Tek Fq alan elemani — 32 byte.
- `pub` — SpendKey'den dogrudan erisim.

#### `struct NullifierKeyVar { pub inner: FqVar }` (Satir 14–16)
- R1CS degiskeni: Fq alan elemani olarak NK.

### Trait Implementasyonlari

#### `impl AllocVar<NullifierKey, Fq> for NullifierKeyVar` (Satir 18–35)
- Sadece `AllocationMode::Witness` desteklenir.
- NK gizli bilgi — prover bilir, verifier bilmez.
- `FqVar::new_witness(cs, || Ok(inner.0))`

#### `impl R1CSVar<Fq> for NullifierKeyVar` (Satir 37–48)
- `value()`: `FqVar` → `Fq` → `NullifierKey(inner_fq)`.

### Tasarim Kararlari

1. **NK'nin Fq olmasi**: Nullifier hesabi `poseidon377::hash_3(NK, position, note_commitment)`
   gibi bir formule dayanir. Poseidon `Fq` alani uzerinde calisir — NK'nin Fq olmasi dogal.
2. **Sadece Witness**: NK hi bir zaman public input degil. ZK kanitinda: prover NK'yi bilir,
   kisitlar nullifier hesabini dogrular.

### Nova Shield Chain Icin Dersler

- **Nullifier sistemi**: Harcanmis notlarin "double spend" olmamasi icin global nullifier seti.
  Nova Shield da `NK + note_position + note_commitment → Nullifier` formulunu Poseidon ile
  implemente edebilir.
- **NK'yi ZK'de tutmak**: NK privacy-critical, hicbir zaman public olmamali. Sadece Witness.

---

## [keys/ovk.rs]

**Dosya:** `/crates/core/keys/src/keys/ovk.rs`
**Satir Sayisi:** 13

### Genel Amac

`OutgoingViewingKey` (OVK) — giden notlari goren anahtar. Sadece 32 byte ham veri.

### Struct/Enum/Type Listesi

#### `pub const OVK_LEN_BYTES: usize = 32`

#### `struct OutgoingViewingKey(pub(crate) [u8; 32])` (Satir 6)
- Crate-private icerik — sadece `to_bytes()` ile erisim.
- Herhangi bir ZK devre tipi yok — OVK devre disinda kullanilir.

### Fonksiyonlar

#### `fn to_bytes(&self) -> [u8; OVK_LEN_BYTES]` (Satir 9–11)
- `self.0` kopyasi.

### Tasarim Kararlari

- OVK maksimum basitligi ile sadece 32 byte. `BackreferenceKey` ve `PositionMetadataKey`
  bu key'den BLAKE2b ile turetilir.
- Hicbir `pub` alan — kask olmaz. Sadece `to_bytes()` ile iceri erisim.

---

## [keys/seed_phrase.rs]

**Dosya:** `/crates/core/keys/src/keys/seed_phrase.rs`
**Satir Sayisi:** 332

### Genel Amac

BIP39 uyumlu anma cumlesini (mnemonic seed phrase) implemente eder. 12 veya 24 kelimelik
seed phrase olusturma, dogrulama ve string donusumunu saglar.

### Struct/Enum/Type Listesi

#### Sabitler
```rust
pub const NUM_PBKDF2_ROUNDS: u32 = 2048;
pub const NUM_WORDS_SHORT: usize = 12;
pub const NUM_WORDS_LONG: usize = 24;
pub const NUM_ENTROPY_BITS_SHORT: usize = 128;
pub const NUM_ENTROPY_BITS_LONG: usize = 256;
pub const NUM_BITS_PER_WORD: usize = 11;  // 2^11 = 2048 kelime
pub const NUM_BITS_PER_BYTE: usize = 8;
```

#### `enum SeedPhraseType` (Satir 21–25)
- `MinimumLength` (12 kelime, 128 bit entropi)
- `MaximumLength` (24 kelime, 256 bit entropi)

#### `struct SeedPhrase(pub Vec<String>)` (Satir 72)
- 12 veya 24 kucuk harfli Ingilizce kelime listesi.

### Fonksiyonlar

#### `fn generate<R: RngCore + CryptoRng>(mut rng: R) -> Self` (Satir 76–80)
- 32 byte (256 bit) rastgele → `from_randomness`.

#### `fn short_generate<R: RngCore + CryptoRng>(mut rng: R) -> Self` (Satir 83–87)
- 16 byte (128 bit) rastgele → `from_randomness`.

#### `fn from_randomness(randomness: &[u8]) -> Self` (Satir 90–124)
**BIP39 Uretim Algoritmasi:**
```
1. randomness bit'lerini bool array'e ayir (MSB first)
2. SHA256(randomness) → checksum:
   - 24-kelime: 8 bit checksum
   - 12-kelime: 4 bit checksum
3. checksum bit'lerini entropi bit'lerinin sonuna ekle
4. Toplam bit'leri 11'lik gruplara bol → kelime indeksleri (0..2047)
5. Her indeks icin BIP39_WORDS[index] secilir
```

#### `fn verify_checksum(&self) -> anyhow::Result<()>` (Satir 132–173)
- Ters islem: kelimeler → indeksler → bit'ler → entropy bytes + expected checksum.
- SHA256(entropy) hesaplanir, beklenen checksum ile karsilastirilir.

#### `fn convert_bits_to_usize(bits: &[bool]) -> usize` (Satir 212–217)
- bool array'den integer'a: `bits[i] ? 1 << (len-1-i) : 0` toplami.

### Testler

#### `bip39_mnemonic_derivation` (Satir 228–308)
- 16 Trezor test vektoru (hem 12 hem 24 kelime).
- Randomness → expected_phrase → SHA512-PBKDF2 sonucu dogrulanir.
- **PBKDF2 testi**: Her kelime listesi icin `HMAC-SHA512` ile `2048` yineleme,
  salt=`mnemonicTREZOR`, 64 byte cikti. Trezor referans degerlerine esit olmali.

#### `seed_phrase_from_str` (Satir 311–330)
- Gecersiz (too short, invalid word, wrong checksum) → `Err`.
- Gecerli (24 kelime, buyuk/kucuk harf karisik) → `Ok`.

### Kriptografik Formuller

**BIP39 Mnemonic Olusturma:**
```
entropy   = randomness (128 veya 256 bit)
checksum  = SHA256(entropy)[0..N/32 bit]
  N=128 → 4 bit checksum
  N=256 → 8 bit checksum
bits      = entropy_bits || checksum_bits
words[i]  = BIP39_WORDS[bits[i*11..(i+1)*11] as usize]
```

**BIP39 → PBKDF2 Seed (SpendKey derivation icin):**
```
seed = PBKDF2-HMAC-SHA512(
    password = "word1 word2 ... word24",
    salt     = "mnemonic" [+ optional_passphrase],
    iterations = 2048,
    output_length = 32 or 64 bytes
)
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **PBKDF2 rounds = 2048 (sadece)**: BIP39 standardi 2048 round. Guvensiz cunku modern
   standart 100k+ round oneriliyor. Ancak hardware cuzdanlar (Ledger) bu parametreyi bekliyor.
   Degistirmek uyumsuzluga yol acar.
2. **lowercase input**: `FromStr`'de `to_lowercase()` — buyuk/kucuk harf karisik seed
   phrase'ler gecerli. Kullanici dostu.
3. **MSB first bit sirasi**: BIP39 standard gereği — bit 0 = en onemli bit.
4. **Sadece 12 veya 24**: Diger uzunluklar BIP39'da desteklenmez, `from_length` hata uretir.

### Nova Shield Chain Icin Dersler

- **BIP39 tam uyumlulugu**: Trezor test vektorleri ile dogrulama standart olmalidir.
  Nova Shield wallet, kullanicilarinin seed phrase'lerini diger cuzdanlarla paylasabilmesi
  icin BIP39 tam uyumlu olmali.
- **PBKDF2 parametre dokumantasyonu**: 2048 round'un neden az oldugunu belgelemek ve
  hardware cuzdan uyumlulugunun bu tradeoff'u neden zorunlu kildigini aciklamak gerekir.
- **Checksum dogrulama**: `FromStr`'de otomatik checksum kontrolu — kullanici yazim hatasini
  erken yakala.

---

## [keys/seed_phrase/words.rs]

**Dosya:** `/crates/core/keys/src/keys/seed_phrase/words.rs`
**Satir Sayisi:** 217

### Genel Amac

BIP39 standart Ingilizce 2048-kelime listesi — kaynak: `bips/bip-0039/english.txt`.

### Struct/Enum/Type Listesi

#### `pub const BIP39_WORDS: [&str; 2048]` (Satir 4)
- Compile-time sabit dizisi — runtime alokasyon yok.
- Her kelime lowercase, 3-8 karakter arasi.
- Ilk 3 harf her kelimede benzersiz (BIP39 gerekliligi).

### Tasarim Kararlari

1. **`const` array**: Compile-time sabit — `&'static str` slice'lari, sifir maliyet.
2. **2048 kelime = 11 bit**: Her kelime 0..2047 indeksi kodlar. 11 bit tam olarak BIP39 kelime sayisini kapsayacak sekilde secilmis.
3. **Kelime listesi degismemeli**: Mevcut seed phrase'lerin gelecekte de calisabilmesi icin
   bu liste asla degistirilmemelidir. Nova Shield kendi ozellestirilmis listesi olusturmamali.

---

## [keys/spend.rs]

**Dosya:** `/crates/core/keys/src/keys/spend.rs`
**Satir Sayisi:** 219

### Genel Amac

`SpendKey` — Penumbra'nin en yuksek yetkili anahtari. Harcama imzalama, FVK turetimi,
ve BIP39/BIP44 ile seed phrase'den deterministik turetim.

### Struct/Enum/Type Listesi

#### `pub const SPENDKEY_LEN_BYTES: usize = 32`

#### `struct SpendKeyBytes(pub [u8; 32])` (Satir 28)
- Ham 32 byte spend seed — serializasyon icin.

#### `struct SpendKey` (Satir 33–37)
```rust
pub struct SpendKey {
    seed: SpendKeyBytes,          // 32 byte ham seed
    ask: SigningKey<SpendAuth>,   // rdsa spend authorization signing key
    fvk: FullViewingKey,          // Onceden hesaplanmis FVK
}
```
- `PartialEq`: sadece `seed` karsilastiriliyor.
- Serde: `pb::SpendKey` ile.

### Fonksiyonlar

#### `impl From<SpendKeyBytes> for SpendKey` (Satir 67–75)
**Ana turetim zinciri:**
```rust
ask = SigningKey::new_from_field(
    prf::expand_ff(b"Penumbra_ExpndSd", &seed.0, &[0; 1])  // label[0]=0 → ask
)
nk  = NullifierKey(
    prf::expand_ff(b"Penumbra_ExpndSd", &seed.0, &[1; 1])  // label[0]=1 → nk
)
fvk = FullViewingKey::from_components(ask.into(), nk)
```

**Kriptografik Formul:**
```
ask (Fr skaleri) = BLAKE2b-512("Penumbra_ExpndSd", key=seed, input=[0]) mod r
nk  (Fq element) = BLAKE2b-512("Penumbra_ExpndSd", key=seed, input=[1]) mod q
```

#### `fn from_seed_phrase_bip39(seed_phrase: SeedPhrase, index: u64) -> Self` (Satir 93–105)
```
password = "word1 word2 ... word24"
salt     = "mnemonic{index}"
seed_bytes = PBKDF2-HMAC-SHA512(password, salt, 2048, 32 bytes)
SpendKeyBytes(seed_bytes).into()
```

#### `fn from_seed_phrase_bip44(seed_phrase: SeedPhrase, path: &Bip44Path) -> Self` (Satir 107–129)
```
password = "word1 word2 ... word24"
salt     = "mnemonic"
seed_bytes = PBKDF2-HMAC-SHA512(password, salt, 2048, 64 bytes)
child_key = BIP32_XPrv::derive_from_path(seed_bytes, path)
SpendKeyBytes(child_key.to_bytes()[0..32]).into()
```

**BIP39 vs BIP44 farki:**
- BIP39: salt=`mnemonic{index}`, 32 byte cikti, tek hesap.
- BIP44: salt=`mnemonic`, 64 byte master seed → BIP32 child key, path=`m/44'/6532'/{account}'`.

### Testler

#### `bip44_test_ledger` (Satir 204–218)
- Belirli seed phrase + path=`m/44'/6532'/0'` → beklenen 32-byte spend key dogrulanir.
- Ledger uygulama test vektoru ile eslesiyor.

### Kriptografik Formuller

**SpendKey → ask, nk:**
```
ask = BLAKE2b-512(personal="Penumbra_ExpndSd", key=seed_bytes, input=[0x00]) as Fr
nk  = BLAKE2b-512(personal="Penumbra_ExpndSd", key=seed_bytes, input=[0x01]) as Fq
```

**BIP44 → SpendKey:**
```
master_seed = PBKDF2-HMAC-SHA512(mnemonic_phrase, "mnemonic", 2048, 64 bytes)
child_xprv  = BIP32_derive(master_seed, "m/44'/6532'/{account}'")
spend_seed  = child_xprv.to_bytes()[0..32]
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **Input byte [0] vs [1]**: ask ve nk ayni seed'den farkli "input" byte'i ile turetilir.
   Basit ama etkili domain separation — ayni PRF, farkli output.
2. **FVK onceden hesaplanir**: `SpendKey::from(SpendKeyBytes)` icinde FVK hemen hesaplanir,
   her `fvk()` cagrisi tekrar turetmez. Cahce'li.
3. **BIP39 ve BIP44 iki ayri metot**: Eski cuzdanlar BIP39 kullaniyor (index ile farkli hesaplar),
   yeniler BIP44. Geriye uyumluluk korunmus.
4. **SerDe icin sadece seed**: `SpendKey`'i sadece 32-byte seed olarak serialize etmek,
   `ask` ve `fvk`'yi yeniden hesaplamak gerektirir. Minimal depolama.

### Nova Shield Chain Icin Dersler

- **ask ve nk turetimi**: `prf::expand` (BLAKE2b) ile iki farkli input byte — Nova Shield
  icin ayni pattern. `spend_seed || [0] → signing_key`, `spend_seed || [1] → nullifier_key`.
- **BIP44 zorunlu**: Hardware cuzdan (Ledger/Trezor) destegi icin BIP44 standardi gerekmektedir.
  Nova Shield wallet SLIP-0044 kayit + BIP44 turetimi olmadan Ledger desteginden mahrum kalir.
- **FVK cachele, seed tut**: `SpendKey` en degerli nesne — `seed` sakla, gerisini turet.
  Bellek boyutu minimize edilir.

---

## [keys/wallet_id.rs]

**Dosya:** `/crates/core/keys/src/keys/wallet_id.rs`
**Satir Sayisi:** 51

### Genel Amac

`WalletId` — FVK'nin Poseidon377 hash'i, hesap kimligi olarak kullanilir. 32 byte.

### Struct/Enum/Type Listesi

#### `struct WalletId(pub [u8; 32])` (Satir 9)
- `Copy, Clone, PartialEq, Eq, PartialOrd, Ord`
- `Display`: bech32m `penumbrawalletid1...` prefix.

### Trait Implementasyonlari

- `TryFrom<v1::WalletId>`: `inner: Vec<u8>` → `[u8; 32]`.
- `From<WalletId>`: `inner: value.0.to_vec()`.
- `Debug` delegates to `Display`.

### Tasarim Kararlari

1. **WalletId = Poseidon(nk, ak)**: FVK'nin Poseidon hash'i. Harcama anahtari bilgisi yok.
   Bu sayede WalletId paylasimi FVK'yi ifsa etmez.
2. **`Ord` derive**: Siralama gerektiren veri yapilari icin — `BTreeMap<WalletId, _>`.

### Nova Shield Chain Icin Dersler

- Wallet kimligini anahtardan deterministik ama geri donusumsuz tureterek tanimlamak.
  Nova Shield wallet ID = Poseidon(master_spend_key bilesenlerinden) seklinde benimsenebilir.

---

## [prf.rs]

**Dosya:** `/crates/core/keys/src/prf.rs`
**Satir Sayisi:** 13

### Genel Amac

Penumbra'nin tek PRF (Pseudo-Random Function) modulu. BLAKE2b uzerinde kisisellestirilmis
keyed hash fonksiyonu.

### Fonksiyonlar

#### `pub fn expand(label: &'static [u8; 16], key: &[u8], input: &[u8]) -> blake2b_simd::Hash` (Satir 3–8)
```rust
blake2b_simd::Params::new()
    .personal(label)    // 16-byte domain tag
    .key(key)           // Anahtar icerigi (blake2b keyed hash)
    .hash(input)        // Mesaj / input
```
- BLAKE2b-512 (64 byte cikti), `personal` ile domain ayirimi, `key` ile HMAC benzeri kullanim.

#### `pub fn expand_ff<F: PrimeField>(label: &'static [u8; 16], key: &[u8], input: &[u8]) -> F` (Satir 10–12)
- `expand(...)` → `as_bytes()` → `F::from_le_bytes_mod_order(...)` — dogrudan alan elemani.

### Kriptografik Formuller

```
expand(label, key, input) = BLAKE2b-512(
    personal = label[16 bytes],
    key      = key,
    message  = input
)

expand_ff(label, key, input) = F::from_le_bytes_mod_order(expand(...))
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **BLAKE2b Personal**: 16-byte kisisel etiket. BLAKE2b, SHA256'ya gore PRF kullanimi icin
   daha iyi yapili cunku dahili "personal" parametresi tam da bunun icin tasarlanmis.
2. **label &'static**: Etiketler daima compile-time sabit — yanlis etiket kullanimi
   derleme zamaninda tutulabilir.
3. **`expand_ff` generic over PrimeField**: Hem `Fq` hem `Fr` icin kullanilabilir.

### Nova Shield Chain Icin Dersler

- `prf::expand` nova-shield icin temel PRF olmali. BLAKE2b personal parametresi ile
  her turetim amaci icin farkli 16-byte etiket (`b"NovaShield_Spend"`, `b"NovaShield_Nullif"` vb.)
- `expand_ff` generic yapisi: Fark alan tipleri icin tek implementasyon.

---

## [symmetric.rs]

**Dosya:** `/crates/core/keys/src/symmetric.rs`
**Satir Sayisi:** 450

### Genel Amac

Penumbra'nin simetrik sifreleme katmani: not, swap, memo ve konum metadata sifreleme/cozme
icin `PayloadKey`, `OutgoingCipherKey`, `BackreferenceKey`, `PositionMetadataKey` tipleri.

### Struct/Enum/Type Listesi

#### `enum PayloadKind` (Satir 18–27)
```rust
pub enum PayloadKind {
    Note,    // nonce = [0,0,...,0]
    MemoKey, // nonce = [1,0,...,0]
    Swap,    // nonce = [2,0,...,0]
    Memo,    // nonce = [3,0,...,0]
}
```
Her PayloadKind icin **farkli bir nonce** kullanilir. Ayni anahtar ile farkli tur veriyi
sifreleme, nonce çakismasindan kaynaklanabilecek saldiriyi onler.

#### `struct PayloadKey(Key)` (Satir 44)
- `ChaCha20Poly1305` simetrik anahtari (32 byte).

**Turetim yontemleri:**

##### `fn derive(shared_secret: &ka::SharedSecret, epk: &ka::Public) -> Self` (Satir 48–58)
```
KDF = BLAKE2b-256(personal="Penumbra_Payload", input = shared_secret.0 || epk.0)
PayloadKey = Key::from_slice(KDF.as_bytes())
```

##### `fn random_key<R: CryptoRng + RngCore>(rng: &mut R) -> Self` (Satir 61–65)
- 32 byte rastgele — memo key wrapping icin.

##### `fn derive_swap(ovk: &OutgoingViewingKey, cm: StateCommitment) -> Self` (Satir 95–107)
```
KDF = BLAKE2b-256(personal="Penumbra_Payswap", input = ovk.bytes || cm_bytes)
```
Swap sifreleme OVK + note commitment ile — gondericinin kendi swap'ini gorebilmesi.

##### `fn encrypt(&self, plaintext: Vec<u8>, kind: PayloadKind) -> Vec<u8>` (Satir 72–81)
- `ChaCha20Poly1305::new(&self.0).encrypt(nonce, plaintext)` — AEAD.
- Nonce: `kind.nonce()` — 12 byte, kind'a gore sabit.

##### `fn decrypt(&self, ciphertext: Vec<u8>, kind: PayloadKind) -> Result<Vec<u8>>` (Satir 83–92)
- `decrypt` ile AEAD dogrulama — manipulasyon tespiti dahil.

#### `struct OutgoingCipherKey(Key)` (Satir 177)
- Giden notlari gondericinin kendisi icin sifreleme.

##### `fn derive(ovk, cv, cm, epk) -> Self` (Satir 181–201)
```
KDF = BLAKE2b-256(
    personal="Penumbra_OutCiph",
    input = ovk.bytes || cv_bytes || cm_bytes || epk.bytes
)
```
- OVK + balance commitment + note commitment + ephemeral public key.
- Gondericinin kendi gonderdiği notu sonradan acabilmesi icin 4 parametre.

#### `struct OvkWrappedKey(pub [u8; 48])` (Satir 238)
- 48 byte = 32 byte PayloadKey + 16 byte AEAD tag.
- `OutgoingCipherKey` ile sifrelenmis `PayloadKey`.

#### `struct WrappedMemoKey(pub [u8; 48])` (Satir 270)
- Memo key'in alici icin sifrelenmis hali.

##### `fn encrypt(memo_key, esk, transmission_key, diversified_generator) -> Self` (Satir 278–297)
```
1. epk = esk.diversified_public(g_d)
2. shared_secret = esk.key_agreement_with(transmission_key)
3. action_key = PayloadKey::derive(shared_secret, epk)
4. encrypted_memo_key = action_key.encrypt(memo_key, MemoKey)
WrappedMemoKey(encrypted_memo_key)
```

##### `fn decrypt(&self, epk: ka::Public, ivk: &IncomingViewingKey) -> Result<PayloadKey>` (Satir 301–313)
```
1. shared_secret = ivk.key_agreement_with(epk)
2. action_key = PayloadKey::derive(shared_secret, epk)
3. decrypted_memo_key = action_key.decrypt(self.bytes, MemoKey)
PayloadKey(decrypted_memo_key)
```

#### `struct BackreferenceKey(pub Key)` (Satir 349)
##### `fn derive(ovk: &OutgoingViewingKey) -> Self` (Satir 352–360)
```
KDF = BLAKE2b-256(personal="Penumbra_Backref", input=ovk.bytes)
```
- Harcama backreference sifreleme — hangi noter'in harcandigini sonradan gormek icin.

#### `struct PositionMetadataKey(pub Key)` (Satir 373)
##### `fn derive(ovk: &OutgoingViewingKey) -> Self` (Satir 379–388)
```
KDF = BLAKE2b-256(personal="Penumbra_PosMeta", input=ovk.bytes)
```
- DEX likidite pozisyon metadata sifreleme (UIP-9).

##### Sabitler:
```rust
POSITION_METADATA_SIZE_BYTES: usize = 10
POSITION_METADATA_NONCE_SIZE_BYTES: usize = 24  // XChaCha20 nonce
POSITION_METADATA_AUTH_TAG_SIZE_BYTES: usize = 16
ENCRYPTED_POSITION_METADATA_SIZE_BYTES: usize = 50
```

##### `fn encrypt(&self, plaintext: &[u8; 10], nonce: &[u8; 24]) -> Vec<u8>` (Satir 391–411)
- `XChaCha20Poly1305::encrypt(nonce, plaintext)` — 24-byte nonce.
- Cikti: `nonce(24) || ciphertext+tag(10+16=26)` = 50 byte.

##### `fn decrypt(&self, ciphertext: &[u8]) -> Option<Vec<u8>>` (Satir 413–429)
- Uzunluk kontrolu (50 byte).
- Nonce ilk 24 byte.
- Sifre cozme basarisizsa `None` — hata mesaji yok (gizlilik).

### Kriptografik Formuller

**PayloadKey Turetimi (ECDH + KDF):**
```
shared_secret = ECDH(esk, pk_d) = esk * pk_d   (decaf377)
epk           = esk * g_d                        (ephemeral public key)
payload_key   = BLAKE2b-256("Penumbra_Payload", shared_secret.bytes || epk.bytes)
```

**OutgoingCipherKey:**
```
ock = BLAKE2b-256("Penumbra_OutCiph", ovk.bytes || cv.bytes || cm.bytes || epk.bytes)
```

**PositionMetadataKey:**
```
pmk = BLAKE2b-256("Penumbra_PosMeta", ovk.bytes)
```

**Nonce Domain Separation:**
```
Note    : [0,0,...,0,0]   (12 bytes)
MemoKey : [1,0,...,0,0]
Swap    : [2,0,...,0,0]
Memo    : [3,0,...,0,0]
```

### Unsafe Bloklar

Yok.

### Tasarim Kararlari

1. **ChaCha20Poly1305 vs AES-GCM**: ChaCha20 secilmis — side-channel direnci, hizli yazilim
   implementasyonu. Nova Shield icin de uygun.
2. **Sabit nonce per kind**: Ayni `PayloadKey` ile farkli tur veriler sifrelenir ama nonce
   sabit degistiriliyor. Cunku her action icin yeni bir `PayloadKey` turetiliyor (ecdh ile).
   Ayni key ikinci kez kullanilmaz — sabit nonce guvenli.
3. **XChaCha20 for PositionMetadata**: 24-byte nonce — rastgele nonce kullanimi icin uygun.
   Normal ChaCha20 (12-byte) rastgele nonce ile guvenilir degil — 2^32 mesaj sonrasi
   cakisma riski. XChaCha20 bu sorunu cozer.
4. **OvkWrappedKey ve WrappedMemoKey 48 byte**: 32-byte plaintext + 16-byte AEAD tag.
   Her ikisi de sabit boyut — transaction sifresizme maliyeti one-pass.
5. **Nonce sifir**: Note ve baska payload tipleri icin nonce hep `[0,0,...,0]` + kind index.
   Bu guvenli cunku her not icin farkli `PayloadKey` var (ECDH ile turetilen).

### Nova Shield Chain Icin Dersler

- **ChaCha20Poly1305 tercih et**: AES-GCM yerine ChaCha20Poly1305 — donanim hizlandirmasi
  olmadan daha hizli, konstant zamani, yan-kanal guvenli.
- **XChaCha20 for random nonce**: DEX pozisyon metadata gibi rastgele nonce gerektiren
  alanlar icin XChaCha20Poly1305 (24-byte nonce). Normal ChaCha20 icin nonce counter veya per-key.
- **Domain separation with personal labels**: Her sifreleme amaci icin farkli BLAKE2b personal
  etiketi. Yeniden kullanim saldirilarini onler.
- **WrappedKey pattern**: `PayloadKey` → `WrappedKey` pattern'i, memo key ve ovk wrapped key
  gibi Nova Shield'in gizli iletisim katmani icin kullanilabilir.
- **Outgoing cipher key**: Gondericinin kendi gonderdigi notlari gorebilmesi (OVK bazli),
  Nova Shield'da "outgoing view" ozelligi icin aynen uygulanabilir.

---

## [test_keys.rs]

**Dosya:** `/crates/core/keys/src/test_keys.rs`
**Satir Sayisi:** 63

### Genel Amac

Hardcoded test anahtarlari ve adresleri — genesis state ve test kodu icin.
062-Iapetus testnet'ten itibaren kullanilan standart test cuzdani.

### Struct/Enum/Type Listesi

#### Sabitler ve Lazy Statics
```rust
pub const SEED_PHRASE: &str = "comfort ten front cycle churn ...";  // 24 kelime
pub const ADDRESS_0_STR: &str = "penumbra147mfall0...";
pub const ADDRESS_1_STR: &str = "penumbra1vmmz304...";
pub static ADDRESS_0: Lazy<Address>
pub static ADDRESS_1: Lazy<Address>
pub static SPEND_KEY: Lazy<SpendKey>   // BIP44 path m/44'/6532'/0'
pub const FULL_VIEWING_KEY_STR: &str = "penumbrafullviewingkey1vzfytwlvq...";
pub static FULL_VIEWING_KEY: Lazy<FullViewingKey>
pub static WALLET_ID: Lazy<WalletId>  // FVK.wallet_id()
```

### Testler

#### `test_fvk_matches` (Satir 58–61)
- `FULL_VIEWING_KEY == SPEND_KEY.full_viewing_key()` dogrulanir.
- Hardcoded FVK'nin hardcoded seed'den deterministik turetildigi kaniti.

### Tasarim Kararlari

1. **`Lazy<T>`**: Test vektorleri program baslaginca deil, ilk kullanildiginda hesaplanir.
   Parser ve kriptografi maliyeti sadece kullanan testlerde olusur.
2. **`once_cell::sync::Lazy`**: Thread-safe gecikmeli baslangic — paralel test yuruyusu guvenli.

### Nova Shield Chain Icin Dersler

- Proje genelinde standart test anahtarlari `Lazy<T>` ile tanimlanmali — her test dosyasinda
  yeniden olusturma yerine, tek merkezi kaynak.
- Hardcoded test vektorleri testnet genesis ile eslesmeli — deterministik turetim kaniti.

---

## [tests/test_wallet_id.rs]

**Dosya:** `/crates/core/keys/tests/test_wallet_id.rs`
**Satir Sayisi:** 31

### Genel Amac

Integration test: BIP39 derivation (eski yol, `from_seed_phrase_bip39`) ile WalletId
bech32m formati dogrulama.

### Testler

#### `wallet_id_to_bech32` (Satir 9–29)
```rust
seed = SeedPhrase::from_str("comfort ten front cycle churn ...").unwrap();
spend_key = SpendKey::from_seed_phrase_bip39(seed, 0);  // BIP39 (index=0)
fvk = spend_key.full_viewing_key();
wallet_id = fvk.wallet_id();
actual = wallet_id.to_string();
expected = "penumbrawalletid15r7q7qsf3hhsgj0g530n7ng9acdacmmx9ajknjz38dyt90u9gcgsmjre75"
assert_eq!(expected, actual);
```

**Dikkat**: Bu test `from_seed_phrase_bip39` kullanirken `test_keys.rs`'deki `SPEND_KEY`
`from_seed_phrase_bip44` kullanir. Ayni seed phrase'den BIP39 ve BIP44 farkli SpendKey uretir.

### Nova Shield Chain Icin Dersler

- Integration testlerini kaynak koddan ayri `tests/` klasorune koymak —
  unit testler ile integration testleri ayri katmanlarda daha kolay yonetilir.
- Bech32 string test vektoru: WalletId'yi bir kez referans string ile dogrulamak,
  geriye donuk uyumlulugun kilkide tutulmasini saglar.

---

## KRIPTOGRAFIK TURETIM ZINCIRI — TAM OZET

```
SeedPhrase (12/24 kelime, BIP39)
    |
    | PBKDF2-HMAC-SHA512(password=phrase, salt="mnemonic", rounds=2048, len=64 bytes)
    v
Master Seed (64 bytes)
    |
    | BIP32 XPrv::derive_from_path("m/44'/6532'/{account}'")
    v
SpendKeyBytes (32 bytes)
    |
    | BLAKE2b("Penumbra_ExpndSd", key=seed, input=[0]) as Fr → ask
    | BLAKE2b("Penumbra_ExpndSd", key=seed, input=[1]) as Fq → nk
    v
SpendKey { seed, ask: SigningKey<SpendAuth>, fvk: FullViewingKey }
    |
    |-- ask (Fr) → VerificationKey<SpendAuth> = ask * G  (decaf377)
    |
    |-- fvk.ovk: BLAKE2b("Penumbra_DeriOVK", key=nk.bytes, input=ak.bytes)[0..32]
    |-- fvk.dk:  BLAKE2b("Penumbra_DerivDK", key=nk.bytes, input=ak.bytes)[0..16]
    |-- fvk.ivk: Poseidon("penumbra.derive.ivk", nk.Fq, ak_s.Fq) as Fr
    |
    v
FullViewingKey { ak, nk, ovk: OVK[32], ivk: IVK(ivk_fr, dk[16]) }
    |
    |-- WalletId: Poseidon("Penumbra_HashFVK", nk.Fq, Fq(ak))[0..32]
    |
    |-- For AddressIndex (account, randomizer):
    |   |
    |   | d = AES-128-ECB(dk, index.to_bytes())
    |   | g_d = BLAKE2b("Penumbra_Divrsfy", d) → Fq → encode_to_curve
    |   | pk_d = ivk_fr * g_d     (decaf377 scalar mul)
    |   | dtk = BLAKE2b("PenumbraExpndFMD", key=ivk.bytes, d.bytes) as Fr
    |   | ck_d = dtk.clue_key()   (FMD detection key → clue key)
    |   v
    | Address { d[16], pk_d[32], ck_d[32] }
    |   → F4Jumble → 80 bytes → Bech32m("penumbrav2t1...")
    |
    |-- PayloadKey (per note):
    |   ECDH: shared_secret = ivk * epk
    |   payload_key = BLAKE2b("Penumbra_Payload", shared_secret || epk)
    |   encrypt: ChaCha20Poly1305(payload_key, nonce_by_kind, plaintext)
    |
    |-- OVK uses:
        - OutgoingCipherKey: BLAKE2b("Penumbra_OutCiph", ovk || cv || cm || epk)
        - BackreferenceKey:  BLAKE2b("Penumbra_Backref", ovk)
        - PositionMetadataKey: BLAKE2b("Penumbra_PosMeta", ovk)
```

---

## GENEL MIMARI DEGERLENDIRME

### Penuembra keys crate'inin Gucleri

1. **Tamamen deterministik turetim**: Seed'den adrese kadar her adim deterministik. Tek
   seed phrase'den tum sistem yeniden olusturulabilir.
2. **ZK-uyumlu yapilar**: Her kritik buyukluk (nk, ask, ivk, g_d) dogrudan R1CS devresinde
   temsil edilebilir. ZK ispatlar hesaplama sistemiyle entegrasyon en basindan tasarlanmis.
3. **Domain separation**: Her PRF/KDF cagrisinda farkli `personal` etiketi — cakisma imkansiz.
4. **Privacy by design**: FMD clue key, transparent adres modu, OVK wrapped key — gizlilik
   katmanlari birbiriyle tutarli ve tamamlayici.
5. **BLAKE2b + Poseidon kombinasyonu**: Off-chain = BLAKE2b (hizli), devre-ici = Poseidon
   (ZK-friendly). Her biri dogru yerde kullanilmis.
6. **Proptest ile matematiksel dogrulama**: Field assumption testleri, round-trip testleri.

### Potansiyel Zayifliklar

1. **PBKDF2 rounds=2048**: Modern standart (Argon2, scrypt veya 600k+ PBKDF2) ile karsilastirildiginda
   zayif. Hardware cuzdan uyumlulugu zorundaysa bu kabul edilebilir trade-off.
2. **`ChaCha20Poly1305` sabit nonce**: `kind.nonce()` sabit; guvenli cunku her action yeni
   PayloadKey alir. Ancak bu baglami anlayan gelistirici olmayan biri icin tehlikeli gorunuyor.
3. **`vartime_decompress`**: Gizli bilgi (ozel anahtar vs) icin degil, gecerli;  ama kodda
   kullanim yeri dokumante edilmeli.

---

## NOVA SHIELD CHAIN ICIN KRITIK EYLEM PLANI

### Dogrudan Kopyalanabilecekler

1. **prf::expand pattern**: `BLAKE2b(personal=label[16], key=key, input=input)` her PRF icin.
2. **Turetim zinciri**: `seed → ask/nk → FVK → IVK/OVK → Address` yapisi.
3. **AES-128 diversifier sistemi**: Tek IVK'dan coklu adres, `AES-128-ECB(dk, index)`.
4. **ChaCha20Poly1305 + XChaCha20Poly1305**: Payload sifreleme icin.
5. **F4Jumble**: Gizli adres format'i karistirma (ZIP-316).
6. **PayloadKind nonce pattern**: `kind.nonce()` ile ayni key, farkli nonce.
7. **BIP39 + BIP44 iki katmanli turetim**: Hardware cuzdan destegi.
8. **`OnceLock` lazy compute**: Pahali kriptografik hesaplamalar icin.
9. **`AddressView::Opaque/Decoded` pattern**: Wallet goruntuleyici katmani.
10. **`enforce_not_equal(identity)` R1CS**: Eliptic curve noktalari icin guvenlik kisiti.

### Nova Shield'de Farkli Yapilmasi Gerekenler

1. **PBKDF2 round sayisi**: Hardware cuzdan destegi zorunlu degilse Argon2id veya 600k+ PBKDF2.
2. **Coin type**: SLIP-0044'e Nova Shield icin yeni coin type kayit et.
3. **ZK proof sistemi**: arkworks + Groth16/PLONK — Penumbra `poseidon377` + `decaf377` kullanir;
   Nova Shield bunu arkworks `BLS12-381` uzerinde uyarlamali.
4. **FMD benzeri sistem**: Penumbra'nin FMD'si olmasayi'nda basit detection key yeterli;
   Nova Shield privacy seviyesine gore karar vermeli.
5. **Transparent address gereksinimi**: IBC ile etkilesim icin gerekli ama Nova Shield
   ilk surum icin atlanabilir.

### Nova Shield icin Eklenecek Ozellikler

1. **MPC threshold signing entegrasyonu**: `ask` (SpendAuth signing key) threshold yapisi
   ile — `cggmp21` ile `ask`'i paylasmak. Penumbra tek-kullanici varsayimli.
2. **Cross-chain address routing**: Penumbra Noble forwarding gibi ama daha genel IBC.
3. **Shielded DEX pozisyon anahtari**: `PositionMetadataKey` pattern Nova Shield DEX icin
   — kullanici kendi likidite pozisyon metadatasini sifreleme.
```

---

*Analiz Tamamlandi: 2026-07-22 | Agent-02 | 20 dosya, ~1850 satir*
