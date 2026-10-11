# CoW Protocol Analiz — Agent 18
## Kapsam: orderbook/src/ — api/ + database/ + dto/ tam derinlik
## DEX Ozel Protokolu: 3x Hassasiyet
## Tarih: 2026-07-22

---

## Genel Bakis: Dosya Listesi

```
orderbook/src/
├── api.rs                         (ana router + middleware + metrik)
├── api/
│   ├── cancel_order.rs            (DELETE /api/v1/orders/{uid})
│   ├── cancel_orders.rs           (DELETE /api/v1/orders — batch)
│   ├── debug_order.rs             (GET /restricted/api/v1/debug/order/{uid})
│   ├── debug_simulation.rs        (GET+POST /restricted/api/v1/debug/simulation)
│   ├── get_app_data.rs            (GET /api/v1/app_data/{hash})
│   ├── get_auction.rs             (GET /api/v1/auction)
│   ├── get_native_price.rs        (GET /api/v1/token/{token}/native_price)
│   ├── get_order_by_uid.rs        (GET /api/v1/orders/{uid})
│   ├── get_order_status.rs        (GET /api/v1/orders/{uid}/status)
│   ├── get_orders_by_tx.rs        (GET /api/v1/transactions/{hash}/orders)
│   ├── get_orders_by_uid.rs       (POST /api/v1/orders/by_uids — batch get)
│   ├── get_solver_competition_v2.rs (GET /api/v2/solver_competition/*)
│   ├── get_token_metadata.rs      (GET /api/v1/token/{token}/metadata)
│   ├── get_total_surplus.rs       (GET /api/v1/users/{user}/total_surplus)
│   ├── get_trades.rs              (GET /api/v1/trades — v1, no pagination)
│   ├── get_trades_v2.rs           (GET /api/v2/trades — paginated)
│   ├── get_user_orders.rs         (GET /api/v1/account/{owner}/orders)
│   ├── post_order.rs              (POST /api/v1/orders — kritik endpoint)
│   ├── post_quote.rs              (POST /api/v1/quote)
│   ├── post_quote_stream.rs       (POST /api/v1/quote/stream — SSE)
│   ├── put_app_data.rs            (PUT /api/v1/app_data[/{hash}])
│   ├── ready.rs                   (GET /api/v1/ready — liveness probe)
│   └── version.rs                 (GET /api/v1/version)
├── database/
│   ├── mod.rs                     (Postgres pool + Config + shared helpers)
│   ├── app_data.rs                (app data CRUD)
│   ├── auction_prices.rs          (son fiyatlari cek)
│   ├── auctions.rs                (auction CRUD)
│   ├── debug_report.rs            (debug raporu olustur)
│   ├── fee_policies.rs            (protocol fee hesabi)
│   ├── orders.rs                  (order CRUD — en buyuk dosya ~900 satir)
│   ├── quotes.rs                  (quote CRUD)
│   ├── solver_competition.rs      (rekabet verileri v1)
│   ├── solver_competition_v2.rs   (rekabet verileri v2)
│   ├── total_surplus.rs           (surplus SQL sorgusu)
│   └── trades.rs                  (trade sorgulari)
└── dto/
    ├── mod.rs                     (OrderSimulationRequest, OrderSimulationResult, TokenMetadata)
    ├── auction.rs                 (Auction, AuctionWithId DTO)
    └── order.rs                   (Order, Quote, FeePolicy, Status DTO)
```

Toplam: ~49 dosya, bu agent kapsaminda 24 Rust dosyasi tam analiz.

---

## BOLUM 1: api/ — Tam Derinlik Analiz

---

### [api.rs] (1019 satir)

**Amac:** Tum API rotalarini tek yerde tanimlar; CORS, metrik, trace, body limiti middleware zinciri kurar; AppState shared state'i olusturur.

**Struct: AppState**
```rust
pub struct AppState {
    pub database_write: Postgres,  // yazma islemleri icin birincil DB
    pub database_read: Postgres,   // salt-okunur replica
    pub orderbook: Arc<Orderbook>, // is mantigi katmani
    pub quotes: QuoteHandler,
    pub app_data: Arc<app_data::Registry>,
    pub native_price_estimator: Arc<dyn NativePriceEstimating>,
    pub quote_timeout: Duration,
    pub current_block_stream: CurrentBlockWatcher,
    pub hide_competition_before_deadline: bool,
}
```

**Kritik fn: hide_competition_before_block()**
- `hide_competition_before_deadline` flag'i true ise mevcut blok numarasini dondurur
- Circuit breaker icin: deadline gecmemis rekabet verileri gizlenir
- Etkilenen endpoint'ler: /api/v2/solver_competition/* (public)
- Gizlenmez: /restricted/api/v2/solver_competition/* (internal)

**Guvenlk Katmani: MAX_JSON_BODY_PAYLOAD = 16 KB**
- Satir 153: `const MAX_JSON_BODY_PAYLOAD: u64 = 1024 * 16;`
- Bu limit tum router'a `DefaultBodyLimit::max()` ile uygulanir
- Istisna: `/api/v1/app_data` ve `/api/v1/app_data/{hash}` — bunlar ayri limit kullanir (`app_data_size_limit`, validatordan gelir, varsayilan 8 KB)
- Guvenlik degerlendirmesi: Body boyutu limiti uygulanmis. Fakat bu yalnizca ham bayt limitidir; JSON depth ya da alan sayisi sinirlanmamistir.

**CORS Politikasi:**
```rust
CorsLayer::new()
    .allow_origin(tower_http::cors::Any)   // TUM origin'lere izin ver
    .allow_methods([GET, POST, DELETE, OPTIONS, PUT, PATCH, HEAD])
    .allow_headers([ORIGIN, CONTENT_TYPE, "x-auth-token", "x-appid"])
```
- `allow_origin(Any)`: Son derece permissive — herhangi bir domain API'ye erisebilir
- `x-auth-token` ve `x-appid` header'lari izinli ama sunucu tarafinda dogrulanmiyor (hic auth yoktur)

**Rate Limiting: YOK**
- Middleware zincirine bakildiginda (satir 344-351): DefaultBodyLimit + CorsLayer + summarize_request + with_matched_path_metric + TraceLayer
- Hicbir middleware rate limitleme yapmaz
- Bu onceki agentle uyumlu — rate limiting uygulamanin disinda, WAF/infra katmaninda

**Middleware Zinciri (iccten disa):**
1. TraceLayer (distributed tracing, span olusturur)
2. `with_matched_path_metric` (Prometheus sayaci: requests_complete, requests_rejected, requests_duration_seconds)
3. `summarize_request` (method+uri+user_agent+status+elapsed loglama)
4. CorsLayer
5. DefaultBodyLimit (16 KB)

**ApiMetrics struct:**
```rust
struct ApiMetrics {
    requests_complete: IntCounterVec,   // labels: method+path, status_code
    requests_rejected: IntCounterVec,   // labels: status_code
    requests_duration_seconds: HistogramVec, // labels: method+path; buckets: 0.1,0.5,1,2,4,6,8,10s
}
```
- Tum metrikler baslangicta sifirlanir (Grafana'da "no data" boslugu engellenir)

**Error struct:**
```rust
pub struct Error {
    pub error_type: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub data: Option<serde_json::Value>,  // skip_serializing_if = None
}
```
- camelCase JSON: `{"errorType": "...", "description": "...", "data": ...}`
- `rich_error()`: ek data alani ile; seri hata durumunda data=None (sessizce devam eder)

**Restricted Endpoint'ler:**
- `/restricted/api/v*/*` prefixli endpoint'ler WAF/infra seviyesinde korunur
- Kod icinde auth yoktur; infra erisi kontrolune guvenir
- Bunlar: debug/order, debug/simulation, solver_competition (filtresiz)

**Test Coverage:**
- Path parsing (OrderUid, Address, B256, AppDataHash, u64) — 0x prefix ile/olmadan, hatalı hex, boyut asimi
- Query parsing (camelCase hex typler, numeric offset/limit)
- rich_error serialization edge cases

**DEX Uygulama Notu — Nova Shield icin:**
- CoW'un CORS politikasi cok gevşek. Nova Shield icin: yalnizca izin verilen domainleri whitelist'e al
- Rate limiting mutlaka uygulama katmaninda (tower-governor veya benzeri middleware) olmali; WAF'a guvenmek yeterli degil
- Metrik yapisi (labels: method+path, buckets) Nova Shield icin kopyalanabilir template
- AppState pattern (Arc<> ile shared state, cift DB) production-ready — Nova Shield'da aynen uygulanabilir
- Body limit ayri endpoint'ler icin farkli degerlere ayarlanabilir (app_data gibi)

---

### [api/post_order.rs] (378 satir) — KRITIK DEX ENDPOINT

**Amac:** `POST /api/v1/orders` — order submission endpoint'i; validation hata tiplerini HTTP response'a donusturmek.

**Handler fn: post_order_handler()**
```rust
pub async fn post_order_handler(
    State(state): State<Arc<AppState>>,
    Json(order): Json<OrderCreation>,
) -> Response
```
- Axum JSON extractor ile orderbook'a iletir
- Basarida: 201 CREATED + body: `"0x<order_uid>"` (JSON stringe sarili hex)
- Hata: status code + JSON error body

**Tam Validation Akisi (orderbook.add_order() icinde):**
1. `PartialValidationError` — on-chain gecerliligi gerektirmeyen hizli kontroller:
   - `UnsupportedBuyTokenDestination` — 400
   - `UnsupportedSellTokenSource` — 400
   - `UnsupportedOrderType` — 400
   - `Forbidden` — 403 (deny-listede olan hesap)
   - `ValidTo::Insufficient` — 400 (gecmis tarih veya cok yakin gelecek)
   - `ValidTo::Excessive` — 400 (cok uzak gelecek)
   - `InvalidNativeSellToken` — 400 (ETH/xDai satis tokeni olamaz)
   - `SameBuyAndSellToken` — 400
   - `UnsupportedToken` — 400 (token kara listede)
   - `Other` — 500
2. `AppDataValidationError`:
   - `Invalid` — 400 (app data format gecersiz)
   - `Mismatch` — 400 (hash uyumsuzlugu)
3. `ValidationError` (tam validasyon, on-chain):
   - `MissingFrom` — 400
   - `AppdataFromMismatch` — 400
   - `WrongOwner` — 400 (imza sahibi != from adresi)
   - `InvalidEip1271Signature` — 400 (EIP-1271 sozlesme imzasi gecersiz)
   - `InsufficientBalance` — 400
   - `InsufficientAllowance` — 400 (VaultRelayer allowance yok)
   - `InvalidSignature` — 400
   - `NonZeroFee` — 400 (fee sifir olmali)
   - `SellAmountOverflow` — 500 (U256 tasma)
   - `TransferSimulationFailed` — 400 (token transfer simule edilemedi; revert data dahil)
   - `QuoteNotVerified` — 400 (quote dogrulanamadi)
   - `ZeroAmount` — 400
   - `IncompatibleSigningScheme` — 400
   - `TooManyLimitOrders` — 400
   - `TooMuchGas` — 400
4. `AddOrderError`:
   - `DuplicatedOrder` — 400
   - `Database` — 500
   - `AppDataMismatch` — 500 (imkansiz durum, log'lanir)
   - `InvalidReplacement` — acilir
   - `InvalidAppData` — 400
   - `MetadataSerializationFailed` — 500

**OrderReplacementError (eski order'i replace ederken):**
- `InvalidSignature` — 400
- `WrongOwner` — 401 (eski ve yeni order'larin sahibi farkli)
- `OldOrderActivelyBidOn` — 400 (yakin auctiondaki order replace edilemez)

**Imza Tipleri:**
- EIP-712 (off-chain)
- EIP-1271 (on-chain, sozlesme imzasi — on-chain cagri yapilarak dogrulanir)
- Pre-sign (sozlesme uzerinden imza)
- Ethsign (Ethereum personal_sign)

**Guvenlik Analizi:**
- On-chain imza dogrulamasi (EIP-1271): orderbook on-chain node cagrisina bagimli
- TransferSimulationFailed: token transfer simulation yapiliyor — bu potansiyel latency kaynagi
- Deny-list kontrolu: Forbidden hata kodu ile
- Sifir-fee zorunlulugu: `NonZeroFee` hata kodu — CoW'un fee modeli order sahibinden dogrudan alinmaz
- `TooManyLimitOrders`: sahibi basi limit order quota var (spam'i engeller)
- Hic rate limiting yok — 400 hatasi dondurmek ucuz, ama brute-force signature scanning mumkun

**DEX Uygulama Notu — Nova Shield icin:**
- Imza dogrulama: EIP-712 + EIP-1271 + pre-sign kombinasyonu esnek bir auth mekanizmasi
- Validation katmanini iki asamaya bolmek (partial + full) mantikli: on-chain cagrilar maliyetli
- `TooManyLimitOrders` quota mekanizmasi anti-spam icin kritik — Nova Shield'da uygulanmali
- Replace order semantigi (old cancel + new insert, atomik transaction) akilli tasarim
- `QuoteNotVerified` error: quote sistemi order kabul kanalinin kalitiyle entegre

---

### [api/cancel_order.rs] (146 satir)

**Amac:** `DELETE /api/v1/orders/{uid}` — tek order iptali; imza zorunlu.

**Handler fn: cancel_order_handler()**
```rust
Json(payload): Json<CancellationPayload>
// payload: { signature: EcdsaSignature, signingScheme: "eip712"|"ethsign" }
```
- Basarida: 200 OK + `"Cancelled"` string
- Request body gerektirir: imza + signing scheme

**OrderCancellationError mapping:**
- `InvalidSignature` — 400
- `AlreadyCancelled` — 400
- `OrderFullyExecuted` — 400
- `OrderExpired` — 400
- `OrderNotFound` — 404
- `WrongOwner` — 401 (imza sahibi != order sahibi)
- `OnChainOrder` — 400 (on-chain order'lar on-chain iptal edilmeli)

**Guvenlik:**
- Iptal icin imza zorunlu: anonymous iptal yok
- On-chain order'lari API ile iptal etmeye calisanlar reddedilir (on-chain semantigi korunur)
- WrongOwner: 401 — sahibin imza dogrulamasi var

**Test:**
- CancellationPayload deserializasyon testi (r, s, v + signingScheme)
- Tum error variant'lari icin status code testleri

**DEX Uygulama Notu — Nova Shield icin:**
- Her cancel'da imza isteme pattern'i: bot spam'ini azaltir
- On-chain / off-chain order semantigi farki kodda net ayrilmis — Nova Shield icin model

---

### [api/cancel_orders.rs] (42 satir)

**Amac:** `DELETE /api/v1/orders` (body'de UID listesi) — batch iptal; tek imza ile birden fazla order.

**Kritik limit:**
```rust
if cancellations.data.order_uids.len() > ORDER_UID_LIMIT {
    // ORDER_UID_LIMIT = 1024
    return error response
}
```
- Yorum: body boyutu limiti (16KB) bu limit icin yeterli degil — 1024 * 56 byte = 57 KB, body limitini asiyor
- Bu yuzden acikca sayisal limit konulmustur

**Guvenlik:**
- Batch boyutu sinirlanmis: 1024 order UID
- Body boyutu kontrolunden bagimsiz ek limit — dogru mimari karar

**DEX Uygulama Notu — Nova Shield icin:**
- Body byte limiti ile alan sayisi limiti farklidir — her ikisi de gerekli
- Batch endpointlerde her zaman COUNT limiti ekle (byte limitine guvenmek yaniltici)

---

### [api/get_order_by_uid.rs] (59 satir)

**Amac:** `GET /api/v1/orders/{uid}` — tek order getir.

**Handler:**
- 200 OK + full `Order` JSON
- 404 + `{"errorType":"NotFound","description":"Order was not found"}`
- 500 ic hata

**Test:**
- Mevcut order: 200 + body roundtrip testi
- Olmayan order: 404

**DEX Uygulama Notu — Nova Shield icin:**
- Response tipi `Order` dogrudan model nesnesi; DTO donusumu yok — basit ama API sozlesmesi icin risk (ic degisiklikler disari yansir)

---

### [api/get_orders_by_uid.rs] (123 satir)

**Amac:** `POST /api/v1/orders/by_uids` — JSON body'de UID listesi, batch getir.

**Limit:** ORDER_UID_LIMIT = 1024

**Response format:**
```rust
enum OrderResultEntry {
    Order(Order),             // basarili
    Error { uid, description } // bu UID icin hata
}
```
- Kismi basari mumkun: bazi UID'ler Order, bazilari Error donebilir
- HTTP status: daima 200 (hatalar body icinde gonderilir)
- Hata aciklamasi kasitli olarak gizlenmis: "Internal server error encountered when retrieving the order"

**DEX Uygulama Notu — Nova Shield icin:**
- Batch get endpoint'lerde partial failure pattern iyi: tek hata tum batch'i batirmamali
- "Error" entry'si yerine HTTP multi-status (207) kullanmak daha RESTful olabilir
- Hata mesajinin gizlenmesi (implementation detail'i saklamak) guvenlik icin iyi pratik

---

### [api/get_order_status.rs] (30 satir)

**Amac:** `GET /api/v1/orders/{uid}/status` — canli durum: Open, Scheduled, Active, Solved, Executing, Traded, Cancelled.

**Response:** `Status` enum (dto/order.rs'te tanimli):
```
Open | Scheduled | Active | Solved([SolutionInclusion]) |
Executing([SolutionInclusion]) | Traded([SolutionInclusion]) | Cancelled
```
- `SolutionInclusion`: `{ solver: String, executed_amounts?: {sell, buy} }`

**DEX Uygulama Notu — Nova Shield icin:**
- Order lifecycle state machine 7 durum: bu model batch auction sistemleri icin referans
- "Solved" ile "Traded" ayrimi: hangi cozucunun kazandigini gosterir — sheffaflik icin kritik

---

### [api/get_trades.rs] (116 satir) — v1

**Amac:** `GET /api/v1/trades?orderUid=...` veya `GET /api/v1/trades?owner=...`

**QueryParams:**
```rust
struct QueryParams {
    order_uid: Option<OrderUid>,
    owner: Option<Address>,
}
```

**Validation:**
- EXACTLY ONE of `owner` or `orderUid` zorunlu
- Her ikisi de verilirse: 400 `InvalidTradeFilter`
- Hicbiri verilmezse: 400

**Pagination:** YOK — v1'de tum sonuclar doner (sqlx sorgusu: offset=0, limit=i64::MAX)

**DEX Uygulama Notu — Nova Shield icin:**
- Filtresiz sorgulari engelleme (her iki filtre birden = hata) cok buyuk scan'leri onler
- Pagination eksikligi v1'de sorunlu — v2'de duzeltilmis (asagida)

---

### [api/get_trades_v2.rs] (189 satir) — v2

**Amac:** `GET /api/v2/trades` — paginated.

**Ek parametreler:**
```rust
struct QueryParams {
    order_uid, owner,
    offset: Option<u64>,  // default: 0
    limit: Option<u64>,   // default: 10, min: 1, max: 1000
}
```

**Limit validation:**
```rust
const DEFAULT_LIMIT: u64 = 10;
const MIN_LIMIT: u64 = 1;
const MAX_LIMIT: u64 = 1000;
if !(MIN_LIMIT..=MAX_LIMIT).contains(&limit) → 400 InvalidLimit
```

**DEX Uygulama Notu — Nova Shield icin:**
- Pagination parametrelerinin default/min/max ile sinirlanmasi zorunlu
- v1→v2 evrimini gormek: v1'deki hatalar (pagination yok) v2'de duzeltildi

---

### [api/get_user_orders.rs] (51 satir)

**Amac:** `GET /api/v1/account/{owner}/orders?offset=&limit=`

**Limit:** [1, 1000], default 10

**Path:** `{owner}` = Ethereum adresi

**DEX Uygulama Notu:** Sahibi bazinda order listeleme — standart DEX API pattern'i.

---

### [api/post_quote.rs] (310 satir)

**Amac:** `POST /api/v1/quote` — order gondermeden once fiyat tahmini al.

**Request: OrderQuoteRequest**
- `from`: Address
- `sellToken`, `buyToken`
- `kind`: "sell" | "buy"
- `sellAmountAfterFee` veya `sellAmountBeforeFee` veya `buyAmountAfterFee`
- `validTo` (UNIX ts) veya `validFor` (saniye)
- `appData`: AppDataHash
- `signingScheme`: "eip712" | "ethsign" | "presign"
- `priceQuality`: "fast" | "optimal" | "verified"
- `timeout`: Optional (millisecond)

**Response: OrderQuoteResponse**
- quote: sell/buy amount, fee, gas estimate, gas price, sell token fiyati
- from, expiration, id (quote id), verified bool, protocol_fee_bps

**CalculateQuoteError mapping:**
- `Price(PriceEstimationError)` — PriceEstimationErrorWrapper'a delege
- `SellAmountDoesNotCoverFee` — 400 + rich_error (fee miktarini data olarak verir)
- `QuoteNotVerified` — 400
- `Other` — 500

**Test:** Deserializasyon testleri — sell/buy/after-fee/before-fee kombinasyonlari

**DEX Uygulama Notu — Nova Shield icin:**
- Quote sistemi order submission'dan bagimsiz: kullanici once quote alir, sonra order gonderir
- `priceQuality` parametresi: fast (hizli tahmin) / optimal (solver cozumu) / verified (simule edilmis)
- Bu uclu kalite seviyesi cok degerlendirilebilir Nova Shield'da
- `SellAmountDoesNotCoverFee` hata mesajinda fee miktarini vermek UX icin iyi

---

### [api/post_quote_stream.rs] (49 satir)

**Amac:** `POST /api/v1/quote/stream` — SSE (Server-Sent Events) akisi ile surekligi olan fiyat guncelleme.

**Nasil calisir:**
1. `calculate_quote_stream()` cagirir — stream dondurur
2. Validasyon hatasi varsa normal HTTP error doner
3. Basariliysa `Sse::new(events).keep_alive(KeepAlive::default())` olarak yanit verir
4. Stream icindeki hatalar `event("error")` SSE event'i olarak gonderilir
5. `filter_map` ile JSON serialization hatasi olan event'ler sessizce atlanir

**DEX Uygulama Notu — Nova Shield icin:**
- Streaming quote = gercek zamanli price feed — DEX UI icin cok degerli
- SSE, WebSocket'ten basit (tek yonlu, HTTP/1.1 uyumlu)
- Error event type ile SSE uzerinden hata iletimi: `event: error\ndata: {...}\n\n`
- keep-alive: baglantinin kopmasini engeller
- Nova Shield'da gerekirse WebSocket tercih edilebilir (cift yonlu, daha esnek)

---

### [api/get_auction.rs] (26 satir)

**Amac:** `GET /api/v1/auction` — mevcut aktif auctionu getir.

**Response:** `AuctionWithId { id: i64, block: u64, orders: Vec<dto::Order>, prices: BTreeMap<Address, U256> }`
- Aktif auction yoksa: 404 "There is no active auction"

**DEX Uygulama Notu:** Mevcut batch auction state'ini public API ile expose etmek — solver'lar icin yarisi reference point.

---

### [api/get_native_price.rs] (24 satir)

**Amac:** `GET /api/v1/token/{token}/native_price` — token'in ETH/native token cinsinden anlık fiyati.

**Not:** `quote_timeout` kullanilir — uzun surebilecek fiyat sorgusuna zaman asimi uygulanir.

**Response:** `{ "price": "<U256 decimal>" }` via `NativeTokenPrice { price: f64 }`

---

### [api/get_app_data.rs] (35 satir)

**Amac:** `GET /api/v1/app_data/{hash}` — kisa app_data hash'inden tam JSON dokumani getir.

**Response:** `{ "fullAppData": "..." }` (AppDataDocument)
- 404 "full app data not found"

---

### [api/put_app_data.rs] (70 satir)

**Amac:** `PUT /api/v1/app_data` (hash'siz) ve `PUT /api/v1/app_data/{hash}` (hash dogrulama ile) — app data kaydet.

**Islem akisi:**
1. Belge validator ile dogrulama (JSON schema + boyut limiti)
2. Hash dogrulama (eger saglandiysa): computed hash != provided hash → 400 HashMismatch
3. DB'ye kaydet

**Response:** 201 CREATED (yeni) veya 200 OK (zaten vardi), body: `AppDataHash` hex string

**Hatalar:**
- `AppDataInvalid` — 400
- `AppDataHashMismatch` — 400
- `AppDataMismatch` — 400 (hash var ama farkli data)
- `Other` — 500

**DEX Uygulama Notu — Nova Shield icin:**
- app_data sistemi: metadata (slippage ayarlari, partner bilgisi, referral kodu) EVM'de az yer kaplar
- Hash-content separation: hash on-chain, full content off-chain API'de — alan tasarrufu
- PUT semantigi (idempotent): ayni hash ile ayni data = OK, farkli data = hata

---

### [api/get_solver_competition_v2.rs] (78 satir)

**Amac:** Solver rekabeti endpointleri — hangi solver'lar katildi, hangi skorlari aldilar.

**Endpointler:**
- `GET /api/v2/solver_competition/latest` — son rekabet
- `GET /api/v2/solver_competition/by_tx_hash/{tx_hash}` — tx'e gore
- `GET /api/v2/solver_competition/{auction_id}` — ID'ye gore (public, filtered)
- `GET /restricted/api/v2/solver_competition/{auction_id}` — ID'ye gore (internal, unfiltered)

**Kritik: auction_id type handling**
```rust
// u64 ile parse edilir (negatif = 400 BAD_REQUEST)
// u64::MAX .. i64::MAX arasi gecerli ama sistemde yok → 404
if auction_id > AuctionId::MAX.cast_unsigned() {
    return LoadSolverCompetitionError::NotFound.into_response();
}
```

**hide_competition_before_block:**
- Public endpointler mevcut blok numarasini filtre olarak verir
- Restricted endpoint: `None` filter (deadline gecmeden oncekiler de gosterilir)

**DB secimi:**
- Solver competition sorgusu `database_write` havuzunu kullanir (satir 71-77)
- Sebep: replication gecikmesi kabul edilemez (circuit breaker kararlarini etkiler)

**DEX Uygulama Notu — Nova Shield icin:**
- Rekabet verisinin public/restricted ayrimasi seffaflik + is mantigi gereksinimleri dengesini sagiyor
- Replication lag kritik sistemler icin: yazma havuzu kullan, gecikmeyi kabul etme

---

### [api/get_total_surplus.rs] (32 satir)

**Amac:** `GET /api/v1/users/{user}/total_surplus` — kullanicinin tum zamanlarindaki toplam surplus (Mart 2023'ten beri).

**Response:** `{ "totalSurplus": "<U256 string>" }`
- U256::from(f64) donusumu yapilmaktadir — buyuk surplus degerleri icin hassasiyet kaybi riski

**DEX Uygulama Notu:** Surplus takibi = kullanici icin "fiyat gelistirmesi" gostergesi; DEX differentiator.

---

### [api/debug_order.rs] (29 satir) ve [api/debug_simulation.rs] (75 satir)

**Amaclar:**
- `/restricted/api/v1/debug/order/{uid}`: full DebugReport — events, auctions, trades, fee_policies hepsini tek sorguda
- `/restricted/api/v1/debug/simulation/{uid}?block_number=`: mevcut order'i simule et
- `/restricted/api/v1/debug/simulation` (POST): custom order simulation

**OrderSimulationError:**
- `NotEnabled` — 405 METHOD_NOT_ALLOWED
- `MalformedInput` — 400 BadRequest
- `Other` — 500

**SimulationQuery:** `{ block_number: Option<u64> }` — gecmis blokta simule imkani

**DEX Uygulama Notu — Nova Shield icin:**
- Internal debug endpointleri restricted prefix ile korumak temiz bir pattern
- Order simulation (Tenderly entegrasyonu) debugging icin paha bicilmez aracsaldik

---

### [api/get_token_metadata.rs] (24 satir), [api/ready.rs] (19 satir), [api/version.rs] (3 satir)

**get_token_metadata:** `GET /api/v1/token/{token}/metadata`
- Response: `{ firstTradeBlock: u32?, nativePrice: U256? }`
- `firstTradeBlock`: token'in ilk islemi gerceklesen blok
- `nativePrice`: en son auction'daki native fiyat

**ready.rs:** `GET /api/v1/ready`
- DB yazma havuzunda `last_used_auction_id()` sorgusu yapar
- Basariliysa 200 (liveness probe)
- Kubernetes readiness probe icin tasarlanmis

**version.rs:** `GET /api/v1/version`
- Git commit hash dondurur (build-time sabitlenen)

---

## BOLUM 2: database/ — Tam Derinlik Analiz

---

### [database/mod.rs] (144 satir)

**Struct: Config**
```rust
pub struct Config {
    pub max_pool_size: u32,          // default: DB_MAX_CONNECTIONS_DEFAULT
    pub statement_timeout: Duration, // default: 30s (sadece try_new_with_timeout'ta kullanilir)
}
```

**Struct: Postgres**
```rust
pub struct Postgres {
    pub pool: PgPool,  // sqlx async connection pool, Arc icinde
    pub config: Config,
}
```

**fn try_new():** `connect_lazy()` — ilk sorguya kadar baglanti kurmaz
**fn try_new_with_timeout():** Her yeni baglantida `SET statement_timeout = {ms}` uygular
- Bu yazma havuzu icin de gecerli (not: sadece explicit timeout versiyonunu kullanan havuzda)

**insert_order_app_data():** private helper — app_data mevcutsa sifir depolama:
```rust
if existing_data != new_data → InsertionError::AppDataMismatch
```

**Metrics:**
- `orderbook_database_queries` histogram: `labels("type")`, her sorgu tipi icin ayri label
- `start_timer()` / `stop_and_record()` pattern her pub fn'de

**Test:** statement_timeout testi (pg_sleep(5) ile) — `#[ignore]` gerektirir gercek DB

**DEX Uygulama Notu — Nova Shield icin:**
- Cift havuz (read/write) replikasyon gecikme yonetimi icin iyi; kritik sorgularda write kullan
- statement_timeout per-connection olarak ayarlanmasi production'da zorunlu (uzun sorgular pool tikamaz)
- connect_lazy: startup hizini arttirir; saglik kontrolunu geciktirebilir (ready endpoint bunu telafi eder)

---

### [database/orders.rs] (~900 satir) — EN BUYUK DOSYA

**Trait: OrderStoring**
```rust
#[async_trait]
pub trait OrderStoring: Send + Sync {
    async fn insert_order(&self, order: &Order) -> Result<(), InsertionError>;
    async fn cancel_orders(&self, order_uids: Vec<OrderUid>, now: DateTime<Utc>) -> Result<()>;
    async fn cancel_order(&self, order_uid: &OrderUid, now: DateTime<Utc>) -> Result<()>;
    async fn replace_order(&self, old: &OrderUid, new: &Order) -> Result<(), InsertionError>;
    async fn orders_for_tx(&self, tx_hash: &B256) -> Result<Vec<Order>>;
    async fn user_orders(&self, owner: &Address, offset: u64, limit: Option<u64>) -> Result<Vec<Order>>;
    async fn latest_order_event(&self, order_uid: &OrderUid) -> Result<Option<OrderEvent>>;
    async fn single_order(&self, uid: &OrderUid) -> Result<Option<Order>>;
    async fn many_orders(&self, uids: &[OrderUid]) -> Result<Vec<(OrderUid, Result<Order>)>>;
}
```

**InsertionError:**
- `DuplicatedRecord` — unique constraint
- `DbError(sqlx::Error)` — genel DB hatasi
- `AppDataMismatch(Vec<u8>)` — hash ayni, icerik farkli
- `MetadataSerializationFailed(serde_json::Error)`

**insert_order() private fn — Transaction semantigi:**
1. `insert_order_event(OrderEventLabel::Created)` — audit log
2. interactions build (pre + post, indexed)
3. `database::orders::insert_order()` — duplicate kontrolu
4. `database::orders::insert_interactions()` — tek batch
5. quote mevcutsa: `database::orders::insert_quote()` — gas_amount/gas_price/sell_token_price as f64

**impl OrderStoring for Postgres — insert_order():**
- `pool.begin()` ile transaction baslatir
- `insert_order()` + `insert_order_app_data()` ayni transaction icinde
- `commit()` ile bitirir
- Hata: transaction rollback

**replace_order() Transaction semantigi:**
```rust
connection.transaction(move |ex| async {
    database::orders::cancel_order(ex, old_uid, creation_date).await?;
    insert_order(&new_order, ex).await?;
    insert_order_app_data(&new_order, ex).await?;
}.boxed()).await
```
- Atomik: eski iptal + yeni ekleme tek transaction'da
- Bu race condition'i onler

**cancel_orders() — batch atomik:**
- `pool.begin()` + loop icinde `cancel_order()` + `commit()`
- Tum iptal aynı transaction: ya hepsi ya hicbiri

**single_order() — JIT order fallback:**
```rust
match orders::single_full_order_with_quote(...).await? {
    Some(order) => convert(order),
    None => database::jit_orders::get_by_id(...).await?, // JIT order kontrol
}
```

**many_orders() — paralel JIT + normal:**
```rust
try_join!(
    database::orders::many_full_orders_with_quotes(...),
    database::jit_orders::get_many_by_uid(...)
)
```
- Deduplication: HashSet ile
- Ayri baglanti her iki sorgu icin (paralel execution)

**orders_for_tx():**
```rust
tokio::try_join!(user_order_for_tx(hash), jit_orders_for_tx(hash))
// sonra extend
```

**LimitOrderCounting impl:**
- Sahibi basi acik limit order sayimini hesaplar
- `is_order_outside_market_price()` ile market di$i fiyattaki order'lari filtreler
- Bu quota mekanizmasi `TooManyLimitOrders` hatasini tetikler

**calculate_status() fn — Pure Rust, veritabani yok:**
```rust
fn calculate_status(order: &FullOrder) -> OrderStatus {
    // Once executed miktarlari kontrol et
    if sell: (sum_sell - sum_fee) == sell_amount → Fulfilled
    if buy: sum_buy == buy_amount → Fulfilled
    // Sonra invalidation
    if invalidated → Cancelled
    // Sonra expiry
    if valid_to < now → Expired
    // Sonra presignature
    if presignature_pending → PresignaturePending
    // Yoksa
    Open
}
```

**Oncelik sirasi (kritik):** Fulfilled > Cancelled > Expired > PresignaturePending > Open
- "Fulfilled ama iptal edilmis" → Fulfilled kazanir (dolu order iptal edilemez)
- "Expired ama presign bekliyor" → Expired kazanir

**is_sell_order_filled():**
```
total_amount = executed_amount - executed_fee
total_amount == sell_amount → filled
```
- Sifir executed_amount → dolmamis (koruyucu kontrol)

**is_buy_order_filled():**
```
!executed_amount.is_zero() && buy_amount == executed_amount
```

**full_order_into_model_order() — Buyuk donusum fn:**
- DB'den model'e tam donusum
- `big_decimal_to_u256`, `big_decimal_to_big_uint` donusumleri — context hatalari ile
- EthflowData, OnchainOrderData, pre/post interactions, ethflow specific alanlar
- Signature reconstruct: `Signature::from_bytes(scheme, &bytes)`

**Test Coverage:**
- order_status(): 12 senaryo — sell/buy, filled/partial/empty, cancelled, expired, presignature
- Quote metadata olup olmadigi senaryolari

**DEX Uygulama Notu — Nova Shield icin:**
- OrderStoring trait: mock icin `#[cfg_attr(test, mockall::automock)]` — test isolasyon deseni
- Atomik replace (cancel + insert) pattern: critical section'da transaction zorunlu
- JIT order fallback: iki farkli order tipi (user + JIT) seffaf sekilde birlestiriliyor
- Status hesaplama pure fn: veritabanindan ayri, test edilmesi kolay — benimsenmeli
- `try_join!` ile paralel sorgu: latency azaltma; Nova Shield'da aggressif kullanilmali

---

### [database/trades.rs] (196 satir)

**Trait: TradeRetrieving**
```rust
async fn trades(&self, filter: &TradeFilter) -> Result<Vec<Trade>>;
```

**Trait: TradeRetrievingPaginated**
```rust
async fn trades_paginated(&self, filter: &PaginatedTradeFilter) -> Result<Vec<Trade>>;
```

**TradeFilter:** `{ owner: Option<Address>, order_uid: Option<OrderUid> }`
**PaginatedTradeFilter:** + `offset: u64, limit: u64`

**SQL sorgusu (database crate'ine delege edilmis):**
- `database::trades::trades(&mut ex, owner, order_uid, offset, limit)`
- v1: offset=0, limit=i64::MAX (pagination yok)
- v2: explicit offset+limit

**executed_protocol_fees fetch'i:**
- Once trades listelenir
- `auction_id` olan her trade icin `(auction_id, order_uid)` pair'lari toplanir
- `u16::MAX` kontrol: sqlx bind limit; limit asilirsa hata doner
- `executed_protocol_fees()` ile batch fee sorgusu
- Fee sonuclari trade'lere eslestirilir

**trade_from() — Row donusum:**
- `block_number: i64 → u32`
- `log_index: i64 → u32`
- `buy_amount, sell_amount, sell_amount_before_fees: BigDecimal → BigUint`
- `tx_hash: Option<ByteArray> → Option<B256>`

**DEX Uygulama Notu — Nova Shield icin:**
- Protocol fee join: her trade icin ayrica fee sorgusu yapilmasi yerine batch fetch — n+1 problemi onlendi
- u16::MAX query param siniri sqlx'e ozgu; Nova Shield'da kullanilan DB driver sinirlari kontrol edilmeli

---

### [database/fee_policies.rs] (146 satir)

**Amac:** Protocol fee hesaplama ve uygulama — executed fee miktarlarini on-chain veriden reconstruct etme.

**fn fee_policy_from() — UC fee tipi:**

**1. Surplus fee:**
```rust
FeePolicy::Surplus {
    factor: f64,          // surplus'un yuzde kaci fee
    max_volume_factor: f64, // maksimum volume bazli sinir
}
```

**2. Volume fee:**
```rust
FeePolicy::Volume {
    factor: f64,  // trade volume'unun yuzde kaci
}
```

**3. PriceImprovement fee:**
```rust
FeePolicy::PriceImprovement {
    factor: f64,
    max_volume_factor: f64,
    quote: Quote {
        sell_amount: U256,
        buy_amount: U256,
        fee: U256, // gas_amount * gas_price / sell_token_price
    }
}
```
- Fee hesabi: `fee = (gas_amount * gas_price) / sell_token_price`
- `BigRational` kullanilir: kesin aritmetik, overflow kontrolu
- `checked_mul` ve `checked_div`: overflow hatalarini context ile propagate eder

**DEX Uygulama Notu — Nova Shield icin:**
- UC fee modeli cok esnek: bazi order'lar surplus bazli, bazilari volume bazli olabilir
- PriceImprovement fee matematik: BigRational ile kesin; float kullanirsaniz muhtemelen hatali
- Fee politikalarinin DB'de order bazinda saklanmasi (auction_id + order_uid key): audit edilebilir

---

### [database/auctions.rs] (35 satir)

**most_recent_auction():** `database::auction::load_most_recent()` + JSON deserializasyon
**last_used_auction_id():** Liveness probe icin

---

### [database/quotes.rs] (57 satir)

**Trait: QuoteStoring impl for Postgres:**
- `save(QuoteData) → QuoteId`
- `get(QuoteId) → Option<QuoteData>`
- `find(QuoteSearchParameters, expiration) → Option<(QuoteId, QuoteData)>`

**find():** QuoteSearchParameters + expiration ile expired quote'lari otomatik filtreler

**DEX Uygulama Notu:** Quote cache — orderbook quote'lari DB'ye yazar, order submission'da quote dogrular.

---

### [database/app_data.rs] (76 satir)

**get_full_app_data():** hash → UTF-8 string (UTF-8 olmayan veriyi reddeder context ile)
**insert_full_app_data():** idempotent upsert — duplicate = Err, mismatch = Err, yeni = Ok

**InsertError:**
- `Duplicate` — ayni hash+data zaten var
- `Mismatch(String)` — ayni hash, farkli data
- `Other(anyhow::Error)` — DB hatasi

---

### [database/auction_prices.rs] (23 satir)

**fetch_latest_prices():** Son auction'daki token fiyatlarini `HashMap<Address, BigDecimal>` olarak dondurur.

---

### [database/total_surplus.rs] (119 satir) — KRITIK SQL

**TOTAL_SURPLUS_QUERY — Tam SQL:**

```sql
WITH trade_components AS (
    -- Kullanicinin dogrudan gonderdigi order'lar
    SELECT
        o.uid,
        CASE o.kind
            WHEN 'sell' THEN t.buy_amount
            WHEN 'buy' THEN t.sell_amount - t.fee_amount
        END AS trade_amount,
        CASE o.kind
            WHEN 'sell' THEN (t.sell_amount - t.fee_amount) * o.buy_amount / o.sell_amount
            WHEN 'buy' THEN t.buy_amount * o.sell_amount / o.buy_amount
        END AS limit_amount,
        o.kind,
        ap.price AS surplus_token_native_price
    FROM orders o
    JOIN trades t ON t.order_uid = o.uid
    JOIN order_execution oe ON oe.order_uid = t.order_uid
    LEFT JOIN auction_prices ap
        ON ap.auction_id = oe.auction_id
        AND ap.token = CASE o.kind WHEN 'sell' THEN o.buy_token ELSE o.sell_token END
    WHERE o.owner = $1

    UNION ALL

    -- Kullanicinin ethflow araciligiyla gonderdigi order'lar (onbehalf-of)
    SELECT ... FROM onchain_placed_orders opo
    JOIN orders o ON o.uid = opo.uid AND o.owner != $1
    WHERE opo.sender = $1

    UNION ALL

    -- JIT order'lar (sahibinin, normal orders tablosunda olmayanlar)
    SELECT ... FROM jit_orders j
    WHERE j.owner = $1 AND NOT EXISTS (SELECT 1 FROM orders o WHERE o.uid = j.uid)
)
SELECT
    COALESCE(SUM(surplus_in_wei ORDER BY uid), 0) AS total_surplus_in_wei
FROM (
    SELECT
        uid,
        CASE kind
            WHEN 'sell' THEN (trade_amount - limit_amount) * surplus_token_native_price
            WHEN 'buy' THEN (limit_amount - trade_amount) * surplus_token_native_price
        END / POWER(10, 18) AS surplus_in_wei
    FROM trade_components
) ts;
```

**Matematik:**
- Sell order surplus: `(actual_buy - minimum_buy) * native_price / 1e18`
  - `actual_buy = t.buy_amount`
  - `minimum_buy = (t.sell_amount - t.fee_amount) * o.buy_amount / o.sell_amount`
- Buy order surplus: `(maximum_sell - actual_sell) * native_price / 1e18`
  - `actual_sell = t.sell_amount - t.fee_amount`
  - `maximum_sell = t.buy_amount * o.sell_amount / o.buy_amount`

**Index kullanimi:**
- `orders.owner`: index beklenir (performans kritik)
- `trades.order_uid`: join icin index beklenir
- `onchain_placed_orders.sender`: index beklenir
- `auction_prices`: (auction_id, token) composite index

**Race condition analizi:**
- Read-only sorgu, transaction yok — dirty read imkani (Postgres: READ COMMITTED default)
- Onemi az: istatistiksel sorgu, atomiklik kritik degil

**Guvenlik:**
- `$1` parametreli sorgu: SQL injection yok
- Cok buyuk sonuclar icin potansiyel performans sorunu — `o.owner` index'i kritik

**DEX Uygulama Notu — Nova Shield icin:**
- Surplus formulasyon (limit fiyat vs gercek fiyat farki): DEX'e ozgu degeri olcen metrik
- UNION ALL ile uc tip order (normal, ethflow, JIT) tek sorguda — on uclu sorgu
- `ORDER BY uid` ile deterministik SUM sirasi: floating point toplamini sabitler
- `POWER(10, 18)` bolme: wei → native token; precision kaybi f64 donusum nedeniyle olabilir

---

### [database/solver_competition.rs] (144 satir)

**Trait: SolverCompetitionStoring impl:**
- `load_competition(Identifier, after_block) → SolverCompetitionAPI`
- `load_latest_competition(after_block) → SolverCompetitionAPI`
- `load_latest_competitions(count) → Vec<SolverCompetitionAPI>`

**Identifier enum:** `Id(i64)` veya `Transaction(B256)`

**deserialize_solver_competition():** JSON value → `SolverCompetitionDB` → `SolverCompetitionAPI`

**after_block filtresi:** `load_by_id(id, after_block)` — None = filtre yok (restricted endpoint)

---

### [database/solver_competition_v2.rs] (180 satir)

**Response: ApiResponse**
```rust
struct ApiResponse {
    auction_id: i64,
    auction_start_block: i64,
    auction_deadline_block: i64,
    transaction_hashes: Vec<B256>,
    reference_scores: BTreeMap<Address, U256>,
    auction: Auction { prices: BTreeMap<Address, U256>, orders: Vec<OrderUid> },
    solutions: Vec<Solution>,
}

struct Solution {
    solver_address: Address,
    score: U256,
    ranking: i64,
    clearing_prices: BTreeMap<Address, U256>,
    orders: Vec<Order>,
    is_winner: bool,
    filtered_out: bool,
    tx_hash: Option<B256>,
    reference_score: Option<U256>,
}
```

**try_into_dto() — uc tabloyu birlestiren donusum:**
1. `native_prices`: price_tokens + price_values → BTreeMap<Address, U256>
2. `settlements`: solution_uid → B256 map
3. `reference_scores`: solver → U256 map
4. `trades`: solution_uid bazinda groupby (HashMap<i64, Vec<Order>>)
5. `solutions`: her cozum icin clearing_prices + trades + settlement + reference_score join
6. Son adim: solutions'i ranking'e gore sirala (worst to best, descending)

**DEX Uygulama Notu — Nova Shield icin:**
- Solver rekabet verisinin API'de acilmasi: seffaf ve rekabetci ecosystem icin kritik
- Clearing prices (her token icin tek fiyat tum batch'te): Coincidence of Wants matematigi
- `filtered_out` flag: cezali solver'larin cozumleri filtrelendi mi? — circuit breaker baglantisi

---

### [database/debug_report.rs] (239 satir)

**fetch_debug_report() — Cok tablodan veri toplayan ana sorgu:**
1. `single_order()` — order varligini kontrol et
2. `order_events::get_all()` — tum lifecycle eventi
3. `solver_competition_v2::find_solutions_for_order()` — bu order'i iceren solver cozumleri
4. `order_execution::read_by_order_uid()` — execution kayitlari
5. `database::trades::trades()` — en fazla 100 trade kaydi
6. `auction::fetch_auction_ids_by_order_uid()` — hangi auction'larda vardi
7. `auction::fetch_multiple()` — bu auction'larin detaylari
8. `settlement_executions::read_by_auction_ids()` — settlement girisim kayitlari
9. `fee_policies::fetch_by_order_uid()` — uygulanmis fee politikalari

**build_auctions():** 6 koleksiyonu auction_id etrafinda HashMap ile birlestirip Auction listesi olusturur.

**DEX Uygulama Notu — Nova Shield icin:**
- Debug endpoint tum lifecycle'i tek API cagrisinda sunar: kullanici destek operasyonlari icin cok degerli
- 9 ayri sorgu + 1 join build: N+1 yoktur, her sey once fetch edilir sonra in-memory join yapilir
- Audit log (order_events) ayrimi: events tablo ayri, sorgulanabilir — Nova Shield'a model olarak alinabilir

---

## BOLUM 3: dto/ — Tam Derinlik Analiz

---

### [dto/mod.rs] (93 satir)

**OrderSimulationRequest:**
```rust
pub struct OrderSimulationRequest {
    sell_token: Address,
    buy_token: Address,
    sell_amount: NonZeroU256,  // NonZeroU256: sifir satis miktar engellenir
    buy_amount: U256,
    kind: OrderKind,
    owner: Address,
    receiver: Option<Address>,
    sell_token_balance: SellTokenSource,  // default ERC20
    buy_token_balance: BuyTokenDestination, // default ERC20
    app_data: String,  // full app data JSON
    block_number: Option<u64>,
    signature: Signature,  // flatten
    fee_amount: U256,  // sifir bekleniyor
    valid_to: u32,
    partially_fillable: bool,
}
```

**OrderSimulationResult:**
```rust
pub struct OrderSimulationResult {
    tenderly_request: tenderly::dto::Request, // tam Tenderly API istegi
    tenderly_url: Option<String>,  // share linki, skip_serializing_if = None
    error: Option<String>,  // simulasyon hatasi
}
```

**TokenMetadata:**
```rust
pub struct TokenMetadata {
    first_trade_block: Option<u32>,
    native_price: Option<U256>,  // serde_as HexOrDecimalU256
}
```

---

### [dto/order.rs] (122 satir)

**Order (auction DTO — Orderbook'un ic order modeli degil):**
```rust
pub struct Order {
    uid: OrderUid,
    sell_token, buy_token: Address,
    sell_amount, buy_amount: U256,  // HexOrDecimalU256
    protocol_fees: Vec<FeePolicy>,  // birden fazla fee politikasi
    created: u32,
    valid_to: u32,
    kind: OrderKind,
    receiver: Option<Address>,
    owner: Address,
    partially_fillable: bool,
    executed: U256,  // su ana kadar execute edilen
    pre_interactions, post_interactions: Vec<InteractionData>,
    sell_token_balance: SellTokenSource,
    buy_token_balance: BuyTokenDestination,
    class: OrderClass,  // flatten
    app_data: AppDataHash,
    signature: Signature,  // flatten
    quote: Quote,
}
```

**Quote (ic DTO — api::Order'a ozgu):**
```rust
pub struct Quote {
    sell_amount, buy_amount, fee: U256, // HexOrDecimalU256
    solver: Address,
}
```

**FeePolicy enum:**
```rust
enum FeePolicy {
    Surplus { factor: f64, max_volume_factor: f64 },
    Volume { factor: f64 },
    PriceImprovement { factor: f64, max_volume_factor: f64, quote: Quote },
}
```
- Serde: `{ "Surplus": { "factor": 0.5, "maxVolumeFactor": 0.1 } }` gibi

**Status enum (order durum makinesi):**
```rust
#[serde(tag = "type", rename_all = "camelCase", content = "value")]
enum Status {
    Open,
    Scheduled,
    Active,
    Solved(Vec<SolutionInclusion>),
    Executing(Vec<SolutionInclusion>),
    Traded(Vec<SolutionInclusion>),
    Cancelled,
}
```
- `SolutionInclusion`: `{ solver: String, executed_amounts?: { sell, buy } }`
- JSON ornegi: `{ "type": "solved", "value": [{ "solver": "...", "executedAmounts": {...} }] }`

**ExecutedAmounts:**
```rust
pub struct ExecutedAmounts {
    sell: U256, // HexOrDecimalU256
    buy: U256,  // HexOrDecimalU256
}
```

---

### [dto/auction.rs] (35 satir)

**Auction (public API DTO):**
```rust
pub struct Auction {
    block: u64,
    orders: Vec<Order>,  // dto::Order tipinde
    prices: BTreeMap<Address, U256>,  // HexOrDecimalU256 values
    surplus_capturing_jit_order_owners: Vec<Address>, // default boş
}
```
- Not: Yorum "only used for liveness checking" (satir 11) — bu yani siralidir

**AuctionWithId:**
```rust
pub struct AuctionWithId {
    id: AuctionId,  // i64
    #[serde(flatten)]
    auction: Auction,
}
```
- JSON: `{ "id": 42, "block": ..., "orders": [...], "prices": {...} }`

---

## BOLUM 4: Guvenlik Tam Ozeti

### Kimlik Dogrulama / Yetkilendirme

**CoW Protocol'de auth mekanizmasi YOKTUR** dogrudan API seviyesinde:
- POST /orders: ECDSA imza dogrulamasi (order sahibinin imzasi gerekli — order icerigini imzalar)
- DELETE /orders/{uid}: Iptal imzasi zorunlu
- DELETE /orders (batch): SignedOrderCancellations — toplu imza
- Diger GET endpointler: herkese acik, auth yok
- Restricted (/restricted/api/*): infra/WAF seviyesinde kontrol, application katmaninda yok

**Imza Tipleri:**
- EIP-712: `keccak256(abi.encode(ORDER_TYPEHASH, order_hash))` — en yaygin
- EIP-1271: sozlesme imzasi, on-chain cagri ile dogrulama
- Pre-sign: sozlesme ile imzalama (on-chain tx)
- EthSign: personal_sign

### Rate Limiting

- Uygulama katmaninda: **YOKTUR**
- WAF/infra katmaninda oldugu varsayilir
- Potansiyel risk: POST /orders ve POST /quote icin brute force, DDoS

### Input Validation

- JSON body: serde ile strongly typed parse; hatalı tipte 400 (Axum extractor otomatik)
- Hex parametreler: 0x prefix opsiyonel, hatalı hex → 400
- Body boyutu: 16 KB (app_data icin ayrıca ayarlanabilir)
- Batch limitleri: 1024 UID (cancel_orders, get_orders_by_uid)
- Pagination limitleri: [1, 1000], default 10

### SQL Injection

- Yok — sqlx prepared statements, parametre binding: `sqlx::query_scalar(QUERY).bind(user)`

### Overflow/Underflow

- U256 arithmetic: `big_decimal_to_u256()` ile checked donusum — None donebilir
- Sell order fill: BigDecimal aritmetigi, sifir division riski `sell_amount != 0` korumasiyla
- Surplus SQL: f64 precision kaybi riski (buyuk U256 degerlerinde)
- Fee hesabi: `BigRational` ile kesin aritmetik; `checked_mul/div` overflow korumasi

### Veri Sizdirma

- `WrongOwner` hata mesaji: recovered signer + order hash → kismı bilgi sizdirma
- `TransferSimulationFailed`: revert data hexlenmis olarak gonderilir
- Debug endpointleri restricted prefix'le korunuyor

---

## BOLUM 5: DEX Uygulama Notlari — Nova Shield Direkt Eylem Plani

### API Tasarimi

| Karar | CoW Approach | Nova Shield Tavsiyesi |
|-------|-------------|----------------------|
| Protokol | REST (axum) | REST + gRPC (ic servisler) |
| Formatı | JSON/camelCase | ayni, + protobuf ic servisler |
| Versiyonlama | /api/v1, /api/v2 | /api/v1 ile basla |
| Auth | imza bazli | imza + optional API key |
| Rate Limit | WAF seviyesi | Uygulama katmaninda ekle (tower-governor) |
| CORS | Any | whitelist |
| Body Limit | 16 KB (ayarlanabilir) | ayni |

### Order Submission Protokolu

1. **Quote → Order** iki adim: kullanici once quote alir, sonra quote_id ile order gonderir
2. **Signature zorunlu**: EIP-712 default, EIP-1271 sozlesme destegi
3. **Validation katmanlari**: partial (ucuz) → full (pahalı, on-chain)
4. **Atomik replace**: eski order cancel + yeni order insert tek transaction
5. **Batch endpoint**: `POST /orders/by_uids` — partial success semantigi

### Streaming Quote

- SSE ile gercek zamanli fiyat guncelleme: `POST /quote/stream`
- Nova Shield icin WebSocket da dusunulebilir (cift yonlu, daha esnek)

### Database Tasarimi

1. **Cift havuz** (read/write ayrimi): kritik sorgular icin write havuzu
2. **Statement timeout**: per-connection, varsayilan 30s
3. **Audit log** (order_events): order yasam dongusu izleme icin ayri tablo
4. **JIT order fallback**: iki farkli order tipi seffaf union ile sunulur
5. **Batch sorgu** (try_join!, paralel): latency minimize

### Surplus Matematigi

Nova Shield DEX icin surplus hesaplama:
```
Sell order:
  actual_buy - (sell_executed * limit_buy_price / limit_sell_amount)
  → native para biriminde

Buy order:
  (buy_amount * limit_sell_price / limit_buy_amount) - sell_executed
  → native para biriminde
```
- BigRational ile kesin aritmetik; floating point kullanma

### Fee Modeli

UC katmanli:
1. `Surplus fee`: kullanicinin kazandigi surplus'tan pay
2. `Volume fee`: trade hacminin sabit yuzdesi
3. `PriceImprovement fee`: quote'a gore fiyat iyilestirmesinin yuzdesi

Nova Shield icin: her uc mod desteklenmeli, politika per-order kaydedilmeli.

### Order Status State Machine

```
Open → Scheduled → Active → Solved → Executing → Traded
                                              ↘ (basarisiz)
                            ↘ Cancelled (iptal)
```
- Minimum 7 durum — Nova Shield icin bu modeli aynen al

### Guvenlk Kontrol Listesi (Nova Shield icin)

- [ ] Rate limiting: uygulama katmaninda (tower-governor veya benzeri)
- [ ] CORS: whitelist, `Any` kullanma
- [ ] Body limit: JSON boyutu + alan sayisi (JSON depth limiti ekle)
- [ ] Batch limiti: COUNT siniri byte limitinden bagimsiz
- [ ] SQL: prepared statements (sqlx kullan, string interpolasyon yok)
- [ ] Auth: imza + deny-list + quota (TooManyLimitOrders benzeri)
- [ ] Audit log: her order eventi kayit altinda
- [ ] Debug endpoint'ler: restricted prefix veya auth korumasinda
- [ ] Statement timeout: per-connection ayarlanmis
- [ ] Replication: kritik sorgular icin write havuzu

---

## BOLUM 6: Mimari Ozet

```
HTTP Request
    ↓
axum Router (api.rs)
    ↓
Middleware: TraceLayer → MetricsLayer → SummarizeRequest → CORS → BodyLimit
    ↓
Handler (api/*.rs): Axum extractor ile parse
    ↓
Arc<AppState>: orderbook, quotes, database_write, database_read, app_data
    ↓
Orderbook (orderbook.rs): is mantigi, validation cagrilari
    ↓
Database (database/*.rs): sqlx PgPool, trait impl'leri
    ↓
PostgreSQL: orders, trades, auction, fee_policies, solver_competition, app_data
```

**Dogrulanan Guvenlik Bulgulari:**
- Rate limiting: YOK (agent-07 ile uyumlu — dogrulandi)
- CORS: `Any` (cok gevşek — dokumante edildi)
- Auth: imza bazli (API key yok, JWT yok)
- SQL injection: YOKTUR (sqlx parametreli sorgular)
- Body limit: 16 KB (uygulaniyor)
- Batch limitleri: uygulanıyor (1024)

**Toplam Dosya Analizi:** 24 Rust dosyasi, ~2800 satir kaynak kodu
