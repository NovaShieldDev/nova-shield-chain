# UV4-01: Uniswap V4 PoolManager — Singleton + Flash Accounting + ERC-6909 + Transient Storage

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v4-core/`
**Kapsanan Dosyalar:**
- `src/PoolManager.sol` (395 satır — tam analiz)
- `src/libraries/Lock.sol`
- `src/libraries/CurrencyDelta.sol`
- `src/libraries/NonzeroDeltaCount.sol`
- `src/ERC6909Claims.sol`
- `src/ERC6909.sol`
- `src/types/PoolId.sol`
- `src/types/BalanceDelta.sol`
- `src/types/PoolKey.sol`

---

## 1. V4'ün Temel Mimarisi: Singleton Pool Manager

### 1.1 V3 vs V4 Karşılaştırması

| Özellik | V3 | V4 |
|---------|-----|-----|
| Pool yapısı | Her pool ayrı contract | Tek contract (Singleton) |
| State | Pool başına storage | PoolManager içinde mapping |
| Token transfer | Her swap'ta ERC20.transfer | Flash accounting (unlok→settle) |
| Reentrancy koruması | lock modifier (storage) | tstore/tload (transient) |
| Token standartı | ERC20 | ERC-6909 (claim token) |
| Hooks | Yok | 14-bit permission bitmap |
| Flash loan | Pool.flash() | take() + settle() |

### 1.2 Singleton Faydaları

1. **Gas tasarrufu:** Token0→Token1→Token2 multi-hop: V3'te 3 transfer, V4'te 0 (net settle)
2. **Atomik çoklu işlem:** Birden fazla pool'da tek tx
3. **Deploy maliyeti:** Her pool için yeni contract yok
4. **State sıkıştırma:** Tüm pool state'i tek contract'ta

---

## 2. Transient Storage (EIP-1153) — V4'ün Kalbi

EIP-1153, Ethereum Cancun upgrade'inde (2024) eklendi. Transient storage:
- `TSTORE` / `TLOAD` opcode'ları
- Transaction süresince yaşar
- Transaction sona erince sıfırlanır (refund yok)
- Normal SSTORE'dan ~20x ucuz

### 2.1 IS_UNLOCKED_SLOT

```solidity
// Lock.sol
bytes32 internal constant IS_UNLOCKED_SLOT =
    0xc090fc4683624cfc3884e9d8de5eca132f2d0ec062aff75d43c0465d5ceeab23;
    // = bytes32(uint256(keccak256("Unlocked")) - 1)
```

**Neden -1?** EIP-1967 pattern — keccak256 output'u 2^256 aralığında uniform dağılır. -1 ile `0x...23` yerine kesin bir değer belirlenir (collision'dan kaçınma).

```solidity
function unlock() internal {
    assembly { tstore(IS_UNLOCKED_SLOT, true) }
}
function lock() internal {
    assembly { tstore(IS_UNLOCKED_SLOT, false) }
}
function isUnlocked() internal view returns (bool) {
    assembly { unlocked := tload(IS_UNLOCKED_SLOT) }
}
```

### 2.2 NONZERO_DELTA_COUNT_SLOT

```solidity
bytes32 internal constant NONZERO_DELTA_COUNT_SLOT =
    0x7d4b3164c6e45b97e7d87b7125a44c5828d005af88f9d751cfd78729c5d99a0b;
    // = bytes32(uint256(keccak256("NonzeroDeltaCount")) - 1)
```

Bu sayaç, settle edilmemiş delta sayısını tutar. `unlock()` biterken:
```solidity
if (NonzeroDeltaCount.read() != 0) CurrencyNotSettled.selector.revertWith();
```

Tüm deltalar sıfırlanmadan işlem bitirilemiyor.

### 2.3 CurrencyDelta — Transient Mapping

```solidity
// CurrencyDelta.sol
function _computeSlot(address target, Currency currency) internal pure returns (bytes32) {
    assembly {
        mstore(0, and(target, 0xffffffffffffffffffffffffffffffffffffffff))
        mstore(32, and(currency, 0xffffffffffffffffffffffffffffffffffffffff))
        hashSlot := keccak256(0, 64)
    }
}
```

**Slot hesabı:** `keccak256(target ++ currency)` — her (kullanıcı, token) çifti için benzersiz transient slot.

```solidity
function applyDelta(Currency currency, address target, int128 delta)
    returns (int256 previous, int256 next)
{
    bytes32 hashSlot = _computeSlot(target, currency);
    assembly { previous := tload(hashSlot) }
    next = previous + delta;
    assembly { tstore(hashSlot, next) }
}
```

---

## 3. Flash Accounting Mekanizması

### 3.1 unlock() → callback → settle() Döngüsü

```
Kullanıcı:
  1. PoolManager.unlock(data) çağırır

PoolManager:
  2. Lock.unlock() → tstore(IS_UNLOCKED_SLOT, true)
  3. IUnlockCallback(msg.sender).unlockCallback(data) çağırır

Kullanıcı (unlockCallback içinde):
  4. PoolManager.swap() / modifyLiquidity() / take() çağırır
     → Delta birikir (transient storage'da)
  5. PoolManager.settle() çağırır
     → Token gönderir, delta sıfırlanır

PoolManager:
  6. NonzeroDeltaCount.read() == 0 kontrolü
  7. Lock.lock() → tstore(IS_UNLOCKED_SLOT, false)
```

### 3.2 Delta Yönleri

| İşlem | Delta yönü | Anlam |
|-------|-----------|-------|
| swap() | Negatif (input), Pozitif (output) | Kullanıcı borçlu, alacaklı |
| modifyLiquidity() (mint) | Pozitif | Kullanıcı borçlu |
| modifyLiquidity() (burn) | Negatif | Kullanıcı alacaklı |
| take() | Negatif | Kullanıcı token alır, borç artar |
| settle() | Pozitif | Token gönderilir, borç azalır |
| mint() ERC6909 | Negatif | Claim token bastırılır, borç azalır |

### 3.3 Çoklu İşlem Nettleşmesi

```
Örnek: ETH/USDC havuzundan USDC al, USDC/DAI havuzuna ver

swap1: ETH giriyor, USDC çıkıyor
  → delta[msg.sender][ETH] += amountETH  (borçlu)
  → delta[msg.sender][USDC] -= amountUSDC (alacaklı)

swap2: USDC giriyor, DAI çıkıyor
  → delta[msg.sender][USDC] += amountUSDC (borçlu)
  → delta[msg.sender][DAI] -= amountDAI  (alacaklı)

Net:
  → delta[USDC] = -amountUSDC + amountUSDC = 0 (settle yok!)
  → delta[ETH] settle, delta[DAI] collect
```

V3'te her hop'ta iki transfer. V4'te sadece net.

---

## 4. PoolKey ve PoolId

### 4.1 PoolKey Struct

```solidity
struct PoolKey {
    Currency currency0;   // 160 bit
    Currency currency1;   // 160 bit
    uint24   fee;         // 24 bit (dinamik fee için 0x800000 sentinel)
    int24    tickSpacing; // 24 bit
    IHooks   hooks;       // 160 bit
}
// Toplam: 5 × 32 byte = 160 byte = 0xa0
```

### 4.2 PoolId Hesabı

```solidity
function toId(PoolKey memory poolKey) internal pure returns (PoolId poolId) {
    assembly ("memory-safe") {
        poolId := keccak256(poolKey, 0xa0)
    }
}
```

**Formül:** `PoolId = keccak256(currency0 ++ currency1 ++ fee ++ tickSpacing ++ hooks)`

Her kombinasyon benzersiz bir Pool ID verir. Hooks adresi de dahil — aynı token çifti farklı hook'larla farklı pool demek.

### 4.3 currency0 < currency1 Zorunluluğu

```solidity
if (key.currency0 >= key.currency1) {
    CurrenciesOutOfOrderOrEqual.selector.revertWith(...);
}
```

V3'te de token0 < token1 zorunluydu. V4'te Currency tipi kullanılır — native ETH = `address(0)`.

---

## 5. BalanceDelta — 256 Bit'te İki int128

### 5.1 Paketleme

```solidity
type BalanceDelta is int256;

// amount0 = üst 128 bit, amount1 = alt 128 bit
function toBalanceDelta(int128 _amount0, int128 _amount1) pure returns (BalanceDelta) {
    assembly {
        balanceDelta := or(shl(128, _amount0), and(sub(shl(128, 1), 1), _amount1))
    }
}
```

**Bit düzeni:**
```
bits [255..128]: amount0 (int128)
bits [127..0]:   amount1 (int128)
```

### 5.2 amount0 ve amount1 Çıkarma

```solidity
function amount0(BalanceDelta bd) internal pure returns (int128) {
    assembly { _amount0 := sar(128, bd) }  // SAR = Signed Arithmetic Right shift
}

function amount1(BalanceDelta bd) internal pure returns (int128) {
    assembly { _amount1 := signextend(15, bd) }  // 15 = 16 byte = 128 bit sign extend
}
```

**SAR (Signed Arithmetic Right):** Sign bit korunarak 128 sağa kaydırma → amount0
**SIGNEXTEND(15, x):** 16. byte'dan (128. bit'ten) işaret uzatır → alt 128 bit olarak amount1

### 5.3 Toplama ve Çıkarma

```solidity
function add(BalanceDelta a, BalanceDelta b) pure returns (BalanceDelta) {
    assembly {
        let a0 := sar(128, a)
        let a1 := signextend(15, a)
        let b0 := sar(128, b)
        let b1 := signextend(15, b)
        res0 := add(a0, b0)
        res1 := add(a1, b1)
    }
    return toBalanceDelta(res0.toInt128(), res1.toInt128());
}
```

Her iki bileşen ayrı ayrı int256'ya genişletilir, toplanır, sonra int128'e cast edilir. Overflow `toInt128()` ile yakalanır.

---

## 6. ERC-6909 — Multi-Token Claim Standardı

### 6.1 ERC-6909 Nedir?

ERC-20'nin minimal çok-token versiyonu. Tek contract'ta sonsuz token ID yönetir. Uniswap V4'te her currency için claim token bastırılabilir.

**Kullanım:** Kullanıcı settle yerine `mint()` çağırarak pozitif delta'yı ERC-6909 claim token'ına dönüştürür. Bu token sonra `burn()` ile harcanar.

### 6.2 Token ID → Currency Dönüşümü

```solidity
// CurrencyLibrary
function fromId(uint256 id) internal pure returns (Currency currency) {
    return Currency.wrap(address(uint160(id)));
}
function toId(Currency currency) internal pure returns (uint256) {
    return uint256(uint160(Currency.unwrap(currency)));
}
```

Token ID = Currency adresinin uint256 değeri. ETH için ID = 0.

### 6.3 mint() ve burn() ERC-6909

```solidity
function mint(address to, uint256 id, uint256 amount) external onlyWhenUnlocked {
    unchecked {
        Currency currency = CurrencyLibrary.fromId(id);
        _accountDelta(currency, -(amount.toInt128()), msg.sender);  // delta azalt
        _mint(to, currency.toId(), amount);                          // claim bas
    }
}

function burn(address from, uint256 id, uint256 amount) external onlyWhenUnlocked {
    Currency currency = CurrencyLibrary.fromId(id);
    _accountDelta(currency, amount.toInt128(), msg.sender);  // delta artır
    _burnFrom(from, currency.toId(), amount);                 // claim yak
}
```

---

## 7. PoolManager Fonksiyonları

### 7.1 `unlock(bytes data)` — Flash Accounting Kapısı

```solidity
function unlock(bytes calldata data) external returns (bytes memory result) {
    if (Lock.isUnlocked()) AlreadyUnlocked.selector.revertWith();
    Lock.unlock();
    result = IUnlockCallback(msg.sender).unlockCallback(data);
    if (NonzeroDeltaCount.read() != 0) CurrencyNotSettled.selector.revertWith();
    Lock.lock();
}
```

**Kritik:** `Lock.isUnlocked()` false olduğunda unlock ediliyor (yani başlangıçta lock'lu). İç içe `unlock()` çağrısı `AlreadyUnlocked` hatası verir.

### 7.2 `initialize(PoolKey, sqrtPriceX96)` — Pool Açma

`onlyWhenUnlocked` modifier YOK — initialize unlock gerektirmez. Hook çağrıları: `beforeInitialize` + `afterInitialize`.

```
1. tickSpacing ve currency sıra kontrolü
2. hook.isValidHookAddress(fee) → Hook adresi 14-bit izin ile uyumlu mu?
3. lpFee = key.fee.getInitialLPFee()
4. hooks.beforeInitialize(key, sqrtPriceX96)
5. pool.initialize(sqrtPriceX96, lpFee)
6. emit Initialize(...)
7. hooks.afterInitialize(key, sqrtPriceX96, tick)
```

### 7.3 `modifyLiquidity(key, params, hookData)` — Likidite Değişikliği

```
hooks.beforeModifyLiquidity(key, params, hookData)

(principalDelta, feesAccrued) = pool.modifyLiquidity(...)
callerDelta = principalDelta + feesAccrued

emit ModifyLiquidity(...)

(callerDelta, hookDelta) = hooks.afterModifyLiquidity(key, params, callerDelta, feesAccrued, hookData)

if hookDelta != 0: _accountPoolBalanceDelta(key, hookDelta, hooks)
_accountPoolBalanceDelta(key, callerDelta, msg.sender)
```

**V3'ten fark:** Mint/burn ayrı fonksiyon yerine tek `modifyLiquidity` (liquidityDelta pozitif/negatif).

### 7.4 `swap(key, params, hookData)` — Swap

```
beforeSwapDelta = hooks.beforeSwap(key, params, hookData)
  → amountToSwap (hook değiştirebilir!)
  → lpFeeOverride (hook dinamik fee belirleyebilir)

swapDelta = _swap(pool, id, Pool.SwapParams{...}, inputCurrency)

(swapDelta, hookDelta) = hooks.afterSwap(key, params, swapDelta, hookData, beforeSwapDelta)

if hookDelta != 0: _accountPoolBalanceDelta(key, hookDelta, hooks)
_accountPoolBalanceDelta(key, swapDelta, msg.sender)
```

**Kritik:** `beforeSwap` hook'u `amountToSwap`'ı değiştirebilir veya sıfırlayabilir (RETURNS_DELTA flag'i varsa pool math'i bypass edebilir).

### 7.5 `donate(key, amount0, amount1, hookData)` — Doğrudan Fee Dağıtımı

```
pool.donate(amount0, amount1)
→ feeGrowthGlobal0X128 += amount0 × Q128 / liquidity
→ feeGrowthGlobal1X128 += amount1 × Q128 / liquidity
```

LP'lere swap ücreti olmaksızın doğrudan ödeme.

### 7.6 `take(currency, to, amount)` — Token Al

```solidity
function take(Currency currency, address to, uint256 amount) external onlyWhenUnlocked {
    unchecked {
        _accountDelta(currency, -(amount.toInt128()), msg.sender);
        currency.transfer(to, amount);
    }
}
```

Delta negatif → sonunda settle gerekmez (borç). Transfer anında gerçekleşir.

### 7.7 `settle()` — Token Öde

```solidity
function settle() external payable onlyWhenUnlocked returns (uint256 paid) {
    return _settle(msg.sender);
}

function _settle(address recipient) internal returns (uint256 paid) {
    Currency currency = CurrencyReserves.getSyncedCurrency();
    if (currency.isAddressZero()) {
        paid = msg.value;  // native ETH
    } else {
        uint256 reservesBefore = CurrencyReserves.getSyncedReserves();
        uint256 reservesNow = currency.balanceOfSelf();
        paid = reservesNow - reservesBefore;
        CurrencyReserves.resetCurrency();
    }
    _accountDelta(currency, paid.toInt128(), recipient);
}
```

**sync() önce çağrılmalı:** `CurrencyReserves.syncCurrencyAndReserves(currency, balance)` rezervleri kaydeder. Sonra token gönderilir. Sonra `settle()` farkı hesaplar.

### 7.8 `updateDynamicLPFee(key, newFee)` — Dinamik Fee Güncelleme

```solidity
function updateDynamicLPFee(PoolKey memory key, uint24 newDynamicLPFee) external {
    if (!key.fee.isDynamicFee() || msg.sender != address(key.hooks)) {
        UnauthorizedDynamicLPFeeUpdate.selector.revertWith();
    }
    newDynamicLPFee.validate();
    _pools[id].setLPFee(newDynamicLPFee);
}
```

Sadece hook contract'ı çağırabilir. `isDynamicFee()` = `fee & 0x800000 != 0` (sentinel bit).

---

## 8. _accountDelta — Delta Muhasebesi

```solidity
function _accountDelta(Currency currency, int128 delta, address target) internal {
    if (delta == 0) return;

    (int256 previous, int256 next) = currency.applyDelta(target, delta);

    if (next == 0) {
        NonzeroDeltaCount.decrement();    // Sıfıra geldi
    } else if (previous == 0) {
        NonzeroDeltaCount.increment();    // Sıfırdan geldi
    }
}
```

**Sayaç mantığı:** `nonzeroDeltaCount` = sıfır olmayan delta sayısı. Unlock biterken bu sıfır olmalı.

---

## 9. Güvenlik Analizi

### 9.1 Flash Loan Güvenliği

V4'te flash loan yerleşik — `take() + settle()` kombinasyonu. Güvence:
- `NonzeroDeltaCount` sıfırlanmadan `lock()` gerçekleşmez → geri ödeme zorunlu
- Transient storage tx sona erince otomatik temizlenir

### 9.2 Reentrancy

V3: `slot0.unlocked = false` (persistent storage) → her SSTORE ~5000 gas
V4: `tstore(IS_UNLOCKED_SLOT, false)` → ~100 gas (20x daha ucuz)

`AlreadyUnlocked` kontrolü: İç içe `unlock()` engellenir.

### 9.3 Delta Manipülasyonu

`CurrencyDelta` slot'u `keccak256(target ++ currency)` — collision için kriptografik preimage bulunmalı. Pratikte imkansız.

### 9.4 Hook Güven Sınırı

Hook'lar `callerDelta` değiştirebilir. `afterModifyLiquidity` hook'unda `hookDelta != 0` ise hook'un kendi delta'sı pool'a yansıtılır. Bu kasıtlı — hook kendi ödeme/tahsilat mantığını uygulayabilir.

---

## 10. Nova Shield Chain — Rust Implementasyon Notları

### 10.1 Nova Shield'de Flash Accounting Gerekli Mi?

Nova Shield native L1 DEX olduğundan EVM callback deseni yok. Ancak aynı prensibi uygulayabiliriz:

```rust
// Nova Shield'de "flash accounting" eşdeğeri
pub struct TransactionContext {
    pub deltas: HashMap<(AccountId, CurrencyId), i128>,
    pub nonzero_count: u32,
}

impl TransactionContext {
    pub fn account_delta(&mut self, currency: CurrencyId, account: AccountId, delta: i128) {
        let current = self.deltas.entry((account, currency)).or_insert(0);
        let prev = *current;
        *current += delta;
        let next = *current;
        if next == 0 && prev != 0 { self.nonzero_count -= 1; }
        if next != 0 && prev == 0 { self.nonzero_count += 1; }
    }

    pub fn finalize(&self) -> Result<(), DexError> {
        if self.nonzero_count != 0 {
            return Err(DexError::UnSettledDeltas);
        }
        Ok(())
    }
}
```

### 10.2 BalanceDelta → Rust Tuple

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceDelta {
    pub amount0: i128,
    pub amount1: i128,
}

impl Add for BalanceDelta {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        BalanceDelta {
            amount0: self.amount0.checked_add(other.amount0).expect("overflow"),
            amount1: self.amount1.checked_add(other.amount1).expect("overflow"),
        }
    }
}
```

### 10.3 PoolId → Nova Shield

```rust
use sha2::{Sha256, Digest};

pub struct PoolKey {
    pub currency0: CurrencyId,
    pub currency1: CurrencyId,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hook_id: Option<HookId>,
}

impl PoolKey {
    pub fn pool_id(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.currency0.as_bytes());
        hasher.update(self.currency1.as_bytes());
        hasher.update(self.fee.to_le_bytes());
        hasher.update(self.tick_spacing.to_le_bytes());
        if let Some(hook) = &self.hook_id {
            hasher.update(hook.as_bytes());
        }
        hasher.finalize().into()
    }
}
```

### 10.4 ERC-6909 Eşdeğeri

Nova Shield'de ERC-6909 yerine native multi-asset modülü kullanılacak. `claim_token` kavramı yerine `credit_balance`:

```rust
// pallet-assets benzeri yapı
pub fn mint_credit(
    who: &AccountId,
    currency: CurrencyId,
    amount: u128,
) -> DispatchResult {
    // ERC-6909 mint() eşdeğeri
    Assets::mint(currency, who, amount)
}
```

---

## Özet: 3 Kritik Bulgu

1. **Transient Storage ile Reentrancy Koruması:** V3'te `slot0.unlocked` persistent storage'da (5000 gas per op). V4'te `tstore/tload` 100 gas. Bu 50x tasarruf — Nova Shield'de transaction context struct'ı ile emüle edilecek.

2. **Flash Accounting Net Settlement:** V4'ün en kritik yeniliği. Çoklu işlemler arası token akışları nettleştirilir — sadece net bakiye transfer edilir. Multi-hop swap'larda V3'e göre ~N-1 transfer tasarrufu (N=hop sayısı).

3. **PoolId = keccak256(key, 0xa0):** 5 parametreli PoolKey'in 160-byte memory layout'u direkt hash'lenir. Hook adresi de dahil olduğundan aynı token çifti farklı hook'larla farklı pool'dur. Nova Shield'de SHA-256 ile aynı pattern.
