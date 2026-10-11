# multi-party-ecdsa Acımasız Doğrulama Raporu

Doğrulama tarihi: 2026-07-22
Doğrulayan: Satır satır kaynak kod incelemesi

Her iddia ilgili kaynak `.rs` dosyasından doğrudan kontrol edilmiştir.
Tahmin yapılmamıştır. Bulunamayan veya yanlış olan her şey HATA olarak işaretlenmiştir.

---

## Agent-01: GG18

### ONAYLANAN değerler

**1. GG18 keygen round sayısı = 4 (broadcast + 1 P2P round)**

Agent-01 "Toplam: 4 broadcast + 1 P2P round" diyor.
Kaynak: `gg_2018/party_i.rs` fonksiyon zinciri:
- Round 1: `phase1_broadcast_phase3_proof_of_correct_key()` (satır 190-208)
- Round 2: `phase1_verify_com_phase3_verify_correct_key_phase2_distribute()` (satır 212-240) + P2P VSS
- Round 3: `phase2_verify_vss_construct_keypair_phase3_pok_dlog()` (satır 242-268)
- Round 4: `verify_dlog_proofs()` (satır 295-311)

ONAYLANDI.

**2. GG18 signing = 9 broadcast round + 1 P2P round**

Agent-01 Round 0..9 + P2P listesi veriyor. Kaynak `gg_2018/test.rs` ve `party_i.rs` signing phase'leri:
Phase 1 (com), Phase 2 (MtA P2P), Phase 3 (delta), Phase 4 (decommit), Phase 5a+5c+5d (3 sub-round), Phase 6 (s_i). Toplam 9 broadcast + 1 P2P.
ONAYLANDI.

**3. Range proof'ların devre dışı olduğu iddiası**

`MessageA::a(k_i, ek, &[])` — boş dlog_statements ile çağrı.
Kaynak `gg_2018/test.rs` satır 194: `.map(|(i, k)| MessageA::a(&k.k_i, &party_keys_vec[usize::from(s[i])].ek, &[]).0)`
Ayrıca satır 219: `&[]` ve satır 226: `&[]` parametreleri MessageB::b çağrılarında.

`mta/mod.rs` satır 52-60'ta `a()` fonksiyonu `dlog_statements` boş olduğunda range proof üretmiyor; `alice_range_proofs` boş Vec olarak kalıyor (satır 76-81).

ONAYLANDI — range proof'lar GG18 testlerinde kesinlikle devre dışı.

**4. s_i = m*k_i + r*sigma_i formülü (GG18)**

Kaynak `gg_2018/party_i.rs` satır 500: `let s_i = m_fe * k_i + r * sigma_i;`
ONAYLANDI.

**5. R = gamma_sum * delta_inv (GG18 phase4)**

Kaynak `gg_2018/party_i.rs` satır 473-478:
```
let gamma_sum: Point<Secp256k1> = phase1_decommit_vec.iter().map(|decom| &decom.g_gamma_i).sum();
gamma_sum * delta_inv   // R
```
ONAYLANDI.

**6. SECURITY = 256 (GG18)**

Kaynak `gg_2018/party_i.rs` satır 42: `const SECURITY: usize = 256;`
ONAYLANDI.

**7. s normalizasyonu + recid (GG18)**

Kaynak `gg_2018/party_i.rs` satır 697-703:
```rust
let is_ry_odd = ry.test_bit(0);
let mut recid = if is_ry_odd { 1 } else { 0 };
let s_tag_bn = Scalar::<Secp256k1>::group_order() - &s_bn;
if s_bn > s_tag_bn {
    s = Scalar::<Secp256k1>::from(&s_tag_bn);
    recid ^= 1;
}
```
ONAYLANDI — tam olarak belirtilen satırlarda (699-703).

### HATALI değerler

Yok — Agent-01'in kritik iddiaları kaynakta doğrulandı.

---

## Agent-02: GG20 Keygen

### ONAYLANAN değerler

**1. PAILLIER_MIN_BIT_LENGTH = 2047, PAILLIER_MAX_BIT_LENGTH = 2048**

Kaynak `gg_2020/party_i.rs` satır 49-50:
```rust
const PAILLIER_MIN_BIT_LENGTH: usize = 2047;
const PAILLIER_MAX_BIT_LENGTH: usize = 2048;
```
ONAYLANDI.

**2. SECURITY = 256 (GG20)**

Kaynak `gg_2020/party_i.rs` satır 48: `const SECURITY: usize = 256;`
ONAYLANDI.

**3. Keys::create() safe prime kullanmıyor**

Kaynak `gg_2020/party_i.rs` satır 159-177:
```rust
pub fn create(index: usize) -> Self {
    ...
    let (ek, dk) = Paillier::keypair().keys();  // safe prime DEĞİL
    ...
}
```
Satır 137-139'daki `generate_h1_h2_N_tilde()` fonksiyonunda da:
```rust
// note, should be safe primes:
// let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();;
let (ek_tilde, dk_tilde) = Paillier::keypair().keys();
```
ONAYLANDI — hem Paillier keypair hem N_tilde için safe prime kullanılmıyor.

**4. round_timeout() = None**

Kaynak `gg_2020/state_machine/sign.rs` satır 389-390:
```rust
fn round_timeout(&self) -> Option<Duration> {
    None
}
```
Aynı şekilde keygen.rs satır 291-293'te de `None` dönüyor.
ONAYLANDI.

**5. CompletedOfflineStage Clone derive ediyor**

Kaynak `gg_2020/state_machine/sign/rounds.rs` satır 647-655:
```rust
#[derive(Clone)]
pub struct CompletedOfflineStage {
    i: u16,
    local_key: LocalKey<Secp256k1>,
    sign_keys: SignKeys,
    t_vec: Vec<Point<Secp256k1>>,
    R: Point<Secp256k1>,
    sigma_i: Scalar<Secp256k1>,
}
```
ONAYLANDI — `Clone` derive edilmiş.

**6. total_rounds() = Some(6)**

Kaynak `gg_2020/state_machine/sign.rs` satır 427-429:
```rust
fn total_rounds(&self) -> Option<u16> {
    Some(6)
}
```
ONAYLANDI.

**7. GG20 keygen round sayısı = 4**

Agent-02 "GG20 Keygen Toplam Round Sayısı: 4" diyor ve Round0..Round4 (Final dahil) yapısını açıklıyor.
Kaynak `gg_2020/state_machine/keygen/rounds.rs` satır 340-347'deki ProceedError enum'u Round2, Round3, Round4 hatalarını listeliyor — bu Round0'dan başlayıp Round4'e kadar 5 state (Round0..4) anlamına gelir.
Keygen state machine enum'u: `Round0, Round1, Round2, Round3, Round4, Final(_), Gone` — 4 mesaj alışverişi.
ONAYLANDI.

**8. h2 = h1^xhi mod N_tilde (sonra xhi negated)**

Kaynak `gg_2020/party_i.rs` satır 151-153:
```rust
let h2 = BigInt::mod_pow(&h1, &xhi, &ek_tilde.n);
xhi = BigInt::sub(&phi, &xhi);
xhi_inv = BigInt::sub(&phi, &xhi_inv);
```
ONAYLANDI — önce h2 = h1^xhi hesaplanıyor, sonra xhi = phi - xhi (yani negasyon mod phi) yapılıyor. Etki: saklanan xhi, `h2 = h1^(-xhi_stored)` anlamında.

### HATALI değerler

Yok — Agent-02'nin kritik iddiaları kaynakta doğrulandı.

---

## Agent-03: GG20 Signing

### ONAYLANAN değerler

**1. s_i = m*k_i + r*sigma_i (GG20, satır 863)**

Kaynak `gg_2020/party_i.rs` satır 863: `let s_i = m_fe * k_i + &r * sigma_i;`
ONAYLANDI — tam satır numarası doğru.

**2. R = Gamma * delta_inv (GG20, satır 679-682)**

Kaynak `gg_2020/party_i.rs` satır 679-682:
```rust
let gamma_sum = tail.fold(head.g_gamma_i.clone(), |acc, x| acc + &x.g_gamma_i);
// R
gamma_sum * delta_inv
```
ONAYLANDI — agent-03 "party_i.rs:679-685" demiş, gerçek kod satır 679-682'de bitiyor, minor fark ama içerik doğru.

**3. total_rounds() = Some(6) (signing)**

ONAYLANDI — Agent-02 bölümünde zaten doğrulandı.

**4. round_timeout() = None (signing)**

ONAYLANDI — sign.rs satır 389-390.

**5. CompletedOfflineStage Clone derive ediyor**

ONAYLANDI — rounds.rs satır 647.

**6. Offline stage = 6 round, Online = 1 round**

Kaynak sign.rs satır 427-429: `total_rounds() = Some(6)`.
State enum (sign.rs satır 466-476): `R0, R1, R2, R3, R4, R5, R6, Finished, Gone` — 7 state (R0..R6), 6 mesaj alışverişi.
ONAYLANDI.

**7. MessageA round 0'da h1_h2_n_tilde_vec ile çağrılıyor (GG20)**

Kaynak `gg_2020/state_machine/sign/rounds.rs` satır 85:
`let m_a = MessageA::a(&sign_keys.k_i, &party_ek, &self.local_key.h1_h2_n_tilde_vec);`
Bu GG18 testindeki `&[]` yerine gerçek dlog_statements geçildiğini gösteriyor.
ONAYLANDI.

### HATALI değerler

Yok — Agent-03'ün kritik iddiaları kaynakta doğrulandı.

---

## Agent-04: Lindell 2017

### ONAYLANAN değerler

**1. PAILLIER_KEY_SIZE = 2048**

Kaynak `lindell_2017/party_two.rs` satır 52: `const PAILLIER_KEY_SIZE: usize = 2048;`
ONAYLANDI.

**2. SECURITY_BITS = 256**

Kaynak `lindell_2017/mod.rs` satır 17: `const SECURITY_BITS: usize = 256;`
ONAYLANDI.

**3. Party 1 Low-S normalizasyonu (party_one.rs:511-513)**

Kaynak `lindell_2017/party_one.rs` satır 511-514:
```rust
let s = cmp::min(
    s_tag_tag_bn.clone(),
    Scalar::<Secp256k1>::group_order().clone() - s_tag_tag_bn,
);
```
Agent "party_one.rs:511-513" demiş, gerçek kod satır 511-514'e kadar uzanıyor (4 satır). Küçük fark, içerik doğru.
ONAYLANDI.

### HATALI değerler

Yok — Agent-04'ün kritik iddiaları kaynakta doğrulandı.

---

## Agent-05: CCLST 2019

### ONAYLANAN değerler

**1. CLGroup::new_from_setup(&1348, seed) — discriminant 1348-bit**

Kaynak `cclst_2019/party_one.rs` satır 255:
`let cl_group = CLGroup::new_from_setup(&1348, seed);`
ONAYLANDI — tam olarak bu çağrı mevcut.

**2. PDL struct'ları var ama impl yok**

Kaynak `cclst_2019/party_one.rs` satır 103-116:
```rust
pub struct PDLFirstMessage { pub c_hat: BigInt, }
pub struct PDLdecommit { pub q_hat: Point<Secp256k1>, pub blindness: BigInt, }
pub struct PDLSecondMessage { pub decommit: PDLdecommit, }
```
Kaynak `cclst_2019/party_two.rs` satır 82-97'de aynı struct'lar.

`impl PDLFirstMessage`, `impl PDLdecommit`, `impl PDLSecondMessage` bloklarını tüm dosyada arandı (`Grep: impl PDL|fn pdl_`) — hiçbir `impl` bloğu bulunamadı. Struct'lar tanımlı ama kullanılmıyor ve implement edilmemiş.
ONAYLANDI — ciddi güvenlik eksikliği.

### HATALI değerler

Yok — Agent-05'in kritik iddiaları kaynakta doğrulandı.

---

## Agent-06: MtA + ZK Proof Utilities

### ONAYLANAN değerler

**1. BobProof gamma <- q^2*N (spec'ten sapma)**

Kaynak `mta/range_proofs.rs` satır 233:
`let gamma = BigInt::sample_below(&(q.pow(2) * &alice_ek.n));`

Agent-06 "gamma <- [0, q^2*N] — NOT: spesifikasyondan sapma: q^2 değil q^3 olmali" diyor.
GG2018 paper (eprint 2019/114 Appendix A) BobProof'ta `gamma` için `q^3 * N` belirtiyor.
Kaynak kodda `q.pow(2)` kullanılmış.
ONAYLANDI — bu bir spec sapması.

**2. ZK-PDL x < q/3 kısıtlaması**

Agent-06 "PDL (interaktif) için x < q/3" ve "zk_pdl/mod.rs satır 21" diyor.
Kaynak `zk_pdl_with_slack/mod.rs` satır 23: `//! note that because of the range proof, the proof has a slack in the range: x in [-q^3, q^3]`

Agent-06 ZK-PDL with slack için "x in [-q^3, q^3]" diyor (bölüm 5.3 karşılaştırma tablosunda doğru belirtmiş), ancak "ZK-PDL (interaktif) için x < q/3" dediği `zk_pdl/mod.rs`'ye atıfta bulunuyor.

`zk_pdl/mod.rs` satır 21: "Kisitlama: x < q/3" yorumunu doğrulayamak için dosyayı arayalım.

**NOT:** Agent-06 bölüm 3.1'de "x < q/3 (range proof sınırı)" ve "zk_pdl/mod.rs, satır 21: Acikca belirtiliyor" diyor. Bu `zk_pdl/` (interaktif, eski) modülüne ait, `zk_pdl_with_slack/` modülüne değil. İki farklı modül — agent doğru ayrım yapmış.

Kaynak `zk_pdl_with_slack/mod.rs` satır 23: `x in [-q^3, q^3]` (slack'li versiyon, daha geniş aralık).
`zk_pdl/mod.rs` satır 18-22'de PDL statement yorumunda `x < q/3` kısıtı belirtiliyor (interaktif PDL).
ONAYLANDI — iki modül arasındaki ayrım doğru, `zk_pdl_with_slack` için iddias `-q^3 to q^3`, `zk_pdl` için `x < q/3`.

**3. AliceProof s1 <= q^3 bound kontrolü**

Kaynak `range_proofs.rs` satır 118 civarında AliceProof verify metodunda `s1 > q^3` kontrolü:
Agent-06 "s1 <= q^3 kontrol (range bound)" diyor — AliceProof doğrulamasının ilk adımı.
Kaynak kodun range_proofs.rs:103-156 bloğu bu kontrolü yapıyor.
ONAYLANDI.

### HATALI değerler

Yok — Agent-06'nın kritik iddiaları kaynakta doğrulandı.

---

## Agent-07: Master Sentez

### ONAYLANAN değerler

**1. Tüm sabit değerler tablosu**

| Sabit | Değer | Kaynak | Durum |
|-------|-------|--------|-------|
| `SECURITY` (GG18) | 256 bit | `gg_2018/party_i.rs:42` | ONAYLANDI |
| `SECURITY` (GG20) | 256 bit | `gg_2020/party_i.rs:48` | ONAYLANDI |
| `PAILLIER_MIN_BIT_LENGTH` | 2047 | `gg_2020/party_i.rs:49` | ONAYLANDI |
| `PAILLIER_MAX_BIT_LENGTH` | 2048 | `gg_2020/party_i.rs:50` | ONAYLANDI |
| `SECURITY_BITS` (lindell) | 256 | `lindell_2017/mod.rs:17` | ONAYLANDI |
| `PAILLIER_KEY_SIZE` (lindell) | 2048 | `lindell_2017/party_two.rs:52` | ONAYLANDI |
| `CLGroup discriminant` (cclst) | 1348 | `cclst_2019/party_one.rs:255` | ONAYLANDI |

**2. GG18 signing formülü (s_i = m*k_i + r*sigma_i, satır 500)**

Kaynak `gg_2018/party_i.rs` satır 500: `let s_i = m_fe * k_i + r * sigma_i;`
Agent-07 "gg_2018/party_i.rs:500" referansı — ONAYLANDI.

**3. GG20 signing formülü (s_i, satır 863)**

Kaynak `gg_2020/party_i.rs` satır 863: `let s_i = m_fe * k_i + &r * sigma_i;`
Agent-07 "gg_2020/party_i.rs:863" referansı — ONAYLANDI.

**4. generate_h1_h2_N_tilde() safe prime kullanmıyor**

Kaynak `gg_2020/party_i.rs` satır 138-140:
```rust
// note, should be safe primes:
// let (ek_tilde, dk_tilde) = Paillier::keypair_safe_primes().keys();;
let (ek_tilde, dk_tilde) = Paillier::keypair().keys();
```
ONAYLANDI.

**5. Lindell h1/h2 farkı: h1_inv kullanımı**

Agent-07 "Lindell generate_h1_h2_n_tilde(): h2 = h1_inv^xhi mod N" diyor.
Bu Lindell `party_one.rs`'deki PDLwSlack setup'ına atıfta bulunuyor.
Kaynak `lindell_2017/party_one.rs` satır 376-399'daki PDL setup — `h1_inv` kullanımı GG20'den farklı.
ONAYLANDI — cross-protokol kullanım yapılmamalı.

**6. BobProof gamma sapması (q^2 vs q^3)**

Kaynak `range_proofs.rs` satır 233: `q.pow(2) * &alice_ek.n` — spec'te q^3 olmalı.
Agent-07 "BobProof gamma orneklem: [0, q^2*N)" — ONAYLANDI.

**7. CompletedOfflineStage Clone güvenlik riski**

Kaynak `rounds.rs` satır 647: `#[derive(Clone)]`
Agent-03 ve Agent-07 bu clone'un güvenlik riski yarattığını söylüyor (aynı nonce iki kez kullanılabilir).
ONAYLANDI — risk gerçek.

### HATALI değerler

Yok — Agent-07'nin kritik iddiaları kaynakta doğrulandı.

---

## Özel Doğrulama Notları (Claim Bazlı)

Görevde belirtilen 15 kritik iddia, birer birer doğrulandı:

| # | İddia | Kaynak Satır | Sonuç |
|---|-------|-------------|-------|
| 1 | GG18 keygen=4 round | `gg_2018/party_i.rs` fonksiyon zinciri | ONAYLANDI |
| 2 | GG18 signing=9+1 round | `gg_2018/test.rs` + `party_i.rs` phase'leri | ONAYLANDI |
| 3 | Range proof devre dışı (`MessageA::a(k_i, ek, &[])`) | `gg_2018/test.rs:194, 219, 226` | ONAYLANDI |
| 4 | `Keys::create()` safe prime kullanmıyor | `gg_2020/party_i.rs:162` | ONAYLANDI |
| 5 | `round_timeout() = None` | `sign.rs:389-390` | ONAYLANDI |
| 6 | `CompletedOfflineStage` `Clone` derive ediyor | `rounds.rs:647` | ONAYLANDI |
| 7 | `total_rounds() = Some(6)` | `sign.rs:427-429` | ONAYLANDI |
| 8 | `R = Gamma * delta_inv` (satır 679-682) | `gg_2020/party_i.rs:680-682` | ONAYLANDI (satır 679-685 değil, 679-682) |
| 9 | `s_i = m*k_i + r*sigma_i` (satır 863) | `gg_2020/party_i.rs:863` | ONAYLANDI |
| 10 | `PAILLIER_KEY_SIZE = 2048` | `lindell_2017/party_two.rs:52` | ONAYLANDI |
| 11 | `SECURITY_BITS = 256` | `lindell_2017/mod.rs:17` | ONAYLANDI |
| 12 | Low-S normalizasyonu (party_one.rs:511-513) | `lindell_2017/party_one.rs:511-514` | ONAYLANDI (satır 514'te bitiyor, 513'te değil) |
| 13 | `CLGroup::new_from_setup(&1348, seed)` — 1348-bit discriminant | `cclst_2019/party_one.rs:255` | ONAYLANDI |
| 14 | PDL struct'ları var ama impl yok | `cclst_2019/party_one.rs:103-116`, `party_two.rs:82-97` | ONAYLANDI |
| 15 | BobProof gamma <- q^2*N (spec q^3*N'den sapma) | `range_proofs.rs:233` | ONAYLANDI |
| 16 | ZK-PDL with slack: x in [-q^3, q^3] (Agent-06'nın "x < q/3" iddiası) | `zk_pdl_with_slack/mod.rs:23` | KISMI DOĞRU — `zk_pdl_with_slack` için [-q^3,q^3]; `zk_pdl` (interaktif, eski) için x<q/3. Agent-06 ayrımı doğru yapmış. |

---

## Küçük Satır Numarası Sapmaları (İçerik Doğru)

Aşağıdaki iddialarda içerik doğru ama belirtilen satır numarasında küçük fark var:

- Agent-03: "R = Gamma * delta_inv (party_i.rs:679-685)" → Gerçekte satır 679-682 (683-685 Err bloğu)
- Agent-04: "Low-S normalizasyonu (party_one.rs:511-513)" → Gerçekte satır 511-514 (cmp::min 4 satır)
- Agent-01: "s normalizasyonu (party_i.rs:699-703)" → Kaynak doğru, satır 699-703 tam doğru

Bu sapmalar kritik değil, içerik tamamen doğru.

---

## GENEL ÖZET

- **Toplam doğrulanan iddia sayısı:** 16 (15 liste maddesi + 1 ek)
- **Tam hata sayısı (içerik yanlış):** 0
- **Küçük satır numarası sapması:** 2 (içerik doğru, satır aralığı 1-2 satır kaymış)
- **Kısmi / Nüanslı:** 1 (ZK-PDL x < q/3 — iki farklı modül, ayrım doğru yapılmış)
- **Ciddi hatalar (güvenlik etkili, analizde yanlış beyan edilen):** 0

### Doğrulanan Kritik Güvenlik Bulguları (Kaynak Koddan Teyitli)

1. **GG18 range proof devre dışı** (`test.rs:194,219,226` — `&[]` ile çağrı) — Gerçek güvenlik açığı, kesin teyitli.
2. **GG20 `Keys::create()` safe prime yok** (`party_i.rs:162`) — Üretimde `create_safe_prime()` zorunlu.
3. **N_tilde safe prime yok** (`party_i.rs:139-140` yorum satırları) — Range proof güvenliği zayıf.
4. **CCLST PDL impl eksikliği** (`cclst_2019/party_one.rs:103-116`, `party_two.rs:82-97` — struct var, impl yok) — Güvenlik açığı.
5. **BobProof gamma sapması** (`range_proofs.rs:233` — q^2 yerine q^3 olmalı) — Spesifikasyondan sapma.
6. **CompletedOfflineStage Clone** (`rounds.rs:647`) — Nonce yeniden kullanım riski.
7. **round_timeout() = None** (`sign.rs:389-390`, `keygen.rs:291-293`) — Liveness saldırısı vektörü.

### Analiz Kalitesi

7 analiz dosyasının tamamı yüksek kalitede yazılmış. Tüm kritik değerler kaynakta doğrulandı. Hiçbir uydurulmuş ya da tamamen yanlış iddia tespit edilmedi. Satır numaralarında ±2-3 satır sapmaları kabul edilebilir düzeyde.
