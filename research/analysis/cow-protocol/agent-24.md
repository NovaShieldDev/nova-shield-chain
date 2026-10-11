# CoW Protocol — Agent 24 Analizi
# Kapsam: eth-domain-types, ethrpc, number, request-sharing, serde-ext, bytes-hex, chain, http-client, signature-validator, account-balances

**Tarih:** 2026-07-22
**Toplam dosya:** 37 Rust kaynak dosyası
**Kapsanan crate'ler:** 10 adet utility/infrastructure crate

---

## 1. eth-domain-types Crate'i (7 dosya)

### 1.1 `src/lib.rs` — Tip Kataloğu ve Re-export'lar (satır 1-152)

**Re-exportlar (satır 1-10):**
```rust
pub use {
    access_list::{AccessList, StorageKey},
    allowance::Allowance,
    alloy_primitives::{Address, B256, U256, U512},
    eip712::DomainSeparator,
    ether::Ether,
    gas::{EffectiveGasPrice, FeePerGas, Gas, GasPrice},
    number::nonzero::NonZeroU256,
    token_amount::{SellTokenAmount, SurplusTokenAmount, TokenAmount},
};
```

Bu crate, dışarıya tek bir giriş noktası sunuyor: alloy_primitives'den Address, B256, U256, U512 re-export ediyor, kendi tanımladığı tip sarmalayıcılarını (newtypes) da eklıyor. Tasarım kararı: **alloy_primitives üzerine domain-specific sarmalayıcı katmanı** inşa etmek — bu Rust'ta yanlış kullanımı derleme zamanında önleme (type-safe API) için klasik bir pattern.

**Temel Struct'lar:**

`ContractAddress(Address)` — satır 34: Akıllı kontrat adresi. `Deref` implement ediyor, iç `Address`'e ulaşım var. Henüz inşa sırasında kontrat doğrulaması yapılmıyor (TODO satır 31).

`TokenAddress(Address)` — satır 40: ERC-20 token adresi. `as_erc20` metodu (satır 44): ETH pseudo-token ise WETH'e çeviriyor.

`WrappedNativeToken(TokenAddress)` — satır 52: WETH (mainnet) veya WXDAI (Gnosis) gibi native token'ın ERC-20 sarmalayıcısı.

`ETH_TOKEN` sabiti — satır 28: `0xEEEE...EEEE` adresi. Ethereum ekosisteminde ETH'yi ERC-20 gibi temsil etmek için kullanılan konvansiyonel adres.

`Asset` struct (satır 87-90):
```rust
pub struct Asset {
    pub amount: TokenAmount,
    pub token: TokenAddress,
}
```
Belirli bir miktarda belirli bir token'ı temsil ediyor.

`BlockNo(u64)` — satır 94: Block numarası wrapper. `num::Saturating` impl'i var (satır 111) — taşma yerine saturate etmek önemli zira blok numarası asla negatif olamaz.

`TxId(B256)` — satır 123: Transaction hash.

`Tx` struct (satır 127-134):
```rust
pub struct Tx {
    pub from: Address,
    pub to: Address,
    pub value: Ether,
    pub input: Bytes,
    pub access_list: AccessList,
}
```
`From<Tx> for TransactionRequest` impl'i (satır 142) alloy'un `TransactionRequest` tipine dönüşüm sağlıyor. Debug türevi `access_list`'i ignore ediyor (satır 133) — bu alanlarda sıralama deterministik olmayabileceğinden doğru tercih.

---

### 1.2 `src/access_list.rs` — EIP-2930 Access List (satır 1-94)

**EIP-2930 standardı:** Transaction'lara, hangi adreslere ve storage slot'larına erişeceklerini önceden belirterek gas maliyetini düşürme imkanı tanır.

`StorageKey(B256)` — satır 8: Storage key sarmalayıcısı, `Into`, `From` derive'lı.

`AccessList(HashMap<Address, HashSet<StorageKey>>)` — satır 15:
- Iç yapı: address → set of storage keys. HashMap + HashSet kombinasyonu, duplikasyonu önliyor (EIP-2930 gereksinimi).
- `is_empty()` metodu — satır 18.
- `merge(mut self, other: Self) -> Self` metodu (satır 25): İki access list'i birleştiriyor. `or_default().extend(storage_keys)` pattern'i — var olan adres için storage key'leri extend ediyor, yoksa yeni entry ekliyor. Bu işlem **DEX settlement için kritik**: çoklu token transferleri farklı access list girdileri oluşturabilir, hepsini birleştirmek gerekir.
- `FromIterator<(Address, I)>` impl'i (satır 42): Iterator'dan inşa.
- `From<alloy_eips::eip2930::AccessList>` (satır 60) ve tersi (satır 80): alloy'un kendi access list tipiyle çift yönlü dönüşüm.

**Tasarım notu:** Set semantiği sayesinde aynı adres + storage key çifti hiç tekrarlanmaz. Alloy'un `AccessList`'i sıralı vector kullanıyor ama bu tip set kullanıyor — bu farklı semantik seçim, CoW'un iç kodunun duplikasyon konusunda daha güçlü garantiler vermesini sağlıyor.

---

### 1.3 `src/allowance.rs` — ERC-20 Allowance Yönetimi (satır 1-63)

EIP-20'nin `allowance()` fonksiyonunun domain modeli.

```rust
pub struct Allowance {
    pub token: TokenAddress,
    pub spender: Address,
    pub amount: U256,
}
```

Üç newtype sarmalayıcı:
- `Existing(pub Allowance)` — satır 23: Şu an zincirde aktif olan allowance.
- `Required(pub Allowance)` — satır 27: Bir işlem için gereken allowance.
- `Approval(pub Allowance)` — satır 44: Yapılması gereken approve call'u.

`Required::approval(&self, existing: &Existing) -> Option<Approval>` (satır 32):
```rust
if self.0.spender != existing.0.spender || self.0.amount <= existing.0.amount {
    None
} else {
    Some(Approval(self.0))
}
```
Mantık: Mevcut allowance yeterliyse veya spender farklıysa `None` döner (onaya gerek yok). Eksikse `Approval` döner. Farklı spender durumu `None` dönmesi kulağa garip gelebilir — bu kasıtlı bir tasarım: eğer spender farklıysa bu required'daki spender için yeni bir allowance gerekir, mevcut allowance bunu zaten karşılamıyor demektir ve approval gereklidir... Aslında bu durumda `None` yerine `Some` dönmeli gibi görünüyor; ancak belki spender kontrolü sadece aynı spender için karşılaştırma yapmak amacıyla ekleniyor. Bu bir tasarım tartışması noktası.

`Approval::max(self) -> Self` (satır 49): `U256::MAX` ile maksimum approve.
`Approval::revoke(self) -> Self` (satır 57): `U256::ZERO` ile onayı iptal etme.

---

### 1.4 `src/token_amount.rs` — Token Miktarı Tipleri (satır 1-126)

Üç farklı token miktarı tipi:

`TokenAmount(U256)` — genel token miktarı.
`SurplusTokenAmount(U256)` — artık miktarı (buy token için sell order, sell token için buy order).
`SellTokenAmount(U256)` — satış token miktarı.

`TokenAmount::apply_factor(&self, factor: f64) -> Option<Self>` (satır 15):
```rust
Some(self.0.checked_mul_f64(factor)?.into())
```
`U256Ext::checked_mul_f64` kullanıyor. Floatla ölçekleme — DEX fiyat hesaplamalarında kritik.

**Aritmetik trait'leri:** `Sub`, `CheckedSub`, `Mul`, `CheckedMul`, `CheckedAdd`, `Div`, `CheckedDiv`, `Add`, `AddAssign`, `Zero` — hepsi U256 delegate ediliyor. `CheckedXxx` varyantları `None` dönerek overflow güvenliği sağlıyor.

**DEX Uygulama Notu:** Token miktarları için ayrı tip kullanmak (SellTokenAmount vs TokenAmount) Rust tip sistemi aracılığıyla karıştırma hatalarını derleme zamanında yakalar. Nova Shield DEX'te de bu pattern kullanılmalı: `BuyAmount`, `SellAmount`, `FeeAmount` gibi newtype'lar.

---

### 1.5 `src/eip712.rs` — EIP-712 Domain Separator (satır 1-9)

```rust
pub struct DomainSeparator(pub [u8; 32]);
```
EIP-712 `domainSeparator` değerini tutan minimal sarmalayıcı. `[u8; 32]` = 32 byte = 256 bit hash. Metodsuz, sadece veri taşıyıcısı.

**EIP-712 önemi DEX için:** Order'ların imzalanmasında kullanılan typed data hashing standardı. `domainSeparator = keccak256(abi.encode(DOMAIN_TYPEHASH, name, version, chainId, verifyingContract))`. CoW Protocol tüm order hash'lerini bu separator üzerinden hesaplıyor.

---

### 1.6 `src/gas.rs` — Gas Hesaplama Tipleri (satır 1-151)

**EIP-1559 gas modeli implementasyonu:**

`Gas(U256)` — satır 13: Gas birimi miktarı. `Display`, `Add` impl.

`GasPrice` struct (satır 33-42):
```rust
pub struct GasPrice {
    max: FeePerGas,     // maxFeePerGas
    tip: FeePerGas,     // maxPriorityFeePerGas
    base: u64,          // baseFeePerGas (mevcut block)
}
```

`GasPrice::effective() -> EffectiveGasPrice` (satır 46):
```rust
U256::from(calc_effective_gas_price(
    u128::try_from(self.max.0.0)...,
    u128::try_from(self.tip.0.0)...,
    Some(self.base),
))
```
alloy_eips'in `calc_effective_gas_price` fonksiyonunu kullanıyor. EIP-1559 formülü: `min(max, baseFee + tip)`.

`GasPrice * f64` (satır 74): Hem max hem tip çarpılıyor, base değişmiyor. `mul_ceil` kullanıyor — taşma/yuvarlamada ceil tercih (daha fazla fee ödemek daha az fee ödemekten iyidir güvenlik açısından).

`FeePerGas(Ether)` — satır 91: Gas başına fee. Ether cinsinden. `mul_ceil(rhs: f64)` (satır 95):
```rust
U256::from((f64::from(self.0.0) * rhs).ceil()).into()
```
f64'e çevirip çarpıp tavan alıyor.

`Gas * FeePerGas = Ether` (satır 120): Fiziksel birim analizi doğru: gas_units × ETH/gas = ETH.

`EffectiveGasPrice(Ether)` — satır 132: Settlement'ta gerçekten ödenen gas fiyatı.

**DEX Uygulama Notu:** Nova Shield DEX'te transaction cost hesaplaması için bu tip hiyerarşisi örnek alınabilir. Özellikle `Gas * FeePerGas → Ether` operator overloading pattern'i tip güvenli maliyet hesaplaması için mükemmel.

---

### 1.7 `src/ether.rs` — Ether Tipi (satır 1-67)

```rust
pub struct Ether(pub U256);
```
Derive'lar: `Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, From, Into, Display, Default, derive_more::Add, derive_more::Sub`.

`From<Ether> for num::BigInt` (satır 30): BE bytes üzerinden dönüşüm.
`num::Saturating` impl (satır 36): Saturating add/sub — blok ödülleri toplarken taşmayı önler.
`num::CheckedSub` (satır 46): Checked subtract — negatif ETH miktarı olmasın.
`num::Zero` (satır 52): `U256::ZERO` ile sıfır kontrolü.
`std::iter::Sum` (satır 62): Iterator'daki tüm Ether değerlerini toplamak için.

---

## 2. ethrpc Crate'i (10 dosya)

### 2.1 `src/lib.rs` — Ana Giriş Noktası (satır 1-186)

**Sabitler ve tipler:**
- `MAX_BATCH_SIZE: usize = 100` (satır 8)
- `AlloyProvider = DynProvider` (satır 10): Type alias, alloy provider dinamik dispatch.

`Web3` struct (satır 19-22):
```rust
pub struct Web3 {
    pub provider: AlloyProvider,
    pub wallet: MutWallet,
}
```
`Web3`'ün eski `web3` kütüphanesinden `alloy`'a geçiş wrapper'ı olduğu belgelerde açıkça söyleniyor (satır 12-15). Clone + Debug derive'lı.

`Config` struct (satır 41-53):
```rust
pub struct Config {
    pub ethrpc_max_batch_size: usize,          // default: 20
    pub ethrpc_max_concurrent_requests: usize, // default: 10
    pub ethrpc_batch_delay: Duration,          // default: Duration::ZERO
}
```

`web3(args: Config, url: &Url, label: Option<&str>) -> Web3` (satır 66):
- `(0 | 1, 0)` batch size ve concurrent 0 → unbuffered_provider
- Diğer durumlar → batching provider

`RpcClientRandomIdExt` trait (satır 92): `with_random_id` metodu. Önce `u32` random üretip u64'e cast ediyor — u64::MAX overflow'u önlemek için küçük başlangıç noktası (satır 104).

`ProviderSignerExt` trait (satır 110): `without_wallet()` metodu — anvil impersonation için cüzdansız provider.

Test utility (`test-util` feature, satır 132-184): `ProviderExt::send_and_watch` ve `CallBuilderExt::send_and_watch` — transaction gönderip confirm beklemek için. `DEFAULT_WATCH_TIMEOUT: Duration = from_secs(2)` (satır 144) — test için kısa timeout.

---

### 2.2 `src/alloy/mod.rs` — Provider Builder (satır 1-186)

Katman mimarisi (içten dışa, innermost → outermost):
1. `RpcHeadersLayer` (en iç, HTTP'ye en yakın)
2. `BatchCallLayer`
3. `TracingRequestIdLayer`
4. `InstrumentationLayer`
5. `LabelingLayer`

`rpc()` fonksiyonu (satır 27): Tam katmanlı RPC client.
`unbuffered_rpc()` (satır 45): BatchCallLayer olmadan — yüksek frekanslı zincirler için.

`provider()` (satır 77) ve `unbuffered_provider()` (satır 62): `ProviderBuilder`'a `wallet` + `simple_nonce_management` ekleyip `erased()` ile dinamik dispatch. `with_simple_nonce_management()` yorumu (satır 82-83): Her seferinde node'dan nonce sorgular — overhead var ama alloy ile paralel kullanım kolaylaştırıyor.

---

### 2.3 `src/alloy/buffering.rs` — Batch RPC Katmanı (satır 1-322)

**Tasarım:** Node'un native batch RPC'sini kullanıyor (alloy'un MultiCall3 tabanlı batching'i değil). Individual request'leri kuyruğa alıp bir background task batch'leyerek gönderiyor.

`BatchCallLayer` struct (satır 34): `config: Config` tutuyor.

`BatchRequestEntry` struct (satır 81-111):
```rust
struct BatchRequestEntry {
    value: Option<ResponseSender>,
    duplicates: VecDeque<ResponseSender>,
}
```
FIFO semantiği: İlk gelen sender `value`'da, sonrakiler `duplicates` dequeue'da. Duplike ID'ler için özel handling — çünkü random u32→u64 ID'ler zamanla overlap edebilir.

`BatchCallProvider<S>` (satır 63-66):
```rust
pub(crate) struct BatchCallProvider<S> {
    inner: PhantomData<S>,
    calls: mpsc::UnboundedSender<CallContext>,
}
```
`PhantomData<S>` — gerçek service background task'ta yaşıyor, provider sadece channel sender'ı tutuyor.

`background_worker` (satır 151-235):
```rust
tokio::task::spawn(
    calls
        .chunks_timeout(config.ethrpc_max_batch_size, config.ethrpc_batch_delay)
        .for_each_concurrent(config.ethrpc_max_concurrent_requests, process_batch),
)
```
`chunks_timeout`: max_batch_size dolana kadar veya batch_delay geçene kadar bekle (Nagle algoritması benzeri).
`for_each_concurrent`: max_concurrent_requests kadar paralel batch gönder.

**Hata yönetimi (satır 219-226):** Batch başarısız olursa tüm sender'lara aynı error gönderiliyor. Partial failure yok — ya hepsi başarılı ya hepsi başarısız.

**Duplikat response yönetimi (satır 175-184):** Aynı ID'ye gelen response FIFO sırasıyla dağıtılıyor. Node'un response sırasını bozduğu durumda yanlış eşleşme yapılabilir (yorum satır 164: "hope the node didn't re-order responses").

**Testler (satır 290-322):** `test_batch_request_entry_pop_twice` ve `test_batch_request_entry_add_element_pop_thrice` — FIFO davranışını doğruluyor.

**DEX Uygulama Notu:** Nova Shield DEX'te on-chain veri okuma (balance, allowance, pool state) için bu tür batch RPC kritik. Özellikle price estimation'da çok sayıda token çifti için tek round-trip. Nagle delay ayarlanabilir olması önemli — düşük latency zincirler için 0, yüksek throughput senaryolar için birkaç ms.

---

### 2.4 `src/alloy/errors.rs` — Hata Sınıflandırma (satır 1-185)

**Temel sorun:** alloy'da contract error ile node/transport error arasındaki sınır belirsiz.

`ContractErrorExt` trait (satır 12):
- `is_contract_error()`: Transport değilse contract error.
- `is_node_error()`: `TransportError(RpcError::ErrorResp)` ve revert data yoksa node error.
- `is_contract_revert()`: Hem revert data varsa, hem `code == 3` (geth standardı), hem "revert" içeriyorsa, hem `InvalidFEOpcode` içeriyorsa true. Rate limit, transport failure → false.

`is_node_error` implementasyonu (satır 43-57):
```rust
ContractError::TransportError(RpcError::ErrorResp(err)) => {
    let no_revert_data = err.as_revert_data().is_none();
    no_revert_data  // revert data yoksa node error
}
ContractError::TransportError(_) => true,
_ => false,
```

`is_contract_revert` (satır 60-88): Çok detaylı case analizi:
- Revert data var → revert
- `code == 3` → revert (geth standard)
- mesajda "revert" var → revert
- `InvalidFEOpcode` → revert (eski Solidity missing selector)
- `ZeroData`, `UnknownFunction`, `UnknownSelector` → revert
- Rate limit (429), transport → revert değil

Testler (satır 108-184): Her case için ayrı test. `error_resp(code, message)` helper. Özellikle "execution reverted" mesajlarının case-insensitive matching'i test ediliyor.

---

### 2.5 `src/alloy/evm_ext.rs` — EVM Namespace Extensions (satır 1-69)

Test/geliştirme ortamı için `evm_*` RPC metodlarını sarmalayan trait:

`EvmProviderExt` trait (satır 11):
- `evm_set_automine(automine: bool)` — satır 14: Auto-mining aç/kapat.
- `evm_set_next_block_timestamp(timestamp: BlockTimestamp)` — satır 21: Sonraki block'un timestamp'ini set et.
- `evm_set_interval_mining(interval_ms: u64)` — satır 28: Interval mining.
- `evm_set_block_gas_limit(gas_limit: u64) -> Result<bool>` — satır 35: Block gas limit.

Her biri `raw_request` ile node'a gönderiliyor. Bu metodlar genellikle anvil/hardhat gibi test node'larında mevcut.

---

### 2.6 `src/alloy/instrumentation.rs` — Metrik ve Logging Katmanı (satır 1-245)

**İki katman:** `LabelingLayer` ve `InstrumentationLayer`.

`LabelingLayer` (satır 26): Her request'e label ekliyor. `LabeledProvider::attach_label` (satır 52): `req.meta_mut().extensions_mut().get_or_insert_default::<ProviderLabel>().append(&self.label)`. Extension system kullanılıyor — alloy request'lere metadata eklemek için extension slot destekliyor.

`ProviderLabel` (satır 181): String wrapper. `append` (satır 191): Label'ı başa ekliyor (`insert_str(0, label)`) — yani katmanlar iç içe geçtikçe en dıştaki label sona geliyor, en içteki başa. `From<ProviderLabel> for String` (satır 196): Son underscore'u siliyor — `"main_balance_"` → `"main_balance"`.

`InstrumentationLayer` (satır 85): `Metrics` instance'ı alıyor.

`Metrics` struct (satır 206):
```rust
struct Metrics {
    requests_inflight: prometheus::IntGaugeVec,   // labels: component, method
    requests_complete: prometheus::IntCounterVec,  // labels: component, method
    requests_duration_seconds: prometheus::HistogramVec, // labels: component, method
    inner_batch_requests_initiated: prometheus::IntCounterVec, // labels: component, method
}
```

`on_request_start` (satır 229): `#[must_use]` — drop guard. `scopeguard::guard` ile drop sırasında inflight dec, complete inc, timer stop.

`ProviderLabelingExt::labeled` (satır 154): Mevcut Web3 üzerine yeni label katmanı ekler. Yeni `RpcClient` oluşturur, aynı wallet'ı klonlar.

---

### 2.7 `src/alloy/rpc_headers.rs` — HTTP Header Ekleyen Katmanlar (satır 1-265)

**İki header:**
- `x-rpc-calls`: Comma-separated `method:id` listesi (örn: `"eth_call:1,eth_getBalance:2"`)
- `x-request-id`: Distributed tracing request ID

`RpcHeadersLayer` (satır 25): En içteki katman — batchlenmiş paketi görüyor. `x-rpc-calls` header'ını ekliyor.

`TracingRequestIdLayer` (satır 70): BatchCallLayer'dan dışarıda — caller'ın task'ında çalışıyor, span'daki request ID'yi capture ediyor (satır 99). Background task'ta span kaybolacağı için bu önemli.

`annotate(req: &mut RequestPacket)` (satır 122): Ana fonksiyon:
1. Tek pass'ta hem `calls` string'ini hem `request_ids` string'ini build ediyor.
2. Duplike request ID'leri deduplicate ediyor (satır 141).
3. Per-sub-request tracing ID'leri kaldırıp aggregate olanı sadece son request'e ekliyor (satır 153-167) — çünkü batch packet HTTP header'ları tüm sub-request'lerin header union'ı.

Testler (satır 174-264): Single call, batch, tracing ID aggregation, deduplication — hepsi test ediliyor.

---

### 2.8 `src/alloy/wallet.rs` — Mutable Wallet (satır 1-133)

**Problem:** alloy's `EthereumWallet` not mutüel — CoW Protocol runtime'da signer eklemek istiyor.

`MutWallet(Arc<RwLock<EthereumWallet>>)` (satır 16): `Arc` ile cheap clone, `RwLock` ile mutable access.

`register_signer` (satır 29): Sync fonksiyon ama async RwLock var. `handle_blocking_operation` ile çözüm:
- `CurrentThread` runtime → yeni OS thread'de blocking lock (satır 77).
- `MultiThread` runtime → `block_in_place` (satır 81).

`NetworkWallet<N>` impl (satır 88):
- `default_signer_address()`, `has_signer_for()`, `signer_addresses()` — sync, `handle_blocking_operation` ile.
- `sign_transaction_from()` — async, `read().await` ile.

**Default signer logic** (satır 41-46): `default_signer_address() == Address::default()` ise yeni signer'ı default olarak kaydet; değilse normal `register_signer`.

---

### 2.9 `src/block_stream.rs` — Block İzleme (satır 1-416)

`BlockInfo` struct (satır 24-34):
```rust
pub struct BlockInfo {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub gas_limit: U256,
    pub gas_price: U256,
    pub base_fee: u64,
    pub observed_at: Instant,
}
```
`observed_at: Instant` — koddaki timestamp değil, sistemin ne zaman gördüğü. `PartialEq` impl'i (satır 51) `observed_at`'ı dışarıda bırakıyor — aynı block farklı zamanlarda görülebilir.

`TryFrom<alloy_rpc_types::Header>` (satır 70): `base_fee_per_gas` olmayan block → error (EIP-1559 öncesi chain desteklemiyor).

`current_block_ws_stream` (satır 105): **WebSocket tabanlı, ana API.**
1. WS bağlantı kur → subscription başlat.
2. HTTP ile ilk block'u fetch et (race condition'ı minimize etmek için önce subscribe, sonra HTTP fetch).
3. `watch::channel` ile receiver dağıt.
4. Background task: WS stream'den block'ları al, sadece yeni numaralıları forward et.
5. `alloy_transport_ws::WsConnect` auto-reconnect sağlıyor.

`current_block_stream` (satır 204): `#[deprecated]` polling tabanlı eski API. Unbuffered provider kullanıyor (batch yapmadan).

`mock_single_block(block: BlockInfo) -> CurrentBlockWatcher` (satır 266): Test için. `std::mem::forget(sender)` — sender hiç drop olmasın ki receiver açık kalsın.

`CurrentBlockWatcher = watch::Receiver<BlockInfo>` (satır 273): Clonable stream, birden fazla consumer için tek subscription.

`next_block(current_block: &CurrentBlockWatcher) -> BlockInfo` (satır 357): İlk değeri skip ediyor (stream her zaman anında mevcut değeri yayıyor), gerçekten yeni block bekliyor.

`Metrics` (satır 322): `block_stream_update_delta` histogram (positive/negative sign label), `last_block_number` gauge.

---

### 2.10 `src/mock.rs` — Test Mock (satır 1-30)

`Web3::with_asserter(asserter: Asserter) -> Self` (satır 9): alloy'un `Asserter` mock provider'ını kullanıyor.

`web3() -> Web3` (satır 22): Boş mock provider.

---

## 3. number Crate'i (8 dosya)

### 3.1 `src/lib.rs` — Modül Tanımları (satır 1-7)

Sadece 7 satır, tüm modülleri public olarak expose ediyor: conversions, nonzero, ratio_ext, serialization, testing, u256_ext, units.

---

### 3.2 `src/u256_ext.rs` — U256 Extension Trait (satır 1-215)

**DEX İçin En Kritik Dosya — 3x Hassasiyet**

`U256Ext` trait (satır 9-51):

**`checked_ceil_div(&self, other: &Self) -> Option<Self>` (satır 11):**
Tavan bölme: `⌈numerator / denominator⌉ = (numerator + denominator - 1) / denominator`

Implementation (satır 54-55):
```rust
fn checked_ceil_div(&self, other: &Self) -> Option<Self> {
    (!other.is_zero()).then(|| self.div_ceil(*other))
}
```
alloy_primitives'in `div_ceil` metodunu kullanıyor. Sıfırla bölme → None.

**DEX Önemi:** AMM fee hesaplamalarında tavan bölme kritik. Örneğin: `fee = ceil(amount * fee_rate / PRECISION)` — tavan almak protokol lehine, trader aleyhine olduğu için protokolün para kaybetmemesini garantiler.

**`checked_mul_f64(&self, factor: f64) -> Option<Self>` (satır 58-88):**

Matematiksel algoritma:
```
result = (self * round(factor * 10^18)) / 10^18
```

Adımlar:
1. `factor.is_finite() && !factor.is_sign_negative()` kontrolü (satır 59).
2. `factor == 1.0` → identity shortcut (satır 65) — büyük değerlerde intermediate overflow önleme.
3. `scaled_factor = factor * 10^18` (satır 70-83).
4. `scaled_factor ≤ u128::MAX` → direct U256::from, değilse BigUint (satır 75-83).
5. `(self * scaled_factor) / 10^18` (satır 85-87).

**Neden BigRational kullanılmıyor (satır 26-30):**
BigRational f64'ün binary semantiğini koruyor — `0.1 * 10^18` = `100000000000000001` gibi binary representation artifact'ları. Integer scale approach decimal intent'e daha uygun.

**Testler (satır 99-214):** Çok kapsamlı:
- `25 ether * 0.2 = 5 ether` (exact)
- `1.0` identity
- `0.0` → zero
- Negatif, NaN, infinity → None
- `0.125 = 1/8` exact binary representation
- `0.00390625 = 1/256` exact binary
- `U256::MAX * 1.1` → overflow → None

**`to_big_rational(&self) -> BigRational` (satır 90):**
`BigRational::new(self.into(), 1.into())` — U256'yı tam kesirli sayı olarak.

**`from_big_int(input: &BigInt) -> Option<Self>` (satır 94):**
`U256::try_from(input).ok()` — negatif veya büyük → None.

**`from_big_rational(value: &BigRational) -> Option<Self>` (satır 44):**
`numer / denom` — floor division, tam kısım alınıyor.

**DEX Uygulama Notu:** Nova Shield DEX'te fiyat hesaplamalarında bu fonksiyonlar doğrudan kullanılabilir:
- `TokenAmount::apply_factor` → slippage hesaplaması: `buy_amount.apply_factor(1.0 - slippage_tolerance)`
- `checked_ceil_div` → fee hesabı: `fee = ceil(trade_amount * fee_bps / 10000)`
- `checked_mul_f64` → price impact: `output = input.checked_mul_f64(exchange_rate)?`

**Nasıl daha iyi yapılır:** Şu an `factor * 10^18` önce f64 çarpması yapıyor, bu yüksek factor değerlerinde (>9e15) f64 precision kaybına yol açabilir. Alternatiif: factor'ı `num::BigDecimal` olarak alıp tam aritmetik yapmak. Ancak API değişimi gerektirir.

---

### 3.3 `src/conversions.rs` — Tip Dönüşüm Fonksiyonları (satır 1-269)

Kapsamlı conversion layer: `BigDecimal`, `BigRational`, `BigInt`, `BigUint`, `U256`, `U160`, `I512` arası dönüşümler.

**Temel fonksiyonlar:**

`big_decimal_to_big_uint(big_decimal: &BigDecimal) -> Option<BigUint>` (satır 11):
`to_bigint().try_into().ok()` — negatif veya kesirli → None.

`rational_to_big_decimal<T>(value: &Ratio<T>) -> BigDecimal` (satır 18):
`BigDecimal::new(numer, 0) / BigDecimal::new(denom, 0)` — tam kesirli sayı bölmesi.

`big_decimal_to_big_rational(value: &BigDecimal) -> BigRational` (satır 28):
```rust
let (numer, scale) = value.as_bigint_and_exponent();
match scale.cmp(&0) {
    Equal    => (numer, 1),
    Greater  => (numer, 10^scale),  // 1.23e-2 → 123/10000
    Less     => (numer * 10^(-scale), 1),  // 1e2 → 100/1
}
```
BigDecimal'ın iç yapısına (numer + exponent) doğrudan erişiyor.

`big_uint_to_u256(input: &BigUint) -> Result<U256>` (satır 42):
BE bytes → `ensure!(bytes.len() <= 32)` → `U256::from_be_slice`.

`big_int_to_u256(input: &BigInt) -> Result<U256>` (satır 48):
`ensure!(sign != Minus)` → `big_uint_to_u256(magnitude)`.

`u256_to_big_uint(input: &U256) -> BigUint` (satır 66):
`BigUint::from_bytes_be(&input.to_be_bytes::<32>())`.

`i512_to_big_int(i512: &I512) -> BigInt` (satır 101):
Sign match + abs bytes. I512 ← signed 512-bit integer, Uniswap V3 tick math gibi hesaplamalarda kullanılır.

`i512_to_u256(input: &I512) -> Result<U256>` (satır 116):
`ensure!(input >= ZERO && input < U256::MAX)` → `from_be_slice(&bytes[32..])` — upper 32 bytes of I512.

**DEX Uygulama Notu:** BigRational tabanlı price calculation pipelines için bu conversions foundation teşkil ediyor. Özellikle `u256_to_big_rational` → BigRational arithmetic → `big_rational_to_u256` döngüsü kayıpsız fiyat hesaplamalarını mümkün kılıyor.

---

### 3.4 `src/serialization.rs` — U256 Serializasyon (satır 1-28)

`HexOrDecimalU256`: `serde_with` uyumlu helper.
- Deserialize: U256::deserialize (alloy's default — hex veya decimal)
- Serialize: `source.to_string()` — decimal string (alloy by default hex serialize eder!)

**Önemli:** alloy'un `U256` by default hex serialize eder. Bu tip serialize ederken decimal string üretir — API response'larında human-readable decimal için.

---

### 3.5 `src/ratio_ext.rs` — Ratio Extension (satır 1-38)

`RatioExt<U256>` trait (satır 6):

```rust
const ZERO: Ratio<U256> = Ratio::new_raw(U256::ZERO, U256::ONE);
const ONE: Ratio<U256> = Ratio::new_raw(U256::ONE, U256::ONE);
```

`scalar_mul(&self, scalar: U256) -> Option<U256>` (satır 24):
```rust
scalar.checked_mul(*self.numer())?.checked_div(*self.denom())
```
Overflow check: çarpma veya bölme fail olursa None. Sıra önemli: önce `scalar * numer`, sonra `/ denom`. Orta sonuç U256'yı taşabilir.

`full_scalar_mul(&self, scalar: U256) -> Option<U256>` (satır 30):
```rust
U256::uint_try_from(
    scalar.widening_mul(*self.numer())     // U256 × U256 → U512
        .checked_div(U512::from(*self.denom()))?  // U512 / U512 → U512
)
```
**Kritik AMM matematiği:** `widening_mul` — çarpma sonucunu U512'ye genişleterek overflow engelliyor. Sonra U512 bölme, sonra U256'ya geri cast. Bu **Uniswap V3 style fixed-point matematik** pattern'i.

**DEX Uygulama Notu:** Nova Shield AMM'de price ratio uygulaması için `full_scalar_mul` şablonu:
```
output = ceil(input * price_numerator / price_denominator)
```
Intermediate U512 kullanmak, U256 sınırı yakın büyük token miktarlarında (USDC/WBTC gibi) kayıpsız hesaplama yapıyor. **Bu pattern kesinlikle Nova Shield'da kullanılmalı.**

**Nasıl daha iyi yapılır:** `full_scalar_mul` intermediate taşma yoktur ama final sonuç U256'ya sığmazsa None döner. Ceiling versiyonu da eklenmeli: `ceil((scalar * numer) / denom) = (scalar * numer + denom - 1) / denom`.

---

### 3.6 `src/nonzero.rs` — NonZeroU256 (satır 1-76)

```rust
pub struct NonZeroU256(U256);
impl NonZeroU256 {
    pub const MAX: Self = NonZeroU256(U256::MAX);
    pub const ONE: Self = NonZeroU256(U256::ONE);
    pub fn new(value: U256) -> Option<Self> {
        (!value.is_zero()).then_some(Self(value))
    }
}
```

`TryFrom<U256>`: hata mesajı "Value cannot be zero!".
`Default`: ONE döner (sıfır olamaz).
Serialize: decimal string. Deserialize: U256::deserialize → NonZeroU256::try_from.
`TryFrom<u128>`: u128 → U256 → NonZeroU256.

**DEX Önemi:** Fiyat hesaplamalarında denominator sıfır olamaz. `NonZeroU256` tip düzeyinde garantisi veriyor — runtime divide-by-zero önleme yerine compile-time.

---

### 3.7 `src/units.rs` — Ethereum Birim Yardımcıları (satır 1-42)

`EthUnit` trait (satır 3):
```rust
fn atom(self) -> U256;          // temel birim (wei)
fn matom(self) -> U256;         // Mwei = atom * 10^6
fn gatom(self) -> U256;         // Gwei = atom * 10^9
fn eth(self) -> U256;           // Ether = atom * 10^18
```

Impl'ler: `u64`, `u128` — integer birim dönüşümü.
`f64` için `eth()` (satır 39-41): `(self * 1e18) as u128` — f64 → u128 truncation. Küçük değerlerde yeterli ama büyük değerlerde hassasiyet kaybı var.

---

### 3.8 `src/testing.rs` — Test Utility (satır 1-197)

`ApproxEq` trait (satır 8):
```rust
fn is_approx_eq(&self, other: &Self, delta: Option<f64>) -> bool;
```

Implementation (satır 27-51):
1. `self_, other` → BigRational.
2. Eşit → true.
3. `expected_delta = BigRational::from_f64(delta.unwrap_or(1e-9))`.
4. `diff = |self - other|`.
5. `calculated_delta = diff / |other|`.
6. `calculated_delta <= expected_delta`.

**Dikkat:** `diff / |other|` formülü — `|self - other| / |other|` hesaplıyor. Bu `other` merkezli relative error. `|self - other| / max(|self|, |other|)` daha simetrik olurdu ama bu implementation asimetrik.

Testler çok kapsamlı: u64, u128, U256 için identical, within, exceeds, zero, custom delta.

---

## 4. request-sharing Crate'i (1 dosya)

### 4.1 `src/lib.rs` — Request Deduplication (satır 1-219)

**Problem çözülen:** Aynı pahalı hesaplama (ör: fiyat tahmini) aynı anda birden fazla component tarafından istenirse, her biri ayrı computation yapmak yerine birincisi başlatılan computation'ı share eder.

`RequestSharing<Request, Fut: Future>` struct (satır 32):
```rust
pub struct RequestSharing<Request, Fut: Future> {
    in_flight: Arc<Mutex<HashMap<Request, WeakShared<Fut>>>>,
    request_label: String,
}
```
`WeakShared<Fut>`: `futures::Shared` future'ın weak referansı — future complete olunca GC tarafından temizlenebilir.

`SharedResult<Fut: Future>` (satır 48):
```rust
pub struct SharedResult<Fut: Future> {
    future: Shared<Fut>,
    pub is_shared: bool,
}
```
`Future` impl delegate to inner `Shared<Fut>`.

`shared_or_else<F>(&self, request: Request, future: F) -> SharedResult<Fut>` (satır 126):
```rust
let existing = in_flight.get(&request).and_then(WeakShared::upgrade);
if let Some(existing) = existing {
    // HIT: mevcut in-flight'i paylaş
    return SharedResult { future: existing, is_shared: true };
}
// MISS: yeni future oluştur, insert et
let shared = future(&request).shared();
in_flight.insert(request, shared.downgrade().unwrap());
```
`downgrade().unwrap()` — henüz poll edilmemiş shared future weak'e dönüştürülebilir, bu yüzden unwrap güvenli.

**GC Stratejisi (satır 81-106):** Background task 500ms'de bir `retain(|_, weak| weak.upgrade().is_some())` çalıştırıyor. Basit yaklaşım — büyük in-flight map beklenmiyor, O(n) linear scan yeterli.

`Drop` impl (satır 109): Cache boşaltılınca metrics gauge sıfıra set.

`Metrics` (satır 165): `request_sharing_access` (hits/misses), `request_sharing_cached_items`.

**Test (satır 187-218):** `result1.shared_or_else(0, |_| async { panic!() })` — ikinci call factory'yi çağırmazsa, paylaşım çalışıyor demektir (factory'nin panic etmesi bu durumda kod çalışmaz).

**DEX Uygulama Notu:** Nova Shield DEX'te price estimation'da: aynı token pair için aynı anda 10 farklı component fiyat sorabilir. `RequestSharing` ile tek network call + paylaşım. Özellikle auction süresince (12-15s) aynı pair için sık sorgularda etkili.

---

## 5. serde-ext Crate'i (3 dosya)

### 5.1 `src/lib.rs` — Re-exports (satır 1-6)
```rust
pub use {self::hex::Hex, u256::U256};
```

### 5.2 `src/hex.rs` — Hex Serializasyon (satır 1-93)

`Hex` struct — `serde_with::SerializeAs/DeserializeAs` implement ediyor.

`Vec<u8>` için (satır 10-43):
- Deserialize: `"0x"` prefix kontrolü → `const_hex::decode(&s[2..])`.
- Serialize: `const_hex::encode_prefixed(source)`.

`[u8; N]` için (satır 46-93): Generic const — compile-time boyut kontrolü:
- Decoded bytes `!= N` → hata.
- `copy_from_slice` ile fixed-size array doldurma.

**Neden `const_hex`:** Standart `hex` crate'inden daha hızlı, SIMD kullanıyor.

### 5.3 `src/u256.rs` — U256 Serde (satır 1-46)

`U256` newtype (shadowing alloy's U256):
- Deserialize: `alloy_primitives::U256::from_str_radix(s, 10)` — decimal string.
- Serialize: `source.to_string()` — decimal string.

`bytes-hex/serialization.rs` ile benzer ama U256'ya özel.

---

## 6. bytes-hex Crate'i (1 dosya)

### 6.1 `src/lib.rs` — Bytes Hex Serializasyon (satır 1-77)

İki API: `mod` level fonksiyonlar ve `BytesHex` struct.

`serialize<S, T>(bytes: T, serializer: S)` (satır 9):
```rust
let mut v = vec![0u8; 2 + bytes.as_ref().len() * 2];
v[0] = b'0';
v[1] = b'x';
const_hex::encode_to_slice(bytes, &mut v[2..]).unwrap();
serializer.serialize_str(&String::from_utf8(v).unwrap())
```
Önce buffer allocate, sonra encode — tek allocation.

`deserialize<'de, D>` (satır 23):
```rust
let prefixed_hex_str = Cow::<str>::deserialize(deserializer)?;
let hex_str = prefixed_hex_str.strip_prefix("0x").ok_or(...)? ;
const_hex::decode(hex_str).map_err(D::Error::custom)
```
`Cow<str>` — owned veya borrowed string deserialize edebilir.

`BytesHex` struct (satır 34): `serde_with` adapter — `SerializeAs<T: AsRef<[u8]>>` ve `DeserializeAs<Vec<u8>>` impl.

Test (satır 57-76): `{"b": "0x0001"}` ↔ `S { b: vec![0, 1] }`.

---

## 7. chain Crate'i (1 dosya)

### 7.1 `src/lib.rs` — Chain Enum (satır 1-249)

```rust
#[repr(u64)]
pub enum Chain {
    Mainnet = 1,
    Gnosis = 100,
    Sepolia = 11155111,
    ArbitrumOne = 42161,
    Base = 8453,
    Hardhat = 31337,
    Bnb = 56,
    Avalanche = 43114,
    Optimism = 10,
    Polygon = 137,
    Linea = 59144,
    Plasma = 9745,
    Ink = 57073,
}
```

**Metodlar:**

`id(&self) -> u64` (satır 28): `*self as u64` — `repr(u64)` sayesinde.

`name(&self) -> &'static str` (satır 33): Human-readable isim (ör: "Ethereum / Mainnet", "xDAI").

`as_str(&self) -> &'static str` (satır 55): Kebab-case slug — URL/config'de kullanılan stable identifier.

`default_amount_to_estimate_native_prices_with(&self) -> U256` (satır 74):
- ETH chains (Mainnet, Sepolia, Arbitrum, Base, BNB, Linea, Optimism, Ink): `10^17` = 0.1 ETH
- Gnosis, Avalanche: `10^18` = 1 native token
- Polygon, Plasma: `10^20` = 100 native token
- Hardhat: panic

Bu fark neden? Native token değerleri farklı: 1 ETH ~$3000, 1 MATIC ~$0.5. Fiyat tahmini için kullanılan miktar token'ın fiyatına göre ayarlanıyor.

`block_time_in_ms(&self) -> Duration` (satır 93):
- Mainnet/Sepolia: 12s
- Gnosis: 5s
- ArbitrumOne: 250ms (!)
- Base/Optimism/Polygon/Avalanche/Linea: 2s
- Plasma/Ink: 1s

`blocks_in(&self, time_in_ms: u64) -> f64` (satır 113): `time_ms / block_time_ms`. Test'te: mainnet 6 saat = 1800 block, ArbitrumOne 6 saat = 86400 block.

`TryFrom<u64>` ve `TryFrom<U256>` (satır 118-153): Chain ID'den enum'a dönüşüm.

`Deserialize` impl (satır 156): `deserialize_any` — hem u64 hem string kabul ediyor.

**DEX Uygulama Notu:** Nova Shield multi-chain olacaksa, chain-specific konfigürasyonları bu pattern ile yönetebilir. Özellikle `block_time_in_ms` auction timeout süresini ayarlamak için kritik — ArbitrumOne'da 250ms/block varken 12s auction window 48 block demek.

---

## 8. http-client Crate'i (1 dosya)

### 8.1 `src/lib.rs` — HTTP Client Factory (satır 1-60)

`USER_AGENT: &str = "cowprotocol-services/2.0.0"` (satır 6).

`HttpClientFactory` struct (satır 15-17):
```rust
pub struct HttpClientFactory {
    pub timeout: Duration,
}
```

Metodlar:
- `new(args: &configs::http_client::HttpClient)` (satır 20)
- `create(&self) -> Client` (satır 27): `self.builder().build().unwrap()`
- `configure(&self, config: impl FnOnce(ClientBuilder) -> ClientBuilder) -> Client` (satır 32): Custom config için.
- `builder(&self) -> ClientBuilder` (satır 37): TCP keepalive 60s, user-agent, timeout ayarlı.

`Default` impl (satır 45): 10 saniye timeout.

`From<configs::http_client::HttpClient>` (satır 53): Config'den factory.

**Tasarım:** Factory pattern — her API için ayrı connection pool ama ortak konfigürasyon. `configure` metodu caller'ın ek header veya proxy eklemesine izin veriyor.

---

## 9. signature-validator Crate'i (2 dosya)

### 9.1 `src/lib.rs` — EIP-1271 Signature Validation (satır 1-129)

**EIP-1271:** Smart contract'ların imza doğrulaması için standart. Kontrat `isValidSignature(bytes32 hash, bytes signature) returns (bytes4)` metodunu implement eder. Magic value `0x1626ba7e` döndürmesi imzanın geçerli olduğunu gösterir.

`SignatureCheck` struct (satır 16-22):
```rust
pub struct SignatureCheck {
    pub signer: Address,
    pub hash: [u8; 32],
    pub signature: Vec<u8>,
    pub interactions: Vec<InteractionData>,
    pub balance_override: Option<BalanceOverrideRequest>,
}
```

`requires_setup(&self) -> bool` (satır 53): `!interactions.is_empty() || balance_override.is_some()` — pre-interactions varsa veya balance override varsa simülasyon gerekli.

**Neden simülasyon gerekebilir?** (satır 46-54): Trader bir pre-interaction ile airdrop talep edebilir, bu balance artışını sağlar. Signature doğrulaması bu state'i görmeden valid sonuç vermeyebilir.

`MAGICAL_VALUE: [u8; 4] = hex!("1626ba7e")` (satır 98).

`SignatureValidating` trait (satır 82):
```rust
#[async_trait::async_trait]
pub trait SignatureValidating: Send + Sync {
    async fn validate_signature(&self, check: SignatureCheck) -> Result<(), SignatureValidationError>;
    async fn validate_signature_and_get_additional_gas(&self, check: SignatureCheck) -> Result<u64, SignatureValidationError>;
}
```
`mockall::automock` ile test mock desteği.

`validator()` (satır 116): `Arc<dyn SignatureValidating>` döndürüyor — `simulation::Validator` wrap ediyor.

---

### 9.2 `src/simulation.rs` — Simülasyon Tabanlı Doğrulama (satır 1-194)

`Validator` struct (satır 25-31):
```rust
pub struct Validator {
    signatures_address: Address,
    settlement: GPv2Settlement::Instance,
    vault_relayer: Address,
    web3: Web3,
    balance_overrider: Arc<dyn StateOverriding>,
}
```

`simulate_without_setup` (satır 56-83): Doğrudan `ERC1271SignatureValidator::isValidSignature` call. Magic bytes hex encode edip string karşılaştırması.

`simulate` (satır 92-160): **Karmaşık case — state override + pre-interactions:**

1. `balance_override` varsa `balance_overrider.balance_override(overrides)` ile state override al (satır 96-104).
2. `Signatures::validateCall` hazırla (satır 110-127): settlement address, vault relayer, signer, order hash, signature, interactions.
3. `settlement.simulateDelegatecall(signatures_address, validate_call.abi_encode())` — Settlement kontratından delegate call ile simülasyon (satır 134).
4. ZKSync uyumluluğu: `static SIMULATION_ACCOUNT: LazyLock<Address> = LazyLock::new(|| Address::random())` (satır 131) — ZKSync default 0x0 kullanmadığı için.
5. Response bytes → `sol_data::Uint<256>` decode → gas_used.

**Neden `simulateDelegatecall`?** (satır 105-109): Settlement kontratının bağlamında simüle etmek, pre-interaction'ların settlement'taki davranışını doğru modelliyor. Ayrıca `isValidSignature` call'larını gerçek settlement akışında test ediyor.

`SignatureValidating` impl (satır 164-189):
- `validate_signature`: `requires_setup()` → `simulate()`, değilse `simulate_without_setup()`.
- `validate_signature_and_get_additional_gas`: sadece `simulate()` — her zaman tam simülasyon.

`Simulation { gas_used: U256 }` (satır 192).

---

## 10. account-balances Crate'i (3 dosya)

### 10.1 `src/lib.rs` — Balance Fetching Interface (satır 1-252)

`Query` struct (satır 19-25):
```rust
pub struct Query {
    pub owner: Address,
    pub token: Address,
    pub source: SellTokenSource,
    pub interactions: Vec<InteractionData>,
    pub balance_override: Option<BalanceOverrideRequest>,
}
```
`Hash + Eq` implement ediyor — HashMap key olarak kullanılıyor (cache).

`Query::from_order(o: &Order)` (satır 28): Order'dan query oluşturma. `balance_override: None` — TODO: eventually delete.

`TransferSimulationError` enum (satır 42-47):
- `InsufficientAllowance`
- `InsufficientBalance`
- `TransferFailed(Vec<u8>)` — revert data
- `Other(anyhow::Error)`

`BalanceFetching` trait (satır 57-84):
```rust
#[async_trait::async_trait]
pub trait BalanceFetching: Send + Sync {
    async fn get_balances(&self, queries: &[Query]) -> Vec<anyhow::Result<U256>>;
    async fn can_transfer(&self, query: &Query, amount: U256) -> Result<(), TransferSimulationError>;
    async fn allowance(&self, owner: Address, token: Address, source: SellTokenSource) -> anyhow::Result<U256>;
}
```

`BalanceSimulator` struct (satır 103-108):
```rust
pub struct BalanceSimulator {
    settlement: GPv2Settlement::Instance,
    balances: Balances::Instance,
    vault_relayer: Address,
    balance_overrider: Arc<dyn StateOverriding>,
}
```

`BalanceSimulator::simulate` (satır 129-216): **Core simülasyon:**

```rust
let balance_call = Balances::Balances::balanceCall {
    contracts: Balances::Balances::Contracts {
        settlement: *self.settlement.address(),
        vaultRelayer: self.vault_relayer,
        vault: Address::ZERO,  // deprecated
    },
    trader: owner,
    token,
    amount: amount.unwrap_or_default(),
    source: source.as_bytes().into(),
    interactions: interactions.iter().map(|i| ...).collect(),
};
settlement.simulateDelegatecall(*self.balances.address(), balance_call.abi_encode()).call().await
```

Response decode (satır 185-204):
```rust
let (token_balance, allowance, effective_balance, can_transfer, transfer_revert_reason) =
    <(Uint<256>, Uint<256>, Uint<256>, Bool, Bytes)>::abi_decode_params(&response.0)?;
```
Tuple decode — 5 değer tek seferde. `U256::from_le_slice(&token_balance.as_le_bytes())` — little-endian bytes.

`Simulation` struct (satır 220-226):
```rust
pub struct Simulation {
    pub token_balance: U256,      // Gerçek token bakiyesi
    pub allowance: U256,          // Settlement'a verilen izin
    pub effective_balance: U256,  // min(balance, allowance)
    pub can_transfer: bool,       // Transfer gerçekleşebilir mi?
    pub transfer_revert_reason: Vec<u8>,  // Neden başarısız?
}
```

---

### 10.2 `src/simulation.rs` — Simülasyon Tabanlı Balance (satır 1-202)

`Balances` struct (satır 17-20): `web3: Web3` + `balance_simulator: BalanceSimulator`.

`tradable_balance_simple` (satır 60-75): Pre-interaction yoksa **hızlı path**:
```rust
let balance = token.balanceOf(query.owner);
let allowance = token.allowance(query.owner, self.vault_relayer());
let (balance, allowance) = futures::try_join!(balance.call(), allowance.call())?;
Ok(std::cmp::min(balance, allowance))
```
Paralel RPC: `balanceOf` ve `allowance` aynı anda. `min(balance, allowance)` — gerçek kullanılabilir miktar.

`tradable_balance_simulated` (satır 36-57): Pre-interaction varsa simülasyon path'i. `balance_simulator.simulate()` → `simulation.can_transfer ? effective_balance : ZERO`.

`get_balances` (satır 81-96): Her query için paralel:
```rust
let futures = queries.iter().map(|query| async {
    if query.interactions.is_empty() {
        tradable_balance_simple(...)
    } else {
        tradable_balance_simulated(...)
    }
});
future::join_all(futures).await
```

`can_transfer` (satır 98-129): Simülasyon, sonra 3 ayrı kontrol:
1. `token_balance < amount` → `InsufficientBalance`
2. `allowance < amount` → `InsufficientAllowance`
3. `!can_transfer` → `TransferFailed(revert_reason)`

`allowance` (satır 131-144): Direkt ERC20 `allowance()` call, no simulation.

---

### 10.3 `src/cached.rs` — Önbellekli Balance (satır 1-367)

`EVICTION_TIME: BlockNumber = 5` — satır 20: 5 block görülmeden sorgulanmayan balance'lar cache'den siliniyor.

`BalanceEntry` (satır 69-73):
```rust
struct BalanceEntry {
    requested_at: BlockNumber,
    updated_at: BlockNumber,
    balance: U256,
}
```

`BalanceCache` (satır 23-66):
- `get_cached_balance`: `entry.requested_at = last_seen_block` — hit'te requested_at güncelleniyor (LRU-like).
- `update_balance`: sadece existing entry'leri günceller (background task).
- `insert_balance`: sadece yeni entry'ler ekler (on-demand).
- `update_block < last_seen_block` → noop (stale güncelleme).

`spawn_background_task` (satır 115-157): **Block-triggered cache refresh:**
```rust
while let Some(block) = stream.next().await {
    // 1. Cache'deki aktif query'leri topla (son 5 block içinde sorgulanmış)
    let balances_to_update = cache.data.iter()
        .filter_map(|(q, e)| {
            let oldest = last_seen_block.saturating_sub(EVICTION_TIME);
            (e.requested_at >= oldest).then_some(q.clone())
        });
    // 2. Batch fetch
    let results = inner.get_balances(&balances_to_update).await;
    // 3. Update
    for (query, result) in zip(balances_to_update, results) {
        if let Ok(balance) = result { cache.update_balance(query, balance, block.number); }
    }
    // 4. Evict stale (updated_at < current block)
    cache.data.retain(|_, v| v.updated_at >= block.number);
}
```

`get_balances` impl (satır 162-188): **Cache-aside pattern:**
1. Cache'den hit olanları al.
2. Miss olanları inner'dan fetch et.
3. Yeni sonuçları cache'e ekle.
4. Sonuçları merge edip orijinal sırayla döndür.

Testler (satır 212-367): 5 test:
- `caches_ok_results`: İlk call inner, ikinci cache.
- `does_not_cache_errors`: Error cache'lenmiyor.
- `background_task_updates_cache_on_new_block`: Block gelince cache refresh.
- `can_return_new_and_cached_results_in_same_call`: Mixed cache/fresh aynı response'da.
- `unused_balances_get_evicted`: EVICTION_TIME block geçince evict.

**DEX Uygulama Notu:** Bu cache pattern Nova Shield DEX'te order validity check için ideal. Auction süresi (~12-15s = ~1 Ethereum block) boyunca balance birden fazla kez sorgulanabilir. Tek simülasyon + cache, sonraki sorguları 0 latency ile yanıtlar. Block-triggered refresh ile stale veri riski minimal.

---

## 11. Mimari Özet ve Cross-Crate İlişkiler

```
eth-domain-types ← number (U256Ext, NonZeroU256 kullanıyor)
ethrpc ← eth-domain-types (Tx, BlockInfo)
account-balances ← ethrpc (Web3, CurrentBlockWatcher)
account-balances ← eth-domain-types (Address, U256)
signature-validator ← ethrpc (Web3)
request-sharing ← (bağımsız, sadece futures + prometheus)
serde-ext ← (bağımsız)
bytes-hex ← (bağımsız)
chain ← (bağımsız, alloy_primitives U256)
http-client ← (bağımsız, reqwest)
```

---

## 12. DEX Uygulama Notu — Nova Shield DEX için Direkt Eylem Planı

### 12.1 Ethereum Tip Sistemi (eth-domain-types)

**Hemen alınabilecek pattern'ler:**

1. **Newtype sarmalayıcıları:** `SellAmount(U256)`, `BuyAmount(U256)`, `FeeAmount(U256)` — tip güvenliği için. Rust compiler'ı aynı U256'yı yanlış yerde kullanmayı önler.

2. **ETH pseudo-address:** `const NATIVE_TOKEN: TokenAddress = TokenAddress(Address::repeat_byte(0xee))` — native token'ı ERC-20 gibi temsil etmek. `as_erc20(wrapped)` helper ile transparent dönüşüm.

3. **EIP-1559 gas tipi hiyerarşisi:** `Gas * FeePerGas → Ether` — tip güvenli transaction maliyeti hesabı. Nova Shield'ın kendi gas estimation modülü için.

4. **AccessList merge:** DEX route'larında birden fazla AMM pool'una dokunuluyorsa access list'leri `merge()` ile birleştirmek transaction gas'ını düşürür.

### 12.2 RPC Client (ethrpc)

**Batch RPC zorunlu:** Nova Shield DEX'te per-block döngüsünde onlarca pool state'i okumak gerekecek. `BatchCallLayer` (Nagle delay + max batch size + concurrent) bu işi tek HTTP round-trip'e indirir.

**Katman mimarisi öneri:**
```
Nova Shield RPC Stack:
LabelingLayer("nova-dex") →
InstrumentationLayer(metrics) →
TracingRequestIdLayer →
BatchCallLayer(batch_size=50, delay=5ms, concurrent=20) →
RpcHeadersLayer →
HTTP Transport
```

**Block stream:** `current_block_ws_stream` — WebSocket subscription ile real-time block takibi. Auction'ların block-aligned olması için kritik.

### 12.3 Sayısal Hesaplama (number)

**DEX matematik için temel:**

`RatioExt::full_scalar_mul`:
```
output = scalar.widening_mul(numerator) / denominator
```
U512 intermediate — büyük token miktarlarında kayıpsız hesaplama. Her AMM output calculation'da kullanılmalı.

`U256Ext::checked_ceil_div` — fee hesaplama:
```rust
// Protokol fee: ceil(amount * fee_bps / 10000)
let fee = amount.checked_ceil_div(&U256::from(10000 / fee_bps))?;
```

`U256Ext::checked_mul_f64` — slippage:
```rust
// Minimum output: floor(expected_output * (1 - slippage))
let min_output = expected_output.checked_mul_f64(1.0 - slippage_tolerance)?;
```

`NonZeroU256` — pool rezerv güvencesi:
```rust
struct PoolReserves {
    reserve_in: NonZeroU256,   // pool boşsa trade imkansız
    reserve_out: NonZeroU256,
}
```

### 12.4 Request Sharing (request-sharing)

Nova Shield DEX'te fiyat tahmini ve liquidity sorgusu için:
```rust
let price_sharing: BoxRequestSharing<TokenPair, PriceResult> =
    RequestSharing::labelled("price_estimation".into());

// Her component ayrı cache görmez, aynı in-flight future'ı paylaşır
let price = price_sharing.shared_or_else(
    TokenPair(USDC, WETH),
    |pair| fetch_price_from_dex(*pair)
).await;
```

### 12.5 Balance & Signature (account-balances + signature-validator)

**Order validity için ikili kontrol:**
1. `BalanceFetching::can_transfer` — token var mı, izin var mı?
2. `SignatureValidating::validate_signature` — imza geçerli mi?

**Cache stratejisi:** `account_balances::cached(web3, simulator, blocks)` — block-triggered refresh. Auction süresi içinde birden fazla validity check için ideal.

**EIP-1271 smart contract imzaları:** Multisig (Safe) kullanıcıları için zorunlu. `simulate` path pre-interaction'ları hesaba katıyor — CoW'daki Composable CoW pattern'i Nova Shield'da da uygulanabilir.

### 12.6 Chain Konfigürasyonu

Nova Shield chain registry için `Chain` enum pattern:
```rust
pub enum NovaChain {
    NovaMainnet = 7777,
    NovaTestnet = 7778,
    // + EVM uyumlu chain'ler
    Ethereum = 1,
    ArbitrumOne = 42161,
    ...
}
impl NovaChain {
    fn block_time_ms(&self) -> Duration { ... }
    fn auction_window_blocks(&self) -> u64 {
        // 12-15 saniye / block_time
        15_000 / self.block_time_ms().as_millis() as u64
    }
}
```

---

## 13. Güvenlik ve Kritik Gözlemler

### 13.1 Zayıf Noktalar

1. **`BatchCallProvider` panic stratejisi (buffering.rs satır 143-146):** Channel kapanırsa panic. Kubernetes'te pod restart mantıklı ama development'ta rahatsız edici. Nova Shield'da graceful shutdown alternatifi düşünülebilir.

2. **`full_scalar_mul` asimetrik hata (ratio_ext.rs satır 30):** Intermediate U512 bölme fail olursa None, ama son U256 cast overflow'u da fail eder. İki farklı failure mode aynı None döndürüyor — debug zorlaşıyor.

3. **`simulate_without_setup` magic bytes comparison (simulation.rs satır 78):** String karşılaştırma `hex_string == "1626ba7e"` — byte comparison olsaydı daha hızlı ve daha güvenli.

4. **`is_contract_revert` mesaj bazlı matching (errors.rs satır 74-81):** `"revert"` string içeriyor mu? Bu false positive verebilir. Rate limit mesajı "revert" içerseydi yanlış sınıflandırılırdı. Şu an rate limit test case'i var (satır 175) ama gelecekte benzer kelime içeren mesajlar sorun çıkarabilir.

5. **`BalanceSimulator` `vault: Address::ZERO` (lib.rs satır 158):** Balancer Vault deprecated olarak belirtilmiş — kod içinde izler kalmış, temizlenmeli.

### 13.2 Güçlü Noktalar

1. `WeakShared` GC ile memory leak yok — completed future'lar otomatik temizleniyor.
2. Block-triggered cache eviction — stale balance riski minimal.
3. `requires_setup` branch — basit case için simülasyon overhead yok.
4. `TracingRequestIdLayer` caller span'ı capture etmesi — batch background task'ta distributed trace bilgisi kaybolmuyor.
5. `EthUnit` trait — birim hatalarını önlüyor (wei vs gwei karışıklığı).

---

## 14. Satır Sayısı Özeti

| Crate | Dosya | Toplam satır |
|-------|-------|-------------|
| eth-domain-types | 7 | ~370 |
| ethrpc | 10 | ~1100 |
| number | 8 | ~650 |
| request-sharing | 1 | ~220 |
| serde-ext | 3 | ~100 |
| bytes-hex | 1 | ~78 |
| chain | 1 | ~250 |
| http-client | 1 | ~60 |
| signature-validator | 2 | ~324 |
| account-balances | 3 | ~570 |
| **Toplam** | **37** | **~3722** |

---

## 15. Sonuç

Bu 10 crate CoW Protocol'ün **infrastructure katmanı**'nı oluşturuyor. Blockchain erişimi (ethrpc), veri modeli (eth-domain-types), sayısal hesaplamalar (number), güvenlik (signature-validator, account-balances), serializasyon (serde-ext, bytes-hex), ağ konfigürasyonu (chain, http-client) ve optimizasyon (request-sharing) alanlarını kapsıyor.

Nova Shield DEX için en değerli alınan dersler:
1. **Type-safe newtype'lar** — derleme zamanı doğruluk.
2. **U512 intermediate arithmetic** (`full_scalar_mul`) — kayıpsız AMM hesaplamalar.
3. **Batch RPC + Nagle delay** — N pool state'i tek round-trip'te.
4. **Block-triggered cache eviction** — stale data riski + minimal latency.
5. **`simulateDelegatecall`** — Settlement bağlamında pre-interaction aware doğrulama.
6. **RequestSharing** — pahalı hesaplamaları concurrent caller'lar arasında paylaşma.

Bu infrastructure crate'leri doğrudan kopyalanabilir veya adapte edilebilir — CoW Protocol'ün MIT/GPL lisansı kontrol edilmeli.
