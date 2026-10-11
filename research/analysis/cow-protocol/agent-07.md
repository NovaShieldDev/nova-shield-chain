# CoW Protocol Analiz — Agent 07
## Kapsam: orderbook/src/ — Order Book API
## DEX Özel Protokolü: 3x Hassasiyet

**Analiz Tarihi:** 2026-07-22
**Toplam Dosya Sayısı:** 50 Rust kaynak dosyası
**Toplam Satır:** ~8.665 satır
**Dizin:** `crates/orderbook/src/`

---

## Genel Mimari

`orderbook` crate'i CoW Protocol'ün kullanıcıya bakan HTTP API katmanıdır. Order ekleme, sorgulama, iptal etme ve fiyat teklifi (quote) hesaplama işlemlerini yönetir. PostgreSQL'i kalıcı depolama olarak kullanır. Solver'lar açık order'ları bu API üzerinden sorgular. Framework: Axum (async Rust HTTP). İki ayrı DB bağlantısı tutar: `database_write` (birincil) ve `database_read` (read replica).

---

## DOSYA ANALİZLERİ

---

### [main.rs] (12 satır)

**Amaç:** Binary giriş noktası — `run::start()` fonksiyonunu çağırır.

**Kritik Fonksiyonlar:**
- `main()`: `run::start(std::env::args())` — tüm mantık `run.rs`'e delege edilir.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da da binary entry point ile business logic'i ayır. `main.rs` yalnızca argüman parse edip `run::start()` çağırmalı. Bu, test edilebilirliği artırır.

---

### [lib.rs] (13 satır)

**Amaç:** Crate'in public modüllerini tanımlar.

**Struct/Enum/Trait Listesi:**
- `pub mod api` — HTTP API katmanı
- `pub mod app_data` — App data registry
- `pub mod arguments` — CLI argümanları
- `pub mod database` — PostgreSQL abstraction
- `pub mod dto` — Data transfer objects
- `pub mod ipfs` — IPFS HTTP istemcisi
- `pub mod ipfs_app_data` — IPFS app data önbellekli erişim
- `pub mod orderbook` — Ana orderbook iş mantığı
- `pub mod quoter` — Quote hesaplama
- `pub mod run` — Servis başlatma
- `pub mod solver_competition` — Solver yarışması trait/hata tipleri

**DEX Uygulama Notu — Nova Shield için:**
- Bu modüler yapı referans alınabilir. Nova Shield'da da orderbook mantığını katmanlara ayır: API katmanı → İş mantığı katmanı → Depolama katmanı.

---

### [arguments.rs] (18 satır)

**Amaç:** Servis için tek CLI argümanını tanımlar.

**Struct/Enum/Trait Listesi:**
- `Arguments` (satır 4): `clap::Parser` derive'lı struct.
  - `config: PathBuf` — TOML konfigürasyon dosyasının yolu (`--config` veya `CONFIG` env var)

**Güvenlik Analizi:**
- Config dosyası path'i validate edilmez. Var olmayan path → runtime panic `run::start()`'ta.
- Tüm konfigürasyon `Configuration::from_path()` içinde validate edilir.

**DEX Uygulama Notu — Nova Shield için:**
- TOML-tabanlı konfigürasyon yaklaşımı sağlıklı. Nova Shield da bütün operasyonel parametreleri (rate limits, token whitelist, fee oranları) TOML/YAML config dosyasından okumalı.

---

### [run.rs] (617 satır)

**Amaç:** Servisin tüm bileşenlerini initialize edip HTTP server'ı başlatır.

**Kritik Fonksiyonlar:**

- `start(args)` (satır 51): CLI argümanlarını parse eder, config dosyasını yükler, tracing/metrics init eder, `run(config)`'i çağırır.

- `run(config)` (satır 97): Tüm bağımlılıkları wire eder:
  1. Ethereum RPC bağlantısı
  2. Chain ID doğrulaması
  3. `GPv2Settlement` kontrat bağlantısı
  4. `VaultRelayer` adresi
  5. PostgreSQL (write + read replica)
  6. Balance fetcher (ERC-20 bakiye simülasyonu için)
  7. Gas price estimator
  8. Token info fetcher (önbellekli)
  9. Price estimator factory (optimal + fast)
  10. Native price estimator (fallback zincirli)
  11. Order validator (`OrderValidator::new(...)`)
  12. IPFS app data bağlantısı (opsiyonel)
  13. `Orderbook::new(...)` — ana iş mantığı nesnesi
  14. `QuoteHandler::new(...)` — quote hesaplayıcı
  15. HTTP API server (Axum)
  16. Metrics server (ayrı port)

- `serve_api(...)` (satır 547): Axum router'ı oluşturur, TCP listener başlatır, graceful shutdown destekler.

- `verify_deployed_contract_constants(contract, chain_id)` (satır 595): **Kritik güvenlik fonksiyonu.** Domain separator ve OrderData type hash'inin deploy edilmiş kontratın bytecode'unda bulunduğunu doğrular. EIP-712 imza tutarsızlıklarına karşı erken alarm.

- `check_database_connection(orderbook)` (satır 540): Başlangıçta DB bağlantısını test eder.

- `shutdown_signal()` (satır 513, Unix): SIGTERM ve SIGINT sinyallerini dinler. Kubernetes graceful shutdown için 10 saniyelik timeout.

**Yapısal Özellikler:**
- `optimal_quoter` ve `fast_quoter` ayrı fiyat tahmincileri üzerinde çalışır.
  - `optimal_quoter`: Tüm solver'lara danışır, verifiable quote üretir, streaming destekler.
  - `fast_quoter`: Hızlı ama güvenilmez tahminler (expiry=0).
- Read replica (`postgres_read`) etkin olduğunda read-only sorgular write DB'ye yük bindirmez.

**Güvenlik Analizi:**
- `verify_deployed_contract_constants` yoksa binary ile kontrat farklı domain separator kullanabilir → tüm imzalar geçersiz.
- OrderValidator, banned kullanıcıları kontrol eder (Chainalysis Oracle + Hermod servis + adres listesi).
- `eip1271_skip_creation_validation` flag'i ile EIP-1271 imza doğrulaması atlanabilir → dikkatli kullanılmalı.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da da servis başlangıcında deployment doğrulaması yapılmalı. L1 zincirinde kontrat adresi ile domain separator eşleşmeli.
- Iki DB: yazma (primary) ve okuma (replica). Bu pattern, yük dengeleme açısından önemli.
- OrderValidator içindeki ban sistemi Nova Shield'a adapt edilebilir: on-chain oracle + off-chain liste.

---

### [orderbook.rs] (936 satır) — ANA İŞ MANTIĞI

**Amaç:** Order ekle, iptal et, sorgula, durum takip et işlemlerini yönetir.

**Struct/Enum/Trait Listesi:**

- `Metrics` (satır 49): Prometheus metric storage.
  - `orders: IntCounterVec` — etiketler: `kind` (market/limit) ve `operation` (created/cancelled)

- `OrderOperation` (satır 57): `Created | Cancelled` — strum Display impl

- `OrderClass` (satır 64): `Market | Limit` — market fiyatına göre sınıflandırma

- `AddOrderError` (satır 124): Order eklenirken oluşabilecek hatalar:
  - `OrderNotFound(OrderCancellationError)` — replace için hedef order bulunamadı
  - `DuplicatedOrder` — aynı UID'li order zaten var
  - `OrderValidation(ValidationError)` — imza/bakiye/token hataları
  - `Database(anyhow::Error)` — DB hatası
  - `InvalidAppData(anyhow::Error)` — geçersiz app data formatı
  - `InvalidReplacement(OrderReplacementError)` — order değiştirme kuralı ihlali
  - `AppDataMismatch { contract_app_data, provided, existing }` — hash/veri uyuşmazlığı
  - `MetadataSerializationFailed(serde_json::Error)` — metadata JSON hatası

- `OrderCancellationError` (satır 185):
  - `InvalidSignature` — imza hatalı
  - `WrongOwner` — imzacı ≠ order sahibi
  - `OrderNotFound` — order DB'de yok
  - `AlreadyCancelled` — zaten iptal edilmiş
  - `OrderFullyExecuted` — tamamen gerçekleşmiş
  - `OrderExpired` — süresi dolmuş
  - `OnChainOrder` — PreSign order'lar off-chain iptal edilemez
  - `Other(anyhow::Error)` — beklenmedik hata

- `OrderReplacementError` (satır 205):
  - `InvalidSignature` — non-ECDSA imzayla replace yasak
  - `WrongOwner` — eski ve yeni order sahibi farklı
  - `OldOrderActivelyBidOn` — eski order aktif açık artırmada → double-spend riski
  - `Other(anyhow::Error)`

- `QuoteMetadata` (satır 217): Order'la ilişkili quote bilgisi.
  - `id: Option<QuoteId>` — quote ID
  - `solver: Address` — quote'u veren solver

- `Orderbook` (satır 232): Ana struct.
  - `domain_separator: DomainSeparator` — EIP-712 domain
  - `settlement_contract: Address` — GPv2Settlement kontrat adresi
  - `database: Postgres` — yazma DB (primary)
  - `database_replica: Postgres` — okuma DB (replica)
  - `order_validator: Arc<dyn OrderValidating>` — validasyon mantığı
  - `app_data: Arc<crate::app_data::Registry>` — app data registry
  - `active_order_competition_threshold: u32` — kaç son açık artırma kontrol edilecek
  - `order_simulator: Option<SettlementSimulator>` — Tenderly simülatörü (opsiyonel)

**Kritik Fonksiyonlar:**

- `add_order(payload: OrderCreation)` (satır 276):

  Order ekleme akışı (6 adım):
  1. **App data çözümleme:** Eğer payload sadece hash içeriyorsa (`OrderCreationAppData::Hash`), registry'den full app data alınır.
  2. **Replace order tespiti:** Full app data varsa, `replaced_order` alanı parse edilir. Eğer varsa, eski order `get_replaced_order()` ile alınır.
  3. **Validasyon:** `order_validator.validate_and_construct_order()` — imza, bakiye, token, expiry vb.
  4. **Replace veya insert:** Eski order varsa `replace_order()`, yoksa `database.insert_order()`.
  5. **Metrik güncelle:** `Metrics::on_order_operation()`.
  6. **Sonuç döndür:** `(OrderUid, Option<QuoteMetadata>)`.

- `get_replaced_order(new_order, app_data_override)` (satır 406):

  App data'yı parse edip `replacedOrder` field'ını bulur. `find_order_for_cancellation()` ile eski order'ı doğrular. Geçersiz app data → `AddOrderError::InvalidAppData`.

- `replace_order(validated_new_order, old_order)` (satır 442):

  **Üç kural zorlanır:**
  1. Yeni order ECDSA imzalı olmalı (PreSign yasak — başkasının order'ını iptal saldırısını önler).
  2. Eski ve yeni order sahibi aynı olmalı.
  3. Eski order aktif açık artırmada olmamalı (double-spend önlemi).

  DB'de atomik işlem: eski order'ı iptal et + yeni order'ı ekle.

- `order_is_actively_bid_on(order_uid)` (satır 480):

  `active_order_competition_threshold` kadar son solver yarışmasını yükler. Bu yarışmalardaki tüm çözümlerin order listelerini tarar. `order_uid` eşleşiyorsa `true` döner.

  **Kritik güvenlik mekanizması:** Order replace edildiğinde eski order aktif açık artırmadaysa, settlement contract çelişkili iki order'ı görebilir (biri cancel, biri yeni). Bu kontrol double-spend'i engeller.

- `cancel_order(cancellation: OrderCancellation)` (satır 374):

  1. `find_order_for_cancellation()` ile order'ı bul ve durumunu doğrula.
  2. `cancellation.validate(&domain_separator)` — EIP-712 imzasını doğrula, signer'ı recover et.
  3. Signer == order owner kontrolü.
  4. `database.cancel_order()` — DB'de iptal et.

- `cancel_orders(cancellation: SignedOrderCancellations)` (satır 343):

  Toplu iptal — her order için ayrı `find_order_for_cancellation()`. Tüm order'ların aynı signer'a ait olduğunu doğrular. DB'de toplu cancel.

- `find_order_for_cancellation(order_uid)` (satır 319):

  Order durumunu kontrol eder:
  - `PresignaturePending` → `OnChainOrder` hatası
  - `Open` + non-ECDSA imza → `OnChainOrder` hatası
  - `Fulfilled` → `OrderFullyExecuted`
  - `Cancelled` → `AlreadyCancelled`
  - `Expired` → `OrderExpired`
  - `Open` + ECDSA → geçerli

- `get_order_status(uid)` (satır 534):

  **Zengin durum makinesi.** Trade tablosu önce kontrol edilir (executed = kesin). Sonra `order_events` tablosundaki son event:
  - `Ready` → `Status::Active`
  - `Created` → `Status::Scheduled`
  - `Considered` → `Status::Solved(competition_data)`
  - `Executing` → `Status::Executing(competition_data)`
  - `Traded` → `Status::Traded(competition_data)`
  - `Cancelled` → `Status::Cancelled`
  - `Filtered` | `Invalid` → `Status::Open`

- `simulate_order(uid, block_number)` (satır 618):

  Mevcut bir order'ı Tenderly üzerinde simüle eder. `SettlementSimulator` üzerinden settlement tx simüle edilir. Sonuç: Tenderly URL + hata mesajı (varsa).

- `simulate_custom_order(request)` (satır 680):

  Keyfi bir order verisiyle simülasyon — DB'de kayıt olmadan.

- `LivenessChecking` impl (satır 772):

  `is_alive()` → `get_auction().is_ok()`. Metrics server bu trait'i kullanarak `/health` endpoint'i yönetir.

**Matematiksel Formüller:**

Order sınıflandırması (market vs limit) — `Metrics::on_order_operation()`:

```
is_market = !is_order_outside_market_price(
    order_amounts = { sell, buy, fee },
    quote_amounts = { sell, buy, fee = gas_amount * gas_price / sell_token_price }
)
```

**Güvenlik Analizi:**

1. **Replay Attack:** EIP-712 domain separator her order imzasına dahil. Aynı imza farklı chain'de geçersiz.
2. **Double-Spend:** `order_is_actively_bid_on()` threshold tabanlı kontrol. Threshold düşük olursa gecikmiş solver önerileri atlanabilir.
3. **PreSign Bypass:** `find_order_for_cancellation()` PreSign order'ları off-chain iptal'e izin vermez — yalnızca on-chain cancel çalışır.
4. **App Data Mismatch:** Hash ≠ full data kombinasyonu tespit edilirse `AppDataMismatch` hatası.
5. **Race Condition:** `add_order` ile `cancel_order` eş zamanlı çalışırsa DB transaction'ları PostgreSQL'in MVCC mekanizması korur.

**Test Analizi (satır 778-936):**

`postgres_replace_order_verifies_signer_and_app_data` (satır 792) — `#[ignore]` (entegrasyon testi):
- Farklı owner ile replace → `WrongOwner`
- Var olmayan replaced_order → `OrderNotFound`
- PreSign imza ile replace → `InvalidSignature`
- Correct flow → başarı, `new_order_uid` döner

**DEX Uygulama Notu — Nova Shield için:**

1. **Order Lifecycle State Machine:** Nova Shield'da order lifecycle şu şekilde olmalı:
   `Created → Scheduled → Active → Solved/Executing → Traded | Cancelled | Expired`
   Her geçiş bir blockchain event veya off-chain event olabilir.

2. **Replace Mekanizması:** Order replace özelliği DEX UX açısından kritik. Kullanıcı limit fiyatını değiştirmek istediğinde yeni order oluşturulur, eski iptal edilir. Atomik DB işlemi şart.

3. **Active Bid-On Kontrolü:** Replace sırasında double-spend riski Nova Shield için de geçerli. Solver'ların hangi order'ları aktif değerlendirdiğini takip eden bir mekanizma gerekli.

4. **Off-Chain vs On-Chain Cancel:** L1'de order settlement yaptığın için on-chain cancel zorunlu olabilir. Off-chain cancel'ı yalnızca ECDSA imzalı order'lara izin ver.

---

### [api.rs] (1018 satır) — HTTP API ROUTER

**Amaç:** Tüm HTTP endpoint'leri tanımlar, middleware'leri konfigüre eder, AppState paylaşımını yönetir.

**Struct/Enum/Trait Listesi:**

- `AppState` (satır 66): Tüm handler'lar arasında paylaşılan uygulama durumu.
  - `database_write: Postgres` — DDL ve mutation sorgular için
  - `database_read: Postgres` — read-only sorgular için (replica)
  - `orderbook: Arc<Orderbook>` — ana iş mantığı
  - `quotes: QuoteHandler` — quote hesaplayıcı
  - `app_data: Arc<app_data::Registry>` — app data kayıt/sorgulama
  - `native_price_estimator: Arc<dyn NativePriceEstimating>` — token → ETH fiyatı
  - `quote_timeout: Duration` — quote hesaplama zaman aşımı
  - `current_block_stream: CurrentBlockWatcher` — anlık blok numarası
  - `hide_competition_before_deadline: bool` — yarışma gizleme özelliği

- `ApiMetrics` (satır 356): Prometheus metrikleri.
  - `requests_complete: IntCounterVec` — etiketler: `method`, `status_code`
  - `requests_rejected: IntCounterVec` — etiket: `status_code`
  - `requests_duration_seconds: HistogramVec` — bucket'lar: 0.1s, 0.5s, 1s, 2s, 4s, 6s, 8s, 10s

- `Error` (satır 404): JSON hata yanıt yapısı.
  - `error_type: Cow<'static, str>` — makine tarafından okunabilir hata kodu
  - `description: Cow<'static, str>` — insan tarafından okunabilir açıklama
  - `data: Option<serde_json::Value>` — ek hata verisi (opsiyonel)

- `PriceEstimationErrorWrapper` (satır 452): Orphan rule nedeniyle wrapper tip.

**Kritik Fonksiyonlar / API Endpoint'leri:**

`handle_all_routes(...)` (satır 156) — tam endpoint listesi:

**V1 Endpointleri (24 adet):**

| Method | Path | Handler | Açıklama |
|--------|------|---------|----------|
| GET | `/api/v1/account/{owner}/orders` | `get_user_orders` | Kullanıcının order listesi |
| PUT | `/api/v1/app_data` | `put_app_data_without_hash` | App data kayıt (hash'siz) |
| GET | `/api/v1/app_data/{hash}` | `get_app_data` | App data sorgulama |
| PUT | `/api/v1/app_data/{hash}` | `put_app_data_with_hash` | App data kayıt (hash'li) |
| GET | `/api/v1/auction` | `get_auction` | Mevcut açık artırma |
| POST | `/api/v1/orders` | `post_order` | Order ekle |
| POST | `/api/v1/orders/by_uids` | `get_orders_by_uid` | UID'ye göre çoklu order sorgulama |
| DELETE | `/api/v1/orders` | `cancel_orders` | Toplu iptal |
| GET | `/api/v1/orders/{uid}` | `get_order_by_uid` | Tekil order sorgulama |
| DELETE | `/api/v1/orders/{uid}` | `cancel_order` | Tekil iptal |
| GET | `/api/v1/orders/{uid}/status` | `get_order_status` | Order durum takibi |
| POST | `/api/v1/quote` | `post_quote` | Fiyat teklifi |
| POST | `/api/v1/quote/stream` | `post_quote_stream` | SSE akışı üzerinden fiyat teklifi |
| GET | `/api/v1/token/{token}/metadata` | `get_token_metadata` | Token meta verisi |
| GET | `/api/v1/token/{token}/native_price` | `get_native_price` | Token'ın ETH cinsinden fiyatı |
| GET | `/api/v1/trades` | `get_trades` | İşlem geçmişi (v1, sınırsız) |
| GET | `/api/v1/transactions/{hash}/orders` | `get_orders_by_tx` | TX'e ait order'lar |
| GET | `/api/v1/users/{user}/total_surplus` | `get_total_surplus` | Kullanıcı toplam artı değeri |
| GET | `/api/v1/version` | `version` | Servis versiyonu |
| GET | `/api/v1/ready` | `ready` | Readiness probe |

**Kısıtlı Endpointler (WAF korumalı, `restricted/` prefix):**

| Method | Path | Handler | Açıklama |
|--------|------|---------|----------|
| GET | `/restricted/api/v1/debug/order/{uid}` | `debug_order` | Debug raporu |
| GET | `/restricted/api/v1/debug/simulation/{uid}` | `debug_simulation` | Simülasyon sonucu |
| POST | `/restricted/api/v1/debug/simulation` | `debug_simulation_post` | Özel order simülasyonu |

**V2 Endpointleri (5 adet):**

| Method | Path | Handler | Açıklama |
|--------|------|---------|----------|
| GET | `/api/v2/solver_competition/latest` | `get_solver_competition_latest` | Son açık artırma |
| GET | `/api/v2/solver_competition/by_tx_hash/{tx_hash}` | `get_solver_competition_by_hash` | TX hash'e göre |
| GET | `/api/v2/solver_competition/{auction_id}` | `get_solver_competition_by_id` | ID'ye göre |
| GET | `/api/v2/trades` | `get_trades_v2` | Sayfalı trade geçmişi |
| GET | `/restricted/api/v2/solver_competition/{auction_id}` | `get_solver_competition_by_id_unfiltered` | Deadline filtresi kapalı |

**Middleware Stack (dıştan içe):**

1. `TraceLayer` (OpenTelemetry distributed tracing, span oluşturma)
2. `with_matched_path_metric` (Prometheus metrikleri — request count + duration)
3. `summarize_request` (method, URI, user-agent, status, elapsed loglar)
4. `CorsLayer` — `Access-Control-Allow-Origin: *` + izin verilen metodlar + headerlar (`x-auth-token`, `x-appid`)
5. `DefaultBodyLimit::max(16KB)` — JSON body sınırı

**Sabitler:**
- `MAX_JSON_BODY_PAYLOAD: u64 = 1024 * 16` (16 KB) — genel JSON body limiti
- App data body limiti: `app_data.size_limit()` (8 KB default, `Validator::new(8192)`)

**AppState Metodu:**
- `hide_competition_before_block()` (satır 82): `hide_competition_before_deadline` flag'i aktifse mevcut blok numarasını döner. Bu değer DB sorgularında yarışma verilerini deadline geçmeden gizlemek için kullanılır.

**PriceEstimationErrorWrapper impl:**
- `UnsupportedToken` → 400 Bad Request
- `UnsupportedOrderType` → 400 Bad Request
- `NoLiquidity | RateLimited | EstimatorInternal` → 404 Not Found (`"NoLiquidity"`)
- `TradingOutsideAllowedWindow` → 400 Bad Request
- `TokenTemporarilySuspended` → 400 Bad Request
- `InsufficientLiquidity` → 400 Bad Request
- `CustomSolverError` → 400 Bad Request
- `ProtocolInternal` → 500 Internal Server Error

**Test Analizi (satır 529-1017):**

Kapsamlı extractor testleri:
- `path_order_uid`: `0x` prefix ile/siz, tek sayıda hex karakteri → BAD_REQUEST
- `path_address`: adres formatı doğrulama
- `path_b256`: TX hash formatı doğrulama
- `path_app_data_hash`: app data hash formatı
- `path_u64`: sayısal path parametresi
- `query_hex_types`: orderUid, owner query param formatları
- `query_numeric_types`: offset, limit parametreleri

**Matematiksel Formüller:**
Histogram bucket'ları: `0.1, 0.5, 1, 2, 4, 6, 8, 10` saniye — request latency dağılımı ölçümü.

**Güvenlik Analizi:**

1. **CORS Açıklığı:** `Access-Control-Allow-Origin: *` — herhangi bir origin'den istek kabul edilir. Bu bilinçli bir tercih (public DEX API), ancak hassas kullanıcı verisi içeriyorsa riskli.
2. **Rate Limiting:** Doğrudan uygulama katmanında rate limiting YOKTUR. WAF veya infra katmanında yapılması bekleniyor.
3. **Body Size Limit:** 16 KB genel limit, 8 KB app data limiti — spam koruması için yeterli.
4. **Kısıtlı Endpoint Güvenliği:** `/restricted/` prefix'li endpoint'ler WAF/infra kurallarıyla korunur. Kod seviyesinde authentication yoktur.
5. **User-Agent Loglama:** Her istek user-agent loglanır — anomali tespitinde kullanılabilir.

**DEX Uygulama Notu — Nova Shield için:**

1. **Endpoint Tasarımı:** Nova Shield DEX API'si de benzer endpoint hiyerarşisi kurabilir. v1/v2 ayrımı geriye dönük uyumluluk için iyi.
2. **Rate Limiting:** Uygulama katmanında explicit rate limiting ekle — CoW'da eksik. `tower-governor` veya benzer middleware kullanılabilir. Özellikle `POST /orders` ve `POST /quote` için.
3. **CORS:** Nova Shield L1 için, frontend'in kim olduğuna göre CORS kısıtlama yapılabilir.
4. **Body Limits:** 16KB genel limit makul. App data için 8KB sınır — Nova Shield'da da benzer limit uygula.
5. **Metrics:** Her endpoint için `requests_complete`, `requests_rejected`, `requests_duration_seconds` üçlüsü standart hale getirilmeli.
6. **`restricted/` prefix:** İç tool'lar ve debug endpointleri için bu pattern iyi. Nova Shield'da da insan-erişilebilir debug endpointlerini `restricted/` altında tut.

---

### [quoter.rs] (816 satır) — QUOTE HESAPLAMA

**Amaç:** Order quote'larını hesaplar, volume fee uygular, streaming quote desteği sağlar.

**Struct/Enum/Trait Listesi:**

- `AdjustedQuoteData` (satır 43): Volume fee sonrası düzeltilmiş miktarlar.
  - `sell_amount: U256`
  - `buy_amount: U256`
  - `protocol_fee_bps: Option<String>` — fee'nin basis point değeri (örn. "2" = 2 bps = 0.02%)

- `QuoteHandler` (satır 63): Yüksek seviye quote yöneticisi.
  - `order_validator: Arc<dyn OrderValidating>`
  - `optimal_quoter: Arc<dyn OrderQuoting>` — verifiable, doğru tahminler
  - `fast_quoter: Arc<dyn OrderQuoting>` — hızlı ama kesin olmayan tahminler
  - `app_data: Arc<app_data::Registry>`
  - `volume_fee: Option<VolumeFeeConfig>` — volume fee konfigürasyonu
  - `volume_fee_policy: Arc<VolumeFeePolicy>` — token bucket bazlı fee override'ları
  - `token_info_fetcher: Arc<dyn TokenInfoFetching>` — token sembol bilgisi (loglama için)
  - `streaming_quoter: Option<Arc<dyn StreamingQuoting>>` — SSE streaming için

- `OrderQuoteError` (satır 473): Quote hatası enum:
  - `AppData(AppDataValidationError)`
  - `Order(PartialValidationError)`
  - `CalculateQuote(CalculateQuoteError)`

**Kritik Fonksiyonlar:**

- `QuoteHandler::new(...)` (satır 77):
  `VolumeFeePolicy::new(volume_fee_bucket_overrides, factor, enable_sell_equals_buy_volume_fee, native_token)` ile policy oluşturur. `fast_quoter` başlangıçta `optimal_quoter` ile aynı (override edilebilir).

- `calculate_quote(request)` (satır 118):

  Quote hesaplama akışı:
  1. `build_quote_params(request)` — validasyon + parametre oluşturma
  2. `PriceQuality::Optimal | Verified` → `optimal_quoter.calculate_quote()` + `store_quote()`
  3. `PriceQuality::Fast` → `fast_quoter.calculate_quote()`, expiry = epoch 0 (anlık geçersiz)
  4. `get_vol_fee_adjusted_quote_data()` — volume fee hesapla ve miktarları düzelt
  5. `build_order_quote_response()` — yanıt oluştur
  6. `QuoteComputedEvent` publish et — event bus

- `calculate_quote_stream(request)` (satır 164):

  SSE streaming için:
  1. Streaming quoter'ın var olup olmadığını kontrol et
  2. `streaming.calculate_quote_stream(params)` — async stream oluştur
  3. Her solver'dan gelen quote event'ini volume fee ile düzelt
  4. Başarılı event'leri SSE stream'e yaz
  5. Hiç başarılı quote yoksa `NoLiquidity` hatası stream'e yaz

- `build_quote_params(request)` (satır 242):

  1. Token sembolleri paralel çek (`join!`)
  2. App data çözümle ve validate et
  3. Order'ı `partial_validate()` ile ön doğrulama
  4. `emit_quote_requested_event()` — event bus
  5. `QuoteParameters` oluştur

- `get_vol_fee_adjusted_quote_data(quote, side, volume_fee, volume_fee_policy, buy_token, sell_token)` (satır 397):

  **Volume fee hesaplama — matematiksel detay:**

  Ön koşul: `volume_fee_config` var ve `effective_from_timestamp <= Utc::now()`.

  Fee faktörünü belirle: `volume_fee_policy.get_applicable_volume_fee_factor(buy_token, sell_token, None)`

  ```
  scaled_factor = U256::from(factor.to_high_precision())
  scale = U512::from(FeeFactor::HIGH_PRECISION_SCALE)
  ```

  **SELL order'ı için:**
  ```
  protocol_fee = floor(buy_amount * scaled_factor / scale)
  adjusted_buy = buy_amount - protocol_fee  (saturating_sub)
  adjusted_sell = sell_amount  (değişmez)
  ```
  Fee, buy token'dan alınır. Kullanıcı beklediğinden daha az buy token alır.

  **BUY order'ı için:**
  ```
  total_sell_volume = sell_amount + fee_amount  (network fee dahil)
  protocol_fee = floor(total_sell_volume * scaled_factor / scale)
  adjusted_sell = sell_amount + protocol_fee  (saturating_add)
  adjusted_buy = buy_amount  (değişmez)
  ```
  Fee, sell token'dan alınır. Kullanıcı beklediğinden daha fazla sell token öder.

  **Örnek:** 0.02% fee (2 bps) için:
  - `scaled_factor = factor.to_high_precision()` — yüksek hassasiyetli temsil
  - `protocol_fee_bps = "2"` döner
  - Sub-BPS desteklenir: 0.3 bps → `"0.3"`

- `build_order_quote_response(request, quote, adjusted, id, valid_to)` (satır 344):

  `OrderQuoteResponse` oluşturur:
  - `sell_amount`, `buy_amount` → `AdjustedQuoteData`'dan (volume fee uygulanmış)
  - `fee_amount` → raw quote'dan (gas fee)
  - `gas_amount`, `gas_price`, `sell_token_price` → f64 → BigDecimal dönüşümü
  - `app_data`: Full ise `Both` (full + hash) formatına çevrilir
  - `protocol_fee_bps` → adjusted'dan

**Matematiksel Formüller (Özet):**

```
# SELL order volume fee
buy_fee = buy_amount * fee_factor
adjusted_buy = buy_amount - buy_fee

# BUY order volume fee
total_volume = sell_amount + gas_fee
sell_fee = total_volume * fee_factor
adjusted_sell = sell_amount + sell_fee

# Fee faktörü (yüksek hassasiyet)
fee_bps = factor * 10000
```

**Test Analizi (satır 497-815) — 7 adet unit test:**

1. `test_volume_fee_sell_order`: 100 ETH sell, 100 ETH buy, 0.02% fee → buy 0.02 ETH azalır.
2. `test_volume_fee_buy_order`: 100 ETH buy, 100 ETH sell, geçmiş effective date → sell 0.02 ETH artar.
3. `test_volume_fee_buy_order_with_network_fee`: 5 ETH network fee eklenir → total_volume = 105 ETH, fee = 0.021 ETH.
4. `test_volume_fee_different_prices`: 100 sell → 200 buy, 0.1% fee → buy 0.2 ETH azalır.
5. `test_volume_fee_basis_points_conversion`: 0.01% = "1", 0.1% = "10", 1% = "100", 5% = "500", 10% = "1000"
6. `test_ignore_volume_fees_before_effective_date`: Gelecek timestamp → fee uygulanmaz.
7. `test_volume_fee_sub_basis_point_precision`: 0.3 bps, 1M ETH → 30 ETH fee, bps = "0.3".

**Güvenlik Analizi:**

1. **Overflow Koruması:** `widening_mul` U512'ye genişletir. `U256::uint_try_from()` ile geri dönüşümde overflow hatası.
2. **Sıfıra Bölme:** `scale` sıfır olamaz (sabit). `checked_div` kullanıldı → güvenli.
3. **Underflow:** `saturating_sub` — buy amount negatife düşmez.
4. **Effective Timestamp:** Fee aktivasyon tarihi geçmişte değilse fee uygulanmaz → planlanmış fee artışları desteklenir.
5. **Sub-BPS Hassasiyet:** `HIGH_PRECISION_SCALE` kullanımı küçük fee oranlarının yuvarlama hatalarını önler.

**DEX Uygulama Notu — Nova Shield için:**

1. **İki Hız Quoter:** Fast (UI önizleme) ve Optimal (gerçek işlem) iki ayrı quoter Nova Shield'da da faydalı.
2. **Volume Fee Hesaplama:** Bu formüller Nova Shield DEX fee mekanizması için doğrudan referans. SELL ve BUY order'larının fee uygulaması farklıdır — dikkatli implement et.
3. **Fee Faktörü Aktivasyon:** `effective_from_timestamp` — fee oranlarını kademeli artırabilirsin.
4. **Sub-BPS Hassasiyet:** U512 widening multiply + high precision scale ile sub-BPS fee desteklenebilir.
5. **SSE Streaming:** `/quote/stream` endpointi quote hesaplaması sırasında kullanıcıya anlık güncellemeler gönderir. Nova Shield'da L1'e gönderim öncesi quote preview için kullanılabilir.

---

### [app_data.rs] (129 satır) — APP DATA REGISTRY

**Amaç:** Order'larla ilişkili JSON metadata'yı (app data) kayıt altına alır ve sorgular. Hash → Full JSON eşlemesini yönetir.

**Struct/Enum/Trait Listesi:**

- `Registry` (satır 11): App data kayıt defteri.
  - `validator: app_data::Validator` — 8KB limit ile JSON formatı doğrulama
  - `database: Postgres` — kalıcı depolama
  - `ipfs: Option<IpfsAppData>` — fallback: IPFS gateway

- `RegistrationStatus` (satır 103): `New | AlreadyExisted`

- `Register` (satır 111): Kayıt sonucu.
  - `status: RegistrationStatus`
  - `hash: AppDataHash`

- `RegisterError` (satır 116):
  - `Invalid(anyhow::Error)` — JSON format hatası
  - `HashMismatch { expected, computed }` — belirtilen hash ≠ hesaplanan
  - `DataMismatch { existing }` — aynı hash, farklı data
  - `Other(anyhow::Error)`

**Kritik Fonksiyonlar:**

- `register(hash: Option<AppDataHash>, document: &[u8])` (satır 42):

  1. `validator.validate(document)` — JSON doğrula, hash hesapla
  2. Hash belirtilmişse: `hash == validated.hash` kontrolü
  3. `database.insert_full_app_data(hash, document)`
  4. `InsertError::Duplicate` → `AlreadyExisted`
  5. `InsertError::Mismatch` → `DataMismatch`

- `find(contract_app_data: &AppDataHash)` (satır 80):

  Full app data bulma zinciri:
  1. Hash sıfır ise → `EMPTY` döner (rezerve değer)
  2. DB'de ara → varsa döner
  3. IPFS'te ara → varsa döner
  4. Hiçbirinde yoksa → `None`

- `size_limit()` (satır 32): Validator'ın boyut limitini döner (default 8KB).

**Güvenlik Analizi:**

1. **Hash Doğrulama:** `validate()` sonucu `validated.hash` ile belirtilen `hash` karşılaştırılır. Hash collision saldırısına karşı güvenli (keccak256).
2. **Boyut Limiti:** 8KB sınır — büyük JSON payload DDoS saldırısını önler.
3. **İçerik Doğrulama:** `Validator` yalnızca boyut kontrolü değil, format da doğrular.
4. **IPFS Fallback:** IPFS'te güvenilmez veri olabilir. `from_utf8` kontrolü yapılıyor. Hash eşleşmesi validate sonrası değil, `find()` DB çıktısında yapılmaz — IPFS'ten gelen veri hash ile doğrulanmaz.
5. **DataMismatch:** Aynı hash'e farklı data yazılmaya çalışıldığında hata verir — collision direnci.

**DEX Uygulama Notu — Nova Shield için:**

1. Nova Shield order'larına metadata eklemek için benzer bir registry kullan. JSON schema validasyonu zorunlu.
2. Hash → Data eşlemesini immutable tut (bir kez yazıldı, değiştirilemez).
3. IPFS gibi decentralized storage optional olabilir — önce DB, sonra external kaynak.

---

### [ipfs.rs] (100 satır) — IPFS HTTP İSTEMCİSİ

**Amaç:** IPFS gateway üzerinden CID'ye göre içerik çeker.

**Struct/Enum/Trait Listesi:**

- `Ipfs` (satır 8):
  - `client: reqwest::Client` — 5 saniyelik timeout ile
  - `base: Url` — gateway base URL
  - `query: Option<String>` — opsiyonel query parametresi (Pinata auth token için)

**Kritik Fonksiyonlar:**

- `fetch(cid: &str)` (satır 34):

  Gateway davranışı tutarsızlıkları dokümante edilmiş:
  - `ipfs.io`: 2 dakika sonra 504 Gateway Timeout
  - Cloudflare: 20 saniye sonra 524
  - Pinata (private): 2 dakika sonra 404

  Timeout → `Ok(None)` (hata değil, sadece "bulunamadı").
  200 OK → `Ok(Some(bytes))`.
  Diğer status kodlar → `Ok(None)` (bulunamadı olarak işlenir).

- `prepare_url(cid)` (satır 54): `{base}/ipfs/{cid}` + optional query

**Güvenlik Analizi:**

1. **Timeout:** 5 saniye — gateway gecikmesine karşı koruma.
2. **HTTP vs HTTPS:** Sadece URL'e bağlı. Production'da HTTPS zorunlu olmalı.
3. **CID Doğrulama:** `fetch()` CID'yi validate etmez. Geçersiz CID → gateway'e gönderilir.
4. **Content Integrity:** Dönen içerik hash ile doğrulanmaz. IPFS'te hash-adreslenmiş içerik doğası gereği doğrudur, ama proxy corruption'a karşı savunmasız.

---

### [ipfs_app_data.rs] (172 satır) — IPFS APP DATA ÖNBELLEKLE ERİŞİM

**Amaç:** IPFS'ten app data çekimini önbellekler ve iki farklı CID formatını destekler.

**Struct/Enum/Trait Listesi:**

- `IpfsAppData` (satır 9):
  - `ipfs: Ipfs` — HTTP istemcisi
  - `cache: Mutex<TimedSizedCache<AppDataHash, Option<String>>>` — LRU önbellek
  - `metrics: &'static Metrics` — Prometheus metrikleri

- `Metrics` (satır 16):
  - `app_data: IntCounterVec` — etiketler: `outcome` (error/found/missing) × `source` (cache/node)
  - `fetches: Histogram` — IPFS fetch süresi

**Kritik Fonksiyonlar:**

- `IpfsAppData::new(ipfs)` (satır 27): Önbellek konfigürasyonu:
  - `with_size_and_lifespan_and_refresh(1000, 600, false)` — 1000 giriş, 600 saniye TTL, refresh=false

- `fetch_raw(contract_app_data)` (satır 55):

  İki CID formatı paralel denenir:
  - `old_app_data_cid()`: eski format (dag-pb + sha2-256 prefix)
  - `new_app_data_cid()`: yeni format (multibase base32lower kodlaması)

  `futures::future::select_ok()` — ilk başarılıyı seç, diğerini iptal et.

- `fetch(contract_app_data)` (satır 87):

  1. Önbellekte ara
  2. Önbellekte yoksa `fetch_raw()` çağır
  3. Sonucu önbelleğe kaydet
  4. Metrik güncelle (outcome × source)

- `new_app_data_cid(hash)` (satır 123): `create_ipfs_cid(hash.0)` → multibase base32lower
- `old_app_data_cid(hash)` (satır 128): Manuel CID oluştur: `[cid_version=1, dag-pb=0x70, sha2-256=0x12, len=32] + hash` → multibase base32lower

**Güvenlik Analizi:**

1. **Cache Poisoning:** `None` değerleri önbelleklenir (600 saniye). Geçici IPFS arızaları uzun süre önbelleğe alınabilir.
2. **Mutex Contention:** `Mutex<TimedSizedCache>` — yüksek eşzamanlılıkta bottleneck olabilir. Async mutex değil sync.
3. **UTF-8 Doğrulama:** `String::from_utf8(result)` — geçersiz UTF-8 → `Ok(None)`.

**DEX Uygulama Notu — Nova Shield için:**

Nova Shield'da order metadata için IPFS optional ama kullanışlı. CID v1 (blake3 veya sha256 destekli) kullan. 600 saniye TTL + 1000 giriş → iyi başlangıç noktası. `select_ok()` ile eski/yeni format paralel deneme yaklaşımı migration kolaylığı sağlar.

---

### [solver_competition.rs] (61 satır) — SOLVER YARIŞMASI TRAIT'İ

**Amaç:** Solver yarışma verilerine erişim için trait ve hata tipi tanımlar.

**Struct/Enum/Trait Listesi:**

- `Identifier` (satır 11): Yarışmayı tanımlar.
  - `Id(AuctionId)` — açık artırma ID
  - `Transaction(B256)` — settlement TX hash

- `SolverCompetitionStoring` (satır 19): `#[async_trait]`, `mockall::automock` destekli.
  - `load_competition(identifier, after_block)` — ID veya TX hash ile yarışma yükle
  - `load_latest_competition(after_block)` — en son yarışma
  - `load_latest_competitions(count)` — son N yarışma

  `after_block: Option<i64>` — deadline geçmemiş yarışmaları gizleme mekanizması.

- `LoadSolverCompetitionError` (satır 56): `NotFound | Other(anyhow::Error)`

**DEX Uygulama Notu — Nova Shield için:**

Nova Shield'da solver/validator yarışması mekanizması farklı (L1 blockchain tabanlı). Ancak off-chain aşama için benzer bir "competition log" tutmak faydalı. Kazanan validator'ın hangi block'a commit ettiği ve diğerlerinin önerilerini takip etmek için bu trait pattern kullanılabilir.

---

## VERİTABANI KATMANI ANALİZİ

### [database/mod.rs] (143 satır) — POSTGRES BAĞLANTI HAVUZU

**Struct/Enum/Trait Listesi:**

- `Config` (satır 28):
  - `max_pool_size: u32` — default: `DB_MAX_CONNECTIONS_DEFAULT`
  - `statement_timeout: Duration` — default: 30 saniye (sadece read replica'da aktif)

- `Postgres` (satır 48): Thread-safe bağlantı havuzu.
  - `pool: PgPool` — Arc sarmalanmış, `Clone` destekli
  - `config: Config`

- `Metrics` (satır 105): `orderbook_database_queries` histogram — etiket: `type`

**Kritik Fonksiyonlar:**

- `try_new(uri, config)` (satır 57): `connect_lazy` — bağlantıyı ilk kullanımda kur.

- `try_new_with_timeout(uri, config)` (satır 68): Her bağlantıda `SET statement_timeout` komutu çalıştır. Read replica için kullanılır.

- `insert_order_app_data(order, ex)` (satır 87): Order içindeki full app data'yı DB'ye yaz. Eşleşmeme durumunda `InsertionError::AppDataMismatch`.

**Test:**
- `postgres_statement_timeout_cancels_slow_query` — `SELECT pg_sleep(5)` → 100ms timeout → `57014` hata kodu (query_canceled).

**Güvenlik Analizi:**
- Read replica'da `statement_timeout` → uzun süreli okuma sorgularını iptal eder. Potansiyel DDoS mitigation.
- `connect_lazy` → uygulama başlarken DB'ye bağlanmaz. İlk query'de bağlanır. `check_database_connection()` bunu erken tetikler.

---

### [database/orders.rs] (1265 satır) — ORDER DEPOLAMA

**Amaç:** Order CRUD operasyonları, JIT order sorgulama, limit order sayımı, durum hesaplama.

**Struct/Enum/Trait Listesi:**

- `OrderStoring` (satır 58): `#[async_trait]`, `mockall::automock` destekli.
  - `insert_order(&Order)` → `Result<(), InsertionError>`
  - `cancel_orders(Vec<OrderUid>, DateTime<Utc>)` → `Result<()>`
  - `cancel_order(&OrderUid, DateTime<Utc>)` → `Result<()>`
  - `replace_order(&OrderUid, &Order)` → `Result<(), InsertionError>`
  - `orders_for_tx(&B256)` → `Result<Vec<Order>>`
  - `user_orders(&Address, offset, limit)` → `Result<Vec<Order>>`
  - `latest_order_event(&OrderUid)` → `Result<Option<OrderEvent>>`
  - `single_order(&OrderUid)` → `Result<Option<Order>>`
  - `many_orders(&[OrderUid])` → `Result<Vec<(OrderUid, Result<Order>)>>`

- `InsertionError` (satır 82):
  - `DuplicatedRecord`
  - `DbError(sqlx::Error)`
  - `AppDataMismatch(Vec<u8>)` — mevcut full app data döner
  - `MetadataSerializationFailed(serde_json::Error)`

**Kritik Fonksiyonlar (özel — free fonksiyonlar):**

- `cancel_order(ex, order_uid, now)` (satır 97): Tek bağlantıda:
  1. `insert_order_event(Cancelled)` — event kaydı
  2. `database::orders::cancel_order()` — DB güncelleme

- `insert_order(order, ex)` (satır 117): Tek transaction'da:
  1. `insert_order_event(Created)` — event kaydı
  2. Pre/post interaction'larını düzleştir + index ver
  3. `database::orders::insert_order()` — ana kayıt
  4. `database::orders::insert_interactions()` — interaction'lar
  5. Quote varsa `insert_quote()` — quote metadata

  **Field mapping** (model → DB):
  - `sell_amount`, `buy_amount`: U256 → BigDecimal (sqlx uyumu için)
  - `valid_to`: u32 → i64 (PostgreSQL int8)
  - `signature`: `to_bytes()` → Vec<u8>
  - `interactions`: (target, value, data, index, execution_time) tuple'ları

**Trait İmplementasyonları (Postgres impl OrderStoring):**

- `insert_order(order)` (satır 217): Transaction + insert_order_app_data. `is_duplicate_record_error()` PostgreSQL hatası ayrıştırır.

- `cancel_orders(uids, now)` (satır 233): Tek transaction'da tüm UID'ler.

- `replace_order(old_uid, new_order)` (satır 260): `connection.transaction()` closure — atomik: cancel_old + insert_new + insert_app_data.

- `single_order(uid)` (satır 292): Önce `orders` tablosunda ara. Bulamazsa `jit_orders` tablosunda ara. JIT order'lar quote'suz döner.

- `many_orders(uids)` (satır 318): UID'leri deduplicate et, `orders` + `jit_orders` tablolarını paralel sorgula (`try_join!`). Her sonuç `(OrderUid, Result<Order>)` — kısmi başarı desteklenir.

- `orders_for_tx(tx_hash)` (satır 362): `user_order_for_tx` + `jit_orders_for_tx` paralel (`try_join!`).

- `user_orders(owner, offset, limit)` (satır 373): `database::order_history::user_orders()` — stream olarak döner, `try_collect()`.

**Postgres impl (diğer metodlar):**

- `user_order_for_tx(tx_hash)` (satır 414): `database::orders::full_orders_in_tx()` — stream.
- `jit_orders_for_tx(tx_hash)` (satır 431): `database::jit_orders::get_by_tx()` — Vec.
- `token_metadata(token)` (satır 445): `tokio::try_join!` ile paralel:
  - `token_first_trade_block()` — tokenin ilk trade edildiği blok
  - `fetch_latest_token_price()` — en son native fiyat

**LimitOrderCounting impl (satır 492):**

- `count(owner)`: Sahibin aktif limit order sayısını hesapla.
  - `user_orders_with_quote()` — sahibin aktif order + quote çiftlerini al
  - Her çift için `is_order_outside_market_price()` kontrolü → true olanları say
  - Limit order = piyasa fiyatının dışında, quote'a sahip order

**Yardımcı Fonksiyonlar:**

- `calculate_status(order)` (satır 536):

  Durum öncelik sırası:
  1. Fulfilled? (order türüne göre)
  2. Invalidated? → Cancelled
  3. Expired? (`valid_to < now`)
  4. PresignaturePending? → PresignaturePending
  5. Aksi halde: Open

- `is_sell_order_filled(amount, executed_amount, executed_fee)` (satır 656):
  ```
  total_net = executed_amount - executed_fee
  filled = (total_net == amount) && executed_amount > 0
  ```

- `is_buy_order_filled(amount, executed_amount)` (satır 668):
  ```
  filled = (amount == executed_amount) && executed_amount > 0
  ```

- `full_order_with_quote_into_model_order(order, quote)` (satır 566):
  DB'den gelen `FullOrder` → model `Order`. Tüm BigDecimal → U256/BigUint dönüşümleri burada. Özellikle:
  - `executed_sell_amount_before_fees = sum_sell - sum_fee` (BigDecimal aritmetik)
  - `full_app_data`: Vec<u8> → UTF-8 String

**Test Analizi (satır 672-936):**

`order_status()` testi — tüm durum geçişlerini test eder:
- Open (sell, %0 dolu)
- Open (sell, %99.99 dolu)
- Open (PreSign, presignature onaylanmış)
- PresignaturePending
- Fulfilled (sell, %100)
- Open (buy, %0)
- Open (buy, %99.99)
- Fulfilled (buy, %100)
- Cancelled (no fills)
- Cancelled (partial fill, sell)
- Cancelled (partial fill, buy)
- Expired (no fills)
- Expired (partial fill, sell)
- Expired (partial fill, buy)
- Expired + presignature pending

**Güvenlik Analizi:**

1. **Atomik İşlemler:** insert, cancel, replace işlemleri SQL transaction içinde. Race condition korunması PostgreSQL MVCC ile.
2. **Deduplication:** `many_orders()` UID listesini `HashSet`'e dönüştürür → duplicate UID saldırısına karşı.
3. **JIT Order:** Solver oluşturduğu JIT order'lar normal order tablosunda değil ayrı tabloda. Karışıklık önlenir.
4. **Bakiye Limitlemesi:** `count()` sadece "market dışı" order'ları sayar → limit order spam'ine karşı kullanılır.

**DEX Uygulama Notu — Nova Shield için:**

1. **Durum Makinesi:** `calculate_status()` fonksiyonu Nova Shield L1 için adapte edilebilir. L1'de filled = on-chain event.
2. **JIT Order:** Solver veya validator oluşturduğu "just-in-time" order'lar için ayrı tablo. Normal kullanıcı order'larıyla karıştırma.
3. **Limit Order Sayım:** Spam önleme için per-user limit order sayısı tutulmalı. Market dışı order = limit order tanımı Nova Shield'da da geçerli.
4. **Paralel Sorgular:** `try_join!` ile paralel DB sorguları → latency azaltma. Nova Shield'da da uygulanabilir.

---

### [database/trades.rs] (195 satır) — TRADE SORGULAMA

**Struct/Enum/Trait Listesi:**

- `TradeFilter` (satır 23): `owner: Option<Address>` | `order_uid: Option<OrderUid>` — tam filtre
- `PaginatedTradeFilter` (satır 29): TradeFilter + `offset: u64` + `limit: u64`
- `TradeRetrieving` (satır 12): V1 API için — `trades(&TradeFilter)` → tüm sonuçlar
- `TradeRetrievingPaginated` (satır 17): V2 API için — `trades_paginated(&PaginatedTradeFilter)` → sayfalı

**Kritik Özellikler:**

V1 `trades()`: `offset=0, limit=i64::MAX` → sınırsız sorgu. V2 `trades_paginated()`: parametrik offset/limit.

Her iki fonksiyon da `executed_protocol_fees()` çağırır → trade başına fee detayları eklenir.

**Önemli Sınırlama:** `auction_order_uids.len() >= u16::MAX` → hata. sqlx query parametresi limiti koruması.

`trade_from(row, executed_protocol_fees)` — DB satırı → model `Trade`:
- `block_number: u32` (i64 → u32)
- `log_index: u32` (i64 → u32)
- `buy_amount`, `sell_amount`, `sell_amount_before_fees` → BigUint
- `tx_hash: Option<B256>`
- `executed_protocol_fees: Vec<ExecutedProtocolFee>`

**DEX Uygulama Notu — Nova Shield için:**

Nova Shield'da trade geçmişi için V2-style sayfalı endpoint şart. V1 sınırsız → DDoS riski. Protocol fee'leri her trade'e ekle — kullanıcı şeffaflığı için önemli.

---

### [database/fee_policies.rs] (145 satır) — FEE POLİTİKASI

**Amaç:** Executed protocol fee'lerini hesaplar. Üç fee türü desteklenir.

**Fee Türleri:**

1. `Surplus`: Kullanıcının limit fiyatına göre elde ettiği artı değere fee.
   - `surplus_factor`, `surplus_max_volume_factor`

2. `Volume`: İşlem hacmine fee.
   - `volume_factor`

3. `PriceImprovement`: Quote fiyatına göre iyileştirmeye fee.
   - `price_improvement_factor`, `price_improvement_max_volume_factor`
   - Quote gerektirir: `sell_amount`, `buy_amount`, `fee`

**fee_policy_from() — PriceImprovement hesabı (satır 110):**

```
gas_fee = gas_amount * gas_price / sell_token_price
```

BigRational aritmetik kullanılır → hassas bölme. `checked_mul` ve `checked_div` overflow/sıfıra bölme koruması.

**DEX Uygulama Notu — Nova Shield için:**

Bu üç fee modeli Nova Shield DEX için doğrudan kullanılabilir:
- **Surplus Fee:** AMM'ye kıyasla daha iyi fiyat alındıysa fee al — DEX agregasyonu teşvik eder.
- **Volume Fee:** Basit ve öngörülebilir.
- **Price Improvement Fee:** Quote bazlı — advanced kullanıcılar için.

---

### [database/total_surplus.rs] (118 satır) — TOPLAM ARTIK DEĞER SQL

**Amaç:** Kullanıcının tarihsel toplam surplus'ını (limit fiyatına göre artı değer) hesaplar.

**SQL Sorgusu Analizi:**

Üç kaynak: `orders`, `onchain_placed_orders`, `jit_orders` — UNION ALL.

Her kaynak için surplus hesabı:

```sql
# SELL order surplus:
trade_amount = t.buy_amount  (ne kadar buy token alındı)
limit_amount = (t.sell_amount - t.fee_amount) * o.buy_amount / o.sell_amount
             # (sattığın kadar için minimum alınması gereken buy amount)
surplus_in_wei = (trade_amount - limit_amount) * native_price / 10^18

# BUY order surplus:
trade_amount = t.sell_amount - t.fee_amount  (ne kadar sell token kullanıldı)
limit_amount = t.buy_amount * o.sell_amount / o.buy_amount
             # (aldığın kadar için maksimum ödemen gereken sell amount)
surplus_in_wei = (limit_amount - trade_amount) * native_price / 10^18
```

Native fiyat `auction_prices` tablosundan — surplus token'ın ETH değeri.

Mart 2023'ten itibaren veri. `COALESCE(SUM(...), 0)`.

**DEX Uygulama Notu — Nova Shield için:**

Total surplus metriği güçlü bir kullanıcı teşvik göstergesi. Nova Shield'da her order için surplus hesapla ve kullanıcı profiline ekle. Agregasyon sorgusu bu SQL'den doğrudan adapte edilebilir.

---

### [database/app_data.rs] (75 satır) — APP DATA VERİTABANI

Basit CRUD: `get_full_app_data()`, `insert_full_app_data()`.

**InsertError:** `Duplicate | Mismatch(String) | Other(anyhow::Error)`

Mismatch → aynı hash, farklı data → veri bütünlüğü ihlali → hata fırlat.

---

### [database/auctions.rs] (34 satır) — AÇIK ARTIRMA SORGULAMA

- `most_recent_auction()`: En son açık artırmayı JSON olarak yükler, `dto::Auction`'a deserialize eder.
- `last_used_auction_id()`: Son kullanılan açık artırma ID'si.

---

### [database/quotes.rs] (56 satır) — QUOTE DEPOLAMA

`QuoteStoring` trait implementasyonu:
- `save(data: QuoteData)` → `QuoteId` — yeni quote kaydet
- `get(id: QuoteId)` → `Option<QuoteData>` — ID ile sorgula
- `find(params, expiration)` → `Option<(QuoteId, QuoteData)>` — parametrik arama (token pair, side, amount, expiration)

---

### [database/solver_competition.rs] (143 satır) — SOLVER YARIŞMASI V1

`SolverCompetitionStoring` impl:
- `load_competition(Identifier::Id | Identifier::Transaction, after_block)` — V1 JSON format
- `load_latest_competition(after_block)`
- `load_latest_competitions(count)`

`deserialize_solver_competition(json, auction_id, tx_hashes)` — DB JSON → `SolverCompetitionAPI`.

`after_block`: Belirtilmişse deadline geçmemiş yarışmaları gizle.

---

### [database/solver_competition_v2.rs] (179 satır) — SOLVER YARIŞMASI V2

V2 daha zengin veri: clearing prices, solution ranking, per-order executed amounts, reference scores.

`try_into_dto(value: DbResponse)` — Karmaşık dönüşüm:
1. `native_prices: BTreeMap<Address, U256>` — token → fiyat
2. `settlements: HashMap<solution_uid, B256>` — çözüm → TX hash
3. `reference_scores: BTreeMap<Address, U256>` — solver → referans puan
4. `trades: HashMap<solution_uid, Vec<Order>>` — çözüm → order listesi
5. `solutions: Vec<Solution>` — her çözüm için clearing prices + orders + is_winner + filtered_out

`solutions.sort_by_key(|s| Reverse(s.ranking))` — en kötüden en iyiye sıralama (v1 uyumluluğu).

---

### [database/debug_report.rs] (238 satır) — DEBUG RAPORU

**Amaç:** Bir order için tam debug bilgisi toplar — tüm auction'lar, çözüm önerileri, execution'lar, settlement denemeleri, fee politikaları.

`fetch_debug_report(uid)`:
1. `single_order()` — order bilgisi
2. `order_events::get_all()` — tüm events (Created, Ready, Considered, ...)
3. `solver_competition_v2::find_solutions_for_order()` — order'ı içeren çözümler
4. `order_execution::read_by_order_uid()` — execution detayları
5. `trades::trades()` — en fazla 100 trade
6. `auction::fetch_auction_ids_by_order_uid()` — ilgili auction ID'leri
7. `auction::fetch_multiple()` — auction detayları
8. `settlement_executions::read_by_auction_ids()` — settlement denemeleri
9. `fee_policies::fetch_by_order_uid()` — fee politikaları
10. Tüm verileri `build_auctions()` ile birleştir

**Güvenlik:** Bu endpoint yalnızca `restricted/` prefix'i altında. İç tool'lar için.

---

## API HANDLER ANALİZLERİ

### [api/post_order.rs] (377 satır) — ORDER EKLEME HANDLER

**Amaç:** `POST /api/v1/orders` işler, hataları HTTP yanıtlarına dönüştürür.

`post_order_handler(state, Json(order))`:
- `orderbook.add_order(order).await`
- Başarı → 201 Created + `Json(order_uid)`
- Hata → `AddOrderError.into_response()`

**AddOrderError → HTTP Mapping:**
- `OrderValidation(ValidationError)` → `ValidationErrorWrapper.into_response()`
- `DuplicatedOrder` → 400 Bad Request `"DuplicatedOrder"`
- `Database` → 500 Internal Server Error
- `AppDataMismatch` → 500 Internal Server Error (log: "impossible" error)
- `OrderNotFound` → `OrderCancellationError.into_response()`
- `InvalidAppData` → 400 `"InvalidAppData"`
- `InvalidReplacement` → `OrderReplacementError.into_response()`
- `MetadataSerializationFailed` → 500 Internal Server Error

**ValidationError → HTTP Mapping (tam liste):**
- `UnsupportedBuyTokenDestination` → 400
- `UnsupportedSellTokenSource` → 400
- `UnsupportedOrderType` → 400
- `Forbidden` → 403 (deny-listed account)
- `ValidTo::Insufficient` → 400 `"InsufficientValidTo"`
- `ValidTo::Excessive` → 400 `"ExcessiveValidTo"`
- `InvalidNativeSellToken` → 400
- `SameBuyAndSellToken` → 400
- `UnsupportedToken` → 400
- `MissingFrom` → 400
- `AppdataFromMismatch` → 400
- `WrongOwner` → 400
- `InvalidEip1271Signature` → 400
- `InsufficientBalance` → 400
- `InsufficientAllowance` → 400
- `InvalidSignature` → 400
- `NonZeroFee` → 400
- `SellAmountOverflow` → 500
- `TransferSimulationFailed(revert_data)` → 400 (revert data hex encode edilir)
- `QuoteNotVerified` → 400
- `ZeroAmount` → 400
- `IncompatibleSigningScheme` → 400
- `TooManyLimitOrders` → 400
- `TooMuchGas` → 400

**Güvenlik Analizi:**

1. **İmza Doğrulama:** `WrongOwner`, `InvalidSignature`, `InvalidEip1271Signature` hataları → 400 (validation layer'da yapılır, handler'da değil).
2. **Balance Kontrolü:** `InsufficientBalance`, `InsufficientAllowance` → balance simülasyon sonucu.
3. **Token Whitelist:** `UnsupportedToken` → deny list kontrol.
4. **Gas Griefing:** `TooMuchGas` → order başına maksimum gas limiti.
5. **Spam Koruması:** `TooManyLimitOrders` → per-user limit order sayısı.
6. **Zero Amount:** `ZeroAmount` → dust order engeli.
7. **Transfer Simülasyonu:** `TransferSimulationFailed` → sell token transfer edilemiyor. Fee griefing saldırısına karşı.

**DEX Uygulama Notu — Nova Shield için:**

Bu validasyon hata listesi Nova Shield DEX API'si için kapsamlı bir kontrol listesi. Minimum zorunlu kontroller:
- İmza doğrulama (EIP-712 veya native L1 imza)
- Bakiye yeterliliği
- Token whitelist
- Expiry kontrolü (min/max valid_to)
- Gas limiti
- Per-user spam koruması
- Zero amount engeli
- Same token (sell == buy) engeli

---

### [api/post_quote.rs] (309 satır) — QUOTE HANDLER

`post_quote_handler` → `quotes.calculate_quote(&request).await` → `Json(response)`.

**CalculateQuoteError → HTTP:**
- `Price { source, .. }` → `PriceEstimationErrorWrapper(source)` → çeşitli 400/404
- `SellAmountDoesNotCoverFee { fee_amount }` → 400 + `rich_error` (fee_amount verisi eklenir)
- `QuoteNotVerified` → 400
- `Other` → 500

`deserializes_sell_after_fees_quote_request` test: Tam OrderQuoteRequest deserialization.

---

### [api/post_quote_stream.rs] (48 satır) — SSE QUOTE STREAMING

`post_quote_stream_handler`: SSE response döner.

- Stream fail → HTTP hata (early return)
- Her event: `Event::default().json_data(&response)` (başarı) veya `Event::default().event("error").data(...)` (hata)
- `KeepAlive::default()` — bağlantı canlı tutar
- Hata event'leri `filter_map` ile None (skip) değil — error event olarak gönderilir

---

### [api/cancel_order.rs] (145 satır) — CANCEL HANDLER

`cancel_order_handler(state, Path(uid), Json(payload: CancellationPayload))`:
- `OrderCancellation` oluştur
- `orderbook.cancel_order()` → `Json("Cancelled")` | hata

**OrderCancellationError → HTTP:**
- `InvalidSignature` → 400
- `AlreadyCancelled` → 400
- `OrderFullyExecuted` → 400
- `OrderExpired` → 400
- `WrongOwner` → 401 Unauthorized
- `OrderNotFound` → 404 Not Found
- `OnChainOrder` → 400

---

### [api/cancel_orders.rs] (41 satır) — TOPLU CANCEL HANDLER

`cancel_orders_handler(state, Json(cancellation: SignedOrderCancellations))`:
`orderbook.cancel_orders(cancellation).await`

---

### [api/get_trades.rs] (115 satır) — TRADE GEÇMIŞI V1

`QueryParams`: `order_uid: Option<OrderUid>`, `owner: Option<Address>`

Validasyon: Tam olarak biri belirtilmeli (XOR mantığı). Her ikisi veya hiçbiri → 400 `"InvalidTradeFilter"`.

---

### [api/get_trades_v2.rs] (188 satır) — TRADE GEÇMİŞİ V2 (SAYFALAMALI)

`QueryParams`: + `offset: Option<u64>`, `limit: Option<u64>`

**Sabitler:**
- `DEFAULT_OFFSET = 0`
- `DEFAULT_LIMIT = 10`
- `MIN_LIMIT = 1`
- `MAX_LIMIT = 1000`

Limit aralık dışıysa → 400 `"InvalidLimit"`.

---

### [api/get_solver_competition_v2.rs] (77 satır)

- `get_solver_competition_by_id_handler`: `u64` path param (negatif → BAD_REQUEST). `i64::MAX` aşıldıysa NOT_FOUND.
- `get_solver_competition_by_hash_handler`: B256 TX hash.
- `get_solver_competition_latest_handler`: En son yarışma.
- `get_solver_competition_by_id_unfiltered_handler`: Restricted. `after_block=None` → deadline filtresi kapalı.

**Not:** `db()` → `&state.database_write` kullanır. Latency kabul edilemez olduğu için replica kullanılmaz (circuit breaker senaryosu).

---

### [api/get_user_orders.rs] (51 satır)

Limitler: DEFAULT=10, MIN=1, MAX=1000. Aralık dışı → 400 `"LIMIT_OUT_OF_BOUNDS"`.

---

### [api/get_order_status.rs] (29 satır)

`NotFound` → 404. Diğer hatalar → 500.

---

### [api/get_native_price.rs] (23 satır)

`estimate_native_price(token, state.quote_timeout)` → `Json(NativeTokenPrice { price })`.

---

### [api/get_orders_by_uid.rs] (122 satır)

`POST /api/v1/orders/by_uids` — body: `Vec<OrderUid>`. `many_orders()` sonuçlarını döner.

---

### [api/get_orders_by_tx.rs] (23 satır)

`GET /api/v1/transactions/{hash}/orders` — TX hash'e ait order'lar.

---

### [api/get_order_by_uid.rs] (58 satır)

Tekil order sorgulaması. Not found → 404.

---

### [api/get_auction.rs] (25 satır)

`GET /api/v1/auction` — mevcut açık artırma. None ise 404.

---

### [api/get_app_data.rs] (34 satır)

App data hash → full JSON döner. DB sonra IPFS'te arar.

---

### [api/put_app_data.rs] (69 satır)

`PUT /api/v1/app_data` (hash'siz) ve `PUT /api/v1/app_data/{hash}` (hash'li).
Yeni → 201 Created. Mevcut → 200 OK. Her iki durumda hash döner.

---

### [api/get_token_metadata.rs] (23 satır)

`GET /api/v1/token/{token}/metadata` → `TokenMetadata { first_trade_block, native_price }`.

---

### [api/get_total_surplus.rs] (31 satır)

`GET /api/v1/users/{user}/total_surplus` → U256 wei cinsinden toplam surplus.

---

### [api/debug_order.rs] (28 satır)

`GET /restricted/api/v1/debug/order/{uid}` — `DebugReport`. Restricted.

---

### [api/debug_simulation.rs] (74 satır)

- `GET /restricted/api/v1/debug/simulation/{uid}` — mevcut order'ı simüle et.
- `POST /restricted/api/v1/debug/simulation` — keyfi order simülasyonu.

---

### [api/ready.rs] (19 satır)

Readiness probe — DB ve servis hazır mı? `200 OK`.

---

### [api/version.rs] (3 satır)

Servis versiyon bilgisi döner.

---

## DTO (DATA TRANSFER OBJECTS) ANALİZİ

### [dto/mod.rs] (92 satır)

- `OrderSimulationRequest` (satır 22): Debug/simulation için keyfi order parametreleri.
  - `sell_token`, `buy_token`, `sell_amount: NonZeroU256`, `buy_amount: U256`
  - `kind: OrderKind`, `owner: Address`, `receiver: Option<Address>`
  - `sell_token_balance: SellTokenSource`, `buy_token_balance: BuyTokenDestination`
  - `app_data: String` — full JSON
  - `block_number: Option<u64>` — hangi blokta simüle edilecek
  - `signature: Signature` — settlement kontrat doğrulaması için
  - `fee_amount: U256` — imzalanan fee (0 olmalı, ama imza uyumu için)
  - `valid_to: u32`, `partially_fillable: bool`

- `OrderSimulationResult` (satır 69):
  - `tenderly_request: tenderly::dto::Request` — tam Tenderly API isteği
  - `tenderly_url: Option<String>` — paylaşılabilir Tenderly URL
  - `error: Option<String>` — simülasyon hatası

- `TokenMetadata` (satır 84):
  - `first_trade_block: Option<u32>`
  - `native_price: Option<U256>` — ETH cinsinden

### [dto/order.rs] (121 satır)

- `Order` (satır 17): API response order yapısı.
  - `uid`, `sell_token`, `buy_token`, `sell_amount: U256`, `buy_amount: U256`
  - `protocol_fees: Vec<FeePolicy>` — uygulanan fee politikaları
  - `created: u32`, `valid_to: u32`, `kind: OrderKind`
  - `receiver`, `owner`, `partially_fillable`, `executed: U256`
  - `pre_interactions`, `post_interactions: Vec<InteractionData>`
  - `sell_token_balance`, `buy_token_balance`
  - `class: OrderClass` (flatten)
  - `app_data: AppDataHash`
  - `signature: Signature` (flatten)
  - `quote: Quote`

- `Quote` (satır 46): Order'a ait quote verisi.
  - `sell_amount`, `buy_amount`, `fee: U256`
  - `solver: Address`

- `FeePolicy` (satır 59): enum
  - `Surplus { factor: f64, max_volume_factor: f64 }`
  - `Volume { factor: f64 }`
  - `PriceImprovement { factor: f64, max_volume_factor: f64, quote: Quote }`

- `ExecutedAmounts` (satır 76): `sell: U256`, `buy: U256` — solver'ın önerdiği miktarlar

- `SolutionInclusion` (satır 88): Bir solver'ın önerisini temsil eder.
  - `solver: String`
  - `executed_amounts: Option<ExecutedAmounts>` — None = solver bu order'ı işlemedi

- `Status` (satır 99): `#[serde(tag = "type", content = "value")]` — tagged enum
  - `Open` — orderbook'ta ama aktif işlenmyor
  - `Scheduled` — bir sonraki auction'a girecek
  - `Active` — mevcut auction'da, solverlar hesaplıyor
  - `Solved(Vec<SolutionInclusion>)` — bazı solverlar çözüm önerdi (kazanmadı)
  - `Executing(Vec<SolutionInclusion>)` — kazanan çözümü on-chain gönderiyor
  - `Traded(Vec<SolutionInclusion>)` — başarıyla on-chain execute edildi
  - `Cancelled` — kullanıcı iptal etti

### [dto/auction.rs] (34 satır)

- `Auction`: `block: u64`, `orders: Vec<Order>`, `prices: BTreeMap<Address, U256>`, `surplus_capturing_jit_order_owners: Vec<Address>`
- `AuctionWithId`: `id: i64` + flatten `Auction`

---

## KAPSAMLı GÜVENLİK ANALİZİ

### 1. Sybil Attack / Sahte Order Flooding

**Mevcut Korumaları:**
- `TooManyLimitOrders` hatası → per-user limit order sayısı (konfigürasyon: `max_limit_orders_per_user`)
- Market order'lar sayılmaz — yalnızca "market dışı" quote'a sahip order'lar
- Balance/Allowance simülasyonu — bakiyesiz order'lar reddedilir

**Eksiklikler:**
- Rate limiting yok (uygulama katmanında). WAF bekleniyor.
- IP bazlı kısıtlama yok.
- Bot aktivitesi tespiti yok.

**Nova Shield için:** Uygulama katmanında rate limit ekle: `POST /api/v1/orders` için per-IP ve per-address limit. Örn. dakikada 10 order.

---

### 2. Invalid Token Spam

**Mevcut Korumaları:**
- `DenyListedTokens` — token'ı bilinen kötü token listesiyle karşılaştırır
- `UnsupportedToken` hatası — bilinmeyen/desteklenmeyen token'lar reddedilir
- `TransferSimulationFailed` — transfer simülasyonu geçersiz token'ları yakalar

**Nova Shield için:** Token whitelist/blacklist mekanizması zorunlu. Transfer simülasyonu maliyetli ama critical. Honeypot token'larını (transfer revert eden) tespit eder.

---

### 3. Gas Griefing

**Mevcut Korumaları:**
- `TooMuchGas` hatası → order başına maksimum gas (`max_gas_per_order`)
- `ZeroAmount` → dust order engeli
- `SellAmountDoesNotCoverFee` → fee yeterli değil

**Nova Shield için:** Max gas per order konfigürasyonu şart. Dust order threshold da ekle.

---

### 4. Replay Attack

**Mevcut Korumaları:**
- EIP-712 domain separator: `DomainSeparator::new(chain_id, settlement_contract)` — chain + kontrat bağlayan imza
- `valid_to` timestamp — expired order tekrar submit edilemez
- `OrderUid` deduplication — aynı UID ikinci kez insert → `DuplicatedOrder`

**Nova Shield için:** L1'de de EIP-712 benzeri domain separator kullan. Order UID = hash(order_data + owner + nonce) → benzersizlik garantisi. Nonce bazlı yaklaşım da kullanılabilir.

---

### 5. MEV Koruması

CoW Protocol'ün temel MEV koruması batch auction mekanizmasında. Orderbook katmanı order'ları toplar, autopilot batch oluşturur, solver yarışması fiyat dengesini bulur. Bu sayede:
- Front-running engellenir (batch içinde uniform clearing price)
- Sandwich attack imkansız (batch settlement)

Orderbook API tek başına MEV koruması sağlamaz — bu özellik sistem genelinde.

**Nova Shield için:** L1 konsensüs + batch settlement kombinasyonu benzer MEV koruması sağlayabilir.

---

## GENEL TASARIM KARARLARI VE GÖZLEMLER

### 1. Read/Write Separation
İki ayrı Postgres bağlantısı. Query tipleri ayrılmış:
- `database_write`: mutation'lar, solver competition (latency kritik)
- `database_read`: user-facing read sorgular (replica lag kabul edilebilir)

### 2. Event-Driven Status Tracking
Order durumları `order_events` tablosundaki event'lerle takip edilir. Son event = durum. Ayrı `trades` tablosu nihai gerçeği tutar (on-chain event).

### 3. JIT Order Separation
Solver oluşturduğu JIT order'lar ayrı tabloda. Normal user flow etkilenmez.

### 4. Metrics-First Design
Her DB sorgusu ve API endpoint'i Prometheus metriği ile sarılmış. Latency histogram'ları, hata sayaçları → observability.

### 5. App Data Indirection
Order → AppDataHash (32 byte) → Full JSON (IPFS veya DB). Hash on-chain, full data off-chain. Zincir depolama maliyetini minimize eder.

### 6. Tenderly Integration (Debug)
Restricted endpoint'ler aracılığıyla simülasyon ve debug imkanı. Production'da aktif kalabilir — infra güvenli.

---

## DEX UYGULAMASI — NOVA SHIELD EYLEM PLANI

### API Tasarımı

Nova Shield DEX API'si için minimum endpoint seti:

```
POST   /api/v1/orders                    # Order submit
DELETE /api/v1/orders/{uid}             # Off-chain cancel (ECDSA only)
GET    /api/v1/orders/{uid}             # Order sorgulama
GET    /api/v1/orders/{uid}/status      # Durum takibi (event-driven)
GET    /api/v1/account/{owner}/orders   # Kullanıcı order listesi
POST   /api/v1/quote                    # Price quote
GET    /api/v1/trades                   # Trade geçmişi
GET    /api/v1/token/{token}/price      # Native price
GET    /api/v1/ready                    # Health probe
GET    /api/v1/version                  # Versiyon
```

### Güvenlik Öncelikleri (Önem Sırası)

1. **EIP-712 Domain Separator** — chain_id + kontrat adresi bağlayan imza. L1'de zorunlu.
2. **Balance Simülasyonu** — order kabul öncesi bakiye kontrolü.
3. **Token Blacklist** — bilinen kötü token'lar reddedilsin.
4. **Rate Limiting** — uygulama katmanında. IP + adres bazlı.
5. **Max Limit Orders per User** — spam önleme.
6. **Min/Max Valid_To** — expiry sınırları.
7. **Max Gas per Order** — gas griefing önleme.
8. **Zero Amount Check** — dust order engeli.

### Volume Fee Formülü (Nova Shield için)

```rust
// SELL order:
protocol_fee = buy_amount * fee_factor
adjusted_buy = buy_amount - protocol_fee

// BUY order:
total_volume = sell_amount + gas_fee
protocol_fee = total_volume * fee_factor
adjusted_sell = sell_amount + protocol_fee

// U512 widening multiply ile taşma koruması:
scaled_factor = U256::from(factor.to_high_precision())
scale = U512::from(HIGH_PRECISION_SCALE)
fee = U256::try_from(amount.widening_mul(scaled_factor).checked_div(scale)?)
```

### Surplus Hesaplama (Nova Shield için)

```
# SELL order user surplus:
surplus = (actual_buy - limit_buy) * native_price_buy_token

# BUY order user surplus:
surplus = (limit_sell - actual_sell) * native_price_sell_token
```

Bu metric kullanıcı memnuniyeti göstergesi ve fee hesabı için kullanılabilir.

### Order Lifecycle Events (Nova Shield için)

```
Created   → order DB'ye eklendi
Scheduled → bir sonraki L1 block'a dahil edilecek
Active    → validator'lar hesaplıyor
Solved    → en az bir validator çözüm önerdi
Executing → kazanan validator L1'e gönderiyor
Traded    → L1'de başarıyla execute edildi
Cancelled → kullanıcı iptal etti
Filtered  → bu round'da dışarıda bırakıldı (geçici)
```

### L1 Orderbook vs Off-Chain Orderbook

CoW Protocol tamamen off-chain orderbook (order submission + matching). L1 yalnızca settlement.

Nova Shield seçenekleri:
1. **Hibrit:** Off-chain orderbook + L1 settlement (CoW benzeri). En düşük latency, en düşük gas.
2. **On-Chain Orderbook:** L1'de order tutan smart contract. Merkeziyetsiz ama pahalı.
3. **L1 + Off-Chain Sıralama:** Order L1'de commit, off-chain eşleşme, L1 settlement. Replay ve MEV direnci.

**Öneri:** Nova Shield için hibrit yaklaşım. Order'lar L1 block'a dahil edilmeden önce off-chain orderbook'ta biriktirilsin, her block'ta batch settlement.

---

## ÖZET TABLO

| Dosya | Satır | Önem | Açıklama |
|-------|-------|------|----------|
| `database/orders.rs` | 1265 | Kritik | Tüm order CRUD, durum hesaplama, JIT |
| `api.rs` | 1018 | Kritik | Tüm endpoint routing, middleware, hata mapping |
| `orderbook.rs` | 936 | Kritik | Ana iş mantığı: add/cancel/replace/status |
| `quoter.rs` | 816 | Yüksek | Volume fee hesaplama, streaming quote |
| `run.rs` | 617 | Yüksek | Servis init, dependency wiring |
| `api/post_order.rs` | 377 | Kritik | Order ekle handler + validasyon hataları |
| `api/post_quote.rs` | 309 | Yüksek | Quote handler |
| `database/debug_report.rs` | 238 | Orta | Debug raporu toplama |
| `database/solver_competition_v2.rs` | 179 | Orta | V2 yarışma DTO dönüşümü |
| `database/trades.rs` | 195 | Yüksek | Trade sorgulama + fee |
| `database/fee_policies.rs` | 145 | Yüksek | 3 fee politikası hesaplama |
| `api/cancel_order.rs` | 145 | Yüksek | İptal handler |
| `app_data.rs` | 129 | Yüksek | App data registry |
| `ipfs_app_data.rs` | 172 | Orta | IPFS önbellekli erişim |
| `database/total_surplus.rs` | 118 | Orta | Karmaşık SQL ile surplus hesaplama |

**Toplam Analiz:** 50 dosya, 8.665 satır — tamamı analiz edildi.
