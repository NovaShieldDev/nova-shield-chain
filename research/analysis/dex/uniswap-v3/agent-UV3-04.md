# Uniswap V3 Analiz — Agent UV3-04
## Kapsam: Oracle + Fee Sistemi (3 dosya)
## DEX Özel Protokolü: 3x Hassasiyet

---

## Analiz Edilen Dosyalar
- `Oracle.sol` (325 satır) — Core oracle kütüphanesi
- `OracleTest.sol` (100 satır) — Test wrapper kontratı
- `OracleEchidnaTest.sol` (137 satır) — Fuzzing property testleri

---

## Oracle.sol (325 satır)

**Amaç:** Her havuzun fiyat ve likidite tarihini circular buffer'da saklayan, TWAP hesaplaması sağlayan on-chain oracle sistemi.

---

### TWAP Konsepti — Temel Anlayış

**TWAP (Time-Weighted Average Price):**
```
TWAP = (tickCumulative_t2 - tickCumulative_t1) / (t2 - t1)
```

**Sayısal Örnek:**
- t=0: tick=100, tickCumulative=0
- t=60: tick=200, tickCumulative=6000 (100 × 60)
- t=120: tick=200, tickCumulative=18000 (6000 + 200×60)
- 60 saniyelik TWAP (t60→t120) = (18000-6000)/60 = 200 ✓
- 120 saniyelik TWAP (t0→t120) = (18000-0)/120 = 150 (ortalaması)

**Neden TWAP, anlık fiyat değil?**
1. Anlık fiyat = tek blokta manipüle edilebilir (flash loan)
2. TWAP = N blokun ortalaması → manipülasyon çok pahalı
3. 1 saatlik TWAP'ı eğmek için 1 saat boyunca blok kontrolü gerekir
4. Ekonomik maliyet, potansiyel kârı aşar → güvenli oracle

**Cardinality Trade-off:**
- Küçük cardinality: daha az geçmiş, daha az gas (grow() maliyeti)
- Büyük cardinality: daha uzun TWAP penceresi, daha pahalı slot initialization
- Varsayılan: 1 (sadece anlık değer)
- grow() ile isteğe bağlı artırılır

---

### Struct Analizi

#### `Observation` struct (satır 12-21)

```solidity
struct Observation {
    uint32 blockTimestamp;           // satır 14
    int56 tickCumulative;            // satır 16
    uint160 secondsPerLiquidityCumulativeX128; // satır 18
    bool initialized;                // satır 20
}
```

**Her field'ın bit genişliği gerekçesi:**

`blockTimestamp: uint32`
- 2^32 saniye = ~136 yıl
- 2106'da overflow → uint32 sıfırlanır
- Sorun DEĞİL: `lte()` fonksiyonu overflow-aware karşılaştırma yapar
- uint256 kullansaydı: her observation 4x daha pahalı SSTORE
- Gas optimizasyonu: tüm Observation tek bir EVM slot'una sığar (32+56+160+8 = 256 bit!)

`tickCumulative: int56`
- tick aralığı: [-887272, 887272] → int24
- Maksimum birikim: 887272 × 2^32 saniye = ~3.8 × 10^15
- int56 = [-36 × 10^15, +36 × 10^15] → yeterli
- int64 kullansaydı struct 256 bit'i aşardı → 2 slot

`secondsPerLiquidityCumulativeX128: uint160`
- Birim: saniye / likidite, Q128.128 fixed-point
- Harmonic mean liquidity hesabı için: `timeWeightedHarmonicMeanLiquidity = timeElapsed × 2^160 / cumulativeDiff >> 32`
- uint160 seçimi: struct toplamı tam 256 bit olsun diye (32+56+160+8=256)

`initialized: bool`
- Circular buffer'da yazılmış slot ile boş slot ayrımı
- False → binarySearch bu slot'u atlar (l = i + 1)

---

### Fonksiyon Analizleri

#### `transform()` (satır 30-45) — private pure

```solidity
function transform(
    Observation memory last,
    uint32 blockTimestamp,
    int24 tick,
    uint128 liquidity
) private pure returns (Observation memory)
```

**Formüller:**

1. **tickCumulative birikimi:**
```
tickCumulative_new = tickCumulative_last + int56(tick) * delta
delta = blockTimestamp - last.blockTimestamp  (uint32 overflow-safe)
```

2. **secondsPerLiquidityCumulative birikimi:**
```
secondsPerLiquidityCumulativeX128_new =
    secondsPerLiquidityCumulativeX128_last +
    (uint160(delta) << 128) / max(liquidity, 1)
```

**Açıklama:**
- `delta << 128` = delta × 2^128 (Q128.128 fixed-point'e çevirme)
- `max(liquidity, 1)` = sıfır bölme koruması (pool boşken 1 kullan)
- Bu birikim → harmonic mean liquidity hesabına olanak tanır

**Neden harmonic mean?**
- Arithmetic mean: büyük likidite dönemlerine aşırı ağırlık verir
- Harmonic mean: küçük likidite dönemlerini daha çok etkiler
- Özellikle concentrated liquidity'de daha anlamlı

---

#### `initialize()` (satır 52-63) — internal

```solidity
function initialize(Observation[65535] storage self, uint32 time)
    internal returns (uint16 cardinality, uint16 cardinalityNext)
```

**Algoritma:**
1. self[0] = Observation(time, 0, 0, true)
2. return (1, 1) → cardinality=1, cardinalityNext=1

**Önemli:** tickCumulative=0 başlangıçta sıfır — bu referans noktası. Sonraki tüm birikimler bu noktaya göre.

---

#### `write()` (satır 78-101) — internal

```solidity
function write(
    Observation[65535] storage self,
    uint16 index,
    uint32 blockTimestamp,
    int24 tick,
    uint128 liquidity,
    uint16 cardinality,
    uint16 cardinalityNext
) internal returns (uint16 indexUpdated, uint16 cardinalityUpdated)
```

**Algoritma adım adım:**

1. `last = self[index]` — en son observation'ı oku

2. **Erken dönüş (kritik):**
   ```solidity
   if (last.blockTimestamp == blockTimestamp) return (index, cardinality);
   ```
   - Aynı blokta iki kez yazma yasak!
   - Neden? Oracle manipülasyon koruması: bir blok içinde fiyat değişikliği oracle'a yansımaz
   - Her blok sadece bir kez yazılabilir → saldırgan birden fazla observation ekleyemez

3. **Cardinality büyütme koşulu:**
   ```solidity
   if (cardinalityNext > cardinality && index == (cardinality - 1)) {
       cardinalityUpdated = cardinalityNext;
   }
   ```
   - Sadece en son index dolunca kapasiteyi büyüt
   - Neden bekle? Ordering'i korumak için — ortasına yeni slot açarsan binary search bozulur

4. **Circular buffer'a yaz:**
   ```solidity
   indexUpdated = (index + 1) % cardinalityUpdated;
   self[indexUpdated] = transform(last, blockTimestamp, tick, liquidity);
   ```

---

#### `grow()` (satır 108-120) — internal

```solidity
function grow(
    Observation[65535] storage self,
    uint16 current,
    uint16 next
) internal returns (uint16)
```

**Kritik Optimizasyon:**
```solidity
for (uint16 i = current; i < next; i++) self[i].blockTimestamp = 1;
```

**Neden blockTimestamp = 1?**
- EVM'de SSTORE: sıfır'dan sıfır-olmayan'a = 20,000 gas
- Sıfır-olmayan'dan sıfır-olmayan'a = 2,900 gas
- Yeni slot'ları şimdi 20k gas ile doldur → swap sırasında 2.9k gas öde
- Sonuç: Swap maliyeti azalır, likidite sağlayıcı grow() maliyetini öder

**initialized=false kalır** → binarySearch bu slot'ları boş sayar, yazılmış gibi davranmaz.

---

#### `lte()` (satır 128-140) — private pure

```solidity
function lte(uint32 time, uint32 a, uint32 b) private pure returns (bool)
```

**uint32 Overflow-Safe Karşılaştırma:**

Normal durum (overflow yok):
```
if (a <= time && b <= time) return a <= b;
```

Overflow durumu:
```
aAdjusted = a > time ? a : a + 2^32
bAdjusted = b > time ? b : b + 2^32
return aAdjusted <= bAdjusted
```

**Örnek — Overflow Senaryosu:**
- Mevcut time = 10 (yeni overflow sonrası)
- a = 2^32 - 5 (eski, henüz overflow olmadan önce)
- b = 8 (yeni)
- Normal karşılaştırma: a > b? Evet, yanlış!
- Düzeltilmiş: aAdjusted = 2^32-5 (>time), bAdjusted = 8 + 2^32
- bAdjusted > aAdjusted → a < b ✓ doğru

**Kural:** `a` ve `b` parametreleri `time`'dan kronolojik olarak önce veya eşit olmalı.

---

#### `binarySearch()` (satır 153-184) — private view

```solidity
function binarySearch(
    Observation[65535] storage self,
    uint32 time,
    uint32 target,
    uint16 index,
    uint16 cardinality
) private view returns (Observation memory beforeOrAt, Observation memory atOrAfter)
```

**Circular Buffer Binary Search:**

Başlangıç:
```
l = (index + 1) % cardinality   // en eski observation
r = l + cardinality - 1         // en yeni observation (sanal index)
```

**Neden sanal index?** Circular buffer'ı düz dizi gibi görmek için. `i % cardinality` ile gerçek index'e çeviriyoruz.

**Adım adım:**
```
while (true):
    i = (l + r) / 2
    beforeOrAt = self[i % cardinality]

    // Uninitialized slot → daha yeniye git
    if (!beforeOrAt.initialized):
        l = i + 1
        continue

    atOrAfter = self[(i + 1) % cardinality]

    targetAtOrAfter = lte(time, beforeOrAt.blockTimestamp, target)

    // Bulundu: beforeOrAt ≤ target ≤ atOrAfter
    if (targetAtOrAfter && lte(time, target, atOrAfter.blockTimestamp)):
        break

    if (!targetAtOrAfter):
        r = i - 1    // target daha eski → sola git
    else:
        l = i + 1    // target daha yeni → sağa git
```

**Sayısal Örnek:**
- cardinality=8, index=5
- l = (5+1)%8 = 6, r = 6+7 = 13
- Hedef: t=50
- i=9 → 9%8=1, beforeOrAt=obs[1] (t=30)
  - 30 ≤ 50? Evet. atOrAfter=obs[2] (t=60). 50 ≤ 60? Evet → BULUNDU!
- beforeOrAt=obs[1](t=30), atOrAfter=obs[2](t=60)

**Karmaşıklık:** O(log N) N = cardinality

---

#### `getSurroundingObservations()` (satır 198-230) — private view

**3 aşamalı yaklaşım:**

**Aşama 1: Target, en yeni'den sonraysa**
```solidity
beforeOrAt = self[index];
if (lte(time, beforeOrAt.blockTimestamp, target)):
    if (beforeOrAt.blockTimestamp == target):
        return (beforeOrAt, atOrAfter)  // atOrAfter kullanılmaz
    else:
        return (beforeOrAt, transform(beforeOrAt, target, tick, liquidity))
```
Yani şu anki blok dahil gelecek için `transform()` ile ekstrapolasyon.

**Aşama 2: Target, en eski'den önce ise**
```solidity
beforeOrAt = self[(index + 1) % cardinality]
if (!beforeOrAt.initialized): beforeOrAt = self[0]
require(lte(time, beforeOrAt.blockTimestamp, target), 'OLD')
```
'OLD' revert → oracle bu kadar geçmişe gidemez.

**Aşama 3: Arada ise → binarySearch**

---

#### `observeSingle()` (satır 245-287) — internal view

```solidity
function observeSingle(
    Observation[65535] storage self,
    uint32 time,
    uint32 secondsAgo,
    int24 tick,
    uint16 index,
    uint128 liquidity,
    uint16 cardinality
) internal view returns (int56 tickCumulative, uint160 secondsPerLiquidityCumulativeX128)
```

**Akış:**

**secondsAgo=0 (anlık):**
```
last = self[index]
if (last.blockTimestamp != time):
    last = transform(last, time, tick, liquidity)
return (last.tickCumulative, last.secondsPerLiquidityCumulativeX128)
```

**secondsAgo>0:**
```
target = time - secondsAgo
(beforeOrAt, atOrAfter) = getSurroundingObservations(...)

if (target == beforeOrAt.blockTimestamp):
    return beforeOrAt değerleri

if (target == atOrAfter.blockTimestamp):
    return atOrAfter değerleri

// Arada: doğrusal interpolasyon
observationTimeDelta = atOrAfter.blockTimestamp - beforeOrAt.blockTimestamp
targetDelta = target - beforeOrAt.blockTimestamp

tickCumulative = beforeOrAt.tickCumulative +
    ((atOrAfter.tickCumulative - beforeOrAt.tickCumulative) / observationTimeDelta) * targetDelta

secondsPerLiquidityCumulativeX128 = beforeOrAt.secondsPerLiquidityCumulativeX128 +
    uint160((uint256(atOrAfter.secondsPerLiquidityCumulativeX128 - beforeOrAt.secondsPerLiquidityCumulativeX128)
    * targetDelta) / observationTimeDelta)
```

**İnterpolasyon Formülü:**
```
value(target) = value(before) + (value(after) - value(before)) × (target - before) / (after - before)
```
Doğrusal interpolasyon — tick sabit hız değişiyor varsayımı.

**Sayısal örnek:**
- before: t=100, tickCumulative=10000
- after: t=200, tickCumulative=30000
- target: t=150
- timeDelta = 200-100 = 100
- targetDelta = 150-100 = 50
- interpolated = 10000 + ((30000-10000)/100) × 50 = 10000 + 10000 = 20000

---

#### `observe()` (satır 300-324) — internal view

```solidity
function observe(
    Observation[65535] storage self,
    uint32 time,
    uint32[] memory secondsAgos,
    int24 tick,
    uint16 index,
    uint128 liquidity,
    uint16 cardinality
) internal view returns (int56[] memory tickCumulatives, uint160[] memory secondsPerLiquidityCumulativeX128s)
```

**Algoritma:**
```
require(cardinality > 0, 'I')
for i in 0..secondsAgos.length:
    (tickCumulatives[i], secondsPerLiquidityCumulativeX128s[i]) =
        observeSingle(self, time, secondsAgos[i], tick, index, liquidity, cardinality)
```

Basit: her secondsAgo için observeSingle çağırır.

**Kullanım örneği:**
```solidity
uint32[] memory secondsAgos = new uint32[](2);
secondsAgos[0] = 3600;  // 1 saat önce
secondsAgos[1] = 0;     // şimdi

(int56[] memory ticks, ) = oracle.observe(secondsAgos);
int56 TWAP = (ticks[1] - ticks[0]) / 3600;
```

---

## Fee Sistemi Analizi

### Fee Tier Tablosu

| Fee (bps) | Fee (%) | tickSpacing | Hedef Kullanım |
|-----------|---------|-------------|----------------|
| 100 | 0.01% | 1 | Stablecoin çiftleri (USDC/USDT) |
| 500 | 0.05% | 10 | Yüksek korelasyonlu (ETH/stETH) |
| 3000 | 0.30% | 60 | Standart (ETH/USDC) |
| 10000 | 1.00% | 200 | Egzotik/volatil çiftler |

**tickSpacing neden fee'ye göre değişiyor?**
- Düşük fee → sıkı aralık → LP'ler daha hassas fiyat noktası seçer
- Yüksek fee → geniş aralık → LP'ler geniş bandda likidite sağlar
- Stablecoin: 0.01% fee, tickSpacing=1 → fiyat 1:1'e çok yakın kalır

### feeGrowthGlobal Hesabı

Her swap'ta:
```
feeGrowthGlobalX128 += FullMath.mulDiv(feeAmount, FixedPoint128.Q128, liquidity)
```

Açılımı:
```
feeGrowthGlobalX128 += feeAmount × 2^128 / liquidity
```

**Neden 2^128?**
- Q128.128 fixed-point format
- Fee büyümesi per-unit-liquidity ölçümü
- Küçük fee miktarları kaybolmasın diye büyük sabit çarpanı

**Birikimli hesap neden?**
- Her position için ayrı tracker tutmak O(N) maliyet
- Global birikim + "last seen" per-position = O(1) hesap
- Kazanılan fee = liquidity × (feeGrowthGlobal - feeGrowthInsideLast) / 2^128

### Protocol Fee Mekanizması

`feeProtocol: uint8` — Slot0'da saklı
- Alt 4 bit: token0 protocol fee oranı
- Üst 4 bit: token1 protocol fee oranı
- Değer = N → protocol fee = swap fee / N
- Örnek: N=6 → protocol fee = 1/6 = ~16.67% swap fee'nin

**Protocol fee toplama:**
```solidity
function collectProtocol(address recipient, uint128 amount0Requested, uint128 amount1Requested)
    external lock onlyFactoryOwner
    returns (uint128 amount0, uint128 amount1)
```
Sadece factory owner toplayabilir.

**feeGrowthInside hesaplama (Position.update'te kullanılır):**
```
fee0 = position.liquidity × (feeGrowthInside0 - position.feeGrowthInside0LastX128) / 2^128
fee1 = position.liquidity × (feeGrowthInside1 - position.feeGrowthInside1LastX128) / 2^128
```

**feeGrowthInside:**
```
feeGrowthInside0 = feeGrowthGlobal0 - feeGrowthBelow0 - feeGrowthAbove0
```
(Tick.getFeeGrowthInside() ile hesaplanır — Tick analizi için UV3-03'e bakınız)

---

## OracleTest.sol (100 satır)

**Amaç:** Oracle kütüphanesini test etmek için izole wrapper kontratı.

**State Variables:**
```solidity
Oracle.Observation[65535] public observations;
uint32 public time;
int24 public tick;
uint128 public liquidity;
uint16 public index;
uint16 public cardinality;
uint16 public cardinalityNext;
```

**Fonksiyonlar:**

`initialize(InitializeParams calldata params)`:
- Sadece bir kez çağrılabilir (cardinality==0 kontrolü)
- State set et + Oracle.initialize() çağır

`advanceTime(uint32 by)`: `time += by` — mock block.timestamp

`update(UpdateParams calldata params)`:
```
advanceTime(params.advanceTimeBy)
(index, cardinality) = observations.write(index, time, tick, liquidity, cardinality, cardinalityNext)
tick = params.tick
liquidity = params.liquidity
```
Önce observation yaz, sonra tick/liquidity değiştir. Gerçek pool davranışını taklit eder.

`batchUpdate(UpdateParams[] calldata params)`:
- Gas optimizasyonu: tüm state'i memory'ye çek, döngüde güncelle, sonunda storage'a yaz
- SLOAD/SSTORE maliyetini minimize eder

`grow(uint16 _cardinalityNext)`: `cardinalityNext = observations.grow(cardinalityNext, _cardinalityNext)`

`observe(uint32[] calldata secondsAgos)`: Oracle.observe() wrapper

`getGasCostOfObserve()`: observe() gas maliyetini ölçer

---

## OracleEchidnaTest.sol (137 satır)

**Amaç:** Fuzzing ile oracle invariant'larını test etmek.

**Mimari:** Echidna → OracleEchidnaTest → OracleTest → Oracle library

**State:**
```solidity
OracleTest private oracle;
bool private initialized;
uint32 private timePassed;
```

**`limitTimePassed(uint32 by)`:**
```solidity
require(timePassed + by >= timePassed);  // overflow koruması
timePassed += by;
```
Birikimli zaman overflow olmasın diye.

### Echidna Property'leri (Invariant'lar)

**1. `echidna_indexAlwaysLtCardinality()` (satır 73-75):**
```solidity
return oracle.index() < oracle.cardinality() || !initialized;
```
**Garantisi:** index her zaman cardinality'den küçük. Circular buffer sınırı aşılamaz.

**2. `echidna_AlwaysInitialized()` (satır 77-80):**
```solidity
(, , , bool isInitialized) = oracle.observations(0);
return oracle.cardinality() == 0 || isInitialized;
```
**Garantisi:** Eğer oracle aktifse, index 0 her zaman initialized. İlk slot asla boş bırakılmaz.

**3. `echidna_cardinalityAlwaysLteNext()` (satır 82-84):**
```solidity
return oracle.cardinality() <= oracle.cardinalityNext();
```
**Garantisi:** Aktif kapasit, planlanan kapasitenin üstüne çıkamaz. grow() hiçbir zaman cardinality'yi cardinalityNext'ten büyük yapmaz.

**4. `echidna_canAlwaysObserve0IfInitialized()` (satır 86-94):**
```solidity
uint32[] memory arr = new uint32[](1);
arr[0] = 0;
(bool success, ) = address(oracle).staticcall(...)
return success;
```
**Garantisi:** Initialized oracle'da secondsAgo=0 her zaman başarılı (revert etmez). Anlık gözlem her zaman mümkün.

### Formal Property Testleri

**`checkTwoAdjacentObservationsTickCumulativeModTimeElapsedAlways0()`:**
```solidity
assert((tickCumulative1 - tickCumulative0) % timeElapsed == 0);
```
**Garantisi:** İki bitişik observation arasındaki tickCumulative farkı, geçen süreye tam bölünür. Bu, integer tick değerlerini kullandığımızı doğrular (kesirli tick yok).

**`checkTimeWeightedResultAssertions()`:**
```solidity
int56 timeWeightedTick = (tickCumulatives[1] - tickCumulatives[0]) / timeElapsed;
uint256 timeWeightedHarmonicMeanLiquidity = ...
assert(timeWeightedHarmonicMeanLiquidity <= type(uint128).max);
assert(timeWeightedTick <= type(int24).max);
assert(timeWeightedTick >= type(int24).min);
```
**Garantisi:** TWAP sonuçları her zaman int24 aralığına sığar. Harmonic mean liquidity uint128'i aşmaz.

**`checkTimeWeightedAveragesAlwaysFitsType()`:**
- Negatif numerator için rounding (floor division):
  ```solidity
  if (numerator < 0 && numerator % int56(secondsAgo) != 0) timeWeightedTick--;
  ```
  TWAP her zaman aşağı yuvarlanır — fiyat manipülasyonuna karşı koruyucu.

---

## Güvenlik Analizi

### Oracle Manipulation Vektörleri

**Vektör 1: Single-block Manipulation**
- Saldırgan tek blokta fiyatı büyük miktarda değiştirir
- Savunma: Her blokta sadece bir observation yazılır. Blok sonu fiyatı → bir sonraki blokta kayıt
- Yani blok başında fiyatı değiştirip geri al → oracle etkilenmez

**Vektör 2: Multi-block Manipulation (Gerçek Risk)**
- Saldırgan N blok boyunca fiyatı yüksek/düşük tutar
- TWAP = N blokluk ortalama
- Savunma: Maliyeti yüksek. Blok başına ödenen net kayıp × N blok
- Daha uzun TWAP penceresi → daha pahalı saldırı

**Vektör 3: Low Cardinality Attack**
- cardinality=1 ise sadece anlık değer → TWAP imkânsız
- Protokol TWAP kullanacaksa grow() çağırmalı ve kafi cardinality sağlamalı

**Vektör 4: Flash Loan + Oracle**
- Flash loan ile anlık fiyatı değiştir → oracle okuma → geri öde
- Savunma: Bir blok içinde oracle değişmez (write() early return). Flash loan aynı blok içinde tamamlanır → oracle etkilenmez ✓

### Güvenlik Garantileri

1. **Blok başına en fazla bir observation** → saldırgan frekansını sınırlandırır
2. **Circular buffer overflow koruması** → index < cardinality invariant
3. **Timestamp overflow-safe karşılaştırma** → 2106'ya kadar güvenli
4. **Monoton artan timestamps** → binarySearch invariant korunur
5. **'OLD' revert** → cardinality'yi aşan geçmiş sorgusu reddedilir

---

## Nova Shield için Oracle Tasarımı

### L1 Gömülü Oracle Avantajları

**Avantaj 1: Otomatik Her Blok**
- Nova Shield L1'de: her blok finaliz oluşunda oracle güncellenir
- Harici çağrı gerekmez, miner/validator sorgulama gerekmez
- EVM'de: swap sırasında çağrı → Nova Shield'de: blok işleme pipeline'ında otomatik

**Avantaj 2: Manipülasyon Daha Zor**
- Nova Shield: PoS/BFT consensus → blok proposer tahmin edilebilir değil
- Ethereum: MEV bot'lar blok başına müdahale edebilir
- Nova Shield: MPC threshold imza → tek bir validatör Oracle'ı manipüle edemez

**Avantaj 3: ZK Doğrulanabilir Oracle**
- Arkworks (Groth16) ile oracle değerleri ZK proof'a dahil edilebilir
- "Bu TWAP değeri X idi" → ZK proof → gizlilik korunarak oracle kullanımı

### Rust Implementasyon Planı

**Observation struct:**
```rust
#[derive(Clone, Copy, Default)]
pub struct Observation {
    pub block_timestamp: u32,
    pub tick_cumulative: i64,  // i56 Rust'ta yok, i64 kullan
    pub seconds_per_liquidity_cumulative_x128: u128,  // u160 → u128 yeterli (Q128)
    pub initialized: bool,
}
```

**Circular Buffer:**
```rust
pub struct OracleState {
    pub observations: Vec<Observation>,  // veya [Observation; 65535] fixed array
    pub index: u16,
    pub cardinality: u16,
    pub cardinality_next: u16,
}
```

**Transform fonksiyonu:**
```rust
fn transform(last: &Observation, block_timestamp: u32, tick: i32, liquidity: u128) -> Observation {
    let delta = block_timestamp.wrapping_sub(last.block_timestamp) as u64;
    Observation {
        block_timestamp,
        tick_cumulative: last.tick_cumulative + (tick as i64) * delta as i64,
        seconds_per_liquidity_cumulative_x128: last.seconds_per_liquidity_cumulative_x128
            .wrapping_add((delta as u128) << 128 / liquidity.max(1)),
        initialized: true,
    }
}
```

**Binary Search:**
```rust
fn binary_search(
    observations: &[Observation],
    time: u32,
    target: u32,
    index: u16,
    cardinality: u16,
) -> (Observation, Observation) {
    let cardinality = cardinality as usize;
    let mut l = (index as usize + 1) % cardinality;
    let mut r = l + cardinality - 1;

    loop {
        let i = (l + r) / 2;
        let before = observations[i % cardinality];
        if !before.initialized { l = i + 1; continue; }
        let after = observations[(i + 1) % cardinality];
        let target_at_or_after = lte(time, before.block_timestamp, target);
        if target_at_or_after && lte(time, target, after.block_timestamp) {
            return (before, after);
        }
        if !target_at_or_after { r = i - 1; } else { l = i + 1; }
    }
}
```

### Funding Rate Oracle (dYdX Yaklaşımı)

Nova Shield perpetual futures için:
```
funding_rate = (mark_price - index_price) / index_price × daily_rate
```

**8 saatlik TWAP:**
```rust
fn get_8h_twap(oracle: &OracleState, current_time: u32) -> i64 {
    let seconds_ago = 8 * 3600u32;
    let (ticks_now, _) = observe_single(oracle, current_time, 0, ...);
    let (ticks_8h, _) = observe_single(oracle, current_time, seconds_ago, ...);
    (ticks_now - ticks_8h) / seconds_ago as i64
}
```

**MEV-Resistant Oracle:**
- Nova Shield: blok başında oracle snapshot al
- Swap'tan ÖNCE oracle değeri snapshot → oracle değişmez
- Funding rate hesabı blok sonunda → swap'ların etkisi yok

### Nova Shield'de Daha İyi Yapabileceğimiz

1. **Cardinality Otomatik Büyüme:** Protokol kendi büyütür, kullanıcı grow() çağırmaz
2. **ZK TWAP Proof:** Gizli fiyat aralığı kanıtı (Penumbra yaklaşımı)
3. **Cross-Asset TWAP:** Birden fazla havuzun TWAP'ını birleştir
4. **Adaptive Cardinality:** Volatiliteye göre dinamik buffer boyutu
5. **L1 Oracle Export:** Diğer zincirler Nova Shield oracle'ını köprü üzerinden kullanabilir

---

## Özet

Oracle.sol, Uniswap V3'ün en zarif tasarım kararlarından birini içerir: 256 bit'e sıkıştırılmış observation struct, her blokta en fazla bir yazma, timestamp overflow-safe binary search. Fee sistemi ise global birikim + per-position "last seen" yaklaşımıyla O(1) fee hesabı sağlar.

Nova Shield için:
- Oracle her blokta otomatik güncellenmeli (L1 avantajı)
- ZK ile oracle gizliliği eklenebilir
- Funding rate için 8 saatlik TWAP kullanılmalı
- Cardinality protokol tarafından yönetilmeli
