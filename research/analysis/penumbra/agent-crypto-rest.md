# Penumbra Kriptografi Analizi — agent-crypto-rest
## Kapsam: decaf377-frost, decaf377-ka, proof-params, proof-setup
## Analiz Tarihi: 2026-07-22
## Toplam Dosya: 27

---

## GENEL BAKIS

Bu 4 crate Penumbra'nın kriptografik altyapısının en kritik parçalarını oluşturur:

1. **decaf377-frost**: FROST eşik imzalama — decaf377 eğrisi üzerinde t-of-n çoklu imza
2. **decaf377-ka**: decaf377 üzerinde Diffie-Hellman anahtar anlaşması — çeşitlendirilebilir public key'ler
3. **proof-params**: 7 devre için Groth16 ispat/doğrulama anahtarları — lazy static yükleme
4. **proof-setup**: Güvenilir kurulum seremonisi (Powers of Tau + Phase 2) — tüm akış burada

---

## BÖLÜM 1: decaf377-frost (6 dosya)

---

## [hash.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-frost/src/hash.rs`
**Satır Sayısı:** 47

### Genel Amaç

FROST protokolüne özgü hash fonksiyonu. BLAKE2b-512 üzerine kurulu, "FROST-decaf377" veya "decaf377-rdsa---" personalization string'leri ile iki farklı hasher modu sağlar.

### Struct/Enum/Type Listesi

- `Hasher` (Satır 4): `state: blake2b_simd::State` tutan yapı.

### Trait Implementasyonları

- `Default for Hasher` (Satır 8-16):
  - `Params::new().hash_length(64).personal(b"FROST-decaf377").to_state()`
  - Standart FROST hash'i için — H1, H3, H4, H5, HDKG, HID fonksiyonlarında kullanılır.

### Fonksiyonlar

#### `Hasher::challenge() -> Self` (Satır 20-27)
- `personal(b"decaf377-rdsa---")` ile yeni hasher.
- **Kritik:** Bu hasher yalnızca H2 (challenge hash) için kullanılır.
- Personalization string `b"decaf377-rdsa---"` tam 16 byte — BLAKE2b kısıtı.
- rdsa kütüphanesiyle uyumluluk için aynı domain separation string kullanılır.

#### `Hasher::update(&mut self, data: impl AsRef<[u8]>) -> &mut Self` (Satır 31-34)
- Builder pattern ile zincirlenebilir veri ekleme.

#### `Hasher::finalize_scalar(&self) -> Fr` (Satır 37-39)
- `Fr::from_le_bytes_mod_order(self.state.finalize().as_array())`
- 64-byte BLAKE2b çıktısını Fr skalara çevirir (modüler indirgeme ile).
- 64 byte kullanılması, indirgeme önyargısını ihmal edilebilir kılmak için (512-bit / 255-bit scalar).

#### `Hasher::finalize(&self) -> [u8; 32]` (Satır 42-47)
- `self.state.finalize().as_bytes()[..32].try_into()`
- İlk 32 byte alınır, geri kalanı atılır.
- H4 ve H5 için kullanılır (mesaj hash ve taahhüt hash).

### Kriptografik Formüller

```
BLAKE2b-512("FROST-decaf377", data) -> 64 bytes -> Fr::from_le_bytes_mod_order
```

Domain separation:
- `b"FROST-decaf377"` → H1, H3, H4, H5, HDKG, HID
- `b"decaf377-rdsa---"` → H2 (challenge, rdsa ile uyumlu)

### Unsafe Bloklar

Yok.

### Testler

Yok (test bu dosyada değil, traits.rs'deki implementasyon test edilir).

### Tasarım Kararları

1. **64-byte hash:** Scalar'a dönüştürmede bias minimize etmek için 512-bit hash.
2. **Personalization string ayrımı:** H2 için farklı personalization kullanılması, challenge üretimini rdsa kütüphanesiyle uyumlu kılar — Penumbra'nın SpendAuth imzaları FROST dışında da rdsa ile üretilebildiğinden challenge'ın aynı olması zorunlu.
3. **`as_array()` kullanımı:** BLAKE2b sonuçları `as_array()` ile 64-byte array olarak alınır.

### Nova Shield Chain İçin Dersler

- FROST implementasyonunda domain separation kritik; her hash fonksiyonu (H1..H5, HDKG, HID) ayrı domain tag almalı.
- Nova Shield Chain MPC (cggmp21) kullanacaksa benzer BLAKE2b tabanlı hash yapısı doğrudan adapte edilebilir.
- `from_le_bytes_mod_order` ile 512-bit→scalar indirgeme: bias < 2^(-128), üretimde yeterince güvenli.

---

## [traits.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-frost/src/traits.rs`
**Satır Sayısı:** 127

### Genel Amaç

FROST protokolü için `frost_core` trait'lerinin decaf377 üzerinde implementasyonu. Üç temel yapı: `Decaf377ScalarField` (scalar alanı), `Decaf377Group` (grup), `Decaf377Rdsa` (ciphersuite).

### Struct/Enum/Type Listesi

- `Decaf377ScalarField` (Satır 9): `Copy + Clone`. decaf377'nin Fr alanını FROST `Field` trait'ine bağlar.
- `Decaf377Group` (Satır 48-49): `Copy + Clone + PartialEq + Eq`. decaf377 grubunu FROST `Group` trait'ine bağlar.
- `Decaf377Rdsa` (Satır 83-84): `Debug + PartialEq + Eq + Copy + Clone`. Tüm ciphersuite; FROST'un `Ciphersuite` trait'ini implemente eder.

### Trait Implementasyonları

#### `Field for Decaf377ScalarField` (Satır 11-46)

| Metot | Implementasyon |
|---|---|
| `type Scalar` | `decaf377::Fr` |
| `type Serialization` | `Vec<u8>` |
| `zero()` | `Fr::zero()` |
| `one()` | `Fr::one()` |
| `invert(scalar)` | `scalar.inverse().ok_or(FieldError::InvalidZeroScalar)` |
| `random(rng)` | `Fr::rand(rng)` |
| `serialize(scalar)` | `scalar.to_bytes().to_vec()` |
| `little_endian_serialize(scalar)` | `Self::serialize(scalar)` (aynı, FR zaten LE) |
| `deserialize(buf)` | `Fr::from_bytes_checked(...)` |

- `from_bytes_checked`: Tam geçerlilik kontrolü yapar (asal alana ait olup olmadığı dahil).

#### `Group for Decaf377Group` (Satır 51-81)

| Metot | Implementasyon |
|---|---|
| `type Field` | `Decaf377ScalarField` |
| `type Element` | `decaf377::Element` |
| `type Serialization` | `Vec<u8>` |
| `cofactor()` | `Fr::one()` |
| `identity()` | `Element::default()` |
| `generator()` | `decaf377::Element::GENERATOR` |
| `serialize(element)` | `element.vartime_compress().0.to_vec()` |
| `deserialize(buf)` | `decaf377::Encoding(...).vartime_decompress()` |

- **`cofactor() = 1`**: decaf377 cofactor-1 gurubудur. Bu FROST güvenlik kanıtları için kritik (subgroup olmayan noktalara dair saldırılar etkisiz).
- **`vartime_compress/decompress`**: Sabit zamanlı değil — timing saldırıları mümkün. Penumbra bu noktaları halka açık (non-secret) sayar.

#### `Ciphersuite for Decaf377Rdsa` (Satır 87-126)

```
const ID: &'static str = "decaf377-rdsa"
type Group = Decaf377Group
type HashOutput = [u8; 32]
type SignatureSerialization = Vec<u8>
```

| FROST Fn | Implementasyon | Domain Tag |
|---|---|---|
| `H1(m)` | rho binding nonce | `b"rho"` |
| `H2(m)` | challenge (Fiat-Shamir) | `Hasher::challenge()` — `b"decaf377-rdsa---"` |
| `H3(m)` | nonce üretimi | `b"nonce"` |
| `H4(m)` | mesaj hash | `b"msg"` |
| `H5(m)` | taahhüt hash | `b"com"` |
| `HDKG(m)` | DKG hash | `b"dkg"` |
| `HID(m)` | identifier hash | `b"id"` |

### Kriptografik Formüller/Protokol

**FROST Signing Flow:**
```
Round 1:
  Her katılımcı i: (hiding_nonce_i, binding_nonce_i) = H3(secret_share_i || random)
  Taahhüt: (D_i, E_i) = (hiding_nonce_i * G, binding_nonce_i * G)

Round 2 (coordinator):
  binding_factor_i = H1(identifier_i || message || {(j, D_j, E_j)})
  R = Σ (D_i + binding_factor_i * E_i)
  challenge c = H2(R || vk || message)

Her katılımcı i:
  z_i = hiding_nonce_i + binding_factor_i * binding_nonce_i + lambda_i * c * secret_share_i
  (lambda_i = Lagrange katsayısı)

Aggregation:
  z = Σ z_i
  Signature = (R, z)

Doğrulama:
  c = H2(R || vk || message)
  z * G == R + c * vk  ✓
```

### Unsafe Bloklar

Yok.

### Testler

Yok (doğrudan test frost.rs test dosyasında).

### Tasarım Kararları

1. **`vartime_` prefix:** Timing yan kanalı kabul ediliyor çünkü bu noktalar public bilgi.
2. **`Vec<u8>` Serialization:** Fixed-size array yerine Vec — daha esnek ama hafıza tahsisatı gerektirir.
3. **`cofactor = 1`:** decaf377'nin sağladığı özellik; cofactor-8 curve'lerde FROST güvenlik kanıtları daha zor.
4. **H2 = challenge için ayrı personalization:** rdsa library'siyle interoperability zorunluluğu.

### Nova Shield Chain İçin Dersler

- decaf377 cofactor-1 olduğu için FROST implementasyonu basittir; eğer Nova Shield Chain farklı bir eğri (secp256k1, ristretto255) kullanacaksa cofactor yönetimi dikkat gerektirir.
- `cggmp21` kütüphanesi benzer abstraction sağlar; bu trait yapısından ilham alınabilir.
- **Önemli:** Tüm hash fonksiyonları domain-separated olmalı (H1..H5, HDKG, HID), aksi halde hash collision saldırıları mümkün.

---

## [keys.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-frost/src/keys.rs`
**Satır Sayısı:** 104

### Genel Amaç

FROST anahtar yönetimi. Merkezi dealer veya DKG ile anahtar payı üretimi, anahtarın parçalara bölünmesi, ve anahtar yeniden oluşturma.

### Type Alias Listesi

| Tip | Açıklama |
|---|---|
| `IdentifierList<'a>` | `frost::keys::IdentifierList<'a, E>` |
| `SecretShare` | `frost::keys::SecretShare<E>` |
| `SigningShare` | `frost::keys::SigningShare<E>` |
| `VerifyingShare` | `frost::keys::VerifyingShare<E>` |
| `VerifyingKey` | `frost_core::VerifyingKey<E>` |
| `KeyPackage` | `frost::keys::KeyPackage<E>` |
| `PublicKeyPackage` | `frost::keys::PublicKeyPackage<E>` |
| `VerifiableSecretSharingCommitment` | `frost::keys::VerifiableSecretSharingCommitment<E>` |

### Fonksiyonlar

#### `generate_with_dealer<RNG>(max_signers, min_signers, identifiers, rng) -> Result<(HashMap<Identifier, SecretShare>, PublicKeyPackage), Error>` (Satır 18-25)
- Güvenilir merkezi dealer ile Shamir secret sharing.
- Dealer rastgele bir signing key üretir, Shamir gizli paylaşımı ile `max_signers` katılımcıya dağıtır.
- `min_signers` adet pay birleşince imza üretilebilir.
- **Güvenlik uyarısı:** Dealer geçici olarak tam signing key'e sahip olur. Güvenilir taraf yoksa DKG kullanılmalı.

#### `split<R>(secret, max_signers, min_signers, identifiers, rng) -> Result<(HashMap<Identifier, SecretShare>, PublicKeyPackage), Error>` (Satır 33-43)
- Mevcut bir signing key'i paylaştırır.
- `secret.to_bytes().to_vec()` → `frost_core::SigningKey::deserialize(...)` dönüşümü gerekir.
- Comment (Satır 40): `// https://github.com/ZcashFoundation/frost/issues/497` — kütüphane sınırı, doğrudan convert mümkün değil.

#### `reconstruct(key_packages: &[KeyPackage]) -> Result<SigningKey<SpendAuth>, Error>` (Satır 56-63)
- Lagrange interpolasyon ile t-of-n paydan tam signing key'i yeniden oluşturur.
- **Kritik uyarı (Satır 45-52):** Bu fonksiyon sadece threshold signing'e gerek kalmadığında kullanılır. FROST'un amacı tam key'i hiçbir zaman bir noktada toplamak olmadan imza üretmektir.
- `min_signers`'dan az pay verilirse **farklı bir key döner**, hata vermez — caller sorumluluğu.

### Tasarım Kararları

1. **type alias pattern:** Tüm tipler `E = traits::Decaf377Rdsa` ile parametrelenmiş `frost_core` tipleridir. Nova Shield Chain başka ciphersuite eklemek isterse sadece `E`'yi değiştirmek yeterli.
2. **`split` fonksiyonu:** Mevcut key'den pay üretimi, migration senaryolarında gerekli (örn. tek imzadan eşik imzaya geçiş).
3. **`reconstruct` tehlikesi:** Fonksiyonun varlığı tehlikeli değil, ama yanlış kullanım (az pay ile çağırma) sessizce farklı key döndürür. Bu Shamir'in matematiksel özelliği.

### Nova Shield Chain İçin Dersler

- FROST'u Nova Shield Chain'de kullanırken `generate_with_dealer` yerine `dkg::part1/2/3` tercih edilmeli.
- `reconstruct` hiçbir zaman production akışında çağrılmamalı — sadece key migration için.
- `min_signers < t` kontrolü uygulayan bir wrapper yazılmalı.

---

## [keys/dkg.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-frost/src/keys/dkg.rs`
**Satır Sayısı:** 184

### Genel Amaç

Güvenilir dealer olmadan dağıtık anahtar üretimi (DKG). 3 aşamalı protokol: part1 (broadcast), part2 (point-to-point gizli kanal), part3 (key package üretimi). Penumbra Protobuf serialization'ı ile entegre.

### Modüller

- `round1` (Satır 10-64): Round 1 yapıları ve protobuf dönüşümleri
- `round2` (Satır 66-118): Round 2 yapıları ve protobuf dönüşümleri

### Type Alias'lar

```rust
round1::SecretPackage = frost::keys::dkg::round1::SecretPackage<E>
round2::SecretPackage = frost::keys::dkg::round2::SecretPackage<E>
```

### Struct'lar

#### `round1::Package` (Satır 26)
- `pub(crate) frost::keys::dkg::round1::Package<E>` wrapper'ı.
- Broadcast edilecek paket: `commitment` (VerifiableSecretSharingCommitment) + `proof_of_knowledge`.
- **Güvenlik:** `SecretPackage` ASLA diğer katılımcılara gönderilmez (kod yorumunda belirtilmiş, Satır 16-21).

#### `round2::Package` (Satır 88)
- `pub(crate) frost::keys::dkg::round2::Package<E>` wrapper'ı.
- Point-to-point paket: `signing_share` (scalar).
- **Güvenlik (Satır 86-87):** Gizli ve doğrulanmış kanalda gönderilmeli. Gizli değilse diğer katılımcılar payı öğrenebilir.

### Fonksiyonlar

#### `part1<R: RngCore + CryptoRng>(identifier, max_signers, min_signers, rng) -> Result<(round1::SecretPackage, round1::Package), Error>` (Satır 126-133)
- `frost::keys::dkg::part1(...)` çağrısını `round1::Package` wrapper'ı ile döner.
- Her katılımcı kendi gizli polinomu üretir, taahhütleri ve bilgi ispatını broadcast eder.

#### `part2(secret_package, round1_packages) -> Result<(round2::SecretPackage, HashMap<Identifier, round2::Package>), Error>` (Satır 143-158)
- Round 1 paketlerini alır, her diğer katılımcıya bir signing share gönderir.
- HashMap dönüşümü: `(a, b.0.clone())` ile inner değerlere erişim.
- `round1_packages` kendi paketimizi içermemeli (testimizde `remove(id)` ile sağlanıyor).

#### `part3(round2_secret_package, round1_packages, round2_packages) -> Result<(KeyPackage, PublicKeyPackage), Error>` (Satır 169-183)
- Tüm round 2 paketlerini alır, kendi long-lived `KeyPackage`'ını ve herkese ait `PublicKeyPackage`'ı üretir.
- Bu noktadan sonra DKG tamamlanmış, imzalama başlayabilir.

### Protobuf Dönüşümleri

`From<Package> for pb::DkgRound1Package` (Satır 28-43):
```
commitment.elements: Vec<Vec<u8>> (her commitment noktası serialize edilmiş)
proof_of_knowledge: Vec<u8> (Schnorr signature serialize edilmiş)
```

`TryFrom<pb::DkgRound1Package> for Package` (Satır 45-63):
- `VerifiableSecretSharingCommitment::deserialize(elements)`
- `frost_core::Signature::deserialize(proof_of_knowledge)`

`From<Package> for pb::DkgRound2Package` (Satır 90-97):
```
signing_share.scalar: Vec<u8>
```

### DKG Protokolü Matematiği

```
Part 1 (her katılımcı i):
  Rastgele polinom f_i(X) = a_{i,0} + a_{i,1}*X + ... + a_{i,t-1}*X^{t-1}
  Taahhüt: C_i = (a_{i,0}*G, a_{i,1}*G, ..., a_{i,t-1}*G)
  Bilgi ispatı: Schnorr proof for a_{i,0}
  Broadcast: (C_i, proof)

Part 2 (her katılımcı i, her j ≠ i için):
  f_i(j) hesapla ve j'ye gizli kanalda gönder

Part 3 (her katılımcı j):
  Received shares doğrula: f_i(j)*G == Σ_k (j^k * C_i[k])
  signing_share_j = Σ_i f_i(j)
  verifying_share_j = signing_share_j * G
  group_public_key = Σ_i a_{i,0} * G = Σ_i C_i[0]
```

### Unsafe Bloklar

Yok.

### Testler

DKG testi `tests/frost.rs`'de kapsamlı şekilde yer alıyor.

### Tasarım Kararları

1. **Protobuf wrapper:** Penumbra'nın protobuf sistemi `DomainType` trait'i ile integrate — her type'ın proto karşılığı var.
2. **`pub(crate)` inner fields:** `Package(pub(crate) frost::...::Package<E>)` — inner FROST type'ına sadece crate içinden erişim.
3. **`anyhow::Error`:** Tüm hata dönüşleri `anyhow::Error` — kaynak hata mesajları korunuyor.

### Nova Shield Chain İçin Dersler

- Nova Shield Chain DKG için aynı 3-aşamalı (part1/2/3) protokolü `cggmp21` veya bu yapı üzerinden kullanabilir.
- **Round 2 güvenli kanal zorunlu:** Nova Shield Chain'de P2P şifreli kanal olmadan DKG kullanılmaz.
- Protobuf dönüşüm katmanı, network serialization için temiz ayrım sağlar.

---

## [lib.rs] (decaf377-frost)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-frost/src/lib.rs`
**Satır Sayısı:** 262

### Genel Amaç

FROST kütüphanesinin ana entry point'i. Round 1 commit, round 2 sign, aggregate ve randomized varyantlarını dışa açar.

### Type Alias'lar

```rust
pub type Error = frost_core::Error<traits::Decaf377Rdsa>
pub type Identifier = frost::Identifier<E>
type E = traits::Decaf377Rdsa  // internal
```

### Modüller

- `pub mod round1`: Commit aşaması
- `pub mod round2`: Sign aşaması
- `pub mod keys`: Key yönetimi
- `mod hash`, `mod traits`: Internal

### Struct'lar

#### `SigningPackage` (Satır 110)
- `frost::SigningPackage<E>` wrapper'ı.
- `new(signing_commitments: BTreeMap<Identifier, round1::SigningCommitments>, message: &[u8])` (Satır 116-125):
  - `BTreeMap` kullanımı: Deterministic sıralama — tüm katılımcılar aynı sırayla commit'leri işler.
- `signing_commitment(&self, identifier)`: Belirli katılımcının taahhüdünü getirir.

#### `round1::SigningCommitments` (Satır 54)
- `frost::round1::SigningCommitments<E>` wrapper'ı.
- `hiding` ve `binding` nonce taahhütleri.
- Protobuf: `pb::SigningCommitments { hiding: pb::NonceCommitment, binding: pb::NonceCommitment }`

#### `round2::SignatureShare` (Satır 148)
- `frost::round2::SignatureShare<E>` wrapper'ı.
- Protobuf: `pb::SignatureShare { scalar: Vec<u8> }`

### Fonksiyonlar

#### `round1::commit<RNG>(secret: &SigningShare, rng: &mut RNG) -> (SigningNonces, SigningCommitments)` (Satır 98-104)
- `frost::round1::commit::<E, RNG>(secret, rng)` çağrısı.
- **Kritik:** `SigningNonces` TEK KULLANIMLIK. Aynı nonce ile iki farklı mesajı imzalamak secret share'i ifşa eder.

#### `round2::sign(signing_package, signer_nonces, key_package) -> Result<SignatureShare, Error>` (Satır 180-185)
- Standart FROST imzalama.

#### `round2::sign_randomized(signing_package, signer_nonces, key_package, randomizer: decaf377::Fr) -> Result<SignatureShare, Error>` (Satır 189-202)
- `frost_rerandomized::sign(...)` çağrısı.
- `randomizer` skaları: `Randomizer::from_scalar(randomizer)`.
- **Amaç:** Doğrulama anahtarını randomize etmek (unlinkability için). Penumbra'da her işlem farklı randomized vk kullanır.

#### `aggregate(signing_package, signature_shares, pubkeys) -> Result<Signature<SpendAuth>, Error>` (Satır 222-235)
- Tüm payları birleştirir, tek imza üretir.
- `frost_sig.serialize()` → `[u8; 64]` → `Signature<SpendAuth>`.

#### `aggregate_randomized(signing_package, signature_shares, pubkeys, randomizer) -> Result<Signature<SpendAuth>, Error>` (Satır 239-261)
- `frost_rerandomized::aggregate(...)` çağrısı.
- `RandomizedParams::from_randomizer(pubkeys.group_public(), Randomizer::from_scalar(randomizer))`.

### Kriptografik Formüller/Protokol

**Randomized FROST:**
```
r = randomizer (Fr)
rk = vk + r * G  (randomized verification key)

Sign: her katılımcı r'yi kullanarak kendi payını hesaplar
Aggregate: imza (R, z) + r değişikliğiyle uyumlu
Verify: z * G == R + c * rk  (rk ile doğrulama)
```

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **`BTreeMap` for commitments:** Sorted map → deterministic serialization → tüm signers aynı challenge hash'i üretir.
2. **`Signature<SpendAuth>` dönüşümü:** FROST imzası Penumbra'nın rdsa `Signature` tipine dönüştürülür — interoperability.
3. **Randomized imzalar:** Penumbra'nın privacy modelinde her transaction farklı bir verification key ile görünür — linkability engellenir.

### Nova Shield Chain İçin Dersler

- **Nonce yeniden kullanım felaketi:** `SigningNonces` kesinlikle tek kullanımlık. Nova Shield Chain imzalama servisinde nonce state management titizlikle yapılmalı.
- **Randomized FROST:** Nova Shield Chain privacy katmanı istiyorsa randomized FROST ile her işlemde farklı vk kullanımı uygulanabilir.
- `BTreeMap` yerine `HashMap` kullanmak challenge hash'i tutarsız kılar — FROST'ta BTreeMap zorunlu.

---

## [tests/frost.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-frost/tests/frost.rs`
**Satır Sayısı:** 159

### Genel Amaç

2-of-3 FROST DKG + signing + randomized signing end-to-end entegrasyon testi.

### Test Yapısı

#### `simple_dkg_and_signing_flow()` (Satır 8-159)

**Sabitler:** `T: u16 = 2` (min signers), `N: u16 = 3` (max signers)

**Adımlar:**

1. **ID üretimi (Satır 13-15):**
   ```rust
   Identifier::derive(&id.to_le_bytes()).unwrap()
   ```
   Her katılımcı için deterministik identifier.

2. **DKG Round 1 (Satır 19-26):**
   - Her katılımcı `dkg::part1(*id, N, T, &mut OsRng)` çağırır.
   - `round1_secrets` (HashMap) ve `round1_packages` (HashMap) tutulur.

3. **DKG Round 2 (Satır 29-42):**
   - Her katılımcı kendi dışındakilerin round1_packages'larını alır (`round1_packages_except_us`).
   - `dkg::part2(round1_secret, &round1_packages_except_us)` → `(round2_secret, packages_for_others)`.

4. **DKG Round 3 / Part 3 (Satır 47-68):**
   - Her katılımcı kendine gelen round2_packages'ı toplar (`recvd_packages`).
   - `dkg::part3(round2_secret, round1_packages_except_us, recvd_packages)` → `(KeyPackage, PublicKeyPackage)`.

5. **İmzalama Round 1 (Satır 74-81):**
   - Her katılımcı `round1::commit(&shares[id].secret_share(), &mut OsRng)` → `(nonce, commitment)`.
   - `signing_package = SigningPackage::new(commitments, MSG)`.

6. **İmzalama Round 2 (Satır 85-93):**
   - Her katılımcı `round2::sign(package, nonce, key)` → `SignatureShare`.

7. **Aggregation & Verification (Satır 98-115):**
   - `frost::aggregate(package, shares, pubkey)` → `Signature<SpendAuth>`.
   - `VerificationKey::<SpendAuth>::try_from(vk_bytes)` ile vk oluşturulur.
   - `vk.verify(MSG, &signature)` ile doğrulama.

8. **Randomized İmzalama (Satır 117-156):**
   - `r = Fr::rand(&mut OsRng)` — randomizer.
   - `sign_randomized(...)` ve `aggregate_randomized(...)`.
   - `vk.randomize(&r)` → `r_vk`.
   - `r_vk.verify(MSG, &signature)` ile doğrulama.

### Kritik Gözlem

Test, tüm katılımcıların imzaladığı (T=N=2→3 değil, T=2 N=3, **hepsi imzalıyor**) durumu test ediyor. Gerçek t-of-n senaryosu (N-T katılımcı yokken) test edilmiyor. Bu bir eksiklik.

### Unsafe Bloklar

Yok.

### Nova Shield Chain İçin Dersler

- Bu test, Nova Shield Chain'in FROST entegrasyonu için başlangıç şablonu.
- **Eksik test:** Sadece t katılımcı ile (diğerleri absent) imzalama senaryosu test edilmeli.
- `Identifier::derive(&id.to_le_bytes())` yerine anlamlı identifier'lar (örn. validator pubkey hash'i) kullanılabilir.

---

## BÖLÜM 2: decaf377-ka (2 dosya)

---

## [src/lib.rs] (decaf377-ka)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-ka/src/lib.rs`
**Satır Sayısı:** 161

### Genel Amaç

decaf377 eğrisi üzerinde Diffie-Hellman anahtar anlaşması. "Çeşitlendirilebilir" (diversified) public key özelliği ile bir secret key'den birden fazla, birbirine bağlanamaz public key türetilebilir.

### Struct/Enum/Type Listesi

#### `Public` (Satır 17)
```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Public(pub [u8; 32]);
```
- 32-byte compressed decaf377 noktası. Geçersiz byte dizisi de kabul edilebilir (key agreement sırasında decompress edilir, o noktada hata verir).

#### `Secret` (Satır 21-22)
```rust
#[derive(Clone, Zeroize, PartialEq, Eq)]
#[zeroize(drop)]
pub struct Secret(decaf377::Fr);
```
- `decaf377::Fr` skalari tutar.
- `#[zeroize(drop)]`: Drop sırasında bellek sıfırlanır. Güvenli bellek yönetimi.

#### `SharedSecret` (Satır 25-27)
```rust
#[derive(PartialEq, Eq, Clone, Zeroize)]
#[zeroize(drop)]
pub struct SharedSecret(pub [u8; 32]);
```
- 32-byte paylaşılan sır. `pub` field — caller erişebilir.
- `#[zeroize(drop)]`: Drop sırasında sıfırlanır.

#### `Error` (Satır 30-38)
```rust
pub enum Error {
    #[error("Invalid public key")]
    InvalidPublic(Public),
    #[error("Invalid secret key")]
    InvalidSecret,
    #[error("Supplied bytes are incorrect length")]
    SliceLenError,
}
```

### Fonksiyonlar

#### `Secret::new<R: RngCore + CryptoRng>(rng: &mut R) -> Self` (Satır 42-44)
- `decaf377::Fr::rand(rng)` ile uniform rastgele scalar.

#### `Secret::new_from_field(sk: decaf377::Fr) -> Self` (Satır 52-54)
- Özel türetme için mevcut Fr değerini kullan. **Caller sorumluluğu:** Güvenli üretim garantisi.

#### `Secret::public(&self) -> Public` (Satır 58-60)
- `self.diversified_public(&decaf377::Element::GENERATOR)`
- Standart basepoint ile public key.

#### `Secret::diversified_public(&self, diversified_generator: &decaf377::Element) -> Public` (Satır 68-70)
- **Kritik özellik:** `Public((self.0 * diversified_generator).vartime_compress().into())`
- Farklı `diversified_generator` değerleri ile aynı secret key'den birden fazla unlinkable public key türetilebilir.
- Penumbra'da her "diversifier" → farklı basepoint → farklı public key → farklı adres.

#### `Secret::key_agreement_with(&self, other: &Public) -> Result<SharedSecret, Error>` (Satır 75-81)
- `decaf377::Encoding(other.0).vartime_decompress().map_err(...)?`
- `SharedSecret((self.0 * pk).vartime_compress().into())`
- ECDH hesaplama: `ss = sk * pk = sk * (peer_sk * G) = peer_sk * (sk * G)`

#### `Secret::to_bytes(&self) -> [u8; 32]` (Satır 84-88)
- `self.into()` — `From<&Secret> for [u8; 32]` implementasyonunu çağırır.

### Trait Implementasyonları

- `TryFrom<&[u8]> for Public` (Satır 119-126): Uzunluk kontrolü + [u8; 32] dönüşümü.
- `TryFrom<&[u8]> for Secret` (Satır 128-135): `bytes.try_into()` → `TryFrom<[u8; 32]>`.
- `TryFrom<[u8; 32]> for Secret` (Satır 137-143): `Fr::from_bytes_checked(...)` — geçerlilik kontrolü.
- `TryFrom<[u8; 32]> for SharedSecret` (Satır 145-154): **Decompress ile geçerlilik kontrolü** — geçersiz nokta reddedilir.
- `From<&Secret> for [u8; 32]` (Satır 156-160): `s.0.to_bytes()`.

### Debug Implementasyonları

- `Public::fmt`: `decaf377_ka::Public(hex_string)` — güvenli (public bilgi).
- `Secret::fmt`: `decaf377_ka::Secret(hex_string)` — **DİKKAT:** Debug ile secret key yazdırılabilir! Sadece geliştirme ortamında güvenli.
- `SharedSecret::fmt`: `decaf377_ka::SharedSecret(hex_string)` — **DİKKAT:** Shared secret de yazdırılabilir.

### Kriptografik Formüller/Protokol

**Standart ECDH:**
```
Alice: sk_a ∈ Fr (random)
Alice pk: pk_a = sk_a * G
Bob: sk_b ∈ Fr (random)
Bob pk: pk_b = sk_b * G

Alice ss: sk_a * pk_b = sk_a * sk_b * G
Bob ss:   sk_b * pk_a = sk_b * sk_a * G
→ Alice ss == Bob ss  ✓
```

**Diversified ECDH:**
```
Alice: sk_a ∈ Fr
Diversifier d: gen = hash_to_curve(d)
Diversified pk: pk_a_d = sk_a * gen

Bob: sk_b
Bob diversified pk: pk_b_d = sk_b * gen

Alice ss: sk_a * pk_b_d = sk_a * sk_b * gen
Bob ss:   sk_b * pk_a_d = sk_b * sk_a * gen
→ Equal  ✓

Ayrıca: pk_a_d1 ve pk_a_d2 birbirine bağlanamaz
(farklı gen'ler → farklı pk'lar, ama aynı underlying sk)
```

### Unsafe Bloklar

Yok.

### Testler

`tests/proptests.rs`'de proptest ile kapsanmış.

### Tasarım Kararları

1. **`Zeroize` + `#[zeroize(drop)]`:** Secret ve SharedSecret drop sırasında sıfırlanır. Side-channel ve memory forensics'e karşı.
2. **`vartime_decompress`:** Timing leak var, ama public key doğrulama sırasında kabul edilebilir (zaten public bilgi).
3. **`SharedSecret(pub [u8; 32])`:** Field `pub` — caller direkt `ss.0` erişebilir. Alternatif olarak getter metodu daha güvenli olurdu.
4. **Diversified public keys:** Aynı wallet farklı adresler sunabilir, bağlanamaz (unlinkable).

### Nova Shield Chain İçin Dersler

- **Diversified addresses:** Nova Shield Chain'in adres modeli bu pattern'i uygulayabilir — tek private key, birden fazla receiving address, linkability yok.
- `Zeroize` zorunlu: Nova Shield Chain'in her secret key ve shared secret tipi `Zeroize` derive etmeli.
- **`Debug` leakage:** Production'da `Secret` ve `SharedSecret`'in `Debug` impl'leri ya kaldırılmalı ya da değerler maskelenmeli.
- `SharedSecret.0` public field tehlikeli — wrapper metodu veya `AsRef<[u8]>` daha güvenli.

---

## [tests/proptests.rs] (decaf377-ka)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/decaf377-ka/tests/proptests.rs`
**Satır Sayısı:** 62

### Genel Amaç

Property-based testing ile ECDH ve diversified ECDH doğruluğunu rasgele inputlarla test eder.

### Stratejiler

- `fq_strategy()` (Satır 4-8): `[u8; 32]` → `Fq::from_le_bytes_mod_order` — uniform Fq elemanı.
- `fr_strategy()` (Satır 10-14): `[u8; 32]` → `Fr::from_le_bytes_mod_order` — uniform Fr skalar.

### Test'ler

#### `key_agreement_works` (Satır 18-32)
- `alice_sk` ve `bob_sk`: rastgele Fr skalar.
- `alice_ss = alice_sk.key_agreement_with(&bob_pk)` ve `bob_ss = bob_sk.key_agreement_with(&alice_pk)`.
- `assert_eq!(alice_ss, bob_ss)` — ECDH simetri özelliği.

#### `diversified_key_agreement_works` (Satır 34-61)
- `div1`, `div2`: rastgele Fq elemanı → `Element::encode_to_curve(div)` ile basepoint.
- Alice ve Bob'un iki farklı diversified basepoint üzerinde key agreement'ı test eder.
- `assert_eq!(alice_ss1, bob_ss1)` ve `assert_eq!(alice_ss2, bob_ss2)` — her div için simetri.

### Tasarım Kararları

1. **`from_le_bytes_mod_order` stratejisi:** Rassal byte dizisi → mod indirgeme → uniform distribution (neredeyse). Bu, proptest'in arbitrary olarak geçersiz Fr/Fq üretmesini engeller.
2. **`encode_to_curve`:** Hash-to-curve benzeri deterministic basepoint üretimi.

### Nova Shield Chain İçin Dersler

- Key agreement testleri proptest ile yazılmalı — manuel test yetersiz.
- Diversified key agreement için farklı basepoint'lerle simetri testi zorunlu.

---

## BÖLÜM 3: proof-params (10 dosya)

---

## [build.rs] (proof-params)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-params/build.rs`
**Satır Sayısı:** 271

### Genel Amaç

Groth16 ispat parametrelerini (proving key binary dosyaları) build sırasında yönetir. Git-LFS pointer dosyalarını tespit eder; `download-proving-keys` feature'ı aktifse GitHub LFS API üzerinden indirir.

### Enum'lar

#### `ProvingKeyFilepath` (Satır 75-100)
```rust
enum ProvingKeyFilepath {
    Absent(String),   // Dosya yok (crates.io build)
    Pointer(String),  // Git-LFS pointer (git-lfs yapılandırılmamış)
    Present(String),  // Gerçek binary key (git-lfs aktif)
}
```

**Tespit mantığı:** `file.len() < 500` bytes ise pointer (LFS pointer dosyaları ~130 byte).

### Fonksiyonlar

#### `main()` (Satır 12-41)

İzlenen dosyalar:
- `src/gen/output_pk.bin`, `spend_pk.bin`, `swap_pk.bin`, `swapclaim_pk.bin`, `convert_pk.bin`, `delegator_vote_pk.bin`, `nullifier_derivation_pk.bin`
- `src/gen/output_vk.param`, `spend_vk.param`, ... (7 doğrulama anahtarı)

`cargo:rerun-if-changed` direktifleri ve `handle_proving_key` çağrıları.

#### `handle_proving_key(file: &str) -> anyhow::Result<()>` (Satır 47-72)
- `ProvingKeyFilepath::new(file)` ile durumu tespit eder.
- `Present`: Devam et.
- `Absent`: Build hatası.
- `Pointer` + `download-proving-keys` feature: `download_proving_key`.
- `Pointer` - feature: Cargo warning.

#### `download_proving_key(filepath: &str) -> anyhow::Result<()>` (Satır 131-149)
- LFS pointer'ı parse et → GitHub LFS Batch API'den indir → SHA256 doğrula → dosyaya yaz.

#### `GitLFSPointer::parse(bytes: &[u8]) -> anyhow::Result<Self>` (Satır 173-213)
- Regex ile `oid sha256:HASH` ve `size N` çıkarımı.

#### `GitLFSPointer::resolve(&self) -> anyhow::Result<Vec<u8>>` (Satır 217-268)
- GitHub LFS Batch API endpoint: `https://github.com/penumbra-zone/penumbra.git/info/lfs/objects/batch`
- JSON body: `{"operation": "download", "transfer": ["basic"], "objects": [{"oid": "...", "size": N}]}`
- Response'dan `objects[0].actions.download.href` çıkarımı.
- **SHA256 doğrulama (Satır 258-265):** `Sha256::digest(&bytes)` → `sha256_str == self.oid`. Tamper-proof indirme.

### Güvenlik Analizi

- **SHA256 content integrity:** İndirilen key'in OID'si ile SHA256 hash'i karşılaştırılır. MITM saldırılarına karşı.
- **`unwrap()` kullanımı (Satır 178):** Build script'te `Regex::new` için — compile-time sabit pattern, panic imkansız.
- **Ağ bağımlılığı:** `download-proving-keys` feature ile build zamanında ağ gerekiyor. CI ortamında sorun olabilir.

### Tasarım Kararları

1. **`< 500 bytes` LFS tespiti:** Hacky ama pratik. LFS pointer'lar ~130 byte, gerçek key'ler MB boyutunda.
2. **Özellik flag'i:** `download-proving-keys` default kapalı — beklenmedik ağ trafiği önlenir.
3. **`cargo:rerun-if-changed`:** Build cache invalidation doğru yönetilmiş.

### Nova Shield Chain İçin Dersler

- Nova Shield Chain ZK devreleri büyük ispat anahtarları üretecek. Bu git-LFS + build-time indirme stratejisi doğrudan uygulanabilir.
- SHA256 content verification şart — key tampering kanıtlanamayan zararlar yaratır.
- `bundled-proving-keys` vs `download-proving-keys` iki ayrı deployment senaryosu için ideal.

---

## [src/lib.rs] (proof-params)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-params/src/lib.rs`
**Satır Sayısı:** 284

### Genel Amaç

7 Groth16 devresi için ispat/doğrulama anahtarlarını `Lazy<LazyProvingKey>` ve `Lazy<PreparedVerifyingKey>` static'leri olarak yönetir. `once_cell` ile thread-safe lazy initialization.

### Sabitler

```rust
pub const GROTH16_PROOF_LENGTH_BYTES: usize = 192;
```
Groth16 ispatı Bls12_377 üzerinde: 192 byte (iki G1 + bir G2 noktası, compressed).

### Struct'lar

#### `LazyProvingKey` (Satır 39-42)
```rust
pub struct LazyProvingKey {
    pk_id: &'static str,
    inner: OnceCell<ProvingKey<Bls12_377>>,
}
```
- `pk_id`: Beklenen anahtarın Bech32m kimliği (`groth16pk1...` formatı).
- `inner`: `OnceCell` — ilk kullanımda bir kez yüklenir, thread-safe.
- `fn new(pk_id: &'static str) -> Self` — `pub` değil, sadece bu modüldeki static'ler oluşturabilir.

### Fonksiyonlar

#### `LazyProvingKey::try_load(&self, bytes: &[u8]) -> Result<&ProvingKey<Bls12_377>>` (Satır 59-74)
```rust
self.inner.get_or_try_init(|| {
    let pk = ProvingKey::deserialize_uncompressed_unchecked(bytes)?;
    let pk_id = pk.debug_id();
    if pk_id != self.pk_id {
        bail!("proving key ID mismatch: expected {}, loaded {}", self.pk_id, pk_id);
    }
    Ok(pk)
})
```
- **ID doğrulama:** Yüklenen key'in SHA256 + Bech32m kimliği beklenenle karşılaştırılır.
- Zaten yüklüyse no-op.

#### `LazyProvingKey::try_load_unchecked(&self, bytes: &[u8]) -> Result<&ProvingKey<Bls12_377>>` (Satır 80-86)
- ID kontrolü atlanır. `bundled-proving-keys` feature'ında (güvenilir kaynak) kullanılır.

#### `Deref for LazyProvingKey` (Satır 89-95)
```rust
fn deref(&self) -> &Self::Target {
    self.inner.get().expect("Proving key cannot be loaded!")
}
```
- `Deref` ile `LazyProvingKey` doğrudan `ProvingKey<Bls12_377>` gibi kullanılabilir.
- `expect` ile panic — yüklenmeden kullanım programcı hatasıdır.

### Static Değişkenler

7 ispat anahtarı ve 7 doğrulama anahtarı static olarak tanımlanmış:

| Circuit | Proving Key Static | Verification Key Static |
|---|---|---|
| Spend | `SPEND_PROOF_PROVING_KEY` | `SPEND_PROOF_VERIFICATION_KEY` |
| Output | `OUTPUT_PROOF_PROVING_KEY` | `OUTPUT_PROOF_VERIFICATION_KEY` |
| Swap | `SWAP_PROOF_PROVING_KEY` | `SWAP_PROOF_VERIFICATION_KEY` |
| SwapClaim | `SWAPCLAIM_PROOF_PROVING_KEY` | `SWAPCLAIM_PROOF_VERIFICATION_KEY` |
| Convert | `CONVERT_PROOF_PROVING_KEY` | `CONVERT_PROOF_VERIFICATION_KEY` |
| DelegatorVote | `DELEGATOR_VOTE_PROOF_PROVING_KEY` | `DELEGATOR_VOTE_PROOF_VERIFICATION_KEY` |
| NullifierDerivation | `NULLIFIER_DERIVATION_PROOF_PROVING_KEY` | `NULLIFIER_DERIVATION_PROOF_VERIFICATION_KEY` |

**Örnek pattern:**
```rust
pub static SPEND_PROOF_PROVING_KEY: Lazy<LazyProvingKey> = Lazy::new(|| {
    let spend_proving_key = LazyProvingKey::new(spend::PROVING_KEY_ID);
    #[cfg(feature = "bundled-proving-keys")]
    spend_proving_key
        .try_load(include_bytes!("gen/spend_pk.bin"))
        .expect("bundled proving key is valid");
    spend_proving_key
});
```

Verification keys:
```rust
pub static SPEND_PROOF_VERIFICATION_KEY: Lazy<PreparedVerifyingKey<Bls12_377>> =
    Lazy::new(|| spend_verification_parameters().into());

fn spend_verification_parameters() -> VerifyingKey<Bls12_377> {
    let vk_params = include_bytes!("gen/spend_vk.param");
    VerifyingKey::deserialize_uncompressed_unchecked(&vk_params[..])
        .expect("can deserialize VerifyingKey")
}
```

**Dikkat:** `deserialize_uncompressed_unchecked` — trusted source (source code), runtime check yok.

### Modüller (gen/ dizini)

7 adet `*_id.rs` dosyası `include!` macro ile dahil edilir:
```rust
pub mod spend { include!("gen/spend_id.rs"); }
```
Her biri 2 sabit içerir: `PROVING_KEY_ID` ve `VERIFICATION_KEY_ID` (Bech32m formatında).

### Unsafe Bloklar

Yok (ancak `deserialize_uncompressed_unchecked` "unchecked" kelimesi semantik riskler taşır).

### Tasarım Kararları

1. **`Lazy<OnceCell>` kombinasyonu:** `Lazy` (once_cell) global initialization, `OnceCell` de içinde saklıyor — ikisi de thread-safe lazy init için ama farklı amaçlarla kullanılmış. Aslında `Lazy` içinde `OnceCell` kullanmak redundant görünüyor; `Lazy<ProvingKey>` yeterli olabilirdi. Fark: `try_load` API'si çalışma zamanında key yüklemeye izin veriyor.
2. **`include_bytes!` + `bundled-proving-keys`:** Compile-time binary gömme. Binary boyutu ~100MB artar.
3. **`PreparedVerifyingKey`:** Doğrulama anahtarı önceden hazırlanmış (miller loop precomputation). Her doğrulama işlemi daha hızlı.
4. **`#[deny(clippy::unwrap_used)]`:** `unwrap` yerine `expect` veya `?` zorunlu — build hatası.

### Nova Shield Chain İçin Dersler

- **Aynı pattern kullanılabilir:** `LazyProvingKey` yapısı Nova Shield Chain'in kendi ZK devreleri için direkt kullanılabilir.
- **Key ID doğrulaması şart:** `try_load`'daki `pk_id` kontrolü olmadan yanlış key yüklenmesi sessizce hatalı ispat üretir.
- **`PreparedVerifyingKey` optimizasyonu:** Validator node'larında doğrulama sık yapılır; `PreparedVerifyingKey` ile pairing hesabı önceden yapılır.
- Nova Shield Chain ~5-10 devre kullanacaksa aynı static yapı tek bir crate'e konulabilir.

---

## [src/gen/*_id.rs] (7 dosya)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-params/src/gen/`
**Her dosya 3 satır**

Her dosya sadece iki sabit içerir:

| Dosya | PROVING_KEY_ID | VERIFICATION_KEY_ID |
|---|---|---|
| spend_id.rs | `groth16pk1ke43yax...` | `groth16vk1zu2ks8...` |
| output_id.rs | `groth16pk1rkpwdlc...` | `groth16vk1lnuw3x...` |
| swap_id.rs | `groth16pk1wkfz5mc...` | `groth16vk1wj0hyk...` |
| swapclaim_id.rs | `groth16pk1k0qqzjg...` | `groth16vk1egnxuw...` |
| convert_id.rs | `groth16pk1tt28h0w...` | `groth16vk1l7qugu...` |
| delegator_vote_id.rs | `groth16pk1udsx8yf...` | `groth16vk1fmrl62...` |
| nullifier_derivation_id.rs | `groth16pk1g5vycm6...` | `groth16vk1szyv67...` |

**Format:** `groth16pk1` veya `groth16vk1` prefix + Bech32m encoded SHA256 hash.

### Tasarım Kararları

- ID hesaplama: `sha2::Sha256::digest(compressed_serialization)` → `bech32::encode("groth16pk"/"groth16vk", hash.to_base32(), Bech32m)`.
- Ayrı dosyalarda tutulmaları CI/CD pipeline'da key regeneration'ı kolaylaştırır — sadece `gen/` dizini değişir.

---

## [src/traits.rs] (proof-params)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-params/src/traits.rs`
**Satır Sayısı:** 109

### Genel Amaç

Groth16 parametre üretimi için yardımcı trait'ler ve fonksiyonlar. Devre kısıtlamaları üretimi, test parametreleri ve anahtar kimlik hesabı.

### Trait'ler

#### `DummyWitness: ConstraintSynthesizer<Bls12_377::ScalarField>` (Satır 12-19)
```rust
pub trait DummyWitness: ConstraintSynthesizer<<Bls12_377 as Pairing>::ScalarField> {
    fn with_dummy_witness() -> Self;
}
```
- Her devre tipi dummy witness değerleri ile kısıtlama sentezi yapabilmelidir.
- Setup seremonisi için: kısıtlama matrisleri witness değerlerinden bağımsız üretilmeli.

#### `VerifyingKeyExt for VerifyingKey<Bls12_377>` (Satır 70-85)
```rust
pub trait VerifyingKeyExt {
    fn debug_id(&self) -> String;
}
impl VerifyingKeyExt for VerifyingKey<Bls12_377> {
    fn debug_id(&self) -> String {
        let mut buf = Vec::new();
        self.serialize_compressed(&mut buf).expect("...");
        let hash = sha2::Sha256::digest(&buf);
        bech32::encode("groth16vk", hash.to_base32(), bech32::Variant::Bech32m)
            .expect("...")
    }
}
```

#### `ProvingKeyExt for ProvingKey<Bls12_377>` (Satır 93-108)
- Aynı pattern, prefix `"groth16pk"`.

### Fonksiyonlar

#### `generate_constraint_matrices<T: DummyWitness>() -> ConstraintMatrices<ScalarField>` (Satır 26-43)
- `T::with_dummy_witness()` ile devre oluştur.
- `cs.set_optimization_goal(Constraints)` + `cs.set_mode(Setup)`.
- `circuit.generate_constraints(cs.clone())` → `cs.to_matrices()`.
- **Setup ceremony için kritik:** Witness değerleri olmadan sadece kısıtlama matrisleri üretilir.

#### `generate_test_parameters<T: DummyWitness>(rng) -> (ProvingKey, VerifyingKey)` (Satır 49-59)
- `Groth16::<Bls12_377, LibsnarkReduction>::circuit_specific_setup(circuit, rng)`.
- **SADECE TEST İÇİN.** Randomness güvenle imha edilmemiş.

#### `generate_prepared_test_parameters<T: DummyWitness>(rng)` (Satır 63-68)
- `generate_test_parameters` + `vk.into()` ile `PreparedVerifyingKey`.

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **`LibsnarkReduction`:** Arkworks'ün Groth16 implementasyonunda QAP indirgeme stratejisi. libsnark ile uyumluluk.
2. **Test parametreleri için `expect`:** Build-time değil, test ortamında — panic kabul edilebilir.
3. **`debug_id` olarak Bech32m SHA256:** Anahtarları karşılaştırmak için human-readable format.

### Nova Shield Chain İçin Dersler

- `DummyWitness` trait'i Nova Shield Chain'in kendi devreleri için standart interface — her devre bu trait'i implement etmeli.
- `generate_test_parameters` ile hızlı test kurulumu; üretimde gerçek seremoni parametreleri kullanılmalı.
- `debug_id` ile anahtar doğrulama: CI pipeline'da `PROVING_KEY_ID == pk.debug_id()` assertion.

---

## BÖLÜM 4: proof-setup (10 dosya)

---

## [src/lib.rs] (proof-setup)
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/lib.rs`
**Satır Sayısı:** 6

```rust
pub mod all;
mod parallel_utils;
pub mod single;
```

### Nova Shield Chain İçin Dersler

Basit modül yapısı: `single` (tek devre başına), `all` (tüm devreler birlikte), `parallel_utils`.

---

## [src/parallel_utils.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/parallel_utils.rs`
**Satır Sayısı:** 60

### Genel Amaç

`rayon` feature flag ile paralel/seri işlem abstraction. Feature kapalıysa sequential, açıksa paralel.

### Fonksiyonlar

#### `transform<A, B, const N: usize>(data: [A; N], f: impl Fn(A) -> B) -> [B; N]` (Satır 1-6)
- Fixed-size array üzerinde map operasyonu.
- `data.into_iter().map(f).collect::<Vec<B>>().try_into()` — boyut değişmemeli, panic imkansız.

#### `transform_parallel<A, B, const N: usize>` (Satır 8-25)
- `#[cfg(not(feature = "parallel"))]`: `transform(data, f)` — sequential.
- `#[cfg(feature = "parallel")]`: `data.into_par_iter().map(f).collect()` — rayon ile paralel.

#### `flatten_results<A, E, const N: usize>(data: [Result<A, E>; N]) -> Result<[A; N], E>` (Satır 27-36)
- Result array'ini flatten eder. İlk hata ile erken çıkar.

#### `zip_map_parallel<A, B, C>` (Satır 38-59)
- İki mutable slice'ı zip ederek paralel map.
- `#[cfg(feature = "parallel")]`: `rayon::join` ile paralel.

### Tasarım Kararları

1. **`const N: usize` const generic:** Fixed-size array → compile-time boyut garantisi.
2. **Feature flag ile paralel:** `rayon` dependency optional tutulur. 7 devreyi paralel işlemek setup zamanını önemli ölçüde azaltır.

### Nova Shield Chain İçin Dersler

- Bu pattern doğrudan alınabilir. Ceremony için paralel işleme şart — tek devre setup bile dakikalar alabilir.

---

## [src/single/log.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/single/log.rs`
**Satır Sayısı:** 351

### Genel Amaç

Seremoni katkı logunu yönetir. Katkıların hash zinciri, doğrulama ve en son geçerli katkıyı bulma.

### Sabitler

```rust
pub const CONTRIBUTION_HASH_SIZE: usize = 32;
```

### Struct/Enum/Type Listesi

#### `ContributionHash` (Satır 8)
```rust
pub struct ContributionHash(pub [u8; CONTRIBUTION_HASH_SIZE]);
```
- `Clone + Copy + Debug + PartialEq + Eq + Hash`.
- `AsRef<[u8]>` impl: `&self.0`.
- `TryFrom<&[u8]>`: Uzunluk kontrolü.
- `dummy() -> Self`: `[0x1; 32]` — test için.

### Trait'ler

#### `Hashable` (Satır 37-39)
```rust
pub trait Hashable {
    fn hash(&self) -> ContributionHash;
}
```

#### `Phase` (Satır 42-75)
```rust
pub trait Phase {
    type CRSElements: Hashable;
    type RawContribution: Hashable;
    type Contribution: Hashable;

    fn parent_hash(contribution: &Self::RawContribution) -> ContributionHash;
    fn elements(contribution: &Self::Contribution) -> &Self::CRSElements;
    fn validate(root: &Self::CRSElements, contribution: &Self::RawContribution) -> Option<Self::Contribution>;
    fn is_linked_to(contribution: &Self::Contribution, elements: &Self::CRSElements) -> bool;
}
```
- Phase 1 ve Phase 2 için ortak abstraction.

### Struct'lar

#### `ContributionLog<P: Phase>` (Satır 81-83)
```rust
pub struct ContributionLog<P: Phase> {
    contributions: Vec<P::RawContribution>,
}
```

#### `ContributionLog::last_good_contribution` (Satır 97-149)
```rust
pub fn last_good_contribution(
    &self,
    root: &P::CRSElements,
    checkpoint: Option<ContributionHash>,
) -> Option<P::Contribution>
```

**Algoritma:**
```
başlangıç: last_good = None, start = 0
checkpoint varsa: log'da o hash'i bul → start noktasını oraya getir

her contribution c (start'tan itibaren):
  1. c.parent_hash == last_good.hash()  (veya root.hash()) değilse: atla
  2. P::validate(root, c) None ise: atla
  3. P::is_linked_to(c, last_good.elements()) False ise: atla
  4. last_good = Some(c)

return last_good
```

**Özellikler:**
- Geçersiz katkılar atlanır (parent yanlış, validate başarısız, veya linking başarısız).
- Checkpoint ile kısmen işlenmiş logdan devam edebilir.
- **Döngüsel olmayan hash chain:** Her katkı parent'ının hash'ini içerir → Merkle benzeri güven zinciri.

### Testler (Satır 152-349)

- `test_empty_log_search_returns_none`
- `test_single_valid_log_search`
- `test_multi_valid_log_search`
- `test_multi_valid_log_search_bad_root`
- `test_multi_log_with_skips_search`: Geçersiz, yanlış parent, bad linking katkılar arasından doğru son katkıyı bulur.
- `test_multi_log_with_skips_and_resuming_search`: Checkpoint ile devam.
- `test_multi_log_with_skips_and_bad_checkpoint`: Geçersiz checkpoint, yine de doğru sonuç.

DummyPhase ile mock test — gerçek kriptografi olmadan log mantığı test edilir.

### Tasarım Kararları

1. **Hash chain:** Her katkı parent hash'ini içerir → katkıların sırasının tamper-proof kaydı.
2. **Checkpoint:** Büyük seremoni loglarında (binlerce katkı) sıfırdan tarama yerine checkpoint'ten devam.
3. **`Phase` trait:** Phase 1 ve Phase 2 aynı log infrastructure paylaşır.

### Nova Shield Chain İçin Dersler

- Güvenilir kurulum seremonisi yapılacaksa bu tam log verification sistemi doğrudan kullanılabilir.
- Hash chain mantığı blockchain finalitesi için de ilham verir: her blok parent hash'ini içerir.

---

## [src/single/group.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/single/group.rs`
**Satır Sayısı:** 207

### Genel Amaç

Bls12_377 üzerinde pairing tabanlı işlemler için abstraction. Toplu pairing kontrolü (batched pairing check) ve grup elemanlarını hash eden `GroupHasher`.

### Type Alias'lar

```rust
pub type G1 = <Bls12_377 as Pairing>::G1
pub type G1Prepared = <Bls12_377 as Pairing>::G1Prepared
pub type G2 = <Bls12_377 as Pairing>::G2
pub type G2Prepared = <Bls12_377 as Pairing>::G2Prepared
pub type GT = PairingOutput<Bls12_377>
pub type F = <Bls12_377 as Pairing>::ScalarField
```

### Makro

#### `make_batched_pairing_checker!` (Satır 46-89)
İki pairing checker tipi üretir:

- `BatchedPairingChecker11`: Her iki tarafı da G1 olan pairing'ler için.
- `BatchedPairingChecker12`: Sol G1, sağ G2 olan pairing'ler için.

**Amacı:** `e(a_i, base_l) == e(base_r, b_i)` şeklinde çoklu pairing eşitliğini toplu (batch) kontrol etmek.

**`check<R: CryptoRngCore>(&self, rng: &mut R) -> bool` (Satır 76-87):**
```
n = |vary_l|
scalars = [rand_small_f() for _ in 0..n]  // 128-bit random
l = MSM(vary_l, scalars)   // multi-scalar multiplication
r = MSM(vary_r, scalars)   // multi-scalar multiplication
return pairing(l, base_l) == pairing(base_r, r)
```

**Neden güvenli?**
Rastgele lineer kombinasyon: `e(Σ s_i * a_i, base_l) == e(base_r, Σ s_i * b_i)`
Her `i` için `e(a_i, base_l) == e(base_r, b_i)` doğruysa toplu eşitlik de doğru.
Tersine, tek bir `i` için yanlışsa, rastgele `s_i` seçimi ile hata yaklanır (2^-128 olasılıkla).

### GroupHasher (Satır 127-206)

BLAKE2b tabanlı, grup elemanlarını güvenli şekilde hash eden yardımcı:

- `new(personalization: &'static [u8]) -> Self`: BLAKE2b state'i ile başlat.
- `eat_bytes(&mut self, x: &[u8])`: **Length-prefixed** byte tüketme. Injection'a karşı.
- `eat_canonical<T: CanonicalSerialize>(&mut self, x: &T)`: `x.compressed_size()` ile length prefix.
- `eat_usize(&mut self, x: usize)`: `(x as u64).to_le_bytes()` olarak yaz.
- `eat_g1`, `eat_g2`, `eat_f`: `eat_canonical` çağrıları.
- `finalize(self) -> F`: BLAKE2b sonucu → `F::from_le_bytes_mod_order`.
- `finalize_bytes(self) -> Hash`: İlk 32 byte.

**Kritik özellik:** `eat_bytes` length prefix kullanır → `[a, b]` ve `[ab]` hash'leri farklı.

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **Batched pairing check:** Tek tek pairing yerine toplu → `2n` pairing yerine `~n/2` (MSM amortize edilmiş).
2. **128-bit random scalars:** Full-size scalar değil, küçük random → MSM daha hızlı; güvenlik hâlâ 128-bit.
3. **`rand_small_f`:** 16 byte random → `from_le_bytes_mod_order` → uniform küçük F değeri.
4. **`write_len` + `eat_bytes` ayrımı:** API'yi anlamsal olarak net kılar — metadata vs veri.

### Nova Shield Chain İçin Dersler

- Batched pairing checker validator node'larında toplu ispat doğrulaması için kritik performans optimizasyonu.
- `GroupHasher` pattern'i: ceremony hash'lemede domain separation garanti eder — Nova Shield Chain setup kodunda aynı yapı kullanılabilir.

---

## [src/single/dlog.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/single/dlog.rs`
**Satır Sayısı:** 141

### Genel Amaç

Schnorr tabanlı ayrık logaritma bilgi ispatı (Sigma protokolü). Ceremony'de her katılımcının kendi katkısını bildiğini kanıtlaması için.

### Struct'lar

#### `Statement` (Satır 11-14)
```rust
pub struct Statement {
    pub result: G1,
    pub base: G1,
}
```
- `result = dlog * base` olduğunu ispat edeceğiz.

#### `Witness` (Satır 17-19)
```rust
pub struct Witness {
    pub dlog: F,
}
```

#### `Proof` (Satır 23-27)
```rust
pub struct Proof {
    big_k: G1,  // nonce taahhüdü: k * base
    s: F,       // yanıt: k + e * dlog
}
```
`CanonicalSerialize + CanonicalDeserialize`. `hash() -> Hash` ile hashelenebilir.

### Fonksiyonlar

#### `challenge(ctx: &[u8], statement: &Statement, big_k: &G1) -> F` (Satır 42-49)
- Fiat-Shamir hash:
```
GroupHasher("PC$:proof_chal")
  .eat_bytes(ctx)
  .eat_g1(&statement.result)
  .eat_g1(&statement.base)
  .eat_g1(big_k)
  .finalize()
```

#### `prove<R: CryptoRngCore>(rng, ctx, statement, witness) -> Proof` (Satır 58-72)
```
k = F::rand(rng)
big_k = statement.base * k      // nonce taahhüdü
e = challenge(ctx, statement, big_k)
s = k + e * witness.dlog        // yanıt
return Proof { big_k, s }
```

#### `verify(ctx: &[u8], statement: Statement, proof: &Proof) -> bool` (Satır 82-85)
```
e = challenge(ctx, statement, proof.big_k)
return statement.base * proof.s == proof.big_k + statement.result * e
```
Matematiksel kontrol: `base * (k + e*dlog) == k*base + e*(dlog*base)`  ✓

### Protokol Matematiği (Sigma/Schnorr)

```
Hedef: dlog bilgisi ispatı — result = dlog * base

Prover:
  k ← random
  K = k * base                     // taahhüt
  e = H(ctx || result || base || K) // Fiat-Shamir challenge
  s = k + e * dlog                 // yanıt
  Proof = (K, s)

Verifier:
  e = H(ctx || result || base || K)
  Kontrol: base * s == K + e * result

Güvenlik: dlog bilinmeden s hesaplanamaz (DLOG varsayımı altında)
Sıfır bilgi: K rastgele seçildiği için (s, K) ifşa etmez
```

### Testler (Satır 88-141)

- `test_proof_happy_path`: Standart prove+verify.
- `test_different_big_k_makes_proof_fail`: Değiştirilmiş K redded edilir.
- `test_different_s_makes_proof_fail`: Değiştirilmiş s reddedilir.
- `test_different_ctx_makes_proof_fail`: Yanlış context reddedilir — domain separation çalışıyor.
- `test_bad_statement_makes_proof_fail`: Yanlış statement reddedilir.

### Unsafe Bloklar

Yok.

### Tasarım Kararları

1. **Context string:** Her kullanım yeri farklı ctx (`b"phase1 alpha proof"`, vb.) — farklı ceremony aşamaları arasında proof geçişi engellenir.
2. **Fiat-Shamir:** Non-interactive Schnorr. Etkileşimsiz, grup elemanlarını ve ctx'i hash'e dahil eder.
3. **`GroupHasher("PC$:proof_chal")`:** 16-byte personalization sınırına uygun.

### Nova Shield Chain İçin Dersler

- Bu NIZK proof sistemi Nova Shield Chain'in ceremony'sinde doğrudan kullanılabilir.
- **Context string zorunlu:** Aynı ispat farklı bağlamlarda geçerli olmamalı — her proof tipi için farklı ctx.

---

## [src/single/phase1.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/single/phase1.rs`
**Satır Sayısı:** 722

### Genel Amaç

Powers of Tau (Phase 1) ceremony. Gizli skalar `x` için `[x^i * G1]` ve `[α, β, x, αx, βx]` grup elemanlarını üretir.

### Struct'lar

#### `RawCRSElements` (Satır 37-46)
```rust
pub struct RawCRSElements {
    pub alpha_1: G1,          // [α]₁
    pub beta_1: G1,           // [β]₁
    pub beta_2: G2,           // [β]₂
    pub x_1: Vec<G1>,         // [xⁱ]₁, i ∈ [0, 2d-2]
    pub x_2: Vec<G2>,         // [xⁱ]₂, i ∈ [0, d-1]
    pub alpha_x_1: Vec<G1>,   // [αxⁱ]₁, i ∈ [0, d-1]
    pub beta_x_1: Vec<G1>,    // [βxⁱ]₁, i ∈ [0, d-1]
}
```

**Uzunluk kuralları (degree d için):**
- `x_1`: `long_len(d) = 2d-1` eleman
- `x_2`, `alpha_x_1`, `beta_x_1`: `short_len(d) = d` eleman

#### `CRSElements` (Satır 261-265)
```rust
pub struct CRSElements {
    pub(crate) degree: usize,
    pub(crate) raw: RawCRSElements,
}
```
- Doğrulanmış (validated) CRS.

#### `LinkingProof` (Satır 304-308)
```rust
pub(crate) struct LinkingProof {
    alpha_proof: dlog::Proof,
    beta_proof: dlog::Proof,
    x_proof: dlog::Proof,
}
```
- Yeni katkının önceki CRS'ye bağlı olduğunu kanıtlar.

### Önemli Fonksiyonlar

#### `RawCRSElements::validate(self) -> anyhow::Result<CRSElements>` (Satır 75-138)

Doğrulama adımları:
1. Geçerli degree çıkarımı.
2. Sıfır olmayan temel elemanlar: `alpha_1`, `beta_1`, `beta_2`, `x_1[1]`, `x_2[1]`.
3. `BatchedPairingChecker12`: `beta_1 ↔ beta_2` ve `x_1[i] ↔ x_2[i]` eşleşmesi.
4. `BatchedPairingChecker12`: `alpha_x_1[i]` ve `alpha_1` ile `x_2[i]` bağlantısı.
5. `BatchedPairingChecker12`: `beta_x_1[i]` ve `beta_1` ile `x_2[i]` bağlantısı.
6. `BatchedPairingChecker11`: `x_1[i]` ardışık üs kontrolü.

Paralel çalıştırma: 4 checker `transform_parallel` ile eş zamanlı.

Kod yorumunda ilginç not (Satır 113): `// "神だけが私を裁ける"` — "Only God can judge me" (Tupac'tan alıntı).

#### `CRSElements::root(d: usize) -> Self` (Satır 275-289)
- Tüm gizli değerler 1 olarak başlatılır: `G1::generator()` her yerde.
- **Production'da direkt kullanılmaz** — bu logical başlangıç noktası.

#### `Contribution::make<R: CryptoRngCore>(rng, parent, old) -> Self` (Satır 391-486)

```
alpha, beta, x ← random Fr

alpha_proof = dlog::prove(b"phase1 alpha proof", {result: old.alpha_1 * alpha, base: old.alpha_1}, {alpha})
beta_proof  = dlog::prove(b"phase1 beta proof",  {result: old.beta_1 * beta,   base: old.beta_1},  {beta})
x_proof     = dlog::prove(b"phase1 x proof",     {result: old.x_1[1] * x,     base: old.x_1[1]},  {x})

tweaks: x_i_tweaks[i] = xⁱ, alpha_x_i_tweaks[i] = alpha*xⁱ, beta_x_i_tweaks[i] = beta*xⁱ

new_x_1[i]     = old.x_1[i]     * x_i_tweaks[i]
new_x_2[i]     = old.x_2[i]     * x_i_tweaks[i]
new_alpha_x_1[i] = old.alpha_x_1[i] * alpha_x_i_tweaks[i]
new_beta_x_1[i]  = old.beta_x_1[i]  * beta_x_i_tweaks[i]
new_alpha_1 = old.alpha_1 * alpha
new_beta_1  = old.beta_1  * beta
new_beta_2  = old.beta_2  * beta
```

#### `Contribution::is_linked_to(&self, parent: &CRSElements) -> bool` (Satır 490-530)
- Degree kontrolü.
- 3 linking proof doğrulaması (alpha, beta, x için DLog).

### Powers of Tau Matematiği

```
Phase 1 Sonuç (n katılımcıdan sonra):
  x_final = x_1 * x_2 * ... * x_n  (kimse tam x_final'i bilmez)

CRS:
  [1]₁, [x_final]₁, [x_final²]₁, ..., [x_final^(2d-2)]₁
  [x_final]₂, ..., [x_final^(d-1)]₂
  [α_final]₁, [β_final]₁, [β_final]₂
  [α_final * x_final^i]₁, [β_final * x_final^i]₁

Güvenlik: En az bir katılımcı kendi gizli değerini güvenle imha ederse
          x_final hiçbir zaman bilinmez.
```

### Testler (Satır 564-721)

- `test_root_crs_is_valid`: Root CRS geçerli.
- `test_nontrivial_crs_is_valid`: Rastgele CRS geçerli.
- `test_changing_alpha/beta_makes_crs_invalid`: Tutarsız alpha/beta → hata.
- `test_setting_zero_elements_makes_crs_invalid`: Sıfır değerler → hata.
- `test_bad_powers_of_x_makes_crs_invalid`: Yanlış x üsleri → hata.
- `test_contribution_produces_valid_crs`, `test_contribution_is_linked_to_parent`, vb.

### Tasarım Kararları

1. **`checked_deserialize_parallel`:** `rayon` feature ile grup eleman validasyonu paralel yapılır.
2. **Batch checker paralel çalıştırma:** 4 checker eş zamanlı → doğrulama ~4x hızlı.
3. **Japonca yorum:** Geliştiricinin mizahı — ciddi kriptografi kodunda.

---

## [src/single/phase2.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/single/phase2.rs`
**Satır Sayısı:** 482

### Genel Amaç

Circuit-specific setup (Phase 2). `delta` gizli değeri ile `p_i` ve `t` polinomları üzerine `1/delta` çarpar. Bu sayede ispat sisteminin malleability'si ortadan kalkar.

### Struct'lar

#### `RawCRSElements` (Satır 15-20)
```rust
pub struct RawCRSElements {
    pub delta_1: G1,           // [δ]₁
    pub delta_2: G2,           // [δ]₂
    pub inv_delta_p_1: Vec<G1>, // [p_i / δ]₁
    pub inv_delta_t_1: Vec<G1>, // [t(x)xⁱ / δ]₁
}
```

#### `Contribution::make` (Satır 241-275)
```
delta ← random Fr
delta_inv = delta.inverse().expect(...)  // ~1/2^256 olasılıkla 0

new.delta_1 *= delta
new.delta_2 *= delta
for each inv_delta_p_i: inv_delta_p_i *= delta_inv  // p_i / delta
for each inv_delta_t_i: inv_delta_t_i *= delta_inv  // t_i / delta

linking_proof = dlog::prove(b"phase2 delta proof", ...)
```

#### `RawCRSElements::validate` (Satır 24-56)
- Uzunluk eşleşmesi ile root kontrolü.
- Sıfır olmayan `delta_1`, `delta_2`.
- Tek `BatchedPairingChecker11`:
  - `e(G1::generator(), delta_1) == e(delta_2, G1::generator())` → delta tutarlılığı.
  - `e(inv_delta_p_i, p_i) == e(delta_2, G1::generator())` → `inv_delta_p_i = p_i / delta`.
  - `e(inv_delta_t_i, t_i) == e(delta_2, G1::generator())` → `inv_delta_t_i = t_i / delta`.

### Phase 2 Matematiği

```
Phase 1 sonrası mevcut:
  p_i(x) = β*u_i(x) + α*v_i(x) + w_i(x)  değerlendirilmiş [p_i]₁
  t(x) * x^i değerlendirilmiş [t_i]₁

Phase 2 her katkı k:
  δ_k ← random
  Yeni [δ]₁ = eski [δ]₁ * δ_k
  Yeni [δ]₂ = eski [δ]₂ * δ_k
  Yeni [p_i/δ]₁ = eski [p_i/δ]₁ * (1/δ_k) = [p_i / (δ_1 * δ_2 * ... * δ_k)]₁

Son Groth16 CRS:
  [δ]₁, [δ]₂ — kök sır (kimse bilmiyor)
  [p_i/δ]₁ — witness katsayıları / delta
  [t_i/δ]₁ — vanishing polinom / delta
```

### Tasarım Kararları

1. **`delta_inv = delta.inverse().expect(...)`:** Sıfır inversiyonu panik — ama olasılık 1/2^256. "e.w. negligible probability" yorumu ile belgelenmiş.
2. **Linking proof:** Phase 2'de sadece bir DLog ispatı (delta için), Phase 1'de üç (alpha, beta, x için).

---

## [src/single/mod.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/single/mod.rs`
**Satır Sayısı:** 385

### Genel Amaç

Phase 1 → Phase 2 geçişi ve Phase 1+2 çıktısından Groth16 `ProvingKey` üretimi.

### Struct'lar

#### `ExtraTransitionInformation` (Satır 39-49)
```rust
pub struct ExtraTransitionInformation {
    u_1: Vec<G1>,   // u polinomlar [x]₁'de değerlendirilmiş
    v_1: Vec<G1>,   // v polinomlar [x]₁'de
    v_2: Vec<G2>,   // v polinomlar [x]₂'de
    p_1: Vec<G1>,   // p = β*u + α*v + w polinomlar [x]₁'de
}
```
- Phase 1'den Phase 2'ye geçişte elde edilen ve `combine`'de kullanılan ara bilgi.

### Fonksiyonlar

#### `circuit_degree(circuit: &ConstraintMatrices<F>) -> Result<usize>` (Satır 54-58)
```
circuit_size = num_constraints + num_instance_variables
Radix2EvaluationDomain::compute_size_of_domain(circuit_size)
```

#### `transition(phase1: &CRSElements, circuit: &ConstraintMatrices<F>) -> Result<(ExtraTransitionInformation, Phase2CRSElements)>` (Satır 64-258)

Bu fonksiyon son derece kapsamlı açıklanmış (Satır 68-141 yorumlar). Özet:

```
1. FFT ile Lagrange katsayıları:
   lagrange_1[j] = L_j(x) cinsinden [x]₁   (IFFT over x_1)
   lagrange_2[j] = L_j(x) cinsinden [x]₂   (IFFT over x_2)
   alpha_lagrange_1[j] = α*L_j(x) cinsinden [αx]₁  (IFFT over alpha_x_1)
   beta_lagrange_1[j]  = β*L_j(x) cinsinden [βx]₁   (IFFT over beta_x_1)

2. QAP polinomları:
   p_i = β*u_i + α*v_i + w_i değerlendirmesi

   identity matrix block (malleability için):
   p[0..num_instance_vars] += beta_lagrange_1[constraint_count..]

   R1CS kısıtlamalarından:
   for each constraint i:
     for (coeff, j) in A[i]: p_1[j] += beta_lagrange_1[i] * coeff; u_1[j] += ...
     for (coeff, j) in B[i]: p_1[j] += alpha_lagrange_1[i] * coeff; v_1[j] += ...; v_2[j] += ...
     for (coeff, j) in C[i]: p_1[j] += lagrange_1[i] * coeff

3. Vanishing polinom t:
   t[i] = x_1[i + domain_size] - x_1[i]
   (t(x)*x^i = x^(i+d) - x^i, domain üzerinde sıfırlanan polinom)

Phase 2 root:
   delta_1 = G1::generator()  (δ = 1 başlangıçta)
   delta_2 = G2::generator()
   inv_delta_p_1 = p_1
   inv_delta_t_1 = t
```

#### `combine(circuit, phase1out, phase2out, extra) -> ProvingKey<Bls12_377>` (Satır 263-288)

```rust
let vk = VerifyingKey {
    alpha_g1: phase1out.raw.alpha_1.into_affine(),   // [α]₁
    beta_g2:  phase1out.raw.beta_2.into_affine(),    // [β]₂
    gamma_g2: G2::generator().into_affine(),         // [γ]₂ = generator (γ=1)
    delta_g2: phase2out.raw.delta_2.into_affine(),   // [δ]₂
    gamma_abc_g1: normalize(extra.p_1[..num_instance_variables]),  // [γ⁻¹*p_i]₁ for public inputs
};
ProvingKey {
    vk,
    beta_g1:     phase1out.raw.beta_1.into_affine(),   // [β]₁
    delta_g1:    phase2out.raw.delta_1.into_affine(),  // [δ]₁
    a_query:     normalize(&extra.u_1),                 // [u_i]₁
    b_g1_query:  normalize(&extra.v_1),                 // [v_i]₁
    b_g2_query:  normalize(&extra.v_2),                 // [v_i]₂
    h_query:     normalize(&phase2out.raw.inv_delta_t_1), // [t_i/δ]₁
    l_query:     normalize(&phase2out.raw.inv_delta_p_1[num_instance_variables..]), // [p_i/δ]₁ for witnesses
}
```

### Test — `test_can_generate_keys_through_ceremony` (Satır 330-384)

Tam end-to-end ceremony testi:
1. `TestCircuit { x, y, x_plus_y }` — `x + y == x_plus_y` kısıtlaması.
2. Phase 1 root → 2 contribution → transition → Phase 2 root → 2 contribution → combine → ProvingKey.
3. `Groth16::prove(pk, circuit, rng)` → `Groth16::verify(vk, public_inputs, proof)`.

### Tasarım Kararları

1. **IFFT ile Lagrange polinomları:** Quadratic yerine `O(n log n)` hesaplama.
2. **Identity matrix (malleability fix):** Groth16 malleability'yi önlemek için referans link `https://geometry.xyz/notebook/groth16-malleability`.
3. **`G2::generator()` for gamma:** Penumbra γ=1 kullanır — gamma için ayrı ceremony yok. Bu güvenlik açısından kabul edilebilir ama bazı Groth16 varyantlarından farklı.

### Nova Shield Chain İçin Dersler

- **Tam ceremony pipeline:** Phase1 → transition → Phase2 → combine — bu 4 adım Nova Shield Chain'in ZK setup'ı için yeterli.
- **Malleability fix şart:** Groth16 malleability olmadan kötü niyetli prover proof'ları değiştirebilir.
- FFT tabanlı Lagrange hesabı: `arkworks` bunu sağlar, özel implementasyon gerekmiyor.
- `gamma = 1` seçimi: Basitleştirme, ama bazı optimizasyonlar (özellikle gamma ABC hesabı) etkilenebilir.

---

## [src/all.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/src/all.rs`
**Satır Sayısı:** 758

### Genel Amaç

7 devrenin tümü için Phase 1 ve Phase 2 ceremony'sini tek bir API'de birleştirir. Protobuf serialize/deserialize ile network koordinasyonu.

### Sabitler

```rust
pub const NUM_CIRCUITS: usize = 7;
const SERIALIZATION_COMPRESSION: Compress = Compress::No;
```

- 7 devre: Spend, Output, DelegatorVote, Convert, Swap, SwapClaim, NullifierDerivation.
- Sıkıştırmasız serializasyon: Ceremony'de dosya boyutu ikincil, hız önemli.

### Struct'lar

#### Phase 1
- `Phase1RawCeremonyCRS([Phase1RawCRSElements; 7])` — doğrulanmamış
- `Phase1CeremonyCRS([Phase1CRSElements; 7])` — doğrulanmış (`PartialEq`)
- `Phase1RawCeremonyContribution([Phase1RawContribution; 7])`
- `Phase1CeremonyContribution([Phase1Contribution; 7])`

#### Phase 2
- `Phase2RawCeremonyCRS([Phase2RawCRSElements; 7])` — doğrulanmamış
- `Phase2CeremonyCRS([Phase2CRSElements; 7])` — doğrulanmış
- `Phase2RawCeremonyContribution([Phase2RawContribution; 7])`
- `Phase2CeremonyContribution([Phase2Contribution; 7])`

#### `AllExtraTransitionInformation([ExtraTransitionInformation; 7])` (Satır 719)
- Geçiş bilgisi tüm devreler için saklanır.

### Önemli Fonksiyonlar

#### `circuits() -> [ConstraintMatrices<F>; NUM_CIRCUITS]` (Satır 61-71)
```rust
[
    generate_constraint_matrices::<SpendCircuit>(),
    generate_constraint_matrices::<OutputCircuit>(),
    generate_constraint_matrices::<DelegatorVoteCircuit>(),
    generate_constraint_matrices::<ConvertCircuit>(),
    generate_constraint_matrices::<SwapCircuit>(),
    generate_constraint_matrices::<SwapClaimCircuit>(),
    generate_constraint_matrices::<NullifierDerivationCircuit>(),
]
```

#### `Phase1CeremonyCRS::root() -> Result<Self>` (Satır 482-494)
- Her devre için `Phase1CRSElements::root(circuit_degree(&c)?)`.

#### `Phase2CeremonyContribution::make(old: &Phase2CeremonyCRS) -> Self` (Satır 372-379)
- `transform_parallel` ile 7 devreyi paralel işle.

#### `Hashable for Phase2CeremonyContribution::hash()` (Satır 382-391)
```rust
fn hash(&self) -> ContributionHash {
    let hashes = transform(self.0.clone(), |x| x.hash());
    let mut hasher = GroupHasher::new(b"phase2contr");
    for h in hashes {
        hasher.eat_bytes(h.as_ref());
    }
    ContributionHash(hasher.finalize_bytes())
}
```
Tüm devrelerin hash'lerinin hash'i — combined ceremony hash.

#### `transition(phase1: &Phase1CeremonyCRS) -> Result<(AllExtraTransitionInformation, Phase2CeremonyCRS)>` (Satır 732-745)
- 7 devre için paralel `single::transition` çağrısı.

#### `combine(phase1out, phase2out, extra) -> [ProvingKey<Bls12_377>; NUM_CIRCUITS]` (Satır 747-757)
- 7 devre için paralel `single::combine` çağrısı.

### Protobuf Dönüşümleri

`pb::CeremonyCrs`:
```protobuf
message CeremonyCrs {
  bytes spend, output, delegator_vote, undelegate_claim, swap, swap_claim, nullifer_derivation_crs;
}
```

`Phase2RawCeremonyCRS::unchecked_from_protobuf` ve `TryFrom<pb::CeremonyCrs>`:
- Checked vs unchecked: `Validate::Yes` vs `Validate::No`.

`Phase2RawCeremonyContribution::TryFrom<pb::participate_request::Contribution>`:
- `transform_parallel` ile 7 devreyi paralel deserialize.
- Hem checked hem unchecked varyant mevcut.

### Tasarım Kararları

1. **Paralel işleme zorunlu:** 7 büyük devre → Phase 1 validation başına dakikalar. `rayon` feature ile paralel.
2. **`Compress::No`:** Network transferde paket boyutu önemli ama zaman daha kritik. Decompression CPU maliyeti var.
3. **`checked` vs `unchecked_from_protobuf`:** Güvenilir kaynaktan (DB) vs ağdan gelen veriler farklı işlenir.
4. **`array::from_fn`:** Const generic array oluşturma — `[0; 7]` yerine closure ile.

### Nova Shield Chain İçin Dersler

- Nova Shield Chain'in kendi devreleri eklenirken `NUM_CIRCUITS` sabiti güncellenmeli ve `circuits()` fonksiyonu genişletilmeli.
- Paralel ceremony katılımcı client kodu `rayon` olmadan kullanılamaz — **`parallel` feature zorunlu** production'da.
- `pb::CeremonyCrs` protobuf mesajı network koordinasyonu için yeterli — gRPC stream üzerinden ceremony koordinasyonu bu struct'larla mümkün.

---

## [benches/all.rs]
**Dosya:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/crypto/proof-setup/benches/all.rs`
**Satır Sayısı:** 73

### Genel Amaç

Ceremony işlemlerinin Criterion benchmarkları: Phase 1 çalıştırma, Phase 1 doğrulama, Phase geçişi, Phase 2 çalıştırma, Phase 2 doğrulama.

### Benchmark Fonksiyonları

| Fonksiyon | Ölçülen İşlem |
|---|---|
| `run_phase1(old)` | `Phase1CeremonyContribution::make(old)` |
| `check_phase1(contribution, parent)` | validate + is_linked_to |
| `run_phase_transition(phase1)` | Phase 1 → Phase 2 geçişi (FFT dahil) |
| `run_phase2(old)` | `Phase2CeremonyContribution::make(old)` |
| `check_phase2(contribution, root, parent)` | validate + is_linked_to |

**Konfigürasyon:** `sample_size(10)` — ceremony çok yavaş, az sample yeterli.

### Tasarım Kararları

1. **`sample_size(10)`:** Phase 1 tek katkı dakikalar sürebilir. Criterion default 100 sample ile ölçmek saatler alır.
2. **Root'tan başlama:** `Phase1CeremonyCRS::root().unwrap()` — benchmark setup'ı seed ile deterministik değil.

### Nova Shield Chain İçin Dersler

- Ceremony benchmarkları development ortamında mandatory — "bu devre çok büyük" gibi sorunları erken tespit eder.
- `sample_size(10)` yeterli; ceremony bir kez yapılır, throughput değil latency önemli.

---

## GENEL SENTEZ VE ÖZET TABLO

### 4 Kriptografi Crate'inin Rolü

| Crate | Rol | Önem |
|---|---|---|
| `decaf377-frost` | Eşik imzalama (t-of-n) + DKG | Validator key management |
| `decaf377-ka` | ECDH anahtar anlaşması | Şifreli not iletişimi, memo key |
| `proof-params` | Groth16 anahtar yükleme/doğrulama | Her ZK ispat için temel |
| `proof-setup` | Güvenilir kurulum seremonisi | Yeni devre eklendiğinde veya başlangıçta |

### Kriptografik Özellikler Özeti

| Özellik | Algoritma | Güvenlik Düzeyi |
|---|---|---|
| Eşik imzalama | FROST (Schnorr tabanlı) | 128-bit (decaf377 üzerinde) |
| Anahtar anlaşma | ECDH (decaf377) | 128-bit |
| ZK ispat sistemi | Groth16 (BLS12-377) | 128-bit |
| Hash (genel) | BLAKE2b-512 | 256-bit preimage |
| Hash (FROST H2) | BLAKE2b-512 "decaf377-rdsa---" | 256-bit |
| Hash (ceremony) | BLAKE2b "PC$:..." | 128-bit output |
| DLog ispatı | Sigma (Schnorr), Fiat-Shamir | DLOG varsayımı |
| Pairing kontrolü | BLS12-377 e: G1×G2→GT | 128-bit |

### Güvenlik Bulguları

1. **`Secret::fmt` leakage:** `decaf377-ka`'da Secret ve SharedSecret Debug ile yazdırılabilir — production loglarında bu kapatılmalı.
2. **`SharedSecret.0` public field:** Doğrudan erişim mümkün; zeroize sonrası erişim UB olabilir. Getter metodu daha güvenli.
3. **`vartime_` decompress:** Timing leak olası ama public değerler için kabul edilebilir.
4. **FROST nonce reuse:** Kod düzeyinde korunma yok — servis katmanında zorunlu.
5. **`reconstruct` zayıflığı:** Yeterli pay olmadan çağrıldığında hata vermez, yanlış key döner.
6. **`deserialize_uncompressed_unchecked`:** Trusted source varsayımı ile kullanılıyor — kaynak code, CI/CD'de key integrity doğrulaması şart.

### Nova Shield Chain İçin Kritik Eylem Planı

#### Direkt Kullanılabilecekler

1. **FROST implementasyonu:** `decaf377-frost` crate'i, `cggmp21` yerine veya yanı sıra eşik imzalama için.
2. **`LazyProvingKey` pattern:** Nova Shield Chain'in kendi devreleri için aynı static lazy loading.
3. **Ceremony pipeline:** Phase 1 → transition → Phase 2 → combine tam akışı.
4. **`GroupHasher`:** Domain-separated hashing için.
5. **Batched pairing checker:** Toplu ZK ispat doğrulaması için.
6. **DLog NIZK:** MPC ceremony'de aynı Sigma protokolü.
7. **`ContributionLog` + `Phase` trait:** Ceremony katkı logunu takip etmek için.
8. **Build-time key management:** `build.rs` LFS + SHA256 doğrulama stratejisi.

#### Nova Shield Chain'de Farklı Yapılması Gerekenler

1. **`Secret::fmt` maskele:** `"decaf377_ka::Secret([REDACTED])"` — production'da key log'lanmaz.
2. **`SharedSecret.0` gizle:** `fn as_bytes(&self) -> &[u8]` getter, `pub` field değil.
3. **FROST nonce tracking:** Servis katmanında nonce state machine — kullanılmış nonce'lar kayıt altında.
4. **`reconstruct` guard:** `key_packages.len() >= min_signers` assertion ekle.
5. **`cggmp21` entegrasyonu:** Penumbra'nın `decaf377-frost`'u `frost_core` üzerine kurulu; Nova Shield Chain `cggmp21` ile secp256k1 veya P-256 kullanabilir — aynı trait abstraction yapısı uygulanabilir.
6. **Devre büyüklüğü kontrolü:** `circuit_degree` yetersiz degree için erken hata — nova shield devreleri bu kontrolü ceremony başında çalıştırmalı.

#### Nova Shield Chain DEX İçin ZK Bağlantısı

- Swap ve SwapClaim devreleri Penumbra'nın batch swap mekanizmasının ZK ispatlarıdır.
- Nova Shield Chain DEX gizli bir swap sistemi istiyorsa bu devrelerin kısıtlamalarını incelemek (DEX agent ayrıca analiz edilecek) anahtar referans noktası.
- `SWAP_PROOF_PROVING_KEY` ve `SWAPCLAIM_PROOF_PROVING_KEY` — DEX gizliliğinin kriptografik temeli.

---

## DOGRULANMIS KRITIK DEGERLER (Kaynak Koddan)

| Değer | Konum | Değer |
|---|---|---|
| `GROTH16_PROOF_LENGTH_BYTES` | proof-params/src/lib.rs:14 | `192` |
| `CONTRIBUTION_HASH_SIZE` | proof-setup/src/single/log.rs:2 | `32` |
| `NUM_CIRCUITS` | proof-setup/src/all.rs:58 | `7` |
| `SERIALIZATION_COMPRESSION` | proof-setup/src/all.rs:34 | `Compress::No` |
| FROST ciphersuite ID | decaf377-frost/src/traits.rs:88 | `"decaf377-rdsa"` |
| H2 personalization | decaf377-frost/src/hash.rs:23 | `b"decaf377-rdsa---"` (16 byte) |
| Default personalization | decaf377-frost/src/hash.rs:12 | `b"FROST-decaf377"` (14 byte) |
| BLAKE2b hash length | decaf377-frost/src/hash.rs:11 | `64` byte |
| decaf377 cofactor | decaf377-frost/src/traits.rs:58 | `Fr::one()` (cofactor=1) |
| Phase1 short_len | proof-setup/src/single/phase1.rs:24-25 | `(d-1)+1 = d` |
| Phase1 long_len | proof-setup/src/single/phase1.rs:32-34 | `(2*d-2)+1 = 2d-1` |
| DLog NIZK personalization | proof-setup/src/single/dlog.rs:43 | `b"PC$:proof_chal"` (14 byte) |
| Ceremony hash G1 personal | proof-setup/src/single/group.rs:138 | Max 16 byte kısıtı |
| Batched check scalar size | proof-setup/src/single/group.rs:39-41 | 16 byte (128-bit) |
| LFS pointer threshold | proof-params/build.rs:107 | `< 500` bytes |
| SPEND pk ID | proof-params/src/gen/spend_id.rs:2 | `groth16pk1ke43yax...` |
| SPEND vk ID | proof-params/src/gen/spend_id.rs:3 | `groth16vk1zu2ks8...` |
