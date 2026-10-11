# Penumbra Agent-14: core/app ve core/asset

**Kapsam:**
- `crates/core/app/` — Ana uygulama katmani, ABCI, genesis, parametreler
- `crates/core/asset/` — Varlik kimligi, Denge, Deger Taahhudleri

**Toplam dosya:** ~55 (app: ~20, asset: ~12)
**Satir:** ~3500
**Tarih:** 2026-07-22

---

## 1. core/app — Ana Uygulama Katmani

### 1.1 App Yapisi ve Sabitleri

**Dosya:** `src/app/mod.rs`

```rust
pub struct App {
    state: InterBlockState,  // Arc<StateDelta<Snapshot>>
}

// Boyut sabitleri:
pub const MAX_BLOCK_TXS_PAYLOAD_BYTES: usize = 1024 * 1024;  // 1 MB
pub const MAX_TRANSACTION_SIZE_BYTES: usize = 96 * 1024;     // 96 KB
pub const MAX_EVIDENCE_SIZE_BYTES: usize = 30 * 1024;        // 30 KB
```

**Mimari:** `App`, tum Penumbra bilesenlerini (Component trait'i implemente eden) bir arada tutar ve ABCI arabirimini (CometBFT/Tendermint) saglar.

### 1.2 Bilesenlerin Listesi

`init_chain` cagrisina gore tam liste:

```
Sct (State Commitment Tree)
ShieldedPool
Distributions (token dagilimi)
Staking
Ibc (IBC relayer)
Auction (Dutch auction)
Dex (DEX)
CommunityPool
Governance
FeeComponent
Funding
```

**Baslangic sirasi onemli:** Her bilesene `init_chain` cagrisi sirali yapilar — bazi bilesenlerin diger bilesenlerden veri gerektirdigi durumlarda (orn. Staking, ShieldedPool icerigini de aliyor).

### 1.3 ABCI Akisi

```
prepare_proposal(proposal: request::PrepareProposal):
  1. chain halted? → process::exit(0)
  2. Her aday TX icin:
     a. tx.len() > MAX_TRANSACTION_SIZE_BYTES → atla
     b. total > max_proposal_size_bytes → dur
     c. deliver_tx_bytes(tx).is_err() → atla
  3. Kabul edilenleri dondur

process_proposal(proposal):
  Her TX icin deliver_tx_bytes() cagir
  Bir hata → Reject

begin_block(begin_block: request::BeginBlock):
  Tum bilesenler icin Component::begin_block() cagir

end_block(end_block: request::EndBlock):
  Tum bilesenler icin Component::end_block() cagir
  validator_updates al

commit():
  inter_block_state.apply() → storage.commit() → yeni Snapshot

deliver_tx(tx: Transaction):
  AppActionHandler::check_stateless() → check_historical() → check_and_execute()  [DUZELTME: onceki check_stateful/execute yanlistı — kaynak: action_handler.rs:26-32]
```

**InterBlockState Pattern:**
```rust
type InterBlockState = Arc<StateDelta<Snapshot>>;

fn apply(&mut self, state_tx: StateDelta<InterBlockState>) -> Vec<Event> {
    let (state2, mut cache) = state_tx.flatten();
    std::mem::drop(state2);  // Dogrudan referans serbest birak
    // Arc::get_mut() ile exclusive access al
    cache.apply_to(Arc::get_mut(&mut self.state).expect("..."));
    events
}
```

**cnidarium:** Penumbra'nin ozgun JMT (Jellyfish Merkle Tree) tabanli depolama katmani. `StateDelta<S>` — anlık durum farkini temsil eder, `flatten()` ile gecistir.

### 1.4 Genesis Durumu

**Dosya:** `src/genesis.rs`

```rust
pub enum AppState {
    Content(Content),         // Tam genesis
    Checkpoint(Vec<u8>),     // Hash kontrol noktasi (hizli yeniden baslatma)
}

pub struct Content {
    pub chain_id: String,
    pub community_pool_content: CommunityPoolContent,
    pub distributions_content: DistributionsContent,
    pub fee_content: FeeContent,
    pub funding_content: FundingContent,
    pub governance_content: GovernanceContent,
    pub ibc_content: IBCContent,
    pub sct_content: SctContent,
    pub shielded_pool_content: ShieldedPoolContent,
    pub stake_content: StakeContent,
    pub dex_content: DexContent,
    pub auction_content: AuctionContent,
}

// Yardimci kurucular:
Content::with_chain_id(chain_id) -> Self
Content::with_epoch_duration(epoch_duration) -> Self  // sct_params.epoch_duration
Content::with_unbonding_delay(unbonding_delay) -> Self // stake_params.unbonding_delay
```

**Onemli:** Genesis'te validator'lar stake_content icerisinde tanimlanir — Tendermint genesis validators alanina guvenilmez.

**Kontrol noktasi modu:** `AppState::Checkpoint` ile baslayan zincirde sadece temel bilesenleri baslatir (Auction ve Funding haric).

### 1.5 Uygulama Parametreleri

**Dosya:** `src/params.rs`, `src/params/change.rs`

```rust
pub struct AppParameters {
    pub chain_id: String,
    pub sct_params: SctParameters,
    pub community_pool_params: CommunityPoolParameters,
    pub governance_params: GovernanceParameters,
    pub ibc_params: IBCParameters,
    pub stake_params: StakeParameters,
    pub fee_params: FeeParameters,
    pub distributions_params: DistributionsParameters,
    pub funding_params: FundingParameters,
    pub shielded_pool_params: ShieldedPoolParameters,
    pub dex_params: DexParameters,
    pub auction_params: AuctionParameters,
}
```

`ParameterChangeExt`: Governance teklifi uzerinden her parametreyi guncelleme yetenegi.

### 1.6 Eylem Isleyici

**Dosya:** `src/action_handler.rs`

```rust
pub trait AppActionHandler {
    fn check_stateless(&self, context) -> impl Future<Output = Result<()>>;
    fn check_historical(&self, state: Arc<S>) -> impl Future<Output = Result<()>>;  // varsayilan impl Ok(()) döner
    fn check_and_execute(&self, state: &mut StateDelta<S>) -> impl Future<Output = Result<()>>;
    // DUZELTME: check_stateful ve execute isimleri YANLISTIR — kaynak: action_handler.rs:26-32
}
```

Her islem turu (Spend, Output, Swap, ...) bu trait'i implemente eder.

---

## 2. core/asset — Varlik Sistemi

### 2.1 Asset ID

**Dosya:** `src/asset/id.rs`

```rust
pub struct Id(pub Fq);  // BLS12-377 skaler alani elemani (32 byte)
```

**Kimlik hesaplama:**
```
from_raw_denom(base_denom: &str) -> Id:
  Id(Fq::from_le_bytes_mod_order(
    BLAKE2b("Penumbra_AssetID", base_denom.as_bytes())
  ))
```

**IBC varlik kimligi:** SHA-256 yerine dogrudan Fq alani elemani olarak hashlanir — bu, asset ID'lerin ZK devrelerinde kullanilabilmesini saglar.

**Deger ureteci:**
```rust
Id::value_generator(&self) -> decaf377::Element:
  poseidon377::hash_1(&VALUE_GENERATOR_DOMAIN_SEP, self.0) → Fq
  decaf377::Element::encode_to_curve(fq)  // hash-to-curve
```
Her asset ID'nin kendi grup elemani (G_v) vardir — deger taahhutleri icin.

**Kodlama:**
```
Bech32m: "passet1..." prefix
Proto: AssetId { inner, alt_bech32m, alt_base_denom }
  - Hepsi birbirinin yerini tutabilir (TryFrom icerisinde cokluk)
  - Uretilenler daima inner kullanir
```

**Gercek valiklar (test verisi):**
- UM: `29ea9c2f3371f6a487e7e95c247041f4a356f983eb064e5d2b3bcf322ca96a10`
- USDC: `76b3e4b10681358c123b381f90638476b7789040e47802de879f0fb3eedc8d0b`
- ATOM: `07ef660132a4c3235fab272d43d9b9752a8337b2d108597abffaff5f246d0f0f`

### 2.2 Dege Taahhutleri (Balance Commitments)

**Dosya:** `src/balance/commitment.rs`

```
VALUE_BLINDING_GENERATOR:
  H = decaf377::Element::encode_to_curve(
    Fq::from_le_bytes_mod_order(BLAKE2b("decaf377-rdsa-binding"))
  )

Value::commit(self, blinding: Fr) -> Commitment:
  G_v = asset_id.value_generator()  // varliga ozgu grup noktas
  C = v * G_v + blinding * H        // Pedersen commitment

Commitment(decaf377::Element)  // 32 byte sıkiştırılmış
```

**Homoforik ozellik:**
```rust
impl Add<Commitment> for Commitment  // C1 + C2 = Enc(v1+v2)
impl Sub<Commitment> for Commitment  // C1 - C2 = Enc(v1-v2)
impl Neg for Commitment              // -C = Enc(-v)
```

Bu ozellik, islem bakiyesini ZK'da dogrulamak icin kritiktir: `sum(inputs) == sum(outputs)`.

**R1CS gadget:** `BalanceCommitmentVar` — arkworks devre icerisinde deger taahhutlerini kismak icin.

### 2.3 Balance Yapisi

**Dosya:** `src/balance.rs`

```rust
pub struct Balance {
    pub negated: bool,
    pub balance: BTreeMap<Id, Imbalance<NonZeroU128>>,
}

enum Imbalance<T> {
    Required(T),   // bu varliktan bu kadar gerekli (giris)
    Provided(T),   // bu varliktan bu kadar saglanmis (cikis)
}
```

**Add/Sub:** `Required + Provided` → iptal (denge sifira dogru gider)
**Neg:** Tum Required ↔ Provided yer degistirir
**Sifir bakiye:** `balance.is_zero()` → islem gecerliligi

**Onemli:** `BTreeMap<Id, Imbalance>` — ayni varliktan `Required(x)` ve `Provided(y)` asla ayni anda depolanamaz; bunun yerine farklar birlestiri.

### 2.4 Deger

**Dosya:** `src/value.rs`

```rust
pub struct Value {
    pub amount: Amount,    // u128 miktar
    pub asset_id: Id,     // varlik kimligi
}
```

### 2.5 Denom ve Metadata

**Dosya:** `src/asset/denom.rs`, `src/asset/denom_metadata.rs`

```rust
pub struct Denom {
    pub denom: String,  // orn. "upenumbra", "transfer/channel-0/uatom"
}
```

`DenomMetadata`: Cosmos SDK'dan gelen `penumbra.core.asset.v1.DenomMetadata` proto mesajini sarmalar. Birimleri (orn. penumbra = 1_000_000 upenumbra) ve IBC izlerini icerir.

**Kayit defteri:**
```rust
// src/asset/registry.rs
pub struct Registry {
    inner: BTreeMap<String, DenomMetadata>,
}
```
Gecerli denominasyon string'lerini dogrular ve normalize eder.

### 2.6 Esdeger Deger

**Dosya:** `src/equivalent_value.rs`, `src/estimated_price.rs`

Fiyat tahminine dayali birimler arasi cevirme yardimcilari — kullanici arayuzu amacli.

---

## 3. Nova Shield Uygulamasi Icin Notlar

### App Mimarisi

**Dogrudan uygulanabilir pattern'lar:**
1. `App` struct + `InterBlockState = Arc<StateDelta<Snapshot>>` — cnidarium'a dogrudan bagli ama pattern alinabilir
2. `MAX_BLOCK_TXS_PAYLOAD_BYTES = 1MB`, `MAX_TRANSACTION_SIZE_BYTES = 96KB` — bizim icin makul baslangic degerleri
3. `AppState::Content` / `AppState::Checkpoint` — genesis formati iki modlu
4. `ActionHandler` trait: `check_stateless → check_stateful → execute` — islem dogrulama pipeline'i

**Genesis icerik yapisi:** Her bilesene kendi `genesis::Content` struct'i, ust seviye `AppState::Content`'te birlestiriliyor. Nova Shield icin ayni yapi kullanilabilir.

### Asset Sistemi

**Kritik bulgu — AssetId olarak Fq:**
- `Id(Fq)` — ZK devresinde dogrudan kullanilabilir
- `from_raw_denom`: `BLAKE2b("Penumbra_AssetID", denom)` — deterministic ve alan disinda
- `value_generator()`: Her asset ID icin `hash_to_curve` — Pedersen commitment icin gerekli

**Nova Shield DEX icin:**
- Ayni asset ID sema kullanilabilir (BLAKE2b → Fq)
- `Value::commit(blinding)` → Pedersen taahhutleri
- Balance homoforik isleme (Add/Sub/Neg) → islem dogrulama

### Bakiye Taahhutleri

```
Islem gecerliligi:
  sum(input.commit(r_i)) == sum(output.commit(r_j))
  <=> sum(v_i * G_v) + sum(r_i * H) == sum(v_j * G_v) + sum(r_j * H)
  <=> v_in == v_out AND sum(r_i) == sum(r_j)
```

Bu yapi, ZK ispat icerisinde islemin deger korumasi saglar.

---

## 4. Kritik Degerler Ozeti

| Deger | Kaynak |
|-------|--------|
| MAX_BLOCK_TXS_PAYLOAD_BYTES | 1_048_576 (1 MB) |
| MAX_TRANSACTION_SIZE_BYTES | 98_304 (96 KB) |
| MAX_EVIDENCE_SIZE_BYTES | 30_720 (30 KB) |
| AssetId hash: personalized BLAKE2b | "Penumbra_AssetID" |
| Value blinding generator | BLAKE2b("decaf377-rdsa-binding") → hash-to-curve |
| Value generator | poseidon377_hash_1(VALUE_GENERATOR_DOMAIN_SEP, asset_id.0) |
| GROTH16_PROOF_LENGTH_BYTES | 192 (A=48, B=96, C=48) |

---

## 5. Dosya Ozeti

### core/app (~20 dosya)
| Dosya | Icerik |
|-------|--------|
| app/mod.rs | App struct, ABCI handler'ları, boyut sabitleri |
| genesis.rs | AppState{Content, Checkpoint}, Content (12 bileseni) |
| params.rs | AppParameters (12 bilesenin parametreleri) |
| params/change.rs | ParameterChangeExt — governance guncellemeleri |
| action_handler.rs | AppActionHandler trait (check_stateless/stateful/execute) |
| action_handler/actions.rs | Tum islem turu dispatch'leri |
| action_handler/transaction.rs | TX-seviyesi kontroller |
| server/consensus.rs | CometBFT ABCI consensus handler'ı |
| server/mempool.rs | Mempool kontrolleri |
| server/info.rs | Info sorgulamalari |
| rpc.rs | gRPC sorgulari |
| app_version.rs | Uygulama surum takibi |
| event.rs | EventAppParametersChange |
| metrics.rs | Prometheus metrikleri |

### core/asset (~12 dosya)
| Dosya | Icerik |
|-------|--------|
| asset/id.rs | Id(Fq), from_raw_denom, value_generator, bech32m kodlama |
| balance.rs | Balance{negated, BTreeMap<Id,Imbalance>}, Add/Sub/Neg |
| balance/commitment.rs | Commitment(Element), VALUE_BLINDING_GENERATOR, commit() |
| balance/imbalance.rs | Imbalance<T>{Required, Provided} |
| value.rs | Value{amount:Amount, asset_id:Id}, ValueVar |
| asset/denom.rs | Denom yapisi |
| asset/denom_metadata.rs | DenomMetadata (Cosmos SDK uyumlu) |
| asset/registry.rs | Registry - denominasyon dogrulama |
| asset/cache.rs | Varlik metadata onbellegi |
| asset/r1cs.rs | AssetIdVar - ZK devre gadget'i |
| equivalent_value.rs | Fiyat tahmini yardicilari |
| estimated_price.rs | EstimatedPrice yapisi |
