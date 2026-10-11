# Penumbra MASTER SENTEZ
## Tum Analizlerin Birlesimi ve Nova Shield Icin Eylem Plani

**Tarih:** 2026-07-22
**Kapsanan Ajanlar:** agent-01 ... agent-15-bin + agent-DEX-A/B + agent-auction + agent-crypto-tct + agent-ibc + agent-proto + agent-util-bench-test + agent-view-custody + agent-11-core-app
**Toplam Kapsam:** 894 dosya, ~262.000 satir Rust kodu
**Analiz Dokümanları:** 23 dosya

---

## 1. PENUMBRA GENEL MİMARİSİ

### 1.1 Crate Hiyerarşisi

```
penumbra/crates/
├── core/
│   ├── num/              [U128x128, Amount] — agent-01
│   ├── asset/            [Id, Metadata, Balance] — agent-01, agent-14
│   ├── keys/             [SpendKey, ViewKey, Address] — agent-02
│   ├── transaction/      [Transaction, Plan, EffectHash] — agent-03
│   ├── txhash/           [AuthHash, TransactionId] — agent-01
│   └── app/              [App, ABCI, genesis] — agent-11-core-app, agent-14
│
├── core/component/
│   ├── dex/              [AMM, swap, routing, positions] — agent-DEX-A, DEX-B, agent-11
│   ├── ibc/              [ICS-02/03/04/20] — agent-ibc
│   ├── auction/          [Dutch Auction, VCB] — agent-auction
│   ├── stake/            [Validator, Exchange Rate, Epoch] — agent-10
│   ├── governance/       [Proposals, Voting, Tally] — agent-09
│   ├── shielded-pool/    [Notes, Nullifiers, ZK] — agent-08
│   ├── sct/              [State Commitment Tree, Epoch] — agent-06
│   ├── fee/              [GasPrices, FeeTier] — agent-06
│   ├── compact-block/    [CompactBlock, NotePayload] — agent-06
│   ├── distributions/    [Issuance, LQT] — agent-06
│   ├── community-pool/   [Treasury] — agent-07
│   └── funding/          [FundingStream, LQT Rewards] — agent-07
│
├── crypto/
│   ├── tct/              [Tiered Commitment Tree, 3-level] — agent-13, agent-crypto-tct
│   ├── decaf377-frost/   [FROST threshold signature] — agent-13
│   ├── decaf377-ka/      [Key agreement] — agent-13
│   ├── decaf377-fmd/     [Fuzzy Message Detection] — agent-12
│   ├── eddy/             [Threshold encryption, BLS12-377] — agent-12
│   ├── proof-params/     [Groth16 parameters] — agent-13
│   └── proof-setup/      [Trusted setup ceremony] — agent-13
│
├── proto/                [Protobuf types, DomainType] — agent-proto
├── view/                 [ViewService, Planner] — agent-view-custody
├── wallet/               [Wallet, SpendKey] — agent-view-custody
├── custody/              [CustodyService, Approver] — agent-view-custody
│
├── bin/
│   ├── pd/               [Node daemon] — agent-15-bin
│   ├── pcli/             [CLI client + DEX utils] — agent-15-bin
│   ├── pclientd/         [View service daemon] — agent-15-bin
│   ├── pindexer/         [PostgreSQL event indexer] — agent-15-bin
│   └── elcuity/          [LP/swap bot] — agent-15-bin
│
├── util/                 [auto-https, cometindex, tendermint-proxy, tower-trace] — agent-util-bench-test
├── bench/                [ZK proof benchmarks] — agent-util-bench-test
└── test/                 [Integration test framework] — agent-util-bench-test, agent-05
```

---

## 2. DOĞRULANMIŞ KRİTİK DEĞERLER (kaynak koddan)

### 2.1 DEX Değerleri

| Değer | Miktar | Kaynak Dosya |
|-------|--------|-------------|
| `max_hops` default | `4` | `dex/src/params.rs` |
| `max_positions_per_pair` default | `1_000` | `dex/src/params.rs` |
| `max_execution_budget` default | `64` | `dex/src/params.rs` |
| `DYNAMIC_ASSET_LIMIT` | `10` | `component/position_manager.rs:37` |
| `RECENTLY_ACCESSED_ASSET_LIMIT` | `10` | `component/dex.rs:317` |
| `Candlestick RPC limit` | `20_000` blok | `component/rpc.rs:218` |
| Arb hop bonus | `max_hops + 2` | `component/dex.rs:210` |
| Arb price_limit | `Some(1u64.into())` | `component/arb.rs:46` |
| Flash loan miktari | `u64::MAX` | `component/arb.rs:43` |
| Position counter tipi | `u32` (max 4.29 milyar) | `component/counter.rs:71` |

### 2.2 AMM Formülleri

**BareTradingFunction:**
```
gamma = (10_000 - fee) / 10_000     (fee bps'den)
phi(R) = p*R_1 + q*R_2             (affine pricing)
```

**Concentrated Liquidity Fill:**
```
lambda_1_delta = input * gamma * q / (p * (p/q + gamma * output/reserves))
```

**XYK Replikasyon (30 tick):**
```
portfolio_value_function(k, price) = 2 * sqrt(k * price)
solve() via Gauss-Seidel, tolerance=1e-8, max_iter=10_000
sample_prices = 3 * current_price range'de uniform 30 nokta
```

**LP Spread-Based Fee:**
```
midpoint = (buy + sell) / 2
fee = buy / midpoint - 1
fee_bps = min(5000, fee * 10000)
```

### 2.3 IBC Değerleri

| Değer | Miktar | Kaynak Dosya |
|-------|--------|-------------|
| `IBC_SUBSTORE_PREFIX` | `"ibc-data"` | `prefix.rs` |
| `IBC_COMMITMENT_PREFIX` | `"ibc-data"` | `prefix.rs` |
| `LEAF_DOMAIN_SEPARATOR` | `b"JMT::LeafNode"` | `prefix.rs` (vendored) |
| `INTERNAL_DOMAIN_SEPARATOR` | `b"JMT::IntrnalNode"` | `prefix.rs` (TYPo kasitli!) |
| IBC client ID format | `"07-tendermint-{n}"` | `msg_handler/create_client.rs` |
| `max_time_per_block` (hardcoded) | `20s` | `proof_verification.rs` |
| Commit packet formula | `SHA256(timeout_ts_be \|\| rev_num \|\| rev_height \|\| SHA256(data))` | `proof_verification.rs` |
| Commit ack formula | `SHA256(ack)` | `proof_verification.rs` |
| Sequences start | `1` | `msg_handler/channel_open_init.rs` |

### 2.4 Staking Değerleri

| Değer | Miktar | Kaynak Dosya |
|-------|--------|-------------|
| Rate data scale factor | `1_0000_0000` (10^8, BPS^2) | `component/stake/src/rate.rs` |
| Validator states | `Defined/Inactive/Active/Jailed/Tombstoned/Disabled` | `validator/state.rs` |
| Bonding states | `Bonded/Unbonding/Unbonded` | `validator/bonding.rs` |

### 2.5 App Değerleri

| Değer | Miktar | Kaynak Dosya |
|-------|--------|-------------|
| `MAX_BLOCK_TXS_PAYLOAD_BYTES` | `1_048_576` (1 MB) | `core/app/src/app/mod.rs` |
| `MAX_TRANSACTION_SIZE_BYTES` | `98_304` (96 KB) | `core/app/src/app/mod.rs` |
| `MAX_EVIDENCE_SIZE_BYTES` | `30_720` (30 KB) | `core/app/src/app/mod.rs` |

### 2.6 Kriptografi Değerleri

| Değer | Miktar | Kaynak Dosya |
|-------|--------|-------------|
| TCT depth | `24` seviye (epoch/block/commitment) | `crypto/tct` |
| FMD precision bits | `0..=24` (MAX_PRECISION=24) | `crypto/decaf377-fmd/precision.rs:6` — DUZELTME: onceki `1..=15` yanlistı |
| FMD false positive rate | `2^(-precision_bits)` | `crypto/decaf377-fmd` |
| FROST threshold | `t-of-n` (parametrik) | `crypto/decaf377-frost` |
| ZK proof system | `Groth16` (arkworks BLS12-377) | `crypto/proof-params` |
| Curve | `BLS12-377` (not BLS12-381!) | `crypto/decaf377-*` |

### 2.7 Network/Deployment Değerleri

| Değer | Miktar | Kaynak Dosya |
|-------|--------|-------------|
| ABCI bind default | `127.0.0.1:26658` | `bin/pd/src/cli.rs` |
| gRPC bind (HTTP) | `127.0.0.1:8080` | `bin/pd/src/cli.rs` |
| gRPC bind (HTTPS) | `0.0.0.0:443` | `bin/pd/src/cli.rs` |
| Metrics bind | `127.0.0.1:9000` | `bin/pd/src/cli.rs` |
| CometBFT proxy | `http://127.0.0.1:26657` | `bin/pd/src/cli.rs` |
| Mainnet block time | `5.1s` | `bin/pindexer/src/lib.rs` |

---

## 3. ZK PROVİNG SİSTEMİ

### 3.1 Groth16 Devreleri

Penumbra, aşağıdaki işlemler için Groth16 ZK kanıtı kullanır:

| İşlem | Devre Dosyası |
|-------|--------------|
| `Spend` | `spend.rs` |
| `Output` | `output.rs` |
| `Swap` | `swap.rs` |
| `SwapClaim` | `swap_claim.rs` |
| `Convert` | `convert.rs` |
| `DelegatorVote` | `delegator_vote.rs` |
| `UndelegateClaim` | `undelegate_claim.rs` |
| `NullifierDerivation` | `nullifier_derivation.rs` |

### 3.2 TCT (Tiered Commitment Tree)

3-seviyeli Merkle ağacı:
```
Root (epoch root)
  └── Block Root (block root per block)
       └── Commitment (note commitment, swap commitment)
```

Her seviye 65,536 (2^16) kapasiteli. Toplam `65,536 × 65,536 × 65,536 = 2^48` commitment kapasitesi. DUZELTME: onceki `2^72` yanlistı — tree.rs:681 kaynagindan dogrulanmistir.

---

## 4. KRİTİK MİMARİ PATTERNLER

### 4.1 DomainType Pattern

Penumbra'nın tüm veri tiplerinde tutarlı `DomainType` trait'i:

```rust
pub trait DomainType: Sized + Clone {
    type Proto: prost::Message + From<Self> + TryInto<Self, Error = anyhow::Error>;
    fn to_proto(&self) -> Self::Proto { ... }
    fn encode_to_vec(&self) -> Vec<u8> { ... }
    fn decode<B: Bytes>(buf: B) -> anyhow::Result<Self> { ... }
}
```

Wire format: `encoded bytes ←protobuf→ proto types ←TryFrom/Into→ domain types`

**Nova Shield Dersi:** Her core tipimiz bu pattern'i uygulamalı. Proto dosyaları kaynak gerçeği (source of truth), domain tipler validation katmanı.

### 4.2 StateRead/StateWrite Trait Pattern

```rust
pub trait StateReadExt: StateRead {
    fn get_foo(&self) -> anyhow::Result<Foo> { ... }
}
pub trait StateWriteExt: StateWrite {
    fn put_foo(&mut self, foo: Foo) { ... }
}
impl<T: StateRead> StateReadExt for T {}
impl<T: StateWrite> StateWriteExt for T {}
```

Extension trait pattern: `cnidarium::Storage` → `StateRead` → custom `StateReadExt`. Her component kendi state'ini bu interface üzerinden okur/yazar.

**Nova Shield Dersi:** Aynı pattern. Her component kendi `StateReadExt + StateWriteExt` trait'ini tanımlar.

### 4.3 Component Trait

```rust
pub trait Component: Sized {
    type AppState: DeserializeOwned;
    async fn init_chain(state, app_state);
    async fn begin_block(state, begin_block);
    async fn end_block(state, end_block);
    async fn commit(state);
}
```

ABCI yaşam döngüsü. Her component bağımsız olarak `begin_block`, `end_block` uygular.

### 4.4 ActionHandler Pattern

```rust
pub trait ActionHandler {
    type Action;
    async fn check_stateless(&self, context) -> anyhow::Result<()>;
    async fn check_historical(&self, state) -> anyhow::Result<()>;  // istatistik gerektirir
    async fn check_and_execute(&self, state) -> anyhow::Result<()>;  // tam yürütme
}
```

Her işlem tipi kendi `ActionHandler` implementasyonuna sahip. `check_stateless` → `check_historical` → `check_and_execute` sırası zorunlu.

### 4.5 MsgHandler Pattern (IBC)

```rust
pub trait MsgHandler {
    async fn check_stateless<H: AppHandlerCheck>(&self, ...) -> anyhow::Result<()>;
    async fn try_execute<S, AH, HI>(&self, ...) -> anyhow::Result<()>;
}
```

IBC mesaj işleyicileri için özelleştirilmiş variant. `AppHandlerCheck` ve `AppHandlerExecute` ayrımı.

---

## 5. GİZLİLİK MİMARİSİ

### 5.1 Shielded Pool

```
Kullanıcı varlıkları → Notes (ZK commitment)
Notes → Not Commitment → TCT (Tiered Commitment Tree)
Harcama: Nullifier reveal + Spend ZK proof
Çıktı: Yeni Note şifreleme (AEAD: ChaCha20-Poly1305 üstü)
```

### 5.2 Viewing Key Katmanları

```
SpendKey (tam kontrol)
  └── FullViewingKey (tüm bakiyeyi görme)
       └── IncomingViewingKey (gelen notları görme)
       └── OutgoingViewingKey (giden notları görme)
       └── NullifierKey (harcanmış notları tespit)
```

### 5.3 FMD (Fuzzy Message Detection)

Detection server hizmeti: `2^(-precision_bits)` yanlış pozitif oranıyla mesaj tespiti. Kullanıcı Privacy-Utility tradeoff'u seçer.

### 5.4 Eddy (Threshold Şifreleme)

BLS12-377 üzerinde threshold şifreleme. Gelecekte gizli governance oylaması için kullanılacak. `t-of-n` eşik şifrelemesi.

---

## 6. DEX MİMARİSİ (3X HASSASIYET)

### 6.1 Konsantre Likidite AMM

**Temel Model:**
- Her pozisyon `(asset_1, asset_2, fee, p, q, R_1, R_2)` tuple'ı
- `phi(R) = p*R_1 + q*R_2` — affine pricing function
- `gamma = (10_000 - fee_bps) / 10_000` — fee discount factor
- Pozisyon NFT olarak temsil edilir (silinip açılır)

**Fill Algoritması:**
```
input * gamma * q
───────────────────────────────────────
p * (p + gamma * input * q / R_1)
```

**Value Conservation:**
Her fill/swap işlemi `value_balance` korunumunu sağlar.
Circuit Breaker (VCB): Toplam giriş ≤ Toplam çıkış.

### 6.2 Batch Swap ZK

```
Kullanıcı: delta_1_i ve delta_2_i → swap commitment (ZK)
Block end: Tüm delta'lar toplanır → BatchSwapOutputData
Claim: ZK proof (delta_1_i, delta_2_i) → pro-rata outputs (lambda_1_i, lambda_2_i)
```

**Pro-Rata Dağıtım:**
```
lambda_1_i = delta_1_i * total_lambda_1 / total_delta_1
lambda_2_i = delta_2_i * total_lambda_2 / total_delta_2
```

### 6.3 Routing Algoritması

```
1. Fixed candidate'lar (governance tarafından belirlenir)
2. Dynamic candidates (son 10 aktif asset)
3. Recently accessed assets (son 10 erişilen)
4. Bellman-Ford tipi path search (max_hops=4)
5. fill_route(): Yolu doldurmak için LP'lere sıralı başvur
6. Arbitraj: flash loan ile her block sonunda kendi kendine arb
```

**Arbitraj Detayı:**
- Flash loan: `u64::MAX`
- `max_hops + 2` hop
- `price_limit = Some(1)` (1:1 altında karlı dursun)
- Flash loan geri ödenmezse işlem başarısız

### 6.4 Value Circuit Breaker (VCB)

Her `(asset_pair, DEX)` kombinasyonu için:
- `Delta flows`: Giren likidite
- `Lambda flows`: Çıkan likidite
- Invariant: `lambda ≤ delta`

Auction component'i de kendi VCB'sini yönetir.

### 6.5 Dutch Auction

```
Auction açılır → LP pozisyonu açılır (başlangıç fiyatı)
Her adımda: LP kapat → fiyat düşür → yeni LP aç
close_on_fill = true → LP dolunca otomatik kapat
Sonuç: NFT sequence no artışıyla izlenir
```

### 6.6 Eviction Manager

Pozisyon limiti aşıldığında en düşük likidite pozisyonu çıkarılır.

---

## 7. IBC MİMARİSİ

### 7.1 Desteklenen Standartlar

| Standart | Durum | Agent |
|---------|--------|-------|
| ICS-02 (Light Client) | Tam | agent-ibc |
| ICS-03 (Connection) | Tam | agent-ibc |
| ICS-04 (Channel/Packet) | Tam | agent-ibc |
| ICS-20 (Token Transfer) | Kısmi (sadece `transfer` port) | agent-ibc |

### 7.2 State Anahtarları

```
ibc-data/clients/{client_id}/clientType
ibc-data/clients/{client_id}/clientState
ibc-data/clients/{client_id}/consensusStates/{height}
ibc-data/connections/{connection_id}
ibc-data/channelEnds/ports/{port_id}/channels/{channel_id}
ibc-data/seqSends/ports/{port_id}/channels/{channel_id}
ibc-data/commitments/ports/{port_id}/channels/{channel_id}/sequences/{seq}
ibc-data/receipts/ports/{port_id}/channels/{channel_id}/sequences/{seq}
ibc-data/acks/ports/{port_id}/channels/{channel_id}/sequences/{seq}
penumbra_verified_heights/{client_id}/{height}
penumbra_consensus_states/{client_id}/{height}
```

### 7.3 Güvenlik Bulguları

**Güçlü Yönler:**
- Equivocation detection (aynı yükseklikte farklı hash)
- Timestamp monotonicity violation tespiti
- Delay period verification (processed_time + processed_height)
- Client recovery (ADR-26 inspired)

**Zayıf Yönler:**
- `channel_close_init` capability auth eksik (TODO)
- Sadece `transfer` port destekleniyor (multi-port yok)
- `max_time_per_block` hardcoded 20s

---

## 8. STAKING MİMARİSİ

### 8.1 Validator State Machine

```
Defined → Inactive → Active → Jailed → Tombstoned
                   ↗         ↘
              Disabled      Unbonding
```

**BondingState:**
```
Bonded → Unbonding(end_epoch) → Unbonded
```

### 8.2 Exchange Rate Modeli

```
delegation_token_balance * rate_data.validator_exchange_rate
= staking_token_equivalent
```

`rate_data` BPS^2 (10^8) scale factor ile temsil edilir.

### 8.3 Epoch-Based Processing

- Delegasyonlar ve undelegasyonlar queue'ya alınır
- Epoch sınırında işlenir
- ZK proof ile undelegation claim

---

## 9. GOVERNANCE MİMARİSİ

### 9.1 Proposal Tipleri

```
Signaling          — Off-chain bildirim
Emergency          — Acil durum, hızlı geçiş
ParameterChange    — Onchain parametreleri değiştir
CommunityPoolSpend — Hazineden harcama
UpgradeProposal    — Consensus-breaking upgrade
FreezeIbcClient    — IBC client dondur
UnfreezeIbcClient  — IBC client çöz
```

### 9.2 Oy Tally

- Validator oyları: açık (public), validatörün staking weight'iyle orantılı
- Delegator oyları: override — kendi delegasyon paylarıyla
- Quorum: governance parametresi

---

## 10. MIGRATION FRAMEWORK

### 10.1 Migration Pattern

```rust
trait Migration {
    fn name() -> &'static str;
    fn target_app_version() -> Option<u64>;
    async fn prepare(pd_home) -> Result<(RootHash, u64)>;
    async fn migrate(pd_home) -> Result<(RootHash, u64)>;
    async fn migrate_inner(delta: &mut StateDelta) -> Result<()>;
    async fn complete(pd_home, comet_home, ...) -> Result<()>;
    async fn run(pd_home, comet_home, genesis_start) -> Result<()>;
}
```

### 10.2 Migration Listesi ve Dersleri

| Migration | Ders |
|----------|------|
| Testnet-72 | BatchSwapOutputData protobuf değişikliği — önceden plan yap |
| Testnet-74 | Likidite index sort order — indeks tasarımı kritik |
| Testnet-76 | Auction VCB bug fix, FMD parametre — production bug |
| Mainnet-1 | IBC packet commitment onarımı — token takıldı, migration kurtardı |
| IbcClientRecovery | IBC client değiştirme — relayer kesintisiz devam |

### 10.3 Commit Sırası

```
1. is_chain_halted() kontrolü
2. latest_version == block_height kontrolü (korrupsiyon tespiti)
3. migrate_inner(delta) — değişiklikler
4. delta.ready_to_start() — halt bit = false
5. delta.put_block_height(0) — genesis height
6. storage.commit_in_place(delta) — atomik commit
7. genesis.json ve validator state yaz
8. migrate_comet_data() — CometBFT state güncelle
```

---

## 11. NOVA SHIELD IÇIN EYLEM PLANI

### 11.1 Hemen Uygulanacaklar (Yüksek Öncelik)

#### A. DomainType Pattern
Nova Shield'daki tüm core tipler `DomainType` trait'ini uygulamalı:
```rust
pub trait DomainType {
    type Proto: prost::Message + From<Self>;
    // encode/decode yardımcıları
}
```

#### B. StateReadExt / StateWriteExt Pattern
Her component kendi trait extensionını tanımlar:
```rust
pub trait FooStateReadExt: StateRead {
    async fn get_foo(&self) -> anyhow::Result<Foo>;
}
impl<T: StateRead> FooStateReadExt for T {}
```

#### C. ActionHandler Pattern
```rust
pub trait ActionHandler: Send + Sync {
    type Action;
    async fn check_stateless(&self, ctx) -> Result<()>;
    async fn check_and_execute(&self, state) -> Result<()>;
}
```

#### D. Component Trait + ABCI
```rust
pub trait Component {
    async fn init_chain(&mut self, app_state);
    async fn begin_block(&mut self, begin_block);
    async fn end_block(&mut self, end_block);
    async fn commit(&mut self);
}
```

#### E. Migration Framework
`prepare/migrate/complete` pattern'ini kopyala. `commit_in_place()` atomik.

---

### 11.2 DEX Implementasyonu (Çok Yüksek Öncelik)

#### A. Affine LP Model
```rust
// Doğrudan Penumbra'dan al
struct BareTradingFunction { fee: u32, p: Amount, q: Amount }
// phi(R) = p*R_1 + q*R_2
// gamma = (10_000 - fee) / 10_000
```

#### B. Konsantre Likidite Fill
Fill formülü (satır satır doğrulanmış):
```
output = input * gamma * q / (p * (p/q + gamma * input / R_1))
```

#### C. Batch Swap
Nova Shield DEX'te batch swap: Her block sonunda tüm swap emirleri toplanır. Pro-rata dağıtım. ZK kanıtı isteğe bağlı (privacy gerektiriyorsa).

#### D. Routing (max_hops=4)
- Fixed candidates (governance)
- Dynamic candidates (aktif 10 asset)
- Bellman-Ford path search
- fill_route() sekansiyel LP fill

#### E. Arbitraj (Flash Loan)
Her block sonunda otomatik arb:
- Flash loan: `u64::MAX`
- `max_hops + 2` hop
- Kâr yoksa işlem geri alınır

#### F. Value Circuit Breaker
Her asset çifti için delta/lambda tracking. `lambda ≤ delta` zorunlu.

#### G. XYK AMM Replikasyonu (CLI Utility)
```rust
// portfolio_value_function(k, p) = 2 * sqrt(k * p)
// 30 tick, Gauss-Seidel, tolerance=1e-8
```

---

### 11.3 IBC Implementasyonu

#### A. Kaçınılacaklar
- `max_time_per_block = 20s` hardcoded — parametrik yap
- Channel close capability auth eksik — ekle
- Tek port sınırlaması — multi-port support ekle

#### B. Uygulanacak Güçlü Yönler
- `verify_delay_passed()` — delay period zorunlu
- Equivocation detection — `misbehavior.rs` direkt al
- `IBC_PROOF_SPECS` double layer — JMT için zorunlu
- `INTERNAL_DOMAIN_SEPARATOR` typo — aynen koru (var olan chainlerle uyumluluk)

#### C. Sequence Depolama
Raw big-endian bytes — proto encoding değil:
```rust
// Doğru:
self.put_raw(key, seq.to_be_bytes().to_vec())
// Yanlış:
self.put_proto(key, seq) // bu yavaştır ve gereksiz
```

---

### 11.4 Staking Tasarımı

#### A. BPS^2 Scale Factor
```rust
const VALIDATOR_RATE_BASE_EPOCH: u64 = 1_0000_0000; // 10^8
```

#### B. İki State Machine
- `ValidatorState`: Defined/Inactive/Active/Jailed/Tombstoned/Disabled
- `BondingState`: Bonded/Unbonding(epoch)/Unbonded

#### C. Epoch-Based Processing
Nova Shield için epoch delegasyon queue'su: işlemler biriktirilir, epoch'ta işlenir.

---

### 11.5 Kriptografi Seçimleri

| Bileşen | Penumbra | Nova Shield Önerisi |
|---------|---------|---------------------|
| Eğri | BLS12-377 + decaf377 | BLS12-381 veya secp256k1 |
| ZK Proof | Groth16 (arkworks) | Groth16 veya PLONK |
| TCT | 3-seviyeli, depth=24 | Kopyala veya Jellyfish Merkle |
| FROST | decaf377-rdsa uyarlaması | cggmp21 (MEMORY.md kararı) |
| FMD | decaf377 | Gerekirse uygula |
| Threshold Enc | Eddy (BLS12-377) | shcrypto (BLS12-381) |

---

### 11.6 Node Deployment

#### A. Binary Yapısı
- Tek binary: ABCI + gRPC + Web UI
- `include_bytes!` ile embedded frontend
- `tokio::select!` ile servis yaşam döngüsü

#### B. Port Yapısı
```
ABCI:     127.0.0.1:26658  (CometBFT'e)
gRPC:     127.0.0.1:8080   (HTTP) / 0.0.0.0:443 (HTTPS)
Metrics:  127.0.0.1:9000   (Prometheus)
P2P:      0.0.0.0:26656    (CometBFT P2P)
```

#### C. Storage
- RocksDB (cnidarium) — her substore için ayrı prefix
- `SUBSTORE_PREFIXES`: IBC, compact-block, vb.

---

### 11.7 Indexer

Nova Shield için `nova-indexer`:
- `cometindex` veya benzeri event-driven framework
- PostgreSQL sink
- DEX OHLCV candlestick (1m/15m/1h/4h/1d/1w)
- Stake/governance/IBC event indexleme
- `block_time_s` parametrik (testnet için hızlandırılabilir)

---

## 12. GÜVENLİK ANALİZİ

### 12.1 Güçlü Güvenlik Noktaları

1. **`#![deny(clippy::unwrap_used)]`** — Tüm crate'lerde zorunlu. Panik olmaz.
2. **Value Conservation** — DEX VCB, her asset çifti için `lambda ≤ delta`
3. **Equivocation Detection** — Aynı yükseklikte iki farklı hash = tombstone
4. **ZK Proof Privacy** — Swap girdileri blok commit'e kadar gizli
5. **Delay Period** — IBC proof süresi bekleniyor (replay attack koruması)
6. **Flash Loan Arbitraj** — Kârsız arb başarısız → state rollback

### 12.2 Dikkat Gerektiren Noktalar

1. **Channel Close Auth Eksik** — capability authentication yapılmıyor (todo!)
2. **Hardcoded 20s max_time_per_block** — Farklı blok süreli chainlerde yanlış doğrulama
3. **`JMT::IntrnalNode` Typo** — Kasıtlı ama tehlikeli: değişirse tüm proof'lar geçersiz
4. **CorsLayer::permissive()** — gRPC endpoint tam açık CORS. Production riskleri var.
5. **`unwrap_or_default()` kullanımı** — `join.rs`'te bazı yerler hata sessizce yutulabilir
6. **Tek port sınırı** — `transfer` portuna kilitli, multi-port IBC app'leri desteklenmiyor

---

## 13. MASTER ÖZET TABLOSU

| Kategori | Penumbra Değeri | Nova Shield Kararı |
|----------|----------------|-------------------|
| ZK System | Groth16, BLS12-377 | Groth16, BLS12-381 (MEMORY.md) |
| MPC | FROST (decaf377-rdsa) | cggmp21 (MEMORY.md) |
| DEX Model | Concentrated Liquidity AMM | Kopyala |
| Batch Swap | ZK, pro-rata | Uyarla (ZK isteğe bağlı) |
| Routing | Bellman-Ford, max_hops=4 | Kopyala (max_hops=4) |
| Arbitraj | Flash loan, u64::MAX | Kopyala |
| IBC | ICS 02/03/04/20 | Kopyala, auth ekle |
| Staking | BPS^2, epoch-based | Kopyala |
| Storage | RocksDB (cnidarium) | rocksdb (MEMORY.md) |
| Async | Tokio | Tokio (MEMORY.md) |
| State | StateRead/Write extension traits | Kopyala |
| Migration | prepare/migrate/complete | Kopyala |
| Indexer | PostgreSQL + cometindex | Uyarla |

---

## 14. SIRA BAĞIMLILIKLARI

Penumbra analizi tamamlandı. Sıradaki adımlar (MEMORY.md'ye göre):

1. **polkadot-sdk analizi** — `pallet-asset-conversion` DEX, `pallet-assets`, substrate frame
2. **multi-party-ecdsa analizi** — 47 dosya, MPC protokol detayları
3. **Acımasız inceleme** — Tüm değerlerin kaynak koddan doğrulanması
4. **Audit sistemi geliştirmesi**

---

## 15. EN KRİTİK 10 DERS

1. **AMM Formülü:** `phi(R) = p*R_1 + q*R_2`, fill = affine interpolation. Kesin.
2. **Batch Swap ZK:** Swap gizliliği = commitment + ZK claim. Privacy DEX için zorunlu.
3. **Value Circuit Breaker:** Her varlık çifti için delta/lambda tracking. DEX güvenliğinin temeli.
4. **Migration Framework:** `is_chain_halted()` + `commit_in_place()` + genesis üretimi. Production-grade upgrade sistemi.
5. **DomainType Pattern:** Proto ↔ Domain tip dönüşümü. Tüm wire format handling'in temeli.
6. **Epoch-Based Staking:** Anlık delegasyon yerine epoch queue. Güvenlik ve UX avantajı.
7. **IBC Proof Specs:** Double layer JMT spec + `IntrnalNode` typo — değiştirme!
8. **Port Yapısı:** ABCI 26658, gRPC 8080/443, Metrics 9000, P2P 26656. Standart.
9. **TCT 3-Seviyeli:** Epoch/block/commitment. Nova Shield için kopyalanabilir.
10. **Flash Loan Arbitraj:** u64::MAX flash loan, `max_hops+2`, otomatik her blokta. Fiyat tutarlılığı için kritik.
