# Agent-04: Lindell 2017 Two-Party ECDSA Analizi

## Kapsanan Dosyalar

| Dosya | Satir Sayisi | Aciklama |
|-------|-------------|----------|
| `src/protocols/two_party_ecdsa/lindell_2017/party_one.rs` | 608 | Party 1 tam implementasyonu |
| `src/protocols/two_party_ecdsa/lindell_2017/party_two.rs` | 424 | Party 2 tam implementasyonu |
| `src/protocols/two_party_ecdsa/lindell_2017/mod.rs` | 24 | Modul tanimi, SECURITY_BITS sabiti |
| `src/protocols/two_party_ecdsa/lindell_2017/test.rs` | 137 | 3 entegrasyon testi |
| `src/protocols/two_party_ecdsa/mod.rs` | 25 | Ust modul (lindell_2017 + cclst_2019) |
| `benches/two_party_ecdsa/lindell_2017/keygen.rs` | 81 | Keygen benchmark |
| `benches/two_party_ecdsa/lindell_2017/sign.rs` | 75 | Sign benchmark |

Referans: Yehuda Lindell, "Fast Secure Two-Party ECDSA Signing" (https://eprint.iacr.org/2017/552.pdf)

---

## 1. Protokol Genel Bakis (2-of-2)

Lindell 2017, iki tarafin (Party 1 ve Party 2) her ikisinin de aktif katilimi olmadan imza uretilemeyen bir 2-of-2 esik imzalama protokoludur. Paillier homomorfik sifrelemesi uzerine insaа edilmistir.

### Temel Fikir

Klasik ECDSA'da `s = k^-1 * (H(m) + r*x) mod q` formulu tek bir tarafca hesaplanir. Lindell 2017'de:

- `x = x1 * x2` (ozel anahtar, birinin `x1`, digerinin `x2` bildigi sekilde paylasılir)
- `k = k1 * k2` (nonce, birinin `k1`, digerinin `k2` bildigi sekilde paylasılir)
- Party 2, Paillier sifrelemesi kullanarak kısmi imza `c3 = Enc(k2^-1 * (H(m) + r*x1*x2))` uretir
- Party 1, `c3`'u Paillier ile cozer ve `k1^-1` ile carparak nihai `s`'yi elde eder

Hicbir taraf `x` veya `k`'nin tamamini bilmez.

### Guvenlik Modeli

- **Semi-honest (honest-but-curious) Party 1**: Protokol kurallarinа uyar ama x2'yi ogrenmeye calisabilir
- **Malicious Party 2**: Protokolden sаpar, sahte imzalar uretmeye calisabilir
- ZK proof'lar malicious Party 2'ye karsi koruma saglar
- Paillier sifrelemesi Party 2'nin x1'i ogrenememesini garantiler

### Sabit

`mod.rs:17` — `SECURITY_BITS: usize = 256`
`party_two.rs:52` — `PAILLIER_KEY_SIZE: usize = 2048`

---

## 2. Party 1 vs Party 2 Rolleri

### Party 1 (Sunucu/Cuzdан)

**Bildikleri:**
- `x1`: Kendi EC ozel anahtar pаyi (Scalar<Secp256k1>)
- `paillier_priv`: Paillier sifresi acma anahtari (DecryptionKey)
- `c_key_randomness`: Paillier sifreleme icin kullanilan rastgelelik (gizli tutulur)
- `k1`: Ephemeral EC ozel anahtar pаyi (imzalama sirasinda)

**Yaptiklarи:**
- KeyGen'de commitment olusturur (pk ve ZK-PoK icin ayri ayri)
- Paillier keypair uretir ve `Enc_ek(x1)` yi Party 2'ye gonderir
- PDL proof ile `Enc_ek(x1)` gercekten `x1`'e karsilik geldigini kanitlar
- Signing'de `c3`'u Paillier ile cozer ve nihai imzayi hesaplar
- `Party1Private` struct'i: `party_one.rs:97-101`

### Party 2 (Istemci/Mobil)

**Bildikleri:**
- `x2`: Kendi EC ozel anahtar pаyi (Scalar<Secp256k1>)
- `ek`: Party 1'in Paillier public key (EncryptionKey)
- `encrypted_secret_share`: `Enc_ek(x1)` — sifreli hali Party 1'in payinin
- `k2`: Ephemeral EC ozel anahtar pаyi (imzalama sirasinda)

**Yaptiklarи:**
- KeyGen'de DLog proof ile x2'yi kanitlar (commitment olmadan)
- Party 1'in commitment/decommitment'ini dogrular
- Paillier correct-key proof'unu dogrular
- PDL proof'unu dogrular
- Signing'de partial sig `c3` hesaplar (Paillier homomorfik islemlerle)
- `Party2Private` struct'i: `party_two.rs:81-84`

### Asimetri Notu

Party 1 commitment yaparken Party 2 yapmaz — bu Lindell makalesiyle uyusur. Party 1'in commitler'i, Party 2'nin x2'sini gonderdikten sonra x1'ini degistirememesini garantiler.

---

## 3. Key Generation: Round-by-Round

### Round 1 — Party 1 Commit

**Party 1** (`party_one.rs:138-177`):

```
x1   <-- rand Scalar<Secp256k1>
Q1   = G * x1           (EC public share)
pi1  = DLogProof::prove(x1)   // Sigma protokolu: ZK-PoK of x1

pk_commitment = Hash(Q1 || pk_blind_factor)
    pk_blind_factor <-- rand 256-bit

zk_pok_commitment = Hash(pi1.pk_t_rand_commitment || zk_pok_blind_factor)
    zk_pok_blind_factor <-- rand 256-bit
```

Gonderilen: `KeyGenFirstMsg { pk_commitment: BigInt, zk_pok_commitment: BigInt }`
Yerel tutulan: `CommWitness { pk_commitment_blind_factor, zk_pok_blind_factor, public_share: Q1, d_log_proof: pi1 }`

**Commitment scheme: Hash-based (Sha256), PEDERSEN DEGIL.**
`party_one.rs:148-158` — `HashCommitment::<Sha256>::create_commitment_with_user_defined_randomness`

### Round 2 — Party 2 Commit + DLog Proof Gonder

**Party 2** (`party_two.rs:141-157`):

```
x2   <-- rand Scalar<Secp256k1>
Q2   = G * x2
pi2  = DLogProof::prove(x2)
```

Gonderilen: `KeyGenFirstMsg { d_log_proof: pi2, public_share: Q2 }`
Party 2 commitment YAPMAZ — dogrudan acik gonderir.

### Round 3 — Party 1 Verify + Decommit

**Party 1** (`party_one.rs:221-229`):

```
DLogProof::verify(pi2)?   // Party 2'nin x2'yi bildigi dogrulanir
```

Gonderilen: `KeyGenSecondMsg { comm_witness: CommWitness }`
Icerik: `{ pk_blind_factor, zk_pok_blind_factor, Q1, pi1 }` — tam decommitment

### Round 4 — Party 2 Verify Commitment + DLog

**Party 2** (`party_two.rs:179-223`):

```
// Commitment kontrolu:
pk_commitment == Hash(Q1 || pk_blind_factor)?
zk_pok_commitment == Hash(pi1.pk_t_rand_commitment || zk_pok_blind_factor)?

// DLog proof kontrolu:
DLogProof::verify(pi1)?
```

### Round 5 — Party 1 Paillier KeyGen + Encrypt

**Party 1** (`party_one.rs:319-337`):

```
(ek, dk) = Paillier::keypair()   // 2048-bit Paillier keypair
rho      <-- Randomness::sample(&ek)
c_key    = Enc_ek(x1; rho)       // x1'i Paillier ile sifirle
```

Yapilari:
- `PaillierKeyPair { ek, dk, encrypted_share: c_key, randomness: rho }` (Party 1'in gizlisi)
- `PaillierPublic { ek, encrypted_secret_share: c_key }` (Party 2'ye giden)

### Round 6 — ZK: Correct Paillier Key Proof

**Party 1** (`party_one.rs:362-364`):

```
proof = NiCorrectKeyProof::proof(&dk, None)
```

**Party 2 verify** (`party_two.rs:302-311`):

```
ek.n.bit_length() >= PAILLIER_KEY_SIZE - 1   // 2047-bit kontrol
proof.verify(ek, SALT_STRING)
```

Amac: Party 1'in gercekten valid bir Paillier keypair'i kullandigini kanit lar. Sahte bir Paillier modulus, ozel anahtar cikarma saldirilarini mumkun kilar.

### Round 7 — ZK: PDL with Slack Proof

**Party 1** (`party_one.rs:366-400`):

```
// N_tilde, h1, h2 olustur:
(ek_tilde, dk_tilde) = Paillier::keypair()
phi = (p-1)(q-1)
h1  <-- rand below phi
xhi <-- rand below 2^256
h2  = h1^(-xhi) mod N_tilde

// PDL-wSlack statement:
PDLwSlackStatement {
    ciphertext: c_key,
    ek: ek,
    Q: G * x1,         // Q1
    G: G,
    h1, h2,
    N_tilde
}

// Witness:
PDLwSlackWitness { x: x1, r: rho }

// Proof:
PDLwSlackProof::prove(&witness, &statement)

// Ayrica composite DLog proof:
CompositeDLogProof::prove(&dlog_statement, &xhi)
```

**Party 2 verify** (`party_two.rs:275-300`):

```
// Statement tutarliligi:
statement.ek == paillier_public.ek
statement.ciphertext == paillier_public.encrypted_secret_share
statement.Q == Q1 (party_one.comm_witness.public_share)

// Prooflari dogrula:
composite_dlog_proof.verify(&dlog_statement).is_ok()
pdl_w_slack_proof.verify(&statement).is_ok()
```

Amac: `c_key = Enc_ek(x1)` dogrulama — Party 1'in, Q1 = G*x1 ile ayni x1'i sifreledigini kanit lar. "Slack" versiyonu cunku standart PDL NP-hard olmayabilir.

### Joint Public Key Hesaplama

**Party 1** (`party_one.rs:231-236`):
```
Q = Q2 * x1   // Q = G*x2*x1 = G*(x1*x2)
```

**Party 2** (`party_two.rs:226-232`):
```
Q = Q1 * x2   // Q = G*x1*x2 = G*(x1*x2)
```

Iki taraf ayni joint public key'i bagımsiz hesaplar. Acik anahtar `Q = G * (x1 * x2)`, gizli anahtar `x = x1 * x2 mod q` (hicbir taraf bu tam degeri bilmez).

---

## 4. Signing: Round-by-Round

### Sign Round 1 — Party 2 Ephemeral Commit

**Party 2** (`party_two.rs:314-371`):

```
k2   <-- rand Scalar<Secp256k1>
R2   = G * k2                    (ephemeral public share)
h    = base_point2()             (ek bir EC generator, NUMS noktasi)
c_2  = h * k2                   (DDH icin)

w    = ECDDHWitness { x: k2 }
delta = ECDDHStatement { g1:G, h1:R2, g2:h, h2:c_2 }
pi_k2 = ECDDHProof::prove(&w, &delta)   // ECDDH ZK proof

pk_commitment    = Hash(R2 || pk_blind)
zk_pok_commitment = Hash(Hash(pi_k2.a1 || pi_k2.a2) || zk_pok_blind)
```

Gonderilen: `EphKeyGenFirstMsg { pk_commitment, zk_pok_commitment }`

### Sign Round 2 — Party 1 Ephemeral (No Commit)

**Party 1** (`party_one.rs:403-434`):

```
k1    <-- rand Scalar<Secp256k1>
R1    = G * k1
h     = base_point2()
c_1   = h * k1

w     = ECDDHWitness { x: k1 }
delta = ECDDHStatement { g1:G, h1:R1, g2:h, h2:c_1 }
pi_k1 = ECDDHProof::prove(&w, &delta)
```

Gonderilen: `EphKeyGenFirstMsg { d_log_proof: pi_k1, public_share: R1, c: c_1 }`
Party 1 commitment YAPMAZ, direkt ECDDH proof gonderir.

### Sign Round 3 — Party 2 Verify Party 1 + Decommit

**Party 2** (`party_two.rs:374-388`):

```
delta = ECDDHStatement { g1:G, h1:R1, g2:h, h2:c_1 }
party_one_first_message.d_log_proof.verify(&delta)?
```

Gonderilen: `EphKeyGenSecondMsg { comm_witness: EphCommWitness { ... R2, pi_k2, c_2 ... } }`

### Sign Round 4 — Party 1 Verify Party 2

**Party 1** (`party_one.rs:436-482`):

```
// Commitment dogrulama:
pk_commitment == Hash(R2 || pk_blind)?
zk_pok_commitment == Hash(Hash(pi_k2.a1 || pi_k2.a2) || zk_pok_blind)?

// ECDDH proof dogrulama:
delta = ECDDHStatement { g1:G, h1:R2, g2:h, h2:c_2 }
pi_k2.verify(&delta)?
```

Gonderilen: `EphKeyGenSecondMsg {}` (bos — sadece onay)

### Sign Round 5 — Party 2 Partial Signature (Critical Step)

**Party 2** (`party_two.rs:390-423`):

```
R       = R1 * k2        // R = G*(k1*k2), gerici hesap
rx      = R.x_coord() mod q

rho     <-- rand BigInt, below q^2
k2_inv  = k2^-1 mod q

// Partial signature hesabi (mod q cinsinden):
partial_sig = rho*q + (k2_inv * H(m) mod q)

c1  = Enc_ek(partial_sig)           // Paillier ile sifirle
v   = k2_inv * rx * x2 mod q
c2  = Paillier::mul(ek, Enc_ek(x1), v)
         = Enc_ek(v * x1) = Enc_ek(k2_inv * rx * x1 * x2)

c3  = Paillier::add(ek, c2, c1)
         = Enc_ek(k2_inv * rx * x1 * x2 + partial_sig)
         = Enc_ek(k2_inv * (H(m) + rx * x1 * x2) + rho*q)
```

Paillier homomorphic islemler:
- `Paillier::mul(ek, ct, pt)` → `Enc_ek(plaintext * pt)`
- `Paillier::add(ek, ct1, ct2)` → `Enc_ek(p1 + p2)`

Gonderilen: `PartialSig { c3: BigInt }`

`rho*q` terimi: Sonucun `mod q` alindiginda kaybolur, ama Paillier plaintextini gizler (blinding).

### Sign Round 6 — Party 1 Final Signature

**Party 1** (`party_one.rs:485-517`):

```
R        = R2 * k1        // R = G*(k2*k1), ayni noktayi bagımsiz hesaplar
rx       = R.x_coord() mod q

k1_inv   = k1^-1 mod q

s_tag    = Paillier::decrypt(paillier_priv, c3)
         // = k2_inv * (H(m) + rx*x1*x2) + rho*q  (mod N_paillier)

s_tag_fe = s_tag mod q    // rho*q terimi burada yok olur
s_tag_tag = s_tag_fe * k1_inv
          // = k1^-1 * k2^-1 * (H(m) + rx*x1*x2) mod q
          // = (k1*k2)^-1 * (H(m) + rx*x)
          // = k^-1 * (H(m) + r*x)   -- standart ECDSA formulu!

s = min(s_tag_tag, q - s_tag_tag)   // Low-S normalizasyonu
```

**Sonuc:** `Signature { s, r: rx }`

### Tam Signing Matematigi

```
x = x1 * x2 mod q           (joint private key)
k = k1 * k2 mod q           (joint nonce)
R = G * k                   (commitment point)
r = R.x mod q
s = k^-1 * (H(m) + r*x) mod q    // standart ECDSA
```

Party 2, `Enc_ek(k2^-1*(H(m)+r*x1*x2))` gonderiyor.
Party 1, bunu decrypt edip `k1^-1` ile carparak standart `s`'yi elde ediyor.

---

## 5. Kriptografik Primitive'ler

### 5.1 Eliptik Egri

- **Egri:** secp256k1 (`curv::elliptic::curves::secp256_k1::Secp256k1`)
- **Skaler:** `Scalar<Secp256k1>` — grup mertebesi `q`
- **Nokta:** `Point<Secp256k1>` — sikistirilmis 33-byte serialize

### 5.2 Hash Fonksiyonu

- **Sha256** — commitment ve DLog proof icin
- `party_one.rs:33` — `use sha2::Sha256`
- `SECURITY_BITS = 256` — commitment rastgeleliginin boyutu

### 5.3 Commitment Scheme (Hash-Based, Pedersen Degil)

`party_one.rs:148-159`:
```
HashCommitment::<Sha256>::create_commitment_with_user_defined_randomness(
    &BigInt::from_bytes(public_share.to_bytes(true).as_ref()),
    &pk_commitment_blind_factor,
)
```

Iki ayri commitment:
1. `pk_commitment` — `Q1` (public share) icin
2. `zk_pok_commitment` — `pi1.pk_t_rand_commitment` (ZK proof'un random commitment'i) icin

### 5.4 DLog Proof (Sigma Protokolu)

**Keygen'de:** `DLogProof<Secp256k1, Sha256>` — Schnorr sigma protokolu
`curv::cryptographic_primitives::proofs::sigma_dlog::DLogProof`

**Signing'de:** `ECDDHProof<Secp256k1, Sha256>` — DDH sigma protokolu
`curv::cryptographic_primitives::proofs::sigma_ec_ddh::ECDDHProof`

Party 1 keygen'de: `DLogProof::prove(x1)` — `party_one.rs:145`
Party 2 keygen'de: `DLogProof::prove(x2)` — `party_two.rs:145`
Party 1 signing'de: `ECDDHProof::prove` — `party_one.rs:420`
Party 2 signing'de: `ECDDHProof::prove` — `party_two.rs:334`

Fark: Keygen'de standart DLog (tek generator G), signing'de ECDDH (iki generator G ve h=base_point2) kullanilir.

ECDDH proof neden gerekli: Signing'de sadece R = G*k kanit i yeterli degil; ayrica c = h*k kanit i gereklidir. Bu iki ikili yapisi, Paillier ile guvenli kombinasyonu saglar. Sahte bir k ile R olusturmak ancak discrete log bilgisiyle mumkundu.

### 5.5 Paillier Homomorfik Sifreleme

**Paillier modul boyu:** 2048 bit
`party_two.rs:52` — `PAILLIER_KEY_SIZE: usize = 2048`
`party_two.rs:307` — `ek.n.bit_length() >= PAILLIER_KEY_SIZE - 1` (2047-bit minimum)

Kullanilan islemler:
- `Paillier::keypair().keys()` — Party 1'de keygen
- `Paillier::encrypt_with_chosen_randomness` — `Enc_ek(x1; rho)` ile deterministik sifreleme
- `Paillier::encrypt(ek, partial_sig)` — Party 2'de c1 uretimi
- `Paillier::mul(ek, ct, pt)` — Skaler carpma (homomorfik)
- `Paillier::add(ek, ct1, ct2)` — Toplama (homomorfik)
- `Paillier::decrypt(dk, ct)` — Party 1'de c3 cozme

### 5.6 ZK Correct Paillier Key Proof

`NiCorrectKeyProof` — Non-interactive, parti 1'in Paillier dk'sinin ge cerli old ugunu kanitlar.
`party_one.rs:362-364`, `party_two.rs:302-311`

### 5.7 PDL with Slack Proof

`PDLwSlackProof` + `PDLwSlackStatement` + `PDLwSlackWitness`
`party_one.rs:366-400`, `party_two.rs:275-300`

"PDL" = Paillier Discrete Log — `c_key = Enc_ek(x1)` ifadesinin dogrulugunu kanitlar.
"Slack" = gevse, cunku tam PDL NP-hard olmayabilir, esneklik verir.

### 5.8 Composite DLog Proof

`CompositeDLogProof` — `N_tilde` kompozit modulus uzerinde DLog proof.
`h2 = h1^(-xhi) mod N_tilde` iliski kanit edilir.
`party_one.rs:376-376`

Bu, PDL with Slack icin gerekli h1, h2, N_tilde parametrelerinin dogru uretildigini dogrular.

### 5.9 Verifiable Recovery (centipede)

`party_one.rs:298-307`, `party_two.rs:248-257`:
```
Msegmentation::to_encrypted_segments(&self.x1, &segment_size, num_of_segments, pub_ke_y, g)
```
Ozel anahtari sifreleyerek kurtarma mekanizmasi. Helgamal segmentation kullanır.

### 5.10 Low-S Normalizasyonu

`party_one.rs:511-513`:
```rust
let s = cmp::min(
    s_tag_tag_bn.clone(),
    Scalar::<Secp256k1>::group_order().clone() - s_tag_tag_bn,
);
```
ECDSA mallealibility karsi onlem. BIP-62 uyumlu.

---

## 6. Test ve Benchmark Analizi

### 6.1 Testler (test.rs)

**Test 1: `test_d_log_proof_party_two_party_one`** (satir 9-24)
Sadece keygen'in ilk 4 round'unu test eder (Paillier setup olmadan).
- Party 1 commit → Party 2 DLog proof → Party 1 verify+decommit → Party 2 verify

**Test 2: `test_full_key_gen`** (satir 28-82)
Tam keygen (Paillier + PDL dahil).
- `x1 = BigInt::sample(253)` — 253-bit rastgele (Party 1 share)
- `x2 = BigInt::from(10)` — Sabit 10 (test icin; Party 2 share)
- Paillier keypair + correct key proof + PDL proof

**Test 3: `test_two_party_sign`** (satir 85-137)
Tam signing testi.
- Message: `BigInt::from(1234)`
- Tam protokol akisi: ephemeral keygen → partial sig → final sig → verify
- `party_one::verify()` ile imza dogrulama

### 6.2 Benchmark Yapisi

Her iki benchmark da `Criterion::default().sample_size(10)` kullanir — sadece 10 ornek.

**keygen.rs benchmark:**
- Olculen: Party 1 commit, Party 2 DLog, Party 1 decommit, Party 2 verify, Paillier keypair, encrypt, NiCorrectKey proof, PDL proof
- HEPSI tek fonksiyon icinde olculur: `bench_full_keygen_party_one_two`

**sign.rs benchmark:**
- Keygen setup'i da iceriyor (benchmark icinde) — yani sadece signing degil, tam akis
- Olculen: ephemeral keygen (her iki taraf) + partial sig + final sig + verify

Kesin ms degerleri bu kaynak dosyalarinda yok (runtime'da olur). Referans olarak:
- Paillier 2048-bit keygen: ~200-500ms (dominant islem)
- Paillier encrypt/decrypt: ~5-20ms
- EC scalar carpma: <1ms
- Toplam keygen: ~300-700ms (donanim bagimlı)
- Toplam sign: ~10-50ms (Paillier keygen yoksa)

### 6.3 Refresh Mekanizmasi

`party_one.rs:246-296` — `Party1Private::refresh_private_key`:
- `x1_new = x1 * factor`
- `x2_new = x2 / factor` (baska yerde)
- Yeni Paillier keypair uretilir
- Tam PDL proof yeniden yapilir

Bu, proaktif guvenlik icin key refresh imkani saglar.

---

## 7. Guvenlik Degerlendirmesi

### 7.1 2-of-2 Zorunlulugu

Her iki tarafın aktif katilimi zorunludur:
- Signing: `c3 = Enc_ek(k2^-1*(H(m)+r*x1*x2))` — Paillier decrypt icin `dk` gerekli (Party 1'de), `c3` uretmek icin `x2` ve `k2` gerekli (Party 2'de)
- Party 1 olmadan: `c3` decrpyt edilemez
- Party 2 olmadan: `c3` hiç uretilmez

### 7.2 Malicious Party 2 Senaryolari

**Senaryo 1: Yanlis c3 gonderme**
Party 2, gecersiz bir `c3` gonderirse imza gecersiz olur. Ama bu Party 2'ye zarar verir, saldiri degildir.

**Senaryo 2: Farkli mesaj icin partial sig**
Party 2 `H(m')` yerine `H(m)` kullansa, Party 1 bunu anlayamaz. HATA: Bu protokolde Party 2 hangi mesaji imzaladigini secmekte ozgurdur. Bu, "Party 1 mesaj dogrulama yapmiyor" problemine yol acar.

**Senaryo 3: Paillier ciphertext manipulation**
Party 2, `Enc_ek(x1)` uzerinde homomorfik islem yaparak `x1`'i dogrudan cikaramaz — Paillier IND-CPA guvenlidir.

**Senaryo 4: k2 tekrar kullanimi**
Party 2 ayni k2'yi farkli mesajlar icin kullanirsa, Party 1 iki imzayi analiz ederek `x2`'yi cikarabilir. Ama bu Party 2'ye zarar verir.

**Senaryo 5: Sahte Range Proof Yoklugu**
Party 2'nin `rho*q` blinding'i olmasa, Paillier aritmetigi `k2^-1*(H(m)+r*x1*x2)` degerini ciphertext'ten sizmaya karsi guclu degil. `rho*q` blinding'i kritik.

### 7.3 Range Proof Gerekliligi

Kodda signing sirasinda range proof YOK. Bu, Lindell 2017 makalasinin birkac versiyonunda ele alinan bir issue'dur.
Paillier plaintext `N_paillier >> q` oldugunda, `partial_sig mod N_paillier` degeri `mod q`'ya indirgemeden sizmayabilir.
`rho*q` blinding terimi bu riski azaltir ama tam range proof kadar guclu degildir.

Bu, GG18'in neden range proof eklediginin temel nedenidir.

### 7.4 PDL Proof Gerekliligi

PDL proof olmadan, Party 1 sahte bir `c_key = Enc_ek(x1')` gonderebilir (x1 != x1') ve sonucta x2'yi cikarabilir. PDL with Slack, `c_key`'in gercekten `Q1` ile uyumlu `x1`'i sifreledigini garanti altina alir.

### 7.5 ECDDH vs DLog Proof (Signing'de)

Signing'de neden DLog degil ECDDH:
Paillier ile birlikte guvenli kanit icin `k1` ve `k2`'nin hem `R = G*k` hem de `c = h*k` uzerindeki bilgisi kanit lanmalidir. Bu iki generator kanitlamasi, Paillier ile combining sirasinda dogrusal saldirilan onler.

### 7.6 Malleable Signature Sorunu

`party_one.rs:585-586` — `verify` fonksiyonu Low-S kontrolu yapar:
```rust
signature.s < Scalar::<Secp256k1>::group_order() - signature.s.clone()
```
Bu, ECDSA imzasinin tek bir kanonik formunun olmasini zorlar (BIP-62).

---

## 8. GG18/GG20 ile Karsilastirma

### Mimari Fark

| Ozellik | Lindell 2017 | GG18/GG20 |
|---------|-------------|-----------|
| Taraf sayisi | 2 (sabit) | n-of-n veya t-of-n |
| Esik | 2-of-2 | t-of-n (genel) |
| Paillier kullanimi | Evet (Party 1'in anahtari) | Evet (her taraf icin) |
| DLog proof | Sigma (Schnorr) | Schnorr |
| Ephemeral nonce proof | ECDDH | Feldman-VSS + ZK |
| Round sayisi (sign) | 6 (3 tur) | 10+ (4-5 tur) |
| MtA (Multiplicative-to-Additive) | Dolaylı Paillier | Acik MtA |
| Range proof (sign) | Hayir (rho*q blinding) | Evet (zorunlu) |
| Verimliligi | Daha az round | Daha fazla round |

### Kriptografik Guvenlik Farklari

**GG18 Zayifliklari (2021'de tespit edildi):**
GG18'de `alpha = k1^-1 * m + k1^-1 * r * x1` kisminda range proof eksikligi, Party 2'nin specially crafted Paillier ciphertext ile `x1`'i cikarmasina izin verebilir.

**Lindell 2017 Avantaji:**
2-party oldugu icin attack surface daha kucuk. Sadece bir Party 2 var ve PDL/correct-key proof ile denetleniyor.

**GG20 (Canetti et al.):**
GG18'in range proof sorunlarini cozer, ama daha karmasik.

### Ne Zaman Hangisi Tercih Edilir

**Lindell 2017:**
- 2 tarafli cuzdan mimarileri (mobil + sunucu)
- Daha az round = daha az latency
- Daha basit implementasyon
- Callee: Lightning Network, non-custodial wallets

**GG18/GG20:**
- 3+ tarafli setup gerektiren senaryolar
- t-of-n flexibility istenen durumlar
- Merkezi olmayan validatorlar
- Chain signature schemes (ICP tarzı)

---

## 9. Nova Shield Chain Icin Cikarsimlar

### 9.1 2-Party vs Multi-Party Karar

Eger Nova Shield Chain, kullanici tarafli key yonetimi (mobil cuzdan + sunucu imzalayici) uzerine kuruluyorsa Lindell 2017 idealdir. Eger validator network geneli icin kullanilacaksa GG20 veya CGGMP21 tercih edilmelidir.

### 9.2 Paillier Key Boyu

2048-bit Paillier kullanilmali — `party_two.rs:52` bunu enforce eder. 1024-bit Paillier guvenli degil, Lindell makalesinde de belirtilmistir.

### 9.3 Commitment Scheme Secimi

Bu implementasyon hash-based commitment kullanir (SHA256, Pedersen degil). SHA256 collision resistance yeterlidir, ama Pedersen commitment hiding ozelligi farkli guvenlik kanitlari saglayabilir. Nova Shield icin hash commitment yeterli.

### 9.4 Refresh Mekanizmasi

`party_one.rs:246-296` — `refresh_private_key` fonksiyonu proaktif guvenlik icin kritik. Key share'ler periyodik olarak refresh edilmeli. Nova Shield, node restart / slashing senaryolarinda bu mekanizmayi kullanabilir.

### 9.5 Verifiable Recovery

`centipede` kutuphanesi ile sifrelenmis key recovery saglanmis. Nova Shield icin onemli: kullanicinin cihazi kaybolursa x2'si recovery edilebilir.

### 9.6 Low-S Normalizasyonu

`party_one.rs:511-513` — Zaten uygulanmis. Ethereum ve Bitcoin ile uyumlu.

### 9.7 Message Dogrulama Eksikligi

Kritik: Protokolde Party 1, Party 2'nin hangi mesaji imzalattigini dogrulamaz. Nova Shield icin transaction dogrulama, imzalama katmaninin uzerinde (uygulama katmaninda) yapilmali.

### 9.8 Signing Round Sayisi

Pratik signing: 6 mesaj alisi-verisi (3 tam tur).
- Tur 1: Party 2 commit + Party 1 acik gonder
- Tur 2: Party 2 verify+decommit + Party 1 verify
- Tur 3: Party 2 partial sig + Party 1 final sig

Bu, GG18'e gore daha az rounddur ve dusuk latency saglar — Nova Shield P2P aginda onemli.

### 9.9 cclst_2019 Alternatifl

`two_party_ecdsa/mod.rs:23-24` — Class Group tabanli iki tarafli ECDSA (feature flag ile). Bu, Paillier'e alternatif olarak daha kucuk key boyutu saglar ama daha az test edilmistir.

### 9.10 Kritik Sayisal Degerler

```
SECURITY_BITS           = 256     (commitment rastgeleligini)
PAILLIER_KEY_SIZE       = 2048    (Paillier modul boyu, bit)
EC group order q        = secp256k1 orderi (~2^256)
rho                     <-- rand below q^2 (blinding)
pk_blind_factor         <-- rand 256-bit
zk_pok_blind_factor     <-- rand 256-bit
h1 blinding             <-- rand below phi(N_tilde)
xhi                     <-- rand below 2^256
```

---

## Ozet Tablo

| Adim | Kim | Islem | Kriptografi |
|------|-----|-------|-------------|
| KeyGen-1 | Party 1 | Commit(Q1, pi1) | HashCommitment, DLogProof |
| KeyGen-2 | Party 2 | Q2, pi2 | DLogProof |
| KeyGen-3 | Party 1 | Verify(pi2), Decommit | DLogProof.verify |
| KeyGen-4 | Party 2 | Verify(Commit, pi1) | HashCommitment.verify, DLogProof.verify |
| KeyGen-5 | Party 1 | Paillier keygen, Enc(x1) | Paillier::keypair, ::encrypt |
| KeyGen-6 | Party 1 | NiCorrectKey proof | ZK |
| KeyGen-7 | Party 1 | PDL-wSlack proof | ZK, CompositeDLog |
| KeyGen-V | Party 2 | Verify NiCorrectKey + PDL | ZK verify |
| Sign-1 | Party 2 | Commit(R2, pi_k2) | HashCommitment, ECDDHProof |
| Sign-2 | Party 1 | R1, pi_k1 (no commit) | ECDDHProof |
| Sign-3 | Party 2 | Verify(pi_k1), Decommit | ECDDHProof.verify |
| Sign-4 | Party 1 | Verify(Commit, pi_k2) | HashCommitment.verify, ECDDHProof.verify |
| Sign-5 | Party 2 | c3 = Enc(k2^-1*(H(m)+r*x1*x2)) | Paillier::mul, ::add |
| Sign-6 | Party 1 | s = Dec(c3) * k1^-1 | Paillier::decrypt, Low-S |
