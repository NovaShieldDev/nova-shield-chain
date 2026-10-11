# UV4-02: Uniswap V4 Hooks Sistemi + Dinamik Fee + Permission Bitmap

**Analiz Tarihi:** 2026-07-22
**Kaynak Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/dex/uniswap-v4-core/`
**Kapsanan Dosyalar:**
- `src/libraries/Hooks.sol` (~350 satır — tam analiz)
- `src/libraries/LPFeeLibrary.sol` (79 satır)
- `src/interfaces/IHooks.sol`
- `src/types/BeforeSwapDelta.sol`

---

## 1. Hook Permission Bitmap — Adres İçine Gömülü İzinler

### 1.1 Tasarım Felsefesi

V4'te hook izinleri hook contract'ının deploy adresinin en düşük 14 bitine gömülür. Bu bit düzeni hook çağrısı sırasında runtime'da kontrol edilir.

```
Hook adresi: 0x0000000000000000000000000000000000002400
Binary:       ...0010 0100 0000 0000
                 ^    ^
                 |    |
                 |    AFTER_ADD_LIQUIDITY_FLAG (bit 10)
                 BEFORE_INITIALIZE_FLAG (bit 13)
```

### 1.2 14-Bit Permission Haritası

```solidity
uint160 internal constant ALL_HOOK_MASK = uint160((1 << 14) - 1);
// = 0x3FFF = binary: 11 1111 1111 1111

uint160 internal constant BEFORE_INITIALIZE_FLAG          = 1 << 13;  // 0x2000
uint160 internal constant AFTER_INITIALIZE_FLAG           = 1 << 12;  // 0x1000
uint160 internal constant BEFORE_ADD_LIQUIDITY_FLAG       = 1 << 11;  // 0x0800
uint160 internal constant AFTER_ADD_LIQUIDITY_FLAG        = 1 << 10;  // 0x0400
uint160 internal constant BEFORE_REMOVE_LIQUIDITY_FLAG    = 1 << 9;   // 0x0200
uint160 internal constant AFTER_REMOVE_LIQUIDITY_FLAG     = 1 << 8;   // 0x0100
uint160 internal constant BEFORE_SWAP_FLAG                = 1 << 7;   // 0x0080
uint160 internal constant AFTER_SWAP_FLAG                 = 1 << 6;   // 0x0040
uint160 internal constant BEFORE_DONATE_FLAG              = 1 << 5;   // 0x0020
uint160 internal constant AFTER_DONATE_FLAG               = 1 << 4;   // 0x0010
uint160 internal constant BEFORE_SWAP_RETURNS_DELTA_FLAG  = 1 << 3;   // 0x0008
uint160 internal constant AFTER_SWAP_RETURNS_DELTA_FLAG   = 1 << 2;   // 0x0004
uint160 internal constant AFTER_ADD_LIQUIDITY_RETURNS_DELTA_FLAG  = 1 << 1;  // 0x0002
uint160 internal constant AFTER_REMOVE_LIQUIDITY_RETURNS_DELTA_FLAG = 1 << 0;// 0x0001
```

**hasPermission kontrolü:**
```solidity
function hasPermission(IHooks self, uint160 flag) internal pure returns (bool) {
    return uint160(address(self)) & flag != 0;
}
```

### 1.3 Permission Bağımlılıkları (İzin Hiyerarşisi)

```
BEFORE_SWAP_RETURNS_DELTA_FLAG → BEFORE_SWAP_FLAG gerektirir
AFTER_SWAP_RETURNS_DELTA_FLAG  → AFTER_SWAP_FLAG gerektirir
AFTER_ADD_LIQUIDITY_RETURNS_DELTA_FLAG  → AFTER_ADD_LIQUIDITY_FLAG gerektirir
AFTER_REMOVE_LIQUIDITY_RETURNS_DELTA_FLAG → AFTER_REMOVE_LIQUIDITY_FLAG gerektirir
```

`isValidHookAddress()` bu bağımlılıkları doğrular:
```solidity
if (!self.hasPermission(BEFORE_SWAP_FLAG) && self.hasPermission(BEFORE_SWAP_RETURNS_DELTA_FLAG))
    return false;
```

RETURNS_DELTA flag'i varken ilgili action flag'i yoksa geçersiz.

### 1.4 Adrese Gömme: CREATE2 ile

Hook deployer'ları CREATE2 ve salt mining kullanarak istenen bit pattern'ını elde eder:

```solidity
// HookMiner benzeri tool:
bytes32 salt = findSalt(
    creationCode,
    requiredFlags: BEFORE_SWAP_FLAG | AFTER_SWAP_FLAG | BEFORE_SWAP_RETURNS_DELTA_FLAG
);
// Salt mining: istenilen 14 bit pattern'ına sahip adres bulunana kadar salt denenr
```

---

## 2. Hook Fonksiyonları Tam Analiz

### 2.1 beforeSwap

```solidity
function beforeSwap(
    IHooks self,
    PoolKey memory key,
    SwapParams memory params,
    bytes calldata hookData
) internal returns (int256 amountToSwap, BeforeSwapDelta hookReturn, uint24 lpFeeOverride) {
    amountToSwap = params.amountSpecified;

    if (self.hasPermission(BEFORE_SWAP_FLAG)) {
        bytes memory result = callHook(
            self, abi.encodeCall(IHooks.beforeSwap, (msg.sender, key, params, hookData))
        );
        // result = [selector (4 bytes)] [BeforeSwapDelta (32 bytes)] [lpFeeOverride (32 bytes)]

        if (self.hasPermission(BEFORE_SWAP_RETURNS_DELTA_FLAG)) {
            hookReturn = BeforeSwapDelta.wrap(result.parseReturnDelta());
            // hookReturn'in specified delta'sı amountToSwap'tan düşülür
            int128 hookDeltaSpecified = hookReturn.getSpecifiedDelta();

            if (hookDeltaSpecified != 0) {
                bool exactInput = amountToSwap > 0;
                amountToSwap += hookDeltaSpecified;
                if (exactInput ? amountToSwap <= 0 : amountToSwap >= 0) {
                    HookDeltaExceedsSwapAmount.selector.revertWith();
                }
            }
        }
        // lpFeeOverride parsed from result
    }
}
```

**Kritik:** `BEFORE_SWAP_RETURNS_DELTA_FLAG` varsa hook swap miktarını değiştirebilir.
Örnek: Hook `hookDeltaSpecified = -50` döndürürse `amountToSwap += -50` → swap miktarı 50 azalır.

**ExactIn → ExactOut dönüşümü engeli:**
```
exactInput = true, amountToSwap = 100
hookDeltaSpecified = -150
amountToSwap = 100 + (-150) = -50  → negatif! exactOut'a dönüştü → HookDeltaExceedsSwapAmount
```

### 2.2 afterSwap

```solidity
function afterSwap(
    IHooks self, PoolKey memory key, SwapParams memory params,
    BalanceDelta swapDelta, bytes calldata hookData, BeforeSwapDelta beforeSwapDelta
) internal returns (BalanceDelta, BalanceDelta hookDelta) {
    if (self.hasPermission(AFTER_SWAP_FLAG)) {
        // hookDelta = delta hook kendi ödeyecek/alacak (RETURNS_DELTA flag'i varsa)
        int256 hookReturn = callHookWithReturnDelta(
            self,
            abi.encodeCall(IHooks.afterSwap, (msg.sender, key, params, swapDelta, hookData)),
            self.hasPermission(AFTER_SWAP_RETURNS_DELTA_FLAG)
        );

        if (hookReturn != 0) {
            hookDelta = (params.zeroForOne == (hookReturn > 0))
                ? toBalanceDelta(hookReturn.toInt128(), 0)
                : toBalanceDelta(0, hookReturn.toInt128());
            swapDelta = swapDelta - hookDelta;  // kullanıcı delta'sından hook delta'sı çıkarılır
        }
    }
    return (swapDelta, hookDelta);
}
```

**Delta yönü:** `zeroForOne == (hookReturn > 0)` koşulu hook'un hangi token'da delta istediğini belirler.

### 2.3 noSelfCall Modifier

```solidity
modifier noSelfCall(IHooks self) {
    if (msg.sender != address(self)) {
        _;
    }
}
```

Hook kendi tetiklediği bir işlemi çağırırsa (circular call) `noSelfCall` engeller. Hook callback'te tekrar aynı pool işlemi çağırırsa kendisinin hook'u tekrar tetiklenmez.

### 2.4 callHook — Güvenli Dış Çağrı

```solidity
function callHook(IHooks self, bytes memory data) internal returns (bytes memory result) {
    bool success;
    assembly ("memory-safe") {
        success := call(gas(), self, 0, add(data, 0x20), mload(data), 0, 0)
    }
    if (!success) CustomRevert.bubbleUpAndRevertWith(address(self), bytes4(data), HookCallFailed.selector);

    // returndata'yı result'a kopyala
    // ...

    // Selector kontrolü: hook doğru selector döndürmeli
    if (result.length < 32 || result.parseSelector() != data.parseSelector()) {
        InvalidHookResponse.selector.revertWith();
    }
}
```

**Hook return value zorunluluğu:** Her hook kendi function selector'ını döndürmeli. Yanlış selector veya sıfır uzunluk → `InvalidHookResponse`.

Bu pattern ERC-721 onERC721Received benzeri — "ack" mekanizması.

---

## 3. BeforeSwapDelta — Hook'un Swap Miktarına Müdahalesi

### 3.1 Struct

```solidity
// BeforeSwapDelta: Two int128 values packed into int256
// Upper 128 bits: specified currency delta (swap miktarını etkiler)
// Lower 128 bits: unspecified currency delta (diğer token)
type BeforeSwapDelta is int256;
```

**Specified:** Kullanıcının belirttiği token (exactIn'de input, exactOut'da output)
**Unspecified:** Diğer token

### 3.2 Hook Pool Math'i Bypass Ediyor

Eğer `BEFORE_SWAP_RETURNS_DELTA_FLAG` set ise ve hook `specifiedDelta = amountSpecified` döndürürse:
```
amountToSwap += specifiedDelta = amountToSwap + (-amountToSwap) = 0
```

`amountToSwap = 0` ile pool.swap() çağrılırsa `SwapAmountCannotBeZero` revert eder... ama kontrol var:

```
Pool.swap() skipped if amountToSwap == 0 (implementation detail)
```

Yani hook tüm swap'ı kendi yönetebilir — pool AMM matematiğini tamamen atlayabilir. Custom market maker implementasyonu için.

---

## 4. LPFeeLibrary — Dinamik ve Statik Fee Sistemi

### 4.1 Fee Sabitleri

```solidity
uint24 public constant DYNAMIC_FEE_FLAG = 0x800000;  // bit 23 (en yüksek bit)
uint24 public constant OVERRIDE_FEE_FLAG = 0x400000; // bit 22
uint24 public constant REMOVE_OVERRIDE_MASK = 0xBFFFFF; // ~0x400000
uint24 public constant MAX_LP_FEE = 1000000;  // 100% = 1,000,000 bps
```

**Fee format:** 1 bps = 1/1,000,000 (milyonda bir)
- 0.01% = 100
- 0.05% = 500
- 0.3%  = 3000
- 1%    = 10000
- 100%  = 1,000,000

### 4.2 Dinamik Fee Sentinel

```solidity
function isDynamicFee(uint24 self) internal pure returns (bool) {
    return self == DYNAMIC_FEE_FLAG;  // 0x800000 tam eşit
}
```

`0x800000` = 8,388,608 > `MAX_LP_FEE` (1,000,000) — geçersiz statik fee. Sentinel olarak kullanılır.

### 4.3 İki Dinamik Fee Mekanizması

**Mekanizma 1: Storage'da güncelleme**
```solidity
// PoolManager.updateDynamicLPFee(key, newFee) — sadece hook çağırabilir
pool.setLPFee(newFee);
```
Pool state'te kalıcı olarak güncellenir. Sonraki tüm swap'larda geçerli.

**Mekanizma 2: Swap bazlı override**
```solidity
// beforeSwap hook'u lpFeeOverride döndürür
uint24 lpFeeOverride = result.parseFee();
if (lpFeeOverride.isOverride()) {
    // Bu swap için özel fee kullan
    lpFee = lpFeeOverride.removeOverrideFlagAndValidate();
}
```
`OVERRIDE_FEE_FLAG (0x400000)` set ise bu swap için farklı fee. Kalıcı değil.

### 4.4 Hook'un Fee Döndürme Koşulu

```
Sadece dinamik fee pool'unda (fee == DYNAMIC_FEE_FLAG) hook fee override dönebilir.
Statik fee pool'unda lpFeeOverride yok sayılır.
```

---

## 5. Hook Akış Diyagramı — Swap Örneği

```
Kullanıcı swap() çağırır
        |
        v
[BEFORE_SWAP_FLAG kontrol]
   Evet → hooks.beforeSwap(key, params, hookData)
              → amountToSwap değişebilir
              → lpFeeOverride döner (dinamik fee ise)
              → BeforeSwapDelta döner (RETURNS_DELTA ise)
        |
        v
pool.swap(amountToSwap, lpFeeOverride)
              → swapDelta hesaplanır
        |
        v
emit Swap(...)
        |
        v
[AFTER_SWAP_FLAG kontrol]
   Evet → hooks.afterSwap(key, params, swapDelta, hookData, beforeSwapDelta)
              → hookDelta döner (RETURNS_DELTA ise)
              → swapDelta = swapDelta - hookDelta
              → hookDelta ayrıca hook adresine yansıtılır
        |
        v
_accountPoolBalanceDelta(key, hookDelta, hooks)   // Hook'un delta'sı
_accountPoolBalanceDelta(key, swapDelta, msg.sender)  // Kullanıcının delta'sı
```

---

## 6. validateHookPermissions — Hook Deploy Doğrulama

```solidity
function validateHookPermissions(IHooks self, Permissions memory permissions) internal pure {
    if (
        permissions.beforeInitialize != self.hasPermission(BEFORE_INITIALIZE_FLAG)
        // ... 14 koşul paralel kontrol
    ) {
        HookAddressNotValid.selector.revertWith(address(self));
    }
}
```

Hook constructor'da çağrılır. Deploy sırasında belirlenen adresin istenen permission'ları taşıdığı doğrulanır.

---

## 7. Hook Güvenlik Modeli

### 7.1 Hook'un Yapabileceği Şeyler

| Yetenek | Flag | Etki |
|---------|------|------|
| Swap'ı durdur | BEFORE_SWAP_FLAG | revert fırlatarak swap'ı iptal et |
| Swap miktarını değiştir | BEFORE_SWAP_RETURNS_DELTA_FLAG | amountToSwap'ı azalt/artır |
| Pool math'i bypass et | BEFORE_SWAP_RETURNS_DELTA_FLAG | amountToSwap=0 yap (tüm swap'ı yönet) |
| Swap sonrası delta al | AFTER_SWAP_RETURNS_DELTA_FLAG | hookDelta al (ek ücret/ödeme) |
| Dinamik fee | AFTER_SWAP + updateDynamicLPFee | Kalıcı fee güncelle |
| Per-swap fee | BEFORE_SWAP + OVERRIDE_FEE_FLAG | O swap için fee belirle |

### 7.2 Hook'un YAPAMAYACAĞI Şeyler

- Pool state'ini doğrudan okuyamaz (PoolManager internal)
- Delta'yı negatife çeviremez (net çıkarım yapamaz, sadece azaltabilir)
- Başka kullanıcı adına delta kaydedemez
- PoolManager.unlock() çağırırsa (iç içe) `AlreadyUnlocked` revert

### 7.3 Güven Modeli

Hook tamamen güvenilmezdir. Kullanıcı hangi hook'u desteklediğini pool açılırken seçer (PoolKey'de). Sonra değiştirilemez. Kullanıcı bu hook'u bilerek kullanmıştır.

**noSelfCall:** Hook kendi callback'inden tekrar işlem yaparsa kendi hook'unu tetiklemez. Sonsuz döngü engellenir.

---

## 8. Hooks Tasarım Örnekleri (Nova Shield İçin İlham)

### 8.1 Dynamic Fee Hook

Volatilite bazlı fee:
```
volatility_score = calculate_volatility(last_n_prices)
lpFeeOverride = base_fee * (1 + volatility_score)
OVERRIDE_FEE_FLAG set → beforeSwap'ta döndür
```

### 8.2 TWAMM Hook (Time-Weighted AMM)

Büyük emirleri zaman boyunca dağıt:
```
beforeSwap:
  execute_virtual_orders() → biriken TWAMM emirlerini execute et
  adjust amountToSwap
afterSwap:
  record execution
```

### 8.3 Limit Order Hook

Tick'e bağlı limit emirleri:
```
afterSwap:
  check if any limit orders triggered
  if tick crossed limit:
    fill limit order (modifyLiquidity ile)
```

---

## 9. Nova Shield Chain — Rust Hook Mimarisi

### 9.1 Hook Trait Tanımı

```rust
#[async_trait]
pub trait Hook: Send + Sync {
    fn permissions(&self) -> HookPermissions;

    async fn before_swap(
        &self,
        pool_key: &PoolKey,
        params: &SwapParams,
        data: &[u8],
    ) -> Result<BeforeSwapResult, HookError>;

    async fn after_swap(
        &self,
        pool_key: &PoolKey,
        params: &SwapParams,
        swap_delta: BalanceDelta,
        data: &[u8],
    ) -> Result<AfterSwapResult, HookError>;
    // ... diğer hook metodları
}

pub struct HookPermissions {
    pub before_initialize: bool,
    pub after_initialize: bool,
    pub before_add_liquidity: bool,
    pub after_add_liquidity: bool,
    pub before_remove_liquidity: bool,
    pub after_remove_liquidity: bool,
    pub before_swap: bool,
    pub after_swap: bool,
    pub before_donate: bool,
    pub after_donate: bool,
    pub before_swap_returns_delta: bool,
    pub after_swap_returns_delta: bool,
    pub after_add_liquidity_returns_delta: bool,
    pub after_remove_liquidity_returns_delta: bool,
}
```

### 9.2 Hook Permission Bitmap (Nova Shield'de Adres Yerine ID)

EVM'de adrese gömme zorunluluğu yok. Nova Shield'de hook ID'si ayrı:

```rust
pub struct HookId(pub [u8; 32]);

pub struct PoolKey {
    pub currency0: CurrencyId,
    pub currency1: CurrencyId,
    pub fee: FeeConfig,  // Statik veya dinamik
    pub tick_spacing: i32,
    pub hook_id: Option<HookId>,
}

pub enum FeeConfig {
    Static(u32),
    Dynamic,
}
```

Hook kayıt:
```rust
pub struct HookRegistry {
    hooks: HashMap<HookId, Arc<dyn Hook>>,
}

impl HookRegistry {
    pub fn register(&mut self, hook: Arc<dyn Hook>) -> HookId {
        let id = HookId(hash_hook(hook.permissions()));
        self.hooks.insert(id, hook);
        id
    }
}
```

### 9.3 Dinamik Fee → Nova Shield

```rust
pub struct PoolState {
    pub lp_fee: AtomicU32,  // Atomic güncelleştirme (concurrent swap'larda)
    // ...
}

impl PoolState {
    pub fn update_dynamic_lp_fee(&self, new_fee: u32, authorized_hook: HookId) -> Result<()> {
        // Sadece bu pool'un hook'u çağırabilir
        if self.hook_id != Some(authorized_hook) {
            return Err(DexError::Unauthorized);
        }
        if new_fee > MAX_LP_FEE {
            return Err(DexError::FeeTooLarge);
        }
        self.lp_fee.store(new_fee, Ordering::SeqCst);
        Ok(())
    }
}
```

---

## Özet: 3 Kritik Bulgu

1. **14-bit Adrese Gömülü Permission:** Hook izinleri CREATE2 salt mining ile hook contract adresine gömülür. Runtime'da `uint160(address) & flag` ile kontrol edilir — sıfır gas overhead. Nova Shield'de adres yoksa HookId içinde bitmap olarak saklanacak.

2. **beforeSwap RETURNS_DELTA ile Pool Math Bypass:** `BEFORE_SWAP_RETURNS_DELTA_FLAG` set hook, `amountToSwap`'ı sıfıra düşürerek AMM matematiğini tamamen atlayabilir. Bu custom market maker (CLMM, DLMM) implementasyonuna izin verir. Nova Shield'de isteğe bağlı native hook sistemi bu prensibi takip eder.

3. **İki Dinamik Fee Mekanizması:** (1) `updateDynamicLPFee` — kalıcı pool fee değişimi, yalnızca hook yetkili. (2) `OVERRIDE_FEE_FLAG` — tek swap bazlı fee, `beforeSwap` döndürür. Nova Shield'de her ikisi de desteklenecek — özellikle volatilite bazlı adaptif fee için.
