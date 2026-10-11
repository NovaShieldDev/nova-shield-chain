# Penumbra Bin Crates Analizi — pd, pindexer, elcuity, pclientd, pmonitor

**Agent:** pd-bins
**Tarih:** 2026-07-22
**Kapsam:** 5 bin crate, 67 dosya
**Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/bin/`

---

## İÇİNDEKİLER

1. [pd — Penumbra Daemon](#1-pd--penumbra-daemon)
2. [pindexer — CometBFT Event Indexer](#2-pindexer--cometbft-event-indexer)
3. [elcuity — Liquidity Tournament Otomasyonu](#3-elcuity--liquidity-tournament-otomasyonu)
4. [pclientd — Client Daemon](#4-pclientd--client-daemon)
5. [pmonitor — Balance Audit Aracı](#5-pmonitor--balance-audit-araci)
6. [Nova Shield Chain İçin Dersler](#6-nova-shield-chain-icin-dersler)

---

## 1. pd — Penumbra Daemon

### 1.1 `src/main.rs` (569 satır)

**Genel Amaç:** Penumbra'nın tam node implementasyonu. ABCI sunucusu, gRPC sunucusu, Prometheus metrikleri, zipserve (frontend), migration sistemi tek entry point'ten başlatılır.

**Struct/Enum/Type Listesi:**
- Harici: `App` (penumbra_sdk_app), `Storage` (cnidarium), `SubstoreConfig`
- Lokal: CLI enum'ları (cli.rs'den import)

**Fonksiyonlar:**

```rust
async fn main() -> anyhow::Result<()>
```
- Tokio `#[tokio::main]` ile async main
- `RootCommand::Start` kolu:
  1. `Storage::load(home, SUBSTORE_PREFIXES)` — RocksDB açılır
  2. `App::is_ready()` ile halt-bit kontrolü — eğer migration gerekiyorsa hata
  3. ABCI server: `tower::ServiceBuilder` ile middlewares + `abci::Server::listen(abci_bind)`
  4. gRPC server: `tonic::transport::Server` ile tüm servislerin merge'i
  5. Prometheus: `PrometheusBuilder::new().with_http_listener(metrics_bind).install()`
  6. DEX histogram bucket'ları: `register_histogram!(ROUTE_FILL_RATIO_GAUGE, ...)`
  7. Frontend: `zipserve::ZipServeLayer::new(MINIFRONT_ARCHIVE_BYTES)`
  8. Node status: `zipserve::ZipServeLayer::new(NODE_STATUS_ARCHIVE_BYTES)`
  9. `tokio::select!` — abci_server, grpc_server, acme_worker birlikte çalışır
  10. sleep_worker + cpu_worker spawn edilir

**gRPC Servisleri (merge sırası):**
```
ViewService, SimulationService, TendermintProxyService,
QueryService (governance, dex, sct, shielded_pool, fee, stake, ibc, compact_block, chain, auction, lqt)
```

**Önemli Tasarım Kararları:**
- `acme_worker`: Let's Encrypt otomatik HTTPS. `grpc_bind = "auto"` → 0.0.0.0:443 + ACME
- Default bindler RFC ayırt edilmiş: ABCI=localhost, gRPC=localhost, metrics=localhost
- `#[cfg(not(feature = "std"))]` kullanılmaz; her şey std ile

**Nova Shield İçin:** Aynı `tokio::select!` pattern ile birden fazla servisi tek süreçte çalıştır. gRPC + REST + metrics ayrı portlarda, tek binary.

---

### 1.2 `src/cli.rs` (279 satır)

**Struct/Enum/Type Listesi:**

```rust
enum RootCommand {
    Start {
        home: PathBuf,
        abci_bind: SocketAddr,      // default: 127.0.0.1:26658
        grpc_bind: Option<String>,  // None → 127.0.0.1:8080, "auto" → 0.0.0.0:443 ACME
        metrics_bind: SocketAddr,   // default: 127.0.0.1:9000
        cometbft_addr: Url,         // default: http://127.0.0.1:26657
        tendermint_addr: Option<Url>, // deprecated alias
    },
    Network { network_home: PathBuf, cmd: NetworkCommand },
    Export { ... },
    Migrate { ... },
}

enum NetworkCommand {
    Generate { ... },
    Join { ... },
    UnsafeResetAll,
}

enum MigrateCommand {
    ReadyToStart,
    IbcRecovery {
        old_client_id: String,
        new_client_id: String,
        target_app_version: u64,
    },
    NoOp { target_app_version: u64 },
}
```

**Tasarım Kararları:**
- `tendermint_addr` deprecated alias — geriye dönük uyumluluk
- `grpc_bind: Option<String>` — string çünkü "auto" özel değer
- ACME modu production için, localhost modu development için

---

### 1.3 `src/lib.rs` (20 satır)

**Genel Amaç:** Statik asset embed.

```rust
pub const MINIFRONT_ARCHIVE_BYTES: &[u8] = include_bytes!("minifront.zip");
pub const NODE_STATUS_ARCHIVE_BYTES: &[u8] = include_bytes!("node-status.zip");
```

ZIP dosyaları derleme zamanında binary'ye embed edilir. `build.rs` bu dosyaları üretir ya da indirip kopyalar.

---

### 1.4 `src/zipserve.rs` (68 satır)

**Genel Amaç:** In-memory ZIP arşivlerinden axum ile statik dosya servis etme.

**Struct/Enum/Type Listesi:**

```rust
pub struct ZipServeLayer {
    archive: Arc<ZipArchive<Cursor<&'static [u8]>>>,
    prefix: String,
}
```

**Fonksiyonlar:**

```rust
fn new(bytes: &'static [u8], prefix: &str) -> Self
fn serve(self, router: Router) -> Router
```

**Route Mantığı:**
- `GET /{prefix}/` → `index.html`
- `GET /{prefix}/*path` → ZIP içinden `path` lookup

**Önemli Detay:**
```rust
#[allow(clippy::unwrap_used)]
```
ZIP parse operasyonları `unwrap` kullanır — derleme zamanında embed edilmiş geçerli ZIP olduğu kesin olduğundan panik riski yok.

`mime_guess::from_path(path).first_or_octet_stream()` ile Content-Type belirlenir.

**Nova Shield İçin:** Dashboard/explorer UI'ı aynı şekilde embed et. Dağıtım basitleşir, ayrı web server gerekmez.

---

### 1.5 `src/migrate.rs` (363 satır)

**Genel Amaç:** Tüm migration'ların merkezi dispatch noktası.

**Enum:**

```rust
enum Migration {
    Testnet72,
    Testnet73,
    Testnet74,
    Testnet75,
    Testnet76,
    Testnet77,
    Testnet78,
    Mainnet1,
    Mainnet2,
    Mainnet3,
    Mainnet4,
    Mainnet5,
    ReadyToStart,
    IbcClientRecovery { old_client_id, new_client_id, target_app_version },
    NoOp { target_app_version },
}
```

**Fonksiyonlar:**

```rust
pub async fn migrate_with_params(
    home: PathBuf,
    genesis_start: Option<DateTime<Utc>>,
    target_migration: Migration,
    force: bool,
) -> anyhow::Result<()>
```

**Kontrol Akışı:**
1. `App::is_ready()` — halt bit kontrolü. Hazır değilse:
   - `force` flag ile devam et veya hata
2. `storage.latest_version()` kontrolü — versiyon/height tutarlılığı
3. Migration enum'una göre dispatch
4. `archive_directory(storage_path)` — migration öncesi gzip tar arşivi

**`archive_directory()`:**
```rust
fn archive_directory(dir: &Path) -> anyhow::Result<()>
```
`tar::Builder + GzEncoder` ile `storage.tar.gz` üretir. Migration öncesi state snapshot.

**`migrate_comet_data()`:**
CometBFT veri temizliği:
- `genesis.json` → `config/config/genesis.json` kopyalanır
- `priv_validator_state.json`: `height` bir artırılır (consensus state devam)
- Temizlenen: `evidence.db`, `state.db`, `blockstore.db`, `cs.wal`
- Korunan: `tx_index.db` (transaction history saklanır)

**Tasarım Kararları:**
- Eski migration'lar (Testnet72-78) kaynak kodda tutulur — referans için
- Yeni migration'lar `migrate2` framework kullanır
- `force` flag üretim ortamında kullanılmamalı (safety bypass)

---

### 1.6 `src/migrate/migrate2/framework.rs` (184 satır)

**Genel Amaç:** Yeni nesil migration trait'i. 3-phase protokol.

**Trait:**

```rust
#[async_trait]
pub trait Migration {
    fn name(&self) -> String;
    fn target_app_version(&self) -> u64;

    // Phase 1: Hazırlık (opsiyonel)
    async fn prepare(&self, _home: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    // Phase 2: Asıl migration (override edilmez, migrate_inner çağrılır)
    async fn migrate(&self, home: &Path) -> anyhow::Result<()> {
        // migrate_inner çağrılır
        // halt bit sıfırlanır (delta.ready_to_start())
        // block height 0 yapılır
        // commit_in_place
    }

    // Override edilecek asıl iş
    async fn migrate_inner(&self, _home: &Path, _delta: &mut StateDelta) -> anyhow::Result<()> {
        Ok(())
    }

    // Phase 3: Tamamlama
    async fn complete(&self, home: &Path, genesis_start: DateTime<Utc>) -> anyhow::Result<()> {
        // post_upgrade_root_hash hesaplanır
        // Yeni genesis.json yazılır: initial_height = pre_upgrade_height + 1
        // app_hash = post_upgrade_root_hash
        // priv_validator_state.json yazılır
    }

    // Tüm üç aşamayı çalıştırır
    async fn run(&self, home: &Path, genesis_start: DateTime<Utc>) -> anyhow::Result<()> {
        self.prepare(home).await?;
        self.migrate(home).await?;
        self.complete(home, genesis_start).await?;
    }
}
```

**Kritik Detaylar:**
- `initial_height = pre_upgrade_height + 1`: upgrade block'undan sonraki blok
- `app_hash = post_upgrade_root_hash`: yeni genesis'in state hash'i
- `delta.ready_to_start()`: JMT'de halt bit sıfırlanır
- Tüm node'lar aynı `migrate()` çalıştırmak zorunda — determinizm kritik

**Nova Shield İçin:** Aynı 3-phase pattern zorunlu. `prepare()` → state dump/check, `migrate()` → JMT transforms, `complete()` → yeni genesis üret.

---

### 1.7 `src/migrate/migrate2/ibc_client_recovery.rs` (59 satır)

**Genel Amaç:** IBC client state recovery — "frozen" client'ı çalışan substitute ile değiştirir.

**Struct:**

```rust
pub struct IbcClientRecoveryMigration {
    pub old_client_id: String,
    pub new_client_id: String,
    pub target_app_version: u64,
}
```

**migrate_inner:**
```rust
async fn migrate_inner(&self, _home: &Path, delta: &mut StateDelta) -> anyhow::Result<()> {
    delta.recover_client::<PenumbraHost>(&self.old_client_id, &self.new_client_id).await?;
    Ok(())
}
```

`recover_client` IBC spec'e göre: subject client'ın state'i substitute ile replace edilir. Frozen height temizlenir.

---

### 1.8 `src/migrate/migrate2/noop.rs` (39 satır)

**Genel Amaç:** Halt bit sıfırlama — state değişikliği yok, sadece chain devam eder.

```rust
pub struct NoOpMigration {
    pub target_app_version: u64,
}
```

`migrate_inner` override edilmez → default no-op. Framework otomatik halt bit sıfırlar ve genesis üretir.

---

### 1.9 `src/migrate/simple.rs` (74 satır)

**Genel Amaç:** Test migration. JMT'ye "banana" yazar.

```rust
async fn migrate_inner(&self, home: &Path, delta: &mut StateDelta) -> anyhow::Result<()> {
    delta.put_raw(
        "banana".to_string(),
        "a good fruit (and migration works!)".as_bytes().to_vec(),
    );
    Ok(())
}
```

**Nova Shield İçin:** Migration framework'ün test edilmesi için mutlaka benzer bir `test_migration` variant'ı ekle.

---

### 1.10 `src/migrate/reset_halt_bit.rs` (21 satır)

**Genel Amaç:** Sadece halt bit sıfırla, başka hiçbir şey yapma.

```rust
pub async fn reset_halt_bit(home: &Path) -> anyhow::Result<()> {
    let storage = Storage::load(...).await?;
    let mut delta = StateDelta::new(storage.latest_snapshot());
    delta.ready_to_start();
    storage.commit_in_place(delta).await?;
    Ok(())
}
```

---

### 1.11 `src/metrics/cpu_worker.rs` (34 satır)

```rust
const SLEEP_DURATION: Duration = Duration::from_secs(2);
const METRICS_PREFIX: &str = "pd_";

pub async fn run_cpu_worker() {
    let collector = metrics_process::Collector::default();
    loop {
        collector.collect();
        tokio::time::sleep(SLEEP_DURATION).await;
    }
}
```

`metrics_process::Collector`: CPU kullanımı, process memory, file descriptors. `pd_` prefix ile Prometheus'a kayıt.

---

### 1.12 `src/metrics/sleep_worker.rs` (65 satır)

**Genel Amaç:** Tokio runtime scheduler drift ölçümü.

```rust
const SLEEP_DRIFT: &str = "pd_async_sleep_drift_microseconds";

pub async fn run_sleep_worker() {
    loop {
        let start = Instant::now();
        tokio::time::sleep(Duration::from_secs(1)).await;
        let elapsed = start.elapsed();
        let drift = elapsed.as_micros().saturating_sub(1_000_000);
        counter!(SLEEP_DRIFT).increment(drift as u64);
    }
}
```

`u64::MAX` fallback overflow için. Drift > birkaç ms ise scheduler tıkandı demektir.

**Nova Shield İçin:** Async scheduler sağlığını izlemek için bu pattern bire bir kopyalanabilir. Kritik bir observability metriği.

---

### 1.13 `src/network/config.rs` (343 satır)

**Struct/Enum/Type Listesi:**

```rust
pub struct NetworkTendermintConfig {
    pub config: TendermintConfig,
}

pub struct ValidatorKeys {
    pub validator_id_sk: SpendAuthSigningKey,    // Ed25519 tabanlı, not gerçek Ed25519
    pub validator_id_vk: SpendAuthVerificationKey,
    pub validator_cons_sk: ed25519_consensus::SigningKey,  // CometBFT consensus key
    pub validator_cons_pk: ed25519_consensus::VerificationKey,
    pub node_key_sk: ed25519_consensus::SigningKey,   // P2P kimlik
    pub node_key_pk: ed25519_consensus::VerificationKey,
    pub validator_spend_key: SpendKey,
}
```

**Fonksiyonlar:**

```rust
impl ValidatorKeys {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        // SpendKey::from_seed(seed) → validator_spend_key
        // validator_id_sk = spend_key.spend_auth_key()
        // validator_cons_sk = Ed25519::from_seed(seed)  ← AYNI SEED!
        // node_key_sk = Ed25519::from_seed(seed)        ← AYNI SEED!
    }

    pub fn generate() -> Self {
        // OsRng ile rastgele seed üret
        // from_seed çağır
    }
}
```

**GÜVENLİK NOTU:** `from_seed` hem consensus key hem node key için aynı seed kullanır. Determinizm için gerekli ama üretimde farklı seed'ler kullanmak daha güvenli.

**`canonicalize_path()`:**
```rust
fn canonicalize_path(path: &str) -> PathBuf {
    // "~" ile başlıyorsa home_dir ile değiştir
    // regex: r"^~"
}
```

**`url_has_necessary_parts()`:**
```rust
fn url_has_necessary_parts(url: &Url) -> bool {
    url.scheme() != "" && url.host().is_some() && url.port().is_some()
}
```

---

### 1.14 `src/network/generate.rs` (823 satır)

**Struct/Enum/Type Listesi:**

```rust
pub struct NetworkConfig {
    pub network_name: String,
    pub network_dir: PathBuf,
    pub validators: Vec<Validator>,
    pub allocations: Vec<Allocation>,
    pub genesis_start: Option<DateTime<Utc>>,
    // ...
}

pub struct NetworkAllocation {
    pub address: Address,
    pub amount: Amount,
    pub denom: String,
}

pub struct NetworkValidator {
    pub name: String,           // max 140 bytes
    pub website: String,        // max 70 bytes
    pub description: String,    // max 280 bytes
    pub sequence_number: u32,
    pub voting_power: u64,
    pub enabled: bool,
    pub keys: ValidatorKeys,
}
```

**Fonksiyonlar:**

```rust
pub fn make_genesis(
    chain_id: String,
    app_state: AppState,
    genesis_start: DateTime<Utc>,
) -> Genesis<AppState> {
    // initial_height = 0
    // max_gas = -1  (sınırsız)
    // evidence max_age_num_blocks = 130_000
    // evidence max_age_duration = 650_000s
    // MAX_BLOCK_TXS_PAYLOAD_BYTES = 12_000_000 (12 MB)
    // MAX_EVIDENCE_SIZE_BYTES = 1_048_576 (1 MB)
}

pub fn make_checkpoint(app_hash: Vec<u8>, height: u64) -> Genesis<AppState> {
    // Upgrade sonrası genesis checkpoint
    // app_hash = son state hash
    // initial_height = pre_upgrade_height + 1
}
```

**`NetworkAllocation::simple()`:**
```rust
fn simple(address: Address) -> Vec<Self> {
    vec![
        NetworkAllocation { amount: 100_000, denom: "upenumbra" },
        NetworkAllocation { amount: 1_000, denom: "test_usd" },
    ]
}
```

**`string_u128` deserializer:**
```rust
fn string_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
// Kabul eder: "1_000_000" (string, alt çizgi desteklenir)
// Kabul eder: 1000000 (sayı)
// İkisi de parse edilir
```

**Nova Shield İçin:** `make_genesis` pattern'i doğrudan kullan. `initial_height = 0`, `max_gas = -1` consensus parametreleri.

---

### 1.15 `src/network/join.rs` (448 satır)

**Genel Amaç:** Mevcut bir ağa katılma. Genesis indir, peer'ları bul.

**Fonksiyonlar:**

```rust
pub async fn network_join(
    home: PathBuf,
    node_name: String,
    external_address: Option<SocketAddr>,
    tendermint_rpc: Url,
    moniker: String,
) -> anyhow::Result<()> {
    // 1. fetch_genesis(tendermint_rpc) → genesis.json
    // 2. fetch_peers(tendermint_rpc) → Vec<TendermintAddress>
    // 3. ValidatorKeys::generate() → yeni validator keys
    // 4. Config dosyaları yaz
}

async fn fetch_peers(rpc_url: &Url, external_address: Option<SocketAddr>) -> anyhow::Result<Vec<TendermintAddress>> {
    // GET /net_info → peers listesi
    // shuffle candidates
    // "seed" içeren moniker → seed_nodes
    // threshold = 5 regular peers (seed node'lar sınırsız)
    // address_could_be_external() filtresi
}

fn address_could_be_external(addr: &SocketAddr) -> bool {
    // RFC1918 private range filtreleme:
    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
    // 127.0.0.0/8 (loopback)
    // false döner → filtrelenir
}

fn parse_tm_address_listener(listener_str: &str) -> anyhow::Result<SocketAddr> {
    // Regex: r"Listener\(.*@(tcp://)?(.*)\)"
    // Örnek input: "Listener(@tcp://0.0.0.0:26656)"
    // Çıkar: "0.0.0.0:26656"
}

async fn fetch_listen_address(
    rpc_url: &Url,
    node_id: &NodeId,
) -> anyhow::Result<TendermintAddress> {
    // self_id@host:port formatında
    // /status endpoint'inden dinleme adresi alınır
}
```

---

## 2. pindexer — CometBFT Event Indexer

### 2.1 `src/lib.rs` (38 satır)

**Struct:**

```rust
pub struct Options {
    pub indexing_denom: String,          // USDC passet1... formatı
    pub dex_ex_min_liquidity: u64,       // default: 100_000_000
    pub dex_ex_ignore_arb: bool,         // default: true
    pub block_time_s: f64,               // default: 5.1
}
```

`block_time_s` LQT indexer versiyon hash'ine girer — değişirse schema reset tetiklenir.

---

### 2.2 `src/indexer_ext.rs` (25 satır)

**Trait:**

```rust
pub trait IndexerExt {
    fn with_default_penumbra_app_views(self, opts: &Options) -> Self;
}
```

**Kayıtlı View'lar (sırayla):**
1. `Block` — block_details tablosu
2. `ValidatorSet` — validator set değişiklikleri
3. `Slashings` — ceza olayları
4. `DelegationTxs` — delegation işlemleri
5. `UndelegationTxs` — undelegation işlemleri
6. `GovernanceProposals` — governance teklif yaşam döngüsü
7. `DexEx` (dex_ex::Component) — DEX explorer, candlestick, fiyat grafikleri
8. `Supply` — toplam arz takibi
9. `IBC` — IBC transfer akışı
10. `Insights` — genel blockchain analitik
11. `Lqt` — likidite turnuvası

---

### 2.3 `src/block.rs` (68 satır)

**SQL Schema:**
```sql
CREATE TABLE IF NOT EXISTS block_details (
    height BIGINT PRIMARY KEY,
    root BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL
);
```

**Fonksiyonlar:**
- `AppView::init_chain()` — tablo oluşturur
- `AppView::index_batch()` — `EventBlockRoot` parse eder, `INSERT ... ON CONFLICT DO NOTHING`

---

### 2.4 `src/parsing.rs` (27 satır)

**Genel Amaç:** Genesis parsing ile yardımcı hata mesajları.

```rust
pub fn parse_content(content: &str) -> anyhow::Result<Content> {
    serde_json::from_str::<Content>(content)
        .context("Failed to parse genesis content. \
                  If this is an upgrade genesis, check that the app_state format matches. \
                  Note: upgrade genesis != initial genesis format")
}
```

---

### 2.5 `src/dex_ex/mod.rs` (~1653 satır) — EN BÜYÜK DOSYA

**Genel Amaç:** DEX Explorer. Candlestick verisi, fiyat grafikleri, LP pozisyon takibi.

**Struct/Enum/Type Listesi:**

```rust
// Temel birim
pub struct Candle {
    pub open: f64,
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub direct_volume: f64,   // doğrudan işlem hacmi
    pub swap_volume: f64,     // batch swap hacmi
}

// Zaman penceresi türleri
pub enum Window {
    W1m,    // 1 dakika
    W15m,   // 15 dakika
    W1h,    // 1 saat
    W4h,    // 4 saat
    W1d,    // 1 gün
    W1w,    // 1 hafta
    W1mo,   // 1 ay
}

// Bir pencere içindeki candle biriktiricisi
pub struct WindowedCandle {
    window: Window,
    start: DateTime<Utc>,
    candle: Option<Candle>,
}

// Fiyat grafiği context (load/update/unload döngüsü)
struct PriceChartContext {
    asset1: AssetId,
    asset2: AssetId,
    candles: BTreeMap<Window, WindowedCandle>,
}

// Pair özet context
struct SummaryContext {
    asset1: AssetId,
    asset2: AssetId,
    // ...
}
```

**SQL Tabloları:**
```sql
-- Fiyat grafikleri
CREATE TABLE dex_ex_price_charts (
    id BIGSERIAL PRIMARY KEY,
    asset1 BYTEA NOT NULL,
    asset2 BYTEA NOT NULL,
    window TEXT NOT NULL,         -- "1m", "15m", "1h", "4h", "1d", "1w", "1mo"
    start_time TIMESTAMPTZ NOT NULL,
    open FLOAT8 NOT NULL,
    close FLOAT8 NOT NULL,
    low FLOAT8 NOT NULL,
    high FLOAT8 NOT NULL,
    direct_volume FLOAT8 NOT NULL,
    swap_volume FLOAT8 NOT NULL,
    UNIQUE(asset1, asset2, window, start_time)
);

-- Pair snapshot (blok bazlı)
CREATE TABLE dex_ex_pairs_block_snapshot (
    id BIGSERIAL PRIMARY KEY,
    height BIGINT NOT NULL,
    asset1 BYTEA NOT NULL,
    asset2 BYTEA NOT NULL,
    price FLOAT8,
    liquidity FLOAT8,
    -- ...
);
```

**İşlenen Event'ler:**
- `EventBatchSwap` — toplu swap sonuçları
- `EventPositionOpen` — LP pozisyon açma
- `EventPositionClose` — LP pozisyon kapama
- `EventPositionWithdraw` — LP likidite çekme
- `EventPositionExecution` — pozisyon dolumu (trade)
- `EventArbExecution` — arbitraj işlemi
- `EventSwap` — bireysel swap emri
- `EventSwapClaim` — swap output talep etme
- `EventLqtPositionReward` — LQT pozisyon ödülü

**`WindowedCandle` Mantığı:**
```rust
impl WindowedCandle {
    fn update(&mut self, price: f64, volume: f64, is_direct: bool, timestamp: DateTime<Utc>) {
        // Eğer timestamp pencere dışındaysa → flush ve yeni candle başlat
        // Aksi halde candle güncelle:
        //   close = price (her zaman)
        //   low = min(low, price)
        //   high = max(high, price)
        //   direct_volume veya swap_volume += volume
    }

    fn window_start(window: &Window, ts: DateTime<Utc>) -> DateTime<Utc> {
        // Window boundary hesapla
        // 1m → dakikayı truncate
        // 1h → saati truncate
        // vb.
    }
}
```

**`ON CONFLICT DO UPDATE` Pattern:**
```sql
INSERT INTO dex_ex_price_charts (...) VALUES (...)
ON CONFLICT (asset1, asset2, window, start_time)
DO UPDATE SET
    close = EXCLUDED.close,
    low = LEAST(dex_ex_price_charts.low, EXCLUDED.low),
    high = GREATEST(dex_ex_price_charts.high, EXCLUDED.high),
    direct_volume = dex_ex_price_charts.direct_volume + EXCLUDED.direct_volume,
    swap_volume = dex_ex_price_charts.swap_volume + EXCLUDED.swap_volume;
```

**Nova Shield İçin:** Bu candlestick implementasyonu nova-dex explorer için kullanılabilir. `ON CONFLICT DO UPDATE` ile idempotent upsert kritik.

---

### 2.6 `src/supply.rs` (953 satır)

**Genel Amaç:** Toplam arz takibi. Staked ve unstaked UM ayrımı.

**Sabitler:**
```rust
const BPS_SQUARED: u128 = 1_0000_0000; // 10^8 — yüzde karesi birimi
```

**İç Modüller:**

```rust
mod unstaked_supply {
    pub struct Supply {
        pub um: i128,
        pub auction: i128,
        pub dex: i128,
        pub arb: i128,
        pub fees: i128,
    }

    impl Supply {
        pub fn modify(&mut self, event: &SomeEvent) {
            // Event'e göre ilgili alanı artır/azalt
        }
    }
}

mod delegated_supply {
    pub struct Supply {
        pub del_um: i128,       // delege edilmiş miktarı temsil eden token
        pub rate_bps2: u128,    // exchange rate, BPS_SQUARED biriminde
    }

    impl Supply {
        pub fn add_um(&mut self, um_amount: i128) {
            // UM → del_UM dönüşümü
            // del_um = um / (rate_bps2 / BPS_SQUARED)
            // Undelegation: round_down (kullanıcı aleyhine)
            // Delegation: round_up (protokol aleyhine, güvenli taraf)
        }
    }
}
```

**İzlenen 10 Event:**
1. `EventFundingStreamReward` — staking ödülü
2. `EventProposalDepositClaim` — governance deposit iade
3. `EventValidatorRewardPaid` — validator ödülü
4. `EventDelegationChange` — delegation değişimi
5. `EventUndelegationChange` — undelegation değişimi
6. `EventArbExecution` — arbitraj kârı
7. `EventBatchSwap` — batch swap fee
8. `EventTransactionFee` — işlem ücreti
9. `EventValueCircuitBreakerCredit` — VCB kredi
10. `EventValueCircuitBreakerDebit` — VCB borç

**VCB (Value Circuit Breaker):** Her bileşen (auction, dex, vb.) ayrı takip edilir. Beklenmedik büyük akışlar alarm tetikler.

---

### 2.7 `src/governance.rs` (543 satır)

**SQL Tabloları:**
```sql
CREATE TABLE governance_proposals (
    id BIGINT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    kind TEXT NOT NULL,         -- "signaling", "emergency", "upgrade", "freeze", "unfreeze", "community_pool_spend", "dao_spend"
    payload JSONB,              -- kind'a özel veri
    start_height BIGINT NOT NULL,
    end_height BIGINT NOT NULL,
    state JSONB NOT NULL,       -- mevcut durum (state machine)
    proposer BYTEA NOT NULL
);

CREATE TABLE governance_validator_votes (
    proposal_id BIGINT NOT NULL,
    identity_key BYTEA NOT NULL,
    vote TEXT NOT NULL,         -- "yes", "no", "abstain"
    PRIMARY KEY (proposal_id, identity_key)
);

CREATE TABLE governance_delegator_votes (
    proposal_id BIGINT NOT NULL,
    nullifier BYTEA NOT NULL,
    vote TEXT NOT NULL,
    unbonded_amount BIGINT NOT NULL,
    PRIMARY KEY (proposal_id, nullifier)
);
```

**State Machine (JSONB):**
```json
// Voting: {"status": "voting"}
// Passed: {"status": "finished", "outcome": "passed"}
// Failed: {"status": "finished", "outcome": "failed"}
// Failed+Withdrawn: {"status": "finished", "outcome": "failed", "withdrawn": true}
// Slashed: {"status": "finished", "outcome": "slashed"}
// Claimed: {"status": "claimed", "outcome": "..."}
```

**`handle_proposal_deposit_claim()`:**
```rust
// Mevcut state'i oku (outcome'u koru)
// state: "finished" → "claimed" yap
// SQL: UPDATE governance_proposals SET state = $1 WHERE id = $2
```

---

### 2.8 `src/ibc/mod.rs` (256 satır)

**Enum:**
```rust
pub enum EventKind {
    InboundTransfer,
    OutboundTransfer,
    OutboundRefund,  // failed transfer geri iadesi
}
```

**Struct:**
```rust
pub struct DatabaseTransfer {
    pub kind: EventKind,
    pub asset: String,
    pub amount: Decimal,
    pub sender: String,
    pub receiver: String,
    pub negate: bool,  // OutboundRefund için true → negatif akış
}
```

**SQL Trick:**
```sql
-- Signed flow: $6 = negate boolean, $2 = amount
UPDATE ibc_summary SET flow = flow + ($6::NUMERIC * $2::NUMERIC)
-- negate=true → -amount, negate=false → +amount
```

**Schema Versioning:**
```rust
fn version(&self) -> Version {
    Version::with_major(1)
    // major versiyon değişirse → reset.sql çalıştırılır
}
```

---

### 2.9 `src/lqt/mod.rs` (439 satır)

**Genel Amaç:** Likidite Turnuvası (LQT) indexer.

**Versiyon:**
```rust
fn version(&self) -> Version {
    let hash = blake2b(block_time_s.to_le_bytes());
    Version::with_major(4).with_option_hash(hash)
    // block_time_s değişirse → schema reset
}
```

**SQL Tabloları:**
```sql
-- LQT parametreleri (genesis'te set edilir)
CREATE TABLE lqt._params (
    gauge_threshold BIGINT NOT NULL,
    delegator_share BIGINT NOT NULL,
    epoch_duration BIGINT NOT NULL,
    rewards_per_block BIGINT NOT NULL
);

-- Epoch metadata
CREATE TABLE lqt._epoch_info (
    epoch_index BIGINT PRIMARY KEY,
    start_height BIGINT NOT NULL,
    end_height BIGINT,
    total_rewards BIGINT
);

-- Tamamlanan epoch'lar
CREATE TABLE lqt._finished_epochs (
    epoch_index BIGINT PRIMARY KEY,
    ...
);

-- Delegator ödülleri
CREATE TABLE lqt._delegator_rewards (
    epoch_index BIGINT,
    delegator BYTEA,
    amount BIGINT,
    PRIMARY KEY (epoch_index, delegator)
);

-- LP ödülleri
CREATE TABLE lqt._lp_rewards (
    epoch_index BIGINT,
    position_id BYTEA,
    amount BIGINT,
    asset_liquidity BIGINT,
    PRIMARY KEY (epoch_index, position_id)
);

-- Oylar (gauge votes)
CREATE TABLE lqt._votes (
    epoch_index BIGINT,
    voter BYTEA,
    asset BYTEA,
    voting_power BIGINT,
    PRIMARY KEY (epoch_index, voter, asset)
);
```

**Epoch Sınırı Tespiti:**
```rust
// EventEpochRoot event'i geldiğinde → epoch bitmiş
// on_startup(): mevcut epoch'u recover et
// set_rewards_for_epoch(): ödülleri finalize et
```

---

### 2.10 `src/insights/mod.rs` (581 satır)

**Genel Amaç:** Genel blockchain analitik. Arz, fiyat, unique depositor sayısı.

**Struct:**
```rust
struct ValidatorSupply {
    del_um: i128,
    rate_bps2: u128,
}

enum DepositorExisted {
    Yes,
    No,
}
```

**Versiyon:**
```rust
fn version(&self) -> Version {
    let hash = blake2b(price_numeraire_asset_id.to_bytes());
    Version::with_major(1).with_option_hash(hash)
    // Fiyat referans asset değişirse → reset
}
```

**`I256` Kullanımı:**
```rust
use ethnum::I256;
// IBC akışı hesaplamalarında i128 yetmez → I256
// signed_flow: I256 = inbound - outbound
```

**Nova Shield İçin:** Analitik dashboard için bu pattern. `I256` ile taşma güvenli akış hesabı.

---

## 3. elcuity — Liquidity Tournament Otomasyonu

### 3.1 `src/lib.rs` (59 satır)

**Struct/Enum:**
```rust
pub struct Opt {
    pub grpc_url: Url,
    pub view_service: Option<Url>,
    #[command(subcommand)]
    pub cmd: Command,
}

pub enum Command {
    Vote,               // Her epoch'ta oy ver
    Lp,                 // LP pozisyonlarını yönet (her 120s)
    Swap,               // Sürekli multi-hop döngüsü
}
```

---

### 3.2 `src/clients.rs` (81 satır)

**Struct:**
```rust
pub struct Clients {
    pub view: ViewServiceClient<BoxBody>,
    pub custody: CustodyServiceClient<BoxBody>,
    pub node: Channel,
}
```

**`Clients::init()`:**
```rust
pub async fn init(opt: &Opt) -> anyhow::Result<Self> {
    let node = ViewServer::get_pd_channel(&opt.grpc_url).await?;
    let svc = box_grpc_svc::connect(opt.view_service.as_ref())?;
    Ok(Self {
        view: ViewServiceClient::new(svc.clone()),
        custody: CustodyServiceClient::new(svc),  // AYNI SVC!
        node,
    })
}
```

**Kritik:** View ve Custody aynı endpoint'ten sunulur. pclientd bu ikisini tek port'ta sağlar.

---

### 3.3 `src/lp/mod.rs` (353 satır)

**Sabitler:**
```rust
const POSITION_CHUNK_SIZE: usize = 5;   // Her iterasyonda en fazla 5 pozisyon
const DEFAULT_PRICE: f64 = 1.0;
const DEFAULT_FEE_BPS: u32 = 500;       // %5 default fee
const ADJUST_WAIT_SECS: u64 = 120;     // 2 dakikada bir güncelle
```

**Ana Fonksiyon:**
```rust
pub async fn lp(clients: &mut Clients, assets: Vec<AssetId>) -> anyhow::Result<()> {
    loop {
        // 1. Mevcut pozisyonları çek (view.positions())
        // 2. Chunk'lara böl (POSITION_CHUNK_SIZE=5)
        // 3. Her chunk için: close → withdraw → open (yeni fiyatla)
        // 4. ADJUST_WAIT_SECS bekle
    }
}
```

**`spread()` Fonksiyonu:**
```rust
async fn spread(clients: &mut Clients, asset1: AssetId, asset2: AssetId) -> anyhow::Result<(f64, f64)> {
    // Küçük miktarla her iki yönde simüle et
    // buy_price = asset1→asset2 simülasyon çıktısı
    // sell_price = asset2→asset1 simülasyon çıktısı
    // (buy_price, sell_price) döndür
}
```

**`spread_to_price_and_fee_bps()`:**
```rust
fn spread_to_price_and_fee_bps(buy: f64, sell: f64) -> (f64, u32) {
    // midpoint = (buy + sell) / 2.0
    // raw_fee = (buy / midpoint) - 1.0   // spread'in yarısı
    // epsilon sıkıştırma: çok küçük/büyük değerleri düzelt
    // fee_bps = (raw_fee * 10_000.0) as u32
    // (midpoint, fee_bps)
}
```

**`price_to_p_q()` — Float → Rational Dönüşüm:**
```rust
fn price_to_p_q(price: f64) -> (u64, u64) {
    // Bit-doubling loop:
    let mut p = price;
    let mut q = 1.0_f64;
    while p.fract() != 0.0 && q < (1u64 << 60) as f64 {
        p *= 2.0;
        q *= 2.0;
    }
    // Tam sayıya yuvarla
    (p as u64, q as u64)
}
```

**Bütçe Yönetimi:**
```rust
// Staking token'ın %90'ı fee için ayrılır
// Kalan %10 ile pozisyon açılır
let fee_reserve = staking_balance * 9 / 10;
let position_budget = staking_balance - fee_reserve;
```

---

### 3.4 `src/planner.rs` (62 satır)

**Genel Amaç:** Transaction planla ve gönder.

```rust
pub async fn submit(
    clients: &mut Clients,
    plan: TransactionPlan,
) -> anyhow::Result<()> {
    // 1. custody.authorize(plan) → auth_data
    // 2. view.witness_and_build(plan, auth_data) → transaction
    // 3. view.broadcast_transaction(tx, wait_for: Confirmed) → bekle
    Ok(())
}

pub async fn build_and_submit<F>(
    clients: &mut Clients,
    gas_prices: GasPrices,
    f: F,
) -> anyhow::Result<()>
where
    F: FnOnce(&mut Planner) -> &mut Planner,
{
    let mut planner = Planner::new(OsRng);
    planner.set_gas_prices(gas_prices);
    f(&mut planner);
    let plan = planner.plan(&mut clients.view, AddressIndex::new(0)).await?;
    submit(clients, plan).await
}
```

---

### 3.5 `src/swap.rs` (129 satır)

**Genel Amaç:** Sürekli multi-hop swap döngüsü.

```rust
pub async fn swap(
    clients: &mut Clients,
    assets: Vec<AssetId>,
    starting_amount: Amount,
) -> anyhow::Result<()> {
    // Döngü sırası: UM → asset[0] → asset[1] → ... → UM → tekrar
    let cycle = iter::once(UM_ASSET_ID)
        .chain(assets.iter().copied())
        .cycle();

    for (from, to) in cycle.zip(cycle.skip(1)) {
        // 1. swap planla (from → to)
        // 2. submit
        // 3. commitment ile swap record bul (view.swap_by_commitment())
        // 4. pro_rata_outputs() → output amount hesapla
        // 5. swap_claim planla
        // 6. submit

        // UM'ye döndüğünde: eğer bakiye < starting_amount → topla (lqt_vote veya başka kaynak)
    }
}
```

---

### 3.6 `src/vote.rs` (90 satır)

**Genel Amaç:** LQT gauge oylama.

```rust
pub fn stream_epochs(
    clients: &mut Clients,
) -> impl Stream<Item = anyhow::Result<Epoch>> + '_ {
    // try_stream! makrosu
    // Her 30s'de bir current epoch'u kontrol et
    // Yeni epoch'ta → yield
}

pub async fn vote(
    clients: &mut Clients,
    epoch: &Epoch,
    assets: Vec<AssetId>,
) -> anyhow::Result<()> {
    // 1. lqt_vote action'ı planla (her asset için)
    // 2. Mevcut tüm voting note'larını harca (konsolidasyon)
    // 3. submit
}
```

---

## 4. pclientd — Client Daemon

### 4.1 `src/lib.rs` (478 satır)

**Struct:**
```rust
pub struct PclientdConfig {
    pub full_viewing_key: FullViewingKey,
    pub grpc_url: Url,
    pub bind_addr: SocketAddr,
    pub kms_config: Option<KmsConfig>,  // None → view-only mod
}

pub struct KmsConfig {
    // SoftKms için seed phrase veya keys
}
```

**Init Mantığı:**

```rust
pub async fn init(
    home: &Path,
    custody_mode: bool,
) -> anyhow::Result<()> {
    if custody_mode {
        // BIP44 seed phrase sor (24 kelime)
        // SoftKms oluştur
        // Default auth policy:
        //   1. DestinationAllowList (sadece kendi adresleri)
        //   2. OnlyIbcRelay
        //   3. PreAuthorization::Ed25519 (throw-away key)
        let pak = Ed25519SigningKey::generate(&mut OsRng);  // tek kullanımlık!
    } else {
        // FVK sor (bech32 formatında)
    }
    // TOML olarak kaydet
}
```

**gRPC Server:**
```rust
// Tüm servisler tek port'ta:
// tonic_web::enable() → HTTP/1.1 grpc-web + HTTP/2 gRPC ikisi birden
let router = Server::builder()
    .accept_http1(true)  // grpc-web için
    .layer(tonic_web::GrpcWebLayer::new())
    .add_service(ViewServiceServer::new(view_svc))
    .add_optional_service(custody_svc.map(CustodyServiceServer::new))
    // 11 proxy servis...
    .serve(bind_addr);
```

**Önemli Tracing Direktifi:**
```rust
// r1cs=off direktifi
// ark-groth16 trace statements >16GB memory kullanıyor
// RUST_LOG'a r1cs=off eklenmeli
```

**`determine_registry_url()`:**
```rust
fn determine_registry_url(base_url: &Url, chain_id: &str) -> Url {
    if base_url.as_str().ends_with(".json") {
        base_url.clone()
    } else {
        base_url.join(&format!("{chain_id}.json")).unwrap()
    }
}
```

**`download_registry_to_temp_file()`:**
- `file://` scheme → doğrudan dosya oku
- `http://` / `https://` → reqwest ile indir
- Temp dosyaya yaz, path döndür

---

### 4.2 `src/proxy.rs` (80+ satır)

**Genel Amaç:** 11 adet gRPC proxy servisi. Her biri Tower `Service<http::Request<BoxBody>>` implement eder.

**Proxy Türleri ve NAME Sabitleri:**
```rust
AppQueryProxy            // "penumbra.core.app.v1.QueryService"
GovernanceQueryProxy     // "penumbra.core.component.governance.v1.QueryService"
DexQueryProxy            // "penumbra.core.component.dex.v1.QueryService"
DexSimulationProxy       // "penumbra.core.component.dex.v1.SimulationService"
SctQueryProxy            // "penumbra.core.component.sct.v1.QueryService"
FeeQueryProxy            // "penumbra.core.component.fee.v1.QueryService"
ShieldedPoolQueryProxy   // "penumbra.core.component.shielded_pool.v1.QueryService"
ChainQueryProxy          // "penumbra.core.component.chain.v1.QueryService"
StakeQueryProxy          // "penumbra.core.component.stake.v1.QueryService"
CompactBlockQueryProxy   // "penumbra.core.component.compact_block.v1.QueryService"
TendermintProxyProxy     // "penumbra.util.tendermint_proxy.v1.TendermintProxyService"
```

**Ortak `proxy()` Fonksiyonu:**
```rust
async fn proxy(
    channel: Channel,
    req: http::Request<BoxBody>,
) -> Result<http::Response<BoxBody>, Infallible> {
    match channel.oneshot(req).await {
        Ok(resp) => Ok(resp),
        Err(e) => Ok(http::Response::builder()
            .status(500)
            .body(BoxBody::new(e.to_string()))
            .unwrap()),
    }
}
```

`Error=Infallible` — proxy hatası 500 HTTP response'a dönüştürülür, asla panic yapmaz.

---

## 5. pmonitor — Balance Audit Aracı

### 5.1 `src/main.rs` (590 satır)

**Sabitler:**
```rust
const ALLOWED_DISCREPANCY: f64 = 0.1;    // 0.1 UM = 100_000 uUM
const MAX_CB_SIZE_BYTES: usize = 12 * 1024 * 1024;  // 12 MB compact block limit
```

**Ana Komutlar:**
```rust
enum SubCommand {
    Init {
        fvks: Vec<FullViewingKey>,
        grpc_url: Url,
        home: PathBuf,
    },
    Audit {
        home: PathBuf,
    },
}
```

**`init()` Akışı:**
```rust
async fn init(fvks: Vec<FullViewingKey>, grpc_url: Url, home: PathBuf) -> anyhow::Result<()> {
    // Her FVK için:
    //   1. Uuid::new_v4() → wallet_id
    //   2. pcli view reset-sync → wallet_{uuid}/ dizini oluştur
    //   3. scan_genesis_block(fvk, grpc_url) → genesis_balance
    //   4. AccountConfig::new(FvkEntry{fvk, wallet_id}, genesis_balance)
    // PmonitorConfig::new(grpc_url, accounts) → pmonitor.toml
}
```

**`audit()` Akışı:**
```rust
async fn audit(home: PathBuf) -> anyhow::Result<()> {
    let config = PmonitorConfig::from_toml(&home)?;

    for (i, account) in config.accounts().iter().enumerate() {
        // 1. pcli view sync (wallet_{uuid}/)
        // 2. check_for_migration(&account, &view_client) → Option<FvkEntry>
        // 3. compute_um_equivalent_balance(&account) → current_balance
        // 4. check_wallet_compliance(genesis_balance, current_balance)
    }
}
```

**Migration Tespiti:**
```rust
async fn check_for_migration(
    account: &AccountConfig,
    view: &mut ViewClient,
) -> anyhow::Result<Option<FvkEntry>> {
    let active_fvk = account.active_fvk();

    // SQL: SELECT * FROM transactions WHERE memo LIKE 'Migrating balance from {active_fvk}%'
    let matching_txs = view.transactions_matching_memo(
        &format!("Migrating balance from {}%", active_fvk)
    ).await?;

    if let Some(tx) = matching_txs.first() {
        let dest_fvk = parse_dest_fvk_from_memo(&tx.memo)?;
        let new_wallet_id = Uuid::new_v4();
        // Yeni pcli wallet oluştur, sync et
        return Ok(Some(FvkEntry { fvk: dest_fvk, wallet_id: new_wallet_id }));
    }

    Ok(None)
}
```

**`compute_um_equivalent_balance()`:**
```rust
async fn compute_um_equivalent_balance(
    view: &mut ViewClient,
    fvk: &FullViewingKey,
) -> anyhow::Result<Amount> {
    let notes = view.notes(NotesRequest::default()).await?;
    let mut total_um: u128 = 0;

    for note in notes {
        if note.asset_id == UM_ASSET_ID {
            total_um += note.amount.value();
        } else if is_delegation_token(&note.asset_id) {
            // validator_id'den RateData çek
            let rate_data = view.validator_rate(validator_id).await?;
            // del_UM → UM dönüşümü
            let um_amount = rate_data.unbonded_amount(note.amount);
            total_um += um_amount.value();
        }
        // Diğer token'lar sayılmaz (sadece UM ve del_UM)
    }

    Ok(Amount::from(total_um))
}
```

**`check_wallet_compliance()`:**
```rust
fn check_wallet_compliance(
    genesis_balance: Amount,
    current_balance: Amount,
) -> ComplianceResult {
    if current_balance >= genesis_balance {
        return ComplianceResult::Compliant;
    }

    let discrepancy_uUM = genesis_balance.value() - current_balance.value();
    let discrepancy_UM = discrepancy_uUM as f64 / 1_000_000.0;

    if discrepancy_UM <= ALLOWED_DISCREPANCY {
        // 0.1 UM'den küçük fark → muhtemelen fee/rounding
        ComplianceResult::Compliant
    } else {
        ComplianceResult::NonCompliant { discrepancy: discrepancy_UM }
    }
}
```

---

### 5.2 `src/config.rs` (118 satır)

**Struct:**
```rust
pub struct FvkEntry {
    pub fvk: FullViewingKey,
    pub wallet_id: Uuid,
}

pub struct AccountConfig {
    original: FvkEntry,
    genesis_balance: Amount,
    migrations: Vec<FvkEntry>,
}

pub struct PmonitorConfig {
    grpc_url: Url,
    accounts: Vec<AccountConfig>,
}
```

**`AccountConfig` Metodları:**
```rust
impl AccountConfig {
    pub fn new(original: FvkEntry, genesis_balance: Amount) -> Self { ... }
    pub fn original_fvk(&self) -> FullViewingKey { self.original.fvk.clone() }
    pub fn genesis_balance(&self) -> Amount { self.genesis_balance }
    pub fn add_migration(&mut self, fvk_entry: FvkEntry) { self.migrations.push(fvk_entry) }

    pub fn active_wallet(&self) -> FvkEntry {
        if self.migrations.is_empty() {
            self.original.clone()
        } else {
            self.migrations.last().expect("migrations must not be empty").clone()
        }
    }

    pub fn active_fvk(&self) -> FullViewingKey { self.active_wallet().fvk }
    pub fn active_uuid(&self) -> Uuid { self.active_wallet().wallet_id }
}
```

**`parse_dest_fvk_from_memo()`:**
```rust
pub fn parse_dest_fvk_from_memo(memo: &str) -> Result<FullViewingKey> {
    let re = Regex::new(r"Migrating balance from .+ to (.+)")?;
    if let Some(captures) = re.captures(memo) {
        if let Some(dest_fvk_str) = captures.get(1) {
            return dest_fvk_str.as_str().parse::<FullViewingKey>()
                .map_err(|_| anyhow!("Invalid destination FVK in memo"));
        }
    }
    Err(anyhow!("Could not parse destination FVK from memo"))
}
```

---

### 5.3 `src/genesis.rs` (117 satır)

**Genel Amaç:** Genesis bloğunu (height=0) tara ve bakiye hesapla.

```rust
pub async fn scan_genesis_block(
    fvk: &FullViewingKey,
    grpc_url: &Url,
) -> anyhow::Result<Amount> {
    // 1. grpc_url'den genesis compact block çek (height=0)
    assert_eq!(block.height, 0, "genesis block must be height 0");

    // 2. Sadece StatePayload::Note işle (swap yok, rolled-up commitment yok genesis'te)
    let note_payloads: Vec<NotePayload> = block.state_payloads
        .into_iter()
        .filter_map(|p| match p { StatePayload::Note { note, .. } => Some(note), _ => None })
        .collect();

    // 3. Her note için parallel tokio::spawn ile FVK ile deneme şifre çözme
    let handles: Vec<_> = note_payloads.iter().map(|note| {
        let fvk = fvk.clone();
        let note = note.clone();
        tokio::spawn(async move {
            fvk.incoming().key().decrypt_note(note)
        })
    }).collect();

    // 4. Başarılı decrypt → note'u topla
    let mut total = Amount::zero();
    for handle in handles {
        if let Ok(Ok(note)) = handle.await {
            if note.asset_id() == UM_ASSET_ID || is_delegation_token(note.asset_id()) {
                total = total + note.value().amount;
            }
        }
    }

    Ok(total)
}
```

**Genesis'e Özel Sabit:**
```rust
// Delegation token'ı genesis'te 1:1 UM olarak say
let base_rate = BaseRateData { base_exchange_rate: 1_0000_0000 };
// Henüz validator epoch'u yok, exchange rate 1.0
```

**Tasarım Kararları:**
- `tokio::spawn` ile paralel şifre çözme — genesis'teki binlerce note için performans kritik
- Sadece height=0 assert edilir — diğer height'larda çağrılması hata
- Swaps ve rollup commitment'lar genesis'te yoktur → `StatePayload::Note` filtreleme yeterli

---

## 6. Nova Shield Chain İçin Dersler

### 6.1 Node Architecture (pd'den)

**Daemon Pattern:**
```rust
// tek binary, çoklu servis, tokio::select!
tokio::select! {
    result = abci_server => { /* CometBFT köprüsü */ },
    result = grpc_server => { /* tüm query servisleri */ },
    result = metrics_server => { /* prometheus */ },
}
```

Nova Shield için: `nova-shield-node` binary → P2P + gRPC + REST + Prometheus tek select!

**Embedded UI:**
```rust
const DASHBOARD_BYTES: &[u8] = include_bytes!("dashboard.zip");
// Build time'da embed → dağıtım tek binary
```

**Migration Framework:**
Migration trait kesinlikle kullanılmalı. 3-phase: prepare → migrate → complete.

---

### 6.2 Scheduler Health Monitoring

```rust
// pd sleep_worker'dan direkt kopyalanabilir
counter!("nova_async_sleep_drift_microseconds").increment(drift as u64);
```

Kritik: tokio runtime tıkanıklığı erken tespiti.

---

### 6.3 DEX Explorer (pindexer dex_ex'den)

**Candlestick Yapısı:**
```rust
pub struct Candle { open, close, low, high, direct_volume, swap_volume }
pub enum Window { W1m, W15m, W1h, W4h, W1d, W1w, W1mo }
```

7 zaman penceresi standart. `ON CONFLICT DO UPDATE` ile idempotent insert.

**Nova Shield DEX Explorer için SQL:**
```sql
CREATE TABLE nova_dex_price_charts (
    asset1 BYTEA NOT NULL,
    asset2 BYTEA NOT NULL,
    window TEXT NOT NULL,
    start_time TIMESTAMPTZ NOT NULL,
    open FLOAT8, close FLOAT8, low FLOAT8, high FLOAT8,
    volume FLOAT8,
    UNIQUE(asset1, asset2, window, start_time)
);
```

---

### 6.4 Supply Tracking

```rust
const BPS_SQUARED: u128 = 1_0000_0000;
// Tüm oran hesapları BPS^2 ile → 8 ondalık hassasiyet
```

Delegated/unstaked ayrımı zorunlu. VCB (Value Circuit Breaker) her bileşen için.

---

### 6.5 IBC Signed Flow

```sql
-- NUMERIC signed trick
UPDATE flows SET amount = amount + ($negate::NUMERIC * $amount::NUMERIC)
```

Bu pattern `i128` yetersizliğini zarif çözer.

---

### 6.6 Governance State Machine (JSONB)

```json
{"status": "voting"}
{"status": "finished", "outcome": "passed"}
{"status": "claimed", "outcome": "passed"}
```

JSONB state machine — Rust enum ↔ PostgreSQL JSONB serialization. Nova Shield governance için aynı pattern.

---

### 6.7 LP Otomasyon (elcuity'den)

**Midpoint/Fee Hesabı:**
```
midpoint = (buy_price + sell_price) / 2
fee = (buy_price / midpoint) - 1
```

Bu formül market spread'den otomatik fee tespit eder. Nova Shield AMM için geçerli.

**Float → Rational:**
```rust
// price_to_p_q: bit-doubling loop
// Herhangi bir float price'ı p/q rasyoneline çevirir
// Limit: q < 2^60
```

---

### 6.8 Multi-Service Single Port (pclientd'den)

```rust
// tonic_web::enable() → HTTP/1.1 grpc-web + HTTP/2 gRPC
Server::builder()
    .accept_http1(true)
    .layer(tonic_web::GrpcWebLayer::new())
    .add_service(...)
    .serve(bind_addr)
```

Tarayıcı uyumluluğu için HTTP/1.1 grpc-web desteği zorunlu.

**r1cs=off Tracing:**
```
RUST_LOG="...,r1cs=off"
```
ark-groth16 ZK proof trace log'ları >16GB bellek tüketir. Mutlaka kapatılmalı.

---

### 6.9 Balance Audit Sistemi (pmonitor'dan)

**ALLOWED_DISCREPANCY Pattern:**
```rust
const ALLOWED_DISCREPANCY: f64 = 0.1;  // fee/rounding için tolerans
```

Token migration chain takibi → `migrations: Vec<FvkEntry>` — her migration zincire eklenir.

**Genesis Paralel Scan:**
```rust
tokio::spawn(async move { fvk.decrypt(note) })
```

Büyük genesis'lerde performans kritik.

---

### 6.10 Network Join (pd network/join.rs'den)

**Peer Discovery:**
```rust
// "seed" moniker → seed node olarak işaretle
// Threshold: 5 regular peer + sınırsız seed
// RFC1918 filtrele
```

**Address Parsing:**
```rust
// CometBFT listener format: "Listener(@tcp://0.0.0.0:26656)"
let re = Regex::new(r"Listener\(.*@(tcp://)?(.*)\)")?;
```

---

## ÖZET TABLO

| Crate | Dosya Sayısı | Ana Katkı |
|-------|-------------|-----------|
| pd | 30 | Daemon, migration, network, metrics |
| pindexer | 18 | PostgreSQL event indexing, DEX explorer, governance |
| elcuity | 7 | LP otomasyon, LQT voting, swap döngüsü |
| pclientd | 5 | Client daemon, proxy, config |
| pmonitor | 7 | Balance audit, migration tracking, genesis scan |
| **TOPLAM** | **67** | |

**Doğrulanmış Kritik Değerler (kaynak koddan):**
- ALLOWED_DISCREPANCY = 0.1 UM (100,000 uUM) — pmonitor/main.rs
- MAX_CB_SIZE_BYTES = 12 MB — pmonitor/main.rs
- BPS_SQUARED = 1_0000_0000 — pindexer/supply.rs
- POSITION_CHUNK_SIZE = 5 — elcuity/lp/mod.rs
- ADJUST_WAIT_SECS = 120 — elcuity/lp/mod.rs
- dex_ex_min_liquidity = 100_000_000 — pindexer/lib.rs
- block_time_s = 5.1 — pindexer/lib.rs
- peer threshold = 5 — pd/network/join.rs
- initial_height = pre_upgrade_height + 1 — pd/migrate/migrate2/framework.rs
- evidence max_age = 130,000 blocks / 650,000 seconds — pd/network/generate.rs
- MAX_BLOCK_TXS_PAYLOAD_BYTES = 12 MB — pd/network/generate.rs
- DEFAULT_FEE_BPS = 500 (5%) — elcuity/lp/mod.rs
