# Penumbra → Nova Shield Chain: Kapsamlı Araştırma ve Uygulama Planı

**Tarih:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/`
**Analiz Kapsamı:** 31 analiz dosyası, ~262k satır kaynak kod, 210+ değer doğrulandı
**Doğrulama Durumu:** Tüm kritik değerler kaynak koddan satır satır doğrulanmış (3 doğrulama belgesi: batchA, batchB, batchC)

---

## A. TOKEN EKONOMİSİ (UM/PENUMBRA MODELI)

### A.1 Penumbra Token Modeli

**Token:** UM (Penumbra native token)
- Ondalık: `upenumbra` = 10^6 UM (Cosmos standartı, Ethereum gibi 10^18 değil)
- Staking token: `penumbra` (insan okunabilir), base denom: `upenumbra`

**Emisyon Mimarisi:**
```
DistributionsParameters {
    staking_issuance_per_epoch: u64,  // her epoch'ta oluşan yeni UM
}
```
Penumbra sabit bir issuance parametresi kullanır (yıllık azalan formül yok).

**Staking Exchange Rate Formülü (kaynak: staking/src/rate.rs):**
```
VXR[e] = VXR[e-1] * (1 + VRR[e])
VRR[e] = r_e * (1 - commission_rate)
r_e    = issuance_budget[e] / total_active_stake[e-1]

BPS² scale factor: 1_0000_0000 (10^8)
delegation_amount = floor(unbonded_amount / VXR[e])
```

**Fee Modeli:**
- Protocol fee: DEX swap için `%0.25` (25 bps)
- Base fee: 4 boyutlu gas sistemi (aşağıda detaylar)

**Nova Shield NSC ile Farklar:**
- Penumbra `10^6` ondalık, NSC `10^18` → NSC Ethereum standardını takip ediyor, değişmemeli
- Penumbra sabit issuance → NSC'de `rate_bps = max(800 - year * 50, 200)` azalan formül daha gelişmiş
- Penumbra %0.25 protocol fee → NSC DEX'te de aynı değer kullanılabilir (zaten eşleşiyor)
- BPS² scale `1_0000_0000` Nova Shield staking modülüne alınmalı (mevcut kodda muhtemelen eksik)

### A.2 Gas Sistemi (4 Boyutlu)

**Kaynak:** `crates/core/component/fee/src/gas.rs`

```rust
pub struct Gas {
    pub block_space: u64,       // tx boyutu (bytes)
    pub compact_block_space: u64, // compact block'taki alan
    pub verification: u64,      // ZK proof doğrulama
    pub execution: u64,         // hesaplama
}

// ZK proof doğrulama maliyeti: 1000 gas unit
// Execution: 10 gas unit per action

// Fee formülü:
// fee = Σ (price_i * gas_i) / 1000
// price: GasPrices{block_space_price, compact_block_space_price, verification_price, execution_price}
```

**Fee Tiers (kaynak: fee/src/fee.rs):**
```
Low    = 105% (base * 1.05)
Medium = 130% (base * 1.30)
High   = 200% (base * 2.00)
```

**Nova Shield Uygulaması:**
```rust
// nova-runtime/src/fee.rs
pub struct GasCost {
    pub block_space: u64,
    pub zk_verification: u64,  // 1000 per proof
    pub execution: u64,        // 10 per action
}

pub const ZK_PROOF_GAS: u64 = 1000;
pub const ACTION_EXECUTION_GAS: u64 = 10;
```

**Boyut Sabitleri (kaynak: core/app/src/app/mod.rs):**
```
MAX_BLOCK_TXS_PAYLOAD_BYTES = 1_048_576 (1 MB)
MAX_TRANSACTION_SIZE_BYTES  = 98_304    (96 KB)
MAX_EVIDENCE_SIZE_BYTES     = 30_720    (30 KB)
```

### A.3 Nova Shield Token Ekonomisi Önerileri

Mevcut NSC token dağılımı ve emisyon modeli Penumbra'dan daha gelişmiş. Penumbra'dan alınacak tek değer:

1. BPS² scale `1_0000_0000` → staking rate hesaplamalarında zorunlu
2. `delegation_amount = floor(unbonded_amount / VXR[e])` formülü → nova-runtime staking modülüne ekle
3. 4 boyutlu gas sistemi → nova-runtime'daki tek boyutlu gas'ı genişlet
4. Fee tier (Low/Medium/High) → mempool önceliklendirmesi için

---

## B. ZK SHIELD — NOTE VE COMMITMENT SİSTEMİ

### B.1 Kriptografik Temel

**Eğri:** BLS12-377 (BLS12-381 DEĞİL — kritik fark)
- Decaf377 grubu: BLS12-377 taban alanı (Fq) üzerinde prime-order grup (cofactor=1)
- Ristretto-benzeri encoding, cofactor sorunları yok
- `Fr` (scalar alan) ve `Fq` (taban alan) ayrımı kritik

**Nova Shield Mevcut Durum:** ZK mock (BLAKE3 ile simüle) → Gerçek arkworks/Groth16 gerekli

**Rust Crate'leri:**
```toml
[dependencies]
decaf377 = "0.10"           # Penumbra'nın eğrisi
poseidon-paramgen = "0.1"   # Poseidon377 parametreleri
arkworks-r1cs-std = "0.4"   # R1CS constraint sistemi
ark-groth16 = "0.4"         # Proof sistemi
ark-bls12-377 = "0.4"       # BLS12-377 eğrisi
```

### B.2 Asset ID Sistemi

**Kaynak:** `crates/core/asset/src/asset/id.rs`

```rust
pub struct Id(pub Fq);  // BLS12-377 scalar field element (32 byte)

// Asset ID hesaplama:
fn from_raw_denom(base_denom: &str) -> Id {
    let hash = blake2b_simd::Params::new()
        .personal(b"Penumbra_AssetID")
        .hash(base_denom.as_bytes());
    Id(Fq::from_le_bytes_mod_order(hash.as_bytes()))
}

// Value generator (Pedersen commitment için):
fn value_generator(&self) -> decaf377::Element {
    let fq = poseidon377::hash_1(&VALUE_GENERATOR_DOMAIN_SEP, self.0);
    decaf377::Element::encode_to_curve(fq)
}
```

**Neden Fq (taban alanı) kullanılıyor:** Asset ID'nin ZK devrelerinde doğrudan kullanılabilmesi için — arkworks R1CS devreleri Fq üzerinde çalışır.

**Doğrulanmış Gerçek Değerler:**
- UM: `29ea9c2f3371f6a487e7e95c247041f4a356f983eb064e5d2b3bcf322ca96a10`
- USDC: `76b3e4b10681358c123b381f90638476b7789040e47802de879f0fb3eedc8d0b`
- ATOM: `07ef660132a4c3235fab272d43d9b9752a8337b2d108597abffaff5f246d0f0f`

**Nova Shield NSC Asset ID:**
```rust
// nova-zk/src/asset.rs
pub fn nsc_asset_id() -> Fq {
    let hash = blake2b_simd::Params::new()
        .personal(b"NovaShield_Asset")
        .hash(b"unsc");
    Fq::from_le_bytes_mod_order(hash.as_bytes())
}
```

### B.3 Pedersen Value Commitment

**Kaynak:** `crates/core/asset/src/balance/commitment.rs`

```
VALUE_BLINDING_GENERATOR:
  H = decaf377::Element::encode_to_curve(
    Fq::from_le_bytes_mod_order(BLAKE2b("decaf377-rdsa-binding"))
  )

Value::commit(value, blinding: Fr) -> Commitment:
  G_v = asset_id.value_generator()   // asset'e özgü grup noktası
  C = v * G_v + blinding * H         // Pedersen commitment

// Homoforik özellik:
// C1 + C2 = Enc(v1 + v2)  [Add trait]
// C1 - C2 = Enc(v1 - v2)  [Sub trait]
// -C      = Enc(-v)        [Neg trait]

// İşlem geçerliliği:
// sum(input.commit(r_i)) == sum(output.commit(r_j))
// <=> v_in == v_out AND sum(r_i) == sum(r_j)
```

**Nova Shield ZK Shield Uygulaması:**
```rust
// nova-zk/src/commitment.rs
pub const VALUE_BLINDING_GENERATOR_DOMAIN: &[u8] = b"NovaShield_Blnd";

pub fn commit(amount: u128, asset_id: Fq, blinding: Fr) -> decaf377::Element {
    let g_v = poseidon377::hash_1(&domain_sep, asset_id);
    let g_v_point = decaf377::Element::encode_to_curve(g_v);
    let h = compute_blinding_generator();
    g_v_point * Fr::from(amount) + h * blinding
}
```

### B.4 Note Commitment (Poseidon377 Hash)

**Kaynak:** `crates/core/component/shielded_pool/src/note/commitment.rs`

```rust
// Note commitment = Poseidon377 7-giriş hash:
cm = poseidon377::hash_7(
    &DOMAIN_SEP,
    (
        diversified_generator.compress_to_field(),  // g_d sıkıştırılmış
        transmission_key.compress_to_field(),       // T = ivk * g_d sıkıştırılmış
        value_blinding_generator.compress_to_field(),
        Fq::from(amount),
        asset_id.0,                                 // Fq
        rseed_randomness_1,                         // Fq
        rseed_randomness_2,                         // Fq
    )
)
```

**Nova Shield'daki Mevcut Sorun (Bug #17):** ZK Merkle tree iki şema uyumsuz → Poseidon377 ile tek tutarlı şema kullanılmalı.

---

## C. ANAHTAR HİYERARŞİSİ (TAM)

### C.1 Tam Türetme Zinciri

**Kaynak:** `crates/core/keys/src/`

```
SpendKey (32 byte rastgele)
  │
  ├─ ask = PRF("Penumbra_ExpndSd" || seed, 0)  // spend authorization key (Fr)
  ├─ nk  = PRF("Penumbra_ExpndSd" || seed, 1)  // nullifier key (Fq)
  └─ ovk = PRF("Penumbra_DeriOVK" || seed, 0)  // outgoing viewing key (32 byte)

FullViewingKey (FVK):
  ak = ask * G    // spend verification key (decaf377::Element)
  nk              // nullifier key (Fq)
  ovk             // outgoing viewing key

IncomingViewingKey (IVK):
  ivk = poseidon2(IVK_DOMAIN_SEP, nk, ak.compress_to_field())
  MOD_R_QUOTIENT = 4
  IVK_LEN_BYTES  = 64

OutgoingViewingKey (OVK):
  ovk (yukarıdan, 32 byte)

NullifierKey (NK):
  nk  (yukarıdan, Fq)
```

**PRF Sabitleri (kaynak: `crates/core/keys/src/keys/prf.rs`):**
```
"Penumbra_ExpndSd" → ask ve nk türetimi
"Penumbra_DeriOVK" → ovk türetimi
```

**Çeşitlendirilmiş Adresler:**
```rust
// Çeşitlendirici (diversifier) → benzersiz adresler için:
dk = AES128_key_from(spend_key)
d  = AES128(dk, index_bytes_le)    // 16 byte çeşitlendirici

// Çeşitlendirilmiş generator:
g_d = BLAKE2b("Penumbra_Divrsfy", d) → encode_to_curve()

// Transmission key:
pk_d = ivk * g_d  // ECDH ile gizli mesaj gönderimi

// Adres kodlaması (F4Jumble - Zcash ZIP-316):
address_bytes = f4jumble(d[16] || pk_d[32] || ck_d[32])  // toplam 80 byte
bech32m("penumbra1...", address_bytes)
```

**Doğrulanmış Sabitler:**
- `ADDRESS_LEN_BYTES = 80` (16 + 32 + 32)
- `NUM_PBKDF2_ROUNDS = 2048` (BIP39 seed phrase)
- BIP39 salt: `format!("mnemonic{index}")` (üretim kodu)
- `POSITION_METADATA_SIZE_BYTES = 10`
- `POSITION_METADATA_NONCE_SIZE_BYTES = 24`
- `ENCRYPTED_POSITION_METADATA_SIZE_BYTES = 50` (10+24+16)

**Nova Shield Anahtar Hiyerarşisi Uygulaması:**

Nova Shield'daki mevcut `SpendingKey → FullViewingKey → IncomingViewingKey / OutgoingViewingKey` yapısı doğru. Eksikler:
1. Poseidon hash ile IVK türetimi (mevcut kod muhtemelen basit hash kullanıyor)
2. Çeşitlendirilmiş adresler (nova-account'da yok)
3. F4Jumble adres kodlaması
4. AES128 tabanlı çeşitlendirici (diversifier) türetimi

```rust
// nova-zk/src/keys/ivk.rs
pub fn derive_ivk(nk: Fq, ak: decaf377::Element) -> Fr {
    let ak_s = ak.compress_to_field();
    let ivk_fq = poseidon377::hash_2(&IVK_DOMAIN_SEP, (nk, ak_s));
    Fr::from_le_bytes_mod_order(ivk_fq.to_bytes())
}
```

**Güvenlik Notu:** `ck_d` (clue key diversification) adrese dahil → FMD sistemi için zorunlu. Nova Shield ZK Shield'da FMD kullanılacaksa adres formatı 80 byte olmalı.

---

## D. NULLİFİER SİSTEMİ

### D.1 Nullifier Türetme Formülü

**Kaynak:** `crates/core/component/shielded_pool/src/nullifier.rs`

```
Nullifier = PRF_nf(nk, cm, position)

Penumbra implementasyonu:
  nf = poseidon377::hash_3(
    &NULLIFIER_DOMAIN_SEP,
    (nk, cm.0, Fq::from(position.into_u64()))
  )
```

**Neden Poseidon377 (ZK-Uyumlu PRF):**
- Nullifier'ın ZK devresinde türetilebilmesi için hashing fonksiyonu R1CS-friendly olmalı
- BLAKE2b veya SHA256 R1CS içinde binlerce constraint gerektirir → prova süresi artışı
- Poseidon377 sadece ~220 constraint → pratik
- `nk` gizli tutulur, `nf` açık → gizli anahtar ifşası olmadan çift harcama koruması

**Nullifier Boyutu:**
```
NULLIFIER_SIZE = 34 byte (2 prefix + 32 veri)
```

**Çift Harcama Koruması Mantığı:**
```
Note kullanıldığında:
1. nf = PRF(nk, cm, position) hesapla
2. nf_set içinde nf var mı? → HATA (double spend)
3. Yoksa → nf_set'e ekle
```

**Nova Shield Uygulaması:**

Mevcut `nova-zk/src/nullifier.rs`:
```rust
// Mevcut (YanlIŞ): Nullifier: PRF(nullifier_key, commitment)
// Doğru Penumbra formülü:
pub fn derive_nullifier(nk: Fq, cm: Fq, position: u64) -> Fq {
    poseidon377::hash_3(
        &NULLIFIER_DOMAIN_SEP,
        (nk, cm, Fq::from(position))
    )
}
```

**Kritik Eksiklik:** Mevcut nova-zk'da position (TCT pozisyonu) nullifier'a dahil edilmiyor. Penumbra bunu yapıyor çünkü aynı note farklı TCT pozisyonlarına eklenseydi aynı nullifier üretirdi. Position dahil etmek, her kullanımı benzersiz yapar.

---

## E. ZK DEVRELERİ — SATIR SATIR KILAVUZ

### E.1 Groth16 Proof Sistemi

**Doğrulanmış Sabitler (kaynak: proof-params/src/lib.rs:14):**
```rust
pub const GROTH16_PROOF_LENGTH_BYTES: usize = 192;
// G1 noktası A: 48 byte (sıkıştırılmış)
// G2 noktası B: 96 byte (sıkıştırılmış)
// G1 noktası C: 48 byte (sıkıştırılmış)
// Toplam: 192 byte
```

**8 Groth16 Devresi (kaynak: doğrulama-batchC.md):**
```
1. Spend          — note harcama, nullifier türetimi
2. Output         — yeni note oluşturma
3. Swap           — DEX swap işlemi
4. SwapClaim      — swap sonucu talep etme
5. Convert        — token dönüştürme (staking/unstaking)
6. DelegatorVote  — gizli oy kullanma
7. UndelegateClaim — unbonding sonrası talep
8. NullifierDerivation — sadece nullifier türetimi
```

**Nova Shield İçin Gerekli Devreler:**
1. `SpendCircuit` — ZK Shield harcama
2. `OutputCircuit` — ZK Shield çıktı
3. `FreezeCircuit` — hesap dondurma teyiti (opsiyonel)
4. `InheritanceCircuit` — miras geçişi ispatı (opsiyonel)

### E.2 Spend Devresi Yapısı (R1CS)

**Kaynak:** `crates/core/component/shielded_pool/src/spend/proof.rs`

```rust
// Gizli (witness) girdiler:
// - nk: nullifier key (Fq)
// - ask: spend authorization key (Fr)
// - note: {diversifier, pk_d, value, rseed}
// - auth_path: [[Hash; 3]; 24] (TCT merkle yolu)
// - position: u64

// Herkese açık (public) girdiler:
// - anchor: TCT kök hash (Fq)
// - nullifier: nf (Fq)
// - rk: randomized spend verification key (decaf377::Element)
// - value_commitment: Pedersen commitment (decaf377::Element)

// Kısıtlar:
// 1. note.cm ∈ TCT (Merkle membership proof)
// 2. nf = PRF(nk, cm, position)
// 3. rk = randomize(ask, randomizer)
// 4. value_commitment = commit(note.value, blinding_r)
// 5. nk uyumlu FVK ile
```

### E.3 Output Devresi Yapısı

```rust
// Gizli (witness) girdiler:
// - note: yeni note {diversifier, pk_d, value, rseed}
// - blinding_s: commitment randomness (Fr)

// Herkese açık (public) girdiler:
// - note_commitment: cm (Fq)
// - value_commitment: C (decaf377::Element)

// Kısıtlar:
// 1. cm = note_commitment(diversifier, pk_d, value, rseed)
// 2. C = value_commitment(value.amount, value.asset_id, blinding_s)
```

### E.4 arkworks Entegrasyonu

**Mevcut Nova Shield Sorunu:** `nova-zk` crate'i BLAKE3 mock kullanıyor, gerçek ZK yok.

**Uygulama Adımları:**

```rust
// 1. Proving key yükleme (LazyProvingKey pattern):
// nova-zk/src/params.rs
pub static SPEND_PROVING_KEY: Lazy<ProvingKey<Bls12_377>> = Lazy::new(|| {
    let pk_bytes = include_bytes!("../params/spend_pk.bin");
    let (_, pk) = ProvingKey::deserialize_with_mode(pk_bytes, Compress::No, Validate::No).unwrap();
    pk
});

// 2. R1CS constraint sistemi kurulumu:
use ark_r1cs_std::prelude::*;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

pub struct SpendCircuit {
    // Witness
    pub note: Option<Note>,
    pub nk: Option<Fq>,
    pub auth_path: Option<[[Hash; 3]; 24]>,
    pub position: Option<u64>,
    // Public
    pub anchor: Fq,
    pub nullifier: Fq,
    pub value_commitment: decaf377::Element,
}

// 3. Proof üretme:
use ark_groth16::Groth16;
let proof = Groth16::<Bls12_377>::prove(&proving_key, circuit, &mut rng)?;
// proof.serialized_size() == 192 byte

// 4. Proof doğrulama:
let valid = Groth16::<Bls12_377>::verify(&verifying_key, &public_inputs, &proof)?;
```

### E.5 Proof Setup (Güvenilir Kurulum Seremonisi)

**Kaynak:** `crates/crypto/proof-setup/` ve agent-13.md

```
Phase 1: Powers of Tau (devre bağımsız)
  - tauG1, tauG2, alphaTauG1, betaTauG1, betaG2 üretimi
  - Randomness katkıları (multi-party ceremony)
  - Sonuç: CRS (Common Reference String)

Phase 2: Devre-Spesifik (her devre için ayrı)
  - Phase 1 CRS'i alır
  - Devre kısıtlarını dahil eder
  - delta, gamma parametreleri eklenir
  - Sonuç: (proving_key, verifying_key) çifti

LazyProvingKey doğrulama:
  SHA256(pk.serialize_compressed()) → bech32m("groth16pk", ...)
```

**Nova Shield Uyarısı:** Güvenilir kurulum seremonisi olmadan üretilen paramlar "trusted" değil. Mainnet öncesi Penumbra'nın "Summoning Ceremony" benzeri bir multi-party ceremony yapılmalı.

---

## F. MERKLE AĞACI / TCT (TIERED COMMITMENT TREE)

### F.1 TCT Mimarisi

**Kaynak:** `crates/crypto/tct/src/` (doğrulama-batchC ile teyit edilmiş)

```
3 katmanlı 4'lü ağaç (quadtree):

Katman 0 (Block): 65,536 (2^16) commitment per blok
Katman 1 (Epoch): 65,536 blok per epoch
Katman 2 (Global): 65,536 epoch

Toplam kapasite: 65,536³ = 2^48 = 281,474,976,710,656 commitment
```

**Pozisyon Kodlaması (kaynak: `src/index.rs:239`):**
```rust
// u64 pozisyon = (epoch << 32) | (block << 16) | commitment_index
pub fn position_to_u64(epoch: u16, block: u16, commitment: u16) -> u64 {
    ((epoch as u64) << 32) | ((block as u64) << 16) | (commitment as u64)
}
```

**Proof Yapısı (doğrulandı):**
```rust
pub struct Proof {
    pub position: u64,                  // 8 byte
    pub auth_path: [[Hash; 3]; 24],    // 24 seviye × 3 kardeş × 32 byte = 2304 byte
    pub leaf: StateCommitment,          // 32 byte
}
// Toplam: ~2344 byte (devre dışı test: `// static_assertions::assert_eq_size!(Proof, [u8; 2344])`)
```

**Hash Fonksiyonları (kaynak: `src/internal/hash.rs`):**
```rust
// Domain separator:
pub const DOMAIN_SEPARATOR: Lazy<Fq> = Lazy::new(|| {
    Fq::from_le_bytes_mod_order(
        blake2b_simd::blake2b("penumbra.tct").as_bytes()
    )
});

// Yaprak hash (leaf):
Hash::of(commitment: StateCommitment) -> Hash:
    poseidon377::hash_1(&DOMAIN_SEP, commitment.0)

// İç düğüm hash (inner node, height dahil):
Hash::node(height: u8, a: Hash, b: Hash, c: Hash, d: Hash) -> Hash:
    poseidon377::hash_4(&(DOMAIN_SEP + Fq::from(height)), (a.0, b.0, c.0, d.0))
// Height domain separation: farklı yüksekliklerdeki aynı içerik farklı hash verir
```

**Özel Değerler:**
```rust
// Ön hesaplanmış boş ağaç hash'leri (hızlandırma):
PRECOMPUTE_HEIGHTS = 9..=16  // 9. seviyeden 16. seviyeye kadar

// Silinmiş/gizlenmiş düğüm sayacı:
pub struct Forgotten([u8; 6]);  // 48-bit counter (little-endian)
```

**Nova Shield Mevcut Durum:** `nova-zk`'da `MAX_TREE_SIZE = 2^20` (çok küçük) → 2^48'e genişletilmeli.

**Nova Shield TCT Uygulaması:**

```rust
// nova-zk/src/tct.rs
// Penumbra'nın tct crate'ini doğrudan kullan:
// [dependencies]
// penumbra-tct = "0.79"
// VEYA yeniden implement et:

pub const TCT_BLOCK_CAPACITY: u64 = 65_536;     // 2^16
pub const TCT_EPOCH_CAPACITY: u64 = 65_536;     // 2^16
pub const TCT_TOTAL_CAPACITY: u128 = 1 << 48;   // 2^48

// Position format: (epoch: u16, block: u16, index: u16)
pub struct Position(pub u64);

impl Position {
    pub fn epoch(&self) -> u16 { ((self.0 >> 32) & 0xFFFF) as u16 }
    pub fn block(&self) -> u16 { ((self.0 >> 16) & 0xFFFF) as u16 }
    pub fn index(&self) -> u16 { (self.0 & 0xFFFF) as u16 }
}
```

### F.2 Nova Shield Epoch/Block Uyumu

Nova Shield'da:
- 1 Epoch = 216,000 slot = 24 saat
- 1 blok = 1 slot = 400ms

TCT'de 65,536 blok per epoch ↔ Nova Shield 216,000 slot per epoch. Çakışma var!

**Çözüm:** TCT'de "blok" konseptini Nova Shield epoch'una bağla. Her Nova Shield epoch için tek bir TCT "block" kullan (216,000 slot = 1 TCT blok). Bu durumda:
- 65,536 TCT epoch = 65,536 Nova epoch = ~179 yıl (yeterli)
- Her epoch'ta 65,536 commitment (note) → epoch başına max 65,536 gizli tx

---

## G. FMD — BULANIK MESAJ TESPİT

### G.1 FMD Protokolü (Doğrulanmış)

**Kaynak:** `crates/crypto/decaf377-fmd/src/`
**Doğrulandı:** doğrulama-batchA.md ve batchB.md

```rust
// Kesin sabitler:
pub const MAX_PRECISION: u8 = 24;  // (kaynak: precision.rs:6) — 0..=24 aralığı

// Clue yapısı:
pub struct Clue([u8; 68]);
// Clue layout:
// [0..32]  P: compressed decaf377::Element (yayın key noktası)
// [32..64] y: compressed decaf377::Element (tek kullanımlık nokta)
// [64]     precision_bits: u8 (0..=24)
// [65..68] ctxts: [u8; 3] (3 şifrelenmiş bit)
// (Not: MAX_PRECISION=24 için 3 byte yeterli: ceil(24/8) = 3)
```

**Yanlış Pozitif Oranı:**
```
false_positive_rate = 2^(-precision_bits)
precision=0  → %100 (tüm mesajlar "pozitif")
precision=4  → %6.25 (1/16) — Penumbra default
precision=24 → %0.000006 (1/16M)
```

**Clue Oluşturma Algoritması (7 adım):**
```rust
// 1. Klue anahtarı türetme (HKD):
// her index i için:
//   x_i = BLAKE2b("decaf377-fmd.hkd", root_pub_encoded || [i as u8]) mod Fr
//   X_i = root_pub + x_i * G  (bu şekilde FVK'dan türetilir)

// 2. Rastgele skalar:
//   r = BLAKE2b("decaf377-fmd.rdv", root_pub_encoded || rseed) mod Fr
//   z = BLAKE2b("decaf377-fmd.zdv", ...) mod Fr

// 3. Noktalar:
//   P = r * G + z * root_pub  (commitment nokta)
//   y = z * G                 (yardımcı nokta)

// 4. Her bit i için:
//   k_i = P + r * X_i
//   bit_i = to_bit(k_i) = BLAKE2b("decaf377-fmd.bit", k_i) & 1

// 5. Şifreli bit oluştur:
//   m_i = message_bit XOR bit_i
//   ctxts[i/8] |= (m_i << (i%8))
// (precision_bits kadar bit işlenir)

// 6. Clue = P || y || precision_bits || ctxts
```

**Tespit Algoritması (DetectionKey):**
```rust
pub struct DetectionKey {
    dtk: Fr,      // detection key scalar
    xs: [Fr; 24], // türetilmiş skalerler (max 24 adet)
}

// examine(clue) → bool:
// 1. P == identity? → false (güvenlik kontrolü)
// 2. y == 0? → false (güvenlik kontrolü)
// 3. Her bit i (0..precision_bits):
//    k_i = dtk * (P + y * X_i)
//    (ya da eşdeğer: k_i = dtk * (P + y * (root + x_i*G)))
//    beklenen_bit = to_bit(k_i)
//    actual_bit = (ctxts[i/8] >> (i%8)) & 1
//    beklenen_bit != actual_bit → false  (kısa devre)
// 4. Tüm bitler eşleşti → true (alıcı bu mesajı aldı / yanlış pozitif)
```

### G.2 Nova Shield FMD Uygulaması

Nova Shield'daki mevcut FMD implementasyonu: `precision 1..=24` tanımlanmış (YANLIŞ — gerçek `0..=24`).

```rust
// nova-zk/src/fmd.rs
pub const MAX_PRECISION: u8 = 24;
pub const DEFAULT_PRECISION: u8 = 4;  // ~6.25% yanlış pozitif (Penumbra default'u)

pub struct ClueKey {
    root_pub: decaf377::Element,
    // derived keys per address index
}

pub struct Clue(pub [u8; 68]);

pub struct DetectionKey {
    dtk: Fr,
    derived_keys: Vec<decaf377::Element>,
}
```

**Kullanım Senaryosu (Nova Shield Guardian + ZK Shield):**
- Guardian, kullanıcı adresinin clue'larını tarar
- Kullanıcı gizliliğini korurken guardian'lar hangi işlemlerin kullanıcıya ait olduğunu anlayabilir
- Precision=4 → guardian gelen işlemlerin %6.25'ini "false positive" olarak inceleme yapar

---

## H. SHIELDED DEX AMM

### H.1 Sabit Toplam CFMM (φ Fonksiyonu)

**Kaynak:** `crates/core/component/dex/src/lp/trading_function.rs`
**Doğrulandı:** doğrulama-batchB.md:204

```
φ(R) = p * R₁ + q * R₂    (Constant-Sum CFMM)

R₁ = rezerv 1 (asset 1 miktarı)
R₂ = rezerv 2 (asset 2 miktarı)
p  = asset 1 fiyat katsayısı (u64)
q  = asset 2 fiyat katsayısı (u64)
```

**Gamma (Ücret Faktörü):**
```rust
// kaynak: trading_function.rs:444-445 (doğrulandı)
pub fn gamma(&self) -> U128x128 {
    (U128x128::from(10_000 - self.fee) / U128x128::from(10_000u64))
        .expect("10_000 != 0")
}
// fee: u64, bps cinsinden (örn. fee=30 → %0.3)

// Etkin fiyat (2→1 yönü):
// effective_price     = (q/p) / gamma
// effective_price_inv = (p/q) * gamma
```

**Doldurma Algoritması (fill):**
```rust
// 2→1 doldurma (asset_2 → asset_1):
// max_output_1 = (input_2 * q * gamma) / p
// output_1 = min(max_output_1, R₁)
// actual_input_2 = output_1 * p / (q * gamma)  // gerçek kullanılan giriş

// 1→2 doldurma (asset_1 → asset_2):
// max_output_2 = (input_1 * p * gamma) / q
// output_2 = min(max_output_2, R₂)
// actual_input_1 = output_2 * q / (p * gamma)
```

**Önemli Limitler:**
```rust
pub const MAX_RESERVE_AMOUNT: u128 = (1 << 80) - 1;  // 80-bit
pub const MAX_FEE_BPS: u64 = 5000;                    // %50 maksimum ücret
```

**Routing Parametreleri (doğrulandı):**
```
max_hops                    = 4     (kaynak: dex/src/params.rs:72)
max_positions_per_pair      = 1_000 (params.rs:73)
max_execution_budget        = 64    (params.rs:74)
DYNAMIC_ASSET_LIMIT         = 10    (position_manager.rs:37)
RECENTLY_ACCESSED_ASSET_LIMIT = 10 (dex.rs:317)
```

**Arb Flash Loan (kaynak: arb.rs:43):**
```rust
let flash_loan = Value {
    asset_id: arb_token,
    amount: u64::MAX.into(),  // sonsuz flash loan → kârlı yollar çözülür
};
// price_limit = Some(1u64.into())  → yalnızca kârlı arb
```

**Bilinen Bug: SPREAD RPC (kaynak: rpc.rs:398):**
```rust
// YANLIŞ (mevcut kod):
let best_2_to_1_position = state.best_position(&pair12);  // pair21 olmalı!
// Bu, 2→1 yönündeki en iyi pozisyonu yanlış yönde arar.
```

### H.2 Dutch Auction (Penumbra)

**Kaynak:** `crates/core/component/auction/`
**Doğrulandı:** doğrulama-batchB.md

```rust
pub const MAX_AUCTION_AMOUNT_RESERVES: u128 = (1 << 52) - 1;  // 52-bit
// Neden 52-bit: fiyat interpolasyonu sırasında taşma olmadan çarpma yapılabilmesi için

// Step count kısıtı:
// step_count >= 2   (minimum 2 adım)
// step_count <= 255 (maksimum 255 adım)
// Geçerli aralık: 2..=255

// NFT sequence (durum takibi):
// 0 = opened
// 1 = closed
// 2+ = withdrawn

// AuctionId (BLAKE2b hash):
AuctionId = BLAKE2b("penumbra_DA_nft",
    nonce || input.asset_id || input.amount ||
    max_output || start_height || end_height || step_count
)
// NOT: min_output dahil DEĞİL (potansiyel bug!)
```

**Nova Shield DEX AMM Uygulaması:**

Nova Shield'da CoW-style batch auction mevcut (ancak CoW feature eksik). Penumbra'nın AMM'sini şu şekilde entegre edebilir:

```rust
// nova-auction/src/amm.rs
pub struct LinearPricingFunction {
    pub p: u64,   // asset 1 katsayısı
    pub q: u64,   // asset 2 katsayısı
    pub fee: u64, // bps (max 5000)
}

impl LinearPricingFunction {
    pub fn gamma(&self) -> u128 {
        (10_000 - self.fee as u128) * u128::MAX / 10_000
    }

    pub fn fill_2_to_1(&self, input: u128, reserve_1: u128) -> (u128, u128) {
        let max_out = input * self.q as u128 / self.p as u128 * self.gamma() / u128::MAX;
        let out = max_out.min(reserve_1);
        let actual_in = out * self.p as u128 / (self.q as u128 * self.gamma() / u128::MAX);
        (out, actual_in)
    }
}

// XYK portfolio değer fonksiyonu (pcli/xyk.rs'den, doğrulandı):
pub fn portfolio_value(invariant_k: f64, price: f64) -> f64 {
    2.0 * f64::sqrt(invariant_k * price)
}
```

---

## I. IBC VE CROSS-CHAIN GİZLİLİK

### I.1 IBC JMT Proof Spec

**Kaynak:** `crates/core/component/ibc/src/prefix.rs` (doğrulandı batchC)

```rust
// Kritik sabitler:
pub const IBC_SUBSTORE_PREFIX: &str = "ibc-data";
pub const IBC_COMMITMENT_PREFIX: &[u8] = b"ibc-data";

// JMT ICS-23 Leaf hash:
pub const LEAF_DOMAIN_SEPARATOR: &[u8] = b"JMT::LeafNode";

// JMT ICS-23 İç node hash:
// KASITLI yazım hatası → ASLA DÜZELTME:
pub const INTERNAL_DOMAIN_SEPARATOR: &[u8] = b"JMT::IntrnalNode";
// (IntrnalNode, InternalNode değil — tarihsel uyumluluk için değiştirilemez)

// Sparse Merkle placeholder:
pub const SPARSE_MERKLE_PLACEHOLDER_HASH: &[u8] = b"SPARSE_MERKLE_PLACEHOLDER_HASH__";
// (tam 32 byte — 2 tane alt çizgi sonda)

// ICS-23 Proof Spec (JMT için):
LeafOp: hash=SHA256, prehash_key=SHA256, prehash_value=SHA256, length=NoPrefix
InnerOp: hash=SHA256, child_order=[0,1], child_size=32
min_prefix_length = max_prefix_length = 16  // len("JMT::IntrnalNode")
min_depth=0, max_depth=64
prehash_key_before_comparison=true

// IBC_PROOF_SPECS: [spec, spec]  // iki kez aynı spec → iki katmanlı Merkle proof
```

**IBC Parametreleri:**
```rust
pub struct IBCParameters {
    pub ibc_enabled: bool,     // default: true
    pub inbound_ics20_transfers_enabled: bool,  // default: true
    pub outbound_ics20_transfers_enabled: bool, // default: true
}

// max_time_per_block (hardcoded, kaynak: proof_verification.rs:466):
let max_time_per_block = std::time::Duration::from_secs(20);

// IBC client ID format:
format!("07-tendermint-{}", client_idx)
```

### I.2 Noble Forwarding Adresi

**Kaynak:** `crates/core/component/ibc/src/component/` (agent-02.md'den)

```rust
// Noble forwarding address hesabı:
// SHA256(SHA256("forwarding") || channel_id_bytes || recipient_bytes)
// Son 20 byte = Cosmos address
let preimage = sha256(b"forwarding");
let hash = sha256(&[preimage, channel_bytes, recipient_bytes].concat());
let noble_addr = &hash[12..32];  // son 20 byte

// Cosmos SDK raw tx:
CosmosTx {
    messages: [MsgRegisterAccount { signer, recipient, channel }],
    fee: { gas_limit: 200_000 },
    memo: "",
    timeout_height: 0,
}
// broadcast mode = 2 (sync)
```

### I.3 Gizli IBC Transferi (Shielded Transfer)

Penumbra'da IBC transferleri iki modda çalışır:

1. **Transparent (Şeffaf):** Normal IBC ICS-20 → varlık miktarı görünür
2. **Shielded Forwarding:** Shielded pool → IBC → hedef zincir akışı
   - Note açılır → ICS-20 transfer oluşturulur → Noble/Cosmos'a gönderilir
   - Geri dönüşte: ICS-20 → shielded pool'a eklenir

**Nova Shield Cross-Chain Guard ile Entegrasyon:**
- Mevcut nova-cross-chain'deki `%0.05 (5 bps)` koruma ücreti devam edebilir
- IBC varlık ID hesabı: `BLAKE2b("NovaShield_IBC" || denom || channel_id)` → Fq
- IBC vault adresine gelecek transferler → shielded pool'a ekle (gizlilik koruması)

---

## J. ACTION SİSTEMİ (CHECK/EXECUTE PIPELINE)

### J.1 AppActionHandler Trait

**Kaynak:** `crates/core/app/src/action_handler.rs:26-32`
**Doğrulandı:** doğrulama-batchA.md ve batchB.md

```rust
pub trait AppActionHandler {
    // Durumsuz kontroller (paralel yapılabilir):
    fn check_stateless(&self, context: Arc<Transaction>) -> impl Future<Output = Result<()>>;

    // Tarihsel durum kontrolleri (default: Ok(())):
    fn check_historical(&self, state: Arc<S>) -> impl Future<Output = Result<()>> {
        async move { Ok(()) }  // varsayılan implementasyon
    }

    // Durum değişikliği + yürütme (tek seferde):
    fn check_and_execute(&self, state: &mut StateDelta<S>) -> impl Future<Output = Result<()>>;
}

// ÖNEMLI: check_stateful ve execute metotları YOKTUR
// Yanlış isimler kaynak kodda bulunmaz
```

**İşlem Pipeline'ı:**
```
TX geldi
  │
  ├─ [Paralel] check_stateless()  → imza formatı, ZK kanıt syntax, boyut limitleri
  │
  ├─ [Sıralı]  check_historical() → belirli blok yüksekliğindeki durum sorgulaması
  │
  └─ [Sıralı]  check_and_execute() → durum değişikliği + yan etki (nullifier ekle, balance güncelle)
```

**Transaction Boyut Limitleri (doğrulandı):**
```rust
// app/mod.rs:56,59,62
MAX_BLOCK_TXS_PAYLOAD_BYTES = 1_048_576  // 1 MB
MAX_TRANSACTION_SIZE_BYTES  = 98_304     // 96 KB
MAX_EVIDENCE_SIZE_BYTES     = 30_720     // 30 KB

// prepare_proposal akışı:
// tx.len() > MAX_TRANSACTION_SIZE_BYTES → atla
// total > max_proposal_size_bytes → dur
// deliver_tx_bytes(tx).is_err() → atla
```

**Paralel TX Doğrulama:**
```rust
// Stateless kontroller JoinSet ile paralel:
use tokio::task::JoinSet;
let mut set = JoinSet::new();
for tx in txs {
    set.spawn(async move { tx.check_stateless(ctx).await });
}
while let Some(result) = set.join_next().await {
    result??;
}
```

### J.2 Nova Shield Action Sistemi Uygulaması

Mevcut Nova Shield'da `TX imzası hiç doğrulanmıyor (Bug #4)` ve `TX hash içerik-bağımsız (Bug #5)`. Penumbra'nın pipeline'ı bu sorunları çözmek için model:

```rust
// nova-runtime/src/action/mod.rs
pub trait ActionHandler {
    /// Durumsuz kontroller: imza, format, boyut
    fn check_stateless(&self) -> Result<(), ActionError>;

    /// Tarihsel kontroller: double-spend, nonce
    fn check_historical(&self, state: &dyn StateReader) -> Result<(), ActionError>;

    /// Yürütme: durum değişikliği
    fn check_and_execute(&self, state: &mut StateDelta) -> Result<(), ActionError>;
}

// ZK Shield işlemi için:
pub struct SpendAction {
    pub proof: Groth16Proof,        // 192 byte
    pub nullifier: Nullifier,        // 34 byte
    pub rk: decaf377::Element,      // 32 byte
    pub value_commitment: decaf377::Element,
    pub anchor: Hash,               // TCT kök
}

impl ActionHandler for SpendAction {
    fn check_stateless(&self) -> Result<(), ActionError> {
        // 1. ZK kanıt syntax kontrolü (192 byte?)
        // 2. Proof doğrulama (arkworks Groth16::verify)
        verify_spend_proof(&self.proof, &self.nullifier, &self.rk, &self.value_commitment, &self.anchor)
    }

    fn check_historical(&self, state: &dyn StateReader) -> Result<(), ActionError> {
        // Double-spend kontrolü:
        if state.nullifier_spent(&self.nullifier)? {
            return Err(ActionError::DoubleSpend);
        }
        // TCT anchor kontrolü:
        if !state.valid_anchor(&self.anchor)? {
            return Err(ActionError::InvalidAnchor);
        }
        Ok(())
    }

    fn check_and_execute(&self, state: &mut StateDelta) -> Result<(), ActionError> {
        // Nullifier'ı harcandı olarak işaretle:
        state.mark_nullifier_spent(&self.nullifier)?;
        Ok(())
    }
}
```

**Governance Parametreleri (kaynak: doğrulama-batchB.md):**
```
PROPOSAL_TITLE_LIMIT       = 80        (action_handler/actions/submit.rs:39)
PROPOSAL_DESCRIPTION_LIMIT = 10_000   (submit.rs:43)
```

---

## K. KANIT SİSTEMİ (GROTH16 + PLONK)

### K.1 Groth16 Yapısı ve Boyutlar

**Doğrulanmış Değerler:**
```
GROTH16_PROOF_LENGTH_BYTES = 192  (proof-params/src/lib.rs:14)
ZKPROOF_SIZE               = 192  (transaction/src/gas.rs)
NULLIFIER_SIZE             = 34   (2 prefix + 32 data)
NOTEPAYLOAD_SIZE           = 240  (32 note_commitment + 32 ephemeral_key + 176 ciphertext)
SWAPPAYLOAD_SIZE           = 304  (32 commitment + 272 ciphertext)
```

**Gas Maliyetleri (kaynak: fee/src/gas.rs):**
```rust
// ZK kanıt doğrulama: 1000 gas unit (en pahalı işlem)
// Action yürütme: 10 gas unit
// Örnek spend tx:
// - 1 × spend: 1000 (zk) + 10 (exec) = 1010 gas
// - 1 × output: 1000 (zk) + 10 (exec) = 1010 gas
// - Toplam: 2020 verification gas + 20 execution gas
```

**Proving Key Yükleme (LazyProvingKey Pattern):**
```rust
// Kaynak: proof-setup crate
use std::sync::LazyLock;

pub static SPEND_PROVING_KEY: LazyLock<ProvingKey<Bls12_377>> =
    LazyLock::new(|| {
        let bytes = include_bytes!("../params/spend_pk.bin");
        // Hash doğrulaması:
        let hash = sha256(bytes);
        let expected = bech32m_decode("groth16pk", SPEND_PK_HASH);
        assert_eq!(hash, expected, "proving key tampered!");
        ProvingKey::deserialize_uncompressed(bytes).expect("valid proving key")
    });
```

### K.2 PLONK Alternatifi

Nova Shield'da `nova-zk` mevcut mimarisi `ZK kanıt sistemi: Groth16 + Plonk (henüz mock, arkworks entegrasyonu gerekli)` diyor. Karşılaştırma:

| Özellik | Groth16 | PLONK |
|---------|---------|-------|
| Kanıt boyutu | 192 byte (sabit) | ~500+ byte |
| Doğrulama hızı | Çok hızlı | Orta |
| Güvenilir kurulum | Devre-spesifik (Phase 2 gerekli) | Evrensel SRS |
| Kanıt üretim hızı | Orta | Yavaş |
| Önerilen kullanım | Üretim | Prototip |

**Öneri:** Nova Shield için başlangıçta Groth16 kullan (Penumbra ile aynı seçim). Uzun vadede recursive PLONK/Nova (SNARKs for recursive proof aggregation) değerlendir.

### K.3 Kanıt Toplayıcı (Aggregation)

Penumbra 7/8 bağımsız devre için ayrı proof üretiyor. Nova Shield da aynı yaklaşımı uygulayabilir:
- Her action → kendi proof'u
- TX'te birden fazla action varsa → birden fazla proof (192 byte × N)
- Gelecekte: Proof aggregation (tüm action'lar tek proof) → boyut azaltma

---

## L. NOVA ZK SHIELD TAM UYGULAMA PLANI

### L.1 Mevcut Durum Analizi

```
nova-zk mevcut eksiklikler:
- ZK kanıt: BLAKE3 mock (gerçek ZK yok)
- Note commitment tree: MAX_TREE_SIZE = 2^20 (yetersiz, 2^48 olmalı)
- ZK kanıt sistemi: Groth16 + Plonk (henüz mock)
- FMD precision: 1..=24 (yanlış, 0..=24 olmalı)
- Note commitment: iki uyumsuz şema (Bug #17)
- Nullifier: position dahil değil (eksik)
- IVK türetimi: Poseidon hash yok
```

### L.2 Uygulama Aşamaları (Sıralı)

**Aşama 1: Kriptografik Temel (2-3 hafta)**

```toml
# nova-zk/Cargo.toml
[dependencies]
decaf377 = "0.10.0"
poseidon377 = "0.3.0"
ark-bls12-377 = "0.4"
ark-groth16 = "0.4"
ark-r1cs-std = "0.4"
ark-relations = "0.4"
blake2b_simd = "1.0"
```

1. Asset ID sistemi: `BLAKE2b("NovaShield_Asset", denom) → Fq`
2. Pedersen commitment: `v * G_v + r * H`
3. Poseidon377 hash entegrasyonu (note commitment için)
4. TCT'yi 2^48 kapasiteye genişlet
5. FMD precision düzelt: `0..=24`

**Aşama 2: Anahtar Hiyerarşisi (1-2 hafta)**

```
SpendKey → (ask, nk) → FVK(ak, nk, ovk) → IVK(ivk) → Adres(diversifier, pk_d, ck_d)
```

1. PRF türetmelerini Penumbra sabit string'leriyle hizala
2. IVK türetimini Poseidon377 ile yeniden yaz
3. Çeşitlendirilmiş adres desteği ekle (AES128 diversifier)
4. F4Jumble adres kodlaması (80 byte)

**Aşama 3: R1CS Devreleri (4-6 hafta)**

Öncelik sırası:
1. `NullifierDerivationCircuit` (en basit, sadece PRF)
2. `OutputCircuit` (note commitment oluşturma)
3. `SpendCircuit` (Merkle membership + nullifier + commitment)
4. Sonraki sürümde: `SwapCircuit`, `ConvertCircuit`

```rust
// Spend devresi implementasyon şablonu:
pub struct SpendCircuit {
    // --- witness ---
    pub note: Note,
    pub nk: Fq,
    pub auth_path: [[Hash; 3]; 24],
    pub position: u64,
    pub value_blinding: Fr,
    // --- public ---
    pub anchor: Fq,
    pub nullifier: Fq,
    pub value_commitment: decaf377::Element,
    pub rk: decaf377::Element,
}

impl ConstraintSynthesizer<Fq> for SpendCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fq>) -> Result<(), SynthesisError> {
        // 1. Note commitment hesapla ve doğrula
        let cm = note_commitment_gadget(&cs, &self.note)?;
        // 2. Merkle membership ispatı
        merkle_auth_path_gadget(&cs, cm, &self.auth_path, self.position, self.anchor)?;
        // 3. Nullifier türet
        let nf = nullifier_gadget(&cs, self.nk, cm, self.position)?;
        enforce_equal(&cs, nf, self.nullifier)?;
        // 4. Value commitment
        let vc = value_commitment_gadget(&cs, &self.note.value, self.value_blinding)?;
        enforce_equal(&cs, vc, self.value_commitment)?;
        Ok(())
    }
}
```

**Aşama 4: Güvenilir Kurulum (2-4 hafta)**

```
Powers of Tau Seremonisi:
1. Phase 1: devre bağımsız (~2^20 kısıt için yeterli CRS)
   - Birden fazla katılımcı (minimum 3 öneri: ekip + topluluk + dış denetçi)
   - Her katılımcı rastgelelik katkısı yapar

2. Phase 2: her devre için ayrı
   - SpendCircuit parametreleri
   - OutputCircuit parametreleri
   - ...

3. Parametre doğrulama:
   SHA256(pk.serialize()) → bech32m("novapk1...", hash) formunda yayınla
```

**Aşama 5: Entegrasyon (2-3 hafta)**

1. `nova-zk` → `nova-runtime` entegrasyonu
2. Action pipeline'ını güncelle: `check_stateless` içinde ZK kanıt doğrulama
3. TCT'yi `nova-account` ile bağla: her note oluşturulduğunda TCT'ye ekle
4. Nullifier set'ini state'e dahil et: double-spend koruması
5. Gas hesaplamasına ZK kanıt maliyeti ekle: `1000 gas per proof`

**Aşama 6: Shielded DEX Entegrasyonu (isteğe bağlı)**

Nova Shield'daki mevcut DEX (Batch Auction) ZK Shield ile entegre edilebilir:
- IBE şifreli emirler → Penumbra'nın eddy flow encryption'ı model alınabilir
- ElGamal homoforik şifreleme: `c1 = r*G, c2 = r*PK + v*G`
- Threshold çözme: DEX batch kapandıktan sonra validatörler decrypt eder

### L.3 Kritik Güvenlik Notları

1. **Proving Key Sızdırma Riski:** Proving key public olabilir (Groth16'da güvenli), ama toxic waste (Phase 2 randomness) imha edilmeli.

2. **Replay Attack:** Nullifier set temizlenirse eski notlar tekrar harcanabilir. TCT kök hash'i checkpoint'lerde sakla.

3. **Anchor Validasyonu:** Çok eski anchor'lara izin verme → TCT'nin son N epoch'unu geçerli anchor olarak tut.

4. **Note Şifreleme:** Notlar alıcının `pk_d` anahtarıyla şifrelenmeli (ChaCha20-Poly1305):
   ```
   NOTEPAYLOAD_SIZE = 240 byte
   = 32 (note_commitment)
   + 32 (ephemeral_key = r*G)
   + 176 (ChaCha20-Poly1305 ciphertext: 160 plaintext + 16 AEAD tag)
   ```

5. **FMD Parametre Yönetimi:** Epoch başında precision parametresini güncelle → kullanıcılar yeni precision'a göre clue üretir. Geçiş döneminde eski ve yeni precision'lar aynı anda desteklenmeli.

6. **Domain Separation:** Her hash fonksiyonu benzersiz domain separator kullanmalı:
   ```
   "NovaShield_Note"  → note commitment
   "NovaShield_Nf"    → nullifier
   "NovaShield_Asset" → asset ID
   "NovaShield_Blnd"  → value blinding generator
   "NovaShield_IVK"   → incoming viewing key
   "NovaShield_TCT"   → TCT hash
   ```

7. **`vartime` İşlemler:** Decaf377 `vartime_compress/decompress` timing yan kanalı yaratır — bu noktalar halka açık bilgi olduğunda kullanım güvenlidir (private key operasyonlarında kullanma).

### L.4 Mevcut Bug Çözüm Haritası

| Bug No | Sorun | Penumbra Çözümü |
|--------|-------|-----------------|
| #4 | TX imzası doğrulanmıyor | `check_stateless`'a imza doğrulama ekle |
| #5 | TX hash içerik-bağımsız | EffectHash = `BLAKE2b("PenumbraEffects" || tüm alanlar)` |
| #12 | EVM function_selector BLAKE3 | Keccak256 kullan (EVM uyumluluk için) |
| #17 | ZK Merkle tree iki şema uyumsuz | Tek Poseidon377 tabanlı şema benimse |
| #18 | Miras distribute hesabı yanlış | Penumbra'nın bakiye toplama mantığını incele |
| #19 | eon_key uyumsuzluğu | Tek tutarlı IBE şifreleme seç |

---

## ÖZET: KRİTİK DEĞERLER TABLOSU

| Parametre | Değer | Kaynak (Penumbra) |
|-----------|-------|-------------------|
| GROTH16_PROOF_LENGTH_BYTES | 192 | proof-params/src/lib.rs:14 |
| MAX_BLOCK_TXS_PAYLOAD_BYTES | 1_048_576 | app/src/app/mod.rs:56 |
| MAX_TRANSACTION_SIZE_BYTES | 98_304 | app/src/app/mod.rs:59 |
| MAX_EVIDENCE_SIZE_BYTES | 30_720 | app/src/app/mod.rs:62 |
| TCT toplam kapasite | 2^48 | tct/src/tree.rs:681 |
| TCT per-blok kapasite | 65_536 | (2^16) |
| TCT proof boyutu | ~2_344 byte | [[Hash;3];24] |
| FMD MAX_PRECISION | 24 | decaf377-fmd/src/precision.rs:6 |
| FMD Clue boyutu | 68 byte | clue.rs Clue([u8;68]) |
| Adres uzunluğu | 80 byte | core/keys/src/address.rs |
| ZK proof gas maliyeti | 1_000 | fee/src/gas.rs |
| Action execution gas | 10 | fee/src/gas.rs |
| MAX_RESERVE_AMOUNT | (1<<80)-1 | dex/src/lp/ |
| MAX_FEE_BPS (DEX) | 5_000 | dex params |
| max_hops (routing) | 4 | dex/src/params.rs:72 |
| max_positions_per_pair | 1_000 | dex/src/params.rs:73 |
| max_execution_budget | 64 | dex/src/params.rs:74 |
| MAX_AUCTION_AMOUNT | (1<<52)-1 | auction/schedule.rs:10 |
| Dutch auction step_count | 2..=255 | schedule.rs:85-93 |
| BPS² scale factor | 1_0000_0000 | staking/rate.rs |
| Flash loan miktarı | u64::MAX | arb.rs:43 |
| Fee tiers | 105%/130%/200% | fee/src/fee.rs |
| NULLIFIER_SIZE | 34 byte | shielded_pool gas |
| NOTEPAYLOAD_SIZE | 240 byte | shielded_pool gas |
| IBC max_time_per_block | 20s | proof_verification.rs:466 |
| IBC_SUBSTORE_PREFIX | "ibc-data" | ibc/src/prefix.rs:5 |
| JMT internal separator | "JMT::IntrnalNode" | prefix.rs:20 (YAZIM HATASI, DEĞİŞTİRME) |
| PROPOSAL_TITLE_LIMIT | 80 | submit.rs:39 |
| PROPOSAL_DESCRIPTION_LIMIT | 10_000 | submit.rs:43 |
| GROTH16 devre sayısı | 8 | proof-params (teyit batchC) |
| BIP39 salt | "mnemonic{index}" | keys/src/keys/spend.rs:95 |
| NUM_PBKDF2_ROUNDS | 2_048 | seed_phrase.rs:9 |
| IVK_LEN_BYTES | 64 | ivk.rs:18 |
| MOD_R_QUOTIENT | 4 | ivk.rs:19 |
| block_time_s (mainnet) | 5.1 saniye | pindexer/src/lib.rs:36 |

---

## DOĞRULAMA NOTALARI

Bu belgedeki tüm teknik değerler 3 doğrulama geçişinden geçirilmiştir:
- `DOGRULAMA-batchA.md`: Agent 01-10 değerleri (agent-01..10)
- `DOGRULAMA-batchB.md`: Agent 11-15 + DEX + Auction değerleri
- `DOGRULAMA-batchC.md`: Geri kalan tüm dosyalar + Master Sentez doğrulaması

**Düzeltilen Kritik Hatalar (önceki analizlerden):**
1. FMD precision: `1..=15` → `0..=24` (gerçek)
2. TCT kapasite: `2^72` → `2^48` (gerçek)
3. AppActionHandler metodları: `check_stateful/execute` → `check_historical/check_and_execute` (gerçek)
4. Groth16 devre sayısı: `7` → `8` (NullifierDerivation dahil)
5. MINIFRONT embed path: göreli → `include_bytes!("../../../../assets/minifront.zip")`
6. Migration enum: Testnet73 ve Testnet75 VAR sanılırdı → YOKTUR
