# Agent-03: GG20 Signing Protokol Analizi

## Kapsanan Dosyalar

| Dosya | Satir Sayisi | Aciklama |
|-------|-------------|----------|
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/sign.rs` | 765 | OfflineStage ve SignManual state machine |
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/sign/rounds.rs` | 731 | Round0-Round7 implementasyonu |
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/sign/fmt.rs` | 126 | Debug ve progress formatlamasi |
| `src/protocols/multi_party_ecdsa/gg_2020/state_machine/traits.rs` | 7 | RoundBlame trait tanimi |
| `src/protocols/multi_party_ecdsa/gg_2020/party_i.rs` | 936 | SignKeys, LocalSignature, verify implementasyonu |
| `src/utilities/mta/mod.rs` | 215 | MtA protokolu (MessageA, MessageB) |
| `examples/gg20_keygen.rs` | 59 | Keygen example |
| `examples/gg20_signing.rs` | 91 | Signing example |
| `examples/gg20_sm_client.rs` | 160 | HTTP istemci implementasyonu |
| `examples/gg20_sm_manager.rs` | 194 | HTTP sunucu (Rocket) implementasyonu |

---

## 1. Signing Protokol Genel Bakis

GG20 signing protokolu **iki fazdan** olusur:

### 1.1 Offline Stage (Cok-Roundlu, Mesaj-Agir)
Sabit bir imzaci seti icin onceden hesaplanir. `OfflineStage` state machine'i uygular. 6 round mesaj alisverisi gerektirir. Ciktisi `CompletedOfflineStage` bir defadan fazla kullanilamaz — her mesaj icin yeni bir offline stage gerekir.

### 1.2 Online Stage / SignManual (Tek Round, Hafif)
`SignManual` state machine **uygulamaz**, sadece iki method sunar:
- `new(message, completed_offline_stage)` — lokal partial signature uretir
- `complete(sigs)` — diger partilerin partial signature'larini toplayip final imzayi olusturur

Referans: `sign.rs:1-16` (dokumantasyon blogu)

### 1.3 Protokol Parametreleri
- `n` = toplam parti sayisi (imzalayanlar)
- `t` = esik (threshold) — keygen'de kullanilan VSS parametresi, `t+1` parti gerekir
- `s_l` = signing'e katilan partilerin keygen indeksleri (Vec<u16>)
- `i` = bu partinin signing icindeki indeksi (1-based)

---

## 2. Round-by-Round Analiz

### 2.1 Round 0: Nonce ve Commitment Uretimi

**Kaynak:** `rounds.rs:67-109` (Round0::proceed), `party_i.rs:546-589` (SignKeys::create, phase1_broadcast)

**Yapilan islemler:**

1. **w_i hesabi (Lagrange interpolasyonu ile weighting):**
   ```
   li = lagrange_coefficient(index, s_l)   // VSS Lagrange katsayisi
   w_i = li * x_i                          // agirliklandirilmis secret share
   g_w_i = G * w_i
   ```
   `party_i.rs:553-560`: `map_share_to_new_params` Feldman VSS uzerinden Lagrange katsayisi hesaplar.

2. **Nonce uretimi:**
   ```
   gamma_i = random() mod q    // imza nonce
   k_i     = random() mod q    // Paillier encryption icin nonce
   g_gamma_i = G * gamma_i
   ```
   `party_i.rs:561-570`

3. **Commitment (Phase 1 broadcast):**
   ```
   blind_factor = random(256-bit)
   com = Hash(g_gamma_i || blind_factor)   // Pedersen-benzeri commitment
   ```
   `party_i.rs:573-589`

4. **Paillier Sifreleme (MessageA):**
   ```
   c_A = Paillier_Enc(ek_i, k_i; randomness)
   range_proof = AliceProof(k_i, c_A, ek_i, h1, h2, N_tilde_j)  // her j icin
   ```
   `mta/mod.rs:52-88`

**Yayinlanan mesaj (Broadcast M1):** `(MessageA, SignBroadcastPhase1)` = `(c_A + range_proofs, com)`

`rounds.rs:82-101`: Round1 state'e gecis.

---

### 2.2 Round 1: MtA (Multiplicative-to-Additive) Protokolu

**Kaynak:** `rounds.rs:121-218` (Round1::proceed), `mta/mod.rs:91-210` (MessageB::b, verify_proofs_get_alpha)

GG20'de **iki paralel MtA** calisir her (i,j) cifti icin:
- **MtA(k_i, gamma_j):** delta_i hesabi icin
- **MtA(k_i, w_j):** sigma_i hesabi icin

**MessageB::b() — Bob tarafindaki MtA adimi** (`mta/mod.rs:91-158`):

Alice'in `MessageA`sini aldiktan sonra Bob (`gamma_i` veya `w_i` sahibi) su islemi yapar:

1. Alinan `c_A = Enc(k_i)` dogrulanir (AliceProof range proof)
2. Rastgele `beta_tag` secilir (gizli blinding faktoru)
3. Homomorfik islemler:
   ```
   c_beta_tag = Enc(beta_tag; randomness_B)
   b_times_c_A = Paillier_Mul(c_A, b)        // = Enc(k_i * b)
   c_B = Paillier_Add(b_times_c_A, c_beta_tag) // = Enc(k_i * b + beta_tag)
   ```
   `mta/mod.rs:133-145`
4. Bob'un lokal degeri:
   ```
   beta = -beta_tag mod q
   ```
   `mta/mod.rs:146`
5. DLog kaniti: `b_proof = NIZK_DLog(b)`, `beta_tag_proof = NIZK_DLog(beta_tag_fe)`

**Alice'in MtA cevabini isleme (verify_proofs_get_alpha)** (`mta/mod.rs:160-179`):

Alice kendi dk ile `c_B` yi decrypt eder:
```
alpha = Paillier_Dec(dk_i, c_B) mod q
      = k_i * b + beta_tag mod q
```
Dogrulama: `G*alpha == G*(k_i*b) + G*beta_tag == b_proof.pk * k_i + beta_tag_proof.pk`

**MtA iliskisi:**
```
alpha_ij + beta_ji = k_i * gamma_j mod q    // gamma MtA icin
miu_ij  + ni_ji   = k_i * w_j   mod q     // w MtA icin
```
Burada `alpha_ij` Alice (parti i) in aldigi deger, `beta_ji` Bob (parti j) in lokal degeri.

**Yayinlanan mesaj (P2P M2):** `(GammaI(MessageB_gamma), WI(MessageB_w))` — her parti her diger partiye P2P gonderir

`rounds.rs:183-192`

---

### 2.3 Round 2: Delta_i ve Sigma_i Hesabi

**Kaynak:** `rounds.rs:233-326` (Round2::proceed), `party_i.rs:591-618` (phase2_delta_i, phase2_sigma_i)

Parti kendi alpha ve beta degerlerini toplayarak partial contribution'lari olusturur:

**delta_i hesabi** (`party_i.rs:591-604`):
```
ki_gamma_i = k_i * gamma_i
delta_i = k_i * gamma_i + sum_j(alpha_ij_gamma + beta_ij_gamma) mod q
```
Kod: `let ki_gamma_i = &self.k_i * &self.gamma_i;` sonra `alpha_vec[i] + beta_vec[i]` toplanir.

**Nihai MtA toplami iliskisi:**
```
sum_i(delta_i) = sum_i(k_i * gamma_i) + sum_{i!=j}(alpha_ij + beta_ji)
              = sum_i(k_i * gamma_i) + sum_{i!=j}(k_i * gamma_j)
              = (sum_i k_i) * (sum_j gamma_j)
              = k * gamma    (mod q)
```

**sigma_i hesabi** (`party_i.rs:606-618`):
```
ki_w_i = k_i * w_i
sigma_i = k_i * w_i + sum_j(miu_ij + ni_ij) mod q
```
Toplam: `sum_i(sigma_i) = k * x` (joint secret key)

**T_i ve Pedersen Proof** (`party_i.rs:620-634`):
```
l_i = random() mod q
T_i = G*sigma_i + H*l_i     // H = base_point2 (farkli generator)
T_i_proof = PedersenProof(sigma_i, l_i)
```
T_i, sigma_i'yi gizleyen bir Pedersen commitment'dir.

**Yayinlanan mesaj (Broadcast M3):** `(DeltaI(delta_i), TI(t_i), TIProof(t_i_proof))`

`rounds.rs:291-298`

---

### 2.4 Round 3: delta_inv Hesabi ve Decommitment

**Kaynak:** `rounds.rs:346-411` (Round3::proceed), `party_i.rs:635-640` (phase3_reconstruct_delta)

1. Tum `delta_i` degerleri toplanir ve inversi alinir:
   ```
   delta = sum_i(delta_i) = k * gamma mod q
   delta_inv = (k * gamma)^(-1) mod q
   ```
   `party_i.rs:635-640`

2. Pedersen proof'lar dogrulanir: `PedersenProof::verify(proof)` her T_i icin
   `rounds.rs:371-377`

3. Phase 1 decommitment yayinlanir (g_gamma_i'yi ortaya cikarir):
   **Yayinlanan mesaj (Broadcast M4):** `SignDecommitPhase1 { blind_factor, g_gamma_i }`

`rounds.rs:380-384`

---

### 2.5 Round 4: R Noktasi Hesabi ve PDL Proof

**Kaynak:** `rounds.rs:430-507` (Round4::proceed), `party_i.rs:642-687` (SignKeys::phase4)

**R noktasinin matematik temeli:**

```
Gamma = sum_i(g_gamma_i)    // tum g_gamma_i noktalarinin toplami
R = Gamma * delta_inv
  = (G * gamma) * (k * gamma)^(-1)
  = G * (gamma / (k * gamma))
  = G * (1/k)
  = G / k
```

**Neden R = G/k?** Cunku standart ECDSA'da `R = k * G` yerine threshold'da gizli `k = sum k_i` kullanilir ve `Gamma = gamma * G` oldugunda `Gamma * delta_inv = G * gamma * (k*gamma)^-1 = G * k^-1`.

**Kod uygulamasi** (`party_i.rs:669-686`):
```rust
let gamma_sum = tail.fold(head.g_gamma_i.clone(), |acc, x| acc + &x.g_gamma_i);
Ok(gamma_sum * delta_inv)  // R = Gamma * delta_inv
```

Oncesinde: decommitment dogrulanir (`bc1_vec[ind].com` ile `Hash(g_gamma_i, blind_factor)` eslestirme), `b_proof.pk == phase1_decommit_vec[ind].g_gamma_i` kontrolu.

**R_dash hesabi:**
```
R_dash_i = R * k_i      // her parti icin
```
`rounds.rs:452`

**PDL with Slack Proof** — R_dash_i'nin dogru k_i ile hesaplandigi kanitlanir:
- Statement: `Q = G * k_i`, `c_A = Enc(k_i)`, yani Paillier sifreli k_i ile `R * k_i` tutarli
- `phase5_proof_pdl()` her diger j icin, j'nin `h1,h2,N_tilde`si kullanilarak kanit uretir
- `party_i.rs:691-717`

**Yayinlanan mesaj (Broadcast M5):** `(RDash(R_dash_i), Vec<PDLwSlackProof>)`

---

### 2.6 Round 5: PDL Dogrulama ve S_i Hesabi

**Kaynak:** `rounds.rs:524-601` (Round5::proceed), `party_i.rs:719-799`

**PDL kanit dogrulama** (`party_i.rs:719-766`):
- Her parti `phase5_verify_pdl()` ile diger tum partilerin PDL kanitlarini dogrular
- Sahtekarligi kanitsiz reddetmek mumkun

**R_dash toplami kontrolu** (`party_i.rs:768-776`):
```
sum_i(R_dash_i) = sum_i(R * k_i) = R * sum_i(k_i) = R * k = G * k^-1 * k = G
```
Kontrol: `sum(R_dash_i) == G` — bu esitlik bozuluyorsa hata firlatirir.

**S_i hesabi** (`party_i.rs:778-799`):
```
S_i = R * sigma_i
```
S_i uzerine Homomorphic ElGamal kaniti uretilir. Bu kanit, `T_i = G*sigma_i + H*l_i` ile tutarlilik kanitlar:
```
Statement: G, H = base_point2, Y = G (generator), D = T_i, E = S_i
Witness: r = sigma_i, x = l_i
Proof: S_i = G * sigma_i AND T_i = G*sigma_i + H*l_i
```

**Yayinlanan mesaj (Broadcast M6):** `(SI(S_i), HEGProof(homo_elgamal_proof))`

`rounds.rs:572-577`

---

### 2.7 Round 6: Son Dogrulama ve CompletedOfflineStage Uretimi

**Kaynak:** `rounds.rs:611-645` (Round6::proceed), `party_i.rs:801-848`

**HomoELGamal proof dogrulama** (`party_i.rs:801-833`):
- Tum `(S_i, HEGProof_i, T_i)` ucclusu dogrulanir
- `phase6_verify_proof()` her parti icin HomoElGamalStatement kurar ve verify eder

**S_i toplami kontrolu** (`party_i.rs:835-848`):
```
sum_i(S_i) = sum_i(R * sigma_i) = R * sum_i(sigma_i) = R * k * x = G * x = pubkey Y
```
`phase6_check_S_i_sum(pubkey_y, S_i_vec)` — bu kontrol basarisiz olursa `Phase6Error` donulur.

**CompletedOfflineStage** olusturulur ve dondurulur:
```rust
CompletedOfflineStage { i, local_key, sign_keys, t_vec, R, sigma_i }
```
`rounds.rs:583-591`

---

### 2.8 Round 7 (Online — SignManual): Partial Signature

**Kaynak:** `rounds.rs:667-693` (Round7), `party_i.rs:850-910` (phase7_local_sig, output_signature)

**phase7_local_sig** (`party_i.rs:850-871`):
```
r = R.x_coord() mod q          // ECDSA r degeri
m_fe = BigInt_to_Scalar(message)
s_i = m_fe * k_i + r * sigma_i  // partial signature
```

**output_signature — Final Aggregation** (`party_i.rs:873-910`):
```
s = sum_i(s_i)
  = sum_i(m * k_i + r * sigma_i)
  = m * sum_i(k_i) + r * sum_i(sigma_i)
  = m * k + r * (k * x)
  = k * (m + r * x)
```
Klasik ECDSA'da `s = k^-1 * (m + r*x)` oldugunda, burada `R = G * k^-1` kullanildigindan:
```
r = R.x = (G/k).x
```
Bu dogrulama icin: `s^-1 * (m*G + r*Y) = (k/(m + rx)) * (m*G + rx*G) = k * G = G/k-den degil`

Bekle — asil aciklama: `R = G * k^-1` (bu GG20 konvansiyonudur), bu durumda:
```
s_i = m * k_i + r * sigma_i
s = m * k + r * sigma    (sigma = k * x)
  = m * k + r * k * x
  = k * (m + r * x)
```
Dogrulama: `s^-1 * G * m + s^-1 * Y * r = G * m/(k*(m+rx)) + G * x * r/(k*(m+rx))`
`= G * (m + rx) / (k * (m+rx)) = G * 1/k = G * k^-1 = R` — x koordinati `r` ile uyusuyor.

**Low-S normalizasyonu** (`party_i.rs:896-902`):
```
s_tag = q - s
if s > s_tag: s = s_tag, recid ^= 1
```
Malleable imza onlemi.

**Recovery ID (recid)** (`party_i.rs:896-902`):
```
is_ry_odd = R.y.test_bit(0)
recid = if is_ry_odd { 1 } else { 0 }
if s > q/2: recid ^= 1
```

**Son dogrulama:** `verify(&sig, &pubkey, &message)` basarisiz olursa `InvalidSig` hatasi.

---

## 3. State Machine

### 3.1 OfflineStage Yapisi

```rust
pub struct OfflineStage {
    round: OfflineR,           // Mevcut round
    msgs1..msgs6: Option<Store<...>>,  // Her round icin mesaj tamponlari
    msgs_queue: MsgQueue,      // Gonderilecek mesajlar
    party_i: u16,
    party_n: u16,
}
```
`sign.rs:51-65`

### 3.2 State Enumu

```rust
enum OfflineR {
    R0(Round0),                      // Baslangic: nonce uretimi
    R1(Round1),                      // M1 bekleniyor (broadcast: MessageA + com)
    R2(Round2),                      // M2 bekleniyor (P2P: GammaI + WI)
    R3(Round3),                      // M3 bekleniyor (broadcast: DeltaI + TI + TIProof)
    R4(Round4),                      // M4 bekleniyor (broadcast: SignDecommitPhase1)
    R5(Round5),                      // M5 bekleniyor (broadcast: RDash + PDLProofs)
    R6(Round6),                      // M6 bekleniyor (broadcast: SI + HEGProof)
    Finished(CompletedOfflineStage), // Tamamlandi
    Gone,                            // pick_output cagrildi
}
```
`sign.rs:466-476`

### 3.3 Round Gecis Mantigi

`proceed_round()` fonksiyonu her round icin uc sart kontrol eder (`sign.rs:126-249`):
1. Mesaj store'u dolu mu (`!storeN_wants_more`)
2. Round pahali mi (`is_expensive()`) ve `may_block` izni var mi
3. `replace(&mut self.round, OfflineR::Gone)` ile atomik gecis

**try_again mekanizmasi:** R0 -> R1 gecisinde `try_again = true` set edilir, yani R0 islendikten sonra R1 icin otomatik devam etmeye calisilir. `sign.rs:136-141`

### 3.4 Mesaj Yonlendirmesi

| Message ID | Tip | Yon | Icerik |
|-----------|-----|-----|--------|
| M1 | Broadcast | i → all | `(MessageA, SignBroadcastPhase1)` |
| M2 | P2P | i → j (her j icin) | `(GammaI, WI)` |
| M3 | Broadcast | i → all | `(DeltaI, TI, TIProof)` |
| M4 | Broadcast | i → all | `SignDecommitPhase1` |
| M5 | Broadcast | i → all | `(RDash, Vec<PDLwSlackProof>)` |
| M6 | Broadcast | i → all | `(SI, HEGProof)` |

`sign.rs:483-490` (OfflineM enum), `sign.rs:510-517` (make_pushable! macro)

**Not:** M2 P2P'dir — n-1 mesaj gonderilir her parti tarafindan. Diger tum mesajlar broadcast.

### 3.5 Timeout

`round_timeout()` `None` dondurur — timeout implementasyonu yoktur!
`round_timeout_reached()` `panic!` cagririr.
`sign.rs:389-395`

Bu production kullanim icin onemli bir eksikliktir.

### 3.6 total_rounds

`StateMachine::total_rounds()` `Some(6)` dondurur.
`sign.rs:427-429`

---

## 4. Mesaj Tipleri

### 4.1 Veri Yapilari (rounds.rs:31-49)

```rust
pub struct GWI(pub Point<Secp256k1>);       // g_w_i (public signing key share)
pub struct GammaI(pub MessageB);             // MtA gamma bob mesaji
pub struct WI(pub MessageB);                 // MtA w bob mesaji
pub struct DeltaI(Scalar<Secp256k1>);        // delta_i = k_i*gamma_i + MtA toplamlar
pub struct TI(pub Point<Secp256k1>);         // T_i = G*sigma_i + H*l_i (commitment)
pub struct TIProof(pub PedersenProof<...>);  // T_i'nin Pedersen ZK kaniti
pub struct RDash(Point<Secp256k1>);          // R_dash_i = R * k_i
pub struct SI(pub Point<Secp256k1>);         // S_i = R * sigma_i
pub struct HEGProof(pub HomoELGamalProof);   // S_i ile T_i tutarlilik kaniti
pub struct PartialSignature(Scalar<Secp256k1>); // s_i = m*k_i + r*sigma_i
```

### 4.2 OfflineProtocolMessage

```rust
pub struct OfflineProtocolMessage(OfflineM);
enum OfflineM {
    M1((MessageA, SignBroadcastPhase1)),
    M2((GammaI, WI)),
    M3((DeltaI, TI, TIProof)),
    M4(SignDecommitPhase1),
    M5((RDash, Vec<PDLwSlackProof>)),
    M6((SI, HEGProof)),
}
```
`sign.rs:478-490`

### 4.3 Hata Tipleri (rounds.rs:696-713)

```rust
pub enum Error {
    Round1(ErrorType),           // MtA Alice-side kanit uretim hatasi
    Round2Stage3(crate::Error),  // alpha_ij hesaplama hatasi
    Round2Stage4(ErrorType),     // g_w_vec tutarsizligi
    Round3(ErrorType),           // Pedersen proof dogrulama hatasi
    Round5(ErrorType),           // PDL with slack proof hatasi VEYA R hesabi
    Round6VerifyProof(ErrorType), // HomoELGamal proof hatasi
    Round6CheckSig(crate::Error), // S_i toplami != pubkey
    Round7(crate::Error),         // output_signature dogrulama hatasi
}
```

---

## 5. Example Implementasyon

### 5.1 sm_manager — Koordinator Sunucu (gg20_sm_manager.rs)

**Framework:** Rocket (Rust async HTTP framework)
**Depolama:** In-memory `HashMap<String, Arc<Room>>`

**API Endpoint'leri:**
```
GET  /rooms/{room_id}/subscribe      — SSE stream (Server-Sent Events)
POST /rooms/{room_id}/issue_unique_idx — Party index alimi (atomik u16 sayac)
POST /rooms/{room_id}/broadcast      — Mesaj yayinlama
```

**Room yapisi** (`gg20_sm_manager.rs:57-62`):
```rust
struct Room {
    messages: RwLock<Vec<String>>,
    message_appeared: Notify,
    subscribers: AtomicU16,
    next_idx: AtomicU16,     // 1'den baslar
}
```

**Onemli:** `next_idx` 1'den baslar (`AtomicU16::new(1)`, `gg20_sm_manager.rs:101`). Mesajlar bellekte tutulur; `is_abandoned()` kontrolu ile eski odalar temizlenir.

**Mesaj limiti:** 100 MB (`gg20_sm_manager.rs:185`)

### 5.2 sm_client — HTTP Istemci (gg20_sm_client.rs)

**SmClient yapisi** (`gg20_sm_client.rs:55-57`):
```rust
pub struct SmClient {
    http_client: surf::Client,  // async HTTP (surf crate)
}
```

**join_computation() akisi** (`gg20_sm_client.rs:10-53`):
1. SSE subscribe akisi acilir
2. `issue_unique_idx` ile parti index alinir
3. Kendi mesajlarini filtrele (`sender != index`)
4. Outgoing: `broadcast()` cagrisin sink'i

**Mesaj formatı:** JSON seri hale getirilmis `Msg<M>` struct.

**Filtreleme** (`gg20_sm_client.rs:36-39`): `msg.sender != index && (msg.receiver.is_none() || msg.receiver == Some(index))`

### 5.3 Keygen Example (gg20_keygen.rs)

```
CLI parametreler: --index, --threshold, --number-of-parties, --output, --address, --room
```

`Keygen::new(index, threshold, n_parties)` ile state machine olusturulur, `AsyncProtocol::new().run().await` ile calistirilir, cikti JSON dosyasina yazilir.

### 5.4 Signing Example (gg20_signing.rs)

**Akis** (`gg20_signing.rs:34-89`):
1. Lokal key JSON dosyasindan okunur
2. `{room}-offline` odasina katilim, `OfflineStage::new(i, parties, local_share)` ile offline stage calistirilir
3. `{room}-online` odasina katilim
4. `SignManual::new(message, completed_offline_stage)` ile partial signature uretilir
5. Partial signature broadcast edilir
6. Diger partilerden `n-1` partial signature beklenir: `incoming.take(number_of_parties - 1)`
7. `signing.complete(&partial_signatures)` ile final imza olusturulur, JSON olarak stdout'a yazilir

**Not:** Mesaj formatlamasi `BigInt::from_bytes(data.as_bytes())` — bu hash degil raw bytes! Production'da mesaj oncelikle hashlenmelidir.

---

## 6. Guvenlik Degerlendirmesi

### 6.1 Kriptografik Kanitlar Ozeti

| Round | Kanit Tipi | Amac |
|-------|-----------|------|
| R0 (MessageA) | AliceProof (range proof) | k_i'nin kucuk oldugunu kanitla (`k_i < q`) |
| R1 (MessageB) | DLogProof(b), DLogProof(beta_tag) | MtA'da gamma_i/w_i bilgisini kanitla |
| R2 | verify_proofs_get_alpha + g_alpha check | MtA sonucunun tutarliligini dogrula |
| R2 | PedersenProof(sigma_i, l_i) | T_i = G*sigma_i + H*l_i dogrula |
| R3 | Commitment open (Hash check) | g_gamma_i commitment acilisini dogrula |
| R4 | PDLwSlackProof | R_dash_i = R * k_i ve k_i Enc ile tutarli |
| R5 | phase5_check_R_dash_sum | sum(R_dash_i) == G kontrolu |
| R5 | HomoELGamalProof | S_i = R * sigma_i ve T_i tutarli |
| R6 | phase6_check_S_i_sum | sum(S_i) == pubkey Y kontrolu |
| R7 | verify(sig, pubkey, msg) | Son ECDSA dogrulama |

### 6.2 Paillier Guvenligi

**SECURITY sabiti:** 256-bit (`party_i.rs:48`)
**Paillier key boyutu:** 2047-2048 bit kontrol edilir (`party_i.rs:287-290`)
**Uyari — Safe Prime:** `Keys::create()` normal prime kullanir, `create_safe_prime()` güvenli prime kullanir.
`party_i.rs:180-199`: "we recommend using safe primes if the code is used in production"
**Mevcut testler ve ornekler** normal (guvenssiz) prime kullanir!

### 6.3 Tanimlanan Hatalar ve Blame Mekanizmasi

`ErrorType` yapisi `bad_actors: Vec<usize>` icerir. Ancak incelendiginde:
- `Round1::proceed()` de `bad_actors: vec![]` — kotü aktör tespiti YOK (`rounds.rs:158-162`)
- `phase4()` de bad actor tespiti mevcut (`party_i.rs:649-667`)
- `phase5_verify_pdl()` de `bad_actors_vec.push(i)` ancak `i` kendi indeksi (`party_i.rs:748-749`) — bu yanlis, `j` olmali

### 6.4 Missing: Timeout

`round_timeout()` `None` dondurur, `round_timeout_reached()` `panic!` cagririr.
Duran parti (liveness failure) protokolu sonsuz beklemeye sokar.

### 6.5 CompletedOfflineStage Tek Kullanimlik

Her offline stage yalnizca bir kez kullanilmalidir cunku `sigma_i` ve `k_i` nonce sifir bilgisi ihlali olusturur. Ayni `CompletedOfflineStage` ile iki farkli mesaj imzalanirsa: `s1 = m1*k + r*k*x`, `s2 = m2*k + r*k*x`, bu iki denklemi cozersek `x` açiga cikar. Implement'in `Clone` turetemesi bu riski dogurur.

---

## 7. Nova Shield Chain Icin Cikarsimlar

### 7.1 Mimari Kararlari

**Kullanilabilir:** GG20 signing protokolunun tamamlanmis implementasyonu `cggmp21` crate'inden farkli olarak daha dusmanca protokol kanitleri icerir (PDL with slack, HomoELGamal). Ancak arastirma amaclidir; production icin:

- `Keys::create_safe_prime()` kullanilmali
- Timeout mekanizmasi eklenmeli
- `CompletedOfflineStage`'in yeniden kullanim korunmasi implement edilmeli

### 7.2 Kritik Matematiksel Noktalar

| Konsept | Deger/Formul | Kaynak |
|---------|-------------|--------|
| w_i = Lagrange(x_i) | `li * x_i` — VSS Lagrange | `party_i.rs:553-558` |
| k_i, gamma_i | `Scalar::random()` — CSP RNG | `party_i.rs:561-563` |
| delta_i | `k_i*gamma_i + sum(alpha_ij + beta_ij)` | `party_i.rs:591-604` |
| sigma_i | `k_i*w_i + sum(miu_ij + ni_ij)` | `party_i.rs:606-617` |
| T_i | `G*sigma_i + H*l_i` (Pedersen) | `party_i.rs:627-630` |
| R | `Gamma * delta_inv = G * k^-1` | `party_i.rs:679-685` |
| R_dash_i | `R * k_i` | `rounds.rs:452` |
| S_i | `R * sigma_i` | `party_i.rs:784` |
| s_i | `m*k_i + r*sigma_i` | `party_i.rs:863` |
| s | `sum(s_i) = k*(m + r*x)` | `party_i.rs:874` |
| r | `R.x_coord() mod q` | `party_i.rs:858-861` |
| Low-S | `if s > q-s: s = q-s` | `party_i.rs:898-901` |

### 7.3 MtA Protokolu Ozeti

```
Alice side:
  MessageA: c_A = Enc(k_i; r_A)  + AliceProof(range)

Bob side (for gamma_j):
  MessageB: c_B = Paillier_Add(
               Paillier_Mul(c_A, gamma_j),
               Enc(beta_tag; r_B)
             )
             = Enc(k_i * gamma_j + beta_tag)
  beta_ij = -beta_tag

Alice decrypts:
  alpha_ij = Dec(c_B) = k_i * gamma_j + beta_tag = k_i * gamma_j - beta_ij

Relationship:
  alpha_ij + beta_ij = k_i * gamma_j  (mod q)
```

### 7.4 Network Protokolu

- **Koordinator:** HTTP sunucu (Rocket), SSE ile yayinlama
- **Istemci:** HTTP (surf), SSE subscribe
- **Mesaj formati:** JSON `{"sender": u16, "receiver": Option<u16>, "body": ...}`
- **Odalar:** Her computation icin ayri room ID (`{room}-offline`, `{room}-online`)
- **P2P simulasyonu:** Broadcast uzerinden filtre (`receiver == Some(index)`)
- **Indeks atama:** Sunucu `issue_unique_idx` ile atomik u16 sayac verir (1'den baslar)

### 7.5 Test Parametreleri

```
simulate_offline_stage_t1_n2_s2  -> t=1, n=2, s=[1,2]
simulate_offline_stage_t1_n3_s2  -> t=1, n=3, s=[1,3]  (2 parti imzalar, 3 keygen)
simulate_offline_stage_t2_n3_s3  -> t=2, n=3, s=[1,2,3]
```
`sign.rs:722-763`

Minimum imzaci sayisi = `t + 1` (VSS esigi + 1). Yukaridaki t=1 testlerinde 2 parti yeterli.

---

## Ek: Toplam Round/Mesaj Kompleksitesi

| Round | Mesaj Tipi | Mesaj Sayisi | Paylasilan Veri |
|-------|-----------|-------------|-----------------|
| R0 | Broadcast | n | MessageA (Paillier ciphertext + range proofs) + commitment |
| R1 | P2P | n*(n-1) | 2 x MessageB (gamma ve w icin) |
| R2 | Broadcast | n | DeltaI + TI + TIProof |
| R3 | Broadcast | n | SignDecommitPhase1 |
| R4 | Broadcast | n | RDash + n-1 adet PDLwSlackProof |
| R5 | Broadcast | n | SI + HEGProof |
| **Online** | Broadcast | n | PartialSignature (tek scalar) |

**Toplam offline:** 6 round, `n + n*(n-1) + 4*n = n*(n+4)` mesaj
**Toplam online:** 1 round, `n` mesaj

Bu GG20'nin temel iddiasi: "one-round online signing" (`sign.rs:2-6`).
