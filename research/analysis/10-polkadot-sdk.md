# Repo 10: Polkadot SDK — Kapsamlı Analiz

**Repo:** `research/chains/polkadot-sdk`
**Dil:** Rust
**Lisans:** GPL-3.0-only
**İnceleme Tarihi:** 2026-07-17
**Analist:** Nova Shield Chain Araştırma Ekibi

---

## İçindekiler

1. Genel Mimari ve Monorepo Yapısı
2. GRANDPA — Finality Gadget
3. BABE — Blok Üretim Protokolü
4. NPoS — Nominated Proof of Stake
5. Phragmén Seçim Algoritması
6. Staking Ödül Hesabı
7. Slashing Mekanizması
8. XCM — Cross-Consensus Messaging (v5)
9. XcmExecutor — Mesaj Yürütme
10. Parachain Mimarisi (Cumulus)
11. HRMP/DMP/UMP Mesaj Kanalları
12. FRAME Pallet Sistemi
13. Runtime vs Node Ayrımı
14. Şifreleme ve Kriptografi
15. Nova Shield için Çıkarımlar

---

## 1. Genel Mimari ve Monorepo Yapısı

Polkadot SDK, 2023'te Substrate + Polkadot + Cumulus + Bridges repolarının birleştirilmesiyle oluşturulmuş devasa monorepo'dur.

### Temel Bileşenler

```
polkadot-sdk/
├── substrate/          # Çekirdek blockchain framework (runtime, consensus, network)
│   ├── frame/          # FRAME palet sistemi (staking, grandpa, babe, session, ...)
│   ├── client/         # Off-chain node (consensus client, network, database)
│   └── primitives/     # Paylaşılan tipler (sp-runtime, sp-core, sp-consensus-*)
├── polkadot/           # Relay chain implementasyonu
│   ├── xcm/            # XCM mesajlaşma formatı ve executor
│   └── node/           # Polkadot validator node subsystem'leri
├── cumulus/            # Parachain SDK
│   ├── pallets/        # Parachain paleti (parachain-system, xcm, xcmp-queue)
│   └── client/         # Parachain node bileşenleri
└── bridges/            # Cross-chain köprüler (Snowbridge: Ethereum köprüsü)
```

### Rust Toolchain

```toml
[workspace.package]
edition = "2021"
license = "GPL-3.0-only"
```

- Stable Rust (latest) ile derlenir
- `wasm32v1-none` target: WASM runtime'lar için
- `rust-src` component: PolkaVM target için
- `SKIP_WASM_BUILD=1`: WASM derlenmeden hızlı check

### Mimari İlke

**Runtime/Node ayrımı:**
- **Runtime:** On-chain mantık, WASM'a derlenir, `no_std`, deterministik
- **Node:** Off-chain client, native, `std`, network/disk/time erişimi var

---

## 2. GRANDPA — Finality Gadget

### Dosya: `substrate/client/consensus/grandpa/src/lib.rs`
### Dosya: `substrate/primitives/consensus/grandpa/src/lib.rs`

GRANDPA (GHOST-based Recursive ANcestor Deriving Prefix Agreement) — Polkadot'un finality gadget'ı. BABE ile ayrışık çalışır: BABE blok üretir, GRANDPA finalize eder.

### Temel Tipler

```rust
// Key type: ed25519 (GRANDPA için)
pub const KEY_TYPE: KeyTypeId = sp_application_crypto::key_types::GRANDPA;
pub type AuthorityId = app::Public;           // ed25519 public key
pub type AuthoritySignature = app::Signature; // ed25519 imzası
pub type AuthorityWeight = u64;               // Ağırlık (genelde 1)
pub type SetId = u64;          // Otority seti ID (her değişimde artar)
pub type RoundNumber = u64;    // Tur sayacı

pub type AuthorityList = Vec<(AuthorityId, AuthorityWeight)>;
pub const GRANDPA_ENGINE_ID: ConsensusEngineId = *b"FRNK";
```

### Mesaj Tipleri

```rust
pub type PrimaryPropose<Header> // Birincil öneri (kendi tercih ettiği blok)
pub type Prevote<Header>        // Ön oy (finalize olmaya namzet)
pub type Precommit<Header>      // Kesin oy (finalize edilecek)
pub type CatchUp<Header>        // Geç katılan için gecikmiş oy paketi
pub type Commit<Header>         // n/n'lik precommit paketi (finality kanıtı)
pub type CompactCommit<Header>  // Sıkıştırılmış commit
```

### GrandpaJustification

```rust
pub struct GrandpaJustification<Header: HeaderT> {
    pub round: u64,
    pub commit: Commit<Header>,          // Precommit toplamı
    pub votes_ancestries: Vec<Header>,   // Precommit → finalize köprüsü
}
```

Justification = Blok finalize edildiğinin zincir üzerinde taşınan kanıtı. Warp sync için kritik: authority set değişimlerindeki justification'lar silinmez.

### ScheduledChange

```rust
pub struct ScheduledChange<N> {
    pub next_authorities: AuthorityList,
    pub delay: N,   // Kaç blok sonra uygulanacak
}
```

Authority seti rotasyonu bu yapı ile duyurulur. `delay` blok sayısı kadar beklendikten sonra yeni set devreye girer.

### GRANDPA Protokolü — Nasıl Çalışır

**Her round (tur) iki aşamadan oluşur:**

1. **Prevote aşaması:**
   - Her validator en uzun zincirin en yeni bloğuna prevote atar
   - Toplam ağırlığın 2/3'ü prevote atarsa → `supermajority_prevote(B)` sağlanır

2. **Precommit aşaması:**
   - Validator, prevote supermajority'si olan en yüksek blok B'ye precommit atar
   - Toplam ağırlığın 2/3'ü precommit atarsa → B FİNALİZE edilir

**Güvenlik özelliği:** GRANDPA, farklı zincirlerde aynı blok numarasını finalize edemez. Bu matematiksel imkânsızlık, 2/3'ün iki ayrı öneri için aynı anda majority oluşturamamasından gelir.

### Authority Set Değişimi

```
Blok B'de: ScheduledChange { next_authorities: [...], delay: N }
         ↓
B + N bloğu finalize olur
         ↓
Yeni authority set (SetId++) devreye girer
         ↓
Eski set artık vote yapamaz
```

### GRANDPA Config

```rust
pub struct Config {
    pub gossip_duration: Duration,              // Oy mesajı yayılma süresi
    pub justification_generation_period: u32,   // Kaç blokta bir justification üret
    pub observer_enabled: bool,                 // Full node observer modu
    pub local_role: Role,                       // authority / full / light
    pub keystore: Option<KeystorePtr>,
    pub protocol_name: ProtocolName,
}
```

### Module Yapısı

```
grandpa/src/
├── authorities.rs     # AuthoritySet, SharedAuthoritySet
├── environment.rs     # VoterSetState, round yönetimi
├── communication/     # NetworkBridge, gossip
├── finality_proof.rs  # FinalityProof, warp sync kanıtı
├── import.rs          # GrandpaBlockImport
├── justification.rs   # GrandpaJustification doğrulama
├── notification.rs    # GrandpaJustificationStream
├── observer.rs        # run_grandpa_observer (full node modu)
├── voting_rule.rs     # BeforeBestBlockBy, ThreeQuartersOfTheUnfinalizedChain
└── warp_proof.rs      # Hızlı sync kanıtı
```

---

## 3. BABE — Blok Üretim Protokolü

### Dosya: `substrate/frame/babe/src/lib.rs`

BABE (Blind Assignment for Blockchain Extension) — VRF tabanlı slot ataması. Her slot için validator, gizli bir VRF hesabı yapar; çıktı eşik değerinin altındaysa o slot'ta blok üretebilir.

### Temel Sabitler

```rust
const UNDER_CONSTRUCTION_SEGMENT_LENGTH: u32 = 256; // Segment başına maksimum VRF çıktısı
pub const RANDOMNESS_LENGTH: usize = 32;             // Epoch randomness boyutu (byte)
pub const RANDOMNESS_VRF_CONTEXT: &[u8] = b"substrate-babe-vrf"; // VRF context
pub const BABE_ENGINE_ID: ConsensusEngineId = *b"BABE";
```

### BABE Pallet Config

```rust
pub trait Config: pallet_timestamp::Config {
    type EpochDuration: Get<u64>;          // Epoch uzunluğu (slot sayısı)
    type ExpectedBlockTime: Get<Self::Moment>; // Beklenen ortalama blok süresi
    type EpochChangeTrigger: EpochChangeTrigger; // ExternalTrigger veya SameAuthoritiesForever
    type DisabledValidators: DisabledValidators; // Slash sonrası devre dışı validator
    type MaxAuthorities: Get<u32>;         // Maks validator sayısı
    type MaxNominators: Get<u32>;
    type KeyOwnerProof: Parameter + GetSessionNumber + GetValidatorCount;
    type EquivocationReportSystem: OffenceReportSystem<...>;
    type WeightInfo: WeightInfo;
}
```

### BABE Storage Items

```rust
EpochIndex: StorageValue<_, u64>                    // Mevcut epoch numarası
Authorities: StorageValue<_, WeakBoundedVec<(AuthorityId, BabeAuthorityWeight)>>
GenesisSlot: StorageValue<_, Slot>                  // İlk epoch'un başladığı slot
CurrentSlot: StorageValue<_, Slot>                  // Mevcut slot numarası
Randomness: StorageValue<_, BabeRandomness>         // Mevcut epoch randomness
NextRandomness: StorageValue<_, BabeRandomness>     // Sonraki epoch randomness
NextAuthorities: StorageValue<_, WeakBoundedVec<...>> // Sonraki epoch validator listesi
SegmentIndex: StorageValue<_, u32>                  // VRF segment sayacı
UnderConstruction: StorageMap<u32, BoundedVec<BabeRandomness, SEGMENT_LENGTH>>
Initialized: StorageValue<_, Option<PreDigest>>     // Bu blok için başlatıldı mı?
AuthorVrfRandomness: StorageValue<_, Option<BabeRandomness>>
EpochStart: StorageValue<_, (BlockNumber, BlockNumber)> // Önceki + mevcut epoch başı
Lateness: StorageValue<_, BlockNumber>              // Blok gecikme metrikleri
EpochConfig: StorageValue<_, BabeEpochConfiguration>
SkippedEpochs: StorageValue<_, BoundedVec<(u64, SessionIndex), ConstU32<100>>>
```

### BabeEpochConfiguration

```rust
pub struct BabeEpochConfiguration {
    pub c: (u64, u64),            // Primary slot eşiği: slot_prob = c.0 / c.1
    pub allowed_slots: AllowedSlots, // Primary only / Primary+Secondary / ...
}
```

Polkadot'ta tipik: `c = (1, 4)` → Her 4 slot'tan biri primary slot üretir (yaklaşık)

### EpochChangeTrigger Türleri

```rust
pub struct ExternalTrigger;  // Session pallet tetikler (production'da kullanılan)
pub struct SameAuthoritiesForever; // Test için, aynı validator seti devam eder
```

### VRF Slot Ataması

```
Her slot için validator:
1. VRF::sign(epoch_randomness || slot_number || chain_specific_context) → (output, proof)
2. Eğer hash(output) < threshold → Bu slot'ta blok üretebilir (PRIMARY)
3. Eğer eşiği geçemezse ama secondary modda → Deterministik rotation ile blok üret (SECONDARY)
```

**Güvenlik:** VRF çıktısı önceden tahmin edilemez, işlem sonrası doğrulanabilir.

### Randomness Birikimi

Epoch boyunca her blok üreticisinin VRF çıktısı XOR'lanarak `UnderConstruction` storage'ına eklenir. Epoch sonunda birikimi → sonraki epoch'un `NextRandomness`'ı.

```
Epoch n randomness = XOR(VRF_output_0, VRF_output_1, ..., VRF_output_k)
```

### Equivocation (Double-block)

Aynı slot'ta iki farklı blok imzalamak = BABE equivocation = slash suçu.

```rust
pub struct EquivocationOffence {
    // EquivocationProof + KeyOwnerProof
    // → off-chain worker tespit eder
    // → on-chain rapor edilir
    // → slash uygulanır
}
```

---

## 4. NPoS — Nominated Proof of Stake

### Dosya: `substrate/frame/staking/src/lib.rs`

NPoS = Nominated Proof of Stake. Polkadot'un staking modeli — validator'lar ve nominator'lar birlikte çalışır.

### Roller

| Rol | Tanım |
|-----|-------|
| **Validator** | Blok üretir ve GRANDPA'da oy atar. Bond + seçilme şartı. |
| **Nominator** | Validator'ları destekler, oy verir. Risk paylaşır (ödül + slash). |
| **Idle (Chill)** | Aktif değil, oy kullanmıyor. |

### Temel Terminoloji

- **Stash account:** Stake edilen fonları tutan hesap (cold wallet'ta olabilir)
- **Controller:** Stash adına komut veren hesap (deprecated, artık stash = controller)
- **Era:** Validator setinin yeniden hesaplandığı dönem (yaklaşık 24 saat)
- **Session:** Era'nın alt birimi. Her era birden fazla session içerir.
- **BondingDuration:** Unbond sonrası kilitleme süresi (era sayısı)

### Dispatch Fonksiyonları

```rust
Call::bond(value, payee)          // Stake et
Call::bond_extra(max_additional)  // Ek stake ekle
Call::unbond(value)               // Stake'i kaldır (BondingDuration süre kilitlenir)
Call::withdraw_unbonded(num_slashing_spans) // BondingDuration sonrası çek
Call::validate(prefs)             // Validator ol (ValidatorPrefs: commission)
Call::nominate(targets)           // Validator(lar) için oy ver
Call::chill()                     // Pasife geç
Call::set_payee(payee)            // Ödül hedefini değiştir
Call::payout_stakers(validator_stash, era) // Era ödülü talep et
```

### StakerStatus

```rust
pub enum StakerStatus<AccountId> {
    Idle,
    Validator,
    Nominator(Vec<AccountId>), // Desteklenen validator'lar
}
```

### Exposure — Validator'ın Nominator Listesi

```rust
pub struct Exposure<AccountId, Balance> {
    pub total: Balance,           // Toplam stake (validator + nominatorlar)
    pub own: Balance,             // Validator'ın kendi stake'i
    pub others: Vec<IndividualExposure<AccountId, Balance>>,
}

pub struct IndividualExposure<AccountId, Balance> {
    pub who: AccountId,
    pub value: Balance,
}
```

Exposure paginatelidir: `MaxExposurePageSize` başına nominator sayısı sınırlıdır.

### ValidatorPrefs

```rust
pub struct ValidatorPrefs {
    pub commission: Perbill,  // Ödülden alınan komisyon (%0-%100)
    pub blocked: bool,        // Yeni nominasyonları kabul ediyor mu?
}
```

---

## 5. Phragmén Seçim Algoritması

### Ne Yapar?

Phragmén = nominator oylarını validator'lar arasında ADIL biçimde dağıtır.

### Hedef

1. En yüksek toplam stake'e sahip validator'ları seç
2. Nominator oylarını validator'lar arasında dengeli dağıt
3. Her nominator'ın oyunu birden fazla validator arasında böl

### Adımlar

```
1. Her nominator, desteklediği validator listesiyle birlikte "voter" olur
2. Tur 1: En yüksek stake'li validator seçilir → tüm nominatörlerin payı normalize edilir
3. Tur 2: Sonraki en adil seçim
...
maxValidators (ör. 297) seçilene kadar devam
```

### Normalizasyon

Her nominator vote'u normalize edilir:
- Nominator A: 100 DOT → Validator1'e 60 DOT + Validator2'ye 40 DOT
- Böylece maksimum sayıda validator aktif stake alır

### ElectionProvider

```rust
// Off-chain worker veya on-chain election
pub trait ElectionProvider {
    fn ongoing() -> bool;
    fn elect(remaining_pages: PageIndex) -> Result<BoundedSupportsOf<Self>, Self::Error>;
}
```

Election iki modda çalışır:
1. **Off-chain worker:** Daha karmaşık Phragmén, WASM dışında çalışır
2. **On-chain fallback:** Basitleştirilmiş versiyon, hata durumunda devreye girer

### MaxNominatorsPerValidator

Her validator için maksimum nominator sayısı kısıtlıdır (ör. 512). Bu:
1. Slash hesaplamasını sınırlı tutar
2. Payout maliyetini kontrol eder

---

## 6. Staking Ödül Hesabı

### Era Payout Formülü

```
staker_payout = yearly_inflation(npos_staked / total_supply) × total_supply / eras_per_year
remaining_payout = max_yearly_inflation × total_supply / eras_per_year - staker_payout
```

Bu `PiecewiseLinear` ile eğri tanımlanır:

```rust
use sp_runtime::curve::PiecewiseLinear;
// Tipik: staking_rate ~= %50 → max APY
// %50'nin altında veya üstünde APY düşer
```

**Amaç:** Staking oranını %50 civarında sabitlemek.

### Ödül Dağılımı

```
era_reward = validator_points / total_points × era_total_reward

1. Validator komisyonu alır: commission × era_reward
2. Kalan = era_reward - commission
3. Kalan, stake oranına göre validator + nominatorlara bölünür:
   nominator_cut = (nominator_stake / total_exposure) × remaining
   validator_self_cut = (validator_own / total_exposure) × remaining
```

### Payee Seçenekleri

- `Staked`: Stash'e ekle + compound (stake büyür)
- `Stash`: Stash'e gönder (kompound değil)
- `Account(X)`: Başka hesaba gönder

### HistoryDepth

Ödüller en fazla `HistoryDepth` (ör. 84 era) geriye kadar talep edilebilir. Daha eski → kayıp.

---

## 7. Slashing Mekanizması

### Slash Türleri

| Suç | Ceza | Tetikleyen |
|-----|------|-----------|
| Equivocation (double sign) | Stake × slash_fraction | Otomatik on-chain rapor |
| Downtime (offline) | Küçük (ayarlanabilir) | Session pallet |
| Unresponsiveness | Stake × fraction | Im-Online pallet |

### Slash Uygulama

```rust
// Validator + TÜM nominatörlar slash alır
slash_amount = validator_stake × slash_fraction
for nominator in nominators:
    nominator_slash = nominator.value × slash_fraction
```

### Slash Geciktirme

Slash anında uygulanmaz. `SlashDeferDuration` (era) kadar bekler:
- Governance müdahale edebilir
- False positive koruması

### OffenceReportSystem

```rust
pub trait OffenceReportSystem<Reporter, Evidence> {
    fn publish_evidence(evidence: Evidence) -> Result<(), ()>;
    fn check_evidence(evidence: Evidence) -> Result<(), TransactionValidityError>;
    fn process_evidence(reporter: Reporter, evidence: Evidence) -> Result<(), DispatchError>;
}
```

---

## 8. XCM — Cross-Consensus Messaging (v5)

### Dosya: `polkadot/xcm/src/v5/mod.rs`

XCM = Cross-Consensus Message Format. Farklı konsensüs sistemleri arasında standart mesajlaşma protokolü.

**XCM v5 (en güncel)** — versiyonlar geriye dönük uyumlu.

### Temel Yapı

```rust
pub struct Xcm<Call>(pub Vec<Instruction<Call>>);
```

XCM bir talimat listesidir. XCVM (XCM Virtual Machine) bu talimatları sırayla çalıştırır.

### Kilit Talimatlar (Instructions)

**Varlık Operasyonları:**
```rust
WithdrawAsset(Assets)           // origin'in sahipliğinden çıkar → Holding Register'a
ReserveAssetDeposited(Assets)   // Reserve chain'de tutulduğunun bildirimi → Holding'e
ReceiveTeleportedAsset(Assets)  // Kaynak zincirde yok edildi, burada yarat → Holding'e
DepositAsset { assets, beneficiary }  // Holding'den alıcıya gönder
TransferAsset { assets, beneficiary } // Direkt transfer (Holding bypass)
DepositReserveAsset { ... }    // Holding → reserve hesap + onward XCM
TransferReserveAsset { ... }   // Transfer + reserve + onward XCM
ExchangeAsset { give, want, maximal } // Holding'de takas
```

**Kontrol Talimatları:**
```rust
ClearOrigin                    // Origin register'ı temizle (güvenlik)
DescendOrigin(InteriorLocation) // Origin'i iç lokasyona indir
SetAppendix(Xcm)               // Sonunda çalışacak talimat dizisi
SetErrorHandler(Xcm)           // Hata durumunda çalışacak talimat dizisi
```

**Ücret Yönetimi:**
```rust
BuyExecution { fees, weight_limit }  // Holding'den execution fee öde
RefundSurplus                         // Fazla fee'yi iade al
```

**Sistem Bildirimleri:**
```rust
HrmpNewChannelOpenRequest { sender, max_message_size, max_capacity }
HrmpChannelAccepted { recipient }
HrmpChannelClosing { initiator, sender, recipient }
```

**Query & Response:**
```rust
QueryResponse { query_id, response, max_weight, querier }
ReportError(QueryResponseInfo)
```

**Çalıştırma:**
```rust
Transact { origin_kind, fallback_max_weight, call }  // Runtime call çalıştır
```

### Response Enum

```rust
pub enum Response {
    Null,
    Assets(Assets),
    ExecutionResult(Option<(u32, Error)>),
    Version(super::Version),
    PalletsInfo(BoundedVec<PalletInfo, MaxPalletsInfo>),
    DispatchResult(MaybeErrorCode),
}
```

### Location Sistemi

```rust
// Her şey Location ile tanımlanır
// Relative path: ../ParaId(1000)/AccountId32(...)
pub struct Location { parents: u8, interior: Junctions }

pub enum Junction {
    Parachain(u32),
    AccountId32 { network: Option<NetworkId>, id: [u8; 32] },
    AccountKey20 { network: Option<NetworkId>, key: [u8; 20] }, // EVM adresi
    PalletInstance(u8),
    GeneralIndex(u128),
    GeneralKey { length: u8, data: [u8; 32] },
    OnlyChild,
    Plurality { id: BodyId, part: BodyPart },
    GlobalConsensus(NetworkId),
}

pub enum NetworkId {
    ByGenesis([u8; 32]),  // Genesis hash ile tanımlama
    ByFork { block_number, block_hash },
    Polkadot, Kusama, Ethereum { chain_id }, BitcoinCore, BitcoinCash, ...
}
```

### XcmContext

```rust
pub struct XcmContext {
    pub origin: Option<Location>,   // Origin register
    pub message_id: XcmHash,        // Mesaj kimliği
    pub topic: Option<[u8; 32]>,    // Topic register
}
```

### FeesMode

```rust
pub struct FeesMode {
    pub jit_withdraw: bool,  // true → fee direkt hesaptan çek, false → Holding'den
}
```

### Sabitler

```rust
pub const RECURSION_LIMIT: u8 = 10;    // Nested XCM derinlik limiti
pub const VERSION: Version = 5;
pub const MAX_ITEMS_IN_ASSETS: usize = 20; // Tek mesajdaki max asset sayısı
```

---

## 9. XcmExecutor — Mesaj Yürütme

### Dosya: `polkadot/xcm/xcm-executor/src/lib.rs`

```rust
pub struct XcmExecutor<Config: config::Config> {
    holding: AssetsInHolding,               // Holding Register (geçici varlık deposu)
    holding_limit: usize,
    context: XcmContext,
    original_origin: Location,
    trader: Config::Trader,                 // Ücret hesaplayıcı
    error: Option<(u32, XcmError)>,        // Hata durumu
    total_surplus: Weight,
    total_refunded: Weight,
    error_handler: Xcm<Config::RuntimeCall>, // Hata durumu talimatları
    appendix: Xcm<Config::RuntimeCall>,     // Son talimatlar
    transact_status: MaybeErrorCode,
    fees_mode: FeesMode,
    fees: AssetsInHolding,
    asset_used_in_buy_execution: Option<AssetId>,
    message_weight: Weight,
    _config: PhantomData<Config>,
}
```

### Yürütme Akışı

```rust
impl ExecuteXcm<Call> for XcmExecutor<Config> {
    fn prepare(message: Xcm<Call>, weight_limit: Weight) -> Result<WeighedMessage<Call>> {
        // 1. Weigher ile toplam ağırlığı hesapla
        // 2. WeighedMessage(weight, message) döndür
    }

    fn execute(origin: Location, WeighedMessage(weight, message): ..., id, weight_credit) -> Outcome {
        // 1. Barrier::should_execute kontrolü
        // 2. XcmExecutor::new(origin, id) oluştur
        // 3. While !message.is_empty: vm.process(message)
        //    - Hata: error_handler → appendix
        //    - Başarı: appendix
        // 4. vm.post_process(xcm_weight) → Outcome
    }
}
```

### Barrier

```rust
pub trait ShouldExecute {
    fn should_execute<RuntimeCall>(
        origin: &Location,
        instructions: &mut Vec<Instruction<RuntimeCall>>,
        max_weight: Weight,
        properties: &mut Properties,
    ) -> Result<(), ProcessMessageError>;
}
```

Barrier, mesajı çalıştırmadan önce filtreleme yapar:
- Kaynak güvenilir mi?
- Fee ödendi mi?
- Recursion limitini aştı mı?

### Config Trait — XcmExecutor Yapılandırması

```rust
pub trait Config {
    type RuntimeCall: Dispatchable;
    type XcmSender: SendXcm;              // Mesaj gönderici
    type AssetTransactor: TransactAsset;   // Varlık transferi
    type OriginConverter: ConvertOrigin;   // Location → DispatchOrigin
    type IsReserve: ContainsPair<...>;     // Reserve asset kontrolü
    type IsTeleporter: ContainsPair<...>;  // Teleport güven listesi
    type Barrier: ShouldExecute;           // Erişim kontrolü
    type Weigher: WeightBounds<...>;       // Ağırlık hesaplayıcı
    type Trader: WeightTrader;             // Fee yönetimi
    type FeeManager: FeeManager;
    type AssetExchanger: AssetExchange;
    type XcmRecorder: RecordXcm;
    // ...
}
```

---

## 10. Parachain Mimarisi (Cumulus)

### Dosya: `cumulus/pallets/parachain-system/src/lib.rs`

Cumulus, Substrate tabanlı bir blockzinciri Polkadot parachain'ine dönüştürür.

### parachain-system Paletinin Sorumlulukları

1. **Validation Data İngestion:** Relay chain durum kanıtı alınır
2. **DMP (Downward):** Relay chain → Parachain mesajları alınır ve dispatch edilir
3. **HRMP (Horizontal):** Diğer parachain'lerden gelen mesajlar
4. **Upgrade Koordinasyonu:** Parachain runtime yükseltmesi relay chain'de koordine edilir
5. **Output İletişimi:** Gönderilen mesajlar + upgrade sinyalleri relay chain'e bildirilir

### PersistedValidationData

```rust
// Relay chain'den parachain'e verilen doğrulama bağlamı
pub struct PersistedValidationData {
    pub parent_head: HeadData,           // Parachzin'in bir önceki block header'ı
    pub relay_parent_number: RelayChainBlockNumber, // Relay chain blok numarası
    pub relay_parent_storage_root: Hash, // Relay chain storage root
    pub max_pov_size: u32,              // Maksimum PoV (Proof of Validity) boyutu
}
```

### Mesaj Türleri

```rust
// Downward Message (Relay → Para)
pub struct DownwardMessage { pub msg: Vec<u8> }

// Upward Message (Para → Relay)
pub struct UpwardMessage { pub data: Vec<u8> }

// Outbound HRMP (Para → Para)
pub struct OutboundHrmpMessage<Id> {
    pub recipient: Id,
    pub data: Vec<u8>,
}
```

### Parachain Validation

```rust
// Her parachain bloğu için:
// 1. Collator: Yeni parachain bloğu üretir (PoV = Proof of Validity)
// 2. Relay chain validator: PoV'u WASM runtime ile doğrular
// 3. Onaylanırsa relay chain'de backing alır
// 4. Availability (erasure coding): PoV shred'lere bölünür
// 5. Relay chain'de finalize olunca parachain bloğu da finalize
```

### Channel Yönetimi (HRMP)

```rust
// HRMP kanalı açmak için:
// 1. Parachain A → relay chain: OpenChannel(ParaId_B)
// 2. Relay chain → Parachain B: HrmpNewChannelOpenRequest(A, ...)
// 3. Parachain B kabul eder → HrmpChannelAccepted(A)
// Kanal açılır → Para A ve B HRMP üzerinden mesajlaşabilir
```

---

## 11. HRMP/DMP/UMP Mesaj Kanalları

### Üç Mesaj Yönü

```
                    Relay Chain
                       │ DMP (Downward)
                       ↓
              [Parachain A] ←────────────→ [Parachain B]
                       ↑    HRMP (Horizontal)
                       │ UMP (Upward)
                    Relay Chain
```

| Kanal | Yön | Gecikme |
|-------|-----|---------|
| UMP | Para → Relay | Anında |
| DMP | Relay → Para | 1 relay block |
| HRMP | Para → Para (relay üzerinden) | 1 relay block |
| XCM | Herhangi kombinasyon | Transport bağımlı |

### MessageQueueChain

```rust
// HRMP mesajları MessageQueueChain ile sıralı ve doğrulanabilir
// Hash zinciri: her mesaj → bir sonrakinin hash'i + içerik hash'i
pub struct MessageQueueChain(RelayChainHash);
```

---

## 12. FRAME Pallet Sistemi

### Pallet Yapısı

FRAME makroları ile her pallet şu bileşenlere sahiptir:

```rust
#[frame_support::pallet]
pub mod pallet {
    #[pallet::pallet]
    pub struct Pallet<T>(_);  // Boş struct, T = Config

    #[pallet::config]          // Parametre trait'i
    pub trait Config: frame_system::Config { ... }

    #[pallet::storage]         // On-chain storage
    pub type SomeValue<T> = StorageValue<_, u64, ValueQuery>;

    pub type SomeMap<T: Config> = StorageMap<
        _,
        Blake2_128Concat,      // Hash fonksiyonu (key için)
        T::AccountId,
        Balance,
        ValueQuery,
    >;

    #[pallet::call]            // Dispatch edilebilir işlemler
    impl<T: Config> Pallet<T> {
        #[pallet::weight(T::WeightInfo::do_something())]
        pub fn do_something(origin: OriginFor<T>, value: u64) -> DispatchResult { ... }
    }

    #[pallet::event]           // On-chain olaylar (emit)
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> { SomethingHappened { value: u64 } }

    #[pallet::error]           // Hata türleri
    pub enum Error<T> { SomeError }

    #[pallet::hooks]           // Blok lifecycle hook'ları
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight { ... }
        fn on_finalize(n: BlockNumberFor<T>) { ... }
        fn offchain_worker(n: BlockNumberFor<T>) { ... }
    }
}
```

### Storage Tipleri

```rust
StorageValue<_, T>                    // Tek değer
StorageMap<_, Hasher, Key, Value>     // Key → Value harita
StorageDoubleMap<_, H1, K1, H2, K2, V> // İki anahtarlı harita
StorageNMap<_, Keys, Value>           // N anahtarlı harita

// BoundedVec: sabit uzunluklu vektör (runtime güvenliği için)
BoundedVec<T, MaxLen>

// WeakBoundedVec: runtime panic yerine truncation
WeakBoundedVec<T, MaxLen>
```

### Hasher Türleri

| Hasher | Güvenlik | Kullanım |
|--------|----------|---------|
| `Blake2_128Concat` | Prefix collision-safe | User-controlled key'ler (AccountId) |
| `Twox64Concat` | Daha hızlı, güvenilir origin | Sistem tarafından üretilen key'ler |
| `Identity` | Hash yok | Sabit boyutlu, kontrollü key'ler |

### Runtime Construct

```rust
// construct_runtime! makrosu ile tüm pallet'ler runtime'a eklenir
construct_runtime!(
    pub struct Runtime {
        System: frame_system,
        Timestamp: pallet_timestamp,
        Balances: pallet_balances,
        Staking: pallet_staking,
        Session: pallet_session,
        Grandpa: pallet_grandpa,
        Babe: pallet_babe,
        // ...
    }
);
```

---

## 13. Runtime vs Node Ayrımı

### Runtime (On-chain, WASM)

- `no_std` — standart kütüphane yok
- Deterministik: aynı state + aynı tx → aynı sonuç
- Consensus-critical: tüm node'lar aynı runtime çalıştırır
- Upgrade: `set_code` extrinsic ile runtime WASM değiştirilebilir
- PolkaVM: Polkadot'un yeni RISC-V tabanlı VM'i

### Node (Off-chain, Native)

- `std` — tam Rust std kütüphanesi
- Zamana erişim (Instant), disk I/O, network
- Off-chain worker: network request, heavy computation
- Keystore: validator private key'leri
- P2P: libp2p (Substrate network stack)

### Forkless Upgrade

```
1. Governance: set_code(new_wasm) extrinsic onaylanır
2. Bir sonraki bloktan itibaren yeni WASM runtime devrede
3. Node'ları yeni binary'ye geçirmek için fırsat penceresi var
   (ama zorunlu değil — node yeni runtime'ı otomatik çalıştırır)
```

---

## 14. Şifreleme ve Kriptografi

### Polkadot SDK'da Kullanılan Algoritmalar

| Bileşen | Algoritma | Amaç |
|---------|-----------|------|
| GRANDPA imzaları | Ed25519 | Finality oyları |
| BABE imzaları | Sr25519 (Schnorrkel) | Blok üretimi |
| Account imzaları | Sr25519 / Ed25519 / ECDSA | Kullanıcı işlemleri |
| VRF | Schnorrkel VRF | Slot ataması + randomness |
| Storage hash | Blake2b, Twox128 | Trie hash, depolama key |
| Merkle trie | Blake2b | State root |
| BEEFY | BLS12-381 (ristretto) | Bridge light client kanıtları |

### Sr25519 (Schnorrkel)

Polkadot'a özgü imza şeması:
- Schnorr imzası + Ristretto255 eğrisi
- Ed25519'dan daha güvenli (nonce yeniden kullanım riski düşük)
- VRF entegrasyonu daha temiz

### BEEFY — BLS Light Client

```rust
// BEEFY: Bridge Efficiency Enabling Finality Yielder
// BLS12-381 üzerinde BLS imzaları
// Polkadot → Ethereum bridge için light client kanıtları üretir
// Her epoch sonunda BLS agregat imzası → Ethereum'da tek doğrulama
```

### Multi-Signature Keystore

```rust
// Farklı key type'lar için ayrı keystore
pub const GRANDPA: KeyTypeId = *b"gran"; // Ed25519
pub const BABE: KeyTypeId = *b"babe";   // Sr25519 + VRF
pub const IMON: KeyTypeId = *b"imon";   // Sr25519 (ImOnline)
pub const PARA: KeyTypeId = *b"para";   // Sr25519 (Parachain)
pub const AUDI: KeyTypeId = *b"audi";   // Sr25519 (Authority Discovery)
```

---

## 15. Nova Shield için Çıkarımlar

### A. GRANDPA Mimari Yaklaşımı — BLS Threshold'a Uyarlama

**Polkadot:** GRANDPA'da ed25519 bireysel imzalar, commit'te topluca gönderilir.
**Nova Shield:** BLS threshold imzaları — aynı mantık fakat agregat:

```
GRANDPA: n bireysel Precommit → Commit (n imza)
Nova Shield: n bireysel BLS shard → BLS.Aggregate → tek imza

Avantaj: Commit boyutu O(1) sabit (validator sayısından bağımsız)
```

**Alınan fikir:** Round yapısı: Prevote → Precommit → Commit (2 round BFT, Simplex benzeri)

### B. GRANDPA Authority Set — Nova Shield Epoch Rotation

**Polkadot:** `ScheduledChange { next_authorities, delay }`
**Nova Shield:** Epoch sonu rotation için benzer mekanizma:

```rust
// Nova Shield için:
pub struct EpochChange {
    pub new_validator_set: ValidatorList,
    pub new_dkg_public_key: ThresholdPublicKey,  // Ekstra: MPC key güncelleme
    pub effective_epoch: EpochNumber,
}
```

### C. BABE VRF → Nova Shield Leader Seçimi

**Polkadot:** VRF → slot sahibi → blok üretici

**Nova Shield için:** DPoS + VRF hibrit:
1. Epoch başında stake-weighted sıralama (Phragmén benzeri)
2. Her slot için VRF → "kimse yoksa" fallback leader
3. Slot 400ms → VRF çıktısı threshold kontrolü

```
slot_proof = VRF(epoch_randomness || slot_number, validator_secret_key)
if hash(slot_proof) < validator_weight / total_weight * MAX_HASH:
    → Bu validator bu slot'un lideri
```

### D. NPoS Phragmén → Nova Shield Guardian Seçimi

**Polkadot NPoS:** Nominator oyları → Phragmén → validator seti
**Nova Shield:** MPC Guardian seçimi için:

- Kullanıcı 3/5 guardian seçer → bazıları sistem guardian'ı
- Sistem guardian'larını kim olacağını belirlemek için Phragmén benzeri stake-weight dağılımı
- Her guardian node belirli sayıda kullanıcıya hizmet verir → yük dengeleme

```rust
// Nova Shield Guardian Set Selection (fikir)
pub struct GuardianAllocation {
    pub user: AccountId,
    pub guardians: Vec<GuardianId>,
    pub weights: Vec<f64>,  // Phragmén çıktısı: her guardian'a düşen ağırlık
}
```

### E. Staking Reward Formülü → Nova Shield Validator Ödülü

**Polkadot:**
```
staker_payout = yearly_inflation(staked/total) × total / eras_per_year
commission ilk alınır, kalan stake oranına bölünür
```

**Nova Shield için aynı mantık:**
```
validator_reward = epoch_inflation × (validator_stake / total_stake)
commission = validator_reward × validator_commission_rate
delegator_reward = (validator_reward - commission) × (delegation / total_stake)
```

NSC token ekonomisi:
- Validator & Staking: %25 (250M NSC) — süregelen ödül havuzu
- Epoch sonunda havuzdan epoch_reward çekiliyor

### F. XCM → Nova Shield Cross-Chain Mesajlaşma

**Polkadot XCM:** `Xcm<Call> = Vec<Instruction<Call>>`
**Nova Shield:** Gelecekte cross-chain özellik gerekirse XCM uyumlu olmalıyız:

```rust
// Nova Shield TX, XCM instruction subset'i destekler:
WithdrawAsset, DepositAsset, TransferAsset  // Asset operasyonları
Transact { call }                           // Remote call
BuyExecution                                // Fee ödeme
```

**Öneri:** XCM uyumluluğunu tasarım aşamasında göz önünde bulundur. Polkadot ekosistemiyle interop, MEXC/Gate listesi sonrası değerli olabilir.

### G. FRAME Pallet → Nova Shield Modüler Mimari

**Polkadot FRAME:** Her özellik bağımsız pallet; `construct_runtime!` ile birleştirilir.

**Nova Shield için:** Aynı modüler yaklaşım:
```rust
// Nova Shield modülleri (fikir):
mod nova_mpc;       // MPC/threshold işlemleri
mod nova_vault;     // Şifreli vault
mod nova_auction;   // Sealed-bid batch auction
mod nova_staking;   // Validator stake/reward
mod nova_guardian;  // Guardian yönetimi
mod nova_governance; // DAO governance
```

Her modül bağımsız upgrade edilebilir.

### H. HRMP → Nova Shield Cross-Shard Mesajlaşma

Nova Shield tek zincir olduğundan HRMP direkt uygulanamaz. Ancak HRMP'nin mesaj kuyruk + sıralama mantığı:

```rust
// Nova Shield shard → shard mesajlaşma için:
// MessageQueueChain hash zinciri (her mesaj öncekinin hash'ini içerir)
// → Siparişi garanti altına alır
// → Replay attack önler
```

### I. Slashing Gradyansı — Nova Shield İçin Geliştirilmiş Model

**Polkadot:** Flat slash (tüm validatörler aynı oran)

**Nova Shield için:**
1. **İlk ihlal:** Uyarı + küçük ceza (stake'in %0.1'i)
2. **İkinci ihlal:** Orta ceza (stake'in %1'i)
3. **Üçüncü ihlal:** Büyük ceza (stake'in %10'u)
4. **Koordineli saldırı (n > t validator aynı anda):** Tam slash

Bu, dürüst doğrulama hatalarını kasıtlı saldırılardan ayırır.

### J. BLS Light Client (BEEFY) → Nova Shield Bridge

**Polkadot BEEFY:** BLS12-381 aggregat imzaları → Ethereum light client kanıtı

**Nova Shield'da:** Ethereum bridge için aynı BEEFY yaklaşımı:
- Epoch'ta validator BLS imzaları toplanır
- Aggregat imza Ethereum'a gönderilir
- Ethereum tarafında tek BLS doğrulama (düşük gas)

---

## Özet Tablo

| Bileşen | Polkadot Yaklaşımı | Nova Shield Kararı |
|---------|-------------------|-------------------|
| Finality | GRANDPA (ed25519, 2-round) | Simplex BFT + BLS agregat (Avalanche'tan) |
| Blok üretimi | BABE (VRF) | DPoS + VRF hibrit, 400ms slot |
| Validator seçimi | NPoS (Phragmén) | Phragmén benzeri guardian dağılımı |
| Staking ödülü | PiecewiseLinear inflation, komisyon sonrası split | Aynı formül, NSC tokeni |
| Slashing | Flat % slash | Graduated slash (3 kademe) |
| Mesaj formatı | XCM v5 (Instruction liste) | XCM uyumlu alt-küme |
| Parachain iletişimi | HRMP/DMP/UMP | Tek zincir; MessageQueueChain hash order |
| Pallet sistemi | FRAME (config trait, storage macro) | Modüler bileşen sistemi |
| Runtime upgrade | Forkless (set_code) | Forkless upgrade (Polkadot modelinden) |
| BLS | BEEFY (BLS12-381) | BLS12-381 aggregat, threshold imza |
| İmza (kullanıcı) | Sr25519 / Ed25519 / ECDSA | Dilithium3 (post-quantum) |
| Randomness | VRF + epoch XOR birikimi | VRF + BLS commit-reveal |
| Light client | BEEFY → Ethereum | Nova → EVM zincirleri BEEFY kanıtı |

---

## Nova Shield Teknik Kararlarına Son Katkılar

Bu analiz sonrasında **netleşen** kararlar:

1. **Forkless Upgrade:** Runtime kodu WASM'a derlenir, governance ile değiştirilebilir. İlk günden tasarla.

2. **VRF Randomness Birikimi:** BABE'nin epoch XOR modeli → Nova Shield beacon chain randomness için kullanılabilir (MPC guardian seçimi + leader seçimi).

3. **Graduated Slashing:** Polkadot'un flat slash modelinden daha iyi bir model. 3 kademe: uyarı → orta → tam.

4. **XCM Uyumluluğu:** Kritik instructions'ı şimdiden belirt (Withdraw/Deposit/Transact). Ekosistem uyumu için değerli.

5. **Komisyon + Stake-weight Ödül:** Staking palet mantığı aynı, Nova Shield validator komisyon + nominator/delegatör ödülü için direkt alınabilir.

6. **MessageQueueChain:** HRMP'nin hash zinciri güvenliği, Nova Shield event sıralama garantisi için kullanılabilir.

---

*10 reponun tümü analiz edildi. Sonraki: GELISTIRME-PLANI.md hazırlanacak.*
