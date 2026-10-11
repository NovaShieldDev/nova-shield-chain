# CoW Protocol Analiz — Agent 20

## Kapsam: database/ + shared/ — TAM DERİNLİK (43 dosya)

## DEX Özel Protokolü: 3x Hassasiyet

**Tarih:** 2026-07-22
**Analiz Eden:** Agent 20
**Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/libs/cow-protocol/`

---

# BÖLÜM 1 — DATABASE CRATE (25 Dosya)

## 1.1 — `crates/database/src/lib.rs` (131 satır)

### Amaç

Tüm PostgreSQL tablo isimlerini merkezi olarak tanımlar; temel type alias'lar ve yardımcı fonksiyonlar burada bulunur. Diğer tüm `database/` modülleri bu tip alias'ları kullanır.

### Tablo/Struct Tanımı

```rust
pub const TABLES: &[&str] = &[
    "order_quotes", "orders", "invalidations", "ethflow_orders",
    "ethflow_refunds", "onchain_broadcasted_orders", "presignature_events",
    "trades", "settlements", "interactions", "order_events",
    "solver_competitions", "app_data", "jit_orders", "last_indexed_blocks",
    "competition_auctions", "proposed_solutions", "proposed_trade_executions",
    "proposed_jit_orders",
];

pub const LARGE_TABLES: &[&str] = &[
    "order_execution", "fee_policies", "auction_prices",
    "settlement_executions", "settlement_observations", "cow_amms",
    "reference_scores", "auction_participants", "scoring_auctions",
];
```

Toplam: 28 tablo (19 normal + 9 büyük tablo). `LARGE_TABLES` ayrı tutulmuş — farklı silme/temizlik stratejisi için.

Type alias'lar:
```rust
pub type Address    = ByteArray<20>;
pub type AppId      = ByteArray<32>;
pub type TransactionHash = ByteArray<32>;
pub type OrderUid   = ByteArray<56>;
pub type PgTransaction<'a> = sqlx::Transaction<'a, sqlx::Postgres>;
```

`OrderUid` 56 bayt: bytes[0..32] = order hash, bytes[32..52] = owner address, bytes[52..56] = valid_to (u32, big-endian).

### SQL Queries

```sql
-- clear_DANGER_: Tüm tabloları siler (test ortamı için)
TRUNCATE TABLE
    order_quotes, orders, invalidations, ethflow_orders, ethflow_refunds,
    onchain_broadcasted_orders, presignature_events, trades, settlements,
    interactions, order_events, solver_competitions, app_data, jit_orders,
    last_indexed_blocks, competition_auctions, proposed_solutions,
    proposed_trade_executions, proposed_jit_orders,
    order_execution, fee_policies, auction_prices, settlement_executions,
    settlement_observations, cow_amms, reference_scores,
    auction_participants, scoring_auctions
```

`TRUNCATE ... CASCADE` yerine tek tablolar listeleniyor — FK cycle'larını döngüsel cascade ile kesmek yerine aynı anda hepsini truncate ediyor.

### Güvenlik

`clear_DANGER_` prefix'i kasıtlı — üretim ortamında çağrılmasını engellemek için semantik bir sinyaldir. Ancak Rust tip sistemi bunu engellemez; sadece isimlendirme konvansiyonu.

### DEX Uygulama Notu — Nova Shield için

Nova Shield DEX veritabanında benzer tablo gruplandırması yapılabilir:
- `HOT_TABLES`: sık yazılan tablolar (orders, trades, order_events) — hızlı SSD'ye
- `COLD_TABLES`: arşiv niteliğindeki tablolar (auction_prices, reference_scores) — standart storage
- `PgTransaction` alias pattern'ı: transaction gerektiren fonksiyonlara `&mut PgTransaction` geçilmesi; tek-query fonksiyonlara `&mut PgConnection` geçilmesi. Bu ayrım Nova Shield'da da birebir uygulanmalı.

---

## 1.2 — `crates/database/src/orders.rs` (~900 satır)

### Amaç

`orders` tablosunun tüm CRUD işlemleri: order ekleme, solvable order sorgulama, tam sipariş detayları (`FullOrder`, `FullOrderWithQuote`), incremental sync, pagination. CoW Protocol'ün kalbi.

### Tablo/Struct Tanımı

```rust
pub struct Order {
    pub uid: OrderUid,           // ByteArray<56>
    pub owner: Address,
    pub creation_timestamp: DateTime<Utc>,
    pub sell_token: Address,
    pub buy_token: Address,
    pub sell_amount: BigDecimal,
    pub buy_amount: BigDecimal,
    pub valid_to: i64,
    pub fee_amount: BigDecimal,
    pub kind: DbOrderKind,       // 'sell' | 'buy'
    pub partially_fillable: bool,
    pub signature: Vec<u8>,
    pub receiver: Option<Address>,
    pub signing_scheme: DbSigningScheme,
    pub settlement_contract: Address,
    pub sell_token_balance: DbSellTokenSource,
    pub buy_token_balance: DbBuyTokenDestination,
    pub full_fee_amount: BigDecimal,
    pub class: DbOrderClass,     // 'market' | 'limit' | 'liquidity'
    pub app_data: AppId,
    pub cancellation_timestamp: Option<DateTime<Utc>>,
}
```

`FullOrder` bu 20 alana ek olarak 9 adet agregasyon sütunu içerir:
- `sum_fee`, `sum_sell`, `sum_buy` — trades tablosundaki toplam yürütülen miktarlar
- `invalidated` — invalidations tablosunda kayıt var mı?
- `presignature_pending` — presignature_events'te henüz imzalanmamış kayıt var mı?
- `pre_interactions`, `post_interactions` — interactions tablosu
- `ethflow_data` — ethflow_orders tablosu
- `onchain_user` — onchain_broadcasted_orders tablosu

`FullOrderWithQuote`: `FullOrder` + opsiyonel quote sütunları (sell/buy amount, fee, solver vb.)

### SQL Queries

**INSERT_ORDER_QUERY** — EthFlow için `true_valid_to` trick:
```sql
INSERT INTO orders (uid, owner, creation_timestamp, sell_token, buy_token,
    sell_amount, buy_amount, valid_to, fee_amount, kind, partially_fillable,
    signature, receiver, signing_scheme, settlement_contract, sell_token_balance,
    buy_token_balance, full_fee_amount, class, app_data,
    true_valid_to)
VALUES ($1, $2, $3, ...,
    CASE
        WHEN $7 = 4294967295 THEN
            (SELECT valid_to FROM ethflow_orders WHERE uid = $1)
        ELSE $7
    END
)
```

`valid_to = 4294967295` (u32::MAX) EthFlow order'larını işaret eder. EthFlow sözleşmesi sipariş UID'ini order hash + owner + u32::MAX olarak kodlar; gerçek geçerlilik süresi `ethflow_orders.valid_to`'da saklanır.

**solvable_orders()** — Batch auction'a katılabilecek order'ları bulur:
```sql
WITH live_orders AS MATERIALIZED (
    SELECT * FROM orders
    WHERE
        valid_to >= $1
        AND cancellation_timestamp IS NULL
        AND class != 'liquidity'
    ORDER BY creation_timestamp
),
trades_agg AS MATERIALIZED (
    SELECT order_uid,
           SUM(sell_amount) as sum_sell,
           SUM(buy_amount)  as sum_buy,
           SUM(fee_amount)  as sum_fee
    FROM trades
    WHERE order_uid IN (SELECT uid FROM live_orders)
    GROUP BY order_uid
)
SELECT lo.*, ta.*
FROM live_orders lo
LEFT JOIN trades_agg ta ON lo.uid = ta.order_uid
WHERE
    (lo.kind = 'sell' AND COALESCE(ta.sum_sell, 0) < lo.sell_amount)
    OR
    (lo.kind = 'buy'  AND COALESCE(ta.sum_buy,  0) < lo.buy_amount)
    AND NOT EXISTS (SELECT 1 FROM invalidations WHERE order_uid = lo.uid)
```

`MATERIALIZED` keyword'ü PostgreSQL query planner'ın CTE'yi materialize etmesini zorlar — planner'ın CTE'yi inline edip suboptimal plan üretmesini engeller.

**open_orders_by_time_or_uids()** — Incremental sync için:
```sql
SELECT <full_order_columns>
FROM orders o
LEFT JOIN LATERAL (
    SELECT SUM(t.sell_amount) AS sum_sell,
           SUM(t.buy_amount)  AS sum_buy,
           SUM(t.fee_amount)  AS sum_fee
    FROM trades t WHERE t.order_uid = o.uid
) trades_agg ON TRUE
WHERE
    o.creation_timestamp > $1
    OR o.uid = ANY($2)
```

`LATERAL` join, her order için bağımsız bir subquery çalıştırır — `trades_agg` CTE'ye kıyasla daha az bellek kullanır çünkü satır satır işler.

**full_orders_in_tx()** — Belirli bir transaction'daki order'ları bulur:
```sql
WITH SETTLEMENT_LOG_INDICES AS (
    SELECT
        s.log_index AS current_log_index,
        LAG(s.log_index) OVER (ORDER BY s.log_index) AS previous_log_index
    FROM settlements s
    WHERE s.tx_hash = $1
)
SELECT <full_order_columns>
FROM orders o
JOIN trades t ON t.order_uid = o.uid
WHERE
    t.block_number = $2
    AND t.log_index > COALESCE((SELECT previous_log_index FROM SETTLEMENT_LOG_INDICES), -1)
    AND t.log_index < (SELECT current_log_index FROM SETTLEMENT_LOG_INDICES)
```

`LAG()` window fonksiyonu: mevcut settlement'ın önceki settlement'ını bulur. Böylece aynı blok'taki birden fazla settlement'ı birbirinden ayırt eder.

### Güvenlik

- `is_duplicate_record_error()`: sqlx error code `"23505"` (unique violation) kontrol eder — insert idempotency.
- `true_valid_to` CASE: EthFlow order'ın UID'i yeniden kullanılabilmesi için unique constraint'te u32::MAX kullanılır, ama solvability kontrolünde gerçek geçerlilik tarihi `true_valid_to`'dan okunur.

### DEX Uygulama Notu — Nova Shield için

1. **`MATERIALIZED` CTE pattern**: Nova Shield'ın order matching engine'i benzer büyük tablo taramalarında bu pattern'ı kullanmalı. Özellikle "çözülebilir order" sorgusu her ~12s'de çalışacağından query planner'ın inlined CTE'yi yanlış optimize etmesini engellemek kritik.

2. **Partial fill tracking**: `sum_sell` ve `sum_buy` birikimleri ile kalan miktarı `sell_amount - sum_sell` olarak hesaplama. Nova Shield'ın AMM'lerinde de partial fill tracking için aynı pattern.

3. **solvable_orders filtresi**: Hem sell order hem buy order için ayrı koşul — `kind='sell' AND sum_sell < sell_amount` | `kind='buy' AND sum_buy < buy_amount`. Nova Shield DEX'te order side'ına göre completion kontrolü birebir uygulanmalı.

4. **LAG() window function**: Aynı blokta birden fazla settlement ayırt etme mekanizması. Nova Shield batch settlement sisteminde aynı yaklaşım uygulanabilir.

---

## 1.3 — `crates/database/src/trades.rs` (~855 satır)

### Amaç

EVM event log'larından decode edilen `Trade` event'larını saklar ve sorgular. Her trade: bir order'ın partial veya full yürütülmesidir.

### Tablo/Struct Tanımı

```rust
pub struct TradesQueryRow {
    pub block_number: i64,
    pub log_index: i64,
    pub order_uid: OrderUid,
    pub buy_amount: BigDecimal,
    pub sell_amount: BigDecimal,
    pub sell_amount_before_fees: BigDecimal, // = sell_amount - fee_amount
    pub owner: Address,
    pub buy_token: Address,
    pub sell_token: Address,
    pub tx_hash: Option<TransactionHash>,
    pub tx_from: Option<Address>,
    pub tx_nonce: Option<i64>,
}
```

`sell_amount_before_fees` query düzeyinde hesaplanır: `t.sell_amount - t.fee_amount`.

### SQL Queries

**Ana `trades()` query** — 3-way UNION:
```sql
-- Segment 1: Normal orders
SELECT t.block_number, t.log_index, t.order_uid,
       t.buy_amount, t.sell_amount,
       t.sell_amount - t.fee_amount AS sell_amount_before_fees,
       o.owner, o.buy_token, o.sell_token,
       settlement.tx_hash, settlement.tx_from, settlement.tx_nonce
FROM trades t
JOIN orders o ON o.uid = t.order_uid
JOIN LATERAL (
    SELECT s.tx_hash, s.tx_from, s.tx_nonce
    FROM settlements s
    WHERE s.block_number = t.block_number
      AND s.log_index > t.log_index
    ORDER BY s.log_index ASC
    LIMIT 1
) settlement ON TRUE
WHERE ($1 IS NULL OR t.block_number = $1)
  AND ($2 IS NULL OR t.order_uid = $2)
  AND ($3 IS NULL OR o.owner = $3)

UNION ALL

-- Segment 2: Onchain placed orders (EIP-1271, PreSign vb.)
SELECT t.block_number, t.log_index, t.order_uid,
       t.buy_amount, t.sell_amount,
       t.sell_amount - t.fee_amount AS sell_amount_before_fees,
       ob.sender AS owner, o.buy_token, o.sell_token,
       settlement.tx_hash, settlement.tx_from, settlement.tx_nonce
FROM trades t
JOIN onchain_broadcasted_orders ob ON ob.uid = t.order_uid
JOIN orders o ON o.uid = t.order_uid
JOIN LATERAL (...same settlement join...) settlement ON TRUE

UNION ALL

-- Segment 3: JIT orders (materialized CTE ile)
WITH jit AS MATERIALIZED (
    SELECT uid, owner, sell_token, buy_token FROM jit_orders
)
SELECT t.block_number, t.log_index, t.order_uid,
       t.buy_amount, t.sell_amount,
       t.sell_amount - t.fee_amount AS sell_amount_before_fees,
       j.owner, j.buy_token, j.sell_token,
       settlement.tx_hash, settlement.tx_from, settlement.tx_nonce
FROM trades t
JOIN jit j ON j.uid = t.order_uid
JOIN LATERAL (...) settlement ON TRUE

ORDER BY block_number DESC, log_index DESC
LIMIT $4 OFFSET $5
```

**`SETTLEMENT_JOIN` LATERAL pattern**: Her trade için `log_index > t.log_index` koşuluyla aynı blokta en yakın sonraki settlement bulunur. Böylece bir blokta birden fazla settlement olsa dahi her trade'e doğru settlement eşlenir.

**`get_trades_for_settlement()`**:
```sql
WITH prev_settlement AS (
    SELECT log_index AS prev_log_index
    FROM settlements
    WHERE block_number = $1
      AND log_index < (SELECT log_index FROM settlements WHERE tx_hash = $2)
    ORDER BY log_index DESC
    LIMIT 1
)
SELECT t.order_uid, t.sell_amount, t.buy_amount, t.fee_amount
FROM trades t
WHERE t.block_number = $1
  AND t.log_index > COALESCE((SELECT prev_log_index FROM prev_settlement), -1)
  AND t.log_index < (SELECT log_index FROM settlements WHERE tx_hash = $2)
```

**`token_first_trade_block()`**:
```sql
SELECT MIN(block_number) FROM (
    SELECT MIN(t.block_number) AS block_number
    FROM trades t
    JOIN orders o ON o.uid = t.order_uid
    WHERE o.sell_token = $1 OR o.buy_token = $1

    UNION ALL

    SELECT MIN(t.block_number) AS block_number
    FROM trades t
    JOIN jit_orders j ON j.uid = t.order_uid
    WHERE j.sell_token = $1 OR j.buy_token = $1
) combined
```

### Güvenlik

- LATERAL JOIN: Her trade için O(log n) index scan (log_index üzerinde). Subquery ile O(n) tarama olurdu.
- `UNION ALL` (not `UNION`): Deduplication maliyetinden kaçınılır; trade'ler doğası gereği distinct olduğundan doğru bir seçim.

### DEX Uygulama Notu — Nova Shield için

1. **LATERAL settlement join**: Nova Shield'da da settlement log index ordering ile trade-settlement eşlemesi yapılmalı. Aynı blokta birden fazla batch settlement olduğunda kritik.

2. **3-way UNION pattern**: Nova Shield farklı order türleri destekliyorsa (regular, onchain, JIT benzeri) aynı pattern uygulanabilir. Her segment kendi join path'ini kullanır.

3. **`sell_amount_before_fees`**: Query düzeyinde hesaplamak, uygulama katmanına BigDecimal aritmetik yükü bindirmez.

---

## 1.4 — `crates/database/src/solver_competition.rs` (~270 satır)

### Amaç

Auction başına solver rekabetini (v1 format: JSON blob) kaydeder ve sorgular. Her auction'da birden fazla solver çözüm sunar; autopilot en iyi çözümü seçer; tüm bu süreç JSON olarak saklanır.

### Tablo/Struct Tanımı

```rust
pub struct LoadCompetition {
    pub json: serde_json::Value,
    pub id: AuctionId,
    pub tx_hashes: Vec<TransactionHash>,
}
```

`tx_hashes`: `ARRAY_AGG(s.tx_hash) FILTER (WHERE s.solution_uid IS NOT NULL)` ile toplanır.

### SQL Queries

**`save()`**:
```sql
INSERT INTO solver_competitions (id, json) VALUES ($1, $2)
ON CONFLICT (id) DO UPDATE SET json = $2
```

**`load_by_id()`**:
```sql
SELECT sc.json, sc.id,
       ARRAY_AGG(s.tx_hash) FILTER (WHERE s.solution_uid IS NOT NULL) AS tx_hashes
FROM solver_competitions sc
LEFT OUTER JOIN competition_auctions ca ON ca.id = sc.id
LEFT JOIN settlements s ON s.auction_id = ca.id
WHERE sc.id = $1
GROUP BY sc.json, sc.id
```

**`load_latest_competitions()`**:
```sql
... WHERE ca.deadline <= $2
ORDER BY sc.id DESC
LIMIT $1
```

### Güvenlik

JSON blob yaklaşımının güvenlik riski: autopilot'un ürettiği büyük JSON'lar (yüzlerce solver, binlerce order) DB'de birikerek storage şişirebilir. v2 (structured tables) bu sorunu çözer.

### DEX Uygulama Notu — Nova Shield için

Nova Shield solver rekabeti için JSON blob yerine direkt v2 pattern'ını (structured tables) kullanmalı. JSON blob'lar debug için kullanışlıdır ama sorgulanamaz. v2 pattern'ı: `proposed_solutions`, `proposed_trade_executions` — Nova Shield'da `auction_solutions`, `solution_trade_executions` olarak implement edilebilir.

---

## 1.5 — `crates/database/src/solver_competition_v2.rs` (~1024 satır)

### Amaç

Auction rekabetini structured tablolarda saklar: çözümler, trade yürütmeleri ve JIT order'lar. v1'in yerini alacak; daha iyi sorgulanabilirlik sağlar.

### Tablo/Struct Tanımı

```rust
pub struct Solution {
    pub uid: i64,           // 0-based, küçük uid = daha iyi çözüm
    pub id: i64,            // solver'ın kendi ID'si
    pub solver: Address,
    pub is_winner: bool,
    pub filtered_out: bool,
    pub score: Option<BigDecimal>,
    pub orders: Vec<OrderUid>,
    pub price_tokens: Vec<Address>,
    pub price_values: Vec<BigDecimal>,
}

pub struct TradeExecution {
    pub order_uid: OrderUid,
    pub sell_amount: BigDecimal,
    pub buy_amount: BigDecimal,
    pub fee: BigDecimal,
}
```

### SQL Queries

**`save()`** — 3 aşamalı transaction:
```sql
-- 1. Çözümleri kaydet
INSERT INTO proposed_solutions (auction_id, uid, id, solver, is_winner, filtered_out, score)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT DO NOTHING

-- 2. Trade yürütmelerini kaydet
INSERT INTO proposed_trade_executions (auction_id, solution_uid, order_uid, sell_amount, buy_amount, fee)
VALUES ...
ON CONFLICT DO NOTHING

-- 3. JIT order'ları kaydet (sadece orders tablosunda yoksa)
INSERT INTO proposed_jit_orders (auction_id, solution_uid, ...)
SELECT $1, $2, ...
WHERE NOT EXISTS (SELECT 1 FROM orders WHERE uid = $3)
ON CONFLICT DO NOTHING
```

**`fetch_in_flight_orders()`**:
```sql
SELECT pte.order_uid
FROM proposed_trade_executions pte
JOIN proposed_solutions ps ON ps.uid = pte.solution_uid
    AND ps.auction_id = pte.auction_id
JOIN competition_auctions ca ON ca.id = ps.auction_id
WHERE
    ca.deadline > $1
    AND ps.is_winner = true
    AND NOT EXISTS (
        SELECT 1 FROM settlements WHERE auction_id = ps.auction_id
    )
    AND NOT EXISTS (
        SELECT 1 FROM settlement_executions se
        WHERE se.auction_id = ps.auction_id
          AND se.outcome IS NOT NULL
          AND se.outcome != 'success'
    )
```

**`BASE_SOLUTIONS_QUERY`**:
```sql
SELECT
    ps.uid, ps.id, ps.solver, ps.is_winner, ps.filtered_out, ps.score,
    ARRAY_AGG(DISTINCT pte.order_uid) AS orders,
    pte.order_uid,
    COALESCE(pjo.sell_amount, o.sell_amount) AS sell_amount,
    COALESCE(pjo.buy_amount, o.buy_amount) AS buy_amount,
    COALESCE(pjo.kind, o.kind) AS kind
FROM proposed_solutions ps
JOIN proposed_trade_executions pte ON ...
LEFT JOIN proposed_jit_orders pjo ON pjo.order_uid = pte.order_uid
LEFT JOIN orders o ON o.uid = pte.order_uid
GROUP BY ps.uid, ps.auction_id, pte.order_uid
ORDER BY ps.uid ASC
```

Ranking: uid'ler 0-based atanır. uid=0 en iyi çözüm.

### Güvenlik

- `ON CONFLICT DO NOTHING`: Event sourcing idempotency.
- JIT order insert guard (`NOT EXISTS orders`): JIT order, bir kullanıcı order'ı olarak daha önce oluşturulmuşsa çift kayıt engellenir.

### DEX Uygulama Notu — Nova Shield için

1. **uid-based ranking**: Solver çözümlerini uid'e göre sıralamak (küçük = daha iyi) basit ve etkili. Nova Shield'da da solver competition sonuçları bu şekilde saklanabilir.

2. **fetch_in_flight_orders**: Nova Shield'ın "order zaten settle ediliyor mu?" kontrolü için aynen uygulanabilir. Double-execution ve redundant submission'ı önler.

3. **COALESCE(jit_fields, regular_order_fields)**: JIT ve regular order'ların aynı query'de normalize edilmesi. Nova Shield'ın farklı order tipleri için aynı pattern.

---

## 1.6 — `crates/database/src/auction.rs` (~174 satır)

### Amaç

Tek aktif auction'ı (`auctions` tablosu) ve auction meta verilerini (`competition_auctions` tablosu) yönetir.

### Tablo/Struct Tanımı

```rust
pub type AuctionId = i64;

pub struct Auction {
    pub id: AuctionId,
    pub block: i64,
    pub deadline: i64,
    pub order_uids: Vec<OrderUid>,
    pub price_tokens: Vec<Address>,
    pub price_values: Vec<BigDecimal>,
    pub surplus_capturing_jit_order_owners: Vec<Address>,
}
```

### SQL Queries

**`insert_auction_with_id()`** — Tek satır invariantı:
```sql
WITH deleted AS (
    DELETE FROM auctions
)
INSERT INTO auctions (id, block, deadline, order_uids, price_tokens, price_values,
                      surplus_capturing_jit_order_owners)
VALUES ($1, $2, $3, $4, $5, $6, $7)
```

CTE `deleted` önce TÜM auctions satırlarını siler, sonra yeni satırı ekler. Tablo her zaman 0 veya 1 satır içerir.

**`fetch_auction_ids_by_order_uid()`**:
```sql
SELECT id FROM competition_auctions
WHERE order_uids @> ARRAY[$1::bytea]
```

`@>`: PostgreSQL array containment operatörü. GIN index ile O(log n) verimli arama.

### Güvenlik

DELETE + INSERT pattern, transaction içinde çalışmalı (`&mut PgTransaction`) — aksi halde DELETE sonrası crash = boş auctions tablosu.

### DEX Uygulama Notu — Nova Shield için

Nova Shield'ın auction state yönetimi için "DELETE + INSERT in transaction" pattern'ı — her yeni auction başladığında öncekini atomik olarak silip yenisini ekler. GIN index'li array containment (`@>`) order lookup için kullanışlı.

---

## 1.7 — `crates/database/src/fee_policies.rs` (~204 satır)

### Amaç

Her order için uygulanan fee politikalarını saklar. Üç tip: Surplus (kazanç bazlı), Volume (hacim bazlı), PriceImprovement (fiyat iyileştirme bazlı).

### Tablo/Struct Tanımı

```rust
pub enum FeePolicyKind {
    Surplus,
    Volume,
    PriceImprovement,
}

pub struct FeePolicy {
    pub auction_id: AuctionId,
    pub order_uid: OrderUid,
    pub kind: FeePolicyKind,
    pub surplus_factor: Option<f64>,
    pub surplus_max_volume_factor: Option<f64>,
    pub volume_factor: Option<f64>,
    pub price_improvement_factor: Option<f64>,
    pub price_improvement_max_volume_factor: Option<f64>,
    pub application_order: i32,
}
```

### SQL Queries

**`fetch_all()`** — Batch key lookup:
```sql
SELECT fp.*
FROM fee_policies fp
JOIN (VALUES ($1::bigint, $2::bytea), ($3::bigint, $4::bytea), ...) AS keys(auction_id, order_uid)
  ON fp.auction_id = keys.auction_id AND fp.order_uid = keys.order_uid
ORDER BY fp.application_order ASC
```

`VALUES` clause ile batch key lookup: N ayrı SELECT sorgusu yerine tek JOIN. `application_order` ile fee politikaları sıralı uygulanır.

### Güvenlik

`f64` kullanımı: fee factor'lar f64 olarak saklanır. Finansal hesaplamalarda floating-point precision kaybı riski var. Ancak bu değerler oran (0.0-1.0 arası) olarak kullanıldığından ve sonuçlar BigDecimal ile çarpılarak U256'ya dönüştürüldüğünden kabul edilebilir; yine de risk mevcuttur.

### DEX Uygulama Notu — Nova Shield için

1. **Üç fee politikası tipi**: Nova Shield DEX'te de çok katmanlı fee yapısı:
   - Surplus fee: kullanıcı beklediğinden daha iyi fiyat aldığında protokol kesintisi
   - Volume fee: işlem hacmine orantılı
   - Price improvement: referans fiyata kıyasla iyileştirme oranında

2. **`application_order`**: Fee politikaları sıralı uygulandığından Nova Shield'da da bu sıralama mekanizması kritik.

3. **`VALUES` JOIN pattern**: Batch key lookup için N+1 query sorununu çözer. Nova Shield'ın tüm batch veri çekme işlemlerinde kullanılmalı.

---

## 1.8 — `crates/database/src/quotes.rs` (~678 satır)

### Amaç

Order oluşturulmadan önce hesaplanan fiyat teklifleri (quote) saklanır. Quote, kullanıcıya gösterilir; order bu quote'a dayanarak oluşturulur.

### Tablo/Struct Tanımı

```rust
pub struct Quote {
    pub id: i64,
    pub sell_token: Address,
    pub buy_token: Address,
    pub sell_amount: BigDecimal,
    pub sell_amount_1: BigDecimal,    // sell_amount + fee_amount
    pub buy_amount: BigDecimal,
    pub gas_amount: f64,
    pub gas_price: f64,
    pub sell_token_price: f64,
    pub order_kind: DbOrderKind,
    pub expiration_timestamp: DateTime<Utc>,
    pub quote_kind: DbQuoteKind,
    pub solver: Address,
    pub verified: bool,
    pub metadata: serde_json::Value,
}

pub enum DbQuoteKind {
    Standard,
    Eip1271OnchainOrder,
    PreSignOnchainOrder,
}
```

`sell_amount_1 = sell_amount + fee_amount` — Hem "fee dahil" hem "fee hariç" sell amount ile arama yapılabilmesi için.

### SQL Queries

**`find()`** — En iyi quote'u bul:
```sql
SELECT *
FROM order_quotes
WHERE
    sell_token = $1
    AND buy_token = $2
    AND (sell_amount = $3 OR sell_amount = $4)
    AND kind = $5
    AND expiration_timestamp >= $6
    AND quote_kind = $7
ORDER BY
    verified DESC,
    buy_amount / (sell_amount + gas_amount * gas_price / sell_token_price) DESC
LIMIT 1
```

**Sıralama formülü analizi**:

```
efektif_fiyat = buy_amount / (sell_amount + gas_amount * gas_price / sell_token_price)
```

`gas_amount * gas_price / sell_token_price` = gas maliyetinin sell token cinsinden karşılığı.

Bu formülle "gas maliyeti dahil en iyi exchange rate" seçilir. `verified DESC` ise doğrulanmış (simüle edilmiş) quote'ları unverified'a tercih eder.

**`remove_expired_quotes()`**:
```sql
DELETE FROM order_quotes WHERE expiration_timestamp < $1
```

### Güvenlik

- `verified` flag: Simülasyonla doğrulanmış quote'lar, onaylanmamışlara tercih edilir. Unrealistic quote'ların order oluşturmasını engeller.
- Çift sell_amount araması: Hem `sell_amount` hem `sell_amount + fee` ile arama yapmak, fee hesabı değiştiğinde de doğru quote bulunmasını sağlar.

### DEX Uygulama Notu — Nova Shield için

1. **Quote verification priority**: Nova Shield'ın quote sistemi, simüle edilmiş quote'ları önceliklendirmeli. `verified DESC` sıralaması bunu sağlar.

2. **Gas-adjusted rate formülü**: `buy_amount / (sell_amount + gas_cost_in_sell_token)` — DEX aggregator karşılaştırması için de kullanılabilir. Nova Shield'ın routing engine'i farklı path'leri bu metrik ile kıyaslamalı.

3. **Dual sell_amount storage**: `sell_amount` ve `sell_amount_1 = sell_amount + fee` her ikisi de saklanmalı.

---

## 1.9 — `crates/database/src/order_execution.rs` (~194 satır)

### Amaç

Settlement sonrası order yürütme sonuçlarını kaydeder: kazanılan reward (deprecated), ödenen fee, uygulanan protocol fee'ler.

### Tablo/Struct Tanımı

```rust
pub struct OrderExecution {
    pub order_uid: OrderUid,
    pub auction_id: AuctionId,
    pub reward: f64,                    // DEPRECATED
    pub executed_fee: BigDecimal,
    pub executed_fee_token: Address,
    pub block_number: i64,
    pub protocol_fee_tokens: Vec<Address>,
    pub protocol_fee_amounts: Vec<BigDecimal>,
}
```

`protocol_fee_tokens` ve `protocol_fee_amounts` paralel array'ler: token[i]'de amount[i] kadar protocol fee ödendi.

### SQL Queries

**`save()`**:
```sql
INSERT INTO order_execution (order_uid, auction_id, reward, executed_fee,
    executed_fee_token, block_number, protocol_fee_tokens, protocol_fee_amounts)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (order_uid, auction_id) DO UPDATE SET
    reward = EXCLUDED.reward,
    executed_fee = EXCLUDED.executed_fee,
    executed_fee_token = EXCLUDED.executed_fee_token,
    block_number = EXCLUDED.block_number,
    protocol_fee_tokens = EXCLUDED.protocol_fee_tokens,
    protocol_fee_amounts = EXCLUDED.protocol_fee_amounts
```

`ON CONFLICT DO UPDATE`: Re-indexing senaryosu — blockchain reorganization veya indexer yeniden çalıştırıldığında mevcut kayıt güncellenir.

**`executed_protocol_fees()`** — Batch fetch:
```sql
SELECT oe.order_uid, oe.auction_id,
       oe.protocol_fee_tokens, oe.protocol_fee_amounts
FROM order_execution oe
JOIN (VALUES ...) AS keys(auction_id, order_uid)
  ON oe.auction_id = keys.auction_id AND oe.order_uid = keys.order_uid
```

### Güvenlik

`reward: f64` deprecated olarak işaretlenmiş ama hâlâ şemada ve kod'da mevcut. Migration tamamlanmamış.

### DEX Uygulama Notu — Nova Shield için

Protocol fee tracking, on-chain settlement sonrası off-chain kayıt tutma için kritik. Nova Shield'da paralel array pattern yerine `JSONB` veya ayrı `protocol_fees` tablosu (normalized form) düşünülebilir — daha kolay sorgulama sağlar.

---

## 1.10 — `crates/database/src/events.rs` (~172 satır)

### Amaç

Blockchain event log'larından okunan 4 event türünü kaydeder: `Trade`, `Invalidation`, `Settlement`, `PreSignature`.

### Tablo/Struct Tanımı

```rust
pub enum Event {
    Trade(Trade),
    Invalidation(Invalidation),
    Settlement(Settlement),
    PreSignature(PreSignature),
}

pub struct Trade {
    pub block_number: i64,
    pub log_index: i64,
    pub order_uid: OrderUid,
    pub sell_amount: BigDecimal,
    pub buy_amount: BigDecimal,
    pub fee_amount: BigDecimal,
}

pub struct Settlement {
    pub block_number: i64,
    pub log_index: i64,
    pub solver: Address,
    pub transaction_hash: TransactionHash,
    pub transaction_index: i64,
    pub auction_id: Option<AuctionId>,
    pub solution_uid: Option<i64>,
}
```

### SQL Queries

Tüm insert'ler `ON CONFLICT DO NOTHING`:
```sql
INSERT INTO trades (block_number, log_index, order_uid, sell_amount, buy_amount, fee_amount)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT DO NOTHING

INSERT INTO settlements (block_number, log_index, solver, tx_hash, tx_index, auction_id, solution_uid)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT DO NOTHING
```

**`delete()`** — Reorg recovery:
```sql
DELETE FROM invalidations WHERE block_number >= $1;
DELETE FROM trades WHERE block_number >= $1;
DELETE FROM settlements WHERE block_number >= $1;
DELETE FROM presignature_events WHERE block_number >= $1;
```

Sıralı silme — FK constraints sırasıyla tatmin edilir.

### Güvenlik

`ON CONFLICT DO NOTHING`: Multi-pod HPA ortamında birden fazla indexer aynı event'ı işleyebilir. İkincisi sessizce geçer.

Reorg recovery: `delete(block_number)` + re-index = güvenli chain reorganization yönetimi.

### DEX Uygulama Notu — Nova Shield için

Nova Shield'ın blockchain indexer'ı için:
1. Her event insert `ON CONFLICT DO NOTHING` — çoklu indexer pod güvenliği
2. Reorg recovery: `DELETE WHERE block_number >= reorg_block` → re-index
3. FK sırasına göre silme: invalidations → trades → settlements → presignatures

---

## 1.11 — `crates/database/src/order_events.rs` (~349 satır)

### Amaç

Her order'ın lifecycle'ını takip eder: oluşturuldu, filtrelendi, işleniyor, tamamlandı vb. Audit trail ve monitoring için.

### Tablo/Struct Tanımı

```rust
pub enum OrderEventLabel {
    Created,
    Ready,
    Filtered,
    Invalid,
    Executing,
    Considered,
    Traded,
    Cancelled,
}

pub enum OrderFilterReason {
    InFlight,
    BannedUser,
    InvalidSignature,
    UnsupportedToken,
    InsufficientBalance,
    DustOrder,
    MissingNativePrice,
}
```

### SQL Queries

**`insert_order_events()`** — Deduplication CTE:
```sql
WITH latest_events AS (
    SELECT DISTINCT ON (order_uid) order_uid, label, reason
    FROM order_events
    WHERE order_uid = ANY($1)
    ORDER BY order_uid, timestamp DESC
),
incoming AS (
    SELECT t.order_uid,
           $2 AS timestamp,
           $3 AS label,
           $4 AS reason
    FROM unnest($1) AS t(order_uid)
)
INSERT INTO order_events (order_uid, timestamp, label, reason)
SELECT DISTINCT i.order_uid, i.timestamp, i.label, i.reason
FROM incoming i
LEFT JOIN latest_events le ON le.order_uid = i.order_uid
WHERE le.label IS DISTINCT FROM i.label
   OR le.reason IS DISTINCT FROM i.reason
```

`IS DISTINCT FROM`: NULL-safe karşılaştırma.

`DISTINCT ON (order_uid)` + `ORDER BY timestamp DESC`: Her order için en son event'ı tek sorguda bulur.

Mantık: Aynı label + reason kombinasyonu ardışık geliyorsa insert edilmez. Yalnızca durum değişikliği kaydedilir.

### Güvenlik

Deduplication: Autopilot her cycle'da aynı event'ı yayınlayabilir. Bu CTE sayesinde DB'de tekrar etmez.

### DEX Uygulama Notu — Nova Shield için

1. **Order lifecycle tracking**: Nova Shield'da da order state machine'ı DB'de audit trail olarak saklanmalı.

2. **`DISTINCT ON` + `ORDER BY timestamp DESC`**: En son event'ı bulmak için verimli PostgreSQL-özgü syntax.

3. **`IS DISTINCT FROM`**: NULL-safe karşılaştırma — özellikle `reason` alanı nullable olduğundan standart `!=` yerine bu kullanılmalı.

---

## 1.12 — `crates/database/src/ethflow_orders.rs` (~579 satır)

### Amaç

EthFlow protokolü: kullanıcılar native ETH gönderir; sözleşme WETH'e wrap eder ve order oluşturur. Bu order'ların özel yönetimi ve refund mantığı burada.

### Tablo/Struct Tanımı

```rust
pub struct EthflowOrder {
    pub uid: OrderUid,
    pub valid_to: i64,
    pub refund_tx_hash: Option<TransactionHash>,
}
```

### SQL Queries

**`insert_or_overwrite_ethflow_order()`**:
```sql
INSERT INTO ethflow_orders (uid, valid_to)
VALUES ($1, $2)
ON CONFLICT (uid) DO UPDATE SET valid_to = $2;

UPDATE orders SET true_valid_to = $2 WHERE uid = $1
```

**`refundable_orders()`** — En karmaşık query:
```sql
SELECT eo.uid, eo.valid_to, o.owner, o.sell_token, o.sell_amount, o.buy_token, o.buy_amount
FROM ethflow_orders eo
JOIN orders o ON o.uid = eo.uid
JOIN order_quotes oq ON oq.uid = eo.uid
WHERE
    eo.valid_to < $1
    AND (eo.valid_to - EXTRACT(EPOCH FROM o.creation_timestamp)) > $2
    AND (1.0 - o.buy_amount::float / oq.buy_amount::float) >= $3
    AND eo.refund_tx_hash IS NULL
    AND NOT EXISTS (SELECT 1 FROM trades t WHERE t.order_uid = eo.uid)
    AND NOT EXISTS (SELECT 1 FROM invalidations i WHERE i.order_uid = eo.uid)
```

**Slippage kontrolü formülü**:
```
slippage_ratio = 1.0 - (order.buy_amount / quote.buy_amount)
```

Eğer bu oran >= `$3` (örn. 0.01 = %1) ise order, quote'dan çok daha kötü fiyattaydı ve fill edilmedi → refund edilebilir.

### Güvenlik

- `float` cast: `o.buy_amount::float / oq.buy_amount::float` precision kaybı riski. BigDecimal daha güvenli olurdu.
- Min lifetime kontrolü: Çok yeni expired order'ları refund'dan korur — race condition önlemi.

### DEX Uygulama Notu — Nova Shield için

EthFlow benzeri "native token wrapping → order creation" akışı için:
1. `valid_to = u32::MAX` trick: UID uniqueness için.
2. Slippage-based refund: Beklenenden çok daha kötü fiyatta dolan ve süresi geçen order'ları otomatik refund.
3. `true_valid_to` ve `valid_to` ayrımı: Farklı amaçlarla kullanılan validity süreleri ayrı kolonlarda.

---

## 1.13 — `crates/database/src/onchain_broadcasted_orders.rs` (~279 satır)

### Amaç

On-chain yayınlanan order'ları (EIP-1271, PreSign vb.) ve hata durumlarını takip eder. Chain reorganization durumunda `is_reorged` flag'i ile recovery.

### Tablo/Struct Tanımı

```rust
pub enum OnchainOrderPlacementError {
    QuoteNotFound,         // DEPRECATED
    InvalidQuote,          // DEPRECATED
    InsufficientFee,       // DEPRECATED
    PreValidationError,
    DisabledSettlementContract,
    SenderNotAllowListed,
    CouldNotFetchOrder,
    InvalidOrderData,
    WrongOwner,
    ZeroFee,
    TooLateToPlaceOrder,
    DuplicatedOrder,
    InvalidEip1271Signature,
    InvalidPreSignature,
}

pub struct OnchainOrderPlacementRow {
    pub uid: OrderUid,
    pub sender: Address,
    pub placement_error: Option<OnchainOrderPlacementError>,
    pub is_reorged: bool,
    pub block_number: i64,
    pub log_index: i64,
}
```

### SQL Queries

**`mark_as_reorged()`**:
```sql
UPDATE onchain_broadcasted_orders
SET is_reorged = true
WHERE block_number >= $1
```

**`insert_onchain_order()`**:
```sql
INSERT INTO onchain_broadcasted_orders (uid, sender, placement_error, is_reorged, block_number, log_index)
VALUES ($1, $2, $3, false, $4, $5)
ON CONFLICT (uid, log_index) DO UPDATE SET is_reorged = false
```

`ON CONFLICT DO UPDATE SET is_reorged = false`: Reorg sonrası aynı order tekrar görüldüğünde, reorged=true olan kaydı reorged=false yapar. Re-confirmation pattern.

### Güvenlik

3 deprecated error variant DB'de hâlâ mevcut — migration tamamlanmamış. `shared/src/db_order_conversions.rs` bu variant'ları `None`'a map eder.

### DEX Uygulama Notu — Nova Shield için

`is_reorged` flag + `mark_as_reorged(block)` + `ON CONFLICT SET is_reorged = false` pattern'ı: chain reorg'larla başa çıkmak için production-ready çözüm. Nova Shield'ın on-chain order sistemi bu pattern'ı birebir kullanabilir.

---

## 1.14 — `crates/database/src/jit_orders.rs` (~253 satır)

### Amaç

Just-In-Time (JIT) orders: solver'ların çözüm süreci sırasında on-chain oluşturdukları likidite order'ları. `orders` tablosunda değil, kendi tablolarında saklanır.

### Tablo/Struct Tanımı

```rust
pub struct JitOrder {
    pub uid: OrderUid,
    pub owner: Address,
    pub creation_block: i64,
    pub sell_token: Address,
    pub buy_token: Address,
    pub sell_amount: BigDecimal,
    pub buy_amount: BigDecimal,
    pub valid_to: i64,
    pub app_data: AppId,
    pub fee_amount: BigDecimal,
    pub kind: DbOrderKind,
    pub partially_fillable: bool,
    pub signature: Vec<u8>,
    pub receiver: Address,
    pub signing_scheme: DbSigningScheme,
}
```

### SQL Queries

**Batch INSERT**:
```sql
INSERT INTO jit_orders (uid, owner, creation_block, sell_token, buy_token, ...)
VALUES (...), (...)
ON CONFLICT DO NOTHING
```

`QueryBuilder` ile dinamik VALUES clause.

**SELECT** — Hardcoded settlement contract:
```sql
SELECT uid, owner, ...,
       '\x9008d19f58aabd9ed0d60971565aa8510560ab41'::bytea AS settlement_contract,
       'liquidity'::OrderClass AS class
FROM jit_orders WHERE uid = ANY($1)
```

Settlement contract adresi hardcode edilmiş — multi-chain deployment için sorun yaratabilir.
`class` her zaman `'liquidity'`.

### Güvenlik

Hardcoded settlement contract: Eğer settlement contract upgrade edilirse tüm bu sorguların güncellenmesi gerekir. Konfigürasyondan okunması daha sağlıklı olurdu.

### DEX Uygulama Notu — Nova Shield için

JIT order kavramı Nova Shield DEX'te çok önemli: solver'lar çözüm üretirken anlık likidite ekleyebilir. Bu order'lar ayrı bir tabloda saklanmalı. Settlement contract adresi hardcode yerine konfigürasyondan okunmalı.

---

## 1.15 — `crates/database/src/order_history.rs` (~364 satır)

### Amaç

Bir kullanıcının tüm order'larını paginated şekilde getirir. Hem regular, hem onchain, hem JIT order'ları kapsar.

### SQL Queries

**Phase 1 — Sayfa UID'lerini bul**:
```sql
SELECT uid, creation_timestamp FROM (
    SELECT uid, creation_timestamp FROM orders WHERE owner = $1
    UNION
    SELECT o.uid, o.creation_timestamp FROM orders o
    JOIN onchain_broadcasted_orders ob ON ob.uid = o.uid
    WHERE ob.sender = $1
    UNION
    SELECT uid, creation_timestamp FROM jit_orders WHERE owner = $1
) combined
GROUP BY uid, creation_timestamp
ORDER BY creation_timestamp DESC
LIMIT $2 OFFSET $3
```

`UNION` (not `UNION ALL`): Aynı order birden fazla branch'te gelebilir. Dedup gerekli.

**Phase 2 — Tam detay getir**:
```sql
SELECT <full_order_columns> FROM orders WHERE uid = ANY($page_uids)
UNION ALL
SELECT <jit_order_columns> FROM jit_orders
WHERE uid = ANY($page_uids)
AND NOT EXISTS (SELECT 1 FROM orders WHERE uid = jit_orders.uid)
```

### Güvenlik

İki aşamalı pagination: Phase 1 lightweight (sadece uid + timestamp), Phase 2 full data yalnızca sayfa büyüklüğünde.

### DEX Uygulama Notu — Nova Shield için

İki fazlı pagination pattern'ı:
1. Faz 1: Sayfa UID'lerini bul (hafif, index scan)
2. Faz 2: Bu UID'ler için tam veri çek

Nova Shield'ın order history API'sı için bu pattern kesinlikle uygulanmalı.

---

## 1.16 — `crates/database/src/byte_array.rs` (~124 satır)

### Amaç

Sabit boyutlu byte array'leri PostgreSQL `bytea` tipine map eden const-generic wrapper.

### Tablo/Struct Tanımı

```rust
#[derive(Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct ByteArray<const N: usize>(pub [u8; N]);
```

**Decode** (PostgreSQL → Rust):
```rust
impl<const N: usize> Decode<'_, Postgres> for ByteArray<N> {
    fn decode(value: PgValueRef) -> Result<Self> {
        match value.format() {
            PgValueFormat::Binary => {
                let slice = value.as_bytes()?;
                Ok(ByteArray(slice.try_into()?))
            }
            PgValueFormat::Text => {
                let s = value.as_str()?;
                let hex = s.strip_prefix("\\x").unwrap_or(s);
                let bytes = hex::decode(hex)?;
                Ok(ByteArray(bytes.try_into()?))
            }
        }
    }
}
```

`PgHasArrayType`: `Vec<ByteArray<N>>` kullanımını mümkün kılar.

### Güvenlik

`try_into()` boyut kontrolü: DB'deki bytea boyutu N ile eşleşmezse `Err` döner — silent data corruption yerine açık hata.

### DEX Uygulama Notu — Nova Shield için

Nova Shield'ın address, hash, UID tipleri için `ByteArray<N>` pattern'ı:
- `Address = ByteArray<20>`
- `TxHash = ByteArray<32>`
- `OrderUid = ByteArray<56>`

Const generics ile compile-time boyut doğrulaması.

---

## 1.17 — `crates/database/src/app_data.rs` (~85 satır)

### Amaç

Order metadata (uygulama verileri) saklar. Her order bir `app_data` hash'ine (32 byte) sahiptir; tam içerik burada saklanır.

### SQL Queries

**`insert()`** — Conditional return pattern:
```sql
WITH inserted AS (
    INSERT INTO app_data (contract_app_data, full_app_data)
    VALUES ($1, $2)
    ON CONFLICT DO NOTHING
    RETURNING full_app_data
)
SELECT full_app_data FROM inserted
UNION ALL
SELECT full_app_data FROM app_data WHERE contract_app_data = $1
LIMIT 1
```

Bu pattern: INSERT başarılıysa `RETURNING` kısmı dolu → yeni kayıt `None` döner. Conflict varsa `inserted` boş, ikinci SELECT mevcut kaydı döndürür → `Some(existing_data)`.

### DEX Uygulama Notu — Nova Shield için

"INSERT OR RETURN EXISTING" pattern'ı Nova Shield'ın idempotent veri insert operasyonlarında kullanışlı. PostgreSQL CTE'nin `RETURNING` ile kombinasyonu elegant bir "upsert with result" sağlar.

---

## 1.18 — `crates/database/src/last_indexed_blocks.rs` (~57 satır)

### Amaç

Her indexed contract için son işlenen blok numarasını saklar. Indexer restart'ta kaldığı yerden devam eder.

### SQL Queries

```sql
INSERT INTO last_indexed_blocks (contract_address, block_number)
VALUES ($1, $2)
ON CONFLICT (contract_address) DO UPDATE SET block_number = EXCLUDED.block_number
```

### DEX Uygulama Notu — Nova Shield için

Blockchain indexer checkpoint mekanizması: Her contract/chain için son işlenen blok takibi. Crash recovery'de buradan devam edilir. Multi-chain (mainnet + Nova Shield chain) için per-contract kayıt önemli.

---

## 1.19 — `crates/database/src/leader_pg_lock.rs` (~93 satır)

### Amaç

PostgreSQL session-level advisory lock kullanarak leader election: multi-pod deployment'ta yalnızca bir pod autopilot görevi yapar.

### Tablo/Struct Tanımı

```rust
pub struct LeaderLock {
    pool: PgPool,
    key: String,
    guard: Option<PgLockGuard>,
    try_every: Duration,
    limiter: DefaultDirectRateLimiter,
}
```

### SQL Queries

**`try_acquire()`**:
```sql
SELECT pg_try_advisory_lock(hashtextextended($1, 0))
```

`pg_try_advisory_lock`: Blocking olmayan advisory lock. Session-level: bağlantı kapanınca lock otomatik serbest kalır.

**`ping()`**:
```sql
SELECT 1
```

### Mekanizma

```
1. try_acquire() rate-limited olarak çalışır
2. pg_try_advisory_lock → true: lock alındı, PgLockGuard oluştur
3. pg_try_advisory_lock → false: başka pod leader
4. ping() başarısız: session öldü → guard drop et
5. PgLockGuard::drop() → pg_advisory_unlock()
```

### Güvenlik

Session-level lock: Uygulama crash veya network partition durumunda bağlantı kesilince lock otomatik release. Distributed lock sistemlerine kıyasla daha basit.

### DEX Uygulama Notu — Nova Shield için

Nova Shield'ın multi-pod autopilot deployment'ı için PostgreSQL advisory lock yeterli bir leader election mekanizması. Öneri: Başlangıçta PG advisory lock, ölçek büyüyünce etcd/Redis.

---

## 1.20 — `crates/database/src/reference_scores.rs` (~81 satır)

### Amaç

Her auction için referans score'ları saklar. Solver scoring ve reward hesabında kullanılır.

### SQL Queries

```sql
INSERT INTO reference_scores (auction_id, solver, reference_score)
SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::numeric[])
ON CONFLICT DO NOTHING
```

`UNNEST` ile paralel array'leri satırlara açar.

### DEX Uygulama Notu — Nova Shield için

Solver incentive mekanizması için reference score tracking. `UNNEST` ile batch insert pattern Nova Shield'ın tüm batch kayıt işlemlerinde kullanılmalı.

---

## 1.21 — `crates/database/src/settlement_executions.rs` (~286 satır)

### Amaç

Settlement transaction submission sürecini takip eder: başlama zamanı, bitiş zamanı, bloklar, deadline, sonuç.

### Tablo/Struct Tanımı

```rust
pub struct SettlementExecution {
    pub auction_id: AuctionId,
    pub solver: Address,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub start_block: i64,
    pub end_block: Option<i64>,
    pub deadline_block: i64,
    pub outcome: Option<String>,   // None = in-progress; Some("success") | Some("error:...")
}
```

### SQL Queries

**`insert()`** — Başlangıç:
```sql
INSERT INTO settlement_executions
    (auction_id, solver, start_timestamp, start_block, deadline_block)
VALUES ($1, $2, $3, $4, $5)
```

**`update()`** — Tamamlandığında:
```sql
UPDATE settlement_executions
SET end_timestamp = $1, end_block = $2, outcome = $3
WHERE auction_id = $4 AND solver = $5
```

### DEX Uygulama Notu — Nova Shield için

Settlement lifecycle tracking: Nova Shield'ın batch settlement sistemi için submission start/end tracking kritik. "Bu auction'da zaten bir submission denendi mi?" sorusunu cevaplamak için.

---

## 1.22 — `crates/database/src/cow_amms.rs` (~183 satır)

### Amaç

CoW AMM kayıtlarını yönetir. CoW AMM birden fazla tradeable token içeren özel havuzlardır.

### Tablo/Struct Tanımı

```rust
pub struct CowAmm {
    pub address: Address,
    pub factory_address: Address,
    pub tradeable_tokens: Vec<Address>,
    pub block_number: i64,
    pub tx_hash: TransactionHash,
}
```

### SQL Queries

**`upsert_batched()`** — 200'lü chunk'lar:
```sql
INSERT INTO cow_amms (address, factory_address, tradeable_tokens, block_number, tx_hash)
SELECT * FROM UNNEST($1::bytea[], $2::bytea[], $3::bytea[][], $4::bigint[], $5::bytea[])
ON CONFLICT (address) DO UPDATE SET
    factory_address = EXCLUDED.factory_address,
    tradeable_tokens = EXCLUDED.tradeable_tokens,
    block_number = EXCLUDED.block_number,
    tx_hash = EXCLUDED.tx_hash
```

`bytea[][]`: 2D bytea array — her AMM için token adresleri listesi.

**`delete_by_block_range()`** — Reorg recovery:
```sql
DELETE FROM cow_amms
WHERE factory_address = $1
  AND block_number >= $2
  AND block_number <= $3
```

### DEX Uygulama Notu — Nova Shield için

Nova Shield AMM yönetimi için:
1. `tradeable_tokens` 2D array: Çoklu-token havuzlarını tek kolonda saklamak.
2. `delete_by_block_range` per-factory: Farklı AMM factory'leri bağımsız reorg recovery için.
3. 200-item batch upsert: Büyük AMM listelerini verimli yükleme.

---

## 1.23 — `crates/database/src/auction_prices.rs` (~148 satır)

### Amaç

Her auction'daki token fiyatlarını saklar. DEPRECATED — veri artık `competition_auctions` tablosunda; ancak solver team için korunuyor.

### SQL Queries

**Batch INSERT (5000-item chunks)**:
```sql
INSERT INTO auction_prices (auction_id, token, price)
SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::numeric[])
ON CONFLICT DO NOTHING
```

**`fetch_latest_token_price()`**:
```sql
SELECT price FROM auction_prices
WHERE token = $1
ORDER BY auction_id DESC
LIMIT 1
```

### DEX Uygulama Notu — Nova Shield için

Token fiyat geçmişi için ayrı tablo, "belirli bir token'ın son fiyatı neydi?" sorgularını verimli destekler. Ancak `competition_auctions.price_tokens/price_values` array'i ile duplikasyon yaratır — Nova Shield bu ikisinden birini seçmeli.

---

## 1.24 — `crates/database/src/` — Diğer Yardımcı Modüller

### Migration sistemi

CoW Protocol, sqlx'in build-time migration verification'ını kullanır. Her migration dosyası `V{n}__{description}.sql` formatında.

Önemli pattern'lar:
- `CREATE TABLE IF NOT EXISTS`: Idempotent migration
- `ALTER TABLE ADD COLUMN IF NOT EXISTS`: Mevcut production tablosuna güvenli sütun ekleme
- `CREATE INDEX CONCURRENTLY`: Production tablosu kilitlemeden index oluşturma

Nova Shield için: sqlx migration sistemi birebir kullanılabilir. `CONCURRENTLY` index creation production deployment için kritik.

---

# BÖLÜM 2 — SHARED CRATE (18 Dosya)

## 2.1 — `crates/shared/src/external_prices.rs` (~168 satır)

### Amaç

Solver'dan alınan token fiyatlarını (ETH cinsinden) normalize eder ve yönetir. Tüm fee hesaplamalarının ve order değerlendirmelerinin temel fiyat kaynağı.

### Tablo/Struct Tanımı

```rust
pub struct ExternalPrices(HashMap<Address, BigRational>);
// Invariant: native_token ve BUY_ETH_ADDRESS her zaman price=1
```

### Matematiksel Formüller

**`to_native_xrate(price: U256)`**:
```rust
fn to_native_xrate(price: U256) -> BigRational {
    price.to_big_rational() / BigRational::from_integer(1_000_000_000_000_000_000u64.into())
}
// = price / 1e18
```

API'dan gelen fiyat formatı: "1e18 adet token almak için gereken ETH miktarı"
→ `price / 1e18` = "1 atom token almak için gereken ETH"

**`try_new()`**:
```rust
pub fn try_new(native_token: Address, tokens: HashMap<Address, U256>) -> Result<Self> {
    let prices: HashMap<Address, BigRational> = tokens.into_iter()
        .map(|(addr, price)| (addr, to_native_xrate(price)))
        .collect();

    for &addr in &[native_token, BUY_ETH_ADDRESS] {
        let p = prices.get(&addr).context("missing native token price")?;
        ensure!(*p == BigRational::one(), "native token price must be 1");
    }

    Ok(ExternalPrices(prices))
}
```

### Güvenlik

Invariant enforcement: `native_token` ve `BUY_ETH_ADDRESS` fiyatlarının tam olarak 1 olması zorunlu. Bu invariant bozulursa tüm fee hesaplamaları yanlış olur.

`BigRational`: Exact rational arithmetic — `f64` yerine hassas hesaplama. `num-rational` crate.

### DEX Uygulama Notu — Nova Shield için

1. **Fiyat formatı**: `price / 1e18` normalizasyonu. Nova Shield'ın oracle fiyatları için aynı format önerilir.

2. **BigRational**: Fee ve slippage hesaplamalarında `f64` yerine `BigRational` kullanımı. Finansal hesaplamalarda kritik.

3. **`BUY_ETH_ADDRESS` özel adresi**: Native token alımı için sahte adres. Nova Shield'ın native token handle etme mekanizması için aynı pattern.

---

## 2.2 — `crates/shared/src/order_quoting.rs` (büyük dosya)

### Amaç

Order oluşturulmadan önce fiyat teklifi (quote) hesaplar ve saklar. `OrderQuoting` trait, quote engine'in core abstraction'ıdır.

### Tablo/Struct Tanımı

```rust
pub struct QuoteParameters {
    pub sell_token: Address,
    pub buy_token: Address,
    pub side: OrderQuoteSide,
    pub verification: QuoteVerificationMode,
    pub signing_scheme: SigningScheme,
    pub additional_gas: Option<u64>,
    pub timeout: Option<Duration>,
}

pub struct QuoteData {
    pub quoted_sell_amount: U256,
    pub quoted_buy_amount: U256,
    pub fee_parameters: FeeParameters,
    pub kind: OrderKind,
    pub expiration: DateTime<Utc>,
    pub quote_kind: QuoteKind,
    pub solver: H160,
    pub verified: bool,
    pub metadata: serde_json::Value,
}

pub struct Quote {
    pub id: Option<QuoteId>,
    pub data: QuoteData,
    pub sell_amount: U256,
    pub buy_amount: U256,
    pub fee_amount: U256,
}
```

### Matematiksel Formüller

**`with_scaled_sell_amount()`** — U512 overflow önleme:

```rust
fn with_scaled_sell_amount(self, sell_amount: U256) -> Self {
    // buy_amount = quoted_buy * (sell_amount / quoted_sell)
    // U256 * U256 → U512 intermediate (overflow riski)
    let scaled_buy = U512::from(self.data.quoted_buy_amount)
        .widening_mul(U512::from(sell_amount))
        / U512::from(self.data.quoted_sell_amount);

    let buy_amount = U256::try_from(scaled_buy).unwrap_or(U256::MAX);

    Quote { sell_amount, buy_amount, ..self }
}
```

`widening_mul`: U256 × U256 → U512 (overflow olmadan). U512 → U256 sığmıyorsa U256::MAX ile doyurma.

### Trait Tanımı

```rust
#[async_trait]
pub trait OrderQuoting: Send + Sync + 'static {
    async fn calculate_quote(&self, params: QuoteParameters) -> Result<Quote, CalculateQuoteError>;
    async fn store_quote(&self, quote: Quote) -> Result<Quote, DbError>;
    async fn find_quote(&self, id: Option<QuoteId>, params: FindQuoteParameters)
        -> Result<Quote, FindQuoteError>;
}
```

`mockall` ile otomatik mock üretilir.

### Error Türleri

```rust
pub enum CalculateQuoteError {
    SellAmountDoesNotCoverFee,
    Price(EstimatorKind, PriceEstimationError),
    QuoteNotVerified,
    Other(anyhow::Error),
}

pub enum FindQuoteError {
    NotFound(QuoteId),
    ParameterMismatch(Box<QuoteData>),
    Expired(QuoteId),
}
```

### Güvenlik

`SellAmountDoesNotCoverFee`: Fee > sell_amount durumunda negatif net amount ile order oluşturulmasını engeller.

`QuoteNotVerified`: Simülasyon zorunluysa ve başarısız olduysa gerçekleşmeyecek quote üzerine order oluşturulmasını engeller.

### DEX Uygulama Notu — Nova Shield için

1. **`widening_mul` U512**: U256 token amount'ları ile oran hesabında overflow kaçınmak için zorunlu. Nova Shield'ın AMM formülleri için aynı pattern.

2. **`OrderQuoteSide` ayrımı**: Sell order ve Buy order için farklı fee hesabı ve amount scaling.

3. **Quote expiration + verification**: Nova Shield quote sistemi her quote için TTL ve simülasyon doğrulaması içermeli.

---

## 2.3 — `crates/shared/src/order_validation.rs` (~300+ satır)

### Amaç

Order oluşturulmadan önce kapsamlı validasyon: kısmi validasyon (imzasız), tam validasyon (EVM simülasyonu ile).

### Tablo/Struct Tanımı

```rust
pub enum PartialValidationError {
    Forbidden,
    ValidTo(InvalidValidTo),
    InvalidNativeSellToken,
    SameBuyAndSellToken,
    UnsupportedBuyTokenDestination(BuyTokenDestination),
    UnsupportedSellTokenSource(SellTokenSource),
    UnsupportedOrderType,
    UnsupportedToken(Address),
    Other(anyhow::Error),
}

pub enum ValidationError {
    PartialValidation(PartialValidationError),
    InsufficientBalance,
    InvalidSignature,
    WrongOwner(H160),
    InvalidEip1271Signature,
    TooManyLimitOrders,
    TooMuchGas,
    QuoteNotVerified,
    NotSupported,
    PriceForQuote(Error),
    MissingFrom,
    OrderAlreadyExists,
    InvalidNativeBuyToken,
    ZeroAmount,
    UnsupportedSignature,
    InvalidBuyAmount,
    InvalidSellAmount,
    Other(anyhow::Error),
}
```

### `OrderSimulator` Yapısı

```rust
pub struct OrderSimulator {
    inner: Arc<dyn OrderSimulating>,
    timeout: Duration,
    metrics: Arc<Metrics>,
}
```

Her simulasyon: başlangıç zamanı alınır, `timeout` ile race, sonuç histogram'a kaydedilir.

### Güvenlik

`WrongOwner`: EIP-1271 veya PreSign için imzacı address doğrulanmaz; imzanın geçerliliği kontrol edilir. Klasik ECDSA için ise recovered address == declared owner zorunlu.

`TooManyLimitOrders`: Per-user limit order sınırı — DDOS ve spam önlemi.

### DEX Uygulama Notu — Nova Shield için

1. İki aşamalı validasyon (partial → full): Signature gerektirmeyen kontroller önce, maliyetli kontroller (EVM simülasyon) sonra.

2. `TooManyLimitOrders` gibi per-user rate limiting — open order book spam önlemi.

3. Tüm error varyantları explicit tanımlanmalı — hangi hata kullanıcı tarafından düzeltilebilir, hangisi fatal?

---

## 2.4 — `crates/shared/src/fee.rs` (~189 satır)

### Amaç

Gas-based fee hesabı ve volume-based fee politikası.

### Tablo/Struct Tanımı

```rust
pub struct FeeParameters {
    pub gas_amount: f64,
    pub gas_price: f64,
    pub sell_token_price: f64,   // default = 1.0
}

pub struct VolumeFeePolicy {
    pub bucket_overrides: Vec<TokenBucketFeeOverride>,
    pub default_factor: f64,
    pub enable_sell_equals_buy_volume_fee: bool,
    pub native_token: Address,
}

pub struct TokenBucketFeeOverride {
    pub token: Address,
    pub factor: f64,
}
```

### Matematiksel Formüller

**Gas-based fee**:
```rust
pub fn fee(&self, additional_cost: f64) -> U256 {
    let total_gas = self.gas_amount + additional_cost;
    let fee_in_eth = total_gas * self.gas_price;
    let fee_in_sell_token = fee_in_eth / self.sell_token_price;
    U256::from(fee_in_sell_token.ceil() as u64)
}
```

Formül:
```
fee_atoms = ceil((gas_amount + additional_cost) * gas_price / sell_token_price)
```

**`get_applicable_volume_fee_factor()`**:
```rust
pub fn get_applicable_volume_fee_factor(
    &self,
    sell_token: &Address,
    buy_token: &Address,
    default_fee_from_arg: Option<f64>,
) -> f64 {
    // WETH <-> ETH aynı token: fee skip
    if !self.enable_sell_equals_buy_volume_fee
        && is_weth_eth_pair(sell_token, buy_token, self.native_token)
    {
        return 0.0;
    }

    // İlk eşleşen bucket
    for bucket in &self.bucket_overrides {
        if bucket.token == *sell_token || bucket.token == *buy_token {
            return bucket.factor;
        }
    }

    // Fallback
    default_fee_from_arg.unwrap_or(self.default_factor)
}
```

Fallback zinciri: bucket_override → arg_default → global_default.

### Güvenlik

`f64` kullanımı: precision risk mevcut ancak `ceil()` ile yuvarlandığından etki minimize edilmiş. Edge case'lerde 1 atom hata olabilir.

### DEX Uygulama Notu — Nova Shield için

1. **Fee formülü**: `ceil((gas_amount + additional) * gas_price / sell_token_price)` — Gas fee, sell token cinsinden hesaplanmalı.

2. **Bucket-based volume fee**: Token-specific fee oranları. Nova Shield'ın farklı token tiplerine farklı fee tier'ları desteklemesi (stable-stable daha düşük, volatile-volatile daha yüksek).

3. **WETH↔ETH exemption**: Native token ve wrapped versiyonu arasındaki swap'larda fee muafiyeti.

---

## 2.5 — `crates/shared/src/remaining_amounts.rs` (~449 satır)

### Amaç

Partial fill order'larda kalan satılabilir/alınabilir miktarı hesaplar. İki ayrı oran (execution + balance) overflow olmadan hesaplanır.

### Tablo/Struct Tanımı

```rust
pub struct Remaining {
    pub execution: Ratio<U256>,
    pub balance: Ratio<U256>,
}

pub struct Ratio<T> {
    numerator: T,
    denominator: T,
}
```

### Matematiksel Formüller

**`from_order_with_balance()`** — Partial fill order:

```
execution_ratio = (sell_amount - sum_executed) / sell_amount
need = (sell_amount + fee) * execution_ratio
balance_ratio = min(actual_balance, need) / need
```

**`remaining(total: U256)`**:
```rust
pub fn remaining(&self, total: U256) -> U256 {
    // 1. execution oranı uygula
    let after_exec = self.execution.scalar_mul(total);
    // 2. balance oranı uygula (U512 intermediate)
    self.balance.full_scalar_mul(after_exec)
}
```

`scalar_mul`: `(numerator * total) / denominator`
`full_scalar_mul`: U256 × U256 → U512, sonra / denominator → U256

İki ayrı adım: tek seferde `total * exec_ratio * balance_ratio` hesaplansaydı intermediate U256 overflow riski vardı.

**FoK (Fill-or-Kill)**:
```rust
let execution = if executed_amount == 0 { Ratio::ONE } else { Ratio::ZERO };
let has_enough = actual_balance >= sell_amount + fee_amount;
let balance = if has_enough { Ratio::ONE } else { Ratio::ZERO };
```

FoK binary: ya tamamen yürütülebilir ya da hiç.

### Güvenlik

`full_scalar_mul` (U512 intermediate): U256::MAX × U256::MAX hesabında overflow oluşmaması kritik. Yanlış kalan miktarı → yanlış order fill.

`min(actual_balance, need) / need`: Bakiye `need`'den fazla olsa dahi `need`'e sınırlandırılır — ratio asla 1'i aşmaz.

### DEX Uygulama Notu — Nova Shield için

1. **Çift-ratio sistemi**: `execution * balance` iki ayrı oran — bağımsız hesaplanır, sıralı uygulanır. U256 overflow'u önler.

2. **`full_scalar_mul` U512**: Nova Shield'ın AMM matematiği için zorunlu.

3. **FoK vs partial**: İki farklı semantik — Nova Shield her ikisini de desteklemeli ve ayrı kod path'leri ile handle etmeli.

---

## 2.6 — `crates/shared/src/current_block.rs` (~87 satır)

### Amaç

Mevcut Ethereum bloğunu takip eder. WS veya HTTP polling ile blok stream'i sağlar.

### Tablo/Struct Tanımı

```rust
pub struct Arguments {
    pub block_stream_poll_interval: Duration,   // default: 500ms
    pub node_ws_url: Option<Url>,
}
```

### Mekanizma

```rust
pub async fn stream(args: &Arguments, http_transport: Web3) -> BlockStream {
    if let Some(ws_url) = &args.node_ws_url {
        current_block_ws_stream(ws_url).await    // push-based, ~100ms latency
    } else {
        current_block_stream(http_transport, args.block_stream_poll_interval)
        // polling-based, 500ms interval
    }
}
```

### DEX Uygulama Notu — Nova Shield için

Nova Shield kendi bloğunu ürettiğinden, block stream'i external RPC'ye gerek kalmadan dahili olarak sağlayabilir. WS vs HTTP polling abstraction'ı test/production ayrımı için kullanışlı.

---

## 2.7 — `crates/shared/src/token_list.rs` (~247 satır)

### Amaç

Desteklenen token listesini yönetir (Uniswap tokenlist format). Arka planda periyodik olarak güncellenir.

### Tablo/Struct Tanımı

```rust
pub struct AutoUpdatingTokenList {
    tokens: Arc<RwLock<HashSet<Address>>>,
}

pub struct TokenListConfiguration {
    pub url: Option<Url>,
    pub chain_id: u64,
    pub client: reqwest::Client,
    pub update_interval: Duration,
    pub hardcoded: Vec<Address>,
}
```

### Mekanizma

```rust
pub async fn from_configuration(config: TokenListConfiguration) -> Self {
    let tokens = Arc::new(RwLock::new(fetch_initial(&config).await));

    tokio::task::spawn(async move {
        loop {
            sleep(config.update_interval).await;
            match config.get_external_list().await {
                Ok(new) => *tokens.write().unwrap() = new,
                Err(e) => warn!("token list update failed: {:?}", e),
            }
        }
    });

    Self { tokens }
}
```

`Arc<RwLock<HashSet>>`: Multiple reader, single writer. Okuma operasyonları concurrent; yazma exclusive.

### Güvenlik

Background task fail: `tracing::warn!` ile log'lanır, mevcut liste tutulur. Silent degradation — update başarısız olsa bile son başarılı liste devam eder.

### DEX Uygulama Notu — Nova Shield için

1. `Arc<RwLock<HashSet<Address>>>`: Thread-safe token whitelist — read-heavy workload için ideal.

2. Background refresh + graceful degradation: Update başarısız olsa mevcut liste devam eder.

3. `hardcoded` Vec: Kritik token'lar listeleme URL'sinden bağımsız her zaman dahil edilir.

---

## 2.8 — `crates/shared/src/db_order_conversions.rs` (~291 satır)

### Amaç

`database::orders::FullOrder` → `model::Order` dönüşümleri. DB katmanı ile API modeli arasındaki köprü.

### Kritik Dönüşümler

**`full_order_into_model_order()`** — EthFlow owner override:
```rust
// DB'de owner = sözleşme adresi; onchain_user = gerçek kullanıcı
// Model'de owner = onchain_user olarak expose edilir
owner: row.onchain_user.map(|u| H160(u.0)).unwrap_or(H160(row.owner.0)),

// true_valid_to varsa onu kullan (EthFlow)
valid_to: row.true_valid_to.unwrap_or(row.valid_to) as u32,
```

**`onchain_order_placement_error_from()`**:
```rust
// Deprecated DB variant'ları None'a map et
DbOnchainError::QuoteNotFound => None,
DbOnchainError::InvalidQuote => None,
DbOnchainError::InsufficientFee => None,
```

### DEX Uygulama Notu — Nova Shield için

DB layer ↔ API model ayrımı: Nova Shield'da da DB struct'ları (raw DB types, BigDecimal) ile API struct'ları (U256, H160) ayrı tutulmalı. Dönüşüm fonksiyonları tek bir yerde toplanmalı.

---

## 2.9 — `crates/shared/src/event_storing_helpers.rs` (~49 satır)

### Amaç

Quote DB kaydına dönüştürme. `QuoteData` → `DbQuote` mapping.

### Kritik Logic

```rust
pub fn create_quote_row(id: QuoteId, quote: &Quote) -> DbQuote {
    DbQuote {
        sell_amount: u256_to_big_decimal(&quote.data.quoted_sell_amount),
        // sell_amount_1 = sell_amount + fee (arama için)
        sell_amount_1: u256_to_big_decimal(
            &(quote.data.quoted_sell_amount + quote.data.fee_parameters.fee(0.0))
        ),
        buy_amount: u256_to_big_decimal(&quote.data.quoted_buy_amount),
        ...
    }
}
```

`sell_amount_1 = sell_amount + fee`: Quote aramasında her iki format da desteklenir.

---

## 2.10 — `crates/shared/src/interaction.rs` (~41 satır)

### Amaç

EVM interaction (calldata) trait abstraction. Pre/post hook ve swap interaction'ları için.

### Trait Tanımı

```rust
pub trait Interaction: Send + Sync + fmt::Debug {
    fn encode(&self) -> (Address, U256, Bytes);
    // (target_contract, value_in_wei, calldata)
}

pub struct EncodedInteraction {
    pub target: Address,
    pub value: U256,
    pub calldata: Bytes,
}
```

### DEX Uygulama Notu — Nova Shield için

Nova Shield'ın settlement contract'ı pre/post hook desteği için aynı `Interaction` trait abstraction. `encode() -> (target, value, calldata)` tuple'ı EVM call standardı.

---

## 2.11 — `crates/shared/src/order_creation_simulation.rs` (~453 satır)

### Amaç

Order oluşturulurken EVM simülasyonu yapar. Bazı başarısız simülasyonlar kabul edilir.

### Tablo/Struct Tanımı

```rust
pub enum OrderSimulationError {
    Reverted {
        reason: String,
        summary: String,
        tenderly_url: Option<Url>,
        tenderly_request: Box<dyn std::fmt::Debug + Send + Sync>,
    },
    Infra(anyhow::Error),
}
```

### Simülasyon Mantığı — 3 Exception Kuralı

```rust
fn extract_critical_revert(result: &SimulationResult, order: &Order) -> Option<OrderSimulationError> {
    // 1. Kural: Broken hook → HER ZAMAN critical
    if let Some(hook_error) = disallow_broken_hook(result) {
        return Some(hook_error);
    }

    // 2. Kural: Buy token transfer revert + state override failure → KABUL ET
    if allow_failed_buy_token_transfer(result) {
        return None;
    }

    // 3. Kural: Sell token transfer revert + PreSign → KABUL ET
    if allow_unfunded_presign_order(result, order) {
        return None;
    }

    // Diğer revert'ler → critical
    result.revert.as_ref().map(|r| OrderSimulationError::Reverted { ... })
}
```

**`prepare_simulation()`** — Partial fill amount clamping:
```rust
fn prepare_simulation(order: &Order, balance: Option<U256>) -> SimulationAmount {
    if !order.partially_fillable {
        return SimulationAmount::Full;
    }
    let actual_balance = balance.unwrap_or(U256::ZERO);
    let amount = actual_balance.clamp(U256::ONE, order.sell_amount);
    SimulationAmount::Partial(amount)
}
```

### Güvenlik — Exception Kural Analizi

1. **Broken hook → always reject**: Pre/post hook'lar dış sözleşme çağrısı. Revert ettiyse order tehlikeli (MEV hook, re-entrancy). Hiçbir istisna yok.

2. **Buy token transfer + state override**: Buy token'ı transfer eden kod simülasyonda revert eder ama bu, test balance state override kaynaklıysa ignore et. Gerçek settlement'ta başarılı olacak.

3. **Unfunded presign**: Kullanıcı order'ı imzaladı ama token balance'ı yok (presign sözleşmesi henüz execute edilmemiş). Settlement zamanında funded olması beklenir.

### DEX Uygulama Notu — Nova Shield için

1. **Simülasyon exception kuralları**: Nova Shield order simülasyonu için benzer exception sistemi. Hangi hata "tolere edilebilir" hangisi "fatal" — protokol ekonomisi açısından kritik karar.

2. **Broken hook = always reject**: Nova Shield'ın hook sistemi için aynı kural. Hiçbir hook hatasına göz yumulmamalı.

3. **Partial fill simulation amount clamping**: Gerçek balance ile simulate et; sıfır balance'da 1 atom kullan.

---

## 2.12 — `crates/shared/src/retry.rs` (~44 satır)

### Amaç

Async operation'lar için jitter retry mekanizması.

### Implementasyon

```rust
const MAX_RETRIES: usize = 5;

pub async fn retry_with_sleep_if<F, T, E, P>(
    future: impl Fn() -> F,
    should_retry: P,
) -> Result<T, Vec<E>>
{
    let mut errors = Vec::new();
    for attempt in 1..=MAX_RETRIES {
        match future().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let retryable = should_retry(&e);
                errors.push(e);
                if !retryable || attempt == MAX_RETRIES {
                    return Err(errors);
                }
                let delay = 50u64 + rand::rng().random_range(0..=50);
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
        }
    }
    Err(errors)
}
```

- Jitter: 50ms base + 0-50ms random = 50-100ms. Thundering herd önler.
- `Vec<E>` return: Tüm deneme hataları toplanır — debug için.
- `should_retry` predicate: Bazı hatalar retry edilemez (auth error, not found).

### DEX Uygulama Notu — Nova Shield için

RPC call retry, solver API retry, DB query retry için bu utility kullanılabilir. `should_retry` ile: connection error → retry, authentication error → no retry.

---

## 2.13 — `crates/shared/src/web3.rs` (~76 satır)

### Amaç

Ethereum RPC client konfigürasyonu ve Web3 instance oluşturma.

### Tablo/Struct Tanımı

```rust
pub const MAX_BATCH_SIZE: usize = 100;

pub struct Arguments {
    pub ethrpc_max_batch_size: usize,           // default: 100
    pub ethrpc_max_concurrent_requests: usize,  // default: 10
    pub ethrpc_batch_delay: Duration,           // default: 0s
}
```

### DEX Uygulama Notu — Nova Shield için

Nova Shield kendi RPC node'u olduğundan `max_batch_size` ve `max_concurrent_requests` kendi kapasitesine göre ayarlanabilir. Label'lı Web3 instance'ları, hangi servisten gelen RPC call'larının yavaş olduğunu Prometheus ile takip eder.

---

## 2.14 — `crates/shared/src/http_solver.rs` (~32 satır)

### Amaç

HTTP solver API ile iletişim için temel tipler.

### Tablo/Struct Tanımı

```rust
pub struct TokenAmount {
    pub amount: U256,
    pub token: Address,
}

pub enum InternalizationStrategy {
    EncodeAllInteractions,
    SkipInternalizableInteraction,
    Unknown,
}
```

`InternalizationStrategy`: Solver'ın interaction'ları nasıl kodlayacağını belirler. `SkipInternalizableInteraction` → solver bazı interaction'ları atlayabilir.

### DEX Uygulama Notu — Nova Shield için

Nova Shield solver API tasarımında `InternalizationStrategy` benzeri mekanizma: Nova Shield native AMM direkt settlement'a entegre edilirse bu interaction'ların ayrıca encode edilmesi gerekmez.

---

## 2.15 — `crates/shared/src/arguments.rs` (~80+ satır)

### Amaç

Ortak CLI argument'ları ve log filter konfigürasyonu.

### Kritik Macro

```rust
macro_rules! logging_args_with_default_filter {
    ($default_filter:expr) => {
        pub struct LoggingArguments {
            #[clap(long, env, default_value = $default_filter)]
            pub log_filter: String,
        }
    }
}
```

Default log filter:
```
info,autopilot=debug,driver=debug,shared=debug,solver=debug,orderbook=debug,...
```

```rust
pub const DB_MAX_CONNECTIONS_DEFAULT: NonZeroU32 = NonZeroU32::new(10).unwrap();
```

`const fn` context'te `unwrap()`: Panic olmaz çünkü 10 > 0.

### DEX Uygulama Notu — Nova Shield için

Per-crate log filter granularity: production'da `info` genel seviye, kritik servisler için `debug`. Nova Shield'da `nova-shield-matching=debug`, `nova-shield-settlement=debug` gibi granüler filtreler.

---

## 2.16 — `crates/shared/src/url.rs` (~73 satır)

### Amaç

URL manipülasyon yardımcıları: path join ve base URL extraction.

### Implementasyon

**`join(url, path)`**:
```rust
pub fn join(base: &Url, path: &str) -> Url {
    let base_str = base.as_str().trim_end_matches('/');
    let path_str = path.trim_start_matches('/');
    format!("{}/{}", base_str, path_str).parse().unwrap()
}
```

Her iki taraftan `/` strip: `https://api.example.com/` + `/v1/orders` → `https://api.example.com/v1/orders`

**`split_at_path(url)`**:
```rust
pub fn split_at_path(url: &Url) -> (Url, String) {
    let mut base = url.clone();
    let path = base.path().to_owned();
    base.set_path("");
    base.set_query(None);
    base.set_fragment(None);
    (base, path)
}
```

Solver URL routing için: base URL (host) + path (endpoint).

### DEX Uygulama Notu — Nova Shield için

Solver ve oracle API URL management. Slash-trimming: konfigürasyondan gelen URL'lerin normalize edilmesi, çift slash veya missing slash sorunlarını önler.

---

## 2.17 — `crates/shared/src/` — Ek Modüller

### `price_estimation/` modülü (shared/src/price_estimation/)

Price estimation, birden fazla external fiyat kaynağından (1inch, 0x, Uniswap, vb.) fiyat alır ve normalize eder. Temel trait:

```rust
#[async_trait]
pub trait PriceEstimating: Send + Sync {
    async fn estimate(&self, query: &Query) -> Vec<Result<Estimate, PriceEstimationError>>;
}
```

Her estimator kendi endpoint'ini çağırır; sonuçlar `CompetitivePriceEstimator` ile merge edilir.

**DEX Uygulama Notu**: Nova Shield'ın fiyat keşfi için birden fazla likidite kaynağı (Nova Shield AMM, external DEX'ler) arasında en iyi fiyatı bulmak aynı pattern'ı kullanabilir.

### `signature_validator.rs`

EIP-1271 ve PreSign signature validation. `isValidSignature(bytes32, bytes)` EVM call ile doğrulama.

**DEX Uygulama Notu**: Nova Shield akıllı sözleşme cüzdanı (EIP-1271) desteği için aynı pattern. On-chain signature validation, off-chain ECDSA'ya ek olarak.

---

# ÖZET — GENEL DEX BULGULARI

## CoW Protocol Database Katmanı — Anahtar Tasarım Kararları

| Pattern | Kullanım | Nova Shield Uygulaması |
|---------|----------|------------------------|
| `ON CONFLICT DO NOTHING` | Event idempotency | Multi-pod indexer safety |
| `ON CONFLICT DO UPDATE` | Reorg recovery | Chain reorganization handling |
| `MATERIALIZED` CTE | Query planner kontrolü | Büyük order taramalarında |
| `LATERAL JOIN` | Per-row dependent subquery | Settlement-trade eşleme |
| `LAG()` window function | Önceki settlement bulma | Batch settlement ayrımı |
| `@>` array containment | Order UID lookup | Auction-order ilişkisi |
| `DISTINCT ON` | Per-group latest row | Order event dedup |
| `VALUES` JOIN | Batch key lookup | N+1 query önleme |
| Two-phase pagination | Lightweight UID → full data | Order history API |
| `DELETE + INSERT` in CTE | Single-row invariant | Aktif auction yönetimi |
| `UNNEST` parallel arrays | Batch insert | Reference scores, prices |
| GIN index + `@>` | Array containment search | Auction-order mapping |

## CoW Protocol Shared Katmanı — Anahtar Algoritmalar

| Algoritma | Lokasyon | Matematiksel Formül |
|-----------|----------|---------------------|
| Gas-based fee | `fee.rs` | `ceil((gas + additional) * price / sell_token_price)` |
| Quote effective rate | `quotes.rs` | `buy / (sell + gas_cost_in_sell)` |
| Remaining amounts | `remaining_amounts.rs` | `execution_ratio × balance_ratio` (U512 intermediate) |
| Price normalization | `external_prices.rs` | `api_price / 1e18` |
| Buy amount scaling | `order_quoting.rs` | `widening_mul U512: quoted_buy * sell / quoted_sell` |
| Volume fee bucket | `fee.rs` | First-match bucket override |
| Slippage check | `ethflow_orders.rs` | `1.0 - (order_buy / quote_buy) >= threshold` |

## Nova Shield DEX için Öncelikli Uygulamalar (Sıralı)

1. **U512 intermediate arithmetic**: AMM matematik hesaplamalarında U256 overflow önleme — `widening_mul` ve `full_scalar_mul` pattern'ları.

2. **MATERIALIZED CTE**: Order matching sorgularında query planner bypass — her ~12s çalışan sorgu için kritik.

3. **Dual ratio remaining amounts**: Partial fill tracking için execution × balance iki ayrı oran; sıralı uygulama ile overflow önleme.

4. **`ON CONFLICT DO NOTHING/UPDATE` pattern'ları**: Blockchain event idempotency ve reorg recovery için.

5. **Bucket-based fee policy**: Token-specific fee tier'ları; first-match-wins semantiği.

6. **Two-phase pagination**: Büyük order geçmişi için lightweight UID scan → full data fetch.

7. **Advisory lock leader election**: Multi-pod autopilot için PostgreSQL session-level lock.

8. **LATERAL JOIN settlement eşleme**: Same-block multi-settlement ayırma.

9. **BigRational fiyat hesabı**: Finansal precision için `f64` yerine exact rational arithmetic.

10. **Order simulation 3-exception rule**: Hook hataları always reject; buy token state override ve unfunded presign kabul edilebilir.

11. **WETH↔ETH fee exemption**: Native token ve wrapped versiyonu arasındaki swap'larda fee muafiyeti.

12. **`ByteArray<N>` const-generic type**: Compile-time boyut doğrulaması ile address, hash, UID tipleri.
