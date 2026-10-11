# Agent-07: multi-party-ecdsa MASTER SENTEZ

> Kaynak: dogrudan `research/libs/multi-party-ecdsa/src/` analiz edildi.
> agent-01..06 dosyalari henuz yazilmamisti; bu sentez kaynak koddan uretildi.
> Tarih: 2026-07-22

---

## 1. Kutuphane Genel Bakis

**Repo:** `KZen-networks/multi-party-ecdsa` (GPL-3.0+)
**Dil:** Rust
**Amac:** Secp256k1 uzerinde t-of-n esik imzalama (threshold ECDSA)

### Modul Yapisi

```
src/
  lib.rs                          <- Error enum (6 varyant)
  protocols/
    multi_party_ecdsa/
      gg_2018/   party_i.rs       <- GG18 t-of-n protokolu
      gg_2020/   party_i.rs       <- GG20 t-of-n (gelismis)
                 blame.rs         <- Identifiable Abort motoru
                 state_machine/
                   keygen/rounds.rs  <- Round0..Round4 state machine
                   sign/rounds.rs    <- Round0..Round7 state machine
    two_party_ecdsa/
      lindell_2017/  party_one.rs + party_two.rs  <- 2-of-2 Paillier tabanli
      cclst_2019/    party_one.rs + party_two.rs  <- 2-of-2 CL/HSMCL tabanli
  utilities/
    mta/  mod.rs + range_proofs.rs  <- MtA + Alice/Bob ZKP
    zk_pdl_with_slack/ mod.rs       <- PDL w/ slack ispati
    zk_pdl/ mod.rs                  <- eski PDL (yedek)
```

### Kritik Sabit Degerler (kaynak koddan)

| Sabit | Deger | Dosya |
|-------|-------|-------|
| `SECURITY` | 256 bit | gg_2018/party_i.rs:42, gg_2020/party_i.rs:48 |
| `PAILLIER_MIN_BIT_LENGTH` | 2047 | gg_2020/party_i.rs:49 |
| `PAILLIER_MAX_BIT_LENGTH` | 2048 | gg_2020/party_i.rs:50 |
| `SECURITY_BITS` (2-party) | 256 | lindell_2017/mod.rs:17, cclst_2019/mod.rs:17 |
| `PAILLIER_KEY_SIZE` (lindell) | 2048 | lindell_2017/party_two.rs:52 |
| `CLGroup discriminant` (cclst) | 1348 | cclst_2019/party_one.rs:255 |

### Dis Bagimliliklar

- `curv` — eliptik egri aritmetigi (Secp256k1, Scalar, Point, BigInt)
- `paillier` — Paillier sifreleme/anahtar uretimi
- `zk_paillier` — NiCorrectKeyProof, CompositeDLogProof, DLogStatement
- `class_group` — CL/HSMCL grup islemleri (CCLST 2019 icin)
- `centipede` — Helgamal segmentli kurtarma (verifiable recovery)
- `round_based` — state machine mesajlasma altyapisi
- `sha2` — Fiat-Shamir hash (Sha256)
- `subtle` — ConstantTimeEq (zamanlama saldirisi onlemi)
- `zeroize` — Hassas degerlerin bellekten silinmesi (range_proofs.rs)

---

## 2. Protokol Karsilastirma Tablosu

| Protokol | Tur | Katilimci | KeyGen Round | Sign Round | Guvenlik Modeli | Paillier? | Identifiable Abort? |
|----------|-----|-----------|-------------|-----------|-----------------|-----------|---------------------|
| GG18 | t-of-n | n>=2, t<n | 3 broadcast | 6 phase | ROM, UC-secure | Evet | Hayir (abort only) |
| GG20 | t-of-n | n>=2, t<n | 4 round | 7 round (offline+online) | ROM, gelismis UC | Evet | Evet (phase5/6/7 blame) |
| Lindell 2017 | 2-of-2 | 2 (P1+P2) | 3 mesaj | 3 mesaj | ROM, UC-secure | Evet (P1 sahibi) | Hayir |
| CCLST 2019 | 2-of-2 | 2 (P1+P2) | 3 mesaj | 3 mesaj | ROM, UC-secure | Hayir (CL grubu) | Hayir |

---

## 3. GG18 Analizi (Ozet)

**Kagit:** Gennaro & Goldfeder 2018 — "Fast Multiparty Threshold ECDSA"
**Dosya:** `src/protocols/multi_party_ecdsa/gg_2018/party_i.rs`

### Key Generation (3 broadcast round)

**Round 1 — Commit:**
- Her parti: `u_i <- Zq` (rastgele), `y_i = u_i * G`
- Paillier keypair `(ek_i, dk_i)` uret
- `NiCorrectKeyProof` (non-interactive) ile `dk_i`'nin dogru oldugunu kanit
- `com_i = Hash(y_i || blind_factor_i)` yayinla + `ek_i` + `correct_key_proof`

**Round 2 — Decommit + VSS Dagitimi:**
- `decom_i = (blind_factor_i, y_i)` yayinla
- Commitment'lari dogrula, Paillier ispatlari dogrula
- Feldman VSS ile `u_i` paylas: `(vss_scheme, shares)` uret
- Her `j != i` icin P2P: `(vss_j, share_i_from_j)` gonder

**Round 3 — DLog Ispat:**
- VSS paylarini dogrula: `vss_scheme.validate_share(share, index) && vss.commitments[0] == y_i`
- `x_i = sum(shares)`, `y = sum(y_i)` hesapla
- `DLogProof::prove(x_i)` yayinla

**Dogrulama:** `DLogProof::verify` — hicbir VSS karsilastirmasi yok (GG18 zaafiyeti).

### Signing (6 phase — hepsi t+1 imzaci ile)

**Phase 1:** `k_i`, `gamma_i` rastgele sec; `g_gamma_i` taahhut et
**Phase 2 (MtA):** Her cift (i,j) icin:
  - `MessageA::a(k_i, ek_i, ...)` yayinla
  - `MessageB::b(gamma_j, ek_i, m_a, ...)` gonder
  - Alice: `alpha_ij = k_i * gamma_j + beta_ij` (Paillier ile)
  - `delta_i = k_i*gamma_i + sum(alpha) + sum(beta)` hesapla
  - `sigma_i = k_i*w_i + sum(miu) + sum(ni)` hesapla

**Phase 3:** `delta = sum(delta_i)` yayinla; `delta_inv = (sum delta_i)^-1`
**Phase 4:** `g_gamma_i` acikla; `R = delta_inv * sum(g_gamma_i)` hesapla
**Phase 5 (Kritik — 3 sub-phase):**
  - `s_i = m*k_i + r*sigma_i` (local imza pari)
  - Commit/Decommit: `V_i = R*s_i + l_i*G`, HomoElGamal ispati
  - Ikinci commit: `u_i = V*rho_i`, `t_i = A*l_i`
  - Kontrol: `G + sum(t_i) + sum(B_i) - sum(u_i) == G`
**Phase 6:** `s = sum(s_i)`, low-s normalize, verify (`output_signature`)

**Kritik s_i formulu (satir 500):**
```
s_i = m_fe * k_i + r * sigma_i
```

**Phase5 zinciri:** Phase5a -> Phase5c -> Phase5d -> output_signature
- Phase5a: HomoELGamalProof + DLogProof(rho_i)
- Phase5c: `v = v_i + sum(V_j) - g*m - y*r`, `u_i = v*rho_i`, `t_i = A*l_i`
- Phase5d: `G + sum(t+B) - sum(u) == G` (imza gecerliligi)

**Hata tipleri:** `InvalidKey, InvalidSS, InvalidCom, InvalidSig, Phase5BadSum, Phase6Error`

### GG18 Eksiklikleri
1. DLog ispatlari VSS taahhutlerine karsi dogrulanmiyor (GG20 duzeltir)
2. Identifiable Abort yok — kotu taraf tespit edilemiyor
3. Paillier icin safe prime zorunlu degil (production uyarisi: `create_safe_prime`)
4. `generate_h1_h2_N_tilde()` fonksiyonu GG18'de yok — GG20 ile geliyor

---

## 4. GG20 Analizi (Ozet)

**Kagit:** Gennaro & Goldfeder 2020 — "One Round Threshold ECDSA with Identifiable Abort"
**Dosyalar:** `gg_2020/party_i.rs`, `gg_2020/blame.rs`, `gg_2020/state_machine/`

### 4.1 Key Generation (4 round state machine)

**Ek GG20 Yapilari (GG18'e gore farklar):**
```rust
pub struct Keys {
    // GG18 alanlarina ek:
    pub N_tilde: BigInt,  // Yardimci Paillier modulu (range proof icin)
    pub h1: BigInt,       // h1 = rastgele, N_tilde'de
    pub h2: BigInt,       // h2 = h1^xhi mod N_tilde
    pub xhi: BigInt,      // gizli ustel (h2 = h1^xhi)
    pub xhi_inv: BigInt,  // xhi^-1 mod phi(N_tilde)
}
```

**Round0 (is_expensive=true):**
- `Keys::create(party_i)` — Paillier keypair + N_tilde/h1/h2/xhi uret
- `phase1_broadcast_phase3_proof_of_correct_key_proof_of_correct_h1h2()` cagir
- Broadcast: `KeyGenBroadcastMessage1 { e, dlog_statement, com, correct_key_proof, composite_dlog_proof_base_h1, composite_dlog_proof_base_h2 }`

**Round1:**
- Diger partilerin broadcast mesajlarini topla
- Kendi `decom1 = (blind_factor, y_i)` yayinla

**Round2 (is_expensive=true):**
- Tum decommit'leri dogrula
- GG20 EK DOGRULAMALAR (GG18'de yok):
  - `bc1.e.n.bit_length() >= 2047 && <= 2048` (Paillier boyut kontrolu)
  - `bc1.dlog_statement.N.bit_length() >= 2047 && <= 2048` (N_tilde boyut)
  - `CompositeDLogProof::verify(dlog_statement_h1)`
  - `CompositeDLogProof::verify(dlog_statement_h2)` (hem h1 hem h2 tabaninda)
- Bad actor tespiti: `bad_actors_vec` doldurulur (ErrorType icinde)
- VSS paylasimi P2P

**Round3 (is_expensive=true):**
- VSS paylarini dogrula (bad_actors takibi ile)
- `x_i = sum(shares)`, DLogProof hesapla

**Round4 (is_expensive=true):**
- `verify_dlog_proofs_check_against_vss()` — GG18'den FARKLI:
  - `xi_commitments[i] == dlog_proofs[i].pk` kontrolu (VSS'e karsi)
  - Bad actor tespiti
- `LocalKey` olustur: `{ paillier_dk, pk_vec, keys_linear, paillier_key_vec, y_sum_s, h1_h2_n_tilde_vec, vss_scheme, i, t, n }`

### 4.2 Signing (7 round — offline + online)

**Round0:** `SignKeys::create()`, `MessageA::a(k_i, ek, h1_h2_n_tilde_vec)`, broadcast

**Round1:** MtA — her j icin:
```rust
MessageB::b(&sign_keys.gamma_i, &paillier_key_vec[l_s[ind]], m_a_vec[ind], &h1_h2_n_tilde_vec)
MessageB::b(&sign_keys.w_i, &paillier_key_vec[l_s[ind]], m_a_vec[ind], &h1_h2_n_tilde_vec)
```
GG20 farki: `h1_h2_n_tilde_vec` range proof icin gecilir

**Round2:** alpha/miu hesapla, Pedersen taahhut:
```rust
let (t_i, l_i, t_i_proof) = SignKeys::phase3_compute_t_i(&sigma_i);
// T_i = sigma_i*G + l_i*H   (H = base_point2)
// PedersenProof::prove(sigma_i, l_i)
```

**Round3:** Delta'lari topla, Pedersen ispatlari dogrula, `gamma_i` acikla

**Round4:** R hesapla, `R_dash = R * k_i`, PDLwSlack ispatlari uret:
```rust
LocalSignature::phase5_proof_pdl(&R_dash, &R, &m_a.c, &ek, &k_i, &m_a_randomness, &dlog_statement)
```

**Round5:** PDLwSlack ispatlari dogrula, `R_dash` toplamini kontrol et, S_i + HomoElGamal ispati

**Round6:** S_i toplamini kontrol et: `sum(S_i) == pubkey_y`

**Round7 (online):**
```rust
LocalSignature::phase7_local_sig(k_i, message, R, sigma_i, pubkey)
// s_i = m*k_i + r*sigma_i
```
`output_signature` ile `s = sum(s_i)`, low-s normalize

### 4.3 Blame / Identifiable Abort

**3 farkli blame senaryosu (`blame.rs`):**

**GlobalStatePhase5::phase5_blame():**
- `g_gamma_i != G * gamma_i` ise suclayan imzaci tespit
- `MessageA::a_with_predefined_randomness` ile mesaj A'yi yeniden uret, karsilastir
- `MessageB::b_with_predefined_randomness` ile mesaj B'yi yeniden uret
- `alpha_beta_matrix` ile delta'lari yeniden hesapla
- `delta_vec[i] != delta_reconstruct[i]` ise o imzaci suclu

**GlobalStatePhase6::phase6_blame():**
- Paillier re-encrypt ile `miu` degerleri dogrula
- `k_randomness` ile MessageA dogrula
- `g_sigma_i` yeniden hesapla, ECDDHProof dogrula

**GlobalStatePhase7::phase7_blame():**
- Her i icin: `R * s_i == R_dash_i * m + S_i * r` kontrol et
- Yanlis hesaplayan imzaciyi tespit et

**Ortak Pattern:** `bad_signers_vec.sort_unstable(); bad_signers_vec.dedup();`

---

## 5. Lindell 2017 Analizi (Ozet)

**Kagit:** Lindell 2017 — "Fast Secure Two-Party ECDSA Signing"
**Dosyalar:** `two_party_ecdsa/lindell_2017/party_one.rs + party_two.rs`
**Mimari:** Asimetrik 2 parti (P1 = Paillier sahibi, P2 = imza baslatan)

### Key Generation (3 mesaj)

**P1 -> P2 (Msg1):**
```
KeyGenFirstMsg { pk_commitment, zk_pok_commitment }
```
- `x1 <- Zq`, `X1 = x1*G`
- `pk_commitment = Hash(X1 || blind1)`
- `zk_pok_commitment = Hash(d_log_proof.pk_t_rand_commitment || blind2)`

**P2 -> P1 (Msg2):**
```
KeyGenFirstMsg { d_log_proof, public_share }  // P2 taahhut etmez, direkt gonderir
```

**P1 -> P2 (Msg3 — decommit):**
```
KeyGenSecondMsg { comm_witness }
```
- P2 commitment'lari acin, DLogProof dogrula
- Paillier keypair uret: `PaillierKeyPair { ek, dk, encrypted_share, randomness }`
- PDLwSlack ispati: `(statement, proof, composite_dlog_proof)`
- **ONEMLI:** `create_commitments()` icinde `x1 = random / 3` (range proof gerekliligi icin!)

**P2 Dogrulamasi (`pdl_verify`):**
- `statement.ek == paillier_public.ek` ve `statement.ciphertext == encrypted_secret_share`
- `CompositeDLogProof::verify(dlog_statement)`
- `PDLwSlackProof::verify(pdl_w_slack_statement)`
- Paillier boyut: `ek.n.bit_length() >= 2047`

### Signing (3 mesaj)

**P2 -> P1:**
```
EphKeyGenFirstMsg { pk_commitment, zk_pok_commitment }
// k2 <- Zq secilir, ECDDHProof ile c = k2 * H kanitlanir
```

**P1 -> P2:**
```
EphKeyGenFirstMsg { d_log_proof, public_share, c }
// ECDDHProof: k1 bilinir, c = k1*H
```

**P2 -> P1 (PartialSig):**
```rust
let rho = BigInt::sample_below(&q.pow(2));
let partial_sig = rho * q + k2_inv * m;  // maskeleme
let c1 = Encrypt(ek, partial_sig);
let v = k2_inv * rx * x2 mod q;
let c2 = Paillier::mul(ek, enc_x1, v);  // Paillier homomorfizm
let c3 = Paillier::add(ek, c2, c1);      // c3 = Enc(k2^-1*(m + rx*x1*x2))
```

**P1 imzalar:**
```rust
let s_tag = Decrypt(dk, c3);              // s_tag = k2^-1*(m + rx*x1*x2) + noise
let s_tag_tag = s_tag * k1_inv;          // imza = k2^-1*k1^-1*(m + rx*x)
let s = min(s_tag_tag, q - s_tag_tag);   // low-s normalize
```

**Guvenlik notu:** `rho * q` terimi maskeleme saglar — P1 s_tag'den `x2*k2` ogrenemiyor.

---

## 6. CCLST 2019 Analizi (Ozet)

**Kagit:** Castagnos, Catalano, Laguillaumie, Savasta, Tucker 2019
**Dosyalar:** `two_party_ecdsa/cclst_2019/party_one.rs + party_two.rs`
**Ana Fark:** Paillier yerine CL/HSMCL (Class Group) sifreleme

### HSMCL Yapisi
```rust
pub struct HSMCL {
    pub public: PK,           // CL grup acik anahtari
    pub secret: SK,           // CL grup gizli anahtari
    pub encrypted_share: CLCiphertext,
    pub cl_group: CLGroup,    // discriminant=1348
}
```

### Key Generation

**P1 -> P2 (Msg1):** `KeyGenFirstMsg { pk_commitment, zk_pok_commitment }`
- **ONEMLI:** `x1 = random() / 3` — Lindell'le ayni kucultme (range ispati icin)
- HSMCL keypair + `verifiably_encrypt((x1, X1))` ile `CLDLProof` uret

**P2 Dogrulamasi:**
```rust
Party2Public::verify_setup_and_zkcldl_proof(hsmcl_public, seed, party1_ec_pubkey)
// setup_verify(seed) + CLDLProof::verify ile x1'in sifrelendigi ispat
```

### Signing

**P2 -> P1 (PartialSig):**
```rust
let c1 = encrypt(&group, &ek, &k2_inv_m_fe);         // CL Enc(k2^-1 * m)
let c2 = eval_scal(&encrypted_secret_share, &v);       // CL skalerleme: v * Enc(x1)
let c3 = eval_sum(&c1.0, &c2);                         // homomorfik toplam
```

**P1 imzalar:**
```rust
let s_tag = decrypt(&hsmcl.cl_group, &hsmcl_priv, &partial_sig_c3);
let s_tag_tag = k1_inv * s_tag mod q;
```

### Lindell vs CCLST Karsilastirma

| Ozellik | Lindell 2017 | CCLST 2019 |
|---------|-------------|-----------|
| Sifreleme | Paillier (RSA tabanli) | CL/HSMCL (class group) |
| Asimptotik verim | O(|q|^2 * |N|) | O(|q|^2 * disc) |
| Anahtar boyutu | 2048 bit RSA | 1348 bit discriminant |
| Guvensiz asal sorun | RSA sabit boyut | HSMCL-DL |
| Kuantum direnci | Hayir | Hayir (EC iceriyor) |
| Uretim olgunlugu | Yuksek | Dusuk-orta |
| Karmasiklik | Dusuk | Yuksek (class group) |

---

## 7. MtA ve ZK Proof Utilities

### MessageA (Alice — k_i sifreleme)
```rust
pub struct MessageA {
    pub c: BigInt,                    // Paillier(k_i)
    pub range_proofs: Vec<AliceProof>, // her j icin range ispati
}
```
`AliceProof` (Fiat-Shamir, non-interactive):
- `z = h1^a * h2^ro mod N_tilde` (taahhut)
- `e = Hash(N, Gen, c, z, u, w)` (challenge)
- `s = r^e * beta mod N`, `s1 = e*a + alpha`, `s2 = e*ro + gamma`
- Bound kontrol: `s1 <= q^3` (range ispati icin kritik)

### MessageB (Bob — gamma_j ile MtA)
```rust
pub struct MessageB {
    pub c: BigInt,                         // Paillier(gamma_j * k_i + beta_tag)
    pub b_proof: DLogProof<..>,            // gamma_j bilgisi ispati
    pub beta_tag_proof: DLogProof<..>,     // beta_tag bilgisi ispati
}
```
MtA denklemi:
- Alice cikti: `alpha = k_i * gamma_j + beta` (azaltilmis mod q)
- Bob cikti: `beta = -beta_tag mod q`
- Toplam: `alpha + beta = k_i * gamma_j`

`verify_proofs_get_alpha()`:
```rust
// ba_btag = b_proof.pk * a + beta_tag_proof.pk == g_alpha
ba_btag == g_alpha  // dlog tutarlilik
```

`BobProof` / `BobProofExt`:
- Bob'un k_i*gamma_j hesaplamasi icin uzun ZKP (z, z', t, v, w, s, s1, s2, t1, t2)
- `BobProofExt`: `g^b = X` ek kontrolu (MtAwc — with check)
- Sampling: `alpha <- [0, q^3)`, `gamma <- [0, q^2*N)`, `tau <- [0, q^3*N_tilde)`

### PDLwSlack (Lindell + GG20'de kullaniliyor)
```
Statement: (c, ek, Q, G, h1, h2, N_tilde)
Witness: (x, r) s.t. Q = x*G ve c = Enc(ek, x; r)
Range: x in [-q^3, q^3]  (slack nedeniyle)
```
Kanit elemanlari: `z, u1 (EC noktasi), u2, u3, s1, s2, s3`
Challenge: `e = Hash(G, Q, ciphertext, z, u1, u2, u3)`

### ZK-PDL (eski, `zk_pdl/` altinda)
- Daha eski yaklasim, PDLwSlack ile ikame edilmis
- Iki parti PDL challenge-response (interaktif)

---

## 8. Kritik Sayisal Degerler ve Parametreler

| Parametre | Deger | Nerede | Aciklama |
|-----------|-------|--------|----------|
| `SECURITY` | 256 bit | gg_2018, gg_2020 | Commitment blind factor boyutu |
| `PAILLIER_MIN_BIT_LENGTH` | 2047 | gg_2020/party_i.rs:49 | GG20 minimum Paillier N boyutu |
| `PAILLIER_MAX_BIT_LENGTH` | 2048 | gg_2020/party_i.rs:50 | GG20 maksimum Paillier N boyutu |
| Lindell Paillier boyutu | >=2047 | lindell/party_two.rs:307 | `ek.n.bit_length() < 2047` → hata |
| CCLST discriminant | 1348 | cclst/party_one.rs:255 | `CLGroup::new_from_setup(&1348, seed)` |
| AliceProof siniri | `q^3` | range_proofs.rs:118 | `s1 > q^3` ise reddedilir |
| BobProof alpha orneklem | `[0, q^3)` | range_proofs.rs:231 | |
| BobProof gamma orneklem | `[0, q^2*N)` | range_proofs.rs:233 | |
| BobProof tau orneklem | `[0, q^3*N_tilde)` | range_proofs.rs:237 | |
| PDLwSlack alpha orneklem | `[0, q^3)` | zk_pdl_with_slack:73 | |
| PDLwSlack rho orneklem | `[0, q*N_tilde)` | zk_pdl_with_slack:76 | |
| PDLwSlack gamma orneklem | `[0, q^3*N_tilde)` | zk_pdl_with_slack:77 | |
| h2 formulu (GG20) | `h2 = h1^xhi mod N_tilde` | gg_2020/party_i.rs:151 | |
| h2 formulu (Lindell) | `h2 = h1_inv^xhi mod N` | lindell/party_one.rs:604 | |
| GG18 signing imza | `s_i = m*k_i + r*sigma_i` | gg_2018/party_i.rs:500 | |
| GG20 signing imza | `s_i = m*k_i + r*sigma_i` | gg_2020/party_i.rs:863 | Ayni formul |
| Lindell maskeleme | `rho*q + k2_inv*m` | lindell/party_two.rs:406 | Gizlilik icin |
| Low-s normalize | `s = min(s, q-s)` | gg_2018, gg_2020, lindell | Bitcoin uyumluluğu |

---

## 9. Guvenlik Ozeti

### GG18 Guvenlik Modeli
- ROM (Random Oracle Model) altinda kanıtlanmis
- UC-secure (Universal Composability)
- t-of-n: t < n olmali, t+1 imzaci gerekir
- **Zafiyet:** VSS dogrulamasi DLog ispatlarina karsi cevrilmiyor — advers verici hatali VSS paylaslayip DLogProof ile kacabilir (GG20 bunu duzeltir)

### GG20 Guvenlik Ozellikleri
- UC-secure, ROM altinda
- **Identifiable Abort:** Hangi tarafin protokolu bozduğu tespit edilebilir
- Paillier boyut zorlamasi (2047-2048 bit): Kucuk N saldirilarini onler
- `CompositeDLogProof` ile h1/h2 iliskisi kanitlanir: Subgroup konfinement saldirisini onler
- `verify_dlog_proofs_check_against_vss()`: DLog ispatlari VSS taahhutleriyle eslestirilir

### Lindell 2017 Guvenlik Ozellikleri
- ROM altinda, UC-secure (2 parti)
- PDLwSlack + CompositeDLogProof: P1'in x1 degerini dogru sifreledigini kanit
- `ConstantTimeEq` kullanimi: Zamanlama saldirisi onlemi
- x1 ve x2 boyutlari: x1 < q/3 (range proof icin gerekli)

### CCLST 2019 Guvenlik Ozellikleri
- CL grubu uzerinde HC (Hard DL in Cl) varsayimi
- Paillier gerektirmez — RSA sayisi uretme karmasikligini yokeder
- Gercekleme daha karmask, kutuphane olgunlugu dusuk

---

## 10. Bilinen Sorunlar ve Dikkat Noktalar

### KRITIK: Safe Prime Kullanimi

GG18 ve GG20 her ikisi de `create()` ile `Paillier::keypair()` (safe prime degil) kullanir:
```rust
// GG18 - gg_2018/party_i.rs:162
// we recommend using safe primes if the code is used in production
pub fn create_safe_prime(index: u16) -> Keys {  // Ayri fonksiyon

// GG20 - gg_2020/party_i.rs:180
// we recommend using safe primes if the code is used in production
pub fn create_safe_prime(index: usize) -> Self {
```
**URETIM ICIN `create_safe_prime()` KULLANILMALIDIR.**
Ayni uyari Lindell `refresh_private_key_safe_prime()` icin de gecerlidir.

### KRITIK: N_tilde Safe Prime Sorunu (GG20)

`generate_h1_h2_N_tilde()` kodunda:
```rust
// note, should be safe primes:
// let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();
let (ek_tilde, dk_tilde) = Paillier::keypair().keys();  // YORUM: safe prime olmali!
```
N_tilde uretimi de safe prime kullanmiyor. Bu, range proof guvenligini zayiflatabilir.

### KRITIK: x1 Boyutu (Lindell + CCLST)

```rust
// cclst_2019/party_one.rs:142-143
// in Lindell's protocol range proof works only for x1<q/3
let secret_share: Scalar<Secp256k1> =
    Scalar::<Secp256k1>::from(&secret_share.to_bigint().div_floor(&BigInt::from(3)));
```
Bu kucultme SADECE `create_commitments()` icinde yapiliyor. `create_commitments_with_fixed_secret_share()` bunu YAPMAZ — harici dogrulama gerekir.

### GG20 Blame'de Eksiklik

`blame.rs` icindeki `TODO` yorumlar:
```rust
// TODO: check all parties submitted inputs
// TODO: if not - abort gracefully with list of parties that did not produce inputs
```
Eksik katilimcilar tespit edilemiyor — tum taraflar input gondermeli.

### Lindell Paillier Boyut Asimetrisi

P1 `create()` ile 2048-bit Paillier uretir (kontrol yok).
P2 dogrulamasi `verify_ni_proof_correct_key()` icinde `bit_length < 2047` kontrolu var.
**Ancak:** P1 kendi Paillier anahtarini kendisi kontrol etmiyor. P2'nin `pdl_verify` cagirmayi unutmasi ihtimali var.

### Identifiable Abort Simuli Gerektirir

GG20 blame mekanizmasi `LocalStatePhase5/6` ile calisir:
- Imzacilar once `LocalState` verisini gondermelidir
- `GlobalState::local_state_to_global_state()` tum taraflardan veri toplar
- Bu senkronizasyon sorumluluğu uygulama katmanina dusuyor

### Lindell h1/h2 Kucuk Fark

GG20 `generate_h1_h2_N_tilde()`:
```rust
// h2 = h1^xhi mod N_tilde (xhi sonra negated)
xhi = BigInt::sub(&phi, &xhi);  // h1^(-xhi) etkisi
```
Lindell `generate_h1_h2_n_tilde()`:
```rust
// h2 = h1_inv^xhi mod N
let h1_inv = BigInt::mod_inv(&h1, &ek_tilde.n).unwrap();
let h2 = BigInt::mod_pow(&h1_inv, &xhi, &ek_tilde.n);
```
Iki implementasyon farklı sekilde h2 uretir — cross-protokol kullanim YAPILMAMALIDIR.

---

## 11. Nova Shield Chain Icin KARAR: Hangi Protokol?

### Gereksinim Degerlendirmesi

Nova Shield Chain bir Layer 1 blockchain oldugundan asagidaki kriter gecerlidir:

| Kriter | GG18 | GG20 | Lindell 2017 | CCLST 2019 |
|--------|------|------|-------------|-----------|
| t-of-n destegi | Evet | Evet | Hayir (2-of-2) | Hayir (2-of-2) |
| Identifiable Abort | Hayir | Evet | Hayir | Hayir |
| Uretim olgunlugu | Orta | Yuksek | Yuksek | Dusuk |
| Kod karsasigligi | Orta | Yuksek (daha fazla round) | Dusuk | Yuksek |
| Paillier bagimlilik | Evet | Evet | Evet | Hayir |
| Key refresh destegi | Kismi | Kismi | Evet | Hayir |
| State machine var mi? | Hayir | Evet (round_based) | Hayir | Hayir |

### KARAR: GG20 Kullanilmalidir

**Gerekce:**

1. **Identifiable Abort zorunlulugu:** Bir blockchain'de hangi validatorun protokolu bozduğunu tespit etmek slaslama ve ceza mekanizmalari icin kritiktir. GG20 bunu saglar; GG18 saglamaz.

2. **t-of-n esnekligi:** Validator setleri degisken esik (ornegin 2/3 coğunluk) gerektirdiginden Lindell ve CCLST (sadece 2-of-2) bu gereksinimi karsilamaz.

3. **State Machine altyapisi:** `round_based` crate entegrasyonu `gg_2020/state_machine/` altinda zaten mevcuttur. Bu, P2P ag katmaniyla entegrasyonu kolaylastirir — gg_2018 icin boyle bir altyapi yok.

4. **VSS-DLogProof tutarlilik kontrolu:** GG20 `verify_dlog_proofs_check_against_vss()` ile DLog ispatlari VSS taahhutleriyle cakistiriliyor; bu GG18'in bilinen zaafiyetini giderir.

5. **Paillier boyut zorlamasi:** 2047-2048 bit kontrolu saldiri yuzeyini daraltiyor.

6. **Pedersen taahhut ile sigma_i:** GG20 Round2'de `T_i = sigma_i*G + l_i*H` ve PedersenProof kullaniliyor; bu sigma_i'nin dogruluğunu erken saglar.

**Neden GG18 DEGIL:** Identifiable abort eksikligi ve VSS zaafiyeti.
**Neden Lindell DEGIL:** Sadece 2-of-2; blockchain icin yetersiz.
**Neden CCLST DEGIL:** Kod olgunlugu dusuk, class_group bagimliliginin ekosistem destegi zayif.

### Alternatif: cggmp21

MEMORY.md'de `MPC:cggmp21` teknoloji karari zaten not edilmis. Bu repo'daki `GG20` ile `cggmp21` (CGGMP 2021 — Canetti, Gennaro, Goldfeder, Makriyannis, Peled) karsilastirildiginda:

| Ozellik | GG20 (bu repo) | cggmp21 |
|---------|------------|---------|
| Identifiable Abort | Evet | Evet |
| Round sayisi (signing) | 7 | 4 (prosign ile 3) |
| Refresh destegi | Kismi | Tam (auxiliary) |
| Audit edildi mi? | Sinirli | Evet (ZenGo/Trail of Bits) |
| Uretim kullanimi | Sinirli | Yuksek (ZenGo, DFINITY) |

**Sonuc:** Bu repodaki GG20, `cggmp21` onemsenmeden once bir referans uygulama olarak kullanilabilir. Ancak MEMORY.md'deki karara uygun olarak `cggmp21` crate'i tercih edilmelidir.

---

## 12. Uretim Ortami Icin Oneriler

### 1. Safe Prime Kullanimi — ZORUNLU

Her anahtar uretiminde safe prime kullanilmalidir:
```rust
// GG20
Keys::create_safe_prime(party_index)
// yerine Keys::create() KULLANMAYINIZ

// Lindell
party_one_private.refresh_private_key_safe_prime(&factor, index)
```

### 2. N_tilde Safe Prime — ZORUNLU

`generate_h1_h2_N_tilde()` fonksiyonundaki yorum satirini aktif edin:
```rust
// Aktif edilmesi gereken satir:
let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();
// Simdi kullanilan (YANLISS):
// let (ek_tilde, dk_tilde) = Paillier::keypair().keys();
```

### 3. Paillier Boyut Dogrulamasi

GG20 zaten boyut kontrolu yapıyor (2047-2048 bit). Uygulama katmaninda ek kontrol:
- Minimum 2048-bit secure Paillier keypair zorunlu kilinmali
- `Paillier::keypair_safe_primes()` kullanilmalidir

### 4. Blame Mekanizmasini Aktif Edin

GG20 blame icin `LocalState` veri koleksiyonu protokolun bir parcasi olmalidir:
```rust
// Her imzaci LocalStatePhase5/6 verisini saklamali
// Protokol basarisizliginda GlobalState olusturularak blame cagrilmali
GlobalStatePhase5::phase5_blame()
GlobalStatePhase6::phase6_blame()
GlobalStatePhase7::phase7_blame()
```

### 5. Anahtar Yenileme (Key Refresh)

Bu repo'da native key refresh desteği sinirlidir:
- GG18/GG20: `update_private_key()` var ama tam refresh protokolu yok
- `cggmp21` crate'i tam `KeyRefresh` protokolu saglar
- Nova Shield Chain icin `cggmp21`'in `aux_info_key_refresh` modulu kullanilmalidir

### 6. Verifiable Recovery

`centipede` entegrasyonu mevcuttur:
```rust
// Helgamal segmentli kurtarma
PartyPrivate::to_encrypted_segment(segment_size, num_segments, pub_key, G)
```
Kurtarma anahtarini ayri, guvenli bir yerde saklanmali.

### 7. Zeroize Kontrol

`AliceZkpRound1` ve `BobZkpRound1` `#[derive(Zeroize)] #[zeroize(drop)]` ile korunuyor.
Ancak `PartyPrivate`, `EcKeyPair`, `Party1Private` struct'lari zeroize UYGULAMIYOR.
Uretimde bu struct'lar icin `zeroize` tureticisi eklenmeli ya da `cggmp21` (daha kapsamli zeroize) tercih edilmeli.

### 8. Ag Katmani Entegrasyonu

GG20 state machine `round_based` crate kullanir:
- `Push<Msg<T>>` trait ile mesaj gonderimi
- `Store<BroadcastMsgs<T>>` ve `Store<P2PMsgs<T>>` ile mesaj depolama
- `is_expensive()` flag'i: Ag katmani, pahali round'lari onceden bilip zamanlama yapabilir
- `round_based` + `libp2p + gossipsub` (MEMORY.md'deki karar) birlikte calismali

### 9. Test Kapsamı

Mevcut testler:
- `test_keygen_t1_n2`, `test_keygen_t2_n3`, `test_keygen_t2_n4`
- `test_sign_n5_t2_ttag4`, `test_sign_n8_t4_ttag6` (GG18)
- Blame testleri: `GlobalStatePhase5/6/7` icin gosterilenler

Uretimde ek edilmesi gerekenler:
- Kotu girdi simulasyonu (manipule edilmis commitment, yanlis VSS)
- Blame senaryosu testleri (hangi taraf bozarsa tespit edilir)
- Performance benchmark (Paillier anahtar uretimi ~1-2s gecikme)

### 10. Kritik Guvenlik Kontrol Listesi

- [ ] `create_safe_prime()` kullaniliyor mu?
- [ ] N_tilde icin safe prime aktif mi?
- [ ] `verify_ni_proof_correct_key()` her oturumda cagriliyor mu?
- [ ] `pdl_verify()` (Lindell) veya `verify_dlog_proofs_check_against_vss()` (GG20) calistirildi mi?
- [ ] `CompositeDLogProof` dogrulamasi (h1/h2) yapildi mi?
- [ ] Paillier N bit uzunlugu 2047-2048 arasi mi?
- [ ] `x1 < q/3` (Lindell/CCLST) kontrolu `create_commitments_with_fixed_secret_share` icin yapildi mi?
- [ ] Blame akisi tum hata senaryolari icin hayata gecirildi mi?
- [ ] Zeroize uygulandi mi?
- [ ] ConstantTimeEq tum karsilastirmalarda kullaniliyor mu?

---

## Ozet Tablo: Nova Shield Chain Icin En Onemli Bulgular

| Konu | Deger / Durum |
|------|--------------|
| Onerilens protokol | GG20 (bu repo) veya `cggmp21` (oncelikli) |
| Minimum Paillier boyutu | 2048 bit (safe prime ile) |
| N_tilde boyutu | 2048 bit (safe prime ile — simdi eksik) |
| MtA range bound | `s1 <= q^3` (AliceProof), `s1 <= q^3` (BobProof) |
| GG18 → GG20 fark sayisi | 5 kritik: VSS-DLog, blame, CompositeDLog, Paillier boyut, Pedersen |
| Blame round sayisi | Phase5, Phase6, Phase7 — 3 ayri blame mekanizmasi |
| State machine mevcut | Sadece GG20 icin (`round_based` entegrasyonu) |
| Uretim kritik eksik | Safe prime (gg_2018/gg_2020/lindell), N_tilde safe prime |
| `x1 < q/3` kucultme | Sadece `create_commitments()`, fixed version'da EKSIK |
| cggmp21 ile fark | 4 round signing (GG20: 7), tam refresh, daha kucuk proof |
