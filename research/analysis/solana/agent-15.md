# Solana ZK Token SDK Analizi — Agent 15

**Analiz Tarihi:** 2026-07-22
**Kapsam:** zk-token-sdk/src/ (64 dosya)
**Analiz Eden:** Agent 15
**Odak:** Confidential Transfer ZK Proof Sistemi — ElGamal, Pedersen, Bulletproofs, Sigma Proofs

---

## GENEL MİMARİ

Solana'nın `zk-token-sdk` kütüphanesi, SPL Token-2022 programının "Confidential Transfer" (gizli transfer) uzantısı için tasarlanmış eksiksiz bir kriptografik ispat sistemidir. Sistemin temelinde şu katmanlar bulunur:

```
zk-token-sdk/src/
├── lib.rs                          — Kök modül
├── macros.rs                       — Operatör makroları
├── errors.rs                       — Hata tipleri
├── transcript.rs                   — Fiat-Shamir transkripti (Merlin)
│
├── curve25519/                     — Elliptik eğri primitifleri
│   ├── mod.rs
│   ├── curve_syscall_traits.rs     — Trait'ler (on-chain syscall)
│   ├── edwards.rs                  — Edwards25519
│   ├── ristretto.rs               — Ristretto255
│   ├── scalar.rs                  — Scalar aritmetiği
│   └── errors.rs
│
├── encryption/                     — Şifreleme primitifleri
│   ├── elgamal.rs                  — Twisted ElGamal şifreleme
│   ├── grouped_elgamal.rs          — Grouped (çoklu alıcı) ElGamal
│   ├── pedersen.rs                 — Pedersen commitment
│   ├── discrete_log.rs            — Baby-step giant-step DLOG çözücü
│   └── auth_encryption.rs         — AES-128-GCM-SIV (bakiye şifreleme)
│
├── range_proof/                    — Bulletproofs range proof
│   ├── mod.rs                      — Ana RangeProof struct
│   ├── generators.rs               — BulletproofGens
│   ├── inner_product.rs            — InnerProductProof
│   └── util.rs                    — Yardımcı fonksiyonlar
│
├── sigma_proofs/                   — Sigma protokolleri
│   ├── mod.rs
│   ├── ciphertext_commitment_equality_proof.rs
│   ├── ciphertext_ciphertext_equality_proof.rs
│   ├── zero_balance_proof.rs
│   ├── pubkey_proof.rs
│   ├── grouped_ciphertext_validity_proof.rs
│   ├── batched_grouped_ciphertext_validity_proof.rs
│   ├── fee_proof.rs
│   └── errors.rs
│
├── instruction/                    — Proof instruction data
│   ├── mod.rs
│   ├── transfer/
│   │   ├── mod.rs
│   │   ├── without_fee.rs
│   │   ├── with_fee.rs
│   │   └── encryption.rs
│   ├── withdraw.rs
│   ├── zero_balance.rs
│   ├── pubkey_validity.rs
│   ├── range_proof.rs
│   ├── batched_range_proof/
│   ├── ciphertext_ciphertext_equality.rs
│   ├── ciphertext_commitment_equality.rs
│   ├── grouped_ciphertext_validity.rs
│   ├── batched_grouped_ciphertext_validity.rs
│   ├── fee_sigma.rs
│   └── errors.rs
│
└── zk_token_elgamal/               — POD (Plain Old Data) tipleri
    ├── pod/
    │   ├── elgamal.rs
    │   ├── pedersen.rs
    │   ├── grouped_elgamal.rs
    │   ├── sigma_proofs.rs
    │   ├── range_proof.rs
    │   ├── instruction.rs
    │   └── auth_encryption.rs
    ├── convert.rs
    ├── decryption.rs
    └── ops.rs
```

---

## 1. LIB.RS — KÖK MODÜL

**Dosya:** `src/lib.rs`

### Global Sabitler (satır 43-47)

```rust
const UNIT_LEN: usize = 32;
const RISTRETTO_POINT_LEN: usize = UNIT_LEN;   // 32 bytes
const SCALAR_LEN: usize = UNIT_LEN;             // 32 bytes
```

### Derleme Koşulları

- `#[cfg(not(target_os = "solana"))]` — Prover tarafı (client): şifreleme, ispat üretimi
- Validator/on-chain: sadece `curve25519`, `instruction`, `zk_token_elgamal`, `zk_token_proof_*` modülleri

**Nova Shield Chain Dersi:** Bu ayrım kritik. Nova Shield Chain'de de on-chain doğrulama kodu minimal tutulmalı. Proof generation (prover) tamamen off-chain.

---

## 2. ENCRYPTION MODÜLLERİ

### 2.1 ElGamal Şifreleme (`encryption/elgamal.rs`)

**Dosya boyutu:** 1075 satır

#### Matematiksel Temel

Solana'nın "Twisted ElGamal" şeması:
- Klasik ElGamal'dan farkı: mesaj doğrudan scalar olarak encode edilmez, Pedersen commitment'a embed edilir
- Formül: `Ciphertext = (C, D)` where:
  - `C = x*G + r*H`  (Pedersen commitment)
  - `D = r*P`         (decrypt handle, P = public key)
- Decrypt: `C - s*D = x*G + r*H - s*(r*P) = x*G + r*H - r*s*(s^{-1}*H) = x*G` (s = secret key, P = s^{-1}*H)

#### Byte Uzunlukları (satır 59-71)

```rust
const DECRYPT_HANDLE_LEN: usize = RISTRETTO_POINT_LEN;           // 32
const ELGAMAL_CIPHERTEXT_LEN: usize = PEDERSEN_COMMITMENT_LEN + DECRYPT_HANDLE_LEN; // 64
const ELGAMAL_PUBKEY_LEN: usize = RISTRETTO_POINT_LEN;           // 32
const ELGAMAL_SECRET_KEY_LEN: usize = SCALAR_LEN;                // 32
const ELGAMAL_KEYPAIR_LEN: usize = ELGAMAL_PUBKEY_LEN + ELGAMAL_SECRET_KEY_LEN; // 64
```

#### Struct Tanımları

**`ElGamalError` enum (satır 73-85)**
```rust
pub enum ElGamalError {
    DerivationMethodNotSupported,
    SeedLengthTooShort,
    SeedLengthTooLong,
    CiphertextDeserialization,
    PubkeyDeserialization,
}
```

**`ElGamal` struct (satır 88)**
```rust
pub struct ElGamal;  // Algoritma handle — durumsuz
```

**`ElGamalKeypair` (satır 182-187) — `Zeroize` derive**
```rust
pub struct ElGamalKeypair {
    pub public: ElGamalPubkey,   // P = s^{-1} * H
    secret: ElGamalSecretKey,    // s (Scalar)
}
// Derive: Clone, Debug, Deserialize, PartialEq, Eq, Serialize, Zeroize
```

**`ElGamalPubkey` (satır 330-331)**
```rust
pub struct ElGamalPubkey(RistrettoPoint);
// Derive: Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Zeroize
```

**`ElGamalSecretKey` (satır 409-411) — `#[zeroize(drop)]`**
```rust
pub struct ElGamalSecretKey(Scalar);
// Derive: Clone, Debug, Deserialize, Serialize, Zeroize
// #[zeroize(drop)] — drop sırasında sıfırlanır
```

**`ElGamalCiphertext` (satır 563-567)**
```rust
pub struct ElGamalCiphertext {
    pub commitment: PedersenCommitment,  // C = x*G + r*H
    pub handle: DecryptHandle,           // D = r*P
}
// Derive: Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize
```

**`DecryptHandle` (satır 699)**
```rust
pub struct DecryptHandle(RistrettoPoint);
// D = r * P  (Pedersen opening * public key)
```

#### Kritik Fonksiyonlar

**`ElGamal::keygen()` (satır 95-102)**
```rust
fn keygen() -> ElGamalKeypair {
    let mut s = Scalar::random(&mut OsRng);
    let keypair = Self::keygen_with_scalar(&s);
    s.zeroize();  // scalar hemen sıfırlanır
    keypair
}
```

**`ElGamal::encrypt()` (satır 121-126)**
```rust
fn encrypt<T: Into<Scalar>>(public: &ElGamalPubkey, amount: T) -> ElGamalCiphertext {
    let (commitment, opening) = Pedersen::new(amount);
    let handle = public.decrypt_handle(&opening);
    ElGamalCiphertext { commitment, handle }
}
```

**`ElGamalPubkey::new()` (satır 335-340)**
```rust
pub fn new(secret: &ElGamalSecretKey) -> Self {
    let s = &secret.0;
    assert!(s != &Scalar::zero());
    ElGamalPubkey(s.invert() * &(*H))   // P = s^{-1} * H
}
```

**`ElGamalSecretKey::new_from_signer()` (satır 416-423) — Deterministik türetim**
```rust
pub fn new_from_signer(signer: &dyn Signer, public_seed: &[u8]) -> Result<Self, ...> {
    let seed = Self::seed_from_signer(signer, public_seed)?;
    let key = Self::from_seed(&seed)?;
    Ok(key)
}
```

**`ElGamalSecretKey::seed_from_signer()` (satır 428-446)**
```rust
// message = b"ElGamalSecretKey" || public_seed
// seed = SHA3-512(signer.sign(message))
pub fn seed_from_signer(signer: &dyn Signer, public_seed: &[u8]) -> Result<Vec<u8>, SignerError> {
    let message = [b"ElGamalSecretKey", public_seed].concat();
    let signature = signer.try_sign_message(&message)?;
    // Reject default (all-zero) signatures
    if bool::from(signature.as_ref().ct_eq(Signature::default().as_ref())) {
        return Err(SignerError::Custom("Rejecting default signatures".into()));
    }
    let mut hasher = Sha3_512::new();
    hasher.update(signature.as_ref());
    Ok(hasher.finalize().to_vec())
}
```

**`ElGamalSecretKey::from_seed()` (satır 456-467)**
```rust
pub fn from_seed(seed: &[u8]) -> Result<Self, ElGamalError> {
    const MINIMUM_SEED_LEN: usize = 32;
    const MAXIMUM_SEED_LEN: usize = 65535;
    // Validates seed length
    Ok(ElGamalSecretKey(Scalar::hash_from_bytes::<Sha3_512>(seed)))
}
```

**`ElGamal::decrypt()` (satır 159-164) — Matematiği**
```rust
fn decrypt(secret: &ElGamalSecretKey, ciphertext: &ElGamalCiphertext) -> DiscreteLog {
    DiscreteLog::new(
        *G,
        ciphertext.commitment.get_point() - &(&secret.0 * &ciphertext.handle.0),
    )
    // = C - s*D = (x*G + r*H) - s*(r*s^{-1}*H) = x*G
    // Sonuç: x*G — discrete log çözülmesi gerekir
}
```

**`ElGamal::encrypt_with()` (satır 131-140) — Belirli opening ile şifreleme**
```rust
fn encrypt_with<T: Into<Scalar>>(amount: T, public: &ElGamalPubkey, opening: &PedersenOpening) -> ElGamalCiphertext {
    let commitment = Pedersen::with(amount, opening);
    let handle = public.decrypt_handle(opening);
    ElGamalCiphertext { commitment, handle }
}
```

**`ElGamal::encode()` (satır 146-151) — Sıfır opening ile**
```rust
pub fn encode<T: Into<Scalar>>(amount: T) -> ElGamalCiphertext {
    let commitment = Pedersen::encode(amount);
    let handle = DecryptHandle(RistrettoPoint::identity());
    ElGamalCiphertext { commitment, handle }
    // Herhangi bir public key altında geçerli — opening = 0
}
```

**`DecryptHandle::new()` (satır 701-703)**
```rust
pub fn new(public: &ElGamalPubkey, opening: &PedersenOpening) -> Self {
    Self(&public.0 * opening.get_scalar())  // D = P * r
}
```

#### Homomorphic Operatörler (satır 629-770)

ElGamalCiphertext ve DecryptHandle için `Add`, `Sub`, `Mul<Scalar>`, `Mul<Ciphertext>`:

```rust
// Ciphertext + Ciphertext
(C1 + C2, D1 + D2)   // Homomorfik toplama

// Ciphertext - Ciphertext
(C1 - C2, D1 - D2)   // Homomorfik çıkarma

// Ciphertext * Scalar
(s*C, s*D)            // Scalar çarpım

// Scalar * Ciphertext
```

**`add_amount()` (satır 569-576) — Sabit miktar ekleme**
```rust
pub fn add_amount<T: Into<Scalar>>(&self, amount: T) -> Self {
    let point = amount.into() * &(*G);
    let commitment_to_add = PedersenCommitment::new(point);
    ElGamalCiphertext {
        commitment: &self.commitment + &commitment_to_add,
        handle: self.handle,  // handle değişmez (opening = 0)
    }
}
```

#### Testler (satır 772-1074)

| Test | Amacı |
|------|-------|
| `test_encrypt_decrypt_correctness` | amount=57 şifrele/çöz, tek thread |
| `test_encrypt_decrypt_correctness_multithreaded` | 4 thread DLOG çözümü |
| `test_decrypt_handle` | Aynı Pedersen opening, farklı handle'lar |
| `test_homomorphic_addition` | Homomorfik toplama doğruluğu |
| `test_homomorphic_subtraction` | Homomorfik çıkarma |
| `test_homomorphic_multiplication` | Scalar çarpım |
| `test_serde_*` | Serialization/deserialization |
| `test_write_keypair_file` | Dosyaya JSON yazma, `chmod 0o600` kontrolü |
| `test_secret_key_new_from_signer` | Signer tabanlı türetim, null signer reddi |
| `test_keypair_from_seed` | Seed uzunluk validasyonu (32-65535) |
| `test_keypair_from_seed_phrase_and_passphrase` | BIP39 mnemonic'ten türetim |

---

### 2.2 Pedersen Commitment (`encryption/pedersen.rs`)

**Dosya:** 355 satır

#### Matematiksel Temel

```
C = x*G + r*H
```
- `G` = Ristretto basepoint (RISTRETTO_BASEPOINT_POINT)
- `H` = Hash-to-point(G) via SHA3-512 (satır 31-33)
- `x` = committed message (scalar)
- `r` = blinding factor / opening (scalar)

#### Sabitler

```rust
const PEDERSEN_OPENING_LEN: usize = SCALAR_LEN;        // 32 bytes
pub(crate) const PEDERSEN_COMMITMENT_LEN: usize = RISTRETTO_POINT_LEN; // 32 bytes
```

#### Generator Noktaları (satır 27-33)

```rust
lazy_static! {
    pub static ref G: RistrettoPoint = RISTRETTO_BASEPOINT_POINT;
    pub static ref H: RistrettoPoint =
        RistrettoPoint::hash_from_bytes::<Sha3_512>(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes());
}
```

**Önemli:** H, G'nin discrete log'u bilinmiyor. Bu, bağlayıcılığı garanti eder.

#### Struct'lar

**`Pedersen` (satır 36) — durumsuz handle**

**`PedersenOpening` (satır 77) — `#[zeroize(drop)]`**
```rust
pub struct PedersenOpening(Scalar);
// r (blinding factor) — drop sırasında sıfırlanır
```

**`PedersenCommitment` (satır 177)**
```rust
pub struct PedersenCommitment(RistrettoPoint);
// C = x*G + r*H
```

#### Fonksiyonlar

**`Pedersen::new()` (satır 44-49) — Rastgele opening ile**
```rust
pub fn new<T: Into<Scalar>>(amount: T) -> (PedersenCommitment, PedersenOpening) {
    let opening = PedersenOpening::new_rand();  // OsRng
    let commitment = Pedersen::with(amount, &opening);
    (commitment, opening)
}
```

**`Pedersen::with()` (satır 56-61) — Belirli opening ile**
```rust
pub fn with<T: Into<Scalar>>(amount: T, opening: &PedersenOpening) -> PedersenCommitment {
    let x: Scalar = amount.into();
    let r = opening.get_scalar();
    PedersenCommitment(RistrettoPoint::multiscalar_mul(&[x, *r], &[*G, *H]))
    // C = x*G + r*H
}
```

**`Pedersen::encode()` (satır 67-69) — Sıfır opening**
```rust
pub fn encode<T: Into<Scalar>>(amount: T) -> PedersenCommitment {
    PedersenCommitment(amount.into() * &(*G))
    // C = x*G  (r = 0)
}
```

#### Homomorfik Operatörler (satır 119-256)

`PedersenOpening` ve `PedersenCommitment` için `Add`, `Sub`, `Mul<Scalar>`:
```rust
// C1 + C2 = (x1 + x2)*G + (r1 + r2)*H
// C * s = (s*x)*G + (s*r)*H
// r1 + r2 = opening1 + opening2  (scalar toplama)
```

**`ConstantTimeEq` for `PedersenOpening` (satır 113-117)**
```rust
impl ConstantTimeEq for PedersenOpening {
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)  // Timing-safe karşılaştırma
    }
}
```

---

### 2.3 Discrete Log (`encryption/discrete_log.rs`)

**Dosya:** 335 satır
**Not:** `#![cfg(not(target_os = "solana"))]` — On-chain çalışmaz

#### Algoritma: Baby-Step Giant-Step (BSGS)

```
x = x_lo + 2^16 * x_hi
x_lo ∈ [0, 2^16)   (online aşama)
x_hi ∈ [0, 2^16)   (precomputation hashmap)
```

#### Sabitler (satır 33-37)

```rust
const TWO16: u64 = 65536;   // 2^16
const TWO17: u64 = 131072;  // 2^17
const MAX_THREAD: usize = 65536;
```

#### Struct'lar

**`DiscreteLog` (satır 51-65)**
```rust
pub struct DiscreteLog {
    pub generator: RistrettoPoint,
    pub target: RistrettoPoint,
    num_threads: usize,            // Varsayılan: 1
    range_bound: usize,            // 2^16 / num_threads
    step_point: RistrettoPoint,    // G (varsayılan)
    compression_batch_size: usize, // 32 (varsayılan)
}
```

**`DecodePrecomputation` (satır 68)**
```rust
pub struct DecodePrecomputation(HashMap<[u8; RISTRETTO_POINT_LEN], u16>);
// key: compressed point bytes, value: x_hi (0..2^16)
// Boyut: 2^16 = 65536 entry
```

#### Precomputation (satır 72-87)

```rust
fn decode_u32_precomputation(generator: RistrettoPoint) -> DecodePrecomputation {
    // generator = 2^17 * G
    // Hashmap: 2^17*0*G => 0, 2^17*1*G => 1, ..., 2^17*(2^16-1)*G => 2^16-1
}
```

**Precomputed hashmap:** `decode_u32_precomputation_for_G.bincode` binary dosyasından yüklenir (satır 92-95):
```rust
lazy_static! {
    pub static ref DECODE_PRECOMPUTATION_FOR_G: DecodePrecomputation = {
        static DECODE_PRECOMPUTATION_FOR_G_BINCODE: &[u8] =
            include_bytes!("decode_u32_precomputation_for_G.bincode");
        bincode::deserialize(DECODE_PRECOMPUTATION_FOR_G_BINCODE).unwrap_or_default()
    };
}
```

#### Çözüm Algoritması (satır 143-173)

```rust
pub fn decode_u32(self) -> Option<u64> {
    // Thread başına farklı başlangıç noktası
    // Thread i: target - i*G, target - (i+num_threads)*G, ...
    // Her thread: range_bound (= 2^16/num_threads) adım
    // Online lookup: hashmap'te bul => x = x_lo + 2^16 * x_hi
}
```

**Batch compression (satır 197-208):**
```rust
let batch_compressed = RistrettoPoint::double_and_compress_batch(&batch_points);
// Verimli batch compression kullanır
```

**ÖNEMLİ NOT (satır 11-15):** "NOT intended to run in constant-time. Hashtable ve thread kullanımı zamanlamayı gizlemez."

#### `RistrettoIterator` (satır 217-236)

```rust
struct RistrettoIterator {
    pub current: (RistrettoPoint, u64),
    pub step: (RistrettoPoint, u64),
}
// X + 0*P, X + 1*P, X + 2*P, ... şeklinde iterate eder
```

#### Testler

| Test | Amacı |
|------|-------|
| `test_serialize_decode_u32_precomputation_for_G` | Precomputed hashmap doğruluğu |
| `test_decode_correctness` | `amount = 4294967295` (u32::MAX) çözüm |
| `test_decode_correctness_threaded` | 4 thread, 0/1/2/3 ve u32::MAX |

---

### 2.4 Authenticated Encryption (`encryption/auth_encryption.rs`)

**Dosya:** 302 satır

**Amaç:** Kullanıcının kendi bakiyesini client-side saklaması için AES-128-GCM-SIV kullanır.

#### Sabitler (satır 35-45)

```rust
const AE_KEY_LEN: usize = 16;      // 128-bit AES key
const NONCE_LEN: usize = 12;       // 96-bit nonce
const CIPHERTEXT_LEN: usize = 24;  // 16 (msg) + 8 (tag)
const AE_CIPHERTEXT_LEN: usize = 36; // nonce + ciphertext
```

#### Struct'lar

**`AeKey` (satır 106)**
```rust
pub struct AeKey([u8; AE_KEY_LEN]);
// Zeroize derive, 128-bit AES key
```

**`AeCiphertext` (satır 220-223)**
```rust
pub struct AeCiphertext {
    nonce: Nonce,      // [u8; 12]
    ciphertext: Ciphertext, // [u8; 24]
}
```

#### Fonksiyonlar

**`AuthenticatedEncryption::encrypt()` (satır 72-87)**
```rust
fn encrypt(key: &AeKey, balance: u64) -> AeCiphertext {
    let mut plaintext = balance.to_le_bytes();   // 8 bytes little-endian
    let nonce: Nonce = OsRng.gen::<[u8; NONCE_LEN]>();
    let ciphertext = Aes128GcmSiv::new(&key.0.into())
        .encrypt(&nonce.into(), plaintext.as_ref())
        .expect("authenticated encryption");
    plaintext.zeroize();  // Plaintext sıfırlanır
    AeCiphertext { nonce, ciphertext: ciphertext.try_into().unwrap() }
}
```

**`AeKey::seed_from_signer()` (satır 125-143) — Key derivation**
```rust
// message = b"AeKey" || public_seed
// seed = SHA3-512(sign(message))
// ElGamal'dakine benzer pattern
```

---

### 2.5 Grouped ElGamal (`encryption/grouped_elgamal.rs`)

**Dosya:** 294 satır

**Amaç:** Tek bir commitment, birden fazla alıcı için farklı decrypt handle'lar.

#### Struct'lar

**`GroupedElGamal<const N: usize>` (satır 34) — const generics**
```rust
pub struct GroupedElGamal<const N: usize>;
// N: handle sayısı (transfer'de 3: source, dest, auditor)
```

**`GroupedElGamalCiphertext<const N: usize>` (satır 129-132)**
```rust
pub struct GroupedElGamalCiphertext<const N: usize> {
    pub commitment: PedersenCommitment,  // Tek commitment
    pub handles: [DecryptHandle; N],     // N adet handle
}
// Byte length: (N+1) * 32
// N=3 => 128 bytes
// N=2 => 96 bytes
```

#### Fonksiyonlar

**`GroupedElGamal::encrypt()` (satır 39-55)**
```rust
pub fn encrypt<T: Into<Scalar>>(pubkeys: [&ElGamalPubkey; N], amount: T) -> GroupedElGamalCiphertext<N> {
    let (commitment, opening) = Pedersen::new(amount);
    let handles: [DecryptHandle; N] = pubkeys.iter()
        .map(|pk| pk.decrypt_handle(&opening))
        .collect::<Vec<_>>().try_into().unwrap();
    GroupedElGamalCiphertext { commitment, handles }
}
// Tüm handle'lar aynı opening ile — hepsi aynı mesajı çözebilir
```

#### Transfer Context

Transfer işleminde kullanım:
```
TransferAmountCiphertext = GroupedElGamalCiphertext<3>
  handles[0] = source_handle     (source çözebilir)
  handles[1] = destination_handle (destination çözebilir)
  handles[2] = auditor_handle    (auditor çözebilir)

FeeEncryption = GroupedElGamalCiphertext<2>
  handles[0] = destination_handle
  handles[1] = withdraw_withheld_authority_handle
```

---

## 3. TRANSCRIPT — FİAT-SHAMIR (`transcript.rs`)

**Dosya:** 187 satır
**Kütüphane:** `merlin` — Interactive proof'ları non-interactive yapar (Fiat-Shamir heuristic)

### `TranscriptProtocol` Trait (satır 7-81)

```rust
pub trait TranscriptProtocol {
    // Domain separator'lar
    fn rangeproof_from_key_domain_separator(&mut self, n: u64);
    fn rangeproof_from_opening_domain_separator(&mut self, n: u64);
    fn innerproduct_domain_separator(&mut self, n: u64);
    fn close_account_proof_domain_separator(&mut self);
    fn withdraw_proof_domain_separator(&mut self);
    fn transfer_proof_domain_separator(&mut self);

    // Veri ekleme
    fn append_scalar(&mut self, label: &'static [u8], scalar: &Scalar);
    fn append_point(&mut self, label: &'static [u8], point: &CompressedRistretto);
    fn append_pubkey(&mut self, label: &'static [u8], point: &pod::ElGamalPubkey);
    fn append_ciphertext(&mut self, label: &'static [u8], point: &pod::ElGamalCiphertext);
    fn append_grouped_ciphertext_2_handles(&mut self, ...);
    fn append_commitment(&mut self, label: &'static [u8], point: &pod::PedersenCommitment);
    fn append_handle(&mut self, label: &'static [u8], point: &pod::DecryptHandle);

    // Sigma proof domain separators
    fn equality_proof_domain_separator(&mut self);
    fn zero_balance_proof_domain_separator(&mut self);
    fn grouped_ciphertext_validity_proof_domain_separator(&mut self);
    fn batched_grouped_ciphertext_validity_proof_domain_separator(&mut self);
    fn fee_sigma_proof_domain_separator(&mut self);
    fn pubkey_proof_domain_separator(&mut self);

    // Güvenlik kontrolü
    fn validate_and_append_point(&mut self, label: &'static [u8], point: &CompressedRistretto)
        -> Result<(), TranscriptError>;  // Identity point'i reddeder

    // Challenge üretimi
    fn challenge_scalar(&mut self, label: &'static [u8]) -> Scalar;
}
```

### Implementasyon (satır 83-186)

**`challenge_scalar()` (satır 132-137):**
```rust
fn challenge_scalar(&mut self, label: &'static [u8]) -> Scalar {
    let mut buf = [0u8; 64];
    self.challenge_bytes(label, &mut buf);
    Scalar::from_bytes_mod_order_wide(&buf)  // 64 byte => scalar (wide reduction)
}
```

**Domain Separator Örnekleri:**
```rust
fn rangeproof_from_opening_domain_separator(&mut self, n: u64) {
    self.append_message(b"dom-sep", b"rangeproof from opening v1");
    self.append_u64(b"n", n);
}

fn equality_proof_domain_separator(&mut self) {
    self.append_message(b"dom-sep", b"equality-proof")
}
```

**`validate_and_append_point()` (satır 119-130):**
```rust
fn validate_and_append_point(&mut self, label: &'static [u8], point: &CompressedRistretto)
    -> Result<(), TranscriptError> {
    if point.is_identity() {
        Err(TranscriptError::ValidationError)  // Identity point => hata
    } else {
        self.append_message(label, point.as_bytes());
        Ok(())
    }
}
```

---

## 4. RANGE PROOF — BULLETPROOFS

### 4.1 Ana RangeProof (`range_proof/mod.rs`)

**Dosya:** 474 satır
**Temel:** Bulletproofs (Bünz et al. 2018) — Aggregated range proofs

#### Struct `RangeProof` (satır 46-55)

```rust
#[allow(non_snake_case)]
pub struct RangeProof {
    pub A: CompressedRistretto,       // 32 bytes — bit decomp commitment
    pub S: CompressedRistretto,       // 32 bytes — blinding commitment
    pub T_1: CompressedRistretto,     // 32 bytes — t(x) polinom katsayısı
    pub T_2: CompressedRistretto,     // 32 bytes — t(x) polinom katsayısı
    pub t_x: Scalar,                  // 32 bytes — t(x) değeri
    pub t_x_blinding: Scalar,         // 32 bytes — blinding
    pub e_blinding: Scalar,           // 32 bytes — inner product blinding
    pub ipp_proof: InnerProductProof, // 448 bytes (withdraw) / 512 (transfer)
}
// Toplam boyut: 7*32 + ipp_proof.size
// Withdraw (64-bit, single): 7*32 + (6 log2(64))*32 + 2*32 = 736 bytes
// Transfer (128-bit, aggregated): farklı boyut
```

#### `RangeProof::new()` — Proof Üretimi (satır 70-251)

**İmza:**
```rust
pub fn new(
    amounts: Vec<u64>,
    bit_lengths: Vec<usize>,
    openings: Vec<&PedersenOpening>,
    transcript: &mut Transcript,
) -> Result<Self, RangeProofGenerationError>
```

**Algoritma Adımları:**

1. **Bit decomposition (satır 100-116):**
   ```rust
   // A = a_blinding * H + Σ_ij [ v_ij * G_ij - (1-v_ij) * H_ij ]
   // v_ij = j'inci bit of amount_i
   let v_ij = Choice::from((amount_i.checked_shr(j as u32).unwrap() & 1) as u8);
   ```

2. **Blinding vectors (satır 119-130):**
   ```rust
   let s_L: Vec<Scalar> = (0..nm).map(|_| Scalar::random(&mut OsRng)).collect();
   let s_R: Vec<Scalar> = (0..nm).map(|_| Scalar::random(&mut OsRng)).collect();
   // S = s_blinding * H + <s_L, G> + <s_R, H>
   ```

3. **Fiat-Shamir challenges:**
   ```rust
   transcript.append_point(b"A", &A);
   transcript.append_point(b"S", &S);
   let y = transcript.challenge_scalar(b"y");
   let z = transcript.challenge_scalar(b"z");
   ```

4. **Polynomial evaluation (satır 142-177):**
   ```rust
   // l(x) = (a_L - z*1) + s_L*x
   // r(x) = y^n * (a_R + z*1) + [z^2*2^n | z^3*2^n | ...] + y^n * s_R*x
   // t(x) = <l(x), r(x)> = t_0 + t_1*x + t_2*x^2
   ```

5. **Commitment to t_1, t_2:**
   ```rust
   let (T_1, t_1_blinding) = Pedersen::new(t_poly.1);
   let (T_2, t_2_blinding) = Pedersen::new(t_poly.2);
   let x = transcript.challenge_scalar(b"x");
   ```

6. **Inner product proof (satır 230-238):**
   ```rust
   let Q = w * &(*G);
   let ipp_proof = InnerProductProof::new(&Q, &G_factors, &H_factors, ...)?;
   ```

#### `RangeProof::verify()` (satır 254-355)

**Mega-check (satır 324-348) — Tek çok-scalar çarpım:**
```rust
let mega_check = RistrettoPoint::optional_multiscalar_mul(
    // A, S, T_1, T_2, H, G, L_vec, R_vec, G generators, H generators, commitments
    ...
)
.ok_or(RangeProofVerificationError::MultiscalarMul)?;

if mega_check.is_identity() { Ok(()) } else { Err(...) }
```

**`delta()` fonksiyonu (satır 413-425):**
```rust
// δ(y,z) = (z - z²)⟨1, y^{nm}⟩ - Σ z^{j+3} ⟨1, 2^n⟩
fn delta(bit_lengths: &[usize], y: &Scalar, z: &Scalar) -> Scalar {
    let nm: usize = bit_lengths.iter().sum();
    let sum_y = util::sum_of_powers(y, nm);
    let mut agg_delta = (z - z * z) * sum_y;
    let mut exp_z = z * z * z;
    for n_i in bit_lengths.iter() {
        let sum_2 = util::sum_of_powers(&Scalar::from(2u64), *n_i);
        agg_delta -= exp_z * sum_2;
        exp_z *= z;
    }
    agg_delta
}
```

#### Testler

| Test | Parametre |
|------|-----------|
| `test_single_rangeproof` | amount=55, 32-bit |
| `test_aggregated_rangeproof` | amounts=[55,77,99], bits=[64,32,32] |

---

### 4.2 Inner Product Proof (`range_proof/inner_product.rs`)

**Dosya:** 508 satır

#### Struct `InnerProductProof` (satır 21-26)

```rust
#[allow(non_snake_case)]
pub struct InnerProductProof {
    pub L_vec: Vec<CompressedRistretto>,  // 32 * log(n) bytes
    pub R_vec: Vec<CompressedRistretto>,  // 32 * log(n) bytes
    pub a: Scalar,                        // 32 bytes
    pub b: Scalar,                        // 32 bytes
}
// Boyut: (2*log(n) + 2) * 32 bytes
// n=64: (12+2)*32 = 448 bytes
// n=128: (14+2)*32 = 512 bytes
```

#### `InnerProductProof::new()` — Halving algoritması (satır 42-213)

```rust
pub fn new(
    Q: &RistrettoPoint,
    G_factors: &[Scalar],
    H_factors: &[Scalar],
    mut G_vec: Vec<RistrettoPoint>,
    mut H_vec: Vec<RistrettoPoint>,
    mut a_vec: Vec<Scalar>,
    mut b_vec: Vec<Scalar>,
    transcript: &mut Transcript,
) -> Result<Self, RangeProofGenerationError>
```

**İlk iterasyon (satır 86-158) — H_factors uygulanır:**
```rust
// L = Σ a_L[i] * g[i] * G_R[i] + Σ b_R[i] * h[i] * H_L[i] + c_L * Q
// R = Σ a_R[i] * g[i] * G_L[i] + Σ b_L[i] * h[i] * H_R[i] + c_R * Q
// u = challenge, u_inv = u^{-1}
// a_L[i] = a_L[i]*u + u_inv*a_R[i]
// b_L[i] = b_L[i]*u_inv + u*b_R[i]
```

**Sonraki iterasyonlar (satır 161-205) — H_factors uygulanmaz (zaten incorporate edildi):**
```rust
// G_L[i] = u_inv*G_L[i] + u*G_R[i]
// H_L[i] = u*H_L[i] + u_inv*H_R[i]
```

#### `verification_scalars()` (satır 221-278)

Verification için s vektörü hesaplar:
```rust
// challenges = [u_k, ..., u_1]  (oluşturma sırasına göre)
// s[i] = Π u_j^{b_j(i)}  where b_j(i) ∈ {-1, +1}
// allinv = Π u_j^{-1}
```

---

### 4.3 Bulletproof Generators (`range_proof/generators.rs`)

**Dosya:** 157 satır

#### `GeneratorsChain` (satır 14-60)

```rust
struct GeneratorsChain {
    reader: Sha3XofReader,  // Extendable output function
}
// SHAKE-256("GeneratorsChain" || label) => deterministik RistrettoPoint stream
```

**`next()` (satır 50-55):**
```rust
fn next(&mut self) -> Option<RistrettoPoint> {
    let mut uniform_bytes = [0u8; 64];
    self.reader.read(&mut uniform_bytes);
    Some(RistrettoPoint::from_uniform_bytes(&uniform_bytes))
    // Hash-to-curve: 64 bytes uniform => valid RistrettoPoint
}
```

#### `BulletproofGens` (satır 64-131)

```rust
pub struct BulletproofGens {
    pub gens_capacity: usize,
    G_vec: Vec<RistrettoPoint>,  // G generators (label = b'G')
    H_vec: Vec<RistrettoPoint>,  // H generators (label = b'H')
}
```

`MAX_GENERATOR_LENGTH = u32::MAX as usize` — 4 milyar generator desteği.

---

### 4.4 Utility Functions (`range_proof/util.rs`)

**Dosya:** 139 satır

**`VecPoly1` (satır 7-38):** Derece-1 vektör polinomu `a + b*x`
```rust
pub struct VecPoly1(pub Vec<Scalar>, pub Vec<Scalar>);
// inner_product: Karatsuba yöntemi
// t0 = <a0, b0>, t2 = <a1, b1>, t1 = <a0+a1, b0+b1> - t0 - t2
```

**`Poly2` (satır 42-48):** Derece-2 skalar polinom `a + b*x + c*x²`
```rust
pub struct Poly2(pub Scalar, pub Scalar, pub Scalar);
pub fn eval(&self, x: Scalar) -> Scalar { self.0 + x * (self.1 + x * self.2) }
```

**`ScalarExp` (satır 53-70):** `x^0, x^1, x^2, ...` iterator

**`sum_of_powers()` (satır 117-133):** O(log n) algoritma (power-of-two için)
```rust
// Σ x^i for i=0..n
// n power-of-two: 2*lg(n) adımda hesaplar
```

**`inner_product()` (satır 102-111):** `<a, b> = Σ a[i]*b[i]`

---

## 5. SIGMA PROOFS

### 5.1 Ciphertext-Commitment Equality Proof (`sigma_proofs/ciphertext_commitment_equality_proof.rs`)

**Dosya:** 428 satır
**Amacı:** ElGamal ciphertext ve Pedersen commitment'ın aynı mesajı içerdiğini kanıtlar.

#### Struct (satır 46-53)

```rust
pub struct CiphertextCommitmentEqualityProof {
    Y_0: CompressedRistretto,  // s maskleme
    Y_1: CompressedRistretto,  // x,s maskleme
    Y_2: CompressedRistretto,  // x,r maskleme
    z_s: Scalar,               // masked secret key
    z_x: Scalar,               // masked amount
    z_r: Scalar,               // masked opening
}
// Boyut: 6 * 32 = 192 bytes
```

#### Protokol (satır 74-127)

```rust
// Prover:
// s = secret key, x = amount, r = opening
// y_s, y_x, y_r = random nonces
Y_0 = y_s * P_source           // P_source = s^{-1} * H
Y_1 = y_x * G + y_s * D_source // D_source = decrypt handle
Y_2 = y_x * G + y_r * H        // Pedersen commitment masking

transcript.append_point("Y_0", &Y_0);
// ...
c = transcript.challenge_scalar("c");

z_s = c * s + y_s   // Masked secret key
z_x = c * x + y_x   // Masked amount
z_r = c * r + y_r   // Masked opening
```

#### Verification (satır 135-210)

```rust
// Batchlenmiş verification (w = batch challenge):
// z_s * P_source - c * H - Y_0 = 0
// w*(z_x * G + z_s * D_source - c * C_source - Y_1) = 0
// ww*(z_x * G + z_r * H - c * C_destination - Y_2) = 0
// Hepsi tek multiscalar_mul'da kontrol edilir
```

**Matematiksel İlişki:**
- `Y_0 = z_s*P_source - c*H` doğruysa: `P_source = s^{-1}*H` ve `s.invert() * s = 1`
- `Y_1 = z_x*G + z_s*D - c*C_source`: ciphertext ile commitment aynı x'e sahip
- `Y_2`: Pedersen commitment'ın içerdiği x de aynı

---

### 5.2 Ciphertext-Ciphertext Equality Proof (`sigma_proofs/ciphertext_ciphertext_equality_proof.rs`)

**Dosya:** 342 satır

#### Struct (satır 41-49)

```rust
pub struct CiphertextCiphertextEqualityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    Y_3: CompressedRistretto,
    z_s: Scalar,
    z_x: Scalar,
    z_r: Scalar,
}
// Boyut: 7 * 32 = 224 bytes
```

**Amacı:** İki farklı ElGamal ciphertext'in aynı mesajı içerdiğini kanıtlar (source key ile). Transfer'de source->destination aynı miktarı şifrelediğini gösterir.

#### Protokol (satır 69-127)

```rust
// Source ciphertext: (C_source, D_source) — source secret key s ile
// Destination ciphertext: (C_dest, D_dest) — destination opening r ile
// Y_0 = y_s * P_source
// Y_1 = y_x * G + y_s * D_source
// Y_2 = y_x * G + y_r * H
// Y_3 = y_r * P_destination
// z_s = c*s + y_s, z_x = c*x + y_x, z_r = c*r + y_r
```

---

### 5.3 Zero Balance Proof (`sigma_proofs/zero_balance_proof.rs`)

**Dosya:** 303 satır

#### Struct (satır 41-45)

```rust
pub struct ZeroBalanceProof {
    Y_P: CompressedRistretto,  // y * P
    Y_D: CompressedRistretto,  // y * D
    z: Scalar,                 // c*s + y
}
// Boyut: 3 * 32 = 96 bytes
```

**Amacı:** ElGamal ciphertext'in 0 şifrelediğini kanıtlar. Hesap kapatma için gerekli.

#### Protokol (satır 64-94)

```rust
// Eğer C = 0*G + r*H ve D = r*P, decrypt = C - s*D = r*(H - s*s^{-1}*H) = 0
// s = secret key, P = s^{-1}*H
// y = random nonce
Y_P = y * P
Y_D = y * D
c = challenge
z = c*s + y
```

#### Verification (satır 102-159)

```rust
// z*P - c*H - Y_P = 0  ve  z*D - c*C - Y_D = 0
// Birincisi s'in bilgisini kanıtlar
// İkincisi C = s*D olduğunu kanıtlar (yani decrypt = 0)
```

---

### 5.4 Public Key Validity Proof (`sigma_proofs/pubkey_proof.rs`)

**Dosya:** 172 satır

#### Struct (satır 40-43)

```rust
pub struct PubkeyValidityProof {
    Y: CompressedRistretto,  // y * H
    z: Scalar,               // c * s^{-1} + y
}
// Boyut: 2 * 32 = 64 bytes
```

**Amacı:** ElGamal public key'in geçerli olduğunu (yani P = s^{-1}*H formunda olduğunu) kanıtlar.

#### Protokol (satır 62-84)

```rust
// s_inv = s^{-1}
// y = random nonce
Y = y * H
c = challenge
z = c * s_inv + y  // Not: s_inv değil s kullanılır doğrulamada
```

#### Verification (satır 91-121)

```rust
// z * H - c * P - Y = 0
// z*H = (c*s^{-1} + y)*H = c*(s^{-1}*H) + y*H = c*P + Y ✓
let check = RistrettoPoint::vartime_multiscalar_mul(
    vec![&self.z, &(-&c), &(-&Scalar::one())],
    vec![&(*H), P, &Y],
);
if check.is_identity() { Ok(()) }
```

---

### 5.5 Grouped Ciphertext Validity Proof (`sigma_proofs/grouped_ciphertext_validity_proof.rs`)

**Dosya:** 373 satır

#### Struct (satır 46-52)

```rust
pub struct GroupedCiphertext2HandlesValidityProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    z_r: Scalar,
    z_x: Scalar,
}
// Boyut: 5 * 32 = 160 bytes
```

**Amacı:** Bir commitment'ın iki alıcı (destination, auditor) tarafından çözülebileceğini kanıtlar.

#### Protokol (satır 73-117)

```rust
// r = opening, x = amount
// y_r, y_x = random nonces
Y_0 = y_r * H + y_x * G   // commitment masking
Y_1 = y_r * P_dest         // destination handle masking
Y_2 = y_r * P_auditor      // auditor handle masking

c = challenge
z_r = c * r + y_r
z_x = c * x + y_x
```

#### Verification (satır 127-202)

```rust
// 3 check:
// 1. z_r*H + z_x*G - c*C - Y_0 = 0  (commitment validity)
// 2. w*(z_r*P_dest - c*D_dest - Y_1) = 0
// 3. ww*(z_r*P_auditor - c*D_auditor - Y_2) = 0
```

---

### 5.6 Batched Grouped Ciphertext Validity Proof (`sigma_proofs/batched_grouped_ciphertext_validity_proof.rs`)

**Dosya:** 159 satır

#### Struct (satır 38)

```rust
pub struct BatchedGroupedCiphertext2HandlesValidityProof(GroupedCiphertext2HandlesValidityProof);
// Wrapper — aynı boyut: 160 bytes
```

**Amacı:** İki ciphertext çiftini (lo/hi) tek proof ile doğrular.

#### Protokol (satır 50-68)

```rust
// t = transcript.challenge_scalar("t")  // batch challenge
// batched_message = amount_lo + amount_hi * t
// batched_opening = opening_lo + opening_hi * t
// Sonra tek GroupedCiphertext2HandlesValidityProof
```

**Önemli:** Lo ve hi amount'larını bir skalerle kombine eder. Bu teknik transfer amount'ı için lo/hi split'i etkin doğrular.

---

### 5.7 Fee Sigma Proof (`sigma_proofs/fee_proof.rs`)

**Dosya:** 657 satır

#### Struct'lar (satır 46-52, 466-494)

```rust
pub struct FeeSigmaProof {
    fee_max_proof: FeeMaxProof,         // Fee >= max_fee durumu
    fee_equality_proof: FeeEqualityProof, // Fee < max_fee durumu
}
// Boyut: 8 * 32 = 256 bytes

pub struct FeeMaxProof {
    Y_max_proof: CompressedRistretto,
    z_max_proof: Scalar,
    c_max_proof: Scalar,    // Partial challenge — disjunction için
}

pub struct FeeEqualityProof {
    Y_delta: CompressedRistretto,
    Y_claimed: CompressedRistretto,
    z_x: Scalar,
    z_delta: Scalar,
    z_claimed: Scalar,
}
```

#### Amacı ve Protokol

Transfer fee'nin doğru hesaplandığını kanıtlar:
- `fee >= max_fee` ise: `fee = max_fee` (doğrudan kanıtlanır)
- `fee < max_fee` ise: `delta_fee = fee_rate * transfer_amount - fee * 10000` in `[0, 10000)` aralığında

**OR-proof yapısı (satır 107-150):**
```rust
// Her iki branch de hesaplanır (constant-time):
let proof_fee_above_max = Self::create_proof_fee_above_max(...);
let proof_fee_below_max = Self::create_proof_fee_below_max(...);
// ct_gt ile seçilir (constant-time comparison)
let below_max = u64::ct_gt(&max_fee, &fee_amount);
let fee_max_proof = FeeMaxProof::conditional_select(..., below_max);
```

**`ConditionallySelectable` (satır 472-506):**
```rust
impl ConditionallySelectable for FeeMaxProof {
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        // Her field için u8::conditional_select
        Self {
            Y_max_proof: conditional_select_ristretto(&a.Y_max_proof, &b.Y_max_proof, choice),
            z_max_proof: Scalar::conditional_select(&a.z_max_proof, &b.z_max_proof, choice),
            c_max_proof: Scalar::conditional_select(&a.c_max_proof, &b.c_max_proof, choice),
        }
    }
}
```

**Simulated proof (satır 160-221) — Schnorr OR yapısı:**
```rust
// fee_above_max => fee_equality_proof simüle edilir:
// c_equality = random, z_x, z_delta, z_claimed = random
// Y_delta = z_x*G + z_delta*H - c_equality*C_delta  (simüle)
// c = c_max_proof + c_equality  =>  c_max_proof = c - c_equality
```

#### `conditional_select_ristretto()` (satır 510-520)

```rust
fn conditional_select_ristretto(a: &CompressedRistretto, b: &CompressedRistretto, choice: Choice) -> CompressedRistretto {
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::conditional_select(&a.as_bytes()[i], &b.as_bytes()[i], choice);
    }
    CompressedRistretto(bytes)
}
```

---

### 5.8 Sigma Proofs Mod (`sigma_proofs/mod.rs`)

**Helper fonksiyonlar:**
```rust
fn ristretto_point_from_optional_slice(optional_slice: Option<&[u8]>)
    -> Result<CompressedRistretto, SigmaProofVerificationError>

fn canonical_scalar_from_optional_slice(optional_slice: Option<&[u8]>)
    -> Result<Scalar, SigmaProofVerificationError>
// from_canonical_bytes ile canonical form kontrolü yapar
```

---

## 6. INSTRUCTION MODÜLLERİ

### 6.1 Instruction Mod (`instruction/mod.rs`)

#### `ProofType` enum (satır 52-71)

```rust
#[derive(Clone, Copy, Debug, FromPrimitive, ToPrimitive, PartialEq, Eq)]
#[repr(u8)]
pub enum ProofType {
    Uninitialized,
    ZeroBalance,
    Withdraw,
    CiphertextCiphertextEquality,
    Transfer,
    TransferWithFee,
    PubkeyValidity,
    RangeProofU64,
    BatchedRangeProofU64,
    BatchedRangeProofU128,
    BatchedRangeProofU256,
    CiphertextCommitmentEquality,
    GroupedCiphertext2HandlesValidity,
    BatchedGroupedCiphertext2HandlesValidity,
    FeeSigma,
}
```

#### `ZkProofData<T>` trait (satır 73-80)

```rust
pub trait ZkProofData<T: Pod> {
    const PROOF_TYPE: ProofType;
    fn context_data(&self) -> &T;
    #[cfg(not(target_os = "solana"))]
    fn verify_proof(&self) -> Result<(), ProofVerificationError>;
}
```

---

### 6.2 Transfer Instruction (`instruction/transfer/without_fee.rs`)

**Dosya:** 616 satır

#### Struct'lar

**`TransferData` (satır 56-62) — `#[repr(C)]`, `Pod`, `Zeroable`**
```rust
pub struct TransferData {
    pub context: TransferProofContext,
    pub proof: TransferProof,
}
```

**`TransferProofContext` (satır 65-79)**
```rust
pub struct TransferProofContext {
    pub ciphertext_lo: pod::TransferAmountCiphertext,  // 128 bytes
    pub ciphertext_hi: pod::TransferAmountCiphertext,  // 128 bytes
    pub transfer_pubkeys: TransferPubkeys,              // 96 bytes
    pub new_source_ciphertext: pod::ElGamalCiphertext, // 64 bytes
}
// Toplam context: 416 bytes
```

**`TransferPubkeys` (satır 82-88)**
```rust
pub struct TransferPubkeys {
    pub source: pod::ElGamalPubkey,       // 32 bytes
    pub destination: pod::ElGamalPubkey,  // 32 bytes
    pub auditor: pod::ElGamalPubkey,      // 32 bytes
}
```

**`TransferProof` (satır 290-305)**
```rust
pub struct TransferProof {
    pub new_source_commitment: pod::PedersenCommitment,                // 32 bytes
    pub equality_proof: pod::CiphertextCommitmentEqualityProof,        // 192 bytes
    pub validity_proof: pod::BatchedGroupedCiphertext2HandlesValidityProof, // 160 bytes
    pub range_proof: pod::RangeProofU128,                              // 736 bytes
}
// Toplam proof: ~1120 bytes
```

#### Transfer Bit Split Stratejisi (satır 36-48)

```rust
const TRANSFER_SOURCE_AMOUNT_BITS: usize = 64;
const TRANSFER_AMOUNT_LO_BITS: usize = 16;
const TRANSFER_AMOUNT_LO_NEGATED_BITS: usize = 16;
const TRANSFER_AMOUNT_HI_BITS: usize = 32;

// transfer_amount = amount_lo + amount_hi * 2^16
// amount_lo ∈ [0, 2^16), amount_hi ∈ [0, 2^32)
// Max transfer: 2^48 - 1

// COMMITMENT_MAX = Commit(2^16 - 1, 0)  // negated lo için üst sınır
```

#### `TransferData::new()` (satır 92-168)

```rust
pub fn new(
    transfer_amount: u64,
    (spendable_balance, ciphertext_old_source): (u64, &ElGamalCiphertext),
    source_keypair: &ElGamalKeypair,
    (destination_pubkey, auditor_pubkey): (&ElGamalPubkey, &ElGamalPubkey),
) -> Result<Self, ProofGenerationError>
```

**Adımlar:**
1. `try_split_u64(transfer_amount, 16)` => `(amount_lo, amount_hi)`
2. `TransferAmountCiphertext::new(amount_lo, source, dest, auditor)` — grouped encrypt
3. `TransferAmountCiphertext::new(amount_hi, ...)` — grouped encrypt
4. `new_source_ciphertext = old_ciphertext - (lo + hi * 2^16)_ciphertext`
5. Transcript oluştur, `TransferProof::new()` çağır

#### `TransferProof::new()` (satır 310-385)

```rust
// 1. Pedersen commitment for new source balance
let (new_source_commitment, source_opening) = Pedersen::new(source_new_balance);

// 2. Equality proof: ciphertext ↔ commitment (new source balance)
let equality_proof = CiphertextCommitmentEqualityProof::new(source_keypair, ...);

// 3. Validity proof: batched lo/hi ciphertexts
let validity_proof = BatchedGroupedCiphertext2HandlesValidityProof::new(...);

// 4. Range proof: [balance, amount_lo, amount_lo_negated, amount_hi]
let range_proof = RangeProof::new(
    vec![source_new_balance, transfer_amount_lo, transfer_amount_lo_negated, transfer_amount_hi],
    vec![64, 16, 16, 32],  // bit lengths
    vec![&source_opening, opening_lo, &opening_lo_negated, opening_hi],
    transcript,
)?;
```

#### `decrypt_amount()` (satır 222-239) — Rol bazlı çözme

```rust
pub fn decrypt_amount(&self, role: Role, sk: &ElGamalSecretKey) -> Result<u64, InstructionError> {
    let ciphertext_lo = self.ciphertext_lo(role)?;
    let ciphertext_hi = self.ciphertext_hi(role)?;
    let amount_lo = ciphertext_lo.decrypt_u32(sk);
    let amount_hi = ciphertext_hi.decrypt_u32(sk);
    // amount = amount_lo + 2^16 * amount_hi
}
```

---

### 6.3 Transfer With Fee (`instruction/transfer/with_fee.rs`)

**Dosya:** 941 satır

#### Ek Struct'lar

**`TransferWithFeeProofContext` (satır 87-108)**
```rust
pub struct TransferWithFeeProofContext {
    pub ciphertext_lo: pod::TransferAmountCiphertext,      // 128 bytes
    pub ciphertext_hi: pod::TransferAmountCiphertext,      // 128 bytes
    pub transfer_with_fee_pubkeys: TransferWithFeePubkeys, // 128 bytes
    pub new_source_ciphertext: pod::ElGamalCiphertext,     // 64 bytes
    pub fee_ciphertext_lo: pod::FeeEncryption,             // 96 bytes
    pub fee_ciphertext_hi: pod::FeeEncryption,             // 96 bytes
    pub fee_parameters: pod::FeeParameters,                // 10 bytes
}
```

**`TransferWithFeePubkeys` (satır 112-118)**
```rust
pub struct TransferWithFeePubkeys {
    pub source: pod::ElGamalPubkey,
    pub destination: pod::ElGamalPubkey,
    pub auditor: pod::ElGamalPubkey,
    pub withdraw_withheld_authority: pod::ElGamalPubkey,
}
```

**`TransferWithFeeProof` (satır 456-465)**
```rust
pub struct TransferWithFeeProof {
    pub new_source_commitment: pod::PedersenCommitment,           // 32 bytes
    pub claimed_commitment: pod::PedersenCommitment,              // 32 bytes
    pub equality_proof: pod::CiphertextCommitmentEqualityProof,   // 192 bytes
    pub ciphertext_amount_validity_proof: pod::BatchedGroupedCiphertext2HandlesValidityProof, // 160
    pub fee_sigma_proof: pod::FeeSigmaProof,                      // 256 bytes
    pub fee_ciphertext_validity_proof: pod::BatchedGroupedCiphertext2HandlesValidityProof,    // 160
    pub range_proof: pod::RangeProofU256,                         // çok büyük
}
```

#### Fee Hesaplaması (satır 776-793)

```rust
fn calculate_fee(transfer_amount: u64, fee_rate_basis_points: u16) -> Option<(u64, u64)> {
    // fee = ceiling(transfer_amount * fee_rate / 10000)
    let numerator = (transfer_amount as u128) * (fee_rate_basis_points as u128);
    let fee = (numerator + ONE_IN_BASIS_POINTS - 1) / ONE_IN_BASIS_POINTS;
    let delta_fee = fee * ONE_IN_BASIS_POINTS - numerator;
    Some((fee as u64, delta_fee as u64))
    // NOT: Division non-constant-time — transfer amount bilgisi sızabilir (satır 781-784 uyarısı)
}
```

#### Delta Commitment (satır 796-808)

```rust
fn compute_delta_commitment_and_opening(
    (combined_commitment, combined_opening): (&PedersenCommitment, &PedersenOpening),
    (combined_fee_commitment, combined_fee_opening): (&PedersenCommitment, &PedersenOpening),
    fee_rate_basis_points: u16,
) -> (PedersenCommitment, PedersenOpening) {
    let fee_rate_scalar = Scalar::from(fee_rate_basis_points);
    let delta_commitment = combined_fee_commitment * MAX_FEE_BASIS_POINTS
        - combined_commitment * fee_rate_scalar;
    // delta = fee*10000 - transfer*rate  (0 <= delta < 10000 ise fee doğru)
}
```

#### Range Proof Boyutu (satır 588-620)

`TransferWithFeeProof` için 8 adet değer:
```rust
RangeProof::new(
    vec![source_new_balance, transfer_amount_lo, transfer_amount_hi,
         delta_fee, delta_negated, fee_amount_lo, fee_amount_hi, amount_sub_fee],
    vec![64, 16, 32, 16, 16, 16, 32, 64],  // Toplam = 256 bit
    ...
)
```

---

### 6.4 Transfer Encryption (`instruction/transfer/encryption.rs`)

**Dosya:** 92 satır

**`TransferAmountCiphertext` (satır 11)**
```rust
pub struct TransferAmountCiphertext(pub(crate) GroupedElGamalCiphertext<3>);
// handles: [source, destination, auditor]
```

**`FeeEncryption` (satır 57)**
```rust
pub struct FeeEncryption(pub(crate) GroupedElGamalCiphertext<2>);
// handles: [destination, withdraw_withheld_authority]
```

---

### 6.5 Transfer Mod (`instruction/transfer/mod.rs`)

#### `Role` enum (satır 29-34)

```rust
pub enum Role {
    Source,
    Destination,
    Auditor,
    WithdrawWithheldAuthority,
}
```

#### Lo/Hi split fonksiyonları (satır 55-175)

```rust
pub fn try_split_u64(amount: u64, bit_length: usize) -> Result<(u64, u64), InstructionError>
// lo = amount & ((1 << bit_length) - 1)
// hi = amount >> bit_length

pub fn try_combine_lo_hi_u64(amount_lo: u64, amount_hi: u64, bit_length: usize) -> Result<u64, ...>
// amount = lo + hi * 2^bit_length

pub fn try_combine_lo_hi_commitments(lo: &PedersenCommitment, hi: &PedersenCommitment, bit_length: usize)
// C = C_lo + C_hi * 2^bit_length (homomorfik)

pub fn try_combine_lo_hi_openings(...)
// r = r_lo + r_hi * 2^bit_length
```

**`FeeParameters` (satır 178-184)**
```rust
pub struct FeeParameters {
    pub fee_rate_basis_points: u16,  // 0.01% increments
    pub maximum_fee: u64,
}
```

---

### 6.6 Withdraw Instruction (`instruction/withdraw.rs`)

**Dosya:** 232 satır

#### Struct'lar

**`WithdrawData` (satır 32-39)**
```rust
pub struct WithdrawData {
    pub context: WithdrawProofContext,  // 96 bytes
    pub proof: WithdrawProof,           // 736 bytes
}
```

**`WithdrawProofContext` (satır 43-51)**
```rust
pub struct WithdrawProofContext {
    pub pubkey: pod::ElGamalPubkey,           // 32 bytes
    pub final_ciphertext: pod::ElGamalCiphertext, // 64 bytes
}
```

**`WithdrawProof` (satır 122-133)**
```rust
pub struct WithdrawProof {
    pub commitment: pod::PedersenCommitment,                    // 32 bytes
    pub equality_proof: pod::CiphertextCommitmentEqualityProof, // 192 bytes
    pub range_proof: pod::RangeProofU64,                       // 512 bytes
}
// Toplam: ~736 bytes
```

#### `WithdrawData::new()` (satır 54-84)

```rust
pub fn new(amount: u64, keypair: &ElGamalKeypair, current_balance: u64, current_ciphertext: &ElGamalCiphertext) -> Result<Self, ...> {
    let final_balance = current_balance.checked_sub(amount).ok_or(ProofGenerationError::NotEnoughFunds)?;
    let final_ciphertext = current_ciphertext - &ElGamal::encode(amount);
    // ElGamal::encode(amount) = (amount*G, identity) — encoding, not encryption
    // final_ciphertext = old_ciphertext - (amount*G, 0)
}
```

---

## 7. ZK TOKEN ELGAMAL — POD TİPLERİ

### 7.1 POD ElGamal (`zk_token_elgamal/pod/elgamal.rs`)

**Dosya:** 179 satır

`#[repr(transparent)]` wrapper'lar — bytemuck ile zero-copy:

```rust
pub struct ElGamalCiphertext(pub [u8; 64]);  // commitment + handle
pub struct ElGamalPubkey(pub [u8; 32]);
pub struct DecryptHandle(pub [u8; 32]);
```

Tüm `From<decoded::X> for pod::X` ve `TryFrom<pod::X> for decoded::X` impls.

### 7.2 POD Sigma Proofs (`zk_token_elgamal/pod/sigma_proofs.rs`)

**Dosya:** 214 satır

| Tip | Boyut |
|-----|-------|
| `CiphertextCommitmentEqualityProof` | 192 bytes |
| `CiphertextCiphertextEqualityProof` | 224 bytes |
| `GroupedCiphertext2HandlesValidityProof` | 160 bytes |
| `BatchedGroupedCiphertext2HandlesValidityProof` | 160 bytes |
| `ZeroBalanceProof` | 96 bytes |
| `FeeSigmaProof` | 256 bytes |
| `PubkeyValidityProof` | 64 bytes |

**Not:** Bazı tipler power-of-two olmayan boyutlarda olduğundan `unsafe impl Pod/Zeroable` kullanılır (satır 200-213).

---

## 8. CURVE25519 MODÜLLERİ

### 8.1 Ristretto (`curve25519/ristretto.rs`)

**Dosya:** 387 satır

#### `PodRistrettoPoint` (satır 4-7)

```rust
pub struct PodRistrettoPoint(pub [u8; 32]);
// Pod, Zeroable, Clone, Copy, Debug, Default, PartialEq, Eq
```

#### On-chain syscall trait'leri (satır 12-131)

```rust
impl PointValidation for PodRistrettoPoint {
    fn validate_point(&self) -> bool {
        CompressedRistretto::from_slice(&self.0).decompress().is_some()
    }
}

impl GroupOperations for PodRistrettoPoint {
    // add, subtract, multiply — dalek crate üzerinden
}

impl MultiScalarMultiplication for PodRistrettoPoint {
    fn multiscalar_multiply(scalars: &[PodScalar], points: &[Self]) -> Option<Self> {
        // RistrettoPoint::optional_multiscalar_mul
    }
}
```

**On-chain (`target_os = "solana"`) versiyonu (satır 133-243):** Syscall'lara yönlendirir:
```rust
solana_program::syscalls::sol_curve_group_op(CURVE25519_RISTRETTO, ADD/SUB/MUL, ...)
solana_program::syscalls::sol_curve_multiscalar_mul(CURVE25519_RISTRETTO, ...)
```

---

## 9. ERRORS MODÜLLERİ

### `errors.rs` (satır 12-87)

```rust
pub enum ProofGenerationError {
    NotEnoughFunds,
    FeeCalculation,
    IllegalCommitmentLength,
    IllegalAmountBitLength,
    InvalidCommitment,
    RangeProof(#[from] RangeProofGenerationError),
    ProofLength,
}

pub enum ProofVerificationError {
    RangeProof(#[from] RangeProofVerificationError),
    SigmaProof(SigmaProofType, SigmaProofVerificationError),
    ElGamal(#[from] ElGamalError),
    ProofContext,
    IllegalCommitmentLength,
    IllegalAmountBitLength,
}

pub enum SigmaProofType {
    EqualityProof,
    ValidityProof,
    ZeroBalanceProof,
    FeeSigmaProof,
    PubkeyValidityProof,
}

pub enum TranscriptError {
    ValidationError,  // Identity point hatası
}
```

---

## 10. PROOF DATA AKIŞI — END-TO-END

### Transfer İşlemi (gizli transfer)

```
[1] Client (off-chain):
    TransferData::new(
        transfer_amount,
        (balance, old_ciphertext),
        source_keypair,
        (dest_pubkey, auditor_pubkey),
    )

    ├─ try_split_u64(amount, 16) => (lo, hi)
    ├─ GroupedElGamal<3>::encrypt_with([source, dest, auditor], lo, opening_lo)
    │    => TransferAmountCiphertext (128 bytes, 3 handles)
    ├─ GroupedElGamal<3>::encrypt_with([source, dest, auditor], hi, opening_hi)
    │    => TransferAmountCiphertext (128 bytes, 3 handles)
    ├─ new_source_ciphertext = old - (lo_src + hi_src * 2^16)
    └─ TransferProof::new(...)
         ├─ Pedersen::new(new_balance) => (commitment, opening_src)
         ├─ CiphertextCommitmentEqualityProof::new(...)  // 192 bytes
         ├─ BatchedGroupedCiphertext2HandlesValidityProof::new(...)  // 160 bytes
         └─ RangeProof::new([balance, lo, lo_neg, hi], [64, 16, 16, 32], ...)  // ~736 bytes

[2] Solana Network:
    TransferData serialize => ~1120+ bytes on-chain

[3] Validator (on-chain):
    TransferData::verify_proof()
    ├─ equality_proof.verify(source_pubkey, new_ciphertext, commitment, transcript)
    ├─ validity_proof.verify(dest_pubkey, auditor_pubkey, ...)
    └─ range_proof.verify([commitment, lo_comm, lo_neg_comm, hi_comm], [64,16,16,32], transcript)
```

---

## 11. UNSAFE BLOKLAR

### `curve25519/ristretto.rs` (satır 146-229) — On-chain syscall

```rust
let result = unsafe {
    solana_program::syscalls::sol_curve_validate_point(
        CURVE25519_RISTRETTO,
        &point.0 as *const u8,
        &mut validate_result,
    )
};
```

Tüm on-chain curve operations `unsafe` syscall ile yapılır. Off-chain versiyonda `unsafe` yok — sadece safe Rust.

### `sigma_proofs/pod/sigma_proofs.rs` (satır 200-213)

```rust
unsafe impl Zeroable for CiphertextCommitmentEqualityProof {}
unsafe impl Pod for CiphertextCommitmentEqualityProof {}
// Power-of-two olmayan boyutlar için manual Pod impl
```

---

## 12. GÜVENLİK ÖZELLİKLERİ

### Constant-Time Operasyonlar

1. **`ElGamalSecretKey::eq()` (satır 550-554):** `ct_eq` kulllanır
2. **`PedersenOpening::eq()` (satır 108-111):** `ct_eq` kullanır
3. **`FeeMaxProof::conditional_select()` (satır 472-480):** Branch-free seçim
4. **`ElGamal::seed_from_signer()` (satır 436-438):** Default signature constant-time reddedilir

### Zeroize

1. **`ElGamalSecretKey` (satır 410, 411):** `#[zeroize(drop)]` — drop sırasında scalar sıfırlanır
2. **`ElGamalKeypair` (satır 181):** `Zeroize` derive
3. **`PedersenOpening` (satır 76):** `#[zeroize(drop)]`
4. **`ElGamal::keygen()` (satır 100):** `s.zeroize()` sonra `keypair` return
5. **`auth_encryption.rs` (satır 81):** `plaintext.zeroize()` şifrelemeden sonra

### Key Derivation Security

- `ElGamalSecretKey` domain separator: `b"ElGamalSecretKey" || public_seed`
- `AeKey` domain separator: `b"AeKey" || public_seed`
- Hash: SHA3-512 (Keccak-based, post-quantum güvenli hash)
- Default signature reddi: NullSigner gibi hardware wallet olmayan cihazlarda

### Identity Point Kontrolü

`validate_and_append_point()` — transcript'e eklemeden önce identity kontrolü.

---

## 13. NOVA SHIELD CHAIN İÇİN DERSLER

### 13.1 Twisted ElGamal — Neden Kullanmalıyız?

Solana'nın twisted ElGamal seçimi mükemmel:
- `P = s^{-1} * H` formülü, Pedersen commitment sistemleriyle kusursuz entegrasyon sağlar
- Aynı opening `r` birden fazla public key'e (grouped ElGamal) ile paylaşılabilir
- Homomorfik özellikler doğrudan transfer aritmetiğine uygulanır
- **Nova Shield Chain için:** MPC context'te birden fazla katılımcı aynı commitment'ı paylaşabilir

### 13.2 Grouped ElGamal — MPC için İdeal

`GroupedElGamalCiphertext<N>` ile:
- N katılımcı (threshold şema üyeleri) aynı ciphertext'i farklı handle'lardan çözebilir
- Transfer'de `N=3` (source, dest, auditor) — Nova Shield'da `N` = threshold üyesi sayısı olabilir
- Threshold şemada her validator kendi handle'ını alır

**Öneri:** Nova Shield'da confidential transfer için:
```
GroupedElGamalCiphertext<T+1>  // T = threshold
handles[0..T] = validator_handles
handles[T] = recipient_handle
```

### 13.3 Bulletproofs — Transfer Amount Range Proof

**Lo/Hi split tekniği kritik:**
- 64-bit amount'ı 16+32+16 bit'e bölerek her parça için ayrı range proof
- Toplam range proof boyutu minimize edilir
- Aggregated bulletproofs ile birden fazla commitment tek proof'ta

**Nova Shield Chain için:** ZK token miktarları için aynı lo/hi split uygulanabilir. Threshold şema için transfer miktarları aynı Bulletproofs yapısıyla korunabilir.

### 13.4 Fiat-Shamir (Merlin Transcript)

Domain separator'ların önemi:
- Her proof tipi farklı domain separator kullanır
- Cross-protocol attack'lar engellenir
- `validate_and_append_point()` identity point saldırısını engeller

**Nova Shield için:** Her proof tipi için ayrı domain separator tanımla. `merlin` crate'i kullan.

### 13.5 Fee Sigma Proof — OR Proof Tekniği

Fee proof'u iki senaryo için OR-proof kullanır:
- Her iki branch hesaplanır (constant-time)
- `ConditionallySelectable` ile branch seçilir
- Schnorr OR yapısı: `c = c_A + c_B` ile partial challenge share

**Nova Shield için:** Şartlı işlemler (örn. threshold geçip geçmeme) için OR-proof yapısı kullanılabilir. MPC protokollerinde de benzer disjunctive proof gerekebilir.

### 13.6 Discrete Log — Güvenlik Notu

`decrypt_u32()` maks `2^32` değer çözebilir:
- On-chain bakiyeler için yeterli
- Büyük miktarlar için lo/hi split gerekli
- BSGS: `O(sqrt(N))` zaman, `O(sqrt(N))` yer

**Nova Shield için:** Şifre çözme 32-bit range'e sınırlı. Eğer daha büyük değerler gerekiyorsa lo/hi split zorunlu. Threshold şema da bunu dikkate almalı.

### 13.7 Constant-Time Kritikalliği

Solana fee proof'u açıkça söylüyor (satır 781-784):
```
// Division may involve CPU opcodes with variable execution times.
// Non-constant-time execution may reveal transfer amount.
```

**Nova Shield için:** MPC protokollerinde tüm kritik operasyonlar constant-time olmalı. `subtle` crate kullan.

### 13.8 Key Derivation Pattern

ElGamal ve AeKey için aynı pattern:
```
seed = SHA3-512(sign(domain_sep || public_seed))
```
Hardware wallet uyumluluğu için: signing key'den türetilen ZK key.

**Nova Shield için:** MPC katılımcılarının ElGamal keylerini Solana signing key'lerinden türetmesi mümkün. Bu, key yönetimini basitleştirir.

### 13.9 POD Tip Tasarımı

`#[repr(C)]` + `Pod` + `Zeroable`:
- Zero-copy serialization
- On-chain instruction data olarak doğrudan kullanılır
- Boyut sabittir (compile-time)

**Nova Shield için:** On-chain proof data için aynı POD pattern kullan. `bytemuck` crate ile zero-copy.

### 13.10 Proof Boyutları Özeti

| Proof | Boyut |
|-------|-------|
| `PubkeyValidityProof` | 64 bytes |
| `ZeroBalanceProof` | 96 bytes |
| `GroupedCiphertext2HandlesValidityProof` | 160 bytes |
| `BatchedGroupedCiphertext2HandlesValidityProof` | 160 bytes |
| `CiphertextCommitmentEqualityProof` | 192 bytes |
| `CiphertextCiphertextEqualityProof` | 224 bytes |
| `FeeSigmaProof` | 256 bytes |
| `RangeProof (64-bit, single)` | ~672 bytes |
| `RangeProof (128-bit, 4 agg)` | ~736 bytes |
| `RangeProof (256-bit, 8 agg)` | ~1024 bytes+ |
| `WithdrawProof` | ~896 bytes |
| `TransferProof` | ~1120 bytes |
| `TransferWithFeeProof` | ~2000+ bytes |

### 13.11 Matematiksel Temel Özeti

```
ElGamal: Ciphertext = (x*G + r*H, r*P)
         Decrypt: (x*G + r*H) - s*(r*s^{-1}*H) = x*G

Pedersen: C = x*G + r*H
          Binding: DL sertliği
          Hiding: r rastgele

Bulletproofs: Bit decomposition + inner product
              O(log n) verifier, O(n) prover

Sigma Proofs: Schnorr-based, Fiat-Shamir
              ZK + soundness = DLOG sertliği

Fee OR Proof: Schnorr OR = c = c_A + c_B
              Simüle edilen branch: c_B random, Y_B = z*G - c_B*C
```

### 13.12 Eksiklikler ve Uyarılar

1. **DLOG çözücü non-constant-time** — Sadece offline kullanım için
2. **Fee calculation non-constant-time** — Transfer amount bilgisi sızabilir (kabul edilmiş risk)
3. **Maksimum transfer miktarı: 2^48 - 1** — Lo 16-bit, Hi 32-bit split nedeniyle
4. **Range proof aggregation zorunluluğu:** Bit lengths toplamı 2'nin kuvveti olmalı

---

## 14. TÜM PROOF TİPLERİ TABLOSU

| Proof Tipi | Struct | Context | Boyut (approx) | Kullanım |
|------------|--------|---------|----------------|----------|
| PubkeyValidity | `PubkeyValidityProof` | `PubkeyValidityProofContext` | 64B | Hesap açma |
| ZeroBalance | `ZeroBalanceProof` | `ZeroBalanceProofContext` | 96B | Hesap kapatma |
| Withdraw | `WithdrawProof` | `WithdrawProofContext` | ~896B | Çekim |
| CiphertextCommitmentEquality | `CiphertextCommitmentEqualityProof` | `CiphertextCommitmentEqualityProofContext` | 192B | Source balance güncellemesi |
| CiphertextCiphertextEquality | `CiphertextCiphertextEqualityProof` | `CiphertextCiphertextEqualityProofContext` | 224B | Transfer equality |
| GroupedCiphertext2HandlesValidity | `GroupedCiphertext2HandlesValidityProof` | `GroupedCiphertext2HandlesValidityProofContext` | 160B | Transfer validity |
| BatchedGroupedCiphertext2HandlesValidity | `BatchedGroupedCiphertext2HandlesValidityProof` | `...` | 160B | Transfer lo+hi |
| FeeSigma | `FeeSigmaProof` | `FeeSigmaProofContext` | 256B | Fee kanıtı |
| RangeProofU64 | `RangeProof` | `RangeProofContext` | ~672B | 64-bit range |
| BatchedRangeProofU128 | `RangeProof` | `BatchedRangeProofContext` | ~736B | Transfer range |
| BatchedRangeProofU256 | `RangeProof` | `BatchedRangeProofContext` | ~1024B+ | Fee+transfer range |
| Transfer | `TransferProof` | `TransferProofContext` | ~1120B | Gizli transfer |
| TransferWithFee | `TransferWithFeeProof` | `TransferWithFeeProofContext` | ~2000B+ | Ücretli transfer |

---

## 15. ÖZET

Solana ZK Token SDK, production-ready bir confidential transfer sistemidir:

1. **Kriptografik temel:** Curve25519/Ristretto, twisted ElGamal, Pedersen commitment
2. **Proof sistemi:** Bulletproofs (range) + Sigma protocols (equality, validity, zero-balance, fee)
3. **Non-interactive:** Merlin transcript (STROBE/Keccak tabanlı) Fiat-Shamir heuristic
4. **On-chain verimlilik:** Batch multiscalar multiplication, POD tipleri, syscall integration
5. **Güvenlik:** Constant-time ops, Zeroize, domain separators, identity point checks
6. **Genişletilebilirlik:** Grouped ElGamal N-handle ile çoklu alıcı

**Nova Shield Chain için en kritik alınacak dersler:**
- Twisted ElGamal + Grouped ElGamal kombinasyonu MPC için mükemmel
- Lo/Hi split ile büyük miktarların verimli kanıtlanması
- Fee OR-proof tekniği şartlı MPC işlemleri için kullanılabilir
- Merlin transcript + domain separator mimarisi kesinlikle benimsenmeli
- Constant-time + Zeroize güvenlik pratikleri zorunlu
