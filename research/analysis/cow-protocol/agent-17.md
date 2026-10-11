# CoW Protocol Analiz — Agent 17
## Kapsam: autopilot/src/ — boundary/ + database/ + infra/ tam derinlik
## DEX Özel Protokolü: 3x Hassasiyet
## Tarih: 2026-07-22

---

## DOSYA LİSTESİ (62 dosya)

```
boundary/events/mod.rs              boundary/events/settlement.rs
boundary/mod.rs                     boundary/order.rs
database/auction_prices.rs          database/auction.rs
database/competition.rs             database/ethflow_events/event_retriever.rs
database/ethflow_events/event_storing.rs  database/ethflow_events/mod.rs
database/events.rs                  database/fee_policies.rs
database/mod.rs                     database/onchain_order_events/ethflow_events.rs
database/onchain_order_events/event_retriever.rs  database/onchain_order_events/mod.rs
database/order_events.rs            database/quotes.rs
infra/api.rs                        infra/blockchain/contracts.rs
infra/blockchain/mod.rs             infra/mod.rs
infra/persistence/dto/auction.rs    infra/persistence/dto/fee_policy.rs
infra/persistence/dto/mod.rs        infra/persistence/dto/order.rs
infra/persistence/dto/quote.rs      infra/persistence/mod.rs
infra/shadow/mod.rs                 infra/shadow/orderbook/mod.rs
infra/solvers/dto/mod.rs            infra/solvers/dto/reveal.rs
infra/solvers/dto/settle.rs         infra/solvers/dto/solve.rs
infra/solvers/mod.rs
+ database crate: auction.rs, auction_prices.rs, fee_policies.rs,
                  leader_pg_lock.rs, orders.rs, settlements.rs
+ database migrations: V001, V016, V102, V103, V112 vb.
```

---

## 1. BOUNDARY KATMANI

### [boundary/mod.rs] (48 satır)

**Amaç:** Dış kütüphanelerden (model, shared, database) kullanılan tipleri tek yerden re-export eder; boundary katmanı ile domain katmanı arasındaki köprü.

**Struct/Enum Listesi:**
- `SolvableOrders`: Auction için filtrelenmiş, aktif order seti.
  - `orders: HashMap<domain::OrderUid, Arc<model::order::Order>>` — UID'den tam Order'a map
  - `quotes: HashMap<domain::OrderUid, Arc<domain::Quote>>` — UID'den Quote'a map
  - `latest_settlement_block: u64` — son settlement bloğu (watermark)
  - `fetched_from_db: chrono::DateTime<Utc>` — DB snapshot timestamp (monoton artış için kullanılır)

**Kritik Fonksiyonlar:**
- `web3_client(ethrpc: &Url, ethrpc_args: &shared::web3::Arguments) -> Web3` — ethrpc args'dan web3 client üretir.

**Güvenlik:**
- `SolvableOrders.fetched_from_db` checkpoint olarak kullanılır: "en az bu andaki veriler sistemdedir" garantisi. Race condition koruması için REPEATABLE READ isolation seviyesiyle birlikte çalışır (bkz. database/auction.rs).

**DEX Uygulama Notu — Nova Shield için:**
- Bu struct Nova Shield'ın "aktif emir cache" tasarımının doğrudan karşılığıdır. L1'de RocksDB column family: `cf_solvable_orders` (UID → SerializedOrder). Cache yenilenirken block watermark aynı transaction içinde yazılmalı (RocksDB WriteBatch).
- `fetched_from_db` yerine Nova Shield'da `fetched_from_block: u64` kullanmak daha blockchain-native olur.

---

### [boundary/order.rs] (53 satır)

**Amaç:** `model::order::Order` (boundary tipi) → `domain::Order` (internal domain tipi) dönüşümü.

**Kritik Fonksiyonlar:**
- `to_domain(order: &model::order::Order, protocol_fees: Vec<domain::fee::Policy>, quote: Option<domain::Quote>) -> domain::Order`
  - `remaining_amounts::Order::from(order.clone())` ile kalan miktarı hesaplar.
  - `order_is_untouched = remaining_order.executed_amount.is_zero()` — eğer order hiç execute edilmemişse pre_interactions eklenir; aksi hâlde boş bırakılır. Bu MEV koruması açısından kritik: pre-hook'lar (approve vb.) sadece ilk execution'da çalışır.
  - `created: u32::try_from(order.metadata.creation_date.timestamp()).unwrap_or(u32::MIN)` — unix timestamp, `u32::MIN` fallback (yani panic yok, sessiz hata).

**Güvenlik:**
- `unwrap_or(u32::MIN)` çok geniş: 1970-01-01 tarihini simulate eder. Nova Shield için bu değerin validasyon ile kontrol edilmesi önerilir.
- pre_interactions sadece `order_is_untouched` ise dahil edilir: partial fill saldırısına karşı savunma — önceden execute edilmiş order'larda pre-hook'ların tekrar çalışması engellenir.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da order domain dönüşümü sırasında `executed_amount` RocksDB'den `cf_order_execution` column family'den okunabilir. Partial fill durumlarında pre_interactions atlanmalı (aynı güvenlik prensibi).

---

### [boundary/events/mod.rs] (27 satır)

**Amaç:** On-chain settlement event'lerinin son indexlenmiş blok numarasını DB'den okur/yazar.

**SQL Queries:**
- `write_last_block_to_db`: `database::last_indexed_blocks::update(&mut ex, contract, i64_block)` — sqlx parameterized query, SQL injection yok.
- `read_last_block_from_db`: `database::last_indexed_blocks::fetch(&mut ex, contract)` — parameterized.

**Güvenlik:** Tüm sorgular parameterized (sqlx). SQL injection riski yok.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da RocksDB `cf_last_indexed_blocks` column family key=contract_address, value=u64 blok. Atomik yazma WriteBatch ile.

---

### [boundary/events/settlement.rs] (87 satır)

**Amaç:** GPv2Settlement sözleşmesinin Trade/Settlement/Invalidation/PreSignature event'lerini indexler.

**Struct/Enum Listesi:**
- `GPv2SettlementContract { provider: AlloyProvider, address: Address }` — Alloy tabanlı event retriever
- `Indexer { db: Postgres, start_indexing_block: u64 }` — EventStoring implementasyonu

**Kritik Tasarım:**
- `replace_events`: DB transaction içinde önce mevcut events silinir (`database::events::delete`), ardından settlement tablosundan ilgili blok aralığı temizlenir (`database::settlements::delete`), sonra yeni events eklenir. Tüm bu işlemler tek `pool.begin() ... transaction.commit()` bloğu içinde — atomik reorg güvenliği.
- `append_events`: Transaction içinde sadece event append eder. Reorg olmayan durumlar için.
- `INDEX_NAME = "settlements"` — `last_indexed_blocks` tablosunda kullanılan anahtar.

**Güvenlik:**
- Blockchain reorg durumunda `replace_events` atomik transaction garantisi sağlar. Kısmi yazma durumu yok.
- `AlloyEventRetrieving::filter()` — sadece `self.address` (GPv2Settlement contract) filtrelemesi — diğer contract logları işlenmez.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield L1'de settlement event indexleme: RocksDB WriteBatch ile atomik. Reorg tespiti: block hash kontrolü. `cf_settlement_events` column family, key=(block_number, log_index), value=SerializedSettlementEvent.

---

## 2. DATABASE KATMANI

### [database/mod.rs] (217 satır)

**Amaç:** `Postgres` struct'ı tanımlar; PgPool yönetimi, DB metrik sistemi.

**Struct/Enum Listesi:**
- `Config { insert_batch_size: NonZeroUsize, max_pool_size: NonZeroU32 }` — insert_batch_size default 500, max_pool_size DB_MAX_CONNECTIONS_DEFAULT
- `Postgres { pool: PgPool, config: Config }` — tüm DB işlemlerinin merkezi yapısı

**Kritik Fonksiyonlar:**
- `Postgres::new(url, config)`: `PgPoolOptions::new().max_connections(...).connect(url)` — connection pool başlatır. Hemen metrics task spawn eder.
- `start_db_metrics_job(pool)`: Her 5 saniyede active/idle connection sayısını Prometheus'a yazar.
- `gin_clean_pending_list()`: `SELECT gin_clean_pending_list('competition_auctions_order_uids_gin')` — GIN index'in pending listesini temizler. GIN indexler INSERT'lerde pending list biriktirir, bu da periyodik latency spike'larına yol açar. Çözüm: auction save sonrası async spawn ile temizleme.
- `count_rows_in_table`: `SELECT COUNT(*) FROM {table}` — **dinamik format string** kullanır. Ancak `table` parametresi her zaman `database::TABLES` sabit dizisinden geldiği için SQL injection riski yoktur. Kullanıcı inputu değil.
- `estimate_rows_in_table`: `SELECT reltuples::bigint FROM pg_class WHERE relname='{table}'` — aynı durum, sabit kaynak.
- `update_large_tables_stats()`: `ANALYZE {table}` — her saat bir kez çalışır, query planner istatistiklerini günceller.

**SQL Güvenliği Detayı:**
- Tablo isimleri sabit dizilerden (`database::TABLES`, `database::LARGE_TABLES`) geldiği için format string interpolasyonu güvenlidir.
- Tüm kullanıcı kaynaklı veri sqlx parameterized binding ile ($1, $2...).

**Prometheus Metrikleri:**
- `table_rows` (IntGaugeVec, label: table): tablo satır sayıları
- `autopilot_database_queries` (HistogramVec, label: type): sorgu süreleri (0.005s — 90s bucket aralığı)
- `database_active_connections`, `database_idle_connections` (IntGauge)

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da PostgreSQL yerine RocksDB kullanılacak. GIN index problemi yoktur fakat RocksDB compaction latency spike'ları benzer sorun yaratabilir. Çözüm: manual compaction trigger + prometheus metriği.
- Connection pool metrik pattern'i aynen alınabilir: tokio::spawn ile periyodik metrics update.

---

### [database/auction.rs] (137 satır) — YÜKSEK ÖNEMLİ

**Amaç:** Auction state'ini DB'ye persist etme; solvable orders, next auction ID, current auction replacement.

**SQL Queries — TAM DETAY:**

**1. Solvable Orders Sorgusu (`all_solvable_orders`, satır 64-103):**
```sql
SET TRANSACTION ISOLATION LEVEL REPEATABLE READ
-- Sonra:
-- database::orders::solvable_orders(&mut ex, i64::from(min_valid_to))
-- (tam SQL: bkz. database/orders.rs solvable_orders() fonksiyonu)
```
- REPEATABLE READ isolation: Aynı transaction içindeki her iki SELECT (orders + latest_settlement_block) aynı DB snapshot'ından okur.
- **Race condition koruması:** İki ayrı SELECT arasında başka bir transaction yeni order ekleyemez/değiştiremez.
- **SELECT FOR UPDATE yok** — pessimistic lock kullanılmıyor; REPEATABLE READ yeterli çünkü sadece okuma yapılıyor.

**2. Get Next Auction ID:**
```sql
SELECT nextval(pg_get_serial_sequence('auctions', 'id'))::bigint as next_id;
```
- PostgreSQL sequence kullanır — atomic, concurrent-safe. İki eş zamanlı call asla aynı ID'yi döndürmez.

**3. Replace Current Auction (insert_auction_with_id):**
```sql
WITH deleted AS (
    DELETE FROM auctions
)
INSERT INTO auctions (id, json)
VALUES ($1, $2::jsonb);
```
- CTE içinde DELETE + INSERT atomik: Eski auction silinir, yeni auction eklenir. Tek transaction.
- Auction state: JSON blob olarak `auctions` tablosunda saklanır. Şema: `{id, json}`.
- `$2::jsonb` — PostgreSQL tipi dönüşümü; JSON validasyonu DB tarafında yapılır.

**Kritik Not:** Auction JSON'u tokio::task::spawn_blocking ile serialize eder — serialization CPU-bound iş ana tokio runtime'ı bloklamaz. Bu pattern Nova Shield için aynen alınmalı.

**Race Condition Analizi:**
- `replace_current_auction` her zaman TÜM mevcut auction'ları siler (DELETE FROM auctions) — sadece 1 auction tutulur. Bu tasarımsal bir karar: "current auction" her zaman tektir.
- Paralel yazma koruması: `upload_queue: mpsc::UnboundedSender<AuctionUpload>` — FIFO sırası. İki thread aynı anda yazamaz; kanal aracılığıyla serileştirilir (bkz. `Persistence::spawn_db_upload_task`).

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da "current auction" state: RocksDB'de `cf_current_auction` key=0 (singleton), value=SerializedAuction(protobuf).
- Auction ID: monoton artan i64, RocksDB atomic increment (merge operator) ile implement edilebilir.
- FIFO serializasyonu: tokio::mpsc::unbounded_channel ile CoW'dan aynı pattern alınabilir.
- JSON yerine protobuf kullanımı: daha compact, daha hızlı serialize/deserialize.

---

### [database/competition.rs] (83 satır) — YÜKSEK ÖNEMLİ

**Amaç:** Solver competition sonuçlarını DB'ye kaydeder: reference scores, auction prices, solver competition table.

**Struct/Enum Listesi:**
- `Competition`:
  - `auction_id: AuctionId` (i64)
  - `reference_scores: HashMap<Address, Score>` — Her solver'ın CIP20 referans skoru
  - `participants: HashSet<Address>` — Ödül alacak katılımcı adresleri
  - `prices: BTreeMap<Address, U256>` — Token -> native price (auction'daki external prices)
  - `block_deadline: u64` — Settlement bu blok öncesi onchain'e gitmeli
  - `competition_simulation_block: u64` — Simulation yapılan blok
  - `competition_table: SolverCompetitionDB` — Tüm solver solution detayları (JSON)

**Kritik Fonksiyon: `save_competition`:**
```
1. tokio::task::spawn_blocking(|| serde_json::to_string(&competition_table))  -- async serialize
2. pool.begin()  -- transaction başlat
3. database::reference_scores::insert(&mut ex, &reference_scores)
4. database::auction_prices::insert(&mut ex, prices_vec)
5. json.await  -- serialization bitmesini bekle (paralel çalışıyordu)
6. database::solver_competition::save(&mut ex, auction_id, &json)
7. ex.commit()
```
**Tasarım Kararı:** JSON serialization ve DB sorguları (1-4) paralel çalışır — CPU ve I/O eş zamanlı.

**SQL Queries:**
- `reference_scores::insert`: solver → reference_score mapping per auction. Parameterized.
- `auction_prices::insert` (DEPRECATED): `INSERT INTO auction_prices (auction_id, token, price)`. 5000'lik batch.
  - **Not:** Bu tablo deprecated (not: auction_prices.rs içinde açıkça belirtilmiş — "duplicated data in competition_auctions"). Solver team hâlâ kullanıyor.
- `solver_competition::save`: auction_id → competition JSON.

**DEX Uygulama Notu — Nova Shield için:**
- Solver competition state: RocksDB `cf_solver_competition` key=auction_id, value=CompetitionProto.
- Reference scores: `cf_reference_scores` key=(auction_id, solver_address), value=U256.
- Paralel serialization + DB write pattern korunmalı.

---

### [database/events.rs] (171 satır)

**Amaç:** GPv2Settlement sözleşmesinden gelen onchain event'leri (Trade, Settlement, Invalidation, PreSignature) DB'ye uygun formata çevirir.

**Struct/Enum Listesi:**
- `ValidatedLog { block: i64, tx_hash: TransactionHash, log_index: i64 }` — Alloy Log validasyonu; eksik field varsa hata döner.

**Kritik Fonksiyonlar:**
- `contract_to_db_events(contract_events)`: GPv2SettlementEvents → Vec<(EventIndex, Event)>
  - `Trade` event: order_uid, sell_amount_including_fee, buy_amount, fee_amount
  - `Settlement` event: solver address, transaction_hash
  - `OrderInvalidated` event: order_uid
  - `PreSignature` event: owner, order_uid, signed(bool)
  - **`Interaction` event: None döner** — TODO yorumu var, henüz handle edilmiyor.
- `append_events(transaction, events)`: DB transaction içinde `database::events::append` çağırır.
- `replace_events(transaction, events, from_block)`: Önce `database::events::delete(transaction, from_block)` (reorg silme), sonra `append`. Atomik.

**convert_trade detayı:**
```rust
Trade {
    order_uid: bytes_to_order_uid(&trade.orderUid.0)?,  // 56 byte kontrolü
    sell_amount_including_fee: u256_to_big_decimal(&trade.sellAmount),
    buy_amount: u256_to_big_decimal(&trade.buyAmount),
    fee_amount: u256_to_big_decimal(&trade.feeAmount),
}
```
- `bytes_to_order_uid`: 56 byte olmayan UID'ler hata döndürür (not panic).

**Güvenlik:**
- Tüm event dönüşümleri `filter_map` + `Result` zinciriyle; parse hatası olan event'ler sessizce atlanır (sadece Trade/Settlement/Invalidation/PreSignature). Bu güvenli çünkü kötü event'ler index'i bozmaz.
- `Interaction` event'lerinin atlanması — TODO — gelecekte MEV-related interaction verisinin indexlenmesi için önemli.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da on-chain event indexleme: Alloy event retriever + RocksDB storage. Event tipleri aynı.
- `Interaction` event'lerini indekslemek Nova Shield DEX için kritik: flashloan, hook, MEV koruması analizi için.

---

### [database/fee_policies.rs] (19 satır — autopilot katmanı)

**Amaç:** `domain::fee::Policy` nesnelerini batch olarak `fee_policies` tablosuna yazar.

**Kritik Fonksiyon: `insert_batch`:**
- `dto::fee_policy::from_domain(auction_id, order_uid, policy)` ile domain'den DB formatına dönüştürür.
- `database::fee_policies::insert_batch(ex, fee_policies)` çağırır.

**DEX Uygulama Notu — Nova Shield için:**
- Fee policy per order per auction: RocksDB `cf_fee_policies` key=(auction_id, order_uid, policy_index), value=FeePolicyProto.

---

### [database/order_events.rs] (41 satır)

**Amaç:** Order lifecycle event'lerini (Created, Traded, Filtered, vb.) kaydeder; eski event'leri temizler.

**Kritik Fonksiyonlar:**
- `delete_order_events_before(timestamp)`: Zaman tabanlı temizleme — `order_events::delete_order_events_before(&self.pool, timestamp)`.
- `store_order_events(ex, order_uids, label, reason, timestamp)`:
  - order_uids'i 1000'lik chunk'lara böler: `for chunk in order_uids.chunks(1000)`
  - Her chunk için `order_events::insert_order_events(&mut ex, chunk, ...)`.
  - **N+1 problemi yok** — tek transaction içinde chunked batch insert.
  - Hata durumunda sadece `tracing::warn!` — kritik değil, sadece debug verisi.

**DEX Uygulama Notu — Nova Shield için:**
- Order lifecycle events: RocksDB `cf_order_events` key=(order_uid, timestamp), value=EventLabel. TTL ile eski event'ler otomatik silinebilir.

---

### [database/auction_prices.rs] (24 satır — autopilot katmanı)

**Amaç:** En güncel auction fiyatlarını DB'den çeker (token → price map).

**SQL Queries:**
- `fetch_latest_prices()`: `database::auction_prices::fetch_latest_prices` çağırır.
  ```sql
  SELECT * FROM auction_prices WHERE auction_id = (
      SELECT MAX(auction_id) FROM auction_prices
  )
  ```
  - Subquery ile en yüksek auction_id bulunur, o auction'ın tüm fiyatları çekilir.
  - Index kullanımı: `auction_prices` tablosunda auction_id üzerinde index olmalı (explicit migration göremedim — implicit PRIMARY KEY üzerinden olabilir).

**DEX Uygulama Notu — Nova Shield için:**
- Latest auction prices: RocksDB `cf_latest_prices` key=token_address, value=U256. Her yeni auction'da WriteBatch ile güncellenir.

---

### [database/quotes.rs] (62 satır — autopilot katmanı)

**Amaç:** Quote kaydetme/getirme (QuoteStoring trait implementasyonu); expired quote temizleme; order'lara ait quote'ları okuma.

**SQL Queries:**
- `save(data: QuoteData)`: `database::quotes::save(&mut ex, &row)` — INSERT INTO quotes.
- `get(id: QuoteId)`: `database::quotes::get(&mut ex, id)` — SELECT by ID.
- `find(params, expiration)`: `database::quotes::find(&mut ex, &params)` — parametreli arama (sell_token, buy_token, sell_amount vb.)
- `remove_expired_quotes()`: `database::quotes::remove_expired_quotes(&mut ex, Utc::now())`.
- `read_quotes(orders)`:
  ```sql
  -- database::orders::read_quotes(&mut ex, &order_uids)
  -- order_uids: Vec<ByteArray>
  -- Her order için ilgili quote'u döndürür
  ```
  - N+1 sorunu yok: tek sorgu ile tüm order'ların quote'ları okunur.
  - Hata durumunda `tracing::warn!` — kayıp quote'lar için.

**Fee Hesabı (dto/quote.rs'den):**
```
fee = (gas_amount * gas_price) / sell_token_price
```
- Matematiksel: gas maliyetini sell token cinsinden ifade eder.
- BigRational aritmetiği: hassas hesap, overflow kontrollü (`CheckedMul`, `CheckedDiv`).
- Sıfıra bölme koruması: `DivisionByZero` error variant.

**DEX Uygulama Notu — Nova Shield için:**
- Quote storage: RocksDB `cf_quotes` key=quote_id, value=QuoteProto. TTL ile expiry.
- Fee formülü aynen alınmalı. sell_token_price native price cacheinden okunacak.

---

### [database/ethflow_events/] (3 dosya)

**Amaç:** Ethereum native token akışlarını (CoWSwapEthFlow contract) indexler.

**event_retriever.rs:**
- `EthFlowRefundRetriever { web3: Web3, addresses: Vec<Address> }` — CoWSwapEthFlow::OrderRefund event'lerini filtreler.
- `AlloyEventRetrieving` implementasyonu: `Filter::new().event_signature(FilterSet::from_iter([OrderRefund::SIGNATURE_HASH])).address(self.addresses.clone())`
- Assertion: `assert!(!addresses.is_empty())` — en az 1 adres zorunlu (startup panic).

**event_storing.rs:**
- `EventStoring<(CoWSwapEthFlowEvents, Log)>` implementasyonu `Postgres` için.
- `append_events`: `get_refunds(events)` → `insert_refund_tx_hashes` — ethflow_orders tablosuna.
- `replace_events`: `delete_refunds(range.start..range.end)` sonra `insert_refund_tx_hashes` — atomik transaction.
- INDEX_NAME = "ethflow_refunds"

**ETH→WETH Pre-Interaction:**
```rust
pre_interaction: Interaction {
    target: ByteArray(order.owner.0),   // ethflow contract
    value: BigDecimal::new(0.into(), 1), // 0 ETH value
    data: WRAP_ALL_SELECTOR.to_vec(),   // 0x4c84c1c8 = wrap_all()
    index: 0,
    execution: ExecutionTime::Pre,
}
```
- ETH order'lar otomatik olarak WETH'e wrap edilir. Bu pre-interaction sayesinde CoW Protocol native ETH order'ları destekler.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da native token wrapping: benzer pre-interaction mekanizması. Kullanıcı ETH gönderir, settlement öncesi WETH'e wrap edilir.
- `WRAP_ALL_SELECTOR = 0x4c84c1c8` — özel contract fonksiyonu.

---

### [database/onchain_order_events/] (3 dosya) — YÜKSEK ÖNEMLİ

**Amaç:** Onchain order placement event'lerini parse eder ve indexler (CoWSwapOnchainOrders contract).

**Struct/Enum Listesi:**
- `OnchainOrderParser<EventData, EventRow>`: Generic parser.
  - `db: Postgres`
  - `web3: Web3`
  - `quoter: Arc<dyn OrderQuoting>` — quote servisi
  - `custom_onchain_data_parser: Box<dyn OnchainOrderParsing<EventData, EventRow>>` — Strategy pattern
  - `domain_separator: DomainSeparator` — EIP-712 domain
  - `settlement_contract: Address`
  - `trampoline: HooksTrampoline::Instance` — hook mekanizması

- `OnchainOrderCustomData<T> { quote_id: i64, additional_data: Option<T> }` — custom parser output

**Trait: `OnchainOrderParsing<EventData, EventRow>`:**
- `parse_custom_event_data(contract_events)` → `Vec<(EventIndex, OnchainOrderCustomData<EventData>)>`
- `append_custom_order_info_to_db(ex, custom_data)` → `Result<()>`
- `customized_event_data_for_event_index(event_index, order, hashmap, onchain_order_placement)` → `EventRow`
- **Tasarım:** Strategy pattern. EthFlow ve PreSign için ayrı implementasyonlar.

**EventStoring implementasyonu:**
- `replace_events`: `delete_events` (mark_as_reorged + delete_invalidations) + `insert_events` — atomik.
- `append_events`: sadece `insert_events`.
- INDEX_NAME = "onchain_orders"

**insert_events akışı:**
1. OrderPlacement event'leri filtrele
2. Invalidation event'leri ayır → `invalided_order_uids`
3. `extract_custom_and_general_order_data` çağır:
   - Block timestamp'lerini al
   - Custom parser ile özel verileri parse et
   - `parse_general_onchain_order_placement_data` ile genel order verisini oluştur
4. `onchain_invalidations::insert_onchain_invalidations`
5. `custom_parser.append_custom_order_info_to_db`
6. `onchain_broadcasted_orders::append`
7. `insert_quotes` (sadece başarılı placement'lar için)
8. `insert_order_hooks` (trampoline aracılığıyla)

**Reorg Koruması:**
```rust
database::onchain_broadcasted_orders::mark_as_reorged(
    transaction,
    i64::try_from(*range.start()).unwrap_or(i64::MAX),
)
```
- Silmek yerine "reorged" olarak işaretleme: veri kaybı yok, audit trail korunur.

**EthFlow Custom Parser (ethflow_events.rs):**
- `WRAP_ALL_SELECTOR = hex!("4c84c1c8")`
- `convert_to_quote_id_and_user_valid_to`: `data[0..8]` = i64 quote_id (BE), `data[8..12]` = u32 user_valid_to (BE). Toplam 12 byte enforced.
- Test: `test_convert_to_quote_id_and_user_valid_to` — encoding doğrulanmış.
- `determine_ethflow_indexing_start` ve `determine_ethflow_refund_indexing_start`: Çeşitli fallback stratejileri (DB → config → deployment block).

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da onchain order placement (zincir üzerinden order): aynı strategy pattern. Her order type (market, limit, flashloan) için ayrı implementasyon.
- Reorg koruması: mark_as_reorged pattern RocksDB'de: `cf_onchain_orders` value'suna `reorged: bool` flag eklenir.

---

## 3. DATABASE CRATE SQL KATMANI

### [crates/database/src/auction.rs] (174 satır) — KRİTİK SQL

**SQL Queries — TAM:**

**auctions tablosu (cache):**
```sql
-- Yeni auction ID alma:
SELECT nextval(pg_get_serial_sequence('auctions', 'id'))::bigint as next_id;

-- Mevcut auction'ı sil, yenisini ekle:
WITH deleted AS (DELETE FROM auctions)
INSERT INTO auctions (id, json) VALUES ($1, $2::jsonb);

-- En son auction:
SELECT id, json FROM auctions ORDER BY id DESC LIMIT 1;
```

**competition_auctions tablosu (kalıcı kayıt):**
```sql
-- Kaydetme:
INSERT INTO competition_auctions (id, block, deadline, order_uids, price_tokens, price_values, surplus_capturing_jit_order_owners)
VALUES ($1, $2, $3, $4, $5, $6, $7);

-- Getirme:
SELECT * FROM competition_auctions WHERE id = $1;
SELECT * FROM competition_auctions WHERE id = ANY($1) ORDER BY id;
SELECT order_uids FROM competition_auctions WHERE id = $1;

-- Order'ın hangi auction'larda olduğunu bul (GIN index kullanır):
SELECT id FROM competition_auctions WHERE order_uids @> ARRAY[$1::bytea] ORDER BY id;
```

**GIN Index:**
```sql
-- V112 migration:
CREATE INDEX CONCURRENTLY IF NOT EXISTS competition_auctions_order_uids_gin
    ON competition_auctions USING GIN (order_uids);
```
- `order_uids @> ARRAY[$1]` (contains operator) GIN index'i kullanır.
- GIN indexler INSERT'lerde pending list biriktirir → periyodik `gin_clean_pending_list()` çağrısı.

**DEX Uygulama Notu — Nova Shield için:**
- `competition_auctions` → RocksDB `cf_competition_auctions` key=auction_id. Order UID indexi: `cf_order_to_auctions` key=order_uid, value=Vec<auction_id> (ya da secondary index crate kullanımı).
- GIN index → Nova Shield'da order_uid secondary index: per order_uid, hangi auction'larda olduğu.

---

### [crates/database/src/settlements.rs] (214 satır) — KRİTİK SQL

**SQL Queries — TAM:**

```sql
-- Settlement transaction bul:
SELECT tx_hash FROM settlements
WHERE auction_id = $1 AND solver = $2 AND solution_uid = $3;

-- Auction bağlanmamış settlement'ları bul (post-processing için):
SELECT block_number, log_index, tx_hash FROM settlements
WHERE auction_id IS NULL
ORDER BY block_number ASC;

-- Settlement'a auction_id bağla:
UPDATE settlements SET auction_id = $1
WHERE block_number = $2 AND log_index = $3;

-- Settlement'a solver ve solution_uid bağla:
UPDATE settlements SET solver = $1, solution_uid = $2
WHERE block_number = $3 AND log_index = $4;

-- Reorg'da temizleme (cascade):
DELETE FROM order_execution WHERE block_number >= $1;
DELETE FROM jit_orders WHERE block_number >= $1;
```

**İki Aşamalı Settlement Lifecycle:**
1. On-chain event indexlendiğinde: `auction_id = NULL`, sadece `block_number, log_index, tx_hash`.
2. Post-processing sırasında: `UPDATE settlements SET auction_id = ...` (hangi auction'a ait olduğu bulunur).
3. Winner doğrulanınca: `UPDATE settlements SET solver = ..., solution_uid = ...`.

**Güvenlik:**
- Tüm SQL parameterized ($1, $2...). SQL injection yok.
- Reorg temizleme: `block_number >= $1` — cascade delete.
- Transaction isolation: settlement kayıt işlemleri `pool.begin()...commit()` içinde.

**DEX Uygulama Notu — Nova Shield için:**
- Settlement state makinesi: iki aşamalı. RocksDB'de: `cf_settlements` key=(block_number, log_index), value=SettlementProto {tx_hash, auction_id?, solver?, solution_uid?}.
- Nullable field yerine Option<> kullanımı.

---

### [crates/database/src/fee_policies.rs] (205 satır) — KRİTİK SQL

**SQL Queries — TAM:**

```sql
-- Batch insert:
INSERT INTO fee_policies (auction_id, order_uid, kind, surplus_factor,
    surplus_max_volume_factor, volume_factor, price_improvement_factor,
    price_improvement_max_volume_factor)
VALUES (...), (...), ...;  -- QueryBuilder ile çoklu row

-- Fetch (INNER JOIN ile filtreli):
SELECT * FROM fee_policies fp
INNER JOIN (VALUES ($1, $2), ($3, $4), ...) AS vals(order_uid, auction_id)
ON (fp.order_uid, fp.auction_id) = (vals.order_uid, vals.auction_id)
ORDER BY fp.application_order;

-- Order UID'e göre fetch:
SELECT * FROM fee_policies WHERE order_uid = $1
ORDER BY auction_id, application_order;
```

**Fee Policy Tipleri:**
- `Surplus { surplus_factor, surplus_max_volume_factor }` — Surplus tabanlı ücret
- `Volume { volume_factor }` — Hacim tabanlı ücret
- `PriceImprovement { price_improvement_factor, price_improvement_max_volume_factor }` — Fiyat iyileştirme ücreti (quote gerektirir)

**application_order:** Politikalar sıralı uygulanır. Bu field hangi policy'nin önce uygulandığını belirler.

**DEX Uygulama Notu — Nova Shield için:**
- Fee policy per order per auction: RocksDB `cf_fee_policies` key=(auction_id, order_uid, application_order), value=FeePolicyProto.
- PriceImprovement policy quote gerektirir: `cf_quotes` ile join.

---

### [crates/database/src/leader_pg_lock.rs] (92 satır) — RACE CONDITION KRİTİK

**Amaç:** Multi-instance autopilot'ta tek bir leader seçmek için PostgreSQL advisory lock kullanır.

**SQL Queries — TAM:**
```sql
-- Lock almayı dene (non-blocking):
SELECT pg_try_advisory_lock(hashtextextended($1, 0));

-- Lock'ı bırak:
SELECT pg_advisory_unlock(hashtextextended($1, 0));

-- Session canlılık kontrolü:
SELECT 1;
```

**Algoritma:**
1. Zaten leader ise: `ping()` ile session kontrolü. Connection ölmüşse lock otomatik gider, `lock_guard = None`.
2. Leader değilse ve son denemeden `try_every` geçtiyse: `pg_try_advisory_lock` dene.
3. Lock alındıysa: `lock_guard = Some(PgLockGuard { conn, key })` — connection TUTULUYOR.

**Kritik Detay:** `pg_advisory_lock` session-scoped — connection'ı kapatmak lock'ı otomatik bırakır. Bu yüzden `PgLockGuard` içinde `conn` tutulur (connection kapatılmaz).

**`hashtextextended($1, 0)`:** Lock key'i string'den i64'e hash'ler. Collision teorik olarak mümkün ama pratikte ihmal edilir.

**Rate Limiting:** `last_try` + `try_every: Duration(200ms)` — lock denemesi çok sık yapılmaz.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield L1 validator node: advisory lock yerine in-process singleton veya Raft consensus ile leader election.
- Multi-node için: etcd veya ZooKeeper tabanlı distributed lock (ya da Raft).
- Mevcut leader_pg_lock pattern: tek DB'ye bağımlı — Nova Shield'da SPF (Single Point of Failure). Dağıtık sistemde leader election farklı olmalı.

---

### [crates/database/src/orders.rs] (kısmi — solvable_orders sorgusu)

**solvable_orders SQL Sorgusu — TAM DETAY:**

```sql
WITH live_orders AS MATERIALIZED (
    SELECT o.*
    FROM   orders o
    WHERE  o.cancellation_timestamp IS NULL
        AND o.true_valid_to >= $1          -- min_valid_to filtresi
        AND NOT EXISTS (SELECT 1 FROM invalidations i WHERE i.order_uid = o.uid)
        AND NOT EXISTS (SELECT 1 FROM onchain_order_invalidations oi WHERE oi.uid = o.uid)
        AND NOT EXISTS (SELECT 1 FROM onchain_placed_orders op WHERE op.uid = o.uid AND op.placement_error IS NOT NULL)
        AND NOT EXISTS (SELECT 1 FROM ethflow_refunds r WHERE r.order_uid = o.uid)
),
trades_agg AS (
    SELECT t.order_uid,
        SUM(t.buy_amount) AS sum_buy,
        SUM(t.sell_amount) AS sum_sell,
        SUM(t.fee_amount) AS sum_fee
    FROM trades t
    JOIN live_orders lo ON lo.uid = t.order_uid
    GROUP BY t.order_uid
)
SELECT
    lo.*,
    COALESCE(ta.sum_buy, 0)  AS sum_buy,
    COALESCE(ta.sum_sell, 0) AS sum_sell,
    COALESCE(ta.sum_fee, 0)  AS sum_fee,
    false AS invalidated,
    (lo.signing_scheme = 'presign' AND COALESCE(pe.unsigned, TRUE)) AS presignature_pending,
    ARRAY(SELECT (p.target, p.value, p.data) FROM interactions p WHERE p.order_uid = lo.uid AND p.execution = 'pre' ORDER BY p.index) AS pre_interactions,
    ARRAY(SELECT (p.target, p.value, p.data) FROM interactions p WHERE p.order_uid = lo.uid AND p.execution = 'post' ORDER BY p.index) AS post_interactions,
    ed.ethflow_data,
    opo.onchain_user,
    NULL AS onchain_placement_error,
    COALESCE(fee_agg.executed_fee, 0) AS executed_fee,
    ...
FROM live_orders lo
LEFT JOIN LATERAL (SELECT NOT signed AS unsigned FROM presignature_events WHERE order_uid = lo.uid ORDER BY block_number DESC, log_index DESC LIMIT 1) pe ON TRUE
LEFT JOIN LATERAL (SELECT sender AS onchain_user FROM onchain_placed_orders WHERE uid = lo.uid ORDER BY block_number DESC LIMIT 1) opo ON TRUE
LEFT JOIN LATERAL (SELECT ROW(tx_hash, eo.valid_to) AS ethflow_data FROM ethflow_orders eo LEFT JOIN ethflow_refunds r ON r.order_uid = eo.uid WHERE eo.uid = lo.uid) ed ON TRUE
LEFT JOIN LATERAL (SELECT SUM(executed_fee) AS executed_fee, (ARRAY_AGG(executed_fee_token))[1] AS executed_fee_token FROM order_execution WHERE order_uid = lo.uid) fee_agg ON TRUE
LEFT JOIN app_data ad ON ad.contract_app_data = lo.app_data
LEFT JOIN trades_agg ta ON ta.order_uid = lo.uid
WHERE ((lo.kind = 'sell' AND COALESCE(ta.sum_sell,0) < lo.sell_amount) OR
       (lo.kind = 'buy'  AND COALESCE(ta.sum_buy ,0) < lo.buy_amount))
```

**Sorgu Analizi:**
- `MATERIALIZED` CTE: live_orders bir kez hesaplanır, sonraki JOIN'lerde cache kullanılır. Büyük veri setlerinde performans kritik.
- 4x `NOT EXISTS` subquery: invalidation, onchain_invalidation, placement_error, refund kontrolü.
- 4x `LEFT JOIN LATERAL`: presignature_events, onchain_placed_orders, ethflow_orders, order_execution — her order için en son durumu getirir. LATERAL sayesinde correlated subquery gibi çalışır ama daha verimli.
- Partial fill filtresi: `sum_sell < sell_amount` (sell order) veya `sum_buy < buy_amount` (buy order) — tam dolmuş order'lar hariç tutulur.

**Index Kullanımı:**
- `orders.true_valid_to` — BTREE index (V102 migration). `>= $1` range scan.
- `invalidations.order_uid` — index gerekli (NOT EXISTS için).
- `trades.order_uid` — COVERING index (V103): `trades_covering ON trades (order_uid) INCLUDE (buy_amount, sell_amount, fee_amount)` — heap erişimi yok.
- `user_order_creation_timestamp ON orders (owner, creation_timestamp DESC)` (V016).

**N+1 Sorunu:**
- `ARRAY(SELECT ...)` subquery'leri sorgu başına çalışır. Ancak tüm live_orders için tek seferlik SQL — stream olarak okunur (sqlx::fetch stream). N+1 yok.

**Transaction Isolation:**
- `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` — her iki SELECT aynı snapshot.

**DEX Uygulama Notu — Nova Shield için:**
- Bu sorgu Nova Shield'da doğrudan RocksDB scan'a çevrilemez (kompleks join). Strateji:
  1. `cf_active_orders` (in-memory HashMap, RocksDB'den bootstrap) — hızlı erişim.
  2. Invalidation/refund state: ayrı column family'ler (`cf_invalidations`, `cf_refunds`).
  3. Partial fill: `cf_order_execution` — executed_amount per order.
  4. Her block'ta invalidated + fully filled order'ları `cf_active_orders`'dan çıkar.
  5. Full rescan yerine incremental update (`solvable_orders_after` pattern).

---

## 4. INFRA KATMANI

### [infra/api.rs] (179 satır)

**Amaç:** Autopilot'un internal HTTP API'si — native token price endpoint.

**Struct/Enum Listesi:**
- `State { estimator: Arc<dyn NativePriceEstimating>, allowed_timeout: RangeInclusive<Duration> }` — Axum state
- `NativePriceQuery { timeout_ms: Option<u64> }` — Query parametresi

**Route:** `GET /native_price/{token}` → native ETH fiyatını döndürür.

**Timeout Mantığı:**
```rust
let timeout = query.timeout_ms.map(Duration::from_millis)
    .unwrap_or(*state.allowed_timeout.end())
    .clamp(MIN_TIMEOUT, *state.allowed_timeout.end());
```
- MIN_TIMEOUT = 250ms — çok kısa timeout'lar clamp edilir.
- Kullanıcı isteği `max_timeout`'u aşamaz.

**Error Mapping:**
- `NoLiquidity` / `EstimatorInternal` → 404 Not Found
- `UnsupportedToken` → 400 Bad Request
- `RateLimited` → 429 Too Many Requests
- `TradingOutsideAllowedWindow`, `TokenTemporarilySuspended`, `InsufficientLiquidity`, `CustomSolverError` → 400 Bad Request (mesaj ile)
- `ProtocolInternal` → 500 Internal Server Error

**DEX Uygulama Notu — Nova Shield için:**
- Native price API: Nova Shield'da gRPC endpoint (Axum yerine tonic). Timeout propagation aynı şekilde.
- Price estimation hatası → Nova Shield'da order filtering'e girer (MissingNativePrice reason).

---

### [infra/blockchain/mod.rs] (204 satır)

**Amaç:** Ethereum blockchain bağlantısı — transaction, trace, current block.

**Struct/Enum Listesi:**
- `Rpc { web3: Web3, chain: Chain, url: Url }` — Ham RPC bağlantısı
- `Ethereum { web3: Web3, unbuffered_web3: Web3, chain: Chain, current_block: CurrentBlockWatcher, contracts: Contracts }` — Full Ethereum erişim katmanı

**Kritik Fonksiyonlar:**
- `Ethereum::transaction(hash)`:
  - `tokio::try_join!(get_transaction_receipt, fetch_debug_trace)` — paralel.
  - `unbuffered_web3` debug trace için: bazı provider'lar batched debug call'ı desteklemez.
  - `debug_traceTransaction` + `CallTracer` — tam call tree.

- `fetch_debug_trace(provider, hash)`:
  - `de.disable_recursion_limit()` + `serde_stacker::Deserializer` — derin call stack deserialization için. Normal serde 128 recursion limitine çarpar; bu 5000+ derinlikli çağrıları handle eder.
  - Örnek derin tx: Tenderly link'i ile dokümante edilmiş.

**Error Enum:**
```rust
pub enum Error {
    Alloy(alloy::transports::TransportError),
    IncompleteTransactionData(anyhow::Error),
    TransactionNotFound,
    UnsupportedChain,
    DeserializationFailed(serde_json::Error),
}
```

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield L1'de kendi blockchain'i var; Ethereum RPC yerine internal block/tx API.
- Call trace'ler MEV koruması için: settlement'ın hangi contract'ları çağırdığını analiz eder.
- `disable_recursion_limit` — bu pattern derin data structure parse'ında gerekli; Nova Shield'da da alınabilir.

---

### [infra/blockchain/contracts.rs] (167 satır)

**Amaç:** GPv2Settlement ve ilgili tüm kontratların instance'larını yönetir.

**Struct/Enum Listesi:**
- `Contracts`:
  - `settlement: GPv2Settlement::Instance` — Ana settlement kontratı
  - `signatures: contracts::support::Signatures::Instance` — İmza doğrulama
  - `weth: WETH9::Instance` — Native token wrap
  - `balances: Balances::Instance` — Token bakiye sorgulama
  - `chainalysis_oracle: Option<ChainalysisOracle::Instance>` — Yasaklı adres kontrolü (optional)
  - `trampoline: HooksTrampoline::Instance` — Pre/post hook yürütme
  - `flashloan_router: Address` — Flashloan yönlendirme
  - `authenticator: GPv2AllowListAuthentication::Instance` — Solver whitelist
  - `settlement_domain_separator: eth::DomainSeparator` — EIP-712 imza alanı

- `Addresses`: Her kontrat için opsiyonel override adresi (test/custom deployment için).

**Kontrat İnit Sırası:**
1. settlement → authenticator adresini çek (`settlement.authenticator().call().await`)
2. settlement → domain separator'ı çek (`settlement.domainSeparator().call().await`)
3. `ChainalysisOracle::Instance::deployed(&web3.provider)` — deployed değilse None

**ChainalysisOracle:** Opsiyonel anti-money-laundering oracle. Yasaklı adresler order filtreleme aşamasında reddedilir.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield L1'de smart contract katmanı: Alloy ile aynı pattern.
- `settlement_domain_separator`: Her chain/network için farklı. Nova Shield testnet/mainnet ayrımı.
- `flashloan_router`: Nova Shield DEX'inde flashloan desteklenecekse bu adres gerekli.
- ChainalysisOracle: compliance için benzer mechanism — on-chain banned list veya off-chain oracle.

---

### [infra/persistence/mod.rs] (1052 satır) — MERKEZ PERSISTENCE KATMANI

**Amaç:** Tüm persistence işlemlerinin orchestrator'ı. Domain ile database arasındaki köprü.

**Struct/Enum Listesi:**
- `Persistence { s3: Option<s3::Uploader>, postgres: Arc<Postgres>, upload_queue: mpsc::UnboundedSender<AuctionUpload> }`
- `AuctionUpload { auction_id: domain::auction::Id, auction_data: RawAuctionData }` — upload queue item

**Kritik Tasarımlar:**

**1. Auction Upload Queue (FIFO serializasyonu):**
```rust
fn spawn_db_upload_task(db: Arc<Postgres>) -> mpsc::UnboundedSender<AuctionUpload> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<AuctionUpload>();
    tokio::task::spawn(async move {
        while let Some(upload) = receiver.recv().await {
            db.replace_current_auction(upload.auction_id, upload.auction_data).await
        }
    });
    sender
}
```
- Birden fazla caller `replace_current_auction_in_db` çağırırsa hepsi FIFO sıraya girer.
- En son `send()` olan en son işlenir — bu "sonuncu auction her zaman kazanır" mantığı.

**2. PostgreSQL LISTEN/NOTIFY:**
```rust
listener.listen("new_order").await
// ...
match listener.recv().await {
    Ok(notification) => notify.notify_one(),  // run loop tetiklenir
}
```
- Yeni order eklendiğinde `new_order` kanalından bildirim → auction run loop hemen başlar.
- Reconnect loop: bağlantı koparsa 5 saniye bekle ve yeniden bağlan.

**3. solvable_orders_after (incremental update) — KRİTİK:**
```
1. SET TRANSACTION ISOLATION LEVEL REPEATABLE READ
2. updated_order_uids_after(tx, after_block)   -- hangi UID'ler değişti?
3. open_orders_by_time_or_uids(tx, updated_uids, after_timestamp)  -- yeni order datası
4. latest_settlement_block(tx)
5. Invalidated/fulfilled order'ları current_orders'dan çıkar
6. Yeni/güncel order'ları ekle
7. Expired order'ları filtrele (true_valid_to < min_valid_to)
8. Yeni quote'ları oku (sadece onchain veya yeni yaratılan order'lar için)
```
- **N+1 yok:** Tek transaction içinde birkaç targeted sorgu.
- **Incremental:** Tüm DB'yi yeniden okumak yerine `after_block`'tan sonra değişen order'ları günceller.

**4. get_auction (post-processing) — KRİTİK:**
```
1. database::auction::fetch(ex, auction_id)         -- competition_auctions tablosu
2. fee_policies::fetch_all(ex, relevant_orders)      -- order-level fee policies
3. postgres.read_quotes(pi_orders)                   -- sadece PriceImprovement için
4. domain::settlement::Auction oluştur
```
- JIT order tespiti: `fee_policies` map'inde olmayan order'lar JIT olarak değerlendirilir.

**5. save_settlement — KRİTİK AKIŞ:**
```
1. update_settlement_auction (auction_id bağla)
2. update_settlement_solver (solver + solution_uid)
3. store_order_events (OrderEventLabel::Traded)
4. order_execution::save (per order: total fee, protocol fee breakdown)
5. JIT orders → jit_orders::insert (trade event ile eşleştirerek)
6. commit()
```

**6. store_settlement_execution_started/ended:**
- Settlement başladığında: `settlement_executions::insert(auction_id, solver, solution_uid, start_timestamp, start_block, deadline_block)`
- Settlement bittiğinde: `settlement_executions::update(... end_timestamp, end_block, outcome)`

**7. fetch_in_flight_orders:**
```rust
solver_competition_v2::fetch_in_flight_orders(&mut ex, current_block.cast_signed())
```
- O anda execution'da olan order'lar — bir sonraki auction'a alınmaz (revert riski).

**8. S3 Upload:**
```rust
pub fn upload_auction_to_s3(&self, id, auction) {
    if auction.orders.is_empty() { return; }
    tokio::task::spawn(async { s3.upload(id.to_string(), auction_dto).await });
}
```
- Boş auction'lar S3'e yüklenmez.
- Async spawn: blocking değil.

**SQL Güvenliği:**
- Tüm sorgular parameterized. SQL injection yok.
- Transaction isolation: `REPEATABLE READ` kullanılan yerlerde belgelenmiş.
- `SELECT FOR UPDATE` hiçbir yerde kullanılmıyor — optimistic concurrency model.

**Race Condition Analizi:**
- Auction upload: mpsc queue ile serileştirilmiş.
- Leader lock: pg_advisory_lock ile tek instance aktif.
- Order cache: `REPEATABLE READ` ile consistent snapshot.
- Settlement kaydetme: tek transaction — partial state yok.

**DEX Uygulama Notu — Nova Shield için:**
- `Persistence` katmanının tamamı Nova Shield'da RocksDB ile implement edilecek.
- LISTEN/NOTIFY → Nova Shield'da tokio watch::channel veya broadcast::channel.
- S3 upload → Nova Shield'da IPFS ya da S3-compatible object store.
- `fetch_in_flight_orders`: Nova Shield'da in-memory HashSet — RocksDB lookup gerekmez.
- Incremental update pattern (`solvable_orders_after`) RocksDB'de: `cf_updated_order_uids` (block_number → Vec<order_uid>) secondary index.

---

### [infra/persistence/dto/] (5 dosya)

**auction.rs:**
- `RawAuctionData { block: u64, orders: Vec<Order>, prices: BTreeMap<Address, U256>, surplus_capturing_jit_order_owners: Vec<Address> }` — JSON-serializable auction snapshot.
- `Auction { id: AuctionId, #[serde(flatten)] auction: RawAuctionData }` — ID ile birlikte.
- `from_domain` / `try_into_domain` dönüşümleri.

**order.rs (329+ satır):**
- `Order` DTO: tüm order field'ları + `protocol_fees: Vec<FeePolicy>` + `quote: Option<Quote>`.
- `from_domain` / `to_domain` dönüşümleri.
- Pre/post interaction dönüşümleri.
- Signature dönüşümleri (Eip712/EthSign/Eip1271/PreSign).
- `class: boundary::OrderClass::Limit` — hardcoded: autopilot sadece limit order'larla çalışır.

**fee_policy.rs (103 satır):**
- `FeePolicy` enum: `Surplus { factor, max_volume_factor }`, `PriceImprovement { factor, max_volume_factor, quote }`, `Volume { factor }`.
- DB ↔ Domain dönüşümleri.
- `PriceImprovement` için quote zorunlu (`MissingQuote` error).

**quote.rs (54 satır):**
- `into_domain(quote: database::orders::Quote) → Result<domain::Quote, QuoteError>`
- **Fee hesabı:**
  ```
  fee = (gas_amount * gas_price) / sell_token_price
  ```
  - gas_amount: f64 (tahmin edilen gas miktarı)
  - gas_price: f64 (wei cinsinden gas fiyatı)
  - sell_token_price: f64 (ETH/token oranı)
  - Sonuç: sell token cinsinden protocol fee miktarı.
  - `BigRational` aritmetik: hassas, overflow-safe.

**DEX Uygulama Notu — Nova Shield için:**
- Fee hesabı formülü sabit alınmalı. Gas price değişkeni Nova Shield'da: L1 block fee market.
- DTO pattern: protobuf kodegen kullanımı JSON yerine daha verimli.

---

### [infra/solvers/mod.rs] (187 satır) — SOLVER İLETİŞİM KATMANI

**Amaç:** HTTP üzerinden driver'larla (solver'lar) iletişim.

**Struct/Enum Listesi:**
- `Driver { name: String, url: Url, submission_address: eth::Address, client: Client }` — tek solver bağlantısı

**Kritik Fonksiyonlar:**
- `solve(request: solve::Request) → Result<solve::Response>` — POST /solve
- `reveal(request: reveal::Request) → Result<reveal::Response>` — POST /reveal
- `settle(request: &settle::Request, timeout: Duration) → Result<()>` — POST /settle

**`settle` özelliği:**
- Diğer endpoint'lerden farklı: response body beklemiyor, sadece 200 OK kontrolü.
- `X-REQUEST-ID: auction_id` header gönderir — driver tarafında duplicate detection için.
- Ayrı timeout parametresi — settle deadline tabanlı.

**`request_response` generic helper:**
- `RESPONSE_SIZE_LIMIT = 10_000_000` (10MB) — DoS koruması.
- `RESPONSE_TIME_LIMIT = 60s` — genel client timeout.
- `tracing_headers()` ile distributed tracing propagation.
- `X-REQUEST-ID` header ile request tracking.

**KMS Entegrasyonu:**
- `Account::Kms(key_id)` → `AwsSigner::new(kms_client, key_id)` — AWS KMS ile submission address.
- `Account::Address(address)` → direct address.

**`solve::Request` (solve.rs):**
- `auction_id: i64, body: Bytes, content_encoding: Option<HeaderValue>, deadline: DateTime<Utc>`
- Brotli compression: `CompressorWriter::new(Vec::new(), 4096, 1, 22)` (quality=1, lgwin=22=4MB).
- Serialize-once reuse: `body: bytes::Bytes` — clone() ucuz (Arc referans sayımı).
- `X-Auction-Id` header — driver duplicate detection için.
- `X-Content-Encoding: br` header — brotli compression sinyali.
- `body_to_string()`: compressed ise `"<compressed>"` döner — log boyutu koruması.

**`reveal::Request/Response` (reveal.rs):**
- Request: `{ solution_id: u64, auction_id: i64 }`
- Response: `{ calldata: { internalized: Vec<u8>, uninternalized: Vec<u8> } }`
- `internalized`: MEV korumalı calldata (internalized interactions gizlenir).
- `uninternalized`: ham calldata (circuit breaker doğrulaması için).

**`settle::Request` (settle.rs):**
- `{ solution_id: u64, submission_deadline_latest_block: u64, auction_id: i64 }`

**`solve::Solution` (solve.rs):**
- `solution_id: u64` — driver-local unique ID
- `submission_address: Address` — settlement gönderen adres
- `orders: HashMap<OrderUid, TradedOrder>` — çözümde yer alan order'lar
- `clearing_prices: HashMap<Address, U256>` — **DEPRECATED** (log edilir, uyarı verilir)
- `gas: Option<u64>` — tahmini gas

**`TradedOrder`:**
- `side, sell_token, buy_token, limit_sell, limit_buy, executed_sell, executed_buy`
- limit_sell/limit_buy: order'ın limit fiyatı
- executed_sell/executed_buy: gerçek execute edilen miktarlar (slippage dahil)

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da solver iletişimi: HTTP yerine gRPC (tonic). Daha iyi tip güvenliği, streaming.
- Brotli compression: auction JSON büyük (~3-4MB) olabilir. Nova Shield'da da kullan.
- Duplicate detection: `X-Auction-Id` header → Nova Shield'da gRPC metadata.
- `clearing_prices` DEPRECATED: Nova Shield'da bu field hiç eklenmemeli.
- `internalized`/`uninternalized` ayrımı: MEV koruması için kritik. Nova Shield DEX'inde aynı pattern.

---

### [infra/shadow/orderbook/mod.rs] (32 satır)

**Amaç:** Shadow mode — üretim orderbook'undan auction verisini çeker.

**Struct:** `Orderbook { client: reqwest::Client, url: Url }`

**Tek endpoint:** `GET /api/v1/auction` → `domain::Auction` döner.

**Kullanım:** Shadow autopilot, üretim sistemini "dinler" — kendi auction'larını üretim auction'ıyla karşılaştırır. Deployment güvenliği için.

**DEX Uygulama Notu — Nova Shield için:**
- Shadow mode: Nova Shield'da staging → production karşılaştırması için aynı pattern kullanılabilir.

---

### [leader_lock_tracker.rs] (131 satır)

**Amaç:** Multi-instance autopilot'ta leader durumunu takip eder.

**Enum:**
- `LeaderLockTracker::Disabled` — single instance mod.
- `LeaderLockTracker::Enabled { is_leader: bool, was_leader: bool, leader_lock: LeaderLock }` — multi instance.

**Kritik:**
- `just_stepped_up()`: `is_leader: true, was_leader: false` → yeni leader. Cache'i ısıtma işlemi tetiklenir.
- `is_leader()`: Disabled ise her zaman true döner (single instance her zaman lider).
- `try_acquire()`: Her run loop iteration'ında çağrılır.
- `release()`: Shutdown'da çağrılır — lock bırakılır, metriklere yazılır.

**Prometheus Metrikleri:**
- `leader_lock_tracker_is_leader` (IntGauge): 1=lider, 0=follower
- `leader_lock_tracker_leader_lock_error` (IntCounter): hata sayısı

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield validator leader seçimi için bu yapı alınabilir. Ancak `LeaderLock` backend Raft veya dağıtık KV store olmalı.

---

## 5. CROSS-CUTTING CONCERNS ANALİZİ

### SQL Injection Güvenliği
- **Sonuç: TEMİZ.** Tüm kullanıcı kaynaklı veri sqlx parameterized binding ($1, $2...) ile.
- Dinamik format string (`count_rows_in_table`, `estimate_rows_in_table`) sadece `database::TABLES` sabit array'inden gelir — SQL injection imkânsız.
- QueryBuilder kullanımı: `push_bind()` ile değer bağlama — güvenli.

### N+1 Query Problemi
- **Sonuç: YOK.** Öne çıkan pattern'ler:
  - `order_events`: 1000'lik chunk'lara bölerek batch insert.
  - `fee_policies::fetch_all`: INNER JOIN VALUES ile batch fetch.
  - `solvable_orders`: tek SQL sorgusu, streaming.
  - `read_quotes`: tek sorgu, tüm order_uid'ler için.
  - `get_auction`: 3 ayrı targeted sorgu (auction + fee_policies + quotes).

### Transaction Isolation
- `REPEATABLE READ`: `all_solvable_orders`, `solvable_orders_after` — consistent snapshot garantisi.
- Default (READ COMMITTED): Diğer tüm işlemler.
- `SELECT FOR UPDATE`: Hiçbir yerde kullanılmıyor — optimistic model.

### Race Condition Koruması
- **Auction state:** mpsc FIFO queue + pg_advisory_lock (leader election).
- **Event indexing:** atomik transaction (replace = DELETE + INSERT).
- **Reorg handling:** `mark_as_reorged` + atomik transaction.
- **Solvable orders:** REPEATABLE READ isolation.

### Concurrency Modeli
- Tokio async + blocking pool (CPU-bound serialization için `spawn_blocking`).
- `Arc<Postgres>` — pool paylaşımı thread-safe.
- `mpsc::UnboundedSender` — auction upload serializasyonu.

---

## 6. KRİTİK BULGULAR VE NOVA SHIELD DEX STRATEJİSİ

### 6.1 State Management Kararı: PostgreSQL → RocksDB + In-Memory

CoW Protocol'deki PostgreSQL rolleri ve Nova Shield eşdeğerleri:

| CoW Protocol | Nova Shield |
|---|---|
| `auctions` tablo (JSON) | `cf_current_auction` (singleton, protobuf) |
| `competition_auctions` tablo | `cf_competition_auctions` (key=auction_id) |
| `fee_policies` tablo | `cf_fee_policies` (key=auction_id+order_uid+idx) |
| `settlements` tablo | `cf_settlements` (key=block_number+log_index) |
| `order_events` tablo | `cf_order_events` (TTL ile) |
| `auction_prices` tablo | `cf_auction_prices` (key=auction_id+token) |
| `last_indexed_blocks` tablo | `cf_last_indexed_blocks` (key=contract) |
| In-memory order cache | `cf_active_orders` + HashMap |
| pg_advisory_lock | Raft leader election |
| LISTEN/NOTIFY | tokio broadcast channel |

### 6.2 Auction ID Yönetimi
CoW'da: `SELECT nextval(pg_get_serial_sequence('auctions', 'id'))`
Nova Shield'da: RocksDB atomic increment. `MERGE` operator veya separate `cf_sequences` column family.

### 6.3 GIN Index Eşdeğeri
CoW'da: `competition_auctions_order_uids_gin` — order → auction lookup.
Nova Shield'da: `cf_order_to_auctions` secondary index (order_uid → Vec<auction_id>). WriteBatch ile her auction save'de güncellenir.

### 6.4 Solvable Orders Sorgusunun RocksDB'ye Çevirimi
Bu komplex SQL sorgu doğrudan RocksDB'ye çevrilemez. Strateji:

```
Boot:
1. cf_active_orders'ı PostgreSQL-benzeri full scan ile doldur (ya da snapshottan yükle)
2. cf_last_indexed_blocks'tan son blok al

Per block:
3. updated_order_uids(after_block) → değişen UID'leri bul
4. Her UID için: invalid? refunded? fully_filled? → cf_active_orders'tan çıkar
5. Yeni/güncel order'lar: cf_active_orders'a ekle
6. min_valid_to kontrolü: expired'ları filtrele
```

Incremental pattern: O(değişen order sayısı) — O(toplam order sayısı) değil.

### 6.5 MEV Koruması
- Pre-interaction sadece untouched order için (`order_is_untouched` kontrolü) → partial fill saldırısı engeli.
- `internalized`/`uninternalized` calldata ayrımı — MEV bot görünürlüğünü azaltır.
- `in_flight_orders` → aktif settlement'daki order'lar bir sonraki auction'a alınmaz.
- Batch auction mekanizması: aynı blok içinde tüm order'lar toplu çözülür — frontrunning fırsatı azalır.

### 6.6 Leader Lock ve Zero-Downtime Deployment
CoW'da:
- `pg_advisory_lock` ile tek autopilot aktif.
- Follower'lar cache'i sıcak tutar (`just_stepped_up` → cache refresh).
- Deployment: yeni instance ayağa kalkar → lock alır → eskisi leader bırakır.

Nova Shield'da:
- Raft consensus ile leader election (daha sağlam, SPF yok).
- Aynı "warm cache" pattern.

### 6.7 Auction Persistence: JSON vs Protobuf
CoW'da auction JSON olarak saklanır (`$2::jsonb`). Nova Shield için:
- Protobuf: 5-10x daha compact, 3-5x daha hızlı serialize.
- Binary format RocksDB ile daha uyumlu.
- Dezavantaj: human-readable değil (debug için tools gerekli).

---

## 7. ÖZET TABLO

| Bileşen | Satır | Önem | Kilit Özellik |
|---|---|---|---|
| boundary/mod.rs | 48 | Yüksek | SolvableOrders checkpoint |
| boundary/order.rs | 53 | Yüksek | Domain dönüşümü, pre-interaction güvenliği |
| boundary/events/settlement.rs | 87 | Kritik | Atomik reorg koruması |
| database/mod.rs | 217 | Yüksek | PgPool, metrics, GIN clean |
| database/auction.rs | 137 | Kritik | REPEATABLE READ, FIFO queue, spawn_blocking |
| database/competition.rs | 83 | Kritik | Paralel serialize+DB, CIP20 scores |
| database/events.rs | 171 | Yüksek | Event dönüşüm, reorg replace |
| database/onchain_order_events/mod.rs | ~360 | Kritik | Strategy pattern, mark_as_reorged |
| database/onchain_order_events/ethflow_events.rs | 364 | Yüksek | ETH→WETH pre-interaction |
| infra/persistence/mod.rs | 1052 | KRİTİK | Tüm persistence orchestration |
| infra/solvers/mod.rs | 187 | Yüksek | Solver HTTP, brotli compress |
| infra/solvers/dto/solve.rs | 372 | Yüksek | Request/Response DTO, clearing_prices deprecated |
| infra/blockchain/mod.rs | 204 | Yüksek | debug_traceTransaction, serde_stacker |
| infra/blockchain/contracts.rs | 167 | Yüksek | Contract suite, chainalysis oracle |
| database crate/auction.rs | 174 | Kritik | CTE DELETE+INSERT, GIN index, nextval |
| database crate/settlements.rs | 214 | Kritik | İki-aşamalı lifecycle, cascade delete |
| database crate/fee_policies.rs | 205 | Kritik | application_order, batch INSERT/INNER JOIN |
| database crate/leader_pg_lock.rs | 92 | Kritik | pg_advisory_lock, session-scoped |
| database crate/orders.rs (solvable) | ~120 SQL | Kritik | Kompleks CTE, MATERIALIZED, LATERAL |
| leader_lock_tracker.rs | 131 | Yüksek | Zero-downtime leader election |
