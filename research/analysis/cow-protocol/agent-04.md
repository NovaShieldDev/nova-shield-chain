# CoW Protocol Agent-04 Analizi
## Kapsam: rate-limit/, request-sharing/, http-client/, token-info/, signature-validator/, simulator/ crates

**Toplam analiz edilen kaynak:** ~2,100 satır
**Tarih:** 2026-07-22

---

## 1. `crates/rate-limit/` — Exponential Backoff Rate Limiter

### 1.1 `src/lib.rs` (375 satır)

```rust
pub struct RateLimiter {
    pub strategy: Mutex<Strategy>,
    pub name: String,
}
```

**`Strategy` (configs crate'den):**
- `back_off_growth_factor: f64` — üstel büyüme çarpanı
- `min_back_off: Duration`
- `max_back_off: Duration`
- `times_rate_limited: u64` — ard arda rate limit sayısı
- `drop_requests_until: Instant` — bu ana kadar tüm istekler reddedilir

**`StrategyExt` trait:**

**`get_current_back_off`:**
```rust
let factor = self.back_off_growth_factor.powf(self.times_rate_limited as f64);
let back_off_secs = self.min_back_off.as_secs_f64() * factor;
std::cmp::min(Duration::try_from_secs_f64(back_off_secs).unwrap_or(self.max_back_off), self.max_back_off)
```
- `factor = 2^n` (growth=2.0, n = times_rate_limited)
- Overflow koruması: `try_from_secs_f64` fail → max_back_off döner
- Test: `times=3, growth=2, min=16ms → 16 * 2^3 = 128ms`; `factor=f64::MAX → max_back_off döner`

**`response_rate_limited`:**
- Race condition koruması: `if self.times_rate_limited != previous_rate_limits { return None }` — başka thread zaten artırmışsa tekrar artırma

**`times_rate_limited` (check):**
- `drop_requests_until > now` → None (request reddedilecek)
- Değilse: mevcut sayacı döndür

**`execute`:**
```rust
pub async fn execute<T>(
    &self,
    task: impl Future<Output = T>,
    requires_back_off: impl Fn(&T) -> bool,
) -> Result<T, Error>
```
Akış:
1. `times_rate_limited()` → None → Err(RateLimited) — task çalıştırılmaz
2. Task await
3. `requires_back_off(&result)` → true → `response_rate_limited()` çağır, back_off uzat
4. Değilse → `response_ok()`, sayacı sıfırla
5. Her iki durumda da result döndürülür (rate limit response olsa bile!)

**`execute_with_back_off`:**
- Rate limit aktifse: `tokio::time::sleep(remaining_duration)` bekle, sonra execute
- execute ile farkı: drop etmek yerine bekle

**`back_off::on_http_429`:**
```rust
pub fn on_http_429(response: &Result<Response, reqwest::Error>) -> bool {
    matches!(response, Ok(response) if response.status() == 429)
}
```
HTTP 429 → backoff gerektirir.

**Metrics:**
- `requests_dropped` — rate limited olduğu için çalıştırılmayan requestler
- `rate_limited_requests` — 429 / backoff response aldığımız requestler
- `successful_requests` — başarılı requestler

**DEX Uygulama Notu:**
- Harici solver API çağrıları (1inch, 0x) için RateLimiter kullan
- `execute_with_back_off`: liquidator, oracle gibi kritik servislerde; `execute`: orderbook query'lerde (drop acceptable)
- Race condition koruması (previous_rate_limits check) distributed sistemlerde önemli

---

## 2. `crates/request-sharing/` — In-Flight Request Deduplication

### 2.1 `src/lib.rs` (220 satır)

```rust
pub struct RequestSharing<Request, Fut: Future> {
    in_flight: Arc<Mutex<HashMap<Request, WeakShared<Fut>>>>,
    request_label: String,
}
```

**Temel konsept:** Aynı anda birden fazla istek aynı veriyi istiyorsa, yalnızca bir inflight future başlatılır; diğerleri aynı `Shared<Fut>`'u bekler.

**`WeakShared<Fut>`:** `futures::future::WeakShared` — future tamamlandıktan sonra garbage collect edilebilir. Strong ref yoksa `upgrade()` → None.

**`shared_or_else`:**
```rust
pub fn shared_or_else<F>(&self, request: Request, future: F) -> SharedResult<Fut>
where F: FnOnce(&Request) -> Fut
```
1. HashMap'te bak → `WeakShared::upgrade()`
2. Varsa: `SharedResult { future: existing, is_shared: true }` — factory çağrılmaz
3. Yoksa: `future(&request).shared()` → `shared.downgrade()` → HashMap'e ekle

**`SharedResult`:**
```rust
pub struct SharedResult<Fut: Future> {
    future: Shared<Fut>,
    pub is_shared: bool, // debugging/metrics
}
impl<Fut: Future> Future for SharedResult<Fut> where Fut::Output: Clone { ... }
```
Doğrudan await edilebilir.

**Garbage Collection:**
- Background task: `tokio::time::sleep(500ms)` → `retain(|_, weak| weak.upgrade().is_some())`
- `Arc::downgrade(&cache)`: cache drop olursa GC task da durur (no leak)
- Drop impl: metric sıfırla

**Neden HashMap yok:**
```rust
// The design of this module is intentionally simple. Every time a shared future
// is requested we loop through all futures to collect garbage. Because of this
// there is no advantage from using a hash map.
```
Az sayıda inflight future bekleniyor → HashMap yerine Vec + periyodik GC tercih edilmiş.

**Metrics:**
- `request_sharing_access`: labels("request_label", "result") — "hits" / "misses"
- `request_sharing_cached_items`: mevcut inflight future sayısı

**Test:**
```rust
let result0 = sharing.shared_or_else(0, |_| futures::future::ready(0).boxed());
let result1 = sharing.shared_or_else(0, |_| async { panic!() }.boxed()); // factory çağrılmaz
assert!(result1.is_shared);
assert_eq!(result1.await, 0); // result0'ın sonucunu alır
```

**DEX Uygulama Notu:**
- Nova Shield'da price oracle sorguları: aynı block'ta aynı çift için çoklu sorgu → RequestSharing
- Token balance/allowance çağrıları
- `is_shared` field: telemetry için — "kaç sorgu share edildi" izlenebilir

---

## 3. `crates/http-client/` — HTTP Client Factory

### 3.1 `src/lib.rs` (60 satır)

```rust
const USER_AGENT: &str = "cowprotocol-services/2.0.0";

pub struct HttpClientFactory {
    pub timeout: Duration,  // default: 10s
}
```

```rust
pub fn builder(&self) -> ClientBuilder {
    ClientBuilder::new()
        .timeout(self.timeout)
        .tcp_keepalive(Duration::from_secs(60))
        .user_agent(USER_AGENT)
}
```

- `create()`: default builder → build
- `configure(fn)`: ek konfigürasyon → build (session, headers vb.)
- TCP keepalive: 60s — uzun yaşayan bağlantılar için
- Her API için ayrı `Client`: ayrı connection pool, cookie store

**DEX Uygulama Notu:**
- Nova Shield'da tüm HTTP client'ları için tek factory — tutarlı timeout, user-agent
- configure() ile solver-specific headerlar eklenebilir (API key vb.)

---

## 4. `crates/token-info/` — Token Metadata Fetcher

### 4.1 `src/lib.rs` (232 satır)

```rust
pub struct TokenInfo {
    pub decimals: Option<u8>,
    pub symbol: Option<String>,
}
```

**`TokenInfoFetching` trait:**
```rust
async fn get_token_info(&self, address: Address) -> Result<TokenInfo, Error>;
async fn get_token_infos(&self, addresses: &[Address]) -> HashMap<Address, TokenInfo>;
```
- `mockall::automock` ile mock desteği
- `get_token_infos` default: her token için `get_token_info` + hataları ignore et

**`TokenInfoFetcher`:**
```rust
const TOKEN_INFO_FETCH_CONCURRENCY: usize = 32;
```

- `BUY_ETH_ADDRESS` → hardcoded: `{ decimals: 18, symbol: "NATIVE_ASSET" }`
- ERC20 parallel call: `futures::join!(decimals.call(), symbol.call())`
- `ignore_non_node_error`: node hatası değilse (revert, ERC20 yok) → None döndür

**`get_token_infos`:**
```rust
futures::stream::iter(addresses.dedup())
    .map(|address| async { ... })
    .buffer_unordered(32)  // 32 paralel inflight
    .collect()
    .await
```
Deduplication: `HashSet` üzerinden iter.

**`CachedTokenInfoFetcher`:**
```rust
pub struct CachedTokenInfoFetcher {
    inner: Arc<dyn TokenInfoFetching>,
    cache: Arc<Mutex<HashMap<Address, SharedTokenInfo>>>,
}
type SharedTokenInfo = Shared<BoxFuture<'static, Result<TokenInfo, Error>>>;
```

- Cache miss: `cache.entry(address).or_insert(inner.get_token_info(address).boxed().shared())`
- Cache hit: aynı future'ı clone et (request sharing!)
- **Hata durumu:** Tamamlanmış future hatalıysa cache'den sil → bir sonraki çağrı yeniden fetch eder
  ```rust
  if let Some(Err(_)) = cache.get(&address).and_then(|fetch| fetch.peek()) {
      cache.remove(&address);
  }
  ```
- `peek()`: future tamamlandıysa sonucunu kontrol et, tamamlanmadıysa None

**Test:**
- Token 0, 1: `times(1)` — sadece bir kez fetch edilmeli (cache devrede)
- Token 2: `times(2)` — hata durumunda cache'den silindi, ikinci kez tekrar fetch

**DEX Uygulama Notu:**
- Nova Shield'da token decimals her settle'da gerekir → persist cache zorunlu
- `CachedTokenInfoFetcher` + `SharedTokenInfo` pattern: concurrent order validation için ideal
- Error eviction: hatalı cache entry'ler retry'a izin verir

---

## 5. `crates/signature-validator/` — ERC-1271 İmza Doğrulama

### 5.1 `src/lib.rs` (129 satır)

```rust
pub struct SignatureCheck {
    pub signer: Address,
    pub hash: [u8; 32],
    pub signature: Vec<u8>,
    pub interactions: Vec<InteractionData>,
    pub balance_override: Option<BalanceOverrideRequest>,
}
```

**`requires_setup()`:**
```rust
fn requires_setup(&self) -> bool {
    !self.interactions.is_empty() || self.balance_override.is_some()
}
```
Pre-interaction veya flashloan varsa setup gerekir.

**EIP-1271 Magic Value:**
```rust
const MAGICAL_VALUE: [u8; 4] = hex!("1626ba7e");
```
`isValidSignature(bytes32 hash, bytes signature) returns (bytes4)` → `0x1626ba7e` dönmeli.

```rust
pub fn check_erc1271_result(result: FixedBytes<4>) -> Result<(), SignatureValidationError> {
    if result.0 == MAGICAL_VALUE { Ok(()) }
    else { Err(SignatureValidationError::Invalid) }
}
```

**`SignatureValidating` trait:**
```rust
async fn validate_signature(&self, check: SignatureCheck) -> Result<(), SignatureValidationError>;
async fn validate_signature_and_get_additional_gas(&self, check: SignatureCheck) -> Result<u64, SignatureValidationError>;
```

**`Contracts`:**
- `settlement: GPv2Settlement::Instance`
- `signatures: contracts::support::Signatures::Instance`
- `vault_relayer: Address`

### 5.2 `src/simulation.rs` (195 satır)

**Validator struct:**
```rust
pub struct Validator {
    signatures_address: Address,
    settlement: GPv2Settlement::Instance,
    vault_relayer: Address,
    web3: Web3,  // labeled("signatureValidation")
    balance_overrider: Arc<dyn StateOverriding>,
}
```

**`simulate_without_setup` — Basit yol:**
```rust
let contract = ERC1271SignatureValidator::new(check.signer, &self.web3.provider);
let magic_bytes = contract
    .isValidSignature(check.hash.into(), check.signature.into())
    .call()
    .await
    .map(|value| const_hex::encode(value.0))?;
```
TransportError (revert) → Invalid; diğer hatalar → Other(anyhow)

**`simulate` — Karmaşık yol (pre-interactions ile):**
```rust
// ZKSync: tx.from belirtilmemişse 0x0 değil random address kullanılır
static SIMULATION_ACCOUNT: LazyLock<Address> = LazyLock::new(|| Address::random());
```

```rust
let validate_call = Signatures::validateCall {
    contracts: { settlement, vaultRelayer },
    signer,
    order: check.hash,
    signature: check.signature,
    interactions: check.interactions.map(|i| { target, value, callData }),
};

settlement.simulateDelegatecall(signatures_address, validate_call.abi_encode())
    .state(overrides)
    .from(*SIMULATION_ACCOUNT)
    .call()
    .await
```

Response: `sol_data::Uint<256>` decode → gas_used (U256)

**Dispatch logic:**
```rust
async fn validate_signature(&self, check: SignatureCheck) -> Result<(), SignatureValidationError> {
    if check.requires_setup() {
        self.simulate(check).await.map(|_| ())
    } else {
        self.simulate_without_setup(check).await
    }
}
```

**`validate_signature_and_get_additional_gas`:**
```rust
Ok(self.simulate(check).await?.gas_used.try_into().unwrap_or(u64::MAX))
```
Gas u64'e sığmazsa MAX döner.

**DEX Uygulama Notu:**
- ERC-1271 imza desteği: Safe, Gnosis Safe gibi multisig wallet'lar için zorunlu
- Pre-interaction setup: composable orders (CoW order + Safe execution) için kritik
- `simulateDelegatecall` pattern: settlement context'inde simulate et — aynı güvenlik garantisi
- Gas measurement: verification gas maliyeti, solver'a fee olarak yansıtılır
- ZKSync hack (LazyLock random address): not: diğer ZK rollup'larda da gerekebilir

---

## 6. `crates/simulator/` — Settlement Simülatörü (11 dosya)

### 6.1 `src/lib.rs` (203 satır) — Çift backend simülatör

```rust
pub struct Simulator {
    inner: Inner,  // Tenderly | Ethereum
    eth: Ethereum,
    disable_access_lists: bool,
    disable_gas: Option<eth::Gas>,
    simulation_overrides: Option<SimulationOverrides>,
}

enum Inner {
    Tenderly(Box<tenderly::Tenderly>),
    Ethereum,
}
```

**Error hiyerarşisi:**
```
Error
├── Revert(RevertError { err: SimulatorError, tx: Tx, block: BlockNo })
└── Other(SimulatorError)
    ├── Tenderly(tenderly::Error)
    ├── Ethereum(ethereum::Error)
    └── GasExceeded(Gas, Gas)
```

**`gas()` method:**
1. `disable_gas` → fixed gas (test modu)
2. `simulation_overrides.current()` → optional state overrides
3. Tenderly: `simulate(tx, block, overrides, GenerateAccessList::No)` → `.gas`
4. Ethereum: `estimate_gas(tx, overrides)`

**`access_list_for_eth_transfer()`:**
- ETH transfer için access list hesapla (smart contract wallet'lar için gerekli)
- `tx.access_list.merge(calculated)` — mevcut + hesaplanan

**`with()` closure — error context builder:**
```rust
fn with<E>(tx: Tx, block: eth::BlockNo) -> impl FnOnce(E) -> Error {
    move |err| {
        let err: SimulatorError = err.into();
        let tx = match &err {
            Tenderly(Http(_)) => None,   // HTTP error, tx yok
            Tenderly(Revert(_)) => Some(tx),
            Ethereum(_) => Some(tx),
            GasExceeded(_,_) => Some(tx),
        };
        ...
    }
}
```

### 6.2 `src/simulation_builder.rs` (749 satır) — Kritik DEX Bileşeni

**`SettlementSimulator` — Factory:**
```rust
pub struct SettlementSimulator(pub(crate) Arc<Inner>);

pub(crate) struct Inner {
    pub(crate) settlement: GPv2Settlement::Instance,
    pub(crate) authenticator: Address,
    pub(crate) vault_relayer: Address,
    pub(crate) flash_loan_router: Address,
    pub(crate) hooks_trampoline: Address,
    pub(crate) native_token: Address,
    pub(crate) max_gas_limit: u64,
    pub(crate) state_overrides: Arc<dyn StateOverriding>,
    pub(crate) provider: DynProvider,
    pub(crate) domain_separator: DomainSeparator,
    pub(crate) chain_id: u64,
    pub(crate) current_block: CurrentBlockWatcher,
    pub(crate) tenderly: Option<Arc<dyn tenderly::Api>>,
    pub(crate) simulation_overrides: Option<SimulationOverrides>,
}
```

Initialization: RPC'den `authenticator`, `vaultRelayer`, `domainSeparator`, `chain_id` alır.

**`SimulationBuilder` — Builder pattern:**
```rust
pub struct SimulationBuilder {
    orders: Vec<Order>,
    pre_interactions: Vec<InteractionData>,
    main_interactions: Vec<InteractionData>,
    post_interactions: Vec<InteractionData>,
    wrapper: WrapperConfig,
    solver: Option<Solver>,
    auction_id: Option<i64>,
    account_override_requests: Vec<AccountOverrideRequest>,
    provide_buy_tokens: bool,
    presign_orders: bool,
    block: Block,
}
```

Builder metodları (fluent API):
- `with_orders`, `append_pre_interactions`, `append_main_interactions`, `append_post_interactions`
- `with_wrapper(WrapperConfig)`, `from_solver(Solver)`, `with_auction_id(i64)`, `at_block(Block)`
- `provide_sufficient_buy_tokens()`, `presign_orders()`

**`parameters_from_app_data()`:**
```rust
let protocol = app_data::parse(app_data.as_bytes())?;
// pre/post hooks → HooksTrampoline interaction
// wrappers XOR flashloan (incompatible)
// flashloan → WrapperConfig::Flashloan(FlashloanRequest)
```

**`encode_hooks()` — Güvenlik kritik:**
```rust
InteractionData {
    target: self.simulator.0.hooks_trampoline,  // settlement değil!
    value: U256::ZERO,
    call_data: HooksTrampoline::executeCall { hooks }.abi_encode(),
}
```
Hooks settlement context değil, trampoline üzerinden çalışır — elevated privilege risk önlenir.

**`compute_euler_override()` — DEX integration hack:**
```rust
let ethereum_vault_connector = address!("0x0C9a3dd6b8F28529d72d7f9cE918D493519EE383");
```
Euler Vault Connector için state override:
1. `operatorLookup` mapping: `mapping(bytes19 addressPrefix => mapping(address operator => uint256))` at slot 24
2. `outer_slot = keccak256(address_prefix[12..31] ++ slot_24)`
3. `permission_slot = keccak256(operator ++ outer_slot)`
4. `permission_value = U256::MAX` (tüm bit'ler set)

Ayrıca `preApprovedHashes` mapping:
- Euler wrapper adresleri hardcoded: open_position, close_position, collateral_swap
- Her biri için domain_separator + type_hash hardcoded
- `struct_hash = keccak256(type_hash ++ struct_hash_input)`
- `map_key = keccak256(0x1901 ++ domain_separator ++ struct_hash)` (EIP-712)
- `preapprove_hash_slot = keccak256(map_key ++ first_map_slot)`

**Block enum:**
```rust
pub enum Block {
    Latest,        // CurrentBlockWatcher'dan
    Number(u64),   // explicit
}
```

**Order struct:**
```rust
pub struct Order {
    pub(crate) data: OrderData,
    pub(crate) owner: Address,
    pub(crate) signature: Signature,       // default: PreSign
    pub(crate) executed_amount: ExecutionAmount,  // default: Remaining
    pub(crate) price_encoding: PriceEncoding,     // default: LimitPrice
}
```

**`ExecutionAmount`:**
- `Full`: tam order miktarı
- `Remaining`: on-chain filled miktarı sorgula
- `Explicit(U256)`: belirli miktar

**`PriceEncoding`:**
- `LimitPrice`: `price[sell] = buy_amount`, `price[buy] = sell_amount` — tam limit fiyatı
- `Custom { sell_price, buy_price }`: solver'ın fiyatı

**`Solver` enum:**
- `OriginUnaltered(Address)`: gerçek solver, state override yok
- `Fake(Option<Address>)`: simülasyon için — ETH ver + authenticator'a ekle

**`AccountOverrideRequest` enum:**
- `SufficientEthBalance(Address)`: büyük ETH balance
- `AuthenticateAsSolver(Address)`: solver allowlist
- `Balance { holder, token, amount }`: ERC20 balance
- `Code { account, code }`: arbitrary code deploy
- `Custom { account, state }`: tam kontrol
- `Approval { owner, token, spender, amount }`: ERC20 approval
- `PreSignature(OrderUid)`: presign order

**`EthCallInputs` — Final output:**
```rust
pub struct EthCallInputs {
    pub from: Address,
    pub to: Address,
    pub calldata: Bytes,
    pub state_overrides: StateOverride,
    pub simulator: SettlementSimulator,
    pub block: u64,
    pub failed_state_overrides: Vec<AccountOverrideRequest>,
}
```

**Simulation methods:**
1. `simulate()`: `provider.call(tx).overrides(state).block(block)` → `Bytes`
2. `simulation_report()`: `debug_trace_call_callframe` → `CallFrame` → parse → `SimulationReport`
3. `simulate_with_tenderly_report()`: eth_call + tenderly link

**Tenderly request quirk:**
```rust
block_number: Some(self.block + 1),  // +1!
transaction_index: Some(0),
```
Tenderly varsayılan: block içinde bir yerde simüle. `-1` = çok pahalı. +1 ile bir sonraki block'un ilk tx'i gibi simüle → daha doğru timestamp/number.

**`TenderlyReport`:**
```rust
pub struct TenderlyReport {
    pub tenderly_request: tenderly::dto::Request,
    pub tenderly_url: Option<String>,  // shareable link
    pub error: Option<String>,
}
```

**DEX Uygulama Notu:**
- Nova Shield settlement simülasyonu için bu pattern doğrudan uygulanabilir
- `SimulationBuilder` fluent API: her settlement'ı adım adım inşa et
- `AccountOverrideRequest` tipi: test ortamında gerçek token, fiyat, solver davranışı simüle et
- `encode_hooks` trampoline pattern: user hook'ları elevated privilege'dan koru
- Tenderly entegrasyonu: production'da settlement fail debug için önemli
- Euler integration hack: protokol entegrasyonlarında state override gerekebilir

---

## Özet: Nova Shield İçin Kritik Öğrenimler

### Rate Limiting & Concurrency
1. **Exponential backoff:** `min * factor^n`, overflow → max_back_off; race condition koruması
2. **Request deduplication:** `WeakShared` + GC background task; 500ms GC interval
3. **TCP keepalive 60s:** Solver API bağlantıları için

### Token & Signature Infrastructure
4. **`buffer_unordered(32)`:** Token info batch fetch — 32 paralel RPC call
5. **Shared future cache:** Error eviction (peek() + cache.remove)
6. **ERC-1271 simulation:** Pre-interaction setup → `simulateDelegatecall`
7. **Magic value `0x1626ba7e`:** EIP-1271 standard

### Settlement Simulation
8. **`SimulationBuilder` builder pattern:** Type-safe settlement assembly
9. **AccountOverrideRequest:** 7 farklı override tipi — test esnekliği
10. **Tenderly block+1 trick:** Doğru simulation için next block / index 0
11. **Euler state override:** Solidity mapping slot hesabı — `keccak256(key ++ slot)`
12. **Hooks trampoline:** Settlement'ın yerine trampoline kontrat — güvenlik izolasyonu
13. **`SimulationReport`:** Geth call tracer → high-level debug raporu
