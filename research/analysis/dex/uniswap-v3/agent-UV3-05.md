---
# Uniswap V3 Analiz — Agent UV3-05
## Kapsam: Periphery (60 dosya)
## DEX Ozel Protokolu: 3x Hassasiyet
## Tarih: 2026-07-22
---

# BOLUM 1: GENEL MIMARIYE BAKIS

Uniswap V3 periphery katmani, core (havuz/fabrika) kontratlarinin uzerinde konumlanan bir kullanici arayuz
ve yardimci fonksiyon katmanidir. Temel gorev: kullanicidan gelen ham talepleri (swap, likidite ekleme,
flash loan vs.) guvenli sekilde core'a ileten, slippage kontrolu yapan, callback'leri dogrulayan ve
ETH/WETH donusum islemlerini otomasyon ile yoneten bir arakatman olmaktir.

Toplam analiz edilen dosya: 60 adet
- Ana kontratlar: SwapRouter, NonfungiblePositionManager, NonfungibleTokenPositionDescriptor, V3Migrator
- Lens kontratlar: QuoterV2, Quoter, TickLens, UniswapInterfaceMulticall
- Base (soyut) kontratlar: LiquidityManagement, PeripheryPayments, PeripheryPaymentsWithFee,
  PeripheryImmutableState, PeripheryValidation, BlockTimestamp, ERC721Permit, Multicall,
  PoolInitializer, SelfPermit
- Kutuphaneler (libraries): Path, PoolAddress, CallbackValidation, LiquidityAmounts, OracleLibrary,
  BytesLib, NFTDescriptor, NFTSVG, HexStrings, TransferHelper, PositionKey, PositionValue,
  SqrtPriceMathPartial, TokenRatioSortOrder, PoolTicksCounter, ChainId
- Arabirimler (interfaces): ISwapRouter, INonfungiblePositionManager, IQuoterV2, IQuoter, ITickLens,
  IV3Migrator, IMulticall, IPoolInitializer, IPeripheryImmutableState, IPeripheryPayments,
  IPeripheryPaymentsWithFee, IERC721Permit, ISelfPermit, INonfungibleTokenPositionDescriptor,
  IERC20Metadata, IERC1271, IERC20PermitAllowed, IWETH9
- Test kontratlar: LiquidityAmountsTest, MockObservable, MockObservations,
  MockTimeNonfungiblePositionManager, MockTimeSwapRouter, OracleTest, PathTest
- Ornek kontrat: PairFlash

---

# BOLUM 2: SWAPROUTER.SOL — DERINLEMESINE ANALIZ

## 2.1 Kontrat Imzasi ve Kalitim Zinciri

```solidity
contract SwapRouter is
    ISwapRouter,
    PeripheryImmutableState,
    PeripheryValidation,
    PeripheryPaymentsWithFee,
    Multicall,
    SelfPermit
```

SwapRouter, 6 farkli kontrat/arabirimden miras alir:
- `ISwapRouter`: exactInputSingle, exactInput, exactOutputSingle, exactOutput fonksiyon imzalari
- `PeripheryImmutableState`: factory ve WETH9 adresleri immutable olarak tutar
- `PeripheryValidation`: `checkDeadline` modifier'i (`_blockTimestamp() <= deadline`)
- `PeripheryPaymentsWithFee`: ETH/WETH sarma/cozme, sweepToken, fee ile odeme
- `Multicall`: tek transaksiyonda coklu cagri (delegatecall tabanli)
- `SelfPermit`: EIP-2612 permit ile approve + swap tek tx'te

## 2.2 Kritik Durum Degiskeni: amountInCached

```solidity
uint256 private constant DEFAULT_AMOUNT_IN_CACHED = type(uint256).max;
uint256 private amountInCached = DEFAULT_AMOUNT_IN_CACHED;
```

Bu degisken, exactOutput ve exactOutputSingle fonksiyonlarinda "geri donus" mekanizmasi olarak kullanilir.
Coklu-hop exact output swapinda, callback zinciri geri sarildikca her hop'ta ne kadar token odendigi
bu gecici storage slot'ta biriktirilir. Islem bittikten sonra sifirlanir (type(uint256).max'e atanir).
type(uint256).max: gecerli bir amountIn degeri olamayacagi icin placeholder olarak guvenlidir.

## 2.3 Pool Adresi Hesaplama: getPool

```solidity
function getPool(address tokenA, address tokenB, uint24 fee)
    private view returns (IUniswapV3Pool)
{
    return IUniswapV3Pool(
        PoolAddress.computeAddress(factory, PoolAddress.getPoolKey(tokenA, tokenB, fee))
    );
}
```

Pool adresi her zaman CREATE2 ile deterministik hesaplanir; storage'dan okunmaz. Bu, gas optimizasyonu
ve fake pool saldirisi onleme amaciyla kritiktir. Adres hesaplama tamamen off-chain ile ozdestir.

## 2.4 exactInputSingle

```solidity
function exactInputSingle(ExactInputSingleParams calldata params)
    external payable override checkDeadline(params.deadline)
    returns (uint256 amountOut)
{
    amountOut = exactInputInternal(
        params.amountIn,
        params.recipient,
        params.sqrtPriceLimitX96,
        SwapCallbackData({
            path: abi.encodePacked(params.tokenIn, params.fee, params.tokenOut),
            payer: msg.sender
        })
    );
    require(amountOut >= params.amountOutMinimum, 'Too little received');
}
```

Parametreler (ExactInputSingleParams):
- tokenIn, tokenOut: swap edilecek token adresleri
- fee: havuz uc seviyesi (500, 3000, 10000 bps)
- recipient: cikti tokenin gidecegi adres
- deadline: islemin son gecerlilik zamani (Unix timestamp)
- amountIn: kesin giris miktari
- amountOutMinimum: slippage korumasi - minimum cikis miktari
- sqrtPriceLimitX96: fiyat siniri (0 = sinir yok)

Path encoding: `abi.encodePacked(tokenIn, fee, tokenOut)` → 20 + 3 + 20 = 43 byte

Slippage kontrolu: `require(amountOut >= params.amountOutMinimum, 'Too little received')`

## 2.5 exactInputInternal — Tek Havuz Swapi

```solidity
function exactInputInternal(
    uint256 amountIn, address recipient, uint160 sqrtPriceLimitX96,
    SwapCallbackData memory data
) private returns (uint256 amountOut)
{
    if (recipient == address(0)) recipient = address(this);
    (address tokenIn, address tokenOut, uint24 fee) = data.path.decodeFirstPool();
    bool zeroForOne = tokenIn < tokenOut;

    (int256 amount0, int256 amount1) = getPool(tokenIn, tokenOut, fee).swap(
        recipient,
        zeroForOne,
        amountIn.toInt256(),
        sqrtPriceLimitX96 == 0
            ? (zeroForOne ? TickMath.MIN_SQRT_RATIO + 1 : TickMath.MAX_SQRT_RATIO - 1)
            : sqrtPriceLimitX96,
        abi.encode(data)
    );
    return uint256(-(zeroForOne ? amount1 : amount0));
}
```

zeroForOne belirleme: `tokenIn < tokenOut` — bu Uniswap'in token0/token1 siralamasinin adreste daha
kucuk olan token0 olduguna dayali bir mantiktir. Eger token0 gonderiliyorsa (zeroForOne=true) token1
alinir ve tam tersi.

Fiyat siniri: sqrtPriceLimitX96 = 0 oldugunda, havuzun maksimum bant disiyla sinirlandirilir:
- zeroForOne=true: TickMath.MIN_SQRT_RATIO + 1 (en dusuk fiyat)
- zeroForOne=false: TickMath.MAX_SQRT_RATIO - 1 (en yuksek fiyat)

Dondurme: `uint256(-(zeroForOne ? amount1 : amount0))` — core swap fonksiyonu signed int dondurur.
Pozitif = odendi, negatif = alindi. Alman miktari negatif olacagindan (-) ile pozitife cevrilir.

## 2.6 exactInput — Coklu Hop (Multi-hop) Swaplari

```solidity
function exactInput(ExactInputParams memory params)
    external payable override checkDeadline(params.deadline)
    returns (uint256 amountOut)
{
    address payer = msg.sender;
    while (true) {
        bool hasMultiplePools = params.path.hasMultiplePools();
        params.amountIn = exactInputInternal(
            params.amountIn,
            hasMultiplePools ? address(this) : params.recipient,
            0,
            SwapCallbackData({path: params.path.getFirstPool(), payer: payer})
        );
        if (hasMultiplePools) {
            payer = address(this);
            params.path = params.path.skipToken();
        } else {
            amountOut = params.amountIn;
            break;
        }
    }
    require(amountOut >= params.amountOutMinimum, 'Too little received');
}
```

Multi-hop mantigi:
1. Ilk hop: payer = msg.sender, recipient = address(this) (router aracilik eder)
2. Ara hop: payer = address(this), recipient = address(this)
3. Son hop: payer = address(this), recipient = params.recipient (gercek alici)

Path iterasyonu:
- `hasMultiplePools()`: path uzunlugu >= 66 byte (ADDR + FEE + ADDR + FEE + ADDR = 20+3+20+3+20)
- `getFirstPool()`: ilk 43 byte'i al (ilk havuzun path segmenti)
- `skipToken()`: ilk 23 byte'i at (ADDR_SIZE + FEE_SIZE = 23), kalan path ile devam

Her hop arasi intermediate tokenlar: Router sozlesmesi gecici olarak tutar, slippage yok (sadece son)

## 2.7 exactOutputSingle ve exactOutput

exactOutputSingle:
```solidity
function exactOutputSingle(ExactOutputSingleParams calldata params)
    external payable override checkDeadline(params.deadline)
    returns (uint256 amountIn)
{
    amountIn = exactOutputInternal(
        params.amountOut,
        params.recipient,
        params.sqrtPriceLimitX96,
        SwapCallbackData({
            path: abi.encodePacked(params.tokenOut, params.fee, params.tokenIn),
            payer: msg.sender
        })
    );
    require(amountIn <= params.amountInMaximum, 'Too much requested');
    amountInCached = DEFAULT_AMOUNT_IN_CACHED;
}
```

Dikkat: Path encoding TERS! exactOutput'da path `tokenOut → fee → tokenIn` sirasinda encode edilir.
Bu, core swap fonksiyonunun ters yonde calistirilmasi icin gereklidir (negative amountSpecified).

exactOutputInternal:
```solidity
function exactOutputInternal(...) private returns (uint256 amountIn)
{
    (address tokenOut, address tokenIn, uint24 fee) = data.path.decodeFirstPool();
    bool zeroForOne = tokenIn < tokenOut;
    (int256 amount0Delta, int256 amount1Delta) = getPool(tokenIn, tokenOut, fee).swap(
        recipient, zeroForOne,
        -amountOut.toInt256(),   // NEGATIF: exact output belirtir
        ...
    );
    ...
    if (sqrtPriceLimitX96 == 0) require(amountOutReceived == amountOut);
}
```

exactOutput multi-hop:
- `exactOutput()` cagrildiginda, callback zinciri icinde `exactOutputInternal` recursive cagrilir
- Her callback: `if (data.path.hasMultiplePools()) { data.path = data.path.skipToken(); exactOutputInternal(...) }`
- Son callback: `amountInCached = amountToPay` — toplam odenen miktar kaydedilir
- Ana fonksiyon: `amountIn = amountInCached` ile gercek odenen miktari okur

## 2.8 uniswapV3SwapCallback — Guvenlik Merkezi

```solidity
function uniswapV3SwapCallback(
    int256 amount0Delta, int256 amount1Delta,
    bytes calldata _data
) external override {
    require(amount0Delta > 0 || amount1Delta > 0);
    SwapCallbackData memory data = abi.decode(_data, (SwapCallbackData));
    (address tokenIn, address tokenOut, uint24 fee) = data.path.decodeFirstPool();
    CallbackValidation.verifyCallback(factory, tokenIn, tokenOut, fee);
    ...
}
```

Guvenlik: `CallbackValidation.verifyCallback` cagrisinin onemi kritiktir. Bu cagri olmadan herhangi
bir kontrat bu callback'i cagirip token transferini tetikleyebilir (fake pool saldirisi).

CallbackValidation.verifyCallback icerigi:
```solidity
function verifyCallback(address factory, address tokenA, address tokenB, uint24 fee)
    internal view returns (IUniswapV3Pool pool)
{
    return verifyCallback(factory, PoolAddress.getPoolKey(tokenA, tokenB, fee));
}

function verifyCallback(address factory, PoolAddress.PoolKey memory poolKey)
    internal view returns (IUniswapV3Pool pool)
{
    pool = IUniswapV3Pool(PoolAddress.computeAddress(factory, poolKey));
    require(msg.sender == address(pool));
}
```

Dogrulama mekanizmasi: msg.sender'in, CREATE2 ile hesaplanan gercek havuz adresiyle eslesip
eslesmedigini kontrol eder. Eger bir saldirgan sahte callback gonderirse, adresi hesaplanan
gercek havuz adresiyle elesemeyecegi icin islem revert eder.

---

# BOLUM 3: PATH KUTUPHANESI — DETAYLI ANALIZ

## 3.1 Sabitler ve Encoding Formati

```solidity
uint256 private constant ADDR_SIZE = 20;     // adres boyutu (byte)
uint256 private constant FEE_SIZE = 3;        // fee boyutu (uint24 = 3 byte)
uint256 private constant NEXT_OFFSET = ADDR_SIZE + FEE_SIZE;       // = 23
uint256 private constant POP_OFFSET = NEXT_OFFSET + ADDR_SIZE;     // = 43
uint256 private constant MULTIPLE_POOLS_MIN_LENGTH = POP_OFFSET + NEXT_OFFSET; // = 66
```

Path encoding formati:
```
[token0: 20 byte][fee01: 3 byte][token1: 20 byte][fee12: 3 byte][token2: 20 byte]...
```

Tek havuz (43 byte):  `[tokenA:20][fee:3][tokenB:20]`
Iki havuz (66 byte):  `[tokenA:20][fee01:3][tokenB:20][fee12:3][tokenC:20]`
Uc havuz (89 byte):   `[tokenA:20][fee01:3][tokenB:20][fee12:3][tokenC:20][fee23:3][tokenD:20]`

## 3.2 hasMultiplePools

```solidity
function hasMultiplePools(bytes memory path) internal pure returns (bool) {
    return path.length >= MULTIPLE_POOLS_MIN_LENGTH;  // >= 66
}
```

## 3.3 numPools

```solidity
function numPools(bytes memory path) internal pure returns (uint256) {
    return ((path.length - ADDR_SIZE) / NEXT_OFFSET);
}
```

Hesaplama: `(path.length - 20) / 23`
- 43 byte (1 havuz): (43-20)/23 = 1
- 66 byte (2 havuz): (66-20)/23 = 2
- 89 byte (3 havuz): (89-20)/23 = 3

## 3.4 decodeFirstPool

```solidity
function decodeFirstPool(bytes memory path)
    internal pure returns (address tokenA, address tokenB, uint24 fee)
{
    tokenA = path.toAddress(0);          // offset 0: ilk 20 byte → adres
    fee = path.toUint24(ADDR_SIZE);      // offset 20: sonraki 3 byte → uint24
    tokenB = path.toAddress(NEXT_OFFSET); // offset 23: sonraki 20 byte → adres
}
```

BytesLib kullanimi:
- `toAddress(bytes, start)`: inline assembly ile `div(mload(add(add(_bytes, 0x20), _start)), 0x1000000000000000000000000)`
- `toUint24(bytes, start)`: `mload(add(add(_bytes, 0x3), _start))`

## 3.5 getFirstPool ve skipToken

```solidity
function getFirstPool(bytes memory path) internal pure returns (bytes memory) {
    return path.slice(0, POP_OFFSET);  // ilk 43 byte
}

function skipToken(bytes memory path) internal pure returns (bytes memory) {
    return path.slice(NEXT_OFFSET, path.length - NEXT_OFFSET);  // 23 byte atla
}
```

skipToken: `tokenA[20] + fee[3]` = 23 byte atlar, geriye `tokenB + fee12 + tokenC + ...` kalir.
Bu, multi-hop dongusunde bir sonraki havuza gecisi saglar.

---

# BOLUM 4: POOLADDRESS KUTUPHANESI — CREATE2 HESAPLAMA

## 4.1 POOL_INIT_CODE_HASH

```solidity
bytes32 internal constant POOL_INIT_CODE_HASH =
    0xe34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54;
```

Bu hash, UniswapV3Pool kontratinin bytecode'unun keccak256'sidir. Sabit ve degistirilemez.
Nova Shield'da kendi havuz kodunu kullanacaksa bu hash farkli olacaktir.

## 4.2 PoolKey Struct ve getPoolKey

```solidity
struct PoolKey {
    address token0;
    address token1;
    uint24 fee;
}

function getPoolKey(address tokenA, address tokenB, uint24 fee)
    internal pure returns (PoolKey memory)
{
    if (tokenA > tokenB) (tokenA, tokenB) = (tokenB, tokenA);
    return PoolKey({token0: tokenA, token1: tokenB, fee: fee});
}
```

Siralanmis pair: her zaman token0 < token1 (adres numerik karsilastirmasi).

## 4.3 computeAddress — Tam CREATE2 Formulu

```solidity
function computeAddress(address factory, PoolKey memory key)
    internal pure returns (address pool)
{
    require(key.token0 < key.token1);
    pool = address(
        uint256(
            keccak256(
                abi.encodePacked(
                    hex'ff',
                    factory,
                    keccak256(abi.encode(key.token0, key.token1, key.fee)),
                    POOL_INIT_CODE_HASH
                )
            )
        )
    );
}
```

CREATE2 adresi formulu:
```
pool_address = keccak256(0xff ++ factory_address ++ salt ++ init_code_hash)[12:]
```
burada:
```
salt = keccak256(abi.encode(token0, token1, fee))
init_code_hash = POOL_INIT_CODE_HASH = 0xe34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54
```

Bu yaklasim sayesinde:
1. Herhangi bir kontrat, fabrika adresini biliyorsa herhangi bir havuzun adresini hesaplayabilir
2. SLOAD operasyonu olmadan gas tasarrufu saglanir
3. Fake pool saldirisi imkansizlasir (sadece gercek fabrika deploy edebilir)

---

# BOLUM 5: LIQUIDITYAMOUNTS KUTUPHANESI — TAM FORMULLER

## 5.1 getLiquidityForAmount0

```solidity
function getLiquidityForAmount0(
    uint160 sqrtRatioAX96,
    uint160 sqrtRatioBX96,
    uint256 amount0
) internal pure returns (uint128 liquidity)
{
    if (sqrtRatioAX96 > sqrtRatioBX96)
        (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);
    uint256 intermediate = FullMath.mulDiv(sqrtRatioAX96, sqrtRatioBX96, FixedPoint96.Q96);
    return toUint128(FullMath.mulDiv(amount0, intermediate, sqrtRatioBX96 - sqrtRatioAX96));
}
```

Matematiksel formul (kaynak koddan):
```
intermediate = (sqrtP_lower * sqrtP_upper) / Q96
L = amount0 * intermediate / (sqrtP_upper - sqrtP_lower)
  = amount0 * (sqrtP_lower * sqrtP_upper / Q96) / (sqrtP_upper - sqrtP_lower)
```

Duzen aciklamasi: Uniswap V3'de fiyatlar Q64.96 formatinda saklanir (sqrtPriceX96 = sqrt(P) * 2^96).
FixedPoint96.Q96 = 2^96. Yani sqrtRatioAX96 * sqrtRatioBX96 / Q96 = sqrt(Pa) * sqrt(Pb) (raw).

Matematiksel turevi:
- V3 formulu: L = amount0 / (1/sqrt(Pa) - 1/sqrt(Pb)) = amount0 * sqrt(Pa)*sqrt(Pb) / (sqrt(Pb)-sqrt(Pa))

## 5.2 getLiquidityForAmount1

```solidity
function getLiquidityForAmount1(
    uint160 sqrtRatioAX96,
    uint160 sqrtRatioBX96,
    uint256 amount1
) internal pure returns (uint128 liquidity)
{
    if (sqrtRatioAX96 > sqrtRatioBX96)
        (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);
    return toUint128(FullMath.mulDiv(amount1, FixedPoint96.Q96, sqrtRatioBX96 - sqrtRatioAX96));
}
```

Matematiksel formul:
```
L = amount1 * Q96 / (sqrtP_upper - sqrtP_lower)
  = amount1 / ((sqrtP_upper - sqrtP_lower) / Q96)
  = amount1 / (sqrt(Pb) - sqrt(Pa))    [raw degerler]
```

V3 formulu: L = amount1 / (sqrt(Pb) - sqrt(Pa)) — token1 fiyat araligina esit dagitilir.

## 5.3 getLiquidityForAmounts — 3 Durum Analizi

```solidity
function getLiquidityForAmounts(
    uint160 sqrtRatioX96,   // mevcut fiyat
    uint160 sqrtRatioAX96,  // alt sinir tick
    uint160 sqrtRatioBX96,  // ust sinir tick
    uint256 amount0,
    uint256 amount1
) internal pure returns (uint128 liquidity)
{
    if (sqrtRatioAX96 > sqrtRatioBX96)
        (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);

    if (sqrtRatioX96 <= sqrtRatioAX96) {
        // DURUM 1: Fiyat aralik altinda — sadece token0
        liquidity = getLiquidityForAmount0(sqrtRatioAX96, sqrtRatioBX96, amount0);
    } else if (sqrtRatioX96 < sqrtRatioBX96) {
        // DURUM 2: Fiyat araligi icinde — her iki token da kullanilir
        uint128 liquidity0 = getLiquidityForAmount0(sqrtRatioX96, sqrtRatioBX96, amount0);
        uint128 liquidity1 = getLiquidityForAmount1(sqrtRatioAX96, sqrtRatioX96, amount1);
        liquidity = liquidity0 < liquidity1 ? liquidity0 : liquidity1;
    } else {
        // DURUM 3: Fiyat aralik ustunde — sadece token1
        liquidity = getLiquidityForAmount1(sqrtRatioAX96, sqrtRatioBX96, amount1);
    }
}
```

3 durum detayli aciklama:

DURUM 1: sqrtP <= sqrtP_lower (mevcut fiyat aralik altinda)
- Pozisyon tamamen token0 icerir
- Token1 kullanilmaz (amount1 gozardi edilir)
- Hesaplama: L = amount0 * sqrt(Pa) * sqrt(Pb) / (sqrt(Pb) - sqrt(Pa))
- Ornekle: Pa=1, Pb=4, amount0=100 → L = 100 * 1 * 2 / (2-1) = 200

DURUM 2: sqrtP_lower < sqrtP < sqrtP_upper (aralik icinde — aktif pozisyon)
- Her iki token da kullanilir
- Iki farkli L hesaplanir, kucuk olan secilir (bottleneck: hangisi biter once?)
- liquidity0 = token0 ile saglanabilecek L (mevcut fiyattan ust sinira)
- liquidity1 = token1 ile saglanabilecek L (alt sinirdan mevcut fiyata)
- liquidity = min(liquidity0, liquidity1) — token tarafina gore kisitlanan taraf belirler
- Ornekle: sqrtP=1.5, Pa=1, Pb=2, amount0=50, amount1=30
  L0 = getLiquidityForAmount0(1.5, 2, 50), L1 = getLiquidityForAmount1(1, 1.5, 30)
  Daha kucuk olan secilir

DURUM 3: sqrtP >= sqrtP_upper (mevcut fiyat aralik ustunde)
- Pozisyon tamamen token1 icerir
- Token0 kullanilmaz (amount0 gozardi edilir)
- Hesaplama: L = amount1 / (sqrt(Pb) - sqrt(Pa))

## 5.4 getAmount0ForLiquidity

```solidity
function getAmount0ForLiquidity(
    uint160 sqrtRatioAX96,
    uint160 sqrtRatioBX96,
    uint128 liquidity
) internal pure returns (uint256 amount0)
{
    if (sqrtRatioAX96 > sqrtRatioBX96)
        (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);
    return
        FullMath.mulDiv(
            uint256(liquidity) << FixedPoint96.RESOLUTION,
            sqrtRatioBX96 - sqrtRatioAX96,
            sqrtRatioBX96
        ) / sqrtRatioAX96;
}
```

Formul:
```
amount0 = (L << 96) * (sqrtPb - sqrtPa) / sqrtPb / sqrtPa
        = L * Q96 * (sqrtPb - sqrtPa) / (sqrtPb * sqrtPa)
        = L * (1/sqrtPa - 1/sqrtPb)    [raw degerler]
```

FixedPoint96.RESOLUTION = 96 (yani `<< 96` islemi Q96 precision ekler)

## 5.5 getAmount1ForLiquidity

```solidity
function getAmount1ForLiquidity(
    uint160 sqrtRatioAX96,
    uint160 sqrtRatioBX96,
    uint128 liquidity
) internal pure returns (uint256 amount1)
{
    if (sqrtRatioAX96 > sqrtRatioBX96)
        (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);
    return FullMath.mulDiv(liquidity, sqrtRatioBX96 - sqrtRatioAX96, FixedPoint96.Q96);
}
```

Formul:
```
amount1 = L * (sqrtPb - sqrtPa) / Q96
        = L * (sqrt(Pb) - sqrt(Pa))   [raw degerler]
```

## 5.6 getAmountsForLiquidity — Ters Donusum

```solidity
function getAmountsForLiquidity(
    uint160 sqrtRatioX96,
    uint160 sqrtRatioAX96,
    uint160 sqrtRatioBX96,
    uint128 liquidity
) internal pure returns (uint256 amount0, uint256 amount1)
{
    if (sqrtRatioAX96 > sqrtRatioBX96)
        (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);

    if (sqrtRatioX96 <= sqrtRatioAX96) {
        amount0 = getAmount0ForLiquidity(sqrtRatioAX96, sqrtRatioBX96, liquidity);
    } else if (sqrtRatioX96 < sqrtRatioBX96) {
        amount0 = getAmount0ForLiquidity(sqrtRatioX96, sqrtRatioBX96, liquidity);
        amount1 = getAmount1ForLiquidity(sqrtRatioAX96, sqrtRatioX96, liquidity);
    } else {
        amount1 = getAmount1ForLiquidity(sqrtRatioAX96, sqrtRatioBX96, liquidity);
    }
}
```

---

# BOLUM 6: NONFUNGIBLEPOSITIONMANAGER.SOL — DERINLEMESINE ANALIZ

## 6.1 Position Struct

```solidity
struct Position {
    uint96 nonce;                          // permit nonce
    address operator;                      // approved spender
    uint80 poolId;                         // havuz ID'si (adres yerine tasarruf)
    int24 tickLower;                       // alt tick siniri
    int24 tickUpper;                       // ust tick siniri
    uint128 liquidity;                     // mevcut likidite miktari
    uint256 feeGrowthInside0LastX128;      // son islemdeki fee growth snapshot
    uint256 feeGrowthInside1LastX128;
    uint128 tokensOwed0;                   // toplanmamis ucretler (token0)
    uint128 tokensOwed1;
}
```

Depolamalar:
- `mapping(address => uint80) private _poolIds`: havuz adresi → havuz ID
- `mapping(uint80 => PoolAddress.PoolKey) private _poolIdToPoolKey`: ID → PoolKey
- `mapping(uint256 => Position) private _positions`: tokenId → Position

poolId optimizasyonu: Dogrudan havuz adresini (20 byte = 160 bit) her pozisyonda saklamak yerine,
uint80 (10 byte) ID kullanilarak 2 SSLOT yerine 1 SSLOT kullanilir.

## 6.2 mint — NFT Pozisyon Olusturma

```solidity
function mint(MintParams calldata params)
    external payable override checkDeadline(params.deadline)
    returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1)
{
    IUniswapV3Pool pool;
    (liquidity, amount0, amount1, pool) = addLiquidity(AddLiquidityParams({
        token0: params.token0,
        token1: params.token1,
        fee: params.fee,
        recipient: address(this),  // NPM kendisi likiditeyi tutar
        tickLower: params.tickLower,
        tickUpper: params.tickUpper,
        amount0Desired: params.amount0Desired,
        amount1Desired: params.amount1Desired,
        amount0Min: params.amount0Min,
        amount1Min: params.amount1Min
    }));

    _mint(params.recipient, (tokenId = _nextId++));  // NFT basilir

    bytes32 positionKey = PositionKey.compute(address(this), params.tickLower, params.tickUpper);
    (, uint256 feeGrowthInside0LastX128, uint256 feeGrowthInside1LastX128, , ) =
        pool.positions(positionKey);

    uint80 poolId = cachePoolKey(address(pool), PoolAddress.PoolKey({...}));

    _positions[tokenId] = Position({
        nonce: 0, operator: address(0), poolId: poolId,
        tickLower: params.tickLower, tickUpper: params.tickUpper,
        liquidity: liquidity,
        feeGrowthInside0LastX128: feeGrowthInside0LastX128,
        feeGrowthInside1LastX128: feeGrowthInside1LastX128,
        tokensOwed0: 0, tokensOwed1: 0
    });

    emit IncreaseLiquidity(tokenId, liquidity, amount0, amount1);
}
```

MintParams yapisi:
- token0, token1: sorted token adresleri (token0 < token1)
- fee: havuz ucreti
- tickLower, tickUpper: pozisyon aralik tikleri
- amount0Desired, amount1Desired: arzu edilen miktarlar (ust sinir)
- amount0Min, amount1Min: slippage koruma (alt sinir)
- recipient: NFT'nin gidecegi adres
- deadline: islem son gecerlilik zamani

Neden NFT? V2'de LP token'lari fungibledi (herkes ayni havuzda ayni tokendan alirdi), V3'de
her pozisyon farkli (tickLower, tickUpper, likidite) oldugundan ERC721 kullanilir.

## 6.3 increaseLiquidity — Mevcut Pozisyona Likidite Ekleme

```solidity
function increaseLiquidity(IncreaseLiquidityParams calldata params)
    external payable override checkDeadline(params.deadline)
    returns (uint128 liquidity, uint256 amount0, uint256 amount1)
{
    Position storage position = _positions[params.tokenId];
    PoolAddress.PoolKey memory poolKey = _poolIdToPoolKey[position.poolId];

    IUniswapV3Pool pool;
    (liquidity, amount0, amount1, pool) = addLiquidity(AddLiquidityParams({...}));

    bytes32 positionKey = PositionKey.compute(address(this), position.tickLower, position.tickUpper);
    (, uint256 feeGrowthInside0LastX128, uint256 feeGrowthInside1LastX128, , ) = pool.positions(positionKey);

    // Birikis ucretleri guncelle
    position.tokensOwed0 += uint128(FullMath.mulDiv(
        feeGrowthInside0LastX128 - position.feeGrowthInside0LastX128,
        position.liquidity,
        FixedPoint128.Q128
    ));
    position.tokensOwed1 += uint128(FullMath.mulDiv(
        feeGrowthInside1LastX128 - position.feeGrowthInside1LastX128,
        position.liquidity,
        FixedPoint128.Q128
    ));

    position.feeGrowthInside0LastX128 = feeGrowthInside0LastX128;
    position.feeGrowthInside1LastX128 = feeGrowthInside1LastX128;
    position.liquidity += liquidity;
}
```

Fee accumulation formulu:
```
tokensOwed0 += (feeGrowthInside0LastX128_new - feeGrowthInside0LastX128_old) * liquidity / Q128
```
Bu formul, o pozisyona atfedilen birikmis ucretlerin dogru hesaplanmasini saglar.

## 6.4 decreaseLiquidity

```solidity
function decreaseLiquidity(DecreaseLiquidityParams calldata params)
    external payable override isAuthorizedForToken(params.tokenId) checkDeadline(params.deadline)
    returns (uint256 amount0, uint256 amount1)
{
    Position storage position = _positions[params.tokenId];
    uint128 positionLiquidity = position.liquidity;
    require(positionLiquidity >= params.liquidity);

    IUniswapV3Pool pool = IUniswapV3Pool(PoolAddress.computeAddress(factory, poolKey));
    (amount0, amount1) = pool.burn(position.tickLower, position.tickUpper, params.liquidity);

    require(amount0 >= params.amount0Min && amount1 >= params.amount1Min, 'Price slippage check');

    position.tokensOwed0 += uint128(amount0) + uint128(FullMath.mulDiv(
        feeGrowthInside0LastX128 - position.feeGrowthInside0LastX128,
        positionLiquidity, FixedPoint128.Q128
    ));
    // ... benzer token1 icin

    position.liquidity = positionLiquidity - params.liquidity;
}
```

Onemli: `pool.burn()` core'a gider ve pozisyonun likidite miktarini azaltir, ancak tokenlar hala
havuzda dururlar (collect() cagrisina kadar). tokensOwed birikimi hem yanilan likidite degeri hem
de birikmis ucret getirisini icermektedir.

## 6.5 collect — MAX_UINT128 Trick

```solidity
function collect(CollectParams calldata params)
    external payable override isAuthorizedForToken(params.tokenId)
    returns (uint256 amount0, uint256 amount1)
{
    require(params.amount0Max > 0 || params.amount1Max > 0);
    address recipient = params.recipient == address(0) ? address(this) : params.recipient;
    Position storage position = _positions[params.tokenId];

    (uint128 tokensOwed0, uint128 tokensOwed1) = (position.tokensOwed0, position.tokensOwed1);

    // Acik pozisyonlar icin fee guncellemesi: burn(0) ile tetiklenir
    if (position.liquidity > 0) {
        pool.burn(position.tickLower, position.tickUpper, 0);
        // fee growth guncellendikten sonra tokensOwed yeniden hesaplanir
        tokensOwed0 += uint128(FullMath.mulDiv(
            feeGrowthInside0LastX128 - position.feeGrowthInside0LastX128,
            position.liquidity, FixedPoint128.Q128
        ));
        // ...
    }

    (uint128 amount0Collect, uint128 amount1Collect) = (
        params.amount0Max > tokensOwed0 ? tokensOwed0 : params.amount0Max,
        params.amount1Max > tokensOwed1 ? tokensOwed1 : params.amount1Max
    );

    (amount0, amount1) = pool.collect(
        recipient, position.tickLower, position.tickUpper,
        amount0Collect, amount1Collect
    );

    (position.tokensOwed0, position.tokensOwed1) =
        (tokensOwed0 - amount0Collect, tokensOwed1 - amount1Collect);
}
```

MAX_UINT128 trick: CollectParams.amount0Max = type(uint128).max olarak set edilirse, tokensOwed0
degerinin tamami cekilir. Bu yaklasim, tam miktar bilgisi olmadan "ne kadar varsa hepsini al"
islemi yapmaya olanak tanir. Uygulama `params.amount0Max > tokensOwed0 ? tokensOwed0 : params.amount0Max`
ile gerceklestir.

burn(0) trick: Aktif pozisyonlarda (liquidity > 0) fee guncelleme icin pool.burn() sifir miktar ile
cagrilir. Bu, core'da fee growth snapshot'ini gunceller ama tokeni yanmaz.

## 6.6 burn — NFT Yakma

```solidity
function burn(uint256 tokenId) external payable override isAuthorizedForToken(tokenId) {
    Position storage position = _positions[tokenId];
    require(
        position.liquidity == 0 && position.tokensOwed0 == 0 && position.tokensOwed1 == 0,
        'Not cleared'
    );
    delete _positions[tokenId];
    _burn(tokenId);
}
```

NFT yakma icin: once decreaseLiquidity ile likidite sifirlanmali, ardindan collect ile ucretler
alinmali, ancak o zaman NFT yakilabilir.

## 6.7 ERC721Permit — Gasless Approve

```solidity
bytes32 public constant override PERMIT_TYPEHASH =
    0x49ecf333e5b8c95c40fdafc95c1ad136e8914a8fb55e9dc8bb01eaa83a2df9ad;
// keccak256("Permit(address spender,uint256 tokenId,uint256 nonce,uint256 deadline)")

function permit(address spender, uint256 tokenId, uint256 deadline, uint8 v, bytes32 r, bytes32 s)
    external payable override
{
    require(_blockTimestamp() <= deadline, 'Permit expired');
    bytes32 digest = keccak256(abi.encodePacked(
        '\x19\x01',
        DOMAIN_SEPARATOR(),
        keccak256(abi.encode(PERMIT_TYPEHASH, spender, tokenId, _getAndIncrementNonce(tokenId), deadline))
    ));
    address owner = ownerOf(tokenId);
    // ERC1271 (smart wallet) veya EOA dogrulamasi
    if (Address.isContract(owner)) {
        require(IERC1271(owner).isValidSignature(digest, abi.encodePacked(r, s, v)) == 0x1626ba7e);
    } else {
        address recoveredAddress = ecrecover(digest, v, r, s);
        require(recoveredAddress != address(0), 'Invalid signature');
        require(recoveredAddress == owner, 'Unauthorized');
    }
    _approve(spender, tokenId);
}
```

DOMAIN_SEPARATOR hesabinda: chainId (ChainId.get() ile assembly `chainid()` opcode), verifyingContract
adresi, name hash ('Uniswap V3 Positions NFT-V1') ve version ('1') kullanilir.

---

# BOLUM 7: QUOTERV2 VE QUOTER — REVERT TRICK MEKANIZMASI

## 7.1 Temel Prensip

QuoterV2 ve Quoter, swap simulasyonu yaparlar ancak gercek swap gerceklestirmezler. Mekanizma:
1. Gercek swap cagrisini `try...catch` ile baslatirlar
2. Callback icinde sonucu revert data'si icerisine yerlestirilir (assembly ile)
3. catch blogu revert data'yi parse eder ve sonucu dondurur

Bu yaklasim, EVM'de "view" olmayan ancak state degistirmeyen simulasyon imkani saglar.

## 7.2 QuoterV2.uniswapV3SwapCallback — Revert ile Veri Aktarimi

```solidity
function uniswapV3SwapCallback(
    int256 amount0Delta, int256 amount1Delta,
    bytes memory path
) external view override {
    require(amount0Delta > 0 || amount1Delta > 0);
    (address tokenIn, address tokenOut, uint24 fee) = path.decodeFirstPool();
    CallbackValidation.verifyCallback(factory, tokenIn, tokenOut, fee);

    (bool isExactInput, uint256 amountToPay, uint256 amountReceived) =
        amount0Delta > 0
            ? (tokenIn < tokenOut, uint256(amount0Delta), uint256(-amount1Delta))
            : (tokenOut < tokenIn, uint256(amount1Delta), uint256(-amount0Delta));

    IUniswapV3Pool pool = getPool(tokenIn, tokenOut, fee);
    (uint160 sqrtPriceX96After, int24 tickAfter, , , , , ) = pool.slot0();

    if (isExactInput) {
        assembly {
            let ptr := mload(0x40)
            mstore(ptr, amountReceived)
            mstore(add(ptr, 0x20), sqrtPriceX96After)
            mstore(add(ptr, 0x40), tickAfter)
            revert(ptr, 96)  // 3 x 32 byte = 96 byte revert data
        }
    } else {
        if (amountOutCached != 0) require(amountReceived == amountOutCached);
        assembly {
            let ptr := mload(0x40)
            mstore(ptr, amountToPay)
            mstore(add(ptr, 0x20), sqrtPriceX96After)
            mstore(add(ptr, 0x40), tickAfter)
            revert(ptr, 96)
        }
    }
}
```

Revert data layout (96 byte):
- [0-31]:  amount (amountReceived veya amountToPay)
- [32-63]: sqrtPriceX96After (swap sonrasi fiyat)
- [64-95]: tickAfter (swap sonrasi tick) — int24 ama 32 byte slot'a sigar

## 7.3 parseRevertReason

```solidity
function parseRevertReason(bytes memory reason)
    private pure
    returns (uint256 amount, uint160 sqrtPriceX96After, int24 tickAfter)
{
    if (reason.length != 96) {
        if (reason.length < 68) revert('Unexpected error');
        assembly { reason := add(reason, 0x04) }
        revert(abi.decode(reason, (string)));
    }
    return abi.decode(reason, (uint256, uint160, int24));
}
```

Parsing mantigi:
- 96 byte: basarili simulasyon → abi.decode ile 3 deger cikar
- 68 byte: standart Solidity require/revert mesaji (4 byte selector + 64 byte + string)
- Digerleri: beklenmedik hata

## 7.4 Quoter (V1) vs QuoterV2 Farki

Quoter (V1): Sadece `amountOut` dondurur (32 byte revert data)
```solidity
assembly {
    let ptr := mload(0x40)
    mstore(ptr, amountReceived)
    revert(ptr, 32)
}
```

QuoterV2: `amountOut + sqrtPriceX96After + tickAfter` dondurur (96 byte) ve ayrica:
- `initializedTicksCrossed`: kac initialized tick gecildigini sayar
- `gasEstimate`: swap gas tuketi tahmini verir
- `handleRevert` fonksiyonu ile `PoolTicksCounter.countInitializedTicksCrossed` cagrisindan
  tick sayisi hesaplanir

## 7.5 quoteExactOutputSingle — amountOutCached Guvenlik Mekanizmasi

```solidity
function quoteExactOutputSingle(QuoteExactOutputSingleParams memory params) public override returns (...) {
    bool zeroForOne = params.tokenIn < params.tokenOut;
    IUniswapV3Pool pool = getPool(params.tokenIn, params.tokenOut, params.fee);

    if (params.sqrtPriceLimitX96 == 0) amountOutCached = params.amount;  // cache
    uint256 gasBefore = gasleft();
    try pool.swap(
        address(this), zeroForOne, -params.amount.toInt256(),
        ...
        abi.encodePacked(params.tokenOut, params.fee, params.tokenIn)
    ) {} catch (bytes memory reason) {
        gasEstimate = gasBefore - gasleft();
        if (params.sqrtPriceLimitX96 == 0) delete amountOutCached;  // temizle
        return handleRevert(reason, pool, gasEstimate);
    }
}
```

amountOutCached, callback icinde `if (amountOutCached != 0) require(amountReceived == amountOutCached)`
ile gercek alim miktarinin istenen miktar ile uyumasini dogrular.

---

# BOLUM 8: LIQUIDITYMANAGEMENT BASE KONTRAT

## 8.1 uniswapV3MintCallback

```solidity
function uniswapV3MintCallback(
    uint256 amount0Owed,
    uint256 amount1Owed,
    bytes calldata data
) external override {
    MintCallbackData memory decoded = abi.decode(data, (MintCallbackData));
    CallbackValidation.verifyCallback(factory, decoded.poolKey);

    if (amount0Owed > 0) pay(decoded.poolKey.token0, decoded.payer, msg.sender, amount0Owed);
    if (amount1Owed > 0) pay(decoded.poolKey.token1, decoded.payer, msg.sender, amount1Owed);
}
```

MintCallbackData: `{ PoolAddress.PoolKey poolKey, address payer }` — kim odeyecek ve hangi havuzdan
callback geldi bilgisini icermektedir. verifyCallback ile sahte callback saldirisindan korunma saglanir.

## 8.2 addLiquidity

```solidity
function addLiquidity(AddLiquidityParams memory params)
    internal returns (uint128 liquidity, uint256 amount0, uint256 amount1, IUniswapV3Pool pool)
{
    PoolAddress.PoolKey memory poolKey = PoolAddress.PoolKey({...});
    pool = IUniswapV3Pool(PoolAddress.computeAddress(factory, poolKey));

    {
        (uint160 sqrtPriceX96, , , , , , ) = pool.slot0();
        uint160 sqrtRatioAX96 = TickMath.getSqrtRatioAtTick(params.tickLower);
        uint160 sqrtRatioBX96 = TickMath.getSqrtRatioAtTick(params.tickUpper);

        liquidity = LiquidityAmounts.getLiquidityForAmounts(
            sqrtPriceX96, sqrtRatioAX96, sqrtRatioBX96,
            params.amount0Desired, params.amount1Desired
        );
    }

    (amount0, amount1) = pool.mint(
        params.recipient, params.tickLower, params.tickUpper, liquidity,
        abi.encode(MintCallbackData({poolKey: poolKey, payer: msg.sender}))
    );

    require(amount0 >= params.amount0Min && amount1 >= params.amount1Min, 'Price slippage check');
}
```

---

# BOLUM 9: PERIPHERYPAYMENTS VE PERIPHERYPAYMENTSWITHFEE

## 9.1 PeripheryPayments.pay — Uclu Odeme Mekanizmasi

```solidity
function pay(address token, address payer, address recipient, uint256 value) internal {
    if (token == WETH9 && address(this).balance >= value) {
        // 1. Yerel ETH ile WETH olustur ve gonder
        IWETH9(WETH9).deposit{value: value}();
        IWETH9(WETH9).transfer(recipient, value);
    } else if (payer == address(this)) {
        // 2. Router'in elindeki tokenlar (multi-hop ara adim)
        TransferHelper.safeTransfer(token, recipient, value);
    } else {
        // 3. Kullanicidan direkt pull
        TransferHelper.safeTransferFrom(token, payer, recipient, value);
    }
}
```

3 odeme senaryosu:
1. Native ETH → WETH9: Sozlesme ETH bakiyesi varsa, WETH'e wrap edip gonder
2. Router kendi tokenindan gonder: Ara swap adimlarinda
3. Kullanicidan transferFrom: Ilk adimda kullanicinin onceden approve ettigi token

## 9.2 PeripheryPaymentsWithFee — Fee Hesaplama Formulu

```solidity
function unwrapWETH9WithFee(uint256 amountMinimum, address recipient, uint256 feeBips, address feeRecipient)
    public payable override
{
    require(feeBips > 0 && feeBips <= 100);
    uint256 balanceWETH9 = IWETH9(WETH9).balanceOf(address(this));
    require(balanceWETH9 >= amountMinimum, 'Insufficient WETH9');
    if (balanceWETH9 > 0) {
        IWETH9(WETH9).withdraw(balanceWETH9);
        uint256 feeAmount = balanceWETH9.mul(feeBips) / 10_000;
        if (feeAmount > 0) TransferHelper.safeTransferETH(feeRecipient, feeAmount);
        TransferHelper.safeTransferETH(recipient, balanceWETH9 - feeAmount);
    }
}
```

Fee formulu: `feeAmount = balanceWETH9 * feeBips / 10000`
feeBips siniri: 1-100 bips arasi (0.01% - 1%)

---

# BOLUM 10: ORACLELIBRARY — TWAP FORMULLERI

## 10.1 consult — Aritmetik Ortalama Tick ve Harmonik Ortalama Likidite

```solidity
function consult(address pool, uint32 secondsAgo)
    internal view returns (int24 arithmeticMeanTick, uint128 harmonicMeanLiquidity)
{
    require(secondsAgo != 0, 'BP');
    uint32[] memory secondsAgos = new uint32[](2);
    secondsAgos[0] = secondsAgo;
    secondsAgos[1] = 0;

    (int56[] memory tickCumulatives, uint160[] memory secondsPerLiquidityCumulativeX128s) =
        IUniswapV3Pool(pool).observe(secondsAgos);

    int56 tickCumulativesDelta = tickCumulatives[1] - tickCumulatives[0];
    uint160 secondsPerLiquidityCumulativesDelta =
        secondsPerLiquidityCumulativeX128s[1] - secondsPerLiquidityCumulativeX128s[0];

    arithmeticMeanTick = int24(tickCumulativesDelta / secondsAgo);
    if (tickCumulativesDelta < 0 && (tickCumulativesDelta % secondsAgo != 0))
        arithmeticMeanTick--;

    uint192 secondsAgoX160 = uint192(secondsAgo) * type(uint160).max;
    harmonicMeanLiquidity = uint128(
        secondsAgoX160 / (uint192(secondsPerLiquidityCumulativesDelta) << 32)
    );
}
```

TWAP formulleri:
- Aritmetik ortalama tick: `(tickCumulatives[now] - tickCumulatives[t_ago]) / secondsAgo`
- Harmonik ortalama likidite: `secondsAgo * 2^160 / (secondsPerLiquidity_delta * 2^32)`

Rounding: Negatif tick cumulative icin her zaman asagi yuvarlama uygulanir (`--` ile).

## 10.2 getQuoteAtTick

```solidity
function getQuoteAtTick(int24 tick, uint128 baseAmount, address baseToken, address quoteToken)
    internal pure returns (uint256 quoteAmount)
{
    uint160 sqrtRatioX96 = TickMath.getSqrtRatioAtTick(tick);
    if (sqrtRatioX96 <= type(uint128).max) {
        uint256 ratioX192 = uint256(sqrtRatioX96) * sqrtRatioX96;
        quoteAmount = baseToken < quoteToken
            ? FullMath.mulDiv(ratioX192, baseAmount, 1 << 192)
            : FullMath.mulDiv(1 << 192, baseAmount, ratioX192);
    } else {
        uint256 ratioX128 = FullMath.mulDiv(sqrtRatioX96, sqrtRatioX96, 1 << 64);
        quoteAmount = baseToken < quoteToken
            ? FullMath.mulDiv(ratioX128, baseAmount, 1 << 128)
            : FullMath.mulDiv(1 << 128, baseAmount, ratioX128);
    }
}
```

Formul: `price = sqrtRatioX96^2 / 2^192 = ratioX192 / 2^192`
- baseToken < quoteToken (token0 → token1): `quoteAmount = ratioX192 * baseAmount / 2^192`
- baseToken > quoteToken (token1 → token0): `quoteAmount = 2^192 * baseAmount / ratioX192`

Overflow Onleme: sqrtRatioX96 > 2^128 olursa kare alma overflow'a neden olabilir, bu durumda
daha dusuk hassasiyetli ratioX128 formati kullanilir.

---

# BOLUM 11: POSITIONVALUE KUTUPHANESI — POZiSYON DEGER HESAPLAMA

## 11.1 total, principal, fees

```solidity
function total(INonfungiblePositionManager positionManager, uint256 tokenId, uint160 sqrtRatioX96)
    internal view returns (uint256 amount0, uint256 amount1)
{
    (uint256 amount0Principal, uint256 amount1Principal) = principal(positionManager, tokenId, sqrtRatioX96);
    (uint256 amount0Fee, uint256 amount1Fee) = fees(positionManager, tokenId);
    return (amount0Principal + amount0Fee, amount1Principal + amount1Fee);
}
```

- `principal`: LiquidityAmounts.getAmountsForLiquidity ile mevcut fiyatta likidite degeri
- `fees`: birikmis ancak henuz cekilmemis ucretler

## 11.2 _getFeeGrowthInside — Ucret Buyume Hesabinin ic Mekanizmasi

```solidity
function _getFeeGrowthInside(IUniswapV3Pool pool, int24 tickLower, int24 tickUpper)
    private view returns (uint256 feeGrowthInside0X128, uint256 feeGrowthInside1X128)
{
    (, int24 tickCurrent, , , , , ) = pool.slot0();
    (, , uint256 lowerFeeGrowthOutside0X128, uint256 lowerFeeGrowthOutside1X128, , , , ) = pool.ticks(tickLower);
    (, , uint256 upperFeeGrowthOutside0X128, uint256 upperFeeGrowthOutside1X128, , , , ) = pool.ticks(tickUpper);

    if (tickCurrent < tickLower) {
        feeGrowthInside0X128 = lowerFeeGrowthOutside0X128 - upperFeeGrowthOutside0X128;
        feeGrowthInside1X128 = lowerFeeGrowthOutside1X128 - upperFeeGrowthOutside1X128;
    } else if (tickCurrent < tickUpper) {
        uint256 feeGrowthGlobal0X128 = pool.feeGrowthGlobal0X128();
        uint256 feeGrowthGlobal1X128 = pool.feeGrowthGlobal1X128();
        feeGrowthInside0X128 = feeGrowthGlobal0X128 - lowerFeeGrowthOutside0X128 - upperFeeGrowthOutside0X128;
        feeGrowthInside1X128 = feeGrowthGlobal1X128 - lowerFeeGrowthOutside1X128 - upperFeeGrowthOutside1X128;
    } else {
        feeGrowthInside0X128 = upperFeeGrowthOutside0X128 - lowerFeeGrowthOutside0X128;
        feeGrowthInside1X128 = upperFeeGrowthOutside1X128 - lowerFeeGrowthOutside1X128;
    }
}
```

3 durum:
1. Fiyat aralik altinda: fee growth inside = lower.outside - upper.outside
2. Fiyat araligi icinde: fee growth inside = global - lower.outside - upper.outside
3. Fiyat aralik ustunde: fee growth inside = upper.outside - lower.outside

---

# BOLUM 12: SQRTPRICEMATHPARTIAL

## 12.1 getAmount0Delta

```solidity
function getAmount0Delta(
    uint160 sqrtRatioAX96, uint160 sqrtRatioBX96,
    uint128 liquidity, bool roundUp
) internal pure returns (uint256 amount0)
{
    if (sqrtRatioAX96 > sqrtRatioBX96) (sqrtRatioAX96, sqrtRatioBX96) = (sqrtRatioBX96, sqrtRatioAX96);
    uint256 numerator1 = uint256(liquidity) << FixedPoint96.RESOLUTION;  // L * 2^96
    uint256 numerator2 = sqrtRatioBX96 - sqrtRatioAX96;
    require(sqrtRatioAX96 > 0);
    return roundUp
        ? UnsafeMath.divRoundingUp(
            FullMath.mulDivRoundingUp(numerator1, numerator2, sqrtRatioBX96),
            sqrtRatioAX96)
        : FullMath.mulDiv(numerator1, numerator2, sqrtRatioBX96) / sqrtRatioAX96;
}
```

Formul:
```
amount0 = L * 2^96 * (sqrtPb - sqrtPa) / sqrtPb / sqrtPa
        = L * (sqrtPb - sqrtPa) / (sqrtPa * sqrtPb / 2^96)
        = L / sqrtPa - L / sqrtPb   [raw Q96 degerler normalized edildiginde]
```

## 12.2 getAmount1Delta

```solidity
function getAmount1Delta(
    uint160 sqrtRatioAX96, uint160 sqrtRatioBX96,
    uint128 liquidity, bool roundUp
) internal pure returns (uint256 amount1)
{
    ...
    return roundUp
        ? FullMath.mulDivRoundingUp(liquidity, sqrtRatioBX96 - sqrtRatioAX96, FixedPoint96.Q96)
        : FullMath.mulDiv(liquidity, sqrtRatioBX96 - sqrtRatioAX96, FixedPoint96.Q96);
}
```

Formul: `amount1 = L * (sqrtPb - sqrtPa) / 2^96`

---

# BOLUM 13: POOLINITIALIZER

```solidity
function createAndInitializePoolIfNecessary(
    address token0, address token1, uint24 fee, uint160 sqrtPriceX96
) external payable override returns (address pool)
{
    require(token0 < token1);
    pool = IUniswapV3Factory(factory).getPool(token0, token1, fee);
    if (pool == address(0)) {
        pool = IUniswapV3Factory(factory).createPool(token0, token1, fee);
        IUniswapV3Pool(pool).initialize(sqrtPriceX96);
    } else {
        (uint160 sqrtPriceX96Existing, , , , , , ) = IUniswapV3Pool(pool).slot0();
        if (sqrtPriceX96Existing == 0) {
            IUniswapV3Pool(pool).initialize(sqrtPriceX96);
        }
    }
}
```

Idempotent: Havuz yoksa olusturur, var ama baslatilmamissa baslatir, var ve baslatilmissa hic bir sey yapmaz.
token0 < token1 zorunlulugu: PoolAddress.computeAddress ile uyumlu siralama.

---

# BOLUM 14: TICKLENS

```solidity
function getPopulatedTicksInWord(address pool, int16 tickBitmapIndex)
    public view override returns (PopulatedTick[] memory populatedTicks)
{
    uint256 bitmap = IUniswapV3Pool(pool).tickBitmap(tickBitmapIndex);
    uint256 numberOfPopulatedTicks;
    for (uint256 i = 0; i < 256; i++) {
        if (bitmap & (1 << i) > 0) numberOfPopulatedTicks++;
    }
    int24 tickSpacing = IUniswapV3Pool(pool).tickSpacing();
    populatedTicks = new PopulatedTick[](numberOfPopulatedTicks);
    for (uint256 i = 0; i < 256; i++) {
        if (bitmap & (1 << i) > 0) {
            int24 populatedTick = ((int24(tickBitmapIndex) << 8) + int24(i)) * tickSpacing;
            (uint128 liquidityGross, int128 liquidityNet, , , , , , ) = IUniswapV3Pool(pool).ticks(populatedTick);
            populatedTicks[--numberOfPopulatedTicks] = PopulatedTick({
                tick: populatedTick, liquidityNet: liquidityNet, liquidityGross: liquidityGross
            });
        }
    }
}
```

Tick bitmap indeksleme: tick / tickSpacing / 256 = wordPos, bit = (tick/tickSpacing) % 256
populatedTick = (wordPos << 8 + bitPos) * tickSpacing

---

# BOLUM 15: V3MIGRATOR — V2'DEN V3'E GEC

## 15.1 migrate Fonksiyonu

```solidity
function migrate(MigrateParams calldata params) external override {
    require(params.percentageToMigrate > 0, 'Percentage too small');
    require(params.percentageToMigrate <= 100, 'Percentage too large');

    // V2 LP token'larini yak
    IUniswapV2Pair(params.pair).transferFrom(msg.sender, params.pair, params.liquidityToMigrate);
    (uint256 amount0V2, uint256 amount1V2) = IUniswapV2Pair(params.pair).burn(address(this));

    // Migrate edilecek miktar hesapla
    uint256 amount0V2ToMigrate = amount0V2.mul(params.percentageToMigrate) / 100;
    uint256 amount1V2ToMigrate = amount1V2.mul(params.percentageToMigrate) / 100;

    // V3 pozisyon ac
    TransferHelper.safeApprove(params.token0, nonfungiblePositionManager, amount0V2ToMigrate);
    TransferHelper.safeApprove(params.token1, nonfungiblePositionManager, amount1V2ToMigrate);

    (, , uint256 amount0V3, uint256 amount1V3) = INonfungiblePositionManager(nonfungiblePositionManager).mint(
        INonfungiblePositionManager.MintParams({...})
    );

    // Kalan miktar iade
    if (amount0V3 < amount0V2) {
        if (amount0V3 < amount0V2ToMigrate) TransferHelper.safeApprove(params.token0, nonfungiblePositionManager, 0);
        uint256 refund0 = amount0V2 - amount0V3;
        if (params.refundAsETH && params.token0 == WETH9) {
            IWETH9(WETH9).withdraw(refund0);
            TransferHelper.safeTransferETH(msg.sender, refund0);
        } else {
            TransferHelper.safeTransfer(params.token0, msg.sender, refund0);
        }
    }
}
```

MigrateParams:
- pair: V2 havuz adresi
- liquidityToMigrate: V2 LP token miktari
- percentageToMigrate: yuzde (1-100), kismi migrasyon destegi
- refundAsETH: kalan WETH9 ETH olarak iade edilsin mi?

---

# BOLUM 16: PAIRFLASH — FLASH LOAN ORNEK UYGULAMASI

## 16.1 Flash Loan Akisi

```solidity
function initFlash(FlashParams memory params) external {
    PoolAddress.PoolKey memory poolKey = PoolAddress.PoolKey({
        token0: params.token0, token1: params.token1, fee: params.fee1
    });
    IUniswapV3Pool pool = IUniswapV3Pool(PoolAddress.computeAddress(factory, poolKey));

    pool.flash(
        address(this),          // alici
        params.amount0,         // token0 odunc
        params.amount1,         // token1 odunc
        abi.encode(FlashCallbackData({
            amount0: params.amount0, amount1: params.amount1,
            payer: msg.sender, poolKey: poolKey,
            poolFee2: params.fee2, poolFee3: params.fee3
        }))
    );
}
```

## 16.2 uniswapV3FlashCallback — Arbitraj Mantigi

```solidity
function uniswapV3FlashCallback(uint256 fee0, uint256 fee1, bytes calldata data) external override {
    FlashCallbackData memory decoded = abi.decode(data, (FlashCallbackData));
    CallbackValidation.verifyCallback(factory, decoded.poolKey);

    address token0 = decoded.poolKey.token0;
    address token1 = decoded.poolKey.token1;

    uint256 amount0Min = LowGasSafeMath.add(decoded.amount0, fee0);  // geri odeme minimum
    uint256 amount1Min = LowGasSafeMath.add(decoded.amount1, fee1);

    // Arbitraj swaplari: token1 -> token0 (pool fee2'de)
    TransferHelper.safeApprove(token1, address(swapRouter), decoded.amount1);
    uint256 amountOut0 = swapRouter.exactInputSingle(ISwapRouter.ExactInputSingleParams({
        tokenIn: token1, tokenOut: token0, fee: decoded.poolFee2,
        recipient: address(this), deadline: block.timestamp,
        amountIn: decoded.amount1,
        amountOutMinimum: amount0Min,
        sqrtPriceLimitX96: 0
    }));

    // Arbitraj swaplari: token0 -> token1 (pool fee3'de)
    TransferHelper.safeApprove(token0, address(swapRouter), decoded.amount0);
    uint256 amountOut1 = swapRouter.exactInputSingle(ISwapRouter.ExactInputSingleParams({
        tokenIn: token0, tokenOut: token1, fee: decoded.poolFee3,
        recipient: address(this), deadline: block.timestamp,
        amountIn: decoded.amount0,
        amountOutMinimum: amount1Min,
        sqrtPriceLimitX96: 0
    }));

    // Flash loan geri odemesi
    if (amount0Min > 0) pay(token0, address(this), msg.sender, amount0Min);
    if (amount1Min > 0) pay(token1, address(this), msg.sender, amount1Min);

    // Kar ise payer'a
    if (amountOut0 > amount0Min) pay(token0, address(this), decoded.payer, amountOut0 - amount0Min);
    if (amountOut1 > amount1Min) pay(token1, address(this), decoded.payer, amountOut1 - amount1Min);
}
```

Flash loan akis sirasi:
1. `initFlash` → havuz `flash()` cagrilir
2. Core pool tokenlari gonderir ve callback cagrilar
3. Callback: token1 ile token0 satın al (farkli fee havuzunda)
4. Callback: token0 ile token1 satın al (farkli fee havuzunda)
5. Anapara + fee geri odenir
6. Artan kar payer'a aktarilir

Guvenlik: `verifyCallback` ile sahte callback onlenir, `amountOutMinimum` ile arbitraj
karliliginin saglanmasi zorunlu kilinir.

FlashCallbackData struct:
```solidity
struct FlashCallbackData {
    uint256 amount0;              // odunc alinan token0 miktari
    uint256 amount1;              // odunc alinan token1 miktari
    address payer;                // kari alacak adres
    PoolAddress.PoolKey poolKey;  // kaynak havuz
    uint24 poolFee2;              // ilk arbitraj havuzu fee
    uint24 poolFee3;              // ikinci arbitraj havuzu fee
}
```

---

# BOLUM 17: NONFUNGIBLLETOKENPOSITIONDESCRIPTOR — NFT METADATA

## 17.1 tokenRatioPriority — Token Siralama

```solidity
address private constant DAI  = 0x6B175474E89094C44Da98b954EedeAC495271d0F;
address private constant USDC = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
address private constant USDT = 0xdAC17F958D2ee523a2206206994597C13D831ec7;
address private constant TBTC = 0x8dAEBADE922dF735c38C80C7eBD708Af50815fAa;
address private constant WBTC = 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599;

function tokenRatioPriority(address token, uint256 chainId) public view returns (int256) {
    if (token == WETH9) return TokenRatioSortOrder.DENOMINATOR; // -100
    if (chainId == 1) {
        if (token == USDC) return TokenRatioSortOrder.NUMERATOR_MOST;  // 300
        else if (token == USDT) return TokenRatioSortOrder.NUMERATOR_MORE; // 200
        else if (token == DAI)  return TokenRatioSortOrder.NUMERATOR;   // 100
        else if (token == TBTC) return TokenRatioSortOrder.DENOMINATOR_MORE; // -200
        else if (token == WBTC) return TokenRatioSortOrder.DENOMINATOR_MOST; // -300
    }
    return 0;
}
```

TokenRatioSortOrder sabitleri:
```solidity
int256 constant NUMERATOR_MOST  = 300;   // pay tarafinda en oncelikli
int256 constant NUMERATOR_MORE  = 200;
int256 constant NUMERATOR       = 100;
int256 constant DENOMINATOR_MOST = -300;  // payda tarafinda en oncelikli
int256 constant DENOMINATOR_MORE = -200;
int256 constant DENOMINATOR     = -100;
```

flipRatio mantigi: Eger token0'in onceligi > token1'in onceligi ise oran cevirilir, boylece
NFT gorselinde her zaman "daha onemli" token pay pozisyonunda gosterilir (USDC/ETH yerine ETH/USDC).

## 17.2 NFT SVG Olusturma

NFTSVG.generateSVG → SVGParams alir ve asagidakileri birlestirir:
- generateSVGDefs: SVG filtreler, clip-path, gradient'lar
- generateSVGBorderText: Donen animasyonlu token adresi ve sembol
- generateSVGCardMantle: Buyuk token sembol ve fee tier metni
- generageSvgCurve: Tick araligina gore egri (8 farkli Bezier egri secenegi)
- generateSVGPositionDataAndLocationCurve: Token ID, alt/ust tick
- generateSVGRareSparkle: Nadir NFT'ler icin doner yildiz animasyonu

isRare: `uint256(keccak256(abi.encodePacked(tokenId, poolAddress))) < type(uint256).max / (1 + mostSignificantBit(tokenId) * 2)`

getCurve — Tick araligina gore egri secimi:
```solidity
int24 tickRange = (tickUpper - tickLower) / tickSpacing;
if (tickRange <= 4)   curve = curve1; // 'M1 1C41 41 105 105 145 145'
if (tickRange <= 8)   curve = curve2;
if (tickRange <= 16)  curve = curve3;
...
if (tickRange > 256)  curve = curve8; // 'M1 1C1 97 49 145 145 145'
```

---

# BOLUM 18: MULTICALL VE SELFPERMIT

## 18.1 Multicall

```solidity
function multicall(bytes[] calldata data) public payable override returns (bytes[] memory results) {
    results = new bytes[](data.length);
    for (uint256 i = 0; i < data.length; i++) {
        (bool success, bytes memory result) = address(this).delegatecall(data[i]);
        if (!success) {
            if (result.length < 68) revert();
            assembly { result := add(result, 0x04) }
            revert(abi.decode(result, (string)));
        }
        results[i] = result;
    }
}
```

delegatecall ile kendi kendini cagirma: context (storage, msg.sender, value) korunur.
Revert yayilimi: Ic cagrilar revert ederse, hata mesaji decode edilerek dis cagri da revert eder.

## 18.2 SelfPermit

EIP-2612 (`IERC20Permit.permit`) ve DAI-style (`IERC20PermitAllowed.permit`) destegi:
- `selfPermit`: standart EIP-2612 — belirli miktar icin approve
- `selfPermitIfNecessary`: mevcut allowance yeterliyse skip
- `selfPermitAllowed`: DAI/CHAI style — boolean "allowed" parametreli
- `selfPermitAllowedIfNecessary`: allowance < max.uint256 ise permit cagrir

---

# BOLUM 19: TEST KONTRATLAR

## 19.1 MockTimeNonfungiblePositionManager ve MockTimeSwapRouter

```solidity
contract MockTimeNonfungiblePositionManager is NonfungiblePositionManager {
    uint256 time;
    function _blockTimestamp() internal view override returns (uint256) { return time; }
    function setTime(uint256 _time) external { time = _time; }
}
```

BlockTimestamp'in sanal (virtual) olarak tanimlanmasinin amaci: Test ortaminda zaman kontrol edilebilinmesi.
Deadline ve permit testleri icin zaman ileri sarma imkani saglar.

## 19.2 MockObservable ve MockObservations

OracleLibrary.consult ve getBlockStartingTickAndLiquidity fonksiyonlarinin test edilmesi icin
sarmalayici mock kontratlar. 2 gozlem noktalari onceden belirlenerek pool.observe() simule edilir.

## 19.3 LiquidityAmountsTest, PathTest, OracleTest

Tum bu test kontratlar, kutuphanelerin internal pure/view fonksiyonlarini public olarak acar:
- Gas olcumu (`getGasCostOf*` fonksiyonlar): `gasBefore - gasleft()` ile
- Fonksiyon sonuclari test edilebilir hale gelir

---

# BOLUM 20: POSITIONKEY VE TRANSFERHELPER

## 20.1 PositionKey.compute

```solidity
function compute(address owner, int24 tickLower, int24 tickUpper) internal pure returns (bytes32) {
    return keccak256(abi.encodePacked(owner, tickLower, tickUpper));
}
```

Core'da her pozisyon `keccak256(owner ++ tickLower ++ tickUpper)` ile indexlenir.
NPM icin owner her zaman NPM'in kendi adresidir (address(this)).

## 20.2 TransferHelper

```solidity
function safeTransferFrom(address token, address from, address to, uint256 value) internal {
    (bool success, bytes memory data) =
        token.call(abi.encodeWithSelector(IERC20.transferFrom.selector, from, to, value));
    require(success && (data.length == 0 || abi.decode(data, (bool))), 'STF');
}
```

`data.length == 0 || abi.decode(data, (bool))`: USDT gibi bool dondurmeyen ERC20'leri destekler.
Hata kodlari: STF (safeTransferFrom), ST (safeTransfer), SA (safeApprove), STE (safeTransferETH)

---

# BOLUM 21: HEXSTRINGS VE CHAINID KUTUPHANELERI

## 21.1 HexStrings

```solidity
bytes16 internal constant ALPHABET = '0123456789abcdef';

function toHexString(uint256 value, uint256 length) internal pure returns (string memory) {
    bytes memory buffer = new bytes(2 * length + 2);
    buffer[0] = '0'; buffer[1] = 'x';
    for (uint256 i = 2 * length + 1; i > 1; --i) {
        buffer[i] = ALPHABET[value & 0xf];
        value >>= 4;
    }
    require(value == 0, 'Strings: hex length insufficient');
    return string(buffer);
}

function toHexStringNoPrefix(uint256 value, uint256 length) internal pure returns (string memory) {
    bytes memory buffer = new bytes(2 * length);
    for (uint256 i = buffer.length; i > 0; i--) {
        buffer[i - 1] = ALPHABET[value & 0xf];
        value >>= 4;
    }
    return string(buffer);
}
```

NFT SVG'de renk kodlari: `tokenToColorHex(uint256(tokenAddress), 136)` ile adresin farkli offset
bit segmentlerinden 3 farkli RGB rengi cikarilir.

## 21.2 ChainId

```solidity
function get() internal pure returns (uint256 chainId) {
    assembly { chainId := chainid() }
}
```

EVM'de `chainid()` opcode'u (EIP-1344 ile eklenmistir, Solidity 0.5.12+).
ERC721Permit'in DOMAIN_SEPARATOR hesabinda kullanilir.

---

# BOLUM 22: POOLTICKCOUNTER

```solidity
function countInitializedTicksCrossed(IUniswapV3Pool self, int24 tickBefore, int24 tickAfter)
    internal view returns (uint32 initializedTicksCrossed)
{
    int16 wordPosLower; int16 wordPosHigher;
    uint8 bitPosLower; uint8 bitPosHigher;
    bool tickBeforeInitialized; bool tickAfterInitialized;

    {
        int16 wordPos = int16((tickBefore / self.tickSpacing()) >> 8);
        uint8 bitPos = uint8((tickBefore / self.tickSpacing()) % 256);
        int16 wordPosAfter = int16((tickAfter / self.tickSpacing()) >> 8);
        uint8 bitPosAfter = uint8((tickAfter / self.tickSpacing()) % 256);

        tickAfterInitialized = ((self.tickBitmap(wordPosAfter) & (1 << bitPosAfter)) > 0) &&
            ((tickAfter % self.tickSpacing()) == 0) && (tickBefore > tickAfter);
        tickBeforeInitialized = ((self.tickBitmap(wordPos) & (1 << bitPos)) > 0) &&
            ((tickBefore % self.tickSpacing()) == 0) && (tickBefore < tickAfter);
        ...
    }

    uint256 mask = type(uint256).max << bitPosLower;
    while (wordPosLower <= wordPosHigher) {
        if (wordPosLower == wordPosHigher) mask = mask & (type(uint256).max >> (255 - bitPosHigher));
        uint256 masked = self.tickBitmap(wordPosLower) & mask;
        initializedTicksCrossed += countOneBits(masked);
        wordPosLower++;
        mask = type(uint256).max;
    }
    ...
}
```

QuoterV2'nin gas tahmini icin kac initialized tick gecildigini sayar. Her initialized tick gecisi
ek gas maliyeti olusturur (tick crossing = likidite guncelleme).

countOneBits: Brian Kernighan algoritmasi ile bit sayimi: `x &= (x-1)` ile her iterasyonda en sag 1 biti siler.

---

# BOLUM 23: BYTESLIB — DUSUK SEVIYE BAYT KUTUPHANESI

```solidity
function toAddress(bytes memory _bytes, uint256 _start) internal pure returns (address) {
    require(_start + 20 >= _start, 'toAddress_overflow');
    require(_bytes.length >= _start + 20, 'toAddress_outOfBounds');
    address tempAddress;
    assembly {
        tempAddress := div(mload(add(add(_bytes, 0x20), _start)), 0x1000000000000000000000000)
    }
    return tempAddress;
}
```

`mload` 32 byte okur, `div` ile soldan 12 byte atilir (160 bit adres elde edilir):
`0x1000000000000000000000000 = 2^96 = 16^24`

`toUint24`: `mload(add(add(_bytes, 0x3), _start))` — 32 byte'i okuyup en sag 3 byte'i alir
(Solidity'nin uint24'e cevirimi otomatik olarak yuksek bitleri atar).

---

# BOLUM 24: ARAYUZLER (INTERFACES) OZETI

## ISwapRouter — Struct Imzalari

ExactInputSingleParams: tokenIn, tokenOut, fee(uint24), recipient, deadline, amountIn, amountOutMinimum, sqrtPriceLimitX96
ExactInputParams: path(bytes), recipient, deadline, amountIn, amountOutMinimum
ExactOutputSingleParams: tokenIn, tokenOut, fee, recipient, deadline, amountOut, amountInMaximum, sqrtPriceLimitX96
ExactOutputParams: path(bytes), recipient, deadline, amountOut, amountInMaximum

## INonfungiblePositionManager — Struct Imzalari

MintParams: token0, token1, fee, tickLower, tickUpper, amount0Desired, amount1Desired, amount0Min, amount1Min, recipient, deadline
IncreaseLiquidityParams: tokenId, amount0Desired, amount1Desired, amount0Min, amount1Min, deadline
DecreaseLiquidityParams: tokenId, liquidity(uint128), amount0Min, amount1Min, deadline
CollectParams: tokenId, recipient, amount0Max(uint128), amount1Max(uint128)

## IQuoterV2 — Ek Cikis Degerleri

quoteExactInput → (amountOut, sqrtPriceX96AfterList[], initializedTicksCrossedList[], gasEstimate)
quoteExactInputSingle → (amountOut, sqrtPriceX96After, initializedTicksCrossed, gasEstimate)

## IV3Migrator

MigrateParams: pair(V2), liquidityToMigrate, percentageToMigrate(uint8 1-100), token0, token1, fee,
tickLower, tickUpper, amount0Min, amount1Min, recipient, deadline, refundAsETH

## ITickLens

PopulatedTick: { int24 tick, int128 liquidityNet, uint128 liquidityGross }

## IERC721Permit

PERMIT_TYPEHASH: `keccak256("Permit(address spender,uint256 tokenId,uint256 nonce,uint256 deadline)")`
= 0x49ecf333e5b8c95c40fdafc95c1ad136e8914a8fb55e9dc8bb01eaa83a2df9ad

## IWETH9

deposit() payable + withdraw(uint256) — ETH/WETH donusum arayuzu

## IERC1271

isValidSignature(bytes32 hash, bytes memory signature) → bytes4 magic value 0x1626ba7e
Smart wallet/multisig imza dogrulamasi icin kullanilir.

## IERC20PermitAllowed

DAI/CHAI tarzinda permit: `permit(holder, spender, nonce, expiry, bool allowed, v, r, s)`
bool allowed = true → type(uint256).max approve, false → 0 approve

---

# BOLUM 25: UniswapInterfaceMulticall

```solidity
struct Call {
    address target;
    uint256 gasLimit;
    bytes callData;
}
struct Result {
    bool success;
    uint256 gasUsed;
    bytes returnData;
}

function multicall(Call[] memory calls) public returns (uint256 blockNumber, Result[] memory returnData) {
    blockNumber = block.number;
    for (uint256 i = 0; i < calls.length; i++) {
        (address target, uint256 gasLimit, bytes memory callData) =
            (calls[i].target, calls[i].gasLimit, calls[i].callData);
        uint256 gasLeftBefore = gasleft();
        (bool success, bytes memory ret) = target.call{gas: gasLimit}(callData);
        uint256 gasUsed = gasLeftBefore - gasleft();
        returnData[i] = Result(success, gasUsed, ret);
    }
}
```

Multicall.sol'dan farkli olarak:
- Herhangi bir hedef kontrat cagrilabilir (sadece `address(this)` degil)
- Per-call gas limiti belirtilebilir
- Basarisiz cagrilar revert etmez, `success = false` olarak isaretlenir
- `gasUsed` her cagri icin olculur

---

# BOLUM 26: NOVA SHIELD CHAIN DEX DEGERLENDIRMESI

## 26.1 L1 DEX'te Periphery Mimarisi Gerekli mi?

Uniswap V3'un periphery mimarisinin temel amaci, EVM'in kositliklarindan (callback pattern, tek
token approve mekanizmasi) kaynaklanan karmasikligi kullanicilardan soyutlamaktir.

Nova Shield Chain L1 DEX'i icin degerlendirme:

GEREKSIZ OLABILECEK BILESENLER:
1. WETH9 Donusum katmani: Eger L1'de native token direkt olarak swap islemine katilabildiyse,
   WETH'in wrap/unwrap akisi elimine edilebilir. Uniswap'ta bu katman EVM'in ETH'i ERC20 olarak
   gorememesinden kaynaklanir.
2. ERC721 Pozisyon NFT: Eger L1'in yerel veri tipleri fungible olmayan pozisyonlari dogrudan
   destekliyorsa, bu soyutlamaya ihtiyac kalmaz.
3. CREATE2 adresi hesaplama: Eger L1'de havuz kayit mekanizmasi farkliysa (hash yerine mapping),
   CallbackValidation tamamen farkli sekilde calisir.

MUHTEMELEN GEREKLI OLACAK BILESENLER:
1. Multi-hop routing mantigi: Fiyat iyilestirmesi icin coklu havuz uzerinden routing, L1 DEX'te
   de degerliyken Path kutuphanesi aynen uyarlanabilir.
2. Slippage koruma mekanizmasi: amountOutMinimum / amountInMaximum kontrolu evrenseldir.
3. Quoter/QuoterV2 mantigi: Revert-trick yaklasimi, simulasyon icin L1'de de kullanilabilir
   ancak daha okunakli bir yaklasim (L1'e ozel state simulation) tercih edilebilir.
4. LiquidityAmounts formulleri: Matematiksel olarak evrenseldir ve L1 DEX'e dogrudan tasiniabilir.
5. Deadline kontrolu: MEV ve stale transaction onleme icin evrensel guvenlik mekanizmasi.

FARKLILIK GEREKTIRECEK BILESENLER:
1. PoolAddress.computeAddress: POOL_INIT_CODE_HASH L1 havuz bytecode'una gore degistirilmeli.
2. PeripheryPayments.pay: Native token sarmala mantigi L1'e gore uyarlanmali.
3. ERC721Permit: ChainId entegrasyonu L1 chain ID'si ile guncellenmeli.

## 26.2 Multi-hop Routing L1'de Nasil Uygulanabilir?

Path kutuphanesinin encode formati (`[token:20][fee:3][token:20]...`) L1'de aynen kullanilabilir
ancak fee boyutu ve encoding bitleri L1'in kendi fee tiplerine gore adapte edilebilir:

```
// L1 icin potansiyel format (fee uint16 ise):
ADDR_SIZE = 20;     // adres uzunlugu
FEE_SIZE = 2;       // uint16 fee (v3'de 3 byte = uint24)
NEXT_OFFSET = 22;   // 20 + 2
POP_OFFSET = 42;    // 22 + 20
```

Multi-hop akis mantigi degismez:
- Ara hop recipient = address(router)
- Son hop recipient = gercek alici
- Payer degisimi: kullanici → router (ilk hop sonrasi)

## 26.3 Flash Loan L1'de Nasil Uygulanabilir?

PairFlash ornek kodu, L1 DEX'teki flash loan uygulama mimarisi icin dogrudan referans alinabilir.
Temel desen:

1. `initFlash` → havuz `flash()` cagir
2. Core havuz: token gonder + callback cagir
3. Callback: arbitraj logigi + geri odeme
4. `CallbackValidation.verifyCallback` benzeri guvenlik zorunlu

L1 farki: Eger L1'de native flash loan destegi yoksa, flash loan mantigi farkli sekilde implemente
edilebilir (pool-internal gibi). Eger core L1 DEX'te flash callback'i destekliyorsa, periphery
kontrati PairFlash patternini birebir taklit edebilir.

Kritik guvenlik notu: Flash callback'te `verifyCallback` mutlaka kullanilmali, aksi halde
herhangi bir kontrat callback'i taklit ederek token transferini tetikleyebilir.

## 26.4 Ozet: Nova Shield DEX Mimarisi Onerileri

1. CALLBACK GUVENLIK PATTERN: SwapRouter ve LiquidityManagement'in kullandigi
   `CallbackValidation.verifyCallback` → `require(msg.sender == computeAddress(factory, poolKey))`
   pattern'i L1 DEX'e aynen tasinalabilir ve tasimasi kritik guvenlik gereksinimidir.

2. FORMUL KUTUPHANELERI: LiquidityAmounts, SqrtPriceMathPartial, OracleLibrary'nin formulleri
   matematiksel olarak evrenseldir. Q64.96 fixed-point aritmetigi L1'de de kullanilabilir.

3. NFT POZISYON TASARIMI: Eger L1 DEX'te concentrated liquidity kullanilacaksa, NonfungiblePositionManager
   pattern'i (poolId optimizasyonu dahil) verimli storage yonetimi icin referans alinabilir.
   tokensOwed birikimi icin fee growth snapshot mekanizmasi da aynen uygulanabilir.

---

# BOLUM 27: TAM DOSYA REFERANS TABLOSU

| # | Dosya | Tur | Satir | Kritik Fonksiyon/Struct |
|---|-------|-----|-------|------------------------|
| 1 | SwapRouter.sol | Contract | 245 | exactInputSingle, exactInput, exactOutputSingle, exactOutput, uniswapV3SwapCallback |
| 2 | NonfungiblePositionManager.sol | Contract | 401 | Position struct, mint, increaseLiquidity, decreaseLiquidity, collect, burn |
| 3 | NonfungibleTokenPositionDescriptor.sol | Contract | 125 | tokenURI, flipRatio, tokenRatioPriority |
| 4 | V3Migrator.sol | Contract | 100 | migrate, MigrateParams |
| 5 | QuoterV2.sol | Contract | 274 | quoteExactInputSingle, quoteExactInput, quoteExactOutputSingle, parseRevertReason, handleRevert |
| 6 | Quoter.sol | Contract | 171 | quoteExactInputSingle, quoteExactInput, quoteExactOutputSingle |
| 7 | TickLens.sol | Contract | 43 | getPopulatedTicksInWord |
| 8 | UniswapInterfaceMulticall.sol | Contract | 40 | multicall (Call/Result struct) |
| 9 | LiquidityManagement.sol | Abstract | 91 | addLiquidity, uniswapV3MintCallback, MintCallbackData |
| 10 | PeripheryPayments.sol | Abstract | 71 | pay, unwrapWETH9, sweepToken, refundETH |
| 11 | PeripheryPaymentsWithFee.sol | Abstract | 56 | unwrapWETH9WithFee, sweepTokenWithFee |
| 12 | PeripheryImmutableState.sol | Abstract | 19 | factory, WETH9 |
| 13 | PeripheryValidation.sol | Abstract | 12 | checkDeadline modifier |
| 14 | BlockTimestamp.sol | Abstract | 13 | _blockTimestamp() virtual |
| 15 | ERC721Permit.sol | Abstract | 87 | permit, DOMAIN_SEPARATOR, PERMIT_TYPEHASH |
| 16 | Multicall.sol | Abstract | 29 | multicall (delegatecall) |
| 17 | PoolInitializer.sol | Abstract | 33 | createAndInitializePoolIfNecessary |
| 18 | SelfPermit.sol | Abstract | 64 | selfPermit, selfPermitIfNecessary, selfPermitAllowed, selfPermitAllowedIfNecessary |
| 19 | Path.sol | Library | 70 | hasMultiplePools, numPools, decodeFirstPool, getFirstPool, skipToken |
| 20 | PoolAddress.sol | Library | 49 | getPoolKey, computeAddress, POOL_INIT_CODE_HASH |
| 21 | CallbackValidation.sol | Library | 37 | verifyCallback (2 overload) |
| 22 | LiquidityAmounts.sol | Library | 138 | getLiquidityForAmount0, getLiquidityForAmount1, getLiquidityForAmounts, getAmount0ForLiquidity, getAmount1ForLiquidity, getAmountsForLiquidity |
| 23 | OracleLibrary.sol | Library | 181 | consult, getQuoteAtTick, getOldestObservationSecondsAgo, getBlockStartingTickAndLiquidity, getWeightedArithmeticMeanTick, getChainedPrice |
| 24 | BytesLib.sol | Library | 102 | slice, toAddress, toUint24 |
| 25 | NFTDescriptor.sol | Library | 478 | constructTokenURI, ConstructTokenURIParams, tickToDecimalString, feeToPercentString |
| 26 | NFTSVG.sol | Library | 407 | generateSVG, getCurve, isRare, SVGParams |
| 27 | HexStrings.sol | Library | 30 | toHexString, toHexStringNoPrefix |
| 28 | TransferHelper.sol | Library | 61 | safeTransferFrom, safeTransfer, safeApprove, safeTransferETH |
| 29 | PositionKey.sol | Library | 14 | compute |
| 30 | PositionValue.sol | Library | 168 | total, principal, fees, _getFeeGrowthInside, FeeParams |
| 31 | SqrtPriceMathPartial.sol | Library | 63 | getAmount0Delta, getAmount1Delta |
| 32 | TokenRatioSortOrder.sol | Library | 13 | NUMERATOR_MOST, NUMERATOR_MORE, NUMERATOR, DENOMINATOR_MOST, DENOMINATOR_MORE, DENOMINATOR |
| 33 | PoolTicksCounter.sol | Library | 97 | countInitializedTicksCrossed, countOneBits |
| 34 | ChainId.sol | Library | 14 | get() |
| 35 | ISwapRouter.sol | Interface | 68 | ExactInputSingleParams, ExactInputParams, ExactOutputSingleParams, ExactOutputParams |
| 36 | INonfungiblePositionManager.sol | Interface | 180 | MintParams, IncreaseLiquidityParams, DecreaseLiquidityParams, CollectParams |
| 37 | IQuoterV2.sol | Interface | 99 | QuoteExactInputSingleParams, QuoteExactOutputSingleParams |
| 38 | IQuoter.sol | Interface | 52 | quoteExactInput, quoteExactInputSingle, quoteExactOutput, quoteExactOutputSingle |
| 39 | ITickLens.sol | Interface | 26 | PopulatedTick, getPopulatedTicksInWord |
| 40 | IV3Migrator.sol | Interface | 35 | MigrateParams, migrate |
| 41 | IMulticall.sol | Interface | 14 | multicall |
| 42 | IPoolInitializer.sol | Interface | 23 | createAndInitializePoolIfNecessary |
| 43 | IPeripheryImmutableState.sol | Interface | 13 | factory, WETH9 |
| 44 | IPeripheryPayments.sol | Interface | 29 | unwrapWETH9, refundETH, sweepToken |
| 45 | IPeripheryPaymentsWithFee.sol | Interface | 30 | unwrapWETH9WithFee, sweepTokenWithFee |
| 46 | IERC721Permit.sol | Interface | 33 | PERMIT_TYPEHASH, DOMAIN_SEPARATOR, permit |
| 47 | ISelfPermit.sol | Interface | 77 | selfPermit, selfPermitIfNecessary, selfPermitAllowed, selfPermitAllowedIfNecessary |
| 48 | INonfungibleTokenPositionDescriptor.sol | Interface | 18 | tokenURI |
| 49 | IERC20Metadata.sol | Interface | 19 | name, symbol, decimals |
| 50 | IERC1271.sol | Interface | 17 | isValidSignature (magic: 0x1626ba7e) |
| 51 | IERC20PermitAllowed.sol | Interface | 28 | permit (DAI style) |
| 52 | IWETH9.sol | Interface | 14 | deposit, withdraw |
| 53 | LiquidityAmountsTest.sol | Test | 121 | gasCost fonksiyonlari ile tam kapsam |
| 54 | MockObservable.sol | Test | 49 | Observation struct, observe mock |
| 55 | MockObservations.sol | Test | 83 | Oracle.Observation[4], slot0, observations mock |
| 56 | MockTimeNonfungiblePositionManager.sol | Test | 24 | _blockTimestamp override, setTime |
| 57 | MockTimeSwapRouter.sol | Test | 20 | _blockTimestamp override, setTime |
| 58 | OracleTest.sol | Test | 68 | consult, getQuoteAtTick, getBlockStartingTickAndLiquidity gas olcumleri |
| 59 | PathTest.sol | Test | 38 | hasMultiplePools, decodeFirstPool, getFirstPool, skipToken gas olcumleri |
| 60 | PairFlash.sol | Example | 150 | FlashCallbackData, FlashParams, initFlash, uniswapV3FlashCallback |

---

# BOLUM 28: KRITIK GUVENLIK OZETI

## 28.1 Sahte Pool Saldirisi Onleme

Her callback (SwapCallback, MintCallback, FlashCallback) basinda `CallbackValidation.verifyCallback`
cagrilir. Bu cagri: `require(msg.sender == address(PoolAddress.computeAddress(factory, poolKey)))`
seklinde msg.sender'in CREATE2 ile hesaplanan gercek havuz adresi oldugunu dogrular.

## 28.2 Slippage Koruma

- Swap: `amountOutMinimum` (exactInput) ve `amountInMaximum` (exactOutput)
- Likidite: `amount0Min`, `amount1Min` (mint/increase/decrease)
- Hepsi require statement ile checked

## 28.3 Deadline Koruma

`checkDeadline` modifier: `require(_blockTimestamp() <= deadline, 'Transaction too old')`
Stale transaction ve sandwich attack onleme amacidir.

## 28.4 Permit Guvenligi (ERC721Permit)

- Nonce artirma: `_getAndIncrementNonce(tokenId)` — tekrar kullanim onler
- Smart wallet: IERC1271 destegi (isValidSignature magic: 0x1626ba7e)
- EIP-712 domain separator: chainId + verifyingContract (replay attack onleme)

## 28.5 collect — Zero Approving Pattern

decreaseLiquidity sonrasi approve sifirlanir:
```solidity
if (amount0V3 < amount0V2ToMigrate)
    TransferHelper.safeApprove(params.token0, nonfungiblePositionManager, 0);
```
Bu, kalan approve miktarinin potansiyel front-running exploit'ini onler.

---

# BOLUM 29: FORMUL OZETI TABLOSU

| Formul | Kaynak Kod | Aciklama |
|--------|-----------|----------|
| L = amount0 * sqrtPa * sqrtPb / Q96 / (sqrtPb - sqrtPa) | LiquidityAmounts.getLiquidityForAmount0 | Token0'dan likidite |
| L = amount1 * Q96 / (sqrtPb - sqrtPa) | LiquidityAmounts.getLiquidityForAmount1 | Token1'den likidite |
| amount0 = L*Q96*(sqrtPb-sqrtPa) / (sqrtPb*sqrtPa) | LiquidityAmounts.getAmount0ForLiquidity | Liknidite'den token0 |
| amount1 = L*(sqrtPb-sqrtPa) / Q96 | LiquidityAmounts.getAmount1ForLiquidity | Likidite'den token1 |
| pool = keccak256(0xff ++ factory ++ keccak256(token0,token1,fee) ++ init_code_hash)[12:] | PoolAddress.computeAddress | CREATE2 havuz adresi |
| fee_inside = global - lower.outside - upper.outside | PositionValue._getFeeGrowthInside | Aralik icindeki fee growth |
| tokensOwed += (feeGrowth_new - feeGrowth_old) * L / Q128 | NonfungiblePositionManager | Fee birikimi |
| mean_tick = (tickCum[now] - tickCum[t]) / secondsAgo | OracleLibrary.consult | TWAP tick |
| harmonic_liquidity = secondsAgo * 2^160 / (secPerLiq_delta * 2^32) | OracleLibrary.consult | TWAP likidite |
| feeAmount = balance * feeBips / 10000 | PeripheryPaymentsWithFee | Fee hesaplama |
| positionKey = keccak256(owner ++ tickLower ++ tickUpper) | PositionKey.compute | Core pozisyon anahtari |

---

# SONUC

Bu analiz, Uniswap V3 periphery katmaninin tam 60 dosyasini kapsamli olarak incelemis ve
tum struct, fonksiyon, guvenlik mekanizmalari ile matematik formullerini kaynak koddan cikararak
belgelemistir.

Nova Shield Chain L1 DEX'inin bu analizi kullanmasi icin en kritik 3 cikarim:

1. CALLBACK GUVENLIK: Her swap, mint ve flash callback'te `verifyCallback` pattern'i (CREATE2
   adresi hesaplama ile msg.sender dogrulama) uygulanmali — bu olmadan sahte pool saldirisi mumkundur.

2. MATEMATIK FORMULLER: LiquidityAmounts.getLiquidityForAmounts (3 durum: below/in-range/above)
   ve SqrtPriceMathPartial formulleri matematiksel olarak evrensel olup Q64.96 fixed-point
   aritmetigi ile L1 DEX'e dogrudan tasiniabilir.

3. MULTI-HOP ROUTING: Path kutuphanesinin byte-packed encoding formati (token:20 + fee:3) ve
   SwapRouter'in multi-hop dongu mantigi (payer degisimi, intermediate recipient=this) L1 DEX
   routing katmani icin dogrudan referans mimarisi olusturur.
