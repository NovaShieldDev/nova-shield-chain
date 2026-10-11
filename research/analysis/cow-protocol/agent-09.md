# CoW Protocol Analiz — Agent 09
## Kapsam: contracts/ — Settlement Smart Contracts
## DEX Ozel Protokolu: 3x Hassasiyet

**Analiz Tarihi:** 2026-07-22
**Toplam Dosya:** 19 Solidity (.sol) + 4 Rust (.rs) = 23 dosya
**Toplam Satir:** ~948 satir Solidity, ~1800 satir Rust (codegen/vendor/main)

---

## Dizin Yapisi

```
contracts/
  solidity/               # Kaynak Solidity kontratlar
    AnyoneAuthenticator.sol     (8 satir)
    Balances.sol                (151 satir)
    Signatures.sol              (91 satir)
    Solver.sol                  (165 satir)
    Spardose.sol                (24 satir)
    Swapper.sol                 (116 satir)
    interfaces/
      IERC1271.sol              (10 satir)
      IERC20.sol                (18 satir)
      IERC4626.sol              (8 satir)
      ISettlement.sol           (38 satir)
      IStorageAccessible.sol    (7 satir)
      IVaultRelayer.sol         (16 satir)
      IVault.sol                (10 satir)
    libraries/
      Caller.sol                (42 satir)
      Math.sol                  (13 satir)
      SafeERC20.sol             (25 satir)
    tests/
      Counter.sol               (24 satir)
      GasHog.sol                (27 satir)
      MockERC4626Wrapper.sol    (54 satir)
      NonStandardERC20Balances.sol (47 satir)
      RemoteERC20Balances.sol   (22 satir)
  script/
    DeployBalances.s.sol        (16 satir)
    DeploySignatures.s.sol      (16 satir)
  src/
    main.rs                     (563 satir) — codegen orchestration
    codegen.rs                  (455 satir) — Rust binding ureteci
    networks.rs                 (13 satir)  — chain ID sabitleri
    vendor.rs                   (347 satir) — artifact indirme sistemi
  generated/
    contracts-facade/src/lib.rs
    contracts-generated/<contract>/src/lib.rs  (75+ uretilmis crate)
```

**Onemli Not:** Bu `contracts/` repo'su v1.0.0 ana kontrat kaynak kodunu (GPv2Settlement.sol, GPv2AllowListAuthentication.sol, GPv2VaultRelayer.sol) *icermez*. Bunlar ayri `@cowprotocol/contracts@1.1.2` npm paketinden *artifact olarak* indirilir (vendor.rs). Bu repo'nun asil kapsami:
1. Simulasyon/gas-olcum yardimci kontratlar (Solver, Swapper, Balances, Signatures)
2. Rust binding kod ureteci (src/)
3. Harici kontratlar icin oluretilmis Rust bindinglari (generated/)

---

## BOLUM 1: ARAYUZ DOSYALARI (interfaces/)

### [ISettlement.sol] (38 satir)

**Amac:** GPv2Settlement ana kontratinin Solidity arayuzu — settle() imzasini tanimlar.

**Sabit:**
```solidity
ISettlement constant SETTLEMENT = ISettlement(0x9008D19f58AAbD9eD0D60971565AA8510560ab41);
```
Bu adres TUM desteklenen aglarda ayni! (Mainnet, Gnosis, Sepolia, Arbitrum, Base, Avalanche, BNB, Optimism, Polygon, Linea, Plasma, Ink)
Bu, CREATE2 ile deterministik deployment'in somut kanitidur.

**Struct Tanimlari:**

```solidity
struct Interaction {
    address target;    // cagrilacak kontrat
    uint256 value;     // ETH miktari
    bytes callData;    // fonksiyon cagrisi verisi
}
```

```solidity
struct Trade {
    uint256 sellTokenIndex;   // tokens[] dizisindeki sat-token indeksi
    uint256 buyTokenIndex;    // tokens[] dizisindeki al-token indeksi
    address receiver;         // alim tokenlarini alacak adres (0x0 = trader)
    uint256 sellAmount;       // maksimum satilacak miktar (order limiti)
    uint256 buyAmount;        // minimum alinacak miktar (order limiti)
    uint32 validTo;           // siparis gecerlilik suresi (Unix timestamp)
    bytes32 appData;          // uygulama meta verisi hash'i
    uint256 feeAmount;        // ucret miktari (sat tokeninda)
    uint256 flags;            // bit-pack: order tipi, imza tipi, balance kaynagi
    uint256 executedAmount;   // solve'un belirledigI gercek miktar (fill-or-kill icin 0)
    bytes signature;          // EIP-712, EIP-1271, veya on-chain pre-sign
}
```

**Interface Fonksiyonlari:**
- `domainSeparator()` → EIP-712 domain separator
- `authenticator()` → solver whitelist kontrat adresi
- `vaultRelayer()` → Balancer vault relayer adresi
- `setPreSignature(bytes orderUid, bool signed)` → on-chain siparis imzalama
- `settle(address[] tokens, uint256[] clearingPrices, Trade[] trades, Interaction[][3] interactions)` → ANA SETTLEMENT FONKSIYONU

**settle() Parametreleri — Matematiksel Analiz:**
- `tokens[]`: settlement'ta kullanilan tum tokenlarin siralanmis listesi
- `clearingPrices[]`: her token icin clearing fiyati. `tokens[i]` icin `clearingPrices[i]`. Fiyat birimi: "tokens[i] cinsinden 1 birimin diger token cinsinden degeri"
- `trades[]`: her Trade, clearing price vektorune indeks uzerinden referans verir
- `interactions[0]`: pre-interactions (settlement oncesi: approve, wrap ETH, vb.)
- `interactions[1]`: main interactions (DEX swap calldata)
- `interactions[2]`: post-interactions (settlement sonrasi: unwrap, transfer, vb.)

**Matematiksel Formul — Clearing Price:**
Eger `tokens = [USDC, WETH]` ve `clearingPrices = [1500, 1]` ise:
- 1 USDC = 1500/1 birim
- 1 WETH = 1/1500 birim (tersi)
- Trade i icin: `buyAmount_executed = sellAmount_executed * clearingPrices[sellIndex] / clearingPrices[buyIndex]`
- Uniform clearing: TUM trades ayni fiyat vektorunden yararlanir

**DEX Uygulama Notu — Nova Shield:**
- `settle()` imzasinin L1 protokol katmanina birebir tercumesi:
  - `tokens[]` → Nova Shield'da settlement block'undaki token listesi
  - `clearingPrices[]` → validator'lar tarafindan mutabik olunan on-chain fiyat vektoru (Tendermint consensus ile)
  - `Trade[]` → siparis batch'i, her siparis StarkDeFi-tipi bir commitment ile imzalanmis
  - `Interaction[]` → Nova Shield'da protokol seviyesi hook'lar (native atomik transfer, wrapped asset swap)

---

### [IVaultRelayer.sol] (16 satir)

**Amac:** GPv2VaultRelayer kontratinin arayuzu — token transferlerini Balancer vault'undan gerceklestirir.

**Sabit:**
```solidity
IVaultRelayer constant VAULT_RELAYER = IVaultRelayer(0xC92E8bdf79f0507f65a392b0ab4667716BFE0110);
```

**Struct:**
```solidity
struct Transfer {
    address account;   // token sahibinin adresi
    address token;     // ERC-20 token adresi
    uint256 amount;    // transfer edilecek miktar
    bytes32 balance;   // keccak256("erc20") | keccak256("external") | keccak256("internal")
}
```

**Tek Fonksiyon:**
```solidity
function transferFromAccounts(Transfer[] calldata transfers) external;
```

**Balance Kaynak Tipleri:**
1. `keccak256("erc20")`: Standart ERC-20 transferFrom — kullanicinin ERC-20 allowance'i VaultRelayer'a verilmis olmali
2. `keccak256("external")`: Balancer Vault harici dengeyi kullanir — vault relayer'a Balancer Vault uzerinden yetki verilmis olmali
3. `keccak256("internal")`: Balancer Vault dahili dengeyi kullanir — gas-verimli, vault token transfer yapmaz

**Guvenlik Analizi:**
- transferFromAccounts yalnizca GPv2Settlement tarafindan cagrilabilir (VaultRelayer'in kendi icerisindeki access control)
- Balances.sol ve Solver.sol'da `require(target != address(contracts.vaultRelayer), "GPv2: forbidden interaction")` — interaction'larin dogrudan VaultRelayer'i cagramamasini garantiler

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield'da VaultRelayer kavraminin L1 karsitigi: **Validator-controlled transfer engine**
- L1'de token transferi = UTXO tukumu veya account state guncellemesi, EVM call degil
- 3 kaynak tipi Nova Shield'da: "native" (L1 account), "wrapped" (cross-chain pozisyon), "locked" (stake-icin kilitlenmis)

---

### [IVault.sol] (10 satir)

**Amac:** Balancer V2 Vault'unun minimal arayuzu — iki sorgulama fonksiyonu.

**Sabit:**
```solidity
IVault constant VAULT = IVault(0xBA12222222228d8Ba445958a75a0704d566BF2C8);
```
Yine: TUM aglarda ayni adres. CREATE2 kanitlanmis.

**Fonksiyonlar:**
- `hasApprovedRelayer(address user, address relayer)` → bool: kullanicinin vault relayer'i yetkilendirip yetkilendirmedigi
- `getInternalBalance(address user, address[] tokens)` → uint256[]: Balancer internal balance sorgusu

**DEX Uygulama Notu — Nova Shield:**
- Balancer ihtiyaci yok. Nova Shield L1 native token transferi yapar.
- Bunun karsitigi: Nova Shield consensus layer'da "approved relayers" kavrami yerine **validator set tarafindan onaylanan settlement executor** kavramini kullanmali.

---

### [IERC20.sol] (18 satir)

**Amac:** Standart ERC-20 + native ETH wrapper arayuzu.

**IERC20 interface:**
- `balanceOf(address)` → uint256
- `allowance(address owner, address spender)` → uint256
- `approve(address, uint256)` → bool
- `transfer(address, uint256)` → bool
- `transferFrom(address, address, uint256)` → bool

**INativeERC20 extends IERC20:**
- `withdraw(uint256)` — WETH → ETH
- `deposit() payable` — ETH → WETH

**Guvenlik Notu:** `transfer()` ve `transferFrom()` bool donuyor — CoW Protocol'un kendi SafeERC20 katmani bunu kontrol eder (bkz: SafeERC20.sol). USDT gibi bool donmeyen tokenlar icin kritik.

---

### [IERC1271.sol] (10 satir)

**Amac:** EIP-1271 smart contract imza dogrulama standardi.

```solidity
bytes4 constant ERC1271_MAGICVALUE = 0x1626ba7e;

interface IERC1271 {
    function isValidSignature(bytes32 hash, bytes memory signature)
        external view returns (bytes4 magicValue);
}
```

**DEX Onem:** CoW Protocol EIP-1271'i kullanarak:
1. Gnosis Safe gibi multisig cuzdan siparisleri — imza = Safe'in "isValidSignature" cagrisinin dogrulugu
2. Composable CoW siparisleri — conditional order'lar
3. Swapper.sol: `isValidSignature` herzaman `0x1626ba7e` doner → simulasyon icin
4. GasHog.sol: `isValidSignature` icinde hesaplama yogumlugu simule eder

---

### [IERC4626.sol] (8 satir)

**Amac:** EIP-4626 tokenized vault standart arayuzu.

```solidity
interface IERC4626 {
    function asset() external view returns (address assetTokenAddress);
    function convertToAssets(uint256 shares) external view returns (uint256 assets);
}
```

**Kullanim:** Price oracle sistemi — vault tokenlarinin (stETH, cUSDC) gercek deger hesaplamasi icin.
- `asset()`: hangi temel tokena karsilik geldigini doner
- `convertToAssets()`: N adet vault hissesinin kac temel token ettigini hesaplar
- MockERC4626Wrapper.sol bunu test icin taklit eder: `shares * rateNumerator / rateDenominator`

**DEX Uygulama Notu — Nova Shield:**
- Vault token fiyatlandirmasi Nova Shield DEX'te kritik olacak (staking turevleri, yield tokenlar)
- `convertToAssets()` formulu L1 native hesap: `oracle_price * shares / denominator`
- Ozyineleme destegi gerekli: MockERC4626Wrapper gosteriyor ki vault'un asset'i bir baska vault olabilir (nested vaults)

---

### [IStorageAccessible.sol] (7 satir)

```solidity
interface IStorageAccessible {
    function simulateDelegatecall(address reader, bytes memory call)
        external returns (bytes memory result);
}
```

**Amac:** GPv2Settlement'in storage'ini harici kontrat kodu calistirarak okumak — on-chain simulation altyapisi. Driver servisi bu arayuzu kullaniyor.

---

## BOLUM 2: KUTUPHANE DOSYALARI (libraries/)

### [Caller.sol] (42 satir)

**Amac:** Dusuk seviyeli assembly call wrapper'lari — gas olcumu ve hata yayilimi.

**Fonksiyonlar:**

```solidity
function doCall(address self, bytes memory cdata) internal returns (bytes memory rdata)
```
Standart call, revert icin hata yayilimiyla.

```solidity
function doCall(address self, uint256 value, bytes memory cdata) internal returns (bytes memory rdata)
```
ETH degeri gonderen call, hata yayilimiyla.

```solidity
function doDelegatecall(address self, bytes memory cdata) internal returns (bytes memory rdata)
```
Delegatecall sarici — IStorageAccessible.simulateDelegatecall icin.

```solidity
function doMeteredCallNoReturn(address self, bytes memory cdata) internal returns (uint256 gasUsed)
function doMeteredCallNoReturn(address self, uint256 value, bytes memory cdata) internal returns (uint256 gasUsed)
```
**Bu fonksiyon kritik:** Gas olcumlu call. Assembly seviyesinde:
```
gasUsed = gasleft();
call(gas(), self, value, add(cdata, 32), mload(cdata), 0, 0)  // 0 output boyutu = geri deger okunmuyor
gasUsed -= gasleft();
```
Revert durumunda: `returndatacopy(0, 0, returndatasize())` + `revert(0, returndatasize())` — hata mesajini yukariya tasir.

**Guvenlik Analizi:**
- Assembly'de `call()` kullanilmasi: reentrancy riski yok cunku call'a gelen donus verisi okunmuyor (0 output size) ve fonksiyon degerinden hemen gas olcuyor
- Return degeri yok, yalnizca gas olcumu — saldirganin donen veriden bir sey yapmasi mumkun degil
- Hata yayilimi dogru: if iszero(call(...)) → revert

**DEX Uygulama Notu — Nova Shield:**
- `doMeteredCallNoReturn` benzeri bir gas olcum mekanizmasi Nova Shield settlement katmaninda L1 native kodda uygulanabilir: `remaining_gas_before - remaining_gas_after`
- Rust'ta: `std::time::Instant` veya validator'a ozel compute_units sayaci

---

### [Math.sol] (13 satir)

**Amac:** Tip donusumu ve basit kontrol yardimcilari.

```solidity
function toInt(uint256 self) internal pure returns (int256) {
    require(self <= uint256(type(int256).max), "Math: int256 overflow");
    return int256(self);
}
```
**Onem:** uint256 → int256 donusumunde overflow guard. CoW Protocol zaman zaman signed aritmetik kullanir (net transfer hesabi: pozitif = alinacak, negatif = gonderilecek).

```solidity
function check(bytes memory self, string memory message) internal pure {
    require(self.length == 0 || abi.decode(self, (bool)), message);
}
```
Non-standard ERC-20 return degerlerini kontrol eder: bos bytes (bazi eski tokenlar bool dondurmez) veya `true` kabul edilir, `false` revert eder.

**DEX Uygulama Notu — Nova Shield:**
- `toInt` benzeri overflow korumalari Rust'ta otomatik: `u256::checked_sub`, `i256::try_from(u256)` ile hata yonetimi
- Non-standard token return degeri sorunu Nova Shield icin gecerli degil — protokol seviyesi transfer, ERC-20 interfacesine bagli degil

---

### [SafeERC20.sol] (25 satir)

**Amac:** Non-standard ERC-20 token uyumlulugu — bool donmeyen tokenlar icin (USDT, DAI gibi).

```solidity
function safeTransfer(IERC20 self, address target, uint256 amount) internal {
    bytes memory cdata = abi.encodeCall(self.transfer, (target, amount));
    bytes memory rdata = address(self).doCall(cdata);
    require(check(rdata), "SafeERC20: transfer failed");
}

function safeApprove(IERC20 self, address target, uint256 amount) internal {
    bytes memory cdata = abi.encodeCall(self.approve, (target, amount));
    bytes memory rdata = address(self).doCall(cdata);
    require(check(rdata), "SafeERC20: approval failed");
}

function check(bytes memory rdata) internal pure returns (bool ok) {
    return rdata.length == 0 || abi.decode(rdata, (bool));
}
```

**Guvenlik Analizi:**
- `check()` fonksiyonu: bos bytes (non-standard, yani void return) veya `true` kabul eder
- Bu OpenZeppelin SafeERC20'den daha minimal bir uygulama
- USDT oncesi approve sifirlanmamis ise: Swapper.sol bunu ele aliyor (once approve(0) sonra approve(amount))
- Reentrancy: transfer/approve cagrilari harici kontrata gidiyor — ama CoW'da token transferleri her zaman settlement'dan VaultRelayer'e delege ediliyor, dogrudan settlement'dan token cagirmak guvenli cunku state mutation once yapiliyor

**Matematiksel Formul:**
- Bos returndata: `rdata.length == 0` → bazi tokenlar sadece `return;` yapar (void), bu basarili kabul edilir
- Bool decode: `abi.decode(rdata, (bool))` → basarili transfer = true

---

## BOLUM 3: ANA SIMULASYON KONTRATLARI

### [Solver.sol] (165 satir)

**Amac:** Solver hesabini taklit eden simulasyon kontrati — driver'in settlement simulasyonu icin.

**Custom Storage Layout (satirlar 19):**
```solidity
contract Solver layout at 0x14f5b2c185fc03c75c787d1f0e10ea137cc6d235a0047448eff18c9a173a722b {
```
Bu `layout at` Solidity ^0.8.30 ozelligi — storage slot cakismasini onlemek icin keccak256 ofset. Klasik `mapping(uint256 => ...)` saldirilarini engeller.

**Storage Degiskenleri:**
- `uint256 private _simulationOverhead` — settlement disindaki gas maliyeti duzeltmesi
- `uint256[] private _queriedBalances` — simulasyon sirasinda okunan bakiyeler

**Sabit:**
```solidity
address private constant PIGGY_BANK = 0x1111111111111111111111111111111111111111;
```
Test simulasyonlarinda trader'a token saglamak icin kullanilan sahte adres (trade_verifier.rs ile eslesmeli).

**Fonksiyonlar — Tam Analiz:**

#### `swap()` (satir 46-78) — ANA SIMULASYON AKISI

```
swap(settlementContract, tokens, receiver, trader, sellToken, sellAmount, settleCallTarget, settlementCall)
    → (gasUsed, queriedBalances)
```

Adim adim:
1. `ensureTradePreconditions(...)` — trader yetkilendirmesini ve bakiyesini kontrol et
2. `receiver.call{value: 0}("")` — ETH almak icin smart contract receiver'i isit (storage warm-up). Revert edilse bile devam et (safeCall degil, result ignore ediliyor)
3. `_storeSettlementBalances(tokens, settlementContract)` — **oncesi** bakiyeleri kaydet
4. `gasUsed = _executeAndMeasure(settleCallTarget, settlementCall)` — settlement'i calistir ve gasi olc
5. `_storeSettlementBalances(tokens, settlementContract)` — **sonrasi** bakiyeleri kaydet
6. `return (gasUsed, _queriedBalances)` — onceki + sonraki bakiyeler diziyle doner

**Matematiksel Amac:**
- `queriedBalances[0..n]` = settlement oncesi her token bakiyesi
- `queriedBalances[n..2n]` = settlement sonrasi her token bakiyesi
- `queriedBalances[i+n] - queriedBalances[i]` = net token akisi — solver'in profitabilite kontrolu icin

#### `_executeAndMeasure()` (satir 82-100) — GAS OLCUM

```solidity
function _executeAndMeasure(address settleCallTarget, bytes calldata settlementCall)
    internal returns (uint256 gasUsed) {
    uint256 gasStart = gasleft();
    bytes memory settlementCallMem = settlementCall;  // calldata → memory kopyasi
    assembly {
        if iszero(call(gas(), settleCallTarget, 0, add(settlementCallMem, 32),
                  mload(settlementCallMem), 0, 0)) {
            returndatacopy(0, 0, returndatasize())
            revert(0, returndatasize())
        }
    }
    gasUsed = gasStart - gasleft() - _simulationOverhead;
}
```

**Kritik Detay:** calldata → memory kopyasi gas farkini doguruyor. Gercek transaction'da calldata dogrudan referans edilir ama simulasyonda memory'e kopyalanmak zorunda. Bu fark `_simulationOverhead` ile duzeltiyor.

**Matematiksel Formul — Gas Olcumu:**
```
gasUsed = gasStart - gasAfterSettlement - _simulationOverhead
```
`_simulationOverhead`, `storeBalance()` cagrilarindaki gas maliyetini telafi eder.

#### `ensureTradePreconditions()` (satir 109-136)

```solidity
function ensureTradePreconditions(ISettlement settlementContract, address trader,
                                   address sellToken, uint256 sellAmount) internal {
    address vaultRelayer = settlementContract.vaultRelayer();
    uint256 allowance = IERC20(sellToken).allowance(trader, vaultRelayer);
    require(allowance >= sellAmount, "trader did not give the required approvals");

    uint256 sellBalance = IERC20(sellToken).balanceOf(trader);
    if (sellBalance < sellAmount) {
        try Spardose(PIGGY_BANK).requestFunds(trader, sellToken, sellAmount - sellBalance) {}
        catch {
            revert("trader does not have enough sell token");
        }
    }
}
```

**Kontrol Sirasi:**
1. Trader'in VaultRelayer'a yeterli allowance verip vermedigi — vermemisse revert ("trader did not give the required approvals")
2. Trader'in yeterli sellToken bakiyesi var mi — yoksa PIGGY_BANK'ten tamamla
3. PIGGY_BANK transfer basarisiz olursa — revert ("trader does not have enough sell token")

**Guvenlik Analizi:**
- PIGGY_BANK adresi sabit — state override ile bu adrese token yüklenebilir (simulasyon ortaminda)
- `try-catch` dogru kullanimi: PIGGY_BANK'in revert etmesi bir sonraki adimda handle ediliyor
- Production'da PIGGY_BANK hic kullanilmaz (simulasyon only)

#### `storeBalance()` (satir 144-155)

```solidity
function storeBalance(address token, address owner, bool countGas) external {
    uint256 gasStart = gasleft();
    _queriedBalances.push(
        token == 0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE
            ? owner.balance
            : IERC20(token).balanceOf(owner)
    );
    if (countGas) {
        _simulationOverhead += gasStart - gasleft() + 4460;
    }
}
```

`0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE` — ETH icin magic adres (ERC-20 olmayan native token). Bu adres goruldugunde `owner.balance` kullaniliyor, aksi halde `IERC20(token).balanceOf(owner)`.

`4460` hardcoded gas sabitidir — fonksiyon call overhead'ini telafi eder.

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield L1'de ETH magic adres gerekmiyor — native token zaten L1'in ulusal tokeni
- Bakiye sorgu mekanizmasi Nova Shield settlement'inda: validator'lar pre-state ve post-state root'u karsilastirarak net akisi dogruluyor
- Gas sayaci analogi: Nova Shield'da "compute_units" — settlement'in kac hesaplama birimi harcadigini olc

**Guvenlik Analizi — Solver.sol butunu:**
- Reentrancy: `storeBalance()` external — settlement'dan storeBalance'i cagirmak mumkun, ama _queriedBalances array'i salt push ediliyor, critical state mutation yok
- Access control: hicbir fonksiyon `onlyOwner` veya access modifieri gerektirmiyor — bu bir simulasyon kontrati, production'da solver address'ine deploy ediliyor, dolayisiyla caller dogrulamasina gerek yok (zaten solver adresi oldugu dogrulanmis)
- Custom storage layout: slot cakismasini onler, proxy pattern veya harici EVM state override ile guvende

---

### [Swapper.sol] (116 satir)

**Amac:** Tekli DEX takas simulasyonu — gas maliyeti dogrulama icin.

**Struct Tanimlari (satir 9-17):**
```solidity
struct Asset {
    address token;
    uint256 amount;
}

struct Allowance {
    address spender;
    uint256 amount;
}
```

**Ana Fonksiyon: `swap()` (satir 37-108)**

```solidity
function swap(
    ISettlement settlement,
    Asset calldata sell,       // satilacak token ve miktar
    Asset calldata buy,        // alinacak token ve miktar
    Allowance calldata allowance,  // settlement'in approve etmesi gereken DEX spender
    Interaction[] calldata calls   // DEX swap calldata'si (uniswap, balancer, vb.)
) external returns (uint256 gasUsed)
```

**Adim adim akis:**

1. **Bakiye kontrolu (satir 46-53):**
```solidity
if (IERC20(sell.token).balanceOf(address(this)) < sell.amount) {
    return 0;  // hooks ile alinan token simule edilemiyor, heuristik kullan
}
```
Hook kullanan trade'lerde swapper yeterli bakiye tutamaz → 0 donerek "heuristik tahmin kullan" sinyali verir.

2. **USDT uyumlu cift approval (satir 55-60):**
```solidity
IERC20(sell.token).safeApprove(address(settlement.vaultRelayer()), 0);
IERC20(sell.token).safeApprove(address(settlement.vaultRelayer()), sell.amount);
```
USDT sorununu cozer: once sifirla, sonra yeni miktar ver.

3. **Clearing price vektoru yapimi (satir 62-68):**
```solidity
address[] memory tokens = new address[](2);
tokens[0] = sell.token;
tokens[1] = buy.token;

uint256[] memory clearingPrices = new uint256[](2);
clearingPrices[0] = buy.amount;   // sell tokenin clearing price'i = buy amount
clearingPrices[1] = sell.amount;  // buy tokenin clearing price'i = sell amount
```

**Bu kritik bir matematiksel detaydir:**
```
clearingPrice[sellToken] = buyAmount
clearingPrice[buyToken]  = sellAmount

executed_buy = executed_sell * clearingPrice[sell] / clearingPrice[buy]
             = executed_sell * buyAmount / sellAmount
```
Yani: kac sell token verilirse o kadar buy token alinir. Uniform clearing price = tam dolum.

4. **Trade struct yapimi (satir 70-87):**
```solidity
trades[0] = Trade({
    sellTokenIndex: 0,
    buyTokenIndex: 1,
    receiver: address(0),         // varsayilan = trader
    sellAmount: sell.amount,
    buyAmount: buy.amount,
    validTo: type(uint32).max,    // hic expire etme
    appData: bytes32(0),
    feeAmount: 0,                 // ucret yok (simulasyon)
    flags: 0x40,                  // EIP-1271 imza tipi
    executedAmount: 0,            // 0 = fill-or-kill, settlement tam dolum yapar
    signature: abi.encodePacked(address(this))  // Swapper adresi = imzaci
});
```
`flags: 0x40` = bit 6 set = EIP-1271 imzasi kullan → settlement `isValidSignature()` cagirir → Swapper.sol'un `isValidSignature` her zaman true doner.

5. **Pre-interaction: DEX spender icin approve (satir 89-99):**
```solidity
if (IERC20(sell.token).allowance(address(settlement), allowance.spender) < allowance.amount) {
    interactions[0] = new Interaction[](1);
    interactions[0][0].target = sell.token;
    interactions[0][0].callData = abi.encodeCall(
        IERC20(sell.token).approve,
        (allowance.spender, allowance.amount)
    );
}
```
Settlement kontrati adina DEX router icin approve yarat. Gercek settlement sirasinda settlement kontrati kendi bakiyesini DEX'e kullandiracak.

6. **Main interaction + settlement cagirisi (satir 100-107):**
```solidity
interactions[1] = calls;  // DEX swap calldata

gasUsed = address(settlement).doMeteredCallNoReturn(
    abi.encodeCall(settlement.settle, (tokens, clearingPrices, trades, interactions))
);
```
Gas olcumlu settlement cagrisi.

**EIP-1271 imza simulasyonu:**
```solidity
function isValidSignature(bytes32, bytes calldata) external pure returns (bytes4) {
    return 0x1626ba7e;  // her zaman gecerli
}
```
Swapper her imzayi kabul eder — bu simulasyon icin, production'da gercek imza dogrulamasi gerekir.

**Matematiksel Ozet — Clearing Price Kullanimi:**
```
tokens[0] = sell_token, tokens[1] = buy_token
clearingPrices = [buy_amount, sell_amount]

executedBuyAmount = executedSellAmount * clearingPrices[0] / clearingPrices[1]
                  = executedSellAmount * buy_amount / sell_amount
```
fill-or-kill (executedAmount=0): settlement `sellAmount` kadar satar, `buyAmount` kadar alir — tam dolum garantisi.

**Guvenlik Analizi:**
- `isValidSignature` herzaman true: SIMULASYON ICIN KASITLI — bu kontrat production settlement'a asla deploy edilmez, sadece simulation state override ile kullanilir
- Pre-interaction'da VaultRelayer cagrisina karsi koruma: settlement'in `executeInteractions` fonksiyonu bunu bloklar
- Integer overflow: Solidity ^0.8 otomatik overflow korumalari

**DEX Uygulama Notu — Nova Shield:**
- Swapper'in clearing price hesabini Nova Shield'da dogrudan uygula:
  ```
  clearing_price_native[token_i] = solver_tarafindan_gelen_birim_fiyat
  surplus = executed_buy - (limit_buy * executed_sell / limit_sell)
  ```
- Gas simulasyonu Nova Shield'da: transaction inclusion oncelikle "compute_units" tahminiyle yapilir
- Solver 0 dondugunde (hook desteklemeyen): Nova Shield'da bu "insufficient liquidity" koduna esitlenir

---

### [Balances.sol] (151 satir)

**Amac:** Settlement oncesi trader bakiye ve transfer edilebilirlik kontrolu.

**Struct:**
```solidity
struct Contracts {
    ISettlement settlement;
    IVaultRelayer vaultRelayer;
    IVault vault;
}
```

**Ana Fonksiyon: `balance()` (satir 43-106)**

```solidity
function balance(
    Contracts memory contracts,
    address trader,
    IERC20 token,
    uint256 amount,          // 0 = tam efektif bakiyeyi transfer et
    bytes32 source,          // keccak256("erc20" | "external" | "internal")
    Interaction[] calldata interactions
) external returns (
    uint256 tokenBalance,     // ham token bakiyesi
    uint256 allowance,        // efektif allowance
    uint256 effectiveBalance, // min(tokenBalance, allowance)
    bool canTransfer,         // transfer basarili olur mu?
    bytes memory transferRevertReason  // basarisiz ise hata nedeni
)
```

**Balance kaynak tipine gore ayri mantik:**

1. **`keccak256("erc20")`** — standart ERC-20:
```solidity
tokenBalance = token.balanceOf(trader);
allowance = token.allowance(trader, address(contracts.vaultRelayer));
```

2. **`keccak256("external")`** — Balancer vault harici:
```solidity
tokenBalance = token.balanceOf(trader);
allowance = contracts.vault.hasApprovedRelayer(trader, address(contracts.vaultRelayer))
    ? token.allowance(trader, address(contracts.vault))
    : 0;
```
Vault relayer yetkisi yoksa allowance sifir kabul edilir.

3. **`keccak256("internal")`** — Balancer vault dahili:
```solidity
tokenBalance = contracts.vault.getInternalBalance(trader, tokens)[0];
allowance = contracts.vault.hasApprovedRelayer(trader, address(contracts.vaultRelayer))
    ? type(uint256).max  // Internal balance'da sonsuz allowance
    : 0;
```
Internal balance'da ayri approve gerekmiyor, vault zaten tokenin sahibi.

**Efektif bakiye hesabi:**
```solidity
effectiveBalance = tokenBalance <= allowance ? tokenBalance : allowance;
```
Matematiksel: `effectiveBalance = min(tokenBalance, allowance)`

**Transfer edilebilirlik testi (satir 90-105):**
```solidity
transfers[0] = Transfer({
    account: trader,
    token: address(token),
    amount: amount != 0 ? amount : effectiveBalance,
    balance: source
});
try contracts.vaultRelayer.transferFromAccounts(transfers) {
    canTransfer = true;
} catch (bytes memory reason) {
    canTransfer = false;
    transferRevertReason = reason;
}
```
Gercek transfer simule edilir — token freeze, blacklist, pause gibi durumlar yakalanir.

**`executeInteractions()` (satir 112-149) — Settlement'tan Alinan Kod:**
```solidity
require(address(this) == address(contracts.settlement), "incorrect calling context");
```
Bu kontrat settlement adresinde `simulateDelegatecall` ile calistirilmali! `address(this)` settlement adresi olmali cunku `executeInteractions` bu sarti kontrol eder.

VaultRelayer kismasi (satir 126-129):
```solidity
require(target != address(contracts.vaultRelayer), "GPv2: forbidden interaction");
```

Assembly ile interaction cagrisi (satir 131-148):
```solidity
assembly {
    let freeMemoryPointer := mload(0x40)
    calldatacopy(freeMemoryPointer, callData.offset, callData.length)
    if iszero(call(gas(), target, value, freeMemoryPointer, callData.length, 0, 0)) {
        returndatacopy(0, 0, returndatasize())
        revert(0, returndatasize())
    }
}
```
`calldatacopy` = calldata'yi memory'e kopyala, `call` ile hedefi cagir, revert durumunda hata yayil.

**Guvenlik Analizi:**
- `address(this) == address(contracts.settlement)` kontrolu kritik: delegatecall olmadan bu fonksiyon calistirilirsa, this != settlement → revert
- VaultRelayer cagrisini engelleme: interaction'lar dogrudan VaultRelayer'i cagiramamali (settlement yapmadan token cekme)
- try-catch transfer testi: token'in `pause()` veya `blacklist()` durumunu bile yakalar
- Gas limiti: `call(gas(), ...)` — tum kalani interaction'a ver, settlement'in normal davranisi

**DEX Uygulama Notu — Nova Shield:**
- `balance()` fonksiyonunun L1 karsitigi: **pre-settlement state inspection**
- Nova Shield protokol katmaninda, settlement oncesi validator'lar:
  1. Trader'in L1 hesabinda yeterli sell token var mi?
  2. Settlement akillisi bu tokenlar uzerinde islem yapabilir mi? (stake lock, vesting, vb.)
  3. Transfer simulasyonu: "gercek transfer basar ise devam et"
- 3 kaynak tipi Nova Shield versiyonu: "account" (L1 balance), "cross-chain" (bridge pozisyon), "escrowed" (L1 lock contract)

---

### [Signatures.sol] (91 satir)

**Amac:** ERC-1271 imza gecerlilik simulasyonu — gas maliyetiyle birlikte.

**Struct:**
```solidity
struct Contracts {
    ISettlement settlement;
    IVaultRelayer vaultRelayer;
}
```

**Ana Fonksiyon: `validate()` (satir 27-46)**

```solidity
function validate(
    Contracts memory contracts,
    IERC1271 signer,           // imzalayan kontrat (Gnosis Safe, Composable CoW, vb.)
    bytes32 order,             // dogrulanacak order hash
    bytes calldata signature,  // imza verisi
    Interaction[] calldata interactions  // pre-interaction'lar (Composable CoW order kaydi gibi)
) external returns (uint256 gasUsed)
```

**Akis:**
1. `executeInteractions(contracts, interactions)` — once on-chain durumu hazirla
2. `gasUsed = gasleft()` — gas sayacini baslt
3. `bytes4 magicValue = signer.isValidSignature(order, signature)` — ERC-1271 cagrisi
4. `gasUsed = gasUsed - gasleft()` — gas hesapla
5. `require(magicValue == ERC1271_MAGICVALUE, "didn't say the magic word")` — gecerli mi kontrol et

**Gas olcumu nedeni:** GasHog.sol gosteriyor ki bazı ERC-1271 imzalari hesaplama yogunlugu gerektirebilir (1000-2000 keccak256 iterasyonu). Driver bu gas miktarini simule ederek siparis dahil etme karlililigi hesapliyor.

`executeInteractions()` — Balances.sol ile ayni implementasyon:
- `require(address(this) == address(contracts.settlement), "incorrect calling context")`
- VaultRelayer engelleme
- Assembly ile interaction cagrisi

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield'da ERC-1271 yok (L1, EVM yok)
- Karsitigi: **Nova Shield native signature validation** — secp256k1, ed25519, BLS12-381 (MPC imzalari icin)
- Gas olcumu yerine Nova Shield: "compute_units_for_signature_verification" — L1 transition fonksiyonunda sabit maliyet
- Composable siparisler (conditional orders) Nova Shield'da: on-chain state kosuluyla tetiklenen siparisler

---

### [AnyoneAuthenticator.sol] (8 satir)

**Amac:** Herkesin solver olabilecegi permissionsless bir authenticator — test ortamlari ve bazi deployment senaryolari icin.

```solidity
contract AnyoneAuthenticator {
    function isSolver(address) external pure returns (bool) {
        return true;
    }
}
```

**Kiyaslama — GPv2AllowListAuthentication (production):**
- Production'da `authenticator` kontrati bir whitelist tutar
- `isSolver(address)` caller'in beyaz listede olup olmadigini kontrol eder
- Nova Shield'in analiz yapabilmesi icin GPv2AllowListAuthentication'in tam implementasyonuna bakmak gerek (o kontrat `@cowprotocol/contracts` npm'de, burada yalnizca Rust binding)

**Deployment Adresi:**
- GPv2AllowListAuthentication: `0x2c4c28DDBdAc9C5E7055b4C863b72eA0149D8aFE` — TUM AGLARDA AYNI

**GPv2AllowListAuthentication Mekanizmasi (vendor.rs ve main.rs'ten dogrulanan bilgi):**
- npm `@cowprotocol/contracts@1.1.2` paketi
- Proxy pattern kullanilmis (vendor.rs: `_Implementation` suffix — proxy'nin implementation kontrati)
- `manager` = protokol yoneticisi, solver ekleyip cikarabilir
- `addSolver(address)` / `removeSolver(address)` — whitelist yonetimi
- `isSolver(address)` → bool

**DEX Uygulama Notu — Nova Shield:**
- AnyoneAuthenticator modeli: **testnet/devnet** icin. Her validator bir "solver" rolunde.
- Production Nova Shield'da 2 secenek:
  1. **Permissioned solver set** (allowlist) — validator onayiyla: stake min gerekenlerle, slashing ile
  2. **Permissionless solving** — AnyoneAuthenticator gibi, ancak MEV korumasiz
- ONERI: Stake-weighted permissioned set. 32 ETH karsitigi miktarda token stake edebilen herhangi bir katilimci solver olabilsin, slashing mekanizmasi: yanlis settlement → stake kaybı

---

### [Spardose.sol] (24 satir)

**Amac:** Test simulasyonlarinda trader'a token pre-fund eden "kumbarasi kontrat".

```solidity
contract Spardose {
    function requestFunds(address receiver, address token, uint256 amount) external {
        IERC20(token).safeTransfer(receiver, amount);
    }
}
```

**Nasil calisir:**
1. Simulasyon ortaminda `PIGGY_BANK = 0x1111...1111` adresine state override ile token yüklenir
2. Solver.sol'un `ensureTradePreconditions()` trader bakiyesi yetersizse `Spardose(PIGGY_BANK).requestFunds()` cagirir
3. Spardose token'i trader'a transfer eder → simulasyon devam eder

**Guvenlik Notu:** Access control yok — herkes token isteyebilir. Ama Spardose adresine token koymak icin zaten state override gerekiyor (sadece simulasyon ortaminda). Production'da bu adres bos.

---

## BOLUM 4: TEST KONTRATLARI (tests/)

### [Counter.sol] (24 satir)

**Amac:** Interaction'larin kac kez cagrildigini sayan test yardimcisi.

```solidity
contract Counter {
    mapping(string => uint256) public counters;

    function incrementCounter(string memory key) public {
        counters[key] += 1;
    }

    function setCounterToBalance(string memory key, address token, address owner) public {
        counters[key] = IERC20(token).balanceOf(owner);
    }
}
```

**Kullanim:** E2E testlerde interaction'in kac kez tetiklendigini dogrulama. `setCounterToBalance` settlement sonrasi token bakiyesini kaydetmek icin kullanilabilir.

---

### [GasHog.sol] (27 satir)

**Amac:** Gas-yogun ERC-1271 imzalarini test etmek icin — `Signatures.sol`'un gas olcumunu dogrular.

```solidity
contract GasHog {
    function isValidSignature(bytes32 order, bytes calldata signature)
        public view returns (bytes4) {
        uint start = gasleft();
        uint target = abi.decode(signature, (uint));  // hedef gas tukenmesi
        bytes32 hash = keccak256("go");
        while (start - gasleft() < target) {
            hash = keccak256(abi.encode(hash));       // keccak256 zincirleme
        }
        require(hash != order);  // derleyici optimizasyonu engelleme
        return 0x1626ba7e;
    }

    function approve(ERC20 token, address spender, uint amount) external {
        token.approve(spender, amount);
    }
}
```

**Matematiksel Notasyon:**
```
target = signature'dan decode edilen gas miktari
while (consumed_gas < target): keccak256 hesapla
```
Ornek: `target = 100000` → ~100k gas harcayan bir imza dogrulama simule edilir.

**approve fonksiyonu:** Test ortaminda token approval'larini kolaylastirmak icin.

---

### [MockERC4626Wrapper.sol] (54 satir)

**Amac:** Ozyinelemeli vault fiyatlandirmasi testleri icin sahte ERC-4626 vault.

**Storage:**
- `address public immutable asset` — temel token (veya baska bir vault)
- `uint8 public immutable decimals`
- `uint256 public immutable rateNumerator` ve `rateDenominator` — donusum orani

**Fonksiyonlar:**
- `convertToAssets(uint256 shares)` → `shares * rateNumerator / rateDenominator`
- `transfer()`, `approve()`, `transferFrom()` — standart ERC-20 uyumlulugu
- `mint(address to, uint256 amount)` — test oncesi token dagitimi (access control yok — test only)

**Ozyineleme testi senaryosu:**
```
MockERC4626Wrapper(asset=USDC, rate=2/1)   → 1 share = 2 USDC
MockERC4626Wrapper(asset=^yukardaki, rate=3/1) → 1 share = 3 * 2 USDC = 6 USDC (ozyinelemeli)
```

**Guvenlik Notu:** `mint()` herkes tarafindan cagrilabilir — kesinlikle test only. Production'da deployment yapilmasin.

---

### [NonStandardERC20Balances.sol] (47 satir)

**Amac:** Standart balance slot detection yontemlerini atlatan ERC-20 — non-standart storage layout testi.

**Storage Yapisi:**
```solidity
struct UserData {
    uint256 epoch;          // slot 0 — balance BU SLOT DEGIL
    mapping(address => uint256) approvals;  // slot 1 (mapping)
    uint256 balance;        // slot 2 — balance uzak slotta
}
mapping(address => UserData) users;
```

**Neden onemli:** Driver'in bakiye simülasyonu token'in balance'ini EVM storage slot'larini okuyarak yapar. Standart ERC-20'de `balanceOf(address)` = `keccak256(address, 0)`. Ama bu kontrat `balance`'i slot 2'de tutuyor → slot discovery mekanizmasini test etmek icin kullanilir.

`mint()` — `epoch = 1` setiyor, bu da balance slot ile epoch slot'un karismamasi icin test.

---

### [RemoteERC20Balances.sol] (22 satir)

**Amac:** Balance'i baska bir kontrat'tan delegated olarak okuyan test tokeni.

```solidity
contract RemoteERC20Balances is NonStandardERC20Balances {
    bool internal immutable isLocalBalance;
    NonStandardERC20Balances public immutable target;

    function balanceOf(address user) external view override returns (uint256) {
        uint256 otherBalanceOf = target.balanceOf(user);
        return isLocalBalance ? users[user].balance : otherBalanceOf;
    }
}
```

`isLocalBalance = true` → bu kontratin kendi bakiyesini doner
`isLocalBalance = false` → target'in bakiyesini doner

**Kullanim:** Bakiye baska bir kontrata "delege edilmis" tokenlari simule etmek — bazı wrapped tokenlar veya vault tokenlar bu desene sahip.

---

## BOLUM 5: DEPLOYMENT SCRIPTLERI (script/)

### [DeployBalances.s.sol] (16 satir)

```solidity
contract DeployBalances is Script {
    function run() public {
        vm.startBroadcast();
        new Balances{salt: ""}();
        vm.stopBroadcast();
    }
}
```

`{salt: ""}` — CREATE2 ile deterministic deployment. Bos salt kullanilarak TUM AGLARDA AYNI ADRES elde ediliyor:
- `0x88b4B74082BffB2976C306CB3f7E9093AE48B94F` (main.rs'ten dogrulanan)

### [DeploySignatures.s.sol] (16 satir)

Ayni pattern — `Signatures.sol` icin:
- `0x8262d639c38470F38d2eff15926F7071c28057Af` (main.rs'ten dogrulanan)

**DEX Uygulama Notu — Nova Shield:**
- Deterministic deployment kavrami Nova Shield'da: genesis blogun bir parcasi olarak kontrat/program adresleri belirlenir
- Ya da Solana Program ID'si gibi fixed-keypair deployment

---

## BOLUM 6: RUST KAYNAK DOSYALARI (src/)

### [networks.rs] (13 satir)

```rust
pub const MAINNET: u64 = 1;
pub const GNOSIS: u64 = 100;
pub const SEPOLIA: u64 = 11155111;
pub const ARBITRUM_ONE: u64 = 42161;
pub const BASE: u64 = 8453;
pub const POLYGON: u64 = 137;
pub const AVALANCHE: u64 = 43114;
pub const BNB: u64 = 56;
pub const OPTIMISM: u64 = 10;
pub const LINEA: u64 = 59144;
pub const PLASMA: u64 = 9745;
pub const INK: u64 = 57073;
```

**Desteklenen Aglar:** 12 ag. CoW Protocol'un ne kadar genis deploy edildigini gosteriyor. TUM aglarda settlement kontrat adresi ayni.

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield chain ID sisteminde kendi aglar icin sabitleri burada tanimla
- Cosmos SDK benseri: chain-id string olabilir (nova-shield-mainnet-1)

---

### [main.rs] (563 satir — build_module() dahil)

**Amac:** Rust binding kodu uretim orkestrasyonu.

**Komut satiri:**
```
contracts-generate vendor    # Harici ABIleri indir
contracts-generate generate  # Rust binding kodu olustur
contracts-generate all       # Ikisini birden yap
```

**build_module() — Kayitli Kontratlar (main.rs satir 92-563):**

Tum kayitli kontratlar ve adresleri:

| Kontrat | Adres (ornekler) | Notlar |
|---------|---------|--------|
| IZeroEx | 0xdef1c0de... | 8 ag |
| GPv2Settlement | 0x9008D19f... | 12 ag, DETERMINISTIK |
| GPv2AllowListAuthentication | 0x2c4c28DD... | 12 ag, DETERMINISTIK |
| BalancerV2Vault | 0xBA122222... | 10 ag, DETERMINISTIK |
| BalancerV3BatchRouter | 0x136f1EFc... | 8 ag |
| HooksTrampoline | 0x60Bf7823... | 12 ag, DETERMINISTIK |
| CoWSwapEthFlow | 0x40a50cf0... | 11 ag |
| FlashLoanRouter | 0x9da8b484... | 12 ag, DETERMINISTIK |
| Permit2 | 0x000000000022... | 11 ag |
| CowAmmConstantProductFactory | 0x40664207... | 3 ag (yeni) |
| LiquoriceSettlement | 0x04486... | Mainnet + Arbitrum (kapali kaynak) |

**Onemli Ek Kontratlar:**
- `Solver7702Delegate` — EIP-7702 (hesap soyutlama) solver delegasyonu — GitHub'dan artifact indirilmis
- `ChainalysisOracle` — AML/KYC oracle — kapali kaynak, manuel vendored
- `ICowWrapper` — cross-chain cow wrapper interface

---

### [codegen.rs] (455 satir)

**Amac:** JSON artifact'larini Rust Alloy binding koduna donusturen kod ureteci.

**Mimarisi:**
1. `Module` — en ust seviye koleksiyon (`build_module()` bunu doldurur)
2. `Submodule` — gruplanmis kontratlar (cow_amm, test, support)
3. `Contract` — tek bir kontrat (ad + ag adresleri)

**Paralel Uretim:**
```rust
.into_par_iter()
.map(|(contract, submodule)| {
    contract.write_crate(artifacts_dir, &contracts_generated_dir)?;
    ...
})
.collect::<Result<Vec<_>>>()?;
```
`rayon::prelude::*` ile paralel crate uretimi — buyuk kontrat listelerinde hizli.

**Her Crate Icindeki Uretilen Kod:**
1. ABI'dan Alloy `sol!` macro uretimi (alloy-sol-macro-expander kullanilarak)
2. `Instance = ContractInstance<DynProvider>` tip takma adi
3. Eger ag bilgisi varsa `deployment_info(chain_id) -> Option<(Address, Option<u64>)>` const fonksiyon
4. `deployment_address(chain_id)` ve `deployment_block(chain_id)` yardimcilari
5. `Instance::deployed(provider)` async factory — otomatik network detection

**Prettyplease + syn formatlamasi:**
```rust
let file: syn::File = syn::parse_file(&buffer)?;
let formatted = prettyplease::unparse(&file);
std::fs::write(src_dir.join("lib.rs"), formatted)?;
```
Uretilen kod okunabilir formatlanmis Rust kodu — elle duzenlenebilir standartlarda.

**NetworkArm `ToTokens` impl:**
```rust
#chain_id => Some((::alloy_primitives::address!(#address), #block_number))
```
match arm olarak uretilir — runtime haritasi degil compile-time sabit.

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield icin benzer bir codegen sistemi: `.abi.json` yerine Rust struct tanimlari
- Alloy yerine: `ethabi` veya Nova Shield native ABI format
- Deployment bilgisi Nova Shield'da genesis.json'dan okunabilir

---

### [vendor.rs] (347 satir)

**Amac:** Harici kontrat ABI ve bytecode'larini internet'ten indirip artifacts/ klasorune kaydeden sistem.

**Iki Mod:**
1. `vendor.full()` — ABI + bytecode: test deployment icin (GPv2Settlement, GnosisSafe, Uniswap vb.)
2. `vendor.abi_only()` — yalnizca ABI: factory tarafindan olusturulan kontratlar (UniswapV2Pair vb.)

**Kaynak Tipleri:**
- `Src::npm(path)` → `https://unpkg.com/{path}` (NPM CDN)
- `Src::github(path)` → `https://raw.githubusercontent.com/{path}` (GitHub raw)
- `vendor.manual(name, reason)` → sadece log, dosya yok (kapali kaynak kontratlar)

**Ozellik Secimi (properties array):**
```rust
properties: &[
    ("abi", "abi,compilerOutput.abi"),   // abi veya compilerOutput.abi
    ("devdoc", "devdoc,compilerOutput.devdoc"),
    ("userdoc", "userdoc"),
    ("bytecode", "bytecode"),           // full icin
],
```
`retrieve_value_from_path()` ile JSON path traversal: `"compilerOutput.abi"` → `json["compilerOutput"]["abi"]`.

**Manuel Vendored Kontratlar:**
- `HooksTrampoline` — manuel ABI ve bytecode
- `LiquoriceSettlement` — kapali kaynak, sadece ABI
- `ERC1271SignatureValidator` — manuel ABI
- `IERC4626` — manuel ABI
- `BalancerV2BasePool/Factory` — Balancer ABI yayinlamiyor
- `ChainalysisOracle` — kapali kaynak

**DEX Uygulama Notu — Nova Shield:**
- Nova Shield'da vendor.rs benzeri bir sistem: genesis kontratlarini CI/CD'de indir
- Alternatif: tum ABI'lari repo'ya commit et (go-ethereum yaklasimi)

---

## BOLUM 7: URETILMIS BINDINGLER (generated/)

### [contracts-facade/src/lib.rs]

Tum kontrat cratelarini tek bir `contracts` crate'i altinda yeniden export eden facade:

```rust
pub use cow_contract_gpv2settlement as GPv2Settlement;
pub use cow_contract_gpv2allowlistauthentication as GPv2AllowListAuthentication;
// ... diger kontratlar
pub mod cow_amm {
    pub use cow_contract_cowamm as CowAmm;
    // ...
}
pub mod support {
    pub use cow_contract_solver as Solver;
    pub use cow_contract_swapper as Swapper;
    // ...
}
```

Kullanim: `use contracts::GPv2Settlement;` veya `contracts::support::Solver::...`

### Uretilmis Crate Sayisi: 75+

Her crate:
- `contracts-generated/<lowercase_name>/Cargo.toml` — bagimsiz crate
- `contracts-generated/<lowercase_name>/src/lib.rs` — uretilmis Alloy binding

---

## BOLUM 8: GENEL MIMARININ COZI UMLENMESI

### Settlement Akisi — Tam Resim

```
Kullanici
  → (EIP-712 veya EIP-1271 ile imzalanmis siparis)
  → Orderbook API

Autopilot
  → Siparis batch'ini solver'lara gonderir
  → Her ~12-15 saniyede bir auction

Solver (harici veya dahili)
  → Clearing price vektoru hesaplar
  → DEX swap calldata'si hazirlar (main interactions)
  → settle() calldata'si olusturur

Driver
  → Swapper.sol ile gas simule eder
  → Balances.sol ile trader bakiyelerini kontrol eder
  → Signatures.sol ile ERC-1271 imzalari dogrular
  → Solver.sol ile tam settlement simule eder
  → En iyi cozumu secer ve zincire gonderir

GPv2Settlement.settle()
  ┌─────────────────────────────────────────────┐
  │ 1. authenticator.isSolver(msg.sender) kontrol │
  │ 2. interactions[0] (pre) calistir            │
  │ 3. VaultRelayer.transferFromAccounts()       │
  │    (sat tokenlarini settlement'a cek)        │
  │ 4. interactions[1] (main) calistir           │
  │    (DEX swap'lari)                           │
  │ 5. Her trade icin clearing price uygula      │
  │    (al tokenlari dagit)                      │
  │ 6. interactions[2] (post) calistir           │
  └─────────────────────────────────────────────┘
```

### Clearing Price Matematigi — Tam Analiz

```
tokens = [T0, T1, T2]
clearingPrices = [P0, P1, P2]

Trade i: T[sellIdx] → T[buyIdx]
  executedBuy = executedSell * P[sellIdx] / P[buyIdx]

Surplus:
  limit_rate = order.buyAmount / order.sellAmount
  clearing_rate = P[sellIdx] / P[buyIdx]
  surplus_pct = clearing_rate / limit_rate - 1
  surplus_token = (clearing_rate - limit_rate) * executedSell

Ornek:
  order: 100 USDC → min 0.05 ETH (limit rate: 0.05/100 = 0.0005 ETH/USDC)
  clearing: P[USDC]=3000, P[ETH]=1
  clearing_rate = 3000/1 * (ETH/USDC) inverted = 1/3000 ETH/USDC
  Wait: clearing rate for "sell USDC, buy ETH":
    executedBuy = executedSell * P[USDC] / P[ETH] = 100 * 3000 / 1 ??

  Bu yanlis. Duzeltme:
  clearingPrices birimini anlayalim: P[i] = token i'nin fiyati (ortak birim, ornegin USD)
  P[USDC] = 1 (USD), P[ETH] = 3000 (USD) deil
  Veya daha dogru: P[i], trade icin normalize edilmis birim fiyati

  Gercek CoW formulu (GPv2Settlement.sol'dan — bu repo'da yok ama bilinen implementasyondan):
  executedBuyAmount = executedSellAmount * clearingPrices[sellTokenIndex] / clearingPrices[buyTokenIndex]

  Eger clearingPrices[USDC] = 1, clearingPrices[ETH] = 2000:
  executedBuy = 100 USDC * 1 / 2000 = 0.05 ETH ✓ (tam limit fiyattan)

  Eger clearingPrices[USDC] = 1, clearingPrices[ETH] = 1800 (ETH ucuzdur):
  executedBuy = 100 * 1 / 1800 = 0.0556 ETH > 0.05 ETH = SURPLUS ✓

Surplus formulu:
  surplus = executedBuy - order.buyAmount * (executedSell / order.sellAmount)
          = executedSell * P[sell] / P[buy] - order.buyAmount * executedSell / order.sellAmount
          = executedSell * (P[sell]/P[buy] - order.buyAmount/order.sellAmount)
```

**Swapper.sol bunu dogrular:**
```solidity
clearingPrices[0] = buy.amount;   // P[sell_token] = buy_amount
clearingPrices[1] = sell.amount;  // P[buy_token] = sell_amount
executedBuy = executedSell * buy.amount / sell.amount  // tam dolum, sifir surplus
```

---

## BOLUM 9: KAPSAMLI GUVENLIK ANALIZI

### Reentrancy

**Solver.sol:** `storeBalance()` external, ama yalnizca `_queriedBalances.push()` yapiyor. Reentrancy tehlikesi yok — callstack'te kritik state değişikliği yok.

**Balances.sol / Signatures.sol:** `executeInteractions()` harici kontratlari cagirir. Reentrancy mumkun, ama:
- Bu kontratlar settlement adresinde `simulateDelegatecall` ile calistirilir
- Interaction'lar VaultRelayer'i cagiramazlar
- Settlement storage'i degistirme etkileri delegatecall context ile sinirli

**Swapper.sol:** `doMeteredCallNoReturn` ile settlement'i cagirir. Settlement icerisinde reentrancy mumkun ama settlement bunu kendi reentrancy guard'i ile koruyor (production GPv2Settlement'da `nonReentrant`).

### Integer Overflow / Underflow

Tum dosyalar `pragma solidity ^0.8.x` — Solidity 0.8+ otomatik overflow/underflow korumalari. Math.sol'daki `toInt()` ek kontrol ekliyor.

### Access Control

- AnyoneAuthenticator: kasitli olarak access control yok (test/dev)
- Spardose: access control yok — state override ile kullaniliyor
- Solver: access control yok — solver adresi zaten authenticate edilmis
- Balances, Signatures: address(this) == settlement kontrolu (delegatecall korumasi)
- Swapper: `isValidSignature` herzaman true — simulasyon only

### MEV Korumaslari

- Batch settlement: MEV korumasinin ana mekanizmasi — CoW (Coincidence of Wants) araciligiyla
- Uniform clearing price: tum kullanicilar en iyi fiyattan yararlanir, frontrunner sadece clearing price'i degistirebilir, bireysel siparisi degil
- Solver competition: birden fazla solver teklif verir → en fazla surplus ureten kazanir → searcher MEV'ini azaltir

**Clearing price CEX-DEX Arbitraj Korumaslari:**
- Solver'in clearing price'i piyasa fiyatindan uzak olmamali (circuit breaker)
- Solver bonding / slashing ile manipulasyon riski dusuruluyor

---

## BOLUM 10: NOVA SHIELD DEX — DIREKTIF EYLEM PLANI

### 1. L1 Settlement Mimarisi

**Problem:** EVM smart contract yok. Settlement protocol katmaninda olmali.

**Cozum:**
```
Nova Shield Settlement Transaction:
  - type: "BatchSettlement"
  - clearing_prices: Vec<(TokenId, U256)>
  - trades: Vec<TradeExecution>
  - interactions: Vec<ProtocolHook>  // pre/main/post
  - solver_signature: MPC_Signature   // threshold imza
  - proof: ZK_Proof (opsiyonel)
```

**Validator dogrulamasi:**
1. `clearing_prices` mantikli mi? (her tokenin fiyati >= 0, normalized sum)
2. Her trade icin: `executed_buy >= limit_buy * executed_sell / limit_sell` (limit iyilestirme)
3. Net token akisi dengeli mi? (settlement kontrati artik fazla token tutmasin)
4. Solver yetkili mi? (stake kontrol)

### 2. Clearing Price Konsepti L1'de

```rust
pub struct ClearingPrice {
    pub token: TokenId,
    pub price: U256,  // normalize edilmis birim (10^18 = 1.0)
}

pub fn execute_buy_amount(
    executed_sell: U256,
    sell_token_price: U256,
    buy_token_price: U256,
) -> U256 {
    executed_sell
        .checked_mul(sell_token_price)
        .and_then(|x| x.checked_div(buy_token_price))
        .expect("price division")
}

pub fn surplus(
    executed_sell: U256,
    executed_buy: U256,
    limit_sell: U256,
    limit_buy: U256,
) -> U256 {
    let min_buy = limit_buy
        .checked_mul(executed_sell)
        .and_then(|x| x.checked_div(limit_sell))
        .unwrap_or(U256::MAX);
    executed_buy.saturating_sub(min_buy)
}
```

### 3. Solver Secimi ve Stake Gerekliligi

CoW Protocol whitelist modeli → Nova Shield hibrit model:

```
Tier 1: Validator Solvers (varsayilan)
  - Her validator bir solver; stake = consensus stake
  - Settlement governance icin on-chain oy

Tier 2: Registered External Solvers
  - Min stake: 10,000 NSC (Nova Shield Coin)
  - Slashing: yanlis settlement → %5-10 stake kaybi
  - Bond: her auction'da 1000 NSC bond (kayip halinde yakilir)
  - Kayit: governance oyu ile veya otomatik (AML/KYC oracle gecerse)

Tier 3: Permissionless Mode (testnet)
  - AnyoneAuthenticator gibi: herkes solver
  - MEV korumasiz
```

### 4. MEV Korumaslari — Native L1 Tasarimi

```
Batch Commit-Reveal:
  1. Solver → encrypted batch commitment (Hash(batch)) → zincire yaz
  2. 2 blok sonra → solver batch'i acikla
  3. Validators → tum aciklanan batch'ler arasinda en iyi surplus ureten sec
  4. Settlement blogu olustur

Avantaj: frontrunner encrypted hash'i gorup batch'i tahmin edemez
Dezavantaj: +2 blok latency
```

Alternatif: **Verifiable Secret Sharing** ile threshold encryption — belirli bir validator esigine ulasilmadan batch acilmaz (Penumbra yaklasimi).

### 5. Balance Kaynak Sistemi — Nova Shield Versiyonu

```rust
pub enum BalanceSource {
    Native,    // L1 hesap bakiyesi
    Wrapped,   // cross-chain bridge pozisyon
    Escrowed,  // L1 lock contract (stake vb.)
    Internal,  // settlement katmaninda dahili bakiye (Balancer gibi)
}

pub fn effective_balance(
    source: BalanceSource,
    trader: Address,
    token: TokenId,
    state: &ChainState,
) -> U256 {
    match source {
        BalanceSource::Native => state.balance(trader, token),
        BalanceSource::Wrapped => state.bridge_balance(trader, token),
        BalanceSource::Escrowed => state.locked_balance(trader, token),
        BalanceSource::Internal => state.internal_balance(trader, token),
    }
    .min(state.allowance(trader, token, &SETTLEMENT_PROGRAM))
}
```

### 6. Gas Olcumu Analogu

CoW'da `doMeteredCallNoReturn` ile gas olcumu → Nova Shield'da:

```rust
pub struct ExecutionMeter {
    compute_units_start: u64,
    compute_units_limit: u64,
}

impl ExecutionMeter {
    pub fn measure_settlement(&self, settlement: &BatchSettlement) -> u64 {
        let before = self.compute_units_start;
        // settlement state transition uygula
        let after = compute_units_remaining();
        before - after
    }
}
```

Solver, settlement'in ne kadar compute unit harcadigini bilerek transaction fee teklifini belirler.

---

## OZET TABLOSU

| Dosya | Satir | Tur | Kritiklik |
|-------|-------|-----|-----------|
| ISettlement.sol | 38 | Interface | KRITIK — settle() imzasi |
| IVaultRelayer.sol | 16 | Interface | KRITIK — token transfer |
| IVault.sol | 10 | Interface | Yuksek — Balancer entegrasyonu |
| IERC20.sol | 18 | Interface | Yuksek — token standardi |
| IERC1271.sol | 10 | Interface | Yuksek — imza dogrulama |
| IERC4626.sol | 8 | Interface | Orta — vault fiyat |
| IStorageAccessible.sol | 7 | Interface | Orta — storage okuma |
| Caller.sol | 42 | Library | KRITIK — gas olcum |
| Math.sol | 13 | Library | Orta — overflow korumaları |
| SafeERC20.sol | 25 | Library | Yuksek — non-standard token |
| Solver.sol | 165 | Contract | KRITIK — simulasyon |
| Swapper.sol | 116 | Contract | KRITIK — DEX gas olcum |
| Balances.sol | 151 | Contract | KRITIK — bakiye dogrulama |
| Signatures.sol | 91 | Contract | Yuksek — imza gas |
| AnyoneAuthenticator.sol | 8 | Contract | Orta — test auth |
| Spardose.sol | 24 | Contract | Dusuk — test funding |
| Counter.sol | 24 | Test | Test only |
| GasHog.sol | 27 | Test | Test only |
| MockERC4626Wrapper.sol | 54 | Test | Test only |
| NonStandardERC20Balances.sol | 47 | Test | Test only |
| RemoteERC20Balances.sol | 22 | Test | Test only |
| main.rs | ~563 | Rust | Yuksek — codegen |
| codegen.rs | ~455 | Rust | Yuksek — binding uretimi |
| vendor.rs | ~347 | Rust | Orta — artifact toplama |
| networks.rs | 13 | Rust | Orta — ag sabitleri |

---

## KRITIK BULGULAR — ACILMASIZ NOKTALARIN LISTESI

1. **GPv2Settlement.sol bu repo'da YOK** — yalnizca artifact olarak npm'den indirilir. Asil `settle()` implementasyonu `@cowprotocol/contracts@1.1.2` paketindedir. Bir onceki agent'in (agent-08) bu kontrati analiz etmis olmasi gerekiyor.

2. **Uniform Clearing Price** — TUM siparisler ayni clearing price vektoru ile settle edilir. Bu MEV korumasinin temelidir: solver bireysel siparisleri degerlendiremez, batch'in tamamini optimize etmek zorundadir.

3. **Swapper.sol clearing price kurulumu kusursuz:**
   ```
   clearingPrices[sell] = buy.amount
   clearingPrices[buy] = sell.amount
   → executed_buy = executed_sell * buy_amount / sell_amount
   ```
   fill-or-kill siparisler icin sifir surplus, limit siparislerde iyilestirme mumkun.

4. **EIP-7702 entegrasyonu:** `Solver7702Delegate` — akilli hesap soyutlama (EIP-7702) ile solver hesaplari icerik dagiliyor. Nova Shield icin ileride onemli (account abstraction).

5. **Cross-chain:** `ICowWrapper` — CoW cross-chain wrapper interface. Nova Shield icin: bridge entegrasyonu ve cross-chain settlement.

6. **LiquoriceSettlement** — kapali kaynak alternatif settlement. Entegrasyon noktalari belirsiz. Nova Shield'in kendi settlement kontratinin acik kaynak olmasi kritik.

7. **ChainalysisOracle** — AML/KYC entegrasyonu. Nova Shield icin: compliance katmani zorunlulugu (bazi jursidikasyonlar icin).

8. **FlashLoanRouter** — `0x9da8b484...` tum aglarda deterministik — flash loan ile settlement entegrasyonu. Nova Shield L1'de atomic flash loan: tek block icinde borclan-settle et-geri ode.
