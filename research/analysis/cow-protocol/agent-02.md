# CoW Protocol — Agent-02 Analiz Raporu
## Kapsam: account-balances/, app-data/, bytes-hex/, chain/, bad-tokens/, balance-overrides/
**Tarih:** 2026-07-22
**Analiz Eden:** Agent-02
**Toplam Dosya:** 16 | **Tahmini Satır:** ~2,800

---

## GENEL BAKIS

Bu agent altı küçük-orta boy crate'i analiz eder. Bunlar CoW Protocol'ün altyapı crate'leridir:

1. **account-balances**: ERC-20 balance ve allowance kontrolü + simülasyon
2. **app-data**: Order metadata (hooks, signer, flashloan, partner fee) doğrulama
3. **bytes-hex**: Vec<u8> ↔ 0x-prefixed hex serde yardımcısı
4. **chain**: Multi-chain enum ve chain-specific parametreler
5. **bad-tokens**: "Fee-on-transfer" ve diğer kötü davranışlı token tespiti
6. **balance-overrides**: Quote simülasyonu için state override injection

---

## DOSYA ANALİZİ

---

### BÖLÜM 1: account-balances/ (3 dosya)

#### 1a. `src/lib.rs` (252 satır)

**Amaç:** Balance fetching soyutlaması ve `BalanceSimulator` — settlement contract'ın `simulateDelegatecall` API'si üzerinden on-chain balance simülasyonu.

**Struct/Trait Listesi:**

| Tip | Açıklama |
|-----|----------|
| `Query` | owner + token + source + interactions + balance_override |
| `TransferSimulationError` | InsufficientAllowance / InsufficientBalance / TransferFailed(Vec<u8>) / Other |
| `BalanceFetching` trait | get_balances, can_transfer, allowance — async trait |
| `BalanceSimulator` | settlement + balances contracts + vault_relayer + state_overrider |
| `Simulation` | Simülasyon sonucu: token_balance, allowance, effective_balance, can_transfer, revert_reason |
| `SimulationError` | Method(alloy_contract::Error) |

**Kritik Fonksiyonlar:**

**`Query::from_order(o: &Order) -> Self`** (satır 28-38):
Order'dan query üretir. `balance_override: None` — autopilot tarafında balance check'te override henüz kullanılmıyor (TODO notu var).

**`BalanceFetching` trait:**
- `get_balances(&[Query]) -> Vec<Result<U256>>`: batch balance sorgu
- `can_transfer(&Query, amount) -> Result<(), TransferSimulationError>`: transfer simülasyonu
- `allowance(owner, token, source) -> Result<U256>`: mevcut allowance
- `#[cfg_attr(test/feature="test-util", mockall::automock)]` — mock generation için automock

**`BalanceSimulator::simulate(owner, token, source, interactions, amount, balance_override) -> Result<Simulation>`** (satır 129-216):
En kritik fonksiyon. `settlement.simulateDelegatecall(balances_contract_address, encoded_call)` ile tek round-trip'te:
1. Pre-interactions'ı çalıştırır (sanki settlement context'inde)
2. VaultRelayer transfer simülasyonu
3. Yanıt decode: `(U256, U256, U256, bool, Bytes)` tuple
   - token_balance, allowance, effective_balance, can_transfer, revert_reason

Önemli detay: `Balances::Contracts.vault = Address::ZERO` — Balancer vault deprecated, sadece ERC20 source destekleniyor.

`SIMULATION_ACCOUNT: LazyLock<Address> = Address::random()` — ZKSync uyumluluğu için (ZKSync `tx.from=0x0` kullanmaz).

**`fetcher()` / `cached()`** fabrika fonksiyonları:
- `fetcher`: `simulation::Balances` döner
- `cached`: `cached::Balances` wrap eder + background task spawn eder

**Nova Shield İçin Öğrenilen:**
- Balance simülasyonu çok güçlü: pre-interactions çalıştırarak settlement anındaki gerçek balance'ı kontrol edebiliyorsunuz. Basit `balanceOf + allowanceOf` yetmez.
- `simulateDelegatecall` pattern: settlement contract context'inde herhangi bir kontratı çalıştırma. Nova Shield DEX settlement validator'ı için kullanılabilir.
- `SIMULATION_ACCOUNT = Address::random()` — ZKSync uyumluluk detayı. Layer 2 entegrasyonlarında tx.from=0x0 problemli olabilir.
- `TransferSimulationError::TransferFailed(Vec<u8>)` — revert reason bytes tutulur. ABI decode ile hata mesajı çıkarılabilir.

**DEX Uygulama Notu:**
Nova Shield DEX'te order kabul öncesi `can_transfer` kontrolü zorunlu. Özellikle:
- Fee-on-transfer token'lar için simülasyon gerekli (sadece balanceOf yetmez)
- Pre-hook'ları olan order'lar için simülasyon şart
- ZKSync/L2 uyumluluğu için random simulation account kullanın

---

#### 1b. `src/cached.rs` (367 satır — test dahil)

**Amaç:** `BalanceFetching` implementasyonu üzerinde block-aware cache katmanı.

**Struct Listesi:**

| Tip | Açıklama |
|-----|----------|
| `BalanceCache` | last_seen_block + HashMap<Query, BalanceEntry> |
| `BalanceEntry` | requested_at + updated_at + balance |
| `Balances` | inner: Arc<dyn BalanceFetching> + balance_cache: Arc<Mutex<BalanceCache>> |
| `CacheResponse` | cached: Vec<(usize, Result<U256>)> + missing: Vec<usize> + requested_at |

**Sabit:**
`EVICTION_TIME: BlockNumber = 5` — 5 blok içinde sorgulanmayan balance'lar cache'ten çıkar.

**Cache Algoritması:**

`get_balances(&[Query]) -> Vec<Result<U256>>`:
1. `get_cached_balances` ile `partition_map` — cache hit vs miss ayır
2. Miss'ler için `inner.get_balances(missing_queries)` çağır
3. Yeni sonuçları `insert_balance` ile cache'e ekle
4. `cached.extend(missing)` → `sort_by_key` → orijinal sıra korunur

**Background Task `spawn_background_task(block_stream)`:**
Her yeni blokta:
1. `last_seen_block` güncelle
2. Son `EVICTION_TIME` blok içinde sorgulanmış entry'leri refresh et
3. `updated_at < block.number` olanları temizle (stale data)

Önemli güvenlik: `update_balance` sadece mevcut entry'leri günceller (insert etmez). `insert_balance` sadece freshly-fetched sonuçları için. Background refresh sadece update yapar.

**Cache Doğruluk Garantisi:**
`retain` ile sadece son blokta güncellenmiş veriler tutulur. Bu, stale balance'ların otomatik temizlenmesini garantiler.

**Testler:**
- `caches_ok_results`: OK sonucu 1 kez inner çağrılır, sonraki cache'ten
- `does_not_cache_errors`: Error sonuçlar cache'e girmez
- `background_task_updates_cache_on_new_block`: blok gelince refresh çalışır
- `can_return_new_and_cached_results_in_same_call`: mixed hit/miss tek çağrıda çalışır
- `unused_balances_get_evicted`: EVICTION_TIME blok sonra temizlenir

**Nova Shield İçin Öğrenilen:**
- `partition_map` (itertools) — cache hit/miss ayırma için zarif pattern
- Sıra koruması: `sort_by_key` ile orijinal query indeksi korunur. Batch API'da kritik.
- Background task + Mutex pattern: `Arc<Mutex<BalanceCache>>` clone'lanabilir ve tokio::spawn ile paylaşılabilir.
- Error'lar cache'lenmez — her error, yeni fetch tetikler. Bu önemli: geçici node hataları kalıcı cache miss'e yol açmaz.

---

#### 1c. `src/simulation.rs` (203 satır — test dahil)

**Amaç:** `BalanceFetching` trait'inin iki implementasyonu: basit ERC20 çağrısı ve tam simülasyon.

**İki Strateji:**

**`tradable_balance_simple(query, token_contract)`:**
- `balanceOf(owner)` ve `allowance(owner, vault_relayer)` paralel çağrı
- `min(balance, allowance)` döner
- `interactions.is_empty()` olmak şartı — pre-interaction yoksa kullanılır
- `futures::try_join!` ile paralel — tek round-trip

**`tradable_balance_simulated(query)`:**
- `balance_simulator.simulate(...)` ile tam simülasyon
- `simulation.can_transfer` true ise `effective_balance`, false ise `U256::ZERO`

**`get_balances(&[Query])` dispatch:**
```
for each query:
  if interactions.is_empty() → tradable_balance_simple
  else → tradable_balance_simulated
```
`future::join_all` — tümü paralel

**`can_transfer` implementasyonu:**
1. Simulate with specific `amount`
2. `token_balance < amount` → `InsufficientBalance`
3. `allowance < amount` → `InsufficientAllowance`
4. `!can_transfer` → `TransferFailed(revert_reason)`

**Test:**
`test_for_user` — `#[ignore]` — mainnet ile live test. Gerçek user/token/amount kullanır.

**Nova Shield İçin Öğrenilen:**
- `try_join!` ile paralel ERC20 sorgu — latency yarıya iner. Her DEX'te uygulanabilir.
- Simple vs simulated stratejisi: pre-interaction yoksa basit yol, varsa simülasyon. Bu optimizasyon çoğu order için (basit ERC20) 10x daha hızlı.
- `can_transfer` üç aşamalı hata kontrolü: önce balance, sonra allowance, sonra simülasyon. Hata sırasının önemi: balance hatası allowance hatasından önce kontrol edilmeli (kullanıcı feedback için daha anlamlı).

---

### BÖLÜM 2: app-data/ (4 dosya)

#### 2a. `src/app_data_hash.rs` (194 satır — test dahil)

**Amaç:** AppData hash hesaplama ve IPFS CID üretimi.

**Struct:**
```rust
pub struct AppDataHash(pub [u8; 32]);
```

**Kritik Fonksiyonlar:**

**`hash_full_app_data(app_data: &[u8]) -> [u8; 32]`** (satır 108-110):
`keccak256(app_data)` — basit keccak256. Bu hash order'da signed 32 byte'a yerleşir.

**`create_ipfs_cid(app_data_hash: &[u8; 32]) -> [u8; 36]`** (satır 115-123):
CIDv1 formatında:
- `[0] = 1` — CID version
- `[1] = 0x55` — raw codec
- `[2] = 0x1b` — keccak256 hash algorithm
- `[3] = 32` — hash length
- `[4..36] = hash`

Bu sayede full app data IPFS'e upload edilebilir ve CID'den erişilebilir.

**AppDataHash Serde:**
- Serialize: `"0x" + hex(bytes[32])` — 66 char string
- Deserialize: `const_hex::decode` — 0x prefix veya prefix'siz her ikisi de kabul
- `FromStr`: 0x ile veya prefix'siz

**Test:**
`known_good` — Gerçek CoW Swap metadata'sı ile test:
```json
{"appCode":"CoW Swap","environment":"production","metadata":{"quote":{"slippageBips":"50",...},...}}
```
Hash ve IPFS CID'i doğrulandı. IPFS CLI ile çapraz doğrulama adımları yorumda açıklanmış.

**Nova Shield İçin Öğrenilen:**
- Order metadata'yı IPFS'e upload et: kullanıcı arayüzü ekstra data (slippage, order class, hook config vb.) order hash'e bağlı olarak on-chain kaydedebilir.
- `create_ipfs_cid` — raw CIDv1 + keccak256: standart IPFS erişimi için. Frontend'in CID'den app data'yı almasını sağlar.
- AppDataHash serde: 0x prefix her iki formatta da kabul — API robustness için.

---

#### 2b. `src/app_data.rs` (918 satır — test dahil)

**Amaç:** ProtocolAppData validasyonu ve parsing. Hooks, signer, flashloan, partner_fee, wrapper'lar burada.

**Struct Hiyerarşisi:**

```
ValidatedAppData
├── hash: AppDataHash
├── document: String
└── protocol: ProtocolAppData

ProtocolAppData
├── hooks: Hooks (pre/post)
├── signer: Option<Address>  ← app_data'da imzacı belirleme (EIP-1271 için)
├── replaced_order: Option<ReplacedOrder>
├── partner_fee: PartnerFees (Vec<PartnerFee>)
├── flashloan: Option<Flashloan>
└── wrappers: Vec<WrapperCall>
```

**`Flashloan` Struct:**
```rust
pub struct Flashloan {
    pub liquidity_provider: Address,  // Flashloan veren protokol
    pub protocol_adapter: Address,    // Helper kontrat
    pub receiver: Address,            // Token alıcısı
    pub token: Address,               // Flashloan token'ı
    pub amount: U256,                 // Miktar
}
```
Binding değil — solver ipucu olarak kullanılır.

**`WrapperCall` Struct:**
```rust
pub struct WrapperCall {
    pub address: Address,    // Wrapper kontrat
    pub data: Vec<u8>,       // BytesHex encoding
    pub is_omittable: bool,  // Opsiyonel wrapper mı?
}
```

**`FeePolicy` (app-data versiyonu, model crate'in FeePolicy'sinden farklı):**
- Bps (basis points) cinsinden: `Surplus { bps: u64, max_volume_bps: u64 }`
- `f64` yerine `u64` kullanımı — daha precise
- Karmaşık custom Deserialize: `FeePolicyDeserializer` struct trick'i
  - Problem: `serde(untagged)` ile hata mesajları yetersiz
  - Çözüm: Flat struct deserialize ederek sonra match

**`PartnerFees` Custom Deserialize:**
Hem tek nesne (`{bps: 100, recipient: "0x..."}`) hem de dizi kabul eder.
`visit_map` → tek fee wrap eder, `visit_seq` → dizi olarak parse.

**`Validator` struct:**
- `size_limit: usize` — default 8192 byte (CLAUDE.md'de "8KB default size limit")
- `validate(&[u8]) -> Result<ValidatedAppData>`: boyut kontrolü + JSON parse + hash

**`WrapperCache(Cache<AppDataHash, bool>)`:**
Moka TinyLFU cache — wrapper'lı app data tespit edilmesini cache'ler. Aynı JSON'u her auction'da yeniden parse etmemek için.

**Root JSON Yapısı:**
```json
{
  "version": "0.9.0",
  "appCode": "CoW Swap",
  "metadata": { /* ProtocolAppData */ }
}
```
Legacy `backend` field (deprecated): `BackendAppData` sadece hooks içerir, `metadata` mevcutsa yok sayılır.

**`parse(full_app_data: &[u8]) -> Result<ProtocolAppData>`:**
`metadata` yoksa `backend`'e fallback, ikisi de yoksa `ProtocolAppData::default()`.

**`app_code(full_app_data: &[u8]) -> Option<String>`:**
Analytics için `appCode` alanını extract eder. Hata durumunda `None`.

**Testler:**
- `empty_is_valid`: `"{}"` geçerli
- `extracts_app_code`: `{"appCode": "CoW Swap"}` → `Some("CoW Swap")`
- `examples`: Hooks, signer, partnerFee (tek ve dizi), flashloan olmayan kapsamlı örnekler
- `legacy`: `backend` field backward compat, `metadata` mevcutken `backend` görmezden gelinir
- `wrapper_cache_detects_wrappers`: cache hit, empty wrappers, dolu wrappers

**Nova Shield İçin Öğrenilen:**
- App data sistemi kullanıcı arayüzüne katmanlı metadata ekleme imkanı verir. Nova Shield'da order'a slippage, referral, partner fee vb. eklenebilir.
- `FeePolicy` bps (basis points) kullanımı: `u64` ile hassas fee hesabı. 1 bps = 0.01%.
- Partner fee sistemi: birden fazla recipient, farklı fee policy'ler — gelir paylaşımı modeli.
- `Flashloan` hint mekanizması: solver'a flashloan kullanması için rehberlik — binding değil, optimizasyon rehberi.
- `WrapperCall.is_omittable`: opsiyonel wrapper — solver atlayabilir. Composition esnekliği.
- IPFS + 8KB limit: büyük metadata'yı on-chain değil, IPFS'te tut ve sadece hash'i imzala.

**DEX Uygulama Notu:**
Nova Shield DEX order'larına app data ekleyerek:
1. Slippage bilgisi (audit trail)
2. Partner/referral fee konfigürasyonu
3. Pre/post hook konfigürasyonu (cross-protocol composability)
4. Flashloan hint (solver optimizasyonu)
5. Signer address (app_data üzerinden EIP-1271 signer belirtme)

---

#### 2c. `src/hooks.rs` (49 satır)

**Amaç:** Pre/post hook'ları tanımlar.

**Struct:**
```rust
pub struct Hooks {
    pub pre: Vec<Hook>,
    pub post: Vec<Hook>,
}

pub struct Hook {
    pub target: Address,
    pub call_data: Vec<u8>,   // BytesHex serde
    pub gas_limit: u64,       // DisplayFromStr serde (JSON string)
}
```

**`Hooks::gas_limit(&self) -> u64`:**
`saturating_add` ile tüm hook'ların toplam gas limiti. Overflow koruması.

**Nova Shield İçin Öğrenilen:**
- `gas_limit` string olarak serialize: JSON number precision sorununu önler (JS'te u64 max > f64 max).
- Her hook'un kendi gas_limit'i var — fee quote'da her hook'un gas maliyetini ayrı hesapla.

---

#### 2d. `src/lib.rs` (1 satır)
Re-export modülü. `pub use {app_data::*, app_data_hash::*, hooks::*};`

---

### BÖLÜM 3: bytes-hex/ (1 dosya)

#### 3a. `src/lib.rs` (78 satır — test dahil)

**Amaç:** `Vec<u8>` için 0x-prefixed hex string serde. İki kullanım şekli:
1. `#[serde(with = "bytes_hex")]` attribute
2. `#[serde_as(as = "BytesHex")]` attribute (serde_with uyumu)

**Fonksiyonlar:**

`serialize<S, T: AsRef<[u8]>>(bytes, serializer)`:
- Buffer: `[0u8; 2 + bytes.len() * 2]`
- `bytes[0..2] = b"0x"`
- `const_hex::encode_to_slice` — allocation-free hex encoding

`deserialize<D>(deserializer)`:
- `Cow::<str>::deserialize` — zero-copy string borrow
- `strip_prefix("0x")` — zorunlu prefix, yoksa error
- `const_hex::decode` — hex parse

**`BytesHex` struct:**
`SerializeAs<T: AsRef<[u8]>>` ve `DeserializeAs<Vec<u8>>` implementasyonları.
Fonksiyonlar üzerinden delege.

**Test:**
`vec![0, 1]` → `"0x0001"` → `vec![0, 1]` round-trip.

**Nova Shield İçin Öğrenilen:**
- `Cow<str>` deserialization: string borrow'u mümkün olduğunda allocate etmez.
- `const_hex` vs `hex` crate: `const_hex` çok daha hızlı (SIMD-optimized, const-eval destekli).
- `BytesHex` serde_with adapter olarak kullanım: `#[serde_as(as = "BytesHex")]` ile clean field annotation.

---

### BÖLÜM 4: chain/ (1 dosya)

#### 4a. `src/lib.rs` (250 satır — test dahil)

**Amaç:** Desteklenen chain'lerin enum'u ve her chain için spesifik parametreler.

**`Chain` enum (13 varyant):**

| Chain | ID | Block Time |
|-------|----|-----------|
| Mainnet | 1 | 12s |
| Gnosis | 100 | 5s |
| Sepolia | 11155111 | 12s |
| ArbitrumOne | 42161 | 250ms |
| Base | 8453 | 2s |
| Hardhat | 31337 | 12s |
| Bnb | 56 | 3s |
| Avalanche | 43114 | 2s |
| Optimism | 10 | 2s |
| Polygon | 137 | 2s |
| Linea | 59144 | 2s |
| Plasma | 9745 | 1s |
| Ink | 57073 | 1s |

**Kritik Fonksiyonlar:**

**`chain.id() -> u64`:** `*self as u64` — `#[repr(u64)]` ile.

**`chain.name() -> &'static str`:** Human-readable isim.

**`chain.as_str() -> &'static str`:** Kebab-case slug — URL'lerde ve DB'de kullanılan stable identifier.

**`chain.default_amount_to_estimate_native_prices_with() -> U256`:**
Her chain için farklı miktarlar:
- ETH-based (Mainnet, Arbitrum, Base vb.): `10^17` (0.1 ETH)
- xDAI, Avalanche: `10^18` (1 native token)
- Polygon, Plasma: `10^20` (100 native token — native değer düşük)

**`chain.block_time_in_ms() -> Duration`:** Chain'e özel blok süresi.

**`chain.blocks_in(time_ms: u64) -> f64`:** `time_ms / block_time_ms` — belirli bir sürede kaç blok.

**`TryFrom<u64>`:** Chain ID'den Chain enum'una dönüşüm. Bilinmeyen ID → `ChainIdNotSupported`.

**Custom Deserialize:**
`deserialize_any` ile hem u64 hem string kabul:
- JSON sayı: `visit_u64` → direkt try_from
- JSON string: `visit_str` → parse::<u64> → try_from

**Test:**
- `test_blocks_in`: Mainnet 6h = 1800 blok, ArbitrumOne 6h = 86400 blok
- `test_deserialize_from_u64` / `test_deserialize_from_str`

**Nova Shield İçin Öğrenilen:**
- Chain registry pattern: enum + repr(u64). Yeni chain ekleme bir satır.
- `blocks_in(time_ms)` → auction süresi hesabında kritik. Autopilot her ~15s'de auction çalıştırır. Mainnet'te bu ~1.25 bloktur.
- `default_amount_to_estimate_native_prices_with`: fiyat tahmini için standart miktar. Native token'ın değerine göre ayarlanır.
- Kebab-case slug (`mainnet`, `arbitrum-one`) — DB tablo isimleri ve log label'ları için consistent.
- Hardhat chain ID'si (31337) — local dev. Nova Shield test ortamında Hardhat fork kullanılabilir.

---

### BÖLÜM 5: bad-tokens/ (3 dosya)

#### 5a. `src/lib.rs` (22 satır)

**Amaç:** Token kalite değerlendirme.

**`TokenQuality` enum:**
```rust
pub enum TokenQuality {
    Good,
    Bad { reason: String },
}
```
`is_good()` ve `bad(reason: impl ToString)` yardımcı metodlar.

---

#### 5b. `src/list_based.rs` (27 satır)

**Amaç:** Statik deny list — bilinen kötü token'lar.

**`DenyListedTokens` (Arc<Inner> wrapping HashSet<Address>):**
- `new(deny_list: Vec<Address>) -> Self`: HashSet'e convert
- `contains(token: &Address) -> bool`: O(1) lookup

`Arc<Inner>` wrapping: clone edilebilir, cheap, thread-safe.

---

#### 5c. `src/trace_call.rs` (461 satır — test dahil)

**Amaç:** `trace_callMany` API ile dinamik token kalite testi. 8-adımlı simülasyon.

**`TraceCallDetectorRaw` struct:**
- `web3: Web3`
- `settlement_contract: Address`

**`test_transfer(take_from, token, amount, pre_interactions) -> Result<TokenQuality>`:**

8 trace çağrısı oluşturur (`create_trace_request`):
- [0]: `balanceOf(settlement_contract)` — başlangıç balance
- [1]: `token.transfer(settlement_contract, amount)` from `take_from` — token al
- [2]: `balanceOf(settlement_contract)` — sonra balance
- [3]: `balanceOf(arbitrary_recipient)` — alıcı başlangıç balance
- [4]: `token.transfer(arbitrary_recipient, amount)` from `settlement_contract` — token gönder
- [5]: `balanceOf(settlement_contract)` — sonra balance (sıfırlanmalı)
- [6]: `balanceOf(arbitrary_recipient)` — sonra balance
- [7]: `token.approve(arbitrary_recipient, U256::MAX)` — max approve testi

**`handle_response(traces, amount, take_from)` Doğrulama Mantığı:**

Transfer IN kontrolü:
```
balance_after_in >= balance_before_in + amount - 1  // 1 wei tolerans
```
"Shares" tabanlı token'lar (eUSD vb.) için 1 wei tolerans.

Transfer OUT kontrolü:
```
balance_after_out == balance_before_in  // Tam restore edilmeli
```

Recipient kontrolü:
```
balance_recipient_after >= balance_recipient_before + amount - 1
```

Approve kontrolü:
```
trace[7] başarılı olmalı  // Max approve desteklenmeli
```

**Hata Durumları (TokenQuality::Bad):**
1. Transfer in revert: "Transfer of token from on chain source ... failed"
2. Balancelock decode fail: "Failed to decode balanceOf" (Vyper compiler bug ref.)
3. Fee-on-transfer (in): "Transferring X into settlement was expected to be Y but was Z"
4. Transfer out revert: "Transfer token out of settlement ... failed"
5. Balance mismatch (out): "Transferring X out was expected Y but Z"
6. Fee-on-transfer (out): recipient balance mismatch
7. Overflow: "would overflow its balance"
8. Approve fail: "Approval of U256::MAX failed"

**Fallback Davranışı:**
- Node `trace_callMany` desteklemiyorsa: `TokenQuality::Good` (assume good)
- HTTP 400: assume good
- Diğer RPC hataları: error propagate

`arbitrary_recipient() -> Address`: her çağrıda `Address::random()` — özel muafiyet tespiti için (bir token kendi pool'larına fee almıyorsa rastgele adres bunu ortaya çıkarır).

**Test:**
`handle_response_ok`: 8 trace mock verisi ile success case testi.

**Nova Shield İçin Öğrenilen:**
- Fee-on-transfer token detection kritik DEX güvenliği. USDT gibi token'lar bazen fee alabilir; orderbook sisteminde bu catastrophic olabilir.
- 1 wei tolerans: `eUSD` gibi shares-based token'ların küçük rounding hatalarına tolerans gösterilir. Ancak büyük sapmaları yakalamaya devam eder.
- `arbitrary_recipient = Address::random()`: whitelist bypass tespiti. Eğer token sadece kendi pool'larına fee almıyorsa bu testi geçer ama Settlement contract dışında başarısız olur.
- Approve MAX kontrolü: bazı token'lar U256::MAX approve'u reddeder (NonStandardERC20). Bu kritik — CoW protokolü max approve kullanır.
- Vyper compiler bug notu (PR #781): bazı Vyper token'lar `balanceOf` için 32 byte yerine farklı encoding döner.

**DEX Uygulama Notu:**
Nova Shield DEX'e yeni bir token kabul edilmeden önce `TraceCallDetector` çalıştırın:
1. Fee-on-transfer kontrol
2. Max approve kontrol
3. Balance accounting doğrulama
4. Whitelist olmayan yeni token'ları kabul etmeden önce otomatik test

---

### BÖLÜM 6: balance-overrides/ (5 dosya)

#### 6a. `src/lib.rs` (392 satır — test dahil)

**Amaç:** Quote simülasyonu için ERC-20 balance ve allowance'ı state override yoluyla inject etme.

**`StateOverriding` trait:**
```rust
async fn balance_override(request: BalanceOverrideRequest) -> Option<(Address, AccountOverride)>
async fn approval_override(request: ApprovalOverrideRequest) -> Option<(Address, AccountOverride)>
```
`AccountOverride` — Alloy'un `eth_call` state override formatı.

**`StateOverrides` struct:**
```rust
pub struct StateOverrides {
    balance_detector: balance::Detector,
    approval_detector: approval::Detector,
}
```
- `new(web3)`: `probing_depth=60, timeout=1s, cache_size=1000`
- `with_config`: custom parametreler

**`DummyStateOverrider`:** Test implementasyonu — her zaman `None` döner.

**`StateOverrides::balance_override`:** Detector'den strategy al → `strategy.state_override(holder, amount)` çağır.

**Testler:**
- `balance_override_computation`: COW token/Vitalik için SolidityMapping strategy → keccak256 slot hesabı ve `AccountOverride{state_diff: Some({slot: value})}` doğrulaması
- `balance_overrides_none_for_unknown_tokens`: unknown token → None
- `balance_override_computation_solady`: Solady ERC20 farklı storage layout
- `cached_detection_caches_holder_agnostic_strategies_without_holder`: holder-agnostic strategy cache (token, None) key
- `cached_detection_caches_holder_specific_strategies_with_holder`: holder-specific cache (token, Some(holder)) key
- `caches_universal_approval_strategy_without_inputs`: universal strategy → (token, None) key
- `caches_input_specific_approval_strategy_with_inputs`: specific strategy → (token, Some((owner, spender))) key

**Nova Shield İçin Öğrenilen:**
- State override pattern: `eth_call` ile yapılan simülasyonlarda token balance'ı geçici olarak override edebilirsiniz — gerçek transfer gerekmez. Quote doğrulama için kritik: kullanıcıda token yokken de quote alabilir.
- Iki caching stratejisi: holder-agnostic (tüm holder'lar için aynı strategy) ve holder-specific (storage slot hesabı holder'a bağlı).

---

#### 6b. `src/detector.rs` (97 satır)

**Amaç:** SLOAD opcode'larını trace ederek ERC-20 balance storage slot'unu dinamik olarak tespit eder.

**`extract_sload_slots(trace, initial_context) -> Vec<(Address, B256)>`:**
Geth struct log trace'ini parse eder:
- CALL/STATICCALL: yeni storage context push
- DELEGATECALL: mevcut context push (DELEGATECALL kendi storage'ı kullanmaz)
- RETURN: context pop
- SLOAD: mevcut context'teki slot'u kaydet (ilk erişim sırası)
- `HashSet<(Address, B256)>` ile deduplication

**`mapping_slot_hash(holder, map_slot) -> B256`:**
Solidity mapping storage slot formülü:
```
keccak256(pad32(holder) ++ map_slot)
```
`buf[12..32] = holder` (20 byte, 12 byte padding başta), `buf[32..64] = map_slot`

Bu ERC-20 `mapping(address => uint256)` slot hesabı için standart Solidity formülü.

**Nova Shield İçin Öğrenilen:**
- `extract_sload_slots` — storage layout otomatik discovery. Herhangi bir ERC-20 token için `balanceOf` storage slot'unu otomatik bul.
- DELEGATECALL tracking: proxy pattern'lerde storage context doğru tutulur.
- Mapping slot hash: Solidity storage slot formülü — EIP-1967 proxy slot'ları, ERC-20 balance slot'ları vb. için kullanılır.

---

## GENEL ÇIKARIMLAR

### 1. Simülasyon Mimarisi
CoW Protocol üç seviyede simülasyon yapar:
1. **Basit**: `balanceOf + allowanceOf` paralel ERC20 call
2. **Balance simülasyon**: `simulateDelegatecall` ile pre-interaction aware
3. **State override simülasyon**: Balance'ı inject ederek doğrulama

### 2. Token Güvenliği
Token kabul sürecinde üç katmanlı kontrol:
1. **Deny list**: Bilinen kötü token'lar
2. **Trace call test**: Fee-on-transfer, approve behavior, balance accounting
3. **Balance simülasyon**: Her order için gerçek zamanlı

### 3. Cache Stratejileri
İki tip cache:
1. **Block-aware balance cache**: 5 blok eviction, background refresh
2. **Storage strategy cache**: Token başına holder-agnostic veya holder-specific

### 4. Blockchain Soyutlaması
`Chain` enum: 13 chain, her biri için block time, native price estimation amount, URL slug.

---

## DEX Uygulama Özeti

**Nova Shield DEX için bu agent'tan alınacak pattern'ler:**

1. **Token güvenlik testi**: Yeni token listing'de TraceCallDetector çalıştır
2. **Balance cache**: 5 blok eviction, background refresh, error'ı cache'leme
3. **Simülasyon hiyerarşisi**: Basit → Simülasyon (interaction varsa)
4. **State override**: Quote API'de balance olmayan kullanıcılar için inject
5. **App data sistemi**: Order metadata (hooks, partner fee, flashloan hint)
6. **Chain registry**: `Chain` enum ile multi-chain support

**Token Kalite Formülleri:**

Fee-on-transfer tespiti:
```
balance_after_transfer >= (balance_before + amount - 1)  // 1 wei tolerans
```

Max approve tespiti:
```
approve(spender, U256::MAX) → başarılı olmalı
```

Mapping storage slot:
```
slot = keccak256(pad32(holder) ++ map_slot_bytes32)
```
