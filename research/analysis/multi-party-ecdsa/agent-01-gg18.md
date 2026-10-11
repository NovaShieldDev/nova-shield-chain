# Agent-01: GG18 Multi-Party ECDSA Analizi

## Kapsanan Dosyalar

| Dosya | Satir Sayisi | Amac |
|-------|-------------|------|
| `src/protocols/multi_party_ecdsa/gg_2018/party_i.rs` | 737 | Temel protokol implementasyonu |
| `src/protocols/multi_party_ecdsa/gg_2018/mod.rs` | 21 | Modul tanimlari |
| `src/protocols/multi_party_ecdsa/gg_2018/test.rs` | 459 | Entegrasyon testleri |
| `examples/gg18_keygen_client.rs` | 271 | Gercek ag uzerinde keygen istemcisi |
| `examples/gg18_sign_client.rs` | 531 | Gercek ag uzerinde signing istemcisi |
| `examples/gg18_sm_manager.rs` | 143 | State machine yonetici sunucusu |
| `examples/common.rs` | 225 | AES-GCM, HTTP, P2P yardimci fonksiyonlari |
| `benches/multi_party_ecdsa/gg18/keygen.rs` | 135 | Keygen benchmark (t=1,n=2 ve t=2,n=3) |
| `src/protocols/multi_party_ecdsa/mod.rs` | 19 | `gg_2018` ve `gg_2020` modulleri |
| `src/lib.rs` | 47 | Crate root; `Error` enum tanimlari |
| `src/utilities/mta/mod.rs` | 214 | MtA (Multiplicative-to-Additive) implementasyonu |
| `src/utilities/mta/range_proofs.rs` | 710 | Alice/Bob ZK aralik ispatlar |

---

## 1. Protokol Genel Bakis (t-of-n Yapisi)

GG18, Gennaro-Goldfeder 2018 makalesi olan "Fast Multiparty Threshold ECDSA with Fast Trustless Setup" protokolunu uygular. Referans: `https://eprint.iacr.org/2019/114.pdf`.

### Parametreler

```
Parameters { threshold: t, share_count: n }
```

- **t**: esik (threshold). Imza icin en az **t+1** taraf gerekir.
- **n**: toplam taraf sayisi.
- Kodu acan: `party_i.rs:45-48`

### Onemli Kural

`t < t' <= n` olmali; imzalayan grup buyuklugu `t' = t+1` (tam esik) veya daha fazlasi olabilir. Test dosyasinda: `test.rs:173 - assert!(ttag > t)`.

### Iki Ana Asamasi

1. **Key Generation** (Distributed Key Generation, DKG): Her taraf kendi payini uretir, ortak acik anahtar `Y = sum(u_i * G)` hesaplanir.
2. **Signing** (Threshold Signing): `t+1` taraf biraraya gelerek bireysel `s_i` kismi imzalar uretir; bunlar toplanarak gecerli ECDSA imzasi olusur.

---

## 2. Key Generation: Round-by-Round Analiz

### Veri Yapilari (party_i.rs:51-83)

```rust
pub struct Keys {
    pub u_i: Scalar<E>,      // partinin gizli kismi (rastgele secp256k1 skalar)
    pub y_i: Point<E>,       // u_i * G — partinin acik anahtari
    pub dk: DecryptionKey,   // Paillier gizli anahtari
    pub ek: EncryptionKey,   // Paillier acik anahtari (yayinlanir)
    pub party_index: u16,
}

pub struct SharedKeys {
    pub y: Point<Secp256k1>,   // ortak acik anahtar Y = sum(y_i)
    pub x_i: Scalar<Secp256k1>, // partinin VSS payi: x_i = sum_j(f_j(i))
}
```

---

### Round 1 — Commit + Paillier Keyini Yayinla

**Fonksiyon:** `Keys::phase1_broadcast_phase3_proof_of_correct_key()` — `party_i.rs:190-209`

**Her taraf yaptiklarini:**
1. Rastgele `u_i <- Z_q` uretir (`create()` icinde: `party_i.rs:149`).
2. `y_i = u_i * G` hesaplar.
3. 256-bit rastgele `blind_factor` ornekler.
4. Pedersen-stili hash commitment olusturur:
   ```
   com = SHA256(y_i_compressed_bytes || blind_factor)
   ```
   Implementasyon: `HashCommitment::<Sha256>::create_commitment_with_user_defined_randomness` — `party_i.rs:195-198`
5. Paillier icin dogruluk kaniti: `NiCorrectKeyProof::proof(&self.dk, None)` — `party_i.rs:194`

**Broadcast mesaji (`KeyGenBroadcastMessage1`, `party_i.rs:67-71`):**
```
{ e: EncryptionKey, com: BigInt, correct_key_proof: NiCorrectKeyProof }
```

**Decommit mesaji (`KeyGenDecommitMessage1`, `party_i.rs:73-77`) — henuz gonderilmez:**
```
{ blind_factor: BigInt, y_i: Point<Secp256k1> }
```

---

### Round 2 — Decommit + Paillier Dogrulama + VSS Dagitimi

**Fonksiyon:** `Keys::phase1_verify_com_phase3_verify_correct_key_phase2_distribute()` — `party_i.rs:212-240`

**Her taraf yaptiklarini:**
1. Tum diger partilerin commitment'larini dogrular:
   ```
   SHA256(decom_vec[i].y_i || decom_vec[i].blind_factor) == bc1_vec[i].com
   ```
2. Tum diger partilerin Paillier anahtarlarinin dogru oldugunu dogrular:
   ```
   bc1_vec[i].correct_key_proof.verify(&bc1_vec[i].e, SALT_STRING)
   ```
3. Feldman VSS olusturur: `VerifiableSS::share(t, n, &self.u_i)` — `party_i.rs:233`
   - Derece `t` polinomu: `f(X) = u_i + a_1*X + ... + a_t*X^t`
   - `n` adet pay uretir: `s_j = f(j)` for j in 1..=n
4. VSS commitments: `C_k = a_k * G` for k in 0..=t

**Gonderim:** Kismi `s_j` payini sadece taraf `j`'ye gizlice gonderir (Point-to-Point).

Gercek implementasyonda (keygen_client.rs:139-154) gizli pay AES-GCM ile sifrelenerek HTTP uzerinden gonderilir. Sifreleme anahtari ECDH ile elde edilir:
```rust
let key_bn = (decom_j.y_i.clone() * party_keys.u_i.clone()).x_coord().unwrap();
```

---

### Round 3 — VSS Dogrulama + Paylasik Anahtar Olusturma + DLOG Kaniti

**Fonksiyon:** `Keys::phase2_verify_vss_construct_keypair_phase3_pok_dlog()` — `party_i.rs:242-269`

**Her taraf yaptiklarini:**
1. Kendi aldigim her `s_j[i]` payini dogrular:
   ```
   g^(s_j[i]) == sum_{k=0}^{t} (C_{j,k})^(i^k)
   ```
   Bu Feldman VSS dogrulama kosuludur.
2. Ayrica `vss_scheme_vec[i].commitments[0] == y_vec[i]` kontrolu: sabit terim commitimenti `y_i`'ye esit olmali.
3. Ortak acik anahtari hesaplar:
   ```
   Y = sum(y_i)   // party_i.rs:262
   ```
4. Kendi payini hesaplar:
   ```
   x_i = sum_j(s_j[i])  // party_i.rs:263
   ```
5. `x_i` icin DLOG kaniti: `DLogProof::prove(&x_i)` — Schnorr sigma protokolu.

**Broadcast:** `DLogProof` yayinlanir.

---

### Round 4 (Son Dogrulama)

**Fonksiyon:** `Keys::verify_dlog_proofs()` — `party_i.rs:295-311`

- Tum partilerin `x_i`'leri icin DLOG kanitlarini dogrular.
- Hepsi gecerliyse DKG tamamlanmistir.

**Kaydedilen Cikti (keygen_client.rs:254-263):**
```json
(party_keys, shared_keys, party_num_int, vss_scheme_vec, paillier_key_vec, y_sum)
```

---

### Keygen Round Ozeti

| Round | Gonderim Tipi | Icindekiler |
|-------|--------------|-------------|
| 1 | Broadcast | `com`, `ek`, `NiCorrectKeyProof` |
| 2 | Broadcast + P2P | Decommit `(blind, y_i)` + sifreli VSS paylar |
| 3 | Broadcast | VSS commitments (`vss_scheme`) |
| 4 | Broadcast | DLOG kaniti `DLogProof(x_i)` |

**Toplam: 4 broadcast + 1 P2P round**

---

## 3. Signing: Round-by-Round Analiz

### Veri Yapilari (party_i.rs:86-145)

```rust
pub struct SignKeys {
    pub w_i: Scalar<Secp256k1>,     // Lagrange katsayili pay: w_i = lambda_i * x_i
    pub g_w_i: Point<Secp256k1>,    // g^w_i (kamuya acik)
    pub k_i: Scalar<Secp256k1>,     // rastgele nonce kismi
    pub gamma_i: Scalar<Secp256k1>, // rastgele maske skalar
    pub g_gamma_i: Point<Secp256k1>,// g^gamma_i
}
```

### Signing Oncesi Hazirlik

**Fonksiyon:** `SignKeys::create()` — `party_i.rs:385-406`

```
lambda_i = Lagrange(index, s)   // Lagrange interpolasyon katsayisi
w_i = lambda_i * x_i            // agirliklandirilmis pay
gamma_i <- Z_q  (rastgele)
k_i <- Z_q      (rastgele)
```

---

### Round 1 — Commit to gamma_i + k_i Sifreleme

**Fonksiyon:** `SignKeys::phase1_broadcast()` — `party_i.rs:408-424`

```
com = SHA256(g^gamma_i || blind)
```

Ayrica `MessageA` olusturulur: `k_i` Paillier ile sifrelenir:
```
c_a = Enc_{ek_i}(k_i, r)
```
(sign_client.rs:111: `MessageA::a(&sign_keys.k_i, &party_keys.ek, &[])`)

**Broadcast:** `(com, m_a_k)` — hem commitment hem sifreli `k_i`

---

### Round 2 — MtA Protokolu (En Kritik Adim)

**Her taraf cift yonlu MtA calistirir (her cift icin).**

**MessageB olusturulmasi (`mta/mod.rs:91-158`):**

Bob (j. taraf), Alice'in (i. tarafin) `c_a = Enc(k_i)` sifresini alir ve su islemleri yapar:
```
beta_tag <- Z_N     (rastgele maskeleme degeri)
c_b = Enc(k_i)^gamma_j * Enc(beta_tag)
    = Enc(k_i * gamma_j + beta_tag)   // Paillier homojenlik ozelligi
beta = -beta_tag mod q
```

Alice `c_b`'yi dekriple edince elde eder:
```
alpha_ij = k_i * gamma_j + beta_tag mod q
```

Dolayisiyla: `alpha_ij + beta_ij = k_i * gamma_j`

**delta_i hesabi** — `phase2_delta_i()` — `party_i.rs:426-434`:
```
delta_i = k_i * gamma_i + sum_j(alpha_ij + beta_ij)
        = k_i * gamma_i + sum_j(k_i * gamma_j)     [modulo q]
        = k_i * (gamma_i + sum_j(gamma_j))
```

Tum partilerin delta toplami:
```
sum_i(delta_i) = sum_i(k_i) * sum_i(gamma_i) = k * gamma   [mod q]
```

**sigma_i hesabi** — `phase2_sigma_i()` — `party_i.rs:436-444` — ayni sekilde w_i ile:
```
sigma_i = k_i * w_i + sum_j(miu_ij + ni_ij)
sum_i(sigma_i) = k * x   [mod q]  // x = ozel anahtar
```

**MtA Gozlemi:** `k` ve `gamma` (veya `x`) hicbir zaman acik gonderilmez; sadece aralarindaki carpim toplamlar uzerinden rekombine edilir.

**P2P gonderim:** `(m_b_gamma, m_b_w)` her taraf ciftine.

---

### Round 3 — delta_i Yayini + delta Inversion

**Fonksiyon:** `SignKeys::phase3_reconstruct_delta()` — `party_i.rs:446-452`

```
delta = sum_i(delta_i) = k * gamma   [mod q]
delta_inv = (k * gamma)^(-1) mod q
```

**Broadcast:** `delta_i`

---

### Round 4 — gamma_i Decommit + R Hesabi

**Fonksiyon:** `SignKeys::phase4()` — `party_i.rs:454-483`

1. Tum `g^gamma_i` degerleri commitment acilarak dogrullanir.
2. MtA'daki `b_proof` (DLogProof for gamma_i) ile tutarlilik kontrolu: `b_proof_vec[i].pk == phase1_decommit_vec[i].g_gamma_i`
3. R hesabi:
   ```
   Gamma = sum_i(g^gamma_i) = g^gamma
   R = delta_inv * Gamma = (k*gamma)^(-1) * g^gamma = g^(1/k)
   r = R.x mod q
   ```

**Broadcast:** `SignDecommitPhase1 { blind_factor, g_gamma_i }`

---

### Round 5 — Kismi Imza Hesabi + Phase5 ZK Kaniti

**Fonksiyon:** `LocalSignature::phase5_local_sig()` — `party_i.rs:487-511`

```
m_fe = hash(message) mod q
s_i = m_fe * k_i + r * sigma_i
```

Bu degeri hemen gondermez. Once Phase5 ZK kanitlari yapilir.

**Fonksiyon:** `LocalSignature::phase5a_broadcast_5b_zkproof()` — `party_i.rs:513-558`

- `rho_i`, `l_i` <- rastgele
- `V_i = R^s_i * g^l_i`
- `A_i = g^rho_i`
- `B_i = g^(l_i * rho_i)`
- HomoElGamal ZK kaniti: `D = R^x * g^r` formunda
- DLOG kaniti: `rho_i` icin

**Broadcast:** `Phase5Com1 { com = SHA256(V_i, A_i, B_i || blind) }`

---

### Round 6 — Phase5 Decommit + Dogrulama

**Fonksiyon:** `LocalSignature::phase5c()` — `party_i.rs:561-636`

1. Tum `Phase5ADecom1 { V_i, A_i, B_i }` acimlari commitment'larla eslestirilir.
2. HomoElGamal kanitlari dogrulanir.
3. DLOG kanitlari dogrulanir.
4. Duzeltilmis toplam hesabi:
   ```
   V = sum(V_i) - m*G - r*Y
     = sum(R^s_i * g^l_i) - m*G - r*Y
   ```
   Eger toplam imza dogru olsaydi: `sum(s_i) = m*k^(-1) + r*x*k^(-1)` ve `R = g^(k^(-1))` => V = g^0 = kimlik (teorik olarak)
5. Dogrulama gereksinimi: `u_i = V * rho_i`, `t_i = A * l_i`

**Broadcast:** `Phase5Com2`

---

### Round 7 — Phase5d Dogrulama

**Fonksiyon:** `LocalSignature::phase5d()` — `party_i.rs:638-673`

Kontrol:
```
G + sum(t_i) + sum(B_i) - sum(u_i) == G
=> sum(t_i + B_i) == sum(u_i)
```
Bu kontrol `s_i` degerlerinin tutarliligini garanti eder. Eger gecerse: `Ok(self.s_i.clone())`

---

### Round 8 — s_i Toplama + Final Imza

**Fonksiyon:** `LocalSignature::output_signature()` — `party_i.rs:674-711`

```
s = sum(s_i) = sum(m*k_i + r*sigma_i)
             = m * sum(k_i) + r * sum(sigma_i)
             = m * k + r * x   (mod q k^(-1) cinsinden: m/k + r*x/k)
```

**Onemli:** Burada `k_i`'ler dogrudan toplanmaz, MtA'daki `s_i = m*k_i + r*sigma_i` toplami alinir. Bu dogru sonucu verir cunku:
- `sigma_i = k_i * w_i + MtA_terms` => `sum(sigma_i) = k * x`
- Dolayisiyla `s = m * k_inv + r * x * k_inv = (m + r*x) / k` [ECDSA formati]

**s normalizasyonu:** Eger `s > q/2` ise `s = q - s` (dusuk-s normalizasyonu, malleability onlemi):
```rust
// party_i.rs:699-703
let s_tag_bn = Scalar::<Secp256k1>::group_order() - &s_bn;
if s_bn > s_tag_bn {
    s = Scalar::<Secp256k1>::from(&s_tag_bn);
    recid ^= 1;
}
```

**Recovery ID (recid) hesabi** — `party_i.rs:697-703`:
```
recid = R.y & 1
if s > q/2: recid ^= 1
```

**Imza dogrulama:** Uretilen imza hemen `verify()` ile dogrulanir: `party_i.rs:705`

---

### Signing Round Ozeti

| Round | Gonderim | Icindekiler |
|-------|---------|-------------|
| 0 | Broadcast | Imzaci kimlik numaralari |
| 1 | Broadcast | `(com_gamma_i, Enc(k_i))` |
| 2 | P2P | `(m_b_gamma, m_b_w)` — MtA cevaplari |
| 3 | Broadcast | `delta_i` |
| 4 | Broadcast | `decommit_gamma_i` |
| 5 | Broadcast | `Phase5Com1` |
| 6 | Broadcast | `(Phase5ADecom1, HomoElGamalProof, DLogProof_rho)` |
| 7 | Broadcast | `Phase5Com2` |
| 8 | Broadcast | `Phase5DDecom2` |
| 9 | Broadcast | `s_i` |

**Toplam: 9 broadcast round + 1 P2P round**

---

## 4. Kriptografik Primitive'ler

### 4.1 Eliptik Egri

- **Egri:** `Secp256k1` — `party_i.rs:30`
- Buyuk ozel eleman: `q` (grup mertebesi)
- `SECURITY: usize = 256` — `party_i.rs:42`

### 4.2 Paillier Sifreleme

- **Amac:** MtA protokolunde `k_i` ve benzerlerini sifreleme; homojen carpma/toplama saglar.
- **Ozellik:** `Enc(a) * b = Enc(a*b)`, `Enc(a) + Enc(c) = Enc(a+c)` (homomorfizm)
- **Anahtar boyutu:** Normal: `Paillier::keypair()`, Uretim icin onerilen: `Paillier::keypair_safe_primes()` — `party_i.rs:162-167`
- Dipnot: `party_i.rs:162 — "we recommend using safe primes if the code is used in production"`
- Kullanilan crate: `paillier` (`Encrypt, Decrypt, Mul, Add` trait'leri)

### 4.3 Feldman VSS (Verifiable Secret Sharing)

- **Amac:** `u_i`'yi `n` parca halinde dagitmak; herhangi `t+1` payi toplama ile `u_i` geri kazanilabilir.
- **Polinom:** `f(X) = u_i + a_1*X + ... + a_t*X^t`, derece = `t`
- **Commitments:** `C_k = a_k * G` for k in 0..=t; `C_0 = y_i`
- **Dogrulama:** `g^(s_j) == sum_k(C_k^(j^k))` — `party_i.rs:255-259`
- Kullanilan crate: `curv::cryptographic_primitives::secret_sharing::feldman_vss::VerifiableSS`

### 4.4 Hash Commitment (Pedersen-stili)

- `com = SHA256(value || blind_factor)` — binding ve hiding
- `blind_factor = BigInt::sample(256)` — 256 bit rastgele
- Kullanilan: `HashCommitment::<Sha256>` — hem keygen hem signing icin

### 4.5 DLOG Kaniti (Schnorr Sigma Protokolu)

- **Amac:** `x_i` bilgisini ifsa etmeden `g^x_i` icin bilgi kaniti.
- Non-interactive (Fiat-Shamir): `DLogProof::prove(&x_i)` — `party_i.rs:264`
- Kullanilan: `curv::cryptographic_primitives::proofs::sigma_dlog::DLogProof`

### 4.6 NiCorrectKeyProof

- **Amac:** Paillier anahtarinin dogru ve kotu bicimde olusturulmamis oldugunu kanitlar.
- Non-interactive: `NiCorrectKeyProof::proof(&self.dk, None)` — `party_i.rs:194`
- Kullanilan: `zk_paillier::zkproofs::NiCorrectKeyProof`

### 4.7 MtA Aralik Ispatlari (Range Proofs)

Kaynak: `utilities/mta/range_proofs.rs` — Appendix A of eprint.iacr.org/2019/114.pdf

**Alice Proof (AliceProof):**
- Alice, `a in [0, q]` oldugunu ve `c_a = Enc(a)` oldugunu ispat eder.
- Kurulum: `h1, h2, N_tilde` (diger tarafin DLogStatement'i) gerekir.
- Round1: `z = h1^a * h2^ro mod N_tilde`, `u = Enc(alpha)`, `w = h1^alpha * h2^gamma mod N_tilde`
- Fiat-Shamir challenge: `e = SHA256(N, Gen, cipher, z, u, w)`
- Round2 yaniti: `s = r^e * beta mod N`, `s1 = e*a + alpha`, `s2 = e*ro + gamma`
- Dogrulama: `s1 < q^3` kontrolu dahil — `range_proofs.rs:118`

**Bob Proof (BobProof / BobProofExt):**
- Bob, `c_b = Enc(a*b + beta)` oldugunu ispat eder (MtA ciktisi).
- Extended versiyonda ek olarak `B = g^b` bilgisi eklenir (MtAwc).
- `BobProofExt.verify()` icinde EC nokta esitligi: `g^s1 == X^e * u` — `range_proofs.rs:522-530`

**KRITIK NOT:** GG18 implementation'inda (test.rs:191-194):
```rust
// range proofs are ignored here, as there's no h1, h2, N_tilde setup in this version of GG18
let m_a_vec: Vec<_> = sign_keys_vec.iter().enumerate()
    .map(|(i, k)| MessageA::a(&k.k_i, &party_keys_vec[usize::from(s[i])].ek, &[]).0)
    .collect();
```
Bos `dlog_statements = &[]` ile cagrilir, yani aralik ispatlari DEVRE DISI. Bu onemli bir guvenlik eksikligidir.

### 4.8 HomoElGamal Kaniti

- `V_i = R^s_i * g^l_i`, `B_i = A_i^l_i` icin bilgi kaniti.
- `HomoELGamalProof::prove(&witness, &delta)` — `party_i.rs:546`
- Kullanilan: `curv::cryptographic_primitives::proofs::sigma_correct_homomorphic_elgamal_enc`

### 4.9 Centipede (Verifiable Recovery)

- `PartyPrivate::to_encrypted_segment()` — `party_i.rs:361-369`
- `Msegmentation::to_encrypted_segments()` kullanilarak `u_i` geri kazanilabilir sekilde sifrelenir.

---

## 5. Struct ve Veri Yapilari

### Keys (`party_i.rs:51-57`)
Keygen asamasinda olusturulan ham anahtar verisi.
- `u_i`: Gizli parca. Hicbir zaman paylasılmaz.
- `y_i = u_i * G`: Acik anahtar parcasi. Round1'de commitment, Round2'de decommit ile yayinlanir.
- `dk, ek`: Paillier anahtar cifti. `ek` broadcast edilir.

### PartyPrivate (`party_i.rs:60-64`)
Signing icin gizli durum. Hem `u_i` hem `x_i` (VSS payi) tutar.
- `set_private(key, shared_key)` ile olusturulur.
- `decrypt()`: Paillier dekripsiyonu; MtA'da alpha hesaplamak icin.
- `refresh_private_key()`: Key rotation (yeni Paillier keypair ile).

### SharedKeys (`party_i.rs:80-83`)
Keygen sonrasi paylasilan anahtar bilgisi.
- `y`: Ortak acik anahtar.
- `x_i`: Partinin VSS toplam payi.

### SignKeys (`party_i.rs:86-92`)
Signing asamasinin gizli durumu.
- `w_i = lambda_i * x_i`: Lagrange agirlikli pay.
- `k_i`: Nonce'un partiye ait parcasi.
- `gamma_i`: Maskeleme skalari.

### LocalSignature (`party_i.rs:106-113`)
Phase5 sonrasi hesaplanan kismi imza bilgisi.
- `l_i, rho_i`: Randomness Phase5 ZK kaniti icin.
- `s_i = m*k_i + r*sigma_i`: Kismi imza.
- `R`: Nonce noktasi.

### SignatureRecid (`party_i.rs:141-145`)
Final ECDSA imzasi.
- `r`: R.x mod q
- `s`: Toplam s (normalized)
- `recid`: Recovery ID (0 veya 1)

### MessageA (`mta/mod.rs:35-38`)
MtA'nin Alice tarafi. `c = Enc(k_i)`.

### MessageB (`mta/mod.rs:41-45`)
MtA'nin Bob tarafi.
- `c = Enc(k_i * gamma_j + beta_tag)`: Homojen sifreli carpim.
- `b_proof`: Bob'un skalari icin DLOG kaniti.
- `beta_tag_proof`: Maskeleme degeri icin DLOG kaniti.

---

## 6. Test Analizi

### test.rs: keygen_t_n_parties() (satir 57-161)

Saf Rust icinde DKG simulasyonu (ag yok). Parametreler:
- `test_keygen_t1_n2`: t=1, n=2
- `test_keygen_t2_n3`: t=2, n=3
- `test_keygen_t2_n4`: t=2, n=4

**Sonuc dogrulama (test.rs:146-152):**
```rust
let x = vss_scheme_for_test[0].clone()
    .reconstruct(&index_vec[0..=usize::from(t)], &xi_vec);
let sum_u_i = party_keys_vec.iter()
    .fold(Scalar::zero(), |acc, x| acc + &x.u_i);
assert_eq!(x, sum_u_i);
```
Bu VSS'in dogru calistigini ve `x = sum(u_i)` olduğunu dogrular.

### test.rs: sign() (satir 163-403)

- `test_sign_n5_t2_ttag4`: n=5, t=2, imzalayanlar=[0,2,3,4]
- `test_sign_n8_t4_ttag6`: n=8, t=4, imzalayanlar=[0,1,2,4,6,7]

**Imza dogrulama (test.rs:401-403):**
```rust
verify(&sig, &local_sig_vec[0].y, &local_sig_vec[0].m).unwrap();
check_sig(&sig.r, &sig.s, &local_sig_vec[0].m, &y);  // secp256k1 kutuphanesiyle
```

Mesaj ornegi: `[79, 77, 69, 82]` ("OMER"), SHA256 ile hashlenerek kullanilir — test.rs:328-331.

### test.rs: test_serialize_deserialize() (satir 446-459)

- `KeyGenBroadcastMessage1` ve `KeyGenDecommitMessage1` icin serde serialization testi.
- `commit.com` ve `decommit.y_i` esitlik kontrolleri.

### bench/keygen.rs

- `sample_size(10)` ile 10 ornekle benchmark.
- `bench_full_keygen_party_one_two`: t=1, n=2
- `bench_full_keygen_party_two_three`: t=2, n=3

---

## 7. Guvenlik Degerlendirmesi

### 7.1 Guvenligi Saglayan Mekanizmalar

| Mekanizma | Amaç | Kaynak |
|-----------|------|--------|
| Pedersen Hash Commitment | `y_i` ve `g^gamma_i` erken aciklamasini engeller | party_i.rs:195,412 |
| NiCorrectKeyProof | Paillier anahtarinin kusurlu uretilmedigini kanitlar | party_i.rs:194 |
| Feldman VSS | Pay dagitiminin tutarliligini kanitlar | party_i.rs:233,254 |
| DLOG Proof of Knowledge | `x_i` sahipligini ispat eder | party_i.rs:264 |
| MtA + Range Proofs | k_i'nin kucuk oldugunu kanitlar (maalesef devre disi) | mta/range_proofs.rs |
| HomoElGamal Proof | s_i'nin dogru hesaplandığini kanitlar | party_i.rs:546 |
| Phase5 ZK | Kismi imzanin onceden bilinen formda oldugunu garanti eder | party_i.rs:513-673 |
| s normalizasyonu | Signature malleability onlemi | party_i.rs:699-703 |

### 7.2 Bilinen Zaafiyetler ve Uyarilar

**KRITIK - Range Proofs Eksikligi:**
- Kod yorumunda acikca belirtilmis (test.rs:190-191):
  ```
  // range proofs are ignored here, as there's no h1, h2, N_tilde setup
  ```
- `MessageA::a(k_i, ek, &[])` — bos `dlog_statements` geciliyor.
- Bu, `k_i` aralik kontrolu olmadan MtA yapildigini anlamina gelir.
- Potansiyel saldirganlara `k_i`'nin kucuk olmadigini ispat etme mecburiyeti olmadan hareket etme imkani verir.
- **GG20, bu problemi komsuluk isaretleme (co-efficient proofs) ile cozer.**

**Normal vs Safe Prime Paillier:**
- `Keys::create()` `keypair()` kullanir — normal asal sayilar.
- `Keys::create_safe_prime()` vardir ama varsayilan degildir.
- Safe prime olmadan bazi Paillier saldırilarina karsi zayif olabilir.
- Kod yorumu: `party_i.rs:162 — "we recommend using safe primes if the code is used in production"`

**MtA Bob Kaniti Eksikligi:**
- Mevcut `MessageB` yapisi sadece `b_proof` ve `beta_tag_proof` icerir (DLog kaniti).
- Tam `BobProof` aralik ispati kullanilmiyor.
- Bob, `c_b` hesabini gizli verileri sifirmek icin kotu sekilde kullanabilir.

**Abort Protokolu Eksik:**
- Hicbir taraf kimlik tespiti yapamaz (who aborted?).
- `phase5c` `InvalidCom` dondurebilir ama suclanan taraf belli degildir.

**State Machine Guvenligi:**
- `gg18_sm_manager.rs` in-memory `HashMap` kullanir — yeniden baslama durumunda kayip.
- `RwLock<HashMap>` multi-thread guvenli ama dayanikli degil.

### 7.3 Guvenlik Modeli

- **Honesty Assumption:** `t` adet kotu niyetli taraf varsayilir (`t-of-n`'den kotu: `t` veya daha az bozulabilir).
- **Gizlilik:** Herhangi `t` adet tarafin bilgileri birlestirilse bile `x` aciklanamaz.
- **Dogru Imza:** `t+1` taraf isbirligi yaparsa imza uretilir.
- **Referans:** GG18 makalesi UC (Universal Composability) guvenlik modeli iddiasinda bulunur ama tam UC kaniti bazi varsayimlara baglidir.

---

## 8. GG18 vs GG20 Karsilastirmasi

| Ozellik | GG18 (`gg_2018`) | GG20 (`gg_2020`) |
|---------|-----------------|-----------------|
| Yayin | 2018 (eprint 2019/114) | 2020 (eprint 2020/540) |
| Signing round sayisi | ~9 round | ~4 round |
| Range proofs | Orijinal (ag overhead) | Gelismis, daha verimli |
| MtA protokolu | Classic (MtA + MtAwc) | Optimized |
| Key refresh | Destekleniyor (refresh_private_key) | Daha iyi destekleniyor |
| Guvenlik | UC, ama eksik aralik ispatlar | Duzeltilmis, daha guclu |
| Abort identification | Yok | Var |
| Kod durumu | Dogru, orijinal referans | Aktif olarak onerilen |

**Neden GG20 tercih edilmeli:**
1. Daha az round = daha dusuk latency.
2. Aralik ispatlari varsayilan olarak aktif.
3. Abort identification protokolu var.
4. Daha modern kriptografik protokoller.

**Bu repoda:** `src/protocols/multi_party_ecdsa/mod.rs:17-18` — ikisi de mevcut:
```rust
pub mod gg_2018;
pub mod gg_2020;
```

---

## 9. Nova Shield Chain Icin Cikarsimlar

### 9.1 Mimari Secim: GG20 Kullan

- GG18 referans olarak degerlidir ancak uretim icin GG20 kullanilmalidir.
- `research/libs/multi-party-ecdsa/src/protocols/multi_party_ecdsa/gg_2020/` analiz edilmeli (agent-02).

### 9.2 Paillier Anahtar Boyutu

- Safe prime ile `keypair_safe_primes()` zorunlu tutulmali.
- Uretim ortaminda normal `keypair()` KULLANILMAMALI.
- Paillier anahtar boyutu minimum 2048-bit olmali (secp256k1 guvenlik seviyesini asmak icin).

### 9.3 Range Proof Altyapisi

- `h1, h2, N_tilde` kurulum asamasini eklemek gerekir.
- Her tarafin `DLogStatement` uretip exchange etmesi zorunlu.
- Bu olmadan MtA guvenli degildir.

### 9.4 VSS Parametreleri

- `t < n/2` olmali (dürüst cogunluk varsayimi).
- Ornek konfigurasyonlar:
  - 2-of-3: t=1, n=2 veya t=2, n=3 (ikinci yaygin)
  - 3-of-5: t=2, n=5
  - 5-of-9: t=4, n=9 (yuksek guvenlik)

### 9.5 AES-GCM P2P Sifreleme

- VSS paylarinin P2P iletimi AES-256-GCM ile sifreleniyor (common.rs:53-69).
- Nonce 12 byte, rastgele uretiliyor (OsRng).
- Sifreleme anahtari ECDH: `(y_j * u_i).x_coord()` — common.rs yokunda keygen_client.rs:114-119.
- Bu yaklasim Nova Shield icin iyi bir referans: Paillier public key ile ECDH key agreement.

### 9.6 State Machine Tasarimi

- `gg18_sm_manager.rs` basit bir HTTP bazli state machine yoneticisi (Rocket framework).
- Uretim icin: P2P mesajlasma, libp2p ile entegrasyon.
- Her round icin mesaj toplama: `poll_for_broadcasts()` polling yapiyor, push-based tercih edilmeli.

### 9.7 Imza Malleability

- S normalizasyonu implementasyonda mevcut: `party_i.rs:699-703`.
- Nova Shield icin bu zorunlu — Bitcoin/Ethereum uyumlulugu.
- `recid` hesabi da mevcut — Ethereum `v` degeri icin kullanilabilir.

### 9.8 Kritik Matematiksel Degerler

```
SECURITY = 256 bits                          // party_i.rs:42
Paillier N ~ 2048 bits                       // onerilen minimum
Curve: secp256k1, q = 2^256 - 2^32 - 977
AliceProof: alpha < q^3, gamma < q^3 * N_tilde
BobProof:   alpha < q^3, gamma < q^2 * N, tau < q^3 * N_tilde
Lagrange: lambda_i = prod_{j!=i}(j/(j-i)) mod q
```

### 9.9 Centipede Verifiable Recovery

- `PartyPrivate::to_encrypted_segment()` (party_i.rs:361-369) ile anahtar kurtarma destekleniyor.
- Nova Shield icin onemli: kayip anahtar senaryosu.
- `Msegmentation` ile `u_i` parcalara ayrilarak farkli yerler/kisilere sifrelenir.

---

## 10. Ozet: Kritik Degerler ve Kurallar

| Deger/Kural | Aciklama | Kaynak |
|-------------|----------|--------|
| `s_i = m*k_i + r*sigma_i` | Kismi imza formulu | party_i.rs:500 |
| `delta_i = k_i*gamma_i + sum(alpha_ij + beta_ij)` | MtA toplam | party_i.rs:432-433 |
| `sigma_i = k_i*w_i + sum(miu_ij + ni_ij)` | MtA anahtar kismi | party_i.rs:442-443 |
| `R = delta_inv * sum(g^gamma_i)` | Nonce noktasi hesabi | party_i.rs:473-479 |
| `com = SHA256(value \|\| blind_factor)` | Commitment schemesi | party_i.rs:195 |
| `alpha_ij + beta_ij = k_i * gamma_j` | MtA invariant | mta/mod.rs:140-146 |
| `sum(delta_i) = k * gamma` | Delta toplami | phase2_delta_i |
| `sum(sigma_i) = k * x` | Sigma toplami | phase2_sigma_i |
| `x = sum(u_i)` | Ana ozel anahtar | test.rs:152 |
| `Y = sum(y_i) = x * G` | Ana acik anahtar | party_i.rs:262 |
| `s > q/2 => s = q - s, recid ^= 1` | Low-S normalizasyonu | party_i.rs:700-703 |
| Range proofs DEVRE DISI | Guvenlik eksikligi | test.rs:190-191 |
| Safe prime onerisi | Uretim gerekliligi | party_i.rs:162 |
