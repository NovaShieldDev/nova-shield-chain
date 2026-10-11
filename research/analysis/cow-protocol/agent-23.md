# CoW Protocol Agent-23: simulator + bad-tokens + balance-overrides Derinlemesine Analizi

**Kapsam:** 3 crate, 19 Rust kaynak dosyası
**Toplam analiz:** ~1200+ satır

---

## 1. GENEL MİMARİ VE CRATE'LER ARASINDAKİ İLİŞKİ

Bu üç crate, CoW Protocol'ün DEX simülasyon altyapısının çekirdeğini oluşturur. Birbirleriyle şu hiyerarşiyle çalışır:

```
simulator
  ├── balance-overrides  (eth_call state injection)
  │     ├── balance/mod.rs   (ERC-20 bakiye slot tespiti)
  │     ├── balance/aave.rs  (Aave v3 aToken özel mantığı)
  │     ├── approval/mod.rs  (ERC-20 allowance slot tespiti)
  │     └── detector.rs      (SLOAD izinden slot çıkarma)
  └── bad-tokens
        ├── trace_call.rs    (token transfer simülasyonu)
        └── list_based.rs    (kara liste)
```

`simulator` crate'i hem Tenderly API'sini hem de doğrudan Ethereum JSON-RPC'yi destekler. `balance-overrides` crate'i, herhangi bir ERC-20 tokenının bakiye ve allowance depolama slotunu EVM çalışma izinden otomatik keşfeder ve `eth_call` üzerinde state-override olarak enjekte eder. `bad-tokens` ise bir tokenın CoW Protocol'ün batch settlement mekanizmasıyla uyumsuz olup olmadığını tespit eder.

---

## 2. CRATE: simulator (11 dosya)

### 2.1 src/lib.rs — Ana Simülatör Yapısı

**Struct:** `Simulator` (satır 19-25)
```rust
pub struct Simulator {
    inner: Inner,
    eth: Ethereum,
    disable_access_lists: bool,
    disable_gas: Option<eth::Gas>,
    simulation_overrides: Option<SimulationOverrides>,
}
```

**Enum:** `Inner` (satır 27-31) — iki backend arasında seçim yapar:
- `Tenderly(Box<tenderly::Tenderly>)` — harici Tenderly API
- `Ethereum` — doğrudan JSON-RPC

**Önemli metodlar:**

`access_list_for_eth_transfer(from, to)` (satır 83-115): Native ETH transferi için EIP-2930 access list üretir. Smart contract cüzdanlarına (Safe, EIP-1271) ETH göndermek için gereklidir. İki backend da desteklenir: Tenderly `GenerateAccessList::Yes` ile çağrılır, Ethereum backend ise `eth_createAccessList` RPC metodunu kullanır. Sonuç, mevcut access list ile birleştirilir: `tx.access_list.clone().merge(access_list)`.

`gas(tx)` (satır 118-147): Transaction gas tahminini yapar. `disable_gas` ayarlanmışsa sabit bir değer döner (test modu). Production'da:
- Tenderly: `simulate()` çağrısıyla `gas_used` alınır, `measure("tenderly_simulate_gas")` ile metrik kaydı yapılır.
- Ethereum: `estimate_gas()` çağrısı yapılır.

**Error hiyerarşisi** (satır 150-202):
- `SimulatorError`: Tenderly hatası, Ethereum hatası, veya gas limitini aşma hatası
- `RevertError`: revert eden bir transaction'ı ve block numarasını birlikte taşır
- `Error`: ya `Revert(RevertError)` ya da `Other(SimulatorError)`

`with()` closure fabrikası (satır 180-202): Hata tipine göre bir transaction'ın `Revert` mi yoksa `Other` kategorisine mi gireceğine karar verir. HTTP hataları ve `Other` hataları transactioni taşımaz; revert hataları ve gas hataları taşır. Bu ayrım, üst katmanın revert nedenini analiz edip etmeyeceğine karar vermesini sağlar.

---

### 2.2 src/simulation_builder.rs — Settlement Simülasyon Fabrikası

Bu dosya tüm simülasyon sistemi içindeki en kapsamlı bileşendir: 749 satır, 15 struct/enum/fn.

**`SettlementSimulator`** (satır 28-131): `Arc<Inner>` içinde settlement sözleşmesini, authentikator adresini, vault relayer'ı, flash loan router'ı, hooks trampoline'i, native token adresini, max gas limitini, state override sağlayıcısını, Ethereum provider'ını, domain separator'ı, chain ID'yi, current block stream'ini, Tenderly handle'ını ve propAMM override stream'ini saklar.

`SettlementSimulator::new()` (satır 49-81): Async fabrika fonksiyonu. Settlement sözleşmesinden `authenticator()`, `vaultRelayer()`, `domainSeparator()` onchain değerlerini okur.

**`SimulationBuilder`** (satır 144-313): Builder pattern ile settlement çağrısı oluşturur. Alanlar:
- `orders: Vec<Order>` — execute edilecek emirler
- `pre_interactions / main_interactions / post_interactions: Vec<InteractionData>` — settlement fazları
- `wrapper: WrapperConfig` — flash loan veya custom wrapper zinciri
- `solver: Option<Solver>` — gerçek veya sahte solver
- `auction_id: Option<i64>` — calldata sonuna eklenen auction ID
- `account_override_requests: Vec<AccountOverrideRequest>` — state override talepleri
- `provide_buy_tokens: bool` — settlement sözleşmesine buy token bakiyesi enjekte et
- `presign_orders: bool` — presign imzaları simüle et

`parameters_from_app_data()` (satır 212-251): App data JSON'ını parse eder. Pre/post hook'ları trampoline üzerinden encode eder. Flash loan ve custom wrapper arasında uyumluluk kontrolü yapar (ikisi aynı anda kullanamaz, satır 220-248).

**Euler hack** `compute_euler_override()` (satır 329-433): Euler Finance entegrasyonu için geçici bir state override hack'i. Euler'in `operatorLookup` mapping'ini (`mapping(bytes19 => mapping(address => uint256))`) storage slot 24'te doğrudan manipüle eder. Euler'in üç wrapper tipi için sabit hardcoded domain separator ve type hash değerleri içerir:
- Open position wrapper: `0x59684A...`
- Close position wrapper: `0xa18c87...`
- Collateral swap wrapper: `0x175fbd...`

Bu, "tavuk ve yumurta" problemi için geçici bir çözümdür: Euler quote almak için imza gerektiriyor, imza için quote gerekiyor. State override ile sistemi bootstrap ediyorlar.

**`EthCallInputs`** (satır 437-567): Build'in çıktısı. İki simülasyon modunu destekler:

`simulate()` (satır 460-469): Doğrudan `eth_call` yapar, state override'ları uygular.

`simulation_report()` (satır 475-499): `debug_traceCall` ile call trace alır ve `report::generate_settlement_report()` ile human-readable rapor üretir.

`simulate_with_tenderly_report()` (satır 505-524): `eth_call` simülasyonu + Tenderly API çağrısı yaparak shareable bir debug link üretir.

`to_tenderly_request()` (satır 528-566): Önemli bir tasarım kararı: Tenderly varsayılan olarak bir transaction'ı block'un başında simüle eder; node ise block'un sonunda. Bunu düzeltmek için `block_number: block + 1`, `transaction_index: 0` kullanılıyor. Bu, Tenderly simülasyonunu node davranışına yaklaştırır.

**`AccountOverrideRequest`** enum (satır 678-706): 7 tip:
1. `SufficientEthBalance(Address)` — `U256::MAX / 2` ETH verir
2. `AuthenticateAsSolver(Address)` — `GPv2AllowListAuthentication` storage slot 1'e keccak hash yazar
3. `Balance { holder, token, amount }` — balance-overrides crate'ine delege eder
4. `Code { account, code }` — EVM bytecode enjekte eder
5. `Custom { account, state }` — ham `AccountOverride`
6. `Approval { owner, token, spender, amount }` — allowance override
7. `PreSignature(OrderUid)` — settlement sözleşmesinin `preSignature` mapping'ine PRE_SIGN_SENTINEL yazar

**`AuthenticateAsSolver` storage manipülasyonu** (satır 583-596):
```
GPv2AllowListAuthentication.solvers[addr] mapping:
slot = keccak256(pad32(addr) ++ pad32(1))
value = bytes32(1)
```
Yani herhangi bir adresin solver listesine "eklenmesi" bu tek storage write ile simüle edilebilir.

**`PreSignature` sentinel değeri** (satır 632-633):
```
PRE_SIGN_SENTINEL = 0xf59c009283ff87aa78203fc4d9c2df025ee851130fb69cc3e068941f6b5e2d6f
```
Bu değer `GPv2Signing.sol#44-46`'da tanımlıdır ve bir orderin pre-sign edilmiş olduğunu gösterir.

**`finish_simulation_builder()`** (satır 344-529): Asıl encoding mantığı. Adımlar:
1. Her order için `executed_amount()` ile fill miktarını hesaplar (Full/Remaining/Explicit)
2. Token-price çiftlerini `[2*i]=sell_token, [2*i+1]=buy_token` şeklinde encode eder
3. `provide_buy_tokens=true` ise settlement sözleşmesine yeterli buy token bakiyesi enjekte eder
4. Presign order'lar için PRE_SIGN_SENTINEL storage override'ları ekler
5. `encode_trade()` ile her order'ı trade tuple'ına dönüştürür
6. `EncodedSettlement` oluşturur, `settleCall` ABI'siyle encode eder
7. Auction ID'yi calldata sonuna ekler (big-endian int64)
8. Wrapper/flash loan routing yapar

**propAMM override mantığı** (satır 502-511): Sadece `Block::Latest` için propAMM state overrides uygulanır. Tarihi bloklarda bu veriler mevcut değil; propAMM fiyatları block builders'ın memory'sindeydi.

**`merge_account_override()`** (satır 684-738): İki `AccountOverride`'ı conflict detection ile birleştirir. State vs state_diff uyumsuzluğu, aynı storage slot'un iki kez yazılması, veya balance/nonce/code çakışması durumunda `MergeConflict` döner.

**Solver Enum** (satır 582-594):
- `OriginUnaltered(Address)` — gerçek bir solver adresi, override yok
- `Fake(Option<Address>)` — random veya belirtilen adres; otomatik olarak ETH verilir ve solver listesine eklenir

**`ExecutionAmount`** (satır 596-608):
- `Full` — tam order miktarı
- `Remaining` — onchain fill durumu sorgulanır, kalan miktar
- `Explicit(U256)` — tam kontrol

**`WrapperConfig`** (satır 663-669):
- `Flashloan(Vec<FlashloanRequest>)` — FlashLoanRouter üzerinden
- `Custom(Vec<WrapperCall>)` — `ICowWrapper::wrappedSettleCall` üzerinden
- `NoWrapper` — doğrudan settlement sözleşmesi

---

### 2.3 src/encoding.rs — ABI Encoding Katmanı

**`EncodedTrade`** tip alias (satır 32-44): 11 alanlı tuple — sellTokenIndex (U256), buyTokenIndex (U256), receiver (Address), sellAmount (U256), buyAmount (U256), validTo (u32), appData (B256), feeAmount (U256), flags (U256), executedAmount (U256), signature (Bytes).

**`order_flags()`** (satır 176-204): 8-bit flag byte encode eder:
- Bit 0: Order kind (0=sell, 1=buy)
- Bit 1: partially_fillable
- Bit 2-3: sell_token_balance (00=ERC20, 10=External, 11=Internal)
- Bit 4: buy_token_destination (0=ERC20, 1=Internal)
- Bit 5-6: signing scheme (00=EIP712, 01=EthSign, 10=EIP1271, 11=PreSign)

**`encode_wrapper_settlement()`** (satır 281-298): Wrapper zinciri için çağrı oluşturur. İlk wrapper'ın adresi transaction hedefidir, geri kalanlar `wrapperData` içinde encode edilir.

**`encode_wrapper_data()`** (satır 327-342): İkili encoding formatı:
```
[A calldata length (u16)] [A calldata] [B address (20 bytes)] [B calldata length (u16)] [B calldata] ...
```
İlk wrapper'ın adresi atlanır (transaction hedefi olarak zaten kullanılır). Uzunluk big-endian u16 olarak encode edilir.

**`EncodedSettlement`** (satır 68-112): `into_settle_call()` metodu `GPv2Settlement::settleCall` ABI struct'ını oluşturur ve ABI-encode eder. GPv2Interaction ve GPv2Trade struct'larını alloy-sol-types ile oluşturur.

**DEX Uygulama Notu:**
> Settlement encoding, "clearing price" mantığını doğrudan içerir. Her order çifti için `sell_price = buy_amount`, `buy_price = sell_amount` (LimitPrice modu) ya da custom fiyatlar kullanılır. Bu, CoW Protocol'ün batch settlement price uniform clearing mekanizmasıdır: her trader, kendi limit fiyatından daha iyi veya eşit bir fiyat alır. Nova Shield DEX bu encoding'i kendi clearing algoritmasının çıktısını on-chain'e göndermek için adapte edebilir.

---

### 2.4 src/tenderly/mod.rs — Tenderly API Entegrasyonu

**`Tenderly`** struct (satır 25-30): `TenderlyApi` + `Ethereum` + `save` + `save_if_fails` bayrakları.

**`TenderlyApi`** (satır 33-40): Base URL, reqwest client, dashboard URL, chain ID. `x-access-key` header ile API key set edilir, `set_sensitive(true)` ile loglardan gizlenir.

**`Api` trait** (satır 42-55):
- `log_simulation_command()` — curl komutu log'a yazar (manuel debug için)
- `simulate(request)` — async simülasyon
- `simulate_and_share(request)` — simüle et + paylaş + link döndür

**`Tenderly::simulate()`** (satır 67-100): Önemli bir pre-processing adımı: State override'lardan `nonce` değerlerini çıkarır (`account_override.nonce = None`). Tenderly bu değeri kabul etmiyor; propAMM frame'leri her zaman nonce taşıdığı için bu adım zorunludur.

**`prepare_request()`** (satır 216-254): `TransactionRequest` → `dto::Request` dönüşümü. Block numarası `block + 1` olarak ayarlanır, `transaction_index = 0`.

**`Instrumented`** wrapper (satır 302-338): `simulate()` çağrılarını Prometheus `IntGaugeVec` ile sayar (label: name, result=ok/err).

**Hata tipleri** (satır 353-359):
- `Http(reqwest::Error)` — ağ hatası
- `Revert(SimulationId)` — EVM revert (bir simülasyon ID'si var)
- `Other(anyhow::Error)` — diğer

`shared_simulation_url()` (satır 212-214): Simülasyon ID'sinden paylaşılabilir URL üretir: `https://dashboard.tenderly.co/shared/simulation/{id}`.

---

### 2.5 src/tenderly/dto.rs — Tenderly Veri Tipleri

**`Request`** (satır 12-56): Tenderly API istek objesi. Tüm opsiyonel alanlar `skip_serializing_if = "Option::is_none"` ile atlanır.

**`StateObject`** (satır 137-151): Tenderly'nin `state_objects` formatı. Üç alan: `balance`, `code`, `storage` (slot-to-value map). Önemli: `nonce` ve tam `state` (full replacement) desteklenmiyor, sadece `state_diff` (partial override) destekleniyor.

`TryFrom<AccountOverride> for StateObject` (satır 153-170): Alloy'un `AccountOverride`'ından Tenderly `StateObject`'ine dönüşüm. Nonce veya full state replacement varsa hata döner.

**`SimulationType`** (satır 173-184):
- `Full` — tam trace, state diff, decoded output (en yavaş)
- `Quick` — sadece raw output (en hızlı)
- `Abi` — decoded input/output ama state diff yok

**`Transaction`** (satır 186-191): `status` (bool), `gas_used` (u64), `call_trace` (Vec<CallTrace>).

**`CallTrace`** (satır 193-200): `output` (hex-encoded bytes), `error` (string).

**`AccessList`** (satır 78-108): Tenderly `snake_case` kullanırken Ethereum standardı `camelCase` kullanır. Bu nedenle özel bir newtype wrapper gerekiyor. Dört direction'da `From` impl var.

---

### 2.6 src/ethereum/mod.rs — Ethereum JSON-RPC Backend

**`Ethereum`** (satır 18-22): `Web3` + `Arc<Inner>`. Inner: chain, contracts, gas estimator, current block stream, tx gas limit.

**`simulation_gas_price()`** (satır 53-64): Base fee'den ve gas estimator'dan gerçek current gas fiyatı hesaplar. Yorum çok önemli: Bazı node'lar gas price 0 olduğunda özel code path'ler tetikleyerek davranış değiştirir — bu bazı token'ların simülasyondan farklı davranmasına neden olur. Bunu önlemek için gerçek gas price her zaman set edilir.

**`cap_gas_limit()`** (satır 92-98): Gas limitini `tx_gas_limit` değeriyle kaplar. Kötü niyetli bir solver'ın block gas limitine kadar binary search yaptırmasını engeller.

**`create_access_list()`** (satır 100-121): Gas price set eder, gas limit kaplar, `eth_createAccessList` çağırır. `ensure_ok()` ile access list computation hatalarını yakalar.

**`estimate_gas()`** (satır 123-148): Gas price set eder, gas limit kaplar, `eth_estimateGas` çağırır. State override desteği var (`overrides_opt`).

**`Error`** enum (satır 162-174): ContractRpc, Rpc, GasPrice, AccessList, Other.

---

### 2.7 src/ethereum/contracts.rs — Sözleşme Bağlamaları

**`Contracts`** (satır 9-12): `GPv2Settlement::Instance` + `WETH9::Instance`.

`Contracts::new()` (satır 14-33): Chain ID'ye göre deployment adresini otomatik lookup eder. Manuel override mümkün.

---

### 2.8 src/report.rs — Settlement Simülasyon Raporu

Bu dosya, bir CoW Protocol settlement simülasyonunun call trace'ini human-readable event listesine çevirir.

**`SimulationReport`** (satır 16-20):
- `events: Vec<Event>` — settlement akışının kronolojik olayları
- `returned_bytes: Option<Bytes>` — settlement call'unun dönüş değeri
- `revert: Option<String>` — en dış revert mesajı

**`Event`** enum (satır 41-85): 9 varyant:
- `WrapperEntered { wrapper }` — flash loan router veya custom wrapper çağrıldı
- `SettlementEntered` — GPv2Settlement.settle() çağrıldı
- `Hook { target, caught_error }` — trampoline üzerinden user hook çalıştı
- `Eip1271SignatureCheck { revert, gas, owner }` — EIP-1271 imza doğrulama maliyeti
- `AllSignatureChecksSuccessful` — tüm imzalar geçti
- `Transfer { token, from, to, amount, revert }` — ERC-20 token transferi
- `SettlementExited { revert }` — settle() bitti
- `WrapperExited { wrapper, revert }` — wrapper kapandı
- `EndOfSimulation { reason }` — simülasyon sona erdi

**`eip1271_gas_cost()`** (satır 23-37): Belirli bir owner adresinin EIP-1271 imza doğrulamasının kaç gas harcadığını döndürür. Bu değer solver scoring'de kullanılır.

**`generate_settlement_report()`** (satır 93-104): `CallFrame` trace'ini olaylara çevirir. `process_frame()` wrapper zincirini DFS traversal ile işler (stack tabanlı).

**`process_frame()`** (satır 109-152): İteratif stack traversal. `ICowWrapper::wrappedSettleCall` veya `FlashLoanRouter::flashLoanAndSettleCall` abi decode başarısıyla wrapper tespiti yapılır. `GPv2Settlement::settleCall` decode başarısıyla settlement tespiti yapılır.

**`process_settle_frame()`** (satır 154-201): settlement() içindeki çağrılar:
- Trampoline adresi → `Hook` event
- Vault relayer → `AllSignatureChecksSuccessful` + `Transfer` eventleri
- ERC-20 transfer → `Transfer` event
- ERC-1271 `isValidSignature` → `Eip1271SignatureCheck` event

**Testler:** İki `#[ignore]` integration testi: `xdai_multiple_wrappers` ve `mainnet_detects_caught_hook_reverts`. Gerçek onchain transaction'ları replay ederek rapor içeriğini byte-level doğrular.

---

### 2.9 src/state_override_stream.rs — PropAMM Live State Overrides

Bu modül, Titan block builder'ının WebSocket üzerinden yayınladığı propAMM (programmatic AMM) state override'larını gerçek zamanlı alır ve simülasyonlara uygular.

**`SimulationOverrides`** (satır 34-72): `Arc<Inner>` wrapper. `current()` metodu:
1. Snapshot `received_at` kontrolü — `max_age` geçtiyse `None`
2. Snapshot block number vs current block number — eşleşmiyorsa `None`
3. Overrides boşsa `None`
4. Tümü geçerliyse `Some(overrides)`

Metrik kaydı: `OverrideResult::Fresh` veya `OverrideResult::Stale`.

**`Frame`** (satır 74-83): WebSocket mesaj formatı. `block_number` + venue map. Venue key'leri Ethereum adresidir; adres parse edilemeyen key'ler sessizce atlanır.

**`VenueUpdate`** (satır 85-89): `state_override: StateOverride` içerir.

**`deserialize_venue_overrides()`** (satır 91-123): Custom deserializer. Map key'leri `Address::parse()` ile parse edilir; başarısız olanlar `IgnoredAny` olarak atlanır. Bu sayede metadata alanları (örneğin `"slot"`, `"blockNumber"`, `"future-metadata"`) ignore edilir.

**`run_stream()`** (satır 146-199): WebSocket reconnect döngüsü. Exponential backoff: 250ms'den başlar, her denemede 2 katına çıkar, max 15 saniye. Bağlantı kesilince override'lar tutulur (yeniden bağlanmada sıfırlanmaz).

**`apply_frame()`** (satır 203-209): Her venue'nun full override'ı mevcut map'e insert edilir. "Latest frame wins" stratejisi: aynı hesap için yeni gelen override eskisini tamamen replace eder.

**Prometheus metrikleri** (satır 223-239):
- `frames_received` (IntCounter)
- `parse_failures` (IntCounter)
- `reconnects` (IntCounter)
- `merge_conflicts` (IntCounter — artık kullanılmıyor ama backward compat için korunuyor)
- `venue_count` (IntGauge)
- `simulations_with_overrides[result]` (IntCounterVec)

**Gerçek Titan frame örneği** (satır 515-516): Block 25475333'ten alınmış iki gerçek frame const olarak tanımlanmış, parse testlerinde kullanılıyor.

---

### 2.10 src/utils/mod.rs — URL Yardımcısı

**`join_url()`** (satır 5-14): Base URL ve path'i birleştirir, fazladan slash karakterlerini normalize eder.

---

### 2.11 tests/aave_replay.rs — Aave Integration Testi

İki gerçek production order replay testi: Aave v3 debt-swap ve collateral-swap. Her ikisi de `#[ignore]` ile işaretlenmiş ve `MAINNET_RPC_URL` env var gerektirir.

**`aave_debt_swap_replay()`** (satır 80-93): Mainnet block 24,992,051'de pin'lenmiş. Başarılı bir Aave v3 debt-swap simülasyonu.

**`aave_debt_swap_replay_fails_when_flashloan_oversubscribed()`** (satır 105-129): Aynı order, flashloan amount `U256::MAX` yapılmış. Aave pool'un yetersiz likidite nedeniyle revert edeceği doğrulanıyor.

**`aave_collateral_swap_replay_fails_naturally()`** (satır 222-240): Production'da düşen bir order'ın replay testi. Trader'ın aToken bakiyesi yetersiz, pre-hook revert ediyor. Bu testin amacı simülasyonun bu revert'i doğru tespit etmesini doğrulamak.

**DEX Uygulama Notu:**
> Bu test dosyası, flash loan tabanlı DEX işlemleri için bir simülasyon test deseni oluşturur. Nova Shield DEX, kendi flash loan entegrasyonunu test etmek için bu pattern'i kullanabilir: fork_block sabit pin'lenmeli, hem başarılı hem de başarısız senaryolar test edilmeli, state override'lar doğru uygulanmalı.

---

## 3. CRATE: bad-tokens (3 dosya)

### 3.1 src/lib.rs — TokenQuality Tipi

**`TokenQuality`** enum (satır 6-9):
```rust
pub enum TokenQuality {
    Good,
    Bad { reason: String },
}
```

`is_good()` ve `bad(reason)` yardımcı metodları. Basit ama kritik: Bu tip, bir tokenın CoW Protocol ile uyumlu olup olmadığını temsil eder.

---

### 3.2 src/list_based.rs — Kara Liste

**`DenyListedTokens`** (satır 8-12): `Arc<Inner>` içinde `HashSet<Address>`.

`DenyListedTokens::new(deny_list: Vec<Address>)` — HashSet'e dönüştürür.
`contains(token: &Address) -> bool` — kara listede mi kontrol eder.

Bu bileşen basit ama önemlidir: Bilinen sorunlu token'ların (örneğin reentrancy bug'ı olan, fee-on-transfer uygulayan) hızla reddedilmesini sağlar. Static yapıdadır; güncelleme için servis yeniden başlatılmalıdır.

---

### 3.3 src/trace_call.rs — Token Kalite Dedektörü (303 satır)

Bu dosya, bir token'ın "kötü" olup olmadığını runtime'da tespit eden asıl algoritmayı içerir.

**`TraceCallDetectorRaw`** (satır 21-24):
```rust
pub struct TraceCallDetectorRaw {
    pub web3: Web3,
    pub settlement_contract: Address,
}
```

**`test_transfer()`** (satır 34-83): Ana metod. Şu sırayla `trace_callMany` çağrısı yapar:
1. Pre-interaction'ları çalıştır
2. Token detection trace'lerini çalıştır (8 işlem)

Node `trace_callMany` desteklemiyorsa (method not found, HTTP 400) token `Good` olarak kabul edilir. Bu "fail open" stratejisi, simülasyon altyapısı olmayan node'larda sistemi çalışmaya devam ettirir.

**`create_trace_request()`** (satır 92-181): 8 sequential işlem oluşturur:
- İşlem 0: `balanceOf(settlement)` — başlangıç bakiyesi
- İşlem 1: `transfer(settlement, amount)` — AMM → settlement (from = liquidity source)
- İşlem 2: `balanceOf(settlement)` — transfer sonrası bakiye
- İşlem 3: `balanceOf(arbitrary_recipient)` — hedef başlangıç bakiyesi
- İşlem 4: `transfer(arbitrary_recipient, amount)` — settlement → random adres
- İşlem 5: `balanceOf(settlement)` — çıkış sonrası bakiye
- İşlem 6: `balanceOf(arbitrary_recipient)` — hedef son bakiye
- İşlem 7: `approve(arbitrary_recipient, U256::MAX)` — max approval testi

**`arbitrary_recipient()`** (satır 88-90): `Address::random()` — her çağrıda farklı adres üretir. Bunun nedeni: Bazı fee-on-transfer token'ları kendi AMM pool'larına veya bilinen adreslere transfer fee uygulamaz. Random adres kullanarak bu whitelist'leri bypass etmek amaçlanır.

**`handle_response()`** (satır 183-298): Trace sonuçlarını analiz eder:

1. **Transfer in başarısız mı?** İşlem 1 revert ettiyse `Bad`.

2. **Transfer out başarısız mı?** İşlem 4 revert ettiyse `Bad`.

3. **balanceOf 32 byte döndürdü mü?** `u256_from_be_bytes_strict()` ile kontrol. Vyper compiler bug'ından dolayı bazı tokenlar yanlış boyutlu return değeri verir.

4. **Fee-on-transfer (giriş)?** Beklenen: `balance_after_in >= balance_before_in + amount - 1`. Bir wei tolerans var (eUSD gibi "shares" mekanizmalı tokenlar için).

5. **Çıkış sonrası bakiye eski değere döndü mü?** `balance_after_out == balance_before_in` olmalı.

6. **Alıcı doğru miktarı aldı mı?** `balance_recipient_after >= balance_recipient_before + amount - 1`.

7. **Max approval başarılı mı?** `approve(U256::MAX)` bazı token'larda revert eder.

**`u256_from_be_bytes_strict()`** (satır 317-322): Tam olarak 32 byte değilse `None` döndürür. Vyper bug detection için kritik.

**`trace_many()`** (satır 328-338): `trace_callMany` RPC çağrısı yapar. Tüm işlemleri `TraceType::Trace` ile birlikte gönderir.

**Test:** `handle_response_ok()` (satır 354-459) — 8 trace sonucunu manuel olarak oluşturur, `Good` döndüğünü doğrular.

**DEX Uygulama Notu:**
> CoW Protocol'ün token kalite testi, her DEX için kritik bir güvenlik katmanıdır. Fee-on-transfer token'lar (SAFEMOON, REFLECT tokenlar) DEX arbitraj ve settlement hesaplamalarını bozar. Nova Shield DEX'in bu testi kendi token whitelist mekanizmasına entegre etmesi zorunludur. Özellikle:
> - Yeni bir token listing'de bu 8-işlem süreci çalıştırılmalı
> - Bir wei tolerans (rounding) eklenmelidir
> - arbitrary_recipient'in gerçekten random olması whitelist bypass için önemlidir
> - Node trace desteği yoksa fail-open stratejisi kullanılmalı

**Nasıl daha iyi yapılır:**
> - Sadece `transfer` değil, `transferFrom` de test edilebilir (CoW Protocol vault relayer `transferFrom` kullanır)
> - Rebase tokenlar için (AMPL, stETH) ek test: Zamanla bakiye değişiyor mu?
> - Proxy/upgradeable tokenlar için storage layout değişikliğine karşı test
> - DenyList güncellenmesi için runtime governance mekanizması eklenebilir

---

## 4. CRATE: balance-overrides (5 dosya)

Bu crate, CoW Protocol'ün simülasyon altyapısının en sofistike bileşenidir. Herhangi bir ERC-20 tokenının balance ve allowance storage slot'unu dinamik olarak keşfeder ve `eth_call` state override'ı olarak enjekte eder.

### 4.1 src/lib.rs — Genel API

**`StateOverriding` trait** (satır 16-26): Async trait:
```rust
#[async_trait]
pub trait StateOverriding: Send + Sync + 'static {
    async fn balance_override(&self, request: BalanceOverrideRequest)
        -> Option<(Address, AccountOverride)>;
    async fn approval_override(&self, request: ApprovalOverrideRequest)
        -> Option<(Address, AccountOverride)>;
}
```

**`StateOverrides`** (satır 31-97): Default impl. `StateOverrides::new(web3)` — default parametreler: `probing_depth=60`, `verification_timeout=DEFAULT_VERIFICATION_TIMEOUT (1s)`, `cache_size=1000`.

`balance_override()` (satır 68-82): `balance_detector.detect(token, holder)` ile strateji bulur, `strategy.state_override(&holder, &amount)` ile override üretir, son elemanı alır (AddressMap → son override).

`approval_override()` (satır 84-96): Aynı pattern, `approval_detector` ile.

**`DummyStateOverrider`** (satır 99-117): Test mock'u, her zaman `None` döner.

**Test suite** (satır 119-391): 6 test:

1. `balance_override_computation` — WETH (0xDEf1CA...) için Solidity mapping slot hesaplar ve doğrular. Beklenen slot: `fca351f4...`. Bu, `keccak256(pad32(vitalik_addr) ++ pad32(0))` hesabıdır.

2. `balance_overrides_none_for_unknown_tokens` — DummyStateOverrider None döndüğünü test eder.

3. `balance_override_computation_solady` — Solady token için özel magic seed hesabı.

4. `cached_detection_caches_holder_agnostic_strategies_without_holder` — Universal stratejiler `(token, None)` key'iyle cache'lenir.

5. `cached_detection_caches_holder_specific_strategies_with_holder` — DirectSlot stratejiler `(token, Some(holder))` key'iyle cache'lenir.

6. `caches_universal_approval_strategy_without_inputs` ve `caches_input_specific_approval_strategy_with_inputs` — Approval cache'i için aynı pattern.

---

### 4.2 src/detector.rs — EVM Trace Slot Extractor

**`DEFAULT_VERIFICATION_TIMEOUT`** (satır 10): `Duration::from_secs(1)`.

**`DetectionError<E>`** (satır 21-28): 3 varyant: `NotFound`, `Rpc`, `Simulation`.

**`extract_sload_slots()`** (satır 34-87): `debug_traceCall`'un `GethTrace` çıktısından SLOAD operasyonlarını çıkarır.

Algoritma:
- Stack tabanlı storage context tracking
- `CALL/STATICCALL`: yeni context (çağrılan adres) push edilir
- `DELEGATECALL`: aynı context korunur (delegatecall storage context ayrı sözleşmenin storage'ını değiştirmez)
- `RETURN`: context pop edilir
- `SLOAD`: mevcut context'teki slot `(address, slot)` pair olarak eklenir. `HashSet` ile duplicate tespiti yapılır, her unique slot sadece bir kez eklenir.

Dönüş: `Vec<(Address, B256)>` — ilk erişim sırası korunur.

**`mapping_slot_hash()`** (satır 91-96): Solidity mapping slot hesabı:
```
slot = keccak256(pad32(holder) ++ map_slot_bytes)
```
64-byte buffer: holder adresi byte 12-32'ye kopyalanır (12 byte sıfır padding + 20 byte adres = pad32), map slot byte 32-64'e kopyalanır.

---

### 4.3 src/balance/mod.rs — Balance Override Dedektörü (921 satır)

En karmaşık ve en önemli dosya.

**Sabitler:**

`BALANCE_SLOT_SEED` (satır 24): `[0x87, 0xa2, 0x11, 0xa2]` — Solady ERC-20'nin balance slot magic bytes. [Kaynak](https://github.com/Vectorized/solady/blob/main/src/tokens/ERC20.sol#L81)

`SENTINEL` (satır 35-36): 32-byte, tüm byte'ları farklı ve non-zero sentinel değer:
```
0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20
```
Bu sentinel, storage slot içindeki balance'ın nereye "paketlendiğini" (shift) tespit etmek için kullanılır.

`NATIVE_ETH` (satır 39): `0xeeeeee...` — ERC-20 olmayan native ETH için marker.

**`Strategy`** enum (satır 116-138): 5 varyant:
1. `SolidityMapping { target_contract, map_slot, shift_bits }` — Standart `mapping(address => uint256)`
2. `SoladyMapping { target_contract, shift_bits }` — Solady magic hash
3. `DirectSlot { target_contract, slot, shift_bits }` — Bilinmeyen layout, gözlemlenen slot
4. `AaveV3AToken { target_contract, pool, underlying, web3 }` — Aave v3 özel mantığı
5. `NativeEth` — Native ETH için ETH balance override

**`Strategy::state_override()`** (satır 141-216):

*SolidityMapping:*
```
slot = keccak256(pad32(holder) ++ pad32(map_slot))
value = amount << shift_bits
```

*SoladyMapping:*
```
buf[0..20] = holder.bytes
buf[28..32] = [0x87, 0xa2, 0x11, 0xa2]
slot = keccak256(buf[0..32])
value = amount << shift_bits
```

*DirectSlot:* Gözlemlenen slot'a doğrudan yazar.

*AaveV3AToken:* `aave::build_override()` çağrısına delege eder.

*NativeEth:* `AccountOverride::default().with_balance(amount)` ile ETH bakiyesini override eder.

**Shift detection** için `with_shift()`: SolidityMapping, SoladyMapping ve DirectSlot'ta `shift_bits` mutate edilebilir; Aave ve NativeEth'de anlamsızdır.

**`PartialEq for Strategy`** (satır 241-296): Web3 handle'ı hariç tüm alanları karşılaştırır. Aynı token/pool/underlying kombinasyonu semantik olarak eşdeğerdir.

**`Detector`** (satır 305-368): Cache + dedeksiyon motoru.

Cache tipi: `moka::sync::Cache<(Address, Option<Address>), Option<Strategy>>`. Key:
- `(token, None)` — universal stratejiler (tüm holder'lar için geçerli)
- `(token, Some(holder))` — holder-specific stratejiler (DirectSlot)

Cache hit önceliği: Önce `(token, None)`, sonra `(token, Some(holder))` kontrol edilir.

**`Detector::uncached_detect()`** (satır 371-493): Asıl detection mantığı:

1. **Native ETH hızlı yolu**: Token `0xeeee...` ise hemen `Strategy::NativeEth` döner.

2. **Aave hızlı yolu** (satır 386-401): Token'ın Aave v3 aToken olup olmadığını `aave::probe_aave_token()` ile kontrol eder. Başarılıysa `USER_STATE_SLOT=52` üzerinden doğrular.

3. **Generic yol** (satır 403-492): `debug_traceCall` ile `balanceOf(holder)` trace eder. `extract_sload_slots()` ile SLOAD'lenen slotları alır. `find_plausible_strategies_for_slots()` ile aday stratejiler üretir. Her strateji için `verify_strategy()` çağırır, `verification_timeout` ile timeout uygular.

**`find_plausible_strategies_for_slots()`** (satır 47-93): Heuristic matching. Tersine sırada işlem yapar (son erişilen slot'lar genellikle balance slot'udur). İki kategoriye ayırır:
- Heuristic: keccak hesabı ile eşleşen SolidityMapping veya SoladyMapping
- Fallback: DirectSlot

**`Detector::verify_strategy()`** (satır 504-548): Sentinel değeri slot'a yazar ve `balanceOf` sonucunu okur. Sentinel'in `balanceOf` çıktısında görünmesi, o slot'un balance slot'u olduğunu kanıtlar.

**`detect_shift()`** (satır 565-574): Observed balance değerinde sentinel'in byte-string olarak nerede göründüğünü bulur. Position sayı of bytes below the pattern, `* 8` = bit shift.

Algoritma:
```rust
let bytes = observed.to_be_bytes::<32>();
let needle = &bytes[bytes.iter().position(|&b| b != 0)?..];
let start = SENTINEL.windows(needle.len()).position(|w| w == needle)?;
Some(SENTINEL.len().checked_sub(start + needle.len())? * 8)
```

Bu, AUSD gibi tokenların `{ bool isFrozen; uint248 balance }` gibi packed struct'larını da destekler.

**Testler** (satır 576-920): 10 test, 4'ü `#[ignore]` (live node gerektirir):
- `detect_shift_locates_packed_balance` — shift detection doğruluğu
- `test_create_strategies_reverses_order` — son erişilen slot'ların önce gelmesi
- `test_create_strategies_with_heuristic_slots_stable` — heuristic önce, DirectSlot sonra
- `detects_storage_slots_mainnet` — WETH, upgradeable ERC20, Solady token live test
- `detects_storage_slots_arbitrum` — Arbitrum USDC.e live test
- `detects_packed_balance_slot_avalanche` — AUSD shift_bits=8
- `detects_native_eth` — native ETH override, WETH deposit test

**DEX Uygulama Notu:**
> Bu balance detection sistemi, CoW Protocol'ün herhangi bir ERC-20 token ile çalışabilmesini sağlayan temel mekanizmadır. Nova Shield DEX için kritik önem taşır çünkü:
> - Solver'ların token bakiyelerini simülasyon ortamında manipüle etmesini sağlar
> - Aave aToken gibi "rebase" veya "yield-bearing" token'ların doğru simülasyonunu sağlar
> - Packed storage (shift_bits) desteği, non-standard ERC-20 implementasyonlarını kapsar
>
> Nova Shield DEX'in bu sistemi benimsemesi için native Rust blockchain ortamına uyarlanması gerekir. EVM state override yerine Nova Shield'ın native storage API'siyle entegre edilmelidir.

**Nasıl daha iyi yapılır:**
> - Verification timeout'u (1s) configurable yapılabilir, ağ koşullarına göre dinamik ayarlanabilir
> - Solady dışında başka optimized ERC-20 kütüphaneleri (örneğin OpenZeppelin v5) için özel heuristic eklenebilir
> - Cache invalidation: Token upgrade'lerinde (proxy) strateji eskiyebilir; TTL tabanlı cache expiry veya bloom filter ile staleness detection eklenebilir
> - Parallel verification: Mevcut kodda stratejiler sequential denenir; birden fazla candidate'i paralel verify etmek hızlandırabilir

---

### 4.4 src/balance/aave.rs — Aave V3 AToken Override (378 satır)

Bu dosya, Aave v3'ün standart ERC-20'den nasıl ayrıldığını ele alır.

**Neden özel işlem?** (satır 4-8):
- `balanceOf` = `scaled_balance × getReserveNormalizedIncome / RAY` — ham storage değeri değil
- Storage = `UserState { uint128 balance; uint128 additionalData }` — packed struct

**ABI tanımları** (satır 19-71): `sol!` makrosu ile üç interface:
- `IAaveV3Pool`: `getReserveNormalizedIncome(asset)`, `getReserveData(asset)`
- `IAaveV3AToken`: `UNDERLYING_ASSET_ADDRESS()`, `POOL()`
- `ReserveData` struct: 15 alan, `aTokenAddress` bizim için önemli alan

**`RAY`** (satır 77): `1e27` — Aave'nin 27 decimal fixed-point birimi.
```
RAY = 1_000_000_000_000_000_000_000_000_000
    = 0x33b2e3c9fd0803ce8000000 (hex)
```

**`USER_STATE_SLOT`** (satır 84): `52` — Tüm standart Aave v3 aToken'ların `_userState` slot'u.

**`SENTINEL`** (satır 91-92): Aave-özel küçük sentinel (büyük değerler ray multiply'ı overflow yapar).

**`ray_div(a, b)`** (satır 99-107): Aave'nin `WadRayMath.rayDiv` formülü:
```
result = (a * RAY + b/2) / b
```
Half-up rounding (round-half-up). `None` döner: `b == 0` veya overflow.

**`pack_user_state(balance, additional_data)`** (satır 112-116):
```
mask = (1 << 128) - 1
packed = ((additional_data & mask) << 128) | (balance & mask)
```
Low 128 bits = balance, high 128 bits = additional_data.

**`probe_aave_token()`** (satır 122-133): Token'ın gerçek Aave v3 aToken olup olmadığını 3 onchain çağrıyla doğrular:
1. `UNDERLYING_ASSET_ADDRESS()` — underlying'i öğren
2. `POOL()` — pool adresini öğren
3. `getReserveData(underlying)` — pool'un `aTokenAddress` alanının bu token'a eşit olması gerekiyor

Bu 3. adım kritik güvenlik önlemidir: Sadece `UNDERLYING_ASSET_ADDRESS()` ve `POOL()` selector'larını implement eden rogue bir contract'ın Aave stratejisini trigger etmesini engeller.

**`build_override()`** (satır 139-190): Aave v3 aToken için state override üretir:
1. `getReserveNormalizedIncome(underlying)` — mevcut liquidity index
2. `scaled = ray_div(amount, index)` — desired balance'ı scaled balance'a çevir
3. `slot = mapping_slot_hash(holder, pad32(52))` — `_userState[holder]` slot'u
4. `value = pack_user_state(scaled, 0)` — sıfır additionalData ile pack
5. State diff olarak `(a_token, slot → value)` döner

**Testler** (satır 192-377): 8 test:
- `ray_div_edge_cases` — sıfır bölme, sıfır dividend
- `pack_user_state_leaves_additional_data_intact` — packing doğruluğu
- `pack_user_state_truncates_to_uint128` — 128-bit overflow truncation
- `mapping_slot_hash_matches_solidity_layout` — slot hesabı Solidity EVM'i ile eşleşiyor
- `probe_aave_token_rejects_when_underlying_call_reverts` — güvenlik testi 1
- `probe_aave_token_rejects_when_pool_is_not_aave` — güvenlik testi 2
- `probe_aave_token_rejects_when_pool_registers_a_different_atoken` — güvenlik testi 3 (rogue contract)
- `aave_v3_a_token_override_scales_amount_and_writes_low_128` — tam integration testi

**DEX Uygulama Notu:**
> Aave entegrasyonu, yield-bearing token'larla DEX işlemi yaparken kritik önem taşır. Nova Shield DEX eğer Aave üzerindeki pozisyonları (aToken) ile işlem yapmak isterse bu override mekanizmasına ihtiyaç duyar. Özellikle:
> - Bir kullanıcının USDC deposu aUSDC olarak tutuluyorsa ve bu aUSDC'yi satmak istiyorsa
> - aToken'ın `balanceOf` değeri time-value göre sürekli artar; static bakiye yerine dinamik bakiye hesabı gerekir
> - `ray_div(amount, liquidityIndex)` formülü değişmez bir şekilde implement edilmelidir — Aave'nin kendi kütüphanesiyle aynı sonucu vermeli
>
> Nasıl daha iyi yapılır:
> - `additionalData` alanı (upper 128 bits) gerçek değerle preserve edilmeli; mevcut kod sıfır yazar ve bu teorik olarak yanlış. Gerçek `additionalData`'yı okuyup preserve etmek daha doğru olur.
> - Aave v3 fork'ları için (Radiant, Seamless Protocol) `USER_STATE_SLOT != 52` olabilir; dynamic probe yerine slot detection ile çalışmalı

---

### 4.5 src/approval/mod.rs — Allowance Override Dedektörü (662 satır)

Balance detector ile aynı pattern ama allowance için. İki nested mapping'i (owner → spender → uint256) destekler.

**`ALLOWANCE_SLOT_SEED`** (satır 22): `[0x7f, 0x5e, 0x9f, 0x20]` — Solady ERC-20 allowance magic bytes.

**`ApprovalStrategy`** enum (satır 100-126): 4 varyant:
1. `SolidityMappingOwnerToSpender { target_contract, map_slot }` — `mapping(owner => mapping(spender => uint256))`
2. `SolidityMappingSpenderToOwner { target_contract, map_slot }` — `mapping(spender => mapping(owner => uint256))`
3. `SoladyMapping { target_contract }` — Solady packed allowance
4. `DirectSlot { target_contract, slot }` — fallback

**Solady allowance slot formülü** (satır 149-155):
```
buf[0..20] = owner.bytes
buf[28..32] = [0x7f, 0x5e, 0x9f, 0x20]
buf[32..52] = spender.bytes
slot = keccak256(buf[0..52])  // 52 bytes!
```

**SolidityMappingOwnerToSpender slot** (satır 133-139):
```
inner = keccak256(pad32(owner) ++ pad32(map_slot))
slot  = keccak256(pad32(spender) ++ inner)
```

**SolidityMappingSpenderToOwner slot** (satır 141-146):
```
inner = keccak256(pad32(spender) ++ pad32(map_slot))
slot  = keccak256(pad32(owner) ++ inner)
```

Her iki sıra da denenir çünkü bazı token implementasyonları değişik sıralama kullanır.

**`is_valid_for_all_pairs()`** (satır 164-171): SolidityMapping ve SoladyMapping her `(owner, spender)` çifti için geçerlidir; DirectSlot yalnızca gözlemlenen spesifik çift için geçerlidir.

**Cache stratejisi:**
- Universal → `(token, None)`
- DirectSlot → `(token, Some((owner, spender)))`

**`verify_approval_strategy()`** (satır 352-390): Test amount `0x1337_1337_1337_1337` yazılır, `allowance(owner, spender)` okunur. Eşleşirse strateji doğrudur.

**Testler** (satır 403-661): 10 test, 3'ü `#[ignore]`:
- `solidity_mapping_owner_to_spender_slot` — slot hesabı doğruluğu
- `solidity_mapping_spender_to_owner_slot` — ters sıra slot hesabı
- `solady_approval_slot` — Solady magic hash
- `override_allowance_simple` — WETH allowance live test
- `override_allowance_solady` — Solady token live test
- `test_unmatched_slots_become_direct_slot` — fallback test
- `test_heuristic_before_direct_slot_fallback` — heuristic önceliği
- `test_zero_heuristic_depth_all_direct_slot` — probing_depth=0 tüm DirectSlot

---

## 5. KRİTİK ALGORİTMALAR VE MATEMATİKSEL FORMÜLLER

### 5.1 Solidity Storage Mapping Hash

```
slot = keccak256(lpad(key, 32) ++ lpad(mapping_slot, 32))
```
64 byte buffer, key sağa hizalı (sol 12 byte sıfır için address), mapping slot sağa hizalı.

### 5.2 Solady Balance Slot Hash

```
slot = keccak256(
  address_20bytes ++
  zeros_8bytes ++
  seed_4bytes [0x87, 0xa2, 0x11, 0xa2]
)
```
32 byte buffer. Adres baştadır (sol-hizalı), sıfırlar doldurma, seed 28-32 arası.

### 5.3 Solady Allowance Slot Hash

```
slot = keccak256(
  owner_20bytes ++
  zeros_8bytes ++
  seed_4bytes [0x7f, 0x5e, 0x9f, 0x20] ++
  spender_20bytes
)
```
52 byte buffer. Allowance için owner + spender birlikte hash'lenir.

### 5.4 Aave v3 Balance ↔ Scaled Balance Dönüşümü

```
scaled_balance = (amount * RAY + index/2) / index  [ray_div, round-half-up]
```

Storage: `packed = (0 << 128) | scaled_balance` (128-bit low half = scaled balance)

`balanceOf` = `scaled_balance * index / RAY` ≈ `amount` (1 wei fark mümkün)

### 5.5 Packed Balance Shift Detection (SENTINEL)

SENTINEL = `0x0102030405...1f20` (distinct bytes, all non-zero)

Balance readback `observed`:
1. Leading zero bytes atlanır
2. Geri kalan `needle` SENTINEL içinde aranır
3. `position` = SENTINEL uzunluğu - start - needle_len → byte count below
4. `shift_bits = byte_count * 8`

Örnekler:
- Full sentinel: `shift_bits = 0` (unpacked)
- `sentinel >> 8`: `shift_bits = 8` (1 byte packed, AUSD isFrozen)
- `sentinel & mask96`: `shift_bits = 0` (narrow uint96, UNI/COMP)

### 5.6 Settlement Clearing Price

```
tokens[2*i]         = sell_token
tokens[2*i+1]       = buy_token
prices[2*i]         = buy_amount  (sell_price)
prices[2*i+1]       = sell_amount (buy_price)
```

Uniform clearing: Her trader kendi limit fiyatından daha iyi veya eşit alır.

Provide buy tokens hesabı (sell order için):
```
buy_amount = (sell_price * executed_amount / buy_price) + 1
```
`+1` rounding hatalarını önler.

### 5.7 Order Flags Bit Layout

```
Bit 0:   kind (0=sell, 1=buy)
Bit 1:   partially_fillable
Bit 2-3: sell_token_balance (00=ERC20, 10=External, 11=Internal)
Bit 4:   buy_token_destination (0=ERC20, 1=Internal)
Bit 5-6: signing_scheme (00=EIP712, 01=EthSign, 10=EIP1271, 11=PreSign)
```

---

## 6. DEX UYGULAMA NOTU — NOVA SHIELD DEX İÇİN EYLEMLİ PLAN

### 6.1 EVM Simülasyonu Mimarisi

CoW Protocol'ün simulator crate'i, herhangi bir EVM tabanlı DEX'in simülasyon altyapısı için mükemmel bir referans modeldir. Nova Shield, EVM yerine native Rust VM kullanıyor olsa da şu tasarım kararları doğrudan uygulanabilir:

**Çift backend mimarisi:** Production ve test için ayrı simülasyon backend'leri. Production: gerçek chain state. Test: state override ile kontrol edilebilir ortam.

**State override sistemi:** Herhangi bir kullanıcının, solver'ın veya market maker'ın bakiyesini ve allowance'ını simülasyon ortamında manipüle etme yeteneği. Bu, "solver herhangi bir emir için teklif verebilir" özelliği sağlar.

**Gas estimation saldırı koruması:** `cap_gas_limit()` tarzı bir üst sınır. Kötü niyetli bir kullanıcı/solver'ın sonsuz gas kullanmasını engeller.

### 6.2 Token Kalite Sistemi

Nova Shield DEX her yeni token listing için şunu yapmalı:
1. `trace_callMany` eşdeğeri — 8 sequential simüle edilmiş transfer
2. `fee-on-transfer` tespiti (1 wei tolerans ile)
3. `U256::MAX` approve testi
4. Rebase token tespiti (AMPL, stETH) — ek test gerekli
5. Dinamik kara liste — governance mekanizması ile güncellenebilir

### 6.3 Aave ve Yield-Bearing Token Entegrasyonu

Nova Shield DEX eğer DeFi entegrasyonu (lending market, yield vault) destekleyecekse:
1. `ray_div/ray_mul` implementasyonu zorunlu
2. Packed storage decode mekanizması (shift_bits)
3. Her desteklenen yield token için özel "balance probe + verify" cycle

### 6.4 PropAMM / Programmatic AMM Desteği

`state_override_stream.rs` dosyası, off-chain hesaplanan AMM fiyatlarını simülasyona "enjekte etme" mekanizmasını gösterir. Nova Shield DEX için:
- Market maker'lar intent'lerini WebSocket üzerinden yayınlayabilir
- Simülatör, bu intent'leri sanki on-chain'de gerçekleşmiş gibi hesaplayabilir
- Stale/fresh kontrolü (block mismatch + max_age) aynen uygulanabilir

### 6.5 Wrapper ve Flash Loan Pattern

CoW Protocol'ün wrapper zinciri (A → B → settlement) ve flash loan router'ı, Nova Shield'ın composable transaction modeliyle doğrudan eşleşir. Her wrapper:
1. Gelen calldata'yı parse eder
2. Setup işlemlerini yapar
3. Downstream'i çağırır
4. Post-processing yapar

Encoding: `[length_u16][data][next_address_20b][length_u16][data]...`

### 6.6 Simulation Report Event Model

`SimulationReport` event modeli (WrapperEntered, SettlementEntered, Hook, Transfer, EIP1271Check...) debug tooling için mükemmeldir. Nova Shield DEX'te:
- Solver simulation trace → human-readable report
- Failed settlement nedenini tespit etmek
- EIP-1271 imza maliyetini scoring'e dahil etmek

---

## 7. GENEL DEĞERLENDİRME VE KARŞILAŞTIRMA

### Güçlü Yönler

1. **Katmanlı abstraction:** Tenderly ve Ethereum JSON-RPC aynı `Api` trait üzerinden kullanılıyor. Backend değiştirmek kod değişikliği gerektirmiyor.

2. **Probing depth ve timeout:** Storage slot detection O(n) probe yapar. `verification_timeout=1s` ile yavaş node'lar veya reflection token'ların infinite loop'ları engellenir.

3. **Aave integration:** `probe_aave_token()` 3-step verification ile rogue contract'ları reddeder. `ray_div` Aave'nin exact implementation'ını eşleştirir (bit-for-bit).

4. **Test kalitesi:** Gerçek mainnet transaction replay testleri, production'da karşılaşılan edge case'leri (Aave debt-swap, collateral-swap) belgeleyerek regresyon koruma sağlar.

5. **Cache stratejisi:** Universal vs holder-specific cache key ayrımı (DirectSlot) iyi düşünülmüş. Moka concurrent cache kullanımı thread-safe.

### Zayıf Yönler / Eksiklikler

1. **Euler hack kalıcılığı:** `TODO: REMOVE THIS HACK!` comment'i var ama ne zaman kaldırılacağı belirsiz. Hardcoded contract adresleri maintenance riski.

2. **additionalData sıfır yazılıyor:** Aave `_userState.additionalData` alanı sıfır yazılıyor. Bu alan stability mode, interest rate gibi protocol state içerebilir; gerçek değerle preserve edilmesi daha doğru olur.

3. **Approval detection paralel çalışmıyor:** Candidate stratejiler sequential test ediliyor. Paralel deneme hızlandırabilir.

4. **Cache TTL yok:** Token upgrade veya storage layout değişikliği durumunda cache invalidation mekanizması yok. Servis restart zorunlu.

5. **Bad token detection `transferFrom` yok:** CoW Protocol'ün vault relayer `transferFrom` kullanır; sadece `transfer` test edilmesi eksik.

---

## 8. DOSYA ÖZET TABLOSU

| Dosya | Satır | Ana Sorumluluk |
|-------|-------|----------------|
| simulator/src/lib.rs | 203 | Simulator struct, dual-backend gas/access-list API |
| simulator/src/simulation_builder.rs | 749 | Settlement encoding, state override builder |
| simulator/src/encoding.rs | 756 | ABI encoding, flag bits, wrapper encoding |
| simulator/src/tenderly/mod.rs | 360 | Tenderly HTTP API, nonce strip, instrumentation |
| simulator/src/tenderly/dto.rs | 206 | Request/Response DTO, StateObject, AccessList |
| simulator/src/ethereum/mod.rs | 175 | Ethereum JSON-RPC backend, gas cap |
| simulator/src/ethereum/contracts.rs | 34 | Contract instance factory |
| simulator/src/report.rs | 357 | Call trace → SimulationReport events |
| simulator/src/state_override_stream.rs | 583 | WebSocket propAMM override stream |
| simulator/src/utils/mod.rs | 15 | URL join utility |
| simulator/tests/aave_replay.rs | 321 | Integration tests, Aave debt/collateral swap |
| bad-tokens/src/lib.rs | 22 | TokenQuality enum |
| bad-tokens/src/list_based.rs | 27 | DenyListedTokens HashSet |
| bad-tokens/src/trace_call.rs | 461 | 8-step token transfer detection |
| balance-overrides/src/lib.rs | 392 | StateOverriding trait, StateOverrides impl |
| balance-overrides/src/detector.rs | 97 | SLOAD extraction, mapping_slot_hash |
| balance-overrides/src/balance/mod.rs | 921 | Strategy enum, Detector, detect_shift |
| balance-overrides/src/balance/aave.rs | 378 | Aave v3 aToken, ray_div, pack_user_state |
| balance-overrides/src/approval/mod.rs | 662 | Approval strategy, dual-mapping detection |

**Toplam:** ~6.720 satır kaynak kod, ~1.250 satır analiz

---

*Agent-23 analizi tamamlandı. Kapsam: simulator, bad-tokens, balance-overrides crate'lerinin tüm 19 .rs dosyası.*
