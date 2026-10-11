# CoW Protocol Analiz — Agent 16
## Kapsam: GPv2Settlement.sol — Ana Settlement Kontratı
## DEX Özel Protokolü: 3x Hassasiyet

---

## 1. Kaynak Kod Durumu

`GPv2Settlement.sol` bu repoda kaynak hâlde bulunmuyor. Kontrat `@cowprotocol/contracts` npm paketinde yaşıyor.
Ancak analize baz alınabilecek 3 birincil kaynak mevcut:

| Kaynak | Yol | İçerik |
|--------|-----|---------|
| ABI + event tanımları | `contracts/artifacts/GPv2Settlement.json` | Tüm fn imzaları, event'lar |
| Solidity arayüzü | `contracts/solidity/interfaces/ISettlement.sol` | `settle()` tam imzası + struct'lar |
| Rust encoding katmanı | `crates/driver/src/domain/competition/solution/encoding.rs` | settle() calldata inşası satır satır |
| Rust trade modeli | `crates/driver/src/domain/competition/solution/trade.rs` | Cleared price hesabı |
| Rust scoring | `crates/driver/src/domain/competition/solution/scoring.rs` | CIP-38 score, surplus, protocol fee |
| Rust settlement | `crates/driver/src/domain/competition/solution/settlement.rs` | encode/simulate/validate akışı |
| Authenticator ABI | `contracts/artifacts/GPv2AllowListAuthentication.json` | Solver whitelist mekanizması |
| Winner selection | `crates/autopilot/src/domain/competition/winner_selection.rs` | Auction arbitration |

Settlement adresi: `0x9008D19f58AAbD9eD0D60971565AA8510560ab41` (mainnet, Gnosis, Arbitrum, Base…)

---

## 2. GPv2Settlement Kontrat Yapısı — ABI'dan Tam Analiz

### 2.1 Constructor

```
constructor(GPv2Authentication authenticator_, IVault vault_)
```

- `authenticator_`: Hangi EOA'ların `settle()` çağırabileceğini belirleyen whitelist kontratı.
- `vault_`: Balancer V2 Vault — token transferleri buradan geçiyor. Internal/external balance desteği.
- Storage'a yazılan 2 immutable slot: `authenticator`, `vault`.
- Domain separator constructor'da hesaplanır: `keccak256(abi.encode(EIP712_DOMAIN_TYPE, "Gnosis Protocol", "v2", chainId, address(this)))`.

### 2.2 Storage Variables (ABI + Rust referansından çıkarılan)

| Variable | Tip | Açıklama |
|----------|-----|----------|
| `filledAmounts` | `mapping(bytes => uint256)` | Order UID → executed amount (replay protection) |
| `preSignature` | `mapping(bytes => uint256)` | Order UID → block number (on-chain pre-sign) |
| `authenticator` | `GPv2Authentication` | Solver whitelist kontratı adresi |
| `vaultRelayer` | `GPv2VaultRelayer` | Vault'tan token pull eden yardımcı kontrat |

`filledAmounts` en kritik storage: partial fill tracking ve replay attack önleme tek buradan geliyor.

### 2.3 Tüm Fonksiyonlar — Tam Liste

#### `settle(address[] tokens, uint256[] clearingPrices, GPv2Trade.Data[] trades, GPv2Interaction.Data[][3] interactions)`

Ana settlement fonksiyonu. Tüm trade execution buradan akıyor. Detaylı analiz bölüm 3'te.

#### `filledAmount(bytes orderUid) view returns (uint256)`

- `orderUid` = 56 byte: `orderHash (32) + owner (20) + validTo (4)`.
- Sell order için: doldurulmuş sell token miktarı.
- Buy order için: doldurulmuş buy token miktarı.
- Return değeri `>= order.amount` ise order tamamen dolmuştur.

#### `invalidateOrder(bytes orderUid)`

- `msg.sender` == order owner olmak zorunda (order UID içinde owner adresi gömülü).
- `filledAmounts[orderUid] = type(uint256).max` yazarak order'ı kalıcı olarak geçersiz kılıyor.
- Event: `OrderInvalidated(owner indexed, orderUid)`.

#### `setPreSignature(bytes orderUid, bool signed)`

- Kullanıcı EIP-712 imzası atmak yerine on-chain işlem göndererek order'ı imzalayabilir.
- `signed = true` → `preSignature[orderUid] = block.number`.
- `signed = false` → `preSignature[orderUid] = 0` (iptal).
- Smart contract cüzdanlar için kritik: EIP-1271 signature veremeyenlerin kullanabileceği yol.
- Event: `PreSignature(owner indexed, orderUid, signed)`.

#### `domainSeparator() view returns (bytes32)`

- EIP-712 domain separator. Order hash hesaplamasında kullanılıyor.
- Chain ID değişirse domain separator değişir (replay protection cross-chain).

#### `authenticator() view returns (GPv2Authentication)`

- Whitelist kontratını döndürür.

#### `vaultRelayer() view returns (address)`

- VaultRelayer adresini döndürür.

#### `receive() payable`

- Settlement kontratı ETH alabilir (native ETH buy order'larında WETH unwrap sonrası kullanılır).

### 2.4 Events — Tam Liste

| Event | İndeksli | Tetiklendiği Yer |
|-------|----------|-----------------|
| `Settlement(solver indexed)` | solver | Her başarılı `settle()` sonunda |
| `Trade(owner indexed, sellToken, buyToken, sellAmount, buyAmount, feeAmount, orderUid)` | owner | Her trade execute edildiğinde |
| `Interaction(target indexed, value, selector)` | target | Her interaction çağrıldığında |
| `OrderInvalidated(owner indexed, orderUid)` | owner | `invalidateOrder()` çağrıldığında |
| `PreSignature(owner indexed, orderUid, signed)` | owner | `setPreSignature()` çağrıldığında |

`Trade` event'ı off-chain indexer'ların en çok baktığı yer. Rust tarafındaki `Indexer` bu event'ları PostgreSQL'e yazıyor.

---

## 3. settle() — Tam Execution Akışı (Adım Adım)

### 3.1 Fonksiyon İmzası

```solidity
function settle(
    address[] calldata tokens,          // Token index array
    uint256[] calldata clearingPrices,  // tokens[i] için price[i]
    GPv2Trade.Data[] calldata trades,   // Tüm kullanıcı order'ları
    GPv2Interaction.Data[][3] calldata interactions  // [pre, main, post]
) external
```

`ISettlement.sol`'dan (satır 32-37):
```solidity
function settle(
    address[] calldata tokens,
    uint256[] calldata clearingPrices,
    Trade[] calldata trades,
    Interaction[][3] calldata interactions
) external;
```

`Trade` struct (satır 6-24):
```solidity
struct Trade {
    uint256 sellTokenIndex;   // tokens[] içindeki index
    uint256 buyTokenIndex;    // tokens[] içindeki index
    address receiver;         // alıcı adres
    uint256 sellAmount;       // imzalanan sell limit
    uint256 buyAmount;        // imzalanan buy limit
    uint32 validTo;           // son geçerlilik timestamp
    bytes32 appData;          // uygulama metadata hash
    uint256 feeAmount;        // ücret (artık 0, legacy)
    uint256 flags;            // 8 bit packed flags (aşağıda)
    uint256 executedAmount;   // gerçekte execute edilen miktar
    bytes signature;          // EIP-712 / EthSign / EIP-1271 / PreSign
}
```

`Interaction` struct (satır 6-10):
```solidity
struct Interaction {
    address target;
    uint256 value;
    bytes callData;
}
```

### 3.2 Flags Bit Paketi

Kaynak: `encoding.rs` satır 393-421 ve `tokenized.rs` satır 51-98.

```
Bit 0    : order side (0=Sell, 1=Buy)
Bit 1    : partially_fillable (0=Fill-or-Kill, 1=Partial)
Bits 2-3 : sell_token_balance (00=ERC20, 10=External, 11=Internal)
Bit 4    : buy_token_balance (0=ERC20, 1=Internal)
Bits 5-6 : signing_scheme (00=EIP-712, 01=EthSign, 10=EIP-1271, 11=PreSign)
```

### 3.3 Execution Adımları (Kontrat içi sıra)

**Adım 1 — Solver Authentication**
```
authenticator.isSolver(msg.sender) == true
```
Aksi hâlde revert. Solver whitelist kontrolü burada yapılıyor. Bu `onlyOwner` değil, merkezi bir whitelist kontratı (`GPv2AllowListAuthentication`). Manager adresi solver ekleyip çıkarabiliyor.

**Adım 2 — Pre-Interactions**
```
interactions[0] → her biri için target.call{value: value}(callData)
```
- Kullanıcı pre-hook'ları burada çalışır (örn. AAVE'den flash loan alma).
- Reentrancy açığı potansiyeli: pre-interaction settlement kontratını tekrar çağırabilir mi? Kontrat bu durumu `nonReentrant` modifier ile engelliyor.

**Adım 3 — Token Transferleri (Sell Token Pull)**
Her trade için:
- Signature doğrulama: `order hash` hesaplanır, signing scheme'e göre doğrulanır.
- `filledAmount[orderUid]` kontrol: order daha önce doldurulmuş mu?
- `validTo >= block.timestamp` kontrol.
- Sell token: `vaultRelayer.transferFrom(owner, settlement, sellAmount + fee)`.
- `filledAmounts[orderUid]` güncelleme.

**Adım 4 — Main Interactions**
```
interactions[1] → her biri için target.call{value: value}(callData)
```
- AMM swap'ları, routing, liquidity pool çağrıları burada.
- Solver'ın hazırladığı arbitraj/routing mantığı burada execute edilir.

**Adım 5 — Clearing Price Validation + Buy Token Pay-out**
Her trade için:

*Sell order için buy amount hesabı:*
```
buyAmount = executedSellAmount * clearingPrices[sellTokenIndex]
            / clearingPrices[buyTokenIndex]
```
Tam formül (ceil division):
```
buyAmount = ceil(executedSellAmount * priceSell / priceBuy)
```

*Buy order için sell amount hesabı:*
```
sellAmount = executedBuyAmount * clearingPrices[buyTokenIndex]
             / clearingPrices[sellTokenIndex]
```

**Limit price kontrolü:**
- Sell order: `buyAmount >= order.buyAmount * executedSellAmount / order.sellAmount`
  (Kullanıcı en az imzaladığı oranda buy token almalı)
- Buy order: `sellAmount <= order.sellAmount * executedBuyAmount / order.buyAmount`
  (Kullanıcı en fazla imzaladığı oranda sell token vermeli)

Buy token: settlement kontratından alıcıya transfer edilir.
Fee: settlement kontratında kalır (solver surplus).

**Adım 6 — Post-Interactions**
```
interactions[2] → her biri için target.call{value: value}(callData)
```
- Kullanıcı post-hook'ları (örn. flash loan geri ödeme).
- WETH → ETH unwrap burada gerçekleşebilir.

**Adım 7 — Settlement Event**
```
emit Settlement(msg.sender)
```

### 3.4 Uniform Clearing Price Mekanizması

Uniform clearing price vektörü tek bir `tokens[]` + `clearingPrices[]` çiftiyle temsil ediliyor:
- `price[A] = clearingPrices[indexOf(A)]`
- Tüm trade'ler aynı token için aynı fiyatı görür.
- `amount_A * price_A = amount_B * price_B` → değer korunumu.

Rust `encoding.rs` satır 57-65'te bu vektörün nasıl inşa edildiği:
```rust
for (token, amount) in solution.clearing_prices()
    .into_iter()
    .sorted_by_cached_key(|(token, _amount)| *token)
{
    tokens.push(token.into());
    clearing_prices.push(amount);
}
```

Sonra her trade için custom prices ekleniyor (satır 67-168): Uniform price vectoründen ayrı olarak her trade'in kendi (`sell_price`, `buy_price`) çifti de vektöre append ediliyor. Bu şekilde fee ve haircut ayarlamaları trade bazında yapılabiliyor.

---

## 4. Güvenlik Mekanizmaları — Detaylı Analiz

### 4.1 Reentrancy Koruması

Kontrat `nonReentrant` (OpenZeppelin ReentrancyGuard veya eşdeğeri) kullanıyor. Rust tarafından doğrudan kaynak görülmese de:
- `Settlement` event'ı settle() bitiminde emit ediliyor (checks-effects-interactions pattern).
- Interaction'lar her ne kadar arbitrary call olsa da settlement kontratına geri dönüş güvenli değil.

Rust `settlement.rs` satır 92-108:
```rust
// Internalization rule: check that internalized interactions only use trusted tokens
let untrusted_tokens = solution
    .interactions.iter()
    .filter(|interaction| interaction.internalize())
    .flat_map(|interaction| interaction.inputs())
    .filter(|asset| !auction.tokens().get(&asset.token)
        .map(|token| token.trusted).unwrap_or_default())
    .map(|asset| asset.token)
    .collect::<BTreeSet<_>>();
if !untrusted_tokens.is_empty() {
    return Err(Error::NonBufferableTokensUsed(untrusted_tokens));
}
```
Bu kontrol off-chain simülasyon aşamasında yapılıyor, on-chain değil.

### 4.2 Solver Authentication (Whitelist)

`GPv2AllowListAuthentication` kontratı (ABI satır 1-165):

```solidity
function isSolver(address prospectiveSolver) view returns (bool)
function addSolver(address solver)
function removeSolver(address solver)
function initializeManager(address manager_)
function setManager(address newManager)
```

- `settle()` çağrısı: `authenticator.isSolver(msg.sender)` false ise revert.
- Manager: solver ekleyip çıkartabiliyor. Manager değişikliği event ile loglanıyor.
- Events: `SolverAdded(solver)`, `SolverRemoved(solver)`, `ManagerChanged(newManager, oldManager)`.
- Bu bir merkezi whitelist, permissioned model. DAO governance ile kontrol edilebilir (manager adresi).

Önemli güvenlik notu: Kontrat `onlyOwner` değil, `onlyManager` (ayrı bir kontrat). Manager bile solver olabilir. Bu tasarım solver'ların slashing ile değil, whitelist removal ile cezalandırılmasına izin veriyor.

### 4.3 Price Sanity Check (Solver Manipulation Koruması)

On-chain fiyat sanity check YOK. Kontrat clearing price vektörünü doğrudan kabul ediyor. Koruma mekanizması off-chain:

1. **Off-chain simülasyon**: Driver, settlement tx'ini blockchain state fork üzerinde simulate ediyor (`settlement.rs` satır 182-198). Simulation revert ederse tx gönderilmiyor.
2. **Limit price check**: Kontrat on-chain olarak her trade'in limit price'ını enforce ediyor (bölüm 3.3). Bu temel MEV koruması: kullanıcı imzaladığından daha kötü fiyat ALAMAZ.
3. **Autopilot circuit breaker**: Gerçekleşen settlement ile beklenen outcome karşılaştırılıyor, sapma varsa solver jail ediliyor.
4. **Score ranking**: Autopilot winner selection'da fairness garantisi: bir order, tek başına execute edilseydi alacağından daha kötü fiyat alamaz (`winner_selection.rs` satır 11-15).

### 4.4 Trade Validation (Executed Amount Limits)

Rust `trade.rs` satır 136-173:
```rust
let executed_with_fee = order::TargetAmount(
    executed.0.checked_add(fee.0).ok_or(error::Trade::InvalidExecutedAmount)?
);
match order.partial {
    order::Partial::Yes { available } => executed_with_fee <= available,
    order::Partial::No => executed_with_fee == order.target(),
}
```

- Fill-or-Kill order: `executed + fee == order.target()` olmalı. Tam doldurulmuş olmalı.
- Partial order: `executed + fee <= available` olmalı. Fazla doldurulamaz.
- Bu kontrol off-chain driver'da yapılıyor; on-chain `filledAmounts` kümülatif takip ediyor.

### 4.5 Interaction Güvenliği

`interactions[][3]` arbitrary call'lardır. Bu kontratın en büyük attack surface'i. Mitigasyonlar:

1. **Solver whitelist**: Arbitrary call sadece whitelisted solver'lar gönderiyor.
2. **Pre-simulation**: Her interaction on-chain'den önce simüle ediliyor.
3. **Internalization**: Trusted token gerektiren internal buffer işlemleri sadece güvenilir token'lar için yapılabiliyor.
4. **Gas limit**: `settlement.rs` satır 408: settlement tx block gas limit'inin yarısını geçemez. EIP-7825 ile ayrıca per-tx cap.

---

## 5. Clearing Price Mekanizması — Matematiksel Detay

### 5.1 Uniform Clearing Price (Batch Auction Özü)

CoW Protocol'ün temel innovasyonu: Tek bir batch içindeki tüm trade'ler aynı uniform clearing price'a göre settle ediliyor.

Matematiksel formül:
```
P(A) / P(B) = k  (sabit oran, batch içinde)
```

Eğer kullanıcı A, B/A ≥ p_min oranını imzalamışsa ve uniform price P(B)/P(A) = k ≥ p_min ise:
- Kullanıcı execute edilen miktarın TAMAMINI k oranında alır.
- k > p_min ise kullanıcı surplus kazanır (price improvement).

### 5.2 Surplus (Price Improvement) Hesabı

Rust `scoring.rs` satır 124-168, `surplus_over()` fonksiyonu:

**Sell Order Surplus:**
```
limit_buy_scaled = limit_buy * executed_sell / limit_sell   (partial fill scaling)
actual_buy = executed_sell * custom_price.sell / custom_price.buy  (ceil div)
surplus_in_buy_token = actual_buy - limit_buy_scaled
```

**Buy Order Surplus:**
```
limit_sell_scaled = limit_sell * executed_buy / limit_buy   (partial fill scaling)
actual_sell = executed_buy * custom_price.buy / custom_price.sell
surplus_in_sell_token = limit_sell_scaled - actual_sell
```

### 5.3 Protocol Fee Hesabı

3 fee policy mekanizması (`scoring.rs` satır 252-276):

**1. Surplus Fee:**
```
fee_before = surplus_before_fee * factor
fee_after  = surplus_after_fee * (factor / (1 - factor))
```
(`scoring.rs` satır 316-337)

**2. Volume Fee:**
```
# Sell order:
fee = traded_buy_amount' * factor / (1 - factor)
# Buy order:
fee = traded_sell_amount' * factor / (1 + factor)
```
(`scoring.rs` satır 342-385)

**3. Price Improvement Fee:**
Quote ile karşılaştırılıyor; quote'dan daha iyi fiyat varsa o fark üzerinden fee alınıyor.

### 5.4 CIP-38 Score Hesabı

Autopilot solver'ları sıralamak için score kullanıyor (`scoring.rs` satır 83-119):

**Sell order için:**
```
score = (surplus_in_buy_token + protocol_fees_in_buy_token) * native_price_of_buy_token
```

**Buy order için:**
```
surplus_in_sell_token → buy_token ile dönüştür:
  buy_amount = surplus_sell * signed_buy_amount / signed_sell_amount
score = buy_amount * native_price_of_buy_token
```

Bu formül `widening_mul` (U512) kullanılarak overflow'a karşı korunuyor (satır 109-113).

### 5.5 Solver Surplus Sınırlaması

Kullanıcı limit price'ı kontrat tarafından on-chain enforce ediliyor. Solver limit price'ı aşan surplus'ı kendine alamaz çünkü:
- Sell order: `actual_buy >= limit_buy * executed / sellAmount` (kontrat kontrol ediyor).
- Buy order: `actual_sell <= limit_sell * executed / buyAmount` (kontrat kontrol ediyor).

Surplus — yani limit'in üzerindeki kısım — settlement kontratında kalır. Bu miktar solver'ın fee'sidir (gas + protocol fee + solver reward).

---

## 6. Custom Clearing Prices (Per-Trade Adjustment)

`encoding.rs` satır 67-168'de her trade için uniform price'tan ayrı custom prices hesaplanıyor:

**JIT Order için:**
```rust
Price {
    sell_token: trade.order().sell.token,
    sell_price: trade.order().buy.amount,   // limit price: buy.amount birim sell token
    buy_price: trade.order().sell.amount,   // limit price: sell.amount birim buy token
}
```
JIT order'lar limit price'tan execute ediliyor (surplus yok).

**Fulfillment Order için:**
```rust
custom_prices = trade.custom_prices(&uniform_prices)?
// custom_prices.sell = actual_buy_amount (buy_amount fonksiyonundan)
// custom_prices.buy  = actual_sell_amount (sell_amount fonksiyonundan)
```
`trade.rs` satır 292-297:
```rust
pub fn custom_prices(&self, prices: &ClearingPrices) -> Result<CustomClearingPrices, error::Math> {
    Ok(CustomClearingPrices {
        sell: self.buy_amount(prices)?.into(),
        buy: self.sell_amount(prices)?.into(),
    })
}
```

Bu tersine çevrilmiş (sell_price = buy_amount, buy_price = sell_amount) encoding, kontratın ratio hesabını doğru yapmasını sağlıyor.

---

## 7. Flashloan Entegrasyonu

`encoding.rs` satır 227-284:

Eğer solution'da flashloan varsa, settlement tx doğrudan GPv2Settlement'a değil `FlashLoanRouter`'a gönderiliyor:

```rust
if has_flashloans {
    encode_flashloan_settlement(solution, contracts, settle_calldata)?
}
// → router.flashLoanAndSettle(flashloans, settle_calldata)
```

`LoanRequest::Data` yapısı:
```rust
{
    amount: flashloan.amount,
    borrower: flashloan.protocol_adapter,
    lender: flashloan.liquidity_provider,
    token: flashloan.token,
}
```

Kısıtlama: Flashloan ve wrapper (EIP-4626 wrapping) aynı settlement'ta olamaz (`FlashloanWrappersIncompatible` error).

---

## 8. WETH / Native ETH Akışı

`encoding.rs` satır 83-85 + satır 204-207:
```rust
if trade.order().buy.token == eth::ETH_TOKEN {
    native_unwrap += trade.buy_amount(&uniform_prices)?;
}
// ...
if !native_unwrap.0.is_zero() && solver_native_token.insert_unwraps {
    interactions.push(unwrap(native_unwrap, contracts.weth()));
}
```

ETH satın alan order'lar WETH üzerinden çalışıyor; driver otomatik olarak WETH.withdraw() interaction'ı ekliyor.

`clearing_prices()` fonksiyonu (`mod.rs` satır 517-542): ETH ve WETH aynı price'a sahip. WETH price, ETH price olarak da ekleniyor.

---

## 9. Mempool Submission ve Gas Güvenliği

`mempools.rs` satır 22-26:
```rust
const GAS_PRICE_BUMP_PCT: u64 = 13;   // %13 bump (EIP-1559 gerekliliği %12.5, +biraz buffer)
const CANCELLATION_GAS_AMOUNT: u64 = 21000;  // pending tx iptal için
```

Gas limit hesabı `settlement.rs` satır 386-432:
```rust
const GAS_LIMIT_FACTOR: f64 = 2.0;
let max_gas = min(block_limit / 2, tx_gas_limit);  // block'un yarısı veya EIP-7825 cap
let limit = min(max_gas, estimate * GAS_LIMIT_FACTOR);
```

**EIP-7825 Mainnet Fusaka cap**: `2^24 - 1 = 16,777,215` gas per tx (`settlement.rs` satır 460).

Solver balance kontrolü (satır 203-213):
```rust
let required_eth_balance = gas.required_balance(submission_max_fee_per_gas(...));
if solver_eth < required_eth_balance {
    return Err(Error::SolverAccountInsufficientBalance(required_eth_balance));
}
```

Submission fee hesabı (satır 440-447):
```rust
override_max_fee_per_gas
    .map(U256::from)
    .unwrap_or_else(|| U256::from(driver_max_fee_per_gas).saturating_mul(U256::from(2)))
```
Driver kendi tahminini 2x ile kullanıyor (gas climbing buffer). Solver override varsa kendi değeri kullanılıyor.

---

## 10. On-Chain Event Indexing

`boundary/events/settlement.rs`:

```rust
pub struct GPv2SettlementContract {
    provider: AlloyProvider,
    address: Address,
}

impl AlloyEventRetrieving for GPv2SettlementContract {
    type Event = GPv2SettlementEvents;  // Trade + Settlement + Interaction + ...
}
```

Indexer event'ları Postgres'e yazıyor. `replace_events()` chain reorg'larını handle ediyor (from_block'tan itibaren siliyor ve yeniden yazıyor).

---

## 11. Winner Selection — Fairness Algoritması

`winner_selection.rs` satır 1-39:

Uniform **directional** clearing price — tek token için 2 fiyat olabiliyor: biri satarken, biri alırken. Bu, aynı token çiftinin 2 yönünde ayrı clearing price kullanılabilmesine izin veriyor.

3 aşama:
1. **Unfair solution filtreleme**: Bir order, o order'ı içeren tek-order solution'dan daha kötü fiyat alamaz.
2. **Winner marking**: Max score'u veren solution set'i seçiliyor (multiple winners mümkün).
3. **Reference score hesabı**: Her solver S için "S olmasa toplam score ne olurdu?" sorusu yanıtlanıyor. Reward = score - reference_score.

Bu mekanizma Vickrey auction'ına benzyor: Solver'lar gerçek değeri açıklamak zorunda, sadece marjinal contribution'larını kazanıyorlar.

---

## 12. USDT Approve Özel Durumu

`mod.rs` satır 330-346:
```rust
let regular_approval_succeeds = erc20_token
    .approve(required.0.spender, required.0.amount)
    .from(settlement_contract)
    .call()
    .await.is_ok();

if regular_approval_succeeds {
    Ok(vec![approval])
} else {
    // USDT-like token: önce 0'a set et
    Ok(vec![approval.revoke(), approval])
}
```
USDT gibi token'larda (allowance'ı azaltma yerine 0'a set etme gerektiren ERC-20'ler) simülasyon yapılarak `approve(0)` eklenip eklenmeyeceğine karar veriliyor.

---

## 13. Auction ID Calldata Trailing Bytes

`encoding.rs` satır 226:
```rust
settle_calldata.extend(auction.id().ok_or(Error::MissingAuctionId)?.to_be_bytes());
```

`settle()` calldata'nın sonuna auction ID ekleniyor. Bu:
1. Off-chain indexer'ların hangi on-chain settlement'ın hangi auction'a ait olduğunu tespit etmesini sağlıyor.
2. ABI decode edildiğinde görünmüyor (trailing bytes), ama event log + calldata kombine analizi ile çıkarılabiliyor.

---

## 14. Tam Güvenlik Risk Matrisi

| Risk | Mitigation | Seviye |
|------|-----------|--------|
| Replay Attack (aynı order tekrar settle) | `filledAmounts[orderUid]` on-chain tracking | Güçlü |
| Rogue Solver (kötü clearing price) | `authenticator.isSolver()` + limit price on-chain check | Orta-Güçlü |
| MEV (frontrun/sandwich) | Private mempool (Flashbots) desteği, revert protection | Orta |
| Reentrancy | `nonReentrant` (on-chain), simulation (off-chain) | Güçlü |
| Order Expiry Bypass | `validTo >= block.timestamp` on-chain check | Güçlü |
| Cross-chain Replay | `domainSeparator` chain ID içeriyor | Güçlü |
| Arbitrage Leak | Protocol fee (surplus fee policy) ile surplus'ın bir kısmı kesilir | Orta |
| Gas DoS | Gas limit: `min(block/2, EIP-7825 cap)` | Güçlü |
| Interaction Exploit | Solver whitelist + pre-simulation + trusted token check | Orta |

---

## 15. DEX Uygulama Notu — Nova Shield L1 Native Eşdeğeri

### 15.1 Smart Contract Yok — Protokol Katmanında Settlement

Nova Shield bir L1 blockchain olduğu için GPv2Settlement'ın Solidity mantığı protokol katmanına taşınmalı. Eşdeğer mekanizmalar:

**Token Transfer Güvenliği (filledAmounts eşdeğeri):**
```rust
// L1 state'de order tracking
struct OrderState {
    filled_amount: u128,     // kümülatif doldurulmuş miktar
    invalidated: bool,       // kullanıcı iptal etti mi
}
// Replay protection: order_uid → OrderState mapping
// Account nonce'ı değil, order UID'e dayalı tracking
```

**Solver Authentication (whitelist eşdeğeri):**
```rust
// Validator set içinde solver whitelist
// Ya PoS stake miktarına göre (stake-based solver admission)
// Ya da governance tarafından onaylanan solver set
// Slashing: misbehaving solver'ın stake'inin kesilmesi
```

**Clearing Price Enforcement (on-chain eşdeğeri):**
```rust
// Settlement transaction payload'ında:
struct BatchSettlement {
    tokens: Vec<TokenId>,
    clearing_prices: Vec<u128>,  // tokens[i] için price[i]
    trades: Vec<TradeExecution>,
    interactions: Vec<ProtocolInteraction>,
    auction_id: u64,
}

// Her trade için protocol seviyesinde doğrulama:
fn validate_trade(trade: &TradeExecution, prices: &[u128]) -> bool {
    let sell_price = prices[trade.sell_token_index];
    let buy_price = prices[trade.buy_token_index];
    // Sell order: actual_buy >= limit_buy * executed / limit_sell
    match trade.side {
        Side::Sell => {
            let min_buy = trade.limit_buy_amount
                .checked_mul(trade.executed_amount).unwrap()
                .checked_div(trade.limit_sell_amount).unwrap();
            let actual_buy = trade.executed_amount
                .checked_mul(sell_price).unwrap()
                .checked_div(buy_price).unwrap();
            actual_buy >= min_buy
        }
        // Buy order analojik...
    }
}
```

**Interaction Mekanizması (L1 native):**
L1 blockchain'de EVM call stack yok. Bunun yerine:
```rust
// Pre/Post interactions → L1 transaction hooks (protokol seviyesinde)
// Main interactions → Cross-program invocation (Solana benzeri) veya IBC mesajları
// Ya da: External protocol calls için bir "solver program" konsepti
```

**Reentrancy Koruması (L1 native):**
L1'de reentrancy farklı tanımlanır. Bir block içinde aynı state slot'una çift yazma ya da paralel batch işleme yarışları önlenmeli:
```rust
// Scheduler seviyesinde: aynı account'a touching eden tx'leri serielize et
// Ya da: settlement transaction türü için özel işlem sırası garantisi
```

### 15.2 Nova Shield DEX Mimari Kararları

**1. Batch Auction Sıklığı:**
CoW Protocol ~12-15 saniyede bir batch. Nova Shield için hedef: her block'ta bir batch (Solana benzeri). Bu agresif bir hedef; solver response time'ı kısalır.

**2. Solver Competition:**
CoW Protocol'den alınacak tasarım: Solver'lar rekabet ediyor, en yüksek surplus (CIP-38 score) kazanıyor. Nova Shield'de:
- Solver'lar L1 validation node'larından ayrı tutulmalı (aksi hâlde validator = solver çakışması MEV riski).
- Solver staking: stake amount = max_settlement_value guarantor.

**3. MEV Koruması (L1 avantajı):**
L1'de validator block production kontrolü var. Private mempool'a gerek yok:
- Block proposer, kendi block'una yalnızca kendi chain'den gelen settlement'ı dahil eder.
- Validator slashing: settlement dışı MEV extract etmeyi ekonomik olarak dezavantajlı yapar.
- Bu CoW Protocol'ün Flashbots gereksinimine kıyasla daha güçlü MEV koruması.

**4. Clearing Price Oracle:**
CoW Protocol'de clearing price solver tarafından öneriliyor + off-chain competition. Nova Shield için ek seçenek:
- ZK-proven price commitment: Solver, batch'i gönderirken kullandığı off-chain price feed'e ZK proof ekler.
- On-chain TWAP oracle ile sanity bound: `clearing_price < TWAP * 1.05` koşulu enforce edilir.

**5. filledAmounts → UTXO veya Account Model:**
CoW Protocol'de `filledAmounts` storage mapping. Nova Shield'de:
- Account model: Her order için ayrı bir "order account" (Solana program account benzeri).
- Partial fill durumunda order account state'i güncelleniyor.
- Order expire edildiğinde account kapatılıyor (rent recovery eşdeğeri: depolama alanı iade).

**6. Auction ID Trailing Bytes:**
Bu pattern korunmalı. Nova Shield'de settlement transaction body'sinin sonuna `auction_id: u64` eklenerek off-chain indexer senkronizasyonu sağlanmalı.

### 15.3 Direkt Eylem Planı

| CoW Protocol Bileşeni | Nova Shield Karşılığı | Öncelik |
|----------------------|----------------------|---------|
| `GPv2Settlement.settle()` | `BatchSettlementTx` L1 transaction tipi | YÜKSEK |
| `filledAmounts` mapping | Order Account state (rocksdb key-value) | YÜKSEK |
| `GPv2AllowListAuthentication` | Solver stake registry (onchain governance) | YÜKSEK |
| Uniform clearing price vector | `BatchSettlementTx.clearing_prices[]` field | YÜKSEK |
| Limit price on-chain check | Protocol-layer trade validator | YÜKSEK |
| Pre/Main/Post interactions | Transaction hooks veya CPI | ORTA |
| CIP-38 scoring | Auction arbitration module (off-chain) | ORTA |
| Flashloan router | Flash credit mekanizması (L1 credit line) | DÜŞÜK |
| WETH unwrap | Native token unwrap hook | ORTA |
| Access list for ETH transfer | Benzer mekanizma gerekmez (L1 native transfer) | YOK |

---

## 16. Önemli Tasarım Kararları

1. **Price vector indexed by position, not address**: Token'lar tokens[] dizisinde bir kez listeleniyor, price'lar paralel clearingPrices[] dizisinde. Bu, aynı token'ın birden fazla yer almasını önlüyor ama indeks tabanlı erişimi gerektiriyor. Nova Shield'de address-keyed map daha güvenli olabilir.

2. **Atomic settlement**: Tek bir `settle()` çağrısı tüm batch'i atomik execute ediyor. Kısmi başarı yok (tümü geçer veya tümü revert). Nova Shield'de bu garanti aynı şekilde korunmalı.

3. **Fee amount her zaman 0**: `encoding.rs` satır 105: `fee_amount: eth::U256::ZERO`. Eski versiyonlarda fee imzalanıyordu, yeni versiyonda fee solver tarafından `executedAmount` içinde işleniyor. Nova Shield'de fee accounting, `executedAmount` içinde gizli değil, explicit bir fee field olarak kodlanmalı (şeffaflık için).

4. **Audit ID in calldata**: Settlement tx'e audit trail için auction_id eklenmesi. Nova Shield'de bu pattern kesinlikle korunmalı ve genişletilmeli (solver_id, auction_round da eklenebilir).

5. **Solution merging**: `mod.rs` satır 361-448: İki solver solution'ı birleştirilebilir eğer token fiyatları congruent ise. `scaling_factor()` ile ölçeklendiriliyorlar. Bu paralel solver competition için kritik: birden fazla solver bir auction'ı farklı token çiftlerinde çözebilir ve sonuçlar birleştirilebilir.

---

## 17. Özet Tablo

| Bileşen | Satırlar / Dosya | Key Finding |
|---------|-----------------|-------------|
| ISettlement.sol | 38 satır | settle() imzası, Trade/Interaction struct |
| GPv2Settlement.json ABI | 500+ satır JSON | 5 function, 5 event, constructor |
| GPv2AllowListAuthentication.json | 165 satır JSON | isSolver(), addSolver(), manager pattern |
| encoding.rs | 471 satır | Token index building, custom prices, flags |
| trade.rs | 557 satır | sell/buy amount, surplus, haircut, fee |
| scoring.rs | 450 satır | CIP-38 score, surplus fee, volume fee |
| settlement.rs | 529 satır | encode+simulate+validate, Gas limit, access list |
| mod.rs (solution) | 787 satır | Solution struct, merge, clearing_prices, approvals |
| winner_selection.rs | 200+ satır | Fairness filter, reference score, arbitration |
| mempools.rs | 300+ satır | GAS_PRICE_BUMP_PCT=13, revert protection |
| tokenized.rs | 116 satır | TradeFlags decode, order_uid recovery |
| boundary/events/settlement.rs | 88 satır | On-chain event indexing, reorg handling |
