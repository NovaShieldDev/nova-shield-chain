# Penumbra Analizi — Agent: util / bench / test Crate'leri

**Analiz Tarihi:** 2026-07-22
**Kapsam:** crates/util (18 dosya) + crates/bench (10 dosya) + crates/test (19 dosya) = 47 dosya
**Toplam Satır:** ~3.200+ satır (tahmini)

---

## BÖLÜM 1: crates/util

### util alt-crate'leri:
- `auto-https` — ACME/TLS otomatik sertifika yönetimi
- `cometindex` — CometBFT event'lerini PostgreSQL'e indeksleyen framework
- `tendermint-proxy` — gRPC proxy, Tendermint RPC'ye yönlendirme
- `tower-trace` — Tower middleware: tracing instrumentation

---

## [lib.rs] — auto-https/src/lib.rs
**Dosya:** `/crates/util/auto-https/src/lib.rs`
**Satır Sayısı:** 91

### Genel Amaç
ACME protokolü (Let's Encrypt) üzerinden otomatik TLS sertifikası alır ve yönetir. `axum` web framework ile entegre çalışan HTTPS bağlantı kabul edici döndürür. Arka planda sertifika yenileme görevi çalıştırır.

### Sabitler
- `ALPN_PROTOCOLS: [&[u8]; 2] = [b"h2", b"http/1.1"]` (Satır 20) — Tercih sırasıyla HTTP/2 + HTTP/1.1 (grpc-web geriye dönük uyumluluk için)
- `CACHE_DIR: &str = "tokio_rustls_acme_cache"` (Satır 24) — Sertifika önbellek dizini (göreceli yol zorunluluğu: `Path::join` davranışı nedeniyle mutlak yol kullanılamaz)

### Fonksiyonlar

#### `pub fn axum_acceptor(home: PathBuf, domain: String, production_api: bool) -> (AxumAcceptor, impl Future<Output = Result<(), Error>>)` (Satır 31-57)
- **Parametreler:** `home` = çalışma dizini, `domain` = domain adı, `production_api` = true → Let's Encrypt üretim API'si, false → staging
- **Dönüş:** Tuple: `(AxumAcceptor, background_worker_future)`
- **Algoritma:**
  1. `home.join(CACHE_DIR)` ile önbellek dizini oluşturulur
  2. `AcmeConfig::new(vec![domain]).cache(cache).directory_lets_encrypt(production_api).state()` — ACME state makinesi
  3. `ServerConfig::builder().with_no_client_auth().with_cert_resolver(state.resolver())` — TLS yapılandırması
  4. `rustls_config.alpn_protocols = self::alpn_protocols()` — ALPN set edilir
  5. `state.axum_acceptor(rustls_config)` → bağlantı kabul edici
  6. `self::acme_worker(state)` → arka plan görevi
- **Tasarım:** Her şey iki ayrı döndürülür; caller her ikisini de spawn etmeli

#### `async fn acme_worker<EC, EA>(mut state: AcmeState<EC, EA>) -> Result<(), anyhow::Error>` (Satır 63-83)
- `#[tracing::instrument(level = "error", skip_all)]`
- Sonsuz döngü: `state.next().await` → ACME event bekleme
- `Ok(ok)` → debug log
- `Err(err)` → error log + `bail!` (fonksiyon sonlanır)
- `None` → `debug_assert!(false)` + hata (end-of-stream beklenmiyor)

#### `fn alpn_protocols() -> Vec<Vec<u8>>` (Satır 88-90)
- `ALPN_PROTOCOLS.into_iter().map(<[u8]>::to_vec).collect()` — sabitten owned copy üretir

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- Nova Shield'in node RPC endpoint'i için aynen bu pattern kullanılabilir: production_api flag'i ile staging/prod ayrımı
- ALPN protokol listesinde HTTP/2 önce gelmeli; gRPC servisler için zorunlu
- ACME worker future'ı ayrı tokio task olarak spawn edilmeli, ana işlem bloklamamalı
- Sertifika cache dizini mutlak yol olmamalı (Path::join ile home'a join edilmeli)

---

## [lib.rs] — cometindex/src/lib.rs
**Dosya:** `/crates/util/cometindex/src/lib.rs`
**Satır Sayısı:** 15

### Genel Amaç
`cometindex` crate'inin dışa aktarma dosyası. CometBFT event indexer framework'ünün public API'sini tanımlar.

### Re-exportlar
- `ContextualizedEvent` (contextualized modülünden)
- `AppView`, `PgPool`, `PgTransaction` (index modülünden)
- `Indexer` (indexer modülünden)
- `async_trait` (async_trait crate'inden re-export — kullanıcıların `use cometindex::async_trait` yapabilmesi için)
- `sqlx` (sqlx crate'i re-export)

### Nova Shield Chain İçin Dersler
- Event indexer, blockchain'den ayrı bir süreç olarak çalıştırılabilir; event'leri PostgreSQL'e yazarak sorgulama yapılabilir hale getirir
- `async_trait` re-export: trait içinde async fn kullanmak için Rust'ta hâlâ gerekli (GAT tamamlanınca değişecek)

---

## [index.rs] — cometindex/src/index.rs
**Dosya:** `/crates/util/cometindex/src/index.rs`
**Satır Sayısı:** 263

### Genel Amaç
Event indexing framework'ünün temel veri tiplerini ve `AppView` trait'ini tanımlar. CometBFT block event'lerini toplu işlemek için veri yapıları: `BlockEvents`, `EventBatch`, `EventBatchContext`, `Version`.

### Struct/Enum/Type Listesi

#### `EventReference` (Satır 13-18) — private
```rust
struct EventReference {
    pub event_index: usize,     // block içindeki sıra
    pub tx_hash: Option<[u8; 32]>, // bağlı tx (varsa)
    pub local_rowid: i64,        // DB'deki local satır ID
}
```
Copy + Clone + Debug. Block içinde event'lerin tx ile ilişkilendirilmesi için.

#### `BlockEvents` (Satır 22-27)
```rust
pub struct BlockEvents {
    height: u64,
    event_refs: Vec<EventReference>,  // sıralı referanslar
    events: Vec<Event>,                // gerçek event'ler
    transactions: BTreeMap<[u8; 32], Vec<u8>>, // tx_hash → tx_bytes
}
```
Builder pattern ile inşa edilir. `EXPECTED_EVENTS: usize = 32` pre-allocation sabiti.

**Fonksiyonlar:**
- `pub(crate) fn new(height: u64) -> Self` (Satır 31) — 32 event kapasiteli oluştur
- `pub(crate) fn push_tx(&mut self, hash: [u8; 32], data: Vec<u8>)` (Satır 43) — tx ekle
- `pub(crate) fn push_event(&mut self, event: Event, tx_hash: Option<[u8; 32]>, local_rowid: i64)` (Satır 48) — event ekle, ref oluştur
- `pub fn height(&self) -> u64` (Satır 60)
- `fn contextualize(&self, event_ref: EventReference) -> ContextualizedEvent<'_>` (Satır 64) — event_ref'ten tam ContextualizedEvent üret
- `pub fn events(&self) -> impl Iterator<Item = ContextualizedEvent<'_>>` (Satır 78) — sıralı event iterator
- `pub fn transactions(&self) -> impl Iterator<Item = ([u8; 32], &'_ [u8])>` (Satır 83) — tx iterator

#### `EventBatch` (Satır 89-96)
```rust
pub struct EventBatch {
    first_height: u64,
    last_height: u64,
    by_height: Arc<Vec<BlockEvents>>,  // Arc: çok index'e aynı batch klonlanabilir
}
```
**Fonksiyonlar:**
- `pub fn new(block_events: Vec<BlockEvents>) -> Self` — first/last height otomatik
- `pub fn empty(&self) -> bool` — `first_height > last_height`
- `pub fn start_later(&mut self, new_start: u64)` — başlangıç height'ı ilerlet (skip)
- `pub fn events_by_block(&self) -> impl Iterator<Item = &'_ BlockEvents>` — skip uygulayarak iterate
- `pub fn events(&self) -> impl Iterator<Item = ContextualizedEvent<'_>>` — flat_map

#### `EventBatchContext` (Satır 149-158)
```rust
pub struct EventBatchContext {
    pub(crate) is_last: bool,
}
```
`is_last()` → bu batch'ten sonra yeni blok gelene kadar başka batch gelmeyeceğini belirtir.

#### `Version` (Satır 170-209)
```rust
pub struct Version {
    major: Option<i64>,
    option_hash: Option<[u8; 32]>,
}
```
`#[derive(PartialOrd, Ord)]` — sıralama destekler. `sqlx::Type` — DB'de native depolanabilir.
- `with_major(v: u64) -> Self` — major versiyon
- `with_option_hash(self, [u8; 32]) -> Self` — seçenek hash ekle
- `major() -> u64` — `unwrap_or_default` + u64 dönüşümü
- `Display` impl: `"v{}"` formatı

#### `AppView` trait (Satır 212-262)
```rust
#[async_trait]
pub trait AppView: Send + Sync {
    fn name(&self) -> String;
    fn version(&self) -> Version { Version::default() }
    async fn reset(&self, dbtx: &mut PgTransaction) -> Result<(), anyhow::Error>;
    async fn on_startup(&self, dbtx: &mut PgTransaction) -> Result<(), anyhow::Error>;
    async fn init_chain(&self, dbtx: &mut PgTransaction, app_state: &serde_json::Value) -> Result<(), anyhow::Error>;
    async fn index_batch(&self, dbtx: &mut PgTransaction, batch: EventBatch, context: EventBatchContext) -> Result<(), anyhow::Error>;
}
```
- `reset` → default impl `unimplemented!()` ile versiyon sıfırlama zorunluluğunu hatırlatır
- `on_startup` → default `Ok(())`; opsiyonel
- `init_chain` → genesis block işleme (zorunlu)
- `index_batch` → toplu event işleme (zorunlu)

### Tasarım Kararları
- `Arc<Vec<BlockEvents>>` kullanımı: Aynı batch N tane index'e fan-out edilirken klonlama ucuz
- `start_later` ile EventBatch'i dilimleyebilmek: Her index kendi watermark'ından itibaren işler
- `Version` struct'ının `option_hash` içermesi: Config değişikliği tespiti için (sadece major version yetmez)

### Nova Shield Chain İçin Dersler
- Nova Shield event indexer için bu pattern direkt kullanılabilir: `AppView` trait implement et, `Indexer` ile kaydet
- `Arc` paylaşımı ile N parallel indexer aynı batch'i işleyebilir (örn. DEX state indexer + balance indexer)
- `start_later` mekanizması: Her indexer kendi watermark'ını takip edip eksik block'ları tekrar işleyebilir

---

## [database.rs] — cometindex/src/database.rs
**Dosya:** `/crates/util/cometindex/src/database.rs`
**Satır Sayısı:** 26

### Genel Amaç
PostgreSQL bağlantı havuzu oluşturmak için iki yardımcı fonksiyon.

### Fonksiyonlar

#### `pub async fn read_only_db(url: &str) -> anyhow::Result<PgPool>` (Satır 8)
- `PgPoolOptions::new().after_connect(...)` — her bağlantı sonrasında `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` çalıştırır
- Yanlışlıkla yazma yapılmasını engeller
- Referans: sqlx issue #481

#### `pub async fn read_write_db(url: &str) -> anyhow::Result<PgPool>` (Satır 23)
- Sade `PgPoolOptions::new().connect(url)` — özel kısıtlama yok

### Tasarım Kararları
- Source DB read-only, destination DB read-write — iki ayrı DB ayrımı mimari düzeyde zorunlu kılınmış
- `after_connect` hook: Her bağlantı üretildiğinde SESSION-level read-only set edilir (transaction-level değil)

### Nova Shield Chain İçin Dersler
- Blockchain state sorgulama için ayrı read-only replika DB tutmak iyi pratik
- `after_connect` hook ile connection-level varsayılanlar set edilebilir (örn. `SET statement_timeout`, `SET lock_timeout`)

---

## [indexer.rs] — cometindex/src/indexer.rs
**Dosya:** `/crates/util/cometindex/src/indexer.rs`
**Satır Sayısı:** 282

### Genel Amaç
CometBFT event'lerini PostgreSQL'e indeksleyen ana Indexer struct'ı ve catchup mantığı.

### Struct/Enum/Type Listesi

#### `Indexer` (Satır 204-207)
```rust
pub struct Indexer {
    opts: Options,
    indices: Vec<Arc<dyn AppView>>,
}
```

**Fonksiyonlar:**
- `pub fn new(opts: Options) -> Self`
- `pub fn with_index(mut self, index: Box<dyn AppView + 'static>) -> Self` — builder pattern, index ekle
- `pub fn with_default_tracing(self) -> Self` — `tracing_subscriber::fmt::init()`
- `pub async fn run(self) -> Result<(), anyhow::Error>` — ana döngü

### `run` Fonksiyon Algoritması (Satır 227-281)
1. `integrity::integrity_check(&src_database_url)` — DB integrity kontrolü
2. `--integrity-checks-only` flag'i varsa erken çık
3. `genesis_json` dosyasını oku ve parse et, `app_state` çıkar
4. `IndexingManager::init(src, dst)` — DB bağlantıları
5. Her index için `reset_index_if_necessary` + `on_startup` (transaction içinde)
6. Sonsuz döngü: `catchup()` → tamamsa ve `exit_on_catchup` set ise çık; değilse `poll_ms` bekle

### `catchup` Fonksiyonu (Satır 91-202)
- **Sabitler:** `DEFAULT_BATCH_SIZE: u64 = 1000`, `BATCH_LOOKAHEAD: usize = 2`
- Her index için ayrı `mpsc::channel<EventBatch>(BATCH_LOOKAHEAD)` kanalı
- Her index için `JoinSet` task'ı spawn edilir:
  - Index height 0 ise `init_chain` çağrılır
  - Her `rx.recv()` ile batch alınır, `start_later` ile watermark uygulanır, `index_batch` çağrılır
  - Her başarılı batch sonrası DB'ye watermark yazılır, commit
- Bir producer task `event_batch(first, last)` ile DB'den batch okur, tüm index kanallarına gönderir

### `reset_index_if_necessary` (Satır 13-85)
- `index_state(&name)` ile mevcut versiyon okunur
- `version.option_hash() != old_version.option_hash()` ve `!new_options` → hata ver (güvenlik)
- `version.major() < old_version.major()` → yanlış binary çalıştırılıyor mu? hata
- `version.major() > old_version.major()` veya `options_changed` → `index.reset()` + watermark sıfırla

### Unsafe Bloklar
Yok.

### Nova Shield Chain İçin Dersler
- `mpsc::channel(BATCH_LOOKAHEAD=2)` — backpressure mekanizması: Producer en fazla 2 batch ileri gidebilir
- Paralel index işleme: Her AppView bağımsız bir task'ta çalışır, JoinSet ile hata yönetimi
- Versiyon kontrolü: `option_hash` ekstra güvenlik; config değişikliğinde yanlışlıkla eski data üzerine yazma engellenir
- `--exit-on-catchup` flag'i batch job kullanım senaryosu için kritik (CI/CD ortamı)

---

## [indexing_state.rs] — cometindex/src/indexer/indexing_state.rs
**Dosya:** `/crates/util/cometindex/src/indexer/indexing_state.rs`
**Satır Sayısı:** 283

### Genel Amaç
`IndexingManager` — kaynak ve hedef PostgreSQL DB'leri yöneten, event batch'lerini çeken ve watermark'ları güncelleyen merkezi yönetici.

### Struct/Enum/Type Listesi

#### `Height` (Satır 12-13)
```rust
#[sqlx(transparent)]
pub struct Height(i64);
```
`#[derive(Default, PartialOrd, Ord, sqlx::Type)]`
- `post_genesis() -> Self` → `Self(1)`
- `advance(self, batch_size: u64, max_height: Self) -> (Self, Self)` — son height ve sonraki first height hesapla
  - `last = min(self + batch_size - 1, max_height)`, `next_first = last + 1`
- `next(self) -> Self` → `Self(self.0 + 1)`
- `From<u64>`: `try_into().unwrap_or(i64::MAX)` — taşma güvenliği
- `From<Height> for u64`: `try_into().unwrap_or_default()` — negatif → 0

#### `IndexState` (Satır 45-51)
```rust
pub struct IndexState {
    pub version: Version,
    pub height: Height,
}
```

#### `IndexingManager` (Satır 54-57)
```rust
pub struct IndexingManager {
    src: PgPool,  // okuma (read-only)
    dst: PgPool,  // yazma (read-write)
}
```

### Fonksiyonlar

#### `create_watermark_table` (Satır 60-76)
```sql
CREATE TABLE IF NOT EXISTS index_watermarks (
    index_name TEXT PRIMARY KEY,
    height BIGINT NOT NULL,
    version BIGINT
)
-- sonra:
ALTER TABLE index_watermarks ADD COLUMN IF NOT EXISTS option_hash BYTEA
```
Migration: `option_hash` sonradan eklenmiş, geriye dönük uyumlu.

#### `src_height` (Satır 79-85)
```sql
SELECT MAX(height) - 1 FROM blocks
```
Neden `-1`? Mevcut blok indexlenirken başka bir thread okuyabilir; son blok tamamlanmamış olabilir.

#### `events_between` (Satır 179-253) — Kritik SQL Sorgusu
```sql
WITH blocks AS (SELECT * FROM blocks WHERE height >= $1 AND height <= $2),
filtered_events AS (
  SELECT e.block_id, e.rowid, e.type, e.tx_id, b.height
  FROM events e JOIN blocks b ON e.block_id = b.rowid
),
events_with_attrs AS (
  SELECT f.*, jsonb_object_agg(a.key, a.value) AS attrs
  FROM filtered_events f LEFT JOIN attributes a ON f.rowid = a.event_id
  GROUP BY f.block_id, f.rowid, f.type, f.tx_id, f.height
)
SELECT e.rowid, e.type, e.height, tx.tx_hash, e.attrs
FROM events_with_attrs e LEFT JOIN tx_results tx ON tx.rowid = e.tx_id
ORDER BY e.height ASC, e.rowid ASC
```
**Performans notu (Satır 185-190):** CTE ile Postgres'i hash join + sort'tan caydırıp lineer event okuma zorlanıyor.

#### `update_index_state` — UPSERT (Satır 119-142)
```sql
INSERT INTO index_watermarks VALUES ($1, $2, $3, $4)
ON CONFLICT (index_name) DO UPDATE SET height = excluded.height, version = excluded.version, option_hash = excluded.option_hash
```

#### `event_batch` (Satır 255-268)
- `(first..=last)` aralığında `BlockEvents::new()` oluşturur
- `transactions_between` ve `events_between` stream'leri sıralı olarak okur
- `height.0 - first.0` index ile doğru block'a push eder

#### `init` (Satır 270-277)
- `tokio::try_join!(read_only_db(src_url), read_write_db(dst_url))` — paralel bağlanma
- `create_watermark_table` — idempotent

### Nova Shield Chain İçin Dersler
- `MAX(height) - 1` pattern: Son block henüz commit edilmemişse yarım data okumayı engeller — Nova Shield'de de uygulama gerekli
- CTE + ORDER BY pattern: PostgreSQL'de event sıralı okuma için hash join'i engelleyen CTE yapısı; büyük tablolarda kritik
- UPSERT watermark: `ON CONFLICT DO UPDATE` — at-least-once semantics sağlar; idempotent operasyonlar zorunlu
- `i64` için `try_into().unwrap_or(i64::MAX)` guard: Height overflowunu sessizce i64::MAX'a klamp eder

---

## [opt.rs] — cometindex/src/opt.rs
**Dosya:** `/crates/util/cometindex/src/opt.rs`
**Satır Sayısı:** 55

### Genel Amaç
CLI argüman parsing. `clap::Parser` derive macro ile.

### Struct

#### `Options` (Satır 13-49)
```rust
pub struct Options {
    pub src_database_url: String,    // -s/--src-database-url
    pub dst_database_url: String,    // -d/--dst-database-url
    pub chain_id: Option<String>,    // -c/--chain-id
    pub poll_ms: Duration,           // -p/--poll-ms, default=500ms
    pub genesis_json: PathBuf,       // -g/--genesis-json
    pub exit_on_catchup: bool,       // --exit-on-catchup (batch job flag)
    pub integrity_checks_only: bool, // --integrity-checks-only (sadece kontrol)
    pub new_options: bool,           // --new-options (config değişikliğine izin ver)
}
```

### `parse_poll_ms` (Satır 52-54)
```rust
fn parse_poll_ms(s: &str) -> Result<Duration> {
    s.parse().map(Duration::from_millis).map_err(Error::from)
}
```
String → milisaniye sayısı → `Duration`.

### Nova Shield Chain İçin Dersler
- `exit_on_catchup` flag: CI pipeline'da indexer'ı batch job olarak çalıştırıp tamamlandığında çıkmak için; Nova Shield monitoring tool'ları için aynı pattern kullanılabilir
- `integrity_checks_only` flag: Deploy öncesi hızlı doğrulama; DB tutarsızlığı tespiti için

---

## [integrity.rs] — cometindex/src/integrity.rs
**Dosya:** `/crates/util/cometindex/src/integrity.rs`
**Satır Sayısı:** 88

### Genel Amaç
PostgreSQL'deki CometBFT raw event veritabanında blok ve event bütünlüğünü kontrol eder. İki kontrol: eksik block height'ları, eksik event rowid'leri.

### Fonksiyonlar

#### `pub async fn missing_blocks(name: &'static str, db: PgPool) -> anyhow::Result<bool>` (Satır 5-34)
```sql
WITH height_gaps AS (
  SELECT height, LEAD(height) OVER (ORDER BY height) as next_height,
         LEAD(height) OVER (ORDER BY height) - height as gap
  FROM blocks
)
SELECT current_height, next_height
FROM height_gaps WHERE gap > 1 AND next_height IS NOT NULL
ORDER BY current_height ASC LIMIT 1
```
İlk boşluğu bulur ve raporlar. `LEAD()` window function ile ardışık height farkı.

#### `async fn missing_events(name: &'static str, db: PgPool) -> anyhow::Result<bool>` (Satır 36-66)
```sql
WITH event_heights AS (
  SELECT e.rowid, b.height,
         LEAD(e.rowid) OVER (PARTITION BY b.height ORDER BY e.rowid) AS next_rowid
  FROM events e JOIN blocks b ON e.block_id = b.rowid
)
SELECT height, rowid, next_rowid
FROM event_heights WHERE next_rowid IS NOT NULL AND next_rowid - rowid <> 1
ORDER BY height ASC, rowid ASC LIMIT 1
```
`PARTITION BY b.height` — her block içinde event rowid'lerin ardışık olup olmadığını kontrol eder.

#### `pub async fn integrity_check(src_database_url: &str) -> anyhow::Result<()>` (Satır 68-87)
- İki kontrolü paralel `tokio::spawn` ile çalıştırır
- Herhangi biri başarısız → `failed = true` → `bail!`

### Nova Shield Chain İçin Dersler
- PostgreSQL `LEAD()` window function ile gap detection: Nova Shield'de block indexer veritabanında eksik slot tespiti için kullanılabilir
- İki kontrolü paralel spawn etmek: Her check bağımsız → toplam süre max(t1, t2) olur
- `tokio::spawn` + `task.await??`: Çift soru işareti = JoinError + inner Error

---

## [contextualized.rs] — cometindex/src/contextualized.rs
**Dosya:** `/crates/util/cometindex/src/contextualized.rs`
**Satır Sayısı:** 30

### Genel Amaç
Event'e block ve transaction bağlamını ekleyen wrapper struct.

### Struct

#### `ContextualizedEvent<'block>` (Satır 4-13)
```rust
pub struct ContextualizedEvent<'block> {
    pub event: &'block Event,           // ABCI event referansı
    pub block_height: u64,
    pub tx: Option<([u8; 32], &'block [u8])>, // (tx_hash, tx_bytes) — opsiyonel
    pub local_rowid: i64,               // yerel DB rowid (farklı DB'lerde farklı olabilir)
}
```
Copy + Clone + Debug (çünkü referanslar).

**Metotlar:**
- `tx_hash() -> Option<[u8; 32]>` — `self.tx.map(|x| x.0)`
- `tx_data() -> Option<&'block [u8]>` — `self.tx.map(|x| x.1)`
- `AsRef<Event>` impl — event referansı erişimi

### Nova Shield Chain İçin Dersler
- `local_rowid` note: Farklı node'lardaki event DB'lerinde aynı event farklı rowid'e sahip olabilir; bu ID chain-global değil
- `tx: Option<...>` — block-level event'ler (tx'siz) ve tx-level event'ler aynı tip ile temsil edilir

---

## [lib.rs] — tendermint-proxy/src/lib.rs
**Dosya:** `/crates/util/tendermint-proxy/src/lib.rs`
**Satır Sayısı:** 29

### Genel Amaç
Penumbra'nın Tendermint/CometBFT RPC proxy servisi. `TendermintProxyService` gRPC trait'ini implement eden `TendermintProxy` struct'ını tanımlar.

### Struct

#### `TendermintProxy` (Satır 18-21)
```rust
pub struct TendermintProxy {
    tendermint_url: url::Url,  // upstream Tendermint node URL
}
```
Clone + Debug.
- `pub fn new(tendermint_url: url::Url) -> Self`
- `#![deny(clippy::unwrap_used)]` — tüm unwrap'lar yasak

---

## [tendermint_proxy.rs] — tendermint-proxy/src/tendermint_proxy.rs
**Dosya:** `/crates/util/tendermint-proxy/src/tendermint_proxy.rs`
**Satır Sayısı:** 212

### Genel Amaç
`TendermintProxyService` gRPC trait'inin `TendermintProxy` için implementasyonu. 6 RPC metot: get_tx, broadcast_tx_async, broadcast_tx_sync, get_status, abci_query, get_block_by_height.

### Fonksiyonlar (tamamı `#[instrument(level = "info", skip_all)]`)

#### `get_tx` (Satır 30-58)
- `HttpClient::new(url)` → `client.tx(hash, prove)` → `GetTxResponse::from`
- **Kritik kontrol (Satır 54-55):** `Transaction::decode(rsp.tx.as_ref())` — cevap gönderilmeden önce decode edilebilirlik kontrol edilir. Hata: `Status::unavailable`

#### `broadcast_tx_async` (Satır 66-86)
- `req_id` tracing span'e record edilir
- `client.broadcast_tx_async(params)` → `BroadcastTxAsyncResponse::from`

#### `broadcast_tx_sync` (Satır 89-115)
- `.tap_ok(|res| tracing::debug!("{:?}", res))` — `tap` crate kullanımı: Ok durumunda yan etki

#### `get_status` (Satır 118-136)
- `client.status()` → `GetStatusResponse::from`
- Comment (Satır 125): `HttpClient::new` generic bounds kötü tasarlanmış → URL String'e render edilip tekrar parse ediliyor

#### `abci_query` (Satır 139-177)
- Height parse: `height.try_into().map_err(|_| Status::invalid_argument("invalid height"))`
- `client.abci_query(Some(path), data, Some(height), prove)`
- Response kodu kontrolü: `Code::Ok` → geçer, `Code::Err(e)` → `Status::unavailable`

#### `get_block_by_height` (Satır 180-210)
- `height = tendermint::block::Height::try_from(height).expect("height should be less than 2^63")` — panic potansiyeli (TODO değil, intentional)
- Deserializasyon hatası: `Status::internal` (client hatası değil, sunucu iç hatası)

### Tasarım Kararları
- Her metot yeni `HttpClient` oluşturuyor — connection pooling yok. Performans dezavantajı.
- `Transaction::decode` validasyonu: get_tx'te dönen verinin decode edilebilirliğini kontrol et — defensive programming

### Nova Shield Chain İçin Dersler
- Her istek için yeni HTTP client oluşturma: Üretimde connection pool kullanılmalı (reqwest::Client ile pool)
- ABCI response code kontrolü: Nova Shield'de de gRPC yanıtlarında error code kontrolü zorunlu
- `tap_ok` pattern: Debug log için fonksiyonel chaining — hata yolunu kırlamadan yan etki

---

## [lib.rs] — tower-trace/src/lib.rs
**Dosya:** `/crates/util/tower-trace/src/lib.rs`
**Satır Sayısı:** 36

### Genel Amaç
`tracing-tower` crate'inin vendor'lanmış kopyası. Tower service'lere tracing instrumentation eklemek için middleware.

### Fonksiyonlar

#### `pub fn remote_addr<B>(req: &http::Request<B>) -> Option<SocketAddr>` (Satır 14-20)
- `req.extensions().get::<TcpConnectInfo>().and_then(|i| i.remote_addr())`
- Tonic'in server level'da remote addr alması için hack (tonic'in normal API'si tüm bunu vermiyor)

### Modüller
- `pub mod trace` — InstrumentableService, InstrumentMake trait'leri
- `pub mod v034`, `v037`, `v038` — Her Tendermint versiyonu için ayrı `RequestExt`

---

## [trace/mod.rs] — tower-trace/src/trace/mod.rs
**Dosya:** `/crates/util/tower-trace/src/trace/mod.rs`
**Satır Sayısı:** 92

### Genel Amaç
Tower service instrumentation traits ve type alias'ları.

### Type Alias
`pub type InstrumentedService<S, R> = service_span::Service<request_span::Service<S, R>>`
— İç içe iki wrapper: dıştaki service span, içteki request span.

### Trait'ler

#### `InstrumentableService<Request>` (Satır 10-39)
Tower `Service` için extension trait:
- `instrument<G>(self, svc_span: G) -> InstrumentedService<Self, Request>` — TRACE seviyesinde request span oluşturur
- `trace_requests<G>(self, get_span: G) -> request_span::Service<Self, Request, G>`
- `trace_service<G>(self, get_span: G) -> service_span::Service<Self>`

Blanket impl: `impl<S, R> InstrumentableService<R> for S where S: Service<R> + Sized`

#### `InstrumentMake<T, R>` (Satır 41-58)
`MakeService` için extension trait:
- `with_traced_service` / `with_traced_requests`

#### `GetSpan<T>` (Satır 64-87)
- `fn span_for(&self, target: &T) -> tracing::Span`
- Sealed trait pattern ile tip güvenliği
- İki impl: `Fn(&T) -> tracing::Span` ve `tracing::Span` (clone ile aynı span)

### Nova Shield Chain İçin Dersler
- Sealed trait pattern: Dışarıdan implement edilemeyen trait — API kısıtlaması için etkili
- `InstrumentedService` type alias: İç içe wrapper'ları okunabilir hale getirir
- `GetSpan` iki impl: Statik span (hep aynı) veya dinamik span (request'e göre) — iki kullanım senaryosu

---

## [trace/service_span.rs] — tower-trace/src/trace/service_span.rs
**Dosya:** `/crates/util/tower-trace/src/trace/service_span.rs`
**Satır Sayısı:** 245

### Genel Amaç
Service-level tracing middleware. Her request'te service span'e girer (`_enter = self.span.enter()`).

### Struct'lar
- `Service<S>` — inner + span
- `Layer<S, R, G>` — Service için layer factory
- `MakeService<M, T, R, G>` — MakeService wrapper
- `MakeFuture<F>` — pin_project! ile pinned future (`span: Option<tracing::Span>`)
- `MakeLayer<T, R, G>` — MakeService için layer

### Kritik Uygulama (Satır 215-232)
```rust
impl<S, R> tower_service::Service<R> for Service<S>
where S: tower_service::Service<R>
{
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<...> {
        let _enter = self.span.enter();  // span'e gir, guard drop'ta çıkar
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let _enter = self.span.enter();
        self.inner.call(request)  // future dönüşünde span'den çık
    }
}
```
**Not:** `call`'da span giriş/çıkışı synchronous — future instrument edilmiyor (bu `request_span` ile fark).

---

## [trace/request_span.rs] — tower-trace/src/trace/request_span.rs
**Dosya:** `/crates/util/tower-trace/src/trace/request_span.rs`
**Satır Sayısı:** 264

### Genel Amaç
Request-level tracing middleware. Her request için ayrı span oluşturulur ve future, span içinde instrument edilir.

### Kritik Uygulama (Satır 217-235)
```rust
fn call(&mut self, request: R) -> Self::Future {
    let span = self.get_span.span_for(&request);
    let _enter = span.enter();
    self.inner.call(request).instrument(span.clone())  // async future instrument
}
```
**Fark:** `service_span.rs`'de future instrument edilmiyor; burada `Instrument` ile future tüm async ömrü boyunca span içinde.

### `Future` tip: `tracing::instrument::Instrumented<S::Future>` (Satır 224)

---

## [v034/request_ext.rs], [v037/request_ext.rs], [v038/request_ext.rs]
**Dosyalar:** Her Tendermint versiyonu için
**Satır Sayısı:** ~123, ~128, ~125

### Genel Amaç
Her ABCI request tipine `create_span()` metodu ekleyen `RequestExt` trait. `ConsensusRequest`, `MempoolRequest`, `InfoRequest`, `SnapshotRequest`, `Request` için impl.

### Ortak Pattern
```rust
pub trait RequestExt {
    fn create_span(&self) -> tracing::Span;
}
// Tüm impl'ler error_span! macro kullanır (her zaman kaydedilmek için)
```

### v034 vs v037 Farkı (Satır 35-42 v037)
v037'de eklenen: `PrepareProposal`, `ProcessProposal` (ABCI++ UX)

### v037 vs v038 Farkı
v038'de:
- `BeginBlock` + `DeliverTx` + `EndBlock` → tek `FinalizeBlock` (ABCI++ 2.0)
- `ExtendVote`, `VerifyVoteExtension` eklendi (vote extension)

### Tasarım Kararları
- `error_span!` (TRACE değil, ERROR) — En yüksek seviye filter geçtiğinden her zaman kaydedilir
- `DeliverTx`'te `txid = ?hex::encode(Sha256::digest(tx))` — TX hash log'a yazılır
- `SetOption(_) => todo!("not implemented")` — v034'te: eski endpointler için

### Nova Shield Chain İçin Dersler
- ABCI versiyonlama: v034 → v037 (PrepareProposal/ProcessProposal) → v038 (FinalizeBlock, ExtendVote) geçişi; Nova Shield CometBFT versiyonunu seçerken ABCI++ desteği göz önünde bulundurulmalı
- `error_span!` tüm request'ler için: Production'da debug log seviyesi kapalı bile olsa request'ler görünür
- TX hash log: Her DeliverTx için SHA256 hash ile request korelasyonu

---

## BÖLÜM 2: crates/bench

### bench crate'i
Criterion benchmark framework kullanır. `src/lib.rs` neredeyse boş (sadece cfg_attr).

---

## [src/lib.rs] — bench/src/lib.rs
**Dosya:** `/crates/bench/src/lib.rs`
**Satır Sayısı:** 3

```rust
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
```
Nightly docs.rs özelliği. İçerik yok — sadece container crate.

---

## [benches/arkworks.rs] — bench/benches/arkworks.rs
**Dosya:** `/crates/bench/benches/arkworks.rs`
**Satır Sayısı:** 186

### Genel Amaç
BLS12-377 eğrisi üzerindeki alan (field) aritmetiğinin Arkworks implementasyonunu benchmark eder. Base field `Fq` (384-bit) ve scalar field `Fr` (256-bit) için altı işlem.

### Benchmark Senaryoları

#### Base Field (Fq — `ark_bls12_377::Fq = Fp384`)
| Fonksiyon | İşlem | İterasyon |
|---|---|---|
| `bench_base_field_addition` | Fq + Fq | 100.000 |
| `bench_base_field_subtraction` | Fq - Fq | 100.000 |
| `bench_base_field_mutliplication` | Fq * Fq | 100.000 |
| `bench_base_field_negation` | Fq + (-Fq) | 100.000 |
| `bench_base_field_square` | `Fp384::square(&x)` | 100.000 |
| `bench_base_field_inverse` | `Fp384::inverse(&x)` | 100.000 |

#### Scalar Field (Fr — `ark_bls12_377::Fr = Fp256`)
Aynı 6 işlem, `Fp256` üzerinde.

### Tasarım Kararları
- `black_box()` kullanımı: Derleyicinin loop'u optimize edip ezmesini engeller
- Her iterasyonda `x = black_box(x) + black_box(y)` — sonuç bir sonraki iterasyona taşınır (bağımlılık zinciri)
- `criterion_group!` + `criterion_main!` macro'ları — Criterion standart kurulum

### Benchmark Önemi
Bu benchmark, Nova Shield'in kullanacağı `arkworks` kütüphanesinin raw field arithmetic hızını ölçer. ZK proof maliyetinin büyük kısmı bu işlemlerden gelir.

### Nova Shield Chain İçin Dersler
- BLS12-377 base field işlemleri Nova Shield ZK kanıtlama süresini belirler
- `inverse` çok daha yavaş (genellikle 50-100x); ZK devresinde minimize edilmeli
- Fq (384-bit) Fr'den (256-bit) daha yavaş; scalar field işlemlerini tercih et
- Kendi benchmark suite'ini oluştururken: `black_box` + bağımlılık zinciri + criterion

---

## [benches/decaf377.rs] — bench/benches/decaf377.rs
**Dosya:** `/crates/bench/benches/decaf377.rs`
**Satır Sayısı:** 186

### Genel Amaç
Penumbra'nın kullandığı `decaf377` eğrisi üzerinde alan aritmetiğini benchmark eder. `Fp` (base field, BLS12-377 üzerinde) ve `Fq` (scalar field) için altı işlem.

### Arkworks ile Fark
- `decaf377::Fp` — `Fp384::square` yerine `.square()` metotu
- `decaf377::Fq` — `Fp256::inverse` yerine `.inverse()` metotu
- Method çağrısı farkı: Trait static vs instance method

### Benchmark Önemi
Decaf377, Penumbra'nın kriptografik temelini oluşturur. Arkworks ile performans karşılaştırması bu benchmark'tan çıkarılabilir.

### Nova Shield Chain İçin Dersler
- Nova Shield decaf377 kullanıyorsa arkworks'ten daha iyi mi kötü mü olduğunu test et
- Eğri seçimi benchmark sonuçlarına göre yapılmalı; sadece "önerilen kütüphane" yeterli değil

---

## [benches/nullifier_derivation.rs] — bench/benches/nullifier_derivation.rs
**Dosya:** `/crates/bench/benches/nullifier_derivation.rs`
**Satır Sayısı:** 76

### Genel Amaç
`NullifierDerivationProof` ZK kanıtı üretme süresi benchmark. Ayrıca devre kısıt sayısını yazdırır.

### Test Kurulumu
1. `SeedPhrase::generate(OsRng)` → `SpendKey` → `FullViewingKey` → `IncomingViewingKey` → payment address
2. `Note::from_parts(address, "1upenumbra", Rseed([1u8; 32]))`
3. `tct::Tree::new()` → note commitment ekle → `state_commitment_proof` al
4. `NullifierDerivationProofPublic { position, note_commitment, nullifier }`
5. `NullifierDerivationProofPrivate { nk: nullifier_key }`

### Benchmark
```rust
c.bench_function("nullifier derivation proving", |b| {
    b.iter(|| prove(public.clone(), private.clone()))
});
```
Ardından constraint sayısı: `cs.num_constraints()` ile print edilir.

### Kanıt Üretme
`NullifierDerivationProof::prove(&mut OsRng, &NULLIFIER_DERIVATION_PROOF_PROVING_KEY, public, private)`
OsRng doğrudan kullanılıyor (r/s blinding değil) — arkworks Groth16 prover.

### Nova Shield Chain İçin Dersler
- Nullifier derivation ZK kanıtı: Nova Shield UTXO modelinde her harcama için nullifier üretilir; kanıt üretim süresi kritik
- `cs.num_constraints()` print: Devrenin karmaşıklığını bilmek optimize etmek için başlangıç noktası

---

## [benches/output.rs] — bench/benches/output.rs
**Dosya:** `/crates/bench/benches/output.rs`
**Satır Sayısı:** 76

### Genel Amaç
`OutputProof` ZK kanıtı üretme benchmark. Note output (değer gönderme) işleminin kanıtlama süresini ölçer.

### Test Kurulumu
```rust
Note::from_parts(address, "1upenumbra", Rseed([1u8; 32]))
let balance_blinding = Fr::from(1u32);
let balance_commitment = (-Balance::from(value_to_send)).commit(balance_blinding);
// Public: balance_commitment + note_commitment
// Private: note + balance_blinding
```

### Kritik Detay
- `(-Balance::from(value)).commit(blinding)` — negatif bakiye commitment: Output kanıtı göndericinin bakiyesini azalttığını ZK ile kanıtlar
- `r = Fq::rand(&mut OsRng)`, `s = Fq::rand(&mut OsRng)` — Groth16 blinding faktörleri

### Nova Shield Chain İçin Dersler
- Balance commitment homomorphic: `(-value).commit(r)` output için, `(+value).commit(r)` input için — toplamları sıfıra eşit olmalı
- ZK output kanıtı: Nova Shield shielded transfer'da her output için ayrı kanıt üretilecek; batch proving düşünülmeli

---

## [benches/convert.rs] — bench/benches/convert.rs
**Dosya:** `/crates/bench/benches/convert.rs`
**Satır Sayısı:** 75

### Genel Amaç
`ConvertProof` ZK kanıtı benchmark. Staking token'larının farklı formlar arasında dönüştürülmesini kanıtlar (örn. unbonding token).

### Test Kurulumu
```rust
let amount = Amount::from(1u64);
let rate = U128x128::from(1u64);  // 1:1 dönüşüm oranı
// from ve to: her ikisi de STAKING_TOKEN_ASSET_ID (trivial örnek)
let balance = Balance::from(Value { asset_id: to, amount })
            - Balance::from(Value { asset_id: from, amount });
```

### Kritik Tip: `U128x128`
128.128 fixed-point sayı — staking oranları için. Nova Shield'de staking reward hesaplarında benzer tip gerekli.

### Nova Shield Chain İçin Dersler
- `ConvertProof`: Staking reward oranlarını ZK ile kanıtlamak Nova Shield staking mekanizması için örnek alınabilir
- `U128x128` fixed-point: overflow/underflow olmadan kesirli oranlar için

---

## [benches/swap.rs] — bench/benches/swap.rs
**Dosya:** `/crates/bench/benches/swap.rs`
**Satır Sayısı:** 103

### Genel Amaç
`SwapProof` ZK kanıtı benchmark. DEX swap işleminin kanıtlanmasını ölçer.

### Test Kurulumu
```rust
let trading_pair = TradingPair::new(gm.id(), gn.id());
let delta_1 = Amount::from(1u64);   // gm miktarı
let delta_2 = Amount::from(0u64);   // gn miktarı (tek yönlü swap)
let fee = Fee::default();

let swap_plaintext = SwapPlaintext::new(&mut OsRng, trading_pair, delta_1, delta_2, fee, claim_address);
let fee_blinding = Fr::from(0u64);
let fee_commitment = swap_plaintext.claim_fee.commit(fee_blinding);
let swap_commitment = swap_plaintext.swap_commitment();

// Balance: - value_1 - value_2 - value_fee (all outflows)
let balance_commitment = balance.commit(Fr::from(0u64));

// Public: balance_commitment + swap_commitment + fee_commitment
// Private: fee_blinding + swap_plaintext
```

### DEX Uygulama Notu
Swap ZK kanıtı şunu kanıtlar: "Ben X miktarda token A ile Y miktarda token B talep etmek istiyorum, swap commitment'ım doğru, fee'yi ödedim." Batch execution'da tüm swap plaintext'leri açığa çıkmaz — sadece commitment public olur.

### Nova Shield Chain İçin Dersler
- Swap commitment: Nova Shield DEX'te her order bir ZK commitment — MEV koruması için kritik
- `SwapProof` public input'ları: `balance_commitment + swap_commitment + fee_commitment` — settlement sırasında doğrulama
- fee_blinding = 0 (test): Üretimde non-zero olmalı
- `TradingPair` ile tam token pair tipi: Token sıralı

---

## [benches/swap_claim.rs] — bench/benches/swap_claim.rs
**Dosya:** `/crates/bench/benches/swap_claim.rs`
**Satır Sayısı:** 126

### Genel Amaç
`SwapClaimProof` ZK kanıtı benchmark. Batch swap'ın ardından output'ların talep edilmesi için kanıt.

### Test Kurulumu
```rust
let output_data = BatchSwapOutputData {
    delta_1: Amount::from(1000u64),     // toplam gm girişi
    delta_2: Amount::from(1000u64),     // toplam gn girişi
    lambda_1: Amount::from(50u64),      // gm çıkışı
    lambda_2: Amount::from(25u64),      // gn çıkışı
    unfilled_1: Amount::from(23u64),    // doldurulamayan gm
    unfilled_2: Amount::from(50u64),    // doldurulamayan gn
    height: height.into(),
    trading_pair: swap_plaintext.trading_pair,
    sct_position_prefix: position,
};
let (lambda_1, lambda_2) = output_data.pro_rata_outputs((delta_1_i, delta_2_i));
```

### DEX Uygulama Notu — KRITIK
`BatchSwapOutputData.pro_rata_outputs((delta_1_i, delta_2_i))` — Pro-rata hesaplama:
- Kullanıcının katkısı / toplam katkı oranında output alınır
- Doldurulmayan kısım geri iade edilir
- Bu hesaplama ZK devresi içinde yapılır: kanıt üretmek = claim yetkisini kanıtlamak

`SwapClaimProofPrivate` şunları içerir:
- `swap_plaintext` — orijinal swap detayları
- `state_commitment_proof` — SCT Merkle kanıtı
- `ak, nk` — anahtar materyaller
- `lambda_1, lambda_2` — beklenen output miktarları
- `note_blinding_1, note_blinding_2` — output note gizlemesi

### Nova Shield Chain İçin Dersler
- Pro-rata batch execution: DEX implement ederken aynı blokta tüm swaplar toplanıp bir batch olarak işlenir; her kullanıcı orantılı çıktı alır (MEV-resistant)
- SwapClaim'deki SCT proof: Swap commitment SCT'deyken claim yapılabilir — commitment tree zorunluluğu
- Output note blinding: Her çıkış notu ayrı blinding faktörüyle gizlenir

---

## [benches/spend.rs] — bench/benches/spend.rs
**Dosya:** `/crates/bench/benches/spend.rs`
**Satır Sayısı:** 89

### Genel Amaç
`SpendProof` ZK kanıtı benchmark. UTXO harcama kanıtının üretim süresini ölçer.

### Test Kurulumu
```rust
let note = Note::generate(&mut OsRng, &sender, value_to_send);
let note_commitment = note.commit();
let rsk = sk_sender.spend_auth_key().randomize(&spend_auth_randomizer);
let nf = Nullifier::derive(&nk, state_commitment_proof.position(), &note_commitment);

// Public: anchor + balance_commitment + nullifier + rk
// Private: state_commitment_proof + note + v_blinding + spend_auth_randomizer + ak + nk
```

### SpendProof Public Input'ları
- `anchor` — SCT root (hangi state tree'ye ait?)
- `balance_commitment` — ne kadar harcandı (gizli)
- `nullifier` — hangi note harcandı (tekrar kullanım engeli)
- `rk` — randomized spend auth verification key

### SpendProof Private Witness'ları
- `state_commitment_proof` — Merkle inclusion kanıtı
- `note` — note içeriği (plaintext)
- `v_blinding` — bakiye commitment randomness
- `spend_auth_randomizer + ak + nk` — kimlik kanıtı

### Nova Shield Chain İçin Dersler
- Spend ZK kanıtı Nova Shield'in temel işlem kanıtıdır: Bir UTXO'yu harcamak için SCT inclusion + nullifier + balance commitment birlikte kanıtlanır
- Randomized spend auth key: Her harcama farklı `rsk = sk.randomize(r)` kullanır — key reuse'u engeller ve unlinkability sağlar
- Nullifier derivation: `Nullifier::derive(nk, position, commitment)` — nullifier = f(nk, position, commitment); position bilgisi olmadan nullifier tahmin edilemez

---

## [benches/delegator_vote.rs] — bench/benches/delegator_vote.rs
**Dosya:** `/crates/bench/benches/delegator_vote.rs`
**Satır Sayısı:** 98

### Genel Amaç
`DelegatorVoteProof` ZK kanıtı benchmark. Governance'da gizli oy kullanma kanıtının süresini ölçer.

### Test Kurulumu
SpendProof ile çok benzer ama `start_position` eklenir:
```rust
sct.end_epoch().unwrap(); // oylama epoch'u bitir
let first_note_commitment = Note::generate(...).commit();
sct.insert(tct::Witness::Keep, first_note_commitment).unwrap();
let start_position = sct.witness(first_note_commitment).unwrap().position();
```

### DelegatorVoteProofPublic
```rust
DelegatorVoteProofPublic {
    anchor, balance_commitment, nullifier, rk,
    start_position,  // SpendProof'tan farklı: oy verilen epoch'un başlangıç pozisyonu
}
```

### Nova Shield Chain İçin Dersler
- Gizli oy: ZK kanıt ile oy kullanan kişinin stake miktarı ve adresi gizli kalır; sadece "bu kişinin oylama anında stake'i vardı" kanıtlanır
- `start_position`: Oy verme dönemi başlamadan önce edinilen stake geçerlidir — snapshot-based voting

---

## BÖLÜM 3: crates/test

---

## [mock-client/src/lib.rs]
**Dosya:** `/crates/test/mock-client/src/lib.rs`
**Satır Sayısı:** 296

### Genel Amaç
Integration test için minimal wallet client. Chain state'i okuyarak note'ları, swap'ları ve nullifier'ları takip eder. ZK kanıt üretimi için witness ve authorization data üretir.

### Struct

#### `MockClient` (Satır 17-28)
```rust
pub struct MockClient {
    latest_height: u64,
    sk: SpendKey,
    pub fvk: FullViewingKey,
    pub notes: BTreeMap<note::StateCommitment, Note>,
    pub nullifiers: BTreeMap<note::StateCommitment, Nullifier>,
    pub spent_notes: BTreeMap<note::StateCommitment, ()>,
    swaps: BTreeMap<tct::StateCommitment, SwapPlaintext>,
    pub sct: penumbra_sdk_tct::Tree,
}
```

### Fonksiyonlar

#### `pub fn new(sk: SpendKey) -> MockClient` (Satır 31)
- `latest_height: u64::MAX` — ilk sync'de `wrapping_add(1) = 0` olacak şekilde başlangıç

#### `pub async fn sync_to<R: StateRead>(&mut self, target_height: u64, state: R)` (Satır 70-97)
- `start_height = self.latest_height.wrapping_add(1)` — overflow safe
- Her height için compact block okuyup scan eder
- SCT root doğrulaması: `state.get_anchor_by_height(height)` ile karşılaştırma

#### `pub fn scan_block(&mut self, block: CompactBlock) -> anyhow::Result<()>` (Satır 99-199)
Üç payload türü:
1. `StatePayload::Note` — `trial_decrypt(&fvk)`:
   - Başarılı: SCT'ye `Keep` ile ekle, note + nullifier kaydet
   - Başarısız: SCT'ye `Forget` ile ekle (hash sakla, içerik değil)
2. `StatePayload::Swap` — `trial_decrypt(&fvk)`:
   - Başarılı: swap plaintext kaydet, output note'ları pre-compute et (`output_1`, `output_2`)
   - `batch_data` = block.swap_outputs[trading_pair] — batch execution sonuçları
3. `StatePayload::RolledUp` — commitment zaten var mı? → Keep, yok → Forget

Nullifier harcama tespiti (Satır 175-189): Linear scan `self.nullifiers.values().any(|n| *n == nullifier)` — küçük set için OK

#### `pub fn nullifier(&self, commitment: note::StateCommitment) -> Option<Nullifier>` (Satır 220-229)
```rust
Nullifier::derive(&nk, position.unwrap(), &commitment)
```

#### `pub fn witness_plan(&self, plan: &TransactionPlan) -> Result<WitnessData, Error>` (Satır 238-260)
- SpendPlan ve SwapClaimPlan için commitment listesi
- SCT'den her commitment için Merkle kanıtı al
- `WitnessData { anchor: sct.root(), state_commitment_proofs: Map<Commitment, Proof> }`

#### `pub async fn witness_auth_build(&self, plan: &TransactionPlan) -> Result<Transaction, Error>` (Satır 266-272)
- `witness_plan` + `authorize_plan` → `plan.build_concurrent(fvk, witness_data, auth_data)`

#### `pub fn spendable_notes_by_asset(&self, asset_id) -> impl Iterator<Item = &Note>` (Satır 287-294)
- Harcanmamış (`!spent_note`) ve doğru asset'e sahip note'lar

### Nova Shield Chain İçin Dersler
- `trial_decrypt`: Her incoming payload için FVK ile deneme şifre çözme — ECDH + AES işlemi; O(n) per block scan
- `wrapping_add(1)` başlangıç trick'i: `u64::MAX + 1 = 0` — ilk bloktan tarama başlar
- `pre-compute output notes`: Swap claim'de output note'ları önceden hesaplayıp map'e kaydet — scan sırasında output bulunabilsin
- SCT root doğrulama: Her blok sonrası client SCT root'u chain ile karşılaştırır — bütünlük garantisi

---

## [mock-consensus/src/lib.rs]
**Dosya:** `/crates/test/mock-consensus/src/lib.rs`
**Satır Sayısı:** 214

### Genel Amaç
Test için mock consensus engine. CometBFT/Tendermint'in yerini tutar; integration testleri wall-clock time beklemeden hızlı çalıştırır.

### Struct

#### `TestNode<C>` (Satır 78-103)
```rust
pub struct TestNode<C> {
    consensus: C,               // Tower Service (consensus)
    last_app_hash: Vec<u8>,
    last_validator_set_hash: Option<tendermint::Hash>,
    last_commit: Option<tendermint::block::Commit>,
    consensus_params_hash: Vec<u8>,
    height: tendermint::block::Height,
    keyring: Keyring,           // BTreeMap<VerificationKey, SigningKey>
    on_block: Option<OnBlockFn>,// Box<dyn FnMut(tendermint::Block) + Send + Sync>
    ts_callback: TsCallbackFn,  // Box<dyn Fn(Time) -> Time + Send + Sync>
    timestamp: Time,
    chain_id: tendermint::chain::Id,
}
```

#### Type Alias'lar
- `OnBlockFn = Box<dyn FnMut(tendermint::Block) + Send + Sync + 'static>`
- `TsCallbackFn = Box<dyn Fn(Time) -> Time + Send + Sync + 'static>`
- `Keyring = BTreeMap<VerificationKey, SigningKey>`

### Sabitler
- `CHAIN_ID: &'static str = "penumbra-test-chain"`

### `fast_forward` (Satır 185-201)
```rust
pub async fn fast_forward(&mut self, blocks: u64) -> anyhow::Result<()>
```
N tane boş block execute eder. Test'lerde "N block ileri git" komutu.

### Nova Shield Chain İçin Dersler
- `ts_callback`: Zaman simülasyonu için callback; staking/unbonding gibi zaman bağımlı senaryolar test edilebilir
- `on_block`: Her block'ta callback → event monitoring test'leri
- `fast_forward(N)` primitive: Nova Shield integration test suite'inde mutlaka bulunmalı
- `BTreeMap<VerificationKey, SigningKey>`: Deterministik sıralama — keyring her çalışmada aynı sıralamayı üretir

---

## [mock-consensus/src/abci.rs]
**Dosya:** `/crates/test/mock-consensus/src/abci.rs`
**Satır Sayısı:** 173

### Genel Amaç
ABCI request/response döngüsü: `begin_block → deliver_tx (x N) → end_block → commit`.

### Fonksiyonlar

#### `begin_block(header, last_commit_info) -> Result<BeginBlock>` (Satır 41-70)
- `ConsensusRequest::BeginBlock { hash: Hash::None, header, last_commit_info, byzantine_validators: Default }` → service

#### `deliver_tx(tx: Bytes) -> Result<DeliverTx>` (Satır 74-106)
- `ConsensusRequest::DeliverTx { tx }` → service
- Response: code, gas_used, gas_wanted, events trace log

#### `end_block() -> Result<EndBlock>` (Satır 109-144)
- Height `i64`'e dönüştürülür
- Response: validator_updates, consensus_param_updates, events

#### `commit() -> Result<Commit>` (Satır 147-171)
- Response: data (yeni app_hash), retain_height

### Nova Shield Chain İçin Dersler
- ABCI döngüsü sırası: BeginBlock → N×DeliverTx → EndBlock → Commit — bu sıra değiştirilemez
- Her adım ayrı async fonksiyon: Testlerde istenen noktada durabilme esnekliği
- `byzantine_validators: Default::default()` — mock'ta Byzantine evidence yok; slashing testleri özel kurulum gerektiriyor

---

## [mock-consensus/src/block.rs]
**Dosya:** `/crates/test/mock-consensus/src/block.rs`
**Satır Sayısı:** 374

### Genel Amaç
Block builder ve execution. `TestNode::block()` ile `Builder` alınır, `.execute()` ile block execute edilir.

### Struct'lar
#### `Builder<'e, C>` (Satır 35-46)
```rust
pub struct Builder<'e, C> {
    test_node: &'e mut TestNode<C>,
    data: Vec<Vec<u8>>,       // transaction bytes
    evidence: evidence::List,
    timestamp: Time,
    disable_signatures: bool,
}
```

#### `EndBlockEvents(pub Vec<Event>)` / `DeliverTxEvents(pub Vec<Event>)` (Satır 353-373)
Newtype wrapper, `Deref<Target = Vec<Event>>`.

### Fonksiyonlar

#### `Builder::execute(self) -> Result<(EndBlockEvents, DeliverTxEvents)>` (Satır 119-176)
Akış:
1. `self.finish()` → `(test_node, block)` üret
2. `begin_block(header, last_commit_info)`
3. Her tx için `deliver_tx(tx)`
4. `end_block()` → events
5. `commit()` → `test_node.last_app_hash = commit_response.data`
6. `on_block` callback çağır

#### `Builder::finish(self) -> Result<(&'e mut TestNode<C>, Block)>` (Satır 184-311)
1. `ts_callback` ile timestamp ilerlet
2. Height increment
3. `last_commit` al
4. Proposer: `Sha256(pk)[0..20]` → AccountId
5. `validators_hash` kullan
6. `data_hash = Sha256(data.concat())` — concat hash! (her tx'in birleşimi)
7. Header oluştur (tüm field'lar doldurulur)
8. Signatures: `generate_signatures(&header)` veya boş
9. `Block::new(header, data, evidence, last_commit)`
10. `test_node.last_commit = Some(Commit { height, round=0, block_id, signatures })`

#### `CommitHashingExt::hash` (Satır 321-351)
Commit'in hash'ini hesaplar:
```rust
signatures.iter()
    .map(|s| CommitSig::from(s).encode_to_vec())
    .collect()
// boş: empty SHA256 hash = E3B0C44...
// dolu: simple_hash_from_byte_vectors::<Sha256>(&bs)
```

### Nova Shield Chain İçin Dersler
- `data_hash = Sha256(data.concat())`: Dikkat — bu tüm tx'lerin birleşiminin hash'i; standart Tendermint Merkle tree değil. Test amaçlı kabul edilebilir, production'da Merkle hash lazım
- Empty hash sabit: `E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855` — SHA256("")
- `last_commit = None` genesis için: İlk block'ta commit yok

---

## [mock-consensus/src/builder.rs]
**Dosya:** `/crates/test/mock-consensus/src/builder.rs`
**Satır Sayısı:** 197

### Genel Amaç
`TestNode` oluşturma builder pattern. Validator sayısı, app_state, timestamp callback, chain_id gibi parametreler.

### Fonksiyonlar
- `single_validator()` — tek validator keyring oluştur
- `two_validators()` — iki validator keyring oluştur
- `with_keys(keys)` — önceden belirlenmiş key'ler
- `with_tendermint_genesis(genesis)` — hardcoded genesis (CometBFT uyumluluk testi için)
- Default timestamp callback: `t + 5 saniye` (const olamıyor — `Duration::from_secs(5)` const fn değil)
- `app_state(bytes)` — genesis app state

### Nova Shield Chain İçin Dersler
- `single_validator()` vs `two_validators()`: Çoğu test tek validator yeterli; equivocation/slashing testi için iki gerekli
- `default_ts_callback`: 5 saniyelik blok zamanı simülasyonu — Nova Shield'de blok süresini simulate etmek için callback özelleştirilebilir

---

## [mock-consensus/src/builder/init_chain.rs]
**Dosya:** `/crates/test/mock-consensus/src/builder/init_chain.rs`
**Satır Sayısı:** 235

### Genel Amaç
`Builder::init_chain<C>(consensus) -> Result<TestNode<C>>` — genesis kurulumu ve TestNode oluşturma.

### `consensus_params` (Satır 213-233)
```rust
consensus::Params {
    block: Size { max_bytes: 1, max_gas: 1, time_iota_ms: 1 },
    evidence: Params { max_age_num_blocks: 1, max_age_duration: 1s, max_bytes: 1 },
    validator: ValidatorParams { pub_key_types: vec![] },
    version: Some(VersionParams { app: 1 }),
    abci: AbciParams { vote_extensions_enable_height: None },
}
```
**Notlar:** Tüm değerler minimal (`max_bytes=1` — teste özel). Production için bunlar çok farklı.

### Validator Power (Satır 150-166)
```rust
Power::try_from(25_000 * 10i64.pow(6))?  // 25.000 * 10^6 = 25 milyar
```
Test validator'ına hardcoded voting power.

### Nova Shield Chain İçin Dersler
- Genesis'te `consensus_params` production değerlerinden çok farklı: max_bytes=1 gibi minimal değerler geçici. Nova Shield testlerinde gerçekçi değerler kullanılmalı
- Validator power: Test'lerde sabit değer; production'da dinamik set edilmeli

---

## [mock-consensus/src/block/signature.rs]
**Dosya:** `/crates/test/mock-consensus/src/block/signature.rs`
**Satır Sayısı:** 160

### Genel Amaç
Ed25519 commit signature üretimi ve VoteInfo dönüştürme.

### `sign::commit` (Satır 20-48)
```rust
let sign_bytes = tendermint_proto::Protobuf::<RawCanonicalVote>::encode_length_delimited_vec(canonical.clone());
let signature = validator_key.sign(sign_bytes.as_slice()).try_into().unwrap();
CommitSig::BlockIdFlagCommit { validator_address, timestamp, signature: Some(signature.into()) }
```

### `sign::nil` (Satır 54-60)
Validator oy kullanmadığında: `CommitSig::BlockIdFlagNil { ..., signature: None }`

### `generate_signatures` (Satır 72-107)
```rust
let canonical = tendermint::vote::CanonicalVote {
    vote_type: Type::Precommit,
    height, round: 0u8.into(),
    block_id: Some(block_id),
    timestamp: Some(header.time.clone()),
    chain_id: self.chain_id.clone(),
};
// Her validator için: SHA256(vk)[0..20] → AccountId → sign::commit
```

### `last_commit_info` (Satır 114-130)
CommitSig → VoteInfo dönüşümü:
```rust
BlockSignatureInfo::Flag(match commit_sig {
    BlockIdFlagAbsent => BlockIdFlag::Absent,
    BlockIdFlagCommit => BlockIdFlag::Commit,
    BlockIdFlagNil => BlockIdFlag::Nil,
})
```
`Power::from(1_u8)` — hardcoded voting power 1 (test)

### Nova Shield Chain İçin Dersler
- `round: 0u8.into()` — mock her zaman round 0'da uzlaşı sağlar; gerçekte BFT retry gerekebilir
- Canonical vote protobuf encoding: `encode_length_delimited_vec` — uzunluk prefixli encoding
- `sha2::Sha256::digest(vk)[0..20]` → AccountId: Tendermint validator adresi = pubkey SHA256'sının ilk 20 byte

---

## [mock-tendermint-proxy/src/lib.rs]
**Dosya:** `/crates/test/mock-tendermint-proxy/src/lib.rs`
**Satır Sayısı:** 7

Re-export: `TestNodeProxy` ve `StubProxy`.

---

## [mock-tendermint-proxy/src/stub.rs]
**Dosya:** `/crates/test/mock-tendermint-proxy/src/stub.rs`
**Satır Sayısı:** 78

### Genel Amaç
Tüm RPC çağrılarına `Status::unimplemented` döndüren stub proxy. Test'lerde Tendermint proxy gerektirmeyen senaryolar için.

### Struct: `StubProxy`
`TendermintProxyService` implementation: 6 metot, hepsi `Err(Status::unimplemented("method_name"))`.

### Nova Shield Chain İçin Dersler
- Stub pattern: Tüm metodları unimplemented bırakan stub, yalnızca kullanılan metodları implement eden testler için temel sağlar
- Test'lerde proxy gerekmeyen durumlarda StubProxy kullan; gerektiğinde TestNodeProxy'ye geç

---

## [mock-tendermint-proxy/src/proxy.rs]
**Dosya:** `/crates/test/mock-tendermint-proxy/src/proxy.rs`
**Satır Sayısı:** 241

### Genel Amaç
Test node'unun ürettiği block'ları takip eden ve `get_status` + `get_block_by_height` implementasyonu sağlayan test proxy.

### Struct'lar
#### `TestNodeProxy` (Satır 30-32)
```rust
pub struct TestNodeProxy {
    inner: Arc<Inner>,
}
```
#### `Inner` (Satır 34-38)
```rust
struct Inner {
    blocks: RwLock<BTreeMap<Height, Block>>,
}
```

### Kritik Metotlar

#### `on_block_callback(&self) -> OnBlockFn` (Satır 47-53)
`Box<dyn FnMut(Block) + Send + Sync>` döndürür. `TestNode::on_block(callback)` ile bağlanır.

#### `get_status` (Satır 162-197)
- `last_block_height()` + `timestamp()` → `SyncInfo { catching_up: false, ... }`
- Test node tek node → hiç "catching_up" değil

#### `get_block_by_height` (Satır 207-239)
- `self.inner.blocks().get(&height)` — BTreeMap lookup
- `penumbra_sdk_proto::tendermint::types::Block::try_from(block)` dönüşümü

### `Inner::blocks` / `blocks_mut`
`RwLock` guards — panic yerine `.expect("block lock should never be poisoned")`.

### `on_block` (Satır 78-91)
Duplicate height → `panic!("proxy received two blocks with height {height}")` — test güvenliği.

### Nova Shield Chain İçin Dersler
- `Arc<RwLock<BTreeMap>>` pattern: Thread-safe shared mutable state; multiple reader veya single writer
- `on_block` callback ile TestNode bağlama: MockClient + MockConsensus + TestNodeProxy trinity — entegrasyon test setup'ı
- Duplicate height panic: Test'lerde kesinlikle hata durumunu yakala, sessizce görmezden gelme

---

## [tct-property-test/src/lib.rs]
**Dosya:** `/crates/test/tct-property-test/src/lib.rs`
**Satır Sayısı:** 5
Boş crate container. Sadece doc_auto_cfg feature.

---

## [tct-property-test/tests/serialize.rs]
**Dosya:** `/crates/test/tct-property-test/tests/serialize.rs`
**Satır Sayısı:** 119

### Genel Amaç
`penumbra_sdk_tct::Tree`'nin artımlı serileştirme doğruluğunu proptest ile kontrol eder.

### Sabitler
- `MAX_USED_COMMITMENTS: usize = 3` — pool'daki commitment sayısı
- `MAX_TIER_ACTIONS: usize = 10` — action sekansı uzunluğu

### Enum: `Action`
```rust
enum Action {
    Serialize,
    EvaluateRoot,
    EndEpoch,
    EndBlock,
    Insert(Witness, StateCommitment),
    Forget(StateCommitment),
}
```

### Test: `incremental_serialize`
1. Rastgele commitment listesi oluştur
2. Rastgele action sekansı oluştur (son action her zaman Serialize)
3. Her action'ı uygula
4. Depolama'dan deserialize et
5. **Assertion 1:** `tree == deserialized` — artımlı serileştirme sonrası eşitlik
6. **Assertion 2:** `incremental == non_incremental` — artımlı = tek seferde serileştirme
7. 4 validasyon: `validate::index`, `all_proofs`, `cached_hashes`, `forgotten`

### sparse vs dense InMemory
`sparse = any::<bool>()` — iki storage modu test edilir.

### Nova Shield Chain İçin Dersler
- TCT artımlı serileştirme: Nova Shield SCT'si sıfırdan serialize etmek yerine sadece değişen node'ları yazabilmeli
- Proptest ile state machine testi: Rastgele action sekanslarıyla determinism ve doğruluk testi
- Dört validasyon kombine: Index, tüm kanıtlar, cache hash, forgotten count

---

## [tct-property-test/tests/witness.rs]
**Dosya:** `/crates/test/tct-property-test/tests/witness.rs`
**Satır Sayısı:** 211

### Genel Amaç
TCT (Tiered Commitment Tree) witness (Merkle kanıt) üretimi ve doğruluğunun proptest ile sistematik kontrolü.

### 5 Property Test

#### `index_correct` (Satır 88-124)
Eklenip sonra silinmemiş her commitment için witness alınabilir ve `proof.verify(root)` geçer.

#### `validate_index` (Satır 126-139)
Her action sekansı sonrasında `validate::index(&tree)` geçmeli.

#### `verify_all_proofs` (Satır 141-154)
`validate::all_proofs(&tree)` — tüm Keep witness'lı commitment'ların kanıtları geçerli.

#### `validate_cached_hashes` (Satır 156-169)
`validate::cached_hashes(&tree)` — önbelleğe alınmış hash değerleri tutarlı.

#### `validate_forgotten` (Satır 171-209)
`forgotten()` sayacı her `Forget` veya duplicate `Insert(Keep, ...)` ile tam bir artmalı.

### Kritik Assertion (witness.rs Satır 50-56)
```rust
let old_position = predicted_position.unwrap();
let new_position = tree.position().unwrap();
assert_eq!(new_position.epoch(), old_position.epoch());
assert_eq!(new_position.block(), old_position.block());
assert_eq!(new_position.commitment(), old_position.commitment() + 1);
```
Insert sonrası sadece commitment index +1 olur; block ve epoch değişmez.

### Nova Shield Chain İçin Dersler
- `predict_position` → `insert` → doğrula pattern: Nova Shield SCT'de position öngörülmeli; bu test prediction mekanizmasının doğruluğunu kontrol ediyor
- Ayrı sayaç tipi `forgotten()`: Forget işlemlerini saymak için monoton sayaç; rollup/snapshot testlerinde

---

## [tct-property-test/tests/spec.rs]
**Dosya:** `/crates/test/tct-property-test/tests/spec.rs`
**Satır Sayısı:** 74

### Genel Amaç
Spec (referans implementasyon) vs gerçek implementasyon karşılaştırma testi altyapısı. Testler şu an COMMENT'te — `/* ... */` içinde.

Macro `spec_vs_impl!`:
- 10.000 case
- İki paralel simülasyon: `spec` ve `real`
- Action + Observation aynı anda uygulanır
- Her observation'da `assert_eq!`

### Durum
`spec_vs_impl!(eternity_spec_vs_impl: eternity)`, `epoch_spec_vs_impl`, `block_spec_vs_impl` testleri TODO comment ile disable.

---

## [tct-property-test/tests/simulate/mod.rs]
**Dosya:** `/crates/test/tct-property-test/tests/simulate/mod.rs`
**Satır Sayısı:** 52

### Genel Amaç
`Simulate` trait tanımı — spec ve real implementasyonu aynı anda simüle eder.

### Trait: `Simulate`
```rust
pub trait Simulate {
    type Spec;
    type Real;
    fn simulate(self, spec: &mut Self::Spec, real: &mut Self::Real);
}
```
Blanket impl: `Vec<T: Simulate>` de `Simulate`'i implement eder (iterator üzerinden).

### Struct: `Params`
```rust
pub struct Params {
    pub commitments: Vec<Commitment>,
    pub max_tier_actions: usize,  // default: 100
}
```

---

## [tct-property-test/tests/simulate/block.rs]
**Dosya:** `/crates/test/tct-property-test/tests/simulate/block.rs`
**Satır Sayısı:** 111

### Genel Amaç
Block tier (TCT'nin ikinci katmanı) için Action ve Observation simülasyonu.

### Action enum
`ForceRoot`, `Insert(Witness, Commitment)`, `Forget(Commitment)`

### Observation enum
`Witness`, `Root`, `Position`, `PositionOf`, `WitnessedCount`, `IsEmpty`

### Simulate impl
Her Action ve Observation: spec ile real'i aynı anda çağır, `assert_eq!` ile karşılaştır.

---

## [tct-property-test/tests/simulate/epoch.rs]
**Dosya:** `/crates/test/tct-property-test/tests/simulate/epoch.rs`
**Satır Sayısı:** 145

### Genel Amaç
Epoch tier simülasyonu. Block action'larını iç içe kapsar.

### Ek Action'lar (block.rs'ye göre ek)
- `InsertBlock(Vec<block::Action>)` — alt-blok simulate et
- `InsertBlockRoot(real::block::Root)` — sadece block root ekle (içerik değil)
- `CurrentBlockRoot` observation eklendi

### Hiyerarşi
Eternity > Epoch > Block — her üst katman alt katmanın Action'larını iç içe tutar.

---

## [tct-property-test/tests/simulate/eternity.rs]
**Dosya:** `/crates/test/tct-property-test/tests/simulate/eternity.rs`
**Satır Sayısı:** 175

### Genel Amaç
Eternity tier (tam TCT = `real::Tree`) simülasyonu.

### Ek Action'lar (epoch.rs'ye göre ek)
- `InsertEpoch(Vec<epoch::Action>)` — iç içe epoch
- `InsertEpochRoot(real::epoch::Root)` — epoch root

### Ek Observation
- `CurrentEpochRoot`

### TCT Hiyerarşisi Tamamlandı
```
Eternity (Tree)
  └── Epoch[]
        └── Block[]
              └── Commitment[]
```

---

## [tracing-subscriber/src/lib.rs]
**Dosya:** `/crates/test/tracing-subscriber/src/lib.rs`
**Satır Sayısı:** 42

### Genel Amaç
Test'lerde kullanılmak üzere hazır tracing subscriber kurulumu.

### Fonksiyonlar

#### `pub fn set_tracing_subscriber() -> DefaultGuard` (Satır 14-20)
```rust
let filter = "info,penumbra_sdk_app=trace,penumbra_sdk_mock_consensus=trace";
```
Default filter: info seviyesi, app ve mock_consensus için trace.

#### `pub fn set_tracing_subscriber_with_env_filter(filter: EnvFilter) -> DefaultGuard` (Satır 25-41)
**Kritik satır (Satır 28-32):**
```rust
let filter = filter.add_directive("r1cs=off".parse().expect("..."));
```
`r1cs=off` direktifi: R1CS (rank-1 constraint system) tracing kapalı — ZK kanıt üretimi sırasında dev miktarda CPU ve bellek tüketir.

`fmt().with_env_filter(filter).pretty().with_test_writer().finish()` — `with_test_writer()`: cargo test output capture ile uyumlu.

`set_default(subscriber)` → `DefaultGuard` döndürür — guard drop olduğunda subscriber kaldırılır.

### Nova Shield Chain İçin Dersler
- `r1cs=off`: ZK kanıt kodunu çalıştıran her testte bu direktif zorunlu; yoksa çıktı/CPU patlar
- `with_test_writer()`: cargo test / nextest ile uyumlu; normal stdout'a değil test capture buffer'ına yazar
- `DefaultGuard` dönüş: Test fonksiyonunun scope'unda tutulmalı — erken drop = subscriber kaldırılır
- `RUST_LOG` env var öncelikli: `try_from_default_env().or_else(|_| try_new(default))` pattern

---

## BÖLÜM 4: GENEL SENTEZ

### util Crate'leri — Özet
| Crate | Amaç | Nova Shield Relevance |
|---|---|---|
| auto-https | ACME otomatik TLS | Node RPC endpoint TLS yönetimi için direkt kullanılabilir |
| cometindex | PostgreSQL event indexer | Blockchain event'lerini query edilebilir yapıya dönüştürmek için |
| tendermint-proxy | gRPC proxy | CometBFT node ile client arasında köprü |
| tower-trace | Middleware tracing | Tüm Tower service'lere observability ekleme |

### bench Crate'leri — ZK Kanıt Performans Özeti
| Kanıt | Amaç | Constraint Sayısı (yaklaşık) |
|---|---|---|
| `NullifierDerivationProof` | Nullifier geçerliliği | Küçük |
| `OutputProof` | Note output | Orta |
| `ConvertProof` | Token dönüşümü | Küçük |
| `SpendProof` | UTXO harcama | Büyük |
| `SwapProof` | DEX swap başlatma | Orta |
| `SwapClaimProof` | DEX swap talep | Büyük |
| `DelegatorVoteProof` | Gizli oy | Orta-büyük |

**Alan aritmetiği benchmark'ı:** Decaf377 ve Arkworks BLS12-377 karşılaştırması yapılmış; `inverse` tüm operasyonların en pahalısı.

### test Crate'leri — Test Altyapısı Özeti
| Crate | Amaç |
|---|---|
| mock-client | Wallet simülasyonu: note/swap scan, witness/auth üretimi |
| mock-consensus | CometBFT yerini tutan test engine: begin/deliver/end/commit |
| mock-tendermint-proxy | RPC proxy: TestNodeProxy (block takip) + StubProxy (unimplemented) |
| tct-property-test | TCT doğruluk: serileştirme + witness + spec-vs-impl proptest |
| tracing-subscriber | Test logging: r1cs=off zorunlu, cargo test uyumlu |

---

## NOVA SHIELD CHAIN İÇİN KRİTİK EYLEM LİSTESİ

### Altyapı
1. **auto-https**: `axum_acceptor` aynen kopyalanabilir. Staging ve prod ACME API ayrımı flag'i ile.
2. **cometindex**: `AppView` trait implement ederek DEX event'lerini (swap, fill, LP) PostgreSQL'e index'le. `Arc<Vec<BlockEvents>>` fan-out mantığını kullan.
3. **tower-trace**: Nova Shield'in tüm gRPC service'leri `InstrumentableService::instrument()` ile wrap edilmeli. `error_span!` her ABCI request için.

### ZK Kanıtları
4. **Spend/Output benchmark**: Nova Shield UTXO ZK kanıt süresini ölç. `criterion` + `black_box` + `cs.num_constraints()` print.
5. **Swap/SwapClaim**: DEX ZK kanıtı için bu iki benchmark şablonu kullan. Pro-rata output hesabı ZK devresi içinde.
6. **`r1cs=off`**: ZK kanıt çalıştıran tüm test'lerde bu direktifi ekle — `tracing_subscriber/src/lib.rs` aynen kopyalanabilir.

### Test Altyapısı
7. **MockClient**: Nova Shield wallet'ın integration test versiyonu. `trial_decrypt`, SCT sync, `witness_auth_build` akışı aynen kullanılabilir.
8. **TestNode**: `fast_forward(N)` ve `ts_callback` Nova Shield'in tüm integration test'lerinde zorunlu primitive'ler.
9. **proptest ile TCT**: Nova Shield SCT'si için `serialize.rs` ve `witness.rs` proptest'leri klonlanabilir — aynı invariant'lar geçerli.
10. **StubProxy vs TestNodeProxy**: Test gereksinimine göre seç; proxy gerektirmeyen testlerde `StubProxy` kullan.

### Mimari Kararlar
11. **PostgreSQL event index**: Read-only src + read-write dst ayrımı mimaride zorunlu. `index_watermarks` tablosu ile replay güvenliği.
12. **Version + option_hash**: Index versiyonlama iki boyutlu (major + option hash); config değişikliği tespiti option_hash ile yapılır.
13. **BATCH_LOOKAHEAD=2**: Event batch channel kapasitesi: producer 2 batch ileri gidebilir. Backpressure için bu değer optimize edilmeli.
