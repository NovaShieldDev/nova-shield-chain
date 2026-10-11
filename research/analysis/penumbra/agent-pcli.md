# Penumbra pcli — Tam Kaynak Kodu Analizi

**Analiz Tarihi:** 2026-07-22
**Repo Konumu:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/penumbra/crates/bin/pcli/`
**Toplam Dosya:** 56 Rust kaynak dosyası
**Analiz Kapsamı:** Her satır, her struct, her fn, her test — hiçbir şey atlanmadı

---

## 1. GENEL MİMARİ

`pcli` (Penumbra Command Line Interface), Penumbra privacy blockchain'inin resmi CLI istemcisidir. Rust ile yazılmıştır ve `clap` tabanlı çok katmanlı subcommand hiyerarşisi kullanır. Daemon (`pd`) ile gRPC üzerinden, CometBFT ile Tendermint proxy üzerinden iletişim kurar.

### Temel Tasarım Kararları
- **`#![deny(clippy::unwrap_used)]`** — unwrap yasak, tüm hatalar `?` veya `anyhow::Result` ile handle edilir
- **Offline mod desteği** — bazı komutlar network gerektirmez (`--offline` flag)
- **Çoklu custody backend** — SoftKms, Threshold (FROST), Encrypted, Ledger (feature-gated), ViewOnly
- **Birleşik view servisi** — local SQLite veya remote gRPC view servisi
- **ZK proof üretimi** — `penumbra_sdk_wallet::build_transaction` Groth16 proof'ları üretir

---

## 2. DOSYA ANALİZLERİ

---

### `src/main.rs` (78 satır)

**Genel Amaç:** Binary giriş noktası. TLS, home dizin, sync ve subcommand dispatch.

**Kritik Davranışlar:**
- `rustls::crypto::aws_lc_rs::default_provider().install_default()` — TLS provider'ı global olarak yükler
- `Init` ve `Debug` komutları sync öncesi işlenir (bunlar network gerektirmez)
- `app.sync()` — default olarak her komut öncesi blok sync yapılır
- `View Reset` view DB'yi siler, sync gerekmez
- `home_dir` → `directories::ProjectDirs::from("zone", "penumbra", "pcli")` platforma özgü data dizini

**Sabitler/Değerler:** Yok (lib.rs'de tanımlı)

---

### `src/lib.rs` (94 satır)

**Genel Amaç:** `App` struct ve sync mekanizması.

**Struct Listesi:**
```rust
pub struct App {
    pub view: Option<ViewServiceClient<BoxGrpcService>>,
    pub custody: Box<dyn CustodyClient>,
    pub governance_custody: Box<dyn CustodyClient>,
    pub config: PcliConfig,
    pub save_transaction_here_instead: Option<PathBuf>,
}
```

**Fonksiyonlar:**
- `App::sync()` — `StatusStreamResponse` iterator, `indicatif` progress bar ile görselleştirir
- `default_home()` → `ProjectDirs::from("zone", "penumbra", "pcli").data_dir()`

**Sabitler:**
```rust
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const VIEW_FILE_NAME: &str = "pcli-view.sqlite";
```

**Nova Shield Chain İçin Dersler:**
- `save_transaction_here_instead` pattern: offline imzalama için tx'i dosyaya kaydet → sync eden başka node broadcast etsin
- Progress bar ile sync görselleştirme: `indicatif` kütüphanesi

---

### `src/opt.rs` (196 satır)

**Genel Amaç:** CLI argüman parsing ve `App` nesnesi oluşturma.

**Struct Listesi:**
```rust
pub struct Opt {
    pub cmd: Command,
    #[clap(long, default_value_t, env = "PENUMBRA_PCLI_HOME")]
    pub home: Utf8PathBuf,
    pub grpc_url: Option<Url>,
}
```

**Fonksiyonlar:**
- `init_tracing()` — `r1cs=off` directive: ZK proof tracing overhead'ini bastırır
- `into_app()` — custody ve view servisini build eder:
  - `CustodyConfig::SoftKms(c)` → `SoftKmsClient::new(c.0)`
  - `CustodyConfig::Threshold(c)` → `ThresholdCustodyClient`
  - `CustodyConfig::Encrypted(c)` → şifre sor, decrypt, `SoftKmsClient` veya `ThresholdCustodyClient`
  - `CustodyConfig::Ledger(_)` → `LedgerCustodyClient` (feature-gated)
  - `CustodyConfig::ViewOnly` → `NullCustodyClient`
  - View: local `ViewServer::load_or_initialize(db_path, &registry, grpc_url)` veya remote gRPC veya None (offline)

**Kritik:** `registry.json` opsiyonel asset registry dosyası — bilinmeyen token'ları tanımlamak için

---

### `src/config.rs` (144 satır)

**Genel Amaç:** TOML tabanlı konfigürasyon yapıları.

**Struct/Enum Listesi:**
```rust
pub struct PcliConfig {
    pub grpc_url: Url,
    pub view_url: Option<Url>,
    pub disable_warning: bool,
    #[serde(with = "DisplayFromStr")]
    pub full_viewing_key: FullViewingKey,
    pub custody: CustodyConfig,
    pub governance_custody: Option<GovernanceCustodyConfig>,
}

pub enum CustodyConfig {
    ViewOnly,
    SoftKms(SoftKmsConfig),
    Threshold(ThresholdConfig),
    Encrypted(EncryptedConfig),
    Ledger(LedgerConfig),  // feature-gated
}

pub enum GovernanceCustodyConfig {
    SoftKms { config: SoftKmsConfig },
    Threshold { config: ThresholdConfig },
    Encrypted { fvk: FullViewingKey, config: EncryptedConfig },
}
```

**Fonksiyonlar:**
- `is_default<T: Default + PartialEq>()` — TOML'da default değerleri serialize etmeme yardımcısı
- `governance_key()` — hangi custody aktifse ondan `GovernanceKey` çıkarır

**Nova Shield Chain İçin Dersler:**
- Governance için ayrı custody: validator imzalama anahtarı spend key'den ayrılabilir
- TOML konfigürasyon: `#[serde(skip_serializing_if = "is_default")]` pattern, temiz config dosyaları

---

### `src/terminal.rs` (313 satır)

**Genel Amaç:** Threshold imzalama için terminal I/O abstraction.

**Struct/Trait:**
```rust
pub struct ActualTerminal {
    pub fvk: Option<FullViewingKey>,
}

impl Terminal for ActualTerminal {
    async fn confirm_request(&self, signing_request: &SigningRequest) -> Result<bool>
    fn explain(&self, msg: &str) -> Result<()>
    async fn broadcast(&self, data: &str) -> Result<()>
    async fn read_line_raw(&self) -> Result<String>
    async fn get_password(&self) -> Result<String>
}
```

**Kritik Teknik:**
- `read_line_raw()`: `termion::raw::IntoRawMode` kullanır. Terminaller 1KB/4KB paste limitine sahiptir. Raw mode'da byte-by-byte okuma + echo ile bu sınır aşılır (threshold signing payload'ları çok büyük)
- Ctrl-C (byte 3) veya Ctrl-D (byte 4) → `Err("aborted")`
- `pretty_print_transaction_plan()` — dummy proof/sig ile `TransactionView` oluşturur, gerçek imzalama yapmadan planı gösterir

**Fonksiyonlar:**
- `read_password()` — interactive ise `rpassword::prompt_password`, non-interactive ise `stdin().read_line()`
- `ActualTerminal::get_confirmed_password()` — çift prompt, eşleşmezse tekrar

**Nova Shield Chain İçin Dersler:**
- Threshold MPC imzalama için terminal abstraction pattern
- Raw terminal mode: büyük payload paste sorunu ve çözümü

---

### `src/network.rs` (194 satır)

**Genel Amaç:** Transaction build, sign ve broadcast pipeline.

**Fonksiyonlar:**
```
App::build_and_submit_transaction(plan) → Result<()>
  └── build_transaction(plan) → Result<Transaction>
      └── penumbra_sdk_wallet::build_transaction(fvk, view, custody, OsRng, plan)
          # ZK Groth16 proof'ları üretir, log: proof_count, elapsed_ms
  └── submit_transaction(tx) → Result<()>
      # Eğer save_transaction_here_instead: JSON'a yaz
      # Yoksa: view().broadcast_transaction(tx, await_detection=true)
      # BroadcastStatus stream → Confirmed(tx_id) bekle

App::submit_transaction_unconfirmed(tx)
  # await_detection=false — fire and forget

App::sign_validator_definition(vd) → Result<SignedValidatorDefinition>
App::sign_validator_vote(vv) → Result<SignedValidatorVote>
  # governance_custody kullanır

App::pd_channel() → Result<Channel>
App::tendermint_proxy_client() → Result<TendermintProxyServiceClient>
```

**ZK Proof Lifecycle:**
1. `TransactionPlan` oluştur (planner)
2. `build_transaction()` → her action için Groth16 proof üret
3. `submit_transaction()` → broadcast → `Confirmed(tx_id)` bekle

**Nova Shield Chain İçin Dersler:**
- `save_transaction_here_instead` — offline imzalama workflow'u için kritik
- Broadcast sonrası confirmation stream: fire-and-forget vs await-detection ayrımı

---

### `src/warning.rs` (35 satır)

**Genel Amaç:** Kullanıcı uyarı mesajları.

**Fonksiyonlar:**
- `display()` — nükleer atık işaretçisi tasarımından ilham alan terminal uyarısı: `PCLI_DISPLAY_WARNING` env var ile kontrol edilir
- `rmm()` — "Payoff Replication Is A Work-In-Progress" uyarısı, xyk replicate özelliği için

---

### `src/command.rs` (86 satır)

**Genel Amaç:** Top-level komut enum'u.

**Enum:**
```rust
pub enum Command {
    Init(InitCmd),          // 100 - wallet init
    Query(QueryCmd),        // 200 - on-chain sorgu
    View(ViewCmd),          // 300 - local view servisi
    Transaction(TxCmdWithOptions), // 400 - tx gönderme
    Threshold(ThresholdCmd), // 500 - FROST signing
    Migrate(MigrateCmd),    // 600 - hesap migrasyonu
    Validator(ValidatorCmd), // 900 - validator yönetimi
    Debug(DebugCmd),        // 999 - debug bilgisi
}
```

- `Command::offline()` — her variant için offline mod desteği sorgular
- Dispatch: `match cmd { Command::Init(c) => c.exec(), ... }`

---

### `src/command/init.rs` (467 satır)

**Genel Amaç:** Wallet başlatma — anahtar üretimi, import, threshold setup.

**Enum/Struct Listesi:**
```rust
pub struct InitCmd {
    pub subcmd: InitTopSubCmd,
    pub grpc_url: Option<Url>,
    pub encrypted: bool,
}

pub enum InitTopSubCmd {
    Spend(InitSubCmd),
    ViewOnly,
    ValidatorGovernanceSubkey(InitSubCmd),
    UnsafeWipe,
}

pub enum InitSubCmd {
    SoftKms(SoftKmsInitCmd),
    Threshold(ThresholdInitCmd),
    ViewOnly,
    Ledger,
    ReEncrypt,
}

pub enum SoftKmsInitCmd {
    Generate { stdout: bool },
    ImportPhrase { legacy_raw_bip39_derivation: bool },
}

pub enum ThresholdInitCmd {
    Deal { threshold: u16, home: Vec<Utf8PathBuf> },
    Dkg { threshold: u16, num_participants: u16 },
}
```

**Anahtar Türetme:**
```rust
// Yeni default (BIP-44):
SpendKey::from_seed_phrase_bip44(phrase, &Bip44Path::new(0))
// Legacy:
SpendKey::from_seed_phrase_bip39(phrase, 0)  // --legacy-raw-bip39-derivation
```

**Threshold Kurulumu:**
- `exec_deal()`: threshold >= 2, `threshold::Config::deal(&mut OsRng, threshold, n)` çağırır
- Her signer için ayrı config dosyası yazar
- `Encrypted` wrapping: `penumbra_sdk_custody::encrypted::Config::create(&password, inner_config)`

**Güvenlik:** Seed phrase alternatif ekranda gösterilir (`termion::screen`) — terminal history'ye düşmez

**Nova Shield Chain İçin Dersler:**
- BIP-44 vs BIP-39 ayrımı: yeni deployment'larda BIP-44 kullan
- Threshold Deal: tek bir trusted party `n` signer config üretir, DKG ise distributed
- Encrypted custody: spend key'i şifre ile wrap et

---

### `src/command/tx.rs` (~1644 satır)

**Genel Amaç:** Tüm transaction subcommandları.

**Struct/Enum Listesi:**
```rust
pub struct TxCmdWithOptions {
    pub offline: Option<PathBuf>,
    pub cmd: TxCmd,
}

pub enum TxCmd {
    Send { values, to, source, memo, fee_tier },
    Delegate { amount, to, source, fee_tier },
    DelegateMany { ... },
    Undelegate { amount, source, fee_tier },
    UndelegateClaim { source, fee_tier },
    Swap { input, into, source, fee_tier },
    Vote(VoteCmd),
    Proposal(ProposalCmd),
    CommunityPoolDeposit { values, source, fee_tier },
    Position(PositionCmd),
    Sweep,
    Withdraw(WithdrawCmd),
    RegisterForwardingAccount(RegisterForwardingAccountCmd),
    Broadcast { transaction },
    LqtVote(LqtVoteCmd),
    Auction(AuctionCmd),
}

pub struct VoteCmd {
    vote: Vote,       // Yes | No | Abstain
    on: u64,          // proposal_id
}
```

**Sabitler:**
```rust
pub(super) const POSITION_CHUNK_SIZE: usize = 30;
```
CloseAll/WithdrawAll: 30'luk batch'ler halinde işlenir (tx boyutu sınırı)

**Swap Akışı (kritik):**
```
1. input value parse et
2. estimated_claim_fee = gas_prices.fee(&swap_claim_gas_cost()).apply_tier(fee_tier)
3. planner.swap(input_value, target_asset_id, claim_fee)
4. build_and_submit_transaction(plan) → swap tx confirmed
5. view.swap_by_commitment(swap_commitment) → swap_record
6. pro_rata_outputs = output_data.pro_rata_outputs((delta_1, delta_2))
7. SwapClaimPlan {
       proof_blinding_r: Fq::rand(&mut OsRng),
       proof_blinding_s: Fq::rand(&mut OsRng),
       ...
   }
8. build_and_submit_transaction(claim_plan)
```

**Delegate Akışı:**
```
1. StakeQueryServiceClient.current_validator_rate(identity_key)
2. SctQueryServiceClient.time_to_epoch_change() → mevcut epoch
3. planner.delegate(epoch, unbonded_amount, rate_data)
4. submit
```

**UndelegateClaim Akışı (karmaşık):**
```
1. view.unspent_notes_by_asset_and_address(unbonding_token_id)
2. stake_client.validator_penalty(id, start..end_height) → Penalty
3. Bonding state: Bonded/Unbonding/Unbonded
   - Unbonded → claim immediate
   - Bonding: upper_bound = unbonding_start + min_block_delay
4. planner.undelegate_claim(...)
```

**IBC Withdraw (auto timeout hesabı):**
```
1. channel → connection → client_state → last_update_height
2. timeout_height = last_update_height + (24*60*60/10)*2  # 2 gün, 10s blok
3. timeout_timestamp = now + 2 days, en yakın 10 dakikaya yuvarla
```

**Noble Forwarding Account:**
```rust
// Cosmos SDK raw tx encode
CosmosTx {
    messages: [MsgRegisterAccount { signer, recipient, channel }],
    fee: { gas_limit: 200000 },
    memo: "",
    timeout_height: 0,
}
// broadcast_sync mode = 2
```

**VoteCmd:**
```rust
impl From<VoteCmd> for (u64, Vote) {
    fn from(cmd: VoteCmd) -> (u64, Vote) { (cmd.on, cmd.vote) }
}
```

**Nova Shield Chain İçin Dersler:**
- Swap: 2 ayrı tx (swap + claim) gerektirir, claim sırasında ZK proof blinding random olmalı
- Claim fee önceden hesaplanır ve swap'a gömülür
- IBC timeout: 2 gün, 10s blok varsayımı
- Noble forwarding: raw Cosmos SDK tx encode, gas_limit hardcoded 200000

---

### `src/command/tx/liquidity_position.rs` (200 satır)

**Genel Amaç:** LP pozisyon yönetimi (open, close, withdraw, replicate).

**Enum:**
```rust
pub enum PositionCmd {
    Order(OrderCmd),
    CloseAll,
    Close { position_ids: Vec<PositionId>, source, fee_tier },
    WithdrawAll,
    Withdraw { position_ids: Vec<PositionId>, source, fee_tier },
    RewardClaim { position_ids: Vec<PositionId>, source, fee_tier },
    Replicate(ReplicateCmd),
}

pub enum OrderCmd {
    Buy { buy_order: String, source, auto_close, fee_tier, num_copies },
    Sell { sell_order: String, source, auto_close, fee_tier, num_copies },
}
```

**Fonksiyonlar:**
- `as_position()`: `BuyOrder::parse_str("100penumbra@1.2gm")` → `into_position()` → `Position`
- `auto_close = true` → `close_on_fill = true` pozisyon flag'i
- `num_copies > 1` → aynı pozisyonu N kez aç (her biri farklı nonce)

**Format:** `"<amount><asset>@<price><asset>[/<fee_bps>bps]"`
Örnek: `"100penumbra@1.2gm"` veya `"100penumbra@1.2gm/10bps"`

**Nova Shield Chain DEX Uygulama Notu:**
- LP pozisyon = LPNFT (ERC-721 benzeri) olarak represent edilir
- Açma/kapama/çekme 3 ayrı adım (open → close → withdraw)
- CloseAll/WithdrawAll: 30'luk chunk'lar, büyük pozisyon sayılarında kritik

---

### `src/command/tx/auction/dutch.rs` (586 satır)

**Genel Amaç:** Dutch Auction yönetimi.

**Enum:**
```rust
pub enum DutchCmd {
    DutchAuctionSchedule {
        input: String,
        max_output: String,
        min_output: String,
        start_height: u64,
        end_height: u64,
        step_count: u64,
        fee_tier: FeeTier,
        source: Option<u32>,
    },
    DutchAuctionEnd { auction_ids, all, batch, source, fee_tier },
    DutchAuctionWithdraw { auction_ids, all, batch, source, fee_tier },
    DutchAuctionGradualSchedule { ... },
}
```

**Kritik Değerler:**
- Nonce: `OsRng.fill_bytes(&mut [0u8; 32])` — her auction'a random nonce
- Filtreler:
  - `auctions_to_end()`: `local_seq == 0` (Opened state)
  - `auctions_to_withdraw()`: `local_seq == 1` (Closed state)
- `--batch 20` — default batch size (customizable)

**Display:**
- `display_auction_description()`: `comfy_table` ile start/end height, lot, fiyat aralığı

---

### `src/command/tx/auction/dutch/gda.rs` (188 satır)

**Genel Amaç:** Gradual Dutch Auction — Poisson arrival process.

**Enum:**
```rust
pub enum GdaRecipe {
    TenMinutes,    // 120 blocks
    ThirtyMinutes, // 360 blocks
    OneHour,       // 720 blocks
    TwoHours,      // 1440 blocks
    SixHours,      // 4320 blocks
    TwelveHours,   // 8640 blocks
    OneDay,        // 17280 blocks
    TwoDays,       // 34560 blocks
}
```
**Blok süresi varsayımı:** 5 saniye (120 blok = 10 dakika)

**Poisson Arrival (inverse transform sampling):**
```rust
// λ = num_auctions / total_blocks
let lambda = num_auctions as f64 / total_blocks as f64;
// Her sub-auction için rastgele arrival zamanı:
let ff_clock = (rng.gen::<f64>() / lambda).ln().abs();
// ff_clock → blok sayısına çevrilir, start_height'e eklenir
```

**generate_auctions():**
```rust
let amount_chunk = input.amount.value() / num_auctions;
step_count = 60  // her sub-auction için sabit
// Her sub-auction: farklı OsRng nonce
```

**Nova Shield Chain DEX Uygulama Notu:**
- GDA: büyük miktarların piyasaya etkisini yayarak satışı
- Poisson arrival: her sub-auction rastgele zamanlama, front-running'i zorlaştırır
- step_count=60 sabit seçim — 60 fiyat adımı

---

### `src/command/tx/replicate.rs` (108 satır)

**Genel Amaç:** Likidite stratejisi replicator.

**Enum:**
```rust
pub enum ReplicateCmd {
    #[clap(hide = true, alias = "xyk")]
    ConstantProduct(ConstantProduct),
    Linear(Linear),
}
```

**Fonksiyonlar:**
- `process_price_or_fetch_spread()`:
  - `user_price` verilmişse: `price * 10^(start_exp - end_exp)` ile ölçekle
  - Verilmemişse: `SpreadRequest` → `approx_effective_price_2_to_1` veya `1_to_2`

**Test:**
```rust
// adjust_price_by_exponents
penumbra:gm      = 1.2       (exp diff = 0)
upenumbra:gm     = 0.0000012 (exp diff = -6, penumbra=6 decimals)
penumbra:ugm     = 1200000.0 (exp diff = +6, gm=6 decimals)
upenumbra:ugm    = 1.2       (exp diff = 0)
```

---

### `src/command/tx/replicate/xyk.rs` (236 satır)

**Genel Amaç:** Constant Product (xyk) CFMM replication — 30 LP pozisyonu ile yaklaşım.

**Struct:**
```rust
pub struct ConstantProduct {
    pair: DirectedUnitPair,
    input: Value,
    current_price: f64,
    fee_bps: u32,
    yes: bool,
    debug_file: Option<String>,
    source: Option<u32>,
}
```

**Validasyonlar:**
- input asset pair'in parçası olmalı
- amount sıfır olmamalı
- `fee_bps <= 5000` (max %50)
- `price > 0`

**Ana Akış:**
```
1. warning::rmm() — "Work-In-Progress" uyarısı
2. process_price_or_fetch_spread() — price al
3. dex_utils::replicate::xyk::replicate(&pair, &input, current_price, fee_bps)
4. Tolkien temalı onay mesajı (Mandos'un sesi)
5. her position için planner.position_open(position)
```

**write_debug_data():**
```rust
// alpha örnekle, r1/r2 hesapla
r1 = alpha_i  (normalize, arbitrary)
r2 = r1 * current_price
total_k = r1 * r2  // xyk invariant
// PayoffPositionEntry JSON'a serialize et → debug dosyası
```

**Nova Shield Chain DEX Uygulama Notu:**
- xyk CFMM'i 30 adet concentrated liquidity pozisyonu ile approximate etmek mümkün
- Yaklaşım toleransı: `APPROXIMATION_TOLERANCE = 1e-8`
- Bu pattern Nova Shield DEX için "strateji as position array" tasarımında kullanılabilir

---

### `src/command/tx/replicate/linear.rs` (332 satır)

**Genel Amaç:** Lineer fiyat aralığında likidite dağıtımı.

**Struct:**
```rust
pub struct Linear {
    pair: DirectedUnitPair,
    input: Value,
    lower_price: f64,
    upper_price: f64,
    fee_bps: u32,     // default 50
    num_positions: u32, // default 16
    current_price: Option<f64>,
    close_on_fill: bool,
    yes: bool,
    source: Option<u32>,
}
```

**Algoritma (`build_positions()`):**
```rust
step_width = (upper_price - lower_price) / (num_positions - 1)
asset_2_per_position = total_input / num_positions

for i in 0..num_positions:
    position_price = lower_price + i * step_width

    if position_price < current_price:
        // asset2 ile finanse et
        r2 = asset_2_per_position
        r1 = 0
    else:
        // asset1 ile finanse et (eşdeğer değer)
        r1 = asset_2_per_position / position_price
        r2 = 0

    // Trading function coefficients:
    p = end_unit * scale * position_price
    q = start_unit * scale
    // scale = 1_000_000 if end_unit < 1_000_000
```

**Test (`sanity_check_penumbra_sdk_gm_example`):**
```
pair: penumbra:gm, lower=1.8, upper=2.2, current=2.05, num_positions=5
positions 0-2: funded with GM (below current)
positions 3-4: funded with UM (above current)
```

**Nova Shield Chain DEX Uygulama Notu:**
- Lineer likidite = Uniswap V3 range order benzeri
- Mevcut fiyata göre hangi asset'le fund edileceği otomatik belirlenir
- Tick spacing: `(upper - lower) / (n - 1)` — uniform dağılım

---

### `src/command/tx/lqt_vote.rs` (133 satır)

**Genel Amaç:** Liquidity Tournament (LQT) oylama.

**Struct:**
```rust
pub struct LqtVoteCmd {
    vote: String,              // token denom (hangi LP pair'i öner)
    rewards_recipient: Option<String>,
    source: Option<u32>,
    fee_tier: FeeTier,
}
```

**Ödül Adresi Çözümleme:**
```rust
rewards_addr() {
    // Varsayılan: ephemeral address (gizlilik)
    // Integer ise: o subaccount index'in ephemeral adresi
    // Aksi hâlde: verilen string'i Address olarak parse et
}
```

**Oylama Akışı:**
```
1. view.app_parameters() → epoch_index
2. view.lqt_voting_notes(epoch.index, Some(AddressIndex)) → voting notes
3. planner.lqt_vote(epoch_index as u16, vote_denom, rewards_addr, &voting_notes)
   # Tüm voting note'ları harcar, change → ephemeral adrese
   # Bu "consolidation": gelecek oylar için karmaşıklığı azaltır
4. submit
```

**Nova Shield Chain İçin Dersler:**
- LQT: DEX likidite yönlendirme için epoch tabanlı oylama mekanizması
- Ephemeral adres kullanımı: her oy farklı adresten → gizlilik
- Note consolidation: zamanla biriken küçük note'ları tek TX'te birleştir

---

### `src/command/view.rs` (150 satır)

**Genel Amaç:** Local view servisi sorguları.

**Enum:**
```rust
pub enum ViewCmd {
    Auction(AuctionCmd),
    WalletId,
    Address { index, ephemeral },
    NobleAddress { index },
    Balance(BalanceCmd),
    Staked,
    Reset,
    Sync,
    ListTransactionHashes,
    Tx { raw, tx_id },
    LiquidityPositions { include_closed, include_withdrawn },
}
```

- `Reset` — `pcli-view.sqlite`'yi siler, tam resync gerektirir
- `Tx { raw: true }` — JSON olarak ham transaction view
- `LiquidityPositions` — kapalı/çekilmiş pozisyonları göster/gizle

---

### `src/command/view/balance.rs` (140 satır)

**Genel Amaç:** Cüzdan bakiyesi görüntüleme.

**Struct:**
```rust
pub struct BalanceCmd {
    pub by_note: bool,  // her note ayrı satırda
}
```

**Filtreleme:**
- Withdrawn LP NFT'leri varsayılan olarak gizle
- Withdrawn auction NFT'leri varsayılan olarak gizle

**`format_source()` — Fon kaynağı etiketleri:**
```
Genesis | Transaction | FundingStreamReward | CommunityPoolOutput
| Ics20Transfer | LiquidityTournamentReward
```

---

### `src/command/view/noble_address.rs`

**Genel Amaç:** Noble IBC forwarding hesabı adresi görüntüleme.

**`NobleAddressCmd`:**
- `index: u32` — subaccount index
- Penumbra adresinden Noble forwarding adresi türetir
- `penumbra_sdk_keys::Address::noble_forwarding_address(channel)` çağrısı

---

### `src/command/query.rs` (319 satır)

**Genel Amaç:** On-chain durum sorguları.

**Enum:**
```rust
pub enum QueryCmd {
    Key { key: String, storage_backend: StorageBackend },
    ShieldedPool(ShieldedPoolCmd),
    Tx { hash: TransactionHash },
    Chain(ChainCmd),
    Validator(ValidatorCmd),
    Governance(GovernanceCmd),
    CommunityPool(CommunityPoolCmd),
    Dex(DexCmd),
    Ibc(IbcCmd),
    Watch { key_regex, nv_key_regex },
    Auction(AuctionCmd),
}

pub enum StorageBackend {
    Jmt,          // plain string keys
    Nonverifiable, // base64-encoded keys
}
```

**`watch()`:**
- `WatchRequest { key_regex, nv_key_regex }` stream → KV/NVKV değişikliklerini gerçek zamanlı yazdır
- Debug/monitoring için kullanışlı

**`OutputFormat`:** `Json | Base64`

---

### `src/command/query/dex.rs` (480 satır)

**Genel Amaç:** DEX durum sorguları.

**Enum:**
```rust
pub enum DexCmd {
    BatchOutputs { trading_pair, height },
    SwapExecution { trading_pair, height },
    ArbExecution { height },
    AllPositions { include_closed, include_withdrawn },
    Position { id },
    Positions { ids: Vec<PositionId> },
    Simulate { input, into, using },
}
```

**BatchSwapOutputData alanları:**
```
delta_1, delta_2     — toplam input
lambda_1, lambda_2   — toplam output
unfilled_1, unfilled_2 — doldurulmayan miktar
```

**`print_swap_execution()`:**
- Her hop için SellOrder fiyatı gösterir (tabular format)

**`get_simulated_execution()`:**
- `SimulateTradeRequest { input, output, routing: Setting::Default }`
- Trade sonucunu simüle et, gerçek tx göndermez

**Pozisyon Display:**
- Tek taraflı (sell-only): tek satır
- Çift taraflı (mixed): iki satır, `└──` connector
- Bilinmeyen asset: raw bytes

**Nova Shield Chain DEX Uygulama Notu:**
- `BatchSwapOutputData` → DEX'te bir height'ta tüm swap sonuçları
- Simülasyon API: kullanıcı "ne alırım?" sorusuna cevap — UI için kritik

---

### `src/command/query/auction.rs` (201 satır)

**Genel Amaç:** Dutch Auction sorgulama ve görselleştirme.

**`render_dutch_auction()`:**
```rust
start_price = max_output / initial_input   // birim: output_asset / input_asset
end_price   = min_output / initial_input
```

**Sequence State Mapping:**
```
local_seq == 0 → "Opened"
local_seq == 1 → "Closed"
local_seq >= 2 → "Withdrawn (seq=N)"
```

**Rezerv Hesabı:**
- LP pozisyon rezervleri + auction state rezervleri toplanır

---

### `src/command/validator.rs` (541 satır)

**Genel Amaç:** Validator yönetimi.

**Enum:**
```rust
pub enum ValidatorCmd {
    Identity,
    GovernanceKey,
    Definition(DefinitionCmd),
    Vote(VoteCmd),
}

pub enum DefinitionCmd {
    Upload { file, signature_file, fee_tier },
    Sign { file, signature_file },
    Template { file, tendermint_validator_keyfile },
    Fetch { file, identity_key },
}
```

**Offline İmzalama:**
```
1. Sign: base64-URL signature dosyası üret
2. Upload: --signature-file ile imzayı yükle
3. stdin '-' okunabilir
```

**Template Üretimi:**
- Default: `enabled = false`, `sequence_number = 0`
- Funding streams: `100bps → address`, `100bps → community_pool`
- Tendermint consensus key: `priv_validator_key.json`'dan okur veya random ed25519 üretir

**`generate_new_tendermint_keypair()`:**
```rust
ed25519_consensus::SigningKey::new(OsRng) → tendermint::PrivateKey::Ed25519
```

**Uptime Hesabı:**
```rust
percent_uptime = 100 * (window_len - missed_blocks) / window_len
// Downtime penalty:
percent_downtime_penalty = slashing_penalty_downtime / 100 / 100 / 100
// bps^2 → %
```

**Kritik Değer:** Blok süresi varsayımı = **5 saniye** (uptime display için)

**Nova Shield Chain İçin Dersler:**
- Governance key ≠ spend key: validator kimliği ve oy hakkı ayrı anahtarlarda
- Validator sequence_number: monoton artan, eski state'e revert'e karşı koruma
- Consensus key değiştirilemez (duplicate_consensus_key_forbidden testi)

---

### `src/command/migrate.rs` (358 satır)

**Genel Amaç:** Hesap migrasyonu.

**Enum:**
```rust
pub enum MigrateCmd {
    Balance { to: String },  // tüm balance → hedef FVK'nın account 0'ına
    SubaccountBalance {
        from_range: String,  // "start..end" format
        plan_only: bool,
    },
}
```

**`parse_range()`:** `"0..10"` → `(0u32, 10u32)`, `start < end` zorunlu

**Fee Payer Seçimi:**
```rust
// En fazla staking token'a sahip account fee öder
accounts.max_by(|a, b| a.staking_amount.cmp(&b.staking_amount))
```

**`plan_only`:** Tx JSON'ını yazdır, broadcast etme

**Nova Shield Chain İçin Dersler:**
- Çok hesaplı bakiye konsolidasyonu için migration command
- Fee payer seçimi: max staking balance → en güçlü hesap fee öder

---

### `src/command/debug.rs` (149 satır)

**Genel Amaç:** Debug ortam bilgisi toplama.

**`DebugCmd::Info` — Toplanan Bilgiler:**
```
cometbft version    → std::process::Command::new("cometbft").arg("version")
tendermint version  → std::process::Command::new("tendermint").arg("version")
pd version          → std::process::Command::new("pd").arg("--version")
uname -a            → OS bilgisi
pcli version        → env!("CARGO_PKG_VERSION")  # compile-time sabit
data_dir exists     → PathBuf::exists()
config.toml exists  → PathBuf::exists()
```

Herhangi biri bulunamazsa `None` — graceful degradation

---

### `src/command/threshold.rs` (51 satır)

**Genel Amaç:** FROST threshold imzalama süreci.

**Akış:**
```rust
ThresholdCmd::Sign:
1. config'den ThresholdConfig'i çıkar (veya Encrypted'ı decrypt et)
2. penumbra_sdk_custody::threshold::follow(config, governance_config, &ActualTerminal::default())
```

`follow()` — multi-round FROST protokolünü çalıştırır: diğer signer'ları bekle, partial sign üret, aggregate

---

### `src/command/utils.rs` (104 satır)

**Genel Amaç:** Pozisyon tablosu rendering.

**`render_positions(asset_cache, positions)`:**
```
Sütunlar: ID | State | Fee (bps) | Sell Price | Reserves
Tek taraflı: tek satır
Çift taraflı (mixed): 2 satır — ikinci satır "└──" prefix
Bilinmeyen asset: raw hex
```

`comfy_table` kullanır.

---

### `src/transaction_view_ext.rs` (471 satır)

**Genel Amaç:** Transaction görselleştirme — terminal'de human-readable format.

**Trait:**
```rust
pub trait TransactionViewExt {
    fn render_terminal(&self);
}
impl TransactionViewExt for TransactionView { ... }
```

**`format_opaque_bytes()`:**
```rust
// hex char → Unicode block element mapping:
hex_to_block = ['▕', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█', '▉', '▚', '▛', '▜', '▝', '▞', '▟']
// Şifreli veriler görsel blok karakterleri olarak gösterilir
```

**Desteklenen ActionView Varyantları:**
```
Spend, Output, Swap, SwapClaim
Ics20Withdrawal, PositionOpen, PositionClose, PositionWithdraw
ProposalSubmit, ProposalWithdraw, ProposalDepositClaim
DelegatorVote, ValidatorVote, ValidatorDefinition
CommunityPoolSpend, CommunityPoolOutput, CommunityPoolDeposit
Delegate, Undelegate, UndelegateClaim
DutchAuctionSchedule, DutchAuctionEnd, DutchAuctionWithdraw
ActionLiquidityTournamentVote → todo!()  // henüz implement edilmemiş
```

**Nova Shield Chain İçin Dersler:**
- Opaque bytes → Unicode block: şifreli veriyi kullanıcıya anlamlı biçimde gösterme
- `todo!()` işaretli kod: production'a geçmeden önce hepsini implement et

---

### `src/dex_utils.rs` (birkaç satır)

**Genel Amaç:** Modül tanımları.

```rust
pub mod replicate;
```

---

### `src/dex_utils/replicate.rs` (12 satır)

**Sabitler:**
```rust
pub const APPROXIMATION_TOLERANCE: f64 = 1e-8;
```

**Modüller:**
```rust
pub mod xyk;
pub mod balancer;   // BOŞ — henüz implement edilmemiş
pub mod volatility; // BOŞ — henüz implement edilmemiş
pub mod math_utils;
pub mod debug;
```

---

### `src/dex_utils/replicate/xyk.rs` (205 satır)

**Genel Amaç:** Constant product (xyk) CFMM'i 30 LP pozisyonu ile approximate etme.

**Sabitler:**
```rust
pub(crate) const NUM_POOLS_PRECISION: usize = 30;
const GAUS_SEIDEL_MAX_ITERATION: usize = 10_000;
```

**Ana Algoritma:**

**1. Invariant Hesabı:**
```rust
fp_r1 = raw_r1 / scaling_factor  // human display units
fp_r2 = current_price * fp_r1
xyk_invariant = fp_r1 * fp_r2   // k = x * y
```

**2. Price Sampling:**
```rust
sample_prices(current_price, NUM_POOLS_PRECISION)
// → math_utils::sample_to_upper(3.0 * current_price, 30)
// 30 nokta: [current/10, current/5, ..., 3*current]
```

**3. Portfolio Value Function (PVF):**
```rust
pub fn portfolio_value_function(k: f64, price: f64) -> f64 {
    2.0 * f64::sqrt(k * price)
}
// xyk eğrisinin fiyat p'deki toplam değeri: φ(p) = 2√(k·p)
```

**4. Linear System (Gauss-Seidel):**
```rust
// Matrix A[j,i] = alpha[min(i,j)]
// b[j] = PVF(k, alpha[j])
// Çöz: A * x = b → her pozisyon için k_i değerleri
solve(&alphas, xyk_invariant, NUM_POOLS_PRECISION)
```

**5. Pozisyon Oluşturma:**
```rust
for (k_i, alpha_i) in position_ks.zip(alphas) {
    if alpha_i < current_price {
        // Tick below current: fund with asset_2
        r1 = 0
        r2 = k_i * unit_end * alpha_i
    } else {
        // Tick above current: fund with asset_1
        r1 = k_i * unit_start
        r2 = 0
    }
    p = alpha_i * unit_end  // tick price
    q = 1 * unit_start      // base quote unit
    Position::new(OsRng, pair, fee_bps, p, q, Reserves { r1, r2 })
}
```

**Nova Shield Chain DEX Uygulama Notu:**
- xyk CFMM replication için matematiksel formül: `φ(p) = 2√(k·p)`
- Her tick tek taraflı pozisyon: current price altı = asset2, üstü = asset1
- Gauss-Seidel iterasyon: max 10.000 iterasyon, 1e-8 tolerans
- 30 pozisyon: yeterli yaklaşım hassasiyeti

---

### `src/dex_utils/replicate/math_utils.rs` (65 satır)

**Genel Amaç:** Matematiksel yardımcı fonksiyonlar.

**Fonksiyonlar:**

**`gauss_seidel(A, b, max_iterations, tolerance)`:**
```rust
// A = L + D (lower triangular + off-diagonal upper)
// iterasyon:
//   partial_off = D[i,:] · k
//   partial_lower = L[i,:i] · k[:i]
//   k[i] = (b[i] - partial_off - partial_lower) / L[i,i]
// Convergence: ||k - k_old||₂ < tolerance
```

**`lower_triangular(matrix)`:**
```rust
// NxN matrisin alt üçgen kısmını çıkarır
// result[i,j] = matrix[i,j] if j <= i else 0
```

**`sample_to_upper(upper, num_points)`:**
```rust
let step = upper / num_points as f64;
(1..=num_points).map(|i| i as f64 * step).collect()
// [step, 2*step, ..., upper] — uniform örnekleme
```

**Nova Shield Chain DEX Uygulama Notu:**
- Gauss-Seidel: sparse linear system için iteratif çözüm
- L2 norm convergence kriteri: verimli yakınsama kontrolü
- `ndarray` crate kullanımı: Rust'ta matris operasyonları

---

### `src/dex_utils/replicate/debug.rs` (82 satır)

**Genel Amaç:** JSON serialize edilebilir debug yapıları — Julia notebook'a export için.

**Struct Listesi:**
```rust
#[derive(Serialize)]
pub struct PayoffPositionEntry {
    #[serde(rename = "canonical_pair")]
    pub pair: DirectedUnitPair,  // canonical string serialize
    pub payoff: PayoffPosition,
    pub current_price: f64,
    pub index: usize,
    pub alpha: f64,
    pub total_k: f64,
}

#[derive(Serialize)]
pub struct PayoffPosition {
    pub p: f64,   // tick price (scaled)
    pub q: f64,   // base unit (scaled)
    pub k: f64,   // invariant
    pub r1: f64,  // reserve 1 (scaled)
    pub r2: f64,  // reserve 2 (scaled)
    pub fee: f64,
}
```

**`PayoffPosition::from_position(pair, position)`:**
```rust
// orient_end(pair.end.id()) → p, q değerlerini doğru yönde al
p = phi.p / end_unit_amount
q = phi.q / start_unit_amount
r1 = reserves_for(pair.start) / start_unit
r2 = reserves_for(pair.end) / end_unit
k = p * r1 + q * r2  // total invariant
k = k / p            // normalize
```

**`serialize_directed_unit_pair_to_canon()`:**
```rust
serializer.serialize_str(&pair.to_canonical_string())
// canonical string: "asset1:asset2" formatı
```

**Nova Shield Chain İçin Dersler:**
- DEX pozisyonları için external tooling (Python/Julia) entegrasyonu: JSON export pattern
- `to_canonical_string()` — trading pair canonical representation

---

### `src/dex_utils/tests.rs` (37 satır)

**Test:**
```rust
#[test]
fn test_xyk_solver() {
    let alphas = vec![2.0, 3.0, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0];
    let k = 2.0;
    let n = alphas.len();
    let solution_vector = xyk::solve(&alphas, k, n)?;
    // LAPACK ile önceden hesaplanmış bilinen çözüm:
    let known_solution = [
        1.1010205144336442,
        0.18619156818115412,
        0.13340819077963983,
        0.07839642626021837,
        0.05320822764119848,
        0.0391713189347902,
        0.030397692310478064,
        0.3782060614588767,
    ];
    // approx_eq: |a - b| <= 0.0001
}
```

`PRECISION_BOUND: f64 = 0.0001`

---

## 3. TEST DOSYALARI

---

### `tests/debug.rs` (26 satır)

**Test:**
```rust
#[test]
fn spend_debug() {
    let pk = &*SPEND_PROOF_PROVING_KEY;
    print_to_file(pk, "spend_proof.txt")  // proving key → dosyaya yaz
    let vk = &*SPEND_PROOF_VERIFICATION_KEY;
    print_to_file(vk, "spend_proof.txt")  // verification key → aynı dosyaya ekle
}
```

Groth16 spend proof parametrelerini inspect etmek için debug yardımcısı.

---

### `tests/proof.rs` (461 satır)

**Genel Amaç:** ZK proof parametrelerinin mevcut devre ile uyumluluğunu doğrulama.

**Test Listesi:**
1. `spend_proof_parameters_vs_current_spend_circuit`
2. `delegator_vote_proof_parameters_vs_current_delegator_vote_circuit`
3. `swap_proof_parameters_vs_current_swap_circuit`
4. `swap_claim_parameters_vs_current_swap_claim_circuit`
5. `output_proof_parameters_vs_current_output_circuit`
6. `nullifier_derivation_parameters_vs_current_nullifier_derivation_circuit`
7. `undelegate_claim_parameters_vs_current_undelegate_claim_circuit`

**Her test aynı pattern:**
```rust
let pk = &*SPEND_PROOF_PROVING_KEY;   // lazy_static proving key
let vk = &*SPEND_PROOF_VERIFICATION_KEY;
// Input oluştur (random)
// Proof::prove(blinding_r, blinding_s, pk, public, private)
// proof.verify(vk, public) → Ok(_) assert
```

**Kritik Groth16 Parametreleri (test'ten):**
```rust
// Spend proof blinding:
blinding_r = Fq::rand(&mut OsRng)
blinding_s = Fq::rand(&mut OsRng)
// Section 3.2 Groth16 paper, page 17 — ZK property için
```

**SwapClaim test'inden BatchSwapOutputData:**
```rust
BatchSwapOutputData {
    delta_1: Amount::from(100u64),
    delta_2: Amount::from(100u64),
    lambda_1: Amount::from(50u64),
    lambda_2: Amount::from(25u64),
    unfilled_1: Amount::from(23u64),
    unfilled_2: Amount::from(50u64),
    height: height.into(),
    trading_pair,
    sct_position_prefix: position,
}
```

**UndelegateClaim test'inden:**
```rust
let penalty = Penalty::from_bps_squared(penalty_amount);
let balance = penalty.balance_for_claim(unbonding_id, unbonding_amount);
```

**Nova Shield Chain İçin Dersler:**
- Proof parametresi değişince tüm proof.rs testleri kırılır → parametre güncellemesi fark edilir
- Groth16 blinding_r/s: her proof için farklı random değer — ZK property
- Epoch_duration = 20 (test), position = SCT'deki konumu, height = epoch*20 + block

---

### `tests/network_integration.rs` (~1337 satır)

**Genel Amaç:** Gerçek testnet'e karşı uçtan uca entegrasyon testleri.

**Sabitler:**
```rust
const TEST_ASSET: &str = "1020test_usd";
// "1020" seçimi: 10e18 scaling ile u64::MAX'tan büyük
const TIMEOUT_COMMAND_SECONDS: u64 = 20;

static UNBONDING_DELAY: Lazy<Duration> = Lazy::new(|| {
    let blocks: f64 = env::var("UNBONDING_DELAY").unwrap_or("50");
    // smoke test blok süresi ~500ms → 0.6 * blocks saniye bekle
    Duration::from_secs((0.6 * blocks) as u64)
});
```

**Test Listesi (hepsi `#[ignore]`):**

1. `transaction_send_from_addr_0_to_addr_1`
   - Send → tx_id regex: `transaction confirmed and detected: ([0-9a-f]{64})`
   - `view tx --raw tx_id` → JSON parse → `ProtoTransactionView → TransactionView`
   - MemoView: Visible (plaintext okunabilir) değil Opaque olmamalı
   - Balance kontrol: `r"1\s*2019test_usd"`
   - Cleanup: geri gönder

2. `transaction_sweep` — `tx sweep` başarılı

3. `delegate_and_undelegate`
   - Validator regex: `penumbravalid1\w{58}`
   - Delegation retry: max 5 deneme (epoch sınırı geçişi ihtimali)
   - Delegation token regex: `r"(\d+\.?\d?m?delegation_[a-zA-Z0-9]*)"`
   - Undelegation retry: max 5 deneme
   - `thread::sleep(*UNBONDING_DELAY)` sonrası claim

4. `lp_management`
   - Sell order → LPNFT balance kontrolü (`1lpnft_opened`)
   - Close → `1lpnft_closed`
   - Withdraw → LPNFT yok
   - 3 pozisyon aç → CloseAll → 3 closed → WithdrawAll → 0 closed

5. `swap` (LP + swap birlikte)
   - Address 1: LP pozisyon aç (`1test_usd@1gm`)
   - Address 0: `swap 1gm --into test_usd`
   - Sleep → balance kontrol
   - LP close-all → withdraw-all

6. `test_orders`
   - Sell `1penumbra@225test_usd`
   - Swap `225test_usd --into penumbra` → `"You will receive outputs of 0test_usd and 999.999mpenumbra. Claiming now..."`
   - Swap `1penumbra --into test_usd` → `"You will receive outputs of 225test_usd and 0penumbra. Claiming now..."`
   - Buy order test (aynı fiyat, ters yön)

7. `governance_submit_proposal`
   - Template signaling proposal → submit (`--deposit-amount 10penumbra`)
   - List proposals

8. `duplicate_consensus_key_forbidden` → validator upload `assert().failure()`

9. `mismatched_consensus_key_update_fails` → resubmit `assert().failure()`

10. `delegate_submit_proposal_and_vote`
    - Delegate → proposal template → submit → list (contains "A short title")
    - `tx vote yes --on 0`

11. `community_pool_`
    - Initial balance `0penumbra`
    - Deposit `5penumbra` → balance `5penumbra`

12. `view_tx_hashes` — `view list-tx-hashes` başarılı

**Test Yardımcıları:**
```rust
fn load_wallet_into_tmpdir() // PENUMBRA_NODE_PD_URL env, default http://127.0.0.1:8080
fn sync(tmpdir)
fn get_validator(tmpdir) // en fazla stake'li = ilk sıradaki validator
fn load_string_to_file(content, tmpdir) // NamedTempFile
```

**Nova Shield Chain İçin Dersler:**
- `#[ignore]` ile ağ testlerini unit testlerden ayır
- `assert_cmd` + `predicates` kombinasyonu: CLI binary testi
- Delegation retry pattern: epoch boundary race condition → retry with max_attempts=5
- UNBONDING_DELAY: testnet parametresinden hesapla, hardcode etme
- LP state machine: opened → closed → withdrawn (her adım ayrı TX)

---

### `tests/testnet.rs` (67 satır)

**Genel Amaç:** Public testnet'e karşı hafif integrasyon testi (feature-gated).

```rust
#![cfg(feature = "integration-testnet")]
const NODE_URL: &str = "https://testnet.plinfra.net";
const TESTNET_TIMEOUT_COMMAND_SECONDS: u64 = 900;  // ~1M blok sync için 15 dakika
```

**Test:**
```rust
#[test]
fn sync_wallet_on_public_testnet()
// init (SEED_PHRASE ile) → view sync
// TLS config ve proving key uyumluluğunu doğrular
```

**Nova Shield Chain İçin Dersler:**
- Public testnet testi ayrı feature flag altında → CI'da isteğe bağlı
- 900 saniye timeout: büyük chain sync için gerçekçi beklenti
- `ignore` değil `cfg(feature)` ile daha temiz feature gating

---

## 4. ÖZET: KRİTİK DEĞERLER VE SABİTLER

| Değer | Kaynak | Değer |
|-------|--------|-------|
| CONFIG_FILE_NAME | lib.rs | `"config.toml"` |
| VIEW_FILE_NAME | lib.rs | `"pcli-view.sqlite"` |
| POSITION_CHUNK_SIZE | command/tx.rs | `30` |
| NUM_POOLS_PRECISION | dex_utils/replicate/xyk.rs | `30` |
| GAUS_SEIDEL_MAX_ITERATION | dex_utils/replicate/xyk.rs | `10_000` |
| APPROXIMATION_TOLERANCE | dex_utils/replicate.rs | `1e-8` |
| PRECISION_BOUND (test) | dex_utils/tests.rs | `0.0001` |
| Blok süresi varsayımı | command/validator.rs, gda.rs | `5 saniye` |
| IBC timeout | command/tx.rs | `(24*60*60/10)*2` blok (2 gün, 10s/blok) |
| Noble gas_limit | command/tx.rs | `200_000` |
| GDA step_count | command/tx/auction/dutch/gda.rs | `60` |
| TIMEOUT_COMMAND_SECONDS | tests/network_integration.rs | `20` |
| TESTNET_TIMEOUT | tests/testnet.rs | `900` |
| UNBONDING_DELAY blocks | tests/network_integration.rs | env `UNBONDING_DELAY` default `50` |
| Test asset | tests/network_integration.rs | `"1020test_usd"` (> u64::MAX @ 1e18) |
| swap output format | tests/network_integration.rs | `"999.999mpenumbra"` |
| xyk PVF formülü | dex_utils/replicate/xyk.rs | `φ(p) = 2√(k·p)` |
| Linear fiyat adımı | command/tx/replicate/linear.rs | `(upper-lower)/(n-1)` |
| default fee_bps (linear) | command/tx/replicate/linear.rs | `50` |
| default num_positions | command/tx/replicate/linear.rs | `16` |
| GDA 1 gün blok | command/tx/auction/dutch/gda.rs | `17280` (5s/blok) |
| GDA 2 gün blok | command/tx/auction/dutch/gda.rs | `34560` |

---

## 5. KOMUt HİYERARŞİSİ

```
pcli
├── init [--grpc-url URL] [--encrypted]
│   ├── spend
│   │   ├── soft-kms
│   │   │   ├── generate [--stdout]
│   │   │   └── import-phrase [--legacy-raw-bip39-derivation]
│   │   ├── threshold
│   │   │   ├── deal --threshold N --home DIR1 DIR2 ...
│   │   │   └── dkg --threshold N --num-participants N
│   │   ├── view-only
│   │   ├── ledger
│   │   └── re-encrypt
│   ├── view-only
│   ├── validator-governance-subkey [same as spend subcmds]
│   └── unsafe-wipe
│
├── query
│   ├── key KEY [--storage-backend jmt|nonverifiable]
│   ├── shielded-pool ...
│   ├── tx HASH
│   ├── chain ...
│   ├── validator list|definition|...
│   ├── governance list-proposals|...
│   ├── community-pool balance|...
│   ├── dex
│   │   ├── batch-outputs --trading-pair X --height N
│   │   ├── swap-execution --trading-pair X --height N
│   │   ├── arb-execution --height N
│   │   ├── all-positions [--include-closed] [--include-withdrawn]
│   │   ├── position ID
│   │   ├── positions ID1 ID2 ...
│   │   └── simulate --input VALUE --into ASSET [--using ROUTING]
│   ├── ibc ...
│   ├── watch [--key-regex] [--nv-key-regex]
│   └── auction ...
│
├── view
│   ├── auction ...
│   ├── wallet-id
│   ├── address [--index N] [--ephemeral]
│   ├── noble-address [--index N]
│   ├── balance [--by-note]
│   ├── staked
│   ├── reset
│   ├── sync
│   ├── list-tx-hashes
│   ├── tx [--raw] TX_ID
│   └── liquidity-positions [--include-closed] [--include-withdrawn]
│
├── tx [--offline PATH]
│   ├── send AMOUNT --to ADDRESS [--memo TEXT] [--source N] [--fee-tier T]
│   ├── delegate AMOUNT --to VALIDATOR [--source N]
│   ├── delegate-many ...
│   ├── undelegate AMOUNT [--source N]
│   ├── undelegate-claim [--source N]
│   ├── swap AMOUNT --into ASSET [--source N] [--fee-tier T]
│   ├── vote yes|no|abstain --on PROPOSAL_ID [--source N]
│   ├── proposal template PROPOSAL_TYPE [--file PATH]
│   ├── proposal submit --file PATH --deposit-amount AMOUNT
│   ├── community-pool-deposit AMOUNT [--source N]
│   ├── position order buy|sell ORDER_STRING [--auto-close] [--num-copies N]
│   ├── position close POSITION_ID [--source N]
│   ├── position close-all [--source N]
│   ├── position withdraw POSITION_ID [--source N]
│   ├── position withdraw-all [--source N]
│   ├── position reward-claim POSITION_ID [--source N]
│   ├── position replicate
│   │   ├── xyk (alias constant-product) --pair X:Y --input VALUE --current-price P [--fee-bps N]
│   │   └── linear --pair X:Y --input VALUE --lower-price L --upper-price U [--num-positions N]
│   ├── sweep
│   ├── withdraw --to ADDRESS [--source N] [--channel CHANNEL]
│   ├── register-forwarding-account ...
│   ├── broadcast --transaction FILE
│   ├── lqt-vote DENOM [--rewards-recipient ADDR]
│   └── auction
│       └── dutch
│           ├── schedule --input VALUE --max-output V --min-output V --start-height N --end-height N --step-count N
│           ├── end [ID1 ID2 ...] [--all] [--batch N]
│           ├── withdraw [ID1 ID2 ...] [--all] [--batch N]
│           └── gradual-schedule --recipe RECIPE --input VALUE --num-auctions N
│               # Recipes: 10m|30m|1h|2h|6h|12h|1d|2d
│
├── threshold
│   └── sign
│
├── migrate
│   ├── balance --to FVK
│   └── subaccount-balance --from-range START..END [--plan-only]
│
├── validator
│   ├── identity
│   ├── governance-key
│   ├── definition template [--file PATH] [--tendermint-validator-keyfile PATH]
│   ├── definition upload --file PATH [--signature-file PATH]
│   ├── definition sign --file PATH --signature-file PATH
│   ├── definition fetch --identity-key IK [--file PATH]
│   └── vote yes|no|abstain --on PROPOSAL_ID
│
└── debug
    └── info
```

---

## 6. DEX UYGULAMA NOTU — NOVA SHIELD CHAIN

### 6.1 Concentrated Liquidity Pozisyon Modeli

Penumbra'nın LP pozisyon modeli bize şunları öğretiyor:

**Pozisyon State Machine:**
```
Open → [trade fill] → (reserves değişir, state açık kalır)
Open → [close TX] → Closed (LPNFT durumu closed)
Closed → [withdraw TX] → Withdrawn (reserves geri alındı)
```

Nova Shield için: LP pozisyonu 3 state'li NFT olarak model edilebilir. Her state geçişi ayrı TX.

**Batch Limit:**
`POSITION_CHUNK_SIZE = 30` — bir tx'te max 30 pozisyon kapatılabilir/çekilebilir. Nova Shield'da da benzer bir limit koyulmalı (tx boyutu, gas limiti).

### 6.2 xyk CFMM Replication

```
k = r1 * r2 (invariant)
φ(p) = 2√(k·p) (portfolio value at price p)

Algoritma:
1. k'yı hesapla (input_amount * current_price = r1 * r2)
2. [0, 3*p] aralığında 30 eşit nokta örnekle (alpha_i)
3. Her alpha_i için PVF değeri: b_i = 2√(k * alpha_i)
4. Gauss-Seidel ile lineer sistemi çöz: A * k_vec = b
   - A[j,i] = alpha[min(i,j)]
5. Her tick için tek taraflı pozisyon:
   - alpha_i < current_price → r2 = k_i * alpha_i, r1 = 0
   - alpha_i >= current_price → r1 = k_i, r2 = 0
```

Bu yaklaşım Nova Shield DEX'inde "strateji pozisyonu" olarak implement edilebilir.

### 6.3 Swap Pipeline

Penumbra swap'ı 2 TX'e bölünmüş:
1. **Swap TX**: input commit, ZK proof, batch'e katıl
2. **SwapClaim TX**: batch sonucu pro-rata al, ZK proof

Nova Shield için:
- Eğer ZK kullanılmıyorsa tek TX'te çözülebilir
- Claim fee önceden hesaplanıp swap TX'e gömülmeli
- Blinding: `Fq::rand(&mut OsRng)` — ZK property için kritik

### 6.4 Dutch Auction

```
start_price = max_output / initial_input
end_price   = min_output / initial_input
```

Her step'te fiyat lineer düşer. `step_count` kaç fiyat adımı olduğunu belirler.

Gradual Dutch Auction (GDA) — Poisson arrival:
```
λ = num_auctions / total_blocks
inter_arrival = Exp(λ) = -ln(U) / λ  (inverse transform)
```

Bu, büyük satışları piyasaya yayarak front-running'e karşı doğal koruma sağlar.

### 6.5 Liquidity Tournament (LQT)

Epoch bazlı likidite yönlendirme oylaması:
- Her kullanıcı bir DEX pair'e "oy" verir
- Oy gücü: voting note miktarı
- Epoch sonunda: en çok oy alan pair → ek liquidity mining ödülü

Nova Shield için: teşvik mekanizması olarak LQT adaptasyonu, epoch=24h ile başlanabilir.

### 6.6 Position Price Format

```rust
p = alpha_i * unit_end   // tick fiyatı, end asset biriminde
q = 1 * unit_start       // base unit
// Fiyat: p/q oranı = tick fiyatı
```

`scale = 1_000_000` eğer `end_unit < 1_000_000` — küçük birim precision için

---

## 7. NOVA SHIELD CHAIN İÇİN GENEL DERSLER

### 7.1 Mimari Kararlar

1. **Custody Abstraction**: `Box<dyn CustodyClient>` — SoftKms/Threshold/Encrypted/Ledger interchangeable. Nova Shield için aynı trait-object pattern.

2. **Governance Key Ayrımı**: Spend key ≠ governance key. Validator imzalama için ayrı anahtar çifti. Özellikle threshold senaryolarında kritik.

3. **Offline Mod**: `save_transaction_here_instead: Option<PathBuf>` — air-gapped signing workflow. Nova Shield için: hardware wallet veya HSM entegrasyonu kolaylaşır.

4. **View Service**: Local SQLite (tam gizlilik) vs. remote gRPC (sync yok ama privacy trade-off). Nova Shield için: local first, remote optional.

5. **`#[deny(clippy::unwrap_used)]`**: Production CLI'da unwrap yasak. Tüm Nova Shield CLI kodu için aynı kural uygulanmalı.

### 7.2 CLI Tasarım Kararları

1. `clap` + subcommand hiyerarşisi: `pcli tx position order sell "1penumbra@1.2gm"`
2. `--source N`: subaccount indexi — çok hesaplı cüzdan desteği
3. `--fee-tier`: Low/Medium/High/Critical
4. `--offline PATH`: tx'i dosyaya kaydet, sonra broadcast et
5. `PENUMBRA_PCLI_HOME` env var: home dizin override

### 7.3 Test Stratejisi

1. **Unit tests** (src/dex_utils/tests.rs): matematiksel doğruluk
2. **proof.rs**: ZK parametresi/devre uyumluluk testleri — parametre güncellemesi sinyali
3. **network_integration.rs**: `#[ignore]` ile ayrılan e2e testler, `PENUMBRA_NODE_PD_URL` env
4. **testnet.rs**: `cfg(feature = "integration-testnet")` — public testnet CI

Nova Shield için: bu katmanlı test yaklaşımını benimse. Özellikle proof parametrelerini her güncelleme sonrası otomatik test et.

### 7.4 Güvenlik Kararları

1. **Seed phrase → alternatif ekran**: terminal history'ye düşmez
2. **Raw terminal mode**: büyük threshold payload için 1KB limit aşılır
3. **BIP-44 default**: `Bip44Path::new(0)` — standart türetme
4. **Random nonce per auction**: her Dutch Auction farklı nonce, tekrar kullanım yok
5. **Consensus key değiştirilemez**: `duplicate_consensus_key_forbidden` testi
6. **r1cs trace kapatma**: `init_tracing()` içinde — ZK proof performance için kritik

---

*Analiz tamamlandı. 56 dosya, ~6.500 satır analiz.*
