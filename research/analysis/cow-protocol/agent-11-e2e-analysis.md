# CoW Protocol Analiz — Agent 11

## Kapsam: e2e/ — End-to-End Test Kapsamı (66 dosya)

## DEX Özel Protokolü: 3x Hassasiyet

**Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/libs/cow-protocol/crates/e2e/`
**Toplam Dosya:** 66 `.rs` dosyası
**Analiz Tarihi:** 2026-07-22

---

## Dosya Envanteri (66 dosya)

```
src/api/liquorice/server.rs
src/api/mod.rs
src/api/zeroex.rs
src/lib.rs
src/nodes/mod.rs
src/setup/colocation.rs
src/setup/colocation/utils/toml.rs
src/setup/deploy.rs
src/setup/mod.rs
src/setup/onchain_components/mod.rs
src/setup/onchain_components/safe.rs
src/setup/proxy.rs
src/setup/services.rs
src/setup/solver/mock.rs
src/setup/solver/solution.rs
tests/e2e/api_version.rs
tests/e2e/app_data.rs
tests/e2e/app_data_signer.rs
tests/e2e/autopilot_leader.rs
tests/e2e/banned_users.rs
tests/e2e/buffers.rs
tests/e2e/contract_revert.rs
tests/e2e/cors.rs
tests/e2e/cow_amm.rs
tests/e2e/database.rs
tests/e2e/debug_order.rs
tests/e2e/deprecated_endpoints.rs
tests/e2e/eip4626.rs
tests/e2e/eth_integration.rs
tests/e2e/eth_safe.rs
tests/e2e/ethflow.rs
tests/e2e/hooks.rs
tests/e2e/jit_orders.rs
tests/e2e/limit_orders.rs
tests/e2e/liquidity.rs
tests/e2e/liquidity_source_notification.rs
tests/e2e/malformed_requests.rs
tests/e2e/main.rs
tests/e2e/order_cancellation.rs
tests/e2e/order_simulation.rs
tests/e2e/parallel_settlement.rs
tests/e2e/partial_fill.rs
tests/e2e/partially_fillable_balance.rs
tests/e2e/partially_fillable_pool.rs
tests/e2e/place_order_with_quote.rs
tests/e2e/pool_indexer.rs
tests/e2e/protocol_fee.rs
tests/e2e/quoting.rs
tests/e2e/quote_verification.rs
tests/e2e/refunder.rs
tests/e2e/replace_order.rs
tests/e2e/smart_contract_orders.rs
tests/e2e/solver_competition.rs
tests/e2e/state_override.rs
tests/e2e/submission.rs
tests/e2e/token_metadata.rs
tests/e2e/tracking_insufficient_funds.rs
tests/e2e/trades_v2.rs
tests/e2e/uncovered_order.rs
tests/e2e/univ2.rs
tests/e2e/user_surplus.rs
tests/e2e/wrapper.rs
```

---

## 1. SRC/ — Test Altyapisi Kutuphanesi

### 1.1 `src/lib.rs` (7 satir)

Kok modul, `#[macro_use]` ile `setup`, `api`, `nodes` modullerini re-export eder.

**DEX Uygulama Notu:** Her DEX protokolu e2e test altyapisi tek kok modulle organize edilmeli; `#[macro_use]` ile `assert_approximately_eq!` gibi tolerans makrolarina global erisim saglanmali.

---

### 1.2 `src/nodes/mod.rs` (126 satir)

**Yapilar:**
- `struct Node { process: Option<tokio::process::Child> }` -- anvil subprocess lifecycle yonetimi

**Fonksiyonlar:**
- `Node::new()` -- yerel anvil baslatir:
  - `--gas-price 1`, `--gas-limit 10_000_000`, `--base-fee 1`, `--balance 1_000_000`
  - `--chain-id 1`, `--timestamp 1577836800` (Unix epoch 2020-01-01)
  - `--prune-history` (hafiza optimizasyonu)
- `Node::forked(fork_url, block_number)` -- `--fork-url` + opsiyonel `--fork-block-number`
- `spawn_process()` -- stdout'dan "Listening on" bekler, 20s timeout
- `Node::kill()` (async) -- tercih edilen temizleme yolu
- `Drop::drop()` (sync) -- `start_kill()` ile sync fallback

**Sabitler:**
- `NODE_HOST = "http://127.0.0.1:8545"`
- `NODE_WS_HOST = "ws://127.0.0.1:8545"`
- `ANVIL_COMMAND` env var -- Docker gibi ortamlarda custom binary

**Tasarim Kararlari:**
- async `kill()` + sync `Drop` ikili pattern: tokio task icerisinden hem `await` hem de RAII garantisi
- Zaman damgasi sabitleme: test belirleyicilik icin epoch baslangic noktasi

**DEX Uygulama Notu:** Nova Shield Chain e2e testleri icin yerel node altyapisi benzer sekilde kurulmali. `chain-id` sabitleme kritik: EIP-712 domain separator chain-id'ye bagli. Farkli chain-id ile imzalanmis siparisler replay saldirisina acik olur.

---

### 1.3 `src/api/zeroex.rs` (186 satir)

**Yapilar:**
- `struct ZeroExApi { orders: Vec<OrderRecord> }` -- mock 0x orderbook HTTP sunucusu
- `struct Eip712TypedZeroExOrder` -- tam 0x limit order EIP-712 tipi

**Kritik Sabitler:**
```
ZEROEX_LIMIT_ORDER_TYPEHASH = 0xce918627... (keccak256 of type string)
signature_type: 2 = EthSign (0x protocol-utils formati)
```

**`hash_struct()` implementasyonu:**
- 416-byte encoding buffer: typehash(32) + makerToken(32) + takerToken(32) + makerAmount(32) + takerAmount(32) + takerTokenFeeAmount(32) + maker(32) + taker(32) + sender(32) + feeRecipient(32) + pool(32) + expiry(32) + salt(32)
- `keccak256(buffer)` = order struct hash

**`ZeroExDomainSeparator::new()`:**
- `alloy::sol_types::eip712_domain!` makrosu kullanir
- 0x exchange proxy adresi domain separator icin

**DEX Uygulama Notu:** Nova Shield DEX limit order EIP-712 semasi icin 0x protokolu iyi referans. Ancak CoW protokolu 0x'i bir likidite kaynagi olarak gorur -- kendi order tipinden ayri. Nova Shield tek tip kullanmali ve o tipin domain separatoru chain ID ile bagli olmali.

---

### 1.4 `src/api/liquorice/server.rs` (64 satir)

**Yapilar:**
- `struct LiquoriceApi { state: Arc<Mutex<State>>, port: u16 }`
- `State { notification_requests: Vec<notification::post::Request> }`

**Amac:** Likidite kaynagi bildirim protokolu icin mock HTTP sunucu. Driver, settlement sonrasi Liquorice'e `/v1/intent-origin/notification` POST atar; bu sunucu bunu kaydeder.

**Test Invarianti:** Settlement tamamlandiktan sonra Liquorice API'ye en az 1 bildirim gitmeli.

**DEX Uygulama Notu:** Nova Shield DEX harici likidite kaynaklarina (RFQ market maker'lar, off-chain siparis kitabi) settlement bildirimi gondermeli. Bu pattern `rfq_id` bazli settle bildirimi gonderir -- Nova Shield MEV korumali settlement'ta ayni mekanizmayi kullanabilir.

---

### 1.5 `src/setup/mod.rs` (266 satir)

**Kritik Sabitler:**
```rust
TIMEOUT: Duration = Duration::from_secs(30)
NODE_MUTEX: Mutex<()>  // Tek anda bir test calisir
```

**Fonksiyonlar:**
- `wait_for_condition(TIMEOUT, f)` -- 200ms polling, zaman asiminda `Err`
- `AwaitableCondition` trait -- `bool`, `Option<bool>`, `Result<bool>` ucunu de destekler
- `run_test(f)` -- yerel node ile test kosucu
- `run_forked_test(f, fork_url)` -- fork ile
- `run_forked_test_with_block_number(f, url, block_number)` -- sabitlenmis blok

**Guvenlik Mekanizmasi:**
```rust
// Panik yakalama: test iflas etse bile DB temizlenir
AssertUnwindSafe::catch_unwind(|| ...)
```

**Test Hesaplari:**
- BIP-39: `"test test test test test test test test test test test junk"`
- Hesap 0-9: deterministik, her test ayni adreslere sahip

**`assert_approximately_eq!` Makrosu:**
```
Tolerans: +-0.00001%
Alt sinir: 99999999999/100000000000 x beklenen
Ust sinir: 100000000001/100000000000 x beklenen + 1
```

**DEX Uygulama Notu:** Nova Shield DEX testlerinde `wait_for_condition` ile blockchain olaylarini beklemek sart. Polling + timeout pattern yerine event subscription daha verimli olabilir ancak anvil'in sinirli WS destegi nedeniyle polling tercih edilmis.

---

### 1.6 `src/setup/colocation.rs` (274 satir)

**Yapilar:**
```rust
struct SolverEngine {
    name: String,
    endpoint: Url,
    account: TestAccount,
    base_tokens: Vec<Address>,
    merge_solutions: bool,
    haircut_bps: u32,
    submission_keys: Vec<PrivateKey>,
}
```

**Fonksiyonlar:**
- `start_baseline_solver()` -- TOML config dosyasi olusturur, `solvers` binary'yi baslatir
  - Port: `11088/{solver_name}` (sabit konvansiyon)
- `start_solver()` -- `oneshot` kanal ile bind adresini alir
- `LiquidityProvider` enum -- `UniswapV2`, `UniswapV3 { subgraph }`, `ZeroEx { api_port }`
- `start_driver()` -- driver binary icin TOML config uretir
- `start_driver_with_config_override()` -- `utils::toml::merge_raw` ile TOML overlay

**Kritik Driver Config Degerleri:**
```toml
relative-slippage = "0.1"         # %10 slippage toleransi
gas-price-cap = "1000000000000"   # 1000 Gwei max gas fiyati
tx-gas-limit = "45000000"         # Maksimum transaction gas limiti
solving-share-of-deadline = 1.0   # Deadline'in tamamini cozmeye harca
```

**EIP-7702 Paralel Submission:**
```toml
# submission_keys dolu oldugunda:
[[submission-accounts]]
private-key = "0x..."
```

**DEX Uygulama Notu:** Solver altyapisi moduler tutulmali -- baseline solver + external solver ayrimi kritik. Nova Shield icin: baseline solver (dahili AMM rotasi) + harici RFQ solver + MEV-korumali solver. Driver config'de `tx-gas-limit` dikkatli ayarlanmali; cok yuksek limit gas tahminlerini kotulastirir.

---

### 1.7 `src/setup/colocation/utils/toml.rs` (156 satir)

**Fonksiyonlar:**
- `merge_raw(base, overlay)` -- iki TOML Table'i birlestir
- `merge_tables()` -- recursive: tablolar bilestirilir, primitive/array degerler overlay ile tamamen degistirilir

**Test Senaryolari:**
- `test_field_override` -- tek alan override
- `test_section_override` -- bolum override
- `test_array_override` -- dizi tamamen replace edilir (merge edilmez)

---

### 1.8 `src/setup/deploy.rs` (316 satir)

**Ana Yapi:**
```rust
struct Contracts {
    chain_id: u64,
    balancer_vault: BalancerV2Vault,
    gp_settlement: GPv2Settlement,
    signatures: Signatures,
    gp_authenticator: GPv2AllowListAuthentication,
    balances: Balances,
    uniswap_v2_factory: UniswapV2Factory,
    uniswap_v2_router: UniswapV2Router,
    weth: WETH9,
    allowance: Address,           // vault relayer adresi
    domain_separator: DomainSeparator,
    ethflows: Vec<CoWSwapEthFlow>,
    hooks: HooksTrampoline,
    flashloan_router: Option<FlashLoanRouter>,
}
```

**Deployment Sirasi:**
1. WETH9
2. BalancerV2Authorizer
3. BalancerV2Vault
4. UniswapV2Factory + UniswapV2Router
5. GPv2AllowListAuthentication
6. GPv2Settlement
7. Balances + Signatures (settlement yardimcilari)
8. CoWSwapEthFlow x 2 (WETH instance farkli)
9. HooksTrampoline
10. FlashLoanRouter

**`grant_required_roles()`:**
```rust
// Vault relayer'a batchSwap ve manageUserBalance izinleri
role_id::<BatchSwap>() = keccak256(vault_address_padded ++ BatchSwap::SELECTOR)
role_id::<ManageUserBalance>() = keccak256(vault_address_padded ++ ManageUserBalance::SELECTOR)
```

**`default_pool_code()`:**
- `chain_id == 100` (Gnosis): Honeyswap init code
- Diger: Uniswap V2 init code

**Guvenlik Notu:** `GPv2AllowListAuthentication` deployment'ta bos -- testler `make_solvers()` ile solver ekler. Bu beyaz liste sistemi MEV korumasinin temelidir.

**DEX Uygulama Notu:** Nova Shield DEX deployment sirasinda izin yonetimi kritik. Settlement contract once deploy edilmeli, sonra vault relayer rolleri verilmeli. `domain_separator` deployment sonrasi hesaplanir: `keccak256(abi.encode(TYPEHASH, chainId, contractAddress))`.

---

### 1.9 `src/setup/onchain_components/mod.rs` (639 satir)

**Yapilar:**
```rust
struct TestAccount { signer: PrivateKeySigner }
struct AccountGenerator { id: usize }  // id=100500'den baslar
struct MintableToken { contract: ERC20Mintable, minter: Address }
struct CowToken { contract: CowProtocolToken, holder: Address }
```

**Kritik Fonksiyonlar:**
- `make_accounts<N>()` -- N deterministik hesap uretir, ETH gonderir
- `make_solvers<N>()` -- `GPv2AllowListAuthentication.addSolver()` ile whitelist'e ekler
- `make_solvers_forked<N>()` -- `anvil_send_impersonated_transaction_with_config` ile auth manager'i taklit eder
- `deploy_tokens_with_weth_uni_v2_pools<N>()` -- N token + her biri icin WETH pool

**`seed_weth_uni_v2_pools()` -- Uniswap V2 Pool Kurulumu:**
1. `mint(settlement_contract, amount+1)` (K degeri icin)
2. `weth.deposit(amount)`
3. `factory.createPair(token, weth)`
4. `token.approve(router, amount)` + `weth.approve(router, amount)`
5. `router.addLiquidity(token, weth, amount, amount, 0, 0, ...)`
6. Sonuc: baslangic fiyati `token/weth = 1:1`

**`mint_token_to_weth_uni_v2_pool()` -- Pool Dengesi Guncellemesi:**
```rust
// Automine devre disi -> mint(amount+1) -> swap 1 out -> tekrar aktif
// Bu hack: K = x*y degismez ama rezervler guncellenir
```

**`send_wei()` -- Kritik Guvenlik Testi:**
```rust
// Bakiye kontrolu: anvil fork bug'ina karsi
let balance_before = get_balance(to);
send_transaction(to, amount);
assert!(get_balance(to) > balance_before);
```

**`CowToken::permit()` -- EIP-712 Izin:**
```
PERMIT_TYPEHASH = 0x6e71edae12b1b97f4d1f60370fef10105fa2faae0126114a169c64845d6126c
```

**DEX Uygulama Notu:** Uniswap V2 pool kurulum sirasi kritik. `addLiquidity` oncesi token approve sart. Nova Shield DEX kendi AMM'ini deploy ederken benzer sirayi takip etmeli. `K = x * y` degismez carpim formulu: baslangic K degeri maximum slippage'i belirler.

---

### 1.10 `src/setup/onchain_components/safe.rs` (283 satir)

**Yapilar:**
```rust
struct Infrastructure { factory, fallback, singleton, provider }
struct Safe { chain_id, contract: GnosisSafe, owner: TestAccount }
```

**EIP-1271 Imza Formatlari:**

**Pre-validated signature (65 byte):**
```
bytes[0..12]  = 0x00..00
bytes[12..32] = owner_address
bytes[32..64] = 0x00..00
bytes[64]     = 0x01  // v = 1: pre-validated
```

**Safe transaction hash:**
```
SAFE_TX_TYPEHASH = 0xbb8310d4...
hash_struct = keccak256(TYPEHASH ++ to ++ value ++ data_hash ++ operation ++ safeTxGas ++ baseGas ++ gasPrice ++ gasToken ++ refundReceiver ++ nonce)
```

**Safe message hash:**
```
SAFE_MSG_TYPEHASH = 0x60b3cbf8...
hash_struct = keccak256(TYPEHASH ++ message_hash)
```

**Safe domain separator:**
```
SAFE_DOMAIN_TYPEHASH = 0x47e79534...
domain_separator = keccak256(TYPEHASH ++ chain_id ++ contract_address)
```

**`Safe::sign_order()` akisi:**
1. `order.data().hash_struct()` -> EIP-712 struct hash
2. `hashed_eip712_message(domain_separator, struct_hash)` -> full message hash
3. `safe.sign_message(message_hash)` -> Safe EIP-1271 imzasi
4. `order.signature = Eip1271(safe_signature)`

**Guvenlik Testi:** `smart_contract_orders.rs`'te invalid Safe imzasi -> `InvalidEip1271Signature` hatasi test edilir.

**DEX Uygulama Notu:** Nova Shield DEX EIP-1271 destegi kritik -- multisig cuzdanlar (Safe, Argent, Gnosis Safe) icin siparis imzalama. Safe pre-validated imzasi hesap sahipligi dogrulamasini settlement sirasinda bypass eder; guvenli cunku Safe zaten imzalandi.

---

### 1.11 `src/setup/services.rs` (980 satir)

**API Endpoint'leri:**
```
API_HOST = "http://127.0.0.1:8080"
/api/v1/orders                    -- siparis olustur/sorgula
/api/v1/quote                     -- fiyat teklifi al
/api/v2/solver_competition        -- cozucu yarismasi verisi
/api/v1/account/{addr}/orders     -- hesaba gore siparisler
/api/v1/transactions/{hash}/orders-- tx'e gore siparisler
/api/v1/token/{addr}/native_price -- token USD fiyati
/restricted/api/v1/debug/order/{uid} -- siparis debug raporu
/restricted/api/v1/debug/simulation -- siparis simulasyonu
```

**Servis Baslatma:**
- `start_autopilot()` -- merkezi acik artirma yoneticisi
- `start_api()` -- HTTP API (orderbook)
- `start_protocol()` -- ikisini birden baslatir + driver baslatir
- `start_protocol_with_args()` -- ozel config ile
- `start_protocol_external_solver()` -- harici solver ile

**`wait_until_autopilot_ready()`:**
```sql
SELECT COUNT(*) FROM auctions > 0
-- Ilk auction olusana kadar bekler
```

**`clear_database()`:**
```rust
database::clear_DANGER_()
// Deadlock durumunda 10 deneme
```

**`get_user_total_surplus()`:**
- JSON `totalSurplus` alanini U256 decimal olarak parse eder

**DEX Uygulama Notu:** Servis saglik kontrolu icin `wait_until_autopilot_ready()` gibi DB polling mekanizmasi kritik. Nova Shield DEX'te de orderbook servisi "hazir" sinyali vermeden test akisina gecilmemeli.

---

### 1.12 `src/setup/proxy.rs` (210 satir)

**Yapilar:**
```rust
struct ReverseProxy { _server_handle: JoinHandle<()> }
struct ProxyState {
    backends: Arc<RwLock<VecDeque<Url>>>,
    on_request: Option<OnRequest>,
}
```

**Fonksiyonlar:**
- `rotate_backends()` -- basarisiz backend kuyrugun sonuna tasinir
- `handle_request()` -- her backend'i sirayla dener, hepsi basarisizsa 502
- `start_with_callback()` -- `OnRequest` callback, istek iletilmeden once ateslenir

**Kullanim:** `autopilot_leader.rs` ve `limit_orders.rs`'te kullanilir:
- HA failover testi: autopilot oldurulur, proxy ikinciye yonlendirir
- Brotli sikistirma testi: `/solve` isteklerinde `content-encoding: br` header'i kontrol

**DEX Uygulama Notu:** Nova Shield DEX production icin reverse proxy arkasinda calismali. HA testi icin bu pattern mukemmel: leader secimi ile proxy failover kombinasyonu.

---

### 1.13 `src/setup/solver/mock.rs` (122 satir)

**Yapilar:**
```rust
struct Mock {
    state: State,
    url: Url,
}
struct State {
    auctions: Arc<Mutex<Vec<Auction>>>,
    response: Arc<Mutex<ResponseFuture>>,
}
```

**Fonksiyonlar:**
- `configure_response()` -- ham HTTP yanit ayarla
- `configure_solution()` -- `SolverResponse` ile yanit
- `configure_solution_async()` -- async future donen yanit
- `configure_response_async()` -- async HTTP yaniti
- `get_auctions()` -- gelen tum auction verilerini dondur

**Default Davranis:** Bos solutions listesi

**Axum endpoint:** POST `/solve` -> `State.response` future'ini calistirir, auction'u kaydeder

**DEX Uygulama Notu:** Mock solver yapisi DEX test altyapisinin kalbidir. Nova Shield DEX solver API testi icin ayni pattern: configurable response + audit trail (gelen her auction kaydedilir).

---

### 1.14 `src/setup/solver/solution.rs` (99 satir)

**Yapilar:**
```rust
struct JitOrder {
    owner: Address,
    sell: Asset,
    buy: Asset,
    kind: OrderKind,
    partially_fillable: bool,
    valid_to: u32,
    app_data: AppDataHash,
    receiver: Address,
}
```

**`JitOrder::sign()` akisi:**
1. `OrderData` olustur (CoW protokolu formati)
2. EIP-712 struct hash hesapla
3. ECDSA imzala
4. `solvers_dto::solution::JitOrder` + `OrderUid` dondur

**Token Balance Mapping:**
```
SellTokenSource::Erc20     -> SellTokenBalance::Erc20
SellTokenSource::Internal  -> SellTokenBalance::Internal
BuyTokenDestination::TokenHolder -> BuyTokenBalance::Erc20
BuyTokenDestination::Internal   -> BuyTokenBalance::Internal
```

---

## 2. TESTS/E2E/ -- Test Dosyalari

### 2.1 `tests/e2e/main.rs` (52 satir)

Tum test modullerini tek binary'e dahil eden kok dosya.

**Test Coverage Ozeti:** 55+ unique test fonksiyonu, `#[tokio::test]` + `#[ignore]` attribute'lari (CI'da opt-in).

---

### 2.2 `tests/e2e/api_version.rs` (26 satir)

**Test:** `local_node_api_version`

**Senaryo:** `GIT_SHA=2491c6a`, `GIT_BRANCH=branch` env var'lari set edilir, API yaniti `"branch@2491c6a"` dondurmeli.

**Emerged Invariant:** Versiyon formati `{branch}@{sha}` -- deployment traceability icin.

---

### 2.3 `tests/e2e/app_data.rs` (297 satir)

**Test Coverage:**
- `local_node_app_data` -- app_data yasam dongusu
- `local_node_app_data_full_format` -- tum app_data ozelliklerini test eder

**Test Senaryolari:**
1. Bilinmeyen hash ile siparis -> HTTP 404 + siparis reddedilir
2. Hash/icerik uyumsuzlugu -> reddedilir
3. Valid `Both`/`Full` format -> kabul edilir
4. Onceden kayitli hash -> idempotent
5. Invalid metadata -> reddedilir (8KB limit)

**Full App Data Ozellikleri (test edilenler):**
```json
{
  "version": "1.1.0",
  "appCode": "test-app",
  "environment": "test",
  "metadata": {
    "quote": { "slippageBps": 50 },
    "hooks": { "pre": [...], "post": [...] },
    "flashloan": { "lender": "0x...", "token": "0x...", "amount": "1000" },
    "signer": "0x...",
    "replacedOrder": { "uid": "0x..." },
    "partnerFee": { "bps": 100, "recipient": "0x..." }
  }
}
```

**Security Test:** `OrderCreationAppData::Hash` bilinmeyen hash -> siparis reddedilir. Kayit sonrasi ayni hash -> kabul edilir.

**DEX Uygulama Notu:** Nova Shield DEX off-chain siparis metadata'si icin app_data semasi kritik. MEV korumasi metadata'ya eklenebilir: `{ "mevProtection": { "level": "high", "maxSlippage": 50 } }`. 8KB limit asildiginda zincir disi storage referansi (IPFS hash) gerekir.

---

### 2.4 `tests/e2e/app_data_signer.rs` (126 satir)

**Test:** `local_node_order_creation_checks_metadata_signer`

**Guvenlik Senaryolari:**
1. app_data signer != siparis imzacisi -> `WrongOwner` hatasi
2. app_data signer = siparis imzacisi -> kabul edilir
3. EIP-1271 Safe: signer = Safe adresi -> kabul
4. `from` ve `metadata.signer` tutarsizligi -> `AppdataFromMismatch`

**Emerged Security Invariant:** App data icindeki `signer` alani siparis imzacisiyla esmesmeli. Bu, metadata sahipligi dogrulamasidir.

**DEX Uygulama Notu:** Nova Shield DEX siparis metadata dogrulamasi icin kritik pattern. Partner fee recipient dogrulamasi: partner fee'nin gercek partner tarafindan belirlendigi dogrulanmali.

---

### 2.5 `tests/e2e/autopilot_leader.rs` (265 satir)

**Test:** `local_node_dual_autopilot_only_leader_produces_auctions`

**Senaryo:** Iki autopilot, leader election ile HA:
1. `enable_leader_lock = true` ile iki autopilot baslatilir
2. Proxy her ikisine de yonlendirebilir
3. 10 tx ile ilk lider test edilir -- solver1 tum settlement'lari halleder
4. Lider `manual_shutdown` ile oldurulur
5. Follower lider olur -- `/metrics` endpoint'inden izlenir: `leader_lock_tracker_is_leader 1`
6. 10 tx ile yeni lider test edilir -- solver2 tum settlement'lari halleder

**Leader Election Mekanizmasi:** PostgreSQL DB'de kilit tablosu -- her autopilot kilidi almaya calisir, kazanan lider.

**Emerged Invariant:** Tek anda sadece bir autopilot auksiyonu uretir. Lider degisikligi 30s icinde tamamlanir.

**DEX Uygulama Notu:** Nova Shield DEX'te merkezi auctioneer HA kritik. DB tabanli leader election basit ve guvenilir. Nova Shield icin oneri: PostgreSQL advisory lock -- DB zaten kullanildigindan ek altyapi gerekmez.

---

### 2.6 `tests/e2e/banned_users.rs` (108 satir)

**Test:** `forked_node_mainnet_single_limit_order`

**Senaryo:** Mainnet fork (block 24843565)
- `BANNED_USER = 0x7F367cC41522cE07553e823bf3be79A889DEbe1B`
- Bu adres icin quote istegi -> HTTP 403 FORBIDDEN

**Emerged Security Invariant:** Kara listedeki kullanicilari protokol hizmetlerine erisemez.

**DEX Uygulama Notu:** Nova Shield DEX'te sanction list entegrasyonu (Chainalysis/OFAC) benzer pattern ile implement edilmeli.

---

### 2.7 `tests/e2e/buffers.rs` (149 satir)

**Test:** `local_node_buffers`

**Senaryo:** Settlement contract buffer yeniden kullanimi:
1. Settlement contract'a 5 ETH token_b onceden yuklenir
2. Ilk trade: buffer kullanilir (settlement contract bakiyesi -> 0)
3. Ikinci trade: harici Uniswap V2 likiditesi kullanilir

**Assert:** `settlement_contract_balance == U256::ZERO` ilk trade sonrasi

**Matematiksel Detay:** Buffer sistemi settlement maliyetini dusuler -- token transferi yapmak yerine mevcut bakiye kullanilir. Gas tasarrufu: `ERC20.transfer` gaz maliyeti x kullanilan miktar.

**DEX Uygulama Notu:** Nova Shield DEX settlement contract likiditesi icin atil tokenlar aktif siparis karsimalari icin kullanilabilir. Guvenlik riski: settlement contract kontrolsuz varlik biriktirir. Circuit breaker ile izlenmeli.

---

### 2.8 `tests/e2e/contract_revert.rs` (61 satir)

**Test:** `forked_node_mainnet_asset_call_is_contract_revert`

**Senaryo:** EIP-4626 olmayan tokenlar icin `asset()` cagrisinin siniflandirilmasi:
- sUSDe: `asset()` basarili, sifir olmayan adres dondurur
- USDC, WETH, BNT, STETH, GNO: `asset()` `is_contract_revert()` olarak siniflandirilmali

**Emerged Invariant:** Transport hatasi ile kontrat revert'i ayrimi kritik -- yanlis siniflandirma EIP-4626 unwrap algoritmasini bozar.

**DEX Uygulama Notu:** Nova Shield DEX EIP-4626 vault destegi icin `asset()` revert siniflandirmasi sart. "Bos revert" (data=0x) ile "transport hatasi" farkli davranis tetiklemeli.

---

### 2.9 `tests/e2e/cors.rs` (119 satir)

**Testler:**
- `local_node_cors_preflight` -- OPTIONS -> 200 + tum HTTP metotlari + zorunlu headerlar
- `local_node_cors_headers_on_error` -- olmayan path -> 404 + CORS headerlari hala mevcut

**Test Edilen CORS Headerlari:**
- `access-control-allow-origin: *`
- `access-control-allow-methods: GET, POST, DELETE, OPTIONS, PUT`
- `access-control-allow-headers: *`

**DEX Uygulama Notu:** Nova Shield DEX API'si web UI ile entegre olacaksa CORS kritik. Hata yanitlarinda da CORS headeri gonderilmeli -- aksi takdirde browser preflight basarisiz olur.

---

### 2.10 `tests/e2e/cow_amm.rs` (1082 satir) -- DEX Cekirdegi

**Test Coverage:** 3 major test fonksiyonu

#### `local_node_cow_amm_jit` -- CoW AMM JIT Siparisi

**Senaryo:**
1. `CowAmmConstantProductFactory` ile CoW AMM deploy edilir
2. AMM oracle fiyati: 300 DAI/WETH; AMM rezerv fiyati: 2000 DAI/WETH (arbitraj var)
3. Mock solver JIT siparis olusturur: sell 0.1 WETH, buy 230 DAI, fee 0.01 WETH
4. Kullanici siparisi: ayni yon
5. Pre-interaction: `generate_commit_interaction` AMM taahhudu

**EIP-1271 Imzasi Akisi:**
```
cow_amm::gpv2_order::generate_eip1271_signature(
    amm_address, order_data, domain_separator
)
```

**Assert Hedefleri:**
- AMM surplus alir: AMM gercek oracle fiyatindan daha iyi fiyat
- Kullanici surplus alir: kullanici limit fiyatindan daha iyi fiyat

**Matematiksel Invariant (Constant Product):**
```
x * y = k  (her swap sonrasi)
Yeni fiyat = sqrt(k / (sell_token_reserve / buy_token_reserve * oracle_price))
```

#### `forked_node_mainnet_cow_amm_driver_support` -- Driver Otomatik JIT

**Senaryo:** Fork at block 20476674
1. Driver, zincir uzerinde indexed CoW AMM'leri otomatik tespit eder
2. USDC/WETH AMM dengesizlestirilir (%33 ekstra WETH)
3. Driver bu dengesizligi tespit edip JIT siparis uretir

**Assert Hedefleri:**
- `amm_usdc_balance` degisir (swap gerceklesti)
- `surplus_capturing_jit_order_owners` indexed listede AMM adresi var
- Native prices available for AMM tokens

**EmptyPoolRemoval Testi:**
- Pendle token'i sifir bakiyeli AMM'den drain edilir
- `assert!(balance == 0)` (bos havuz temizlendi)

#### `local_node_cow_amm_opposite_direction` -- Zit Yonde AMM

**Senaryo:** AMM token-A/WETH, kullanici DAI->WETH satiyor
- Quote verification: `quote_response.verified == true`
- `bob_weth_received >= user_order.buy_amount`
- `amm_weth_sent == cow_amm_order.sellAmount`

**DEX Uygulama Notu (3x Hassasiyet):**

CoW AMM -- Nova Shield DEX'in En Kritik Ogrenme Noktasi:

1. **Constant Product AMM formulu:** `x * y = k` -- Oracle price ile karsilastirilarak JIT siparis olusturulur. Nova Shield icin: concentrated liquidity (UniswapV3 tarzi) daha verimli ancak karmasiklik artar.

2. **JIT (Just-In-Time) Siparis:** Solver, settlement aninda AMM'in de bir siparis verdigini bildirir. Bu siparis settlement sirasinda calisir ve AMM hem likiditor hem kullanici olur. Nova Shield bu pattern'i MEV saldiri onleme icin uygulayabilir.

3. **EIP-1271 ile On-chain AMM:** AMM'in "imzasi" `isValidSignature(hash, sig)` cagrisi ile dogrulanir. Nova Shield DEX'te on-chain AMM'ler icin ayni EIP-1271 pattern kullanilmali.

4. **Dengesizlik Tespiti:** Driver periyodik olarak AMM rezervlerini kontrol eder ve oracle fiyatindan sapma varsa JIT siparis uretir. Gas cost < arb profit oldugunda tetiklenmeli.

5. **EmptyPoolRemoval:** Bos havuzlar indexer'dan kaldirilmali, aksi halde stale JIT siparisler olusturulur. Nova Shield icin: pool bakiye threshold'u belirlenmeli (ornek: < 0.01 ETH degerinde ise atla).

---

### 2.11 `tests/e2e/database.rs` (72 satir)

**Yardimci Fonksiyonlar (test veritabani sorgulari):**

```sql
-- Siparis event'leri (zaman sirali)
SELECT * FROM order_events WHERE order_uid = $1 ORDER BY timestamp ASC

-- Quote metadata
SELECT metadata FROM quotes WHERE id = $1

-- Auction katilimcilari
SELECT DISTINCT ps.solver FROM proposed_solutions WHERE auction_id = $1

-- Auction fiyatlari
SELECT token, price FROM auction_prices WHERE auction_id = $1

-- Referans skorlari
SELECT solver, score FROM reference_scores WHERE auction_id = $1

-- Son auction ID
SELECT MAX(id) FROM auctions
```

**DEX Uygulama Notu:** Bu sorgular settlement audit trail olusturur. Nova Shield DEX'te her settlement icin benzer kayitlar tutulmali: siparis olusturma, quota, auction, cozum onerisi, kazanan, settlement bloku.

---

### 2.12 `tests/e2e/debug_order.rs` (277 satir)

**Test Coverage:**
- `local_node_debug_order` -- kapsamli debug endpoint testi
- `local_node_debug_order_filter_reason` -- yetersiz bakiye filtreleme

**Debug Report Yapisi (assert edilen alanlar):**
```json
{
  "order_uid": "0x...",
  "events": [
    { "label": "created" },
    { "label": "ready" },
    { "label": "executing" },
    { "label": "traded" }
  ],
  "trades": [...],
  "auctions": [
    {
      "native_prices": { "token_address": "price" },
      "proposed_solutions": [...],
      "executions": [...],
      "settlement_attempts": [...]
    }
  ]
}
```

**Filter Reason Test:**
- WETH withdraw sonrasi siparis invalid
- Event: `{ "label": "invalid", "reason": "insufficient_balance" }`
- Invalid olmayan event'lerde reason = None

---

### 2.13 `tests/e2e/deprecated_endpoints.rs` (88 satir)

**Test:** `local_node_single_order_cancellation`

**Eski tek siparis iptal endpoint'i:** DELETE /api/v1/orders/{uid}:
1. `OrderCancellation` ile EIP-712 iptal imzasi olustur
2. `CancellationPayload { signature, signing_scheme }` POST
3. Yanit: `"Cancelled"` string
4. Siparis durumu: `Status::Cancelled`

---

### 2.14 `tests/e2e/eip4626.rs` (417 satir) -- DEX Fiyatlama

**Test Coverage:** 4 test fonksiyonu

#### `forked_node_mainnet_eip4626_native_price`
- sDAI (0x83F20F44...) -> EIP-4626 native price estimator
- sDAI/DAI donusum oranindan DAI/ETH x donusum orani = sDAI/ETH

#### `forked_node_mainnet_eip4626_recursive_native_price`
**Recursive Unwrap Matematik:**
```
MockERC4626Wrapper rates:
  wrapper_3_2: 3/2 shares/asset ratio
  wrapper_2_1: 2/1 shares/asset ratio
  wrapper_1_3: 1/3 shares/asset ratio

wrapper_2_1 native price = sDAI_baseline x 4/3
wrapper_1_3 native price = sDAI_baseline x 2/9
tolerans: < 1%
```

#### `forked_node_mainnet_eip4626_decimal_mismatch_native_price`
**Decimal Normalizasyon:**
```
18-decimal wrapper, 6-decimal USDC:
  per-atom factor = 10^-12

6-decimal wrapper, 18-decimal DAI:
  per-atom factor = 10^12
```

#### `forked_node_mainnet_eip4626_empty_revert_terminal_token`
- USDC `asset()` empty-revert -> unwrap algoritmasi DURMAMALI
- Empty revert terminal token olarak isaretlenir

**DEX Uygulama Notu (3x Hassasiyet):**

EIP-4626 Vault Fiyatlama -- Nova Shield DEX Icin Kritik:

1. **Recursive fiyatlama:** `vault.convertToAssets(1e18)` ile anlik donusum orani alinir. Nested vault: `share_price = vault1.convertToAssets(1) * vault2.convertToAssets(1) / base_token_price`.

2. **Decimal mismatch handling:** `normalize_factor = 10^(18 - asset_decimals)`. Negatif normalize faktoru sikca gozden kacan hata. Nova Shield fiyatlamada her token icin `decimals()` on-chain cagrisi yapmali.

3. **Empty revert terminal token:** `asset()` bos revert -> terminal token. Unwrap dongusu durmali. Nova Shield icin: terminal token listesi cache'lenmeli.

4. **Guvenlik:** Manipule edilmis `convertToAssets` degeri flash loan ile price oracle saldirisina yol acabilir. Time-weighted average kullanilmali.

---

### 2.15 `tests/e2e/eth_integration.rs` (120 satir)

**Test:** `local_node_eth_integration`

**Senaryo:** `BUY_ETH_ADDRESS` (0xEEEE...EEE) ile native ETH siparis:
1. ETH yalnizca buy token olarak desteklenir
2. `quote(BUY_ETH_ADDRESS, token)` -> 400 Bad Request
3. Iki trader token satip ETH aliyor

**Assert:** Her iki trader ETH bakiyesi +49 ETH

**Emerged Invariant:** `BUY_ETH_ADDRESS != WETH address` (test ediyor `assert_ne!`)

---

### 2.16 `tests/e2e/eth_safe.rs` (87 satir)

**Test:** `local_node_test`

**Senaryo:** Gnosis Safe ile ETH siparis:
1. Safe deploy edilir, token'lar mint edilir
2. Siparis: `from = safe.address()`, `buy_token = BUY_ETH_ADDRESS`, `receiver = safe.address()`
3. Signature: `Eip1271(safe.sign_message(hash))`

**Assert:** Safe ETH bakiyesi 3.899 ETH - 4 ETH arasinda (fee nedeniyle)

---

### 2.17 `tests/e2e/ethflow.rs` (1119 satir) -- DEX Giris Noktasi

**Test Coverage:** 6 test fonksiyonu

#### `local_node_eth_flow_tx` -- Tam Ethflow Round-Trip

**Parametreler:**
```
DAI_PER_ETH = 1000
Slippage: 300 bps (3%)
Quote toleransi: +-10%
```

**Hooks Guvenlik Testi:**
```
Pre-hooks: allowance -> HooksTrampoline (settlement contract DEGIL)
Post-hooks: settlement contract has ZERO allowance
WRAP_ALL_SELECTOR: ETH -> WETH pre-interaction
true_valid_to DB alani = ethflow_data.user_valid_to
```

**Guvenlik Invarianti:** Hooks, settlement contract'a dogrudan token calamaz. HooksTrampoline sandbox gorevi gorur.

#### `local_node_eth_flow_native_bridge_post_hook`

**Senaryo:** ETH->ETH "bridge" (ayni token siparisi):
- Counter contract hook'ta tam olarak 1 kez artisi -> idempotency
- WETH->native ETH same-token trade: volume fee YOK
- `same_tokens_policy: AllowSell` gerekli
- `min_delivered = 95% x sell_amount`

#### `local_node_eth_flow_indexing_after_refund`

**Senaryo:** Iptal edilmis + gecerli siparis es zamanli:
1. Sahte (dummy) siparis olustur -> iptal et
2. Gecerli siparis olustur
3. Sadece gecerli siparis settle edilir
4. Dummy siparis `Cancelled` event alir

#### `local_node_eth_flow_zero_buy_amount`

**Senaryo:** `buy_amount=0` ile kotu siparis -> gecerli siparisi engellemez

**Helper Yapisi:**
```rust
struct ExtendedEthFlowOrder(CoWSwapEthFlow::EthFlowOrder::Data) {
    fn from_quote(quote) -> Self
    fn include_slippage_bps(bps) -> Self
    fn mine_order_creation() -> OrderUid
    fn mine_order_invalidation()
    fn uid() -> OrderUid
}
```

**DEX Uygulama Notu (3x Hassasiyet):**

EthFlow -- Native ETH Gateway:

1. **ETH wrapping:** `WRAP_ALL_SELECTOR` pre-interaction ETH'yi WETH'e cevirir. Nova Shield DEX'te native token gateway ayni sekilde calismali.

2. **HooksTrampoline guvenlik:** Hook'lar settlement contract yerine trampoline contract uzerinden calisir. Bu izolasyon MEV ve token calma saldirilairni onler. Nova Shield icin zorunlu.

3. **Expiry ve refund:** `user_valid_to` ethflow kontratinda saklanir, autopilot suresi dolan siparisleri refunder servisine bildirir. Nova Shield icin: on-chain siparis yonetimi gerekiyorsa ayni dual-expiry pattern.

4. **Same-token trade:** WETH->ETH same-token swap volume fee exempt. Nova Shield DEX'te native coin wrapping/unwrapping fee'siz olmali.

---

### 2.18 `tests/e2e/hooks.rs` (628 satir) -- Guvenlik Kritik

**Test Coverage:** 4 test fonksiyonu

#### `local_node_gas_limit`

**Guvenlik Testi:** `gas_limit = 8_000_000` -> `TooMuchGas` hatasi

**Invariant:** Hook gas limiti kontrol edilmeli.

#### `local_node_allowance` -- Hook Guvenlik Testi

**Senaryo:** Kotu niyetli hooks:
1. Pre-hook: settlement contract'a token approve
2. Post-hook: settlement contract'tan token cek

**Assert:** Allowance settlement contract'a DEGIL, HooksTrampoline'e gider.

**Guvenlik Invarianti:** Hooks, settlement contract token calamaz.

#### `local_node_signature` -- Gelecekte Deploy Edilecek Safe

**Senaryo:** Henuz deploy edilmemis Safe'in imzaladigi siparis:
1. Siparis olusturulur (Safe henuz yok)
2. Pre-hooks: `safe_creation` + `approval`
3. Settlement sirasinda Safe deploy edilir
4. EIP-1271 imzasi dogrulanir (Safe artik var)

**Assert:** `safe.code.len > 0` settlement sonrasi

#### `local_node_partial_fills` -- Hook ile Kismi Doldurma

**Senaryo:** Iki kismi doldurma, her biri icin pre+post hook dogru tutarla calisir.

**DEX Uygulama Notu:** Hook siralamas garantilenmeli: pre-hooks trade oncesi, post-hooks trade sonrasi. Partial fill durumunda executed amounts hook'a gecilmeli.

---

### 2.19 `tests/e2e/jit_orders.rs` (246 satir)

**Test:** `local_node_single_limit_order` (jit_orders modulunde)

**Senaryo:**
1. Mock solver hem kullanici siparisi hem JIT siparis iceren cozum dondurur
2. JIT siparis: token->WETH, executed_amount=9 ETH, fee=1 ETH
3. Kullanici siparisi: WETH->token, sell 10 ETH

**Assert Hedefleri:**
- Trader bakiyesi +>=5 ETH (surplus dahil)
- Solver bakiyesi tam olarak -10 ETH

**JIT Siparis Index Kontrolu:**
- `/api/v1/orders` listesinde JIT siparis var
- `/api/v1/trades` listesinde JIT siparis var
- `/api/v1/transactions/{hash}/orders` listesinde JIT var
- `/api/v1/account/{owner}/orders` listesinde JIT var

**Pagination Testi:** `get_orders_for_owner(owner, offset=1)` -> bos liste

**DEX Uygulama Notu:** JIT Siparisler -- Nova Shield DEX'te Merkezi Mekanizma:

1. **Solver-generated liquidity:** JIT siparisler solver'in anlik likidite koymasina izin verir. Nova Shield icin: solver kendi likidite havuzundan JIT siparis uretebilir.

2. **Fee accounting:** JIT siparis fee'si solver'in maliyetidir. Net kar: `executed_amount - fee - gas`.

3. **Index invarianti:** JIT siparisler tum ilgili endpoint'lerde gorunmeli.

---

### 2.20 `tests/e2e/limit_orders.rs` (1682 satir) -- DEX Siparis Turleri

**Test Coverage:** 10 test fonksiyonu:
- `local_node_single_limit_order` -- temel limit siparis akisi
- `local_node_two_limit_orders` -- iki limit siparis + Brotli sikistirma testi
- `local_node_two_limit_orders_multiple_winners` -- coklu kazanan cozum
- `local_node_too_many_limit_orders` -- limit asimi
- `local_node_limit_does_not_apply_to_in_market_orders_test` -- market siparislerine limit uygulanmaz
- `local_node_no_liquidity_limit_order` -- likiditesiz ortamda limit siparis
- `local_node_limit_order_with_haircut` -- satis siparisi haircut testi
- `local_node_buy_order_with_haircut` -- alis siparisi haircut testi
- `forked_node_mainnet_single_limit_order` -- mainnet fork (block 24843565)
- `forked_node_gnosis_single_limit_order` -- Gnosis Chain fork (block 45588623)

**`single_limit_order_test` Detaylari:**
```
sell: 10 ETH token_a
buy: >= 5 ETH token_b
Pool: 1000:1000 token_a:token_b

Assert: balance_after - balance_before >= 5 ETH
Assert: OrderClass::Limit
Assert: quote metadata hidden while fillable -> visible after filled
```

**`two_limit_orders_test` -- Brotli Sikistirma Testi:**
```rust
// Proxy callback: /solve request'lerinde content-encoding: br kontrol et
let on_request: OnRequest = Arc::new(move |parts, _body| {
    if path.contains("/solve") && has_br {
        flag.store(true, Ordering::Release);
    }
});
// compress_solve_request = true ile autopilot config
```

**Haircut Mekanizmasi:**
```
Satis siparisi haircut:
  reported_buy_amount = actual_buy_amount * (1 - haircut)
  on_chain_buy_amount = actual_buy_amount * (1 - haircut)
  Kullanici daha az alir, fark settlement contract'a gider

Alis siparisi haircut:
  executedBuy == signedBuyAmount (kullanici tam istedigini alir)
  executedSell = signedSellAmount * (1 + haircut)
  Kullanici daha fazla oder, fark settlement contract'a gider
```

**DEX Uygulama Notu (3x Hassasiyet):**

Limit Siparisler -- Nova Shield DEX'in Temel Tipi:

1. **Limit vs Market Ayrimi:** Quote metadata limit sipariste gizlenir (fill edilene kadar) -- bu MEV korumasidir.

2. **Haircut Mekanizmasi:** Protocol'un gizli fee toplama yontemi. Nova Shield icin: haircut yerine explicit fee accounting daha seffaf.

3. **Brotli Sikistirma:** `/solve` istekleri brotli ile sikistirilir. 50-70% boyut azaltma. Nova Shield DEX solver API icin ayni sikistirmayi kullanmali.

4. **Likiditesiz ortam:** Minimum likidite esigi altinda siparis "uncovered" olarak isaretlenmeli.

5. **Coklu zincir:** Mainnet ve Gnosis Chain icin ayri fork testleri. Her zincirin farkli DEX init code'u var.

---

### 2.21 `tests/e2e/liquidity.rs` (445 satir)

**Test:** `forked_node_zero_ex_liquidity_mainnet`

**Senaryo:** Fork at block 24843565, 0x likidite entegrasyonu:
1. 0x signed order'lar olusturulur (EIP-712)
2. Mock 0x orderbook API baslatilir
3. Driver 0x API'den order'lari alir
4. USDC->USDT swap, 0x maker likiditey saglar

**DEX Uygulama Notu:** Harici likidite kaynagi entegrasyonu icin referans. Nova Shield DEX'te 0x, 1inch, Paraswap gibi aggregator entegrasyonu benzer pattern ile yapilabilir.

---

### 2.22 `tests/e2e/liquidity_source_notification.rs` (377 satir)

**Test:** `forked_node_liquidity_source_notification_mainnet`

**Senaryo:** Liquorice RFQ integration + settlement bildirimi:
1. USDC trader, USDT Liquorice maker
2. Mock Liquorice solver API ile cozum hazirlanir
3. Liquorice settlement cagrisi: `settleSingle(maker, order, signature, ...)`
4. Settlement sonrasi: driver Liquorice API'ye bildirim gonderir

**Liquorice Order Yapisi:**
```rust
Single {
    rfqId: "c99d2e3f-...",
    nonce: 0,
    trader: settlement_contract_address,
    baseToken: USDC,
    quoteToken: USDT,
    baseTokenAmount: 500,
    quoteTokenAmount: 500,
    minFillAmount: 1,
    quoteExpiry: now + 10,
}
```

**Bildirim Dogrulama:**
```rust
assert!(matches!(notification.content, Content::Settle(Settle {
    rfq_ids,
    ..
}) if rfq_ids.contains(&liquorice_order.rfqId)));
```

**DEX Uygulama Notu:** RFQ (Request for Quote) entegrasyonu -- off-chain market maker ile on-chain settlement. Nova Shield DEX'te RFQ sistemi: 1) Kullanici fiyat ister 2) MM off-chain teklifini imzalar 3) CoW settlement sirasinda on-chain cagrilir.

---

### 2.23 `tests/e2e/malformed_requests.rs` (499 satir) -- API Guvenlik

**Test:** `local_node_http_validation`

**Test Edilen HTTP Durumlari:**

| Durum | HTTP Kodu |
|-------|-----------|
| Gecersiz OrderUid | 400 |
| Gecersiz Address | 400 |
| Gecersiz tx hash | 400 |
| Olmayan endpoint | 404 |
| Yanlis API versiyonu | 404 |
| Gecersiz JSON body | 400 |
| Bos body | 400 veya 411 |
| Eksik zorunlu alanlar | 422 |
| Yanlis alan tipleri | 422 |
| Gecersiz enum degeri | 422 |
| ORDER_UID_LIMIT+1 UID | 400 |
| Sifir sellAmount | 422 |
| Bozuk appData | 400 (hata mesajinda appData ve deger gecer) |

**Hata Format Invariantlari:**
- Deserialization hatalari: plain text, "deserialize"/"missing field" icermeli
- Business logic hatalari (404): JSON `{ errorType, description }` formati
- appData hatasi: hem `"app data"` hem bozuk deger hata mesajinda

**DEX Uygulama Notu:** Nova Shield DEX API guvenlik katmani icin referans. `ORDER_UID_LIMIT` DOS saldirisi onleme.

---

### 2.24 `tests/e2e/order_cancellation.rs` (294 satir)

**Test Coverage:**
- Tek siparis iptal (DELETE /orders/{uid})
- Toplu siparis iptal (DELETE /orders)

**EIP-712 Iptal Yapisi:**
```
OrderCancellations { uids: [uid1, uid2, ...] }
struct_hash = keccak256(TYPEHASH ++ uids_hash)
SignedOrderCancellations { cancellations, signing_scheme, signature }
```

**DEX Uygulama Notu:** EIP-712 uzerinden toplu iptal gas acisindan verimli. Nova Shield DEX'te kullanici birden fazla siparisi tek imzayla iptal edebilmeli.

---

### 2.25 `tests/e2e/order_simulation.rs` (425 satir)

**Test Coverage:**
- `local_node_order_simulation` -- `/restricted/api/v1/debug/simulation`
- `local_node_custom_order_simulation` -- ozel `OrderSimulationRequest`

**Senaryo:**
1. Fonlama oncesi: simulation -> `"reverted"` iceren hata
2. `mint(trader, amount)` + `approve(allowance, amount)`
3. Sonrasi: simulation basarili (error = None)

**DEX Uygulama Notu:** Siparis simulasyonu -- kullaniciya "Bu siparis su an gerceklesebilir mi?" cevabi verir.

---

### 2.26 `tests/e2e/parallel_settlement.rs` (360 satir) -- MEV Korumasi

**Test:** `local_node_parallel_settlement_submission`

**EIP-7702 Paralel Settlement Mekanizmasi:**
```
submission_keys = [submitter_a, submitter_b]

Solver EOA -> delegates to -> Solver7702Delegate contract
Delegate address = CREATE2(CREATE2_DEPLOYER, CREATE2_SALT)
Code = DELEGATION_PREFIX ++ delegate_address

Iki es zamanli /settle cagrisi:
  - Auction 1: submitter_a -> settlement contract (direct tx)
  - Auction 2: submitter_b -> solver EOA (delegated tx via EIP-7702)

Mempool'da beklenen:
  - 1 direct tx (to = settlement contract)
  - 1 delegated tx (to = solver EOA, code = delegation_prefix)
```

**DEX Uygulama Notu (3x Hassasiyet):**

EIP-7702 Paralel Settlement -- Nova Shield DEX'in MEV Savunma Mimarisi:

1. **Paralellik neden onemli:** Sirali settlement -> blok gecikmeleri -> MEV firsati. Paralel settlement -> ayni blokta birden fazla auction -> MEV penceresi kapanir.

2. **EIP-7702 delegation:** EOA gecici olarak kontrat kodu alir. Kontrat yetki devri gaz maliyeti sifir. Nova Shield icin: solver EOA'lari EIP-7702 ile settlement vault'larina delegate edilebilir.

3. **CREATE2 deterministik adres:** `keccak256(0xFF ++ deployer ++ salt ++ init_code_hash)[12:]`. Nova Shield deployment'ta adreslerin onceden bilinmesi critical dependency.

4. **Mempool guvenlik:** Mempool monitoring MEV bot tespitinde kullanilabilir.

---

### 2.27 `tests/e2e/partial_fill.rs` (128 satir)

**Test:** `local_node_test` (kismi doldurma)

**Senaryo:** Sig pool (10:10), 4 ETH WETH sat, 3 ETH token al:
- Pool kismi doldurmayı zorlar
- `sell_balance in [1.999 ETH, 2.000 ETH)` (tam yarisi + fee)
- `buy_balance in [1.650 ETH, 1.670 ETH)`

**EthSign kullanimi:** `SigningScheme::EthSign` (EIP-712 degil)

**Post-trade Assert:**
- `order.metadata.executed_fee > U256::ZERO`
- `Status::Traded(solutions)` -> `solutions[0].solver == "test_solver"`

---

### 2.28 `tests/e2e/partially_fillable_balance.rs` (128 satir)

**Test:** `local_node_partially_fillable_balance`

**Senaryo:** Kullanicinin yetersiz bakiyesiyle kismi doldurma:
- Siparis: sat 100 ETH, al 50 ETH
- Kullanici bakiyesi: sadece 50 ETH
- `sell_balance < 1 ETH`, `buy_balance in [45 ETH, 55 ETH]`

**Invariant:** Kullanici bakiyesi limiti asilam -- siparis limit fiyatiyla execute edilir ama maksimum bakiyeyle.

---

### 2.29 `tests/e2e/partially_fillable_pool.rs` (138 satir)

**Test:** `local_node_partially_fillable_pool`

**Senaryo:** Pool kapasitesi yetersizligiyle kismi doldurma:
- Siparis: sat 500 ETH, al >=390 ETH
- `sell_balance in [249.999 ETH, 250 ETH)` (tam yarisi)
- `buy_balance in [199 ETH, 201 ETH]`

**Matematik:** UniswapV2 constant product formulu:
```
Dy = (y * Dx * 997) / (x * 1000 + Dx * 997)
500 ETH sell -> pool fiyati kayar -> ~249 ETH ile optimal
```

---

### 2.30 `tests/e2e/place_order_with_quote.rs` (375 satir)

**Test Coverage:** 3 test fonksiyonu:
- `local_node_place_order_with_quote_basic` -- temel quote+order akisi
- `local_node_disabled_same_sell_and_buy_token_order_feature` -- ayni token siparisi devre disi
- `local_node_fallback_native_price_estimator` -- fallback fiyatlama

**`place_order_with_quote` Detaylari:**
1. Automine devre disi (settlement kazara gerceklesmesin)
2. Quote al: `BeforeFee` sell amount
3. `quote_response.verified == true`
4. Siparisi quote ile olustur: `quote_id = quote_response.id`
5. DB'de dogrula: `order_quote.verified == quote_response.verified`

**`disabled_same_sell_and_buy_token_order_feature`:**
- `sell_token == buy_token` -> 400 + "SameBuyAndSellToken"
- `WETH sell + BUY_ETH_ADDRESS buy` -> 400 + "SameBuyAndSellToken"

**`fallback_native_price_estimator`:**
1. Autopilot baslar, quote alinir
2. Autopilot `manual_shutdown` ile oldurulur
3. Native price cache 2s sonra expires
4. `FallbackNativePriceEstimator` 3 ProtocolInternal hatasi sonrasi fallback aktif
5. Autopilot olmadan da quote calisir

**DEX Uygulama Notu:** Native price fallback kritik HA ozelligi. Nova Shield DEX'te primary price estimator basarisiz oldugunda secondary (on-chain TWAP, Chainlink) devreye girmeli.

---

### 2.31 `tests/e2e/pool_indexer.rs` (695 satir) -- Likidite Altyapisi

**Test Coverage:** 5 test fonksiyonu

**Mock Kontratlar:**
- `MockUniswapV3Factory` -- PoolCreated event'i yayan fabrika
- `MockUniswapV3Pool` -- initialize + mockMint

**Test Fonksiyonlari:**

#### `local_node_pool_indexer_driver_integration`
- Pool indexer baslatilir (port 7778)
- Driver `pool-indexer-url` ile yapilandirilir
- Driver metrics'ten `/pools`, `/pools/by-ids`, `/ticks` endpoint'lerinin kullanildigi dogrulanir

#### `local_node_pool_indexer_checkpoint_resume`
- Iki kez ardisik calistirma -> ayni sonuc (idempotent)
- `sqrt_price_x96`, `tick`, `liquidity` degerleri degismez
- Checkpoint blok numarasi en az `head`'e ulasmali

#### `local_node_pool_indexer_api_errors`
- Gecersiz pool adresi -> 400
- Bilinmeyen adres -> 200 + bos ticks (hata degil, veri yok)

#### `local_node_pool_indexer_pagination`
- 3 pool olusturulur (fee: 500, 3000, 10000)
- `limit=1` ile cursor pagination, duplicate'siz tam traversal

#### `local_node_pool_indexer_bootstrap_idempotent`
- `--bootstrap-only` onceden seeded DB -> hizli cikis
- Mevcut checkpoint degismez

**DB Semasi:**
```sql
uniswap_v3_pools (address, token0, token1, fee, token0_decimals, token1_decimals)
uniswap_v3_pool_states (pool_address, sqrt_price_x96, tick, liquidity)
uniswap_v3_ticks (pool_address, tick_index, ...)
pool_indexer_checkpoints (contract_address, block_number)
```

**DEX Uygulama Notu (3x Hassasiyet):**

Pool Indexer -- Nova Shield DEX Likidite Takip Sistemi:

1. **Checkpoint sistemi:** Blok numarasina gore resumable indexing. Her yeniden baslastirmada bastan tarama yerine kaldigi yerden devam.

2. **Cursor pagination:** Pool sayisi binleri asan DEX'lerde kritik. `limit + after` parametresi deterministic pagination saglar.

3. **UniswapV3 sqrtPriceX96 formati:** Fiyat = `(sqrtPriceX96 / 2^96)^2`. Nova Shield concentrated liquidity icin ayni format kullanilabilir.

4. **Token decimals backfill:** Pool indexer ERC20 `decimals()` cagrisi yapiyor, sifir ondalik basamak olan tokenlari filtreler. Nova Shield icin: decimal validation pool'a eklenirken yapilmali.

5. **Separate DB:** Pool indexer kendi DB'sine sahip. Nova Shield DEX likidite indexer'i orderbook DB'sinden ayri tutulmali.

---

### 2.32 `tests/e2e/protocol_fee.rs` (1648 satir) -- DEX Ucret Mekanizmasi

**Test Coverage:** 10+ test fonksiyonu

**Ucret Turleri:**

| Tur | Aciklama | Formul |
|-----|----------|--------|
| Surplus | Quote'dan fazla kazanimin yuzdesi | `factor * (executed_buy - quote_buy)` |
| PriceImprovement | Piyasadan iyilestirme yuzdesi | `factor * (market_price - order_price) * amount` |
| Volume | Islem hacminin yuzdesi | `factor * sell_amount` |
| PartnerFee | Partner tarafindan belirlenen ucret | `bps * buy_amount` |

**Kritik Test: `combined_protocol_fees`**
```
market_executed_fee >= 30% of (quote_market - actual_price) * amount
limit_executed_fee >= 30% of (quote_limit - actual_price) * amount
partner_fee >= 2% of buy_amount
```

**`surplus_partner_fee` Testi -- Ust Limit:**
```
MAX_PARTNER_VOLUME_FEE = 37.5%

3 partner fee policy:
  1. Surplus 30%, max 25% -> tam uygulanir
  2. PriceImprovement 30%, max 15% -> 10%'e indirilir (toplam cap'e yaklasyor)
  3. PriceImprovement 30%, max 15% -> 0%'e indirilir (cap asildi)

Matematiksel dogrulama:
  (1 + 0.25) * (1 + 0.10) * (1 + 0) = 1.375 = 1 + 0.375 (correct)
```

**UpcomingFeePolicies Testi:**
```
effective_from_timestamp = gecmiste -> YENİ politika uygulanir
effective_from_timestamp = gelecekte -> ESKI politika uygulanir
```

**Same-Token Fee Testleri:**
- `volume_fee_dropped_on_same_token` -- sell=buy -> fee alinmaz
- `volume_fee_charged_on_same_token_when_enabled` -- `enable_sell_equals_buy_volume_fee=true` -> fee alinir
- `volume_fee_dropped_on_wrapped_to_native_token` -- WETH->ETH -> fee alinmaz

**Fee Dogrulama:**
```rust
fn fee_in_buy_token(order: &Order, quote: &OrderQuote) -> U256 {
    order.metadata.executed_fee * quote.buy_amount / quote.sell_amount
}
// Settlement contract bakiyesi == executed fee
assert_approximately_eq!(fee, settlement_contract_balance);
```

**DEX Uygulama Notu (3x Hassasiyet):**

Protocol Fee Mekanizmasi -- Nova Shield DEX Gelir Modeli:

1. **Cok katmanli fee:** Surplus + Volume + PriceImprovement uc ayri fee policy. Nova Shield DEX baslangiçta basit volume fee ile baslayabilir.

2. **Partner fee limit:** Global `max_partner_fee` olmasi kritik -- sonsuz partner fee MEV saldirisi vektoru. Nova Shield icin: `total_fee <= 0.5%` gibi hard cap.

3. **Upcoming fee policies:** Zaman bazli politika gecisi -- kullanicilar onceden bilgilendirilir. Fee degisikligi minimum 7 gun onceden duyurulmali (governance).

4. **Same-token fee exemption:** Wrap/unwrap islemleri fee'siz olmali. `sell_token == buy_token || (sell_token == WETH && buy_token == NATIVE_ETH)` -> fee = 0.

5. **Settlement contract balance = fee:** Settlement contract, protocol fee icin gecici depo. Fee treasury ayri contract'ta tutulabilir.

---

### 2.33 `tests/e2e/quoting.rs` (1104 satir) -- Fiyatlama Sistemi

**Test Coverage:** 8+ test fonksiyonu

**`local_node_test` -- Gas Fee Karsilastirmasi:**
```
base.fee_amount < with_eip1271.fee_amount (+50k gas EIP-1271 dogrulama)
base.fee_amount < with_hooks.fee_amount (+10k gas pre+post hooks)
base.fee_amount < with_both.fee_amount
```

**`local_node_uses_stale_liquidity`:**
- Ilk quote al
- Pool dengesizlestir
- Ikinci quote ayni (stale cache kullaniyor)
- Cache eviction sonrasi farkli quote

**DEX Uygulama Notu:** Stale cache onemli sorun. Nova Shield DEX cache invalidation politikasi: yeni blok geldiginde pool rezervleri yenile.

---

### 2.34 `tests/e2e/quote_verification.rs` (587 satir)

**Test Coverage:** 3 test fonksiyonu

**`local_node_standard_verified_quote`:**
- Bakiye + approval -> `verified = true`
- State override ile dogrulama (gercek fon gerekmez)

**`local_node_verified_quote_eth_balance`:**
- Native ETH bakiyesi + WETH approval YOK -> yine de `verified = true`
- ETH bakiyesi otomatik olarak WETH karsiligi sayilir

**`local_node_verified_quote_for_settlement_contract`:**
- Settlement contract adresi trader olarak (3 ETH)
- State override ile balance simulasyonu

**DEX Uygulama Notu:** Quote verification -- "Gercekten execute edebilir misin?" sorusunun yaniti. State override ile gercek fon gerektirmeden simulasyon. Nova Shield DEX icin: basarili simulation -> verified = true -> daha iyi UX.

---

### 2.35 `tests/e2e/refunder.rs` (689 satir)

**Amac:** Suresi dolan ethflow siparisleri icin refund servisi

**Sabitler:**
```
SELL_AMOUNT = 0.003 ETH
MAX_GAS_PRICE = 2000 Gwei
SLIPPAGE_BPS = 300
```

**`EthflowOrderBuilder` -- Fluent builder:**
```rust
EthflowOrderBuilder::new()
    .sell_amount(SELL_AMOUNT)
    .slippage_bps(SLIPPAGE_BPS)
    .valid_to(now + 300)
    .build()
```

**`advance_time_past_expiration()`:**
```rust
evm_set_next_block_timestamp(valid_to + 60)
evm_mine()
```

**DEX Uygulama Notu:** Refund mekanizmasi kritik UX -- kullanici siparisi expire oldugunda fonlari geri almali. Gas price cap: refund gas maliyeti refund miktarini asmamali.

---

### 2.36 `tests/e2e/replace_order.rs` (574 satir)

**Test Coverage:** 3 test fonksiyonu

**`single_replace_order_test`:**
1. Solver devre disi birakilir
2. Orijinal siparis olusturulur
3. Yeni siparis, `replacedOrder.uid = old_uid` ile olusturulur
4. Orijinal siparis `Status::Cancelled` olur
5. Yeni siparishm app_data'si korunur
6. Sadece yeni siparis auction'da -> settle edilir

**`try_replace_someone_else_order_test`:**
- Baskasinin siparisini degistirme girisiimi -> 401 UNAUTHORIZED

**`try_replace_unreplaceable_order_test`:**
- Auction bid edilmis siparis -> `OldOrderActivelyBidOn`
- Execute edilmis siparis -> `OrderFullyExecuted`

**Siparis Degistirme App Data Formati:**
```json
{
  "version": "1.1.0",
  "metadata": {
    "replacedOrder": { "uid": "0x..." },
    "customStuff": 20
  }
}
```

**DEX Uygulama Notu:** Siparis degistirme kullanici deneyimini iyilestirir. Guvenlik: yalnizca orijinal siparis sahibi degistirebilir.

---

### 2.37 `tests/e2e/smart_contract_orders.rs` (209 satir)

**Test Coverage:** 2 test fonksiyonu

**`smart_contract_orders`:**
- EIP-1271 invalid signature -> `InvalidEip1271Signature` x 2
- EIP-1271 valid signature -> siparis olusturulur, settle edilir
- PreSign -> `PresignaturePending` -> `setPreSignature(uid, true)` -> settle edilir

**`erc1271_gas_limit` -- GasHog Kontratt:**
```rust
// GasHog: isValidSignature cagrisinda N gas tuketir
let signature = U256::from(1_000_000).to_be_bytes::<32>();
// max_gas_per_order = 1_000_000
// 1M gas'lik imza -> TooMuchGas
```

**Guvenlik Invarianti:** EIP-1271 gas limiti zorunlu. Sonsuz gas tuketen kontrat imzalari reddedilmeli.

**DEX Uygulama Notu:** Nova Shield DEX EIP-1271 destegi zorunlu max gas ile sinirlandirilmali. `max_gas_per_order` production'da 300k gas makul.

---

### 2.38 `tests/e2e/solver_competition.rs` (649 satir)

**Test Coverage:** 3 test fonksiyonu

**`local_node_solver_competition`:**
- Iki solver yarısır
- 3 blok deadline dolmadan: `get_latest_solver_competition()` -> 404
- Deadline sonrasi: tam competition verisi, 2 cozum, kazanan `ranked=1`

**`local_node_wrong_solution_submission_address`:**
- solver1 yanlis submission address (WETH adresi) ile yapilandirilir
- solver1 cozumu reddedilir
- Yalnizca solver2 settle eder, `competition.solutions.len() == 1`

**`local_node_store_filtered_solutions`:**
```
good_solver: 3:1 fiyat (2 farkli siparis)
bad_solver: 2:1 fiyat (her iki siparis birlikte)

Kombinatorial auction sonucu:
  bad_solver.filtered_out = true
  bad_solver.reference_score = None
  good_solver.is_winner = true
  good_solver.reference_score = Some(0)
```

**DEX Uygulama Notu (3x Hassasiyet):**

Solver Competition -- Nova Shield DEX Fiyat Rekabeti Mekanizmasi:

1. **Kombinatorial auction:** Her siparis icin ayri ayri en iyi cozum degil, TUM siparisler icin global optimal cozum aranir.

2. **Reference score:** Filtrelenen solver'in "eger olsaydi ne kazan alirdi?" puani. Shapley value benzeri adil odullendirme sistemi.

3. **Submission address guvenlik:** Solver cozumunu dogru adresle imzalamali. Nova Shield icin: solver key rotation proseduru kritik.

4. **Deadline gating:** Competition verisi deadline dolmadan public olmamali -- rakip solverlar son saniyede cozum kopyalayabilir.

---

### 2.39 `tests/e2e/state_override.rs` (70 satir)

**Test:** `local_node_estimate_gas_state_override`

**Gate Kontratt:** `slot_0 == 0` ise revert. State override ile slot_0 = 1 -> basarili gas estimation.

**DEX Uygulama Notu:** State override gas estimation icin kritik. Nova Shield DEX siparis dogrulamada balance/allowance simulasyonu benzer sekilde yapilir.

---

### 2.40 `tests/e2e/submission.rs` (1029 satir)

**Test Coverage:** 5+ test fonksiyonu

**`local_node_on_expiry`:**
- Block gas limit = 100,000 -> settlement tx cok buyuk
- Timeout: cancellation tx gonderilir

**`local_node_execute_same_sell_and_buy_token`:**
- `sell_token == buy_token`, `AllowSell` policy
- Settlement contract'a tx gonderilir (cancellation degil)
- `final_balance < initial_balance` (fee alindi)

**`test_execute_same_sell_and_buy_native_token`:**
- WETH sat, BUY_ETH_ADDRESS al
- Quote: `buy_amount < sell_amount` (fee)
- Farkli receiver -> quote miktarlari degismez (~0.01% tolerans)
- Settlement tx -> settlement contract'a

**`test_execute_same_sell_and_buy_native_token_buy_order`:**
- Buy order: WETH sat, ETH al
- `SameTokensPolicy::Allow`
- `sell_amount = buy_amount + fee`

**DEX Uygulama Notu:** Cancellation tx mekanizmasi -- settlement basarisiz olursa nonce'i tuketmek icin bos tx. Nova Shield icin: belirli sayida basarisiz settlement -> jailing.

---

### 2.41 `tests/e2e/token_metadata.rs` (121 satir)

**Test Coverage:** 2 test fonksiyonu

**`token_metadata`:**
- Trade sonrasi: `first_trade_block > 0`, `native_price > 0`

**`token_metadata_no_trade`:**
- Hic trade edilmemis token: `first_trade_block = None`
- Native price mevcut olabilir (pool varligindan)

**DEX Uygulama Notu:** Token metadata indexing. `first_trade_block` guven sinyali olarak kullanilabilir.

---

### 2.42 `tests/e2e/tracking_insufficient_funds.rs` (182 satir)

**Test:** `local_node_test`

**Senaryo:** WETH cekilince siparis invalid:
1. Iki siparis olusturulur (WETH bazli)
2. Her iki trader WETH'ini ceker
3. Siparisler `Invalid` event alir (Created -> Invalid)
4. trader_a tekrar yatirir -> order_a `Ready` event alir
5. order_b hala invalid -> yeniden `Invalid` event almaz

**Kritik Invariant:**
```rust
fn check_non_consecutive_invalid_events(events: &[OrderEvent]) -> bool {
    !events.windows(2).any(|(a, b)| a == Invalid && b == Invalid)
}
// Ardisik Invalid event'ler yasak
```

**DEX Uygulama Notu:** Event deduplication kritik. Siparis durum izleme: balance degisiklikleri siparis durumunu etkiler.

---

### 2.43 `tests/e2e/trades_v2.rs` (165 satir)

**Test:** `local_node_trades_v2_endpoints`

**V2 Trades Endpoint Ozellikleri:**
- `GET /api/v2/trades?order_uid={uid}` -- UID'ye gore
- `GET /api/v2/trades?owner={addr}` -- sahibine gore
- `GET /api/v2/trades?owner={addr}&offset=0&limit=2` -- pagination

**Pagination Testleri:**
- `limit=2` -> 2 sonuc
- `offset=1, limit=100` -> 2 sonuc (3-1=2)

---

### 2.44 `tests/e2e/uncovered_order.rs` (208 satir)

**Test Coverage:** 2 test fonksiyonu

**`test` -- Sifir Bakiyeli Siparis:**
1. Sifir bakiye + sifir approval -> `InsufficientBalance`
2. Sifir bakiye + yeterli approval -> kabul edilir
3. WETH yatirilir -> trade gerceklesir

**`test_full_balance_check`:**
- `full_balance_check = false` + 1 ETH bakiye (2 ETH gerekiyor) -> kabul
- `full_balance_check = true` + 1 ETH bakiye -> `InsufficientBalance`
- Bakiye 2 ETH'e cikarilir + allowance 1 ETH -> `InsufficientAllowance`
- Allowance 2 ETH'e cikarilir -> kabul

**Guvenlik Invarianti:** `full_balance_check` siparis olusturmada daha siki validasyon.

**DEX Uygulama Notu:** Approval-only (zero balance) siparis destegi kullanici UX'ini iyilestirir.

---

### 2.45 `tests/e2e/univ2.rs` (169 satir)

**Test:** `local_node_test` (UniswapV2 temel test)

**Senaryo:** Solver yanlis `settle()` cagrisi yapar (trivial settlement, auction ID yok):
- Bu "fee withdrawal" gibi eksternal settlement'i simule eder
- Autopilot bunu gracefully handle etmeli

**Trivial Settlement:**
```rust
gp_settlement.settle([], [], [], [[interaction], [], []])
```

**Assert:**
- Token transfer gerceklesir (1 ETH)
- Order events: Created -> Ready -> Executing -> Traded
- DB verileri: prices, participants, scores

---

### 2.46 `tests/e2e/user_surplus.rs` (157 satir)

**Test:** `local_node_user_surplus_endpoint`

**Senaryo:**
1. Sifir trade -> `totalSurplus = 0`
2. 1 trade -> `totalSurplus > 0`
3. 3 trade -> `totalSurplus_3 > totalSurplus_1` (birikimli)

**Surplus Hesaplama:** Settlement price vs limit price farki.

---

### 2.47 `tests/e2e/wrapper.rs` (321 satir)

**Test:** `forked_node_mainnet_wrapper`

**EmptyWrapper:** `0x751871E9cA28B441Bb6d3b7C4255cf2B5873d56a` (mainnet)

**App Data Wrapper Formati:**
```json
{
  "version": "0.9.0",
  "metadata": {
    "wrappers": [
      { "address": "0x751...", "data": "0xbeef", "isOmittable": false },
      { "address": "0x751...", "data": "0xfeed", "isOmittable": false }
    ]
  }
}
```

**Wrapper Cagri Dogrulama:**
```rust
collect_wrapper_calls(&call_frame, EMPTY_WRAPPER_MAINNET, &mut wrapper_calls);
assert_eq!(wrapper_calls.len(), 2);
assert!(call_data_strings[0].contains("0002beef"));
assert!(call_data_strings[1].contains("0002feed"));
```

**Senaryo:**
1. WETH->USDC siparisi, app_data'da 2 wrapper tanimli
2. Tx `to = EMPTY_WRAPPER_MAINNET` (settlement contract degil!)
3. Wrapper her iki calldata ile cagrilir
4. Auction ID wrapperlar uzerinden propagate edilir

**DEX Uygulama Notu:** Wrapper kontratlar settlement'i ozellestirme mekanizmasi. Ozel clearing mekanizmalari (dark pool, batch auction) wrapper pattern ile implement edilebilir.

---

## 3. Emerged Invariantlar -- Sistematik Ozet

### 3.1 Guvenlik Invariantlari

| # | Invariant | Kaynak |
|---|-----------|--------|
| 1 | Hook allowance -> HooksTrampoline, settlement contract'a 0 allowance | hooks.rs |
| 2 | EIP-1271 gas limiti: `max_gas_per_order` asimi reddedilir | smart_contract_orders.rs |
| 3 | Kara listedeki kullanicilar 403 alir | banned_users.rs |
| 4 | App data `signer` siparis imzacisiyla esmesmeli | app_data_signer.rs |
| 5 | JIT siparis imzacisi solver tarafindan dogrulanmali | jit_orders.rs |
| 6 | Solver submission address kontrolu zorunlu | solver_competition.rs |
| 7 | Ardisik Invalid event'ler yasak | tracking_insufficient_funds.rs |
| 8 | Sifir bakiye + sifir approval -> `InsufficientBalance` | uncovered_order.rs |
| 9 | Partner fee global cap asilmaz | protocol_fee.rs |
| 10 | Ordersizce siparis kaldirim -> offset pagination ile tespiti | jit_orders.rs |

### 3.2 DEX Fiyatlama Invariantlari

| # | Invariant | Kaynak |
|---|-----------|--------|
| 1 | CoW AMM constant product: `x * y = k` | cow_amm.rs |
| 2 | EIP-4626 recursive unwrap: `price = vault.convertToAssets(1e18) * asset_price` | eip4626.rs |
| 3 | Decimal normalizasyon: `factor = 10^(18 - asset_decimals)` | eip4626.rs |
| 4 | Empty revert -> terminal token (unwrap durur) | eip4626.rs, contract_revert.rs |
| 5 | UniswapV2 AMM: `Dy = y*Dx*997 / (x*1000 + Dx*997)` | partially_fillable_pool.rs |
| 6 | Native ETH: WETH balance = ETH balance (quote icin) | quote_verification.rs |

### 3.3 Settlement Invariantlari

| # | Invariant | Kaynak |
|---|-----------|--------|
| 1 | Settlement contract balance = accumulated protocol fees | protocol_fee.rs |
| 2 | Buffer tukenir -> harici likiditesine gecis | buffers.rs |
| 3 | Cancellation tx: settlement basarisizsa nonce tuketimi | submission.rs |
| 4 | Trivial settlement (audit): gracefully handled | univ2.rs |
| 5 | Parallel settlement: iki es zamanli tx farkli hesaptan | parallel_settlement.rs |
| 6 | Same-token wrap/unwrap: volume fee exempt | protocol_fee.rs |

---

## 4. DEX Uygulama Notu -- Nova Shield DEX Eylem Plani

### 4.1 Mimari Kararlar (CoW Protokolunden Ogrenilen)

**Siparis Yonetimi:**
```
Nova Shield Siparis Akisi:
  User -> Orderbook API -> Validation -> Database
  Autopilot -> Auction Creation -> Solver Dispatch
  Solver -> Solution -> Driver -> Simulation -> Settlement
```

**Kritik Mimari Noktalar:**
1. EIP-712 domain separator her zincire ozgu, deployment adresine bagli
2. Settlement contract merkezi, tum tokenlar buradan gecer
3. Solver whitelist (GPv2AllowListAuthentication) MEV korumasinin temeli
4. HooksTrampoline guvenli izolasyon katmani

### 4.2 Guvenlik Oncelikleri

| Oncelik | Ozellik | Referans Test |
|---------|---------|---------------|
| P0 | Hook sandbox (HooksTrampoline) | hooks.rs |
| P0 | EIP-1271 max gas limiti | smart_contract_orders.rs |
| P0 | Partner fee global cap | protocol_fee.rs |
| P1 | Solver whitelist | banned_users.rs |
| P1 | Submission address dogrulama | solver_competition.rs |
| P1 | Full balance check (opt-in) | uncovered_order.rs |
| P2 | State override gas estimation | state_override.rs |
| P2 | Stale liquidity cache eviction | quoting.rs |

### 4.3 DEX Ozgun Ozellikler

**CoW AMM Butunlestirmesi:**
```
Nova Shield DEX <- CoW AMM
  - JIT siparis uretimi (driver)
  - EIP-1271 imzasi (kontrat AMM)
  - Pre-interaction commit
  - Constant product formula
  - Dengesizlik esigi: oracle_price vs pool_price farki
```

**Batch Auction Optimizasyonu:**
```
Her blokta:
  1. Pending siparisleri topla
  2. Solver'lara gonder (Brotli sikistirmali)
  3. En iyi cozumu sec (kombinatorial)
  4. Settlement simule et
  5. Submit (EIP-7702 ile paralel)
```

**Fee Mimarisi:**
```
Surplus Fee: (actual_buy - quote_buy) * factor
Volume Fee:  sell_amount * factor
Partner Fee: buy_amount * bps / 10000

Max total fee: ~0.5-1% (governance tarafindan belirlenir)
Same-token exempt: WETH<->ETH
```

### 4.4 Test Stratejisi

Nova Shield DEX icin e2e test oncelikleri:

1. Temel akis: siparis olusturma -> settlement -> bakiye kontrolu
2. Kismi doldurma: balance limit + pool limit senaryolari
3. Hook guvenligi: trampoline izolasyon testi
4. Fee dogrulama: settlement contract bakiyesi = fee
5. EIP-1271: Safe + GasHog guvenlik testleri
6. Lider secimi: HA failover testi
7. Parallel settlement: EIP-7702 mempool testi
8. CoW AMM: JIT siparis + constant product dogrulama
9. EIP-4626: Recursive unwrap + decimal mismatch
10. API guvenligi: malformed request sinir testleri

---

## 5. Analiz Ozeti

**Toplam Incelenen Dosya:** 66
**Toplam Satir Tahmini:** ~12,000 (src/) + ~25,000 (tests/) = ~37,000 satir
**Test Fonksiyon Sayisi:** 55+ unique test
**Guvenlik Testi Sayisi:** 15+
**DEX Ozgun Test Sayisi:** 25+

**En Kritik Dosyalar (Nova Shield icin):**
1. `tests/e2e/cow_amm.rs` -- AMM mekanizmasi
2. `tests/e2e/protocol_fee.rs` -- Ucret sistemi
3. `tests/e2e/hooks.rs` -- Guvenlik sandbox
4. `tests/e2e/parallel_settlement.rs` -- MEV korumasi
5. `tests/e2e/solver_competition.rs` -- Rekabet mekanizmasi
6. `src/setup/deploy.rs` -- Kontrat deployment sirasi
7. `src/setup/onchain_components/safe.rs` -- EIP-1271 implementasyonu

**DEX Uygulama Onceligi:**
Nova Shield DEX icin CoW protokolu e2e test altyapisinin %80'i dogrudan adapt edilebilir. Ozellikle `OnchainComponents`, `Services`, `Mock`, `colocation` modulleri test altyapisinin omurgasidir.
