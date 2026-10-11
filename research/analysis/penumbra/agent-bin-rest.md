# Penumbra Binary Crates Tam Analizi
## pd, pindexer, pmonitor, elcuity, pclientd

**Analiz Eden:** agent-bin-rest
**Tarih:** 2026-07-22
**Kapsam:** 5 binary crate — pd (30 dosya), pindexer (18 dosya), pmonitor (7 dosya), elcuity (7 dosya), pclientd (5 dosya) + tüm test dosyaları

---

## CRATE 1: pd — Penumbra Daemon (Node Binary)

### Genel Amaç
Penumbra'nın ana node daemon binary'si. CometBFT/Tendermint ABCI üzerinden konsensüs katmanına bağlanır, gRPC API'yi axum üzerinde sunar, ACME/Let's Encrypt ile otomatik HTTPS, Prometheus metrikleri, RocksDB storage, ve zincir migrasyon altyapısını yönetir.

---

### `pd/src/lib.rs` (21 satır)

**Genel Amaç:** Crate'in modül yapısını bildirir, minifront ve node-status statik asset'lerini embed eder.

**Sabitler:**
- `MINIFRONT_ARCHIVE_BYTES`: `include_bytes!("../../../../assets/minifront.zip")` — build zamanında ZIP embed, runtime'da `zipserve` ile sunulur
- `NODE_STATUS_ARCHIVE_BYTES`: `include_bytes!("../../../../assets/node-status.zip")` — node durum panosu ZIP

**Modüller:** metrics, cli, migrate, network, zipserve

**Nova Shield İçin Ders:** Node binary'sine statik frontend ZIP embed etmek çok temiz bir pattern — ayrı HTTP sunucusu gerektirmiyor, tek binary ile her şey çalışıyor.

---

### `pd/src/main.rs` (569 satır)

**Genel Amaç:** Tüm node başlatma mantığı. tokio::main ile async runtime, tracing kurulumu (MetricsLayer + FmtLayer + EnvFilter), aws-lc-rs FIPS kriptografi provider kurulumu, clap ile CLI parse.

**Kritik Struct'lar:**
- `RootCommand`: Start, Network{Generate, Join, UnsafeResetAll}, Export, Migrate

**Önemli Fonksiyonlar:**

`RootCommand::Start` — Satır ~100–300:
- `Storage::load(home.join("rocksdb"))` ile RocksDB açar
- ABCI TCP server başlatır (default: 127.0.0.1:26658)
- `TendermintProxy` oluşturur (CometBFT RPC'ye proxy)
- axum Router kurulumu:
  - gRPC routes (11 adet servis)
  - minifront ZIP (`/app/*path`)
  - node-status ZIP
  - permissive CORS: `Access-Control-Allow-Origin: *`, `Access-Control-Expose-Headers: *`
- ACME auto-HTTPS: `--auto-https` flag ile aktif
- Prometheus exporter: default 127.0.0.1:9000
- `sleep_worker` spawn: async tokio sleep drift monitor
- `cpu_worker` spawn: CPU metrik toplayıcı
- `tokio::select!` ile tüm servisler eşzamanlı bekler

**Default Portlar:**
- ABCI: 127.0.0.1:26658
- gRPC (HTTP): 127.0.0.1:8080
- gRPC (HTTPS): 0.0.0.0:443
- Metrics: 127.0.0.1:9000
- CometBFT RPC: http://127.0.0.1:26657

`RootCommand::Network::Join`:
- `network_join()` çağırır
- `--archive-url` ile state snapshot indirir: `unpack_state_archive()`

`RootCommand::Network::Generate`:
- `--preserve-chain-id` yoksa suffix rasgele: `chain_id-XXXXXXXX`

`RootCommand::Export`:
- `fs_extra::copy_items` ile rocksdb dizinini kopyalar
- `--export-at HEIGHT` ile belirli yükseklikte export
- `archive_directory()` ile `.tar.gz` arşiv oluşturur

`RootCommand::Migrate`:
- `is_chain_halted()` kontrolü
- Migration varyantına göre dispatch

---

### `pd/src/cli.rs` (278 satır)

**Genel Amaç:** clap ile CLI yapısı tanımlar.

**Struct Listesi:**
- `Opt { cmd: RootCommand }` — clap Parser, global entry point
- `StartCmd` — 7 alan: home, abci_bind, grpc_bind, grpc_auto_https, metrics_bind, cometbft_addr, tendermint_addr
- `NetworkCommand` — Generate(13 alan), Join(7 alan), UnsafeResetAll
- `MigrateCommand` — ReadyToStart, IbcRecovery{old_client_id, new_client_id, target_app_version}, NoOp{target_app_version}
- `ExportCmd` — home, export_path, export_at, prune

**NetworkCommand::Generate 13 Parametresi:**
chain_id, active_validator_limit, timeout_commit, epoch_duration, unbonding_delay, allocations_input_file, validators_input_file, peer_address_template, external_addresses, gas_price_simple, gas_price_tier1/2/3, preserve_chain_id

---

### `pd/src/auto_https.rs` (1 satır — boş)

Gerçek ACME implementasyonu ayrı `penumbra_sdk_auto_https` crate'inde. Bu dosya sadece module placeholder.

---

### `pd/src/metrics.rs` (29 satır)

**Genel Amaç:** metrics crate re-export, alt modül bildirimi.

**Fonksiyonlar:**

`register_metrics()`:
- `penumbra_sdk_app::register_metrics()` — uygulama katmanı metrikleri
- `sleep_worker::register_metrics()` — async drift monitor
- `cpu_worker::register_metrics()` — CPU koleksiyonu

---

### `pd/src/metrics/cpu_worker.rs` (34 satır)

**Genel Amaç:** CPU kullanım metrikleri toplama.

**Sabitler:**
- `SLEEP_DURATION = Duration::from_secs(2)` — her 2 saniyede bir CPU ölç
- `METRICS_PREFIX = "pd_"`

**Fonksiyonlar:**

`register_metrics()`: `Collector::new("pd_").describe()` ile Prometheus'a metrik açıklaması yazar

`run() -> Infallible` (satır 18–34):
```rust
loop {
    tokio::time::sleep(SLEEP_DURATION).await;
    collector.collect(); // process-metrics crate, /proc/stat okur
}
```
`Infallible` dönüş tipi: bu fonksiyon asla normal dönmez — sonsuz döngü.

---

### `pd/src/metrics/sleep_worker.rs` (65 satır)

**Genel Amaç:** Tokio async scheduler drift'ini ölçer.

**Sabitler:**
- `SLEEP_DRIFT = "pd_async_sleep_drift_microseconds"`
- `ONE_SECOND = Duration::from_secs(1)`
- `ONE_SECOND_US: u64 = 1_000_000`

**Fonksiyonlar:**

`register_metrics()`: Prometheus counter açıklaması

`run() -> Infallible` (satır 35–65):
```rust
loop {
    let start = Instant::now();
    tokio::time::sleep(ONE_SECOND).await;
    let actual = start.elapsed().as_micros() as u64;
    let drift = actual.saturating_sub(ONE_SECOND_US);
    if drift != 0 {
        counter!(SLEEP_DRIFT, drift);
    }
}
```
`saturating_sub`: underflow koruması — actual < 1_000_000 ise 0 döner, negatif olmaz.

**Nova Shield İçin Ders:** Tokio scheduler'ın ne kadar meşgul olduğunu ölçmenin basit ve etkili yolu. Yüksek drift değerleri = blocking kod var ya da CPU overload. Production monitoring için kritik.

---

### `pd/src/migrate.rs` (362 satır)

**Genel Amaç:** Zincir upgrade migrasyonlarını koordine eder.

**Enum:**
```rust
pub enum Migration {
    ReadyToStart,
    SimpleMigration,
    Testnet72, Testnet74, Testnet76, Testnet77, Testnet78,
    Mainnet1, Mainnet2, Mainnet3, Mainnet4, Mainnet5,
    IbcClientRecovery { old_client_id: String, new_client_id: String },
    NoOp { target_app_version: Option<u64> },
}
```

**Fonksiyonlar:**

`migrate()`: `migrate_with_params(pd_home, comet_home, genesis_start, force, vec![])` çağırır

`migrate_with_params()` (satır ~80–250):
1. `Storage::load()` ile RocksDB açar
2. `is_chain_halted() || force` kontrolü — yalnızca durdurulmuş zincirde migrasyon
3. `latest_version == block_height || force` kontrolü — doğru yükseklikte miyiz
4. Migration dispatch:
   - ReadyToStart → `reset_halt_bit::migrate()`
   - IbcClientRecovery → `ibc_client_recovery` modülü
   - NoOp → `noop` modülü
   - Mainnet5 → `migrate2::framework::Migration` trait implementasyonu
   - Testnet* / Mainnet1-4 → eski pattern (modül fonksiyonu)

`archive_directory(src, dest)` (satır ~270–310):
```rust
let tar_gz = File::create(dest.with_extension("tar.gz"))?;
let enc = GzEncoder::new(tar_gz, Compression::default());
let mut tar = tar::Builder::new(enc);
tar.append_dir_all(".", src)?;
```
flate2 + tar crate ile arşivleme.

`last_block_timestamp()`: En son snapshot'tan timestamp okur

`migrate_comet_data(comet_home, genesis_start)` (satır ~320–360):
- `config/genesis.json` günceller — `initial_height` yazar
- `adjust_priv_validator_state()` çağırır
- `clear_comet_data()` çağırır

`adjust_priv_validator_state(path, new_height)`:
- Mevcut `priv_validator_state.json` okur
- Yalnızca `height` artıyorsa yazar — azaltma asla yapılmaz (güvenlik)

`clear_comet_data(comet_home)`:
- `evidence.db` siler
- `state.db` siler
- `blockstore.db` siler
- `cs.wal` siler (write-ahead log)
- `tx_index.db` KORUR — transaction indeks korunur

---

### `pd/src/migrate/migrate2/framework.rs` (184 satır)

**Genel Amaç:** Yeni nesil migration trait-based framework. Mainnet5+ için kullanılır.

**Trait:**
```rust
pub trait Migration {
    fn name(&self) -> &'static str;
    fn target_app_version(&self) -> Option<u64>;
    async fn prepare(&self, storage: &Storage) -> anyhow::Result<()>;
    async fn migrate(&self, delta: &mut StateDelta<Snapshot>) -> anyhow::Result<()>;
    async fn migrate_inner(&self, delta: &mut StateDelta<Snapshot>) -> anyhow::Result<()>;
    async fn complete(&self, storage: Storage, ...) -> anyhow::Result<()>;
    async fn run(&self, pd_home: PathBuf, comet_home: PathBuf, ...) -> anyhow::Result<()>;
}
```

**`migrate()` default implementasyonu** (satır ~60–110):
```rust
let post_upgrade_height = pre_upgrade_height.wrapping_add(1);
if let Some(v) = self.target_app_version() {
    migrate_app_version(&mut delta, v).await?;
}
self.migrate_inner(&mut delta).await?;
delta.ready_to_start();           // halt bit kaldır
delta.put_block_height(0u64);     // height sıfırla
commit_in_place(delta).await?;    // JMT'ye commit
```
`wrapping_add(1)`: post_upgrade_height taşma korumalı artış.

**`complete()` implementasyonu** (satır ~115–160):
```rust
// Post-upgrade root hash al
let post_upgrade_root = storage.latest_snapshot().root_hash().await?;
// Yeni genesis oluştur
let genesis = NetworkConfig::make_genesis(app_state)?;
// app_hash'i post-upgrade root ile güncelle
genesis.app_hash = post_upgrade_root.0.to_vec();
// Checkpoint oluştur
let checkpoint = make_checkpoint(post_upgrade_root, genesis);
// genesis.json ve priv_validator_state.json yaz
checkpoint.write(comet_home)?;
```

**`run()` implementasyonu** (satır ~165–184):
1. `prepare()` — pre-migration hazırlık
2. `let start = Instant::now(); migrate(); duration = start.elapsed();` — timed
3. `complete()` — post-migration genesis oluştur
4. `info!("migration complete in {:.2?}", duration)` — log

**Nova Shield İçin Ders:** Migration trait pattern çok sağlam bir abstraction. `prepare → migrate → complete` lifecycle her yükseltme için aynı, customize edilebilecek tek yer `migrate_inner()`. JMT commit sonrası root hash'i genesis'e yazmak, yeniden başlatmadan sonraki state doğrulamasını garanti ediyor.

---

### `pd/src/migrate/migrate2/ibc_client_recovery.rs` (59 satır)

**Genel Amaç:** IBC ışık istemci kurtarma — eski client'ı yeni client ile değiştirir.

**Struct:**
```rust
pub struct IbcClientRecoveryMigration {
    old_client_id: String,
    new_client_id: String,
    app_version: u64,
}
```

**`migrate_inner()` implementasyonu:**
```rust
let subject = ClientId::from_str(&self.old_client_id)?;
let substitute = ClientId::from_str(&self.new_client_id)?;
delta.recover_client::<PenumbraHost>(&subject, &substitute).await?;
```
`ClientRecoveryExt::recover_client` — `subject` state'ini `substitute`'dan alır, IBC spec'e göre.

---

### `pd/src/migrate/migrate2/noop.rs` (39 satır)

**Genel Amaç:** Boş migrasyon — yalnızca app_version günceller, state değiştirmez.

```rust
pub struct NoOpMigration { target_app_version: Option<u64> }
// migrate_inner: boş
```
Mainnet2 → Mainnet3 gibi IBC bağımlılık güncellemelerinde kullanılır.

---

### `pd/src/migrate/reset_halt_bit.rs` (21 satır)

**Genel Amaç:** Zincir halt bit'ini sıfırla, node'un yeniden başlamasına izin ver.

```rust
pub async fn migrate(storage: Storage) -> anyhow::Result<()> {
    let mut delta = StateDelta::new(storage.latest_snapshot());
    delta.ready_to_start();        // halt bit = false
    commit_in_place(delta).await?; // JMT commit
    storage.release().await?;      // RocksDB kilidi bırak
    Ok(())
}
```

---

### `pd/src/migrate/simple.rs` (74 satır)

**Genel Amaç:** Örnek migrasyon. Gerçek bir şey yapmaz.

```rust
delta.put_raw("banana", "a good fruit (and migration works!)");
```
Test/demo amaçlı framework doğrulama.

---

### `pd/src/migrate/mainnet1.rs` (184 satır)

**Genel Amaç:** ICS-20 kayıp transfer kurtarması — mainnet app version 8.

**Sabitler:**
- `ICS20_TRANSFER_START_HEIGHT = 411616` — IBC transfer başlangıç bloğu

**Fonksiyonlar:**

`tx_lost_transfers(tx)` (satır ~30–70):
- `IbcRelay::Acknowledgement` action'ları tarar
- `TokenTransferAcknowledgement::Error` olan'ları bulur — bu paketler kayıp

`lost_transfers(storage, height)` (satır ~75–120):
- `ICS20_TRANSFER_START_HEIGHT..=height` aralığında her bloğu tara
- Her bloktan `tx_lost_transfers()` çağır
- Tüm kayıp paketleri topla

`replace_lost_packets(delta, packets)` (satır ~125–150):
- Her kayıp paket için `delta.put_packet_commitment(&packet)` — commitment'ı geri yaz

`migrate()` (satır ~155–184):
1. `migrate_app_version(&mut delta, 8)`
2. `replace_lost_packets(&mut delta, lost_packets)`

**Nova Shield İçin Ders:** IBC kayıp paket tespiti için tüm tarihsel blokları taramak zorunda kaldılar. Bu O(n) bir işlem — mainnet'te ~411k blok. Nova Shield, IBC implementasyonunda kayıp paket durumunu daha iyi takip etmeli, böyle kapsamlı taramalardan kaçınılabilir.

---

### `pd/src/migrate/mainnet2.rs` (103 satır) / `mainnet3.rs` / `mainnet4.rs`

**Genel Amaç:** Sırasıyla app version 9, 10, 11'e güncelleme. IBC bağımlılık güncellemeleri ve LQT (Liquidity Quality Tournament) desteği ekleme. Mainnet2-3'de state değişikliği yok.

---

### `pd/src/migrate/testnet72.rs` (210 satır)

**Genel Amaç:** Testnet 72 büyük migrasyon — SCT position prefix formatını değiştirdi.

**Struct:**
```rust
pub struct Context {
    sct_server: Arc<SctServer>,
}
```

**Fonksiyonlar:**

`epoch_height_to_index(height)`: Verilen height'tan epoch index'i hesaplar

`translate_bsod(bsod)` (BatchSwapOutputData):
- `epoch_starting_height` → `sct_position_prefix` dönüşümü
- DEX output verisindeki format değişikliği

`translate_compact_block(cb)`:
- CompactBlock'un BatchSwapOutputData alanlarını çevirir

`translate_dex_storage(storage)` (satır ~100–160):
- `dex/output/` prefix'ini stream olarak okur (key-value iter)
- Her değeri `BatchSwapOutputData` olarak decode eder
- `translate_bsod()` ile çevirir
- Yeni formatla geri yazar

`translate_compact_block_storage(storage)` (satır ~165–210):
- `compactblock/` nonverifiable prefix'ini stream okur
- Her bloğu çevirir

**Nova Shield İçin Ders:** State migration sırasında prefix-scan + decode + re-encode pattern. Bu pattern Nova Shield'da storage format değişikliklerinde kullanılabilir. Önemli: tüm prefix'i stream olarak okumak için iterator kullan, bellekte tut.

---

### `pd/src/migrate/testnet74.rs` (214 satır)

**Genel Amaç:** DEX liquidity position index formatını düzeltme.

**Kritik Fonksiyonlar:**

`write_auction_parameters()`: Auction modülü parametrelerini genesis değerlerine sıfırla

`fix_arb_execution_outputs(delta)` (satır ~50–90):
- Arb execution output verilerindeki hata: input miktarı output'a eklenmemişti
- `output.amount += input.amount` ile düzeltir

`rewrite_base_liquidity_indices(delta)` (satır ~95–140):
- LP index key formatı: big-endian bytes → proto-encoded bytes
- Tüm `lp/` prefix'ini stream okur, yeniden yazar

`update_lp_index_order(delta)` (satır ~145–200) **[KRİTİK]:**
- Descending order için complement trick:
```rust
// Eski: a_from_b doğrudan
// Yeni: !a_from_b (bitwise NOT) — bu sayede büyük değer küçük key olur
new_key[32 + 7..32 + 7 + 16].copy_from_slice(&(!a_from_b).to_be_bytes());
```
Bit tamamlayıcısı ile azalan sıralama trick'i: büyük olan değer complement'ta küçük olur, B-Tree'de ilk gelir.

**Nova Shield İçin Ders:** DEX liquidity index sıralaması kritik. B-Tree'de descending order için complement trick basit ama etkili. Doğal sıralama yapmak için index key'lerini tasarlarken bu pattern'i uygula.

---

### `pd/src/migrate/testnet76.rs` (169 satır)

**Genel Amaç:** Auction VCB (Value Circuit Breaker) onarımı.

**Kritik Fonksiyon:**

`heal_auction_vcb(delta)` (satır ~30–100):
- Tüm aktif açık artırmaları (sequence <= 1) iter eder
- Her auction'ın `input_reserves + output_reserves` toplar
- Toplam bakiyeyi `asset_id → amount` map'e ekler
- VCB'yi (value circuit breaker) güncellenmiş değerlerle yazar

`write_shielded_pool_params()`: Shielded pool parametrelerini sıfırla
`write_fmd_params()`: FMD (Fuzzy Message Detection) parametrelerini sıfırla

---

### `pd/src/migrate/testnet77.rs` (89 satır)

**Genel Amaç:** Minimal migrasyon — sadece halt bit'i sıfırla.

---

### `pd/src/migrate/testnet78.rs` (615 satır)

**Genel Amaç:** Kapsamlı güvenlik ve temizleme migrasyonu.

**Sabit Limitler:**
- Validator name: ≤140 byte
- Validator website: ≤70 byte
- Validator description: ≤280 byte
- Proposal title: ≤80 byte
- Proposal description: ≤10000 byte
- Proposal key: ≤64 byte
- Proposal value: ≤2048 byte
- Proposal component: ≤64 byte
- Proposal client_id: ≤128 byte
- Proposal commit: ≤255 byte
- Proposal outcome reason: ≤1024 byte

**Kritik Fonksiyonlar:**

`floor_char_boundary(s, index)` (satır ~30–60) **[UTF-8 güvenli]:**
```rust
// UTF-8 char boundary'yi bul — emoji'lerin ortasından kesme
fn is_utf8_char_boundary(b: u8) -> bool {
    (b as i8) >= -0x40  // continuation byte değil
}
```

`truncate(s, max_bytes)` (satır ~65–90):
```rust
let floor = floor_char_boundary(s, max_bytes);
s.truncate(floor);
```
Rust'ın UTF-8 string slicing güvenliği — karakter sınırına snapping.

`truncate_validator_fields(delta)`: Validator meta verisini truncate eder
`truncate_proposal_fields(delta)`: Governance proposal meta verisi
`truncate_proposal_outcome_fields(delta)`: Proposal outcome meta verisi

`delete_empty_deleted_packet_commitments(delta)` (satır ~280–330):
- `ibc-data/commitments/` prefix'ini iter eder
- Boş değerli commitments'ları siler
- Bunlar "soft-deleted" paketlerin kalıntısı

`update_dex_params(delta)`:
- `max_execution_budget = 64` — DEX execution budget limiti

`reindex_dex_positions(delta)` (satır ~380–450) **[KRİTİK]:**
```rust
for position_id in open_positions {
    let pos = delta.position_by_id(&position_id).await?;
    // 1. Kapat
    delta.close_position_by_id(&position_id).await?;
    // 2. Index'ten sil
    delta.delete_position_from_index(&position_id).await?;
    // 3. Yeni index formatıyla yeniden aç
    delta.open_position(&pos).await?;
}
```
Tüm açık LP pozisyonlarını yeniden index'e yazar.

`update_cometbft_mempool_settings(comet_home)` (satır ~460–510):
- `max_txs_bytes = 10_485_760` (10 MiB)
- `max_tx_bytes = 30_720` (30 KiB)
- config.toml'u günceller

**Test:**
```rust
#[test]
fn test_truncation_of_emoji_strings_at_correct_byte_boundaries() {
    // "Hello 🌍" → 10 byte
    // truncate("Hello 🌍", 9) = "Hello " (emoji kesilmez, sınır korunur)
}
```

**Nova Shield İçin Ders:** Blockchain state'de kullanıcı girişi (validator name, proposal description) mutlaka byte limitiyle kısıtlanmalı, ve UTF-8 güvenli truncation kullanılmalı. `floor_char_boundary` pattern'ini kopyala.

---

### `pd/src/network.rs` (6 satır — modül bildirimleri)

`mod config; mod generate; mod join;`

---

### `pd/src/network/config.rs` (342 satır)

**Genel Amaç:** Penumbra node ağ konfigürasyonunu dosyaya yazar.

**Struct Listesi:**

```rust
pub struct NetworkTendermintConfig(TendermintConfig);
// TendermintConfig wrapper — embedded TOML template'inden oluşturulur

pub struct ValidatorKeys {
    pub validator_id_sk: SpendAuth,      // Spend authorization key
    pub validator_id_vk: VerificationKey, // Spend auth doğrulama
    pub validator_spend_key: SpendKeyBytes,
    pub validator_cons_sk: ed25519::SigningKey,  // CometBFT konsensüs key
    pub validator_cons_pk: ed25519::VerificationKey,
    pub node_key_sk: ed25519::SigningKey,  // P2P node key
    pub node_key_pk: ed25519::VerificationKey,
}
```

**Üç Key Tipi:**
1. **Identity Key** (SpendAuth): Validator kimliği, governance oyları
2. **Consensus Key** (Ed25519): Tendermint/CometBFT blok imzalama
3. **Node Key** (Ed25519): P2P peer-to-peer iletişim

**Fonksiyonlar:**

`ValidatorKeys::from_seed(seed: [u8; 32])` (satır ~80–120):
- Deterministic key türetme — aynı seed, aynı keyler
- Her key tipi için farklı domain separation

`ValidatorKeys::generate()` (satır ~125–155):
- `OsRng` ile rastgele key üretimi
- Production validator için

`NetworkTendermintConfig::new(...)` (satır ~160–200):
- moniker, peers, external_address ayarlar
- rpc_bind, p2p_bind port'larını yapılandırır

`write_config(home_dir)` (satır ~205–280):
- 6 dosya yazar:
  1. `genesis.json`
  2. `config.toml`
  3. `node_key.json`
  4. `priv_validator_key.json`
  5. `priv_validator_state.json`
  6. `validator_custody.json`

`parse_tm_address(addr, port, node_id)`:
- `<id>@<host>:<port>` veya `<host>:<port>` formatı
- Default port: 26656

`canonicalize_path(p)`: Tilde (~) genişletme — regex `^~/` ile eşleşir

`url_has_necessary_parts(url)`: Scheme + host + port hepsi zorunlu

**Nova Shield İçin Ders:** Validator node için 3 ayrı key tipi mantıklı separation of concerns: identity (chain-level), consensus (PBFT), P2P (network). Bu pattern Nova Shield validator architecture'ında kullanılmalı.

---

### `pd/src/network/generate.rs` (823 satır)

**Genel Amaç:** Yeni Penumbra ağı oluşturma.

**Struct Listesi:**

```rust
pub struct NetworkConfig {
    pub name: String,
    pub genesis: Genesis<AppState>,
    pub network_dir: PathBuf,
    pub network_validators: Vec<NetworkValidator>,
    pub validators: Vec<Validator>,
    pub peer_address_template: Option<String>,
    pub tendermint_timeout_commit: tendermint::Timeout,
}

pub struct NetworkValidator {
    pub identity_key: IdentityKey,
    pub consensus_key: PublicKey,
    pub name: String,
    pub website: String,
    pub description: String,
    pub enabled: bool,
    pub funding_streams: Vec<FundingStream>,
    pub sequence_num: u32,
}

pub struct NetworkAllocation {
    pub amount: Amount,
    pub denom: String,
    pub address: Address,
}
```

**Fonksiyonlar:**

`NetworkConfig::generate(...)` (satır ~80–250):
1. Validator listesini JSON/CSV'den veya default'tan topla
2. Allocation listesini CSV'den veya default'tan topla
3. Her validator için delegation allocation ekle
4. Dynamic allocation address ekle
5. `make_genesis_content()` ile AppState oluştur
6. `make_genesis()` ile genesis bloğu oluştur

`make_genesis_content()` (satır ~255–450) **[KRİTİK]:**
- Tüm zincir parametrelerini ayarlar
- Gas prices: `fixed_alt_gas_prices` — gm/gn için `simple_price * 10`
- SCT params: epoch_duration, unbonding_delay
- Governance: proposal_voting_blocks
- FMD params: precision, clue_plan_mem_limit
- IBC: özelleştirilebilir

`make_genesis()` (satır ~455–520) **[CometBFT Genesis]:**
- `MAX_BLOCK_TXS_PAYLOAD_BYTES`
- `max_gas = -1` — gas limiti yok
- `time_iota_ms = 500` — blok zaman adımı
- `max_age_num_blocks = 130000` — evidence max yaş
- `max_age_duration = 650000s` — evidence max yaş (süre)
- Validator tip: yalnızca Ed25519

`make_checkpoint(root_hash, genesis)` (satır ~525–545):
- `AppState::Checkpoint(root_hash_bytes)` — checkpointed genesis
- Migration sonrası genesis, tam genesis yerine checkpoint ile

`NetworkAllocation::simple(address)` (satır ~550–570):
- 100_000 * 10^6 upenumbra (100k UM)
- 1000 test_usd

`NetworkValidator::peering_address()` (satır ~590–620):
- Priority: external_address → peer_address_template → "127.0.0.1:26656"

`NetworkValidator::initial_state()` (satır ~625–640):
```json
{"height":"0","round":0,"step":0}
```

**Validation (satır ~700–750):**
```rust
if name.len() > 140 { bail!("validator name too long") }
if website.len() > 70 { bail!("validator website too long") }
if description.len() > 280 { bail!("validator description too long") }
```

**Testler (satır ~760–823):**
- `parse_allocations_from_good_csv`: CSV parsing doğrulama
- `parse_allocations_from_bad_csv`: Hatalı CSV error testi
- `generate_devnet_config`: Varsayılan devnet oluşturma
- `generate_network_config`: Özel validatorlü ağ

---

### `pd/src/network/join.rs` (448 satır)

**Genel Amaç:** Mevcut ağa join işlemi.

**Fonksiyonlar:**

`network_join(home_dir, node_name, grpc_url, ...)` (satır ~30–120):
1. CometBFT RPC'den genesis al
2. `fetch_listen_address()` çağır
3. `fetch_peers()` çağır (threshold=5, seeds ayrı tutulur)
4. Config yaz

`fetch_listen_address(rpc_url)` (satır ~125–175):
- `/net_info` → `listeners[0]` → adresi parse eder
- `/status` → `node_id` çeker
- Format: `node_id@addr:port`

`fetch_peers(rpc_url)` (satır ~180–250):
- `/net_info` → `peers` listesi
- `OsRng` ile shuffle — deterministik olmayan peer sırası
- Private/loopback adresleri filtrele: `address_could_be_external()` kontrolü
- Seed tespiti: `"seed"` içeriyorsa moniker'da → ayrı listeye
- Threshold: minimum 5 peer

`unpack_state_archive(url, home_dir)` (satır ~255–360):
- `file://` veya `https://` destekler
- Stream download → temp file
- `GzDecoder` + `tar::Archive::unpack()`
- Yeni `genesis.json` ve `priv_validator_state.json`'ı CometBFT dizinlerine kopyalar

`address_could_be_external(addr)` (satır ~365–410):
- Reddedilenler: private IPv4 (10.x, 172.16.x, 192.168.x), loopback (127.x, ::1), unspecified (0.0.0.0, ::)
- Gerekli: port olmalı

`parse_tm_address_listener(s)`:
- Regex: `Listener\(.*@(tcp://)?(.*)\)` — CometBFT listener format

**Testler:**
- `external_address_detection`: private/public adreslerin doğru tespiti
- `parse_tendermint_address_tcp`: `tcp://id@host:port` parse
- `parse_tendermint_address_listener`: `Listener(...)` format

---

### `pd/src/zipserve.rs` (68 satır)

**Genel Amaç:** Statik ZIP dosyasından HTTP yanıtları serve eder.

**Fonksiyonlar:**

`router(prefix: &str, archive_bytes: &'static [u8])` (satır ~15–35):
- axum Router oluşturur
- `GET /{prefix}` → `index.html`
- `GET /{prefix}/*path` → wildcard path

`serve_zip(path: Path<String>, archive_bytes: &'static [u8])` (satır ~40–68):
- `zip::ZipArchive::new(Cursor::new(archive_bytes))?`
- Trailing `/` → `/index.html` rewrite
- `mime_guess::from_path()` ile Content-Type
- 404 if not found

**Nova Shield İçin Ders:** Static frontend'i binary'ye embed etmek için mükemmel pattern. ZIP'i `include_bytes!` ile yükle, `zip` crate ile runtime'da parse et. Build script ile asset'lerin mevcut olduğunu doğrula.

---

### `pd/build.rs` (144 satır)

**Genel Amaç:** Build-time doğrulama ve testnet config embed.

**Fonksiyonlar:**

`check_frontend_asset_zipfiles()` (satır ~15–70):
```rust
// ZIP dosyaları >500 byte olmalı — git-lfs pointer değil
assert!(minifront.len() > 500, "minifront.zip is a git-lfs pointer, not an actual ZIP");
assert!(node_status.len() > 500, "node-status.zip is a git-lfs pointer, not an actual ZIP");
```
Git LFS pointer tespiti: gerçek ZIP dosyası her zaman >500 byte.

`setup_testnet_config()` (satır ~75–144):
- `testnets/` dizinini tarar
- `NNN-name` formatında en büyük N'i bulur (latest testnet)
- `PD_LATEST_TESTNET_NAME`, `PD_LATEST_TESTNET_ALLOCATIONS`, `PD_LATEST_TESTNET_VALIDATORS` env var'larını set eder
- Bu env var'lar `include_str!()` macro'su ile compile-time embed

---

### `pd/tests/network_integration.rs` (154 satır)

**Genel Amaç:** pd binary'sine yönelik integration testler.

**Test Fonksiyonları:**

`confirm_metrics_emission` (#[ignore], rstest parametric):
- `PENUMBRA_NODE_PD_METRICS_URL` env var ile (default: localhost:9000/metrics)
- Çeşitli metric pattern'lerini regex ile kontrol eder:
  - `cnidarium_get_raw_duration_seconds_count_seconds`
  - `pd_async_sleep_drift_microseconds`
  - `penumbra_dex_path_search_duration_seconds_count_seconds`
  - `penumbra_stake_missed_blocks{identity_key="..."}`

`check_cors_headers` (#[ignore]):
- `access-control-allow-origin: *`
- `access-control-expose-headers: *`

`check_minifront_http_ok` (#[ignore]):
- GET / → HTTP 200

`check_grpc_server_reflection` (#[ignore]):
- `grpcurl list` ile reflection API test
- `FILE_DESCRIPTOR_SET` üzerindeki tüm servislerin görünür olduğunu doğrular
- `get_all_grpc_services()`: `DescriptorPool::decode(FILE_DESCRIPTOR_SET)` → service listesi

---

## CRATE 2: pindexer — Penumbra Event Indexer

### Genel Amaç
Penumbra zincirindeki ABCI event'lerini PostgreSQL'e indexler. `cometindex` crate'inin `AppView` trait'i ile her event tipi için ayrı modüller. Tek binary, tek giriş noktası, birden fazla SQL view.

---

### `pindexer/src/main.rs` (15 satır)

```rust
Indexer::new(opts.cometindex)
    .with_default_tracing()
    .with_default_penumbra_app_views(&opts)
    .run()
    .await?;
```
Minimalist entry point — tüm logic `lib.rs` ve component modüllerinde.

---

### `pindexer/src/lib.rs` (38 satır)

**Struct:** `Options`
- `indexing_denom: String` — default: `passet1w6e7...` (USDC benzeri denom)
- `dex_ex_min_liquidity: f64` — default: `100_000_000` (100 UM minimum likidite)
- `dex_ex_ignore_arb: bool` — default: `true` — arb execution'ları volume'a sayma
- `block_time_s: f64` — default: `5.1` saniye (LQT hesaplamaları için)

---

### `pindexer/src/indexer_ext.rs` (25 satır)

**Genel Amaç:** `IndexerExt` trait — tüm Penumbra AppView'larını tek çağrıda kaydeder.

`with_default_penumbra_app_views()` kayıt sırası:
1. `Block` — temel blok detayları
2. `ValidatorSet` — validator durumu
3. `Slashings` — slash olayları
4. `DelegationTxs` — delegation tx'leri
5. `UndelegationTxs` — undelegation tx'leri
6. `GovernanceProposals` — governance
7. `dex_ex::Component` — DEX explorer (merkezi!)
8. `supply::Component` — token arzı
9. `ibc::Component` — IBC transferleri
10. `insights::Component` — genel chain insights
11. `lqt::Lqt` — Liquidity Quality Tournament

---

### `pindexer/src/parsing.rs` (27 satır)

`parse_content(value: Value) -> Content`:
- JSON Value'dan `AppState` deserialize eder
- `AppState::Content(c)` varyantını gerektirir
- `AppState::Checkpoint(_)` ise hata — checkpoint genesis'te parse yapılamaz

---

### `pindexer/src/block.rs` (68 satır)

**AppView:** `Block`

**SQL Şeması:**
```sql
CREATE TABLE IF NOT EXISTS block_details (
    height BIGINT PRIMARY KEY,
    root BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL
);
```

`index_batch()`: `EventBlockRoot` event'ini yakalar — her blok için `height + root + timestamp` kaydeder.

---

### `pindexer/src/stake.rs` (11 satır — re-export)

`pub use validator_set::ValidatorSet; pub use slashings::Slashings; ...`

---

### `pindexer/src/stake/validator_set.rs` (372 satır)

**AppView:** `ValidatorSet` (version 3)

**SQL Şeması:**
```sql
CREATE TABLE IF NOT EXISTS stake_validator_set (
    id SERIAL PRIMARY KEY,
    ik TEXT UNIQUE NOT NULL,          -- identity key
    name TEXT NOT NULL,
    definition JSONB NOT NULL,        -- tam validator tanımı
    voting_power BIGINT NOT NULL DEFAULT 0,
    queued_delegations BIGINT NOT NULL DEFAULT 0,
    queued_undelegations BIGINT NOT NULL DEFAULT 0,
    validator_state TEXT NOT NULL,    -- "Active", "Inactive", etc.
    bonding_state TEXT NOT NULL       -- "Bonded", "Unbonding(height)", etc.
);
```

**6 Event Tipi:**

`EventDefinitionUpload`: Yeni validator kaydı
- Yeni: `state=Defined, bonding=Unbonded, vp=0`
- Güncelleme: `definition` güncellenir

`EventDelegate(ik, amount)`:
- `queued_delegations += amount`

`EventUndelegate(ik, amount)`:
- `queued_undelegations += amount`

`EventVotingPowerChange(ik, new_power)`:
- `voting_power = new_power`
- `queued_delegations = 0, queued_undelegations = 0` — flush

`EventStateChange(ik, new_state)`:
- `validator_state` günceller

`EventBondingStateChange(ik, new_bonding_state)`:
- `bonding_state` günceller

**Genesis:** Tüm validatorler `Active/Bonded` başlar, delegation_amount shielded pool allocation'larından.

---

### `pindexer/src/stake/missed_blocks.rs` (83 satır)

**AppView:** `MissedBlocks`

**SQL Şeması:**
```sql
CREATE TABLE IF NOT EXISTS stake_missed_blocks (
    id SERIAL PRIMARY KEY,
    height BIGINT NOT NULL,
    ik TEXT NOT NULL
);
CREATE INDEX stake_missed_blocks_height ON stake_missed_blocks(height DESC);
CREATE INDEX stake_missed_blocks_ik ON stake_missed_blocks(ik);
CREATE INDEX stake_missed_blocks_height_ik ON stake_missed_blocks(height DESC, ik);
```

Event: `EventValidatorMissedBlock(ik)` → satır ekle

---

### `pindexer/src/stake/delegation_txs.rs` (93 satır)

**AppView:** `DelegationTxs`

**SQL Şeması:**
```sql
CREATE TABLE IF NOT EXISTS stake_delegation_txs (
    id SERIAL PRIMARY KEY,
    ik TEXT NOT NULL,
    amount BIGINT NOT NULL,
    height BIGINT NOT NULL,
    tx_hash BYTEA NOT NULL
);
```
Composite index: `(ik, height DESC)`

---

### `pindexer/src/stake/undelegation_txs.rs` (93 satır)

`DelegationTxs` ile aynı yapı, `stake_undelegation_txs` tablosu.

---

### `pindexer/src/stake/slashings.rs` (101 satır)

**AppView:** `Slashings`

**SQL Şeması:**
```sql
CREATE TABLE IF NOT EXISTS stake_slashings (
    id SERIAL PRIMARY KEY,
    height BIGINT NOT NULL,
    ik TEXT NOT NULL,
    epoch_index BIGINT NOT NULL,
    penalty TEXT NOT NULL    -- "0.1" gibi penalty yüzdesi
);
```

---

### `pindexer/src/supply.rs` (953 satır)

**Genel Amaç:** Toplam token arzını iki boyutuyla izler: unstaked (serbest) ve delegated (stake edilmiş).

**Yapılar:**

`unstaked_supply::Supply`:
```rust
struct Supply {
    um: u64,      // toplam serbest UM
    auction: i64, // auction'lardaki UM (+ veya -)
    dex: i64,     // DEX'teki UM (VCB)
    arb: u64,     // arb karından gelen UM
    fees: u64,    // toplanan ücretler
}
```

`delegated_supply::Supply`:
```rust
struct Supply {
    um: u64,       // UM cinsinden stake edilmiş değer
    del_um: u64,   // delegation token miktarı
    rate_bps2: u64, // exchange rate (BPS^2)
}
```

**Kritik Sabit:**
```rust
const BPS_SQUARED: u64 = 1_0000_0000;  // 10^8, genesis exchange rate
```

**SQL Şeması:**
```sql
CREATE TABLE supply_total_unstaked (
    height BIGINT PRIMARY KEY,
    um BIGINT NOT NULL,
    auction BIGINT NOT NULL,
    dex BIGINT NOT NULL,
    arb BIGINT NOT NULL,
    fees BIGINT NOT NULL
);

CREATE TABLE supply_validators (
    id SERIAL PRIMARY KEY,
    identity_key TEXT UNIQUE NOT NULL
);

CREATE TABLE supply_total_staked (
    validator_id INT NOT NULL REFERENCES supply_validators(id),
    height BIGINT NOT NULL,
    um BIGINT NOT NULL,
    del_um BIGINT NOT NULL,
    rate_bps2 BIGINT NOT NULL,
    PRIMARY KEY (validator_id, height)
);
```

**Kritik Hesaplamalar:**

`Supply::add_um(delta: i64)` (satır ~180–220):
```rust
let rate = rate_bps2 as f64 / BPS_SQUARED as f64;
let del_um_delta = delta as f64 / rate;
// Pozitif delta: round_down (kullanıcı daha az del_um alır)
// Negatif delta: round_up (kullanıcı daha fazla del_um öder)
```

`Supply::change_rate(rate_data)` (satır ~225–250):
```rust
let new_um = rate_data.unbonded_amount(self.del_um);
self.rate_bps2 = rate_data.validator_exchange_rate.value() as u64;
self.um = new_um;
```

**10 Event Tipi:**
1. `Undelegate` — delegated_supply azal, unstaked_supply artar
2. `Delegate` — unstaked_supply azal, delegated_supply artar
3. `FundingStreamReward` — yeni UM mint (inflation)
4. `RateDataChange` — validator exchange rate değişimi
5. `AuctionVCBCredit` — auction'dan UM girişi
6. `AuctionVCBDebit` — auction'a UM çıkışı
7. `DexVCBCredit` — DEX'ten UM girişi
8. `DexVCBDebit` — DEX'e UM çıkışı
9. `DexArb` — arb karı: `profit = output.amount - input.amount` (sadece UM→UM)
10. `BlockFees` — blok ücretleri

**Genesis:** Community pool + shielded pool allocation'larından toplam supply başlatılır.

**Nova Shield İçin Ders:** Token arzını iki boyutlu izlemek (unstaked + delegated) çok sağlam bir accounting. `BPS_SQUARED = 1_0000_0000` exchange rate formatı bps^2 cinsinden — hassasiyet kaybı minimuma iniyor. VCB (Value Circuit Breaker) pattern: DEX ve auction'lardaki token miktarını ayrı izle.

---

### `pindexer/src/governance.rs` (543 satır)

**AppView:** `GovernanceProposals` (version 1)

**SQL Şeması (3 tablo):**
```sql
CREATE TABLE governance_proposals (
    id SERIAL PRIMARY KEY,
    proposal_id BIGINT UNIQUE NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    payload JSONB NOT NULL,
    start_block_height BIGINT NOT NULL,
    end_block_height BIGINT NOT NULL,
    state TEXT NOT NULL  -- "Voting", "Finished/Passed", "Finished/Failed", "Finished/Slashed"
);

CREATE TABLE governance_validator_votes (
    id SERIAL PRIMARY KEY,
    proposal_id BIGINT NOT NULL REFERENCES governance_proposals(proposal_id),
    identity_key TEXT NOT NULL,
    vote TEXT NOT NULL,  -- "Yes", "No", "Abstain"
    voting_power BIGINT NOT NULL
);

CREATE TABLE governance_delegator_votes (
    id SERIAL PRIMARY KEY,
    proposal_id BIGINT NOT NULL,
    identity_key TEXT NOT NULL,  -- hangi validator için
    vote TEXT NOT NULL,
    voting_power BIGINT NOT NULL,
    tx_hash BYTEA
);
```

17 index (her tabloda composite ve tekil).

**8 Event Tipi:**
1. `EventProposalSubmit` → `state="Voting"`
2. `EventDelegatorVote` → delegator_votes ekle
3. `EventValidatorVote` → validator_votes ekle
4. `EventProposalWithdraw` → `state="Withdrawn"`
5. `EventProposalPassed` → `state="Finished/Passed"`
6. `EventProposalFailed` → `state="Finished/Failed"`
7. `EventProposalSlashed` → `state="Finished/Slashed"`
8. `EventProposalDepositClaim` → deposit geri alındı kaydı

---

### `pindexer/src/ibc/mod.rs` (256 satır)

**AppView:** `Component` (version 1)

**Enum'lar:**
```rust
enum EventKind { InboundTransfer, OutboundTransfer, OutboundRefund }
enum RefundReason { Unspecified, Timeout, Error }
```

**SQL Şeması:**
```sql
CREATE TABLE ibc_transfer (
    id SERIAL PRIMARY KEY,
    penumbra_addr TEXT NOT NULL,
    foreign_addr TEXT NOT NULL,
    asset_id BYTEA NOT NULL,
    -- NUMERIC(39,0) * sign: outbound negatif
    amount NUMERIC(39,0) NOT NULL,
    kind TEXT NOT NULL,   -- "inbound", "outbound", "refund_timeout", "refund_error", "refund_other"
    block_height BIGINT NOT NULL,
    tx_hash BYTEA
);
```

**Kritik:** `amount` için `NUMERIC(39,0)` — 128-bit integer PostgreSQL'de. Outbound negatif, inbound pozitif. Refund'lar total_value'ya sayılmaz (`refund=true`).

`reset()`: Versioned schema reset — AppView version değişince tablolar drop + recreate.

---

### `pindexer/src/dex_ex/mod.rs` (1649 satır)

**Genel Amaç:** Kapsamlı DEX explorer — candlestick'ler, LP pozisyon takibi, batch swap traces, aggregate statistics.

**[DEX MODÜLÜ — 3X HASSASİYET]**

---

#### Candlestick Sistemi

**Struct:**
```rust
pub struct Candle {
    pub open: f64,
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub direct_volume: f64,   // LP'den direkt exchange
    pub swap_volume: f64,     // batch swap volume
}
```

**Window Enum:**
```rust
pub enum Window { W1m, W15m, W1h, W4h, W1d, W1w, W1mo }
```

**`Window::anchor(time)` (satır ~50–100):**
```rust
W1m  → minute boundary
W15m → m - (m % 15)
W1h  → hour boundary
W4h  → h - (h % 4)
W1d  → day boundary
W1w  → weekday = 0 (Monday), subtract weekday*1day
W1mo → day = 1, subtract (day-1)*1day
```
Zaman snap'i — her pencere için başlangıç noktasını belirler.

**`Window::subtract_from(time)` (satır ~105–125):**
```rust
W1m  → 1 dakika çıkar
W15m → 15 dakika çıkar
W1h  → 1 saat çıkar
W4h  → 4 saat çıkar
W1d  → 1 gün çıkar
W1w  → 7 gün çıkar
W1mo → 30 gün çıkar
```

**`Candle::merge(&mut self, other: &Candle)`:**
```rust
// open korunur (ilk değer)
self.close = other.close;        // en son close
self.low = self.low.min(other.low);
self.high = self.high.max(other.high);
self.direct_volume += other.direct_volume;
self.swap_volume += other.swap_volume;
```

**`Candle::point(price, volume)`:**
```rust
Candle { open: price, close: price, low: price, high: price,
         direct_volume: volume, swap_volume: 0.0 }
```

**`WindowedCandle`:**
```rust
struct WindowedCandle { start: DateTime, window: Window, candle: Candle }
```

`with_candle(now, new_candle)` — yeni timestamp başka bir pencereye düşüyorsa eski candle'ı döner (flush trigger), aynı penceredeyse merge eder.

---

#### Price Chart Context

`PriceChartContext::load(dbtx, start, end, window)`: Mevcut en son candle'ı PostgreSQL'den yükler.

`PriceChartContext::update(dbtx, now, candle)`: Yeni candle gelince, pencere değişmişse flush (eski candle'ı yaz), aynı penceredeyse merge.

`PriceChartContext::unload(dbtx)`: Batch sonunda son candle'ı yaz.

**SQL tablo: `dex_ex_price_charts`:**
```sql
(id, asset_start, asset_end, the_window, start_time, open, close, high, low, direct_volume, swap_volume)
UNIQUE(asset_start, asset_end, the_window, start_time)
ON CONFLICT DO UPDATE SET ...
```

---

#### Summary Context

`summary::Context` — her çift için block-bazlı snapshot:
- `price: f64` — son close
- `liquidity: f64` — kümülatif likidite değişimi
- `start_price_indexing_denom: f64` — indexing denom cinsinden fiyat

`summary::update_summaries(dbtx, now, window)`:
- Tüm çiftler için `dex_ex_pairs_summary` upsert
- SQL CTE: `previous` (pencere başındaki snapshot) + `now` (son snapshot) + `sums` (pencere toplamları)
- Hesaplananlar: `price_change`, `direct_volume_over_window`, `swap_volume_over_window`, `trades_over_window`, `high`, `low`

`summary::update_aggregate_summary(dbtx, window, denom, min_liquidity)`:
- Büyük SQL CTE: eligible_denoms → converted_pairs_summary → sums → total_liquidity → largest_sv → largest_dv → top_price_mover → counts
- `dex_ex_aggregate_summary` tablosuna tek satır per window

---

#### Events Struct (DEX Event Aggregator)

```rust
struct Events {
    time: Option<DateTime>,
    height: i32,
    candles: HashMap<DirectedTradingPair, Candle>,
    metrics: HashMap<DirectedTradingPair, PairMetrics>,
    positions: BTreeMap<PositionId, Position>,
    position_opens: Vec<EventPositionOpen>,
    position_executions: Vec<EventPositionExecution>,
    position_closes: Vec<EventPositionClose>,
    position_withdrawals: Vec<EventPositionWithdraw>,
    batch_swaps: Vec<EventBatchSwap>,
    swaps: BTreeMap<TradingPair, Vec<EventSwap>>,
    swap_claims: BTreeMap<TradingPair, Vec<EventSwapClaim>>,
    position_open_txs: BTreeMap<PositionId, [u8; 32]>,
    position_close_txs: BTreeMap<PositionId, [u8; 32]>,
    position_withdrawal_txs: BTreeMap<PositionId, [u8; 32]>,
}
```

**`Events::extract(block, ignore_arb)` (satır ~828–927):**
Tüm event'leri parse eder:
- `EventBlockRoot` → timestamp al
- `EventPositionOpen` → reserves_change + position kaydet
- `EventPositionWithdraw` → reserves_change (removed=true)
- `EventPositionExecution` → reserves_change (old → new)
- `EventPositionClose` → close list
- `EventQueuePositionClose` → tx hash kaydet
- `EventSwap` → pair bazında liste
- `EventSwapClaim` → pair bazında liste
- `EventLqtPositionReward` → liquidite değişimi (LQT reward ekle)
- `EventBatchSwap` → her iki yön için `with_swap_execution()`
- `EventArbExecution` → `ignore_arb` false ise volume say

**`Events::with_trace(input, output)` (satır ~737–767):**
```rust
let price_1_2 = output_amount / input_amount;
let candle_1_2 = Candle::point(price_1_2, input_amount);
let candle_2_1 = Candle::point(1.0 / price_1_2, output_amount);
// Her iki yön için candle ekle
```

**`Events::with_swap_execution(se)` (satır ~769–791):**
Her trace çifti için fiyat hesapla; ek olarak toplam input/output'u `swap_volume`'a ekle.

**`Events::with_reserve_change(pair, old, new, removed)` (satır ~793–826):**
```rust
let diff_1 = new.r1 - old.r1 (veya ±new.r1 if removed/added)
let diff_2 = new.r2 - old.r2
// pair_1_to_2: diff_2 (karşı tarafın likidite değişimi)
// pair_2_to_1: diff_1
metrics[pair].liquidity_change += diff
```

**`Events::load_positions(dbtx)` (satır ~929–965):**
- position_executions'taki eksik pozisyonları `dex_ex_position_state` tablosundan yükler
- `SELECT position_raw FROM dex_ex_position_state WHERE position_id = ANY($1)`

---

#### Component AppView

**`Component::new(denom, min_liquidity, ignore_arb_executions)`**

**`record_position_open(dbtx, time, height, tx_hash, event)` (satır ~991–1071):**
1. `dex_ex_position_reserves` INSERT → `rowid` al
2. `dex_ex_position_state` INSERT:
   - `p, q` — fiyat oranı parametreleri
   - `fee_bps` — ücret
   - `effective_price_1_to_2`, `effective_price_2_to_1` — `phi.orient_start().effective_price()`
   - `position_raw` — proto encoded position
   - `opening_reserves_rowid`

**`record_position_execution(dbtx, time, height, event, positions)` (satır ~1276–1349):**
- `Flows::from_phi_and_reserves(&position.phi, &current, &prev)` — akış hesabı
- `delta_1, delta_2`: ne kadar girdi
- `lambda_1, lambda_2`: ne kadar çıktı
- `fee_1, fee_2`: ücret miktarı
- `context_asset_start, context_asset_end`: swap yönü

**`record_position_withdraw(...)` (satır ~1376–1425):**
- Withdrawal sonrası reserves sıfırlanır: `$4, $4` (zero twice)

**`index_batch()` (satır ~1516–1647) — ANA FONKSİYON:**
Blok bazında işlem sırası:
1. `metadata::set(dbtx, denom)` — indexing denom kaydet
2. Her blok için:
   a. `Events::extract()` — event'leri parse et
   b. `record_all_transactions()` — tx'leri kaydet
   c. `events.load_positions()` — eksik pozisyonları yükle
   d. `record_block_summary()` — blok özeti
   e. `record_batch_swap_traces()` — swap trace'leri
   f. Position open/execute/close/withdraw kaydet
   g. Her candle için 7 window'da `PriceChartContext::update()`
   h. Snapshot context güncelle
3. Son batch (`ctx.is_last()`):
   - Her window için `update_summaries()` ve `update_aggregate_summary()`
4. Chart'ları flush (`chart.unload()`)

**DEX Uygulama Notu:**
Nova Shield DEX için candlestick aggregation algoritması direkt uygulanabilir:
- `Window::anchor()` — zaman snap
- `Candle::merge()` — period birleştirme
- Dual volume tracking: direct_volume (LP fill) vs swap_volume (batch swap)
- Descending sıra için complement trick (testnet74'ten)
- `ignore_arb=true` default — arb volume'u özel hesapla

---

### `pindexer/src/insights/mod.rs` (581 satır)

**Genel Amaç:** Chain-wide istatistikler — toplam arz, staking oranı, shielded pool akışları.

**Struct'lar:**

```rust
struct ValidatorSupply { del_um: u64, rate_bps2: u64 }
struct Supply { total: u64, staked: u64, price: Option<f64> }
```

**SQL Şeması:**
```sql
-- Validator başına historical supply
CREATE TABLE _insights_validators (
    validator_id TEXT NOT NULL,
    height BIGINT NOT NULL,
    del_um BIGINT NOT NULL,
    rate_bps2 BIGINT NOT NULL,
    PRIMARY KEY (validator_id, height)
);

-- Chain-wide toplam supply
CREATE TABLE insights_supply (
    height BIGINT PRIMARY KEY,
    total BIGINT NOT NULL,
    staked BIGINT NOT NULL,
    price FLOAT,
    price_numeraire_asset_id BYTEA
);

-- Shielded pool token akışları
CREATE TABLE insights_shielded_pool (
    asset_id BYTEA NOT NULL,
    height BIGINT NOT NULL,
    total_value NUMERIC(78,0),   -- kümülatif giriş (NUMERIC: büyük sayı)
    current_value NUMERIC(78,0), -- mevcut değer (çıkış çıkarılmış)
    unique_depositors INT NOT NULL,
    PRIMARY KEY (asset_id, height)
);

CREATE TABLE _insights_shielded_pool_depositors (
    asset_id BYTEA,
    address TEXT,
    PRIMARY KEY (asset_id, address)
);
```

**Önemli Fonksiyon:**

`convert_ceil(amount, inv_rate_bps2)` (satır ~32–36):
```rust
(u128::from(amount) * 1_0000_0000).div_ceil(u128::from(inv_rate_bps2))
```
Tavan bölme: undelegation sırasında del_um azaltma için.

`modify_validator_supply(dbtx, height, ik, f)` — closure ile validator supply güncelle, upsert.

`modify_supply(dbtx, height, price_numeraire, f)` — closure ile chain supply güncelle.

**Component::index_event() — 10 Event:**
1. `EventUndelegate` → validator del_um azalt (ceiling), staked azalt
2. `EventDelegate` → validator del_um artır, staked artır
3. `EventRateDataChange` → `delta = new_um - old_um`, total ve staked güncelle (yeni token mint/burn)
4. `EventBlockFees` → total azalt (yakılan ücret)
5. `EventArbExecution` → total azalt (UM→UM arb karı yakılır)
6. `EventFundingStreamReward` → total artır (yeni mint)
7. `EventInboundFungibleTokenTransfer` → shielded pool artır, depositor kaydet
8. `EventOutboundFungibleTokenTransfer` → shielded pool azalt (negatif flow)
9. `EventOutboundFungibleTokenRefund` → shielded pool düzelt (refund=true, total_value'ya sayılmaz)
10. `EventCandlestickData` → price_numeraire ile UM fiyatını güncelle

**`version()` implementasyonu:**
- `blake2b_simd` ile `price_numeraire` hash'i
- `Version::default().with_option_hash(hash)` — konfigürasyona göre schema versioning

**Nova Shield İçin Ders:** `insights` modülü ekonomik istatistiklerin nasıl tutulacağını mükemmel gösteriyor. `NUMERIC(78,0)` kullanımı — 78 basamak, 128-bit değerlerden büyük (overflow imkansız). `unique_depositors` sayacı için ayrı tablo pattern'i.

---

### `pindexer/src/lqt/mod.rs` (440 satır)

**Genel Amaç:** Liquidity Quality Tournament (LQT) — LP'lere verilecek ödülleri ve oy sonuçlarını izler.

**Struct:**
```rust
pub struct Lqt { block_time_s: f64 }
```

**Parameters:**
```rust
struct Parameters {
    funding: FundingParameters,        // gauge_threshold, delegator_share
    sct: SctParameters,               // epoch_duration
    distribution: DistributionsParameters, // rewards_per_block
}
```

**SQL Şeması (schema.sql'den):**
```sql
-- Epoch bazında parametreler
CREATE TABLE lqt._params (
    epoch BIGINT PRIMARY KEY,
    delegator_share NUMERIC,     -- % olarak
    gauge_threshold NUMERIC,     -- % olarak
    epoch_duration BIGINT,
    rewards_per_block BIGINT
);

-- Meta: mevcut height ve block_time
CREATE TABLE lqt._meta (rowid INT PRIMARY KEY, current_height BIGINT, block_time_s FLOAT);

-- Epoch başlangıç ve bitiş bilgisi
CREATE TABLE lqt._epoch_info (
    epoch BIGINT PRIMARY KEY,
    start_block BIGINT NOT NULL,
    updated_block BIGINT NOT NULL,
    end_block BIGINT,
    available_rewards NUMERIC
);

-- Biten epoch'lar
CREATE TABLE lqt._finished_epochs (epoch BIGINT PRIMARY KEY);

-- Delegator ödülleri
CREATE TABLE lqt._delegator_rewards (
    epoch BIGINT NOT NULL,
    address BYTEA NOT NULL,
    amount NUMERIC NOT NULL,
    PRIMARY KEY (epoch, address)
);

-- LP ödülleri ve istatistikleri
CREATE TABLE lqt._lp_rewards (
    epoch BIGINT NOT NULL,
    position_id BYTEA NOT NULL,
    asset_id BYTEA,
    amount NUMERIC,
    executions BIGINT,
    um_volume NUMERIC,
    asset_volume NUMERIC,
    um_fees NUMERIC,
    asset_fees NUMERIC,
    points NUMERIC,
    PRIMARY KEY (epoch, position_id)
);

-- Oylar
CREATE TABLE lqt._votes (
    id SERIAL PRIMARY KEY,
    epoch BIGINT NOT NULL,
    power NUMERIC NOT NULL,
    asset_id BYTEA NOT NULL,
    address BYTEA NOT NULL
);
```

**Private modüller:**
- `_params::set_initial()`, `_params::set_epoch()` — gauge_threshold ve delegator_share yüzde cinsinden
- `_meta::update_with_batch()` — her batch sonunda current_height güncelle
- `_epoch_info::start_epoch()`, `end_epoch()`, `set_rewards_for_epoch()`, `current()`
- `_finished_epochs::declare_finished()` — idempotent
- `_delegator_rewards::add()` — ON CONFLICT akümülatör
- `_lp_rewards::add_reward()`, `add_flows()` — volume, fee, point akümülatörleri
- `_votes::add()` — oy ekle

**`index_event()` (satır ~331–378) — 6 Event:**
1. `EventLqtVote` → `_votes::add(epoch, power, asset_id, addr)`
2. `EventLqtDelegatorReward` → `_delegator_rewards::add(epoch, addr, amount)`
3. `EventLqtPositionReward` → `_lp_rewards::add_reward(epoch, position_id, amount)`
4. `EventLqtPositionVolume` → `_lp_rewards::add_flows(epoch, position_id, ...)` (volume + fees + points)
5. `EventEpochRoot` → epoch bitir + yeni epoch başlat
6. `EventLqtPoolSizeIncrease` → epoch'un available_rewards güncelle
7. `EventAppParametersChange` → `_params::set_epoch(current_epoch, new_params)`

**`version()` implementasyonu:**
```rust
Version::with_major(4).with_option_hash(hash)
```
Major version 4 — önceki şemalarla uyumsuz, reset gerekli.

**`on_startup()` — Schema Migration:**
- `schema.sql` çalıştır
- `_epoch_info::start_epoch(dbtx, 1, 1)` — epoch 1'i başlat (genesis)

**Nova Shield İçin Ders:** LQT sistemi öğretici bir incentive/gamification mekanizması. LP'leri volume, fee ve "points" üzerinden ödüllendirme — `points` ayrı takip edilip ödüle çevrilebilir. ON CONFLICT akümülatör pattern'i (lp_rewards) idempotent batch işleme için ideal.

---

## CRATE 3: pmonitor — Wallet Compliance Monitor

### Genel Amaç
FVK (Full Viewing Key) listesi verilen Penumbra cüzdanlarının genesis bakiyelerini ve mevcut bakiyelerini karşılaştırır. Compliance monitoring aracı.

---

### `pmonitor/src/main.rs` (590 satır)

**Sabitler:**
- `MAX_CB_SIZE_BYTES = 12 * 1024 * 1024` — Compact Block max boyut (12 MiB)
- `VIEW_FILE_NAME = "pcli-view.sqlite"` — SQLite view dosyası
- `ALLOWED_DISCREPANCY = 0.1` — izin verilen kayıp (0.1 UM)

**Enum:** `Command: Init | Audit | Reset`

**Fonksiyonlar:**

`view(home, fvk, grpc_url)` (satır ~60–90):
- `ViewServer::load_or_initialize()` — SQLite view server başlat
- `ViewClient` döner

`sync(view_client)` (satır ~95–120):
- `ViewClient::status_stream()` — sync stream
- Her 100 blokta bir log: "synced X blocks"
- Stream bitince sync tamamlandı

`fetch_genesis_compact_block(grpc_url)` (satır ~125–160):
- gRPC bağlantısı
- `CompactBlock` request: `height = 0`
- `MAX_CB_SIZE_BYTES` ile decode limit

`create_wallet(fvk, grpc_url, wallet_dir)` (satır ~165–190):
- `PcliConfig { full_viewing_key, custody: ViewOnly }` — imzasız view-only config
- `config.toml` yazar
- `ViewServer::load_or_initialize()` ile SQLite başlatır

`compute_um_equivalent_balance(view_client)` (satır ~195–240):
- `view_client.balances()` — tüm notlar
- UM notlar: direkt topla
- Delegation token notlar: `RateData::unbonded_amount()` ile UM'e çevir (mevcut exchange rate)
- Toplam UM eşdeğer bakiye döner

**`exec()` — Ana Komut Yürütücü (satır ~245–500):**

**`Command::Init` (satır ~250–340):**
1. Config dosyasından FVK listesini oku
2. Genesis compact block'u getir
3. Her FVK için:
   a. `create_wallet()` — SQLite oluştur
   b. `scan_genesis_block()` — genesis balances
   c. `Uuid::new_v4()` ile wallet_id
   d. `AccountConfig::new(FvkEntry, genesis_balance)`
4. `PmonitorConfig` TOML olarak yaz

**`Command::Audit` (satır ~345–480):**
1. Config yükle
2. Her hesap için:
   a. Active FVK'yı sync et
   b. Mevcut UM bakiyesini hesapla
   c. **Migration tespiti:** `view_client.transaction_memos()` — `"Migrating balance from X%"` pattern arama
   d. Migration varsa: `parse_dest_fvk_from_memo()` ile dest FVK bul
   e. Yeni FVK için wallet oluştur ve config güncelle
3. `check_wallet_compliance()` — bakiye kontrolü
4. `emit_summary_message()` — renkli özet

`check_wallet_compliance(current, genesis)` (satır ~485–510):
```rust
if current >= genesis { return Ok(true); }
let discrepancy = genesis - current;
let allowed = Amount::from(ALLOWED_DISCREPANCY * 1_000_000); // 0.1 UM
discrepancy <= allowed
```
0.1 UM'ye kadar kayıp tolere edilir (gas fees hesabı).

`emit_summary_message(results)` (satır ~515–550):
- Her hesap için renkli output: PASS (yeşil) / FAIL (kırmızı)
- Herhangi failure varsa: `anyhow::bail!("compliance check failed")` — process exit code non-zero

---

### `pmonitor/src/config.rs` (118 satır)

**Struct Listesi:**

```rust
pub struct FvkEntry {
    pub fvk: FullViewingKey,
    pub wallet_id: Uuid,
}

pub struct AccountConfig {
    original: FvkEntry,       // genesis FVK
    genesis_balance: Amount,
    migrations: Vec<FvkEntry>, // migrate balance ile taşınanlar
}

pub struct PmonitorConfig {
    grpc_url: Url,
    accounts: Vec<AccountConfig>,
}
```

**Metodlar:**

`AccountConfig::active_wallet()`:
- `migrations.is_empty()` → `original`
- Değilse: `migrations.last()` — son migration hedefi aktif wallet

`parse_dest_fvk_from_memo(memo)`:
```rust
let re = Regex::new(r"Migrating balance from .+ to (.+)")?;
// captures[1] → dest FVK string
```
Memo formatı: `"Migrating balance from <src_fvk> to <dest_fvk>"`

---

### `pmonitor/src/genesis.rs` (117 satır)

**Struct:**
```rust
pub struct FilteredGenesisBlock {
    pub notes: BTreeMap<String, BTreeMap<StateCommitment, Note>>,
    pub balances: BTreeMap<String, Amount>,
}
```

**`scan_genesis_block(block, fvks)` (satır ~30–117):**

Base rate genesis'de:
```rust
let base_rate = BaseRateData {
    epoch_index: 0,
    base_reward_rate: 0u128.into(),
    base_exchange_rate: 1_0000_0000u128.into(), // 1:1 oranı
};
```

Her FVK için paralel trial decryption:
```rust
let trial_decrypt_note = |note_payload: NotePayload| -> JoinHandle<Option<Note>> {
    let fvk2 = fvk.clone();
    tokio::spawn(async move { note_payload.trial_decrypt(&fvk2) })
};
```

Note tipleri:
- UM note → direkt bakiye ekle
- Delegation token → `RateData::unbonded_amount()` ile 1:1 UM'e çevir (genesis'te her zaman 1:1)
- Diğer asset → `tracing::warn!()` ve atla

---

## CRATE 4: elcuity — Automated LQT Bot

### Genel Amaç
Liquidity Quality Tournament (LQT) için otomatik bot. Seçilen IBC asset için oylama, LP sağlama ve swap döngüsü yapar.

---

### `elcuity/src/main.rs` (41 satır)

```rust
init_tracing();
let opt = Opt::parse();
rustls::crypto::aws_lc_rs::install_default_provider()?;
opt.run().await?;
```

---

### `elcuity/src/lib.rs` (59 satır)

**Struct:**
```rust
pub struct Opt {
    #[clap(long)] grpc_url: String,
    #[clap(long)] view_service: String,
    command: Command,
}

pub enum Command {
    Vote(vote::Opt),
    Lp(lp::Opt),
    Swap(swap::Opt),
}
```

`run()`: `Clients::init(grpc_url, view_service)` → command dispatch

---

### `elcuity/src/clients.rs` (81 satır)

**Struct:**
```rust
pub struct Clients {
    node_channel: Channel,
    custody_client: CustodyServiceClient<BoxGrpcService>,
    view_client: ViewServiceClient<BoxGrpcService>,
}
```

**Type Alias'lar:**
- `SctQueryServiceClient` — epoch sorguları
- `DexQueryServiceClient` — LP sorguları
- `DexSimulationServiceClient` — fiyat simülasyonu

`Clients::init(node_url, view_url)`:
- `ViewServer::get_pd_channel()` — node bağlantısı
- `box_grpc_svc::connect(endpoint)` — view/custody bağlantısı
- Tek BoxGrpcService hem ViewClient hem CustodyClient için paylaşılır

`clients.view()` → `Box<DynViewClient>` (dyn dispatch)
`clients.custody()` → `Box<DynCustodyClient>` (dyn dispatch)
`clients.sct_query_service()` → gRPC channel klonu
`clients.dex_simulation_service()` → gRPC channel klonu
`clients.dex_query_service()` → gRPC channel klonu

---

### `elcuity/src/planner.rs` (63 satır)

**İki Fonksiyon:**

`submit(clients, plan)` (satır ~13–43):
```rust
// 1. custody.authorize(plan) → auth_data
// 2. view.witness_and_build(plan, auth_data) → Transaction
// 3. view.broadcast_transaction(tx, await=true) → stream
// 4. BroadcastStatus::BroadcastSuccess → log
//    BroadcastStatus::Confirmed → break + return tx
```

`build_and_submit<F>(clients, source, add_to_plan: F)` (satır ~45–62):
```rust
// Generic: F: FnOnce(Planner<OsRng>) -> Future<Output=Result<Planner<OsRng>>>
let mut planner = Planner::new(OsRng);
planner.set_gas_prices(view.gas_prices().await?);
let planner = add_to_plan(planner).await?;
let plan = planner.plan(view.as_mut(), source).await?;
submit(clients, plan).await
```
Closure-based planner API — esnek action ekleme.

---

### `elcuity/src/vote.rs` (90 satır)

**Genel Amaç:** Her epoch başında LQT için oy ver, oylama notlarını konsolide et.

**`stream_epochs(clients)` (satır ~11–40):**
```rust
const POLL_S: u64 = 30; // 30 saniyede bir kontrol
try_stream! {
    loop {
        let status = view.status().await?;
        if status.catching_up { continue; } // sync bitmemişse atla
        let current_epoch = sct.epoch_by_height(height).await?...;
        if current_epoch > prev_epoch {
            yield current_epoch;
        }
        sleep_until(start + 30s).await;
    }
}
```
Yeni epoch başlayınca yield eder.

**`vote(clients, epoch, denom)` (satır ~42–67):**
```rust
build_and_submit(clients, Default::default(), |mut planner| async {
    let rewards_addr = view.address_by_index(0).await?;
    let voting_notes = view.lqt_voting_notes(epoch, None).await?;

    // Oy ver
    planner.lqt_vote(epoch, denom, rewards_addr, &voting_notes);

    // Notları konsolide et (bir sonraki epoch için)
    for note in voting_notes {
        planner.spend(note.note, note.position);
    }
    Ok(planner)
})
```
Oy + konsolidasyon aynı tx'te — gas verimli.

**`Opt { denom: String }`** — `transfer/channel-X/uusdcXXX` formatında IBC denom

`Opt::run()`: Sonsuz döngü — her yeni epoch'ta `vote()` çağır.

---

### `elcuity/src/swap.rs` (129 satır)

**Genel Amaç:** Otomatik döngüsel swap — UM → asset → UM → asset...

**`swap(clients, input, to)` (satır ~19–81):**
```rust
// 1. Planner kur: swap(input, to, estimated_claim_fee, claim_addr)
// 2. Plan'dan swap_plaintext çıkar (claim için lazım)
// 3. submit() ile swap gönder
// 4. view.swap_by_commitment(plaintext.commit()) — SwapRecord al
// 5. pro_rata_outputs hesapla
// 6. SwapClaimPlan oluştur (proof_blinding_r/s: OsRng)
// 7. submit() ile claim tx gönder
// 8. Output amount döner
```

`estimated_claim_fee`: `gas_prices.fee(&swap_claim_gas_cost())` — claim gas maliyeti swap önceden ödenir.

**`Opt`:**
```rust
pub struct Opt {
    starting_amount: u32, // UM cinsinden başlangıç
    cycle: Vec<String>,   // döngü varlıkları
}
```

**`Opt::run()` (satır ~99–129):**
```rust
let mut cycle = iter::once(*STAKING_TOKEN_ASSET_ID)
    .chain(asset_ids.into_iter())
    .cycle()
    .peekable(); // sonsuz döngü

let mut amount = starting_amount;
while let Some(asset_id) = cycle.next() {
    if asset_id == UM { amount = amount.max(starting_amount); } // top-up
    let next = cycle.peek()?;
    amount = swap(clients, Value{asset_id, amount}, next).await?;
}
```
`iter::cycle()` ile sonsuz döngü. UM'e döndüğünde balance'ı `starting_amount`'a top-up eder (kayıp telafi).

---

### `elcuity/src/lp/mod.rs` (354 satır)

**Genel Amaç:** Otomatik likidite sağlama — mevcut spread'e göre LP aç, her 120 saniyede güncelle.

**Sabitler:**
- `POSITION_CHUNK_SIZE = 5` — toplu close/withdraw için batch boyutu
- `DEFAULT_PRICE: u8 = 1` — fiyat yoksa varsayılan 1:1
- `DEFAULT_FEE_BPS: u16 = 500` — fiyat yoksa 5% fee
- `ADJUST_WAIT_SECS: u64 = 120` — her 120 saniyede yeniden ayarla

**`close_all_positions(clients, pair)` (satır ~34–55):**
- `view.owned_position_ids(State::Opened, Some(pair))` → ID listesi
- Chunk'lar halinde `planner.position_close()` batch tx

**`withdraw_all_positions(clients, pair)` (satır ~57–89):**
- `view.owned_position_ids(State::Closed, Some(pair))` → ID listesi
- Her position için `dex_query_service.liquidity_position_by_id()` ile reserves çek
- `planner.position_withdraw(id, reserves, pair, sequence=0)`

**`get_sell_price(clients, pair)` (satır ~91–130):**
```rust
// 1_000_000 birim input ile simüle et
simulate_trade(input=1_000_000, output=pair.end)
// İlk trace: first[start] / first[end] → fiyat
U128x128::ratio(last_value.amount, first_value.amount)
```

**`spread(clients, pair)` (satır ~132–144):**
```rust
let sell_price = get_sell_price(clients, pair)?; // pair: A→B
let other_sell_price = get_sell_price(clients, pair.flip())?; // B→A
let buy_price = 1.0 / other_sell_price;
(buy_price, sell_price)
```

**`spread_to_price_and_fee_bps(buy, sell)` (satır ~146–176) [KRİTİK]:**
```rust
// buy >= sell garantile
let epsilon = U128x128 { bytes: [0..0, 1] }; // en küçük değer
let buy = buy - epsilon;
let sell = sell + epsilon;
// Midpoint: (buy + sell) / 2
let midpoint = (buy + sell) / 2;
// Fee: f = (buy/midpoint) - 1
//   => x(1+f) = buy, x(1-f) = sell, x = midpoint
let fee = (buy / midpoint) - 1;
let fee_as_u16 = min(5000, fee * 10_000);  // max 50% fee
```
Matematiksel türetme: bid-ask spread'den optimal LP fee hesabı.

**`price_to_p_q(price, precision)` (satır ~197–222):**
```rust
// price = p/q rasyonel olarak ifade et
let mut p = price; // U128x128
let mut q = 1;
while !p.is_integral() && p < precision && q < precision {
    p = p * 2;
    q = q * 2;
}
(p.round_down(), q)
```
Floating point fiyatı integer oranına çevir (precision < 2^60).

**`make_position(balance_limits, pair, liquidity_target, price, fee_bps)` (satır ~224–260):**
```rust
let half_liquidity_target = liquidity_target.amount / 2;
let other_target = price * half_liquidity_target; // karşı taraf
// Balance limit uygula
start_target = min(start_target, balance_limits[pair.start]);
end_target = min(end_target, balance_limits[pair.end]);
let (p, q) = price_to_p_q(price, 1u64 << 60);
Position::new(OsRng, pair, fee_bps, p, q, Reserves{r1: start_target, r2: end_target})
```

**`calculate_balance_limits(clients)` (satır ~179–195):**
```rust
// Tüm bakiyeyi al
// Staking token: 90% kullan (%10 fee için sakla)
*v -= v * 10 / 100
```

**`adjust_liquidity_provision(clients, asset, target, default_price)` (satır ~285–300):**
1. `close_all_positions()` — mevcut LP'leri kapat
2. `withdraw_all_positions()` — reserves çek
3. `create_position_at_current_spread()` — yeni LP aç

**`Opt::run()` — Sonsuz Döngü:**
```rust
loop {
    let start = Instant::now();
    adjust_liquidity_provision(...).await; // hata logla, bail etme
    sleep_until(start + 120s).await;
}
```

**DEX Uygulama Notu:**
Nova Shield DEX için `spread_to_price_and_fee_bps` formülü değerli:
- `midpoint = (buy + sell) / 2`
- `fee = buy/midpoint - 1`
Bu formül LP'nin spread üzerinden optimal fee belirleme mantığını gösteriyor.

---

## CRATE 5: pclientd — Penumbra Client Daemon

### Genel Amaç
Penumbra cüzdan daemon'ı. İki mod: view-only (yalnızca FVK) ve custody (imzalama yetkisi). gRPC API'yi 11 proxy servis olarak sunar, opsiyonel SoftKms ile güvenli imzalama.

---

### `pclientd/src/main.rs` (35 satır)

```rust
// r1cs loglarını kapat! 16GB+ memory kullanımından kaçın
tracing_subscriber::EnvFilter::builder()
    .with_default_directive(LevelFilter::INFO.into())
    .with_env_var("RUST_LOG")
    .parse_lossy("r1cs=off");
```
`r1cs=off` direktifi: ZK proof building sırasında constraint system log'unu bastırır. Aksi halde 16 GB+ RAM kullanılabilir.

---

### `pclientd/src/lib.rs` (478 satır)

**Struct:**
```rust
pub struct PclientdConfig {
    pub full_viewing_key: FullViewingKey,
    pub grpc_url: Url,
    pub bind_addr: SocketAddr,
    pub kms_config: Option<soft_kms::Config>,
}
```

**`Command::Init` custody modu (satır ~80–170):**
```rust
// Throwaway Ed25519 anahtar çifti
let (sk, vk) = ed25519_consensus::SigningKey::new(OsRng).into_keypair();
let pvk = PreAuthorizationKey::Ed25519 { verification_key: vk };

// 3 katlı AuthPolicy:
let policy = AuthPolicy::DestinationAllowList {
    allowed_destination_addresses: vec![self_address]  // sadece kendi adresine
}
| AuthPolicy::OnlyIbcRelay  // veya IBC relay
| AuthPolicy::PreAuthorization(Ed25519 {
    required_signatures: 1,
    allowed_signers: [pvk],  // throwaway key
});

let kms_config = soft_kms::Config { spend_key, auth_policy: policy };
```

**`Command::Start` — Servis Başlatma (satır ~175–350):**

11 gRPC proxy servisi + ViewServer + optional CustodyServer:
```rust
let router = Server::builder()
    .accept_http1(true)  // HTTP/1.1 için
    .layer(tonic_web::GrpcWebLayer::new())  // grpc-web support
    .add_service(ViewServiceServer::new(view_service))
    .add_service(CustodyServiceServer::new(custody_service)) // if custody
    .add_service(AppQueryProxy::new(node_channel.clone()))
    .add_service(GovernanceQueryProxy::new(...))
    .add_service(DexQueryProxy::new(...))
    .add_service(DexSimulationProxy::new(...))
    .add_service(FeeQueryProxy::new(...))
    .add_service(SctQueryProxy::new(...))
    .add_service(ShieldedPoolQueryProxy::new(...))
    .add_service(ChainQueryProxy::new(...))
    .add_service(StakeQueryProxy::new(...))
    .add_service(CompactBlockQueryProxy::new(...))
    .add_service(TendermintProxyProxy::new(...))
    .add_service(reflection_service);  // gRPC reflection
```

**`Command::LoadRegistry` (satır ~355–420):**
- `determine_registry_url()`: `chain_id.json` ekle (eğer `.json` ile bitmiyorsa)
- `download_registry_to_temp_file()`: `file://` veya `https://` destekler
- `storage.load_asset_metadata(temp_file)` — asset registry yükle

---

### `pclientd/src/proxy.rs` (272 satır)

**Genel Amaç:** 11 gRPC proxy servisi — her biri aynı pattern.

**Generic `proxy(channel, req)` fonksiyonu:**
```rust
async fn proxy(channel: &mut Channel, req: Request<B>) -> Result<Response<BoxBody>, Infallible> {
    channel.oneshot(req).await
        .map_err(|e| Status::internal(format!("transport error: {}", e)))
        // Error = Infallible (never panics)
}
```

**11 Proxy Struct (hepsi aynı pattern):**

| Struct | gRPC Servis Adı |
|--------|-----------------|
| `AppQueryProxy` | `penumbra.core.app.v1.QueryService` |
| `GovernanceQueryProxy` | `penumbra.core.component.governance.v1.QueryService` |
| `DexQueryProxy` | `penumbra.core.component.dex.v1.QueryService` |
| `DexSimulationProxy` | `penumbra.core.component.dex.v1.SimulationService` |
| `FeeQueryProxy` | `penumbra.core.component.fee.v1.QueryService` |
| `SctQueryProxy` | `penumbra.core.component.sct.v1.QueryService` |
| `ShieldedPoolQueryProxy` | `penumbra.core.component.shielded_pool.v1.QueryService` |
| `ChainQueryProxy` | `penumbra.core.component.chain.v1.QueryService` |
| `StakeQueryProxy` | `penumbra.core.component.stake.v1.QueryService` |
| `CompactBlockQueryProxy` | `penumbra.core.component.compact_block.v1.QueryService` |
| `TendermintProxyProxy` | `penumbra.util.tendermint_proxy.v1.TendermintProxyService` |

Her proxy: `tower::Service<Request<BoxBody>>` implement eder.
`Error = Infallible` — transport hatası `Status::internal`'a dönüştürülür, panic olmaz.

---

## TEST DOSYALARI

### `pd/tests/network_integration.rs`

**`confirm_metrics_emission`** (parametric rstest, #[ignore]):
- Regex ile Prometheus metrik varlığını kontrol eder
- `cnidarium_*`, `pd_*`, `penumbra_stake_*`, `penumbra_dex_*` kategorileri

**`check_cors_headers`** (#[ignore]):
- `access-control-allow-origin: *` zorunlu

**`check_minifront_http_ok`** (#[ignore]):
- GET / → HTTP 200

**`check_grpc_server_reflection`** (#[ignore]):
- `grpcurl list` + `FILE_DESCRIPTOR_SET` üzerindeki tüm servisler görünmeli
- `get_all_grpc_services()`: `DescriptorPool::decode(FILE_DESCRIPTOR_SET)` → servis adları

---

### `pindexer/tests/network_integration.rs`

**Özellik:** `#[cfg(feature = "network-integration")]` — sadece integration test ortamında

**Sabitler:**
- `COMETBFT_DATABASE_URL = "postgresql://penumbra:penumbra@localhost:5432/penumbra_cometbft"`
- `PINDEXER_DATABASE_URL = "postgresql://penumbra:penumbra@localhost:5432/penumbra_pindexer"`

**Test Fonksiyonları:**

`chain_is_progressing()`: height_1 al → 7 saniye bekle → height_2 > height_1 kontrol

`cometbft_indexing_is_working()`: RPC height == DB height (±1 tolerans)

`cometbft_events_are_not_null()` (parametric): 6 NULL kontrol sorgusu — hiçbir event attribute NULL olmamalı

`pindexer_is_working()`: pindexer height ≥ RPC height - 4 (4 blok tolerans)

---

### `pmonitor/tests/network_integration.rs`

**Özellik:** `#[cfg(feature = "network-integration")]`

**5 Test Senaryosu:**

1. `audit_passes_on_compliant_wallets()`: Genesis bakiyeleri → audit OK
2. `audit_passes_on_wallets_that_migrated_once()`: migrate balance → audit OK (FVK güncellendi)
3. `audit_passes_on_wallets_that_migrated_twice()`: İki kez migrate → audit OK
4. `audit_fails_on_misbehaving_wallet_that_sent_funds()`: `pcli tx send 900penumbra` → audit FAIL
5. `audit_passes_on_misbehaving_wallet_that_sent_funds_but_got_them_back()`: Gönder → al geri → audit OK

---

### `pmonitor/tests/testnet.rs`

**Özellik:** `#[cfg(feature = "integration-testnet")]`

`pmonitor_passes_with_empty_wallet_on_testnet()`:
- `NODE_URL = "https://testnet.plinfra.net"`
- Boş wallet oluştur → pmonitor init → pmonitor audit OK
- HTTPS desteği testi

---

### `pmonitor/tests/common/mod.rs` + `pcli_helpers.rs`

**`PmonitorTestRunner`:**
- `/tmp/pmonitor-integration-test/` hardcoded
- `Drop::drop()` → `stop_devnet()` — test sonrası temizlik
- `start_devnet()` → `process-compose` ile pd + cometbft başlat
- `poll_for_ready(process_name)` → 60 saniye timeout, 1 saniye polling
- `generate_genesis_allocations()` → CSV: `1_000_000__000_000,upenumbra,<addr>`

**`pcli_helpers.rs`:**
- `pcli_init_softkms(home)` — `pcli init soft-kms generate` çağırır
- `pcli_view_address(home)` → `pcli view address` çıktısını parse eder
- `pcli_migrate_balance(home, fvk)` → `pcli migrate balance` + FVK stdin

---

### `pclientd/tests/network_integration.rs`

**2 Test (her ikisi de #[ignore]):**

`transaction_send_flow()`:
- pclientd'yi subprocess olarak başlat (kill_on_drop=true)
- View sync bekle
- `TransactionPlannerRequest`: output (1 UM) + IbcRelay (MsgCreateClient stargaze) gönder
- `AuthorizeRequest` → custody imzala
- `WitnessAndBuildRequest` → tx oluştur
- `BroadcastTransactionRequest` → gönder ve onay bekle

`swap_claim_flow()`:
- swap planla (UM → gm)
- `swap_plaintext` kaydet (claim için)
- Swap gönder ve onay bekle
- `swap_by_commitment()` → SwapRecord
- `SwapClaimPlan` oluştur
- Claim gönder ve onay bekle

---

### `pclientd/tests/testnet.rs`

**`pclientd_sync_against_testnet()` (#[ignore]):**
- `NODE_URL = "https://testnet.plinfra.net"`
- View-only config (seed phrase YOK)
- pclientd subprocess başlat (kill_on_drop=true)
- `status_stream` ile sync tamamlanana dek bekle
- HTTPS tüneli üzerinden sync testi

---

## ÖZET TABLO

| Crate | Dosya Sayısı | Temel İşlev | Nova Shield İlgisi |
|-------|-------------|-------------|-------------------|
| pd | 30 | Node daemon, CometBFT ABCI, migration | YÜK — node architecture |
| pindexer | 18 | PostgreSQL event indexer | YÜKSEK — DEX metrics |
| pmonitor | 7 | Wallet compliance monitor | ORTA — audit mekanizması |
| elcuity | 7 | LQT bot (vote/lp/swap) | YÜKSEK — DEX bot örnek |
| pclientd | 5 | Client daemon, gRPC proxy | YÜKSEK — custody daemon |
| Test'ler | 12 | Integration testler | ORTA — test pattern'leri |

---

## NOVA SHIELD CHAIN İÇİN KRİTİK ÇIKTILAR

### 1. Node Architecture
- ABCI TCP + gRPC axum ayrımı: iç bağlantı TCP, dış API HTTP/gRPC
- CORS: `access-control-allow-origin: *` (permissive) — frontend için zorunlu
- Static frontend ZIP embed: `include_bytes!` + runtime ZIP serve
- Prometheus metrics: dedicated endpoint (9000), per-component metrics
- Sleep drift monitor: tokio scheduler health check — production kritik
- r1cs=off log direktifi: ZK işlemlerde 16GB+ memory önlemi

### 2. Migration Framework
- Trait-based: `prepare → migrate_inner → commit_in_place → complete`
- Halt bit mekanizması: `is_chain_halted()` check, `ready_to_start()` clear
- JMT root hash → genesis app_hash: upgrade sonrası state doğrulaması
- UTF-8 güvenli string truncation: `floor_char_boundary` pattern
- CometBFT data temizleme: evidence.db, state.db, blockstore.db sil; tx_index.db koru

### 3. DEX Event Indexing
- 7 pencere candlestick: W1m → W1mo
- `Window::anchor()` snap, `Candle::merge()` birleştirme
- Dual volume: direct_volume vs swap_volume
- LP pozisyon full lifecycle: open → execute (Flows) → close → withdraw
- Aggregate summary: büyük CTE ile cross-pair istatistikler
- `ignore_arb=true` default: arb volume ayrı hesap

### 4. DEX LP Bot
- `spread_to_price_and_fee_bps()`: `midpoint = (buy+sell)/2`, `fee = buy/midpoint - 1`
- Balance limit: staking token'ın %90'ını kullan (%10 fee ayır)
- Descending LP index: complement trick `!a_from_b`
- Position chunk: 5'li batch close/withdraw

### 5. Custody Daemon (pclientd)
- 3 katlı AuthPolicy: DestinationAllowList | OnlyIbcRelay | PreAuthorization(Ed25519)
- Throwaway PAK (Pre-Authorization Key) pattern
- 11 gRPC proxy: `tower::Service + NamedService`, `Error = Infallible`
- `tonic_web::GrpcWebLayer` + `accept_http1(true)` — grpc-web desteği

### 6. Wallet Compliance Monitor
- Genesis bakiyesi scan: parallel `tokio::spawn` trial decryption
- Migration detection: memo regex `"Migrating balance from .+ to (.+)"`
- ALLOWED_DISCREPANCY = 0.1 UM — küçük sapma tolere
- Active wallet: son migration hedefi

### 7. LQT (Liquidity Quality Tournament)
- Epoch-bazlı LP ve delegator ödülleri
- `points` metriği: volume × time weighted
- Idempotent ON CONFLICT akümülatörler
- `block_time_s` konfigürasyonu: ~5.1 saniye default
- Version hash: `blake2b_simd` ile config parametrelerinden türetilen schema versiyonu

---

## DEX UYGULAMASI NOTU

Nova Shield DEX için Penumbra'dan alınacak direkt uygulamalar:

1. **Candlestick Engine**: `Window::anchor()` + `Candle::merge()` — aynen kopyalanabilir
2. **LP Index Sıralama**: Descending order için `!a_from_b` complement trick
3. **Batch Swap Volume Tracking**: direct vs swap volume ayrımı — LP fill vs batch ayrımı için kritik
4. **Position Lifecycle SQL**: `dex_ex_position_state`, `_reserves`, `_executions`, `_withdrawals` — 4 tablo pattern
5. **Spread-to-Fee Formula**: `fee = buy/midpoint - 1` — market-maker optimal fee formula
6. **Aggregate Summary CTE**: eligible_denoms + converted_pairs + sums + largest_vol + top_mover — tek SQL komutu

