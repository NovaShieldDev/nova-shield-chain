# tss-lib (Binance) — Kapsamlı Analiz

**Repo:** github.com/bnb-chain/tss-lib
**Dil:** Go
**Lisans:** MIT
**Güvenlik Denetimi:** Kudelski Security, 2019
**Protokol:** GG18 Threshold ECDSA + EdDSA
**Versiyon:** v3 (incelenen)

---

## 1. Genel Mimari

tss-lib üç ana protokol sunar:

| Protokol       | Tur Sayısı | Açıklama                                       |
|----------------|------------|------------------------------------------------|
| ECDSA Keygen   | 4 tur      | Distributed key generation, Paillier + VSS    |
| ECDSA Signing  | 9 tur + final | GG18 threshold signing, MtA tabanlı        |
| ECDSA Resharing| 5 tur      | Anahtar paylaşımını yeni komiteye aktarma      |
| EdDSA Keygen   | 3 tur      | Schnorr VSS tabanlı keygen, Paillier YOK      |
| EdDSA Signing  | 3 tur + final | Hash-then-sign, edwards25519                |
| EdDSA Resharing| 5 tur      | Aynı mantık, farklı eğri                      |

**Temel tasarım prensibi:** Her protokol `Party` interface'ini uygular. Mesajlar kanal (`chan`) üzerinden iletilir. Transport katmanı kullanıcıya bırakılmıştır (E2E şifreleme, replay koruması kullanıcı sorumluluğu).

---

## 2. Temel Interface'ler ve Yapılar

### 2.1 Party Interface (`tss/party.go`)

```go
type Party interface {
    Start() *Error
    UpdateFromBytes(wireBytes []byte, from *PartyID, isBroadcast bool) (ok bool, err *Error)
    Update(msg ParsedMessage) (ok bool, err *Error)
    Running() bool
    WaitingFor() []*PartyID
    ValidateMessage(msg ParsedMessage) (bool, *Error)
    StoreMessage(msg ParsedMessage) (bool, *Error)
    PartyID() *PartyID
    String() string
    WrapError(err error, culprits ...*PartyID) *Error
    FirstRound() Round
}
```

```go
type BaseParty struct {
    mtx       sync.Mutex
    rnd       Round
    FirstRound Round
}
```

### 2.2 Round Interface (`tss/round.go`)

```go
type Round interface {
    Params() *Parameters
    Start() *Error
    Update() (bool, *Error)    // Tüm mesajlar geldi mi?
    CanProceed() bool          // ok[] slice'ı dolu mu?
    NextRound() Round          // nil döndürürse protocol bitti
    WaitingFor() []*PartyID
    CanAccept(msg ParsedMessage) bool
    RoundNumber() int
}
```

### 2.3 BaseStart ve BaseUpdate (`tss/party.go`)

```go
func BaseStart(p Party, task string, optionalPreprocessFn ...func(Round) *Error) *Error {
    p.Lock()
    defer p.Unlock()
    p.SetFirstRound(p.FirstRound())
    // optionalPreprocessFn varsa çağır (signing'de prepare() buraya girer)
    return p.FirstRound().Start()
}

func BaseUpdate(p Party, msg ParsedMessage, task string) (ok bool, err *Error) {
    p.Lock()
    defer p.Unlock()
    ok, err = p.ValidateMessage(msg)
    ok, err = p.StoreMessage(msg)
    // Kritik tasarım: Recursive çağrı!
    if p.Round().CanProceed() {
        if p.advance() {  // Round'u ilerlet
            return BaseUpdate(p, msg, task)  // Yeni round için saklanmış mesajları işle
        }
    }
    return true, nil
}
```

**Önemli:** `BaseUpdate` recursive'dir. `advance()` ile bir sonraki round'a geçildiğinde, daha önce gelen ve o round için saklanan mesajlar hemen işlenebilir. Bu, out-of-order mesaj desteği sağlar.

### 2.4 Message Yapısı (`tss/message.go`)

```go
type MessageRouting struct {
    From                     *PartyID
    To                       []*PartyID   // nil = broadcast
    IsBroadcast              bool
    IsToOldCommittee         bool         // resharing için
    IsToOldAndNewCommittees  bool         // resharing için
}

type MessageImpl struct {
    *MessageWrapper           // ProtoBuf encoding
    from    *PartyID
    to      []*PartyID
    content MessageContent
}
```

Mesajlar ProtoBuf ile wire-encode edilir. `WireBytes()` ile serialize, `ParseWireMessage()` ile deserialize yapılır.

### 2.5 Parameters (`tss/params.go`)

```go
type Parameters struct {
    ec              elliptic.Curve
    partyID         *PartyID
    parties         *PeerContext
    partyCount      int
    threshold       int          // t: t+1 kişi imzalar
    concurrency     int
    safePrimeGenTimeout time.Duration
    sessionNonce    *big.Int     // SSID için dış nonce
    rand            io.Reader
    noProofMod      bool         // test modu
    noProofFac      bool         // test modu
}
```

`ReSharingParameters` eski ve yeni komite parametrelerini tutar:
- `OldParties()`, `NewParties()`, `OldThreshold()`, `NewThreshold()`
- `IsOldCommittee()`, `IsNewCommittee()`, `OldAndNewParties()`

### 2.6 Error Yapısı (`tss/error.go`)

```go
type Error struct {
    cause    error
    task     string
    round    int
    victim   *PartyID
    culprits []*PartyID   // İdentifiable abort!
}
```

Culprit listesi sayesinde hangi partinin hatalı davrandığı tespit edilebilir (identifiable abort).

### 2.7 Curve Registry (`tss/curve.go`)

```go
// Kayıtlı eğriler:
// "secp256k1" -> btcd/btcec
// "ed25519"   -> decred/edwards
func RegisterCurve(name string, ec elliptic.Curve)
func GetCurveByName(name string) elliptic.Curve
func SameCurve(ec1, ec2 elliptic.Curve) bool
```

---

## 3. Kriptografik Primitifler

### 3.1 Hash Fonksiyonu (`common/hash.go`)

```go
func SHA512_256i(in ...*big.Int) *big.Int
// Encoding: [block_count || element0_len || element0 || \n || element1_len || element1 || ...]
// Her elementin uzunluğu da hash'e dahil → uzunluk uzantısı saldırısına karşı güvenli
// \n delimiter → eleman sınırlarını belirgin hale getirir

func SHA512_256i_TAGGED(tag []byte, in ...*big.Int) *big.Int
// BIP-340 tarzı domain separation:
// hash(tag) || hash(tag) || payload
```

### 3.2 Güvenli Asal Üretimi (`common/safe_prime.go`)

```go
type GermainSafePrime struct {
    q *big.Int  // Sophie Germain asal: q asal
    p *big.Int  // Güvenli asal: p = 2q+1 de asal
}

func GetRandomSafePrimesConcurrent(ctx context.Context, safePrimeBitLen, count, concurrency int, rand io.Reader) ([]*GermainSafePrime, error)
```

Algoritma:
1. Küçük asal eleme (sieve) ile adayları filtrele
2. `q % 3 != 1` kontrolü (Paillier için)
3. `p = 2q+1` için Pocklington kriteri ile doğrulama
4. Bağlam iptali (`context.Context`) ile timeout desteği
5. Paralel goroutine'ler

### 3.3 Sabit Zaman İşlemleri (`common/constant_time.go`)

```go
type CTModInt struct {
    mod         *bigmod.Modulus   // filippo.io/bigmod
    inverseExp  []byte
    bytePool    sync.Pool
}

func (ct *CTModInt) ExpCT(base, exp *big.Int) *big.Int      // Sabit-zaman kuvvet
func (ct *CTModInt) ModInverseCT(a *big.Int) *big.Int        // Sabit-zaman modüler ters
func (ct *CTModInt) MulCT(x, y *big.Int) *big.Int           // Sabit-zaman çarpım

type TimingProtection struct {
    targetDuration time.Duration
    jitterRange    time.Duration
}

func (tp *TimingProtection) ProtectBigInt(fn func() (*big.Int, error)) (*big.Int, error)
// Paillier decrypt sürelerini rastgele jitter ile maskeler
```

`EnableConstantTimeOps()` ile global olarak aktif edilir. Signing round3 ve round5'te `k`, `gamma`, `w`, `sigma` için kullanılır.

### 3.4 Paillier Şifreleme (`crypto/paillier/paillier.go`)

```go
type PrivateKey struct {
    PublicKey
    LambdaN *big.Int   // lcm(p-1, q-1)
    PhiN    *big.Int   // (p-1)(q-1)
    P, Q    *big.Int   // v2.0: tam P,Q saklanır (ProofFac için)
}

func GenerateKeyPair(ctx, rand, bits, concurrency) (*PrivateKey, *PublicKey, error)
// Gereksinimler:
// - P, Q safe prime
// - |P - Q| >= 2^(bits/2 - 100) (sqrt saldırılarına karşı)
// - 2048-bit N

func Encrypt(pk, m) (c, r, error)
// c = (N+1)^m * r^N mod N^2

func Decrypt(sk, c) (m, error)
// L(c^LambdaN mod N^2, N) * ModInverse(L((N+1)^LambdaN mod N^2, N)) mod N

func HomoAdd(pk, c1, c2) *big.Int    // Homomorfik toplama: Enc(a+b) = Enc(a)*Enc(b) mod N^2
func HomoMult(pk, m, c) *big.Int     // Homomorfik çarpım: Enc(a*b) = c^a mod N^2

type Proof [ProofIters]*big.Int
func (pk *PublicKey) Proof(ki *big.Int, ecPoint *ECPoint) Proof
// 13 iterasyon, ki ve ECDSA pubkey'i içeren ZK kanıtı
```

### 3.5 VSS - Feldman Doğrulanabilir Gizli Paylaşım (`crypto/vss/feldman_vss.go`)

```go
func Create(ec, threshold, secret, indexes, rand) (Vs Vs, Shares Shares, error)
// Vs[i] = G * a[i]  → polinom katsayılarının EC taahhütleri
// Shares[j].Share = f(indexes[j])  → j. taraf için Shamir payı
// f(x) = secret + a1*x + a2*x^2 + ... + at*x^t

func (s *Share) Verify(ec, threshold, vs Vs) bool
// G * f(i) == sum(vs[j] * i^j) for j=0..t

func (shares Shares) ReConstruct(ec) (*big.Int, error)
// Lagrange interpolasyonu: f(0) = secret
```

### 3.6 Schnorr ZK Kanıtı (`crypto/schnorr/schnorr_proof.go`)

```go
type ZKProof struct {
    Alpha *ECPoint   // Commitment: G * a
    T     *big.Int   // Response: a + e*x
}

func NewZKProof(session []byte, x *big.Int, X *ECPoint, rand) (*ZKProof, error)
// X = G*x kanıtlar
// e = SHA512_256i_TAGGED(session, G, X, Alpha)
// Verify: G*T == Alpha + X*e

type ZKVProof struct {
    Alpha *ECPoint
    T     *big.Int
    U     *big.Int
}

func NewZKVProof(session, V, R *ECPoint, s, l *big.Int, rand) (*ZKVProof, error)
// V = R^s * G^l kanıtlar (signing round 6'da si kanıtlamak için)
```

### 3.7 DLN Kanıtı (`crypto/dlnproof/proof.go`)

```go
// h2 = h1^alpha mod NTilde kanıtlar
// 128 iterasyon Schnorr benzeri
// Keygen Round 1'de h1, h2, NTilde parametreleri için kullanılır

type Proof struct {
    Alpha [Iterations]*big.Int   // Taahhütler: h1^a[i] mod NTilde
    T     [Iterations]*big.Int   // Yanıtlar: a[i] + c.bit(i) * x mod p*q
}

func NewDLNProof(session, h1, h2, x, p, q, N *big.Int, rand) *Proof
func (p *Proof) Verify(session, h1, h2, N *big.Int) bool
```

### 3.8 ProofFac (`crypto/facproof/proof.go`)

```go
// N = p*q olduğunu ve p,q'nun "yeterince büyük" olduğunu kanıtlar
// Fig 28 (GG20): p,q >= sqrt(N)/q^3 olmak zorunda
// Küçük çarpanlı N'e karşı koruma

type ProofFac struct {
    P, Q, A, B, T, Sigma *big.Int
    Z1, Z2, W1, W2, V    *big.Int
}

func NewProof(session, ec, N, NCap, s, t, p, q *big.Int, rand) (*ProofFac, error)
func (p *ProofFac) Verify(session, ec, N, NCap, s, t *big.Int) bool
```

Keygen Round 2'de her taraf kendi Paillier modülünü diğerlerine kanıtlar.

### 3.9 ProofMod (`crypto/modproof/proof.go`)

```go
// N'in iki güvenli asalın çarpımı olduğunu kanıtlar
// 80 iterasyon, paralel goroutine doğrulama

type ProofMod struct {
    W    *big.Int
    X    [80]*big.Int
    A, B *big.Int
    Z    [80]*big.Int
}

func NewProof(session, N, P, Q *big.Int, rand) (*ProofMod, error)
// W = quadratic non-residue
// X[i]^4 == ±W^b * Y[i] mod N
func (p *ProofMod) Verify(session, N *big.Int) bool
```

Keygen Round 2'de her taraf kendi N'inin güvenli asal çarpanlara sahip olduğunu kanıtlar.

### 3.10 MtA Protokolü (`crypto/mta/`)

**Alice Init** (`share_protocol.go`):
```go
func AliceInit(session, ec, pk *paillier.PublicKey, a, NTildeJ, h1J, h2J *big.Int, rand) (cA *big.Int, pi *RangeProofAlice, error)
// cA = Enc(pk, a)  (Paillier ile a şifreler)
// pi = RangeProof(a < q^3)  (a'nın aralıkta olduğunu kanıtlar)
```

**Bob Mid** (gamma * k için MtA):
```go
func BobMid(session, ec, pk, piAlice, b, cA, NTildeJ, h1J, h2J, NTildeI, h1I, h2I *big.Int, rand) (beta, cB, betaPrime, piB *ProofBob, error)
// betaPrime: rastgele [-q^5, q^5] aralığında örneklenir
// cB = b*cA + Enc(betaPrime)  (Paillier homomorfik)
// beta = -betaPrime mod q
// Sonuç: Alice'in alpha = a*b + betaPrime, Bob'un beta = -betaPrime
// alpha + beta = a*b (mod q) ✓
```

**Bob Mid WC** (w * k için MtA, EC key tutarlılık kanıtıyla):
```go
func BobMidWC(session, ec, pk, piAlice, b, cA, ..., bigB *ECPoint, rand) (v, cB, vPrime, piB *ProofBobWC, error)
// ProofBobWC: bigB = G*b de kanıtlanır (signing'de wi için)
```

**Alice End**:
```go
func AliceEnd(session, ec, pk, proofBob, h1I, h2I, cA, cB, NTildeI *big.Int, sk) (*big.Int, error)
// 1. ProofBob doğrula
// 2. alpha = Decrypt(sk, cB)
```

**Range Proof Alice** (`range_proof.go`):
```go
type RangeProofAlice struct {
    Z, U, W, S, S1, S2 *big.Int
}
// m < q^3 kanıtlar
// z = h1^m * h2^rho mod NTilde
// u = gamma^alpha * beta^N mod N^2
// Challenge: SHA512/256 + RejectionSample → uniform dağılım
```

**Proof Bob** (`proofs.go`):
```go
type ProofBob struct {
    Z, ZPrm, T, V, W, S, S1, S2, T1, T2 *big.Int
}
// b in [-q^3, q^3] kanıtlar

type ProofBobWC struct {
    ProofBob
    U *ECPoint   // U = G*alpha → EC key tutarlılığı
}
// b in [-q^3, q^3] VE bigB = G*b kanıtlar
```

### 3.11 BIP-32 Çocuk Anahtar Türetimi (`crypto/ckd/`)

```go
type ExtendedKey struct {
    PublicKey  ecdsa.PublicKey
    Depth      uint8
    ChildIndex uint32
    ChainCode  []byte
    ParentFP   []byte
    Version    []byte
}

func DeriveChildKey(index uint32, pk *ExtendedKey, ec) (*big.Int, *ExtendedKey, error)
// HMAC-SHA512(ChainCode, PublicKey.Bytes || index) → IL || IR
// childKey = parentKey + IL  (mod n)
// Sadece non-hardened (public) türetme
// RIPEMD-160 ile fingerprint

func DeriveChildKeyFromHierarchy(indexes []uint32, pk *ExtendedKey, q, ec) (*big.Int, *ExtendedKey, error)
// Hiyerarşik path: [12, 209, 3] gibi
```

### 3.12 ECPoint (`crypto/ecpoint.go`)

```go
type ECPoint struct {
    curve  elliptic.Curve
    coords [2]*big.Int   // X, Y
}

func NewECPoint(ec, x, y *big.Int) (*ECPoint, error)
// Eğri üzerinde mi kontrol eder
// X, Y [0, P) aralığında mı kontrol eder → SRC-2026-573 düzeltmesi
// Non-canonical repr. red

func (p *ECPoint) EightInvEight() *ECPoint
// Edwards25519 için: 8 * (1/8 * P) = P ama cofactor temizler
// EdDSA için önemli: small subgroup saldırısına karşı
```

---

## 4. ECDSA Keygen — 4 Tur

### Ön Hazırlık (`ecdsa/keygen/prepare.go`)

```go
func GeneratePreParamsWithContextAndRandom(ctx, rand, concurrency) (*LocalPreParams, error)
```

Paralel olarak üretilir:
- **Paillier keypair:** 2048-bit, safe prime P,Q; |P-Q| büyük olmalı
- **NTilde, h1, h2:** İki 1024-bit safe prime P=2p+1, Q=2q+1 seçilir
  - NTilde = P*Q
  - f1 = rastgele, h1 = f1^2 mod NTilde (quadratic residue)
  - alpha = rastgele, beta = alpha^(-1) mod p*q
  - h2 = h1^alpha mod NTilde
  - h2 = h1^alpha = h1^(1/beta) → DLN ilişkisi

```go
type LocalPreParams struct {
    PaillierSK      *paillier.PrivateKey
    NTildei, H1i, H2i *big.Int
    Alpha, Beta, P, Q *big.Int   // DLN kanıtı için saklanır
}
```

**Neden önceden üretilir?** Safe prime + Paillier üretimi dakika sürebilir. Protocol başlamadan önce yapılırsa keygen hızlanır.

### Round 1 (`ecdsa/keygen/round_1.go`)

```
Yayımlananlar (broadcast):
  KGRound1Message {
    Commitment C,     // VSS commitments'ının hash taahhüdü
    PaillierPK,       // N (2048-bit)
    NTilde,           // 2048-bit
    H1, H2,           // Pedersen parametreleri
    DLNProof1,        // h1 = h2^beta → DLOG ilişkisi
    DLNProof2         // h2 = h1^alpha → DLOG ilişkisi
  }
```

İşlemler:
1. `ui` → rastgele skalar (gizli pay katkısı)
2. VSS → `Create(ec, threshold, ui, ids)` → `vs[]` (EC taahhütler), `shares[]` (Shamir payları)
3. `cmt = HashCommit(flatten(vs))` → VSS commitments taahhüdü
4. PreParams'ı kullan veya üret (Paillier + NTilde/h1/h2)
5. DLN kanıtları üret (her iki yönde: h1→h2 ve h2→h1)
6. `ui` bellekten silinir (`ui = zero`)
7. SSID hesapla: `SHA512_256i(curve_params || party_keys || round || ssidNonce)`

### Round 2 (`ecdsa/keygen/round_2.go`)

```
P2P mesajlar (her Pj'ye ayrı):
  KGRound2Message1 {
    Share,     // Pj'nin Shamir payı
    FacProof   // N = p*q ve p,q büyük kanıtı
  }

Broadcast:
  KGRound2Message2 {
    DeCommitment,  // VSS commitments'ın açılması
    ModProof       // N güvenli asal çarpanlı kanıtı
  }
```

İşlemler:
1. Tüm Round 1 mesajları doğrula:
   - Paillier N boyutu = 2048-bit
   - H1 != H2
   - NTilde boyutu = 2048-bit
   - H1, H2 değerleri benzersiz (tüm partiler arasında)
   - DLN kanıtı 1 doğrula (paralel goroutine, semaphore ile concurrency sınırı)
   - DLN kanıtı 2 doğrula (paralel)
2. Veriler kaydedilir: `PaillierPKs[j]`, `NTildej[j]`, `H1j[j]`, `H2j[j]`, `KGCs[j]`
3. Her Pj için Shamir payı gönder + FacProof
4. VSS commitments açılımını yayımla + ModProof

`DlnProofVerifier`: Semaphore tabanlı concurrency sınırlayıcı. `concurrency` level ile init edilir, goroutine başlatmadan önce kanal'a yazar.

### Round 3 (`ecdsa/keygen/round_3.go`)

```
Broadcast:
  KGRound3Message {
    PaillierProof   // 13 iterasyon ZK kanıtı
  }
```

İşlemler:
1. `xi = sum(shares)` → kendi gizli payını hesapla (tüm tarafların katkısı)
2. Her j için paralel goroutine:
   - Taahhüt açılımını doğrula: `HashDeCommit(KGCj, KGDj)` → `flatPolyGs`
   - ModProof doğrula: N güvenli asal çarpanlı mı?
   - VSS payını doğrula: `PjShare.Verify(threshold, PjVs)`
   - FacProof doğrula: N = p*q ve p,q büyük mü?
3. `Vc[c] = sum(vs[c])` → tüm tarafların VSS commitments'ını topla
4. Her Pj için `BigXj` hesapla (Lagrange ile):
   - `BigXj = Vc[0] + Vc[1]*kj + Vc[2]*kj^2 + ...`
5. `ECDSAPub = Vc[0]` → agregat public key
6. Paillier kanıtı yayımla

### Round 4 (`ecdsa/keygen/round_4.go`)

Son tur, mesaj üretilmez:
1. Her j için paralel: `PaillierProof.Verify(paiPKj_N, partyID_j, ECDSAPub)`
2. Hepsi geçerse `end <- save` → protokol bitti

### Kaydedilen Veri (`ecdsa/keygen/save_data.go`)

```go
type LocalPartySaveData struct {
    LocalPreParams                           // Paillier SK, NTilde, H1, H2, P, Q
    LocalSecrets {Xi, ShareID *big.Int}      // Gizli pay
    Ks      []*big.Int                       // Tüm tarafların share ID'leri
    NTildej, H1j, H2j []*big.Int            // Diğer tarafların Pedersen params
    BigXj   []*crypto.ECPoint               // Xi = G*xi (her taraf için)
    PaillierPKs []*paillier.PublicKey        // Diğer tarafların public key'leri
    ECDSAPub *crypto.ECPoint                 // Agregat public key y
}
```

### SSID Hesaplama

```go
func (round *base) getSSID() ([]byte, error) {
    ssidList := [ec.P, ec.N, ec.B, ec.Gx, ec.Gy]   // Eğri parametreleri
    ssidList += party_ids.Keys()                      // Tüm taraf ID'leri
    ssidList += flatten(BigXj)                        // Tüm EC public paylar
    ssidList += NTildej                               // Pedersen N'leri
    ssidList += H1j, H2j                             // Pedersen generators
    ssidList += round_number                          // Tur numarası
    ssidList += ssidNonce                             // Dış nonce
    return SHA512_256i(ssidList...).Bytes()
}
```

Her protokol turu için farklı SSID → cross-session proof replay'e karşı.

---

## 5. ECDSA Signing — 9 Tur + Final (GG18)

### Ön Hazırlık (`ecdsa/signing/prepare.go` + `round_1.go`)

```go
func PrepareForSigning(ec, i, pax, xi, ks, bigXs) (wi, bigWs)
// Lagrange katsayısını hesapla:
// wi = xi * prod(ks[j] / (ks[j] - ks[i])) for j!=i
// Bu, xi'yi tam imzalama payına dönüştürür
// bigWs[j] = BigXj * prod(ks[c] / (ks[c] - ks[j])) for c!=j
```

### Round 1: MtA Başlatma + Gamma Taahhüdü

```
P2P (her Pj'ye):
  SignRound1Message1 {
    cA,          // Enc(Paillier, k_i) — k şifreli
    RangeProofAlice  // k_i < q^3 kanıtı
  }

Broadcast:
  SignRound1Message2 {
    Commitment   // gamma_i*G taahhüdü
  }
```

İşlemler:
1. `k` ve `gamma` → rastgele skalärlar seç
2. `pointGamma = G * gamma`, taahhüt al
3. Her Pj için: `AliceInit(ssid || j, k, NTildej[j], H1j, H2j)` → `(cA, rangeProof)`

### Round 2: MtA Bob Yanıtı

```
P2P (her Pj'ye geri):
  SignRound2Message {
    c1ji, ProofBob,    // gamma*k için MtA
    c2ji, ProofBobWC   // wi*k için MtA
  }
```

Paralel iki goroutine:
- `BobMid(gamma_i, cA_j, ...)` → `(beta, c1ji, pi1ji)` — gamma*k için
- `BobMidWC(wi, cA_j, ..., bigWi)` → `(v, c2ji, pi2ji)` — wi*k için

### Round 3: MtA Tamamlama + Theta Hesabı

```
Broadcast:
  SignRound3Message { theta }   // theta_i = ki*gamma_i + sum(alpha+beta)
```

İşlemler:
1. Her j için paralel:
   - `AliceEnd(cij, c1ji, ...)` → `alpha[j]` (ki*gammaj + betaPrime)
   - `AliceEndWC(cij, c2ji, ..., bigWj)` → `u[j]`
2. `theta = ki*gammai + sum(alpha[j] + beta[j])` — ki*Gamma toplamı
3. `sigma = ki*wi + sum(u[j] + v[j])` — ki*X toplamı (gizli, yayımlanmaz)

**Sabit zaman koruması:**
```go
if common.IsConstantTimeEnabled() {
    ctModN := common.NewCTModInt(N)
    theta = ctModN.MulCT(k, gamma)
    sigma = ctModN.MulCT(k, w)
}
```

### Round 4: Theta Ters + Schnorr Kanıtı

```
Broadcast:
  SignRound4Message {
    DeCommit(gamma*G),  // Round 1'deki taahhüdün açılımı
    SchnorrProof(gamma, gamma*G)  // gamma bilgisi kanıtı
  }
```

İşlemler:
1. `thetaTotal = sum(theta_j)` → tüm taraflardan theta toplanır
2. `thetaInverse = ModInverse(thetaTotal)` — sabit zaman: `CTModInt.ModInverseCT`
3. Schnorr kanıtı: `NewZKProof(ssid||i, gamma, gammaPoint)`

### Round 5: R Hesabı + si

```
Broadcast:
  SignRound5Message { Commitment(Vi, Ai) }
```

İşlemler:
1. Tüm `gammaJ * G` noktaları topla: `R = sum(gammaJ * G)`
2. `R = R * thetaInverse` → `R = G * (k^-1)` (imzanın R noktası)
3. `rx = R.x`, `ry = R.y`
4. `si = m*ki + rx*sigma_i` — imzanın s payı
5. `li`, `roi` rastgele → `Vi = R^si * G^li`, `Ai = G^roi`
6. **k ve w bellekten silindi**: `round.temp.k = zero`, `round.temp.w = zero`

**Sabit zaman:**
```go
ctModN := common.NewCTModInt(N)
mk := ctModN.MulCT(m, k)
rxSigma := ctModN.MulCT(rx, sigma)
si = modN.Add(mk, rxSigma)
```

### Round 6: ZK Kanıtları

```
Broadcast:
  SignRound6Message {
    DPower,   // Decommit(Vi, Ai) açılımı
    piAi,     // Schnorr: roi bilgisi (Ai = G^roi)
    piV       // ZKV: (si, li) bilgisi (Vi = R^si * G^li)
  }
```

`NewZKVProof(ssid||i, Vi, R, si, li)` — V kanıtı si'yi doğrudan ifşa etmeden kanıtlar.

### Round 7: V ve A Doğrulama

İşlemler:
1. Her j için: taahhüt aç, `bigVjs[j]`, `bigAjs[j]` al
2. Schnorr kanıtı doğrula: `piAj.Verify(Aj)`
3. ZKV kanıtı doğrula: `piVj.Verify(Vj, R)`
4. `V = g^(-m) * y^(-rx) * prod(Vj)` — toplam V kontrol noktası
5. `A = prod(Aj)`
6. `Ui = V^roi`, `Ti = A^li`
7. Taahhüt al: `commit(Ui, Ti)`

### Round 8: Ui/Ti Decommit

```
Broadcast: SignRound8Message { DeCommit(Ui, Ti) }
```

### Round 9: U == T Kontrolü + s Yayımlama

İşlemler:
1. Tüm `Uj`, `Tj` taahhüt aç ve topla
2. `U = prod(Uj)`, `T = prod(Tj)` → eşit olmalı
3. `U != T` → **hata, imzalamayı iptal et** (aldatma tespiti)
4. `si` yayımla

**Neden U == T kontrolü?** Eğer bir taraf yanlış `si` gönderirse, imzayı doğrulamak başarısız olur ama hangi taraf hatalı bilinmez. Bu ek tur ile hatalı taraf tespit edilebilir (identifiable abort alternatifi).

### Finalization (Round 10)

```go
sumS = sum(si_j)
// Low-S normalizasyon (Bitcoin/Tendermint uyumluluğu):
if sumS > N/2 { sumS = N - sumS; recid ^= 1 }
// recid hesabı: rx > N veya ry tek → bit ayarları
round.data.Signature = R.Bytes() || sumS.Bytes()
// ECDSA.Verify kontrolü
round.end <- round.data
```

---

## 6. ECDSA Resharing — 5 Tur

Eski komite (old committee, t+1 taraf) sırrını kaybetmeden yeni komiteye (new committee, n' taraf) aktarır.

### Round 1 (Eski Komite → Yeni Komite)

Eski komite her üyesi:
1. `wi = PrepareForSigning(xi, ks)` — Lagrange ağırlıklı pay
2. `vss.Create(newThreshold, wi, newKs)` → yeni komite için VSS
3. Taahhüt yayımla + SSID paylaş

### Round 2 (Yeni Komite → Eski + Yeni)

Yeni komite her üyesi:
1. SSID tutarlılığını doğrula (tüm eski komite aynı SSID göndermiş mi?)
2. Kendi PreParams'ını üret (Paillier + NTilde/h1/h2)
3. DLN kanıtları üret
4. ModProof üret
5. "ACK" → eski komiteye
6. Paillier PK + NTilde/H1/H2 + DLN + Mod → yeni komiteye

### Round 3 (Eski Komite → Yeni Komite)

Eski komite:
1. Her yeni komite üyesi için Shamir payı gönder (P2P)
2. VSS commitments açılımı yayımla

### Round 4 (Yeni Komite → Eski + Yeni)

Yeni komite:
1. DLN kanıtlarını doğrula (paralel)
2. ModProof doğrula
3. Taahhütleri aç, VSS paylarını doğrula
4. Kontrol: `Vc[0] == ECDSAPub` → sır değişmedi!
5. `newXi = sum(shareJ)` — yeni gizli pay
6. `newBigXj` hesapla (yeni komite için)
7. FacProof → diğer yeni taraflara
8. "ACK" → her ikisine de

### Round 5 (Final)

Yeni komite: FacProof doğrula, tüm verileri kaydet
Eski komite: `input.Xi.SetInt64(0)` — gizli payı sil!

---

## 7. EdDSA Keygen — 3 Tur

ECDSA'dan temel farklar:
- **Paillier YOK** — EdDSA signing MtA gerektirmez
- **ProofFac/ProofMod YOK**
- **DLN kanıtı YOK**
- Round 3'te `EightInvEight()` cofactor temizleme (edwards25519 için)
- Daha basit save data: `{Xi, ShareID, Ks, BigXj, EDDSAPub}`

Round 2'de Schnorr kanıtı eklenir: `NewZKProof(ssid||i, ui, vs[0])` — ui bilgisi kanıtlanır.

---

## 8. EdDSA Signing — 3 Tur + Final

**Çok daha basit** — MtA YOK:

Round 1: `ri` seç, `Ri = G*ri`, taahhüt al
Round 2: `Ri` açılımı + Schnorr kanıtı
Round 3:
```
lambda = SHA512(R || pubkey || message)
si = ri + lambda * wi  (mod q, edwards25519 aritmetiği)
```
Final: `S = sum(si)`, `edwards.Verify(&pk, M, r, S)`

**edwards25519 encoding:** Little-endian `[32]byte`, `reverse()` ile dönüşüm yapılır.

---

## 9. Test Altyapısı

### `test/config.go`

```go
TestParticipants = 5
TestThreshold    = 2  // n/2 = 5/2 = 2
```

### `test/utils.go`

```go
func SharedPartyUpdater(party tss.Party, msg tss.Message, errCh chan<- *tss.Error)
// Wire encode et → Wire decode et → Update çağır
// Gerçek network şartlarını simüle eder
```

### Test Fixture Sistemi

- `tryWriteTestFixtureFile` → JSON serialize, `0600` izin
- `LoadKeygenTestFixtures` → Test sırasında fixture yoksa from scratch üretir
- `LoadKeygenTestFixturesRandomSet` → t+1 rastgele taraf seçer (signing için)
- Fixture dosyaları `test/_ecdsa_fixtures/` ve `test/_eddsa_fixtures/` altında

### `TestE2EConcurrentAndSaveFixtures` (keygen test)

Tam protokol testi:
- Tüm taraflar paralel goroutine'de başlatılır
- `select` döngüsü: `outCh` → mesaj yönlendir, `endCh` → save data al
- Doğrulama: `u*G == ECDSAPub`, `BigXj == xj*G`, VSS reconstruct
- Sahte taraf testi: payı sıfırla → reconstruct farklı sonuç verir

### `TestE2EWithHDKeyDerivation` (signing test)

BIP-32 entegrasyonu testi:
- `derivingPubkeyFromPath([12, 209, 3])` → `keyDerivationDelta`
- `UpdatePublicKeyAndAdjustBigXj` → tüm tarafların BigXj'si güncellenir
- `NewLocalPartyWithKDD(..., keyDerivationDelta)` → signing

---

## 10. Güvenlik Analizi

### 10.1 Sabit Zaman

- `CTModInt` (`filippo.io/bigmod`) — Go'nun `crypto/rsa` ile aynı kütüphane
- `TimingProtection` — Paillier decrypt için rastgele jitter
- Signing round 3, 4, 5'te secret değerler için zorunlu

### 10.2 Bellek Temizleme

```go
// Round 1 keygen: ui sıfırlanır
ui = zero
// Round 5 signing: k ve w sıfırlanır
round.temp.w = zero
round.temp.k = zero
// Round 5 resharing: eski xi sıfırlanır
round.input.Xi.SetInt64(0)
```

**Sorun:** Go'da GC vardır ve `zero` ataması compiler tarafından optimize edilebilir. Gerçek güvenli silme için `memzero` veya `sync/atomic` gerekir. Bu Rust'ta daha güvenlidir (`zeroize` crate).

### 10.3 DLN Concurrent Doğrulama

`DlnProofVerifier` semaphore pattern:
```go
dpv.semaphore <- struct{}{}   // Slot al (bloklar)
go func() {
    defer func() { <-dpv.semaphore }()  // Slot serbest bırak
    // Kanıt doğrula
}()
```
Maksimum `concurrency` paralel DLN doğrulama yapar.

### 10.4 ECPoint Non-Canonical Rejection

```go
// SRC-2026-573 düzeltmesi
if x.Sign() < 0 || x.Cmp(curve.Params().P) >= 0 { error }
if y.Sign() < 0 || y.Cmp(curve.Params().P) >= 0 { error }
```

Koordinatlar `[0, P)` aralığında olmalı — non-canonical encoding saldırılarına karşı.

### 10.5 H1/H2 Benzersizlik Kontrolü

```go
// Round 2 keygen:
if H1j.Cmp(H2j) == 0 { error }   // H1 != H2
// h1H2Map ile tüm partiler arasında benzersizlik
```

Eğer iki taraf aynı H1 veya H2 değeri kullanırsa → hata ve culprit raporlanır.

### 10.6 Transport Güvenliği

README'den: Kullanıcı sorumluluğu:
- E2E şifreleme (her P2P mesaj)
- Güvenilir broadcast
- Session ID (SSID) koordinasyonu
- Replay koruması

tss-lib bunları sağlamaz, sadece protokolü implement eder.

---

## 11. Mimari Kararlar — Nova Shield İçin Çıkarımlar

### 11.1 Round-Based State Machine

tss-lib'in `Round` interface tasarımı mükemmel:
- Her round bağımsız `Start()` → `Update()` → `CanProceed()` → `NextRound()` döngüsü
- `ok[]` array ile taraf bazlı ilerleme takibi
- Recursive `BaseUpdate` ile out-of-order mesaj desteği

**Nova Shield için:** Aynı pattern'i Rust'ta `trait Round` olarak implement et. `tokio::channel` kullanarak async uyumlu yap.

### 11.2 PreParams Tasarımı

Ağır kriptografik hesaplamalar (Paillier + safe primes) önceden yapılır, protokolde hazır verilir. Bu fikir Nova Shield'de MPC hesap açılışı için kritik.

**Nova Shield için:** Guardian'lar hesap açılırken PreParams üretip güvenli saklar. İmza sırasında hazır kullanılır.

### 11.3 Identifiable Abort

`Error.culprits` — kötü niyetli partiyi tespit et ve raporla. Nova Shield'de Emergency Freeze protokolü için temel: Freeze tetiklendiğinde sorumluluk zinciri izlenebilir.

### 11.4 SSID Tasarımı

`SHA512_256i(curve || parties || public_keys || NTildes || H1s || H2s || round || nonce)`

Bu zengin SSID şunları sağlar:
- Farklı turlar → farklı SSID
- Farklı session'lar → farklı SSID
- Farklı katılımcılar → farklı SSID

**Nova Shield için:** SSID'ye `emergency_freeze_nonce` ekle — dondurma sonrası yeni session'lar eski ZK kanıtlarını yeniden kullanamaz.

### 11.5 Constant-Time Zorunluluğu

`CTModInt` pattern'i doğru. Nova Shield Rust implementasyonunda:
- `subtle` crate (constant-time comparisons)
- `zeroize` crate (memory zeroization)
- `constant_time_eq` crate

### 11.6 MtA Betaprime Aralığı

tss-lib: `betaPrime ← [-q^5, q^5]`
multi-party-ecdsa (Rust): `betaPrime ← [-q^3, q^3]`

**Fark:** tss-lib daha büyük aralık kullanıyor. q^5 range → daha güvenli ama daha büyük proof. Güvenlik kanıtları q^5 aralığını destekliyor.

**Nova Shield için:** q^5 kullan (Binance'in tercihi).

### 11.7 EdDSA vs ECDSA Karşılaştırması

| Özellik        | ECDSA (GG18) | EdDSA |
|----------------|--------------|-------|
| Paillier       | Zorunlu      | Yok   |
| MtA            | 2 tur (gamma+w) | Yok |
| Signing turları | 9 + final   | 3 + final |
| Hız            | Yavaş        | Hızlı |
| Validator imzası| ECDSA/secp256k1 | Ed25519 |

**Nova Shield için:** BLS threshold için bu pattern'den yararlan. Validator imzaları BLS12-381 ama kullanıcı hesap imzaları threshold Secp256k1 (Bitcoin/Ethereum uyumluluğu için).

### 11.8 Resharing Sırasında Kriptografik Güvence

`Vc[0] == ECDSAPub` kontrolü: Yeni komite, sırrın değişmediğini matematiksel olarak doğrular. Bu, Nova Shield'in Guardian değişimi protokolü için kritik.

**Nova Shield için:** Guardian rotate işleminde bu kontrol zorunlu — yeni guardian, eski sırrın doğru taşındığını kanıtlamak zorunda.

### 11.9 Signing'de Lagrange Ağırlıklandırma

```go
func PrepareForSigning(ec, i, pax, xi, ks, bigXs) (wi, bigWs)
// wi = xi * prod(ks[j] / (ks[j] - ks[i])) for j!=i
```

t+1 taraftan herhangi biri katılabilir, keygen'deki sıralamaya gerek yok. `LoadKeygenTestFixturesRandomSet` ile test ediliyor.

**Nova Shield için:** Hangi Guardian'ların mevcut olduğuna göre dinamik seçim yapılabilir. Offline guardian'lar atlanır.

---

## 12. Özet Tablo — tss-lib'de Kullanılan Kriptografi

| Bileşen | Algoritma | Kaynak |
|---------|-----------|--------|
| Eğri | secp256k1 (ECDSA), Ed25519 (EdDSA) | btcd, decred |
| Threshold | (t,n)-Shamir VSS | Feldman |
| MPC imza | GG18 (signing 9 tur) | Gennaro-Goldfeder 2018 |
| Paillier | 2048-bit, safe prime | standart |
| Pedersen | NTilde, h1, h2 (safe prime tabanlı) | standart |
| ZK-Proof | DLN (128 iter), Schnorr, ZKV, RangeProof | çeşitli |
| ZK-Proof | ProofFac (Fig 28), ProofMod (80 iter) | GG20 eklemeleri |
| Hash | SHA512/256 + length prefix + \n delimiter | özel |
| Sabit zaman | filippo.io/bigmod | Go stdlib benzeri |
| BIP-32 | Non-hardened child key derivation | BIP-32 |
| Transport | ProtoBuf encoding | Google |

---

## 13. Dosya Referans Haritası

```
tss-lib/
├── tss/
│   ├── party.go          BaseParty, BaseStart, BaseUpdate (recursive!)
│   ├── party_id.go       PartyID, SortPartyIDs
│   ├── round.go          Round interface
│   ├── message.go        Message, MessageRouting, ProtoBuf wire
│   ├── params.go         Parameters, ReSharingParameters
│   ├── error.go          Error + culprits (identifiable abort)
│   ├── curve.go          Curve registry
│   └── wire.go           ParseWireMessage
├── common/
│   ├── hash.go           SHA512_256i, tagged variant
│   ├── safe_prime.go     GermainSafePrime, concurrent generation
│   ├── constant_time.go  CTModInt, TimingProtection
│   ├── int.go            ModInt wrapper
│   └── random.go         GetRandomPositiveInt, QNR, etc.
├── crypto/
│   ├── paillier/         GenerateKeyPair, Encrypt, Decrypt, Proof
│   ├── vss/              Create, Verify, ReConstruct (Feldman VSS)
│   ├── schnorr/          ZKProof, ZKVProof (session-tagged)
│   ├── dlnproof/         128-iteration DLN proof
│   ├── mta/              AliceInit, BobMid(WC), AliceEnd(WC), RangeProof
│   ├── facproof/         ProofFac (Fig 28, GG20)
│   ├── modproof/         ProofMod (80 iter, safe prime proof)
│   ├── commitments/      HashCommitDecommit (SHA512_256i)
│   ├── ckd/              BIP-32 non-hardened child key derivation
│   ├── ecpoint.go        ECPoint + non-canonical rejection
│   └── utils.go          GenerateNTildei
├── ecdsa/
│   ├── keygen/           4-tur DKG (LocalPreParams + 4 round)
│   ├── signing/          9+1 tur signing (MtA + R + si + verify)
│   └── resharing/        5-tur dinamik grup değişimi
├── eddsa/
│   ├── keygen/           3-tur, Paillier yok
│   ├── signing/          3+1 tur, lambda*wi pattern
│   └── resharing/        5-tur, benzer ECDSA
└── test/
    ├── config.go         TestParticipants=5, TestThreshold=2
    └── utils.go          SharedPartyUpdater
```
