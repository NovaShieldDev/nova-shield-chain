# Penumbra Proto Crate Analizi — Agent Proto

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `/research/chains/penumbra/crates/proto/` — tum .rs dosyalari (88 dosya)
**Toplam Satir:** ~25.000+ satir incelendi

---

## YAPI OZETI

Penumbra'nin `penumbra-proto` crate'i, blokchainin tum wire-format mesajlarini tek bir yerde toplar. Mimarisi:

```
encoded bytes <--protobuf wire--> proto types <--TryFrom/Into validation--> domain types (diger crate'ler)
```

Crate **uretilmis (generated) kod + elle yazilmis glue kodu** olarak ikiye ayrilir:
- `src/gen/*.rs` — prost-build tarafindan olusturulan protobuf tipleri
- `src/lib.rs`, `src/protobuf.rs`, `src/event.rs`, `src/state/*.rs`, vb. — elle yazilmis, kritik abstraction katmani

---

## DOSYA ANALIZLERI

---

## [build.rs]
**Dosya:** `/crates/proto/build.rs` **Satir Sayisi:** 47

### Genel Amac
Build script, eski bir LFS (Large File Storage) pointer kontrolunu disable etmistir. Orijinal amac: `src/gen/proto_descriptor.bin.no_lfs` dosyasinin LFS pointer mi yoksa gercek binary mi oldugunu dogrulamakti (500 byte esik). Su an tum kod comment icerisinde, sadece `Ok(())` donuyor.

### Tasarim Kararlari
- gRPC server reflection icin `proto_descriptor.bin` dosyasi gerekliydi; git-lfs kurulu olmayan ortamlarda build hata verir, bu yuzden kontrol gecilik olarak devre disi birakildi.
- Nova Shield icin: ayni sorun yasanabilir; build.rs'de descriptor dosyasi kontrolu ve acik hata mesajlari kullanilmali.

### Nova Shield Chain Icin Dersler
- Proto descriptor dosyasi icin build.rs kontrolu iyi bir pratik: buyuk binary dosyalari git-lfs ile yonetilmeli ve build zamaninda dogrulanmali.

---

## [src/lib.rs]
**Dosya:** `/crates/proto/src/lib.rs` **Satir Sayisi:** 347

### Genel Amac
Tum proto modullerini birlestiren kok modul. Tum uretilmis `.rs` dosyalari `include!()` makrosuyla dahil edilir — bu, her ikisi de ayni namespace icerisinde derlenir ve alintilanabilir tip olarak sunulur.

### Yapisi

Uc katmanli modul hiyerarsisi:
1. `penumbra::core::*` — protokol tiplerinin cogu burada
2. `penumbra::crypto::*` — kriptografik primitifler
3. `penumbra::custody::*` — imzalama protokolu

**Re-exportlar (satir 26-46):**
```rust
pub use prost::{Message, Name};
pub use protobuf::DomainType;
pub use state::StateReadProto;   // cnidarium feature
pub use state::StateWriteProto;  // cnidarium feature
pub use penumbra::*;
```

**Feature bayraklari:**
- `box-grpc` — `BoxGrpcService` type erased gRPC servisi
- `cnidarium` — storage read/write trait'leri
- `rpc` — `FILE_DESCRIPTOR_SET` ve tonic client/server kodu
- `tendermint` — tendermint_compat module

**Modul listesi (tum bilesenler):**

| Proto Paketi | Rust Modulu |
|---|---|
| `penumbra.core.app.v1` | `penumbra::core::app::v1` |
| `penumbra.core.asset.v1` | `penumbra::core::asset::v1` |
| `penumbra.core.txhash.v1` | `penumbra::core::txhash::v1` |
| `penumbra.core.component.auction.v1` | `penumbra::core::component::auction::v1` |
| `penumbra.core.component.compact_block.v1` | `penumbra::core::component::compact_block::v1` |
| `penumbra.core.component.community_pool.v1` | `penumbra::core::component::community_pool::v1` |
| `penumbra.core.component.dex.v1` | `penumbra::core::component::dex::v1` |
| `penumbra.core.component.distributions.v1` | `penumbra::core::component::distributions::v1` |
| `penumbra.core.component.fee.v1` | `penumbra::core::component::fee::v1` |
| `penumbra.core.component.funding.v1` | `penumbra::core::component::funding::v1` |
| `penumbra.core.component.governance.v1` | `penumbra::core::component::governance::v1` |
| `penumbra.core.component.ibc.v1` | `penumbra::core::component::ibc::v1` |
| `penumbra.core.component.sct.v1` | `penumbra::core::component::sct::v1` |
| `penumbra.core.component.shielded_pool.v1` | `penumbra::core::component::shielded_pool::v1` |
| `penumbra.core.component.stake.v1` | `penumbra::core::component::stake::v1` |
| `penumbra.core.keys.v1` | `penumbra::core::keys::v1` |
| `penumbra.core.num.v1` | `penumbra::core::num::v1` |
| `penumbra.core.transaction.v1` | `penumbra::core::transaction::v1` |
| `penumbra.crypto.decaf377_fmd.v1` | `penumbra::crypto::decaf377_fmd::v1` |
| `penumbra.crypto.decaf377_frost.v1` | `penumbra::crypto::decaf377_frost::v1` |
| `penumbra.crypto.decaf377_rdsa.v1` | `penumbra::crypto::decaf377_rdsa::v1` |
| `penumbra.crypto.tct.v1` | `penumbra::crypto::tct::v1` |
| `penumbra.custody.threshold.v1` | `penumbra::custody::threshold::v1` |
| `penumbra.custody.v1` | `penumbra::custody::v1` |
| `penumbra.util.tendermint_proxy.v1` | `penumbra::util::tendermint_proxy::v1` |
| `penumbra.tools.summoning.v1` | `penumbra::tools::summoning::v1` |
| `penumbra.view.v1` | `penumbra::view::v1` |
| `tendermint.*` | `tendermint::*` |
| `noble.forwarding.v1` | `noble::forwarding::v1` |
| `cosmos.*` | `cosmos::*` |

**Onemli Clippy izinleri (satir 19-23):**
```rust
#![allow(clippy::derive_partial_eq_without_eq)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::unwrap_used)]
#![allow(non_snake_case)]
```
Tumu uretilmis koddan kaynaklanir. `unwrap_used` izni: proto mesajlari icin `None` olmasi gerekenler zaten bos gelir, dolayisiyla `expect` yerine `unwrap` kullanilmis.

### Nova Shield Chain Icin Dersler
- `include!()` makrosu ile uretilmis kodu elle yazilmis modullere dahil etmek cok temiz: compile check korunur, modul grafigi acik kalir.
- `pub use penumbra::*` ile kok re-export yapilmasi kullanicilari tek crate'e bagimli kilar — mimari acidan dogru secim.

---

## [src/protobuf.rs]
**Dosya:** `/crates/proto/src/protobuf.rs` **Satir Sayisi:** 209

### Genel Amac
`DomainType` trait'ini tanimlar ve tum "yabanci tip" (foreign type) implementasyonlarini icerir. Bu dosya **sistemin kalbi**dir: protobuf tipleri ile Rust domain tipleri arasindaki kopruyu kurar.

### DomainType Trait (Satir 8-36)

```rust
pub trait DomainType
where
    Self: Clone + Sized + TryFrom<Self::Proto>,
    Self::Proto: prost::Name + prost::Message + Default + From<Self> + Send + Sync + 'static,
    anyhow::Error: From<<Self as TryFrom<Self::Proto>>::Error>,
{
    type Proto;

    fn encode_to_vec(&self) -> Vec<u8>  // Self -> Proto -> bytes
    fn to_proto(&self) -> Self::Proto    // Self -> Proto (via From)
    fn decode<B: bytes::Buf>(buf: B) -> anyhow::Result<Self>  // bytes -> Proto -> Self (via TryFrom)
}
```

**Kritik kisitlar:**
- `Self: Clone` — domain tipinin kopyalanabilmesi gerekiyor (proto'ya gecerken `self.clone()` cagrilir)
- `Self::Proto: From<Self>` — infallible conversion (domain -> proto her zaman basarili)
- `Self: TryFrom<Self::Proto>` — fallible conversion (proto -> domain basarisiz olabilir, validation burada)
- `anyhow::Error: From<Error>` — hata tipi `anyhow`'a cevirilmeli

**Fonksiyon ayrinti:**
- `encode_to_vec()` (satir 17-19): `to_proto()` cagirip `prost::Message::encode_to_vec()` ile baytlara donusturur.
- `to_proto()` (satir 26-28): `Self::Proto::from(self.clone())` — bir klon maliyeti var ama API tutarli.
- `decode()` (satir 31-35): `<Proto>::decode(buf)?` + `.try_into().map_err(Into::into)` — iki adimli; once parse, sonra validate.

**Onemli tasarim karari:** `From<Self> for Proto` infallible olmali — domain tipinden proto tipine donusum hic basarisiz olmamali. Ters yon fallible: proto baytlari gecersiz olabilir.

### Foreign Type Implementasyonlari

**decaf377-rdsa imzalari (satir 50-102):**
```
Signature<SpendAuth> <-> SpendAuthSignature { inner: Vec<u8> }
Signature<Binding>   <-> BindingSignature   { inner: Vec<u8> }
VerificationKey<SpendAuth> <-> SpendVerificationKey { inner: Vec<u8> }
```
- `From` yonunde: `.to_bytes().to_vec()` ile ham byte'a donusturur.
- `TryFrom` yonunde: `value.inner.as_slice().try_into()?` — baytlarin gecerli imza olmasi gerekiyor, basarisiz olursa hata propagate edilir.

**FMD Clue (satir 106-127):**
```
decaf377_fmd::Clue <-> ProtoClue { inner: Vec<u8> }
```
- `TryFrom<ProtoClue>`: 68-byte clue bekleniyor; uzunluk farkli gelirse `anyhow!("expected 68-byte clue")`.

**ConsensusKey (satir 130-154):**
```
tendermint::PublicKey <-> ConsensusKey { inner: Vec<u8> }
```
- `TryFrom`: `Self::from_raw_ed25519(value.inner.as_slice())` — yalnizca Ed25519 anahtari destekli; `None` gelirse `anyhow!("invalid ed25519 key")`.
- Not: tendermint-rs'in kendi proto tipi mevcuttu ama encoding konsensus-kritik, bu yuzden kendi konsensus key tipi tanimlanmis.

**IBC tipleri (satir 156-208):**
```
ibc_types::core::channel::ChannelEnd       <-> ibc_proto::...::Channel
ibc_types::core::client::Height            <-> ibc_proto::...::Height
ibc_types::core::connection::ClientPaths   <-> ibc_proto::...::ClientPaths
ibc_types::core::connection::ConnectionEnd <-> ibc_proto::...::ConnectionEnd
ibc_types::ClientState                     <-> google.protobuf.Any
ibc_types::ConsensusState                  <-> google.protobuf.Any
```
IBC tipleri icin `DomainType` impl. blanket: `ibc_types::DomainType` zaten var, sadece proxy.

**IbcRelay generic impl (satir 192-208):**
```rust
impl<T> From<T> for IbcRelay
where
    T: ibc_types::DomainType + Send + Sync + 'static,
    ...
{
    fn from(v: T) -> Self {
        let value_bytes = v.encode_to_vec();
        let any = pbjson_types::Any { type_url, value: value_bytes.into() };
        Self { raw_action: Some(any) }
    }
}
```
Her IBC action tipini generic olarak `IbcRelay` mesajina sariyor. `type_url` otomatik `<T::Proto>::type_url()` ile alinir. Elegant tasarim.

### Nova Shield Chain Icin Dersler
- `DomainType` trait'i birebir kullanilabilir. Nova Shield icin de ayni pattern: `proto tip (wire) <--> domain tip (i_mantik)` ayrimini yapmak serilerileme/validasyon bug'larini onler.
- `From<Domain> for Proto` infallible olmali — bu constraint tip sistemini kullanarak yanlis yonlu donusum hatasini derleme zamaninda yakalar.
- 68-byte Clue kontrolu gibi boyut/format validasyonlari `TryFrom` implementasyonunda olmali, dis layerlarda degil.
- IBC action'lari icin generic `From<T> for IbcRelay` pattern'i: bir `Any` sarici tipiyle ayni mantigi kullanmak, action type proliferation'ini onler.

---

## [src/protobuf/tendermint_compat.rs]
**Dosya:** `/crates/proto/src/protobuf/tendermint_compat.rs` **Satir Sayisi:** 795

### Genel Amac
`tendermint-rpc` karisik API'si ile Penumbra'nin kendi proto tipleri arasindaki donusimleri saglar. Bu dosya ozellikle `penumbra-tendermint-proxy` icin gerekli.

### Tanimlanan Conversion'lar

**get_tx endpoint:**
- `tendermint_rpc::endpoint::tx::Response` -> `penumbra_sdk_pb::GetTxResponse` (satir 12-31): `hash`, `height`, `index`, `tx_result`, `tx` alanlari kopyalanir. `proof` alani kasitli olarak ignore edilir.
- `tendermint::abci::types::ExecTxResult` -> `penumbra_sdk_pb::TxResult` (satir 33-59): `gas_wanted`/`gas_used` `i64 -> u64` donusturme (TODO yorumuyla: mismatch var). Events flatten edilerek `Tag` listesi olusturulur.
- `tendermint::abci::EventAttribute` -> `penumbra_sdk_pb::Tag` (satir 62-73): `index: false` hardcoded — TODO yorumu var.

**broadcast_tx_async/sync:**
- `code`: `u32::from(code) as u64` — u32'den u64'e genisletme.
- `hash`: `.as_bytes().to_vec()`

**get_status endpoint (satir 142-265):**
- `SyncInfo` donusumu: `Timestamp` tipi `time::PrimitiveDateTime` sarar ve private oldugundan RFC3339 string round-trip gerekiyor — performans kaybi belli, ancak alternatif yok.
- `DefaultNodeInfo`: `channels` alani `to_string().as_bytes().to_vec()` — bytes degil string isteniyor.
- Validator info: Ed25519 public key ile `Sum::Ed25519` variant; diger imza tiplerinisi desteklemiyor.

**abci_query endpoint (satir 267-339):**
- `HeightOverflowError` struct (satir 268-274): `u64 height -> i64` overflow tespiti icin ozel hata tipi. `thiserror` kullanilmis.
- `TryFrom<AbciQuery>` -> `AbciQueryResponse`: Height overflow u64->i64 donemin gorulmesi onemli. `ProofOps` donusumu dahil.

**get_block_by_height endpoint (satir 341-795):**
- `TryFrom<tendermint::Block>` -> `crate::tendermint::types::Block`: Header, data, evidence, last_commit donusumleri.
- `TryFrom<tendermint::block::Header>` -> `crate::tendermint::types::Header` (satir 448-508): `tonic::Status` error tipi kullanilmis (not ideal, TODO var).
- `TryFrom<tendermint::evidence::Evidence>` -> `...Evidence` (satir 523-684): `DuplicateVoteEvidence` icin vote bilgilerinin karmasik cikartilmasi. `.ok_or(anyhow!("block id"))?` pattern'i tekrarlaniyor.
- `LightClientAttackEvidence`: Yaratici bir cozum — `tendermint_proto` tipi kullanilip encode -> decode round-trip yapilarak Penumbra proto tipine donusturuluyor.
- `CommitSig` -> proto (satir 711-769): `BlockIdFlagAbsent`, `BlockIdFlagCommit`, `BlockIdFlagNil` ucleme match. Her variant icin timestamp RFC3339 roundtrip tekrar ediyor.

### Guvenlik/Korreklik Notalari
- `i64 -> u64` donusumlerinde overflow kontrolu: `HeightOverflowError` struct'i bu icin var; iyi pratik.
- Timestamp RFC3339 round-trip: `expect("timestamp should roundtrip to string")` — production'da panic edebilir; daha iyi hata yonetimi gerekebilir.
- `index: false` hardcoded Tag icin: ABCI indexing davranisini etkiler.

### Nova Shield Chain Icin Dersler
- Tendermint/CometBFT uyumlulugu icin ayri bir "compat" modul mantikli. Tip donusimleri diger logikle karistirilmamali.
- `i64 <-> u64` donusumlerinde explicit overflow kontrolu zorunlu; `as u64` donusumu yerine `try_into().map_err(...)` kullanilmali.
- Timestamp tipleri arasindaki donusumler icin string round-trip son care olmali; dogrudan UNIX timestamp + nanosaniye ile calis.

---

## [src/state.rs]
**Dosya:** `/crates/proto/src/state.rs` **Satir Sayisi:** 8

### Genel Amac
State mod modullerini re-export eden kucuk bir router. `StateReadProto` ve `StateWriteProto`'yu dis dunyaya acar.

---

## [src/state/read.rs]
**Dosya:** `/crates/proto/src/state/read.rs` **Satir Sayisi:** 159

### Genel Amac
`StateReadProto` trait'ini tanimlar. `cnidarium::StateRead` extends ederek domain/proto tiplerini storage'dan okuyabilmeyi saglar.

### Trait Tanimlamalari

```rust
pub trait StateReadProto: StateRead + Send + Sync {
    fn get<D>(&self, key: &str) -> DomainFuture<D, Self::GetRawFut>
    fn nonverifiable_get<D>(&self, key: &[u8]) -> DomainFuture<D, Self::GetRawFut>
    fn get_proto<P>(&self, key: &str) -> ProtoFuture<P, Self::GetRawFut>
    fn nonverifiable_get_proto<P>(&self, key: &[u8]) -> ProtoFuture<P, Self::GetRawFut>
    fn prefix<'a, D>(&'a self, prefix: &'a str) -> Pin<Box<dyn Stream<...>>>
    fn nonverifiable_prefix<'a, D>(&'a self, prefix: &[u8]) -> Pin<Box<dyn Stream<...>>>
    fn prefix_proto<'a, P>(&'a self, prefix: &'a str) -> Pin<Box<dyn Stream<...>>>
    fn nonverifiable_prefix_proto<'a, P>(&'a self, prefix: &'a [u8]) -> Pin<Box<dyn Stream<...>>>
}
impl<T: StateRead + ?Sized> StateReadProto for T {}  // blanket impl
```

**Iki depolama katmani:**
- Verifiable (Merkle tree dahil, consensus-critical): `get`, `prefix`, `prefix_proto`
- Nonverifiable (sadece verimlilik, Merkle dahil degil): `nonverifiable_get`, `nonverifiable_prefix`, `nonverifiable_prefix_proto`

**Iki tip okuma:**
- `get<D>` / `nonverifiable_get<D>`: Domain tipi dondurur (`DomainFuture`) — bytes -> proto -> domain
- `get_proto<P>` / `nonverifiable_get_proto<P>`: Ham proto tipi dondurur (`ProtoFuture`) — bytes -> proto

**prefix fonksiyonlari (satir 83-157):** `prefix_proto` uzerinden `prefix` olusturulur — `Stream<Item=Result<(String, P)>>` -> `Stream<Item=Result<(String, D)>>` donusumu. `StreamExt::map` kullanilmis.

**Donusum akisi (satir 91-97):**
```rust
Box::pin(self.prefix_proto(prefix).map(|p| match p {
    Ok(p) => match D::try_from(p.1) {
        Ok(d) => Ok((p.0, d)),
        Err(e) => Err(e.into()),
    },
    Err(e) => Err(e),
}))
```

**Blanket impl (satir 159):** `impl<T: StateRead + ?Sized> StateReadProto for T {}` — yani `StateRead`'i implemeneted eden her tip otomatik olarak `StateReadProto`'yu da kazanir.

### Nova Shield Chain Icin Dersler
- Bu pattern aynen kullanilmali: storage trait'ini domain/proto awareness ile extend etmek, ust katman kodunu temiz tutar.
- Verifiable/nonverifiable ayirimi kritik: consensus-critical state Merkle tree'de, gecici/cache state nonverifiable'da tutulmali.
- `DomainFuture` ve `ProtoFuture` tip parametreli future'lar generic kod yazmak icin ideal.
- Blanket impl pattern: trait bound'lari karsilayan her tipe otomatik davranis kazandirmak, bazi durumlarda `impl Trait for dyn OtherTrait` yazmaktan daha iyidir.

---

## [src/state/write.rs]
**Dosya:** `/crates/proto/src/state/write.rs` **Satir Sayisi:** 51

### Genel Amac
`StateWriteProto` trait'ini tanimlar. `cnidarium::StateWrite`'i domain/proto awareness ile extend eder.

### Tanimlanan Metodlar

```rust
pub trait StateWriteProto: StateWrite + Send + Sync {
    fn put<D>(&mut self, key: String, value: D)                  // domain -> proto -> raw bytes
    fn put_proto<P>(&mut self, key: String, value: P)            // proto -> raw bytes
    fn nonverifiable_put<D>(&mut self, key: Vec<u8>, value: D)  // nonverifiable, domain
    fn nonverifiable_put_proto<P>(&mut self, key: Vec<u8>, value: P)  // nonverifiable, proto
    fn record_proto<E>(&mut self, proto_event: E)                // ABCI event kayit
}
impl<T: StateWrite + ?Sized> StateWriteProto for T {}
```

**`put<D>` implementasyonu (satir 9-15):**
```rust
fn put<D>(&mut self, key: String, value: D)
where
    D: DomainType,
    anyhow::Error: From<<D as TryFrom<D::Proto>>::Error>,
{
    self.put_proto(key, D::Proto::from(value));
}
```
Domain tipi `put_proto`'ya yonlendirilir; `D::Proto::from(value)` infallible.

**`record_proto<E>` (satir 43-48):** `ProtoEvent::into_event()` cagirip ABCI event'ini kayit eder.

**Note:** Write tarafinda key tipi `String` (verifiable) vs `Vec<u8>` (nonverifiable) — asimetri read tarafiyla tutarli.

### Nova Shield Chain Icin Dersler
- Write metodlarinda key tipini String/Vec<u8> olarak ayirmak, verifiable/nonverifiable karisilmasini engeller.
- `record_proto` pattern'i: ABCI eventleri icin ayri bir `ProtoEvent` trait yerine dogrudan serde kullanmak da dusunulebilir, ancak protobuf uyumlulugu icin bu approach daha tutarli.

---

## [src/state/future.rs]
**Dosya:** `/crates/proto/src/state/future.rs` **Satir Sayisi:** 72

### Genel Amac
`ProtoFuture<P, F>` ve `DomainFuture<D, F>` future tiplerinini tanimlar. Asenkron storage okuma operasyonlari icin ara katman.

### Tipleri ve Implementasyonlari

**ProtoFuture (satir 15-46):**
```rust
pub struct ProtoFuture<P, F> {
    inner: F,   // gerçek async storage future
    _marker: PhantomData<P>,
}
```
`Future for ProtoFuture<P, F>` implementasyonu:
- `inner.poll()` -> `Ok(Some(bytes))` -> `P::decode(&*bytes)` -> `Ok(Some(P))`
- `Ok(None)` -> `Ok(None)` (anahtar bulunamadi)
- `Err(e)` -> `Err(e)` (storage hatasi)

**DomainFuture (satir 49-71):**
```rust
pub struct DomainFuture<D, F> {
    inner: F,
    _marker: PhantomData<D>,
}
```
`Future for DomainFuture<D, F>` implementasyonu:
- bytes -> `D::Proto::decode()` -> `D::try_from()` — iki adimli donusum
- Her adim hata dondurebilir; `.context("could not decode proto from bytes")` ve `.context("could not parse domain type from proto")` ile anlamli hata mesajlari eklenir.

**pin_project kullanimi:** `#[pin_project]` ile pin projeksiyonu — unsafe olmadan `Pin<&mut Self>` manipulasyonu saglar.

### Nova Shield Chain Icin Dersler
- `PhantomData<T>` ile tip parametreli future wrapping: zero-cost abstraction, runtime maliyeti yok.
- `pin_project` crate'i async/await ile calisan struct'lar icin zorunlu; biz de kullanmaliyiz.
- Hata mesajlarina `.context()` ekleme: debugging icin kritik. "could not decode" vs "could not parse" ayrimi hangi adimda hata olustu gosterir.

---

## [src/event.rs]
**Dosya:** `/crates/proto/src/event.rs` **Satir Sayisi:** 149

### Genel Amac
`ProtoEvent` ve `EventDomainType` trait'lerini tanimlar. Protobuf mesajlarini ABCI event'lerine ve geri donusturmek icin sistem.

### ProtoEvent Trait (Satir 7-63)

```rust
pub trait ProtoEvent: Message + Name + Serialize + DeserializeOwned + Sized {
    fn into_event(&self) -> abci::Event
    fn from_event(event: &abci::Event) -> anyhow::Result<Self>
}
impl<E: Message + Name + Serialize + DeserializeOwned + Sized> ProtoEvent for E {}
```

**`into_event` (satir 8-34):**
1. `Self::full_name()` ile event kind belirlenir (ornek: `"penumbra.core.component.shielded_pool.v1.EventSpend"`)
2. `serde_json::to_value(&self)` ile JSON nesnesine seri edilir
3. JSON nesnesinin her alanini `EventAttribute` olarak listeler
4. Attribute'lar **alfabetik siraya gore siralanir** (cosmos-sdk davranisini taklit)
5. `abci::Event::new(kind, attributes)` ile olusturulur

**Sira kritik (satir 30-31):**
```rust
attributes.sort_by(|a, b| (&a.key_bytes()).cmp(&b.key_bytes()));
```
Determinizm icin zorunlu. Cosmos-SDK referansi verilmis.

**`from_event` (satir 36-60):**
1. Event kind kontrolu: `Self::full_name() != event.kind` ise hata
2. Attribute'lari `HashMap<String, serde_json::Value>` haritasina toplar
3. JSON'a cevirip `serde_json::from_value` ile parse eder

**Test (satir 67-125):**
- `EventSpend` -> ABCI event -> `EventSpend` round-trip testi
- Base64 encoding kontrolu: `inner` alani `"lL6VF1ZxmJFo8o6i6e+JjYyktGKaN6j/o+SzsBoZ29M="` seklinde seri edilir
- `noteCommitment` -> camelCase: ProtoJSON konvansiyonu, snake_case -> camelCase donusumu
- Attribute value'lar JSON string olarak: `{"inner":"..."}`

**EventDomainType Trait (satir 133-148):**
```rust
pub trait EventDomainType: DomainType
where
    <Self as DomainType>::Proto: ProtoEvent,
    ...
{
    fn try_from_event(event: &abci::Event) -> anyhow::Result<Self>
}
```
Domain tipleri icin: `abci::Event` -> `Proto::from_event` -> `Self::try_from`. Olaylar izleme kodunda kullanisli.

### Kritik Tasarim Notu
ProtoEvent JSON tabanlicalisar: bu, ABCI event icerigi ile servis icin JSON API uretilmesini birlestirir. Ancak `serde_json` ile encode edilen `bytes` alanlari base64 string olarak gelir (ProtoJSON standardina gore) — bu beklenen davranistir.

### Nova Shield Chain Icin Dersler
- ABCI event tipleri icin protobuf kullanmak ideal: JSON seri edilebilirlik ile tam type safety birlikte saglanir.
- Event attribute siralamasi **deterministik olmali** — aksi halde event hash'i farkli node'larda farklilasabilir.
- `try_from_event` pattern: event izleme / indexer kodunda kullanilmali; ham ABCI event string parsing yerine tip guvenli dönüsüm.

---

## [src/serializers.rs]
**Dosya:** `/crates/proto/src/serializers.rs` **Satir Sayisi:** 6

Sadece `pub mod bech32str;` re-export. TODO: ileride kaldirilacak.

---

## [src/serializers/bech32str.rs]
**Dosya:** `/crates/proto/src/serializers/bech32str.rs` **Satir Sayisi:** 295

### Genel Amac
Bech32m formatinda adres ve anahtar encode/decode fonksiyonlari. Her tip icin HRP (human-readable prefix) ve variant belirlenmis.

### Genel Fonksiyonlar

**`decode(string, expected_hrp, expected_variant)` (satir 14-37):**
```rust
pub fn decode(string: &str, expected_hrp: &str, expected_variant: Variant) -> anyhow::Result<Vec<u8>>
```
- `bech32::decode(string)` ile cozulur
- variant kontrol edilir
- HRP kontrol edilir
- `Vec::from_base32(&data)` ile veri alinir

**`encode(data, hrp, variant)` (satir 43-45):**
```rust
pub fn encode(data: &[u8], hrp: &str, variant: Variant) -> String
```
- Geçersiz HRP halinde panic (`.expect("HRP should be valid")`) — gelistirici hatasi

### Tanimli Bech32 Prefix Sabitleri

| Modul | `BECH32_PREFIX` | Variant |
|---|---|---|
| `validator_identity_key` | `"penumbravalid"` | Bech32m |
| `validator_governance_key` | `"penumbragovern"` | Bech32m |
| `address` | `"penumbra"` | Bech32m |
| `compat_address` | `"penumbracompat1"` | Bech32 (eski format!) |
| `asset_id` | `"passet"` | Bech32m |
| `full_viewing_key` | `"penumbrafullviewingkey"` | Bech32m |
| `wallet_id` | `"penumbrawalletid"` | Bech32m |
| `spend_key` | `"penumbraspendkey"` | Bech32m |
| `lp_id` | `"plpid"` | Bech32m |
| `auction_id` | `"pauctid"` | Bech32m |

**Onemli:** `compat_address` modulu eski Bech32 (m-siz) variantini kullanir — geriye donuk uyumluluk icin.

### Nova Shield Chain Icin Dersler
- HRP'leri sabit olarak tanimlamak gerekli; kullanici girdisinde HRP dogrulamak anahtar guvenlik tedbiridir.
- Bech32m (BIP-350) tercih edilmeli — Bech32 (BIP-173) sadece uyumluluk icin.
- Farkli tip'ler icin farkli prefix kullanmak yanlis tip kullanimi hatasini kullanici katmaninda yakalar.
- LP ID'si (`plpid`), auction ID (`pauctid`) vb. tiple-ozgu prefix'ler Nova Shield DEX'te de kullanilmali.

---

## [src/box_grpc_svc.rs]
**Dosya:** `/crates/proto/src/box_grpc_svc.rs` **Satir Sayisi:** 53

### Genel Amac
Type-erased gRPC servis wrapping utility'si. `box-grpc` feature bayragi ile aktif olur.

### Tanimlanan Tipler

```rust
pub type BoxGrpcService = BoxCloneService<
    grpc::Request<ReqBody>,
    grpc::Response<RspBody>,
    BoxError
>;
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type RspBody = UnsyncBoxBody<Bytes, BoxError>;
```

### Fonksiyonlar

**`connect(ep: Endpoint) -> anyhow::Result<BoxGrpcService>` (satir 17-24):**
- Uzak endpoint'e baglanir
- `ServiceBuilder::new().map_response(...).map_err(...).service(conn)` ile tip silme yapar
- `BoxCloneService::new(svc)` ile donduruluyor — `Clone` kabiliyeti dahil

**`local<S, B>(svc: S) -> BoxGrpcService` (satir 28-42):**
- In-process (local) servisi tip siler
- `BoxCloneService::new(svc)` ile donduruluyor

**`box_rsp_body<B>(body: B) -> RspBody` (satir 44-51):**
- `body.map_err(BoxError::from).boxed_unsync()` ile response body'yi BoxError ile sigiyor
- `UnsyncBoxBody` kullanilmis — `Send` gerektirir ama `Sync` gerekmez

### Neden `UnsyncBoxBody`?
gRPC response body'leri async context'te akis (stream) olarak kullanilir ve `Sync` gerektirmez. `Unsync` varyant bazi performans iyilestirmeleri saglar.

### Nova Shield Chain Icin Dersler
- Birden fazla gRPC servisini kompoze etmek gerektiginde `BoxGrpcService` pattern'i kullanilmali.
- `tower::ServiceBuilder` ile middleware (middleware: auth, rate-limit, retry) eklenmesi hizali.
- In-process ve remote servis ayrimini interface'de yapma — her ikisi de `BoxGrpcService` tipinde.

---

## GEN DOSYALARI ANALIZI

---

## [gen/penumbra.core.num.v1.rs]
**Dosya:** `.../gen/penumbra.core.num.v1.rs` **Satir Sayisi:** 24

### Genel Amac
128-bit unsigned integer icin `Amount` tipi. Blockchain'de token miktari temsili.

### Tanimlanan Tip: `Amount`
```rust
pub struct Amount {
    pub lo: u64,  // tag=1: dusuk 64 bit
    pub hi: u64,  // tag=2: yuksek 64 bit
}
```

**Kritik:** Protobuf'ta `uint128` tipi yoktur. Bu yuzden `lo`/`hi` parcalamasina gidilmis. `Amount` protobuf'ta 2 alan, domain katmaninda birlestirilir.

**Kullanim:** `lo | (hi << 64)` ile gercek u128 alinir.

**`prost::Name` impl:** `full_name() -> "penumbra.core.num.v1.Amount"`, `type_url() -> "/penumbra.core.num.v1.Amount"` — `Any` sarici ile kullanim icin zorunlu.

### DEX Uygulama Notu (Nova Shield)
`Amount` yapisi DEX rezervleri, swap miktarlari, fee'ler icin her yerde kullanilir. Nova Shield'de:
- u128 yerine `Amount { lo, hi }` kullanmak wire-format uyumlulugu saglar
- IBC araciligiyla gelen `Amount` mesajlarini dogru parse etmek icin `lo | (hi << 64)` dogrulmasi gerekli
- Overflow riski: `lo` ve `hi`'nin birlesimi `u128` sinirini asmayi matematiksel olarak imkansiz kilar, ancak domain katmaninda boyut kontrolu eklenebilir

---

## [gen/penumbra.core.asset.v1.rs]
**Dosya:** `.../gen/penumbra.core.asset.v1.rs` **Satir Sayisi:** 375

### Genel Amac
Varlik (asset) yonetimi icin temel protobuf tipleri.

### Tanimlanan Tipler

**`AssetId` (satir 18-50):**
```rust
pub struct AssetId {
    pub inner: Vec<u8>,        // tag=1: ham bytes, tercih edilen
    pub alt_bech32m: String,   // tag=2: Bech32m string alternati (isteğe bağlı)
    pub alt_base_denom: String, // tag=3: denom string alternati (isteğe bağlı)
}
```
Kural: `inner` VE `alt_bech32m`/`alt_base_denom` ayni anda set edilemez. Sadece biri kullanilmali. Bu "convenience for RPC users" mimarisidir — RPC cagranlari insan-okunabilir string gonderebilir.

**`Denom` (satir 52-65):** Sadece `denom: String`. Temel denomination.

**`Metadata` (satir 67-115):** Zengin token metadata:
- `description`, `denom_units: Vec<DenomUnit>`, `base`, `display`, `name`, `symbol`
- `penumbra_asset_id: Option<AssetId>` — **tag=1984**, bilerek yuksek field numarasi (Cosmos uzantisi olarak)
- `images: Vec<AssetImage>` — tag=1985
- `priority_score: u64` — tag=1986 (istemci tarafli token listesi siralama)
- `badges: Vec<AssetImage>` — tag=1987 (IBC varliklar icin kaynak chain ikonlari)
- `coingecko_id: String` — tag=1988

**`DenomUnit` (satir 117-142):** `denom: String`, `exponent: u32`, `aliases: Vec<String>`. `exponent`: `1 token = 10^exponent base_denom`.

**`Value` (satir 143-159):** `amount: Option<Amount>` + `asset_id: Option<AssetId>`. En temel "degerli varlik" tipi.

**`Balance` (satir 160-195):** `values: Vec<SignedValue>` ile signed balance vektoru.
- `SignedValue { value: Option<Value>, negated: bool }` — boolean negation ile hem pozitif hem negatif bakiye.

**`ValueView` (satir 197-273):** Oneof iki varyant:
- `KnownAssetId { amount, metadata, equivalent_values, extended_metadata }` — varlik biliniyorsa tam metadata
- `UnknownAssetId { amount, asset_id }` — bilinmiyorsa sadece ham ID
Bu "view" pattern — hangi varligin ne kadar degerinde oldugunu gosterir.

**`EstimatedPrice` (satir 323-346):** `priced_asset`, `numeraire`, `numeraire_per_unit: f64`, `as_of_height`. Fiyat f64 — yaklasimsaldir.

**`EquivalentValue` (satir 354-374):** Baska bir numeraire cinsinden deger. `as_of_height` ile fiyatin ne kadar guncel oldugu belli olur.

### DEX Uygulama Notu (Nova Shield)
- `Balance` tipi islem denge dogrulamasi icin: her `Action`'in bakiyesi toplanir, net balance sifir olmali (ZK commitment).
- `ValueView` tipi kullanici arayzunde gosterim icin; DEX UI hangi varligin ne kadar oldugunu gostermek istediginde.
- `equivalent_values` alani: DEX uygulama notasi — bir tokenin birden fazla numeraire cinsinden degerini UI'da gostermek icin kullanici deneyimini iyilestirir.
- `extended_metadata: Option<Any>` — LP NFT'ler icin position bilgisi embed etmek.

---

## [gen/penumbra.core.keys.v1.rs]
**Dosya:** `.../gen/penumbra.core.keys.v1.rs` **Satir Sayisi:** 245

### Genel Amac
Penumbra'nin anahtar hiyerarsisi icin protobuf tipleri.

### Tanimlanan Tipler

**`Address` (satir 4-19):** 80-byte Penumbra adresi.
```rust
pub struct Address {
    pub inner: Vec<u8>,       // 80 byte, tercih edilen
    pub alt_bech32m: String,  // convenience, HRP: "penumbrav2t" -> not: bech32str.rs'te "penumbra"
}
```

**`AddressView` (satir 30-90):** Oneof: `Decoded { address, index, wallet_id }` veya `Opaque { address }`.
- `Decoded`: view servisi tarafindan acilabilinmis; `AddressIndex` ve `WalletId` gozukuyor.
- `Opaque`: ham adres, yapisi cozulemez.

**`AddressIndex` (satir 166-182):**
```rust
pub struct AddressIndex {
    pub account: u32,      // tag=2 (tag=1 kullanilmamis)
    pub randomizer: Vec<u8>, // adres rastgeleligini saglar
}
```

**Anahtar Hiyerarsisi:**
```
SpendKey { inner: Vec<u8> }          -- tam harcama yetkisi
FullViewingKey { inner: Vec<u8> }    -- okuma yetkisi (view)
WalletId { inner: Vec<u8> }          -- cuzdan kimligi
PayloadKey { inner: Vec<u8> }        -- not sifreleme
Diversifier { inner: Vec<u8> }       -- adres cesitlendirme
```

**Validator anahtarlari:**
```
IdentityKey { ik: Vec<u8> }          -- decaf377-rdsa SpendAuth dogrulama anahtari
GovernanceKey { gk: Vec<u8> }        -- yonetim oylama anahtari
ConsensusKey { inner: Vec<u8> }      -- Ed25519 Tendermint consensus anahtari
PositionMetadataKey { inner: Vec<u8> } -- pozisyon metadata sifreleme anahtari
```

**Kritik not:** `IdentityKey` alani `ik` (diger tiplerde `inner`). Bu kucuk tutarsizlik dikkat gerektirir.

### Nova Shield Chain Icin Dersler
- Anahtar hiyerarsisi: SpendKey -> FullViewingKey -> WalletId -> Address/AddressIndex ayrimini biz de uygulamali.
- Her anahtar tipi icin ayri proto type kullanmak, tip karistirilmasini engeller.
- `AddressIndex { account, randomizer }` yapisi: bir cuzdan birden fazla adres uretebilmesi icin.
- `PositionMetadataKey` — DEX pozisyon metadata sifreleme icin ayri bir anahtar; gizlilik kritik.

---

## [gen/penumbra.core.component.dex.v1.rs] — DEX MODULU (3x HASSASIYET)
**Dosya:** `.../gen/penumbra.core.component.dex.v1.rs` **Satir Sayisi:** 3638

### Genel Amac
Penumbra DEX'inin tam protobuf tanimlari. Concentrated liquidity, batch swap, ZK kanitleri, pozisyon yonetimi, arb yurütme, liquidity tournament — hepsi bu dosyada.

### ZK Kanit Tipleri (Satir 1-33)

```rust
pub struct ZkSwapProof      { inner: Vec<u8> }   // swap ZK kaniti
pub struct ZkSwapClaimProof { inner: Vec<u8> }   // swap claim ZK kaniti
```
Ham bayt — sadece wire format. Kanit gerceklestirmesi baska crate'te.

### Swap Akis Yapisi

**`Swap` (satir 35-53):**
```rust
pub struct Swap {
    pub proof: Option<ZkSwapProof>,  // ZK kanit
    pub body: Option<SwapBody>,      // imzalanan alanlar
}
```

**`SwapBody` (satir 114-146):**
```rust
pub struct SwapBody {
    pub trading_pair: Option<TradingPair>,
    pub delta_1_i: Option<Amount>,   // varlik 1 miktari
    pub delta_2_i: Option<Amount>,   // varlik 2 miktari
    pub fee_commitment: Option<BalanceCommitment>, // shielded fee taahhüdü
    pub payload: Option<SwapPayload>, // commitment + encrypted_swap
}
```

**`SwapPayload` (satir 147-164):**
```rust
pub struct SwapPayload {
    pub commitment: Option<StateCommitment>,  // TCT'deki commitment
    pub encrypted_swap: Vec<u8>,              // sifrelenmis swap bilgisi
}
```
Kritik: `encrypted_swap` gizliligi saglar — coin miktarlari ve hedef adres sifrelenmis.

**`SwapPlaintext` (satir 166-196):**
```rust
pub struct SwapPlaintext {
    pub trading_pair: Option<TradingPair>,
    pub delta_1_i: Option<Amount>,
    pub delta_2_i: Option<Amount>,
    pub claim_fee: Option<Fee>,            // SwapClaim icin on-odeme
    pub claim_address: Option<Address>,    // ciktinin gonderilecegi adres
    pub rseed: Vec<u8>,                   // blinding factorlar buradan turetilir
}
```
Bu plaintext `encrypted_swap` icinde saklanir.

**`SwapClaim` (satir 58-79):**
```rust
pub struct SwapClaim {
    pub proof: Option<ZkSwapClaimProof>,
    pub body: Option<SwapClaimBody>,
    pub epoch_duration: u64,
}
```

**`SwapClaimBody` (satir 81-112):**
```rust
pub struct SwapClaimBody {
    pub nullifier: Option<Nullifier>,          // swap commitment'i harcamak icin
    pub fee: Option<Fee>,                      // claim fee (on-odeme'den)
    pub output_1_commitment: Option<StateCommitment>, // cikti 1
    pub output_2_commitment: Option<StateCommitment>, // cikti 2
    pub output_data: Option<BatchSwapOutputData>,    // clearing fiyati
}
```

### Ticaret Ciftleri (Satir 463-504)

**`TradingPair` (satir 466-484):**
```rust
pub struct TradingPair {
    pub asset_1: Option<AssetId>,
    pub asset_2: Option<AssetId>,
}
```
`asset_1 < asset_2` field element siralamasi zorunlu — canonical representation.

**`DirectedTradingPair` (satir 487-504):**
```rust
pub struct DirectedTradingPair {
    pub start: Option<AssetId>,
    pub end: Option<AssetId>,
}
```
Yon bilgisi olan cifti. Routing icin.

### BatchSwapOutputData (Satir 506-552) — KRITIK

```rust
pub struct BatchSwapOutputData {
    pub delta_1: Option<Amount>,     // toplam varlik 1 girdi
    pub delta_2: Option<Amount>,     // toplam varlik 2 girdi
    pub lambda_1: Option<Amount>,    // 2->1 islemleri icin varlik 1 cikti
    pub lambda_2: Option<Amount>,    // 1->2 islemleri icin varlik 2 cikti
    pub unfilled_1: Option<Amount>,  // 1->2 icin doldurulamayan miktar
    pub unfilled_2: Option<Amount>,  // 2->1 icin doldurulamayan miktar
    pub height: u64,                 // hangi blok yüksekliginde
    pub trading_pair: Option<TradingPair>,
    pub epoch_starting_height: u64,  // DEPRECATED
    pub sct_position_prefix: u64,    // (epoch, block) prefix
}
```

Bu BSOD (Batch Swap Output Data) ZK swap claim proof icin public input olarak kullanilir. Tek islem bazinda degil, bloktaki tum o ticaret ciftinin toplam batch sonucu — gizlilik saglar.

**Clearing fiyati turetme:** Kullanicinin pay ettigi fiyat = `lambda_2 / delta_1` (1->2 icin). Tam saydamlik yerine sadece aggregate bilgi aciklanir.

### TradingFunction ve BareTradingFunction (Satir 554-599) — KRITIK DEX

**`BareTradingFunction` (satir 580-599):**
```rust
pub struct BareTradingFunction {
    pub fee: u32,            // baz puan cinsinden ucret
    pub p: Option<Amount>,   // fiyat parametresi p
    pub q: Option<Amount>,   // fiyat parametresi q
}
```
Ticaret fonksiyonu formulü: `phi(R) = p*R_1 + q*R_2`, `gamma = 1 - fee`. Bu CFM (Constant Function Market Maker) kernel'in protobuf temsili. `p` ve `q`, aslinda `Amount` tipinde degil (yorum: "not actually an amount, it's an integer the same width") — boyut uyumlulugu icin ayni tip kullanilmis.

**`TradingFunction` (satir 558-573):** `component: BareTradingFunction` + `pair: TradingPair`.

### Position ve Reserves (Satir 601-654)

**`Reserves` (satir 607-623):**
```rust
pub struct Reserves {
    pub r1: Option<Amount>,  // varlik 1 rezervi
    pub r2: Option<Amount>,  // varlik 2 rezervi
}
```

**`Position` (satir 624-654):**
```rust
pub struct Position {
    pub phi: Option<TradingFunction>,   // ticaret fonksiyonu (immutable)
    pub nonce: Vec<u8>,                 // benzersiz rastgele deger (PositionId icin)
    pub state: Option<PositionState>,   // Opened/Closed/Withdrawn
    pub reserves: Option<Reserves>,     // guncel rezervler
    pub close_on_fill: bool,            // doldurulunca otomatik kapat (limit order)
}
```

**`PositionId` (satir 656-680):** `inner: Vec<u8>` + `alt_bech32m: String`. Hash of Position data.

**`PositionState` (satir 681-758):** Enum:
- `Opened = 1` — aktif, islem yapilabilir
- `Closed = 2` — kapali, rezervler hala var
- `Withdrawn = 3` — rezervler cekildi (coklu cekis desteklenir, `sequence: u64`)
- `Claimed = 4` — DEPRECATED

**`LpNft` (satir 760-791):** `position_id + state` — LP NFT'nin wire temsili. Sahiplik ve durum birlikte iz surulur.

**`PositionMetadata` (satir 792-813):**
```rust
pub struct PositionMetadata {
    pub strategy: u32,     // UIP-9: 0x01..0x40 rezerv, strateji etiketi
    pub identifier: u32,   // pozisyon/bundle benzersiz kimlik
}
```
UIP-9 referansi: pozisyon bundle'lari icin metadata.

### Pozisyon Aksiyonlari (Satir 814-990)

**`PositionOpen` (satir 818-829):**
```rust
pub struct PositionOpen {
    pub position: Option<Position>,
    pub encrypted_metadata: Vec<u8>,  // 50 byte cipher, pozisyon metadata
}
```

**`PositionClose` (satir 924-938):** Sadece `position_id: Option<PositionId>`. Harcama belgesi (opened LPNFT harcama).

**`PositionWithdraw` (satir 944-960):**
```rust
pub struct PositionWithdraw {
    pub position_id: Option<PositionId>,
    pub reserves_commitment: Option<BalanceCommitment>,  // seffaf commitment
    pub sequence: u64,  // coklu cekis sira numarasi
}
```
Seffaf `reserves_commitment` — chain zinciri on-chain rezervlerle dogrular.

**`PositionRewardClaim` (satir 972-980):** DEPRECATED — `PositionWithdraw` ile birlestirilen.

### SwapExecution (Satir 991-1034)

```rust
pub struct SwapExecution {
    pub traces: Vec<swap_execution::Trace>,  // bireysel islem izleri
    pub input: Option<Value>,                // toplam girdü
    pub output: Option<Value>,               // toplam cikti
}

pub struct Trace {
    pub value: Vec<Value>,  // her hop'taki deger listesi
}
```
Routing izi: her hop'ta hangi varligin ne kadar oldugu listelenir. Multi-hop routing gozlemlenebilir.

### Plan ve View Tipleri (Satir 1035-...)

Her islem aksiyonu icin uc varyant mevcuttur:

| Aksiyon | Plan (istem oncesi) | View (gizli bilgiyle) |
|---|---|---|
| `Swap` | `SwapPlan { swap_plaintext, fee_blinding, proof_blinding_r, proof_blinding_s }` | `SwapView::Visible/Opaque` |
| `SwapClaim` | `SwapClaimPlan { swap_plaintext, position, output_data, epoch_duration, proof_blinding_r, proof_blinding_s }` | `SwapClaimView::Visible/Opaque` |
| `PositionOpen` | `PositionOpenPlan { position, metadata }` | `PositionOpenView::Visible/Opaque` |
| `PositionWithdraw` | `PositionWithdrawPlan { reserves, position_id, pair, sequence, rewards }` | — |

**Visible vs Opaque:** `Visible` — view servisi tarafindan acilmis (plaintext bilgisi mevcut). `Opaque` — sadece on-chain veri (sifrelenmis).

### Query/Response Tipleri

**`BatchSwapOutputDataRequest/Response`** — belirli yukseklik ve cift icin BSOD.
**`SwapExecutionRequest/Response`** — belirli cift ve yukseklik icin swap yürütme izi.
**`ArbExecutionRequest/Response`** — arbitraj yürütme izi.
**`SwapExecutionsRequest/Response`** — belirli aralik ve cift icin akis olarak.
**`ArbExecutionsRequest/Response`** — belirli aralik icin arbitraj.
**`LiquidityPositionsRequest/Response`** — tüm aktif pozisyonlar (closed dahil edilebilir).
**`LiquidityPositionByIdRequest/Response`** — tek pozisyon ID'siyle.
**`LiquidityPositionsByIdRequest/Response`** — coklu ID ile.
**`LiquidityPositionsByPriceRequest/Response`** — fiyata gore siralama (satir 1343-1376): `{ trading_pair, limit }` -> `{ data: Position, id: PositionId }`.
**`SpreadRequest/Response`** (satir 1378-1416): En iyi 1->2 ve 2->1 pozisyonlari + yaklasik efektif fiyatlar (`f64`).
**`SimulateTradeRequest/Response`** (satir 1418-...): `Routing::SingleHop` veya `Routing::Default` opsiyonu — simüle islem.

### DEX Uygulama Notu — Nova Shield

**BareTradingFunction Analizi:**
- `fee: u32` baz puan cinsinden (10000 bps = %100)
- `p`, `q` parametreleri: concentrated liquidity fiyat araligini kodlar
- `gamma = 1 - fee/10000` olarak hesaplanir (domain katmaninda)
- Formül: `phi(R) = p*R1 + q*R2` = ticaret fonksiyonunun sabit deger (conserved quantity)

**Batch Swap Mekanizmasi:**
1. Kullanicinin swap'i sifrelenmis olarak `SwapPayload` icinde commit edilir
2. Blok sonunda tum swap'ler batch halinde calistirilir
3. `BatchSwapOutputData` hesaplanir (aggregate ciktilar)
4. `SwapClaim`'de kullanici BSOD'yi public input olarak kullanarak pay ettigi fiyati kanıtlar

**LP NFT Mekanizması:**
- Pozisyon acma: `PositionOpen` -> `opened LPNFT` mint
- Pozisyon kapama: `PositionClose` -> `closed LPNFT` mint (opened harcar)
- Para cekme: `PositionWithdraw` -> `withdrawn LPNFT` mint + rezervler iade
- Tum bunlar ZK value commitment mekanizmasini kullanir (fazladan proof gerektirmez)

**Nova Shield DEX icin eylem plani:**
1. `BareTradingFunction` (p, q, fee) yapis — aynen kullanilabilir
2. Batch swap output data modeli benimsenmeli — gizlilik + routing
3. `close_on_fill` flag'i ile limit order destegi zaten proto'ya eklenmiş — uygulama katmaninda aktif edilmeli
4. Pozisyon metadata sifreleme (`encrypted_metadata: Vec<u8>`) — metadata gizliligi icin
5. Routing trace (`SwapExecution::Trace`) — her hop'u kayit altina almak UI ve debug icin kritik

---

## [gen/penumbra.core.component.fee.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.fee.v1.rs` **Satir Sayisi:** 580

### Genel Amac
Ucret (fee) bileseninin tam tanimlari. Gas modeli, fiyatlar, tier sistemi.

### Gas Modeli

**`Gas` (satir 27-40):**
```rust
pub struct Gas {
    pub block_space: u64,         // islemin blok icerisindeki boyutu
    pub compact_block_space: u64, // kompakt blok alaninda kaplanan yer
    pub verification: u64,        // ZK kanit dogrulama maliyeti
    pub execution: u64,           // islem yurutme maliyeti
}
```
4 boyutlu gas modeli. Her boyut farkli ucret parametresiyle carpilir.

**`GasPrices` (satir 51-80):**
```rust
pub struct GasPrices {
    pub asset_id: Option<AssetId>,         // tag=15! Hangi token ile odeme
    pub block_space_price: u64,            // implicit 1000 pay
    pub compact_block_space_price: u64,
    pub verification_price: u64,
    pub execution_price: u64,
}
```
Fiyatlar `u64` olarak depolanir, **1000 ile orantilidir** (yorumdan: "with an implicit 1,000 denominator"). Yani `block_space_price = 500` -> gercek fiyat `0.5`.

**`FeeTier` (satir 81-142):**
```
Tier { Unspecified=0, Low=1, Medium=2, High=3 }
```
Kullanici ucret tipi secer (Low/Medium/High); sistem bu tier'dan gercek `Fee` hesaplar. Strateji: daha yuksek tier -> islem daha cok onceliklendirilebilir.

**`Fee` (satir 3-22):** `amount: Option<Amount>` + `asset_id: Option<AssetId>` (yoksa staking token).

**`FeeParameters` (satir 144-173):** Genesis ve governance ile ayarlanabilir parametreler:
- `fixed_gas_prices: Option<GasPrices>` — native token icin sabit fiyatlar
- `fixed_alt_gas_prices: Vec<GasPrices>` — alternatif tokenlar icin sabit fiyatlar

**Events:**
- `EventPaidFee { fee, base_fee, tip, gas_used }` — her islemde yayinlanan event
- `EventBlockFees { swapped_fee_total, swapped_base_fee_total, swapped_tip_total }` — blok ozeti

**gRPC:** `QueryService::CurrentGasPrices` — suanki gas fiyatlarini sorgular. Tonic client + server.

### Nova Shield Chain Icin Dersler
- 4 boyutlu gas modeli: biz de block_space, ZK verification, execution ayrimini yapmaliyiz
- `implicit 1000 denominator` yerine explicit kesirli tip kullanmak daha acik
- `FeeTier` pattern'i kullanici deneyimi icin iyidir; Low/Medium/High seçeneği blockchain explorer'larda gosterilmeli
- Alternatif fee token destegi (`fixed_alt_gas_prices`): diger token ile odeme imkani

---

## [gen/penumbra.core.component.stake.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.stake.v1.rs` **Satir Sayisi:** 1810

### Genel Amac
Staking ve dogrulayici (validator) bileseninin tum tanimlari.

### Validator Yapisi

**`Validator` (satir 19-54):**
```rust
pub struct Validator {
    pub identity_key: Option<IdentityKey>,       // decaf377-rdsa kimlik
    pub consensus_key: Vec<u8>,                  // Ed25519 Tendermint anahtari
    pub name: String,                            // tag=3
    pub website: String,                         // tag=4
    pub description: String,                     // tag=5
    pub enabled: bool,                           // tag=8: devre disi -> unbonding
    pub funding_streams: Vec<FundingStream>,     // tag=6: komisyon akislari
    pub sequence_number: u32,                    // tag=7: replay koruması
    pub governance_key: Option<GovernanceKey>,   // tag=9: yonetim anahtari
}
```
`sequence_number` kritik: her validator tanimi guncellemesinde artmali. `enabled: false` -> hemen unbonding baslangici.

**`FundingStream` (satir 84-147):** Validator komisyonu iki hedefe yonlendirilebilir:
- `ToAddress { address: String, rate_bps: u32 }` — belirli adrese
- `ToCommunityPool { rate_bps: u32 }` — community pool'a
`rate_bps`: baz puan, toplam staking odulu delegasyon havuzunun yuzdesi.

**`RateData` (satir 148-178):**
```rust
pub struct RateData {
    pub identity_key: Option<IdentityKey>,
    pub epoch_index: u64,  // DEPRECATED
    pub validator_reward_rate: Option<Amount>,
    pub validator_exchange_rate: Option<Amount>,  // 10^8 olcekleme
}
```
`validator_exchange_rate` 10^8 olcekleme: `1.234 -> 123400000`. Bu delegation token / staking token donusumunun kuru.

**`BaseRateData` (satir 180-198):** Her donem icin temel odul orani ve exchange rate.

**Delegasyon aksiyonlari:**
- `Delegate { validator_identity_key, from_epoch, unbonded_amount, delegation_amount }`
- `Undelegate { validator_identity_key, from_epoch, unbonded_amount, delegation_amount }`
- `UndelegateClaim { body, proof }` — ZK kanit gerektiren geri alinma talebi

**Validator durumlari:** `Inactive`, `Active`, `Jailed`, `Tombstoned`, `Disabled` — enum.

**`ValidatorDefinition` (satir 200+):** `validator + auth_sig` — validator tanim guncelleme aksiyonu.

### Nova Shield Chain Icin Dersler
- `sequence_number` ile replay saldirisi korunmasi zorunlu
- `enabled: bool` ile validator hizli devre disi birakilmasi kritik
- Exchange rate 10^8 olcekleme: tam sayi aritmetigi icin; Nova Shield'de ayni yaklasim kullanilmali
- `FundingStream` oneof pattern: iki farkli alici tipi oneof ile — extensible

---

## [gen/penumbra.core.component.shielded_pool.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.shielded_pool.v1.rs` **Satir Sayisi:** 1302

### Genel Amac
Shielded pool bileseninin tanimlari. Not sifreleme, FMD parametreleri, spend/output aksiyonlari.

### FMD (Fuzzy Message Detection) Parametreleri

**`FmdMetaParameters` (satir 65-118):**
```rust
pub struct FmdMetaParameters {
    pub fmd_grace_period_blocks: u64,
    pub algorithm: Option<Algorithm>,  // oneof
}
```
Algorithm: `FixedPrecisionBits(u32)` veya `SlidingWindow { window_update_periods, targeted_detections_per_window }`.

**`FmdParameters` (satir 182-196):**
```rust
pub struct FmdParameters {
    pub precision_bits: u32,         // algilamayi kontrol eder
    pub as_of_block_height: u64,     // hangi bloktan itibaren gecerli
}
```
`precision_bits` buyukse daha az yanlis pozitif, daha az gizlilik. Küçükse tam tersi.

**`FmdMetaParametersAlgorithmState`:** `FixedState {}` veya `SlidingWindowState { approximate_clue_count: u32 }`.

### Not Yapilari

**`Note` (satir 198+):** Shielded pool'daki temel birim. `value`, `rseed`, `address` iceriyor.

**`NoteView`:** Visible/Opaque variantli not gorunumu.

**`NotePayload`:** Wire format — `note_commitment + ephemeral_key + encrypted_note`.

**`ShieldedPoolParameters` (satir 3-20):**
- `fixed_fmd_params: DEPRECATED`
- `fmd_meta_params: Option<FmdMetaParameters>` — yeni yaklasim

**`GenesisContent` (satir 22-63):** `shielded_pool_params` + `allocations: Vec<Allocation>`. Her allocation: `amount + denom + address`.

### Spend/Output Aksiyonlari

**`Spend`:** `body + auth_sig + proof` — ZK spend kaniti.
**`Output`:** `body + proof` — ZK output kaniti.
**Event'ler:** `EventSpend { nullifier }`, `EventOutput { note_commitment }`, `EventClue { clue, tx }`.

### Nova Shield Chain Icin Dersler
- FMD (Fuzzy Message Detection) gizlilik bilesenine entegre: her islemde clue gonder, view servisi kendi clue'unu kontrol eder.
- `SlidingWindow` FMD algoritmasi adaptif: biz de parametrik bir FMD sistemi tasarlamali, sabit deger yerine.
- Genesis allocation pattern: `Vec<Allocation>` listesiyle genesis state yonetimi.

---

## [gen/penumbra.core.component.sct.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.sct.v1.rs` **Satir Sayisi:** ~400

### Genel Amac
State Commitment Tree (SCT) bileseninin tanimlari.

### Temel Tipler

**`SctParameters { epoch_duration: u64 }`** — varsayilan epoch uzunlugu (blok sayisi).

**`Epoch { index: u64, start_height: u64 }`** — ardisik blok koleksiyonu.

**`Nullifier { inner: Vec<u8> }`** — harcanmis commitment'in birinci preimage dayanikli karması.

**`CommitmentSource` (satir 61-...):**
```rust
pub enum Source {
    Genesis {},
    Transaction { id: Vec<u8> },
    FundingStreamReward { epoch_index: u64 },
    CommunityPoolOutput {},
    Ics20Transfer { packet_seq, channel_id, sender },
    // tag: 1, 2, 20, 30, 40, 50
}
```
6 farkli kaynak tipi. `Transaction.id` bos gelebilir — `"Transaction (Unknown)"` olarak yorumlanir. Bu "dehydration" — `CompactBlock` boyutunu kucultur.

**Block/Epoch Event'leri:** `EventCommitment`, `EventEpochRoot`, `EventBlockRoot` vb.

### Nova Shield Chain Icin Dersler
- Commitment source'u kayit altinda tutmak zorunlu: indexer ve explorer'lar icin kritik.
- ICS20 Transfer kaynagi ayri tip: IBC transferlerinden gelen commitmentlari izleme.
- "Dehydration" pattern: compact block'ta transaction ID'yi dusurmek, later "rehydration" ile eslestirme.

---

## [gen/penumbra.core.txhash.v1.rs]
**Dosya:** `.../gen/penumbra.core.txhash.v1.rs` **Satir Sayisi:** 37

### Tanimlanan Tipler

**`EffectHash { inner: Vec<u8> }`** — islemin *etkileyen verilerinin* Sha256 hash'i. Konsensus kritik.

**`TransactionId { inner: Vec<u8> }`** — `Transaction` nesnesinin Sha256 hash'i. Tendermint tarafindan kullanilan ID.

Not: `EffectHash` != `TransactionId`. Effecting data validation-critical; transaction ID raw byte encoding hash'i.

---

## [gen/penumbra.crypto.tct.v1.rs]
**Dosya:** `.../gen/penumbra.crypto.tct.v1.rs` **Satir Sayisi:** 73

### Tanimlanan Tipler

**`StateCommitment { inner: Vec<u8> }`** — TCT'deki tek bir yaprak commitment.

**`MerkleRoot { inner: Vec<u8> }`** — TCT'nin kok hash'i. ZK anchor olarak kullanilir.

**`StateCommitmentProof { note_commitment, position, auth_path: Vec<MerklePathChunk> }`** — Membership kaniti.

**`MerklePathChunk { sibling_1, sibling_2, sibling_3 }`** — 3 kardes hash; 4-ary Merkle tree (her node 4 cocuk).
`auth_path` **her zaman uzunlugu 24'tur** (yorum: `// always length 24`).

**4-ary Tree Derinligi:** 24 katman x 4^24 = ~281 trilyon yaprak kapasitesi.

---

## [gen/penumbra.crypto.decaf377_rdsa.v1.rs]
**Dosya:** `.../gen/penumbra.crypto.decaf377_rdsa.v1.rs` **Satir Sayisi:** 47

### Tanimlanan Tipler

```rust
SpendAuthSignature { inner: Vec<u8> }   // SpendAuth imzasi
BindingSignature { inner: Vec<u8> }     // Binding imzasi
SpendVerificationKey { inner: Vec<u8> } // SpendAuth dogrulama anahtari
```

decaf377-rdsa: Penumbra'nin kendi eliptik egri (BLS12-377 subgroup) uzerinde RedDSA imzalama.

- `SpendAuthSignature`: harcama yetkisi — Not'u kim harcayabilir
- `BindingSignature`: islem balansini baglayan imza (value commitment binding)

---

## [gen/penumbra.crypto.decaf377_fmd.v1.rs]
**Dosya:** `.../gen/penumbra.crypto.decaf377_fmd.v1.rs` **Satir Sayisi:** 18

### Tanimlanan Tip

**`Clue { inner: Vec<u8> }`** — FMD clue, **68 byte** olmal (TryFrom implementasyonunda dogrulandi).

---

## [gen/penumbra.crypto.decaf377_frost.v1.rs]
**Dosya:** `.../gen/penumbra.crypto.decaf377_frost.v1.rs` **Satir Sayisi:** ~150

### Genel Amac
FROST (Flexible Round-Optimized Schnorr Threshold signatures) uzerinden dagitik imza icin protobuf tipleri. decaf377 egrisi uzerinde.

### Tanimlanan Tipler

**DKG (Distributed Key Generation):**
- `VerifiableSecretSharingCommitment { elements: Vec<Vec<u8>> }` — polinom commitment (VSS)
- `DkgRound1Package { commitment, proof_of_knowledge }` — Round 1 paketi
- `SigningShare { scalar: Vec<u8> }` — imzalama hissesi (scalar)
- `DkgRound2Package { signing_share }` — Round 2 paketi

**FROST Imzalama:**
- `NonceCommitment { element: Vec<u8> }` — nonce commitment (group element)
- `SigningCommitments { hiding, binding }` — Round 1 imzalama taahhudleri
- `SignatureShare { scalar: Vec<u8> }` — her katilimcinin imza hissesi
- `SigningPackage { message, commitments: Vec<SigningCommitmentsIdentified> }`

### Nova Shield Chain Icin Dersler
- FROST/DKG: threshold signature icin kullanilabilir — MPC cuzdan/validator anahtari yonetimi
- DKG 2 round, imzalama 2 round — toplam 4 mesajlasma turu
- `VerifiableSecretSharingCommitment` polynomial commitment: VSS (Verifiable Secret Sharing) icin

---

## [gen/penumbra.core.component.compact_block.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.compact_block.v1.rs` **Satir Sayisi:** ~300

### Genel Amac
Light client senkronizasyonu icin minimalist blok temsili.

### CompactBlock (Satir 4-48)

```rust
pub struct CompactBlock {
    pub height: u64,
    pub state_payloads: Vec<StatePayload>,        // yeni state parcalari
    pub nullifiers: Vec<Nullifier>,               // harcanan notlar
    pub block_root: Option<MerkleRoot>,
    pub epoch_root: Option<MerkleRoot>,           // sadece epoch sonu blogunda
    pub proposal_started: bool,                   // yonetim oneri basladi
    pub fmd_parameters: Option<FmdParameters>,
    pub swap_outputs: Vec<BatchSwapOutputData>,   // blok icindeki swap ciktilari
    pub app_parameters_updated: bool,
    pub gas_prices: Option<GasPrices>,            // guncellendiyse
    pub alt_gas_prices: Vec<GasPrices>,           // alternatif tokenlar icin (tag=100!)
    pub epoch_index: u64,
}
```

**`StatePayload` (satir 61-140):**
```rust
pub struct StatePayload {
    pub source: Option<CommitmentSource>,
    pub state_payload: Option<StatePayload>,  // oneof: RolledUp | Note | Swap
}
```
- `RolledUp { commitment }` — not gizlenmis, sadece commitment
- `Note { note: NotePayload }` — not mevcut (alici scan edebilir)
- `Swap { swap: SwapPayload }` — swap mevcut

### CompactBlockRangeRequest (Satir 142-...)
`start_height`, `end_height`, `keep_alive: bool` (streaming icin).

### Nova Shield Chain Icin Dersler
- `CompactBlock` pattern: istemci senkronizasyonu icin minimaldir. Tam blok yerine sadece TCT degisiklikleri + nullifierlar.
- `keep_alive: bool` ile streaming RPC — blok stream subscription.
- `swap_outputs` compact block'ta: DEX kullanicilari clearing fiyatini compact block uzerinden ogrenebilir.
- `epoch_root`: epoch sinirlarini compact block uzerinden izlemek.

---

## [gen/penumbra.core.component.auction.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.auction.v1.rs` **Satir Sayisi:** ~600

### Genel Amac
Dutch auction protokolu — DEX uzerinde programatik likidite ile fiyat kesifi.

### DutchAuctionDescription (Satir 144-191)

```rust
pub struct DutchAuctionDescription {
    pub input: Option<Value>,          // satilik varlik
    pub output_id: Option<AssetId>,    // hedef varlik
    pub max_output: Option<Amount>,    // baslangic fiyat (implicitly)
    pub min_output: Option<Amount>,    // bitis fiyat (implicitly)
    pub start_height: u64,
    pub end_height: u64,               // max_output, min_output ve (end-start) -> hiz
    pub step_count: u64,               // (end - start) / step_count blok basi
    pub nonce: Vec<u8>,                // ayni aciklamanin farkli ID'si olabilsin
}
```

**Kritik kilit:** `(end_height - start_height)` `step_count`'un kati olmali.

**`DutchAuctionState { seq, current_position, next_trigger }` (satir 193+):**
- `seq`: 0=acik, 1=kapali, n=cekilmis (coklu cekis)
- `current_position: Option<PositionId>` — aktif DEX pozisyonu
- `next_trigger: u64` — sonraki adim tetiklenecek yukseklik

**`AuctionNft { id, seq }`** — sahiplik ve durum takibi icin NFT.

**`AuctionId`** — hash(domain_separator, description_immutable_fields).

**Auctions ve DEX entegrasyonu:** Her adimda `current_position` olarak DEX'e pozisyon acilir. Fiyat dutch mekanizmasina gore dusurilir. Bu "programmatic liquidity" — insan mudahalesi gerektirmeyen otomatik MM.

### Nova Shield Chain Icin Dersler — KRITIK DEX

**Dutch Auction = Fiyat Kesif Mekanizmasi:**
Penumbra'nin Dutch auction'i DEX likilditesiyle dogrudan entegre. Her adimda:
1. Onceki adimin pozisyonu kapanir
2. Yeni fiyat hesaplanir (max_output'tan min_output'a lineer azalis)
3. Yeni DEX pozisyonu acilir

**Nova Shield DEX icin tavsiye:**
- Dutch auction modeli token launch, buyuk token satisilari ve price discovery icin uygun
- `step_count` ve height araliginin katini zorunlu kilmak, zamanlama hatalarini onler
- `AuctionNft` ile sahiplik takibi — her cekis yeni NFT mint eder (seq artar)

---

## [gen/penumbra.core.component.distributions.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.distributions.v1.rs` **Satir Sayisi:** 530

### Genel Amac
Token dagitimi ve Liquidity Tournament (LQT) incentive sistemi.

### DistributionsParameters (Satir 4-22)

```rust
pub struct DistributionsParameters {
    pub staking_issuance_per_block: u64,                     // her blokta staking odulu
    pub liquidity_tournament_incentive_per_block: u64,       // her blokta LQT havuzuna
    pub liquidity_tournament_end_block: u64,                 // bu yukseklikten sonra odul yok
}
```

### Query Servisi
- `CurrentLqtPoolSizeRequest/Response { epoch_index, pool_size }` — mevcut LQT havuzu
- `LqtPoolSizeByEpochRequest/Response { epoch, epoch_index, pool_size }` — gecmis epoch

**Event:** `EventLqtPoolSizeIncrease { epoch, increase, new_total }`.

### Nova Shield Chain Icin Dersler
- `staking_issuance_per_block`: blok basi yeni token basimi, Nova Shield tokenomics ile hizalanmali
- LQT havuzu: likidite saglaycilara odul icin; Nova Shield DEX'te benzer tesvikleme yapisi kurulabilir
- `end_block` parametresi: odul programinin belirli bir tarihte bitmesi — inflation kontrolu icin kritik

---

## [gen/penumbra.core.component.funding.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.funding.v1.rs` **Satir Sayisi:** 738

### Genel Amac
Funding stream odulleri, LQT (Liquidity Tournament) oylaması ve odul dagitimi.

### FundingParameters::LiquidityTournament (Satir 60-93)

```rust
pub struct LiquidityTournament {
    pub gauge_threshold_percent: u64,  // min oy eşigi [0,100]
    pub max_positions: u64,            // DoS koruması
    pub max_delegators: u64,           // DoS koruması
    pub delegator_share_percent: u64,  // odulun delegatorlara giden kismi [0,100]
}
```

### ActionLiquidityTournamentVote (Satir 148-171)

```rust
pub struct ActionLiquidityTournamentVote {
    pub body: Option<LiquidityTournamentVoteBody>,
    pub auth_sig: Option<SpendAuthSignature>,
    pub proof: Option<ZkLiquidityTournamentVoteProof>,
}
```

**LiquidityTournamentVoteBody (satir 172-208):**
```rust
pub struct LiquidityTournamentVoteBody {
    pub incentivized: Option<Denom>,          // hangi asset tesviklensin
    pub rewards_recipient: Option<Address>,   // odul alacak adres
    pub start_position: u64,                  // tournament baslangi pozisyonu
    pub value: Option<Value>,                 // delege token miktari
    pub nullifier: Option<Nullifier>,         // harcanan notan nullifieru
    pub rk: Option<SpendVerificationKey>,     // randomize edilmis dogrulama anahtari
}
```

ZK kanit ile private: hangi notan kim tarafindan harcandigini aciklamadan oy kullanilabiliyor.

### Events

**EventFundingStreamReward:** `{ recipient, epoch_index, reward_amount }` — validator komisyon odeme.
**EventLqtDelegatorReward:** `{ epoch_index, reward_amount, delegation_tokens, address, incentivized_asset_id }` — delegator odeme.
**EventLqtPositionReward:** `{ epoch_index, reward_amount, position_id, incentivized_asset_id, tournament_volume, position_volume }` — LP pozisyon odeme.
**EventLqtVote:** `{ epoch_index, voting_power, incentivized_asset_id, rewards_recipient, tx_id }`.

### Nova Shield Chain Icin Dersler
- Liquidity Tournament: delegatorlar oy kullanarak hangi trading pair'in tesviklendirileceğini belirler; bir gauge sistemi
- `gauge_threshold_percent` ile minimum oy esigi: spam oylama koruması
- `max_positions` ve `max_delegators`: DoS koruması parametrik ve governance ile guncellenebilir
- `delegator_share_percent` ile delegator/LP odul payi boluşumu
- ZK oy: gizli oy ancak double-voting koruması — nullifier ile

---

## [gen/penumbra.core.component.governance.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.governance.v1.rs` **Satir Sayisi:** 2425

### Genel Amac
Tam on-chain yonetim sistemi. Oneriler, oylar, yetki transferi.

### Temel Aksiyonlar

**`ProposalSubmit { proposal, deposit_amount }`** — oneri sunma, depozito kilitleme.
**`ProposalWithdraw { proposal: u64, reason: String }`** — oneri geri cekme.
**`ProposalDepositClaim { proposal, deposit_amount, outcome }`** — depozito geri alma.

**`ValidatorVote { body: ValidatorVoteBody, auth_sig }`:**
```rust
pub struct ValidatorVoteBody {
    pub proposal: u64,
    pub vote: Option<Vote>,
    pub identity_key: Option<IdentityKey>,
    pub governance_key: Option<GovernanceKey>,
    pub reason: Option<ValidatorVoteReason>,
}
```
Validator'in oy kullanmasi; governance key ile imzalanir (identity key degil).

**`DelegatorVote { body: DelegatorVoteBody, auth_sig, proof }`** — ZK kanit ile delegator oyu.

**`Proposal { id, title, description, payload }`:**
```rust
pub enum Payload {
    Signaling {},                                     // signal only
    Emergency { halt_chain: bool },                  // acil durum
    ParameterChange { old_app_parameters, new_app_parameters },  // parametre degisikliği
    DaoSpend { transaction_plan },                   // community pool harcamasi
    UpgradeP { new_app_hash },                       // protokol yukseltme
    CommunityPoolSpend { transaction_plan },          // community pool
    FreezeIbcClient { client_id },                   // IBC dondurma
    UnfreezeIbcClient { client_id },                 // IBC cozme
}
```

**`ProposalOutcome`:** `Passed`, `Failed`, `Slashed`.

### Nova Shield Chain Icin Dersler
- `Emergency { halt_chain }` — zinciri durdurabilmek icin acil yonetim yolu
- `ParameterChange` ile on-chain parametre guncelleme: Penumbra'da tum parametreler governance yoluyla degistirilebilir
- Governance key / Identity key ayirimi: validator iki farkli anahtar tutar — kimlik ve yonetim

---

## [gen/penumbra.core.component.community_pool.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.community_pool.v1.rs` **Satir Sayisi:** 401

### Genel Amac
Community pool yonetimi — governance tarafindan kontrol edilen hazine.

### Temel Tipler

**`CommunityPoolParameters { community_pool_spend_proposals_enabled: bool }`**

**`GenesisContent { community_pool_params, initial_balance: Option<Value> }`**

**Aksiyonlar:** `CommunityPoolSpend { transaction_plan }`, `CommunityPoolDeposit { value }`, `CommunityPoolOutput { value, address }`.

**Query:** `CommunityPoolAssetBalancesRequest { asset_ids: Vec<AssetId> }` -> `CommunityPoolAssetBalancesResponse { balance: Option<Value> }`.

### Nova Shield Chain Icin Dersler
- Community pool baslangic bakiyesi genesis'te belirlenebilir (`initial_balance`)
- Harcama onaylari governance yoluyla — treasury tek kisi kontrolunde olmamali

---

## [gen/penumbra.core.component.ibc.v1.rs]
**Dosya:** `.../gen/penumbra.core.component.ibc.v1.rs` **Satir Sayisi:** 236

### Genel Amac
IBC entegrasyon tipleri.

### Temel Tip

**`IbcRelay { raw_action: Option<pbjson_types::Any> }`** — IBC mesajlarini `Any` ile tasiyan kap. Gercek IBC mesajlari `ibc_proto` crate'inden gelir; bu sadece sarici.

### Aksiyonlar
`IbcRelayAction`, `Ics20Withdrawal { amount, denom, destination_chain_address, ... }`.

---

## [gen/penumbra.core.transaction.v1.rs]
**Dosya:** `.../gen/penumbra.core.transaction.v1.rs` **Satir Sayisi:** 952

### Genel Amac
Islem (transaction) yapisinin tum tanimlari.

### Transaction (Satir 3-26)

```rust
pub struct Transaction {
    pub body: Option<TransactionBody>,
    pub binding_sig: Option<BindingSignature>,  // value commitment'lari baglayan imza
    pub anchor: Option<MerkleRoot>,              // gecmis TCT kok'u (ZK proof icin anchor)
}
```

### TransactionBody (Satir 28-53)

```rust
pub struct TransactionBody {
    pub actions: Vec<Action>,
    pub transaction_parameters: Option<TransactionParameters>,
    pub detection_data: Option<DetectionData>,   // FMD clue'lari
    pub memo: Option<MemoCiphertext>,            // sifrelenmis not (output varsa)
}
```

**`TransactionParameters`:**
```rust
pub struct TransactionParameters {
    pub expiry_height: u64,  // 0 = max; son blok
    pub chain_id: String,    // replay koruması
    pub fee: Option<Fee>,
}
```

### Action (Satir 138-...) — KRITIK

Oneof 26 varyant:

| Tag | Varyant |
|---|---|
| 1 | `Spend` (shielded_pool) |
| 2 | `Output` (shielded_pool) |
| 3 | `Swap` (dex) |
| 4 | `SwapClaim` (dex) |
| 16 | `ValidatorDefinition` (stake) |
| 17 | `IbcRelayAction` (ibc) |
| 18 | `ProposalSubmit` (governance) |
| 19 | `ProposalWithdraw` (governance) |
| 20 | `ValidatorVote` (governance) |
| 21 | `DelegatorVote` (governance) |
| 22 | `ProposalDepositClaim` (governance) |
| 30 | `PositionOpen` (dex) |
| 31 | `PositionClose` (dex) |
| 32 | `PositionWithdraw` (dex) |
| 34 | `PositionRewardClaim` (dex, DEPRECATED) |
| 40 | `Delegate` (stake) |
| 41 | `Undelegate` (stake) |
| 42 | `UndelegateClaim` (stake) |
| 50 | `CommunityPoolSpend` (community_pool) |
| 51 | `CommunityPoolOutput` (community_pool) |
| 52 | `CommunityPoolDeposit` (community_pool) |
| 53 | `ActionDutchAuctionSchedule` (auction) |
| 54 | `ActionDutchAuctionEnd` (auction) |
| 55 | `ActionDutchAuctionWithdraw` (auction) |
| 70 | `ActionLiquidityTournamentVote` (funding) |
| 200 | `IcsWithdrawal` (ibc) |

Tag numaralari kasitli: ortak (1-4), stake (16-17), governance (18-22), DEX pozisyon (30-34), stake (40-42), community pool (50-52), auction (53-55), funding (70), IBC (200).

### TransactionSummary ve TransactionPlan

`TransactionSummary { effects: Vec<Effects { address, balance }> }` — islemin bakiye etkilerinin ozeti.

`TransactionPlan` — kullanicinin henuz imzalamadigi islem plani. Her aksiyon plan formatinda.

`AuthorizationData` — custodian'dan gelen imzalama yetkisi (spend_auth_signatures, effect_hash).

### Nova Shield Chain Icin Dersler
- `anchor: Option<MerkleRoot>` — ZK kanalitini hangi TCT kok'une baglandigi; replay koruması icin kritik
- `chain_id: String` + `expiry_height` — iki katmanlı replay koruması
- 26 action varyanti: Nova Shield'in tum on-chain aksiyonlari tek bir `Action` oneof icerisinde olmali
- `TransactionPlan` -> `AuthorizationData` -> `Transaction` akisi: kullanici deneyimi icin plan-authorize-build ayirimi

---

## [gen/penumbra.custody.v1.rs]
**Dosya:** `.../gen/penumbra.custody.v1.rs` **Satir Sayisi:** ~400

### Genel Amac
Custody protokolu — cuzdan/imzalama servisleri.

### Temel Tipler

**`AuthorizeRequest { plan, pre_authorizations }`** — plan imzalama talebi.
**`AuthorizeResponse { data: AuthorizationData }`** — imzalanmis yetki.
**`AuthorizeValidatorDefinitionRequest/Response`** — validator tanimi imzalama.
**`AuthorizeValidatorVoteRequest/Response`** — validator oyu imzalama.

**`PreAuthorization`** — backend-spesifik on-yetki verisi. Cok tarafli pre-auth icin birden fazla paket gonderilebilir.

---

## [gen/penumbra.custody.threshold.v1.rs]
**Dosya:** `.../gen/penumbra.custody.threshold.v1.rs` **Satir Sayisi:** ~300

### Genel Amac
Threshold (esikli) imzalama icin FROST protokolu wire format.

### Temel Tipler

**`VerificationKey { inner: Vec<u8> }`**, **`Signature { inner: Vec<u8> }`**

**`CoordinatorRound1`** — koordinatorun Round 1'de gonderdigi (plan, validator_definition, veya validator_vote istek).

**`CoordinatorRound2`** — Round 2'de gonderdigi imzalama paketleri.
- `IdentifiedCommitments { identifier, commitments: SigningCommitments }` — FROST identifier + Round 1 commitmentlari.

**`FollowerRound1`** — takipcinin Round 1'e cevabi (`commitments: Vec<SigningCommitments>`).
**`FollowerRound2`** — takipcinin Round 2'ye cevabi (`signature_shares: Vec<SignatureShare>`).

### FROST 2 Round Protokol Ozeti
1. Koordinator: `CoordinatorRound1` -> Takipciler
2. Takipciler: `FollowerRound1` -> Koordinator (commitmentlar)
3. Koordinator: `CoordinatorRound2` -> Takipciler (signing package)
4. Takipciler: `FollowerRound2` -> Koordinator (signature shares)
5. Koordinator: imza paylarini birlestirerek final imzayi olusturur

### Nova Shield Chain Icin Dersler
- FROST protokolu: `cggmp21` (memory'de zaten secilmis) ile uyumlu degil ama yapi benzer
- Threshold imzalama icin bu 2-round gidis-gelis minimize etmek icin offline hazirlik yapilabilir
- `pre_authorizations` alani: esikli imzalama icin Round 1 commitmentlari pre-authorization olarak gonderilebilir

---

## [gen/penumbra.core.app.v1.rs]
**Dosya:** `.../gen/penumbra.core.app.v1.rs` **Satir Sayisi:** 641

### Genel Amac
Uygulamanin en ust seviye Genesis ve AppParameters yapisi.

### AppParameters

```rust
pub struct AppParameters {
    pub chain_id: String,
    pub sct_params: Option<SctParameters>,
    pub community_pool_params: Option<CommunityPoolParameters>,
    pub governance_params: Option<GovernanceParameters>,
    pub ibc_params: Option<IbcParameters>,
    pub stake_params: Option<StakeParameters>,
    pub fee_params: Option<FeeParameters>,
    pub distributions_params: Option<DistributionsParameters>,
    pub funding_params: Option<FundingParameters>,
    pub shielded_pool_params: Option<ShieldedPoolParameters>,
    pub dex_params: Option<DexParameters>,
    pub auction_params: Option<AuctionParameters>,
}
```

Tum bilesenin parametreleri tek bir mesajda — governance ile guncellenebilir.

### GenesisAppState

```rust
pub struct GenesisAppState {
    pub genesis_app_state: Option<OnOf>,  // GenesisContent | MigrateContent
}
```

`GenesisContent { app_params, community_pool_content, distributions_content, ... }` — tam genesis.
`MigrateContent { halt_height }` — yukseltme migration'i icin.

---

## [gen/penumbra.view.v1.rs]
**Dosya:** `.../gen/penumbra.view.v1.rs` **Satir Sayisi:** 5575

### Genel Amac
View servisi: cuzdan goruntuleme protokolu. Shielded pool'u tarayan ve kullanicinin notlarini takip eden servis.

### Temel Kavramlar

View servisi kullanicinin Full Viewing Key'i ile calisir ve:
- Compact block'lari tarayarak kullanciya ait notlari bulur
- Not bakiyelerini, islem gecmisini, swap durumunu saglar

### Onemli Request/Response'lar

**`TransactionInfoByHashRequest/Response`** — TX hash ile islem sorgusu.
**`NotesRequest/Response`** — filtreleme ile not listesi.
**`BalancesRequest/Response`** — hesap/adres bakiyeleri.
**`SwapsByHeightRequest`** — belirli blok yuksekligindeki swap'ler.
**`WitnessRequest`** — Not'un TCT kaniti.
**`OwnedPositionsRequest`** — kullanicinin sahip oldugu LP pozisyonlari.
**`AssetMetadataByIdRequest`** — asset metadata sorgusu.
**`LiquidityTournamentVotesByAddressRequest`** — adrese gore LQT oy gecmisi.
**`UnclaimedSwapsRequest/Response`** — talep edilmemis swap'ler.
**`AuthorizeAndBuildRequest`** — plan + yetki -> gercek islem.

### NotesResponse

```rust
pub struct NotesResponse {
    pub note_record: Option<SpendableNoteRecord>,
}

pub struct SpendableNoteRecord {
    pub note_commitment: Option<StateCommitment>,
    pub note: Option<Note>,
    pub address_index: Option<AddressIndex>,
    pub nullifier: Option<Nullifier>,
    pub height_created: u64,
    pub height_spent: Option<u64>,  // None = harcanmamis
    pub position: u64,
    pub source: Option<CommitmentSource>,
    pub return_address: Option<AddressView>,
}
```

### Nova Shield Chain Icin Dersler
- View servisi tamamen ayri bir RPC servisi: wallet'in calisma mantigi zincir validasyonundan ayri
- `SpendableNoteRecord` — kullanicinin kendi notlarini izlemek icin gerekli tum bilgi
- `height_spent: Option<u64>` — `None` harcanmamis, set ise harcama yuksekligi
- Streaming response'lar (`keep_alive: bool` gibi) real-time sync icin zorunlu

---

## [gen/penumbra.util.tendermint_proxy.v1.rs]
**Dosya:** `.../gen/penumbra.util.tendermint_proxy.v1.rs` **Satir Sayisi:** 1068

### Genel Amac
Tendermint RPC'sini Penumbra proto formatinda proxy eden servis.

### Desteklenen RPC Endpoint'leri

- `GetTx` — TX hash ile sorgulama
- `BroadcastTxAsync` — async yayinlama
- `BroadcastTxSync` — sync yayinlama
- `GetStatus` — node durumu
- `AbciQuery` — ABCI sorgusu
- `GetBlockByHeight` — blok sorgusu

Her endpoint icin `Request/Response` tipleri.

**Not:** `tendermint_compat.rs`'deki donusimlerin kullanici tarafindaki protokolü bu tiplere gore sekilleniyor.

---

## [gen/penumbra.tools.summoning.v1.rs]
**Dosya:** `.../gen/penumbra.tools.summoning.v1.rs` **Satir Sayisi:** 583

### Genel Amac
ZK devre trusted setup seremoni — Powers of Tau (Groth16 CRS) yonetimi.

### Temel Tipler

**`ParticipantRequest { position, updated_elements }`** — katilimci deger gonderir.
**`ParticipantResponse { done, contribution_in }` `

**`CeremonyCrs`** — CRS (Common Reference String), Groth16 kurulum tanimlari.
**`CeremonyLinkingProof`** — CRS dogrulama kaniti.
**`CeremonyParticipantPosition`** — sira numarasi.
**`CeremonyContribution { updated, signing_key, signature, parent_hash }`** — bir katilimcinin katkisi.

**Streaming servisi:** `participate` — hem request hem response streaming.

### Nova Shield Chain Icin Dersler
- ZK devreleri icin trusted setup gerekiyorsa bu protokol referans alinabilir
- `CeremonyContribution { parent_hash }` zincir uzerinden katilim ispati saglar
- Streaming seremoni API'si: cok katilimcili seremoniyi asenkron yonetir

---

## [gen/cosmos.* + tendermint.*] — External Proto Tipleri

### cosmos.base.v1beta1
- `Coin { denom, amount }` — Cosmos ekosistemi temel para birimi.

### cosmos.tx.v1beta1
- `Tx`, `TxBody`, `AuthInfo`, `SignerInfo`, `ModeInfo`, `Fee` — Cosmos TX yapisi (IBC uyumlulugu icin)

### cosmos.auth.v1beta1
- `BaseAccount`, `ModuleAccount` — hesap tipleri.

### cosmos.bank.v1beta1
- `MsgSend`, `Balance` — banka transferleri.

### tendermint.*
- `abci.*` — ABCI Request/Response tipleri.
- `types.*` — Block, Header, Vote, Evidence, Commit.
- `crypto.*` — PublicKey, ProofOps.
- `p2p.*` — DefaultNodeInfo.
- `version.*` — Consensus version.

### noble.forwarding.v1
- `ForwardingAddress` — token forwarding (IBC).
- `MsgRegisterAccount`, `MsgClearAccount` — Forward hesap yonetimi.

---

## [*.serde.rs Dosyalari]

Tum `gen/penumbra.*.serde.rs` dosyalari prost-build + pbjson ile olusturulmustur. Her proto tip icin `serde::Serialize` ve `serde::Deserialize` implementasyonlari saglar.

**Temel ozellikler:**
- `bytes` alanlari base64 string olarak seri edilir (ProtoJSON standardina gore)
- `enum` alanlari string olarak: `"POSITION_STATE_ENUM_OPENED"`
- `oneof` alanlari Rust enum olarak; seri edilirken variant adi JSON key olur
- Snake_case -> camelCase donusumu otomatik (ProtoJSON)

---

## GENEL MIMARI DERSLER

### DomainType Pattern — En Onemli Ders

```
Wire bytes <--prost--> Proto Type <--TryFrom/From--> Domain Type
                      validation boundary
```

Bu ayrim:
1. Proto tip: sadece serialize/deserialize, validation YOK
2. Domain tip: tam validasyon, semantik kontrollar
3. `TryFrom` sinirinda hata yonetimi merkezlesmis

Nova Shield icin **bu mimayi aynen benimsemek zorunludur.**

### StateReadProto/StateWriteProto Pattern

Storage'i dogrudan byte'larla degil, domain/proto tiplerle kullanmak:
- Kodun okunabilirligini arttirir
- Serialization hatasini yakalamay kolaylastirir
- Verifiable/nonverifiable ayirimi acik

### Gizlilik Katmanlari

Penumbra uc katmanli gizlilik:
1. **ZK Kanit** — harcama/cikti/swap gizliligi
2. **FMD (Fuzzy Message Detection)** — kime not gonderildigini gizleme
3. **View Katmani** — Full Viewing Key ile gorunurluk kontrolu

### NFT Tabanli Durum Yonetimi

LP pozisyonlari, swap'ler, auction'lar hepsi NFT olarak temsil edilir:
- Sahiplik takibi: shielded pool mekanizmasi kullanilir
- Durum gecisi: eski NFT harcar, yeni NFT mint edilir
- Avantaj: LP mekanizmasi ek kriptografi gerektirmez

---

## OZET — NOVA SHIELD CHAIN EYLEM PLANI

### Kritik Uygulama Notalari

1. **DomainType trait'ini birebir uygula** — proto/domain ayirimi Nova Shield'de de zorunlu.

2. **Amount tipi: lo/hi u128 ayirimi** — Protobuf u128'i desteklemiyor; `{ lo: u64, hi: u64 }` standardini kullan.

3. **DEX trading function: BareTradingFunction(fee, p, q)** — `phi(R) = p*R1 + q*R2`, `gamma = 1-fee`. Aynen kullanilabilir.

4. **Batch Swap Output Data** — gizliligi koruyan clearing fiyat mekanizmasi. Her (pair, height) icin aggregate. ZK swap claim proof icin public input.

5. **LP NFT mekanizmasi** — pozisyon acma/kapama/cekme: NFT state machine. Ek kriptografi gerekmez.

6. **Bech32m prefix'leri** — her tip icin farkli HRP:
   - Adres: `nova` (veya proje ozgu)
   - LP ID: `nlpid`
   - Auction ID: `nauctid`
   - Asset ID: `nasset`

7. **Gas modeli: 4 boyutlu** — block_space + compact_block_space + verification + execution. ZK kanit dogrulamasi ayri boyut olmali.

8. **Fee tier sistemi** — Low/Medium/High; kullanicinin secmesi icin; islem onceligi ile iliskili.

9. **Compact block pattern** — light client sync icin minimal blok: sadece TCT degisiklikleri + nullifierlar + swap output'lar.

10. **Dutch auction ile DEX entegrasyonu** — programatik likidite ile fiyat kesifi. Token launch icin.

11. **FROST threshold custody** — validator anahtarlari icin; 2-round koordinator-takipci protokol.

12. **Timestamp dogrulama** — RFC3339 string round-trip yerine direct UNIX timestamp kullan.

13. **Height overflow kontrolu** — `u64 -> i64` donusumlerinde explicit `try_into().map_err(...)` kullan.

14. **ABCI event determinizmi** — attribute'lari alfabetik siraya koy (cosmos-sdk uyumluluğu).

15. **ProtoEvent pattern** — protobuf mesajlari dogrudan ABCI event olarak kullanilabilir; JSON seri edilebilirlik ile type-safe event.

---

**Analizi Yapan:** Claude Sonnet 4.6
**Kapsam:** 88 .rs dosyasi, ~25.000+ satir
**Onem Siralamasi:** dex.v1.rs (3638 satir, en kapsamli), view.v1.rs (5575 satir), governance.v1.rs (2425 satir), protobuf.rs (DomainType — en kritik), state/read.rs ve state/write.rs (storage entegrasyonu)
