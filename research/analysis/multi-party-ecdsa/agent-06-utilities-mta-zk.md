# Agent-06: MtA + ZK Proof Utilities Analizi

## Kapsanan Dosyalar

| Dosya | Satir | Icerik |
|-------|-------|--------|
| `src/utilities/mod.rs` | 4 | Modul bildirimleri |
| `src/utilities/mta/mod.rs` | 215 | MtA protokolu, MessageA/B, alpha hesaplama |
| `src/utilities/mta/range_proofs.rs` | 711 | AliceProof, BobProof, BobProofExt range ZKP |
| `src/utilities/mta/test.rs` | 19 | MtA duzeltlik testi |
| `src/utilities/zk_pdl/mod.rs` | 263 | Interaktif PDL ispatı (protokol 6.1) |
| `src/utilities/zk_pdl/test.rs` | 58 | PDL test |
| `src/utilities/zk_pdl_with_slack/mod.rs` | 203 | Non-interaktif PDL with slack |
| `src/utilities/zk_pdl_with_slack/test.rs` | 129 | PDL+slack testi, soundness testi |

Referans makaleler:
- MtA: https://eprint.iacr.org/2019/114.pdf Section 3 + Appendix A
- PDL: https://eprint.iacr.org/2017/552.pdf Protocol 6.1
- PDL w/ Slack: https://eprint.iacr.org/2016/013.pdf (Proof PIi) + IJIS 2004 Proof 6.3

---

## 1. MtA (Multiplicative-to-Additive) Protokolu

### 1.1 Matematiksel Temel

**Amac:** Alice'in `a` degeri ile Bob'un `b` degeri carpilarak iki additive paya bolunur:

```
alpha + beta = a * b   (mod q,  q = secp256k1 grup mertebesi)
```

Alice `alpha`'yi, Bob `beta`'yi ogrenir; ama Alice `b`'yi, Bob `a`'yi ogrenemiyor.

**Guclu nokta:** Paillier'in homomorfik ozelligi kullanilir:
- `Enc(a*b + beta') = Enc(a)^b * Enc(beta')`
- Alice Paillier sifreli metni cozunce `a*b + beta'` elde eder.
- Bob `beta = -beta'` atar, dolayisiyla `alpha + beta = a*b` saglanir.

### 1.2 Alice Tarafi

**`MessageA` struct** (`mta/mod.rs`, satir 34-38):
```rust
pub struct MessageA {
    pub c: BigInt,                     // Paillier(a) sifreli metin
    pub range_proofs: Vec<AliceProof>, // Her Bob icin ayri range proof
}
```

**`MessageA::a()` fonksiyonu** (`mta/mod.rs`, satir 52-60):
1. `randomness <- Z_N` orneklenir (Paillier modulusu)
2. `c_a = Enc_A(a; randomness)` -- Paillier sifrelemesi
3. Her `dlog_statement` (diger partinin h1,h2,N_tilde) icin `AliceProof::generate()` uretilir
4. Donus: `(MessageA { c: c_a, range_proofs }, randomness)`

**Alice'in aldigi pay (alpha)** (`mta/mod.rs`, satir 160-179):
```rust
pub fn verify_proofs_get_alpha(&self, dk, a) -> Result<(Scalar, BigInt), Error>
```
1. `alice_share = Dec_A(c_b)` -- Bob'un mesajini Paillier ile cozer
2. `alpha = Scalar::from(alice_share)` -- mod q'ya indirgenir
3. Dogrulama: `G*alpha == B*a + G*beta'` olmali (EC noktasi esitligi)
   - `ba_btag = b_proof.pk * a + beta_tag_proof.pk`
   - `g_alpha = G * alpha`
   - Esit olmali
4. Hem `b_proof` hem `beta_tag_proof` DLog kanıtları dogrulanir

**Kritik guvenlik notu:** `verify_proofs_get_alpha` fonksiyonu Alice'in Bob'un DLog ispatlarini dogruladigini, ancak ek bir range proof gerektirmedigini gosterir. Bob'un `b` ve `beta'` degerlerinin EC gruplarinda oldugu DLogProof ile gosterilmektedir.

### 1.3 Bob Tarafi

**`MessageB` struct** (`mta/mod.rs`, satir 40-45):
```rust
pub struct MessageB {
    pub c: BigInt,                              // Paillier(a*b + beta') sifreli metin
    pub b_proof: DLogProof<Secp256k1, Sha256>,  // G*b'nin bilgisi
    pub beta_tag_proof: DLogProof<Secp256k1, Sha256>, // G*beta''nin bilgisi
}
```

**`MessageB::b()` ve `b_with_predefined_randomness()` fonksiyonu** (`mta/mod.rs`, satir 91-158):

1. **Giris dogrulamasi:** `m_a.range_proofs.len() == dlog_statements.len()`; her AliceProof verify edilir. Herhangi biri basarisiz olursa `Err(InvalidKey)`.
2. `beta' <- Z_N` orneklenir (Bob'un gizli kaymasi)
3. `r <- Z_N` orneklenir (Paillier randomness)
4. `c_beta' = Enc_A(beta'; r)` -- Alice'in public key ile sifrele
5. `b_c_a = c_a^b = Enc_A(a*b)` -- Paillier homomorfik carpma
6. `c_b = b_c_a + c_beta' = Enc_A(a*b + beta')` -- Paillier homomorfik toplama
7. `beta = -beta'` (mod q) -- Bob'un aldigi pay
8. `b_proof = DLogProof(b)` -- G*b bilgisi
9. `beta_tag_proof = DLogProof(beta')` -- G*beta' bilgisi
10. Donus: `(MessageB { c: c_b, b_proof, beta_tag_proof }, beta, randomness, beta')`

**Matematiksel ispatı:**
```
c_b = Enc(a*b + beta')
Alice cozer: alpha = a*b + beta'  (mod N, mod q)
beta = -beta'
=> alpha + beta = a*b + beta' - beta' = a*b
```

### 1.4 MtA vs MtAwc (Malicious)

Kod, "MtAwc" (with check) kavramini `range_proofs.rs` icerisinde `BobProofExt` ve `check: bool` parametresi uzerinden uygulamaktadir (`range_proofs.rs`, satir 422-423).

| Ozellik | MtA (semi-honest) | MtAwc (malicious) |
|---------|-------------------|-------------------|
| Kullanilan struct | `BobProof` | `BobProofExt` |
| Ek alan | Yok | `u: Point` (EC taahhut) |
| Hash girdileri | 9 deger | +4 deger (X, u EC koordinatlari) |
| Dogrulama | Temel ZKP esitligi | + `G*s1 == X*e + u` kontrol |
| Neden gerekli | Semi-honest protokol | Malicious adversary tespiti |

**MtAwc'nin eklentisi** (`range_proofs.rs`, satir 447-469):
- Bob ayrica `X = G*b` ve `u = G*alpha` EC noktalarini hash'e dahil eder
- `BobProofExt::verify()` ek olarak: `G*s1 == X*e + u` kontrol eder (`range_proofs.rs`, satir 524-531)
- Bu, Bob'un `b`'nin gercek degeri uzerinde dogrusal bilgi ispati saglar

---

## 2. Range Proofs

### 2.1 Neden Gerekli?

MtA protokolunde Alice `a`'nin kucuk oldugunu (|a| < q^3) kanitlamazsa Bob, Alice'i kandirarak buyuk bir deger gonderip `k^-1` seklini cikarabilir. Paillier sifrelemesi plaintext'in buyuklugunu saklar, dolayisiyla zemin kaniti olmaksizin protokol guvenli degildir.

**Kaynak dosya notu** (`range_proofs.rs`, satir 3-11):
- ING bank threshold signatures implementasyonundan adapte
- Resmi tanim: https://eprint.iacr.org/2019/114.pdf Appendix A
- **Onemli sapma:** Bob'un ispatinda `gamma <- [0, q^2*N]` ve `tau <- [0, q^3*N_tilde]` (spesifikasyondan farkli)
- **Non-interactive versiyonu:** Challenge `e` Fiat-Shamir ile hesaplanir

### 2.2 AliceProof (RangeProofNi Benzeri, Alice icin Ozellesmis)

**Struct** (`range_proofs.rs`, satir 94-101):
```rust
pub struct AliceProof {
    z: BigInt,   // Pedersen commitment: h1^a * h2^ro mod N_tilde
    e: BigInt,   // Fiat-Shamir challenge (SHA-256)
    s: BigInt,   // r^e * beta mod N
    s1: BigInt,  // e*a + alpha
    s2: BigInt,  // e*ro + gamma
}
```

**Round1 (`AliceZkpRound1`)** (`range_proofs.rs`, satir 26-67):
```
alpha <- Z_{q^3}
beta  <- Z*_N  (Paillier N'den ornekleme, gcd=1)
gamma <- Z_{q^3 * N_tilde}
ro    <- Z_{q * N_tilde}
z  = h1^a  * h2^ro   mod N_tilde    (Pedersen commitment to a)
u  = (N+1)^alpha * beta^N  mod N^2  (Paillier encryption of alpha)
w  = h1^alpha * h2^gamma mod N_tilde (commitment to alpha)
```

**Fiat-Shamir Challenge** (`range_proofs.rs`, satir 175-182):
```
e = SHA256(N || N+1 || cipher || z || u || w)
```

**Round2 (`AliceZkpRound2`)** (`range_proofs.rs`, satir 77-91):
```
s  = r^e * beta  mod N       (randomness birlesimi)
s1 = e*a + alpha             (linear combination: a'nin gizli tutarak a'yi acikla)
s2 = e*ro + gamma            (binding factor)
```

**Dogrulama** (`range_proofs.rs`, satir 103-156):
1. `s1 <= q^3` kontrol (range bound)
2. `w_test = h1^s1 * h2^s2 * z^{-e} mod N_tilde`
3. `u_test = (N+1)^s1 * s^N * cipher^{-e} mod N^2`
4. `e_test = SHA256(N || N+1 || cipher || z || u_test || w_test)`
5. `e_test == self.e` kontrol

**Neden calisir:**
- `w_test = h1^{e*a+alpha} * h2^{e*ro+gamma} * (h1^a * h2^ro)^{-e} = h1^alpha * h2^gamma = w` (round1)
- `u_test = (N+1)^{e*a+alpha} * s^N * Enc(a)^{-e} = Enc(alpha) = u` (round1)

### 2.3 BobProof (Bob icin Range Proof)

**Struct** (`range_proofs.rs`, satir 307-317):
```rust
pub struct BobProof {
    t:  BigInt,  // h1^beta' * h2^sigma mod N_tilde (commitment to beta')
    z:  BigInt,  // h1^b * h2^ro mod N_tilde (commitment to b)
    e:  BigInt,  // Fiat-Shamir challenge
    s:  BigInt,  // r^e * beta mod N
    s1: BigInt,  // e*b + alpha
    s2: BigInt,  // e*ro + ro'
    t1: BigInt,  // e*beta' + gamma
    t2: BigInt,  // e*sigma + tau
}
```

**Round1 (`BobZkpRound1`)** (`range_proofs.rs`, satir 197-265):
```
alpha  <- Z_{q^3}
beta   <- Z*_N
gamma  <- Z_{q^2 * N}      (NOT: spesifikasyondan sapma: q^2 degil q^3 olmali)
ro     <- Z_{q * N_tilde}
ro'    <- Z_{q^3 * N_tilde}
sigma  <- Z_{q * N_tilde}
tau    <- Z_{q^3 * N_tilde}

z      = h1^b * h2^ro mod N_tilde
z'     = h1^alpha * h2^ro' mod N_tilde
t      = h1^beta' * h2^sigma mod N_tilde
w      = h1^gamma * h2^tau mod N_tilde
v      = c_a^alpha * (N+1)^gamma * beta^N mod N^2
         (= Enc_A(a*alpha + gamma))
```

**Fiat-Shamir Challenge** (9 deger hashlenir, `range_proofs.rs`, satir 433-444):
```
e = SHA256(N || N+1 || a_encrypted || mta_encrypted || z || z' || t || v || w)
MtAwc durumunda + SHA256(X_x || X_y || u_x || u_y)
```

**Round2 (`BobZkpRound2`)** (`range_proofs.rs`, satir 289-297):
```
s  = r^e * beta mod N
s1 = e*b + alpha
s2 = e*ro + ro'
t1 = e*beta' + gamma
t2 = e*sigma + tau
```

**Dogrulama** (`range_proofs.rs`, satir 321-411):
1. `s1 <= q^3` range kontrol
2. `z'_test = h1^s1 * h2^s2 * z^{-e} mod N_tilde`
3. `v_test = c_a^s1 * s^N * (N+1)^t1 * c_b^{-e} mod N^2`
4. `w_test = h1^t1 * h2^t2 * t^{-e} mod N_tilde`
5. `e_test == self.e` kontrol
6. MtAwc icin ek: `G*s1 == X*e + u`

**BobProofExt Struct** (`range_proofs.rs`, satir 491-495):
```rust
pub struct BobProofExt {
    proof: BobProof,
    u: Point<Secp256k1>,  // G * alpha (randomness taahhut)
}
```

**SampleFromMultiplicativeGroup trait** (`range_proofs.rs`, satir 537-557):
```rust
pub trait SampleFromMultiplicativeGroup {
    fn from_modulo(N: &BigInt) -> BigInt;        // gcd(r, N) = 1 olana dek ornekle
    fn from_paillier_key(ek: &EncryptionKey) -> BigInt;
}
```
Bu trait Paillier N modulus'una gore Z*_N'den ornekleme saglar.

---

## 3. ZK-PDL (Paillier Discrete Logarithm)

### 3.1 PDLProof (Interaktif Protokol, zk_pdl/mod.rs)

**Amac ve Statement** (`zk_pdl/mod.rs`, satir 18-22):
```
Statement: (c, pk, Q, G)
Witness:   (x, r, sk)
sucht that: Q = x*G,  c = Enc(pk, x; r),  Dec(sk, c) = x
Kisitlama: x < q/3  (range proof sınırı)
```

**Neden PDL Gerekli?**
GG18 protokolunde her parti Paillier sifrelenmis gizli anahtarini paylasir. Diger partiler `c = Enc(x)` ve `Q = G*x` aldiktan sonra, bunlarin ayni `x` degerine karsilik geldigini dogrulamalidir. Aksi halde bir saldirgan farkli bir `x` sifreleyip farkli bir `Q` bildirebilir.

**Yapilar:**

```rust
pub struct PDLStatement {
    pub ciphertext: BigInt,         // c = Enc(pk, x)
    pub ek: EncryptionKey,          // Paillier public key
    pub Q: Point<Secp256k1>,        // G*x (EC public key)
    pub G: Point<Secp256k1>,        // Generator
}

pub struct PDLWitness {
    pub x: Scalar<Secp256k1>,       // gizli anahtar
    pub r: BigInt,                  // Paillier randomness
    pub dk: DecryptionKey,          // Paillier decryption key
}

pub struct PDLVerifierState {
    pub c_tag: BigInt,              // Verifier'in challenge cifresi
    pub c_tag_tag: BigInt,          // (a,b) commitments
    a: BigInt,                      // random scalar
    b: BigInt,                      // random integer < q^2
    blindness: BigInt,              // commitment randomness
    q_tag: Point<Secp256k1>,        // beklenen EC noktasi: a*Q + b*G
    c_hat: BigInt,                  // Prover'in commitment (prover mesaj 1'den)
}

pub struct PDLProverState {
    pub decommit: PDLProverDecommit, // q_hat ve blindness
    pub alpha: BigInt,               // Dec(c_tag) degeri
}
```

**Protokol Adimlari (Interaktif, 3 Tur):**

**Tur 1 - Verifier mesaji** (`zk_pdl/mod.rs`, satir 111-148):
```
a   <- Z_q (random scalar)
b   <- Z_{q^2}
c_tag = a * c + Enc(b)   (Paillier: a*Enc(x) + Enc(b) = Enc(a*x + b))
ab_concat = a + b << bit_len(a)
blindness <- Z_q
c_tag_tag = Hash(ab_concat; blindness)   (commitment to (a,b))
q_tag = a*Q + b*G  (beklenen sonuc: (a*x+b)*G)
```

**Tur 1 - Prover cevabi** (`zk_pdl/mod.rs`, satir 190-213):
```
alpha = Dec(c_tag) = a*x + b   (Paillier cozme)
q_hat = alpha * G              (alpha'nin EC karsiligi)
blindness <- Z_q
c_hat = Hash(q_hat; blindness) (commitment to q_hat)
range_proof = RangeProofNi::prove(ek, q, ciphertext, x, r)  (paralel)
```
Prover `(c_hat, range_proof)` gonderir.

**Tur 2 - Verifier decommit** (`zk_pdl/mod.rs`, satir 150-168):
```
Verifier range_proof dogrular: verify_range_proof(statement, range_proof)
Basarisizsa: Err(ZkPdlError::Message2)
Basarisizsa decommit gonderilmez!
Verifier (a, b, blindness) gonderir.
```

**Tur 2 - Prover dogrulama ve decommit** (`zk_pdl/mod.rs`, satir 216-241):
```
ab_concat_test = a + b << bit_len(a)
c_tag_tag_test = Hash(ab_concat_test; blindness)
alpha_test = a*x + b
kontrol: alpha_test == state.alpha  ve  c_tag_tag == c_tag_tag_test
Basarisizsa: Err(ZkPdlError::Message2)
Prover q_hat ve blindness gonderir (decommit)
```

**Finalize - Verifier dogrulama** (`zk_pdl/mod.rs`, satir 170-187):
```
c_hat_test = Hash(q_hat; blindness)
kontrol 1: c_hat == c_hat_test         (commitment opening)
kontrol 2: q_hat == state.q_tag        (alpha*G == a*Q + b*G)
```

**Guvenlik aciklamasi:**
- `q_tag = a*Q + b*G = a*(x*G) + b*G = (a*x+b)*G = alpha*G` olmalidir
- `c_tag_tag` commitment onceden gonderilir, boylece Verifier sonucu degistiremez
- Range proof `x < q/3` sinirini sağlar (PDL sesi sadece bu aralikta gecerli)

**Kritik limit** (`zk_pdl/mod.rs`, satir 21): `x < q/3` olmali. Test dosyasinda (`zk_pdl/test.rs`, satir 20-21):
```rust
let x = Scalar::from(&x.to_bigint().div_floor(&BigInt::from(3)));
```
x kasitli olarak 3'e bolunuyor.

### 3.2 PDLwSlackProof (Non-Interaktif, zk_pdl_with_slack/mod.rs)

**Amac ve Statement** (`zk_pdl_with_slack/mod.rs`, satir 18-23):
```
Statement: (c, pk, Q, G, h1, h2, N_tilde)
Witness:   (x, r) such that Q = x*G, c = Enc(pk, x; r)
Slack:     x in [-q^3, q^3]   (genis aralik, daha zayif kisitlama)
```

**Fark: Statement'a h1, h2, N_tilde eklendi (Pedersen setup):**
```rust
pub struct PDLwSlackStatement {
    pub ciphertext: BigInt,
    pub ek: EncryptionKey,
    pub Q: Point<Secp256k1>,
    pub G: Point<Secp256k1>,
    pub h1: BigInt,      // YENİ: Pedersen generator
    pub h2: BigInt,      // YENİ: Pedersen generator (h2 = h1^xhi)
    pub N_tilde: BigInt, // YENİ: Pedersen modulus (ayri RSA modulus)
}

pub struct PDLwSlackWitness {
    pub x: Scalar<Secp256k1>,  // gizli anahtar
    pub r: BigInt,             // Paillier randomness
    // NOT: dk (decryption key) yok! Interaktif versiyondan fark.
}
```

**`PDLwSlackProof` struct** (`zk_pdl_with_slack/mod.rs`, satir 56-65):
```rust
pub struct PDLwSlackProof {
    z: BigInt,            // Pedersen commitment: h1^x * h2^rho mod N_tilde
    u1: Point<Secp256k1>, // G * alpha  (EC commitment)
    u2: BigInt,           // Paillier(alpha): (N+1)^alpha * beta^N mod N^2
    u3: BigInt,           // Pedersen(alpha): h1^alpha * h2^gamma mod N_tilde
    s1: BigInt,           // e*x + alpha
    s2: BigInt,           // r^e * beta mod N
    s3: BigInt,           // e*rho + gamma
}
```

**Prove** (`zk_pdl_with_slack/mod.rs`, satir 68-125):
```
alpha <- Z_{q^3}
beta  <- Z*_N  (1 <= beta <= N-1)
rho   <- Z_{q * N_tilde}
gamma <- Z_{q^3 * N_tilde}

z  = h1^x  * h2^rho   mod N_tilde          (commitment to x)
u1 = G * alpha                              (EC commitment to alpha)
u2 = (N+1)^alpha * beta^N  mod N^2         (Paillier enc of alpha)
u3 = h1^alpha * h2^gamma mod N_tilde       (commitment to alpha)

e  = SHA256(G || Q || ciphertext || z || u1 || u2 || u3)   [Fiat-Shamir]

s1 = e*x + alpha
s2 = r^e * beta  mod N
s3 = e*rho + gamma
```

`commitment_unknown_order` yardimci fonksiyonu (`zk_pdl_with_slack/mod.rs`, satir 182-199):
```rust
fn commitment_unknown_order(h1, h2, N_tilde, x, r) -> BigInt {
    h1^x * h2^r mod N_tilde   // negatif r icin h2^(-1) kullanilir
}
```

**Verify** (`zk_pdl_with_slack/mod.rs`, satir 127-179):
```
e = SHA256(G || Q || ciphertext || z || u1 || u2 || u3)   [ayni hash]

u1_test = G*s1 - e*Q              (== G*(e*x+alpha) - e*(x*G) = G*alpha = u1)
u2_test = (N+1)^s1 * s2^N * c^{-e} mod N^2    (Paillier dogrulama)
u3_test = h1^s1 * h2^s3 * z^{-e} mod N_tilde  (Pedersen dogrulama)

kontrol: u1 == u1_test && u2 == u2_test && u3 == u3_test
```

---

## 4. Test Analizi

### 4.1 `mta/test.rs` — `test_mta()` (satir 5-19)

Tek test, temel MtA duzeltligini dogrular:
1. `alice_input`, `bob_input` rastgele secp256k1 scalar
2. `MessageA::a()` cagirilir (1 dlog_statement ile range proof da dahil)
3. `MessageB::b()` cagirilir — range proof dogrulanir, `beta` hesaplanir
4. `verify_proofs_get_alpha()` ile `alpha` hesaplanir
5. **Assert:** `alpha + beta == alice_input * bob_input`

Bu test tam MtA akisini dogrular. Onemli: `alice_input * bob_input` mod q olarak hesaplanir.

### 4.2 `range_proofs.rs` Tests (satir 559-710)

**`alice_zkp()` testi** (satir 616-633):
- Alice gizli deger + Paillier sifreler
- `AliceProof::generate()` + `verify()` dogrular
- Tek iterasyon

**`bob_zkp()` testi** (satir 637-709):
- 5x5 = 25 iterasyon (5 DLogStatement x 5 MtA kosusu)
- Hem `BobProof` (semi-honest, `check=false`) hem `BobProofExt` (with-check) test edilir
- Her ikisi icin de verify basarili olmali

**`generate_init()`** (satir 592-613) — Test ortami:
```
(ek_tilde, dk_tilde) = Paillier::keypair()
phi = (p-1)*(q-1)
h1 = rand < N_tilde
xhi = rastgele, mod_inv gecerli
h2 = h1^xhi mod N_tilde
DLogStatement { g: h1, ni: h2, N: N_tilde }
(ek, dk) = yeni Paillier keypair
```

### 4.3 `zk_pdl/test.rs` — `test_zk_pdl()` (satir 13-58)

Onemli: `x` kasitli olarak `x/3` yapiliyor (satir 20-21) cunku ispat yalnizca `x < q/3` icin gecerli.
Tam 3-tur interaktif protokol test ediliyor.

### 4.4 `zk_pdl_with_slack/test.rs` — Iki Test (satir 11-129)

**`test_zk_pdl_with_slack()`** (satir 11-68):
- `CompositeDLogProof` ile h1,h2,N_tilde setup'ini dogrular
- Rastgele `x` (range siniri YOK — slack sayesinde)
- `prove()` + `verify()` basarili olmali

**`test_zk_pdl_with_slack_soundness()`** (satir 70-129, `#[should_panic]`):
- `x` yerine `x+1` sifrelenyor (yanlış witness)
- Proof basarili uretiliyor (`x` ile) ama statement `c = Enc(x+1)` ile uyusmuyor
- `verify()` Err donuyor, `assert!(result.is_ok())` PANIC -> test gecer
- **Onemli: Bu bir soundness testi** — sahte witness ile ispat gecersiz olmali

**Test'teki Dikkat Cekici Yorum** (`zk_pdl_with_slack/test.rs`, satir 15-16, 35-36):
```rust
// note: safe primes should be used:
// let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();
```
Testte safe prime KULLANILMIYOR — bu production'da guvenlik acigi.

---

## 5. Guvenlik Degerlendirmesi

### 5.1 Guclu Yonler

1. **Fiat-Shamir donusumu dogru uygulanmis:** Hem AliceProof hem BobProof SHA-256 ile non-interactive — commit edilen tum degerler hash girdisine dahil edilmis.

2. **Zeroize kullaniliyor:** `AliceZkpRound1` ve `BobZkpRound1` `#[zeroize(drop)]` attribute'u tasiyor — hassas randomness bellekten silinir.

3. **Range bound dogrulamasi:** Her iki proofta `s1 <= q^3` ilk kontrol olarak yapiliyor — erken red mekanizması.

4. **Invertibility kontrol:** `z_e_inv`, `cipher_e_inv` icin `None` durumu dogruca `false` donuyor.

5. **Soundness testi var:** `test_zk_pdl_with_slack_soundness` sahte witness'in tespit edildigini gosterir.

### 5.2 Potansiyel Riskler ve Zayiflıklar

**Risk 1: Safe Prime Kullanilmamasi**
- `zk_pdl_with_slack/test.rs`, satir 15-16: `keypair_safe_primes()` yorumda kaliyor
- `range_proofs.rs`'deki `generate_init()` de normal `keypair()` kullaniyor
- **Etki:** N_tilde guvenli asal olmadan Pedersen commitment guvenlik parametresi duser
- **Siddet:** Orta-Yuksek — production'da zorunlu duzeltme

**Risk 2: PDL Range Siniri x < q/3**
- `zk_pdl/mod.rs`, satir 21: Acikca belirtiliyor
- Kullanicinin `x`'i kisitlamasi gerekiyor (test dosyasinda `div_floor(3)`)
- **Etki:** Yanlis kullanim durumunda ispat gecerli olmaz veya guvensiz olur

**Risk 3: BobProof'ta gamma Orneklemesi**
- `range_proofs.rs`, satir 9: `gamma <- [0, q^2 * N]` — spesifikasyondan sapma
- Orijinal makalede bu `q^3 * N` olmalidir
- **Etki:** Daha zayif range garantisi; ancak pratikte etkisi minimum olabilir

**Risk 4: Interaktif PDL'de Verifier Onceden Commit**
- `PDLVerifierFirstMessage`: `c_tag` ve `c_tag_tag` gonderiliyor
- Verifier `a,b` secip commit ediyor, boylece Prover cevabini degistiremez
- Bu dogru tasarim — ancak uygulamanin ag katmaninda mesaj siralamasi kritik

**Risk 5: MessageB'de Range Proof Yok**
- `MessageB` struct'i range proof icermiyor (sadece DLogProof)
- Bob'un `b` degeri icin range proof zorlanmiyor
- **Etki:** Semi-honest modelde sorun degil, ancak malicious modelde Bob buyuk `b` gonderebilir
- Mitigation: `BobProofExt` (MtAwc) bu durumu kismen ele aliyor

**Risk 6: verify_proofs_get_alpha'da EC Esitligi**
- `mta/mod.rs`, satir 169: `ba_btag = b_proof.pk * a + beta_tag_proof.pk`
- `b_proof.pk = G*b` ve `beta_tag_proof.pk = G*beta'`
- `ba_btag = a*G*b + G*beta' = G*(a*b + beta') = G*alpha` olmali
- Bu kontrol dogru ama **b_proof.pk * a** hesaplamasinda `a` Alice'in gizli verisi — EC skalar carpimi guvenlidir

### 5.3 PDL vs PDLwSlack Karsilastirmasi

| Ozellik | PDL (zk_pdl) | PDL w/ Slack (zk_pdl_with_slack) |
|---------|-------------|----------------------------------|
| Interaktiflik | 3 tur interaktif | Non-interaktif (Fiat-Shamir) |
| Range | x < q/3 (dar) | x in [-q^3, q^3] (genis) |
| Witness | (x, r, dk) — dk gerekli! | (x, r) — dk gereksiz |
| Pedersen Setup | Yok (yalnizca EC+Paillier) | h1, h2, N_tilde gerekli |
| Protokol referansi | eprint 2017/552, Protocol 6.1 | eprint 2016/013, PIi |
| Kullanim yeri | GG18 Phase2 | GG18/GG20 genel kullanim |
| Avantaj | Dar range, daha guclu | Asenkron, dk paylasilmiyor |

---

## 6. Nova Shield Chain Icin Cikarsimlar

### 6.1 MtA Implementasyonu

**Kullan:** `MessageA` + `MessageB` yapisi GG18 imzalama protokolu icin. Nova Shield Chain kendi MPC imzalama modulu yaziyorsa:
- MtA: `alpha + beta = k * gamma` seklinde kullanilir (signing round 2)
- Semi-honest icin: `MessageB` yeterli
- Malicious modelde: `BobProofExt` (MtAwc) ZORUNLU

**Kritik:** `verify_proofs_get_alpha` cagirilmadan `alpha` kullanilmamali.

### 6.2 Range Proof Secimi

**AliceProof:** Paillier sifreli degerin kucuklugunu kanıtlar. Nova Shield'in MPC keygeninde her parti kendi Paillier sifrelenmis gizli anahtarini gonderirken Alice-tarafi range proof eklemeli.

**BobProof / BobProofExt:** MtA ciktilarinin gucerliligi icin. Malicious adversary senaryosunda `BobProofExt` (MtAwc) kullanilmali.

### 6.3 ZK-PDL Secimi

**PDL (interaktif):** Kullanmayin — interaktif olmasi ag gecikmesini 3 kere artirir.

**PDLwSlack (non-interaktif):** TERCIH EDIN. Avantajlari:
- Tek mesaj
- Decryption key paylasilmiyor (dk gereksiz)
- Fiat-Shamir ile asenkron calisiyor

**Zorunlu:** Safe prime Paillier keypair kullanin:
```rust
Paillier::keypair_safe_primes()  // test.rs satir 15-16'daki yorumu actirin
```

### 6.4 commitment_unknown_order

`zk_pdl_with_slack/mod.rs`, satir 182-199 — Bu fonksiyon Nova Shield Chain'in baska Pedersen commitment gerektiren modullerinde de kullanilabilir. Ozellikle negatif exponent destegi (satir 191-194 h2^{-|r|}) dikkat cekici ve diger ZKP'lerde gerekebilir.

### 6.5 Pratik Guvenlik Parametreleri

| Parametre | Deger | Kullanim |
|-----------|-------|---------|
| `q` | secp256k1 grup mertebesi (~256 bit) | Range siniri |
| `q^3` | ~768 bit | Range proof bound |
| `N`, `N^2` | Paillier (2048 bit tavsiye) | Sifreli metin modulus |
| `N_tilde` | Pedersen RSA modulus (2048 bit safe prime) | Commitment modulus |
| SHA-256 | 256 bit challenge | Fiat-Shamir |

**Tavsiye:** Paillier icin 2048 bit RSA, Pedersen icin safe-prime 2048 bit RSA.

### 6.6 Eksiklikler ve Dikkat Edilmesi Gerekenler

1. **Safe prime zorunlulugu:** Test kodundaki yorum satirlari production'da aktif edilmeli
2. **BobProof gamma sapması:** Orijinal spesifikasyona gore `q^3 * N` olmali; bu sapma izlenebilir
3. **PDL range:** Interaktif PDL kullanirsaniz `x < q/3` sinirini uygulamaniz gerekiyor
4. **Batch dogrulama yok:** Her proof ayri ayr dogrulanir — buyuk n party icin paralel verification performansi iyilestirilebilir
5. **`verify_b_against_public`** (`mta/mod.rs`, satir 204-209): Tek satir equality check — bu fonksiyonun amaci baska bir kanala ulasan G*b degerini MtA ciktisi ile karsilastirmaktir; GG18 abort identification icin kullaniliyor

---

*Analiz tamamlandi: 8 dosya, ~1200 satir. Referans: eprint 2019/114, 2017/552, 2016/013*
