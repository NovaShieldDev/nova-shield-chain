# multi-party-ecdsa — Tam Analiz Raporu

**Repo:** KZen-networks/multi-party-ecdsa
**Lisans:** GPL-3.0
**Dil:** Rust
**Versiyon:** 0.8.1
**Durum:** AKTIF OLARAK BAKIMI YAPILMIYOR (README'de açıkça belirtilmiş)

---

## 1. Genel Bakis

Bu repo, threshold ECDSA (Eliptik Egri Dijital Imza Algoritmasi) protokollerinin Rust implementasyonudur. Iki ana kategori vardir:

1. **Multi-party (cok tarafli):** GG18 ve GG20 protokolleri (t-of-n threshold imza)
2. **Two-party (iki tarafli):** Lindell 2017 ve CCLST 2019 protokolleri (2-of-2)

### Kullanilan Bagimliliklar (Cargo.toml)

| Paket | Versiyon | Amac |
|-------|----------|------|
| curv-kzen | 0.9 | Eliptik egri islemleri |
| kzen-paillier | 0.4.2 | Paillier homomorfik sifreler |
| zk-paillier | 0.4.3 | Paillier ZK ispatlari |
| round-based | 0.1.4 | State machine altyapisi |
| centipede | 0.3 | Gizli paylasim (centipede VSS) |
| subtle | 2 | Constant-time karsilastirmalar |
| zeroize | 1 | Bellekten hassas veri silme |
| sha2 | 0.9 | Hash fonksiyonu |
| class_group | (optional, cclst feature) | Class group sifreler |

---

## 2. Modul Yapisi

```
src/
├── lib.rs                          — Root, Error enum
├── protocols/
│   ├── multi_party_ecdsa/
│   │   ├── gg_2018/
│   │   │   ├── party_i.rs          — GG18 tam implementasyon (738 satir)
│   │   │   └── test.rs             — GG18 entegrasyon testleri (460 satir)
│   │   └── gg_2020/
│   │       ├── party_i.rs          — GG20 tam implementasyon (937 satir)
│   │       ├── blame.rs            — Identifiable abort / blame protokolu (456 satir)
│   │       ├── test.rs             — GG20 testleri + blame testleri (784 satir)
│   │       └── state_machine/
│   │           ├── keygen.rs       — Keygen state machine (529 satir)
│   │           ├── sign.rs         — Signing state machine (765 satir)
│   │           ├── keygen/
│   │           │   └── rounds.rs   — Keygen round implementasyonlari (348 satir)
│   │           ├── sign/
│   │           │   ├── rounds.rs   — Signing round implementasyonlari (731 satir)
│   │           │   └── fmt.rs      — Debug formatting (126 satir)
│   │           └── traits.rs       — RoundBlame trait
│   └── two_party_ecdsa/
│       ├── lindell_2017/
│       │   ├── party_one.rs        — Lindell Party1 (608 satir)
│       │   ├── party_two.rs        — Lindell Party2 (425 satir)
│       │   └── test.rs             — Lindell testleri
│       └── cclst_2019/
│           ├── party_one.rs        — CCLST Party1 (420 satir)
│           ├── party_two.rs        — CCLST Party2 (357 satir)
│           └── test.rs             — CCLST testleri
└── utilities/
    ├── mta/
    │   ├── mod.rs                  — MtA protokolu (215 satir)
    │   ├── range_proofs.rs         — ZK aralik ispatlari (711 satir)
    │   └── test.rs                 — MtA testi
    ├── zk_pdl/
    │   ├── mod.rs                  — Interaktif PDL ispati (263 satir)
    │   └── test.rs
    └── zk_pdl_with_slack/
        ├── mod.rs                  — PDLwSlack ispati (203 satir)
        └── test.rs

examples/
├── gg18_keygen_client.rs           — GG18 keygen CLI (272 satir)
├── gg18_sign_client.rs             — GG18 imzalama CLI (532 satir)
├── gg18_sm_manager.rs              — GG18 Rocket HTTP sunucusu (144 satir)
├── gg20_keygen.rs                  — GG20 keygen CLI (59 satir)
├── gg20_signing.rs                 — GG20 imzalama CLI (91 satir)
├── gg20_sm_client.rs               — GG20 SSE HTTP istemci (160 satir)
├── gg20_sm_manager.rs              — GG20 Rocket SSE sunucusu (194 satir)
└── common.rs                       — Ortak yardimci fonksiyonlar (226 satir)

benches/
├── multi_party_ecdsa/gg18/keygen.rs
└── two_party_ecdsa/
    ├── lindell_2017/keygen.rs + sign.rs
    └── cclst_2019/keygen.rs + sign.rs
```

---

## 3. src/lib.rs — Kok Modul

```rust
pub enum Error {
    InvalidKey,
    InvalidSS,      // Invalid Secret Share
    InvalidCom,     // Invalid Commitment
    InvalidSig,     // Invalid Signature
    Phase5BadSum,
    Phase6Error,
}
```

`protocols` ve `utilities` modulleri public export edilmistir.

---

## 4. GG18 Protokolu — Tam Analiz

**Kaynak:** Gennaro & Goldfeder 2018/2019 makalesi
**Dosya:** `src/protocols/multi_party_ecdsa/gg_2018/party_i.rs` (738 satir)

### 4.1 Temel Struct'lar

```rust
pub struct Parameters {
    pub threshold: u16,    // t (en az t+1 kisi gerekli)
    pub share_count: u16,  // n (toplam taraf sayisi)
}

pub struct Keys {
    pub u_i: Scalar<Secp256k1>,    // Ozel anahtar paylasi
    pub y_i: Point<Secp256k1>,     // y_i = G * u_i (public)
    pub dk: DecryptionKey,          // Paillier sifre cozme anahtari
    pub ek: EncryptionKey,          // Paillier sifreleme anahtari
    pub party_index: u16,
}

pub struct PartyPrivate {
    u_i: Scalar<Secp256k1>,        // Ham paylasim
    x_i: Scalar<Secp256k1>,        // VSS'den gelen paylasim
    dk: DecryptionKey,
}

pub struct SharedKeys {
    pub y: Point<Secp256k1>,       // Ortak public anahtar
    pub x_i: Scalar<Secp256k1>,    // Bu tarafin ozel paylasimi
}

pub struct SignKeys {
    pub w_i: Scalar<Secp256k1>,    // Lagrange agirlikli paylasim
    pub g_w_i: Point<Secp256k1>,   // G * w_i
    pub k_i: Scalar<Secp256k1>,    // Rastgele nonce paylasimi
    pub gamma_i: Scalar<Secp256k1>,// Rastgele yardimci skalar
    pub g_gamma_i: Point<Secp256k1>,
}

pub struct LocalSignature {
    pub l_i: Scalar<Secp256k1>,
    pub rho_i: Scalar<Secp256k1>,
    pub R: Point<Secp256k1>,       // Imzada kullanilanR noktasi
    pub s_i: Scalar<Secp256k1>,    // Yerel imza paylasimi
    pub m: BigInt,                 // Mesaj hash'i
    pub y: Point<Secp256k1>,       // Public anahtar
}
```

### 4.2 Anahtar Uretimi (KeyGen) — 5 Faz

**Faz 1 — Broadcast + Paillier Kaniti:**
- `Keys::create(index)`: Rastgele `u_i` scalar, `y_i = G*u_i` hesaplar, Paillier keypair uretir
- `Keys::phase1_broadcast_phase3_proof_of_correct_key()`:
  - `KnowledgeOfExponent::prove()` ile `y_i` icin DLog ispati
  - `CorrectKeyProof` ile Paillier modulunun dogru oldugunun ZK ispati
  - `HashCommitment` ile `y_i` ve DLog ispatina commitment olusturur
  - Dondurulenler: `(KeyGenBroadcastMessage1, KeyGenDecommitMessage1)`

**Faz 2 — VSS Dagitimi:**
- `phase1_verify_com_phase3_verify_correct_key_phase2_distribute()`:
  - Diger taraflarin commitment'larini dogrular
  - Paillier dogru anahtar ispatlarini dogrular
  - Feldman VSS ile gizli `u_i`'yi n paya boler
  - Her taraf icin bir pay hesaplar: `secret_shares[j]`
  - Returns: `(VerifiableSS, Vec<Scalar>, index)`

**Faz 3 — VSS Dogrulama + Ortak Anahtar:**
- `phase2_verify_vss_construct_keypair_phase3_pok_dlog()`:
  - Her taraftan gelen VSS payini dogrular
  - `x_i = sum(f_j(i))` hesaplar (butun taraflarin paylarinin toplami)
  - `y = sum(y_j)` ortak public anahtari hesaplar
  - Kendi `x_i`'si icin DLog ispati olusturur
  - Returns: `(SharedKeys, DLogProof)`

**Faz 4 — DLog Ispat Dogrulama:**
- `Keys::verify_dlog_proofs()`:
  - Butun taraflarin DLog ispatlarini dogrular
  - VSS commitment'lariyla tutarlilik kontrolu

### 4.3 Imzalama (Signing) — 9+ Adim

**SignKeys Olusturma:**
- `SignKeys::create(private, vss_scheme, index, signers_vec)`:
  - `w_i`: Lagrange interpolasyon katsayisiyla agirliklandirilmis pay
  - `gamma_i`: Rastgele scalar (nonce icin)
  - `k_i`: Rastgele scalar (k nonce'unun paylasimi)

**Faz 1 (Round 1):** Her taraf `g_gamma_i`'ye commitment yapar

**Faz 2 (Round 2-3) — MtA:**
- `MessageA::a()`: `k_i`'yi Paillier ile sifreler (`c = Enc(ek, k_i)`)
- `MessageB::b()`: Bob `gamma_j * k_i + beta_tag` hesaplar (homomorfik)
  - `alpha_ij = gamma_j * k_i + beta_ij` (Alice tarafinca decrypt edilir)
  - `beta_ij = -beta_tag` (Bob'un rastgele degeri)
  - Boylece `alpha_ij + beta_ij = gamma_j * k_i`
- Ayni islem `w_i` icin tekrarlanir (sigma icin)
- `phase2_delta_i()`: `delta_i = k_i * gamma_i + sum(alpha) + sum(beta)`
- `phase2_sigma_i()`: `sigma_i = k_i * w_i + sum(miu) + sum(ni)`
- Toplam: `sum(delta_i) = k * gamma` (distributed)

**Faz 3 (Round 4):** `delta_inv = (sum delta_i)^-1`

**Faz 4 (Round 5):**
- `SignKeys::phase4()`: `R = (sum g_gamma_i) * delta_inv`
- Bu `R = G * k^-1` anlamina gelir (ECDSA nonce noktasi)

**Faz 5 (Rounds 5-8) — ZK Dogrulama:**
- `LocalSignature::phase5_local_sig()`: `s_i = m * k_i + r * sigma_i`
- `phase5a_broadcast_5b_zkproof()`: HomoElGamal ZK ispati + DLog ispati
- `phase5c()`: Diger taraflarin ispatlarini dogrular, Phase5Com2 olusturur
- `phase5d()`: Final commitment dogrulama

**Cikti:** `s_i` parcalari toplanip normalize edilir: `s = sum(s_i) mod q`

Son imza: `(r, s)` where `r = R.x mod q`

### 4.4 GG18 Test Sonuclari

`test.rs`'de tam entegrasyon testi:
- `keygen_t_n_parties(t, n)`: Tam keygen simule eder
- `sign(t, n)`: Tam imzalama simule eder
- Testler: t=1,n=2 ve t=2,n=3 parametreleriyle calisir

---

## 5. GG20 Protokolu — Tam Analiz

**Kaynak:** Gennaro & Goldfeder 2020
**Fark:** GG18'den ek ozellik: **Identifiable Abort** (kimin hile yaptigini tespit eder)
**Dosyalar:** `party_i.rs` (937 satir) + `blame.rs` (456 satir)

### 5.1 GG18'den Ek Alanlar

```rust
// Keys struct'a eklenenler:
pub N_tilde: BigInt,   // Pedersen parametresi (aralik ispatlari icin)
pub h1: BigInt,        // h1 = rastgele
pub h2: BigInt,        // h2 = h1^(-xhi) mod N_tilde
pub xhi: Scalar<...>,  // h1,h2 arasindaki iliskinini bilen witness
pub xhi_inv: Scalar<...>,
```

### 5.2 h1, h2, N_tilde Uretimi

```rust
pub fn generate_h1_h2_N_tilde() -> (BigInt, BigInt, BigInt, Scalar, Scalar) {
    // Paillier key gen (guvenli asallar icin)
    let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();
    let N_tilde = ek_tilde.n;
    // h1 rastgele secer
    // h2 = h1_inv^xhi mod N_tilde
    // Sonra CompositeDLogProof ile N_tilde'in dogru olusturuldugunu ispat eder
}
```

### 5.3 KeyGen Broadcast'e Eklemeler

```rust
pub struct KeyGenBroadcastMessage1 {
    pub e: EncryptionKey,
    pub dlog_statement: DLogStatement,        // N_tilde, h1, h2
    pub com: BigInt,
    pub correct_key_proof: NICorrectKeyProof,
    pub composite_dlog_proof_base_h1: CompositeDLogProof,
    pub composite_dlog_proof_base_h2: CompositeDLogProof,
}
```

- Faz 1 dogrulamasinda Paillier anahtar boyutu [2047, 2048] arasinda olmalidur
- CompositeDLogProof dogrulamasi: h1 ve h2'nin dogru uretildigini kanitar

### 5.4 Phase 3 Eklemesi: T_i

```rust
pub fn phase3_compute_t_i(sigma_i: &Scalar) -> (Point, Scalar, PedersenProof) {
    let l_i = Scalar::random();
    let T_i = g * sigma_i + h * l_i;  // Pedersen commitment
    // PedersenProof: sigma_i ve l_i'nin bilindigini ispat eder
    (T_i, l_i, proof)
}
```

### 5.5 Phase 5 — PDLwSlack ile R_dash Kontrolu

```rust
pub fn phase5_proof_pdl(
    ek_vec, dlog_statement_vec, R, R_dash, k_i, k_randomness, sigma_i
) -> Vec<PDLwSlackProof> {
    // R_dash = k_i * R (beklenen)
    // PDLwSlack ispati: k_i, Paillier ciphertextindeki degerle ayni
}
```

### 5.6 Phase 6 — S_i HomoElGamal Dogrulama

```rust
pub fn phase6_compute_S_i_and_proof_of_consistency(
    R, sigma_i, l_i, T_i
) -> (Point, HomoELGamalProof, DLogProof) {
    let S_i = R * sigma_i;
    // HomoElGamal: S_i = R * sigma_i ve T_i = G * sigma_i + H * l_i tutarli
}
// phase6_check_S_i_sum: sum(S_i) == public key Y kontrol edilir
```

---

## 6. Blame Protokolu — Tam Analiz

**Dosya:** `src/protocols/multi_party_ecdsa/gg_2020/blame.rs` (456 satir)

### 6.1 Amaç

GG20'nin blameleme protokolu: Hangi tarafin yanlis davrandigini matematiksel olarak tespit eder. UC faz vardir:

### 6.2 Phase5 Blame (delta_i yanlis)

```rust
pub struct LocalStatePhase5 {
    pub k: Scalar,             // k_i
    pub k_randomness: BigInt,  // Paillier rastgelelik
    pub gamma: Scalar,         // gamma_i
    pub beta_randomness: Vec<BigInt>,
    pub beta_tag: Vec<Scalar>,
    pub encryption_key: Vec<EncryptionKey>,
}
```

`GlobalStatePhase5::phase5_blame()`:
1. Her taraf ozel durumunu aciklar
2. Sistem alpha_ij = gamma_j * k_i + beta_ij'i yeniden hesaplar
3. Orjinal MtA ciphertext'iyle karsilastirir
4. Yanlis delta_i ureten taraf tespit edilir → `bad_actors: Vec<usize>`

### 6.3 Phase6 Blame (sigma_i yanlis)

```rust
pub struct LocalStatePhase6 {
    pub k: Scalar,
    pub k_randomness: BigInt,
    pub miu: Vec<Scalar>,
    pub miu_randomness: Vec<BigInt>,
    pub proof_of_eq_dlog: ECDDHStatement,
}
```

`GlobalStatePhase6::phase6_blame()`:
1. miu_ij'leri Paillier ile yeniden sifreler ve karsilastirir
2. ECDDH ispatiyla sigma_i tutarliligini kontrol eder
3. Yanlis sigma_i ureten taraf bulunur

### 6.4 Phase7 Blame (s_i yanlis)

```rust
pub struct GlobalStatePhase7 {
    pub s_vec: Vec<Scalar>,      // Aciklanan s_i degerleri
    pub r: Scalar,               // R.x
    pub R_dash_vec: Vec<Point>,  // k_i * R
    pub m: BigInt,               // Mesaj
    pub R: Point,
    pub S_vec: Vec<Point>,       // R * sigma_i
}
```

`phase7_blame()`:
- Her taraf icin: `R * s_i == R_dash_i * m + S_i * r` kontrol edilir
- Bu basit ECDSA dogrulama: `G * s_i = G * (m*k_i + r*sigma_i)` demektir

---

## 7. State Machine (GG20) — Tam Analiz

**Dosyalar:** `keygen.rs` + `sign.rs` + `rounds.rs`

### 7.1 StateMachine Trait

`round_based` crate'inin `StateMachine` trait'i:
- `handle_incoming(msg)`: Gelen mesaji isleme
- `message_queue()`: Gonderilecek mesajlar
- `wants_to_proceed()`: Bu round'u ilerletmeye hazir mi?
- `proceed()`: Bir sonraki round'a gec
- `is_finished() / pick_output()`: Protokol bitti mi, sonuc ne?
- `RoundBlame::round_blame()`: Hangi taraflar cevap vermedi?

### 7.2 Keygen State Machine

```
Round0 → Round1 → Round2 → Round3 → Round4 → LocalKey
```

- **Round0:** `Keys::create()`, `KeyGenBroadcastMessage1` broadcast
- **Round1:** Decommit mesajini broadcast
- **Round2:** Commitment'lari dogrula, VSS paylarini P2P gonder (AES-GCM ile sifrelenmis)
- **Round3:** VSS'yi dogrula, `SharedKeys` ve DLogProof hesapla
- **Round4:** DLog ispatlarini dogrula → `LocalKey` cikar

`LocalKey<E>` struct (tam cikti):
```rust
pub struct LocalKey<E: Curve> {
    pub paillier_dk: DecryptionKey,
    pub pk_vec: Vec<Point<E>>,
    pub keys_linear: SharedKeys<E>,   // y (public key) ve x_i (ozel pay)
    pub paillier_key_vec: Vec<EncryptionKey>,
    pub y_sum_s: Point<E>,            // Toplam public key
    pub h1_h2_n_tilde_vec: Vec<...>,  // Pedersen parametreleri
    pub vss_scheme: VerifiableSS<E>,
    pub i: u16, pub t: u16, pub n: u16,
}
```

### 7.3 Signing State Machine

```
OfflineStage: Round0 → Round1 → Round2 → Round3 → Round4 → Round5 → Round6 → CompletedOfflineStage
Online: SignManual::new() → PartialSignature → SignManual::complete() → SignatureRecid
```

**Round0:** SignKeys olustur, (MessageA, broadcast commitment) gonder
**Round1:** Her diger taraf icin 2 MtA mesaji gonder (gamma ve w icin ayrı ayrı)
**Round2:** MtA sonuclarini al, delta_i, sigma_i, T_i hesapla, broadcast
**Round3:** delta_i'leri al, Pedersen ispatlarini dogrula, delta_inv hesapla, broadcast decommit
**Round4:** Decommit al, R yeniden olustur, R_dash = k_i * R, PDLwSlack ispat gonder
**Round5:** PDLwSlack ispatlarini dogrula, R_dash toplamini kontrol et, S_i + HomoElGamal ispati
**Round6:** HomoElGamal ispatlarini dogrula, S_i toplami = Y kontrol et → `CompletedOfflineStage`

**Online (SignManual):**
- `new(message, completed)`: `s_i = m * k_i + r * sigma_i` hesapla
- `complete(sigs)`: `s = sum(s_i)` topla, normalize et → `SignatureRecid`

---

## 8. Utilities — Tam Analiz

### 8.1 MtA (Multiplicative-to-Additive) — mod.rs (215 satir)

```rust
pub struct MessageA {
    pub c: BigInt,                   // Enc(ek, k_i)
    pub range_proofs: Vec<AliceProof>, // k_i aralik ispatlari
}

pub struct MessageB {
    pub c: BigInt,                   // c_B = b * c_A + Enc(beta_tag)
    pub b_proof: DLogProof,           // Bob'un b'sini bildigi ispat
    pub beta_tag_proof: DLogProof,    // beta_tag'i bildigi ispat
}
```

**Protokol ozeti:**
1. Alice: `c_A = Enc(ek_Alice, k_i)` gonder
2. Bob: `c_B = b * c_A + Enc(beta_tag)` hesapla (homomorfik)
   - Bu `Enc(b * k_i + beta_tag)` demektir
3. Alice: `alpha = Dec(dk_Alice, c_B) = b * k_i + beta_tag`
4. Bob: `beta = -beta_tag`
5. Sonuc: `alpha + beta = b * k_i` (carpma toplama donusturuldu)

### 8.2 ZK Aralik Ispatlari — range_proofs.rs (711 satir)

**AliceProof** (Alice'in k_i'sinin kucuk oldugunu ispat eder):
```
z = h1^x * h2^rho mod N_tilde  (Pedersen commitment)
e = SHA256(ek, c, z, u1, u2, u3)  (Fiat-Shamir challenge)
Cevap: s, s1, s2
```
x < q^3 olmak zorunda (aralik kısiti)

**BobProof / BobProofExt** (Bob'un cevabinin aralikta oldugunu ispat eder):
- Daha karmasik: t, z, e, s, s1, s2, t1, t2 alanlari
- `BobProofExt`: ek u alani (ek kanit)

**SampleFromMultiplicativeGroup** trait:
- `sample_from_mult_group(n: &BigInt) -> BigInt`
- Zn* grubundan eleman ornekleme

### 8.3 ZK-PDL — zk_pdl/mod.rs (263 satir)

Interaktif ispat (Lindell 2017'de kullanilir):

```
Statement: (c, pk, Q, G)
Witness: (x, r, sk) — Q = xG, c = Enc(pk, x, r)
```

3 turlu interaktif protokol:
1. Verifier: `c_tag = a*c + Enc(b)`, commitment `(a,b,blindness)` gonder
2. Prover: `alpha = Dec(dk, c_tag)`, `q_hat = G*alpha` commit et, aralik ispati uret
3. Verifier: (a, b, blindness) ac
4. Prover: `a*x + b = alpha` kontrol et, q_hat ac
5. Verifier: `q_hat = a*Q + b*G` kontrol et

### 8.4 PDLwSlack — zk_pdl_with_slack/mod.rs (203 satir)

Non-interaktif ispat (GG20'de kullanilir):

```
Statement: (c, pk, Q, G, h1, h2, N_tilde)
Witness: (x, r) — Q = xG, c = Enc(pk, x, r)
Aralik: x ∈ [-q^3, q^3]
```

Sigma protokolu, Fiat-Shamir ile NI hale getirilmis:
- `z = h1^x * h2^rho mod N_tilde`
- `u1 = G * alpha`
- `u2 = Enc(alpha, beta)` (Paillier homomorfik)
- `u3 = h1^alpha * h2^gamma mod N_tilde`
- `e = SHA256(G, Q, c, z, u1, u2, u3)`
- Cevaplar: `s1 = e*x + alpha`, `s2 = r^e * beta mod n`, `s3 = e*rho + gamma`

`commitment_unknown_order(h1, h2, N_tilde, x, r)`:
- `h1^x * h2^r mod N_tilde`
- Negatif r icin: `h2_inv^(-r) mod N_tilde` kullanilir

---

## 9. Lindell 2017 (2-Party) — Tam Analiz

**Dosyalar:** `party_one.rs` (608 satir) + `party_two.rs` (425 satir)

### 9.1 Anahtar Yapisi

- **Party1:** `x1` ozel paylasimi + Paillier keypair + `x1` Paillier ile sifrelenmis
- **Party2:** `x2` ozel paylasimi + Party1'in Paillier public key'i + sifrelenmis `x1`
- Ortak public key: `Q = x1 * G + x2 * G = (x1 + x2) * G`
- Ozel anahtar (kimse tam bilemedigi): `x = x1 + x2`

### 9.2 KeyGen

**Party1:**
1. `KeyGenFirstMsg::create_commitments()`: Rastgele `x1`, `public_share = G*x1`, commitment uret
2. `PaillierKeyPair::generate_keypair_and_encrypted_share()`: Paillier keypair + `Enc(x1)` uret
3. `PaillierKeyPair::pdl_proof()`: PDLwSlack ispati — Paillier sifreleme ve EC public key tutarli

**Party2:**
1. `KeyGenFirstMsg::create()`: Rastgele `x2`, DLog ispati
2. `PaillierPublic::pdl_verify()`: Party1'in PDL ispatini dogrula
3. `PaillierPublic::verify_ni_proof_correct_key()`: Paillier modulus >= 2047 bit

### 9.3 Imzalama

**Gecici anahtar olusturma:**
- Her iki taraf da rastgele `k1`, `k2` uretir
- Commit-reveal protokoluyle R noktasi = `k1 * k2 * G` hesaplanir
- ECDDH ispatlariyla tutarlilik saglanir

**Party2 imza hesabi:**
```rust
pub fn compute(ek, encrypted_secret_share, party2_private, eph_ec_key_pair2, R1, message) {
    let rho = Scalar::random();
    // c3 = Paillier(k2_inv * m + k2_inv * r * x2 * x1)
    //     = k2_inv * m * G(ek) + k2_inv * r * x2 * c_key (homomorfik)
    let c3 = ...;
    PartialSig { c3 }
}
```

**Party1 imza tamamlama:**
```rust
pub fn compute(party1_private, c3, eph_ec_key_pair1, R2) {
    let alpha = Dec(dk, c3);  // = k2_inv * m + r * k2_inv * x2 * x1
    let s = k1_inv * alpha;   // = k1_inv * k2_inv * m + r * x = k^-1 * m + r * x
    // normalize s
}
```

### 9.4 Key Rotation

```rust
pub fn refresh_private_key(party1_private, factor: &Scalar) -> Party1Private {
    // x1_new = x1 * factor
    // Encrypted share yenilenir
}
```

---

## 10. CCLST 2019 (2-Party Class Group) — Tam Analiz

**Dosyalar:** `party_one.rs` (420 satir) + `party_two.rs` (357 satir)

### 10.1 Temel Fark: Paillier YOK, HSM-CL VAR

CCLST 2019, Paillier yerine **Class Group Encryption** (HSM-CL) kullanir:
- RSA benzeri varsayimlara dayanmaz
- Daha guclu guvenlik varsayimlari altinda

```rust
// CLGroup parametresi: discriminant = 1348 bit
let cl_group = CLGroup::new_from_setup(&1348, seed);
```

### 10.2 HSMCL Struct

```rust
pub struct HSMCL {
    pub sk: SK,       // Gizli anahtar
    pub pk: PK,       // Public anahtar
    pub ciphertext: Ciphertext,  // Enc(x1)
    ...
}
```

`generate_keypair_and_encrypted_share_and_proof()`:
- `verifiably_encrypt(cl_group, pk, x1)` — hem sifreler hem ispat uretir
- `CLDLProof`: Discrete log x1'in bilindigini class group'ta ispat eder

### 10.3 Party2 Imzalama

```rust
pub fn compute(party_two_hsmcl_pub, party2_private, eph2, R1, message) {
    // eval_scal: Class group skalerle carpma (Paillier mul gibi)
    // eval_sum: Class group toplama (Paillier add gibi)
    let c3 = eval_sum(eval_scal(c1, k2_inv_m), eval_scal(c_key, k2_inv_rx2));
    PartialSig { c3 }
}
```

### 10.4 Party1 Sifre Cozme

```rust
pub fn compute(hsmcl, party1_private, c3, eph1, R2) {
    let alpha = decrypt(cl_group, sk, c3);  // Class group decrypt
    // Geri kalan: Lindell ile ayni
}
```

---

## 11. Ornekler (Examples) — Tam Analiz

### 11.1 GG18 Keygen Client (272 satir)

HTTP tabanli koordinasyon:
1. `/signupkeygen` → party numarasi ve UUID al
2. 5 round HTTP broadcast/p2p:
   - Round 1: `KeyGenBroadcastMessage1`
   - Round 2: `KeyGenDecommitMessage1`
   - Round 3: P2P VSS paylar (AES-GCM sifrelenmis)
   - Round 4: `VerifiableSS` commitmentlari
   - Round 5: DLog ispatlari
3. Anahtar JSON dosyasina kaydedilir

**P2P Sifrelem:** `key = decom_j.y_i * u_i` (ECDH) → AES-256-GCM

### 11.2 GG18 Sign Client (532 satir)

9 round HTTP koordinasyonu, butun GG18 imzalama adimlarini kapsar:
- Round 0: Signer IDleri topla
- Round 1: MtA mesaji + commitment
- Round 2: P2P MtA cevaplari
- Round 3: delta_i broadcast
- Round 4: Decommit
- Rounds 5-8: Phase5 ZK ispatlari (4 asamali)
- Round 9: s_i parcalari

### 11.3 GG18 SM Manager (144 satir)

Rocket framework ile basit HTTP sunucu:
- `GET /get, POST /set`: Key-value store (HashMap<String, String>)
- `POST /signupkeygen`: Party numarasi ata
- `POST /signupsign`: Imzalama icin party numarasi ata
- In-memory `RwLock<HashMap>`, kalici depolama yok

### 11.4 GG20 SM Client (160 satir)

Rocket SSE (Server-Sent Events) ile:
- `join_computation(addr, room_id)` → `(party_index, message_stream, message_sink)`
- `subscribe()`: SSE stream → `impl Stream<Item=Msg>`
- `broadcast()`: HTTP POST

### 11.5 GG20 SM Manager (194 satir)

`Arc<Room>` ile room yonetimi:
- `HashMap<String, Arc<Room>>` — room adi → room
- `Notify` ile broadcast: Her mesaj gelince tum subscribe'cilari bildirir
- SSE stream JSON mesajlari (index + body)

### 11.6 Common (226 satir)

```rust
fn postb(client, path, body) → Result<String>  // HTTP POST
fn broadcast(client, party, round, data, uuid)  // Herkese gonder
fn sendp2p(client, from, to, round, data, uuid) // Sadece birine gonder
fn poll_for_broadcasts(client, party, n, delay, round, uuid) // Bekle
fn aes_encrypt(key, plaintext) -> AEAD          // AES-256-GCM
fn aes_decrypt(key, aead) -> Vec<u8>
fn check_sig(r, s, msg, pk)                     // secp256k1 dogrulama
```

---

## 12. Benchmark'lar — Tam Analiz

**GG18 Keygen bench:** t=1 n=2 ve t=2 n=3 icin `Criterion` benchmark
**Lindell 2017 keygen bench:** 10 ornekle tam keygen
**Lindell 2017 sign bench:** 10 ornekle tam imzalama
**CCLST 2019 keygen bench:** 10 ornekle (pi seed'i kullanir)
**CCLST 2019 sign bench:** 10 ornekle tam HSMCL imzalama

---

## 13. Test Kapsamı — Tam Analiz

### MtA Testi (test_mta)
- Alice input ve Bob input rastgele
- MtA protokolu calistirilir
- `alpha + beta == alice_input * bob_input` dogrulama

### ZK-PDL Testi (test_zk_pdl)
- x < q/3 (aralik gerekliligi)
- Tam 3-turlu protokol calistirilir
- Dogrulama yapilir

### PDLwSlack Testleri
- `test_zk_pdl_with_slack`: Dogru ispat calisir
- `test_zk_pdl_with_slack_soundness`: Yanlis ispat (x+1 yerine x sifrelenir) → `should_panic`

### Lindell 2017 Testleri
- `test_d_log_proof_party_two_party_one`: DLog kanit dogrulama
- `test_full_key_gen`: Tam keygen (Paillier + PDL)
- `test_two_party_sign`: Tam imzalama ve dogrulama

### CCLST 2019 Testleri
- Ayni pattern, HSMCL ile (CLGroup parametresi: pi'nin rakamlarindan seed)
- `test_d_log_proof_party_two_party_one`
- `test_full_key_gen`
- `test_two_party_sign`

### GG20 Blame Testleri (test.rs, 784 satir)
- `corrupt_step_5`: Phase 5'te hile yapan taraf tespit ediliyor
- `corrupt_step_6`: Phase 6'da hile yapan tespit ediliyor
- `corrupt_step_7`: Phase 7'de hile yapan tespit ediliyor
- Her test `bad_actors` vektorunu dogrular

---

## 14. Guvenlik Notlari ve Zaafiyetler

### Kritik Problemler:

1. **REPO BAKIMSIZ** — README'de acik: "This repo is no longer maintained". GG20 bile bu repoda yetersiz kalabilir. CGGMP21 kullanmak gerekir.

2. **GG20 Identifiable Abort Eksikligi** — GG18'de identifiable abort YOK, acik sorun. GG20'de var ama blame protokolu karmasik.

3. **N_tilde Guvenli Asallarla Uretilmeli** — Test kodunda yorum olarak belirtilmis:
   ```rust
   // note: safe primes should be used:
   // let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();
   ```
   Testlerde guvenli asallar KULLANILMIYOR — guvenli asalsiz N_tilde guvenlik zafiyeti olusturur!

4. **Paillier Modulus Boyutu** — En az 2047 bit kontrol edilmis, ama 2048 bit olmasini ensure etmek daha iyi.

5. **AES-GCM Anahtar Turetimi** — `key = ecdh_point.x_coord()` → raw x koordinat, HKDF kullanilmamis. Potansiyel zayiflik.

6. **Constant-Time Degil** — `subtle` crate eklenmis ama buyuk int islemleri (BigInt) constant-time degil. Timing attack riski.

7. **Paralel Oturum Saldirisi** — GG20 state machine'de oturum ID kontrolu yok, paralel oturumlarda sorun cikabilir.

---

## 15. Nova Shield Chain Icin Cikarsimlar

### Kullanilabilecek Yapi ve Fikirler:

1. **State Machine Pattern**: `StateMachine` + `RoundBlame` — Nova Shield icin keygen/signing protokolunde ayni pattern uygulanmali

2. **Blame Protokolu Yapisi**: `LocalStatePhase5/6/7` pattern — Nova Shield'in guardian-based signing'inde "kim hile yapti" mekanizmasi icin referans

3. **PDLwSlack Ispati**: `commitment_unknown_order` — Nova Shield'in ZK ispat altyapisinda kullanilabilir

4. **MtA Kalip Kodu**: Alice-Bob message pattern, ZK aralik ispatlari — Nova Shield MPC'sinde referans

5. **AES-GCM P2P Sifrelem**: Guardian-to-guardian sifreleme icin referans (ancak HKDF eklenmeli)

### Kullanilmayacak / Degistirilecekler:

1. **GG20 yerine CGGMP21** — Kararimiz dogru, bu repo zaten eski/bakimsiz

2. **Paillier yerine daha modern sema** — Nova Shield icin Pedersen commitment + Bulletproofs veya PLONK dusunulmeli

3. **Secp256k1 yerine** — Nova Shield Dilithium3 (post-quantum) + standart EC kombinasyonu kullanacak

4. **BigInt yerine constant-time implementasyon** — timing attack onleme icin zorunlu

5. **HTTP koordinasyonu yerine** — Nova Shield P2P libp2p + gossipsub kullanacak

---

## 16. Ozet

| Ozellik | Deger |
|---------|-------|
| Toplam satir (tahmin) | ~6,000 satir kaynak |
| Ana protokoller | GG18, GG20, Lindell2017, CCLST2019 |
| Test kapsamı | Tam entegrasyon + blame testleri |
| Uretim icin hazir mi? | HAYIR — bakimsiz, guvenli asal eksik |
| Nova Shield icin faydali mi? | EVET — referans mimarisi olarak |
| En onemli ogrenme | State machine pattern, blame protokolu yapisi |

**Sonuc:** Bu repo, threshold ECDSA'nin pedagojik ve referans implementasyonu olarak mukemmeldir. Ancak uretim ortaminda dogrudan kullanamayiz. Nova Shield icin CGGMP21'i esas alacagiz ve bu repodan ogrenerek kendi implementasyonumuzu yazacagiz.
