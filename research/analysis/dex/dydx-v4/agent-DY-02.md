# dYdX V4 Derinlemesine Analiz — Dosya 02
## x/clob MemClob Modülü: Orderbook Yapısı, Order Placement ve Operations Queue

**Analiz Tarihi:** 2026-07-22
**Kaynak Dosyalar:**
- `protocol/x/clob/memclob/memclob.go` (2605 satır)
- `protocol/x/clob/memclob/orderbook.go` (603 satır)
- `protocol/x/clob/types/clob_pair.go`
- `protocol/x/clob/types/order_id.go`
- `protocol/x/clob/types/keys.go`

---

## 1. MemClob: Neden In-Memory?

dYdX V4'te CLOB (Central Limit Order Book) state **blok başına kesinleşir** ama **matching işlemi blok içinde bellekte gerçekleşir**. Bunu mümkün kılan `MemClobPriceTimePriority` struct'ıdır.

**Neden in-memory?**
- State yazma (IAVL tree) pahalıdır. Her emir eşleşmesini state'e yazmak imkansız.
- CheckTx sırasında deterministik eşleşme yapılması gerekir.
- PrepareProposal'da proposer'ın lokal görüntüsü alınır.

**Trade-off:** In-memory veri restart'ta kaybolur → `PrepareCheckState` bunu çözer.

---

## 2. MemClobPriceTimePriority Struct

```go
// protocol/x/clob/memclob/memclob.go:35-50
type MemClobPriceTimePriority struct {
    // Her ClobPairId için bir Orderbook tutar
    orderbooks map[types.ClobPairId]*Orderbook

    // Bir sonraki blokta önerilecek operasyonlar
    operationsToPropose types.OperationsToPropose

    // ClobKeeper referansı (bidirectional dependency)
    clobKeeper types.MemClobKeeper

    // Indexer için off-chain güncelleme mesajları üret mi?
    generateOffchainUpdates bool

    // Full-node streaming için orderbook snapshot'ları üret mi?
    generateOrderbookUpdates bool
}
```

### 2.1 Singleton vs Multi-Orderbook

MemClob **birden fazla** orderbook tutabilir. Her `ClobPairId` için ayrı bir `Orderbook` struct'ı vardır:

```go
// BTC-USD için clobPairId=0, ETH-USD için clobPairId=1, vs.
m.orderbooks[types.ClobPairId(0)]  // BTC-USD orderbook
m.orderbooks[types.ClobPairId(1)]  // ETH-USD orderbook
```

### 2.2 Constructor

```go
// memclob.go:57-66
func NewMemClobPriceTimePriority(
    generateOffchainUpdates bool,
) *MemClobPriceTimePriority {
    return &MemClobPriceTimePriority{
        orderbooks:              make(map[types.ClobPairId]*Orderbook),
        operationsToPropose:     *types.NewOperationsToPropose(),
        generateOffchainUpdates: generateOffchainUpdates,
        generateOrderbookUpdates: false,
    }
}
```

---

## 3. Orderbook Struct: Detaylı Analiz

```go
// protocol/x/clob/memclob/orderbook.go:14-53
type Orderbook struct {
    // Fiyat granularitesi: SubticksPerTick kadar subtick = 1 tick
    SubticksPerTick types.SubticksPerTick

    // Fiyat seviyesi (subticks) → emir listesi eşleşmesi
    Bids map[types.Subticks]*types.Level  // Alış emirleri
    Asks map[types.Subticks]*types.Level  // Satış emirleri

    // En iyi fiyatlar (O(1) erişim için cache)
    BestBid types.Subticks  // 0 = bid yok
    BestAsk types.Subticks  // math.MaxUint64 = ask yok

    // Subaccount bazında açık emirler (collateral check için)
    SubaccountOpenClobOrders map[satypes.SubaccountId]map[types.Order_Side]map[types.OrderId]bool

    // Minimum emir boyutu
    MinOrderBaseQuantums satypes.BaseQuantums

    // Reduce-only emirler (pozisyon kapama)
    SubaccountOpenReduceOnlyOrders map[satypes.SubaccountId]map[types.OrderId]bool

    // Toplam açık emir sayısı (metrik için)
    TotalOpenOrders uint

    // OrderId → LevelOrder referansı (O(1) silme için)
    orderIdToLevelOrder map[types.OrderId]*types.LevelOrder

    // Blok numarası → süresi dolacak emirler (short-term için)
    blockExpirationsForOrders map[uint32]map[types.OrderId]bool

    // İptal edilmiş emirler: orderId → iptal bloğu
    orderIdToCancelExpiry map[types.OrderId]uint32

    // Blok → iptal edilen emirler (tersine indexleme)
    cancelExpiryToOrderIds map[uint32]map[types.OrderId]bool
}
```

### 3.1 Level ve LevelOrder

```
Level struct (types/level.go):
    - Bağlı liste: en iyi fiyat önünde (Price-Time Priority)
    - Her fiyat seviyesi için FIFO kuyruk

LevelOrder struct:
    - Value: OrderWithSubaccountId (gerçek emir)
    - Prev, Next: bağlı liste pointer'ları
```

### 3.2 BestBid / BestAsk Cache

```go
// orderbook.go:63-72
func (ob *Orderbook) GetMidPrice() (midPrice types.Subticks, exists bool) {
    if ob.BestBid == 0 || ob.BestAsk == math.MaxUint64 {
        return 0, false  // Orderbook bir tarafı boş
    }
    return ob.BestBid + (ob.BestAsk-ob.BestBid)/2, true
}
```

**Sıfır ve MaxUint64 sentinel değerleri:** BestBid=0 "bid yok", BestAsk=MaxUint64 "ask yok" anlamına gelir. Bu değerler match kontrolünde de kullanılır.

---

## 4. Orderbook Oluşturma: CreateOrderbook

```go
// memclob.go:210-244
func (m *MemClobPriceTimePriority) CreateOrderbook(clobPair types.ClobPair) {
    clobPairId := clobPair.GetClobPairId()

    // Aynı ID ile ikinci kez oluşturma → panic
    if _, exists := m.orderbooks[clobPairId]; exists {
        panic(fmt.Sprintf("Orderbook for ClobPair ID %d already exists", clobPairId))
    }

    subticksPerTick := clobPair.GetClobPairSubticksPerTick()
    if subticksPerTick == 0 {
        panic("subticksPerTick must be greater than zero")
    }

    minOrderBaseQuantums := clobPair.GetClobPairMinOrderBaseQuantums()
    if minOrderBaseQuantums == 0 {
        panic("minOrderBaseQuantums must be greater than zero")
    }

    m.orderbooks[clobPairId] = &Orderbook{
        Asks:                           make(map[types.Subticks]*types.Level),
        BestAsk:                        math.MaxUint64,  // Boş sentinel
        BestBid:                        0,               // Boş sentinel
        Bids:                           make(map[types.Subticks]*types.Level),
        MinOrderBaseQuantums:           minOrderBaseQuantums,
        SubaccountOpenClobOrders:       make(map[satypes.SubaccountId]map[types.Order_Side]map[types.OrderId]bool),
        SubticksPerTick:                subticksPerTick,
        SubaccountOpenReduceOnlyOrders: make(map[satypes.SubaccountId]map[types.OrderId]bool),
        orderIdToLevelOrder:            make(map[types.OrderId]*types.LevelOrder),
        blockExpirationsForOrders:      make(map[uint32]map[types.OrderId]bool),
        orderIdToCancelExpiry:          make(map[types.OrderId]uint32),
        cancelExpiryToOrderIds:         make(map[uint32]map[types.OrderId]bool),
    }
}
```

**SubticksPerTick Monoton Azalma Kuralı:**

```go
// memclob.go:182-208 SyncOrderbookState
if subticksPerTick > orderbook.SubticksPerTick ||
    orderbook.SubticksPerTick%subticksPerTick != 0 {
    panic("SubticksPerTick increased and/or not a divisor of the previous")
}
```

SubticksPerTick sadece azaltılabilir ve önceki değerin bölenlerinden biri olmalıdır. Bu kural, mevcut emirlerin hâlâ geçerli olmasını sağlar (her subticks değeri yeni granülaritede de geçerliyse fiyat dengesi bozulmaz).

---

## 5. PlaceOrder: Tam Akış Analizi

`PlaceOrder` main agent tarafından `CheckTx`'te short-term emirler için çağrılır:

```go
// memclob.go:498-722
func (m *MemClobPriceTimePriority) PlaceOrder(
    ctx sdk.Context,
    order types.Order,
) (
    orderSizeOptimisticallyFilledFromMatchingQuantums satypes.BaseQuantums,
    orderStatus types.OrderStatus,
    offchainUpdates *types.OffchainUpdates,
    err error,
)
```

### 5.1 Adım Adım Akış

**Adım 1: CheckTx Modu Doğrulama**
```go
lib.AssertCheckTxMode(ctx)
// DeliverTx sırasında PlaceOrder çağrılmamalı!
// Eşleşme yalnızca CheckTx'te in-memory gerçekleşir.
```

**Adım 2: Invariant Kontrolü (Deferred)**
```go
defer func() {
    bestBid, hasBid := orderbook.getBestOrderOnSide(true)
    bestAsk, hasAsk := orderbook.getBestOrderOnSide(false)
    if hasBid && hasAsk && bestBid.Value.Order.Subticks >= bestAsk.Value.Order.Subticks {
        panic("PlaceOrder: orderbook is crossed")
    }
}()
// PlaceOrder sonunda orderbook çakışmamış olmalı
// Bu defer, her zaman çalışarak invariant'ı zorlar
```

**Adım 3: validateNewOrder**
```go
if err := m.validateNewOrder(ctx, order); err != nil {
    return 0, 0, offchainUpdates, err
}
```

Validasyon kontrolleri (memclob.go:1419-1533):
1. İptal edilmiş emir mi? (`goodTilBlock >= cancel.goodTilBlock`)
2. Replacement emir mi ve kurallar karşılanıyor mu?
3. Kalan miktar MinOrderBaseQuantums'dan büyük mü?
4. Reduce-only ise doğru tarafta mı?
5. IOC ise zaten kısmen dolmuş mu?

**Adım 4: Off-chain Güncelleme (Indexer için)**
```go
if m.generateOffchainUpdates {
    // Replacement varsa önce remove mesajı gönder
    if _, found := orderbook.getOrder(orderId); found {
        offchainUpdates.AddRemoveMessage(orderId, removeMsg)
    }
    offchainUpdates.AddPlaceMessage(order.OrderId, placeMsg)
}
```

**Adım 5: matchOrder**
```go
takerOrderStatus, takerOffchainUpdates, _, err := m.matchOrder(ctx, &order)
offchainUpdates.Append(takerOffchainUpdates)
```

Bu fonksiyon `mustPerformTakerOrderMatching` çağırır (DY-03'te derinlemesine analiz).

**Adım 6: Hata Kontrolü**
```go
if err != nil {
    // Stateful order için OrderRemoval ekle
    if order.IsStatefulOrder() {
        m.operationsToPropose.MustAddOrderRemovalToOperationsQueue(...)
    }
    return 0, 0, offchainUpdates, err
}
```

**Adım 7: OrderStatus'a Göre Karar**
```go
remainingSize := takerOrderStatus.RemainingQuantums

// Durum 1: Başarısız collateral check
if !takerOrderStatus.OrderStatus.IsSuccess() {
    return optimisticallyFilled, takerOrderStatus.OrderStatus, offchainUpdates, nil
}

// Durum 2: Tamamen doldu
if remainingSize == 0 {
    return optimisticallyFilled, types.Success, offchainUpdates, nil
}

// Durum 3: IOC emir → kalan boyutu iptal et
if order.GetTimeInForce() == types.Order_TIME_IN_FORCE_IOC {
    return optimisticallyFilled, ImmediateOrCancelWouldRestOnBook, offchainUpdates, nil
}

// Durum 4: Kalan boyutu orderbook'a ekle
m.mustAddOrderToOrderbook(ctx, order, false)
return optimisticallyFilled, types.Success, offchainUpdates, nil
```

---

## 6. validateNewOrder: Detaylı İnceleme

```go
// memclob.go:1419-1533
func (m *MemClobPriceTimePriority) validateNewOrder(ctx, order) (err error) {
    orderId := order.OrderId
    orderbook := m.mustGetOrderbook(order.GetClobPairId())

    // Kural 1: Short-term iptal kontrolü
    if orderId.IsShortTermOrder() {
        if cancelTilBlock, cancelExists := orderbook.getCancel(orderId);
            cancelExists && cancelTilBlock >= order.GetGoodTilBlock() {
            return ErrOrderIsCanceled
        }
    }

    // Kural 2: Replacement validation
    existingRestingOrder, restingOrderExists := orderbook.getOrder(orderId)
    existingMatchedOrder, matchedOrderExists := m.operationsToPropose.MatchedOrderIdToOrder[orderId]
    if restingOrderExists {
        if err := validateReplacement(existingRestingOrder, order); err != nil {
            return err
        }
    }
    if matchedOrderExists {
        if err := validateReplacement(existingMatchedOrder, order); err != nil {
            return err
        }
    }

    // Kural 3: Kalan miktar > MinOrderBaseQuantums
    remainingAmount, hasRemainingAmount := m.GetOrderRemainingAmount(ctx, order)
    if !hasRemainingAmount || remainingAmount < orderbook.MinOrderBaseQuantums {
        return ErrOrderFullyFilled
    }

    // Kural 4: Reduce-only doğru tarafta mı?
    if order.IsReduceOnly() {
        existingPositionSize := m.clobKeeper.GetStatePosition(ctx, orderId.SubaccountId, order.GetClobPairId())
        orderSize := order.GetBigQuantums()
        if orderSize.Sign()*existingPositionSize.Sign() != -1 {
            return ErrReduceOnlyWouldIncreasePositionSize
        }
    }

    // Kural 5: IOC zaten kısmen dolmuşsa reject
    if order.RequiresImmediateExecution() && remainingAmount < order.GetBaseQuantums() {
        if restingOrderExists {
            return ErrInvalidReplacement  // Kısmen dolmuşla IOC replace olmaz
        }
        return ErrImmediateExecutionOrderAlreadyFilled
    }

    return nil
}
```

### validateReplacement Kuralları

```go
// memclob.go:1388-1417
func validateReplacement(existing, new types.Order) error {
    // GoodTilBlock/GoodTilBlockTime azaltılamaz
    if existing.MustCmpReplacementOrder(&new) >= 0 {
        return ErrInvalidReplacement
    }
    // Taraf değiştirilemez
    if existing.Side != new.Side {
        return ErrInvalidReplacement
    }
    // TimeInForce değiştirilemez
    if existing.TimeInForce != new.TimeInForce {
        return ErrInvalidReplacement
    }
    // ReduceOnly bayrağı değiştirilemez
    if existing.ReduceOnly != new.ReduceOnly {
        return ErrInvalidReplacement
    }
    return nil
}
```

**Not:** Fiyat ve miktar replacement'ta değişebilir. Ama TTL asla azaltılamaz (yalnızca artırılabilir veya aynı kalabilir).

---

## 7. CancelOrder: Short-Term ve Stateful

### 7.1 Short-Term Cancel (memclob.go:104-157)

```go
func (m *MemClobPriceTimePriority) CancelOrder(ctx, msgCancelOrder) {
    lib.AssertCheckTxMode(ctx)
    orderbook := m.mustGetOrderbook(clobPairId)
    orderIdToCancel := msgCancelOrder.GetOrderId()
    orderIdToCancel.MustBeShortTermOrder()  // Stateful için farklı akış

    oldCancellationGoodTilBlock, cancelAlreadyExists := orderbook.getCancel(orderIdToCancel)
    goodTilBlock := msgCancelOrder.GetGoodTilBlock()

    // Daha yeni bir iptal zaten varsa hata dön
    if cancelAlreadyExists && oldCancellationGoodTilBlock >= goodTilBlock {
        return nil, ErrMemClobCancelAlreadyExists
    }

    // Orderbook'ta varsa ve iptal TTL >= emirin TTL ise kaldır
    if levelOrder, orderExists := orderbook.orderIdToLevelOrder[orderIdToCancel]; orderExists &&
        goodTilBlock >= levelOrder.Value.Order.GetGoodTilBlock() {
        m.mustRemoveOrder(ctx, orderIdToCancel)
    }

    // Eski iptal kaydını sil (varsa)
    if cancelAlreadyExists {
        orderbook.mustRemoveCancel(orderIdToCancel)
    }

    // Yeni iptali ekle
    orderbook.addShortTermCancel(orderIdToCancel, goodTilBlock)
}
```

**Semantik:** Cancel, emirden önce gelebilir (pre-cancel). Bu durumda emir hiç orderbook'a girmeden reject edilir.

---

## 8. Operations Queue: OperationsToPropose

`OperationsToPropose` struct'ı, validator'ın bir sonraki blokta önermek istediği operasyonları tutar:

```go
// types/operations_to_propose.go (özet)
type OperationsToPropose struct {
    // Operasyon kuyruğu (InternalOperation listesi)
    OperationsQueue []InternalOperation

    // OrderId → Order eşleşmesi (matched emirler)
    MatchedOrderIdToOrder map[OrderId]Order

    // OrderHash seti (duplicate detection)
    OrderHashesInOperationsQueue map[OrderHash]bool

    // Short-term order tx bytes (validation için)
    ShortTermOrderHashToTxBytes map[OrderHash][]byte
}
```

### 8.1 InternalOperation Türleri

```go
// Bu interface'i implement eden tipler:
type InternalOperation struct {
    Operation isInternalOperation_Operation
}

// 1. Short-term order placement
*InternalOperation_ShortTermOrderPlacement { Order: types.Order }

// 2. Pre-existing stateful order placement
*InternalOperation_PreexistingStatefulOrder { *OrderId }

// 3. Match (eşleşme)
*InternalOperation_Match { *ClobMatch }

// 4. Order removal
*InternalOperation_OrderRemoval { OrderRemoval }
```

### 8.2 Operations Queue Yaşam Döngüsü

```
CheckTx (PlaceOrder)
    → MustAddShortTermOrderTxBytes()
    → MustAddShortTermOrderPlacementToOperationsQueue()
    → MustAddMatchToOperationsQueue()

PrepareProposal
    → GetOperationsRaw() → []OperationRaw (Protobuf encode)
    → MsgProposedOperations olarak önerilir

ProcessProposal (validators)
    → MsgProposedOperations doğrulanır

FinalizeBlock
    → MsgProposedOperations işlenir, state kesinleşir

PrepareCheckState
    → RemoveAndClearOperationsQueue()  // Temizle
    → ReplayOperations()               // Lokal operasyonları yeniden oynat
```

### 8.3 Replay Mekanizması (ReplayOperations)

```go
// memclob.go:929-1158
func (m *MemClobPriceTimePriority) ReplayOperations(
    ctx, localOperations, shortTermOrderTxBytes, existingOffchainUpdates, postOnlyFilter bool,
) *OffchainUpdates {
    // Panic recovery - replay başarısız olursa chain durmamalı
    defer func() {
        if r := recover(); r != nil {
            log.ErrorLog(ctx, "panic in replay operations", ...)
        }
    }()

    for _, operation := range localOperations {
        switch operation.Operation.(type) {
        case *InternalOperation_ShortTermOrderPlacement:
            order := operation.GetShortTermOrderPlacement().Order
            // Post-only filter
            if postOnlyFilter != order.IsPostOnlyOrder() {
                continue
            }
            // ctx'e tx bytes ekle
            ctx = ctx.WithTxBytes(shortTermOrderTxBytes[order.GetOrderHash()])
            // Yeniden yerleştir
            _, _, _, err = m.clobKeeper.ReplayPlaceOrder(ctx, msg)

        case *InternalOperation_PreexistingStatefulOrder:
            // State'den stateful emiri oku ve yerleştir
            statefulOrderPlacement, found := m.clobKeeper.GetLongTermOrderPlacement(ctx, *orderId)
            if !found { continue }  // Geçmiş blokta kaldırılmış olabilir
            m.clobKeeper.AddPreexistingStatefulOrder(ctx, &statefulOrderPlacement.Order, m)

        case *InternalOperation_Match:
            // No-op: Match'ler yeniden oynatılmaz, sadece emirler
            // Emirler yerleştirilince match otomatik gerçekleşir

        case *InternalOperation_OrderRemoval:
            // Kaldırılan emir yeniden yerleştirmeye çalış ("second chance")
            statefulOrderPlacement, found := m.clobKeeper.GetLongTermOrderPlacement(ctx, orderId)
            if !found { continue }
            m.PlaceOrder(ctx, statefulOrderPlacement.Order)
        }
    }

    existingOffchainUpdates.CondenseMessagesForReplay()
    return existingOffchainUpdates
}
```

**"Second Chance" Mantığı:** Order Removal operasyonu olan bir emir, bir sonraki blokta tekrar yerleştirilmeye çalışılır. Bu, block proposer'ın emirleri kaldırmasını, ama lokal validator'ın yeniden match etmesini sağlar.

---

## 9. PurgeInvalidMemclobState

```go
// memclob.go:1278-1386
func (m *MemClobPriceTimePriority) PurgeInvalidMemclobState(
    ctx,
    filledOrderIds []OrderId,          // FinalizeBlock'ta doldurulan emirler
    expiredStatefulOrderIds []OrderId, // EndBlock'ta süresi dolan emirler
    canceledStatefulOrderIds []OrderId,// DeliverTx'te iptal edilen stateful emirler
    removedStatefulOrderIds []OrderId, // FinalizeBlock'ta zorla kaldırılan emirler
    existingOffchainUpdates,
) {
    // 1. Tamamen dolmuş emirleri kaldır
    for _, orderId := range filledOrderIds {
        m.RemoveOrderIfFilled(ctx, orderId)
    }

    // 2. İptal edilen stateful emirleri kaldır
    for _, statefulOrderId := range DedupeSlice(canceledStatefulOrderIds) {
        if orderbook.hasOrder(statefulOrderId) {
            m.mustRemoveOrder(ctx, statefulOrderId)
        }
    }

    // 3. Süresi dolan stateful emirleri kaldır + indexer mesajı gönder
    for _, statefulOrderId := range DedupeSlice(expiredStatefulOrderIds) {
        if orderbook.hasOrder(statefulOrderId) {
            m.mustRemoveOrder(ctx, statefulOrderId)
            offchainUpdates.AddRemoveMessage(statefulOrderId, ...)
        }
    }

    // 4. Süresi dolan short-term emirleri kaldır (blok bazlı TTL)
    for _, orderbook := range m.orderbooks {
        if blockExpirations, beExists := orderbook.blockExpirationsForOrders[blockHeight]; beExists {
            for shortTermOrderId := range blockExpirations {
                m.mustRemoveOrder(ctx, shortTermOrderId)
            }
        }
    }

    // 5. Zorla kaldırılan stateful emirleri kaldır
    for _, statefulOrderId := range removedStatefulOrderIds {
        if orderbook.hasOrder(statefulOrderId) {
            m.mustRemoveOrder(ctx, statefulOrderId)
        }
    }

    // 6. Süresi dolan short-term iptallerini temizle
    for _, orderbook := range m.orderbooks {
        orderbook.removeAllCancelsAtBlock(blockHeight)
    }
}
```

---

## 10. mustRemoveOrder: O(1) Silme Analizi

```go
// memclob.go:2002-2039
func (m *MemClobPriceTimePriority) mustRemoveOrder(ctx, orderId) {
    orderbook := m.mustGetOrderbook(types.ClobPairId(orderId.GetClobPairId()))

    // O(1) lookup: orderIdToLevelOrder haritası
    levelOrder, exists := orderbook.orderIdToLevelOrder[orderId]
    if !exists {
        panic("mustRemoveOrder: order does not exist")
    }

    // Bağlı listeden çıkar (O(1) çünkü pointer var)
    orderbook.mustRemoveOrder(levelOrder)

    // Short-term order ise ve operations queue'da yoksa tx bytes'ı sil
    order := levelOrder.Value.Order
    if order.IsShortTermOrder() &&
        !m.operationsToPropose.IsOrderPlacementInOperationsQueue(order) {
        m.operationsToPropose.RemoveShortTermOrderTxBytes(order)
    }

    // Full-node streaming varsa orderbook update gönder
    if m.generateOrderbookUpdates {
        orderbookUpdate := m.GetOrderbookUpdatesForOrderRemoval(ctx, order.OrderId)
        m.clobKeeper.SendOrderbookUpdates(ctx, orderbookUpdate)
    }
}
```

**O(1) Silme Neden Mümkün?**
- `orderIdToLevelOrder` haritası `OrderId → *LevelOrder` pointer'ı tutar
- `LevelOrder.Prev` ve `LevelOrder.Next` bağlı liste pointer'ları
- Tek operasyonda hem haritadan sil, hem listeden çıkar

---

## 11. findNextBestLevelOrder: Price-Time Priority Traversal

```go
// orderbook.go:77-109
func (ob *Orderbook) findNextBestLevelOrder(levelOrder *types.LevelOrder) (
    nextBestLevelOrder *types.LevelOrder,
    foundOrder bool,
) {
    // Önce aynı fiyat seviyesindeki bir sonraki emiri dene (Time Priority)
    if levelOrder.Next != nil {
        return levelOrder.Next, true
    }

    // Aynı seviyede başka emir yok → bir sonraki fiyat seviyesine geç (Price Priority)
    order := levelOrder.Value.Order
    subticks := order.GetOrderSubticks()
    isBuy := order.IsBuy()

    nextBestSubticks, foundOrder := ob.findNextBestSubticks(subticks, isBuy)
    if !foundOrder {
        return nil, false
    }

    nextBestLevelOrder, foundOrder = ob.getFirstOrderAtSideAndSubticks(isBuy, nextBestSubticks)
    return nextBestLevelOrder, foundOrder
}
```

**Price-Time Priority Kuralı:**
1. Aynı fiyattaki emirler FIFO sırasıyla match edilir (Time Priority)
2. Farklı fiyatlar en iyi fiyattan başlayarak işlenir (Price Priority)
   - Bid tarafı: En yüksek subticks önce
   - Ask tarafı: En düşük subticks önce

---

## 12. mustUpdateOrderbookStateWithMatchedMakerOrder

```go
// memclob.go:2041-2077
func (m *MemClobPriceTimePriority) mustUpdateOrderbookStateWithMatchedMakerOrder(
    ctx, makerOrder types.Order,
) *types.OffchainUpdates {
    offchainUpdates := types.NewOffchainUpdates()
    makerOrderBaseQuantums := makerOrder.GetBaseQuantums()

    // Güncel toplam dolu miktarı al (state'den, optimistic)
    newTotalFilledAmount := m.GetOrderFilledAmount(ctx, makerOrder.OrderId)

    // Invariant: fill > size olamaz
    if newTotalFilledAmount > makerOrderBaseQuantums {
        panic("Total filled size of maker order greater than the order size")
    }

    // Tamamen dolduğunda orderbook'tan kaldır
    if newTotalFilledAmount == makerOrderBaseQuantums {
        m.mustRemoveOrder(ctx, makerOrder.OrderId)
        // Not: Matched emirler için ShortTermOrderTxBytes burada kaldırılmaz
    }

    // Indexer güncelleme mesajı
    if m.generateOffchainUpdates {
        offchainUpdates.AddUpdateMessage(makerOrder.OrderId, updateMsg)
    }

    return offchainUpdates
}
```

---

## 13. Operations Queue'ya Ekleme: MustAddMatchToOperationsQueue

```go
// types/operations_to_propose.go (özet)
func (otp *OperationsToPropose) MustAddMatchToOperationsQueue(
    takerOrder types.MatchableOrder,
    makerFillsWithOrders []types.MakerFillWithOrder,
) types.InternalOperation {
    // ClobMatch oluştur
    var clobMatch *ClobMatch

    if takerOrder.IsLiquidation() {
        clobMatch = &ClobMatch{Match: &ClobMatch_MatchPerpetualLiquidation{...}}
    } else {
        clobMatch = &ClobMatch{Match: &ClobMatch_MatchOrders{
            MatchOrders: &MatchOrders{
                TakerOrderId: takerOrder.MustGetOrder().OrderId,
                Fills:        makerFills,
            },
        }}
    }

    internalOp := types.InternalOperation{
        Operation: &InternalOperation_Match{Match: clobMatch},
    }

    otp.OperationsQueue = append(otp.OperationsQueue, internalOp)
    return internalOp
}
```

---

## 14. GetOrderRemainingAmount

```go
// memclob.go:2095-2130
func (m *MemClobPriceTimePriority) GetOrderRemainingAmount(
    ctx, order types.Order,
) (remainingAmount satypes.BaseQuantums, hasRemainingAmount bool) {
    // State'den fill amount oku
    orderStateFilledAmount, hasStateFilledAmount, _ := m.clobKeeper.GetOrderFillAmount(ctx, order.OrderId)

    if !hasStateFilledAmount {
        orderStateFilledAmount = 0
    }

    orderSize := order.GetBaseQuantums()
    if orderStateFilledAmount > orderSize {
        return 0, false  // Fazla dolmuş - invalid
    }
    if orderStateFilledAmount == orderSize {
        return 0, false  // Tamamen dolu
    }

    return orderSize - orderStateFilledAmount, true
}
```

**Önemli:** Fill amount state'de (persistent KV store) tutulur. MemClob'da değil. Bu sayede restart'tan sonra da fill miktarları korunur.

---

## 15. DeleverageSubaccount

```go
// memclob.go:756-790
func (m *MemClobPriceTimePriority) DeleverageSubaccount(
    ctx, subaccountId, perpetualId uint32,
    deltaQuantums *big.Int,
    isFinalSettlement bool,
) (quantumsDeleveraged *big.Int, err error) {
    lib.AssertCheckTxMode(ctx)

    // Subaccount'ın karşıt pozisyon sahibini bul ve eşleştir
    fills, deltaQuantumsRemaining := m.clobKeeper.OffsetSubaccountPerpetualPosition(
        ctx, subaccountId, perpetualId, deltaQuantums, isFinalSettlement,
    )

    // Eşleşme olduysa operations queue'ya ekle
    if len(fills) > 0 {
        m.operationsToPropose.MustAddDeleveragingToOperationsQueue(
            subaccountId, perpetualId, fills, isFinalSettlement,
        )
    }

    // Ne kadar deleveraged edildi?
    quantumsDeleveraged = new(big.Int).Abs(new(big.Int).Sub(deltaQuantums, deltaQuantumsRemaining))
    return quantumsDeleveraged, nil
}
```

**Deleveraging:** Sigorta fonu yetersizse, kârlı pozisyon sahipleri seçilerek zarar yazılır. Bu, sistemi solvency'de tutar.

---

## 16. SetMemclobGauges: Gözlemlenebilirlik

```go
// memclob.go:1936-1999
func (m *MemClobPriceTimePriority) SetMemclobGauges(ctx) {
    for clobPairId, orderbook := range m.orderbooks {
        // Her piyasa için toplam açık emir sayısı
        telemetry.SetGaugeWithLabels(
            []string{types.ModuleName, metrics.TotalOrdersInClob},
            float32(orderbook.TotalOpenOrders),
            []gometrics.Label{metrics.GetLabelForIntValue(metrics.ClobPairId, int(clobPairId))},
        )
        // En iyi bid fiyatı
        telemetry.SetGaugeWithLabels(
            []string{types.ModuleName, metrics.BestBidClobPair},
            float32(orderbook.BestBid),
            ...
        )
        // En iyi ask fiyatı
        telemetry.SetGaugeWithLabels(
            []string{types.ModuleName, metrics.BestAskClobPair},
            float32(orderbook.BestAsk),
            ...
        )
    }
    // Operations queue uzunlukları
    telemetry.SetGauge(float32(len(otp.OperationsQueue)), ..., metrics.OperationsQueueLength)
}
```

---

## 17. Nova Shield Rust İmplementasyon Notları

### 17.1 Orderbook Rust Karşılığı

```rust
use std::collections::{BTreeMap, HashMap};
use std::collections::VecDeque;

pub struct Orderbook {
    // BTreeMap: price-sorted O(log n) insertion/deletion
    // Bid tarafı: yüksekten düşüğe (max-first), BTreeMap<Reverse<Subticks>, Level>
    // Ask tarafı: düşükten yükseğe (min-first), BTreeMap<Subticks, Level>
    bids: BTreeMap<std::cmp::Reverse<u64>, Level>,
    asks: BTreeMap<u64, Level>,

    // O(1) silme için
    order_id_to_level_order: HashMap<OrderId, *mut LevelOrder>,

    // Blok bazlı TTL (Short-term emirler için)
    block_expirations: HashMap<u32, HashSet<OrderId>>,

    // En iyi fiyat cache
    best_bid: u64,   // 0 = boş
    best_ask: u64,   // u64::MAX = boş

    subticks_per_tick: u32,
    min_order_base_quantums: u64,
    total_open_orders: u32,
}

pub struct Level {
    // FIFO: VecDeque (önden çıkar, arkadan ekle)
    orders: VecDeque<LevelOrder>,
}

pub struct LevelOrder {
    order: Order,
    // Rust'ta bağlı liste yerine index tabanlı yapı daha güvenli
    // unsafe işaretçi yerine arena allocator + index
}
```

**Rust'ta Bağlı Liste Uyarısı:** Go'daki `*LevelOrder` pointer'lar Rust'ta `unsafe` gerektirir. Alternatif:
- `typed-arena` crate ile arena allocation
- `slab` crate ile index tabanlı referanslar
- `std::ptr::NonNull<T>` + `unsafe impl Send`

### 17.2 MemClob Rust Karşılığı

```rust
pub struct MemClob {
    orderbooks: HashMap<u32, Orderbook>,  // clobPairId → Orderbook
    operations_to_propose: OperationsToPropose,
    clob_keeper: Arc<dyn ClobKeeper>,
    generate_offchain_updates: bool,
}
```

### 17.3 PlaceOrder Rust İmzası

```rust
pub fn place_order(
    &mut self,
    ctx: &mut BlockContext,
    order: Order,
) -> Result<(BaseQuantums, OrderStatus, Vec<OffchainUpdate>)> {
    // AssertCheckTxMode
    assert!(ctx.is_check_tx_mode(), "PlaceOrder must be called in CheckTx mode");

    let orderbook = self.must_get_orderbook(order.get_clob_pair_id())?;

    // Validation
    self.validate_new_order(ctx, &order)?;

    // Match
    let (taker_status, offchain_updates) = self.match_order(ctx, &order)?;

    // Order Book'a ekle
    if taker_status.remaining_quantums > 0 && taker_status.is_success() {
        self.must_add_order_to_orderbook(ctx, order, false)?;
    }

    Ok((taker_status.optimistically_filled, taker_status.order_status, offchain_updates))
}
```

### 17.4 Kritik Güvenlik Notları

1. **Checked Arithmetic:** Subticks karşılaştırmalarında (`BestBid >= BestAsk` → crossing) Rust'ta `checked_add`, `saturating_sub` kullan.

2. **Invariant Assertions:** `debug_assert!(best_bid < best_ask || !has_bid || !has_ask)` pattern'i kullan.

3. **Thread Safety:** MemClob `Send + Sync` değil — tek thread'den erişilmeli veya `Mutex<MemClob>` wrap'le.

4. **Arena Allocation:** `LevelOrder` struct'ları için arena allocator kullan, GC pressure'ı önlemek için.

---

## 18. Özet: MemClob Veri Yapıları Karmaşıklık Analizi

| Operasyon | Karmaşıklık | Notlar |
|-----------|------------|--------|
| PlaceOrder | O(log N) amortized | N = fiyat seviyesi sayısı |
| CancelOrder | O(1) | orderIdToLevelOrder + bağlı liste |
| mustRemoveOrder | O(1) | Pointer-based removal |
| findNextBestLevelOrder | O(1) avg | Same level: O(1). Next level: O(1) lookup |
| PurgeInvalidMemclobState | O(K) | K = expire/fill/cancel sayısı |
| ReplayOperations | O(M log N) | M = queue uzunluğu |
| GetOrderRemainingAmount | O(1) | KV store lookup |
| GetMidPrice | O(1) | BestBid/BestAsk cache |

---

## Sonraki Dosya

**DY-03** → Matching Engine: `mustPerformTakerOrderMatching` derinlemesine analiz — çakışma kontrolü, collateral check, self-trade, reduce-only resize, post-only, IOC, branch/writeCache pattern.
