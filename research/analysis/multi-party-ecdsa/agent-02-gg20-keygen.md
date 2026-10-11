# Agent-02: GG20 Core + Key Generation Analizi

## Kapsanan Dosyalar

| Dosya | Satir | Amac |
|-------|-------|------|
| `src/protocols/multi_party_ecdsa/gg_2020/mod.rs` | 28 | Modul tanimlamalari + ErrorType |
| `src/protocols/multi_party_ecdsa/gg_2020/party_i.rs` | 937 | Core kriptografi: Keys, signing, verification |
| `src/protocols/multi_party_ecdsa/gg_2020/blame.rs` | 456 | Blame protokolu: Phase5/6/7 |
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/mod.rs` | 4 | SM modul bildirimleri |
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/keygen.rs` | 529 | Keygen state machine |
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/keygen/rounds.rs` | 348 | Round implementasyonlari + LocalKey |
| `src/protocols/multi_party_ecdsa/gg_2020/test.rs` | 784 | Entegrasyon testleri (keygen + sign) |

---

## 1. GG20 vs GG18: Temel Farklar

### GG18'in Guvenlik Sorunu
GG18 (Gennaro-Goldfeder 2018), imza sirasinda bir parti yanlis davrandiginda protokolu sonlandirabiliyordu ancak **hangi partinin yanlis davrandigini kanitleyamiyordu**. Bu "non-identifiable abort" (kimlik belirtmeyen iptal) olarak bilinir.

Sonuc: Dogrudan suistimal vektoru. Kotü niyetli bir parti:
- Herhangi bir asamada hatalı mesaj gonderir
- Protokol iptal olur
- Suclanan parti belli olmaz
- Bu durum hizmet reddi (DoS) icin kolayca kullanilabilir

### GG20'nin Cozumu: Identifiable Abort
GG20 (Gennaro-Goldfeder 2020), her potansiyel iptal noktasina bir **blame alt-protokolu** ekler. Eger protokol basarisiz olursa, surecin iptal gerektiren adiminda suclanan partiler kriptografik olarak tanimlaniyor.

Teknik mekanizma:
- Her parti, kriptografik aciklamalar (witnesses) iceren lokal durumunu saklar
- Hata durumunda lokal durumlar paylasma agina yayilir
- Herkese acik dogrulama ile hangi tarafin protokol kurallarini ihlal ettigini tespit edilir
- Tespit edilen kotu partilerin listesi (`bad_actors: Vec<usize>`) `ErrorType` ile dondurulur

**Keygen icin blame gerekmez** (Round1-4 deterministik hatalar veriyor, already identifiable). Blame mekanizmasi **signing protokolunun** asagidaki asamalari icin uygulanmistir:
- Phase5 (delta/gamma tutarsizligi)
- Phase6 (sigma/S_i tutarsizligi)
- Phase7 (local signature dogrulama hatasi)

---

## 2. party_i.rs: Kriptografik Veri Yapilari

### Temel Sabitler (satir 48-50)
```
SECURITY: usize = 256         // commitment randomness bit uzunlugu
PAILLIER_MIN_BIT_LENGTH: usize = 2047
PAILLIER_MAX_BIT_LENGTH: usize = 2048
```
Paillier modulus **tam olarak 2047-2048 bit** olmak zorunda. test_small_paillier (satir 765) bu kontrolu test ediyor — 2046-bit modulus reddediliyor.

### Elliptik Egri Kutuphanesi
`curv` kutuphanesi kullaniliyor (KZen Networks).
- `Point<Secp256k1>` = `GE` (grup elemani / public key parcasi)
- `Scalar<Secp256k1>` = `FE` (sonlu alan elemani / ozel anahtar parcasi)
- Egri: **secp256k1** (Bitcoin/Ethereum ile ayni)
- Hash: **SHA-256**

### `Parameters` Struct (satir 52-56)
```rust
pub struct Parameters {
    pub threshold: u16,   // t: minimum imzacı sayısı - 1 (t+1 imza yeterli)
    pub share_count: u16, // n: toplam parti sayisi
}
```
Anlam: `t`-of-`n` threshold sema. `t+1` parti birleste imzalayabilir.

### `Keys` Struct (satir 58-70) — En Kritik Struct
```rust
pub struct Keys<E: Curve = Secp256k1> {
    pub u_i: Scalar<E>,       // Partinin gizli anahtari (henuz paylasilmamis)
    pub y_i: Point<E>,        // g^u_i — partinin acik anahtari
    pub dk: DecryptionKey,    // Paillier sifre cozme anahtari
    pub ek: EncryptionKey,    // Paillier sifreleme anahtari (broadcast edilecek)
    pub party_index: usize,   // 0-tabanli parti indisi
    pub N_tilde: BigInt,      // Range proof icin yardimci RSA modulus
    pub h1: BigInt,           // h1 in Z*_{N_tilde}
    pub h2: BigInt,           // h2 = h1^xhi mod N_tilde
    pub xhi: BigInt,          // h1->h2 donusumunun gizli katsayisi (phi'nin bir elemani)
    pub xhi_inv: BigInt,      // xhi'nin tersi mod phi(N_tilde)
}
```

**h1, h2, N_tilde Neden Gerekli?**
MtA (Multiplicative-to-Additive) protokolunde range proof icin Pedersen commitment kurulumu. Signing sirasinda `k_i * gamma_j` ve `k_i * w_j` hesaplamalari Paillier sifreleme uzerinden gerceklestiriliyor, ve bu degerlerin dogru aralikta oldugu ZK kanitlari icin `N_tilde, h1, h2` kullaniliyor.

### `generate_h1_h2_N_tilde` Fonksiyonu (satir 137-156)
```
h2 = h1^xhi mod N_tilde
```
Sonra `xhi = phi - xhi` ve `xhi_inv = phi - xhi_inv` yapiliyor — bu negation mod phi. Anlam: gercekte `h2 = h1^(-xhi) mod N_tilde` saklanmasi saglanmaktadir.

**UYARI**: Satir 140'ta yorumda belirtilmis: guvenli asal (safe prime) kullanilmasi onerilmesine ragmen `keypair()` cagiriliyor, `keypair_safe_primes()` degil. Uretim ortami icin `create_safe_prime()` kullanilmali (satir 180).

### `PartyPrivate` Struct (satir 72-77)
```rust
pub struct PartyPrivate {
    u_i: Scalar<Secp256k1>,  // Partinin orijinal gizli degeri
    x_i: Scalar<Secp256k1>,  // Partinin threshold share'i (tum paylasimlarin toplami)
    dk: DecryptionKey,        // Paillier cozme anahtari
}
```
Keygen tamamlandiktan sonra `set_private(key, shared_key)` ile olusturuluyor.

### `KeyGenBroadcastMessage1` Struct (satir 79-87) — Round 0 Cikti
```rust
pub struct KeyGenBroadcastMessage1 {
    pub e: EncryptionKey,                         // Paillier acik anahtar
    pub dlog_statement: DLogStatement,             // N_tilde, h1, h2
    pub com: BigInt,                               // Commitment: H(g^u_i || blind_factor)
    pub correct_key_proof: NiCorrectKeyProof,      // Paillier'in dogru olusturuldugu NI-ZKP
    pub composite_dlog_proof_base_h1: CompositeDLogProof,  // h2 = h1^x mod N_tilde ZKP
    pub composite_dlog_proof_base_h2: CompositeDLogProof,  // h1 = h2^x_inv mod N_tilde ZKP
}
```

### `KeyGenDecommitMessage1` Struct (satir 89-93) — Round 1 Cikti
```rust
pub struct KeyGenDecommitMessage1 {
    pub blind_factor: BigInt,        // Commitment randomness
    pub y_i: Point<Secp256k1>,       // g^u_i (gizli anahtarin acik gorunumu)
}
```

### `SharedKeys` Struct (satir 95-99) — Keygen Nihai Cikti
```rust
pub struct SharedKeys {
    pub y: Point<Secp256k1>,      // Kolektif acik anahtar: sum(y_i) = g^(sum(u_i))
    pub x_i: Scalar<Secp256k1>,   // Partinin threshold share'i: sum of all VSS shares received
}
```

### `LocalKey<E>` Struct (rounds.rs satir 311-322) — State Machine Cikti
Keygen state machine'in nihai ciktisi, `SharedKeys`ten daha kapsamli:
```rust
pub struct LocalKey<E: Curve> {
    pub paillier_dk: DecryptionKey,             // Signing icin Paillier cozme anahtari
    pub pk_vec: Vec<Point<E>>,                  // Her partinin x_i icin g^x_i (DLog proof'tan)
    pub keys_linear: SharedKeys,                // y (acik anahtar) + x_i (threshold share)
    pub paillier_key_vec: Vec<EncryptionKey>,   // Tum partilerin Paillier acik anahtarlari
    pub y_sum_s: Point<E>,                      // Kolektif acik anahtar (redundant ama erismesi kolay)
    pub h1_h2_n_tilde_vec: Vec<DLogStatement>,  // Tum partilerin range proof parametreleri
    pub vss_scheme: VerifiableSS<E>,            // Kendi VSS sema (threshold bilgisiyle)
    pub i: u16,                                 // 1-tabanli parti indisi
    pub t: u16,                                 // Threshold
    pub n: u16,                                 // Toplam parti sayisi
}
```

### Signing Icin Veri Yapilari (satir 101-135)
```rust
pub struct SignKeys {
    pub w_i: Scalar<Secp256k1>,    // Lagrange interpolasyon agirligi uygulanmis share: lambda_i * x_i
    pub g_w_i: Point<Secp256k1>,   // g^w_i
    pub k_i: Scalar<Secp256k1>,    // Rastgele nonce parcasi (gizli)
    pub gamma_i: Scalar<Secp256k1>,// Rastgele maskeleme degeri
    pub g_gamma_i: Point<Secp256k1>,// g^gamma_i
}
```

`delta_i = k_i * gamma_i + sum(alpha_ij + beta_ij)` (MtA protokolu)
`sigma_i = k_i * w_i + sum(mu_ij + nu_ij)` (MtAwc protokolu)

---

## 3. Key Generation: Round-by-Round

### GG20 Keygen Toplam Round Sayisi: 4

(GG18 ile ayni round sayisi, ancak Round 0'daki broadcast mesaji cok daha zengin — ZKP'ler eklenmis)

```
Round 0 → broadcast: KeyGenBroadcastMessage1 (Paillier EK + commitment + ZKP'ler)
Round 1 → broadcast: KeyGenDecommitMessage1  (acik anahtar + decommitment)
Round 2 → P2P:       (VerifiableSS, Scalar)  (VSS schema + gizli pay)
Round 3 → broadcast: DLogProof               (x_i icin bilgi kanitı)
[Final]              LocalKey<Secp256k1>
```

---

### Round 0 (rounds.rs satir 20-52): Keygen Baslangici + Broadcast

**Yapilacaklar:**
1. `Keys::create(party_i)` — rastgele `u_i`, Paillier keypair, N_tilde/h1/h2 uret
2. `phase1_broadcast_phase3_proof_of_correct_key_proof_of_correct_h1h2()` cagir
3. `KeyGenBroadcastMessage1` broadcast et, `KeyGenDecommitMessage1` sakla

**Commitment neden gerekli?**
Party'nin `y_i = g^u_i` degerini once commit etmesi gerekiyor. Eger dogrudan yayinsaydi, baska bir parti kendi `u_j`'sini diger partilerin acik anahtarlarini gordukten sonra sectigi tehlikeli anahtarla (rogue key attack) secebilirdi. Commitment + decommitment sema bu saldiriyi engeller.

**Commitment formulu (satir 241-244):**
```
com_i = H(g^u_i || blind_factor_i)
```
SHA-256 ile `HashCommitment::<Sha256>`. `blind_factor_i` rastgele 256-bit.

**Round 0 Cikarilan ZKP'ler:**
1. `NiCorrectKeyProof` — Paillier modulus N'in iki buyuk asal carpaninin carpimi oldugu NI-ZKP (Paillier anahtarinin gecerliligini kanitlar)
2. `CompositeDLogProof` (base h1) — `h2 = h1^x mod N_tilde` icin DLOG bilgisi kanitı
3. `CompositeDLogProof` (base h2) — `h1 = h2^x_inv mod N_tilde` icin DLOG bilgisi kanitı

Bu ZKP'lerin amaci: N_tilde'nin dogru olusturuldugunu ve h1/h2 arasindaki iliskinin bilindigi gostermek. Bu MtA protokolundeki range proof'larin guvenligini saglar.

**Round0 is_expensive = true** (Paillier key generation agir islem)

---

### Round 1 (rounds.rs satir 54-93): Decommit

**Girdi:** Tum partilerden `KeyGenBroadcastMessage1`
**Cikti (broadcast):** `KeyGenDecommitMessage1` — `{blind_factor, y_i}`

Commit-then-reveal sema. Party tum commitment'lari aldiktan sonra kendi `y_i`'sini aciklar.

**Round1 is_expensive = false** (sadece mesaj gonderimi)

---

### Round 2 (rounds.rs satir 95-161): Commitment Dogrulama + VSS Dagitimi

**Girdi:** Tum partilerden `KeyGenDecommitMessage1`
**Islemler (party_i.rs satir 260-320):**

Her parti `j` icin su dogrulamalar yapilir:
```
1. H(y_j || blind_factor_j) == com_j             # commitment dogrulama
2. correct_key_proof.verify(ek_j).is_ok()        # Paillier dogru mu?
3. ek_j.n.bit_length() ∈ [2047, 2048]            # Paillier boyutu yeterli mi?
4. dlog_statement_j.N.bit_length() ∈ [2047, 2048] # N_tilde boyutu yeterli mi?
5. composite_dlog_proof_base_h1.verify().is_ok()   # h2 = h1^x ZKP
6. composite_dlog_proof_base_h2.verify().is_ok()   # h1 = h2^x_inv ZKP
```
Herhangi biri basarisiz olursa `bad_actors` listesine eklenir ve `Err(ErrorType)` dondurulur.

**VSS Hesaplama ve Dagitimi (satir 313-316):**
```rust
let (vss_scheme, secret_shares) =
    VerifiableSS::share(params.threshold, params.share_count, &self.u_i);
```

Feldman VSS kullaniliyor. `u_i` icin `t`-dereceli polinom uretilir:
```
f_i(x) = u_i + a_1*x + a_2*x^2 + ... + a_t*x^t
```
Commitments: `{g^u_i, g^a_1, ..., g^a_t}` (publiklastiriliyor)
Shares: `{f_i(1), f_i(2), ..., f_i(n)}` (her parti j'ye f_i(j) P2P gonderilir)

**Cikti (P2P):** Her parti j'ye `(vss_scheme_i, f_i(j))` gonderilir
- `vss_scheme_i` — VSS commitments vektoru (public)
- `f_i(j)` — Parti j'nin alacagi gizli pay

**Round2 is_expensive = true** (VSS hesaplamasi + ZKP dogrulamalari)

---

### Round 3 (rounds.rs satir 163-234): VSS Dogrulama + Keypair Olusturma + DLog Kanitı

**Girdi:** Her parti j'den `(vss_scheme_j, share_ji)` (P2P mesajlar)
**Islemler (party_i.rs satir 322-367):**

Her `(vss_scheme_j, share_ji)` icin:
```
1. vss_scheme_j.validate_share(share_ji, index_i).is_ok()  # f_j(i) gerçekten f_j'nin share'i mi?
2. vss_scheme_j.commitments[0] == y_j                       # VSS'in sabit terimi y_j mi?
```

Hepsi gecerliyse:
```rust
let y = sum(y_vec)      // Kolektif acik anahtar: Y = sum_j(g^u_j) = g^(sum u_j)
let x_i = sum(shares)  // Threshold share: x_i = sum_j(f_j(i))
let dlog_proof = DLogProof::prove(&x_i)  // g^x_i icin Schnorr ZKP
```

**Kolektif anahtar matematigi:**
```
Y = g^(u_1 + u_2 + ... + u_n)
x_i = f_1(i) + f_2(i) + ... + f_n(i)  (Lagrange interpolasyonu ile reconstruct edilirse sum(u_j) verir)
```

**Cikti (broadcast):** `DLogProof<Secp256k1, Sha256>` — `x_i` icin Schnorr kanitı
- `pk` = g^x_i
- sigma, challenge (Fiat-Shamir)

**Round3 is_expensive = true**

---

### Round 4 (rounds.rs satir 236-307): DLog Kanit Dogrulama + LocalKey Olusturma

**Girdi:** Tum partilerden `DLogProof`
**Islemler (party_i.rs satir 405-438):**

Her `dlog_proof_j` icin:
```
1. DLogProof::verify(dlog_proof_j).is_ok()       # Schnorr ZKP gecerli mi?
2. xi_commitments[j] == dlog_proof_j.pk          # VSS commitment ile tutarli mi?
```

`xi_commitments` VSS sema'larindan turetiliyor:
```rust
let xi_commitments = Keys::get_commitments_to_xi(vss_vec);
```
Bu fonksiyon (satir 369-388) global VSS polinomunu olusturuyor:
```
global_vss.commitments[k] = sum_j(vss_j.commitments[k])
commitment_to_xi = global_vss.evaluate_at(i)
```
Yani: `g^(f_1(i) + ... + f_n(i)) = g^x_i` hesaplaniyor ve DLog proof'taki `pk`'ya karsilastiriliyor.

Dogrulama gecerliyse `LocalKey` olusturuluyor.

**Round4 is_expensive = true**

---

## 4. State Machine Mimarisi

### `StateMachine` Trait (keygen.rs satir 187-338)
`round_based` kutuphanesinden geliyor. Temel metodlar:

**`handle_incoming(msg: Msg<ProtocolMessage>) -> Result<()>` (satir 192-265)**
- Mesaji round'una gore ilgili `Store`'a push eder
- Her push sonrasi `proceed_round(false)` cagrilir (engelleme yapmadan)
- Yanlis round mesaji: `ReceivedOutOfOrderMessage` hatasi
- Store hatasi: `HandleMessage(StoreErr)`

**`message_queue() -> &mut Vec<Msg<ProtocolMessage>>` (satir 267-269)**
- Gonderilecek mesajlarin kuyrugundan referans dondurur
- Runtime bu kuyrugu okuyup diger partilere iletir

**`wants_to_proceed() -> bool` (satir 271-285)**
- Su andaki round icin tum mesajlar alindi mi?
- `Round0` icin daima `true` (hic mesaj beklemiyor)
- `Round1-4` icin ilgili store'un `wants_more()` degili

**`proceed() -> Result<()>` (satir 287-289)**
- `proceed_round(true)` cagrilir — agir islemlere izin verilir
- Runtime bunu sadece `wants_to_proceed()` true olduğunda cagirmali

**`round_timeout() -> Option<Duration>` (satir 291-293)**
- `None` — timeout uygulanmamis
- Nova Shield icin bu bir risk: yavas/cevap vermeyen parti protokolu sonsuza kadar bloke edebilir

**`is_finished() -> bool` (satir 299-301)**
- `matches!(self.round, R::Final(_))`

**`pick_output() -> Option<Result<LocalKey<Secp256k1>>>` (satir 303-314)**
- Bir kez cagrilabilir (iki kez: `DoublePickOutput` hatasi)
- `R::Gone` durumuna gecer

### `Msg<M>` Yapisi
```rust
Msg {
    sender: u16,           // Gonderen parti (1-tabanli)
    receiver: Option<u16>, // None = broadcast, Some(j) = P2P
    body: M,               // Mesaj icerigi
}
```

### Round Enum (satir 402-410)
```rust
enum R {
    Round0(Round0),   // Baslangic
    Round1(Round1),   // BC1 bekleniyor
    Round2(Round2),   // Decommit bekleniyor
    Round3(Round3),   // VSS paylasimlari bekleniyor (P2P)
    Round4(Round4),   // DLog proof'lar bekleniyor
    Final(LocalKey<Secp256k1>),  // Bitti
    Gone,             // Output alindi
}
```

### Mesaj Tipleri (satir 420-426)
```rust
enum M {
    Round1(KeyGenBroadcastMessage1),              // BC1: Commitment + Paillier + ZKP'ler
    Round2(KeyGenDecommitMessage1),               // Decommit: y_i + blind_factor
    Round3((VerifiableSS<Secp256k1>, Scalar<Secp256k1>)), // VSS schema + share (P2P)
    Round4(DLogProof<Secp256k1, Sha256>),         // Schnorr DLog proof
}
```

### `RoundBlame` Trait (satir 340-358)
```rust
fn round_blame(&self) -> (u16, Vec<u16>)
// Donder: (beklenen_mesaj_sayisi, mesaj_gondermeyen_partiler)
```
Store'larin `blame()` metodunu proxy olarak cagiriyor. Bu, timeout durumunda hangi partinin mesaj gondermedigini tespit etmek icin kullanilir.

### Hata Tipleri (satir 434-466)
```rust
pub enum Error {
    ProceedRound(ProceedError),         // Round isleme hatasi (alt hata)
    TooFewParties,                      // n < 2
    InvalidThreshold,                   // t == 0 || t >= n
    InvalidPartyIndex,                  // i == 0 || i > n
    HandleMessage(StoreErr),            // Mesaj store'a push edilemedi
    ReceivedOutOfOrderMessage { current_round, msg_round }, // Yanlis round mesaji
    DoublePickOutput,                   // pick_output iki kez cagirildi
    InternalError(InternalError),       // Bug: StoreGone, RetrieveRoundMessages
}
```

`ProceedError` alttipleri (rounds.rs satir 340-347):
```rust
pub enum ProceedError {
    Round2VerifyCommitments(ErrorType),    // Commitment/ZKP dogrulama hatasi
    Round3VerifyVssConstruct(ErrorType),   // VSS dogrulama hatasi
    Round4VerifyDLogProof(ErrorType),      // DLog proof dogrulama hatasi
}
```

`ErrorType` (`mod.rs` satir 23-27):
```rust
pub struct ErrorType {
    error_type: String,      // Hata mesaji
    bad_actors: Vec<usize>,  // Suclanan parti indisleri
}
```
**DİKKAT**: `bad_actors` ve `error_type` alanlari `pub` degil — sadece modul icinden erisilebilir. Nova Shield entegrasyonu icin bu alanlar acilmali.

---

## 5. Blame Mekanizmasi (blame.rs)

Blame mekanizmasi yalnizca **signing (imzalama)** sirasinda aktive olur. Keygen'de hatalar dogrudan `bad_actors` listesi ile dondurulur.

### Signing Blame Mimarisi: 3 Fase

#### Phase5 Blame: Delta Tutarsizligi
**Ne zaman tetiklenir:** `phase5_check_R_dash_sum` basarisiz olursa (satir 768-776, party_i.rs)
- `sum(R_dash_i) == sum(k_i * R) == R * sum(k_i) ?` kontrolu
- Basarisizlik: Birinin `delta_i`'si yanlis

**`LocalStatePhase5` (satir 33-40):**
```rust
pub struct LocalStatePhase5 {
    pub k: Scalar<Secp256k1>,          // Partinin k_i degeri
    pub k_randomness: BigInt,           // Paillier randomness (k'nin sifrelemesinde kullanilan)
    pub gamma: Scalar<Secp256k1>,       // Partinin gamma_i degeri
    pub beta_randomness: Vec<BigInt>,   // MtA'da beta terimleri icin randomness
    pub beta_tag: Vec<BigInt>,          // MtA'da beta' terimleri
    pub encryption_key: EncryptionKey,  // Partinin Paillier EK'si
}
```

**`GlobalStatePhase5.phase5_blame` (satir 116-224):**
1. `g^gamma_i == Point::generator() * gamma_i` kontrolu (decommit dogrulama)
2. `MessageA.a_with_predefined_randomness(k_i, ek_i, k_randomness)` ile gonderilen MA mesaji yeniden hesaplanir ve kaydedilenle karsilastirilir
3. Herkes tutarliysa: `delta_i`'yi tum MtA alpha/beta terimleriyle yeniden hesapla
4. Yeniden hesaplanan `delta_i` != broadcast edilen `delta_i` ise: suclanan parti

**Blame kaniti oncesi gerekli:** Tum partilerin acilmis `k_i, gamma_i` degerleri ve Paillier randomness'lari tutarli olmali. Tutarsizlik durumunda ilk suclananlar onlar olur.

#### Phase6 Blame: Sigma/S_i Tutarsizligi
**Ne zaman tetiklenir:** `phase6_check_S_i_sum` basarisiz olursa (satir 835-848, party_i.rs)
- `sum(S_i) == Y` (kolektif acik anahtar) kontrolu
- `S_i = R * sigma_i` olmali

**`LocalStatePhase6` (satir 228-234):**
```rust
pub struct LocalStatePhase6 {
    pub k: Scalar<Secp256k1>,
    pub k_randomness: BigInt,
    pub miu: Vec<BigInt>,           // mu degerleri BigInt olarak (reduce oncesi)
    pub miu_randomness: Vec<BigInt>,// Paillier randomness
    pub proof_of_eq_dlog: ECDDHProof<Secp256k1, Sha256>, // sigma_i dogrulama
}
```

**`GlobalStatePhase6.phase6_blame` (satir 322-419):**
1. Paillier re-encryption ile `miu_ij` degerlerinin dogrulugu kontrol edilir
2. `k_i`'nin broadcast edilen `MessageA`'ya uygunlugu kontrol edilir
3. Tutarliysa: `g_sigma_i` hesaplanir
4. `ECDDHProof` (EC-DDH es denklilik kanitı) dogrulanir:
   ```
   statement: (g, R, g^sigma_i, R^sigma_i = S_i)
   yani: log_g(g^sigma_i) == log_R(S_i)
   ```
5. Gecersiz proof sahibi suclaniyor

**`GlobalStatePhase7.phase7_blame` (satir 434-455):**
```rust
// Her i icin:
// R * s_i == R_dash_i * m + S_i * r  mi kontrol et
let R_si = &self.R * &self.s_vec[i];
let right = R_dash_m + Si_r;  // R_dash_i*m + S_i*r
if left != right { bad_signers_vec.push(i) }
```
Basarisiz imza parcalari olan partileri tespit eder.

### Blame Zinciri
```
Signing basarisiz
    → Phase5 check basarisiz? → phase5_blame() → bad_actors
    → Phase6 check basarisiz? → phase6_blame() → bad_actors
    → Phase7 (output_signature) basarisiz? → phase7_blame() → bad_actors
```

---

## 6. Test Analizi

### Keygen Testleri (test.rs satir 41-67)
```rust
fn test_keygen_t1_n2()  // 1-of-2: minimum threshold
fn test_keygen_t2_n3()  // 2-of-3: klasik 3-parti setup
fn test_keygen_t2_n4()  // 2-of-4
```

**`keygen_t_n_parties` fonksiyonu** (satir 150-280) tam keygen protokolunu simule eder:
1. `n` parti olustur
2. Round1 mesajlarini uret ve yayinla
3. Her parti tum commitment'lari dogrular + VSS hesaplar
4. Gizli paylari P2P ilet
5. Her parti aldigi VSS paylarini dogrular + DLog kanitlari uret
6. DLog kanitleri dogrula
7. Dogrulama: `vss[0].reconstruct(&indices, &xi_vec) == sum(u_i)` (satir 263-269)

### Sign Testleri (satir 56-148)
Parametreler: `sign(t, n, ttag, s, corrupt_step, corrupted_parties)`

**Normal imzalama testleri:**
```
test_sign_n2_t1_ttag1   (1-of-2, 2 parti)
test_sign_n5_t2_ttag4   (2-of-5, 4 parti imzalar)
test_sign_n8_t4_ttag6   (4-of-8, 6 parti imzalar)
```

**Blame testleri (phase 5):**
```
corrupt_step=5: delta_i yanlis (delta = delta + delta)
→ test_sign_n2_t1_ttag1_corrupt_step5_party1: bad_actors == [0]
→ test_sign_n2_t1_ttag1_corrupt_step5_party12: bad_actors == [0, 1]
→ test_sign_n5_t2_ttag4_corrupt_step5_party14: bad_actors == [0, 3]
```

**Blame testleri (phase 6):**
```
corrupt_step=6: sigma_i yanlis (sigma = sigma + sigma)
→ Ayni pattern, farkli phase
```

**Blame testleri (phase 7):**
```
corrupt_step=7: s_i yanlis (s = s + s)
→ test_sign_n2_t1_ttag1_corrupt_step7_party2: bad_actors == [1]
→ test_sign_n5_t2_ttag4_corrupt_step7_party24: bad_actors == [1, 3]
```

**State machine keygen testleri (keygen.rs satir 514-527):**
```rust
fn simulate_keygen_t1_n2()
fn simulate_keygen_t1_n3()
fn simulate_keygen_t2_n3()
```
`round_based::dev::Simulation` kullaniliyor — gercek network simulasyonu, benchmark'lar aktif.

**`test_small_paillier` (satir 765-783):**
2046-bit Paillier modulus reddediliyor. Bu onemli bir guvenlik testi.

**`test_serialize_deserialize` (satir 750-763):**
`KeyGenBroadcastMessage1` ve `KeyGenDecommitMessage1` JSON serde roundtrip.

---

## 7. Guvenlik Degerlendirmesi

### Guclu Yonler

1. **Commitment sema (satir 241-244):** Rogue key attack'i engeller. `y_i` once commit, sonra reveal.

2. **Paillier modulus boyutu zorlama (satir 287-290):** 2047-2048 bit. Kucuk modulus saldirilarini engeller.

3. **NI-CorrectKeyProof (satir 283-285):** Paillier'in dogru olusturuldugunu kanitlar — zayif Paillier anahtari ile MtA'yi manipule edemezsin.

4. **CompositeDLogProof for h1,h2 (satir 291-298):** Range proof parametrelerinin gecerligini kanitlar.

5. **VSS commit-share tutarliligi (satir 340):** `vss_scheme_j.commitments[0] == y_j` — VSS sabit terimi y_j'ye esdeger olmali.

6. **DLog proof vs VSS commitment carpi-dogrulama (satir 418):** `xi_commitments[i] == dlog_proof_i.pk` — VSS sema'larindan turetilen commitment ile Schnorr proof'taki public key tutarli olmali.

7. **Identifiable abort:** Blame mekanizmasi kotu aktoru tanimlama yapabiliyor.

### Zayif Yonler / Eksiklikler

1. **Safe prime kullanilmiyor (satir 139-140):**
   ```rust
   // note, should be safe primes:
   let (ek_tilde, dk_tilde) = Paillier::keypair().keys();  // NOT safe primes!
   ```
   `generate_h1_h2_N_tilde` safe prime kullanmiyor. `create()` de safe prime kullanmiyor (satir 162). Sadece `create_safe_prime()` ve `refresh_private_key_safe_prime()` guvenli. **Nova Shield mutlaka safe prime kullanmali.**

2. **Timeout yok (satir 291-296):**
   ```rust
   fn round_timeout(&self) -> Option<Duration> { None }
   fn round_timeout_reached(&mut self) -> Self::Err { panic!("no timeout was set") }
   ```
   Cevap vermeyen parti protokolu sonsuz bloke eder.

3. **Phase5 blame'de TODO (blame.rs satir 57-58):**
   ```
   // TODO: check all parties submitted inputs
   // TODO: if not - abort gracefully with list of parties that did not produce inputs
   ```
   Eksik girdi durumu tam olarak ele alinmamis.

4. **Keygen'de indexleme (test.rs satir 235-236):**
   ```rust
   (&index_vec[i] + 1).into()
   ```
   0-tabanli `party_index` ile 1-tabanli state machine indisleri arasinda karmasiklik var.

5. **Paralel randomness (test.rs satir 327-330):**
   ```
   // we assume here that party sends the same encryption to all other parties.
   // It should be changed to different encryption (randomness) to each counter party
   ```
   Uretimde farkli randomness kullanilmali.

6. **`bad_actors` alanlari private (mod.rs satir 23-27):**
   `ErrorType.bad_actors` ve `ErrorType.error_type` `pub` degil. Dis kutuphaneler bu bilgilere erisemez. Entegrasyon icin sorun.

---

## 8. Nova Shield Chain Icin Cikarsimlar

### Direkt Kullanilabilir Yapi

| Bilesyen | Kullanilabilirlik | Not |
|----------|------------------|-----|
| `Keygen` state machine | Direkt | round_based uyumlu |
| `LocalKey` | Direkt | Sign protokolune giris |
| Blame mekanizmasi | Direkt | Phase5/6/7 |
| `Parameters` struct | Direkt | threshold, share_count |
| `ErrorType` | Degistir | bad_actors'i pub yap |

### Kritik Uretim Ayarlari

```rust
// HATALI (varsayilan):
Keys::create(index)           // safe prime yok
// DOGRU:
Keys::create_safe_prime(index) // uretimde bu kullanilmali
```

N_tilde icin de safe prime kullanmak gerekiyor ama `generate_h1_h2_N_tilde` fonksiyonu bunu yapmıyor. Bu guvenlik acigi acilmis ama duzeltilmemis.

### Threshold Parametreleri

Nova Shield icin onerilen konfigurasyonlar:
```
2-of-3: t=1, n=3  (minimum guvenli setup)
3-of-5: t=2, n=5  (production icin standart)
4-of-7: t=3, n=7  (yuksek guvenlik)
```
NOT: Parametre isimlendirmesi dikkatli yapilmali — `threshold` (t) "minimum - 1" anlaminda. `t+1` parti imzalayabilir.

### State Machine Entegrasyon Akisi

```
Nova Shield Node
    ↓ Keygen::new(i, t, n)
    ↓ proceed() → msgs_queue'dan mesajlar al
    ↓ broadcast/P2P mesajlari diger nodlara ilet
    ↓ handle_incoming(msg) diger partilerden mesajlar icin
    ↓ Dongu: wants_to_proceed() && proceed()
    ↓ is_finished() → pick_output() → LocalKey
    ↓ LocalKey'i guvenli depola (HSM ya da encrypted disk)
```

### Timeout Eklenmeli

```rust
// round_based::StateMachine timeout destekliyor
// Uygulama tarafinda eklenebilir:
fn round_timeout(&self) -> Option<Duration> {
    Some(Duration::from_secs(30)) // her round icin 30 saniye
}
```

### Agirlik ve Performans

- Round0 ve Round2-4: `is_expensive = true` — async ortamda bu roundlara `.await` oncesi kontrol ekle
- Paillier keypair uretimi (2048-bit): ~50-200ms (safe prime: daha uzun)
- Tum keygen (3 parti, t=1): benchmark ile olculebilir (`simulate_keygen` bunu yapiyor)

### Guvenlik Garantisi

GG20 asagidaki saldiri modellerine karsi guvenlidir:
- **Threshold asilmadan anahtar carpilamaz:** t+1'den az parti bulusamaz
- **Rogue key attack:** Commitment sema ile onleniyor
- **Kotu Paillier secimi:** NiCorrectKeyProof ile onleniyor
- **Yanlis range parametreleri:** CompositeDLogProof ile onleniyor
- **Identifiable abort:** Phase5/6/7 blame mekanizmasi ile

**Guvenilmeyen varsayim:** Senkron mesajlasma modeli. Nova Shield P2P katmanininda round bazli mesaj toplama mekanizmasi kurulmali, aksi takdirde devam etmeyen parti (timeout yok) sistemi bloke eder.

---

## Ozet: Kritik Degerler

| Deger | Aciklama |
|-------|----------|
| Paillier modulus | Tam 2047-2048 bit (`PAILLIER_MIN/MAX_BIT_LENGTH`) |
| SECURITY | 256 bit (commitment randomness) |
| Commitment formulu | `H(g^u_i \|\| blind_factor)`, SHA-256 |
| VSS polinom derecesi | `t` (threshold) |
| Kolektif acik anahtar | `Y = sum(y_i) = g^(sum(u_i))` |
| Threshold share | `x_i = sum_j(f_j(i))` |
| DLog ZKP | Schnorr (curv kütüphanesi, SHA-256 Fiat-Shamir) |
| Phase5 blame tetik | `sum(k_i * R) != R * k_toplam` |
| Phase6 blame tetik | `sum(sigma_i * R) != Y` |
| Phase7 blame kontrol | `R * s_i == R_dash_i * m + S_i * r` |
| Round sayisi (keygen) | 4 (Round0..Round4, Final) |
| Mesaj tipleri Round1/2 | Broadcast |
| Mesaj tipleri Round3 | P2P (her parti'ye farkli) |
| Mesaj tipleri Round4 | Broadcast |
| Uretimde kullanilmasi gereken | `Keys::create_safe_prime()` |
