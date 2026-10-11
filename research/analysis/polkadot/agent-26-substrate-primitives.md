# Agent-26: substrate/primitives Analizi

## Kapsanan Dosyalar

### sp-io (Host Functions)
- `substrate/primitives/io/src/lib.rs` (1870+ satir)
- `substrate/primitives/io/src/global_alloc.rs`

### sp-arithmetic
- `substrate/primitives/arithmetic/src/lib.rs`
- `substrate/primitives/arithmetic/src/fixed_point.rs` (2476 satir)
- `substrate/primitives/arithmetic/src/per_things.rs` (1920+ satir)
- `substrate/primitives/arithmetic/src/traits.rs`
- `substrate/primitives/arithmetic/src/helpers_128bit.rs`
- `substrate/primitives/arithmetic/src/rational.rs`
- `substrate/primitives/arithmetic/src/biguint.rs`

### sp-weights
- `substrate/primitives/weights/src/lib.rs`
- `substrate/primitives/weights/src/weight_v2.rs`
- `substrate/primitives/weights/src/weight_meter.rs`

### sp-trie
- `substrate/primitives/trie/src/lib.rs`
- `substrate/primitives/trie/src/storage_proof.rs`
- `substrate/primitives/trie/src/recorder.rs`
- `substrate/primitives/trie/src/node_codec.rs`
- `substrate/primitives/trie/src/trie_stream.rs`
- `substrate/primitives/trie/src/accessed_nodes_tracker.rs`
- `substrate/primitives/trie/src/proof_size_extension.rs`

### sp-staking
- `substrate/primitives/staking/src/lib.rs`
- `substrate/primitives/staking/src/offence.rs`
- `substrate/primitives/staking/src/budget.rs`
- `substrate/primitives/staking/src/currency_to_vote.rs`

### sp-consensus-*
- `substrate/primitives/consensus/babe/src/lib.rs`
- `substrate/primitives/consensus/babe/src/digests.rs`
- `substrate/primitives/consensus/babe/src/inherents.rs`
- `substrate/primitives/consensus/grandpa/src/lib.rs`
- `substrate/primitives/consensus/aura/src/lib.rs`
- `substrate/primitives/consensus/aura/src/digests.rs`
- `substrate/primitives/consensus/aura/src/inherents.rs`
- `substrate/primitives/consensus/slots/src/lib.rs`
- `substrate/primitives/consensus/beefy/src/lib.rs`

### sp-inherents
- `substrate/primitives/inherents/src/lib.rs`
- `substrate/primitives/inherents/src/client_side.rs`

### sp-block-builder
- `substrate/primitives/block-builder/src/lib.rs`

---

## 1. sp-io: Host Functions

**Kaynak:** `substrate/primitives/io/src/lib.rs`

sp-io, Substrate runtime'in WASM sandboxu disindaki "host" ortami ile iletisim kurdugu tek kanaldir. Her islem bir `#[runtime_interface]` makrosu ile tanimlanir ve runtime tarafindan `extern` cagri gibi kullanilir. Externalities ortami disinda cagrildigi zaman panic atar.

### 1.1 Storage Host Functions (satir 180-397)

```rust
pub trait Storage {
    fn get(key: &[u8]) -> Option<bytes::Bytes>
    fn read(key: &[u8], value_out: &mut [u8], value_offset: u32) -> Option<u32>
    fn set(key: &[u8], value: &[u8])
    fn clear(key: &[u8])
    fn exists(key: &[u8]) -> bool
    fn clear_prefix(prefix: &[u8])  // v1: limit yok
    fn clear_prefix(prefix: &[u8], limit: Option<u32>) -> KillStorageResult  // v2
    fn clear_prefix(maybe_prefix, maybe_limit, maybe_cursor) -> MultiRemovalResults  // v3 register_only
    fn append(key: &[u8], value: Vec<u8>)
    fn root() -> Vec<u8>  // v1: StateVersion::V0
    fn root(version: StateVersion) -> Vec<u8>  // v2
    fn changes_root(_parent_hash) -> Option<Vec<u8>>  // daima None (uyumluluk)
    fn next_key(key: &[u8]) -> Option<Vec<u8>>
    fn start_transaction()
    fn rollback_transaction()
    fn commit_transaction()
}
```

**Kritik Tasarim Kararlari:**
- `clear_prefix` v1: overlay'deki key'ler sayilmaz, sadece backend'e limit uygulanir
- v3 (`register_only`): imlecsiz cagri, sonraki cagrida `maybe_cursor` kullanilmali
- `start_transaction`/`commit_transaction` dusuk seviye API — FRAME yuksek seviye abstraction kullanmali
- `changes_root` her zaman `None` doner (eski uyumluluk icin vardir)

### 1.2 DefaultChildStorage (satir 400-617)

Ayni Storage arayuzunun child trie versiyonu. `storage_key` parametresi parent trie'daki child root'un prefixsiz adresidir.

```rust
fn get(storage_key: &[u8], key: &[u8]) -> Option<Vec<u8>>
fn set(storage_key: &[u8], key: &[u8], value: &[u8])
fn storage_kill(storage_key: &[u8])  // v1
fn storage_kill(storage_key: &[u8], limit: Option<u32>) -> bool  // v2
fn storage_kill(storage_key: &[u8], limit: Option<u32>) -> KillStorageResult  // v3
fn storage_kill(storage_key, maybe_limit, maybe_cursor) -> MultiRemovalResults  // v4 register_only
fn root(storage_key: &[u8]) -> Vec<u8>  // v1: V0
fn root(storage_key: &[u8], version: StateVersion) -> Vec<u8>  // v2
```

### 1.3 Trie Host Functions (satir 619-781)

```rust
pub trait Trie {
    fn blake2_256_root(input: Vec<(Vec<u8>, Vec<u8>)>) -> H256  // v1: V0
    fn blake2_256_root(input, version: StateVersion) -> H256  // v2: V0/V1
    fn blake2_256_ordered_root(input: Vec<Vec<u8>>) -> H256  // v1/v2
    fn keccak_256_root(input, [version]) -> H256  // v1/v2
    fn keccak_256_ordered_root(input, [version]) -> H256  // v1/v2
    fn blake2_256_verify_proof(root, proof, key, value) -> bool  // v1: V0
    fn blake2_256_verify_proof(root, proof, key, value, version) -> bool  // v2
    fn keccak_256_verify_proof(root, proof, key, value, [version]) -> bool  // v1/v2
}
```

**LayoutV0 vs LayoutV1:**
- V0: `MAX_INLINE_VALUE = None` — tum degerler node icinde inline
- V1: `MAX_INLINE_VALUE = Some(TRIE_VALUE_NODE_THRESHOLD)` — buyuk degerler ayri node

### 1.4 Crypto Host Functions (satir 876-1433)

```rust
pub trait Crypto {
    // ED25519
    fn ed25519_public_keys(id: KeyTypeId) -> Vec<ed25519::Public>
    fn ed25519_generate(id: KeyTypeId, seed: Option<Vec<u8>>) -> ed25519::Public
    fn ed25519_sign(id, pub_key, msg) -> Option<ed25519::Signature>
    fn ed25519_verify(sig: &ed25519::Signature, msg: &[u8], pub_key: &ed25519::Public) -> bool
    // UseDalekExt extension varsa ed25519-dalek, yoksa ed25519-zebra kullanir

    // SR25519
    fn sr25519_public_keys(id: KeyTypeId) -> Vec<sr25519::Public>
    fn sr25519_generate(id, seed) -> sr25519::Public
    fn sr25519_sign(id, pub_key, msg) -> Option<sr25519::Signature>
    fn sr25519_verify(sig, msg, pubkey) -> bool  // v2: strict verify
    // v1 (deprecated): verify_deprecated (daha permissif)

    // ECDSA
    fn ecdsa_public_keys(id) -> Vec<ecdsa::Public>
    fn ecdsa_generate(id, seed) -> ecdsa::Public
    fn ecdsa_sign(id, pub_key, msg) -> Option<ecdsa::Signature>
    fn ecdsa_sign_prehashed(id, pub_key, msg: &[u8;32]) -> Option<ecdsa::Signature>
    fn ecdsa_verify(sig, msg, pub_key) -> bool  // v1: overflowing sigs kabul eder
    fn ecdsa_verify(sig, msg, pub_key) -> bool  // v2: strict
    fn ecdsa_verify_prehashed(sig, msg, pub_key) -> bool

    // SECP256k1 ECDSA Recovery
    fn secp256k1_ecdsa_recover(sig: &[u8;65], msg: &[u8;32]) -> Result<[u8;64], EcdsaVerifyError>
    fn secp256k1_ecdsa_recover(sig, msg) -> Result<[u8;64], ...]>  // v2: secp256k1 crate
    fn secp256k1_ecdsa_recover_compressed(...) -> Result<[u8;33], ...>  // v1/v2

    // BLS (experimental feature)
    fn bls381_generate(id, seed) -> bls381::Public (144 bytes)
    fn bls381_generate_proof_of_possession(id, pub_key, owner) -> Option<bls381::ProofOfPossession>
    fn ecdsa_bls381_generate(id, seed) -> Public (144+33=177 bytes)

    // Bandersnatch (experimental)
    fn bandersnatch_generate(id, seed) -> bandersnatch::Public (32 bytes)
    fn bandersnatch_sign(id, pub_key, msg) -> Option<bandersnatch::Signature>

    // Deprecated batch verify (register_only)
    fn start_batch_verify()
    fn finish_batch_verify() -> bool
    fn ed25519_batch_verify(sig, msg, pub_key) -> bool  // register_only
    fn sr25519_batch_verify(sig, msg, pub_key) -> bool  // register_only
    fn ecdsa_batch_verify(sig, msg, pub_key) -> bool    // register_only
}
```

**Kritik: ed25519 Uyumluluk**
- `ed25519-dalek` (daha katı) vs `ed25519-zebra` (daha permissif) farki
- Eski zincirler genesis'ten sync edebilmek icin `UseDalekExt` extension kaydeder
- `ed25519_verify` icinde externality kontrolu yapilir (satir 935-950)

### 1.5 Hashing Host Functions (satir 1436-1477)

```rust
pub trait Hashing {
    fn keccak_256(data: &[u8]) -> [u8; 32]
    fn keccak_512(data: &[u8]) -> [u8; 64]
    fn sha2_256(data: &[u8]) -> [u8; 32]
    fn blake2_128(data: &[u8]) -> [u8; 16]
    fn blake2_256(data: &[u8]) -> [u8; 32]
    fn twox_256(data: &[u8]) -> [u8; 32]
    fn twox_128(data: &[u8]) -> [u8; 16]
    fn twox_64(data: &[u8]) -> [u8; 8]
}
```

**Hashing Ozeti:**
| Fonksiyon     | Cikti Boyutu | Kullanim Amaci                    |
|---------------|--------------|-----------------------------------|
| keccak_256    | 32 byte      | EVM uyumlulugu                    |
| keccak_512    | 64 byte      | EVM uyumlulugu (genisletilmis)    |
| sha2_256      | 32 byte      | Standart kriptografik hash        |
| blake2_128    | 16 byte      | Storage key prefix (hizli)        |
| blake2_256    | 32 byte      | Trie hash, genel kriptografik     |
| twox_256      | 32 byte      | Kriptografik olmayan, hizli       |
| twox_128      | 16 byte      | Storage key prefix (hizli, guvenilmez) |
| twox_64       | 8 byte       | Storage key suffix                |

### 1.6 Diger Host Function Gruplari

```rust
// Misc (satir 785-845)
fn print_num(val: u64)
fn print_utf8(utf8: &[u8])
fn print_hex(data: &[u8])
fn runtime_version(wasm: &[u8]) -> Option<Vec<u8>>  // cok pahali!

// Offchain (satir 1521-1763)
fn is_validator() -> bool
fn submit_transaction(data: Vec<u8>) -> Result<(), ()>
fn network_state() -> Result<OpaqueNetworkState, ()>
fn timestamp() -> Timestamp
fn local_storage_set(kind: StorageKind, key, value)
fn local_storage_compare_and_set(kind, key, old_value, value) -> bool
fn local_storage_get(kind, key) -> Option<Vec<u8>>
fn http_request_start(method, uri, meta) -> Result<HttpRequestId, ()>
fn http_request_add_header(id, name, value) -> Result<(), ()>
fn http_request_write_body(id, chunk, deadline) -> Result<(), HttpError>
fn http_response_wait(ids, deadline) -> Vec<HttpRequestStatus>
fn http_response_headers(id) -> Vec<(Vec<u8>, Vec<u8>)>
fn http_response_read_body(id, buffer, deadline) -> Result<u32, HttpError>

// Allocator (WASM only)
fn malloc(size: u32) -> Pointer<u8>
fn free(ptr: Pointer<u8>)

// OffchainIndex (on-chain'den offchain DB'ye yaz)
fn set(key: &[u8], value: &[u8])
fn clear(key: &[u8])

// Logging
fn log(level, target, message)
fn max_level() -> LogLevelFilter
```

### 1.7 SubstrateHostFunctions Amalgamation

```rust
pub type SubstrateHostFunctions = (
    storage::HostFunctions,
    default_child_storage::HostFunctions,
    misc::HostFunctions,
    wasm_tracing::HostFunctions,
    offchain::HostFunctions,
    crypto::HostFunctions,
    hashing::HostFunctions,
    allocator::HostFunctions,
    panic_handler::HostFunctions,
    logging::HostFunctions,
    sandbox::HostFunctions,  // sandboxed execution
    cvm_executor::HostFunctions,  // PolkaVM
    transaction_index::HostFunctions,
    offchain_index::HostFunctions,
    trie::HostFunctions,
)
```

---

## 2. sp-arithmetic: Fixed Point ve Per-Thing

**Kaynak:** `substrate/primitives/arithmetic/src/`

### 2.1 PerThing Tipleri

`PerThing` trait'i `[0, 1]` araligindaki kesirli degerleri integer ile temsil eder. Overflow olmaz, her zaman saturate eder.

```rust
pub trait PerThing {
    type Inner: BaseArithmetic + Unsigned + Copy + Into<u128>;
    type Upper: BaseArithmetic + Copy + From<Self::Inner> + Unsigned;
    const ACCURACY: Self::Inner;  // "1.0" in karsilik gelen tamsayi deger

    fn one() -> Self { Self::from_parts(Self::ACCURACY) }
    fn zero() -> Self { Self::from_parts(Inner::zero()) }
    fn from_percent(x: Self::Inner) -> Self
    fn from_rational<N>(p: N, q: N) -> Self  // daima DOWN round
    fn from_rational_with_rounding<N>(p, q, r: Rounding) -> Result<Self, ()>
    fn mul_floor<N>(self, b: N) -> N  // her zaman asagiya yuvarla
    fn mul_ceil<N>(self, b: N) -> N   // her zaman yukari yuvarla
    fn saturating_reciprocal_mul<N>(self, b: N) -> N  // b * (1/self)
    fn square(self) -> Self
    fn left_from_one(self) -> Self  // 1 - self
    fn less_epsilon(self) -> Self
    fn plus_epsilon(self) -> Self
}
```

**Tum PerThing Tipleri (satir 1881-1921):**

| Tip          | Inner | ACCURACY (one())          | Upper | Aciklama              |
|--------------|-------|---------------------------|-------|-----------------------|
| Percent      | u8    | 100                       | u16   | Yuzde (0-100)         |
| PerU16       | u16   | 65535                     | u32   | Parts per 65535       |
| Permill      | u32   | 1_000_000                 | u64   | Parts per Million     |
| Perbill      | u32   | 1_000_000_000             | u64   | Parts per Billion     |
| Perquintill  | u64   | 1_000_000_000_000_000_000 | u128  | Parts per Quintillion |

**Kritik Degerler:**
- `Percent::one()` = 100 (u8)
- `PerU16::one()` = 65535 (u16) — dikkat! u16::MAX ile ayni, bazi hesaplamalarda ozel durumlar
- `Permill::one()` = 1_000_000 (u32)
- `Perbill::one()` = 1_000_000_000 (u32)
- `Perquintill::one()` = 1_000_000_000_000_000_000 (u64)

**Matematiksel Modeller:**
- Standart Mul: `round_to_nearest(p * value / ACCURACY)`
- `mul_floor`: `floor(p * value / ACCURACY)`
- `mul_ceil`: `ceil(p * value / ACCURACY)`
- `from_rational(p, q)`: `floor(p * ACCURACY / q)` — cevap her zaman asagi yuvarlanir
- Overflow korumalari: upper type (U kullanir) ile overflow engellenir

**PerU16 Ozel Durumu:**
PerU16 icin `ACCURACY = 65535 = u16::MAX`. Bu durum normalizasyon hesaplamalarinda ozel durum yaratabilir — mevcut test `can_work_for_peru16` bunu denetle (satir 388-400, lib.rs).

### 2.2 Fixed Point Tipleri

`FixedPointNumber` trait'i `>1` degerleri de tutabilir.

```rust
pub trait FixedPointNumber {
    type Inner: Debug + One + CheckedMul + CheckedDiv + FixedPointOperand;
    const DIV: Self::Inner;   // hassasiyet birimi (10'un kuvveti)
    const SIGNED: bool;

    fn accuracy() -> Self::Inner { Self::DIV }
    fn from_inner(int: Self::Inner) -> Self
    fn into_inner(self) -> Self::Inner
    fn saturating_from_integer<N>(int: N) -> Self
    fn checked_from_integer<N>(int: N) -> Option<Self>
    fn saturating_from_rational<N, D>(n: N, d: D) -> Self  // panic if d=0
    fn checked_from_rational<N, D>(n: N, d: D) -> Option<Self>  // None if d=0 or overflow
    fn checked_sqrt(self) -> Option<Self>
    fn from_perbill(n: Perbill) -> Self  // 1_000_000_000 boleci
    fn into_perbill(self) -> Perbill     // saturate [0,1] disinda
}
```

**Tum Fixed Point Tipleri (satir 2439-2475):**

| Tip      | Inner | SIGNED | DIV                          | Aralik                                    |
|----------|-------|--------|------------------------------|-------------------------------------------|
| FixedI64 | i64   | true   | 1_000_000_000                | [-9223372036.8..., 9223372036.8...]       |
| FixedU64 | u64   | false  | 1_000_000_000                | [0.0, 18446744073.7...]                   |
| FixedI128| i128  | true   | 1_000_000_000_000_000_000    | [-170141183460469231731.7..., ...]        |
| FixedU128| u128  | false  | 1_000_000_000_000_000_000    | [0.0, 340282366920938463463.4...]         |

**Hassasiyet (DIV):**
- `FixedU64`/`FixedI64`: DIV = 10^9 — 9 ondalik basamak hassasiyet
- `FixedU128`/`FixedI128`: DIV = 10^18 — 18 ondalik basamak hassasiyet

**Kritik Matematik Davranislari:**
- `saturating_from_integer(x)`: ic deger `x * DIV`, overflow -> saturate
- `checked_from_rational(n, d)`: ic deger `n * DIV / d` — multiply_by_rational_with_rounding kullanir
- `into_inner()`: raw internal integer dondurur (DIV ile capraz anlamaniz gerekir)
- `from_inner(x)`: x'in DIV'e bolunmusunu temsil eder (ornegin FixedU128::from_inner(DIV) == 1.0)
- `Rounding::Minor`: signed negative icin farkli davranir (negatif sayi "down" = daha buyuk mutlak deger)

**FixedU128 Kullanim Orneği (RewardCounter gibi):**
```rust
// 1.0 temsili: FixedU128::from_inner(1_000_000_000_000_000_000u128)
// 0.5 temsili: FixedU128::from_inner(500_000_000_000_000_000u128)
// Perbill::from_percent(50) -> FixedU128 donusumu:
let fp = FixedU128::from_perbill(Perbill::from_percent(50));
// ic deger = 500_000_000 * 10^18 / 10^9 = 500_000_000_000_000_000
```

### 2.3 Arithmetic Overflow Korumalari

`sp-arithmetic/src/traits.rs` dosyasinda:
- `Saturating`: `saturating_add`, `saturating_sub`, `saturating_mul` — panik olmaz, sinirda doyar
- `CheckedAdd`, `CheckedSub`, `CheckedMul`, `CheckedDiv`: `Option<T>` dondurur
- `SaturatedConversion`: guvenli tip donusumu, overflow -> saturate
- `UniqueSaturatedInto`: daha kucuk tipe donustururken saturate

**Kullanim Karari:**
- **Para miktarlari**: `saturating_*` kullan — silme yerine kaybi tercih et
- **Kritik hesaplamalar**: `checked_*` kullan — `None` donunce hata ver
- **Gecici hesaplamalar**: `checked_*` kullan, unwrap'ta "proof; qed" yorumu ekle

### 2.4 normalize() Fonksiyonu (lib.rs satir 204-319)

```rust
pub fn normalize<T>(input: &[T], targeted_sum: T) -> Result<Vec<T>, &'static str>
where T: Clone + Copy + Ord + BaseArithmetic + Unsigned + Debug
```

`input` vektorunu hedef toplama normalize eder (staking odul dagilimi icin kullanilir). Algorithm:
1. `per_round = diff / count` — her elemana esit olarak dagit
2. `leftover = diff % count` — artan miktari en kucukten/en buyukten baslayarak 1'er ekle/cikar
3. Sonuc toplami KESINLIKLE `targeted_sum` olur

Hata durumu: `sum(input)` veya `count` tip T'ye sigmiyorsa `Err` doner.

---

## 3. sp-weights: Weight Sistemi

**Kaynak:** `substrate/primitives/weights/src/`

### 3.1 Weight Yapisi (weight_v2.rs)

```rust
pub struct Weight {
    ref_time: u64,   // nanosaniye cinsinden hesaplama suresi (referans donanima gore)
    proof_size: u64, // PoV (Proof of Validity) byte cinsinden boyut
}
```

**Neden Iki Boyutlu?**
- `ref_time`: CPU gecikme limiti — validator'in block'u isleyip isleyemeyecegini belirler
- `proof_size`: Parachain PoV boyut limiti — sistematik kullanim: parachains relay chain'e PoV gondermek zorunda, bunun boyutu sinirlanmali

Layer 1 chain icin `proof_size` dogrudan "storage proof size" anlami tasir — storage erisimleri ne kadar data kapsadigini olcer.

**Kritik Sabitler (lib.rs satir 42-48):**
```
WEIGHT_REF_TIME_PER_SECOND = 1_000_000_000_000  (1 trilyon = 10^12 pikosaniye)
WEIGHT_REF_TIME_PER_MILLIS = 1_000_000_000
WEIGHT_REF_TIME_PER_MICROS = 1_000_000
WEIGHT_REF_TIME_PER_NANOS  = 1_000
WEIGHT_PROOF_SIZE_PER_MB   = 1024 * 1024
WEIGHT_PROOF_SIZE_PER_KB   = 1024
```

Yani ref_time birimi pikosaniye degil, "referans donanim pico-second eşdeğeri" — pratikte 1 saniye = 10^12 birim.

**Weight Aritmetigi:**
```rust
// Saturating (guvensiz - debug'da panic)
let w = Weight::from_parts(ref_time, proof_size);
w.saturating_add(other)   // her iki boyut ayri saturate
w.saturating_sub(other)   //
w.saturating_mul(scalar)  // scalar ile her iki boyut
w.saturating_div(scalar)

// Checked (guvensiz - None doner)
w.checked_add(&other)     // IKISI BIRDEN overflow olursa None
w.checked_sub(&other)
w.checked_mul(scalar)
w.checked_div(scalar)
w.checked_div_per_component(&other)  // min(ref_time/other.ref_time, proof_size/other.proof_size)

// Karsilastirma
w.any_gt(other)   // herhangi bir boyut buyukse true
w.all_gt(other)   // her iki boyut buyukse true
w.any_lte(other)
w.all_lte(other)
```

**WeightToFeeCoefficient ve FeePolynomial (lib.rs satir 88-208):**
```rust
pub struct WeightToFeeCoefficient<Balance> {
    coeff_integer: Balance,
    coeff_frac: Perbill,    // Perbill kullanir
    negative: bool,
    degree: u8,
}
// Polinom: sum_i(coeff_i * x^degree_i)  burada x = ref_time
// SADECE ref_time kullanilir! proof_size fee hesabina dahil edilmez (WeightToFee icin)
```

**IdentityFee:** `weight_to_fee(w) = w.ref_time()` (1-1 eslesme)
**FixedFee:** her weight icin sabit ucret

**RuntimeDbWeight (satir 55-74):**
```rust
pub struct RuntimeDbWeight {
    pub read: u64,   // okuma ref_time maliyeti
    pub write: u64,  // yazma ref_time maliyeti
}
// reads(r) = Weight::from_parts(read * r, 0)
// writes(w) = Weight::from_parts(write * w, 0)
// reads_writes(r, w) = Weight::from_parts(read*r + write*w, 0)
```

### 3.2 WeightMeter (weight_meter.rs)

```rust
pub struct WeightMeter {
    consumed: Weight,  // harcanan
    limit: Weight,     // maksimum
}

impl WeightMeter {
    fn with_limit(limit: Weight) -> Self     // limit ile baslat
    fn new() -> Self                          // Weight::MAX limit ile baslat
    fn with_consumed_and_limit(consumed, limit) -> Self
    fn limit_to(self, weight: Weight) -> Self  // min(remaining, weight) yeni limit

    fn consumed(&self) -> Weight
    fn limit(&self) -> Weight
    fn remaining(&self) -> Weight { limit.saturating_sub(consumed) }

    // consumed_ratio = max(consumed.ref_time/limit.ref_time, consumed.proof_size/limit.proof_size)
    // Perbill olarak dondurur
    fn consumed_ratio(&self) -> Perbill

    fn consume(&mut self, w: Weight)      // debug_assert ile limit kontrolu
    fn try_consume(&mut self, w: Weight) -> Result<(), ()>  // atomik
    fn can_consume(&self, w: Weight) -> bool  // degistirmeden kontrol
    fn reclaim_proof_size(&mut self, s: u64)  // onceden hesaplanan proof_size'i geri al
}
```

**try_consume Mantig (satir 130-139):**
- `consumed + w > limit` (any_gt) ise `Err(())`
- Yoksa `Ok(())` ve consumed guncellenir
- Atomik — basariz olursa consumed degismez

**consumed_ratio:** iki boyutun maksimumunu Perbill olarak dondurur — "neyi doldurdum?" sorusunun cevabi

---

## 4. sp-trie: Merkle Trie

**Kaynak:** `substrate/primitives/trie/src/`

### 4.1 Trie Layout'lari

```rust
// LayoutV0 — klasik, inline values
pub struct LayoutV0<H>(PhantomData<H>);
impl TrieLayout for LayoutV0<H> {
    const USE_EXTENSION: bool = false;   // Patricia trie, extension node yok
    const ALLOW_EMPTY: bool = true;
    const MAX_INLINE_VALUE: Option<u32> = None;  // tum degerler inline
    type Hash = H;
    type Codec = NodeCodec<Self::Hash>;
}

// LayoutV1 — external value nodes (buyuk degerler icin)
pub struct LayoutV1<H>(PhantomData<H>);
impl TrieLayout for LayoutV1<H> {
    const USE_EXTENSION: bool = false;
    const ALLOW_EMPTY: bool = true;
    const MAX_INLINE_VALUE: Option<u32> = Some(TRIE_VALUE_NODE_THRESHOLD);
    // deger TRIE_VALUE_NODE_THRESHOLD'dan buyukse ayri node olarak saklanir
}
```

**LayoutV0 vs LayoutV1 Performans:**
- V0: butun degerleri inline saker — kucuk degerler icin efficient
- V1: buyuk degerleri ayri node'da tutar — buyuk state icin proof boyutunu azaltir
- `sp_core::storage::TRIE_VALUE_NODE_THRESHOLD` — tam degeri kontrol edilmeli

**Trie Hash Fonksiyonu:** Substrate trie'da varsayilan hasher `Blake2Hasher` (256-bit). Keccak hasher de desteklenir (EVM-uyumlu chain'ler icin).

### 4.2 StorageProof Yapisi (storage_proof.rs)

```rust
pub struct StorageProof {
    trie_nodes: BTreeSet<Vec<u8>>,  // encoded trie node'lari (lexicographic sirayla)
}

impl StorageProof {
    fn new(nodes: impl IntoIterator<Item = Vec<u8>>) -> Self
    fn new_with_duplicate_nodes_check(...) -> Result<Self, StorageProofError>
    fn empty() -> Self
    fn len() -> usize
    fn merge(proofs: impl IntoIterator<Item = Self>) -> Self  // birden fazla proof birlestir
    fn into_compact_proof<H: Hasher>(self, root: H::Out) -> Result<CompactProof, ...>
    fn to_compact_proof<H: Hasher>(&self, root: H::Out) -> Result<CompactProof, ...>
    fn into_memory_db<H: Hasher>(self) -> MemoryDB<H>
}

pub struct CompactProof {
    encoded_nodes: Vec<Vec<u8>>,  // kompakt formatta
}
```

**Proof Boyutu Hesabinin Calismasi:**
1. `Recorder` trie erisimleri sirasinda node'lari kaydeder
2. Erilen her node `StorageProof.trie_nodes` seti'ne eklenir (BTreeSet — deduplication)
3. `proof.encoded_size()` toplam byte cinsinden boyut verir
4. `CompactProof` daha kucuk — shared node'lar tekrar edilmez

**ProofSizeProvider Trait (lib.rs satir 188-220):**
```rust
pub trait ProofSizeProvider {
    fn estimate_encoded_size(&self) -> usize;
    fn start_transaction(&mut self, is_host: bool) {}
    fn rollback_transaction(&mut self) {}
    fn commit_transaction(&mut self) {}
}
```

### 4.3 TrieRecorder (recorder.rs)

```rust
pub struct Recorder<H: Hasher> {
    inner: Arc<Mutex<RecorderInner<H::Out>>>,
    recorded_keys: Arc<Mutex<HashMap<...>>>,
    encoded_size_estimation: Arc<AtomicUsize>,
}
```

- `IgnoredNodes`: onceden bilinen node'lari kaydetmez (incremental proof generation icin)
- `IgnoredNodes::from_storage_proof(proof)`: var olan proof'un node'larini atlat
- `IgnoredNodes::from_memory_db(db)`: memory DB'deki node'lari atlat
- Thread-safe: `Arc<Mutex<...>>` + `AtomicUsize` ile

### 4.4 TrieRecorderProvider ve Trie API'lari

```rust
pub trait TrieRecorderProvider<H: Hasher> {
    type Recorder<'a>: trie_db::TrieRecorder<H::Out> + 'a;
    fn drain_storage_proof(self) -> Option<StorageProof>;
    fn as_trie_recorder(&self, storage_root: H::Out) -> Self::Recorder<'_>;
}
```

**Proof Uretim Akisi:**
1. Recorder olustur
2. TrieDB'yi `recorder` ile ac
3. Key'lere eris (okuma/yazma)
4. `drain_storage_proof()` ile StorageProof al
5. Proof'u gonder, alici `verify_trie_proof(root, proof, [(key, Some(value))])` ile dogrular

**Accessed Nodes Tracker (accessed_nodes_tracker.rs):**
- Hangi node'larin erisildigi takip edilir
- Storage erisimleri `proof_size` Weight bilesenine nasil hesaplanir buradan gelir

---

## 5. sp-staking: Staking Primitives

**Kaynak:** `substrate/primitives/staking/src/`

### 5.1 Temel Tipler

```rust
pub type SessionIndex = u32;
pub type EraIndex = u32;
pub type Page = u32;

pub enum StakingAccount<AccountId> {
    Stash(AccountId),
    Controller(AccountId),  // deprecated — artik stash hesabi kullanilmali
}

pub enum StakerStatus<AccountId> {
    Idle,                          // chill
    Validator,                     // validate ediyor
    Nominator(Vec<AccountId>),     // nominate ediyor
}

pub struct Stake<Balance> {
    pub total: Balance,   // aktif + unbonding toplami
    pub active: Balance,  // aktif bonded miktar
}
```

### 5.2 StakingInterface Trait (satir 166-336)

Staking sistemi ile etkilesim icin kullanilacak standart arayuz. NPoS terminolojisi kullanir ama genel yapilandirilebilir.

```rust
pub trait StakingInterface {
    type Balance: Sub + Ord + PartialEq + Default + Copy + MaxEncodedLen + FullCodec + Saturating;
    type AccountId: Clone + Debug;
    type CurrencyToVote: CurrencyToVote<Self::Balance>;

    // Parametreler
    fn minimum_nominator_bond() -> Self::Balance;
    fn minimum_validator_bond() -> Self::Balance;
    fn bonding_duration() -> EraIndex;
    fn nominator_bonding_duration() -> EraIndex { Self::bonding_duration() }  // override edebilir
    fn current_era() -> EraIndex;
    fn desired_validator_count() -> u32;
    fn slash_reward_fraction() -> Perbill;

    // Sorgulama
    fn stake(who: &AccountId) -> Result<Stake<Balance>, DispatchError>;
    fn total_stake(who) -> Result<Balance, _> { stake(who).map(|s| s.total) }
    fn active_stake(who) -> Result<Balance, _> { stake(who).map(|s| s.active) }
    fn is_unbonding(who) -> Result<bool, _> { stake(who).map(|s| s.active != s.total) }
    fn status(who) -> Result<StakerStatus<AccountId>, DispatchError>;
    fn is_validator(who) -> bool { status(who).matches(Validator) }
    fn is_virtual_staker(who) -> bool;
    fn nominations(who) -> Option<Vec<AccountId>>;
    fn is_exposed_in_era(who, era) -> bool;
    fn election_ongoing() -> bool;
    fn stash_by_ctrl(controller) -> Result<AccountId, DispatchError>;  // deprecated

    // Islemler
    fn bond(who, value, payee) -> DispatchResult;
    fn bond_extra(who, extra) -> DispatchResult;
    fn unbond(stash, value) -> DispatchResult;    // current_era + bonding_duration'da unlock
    fn withdraw_unbonded(stash, num_slashing_spans) -> Result<bool, DispatchError>;
    fn fully_unbond(who) -> DispatchResult { unbond(who, stake(who)?.active) }
    fn chill(who) -> DispatchResult;
    fn nominate(who, validators) -> DispatchResult;
    fn set_payee(stash, reward_acc) -> DispatchResult;
    fn force_unstake(who) -> DispatchResult;  // aninda tamamen cikart
}
```

**Kritik Metot Davranislari:**
- `nominator_bonding_duration()`: pure nominator'lar (hic validator olmamis) icin daha kisa sure
- `fully_unbond()`: `active` bakiyenin tamamini unbond eder
- `withdraw_unbonded()` -> `true`: stash hesabi kapandi (artik staker degil)

### 5.3 StakingUnchecked Trait (satir 342-364)

Dusuk seviye, guvenli olmayan API. Sanal staker'lar (nomination pools gibi) icin:

```rust
pub trait StakingUnchecked: StakingInterface {
    fn migrate_to_virtual_staker(who) -> DispatchResult;
    fn virtual_bond(keyless_who, value, payee) -> DispatchResult;
    // keyless_who: tuş bilgisi olmayan hesap, dogrudan staking ile etkilesemez
    // Caller kilitleri yonetmekten sorumludur

    #[cfg(feature = "runtime-benchmarks")]
    fn migrate_to_direct_staker(who);
}
```

### 5.4 OnStakingUpdate Trait (satir 102-160)

Staking guncellemelerinde tetiklenen event listener.

```rust
pub trait OnStakingUpdate<AccountId, Balance> {
    fn on_stake_update(who, prev_stake: Option<Stake<Balance>>) {}
    fn on_nominator_add(who) {}
    fn on_nominator_update(who, prev_nominations) {}
    fn on_nominator_remove(who, nominations) {}
    fn on_validator_add(who) {}
    fn on_validator_update(who) {}
    fn on_validator_remove(who) {}
    fn on_unstake(who) {}
    fn on_slash(stash, slashed_active, slashed_unlocking: &BTreeMap<EraIndex, Balance>, total) {}
    fn on_withdraw(stash, amount) {}
}
```

`#[impl_trait_for_tuples(10)]` ile 10'a kadar tuple icin implement edilir.

### 5.5 Exposure Structs (satir 366-415)

```rust
pub struct IndividualExposure<AccountId, Balance: HasCompact> {
    pub who: AccountId,
    pub value: Balance,  // compact encoded
}

pub struct Exposure<AccountId, Balance: HasCompact> {
    pub total: Balance,  // toplam backing
    pub own: Balance,    // validator'in kendi hissesi
    pub others: Vec<IndividualExposure<AccountId, Balance>>,  // nominator hisseleri
}
```

### 5.6 Offence Trait (offence.rs)

```rust
pub type Kind = [u8; 16];  // "babe:equivocatio", "im-online:offlin" gibi

pub trait Offence<Offender> {
    const ID: Kind;
    type Slot: Clone + Codec + Ord;

    fn offenders(&self) -> Vec<Offender>;
    fn session_index(&self) -> SessionIndex;
    fn validator_set_count(&self) -> u32;
    fn slot(&self) -> Self::Slot;    // soyut zaman dilimi
    fn slash_fraction(&self, offenders_count: u32) -> Perbill;
}

pub trait ReportOffence<Reporter, Offender, O: Offence<Offender>> {
    fn report_offence(reporters: Vec<Reporter>, offence: O) -> Result<(), OffenceError>;
    fn is_known_offence(offenders: &[Offender], time_slot: &O::Slot) -> bool;
}

pub trait OnOffenceHandler<Reporter, Offender, Res> {
    fn on_offence(offenders: &[OffenceDetails<Reporter, Offender>], slash_fraction: &[Perbill], session: SessionIndex) -> Res;
}
```

**Offence Slot Mantigi:**
- `slot()` ayni "an"daki suclari gruplar
- BABE icin: slot numarasi
- GRANDPA icin: tur numarasi (epoch bittikten sonra sifirlanir)

### 5.7 Budget Primitives (budget.rs) — YENI

```rust
pub const MAX_BUDGET_KEY_LEN: u32 = 32;
pub type BudgetKey = BoundedVec<u8, ConstU32<MAX_BUDGET_KEY_LEN>>;

pub trait IssuanceCurve<Balance> {
    fn issue(total_issuance: Balance, elapsed_millis: u64) -> Balance;
}

pub trait BudgetRecipient<AccountId> {
    fn budget_key() -> BudgetKey;
    fn pot_account() -> AccountId;
}

pub trait BudgetRecipientList<AccountId> {
    fn recipients() -> Vec<(BudgetKey, AccountId)>;
}
// Tuples icin: (StakerRewards, ValidatorIncentive) gibi
```

Enflasyon dagitimi icin yeni mekanizma. Staking state'inden bagimsiz issuance hesabi yapilir (`IssuanceCurve`), ardindan `BudgetRecipient` listesine dagitilir.

---

## 6. sp-consensus-*: Consensus Primitives

### 6.1 sp-consensus-babe (babe/src/lib.rs)

**Tip Tanimlari:**
```rust
pub const KEY_TYPE: KeyTypeId = key_types::BABE;
pub type AuthorityPair = app::Pair;       // sr25519
pub type AuthoritySignature = app::Signature;
pub type AuthorityId = app::Public;       // sr25519 public key
pub const BABE_ENGINE_ID: ConsensusEngineId = *b"BABE";
pub type Randomness = [u8; 32];
pub type BabeAuthorityWeight = u64;
pub type BabeBlockWeight = u32;           // primary=1, secondary=0
pub type AuthorityIndex = u32;

// MEDIAN_ALGORITHM_CARDINALITY = 1200 (slots) — w3f'nin onerisine gore
```

**BabeConfiguration:**
```rust
pub struct BabeConfiguration {
    pub slot_duration: u64,     // milisaniye
    pub epoch_length: u64,      // slot sayisi
    pub c: (u64, u64),          // (numerator, denominator), oneri: (1,4)
    // c: slot dolu olma olasiligi, 1-c = bos kalma olasiligi
    // p = 1 - (1-c)^theta (theta = relatif stake agirlik)
    pub authorities: Vec<(AuthorityId, BabeAuthorityWeight)>,
    pub randomness: Randomness,
    pub allowed_slots: AllowedSlots,
}

pub enum AllowedSlots {
    PrimarySlots,                     // sadece VRF primary
    PrimaryAndSecondaryPlainSlots,    // primary + deterministik secondary
    PrimaryAndSecondaryVRFSlots,      // primary + VRF secondary (varsayilan)
}
```

**BabeEpochConfiguration:**
```rust
pub struct BabeEpochConfiguration {
    pub c: (u64, u64),
    pub allowed_slots: AllowedSlots,
}
impl Default for BabeEpochConfiguration {
    fn default() -> Self {
        Self { c: (1, 4), allowed_slots: AllowedSlots::PrimaryAndSecondaryVRFSlots }
    }
}
```

**Epoch Hesaplama (satir 379-395):**
```rust
pub fn epoch_index(slot: Slot, genesis_slot: Slot, epoch_duration: u64) -> u64 {
    *slot.saturating_sub(genesis_slot) / epoch_duration
}

pub fn epoch_start_slot(epoch_index: u64, genesis_slot: Slot, epoch_duration: u64) -> Slot {
    // panic olabilir sadece u64 tasmasi durumunda (pratik olarak imkansiz)
    epoch_index.checked_mul(epoch_duration)
        .and_then(|s| s.checked_add(*genesis_slot))
        .expect("proof; qed")
        .into()
}
```

**VRF Transcript Olusturma (satir 103-117):**
```rust
pub fn make_vrf_transcript(randomness: &Randomness, slot: Slot, epoch: u64) -> VrfInput {
    VrfInput::new(
        &BABE_ENGINE_ID,
        &[
            (b"slot number", &slot.to_le_bytes()),
            (b"current epoch", &epoch.to_le_bytes()),
            (b"chain randomness", randomness),
        ],
    )
}
```

**Equivocation Dogrulama (satir 282-337):**
- Her iki header farkli hash olmali
- Ayni slotu hedef aliyorlar olmali
- Ayni authority_index'e sahip olmali
- Offender'in public key'i ile imza gecerli olmali

### 6.2 BABE Digests (babe/src/digests.rs)

```rust
pub struct PrimaryPreDigest {
    pub authority_index: AuthorityIndex,
    pub slot: Slot,
    pub vrf_signature: VrfSignature,  // sr25519 VRF
}

pub struct SecondaryPlainPreDigest {
    pub authority_index: AuthorityIndex,
    pub slot: Slot,
    // VRF yok — deterministik hesaplama
}

pub struct SecondaryVRFPreDigest {
    pub authority_index: AuthorityIndex,
    pub slot: Slot,
    pub vrf_signature: VrfSignature,
}

pub enum PreDigest {
    #[codec(index = 1)] Primary(PrimaryPreDigest),
    #[codec(index = 2)] SecondaryPlain(SecondaryPlainPreDigest),
    #[codec(index = 3)] SecondaryVRF(SecondaryVRFPreDigest),
}

impl PreDigest {
    fn added_weight(&self) -> BabeBlockWeight {
        match self { Primary(_) => 1, SecondaryPlain | SecondaryVRF => 0 }
    }
}
```

**NextEpochDescriptor / NextConfigDescriptor:**
Epoch degisiminde ilk block'a eklenir — bir sonraki epoch'un authority listesi ve randomness'ini icerir.

### 6.3 sp-consensus-grandpa (grandpa/src/lib.rs)

```rust
pub const KEY_TYPE: KeyTypeId = key_types::GRANDPA;
pub type AuthorityId = app::Public;    // ed25519
pub type AuthoritySignature = app::Signature;
pub const GRANDPA_ENGINE_ID: ConsensusEngineId = *b"FRNK";
pub type AuthorityWeight = u64;
pub type AuthorityIndex = u64;
pub type SetId = u64;
pub type RoundNumber = u64;
pub type AuthorityList = Vec<(AuthorityId, AuthorityWeight)>;
```

**Mesaj Tipleri:**
```rust
pub type Message<Header> = finality_grandpa::Message<Hash, BlockNumber>;
pub type SignedMessage<Header> = finality_grandpa::SignedMessage<Hash, BlockNumber, Signature, AuthorityId>;
pub type PrimaryPropose<Header>
pub type Prevote<Header>
pub type Precommit<Header>
pub type CatchUp<Header>
pub type Commit<Header>
pub type CompactCommit<Header>
```

**GrandpaJustification:**
```rust
pub struct GrandpaJustification<Header: HeaderT> {
    pub round: u64,
    pub commit: Commit<Header>,
    pub votes_ancestries: Vec<Header>,  // precommit hedeflerinden commit hedefine routing
}
```

**ConsensusLog (GRANDPA digest items):**
```rust
pub enum ConsensusLog<N: Codec> {
    #[codec(index = 1)] ScheduledChange(ScheduledChange<N>),
    // delay blok finalizasyonunda gecerli (planned change)
    #[codec(index = 2)] ForcedChange(N, ScheduledChange<N>),
    // delay import edilmis block'lara gore (forced change) — oncelige sahip
    #[codec(index = 3)] OnDisabled(AuthorityIndex),
    #[codec(index = 4)] Pause(N),    // oy vermayI durdur (delay block finalizasyonda)
    #[codec(index = 5)] Resume(N),   // oy vermeyi surduR (delay block authorship'te)
}

pub struct ScheduledChange<N> {
    pub next_authorities: AuthorityList,
    pub delay: N,
}
```

**Kritik Farkindalik:** `ScheduledChange.delay` finalize edilmis blok sayisina gore, `ForcedChange` ise import edilmis blok sayisina gore sayar.

### 6.4 sp-consensus-aura (aura/src/lib.rs)

```rust
pub const AURA_ENGINE_ID: ConsensusEngineId = [b'a', b'u', b'r', b'a'];
pub type AuthorityIndex = u32;

// Iki crypto destegi: sr25519 ve ed25519
pub mod sr25519 { pub type AuthorityId = ...; pub type AuthoritySignature = ...; }
pub mod ed25519 { pub type AuthorityId = ...; pub type AuthoritySignature = ...; }

pub enum ConsensusLog<AuthorityId: Codec> {
    #[codec(index = 1)] AuthoritiesChange(Vec<AuthorityId>),
    #[codec(index = 2)] OnDisabled(AuthorityIndex),
}

// Runtime API
pub trait AuraApi<AuthorityId: Codec> {
    fn slot_duration() -> SlotDuration;
    fn authorities() -> Vec<AuthorityId>;
}
```

**AURA Slot Hesabi (client onceki arastirmadan):**
```
author = authorities[slot % n]
```

Basit round-robin — VRF yok, her slot onceden tahmin edilebilir.

---

## 7. sp-inherents: Inherent Data Mekanizmasi

**Kaynak:** `substrate/primitives/inherents/src/lib.rs`

### 7.1 Temel Tipler ve Arayuzler

```rust
pub type InherentIdentifier = [u8; 8];  // "timstap0", "parachn0" gibi

pub struct InherentData {
    data: BTreeMap<InherentIdentifier, Vec<u8>>,  // SCALE-encoded degerler
}

impl InherentData {
    fn put_data<I: Encode>(&mut self, identifier, inherent: &I) -> Result<(), Error>
    // Ayni identifier varsa Error::InherentDataExists

    fn replace_data<I: Encode>(&mut self, identifier, inherent: &I)
    // Varsa ustune yaz, yoksa ekle

    fn get_data<I: Decode>(&self, identifier) -> Result<Option<I>, Error>
    // Ok(None): yok, Ok(Some(I)): basarili decode, Err: decode hatasi

    fn len() -> usize
    fn identifiers() -> impl Iterator<Item = &InherentIdentifier>
}
```

### 7.2 CheckInherentsResult

```rust
pub struct CheckInherentsResult {
    okay: bool,
    fatal_error: bool,
    errors: InherentData,  // hata depolama icin InherentData yeniden kullanilir
}

impl CheckInherentsResult {
    fn put_error<E: Encode + IsFatalError>(&mut self, identifier, error: &E) -> Result<(), Error>
    // fatal error daha once raporlandiysa Error::FatalErrorReported doner
    // is_fatal_error() == true ise: diger hatalar temizlenir, fatal_error = true

    fn get_error<E: Decode>(&self, identifier) -> Result<Option<E>, Error>
    fn ok() -> bool
    fn fatal_error() -> bool
    fn into_errors(self) -> IntoIter<InherentIdentifier, Vec<u8>>
}
```

**IsFatalError Trait:**
```rust
pub trait IsFatalError {
    fn is_fatal_error(&self) -> bool;
}
// MakeFatalError<E>: her zaman fatal sayar
```

### 7.3 InherentDataProvider Trait (client_side.rs)

```rust
#[async_trait]
pub trait InherentDataProvider: Send + Sync {
    async fn provide_inherent_data(&self, inherent_data: &mut InherentData) -> Result<(), Error>;

    async fn try_handle_error(
        &self,
        identifier: &InherentIdentifier,
        error: &[u8],
    ) -> Option<Result<(), Error>>;
    // None: bu identifier ile ilgilenmiyorum
    // Some(Ok(())): hata handle edildi (orn: timestamp gelecekte, bekle)
    // Some(Err(...)): block import iptal et
}

pub trait CreateInherentDataProviders<Block, ExtraArgs> {
    type InherentDataProviders: InherentDataProvider;
    async fn create_inherent_data_providers(&self, parent: Block::Hash, extra_args: ExtraArgs)
        -> Result<Self::InherentDataProviders>;
}
```

**Inherent Yasam Dongusu:**
1. Block producer: `InherentDataProvider::provide_inherent_data()` ile `InherentData` olustur
2. `BlockBuilder::inherent_extrinsics(inherent_data)` runtime cagrisi ile extrinsic listesi al
3. Her inherent extrinsic normal block uygulamasina uygula
4. Import sirasinda: `BlockBuilder::check_inherents(block, inherent_data)` dogrula
5. Hata olursa: `try_handle_error()` ile recover veya import iptal

**Bilinen Inherent Identifier'lari:**
- Timestamp pallet: `b"timstap0"` (8 byte)
- Parachains: `b"parachn0"`
- BABE: `b"babeslot"` (genellikle)
- AURA: `b"auraslot"` (genellikle)

---

## 8. Diger Onemli Primitives

### 8.1 sp-block-builder (block-builder/src/lib.rs)

```rust
#[api_version(6)]
pub trait BlockBuilder {
    fn apply_extrinsic(extrinsic: Block::Extrinsic) -> ApplyExtrinsicResult;
    // v5 oncesi: legacy ByteSizedError kullanirdi

    fn finalize_block() -> Block::Header;
    // "finalise_block" v3'ten once

    fn inherent_extrinsics(inherent: InherentData) -> Vec<Block::Extrinsic>;

    fn check_inherents(block: Block::LazyBlock, data: InherentData) -> CheckInherentsResult;
}
```

**Kritik:** `apply_extrinsic` v6 `ApplyExtrinsicResult` kullaniyor, v5 oncesi `ByteSizedError` kullaniyordu. `LazyBlock`: block'un lazy-decode edilmis versiyonu (inherent check sirasinda tam decode gerekmez).

### 8.2 sp-consensus-beefy (beefy/src/lib.rs)

BEEFY (Bridge Efficiency Enabling Finality Yielder) — Polkadot'u Ethereum gibi external zincirlerle bridge'lemek icin:
- MMR (Merkle Mountain Range) commitments
- Schnorr/ECDSA imzalari (ecdsa-bls381 destegi)
- Light client dogrulama icin minimal proof

```rust
pub struct Commitment<TBlockNumber> {
    pub payload: Payload,
    pub block_number: TBlockNumber,
    pub validator_set_id: ValidatorSetId,
}

pub struct SignedCommitment<TBlockNumber, TSignature> {
    pub commitment: Commitment<TBlockNumber>,
    pub signatures: Vec<Option<TSignature>>,  // her authority icin optional imza
}
```

### 8.3 sp-consensus-slots (slots/src/lib.rs)

```rust
pub struct Slot(u64);  // newtype wrapper
pub struct SlotDuration(Duration);

pub struct EquivocationProof<H, Id> {
    pub offender: Id,
    pub slot: Slot,
    pub first_header: H,
    pub second_header: H,
}
```

Hem BABE hem AURA bu temel tipleri paylasilir.

---

## 9. Kritik Sayisal Degerler ve Hassasiyetler

### 9.1 Tam PerThing Degerleri

| Tip          | `one()` = ACCURACY        | Inner | Upper |
|--------------|---------------------------|-------|-------|
| Percent      | 100                       | u8    | u16   |
| PerU16       | 65_535                    | u16   | u32   |
| Permill      | 1_000_000                 | u32   | u64   |
| Perbill      | 1_000_000_000             | u32   | u64   |
| Perquintill  | 1_000_000_000_000_000_000 | u64   | u128  |

### 9.2 FixedPoint DIV Degerleri

| Tip       | DIV                          | Inner | Ondalik Hassasiyeti |
|-----------|------------------------------|-------|---------------------|
| FixedI64  | 1_000_000_000                | i64   | 9 basamak           |
| FixedU64  | 1_000_000_000                | u64   | 9 basamak           |
| FixedI128 | 1_000_000_000_000_000_000    | i128  | 18 basamak          |
| FixedU128 | 1_000_000_000_000_000_000    | u128  | 18 basamak          |

### 9.3 Weight Sabitleri

```
WEIGHT_REF_TIME_PER_SECOND = 1_000_000_000_000  (1 saniye = 10^12 unit)
WEIGHT_PROOF_SIZE_PER_MB   = 1_048_576  (1024 * 1024)
WEIGHT_PROOF_SIZE_PER_KB   = 1024
```

Tipik block weight limiti Polkadot'ta: `Weight::from_parts(500_000_000_000, 5 * 1024 * 1024)`
(yarım saniye hesaplama, 5 MB PoV)

### 9.4 BABE Sabitleri

```
KEY_TYPE = BABE (sr25519)
BABE_ENGINE_ID = *b"BABE"
RANDOMNESS_LENGTH = 32 bytes
MEDIAN_ALGORITHM_CARDINALITY = 1200 slots
BabeBlockWeight: primary=1, secondary=0
Default c = (1, 4) — slot dolu olma olasiligi 1/4
Default AllowedSlots = PrimaryAndSecondaryVRFSlots
```

### 9.5 GRANDPA Sabitleri

```
KEY_TYPE = GRANDPA (ed25519)
GRANDPA_ENGINE_ID = *b"FRNK"
AuthorityWeight = u64
SetId = u64 (monoton artar)
RoundNumber = u64 (epoch'ta sifirlenir)
```

### 9.6 Storage Proof Boyutu

- `StorageProof`: `BTreeSet<Vec<u8>>` — deduplicated node'lar
- `CompactProof`: daha kucuk format (shared path prefix'ler bir kez saklanir)
- Proof boyutu = erilen her trie node'un toplam SCALE-encoded boyutu
- Node buyuklugu: branch node ~33 + (16 * hash) = ~545 byte, leaf node ~hash + value

### 9.7 Inherent Identifier Boyutu

```
InherentIdentifier = [u8; 8]  // 8 byte sabit
InherentData: BTreeMap<[u8;8], Vec<u8>>  // key=identifier, value=encoded data
```

---

## 10. Nova Shield Chain Icin Cikarsimlar

### 10.1 Host Function Secimi (sp-io)

**Kullanilacaklar:**
- `storage::*`: Tum storage CRUD islemleri
- `hashing::blake2_256`, `blake2_128`: Storage key'leri icin (twox'dan daha guvenli)
- `hashing::keccak_256`, `sha2_256`: EVM uyumlulugu ve cross-chain hash'ler
- `crypto::sr25519_*`: Konsensus imzalama (BABE benzeri VRF)
- `crypto::ed25519_*`: GRANDPA benzeri finality oylamalari icin
- `crypto::secp256k1_ecdsa_recover`: Ethereum adresi recovery icin
- `trie::blake2_256_verify_proof`: Storage proof dogrulama

**Dikkat Edilecekler:**
- Offchain HTTP cagrisi sadece offchain worker context'te calisir — `OffchainWorkerExt` required
- `clear_prefix` v1 (limitsiz) production'da tehlikeli — her zaman limit kullan
- `ed25519_verify` davranisi `UseDalekExt`'e bagli — genesis'ten sync icin dikkat

### 10.2 Arithmetic Tasarim Kararlari

**Nova Shield Chain icin onerilen kullanim:**

1. **Para miktarlari ve bakiye hesaplari:** `saturating_*` kullan
   - Mantik: kullanici bakiyesi asla negatif olmamali, underflow -> 0

2. **Faiz/emission hesaplari:** `FixedU128` kullan (18 basamak hassasiyet)
   - DIV = 10^18 ile 1 token = 10^18 pikosaniye precision
   - `Perbill` sadece %0.0000001 hassasiyete sahip, yeterli degil

3. **Oran hesaplari (stake/toplam):** `Perbill` yeterli (milyarda bir)
   - `slash_fraction`, `commission` icin

4. **Normalizasyon:** `normalize()` fonksiyonunu `sp_arithmetic::normalize` olarak import et
   - Staker odul dagilimi icin

5. **PerU16 dikkat:** `ACCURACY = 65535 = u16::MAX` — toplam hesaplamalarinda overflow riski

### 10.3 Weight Sistemi Entegrasyonu

**Nova Shield Chain Layer 1 icin:**
- `ref_time`: consensus'un zamanlama limiti, her extrinsic'e benchmark yap
- `proof_size`: Storage erisimleri otomatik track edilir — her DB okuma proof_size arttirir
- `WeightMeter`: Her pallet call'da, ozellikle cok asamali islemlerde kullan
- `WeightToFeePolynomial`: Linear fee genellikle yeterli — `IdentityFee` ile basla
- `RuntimeDbWeight`: Custom donanim icin kalibre et

**Kritik:** `WeightToFee` sadece `ref_time` kullanir, `proof_size` kullanmaz. Proof size fee'yi ayri bir mekanizma ile (ornegin max storage fee) hesapla.

### 10.4 Trie ve Storage Proof

**Kullanim Senaryolari:**
- Cross-chain dogrulama: Diger zincirden storage proof al, `verify_trie_proof` ile dogrula
- Light client destegi: Her block'a storage proof ekle
- `LayoutV1` tercih et (buyuk degerler icin daha efficient PoV)

**Proof Boyutu Optimizasyonu:**
- `IgnoredNodes` kullanarak incremental proof'lar olustur
- `StorageProof::merge()` ile birden fazla prooflu block'lari birlestir
- `CompactProof` gonder — %30-50 daha kucuk

### 10.5 Staking Primitives Kullanimi

**Nova Shield Chain icin:**
- `StakingInterface` trait'ini implement et — nominasyon havuzu, delegation, vb. pallet'ler bu interface uzerinden calisir
- `StakingUnchecked` sadece nomination pools ve trusted pallet'ler icin implemente et
- `OnStakingUpdate` ile DEX ve diger pallet'leri staking olaylarindan haberdar et
- `budget::IssuanceCurve` ile enflasyon egrisini merkezi yonet

**Yeni `budget.rs` (sanal para basimi):**
```rust
// Nova Shield Chain enflasyon yuzu:
struct NovaIssuance;
impl IssuanceCurve<Balance> for NovaIssuance {
    fn issue(total_issuance: Balance, elapsed_millis: u64) -> Balance {
        // yillik %X oraniyla hesapla
        let rate = Perquintill::from_rational(X_RATE_PARTS, 1_000_000_000_000_000_000u64);
        let elapsed_year_fraction = FixedU128::from_rational(elapsed_millis, MILLIS_PER_YEAR);
        // ...
    }
}
```

### 10.6 Consensus Primitive Secimi

**Nova Shield Chain icin:**
- **Block authorship:** BABE (VRF-based) veya Sassafras (yeni, gizli secimleri destekler)
- **Finality:** GRANDPA (oy agirlikli, onlarca/yuzlerce validator ile olceklenebilir)
- **Inherents:** Timestamp (zorunlu), ayrica chain-ozgul veri icin ozel inherent

**VRF Yapilandirmasi (BABE benzeri):**
```rust
// VRF transcript icin girdi:
// (slot, epoch, chain_randomness) -> VRF output
// Primary assignment: VrfOutput < threshold = (2^128) * (1 - (1-c)^stake_weight)
// Secondary fallback: slot_number % n == authority_index (round-robin)
```

**GRANDPA Key Tipleri:**
- `ed25519` kullanir — sr25519'dan farkli!
- BABE/AURA `sr25519`, GRANDPA `ed25519`

### 10.7 Inherent Data Entegrasyonu

```rust
// Timestamp inherent icin:
const TIMESTAMP_ID: InherentIdentifier = *b"timstap0";

// Custom inherent ornegi:
const NOVA_ID: InherentIdentifier = *b"novashld";

// Provider:
struct NovaShieldInherentProvider { ... }
#[async_trait]
impl InherentDataProvider for NovaShieldInherentProvider {
    async fn provide_inherent_data(&self, data: &mut InherentData) -> Result<(), Error> {
        data.put_data(NOVA_ID, &self.current_data)?;
        Ok(())
    }
    async fn try_handle_error(&self, id, err) -> Option<Result<(), Error>> {
        if *id != NOVA_ID { return None; }
        // decode and handle
    }
}
```

### 10.8 Genel Guvenlik Notlari

1. **ed25519 dalek vs zebra farki**: Genesis'ten sync destegi gerekiyorsa `UseDalekExt` yonetimini planla
2. **PerU16 kullanimini sinirla**: u16 max kapasitesi normalizasyonda hatalara yol acabilir; Perbill tercih et
3. **`clear_prefix` limitsiz kullanimi yasak**: Production'da her zaman limit ver
4. **`WeightMeter::consume` debug panic**: Sadece debug'da — production'da sessizce saturate eder
5. **Inherent duplicate hatasi**: `put_data` duplicate kontrolu yapar, `replace_data` kullanilirsa kontrol yapilmaz
6. **GRANDPA vs BABE key tipleri**: BABE=sr25519, GRANDPA=ed25519 — ayni keystore'dan farkli key type'lar

---

## Kaynak Referans Tablosu

| Modul              | Dosya                                                  | Kritik Satirlar |
|--------------------|--------------------------------------------------------|-----------------|
| Storage API        | primitives/io/src/lib.rs                               | 180-617         |
| Trie API           | primitives/io/src/lib.rs                               | 619-781         |
| Crypto API         | primitives/io/src/lib.rs                               | 876-1433        |
| Hashing API        | primitives/io/src/lib.rs                               | 1436-1477       |
| PerThing trait     | primitives/arithmetic/src/per_things.rs                | 128-200         |
| PerThing impls     | primitives/arithmetic/src/per_things.rs                | 1881-1921       |
| FixedPointNumber   | primitives/arithmetic/src/fixed_point.rs               | 99-200          |
| Fixed impls (DIV)  | primitives/arithmetic/src/fixed_point.rs               | 2439-2475       |
| Weight struct      | primitives/weights/src/weight_v2.rs                    | 39-46           |
| Weight constants   | primitives/weights/src/lib.rs                          | 42-48           |
| WeightMeter        | primitives/weights/src/weight_meter.rs                 | 44-150          |
| WeightToFee        | primitives/weights/src/lib.rs                          | 88-208          |
| LayoutV0/V1        | primitives/trie/src/lib.rs                             | 81-168          |
| StorageProof       | primitives/trie/src/storage_proof.rs                   | 43-200          |
| Recorder           | primitives/trie/src/recorder.rs                        | 40-100          |
| StakingInterface   | primitives/staking/src/lib.rs                          | 166-336         |
| StakingUnchecked   | primitives/staking/src/lib.rs                          | 342-364         |
| Offence trait      | primitives/staking/src/offence.rs                      | 45-88           |
| BudgetRecipient    | primitives/staking/src/budget.rs                       | 39-80           |
| BabeConfiguration  | primitives/consensus/babe/src/lib.rs                   | 136-218         |
| BabePreDigest      | primitives/consensus/babe/src/digests.rs               | 34-126          |
| VRF transcript     | primitives/consensus/babe/src/lib.rs                   | 103-117         |
| GrandpaJustif.     | primitives/consensus/grandpa/src/lib.rs                | 131-137         |
| GRANDPA ConsLog    | primitives/consensus/grandpa/src/lib.rs                | 150-194         |
| AURA API           | primitives/consensus/aura/src/lib.rs                   | 86-97           |
| InherentData       | primitives/inherents/src/lib.rs                        | 208-278         |
| CheckInherents     | primitives/inherents/src/lib.rs                        | 286-365         |
| InherentProvider   | primitives/inherents/src/client_side.rs                | (async trait)   |
| BlockBuilder API   | primitives/block-builder/src/lib.rs                    | 32-59           |
