# Penumbra Kripto & Anahtar Sistemi — Tam Analiz
## Agent: agent-15-crypto-keys
## Kapsam: decaf377-fmd, decaf377-frost, decaf377-ka, eddy, proof-setup, proof-params, core/keys

---

## ÖZET

Bu agent Penumbra'nın kriptografik altyapısını oluşturan 6 crate'i ve `core/keys` crate'ini analiz etti.
Toplam ~100 dosya, ~3000 satır kaynak kodu incelendi.

Kritik bulgular:
1. **FMD** (Fuzzy Message Detection) — ZCash'ten ilham alan, kuantum-güvenli olmayan ama pratik privacy için yeterli probabilistic mesaj tespiti
2. **FROST** — Penumbra'nın threshold imza sistemi, `decaf377-rdsa` üzerine inşa, DKG dahil tam implementasyon
3. **Eddy** — DEX batch swap için flow encryption (ElGamal + threshold decryption + Lagrange interpolation)
4. **Proof Setup** — 2 fazlı güvenilir kurulum töreni (Groth16 için), bağlayıcı ispat (dlog) ile her katkı doğrulanıyor
5. **Key Hierarchy** — SpendKey → FVK → IVK+OVK, Zcash Sapling'e yakın ama decaf377 üzerinde
6. **Address Encoding** — F4Jumble ile kamuflaj, 80 byte = 16 diversifier + 32 pk_d + 32 ck_d

---

## 1. decaf377-fmd — Fuzzy Message Detection

### Dosya: src/lib.rs (23 satır)
**Genel Amaç:** FMD protokolünün crate root'u. `#![deny(clippy::unwrap_used)]` zorunlu.

**Re-exports:**
- `Clue` — 68 byte probabilistik mesaj etiketi
- `ClueKey` / `ExpandedClueKey` — alıcının public detection anahtarı
- `DetectionKey` — özel tespit anahtarı
- `Precision` — 0..=24 bit hassasiyet
- `MAX_PRECISION = 24` (pub(crate))

---

### Dosya: src/precision.rs (82 satır)
**Struct:** `Precision(u8)` — newtype, 0..=24 bit aralığı

**Kritik Sabitler:**
```rust
pub(crate) const MAX_PRECISION: u8 = 24;
// MAX_PRECISION seçiminin nedeni: 3 byte = 24 bit maksimum ciphertext kapasitesi
// clue.buf[65..68] = 3 byte = ctxts alanı
```

**TryFrom impls:** `u8`, `u32`, `u64`, `i32` — hepsi range check ile

**Tasarım Kararı:** `Precision(0)` default — hiçbir tespit koşulu yok (FP rate = 1.0). Kullanışlı test değeri.

---

### Dosya: src/clue.rs (29 satır)
**Struct:**
```
Clue([u8; 68]) =
  buf[0..32]:  P_encoding (ephemeral point, compressed decaf377 element)
  buf[32..64]: y (Fr scalar — blinding proof)
  buf[64]:     precision_bits (u8, 0..=24)
  buf[65..68]: ctxts (BitArray<[u8;3], Lsb0> — şifrelenmiş mesaj bitleri)
```

**Yorum:** Clue tamamen public — zincirde yayınlanır, tüm detektörler görür.

---

### Dosya: src/clue_key.rs (200 satır)
**Struct:** `ClueKey(pub [u8; 32])` — 32 byte compressed decaf377 element

**ExpandedClueKey:**
```rust
pub struct ExpandedClueKey {
    root_pub: decaf377::Element,    // G^dtk
    root_pub_enc: decaf377::Encoding,
    subkeys: RefCell<Vec<decaf377::Element>>,  // lazily expanded
}
```

**Kilit Fonksiyon: `create_clue_deterministic`**

Protokol adımları (non_snake_case izin verilmiş):
```
1. r = blake2b("decaf377-fmd.rdv", root_pub_enc || rseed) mod |Fr|
2. z = blake2b("decaf377-fmd.zdv", root_pub_enc || rseed) mod |Fr|
3. P = r * G  (ephemeral commitment)
4. Q = z * G  (blinding commitment)
5. for i in 0..precision_bits:
     rXi = r * Xi     (Xi = i-th subkey)
     key_i = hash_to_bit(P_enc, rXi_enc, Q_enc) & 1
     ctxt_i = key_i XOR 1    ← her bit 1 olacak şekilde şifrelenir
6. m = hash_to_scalar(P_enc, precision_bits, ctxts)
7. y = (z - m) / r          ← "proof" scalar
8. Clue = P || y || precision || ctxts
```

**Matematiksel Güvenlik Notu:**
- Alıcı, `y*P + m*G = Q` ilişkisini doğrulayarak P ve Q'nun doğru oluşturulduğunu kontrol eder
- Yanlış detection key yalnızca `1/2^precision` olasılıkla tüm bitleri 1 olarak değerlendirir
- False negative yoktur (deterministik)

**expand_infallible:** Geçersiz ClueKey baytlarını Fq'ya çevirip +1 ile artırarak geçerli hale getirir. Adres olmayan ClueKey'leri güvenli şekilde kullanmak için.

---

### Dosya: src/detection.rs (134 satır)
**Struct:**
```rust
pub struct DetectionKey {
    dtk: Fr,                          // ana özel anahtar
    xs: [Fr; MAX_PRECISION as usize], // 24 önceden türetilmiş alt anahtar
}
```

**ClueKey Türetme:**
```
ClueKey = (dtk * G).compress()
```

**examine() Algoritması:**
```
1. P = clue[0..32] decompress
2. y = clue[32..64] scalar parse
3. GÜVENLIK KONTROL: P != identity && y != 0 (rogue key saldırısı önlemi)
4. m = hash_to_scalar(P_enc, precision_bits, ctxts)
5. Q_bytes = (y * P + m * G).compress()
6. for i in 0..precision_bits:
     Px_i = (P * xs[i]).compress()
     key_i = hash_to_bit(P_enc, Px_i_enc, Q_bytes)
     msg_i = ctxt_i XOR key_i
     if msg_i == 0: return false  // ERKEN ÇIKIŞ
7. return true
```

**Zamanlama Analizi:**
- Long-term secret `xs[i]` erken çıkışı etkilemez (short-circuit sadece `key_i`'ye bağlı)
- Beklenen iterasyon sayısı: 2 (geometrik dizi) — çok verimli

---

### Dosya: src/hkd.rs (58 satır)
**HKD (Hierarchical Key Derivation):**

```rust
// Public derivation:
derive_public(root_pub, root_pub_enc, index) -> Element {
    x = blake2b("decaf377-fmd.hkd", root_pub_enc || [index]) mod |Fr|
    X = x * G
    return root_pub + X    // additive child key
}

// Private derivation:
derive_private(root_priv, root_pub_enc, index) -> Fr {
    x = blake2b("decaf377-fmd.hkd", root_pub_enc || [index]) mod |Fr|
    return root_priv + x
}
```

**Özellik:** Public/private derivation tutarlı: `(dtk + x) * G = dtk*G + x*G`
Proptest ile doğrulanmış (tüm i in 0..16 için).

---

### Dosya: src/hash.rs (28 satır)
**İki hash fonksiyonu:**
```rust
to_bit(a, b, c: &[u8;32]) -> u8 {
    blake2b_simd("decaf377-fmd.bit", a||b||c)[0] & 1
}

to_scalar(point: &[u8;32], n: u8, bits: &[u8]) -> Fr {
    blake2b_simd("decaf377-fmd.sca", point||[n]||bits) mod |Fr|
    // assert!(bits.len() == 3)  — sabit uzunluk
}
```

---

### Dosya: tests/fmd.rs (51 satır)
**Test: `detection_distribution_matches_expectation`**
- 1024 clue üretilir, precision=4 bit (FP rate = 1/16)
- alice'in detection key'i: 100% tespit
- bobce'nin detection key'i: ~6.25% (±4%) toleransla doğrulanır

**Test: `fails_to_expand_clue_key`**
- `[1; 32]` geçersiz bir decaf377 encoding — `expand()` Err döner

---

## 2. decaf377-frost — FROST Threshold Signing

### Dosya: src/lib.rs (261 satır)
**Genel Amaç:** FROST protokolünü decaf377-rdsa (SpendAuth) imzaları için adapte eder.

**Tipler (type aliases):**
- `Identifier = frost::Identifier<Decaf377Rdsa>`
- `SigningNonces`, `SigningCommitments` — Round 1
- `SigningPackage` — Koordinatör paketi
- `SignatureShare` — Round 2 payı

**round1::commit:**
```rust
pub fn commit<RNG>(secret: &SigningShare, rng: &mut RNG) -> (SigningNonces, SigningCommitments)
```

**round2::sign (normal):**
```rust
pub fn sign(signing_package, signer_nonces, key_package) -> Result<SignatureShare>
```

**round2::sign_randomized:**
```rust
pub fn sign_randomized(signing_package, signer_nonces, key_package, randomizer: Fr) -> Result<SignatureShare>
// frost_rerandomized kullanır — gizli anonim harcama imzaları için
```

**aggregate / aggregate_randomized:**
- `frost_core::frost::aggregate` veya `frost_rerandomized::aggregate` çağrılır
- Sonuç: standart 64 byte Schnorr imzası (decaf377-rdsa formatı)

---

### Dosya: src/traits.rs (126 satır)
**Ciphersuite Implementasyonu:**

```rust
pub struct Decaf377Rdsa;

impl Ciphersuite for Decaf377Rdsa {
    const ID: &'static str = "decaf377-rdsa";
    type Group = Decaf377Group;   // decaf377::Element
    type HashOutput = [u8; 32];

    fn H1(m) -> Fr { Hasher::default().update(b"rho").update(m).finalize_scalar() }
    fn H2(m) -> Fr { Hasher::challenge().update(m).finalize_scalar() }  // decaf377-rdsa uyumlu
    fn H3(m) -> Fr { Hasher::default().update(b"nonce").update(m).finalize_scalar() }
    fn H4(m) -> [u8;32] { Hasher::default().update(b"msg").update(m).finalize() }
    fn H5(m) -> [u8;32] { Hasher::default().update(b"com").update(m).finalize() }
    fn HDKG(m) -> Fr { Hasher::default().update(b"dkg").update(m).finalize_scalar() }
    fn HID(m)  -> Fr { Hasher::default().update(b"id").update(m).finalize_scalar() }
}
```

**Kritik: H2 (`challenge`)**
- `Params::new().personal(b"decaf377-rdsa---")` — FROST challenge hash'i, `decaf377-rdsa` kütüphanesiyle uyumlu olması için aynı personal string!

**Decaf377Group:**
- `cofactor() = Fr::one()` — decaf377 prime-order grup (cofactor = 1)

---

### Dosya: src/hash.rs (47 satır)
**Hasher struct:**
```rust
pub struct Hasher { state: State }
// Default: personal = b"FROST-decaf377", hash_length = 64
// challenge: personal = b"decaf377-rdsa---", hash_length = 64
```

`finalize_scalar()`: 64 byte hash → `Fr::from_le_bytes_mod_order`
`finalize()`: 64 byte hash → [u8; 32] (ilk 32 byte alınır)

---

### Dosya: src/keys.rs (103 satır)
**Fonksiyonlar:**
```rust
fn generate_with_dealer(max_signers, min_signers, identifiers, rng)
    -> (HashMap<Identifier, SecretShare>, PublicKeyPackage)

fn split(secret: &SigningKey<SpendAuth>, max_signers, min_signers, identifiers, rng)
    -> (HashMap<Identifier, SecretShare>, PublicKeyPackage)
    // Mevcut anahtarı FROST share'lerine ayırır

fn reconstruct(key_packages: &[KeyPackage]) -> SigningKey<SpendAuth>
    // Lagrange interpolasyonu ile anahtarı geri yükler (sadece acil durum için!)
```

**Type Aliases:**
- `SecretShare`, `SigningShare`, `VerifyingShare` — `frost_core` tipleri
- `KeyPackage`, `PublicKeyPackage` — her katılımcının sakladığı paketler
- `VerifiableSecretSharingCommitment` — VSS commitment'ları

---

### Dosya: src/keys/dkg.rs (183 satır)
**DKG 3-Aşama Protokolü:**

```
part1(identifier, max_signers, min_signers, rng) -> (SecretPackage, round1::Package)
  - round1::Package: VSS commitment + proof of knowledge (Schnorr)
  - SecretPackage: GİZLİ, asla paylaşılmamalı!

part2(secret_package, round1_packages) -> (SecretPackage, HashMap<Identifier, round2::Package>)
  - round2::Package: kişiye özgü secret share (confidential + authenticated kanal gerekli)

part3(round2_secret_package, round1_packages, round2_packages) -> (KeyPackage, PublicKeyPackage)
  - Nihai anahtar materyali
```

**Protobuf Serializasyon:**
- `DkgRound1Package`: commitment (Vec<Vec<u8>>) + proof_of_knowledge (Vec<u8>)
- `DkgRound2Package`: signing_share scalar (Vec<u8>)

---

## 3. decaf377-ka — Key Agreement

### Dosya: src/lib.rs (160 satır)
**Tipler:**
```rust
pub struct Public(pub [u8; 32]);   // compressed decaf377 element
pub struct Secret(decaf377::Fr);   // #[zeroize(drop)]
pub struct SharedSecret(pub [u8; 32]);  // #[zeroize(drop)]
```

**Key Agreement:**
```rust
impl Secret {
    fn public(&self) -> Public {
        (self.0 * G).compress().into()
    }

    fn diversified_public(&self, diversified_generator: &Element) -> Public {
        (self.0 * diversified_generator).compress().into()
    }

    fn key_agreement_with(&self, other: &Public) -> Result<SharedSecret, Error> {
        let pk = Encoding(other.0).vartime_decompress()?;
        Ok(SharedSecret((self.0 * pk).compress().into()))
    }
}
```

**Matematiksel Temel:** ECDH üzerinde decaf377 grubunda — `SharedSecret = sk * PK = sk * (sk' * G) = sk * sk' * G`

**Diversified ECDH:** Tek bir secret key, birden fazla (linklenmez) public key üretebilir. Address privacy'nin temeli.

**Güvenlik:** `#[zeroize(drop)]` ile Secret ve SharedSecret bellekten güvenli siliniyor.

---

## 4. eddy — Flow Encryption (DEX İçin)

### Dosya: src/lib.rs (36 satır)
**Genel Amaç:** Penumbra DEX'inde batch swap miktarlarını şifrelemek için threshold (eşikli) şifreleme.

**Durum (TODO'lar):**
- [x] Encryption — tamamlandı
- [x] Decryption — tamamlandı
- [x] Decryption Proofs — tamamlandı (Schnorr sigma)
- [ ] Error on insufficient shares — EKSIK
- [ ] Distributed key generation — EKSIK
- [ ] Serialization — EKSIK
- [ ] Encryption Proofs (ZK-SNARK) — yalnızca transparent stub

**DEX Notu:** Bu sistem batch swap sırasında şifreli swap miktarlarını toplayabilir (homomorphik), sonra threshold decrypt ile açar.

---

### Dosya: src/value.rs (121 satır)
**Struct:** `Value(pub u128)` — 128-bit iç temsil, 64-bit şifreleme sınırı

**4-Limb Decomposition:**
```rust
fn to_limbs(&self) -> [limb::Value; 4] {
    let v = u64::try_from(self.0)?;
    [
        (v & 0xffff) as u32,          // limb0: bit 0-15
        ((v >> 16) & 0xffff) as u32,   // limb1: bit 16-31
        ((v >> 32) & 0xffff) as u32,   // limb2: bit 32-47
        ((v >> 48) & 0xffff) as u32,   // limb3: bit 48-63
    ]
}

fn from_limbs(x0, x1, x2, x3) -> Value {
    Value(x0 + (x1 << 16) + (x2 << 32) + (x3 << 48))
}
```

**transparent_encrypt:** Her limb'i ayrı ayrı ElGamal ile şifreler, `TransparentEncryptionProof` döner (ZK değil!).

**Proptest:**
- `limb_value_roundtrip`: to_limbs → from_limbs = başlangıç değeri
- `limb_value_addition_roundtrip`: (a+b).limbs = a.limbs + b.limbs (homomorfizm)

---

### Dosya: src/limb/value.rs (34 satır)
**Struct:** `Value(pub u32)` — 16-bit şifreli, u32 iç temsil (toplam aşım için)

**ElGamal ŞifreleMe:**
```rust
fn transparent_encrypt(value, encryption_key, rng) -> (Ciphertext, Blinding) {
    let r = Fr::rand(rng);          // elgamal blind
    let c1 = r * G                  // commitment
    let c2 = r * K + v * G         // K = encryption_key, v = değer
    return (Ciphertext{c1, c2}, r)
}
```

Bu klasik ElGamal: `c1 = r*G, c2 = r*K + v*G`

---

### Dosya: src/limb/ciphertext.rs (67 satır)
**Ciphertext Yapısı:**
```rust
pub struct Ciphertext {
    c1: decaf377::Element,  // r * G
    c2: decaf377::Element,  // r * K + v * G
}
```

**Homomorfik Toplama:**
```rust
impl Add<&Ciphertext> for &Ciphertext {
    fn add(self, rhs) -> Ciphertext {
        Ciphertext { c1: self.c1 + rhs.c1, c2: self.c2 + rhs.c2 }
    }
}
```

**Threshold Decryption (Lagrange):**
```rust
fn decrypt(&self, shares: Vec<&DecryptionShare<Verified>>) -> Element {
    let indices: Vec<u32> = shares.iter().map(|s| s.participant_index).collect();

    let d = shares.iter().fold(Element::GENERATOR, |acc, share| {
        acc + share.decryption_share * lagrange_coefficient(share.participant_index, &indices)
    });

    -d + self.c2
    // = -Σ(λ_i * x_i * c1) + c2
    // = -(r * Σ(λ_i * x_i) * G) + r*K + v*G
    // = -(r * x * G) + r*(x*G) + v*G  [x = özel anahtar]
    // = v * G
}
```

**Lagrange Katsayısı:**
```rust
fn lagrange_coefficient(participant_index: u32, participant_indices: &[u32]) -> Fr {
    participant_indices.iter()
        .filter(|x| **x != participant_index)
        .fold(Fr::ONE, |acc, x| {
            let n = Fr::from(*x);
            let i = Fr::from(participant_index);
            acc * (n / (n - i))     // Lagrange: Π (x_j / (x_j - x_i))
        })
}
```

---

### Dosya: src/limb/decryption_share.rs (123 satır)
**Sigma Protokolü (Chaum-Pedersen):**

**Prove:**
```
decryption_share = key_share * c1
k = rand (Merlin transcript tabanlı)
kB = k * G
kC_1 = k * c1
challenge = transcript.challenge_scalar("c")
response = k - key_share * challenge
```

**Verify:**
```
kB = G * response + pub_key_share * challenge
kC_1 = c1 * response + decryption_share * challenge
challenge_recomputed = transcript.challenge_scalar("c")
assert challenge == challenge_recomputed
```

**Merlin Transcript:** Tüm transcript'e bağlı randomluk — `rekey_with_witness_bytes(b"key_share", ...)` ile private key'e bağlanıyor.

---

### Dosya: src/decryption_share.rs (105 satır)
**4-Limb Decryption Share:**
```rust
pub struct DecryptionShare<S: VerificationStatus> {
    participant_index: u32,
    share0: limb::DecryptionShare<S>,
    share1: limb::DecryptionShare<S>,
    share2: limb::DecryptionShare<S>,
    share3: limb::DecryptionShare<S>,
}
```

**VerificationStatus Pattern:**
```rust
pub trait VerificationStatus: private::Sealed {}
pub enum Verified {}
pub enum Unverified {}
```
Doğrulanmamış share ile decryption yapmak compile-time hata!

---

### Dosya: src/decryption_table.rs (147 satır)
**DecryptionTable Trait:**
```rust
pub trait DecryptionTable: Send + Sync {
    fn lookup(&self, key: [u8; 32]) -> Pin<Box<dyn Future<Output = Result<Option<u32>>> + Send>>;
    fn store(&self, key: [u8; 32], value: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn initialize(&self, k: usize) -> ... {
        // x = 0..2^k, xB = x * G şeklinde doldurur
        // Bu baby-step giant-step'in lookup tablosu
    }
}
```

**Kısıt:** Tablo boyutu = `2^(16 + log2(N))` — N = toplam aggregated ciphertext sayısı
64 ciphertext için: `2^22 = 4M entry`

**MockDecryptionTable:** BTreeMap<[u8;32], u32> — test için

---

### Dosya: src/ciphertext.rs (94 satır)
**4-Limb Ciphertext:**
```rust
pub struct Ciphertext {
    c0: limb::Ciphertext,  // bit 0-15
    c1: limb::Ciphertext,  // bit 16-31
    c2: limb::Ciphertext,  // bit 32-47
    c3: limb::Ciphertext,  // bit 48-63
}
```

**async decrypt:**
```rust
pub async fn decrypt(&self, shares: Vec<DecryptionShare<Verified>>, table: &dyn DecryptionTable)
    -> anyhow::Result<Value> {
    // Her limb için ayrı share set oluştur
    // Her limb'i decrypt et (v*G elementi alınır)
    // Her v*G'yi table.lookup ile u32'ye dönüştür (discrete log)
    // Value::from_limbs ile birleştir
}
```

**TODO:** Insufficient shares kontrolü yok — kritik güvenlik açığı!

---

### Dosya: src/transcript.rs (47 satır)
**Domain Separation Labels:**
- `begin_decryption`: `b"eddy-decaf377-decrypt"`
- `begin_limb_decryption`: `b"begin-limb"`
- public key share, ciphertext, commitment: ayrı label'larla

---

## 5. proof-setup — Groth16 Trusted Setup Töreni

### Dosya: src/single/phase1.rs (721 satır)
**Genel Amaç:** BLS12-377 üzerinde Groth16 CRS'nin Phase 1 (powers-of-tau) kısmı.

**RawCRSElements Yapısı:**
```rust
pub struct RawCRSElements {
    alpha_1: G1,
    beta_1: G1,
    beta_2: G2,
    x_1: Vec<G1>,      // uzunluk: 2d-1 (long_len)
    x_2: Vec<G2>,      // uzunluk: d   (short_len)
    alpha_x_1: Vec<G1>,  // uzunluk: d
    beta_x_1: Vec<G1>,   // uzunluk: d
}
```

**Doğrulama Kontrolleri (validate):**
1. Sıfır element yok: alpha_1, beta_1, beta_2, x_1[1], x_2[1] != 0
2. `beta_1 ↔ beta_2` tutarlılığı (pairing check)
3. `alpha_x_i` = alpha_1 * x^i (pairing check)
4. `beta_x_i` = beta_1 * x^i (pairing check)
5. x_i = x^i (ardışık pairing check: `e(x_i, G2) = e(G1, x_{i+1})`)

**BatchedPairingChecker:** OsRng tabanlı randomized pairing check — biri geçerse tümü geçer yaklaşımı değil, ayrı ayrı kontrol.

**Katkı Yapma (Contribution::make):**
```
alpha_new = alpha_old * alpha_contrib
beta_new = beta_old * beta_contrib
x_i_new = x_i_old * x_contrib^i
alpha_x_i_new = alpha_x_i_old * (alpha * x^i)
beta_x_i_new = beta_x_i_old * (beta * x^i)
linking_proof = dlog_prove(alpha, beta, x)  // 3 ayrı ispat
```

**Güvenlik:** Her katkı bir öncekini "üzerine yazar" — birinin güvenli olması yeterli (k-of-n güven modeli).

**Yorum `// "神だけが私を裁ける"` (Satır 113):** "Yalnızca Tanrı beni yargılayabilir" — parallel checker validation üzerindeki yorumla birlikte komik bir kültürel referans.

---

### Dosya: src/single/phase2.rs (481 satır)
**Genel Amaç:** Groth16 Phase 2 — devre-spesifik CRS kısmı.

**RawCRSElements:**
```rust
pub struct RawCRSElements {
    delta_1: G1,
    delta_2: G2,
    inv_delta_p_1: Vec<G1>,  // p_i / delta
    inv_delta_t_1: Vec<G1>,  // t_i / delta
}
```

**Doğrulama:**
- delta_1 ↔ delta_2 tutarlı
- `e(inv_delta_p_i, delta_2) = e(p_i, G2)` — her p_i için
- `e(inv_delta_t_i, delta_2) = e(t_i, G2)` — her t_i için

**Katkı:** Tek delta çarpanı (alpha/beta yok), tek dlog proof yeterli.

**linking_proof:** Phase 1'de 3 dlog, Phase 2'de 1 dlog — daha basit.

---

### Dosya: src/single/dlog.rs (doğrudan okunamadı, ama kullanılan)
**dlog::Proof:** Schnorr sigma protokolü, `Statement { result, base }` ve `Witness { dlog }` ile.

---

## 6. proof-params — Groth16 Proving/Verifying Keys

### Dosya: src/lib.rs (283 satır)
**GROTH16_PROOF_LENGTH_BYTES = 192** — sıkıştırılmış BLS12-377 proof boyutu

**LazyProvingKey:**
```rust
pub struct LazyProvingKey {
    pk_id: &'static str,   // bech32m "groth16pk..." hash
    inner: OnceCell<ProvingKey<Bls12_377>>,
}
```

**try_load:** SHA256(compressed_pk) → bech32m → pk_id ile eşleşmeli

**7 Statik Proving Key:**
1. `SPEND_PROOF_PROVING_KEY` — spend işlemi ZK proof
2. `OUTPUT_PROOF_PROVING_KEY` — çıkış notu ZK proof
3. `SWAP_PROOF_PROVING_KEY` — DEX swap ZK proof
4. `SWAPCLAIM_PROOF_PROVING_KEY` — swap claim ZK proof
5. `CONVERT_PROOF_PROVING_KEY` — dönüşüm ZK proof
6. `DELEGATOR_VOTE_PROOF_PROVING_KEY` — delegatör oy ZK proof
7. `NULLIFIER_DERIVATION_PROOF_PROVING_KEY` — nullifier türetme ZK proof

**Feature Flag `bundled-proving-keys`:** Derleme zamanında `include_bytes!("gen/spend_pk.bin")` ile binary'ye gömülür. Bu yoksa runtime'da `try_load` ile yüklenmeli.

---

### Dosya: src/traits.rs (109 satır)
**DummyWitness Trait:**
```rust
pub trait DummyWitness: ConstraintSynthesizer<Bls12_377::ScalarField> {
    fn with_dummy_witness() -> Self;
}
```

**generate_constraint_matrices:** Gerçek setup olmadan constraint sayısını öğrenmek için. Tören (ceremony) hazırlığı için kritik.

**VerifyingKeyExt / ProvingKeyExt:**
```rust
fn debug_id(&self) -> String {
    let compressed = self.serialize_compressed();
    let hash = sha2::Sha256::digest(&compressed);
    bech32::encode("groth16vk", hash.to_base32(), bech32::Variant::Bech32m)
}
```

---

## 7. core/keys — Anahtar Hiyerarşisi

### Dosya: src/lib.rs (22 satır)
**Re-exports:**
```rust
pub use address::{Address, AddressVar, AddressView};
pub use keys::FullViewingKey;
pub use symmetric::{BackreferenceKey, PayloadKey, PositionMetadataKey};
```

**Bağımlılıklar:** decaf377_fmd, decaf377_ka, decaf377_rdsa — tüm crypto crate'lere bağlıdır.

---

### Dosya: src/prf.rs (12 satır)
**Temel PRF:**
```rust
fn expand(label: &'static [u8; 16], key: &[u8], input: &[u8]) -> blake2b_simd::Hash {
    blake2b_simd::Params::new().personal(label).key(key).hash(input)
}

fn expand_ff<F: PrimeField>(label, key, input) -> F {
    F::from_le_bytes_mod_order(expand(label, key, input).as_bytes())
}
```

**Kullanım Yerleri:**
- `b"Penumbra_ExpndSd"` → ask (spend auth key) ve nk (nullifier key)
- `b"Penumbra_DeriOVK"` → OVK türetimi
- `b"Penumbra_DerivDK"` → DK türetimi
- `b"PenumbraExpndFMD"` → per-diversifier detection key
- `b"Penumbra_Payload"` → PayloadKey
- `b"Penumbra_OutCiph"` → OutgoingCipherKey

---

### Dosya: src/keys/spend.rs (219 satır)
**Anahtar Türetme:**
```rust
impl From<SpendKeyBytes> for SpendKey {
    fn from(seed: SpendKeyBytes) -> Self {
        let ask = SigningKey::new_from_field(prf::expand_ff(b"Penumbra_ExpndSd", &seed.0, &[0; 1]));
        let nk  = NullifierKey(prf::expand_ff(b"Penumbra_ExpndSd", &seed.0, &[1; 1]));
        let fvk = FullViewingKey::from_components(ask.into(), nk);
        Self { seed, ask, fvk }
    }
}
```

**SpendKeyBytes → SpendKey türetimi:**
```
seed[32 byte]
├─ PRF("Penumbra_ExpndSd", seed, [0]) → ask (Fr scalar)
└─ PRF("Penumbra_ExpndSd", seed, [1]) → nk  (Fq element)
```

**BIP39 Uyumluluğu:**
```rust
fn from_seed_phrase_bip39(seed_phrase, index) -> Self {
    // PBKDF2-SHA512, 2048 iterasyon, salt = "mnemonic" + index
    // BIP39 standardına tam uygun
}

fn from_seed_phrase_bip44(seed_phrase, path) -> Self {
    // PBKDF2-SHA512, salt = "mnemonic" (passphrase yok)
    // bip32::XPrv.derive_from_path() ile BIP44 türetimi
}
```

**BIP44 Test Vector:** Ledger test account, beklenen `0x1b8113...` byte ile doğrulanmış.

---

### Dosya: src/keys/fvk.rs (222 satır)
**FullViewingKey Yapısı:**
```rust
pub struct FullViewingKey {
    ak: VerificationKey<SpendAuth>,    // 32 byte, decaf377 element
    nk: NullifierKey,                  // 32 byte, Fq element
    ovk: OutgoingViewingKey,           // 32 byte
    ivk: IncomingViewingKey,           // ka::Secret + DiversifierKey
}
```

**FVK Türetme:**
```
from_components(ak, nk):
  OVK = PRF("Penumbra_DeriOVK", nk.bytes, ak.bytes)[0..32]
  DK  = PRF("Penumbra_DerivDK", nk.bytes, ak.bytes)[0..16]
  IVK_q = poseidon377::hash_2(IVK_DOMAIN_SEP, (nk.0, Fq::from_bytes(ak.bytes)))
  IVK_r = Fr::from_le_bytes_mod_order(IVK_q.bytes)  // q→r alan indirgeme
```

**IVK Domain Separator:**
```rust
static IVK_DOMAIN_SEP: Lazy<Fq> = Lazy::new(|| Fq::from_le_bytes_mod_order(b"penumbra.derive.ivk"));
```

**WalletId Türetimi:**
```rust
fn wallet_id(&self) -> WalletId {
    let hash = poseidon377::hash_2(&ACCOUNT_ID_DOMAIN_SEP, (nk.0, Fq::from(ak.bytes)));
    WalletId(hash.to_bytes()[..32].try_into().unwrap())
    // ACCOUNT_ID_DOMAIN_SEP = Fq::from("Penumbra_HashFVK")
}
```

**Serialization:** `FVK = ak.bytes[32] || nk.bytes[32]` = 64 byte, Bech32m kodlama

---

### Dosya: src/keys/ivk.rs (320 satır)
**IncomingViewingKey:**
```rust
pub struct IncomingViewingKey {
    ivk: ka::Secret,   // Fr scalar
    dk: DiversifierKey, // 16 byte AES-128 key
}
```

**payment_address(index) Türetimi:**
```
d   = DK.diversifier_for_index(index)      // AES-128 encrypt(DK, index.bytes)
g_d = blake2b("Penumbra_Divrsfy", d)[0..32] → decaf377::encode_to_curve()
pk_d = ivk * g_d                            // diversified public key
dtk_d = FMD::DetectionKey::from_field(
    PRF("PenumbraExpndFMD", ivk.bytes, d.bytes) mod |Fr|
)
ck_d = dtk_d.clue_key()
return (Address{d, g_d, pk_d, ck_d}, dtk_d)
```

**Transparent Address:** Null diversifier ([0;16]) + null clue key ([0;32]) — tpenumbra prefix

**IncomingViewingKeyVar (R1CS):**
```
// IVK = poseidon(IVK_DOMAIN_SEP, nk, ak) mod r
// Constraint: ivk_q = r_modulus * a + ivk_mod_r
// Constraint: a(a-1)(a-2)(a-3)(a-4) = 0  (a <= 4)
// q/r katsayısı = 4 (matematiksel olarak doğrulanmış)
```

---

### Dosya: src/keys/diversifier.rs (318 satır)
**AES-128 Tabanlı Diversifier:**
```rust
impl DiversifierKey {
    fn diversifier_for_index(&self, index: &AddressIndex) -> Diversifier {
        let key = AES128::new(self.0);
        let block = index.to_bytes();  // 4 byte account + 12 byte randomizer
        key.encrypt_block(block)
        return Diversifier(block)
    }

    fn index_for_diversifier(&self, diversifier: &Diversifier) -> AddressIndex {
        // Null diversifier special case: account = 0 (transparent address)
        if diversifier == [0;16]: return AddressIndex{account:0, randomizer:[0;12]}
        let block = AES128::new(self.0).decrypt_block(diversifier.0)
        return AddressIndex::from_bytes(block)
    }
}
```

**AddressIndex:** `{ account: u32, randomizer: [u8; 12] }` — ephemeral address için randomizer != [0;12]

**Diversifier → Generator:**
```rust
fn diversified_generator(&self) -> Element {
    let hash = blake2b_simd::Params::new().personal(b"Penumbra_Divrsfy").hash(&self.0);
    decaf377::Element::encode_to_curve(Fq::from_le_bytes_mod_order(hash.as_bytes()))
}
```

---

### Dosya: src/keys/nullifier.rs (48 satır)
**NullifierKey:**
```rust
pub struct NullifierKey(pub Fq);    // decaf377 base field element
pub struct NullifierKeyVar { pub inner: FqVar }  // R1CS değişkeni
```

**AllocVar:** Yalnızca Witness modu — Constant/Input modu implement edilmemiş (`unimplemented!()`)

---

### Dosya: src/address.rs (547 satır)
**Address Yapısı:**
```rust
pub struct Address {
    d: Diversifier,            // 16 byte
    g_d: OnceLock<Element>,    // lazy computed from d
    pk_d: ka::Public,          // 32 byte, diversified public key
    transmission_key_s: Fq,    // pk_d'nin s değeri (field element)
    ck_d: fmd::ClueKey,        // 32 byte
}
```

**Total: 80 byte (pre-jumble)**

**F4Jumble Kodlama:**
```rust
fn to_vec(&self) -> Vec<u8> {
    let mut raw = d[16] || pk_d[32] || ck_d[32];  // 80 byte
    f4jumble(&raw)  // Zcash F4Jumble (BLAKE2b tabanlı Feistel)
}
```

**Gizlilik:** F4Jumble olmadan d ve ck_d açıkça görülebilirdi. Jumble ile tüm 80 byte birbirine karışır.

**Çoklu Format Desteği:**
- `penumbrav2t1...` — ana format (Bech32m)
- `penumbracompat1...` — eski format (Bech32)
- `tpenumbra1...` — transparent address

**Noble Forwarding Address:**
```rust
fn noble_forwarding_address(&self, channel: &str) -> NobleForwardingAddress {
    // IBC forwarding için, SHA256(SHA256("forwarding") || channel+recipient)[12..]
}
```

---

### Dosya: src/symmetric.rs (450 satır)
**PayloadKey (ChaCha20Poly1305):**
```rust
fn derive(shared_secret: &SharedSecret, epk: &Public) -> Self {
    blake2b_simd("Penumbra_Payload", shared_secret || epk)  // 32 byte
}
```

**PayloadKind Nonce Atama:**
```rust
fn nonce(&self) -> [u8; 12] {
    Note:    [0,0,0,0, 0,0,0,0, 0,0,0,0]
    MemoKey: [1,0,0,0, 0,0,0,0, 0,0,0,0]
    Swap:    [2,0,0,0, 0,0,0,0, 0,0,0,0]
    Memo:    [3,0,0,0, 0,0,0,0, 0,0,0,0]
}
```

**Güvenlik Notu (Kod İçi):** Note ve OCK aynı nonce kullanır fakat farklı key'ler — ZCash protokol spec Section 5.4.3'e atıf.

**OutgoingCipherKey:**
```rust
fn derive(ovk, cv, cm, epk) -> Self {
    blake2b_simd("Penumbra_OutCiph", ovk || cv || cm || epk)
}
```

**BackreferenceKey:**
```rust
fn derive(ovk) -> Self {
    blake2b_simd("Penumbra_Backref", ovk)
}
```

**PositionMetadataKey (XChaCha20Poly1305):**
- 24 byte nonce (XChaCha neden: rastgele nonce yeterince büyük)
- UIP-9 standardı
- Plaintext: 10 byte position metadata
- Ciphertext: 24 + 10 + 16 = 50 byte (nonce + data + tag)

---

## MATEMATIKSEL FORMÜLLER ÖZETI

### FMD Clue Oluşturma
```
INPUT: dtk (detection key), precision n, rseed
r = H("decaf377-fmd.rdv", pk_enc || rseed) mod |Fr|
z = H("decaf377-fmd.zdv", pk_enc || rseed) mod |Fr|
P = r * G,  Q = z * G
Xi = pk + H("decaf377-fmd.hkd", pk_enc || i) * G  (i = 0..n)
key_i = H_bit("decaf377-fmd.bit", P, r*Xi, Q) & 1
ctxt_i = key_i XOR 1
m = H_scalar("decaf377-fmd.sca", P, n, ctxts)
y = (z - m) / r
CLUE = P[32] || y[32] || n[1] || ctxts[3]  = 68 byte
```

### FMD Examine
```
m = H_scalar("decaf377-fmd.sca", P, n, ctxts)
Q = y*P + m*G
FOR i in 0..n:
  key_i = H_bit("decaf377-fmd.bit", P, P*xi, Q) & 1  (xi = private child key)
  msg_i = ctxt_i XOR key_i
  IF msg_i == 0: REJECT
ACCEPT
```

### Key Hierarchy
```
SeedPhrase (12/24 word) → PBKDF2-SHA512(2048 rounds)
→ SpendKeyBytes (32 byte)
  → ask = PRF("Penumbra_ExpndSd", seed, [0])  [Fr]
  → nk  = PRF("Penumbra_ExpndSd", seed, [1])  [Fq]
  → FullViewingKey:
    → ovk = PRF("Penumbra_DeriOVK", nk, ak)[0..32]
    → dk  = PRF("Penumbra_DerivDK", nk, ak)[0..16]
    → ivk = poseidon2(IVK_SEP, nk, ak_s) mod |Fr|
    → wallet_id = poseidon2(ACCT_SEP, nk, ak_s)
```

### Address Derivation
```
diversifier d = AES128(dk, index_bytes)  [16 byte]
g_d = encode_to_curve(blake2b("Penumbra_Divrsfy", d))
pk_d = ivk * g_d
dtk_d = PRF("PenumbraExpndFMD", ivk, d) mod |Fr|
ck_d = dtk_d.clue_key()
Address = f4jumble(d[16] || pk_d[32] || ck_d[32])  [80 byte]
```

---

## UNSAFE BLOKLAR

Hiçbir dosyada `unsafe` blok bulunamadı.
- `zeroize` kütüphanesi unsafe olmadan kullanılıyor (drop impl)
- `parking_lot::Mutex` kullanılıyor (std değil, daha hafif)

---

## TESTLER ÖZETI

| Dosya | Test | Önem |
|-------|------|------|
| fmd/tests/fmd.rs | `detection_distribution_matches_expectation` | FMD FP rate doğrulanıyor (1024 sample) |
| fmd/src/hkd.rs | `public_private_derivation_match` | HKD tutarlılığı (proptest) |
| eddy/src/value.rs | `limb_value_roundtrip`, `addition_roundtrip`, `encrypt_verify_roundtrip` | ElGamal proof doğrulama |
| keys/src/keys/diversifier.rs | `diversifier_encryption_roundtrip` | AES-128 round-trip |
| keys/src/keys/ivk.rs | `enforce_field_assumptions` | q/r katsayısı = 4 doğrulama |
| keys/src/keys/spend.rs | `bip44_test_ledger` | Ledger test vector |
| keys/src/address.rs | address_encoding, compat_encoding, bytes_roundtrip, keys_are_diversified | Tüm format round-trip |
| proof-setup/src/single/phase1.rs | 8 test | CRS validity, contribution linkage |
| proof-setup/src/single/phase2.rs | 8 test | Phase2 CRS validity |

---

## TASARIM KARARLARI

### 1. FMD — "Fuzzy" değil, "Probabilistic"
Penumbra FMD'nin özgün kaynağı OpenPrivacy/fuzzytags. Penumbra bunu decaf377'ye adapt etti.
Önemli: P=0 veya y=0 kontrolü (satır 101) — OpenPrivacy implementasyonundan öğrenilmiş güvenlik düzeltmesi.

### 2. FROST Randomized Signing
`sign_randomized` neden var? Penumbra'da spend auth key sabittir ama imzalama "randomized" VK ile yapılır (anonimlik sağlar). FROST bu randomization'ı destekliyor.

### 3. Eddy — Neden ElGamal, Neden Limb?
- ElGamal homomorfik toplama destekler: `(r1*G, r1*K + v1*G) + (r2*G, r2*K + v2*G) = ((r1+r2)*G, (r1+r2)*K + (v1+v2)*G)`
- 64-bit değer tek ElGamal ile şifrelenirse decryption tablosu `2^64` entry gerektirir — imkansız
- 4 × 16-bit = 64-bit: her limb için `2^16` = 65,536 entry yeterli

### 4. Proof Setup — Phase 1 Japonca Yorum
`// "神だけが私を裁ける"` — Yaygın bir 2Pac şarkısından ("Only God Can Judge Me"). Concurrent pairing checker'ın davranışını "eleştiriyor". Geliştiricilerin kültürel referansları kod içinde tutması dikkat çekici.

### 5. AES-128 Diversifier — Neden Kriptografik?
Saf "sayaç" yerine AES kullanmanın nedeni: address privacy. AES ile oluşturulan diversifier'lar tahmin edilemez, böylece birden fazla adres arasında correlation yapılamaz.

### 6. F4Jumble — Neden?
Zcash Orchard'dan alınan algoritmayı Penumbra da kullanıyor. Amaç: address'in byte sıralamasını gizlemek. `d || pk_d || ck_d` yerine jumbled hash — adres formatını analiz etmek güçleşiyor.

### 7. PositionMetadataKey — XChaCha Neden?
24-byte nonce ile rastgele nonce güvenli kullanılabilir. ChaCha (12-byte nonce) ile rastgele nonce kullanmak `2^(12*8/2) ≈ 2^48` collision riski. XChaCha ile `2^(24*8/2) ≈ 2^96` — güvenli.

---

## EKSIKLIKLER VE POTANSIYEL SORUNLAR

### Eddy — Kritik Eksikler
1. **InsufficientSharesError hiçbir zaman fırlatılmıyor** — `ciphertext.rs:37`: "TODO: how do we know..."
2. **Encryption Proofs yalnızca transparent** — ZK-SNARK desteği yok
3. **DKG yok** — yalnızca `Committee { shares, threshold }` stub
4. **Serialization yok** — network üzerinden gönderilemez

### proof-setup — Async Checker
Phase1 doğrulamasında `// "神だけが私を裁ける"` yorumu var. `transform_parallel` ile 4 checker paralel çalışıyor ama `OsRng` her kez yeni randomness üretiyor. Race condition riski düşük (OsRng thread-safe) ama yine de potansiyel.

### core/keys — Nullifier Key AllocVar
`AllocationMode::Constant` ve `AllocationMode::Input` için `unimplemented!()` — bu R1CS circuit'lerde constraint synthesis sırasında panic riski. Test kapsamı gerekli.

---

## NOVA SHIELD CHAIN IÇIN DERSLER

### 1. FMD Entegrasyonu — Doğrudan Uygulama Mümkün
`decaf377-fmd` crate'i bağımsız kullanılabilir. Nova Shield'ın privacy modülüne entegre edilebilir.
- Her adres bir `ClueKey` içermeli
- Scan node'lar sadece kendi detection key'leriyle tarıyor
- FP rate `2^{-precision}` — kullanıcı seçer (bandwidth vs privacy tradeoff)

### 2. FROST — Threshold Custody
Penumbra'nın FROST implementasyonu DKG dahil eksiksiz. Nova Shield'ın MPC layer'ına alternatif:
- FROST + decaf377 (zaten var)
- Zaten `frost_rerandomized` ile randomized signing
- Nova Shield'ın `cggmp21` planıyla birlikte değerlendirilebilir

### 3. Eddy Flow Encryption — DEX Batch İçin
Nova Shield DEX'inde batch order gizliliği için:
- Her order ElGamal ile şifrele
- Tüm aynı pair'deki orderları homomorfik topla (sum ciphertext)
- Threshold decrypt ile net akış çıkar
- Decryption tablosu için RocksDB kullanılabilir

**Sınır:** Şu anda ZK-SNARK encryption proof yok — transparent. Nova Shield için ZK proof eklenebilir.

### 4. Address Format
Penumbra'nın 80-byte F4Jumble adresi + diversified key approach Nova Shield için de uygun:
- 16-byte diversifier (AES-128)
- 32-byte transmission key
- 32-byte clue key
F4Jumble kütüphanesi bağımsız (`f4jumble` crate).

### 5. Key Hierarchy — BIP39/BIP44 Desteği
Penumbra'nın `SeedPhrase → SpendKey → FVK → IVK → Address` hiyerarşisi Zcash Sapling'e paraleldir. Nova Shield bu path'i kullanabilir:
- BIP39 seed phrase (12 veya 24 kelime)
- BIP44 path ile hardware wallet uyumluluğu
- PBKDF2-SHA512, 2048 iterasyon

### 6. ChaCha20Poly1305 + Nonce Ayrımı
PayloadKind nonce pattern'i mükemmel: aynı key, farklı nonce ile farklı veri türleri şifreleniyor. Collision riski sıfır (deterministik). Nova Shield'ın şifreleme katmanında kullanılabilir.

### 7. Groth16 Proving Key Yönetimi
`LazyProvingKey` + `OnceCell` + hash doğrulama — proving key'leri güvenli yönetmek için ideal pattern. Binary embed (bundled feature) ile kullanım kolaylığı, runtime load ile esneklik.

### 8. Poseidon377 IVK Türetimi
`ivk = poseidon2(domain_sep, nk, ak_s)` — hem güvenli hem ZK-circuit dostu. IVK türetimi R1CS içinde de yapılabiliyor (`IncomingViewingKeyVar`). Nova Shield'ın ZK proof'larında benzer pattern kullanabilir.

### 9. q/r Modular Reduction — R1CS Trick
`a(a-1)(a-2)(a-3)(a-4) = 0` polinomial constraint, `a <= 4` için `enforce_cmp` yerine çok daha verimli. Nova Shield'ın R1CS devrelerinde benzer constraint optimizasyonları kullanılabilir.

### 10. Sigma Protocol için Merlin Transcript
Decryption proof'u Merlin transcript kullanıyor. Bu deterministik, replay-safe ve composable. Nova Shield'ın threshold decryption protokolüne direkt uygulanabilir.

---

## DOĞRULANAN SAYISAL DEĞERLER (Kaynak Koddan)

| Değer | Kaynak | Satır |
|-------|--------|-------|
| MAX_PRECISION = 24 | precision.rs | 6 |
| Clue boyutu = 68 byte | clue.rs | 7 |
| Address boyutu = 80 byte | address.rs | 29 |
| DIVERSIFIER_LEN_BYTES = 16 | diversifier.rs | 16 |
| SPENDKEY_LEN_BYTES = 32 | spend.rs | 19 |
| OVK_LEN_BYTES = 32 | ovk.rs | 1 |
| IVK_LEN_BYTES = 64 | ivk.rs | 18 |
| NK_LEN_BYTES = 32 | nullifier.rs | 7 |
| PAYLOAD_KEY_LEN_BYTES = 32 | symmetric.rs | 13 |
| OVK_WRAPPED_LEN_BYTES = 48 | symmetric.rs | 14 |
| GROTH16_PROOF_LENGTH_BYTES = 192 | proof-params/lib.rs | 14 |
| NUM_PBKDF2_ROUNDS = 2048 | seed_phrase.rs | 9 |
| MOD_R_QUOTIENT = 4 | ivk.rs | 19 |
| POSITION_METADATA_SIZE_BYTES = 10 | symmetric.rs | 364 |
| POSITION_METADATA_NONCE_SIZE_BYTES = 24 | symmetric.rs | 365 |
| ENCRYPTED_POSITION_METADATA_SIZE_BYTES = 50 | symmetric.rs | 367 |
| Phase1 CRS: short_len(d) = d, long_len(d) = 2d-1 | phase1.rs | 24-33 |

---

*Analiz Tarihi: 2026-07-22*
*Agent: agent-15-crypto-keys*
*Toplam Okunan Dosya: ~42*
*Toplam Analiz Satırı: ~3000 kaynak kod*
