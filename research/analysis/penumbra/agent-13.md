# Penumbra Agent-13: Kriptografi — decaf377-frost, decaf377-ka, proof-params, proof-setup, tct

**Kapsam:**
- `crates/crypto/decaf377-frost/` — FROST esik imzalama (5 dosya)
- `crates/crypto/decaf377-ka/` — Anahtar anlasma protokolu (1 dosya)
- `crates/crypto/proof-params/` — Groth16 ispat parametreleri (10 dosya)
- `crates/crypto/proof-setup/` — Guvenilir kurulum seremonisi (10 dosya)
- `crates/crypto/tct/` — Tiered Commitment Tree (30+ dosya)

**Toplam:** ~55 dosya, ~3500 satir
**Tarih:** 2026-07-22

---

## 1. decaf377-frost — FROST Esik Imzalama

### 1.1 Amac

FROST (Flexible Round-Optimized Schnorr Threshold Signatures), t-of-n esik Schnorr imzalama protokoludu. Bu crate, FROST'u `decaf377-rdsa` `SpendAuth` imzalari icin uyarlar.

**Sadece SpendAuth destekleniyor** — `decaf377-rdsa` imzalari, konvansiyonel decaf377 basepoint kullanir.

### 1.2 Ciphersuite Tanimi

**Dosya:** `traits.rs`

```rust
pub struct Decaf377Rdsa;

impl Ciphersuite for Decaf377Rdsa {
    const ID: &'static str = "decaf377-rdsa";
    type Group = Decaf377Group;       // decaf377::Element
    type HashOutput = [u8; 32];
    type SignatureSerialization = Vec<u8>;

    // Hash fonksiyonlari (BLAKE2b-64 bit, kisisellestirilmis):
    H1(m) = BLAKE2b("FROST-decaf377", b"rho" || m) mod Fr   // binding factor
    H2(m) = BLAKE2b("decaf377-rdsa---", m) mod Fr            // challenge (rdsa ile esit!)
    H3(m) = BLAKE2b("FROST-decaf377", b"nonce" || m) mod Fr  // nonce
    H4(m) = BLAKE2b("FROST-decaf377", b"msg" || m)[0..32]    // mesaj ozeti
    H5(m) = BLAKE2b("FROST-decaf377", b"com" || m)[0..32]    // commitment ozeti
    HDKG(m) = BLAKE2b("FROST-decaf377", b"dkg" || m) mod Fr  // DKG hash
    HID(m) = BLAKE2b("FROST-decaf377", b"id" || m) mod Fr    // katilimci ID
}
```

**Onemli:** `H2` (challenge), `decaf377-rdsa---` kisisel etiketini kullanir — bu, FROST imzalarinin normal rdsa dogrulama ile uyumlu olmasini saglar.

**Grup ozellikleri:**
- `cofactor() = 1` — ristretto/decaf gruplarinin cofactor-1 ozelligi
- `generator() = decaf377::Element::GENERATOR`
- `identity() = Element::default()` (noktron, sifir noktasi)

### 1.3 Anahtar Yonetimi

**Dosya:** `keys.rs`

```rust
// Guvenilir dealer ile anahtar uretimi:
generate_with_dealer(max_signers, min_signers, identifiers, rng)
  -> (HashMap<Identifier, SecretShare>, PublicKeyPackage)

// Mevcut anahtari bolme:
split(secret: &SigningKey<SpendAuth>, max, min, identifiers, rng)
  -> (HashMap<Identifier, SecretShare>, PublicKeyPackage)

// t-of-n paylardan anahtari yeniden olusturma (FROST icin gerekmez!):
reconstruct(key_packages: &[KeyPackage]) -> SigningKey<SpendAuth>
```

**Tip aliklaslari:** Tum tipler `frost_core`'dan turkulmis:
- `SecretShare`, `SigningShare`, `VerifyingShare`, `VerifyingKey`
- `KeyPackage`, `PublicKeyPackage`
- `VerifiableSecretSharingCommitment`

### 1.4 DKG (Dagitik Anahtar Uretimi)

**Dosya:** `keys/dkg.rs`

3 asama, `frost_core::keys::dkg` sarmalayici:

```
Asama 1 — part1(identifier, max_signers, min_signers, rng):
  - Gizli: round1::SecretPackage (SAKLANMALI!)
  - Acik: round1::Package (BROADCAST edilmeli)
    - commitment: VerifiableSecretSharingCommitment
    - proof_of_knowledge: Signature (sahte imza)

Asama 2 — part2(secret_package, round1_packages):
  - Gizli: round2::SecretPackage (SAKLANMALI!)
  - Her katilimci icin ayri: round2::Package (GIZLI KANAL!)
    - signing_share: SigningShare (sifreli, aliciya ozel)

Asama 3 — part3(round2_secret, round1_packages, round2_packages):
  - Cikti: (KeyPackage, PublicKeyPackage)
    - KeyPackage: uzun omurlu anahtar payi
    - PublicKeyPackage: tum katilimcilarin acik anahtarlari + grup acik anahtari
```

**Serileştirme:** Proto mesajlari `DkgRound1Package` ve `DkgRound2Package` ile; `DomainType` trait uygulamali.

### 1.5 Imzalama Protokolu

**Dosya:** `lib.rs`

```
Round 1 — commit(secret, rng):
  -> (SigningNonces, SigningCommitments)
  SigningCommitments = {hiding: Element, binding: Element}

Round 2 — sign(signing_package, signer_nonces, key_package):
  -> SignatureShare
  signing_package = {commitments: BTreeMap<Id, Commitments>, message}

Toplama — aggregate(signing_package, signature_shares, pubkeys):
  -> Signature<SpendAuth>  // normal rdsa imzasi!
```

**Randomize edilmis imzalama** (icin ek fonksiyonlar):
```rust
// Randomizer ile imzalama (SpendAuth icin onemli!)
round2::sign_randomized(package, nonces, key_package, randomizer: Fr)
aggregate_randomized(package, shares, pubkeys, randomizer: Fr)
```

`frost_rerandomized` crate'i kullanilir; `Randomizer::from_scalar(randomizer)` ile `RandomizedParams` olusturulur.

### 1.6 Serileştirme

```
SigningCommitments -> pb::SigningCommitments {
    hiding: NonceCommitment { element: Vec<u8> },
    binding: NonceCommitment { element: Vec<u8> },
}

SignatureShare -> pb::SignatureShare { scalar: Vec<u8> }
DkgRound1Package -> pb::DkgRound1Package { commitment, proof_of_knowledge }
DkgRound2Package -> pb::DkgRound2Package { signing_share }
```

---

## 2. decaf377-ka — Anahtar Anlasma

**Dosya:** `src/lib.rs`

### 2.1 Yapılar

```rust
Public([u8; 32])        // Acik anahtar: sk * G sıkiştırılmış
Secret(decaf377::Fr)    // Gizli skaler (Zeroize + Drop)
SharedSecret([u8; 32])  // Paylasilan sir: sk * PK sıkiştırılmış
```

### 2.2 Protokol

```
// Anahtar turetimi:
Secret::new(rng) -> Secret
Secret::public() -> Public = (sk * G).compress()

// Cesitlendirilmis acik anahtar (bir gizli anahtardan cok farkli acik anahtar):
Secret::diversified_public(diversified_generator) = (sk * G_d).compress()

// Anahtar anlasma:
Secret::key_agreement_with(other: &Public) -> SharedSecret
  = (sk * PK).compress()
  // PK = decompress(other); hata -> InvalidPublic
```

**Cesitlendirme ozelligi:** `ivk * G_d` formulu — ayni gizli anahtar, farkli `G_d` ile farkli (baglantisiz) acik anahtarlar uretir. Bu, note adreslerinin cesitlendirilmesinde kullanilir.

**Zeroize:** `Secret` ve `SharedSecret` `Zeroize + Drop` implement eder — hafizadan temizlenir.

**Test:** `proptests.rs` — roundtrip ve anlasma uyumluluğu

---

## 3. proof-params — Groth16 Ispat Parametreleri

### 3.1 GROTH16_PROOF_LENGTH_BYTES

```rust
pub const GROTH16_PROOF_LENGTH_BYTES: usize = 192;
// Bls12_377 uzerinde Groth16 ispati: A(G1) + B(G2) + C(G1) = 48 + 96 + 48 = 192 byte
```

### 3.2 LazyProvingKey

```rust
pub struct LazyProvingKey {
    pk_id: &'static str,      // beklenen anahtarin bech32 kimlik ozeti
    inner: OnceCell<ProvingKey<Bls12_377>>,  // tembel yukleme
}

// Yukleme:
try_load(bytes: &[u8]) -> Result<&ProvingKey>
  // pk.debug_id() != self.pk_id → hata (kimlik uyumsuzlugu)

// Kimlik hesaplama:
ProvingKeyExt::debug_id(pk):
  SHA256(pk.serialize_compressed()) → bech32m("groth16pk", ...)

VerifyingKeyExt::debug_id(vk):
  SHA256(vk.serialize_compressed()) → bech32m("groth16vk", ...)
```

**Guvenlik:** Dogrulama anahtari kaynak kodda gomulmus `.param` dosyalarindan yuklenir — seri dogrulama hash'e gore yapilir. Ispat anahtari ise (cok buyuk) harici kaynaklardan yuklenir ama hash dogrulama ile korunur.

### 3.3 Desteklenen Devreler

| Statik Degisken | Ispat Anahtari | Dogrulama Anahtari |
|----------------|----------------|---------------------|
| SPEND_PROOF_PROVING_KEY | gen/spend_pk.bin | gen/spend_vk.param |
| OUTPUT_PROOF_PROVING_KEY | gen/output_pk.bin | gen/output_vk.param |
| SWAP_PROOF_PROVING_KEY | gen/swap_pk.bin | gen/swap_vk.param |
| SWAPCLAIM_PROOF_PROVING_KEY | gen/swapclaim_pk.bin | gen/swapclaim_vk.param |
| CONVERT_PROOF_PROVING_KEY | gen/convert_pk.bin | gen/convert_vk.param |
| DELEGATOR_VOTE_PROOF_PROVING_KEY | gen/delegator_vote_pk.bin | gen/delegator_vote_vk.param |
| NULLIFIER_DERIVATION_PROOF_PROVING_KEY | gen/nullifier_derivation_pk.bin | gen/nullifier_derivation_vk.param |

**Ozellik bayraklari:**
- `bundled-proving-keys`: Ispat anahtarlarini derleme zamaninda ikili dosyaya gom
- `download-proving-keys`: Derleme zamaninda agdan indir (hash kontrolu ile)

### 3.4 Test Araclari

```rust
// Devre kisitlama matrislerini uret (seremonya olmadan):
generate_constraint_matrices<T: DummyWitness>() -> ConstraintMatrices<Fq>

// Test icin gecici parametreler (guvenli degil!):
generate_test_parameters<T: DummyWitness>(rng) -> (ProvingKey, VerifyingKey)
generate_prepared_test_parameters<T: DummyWitness>(rng) -> (ProvingKey, PreparedVerifyingKey)
```

**DummyWitness trait:**
```rust
pub trait DummyWitness: ConstraintSynthesizer<Fq> {
    fn with_dummy_witness() -> Self;
}
```
Her devrenin kukla tanik degerleriyle somutlastirilabilmesi gerekir — kisitlama sentezi icin.

---

## 4. proof-setup — Guvenilir Kurulum Seremonisi

### 4.1 Genel Yapi

Groth16 icin iki fazli Powers of Tau seremonisi:
- **Faz 1 (Powers of Tau):** Devreden bagimsiz; `[x^i]`, `[αx^i]`, `[βx^i]`
- **Faz 2 (Devre Ozgul):** Devreden bagimsiz Faz 1 ciktisini devriye donusturur

### 4.2 Faz Gecisi

**Dosya:** `single/mod.rs` — `transition(phase1, circuit)` fonksiyonu

```
1. Lagrange katsayilari hesapla:
   extracting_poly = phase1.x_1[0..domain_size]
   domain.ifft_in_place(extracting_poly) → lagrange_1

2. ayni islem alpha_x_1, beta_x_1, x_2 icin

3. R1CS → QAP:
   A matrisi → u_i(X) → u_1 = beta*u_i + ...
   B matrisi → v_i(X) → v_1, v_2
   C matrisi → w_i(X) → p_1 katkisi

4. Vanishing polinom:
   t[i] = phase1.x_1[i + domain_size] - phase1.x_1[i]
   (X^d - 1 nin [x]'de degerlendirmesi)

5. Ozel Groth16 malleability onlemi:
   B matrisinin altina kimlik matrisi bloku ekleme
```

**FFT tabanli Lagrange:** `domain.ifft_in_place()` — O(d log d) yerine O(d^2) kacinir.

### 4.3 Ispat Anahtari Birlestirme

**`combine(circuit, phase1, phase2, extra)` → `ProvingKey<Bls12_377>`:**

```
VerifyingKey:
  alpha_g1 = phase1.alpha_1          (dogrulama icin)
  beta_g2  = phase1.beta_2
  gamma_g2 = G2::generator()
  delta_g2 = phase2.delta_2          (katki imzasi)
  gamma_abc_g1 = p_1[0..num_instances] (acik giris degerleri)

ProvingKey:
  beta_g1  = phase1.beta_1
  delta_g1 = phase2.delta_1
  a_query  = u_1                     (A matrisi sorgulari)
  b_g1_query = v_1
  b_g2_query = v_2                   (B matrisi G2 sorgulari)
  h_query  = inv_delta_t_1           (vanishing polinom)
  l_query  = p_1[num_instances..]    (ozel girdiler)
```

### 4.4 Test

`test_can_generate_keys_through_ceremony`: Uc bilesenli test devresine (x + y = x_plus_y) tam seremonya uygulanir:
1. Phase1 root olustur
2. Iki Phase1 katki yap
3. `transition()` ile Phase2 olustur
4. Iki Phase2 katki yap
5. `combine()` ile ispat anahtari birlestir
6. Groth16 ispati uret + dogrula

---

## 5. tct — Tiered Commitment Tree

### 5.1 Mimari

TCT, not taahhutlerini depolayan uclu katmanli Merkle agacidir:

```
Eternity Tree (Kuresel Agac):
  Kok: Anchor (Global Agac Koku = "Demir Cubuk")
  8 seviye, 4'lu
  Yaprak = Epoch Koku

Epoch Tree:
  8 seviye, 4'lu
  Yaprak = Blok Koku

Block Tree:
  8 seviye, 4'lu
  Yaprak = Note Commitment (StateCommitment)
```

**Kapasite:**
- Her blok: 4^8 = 65_536 taahhut
- Her epoch: 65_536 blok
- Eternity: 65_536 epoch
- Toplam: 65_536^3 ≈ 2.8 × 10^14 taahhut

**Tip:**
```rust
pub struct Tree {
    index: HashedMap<StateCommitment, index::within::Tree>,
    inner: Arc<frontier::Top<frontier::Tier<frontier::Tier<frontier::Item>>>>,
}
```

**`frontier`:** Henuz tamamlanmamis (yeni ekleme bekleyen) dugum tipi
**`complete`:** Tamamlanmis, ozet alinmis dugum tipi

### 5.2 Hash Fonksiyonlari

**Dosya:** `internal/hash.rs`

```rust
// Domain separator:
DOMAIN_SEPARATOR = Fq::from_le_bytes_mod_order(BLAKE2b("penumbra.tct"))

// Yaprak hash'i:
Hash::of(commitment: StateCommitment) = poseidon377::hash_1(&DOMAIN_SEPARATOR, commitment.0)

// Ic dugum hash'i (4 cocuk):
Hash::node(height, a, b, c, d) = poseidon377::hash_4(&(DOMAIN_SEPARATOR + height), (a, b, c, d))
```

**Poseidon377 hash:** ZK ispat dostu olan Poseidon hash fonksiyonu kullanilir (R1CS uyumlu).

### 5.3 Onceden Hesaplanmis Hash'ler

Bos blok dizileri uzerindeki ic dugum hash'leri onceden hesaplanir:

```rust
// PRECOMPUTE_HEIGHTS = 9..=16  (epoch yapraginin hemen ustunden epoch kokune kadar)
// Boş blok hash'lerinin gucu-4-hizali dizileri icin:
static PRECOMPUTED_HASH_PAIRS: Lazy<[(Hash, Hash); TOTAL_PRECOMPUTED]>

// Kullanim: eger tum 4 cocuk = onceden hesaplanmis girdi → dogrudan sonucu dondur
```

**Performans:** Epoch icindeki bos blok dizileri cok sık olusur → bu optimizasyon onemlidir.

### 5.4 Inklüzyon Ispati

**Dosya:** `proof.rs`

```rust
pub struct Proof(internal::proof::Proof<...>);

// Auth yolu:
Proof::new(commitment, position, auth_path: [[Hash; 3]; 24])
// 24 seviye, her seviyede 3 kardes hash (4'lu agac)
// Toplam: 24 * 3 = 72 Hash -> 72 * 32 = 2304 byte

// Dogrulama:
Proof::verify(&self, root: Root) -> Result<(), VerifyError>

// Kopya icin:
Proof::dummy(rng, commitment) -> Self
```

### 5.5 Position ve Root

```rust
pub struct Position(u64);  // 64-bit pozisyon indeksi
pub struct Root(Hash);      // Fq elemani olarak agac koku
```

`Root::is_empty()`: `hash == Hash::zero()` ise bos agac.

### 5.6 Storage Arayuzu

**Dosya:** `storage.rs`

Async storage trait: TCT'yi herhangi bir veri deposuna seri hale getirir. `in_memory.rs`: test amacli bellek ici implementasyon.

**Serileştirme stratejisi:** `serialize/fq.rs` — `Fq` alani elemanlari 32 byte olarak seri hale getirilir.

### 5.7 R1CS Desteği

**Dosya:** `r1cs.rs`

TCT ispati, ZK devresinde dogrulanabilir: `StateCommitmentVar`, `ProofVar` — arkworks R1CS gadget'lari.

### 5.8 Unutma (Forgotten)

```rust
pub struct Forgotten([u8; 6]);  // 6 byte = 48 bit, u64 olarak temsil edilir
// Agacin bir bolumu acikca "unutulduğunda" artan siralama sayaci
```

`Tree::forget(commitment)` — taahhudun tanikligini agactan kaldirir (hash'ler korunur).

---

## 6. Kritik Bulgular

### FROST

1. **H2 uyumluluğu kritik:** `BLAKE2b("decaf377-rdsa---", ...)` — normal rdsa challenge hash'i ile identik. FROST imzalari, normal Schnorr dogrulamasinda gecerlidir.

2. **Randomize imzalama:** `sign_randomized` + `aggregate_randomized` — SpendAuth icin zorunlu (korelasyon onleme). Randomizer harici olarak saglanir (not rseed'den turenir).

3. **round2::Package gizli kanal gerektiriyor** — Belge bunu acikca vurguluyor.

### proof-params

4. **Hash tabanli kimlik dogrulama:** `LazyProvingKey::try_load`, anahtarin SHA256 ozet bech32m kodlamasini (groth16pk) tur ID ile karsilastirir — yanlis anahtar yuklenirse hata verir.

5. **GROTH16_PROOF_LENGTH_BYTES = 192** — Bls12_377 uzerinde A(G1=48) + B(G2=96) + C(G1=48).

### proof-setup

6. **Malleability korumasi:** R1CS matrisine kimlik bloğu ekleme — Groth16 mallabililesine karsi; bkz. https://geometry.xyz/notebook/groth16-malleability.

7. **Lagrange FFT trick:** `ifft_in_place` ile O(d log d) Lagrange katsayisi hesaplama — saf O(d^2) yerine.

### TCT

8. **Poseidon hash zorunlu:** TCT, Poseidon377 kullanir cunku ZK devrelerinde verimli. SHA256 veya BLAKE2b kullansaydi R1CS maliyeti cok yuksek olurdu.

9. **4'lu agac:** Her dugum 4 cocuğa sahip (ikili degil). `auth_path: [[Hash; 3]; 24]` — 24 seviye, her seviyede 3 kardes.

10. **Epoch hash on hesaplama:** `PRECOMPUTE_HEIGHTS = 9..=16` — Bos blok dizileri icin (cok sik oluşur) hash yeniden hesaplanmaz.

---

## 7. Nova Shield Uygulama Notlari

### FROST (Esik Imzalama)

**Dogrudan kullanilabilir:** `decaf377-frost` → MPC validator set'i icin
- Validator kumulatif imzalamasi FROST ile yapilabilir
- `generate_with_dealer` + `split` → gecis doneminde guvenilir dealer
- `keys::dkg` → tam dagitik kurulum icin

**Dikkat:** Nova Shield, `cggmp21` ile MPC yapacak (hafizada). FROST bundan bagimsiz, Schnorr esik imzasi icin.

### proof-params (ZK Ispat Yonetimi)

**Dogrudan kopyalanabilir tasarim:**
- `LazyProvingKey` pattern → hash tabanli anahtar kimlik dogrulama ile
- `bundled-proving-keys` / `download-proving-keys` ozellik bayraklari
- `DummyWitness` trait → her devre icin test parametresi uretimi

**Nova Shield devreleri:** spend, output, swap, swapclaim, convert, delegator_vote — ayni kategoriler kullanilacak.

### proof-setup (Guvenilir Kurulum)

**Mainnet oncesi zorunlu:** Her yeni devre icin Faz 1 + Faz 2 seremonisi gerekli.
- `Phase1Contribution::make` → katki uretimi
- `transition(phase1, circuit)` → Faz 2 baslatma
- `combine(circuit, phase1, phase2, extra)` → nihai ispat anahtari

**Topluluk seremonisi:** Penumbra modeli: katki hash'leri zincirli (`contribution.hash()` bir sonraki girdi).

### TCT (Taahhut Agaci)

**Nova Shield SCT (State Commitment Tree) icin:**
- Poseidon377 hashleme → ZK uyumlu
- Uclu katmanlik (blok/epoch/eternity) → saglamlama planlamasiyla uyumlu
- `frontier` + `complete` ayirimi → etkin bellek kullanimi
- Epoch onceden hesaplanmis hash'ler → bos blok performansi

**Dikkat:** TCT'nin tam implementasyonu karmasiktir (30+ dosya). Nova Shield icin, `penumbra_sdk_tct` crate'ini dogrudan kullanmak yerine arkworks ornek Merkle tree'lerini kullanmak daha hizli olabilir.

---

## 8. Dosya Ozeti

### decaf377-frost (5 dosya)
| Dosya | Icerik |
|-------|--------|
| lib.rs | Signing round 1/2, aggregate, aggregate_randomized |
| traits.rs | Decaf377Rdsa Ciphersuite, H1-H5/HDKG/HID hash fonksiyonlari |
| hash.rs | Hasher (BLAKE2b-64, "FROST-decaf377" kisisel etiketi) |
| keys.rs | generate_with_dealer, split, reconstruct, tip takma adlari |
| keys/dkg.rs | part1/part2/part3, Round1/Round2 paket serileştirmesi |

### decaf377-ka (1 dosya + test)
| Dosya | Icerik |
|-------|--------|
| lib.rs | Public, Secret, SharedSecret; key_agreement_with, diversified_public |

### proof-params (10 dosya)
| Dosya | Icerik |
|-------|--------|
| lib.rs | LazyProvingKey, 7 statik PK/VK cifti, GROTH16_PROOF_LENGTH_BYTES=192 |
| traits.rs | DummyWitness, ProvingKeyExt/VerifyingKeyExt (bech32 debug_id) |
| gen/*.rs | Her devre icin sabit PROVING_KEY_ID string'leri |

### proof-setup (10 dosya)
| Dosya | Icerik |
|-------|--------|
| single/mod.rs | transition(), combine(), circuit_degree() |
| single/phase1.rs | Phase1CRSElements, Phase1Contribution |
| single/phase2.rs | Phase2CRSElements, Phase2Contribution |
| single/dlog.rs | DLogProof (katki dogrulama icin) |
| single/group.rs | F, G1, G2 tip takma adlari |
| all.rs | Tum devreler icin toplu kurulum |
| parallel_utils.rs | Paralel katki hesaplama yardimcilari |

### tct (30+ dosya)
| Dosya | Icerik |
|-------|--------|
| lib.rs | Tree, Root, Position, Proof, StateCommitment, Witness, Forgotten |
| tree.rs | Tree struct, HashedMap index, Arc<frontier::Top<...>> |
| proof.rs | Proof([Hash;3];24 auth yolu), verify(root) |
| internal/hash.rs | Hash(Fq), DOMAIN_SEPARATOR, GetHash trait, onceden hesaplanmis hash'ler |
| internal/frontier/*.rs | Tamamlanmamis dugum tipleri (item, leaf, node, tier, top) |
| internal/complete/*.rs | Tamamlanmis dugum tipleri |
| storage/*.rs | Async storage trait + bellek ici impl |
| r1cs.rs | ZK devre gadget'lari |
