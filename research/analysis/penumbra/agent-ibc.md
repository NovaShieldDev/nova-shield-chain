# Penumbra IBC Bileşeni — Kapsamlı Analiz

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `crates/core/component/ibc/` — 49 dosya, ~7770 satır Rust
**Analist:** Agent-IBC

---

## Genel Bakış

Penumbra'nın IBC bileşeni, Inter-Blockchain Communication protokolünün tam bir Rust implementasyonudur. Bileşen, ICS-02 (light client), ICS-03 (connection), ICS-04 (channel/packet) ve ICS-20 (token transfer) protokollerini destekler. Tasarım, generic `AppHandler` ve `HostInterface` trait'leri üzerine kuruludur; bu sayede IBC çekirdeği uygulama katmanından tamamen ayrılmıştır.

**Toplam Satır:** ~7770 Rust kodu
**Desteklenen IBC Mesaj Tipleri:** 17 (CreateClient, UpdateClient, UpgradeClient, SubmitMisbehaviour, 4x Connection, 6x Channel, RecvPacket, Acknowledgement, Timeout)
**Light Client:** Sadece Tendermint (07-tendermint) — diğer tipler `Unknown` olarak işlenir
**IBC Proof Store:** JMT (Jellyfish Merkle Tree) tabanlı, ics23 spec ile
**RPC:** Tonic gRPC — client_query, connection_query, consensus_query (channel query dahil)

---

## Dosya Bazlı Analiz

---

## [lib.rs]

**Dosya:** `.../ibc/src/lib.rs`
**Satır Sayısı:** 28

### Genel Amaç

Crate'in kök modülü. Publik API'yi organize eder ve `component` feature flag'i ile koşullu export yapar.

### Struct/Enum/Type Listesi

Doğrudan tanım yok; re-export'lar:
- `IbcRelay` — tüm IBC mesaj tiplerini kapsayan enum
- `IbcToken` — IBC üzerinden gelmiş token temsili
- `MerklePrefixExt`, `IBC_COMMITMENT_PREFIX`, `IBC_PROOF_SPECS`, `IBC_SUBSTORE_PREFIX` — prefix ve proof spec sabitleri
- `IbcRelayWithHandlers` — handler bilgisini taşıyan wrapper (feature: component)
- `StateReadExt`, `StateWriteExt` — state okuma/yazma trait'leri (feature: component)

### Tasarım Kararları

- `#![deny(clippy::unwrap_used)]` — tüm crate'te `.unwrap()` yasak, hata yönetimi zorunlu
- `#![allow(unreachable_patterns)]` — IBC enum variant'ları build config'e göre değiştiğinden fallthrough pattern'lara izin verilir
- `component` feature flag: RocksDB ve ağır bağımlılıkları olan implementasyon kodu, proof verifikasyonu için gereken hafif sabitleri (IBC_PROOF_SPECS) bu flag olmadan da kullanılabilir kılar (relayer'lar için önemli)

### Nova Shield Chain İçin Dersler

- IBC'yi Nova Shield'e entegre ederken `component` feature flag'i aynı şekilde kullanılabilir: hafif istemciler (relayer) ve tam node'lar ayrı bağımlılıklara sahip olabilir
- `unwrap_used` deny kuralı production güvenliği için zorunlu; kendi IBC implementasyonumuzda aynı kural uygulanmalı

---

## [params.rs]

**Dosya:** `.../ibc/src/params.rs`
**Satır Sayısı:** 50

### Genel Amaç

IBC bileşeninin runtime parametrelerini tanımlar. Parametreler zincir yönetişimi aracılığıyla değiştirilebilir.

### Struct/Enum/Type Listesi

#### `IBCParameters` (Satır 5–14)
```rust
pub struct IBCParameters {
    pub ibc_enabled: bool,
    pub inbound_ics20_transfers_enabled: bool,
    pub outbound_ics20_transfers_enabled: bool,
}
```
- Protobuf: `pb::IbcParameters`
- Default: hepsi `true`

### Trait Implementasyonları

- `DomainType for IBCParameters`: Proto `pb::IbcParameters` ile dönüşüm
- `TryFrom<pb::IbcParameters>`: Protobuf'tan domain tipine
- `From<IBCParameters> for pb::IbcParameters`: Domain tipinden protobuf'a
- `Default`: Tüm parametreler true

### Tasarım Kararları

- İnbound ve outbound ICS-20 transferleri bağımsız olarak açılıp kapatılabilir: sadece gelen veya sadece giden transferi disable etme imkanı. Bu güvenlik açısından değerli (örneğin, yalnızca outbound transfer dondurulabilir).
- `ibc_enabled: false` iken tüm IBC mesajları reddedilir (check_historical aşamasında)

### Nova Shield Chain İçin Dersler

- Nova Shield'in IBC parametresi sistemi bu yapıyı baz alabilir
- Governance override için "emergency pause" — inbound/outbound ayrı kontrol kritik güvenlik özelliği
- Önerimiz: `max_clients`, `max_channels`, `allowed_client_types` gibi ek parametreler eklenebilir

---

## [prefix.rs]

**Dosya:** `.../ibc/src/prefix.rs`
**Satır Sayısı:** 67

### Genel Amaç

IBC state'inin JMT içinde hangi alt-namespace'de saklandığını ve ICS-23 proof spec'inin ne olduğunu tanımlar. Relayer'ların da bu sabitlere ihtiyacı olduğundan `component` feature flag'i dışında.

### Struct/Enum/Type Listesi

#### Sabitler
```rust
pub static IBC_SUBSTORE_PREFIX: &'static str = "ibc-data";
pub static IBC_COMMITMENT_PREFIX: Lazy<MerklePrefix> = ...;
pub static IBC_PROOF_SPECS: Lazy<Vec<ics23::ProofSpec>> = ...;
```

#### `MerklePrefixExt` trait (Satır 55–66)
```rust
pub trait MerklePrefixExt {
    fn apply_string(&self, path: String) -> String;
}
impl MerklePrefixExt for MerklePrefix {
    fn apply_string(&self, path: String) -> String {
        format!("{}/{}", prefix_string, path)
    }
}
```

### ICS-23 Proof Spec (KRİTİK)

JMT (Jellyfish Merkle Tree) için özel ics23 ProofSpec — **satır satır değerler:**

```
LEAF_DOMAIN_SEPARATOR    = b"JMT::LeafNode"
INTERNAL_DOMAIN_SEPARATOR = b"JMT::IntrnalNode"   // YAZIM HATASI: "Intrnal"
SPARSE_MERKLE_PLACEHOLDER_HASH = b"SPARSE_MERKLE_PLACEHOLDER_HASH__"

LeafOp:
  hash = SHA256, prehash_key = SHA256, prehash_value = SHA256
  length = NoPrefix
  prefix = LEAF_DOMAIN_SEPARATOR

InnerOp:
  hash = SHA256
  child_order = [0, 1]
  min_prefix_length = max_prefix_length = len("JMT::IntrnalNode") = 16
  child_size = 32
  empty_child = SPARSE_MERKLE_PLACEHOLDER_HASH

min_depth = 0, max_depth = 64
prehash_key_before_comparison = true
```

**IBC_PROOF_SPECS:** `vec![ics23_spec(), ics23_spec()]` — İKİ kez aynı spec. Bu iki katmanlı Merkle proof'a işaret eder: önce substore, sonra global store.

### Tasarım Kararları

- Spec "vendored" — cnidarium crate'inden bağımsız kopyalanmış. Relayer'lar ve chain config'ler bu değerleri replicate etmek zorunda olduğundan bağımlılık ağırlıklı bir import'tan kaçınılmış.
- `SPARSE_MERKLE_PLACEHOLDER_HASH` 32 byte sabit ASCII — JMT'nin boş düğüm gösterimi
- `"JMT::IntrnalNode"` yazım hatası (Intrnal değil Internal), **kaynakta böyle** — kritik: değiştirirseniz mevcut proof'lar bozulur

### Nova Shield Chain İçin Dersler

- Eğer Nova Shield de JMT kullanacaksa aynı IBC_PROOF_SPECS kullanılmalı
- Relayer uyumluluğu için bu spec değerleri değiştirilemez; chain launch'ta doğru set edilmeli
- "JMT::IntrnalNode" yazım hatasına dikkat — mevcut state uyumluluğu için kopyalanmalı

---

## [version.rs]

**Dosya:** `.../ibc/src/version.rs`
**Satır Sayısı:** 27

### Genel Amaç

IBC bağlantı versiyonu müzakeresini yapar. Yerel ve karşı tarafın desteklediği versiyonların kesişim kümesini bulur.

### Fonksiyonlar

#### `pick_connection_version(supported_versions, counterparty_versions) -> Result<Version>` (Satır 4–27)

**Algoritma:**
1. `supported_versions` × `counterparty_versions` çift döngüsü
2. `identifier` eşleşmesi kontrolü
3. Her feature'ın boş olmadığı doğrulaması
4. Eşleşenleri `intersection` vektörüne ekle
5. `sort_by identifier` — deterministik seçim için
6. `intersection[0]` — alfabetik olarak en küçük identifier seçilir (genellikle "1")

**Önemli:** Empty feature kontrolü: `feature.trim().is_empty()` → bail! Bu, geçersiz feature'lara sahip versiyonları reddeder.

### Tasarım Kararları

- `intersection.sort()` sonrası `[0]` seçimi deterministik — her iki taraf da aynı versiyonu seçer
- Nested loop O(n²) — versiyonlar az sayıda olduğu için sorun değil

### Nova Shield Chain İçin Dersler

- Nova Shield IBC bağlantılarında bu versiyon algoritmasını aynen kullanmalı
- `SUPPORTED_VERSIONS: Lazy<Vec<Version>> = vec![Version::default()]` — sadece varsayılan versiyon ("1") destekleniyor

---

## [ibc_action.rs]

**Dosya:** `.../ibc/src/ibc_action.rs`
**Satır Sayısı:** 292

### Genel Amaç

`IbcRelay` enum'ı — Penumbra transaction'ları içinde IBC mesajlarını temsil eden tip. 17 IBC mesaj tipini tek bir enum altında toplar.

### Struct/Enum/Type Listesi

#### `IbcRelay` (Satır 41–60)
```rust
pub enum IbcRelay {
    CreateClient(MsgCreateClient),
    UpdateClient(MsgUpdateClient),
    UpgradeClient(MsgUpgradeClient),
    SubmitMisbehavior(MsgSubmitMisbehaviour),
    ConnectionOpenInit(MsgConnectionOpenInit),
    ConnectionOpenTry(MsgConnectionOpenTry),
    ConnectionOpenAck(MsgConnectionOpenAck),
    ConnectionOpenConfirm(MsgConnectionOpenConfirm),
    ChannelOpenInit(MsgChannelOpenInit),
    ChannelOpenTry(MsgChannelOpenTry),
    ChannelOpenAck(MsgChannelOpenAck),
    ChannelOpenConfirm(MsgChannelOpenConfirm),
    ChannelCloseInit(MsgChannelCloseInit),
    ChannelCloseConfirm(MsgChannelCloseConfirm),
    RecvPacket(MsgRecvPacket),
    Acknowledgement(MsgAcknowledgement),
    Timeout(MsgTimeout),
    Unknown(pbjson_types::Any),
}
```

### Trait Implementasyonları

#### `EffectingData for IbcRelay` (Satır 135–139)
```rust
fn effect_hash(&self) -> EffectHash {
    EffectHash::from_proto_effecting_data(&self.to_proto())
}
```
Transaction etki hash'i — protobuf serializasyonu üzerinden

#### `DomainType for IbcRelay` (Satır 141–143)
Proto: `pb::IbcRelay`

#### `TryFrom<pb::IbcRelay>` (Satır 145–213)
Deserialization: `raw_action.type_url` üzerinden büyük if-else zinciri. `fn` çağrıları `match` pattern'larında kullanılamadığından if-else gerekli.

#### `From<IbcRelay> for pb::IbcRelay` (Satır 215–292)
Her variant için `pbjson_types::Any { type_url, value: msg.encode_to_vec().into() }` oluşturur.

### Fonksiyonlar

#### `create_span(&self, parent: &tracing::Span) -> tracing::Span` (Satır 67–132)
Her IBC mesaj tipi için tracing span oluşturur. `CreateClient` için Tendermint client state'inden `chain_id` ayrıştırılmaya çalışılır; başarısız olursa span'da chain_id olmaz ("HACK" yorumu var — satır 70).

### Tasarım Kararları

- `Unknown(pbjson_types::Any)` — bilinmeyen IBC mesaj tiplerini drop etmek yerine sararak ilerletir; uygulama seviyesinde hata verilir
- Type URL tabanlı dispatch: her protobuf mesaj tipi kendi `type_url()` metoduna sahip; bu, yanlış decode'u önler
- `EffectHash`: Penumbra'ya özgü — her IBC action'ın transaction hash'e katkısını protobuf serialization üzerinden hesaplar

### Nova Shield Chain İçin Dersler

- Nova Shield'in kendi mesaj dispatch sistemi benzer bir enum yapısı kullanabilir
- `type_url` tabanlı dispatch — mesaj tipi güvenliğini protobuf type system seviyesinde sağlar
- `Unknown` variant pattern: forward uyumluluk için kritik — yeni IBC mesaj tipleri eklenmeden önce Unknown'dan geçmeli

---

## [ibc_token.rs]

**Dosya:** `.../ibc/src/ibc_token.rs`
**Satır Sayısı:** 68

### Genel Amaç

IBC üzerinden transfer edilmiş token'ların temsili. ICS-20 denom trace formatını uygular.

### Struct/Enum/Type Listesi

#### `IbcToken` (Satır 6–12)
```rust
pub struct IbcToken {
    channel_id: ChannelId,
    port_id: PortId,
    original_denom: String,
    base_denom: asset::Metadata,
}
```

### Fonksiyonlar

#### `new(channel_id, port_id, denom) -> Self` (Satır 15–29)
```
transfer_path = "{port_id}/{channel_id}/{denom}"
// Örnek: "transfer/channel-31/uatom"
base_denom = asset::REGISTRY.parse_denom(&transfer_path)
```
`parse_denom` panic yapabilir (`expect`) — geçersiz denom durumunda

#### `denom(&self) -> asset::Metadata`
#### `default_unit(&self) -> asset::Unit`
#### `id(&self) -> asset::Id`
#### `transfer_path(&self) -> String` — `"{port_id}/{channel_id}/{original_denom}"`

### Test

```rust
fn test_derive_ibc_denom() {
    let expected = "transfer/channel-31/uatom";
    let ibctoken = IbcToken::new(&ChannelId::new(31), &PortId::transfer(), "uatom");
    assert_eq!(expected, ibctoken.transfer_path());
}
```

### Tasarım Kararları

- IBC denom formatı: `transfer/{channel_id}/{original_denom}` — ICS-20 standardı
- `asset::REGISTRY.parse_denom` ile tam entegrasyon — IBC token'ları native Penumbra asset sistemiyle aynı primitive üzerinden çalışır

### Nova Shield Chain İçin Dersler

- Nova Shield'in kendi IBC token temsili aynı `port_id/channel_id/denom` formatını kullanmalı
- IBC denom registry entegrasyonu kritik — native token'lar ile IBC token'lar aynı asset ID sistemiyle çalışmalı
- `expect` yerine `Result` döndürmek daha güvenli

---

## [genesis.rs]

**Dosya:** `.../ibc/src/genesis.rs`
**Satır Sayısı:** 37

### Genel Amaç

Zincir genesis'inde IBC bileşeninin başlangıç durumunu tanımlar.

### Struct/Enum/Type Listesi

#### `Content` (Satır 9–12)
```rust
pub struct Content {
    pub ibc_params: IBCParameters,
}
```

### Trait Implementasyonları

- `DomainType for Content`: Proto `pb::GenesisContent`
- `From<Content> for pb::GenesisContent`
- `TryFrom<pb::GenesisContent>`: `ibc_params` yoksa hata (context: "ibc params not present in protobuf message")

### Nova Shield Chain İçin Dersler

- Genesis content minimal: sadece `IBCParameters`. Nova Shield genesis'inde benzer bir yapı kullanılabilir.
- `ibc_params` field zorunlu — genesis dosyasında eksikse TryFrom hatası verir

---

## [component.rs]

**Dosya:** `.../ibc/src/component.rs`
**Satır Sayısı:** 41

### Genel Amaç

`component` modülünün kök dosyası. Alt modülleri organize eder ve publik API'yi re-export eder.

### Export Edilen Tipler

```rust
pub use client::ClientStatus;
pub use client::ConsensusStateWriteExt;
pub use client::StateReadExt as ClientStateReadExt;
pub use client::StateWriteExt as ClientStateWriteExt;
pub use client_recovery::ClientRecoveryExt;
pub use connection::StateReadExt as ConnectionStateReadExt;
pub use connection::StateWriteExt as ConnectionStateWriteExt;
pub use host_interface::HostInterface;
pub use view::{StateReadExt, StateWriteExt};
pub use ibc_component::Ibc;
```

### Tasarım Kararları

- Her sub-modülün kendi `StateReadExt` ve `StateWriteExt` trait'i var, isim çakışmalarını önlemek için `as` ile rename edilir

---

## [component/client.rs]

**Dosya:** `.../ibc/src/component/client.rs`
**Satır Sayısı:** 742

### Genel Amaç

IBC light client (ICS-02) state management. Tendermint light client oluşturma, güncelleme, durum sorgulama ve consensus state yönetimi.

### Struct/Enum/Type Listesi

#### `ClientStatus` (Satır 33–44)
```rust
pub enum ClientStatus {
    Active,    // Kullanılabilir
    Frozen,    // Misbehavior sonucu dondurulmuş
    Expired,   // trusting_period dolmuş
    Unknown,   // Durum belirlenemedi
    Unauthorized, // İzin verilmemiş client tipi
}
```

### Trait Implementasyonları

#### `Ics2ClientExt: StateWrite` (Satır 59–147) — `pub(crate)`

##### `async fn next_tendermint_state(client_id, trusted_client_state, verified_header) -> (TendermintClientState, TendermintConsensusState)`

**Algoritma (KRİTİK — Equivocation tespiti):**

1. Eğer bu height için zaten consensus state varsa:
   - Eşleşiyorsa: early return (duplicate update)
   - Eşleşmiyorsa: **CLIENT FREEZE** (`frozen_height = Height { revision_number: 0, revision_height: 1 }`)

2. `next_verified_consensus_state` ve `prev_verified_consensus_state` sorgula

3. Case 1 (prev state var): `header.time < prev_state.timestamp` → **CLIENT FREEZE** (timestamp geriye gidemiyor)

4. Case 2 (next state var): `header.time > next_state.timestamp` → **CLIENT FREEZE** (timestamp ileri giden bir block'un arkasına giremez)

5. Normal: `trusted_client_state.with_header(verified_header)` — header eklenerek güncellenir

**Satır 83–85 — Freeze height:** `Height { revision_number: 0, revision_height: 1 }` — ICS-07 standardına göre freeze için en küçük valid height

#### `ConsensusStateWriteExt: StateWrite + Sized` (Satır 150–194)

##### `async fn put_verified_consensus_state<HI: HostInterface>(height, client_id, consensus_state) -> Result<()>`

State yazım akışı:
1. `IBC_COMMITMENT_PREFIX.apply_string(ClientConsensusStatePath)` → consensus state yaz
2. `client_processed_times(client_id, height)` → nanosaniye cinsinden zaman damgası yaz (u64)
3. `client_processed_heights(client_id, height)` → hangi Penumbra block'unda işlendiği
4. `verified_heights` listesine bu height'ı ekle → `VerifiedHeights` güncelle

Bu 4 alan packet proof doğrulamasında delay_period hesabı için gerekli.

#### `StateWriteExt: StateWrite + StateReadExt` (Satır 197–241)

- `put_client_counter(counter)`: `"ibc_client_counter"` key
- `put_client(client_id, state)`: ClientTypePath + ClientStatePath
- `put_verified_heights(client_id, heights)`: `"penumbra_verified_heights/{client_id}/verified_heights"` — **PENUMBRA ÖZEL** (IBC spec değil)
- `put_penumbra_sdk_consensus_state(height, cs)`: `"penumbra_consensus_states/{height}"` — **PENUMBRA ÖZEL**

#### `StateReadExt: StateRead` (Satır 244–459)

- `client_counter()` → default 0
- `get_client_type(client_id)` → ClientType
- `get_client_state(client_id)` → TendermintClientState
- `get_client_status(client_id, current_block_time)` → ClientStatus

  **Status belirleme algoritması:**
  1. client_type yok → Unknown
  2. client_state yok → Unknown
  3. `client_state.is_frozen()` → Frozen
  4. latest_consensus_state yok → Expired
  5. `current_block_time.duration_since(latest_consensus_state.timestamp)` hata → Unknown
  6. `client_state.expired(time_elapsed)` → Expired
  7. Yoksa → Active

- `get_verified_heights(client_id)` → Option<VerifiedHeights>
- `get_penumbra_sdk_consensus_state(height)` → TendermintConsensusState
- `get_verified_consensus_state(height, client_id)` → TendermintConsensusState
- `get_client_update_height(client_id, height)` → ibc_types::Height
- `get_client_update_time(client_id, height)` → Timestamp (nanosaniye parse)
- `next_verified_consensus_state(client_id, height)` → height'tan **yüksek** olan en düşük consensus state (Binary search — **"WARNING: load-bearing sort"** yorumu satır 411)
- `prev_verified_consensus_state(client_id, height)` → height'tan **düşük** olan en yüksek consensus state (aynı uyarı satır 443)

### Testler

#### `test_create_and_update_light_client` (Satır 591–710)
Gerçek Cosmos Hub-Stargaze IBC mesajlarını kullanır (base64 encoded):
- `test/create_client.msg` — Stargaze light client oluşturma
- `test/update_client_1.msg` — İlk güncelleme
- `test/update_client_2.msg` — İkinci güncelleme

Test akışı: genesis setup → timestamp ayarla → create → client counter assert (1) → update × 2

#### `test_disabled_ibc_component` (Satır 712–741)
`ibc_enabled: false` parametresiyle `check_historical` aşamasında hata beklenir.

### MockHost ve MockAppHandler

Test için kullanılan mock implementasyonlar:
- `MockHost`: chain_id="mock_chain_id", revision=0, height ve timestamp state'den
- `MockAppHandler`: tüm methodlar `Ok(())`/no-op

### Tasarım Kararları

- **Equivocation tespiti inline**: `next_tendermint_state` içinde, ayrı bir misbehavior handler'a gerek yok; header update sırasında da tespit edilir
- **"WARNING: load-bearing sort"**: `verified_heights` heap'te sırasız saklanır, her sorguda sort gerekli — performans maliyeti var ama correctness açısından zorunlu
- **PENUMBRA ÖZEL state key'ler**: `penumbra_verified_heights/` ve `penumbra_consensus_states/` key'leri IBC spec'inde değil; counterparty chain'in bunları doğrulaması beklenmez

### Nova Shield Chain İçin Dersler

- Freeze height `{ revision_number: 0, revision_height: 1 }` tam standardı takip eder — aynen kullanılmalı
- `put_verified_consensus_state` imzası: `<HI: HostInterface>` generic — bu tasarım pattern host interface'i test edilebilir kılar
- `VerifiedHeights` listesi büyüyebilir — Nova Shield için height listesini sorted B-Tree olarak saklamak daha iyi olabilir (her sorguda sort gereksiz olur)
- Consensus state expiry: `current_block_time.duration_since(latest_consensus_state.timestamp)` hesabı — `duration_since` negatif olamaz (hata döner), dikkat!

---

## [component/connection.rs]

**Dosya:** `.../ibc/src/component/connection.rs`
**Satır Sayısı:** 89

### Genel Amaç

IBC bağlantı (ICS-03) state yönetimi. Bağlantı oluşturma, güncelleme ve sorgulama.

### Trait Implementasyonları

#### `StateWriteExt: StateWrite` (Satır 19–63)

- `put_connection_counter(counter)`: `state_key::counter()` = `"ibc/connection_counter"`
- `put_client_connection(client_id, paths)`: `IBC_COMMITMENT_PREFIX + ClientConnectionPath`
- `async fn put_new_connection(connection_id, connection) -> Result<()>`
  - ConnectionPath'e yazır
  - Client'ın connection listesine ekler
  - Counter'ı artırır (unwrap_or default 0'dan başlar)
- `update_connection(connection_id, connection)`: Counter artırmadan günceller

#### `StateReadExt: StateRead` (Satır 66–89)

- `get_connection_counter()` → default ConnectionCounter(0)
- `get_connection(connection_id)` → Option<ConnectionEnd>
- `get_client_connections(client_id)` → ClientPaths (default boş)

### Tasarım Kararları

- `ConnectionPath` ve `ClientConnectionPath` IBC spec'inin standart path formatlarıdır; IBC_COMMITMENT_PREFIX ile birleştirilir
- `update_connection` vs `put_new_connection`: yeni bağlantı counter artırır, güncelleme artırmaz

### Nova Shield Chain İçin Dersler

- `IBC_COMMITMENT_PREFIX.apply_string(path.to_string())` pattern'ı tüm IBC state'ine uygulanmalı — counterparty chain'lerin proof doğrulayabilmesi için

---

## [component/channel.rs]

**Dosya:** `.../ibc/src/component/channel.rs`
**Satır Sayısı:** 268

### Genel Amaç

IBC kanal (ICS-04 channel) state yönetimi. Sequence sayaçları, packet commitments, receipts ve acknowledgements.

### Trait Implementasyonları

#### `StateWriteExt: StateWrite + StateReadExt` (Satır 19–106)

| Method | Key Format | Veri |
|--------|-----------|------|
| `put_channel_counter(n)` | `"ibc_channel_counter"` | proto u64 |
| `next_channel_id()` | counter'dan türetilir | ChannelId |
| `put_channel(id, port, channel)` | `IBC_PREFIX + ChannelEndPath(port, id)` | ChannelEnd |
| `put_ack_sequence(...)` | `IBC_PREFIX + SeqAckPath(port, id)` | **big-endian u64 raw bytes** |
| `put_recv_sequence(...)` | `IBC_PREFIX + SeqRecvPath(port, id)` | **big-endian u64 raw bytes** |
| `put_send_sequence(...)` | `IBC_PREFIX + SeqSendPath(port, id)` | **big-endian u64 raw bytes** |
| `put_packet_receipt(packet)` | `IBC_PREFIX + ReceiptPath(port_b, chan_b, seq)` | `"1"` string |
| `put_packet_commitment(packet)` | `IBC_PREFIX + CommitmentPath(port_a, chan_a, seq)` | `commit_packet(packet)` hash |
| `delete_packet_commitment(...)` | CommitmentPath | silinir |
| `put_packet_acknowledgement(...)` | `IBC_PREFIX + AckPath(port, chan, seq)` | `commit_acknowledgement(ack)` hash |

**KRİTİK:** Sequence sayaçları raw bytes olarak saklanır (`put_raw` / `get_raw`) — `u64::to_be_bytes()` / `u64::from_be_bytes()`. Bu IBC standardı gereği: counterparty chain'lerin bu değerleri Merkle proof ile doğrulaması gerekir.

#### `StateReadExt: StateRead` (Satır 108–268)

- `get_channel_counter()` → default 0
- `get_channel(id, port)` → Option<ChannelEnd>
- `get_recv_sequence`, `get_ack_sequence`, `get_send_sequence`: raw bytes → `u64::from_be_bytes`; yoksa default 0
- `seen_packet(packet)` → ReceiptPath'te değer var mı?
- `seen_packet_by_channel(...)` → ReceiptPath + `filter(|s| !s.is_empty())` — boş string'i de yokmuş gibi say
- `get_packet_commitment(packet)` → CommitmentPath'ten raw bytes; boş bytes → None
- `get_packet_commitment_by_id(...)` → aynı mantık
- `get_packet_acknowledgement(port, chan, seq)` → AckPath raw bytes

### Tasarım Kararları

- **Raw bytes storage:** Sequence numaraları proto yerine raw big-endian bytes olarak saklanır. Bunun sebebi IBC spec'inde bu değerlerin Merkle proof'a dahil olması; proto encoding ile proof oluşturmak counterparty uyumsuzluğu yaratabilir.
- **Packet receipt = `"1"` string:** Minimal — sadece var/yok bilgisi gerektiğinden
- **Comment satır 17:** "Note: many of the methods on this trait need to write raw bytes, because the data they write is interpreted by counterparty chains." — kritik tasarım gerekçesi

### Nova Shield Chain İçin Dersler

- IBC sequence sayaçları mutlaka raw big-endian bytes olarak saklanmalı — proto encoding kullanılırsa counterparty chain'ler proof'u doğrulayamaz
- Packet commitment hash (commit_packet) ve ack hash (commit_acknowledgement) da ham bytes; bunlar ibc-go ile birebir uyumlu

---

## [component/packet.rs]

**Dosya:** `.../ibc/src/component/packet.rs`
**Satır Sayısı:** 313

### Genel Amaç

IBC packet gönderimi için type-safe API. `IBCPacket<S: CheckStatus>` ile "checked" ve "unchecked" paket durumlarını Rust type sisteminde ifade eder.

### Struct/Enum/Type Listesi

#### Sealed trait pattern (Satır 16–32)
```rust
pub trait CheckStatus: private::Sealed {}
pub enum Checked {}
pub enum Unchecked {}
impl CheckStatus for Checked {}
impl CheckStatus for Unchecked {}
mod private {
    pub trait Sealed {}
    impl Sealed for Checked {}
    impl Sealed for Unchecked {}
}
```

#### `IBCPacket<S: CheckStatus>` (Satır 34–94)
```rust
pub struct IBCPacket<S: CheckStatus> {
    pub(crate) source_port: PortId,
    pub(crate) source_channel: ChannelId,
    pub(crate) timeout_height: Height,
    pub(crate) timeout_timestamp: u64,  // nanoseconds
    pub(crate) data: Vec<u8>,
    m: PhantomData<S>,
}
```
- `IBCPacket<Unchecked>::assume_checked()` → `IBCPacket<Checked>` (güvensiz dönüşüm — sadece testte?)
- Getters: `source_port()`, `source_channel()`, `timeout_height()`, `timeout_timestamp()`, `data()`

### Trait Implementasyonları

#### `SendPacketRead: StateRead` (Satır 97–182)

##### `async fn send_packet_check(packet: IBCPacket<Unchecked>, current_block_time) -> Result<IBCPacket<Checked>>`

**Doğrulama adımları:**
1. Channel var mı?
2. Channel Closed mı?
3. Connection var mı?
4. Client frozen mı?
5. Client expired mı? (`client_state.expired(time_elapsed)`)
6. `packet.timeout_height <= latest_height` → hata (timeout zaten geçmiş)
7. `packet.timeout_timestamp <= chain_ts` → hata (timestamp zaten geçmiş)

Başarılıysa `IBCPacket<Checked>` döner.

#### `SendPacketWrite: StateWrite` (Satır 186–253)

##### `async fn send_packet_execute(packet: IBCPacket<Checked>)`

1. `get_send_sequence(channel, port)` → mevcut sequence
2. `put_send_sequence(channel, port, sequence + 1)` → artır
3. Packet nesnesini oluştur (counterparty channel/port bilgisi channel'dan alınır)
4. `put_packet_commitment(&packet)` → commit hash'i sakla
5. `record(events::packet::SendPacket {...})` → event emit

#### `WriteAcknowledgement: StateWrite` (Satır 256–313)

##### `async fn write_acknowledgement(packet, ack_bytes) -> Result<()>`

ICS-04 spec satır 779 referanslı:
1. `ack_bytes.is_empty()` → bail!
2. Zaten ack varsa → bail! (double ack önleme)
3. Channel var mı?
4. `put_packet_acknowledgement(...)` → SHA256(ack_bytes) sakla
5. `record(events::packet::WriteAcknowledgement {...})` → event emit

### Tasarım Kararları

- **Type-state pattern:** `IBCPacket<Checked>` vs `IBCPacket<Unchecked>` — `send_packet_execute` yalnızca Checked kabul eder; check bypass'ı compile-time'da imkânsız
- **`assume_checked()`:** Bir escape hatch; dikkatli kullanılmalı
- Packet event `dst_connection_id`: channel'ın `connection_hops[0]`'dan alınır — sadece 1-hop destekleniyor

### Nova Shield Chain İçin Dersler

- Type-state pattern packet güvenliği için ideal — `IBCPacket<Unchecked>` ile execute'a gitmek mümkün olmamalı
- `timeout_timestamp: u64` nanoseconds — Unix epoch nanoseconds standardı
- Double ack koruması kritik: önce var/yok kontrol, sonra yaz

---

## [component/proof_verification.rs]

**Dosya:** `.../ibc/src/component/proof_verification.rs`
**Satır Sayısı:** 486

### Genel Amaç

IBC Merkle proof doğrulaması. ICS-23 uyumlu membership ve non-membership proof'larını doğrular. Bağlantı, kanal, packet, client ve consensus state proof'larını kapsar.

### Fonksiyonlar (Genel Yardımcılar)

#### `commit_packet(packet) -> Vec<u8>` (Satır 40–58)

**Algoritma (ibc-go ile uyumlu):**
```
commit = timeout_timestamp_on_b (8 bytes, big-endian)
       + timeout_height.revision_number (8 bytes, big-endian)
       + timeout_height.revision_height (8 bytes, big-endian)
       + SHA256(packet.data)
result = SHA256(commit)
```
Toplam: SHA256 of (8+8+8+32 = 56 bytes)

#### `commit_acknowledgement(ack_data) -> Vec<u8>` (Satır 63–65)
```
SHA256(ack_data)
```

#### `calculate_block_delay(delay_period_time, max_expected_time_per_block) -> u64` (Satır 67–77)
```
if max_expected == 0: return 0
ceil(delay_period_time.secs / max_time_per_block.secs)
```
`FloatCore::ceil` kullanılır. Sabit: `max_time_per_block = 20 seconds` (hardcoded — satır 466)

#### `verify_merkle_absence_proof(proof_specs, prefix, proof, root, path)` (Satır 79–90)
```
merkle_path = prefix.apply(vec![path.to_string()])
proof.verify_non_membership(proof_specs, root, merkle_path)
```

#### `verify_merkle_proof(proof_specs, prefix, proof, root, path, value)` (Satır 92–109)
```
merkle_path = prefix.apply(vec![path.to_string()])
proof.verify_membership(proof_specs, root, merkle_path, value, 0)
```
`0` parametresi: `min_depth` (ICS-23 spec)

### Trait Implementasyonları

#### `ClientUpgradeProofVerifier: StateReadExt + Sized` (Satır 112–179)

##### `async fn verify_client_upgrade_proof<HI>(client_id, client_state_proof, consensus_state_proof, upgraded_tm_consensus_state, upgraded_tm_client_state) -> Result<()>`

1. trusted_client_state → upgrade_path var mı?
2. `upgrade_path.pop()` → upgrade_path_prefix oluştur
3. client frozen mı?
4. client expired mı?
5. `verify_merkle_proof(proof_specs, upgrade_path_prefix, client_state_proof, trusted_root, ClientUpgradePath::UpgradedClientState(height))`
6. `verify_merkle_proof(proof_specs, upgrade_path_prefix, consensus_state_proof, trusted_root, ClientUpgradePath::UpgradedClientConsensusState(height))`

#### `ChannelProofVerifier: StateReadExt` (Satır 182–224)

##### `async fn verify_channel_proof(connection, proof, proof_height, channel_id, port_id, expected_channel) -> Result<()>`

1. trusted_client_state → frozen mı?
2. `get_verified_consensus_state(proof_height, client_id)` → trusted_consensus_state
3. `client_state.verify_height(proof_height)` — height doğrula
4. `verify_merkle_proof(proof_specs, counterparty.prefix, proof, root, ChannelEndPath(port_id, channel_id), expected_channel.encode_vec())`

#### `verify_connection_state(client_state, height, prefix, proof, root, conn_path, expected_conn) -> Result<()>` (Satır 226–250)
1. height doğrula
2. `verify_merkle_proof(proof_specs, prefix, proof, root, conn_path, expected_conn.encode_vec())`

#### `verify_client_full_state(...)` (Satır 252–275)
1. height doğrula
2. `verify_merkle_proof(proof_specs, prefix, proof, root, client_state_path, expected_client.encode_to_vec())`

#### `verify_client_consensus_state(...)` (Satır 277–300)
1. height doğrula
2. `verify_merkle_proof(proof_specs, prefix, proof, root, cons_state_path, expected_cons.encode_to_vec())`

#### `PacketProofVerifier: StateReadExt + inner::Inner` (Satır 302–429)

##### `async fn verify_packet_recv_proof<HI>(connection, msg: &MsgRecvPacket) -> Result<()>`
1. `get_trusted_client_and_consensus_state::<HI>(client_id, msg.proof_height_on_a, connection)` — delay check dahil
2. CommitmentPath oluştur
3. `commit_packet(&msg.packet)` — beklenen commitment hash'i hesapla
4. `verify_merkle_proof(proof_specs, counterparty.prefix, msg.proof_commitment_on_a, root, commitment_path, commitment_bytes)`

##### `async fn verify_packet_ack_proof<HI>(connection, msg: &MsgAcknowledgement) -> Result<()>`
1. `get_trusted_client_and_consensus_state::<HI>(client_id, msg.proof_height_on_b, connection)`
2. AckPath oluştur
3. `commit_acknowledgement(&msg.acknowledgement)` — beklenen ack hash'i hesapla
4. `verify_merkle_proof(..., ack_path, ack_bytes)`

##### `async fn verify_packet_timeout_proof<HI>(connection, msg: &MsgTimeout) -> Result<()>`
Ordered channel için:
- `seq_bytes = msg.next_seq_recv_on_b.to_be_bytes()`
- `SeqRecvPath(msg.packet.port_on_b, msg.packet.chan_on_b)`
- `verify_merkle_proof(..., seq_path, seq_bytes)`

##### `async fn verify_packet_timeout_absence_proof<HI>(connection, msg: &MsgTimeout) -> Result<()>`
Unordered channel için (packet alınmadı = receipt yok):
- ReceiptPath
- `verify_merkle_absence_proof(...)` — NON-MEMBERSHIP proof

#### `inner::Inner` (Satır 431–486)

##### `async fn get_trusted_client_and_consensus_state<HI>(client_id, height, connection) -> Result<(TendermintClientState, TendermintConsensusState)>`

**Delay period doğrulaması:**
```
max_time_per_block = 20 seconds (hardcoded)
delay_period_time = connection.delay_period
delay_period_blocks = calculate_block_delay(delay_period_time, max_time_per_block)

TendermintClientState::verify_delay_passed(
    current_timestamp,
    current_height,
    processed_time,
    processed_height,
    delay_period_time,
    delay_period_blocks,
)
```
Bu, bir client update'in yeterince eski olduğunu doğrular (relay saldırılarını önler).

### Tasarım Kararları

- **`inner::Inner`:** Sealed inner module pattern — `get_trusted_client_and_consensus_state` dışarıya expose edilmez
- **Hardcoded `max_time_per_block = 20s`:** Satır 466'da yorum: "NOTE: hardcoded for now, should probably be a chain parameter" — potansiyel iyileştirme noktası
- **Non-membership proof (timeout unordered):** Sadece receipt yolu için absence proof yeterli — sequence sayacı gerekmez

### Nova Shield Chain İçin Dersler

- `commit_packet` formülü ibc-go ile birebir uyumlu — farklı implementasyon relayer uyumsuzluğu yaratır
- `max_time_per_block = 20s` hardcoded — Nova Shield'de bunu chain parameter yapmak mantıklı
- Delay period doğrulaması: packet proof'larında `processed_time` ve `processed_height` kaydedilmeli; bu olmadan delay period güvenliği yoktur
- Non-membership proof: JMT desteklemeli; eğer Nova Shield farklı Merkle tree kullanıyorsa ICS-23 spec'ini update etmeli

---

## [component/msg_handler.rs]

**Dosya:** `.../ibc/src/component/msg_handler.rs`
**Satır Sayısı:** 40

### Genel Amaç

`MsgHandler` trait tanımı — tüm IBC mesaj handler'larının uygulaması gereken interface.

### Trait

#### `MsgHandler: (crate internal)` (Satır 29–39)
```rust
pub(crate) trait MsgHandler {
    async fn check_stateless<AH: AppHandlerCheck>(&self) -> Result<()>;
    async fn try_execute<S: StateWrite, AH: AppHandlerCheck + AppHandlerExecute, HI: HostInterface>(
        &self, state: S,
    ) -> Result<()>;
}
```

**Önemli:** `check_historical` ayrı bir adım **değil** — `try_execute` içine entegre. Bu, ActionHandler (üst seviye trait) ile farklıdır.

### Tasarım Kararları

- `check_stateless` ve `try_execute` ayrımı: stateless check'ler execute'dan önce, validate-first yaklaşımı
- Her mesaj tipi kendi `impl MsgHandler for MsgXxx` blokuna sahip — separation of concerns

---

## [component/msg_handler/create_client.rs]

**Dosya:** `.../ibc/src/component/msg_handler/create_client.rs`
**Satır Sayısı:** 92

### Genel Amaç

IBC client oluşturma (ICS-02 CreateClient) mesajı handler'ı.

### Fonksiyonlar

#### `check_stateless` (Satır 17–21)
1. `client_state_is_tendermint(self)` — type_url == TENDERMINT_CLIENT_STATE_TYPE_URL
2. `consensus_state_is_tendermint(self)` — type_url == TENDERMINT_CONSENSUS_STATE_TYPE_URL

#### `try_execute<S: StateWrite, AH, HI: HostInterface>` (Satır 31–71)

**Akış:**
1. `ics02_validation::get_tendermint_client_state(self.client_state)` — deserialize
2. `state.client_counter().await?` → id_counter
3. `ClientId::new(client_type(), id_counter.0)` → `"07-tendermint-{id_counter}"`
4. `ics02_validation::get_tendermint_consensus_state(self.consensus_state)` — deserialize
5. `state.put_client(&client_id, client_state.clone())` — client tipi ve durumu yaz
6. `state.put_verified_consensus_state::<HI>(client_state.latest_height(), client_id, consensus_state)` — consensus state yaz
7. Counter artır
8. `state.record(CreateClient { client_id, client_type, consensus_height }.into())` — event emit

### Nova Shield Chain İçin Dersler

- Client ID format: `07-tendermint-{counter}` — ICS-07 standardı
- Sadece Tendermint light client kabul ediliyor — farklı light client istiyorsak bu kontrolü genişletmek gerekir

---

## [component/msg_handler/update_client.rs]

**Dosya:** `.../ibc/src/component/msg_handler/update_client.rs`
**Satır Sayısı:** 271

### Genel Amaç

IBC light client güncelleme (ICS-07 UpdateClient). Tendermint light client header doğrulama ve state güncelleme.

### Fonksiyonlar

#### `check_stateless` (Satır 27–31)
`header_is_tendermint(self)` — client_message type_url kontrolü

#### `try_execute<S: StateWrite, AH, HI: HostInterface>` (Satır 33–158)

**Tam akış:**
1. `update_is_already_committed` — duplicate check (no-op ise erken çık)
2. `client_is_present` — client var mı?
3. `client_is_not_frozen` — frozen değil mi?
4. `client_is_not_expired::<&S, HI>` — expired değil mi?
5. `ics02_validation::get_tendermint_header(self.client_message)` — header deserialize
6. `header_revision_matches_client_state` — revision number uyumu
7. `header_height_is_consistent` — `header.height > header.trusted_height`
8. `get_verified_consensus_state(&trusted_height, &self.client_id)` — trusted consensus state al
9. `verify_header_validator_set(&untrusted_header, &last_trusted_consensus_state)` — validator set hash doğrula
10. `TrustedBlockState { chain_id, header_time, height, next_validators, next_validators_hash }` oluştur
11. `UntrustedBlockState { signed_header, validators, next_validators: None }` oluştur
12. `ProdVerifier::default().verify_update_header(untrusted_state, trusted_state, &options, current_time)` → Verdict
13. Verdict match: Success → Ok, NotEnoughTrust → hata, Invalid → hata
14. `state.next_tendermint_state(...)` — equivocation kontrolü dahil yeni state hesapla
15. `state.put_client`, `state.put_verified_consensus_state::<HI>` — güncelle
16. Event emit

#### `verify_header_validator_set<'h>(untrusted_header, last_trusted_consensus_state)` (Satır 258–271)
```
untrusted_header.trusted_validator_set.hash() != last_trusted_consensus_state.next_validators_hash
→ hata
```
Trusted validator set hash'i son consensus state'in `next_validators_hash`'iyle eşleşmeli.

### Tasarım Kararları

- **Duplicate update no-op:** İki yarışan relay transaction'ının aynı header'ı göndermesi durumunda ikincisi hata yerine no-op döner — relay güvenilirliği için kritik
- **`ProdVerifier`:** tendermint-light-client-verifier crate'inin production doğrulayıcısı — BFT light client doğrulama algoritması

---

## [component/msg_handler/misbehavior.rs]

**Dosya:** `.../ibc/src/component/msg_handler/misbehavior.rs`
**Satır Sayısı:** 191

### Genel Amaç

IBC client misbehavior (çift imza / fork) kanıtı işleme ve client dondurma.

### Fonksiyonlar

#### `check_stateless` (Satır 23–37)
1. `misbehavior_is_tendermint` — tip kontrolü
2. `misbehavior_equivocation_violation || misbehavior_timestamp_monotonicity_violation` — en az biri gerekli

#### `try_execute` (Satır 39–96)

**Akış:**
1. Aynı misbehavior tipleri check (stateless ile aynı doğrulama)
2. `client_is_present` → client_state al
3. `client_is_not_frozen` — zaten dondurulmuş değil mi? (expired izin veriliyor — satır 58: "expired client'ı kanıt üzerine dondurmak mantıklı")
4. `verify_misbehavior_header(header1)` — header1'i light client ile doğrula
5. `verify_misbehavior_header(header2)` — header2'yi light client ile doğrula
6. `trusted_client_state.with_frozen_height(misbehavior.header1.height())` — freeze
7. `state.put_client(&self.client_id, frozen_client)` — dondurulmuş client'ı kaydet
8. `state.record(ClientMisbehaviour { ... }.into())` — event emit

#### `misbehavior_equivocation_violation(misbehavior) -> bool` (Satır 171–175)
```
header1.height() == header2.height()
&& header1.signed_header.commit.block_id.hash != header2.signed_header.commit.block_id.hash
```
**Aynı height'ta farklı blok → equivocation (çift imza)**

#### `misbehavior_timestamp_monotonicity_violation(misbehavior) -> bool` (Satır 177–181)
```
header1.height() < header2.height()
&& header1.signed_header.header.time > header2.signed_header.header.time
```
**Daha yüksek height'ta daha küçük zaman damgası → timestamp monotonicity violation**

### Tasarım Kararları

- `verify_misbehavior_header`: update_client'taki `verify_update_header` yerine `verify_misbehaviour_header` kullanılır — ikisi aynı verifier'ın farklı metodları
- Freeze height = `misbehavior.header1.height()` — update_client'tan farklı (orada `{0,1}` sabit)

---

## [component/msg_handler/upgrade_client.rs]

**Dosya:** `.../ibc/src/component/msg_handler/upgrade_client.rs`
**Satır Sayısı:** 150

### Genel Amaç

IBC client upgrade (counterparty chain upgrade sonrası client güncelleme).

### Fonksiyonlar

#### `try_execute` (Satır 42–149)

**Kritik tasarım (satır 52–73 — "sketchy and undocumented" yorumu):**
İnce nokta: verify etmeden önce bazı alanlar sıfırlanır (`zeroed_fields`):
- `trusting_period = ZERO_DURATION`
- `trust_level = TrustThreshold::ZERO`
- `allow_update.after_expiry = false`
- `allow_update.after_misbehaviour = false`
- `frozen_height = None`
- `max_clock_drift = ZERO_DURATION`

Bu, ibc-go'nun yaptığıyla birebir uyumlu. Gerekçe: upgrade sırasında trust parametrelerinin değişmesine izin vermemek; ama proof doğrulaması sıfırlanmış state üzerinden yapılır.

**Sonra new_client_state:**
- chain_id, unbonding_period, latest_height, proof_specs, upgrade_path → upgraded'dan alınır
- trust_level, trusting_period, max_clock_drift, allow_update, frozen_height → eski client'tan korunur

**new_consensus_state:**
- `root = "sentinel_root"` — sentinal root; post-upgrade ilk block'a kadar proof doğrulanmaz
- timestamp ve next_validators_hash → upgraded'dan alınır

### Tasarım Kararları

- `static SENTINEL_UPGRADE_ROOT: &str = "sentinel_root"` — upgrade sonrası geçersiz root, post-upgrade update'e kadar proof'lar çalışmaz
- Yorum satır 100: "NOTE: this client state can differ from the one that was committed on the other chain!" — diğer zincirin commitlediği state'den farklı olabilir, bu kasıtlı

---

## [component/msg_handler/connection_open_init.rs]

**Dosya:** `.../ibc/src/component/msg_handler/connection_open_init.rs`
**Satır Sayısı:** 89

### Genel Amaç

ICS-03 connection handshake adım 1: ConnectionOpenInit. Zincir A tarafında bağlantı başlatma.

**State geçişi:** none → INIT

### Fonksiyonlar

#### `check_stateless` (Satır 19–23)
`version_is_supported(self)`: nil versiyon OK, diğer versiyonlar SUPPORTED_VERSIONS'da olmak zorunda.

#### `try_execute` (Satır 25–66)

1. Client var mı? (`get_client_state`, `get_client_type`)
2. `connection_id = ConnectionId::new(counter)` — monotonically increasing
3. `compatible_versions = vec![Version::default()]`
4. `ConnectionEnd { state: Init, client_id, counterparty, versions, delay_period }` oluştur
5. `put_new_connection(&connection_id, new_connection_end)` — counter artırılır
6. Event emit

---

## [component/msg_handler/connection_open_try.rs]

**Dosya:** `.../ibc/src/component/msg_handler/connection_open_try.rs`
**Satır Sayısı:** 222

### Genel Amaç

ICS-03 connection handshake adım 2: ConnectionOpenTry. Zincir B tarafında.

**State geçişi:** none → TRYOPEN

### Doğrulama Adımları (try_execute)

1. `consensus_height_is_correct` — consensus height < current block height
2. `penumbra_sdk_client_state_is_well_formed` — counterparty'nin bizim için oluşturduğu client doğrulama
3. `pick_connection_version(&SUPPORTED_VERSIONS, &msg.versions_on_a)` — versiyon müzakeresi
4. `expected_conn` oluştur: `ConnectionEnd { state: Init, client_id: counterparty.client_id, ... }`
5. Client frozen mı?
6. Trusted consensus state al
7. **3 PROOF DOĞRULAMA:**
   - `verify_connection_state(...)` — counterparty INIT durumundaki bağlantıyı doğrula
   - `verify_client_full_state(...)` — counterparty'nin bizim client state'imizi doğrula
   - `verify_client_consensus_state(...)` — counterparty'nin bizim consensus state'imizi doğrula
8. Yeni bağlantı oluştur: `ConnectionEnd { state: TryOpen, ... }`
9. `put_new_connection`

---

## [component/msg_handler/connection_open_ack.rs]

**Dosya:** `.../ibc/src/component/msg_handler/connection_open_ack.rs`
**Satır Sayısı:** 243

### Genel Amaç

ICS-03 connection handshake adım 3: ConnectionOpenAck. Zincir A tarafında.

**State geçişi:** (INIT, TRYOPEN) → (OPEN, TRYOPEN)

### Doğrulama Adımları

1. `consensus_height_is_correct`
2. `penumbra_sdk_client_state_is_well_formed`
3. `verify_previous_connection` — mevcut bağlantı INIT durumunda ve versiyon uyumlu mu?
4. **3 PROOF DOĞRULAMA:**
   - `verify_connection_state` — counterparty TRYOPEN bağlantısını doğrula
   - `verify_client_full_state` — counterparty'nin bizim client state'imizi doğrula
   - `verify_client_consensus_state` — counterparty'nin bizim consensus state'imizi doğrula
5. Bağlantı durumunu OPEN'a al, counterparty connection_id güncelle

---

## [component/msg_handler/connection_open_confirm.rs]

**Dosya:** `.../ibc/src/component/msg_handler/connection_open_confirm.rs`
**Satır Sayısı:** 132

### Genel Amaç

ICS-03 connection handshake adım 4: ConnectionOpenConfirm. Zincir B tarafında.

**State geçişi:** (OPEN, TRYOPEN) → (OPEN, OPEN)

### Doğrulama Adımları

1. `verify_previous_connection` — bağlantı TRYOPEN'da mı?
2. **1 PROOF DOĞRULAMA** (diğerlerinden daha basit):
   - `verify_connection_state` — counterparty OPEN bağlantısını doğrula
3. Durum OPEN'a al

**Tasarım Notu:** OpenConfirm'de sadece connection state doğrulama yeterli; client/consensus state proof'ları OpenAck'te yapıldı.

---

## [component/msg_handler/channel_open_init.rs]

**Dosya:** `.../ibc/src/component/msg_handler/channel_open_init.rs`
**Satır Sayısı:** 123

### Genel Amaç

ICS-04 channel handshake adım 1: ChannelOpenInit. Zincir A tarafında.

### Önemli Notlar

1. **Optimistic channel handshake** izin veriliyor — connection OPEN olmak zorunda değil! (Satır 38–39 yorum)
2. **Sadece `transfer` port'u** destekleniyor: `if self.port_id_on_a == PortId::transfer()` else `bail!`
3. `AH::chan_open_init_check` → `AH::chan_open_init_execute` (app handler sıralı çağrı)
4. Sequence sayaçları 1'den başlar: `put_send_sequence(1)`, `put_recv_sequence(1)`, `put_ack_sequence(1)`

---

## [component/msg_handler/channel_open_try.rs]

**Dosya:** `.../ibc/src/component/msg_handler/channel_open_try.rs`
**Satır Sayısı:** 141

### Genel Amaç

ICS-04 channel handshake adım 2: ChannelOpenTry. Zincir B tarafında.

### Önemli Notlar

1. Connection **OPEN** olmak zorunda (Init'in aksine)
2. `verify_channel_proof(...)` — counterparty'nin INIT durumundaki channel'ını doğrula
3. `expected_channel_on_a = ChannelEnd { state: Init, ... }`
4. Sadece `transfer` port'u
5. Sequence sayaçları 1'den başlar

---

## [component/msg_handler/channel_open_ack.rs]

**Dosya:** `.../ibc/src/component/msg_handler/channel_open_ack.rs`
**Satır Sayısı:** 135

### Genel Amaç

ICS-04 channel handshake adım 3: ChannelOpenAck. Zincir A tarafında.

**State geçişi:** Init → OPEN

### Doğrulama

1. Channel INIT durumunda mı?
2. Connection OPEN mu?
3. `verify_channel_proof` — counterparty TRYOPEN channel'ı doğrula
4. `AH::chan_open_ack_check` → state update → `AH::chan_open_ack_execute`
5. `channel.set_state(Open)`, `channel.set_version(version_on_b)`, `channel.set_counterparty_channel_id(chan_id_on_b)`

---

## [component/msg_handler/channel_open_confirm.rs]

**Dosya:** `.../ibc/src/component/msg_handler/channel_open_confirm.rs`
**Satır Sayısı:** 120

### Genel Amaç

ICS-04 channel handshake adım 4: ChannelOpenConfirm. Zincir B tarafında.

**State geçişi:** TryOpen → OPEN

### Doğrulama

1. Channel TRYOPEN durumunda mı?
2. Connection OPEN mu?
3. `verify_channel_proof` — counterparty OPEN channel'ı doğrula
4. `AH::chan_open_confirm_check` → `AH::chan_open_confirm_execute`

---

## [component/msg_handler/channel_close_init.rs]

**Dosya:** `.../ibc/src/component/msg_handler/channel_close_init.rs`
**Satır Sayısı:** 87

### Genel Amaç

Kanal kapatma başlatma. Zincir A tarafında.

### Güvenlik Notu

Satır 33–37 yorum: "we probably do need capability authentication here, or some other authorization method, to prevent anyone from spuriously closing channels." — **YETKİLENDİRME EKSİK**. Herhangi biri `transfer` portundan kanal kapatma başlatabilir. Bu potansiyel güvenlik açığı.

### Doğrulama

1. Channel zaten Closed mı? (hata)
2. Connection OPEN mu?
3. `AH::chan_close_init_check` → state Closed → `AH::chan_close_init_execute`

---

## [component/msg_handler/channel_close_confirm.rs]

**Dosya:** `.../ibc/src/component/msg_handler/channel_close_confirm.rs`
**Satır Sayısı:** 125

### Genel Amaç

Kanal kapatma onayı. Zincir B tarafında. Proof gerektiriyor (Init'in aksine).

### Doğrulama

1. Channel zaten Closed mı? (hata)
2. Connection OPEN mu?
3. `verify_channel_proof` — counterparty CLOSED channel'ı doğrula
4. `AH::chan_close_confirm_check` → state Closed → `AH::chan_close_confirm_execute`

**Güvenlik notu:** Aynı capability authentication eksikliği (satır 37–40)

---

## [component/msg_handler/recv_packet.rs]

**Dosya:** `.../ibc/src/component/msg_handler/recv_packet.rs`
**Satır Sayısı:** 162

### Genel Amaç

Paket alma (ICS-04 RecvPacket). Gelen packet'i doğrular, duplicate kontrolü yapar.

### Doğrulama Akışı

1. Channel OPEN mu?
2. Counterparty port/channel eşleşmesi
3. Connection OPEN mu?
4. `packet.timeout_height_on_b.has_expired(height)` → bail!
5. Timeout timestamp kontrolü
6. `verify_packet_recv_proof::<HI>(&connection, self)` — Merkle proof doğrulama
7. **Ordered channel:** `sequence == next_sequence_recv` zorunlu
8. **Unordered channel:** `seen_packet(packet)` → bail! (zaten alındı)
9. `AH::recv_packet_check`
10. Ordered: `put_recv_sequence(+1)` / Unordered: `put_packet_receipt` (receipt = "1")
11. Event emit
12. `AH::recv_packet_execute`

### Tasarım Kararları

- **Ordered vs Unordered:** Farklı duplicate önleme: ordered = sequence counter, unordered = receipt store
- Timeout hem height hem timestamp üzerinden kontrol

---

## [component/msg_handler/acknowledgement.rs]

**Dosya:** `.../ibc/src/component/msg_handler/acknowledgement.rs`
**Satır Sayısı:** 139

### Genel Amaç

Paket onaylama (ICS-04 Acknowledgement). Gönderici tarafında ack işleme.

### Doğrulama Akışı

1. Channel OPEN mu?
2. Counterparty port/channel eşleşmesi
3. Connection OPEN mu?
4. `get_packet_commitment(&packet)` — gönderdiğimiz packet var mı?
5. `commitment == commit_packet(&packet)` — hash eşleşmesi (kendi gönderdiğimiz packet mı?)
6. `verify_packet_ack_proof::<HI>(&connection, self)` — Merkle proof
7. **Ordered channel:** `sequence == next_sequence_ack`
8. `AH::acknowledge_packet_check`
9. Ordered: `put_ack_sequence(+1)`
10. `delete_packet_commitment(...)` — commitment sil (double ack önleme)
11. Event emit
12. `AH::acknowledge_packet_execute`

### Tasarım Kararları

- Commitment silme kritik: ack sonrası commitment silinmezse aynı packet iki kez ack'lenebilir
- `commit_packet` hash doğrulaması: alınan ack'in gerçekten gönderilen packet'a ait olduğunu doğrular

---

## [component/msg_handler/timeout.rs]

**Dosya:** `.../ibc/src/component/msg_handler/timeout.rs`
**Satır Sayısı:** 158

### Genel Amaç

Zaman aşımına uğramış packet geri alma (ICS-04 Timeout).

### Doğrulama Akışı

1. Channel OPEN mu?
2. Counterparty channel/port eşleşmesi
3. Connection al
4. `last_update_time`, `proof_update_height` al
5. `packet.timed_out(&last_update_time.into(), proof_update_height)` — gerçekten timeout olmuş mu?
6. `get_packet_commitment` + hash doğrulama (gerçekten biz gönderdik mi?)
7. **Ordered channel:** `next_seq_recv_on_b > packet.sequence` + `verify_packet_timeout_proof`
8. **Unordered channel:** `verify_packet_timeout_absence_proof`
9. `H::timeout_packet_check`
10. `delete_packet_commitment`
11. **Ordered channel:** Channel CLOSED olur!
12. Event emit
13. `H::timeout_packet_execute`

### Tasarım Kararları

- Ordered channel timeout → kanal kapanır. Bu ICS-04 standardı: sıralı kanalda kayıp packet tolere edilemez.
- Unordered channel timeout → sadece commitment silinir, kanal açık kalır.

---

## [component/ibc_component.rs]

**Dosya:** `.../ibc/src/component/ibc_component.rs`
**Satır Sayısı:** 78

### Genel Amaç

IBC bileşeninin ABCI lifecycle hook'ları: `init_chain`, `begin_block`, `end_block`, `end_epoch`.

### Struct

#### `Ibc` (Satır 18)
Boş struct. Component state'i store'da tutulur.

### Fonksiyonlar

#### `init_chain<S: StateWrite>(mut state, app_state: Option<&genesis::Content>)` (Satır 25–33)
- Genesis varsa: `put_ibc_params(genesis.ibc_params)` + `put_client_counter(ClientCounter(0))`
- Yoksa: upgrade specific check (boş)

#### `begin_block<HI: HostInterface, S: StateWrite + 'static>(state, begin_block)` (Satır 36–65)

**KRİTİK:** Her block'un başında Penumbra'nın kendi consensus state'ini kendi store'una yazar:
```rust
let commitment_root = begin_block.header.app_hash.clone().into();
let cs = TendermintConsensusState::new(
    MerkleRoot { hash: commitment_root },
    begin_block.header.time,
    begin_block.header.next_validators_hash,
);
let height = Height::new(revision_number, begin_block.header.height)?;
state.put_penumbra_sdk_consensus_state(height, cs);
```

**Neden:** Counterparty chain'ler ConnectionOpenTry/Ack sırasında "bizim" consensus state'imizi doğrulamak zorunda. Bizim state'imizi bizim yazmamız gerekiyor, karşı tarafın gönderdiği ile karşılaştırmak için.

#### `end_block` ve `end_epoch`: Boş (satır 67–77)

### Nova Shield Chain İçin Dersler

- `begin_block` hook: kendi consensus state'imizi kendi store'umuza yazma pattern'ı — IBC'li her L1'in yapması gereken kritik adım
- `Arc::get_mut` pattern: `&mut Arc<S>` üzerinden exclusive write erişimi — async context'te dikkatli kullanılmalı

---

## [component/app_handler.rs]

**Dosya:** `.../ibc/src/component/app_handler.rs`
**Satır Sayısı:** 64

### Genel Amaç

IBC uygulama katmanı (app layer) trait'leri. IBC çekirdeği ile ICS-20 gibi uygulama katmanı arasındaki interface.

### Trait Listesi

#### `AppHandlerCheck: Send + Sync` (Satır 21–42)
```rust
async fn chan_open_init_check<S: StateRead>(state, msg) -> Result<()>;
async fn chan_open_try_check<S: StateRead>(state, msg) -> Result<()>;
async fn chan_open_ack_check<S: StateRead>(state, msg) -> Result<()>;
async fn chan_open_confirm_check<S: StateRead>(state, msg) -> Result<()>;
async fn chan_close_confirm_check<S: StateRead>(state, msg) -> Result<()>;
async fn chan_close_init_check<S: StateRead>(state, msg) -> Result<()>;
async fn recv_packet_check<S: StateRead>(state, msg) -> Result<()>;
async fn timeout_packet_check<S: StateRead>(state, msg) -> Result<()>;
async fn acknowledge_packet_check<S: StateRead>(state, msg) -> Result<()>;
```

#### `AppHandlerExecute: Send + Sync` (Satır 48–62)
```rust
async fn chan_open_init_execute<S: StateWrite>(state, msg);
async fn chan_open_try_execute<S: StateWrite>(state, msg);
async fn chan_open_ack_execute<S: StateWrite>(state, msg);
async fn chan_open_confirm_execute<S: StateWrite>(state, msg);
async fn chan_close_confirm_execute<S: StateWrite>(state, msg);
async fn chan_close_init_execute<S: StateWrite>(state, msg);
async fn recv_packet_execute<S: StateWrite>(state, msg) -> Result<()>;
async fn timeout_packet_execute<S: StateWrite>(state, msg) -> Result<()>;
async fn acknowledge_packet_execute<S: StateWrite>(state, msg) -> Result<()>;
```

#### `AppHandler: AppHandlerCheck + AppHandlerExecute` (Satır 64)
Marker trait — ikisini birleştirir.

### Tasarım Kararları

- Check ve Execute'ın ayrı trait'ler olması: check sadece StateRead, execute StateWrite gerektirir — en az ayrıcalık prensibi
- `chan_close_confirm_execute` vs `chan_close_init_execute` dönüş tipi farkı: confirm `Result<()>` değil `()` — satır 53'te close_confirm_execute için Result yok (tutarsızlık?)

### Nova Shield Chain İçin Dersler

- ICS-20 transfer modülü bu trait'leri implemente etmeli
- Farklı uygulama modülleri için (örn. ICS-27 interchain accounts) farklı AppHandler implementasyonları yazılabilir
- Check/Execute ayrımı ile kötü durumda rollback ihtiyacı azalır

---

## [component/ibc_action_with_handler.rs]

**Dosya:** `.../ibc/src/component/ibc_action_with_handler.rs`
**Satır Sayısı:** 33

### Genel Amaç

`IbcRelayWithHandlers<AH, HI>` — type-level handler ve host interface bilgisini taşıyan wrapper.

### Struct

```rust
pub struct IbcRelayWithHandlers<AH, HI>(IbcRelay, PhantomData<AH>, PhantomData<HI>);
```

PhantomData ile generic parametreler çalışma zamanında sıfır maliyet.

### Fonksiyonlar

- `new(action)`, `action()`, `into_inner()`
- `IbcRelay::with_handler<AH, HI>(self) -> IbcRelayWithHandlers<AH, HI>` — builder style

---

## [component/action_handler/ibc_action.rs]

**Dosya:** `.../ibc/src/component/action_handler/ibc_action.rs`
**Satır Sayısı:** 128

### Genel Amaç

`IbcRelayWithHandlers` için ana dispatch noktası. `check_stateless`, `check_historical`, `check_and_execute` metodları.

### Fonksiyonlar

#### `check_stateless(&self, _context: ()) -> Result<()>` (Satır 12–38)
17 variant için dispatch — her mesaj tipinin `check_stateless::<AH>()` çağrılır. `Unknown` → bail!

#### `check_historical<S: StateRead + 'static>(state: Arc<S>) -> Result<()>` (Satır 40–48)
```rust
ensure!(
    state.get_ibc_params().await?.ibc_enabled,
    "transaction contains IBC actions, but IBC is not enabled"
);
```
Sadece IBC enabled kontrolü — minimal historical check.

#### `check_and_execute<S: StateWrite>(state: S) -> Result<()>` (Satır 50–127)
17 variant için dispatch — her mesaj tipinin `try_execute::<S, AH, HI>(state)` çağrılır. `context(...)` ile hata mesajları zenginleştirilir.

---

## [component/host_interface.rs]

**Dosya:** `.../ibc/src/component/host_interface.rs`
**Satır Sayısı:** 10

### Genel Amaç

IBC bileşeninin host zincirden ihtiyaç duyduğu bilgileri abstraksiyon üzerinden alması.

### Trait

```rust
pub trait HostInterface {
    async fn get_chain_id<S: StateRead>(state: S) -> anyhow::Result<String>;
    async fn get_revision_number<S: StateRead>(state: S) -> anyhow::Result<u64>;
    async fn get_block_height<S: StateRead>(state: S) -> anyhow::Result<u64>;
    async fn get_block_timestamp<S: StateRead>(state: S) -> anyhow::Result<tendermint::Time>;
}
```

### Tasarım Kararları

- `S: StateRead` generic parametresi: farklı state backend'ler kullanılabilir (production store, test mock)
- `tendermint::Time` dönüş tipi: doğrudan Tendermint tipine bağlı; abstraksiyon tam değil

### Nova Shield Chain İçin Dersler

- Bu trait Nova Shield'e adapte edilirken Tendermint bağımlılığından kurtulunabilir: `get_block_timestamp` için kendi `BlockTime` tipi kullanılabilir

---

## [component/client_counter.rs]

**Dosya:** `.../ibc/src/component/client_counter.rs`
**Satır Sayısı:** 88

### Genel Amaç

Client ve connection sayaç tipleri + ilgili yardımcı tipler.

### Struct/Enum/Type Listesi

#### `ClientCounter(pub u64)` (Satır 6)
Proto: `pb::ClientCounter { counter: u64 }`

#### `VerifiedHeights { heights: Vec<Height> }` (Satır 27)
Proto: `pb::VerifiedHeights`

#### `ClientConnections { connection_ids: Vec<ConnectionId> }` (Satır 55)
Proto: `pb::ClientConnections`
- From: connection_id.as_str().to_string() → String
- TryFrom: String.parse() → ConnectionId

---

## [component/connection_counter.rs]

**Dosya:** `.../ibc/src/component/connection_counter.rs`
**Satır Sayısı:** 26

### Genel Amaç

Connection sayaç tipi ve desteklenen versiyon sabiti.

### Struct/Const

#### `ConnectionCounter(pub u64)`
Proto: `pb::ConnectionCounter`

#### `SUPPORTED_VERSIONS: Lazy<Vec<Version>> = vec![Version::default()]`
Sadece varsayılan versiyon ("1") destekleniyor.

---

## [component/client_recovery.rs]

**Dosya:** `.../ibc/src/component/client_recovery.rs`
**Satır Sayısı:** 264

### Genel Amaç

IBC light client kurtarma (ADR-26 ilhamı). Dondurulmuş veya süre dolmuş client'ları aktif bir "yedek" client ile ikame etme.

### Trait

#### `ClientRecoveryExt: StateWrite + ConsensusStateWriteExt` (Satır 19)

##### `async fn validate_recover_client<HI>(subject_client_id, substitute_client_id) -> Result<()>` (Satır 21–91)

**7 adım:**
1. `validate_client_id_format_ics07(subject_client_id)` — regex: `^07-tendermint-\d+$` + leading zero check
2. `validate_client_id_format_ics07(substitute_client_id)` — aynı
3. Subject client state al
4. Substitute client state al
5. Subject NOT Active (Frozen veya Expired olmalı)
6. Substitute IS Active
7. `check_field_consistency(subject, substitute)` — alanlar uyumlu mu?
8. `substitute_height > subject_height`

##### `async fn recover_client<HI>(subject_client_id, substitute_client_id) -> Result<()>` (Satır 102–143)
1. `validate_recover_client` çağır
2. substitute'ın consensus state'ini subject'a yaz
3. substitute'ın client state'ini subject'a yaz

### Yardımcı Fonksiyonlar

#### `validate_client_id_format_ics07(client_id) -> Result<()>` (Satır 152–177)
```rust
let re = Regex::new(r"^07-tendermint-\d+$")
// Leading zero kontrolü: "07-tendermint-01" geçersiz
```

#### `check_field_consistency(subject, substitute) -> Result<()>` (Satır 205–257)
Eşleşmesi gereken alanlar:
- `chain_id` — aynı zincir olmalı
- `trust_level` — aynı güven seviyesi
- `unbonding_period` — aynı unbonding süresi
- `max_clock_drift` — aynı saat kayması toleransı
- `upgrade_path` — aynı upgrade yolu
- `allow_update` — aynı güncelleme izinleri

**Kasıtlı olarak eşleşmesi gerekmeyen:** `trusting_period` (satır 223–227 yorum), `latest_height`, `frozen_height`, `proof_specs`

### Tasarım Kararları

- ADR-26 ilhamı (Cosmos): subject/substitute aynı "kimliği" temsil etmeli, sadece hayat döngüsünde farklı noktalarda
- `trusting_period` kasıtlı atlanmış: yanlış yapılandırılmış client'a tolerans + ICS02 `trust_period < unbonding_period` guarantee'si dolaylı güvenlik sağlar

### Nova Shield Chain İçin Dersler

- IBC client kurtarma mekanizması governance aracılığıyla tetiklenmeli (herhangi biri başlatamamalı)
- Leading zero regex kontrolü ICS-07 standardı uyumu için kritik

---

## [component/ics02_validation.rs]

**Dosya:** `.../ibc/src/component/ics02_validation.rs`
**Satır Sayısı:** 160

### Genel Amaç

ICS-02 ile ilgili doğrulama fonksiyonları. Type URL tabanlı client tipi kontrolü ve client state doğrulaması.

### Fonksiyonlar

#### Type kontrol fonksiyonları (Satır 17–28)
```rust
pub fn is_tendermint_header_state(header: &Any) -> bool
pub fn is_tendermint_consensus_state(consensus_state: &Any) -> bool
pub fn is_tendermint_client_state(client_state: &Any) -> bool
pub fn is_tendermint_misbehavior(misbehavior: &Any) -> bool
```
Hepsi: `any.type_url.as_str() == TENDERMINT_XXX_TYPE_URL`

#### Deserialize fonksiyonları (Satır 30–81)
```rust
pub fn get_tendermint_misbehavior(misbehavior: Any) -> Result<TendermintMisbehavior>
pub fn get_tendermint_header(header: Any) -> Result<TendermintHeader>
pub fn get_tendermint_consensus_state(consensus_state: Any) -> Result<TendermintConsensusState>
pub fn get_tendermint_client_state(client_state: Any) -> Result<TendermintClientState>
```

#### `validate_penumbra_sdk_client_state(client_state: Any, chain_id: &str, current_height: u64) -> Result<()>` (Satır 85–139)

**Penumbra'nın kendi client state doğrulaması (ConnectionOpenTry/Ack'te kullanılır):**
1. frozen_height yok mu?
2. chain_id eşleşmesi
3. revision number = chain_id version
4. latest_height < current_height
5. proof_specs == IBC_PROOF_SPECS (veya legacy uyum: `prehash_key_before_comparison` olmadan da kabul)
6. `validate_trust_threshold(trust_level)`:
   - denominator != 0
   - numerator * 3 >= denominator (en az 1/3)
   - numerator <= denominator (en fazla 1)
7. unbonding_period >= trusting_period

### Tasarım Kararları

- **Legacy proof spec uyumu (satır 117–122):** `prehash_key_before_comparison` alanı olmayan eski spec'ler de kabul edilir. Bu backward compatibility.
- Trust threshold: `1/3 <= threshold <= 1` — BFT güvenliği için minimum 1/3 gerekli

---

## [component/state_key.rs]

**Dosya:** `.../ibc/src/component/state_key.rs`
**Satır Sayısı:** 32

### Genel Amaç

IBC state key'lerini merkezi olarak tanımlar.

### Fonksiyonlar / Sabitler

| Fonksiyon | Dönen Key |
|-----------|-----------|
| `ibc_params()` | `"ibc/params"` |
| `client_processed_heights(client_id, height)` | `"ibc/clients/{client_id}/processedHeights/{height}"` |
| `client_processed_times(client_id, height)` | `"ibc/clients/{client_id}/processedTimes/{height}"` |
| `counter()` | `"ibc/connection_counter"` |
| `ics20_value_balance::prefix()` | `"ibc/ics20-value-balance/"` |
| `ics20_value_balance::by_asset_id(chan, asset)` | `"ibc/ics20-value-balance/{channel_id}/{asset_id}"` |

**Not:** `client_processed_heights` ve `client_processed_times` IBC spec'i değil, Penumbra'ya özgü; delay_period doğrulaması için.

---

## [component/view.rs]

**Dosya:** `.../ibc/src/component/view.rs`
**Satır Sayısı:** 30

### Genel Amaç

IBC parametrelerini okuma/yazma.

### Trait Implementasyonları

#### `StateWriteExt: StateWrite`
`put_ibc_params(params: IBCParameters)` → `"ibc/params"` key

#### `StateReadExt: StateRead`
`get_ibc_params() -> Result<IBCParameters>` → Missing ise hata

---

## [component/metrics.rs]

**Dosya:** `.../ibc/src/component/metrics.rs`
**Satır Sayısı:** 27

### Genel Amaç

Metrics modülü. Şu an metrik yok (`register_metrics()` boş fonksiyon). Bir TODO niteliğinde.

### Nova Shield Chain İçin Dersler

- IBC bileşeni için temel metrikler eklenmeli: client update sayısı, packet recv/ack/timeout sayıları, connection/channel sayısı

---

## [component/rpc.rs]

**Dosya:** `.../ibc/src/component/rpc.rs`
**Satır Sayısı:** 33

### Genel Amaç

IBC gRPC sorgu endpoint'lerini organize eder.

### Struct

#### `IbcQuery<HI: HostInterface>` (Satır 16–18)
```rust
pub struct IbcQuery<HI: HostInterface> {
    storage: cnidarium::Storage,
    _marker: PhantomData<HI>,
}
```

### Tasarım Kararları

- `pub fn routes(_storage: Storage) -> Routes { unimplemented!("...") }` — routes() tonic 0.10 bekliyor; mevcut sürümde implemente edilmemiş

---

## [component/rpc/client_query.rs]

**Dosya:** `.../ibc/src/component/rpc/client_query.rs`
**Satır Sayısı:** 311

### Genel Amaç

ICS-02 client sorgularını implement eder (`ClientQuery` gRPC trait'i).

### Implemente Edilen Endpoint'ler

| Endpoint | Açıklama |
|----------|---------|
| `client_state` | Tek client state + Merkle proof |
| `client_states` | Tüm client'lar (counter'a göre 0..n döner) |
| `consensus_state` | Belirli height'ta consensus state + proof |
| `consensus_states` | Tüm consensus state'ler (VerifiedHeights listesinden) |
| `consensus_state_heights` | Tüm doğrulanmış yükseklikler |
| `client_status` | Active/Frozen/Expired/Unknown |

### İmplemente Edilmemiş

- `client_params` → `Status::unimplemented`
- `upgraded_client_state` → `Status::unimplemented`
- `upgraded_consensus_state` → `Status::unimplemented`

### Önemli Detaylar

- `client_states`: Sadece `07-tendermint-{0..counter}` formatındaki clientlar aranır — hardcoded prefix
- `determine_snapshot_from_metadata`: height header'a göre historical snapshot açılır
- `proof_height = current_height + 1`: proof'un bir sonraki blok'a ait olduğu gösterilir (JMT implementasyon detayı)

---

## [component/rpc/connection_query.rs]

**Dosya:** `.../ibc/src/component/rpc/connection_query.rs`
**Satır Sayısı:** 319

### Genel Amaç

ICS-03 connection sorgularını implement eder.

### Implemente Edilen Endpoint'ler

| Endpoint | Açıklama |
|----------|---------|
| `connection` | Tek connection + Merkle proof |
| `connections` | Tüm connections (counter'a göre "connection-{0..n}") |
| `client_connections` | Client için connection listesi + proof |
| `connection_client_state` | Connection üzerinden client state + proof |
| `connection_consensus_state` | Connection üzerinden consensus state + proof |

### İmplemente Edilmemiş

- `connection_params` → `Status::unimplemented`

---

## [component/rpc/consensus_query.rs]

**Dosya:** `.../ibc/src/component/rpc/consensus_query.rs`
**Satır Sayısı:** 878

### Genel Amaç

ICS-04 channel ve packet sorgularını implement eder (`ConsensusQuery` gRPC trait'i — isimlendirme yanıltıcı).

### Implemente Edilen Endpoint'ler

| Endpoint | Açıklama |
|----------|---------|
| `channel` | Tek channel + Merkle proof |
| `channels` | Tüm channel'lar (sadece `transfer` port'u!) |
| `connection_channels` | Bağlantı bazlı channel listesi |
| `channel_client_state` | Channel → connection → client state + proof |
| `channel_consensus_state` | Channel → connection → consensus state + proof |
| `packet_commitment` | Tek packet commitment + proof |
| `packet_commitments` | Tüm commitments (send_sequence'ye kadar) |
| `packet_receipt` | Receipt var mı? + proof |
| `packet_acknowledgement` | Tek ack hash + proof |
| `packet_acknowledgements` | Seçici ack listesi |
| `unreceived_packets` | Alınmamış packet'ların sequence listesi |
| `unreceived_acks` | Ack bekleyen packet'lar (commitment hâlâ var) |
| `next_sequence_receive` | Sonraki alım sequence'i + proof |
| `next_sequence_send` | Sonraki gönderim sequence'i + proof |

### İmplemente Edilmemiş

- `channel_params`, `upgrade`, `upgrade_error` → `Status::unimplemented`

### Kritik Notlar

- `channels` endpoint: **sadece `transfer` portundan** channel'lara bakıyor (satır 121): `get_channel(&chan_id, &PortId::transfer())`. Multi-port desteği yok.
- `packet_commitments`: sequence 1'den başlar (satır 476)
- `unreceived_packets`: sequence 0 geçersiz (satır 680–685)

---

## [component/rpc/utils.rs]

**Dosya:** `.../ibc/src/component/rpc/utils.rs`
**Satır Sayısı:** 163

### Genel Amaç

RPC height header'ını parse eden yardımcı fonksiyon.

### Fonksiyonlar

#### `determine_snapshot_from_metadata(storage, metadata) -> Result<Snapshot>` (Satır 17–33)
- height=0 → `storage.latest_snapshot()`
- height>0 → `storage.snapshot(revision_height)`

#### `TheHeight: FromStr` (Satır 54–94)
Format: `"0"` → zero, `"0-0"` → zero, `"1-2"` → `{ revision_number: 1, revision_height: 2 }`

### Testler (Satır 107–163)

```rust
assert_ibc_height_is_parsed_correctly("0", zero());
assert_ibc_height_is_parsed_correctly("0-0", zero());
assert_ibc_height_is_parsed_correctly("0-1", height(0, 1));
assert_ibc_height_is_parsed_correctly("1-0", height(1, 0));
assert_ibc_height_is_parsed_correctly("1-1", height(1, 1));
```

---

## Genel Mimari Özeti

### IBC Mesaj İşleme Pipeline

```
Transaction
  └─ IbcRelay (enum, 17 variant)
       └─ IbcRelayWithHandlers<AH, HI>
            ├─ check_stateless() → per-msg check
            ├─ check_historical() → ibc_enabled check
            └─ check_and_execute()
                 └─ MsgHandler::try_execute<S, AH, HI>()
                      ├─ Validation
                      ├─ Proof Verification (proof_verification.rs)
                      ├─ State Write (client/connection/channel/packet.rs)
                      └─ AppHandler callbacks
```

### Proof Doğrulama Zinciri (Packet örneği)

```
verify_packet_recv_proof
  └─ get_trusted_client_and_consensus_state
       ├─ get_client_state (frozen check)
       ├─ get_verified_consensus_state (at proof_height)
       ├─ verify_height
       └─ verify_delay_passed (processed_time, processed_height)
  └─ commit_packet (hash hesapla)
  └─ verify_merkle_proof
       └─ proof.verify_membership(ICS23)
```

### State Key Hiyerarşisi

```
ibc/params                                          → IBCParameters
ibc/connection_counter                              → ConnectionCounter
ibc_client_counter                                  → ClientCounter (prefix yok!)
ibc_channel_counter                                 → u64 (prefix yok!)

ibc-data/{ClientStatePath}                          → TendermintClientState
ibc-data/{ClientConsensusStatePath}                 → TendermintConsensusState
ibc-data/{ClientTypePath}                           → "07-tendermint" (string)
ibc-data/{ClientConnectionPath}                     → ClientPaths
ibc-data/{ConnectionPath}                           → ConnectionEnd
ibc-data/{ChannelEndPath}                           → ChannelEnd
ibc-data/{SeqSendPath}                              → u64 (big-endian raw)
ibc-data/{SeqRecvPath}                              → u64 (big-endian raw)
ibc-data/{SeqAckPath}                               → u64 (big-endian raw)
ibc-data/{CommitmentPath}                           → SHA256(packet hash)
ibc-data/{ReceiptPath}                              → "1"
ibc-data/{AckPath}                                  → SHA256(ack)

penumbra_verified_heights/{client_id}/verified_heights → VerifiedHeights
penumbra_consensus_states/{height}                  → TendermintConsensusState
ibc/clients/{client_id}/processedHeights/{height}   → Penumbra block height
ibc/clients/{client_id}/processedTimes/{height}     → nanoseconds timestamp
ibc/ics20-value-balance/{channel_id}/{asset_id}     → (başka modülde kullanılır)
```

---

## Güvenlik Analizi

### Kuvvetli Noktalar

1. **Equivocation tespiti:** `next_tendermint_state` timestamp monotonicity ve hash çakışma kontrolü
2. **Duplicate update no-op:** İki relay'ın aynı header'ı göndermesi durumunda hata değil no-op
3. **Type-state pattern:** `IBCPacket<Checked>` compile-time güvenliği
4. **Delay period doğrulama:** Relay saldırılarına karşı `verify_delay_passed`
5. **Non-membership proof:** Timeout unordered — receipt yokluğunun Merkle ile ispatı
6. **Trust threshold validation:** `1/3 <= threshold <= 1` hard constraint
7. **`deny(clippy::unwrap_used)`:** Tüm crate'te `unwrap()` yasak

### Potansiyel Zayıf Noktalar

1. **Channel close yetkisizlik (satır 33 kanal close_init, satır 37 close_confirm):** Herhangi biri `transfer` portu üzerinden kanal kapatabilir — capability authentication yok
2. **Sadece transfer port'u:** Multi-port desteği yok — ICS-27 gibi diğer IBC uygulamaları eklenemez kolayca
3. **Hardcoded `max_time_per_block = 20s`:** Chain parametresi olmalı (delay_period doğrulaması)
4. **`VerifiedHeights` her sorguda sort:** O(n log n) — çok fazla height birikirse yavaşlar
5. **`channels` endpoint sadece `transfer`:** RPC katmanındaki kısıtlama gizlidir — çok portlu sistem için breaking
6. **Legacy proof spec uyumu:** `prehash_key_before_comparison` olmadan kabul — güvenlik downgrade mı?

---

## Nova Shield Chain İçin Ana Dersler

### 1. IBC Proof Spec (KRİTİK)
JMT kullanılacaksa:
```
LEAF_DOMAIN_SEPARATOR = b"JMT::LeafNode"
INTERNAL_DOMAIN_SEPARATOR = b"JMT::IntrnalNode"  // yazım hatasıyla!
SPARSE_MERKLE_PLACEHOLDER_HASH = b"SPARSE_MERKLE_PLACEHOLDER_HASH__"
IBC_PROOF_SPECS = [ics23_spec(), ics23_spec()]  // 2 katman
```

### 2. `begin_block` Zorunlu Hook
Her block başında kendi consensus state'ini kendi store'una yaz:
```rust
state.put_penumbra_sdk_consensus_state(height, TendermintConsensusState::new(
    MerkleRoot { hash: app_hash },
    block_time,
    next_validators_hash,
));
```
Bu olmadan ConnectionOpenTry/Ack'teki consensus state proof'u doğrulanamaz.

### 3. Packet Commitment Hash
ibc-go ile uyumlu olmak zorunlu:
```
SHA256(
  timeout_timestamp_be_bytes (8) ||
  timeout_height.revision_number_be (8) ||
  timeout_height.revision_height_be (8) ||
  SHA256(packet.data)
)
```

### 4. Sequence Sayaçları Big-Endian Raw Bytes
Proto encoding kullanılmamalı — counterparty chain Merkle proof doğrulaması için.

### 5. Delay Period & Processed Time/Height
Her consensus state güncellemesinde `processed_time` ve `processed_height` kaydedilmeli. `max_time_per_block` chain parametresi olmalı.

### 6. AppHandler Interface
ICS-20 ve gelecekteki uygulamalar (ICS-27, ICS-29) için `AppHandlerCheck + AppHandlerExecute` pattern'ı kullanılmalı. Port tabanlı routing yapılmalı (Penumbra'da eksik).

### 7. Client Recovery Mekanizması
Governance aracılığıyla client recovery desteklenmeli. `check_field_consistency` doğrulaması kritik; `chain_id`, `trust_level`, `unbonding_period` değişmemeli.

### 8. Güvenlik Geliştirmeleri
- Channel close'da capability authentication ekle
- Multi-port desteği baştan planla
- `max_time_per_block` parametrize et
- `VerifiedHeights` için sorted set kullan

### 9. ICS-23 Absence Proof
Unordered channel timeout için non-membership proof desteklenmeli. Nova Shield'in Merkle tree implementasyonu bu özelliği sağlamalı.

### 10. Metrics
IBC metrikleri baştan ekle:
- `ibc_client_updates_total`
- `ibc_packets_sent/received/acknowledged/timeout_total`
- `ibc_connection_opens/closes_total`
- `ibc_channel_opens/closes_total`

---

## Özet Tablo

| Kategori | Penumbra Değeri | Nova Shield Önerisi |
|----------|-----------------|---------------------|
| Light Client | Sadece 07-tendermint | Başlangıçta aynı, sonra genişlet |
| IBC Substore | `"ibc-data"` | Aynı |
| Proof Spec | JMT double-layer ICS23 | Aynı (JMT kullanılıyorsa) |
| Sequence Storage | Big-endian raw bytes | Aynı zorunluluk |
| max_time_per_block | 20s hardcoded | Parametrize et |
| Port Support | Sadece `transfer` | Multi-port baştan |
| Channel Auth | YOK | Capability authentication ekle |
| Client Recovery | ADR-26 ilhamlı | Governance'a bağla |
| Packet Commit Hash | ibc-go uyumlu SHA256 | Birebir kopyala |
| Metrics | Boş | İlk günden ekle |
