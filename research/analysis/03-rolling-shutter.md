# Rolling Shutter — Kapsamlı Teknik Analiz

**Repo:** `github.com/shutter-network/rolling-shutter`
**Dil:** Go 1.23
**Lisans:** MIT
**Amaç:** Rollup'lar için MEV koruması — eşik şifrelemeli mempool

---

## İçindekiler

1. [Genel Mimari](#1-genel-mimari)
2. [Kriptografik Temel — shcrypto](#2-kriptografik-temel--shcrypto)
3. [puredkg — Saf DKG Protokolü](#3-puredkg--saf-dkg-protokolü)
4. [ShutterApp — Tendermint ABCI Uygulaması](#4-shutterapp--tendermint-abci-uygulaması)
5. [KeyperCore — Keyper Ana Servisi](#5-keypercor--keyper-ana-servisi)
6. [EpochKG — Epoch Anahtar Üretimi](#6-epochkg--epoch-anahtar-üretimi)
7. [epochkghandler — Şifre Çözme Tetikleyicisi](#7-epochkghandler--şifre-çözme-tetikleyicisi)
8. [smobserver — Shuttermint Durum Gözlemcisi](#8-smobserver--shuttermint-durum-gözlemcisi)
9. [P2P Katmanı — libp2p + GossipSub](#9-p2p-katmanı--libp2p--gossipsub)
10. [Mesaj Yapıları — p2pmsg Protobuf](#10-mesaj-yapıları--p2pmsg-protobuf)
11. [IdentityPreimage ve EpochID](#11-identitypreimage-ve-epochid)
12. [Tam İşlem Akışı (Pipeline)](#12-tam-işlem-akışı-pipeline)
13. [Keyper Implementasyonları](#13-keyper-implementasyonları)
14. [Güvenlik Analizi](#14-güvenlik-analizi)
15. [Nova Shield İçin Çıkarımlar](#15-nova-shield-için-çıkarımlar)

---

## 1. Genel Mimari

Rolling Shutter üç katmanlı bir sistemdir:

```
┌─────────────────────────────────────────────────────┐
│  Katman 1: Shuttermint (Tendermint sidechain)        │
│  - DKG koordinasyonu (PolyCommit/PolyEval/Accusation)│
│  - BatchConfig (keyper seti) oylama                  │
│  - Validator yönetimi                                │
└─────────────────────────────────────────────────────┘
             ↑ ABCI mesajları (shmsg)
┌─────────────────────────────────────────────────────┐
│  Katman 2: Keyper Node'ları                         │
│  - DKG katılımcısı (puredkg)                        │
│  - Epoch secret key share hesaplama                 │
│  - Decryption trigger bekle → key share yay         │
└─────────────────────────────────────────────────────┘
             ↑ libp2p GossipSub
┌─────────────────────────────────────────────────────┐
│  Katman 3: P2P Gossip                               │
│  - DecryptionTrigger (sequencer → keyperler)        │
│  - DecryptionKeyShares (keyper → herkese)           │
│  - DecryptionKeys (eşik tamamlandı → herkese)       │
│  - EonPublicKey (yeni eon başladı → herkese)        │
└─────────────────────────────────────────────────────┘
```

**Temel akış:**
1. Kullanıcı TX'ini `Encrypt(tx, eonPubKey, epochID, sigma)` ile şifreler
2. Sequencer şifreli TX'i bloğa alır (MEV koruması: içerik bilinmiyor)
3. Blok kapandıktan sonra sequencer DecryptionTrigger gönderir
4. Her keyper kendi epoch secret key share'ini hesaplayıp P2P'de yayınlar
5. Threshold kadar share toplandığında DecryptionKey oluşturulur
6. Sequencer TX'i çözer, state'e uygular

**Kritik özellik:** Şifreli TX'in içeriği blok commit olmadan ÖNCE bilinmez → MEV matematiği imkansız.

---

## 2. Kriptografik Temel — shcrypto

Rolling Shutter kendi şifreleme kütüphanesini kullanır: `github.com/shutter-network/shutter/shlib/shcrypto`

Bu kütüphane doğrudan bu repoda değil, dependency olarak gelir. Ancak WASM sarmalayıcısından (`shcryptowasm/shutter_crypto_wasm.go`) ve Go testlerinden tam API'ı çıkardık:

### 2.1 Temel Tipler

```go
type EonPublicKey        // DKG sonucu oluşan eşik public key (BLS12-381 G2)
type EonSecretKeyShare   // Her keyper'in private payı (skaler)
type EonPublicKeyShare   // Her keyper'in public payı (G2 noktası)
type EpochID             // Hash-to-G1 sonucu (BLS12-381 G1 noktası)
type EpochSecretKey      // Çözme anahtarı (G1 noktası)
type EpochSecretKeyShare // Keyper'in katkısı (G1 noktası)
type EncryptedMessage    // Şifreli mesaj yapısı
type Block               // [32]byte sabit boyutlu blok (sigma için)
```

### 2.2 Şifreleme — `Encrypt()`

```go
func Encrypt(
    message    []byte,
    eonPubKey  *EonPublicKey,
    epochID    *EpochID,
    sigma      Block,          // rastgele 32 byte (kullanıcı sağlar)
) *EncryptedMessage
```

**Matematiksel yapı (IBE — Identity-Based Encryption, Boneh-Franklin varyantı):**
- BLS12-381 eğrisi kullanılır
- `epochID = H_G1(identityPreimage)` — identity preimage'ı G1 noktasına hashle
- `r` rastgele seçilir (sigma'dan türetilir)
- `C1 = r * G2` — kullanıcı ephemeral public key
- `C2 = message XOR H(e(epochID, eonPubKey)^r)` — XOR şifreleme
- `C3 = sigma XOR H(r * G2)` — sigma şifreleme

**Neden IBE?** Şifreleme için epochID (TX'in hangi blok/slota ait olduğu) yeterli. Şifre çözme anahtarı henüz mevcut değil (ileride keyperler üretecek). Bu classik "encrypt-then-reveal" paradigması.

### 2.3 Şifre Çözme — `Decrypt()`

```go
func (m *EncryptedMessage) Decrypt(
    decryptionKey *EpochSecretKey,
) ([]byte, error)
```

**Matematiksel yapı:**
- `decryptionKey = e(epochSecretKey, G2)` biçiminde verify edilir
- `e(C1, epochSecretKey) = e(r*G2, sk*epochID) = e(G2, epochID)^(r*sk)`
- Pairing ile orijinal `H(...)` değeri geri kazanılır
- `message = C2 XOR H(...)`

### 2.4 EpochSecretKey Üretimi

```go
// Her keyper kendi payını hesaplar
func ComputeEpochSecretKeyShare(
    eonSecretKeyShare *EonSecretKeyShare,
    epochID           *EpochID,
) *EpochSecretKeyShare

// Threshold kadar pay toplandığında tam key
func ComputeEpochSecretKey(
    keyperIndices       []int,
    epochSecretKeyShares []*EpochSecretKeyShare,
    threshold            uint64,
) (*EpochSecretKey, error)
```

**Matematiksel yapı:**
- `epochSecretKeyShare_i = eonSecretKeyShare_i * epochID` — skaler çarpım G1'de
- `epochSecretKey = Lagrange_interpolate(shares, threshold)` — Lagrange ile birleştirme
- Bu BLS eşik imzasının deterministik versiyonu

### 2.5 Doğrulama

```go
// Payın geçerliliğini doğrula (ZK olmayan, pairing tabanlı)
func VerifyEpochSecretKeyShare(
    share    *EpochSecretKeyShare,
    pubShare *EonPublicKeyShare,
    epochID  *EpochID,
) bool
// Doğrulama: e(share, G2) == e(epochID, pubShare)

// Tam anahtarın doğruluğunu doğrula
func VerifyEpochSecretKey(
    key    *EpochSecretKey,
    pubKey *EonPublicKey,
    identity []byte,
) (bool, error)
```

### 2.6 WASM Entegrasyonu

`shcryptowasm/` paketi Go'yu WebAssembly'ye derleyip JavaScript'ten kullanılabilir hale getirir:
- `jsRegistry.Set("encrypt", encrypt)` — tarayıcıdan çağrılabilir
- `jsRegistry.Set("decrypt", decrypt)`
- `jsRegistry.Set("verifyDecryptionKey", verifyDecryptionKey)`
- `_initialized` promise → WASM yükleme tamamlandığında resolve

---

## 3. puredkg — Saf DKG Protokolü

`github.com/shutter-network/shutter/shlib/puredkg` — dependency olarak gelir.

Test kodundan (`epochkg_test.go`) tam protokolü çıkardık:

### 3.1 PureDKG Yapısı

```go
type PureDKG struct {
    Phase     Phase  // Off | Dealing | Accusing | Apologizing | Finalized
    Eon       uint64
    NumKeypers uint64
    Threshold  uint64
    Keyper     uint64 // bu keyper'in indeksi
    // ... iç state
}

func NewPureDKG(eon, numKeypers, threshold, keyperIndex uint64) PureDKG
```

### 3.2 DKG Fazları

#### Faz 1 — Dealing (Paylaşım)

```go
commitment, polyEvalMsgs, err := dkg.StartPhase1Dealing()
```

- Her keyper `t` dereceli rastgele bir polinom `f_i(x)` oluşturur
- `commitment.Gammas = [G * a_0, G * a_1, ..., G * a_t]` — Feldman VSS taahhüdü
- `polyEvalMsgs[j].Eval = f_i(j)` — j'ye gönderilecek paylaşım
- `HandlePolyCommitmentMsg(msg)` → diğerinin taahhüdünü kaydet
- `HandlePolyEvalMsg(msg)` → kendi paylaşımını doğrula vs taahhüt

**Önemli:** PolyEval mesajları on-chain Shuttermint'te gider ancak alıcının ECIES public key'i ile şifrelenir (`smobserver.go:sendPolyEvals`'da `ecies.Encrypt`).

#### Faz 2 — Accusing (Suçlama)

```go
accusations := dkg.StartPhase2Accusing()
// accusation: {Eon, Accuser, Accused}
dkg.HandleAccusationMsg(AccusationMsg{...})
```

- Geçersiz pay gönderen keyper'lar suçlanır
- Her keyper en fazla 1 suçlama mesajı gönderebilir
- ShutterApp bu mesajları kayıt altına alır

#### Faz 3 — Apologizing (Özür Dileme)

```go
apologies := dkg.StartPhase3Apologizing()
// apology: {Eon, Accuser, Accused, Eval}
dkg.HandleApologyMsg(ApologyMsg{...})
```

- Suçlanan keyper özür dilerse gerçek `f(j)` değerini açıklar
- Apology geçerliyse suçlayan mahkum, geçersizse suçlanan elenir

#### Sonuç

```go
dkg.Finalize()
result, err := dkg.ComputeResult()
// result: {Eon, SecretKeyShare, PublicKey, PublicKeyShares, ...}
```

### 3.3 DKG Faz Zamanlaması

```go
// dkgphase/phase.go
type PhaseLength struct {
    off         int64  // = 0
    dealing     int64  // = 1 * phaseLen
    accusing    int64  // = 2 * phaseLen
    apologizing int64  // = 3 * phaseLen
}

func (plen *PhaseLength) GetPhaseAtHeight(height, eonStartHeight int64) puredkg.Phase
```

- Her faz eşit uzunlukta (config'e göre blok sayısı)
- `height - eonStartHeight` ile hangi fazda olduğu belirlenir
- Tipik deployment: her faz 4-8 blok

---

## 4. ShutterApp — Tendermint ABCI Uygulaması

`app/app.go` — Shuttermint sidechain'in Tendermint ABCI implementasyonu.

### 4.1 Veri Yapısı

```go
type ShutterApp struct {
    Configs      []*BatchConfig        // keyper seti geçmişi
    DKGMap       map[uint64]*DKGInstance  // eon → aktif DKG
    ConfigVoting ConfigVoting           // yeni config oylama
    Identities   map[common.Address]ValidatorPubkey  // checkin kayıtları
    BlocksSeen   map[common.Address]uint64   // keyper L1 blok gözlemi
    Validators   Powermap              // mevcut validator güç dağılımı
    EONCounter   uint64                // monoton artan eon sayacı
    CheckTxState *CheckTxState         // commit'te sıfırlanan mempool state
    NonceTracker *NonceTracker         // replay koruması
    ChainID      string
    ForkHeights  *ForkHeights          // protocol fork yükseklikleri
}
```

### 4.2 TX Akışı

```
DeliverTx
├── decodeTx()    — base64url decode → signer + MessageWithNonce
├── ChainID check
├── NonceTracker.Check() + Add()  — replay koruması
└── deliverMessage()
    ├── BatchConfig     → deliverBatchConfig()
    ├── BlockSeen       → deliverBlockSeen()
    ├── CheckIn         → deliverCheckIn()
    ├── DKGResult       → deliverDKGResult()
    ├── PolyEval        → handlePolyEvalMsg()
    ├── PolyCommitment  → handlePolyCommitmentMsg()
    ├── Accusation      → handleAccusationMsg()
    └── Apology         → handleApologyMsg()
```

### 4.3 BatchConfig (Keyper Seti) Güncelleme

```go
func (app *ShutterApp) deliverBatchConfig(
    msg *shmsg.BatchConfig, sender common.Address,
) abcitypes.ResponseDeliverTx
```

1. `BatchConfigFromMessage(msg)` → `BatchConfig` oluştur
2. `checkConfig(bc)` → geçerlilik kontrolü:
   - Threshold > 0, threshold ≤ len(keypers)
   - ActivationBlockNumber ≥ mevcut config'in activation bloğu
   - KeyperConfigIndex > mevcut config'in index'i
3. `ConfigVoting.AddVote(sender, bc)` → eşik oylama
4. `ConfigVoting.Outcome(threshold)` → threshold kadar aynı config oyu gelince:
   - `addConfig(bc)` → app.Configs'e ekle
   - `StartDKG(bc)` → yeni DKG instance başlat, EONCounter artır
   - `EonStarted` ABCI eventi yayınla

### 4.4 CheckIn

```go
func (app *ShutterApp) deliverCheckIn(
    msg *shmsg.CheckIn, sender common.Address,
) abcitypes.ResponseDeliverTx
```

- `msg.ValidatorPublicKey` → ed25519 validator key (Tendermint için)
- `msg.EncryptionPublicKey` → ECIES public key (PolyEval şifrelemesi için)
- `app.Identities[sender] = validatorPublicKey` → kaydet
- `CheckIn{Sender, EncryptionPublicKey}` eventi yayınla

### 4.5 EndBlock — Validator Güncellemesi

```go
func (app *ShutterApp) EndBlock(req abcitypes.RequestEndBlock) abcitypes.ResponseEndBlock
```

1. Başlatılmamış BatchConfig'ler için `BlocksSeen` sayılır
2. `numVotes >= numRequiredVotes` → config `Started = true`
3. `BatchConfigStarted` eventi yayınla
4. `ValidatorsUpdated` kontrolü: threshold kadar keyper check-in yaptıysa
5. `DiffPowermaps` → validator değişikliklerini hesapla
6. `ResponseEndBlock{ValidatorUpdates: ...}` döndür

**numRequiredTransitionValidators:**
```go
defenders := uint64(n - (n+2)/3 + 1)  // 2/3'ten fazla
if config.Threshold >= defenders {
    return config.Threshold
}
return defenders
```

Hem threshold hem de BFT 2/3 koşulunu karşılaması gerekiyor.

### 4.6 DKGInstance

```go
type DKGInstance struct {
    Config              BatchConfig
    Eon                 uint64
    SuccessVoting       Voting[bool, ComparableEquals[bool]]
    PolyEvalsSeen       map[SenderReceiverPair]struct{}
    PolyCommitmentsSeen map[common.Address]struct{}
    AccusationsSeen     map[common.Address]struct{}
    ApologiesSeen       map[common.Address]struct{}
}
```

- Her mesaj tipi için "daha önce görüldü mü" kontrolü
- `SuccessVoting` → DKG başarılı/başarısız oylaması
- `maybeStartEon()` → threshold oyu gelince yeni eon başlat

### 4.7 Voting Generic Tipi

```go
type Voting[T any, E Equalser[T]] struct {
    votes map[common.Address]T
}
func (v *Voting[T,E]) AddVote(sender common.Address, val T) error
func (v *Voting[T,E]) Outcome(threshold int) (T, bool)
```

- Eşik sayıda aynı değere oy gelince `Outcome` döner
- `ComparableEquals[bool]` veya `ReflectDeepEquals[BatchConfig]` eşitlik karşılaştırıcısı

---

## 5. KeyperCore — Keyper Ana Servisi

`keyper/keyper.go`

### 5.1 Yapı

```go
type KeyperCore struct {
    trigger           <-chan *broker.Event[*epochkghandler.DecryptionTrigger]
    config            *kprconfig.Config
    dbpool            *pgxpool.Pool        // PostgreSQL
    shuttermintClient client.Client        // Tendermint RPC
    messaging         p2p.Messaging        // libp2p GossipSub
    messageSender     fx.RPCMessageSender  // Shuttermint TX gönderici
    blockSyncClient   *ethclient.Client    // Ethereum L1 dinleyici
    shuttermintState  *smobserver.ShuttermintState
    metricsServer     *metricsserver.MetricsServer
}
```

### 5.2 Başlatma ve Servisler

```go
func (kpr *KeyperCore) Start(ctx context.Context, runner service.Runner) error
```

Başlatılan servisler:
1. `kpr.messaging` — libp2p P2P servisi
2. `operateShuttermint` — L1 blok + Shuttermint senkronizasyon döngüsü
3. `newEonPubKeyHandler(kpr)` — yeni Eon public key yayınlayıcı
4. `KeyShareHandler` — DecryptionTrigger alıp key share gönderen servis
5. (opsiyonel) `kprapi.NewHTTPService` — HTTP API (manuel DecryptionTrigger için)
6. (opsiyonel) `metricsServer` — Prometheus metrikleri

### 5.3 operateShuttermint Döngüsü

```go
func (kpr *KeyperCore) operateShuttermint(ctx context.Context, _ service.Runner) error {
    for {
        syncBlockNumber, _ := retry.FunctionCall(ctx, kpr.blockSyncClient.BlockNumber)
        smobserver.SyncAppWithDB(ctx, kpr.shuttermintClient, kpr.dbpool, kpr.shuttermintState)
        kpr.dbpool.BeginFunc(ctx, func(tx pgx.Tx) error {
            return kpr.handleOnChainChanges(ctx, tx, syncBlockNumber)
        })
        fx.SendShutterMessages(ctx, database.New(kpr.dbpool), &kpr.messageSender)
        time.After(2 * time.Second)
    }
}
```

Her 2 saniyede:
1. L1 güncel blok numarasını al
2. Shuttermint'i DB ile senkronize et (yeni eventi işle)
3. L1 değişikliklerini işle (yeni BatchConfig gönder, BlockSeen gönder)
4. Bekleyen Shuttermint mesajlarını gönder

### 5.4 handleOnChainKeyperSetChanges

```go
func (kpr *KeyperCore) handleOnChainKeyperSetChanges(
    ctx context.Context, tx pgx.Tx, blockNumber uint64,
) error
```

1. `GetLatestBatchConfig` → mevcut en son config
2. `GetKeyperSetByKeyperConfigIndex(nextIndex)` → yeni keyper seti var mı?
3. `validateBatchConfig` → geçerlilik kontrolü
4. `activationBlockNumber - blockNumber > DKGStartBlockDelta` → henüz çok erken mi?
5. `ScheduleShutterMessage(NewBatchConfig(...))` → Shuttermint'e gönder

### 5.5 LinkConfigToDB

```go
func LinkConfigToDB(ctx context.Context, config *kprconfig.Config, dbpool *pgxpool.Pool) error
```

İlk kullanımda Ethereum adresini DB'ye yazar. Sonraki kullanımlarda DB'deki ile karşılaştırır. Farklıysa hata: **aynı DB'yi farklı keyper kullanamaz.**

---

## 6. EpochKG — Epoch Anahtar Üretimi

`keyper/epochkg/epochkg.go`

### 6.1 EpochKG Yapısı

```go
type EpochKG struct {
    Eon             uint64
    NumKeypers      uint64
    Threshold       uint64
    Keyper          KeyperIndex        // bu keyper'in indeksi
    SecretKeyShare  *shcrypto.EonSecretKeyShare  // bu keyper'in DKG payı
    PublicKey       *shcrypto.EonPublicKey        // eon public key
    PublicKeyShares []*shcrypto.EonPublicKeyShare // her keyper'in public payı

    SecretShares map[string][]*EpochSecretKeyShare // bekleyen paylar
    SecretKeys   map[string]*shcrypto.EpochSecretKey // tamamlanan anahtarlar
}
```

Map key'i: `identityPreimage.Hex()` — her epoch (TX identity) için ayrı anahtar.

### 6.2 Pay Hesaplama

```go
func (epochkg *EpochKG) ComputeEpochSecretKeyShare(
    identityPreimage identitypreimage.IdentityPreimage,
) *shcrypto.EpochSecretKeyShare {
    epochID := shcrypto.ComputeEpochID(identityPreimage.Bytes())
    return shcrypto.ComputeEpochSecretKeyShare(epochkg.SecretKeyShare, epochID)
}
```

- `ComputeEpochID` → identityPreimage'ı BLS12-381 G1 noktasına hash-to-curve
- `ComputeEpochSecretKeyShare` → `skShare_i * epochID` (G1 skaler çarpımı)

### 6.3 Pay Biriktirme ve Anahtar Üretimi

```go
func (epochkg *EpochKG) HandleEpochSecretKeyShare(
    share *EpochSecretKeyShare,
) error
```

Akış:
1. `SecretKeys`'te zaten tamamlanmış mı? → erken çıkış
2. `VerifyEpochSecretKeyShare(share, publicKeyShares[sender], epochID)` → ZK doğrulama
3. `addEpochSecretKeyShare(share)` → biriktir
4. `len(shares) == threshold` → `computeEpochSecretKey(shares)` çağır
5. `SecretShares`'ten sil, `SecretKeys`'e ekle

```go
func (epochkg *EpochKG) computeEpochSecretKey(
    shares []*EpochSecretKeyShare,
) (*shcrypto.EpochSecretKey, error) {
    // Lagrange interpolasyonu ile birleştirme
    return shcrypto.ComputeEpochSecretKey(keyperIndices, epochSecretKeyShares, epochkg.Threshold)
}
```

**Kritik:** Duplicate pay kontrolü — aynı sender iki kez pay gönderirse hata.

---

## 7. epochkghandler — Şifre Çözme Tetikleyicisi

### 7.1 KeyShareHandler — Trigger → Key Share

`keyper/epochkghandler/service.go` + `sendkeyshare.go`

```go
type KeyShareHandler struct {
    InstanceID           uint64
    KeyperAddress        common.Address
    MaxNumKeysPerMessage uint64
    DBPool               *pgxpool.Pool
    Messaging            p2p.Messaging
    Trigger              <-chan *broker.Event[*DecryptionTrigger]
}
```

`handleEvent` akışı:
```
DecryptionTrigger geldi
├── getEonForBlockNumber() → DB'den eon bul
├── ConstructDecryptionKeyShares()
│   ├── GetKeyperIndex() → bu keyper'in indeksi
│   ├── ExistsDecryptionKeyShare() → zaten gönderdik mi?
│   ├── GetDKGResult() → başarılı DKG var mı?
│   ├── NewEpochKG(pureDKGResult)
│   └── ComputeEpochSecretKeyShare(identityPreimage) × n
└── Messaging.SendMessage(DecryptionKeyShares, retry×4, 200ms aralık)
```

### 7.2 DecryptionKeyShareHandler — Pay Doğrulama ve Biriktirme

`keyper/epochkghandler/keyshare.go`

**ValidateMessage:**
```go
func (handler *DecryptionKeyShareHandler) ValidateMessage(
    ctx context.Context, msg p2pmsg.Message,
) (pubsub.ValidationResult, error)
```

1. InstanceID kontrolü
2. Bu keyper'in bu eon'da görevli olup olmadığı kontrolü
3. Başarılı DKG sonucu var mı?
4. `checkKeyShares()`:
   - Her pay için `VerifyEpochSecretKeyShare(...)` → geçerli mi?
   - Paylar sıralı mı? (`bytes.Compare(a, b) < 0`)

**HandleMessage:**
```go
func (handler *DecryptionKeyShareHandler) HandleMessage(
    ctx context.Context, msg p2pmsg.Message,
) ([]p2pmsg.Message, error)
```

1. `InsertDecryptionKeySharesMsg(ctx, msg)` → DB'ye kaydet
2. Tüm anahtarlar zaten var mı? → nil döndür
3. `aggregateDecryptionKeySharesFromDB(...)` → DB'den tüm payları çek
4. EpochKG ile birleştir → `DecryptionKey` oluştu mu?
5. `InsertDecryptionKeysMsg(ctx, message)` → DB'ye kaydet
6. `[]p2pmsg.Message{message}` → yayınla

### 7.3 DecryptionKeyHandler — Tam Anahtar Doğrulama

`keyper/epochkghandler/key.go`

**ValidateMessage:**
1. InstanceID kontrolü
2. Bu keyper'in bu eon'da görevli olup olmadığı
3. Başarılı DKG var mı?
4. `checkKeysErrors()`:
   - Her anahtar için `VerifyEpochSecretKey(key, publicKey, identity)` → BLS pairing
   - Anahtarlar sıralı mı?

**HandleMessage:**
```go
return nil, database.New(handler.dbpool).InsertDecryptionKeysMsg(ctx, key)
```
Sadece DB'ye kaydeder. Sequencer HTTP ile okur.

---

## 8. smobserver — Shuttermint Durum Gözlemcisi

`keyper/smobserver/smstate.go`

### 8.1 ShuttermintState

```go
type ShuttermintState struct {
    config       Config
    synchronized bool
    isKeyper     bool
    dkg          map[uint64]*ActiveDKG  // eon → aktif DKG
    phaseLength  PhaseLength
}

type ActiveDKG struct {
    pure        *puredkg.PureDKG
    startHeight int64
    dirty       bool
    keypers     []common.Address
}
```

### 8.2 HandleEvent — Event Dispatch

```go
func (st *ShuttermintState) HandleEvent(
    ctx context.Context, queries *database.Queries,
    event shutterevents.IEvent,
) error
```

| Event | Handler | Açıklama |
|-------|---------|----------|
| `CheckIn` | `handleCheckIn` | ECIES public key'i DB'ye kaydet |
| `BatchConfig` | `handleBatchConfig` | Bu keyper'se CheckIn mesajı gönder, config'i kaydet |
| `BatchConfigStarted` | `handleBatchConfigStarted` | Started flag güncelle |
| `EonStarted` | `handleEonStarted` | PureDKG başlat, Dealing fazına geç |
| `PolyCommitment` | `handlePolyCommitment` | `dkg.pure.HandlePolyCommitmentMsg` |
| `PolyEval` | `handlePolyEval` | ECIES decrypt + `HandlePolyEvalMsg` |
| `Accusation` | `handleAccusation` | `HandleAccusationMsg` |
| `Apology` | `handleApology` | `HandleApologyMsg` |

### 8.3 shiftPhase — Faz Geçişi

```go
func (st *ShuttermintState) shiftPhase(
    ctx context.Context, queries *database.Queries,
    height int64, eon uint64, dkg *ActiveDKG,
) error
```

Mevcut blok yüksekliğine göre hangi fazda olduğu hesaplanır ve gerekli fazlar sırayla geçilir:

```
Off → startPhase1Dealing()  → PolyCommitment + PolyEval mesajları gönder
    → startPhase2Accusing() → Accusation mesajları gönder
    → startPhase3Apologizing() → Apology mesajları gönder
    → finalizeDKG()         → ComputeResult() + DKGResult mesajı gönder
```

### 8.4 sendPolyEvals — ECIES Şifreleme

```go
func (st *ShuttermintState) sendPolyEvals(
    ctx context.Context, queries *database.Queries,
) error
```

Her poly eval:
1. Alıcının ECIES public key'ini DB'den al (CheckIn'den kaydedilmiş)
2. `ecies.Encrypt(rand.Reader, pubkey, eval, nil, nil)` → şifrele
3. `shmsg.NewPolyEval(eon, receivers, encryptedEvals)` → Shuttermint'e gönder

**Kritik:** P2P'de giden poly eval mesajları şifreli! Sadece alıcı keyper okuyabilir.

### 8.5 finalizeDKG

```go
func (st *ShuttermintState) finalizeDKG(
    ctx context.Context, queries *database.Queries,
    eon uint64, dkg *ActiveDKG,
) error
```

1. `dkg.pure.Finalize()` — PureDKG sonuçlandır
2. `dkg.pure.ComputeResult()` — `EonSecretKeyShare` + `EonPublicKey` + `PublicKeyShares`
3. Başarılıysa:
   - `InsertEonPublicKey` → DB'ye kaydet
   - `EncodePureDKGResult` → seri hale getir
4. `InsertDKGResult(Eon, Success, Error, PureResult)` → DB'ye kaydet
5. `ScheduleShutterMessage(DKGResult, success/fail)` → Shuttermint'e oy gönder
6. `DeletePureDKG` + `DeletePolyEvalByEon` → temizlik

---

## 9. P2P Katmanı — libp2p + GossipSub

### 9.1 P2PNode Yapısı

```go
type P2PNode struct {
    config      p2pNodeConfig
    host        host.Host          // libp2p host
    dht         *dht.IpfsDHT      // Kad-DHT peer discovery
    discovery   *routing.RoutingDiscovery
    pubSub      *pubsub.PubSub    // GossipSub
    gossipRooms map[string]*gossipRoom  // topic → room
    GossipMessages chan *pubsub.Message
}
```

### 9.2 Host Yapılandırması

```go
options := []libp2p.Option{
    libp2p.Identity(&config.PrivKey.Key),  // libp2p private key
    libp2p.ListenAddrs(config.ListenAddrs...),
    libp2p.UserAgent("shutter-network/<version>"),
    libp2p.ConnectionManager(connectionManager),  // low=peerLow, high=peerHigh
    libp2p.ProtocolVersion("/shutter/0.1.0"),
    libp2p.EnableRelay(),
    libp2p.Ping(true),
    // Mainnet için:
    libp2p.EnableNATService(),
    libp2p.NATPortMap(),  // uPNP
    libp2p.EnableAutoRelayWithStaticRelays(bootstrapPeers),
}
```

### 9.3 PubSub Yapılandırması

```go
pubsub.NewGossipSub(ctx, p2pHost,
    pubsub.WithGossipSubParams(*gossipSubParams),
    pubsub.WithPeerScore(peerScoreParams, peerScoreThresholds),
    pubsub.WithDiscovery(discovery),
    pubsub.WithPeerExchange(true),  // bootstrap node'da
)
```

### 9.4 Mesaj İşleme Pipeline

```
GossipSub → gossipRoom.readLoop() → P2P.GossipMessages chan
    → P2PMessaging.runHandleMessages()
        → UnmarshalPubsubMessage() → p2pmsg.Message
        → ValidatorRegistry[topic](msg)  → Accept/Reject/Ignore
        → HandlerRegistry[messageType](msg) → []p2pmsg.Message
        → SendMessage(outMsgs...)
```

### 9.5 ValidatorRegistry

```go
type ValidatorRegistry map[string][]pubsub.ValidatorEx
// topic → validator fonksiyonları listesi

func (r *ValidatorRegistry) GetCombinedValidator(topic string) pubsub.ValidatorEx
```

Tüm validator'lar AND mantığıyla çalışır:
- Herhangi biri Reject → mesaj reddedilir
- Herhangi biri Ignore (Reject yok) → mesaj yoksayılır
- Hepsi Accept → mesaj kabul edilir

### 9.6 Peer Score

GossipSub peer scoring etkin:
- Kötü mesaj gönderen peer'lar cezalandırılır
- Peer exchange (PX) bootstrap node'lar için aktif
- FloodSub peer discovery opsiyonel

---

## 10. Mesaj Yapıları — p2pmsg Protobuf

`p2pmsg/gossip.proto`

### 10.1 DecryptionTrigger

```proto
message DecryptionTrigger {
    uint64 instance_id      = 1;  // hangi shutter instance'ı
    bytes  identity_preimage = 2;  // hangi epoch için
    uint64 block_number     = 3;  // tetikleyen L1 bloğu
    bytes  transactions_hash = 4; // şifreli TX'lerin hash'i
    bytes  signature        = 5;  // sequencer imzası
}
```

### 10.2 DecryptionKeyShares

```proto
message DecryptionKeyShares {
    uint64 instance_id   = 1;
    uint64 eon           = 4;  // hangi eon (DKG instance)
    uint64 keyper_index  = 5;  // gönderen keyper'in indeksi
    repeated KeyShare shares = 9;  // her epoch için bir pay
    oneof extra {                  // hangi rollup tipi
        GnosisDecryptionKeySharesExtra gnosis   = 10;
        OptimismDecryptionKeySharesExtra optimism = 11;
        ShutterServiceDecryptionKeySharesExtra service = 12;
    }
}

message KeyShare {
    bytes identity_preimage = 1;  // hangi epoch
    bytes share             = 2;  // marshal edilmiş EpochSecretKeyShare
}
```

### 10.3 DecryptionKeys

```proto
message DecryptionKeys {
    uint64 instance_id = 1;
    uint64 eon         = 2;
    repeated Key keys  = 3;
    oneof extra { ... }
}

message Key {
    bytes identity_preimage = 1;
    bytes key               = 2;  // marshal edilmiş EpochSecretKey
}
```

### 10.4 EonPublicKey

```proto
message EonPublicKey {
    uint64 instance_id         = 1;
    bytes  public_key          = 2;  // marshal edilmiş EonPublicKey
    uint64 activation_block    = 3;
    uint64 keyper_config_index = 6;
    uint64 eon                 = 7;
    bytes  signature           = 5;  // keyper imzası
}
```

### 10.5 Envelope

```proto
message Envelope {
    string version          = 1;  // "0.0.1"
    google.protobuf.Any message = 2;
    optional TraceContext trace = 3;  // OpenTelemetry
}
```

Tüm P2P mesajları `Envelope` içinde gönderilir. `UnmarshalNew()` ile tip güvenli parse.

### 10.6 İmzalama

```go
// p2pmsg/signing.go
func Sign(s Signable, privKey *ecdsa.PrivateKey) error
func RecoverAddress(s Signable) (common.Address, error)
func VerifySignature(s Signable, address common.Address) (bool, error)
```

Ethereum ECDSA imzası (secp256k1). `ethcrypto.Sign(s.Hash(), privKey)`.

---

## 11. IdentityPreimage ve EpochID

`medley/identitypreimage/identitypreimage.go`

```go
type IdentityPreimage []byte

// Dönüşümler
func BigToIdentityPreimage(n *big.Int) IdentityPreimage
func Uint64ToIdentityPreimage(n uint64) IdentityPreimage
func HexToIdentityPreimage(n string) (IdentityPreimage, error)

// Metodlar
func (e IdentityPreimage) Bytes() []byte
func (e IdentityPreimage) Big() *big.Int
func (e IdentityPreimage) Uint64() uint64
func (e IdentityPreimage) Hex() string  // "0x..."
```

**Kritik kavram:** IdentityPreimage ham bayttır. `shcrypto.ComputeEpochID(preimage)` bunu BLS12-381 G1 noktasına dönüştürür (hash-to-curve).

**Kullanım örnekleri:**
- Gnosis Beacon Chain: slot numarası → `Uint64ToIdentityPreimage(slot)`
- Optimism: blok numarası
- ShutterService: TX hash veya özel identity

**Aynı identity preimage** → aynı epoch → aynı decryption key. Bu, bir bloktaki tüm TX'lerin aynı anahtarla şifrelenmesine olanak tanır.

---

## 12. Tam İşlem Akışı (Pipeline)

### 12.1 DKG Başlatma (Tek seferlik, yeni keyper seti için)

```
1. L1 on-chain: KeyperSet güncellemesi
2. Keyper: handleOnChainKeyperSetChanges()
   → ScheduleShutterMessage(BatchConfig)
   → Shuttermint'e TX gönder
3. Shuttermint: deliverBatchConfig()
   → ConfigVoting threshold'a ulaşırsa:
   → addConfig(), StartDKG(), EonStarted eventi
4. Keyper: handleEonStarted()
   → NewPureDKG() başlat
   → startPhase1Dealing():
     → PolyCommitment gönder (broadcast, Shuttermint TX)
     → PolyEval'ları ECIES ile şifrele, gönder (P2P encrypted)
5. Diğer keyperler: HandlePolyCommitmentMsg, HandlePolyEvalMsg
6. Accusing, Apologizing fazları
7. Finalize: DKG sonucu DB'ye kaydet, DKGResult Shuttermint'e gönder
8. EonPublicKey P2P'de yayınla → sequencer bunu kullanacak
```

### 12.2 TX Şifreleme (Kullanıcı tarafı)

```
Kullanıcı:
1. eonPublicKey'i al (EonPublicKey mesajından)
2. epochID belirle (hangi bloğa ait?)
   → Gnosis: slot numarası
   → Optimism: blok numarası
3. sigma = random 32 byte
4. encryptedTX = shcrypto.Encrypt(tx, eonPublicKey, epochID, sigma)
5. encryptedTX'i sequencer'a gönder
```

### 12.3 Şifre Çözme (Blok kapandıktan sonra)

```
Sequencer/Collator:
1. Blok tamamlandı
2. DecryptionTrigger{epochID, blockNumber, txHash, sig} gönder

Keyperler (paralel):
3. DecryptionTrigger alındı → ConstructDecryptionKeyShares()
   → EpochKG.ComputeEpochSecretKeyShare(identityPreimage)
   → DecryptionKeyShares P2P'de yayınla (retry×4)

Herhangi bir keyper (threshold toplandığında):
4. ValidateMessage: her share için VerifyEpochSecretKeyShare
5. HandleMessage: aggregateDecryptionKeySharesFromDB()
   → EpochKG.HandleEpochSecretKeyShare() × n
   → threshold dolunca: ComputeEpochSecretKey()
   → DecryptionKeys P2P'de yayınla

Sequencer:
6. DecryptionKeys alındı
7. VerifyEpochSecretKey(key, eonPublicKey, identity) → doğrula
8. encryptedTX.Decrypt(epochSecretKey) → orijinal TX
9. TX'i sırasıyla uygula
```

---

## 13. Keyper Implementasyonları

`keyperimpl/` altında farklı rollup'lar için özelleştirilmiş keyper'lar:

### 13.1 Gnosis Keyper

`keyperimpl/gnosis/`

- **Tetikleyici:** Beacon Chain slot'ları
- `newslot.go` → her yeni slot'ta işlem yap
- `sequencersyncer.go` → Gnosis sequencer ile senkronizasyon
- `validatorsyncer.go` → validator seti takibi
- Ekstra payload: `GnosisDecryptionKeySharesExtra{slot, tx_pointer, signature}`
- SSZ tip tanımları: `gnosisssztypes/`

### 13.2 Optimism Keyper

`keyperimpl/optimism/`

- **Tetikleyici:** Optimism blok numaraları
- `OptimismDecryptionKeySharesExtra{}` — boş extra

### 13.3 ShutterService Keyper

`keyperimpl/shutterservice/`

- Genel amaçlı keyper servisi
- `ShutterServiceDecryptionKeySharesExtra{signer_indices, signatures}`

### 13.4 Primev Keyper

`keyperimpl/primev/`

- MEV-Commit entegrasyonu
- `Commitment` mesajı desteği

---

## 14. Güvenlik Analizi

### 14.1 Güçlü Yanlar

| Özellik | Detay |
|---------|-------|
| **MEV koruması** | TX içeriği blok kapanmadan bilinmez — matematiksel garanti |
| **Threshold güvenliği** | n-1 keyper tehlikeye girse bile şifre çözülmez |
| **Pay doğrulama** | `VerifyEpochSecretKeyShare` — pairing tabanlı, ZK-siz doğrulama |
| **Tam anahtar doğrulama** | `VerifyEpochSecretKey` — sequencer sahte anahtar kullanamaz |
| **Suçlama mekanizması** | Hatalı PolyEval gönderenler tespit edilir |
| **Replay koruması** | `NonceTracker` (Shuttermint TX'leri için) |
| **Config değişikliği** | Threshold oyu gerektiriyor |
| **ECIES şifreleme** | PolyEval'lar şifreli (keyper-to-keyper) |
| **DB bağlama** | Ethereum adresi DB'ye bağlı, yanlış keyper aynı DB'yi kullanamaz |

### 14.2 Potansiyel Zayıf Noktalar

| Zayıflık | Açıklama |
|----------|----------|
| **Shuttermint merkezi** | Shuttermint sidechain başarısız olursa DKG duruyor |
| **Go GC bellek sorunu** | `zeroize` yoktur — secretKeyShare Go GC tarafından silinmeyebilir |
| **Timing saldırısı** | 2 saniyelik poll döngüsü tahmin edilebilir — timing leak |
| **ECIES yerine IBE-tabanlı** | CheckIn'de ECDH/ECIES yerine shcrypto kullanmak daha tutarlı olurdu |
| **Tendermint bağımlılığı** | v0.37.0-rc2 — eski RC sürümü |
| **Sequencer merkezi** | Decryption trigger'ı sequencer gönderiyor — sequencer checklanmıyor |
| **Bootstrap peer listesi boş** | `DefaultBootstrapPeers = []` — mainnet için henüz doldurulmamış |
| **gob serileştirme** | ShutterApp gob ile serialize — type-safety zayıf |

### 14.3 Censorship Resistance (Sansür Direnci)

- Şifreli TX mempool'da: sequencer içeriği göremez → sansür zorlaşır
- Ama: sequencer TX'i hiç dahil etmeyebilir → inside-censorship hâlâ mümkün
- Çözüm: force-inclusion mekanizması (L1 forced inclusion — Optimism tarzı) gerekli

---

## 15. Nova Shield İçin Çıkarımlar

### 15.1 Şifreli Mempool Tasarımı

Rolling Shutter'ın BLS12-381 IBE şeması Nova Shield için **doğrudan ilham kaynağı:**

```
Nova Shield Şifreli Mempool Tasarımı:
- EonPublicKey → Guardian Public Key (Pedersen DKG ile üretilen)
- EpochID → TX'in hedef bloğunun hash'i veya nonce'u
- Encrypt(tx, guardianKey, epochID, sigma) → şifreli TX
- Threshold decrypt → guardian'ların key share'leri topla
- Decrypt(encTx, epochSecretKey) → orijinal TX
```

### 15.2 DKG Protokolü Seçimi

Rolling Shutter **Pedersen DKG** kullanıyor (Feldman VSS tabanlı). Nova Shield için:

| Kriter | Rolling Shutter | Nova Shield (öneri) |
|--------|----------------|---------------------|
| DKG protokolü | Pedersen | FROST veya GJKR21 |
| İmza şeması | BLS (IBE) | CGGMP21 (ECDSA) |
| Eşik | t-of-n | t-of-n |
| Suçlama | Accusation/Apology | Blame protocol |
| Faz gecikmesi | Blok bazlı | Round bazlı |

**Öneri:** Nova Shield'in MPC hesap yönetimi için CGGMP21, şifreli mempool için Pedersen + shcrypto IBE — iki ayrı DKG altyapısı.

### 15.3 P2P Katmanı

Rolling Shutter'ın P2P mimarisi **Nova Shield için alınabilecek en iyi tasarım:**

```go
// Alınacak pattern:
type MessageHandler interface {
    ValidateMessage(ctx, msg) (ValidationResult, error)
    HandleMessage(ctx, msg) ([]Message, error)
    MessagePrototypes() []Message
}
// Avantaj: validator + handler birlikte → topic bağlama otomatik
```

**Nova Shield P2P:**
- libp2p + GossipSub (Rolling Shutter'dan doğrudan)
- Protobuf mesaj katmanı (Rolling Shutter'dan doğrudan)
- Peer scoring (kötü node'ları cezalandır)
- Konular: `GuardianKeyShare`, `GuardianKey`, `TxCipher`, `DecryptionTrigger`

### 15.4 Faz Zamanlaması

```go
// Rolling Shutter yaklaşımı — Nova Shield'e adapte et:
type PhaseLength struct {
    dealing     int64  // blok sayısı
    accusing    int64
    apologizing int64
}
// GetPhaseAtHeight(height, eonStartHeight) → Phase
```

**Nova Shield:** DKG fazlarını block height'a bağla, runtime'da konfigüre edilebilir yap.

### 15.5 Kimlik Doğrulama (IdentityPreimage)

Rolling Shutter'ın `IdentityPreimage` soyutlaması mükemmel:
- Farklı rollup tipleri için aynı şifreleme motoru
- Block number, slot, TX hash — hepsi `[]byte`

**Nova Shield:** `TxIdentity = BLAKE3(blockHash || txIndex)` olarak tanımla.

### 15.6 Suçlama Mekanizması

**Rolling Shutter'da eksik:** DKG suçlama var ama epoch key share'i kasıtlı yanlış gönderenlere karşı ceza mekanizması yok.

**Nova Shield'e ekle:**
- Yanlış key share → slashing (staking tokeni kesilir)
- Onchain kanıt: `VerifyEpochSecretKeyShare` başarısız → slashing delili

### 15.7 Decoder Sırası (Keyper Önceliği)

**Rolling Shutter'da:** İlk threshold'a ulaşan keyper `DecryptionKeys` yayınlar.

**Nova Shield:** Round-robin veya deterministic seçim ile "primary decoder" belirle. Primary decoder hatalıysa backup. Daha verimli.

### 15.8 Bellek Güvenliği

Rolling Shutter'da `zeroize` yok → Go GC'nin hassas veriyi silememe riski.

**Nova Shield:** Rust kullanıyoruz → `zeroize` crate → `EonSecretKeyShare`, `EpochSecretKey` sıfırlanır.

### 15.9 Sequencer Trust Sorunu

Rolling Shutter'da sequencer `DecryptionTrigger` gönderir ve bu mesaj doğrusal sıralamayı belirler. Güven gerektirir.

**Nova Shield:** Guardian Node'lar (keyper eşdeğeri) DecryptionTrigger'ı çoğunluk oyuyla tetiklesin → sequencer tek başına kontrol etmesin.

### 15.10 Uygulamaya Doğrudan Kopyalanabilecek Parçalar

| Bileşen | Kopyalanabilirlik |
|---------|-------------------|
| `IdentityPreimage` soyutlaması | Doğrudan (Rust'a port) |
| P2P mesaj envelope/topic yapısı | Doğrudan (Protobuf) |
| `ValidatorRegistry` + `HandlerRegistry` | Doğrudan (Rust trait'e dönüştür) |
| `EpochKG.HandleEpochSecretKeyShare` mantığı | Doğrudan (eşik biriktirme) |
| DKG faz zamanlaması (block-height tabanlı) | Doğrudan |
| `VerifyEpochSecretKeyShare` pairing kontrolü | Doğrudan (BLS12-381) |
| Nonce tracker (replay koruması) | Doğrudan |
| `PhaseLength` konfigürasyonu | Doğrudan |

---

## Özet Tablo

| Bileşen | Teknoloji | Nova Shield Önemi |
|---------|-----------|------------------|
| Şifreleme | BLS12-381 IBE (shcrypto) | Şifreli mempool için temel |
| DKG | Pedersen + Feldman VSS | Guardian key üretimi için referans |
| Sidechain | Tendermint ABCI | DKG koordinasyonu için pattern |
| P2P | libp2p + GossipSub | Doğrudan alınabilir |
| Mesajlaşma | Protobuf + Envelope | Doğrudan alınabilir |
| Doğrulama | Pairing tabanlı | ZK yerine verimli alternatif |
| Suçlama | Accusation/Apology | Blame protocol'e ilham |
| Faz yönetimi | Block-height tabanlı | Deterministik zaman yönetimi |

**Ana çıkarım:** Rolling Shutter, şifreli mempool için bugüne kadar yapılmış en iyi prodüksiyon implementasyonlarından biridir. Nova Shield, kriptografik çekirdeği (shcrypto IBE) ve P2P katmanını bu projeden ilham alarak Rust'ta implement etmeli; CGGMP21 ECDSA ile IBE şifrelemesini aynı guardian infrastructure üzerinde birleştirmelidir.
