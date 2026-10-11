# Penumbra DEX Modülü — Agent-04 (Çift Agent: Birinci Perspektif)
## LP Çekirdek Veri Modeli, AMM Formülleri ve Pozisyon Yaşam Döngüsü

**Kapsanan Dosyalar:** 35 dosya, DEX çekirdeği
**Odak:** Concentrated Liquidity AMM Matematiği, Position State Machine, Batch Swap ZK
**Hassasiyet:** 3x (DEX modülü — çift agent protokolü)

---

## 1. `src/lp/trading_function.rs` — AMM Çekirdeği (716 satır)

### Struct: `TradingFunction` (satır 14-17)
```rust
pub struct TradingFunction {
    pub component: BareTradingFunction,
    pub pair: TradingPair,
}
```
`TradingPair` ile birleştirilmiş tam tip bilgisi. `BareTradingFunction` asset ID'siz ham matematik.

### Struct: `BareTradingFunction` (satır 203-214)
```rust
pub struct BareTradingFunction {
    pub fee: u32,   // basis points, 0 <= fee < 5000
    pub p: Amount,  // asset_1 valuasyonu (<=2^80)
    pub q: Amount,  // asset_2 valuasyonu (<=2^80)
}
```

**AMM Formülü (Tasarım Açıklaması, satır 196-198):**
```
phi(R) = p*R_1 + q*R_2
```
Bu **sabit-toplam CFMM** (constant-sum market maker). Uniswap v3 gibi concentrated liquidity değil, farklı yaklaşım: her pozisyon birbirinden bağımsız sabit-fiyat emir gibi çalışır. Etkin fiyat parametreler `p/q` oranından belirlenir.

### Kritik Fonksiyon: `gamma()` (satır 444-446)
```rust
pub fn gamma(&self) -> U128x128 {
    (U128x128::from(10_000 - self.fee) / U128x128::from(10_000u64)).expect("10_000 != 0")
}
```
**gamma = (10_000 - fee) / 10_000**
0 bps fee → gamma=1.0; 30 bps → gamma=0.997; 5000 bps (max) → gamma=0.5

### Kritik Fonksiyon: `effective_price()` (satır 413-419)
```rust
pub fn effective_price(&self) -> U128x128 {
    let price_ratio = (q / p).expect("p != 0 and p,q <= 2^60");
    price_ratio.checked_div(&self.gamma()).expect("gamma != 0")
}
```
**effective_price = (q/p) / gamma = q / (p * gamma)**

Yorumu: lambda_2 birim asset_2 almak için kaç delta_1 (asset_1) gerekeceği.

### Kritik Fonksiyon: `effective_price_inv()` (satır 402-408)
```rust
pub fn effective_price_inv(&self) -> U128x128 {
    let price_ratio = (p / q).expect("q != 0 and p,q <= 2^60");
    (price_ratio * self.gamma()).expect("2^-1 <= gamma <= 1")
}
```
**effective_price_inv = (p/q) * gamma**

delta_1 miktarını lambda_2'ye dönüştürür: `lambda_2 = delta_1 * effective_price_inv`

### Kritik Fonksiyon: `fill()` (satır 290-388) — EN ÖNEMLİ FONKSIYON

**Parametre:** `delta_1: Amount, reserves: &Reserves`
**Dönüş:** `(unfilled: Amount, new_reserves: Reserves, output: Amount)`

İki dal vardır:

**Dal 1 — Partial fill (tentative_lambda_2 <= reserves.r2):**
```
lambda_2 = floor(delta_1 * effective_price_inv)
new_r1 = r1 + delta_1
new_r2 = r2 - lambda_2
unfilled = 0
```
Yuvarlama aşağı yapılır → rounding error yakılır (outputs değil inputs kesin)

**Dal 2 — Full reserve drain (tentative_lambda_2 > reserves.r2):**
```
lambda_2 = r2  (tüm rezerv çekilir)
fillable_delta_1 = ceil(r2 * effective_price)  // yukarı yuvarlama: input'u burn et
unfilled = delta_1 - fillable_delta_1
new_r1 = r1 + fillable_delta_1
new_r2 = 0
```

**Matematiksel İspat (satır 365-379):**
Kod içi yorum şunu kanıtlar: `delta_1 >= fillable_delta_1_exact` her zaman → `unfilled >= 0`

**Değer Korunumu (test satır 578-579):**
```
old_r1 + delta_1 == new_r1 + lambda_1  (asset 1 korunumu)
old_r2 + delta_2 == new_r2 + lambda_2  (asset 2 korunumu)
```

### Kritik Fonksiyon: `fill_output()` (satır 241-281) — Ters yön fill

Verilen çıktı miktarı `lambda_2` için gereken girdi `delta_1` hesaplar (backward fill).

```
delta_1 = ceil(lambda_2 * effective_price)
new_r1 = r1 + delta_1
new_r2 = r2 - lambda_2
```

Eğer `lambda_2 > reserves.r2` → `None` döner (yetersiz likidite).

### Fonksiyon: `flip()` (satır 221-227)
```rust
pub fn flip(&self) -> Self {
    Self { fee: self.fee, p: self.q, q: self.p }
}
```
Asset 2'den asset 1'e takas için kullanılır. `TradingFunction::fill()` bu flip mekanizmasını kullanarak her iki yönde de çalışır (satır 62-77).

### Fonksiyon: `effective_price_key_bytes()` (satır 395-397)
`[u8; 32]` döner. Özellik: **lexicographic sıralama == numerik sıralama** (U128x128'in `to_bytes()` özelliği). Bu sayede fiyat endeksi RocksDB prefix taramasıyla çalışır.

### Testler (satır 487-714)

**`test_trading_function_to_bytes`:** Ücretli vs ücretsiz işlemin fiyat sıralamasını doğrular.
- Sıfır ücret → daha iyi fiyat (bytes1 < bytes2)

**`fill_conserves_value`:** İki durumu test eder:
1. Kısmi fill: delta_1=10M, r2=100M, p=1, q=3 → lambda_2=3_333_333
2. Tam fill: delta_1=600M, r2=100M → lambda_2=r2=100M, unfilled=300M

**`fill_bad_rounding`:** p=12, q=10, delta_1=100, r2=120 → lambda_2=119 (rounding loss=1)

**Nova Shield Dersi:** `lambda_2 = floor(delta_1 * p * gamma / q)` formülünü aynen kullanabiliriz. Rounding her zaman trader aleyhine, LP lehine. Bu protokolün değer sızıntısını önler.

---

## 2. `src/lp/position.rs` — Pozisyon Data Modeli (704 satır)

### Sabitler (satır 15-18)
```rust
pub const MAX_RESERVE_AMOUNT: u128 = (1 << 80) - 1;  // 80 bit max rezerv
pub const MAX_FEE_BPS: u32 = 5000;                     // max %50 ücret
```

### Struct: `Position` (satır 24-38)
```rust
pub struct Position {
    pub state: State,
    pub reserves: Reserves,
    pub phi: TradingFunction,
    pub nonce: [u8; 32],       // benzersizlik için rastgele değer
    pub close_on_fill: bool,   // limit order modu
}
```

**`close_on_fill`:** `true` olduğunda, rezervlerden biri sıfırlandığında pozisyon otomatik kapanır. Limit order davranışı.

### Enum: `State` (satır 366-381)
```rust
pub enum State {
    Opened,
    Closed,
    Withdrawn { sequence: u64 },
}
```

**Sequence numarası:** Her çekilmede artar. Retroaktif ödüller için çekilmiş pozisyona tekrar para yatırılabilir. Sequence numarası bu durumda sabit kalır.

### Fonksiyon: `Position::new()` (satır 58-100)

**Kanoniku çiftleştirme mantığı (satır 77-90):**
```rust
let canonical_tp: TradingPair = pair.into();
if pair.start != canonical_tp.asset_1() {
    std::mem::swap(&mut p, &mut q);
    reserves = Reserves { r1: reserves.r2, r2: reserves.r1 };
}
```
`DirectedTradingPair` (yönlü) → `TradingPair` (kanonik) dönüşümü. Asset ID küçük olan her zaman asset_1. Kullanıcı yönlü girmişse p/q ve rezervler swap edilir.

### Fonksiyon: `Position::id()` (satır 145-161)

**Blake2b ile pozisyon ID'si:**
```rust
let mut state = blake2b_simd::Params::default()
    .personal(b"penumbra_lp_id")
    .to_state();
state.update(&self.nonce);
state.update(&pair.asset_1().to_bytes());
state.update(&pair.asset_2().to_bytes());
state.update(&fee.to_le_bytes());
state.update(&p.to_le_bytes());
state.update(&q.to_le_bytes());
```

Hash: `blake2b("penumbra_lp_id" | nonce | asset1 | asset2 | fee | p | q)`

### Fonksiyon: `check_stateless()` (satır 163-189)

Doğrulama kuralları:
1. `r1 <= 2^80-1` ve `r2 <= 2^80-1`
2. `r1 != 0 || r2 != 0` (en az biri sıfır değil)
3. `p != 0 && q != 0`
4. `p <= 2^80-1 && q <= 2^80-1`
5. `asset_1 != asset_2` (döngüsel çiftler yasak)
6. `fee <= 5000`

### Struct: `Flows` (satır 229-325)

İki pozisyon snapshot'ı arasındaki fark: delta (giren), lambda (çıkan), fee.

```rust
pub struct Flows {
    pair: DirectedTradingPair,
    delta_1: Amount,  // asset 1 girişi
    delta_2: Amount,  // asset 2 girişi
    lambda_1: Amount, // asset 1 çıkışı
    lambda_2: Amount, // asset 2 çıkışı
    fee_1: Amount,    // asset 1 ücreti
    fee_2: Amount,    // asset 2 ücreti
}
```

**Ücret hesabı (satır 287-289):**
```rust
let fee_1 = Amount::from((delta_1.value() * fee_bps) / 10_000u128);
let fee_2 = Amount::from((delta_2.value() * fee_bps) / 10_000u128);
```

Dikkat: bu hesaplama `BareTradingFunction::fill()` ile tutarlı mı? `fill()` ücreti `effective_price_inv = (p/q)*gamma` üzerinden örtük uyguluyor. `Flows` ise fee_bps'dan açık hesaplıyor. Küçük farklılık olabilir.

### Struct: `Id` (satır 329-330)
```rust
pub struct Id(pub [u8; 32]);
```
Bech32m kodlaması: `plpid1...` prefix. Display/Debug otomatik bech32m.

### Test: `test_position()` (satır 581-702)

6 iddia: yönlü çiftten bağımsız olarak ekonomik olarak eşdeğer pozisyonlar aynı rezerv/fiyat değerlerine sahip.

**Nova Shield Dersi:** 80-bit rezerv limiti overflow koruması sağlar. Kanonik çift mantığı her zaman tutarlı fiyat indeksi garantiler. `close_on_fill` ek durum takibi olmadan limit order simüle eder.

---

## 3. `src/lp/reserves.rs` — Rezerv Yönetimi (104 satır)

```rust
pub struct Reserves {
    pub r1: Amount,
    pub r2: Amount,
}
```

Yardımcı metodlar:
- `check_bounds()`: 80-bit kontrolü + her ikisi sıfır olamaz
- `balance(&pair) -> Balance`: `r1*asset_1 + r2*asset_2`
- `flip() -> Reserves`: `Reserves { r1: r2, r2: r1 }` — ters yön için
- `zero() -> Self`: Default

**Nova Shield Dersi:** `flip()` paterni hem Reserves hem BareTradingFunction'da kullanılıyor. Ters yön swap'ı "çevirme + normal yönde işlem" olarak modellemek zarif bir soyutlama.

---

## 4. `src/batch_swap_output_data.rs` — Batch Swap ZK Veri Modeli (500 satır)

### Struct: `BatchSwapOutputData` (satır 23-42)
```rust
pub struct BatchSwapOutputData {
    pub delta_1: Amount,    // toplam asset_1 girişi
    pub delta_2: Amount,    // toplam asset_2 girişi
    pub lambda_1: Amount,   // 2→1 takaslardan asset_1 çıkışı
    pub lambda_2: Amount,   // 1→2 takaslardan asset_2 çıkışı
    pub unfilled_1: Amount, // 1→2 takasından doldurulamayan asset_1
    pub unfilled_2: Amount, // 2→1 takasından doldurulamayan asset_2
    pub height: u64,
    pub trading_pair: TradingPair,
    pub sct_position_prefix: Position, // commitment=0 zorunlu
}
```

### Kritik Fonksiyon: `pro_rata_outputs()` (satır 47-83)

**Pro-rata formülü (matematiksel):**
```
lambda_2_i = (delta_1_i / delta_1) * lambda_2 + (delta_2_i / delta_2) * unfilled_2
lambda_1_i = (delta_1_i / delta_1) * unfilled_1 + (delta_2_i / delta_2) * lambda_1
```

Yorumu: Kullanıcı hem 1→2 hem 2→1 koymuşsa her iki havuzdan pay alır. `unfilled` pay korunur.

**Sıfır bölme koruması (satır 63-64):**
```rust
let pro_rata_input_1 = (delta_1_i / delta_1).unwrap_or_default();
```
`delta_1 == 0` → pay = 0 (Division by zero → default = 0)

**Yuvarlama:** Her zaman `round_down()` → kullanıcı aleyhine, sistemin lehine.

### ZK Devre Entegrasyonu: `BatchSwapOutputDataVar` (satır 139-276)

`U128x128Var` ile tam R1CS devre. `pro_rata_outputs` aynı formülü R1CS kısıtlamaları olarak uygular.

**Bit kısıtlama (satır 182-188):**
```rust
bit_constrain(epoch.clone(), 16)?;
bit_constrain(block_within_epoch.clone(), 16)?;
```
Epoch ve block: en fazla 16 bit (65535 maksimum)

**Test: `pasiphae_inflation_bug`** (satır 356-389)
Mainnet'te bulunan bir bug'ın regresyon testi. delta_1=31730032, lambda_2=28766268 oranı doğru hesaplanıyor.

**Test: `happy_path_bsod_pro_rata` + ZK proof** (satır 452-498)
delta_1=200, delta_2=300, lambda_1=150, lambda_2=125 → kullanıcı delta_1=100, delta_2=200 koyduysa:
- lambda_2_i = (100/200)*125 + (200/300)*50 = 62.5 + 33.33... = 95 (round down)
- lambda_1_i = (100/200)*23 + (200/300)*150 = 11.5 + 100 = 111 (round down)

Groth16 proof oluşturma + doğrulama testi.

**Nova Shield Dersi:** Batch swap mekanizması tüm kullanıcıları aynı fiyata eşitler → MEV manipülasyonunu azaltır. Pro-rata formülü ZK proof ile doğrulanır → merkezi sunucuya güvene gerek yok.

---

## 5. `src/component/circuit_breaker/execution.rs` — Execution Circuit Breaker (28 satır)

```rust
pub(crate) struct ExecutionCircuitBreaker {
    pub counter: u32,
    pub max: u32,
}
```

`increment()`: counter++
`exceeded_limits()`: `counter >= max`

**Parametreler:** `DexParameters::max_execution_budget = 64` (default)

Önemli: Circuit breaker `exceeded_limits` kontrolü **fill öncesinde** yapılır. Yani 64 iterasyon gerçekleşir, 65'inci engellenir. Kısmi fill mümkün.

**Nova Shield Dersi:** Routing döngüsü sonsuz veya çok uzun çalışmamalı. Circuit breaker BLOK SÜRESINI korur. Parametrik olmalı (genesis'te ayarlanabilir).

---

## 6. `src/component/circuit_breaker/value.rs` — Value Circuit Breaker (298 satır)

### Trait: `ValueCircuitBreaker`

```rust
async fn dex_vcb_credit(&mut self, value: Value) -> Result<()>
async fn dex_vcb_debit(&mut self, value: Value) -> Result<()>
```

**Credit:** `prev_balance + value.amount` (overflow korumalı, `checked_add`)
**Debit:** `prev_balance - value.amount` (underflow korumalı, `checked_sub`)

DEX içindeki her token değer hareketi kayıt altına alınır. Yetersiz balance → hata fırlatılır.

**Entegrasyon:**
- `open_position` → `dex_vcb_credit(r1) + dex_vcb_credit(r2)`
- `withdraw_position` → `dex_vcb_debit(r1) + dex_vcb_debit(r2)`
- `arbitrage profit` → `dex_vcb_debit(arb_profit)`

**Test: `batch_swap_circuit_breaker`** (satır 239-297)
VCB bilgisi olmadan pozisyon state'e doğrudan yazılırsa (hack girişimi) → batch swap sırasında debit underflow → panik.

**Test: `position_value_circuit_breaker`** (satır 145-237)
VCB sıfırlanmış durumda position_withdraw → hata.

**Nova Shield Dersi:** DEX'teki her value change muhasebesi. Asla DEX'ten daha fazla çıkış olamaz. Bu "DEX solvency invariant"ı. Bizim DEX'imizde de zorunlu.

---

## 7. `src/component/arb.rs` — Flash Loan Arbitraj (151 satır)

### Trait: `Arbitrage`

```rust
async fn arbitrage(
    self: &mut Arc<Self>,
    arb_token: asset::Id,
    routing_params: RoutingParams,
) -> Result<Option<Value>>
```

**Algoritma:**
1. `flash_loan = Value { amount: u64::MAX, asset_id: arb_token }` (2^64 sanal token)
2. `route_and_fill(arb_token, arb_token, u64::MAX, params, circuit_breaker)` — kendine dönen yol
3. `total_output = output + unfilled_input`
4. `arb_profit = total_output - flash_loan.amount`
5. Kar sıfır veya negatif → state delta atılır
6. Kar pozitif → state delta uygulanır, `dex_vcb_debit(arb_profit)` → VCB'den çıkarılır

**Flash Loan Modeli:** Sanal borç, blok içinde geri ödenir. Ödeme: dönüş yolundaki gerçek karlılık.

**Kısmen doldurulamayan arbitage (satır 78-84):**
`total_output - flash_loan.amount` negatif olabilir (route_and_fill guarantee'leri neden). Bu durumda arb atılır.

**Kritik:** Arb karı VCB'den debit edilir → DEX'ten value outflow olarak muhasebe. LP'ler arb karını finanse eder (spread marjı üzerinden).

**Nova Shield Dersi:** Block-end arbitrage: her blokta fiyat dengeleme. Bu MEV'i "ön-cephane" yerine "blok sonu" katmanına taşır. Bizim DEX'imizde de böyle yapabiliriz.

---

## 8. `src/component/position_manager.rs` — Pozisyon State Machine (619 satır)

### Trait: `PositionRead`

- `all_positions()`: Tüm pozisyonlar stream
- `positions_by_price(pair)`: Fiyata göre sıralı stream (prefix: `engine::price_index`)
- `best_position(pair)`: İlk (en iyi fiyatlı) pozisyon
- `candidate_set(from, fixed_candidates)`: Routing için aday asset'lar (fixed + dinamik)
- `ordered_routable_assets(start)`: Likiditeye göre sıralı asset'lar

### Trait: `PositionManager`

`open_position()` (satır 275-315):
1. Açık olmalı (State::Opened)
2. ID çakışması yok
3. `dex_vcb_credit(r1) + dex_vcb_credit(r2)` → VCB güncelleme
4. routing params'a yeni asset'ları ekle
5. pair'i aktif olarak işaretle
6. `update_position(None, position)` → indeksler güncellenir

`position_execution()` (satır 340-410):
- `prev_state == new_state` → no-op
- `close_on_fill` kontrolü: r1=0 veya r2=0 → State::Closed
- `update_position(Some(prev), new)` → tüm indeksleri günceller

`withdraw_position()` (satır 415-492):
- sequence=0 → önceki Closed beklenir
- sequence>0 → önceki `Withdrawn { sequence-1 }` beklenir
- `dex_vcb_debit(r1) + dex_vcb_debit(r2)`
- `new_state.reserves = Reserves::zero()` → double-withdraw önleme
- `new_state.state = Withdrawn { sequence }`

`reward_position()` (satır 496-538):
- Sadece staking token içeren pair'e ödül
- `dex_vcb_credit(reward)` → VCB güncelleme

### Özel Trait: `Inner` (satır 543-619)

`update_position()` — TEK VE YEGÂNİ state yazma entrypoint:
1. `guard_invalid_transitions()` → state machine kuralları
2. `update_position_by_inventory_index()`
3. `update_asset_by_base_liquidity_index()`
4. `update_trading_pair_position_counter()`
5. `update_position_by_price_index()`
6. `update_volume_index()`
7. `self.put(state_key::position_by_id(&id), new_state)` → gerçek yazma

**State Machine Kuralları (satır 584-616):**
```
None → Opened        (yeni pozisyon)
Opened → Opened      (execution güncelleme)
Opened → Closed      (kapatma)
Closed → Closed      (no-op)
Closed → Withdrawn{0} (ilk çekilme)
Withdrawn{n} → Withdrawn{n+1}  (tekrar çekilme — ödüller için)
Withdrawn{n} → Withdrawn{n}    (ödül yatırma, sequence değişmez)
```

**Nova Shield Dersi:** State machine geçişleri merkezi bir fonksiyona kilitlenmeli. Tüm indeks güncellemeleri atomik. Hiçbir yerde `position_by_id`'a doğrudan yazılmamalı (yalnızca `update_position` üzerinden).

---

## 9. `src/component/position_manager/price_index.rs` — Fiyat Endeksi (84 satır)

```rust
pub(crate) trait PositionByPriceIndex: StateWrite {
    fn update_position_by_price_index(&mut self, ...) -> Result<()>
    fn deindex_position_by_price(&mut self, position, id)
    fn index_position_by_price(&mut self, position, id)  // inner trait
}
```

**Endeksleme kuralı:**
- `r2 != 0` → 1→2 yönünde endeksle (`pair12` ile `phi12`)
- `r1 != 0` → 2→1 yönünde endeksle (`pair21` ile `phi21=phi12.flip()`)

Yani her rezerv sıfırlandığında o yön için endeksten kaldırılır.

**Anahtar yapısı (state_key::engine::price_index::key):**
103 byte prefix + 32 byte position_id
Positions_by_price'ta `raw_id = k[103..135]` (satır 74 fill_route.rs'de)

Anahtar lexicographic sırası = effective_price sırası → verimli B-tree taraması.

**Nova Shield Dersi:** Fiyat endeksi hem 1→2 hem 2→1 yönünü ayrı tutar. Rezerv durumuna göre dinamik güncelleme. Her yazma işlemi (open/close/execute) endeksleri günceller.

---

## 10. `src/swap/plaintext.rs` — Swap Düz Metin (470 satır)

### Struct: `SwapPlaintext` (satır 28-41)
```rust
pub struct SwapPlaintext {
    pub trading_pair: TradingPair,
    pub delta_1_i: Amount,      // asset_1 giriş miktarı
    pub delta_2_i: Amount,      // asset_2 giriş miktarı
    pub claim_fee: Fee,         // SwapClaim için prepaid ücret
    pub claim_address: Address, // çıktı notu alacak adres
    pub rseed: Rseed,           // rastgele seed
}
```

### Fonksiyon: `swap_commitment()` (satır 97-122)

**Poseidon hash zinciri:**
```
inner_hash4 = hash_4(DOMAIN_SEP, asset_1, asset_2, delta_1_i, delta_2_i)
commitment = hash_7(DOMAIN_SEP,
    rseed, claim_fee.amount, claim_fee.asset_id,
    diversified_generator, transmission_key_s, clue_key,
    inner_hash4)
```

Bu `StateCommitment` SwapClaim proof'unda kullanılır.

### Fonksiyon: `output_notes()` (satır 65-92)

Pro-rata çıktıları alır, iki `Note` oluşturur:
- Note1: lambda_1_i miktarında asset_1
- Note2: lambda_2_i miktarında asset_2
- Her ikisi de claim_address'e

### Fonksiyon: `encrypt()` (satır 132-146)
OVK ile şifreleme: `PayloadKey::derive_swap(ovk, commitment)` → `key.encrypt_swap(plaintext)`

### Serileştirme Formatı (satır 338-351)
```
[0..64]    trading_pair (64 bytes)
[64..80]   delta_1_i (16 bytes, Amount = u128)
[80..96]   delta_2_i (16 bytes)
[96..112]  claim_fee.amount (16 bytes)
[112..144] claim_fee.asset_id (32 bytes)
[144..224] claim_address (80 bytes)
[224..256] rseed (32 bytes)
Toplam: 256 bytes = SWAP_LEN_BYTES
```

**Nova Shield Dersi:** Swap commitment poseidon hash ile ZK devre dostu. `SwapPlaintextVar` ile R1CS devre içinde aynı commitment hesaplanır → prover/verifier tutarlılığı.

---

## DEX Uygulama Notu (Nova Shield için)

### AMM Modeli Seçimi
Penumbra'nın sabit-toplam CFMM'i her pozisyonu bağımsız limit emir olarak modeller. `effective_price = (q/p) / gamma` formülü ile:
- p/q oranı = temel döviz kuru
- gamma = fee iskontolu etkin oran
- Her pozisyon belirli bir fiyat noktasında çalışır (concentrated liquidity gibi)

**Nova Shield DEX'i için öneri:** Penumbra'nın LP modeli ile Uniswap v3 concentrated liquidity kombinasyonu. Sabit-toplam LP basit ve audit edilebilir.

### Kritik Değerler (doğrulanmış kaynak koddan)
- `MAX_RESERVE_AMOUNT = (1 << 80) - 1` = 1.208.925.819.614.629.174.706.175
- `MAX_FEE_BPS = 5000` (%50 maximum)
- `gamma = (10_000 - fee) / 10_000`
- `effective_price = (q/p) / gamma`
- `effective_price_inv = (p/q) * gamma`
- Fill rounding: **her zaman output aşağı, input yukarı yuvarlanır**

### Flash Loan Arbitraj
Blok sonunda otomatik arbitraj: `flash_loan = u64::MAX`. Kar pozitifse state uygulanır, VCB'den düşülür. LP'ler arb zararı yaşar (spread bunu karşılar).

### ZK Batch Swap
`pro_rata_outputs` hem Rust'ta hem R1CS devresinde aynı formül. SwapClaim proof'u:
- Genel: anchor, nullifier, claim_fee, output_data, note_commitment_1, note_commitment_2
- Özel: swap_plaintext, state_commitment_proof, ak, nk, lambda_1, lambda_2

### Pozisyon Lifecycle
```
None → Opened → (execution*) → Closed → Withdrawn{0} → (rewards) → Withdrawn{0} → Withdrawn{1} → ...
```

State machine ihlali → anında hata. VCB her transition'da güncellenir.
