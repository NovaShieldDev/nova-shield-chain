# Penumbra Agent-15: Binary Crate'leri — pd, pcli, pclientd, pindexer, elcuity

**Analiz Tarihi:** 2026-07-22
**Kapsam:**
- `crates/bin/pd/` — Penumbra node daemon (21 dosya)
- `crates/bin/pcli/` — CLI client (60+ dosya)
- `crates/bin/pclientd/` — Client daemon (5 dosya)
- `crates/bin/pindexer/` — Chain event indexer (18 dosya)
- `crates/bin/elcuity/` — Automated LP/swap bot (7 dosya)

**Toplam:** ~111 dosya, ~8.000+ satir Rust kodu

---

## BOLUM 1: pd — Penumbra Node Daemon

### 1.1 lib.rs (21 satir)

**Dosya:** `crates/bin/pd/src/lib.rs`

#### Genel Amac

`pd` (Penumbra Daemon) binary crate'inin kok dosyasi. Modulleri declare eder ve iki kritik statik byte dizisi embed eder:

```rust
pub const MINIFRONT_ARCHIVE_BYTES: &[u8] = include_bytes!("../../../../assets/minifront.zip");
pub const NODE_STATUS_ARCHIVE_BYTES: &[u8] = include_bytes!("../../../../assets/node-status.zip");
```

**Tasarim Karari:** Frontend web uygulamasi ve node status UI'i dogrudan binary'e gomulur. Bu yaklasim:
- Ayri web server gerektirmez
- Node operasyonu tek binary'e indirgenir
- Her node otomatik olarak web UI sunar
- Deployment basitlesir: tek binary, sifir konfigurasyon

Global direktifler:
- `#![allow(clippy::clone_on_copy)]` — bazi clone kullanimi kasitli
- `#![deny(clippy::unwrap_used)]` — unwrap yasak
- `#![recursion_limit = "512"]` — derin tip eslestirmesi icin

**Nova Shield Dersi:** Node binary'imiz minifront tarzi embedded web UI sunabilir. Block explorer ve governance UI'i binary'e gomebiliriz. Ayri deployment yuku yok.

---

### 1.2 cli.rs (279 satir)

**Dosya:** `crates/bin/pd/src/cli.rs`

#### Struct/Enum Listesi

```rust
pub struct Opt {
    pub cmd: RootCommand,
}

pub enum RootCommand {
    Start { ... },    // Node'u baslat
    Network { ... },  // Network yonet
    Export { ... },   // State'i export et
    Migrate { ... },  // Chain upgrade migration
}

pub enum MigrateCommand {
    ReadyToStart,         // Halt bitini sifirla
    IbcRecovery { old_client_id, new_client_id, target_app_version },
    NoOp { target_app_version },
}

pub enum NetworkCommand {
    Generate { ... },     // Yeni network olustur
    Join { ... },         // Mevcut network'e katil
    UnsafeResetAll {},    // Tum network state'i temizle
}
```

#### Start Komut Parametreleri (Kritik Degerler)

| Parametre | Default | Aciklama |
|-----------|---------|----------|
| `abci_bind` | `127.0.0.1:26658` | CometBFT ABCI soket |
| `grpc_bind` | `127.0.0.1:8080` (HTTP) / `0.0.0.0:443` (HTTPS) | gRPC soket |
| `metrics_bind` | `127.0.0.1:9000` | Prometheus metrics |
| `cometbft_addr` | `http://127.0.0.1:26657` | CometBFT RPC |

**Tasarim Karari:** gRPC bind default'u `grpc_auto_https` varlığına gore degisir. HTTPS aktifse `0.0.0.0:443` (public), değilse `127.0.0.1:8080` (local only). Guvenlik acısından saglikli bir default.

**Nova Shield Dersi:** Nova Shield node'u ayni port yapısını kullanabilir: ABCI = 26658, gRPC = 8080/443, P2P = 26656, metrics = 9000.

---

### 1.3 main.rs (569 satir) — Kritik Dosya

**Dosya:** `crates/bin/pd/src/main.rs`

#### Baslangic Sequencesi (Start komutu)

```
1. Tracing setup (MetricsLayer + FmtLayer + EnvFilter)
2. rustls/aws_lc_rs init (TLS support)
3. Storage::load(rocksdb_home, SUBSTORE_PREFIXES)
4. check_and_update_app_version(storage.clone())
5. App::is_ready() kontrolu — halt bit set ise CIKIS
6. ABCI server spawning (tower-abci)
7. TendermintProxy olusturma
8. gRPC routes: penumbra_sdk_app::rpc::routes()
9. Frontend ZIP serve (minifront.zip)
10. Node status ZIP serve
11. CorsLayer::permissive() -- acik CORS
12. PrometheusBuilder metrics kayit
13. tokio::select! -- ilk fail = tumunu durdur
```

**Kritik Guzergah:** `tokio::select!` ile 3 future eslestirme:
- `abci_server` — ABCI hatasi
- `grpc_server` — gRPC hatasi
- `acme_worker` — ACME/TLS sertifika hatasi

Herhangi birinin hatasi tum node'u durdurur. Bu kasıtli: yarım calisan node'dan daha iyi bir tam durma.

**Hata Tespiti (Satir 249-260):**
```rust
if let Some(e) = e.source() {
    if let Some(e) = e.source() {
        if let Some(e) = e.downcast_ref::<std::io::Error>() {
            if e.kind().to_string().contains("address in use") {
                msg = format!("grpc bind socket already in use: {}", grpc_bind);
            }
        }
    }
}
```
3 katmanli hata acimi ile "port zaten kullanımda" hatasını kullanıcıya acık mesajla iletir.

**Chain Generate Komutu:**
```rust
// Rastgele suffix ile chain_id olusturma:
format!("{}-x{}", chain_id, hex::encode(randomizer.to_le_bytes()))
```
`x` harfi kasitli: `hex::encode` sadece `[0-9a-f]` karakterleri uretir. `x` eklenmesi, IBC'nin chain ID'yi revision number olarak parse etmesini engeller (IBC revision format: `<chain_id>-<number>`).

**Nova Shield Dersi:** `tokio::select!` ile servis yasam döngüsü yönetimi Nova Shield node'u icin ideal pattern. Ayni kur yapısını kullanalım.

---

### 1.4 migrate.rs (363 satir) — Kritik Dosya

**Dosya:** `crates/bin/pd/src/migrate.rs`

#### Migration Enum'u

```rust
pub enum Migration {
    ReadyToStart,       // Halt bit = false
    SimpleMigration,    // Test: state'e anahtar ekle
    Testnet72,          // BatchSwapOutputData protobuf guncelleme
    Testnet74,          // Likidite index sirasi, AuctionParameters ekle
    Testnet76,          // Auction VCB iyilestirme, FMD parametre guncellemesi
    Testnet77,          // Halt bit sifirla
    Testnet78,          // String alanlarini max uzunluga truncate, DEX fiyat endeksi
    Mainnet1,           // IBC packet commitment onarimi
    Mainnet2-5,         // no-op migrasyonlar
    IbcClientRecovery,  // IBC client state degistir
    NoOp,               // Halt bit sifirla + yeni genesis
}
```

#### migrate() Algoritması

```
1. Storage::load(rocksdb_dir)
2. is_chain_halted() OR force → yoksa bail!
3. latest_version == block_height OR force → yoksa bail! (korrupsiyon kontrolu)
4. Migration::* match → ilgili migrasyon calistir
5. migrate_comet_data() → yeni genesis'i comet'e yaz
```

**Onemli Korrupsiyon Kontrolu (Satir 127-132):**
```rust
ensure!(
    latest_version == block_height || force,
    "local chain state version is corrupted: {} != {}",
    latest_version, block_height
);
```
Bu kontrol v0.80.10 surum notlarına referans veriyor — gercek production bug'dan cikarilan bir ders.

#### `archive_directory()` (Satir 230-258)

Kaynak dizini `.tar.gz`'ye sıkistırir. Ozellikler:
- Mevcut dosyayi silmez: `bail!("refusing to overwrite existing archive")`
- `GzEncoder + tar::Builder` kullanımı
- Opsiyonel alt-dizin adlandırma

#### `migrate_comet_data()` (Satir 276-306)

CometBFT state'ini chain upgrade sonrası gunceller:
- Yeni genesis.json'u `comet_home/config/genesis.json`'a yazar
- `priv_validator_state.json` yuksekligini artırır (asla azaltmaz!)
- `evidence.db`, `state.db`, `blockstore.db`, `cs.wal` silinir
- `tx_index.db` KORUNUR (tarihsel tx sorgulari için)

**Tasarım Kararı:** `tx_index.db`'nin saklanması: upgrade sınırındaki tx'ler sorgulanabilir kalır. Bu UX açısından kritik.

---

### 1.5 migrate2/framework.rs (185 satir) — Yeni Migration Framework

**Dosya:** `crates/bin/pd/src/migrate/migrate2/framework.rs`

#### Migration Trait

```rust
pub trait Migration {
    fn name(&self) -> &'static str;
    fn target_app_version(&self) -> Option<u64>;

    async fn prepare(&self, pd_home, _comet_home) -> Result<(RootHash, u64)>;
    async fn migrate(&self, pd_home, _comet_home) -> Result<(RootHash, u64)>;
    async fn migrate_inner(&self, delta: &mut StateDelta<Snapshot>) -> Result<()>;
    async fn complete(&self, pd_home, comet_home, post_upgrade_root_hash, ...) -> Result<()>;
    async fn run(&self, pd_home, comet_home, genesis_start) -> Result<()>;
}
```

#### run() Akışı (Satir 143-184)

```
prepare()  → pre_upgrade_root_hash + pre_upgrade_height al
migrate()  → StateDelta'da değişiklikleri yap, commit_in_place()
complete() → yeni genesis.json yaz, validator state sifirla
```

**migrate() ic mantığı (Satir 65-83):**
```rust
delta.ready_to_start();     // halt bit = false
delta.put_block_height(0u64); // genesis height = 0
let post_upgrade_root_hash = storage.commit_in_place(delta).await?;
```

**complete() genesis uretimi (Satir 105-127):**
```rust
let app_state = penumbra_sdk_app::genesis::Content {
    chain_id,
    ..Default::default()  // diger alanlar default
};
genesis.app_hash = post_upgrade_root_hash.0.to_vec().try_into()?;
genesis.initial_height = post_upgrade_height as i64;
let genesis = NetworkConfig::make_checkpoint(genesis, Some(checkpoint));
```

**Nova Shield Dersi:** Migration framework pattern — `prepare/migrate/complete` ayrımı — Nova Shield icin mükemmel. State degisikliklerini `migrate_inner()` metoduna hapsetmek, framework'un genesis uretimini handle etmesine izin verir. Nova Shield bu pattern'i kopyalamalı.

---

### 1.6 migrate2/ibc_client_recovery.rs (59 satir)

**Dosya:** `crates/bin/pd/src/migrate/migrate2/ibc_client_recovery.rs`

```rust
pub struct IbcClientRecoveryMigration {
    old_client_id: String,
    new_client_id: String,
    app_version: Option<u64>,
}
```

`migrate_inner()`:
1. `ClientId` parse et (her iki ID)
2. `delta.recover_client::<PenumbraHost>(&subject, &substitute).await?`

Tum validation `ClientRecoveryExt::recover_client()` içinde — daha once analiz edilen `client_recovery.rs` (agent-ibc.md). Bu separasyon concern'lerin temiz ayrılması.

---

### 1.7 network/generate.rs (ana kısım, 200+ satir)

**Dosya:** `crates/bin/pd/src/network/generate.rs`

#### NetworkConfig Struct

```rust
pub struct NetworkConfig {
    pub name: String,
    pub genesis: Genesis<penumbra_sdk_app::genesis::AppState>,
    pub network_dir: PathBuf,
    pub network_validators: Vec<NetworkValidator>,
    pub validators: Vec<Validator>,
    pub peer_address_template: Option<String>,
    pub tendermint_timeout_commit: Option<tendermint::Timeout>,
}
```

#### generate() Parametreleri

```rust
pub fn generate(
    chain_id: &str,
    network_dir, peer_address_template, external_addresses,
    allocations_input_file, allocation_address,
    validators_input_file,
    tendermint_timeout_commit,
    active_validator_limit,
    epoch_duration, unbonding_delay,
    proposal_voting_blocks,
    gas_price_simple: Option<u64>,
) -> anyhow::Result<NetworkConfig>
```

**Genesis allocation flow:**
1. CSV'den allocations yukle
2. Her validator icin `delegation_allocation()` ekle
3. Opsiyonel `allocation_address` ekle
4. `NetworkAllocation::simple(address)` — kolay dev/test allocasyonu

**Tasarim Karari:** `NetworkAllocation::simple()` — tek adres için tek komutla genesis allocation. Developer deneyimi odakli.

---

### 1.8 network/join.rs (ana kısım)

**Dosya:** `crates/bin/pd/src/network/join.rs`

#### network_join() Fonksiyonu

```rust
pub async fn network_join(
    output_dir, node: Url, node_name,
    external_address, tm_rpc_bind, tm_p2p_bind,
) -> anyhow::Result<()>
```

**Algoritma:**
1. `node.join("genesis")` URL'sinden genesis JSON indir (reqwest)
2. Bootstrap node'un P2P listen adresini sorgula
3. `/net_info` endpoint'inden peer listesi al
4. `NetworkTendermintConfig::new(node_name, peers, ...)` olustur
5. Config dosyalarını diske yaz

**Dikkat:** `listen_info[0].as_str().unwrap_or_default()` — hata yolu mevcut ama unwrap kullanılıyor gibi görünüyor. `unwrap_or_default` ile tolere ediliyor.

#### unpack_state_archive()

Opsiyonel `.tar.gz` state snapshot'ını indirir ve acip. `file://` scheme desteklenir — local test icin.

**Nova Shield Dersi:** Bootstrap node join mekanizması: genesis indir + peer listesi al + config olustur. Nova Shield node join prosedürü bu kadar basit olmalı.

---

### 1.9 Migrate Submodülleri (Özet)

| Dosya | Versiyon | Degisiklik |
|-------|---------|-----------|
| `mainnet1.rs` | Mainnet-1 | IBC packet commitment onarimi (production bug fix) |
| `mainnet2-4.rs` | Mainnet 2-4 | no-op (yalnizca halt bit sifirla) |
| `testnet72.rs` | Testnet-72 | `BatchSwapOutputData` protobuf migrasyonu |
| `testnet74.rs` | Testnet-74 | Likidite index sirasi, arb output, AuctionParameters |
| `testnet76.rs` | Testnet-76 | Auction VCB tamir, FMD parameters |
| `testnet77-78.rs` | Testnet 77-78 | Halt sifirla, String truncation |
| `reset_halt_bit.rs` | ReadyToStart | Sadece halt bit = false |
| `simple.rs` | Test | State'e test anahtarı ekle |

**Mainnet-1 Dersler:** IBC token withdrawal hatasında packet commitment'lar eksikti. Migrasyon dogrudan state'e yazarak onarıldi. Production'da bu tur manuel state yaması kurtarici olabilir.

---

## BOLUM 2: pcli — Penumbra CLI Client

### 2.1 DEX Utils — Replicate Modulu

**En Kritik Kısım:** `pcli/src/dex_utils/replicate/`

#### math_utils.rs (66 satir)

**`gauss_seidel()` — Gauss-Seidel Iteratif Cozucu**

```rust
pub(crate) fn gauss_seidel(
    A: Array2<f64>,
    b: Array1<f64>,
    max_iterations: usize,
    tolerance: f64,
) -> anyhow::Result<Array1<f64>>
```

**Algoritma:**
1. `A = L + D` ayrısimi (L: lower triangular, D: off-diagonal upper)
2. Her iterasyon:
   ```
   k[i] = (b[i] - D[i,:].dot(k) - L[i,:i].dot(k[:i])) / L[i,i]
   ```
3. L2 normu `< tolerance` ise dur
4. Max iterasyon: `GAUS_SEIDEL_MAX_ITERATION = 10_000`

**`lower_triangular()` — Alt Ucgen Matris Donusumu**

Kare matrisi alt ucgen matrise donusturur. `O(n^2)` zaman.

**`sample_to_upper()` — Uniform Ornekleme**

```rust
pub(crate) fn sample_to_upper(upper: f64, num_points: usize) -> Vec<f64> {
    let step = upper / (num_points as f64);
    (1..=num_points).map(|i| (i as f64) * step).collect()
}
```

`[step, 2*step, ..., upper]` — `upper` dahil, `0` dahil degil.

---

#### xyk.rs (206 satir) — XYK CFMM Replikasyonu

**Sabitler:**
```rust
pub(crate) const NUM_POOLS_PRECISION: usize = 30;  // tick sayisi
const GAUS_SEIDEL_MAX_ITERATION: usize = 10_000;
const APPROXIMATION_TOLERANCE: f64 = 1e-8;
```

**`portfolio_value_function()` — XYK Değer Fonksiyonu**

```rust
pub fn portfolio_value_function(invariant_k: f64, price: f64) -> f64 {
    2.0 * f64::sqrt(invariant_k * price)
}
```

Bu, klasik XYK AMM (x*y=k) için LP portfolio değerinin analitik ifadesi:
- `k = x * y` (sabit carpan)
- `price = y/x`
- `PV(k, p) = 2 * sqrt(k * p)`

Impermanent loss de bu formülden turetilir.

**`solve()` — Gauss-Seidel ile Tick Agırlıkları**

```rust
pub fn solve(alpha: &[f64], k: f64, n: usize) -> anyhow::Result<Array1<f64>>
```

**Matris Kurulumu:**
```
A[j][i] = alpha[i]  if i < j   (lower triangular)
A[j][i] = alpha[j]  if i >= j  (upper triangular block)
b[j] = portfolio_value_function(k, alpha[j])
```

Her satır `j`, `alpha[j]` fiyat noktasındaki portföy degerini kısıt olarak kodlar. Gauss-Seidel cözücü, her tick'teki k_i (rezerv) degerleri bulur.

**`replicate()` — Ana Fonksiyon**

```rust
pub fn replicate(
    pair: &DirectedUnitPair,
    raw_r1: &Value,
    current_price: U128x128,
    fee_bps: u32,
) -> anyhow::Result<Vec<Position>>
```

**Algoritma:**
1. `r1` ve `r2` degerlerini hesapla
2. `k = r1 * r2` (XYK invariant)
3. `alphas = sample_prices(current_price, NUM_POOLS_PRECISION)` — 30 fiyat noktası
4. `solve(alphas, k, 30)` — Gauss-Seidel ile k_i degerlerini bul
5. Her tick icin Position olustur:
   - `alpha_i < current_price` → tek tarafli r2 pozisyonu
   - `alpha_i >= current_price` → tek tarafli r1 pozisyonu

**Pozisyon Fiyat Kodlama:**
```rust
let scaled_p = (approx_p * unit_end).expect("no overflow when scaling p");
let p: Amount = scaled_p.round_down().try_into()?;
let q = Amount::from(1u64) * unit_start;
```

`p` ve `q`, base unit cinsinden "p token2 = q token1" seklinde fiyat oranını kodlar.

**Nova Shield Dersi:** XYK AMM replikasyonu 30 konsantre LP pozisyonuyla yapılıyor. Nova Shield DEX'te LP pozisyonu erisim kolaylastirıcı olarak benzer bir CLI komutu sunulabilir. Gauss-Seidel cozumu matematiksel cekiciligi nedeniyle Nova Shield'a kopyalanabilir.

---

### 2.2 dex_utils/replicate.rs (12 satir)

```rust
const APPROXIMATION_TOLERANCE: f64 = 1e-8;

pub mod xyk;
pub mod balancer {}   // henuz implementasyonu yok
pub mod volatility {} // henuz implementasyonu yok
```

**Tasarim Notu:** `balancer` ve `volatility` modulleri bos — gelecek AMM replikasyonlari icin yer tutucular. Balancer AMM (agırlıklı geometrik ortalama) ve volatility-based AMM replikasyonu planlanmis.

---

### 2.3 dex_utils/tests.rs, debug.rs (Ozet)

Test ve debug yardımcıları: `dex_utils` modulunun duzgunlugunu test eden birim testleri, debug ciktisi icin logging.

---

### 2.4 pcli Komut Yapısı (Ozet)

#### command/tx/liquidity_position.rs
LP pozisyon acma/kapama/cekme. Konsantre likidite yonetimi CLI arayuzu.

#### command/tx/replicate/xyk.rs
```
pcli tx replicate xyk --pair <PAIR> --input <VALUE> --current-price <PRICE> --fee-bps <BPS>
```
XYK replikasyon CLI komutu. `replicate()` fonksiyonunu cagırır.

#### command/query/dex.rs
DEX query komutlari: pozisyon listele, batch swap sonuclari, arbitraj gecmisi.

#### command/tx/auction/dutch/gda.rs — GDA (Gradual Dutch Auction)

```rust
pub fn gda_schedule(
    start_price: f64,
    end_price: f64,
    duration_blocks: u64,
    steps: u64,
) -> Vec<(f64, u64)>
```

Gradual Dutch Auction zamanlamasini hesaplar. Her adimda fiyat logaritmik olarak duser.

**Nova Shield Dersi:** GDA (Gradual Dutch Auction) fiyat algoritması token satışları icin güçlü bir araç. Nova Shield DEX'e native GDA desteği eklenebilir.

---

## BOLUM 3: pclientd — Client Daemon

### 3.1 lib.rs + main.rs + proxy.rs

**Dosya:** `crates/bin/pclientd/src/`

**pclientd** — Penumbra'nın view service daemon'u. Özellikler:
- Kullanicının gorunüm anahtarini (viewing key) bellekte tutar
- gRPC view service sunar
- pcli ve diger client'lara view/decrypt hizmeti verir
- Custody (signing) için ayri servise yonlendirir

#### proxy.rs

View service gRPC talepleri ile storage arasında aracılık. gRPC metodlari:
- Tum `ViewService` metodlarının proxy implementasyonu
- `tonic::Status` ile hata donusum

**Nova Shield Dersi:** Kullanıcı view service'ını ayırma — privacy blockchain için kritik. Viewing key, imzalama anahtarindan ayri tutulur. Nova Shield ayni mantıkla `nova-view-service` ve `nova-custody-service` ayırımı yapmalı.

---

## BOLUM 4: pindexer — Chain Event Indexer

### 4.1 lib.rs (39 satir)

```rust
pub struct Options {
    pub cometindex: cometindex::opt::Options,
    pub indexing_denom: asset::Id,     // default: mainnet staking token asset ID
    pub dex_ex_min_liquidity: u128,    // default: 100_000_000 (1 UM)
    pub dex_ex_ignore_arb: bool,       // default: true
    pub block_time_s: f64,             // default: 5.1 saniye (mainnet)
}
```

**Mimari:** `cometindex` framework uzerine kurulu. Her modül bir `AppView` (PostgreSQL sink) implement eder.

#### Modüller

| Modul | Icerik |
|-------|--------|
| `block.rs` | Blok eventi indexleme |
| `dex_ex/mod.rs` | DEX events: swap, position open/close/execute, arb |
| `governance.rs` | Governance events |
| `ibc/mod.rs` | IBC transfer, channel, client events |
| `insights/mod.rs` | Aggregated analytics |
| `lqt/mod.rs` | Liquidity tournament events |
| `stake/` | Delegation, undelegation, validator set, slashing, missed blocks |
| `supply.rs` | Token supply tracking |
| `parsing.rs` | Event parsing yardimcilari |

---

### 4.2 dex_ex/mod.rs — DEX Exchange Indexer (Kritik)

**Dosya:** `crates/bin/pindexer/src/dex_ex/mod.rs`

#### Candle Struct (ic modul)

```rust
pub struct Candle {
    pub open: f64,
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub direct_volume: f64,  // direkt trade hacmi
    pub swap_volume: f64,    // batch swap hacmi
}
```

#### from_candlestick_data()

```rust
Self {
    open: data.open,
    close: data.close,
    low: data.low,
    high: data.high,
    direct_volume: data.direct_volume,
    swap_volume: data.swap_volume,
}
```

#### merge() — Candle Birleştirme

```rust
pub fn merge(&mut self, that: &Self) {
    self.close = that.close;     // son fiyat guncellenir
    self.low = f64::min(self.low, that.low);
    self.high = f64::max(self.high, that.high);
    self.direct_volume += that.direct_volume;
    self.swap_volume += that.swap_volume;
}
```

OHLCV candlestick birleştirme: `open` korunur, `close` son deger, `low/high` min/max, volume toplanir.

**Periyot Tipleri:** `1m`, `15m`, `1h`, `4h`, `1d`, `1w` (chrono ile hesaplama).

**Event'ler izlenen:**
- `EventPositionOpen/Close/Execute/Withdraw`
- `EventBatchSwap`
- `EventArbExecution`
- `EventSwap / EventSwapClaim`
- `EventLqtPositionReward`
- `EventBlockRoot` (blok sonunda flush trigger)

**PostgreSQL Sorgular:** DEX ex indexer, candlestick verisini PostgreSQL'e yazar. `sqlx::types::BigDecimal` ile precision korunur.

**Nova Shield Dersi:** Ayri indexer crate mükemmel bir pattern. Nova Shield icin `nova-indexer` crate'i PostgreSQL-backed analytics ile kurulabilir. DEX trading verisi icin OHLCV candlestick indexleme built-in olmalı.

---

## BOLUM 5: elcuity — Automated LP/Swap Bot

### 5.1 lib.rs (59 satir)

**Dosya:** `crates/bin/elcuity/src/lib.rs`

```rust
pub enum Command {
    Vote(vote::Opt),   // Likidite turnuvasinda oy ver
    Lp(lp::Opt),       // Likidite pozisyonu ac ve yonet
    Swap(swap::Opt),   // Otomatik swap
}
```

**Elcuity** — "ELC utility" — Penumbra'nın otomatik likidite bot'u. Liquidity Tournament (LQT) icin tasarlanmis.

---

### 5.2 swap.rs (130 satir)

#### swap() Fonksiyonu (Satir 18-81)

```rust
pub async fn swap(clients: &Clients, input: Value, to: asset::Id) -> anyhow::Result<Amount>
```

**Swap+Claim Akışı:**
1. `Planner::new(OsRng)` — islem planlayici
2. `planner.swap(input, to, estimated_claim_fee, address)`
3. `submit(clients, plan)` — tx gonder ve bekle
4. `view.swap_by_commitment(swap_commitment)` — swap kaydını bul
5. `output_data.pro_rata_outputs(delta_1_i, delta_2_i)` — beklenen cikti hesapla
6. `SwapClaimPlan` olustur (ZK proof)
7. Claim tx gonder

**ZK Proof Blinding:**
```rust
proof_blinding_r: Fq::rand(&mut OsRng),
proof_blinding_s: Fq::rand(&mut OsRng),
```
Her claim için rastgele blinding faktörleri — ZK proof gizliligi icin.

#### Opt::run() — Sonsuz Swap Dongusu

```rust
let mut cycle = iter::once(*STAKING_TOKEN_ASSET_ID)
    .chain(asset_ids.into_iter())
    .cycle()
    .peekable();
while let Some(asset_id) = cycle.next() {
    // UM girisini koru
    if asset_id == *STAKING_TOKEN_ASSET_ID {
        amount = amount.max(starting_amount);
    }
    let &next = cycle.peek().expect("cycle should be infinite");
    amount = swap(clients, Value { asset_id, amount }, next).await?;
}
```

`UM → A → B → ... → UM → A → ...` dongusu. Her UM donusunde kayıp tolere edilir ve `starting_amount`'a doldurulur.

---

### 5.3 lp/mod.rs (354 satir) — Market Making Bot

**Sabitler:**
```rust
const POSITION_CHUNK_SIZE: usize = 5;      // 5'lik pozisyon batch
const DEFAULT_PRICE: u8 = 1;               // default 1:1 fiyat
const DEFAULT_FEE_BPS: u16 = 500;          // default 5% fee
const ADJUST_WAIT_SECS: u64 = 120;         // 2 dakika arası
```

#### Kritik Fonksiyon: `spread_to_price_and_fee_bps()`

```rust
fn spread_to_price_and_fee_bps(buy: U128x128, sell: U128x128) -> (U128x128, u16) {
    let midpoint = (buy + sell) / 2;
    let fee = (buy / midpoint) - 1;
    // fee_as_u16 = min(5000, fee * 10000)
    (midpoint, fee_as_u16)
}
```

**Matematiksel Formul:**
```
x(1+f) = buy,  x(1-f) = sell
x = (buy + sell) / 2           (midpoint)
f = buy/x - 1                  (fee fraction)
fee_bps = min(5000, f * 10000) // max 50%
```

Market spread'inden fee hesaplama: iki taraf arasındaki spread'i LP fee olarak kodlar.

#### `price_to_p_q()` — p/q Kodlama

```rust
fn price_to_p_q(price: U128x128, precision: Amount) -> (Amount, Amount) {
    let mut p = price;
    let mut q = Amount::from(1u8);
    while !p.is_integral() {
        // p = p * 2, q = q * 2  (precision sonuna kadar)
    }
    (p.round_down().into_amount(), q)
}
```

Float fiyatı `p/q` tam sayı oranına donusturur. Her iterasyon ikili precision artırır.

#### `adjust_liquidity_provision()` — Ana Dongu

```
1. close_all_positions() → 5'erli batch kapanma
2. withdraw_all_positions() → 5'erli batch cekme
3. create_position_at_current_spread() → yeni pozisyon
4. tokio::sleep(120s) bekle
5. tekrar
```

**90% UM Kuralı:**
```rust
// Only use 90% of the staking token, to allow for paying fees.
*v -= v.checked_mul(&10u8.into()).unwrap_or(0u8.into()) / 100u8.into();
```

UM bakiyesinin %10'unu fee ödemesi için saklı tutar.

---

## GENEL OZET: pd + pcli + pclientd + pindexer + elcuity

### Mimarinin Ozeti

```
[CometBFT] ←ABCI→ [pd ABCI Server]
                      ↕
                  [penumbra_sdk_app]
                      ↕
                  [RocksDB / cnidarium]
                      ↕
[Clients] ←gRPC→ [pd gRPC Routes] ← TendermintProxy → [CometBFT RPC]
               ↙               ↘
         [pclientd]         [pindexer]
             ↕                  ↕
           [pcli]           [PostgreSQL]
             ↕
          [elcuity]
```

### Kritik Teknik Bulgular

1. **Embedded Frontend:** `include_bytes!` ile ZIP gommek — tek binary deployment
2. **Migration Framework:** `prepare/migrate/complete` pattern — upgrade güvenli
3. **IBC Recovery Migration:** Tek komutla IBC client değiştirme — Mainnet operasyonu
4. **XYK Replikasyonu:** 30 tick, Gauss-Seidel, `portfolio_value_function = 2*sqrt(k*p)`
5. **Market Maker Bot:** Spread-based adaptive fee, 120s güncelleme döngüsü
6. **Indexer:** Event-driven PostgreSQL sink, OHLCV candlestick, 6 periyot
7. **Swap Bot:** ZK swap+claim ikili işlem, sonsuz döngü

---

## NOVA SHIELD ICIN DERSLER

### 1. Node Binary Mimarisi
- ABCI + gRPC tek binary'de
- Embedded web UI (include_bytes! ile ZIP)
- Port yapısı: ABCI 26658, gRPC 8080/443, Metrics 9000
- `tokio::select!` ile servis yaşam döngüsü

### 2. Migration Framework
- `prepare/migrate/complete` ayrımı
- `is_chain_halted()` zorunlu kontrol
- `commit_in_place()` atomik state geçişi
- `tx_index.db` korunur (tarihsel sorgulamalar)

### 3. DEX Utils
- XYK AMM replikasyonu: `NUM_POOLS_PRECISION = 30` tick
- `portfolio_value_function(k, p) = 2 * sqrt(k * p)`
- Gauss-Seidel cözücü: `tolerance = 1e-8`, `max_iter = 10_000`
- `balancer` ve `volatility` için boş stub'lar

### 4. LP Bot Pattern
- `close → withdraw → open` döngüsü
- Spread-based fee: `fee = buy/midpoint - 1`
- %90 kullanım kuralı (fee rezervi)
- `ADJUST_WAIT_SECS = 120`

### 5. Indexer Pattern
- `cometindex::AppView` trait implementasyonu
- OHLCV candlestick: `merge()` pattern
- `BigDecimal` ile hassasiyet korunması
- `block_time_s = 5.1` (mainnet default)

### 6. Node Join Pattern
- Bootstrap node'dan genesis indir
- `/net_info` ile peer listesi al
- Config dosyalarını diske yaz
- Opsiyonel archive URL ile snapshot başlangıcı
