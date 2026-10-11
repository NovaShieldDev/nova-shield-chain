# Repo 7: avalanchego — Kapsamlı Analiz

**Kaynak:** `research/chains/avalanchego/`
**Dil:** Go 1.25
**Yazar:** Ava Labs
**Lisans:** BSD-3-Clause
**Analiz Tarihi:** 2026-07-17

---

## İçindekiler

1. [Genel Mimari — 3 Zincir Modeli](#1-genel-mimari)
2. [go.mod — Bağımlılıklar ve Araçlar](#2-gomod)
3. [Snowball Consensus — Temel Primitifler](#3-snowball-consensus)
4. [Snowball Parametreleri ve Güvenlik Koşulları](#4-snowball-parametreleri)
5. [Snowflake — Güven Sayacı Mekanizması](#5-snowflake)
6. [Snowball — Kümülatif Tercih Gücü](#6-snowball)
7. [Tree Consensus — Patricia Ağacı Üzerinde Snowball](#7-tree-consensus)
8. [Snowman Topological — Kahn Sıralamalı Blok Oylama](#8-snowman-topological)
9. [Snow Engine — Blok İşleme ve Polling](#9-snow-engine)
10. [voter.go — Chit Uygulaması ve Bağımlılık Zinciri](#10-voter)
11. [PlatformVM (P-Chain) — Validator ve Staking](#11-platformvm)
12. [Ödül Hesaplama — Reward Calculator](#12-reward-calculator)
13. [Block Tipleri: Apricot vs Banff](#13-block-tipleri)
14. [PlatformVM Blok Doğrulama — verifier.go](#14-block-verifier)
15. [PlatformVM Blok Kabulü — acceptor.go](#15-block-acceptor)
16. [Ağ Katmanı — network.go](#16-network)
17. [Chain Manager — Zincir Yönetimi](#17-chain-manager)
18. [Simplex — Yeni BFT Consensus Alternatifi](#18-simplex)
19. [Uptime Mekanizması — Validator Ödül Koşulu](#19-uptime)
20. [Warp Messaging — Zincirlerarası İletişim](#20-warp-messaging)
21. [Nova Shield için Çıkarımlar](#21-nova-shield-cikarimlar)
22. [Özet Karşılaştırma Tablosu](#22-ozet)

---

## 1. Genel Mimari

Avalanche, **üç zincirli** bir birincil ağ üzerine kuruludur:

```
Primary Network
├── P-Chain (PlatformVM)   — Validator yönetimi, subnet/chain oluşturma
├── X-Chain (AVM)          — UTXO tabanlı varlık transferleri
└── C-Chain (coreth/EVM)   — EVM uyumlu akıllı kontrat zinciri
```

Her zincir ayrı bir **Virtual Machine (VM)** instance'ı çalıştırır. Zincirler arasındaki atomik transfer **SharedMemory** üzerinden gerçekleşir. Subnet, kendi VM'lerini çalıştıran bağımsız validator kümesidir.

### Temel Katman Ayrımı

```
snow/consensus/       — Saf algoritma (Snowball, Snowman, Simplex)
snow/engine/          — Consensus + P2P entegrasyonu
chains/               — Chain manager (zincir yaşam döngüsü)
vms/platformvm/       — P-Chain: validator, staking, subnet
vms/coreth/           — C-Chain: geth fork (EVM)
network/              — P2P bağlantı yönetimi
```

---

## 2. go.mod

**Go versiyonu:** 1.25.10

### Kritik Bağımlılıklar

```go
github.com/ava-labs/simplex          // Yeni BFT consensus (ava-labs'ın kendi geliştirmesi)
github.com/ava-labs/firewood-go-ethhash  // Firewood DB için Ethereum hash FFI
github.com/cockroachdb/pebble        // LSM-tree DB (consensus state)
k8s.io/client-go                     // Kubernetes entegrasyonu (cloud deployments)
github.com/supranational/blst        // BLS12-381 imzalar (Warp Messaging için)
```

**Önemli:** `ava-labs/simplex` yeni BFT consensus implementasyonu — Snowball'ın yanı sıra alternatif olarak mevcut. MaxNetworkDelay=5s ile PBFT benzeri kesinlik sağlıyor.

---

## 3. Snowball Consensus — Temel Primitifler

**Dosya:** `snow/consensus/snowball/consensus.go`

Snowball, 4 temel interface ile tanımlanır:

```go
// Genel (N'ary): ID tabanlı tercih
type Consensus interface {
    Add(newChoice ids.ID)
    Preference() ids.ID
    RecordPoll(votes bag.Bag[ids.ID]) bool
    RecordUnsuccessfulPoll()
    Finalized() bool
}

// N'ary: doğrudan count+choice ile
type Nnary interface {
    Add(newChoice ids.ID)
    Preference() ids.ID
    RecordPoll(count int, choice ids.ID)
    RecordUnsuccessfulPoll()
    Finalized() bool
}

// Binary: 0 veya 1 tercih
type Binary interface {
    Preference() int
    RecordPoll(count, choice int)
    RecordUnsuccessfulPoll()
    Finalized() bool
}

// Unary: tek seçenek, Binary'ye genişletilebilir
type Unary interface {
    RecordPoll(count int)
    RecordUnsuccessfulPoll()
    Finalized() bool
    Extend(alphaConfidence int) Binary   // Tek seçeneği binary hale getir
    Clone() Unary
}
```

### Algoritma Hiyerarşisi

```
Slush (en basit)
  └── Snowflake (confidence sayacı ekle)
        └── Snowball (preferenceStrength map ekle)
              └── Tree (patricia tree üzerinde çalış)

Her tip Binary/Nnary/Unary varyantı ile gelir.
```

---

## 4. Snowball Parametreleri

**Dosya:** `snow/consensus/snowball/parameters.go`

```go
var DefaultParameters = Parameters{
    K:                    20,   // Her poll için seçilen validator sayısı
    AlphaPreference:      15,   // Tercihi değiştirmek için gerekli minimum oy (K*0.75)
    AlphaConfidence:      15,   // Confidence artırmak için gerekli minimum oy
    Beta:                 20,   // Kesinlik için gereken ardışık başarılı poll sayısı
    ConcurrentRepolls:    4,    // Önceki round bitmeden başlatılabilecek yeni round sayısı
    OptimalProcessing:    10,   // İdeal işlem kuyruğu büyüklüğü
    MaxOutstandingItems:  256,  // Maksimum bekleyen öğe
    MaxItemProcessingTime: 30 * time.Second,
}
```

### Güvenlik Koşulları (Verify())

```
K/2 < AlphaPreference <= AlphaConfidence <= K
// Yani: 10 < 15 <= 15 <= 20  ✓

MinPercentConnectedHealthy = alphaConfidence/K * (1 - 0.2) + 0.2
// = 15/20 * 0.8 + 0.2 = 0.6 + 0.2 = 0.8  (ağın %80'i bağlı olmalı)
```

### Matematiksel Anlamı

- **K=20:** Her karar turu için 20 validator örneklenir
- **Alpha=15:** Bu 20'den en az 15'i aynı kararı vermeli (supermajority, %75)
- **Beta=20:** Bu supermajority 20 ardışık turda sağlanmalı → kesinlik
- Güvenlik: Byzantine validator sayısı < K - Alpha = 5 iken sistem güvenli

---

## 5. Snowflake — Güven Sayacı Mekanizması

**Dosya:** `snow/consensus/snowball/nnary_snowflake.go`

```go
type nnarySnowflake struct {
    nnarySlush                           // preference: tercih edilen ID
    alphaPreference int                  // tercih değişimi için minimum oy
    terminationConditions []terminationCondition  // {alphaConfidence, beta} çiftleri
    confidence []int                     // her terminationCondition için sayaç
    finalized bool
}
```

### RecordPoll Algoritması

```go
func (sf *nnarySnowflake) RecordPoll(count int, choice ids.ID) {
    if count < sf.alphaPreference {
        sf.RecordUnsuccessfulPoll()  // Yeterli oy yok → sıfırla
        return
    }

    if sf.preference != choice {
        sf.preference = choice      // Yeni tercih
        sf.confidence = sıfırla    // Confidence sıfırlanır!
    }

    for i, terminationCondition := range sf.terminationConditions {
        if count < terminationCondition.alphaConfidence {
            // Bu ve sonraki tüm confidence'ları sıfırla
            sf.confidence[i:] = sıfırla
            return
        }
        sf.confidence[i]++
        if sf.confidence[i] >= terminationCondition.beta {
            sf.finalized = true
            return
        }
    }
}

func (sf *nnarySnowflake) RecordUnsuccessfulPoll() {
    sf.confidence = sıfırla  // Tüm confidence sıfırlanır
}
```

### Kritik Nokta
Preference değiştiğinde confidence sıfırlanır. Bu, ağın farklı görüşler arasında gidip gelmesini engeller — bir tercih Beta kez üst üste supermajority almalıdır.

---

## 6. Snowball — Kümülatif Tercih Gücü

**Dosya:** `snow/consensus/snowball/nnary_snowball.go`

```go
type nnarySnowball struct {
    nnarySnowflake                     // Snowflake'i wrap eder
    preference ids.ID                  // En güçlü tercih
    maxPreferenceStrength int           // En yüksek preferenceStrength değeri
    preferenceStrength map[ids.ID]int  // Her tercih için kümülatif oy sayısı
}
```

### RecordPoll Algoritması

```go
func (sb *nnarySnowball) RecordPoll(count int, choice ids.ID) {
    if count >= sb.alphaPreference {
        // Kümülatif gücü artır
        preferenceStrength := sb.preferenceStrength[choice] + 1
        sb.preferenceStrength[choice] = preferenceStrength

        // Daha güçlü bir tercih ortaya çıktıysa güncelle
        if preferenceStrength > sb.maxPreferenceStrength {
            sb.preference = choice
            sb.maxPreferenceStrength = preferenceStrength
        }
    }
    // Snowflake mantığını da çalıştır (confidence, finalize)
    sb.nnarySnowflake.RecordPoll(count, choice)
}
```

### Snowflake vs Snowball Farkı

| Özellik | Snowflake | Snowball |
|---------|-----------|---------|
| Preference | Son başarılı tercih | Kümülatif en güçlü tercih |
| Preference değişimi | Anlık poll sonucuna göre | Tarihsel birikime göre |
| Güvenlik | Daha zayıf | Daha güçlü (liveness + safety) |
| Kullanım | İç bileşen | Tree/Topological'ın temeli |

---

## 7. Tree Consensus — Patricia Ağacı Üzerinde Snowball

**Dosya:** `snow/consensus/snowball/tree.go`

Tree, Consensus interface'ini implement eder ve ID bit'lerini kullanarak patricia ağacı üzerinde Snowball uygular.

### Temel Yapı

```go
type Tree struct {
    // Fabrika: yeni Nnary/Binary instance'ları oluşturur
    Factory SnowballFactory
    params  Parameters
    // Patricia tree kök düğümü
    node    node
    // shouldReset: başarısız poll sonrası lazy reset için
    shouldReset bool
}
```

### Node Tipleri

```go
// unaryNode: Tek çocuklu düğüm (prefix eşleşmesi)
type unaryNode struct {
    snowball Unary   // RecordPoll/Unary interface
    commonPrefix []uint    // Bu düğümün prefix'i
    child node
}

// binaryNode: İki çocuklu düğüm (0 veya 1 bit)
type binaryNode struct {
    snowball Binary  // RecordPoll/Binary interface
    bit uint         // Ayrım yapılan bit pozisyonu
    children [2]node // 0 ve 1 için ayrı ağaçlar
}
```

### shouldReset Optimizasyonu

Başarısız poll geldiğinde tüm ağaçta her düğümü sıfırlamak yerine, `shouldReset=true` flag'i set edilir. Bir sonraki `RecordPoll`'da bu flag kontrol edilerek lazy sıfırlama yapılır. Bu O(n) operasyonu O(1) amortize eder.

### Neden Tree?

Snowball N'ary, tüm seçenekleri flat map'te tutar. Tree, ID bit'lerine göre dallanarak:
- Daha az bellek kullanır (ortak prefix'ler paylaşılır)
- Daha hızlı poll işler (sadece ayrışan bitlere bakar)
- Özellikle çok sayıda fork'ta etkin çalışır

---

## 8. Snowman Topological — Kahn Sıralamalı Blok Oylama

**Dosya:** `snow/consensus/snowman/topological.go`

### Temel Veri Yapısı

```go
type Topological struct {
    // Factory: yeni Snowball tree instance'ı oluşturur
    Factory snowball.SnowballFactory
    params  snowball.Parameters

    // Aktif bloklar: blokID → snowmanBlock
    blocks map[ids.ID]*snowmanBlock

    // Tercih edilen blok ID'leri kümesi
    preferredIDs set.Set[ids.ID]

    // Her yükseklik için tercih edilen blok
    preferredHeights map[uint64]ids.ID

    // Genel tercih: en son kabul edilecek blok
    preference ids.ID

    // Kahn algoritması için geçici state
    kahnNodes map[ids.ID]kahnNode
    leaves    set.Set[ids.ID]

    lastAcceptedID     ids.ID
    lastAcceptedHeight uint64
}
```

### RecordPoll Akışı (4 Faz)

```
RecordPoll(votes bag.Bag[ids.ID])
    ↓
1. calculateInDegree()    — Her blok için in-degree hesapla
    ↓
2. pushVotes()            — Oy ağırlıklarını yapraktan köke doğru ilet
    ↓
3. vote()                 — Her blok için Snowball RecordPoll çağır
    ↓
4. acceptPreferredChild() — Kazanan bloğu kabul et, diğerlerini reddet
```

### Kahn Topolojik Sıralama

```go
type kahnNode struct {
    votes bag.Bag[ids.ID]  // Bu blok için toplanan oylar
    inDegree int            // Kaç çocuk bloğu var (henüz işlenmemiş)
}

// calculateInDegree: Yapraklar (in-degree=0) ile başla, kök'e kadar ilet
// pushVotes: Yaprak → parent yönünde oy topla
// vote: Her blok kendi Snowball'ına oy bildir
```

### acceptPreferredChild — Blok Kabulü

```go
func (ts *Topological) acceptPreferredChild(
    ctx context.Context, n *snowmanBlock,
) error {
    pref := n.sb.Preference()  // Snowball'ın tercih ettiği child

    if n.sb.Finalized() {
        // Kazanan: kabul et
        winner := n.children[pref]
        winner.Accept(ctx)

        // Diğer child'ları reddet
        for childID, child := range n.children {
            if childID != pref {
                child.Reject(ctx)
                ts.rejectTransitively(ctx, child)
            }
        }
    }
    return nil
}

// rejectTransitively: Reddedilen bloğun tüm torunlarını da reddet
func (ts *Topological) rejectTransitively(ctx context.Context, blk snowman.Block) {
    // BFS/DFS ile tüm alt ağacı reddet
}
```

### Complexity

```
RecordPoll Runtime: O(4 * |live set| + |votes|)
RecordPoll Space:   O(2 * |live set| + |votes|)
```

---

## 9. Snow Engine — Blok İşleme ve Polling

**Dosya:** `snow/engine/snowman/engine.go`

### Engine Yapısı

```go
type Engine struct {
    Config
    *metrics

    // NoOp handlers (kullanılmayan mesaj tipleri için)
    common.StateSummaryFrontierHandler
    common.AcceptedStateSummaryHandler
    // ...

    requestID uint32

    // Aktif poll'lar
    polls poll.Set

    // Bekleyen blok istekleri: request → blokID
    blkReqs *bimap.BiMap[common.Request, ids.ID]

    // Bağımlılıkları bekleyen bloklar: blokID → Block
    pending map[ids.ID]snowman.Block

    // Atalarına ulaşılamayan bloklar
    unverifiedIDToAncestor ancestor.Tree

    // Doğrulama başarısız olan bloklar (64 MiB cache)
    unverifiedBlockCache cache.Cacher[ids.ID, snowman.Block]

    // Diğer validator'ların son kabul ettiği sınır
    acceptedFrontiers tracker.Accepted

    // Bağımlılık tabanlı iş zamanlayıcı
    blocked *job.Scheduler[ids.ID]

    // İnşa edilecek blok sayacı
    pendingBuildBlocks int
}
```

### Start() — Engine Başlangıcı

```go
func (e *Engine) Start(ctx context.Context, startReqID uint32) error {
    lastAcceptedID, _ := e.VM.LastAccepted(ctx)
    lastAccepted, _ := e.VM.GetBlock(ctx, lastAcceptedID)

    // Consensus'ı son kabul edilen blok ile başlat
    e.Consensus.Initialize(e.Ctx, e.Params, lastAcceptedID,
                           lastAcceptedHeight, lastAccepted.Timestamp())

    // Oracle blok ise option'larını issue et (ProposerVM için)
    if oracleBlk, ok := lastAccepted.(snowman.OracleBlock); ok {
        options, _ := oracleBlk.Options(ctx)
        for _, blk := range options {
            e.deliver(ctx, e.Ctx.NodeID, blk, false, issuedMetric)
        }
    }

    e.Ctx.State.Set(snow.EngineState{
        Type:  p2p.EngineType_ENGINE_TYPE_CHAIN,
        State: snow.NormalOp,
    })
    e.VM.SetState(ctx, snow.NormalOp)
    return e.executeDeferredWork(ctx)
}
```

### Poll Döngüsü

```
PullQuery geldi (nodeID, requestID, blkID, requestedHeight)
    ↓
sendChits()      — Kendi tercihini döndür (preferredID, preferredIDAtHeight)
    ↓
issueFromByID()  — blkID'yi consensus'a ekle (yoksaATAlarını iste)
    ↓
executeDeferredWork()

---

Chits geldi (nodeID, requestID, preferredID, preferredIDAtHeight, acceptedID)
    ↓
issueFromByID(preferredID)   — Tercih edilen bloğu getir
issueFromByID(preferredIDAtHeight)  — Yükseklik bazlı tercihi getir
    ↓
voter oluştur (responseOptions = [preferredID, preferredIDAtHeight])
    ↓
blocked.Schedule(voter, deps...)  — Bağımlılıklar çözülünce çalıştır
```

### Put() — Blok Alma

```go
func (e *Engine) Put(ctx context.Context, nodeID ids.NodeID,
                     requestID uint32, blkBytes []byte) error {
    blk, err := e.VM.ParseBlock(ctx, blkBytes)
    if err != nil {
        return e.GetFailed(ctx, nodeID, requestID)  // Parse başarısız
    }

    // Beklenen blok mu?
    req := common.Request{NodeID: nodeID, RequestID: requestID}
    if expectedBlkID, ok := e.blkReqs.GetValue(req); ok {
        if blk.ID() != expectedBlkID {
            return e.GetFailed(ctx, nodeID, requestID)  // Yanlış blok
        }
    }

    e.issueFrom(ctx, nodeID, blk, issuedMetric)
    return e.executeDeferredWork(ctx)
}
```

### Gossip Mekanizması

```go
func (e *Engine) Gossip(ctx context.Context) error {
    // Aktif blok varsa gossip atla, repoll yap
    if numProcessing := e.Consensus.NumProcessing(); numProcessing != 0 {
        e.repoll(ctx)
        return nil
    }

    // Uniform örnekleme (büyük stake'li node'ların bant genişliği baskısını azaltır)
    vdrID, ok := e.ConnectedValidators.SampleValidator()

    // Tercihimizi sor
    e.Sender.SendPullQuery(ctx, set.Of(vdrID), e.requestID,
                           e.Consensus.Preference(), nextHeightToAccept)
}
```

---

## 10. voter.go — Chit Uygulaması ve Bağımlılık Zinciri

**Dosya:** `snow/engine/snowman/voter.go`

```go
type voter struct {
    e               *Engine
    nodeID          ids.NodeID
    requestID       uint32
    responseOptions []ids.ID    // [preferredID, preferredIDAtHeight] sıralamalı
}

func (v *voter) Execute(ctx context.Context, _ []ids.ID, _ []ids.ID) error {
    var (
        vote       ids.ID
        shouldVote bool
        voteIndex  int
    )

    // responseOptions'ı sırayla dene
    // İlk geçerli (processing) bloğu bul
    for i, voteOption := range v.responseOptions {
        vote, shouldVote = v.e.getProcessingAncestor(voteOption)
        if shouldVote {
            voteIndex = i
            break
        }
    }

    var results []bag.Bag[ids.ID]
    if shouldVote {
        v.e.selectedVoteIndex.Observe(float64(voteIndex))
        results = v.e.polls.Vote(v.requestID, v.nodeID, vote)
    } else {
        results = v.e.polls.Drop(v.requestID, v.nodeID)  // Geçersiz oy at
    }

    for _, result := range results {
        v.e.Consensus.RecordPoll(ctx, result)  // Snowman Topological'a bildir
    }

    v.e.VM.SetPreference(ctx, v.e.Consensus.Preference())

    if v.e.Consensus.NumProcessing() > 0 {
        v.e.repoll(ctx)  // Hala işlenecek blok var, polling sürsün
    }
}
```

### getProcessingAncestor

Verilen blokID işleniyor mu? Değilse, işlenen en yakın atayı bul. Bu, yayılmamış bağımlılıklar nedeniyle kilitlenmeleri önler.

---

## 11. PlatformVM (P-Chain) — Validator ve Staking

**Dosya:** `vms/platformvm/config/internal.go`

P-Chain, tüm validator yönetimini koordine eder:

```go
type Internal struct {
    Chains chains.Manager          // Diğer zincirleri yönet
    Validators validators.Manager  // SubnetID → ValidatorSet

    // Etna upgrade sonrası dinamik ücretler
    DynamicFeeConfig gas.Config
    ValidatorFeeConfig fee.Config  // ACP-77

    UptimeLockedCalculator uptime.LockedCalculator
    SybilProtectionEnabled bool
    PartialSyncPrimaryNetwork bool
    TrackedSubnets set.Set[ids.ID]

    // Stake parametreleri
    MinValidatorStake uint64       // Minimum stake (nAVAX)
    MaxValidatorStake uint64       // Maksimum stake
    MinDelegatorStake uint64
    MinDelegationFee  uint32

    // Uptime koşulu
    UptimePercentage float64       // Genellikle 0.8 (%80)

    MinStakeDuration time.Duration
    MaxStakeDuration time.Duration
    HeliconMinStakeDuration time.Duration  // ACP-273 sonrası

    RewardConfig reward.Config
    UpgradeConfig upgrade.Config
    UseCurrentHeight bool           // ProposerVM aktivasyonu için
}
```

### PlatformVM Cache Yapılandırması

```go
var Default = Config{
    BlockCacheSize:                64 * units.MiB,
    TxCacheSize:                   128 * units.MiB,
    TransformedSubnetTxCacheSize:  4 * units.MiB,
    RewardUTXOsCacheSize:          2048,
    ChainCacheSize:                2048,
    BlockIDCacheSize:              8192,
    MempoolPruneFrequency:         30 * time.Minute,
    MempoolGasCapacity:            1_000_000,
}
```

### Validator Seçim Akışı

```
AddValidatorTx → P-Chain işler
    ↓
ValidatorSet güncellenir (SubnetID → []Validator)
    ↓
Snowball K=20 örneklemesi bu set'ten yapılır
    ↓
Uptime takibi (Connected/Disconnected mesajları)
    ↓
Epoch sonu: UptimePercentage >= 0.8 ise ödül
```

---

## 12. Reward Calculator

**Dosya:** `vms/platformvm/reward/calculator.go`

### Ödül Formülü

```
RemainingSupply    = SupplyCap - CurrentSupply
PortionOfSupply    = StakedAmount / CurrentSupply
PortionOfDuration  = StakingDuration / MintingPeriod
MintingRate        = MinRate + (MaxRate - MinRate) * PortionOfDuration
Reward             = RemainingSupply * PortionOfSupply * MintingRate * PortionOfDuration
```

### Go Implementasyonu

```go
func (c *calculator) Calculate(
    _ time.Time,
    stakedDuration time.Duration,
    stakedAmount, currentSupply uint64,
) uint64 {
    bigStakedDuration := new(big.Int).SetUint64(uint64(stakedDuration))
    bigStakedAmount   := new(big.Int).SetUint64(stakedAmount)
    bigCurrentSupply  := new(big.Int).SetUint64(currentSupply)

    // Adjusted consumption rate: lineer interpolasyon
    adjustedConsumptionRateNumerator := new(big.Int).Mul(
        c.maxSubMinConsumptionRate, bigStakedDuration)
    adjustedMinConsumptionRateNumerator := new(big.Int).Mul(
        c.minConsumptionRate, c.mintingPeriod)
    adjustedConsumptionRateNumerator.Add(
        adjustedConsumptionRateNumerator, adjustedMinConsumptionRateNumerator)
    adjustedConsumptionRateDenominator := new(big.Int).Mul(
        c.mintingPeriod, consumptionRateDenominator)  // consumptionRateDenominator=1_000_000

    remainingSupply := c.supplyCap - currentSupply
    reward := new(big.Int).SetUint64(remainingSupply)
    reward.Mul(reward, adjustedConsumptionRateNumerator)
    reward.Mul(reward, bigStakedAmount)
    reward.Mul(reward, bigStakedDuration)
    reward.Div(reward, adjustedConsumptionRateDenominator)
    reward.Div(reward, bigCurrentSupply)
    reward.Div(reward, c.mintingPeriod)

    return min(remainingSupply, reward.Uint64())
}
```

### Helicon Upgrade (ACP-285)

```go
// Helicon ile MinConsumptionRate %10'dan %7.5'a düşürüldü
// 90 günlük geçiş süresi: lineer rampa
const heliconMinConsumptionRateTarget = 75_000  // = 7.5%
const heliconMinConsumptionRateReductionPeriod = 90 * 24 * time.Hour
```

### Delegasyon Bölme

```go
// PercentDenominator = 1_000_000
func Split(totalAmount uint64, shares uint32) (validatorAmount, delegatorAmount uint64) {
    remainderShares := PercentDenominator - uint64(shares)
    // delegatorAmount = totalAmount * remainderShares / PercentDenominator
    // validatorAmount = totalAmount - delegatorAmount
}
```

---

## 13. Block Tipleri: Apricot vs Banff

**Dosya:** `vms/platformvm/block/block.go`

### Block Interface

```go
type Block interface {
    snow.ContextInitializable
    ID() ids.ID
    Parent() ids.ID
    Bytes() []byte
    Height() uint64
    Txs() []*txs.Tx
    Visit(visitor Visitor) error
    initialize(bytes []byte) error
}

// Banff blokları timestamp içerir
type BanffBlock interface {
    Block
    Timestamp() time.Time
}
```

### Blok Tipleri

| Blok Tipi | Dönem | Açıklama |
|-----------|-------|---------|
| `ApricotProposalBlock` | Apricot | Staker değişikliği önerisi |
| `ApricotAbortBlock` | Apricot | Önerinin reddi |
| `ApricotCommitBlock` | Apricot | Önerinin kabulü |
| `ApricotStandardBlock` | Apricot | Normal tx bloğu |
| `ApricotAtomicBlock` | Apricot | Cross-chain atomik tx |
| `BanffProposalBlock` | Banff+ | Timestamp'li proposal |
| `BanffAbortBlock` | Banff+ | Timestamp'li abort |
| `BanffCommitBlock` | Banff+ | Timestamp'li commit |
| `BanffStandardBlock` | Banff+ | Timestamp'li standart blok |

### Visitor Pattern

```go
type Visitor interface {
    BanffAbortBlock(*BanffAbortBlock) error
    BanffCommitBlock(*BanffCommitBlock) error
    BanffProposalBlock(*BanffProposalBlock) error
    BanffStandardBlock(*BanffStandardBlock) error
    ApricotAbortBlock(*ApricotAbortBlock) error
    ApricotCommitBlock(*ApricotCommitBlock) error
    ApricotProposalBlock(*ApricotProposalBlock) error
    ApricotStandardBlock(*ApricotStandardBlock) error
    ApricotAtomicBlock(*ApricotAtomicBlock) error
}
// Kullanım: blk.Visit(verifier) veya blk.Visit(acceptor)
```

### ProposalBlock Mantığı

ProposalBlock, iki olası outcome içerir:
- **CommitBlock:** Proposal kabul edildi (örn. validator eklendi)
- **AbortBlock:** Proposal reddedildi (örn. stake süresi dolmadı)

Bu iki option, Snowball ile rekabete girer. Hangisi kazanırsa o kabul edilir.

---

## 14. PlatformVM Blok Doğrulama — verifier.go

**Dosya:** `vms/platformvm/block/executor/verifier.go`

```go
type verifier struct {
    *backend
    txExecutorBackend *executor.Backend
    pChainHeight      uint64
}

func (v *verifier) BanffProposalBlock(b *block.BanffProposalBlock) error {
    // 1. Banff formatı kontrolü
    if err := v.banffNonOptionBlock(b); err != nil { return err }

    // 2. Diff state oluştur (parent üzerinde)
    isAddingStakerAfterDeletionAllowed := state.StakerAdditionAfterDeletionLegality(
        v.txExecutorBackend.Config.UpgradeConfig.IsHeliconActivated(b.Timestamp()))
    onDecisionState, _ := state.NewDiff(parentID, v.backend, ...)

    // 3. Zamanı ilerlet
    executor.AdvanceTimeTo(v.txExecutorBackend, onDecisionState, nextChainTime)

    // 4. Ücret hesaplayıcı seç
    feeCalculator := state.PickFeeCalculator(v.txExecutorBackend.Config, onDecisionState)

    // 5. Tx'leri işle
    inputs, atomicRequests, onAcceptFunc, gasConsumed, _, _ :=
        v.processStandardTxs(b.Transactions, feeCalculator, onDecisionState, b.Parent())

    // 6. Commit ve Abort state'leri oluştur
    onCommitState, _ := state.NewDiffOn(onDecisionState, ...)
    onAbortState,  _ := state.NewDiffOn(onDecisionState, ...)

    return v.proposalBlock(...)
}
```

### Hata Tipleri

```go
var (
    ErrConflictingBlockTxs         // Blok içindeki tx'ler çakışıyor
    ErrStandardBlockWithoutChanges // Banff StandardBlock boş
    errApricotBlockIssuedAfterFork // Apricot blok Banff sonrası geldi
    errIncorrectBlockHeight        // Yükseklik tutarsız
    errOptionBlockTimestampNotMatchingParent  // Timestamp uyuşmuyor
)
```

---

## 15. PlatformVM Blok Kabulü — acceptor.go

**Dosya:** `vms/platformvm/block/executor/acceptor.go`

```go
type acceptor struct {
    *backend
    metrics    metrics.Metrics
    validators *validators.Manager
}

func (a *acceptor) BanffStandardBlock(b *block.BanffStandardBlock) error {
    return a.standardBlock(b, "banff standard")
}

func (a *acceptor) ApricotAtomicBlock(b *block.ApricotAtomicBlock) error {
    blkState, ok := a.blkIDToState[blkID]

    // State'i ana state'e uygula
    blkState.onAcceptState.Apply(a.state)

    defer a.state.Abort()
    batch, _ := a.state.CommitBatch()

    // Atomik olarak hem DB hem SharedMemory güncelle
    a.ctx.SharedMemory.Apply(blkState.atomicRequests, batch)
}
```

### Kabul Sırası

```
1. commonAccept()           — height, timestamps güncelle
2. onAcceptState.Apply()    — Değişiklikleri ana state'e yaz
3. state.CommitBatch()      — DB transaction oluştur
4. SharedMemory.Apply()     — Atomik cross-chain güncelleme
```

---

## 16. Ağ Katmanı — network.go

**Dosya:** `network/network.go`

### Network Interface

```go
type Network interface {
    sender.ExternalSender    // P2P mesaj gönderimi
    health.Checker            // Health check
    peer.Network              // Peer yönetimi
    StartClose()              // Graceful shutdown
    Dispatch() error          // Ana döngü
    ManuallyTrack(nodeID ids.NodeID, ip netip.AddrPort)
    PeerInfo(nodeIDs []ids.NodeID) []peer.Info
    NodeUptime() (UptimeResult, error)
}
```

### network Struct

```go
type network struct {
    config     *Config
    peerConfig *peer.Config
    metrics    *metrics

    // Throttling
    outboundMsgThrottler throttling.OutboundMsgThrottler
    inboundConnUpgradeThrottler throttling.InboundConnUpgradeThrottler

    listener      net.Listener      // Gelen bağlantılar
    dialer        dialer.Dialer     // Giden bağlantılar
    serverUpgrader peer.Upgrader    // TLS (inbound)
    clientUpgrader peer.Upgrader    // TLS (outbound)

    closeOnce            sync.Once
    onCloseCtx           context.Context
    sendFailRateCalculator safemath.Averager

    // IP Takibi
    ipTracker *ipTracker

    // Peer State
    peersLock       sync.RWMutex
    trackedIPs      map[ids.NodeID]*trackedIP
    connectingPeers *peer.Set     // Handshake devam ediyor
    connectedPeers  *peer.Set     // Handshake tamamlandı
    closing         bool

    router router.ExternalHandler  // Mesajları engine'e ilet
}
```

### NewNetwork — Başlangıç Akışı

```go
func NewNetwork(...) (Network, error) {
    // 1. Inbound message throttler (CPU/disk bazlı)
    inboundMsgThrottler, _ := throttling.NewInboundMsgThrottler(
        log, metricsRegisterer, config.Validators,
        config.ThrottlerConfig.InboundMsgThrottlerConfig,
        config.ResourceTracker, config.CPUTargeter, config.DiskTargeter)

    // 2. Outbound message throttler (Sybil-resistant: stake ağırlıklı)
    outboundMsgThrottler, _ := throttling.NewSybilOutboundMsgThrottler(
        log, metricsRegisterer, config.Validators,
        config.ThrottlerConfig.OutboundMsgThrottlerConfig)

    // 3. IP Tracker (validator IP'lerini takip et ve gossip et)
    ipTracker, _ := newIPTracker(config.TrackedSubnets, ...)
    config.Validators.RegisterCallbackListener(ipTracker)

    // 4. Genesis bootstrappers'ı gossip edilecek şekilde ekle
    for _, bootstrapper := range genesis.GetBootstrappers(config.NetworkID) {
        ipTracker.ManuallyGossip(constants.PrimaryNetworkID, bootstrapper.ID)
    }

    // 5. Genesis validator'larını optimistik bağlantı için kaydet
    for nodeID := range genesis.GetValidators(config.NetworkID) {
        ipTracker.ManuallyTrack(nodeID)
    }
}
```

### UptimeResult

```go
type UptimeResult struct {
    // Stake'in yüzde kaçı bizi uptime gereksiniminin üzerinde görüyor
    RewardingStakePercentage float64

    // Stake ağırlıklı ortalama uptime
    WeightedAveragePercentage float64
}
```

### Throttling Stratejisi

- **Inbound:** CPU ve disk kullanımına göre kısıtlama (kaynak açısı)
- **Outbound:** Stake ağırlığına göre kısıtlama (Sybil açısı)
  - Çok az stake'i olan node daha az mesaj gönderebilir
  - Bu, Sybil saldırılarını pahalı hale getirir

---

## 17. Chain Manager — Zincir Yönetimi

**Dosya:** `chains/manager.go`

### Manager Interface

```go
type Manager interface {
    ids.Aliaser
    QueueChainCreation(ChainParameters)  // P-Chain thread'inden çağrılır
    AddRegistrant(Registrant)            // Yeni zincir oluşunca bildirilecek
    Lookup(alias string) (ids.ID, error)
    LookupVM(alias string) (ids.ID, error)
    IsBootstrapped(ids.ID) bool
    StartChainCreator(platformChain ChainParameters) error
    Shutdown()
}
```

### ChainParameters

```go
type ChainParameters struct {
    ID          ids.ID        // Zincir ID
    SubnetID    ids.ID        // Bu zinciri valide eden subnet
    GenesisData []byte        // Genesis durumu
    VMID        ids.ID        // Çalıştırılacak VM
    FxIDs       []ids.ID      // Özellik uzantıları (secp256k1fx, nftfx, propertyfx)
    CustomBeacons validators.Manager  // Sadece P-Chain için
}
```

### manager Struct

```go
type manager struct {
    ids.Aliaser
    ManagerConfig
    pChainProgress *lock.ProgressSubscription[uint64]
    registrants    []Registrant

    // Zincir oluşturma kuyruğu
    chainsQueue           buffer.BlockingDeque[ChainParameters]
    unblockChainCreatorCh chan struct{}

    chainsLock sync.Mutex
    chains     map[ids.ID]handler.Handler

    // Prometheus metric toplayıcıları (chainID bazlı)
    avalancheGatherer    metrics.MultiGatherer
    handlerGatherer      metrics.MultiGatherer
    proposervmGatherer   metrics.MultiGatherer
    snowmanGatherer      metrics.MultiGatherer
    // ...
}
```

### Yerleşik FX'ler

```go
var fxs = map[ids.ID]fx.Factory{
    secp256k1fx.ID: &secp256k1fx.Factory{},  // ECDSA imzaları
    nftfx.ID:       &nftfx.Factory{},         // NFT işlemleri
    propertyfx.ID:  &propertyfx.Factory{},    // Mülkiyet işlemleri
}
```

### VM Sarmalama Hiyerarşisi

```
Ham VM
  → tracedvm (OpenTelemetry tracing)
  → metervm (Prometheus metrik ölçümü)
  → proposervm (Snowman++ blok önerisi)
```

**proposervm:** P-Chain yüksekliğini kullanarak blok önericisini deterministik seçer. Böylece her slot için tek bir "leader" belirlenir.

---

## 18. Simplex — Yeni BFT Consensus Alternatifi

**Dosya:** `snow/consensus/simplex/parameters.go`

```go
type Simplex struct {
    // Ağ gecikmesi üst sınırı
    MaxNetworkDelay time.Duration   // = 5 * time.Second

    // Yeniden yayın bekleme süresi üst sınırı
    MaxRebroadcastWait time.Duration // = 5 * time.Second

    InitialValidators []ValidatorInfo
}

type ValidatorInfo struct {
    NodeID ids.NodeID
    Weight uint64
}
```

### Simplex vs Snowball

| Özellik | Snowball | Simplex |
|---------|----------|---------|
| Tip | Probabilistic | BFT (deterministik) |
| Finality | Olasılıksal (Beta round sonra) | Matematiksel kesinlik |
| Network Model | Asenkron toleranslı | Kısmi senkroni |
| Round Sayısı | Değişken (Beta=20) | 2 round (propose+vote) |
| Fault Tolerance | < K - Alpha | < N/3 |
| Ağ Gecikmesi | Toleranslı | MaxNetworkDelay=5s |
| Kullanım Durumu | Multi-chain, subnet | Primary chain, kritik kararlar |

### Neden İkisi Birlikte?

Avalanche, yeni Simplex'i Snowball'ın yanı sıra sunuyor:
- **Simplex:** P-Chain ana kararları (validator ekle/çıkar) için daha güçlü finality
- **Snowball:** Subnet'ler ve C-Chain için hız ve esneklik

---

## 19. Uptime Mekanizması — Validator Ödül Koşulu

Avalanche'ta validator ödülü almak için yeterli uptime gereklidir:

### Nasıl Ölçülür?

```
Her validator için:
- Connected event → uptime timer başlar
- Disconnected event → uptime timer durur
- Epoch sonu: totalOnlineTime / stakeDuration >= UptimePercentage (0.8)
    → Ödül hak edilir
    → Ödül hak edilmezse AddValidatorTx'deki stake geri döner, ödül yoktur
```

### NodeUptime()

```go
type UptimeResult struct {
    RewardingStakePercentage  float64  // Bizi ödüllendirecek stake yüzdesi
    WeightedAveragePercentage float64  // Ağırlıklı ortalama uptime
}
```

Ağdaki diğer node'lar, bizim uptime'ımızı izler. Toplam stake'in %80'i bizi uptime gereksiniminin üzerinde görüyorsa ödül alırız.

---

## 20. Warp Messaging — Zincirlerarası İletişim

**Paket:** `vms/platformvm/warp/`
**Bağımlılık:** `github.com/supranational/blst` (BLS12-381)

### Genel Çalışma Prensibi

```
1. Kaynak zincirde mesaj oluşturulur
2. Validator'lar BLS imzası atar (kendi private key ile)
3. Yeterli stake ağırlığı toplanınca (≥67%) imzalar aggrege edilir
4. Aggregated BLS imzası hedef zincire gönderilir
5. Hedef zincir ValidatorSet'e göre imzayı doğrular
```

### ACP-118 — BLS Aggregation

`network/p2p/` altında `acp118` paketi: validator'lar arası BLS imza toplama protokolü.

```
Requestor → Validators: "Bu mesajı imzalar mısın?"
Validators → Requestor: BLS imzaları
Requestor: Aggregated signature oluştur
    → Hedef zincire gönder
```

### Nova Shield Relevansı

Warp Messaging, Cross-chain communication için güçlü bir model. Nova Shield'in eventual cross-chain messaging ihtiyacında benzer BLS aggregation kullanılabilir. Ancak biz tek zincir modeli seçtik.

---

## 21. Nova Shield için Çıkarımlar

### A. Snowball Consensus — Uyarlanamaz

Nova Shield **DPoS + BLS Threshold + Single-Slot Finality** seçti. Snowball probabilistik ve çok roundlu, single-slot finality ile uyuşmuyor.

**Ancak Snowball'dan alınabilecek fikirler:**
- `terminationConditions[]` yapısı: Birden fazla (alphaConfidence, beta) çifti. Farklı threshold'larda farklı güvenlik seviyeleri. Nova Shield'de "soft finality" ve "hard finality" ayrımı için kullanılabilir.
- `shouldReset` lazy sıfırlama: Büyük veri yapılarında batch reset yerine flag-based lazy reset.
- `Bag[ids.ID]` oy toplama yapısı: Vote aggregation için verimli multiset implementasyonu.

### B. Topological — Doğrudan Alınabilir

Kahn topolojik sıralama ile blok ağacını işleme **Nova Shield'de de gerekli.** Fork durumunda hangi bloğun kabul edileceğini belirlemek için:

```rust
// Nova Shield'de Rust implementasyonu:
struct BlockTree {
    blocks: HashMap<BlockId, BlockNode>,
    last_accepted_height: u64,
    preference: BlockId,
}

impl BlockTree {
    fn record_votes(&mut self, votes: &HashMap<BlockId, u64>) {
        // Kahn sort ile vote propagation
        // Threshold (2/3 stake) geçince finalize
    }
}
```

### C. Poll Seti — Alınabilir

`poll.Set` ve `poll.NewEarlyTermFactory` yapısı: Poll'lar erken sonlandırılabilir — supermajority sağlandıktan sonra kalan cevapları beklemeye gerek yok. **Nova Shield'in BLS threshold signature collection'ında aynı "early termination" uygulanabilir.**

### D. Voter ve Job Scheduler — Doğrudan Alınabilir

`job.Scheduler[ids.ID]` dependency-based scheduling: Bir blok, bağımlılıkları çözülünce işlenir. Nova Shield'de:

```rust
// Bağımlılık zinciri yönetimi
struct PendingVotes {
    // blockId → voter (bağımlılıkları bekliyor)
    pending: HashMap<BlockId, Vec<VoteCollector>>,
}
```

### E. Reward Formula — Doğrudan İlham

Avalanche'ın reward formülü: `Remaining * StakeRatio * DurationRatio * MintingRate`

Nova Shield için adaptasyon:
```
Reward = RemainingSupply × (StakedAmount / CurrentStaked)
       × (StakeDuration / MaxStakeDuration)
       × (MinRate + (MaxRate - MinRate) × DurationRatio)
```

- `PercentDenominator = 1_000_000` (integer matematik için)
- `big.Int` kullanımı: Taşma önlemek için kritik
- Helicon upgrade rampa mantığı: Parametre değişikliklerinde ani geçiş yerine lineer rampa

### F. Uptime Ödülü — Nova Shield'de Kullanılacak

```
MinUptime = %80 (varsayılan)
Epoch sonu: uptime < %80 → ödül yok, stake geri döner
```

Nova Shield'de de aynı mekanizma: Validator her epoch başında stake + ödül alır, ancak yeterli uptime yoksa sadece stake geri döner.

### G. Network Throttling — Alınabilir

İki katmanlı throttling:
1. **Inbound:** CPU/disk bazlı (node koruması)
2. **Outbound:** Stake bazlı Sybil koruması

Nova Shield implementasyonu:
```rust
struct ThrottleConfig {
    inbound_cpu_limit: f64,
    inbound_disk_limit: f64,
    outbound_stake_weight_min: u64,  // Minimum stake yoksa mesaj gönderme
}
```

### H. Chain Manager Pattern — Subnet Yerine Shard

Avalanche'ın chain manager → subnet modeli, Nova Shield'in potansiyel sharding tasarımı için referans:

```
Nova Shield (gelecek):
├── Primary Chain      — Validator yönetimi (P-Chain karşılığı)
├── Execution Shard 1  — İşlem zinciri (C-Chain karşılığı)
└── Execution Shard N  — Ölçeklenme
```

Her shard kendi Snowball/Simplex benzeri consensus çalıştırır. Chain Manager `QueueChainCreation()` → P-Chain thread koordinasyonu pattern'i doğrudan kullanılabilir.

### I. Simplex — En Önemli Çıkarım

Simplex, **Nova Shield'in single-slot finality hedefine en uygun model:**

```
Avalanche Simplex Parameters:
- MaxNetworkDelay: 5s
- MaxRebroadcastWait: 5s
- 2 round: Propose + Vote

Nova Shield Single-Slot Finality:
- Slot süresi: 400ms
- MaxNetworkDelay: <100ms (yeterli)
- Round: BLS Threshold signature collection
```

Simplex'in 2-round BFT modeli, Nova Shield'in "400ms slot → instant finality" hedefine doğrudan uyarlanabilir.

### J. ProposerVM — Leader Seçimi

Avalanche'ta proposervm, P-Chain yüksekliğini VRF benzeri şekilde kullanarak her slot için deterministik bir blok önerici seçer. Nova Shield'de:

```rust
// VRF tabanlı leader seçimi (DPoS)
fn select_leader(epoch: u64, slot: u64, validator_set: &ValidatorSet) -> ValidatorId {
    let seed = hash(epoch, slot, prev_block_hash);
    let vrf_output = vrf_prove(validator_key, seed);
    validator_set.weighted_select(vrf_output)
}
```

### K. Block Visitor Pattern — İyi Uygulama

Avalanche'ta `block.Visit(visitor)` pattern:
- Verifier, Acceptor, Rejector ayrı struct'lar
- Her struct bir Visitor implement eder
- Block tipi dispatch: `blk.Visit(verifier)` → doğru metod çağrılır

Nova Shield'de aynı pattern:
```rust
trait BlockVisitor {
    fn visit_standard_block(&self, blk: &StandardBlock) -> Result<()>;
    fn visit_genesis_block(&self, blk: &GenesisBlock) -> Result<()>;
}
```

---

## 22. Özet Karşılaştırma Tablosu

| Özellik | Avalanche | Nova Shield |
|---------|-----------|-------------|
| Consensus | Snowball (probabilistic) | BLS Threshold BFT (deterministic) |
| Finality | Olasılıksal (~Beta=20 round) | Single-slot (400ms) |
| Validator | Stake ağırlıklı (P-Chain) | DPoS + minimum stake |
| Blok Önerici | ProposerVM (P-Chain height tabanlı) | VRF (epoch+slot tabanlı) |
| Çok Zincir | P/X/C-Chain + Subnet | Tek zincir (başlangıç) |
| P2P | Custom TCP + TLS | libp2p + Noise |
| Şifreleme | ECDSA + BLS12-381 | Dilithium3 + BLS (post-quantum) |
| Uptime | %80 gereksinim | Aynı (alınıyor) |
| Ödül | Remaining×Ratio formülü | Aynı formül (uyarlanıyor) |
| MEV Koruması | Yok | Encrypted Mempool + Batch Auction |
| Seed Phrase | Var | Yok (MPC native) |

---

## Sonuç

Avalanchego'dan alınacak en kritik 5 element:

1. **Reward Formula** (`big.Int` integer matematiği, lineer interpolasyon) — Nova Shield'in staking reward mekanizmasına doğrudan uyarlanabilir.

2. **Simplex BFT parametreleri** (MaxNetworkDelay=5s, 2-round finality) — Nova Shield'in single-slot finality'si için en yakın referans.

3. **Uptime ≥ %80 = ödül** mekanizması — Validator teşvik tasarımı için alınıyor.

4. **Job Scheduler (dependency-based)** — Blok bağımlılığı çözülünce vote uygulama. Rust'a port edilebilir.

5. **Network Throttling** (inbound CPU/disk + outbound stake-weight) — P2P katmanı Sybil koruması için alınıyor.
