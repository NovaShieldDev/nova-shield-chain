# Agent-05: CCLST 2019 Two-Party ECDSA Analizi

## Kapsanan Dosyalar

| Dosya | Satir | Aciklama |
|-------|-------|----------|
| `src/protocols/two_party_ecdsa/cclst_2019/party_one.rs` | 419 | P1 struct/impl |
| `src/protocols/two_party_ecdsa/cclst_2019/party_two.rs` | 356 | P2 struct/impl |
| `src/protocols/two_party_ecdsa/cclst_2019/mod.rs` | 24 | Modul bildirimi + SECURITY_BITS |
| `src/protocols/two_party_ecdsa/cclst_2019/test.rs` | 142 | 3 entegrasyon testi |
| `benches/two_party_ecdsa/cclst_2019/keygen.rs` | 64 | Keygen benchmark |
| `benches/two_party_ecdsa/cclst_2019/sign.rs` | 92 | Sign benchmark |

Dis bagimlilk: `class_group = "0.6"` (optional feature = "cclst", Cargo.toml satir 49-52)
`cl_dl_public_setup` modulu: `class_group::primitives::cl_dl_public_setup`

---

## 1. CCLST 2019 vs Lindell 2017: Temel Fark

### Sorun: Lindell 2017 neden degistirildi?

Lindell 2017 protokolu asimetrik sifreleme olarak **Paillier** kullanir. Paillier'in iki kritik maliyeti vardir:

1. **Anahtar Uretimi**: Paillier key generation, iki buyuk asal sayi uretimini gerektirir (RSA benzeri). Bu islem agir ve yavaştır.
2. **Guvenlik Kaniti (ZKP)**: Paillier anahtarinin "dogru" uretildigini kanitlamak icin `NiCorrectKeyProof` ve `PDLwSlackProof` gibi karmasik ZK kanitleri gerekir (Lindell party_one.rs satir 36-48).

### CCLST 2019'un Cozumu

CCLST 2019 (Castagnos-Catalano-Laguillaumie-Savasta-Tucker, 2019) Paillier'i tamamen atar ve yerine **class group sifreleme** koyar. Temel fark:

| Ozellik | Lindell 2017 | CCLST 2019 |
|---------|-------------|------------|
| Sifreleme | Paillier (RSA tabanli) | Class group (CL) sifreleme |
| Anahtar uretimi | Buyuk asal uretim (agir) | CLGroup parametrelerinden turetilir |
| ZKP tipi | `NiCorrectKeyProof` + PDL with slack | `CLDLProof` (class group DL kaniti) |
| Guvenlik varsayimi | RSA, Strong RSA | Class group DL problemi |
| Range proof | Paillier ciphertexti uzerinde | CL ciphertexti uzerinde (dahili) |
| Kodda fark | `PaillierKeyPair` struct | `HSMCL` struct (party_one.rs:67-72) |

### HSMCL Nedir?

`HSMCL` = **H**idden **S**ubgroup **M**embership + **C**lass group **L**inear encryption.

Kodda (party_one.rs satir 67-72):
```rust
pub struct HSMCL {
    pub public: PK,
    pub secret: SK,
    pub encrypted_share: CLCiphertext,
    pub cl_group: CLGroup,
}
```

- `CLGroup`: Class group matematiksel yapisi (discriminant ile tanimli)
- `PK` / `SK`: Class group public/secret anahtar cifti
- `CLCiphertext`: Plaintext'in class group ile sifrelanmis hali
- `encrypted_share`: P1'in `x1` ozel anahtarinin sifrelanmis hali

---

## 2. Class Group Kriptografisi

### 2.1 Matematiksel Temel

Class group kriptografisi, belirli bir **discriminant** (ayirt edici) `Delta < 0` ile tanimlanan **ikinci dereceden sayi cismi** uzerinde calisir:

```
Q(sqrt(Delta)) = { a + b*sqrt(Delta) : a, b ∈ Z }
```

Bu cisminin **ideal sinif grubu** `Cl(Delta)`, belirli bolunsuzluk kosullarini saglayan ideallerin esitlik siniflari kümesidir.

### 2.2 CLGroup Kurulumu

`party_one.rs` satir 255:
```rust
let cl_group = CLGroup::new_from_setup(&1348, seed);
```

- **Discriminant boyutu**: 1348 bit
- **seed**: "Nothing up my sleeve" sabiti (pi'nin ilk 150 hanesi, test.rs satir 49-52)
- `seed`, grup parametrelerinin deterministik ve guvenli sekilde uretildigini dogrulayabilmek icin kullanilir

Neden 1348 bit? CCLST 2019 makalesinde onerilen guvenlik parametresi; class group DL probleminin zorlugu icin 1348-bit discriminant, ~128-bit klasik guvenligi saglar.

### 2.3 Sifreleme Mekanizmasi

Class group sifreleme ElGamal benzeridir ancak `Cl(Delta)` uzerinde calisir:

**Anahtar Uretimi**:
- Ozel anahtar: `sk ∈ Z` (buyuk rastgele tam sayi)
- Acik anahtar: `pk = g^sk` (grup ureticisinin kuvveti)

**Sifreleme** (`encrypt` fonksiyonu, party_two.rs satir 346):
- Mesaj `m` icin: rastgele `r` sec
- Ciphertext: `(c1, c2) = (g^r, pk^r * f^m)`
- Burada `f` sabit bir grup eleman (mesajlari alana yuklemek icin)

**Homofonik Islemler** (party_two.rs satir 350-351):
```rust
let c2 = eval_scal(&party_two_public.encrypted_secret_share, &v);
let c3 = eval_sum(&c1.0, &c2);
```
- `eval_scal(ct, v)`: Ciphertext'i skaler ile carpma (homomorfik skaler carpim)
- `eval_sum(ct1, ct2)`: Iki ciphertext'i toplama (homomorfik toplama)

Bu iki islem, party two'nun signing sirasinda sifreli aritmetik yapmasini saglar.

### 2.4 Paillier'den Farklar (Ozet)

```
Paillier:
  KeyGen: p, q asal → n=pq, lambda=lcm(p-1,q-1)
  Encrypt: c = g^m * r^n mod n^2
  Complexity: O(|n|^2) mod exp

Class Group:
  KeyGen: discriminant Delta, seed → CLGroup
  Encrypt: (g^r, pk^r * f^m) in Cl(Delta)
  Complexity: group operations in Cl(Delta)
```

Class group'un avantaji: RSA modulu gibi faktorleme gerekmez, bu nedenle anahtar uretimi daha hizlidir ve ZKP gereksinimleri daha temizdir.

---

## 3. Key Generation: Round-by-Round

### Round 1 — P1 baslangici

**P1 (`KeyGenFirstMsg::create_commitments`, party_one.rs satir 137-179)**:

```
x1 ← Z_q/3       (satir 142-143: range kisitlamasi, x1 < q/3)
Q1 = x1 * G       (ECDSA public share)
pi1 = DLogProof(x1)
c1 = Commit(Q1, r1)       (hash commitment)
c2 = Commit(pi1.R, r2)    (ZK pok commitment)
```

Onemli not: `x1 ← Zq/3` (satir 142-143). Bu Lindell 2017'den miras kalan bir kalinti. Yorum satirinda acikca yaziliyor: *"in Lindell's protocol range proof works only for x1<q/3"*. CCLST 2019'da bu kisitlama teknik olarak gerekli olmayabilir, ancak kod muhafaza etmis.

P1 gonderir: `(pk_commitment, zk_pok_commitment)` — yani SADECE commitment'lar, henuz public key veya kanit yok.

---

**P2 (`KeyGenFirstMsg::create`, party_two.rs satir 127-143)**:

```
x2 ← Z_q
Q2 = x2 * G
pi2 = DLogProof(x2)
```

P2 gonderir: `(Q2, pi2)` — commitment yok, direkt acik.

---

### Round 2 — P1 decommit + HSMCL kurulumu

**P1 (`KeyGenSecondMsg::verify_and_decommit`, party_one.rs satir 223-231)**:

```
1. DLogProof::verify(pi2)   → P2'nin Q2 gercek mi?
2. Decommit: (Q1, pi1, r1, r2) gonder
```

---

**P2 (`KeyGenSecondMsg::verify_commitments_and_dlog_proof`, party_two.rs satir 166-209)**:

```
1. Hash commitment dogrulama (x2 kez):
   Commit(Q1, r1) == c1?
   Commit(pi1.R, r2) == c2?
2. DLogProof::verify(pi1)   → P1'in Q1 gercek mi?
```

---

### Round 3 — HSMCL Kurulumu (tek sefer, keygen sonunda)

**P1 (`HSMCL::generate_keypair_and_encrypted_share_and_proof`, party_one.rs satir 251-278)**:

```
cl_group = CLGroup::new_from_setup(1348, seed)
(sk_cl, pk_cl) = cl_group.keygen()
(Enc(x1), pi_cldl) = verifiably_encrypt(cl_group, pk_cl, (x1, Q1))
```

`verifiably_encrypt` iki sey yapar:
1. `x1`'i class group ile sifreler → `Enc(x1)`
2. `CLDLProof` uretir: "Sifreledim `x1`'i VE bu `x1` oyle ki `Q1 = x1 * G`"

P1 gonderir: `HSMCLPublic { pk_cl, pi_cldl, Enc(x1), cl_group }`

---

**P2 (`Party2Public::verify_setup_and_zkcldl_proof`, party_two.rs satir 228-251)**:

```
1. cl_group.setup_verify(seed)   → grup parametreleri gercek mi?
2. pi_cldl.verify(cl_group, pk_cl, Enc(x1), Q1)   → CLDLProof dogru mu?
```

Ikisi de basarili olursa P2 `Party2Public { group, ek: pk_cl, encrypted_secret_share: Enc(x1) }` saglar.

---

### Sonuc: Joint Public Key

```
Q = x1 * Q2 = x2 * Q1 = x1 * x2 * G
```

Her iki taraf kendi ozel anahtarini bilerek toplam public key'i hesaplar:
- P1: `compute_pubkey(&party1_private, Q2)` → `x1 * Q2`
- P2: `compute_pubkey(local_share, Q1)` → `x2 * Q1`

---

## 4. Signing: Round-by-Round

### Signing Round 1 — Efemeral Key Uretimi

**P2 (`EphKeyGenFirstMsg::create_commitments`, party_two.rs satir 254-309)**:

```
k2 ← Z_q
R2 = k2 * G
H = base_point2()     # deterministik ikinci uretec
c_hat = k2 * H
pi_ecdh = ECDDHProof(k2, (G, R2, H, c_hat))
commitment_pk = Commit(R2, r1)
commitment_zk = Commit(Hash(pi_ecdh.a1, pi_ecdh.a2), r2)
```

P2 gonderir: `(commitment_pk, commitment_zk)` — commitment'lar

ECDDHProof: "k2 icin x var oyle ki R2 = x*G ve c_hat = x*H" — bu double DH kaniti. Neden? Malleability saldirilarini onlemek icin (P1 R2'yi degistiremez).

---

**P1 (`EphKeyGenFirstMsg::create`, party_one.rs satir 281-310)**:

```
k1 ← Z_q
R1 = k1 * G
H = base_point2()
c = k1 * H
pi_ecdh = ECDDHProof(k1, (G, R1, H, c))
```

P1 gonderir: `EphKeyGenFirstMsg { pi_ecdh, R1, c }` — commitment YOK, direkt.

---

### Signing Round 2 — Verification ve Decommit

**P2 (`EphKeyGenSecondMsg::verify_and_decommit`, party_two.rs satir 312-326)**:

```
ECDDHProof::verify(pi_ecdh_P1, (G, R1, H, c))
Decommit: (R2, pi_ecdh_P2, r1, r2) gonder
```

---

**P1 (`EphKeyGenSecondMsg::verify_commitments_and_dlog_proof`, party_one.rs satir 312-357)**:

```
1. Commit(R2, r1) == commitment_pk?
2. Commit(Hash(pi_ecdh_P2.a1, pi_ecdh_P2.a2), r2) == commitment_zk?
3. ECDDHProof::verify(pi_ecdh_P2, (G, R2, H, c_hat))
```

---

### Signing Round 3 — Partial Signature (P2)

**`PartialSig::compute` (party_two.rs satir 329-356)**:

```
r = k2 * R1         # ortak noktayi hesapla (r = k1*k2*G)
rx = r.x mod q

k2_inv = k2^{-1} mod q
k2_inv_m = k2_inv * message mod q      # k2^{-1} * m

c1 = Encrypt(pk_cl, k2_inv_m)          # class group ile sifrele

v = k2_inv * x2 * rx mod q             # k2^{-1} * x2 * rx
c2 = eval_scal(Enc(x1), v)             # Enc(x1 * v) homomorfik
c3 = eval_sum(c1, c2)                   # Enc(k2_inv_m + x1*x2*rx/k2)
```

Matematiksel olarak `c3` sunun sifrelenmesidir:
```
c3 = Enc( k2^{-1} * m + k2^{-1} * x1 * x2 * rx ) mod q
   = Enc( k2^{-1} * (m + x1*x2*rx) ) mod q
```

P2 gonderir: `PartialSig { c3 }` — sifreli kismi imza

---

### Signing Round 4 — Final Signature (P1)

**`Signature::compute` (party_one.rs satir 359-392)**:

```
r = k1 * R2         # r = k1*k2*G (ayni nokta)
rx = r.x mod q

k1_inv = k1^{-1} mod q

s_tag = decrypt(cl_group, sk_cl, c3)
# s_tag = k2^{-1} * (m + x1*x2*rx)

s_tag_tag = k1_inv * s_tag mod q
# s_tag_tag = k1^{-1} * k2^{-1} * (m + x1*x2*rx)
#           = (k1*k2)^{-1} * (m + x_shared * rx)

s = min(s_tag_tag, q - s_tag_tag)   # dusuk-s normalizasyonu
```

Sonuc: `Signature { s, r: rx }` — standart ECDSA imzasi.

**Dogruluk ispati**:
```
s = (k1*k2)^{-1} * (m + x_shared * rx)  (mod q)

ECDSA verify: u1 = m * s^{-1} = m * (k1*k2) / (m + x_shared * rx)
              u2 = rx * s^{-1} = rx * (k1*k2) / (m + x_shared * rx)
u1*G + u2*Q = [m/(m+x*rx)] * G*k1k2 + [x*rx/(m+x*rx)] * G*k1k2
            = G * k1*k2 * (m + x*rx) / (m + x*rx) = G * k1*k2 = R
=> x(R) = rx ✓
```

---

## 5. Kriptografik Primitive'ler

### 5.1 CLDLProof (Class Group Discrete Log + DL baglantisi)

**Kullanim**: `verifiably_encrypt` (party_one.rs satir 257-261) ve `proof.verify` (party_two.rs satir 235-240)

Bu kanit iki seyi birden ispat eder:
1. `Enc(x)` dogru sifrelenmis (`x` bilinmekte)
2. `Q = x * G` (ayni `x` EC public key'i olusturuyor)

Yani: "Sifreledim gizli anahtarimi VE sifre icindekilerin EC karsiligi benim public key'im"

Bu, Lindell 2017'deki PDLwSlackProof ile esdeser role sahip, ancak class group ortaminda.

### 5.2 ECDDHProof (Sigma Protokolu)

**Kullanim**: `EphKeyGenFirstMsg::create` (party_one.rs satir 287-296, party_two.rs satir 262-272)

Statement: `(G, R, H, c)` icin `x` var oyle ki `R = x*G` ve `c = x*H`

Bu, sabit ikinci uretec `H = base_point2()` uzerinde caligir. Amaci: tarafin efemeral anahtarini tek bir deger olarak kilitlemek — biri R, digeri H uzerindeki commitment. Malleable attack'lara karsi.

### 5.3 DLogProof (Schnorr benzeri)

**Kullanim**: Uzun vadeli key'ler icin (party_one.rs satir 147, party_two.rs satir 131)

Standard sigma protokolu: `x` bilinmekte, `Q = x*G` ispatlaniyor.

### 5.4 HashCommitment (SHA-256)

**Kullanim**: `pk_commitment`, `zk_pok_commitment` (party_one.rs satir 149-160)

```
Commit(m, r) = SHA256(m || r)
```

Binding ve hiding ozellikleri saglar. `SECURITY_BITS = 256` (mod.rs satir 17) ile rastgelelik ornekleniyor.

### 5.5 Homomorfik Islemler

| Fonksiyon | Etki | Dosya |
|-----------|------|-------|
| `encrypt(group, pk, m)` | `Enc(m)` uretir | party_two.rs:346 |
| `eval_scal(ct, v)` | `Enc(m*v)` hesaplar | party_two.rs:350 |
| `eval_sum(ct1, ct2)` | `Enc(m1+m2)` hesaplar | party_two.rs:351 |
| `decrypt(group, sk, ct)` | Plaintext'e doner | party_one.rs:376 |

Bu dort islem, `PartialSig` hesaplamasindaki butun sifirsel aritmetigi mumkun kilar.

### 5.6 CLGroup Kurulum Dogrulamasi

Party_two.rs satir 233: `hsmcl_public.cl_group.setup_verify(seed)`

Party 2, CLGroup'un public seed'e gore dogru kurulup kurulmadigini dogrular. Bu, P1'in ozel bir discriminant secip CLGroup DL problemini kolay hale getirmesini engeller (backdoor prevention).

---

## 6. Test ve Benchmark Analizi

### 6.1 Testler (test.rs)

**`test_d_log_proof_party_two_party_one`** (satir 9-24):
- Sadece keygen Round 1-2 test eder (DLog commitment/decommit)
- HSMCL kurulumu yapilmiyor, minimal test

**`test_full_key_gen`** (satir 27-67):
- Tam keygen: commitment, DLog, HSMCL kurulumu, CLDLProof dogrulamasi
- P2'nin `x2 = 10` (sabit, test amacli)
- Seed: pi'nin ilk ~150 hanesi (deterministik "nothing up my sleeve")
- CLGroup discriminant: 1348 bit

**`test_two_party_sign`** (satir 70-142):
- Uc asamali: keygen simule et → signing round'lari → verify
- Mesaj: `BigInt::from(1234)` — trivial deger, gercek hash degil
- Tam akis: HSMCL kurulumu, efemeral key exchange, partial sig, final sig, verify

### 6.2 Benchmark Kurgusu

Her iki benchmark (`keygen.rs` ve `sign.rs`) icin:
- Framework: `criterion` v0.3
- `sample_size(10)` — sadece 10 ornek (normal 100)
- Bu, class group islemlerinin cok agir oldugunu gosteriyor

**Onemli**: Sign benchmark'i keygen'i icinde tekrar calistiriyor (satir 14-39 in sign.rs). Yani sign benchmark aslinda tam keygen + sign surecini olcuyor, izole bir signing suresi degil.

### 6.3 Lindell 2017 Benchmark Karsilastirmasi (kod analizinden)

Keygen siralamasi (Lindell'de, lindell_2017/keygen.rs):
1. EC key commitment/decommit
2. `PaillierKeyPair::generate_keypair_and_encrypted_share` — buyuk asal uretimi
3. `generate_ni_proof_correct_key` — Paillier dogru anahtar ZKP
4. `pdl_proof` — PDL with slack proof

Keygen siralamasi (CCLST'de, cclst_2019/keygen.rs):
1. EC key commitment/decommit
2. `CLGroup::new_from_setup(1348, seed)` — deterministik grup kurulumu
3. `cl_group.keygen()` — class group anahtar ciftini uret
4. `verifiably_encrypt` — sifrele + CLDLProof uret

**Kiyaslama notu**: `sample_size(10)` CCLST 2019 icin kullanilmis, ancak Lindell 2017 icin de ayni deger kullanilmis. Bu, ikisinin de agir oldugunu ancak kesin sureler icin benchmark calistirmak gerektigini gosteriyor.

Teorik beklenti (CCLST 2019 makalesi ve ilgili literature gore):
- Paillier anahtar uretimi: buyuk RSA modulu gerektiriyor (~2048 bit) — class group'tan muhtemelen daha agir
- Class group keygen: discriminant bilindikten sonra gruba ozel islemler — daha hafif
- Signing: ikisi de benzer homoorfik islem sayisina sahip

---

## 7. Guvenlik Degerlendirmesi

### 7.1 Guvenlik Varsayimlari

| Varsayim | Gercek | Karsiligi |
|----------|--------|-----------|
| Class Group DL | `Cl(Delta)` uzerinde DL zor | Temel sifrelem guvenligi |
| Hash Commit Binding | SHA-256 collision resistant | Commitment scheme |
| EC DL (secp256k1) | Standart ECDSA | Uzun vadeli anahtar |
| DDH (secp256k1) | ECDDHProof guvenligi | Efemeral anahtar dogrulamasi |

### 7.2 Gozlemlenen Guvenlik Ozellikleri

**"Nothing up my sleeve" seed** (test.rs satir 49-52, sign.rs satir 22-23):
```
seed = 314159265358979323846264338327950288419716939937510582097494...
```
Pi sayisi kullanilmis. `setup_verify(seed)` ile P2 bu deterministizmi dogruluyor.

**Low-s normalizasyonu** (party_one.rs satir 386-389):
```rust
let s = cmp::min(s_tag_tag.clone(), group_order() - s_tag_tag);
```
BIP-62 uyumlu: malleability'i azaltir.

**verify'de constant-time karsilastirma** (party_one.rs satir 412):
```rust
rx_bytes.ct_eq(u1_plus_u2_bytes).unwrap_u8() == 1
```
`subtle` crate kullaniyor — timing side-channel'lara karsi.

**x1 < q/3 kisitlamasi** (party_one.rs satir 142-143):
Lindell 2017 protokolunden aktarilmis. CCLST'de class group range proof farkli calistigindan bu kisitlama artik zorunlu degil ama kod korumaya devam ediyor.

### 7.3 Eksiklikler ve Soru Isaretleri

1. **PDL Protokolu eksik**: `PDLFirstMessage`, `PDLSecondMessage`, `PDLdecommit` struct'lari tanimlanmis (party_one.rs satir 102-116, party_two.rs satir 72-97) ancak hicbir `impl` blogu yok. Bu struct'lar kullanilmiyor. Muhtemelen implement edilmemiş bir PDL kontrolü (P2'nin x2'sinin dogru oldugunu ispatlama). BU EKSIKLIK GUVENLIK ACIGI OLUSTURABILIR.

2. **Signing mesaji hash'lenmiyor**: `test_two_party_sign`'da mesaj `BigInt::from(1234)` kullaniliyor. Gercek uygulamada mesajin once hash'lenmesi (SHA-256 veya keccak) zorunludur.

3. **P1'in CLDLProof keyleri tek sefer**: HSMCL keyleri keygen'de bir kez olusturuluyor ve her signing'de ayni `sk_cl` kullaniliyor. Bu, class group sifreleme guvenliginin uzun vadede korunmasini gerektirir.

4. **`x1 ← Zq/3` yorum**: Kodun yorumuna gore bu Lindell protokolu icin gereklidir. CCLST'de class group range proof farkli cisgida calistigindan bu kisitlamanin kaldirilip kaldirilmadigini resmi makale ile dogrulamak gerekir.

---

## 8. Nova Shield Chain Icin Cikarsimlar

### 8.1 CCLST 2019 mu, Lindell 2017 mi, cggmp21 mi?

Proje hafizasina gore Nova Shield Chain MPC icin `cggmp21` kullanmaya karar verilmis. CCLST 2019 analizi yine de bilgilendiricidir:

| Kriter | CCLST 2019 | Lindell 2017 | cggmp21 (hedef) |
|--------|-----------|-------------|-----------------|
| Taraf sayisi | 2 | 2 | t-of-n |
| Sifreleme | Class Group | Paillier | Paillier |
| ZKP maliyeti | CLDLProof | NiCorrectKey + PDL | Elgamal+Paillier ZKP |
| Maintenanmce | Aktif degil | Aktif degil | Aktif (dfns) |
| Rust olgunlugu | class_group v0.6 | kzen-paillier | cggmp21 crate |

### 8.2 Class Group Hakkinda Notlar

- Eger threshold signature (t-of-n) yerine 2-of-2 yapisiniz varsa CCLST 2019 guclu bir secenektir
- Class group'un `setup_verify` mekanizmasi, trusted setup gerektirmemesi acisindan avantajlidir
- 1348-bit discriminant gecerli guvenlik parametresi, ancak gelecekte arttirilabilir (2048-bit tartismali)
- `class_group = "0.6"` crate'i KZen Networks tarafindan yazilmis, aktif maintain edilmeyebilir

### 8.3 Kod Mimarisinden Alinan Dersler

**Pozitif desenler**:
1. Struct bazli mesaj tipleri — her round kendi struct'i
2. `Result<_, ProofError>` ile hata yonetimi — dogrulama basarisizliklari propagate edilir
3. Commitment-then-reveal deseni efemeral key exchange'de tutarli uygulanmis
4. `subtle::ConstantTimeEq` ile side-channel korumasi

**Dikkat edilmesi gerekenler**:
1. PDL struct'lari implement edilmemis — eksik guvenlik kaniti
2. Benchmark'lar keygen'i sign icinde tekrar calistiriyor — yaniltici sure
3. `x1 < q/3` kisitlamasi: CCLST 2019'da gereksiz olabilir, performans etkileyebilir

### 8.4 Nova Shield Chain Entegrasyonu Icin Tavsiyeler

Eger CCLST 2019 kullanilacaksa:
- `class_group` crate'inin kaynak kodunu inceleyin ve aktif bir fork'unu kullanin
- PDL protokolunu implement edin veya alternatif P2 sahteciligine karsi koruma ekleyin
- Signing'de mesajin `SHA256(message)` olarak iletildiginden emin olun
- `CLGroup::new_from_setup` parametrelerini (discriminant boyutu) guvenlik gereksinimlerinize gore belirleyin
- Benchmark'lari izole halde calistirin (keygen ve signing ayri ayri olcun)

Eger cggmp21 kullanilacaksa (mevcut karar):
- Homoorfik sifreleme olarak yine Paillier kullanilir
- Threshold t-of-n yapisina ihtiyac varsa cggmp21 daha uygun
- CCLST mimarisinden ogrenilecek ders: `setup_verify` benzeri bir determinizm dogrulamasi cggmp21 entegrasyonunda da faydali olabilir

---

## Onemli Satir Referanslari

| Konu | Dosya | Satir |
|------|-------|-------|
| `HSMCL` struct tanimi | party_one.rs | 67-72 |
| `CLGroup::new_from_setup(1348, ...)` | party_one.rs | 255 |
| `verifiably_encrypt` cagrisi | party_one.rs | 257-261 |
| `x1 ← Zq/3` kisitlamasi | party_one.rs | 142-143 |
| `eval_scal` + `eval_sum` | party_two.rs | 350-351 |
| `c3` matematik formulu | party_two.rs | 344-351 |
| `decrypt` + final imza | party_one.rs | 376-390 |
| `setup_verify` dogrulamasi | party_two.rs | 233 |
| `CLDLProof.verify` cagrisi | party_two.rs | 235-240 |
| `ct_eq` (constant-time) | party_one.rs | 412 |
| `sample_size(10)` benchmark | keygen.rs | 60 |
| PDL struct'lari (impl yok) | party_one.rs | 102-116 |
| Pi seed (nothing-up-my-sleeve) | test.rs | 49-52 |
| `SECURITY_BITS = 256` | mod.rs | 17 |
