# dYdX V4 Derinlemesine Analiz — Dosya 01
## L1 Cosmos SDK Mimarisi: Modül Yapısı, ABCI Yaşam Döngüsü ve Keeper Pattern

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/dydx-v4/`
**Amaç:** Nova Shield Chain için native DEX modülü geliştirmede referans alınacak mimari kararların Türkçe olarak tam dokümantasyonu

---

## 1. Genel Mimari: dYdX V4 Neden L1 Cosmos SDK?

dYdX V4, Uniswap gibi EVM üzerinde çalışan bir AMM değildir. Tam tersine, **kendi Cosmos SDK tabanlı L1 blockchain**'ini oluşturmuştur. Bu kararın arkasındaki temel gerekçeler:

| Sorun | EVM'de Sorun | Cosmos SDK Çözümü |
|-------|-------------|-------------------|
| MEV | Her blok sıralama manipülasyona açık | Validator deterministik sıralama |
| Gas | Her order/cancel gas ücretli | İşlem başı ücret değil blok başı ücret modeli |
| Throughput | EVM saniye başı ~15 tx | CometBFT ~500-2000 tx/s |
| Order Book | On-chain CLOB imkansız (gas) | In-memory memclob, blok sonunda settle |
| Perpetuals | Karmaşık collateral yönetimi | Dedicated `x/subaccounts` modülü |

### 1.1 Fiziksel Katman Yapısı

```
dydx-v4/
├── protocol/              ← Cosmos SDK uygulama kodu
│   ├── app/               ← App wiring, ante handlers
│   ├── x/                 ← Custom modüller
│   │   ├── clob/          ← Central Limit Order Book (ANA MODÜL)
│   │   ├── perpetuals/    ← Perpetual futures + funding rate
│   │   ├── subaccounts/   ← Collateral yönetimi (marjin hesapları)
│   │   ├── prices/        ← Oracle fiyat beslemesi
│   │   ├── epochs/        ← Zaman bazlı epoch sistemi (funding)
│   │   ├── feetiers/      ← Maker/taker fee tier yönetimi
│   │   ├── assets/        ← USDC ve diğer varlıklar
│   │   ├── bridge/        ← L1 ↔ dYdX köprüsü
│   │   ├── vault/         ← LP vault sistemi
│   │   └── delaymsg/      ← Gecikmeli mesaj kuyruğu
│   ├── daemons/           ← Arka plan servisleri (liquidation, price)
│   ├── indexer/           ← Off-chain indeksleme
│   └── lib/               ← Ortak matematik + yardımcı fonksiyonlar
├── indexer/               ← Kafka tabanlı event streaming
└── proto/                 ← Protobuf tanımları
```

---

## 2. Cosmos SDK Temel Kavramlar

### 2.1 Modül Yapısı

Her Cosmos SDK modülü standart bir yapıya sahiptir:

```go
// Örnek: x/clob/module.go
type AppModule struct {
    cdc        codec.Codec
    keeper     keeper.Keeper
    // ...
}

func (am AppModule) RegisterServices(cfg module.Configurator) {
    types.RegisterMsgServer(cfg.MsgServer(), keeper.NewMsgServerImpl(am.keeper))
    types.RegisterQueryServer(cfg.QueryServer(), am.keeper)
}

// ABCI Hook'ları
func (am AppModule) BeginBlock(ctx sdk.Context, req abci.RequestBeginBlock) {}
func (am AppModule) EndBlock(ctx sdk.Context, req abci.RequestEndBlock) abci.ResponseEndBlock {}
```

### 2.2 Store Türleri

dYdX V4 üç tür store kullanır:

| Store Türü | dYdX Kullanımı | Özellik |
|-----------|---------------|---------|
| `KVStore` | Stateful orders, ClobPair, LiquidationsConfig | Bloklar arası kalıcı |
| `MemStore` | ProcessProposerMatchesEvents, memclob init flag | Blok içi geçici, restart'ta sıfır |
| `TransientStore` | SubaccountLiquidationInfo, uncommitted orders | Sadece mevcut blok içi geçerli |

```go
// x/clob/types/keys.go
const (
    StoreKey          = ModuleName        // "clob"
    MemStoreKey       = "mem_" + ModuleName   // "mem_clob"
    TransientStoreKey = "tmp_" + ModuleName   // "tmp_clob"
)
```

**Kritik Detay:** `MemStore` restart'ta sıfırlandığı için, node restart sonrası `PrepareCheckState`'de memclob baştan doldurulur. Bu Nova Shield için önemli bir tasarım kararıdır.

### 2.3 Keeper Pattern

Cosmos SDK'da business logic `Keeper` struct'larında yaşar:

```go
// x/clob/keeper/keeper.go (basitleştirilmiş)
type Keeper struct {
    cdc                        codec.BinaryCodec
    storeKey                   storetypes.StoreKey
    memKey                     storetypes.StoreKey
    transientStoreKey          storetypes.StoreKey
    MemClob                    types.MemClob           // In-memory CLOB
    UntriggeredConditionalOrders // conditional order store
    DaemonLiquidationInfo      flags.DaemonLiquidationInfo
    mevTelemetryConfig         MevTelemetryConfig
    Flags                      flags.ClobFlags
    // Cross-keeper references
    subaccountsKeeper          types.SubaccountsKeeper
    assetsKeeper               types.AssetsKeeper
    bankKeeper                 types.BankKeeper
    feeTiersKeeper             types.FeeTiersKeeper
    perpetualsKeeper           types.PerpetualsKeeper
    statsKeeper                types.StatsKeeper
    rewardsKeeper              types.RewardsKeeper
    indexerEventManager        indexer_manager.IndexerEventManager
    streamingManager           streamingtypes.FullNodeStreamingManager
}
```

**Bidirectional Dependency Sorunu:** CLOB Keeper ve Perpetuals Keeper birbirine referans vermek zorundadır (CLOB perpetuals için premium hesaplar, Perpetuals CLOB'u funding için okur). Bu döngüsel bağımlılık `SetClobKeeper()` metoduyla çözülür:

```go
// x/perpetuals/keeper/keeper.go
func (k *Keeper) SetClobKeeper(getter types.PerpetualsClobKeeper) {
    k.clobKeeper = getter  // App wiring sırasında inject edilir
}
```

Aynı pattern MemClob ↔ ClobKeeper arasında da geçerlidir:

```go
// x/clob/memclob/memclob.go:83
func (m *MemClobPriceTimePriority) SetClobKeeper(clobKeeper types.MemClobKeeper) {
    m.clobKeeper = clobKeeper
}
```

---

## 3. ABCI Yaşam Döngüsü (Block Processing)

CometBFT (eski Tendermint) blok işleme akışı dYdX V4'te çok spesifik bir şekilde kullanılmaktadır:

### 3.1 Tam Blok Yaşam Döngüsü

```
CheckTx (p2p mempool)
    ↓ ClobDecorator.AnteHandle()
    ↓ Short-term orders → MemClob.PlaceOrder()
    ↓ Stateful orders → state'e yazılır (transient store)

PrepareProposal (block proposer)
    ↓ MemClob'daki eşleşmiş emirleri MsgProposedOperations olarak paketle

ProcessProposal (validators - consensus)
    ↓ Önerilen operasyonları doğrula

FinalizeBlock / DeliverTx (tüm validators)
    ↓ MsgProposedOperations işlenir, state kesinleşir

EndBlock
    ↓ Süresi dolan stateful emirler temizlenir
    ↓ Conditional orders trigger edilir
    ↓ TWAP suborders üretilir

Precommit (Commit öncesi hook)
    ↓ Staged finalize block events işlenir
    ↓ Yeni ClobPair'ler MemClob'a eklenir

PrepareCheckState (Commit sonrası, sonraki blok CheckTx öncesi)
    ↓ Lokal operations queue temizlenir
    ↓ MemClob geçersiz state temizlenir
    ↓ Stateful orders yeniden MemClob'a eklenir
    ↓ Replay: Lokal operasyonlar yeniden oynatılır
    ↓ Likidasyonlar denenir
    ↓ Deleveraging yapılır
```

### 3.2 BeginBlocker

```go
// x/clob/abci.go:29-47
func BeginBlocker(ctx sdk.Context, keeper types.ClobKeeper) {
    // ProcessProposerMatchesEvents'i sıfırla (önceki blok eventlerini temizle)
    keeper.MustSetProcessProposerMatchesEvents(
        ctx,
        types.ProcessProposerMatchesEvents{
            BlockHeight: lib.MustConvertIntegerToUint32(ctx.BlockHeight()),
        },
    )
    keeper.ResetAllDeliveredOrderIds(ctx)
}
```

`ProcessProposerMatchesEvents` her blok başında sıfırlanır. Bu struct, blok işleme sürecinde toplanan doldurulan/iptal edilen/süresi dolan order ID'lerini taşır.

### 3.3 EndBlocker

```go
// x/clob/abci.go:66-134
func EndBlocker(ctx sdk.Context, keeper keeper.Keeper) {
    // 1. Short-term order fill amount'larını prune et
    keeper.PruneStateFillAmountsForShortTermOrders(ctx)

    // 2. Süresi dolan stateful emirleri state'den sil
    expiredStatefulOrderIds := keeper.RemoveExpiredStatefulOrders(ctx, ctx.BlockTime())
    for _, orderId := range expiredStatefulOrderIds {
        keeper.RemoveOrderFillAmount(ctx, orderId)
        keeper.DeleteLongTermOrderPlacement(ctx, orderId)
        // Indexer'a event gönder
        keeper.GetIndexerEventManager().AddBlockEvent(...)
    }

    // 3. TWAP suborder üret (TWAP emirleri parçalara bölünür)
    keeper.GenerateAndPlaceTriggeredTwapSuborders(ctx)

    // 4. Tetiklenen conditional orderları bul
    triggeredConditionalOrderIds := keeper.MaybeTriggerConditionalOrders(ctx)

    // 5. Events'i state'e yaz (PrepareCheckState'de okunacak)
    keeper.MustSetProcessProposerMatchesEvents(ctx, processProposerMatchesEvents)

    // 6. Insurance fund bakiyesini metrik olarak kaydet
    metrics.SetGauge(metrics.InsuranceFundBalance,
        metrics.GetMetricValueFromBigInt(keeper.GetCrossInsuranceFundBalance(ctx)))
}
```

### 3.4 PrepareCheckState (En Kritik Fonksiyon)

Bu fonksiyon commit'ten sonra, bir sonraki CheckTx öncesinde çalışır. MemClob'u yeni blok için hazırlar:

```go
// x/clob/abci.go:136-310
func PrepareCheckState(ctx sdk.Context, keeper *keeper.Keeper) {
    // 1. Önceki lokal operasyon kuyruğunu al
    localValidatorOperationsQueue, shortTermOrderTxBytes :=
        keeper.MemClob.GetOperationsToReplay(ctx)

    // 2. MemClob'daki tüm emirleri kaldır
    keeper.MemClob.RemoveAndClearOperationsQueue(ctx, localValidatorOperationsQueue)

    // 3. Geçersiz MemClob state'ini temizle
    offchainUpdates = keeper.MemClob.PurgeInvalidMemclobState(
        ctx,
        processProposerMatchesEvents.OrderIdsFilledInLastBlock,  // doldurulanlar
        processProposerMatchesEvents.ExpiredStatefulOrderIds,    // süresi dolanlar
        keeper.GetDeliveredCancelledOrderIds(ctx),               // iptal edilenler
        processProposerMatchesEvents.RemovedStatefulOrderIds,    // kaldırılanlar
        offchainUpdates,
    )

    // 4. Post-only emirleri önce yerleştir (ordering önceliği)
    keeper.PlaceStatefulOrdersFromLastBlock(ctx, longTermOrderIds, offchainUpdates, true)
    keeper.PlaceConditionalOrdersTriggeredInLastBlock(ctx, triggedIds, offchainUpdates, true)
    keeper.MemClob.ReplayOperations(ctx, localOpsQueue, txBytes, updates, true)

    // 5. Normal emirleri yerleştir
    keeper.PlaceStatefulOrdersFromLastBlock(ctx, longTermOrderIds, offchainUpdates, false)
    keeper.PlaceConditionalOrdersTriggeredInLastBlock(ctx, triggedIds, offchainUpdates, false)
    keeper.MemClob.ReplayOperations(ctx, localOpsQueue, txBytes, updates, false)

    // 6. Likit edilebilir subaccount'ları bul ve likide et
    liquidatableSubaccountIds := keeper.DaemonLiquidationInfo.GetLiquidatableSubaccountIds()
    subaccountsToDeleverage, err := keeper.LiquidateSubaccountsAgainstOrderbook(
        ctx, liquidatableSubaccountIds)

    // 7. Deleveraging yap
    keeper.DeleverageSubaccounts(ctx, subaccountsToDeleverage)

    // 8. Negatif TNC subaccount'lar için withdrawal gate koy
    keeper.GateWithdrawalsIfNegativeTncSubaccountSeen(ctx, negativeTncSubaccountIds)
}
```

**Post-only İlk Geçiş Neden?** Post-only emirler hiçbir zaman taker olmaz. Önce yerleştirilirler ki sonradan gelen normal emirler bunlarla match edilebilsin.

### 3.5 ClobDecorator AnteHandler

Cosmos SDK'da `CheckTx` ve `DeliverTx`'e girmeden önce AnteHandler zinciri çalışır:

```go
// x/clob/ante/clob.go:51-160
func (cd ClobDecorator) AnteHandle(ctx, tx, simulate, next) (sdk.Context, error) {
    // DeliverTx veya simulation'da no-op
    if lib.IsDeliverTxMode(ctx) || simulate {
        return next(ctx, tx, simulate)
    }

    // Tx yapısını doğrula (tek mesaj olmalı)
    if err := ValidateMsgsInClobTx(tx); err != nil {
        return ctx, err
    }

    msgs := tx.GetMsgs()
    for _, msg := range msgs {
        switch msg := msg.(type) {
        case *types.MsgCancelOrder:
            if msg.OrderId.IsStatefulOrder() {
                err = cd.clobKeeper.CancelStatefulOrder(ctx, msg)
            } else {
                err = cd.clobKeeper.CancelShortTermOrder(ctx, msg)
            }
        case *types.MsgPlaceOrder:
            if msg.Order.IsStatefulOrder() {
                err = cd.clobKeeper.PlaceStatefulOrder(ctx, msg)
            } else {
                // Short-term → doğrudan MemClob'a
                _, err = cd.clobKeeper.PlaceShortTermOrder(ctx, msg)
            }
        }
    }
}
```

**Kritik Kural:** Tek bir transaction içinde sadece bir `MsgPlaceOrder` veya `MsgCancelOrder` olabilir. Diğer mesajlarla karıştırılamaz.

---

## 4. Order Türleri ve OrderId Yapısı

### 4.1 OrderId Flags

```go
// x/clob/types/order_id.go:12-18
const (
    OrderIdFlags_ShortTerm    = uint32(0)    // 0b00000000 - blok TTL'li
    OrderIdFlags_Conditional  = uint32(32)   // 0b00100000 - trigger fiyatlı
    OrderIdFlags_LongTerm     = uint32(64)   // 0b01000000 - zaman TTL'li
    OrderIdFlags_Twap         = uint32(128)  // 0b10000000 - TWAP ana emir
    OrderIdFlags_TwapSuborder = uint32(256)  // 0b100000000 - TWAP parçası
)
```

### 4.2 Order Tipleri Karşılaştırma

| Tip | Flags | TTL | State | MemClob | CheckTx'te |
|-----|-------|-----|-------|---------|-----------|
| Short-Term | 0 | GoodTilBlock | Transient | Evet | Doğrudan |
| Long-Term | 64 | GoodTilBlockTime | Persistent | PrepareCheckState'de | State'e yaz |
| Conditional | 32 | GoodTilBlockTime | Persistent | Trigger'da | State'e yaz |
| TWAP | 128 | GoodTilBlockTime | Persistent | EndBlock'ta | State'e yaz |

### 4.3 OrderId Benzersizliği

```go
// OrderId = {SubaccountId, ClientId, OrderFlags, ClobPairId}
// Bu 4 alan birlikte bir emiri benzersiz tanımlar.
// SubaccountId = {Owner (address), Number (uint32)}
// Bu yapı, aynı kullanıcının farklı subaccount'lardan aynı anda pozisyon almasına izin verir.
```

---

## 5. Modüller Arası Etkileşim Grafiği

```
x/clob (MemClob)
    │
    ├──→ x/subaccounts  [collateralization checks, position sizing]
    │       └──→ x/assets      [USDC ve diğer varlık bakiyeleri]
    │       └──→ x/perpetuals  [funding index okuma]
    │
    ├──→ x/perpetuals   [GetPricePremiumForPerpetual, open interest]
    │       └──→ x/prices     [oracle index price]
    │       └──→ x/epochs     [funding tick/sample epoch kontrolü]
    │
    ├──→ x/feetiers     [maker/taker fee hesabı]
    ├──→ x/stats        [istatistik güncelleme]
    ├──→ x/rewards      [ticaret ödülleri]
    └──→ x/affiliates   [affiliate rev share]
```

---

## 6. State Şeması (x/clob)

### 6.1 Persistent KV Store Anahtarları

```go
// x/clob/types/keys.go
const (
    // CLOB Pair yapılandırması
    ClobPairKeyPrefix = "Clob:"

    // Order fill miktarları (süresi dolunca prune edilir)
    OrderAmountFilledKeyPrefix = "Fill:"

    // Stateful order yerleşimleri
    LongTermOrderPlacementKeyPrefix    = "SO/P/L:"   // Long-term
    TriggeredConditionalOrderKeyPrefix = "SO/P/T:"   // Triggered conditional
    UntriggeredConditionalOrderKeyPrefix = "SO/U:"   // Untriggered
    TWAPOrderKeyPrefix                 = "TWAP:"

    // Süresi dolma zaman dilimleri
    StatefulOrdersExpirationsKeyPrefix = "Exp/%s:"

    // Yapılandırma
    LiquidationsConfigKey    = "LiqCfg"
    EquityTierLimitConfigKey = "EqTierCfg"
    BlockRateLimitConfigKey  = "RateLimCfg"
)
```

### 6.2 Transient Store (Blok İçi)

```go
const (
    // Subaccount likelilik bilgisi (blok içi sıfırlanır)
    SubaccountLiquidationInfoKeyPrefix = "SaLiqInfo:"

    // Uncommitted stateful orders
    UncommittedStatefulOrderPlacementKeyPrefix = "UncmtSO:"

    // Fiyat izleme (conditional order trigger için)
    MinTradePricePrefix = "MinTrade:"
    MaxTradePricePrefix = "MaxTrade:"
)
```

### 6.3 MemStore (Restart'ta Sıfırlanır)

```go
const (
    // ProcessProposerMatchesEvents - blok işleme meta verisi
    ProcessProposerMatchesEventsKey = "ProposerEvents"

    // MemClob başlatma bayrağı
    KeyMemstoreInitialized = "MemstoreInit"

    // Teslim edilen emirler (PrepareCheckState'de okunur)
    OrderedDeliveredLongTermOrderKeyPrefix = "DLTO:"
    DeliveredCancelKeyPrefix              = "DCancel:"
)
```

---

## 7. ClobPair Yapısı

ClobPair, bir perpetual market için order book yapılandırmasını tanımlar:

```protobuf
// proto/dydxprotocol/clob/clob_pair.proto
message ClobPair {
    uint32 id = 1;

    // Ya SpotClobMetadata ya da PerpetualClobMetadata
    oneof metadata {
        SpotClobMetadata spot_clob_metadata = 2;
        PerpetualClobMetadata perpetual_clob_metadata = 3;
    }

    // Minimum order boyutu (base quantums cinsinden)
    uint64 step_base_quantums = 4;

    // Alt tick içindeki subtick sayısı
    uint32 subticks_per_tick = 5;

    // Fiyat ↔ notional dönüşümü için
    sint32 quantum_conversion_exponent = 6;

    // Market durumu
    ClobPair_Status status = 7;
}

enum ClobPair_Status {
    STATUS_UNSPECIFIED    = 0;
    STATUS_ACTIVE         = 1;
    STATUS_PAUSED         = 2;  // Kullanılmıyor
    STATUS_CANCEL_ONLY    = 3;  // Kullanılmıyor
    STATUS_POST_ONLY      = 4;  // Kullanılmıyor
    STATUS_INITIALIZING   = 5;  // İlk fiyat bulunana kadar
    STATUS_FINAL_SETTLEMENT = 6; // Market kapanıyor
}
```

**Durum Geçişleri:**
```go
// x/clob/types/clob_pair.go
var SupportedClobPairStatusTransitions = map[ClobPair_Status]map[ClobPair_Status]struct{}{
    ClobPair_STATUS_ACTIVE: {
        ClobPair_STATUS_FINAL_SETTLEMENT: {},  // Kapanmaya gidebilir
    },
    ClobPair_STATUS_INITIALIZING: {
        ClobPair_STATUS_ACTIVE:           {},  // Fiyat bulununca aktif ol
        ClobPair_STATUS_FINAL_SETTLEMENT: {},  // Ya da direkt kapat
    },
    ClobPair_STATUS_FINAL_SETTLEMENT: {
        ClobPair_STATUS_INITIALIZING: {},      // Yeniden başlat
    },
}
```

---

## 8. Subtick ve Quantum Sistemi

dYdX V4 tamsayı aritmetiği için özel birim sistemi kullanır:

### 8.1 Temel Birimler

```
BaseQuantum: Perpetual pozisyonunun en küçük birimi
            BTC-USD için: 1 baseQuantum = 10^(-10) BTC (atomicResolution = -10)

Subtick: Fiyatın en küçük birimi
         BTC-USD için SubticksPerTick = 100_000

Tick: SubticksPerTick * Subtick
```

### 8.2 Fiyat → Notional Dönüşümü

```
quantumConversionExponent (QCE):
  notional_in_quote_quantums = base_quantums * subticks * 10^QCE

Örnek BTC-USD:
  QCE = -9
  base_quantums = 1_000_000_000 (1 BTC = 10^10 baseQuantum → 10^9 baseQuantum = 0.1 BTC)
  subticks = 50_000_000_000 ($50,000 = 50_000 tick * 1_000_000 subtick/tick)
  notional = 1e9 * 5e10 * 10^(-9) = 5e10 = $50,000
```

Bu hesaplama `types.FillAmountToQuoteQuantums()` ile yapılır ve impact price hesabında kritik rol oynar.

---

## 9. Daemon Servisleri

dYdX V4'te bazı işler ana node'dan ayrı goroutine'lerde çalışır:

### 9.1 Liquidation Daemon

```
liquidation daemon (goroutine)
    ↓ Her blokta likit edilebilir subaccount'ları hesapla
    ↓ DaemonLiquidationInfo güncelle
    ↓ PrepareCheckState'de keeper bu listeyi okur
```

### 9.2 Price Feed Daemon

```
pricefeed daemon (goroutine)
    ↓ Harici fiyat kaynaklarından fiyat topla (Binance, Coinbase vs.)
    ↓ x/prices modülüne ValidateProposal aracılığıyla yaz
    ↓ Validator'lar oylarını MsgAddPremiumVotes olarak gönderir
```

### 9.3 Bridge Daemon

```
bridge daemon (goroutine)
    ↓ Ethereum event'lerini izle (USDC deposit/withdraw)
    ↓ x/bridge modülüne bildir
```

---

## 10. Nova Shield DEX İçin Çıkarımlar

### 10.1 Mimari Kararlar

| dYdX Yaklaşımı | Nova Shield Uyarlaması |
|---------------|----------------------|
| Cosmos SDK modülü | Rust custom runtime modülü (`crates/dex/`) |
| KV Store (IAVL) | RocksDB (zaten seçilmiş) |
| MemClob in-memory | `Arc<RwLock<HashMap>>` veya DashMap |
| Transient store | Rust'ta `thread_local!` veya block-scoped state |
| Daemon services | Tokio task'lar |
| CometBFT ABCI | Custom consensus hook'ları |

### 10.2 ABCI Hook Eşleştirmesi

```rust
// Nova Shield DEX Rust mimarisi

pub trait DexModule {
    // dYdX BeginBlocker ≈
    fn begin_block(&mut self, ctx: &BlockContext) -> Result<()>;

    // dYdX PrepareCheckState ≈
    fn prepare_mempool(&mut self, ctx: &BlockContext) -> Result<()>;

    // dYdX EndBlocker ≈
    fn end_block(&mut self, ctx: &BlockContext) -> Result<Vec<Event>>;

    // dYdX AnteHandler ≈
    fn validate_order(&self, order: &Order) -> Result<()>;

    // dYdX PlaceOrder ≈
    fn place_order(&mut self, ctx: &TxContext, order: Order) -> Result<OrderStatus>;
}
```

### 10.3 Modül Bağımlılık Haritası (Nova Shield)

```
dex/
├── orderbook/     ← dYdX MemClob karşılığı
├── matching/      ← dYdX mustPerformTakerOrderMatching karşılığı
├── perpetuals/    ← dYdX x/perpetuals karşılığı
├── funding/       ← dYdX x/perpetuals/funding karşılığı
├── subaccounts/   ← dYdX x/subaccounts karşılığı
├── liquidations/  ← dYdX x/clob/keeper/liquidations karşılığı
└── oracle/        ← dYdX x/prices karşılığı
```

### 10.4 Kritik Implementasyon Notları

1. **Restart Recovery:** MemClob restart'ta boşalır. `PrepareCheckState`'de state'den yeniden doldurulmalıdır. Nova Shield da aynı pattern'i uygulamalı: RocksDB'deki stateful order'ları startup'ta okuyarak in-memory yapıyı doldur.

2. **Post-Only İlk Geçiş:** Post-only emirlerin önce yerleştirilmesi kritik. Normal emirlerin bunlarla match edilebilmesi için sıra önemli.

3. **Bidirectional Dependency:** DEX ↔ Perpetuals arasındaki döngüsel bağımlılık Rust'ta `Arc<Mutex<Box<dyn Trait>>>` veya `Weak<>` referanslarla çözülmeli.

4. **Block-scoped State:** Transient store'un Rust eşdeğeri: `ThreadLocalKey` veya `block_local!` makro.

5. **Determinism:** Cosmos SDK ABCI deterministiktir çünkü state geçişleri sıralı uygulanır. Nova Shield'da da aynı deterministik sıralama sağlanmalı.

---

## 11. Precommit Hook (Yeni Cosmos SDK Özelliği)

```go
// x/clob/abci.go:49-64
func Precommit(ctx sdk.Context, keeper keeper.Keeper) {
    // StagedFinalizeBlockEvents işle
    // Bu eventler FinalizeBlock sırasında sıraya alınır ama
    // MemClob etkisi Precommit'te uygulanır
    keeper.ProcessStagedFinalizeBlockEvents(ctx)

    if streamingManager := keeper.GetFullNodeStreamingManager(); !streamingManager.Enabled() {
        return
    }
    keeper.StreamBatchUpdatesAfterFinalizeBlock(ctx)
}
```

**Ne işe yarıyor:** FinalizeBlock sırasında yeni ClobPair oluşturulursa, bunu MemClob'a hemen ekleyemeyiz (MemClob CheckTx'e özel). Staging mekanizması bu olayları Precommit'e erteler.

---

## 12. Rate Limiting (Block Rate Limit)

dYdX V4 MEV ve spam koruması için blok başına order sayısını sınırlar:

```go
// x/clob/types/block_rate_limit_config.go
type BlockRateLimitConfiguration struct {
    MaxShortTermOrdersPerNBlocks        []MaxPerNBlocksRateLimit
    MaxStatefulOrdersPerNBlocks         []MaxPerNBlocksRateLimit
    MaxShortTermOrderCancellationsPerNBlocks []MaxPerNBlocksRateLimit
}

type MaxPerNBlocksRateLimit struct {
    NumBlocks uint32
    Limit     uint32
}

// Örnek:
// MaxShortTermOrdersPerNBlocks = [{NumBlocks: 1, Limit: 200}]
// → Her blokta en fazla 200 short-term order
```

### Rate Limit Ante Decorator

```go
// x/clob/ante/rate_limit.go (basit özet)
type RateLimitDecorator struct {
    clobKeeper types.ClobKeeper
}

func (rld RateLimitDecorator) AnteHandle(ctx, tx, simulate, next) {
    // Emir sayısını kontrol et, limiti geçerse hata dön
    if err := rld.clobKeeper.RateLimit(ctx, msg); err != nil {
        return ctx, err
    }
    return next(ctx, tx, simulate)
}
```

---

## 13. İzinler ve Yönetişim

dYdX V4 merkezi yönetim için `authorities` pattern kullanır:

```go
type Keeper struct {
    authorities map[string]struct{}  // Authorized addresses set
}

func (k Keeper) HasAuthority(authority string) bool {
    _, ok := k.authorities[authority]
    return ok
}
```

Market yaratma, parametreleri güncelleme gibi işlemler sadece `authorities` listesindeki adreslerce yapılabilir. Bu genellikle governance modülünün adresi veya multisig'dir.

---

## 14. Özet Tablo: dYdX vs Nova Shield Mimari

| Bileşen | dYdX V4 | Nova Shield |
|---------|---------|------------|
| Consensus | CometBFT | Custom (veya adapte) |
| State DB | IAVL (Cosmos KV) | RocksDB |
| Language | Go | Rust |
| Order Book | In-memory Go map | In-memory Rust HashMap/BTreeMap |
| Perpetuals | x/perpetuals Cosmos module | nova_perpetuals crate |
| Funding Rate | 8-saat epoch, median premium | Aynı formül, Rust impl. |
| Collateral | x/subaccounts modülü | nova_subaccounts crate |
| Oracle | x/prices + daemon | zkTLS oracle veya MPC oracle |
| Liquidation | Background daemon + PrepareCheckState | Tokio background task |
| MEV Protection | OperationsToPropose flow | Custom proposer validation |

---

## Sonraki Dosya

**DY-02** → x/clob MemClob Modülü: Orderbook struct, order placement, matching engine giriş noktası, operations queue mekanizması.
