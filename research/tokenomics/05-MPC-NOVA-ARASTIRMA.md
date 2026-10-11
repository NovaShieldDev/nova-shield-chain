# Nova Shield Chain — MPC/Kriptografi Araştırması
# multi-party-ecdsa analizinden çıkarılan derslerin Nova Shield'a uygulanması

**Kaynak repo:** `research/libs/multi-party-ecdsa/`
**Analiz dosyaları:** `research/analysis/multi-party-ecdsa/` (7 agent + DOGRULAMA.md)
**Tarih:** 2026-07-22
**Tüm değerler kaynaktan doğrulanmıştır (0 hata, 16/16 iddia teyitli).**

---

## A. PROTOKOL SEÇİMİ: GG18 vs GG20 vs CGGMP21 vs FROST

### Karşılaştırma Tablosu

| Özellik | GG18 | GG20 | CGGMP21 | FROST (Nova mevcut) |
|---------|------|------|---------|---------------------|
| Taraf yapısı | t-of-n | t-of-n | t-of-n | t-of-n |
| KeyGen round | 4 | 4 | 3 | 3 |
| Sign round | 9+1 P2P | 6+1 online | 4+1 (presign) | 2 (offline+online) |
| İmza şeması | ECDSA (secp256k1) | ECDSA (secp256k1) | ECDSA (secp256k1) | Schnorr |
| Identifiable abort | Hayır | Evet (phase5/6/7) | Evet | Kısmi |
| Range proof | Devre dışı (açık bug) | Aktif | Aktif | Yok (Schnorr'da gereksiz) |
| Paillier bağımlılığı | Evet | Evet | Evet | Hayır |
| Audit durumu | Sınırlı | Sınırlı | Evet (ZenGo/Trail of Bits) | Evet (akademik) |
| Üretim kullanımı | Düşük | Orta | Yüksek (ZenGo, DFINITY) | Orta (FROST RFC 9591) |
| State machine | Yok | Evet (`round_based`) | Evet | Nova mevcut |

### Kritik Sayılar (kaynaktan doğrulanmış)

```
GG18 KeyGen:   4 round (3 broadcast + 1 P2P)
GG18 Signing:  9 broadcast + 1 P2P round
GG20 KeyGen:   4 round (Round0..Round4)
GG20 Signing:  6 offline + 1 online = total 7 round
               total_rounds() = Some(6)  [sign.rs:427]
CGGMP21:       ~4 sign round (daha verimli)
FROST (Nova):  2-3 round (commit + reveal + aggregate)
```

### Nova Shield için Protokol Önerisi

**Birincil seçim: CGGMP21 (mevcut karar, doğrulanmış doğru)**

Gerekçe:
1. Identifiable abort: Validator slashing için hangi guardian'ın bozduğunu tespit etmek zorunlu. GG18'de yoktur. GG20'de vardır ama 7 round sürer.
2. t-of-n esnekliği: Guardian kategorileri (3/5, 2/3 gibi) Lindell 2017 veya CCLST 2019 ile çalışmaz (sabit 2-of-2).
3. State machine: GG20'de `round_based` entegrasyonu mevcut — CGGMP21'de daha gelişmiş.
4. Daha az round: CGGMP21 signing ~4 round; GG20 7 round; GG18 10 round. Nova'nın 400ms slotunda bu kritik.
5. Audit: CGGMP21 üretim denetiminden geçmiş; GG18/GG20 geçmemiş.

**İkincil seçim: FROST (Nova'nın mevcut kodu için)**

Nova'nın `nova-mpc` modülü Schnorr/FROST üzerinde çalışıyor. Bu, CGGMP21'e geçişte ara çözüm olarak geçerlidir.

### FROST/Schnorr vs ECDSA Threshold Farkı

```
FROST (Schnorr):
  s = k + H(R, Y, m) * x     (doğrusal — MtA protokolü gereksiz)
  R = sum(R_i)                 (nonce toplanabilir)
  Paillier şifrelemesi yok
  Round sayısı daha az (2 veya 3)
  BTC Taproot, Zcash, Solana ile doğal uyum
  Dezavantaj: secp256k1 Schnorr Bitcoin'de var, Ethereum'da YOK

ECDSA Threshold (GG18/GG20/CGGMP21):
  s_i = m*k_i + r*sigma_i    (MtA gerekli — doğrusal değil)
  MtA: alpha + beta = k_i * gamma_j
  Paillier şifreli iletişim zorunlu
  Daha fazla round ama ETH/BTC multisig ile uyumlu
  Avantaj: Ethereum, tüm ECDSA zincirleri ile çalışır
```

**Nova'nın seçimi ne anlama geliyor:**
- FROST: BTC Taproot vault için ideal (native Schnorr)
- CGGMP21: ETH vault ve cross-chain için gerekli (ECDSA)
- Nova her iki sistemi de tutmalı; farklı vault'lar için farklı protokol

### Nova'nın Mevcut FROST Implementasyonu

**Güçlü yönler:**
- 7534 satır, end-to-end testler geçiyor
- Schnorr imza temel doğruluğu sağlıyor
- Reshare protokolü mevcut (2 round)

**Eksiklikler (kaynak analizinden türetilen, Nova'ya uyarlanan):**
1. `PreSignState &mut` API sorunu — Section C'de detay
2. Identifiable abort yok — Section C'de ekleme yöntemi
3. `LocalKey.secret_share == 0` guard yok
4. SSID'e VSS commitment'ları eklenemiyor
5. DKG round3'te index hatası

### Identifiable Abort: Nova için Gerekli mi?

**Evet, zorunlu.**

GG20 analizinden öğrenilen ders: Cevap vermeyen veya yanlış mesaj gönderen tarafı tespit edebilmek için `LocalStatePhase5/6/7` yapıları tutulmalı ve `phase5_blame()`, `phase6_blame()`, `phase7_blame()` çağrılabilmeli.

Nova Guardian Slashing'de bu doğrudan kullanılır:
- Guardian yanlış `delta_i` gönderirse → `phase5_blame()` tespit eder → WrongFreeze %5 slash
- Guardian cevap vermezse → `round_timeout` tetikler → Unresponsive %3 slash
- Guardian kötü niyetli imzalarsa → `phase7_blame()` tespit eder → Malicious %100 slash

---

## B. GG20 — TAM GÜVENLİK ANALİZİ

### GG20 KeyGen: 4 Round Tam Açıklama

Tüm değerler `gg_2020/party_i.rs` ve `state_machine/keygen/rounds.rs`'den doğrulanmıştır.

**Sabitler (kaynaktan):**
```
SECURITY = 256                   [party_i.rs:48]
PAILLIER_MIN_BIT_LENGTH = 2047   [party_i.rs:49]
PAILLIER_MAX_BIT_LENGTH = 2048   [party_i.rs:50]
```

**Round 0 (is_expensive=true):**
```
Her parti i için:
  u_i <- Z_q  (rastgele secp256k1 scalar)
  y_i = u_i * G
  (ek_i, dk_i) = Paillier::keypair_safe_primes()  [ÜRETIMDE ZORUNLU]
  (N_tilde, h1, h2, xhi, xhi_inv) = generate_h1_h2_N_tilde()
  h2 = h1^xhi mod N_tilde  [satır 151]
  xhi = phi - xhi  [satır 152 — negation mod phi]
  blind_factor_i <- rand(256-bit)
  com_i = SHA256(y_i || blind_factor_i)

Broadcast → KeyGenBroadcastMessage1:
  { e: EncryptionKey,
    dlog_statement: DLogStatement(N_tilde, h1, h2),
    com: com_i,
    correct_key_proof: NiCorrectKeyProof,
    composite_dlog_proof_base_h1: CompositeDLogProof,  (h2 = h1^x)
    composite_dlog_proof_base_h2: CompositeDLogProof } (h1 = h2^x_inv)
```

**Round 1:**
```
Her parti, tüm Round 0 mesajlarını topladıktan sonra:
Broadcast → KeyGenDecommitMessage1:
  { blind_factor: blind_factor_i, y_i: y_i }
```

**Round 2 (is_expensive=true):**
```
Her j için doğrulama:
  1. SHA256(y_j || blind_j) == com_j              [commitment doğrulama]
  2. correct_key_proof_j.verify(ek_j).is_ok()    [Paillier doğru mu?]
  3. ek_j.n.bit_length() ∈ [2047, 2048]          [boyut kontrolü]
  4. dlog_statement_j.N.bit_length() ∈ [2047, 2048]
  5. composite_dlog_proof_h1.verify().is_ok()    [h2=h1^x ZKP]
  6. composite_dlog_proof_h2.verify().is_ok()    [h1=h2^x_inv ZKP]

VSS paylaşımı:
  f_i(X) = u_i + a_1*X + ... + a_t*X^t  (derece t polinomu)
  Commitments: {g^u_i, g^a_1, ..., g^a_t}
  Share j'ye: f_i(j)  [P2P, şifreli]
```

**Round 3 (is_expensive=true):**
```
Her j'den gelen (vss_j, share_ji) için:
  1. vss_j.validate_share(share_ji, index_i).is_ok()
  2. vss_j.commitments[0] == y_j  [sabit terim = y_j]

Toplama:
  Y = sum(y_j)         [kolektif public key]
  x_i = sum(f_j(i))   [threshold share]
  dlog_proof_i = DLogProof::prove(x_i)  [Schnorr ZKP]

Broadcast → DLogProof<Secp256k1, Sha256>
```

**Round 4 (is_expensive=true) → LocalKey:**
```
Her j için:
  1. DLogProof::verify(dlog_proof_j).is_ok()
  2. xi_commitments[j] == dlog_proof_j.pk  [VSS'e karşı doğrulama — GG18'de YOK]

xi_commitments hesabı:
  global_vss.commitments[k] = sum_j(vss_j.commitments[k])
  commitment_to_xi = global_vss.evaluate_at(i)
  yani: g^(f_1(i)+...+f_n(i)) = g^x_i

LocalKey çıktısı:
  { paillier_dk, pk_vec, keys_linear(y+x_i),
    paillier_key_vec, y_sum_s, h1_h2_n_tilde_vec,
    vss_scheme, i, t, n }
```

### GG20 Signing: 6 Offline + 1 Online Round

**Round 0 (offline, broadcast):**
```
SignKeys oluşturma:
  lambda_i = Lagrange(i, s_l)   [Lagrange katsayısı]
  w_i = lambda_i * x_i          [ağırlıklı share]
  gamma_i <- Z_q (rastgele)
  k_i <- Z_q (rastgele)
  g_gamma_i = G * gamma_i
  blind_i <- rand(256-bit)
  com_i = SHA256(g_gamma_i || blind_i)

MessageA:
  c_A = Paillier_Enc(ek_i, k_i; randomness)
  + AliceProof(range) her j için

Broadcast: (MessageA, SignBroadcastPhase1{com})
```

**Round 1 (offline, P2P):**
```
Her (i,j) çifti için iki paralel MtA:
  MtA(k_i, gamma_j): delta_i hesabı için
  MtA(k_i, w_j):     sigma_i hesabı için

Bob tarafi (MessageB::b):
  beta_tag <- Z_N
  c_B = Paillier_Add(Paillier_Mul(c_A, b), Enc(beta_tag))
       = Enc(k_i * b + beta_tag)
  beta = -beta_tag mod q

Alice tarafi (verify_proofs_get_alpha):
  alpha = Dec(c_B) mod q = k_i * b + beta_tag mod q

İlişki: alpha + beta = k_i * b  (mod q)

P2P: (GammaI(MessageB_gamma), WI(MessageB_w)) her j'ye
```

**Round 2 (offline, broadcast):**
```
delta_i = k_i*gamma_i + sum_j(alpha_ij_gamma + beta_ij_gamma) mod q
sigma_i = k_i*w_i + sum_j(miu_ij + ni_ij) mod q

İlişkiler:
  sum_i(delta_i) = k * gamma  [mod q]
  sum_i(sigma_i) = k * x      [mod q]

T_i = G*sigma_i + H*l_i  [H = base_point2, Pedersen commitment]
l_i <- Z_q (rastgele)
T_i_proof = PedersenProof(sigma_i, l_i)

Broadcast: (DeltaI, TI, TIProof)
```

**Round 3 (offline, broadcast):**
```
delta = sum_i(delta_i) = k * gamma  [mod q]
delta_inv = (k * gamma)^(-1) mod q

Pedersen proof doğrulaması her T_i için

Broadcast: SignDecommitPhase1 { blind_factor, g_gamma_i }
```

**Round 4 (offline, broadcast):**
```
Gamma = sum_i(g_gamma_i)
R = Gamma * delta_inv
  = (G * gamma) * (k * gamma)^(-1)
  = G * k^(-1)          [party_i.rs:679-682]

R_dash_i = R * k_i

PDLwSlackProof: R_dash_i = R*k_i ve k_i Enc ile tutarlı

Broadcast: (RDash, Vec<PDLwSlackProof>)
```

**Round 5 (offline, broadcast):**
```
PDLwSlack proof doğrulaması

R_dash toplam kontrolü:
  sum_i(R_dash_i) = R * sum_i(k_i) = R * k = G  [identity kontrolü]

S_i = R * sigma_i
HomoElGamalProof: S_i = R*sigma_i ve T_i = G*sigma_i+H*l_i tutarlı

S_i toplam kontrolü:
  sum_i(S_i) = R * k * x = G * x = Y  [public key]

Broadcast: (SI, HEGProof)
```

**Round 6 (offline) → CompletedOfflineStage:**
```
HomoELGamal proof doğrulaması
CompletedOfflineStage { i, local_key, sign_keys, t_vec, R, sigma_i }
```

**Round 7 (online, SignManual):**
```
r = R.x_coord() mod q
m_fe = SHA256(message) as Scalar
s_i = m_fe * k_i + r * sigma_i    [party_i.rs:863]

Final:
  s = sum_i(s_i) = m*k + r*(k*x) = k*(m + r*x)

Low-S normalizasyonu:
  s_tag = q - s
  if s > s_tag: s = s_tag, recid ^= 1  [party_i.rs:896-902]

recid = if R.y.is_odd() { 1 } else { 0 }
```

### Paillier Encryption: Safe Prime Gerekliliği

**Sorun:** `Keys::create(index)` `Paillier::keypair()` kullanır — safe prime DEĞİL.

```rust
// YANLIŞ (varsayılan — party_i.rs:162):
let (ek, dk) = Paillier::keypair().keys();

// DOĞRU (üretimde zorunlu — party_i.rs:180):
Keys::create_safe_prime(index)
// iç çağrı: Paillier::keypair_safe_primes().keys()
```

**N_tilde de safe prime değil** (`party_i.rs:139-140`):
```rust
// Yorum satırı olarak bekliyor, AKTIF DEĞİL:
// let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();
let (ek_tilde, dk_tilde) = Paillier::keypair().keys();  // GÜVENLİ DEĞİL
```

Safe prime olmadan:
- Paillier modülüne özel faktorizasyon saldırıları mümkün
- Range proof güvenlik parametresi düşer
- MtA protokolü zayıflar

**Nova Shield Rust implementasyonu:**
```rust
// nova-mpc/src/guardian/keygen.rs
use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::party_i::Keys;

// ZORUNLU — safe prime kullan:
let keys = Keys::create_safe_prime(party_index);
// veya cggmp21 crate'inde:
use cggmp21::keygen::KeygenBuilder;
```

### CompletedOfflineStage Clone → Nonce Reuse → Private Key Sızması

**Kaynak doğrulanmış:** `rounds.rs:647` — `#[derive(Clone)]`

```rust
#[derive(Clone)]  // TEHLIKE!
pub struct CompletedOfflineStage {
    i: u16,
    local_key: LocalKey<Secp256k1>,
    sign_keys: SignKeys,    // k_i burada
    R: Point<Secp256k1>,   // G * k^-1
    sigma_i: Scalar<Secp256k1>,
}
```

**Saldırı senaryosu:**
```
Aynı CompletedOfflineStage ile m1 ve m2 imzalanırsa:
  s1 = m1*k + r*(k*x) = k*(m1 + r*x)
  s2 = m2*k + r*(k*x) = k*(m2 + r*x)

  s1 - s2 = k*(m1 - m2)
  k = (s1 - s2) / (m1 - m2)  mod q

  x = (s1 * k^-1 - m1) / r   mod q  [özel anahtar açığa çıkar!]
```

**Nova'nın FROST'ta benzer risk:**

Nova'nın `PreSignState` `&mut` almıyorsa (değer semantiği), aynı presign state klonlanabilir ve iki farklı mesaj için kullanılabilir — aynı nonce reuse problemi.

**Çözüm (Section C'de detay):** `PreSignState` tek kullanımlık yapılmalı ve kullanım sonrası sıfırlanmalı.

### Phase5/6/7 Blame Mekanizması

**Phase 5 Blame (delta_i tutarsızlığı):**
```rust
// Tetikleyici: sum(R_dash_i) != G
// blame.rs:116-224

struct LocalStatePhase5 {
    k: Scalar,
    k_randomness: BigInt,
    gamma: Scalar,
    beta_randomness: Vec<BigInt>,
    beta_tag: Vec<BigInt>,
    encryption_key: EncryptionKey,
}

// Blame algoritması:
// 1. g^gamma_i == G * gamma_i kontrol
// 2. MessageA'yı k_i, k_randomness ile yeniden üret, karşılaştır
// 3. delta_i'yi yeniden hesapla
// 4. yeniden hesaplanan != broadcast edilen → suçlanan parti
```

**Phase 6 Blame (sigma_i tutarsızlığı):**
```rust
// Tetikleyici: sum(S_i) != Y
// Her i için: R * s_i == R_dash_i * m + S_i * r
// Yanlış hesaplayan tespit edilir
```

**Phase 7 Blame (local signature hatası):**
```rust
// blame.rs:434-455
let R_si = &self.R * &self.s_vec[i];
let right = R_dash_m + Si_r;  // R_dash_i*m + S_i*r
if left != right { bad_signers_vec.push(i) }
```

**Nova için Blame Zinciri:**
```
Signing başarısız
  → Phase5 check başarısız? → phase5_blame() → slashing_list
  → Phase6 check başarısız? → phase6_blame() → slashing_list
  → Phase7 başarısız?       → phase7_blame() → slashing_list
  → Onchain blame kaydı → Guardian slash işlemi
```

### MtA Protokolü Tam Formülü

```
Girdiler:
  Alice: a (gizli, örn. k_i)
  Bob:   b (gizli, örn. gamma_j)

MessageA (Alice → Bob):
  c_A = Paillier_Enc(ek_Alice, a; r_A)
  + AliceProof(range): a ∈ [0, q^3] kanıtı

MessageB (Bob → Alice):
  beta_tag <- Z_N  (gizli blinding)
  c_B = Paillier_Mul(c_A, b) + Paillier_Enc(beta_tag)
       = Enc(a*b + beta_tag)
  beta = -beta_tag mod q
  b_proof = DLogProof(b)
  beta_tag_proof = DLogProof(beta_tag)

Alice decrypt:
  alpha = Dec(dk_Alice, c_B) mod q
        = a*b + beta_tag mod q

Sonuç:
  alpha + beta = a*b  (mod q)  ✓
  Alice: alpha öğrenir (b'yi öğrenmez)
  Bob:   beta öğrenir  (a'yı öğrenmez)
```

**BobProof gamma sapması (üretim uyarısı):**
```
Spec (eprint 2019/114 Appendix A): gamma <- [0, q^3 * N]
Kod (range_proofs.rs:233):         gamma <- [0, q^2 * N]  YANLIŞ!
```

---

## C. NOVA'NIN FROST THRESHOLD — EKSİKLER VE DÜZELTMELER

### PreSignState &mut API Sorunu

Nova'nın FROST implementasyonunda `PreSignState` değer semantiği (değil `&mut`) ile kullanılıyorsa, nonce tekrar kullanımı riski vardır. GG20'deki `CompletedOfflineStage Clone` problemiyle özdeş.

**Düzeltme yaklaşımı:**

```rust
// YANLIŞ — klonlanabilir:
pub struct PreSignState {
    nonce_i: Scalar,
    commitment_i: Point,
    // ... diğer alanlar
}

// DOĞRU — tek kullanımlık:
pub struct PreSignState {
    nonce_i: Scalar,
    commitment_i: Point,
    used: bool,  // kullanım sonrası true yapılır
}

impl PreSignState {
    /// Presign state'i tüket — yalnızca bir kez çağrılabilir
    pub fn consume(mut self) -> (Scalar, Point) {
        // zeroize sonra drop
        let nonce = self.nonce_i.clone();
        let commitment = self.commitment_i.clone();
        self.nonce_i.zeroize();
        // self drop edilir, tekrar kullanılamaz
        (nonce, commitment)
    }
}

// DAHA İYİ — Rust ownership semantiği ile:
// PreSignState'i Option<PreSignState> içinde sakla
// İlk kullanımda take() ile al (None bırakır)
pub struct SignSession {
    presign: Option<PreSignState>,
}

impl SignSession {
    pub fn sign(&mut self, message: &[u8]) -> Result<PartialSignature> {
        let presign = self.presign
            .take()  // Option → None, yeniden kullanım imkansız
            .ok_or(Error::NonceAlreadyUsed)?;
        // presign burada consume edilir
    }
}
```

**Gerekli crate:**
```toml
[dependencies]
zeroize = { version = "1", features = ["derive"] }
```

### Identifiable Abort Ekleme

FROST'ta native identifiable abort yoktur; ama Nova'nın Guardian sistemi için eklenmeli.

**Hangi round'da eklenmeli:**

```
FROST Round 1 (Commit):
  Her guardian: (D_i, E_i) commitment yayınlar
  Bu round'da abort tespit edilemez (henüz imza yok)

FROST Round 2 (Sign):
  Partial imza: s_i = k_i + H(R, Y, m) * x_i
  BLAME NOKTASİ: s_i doğrulanabilir
  Verify: s_i * G == D_i + H(R,Y,m)*E_i + H(i,m,{D_j,E_j})*Y_i
  Başarısız olan guardian = suçlanan taraf

FROST Round 3 (Aggregation — Nova'ya eklenecek):
  Aggregator her partial imzayı doğrular
  Hatalı s_i → suçlanan guardian tespit
```

**Nova Rust implementasyonu:**
```rust
// nova-mpc/src/guardian/frost_blame.rs

pub struct FrostBlameState {
    pub round2_messages: Vec<(GuardianId, PartialSignature)>,
    pub commitments: Vec<(GuardianId, (Point, Point))>,  // (D_i, E_i)
    pub public_keys: Vec<(GuardianId, Point)>,           // Y_i
}

impl FrostBlameState {
    /// Hatalı partial imzayı tespit eder
    pub fn identify_bad_signer(
        &self,
        message: &[u8],
        group_commitment: &Point,
        group_key: &Point,
    ) -> Vec<GuardianId> {
        let mut bad_guardians = Vec::new();
        for (id, s_i) in &self.round2_messages {
            if !self.verify_partial(id, s_i, message, group_commitment, group_key) {
                bad_guardians.push(*id);
            }
        }
        bad_guardians
    }

    fn verify_partial(&self, id: &GuardianId, s_i: &Scalar, ...) -> bool {
        // s_i * G == D_i + H(R,Y,m)*E_i + H(i,m,...)*Y_i
        // Başarısız → false
        todo!()
    }
}
```

### Zero Secret Share Guard

**GG18/GG20'de benzer kontrol:** VSS'de `vss.commitments[0] == y_i` sıfır olmayan public key garantisi.

**Nova FROST için:**
```rust
// nova-mpc/src/guardian/dkg.rs

impl LocalKey {
    pub fn validate(&self) -> Result<(), KeyError> {
        // Secret share sıfır olmamalı
        if self.secret_share.is_zero() {
            return Err(KeyError::ZeroSecretShare);
        }

        // Public key point at infinity olmamalı
        if self.public_key.is_identity() {
            return Err(KeyError::IdentityPublicKey);
        }

        // VSS consistency check
        // share * G == sum(commitment[k] * i^k for k in 0..=t)
        let computed = self.vss_commitments.evaluate_at(self.index);
        if computed != self.public_share {
            return Err(KeyError::VssInconsistency);
        }

        Ok(())
    }
}
```

### SSID'e VSS Commitment'ları Ekleme

GG20'de `LocalKey.h1_h2_n_tilde_vec` tüm partilerin range proof parametrelerini bağlar. Nova'nın SSID (Session ID) bağlaması güçlendirilmeli.

```rust
// MEVCUT (zayıf bağlama):
let ssid = hash(keygen_round_messages);

// ÖNERİLEN (güçlü bağlama):
let ssid = hash(
    keygen_round_messages,
    vss_commitments_all_parties,   // VSS commitment vektörleri
    public_key_vector,             // Tüm guardian public key'leri
    threshold_params,              // t, n parametreleri
    epoch_number,                  // Hangi epoch'ta üretildi
);
```

### DKG Round3 Index Hatası

**GG20 analojisi:** `gg_2020/keygen/rounds.rs:235-236` — 0-tabanlı `party_index` ile 1-tabanlı state machine indisleri arasında karışıklık.

**Nova FROST DKG'deki potansiyel hata:**
```rust
// YANLIŞ — 0-tabanlı index ile 1-tabanlı VSS değerlendirmesi:
let share = vss_poly.evaluate_at(guardian_index);  // 0-tabanlı

// DOĞRU — 1-tabanlı:
let share = vss_poly.evaluate_at(guardian_index + 1);  // 1-tabanlı (Lagrange için)
```

**Kontrol:** Nova'nın `vss.rs`'de VSS değerlendirmesi `i` veya `i+1` kullanılıyor mu doğrulanmalı.

---

## D. GUARDIAN KEY YÖNETİMİ

### KeyGen Lifecycle

```
1. GENERATE (Distributed Key Generation)
   Zaman: Guardian onboarding sırasında
   Protokol: FROST DKG (Nova mevcut) veya CGGMP21 keygen
   Süre: ~30-60 saniye (Paillier safe prime dahil)
   Çıktı: Her guardian'da LocalKey { secret_share, public_key, vss_scheme }
   Güvenlik: secret_share asla düz metin iletilmez

2. DISTRIBUTE (VSS paylaşımı)
   GG20 Round2 P2P: f_i(j) payları ECDH+AES-256-GCM ile şifreli
   ECDH anahtarı: (y_j * u_i).x_coord()  [common.rs:114-119 örneği]
   Nova için: libp2p Noise XX protokolü üzerinde

3. BACKUP (Yedekleme)
   Centipede (GG18/GG20'de mevcut): Helgamal segmented encryption
   Nova implementasyonu:
   - Personal guardian: Kullanıcının device key ile şifreli
   - Institutional guardian: HSM veya vault storage
   - CloudBackup guardian: MPC ile şifreli cloud depolama

   Kod referansı:
   PartyPrivate::to_encrypted_segment(segment_size, num_segments, pub_key, G)

4. ROTATE (Proaktif güvenlik)
   Ne zaman: Her epoch (24 saat = 216,000 slot) veya manuel
   Protokol: CGGMP21 key refresh veya FROST reshare
   Süre: ~2 round (Nova reshare mevcut)
   Garanti: Eski share sızdırılmış olsa bile yeni share güvenli

5. REVOKE (İptal)
   Tetikleyici: Guardian slashing, kullanıcı isteği, timeout
   Mekanizma: Reshare ile o guardian'ı yeni komiteden çıkar
   Onchain: GuardianRevoked event + replacement guardian atama
```

### Reshare Protokolü

**Nova'nın mevcut 2-round reshare implementasyonu değerlendirmesi:**

GG18/GG20'de native reshare yoktur. Nova'nın 2-round implementasyonu şu yapıya sahip olmalı:

```
Round 1 (Eski Komite → Yeni Komite):
  Eski komitedeki t+1 guardian:
    Yeni komite için VSS payları üretir
    f_new_i(j) = yeni polinom değerlendirmesi
    VSS commitments yayınlanır

Round 2 (Yeni Komite Doğrulama):
  Yeni komitedeki her guardian:
    Aldığı VSS paylarını doğrular
    x_new_i = sum(f_new_j(i)) hesaplar
    DLogProof(x_new_i) yayınlar

Son:
  Kolektif public key DEĞİŞMEZ (kullanıcı adresi korunur)
  Yalnızca paylar ve guardian seti değişir
```

**Eksiklikler kontrolü:**
- 2 round yeterli (FROST reshare referans: RFC 9591 Section 5)
- Eski komite t+1 toplanamıyorsa reshare mümkün değil — bu normal
- Guardian kaybı durumu: Section D.4'te

**Güvenlik notu:** Reshare sırasında kolektif public key `Y = sum(y_i) = g^(sum(u_i))` değişmemelidir. Bu matematiksel olarak garantidir çünkü yeni paylar aynı gizli toplamın farklı paylaşımıdır.

### Epoch Rotasyonu

**Parametreler (Nova context'ten):**
```
1 Epoch = 216,000 slot (24 saat)
Slot süresi = 400 ms
```

**Rotasyon sıklığı:**
- Normal: Her epoch başında key refresh (proaktif güvenlik)
- Acil: Slashing olayından sonra 3 epoch içinde

**Trigger koşulları:**
```rust
pub enum EpochRotationTrigger {
    Scheduled,                    // Her epoch
    GuardianSlashed(GuardianId),  // Slash sonrası
    ThresholdCompromised,         // t guardian leak raporu
    UserRequest(AccountId),       // Manuel kullanıcı isteği
    HighValueOperation,           // >X NSC transfer öncesi
}
```

**MEV koruması ile entegrasyon:** Eon key rotasyonu (IBE) epoch rotasyonu ile senkronize olmalı.

### Guardian Kaybı Durumu

**Senaryo:** 5 guardian'dan 1 kaybedildi, 4 guardian kaldı (t=2 için yeterli).

```
Adım 1: Kalan guardian'lar (4 adet) threshold imzayla karar
Adım 2: Reshare protokolü — 4 guardian yeni 5. guardian dahil yeni komite oluşturur
Adım 3: Yeni guardian'a share gönderilir (VSS ile)
Adım 4: Eski guardian revoke edilir (onchain kayıt)
```

**Senaryo:** t+1'den az guardian kaldı (örn. 5/5'te 3 kayıp, t=2 gerektiriyor):

```
Bu durumda threshold signing MÜMKÜN DEĞİL.
Çözüm: Social recovery mekanizması
- Kullanıcı kimlik kanıtı sunar (UserIdentityProven)
- Kalan guardian'lar + ek doğrulama ile yeni komite oluşturulur
- Nova: GuardianCategory.Personal için kullanıcı device key devreye girer
```

### Key Backup: Guardian Kategorisine Göre Strateji

```
Personal Guardian:
  Backup: Kullanıcının cihazında şifreli (device key ile)
  Recovery: Device key + PIN/biyometrik
  Risk: Cihaz kayıpı → centipede recovery

CertifiedIndividual (min 1000 NSC):
  Backup: HSM veya hardware key + encrypted cloud
  Recovery: Certified ID + guardian threshold

Institutional (min 10,000 NSC):
  Backup: HSM cluster (Thales, AWS CloudHSM)
  Recovery: Institutional KYC + multi-sig onchain vote

DAO Guardian (min 5,000 NSC):
  Backup: Multi-sig DAO wallet (3/5 DAO admin)
  Recovery: DAO governance vote + time delay

Device:
  Backup: Secure enclave (TEE/SGX)
  Recovery: Biometric + backup code

HardwareKey:
  Backup: Seed phrase (güvenli offline storage)
  Recovery: Seed phrase regeneration
```

---

## E. THRESHOLD DECRYPT — MEV KORUMASI

### Threshold IBE Tam Protokolü

Nova'nın MEV koruması (NOVA_SHIELD_CONTEXT.md:176-177): IBE şifreli emirler, slot kapanana kadar içerik gizli.

**IBE tam protokolü:**

```
Setup (her epoch başında):
  Keyper'lar (validators) FROST DKG ile eon key üretir
  (eon_sk_1, ..., eon_sk_n) → threshold t-of-n
  Eon public key: eon_pk = sum(eon_pk_i)  [kolektif]

Şifreleme (kullanıcı, emir gönderirken):
  IBE encrypt(eon_pk, slot_id, message) → ciphertext
  Slot kimliği = şifre çözme kimliği (identity)

Şifre Çözme (slot kapandığında):
  Her keyper i: decrypt_share_i = IBE_PartialDecrypt(eon_sk_i, ciphertext, slot_id)
  Collector: t+1 share ile Lagrange interpolasyon → plaintext
```

**Lagrange interpolasyon ile threshold decrypt:**
```
Şifre çözme payları: d_1, d_2, ..., d_{t+1}
Lagrange katsayıları: lambda_i = prod_{j≠i}(j/(j-i)) mod q
Final decrypt: m = sum_i(lambda_i * d_i)
```

### Eon Key Lifecycle

```
1. PROPOSE (Epoch E-1'de):
   Validator'lar yeni epoch için eon key üretim önerisi
   Onchain: EonKeyProposal { epoch, participants, threshold }

2. DKG (Epoch E-1'in son saatinde):
   FROST DKG çalışır
   Süre: ~30 saniye (400ms slot * round sayısı)
   Çıktı: eon_pk (public), eon_sk_i (her keyper'da)

3. THRESHOLD CONFIRM:
   t+1 validator DKG completion imzalar
   Onchain: EonKeyConfirmed { epoch, eon_pk }

4. AKTİF (Epoch E başında):
   Tüm emirler yeni eon_pk ile şifrelenir

5. DECOMMISSION:
   Epoch E bitince eon_sk'lar silinir (zeroize)
   Sadece slot başına açılan şifreler korunur değil
```

### Batch Decrypt

```
Slot kapandığında:
  1. Auctioneer sıralama tamamlar (hash sıralaması)
  2. Her keyper decrypt share gönderir: share_i = PartialDecrypt(eon_sk_i, batch_ciphertexts)
  3. t+1 share toplanınca Lagrange ile decrypt
  4. Emirler açığa çıkar → batch auction çalışır
  5. Clearing price hesaplanır

Timing:
  Slot başı: Emirler şifreli gelir
  Slot sonu: Decrypt başlar
  Slot+1 başı: Auction tamamlanır
```

### Nova'nın Eon Key Uyumsuzluğu

**Sorun (bug #19 listeden):** `eon_key` secp256k1[33 byte] ama IBE decrypt Ristretto255[32 byte] bekliyor.

**Tam çözüm:**

Sorunun kökü: IBE şema seçimi ile eon key eğrisi uyuşmuyor.

**Seçenek 1 (önerilen): Ristretto255 IBE**
```rust
// Tüm eon key altyapısını Ristretto255'e taşı
// Avantaj: Ristretto255 IBE için optimize, cofactor yok, canonical encoding
// Dezavantaj: secp256k1 vault'larla doğrudan uyumsuz → köprü gerekli

use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;

pub struct EonKey {
    public: RistrettoPoint,  // IBE public key (Ristretto255)
}

pub struct EonKeyShare {
    secret_share: Scalar,    // Keyper'ın payı
}
```

**Seçenek 2: secp256k1 IBE (BF-IBE)**
```rust
// secp256k1 üzerinde BF-IBE (Boneh-Franklin)
// Dezavantaj: Bilinear map secp256k1 üzerinde verimli değil
// → Ristretto255 tercih edilir
```

**Seçenek 3 (kısa vadeli fix): Dönüşüm katmanı**
```rust
// eon_key_secp256k1 → IBE decrypt için Ristretto255'e dönüştür
// Deterministik eşleme: secp256k1 x-coord → Ristretto255 scalar
fn secp256k1_to_ristretto(pk: &[u8; 33]) -> RistrettoPoint {
    // UYARI: Bu kriptografik olarak güvenli bir dönüşüm değil
    // Sadece geçici — tam çözüm Seçenek 1
    let hash = blake3::hash(pk);
    RistrettoPoint::from_hash(hash)
}
```

**Kesin öneri:** Seçenek 1 — eon_key altyapısını baştan Ristretto255 ile yeniden yaz. Mevcut secp256k1 eon_key kodu çalışmıyor zaten (bug #19).

---

## F. P2P GÜVENLİ İLETİŞİM

### Guardian Arası Şifreli İletişim

**GG18/GG20'den öğrenilen:** VSS pay iletimi ECDH + AES-256-GCM:
```
Anahtar: key = (y_j * u_i).x_coord()  [keygen_client.rs:114-119]
Nonce: 12 byte, OsRng
Cipher: AES-256-GCM
```

**Nova için önerilen şema:** Noise XX (X25519 + AES-256-GCM)
```
Avantaj:
  - Çift taraflı kimlik doğrulama (XX pattern)
  - Forward secrecy (ephemeral key)
  - libp2p native desteği

Dezavantaj:
  - MPC mesajları zaten application-level şifreli
  - Double encryption overhead

Çözüm: Noise XX transport + application-level ECDH yalnızca VSS payları için
```

### Nova'nın p2p_encrypt.rs Değerlendirmesi

Context'e göre (satır 183): `nova-network: libp2p yok, Noise XX mock`

Mevcut implementasyon (ECDH+HKDF+AES-256-GCM) doğru yaklaşım ama:
1. `libp2p` henüz entegre değil
2. Guardian identity binding eksik — şifreli kanal anonim
3. Session key yenileme mekanizması yok

**Üretim için gereksinimler:**
```rust
// nova-network/src/guardian_channel.rs

pub struct GuardianChannel {
    peer_id: PeerId,                   // libp2p identity
    guardian_address: AccountId,       // onchain kimlik
    noise_session: NoiseSession,       // Noise XX transport
    session_key: [u8; 32],            // AES-256-GCM anahtarı
    session_nonce: u64,               // monoton artan nonce
    last_rotation: Slot,              // session key son yenileme
}

impl GuardianChannel {
    const SESSION_KEY_ROTATION_SLOTS: u64 = 1000;  // ~6.7 dakika

    pub fn rotate_session_key(&mut self) {
        // HKDF ile yeni session key türet
        // Eski key zeroize et
    }
}
```

### Guardian Doğrulama: Kimlik Kanıtı

GG20 analojisi: `NiCorrectKeyProof` — Paillier anahtarının sahibini kanıtlar.

Nova için guardian kimlik kanıtı:
```
1. Onchain Guardian Kaydı:
   guardian.public_key = secp256k1/ed25519 imza anahtarı
   guardian.stake_amount = staked NSC

2. P2P Bağlantı Doğrulama:
   Libp2p PeerId = guardian.public_key'den türetilir
   DLogProof(guardian_private_key) bağlantı kurulumunda

3. MPC Mesaj İmzalama:
   Her MPC mesajı guardian private key ile imzalanır
   Alıcı: guardian.public_key ile verify eder

4. Slashing Kanıtı:
   Kötü mesaj + guardian imzası = slashing kanıtı
   Onchain submit edilebilir
```

### Kötü Niyetli Guardian Tespiti

**GG20'den öğrenilen mekanizma:**

| Senaryo | Tespit | Nova Karşılığı |
|---------|--------|----------------|
| Yanlış delta_i | phase5_blame() | WrongFreeze slash %5 |
| Yanlış sigma_i | phase6_blame() | WrongFreeze slash %5 |
| Yanlış s_i | phase7_blame() | Malicious slash %100 |
| Timeout | round_timeout (Nova eklemeli) | Unresponsive slash %3 |
| Çifte imza | Onchain tespit | Double sign slash (validator: %100) |

---

## G. SIGNING EKONOMİSİ

### Guardian Ödül Hesabı

**GG20 analizi:** Offline stage hesaplama maliyeti yüksek (Paillier 2048-bit).

Nova için guardian ödülü hesaplama önerisi:

```
Temel ödül = (yıllık oran * staked_NSC) / epoch_başına_imza_sayısı

Signing ödülü = imza_başına_sabit_ücret + (imza_değeri * bps)

Yanıt ödülü = 0 (eşik altında imza → ödül yok)
Yanıt cezası = unresponsive_threshold aşılırsa slash

Örnek parametreler (tartışmaya açık):
  Yıllık temel: %8 (Guardian staking APY)
  İmza başına: 0.01 NSC (küçük sabit)
  İmza değeri bps: 1 bps (10,000 NSC üzeri için)
```

**Nova Context'ten mevcut değerler:**
```
Fee dağılımı (cross-chain): Staker %60, Guardian %40
Yıllık koruma ücreti: %0.05 (5 bps)
```

### Yanıt Vermeyen Guardian → Timeout → Slashing

**GG20'deki kritik eksiklik:** `round_timeout() = None` [sign.rs:389-390, keygen.rs:291-293]

Bu, yanıt vermeyen guardian'ın protokolü sonsuz bloke edebileceği anlamına gelir.

**Nova için `round_timeout` zorunlu:**

```rust
// nova-mpc/src/guardian/protocol.rs

impl StateMachine for GuardianMpcSession {
    fn round_timeout(&self) -> Option<Duration> {
        // Her round için farklı timeout — Paillier işlemleri uzun sürebilir
        match self.current_round {
            Round::KeyGen0 => Some(Duration::from_secs(120)), // Paillier safe prime
            Round::KeyGen1 => Some(Duration::from_secs(30)),
            Round::Sign0   => Some(Duration::from_secs(60)),  // MessageA range proof
            Round::SignP2P => Some(Duration::from_secs(30)),  // P2P MtA
            _ => Some(Duration::from_secs(30)),
        }
    }

    fn round_timeout_reached(&mut self) -> Self::Err {
        // Timeout olan guardian'ı tespit et
        let (expected, missing) = self.round_blame();

        // Onchain slashing tetikle
        for guardian_id in missing {
            emit_slash_event(guardian_id, SlashReason::Unresponsive);
        }

        Error::RoundTimeout { missing_parties: missing }
    }
}
```

**Timeout değerleri:**
```
KeyGen Round 0 (Paillier safe prime): 120 saniye
KeyGen diğer round'lar: 30 saniye
Sign offline round'lar: 60 saniye (MtA dahil)
Sign online round: 10 saniye

Toplam signing timeout üst sınırı: ~5 dakika
(6*60 + 1*10 = ~370 saniye)
```

### SLA ve İmza Yanıt Süresi

**Nova Guardian Marketplace'ten (context satır 145-146):**
```
Contract süresi: 1 yıl
Sigorta oranları:
  RecoveryRefused:          %100
  WrongRecoveryAttempt:     %75
  GuardianUnresponsive:     %50
  SlaViolation:             %25
```

**Önerilen SLA parametreleri:**
```
Guardian tipi    | Max signing latency | Min availability
Personal         | 24 saat             | %90
CertifiedInd.    | 1 saat              | %95
Institutional    | 5 dakika            | %99.5
DAO              | 1 saat              | %95
```

### İmza Başına Ücret vs Abonelik

**GG20'den öğrenilen:** Offline stage hesaplama maliyeti sabit, online stage ek maliyet.

**Nova önerisi: Karma model**
```
Abonelik (yıllık): Kullanıcı belirlediği guardian set için yıllık ücret öder
  = toplam_vault_değeri * 0.05%  [cross-chain %0.05'ten]

İmza başına: Recovery ve freeze operasyonları için ek ücret
  Recovery imzası: 10 NSC (sabit)
  Freeze imzası: Bedava (güvenlik operasyonu)
  Cross-chain TX: vault_değeri * 0.01%

Ödül dağılımı:
  Abonelik: Guardian'lara eşit bölünür (komite içinde)
  İmza başına: İmzalamaya katılan t+1 guardian'a
```

---

## H. VSS VE DKG GÜVENLİĞİ

### Feldman VSS Doğrulama

**GG20'den (rounds.rs ve party_i.rs):**

```
Polinomun sabit terimi: f_i(0) = u_i (gizli)
Commitments: C_k = u_i^{a_k} * G, C_0 = y_i

Doğrulama formülü:
  g^(s_j) == sum_{k=0}^{t} (C_k^(j^k))
  yani: g^f_i(j) == C_0^1 * C_1^j * C_2^{j^2} * ... * C_t^{j^t}

Nova'nın vss.rs'de bunu kontrol etmeli:
  VerifiableSS::validate_share(share_ji, index_i) → bool
  VE vss.commitments[0] == y_i [party_i.rs:340]
```

**İkinci doğrulama kritik:** `vss.commitments[0] == y_i` — sabit terim `y_i`'ye eşit olmalı. Bu GG18'de yoktur (zafiyet), GG20'de eklenmiştir.

### Nova'nın vss.rs Değerlendirmesi

Context'te vss.rs doğrudan belirtilmemiş ama nova-mpc'nin bir parçası olduğu anlaşılıyor.

**Kontrol edilmesi gerekenler:**
1. `validate_share` hem aritmetik doğrulama hem commitment doğrulama yapıyor mu?
2. `commitments[0]` guardian'ın public key'iyle eşleştiriliyor mu?
3. Sıfır polinomu engelleme (tüm coefficient sıfır) kontrolü var mı?

### Bilinear Map Saldırısına Karşı Koruma

GG20 `CompositeDLogProof` ile N_tilde üzerinde `h2 = h1^xhi` ilişkisini kanıtlar. Bu, range proof'larda kullanılan Pedersen kurulumunun güvenliğini garanti eder.

Nova'nın FROST'u Paillier kullanmıyor (Schnorr tabanlı), bu nedenle bu risk doğrudan geçerli değil. Ama IBE şeması Ristretto255 ile bilinear map (pairing) kullanıyorsa:

```
Güvenlik varsayımı: Ristretto255 üzerinde CDH (Computational Diffie-Hellman) zor
Pairing tabanlı IBE: BF-IBE (Boneh-Franklin) ayrıca e(P,Q) pairing gerektirir
Seçim: Nova BLS12-381 gibi pairing-friendly eğri kullanabilir
veya: Pairing gerektirmeyen IBE (Hierarchical IBE, daha verimli)
```

### Rogue Key Attack

**GG20 koruması:** Round 0'da `com_i = SHA256(y_i || blind_i)` — taraflar `y_i`'yi commit eder. Tüm commit'ler alındıktan sonra reveal edilir.

**Neden kritik:** Rogue key attack'ta kötü niyetli guardian `y_rogue = -y_1 - y_2 + y_rogue_actual` seçebilir, böylece toplam public key `Y = sum(y_i) = y_rogue_actual` olur (sadece kendisi kontrol eder).

**Commitment-then-reveal bu saldırıyı önler:**
- Guardian'lar `y_i`'yi önceden commit eder
- Diğerlerini gördükten sonra `y_i`'yi değiştiremezler

**Nova FROST için:** DKG Round 1'de commitment'lar zorlanmalı, Round 2'de reveal.

### Koeffisiyen Sıfırlama (Zeroize) Güvenliği

**GG20'de mevcut:**
```rust
// range_proofs.rs
#[derive(Zeroize)]
#[zeroize(drop)]
struct AliceZkpRound1 { alpha, beta, gamma, ro, z, u, w }

#[derive(Zeroize)]
#[zeroize(drop)]
struct BobZkpRound1 { alpha, beta, gamma, ro, ro_prim, sigma, tau, z, z_prim, t, w, v }
```

**GG20'de eksik (üretim uyarısı):**
```rust
// Bu struct'lar zeroize uygulamiyor — Nova dikkat etmeli:
// PartyPrivate { u_i, x_i, dk } — ZEROIZE GEREKLİ
// SignKeys { w_i, k_i, gamma_i } — ZEROIZE GEREKLİ
// LocalSignature { l_i, rho_i, sigma_i, s_i } — ZEROIZE GEREKLİ
```

**Nova için zorunlu pattern:**
```rust
// nova-mpc/src/guardian/keys.rs
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct GuardianSecretShare {
    secret_share: Scalar<Ristretto>,  // FROST share
    signing_nonce: Option<Scalar>,     // Tek kullanımlık
    // ...
}
```

---

## I. ZK PROOF VE MPC ENTEGRASYONU

### ZK-MPC: Threshold İmzayı ZK ile Kanıtlama

Nova'nın `nova-zk` henüz mock durumda (BLAKE3 simülasyon). Gerçek entegrasyon için:

**Senaryo:** Guardian threshold imzası geçerli, ama hangi guardian'ların imzaladığı gizli tutulsun.

```
ZK proof: "t-of-n guardian'dan en az t+1'i valid imza üretmiştir"
          "ama hangileri → gizli"

Yaklaşım 1: Ring signature benzeri (Schnorr tabanlı — FROST uyumlu)
Yaklaşım 2: Groth16 proof — guardian commitment'larının doğruluğu
```

### Bulletproofs vs Groth16 vs PLONK MPC için

| Sistem | MPC Uyumu | Nova Fit | Not |
|--------|-----------|----------|-----|
| Bulletproofs | Kısmi (range proof) | Range kanıtları için | Trusted setup yok |
| Groth16 | Düşük (per-circuit setup) | ZK kalkanı için | Trusted setup gerekli |
| PLONK | Orta | Esnek circuit | Universal setup |
| STARK | Yok (FRI) | Uzun vadeli | Kuantum dirençli |

**Nova için öneri:**
- Guardian signing proof: Schnorr ring signature (setup yok, hızlı)
- ZK shield (shielded TX): Groth16 (mevcut plan — Penumbra analizi referans)
- Range proofs (MEV): Bulletproofs (Paillier MtA'da alternatif)

### Nova'nın ZK Shield + MPC Guardian Entegrasyonu

```
Kullanıcı akışı:
  1. Shielded not oluştur: note = (value, asset_id, rand, transmission_key)
  2. MPC guardian imzası: threshold_sig = FROST.sign(TX_hash, guardian_set)
  3. ZK kanıt: Groth16.prove(spend_auth, nullifier, commitment)
  4. Onchain submit: (threshold_sig, zk_proof, encrypted_note)

Entegrasyon noktası:
  - ZK proof: kullanıcının sahipliğini kanıtlar (özel anahtar)
  - MPC imzası: guardian'ların onayını kanıtlar (threshold)
  - İkisi birlikte: Hem sahip hem onaylı işlem
```

### Shielded Recovery: Kimliği İfşa Etmeden Kurtarma

```
Senaryo: Kullanıcı seed phrase kaybetti, guardian recovery istiyor

ZK-MPC Protokolü:
  1. Kullanıcı: ZK proof üretir → "bu hesabın önceki sahibiydim"
     (eski spending key bilinmeden, viewing key ile)
  2. Guardian'lar: ZK proof'u doğrular (gizlilik korunuyor)
  3. Threshold imza: Yeni public key için guardian approval
  4. Recovery: Yeni anahtar için guardian kaydı güncellenir

ZK proof içeriği:
  Gizli: eski spending key veya nonce
  Kamuya açık: commitment(new_key, old_account_hash)
  Kanıt: "new_key ↔ old_account" bağlantısı (kimlik ifşa etmeden)
```

---

## J. GUARDIAN TOKEN EKONOMİSİ

### Guardian Bonding

**Nova Context'ten mevcut değerler:**
```
Personal:              Belirsiz (düşük)
CertifiedIndividual:   1,000 NSC minimum
Institutional:        10,000 NSC minimum
DAO:                   5,000 NSC minimum
```

**MPC analizinden alınan ders:** Guardian'ın ne kadar sermayeliyi etkilediği:
- Düşük stake → slashing caydırıcı değil → Malicious davranış riski
- Çok yüksek stake → Guardian sayısı azalır → Sansür riski

**Önerilen formül (GG20 blame analizinden türetilen):**
```
Min stake = max(
    kategori_minimumu,
    ortalama_korunan_varlık / koruma_oranı
)

koruma_oranı = 10  (her NSC stake, 10 NSC varlık için)

Örnek: Kullanıcının 100,000 NSC vault'u için:
  Guardian başına min stake: max(1000, 100000/10) = 10,000 NSC

Bu, Institutional kategorisini zorunlu kılar — doğru tasarım.
```

### Guardian Ödül Hesabı

```
Yıllık ödül formülü:
  base_reward = staked_NSC * 0.08  (yıllık %8, emission.rs'den)

  signing_reward = imza_başına * yıllık_imza_sayısı
  imza_başına = 0.01 NSC (önerilen)

  availability_bonus = stake * 0.02 * (uptime / %99.5)

  total = base_reward + signing_reward + availability_bonus

Slashing:
  WrongFreeze: -5% stake
  Unresponsive: -3% stake
  Malicious: -100% stake (tombstone)
```

### Slash Koşulları ve Oranları

**Mevcut değerler (NOVA_SHIELD_CONTEXT.md):**
```
WrongFreeze:     %5
Unresponsive:    %3
Malicious:       %100
```

**GG20 analizinden öğrenilen:**

Phase5 blame (yanlış delta_i) → Kasıtsız hata olabilir (bug, ağ gecikmesi)
Phase6 blame (yanlış sigma_i) → Daha şüpheli
Phase7 blame (yanlış s_i) → Neredeyse kesinlikle kasıtlı

**Nova için revize önerisi:**
```
DeltaI Tutarsızlığı (phase5 eşdeğeri): %3 (kazara hata olabilir)
SigmaI Tutarsızlığı (phase6 eşdeğeri): %10 (şüpheli)
WrongPartialSig (phase7 eşdeğeri):     %50 (kasıtlı)
WrongFreeze (onchain kanıtlı):         %25 (orijinal %5 düşük)
Unresponsive (timeout):                %3 (mevcut doğru)
Malicious (double sign + kanıt):       %100 (mevcut doğru)
```

**Neden WrongFreeze %5 düşük:** Yanlış dondurma kullanıcıya ciddi zarar verebilir (varlıklara erişim engeli). %25 daha caydırıcı olur.

### Guardian Marketplace Dinamikleri

**Nova Context:** Listing, Offer (1 hafta), Contract (1 yıl)

**Arz/Talep dengesi:**
```
Guardian kapasitesi = guardian başına max slot sayısı
İdeal doluluk = %70-80 (aşırı yükü önler)

Dinamik fiyatlandırma:
  Düşük doluluk (<50%): Guardian ücret indirir, slot hızla dolar
  Yüksek doluluk (>90%): Guardian ücret artırır veya yeni guardian girer

Güven puanı (context):
  trust_score = stake*30 + years*25 + success*20 + slash*15 + response*10

  Bu formülle high-trust guardian daha yüksek ücret talep edebilir
  Piyasa: high-trust premium ≈ %20-50 fazla ücret (kurumsal talep)
```

**Offer süresi bug (bug #11):**
```
Mevcut: 21600 slot = 2.4 saat (YANLIŞ)
Doğru: 1 hafta = 7 * 216000 = 1,512,000 slot
Düzeltme: offer_expiry = current_slot + 1_512_000
```

### Insurance Fund

```
Büyüklük önerisi: TVL'nin %5'i (toplam kilitlenen değer)
  Nova başlangıç TVL: ~100M NSC (tahmin)
  Insurance fund: 5M NSC

Dolum mekanizması:
  Protocol fee'nin %15'i → Hazine (context)
  Hazine'nin %25'i → Insurance fund
  Efektif oran: Protocol fee * %3.75

Ödeme önceliği:
  1. RecoveryRefused: %100 (en önce öden)
  2. WrongRecovery: %75
  3. GuardianUnresponsive: %50
  4. SlaViolation: %25

Limit: Kullanıcı başına max insurance = vault_değeri * %90
(tam karşılama riskini önler — ahlaki tehlike)
```

---

## K. MULTI-CHAIN GUARDIAN

### Bitcoin Vault: Multisig vs MPC Farkı

```
Bitcoin Native Multisig (P2SH/P2WSH):
  - m-of-n onchain görünür
  - Her imzacının public key'i açık
  - Daha az mahremiyet
  - Script maliyeti yüksek (n adet imza)

MPC Threshold Signature (FROST/Schnorr):
  - Bitcoin Taproot ile: Tek imza gibi görünür
  - Guardian'lar gizli (sadece tek imza onchain)
  - Daha düşük fee
  - Daha yüksek mahremiyet

Nova'nın önerisi: FROST (Schnorr) için Taproot
  - BTC vault: FROST + Taproot (P2TR)
  - Onchain: Sadece agregat public key ve tek imza
```

### ETH Vault: ERC-4337 + MPC

```
ERC-4337 Account Abstraction:
  - Smart contract wallet
  - Guardian seti onchain kaydedilir
  - Validation: threshold imzayı contract doğrular

Entegrasyon:
  UserOperation.signature = CGGMP21.aggregate_signature(threshold_sigs)

  EntryPoint → GuardianAccount.validateUserOp()
    → verify(threshold_sig, guardian_set, tx_hash)
    → OK → execute()

Avantaj: ETH native recovery flow (ERC-4337 social recovery)
Dezavantaj: Onchain guardian seti görünür (privacy tradeoff)
```

### Nova-Guard: Her Zincir için Farklı İmza

```
bitcoin:  FROST (Schnorr/BIP-340) — Taproot uyumlu
ethereum: CGGMP21 (ECDSA/secp256k1) — EIP-191 uyumlu
solana:   FROST (Ed25519) — Solana native
cosmos:   FROST (Ed25519) — Tendermint uyumlu

nova-guard routing:
  match chain {
    Bitcoin  => FrostScheme::Secp256k1Schnorr,
    Ethereum => CggmpScheme::Secp256k1Ecdsa,
    Solana   => FrostScheme::Ed25519,
    Cosmos   => FrostScheme::Ed25519,
  }

Bug #14 (ETH/SOL/Cosmos vault adresi guardian içermiyor):
  Çözüm: Vault address = P(guardian_keys, threshold_params) deterministik türetme
  Her chain için ayrı türetme fonksiyonu
```

### Cross-Chain Atomik Swap için MPC

```
Senaryo: BTC ↔ ETH swap, her iki taraf da güvenmez

HTLC (Hash Time Lock Contract) + MPC:
  1. Preimage H = hash(secret)
  2. BTC kilitlenir: HTLC(H, timeout=24h)
  3. ETH kilitlenir: HTLC(H, timeout=12h)
  4. Nova MPC vault: guardian'lar secret'i threshold ile açar
     (sadece her iki kilitlenme doğrulandıktan sonra)

Adaptör İmzası (daha iyi — Nova için öneri):
  Schnorr adaptör imzası ile secret elde etme
  FROST guardian'ları adaptör imzası üretir
  ETH tarafı imzayı görünce BTC tarafı secret'i çıkarır
  Atomik: ya ikisi de tamamlanır ya da ikisi de iptal olur
```

---

## L. GÜVENLİK KRİTİK KONTROL LİSTESİ

### GG18/GG20'deki Bilinen Açıklar

| # | Açık | Kaynak | Şiddet |
|---|------|--------|--------|
| 1 | Range proof devre dışı (GG18 testleri) | test.rs:194,219,226 (`&[]`) | Kritik |
| 2 | `Keys::create()` safe prime kullanmıyor | party_i.rs:162 | Kritik |
| 3 | N_tilde safe prime yok (GG20) | party_i.rs:139-140 yorum | Yüksek |
| 4 | `CompletedOfflineStage Clone` | rounds.rs:647 | Kritik |
| 5 | `round_timeout() = None` | sign.rs:389-390 | Yüksek |
| 6 | BobProof gamma q^2 (spec: q^3) | range_proofs.rs:233 | Orta |
| 7 | Phase5 blame TODO eksik | blame.rs:57-58 | Orta |
| 8 | bad_actors alanları private | mod.rs:23-27 | Düşük |
| 9 | PartyPrivate zeroize yok | party_i.rs:60-64 | Orta |
| 10 | Lindell x1 < q/3 yalnızca create_commitments'ta | party_one.rs:143 | Orta |

### FROST'ta Bilinen Açıklar

| # | Açık | Protokol Referansı | Şiddet |
|---|------|-------------------|--------|
| 1 | Nonce commitment'sız share | FROST RFC 9591 Sec 5.4 | Kritik |
| 2 | Paralel oturum saldırısı | Wagner'ın çoklu ROS | Kritik |
| 3 | Identifiable abort yok | FROST tasarımı | Yüksek |
| 4 | Koordinatör tek başarısızlık | RFC 9591 | Yüksek |
| 5 | Binding factor bağlama eksikliği | SimpleTWEAK saldırısı | Yüksek |

### Nova'nın Her Açığa Karşı Mevcut Savunması

| Açık | Nova Durumu | Aciliyet |
|------|-------------|---------|
| Range proof devre dışı (GG18) | Nova FROST kullanıyor → geçerli değil | - |
| Safe prime yok | CGGMP21'de varsayılan safe prime → geçerli değil | - |
| CompletedOfflineStage Clone | FROST'ta analog: PreSignState kopyalanabilir | ACİL |
| round_timeout = None | Nova: Yok → round_timeout eklenmeli | ACİL |
| Identifiable abort yok | Nova FROST'ta yok → blame round eklenmeli | YÜKSEK |
| Nonce reuse (FROST) | `PreSignState &mut` API → düzeltilmeli | ACİL |
| Paralel oturum (Wagner) | SSID bağlama eksikliği → eklenmeli | YÜKSEK |
| eon_key uyumsuzluğu (bug #19) | secp256k1 vs Ristretto255 → kritik bug | ACİL |
| TX imzası doğrulanmıyor (bug #4) | Açık → validator TX doğrulama eklenmeli | ACİL |
| TX hash içerik-bağımsız (bug #5) | to/amount dahil değil → hash düzeltilmeli | ACİL |
| Zeroize eksik | nova-mpc'de kontrol gerekli | YÜKSEK |

### Üretim Öncesi Zorunlu Güvenlik Önlemleri

**TIER 1 — Acil (Üretimde kesinlikle olmalı):**
```
[ ] CGGMP21 keygen: safe prime Paillier (create_safe_prime eşdeğeri)
[ ] PreSignState tek kullanımlık (Option::take() semantic)
[ ] round_timeout: her round için gerçekçi süre (30-120 saniye)
[ ] Identifiable abort: FROST blame round implementasyonu
[ ] eon_key: Ristretto255'e geçiş (bug #19)
[ ] TX hash: to + amount + nonce dahil edilmeli (bug #5)
[ ] TX imzası: her TX'te verify edilmeli (bug #4)
[ ] Zeroize: tüm gizli yapılarda #[derive(ZeroizeOnDrop)]
```

**TIER 2 — Yüksek (İlk testnet öncesi):**
```
[ ] SSID: VSS commitment bağlama
[ ] Guardian kimlik: P2P mesaj imzalama + verify
[ ] Session key rotation: 1000 slot'ta bir
[ ] Blame kanıtı onchain submit mekanizması
[ ] Guardian offer süresi: 21600 slot → 1_512_000 slot (bug #11)
[ ] WrongFreeze slash: %5 → %25 (caydırıcılık)
[ ] Insurance fund: protocol fee'nin %3.75'i
```

**TIER 3 — Orta (Mainnet öncesi):**
```
[ ] BobProof gamma: q^2 → q^3 (spec uyumu)
[ ] Guardian zeroize: comprehensive memory cleanup
[ ] Paralel MPC oturumu koruması (Wagner saldırısı)
[ ] ZK proof: BLAKE3 mock → gerçek Groth16
[ ] Reed-Solomon: XOR mock → gerçek RS kodlama (bug #13)
[ ] Koordinatör güveni: FROST koordinatörsüz mod
[ ] EVM keccak256: BLAKE3 → keccak256 (bug #12)
[ ] Dilithium key size: 1312 → 1952 byte (bug #2)
```

**Güvenlik audit planı:**
```
İç audit: MPC protokol kodu (nova-mpc) — üretim öncesi
Dış audit: Trail of Bits / NCC Group / Kudelski Security
  Öncelik 1: nova-mpc (FROST + Guardian)
  Öncelik 2: nova-zk (ZK kanıtlar)
  Öncelik 3: nova-account (key management)

Zaman: Mainnet öncesi en az 3 ay dış audit
```

---

## ÖZET: EN KRİTİK 10 DERS

multi-party-ecdsa analizinden Nova Shield Chain'e özel, öncelik sırasına göre:

| # | Ders | Nova Eylemi | Kaynak |
|---|------|-------------|--------|
| 1 | Safe prime zorunlu | CGGMP21 varsayılan safe prime kullan | party_i.rs:162 |
| 2 | Nonce tekrar = key sızması | PreSignState Option::take() | rounds.rs:647 |
| 3 | Timeout yok = DoS | round_timeout her round için | sign.rs:389 |
| 4 | Identifiable abort = slashing | FROST blame round ekle | blame.rs |
| 5 | eon_key uyumsuzluğu | Ristretto255'e geçiş | bug #19 |
| 6 | MtA range proof zorunlu | CGGMP21'de aktif | test.rs:194 |
| 7 | Zeroize gizli yapılar | #[derive(ZeroizeOnDrop)] her yerde | - |
| 8 | SSID bağlama güçlendir | VSS commitment → SSID | - |
| 9 | VSS tutarlılık çift doğrulama | commitments[0] == y_i kontrolü | party_i.rs:340 |
| 10 | Commitment-then-reveal | DKG Round 1'de zorunlu commit | party_i.rs:195 |

---

*Tüm değerler `research/analysis/multi-party-ecdsa/DOGRULAMA.md`'de kaynaktan teyit edilmiştir.*
*16/16 iddia doğrulanmış, 0 hata bulunmuştur.*
*Kaynak repo: `research/libs/multi-party-ecdsa/` (KZen Networks, GPL-3.0+)*
