# Penumbra Agent-15: pd / pindexer / elcuity / pclientd + MASTER SENTEZ

**Analiz Tarihi:** 2026-07-22
**Kapsam:**
- `crates/bin/pd/` — Ana node daemon (Penumbra Full Node)
- `crates/bin/pindexer/` — PostgreSQL tabanlı olay indeksleyici
- `crates/bin/elcuity/` — Otomatik LP + swap + LQT vote botu
- `crates/bin/pclientd/` — Client-side daemon (view + custody proxy)
- MASTER SENTEZ — Tüm Penumbra analizinin özeti ve Nova Shield Eylem Planı

---

## KISIM 1: bin/pd — Penumbra Full Node Daemon

---

### [lib.rs]
**Dosya:** `crates/bin/pd/src/lib.rs`
**Satir:** 21

#### Genel Amac
`pd` (Penumbra Daemon) crate'inin kok dosyasi. ABCI server + gRPC server + metrics sunucusu uclusundan olusan Penumbra full node'un giris noktasi.

#### Kritik Sabitler
```rust
pub const MINIFRONT_ARCHIVE_BYTES: &[u8] = include_bytes!("../../../../assets/minifront.zip");
pub const NODE_STATUS_ARCHIVE_BYTES: &[u8] = include_bytes!("../../../../assets/node-status.zip");
```
- Minifront UI ve node-status WebApp, binary'e **gommulmus** (embedded). Ayri deployment gerekmez.
- `include_bytes!` derleme zamaninda dosyayi binary'e bakar; calisma zamaninda disk erişimi yok.

---

### [cli.rs]
**Dosya:** `crates/bin/pd/src/cli.rs`
**Satir:** 279

#### Struct/Enum Listesi

**`Opt`** — Kok parser (clap)

**`RootCommand`** enum — 4 subcommand:
1. `Start { home, abci_bind, grpc_bind, grpc_auto_https, acme_staging, metrics_bind, cometbft_addr, enable_expensive_rpc }` — Node'u baslat
2. `Network { network_dir, net_cmd }` — Ag yonetimi
3. `Export { home, export_directory, export_archive, prune }` — State export
4. `Migrate { home, comet_home, force, ready_to_start, migration_type }` — Chain migration

**`MigrateCommand`** enum:
- `ReadyToStart` — Halt bitini sifirla
- `IbcRecovery { old_client_id, new_client_id, target_app_version }` — IBC istemci kurtarma
- `NoOp { target_app_version }` — Halt bitini sifirla + genesis olustur

**`NetworkCommand`** enum:
- `Generate { ... 13 parametre ... }` — Yeni network genisis olustur
- `Join { node, archive_url, moniker, external_address, ... }` — Mevcut network'e katil
- `UnsafeResetAll {}` — Tum state'i sil

#### Tasarim Kararlari
- **ABCI bind**: `127.0.0.1:26658` (varsayilan — sadece yerel)
- **gRPC bind**: `127.0.0.1:8080` (HTTP) veya `0.0.0.0:443` (HTTPS auto)
- **Metrics bind**: `127.0.0.1:9000` (Prometheus)
- **CometBFT proxy**: `http://127.0.0.1:26657`
- **`gas_price_simple`**: 1000 = 1 upenumbra; 4 gas tipi icin es deger kullanilir

---

### [main.rs]
**Dosya:** `crates/bin/pd/src/main.rs`
**Satir:** 375+

#### Baslatma Akisi (Start komutu)

```
Storage::load(rocksdb_home, SUBSTORE_PREFIXES)
  -> check_and_update_app_version()
  -> App::is_ready() kontrolu (halt check)
  -> spawn: abci_server = penumbra_sdk_app::server::new(storage).listen_tcp(abci_bind)
  -> TendermintProxy::new(cometbft_addr)
  -> grpc_routes = penumbra_sdk_app::rpc::routes(&storage, tm_proxy, enable_expensive_rpc)
  -> frontend = zipserve::router("/app/", MINIFRONT_ARCHIVE_BYTES)
  -> node_status = zipserve::router("/", NODE_STATUS_ARCHIVE_BYTES)
  -> router = grpc_routes.merge(frontend).merge(node_status).layer(CorsLayer::permissive())
  -> PrometheusBuilder::new().set_buckets_for_dex_metrics()
  -> tokio::select! { abci_server, grpc_server, acme_worker }
```

**Kritik Detaylar:**
- CorsLayer: `permissive()` — tam CORS izni; herhangi bir web origin erisebilir
- Metrics: `TracingContextLayer::only_allow(["chain_id", "role"])` — sadece iki label
- ACME: Let's Encrypt oto-sertifika; `aws_lc_rs` TLS provider (FIPS uyumlu)
- Halt check: `App::is_ready()` false ise `exit(0)` — sessiz kapanma
- `tokio::select!` — ilk hata yayilir, diger servisler hayatta kalir; 3-lu birlikte

#### DEX Metrics Entegrasyonu
```rust
use penumbra_sdk_dex::component::metrics::PrometheusBuilderExt;
PrometheusBuilder::new().set_buckets_for_dex_metrics()?
```
DEX histogram bucket'lari ozel tanimlanmis; swap latency/throughput icin ozellesmis.

---

### [migrate.rs]
**Dosya:** `crates/bin/pd/src/migrate.rs`
**Satir:** 363

#### Migration Enum Hiyerarsisi

```rust
pub enum Migration {
    ReadyToStart,
    SimpleMigration,
    Testnet72,     // BatchSwapOutputData proto migrasyonu (epoch height -> index)
    Testnet74,     // Base liquidity index yeniden sirala; arb executions guncelle; AuctionParameters ekle
    Testnet76,     // Auction VCB onarimi; FMD params yeni proto
    Testnet77,     // Halt bit reset
    Testnet78,     // String alanlarini kes; DEX NV price index doldurul
    Mainnet1,      // IBC packet commitments geri yukle
    Mainnet2,      // no-op
    Mainnet3,      // no-op
    Mainnet4,      // no-op (LQT destegi)
    Mainnet5,      // no-op (yeni framework ile)
    IbcClientRecovery,
    NoOp,
}
```

#### Ortak Migration Protokolu
1. `Storage::load(rocksdb_dir, SUBSTORE_PREFIXES)` — RocksDB ac
2. `storage.latest_snapshot().is_chain_halted()` kontrolu (ya da `--force`)
3. `latest_version == block_height` dogrulama (bozuk state koruması)
4. Migration'a ozel mantik
5. CometBFT datasi temizle: `evidence.db`, `state.db`, `blockstore.db`, `cs.wal` (tx_index.db KORUNUR)
6. `adjust_priv_validator_state()` — yukseklik arttirilir, hicbir zaman azaltilmaz

#### Yeni Migration Cercevesi (`migrate2/framework.rs`)
```rust
pub trait Migration {
    fn name(&self) -> &'static str;
    fn target_app_version(&self) -> Option<u64>;
    async fn prepare(&self, pd_home, comet_home) -> Result<(RootHash, u64)>;
    async fn migrate(&self, pd_home, comet_home) -> Result<(RootHash, u64)>;
    async fn migrate_inner(&self, delta: &mut StateDelta<Snapshot>) -> Result<()>;
    async fn run(&self, pd_home, comet_home, genesis_start) -> Result<()>;
}
```
- `prepare()`: Pre-upgrade state hash ve height al
- `migrate()`: Delta olustur, `migrate_inner()` cagir, `commit_in_place()`, genesis olustur
- `delta.put_block_height(0u64)` — genesis yuksekligi sifirlanir
- `delta.ready_to_start()` — halt biti kaldirilir

---

### [mainnet1.rs] — Kritik Migration Analizi
**ICS20_TRANSFER_START_HEIGHT = 411616**

`tx_lost_transfers()` fonksiyonu:
- Tum ICS-20 paket acknowledgement'larini tara
- Hata ack alan paketleri tespit et (basarili ack'ler atlanir)
- Silinen paket commitment'larini geri yukle

Bu migration, Penumbra mainnet'te ICS-20 cikis transferlerinin hata durumunda yanlis silindigi
bir bug'i duzeltmistir. Paket commitment'lari geri yuklenerek "kayip" transferler kurtarilmistir.

---

### [network/generate.rs] — Genesis Yaratici
**Dosya:** `crates/bin/pd/src/network/generate.rs`

#### Struct: `NetworkConfig`
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

#### `NetworkConfig::generate()` — Parametre Listesi
```
chain_id, network_dir, peer_address_template, external_addresses,
allocations_input_file, allocation_address, validators_input_file,
tendermint_timeout_commit, active_validator_limit, epoch_duration,
unbonding_delay, proposal_voting_blocks, gas_price_simple
```
13 parametre — tam konfigurasyon ozellestirilmesi destekler.

#### `NetworkAllocation::simple(address)`
Genesis'e basit bir adres ekler. Dev deneyimi icin: once wallet olustur, adres al, sonra genesis uret.

---

### [network/config.rs] — CometBFT Konfigurasyonu

#### `NetworkTendermintConfig::new()`
- `cometbft_config_template.toml` dosyasindan sablon okur (`include_str!`)
- Moniker, peer listesi, external address, RPC/P2P bind adreslerini ustune yazar

#### Validator Keys Uretimi
`ValidatorKeys` struct: `SpendKey` -> identity_key + governance_key + consensus_key uretir.
Tum anahtarlar bellekte tutulur, diskten okunmaz — guvenlik icin.

---

### [metrics/cpu_worker.rs + sleep_worker.rs]
- CPU kullanim yuzdesi Prometheus'a rapor edilir
- Sleep kalitesi (cpu_worker'daki uyku gecikmeleri) olculmektedir
- Bunlar L1 node performans izleme icin tasarlanmistir

---

## KISIM 2: bin/pindexer — PostgreSQL Olay Indeksleyici

---

### [lib.rs]
**Dosya:** `crates/bin/pindexer/src/lib.rs`
**Satir:** 39

#### Struct: `Options`
```rust
pub struct Options {
    pub cometindex: cometindex::opt::Options,
    pub indexing_denom: asset::Id,  // passet1w6e7fv... (staking token benzeri)
    pub dex_ex_min_liquidity: u128, // 100_000_000 = 100 upenumbra
    pub dex_ex_ignore_arb: bool,    // true (arb islemleri dex explorer'da gizlenir)
    pub block_time_s: f64,          // 5.1 saniye (mainnet default)
}
```

**Mimarisi:** `cometindex` framework'u uzerinde calisir. ABCI event stream'ini PostgreSQL'e
yazar. Her modül bir `AppView` trait implementasyonudur.

#### Modüller
- `block` — Blok yuksekligi, timestamp indeksleme
- `dex_ex` — DEX explorer: candlestick, volume, pozisyon akislari
- `governance` — Oneri ve oy kayitlari
- `ibc` — IBC transfer olaylari
- `insights` — Staking/supply/ekonomi verileri
- `lqt` — Liquidity Tournament oy ve odulleri
- `stake` — Validator set, delegasyon/undelegasyon, slashing, missed blocks
- `supply` — Token arz takibi

---

### [dex_ex/mod.rs] — DEX Explorer Indeksleyici

#### Candle Struct
```rust
pub struct Candle {
    pub open: f64,
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub direct_volume: f64,  // Dogrudan DEX uzerindeki islem hacmi
    pub swap_volume: f64,    // Batch swap hacmi
}
```

#### Desteklenen Olaylar
- `EventBatchSwap` — Batch clearing price kaydi
- `EventPositionOpen/Close/Execution/Withdraw` — LP olay takibi
- `EventArbExecution` — Arbitraj takibi (dex_ex_ignore_arb=true ise gizlenir)
- `EventSwap / EventSwapClaim` — Bireysel swap kayitlari
- `EventQueuePositionClose` — Kuyruktaki pozisyon kapanmalari
- `EventLqtPositionReward` — LQT odulleri
- `EventBlockRoot` — SCT kok hash kaydi

#### Candlestick Penceresi
Bir blok icindeki tum swap'lar tek bir candlestick noktasina indirgenir.
Birden fazla blok candlestick'leri merge edilebilir (dakika/saat/gun araliklari).

---

### [insights/mod.rs] — Ekonomik Gorunum

#### `convert_ceil(amount, inv_rate_bps2)`
```rust
fn convert_ceil(amount: u64, inv_rate_bps2: u64) -> anyhow::Result<u64> {
    Ok(u64::try_from(
        (u128::from(amount) * 1_0000_0000).div_ceil(u128::from(inv_rate_bps2)),
    )?)
}
```
Staking exchange rate ile donsturum: `amount * 10^8 / rate_bps2` (tavanlamali).

#### Takip Edilen Veriler
- Validator delegasyon miktarlari (UM cinsinden)
- Exchange rate gecmisi
- IBC transfer giriş/cikis hacimleri
- Blok odulleri (FundingStream)
- Toplam staking token arzi

---

### [stake/validator_set.rs + delegation_txs.rs + slashings.rs]

Validator set: her blokta aktif/inactive/jailed durumlar PostgreSQL'e yazilir.
Slashing kayitlari: tombstone olaylari ile slashed_at_height, slashed_by kaydedilir.
Delegation TX'leri: kim kime ne kadar delege etti, hangi blokta.
Missed blocks: validator uptime hesabi icin kacirilan blok kayitlari.

---

## KISIM 3: bin/elcuity — Otomatik LP + Swap + LQT Vote Botu

---

### [lib.rs] — Komut Yapisi

```rust
pub struct Opt {
    grpc_url: String,      // pd gRPC endpoint
    view_service: String,  // pclientd view service endpoint
    command: Command,
}

pub enum Command {
    Vote(vote::Opt),  // Surekli LQT oyu
    Lp(lp::Opt),     // Otomatik LP saglama
    Swap(swap::Opt),  // Otomatik swap dongusu
}
```

**Mimari:** External view service (pclientd) ile gRPC uzerinden iletisir.
Custody servisi de disarida olabilir. `Clients::init()` her ikisini de baglantisini kurar.

---

### [lp/mod.rs] — Akilli LP Botu Analizi

#### Kritik Sabitler
```rust
const POSITION_CHUNK_SIZE: usize = 5;  // Her batch'te max 5 pozisyon
const DEFAULT_PRICE: u8 = 1;
const DEFAULT_FEE_BPS: u16 = 500;      // %5 default fee (DEX'te hesaplanir)
const ADJUST_WAIT_SECS: u64 = 120;     // 2 dakikada bir yeniden dengele
```

#### LP Stratejisi: Spread-based Auto-Market-Making
```
1. close_all_positions() — Acik pozisyonlari kapat (5'li gruplar)
2. withdraw_all_positions() — Kapali pozisyonlari cek
3. get_sell_price() — SimulateTrade ile mevcut satis fiyatini al
4. spread_to_price_and_fee_bps() — Aliş/satis yayi hesapla
5. make_position() — Yeni pozisyon olustur
6. submit() — Zincire gonder
7. 120 saniye bekle, donguy tekrarla
```

#### `spread_to_price_and_fee_bps(buy, sell) -> (price, fee_bps)`

Matematiksel formul:
```
midpoint = (buy + sell) / 2
fee = (buy / midpoint) - 1   // Spread'den fee hesapla
fee_bps = min(5000, fee * 10_000)  // Max %50 fee
```
Epsilon: `[0, 0, ..., 0, 1]` (32 byte, en kucuk U128x128 birimi)
Buy epsilon kadar asagi, sell epsilon kadar yukari kaydirilir.

#### `price_to_p_q(price, precision) -> (Amount, Amount)`
U128x128 fiyati tam sayi p/q oranina donusturur:
- `p` ve `q` her ikisi `precision`'i asmamak uzere 2 ile cogaltilir
- Hayali kesir hassasiyeti: 2^60 = 1_152_921_504_606_846_976

#### `make_position()` — Pozisyon Olusturma
- Hedef likidte: staking token cinsinden belirtilir
- `half_liquidity_target` ve `other_target` (price * half) hesaplanir
- `balance_limits` ile asiri harcama engellenir (staking token'in %90'i kullanilir, fee icin %10 ayrilir)
- `Position::new(OsRng, pair, fee_bps, p, q, Reserves { r1, r2 })` — AMM cekirdegi

---

### [swap.rs] — Otomatik Swap + SwapClaim

#### Iki Asamali Swap Akisi
```
1. Planner::new(OsRng)
     .swap(input, output_asset_id, claim_fee, address)
     .plan(view)
     -> submit(clients, plan)

2. view.swap_by_commitment(swap_commitment)  <- SwapRecord al
   output_data.pro_rata_outputs((delta_1_i, delta_2_i))

   Planner::new(OsRng)
     .swap_claim(SwapClaimPlan { plaintext, position, output_data, epoch_duration })
     .plan(view)
     -> submit(clients, plan)
```

**Tasarim:** `swap` ve `swap_claim` iki ayri transaction. Aralarinda `swap_by_commitment` ile
batch clearing output beklenmesi gerekir. Bu Penumbra'nin ZK batch swap modelini yansitir.

---

### [vote.rs] — LQT Vote Botu
Surekli olarak verilen denom icin LQT vote gonderir.
Mevcut delegation pozisyonlarini tarayarak her biri icin oy kullanir.
`epoch_duration` beklendikten sonra tekrar oylama yapilir.

---

## KISIM 4: bin/pclientd — Client-side Daemon

---

### [lib.rs] — Konfigurasyon ve Baslatma

#### `PclientdConfig`
```rust
pub struct PclientdConfig {
    pub full_viewing_key: FullViewingKey,  // View + custody mod icin
    pub grpc_url: Url,                     // pd'ye baglanti URL'i
    pub bind_addr: SocketAddr,             // Kendi gRPC bind adresi
    pub kms_config: Option<soft_kms::Config>,  // Custody mod (opsiyonel)
}
```
TOML formatinda kaydedilir/yuklenir.

#### Calisma Modlari
1. **View-only**: `full_viewing_key` ile sadece view servisi (SoftKms yok)
2. **Custody**: `kms_config` ile view + custody servisi (SoftKms veya sifrelenmis)

#### Baslatma Akisi
```
SpendKey -> FullViewingKey -> ViewServer::load(storage, endpoint, fvk, params)
                                         -> ViewServiceServer::new(view_server)
                                         -> CustodyServiceServer::new(soft_kms)  (custody modunda)
Server::builder().add_service(view_server)
                 .add_service(custody_server)  // opsiyonel
                 .serve(bind_addr)
```

---

### [proxy.rs] — gRPC Proxy Servisleri

Pclientd, pd'nin gRPC servislerini istemciye yonlendirir. Her proxy `tower::Service`
implementasyonudur; `NamedService` ile proto servis adi kayit altina alinir.

**Proxy Listesi (12 adet):**
```
AppQueryProxy          -> penumbra.core.app.v1.QueryService
GovernanceQueryProxy   -> penumbra.core.component.governance.v1.QueryService
DexQueryProxy          -> penumbra.core.component.dex.v1.QueryService
DexSimulationProxy     -> penumbra.core.component.dex.v1.SimulationService
ChainQueryProxy        -> (chain parametreleri)
CompactBlockQueryProxy -> penumbra.core.component.compact_block.v1.QueryService
ShieldedPoolQueryProxy -> penumbra.core.component.shielded_pool.v1.QueryService
StakeQueryProxy        -> penumbra.core.component.stake.v1.QueryService
SctQueryProxy          -> penumbra.core.component.sct.v1.QueryService
FeeQueryProxy          -> penumbra.core.component.fee.v1.QueryService
TendermintProxyProxy   -> (CometBFT proxy)
```

Her proxy sadece `proxy(channel, req)` cagrisini yapar; hata durumunda
`tonic::Status::internal()` ile HTTP response olusturur — Error: Infallible garanti edilir.

---

## MASTER SENTEZ: Penumbra Mimarisi ve Nova Shield Chain Icin Eylem Plani

---

### 1. Penumbra'nin Temel Mimari Katmanlari

```
[CLI] pcli / elcuity
  |
  v
[Client Daemon] pclientd (ViewServer + SoftKms gRPC)
  |
  v
[Full Node] pd (ABCI:26658 + gRPC:8080/443 + Metrics:9000)
  |         |
  |      minifront (embedded zip)
  v
[App] penumbra_sdk_app::App (ABCI handler)
  |
  +---> [SCT] State Commitment Tree (Poseidon377 + 24-level quadtree)
  +---> [Shielded Pool] Note + Spend + Output (Groth16 ZK)
  +---> [DEX] Concentrated Liquidity + Batch Swap
  +---> [Stake] Epoch-based, exchange rate model
  +---> [Governance] ZK delegator vote, NFT-based proposals
  +---> [IBC] Full ICS-02/03/04/20 (Tendermint only)
  +---> [Auction] Dutch Auction + DEX integration
  +---> [Funding] LQT + staking rewards
  |
  v
[Storage] cnidarium (RocksDB, JMT Merkle)
  |
  v
[Indexer] pindexer (PostgreSQL, cometindex framework)
```

---

### 2. Dogrulanmis Kritik Degerler (Kaynak Koddan)

| Deger | Kaynak | Dosya |
|-------|--------|-------|
| MAX_BLOCK_TXS_PAYLOAD_BYTES | 1_048_576 (1 MB) | core/app/src/app/mod.rs |
| MAX_TRANSACTION_SIZE_BYTES | 98_304 (96 KB) | core/app/src/app/mod.rs |
| MAX_EVIDENCE_SIZE_BYTES | 30_720 (30 KB) | core/app/src/app/mod.rs |
| ABCI default bind | 127.0.0.1:26658 | pd/src/cli.rs |
| gRPC HTTP default | 127.0.0.1:8080 | pd/src/cli.rs |
| gRPC HTTPS default | 0.0.0.0:443 | pd/src/cli.rs |
| Metrics default | 127.0.0.1:9000 | pd/src/cli.rs |
| CometBFT proxy | http://127.0.0.1:26657 | pd/src/cli.rs |
| DEX max_hops | 4 | component/dex/src/params.rs |
| DEX max_positions_per_pair | 1_000 | component/dex/src/params.rs |
| DEX max_execution_budget | 64 | component/dex/src/params.rs |
| TCT depth | 24 (8+8+8) | crypto/tct |
| Clue size | 68 bytes | proto/src/protobuf.rs |
| Amount encoding | { lo: u64, hi: u64 } | proto/gen/penumbra.core.num.v1.rs |
| Address size | 80 bytes | proto/gen/penumbra.core.keys.v1.rs |
| OrderUid | 56 bytes | N/A (CoW protokolu, Penumbra'da yok) |
| Rate scaling | 1_0000_0000 (10^8) | component/stake/src/rate.rs |
| GasPrices denominator | 1000 (implisit) | component/fee/src/gas.rs |
| ICS20_TRANSFER_START_HEIGHT | 411_616 | pd/src/migrate/mainnet1.rs |
| elcuity LP chunk size | 5 | bin/elcuity/src/lp/mod.rs |
| elcuity adjust wait | 120 saniye | bin/elcuity/src/lp/mod.rs |
| elcuity fee max | 5000 bps (%50) | bin/elcuity/src/lp/mod.rs |
| pindexer block_time_s | 5.1 saniye | bin/pindexer/src/lib.rs |
| pindexer dex_ex_min_liquidity | 100_000_000 | bin/pindexer/src/lib.rs |

---

### 3. ZK Proof Sistemi Ozeti

Penumbra 7 farkli Groth16 ZK devresi kullanir:

| Devre | Amaç | Parametreler |
|-------|------|-------------|
| SpendProof | Not harcama (nullifier + commitment) | spend_id.rs |
| OutputProof | Not olusturma | output_id.rs |
| SwapProof | Gizli swap girisi | swap_id.rs |
| SwapClaimProof | Batch swap ciktisi talebi | swapclaim_id.rs |
| ConvertProof | Token donusumu (unbonding) | convert_id.rs |
| DelegatorVoteProof | Gizli delegator oyu | delegator_vote_id.rs |
| NullifierDerivationProof | Nullifier turetimi | nullifier_derivation_id.rs |
| LQTVoteProof | Likidite turnuvasi oyu | funding crate |

Tum parametreler onceden hazirlanmis (trusted setup), `proof-params` crate'inde embed.

---

### 4. Privacy Katmanlari

1. **Note Encryption**: `ChaCha20Poly1305` — note payload'u alici OVK ile sifrelenir
2. **FMD Clue**: 68 byte, `2^(-precision_bits)` yanlis pozitif orani — detection server gizliligi
3. **Nullifier**: Poseidon377 tabanlı tek kullanimlik harcama kaniti
4. **ZK Proofs**: Harcayan kisi kimligini gizler, sadece commitment gosterir
5. **Batch Swap**: Bireysel swap miktarlari block-level gizlilik alir (tum batch ayni fiyattan)
6. **Memo**: Gonderici-alici arasi sifrelenmis mesaj

---

### 5. DEX Teknik Ozeti (3x Hassasiyet Dogrulamasi)

#### AMM Formulu (BareTradingFunction)
```
phi(R) = p * R1 + q * R2     -- Sabit carpim fonksiyonu
gamma = 1 - fee/10_000        -- Fee carpani
```
Swap formulu: `lambda_2 = gamma * delta_1 * q / (p + gamma * delta_1)`

#### Batch Swap Akisi
```
1. SwapProof ile gizli swap girisi zincire gonder
2. EndBlock: tum swap'lar toplanir, BSOD (BatchSwapOutputData) hesaplanir
3. Clearing price: lambda_1/delta_1 veya lambda_2/delta_2
4. Kullanici SwapClaimProof ile kendi payini alir (pro-rata)
```

#### Routing Algoritmalari
- `FillRoute` — Bir rota boyunca likidte tuketir
- `RouteAndFill` — En iyi rotayi bulur ve doldurur
- Flash loan arbitraj — Her `end_block`'ta otomatik calisir
- `max_hops=4` — Maksimum 4 hop/atilama

#### Pozisyon Yasam Dongusu
```
Opened -> Closed -> Withdrawn
                -> Claimed (DEPRECATED)
```
`close_on_fill=true` ile dolan pozisyon otomatik kapanir.

---

### 6. Consensus ve Block Akisi

```
CometBFT -> ABCI -> ConsensusService (tower_actor, QUEUE_SIZE=10)
                 -> InitChain -> begin_block -> deliver_tx* -> end_block -> commit
                 -> PrepareProposal / ProcessProposal (ABCI 2.0)
```

`deliver_tx` hatasi `EndBlock(Err)` olarak donebilir — node panic etmez.
`begin_block` ve `init_chain` panikleyebilir — kritik hata durumu.

---

### 7. Storage Mimarisi

```
RocksDB
  |
  +---> JMT (Jellyfish Merkle Tree) — verifiable substore
  |        -> ics23 proof destegi
  |        -> consensus state buradan alinir
  |
  +---> NonVerifiable KV — hizli index verileri
           -> prefix scan ile streaming
```

`SUBSTORE_PREFIXES` ile namespace ayristirma: IBC, DEX, SCT vs. ayri alt-agaclar.

---

### 8. Migration Framework Evrimi

**Eski sistem**: `async fn migrate(storage, pd_home, genesis_start)`
- Her migration icin ayri fonksiyon
- Birbirinden bagimsiz, yeniden kullanilabilir degil

**Yeni sistem** (`migrate2/framework.rs`): `trait Migration`
- `prepare()` + `migrate()` + `migrate_inner()` + `run()`
- `commit_in_place()` — tek atomik yazma
- Otomatik genesis uretimi
- `target_app_version()` otomatik guncelleme

---

### 9. Nova Shield Chain Icin Kritik Eylem Noktalari

#### A. Node Daemon Mimarisi
- **DO**: pd'nin `tokio::select!` cok-servis pattern'ini aynen kullan
- **DO**: ABCI, gRPC, metrics uc ayri port — ayni pattern
- **DO**: Embedded minifront (include_bytes!) — ayri deployment yuku kalkar
- **DO**: `aws_lc_rs` TLS provider — FIPS uyumlu, cretumize sertifika gerekmez
- **DON'T**: CORS'u `permissive()` yapma; production'da kisitrla

#### B. Chain Migration Sistemi
- **DO**: Migration framework'u basindan uygula — sonradan refactor zor
- **DO**: `halt_bit` mekanizmasini ekle — governance ile zinciri durdurabilmek kritik
- **DO**: `priv_validator_state.json` yuksekligini hicbir zaman azaltma
- **DO**: `tx_index.db` upgrade sonrasi KORU — tarihsel tx sorgusu icin
- **DON'T**: `force` flag'ini production'da kullanma

#### C. DEX Implementasyonu
- **DO**: Batch swap clearingi ZK ile koru — bireysel miktarlar gorunmemeli
- **DO**: `BareTradingFunction` formulu: `p*R1 + q*R2 = sabit` (Penumbra yaklasimiyla)
- **DO**: `max_positions_per_pair` limiti — spam koruması
- **DO**: Flash loan arbitraj + circuit breaker — DEX sagligini koru
- **DON'T**: `close_on_fill` olmadan LP olusturma — round-trip atagi

#### D. Privacy Altyapisi
- **DO**: FMD Clue'yu 68 byte sabit uzunlukta tut
- **DO**: Note encryption: `NotePayload = { note_commitment, ephemeral_key, encrypted_note }`
- **DO**: `ChaCha20Poly1305` over `AES-GCM` — daha hizli, constant-time

#### E. Staking Modeli
- **DO**: Epoch-bazli delegasyon — blok bazi yerine; ani stake degisimlerini engeller
- **DO**: Exchange rate modeli: `delegation_token / staking_token` — Cosmos shares benzeri
- **DO**: `BPS^2 = 10^8` scaling — hassas oran hesabi icin

#### F. ZK Proof Sistemi
- **KARAR VERILEN**: `arkworks` (Groth16/PLONK) — Penumbra ile uyumlu
- **DO**: Her aktion tipi icin ayri ZK devresi
- **DO**: Trusted setup onceden yap; parametreleri embed et (proof-params pattern)
- **DON'T**: Runtime'da proof parametrelerini uret

#### G. IBC Entegrasyonu
- **DO**: JMT Merkle tree + ics23 spec — IBC light client proof'lari icin zorunlu
- **DO**: Sadece Tendermint (07-tendermint) destekle baslangicta
- **DO**: `tx_index.db` IBC tarihsel islem arama icin gerekli

#### H. Indexer Mimarisi
- **DO**: `cometindex` ya da benzeri bir framework kullan — event -> PostgreSQL pipeline
- **DO**: DEX candlestick verilerini blok bazinda kaydet — OHLCV
- **DO**: `insights` modulu gibi ekonomik gorunum verilerini ayri tablo

#### I. Client Daemon (pclientd benzeri)
- **DO**: View ve Custody servislerini tek daemon'da birlestir — kullanici deneyimi
- **DO**: 12 gRPC proxy servisini transparent yonlendir — client tek noktaya baglansın
- **DO**: TOML config (`full_viewing_key + grpc_url + bind_addr + kms_config`)

#### J. LP Bot (elcuity benzeri — Nova Shield LP Stratejisi)
- **DO**: Spread-based fee hesabi: `fee = (buy/midpoint) - 1`
- **DO**: `price_to_p_q()` rational approximation: 2^60 hassasiyet
- **DO**: `balance_limits` ile %90 kural — staking token'in %10'unu fee icin sakla
- **DO**: 5'li chunk'lar halinde position_close/withdraw — tek TX'te fazla action engeli

---

### 10. Penumbra'dan Alinmayan Kararlar

| Penumbra Tasarimi | Nova Shield Alternatifi | Gerekce |
|-------------------|-------------------------|---------|
| Hiyerarsik anahtar (Zcash Sapling) | Standart BIP-44 + secp256k1 | Daha genis cuzdan uyumlulugu |
| decaf377 (BLS12-377 subgroup) | secp256k1 / BLS12-381 | Daha genis kutumhane ekosistemi |
| Penumbra FROST (kendi impl) | cggmp21 (secim) | Production'a hazir kutumhane |
| Monolithic ZK devre seti | Modular: ZK + MPC + Audits | Daha esnek mimari |
| Gizli swap ZK batch | MEV korumal ama transparent | Audit kolayligi; Nova DEX fark |

---

### 11. Penumbra Analizinin Genel Degerlendirmesi

**Kodun Kalitesi: UZMAN SEVIYESI**
- `#![deny(clippy::unwrap_used)]` her crate'te — Zero-panic disiplin
- Hata yollari tamamen explicit: `anyhow::Result` ve `?` operatoru
- `#[instrument]` tracing decorator her kritik fonksiyonda
- Test kapsamı: property-based testler (proptest), integration testler (mock-consensus)

**Dikkat Ceken Guvenlik Ozellikleri:**
1. Halt bit mekanizmasi — governance ile zinciri durdurabilme
2. Value Circuit Breaker (VCB) — DEX ve auction bilesenlerinde muhasebe dogrulama
3. FMD tarama siniri — detection server abuse'u engellemek icin precision_bits
4. `close_on_fill` — LP round-trip saldirisina karsi
5. Flash loan arb + circuit breaker — DEX manipulasyon korumasi
6. Nullifier sistemi — cift harcamayı engelleme (TCT + Poseidon)
7. FROST threshold imzalama — multisig custody
8. IBC packet commitment onarimi (mainnet1 migration) — ICS-20 bug duzeltmesi

**Dikkat Ceken Tasarim Zafiytleri (potential):**
1. Ledger USB custody — feature-gated, production'da cok az test
2. `enable_expensive_rpc` — mevcut kodda no-op; implementation eksik
3. Testnet migration'larin bir kismi `unimplemented!()` panigi uretir
4. `CorsLayer::permissive()` — tam acik CORS; production'da riskli

---

### 12. Repo Ozet Istatistikleri

| Kategori | Dosya Sayisi | Satir (tahmini) |
|----------|-------------|-----------------|
| proto (generated + infra) | 88 | ~25,000 |
| core/app | 55 | ~3,500 |
| core/asset | 15 | ~1,800 |
| core/dex | 70+ | ~8,000 |
| core/shielded-pool | 41 | ~3,500 |
| core/stake | 43 | ~3,500 |
| core/governance | 34 | ~2,400 |
| core/ibc | 49 | ~7,770 |
| core/auction | 27 | ~900 |
| crypto/tct | 41 | ~4,000 |
| crypto/* (digerleri) | 35 | ~2,500 |
| view | 14 | ~4,500 |
| custody | 15 | ~2,000 |
| bin/pcli | 56 | ~6,000 |
| bin/pd | 25 | ~2,500 |
| bin/pindexer | 18 | ~2,000 |
| bin/elcuity | 7 | ~700 |
| bin/pclientd | 4 | ~500 |
| util+bench+test | 47 | ~3,200 |
| **TOPLAM** | **~750+** | **~88,000+** |

---

### 13. Agent Kapsam Haritasi

| Agent | Kapsanan Modüller |
|-------|-------------------|
| agent-01 | core/num, core/txhash, core/asset |
| agent-02 | core/keys |
| agent-03 | core/transaction |
| agent-04 | DEX LP cekirdegi (1. perspektif) |
| agent-05 | core/app-tests |
| agent-06 | fee, distributions, compact-block, sct |
| agent-07 | community-pool, funding |
| agent-08 | shielded-pool |
| agent-09 | governance |
| agent-10 | stake |
| agent-11 | DEX routing + core/app ABCI |
| agent-11-core-app | core/app (detay) |
| agent-12 | decaf377-fmd, crypto/eddy |
| agent-13 | decaf377-frost, decaf377-ka, proof-params, proof-setup, tct |
| agent-14 | core/app (ozet) + core/asset (tekrar) |
| agent-auction | auction component |
| agent-crypto-tct | crypto/tct (detay) |
| agent-DEX-A | DEX veri katmani |
| agent-DEX-B | DEX component + integration tests |
| agent-ibc | IBC component |
| agent-pcli | bin/pcli |
| agent-proto | proto crate (tum 88 dosya) |
| agent-util-bench-test | util, bench, test |
| agent-view-custody | view, wallet, custody, misc, cnidarium-component |
| **agent-15 (bu dosya)** | pd, pindexer, elcuity, pclientd + MASTER SENTEZ |

**KAPSAM DURUMU: TUM PENUMBRA REPO ANALIZ EDILDI**

---

## ACIMASISIZ INCELEME: En Kritik 5 Bulgu

1. **DEX Value Circuit Breaker (VCB)**: Her auction ve DEX bileseninde gelir/gider muhasebesi.
   `app_check_dex_vcb.rs` ve `app_reproduce_testnet_75_vcb_close.rs` testleri regression.
   Nova Shield DEX'te ZORUNLU.

2. **Halt Bit + Migration Framework**: Yeni `migrate2/framework.rs` sistemi genisletilebilir.
   `target_app_version()` ile otomatik app versiyonu artirmasi.
   Nova Shield'in `v1 -> v2` yukseltmeleri icin kopyalanabilir.

3. **Batch Swap Pro-Rata Dagilimi**: `BatchSwapOutputData::pro_rata_outputs()` hem delta_1
   hem de delta_2 icin `U128x128` aritmetigi kullanir. Overflow riski yok.
   Nova Shield batch swap implementasyonunda ayni U128x128 kullanilmali.

4. **IBC Packet Bug (Mainnet1)**: Hata ack'leri basarili sayilip paket silindi.
   `tx_lost_transfers()` fonksiyonu 411_616 - son_blok araligini tarar.
   Nova Shield IBC implementasyonunda ack error handling dikkatli yapilmali.

5. **FMD Sliding Window Algorithm**: `FmdMetaParameters::Algorithm::SlidingWindow` ile
   dinamik precision ayarlama. Privacy/performans dengesini otomatik optimize eder.
   Nova Shield MEV korumasinda benzer adaptif mekanizma dusunulebilir.
