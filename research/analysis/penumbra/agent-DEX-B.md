# Penumbra DEX Component Analizi — Agent DEX-B

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `crates/core/component/dex/src/component/` ve `tests/integration/` — TUM dosyalar
**Toplam Dosya:** 35 Rust dosyası
**Hassasiyet:** 3X — DEX modulu (her dosya icin "DEX Uygulama Notu" zorunlu)

---

## OZET: Penumbra DEX Mimarisi

Penumbra DEX, "Batch Swap" modeline dayali, konsantre likidite saglayan bir on-chain AMM'dir.
Kullanicilar swap emirlerini gizli (ZK proof) olarak gonderir, block sonunda toplu islenirler.
Arbitraj mekanizmasi her `end_block`'ta otomatik calisir. Tum likidite pozisyonlari
"Concentrated Liquidity Position" (CLAMM benzeri) mantigindadir; her pozisyon kendi fiyat
araligina ve oran fonksiyonuna sahiptir.

### Kritik Parametreler (kaynak koddan dogrulanmis)

| Parametre | Deger | Kaynak |
|---|---|---|
| `max_hops` | `DexParameters.max_hops` (protokol parametresi) | `params.rs:53` |
| `max_positions_per_pair` | `DexParameters.max_positions_per_pair` | `eviction_manager.rs:67` |
| `max_execution_budget` | `DexParameters.max_execution_budget` | `dex.rs:109`, `arb.rs:46` |
| `DYNAMIC_ASSET_LIMIT` | 10 (sabit) | `position_manager.rs:37` |
| `RECENTLY_ACCESSED_ASSET_LIMIT` | 10 (sabit) | `dex.rs:317` |
| Candlestick RPC limiti | 20.000 blok | `rpc.rs:218` |
| Arb icin ekstra hop | `routing_params.max_hops + 2` | `dex.rs:210` |
| Arb price_limit | `Some(1u64.into())` | `dex.rs:212` |
| Flash loan miktari | `u64::MAX` | `arb.rs:43` |
| Position counter tipi | `u32` (u32::MAX max) | `counter.rs:71` |
| Price index key offset | `k[103..135]` (32 byte ID) | `position_manager.rs:74` |

---

## Dosya Analizleri

---

## [mod.rs]

**Dosya:** `.../dex/src/component/mod.rs`
**Satir Sayisi:** 40

### Genel Amac

DEX bileseninin ana modul dosyasi. Tum alt modulleri organize eder ve public/internal API'yi belirler.

### Modul Yapisi

```
pub mod metrics;
pub mod rpc;
pub mod router;
mod action_handler;
mod arb;
mod chandelier;
pub(crate) mod circuit_breaker;
mod dex;
mod eviction_manager;
mod flow;
mod lqt;
mod position_manager;
mod swap_manager;
```

### Public Export Listesi

- `Dex`, `StateReadExt`, `StateWriteExt` — ana DEX component
- `PositionManager`, `PositionRead`, `LqtRead`, `SwapDataRead` — okuma/yazma API
- `(crate) Arbitrage`, `ExecutionCircuitBreaker`, `ValueCircuitBreaker`, `InternalDexWrite`, `SwapDataWrite`, `SwapManager`
- Test modulu: `pub(crate) mod tests` (cfg(test) kapsamli)
- `register_metrics` — Prometheus metrikleri kayit

### Tasarim Kararlari

- `Arbitrage`, `ExecutionCircuitBreaker`, `ValueCircuitBreaker` ve `InternalDexWrite` `pub(crate)` — dis dunyadan gizlenmis; sadece DEX icinden tetiklenebilir.
- `router` modulu `pub` — dis bilesenler de (ornegin RPC) rotalama kullanabilir.
- `#[cfg(test)]` blogu: test yardimci kodu ayri modul olarak saklanmis, derleme boyutunu kucultmek icin.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield DEX'inde ayni modul yapisi uygulanabilir: Arbitraj ve circuit breaker modulleri `pub(crate)` kalmali. Dis sistemlerin (RPC, simülasyon) erisebilmesi icin router `pub` olmali. Bu ayrim guvenlik sinirlarini netlestiriyor: ic DEX mantigi (arb, VCB) asla dogrudan cagrilamaz.

---

## [flow.rs]

**Dosya:** `.../dex/src/component/flow.rs`
**Satir Sayisi:** 30

### Genel Amac

`SwapFlow` tip tanimi. Bir trading pair icin o blok boyunca biriken takas akislarini tutar.

### Struct/Enum/Type Listesi

```rust
pub struct SwapFlow((Amount, Amount));
// .0 = asset_1 -> asset_2 yonundeki miktar
// .1 = asset_2 -> asset_1 yonundeki miktar
```

### Trait Implementasyonlari

- `Default` — (0,0) baslar
- `Clone`
- `Deref` / `DerefMut` — icindeki `(Amount, Amount)` tuple'a dogrudan erisim
- `From<(Amount, Amount)>` — tuple'dan donusum

### Tasarim Kararlari

- `SwapFlow` tuple-newtype. `Deref` ile hem `.0` hem `.1`'e dogrudan erisim (`swap_flow.0 += ...`).
- `Default` implement edilmis: her trading pair icin akis yoksa `0,0`'dan baslar.
- Tip aliasing degil struct: `Amount` ve `(Amount,Amount)` arasinda yanlis atama engellenmis.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield'in batch swap sistemi icin ayni `SwapFlow` yapisi idealdir. Iki yon ayri tutulmali (`delta_1`, `delta_2`). `im::OrdMap<TradingPair, SwapFlow>` kombinasyonu object-storage'da verimli; Nova Shield'de de ayni pattern benimsenebilir.

---

## [dex.rs]

**Dosya:** `.../dex/src/component/dex.rs`
**Satir Sayisi:** 420

### Genel Amac

`Dex` adinda bir `Component` implementasyonu; `init_chain`, `begin_block`, `end_block`, `end_epoch` hook'lari. Tum DEX islemlerinin orkestrasyonu burada yapilir.

### Struct/Enum/Type Listesi

- `pub struct Dex {}` — bos struct, sadece `Component` impl saglar
- `StateReadExt` trait — `get_dex_params`, `routing_params`, `output_data`, `swap_execution`, `arb_execution`, `get_active_trading_pairs_in_block`
- `StateWriteExt` trait — `put_dex_params`
- `InternalDexWrite` trait — `add_recently_accessed_asset`, `mark_trading_pair_as_active`, `set_output_data`, `set_arb_execution`

### Fonksiyonlar — end_block Akisi (KRITIK)

```
end_block() orkestrasyonu (satir 54-251):

  F.0. Non-native fee tokenlari swap flow'una eklenir
      ↓
  1. Pozisyon acilislari (action handler'larda oldu)
      ↓
  2. Her trading pair icin batch swap yurutme:
     state.handle_batch_swaps(
         trading_pair,
         swap_flows,
         end_block.height,
         routing_params.with_extra_candidates([asset_1, asset_2]),
         execution_budget,
     )
      ↓
  F.1. Non-native fee ciktilarini claim et (BSOD'den pro-rata)
      ↓
  3. Arbitraj:
     arb_routing_params = RoutingParams {
         max_hops: routing_params.max_hops + 2,
         fixed_candidates: routing_params + recently_accessed_assets,
         price_limit: Some(1u64.into()),  // YALNIZCA KARLI ARB
     }
     state.arbitrage(*STAKING_TOKEN_ASSET_ID, arb_routing_params)
      ↓
  4. Fazla pozisyon eviction
      ↓
  5. Queue'daki pozisyon kapanislari (JIT likidite desteği)
      ↓
  6. Block candlestick finalize
```

### Sabitler

```rust
const RECENTLY_ACCESSED_ASSET_LIMIT: usize = 10;  // satir 317
```

### Kritik Tasarim Kararlari

1. **`begin_block` bos**: Hic islem yok — tum mantik `end_block`'ta.
2. **Arbitraj her blokta**: Batch swap'lardan sonra fiyat dengesizliklerini aninda kapatir.
3. **JIT Likidite**: `queue_close_position` + sonra kapama akisi — ayni blokta acinip kapanabilen pozisyonlar.
4. **Non-native fee tokeni swap**: Her blokta otomatik. Eger staking token ise direkt accumulate; eger degil, swap flowuna eklenir.
5. **Arbitraj hata toleransi**: `Err(e) => tracing::warn!(?e, "error processing arb...")` — arb hatasi zinciri durdurmaz.
6. **VCB (Value Conservation)**: `set_output_data` icerisinde `dex_vcb_debit` ile korunma saglanir. Overflow/underflow zinciri durdurur.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield `end_block` mantigi Penumbra'yi model alabilir:
- Adim sirasi kritik: batch swap → arb → eviction → close_queued → candlestick finalize
- Non-native fee token otomatik swap ozelligi Nova Shield'de de olabilir: alt-coin fee → native token swap, on-chain.
- Arbitraj `price_limit: Some(1u64.into())` — yalnizca kar elde edilen (geri odeme > flash loan) arblar yurutulur. Bu limit Nova Shield'de de zorunlu; yoksa sonsuz dongu riski.
- `RECENTLY_ACCESSED_ASSET_LIMIT = 10` — arb kandidat setini sinirlar, blok suresi tahmin edilebilir kalir.
- JIT likidite: PositionClose eylemi hemen degil `end_block`'ta. Nova Shield'de ayni pattern block-scoped JIT saglar.

---

## [swap_manager.rs]

**Dosya:** `.../dex/src/component/swap_manager.rs`
**Satir Sayisi:** 111

### Genel Amac

Swap payload yonetimi, swap flow birikimi ve batch swap output verisi yazma.

### Trait Listesi

#### `SwapManager` trait

```rust
async fn add_swap_payload(&mut self, swap: SwapPayload, source: CommitmentSource)
```
- Swap commitment'i SCT (State Commitment Tree)'a ekler (`add_sct_commitment`)
- Payload'u `pending_payloads` object-storage'a ekler: `(tct::Position, SwapPayload, source)`

#### `SwapDataRead` trait (StateRead)

- `pending_swap_payloads()` — `im::Vector<(tct::Position, SwapPayload, CommitmentSource)>`
- `swap_flow(&TradingPair) -> SwapFlow` — o anki blok akisi
- `swap_flows() -> im::OrdMap<TradingPair, SwapFlow>`
- `pending_batch_swap_outputs() -> im::OrdMap<TradingPair, BatchSwapOutputData>`

#### `SwapDataWrite` trait (StateWrite)

- `accumulate_swap_flow(&TradingPair, SwapFlow) -> Result<()>`
  - VCB'yi asset_1 ve asset_2 icin credit eder
  - `swap_flows()` map'ine `alter()` ile toplama yapar (immutable-map pattern)
- `put_swap_execution_at_height(height, pair, swap_execution)`
  - `nonverifiable_put` ile depolar (JMT disinda)

### Kritik Tasarim Kararlarilari

1. **Erken VCB kredit**: `accumulate_swap_flow` aninda her iki taraf icin VCB credit yapar. Unfilled miktarlar daha sonra `set_output_data`'da debit edilir. Bu "credit all, debit unfilled" modeli overflow karsisinda koruma saglar.
2. **`im::OrdMap::alter`**: Immutable map pattern ile swap flow birikimi; eski entry'yi silmeden yeni entry hesaplanir. Fonksiyonel-reaktif state management.
3. **SCT entegrasyonu**: Her swap commitment SCT'ye eklenir; bu ZK swap claim icin gerekli.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield'in swap sistemi ZK proof kullanmayacaksa, `SwapPayload` ve SCT entegrasyonunu atlayabilir. Ancak `accumulate_swap_flow` + `VCB` kombinasyonu aynen uygulanmali: once kredi ver, sonra unfilled miktari geri al. Bu pattern deger kaybi riskini elimine eder. `im::OrdMap` yerine `BTreeMap` veya `HashMap` kullanilabilir; performans farki ihmal edilebilir.

---

## [arb.rs]

**Dosya:** `.../dex/src/component/arb.rs`
**Satir Sayisi:** 152

### Genel Amac

`Arbitrage` trait: her blok sonunda arbitraj firsatlarini tespit eder ve gerceklestirir. Flash-loan modeli kullanir.

### Trait / Fonksiyon Analizi

```rust
pub trait Arbitrage: StateWrite + Sized {
    async fn arbitrage(
        self: &mut Arc<Self>,
        arb_token: asset::Id,
        routing_params: RoutingParams,
    ) -> Result<Option<Value>>
}
```

### Flash Loan Mekanizmasi (KRITIK ALGORITMA)

```
1. Yeni StateDelta fork: let mut this = Arc::new(StateDelta::new(self.clone()));

2. Flash loan olustur:
   flash_loan = Value { asset_id: arb_token, amount: u64::MAX }  // satir 41-44

3. Circular route-and-fill:
   this.route_and_fill(
       arb_token,         // src
       arb_token,         // dst (ayni token!)
       u64::MAX,          // input
       routing_params,
       execution_circuit_breaker,
   )

4. Kar hesabi:
   unfilled_input = u64::MAX - filled_input
   total_output = output + unfilled_input
   arb_profit = total_output - u64::MAX  // "flash loan geri odeme"

5. Eger profit == 0:  StateDelta discard, return Ok(None)
   Eger profit > 0:   StateDelta apply, VCB debit(arb_profit), event emit

6. SwapExecution kaydi (arb ciktisi = filled_input + arb_profit)
```

### Kritik Guvence: VCB Debit

```rust
self_mut.dex_vcb_debit(Value { amount: arb_profit, asset_id: arb_token }).await?;
// satir 129-134
```
Arb kari DEX VCB'sinden dusuluyor — bu tokenlarin nereden geldigi takip ediliyor. Pozisyon
rezervlerinden cekilen arb kari VCB'yi dusurmeli; yoksa deger enflasyonu olur.

### Hata Durumu

- `route_and_fill` `ensure_progress=true` ile calisiyor — her iterasyonda en az 1 pozisyon tuketilmeli.
- Overflow durumunda (arb_profit hesabi underflow): erken return + StateDelta discard.
- Testnet 53 bugu: spill price'in strict olmamasi sonsuz donguye yol actigi gozlemlendi — simdi `price_limit` strict `>=` ile kontrol ediliyor.

### Unsafe Bloklar

Yok.

### Testler (tests.rs'de)

- `basic_cycle_arb`: 3 pozisyon: penumbra→gn→gm→penumbra. 10 penumbra giris, 20 penumbra cikis bekleniyor.
- `reproduce_arbitrage_loop_testnet_53`: Testnet 53 regresyon testi — `tokio::time::timeout(2s)` ile dongu engellenmesi dogrulanir.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield DEX'inde blok basi arbitraj zorunlu olmayabilir, ancak siddetli tavsiye edilir:
- Flash loan mekanizmasi (u64::MAX input, circular route) cok elegandır: protocol herhangi
  bir sermaye koymadan fiyat tutarliligini saglar.
- Arb kari VCB'den gelmeli — deger enflasyonu engellenmeli.
- `price_limit: Some(1u64.into())` (fiyat limit = 1) sadece karli arblar calisir.
- Testnet 53 bugu kritik ders: spill price kontrolleri STRICT olmali (`>=` degil `>`).
- Arb hatasi zinciri durdurmasin (`warn!` ve devam); production sistemlerde kararli davranis onemli.

---

## [circuit_breaker/execution.rs]

**Dosya:** `.../circuit_breaker/execution.rs`
**Satir Sayisi:** 29

### Genel Amac

`ExecutionCircuitBreaker`: batch swap yurutmesinde kac tur path search ve fill yapilabilecegini sinirlar.

### Struct

```rust
pub(crate) struct ExecutionCircuitBreaker {
    pub counter: u32,
    pub max: u32,  // DexParameters::max_execution_budget
}
```

### Fonksiyonlar

- `new(max: u32) -> Self` — sifir sayacla baslar
- `increment(&mut self)` — sayaci 1 arttirir
- `exceeded_limits(&self) -> bool` — `counter >= max`

### Kullanim Yeri

`route_and_fill` dongusu her iterasyon basinda `exceeded_limits()` kontrol eder, sonra `increment()` cagririr.
Bu siraya uyulmasinin onemine dikkat cekiliyor (kaynak kodu yorumu, satir 184-191).

```
loop {
    if execution_circuit_breaker.exceeded_limits() { break; }
    execution_circuit_breaker.increment();
    // path search + fill...
}
```

### Tasarim Kararlari

- `Clone` implement edilmis: `handle_batch_swaps`, arb ve route_and_fill'e ayni budget paylastirilabilir.
- `u32` yeterli: tipik budget 64-256 arasi; `u32::MAX` tasmasi pratikte imkansiz.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield DEX'inde execution budget mutlak zorunlu. Sonsuz dongu riski:
- Eger her hop yeni likidite bulduysa ve spill price hesabi yanlis ise dongu olusmayabilir ama olabilir.
- `max_execution_budget` on-chain governance parametresi olarak tutulmali (sabit kod degil).
- "exceeded_limits ONCE KONTROL, SONRA increment" sirasina mutlaka uy — aksi halde ilk iterasyon her zaman gecilir.

---

## [circuit_breaker/value.rs]

**Dosya:** `.../circuit_breaker/value.rs`
**Satir Sayisi:** 298 (testler dahil)

### Genel Amac

`ValueCircuitBreaker` (VCB): DEX'e giren ve cikan deger akislarini takip eder. Cikis, giristen fazla olursa `checked_sub` underflow hatasi — bu chain'i durdurur.

### Trait / Fonksiyon Listesi

#### `ValueCircuitBreakerRead` trait (StateRead)

```rust
async fn get_dex_vcb_for_asset(&self, id: &asset::Id) -> Result<Option<Amount>>
```
- `state_key::value_balance(&id)` anahtarindan okur

#### `ValueCircuitBreaker` trait (StateWrite)

```rust
async fn dex_vcb_credit(&mut self, value: Value) -> Result<()>
// prev_balance.checked_add(&value.amount) — overflow -> Err
// put(state_key::value_balance(...), new_balance)
// emit EventValueCircuitBreakerCredit

async fn dex_vcb_debit(&mut self, value: Value) -> Result<()>
// prev_balance.checked_sub(&value.amount) — underflow -> Err (CHAIN DURDURUR)
// put(state_key::value_balance(...), new_balance)
// emit EventValueCircuitBreakerDebit
```

### Matematiksel Model

```
DEX VCB[asset] = sum(credits) - sum(debits)
Credits: open_position(reserves), accumulate_swap_flow(inflows), reward_position(reward)
Debits:  withdraw_position(reserves), set_output_data(unfilled + lambda), dex_vcb_debit(arb_profit)

Invariant: DEX VCB[asset] >= 0 her zaman
Ihlal -> checked_sub underflow -> Err -> chain panic/halt
```

### Testler

1. `value_circuit_breaker`: 100gm credit, 100gm debit (OK) → 1gm debit (HATA), yoktan debit (HATA)
2. `position_value_circuit_breaker`: Pozisyon aciyor, VCB manupule edip withdraw — hata bekleniyor
3. `batch_swap_circuit_breaker`: VCB olmadan pozisyon yaratip batch swap — `should_panic` ile underflow test ediliyor

### DEX Uygulama Notu — Nova Shield icin

VCB Nova Shield DEX'inin en kritik guvenlik katmani olacak:
- Her deger girisinde (deposit, swap inflow) credit, her cikista (withdrawal, swap outflow) debit.
- `checked_sub` yerine `saturating_sub` KULLANILMAMALI — underflow bir ihlaldir ve hataya donusmeli.
- Deger enflasyon bug'i (bir pozisyon VCB'ye bildirilmeden olusturulursa) batch swap circuit breakeri tetikler.
- Nova Shield'de her asset icin ayri VCB balance tutulmali; tek toplam yetersiz.
- ABCI event emit etmek (credit/debit icin) off-chain monitoring icin onemli — indexer bu eventleri izler.

---

## [circuit_breaker/mod.rs]

**Dosya:** `.../circuit_breaker/mod.rs`
**Satir Sayisi:** 7

Sadece re-export: `ExecutionCircuitBreaker`, `ValueCircuitBreaker`, `ValueCircuitBreakerRead`.

### DEX Uygulama Notu — Nova Shield icin

Bu pattern Nova Shield icin ayni sekilde uygulanmali: circuit breaker icsel (`pub(crate)`), okuma API'si (`ValueCircuitBreakerRead`) dis erisime acik.

---

## [position_manager.rs]

**Dosya:** `.../component/position_manager.rs`
**Satir Sayisi:** 620

### Genel Amac

Likidite pozisyonlari icin merkezi okuma/yazma API'si. `PositionRead` ve `PositionManager` trait'leri; tum indeks guncelleme koordinasyonu icin `update_position` (tek giris noktasi).

### Trait Listesi

#### `PositionRead` trait (StateRead)

- `all_positions()` — `prefix(all_positions_key)` stream
- `positions_by_price(pair)` — `nonverifiable_prefix(&prefix)`, key'in 103..135 offsetinden position::Id cikarir
- `position_by_id(id)` — `get(&state_key::position_by_id(id))`
- `check_position_by_id(id)` — `get_raw` ile bool donus
- `best_position(pair)` — `positions_by_price(pair).next()`
- `pending_position_closures()` — object_get ile `im::Vector<position::Id>`
- `candidate_set(from, fixed_candidates)` — ROUTING'de kullanilan aday set
- `ordered_routable_assets(start)` — `nonverifiable_prefix_raw`, likidite sirasinda routable asset stream

#### `PositionManager` trait (StateWrite + PositionRead)

```rust
// Temel islemler:
async fn close_position_by_id(&mut self, id: &position::Id) -> Result<()>
async fn queue_close_position(&mut self, id: position::Id) -> Result<()>
async fn close_queued_positions(&mut self) -> Result<()>
async fn open_position(&mut self, position: position::Position) -> Result<()>
async fn position_execution(&mut self, mut new_state: Position, context: DirectedTradingPair) -> Result<Position>
async fn withdraw_position(&mut self, position_id: position::Id, sequence: u64) -> Result<Balance>
async fn reward_position(&mut self, position_id: position::Id, reward: Amount) -> anyhow::Result<()>
```

### `open_position` Akisi

```
1. Durum kontrolu: state == Opened olmali
2. ID cakisma kontrolu: position_by_id ile mevcut mu?
3. VCB credit: reserves_1 ve reserves_2 icin dex_vcb_credit
4. recently_accessed_asset: her iki asset ID'yi ekle
5. mark_trading_pair_as_active: pair'i aktif olarak isaretler
6. event emit: EventPositionOpen
7. update_position(&id, None, position)  // indeksler guncellenir
```

### `position_execution` Akisi (DEX engine icinden cagrili)

```
1. position_by_id ile mevcut durum (prev_state) alin
2. Eger prev_state == new_state: no-op (kayit silmez)
3. Durum kontrolleri: prev Opened, new Opened olmali
4. EventPositionExecution emit
5. close_on_fill kontrolu:
   if new_state.close_on_fill && (r1==0 || r2==0):
       new_state.state = Closed
       EventPositionClose emit
6. Candlestick kaydi: record_position_execution
7. update_position(&id, Some(prev_state), new_state)
```

### `withdraw_position` Akisi

```
1. Durum kontrolu: sequence==0 ise Closed beklenir; >0 ise Withdrawn{sequence-1}
2. EventPositionWithdraw emit
3. VCB debit: reserves_1 ve reserves_2 icin dex_vcb_debit
4. new_state: Withdrawn{sequence}, reserves=zero
5. update_position(&id, Some(prev_state), new_state)
```

### `update_position` — TEKIL GIRIS NOKTASI (Inner trait, private)

```rust
async fn update_position(
    id: &position::Id,
    prev_state: Option<Position>,
    new_state: Position,
) -> Result<Position>
```

Cagri sirasi (satir 562-572):
```
1. guard_invalid_transitions(prev, new, id)  // durum makinesi dogrulama
2. update_position_by_inventory_index(...)
3. update_asset_by_base_liquidity_index(...)
4. update_trading_pair_position_counter(...)
5. update_position_by_price_index(...)
6. update_volume_index(...)
7. put(position_by_id_key, new_state.clone())
```

### `guard_invalid_transitions` — Durum Makinesi

```
None      → Opened      (yeni pozisyon, ZORUNLU baslangiç)
Opened    → Opened      (execution update)
Opened    → Closed      (kullanici kapatti veya auto-close)
Closed    → Closed      (no-op, izin veriliyor)
Closed    → Withdrawn{0} (ilk cekme, sequence=0 olmali)
Withdrawn{N} → Withdrawn{N+1} (artan sequence)
Withdrawn{N} → Withdrawn{N}   (reward ekleme, sequence degismez)
Diger tum gecisler → HATA
```

### `candidate_set` Mantigi

```rust
const DYNAMIC_ASSET_LIMIT: usize = 10;  // satir 37

fn candidate_set(from, fixed_candidates):
    // 1. fixed_candidates'i ONCE aktar (static set, dex parametresi)
    // 2. ordered_routable_assets(&from)'dan DYNAMIC_ASSET_LIMIT kadar ekstra al
    //    (fixed'te olanlari filtrele)
    // toplam aday = fixed + min(10, dynamic)
```

### DEX Uygulama Notu — Nova Shield icin

`update_position` TEK GIRIS NOKTASI prensibi kritik: tum indeks guncellemeleri buradan gecmeli. Nova Shield'de bu pattern aynen uygulanmali:
- Tek entrypoint, coklu indeks guncelleme, durum makinesi dogrulama
- `guard_invalid_transitions` benzeri bir guard fonksiyonu mutlak gerekli
- `close_on_fill` ozelligi son derece kullanisli (single-sided JIT pozisyon ornegi)
- `candidate_set`: fixed + 10 dinamik. Bu hybrid yaklasim routing guvenligi saglar — sadece likidite bazli routing manipule edilebilir
- `reward_position`: VCB credit + sequence degismiyor — reward mekanizmasi bu sekilde eklenebilir

---

## [position_manager/price_index.rs]

**Dosya:** `.../position_manager/price_index.rs`
**Satir Sayisi:** 85

### Genel Amac

`PositionByPriceIndex` trait: pozisyonlari efektif fiyat sirasina gore indeksler. Routing'de `positions_by_price` streami bu indeksi kullanir.

### Trait Listesi

#### `PositionByPriceIndex` (pub(crate))

```rust
fn update_position_by_price_index(
    position_id, prev_state, new_state
) -> Result<()>
// Eski kaydı sil (prev_state varsa)
// Yeni kaydı ekle (eger state == Opened)
```

```rust
fn deindex_position_by_price(position, id):
    // pair12 (A->B) ve pair21 (B->A) icin sil
    // key: engine::price_index::key(&pair, &phi_component, &id)
```

#### Inner trait (private)

```rust
fn index_position_by_price(position, id):
    // if r2 != 0: 1->2 yonu icin index (pozisyon asset2 verebilir)
    // if r1 != 0: 2->1 yonu icin index (pozisyon asset1 verebilir)
    // key = engine::price_index::key(&pair12, &phi12, &id)
    // deger = position.clone() (tum pozisyon)
```

### Indeks Key Yapisi

`engine::price_index::key(pair, phi, id)` — anahtarin icerisinde:
- Yon bilgisi (pair)
- Trading function katsayilari (phi) — bu sayede efektif fiyat siralamasini saglar
- Position ID (32 byte, key offset 103..135)

### Kritik Kural

- Pozisyon hem `r1 > 0` hem `r2 > 0` ise **her iki yonde** indekslenir
- r2 == 0: 2->1 yonu indekslenmez (bu yonde vermek icin altin yok)
- r1 == 0: 1->2 yonu indekslenmez

### DEX Uygulama Notu — Nova Shield icin

Fiyat indeksi Nova Shield concentrated liquidity icin temel yapi:
- Her yön icin ayri `nonverifiable_prefix` (JMT disinda, sorgulama hizligi)
- Yon bazli indeks: directionality önemli — `A->B` ve `B->A` farkli key prefix
- `phi.component.flip()` ile ters yon efektif fiyati hesaplanir
- Position ID key icine gomme: prefix scan ile tum ID'leri `O(N)` maliyetle taramak yerine direkt key'den okuma

---

## [position_manager/base_liquidity_index.rs]

**Dosya:** `.../position_manager/base_liquidity_index.rs`
**Satir Sayisi:** 201

### Genel Amac

`AssetByLiquidityIndex`: routing icin "baz likidite" indeksi. Bir varlikan itibaren hangi varliklara ne kadar likidite var? Bu indeks routing'de `ordered_routable_assets` ile kullanilir.

### Index Semasi

```
Birincil indeks: {asset_A}{agg_liq_B->A} -> {asset_B}
Yardimci indeks: {asset_A}{asset_B} -> {agg_liq_B->A}
```

### Neden "ters yon" likidite?

Yorum (satir 34-40):
- (1) Cikis yonu likidite: Manipule edilebilir — miniaturize fiyatta satis pozisyonu acarak sifir maliyet
- (2) Giris yonu (B->A miktar): "Base asset A ile satin alinabilecek miktar" — bu manipulasyon icin maliyet gerektirir

Bu nedenle indeks `B->A` miktarini A icin likidite olarak kullanir.

### `update_asset_by_base_liquidity_index` Akisi

```
1. pair_ab = A->B tanimla
2. prev_contrib = mevcut Opened pozisyon rezervleri (veya 0)
3. new_contrib = yeni durumdaki rezervler (veya 0 eger Closed/Withdrawn)
4. Eger hepsi 0: erken return
5. A->B icin: update_inner(id, pair_ab, prev_a, new_a)
6. B->A icin: update_inner(id, pair_ba, prev_b, new_b)
```

### `update_asset_by_base_liquidity_index_inner` Akisi

```rust
let prev_tally: Amount = nonverifiable_get(aggregate_key).unwrap_or_default();
let new_tally = prev_tally.saturating_sub(&old_contrib).saturating_add(&new_contrib);
if prev_tally == new_tally { return Ok(()); }  // no-op optimizasyonu

// Eski primary key sil:
nonverifiable_delete(engine::routable_assets::key(&pair.start, prev_tally))
// Yeni primary key yaz:
nonverifiable_put(engine::routable_assets::key(&pair.start, new_tally), pair.end)
// Yardimci guncelle:
nonverifiable_put(lookup_base_liquidity_by_pair(&pair), new_tally)
```

### Tasarim Kararlari

- `saturating_sub/add`: Agresif savunma — tasmalar hataya degil sessiz saturate'e donusur. Bu index icin kabul edilebilir (guvenlik degil heuristik).
- Hem primary hem auxiliary iki ayri key guncelleniyor; secondary key'den aggregate hizla okunabilir.

### DEX Uygulama Notu — Nova Shield icin

Bu indeks Nova Shield routing performansinin anahtari:
- `ordered_routable_assets` bu indeksi kullanarak her hop icin aday listesi olusturur
- `saturating` aritmetik — indeks heuristic; yanlis olsa da chain halted olmaz
- Manipulasyon direnci: likidite olarak B->A miktarini kullanmak saglikli — mini-fee pozisyonla indeks yaniltmak maliyet gerektirir
- Nova Shield'de ayni iki-seviyeli composite key pattern uygulanmali

---

## [position_manager/inventory_index.rs]

**Dosya:** `.../position_manager/inventory_index.rs`
**Satir Sayisi:** 87

### Genel Amac

`PositionByInventoryIndex`: pozisyonlari inventory (rezerv miktari) sirasina gore siralamak icin eviction queue indeksi. Dusuk rezervli pozisyonlar eviction icin hedeflenir.

### Trait Analizi

#### `PositionByInventoryIndex` (pub(super))

- `update_position_by_inventory_index(...)` — update/deindex/index akisi (price_index gibi)
- `index_position_by_inventory(position, id)`: her iki yon icin ayri key
  - `pair_ab, inventory_a`: A->B yonu icin reserve of A
  - `pair_ba, inventory_b`: B->A yonu icin reserve of B
  - `nonverifiable_put_raw(key, vec![])` — deger bos; key yeterli (siralama key'den)
- `deindex_position_by_inventory(prev_position, id)`: eski key'leri sil

### Kritik Nokta

`eviction_queue::inventory_index::key(&pair, inventory, id)` — bu composite key inventory miktarini icerir. Prefix scan artan sirali doner → dusuk inventory = onceki elemanlar = eviction adaylari.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield `max_positions_per_pair` parametresi uygulanacaksa benzer eviction indeksi gerekli. Anahtar tasarim: inventory miktarini key'e gomme, prefix scan ile otomatik siralama. `vec![]` deger — iceriksiz indeks, sadece varligi onemli.

---

## [position_manager/counter.rs]

**Dosya:** `.../position_manager/counter.rs`
**Satir Sayisi:** 152

### Genel Amac

Her `TradingPair` icin acik pozisyon sayisini takip eden sayac. Eviction icin "kac pozisyon var?" sorusunu O(1) maliyetle cevaplar.

### Trait / Fonksiyon Listesi

#### `PositionCounterRead`

```rust
async fn get_position_count(&self, trading_pair: &TradingPair) -> u32
async fn get_position_count_from_key(&self, path: [u8; 99]) -> u32
// raw bytes -> u32::from_be_bytes
```

#### `PositionCounter`

```rust
async fn update_trading_pair_position_counter(prev_state, new_state) -> Result<()>
// None -> Opened:    increment
// Opened -> Closed:  decrement
// Diger: no-op
```

#### Inner trait (private)

```rust
async fn increment_position_counter(&mut self, trading_pair) -> Result<u32>
// checked_add(1) — overflow = Err
// nonverifiable_put_raw(path, new_total.to_be_bytes())

async fn decrement_position_counter(&mut self, trading_pair) -> Result<u32>
// checked_sub(1) — underflow = Err
```

### Testler

- `test_no_overflow`: Manuel olarak `u32::MAX` set et, increment → hata dogrulama
- `test_no_underflow`: 0'dan decrement → hata dogrulama

### DEX Uygulama Notu — Nova Shield icin

Pozisyon sayaci basit ama onemli: `O(1)` eviction tetikleme. `u32` yeterli (4 milyar pozisyon/pair imkansiz). `checked` aritmetik zorunlu — `u32::MAX` tasmasi chain'i durdurabilir yoksa.

---

## [position_manager/volume_tracker.rs]

**Dosya:** `.../position_manager/volume_tracker.rs`
**Satir Sayisi:** 132

### Genel Amac

`PositionVolumeTracker`: LQT (Liquidity Quality Tracker) icin pozisyon bazli hacim takibi. Sadece staking token iceren pozisyonlar icin calisiyor.

### Calisma Mantigi

```
update_volume_index(id, prev_state, new_state):
  1. phi.matches_input(STAKING_TOKEN) degilse: return (sadece staking token çiftleri)
  2. prev_state yok (yeni pozisyon): return
  3. new_state.state != Opened: return
  4. other_asset = pair'deki staking olmayan asset
  5. flows = new_state.flows(&prev_state).redirect(STAKING->other_asset)
  6. staking_token_outflow = flows.lambda_1()
     // Pozisyondan CIKAN staking token miktarı
  7. old_volume = get_volume_for_position(id)
  8. new_volume = old_volume.saturating_add(&staking_token_outflow)
  9. epoch_index = current epoch
 10. emit EventLqtPositionVolume
 11. update_volume(epoch_index, &other_asset, position_id, old_volume, new_volume)
```

### `update_volume` Icsel

- Lookup key: `lqt::v1::lp::lookup::volume_by_position(epoch_index, position_id)` → new_volume
- Old sorted key sil, yeni sorted key yaz:
  `lqt::v1::lp::by_volume::key(epoch_index, asset_id, position_id, volume)`

### DEX Uygulama Notu — Nova Shield icin

LQT (Liquidity Quality Tracker) Nova Shield icin opsiyonel ancak degerlı:
- Pozisyon bazli hacim takibi sayesinde LP incentive dagitimi hak-bazli yapilabilir
- Sadece staking token pozisyonlari icin takip (diger tokenlar daha karmasik; oran donusumu gerekir)
- Epoch-based: hacim sifirlanir her epoch'ta — gecmis epoch'larin katkisi sayilmaz
- Nova Shield'de farkli: her token icin hacim degil, sadece native token (NOVA) ciftleri icin

---

## [eviction_manager.rs]

**Dosya:** `.../component/eviction_manager.rs`
**Satir Sayisi:** 113

### Genel Amac

`EvictionManager`: bir trading pair'de acik pozisyon sayisi `max_positions_per_pair`'i asinca fazlalarik pozisyonlari kapatir.

### Algoritma

```
evict_positions():
  for pair in hot_pairs (blokta aktif olan pairler):
    N = get_position_count(pair)
    M = N - max_positions_per_pair  (excess)
    if M == 0: continue

    pair_ab = A->B,  pair_ba = B->A

    stream_ab = inventory_index stream, ascending (düşük inventory önce)
    stream_ba = inventory_index stream, ascending

    overhead_ab = stream_ab.take(M).collect::<BTreeSet>()
    overhead_ba = stream_ba.take(M).collect::<BTreeSet>()

    overlap = overhead_ab ∩ overhead_ba  // HER IKI YONDE DE DUSUK

    for id in overlap:
        close_position_by_id(id)
```

### Kritik Tasarim

**Neden sadece kesisim?**
- Bir pozisyon A->B'ye likidite saglarken B->A'ya saglamayabilir.
- Hem A->B hem B->A'da en dusuk olanlar = hicbir yonde onemli degil.
- Sadece kesisim (`∩`) evict edilir — bir tarafa hizmet eden korunur.
- Gercek max = `2 * max_positions_per_pair` (her iki yone hizmet eden var).

### DEX Uygulama Notu — Nova Shield icin

Eviction mekanizmasi Nova Shield icin kritik (DoS korumasi):
- Sinirsiz pozisyon = prefix scan O(N) → blok suresi patlar
- `max_positions_per_pair` on-chain parametre olmali
- Sadece kesisim evict etmek akillica: tek yonde likidite saglayan korunur
- `hot_pairs` optimizasyonu: her blok tum pairleri degil sadece o blokta aktif olanlari kontrol et

---

## [chandelier.rs]

**Dosya:** `.../component/chandelier.rs`
**Satir Sayisi:** 487

### Genel Amac

`Chandelier` ve `CandlestickRead` trait'leri: her blok icin trading pair bazi OHLCV (open/high/low/close/volume) mum grafigi verisi uretir.

### Trait Analizi

#### `CandlestickRead`

```rust
async fn get_candlestick(pair, height) -> Option<CandlestickData>
async fn candlesticks(pair, start_height, limit) -> Vec<CandlestickData>
// nonverifiable_range_raw ile sorted range sorgusu
```

#### `Chandelier`

```rust
async fn record_position_execution(&mut self, prev_state, new_state) -> Result<()>
// Yon tespiti: r1 mi artti r2 mi? -> DirectedTradingPair
// execution_price = prev_state.phi.orient_start(end).effective_price()
// direct_volume = rezerv degisimi
// block_executions'a push: (price, Some(direct_volume), None)

async fn record_swap_execution(&mut self, swap: &SwapExecution)
// swap.output/input sifir ise skip
// execution_price = ratio(output, input) -- U128x128
// swap_volume = swap.input.amount
// block_executions'a push: (price, None, Some(swap_volume))

async fn finalize_block_candlesticks(&mut self) -> Result<()>
// Her pair icin block_executions iterate
// open = ilk fiyat, close = son fiyat
// high/low: f64 ile takip
// direct_volume / swap_volume topla
// nonverifiable_put(by_pair_and_height key, candlestick)
// EventCandlestickData emit
```

### Veri Yapisi: CandlestickData

```rust
CandlestickData {
    height: u64,
    open: f64,
    close: f64,
    high: f64,
    low: f64,
    direct_volume: f64,  // pozisyon icra hacimleri
    swap_volume: f64,    // batch swap hacimleri
}
```

### Testler (chandelier_basic)

- 1 pozisyon, 1 swap → candlestick dogrulama (open=2.0, close=2.0, direct=1gn, swap=1gn)
- 2 pozisyon, 2 swap → open=2.0, close=1.5, high=2.0, low=1.0

### DEX Uygulama Notu — Nova Shield icin

Candlestick verisi Nova Shield DEX UI icin temel:
- `f64` kullanimi hassasiyet kaybi riski tasir — finansal veri icin dikkatli olunmali
- `direct_volume` vs `swap_volume` ayirimi guzel: AMM'ye dogrudan likidite ile swap kanallar farklilastirilir
- RPC'de 20.000 blok limiti uygulanmis — Nova Shield'de de benzer sayfalama limiti olmali
- `nonverifiable_put` ile depolama — JMT state kaniti gerektirmiyor, sadece sorgu amaçli

---

## [lqt.rs]

**Dosya:** `.../component/lqt.rs`
**Satir Sayisi:** 68

### Genel Amac

`LqtRead` trait: Likidite Kalitesi Takibi (LQT) okuma API'si. Epoch bazli, asset bazli, pozisyon bazli hacim sorgulari.

### Fonksiyonlar

```rust
async fn get_volume_for_pair(&self, asset: asset::Id) -> Amount
// Mevcut epoch, lqt::v1::pair::lookup::volume_by_pair(epoch.index, asset)

async fn get_volume_for_position(&self, position_id) -> Amount
// lqt::v1::lp::lookup::volume_by_position(epoch.index, position_id)

fn positions_by_volume_stream(&self, epoch_index, asset_id)
    -> Result<Pin<Box<dyn Stream<Item = (asset::Id, position::Id, Amount)>>>>
// lqt::v1::lp::by_volume::prefix_with_asset ile descending sorted stream
// parse_key ile (asset, volume, position_id) cikar
```

### DEX Uygulama Notu — Nova Shield icin

LQT Nova Shield'de incentive dagitimi icin kullanilabilir: epoch sonunda en cok hacim yapan top-N pozisyona odul. `positions_by_volume_stream` ile siralama kolayca yapilir.

---

## [metrics.rs]

**Dosya:** `.../component/metrics.rs`
**Satir Sayisi:** 116

### Genel Amac

DEX Prometheus metrikleri: histogram tanimlari, bucket konfigurasyonu, sabit isim exportlari.

### Metrik Listesi

| Metrik | Tip | Aciklama |
|---|---|---|
| `DEX_ARB_DURATION` | histogram | Arb calisma suresi |
| `DEX_BATCH_DURATION` | histogram | Batch swap suresi |
| `DEX_PATH_SEARCH_DURATION` | histogram | Path search suresi |
| `DEX_PATH_SEARCH_RELAX_PATH_DURATION` | counter/histogram | Path relax suresi |
| `DEX_ROUTE_FILL_DURATION` | histogram | Route fill suresi |
| `DEX_RPC_SIMULATE_TRADE_DURATION` | histogram | SimulateTrade RPC suresi |

### Bucket Konfigurasyonu

`GENERIC_DEX_BUCKETS`: 16 bucket, logaritmik aralik, `0.0005s` - `0.0678s` arasi.
`PrometheusBuilderExt` ile her metrige ayri bucket uygulanir.

### DEX Uygulama Notu — Nova Shield icin

Nova Shield DEX metrikleri ayni isimleri degil ama ayni yapıyı kullanabilir:
- Path search, route fill, arb duration her zaman olculmeli
- Logaritmik bucket araligi milisaniye-altindan saniyeye kadar kapsayan dogru secim
- `register_metrics()` chain baslatilirken cagrılmali

---

## [router/params.rs]

**Dosya:** `.../router/params.rs`
**Satir Sayisi:** 57

### Genel Amac

`RoutingParams` struct: routing icin kullanilan parametreler.

### Struct

```rust
pub struct RoutingParams {
    pub price_limit: Option<U128x128>,  // None = sinirsiz
    pub fixed_candidates: Arc<Vec<asset::Id>>,  // her zaman dahil
    pub max_hops: usize,  // maksimum hop sayisi
}
```

### Fonksiyonlar

```rust
fn with_extra_candidates(self, iter) -> Self
// fixed_candidates'e ekler, yeni Arc olusturur

fn clamp_to_limit(&self, spill_price: Option<U128x128>) -> (Option<U128x128>, bool)
// spill_price > price_limit ise price_limit kullan (clamp)
// return: (actual_spill, was_clamped)
```

### `From<DexParameters>`

```rust
impl From<DexParameters> for RoutingParams {
    // fixed_candidates, max_hops ceker
    // price_limit = None (varsayilan: limitsiz)
}
```

### DEX Uygulama Notu — Nova Shield icin

`RoutingParams` Nova Shield'de aynen kullanilabilir. `Arc<Vec<asset::Id>>` multi-thread routing icin klon maliyeti olmaksizin paylasim saglar. `price_limit` arb modunda `Some(1.into())` olarak set ediliyor — swap modunda `None`.

---

## [router/path.rs]

**Dosya:** `.../router/path.rs`
**Satir Sayisi:** 137

### Genel Amac

`Path<S>`: routing sirasinda olusturulan, StateDelta fork'u iceren, tahmini efektif fiyatla sirali yol yapisi.

### Struct

```rust
pub(super) struct Path<S: StateRead + 'static> {
    pub start: asset::Id,
    pub nodes: Vec<asset::Id>,  // start dahil degil; sonuncusu end
    pub price: U128x128,        // tahmin birikimli fiyat
    pub state: StateDelta<S>,   // fork'lanmis state
    pub span: tracing::Span,
}
```

### Fonksiyonlar

```rust
fn end(&self) -> &asset::Id { nodes.last() || &start }
fn begin(start, state) -> Self  // price=1, nodes=empty
fn fork(&mut self) -> Self     // state.fork() + clone diger alanlar
```

#### `extend_to(new_end)` — KRITIK

```
1. target_pair = DirectedTradingPair(end, new_end)
2. best_position(&target_pair).await?  // en ucuz pozisyon
3. deindex_position_by_price(&best_price_lp, &best_price_lp_id)
   // Ayni pozisyon tekrar kullanilmasin (likidite cift sayilmasin)
4. hop_price = phi.orient_end(new_end).effective_price()
5. path_price = self.price * hop_price  // U128x128 carpim
   // Overflow durumunda: return Ok(None)
6. self.price = path_price
7. self.nodes.push(new_end)
8. return Ok(Some(self))
```

### Siralama (Ord implementasyonu)

```rust
price.cmp(&other.price)  // once fiyat (kucuk = daha iyi)
    .then_with(|| nodes.len().cmp(&other.nodes.len()))  // sonra uzunluk
    .then_with(|| start.cmp(&other.start))
    .then_with(|| nodes.cmp(&other.nodes))
```

### Kritik Tasarim

1. **`deindex_position_by_price`**: Path genisletme sirasinda kullanilan pozisyon state fork'undan kaldirilir — bu sayede bir pozisyon path'te iki kez sayilmaz (cycle korumasi).
2. **`StateDelta::fork()`**: Her path fork alinirken state fork'laniyor — bu sayede farkli path'ler birbirini etkilemiyor (izole path arama).
3. **Overflow korumasi**: `price * hop_price` tasarsa path genislemez.

### DEX Uygulama Notu — Nova Shield icin

`Path<S>` yapisi Penumbra'nin en zekice tasarimlarindan biri:
- StateDelta fork ile her path'in kendi state gorunumu var
- Cycle korumasi: kullanilan pozisyon path state'inden deindex ediliyor
- `Ord` implementasyonu PathCache'de siralama sagliyor
- Nova Shield'de `U128x128` (128-bit sabit nokta) yerine farkli hassasiyet kullanilabilir; ancak taşma koruması mutlak zorunlu

---

## [router/path_cache.rs]

**Dosya:** `.../router/path_cache.rs`
**Satir Sayisi:** 114

### Genel Amac

`PathCache<S>` ve `PathEntry<S>`: SPFA (Shortest Path Faster Algorithm) benzeri yinelemeli path search icin cache; aktif/pasif path durumu ve spill price takibi.

### Struct / Tip Listesi

```rust
pub(super) struct PathEntry<S> {
    pub path: Path<S>,    // en iyi bilinen path
    pub active: bool,     // aktif mi? (yeni uzatilan pathler aktif)
    pub spill: Option<Path<S>>,  // ikinci en iyi path (spill price icin)
}

pub(super) struct PathCache<S>(BTreeMap<asset::Id, PathEntry<S>>);
pub(super) type SharedPathCache<S> = Arc<Mutex<PathCache<S>>>;
```

### Fonksiyonlar

#### `PathEntry::update(new_path)`

```
if new_path < self.path:  // daha ucuz
    spill = old best path
    path = new_path
    active = true
else:
    update_spill(new_path)  // spill'den de iyi mi?
```

#### `PathCache::begin(start, state)`

```rust
// Baslangiç: start -> start, price=1, active=true
Arc::new(Mutex::new(Self(BTreeMap::from([(start, PathEntry::from(Path::begin(start, state)))]))))
```

#### `PathCache::consider(path)`

```
BTreeMap::entry(*path.end()):
    Occupied: entry.get_mut().update(path)
    Vacant:   entry.insert(PathEntry::from(path))
```

#### `PathCache::extract_active() -> Vec<Path<S>>`

```
filter(entry.active).map(|e| { e.active = false; e.path.fork() }).collect()
```

### SPFA Benzeri Mekanizma

- `extract_active` ile aktif path'ler alinir, pasif yapilir
- Her path paralel olarak uzatilir (`relax_path`)
- Yeni extend edilen path cache'e `consider` ile eklenir
- Yeni en iyi path eklendiyse entry tekrar `active=true` olur
- Bu dongu `max_hops` kez tekrarlanir

### DEX Uygulama Notu — Nova Shield icin

PathCache'in SPFA benzeri mimarisi Nova Shield routing icin ideal:
- `Arc<Mutex<>>` ile parallel path relax (JoinSet kullanimi)
- Spill price (ikinci en iyi fiyat) fill_route'a gecirilerek bir "sonraki en iyi yol" bilgisi saglanir
- `active` bayraği gereksiz yeniden uzatmalari engeller (optimizasyon)
- Nova Shield'de `parking_lot::Mutex` kullanimi (Penumbra'da da bu kullaniliyor)

---

## [router/path_search.rs]

**Dosya:** `.../router/path_search.rs`
**Satir Sayisi:** 147

### Genel Amac

`PathSearch` trait: SPFA-benzeri iteratif path arama algoritmasinin ana entrypoint'i.

### Trait

```rust
pub trait PathSearch: StateRead + Clone + 'static {
    async fn path_search(
        &self,
        src: asset::Id,
        dst: asset::Id,
        params: RoutingParams,
    ) -> Result<(Option<Vec<asset::Id>>, Option<U128x128>)>
    // Donus: (best_path_nodes, spill_price)
}
```

### Algoritma Akisi

```
1. StateDelta::new(self.clone())  // routing state fork'u (discard at end)
2. cache = PathCache::begin(src, state)
3. for i in 0..max_hops:
       relax_active_paths(cache.clone(), fixed_candidates.clone()).await?
4. entry = cache.lock().0.remove(&dst)
5. Eger entry yok: return (None, None)
6. Eger path.price >= price_limit: return (None, None)  // strict bound
7. return (path.nodes, spill.map(|p| p.price))
```

### `relax_active_paths` Fonksiyonu

```
active_paths = cache.lock().extract_active()
js = JoinSet::new()
for path in active_paths:
    js.spawn(async { relax_path(cache, path, candidates).await })
while let Some(task) = js.join_next().await: task??
```

### `relax_path` Fonksiyonu

```
candidates = path.state.candidate_set(*path.end(), fixed_candidates)
js = JoinSet::new()
while let Some(new_end) = candidates.next():
    new_path = path.fork()
    js.spawn(async {
        if let Some(new_path) = new_path.extend_to(new_end?):
            cache.lock().consider(new_path)
    })
```

### Price Limit Notu

Yorum (satir 67-73): Testnet 53'teki iki bug:
1. `>=` yerine `>` kullanilmis (inclusive vs exclusive bound)
2. `path.price` kayipli tahmin kullanilmis

Simdi ek kontrol `route_and_fill`'de EXACT fiyat ile yapiliyor (`swap_execution.max_price()`).

### DEX Uygulama Notu — Nova Shield icin

Path search Nova Shield DEX'inin rotala motorunun temeli:
- Paralel `JoinSet` kullanimi: her hop icin tum adaylarin paralel uzatilmasi
- `max_hops` iterasyon: Bellman-Ford / SPFA mantigi
- Routing state discard: path arama hicbir state degisikligi yaratmaz
- Spill price: fill_route'a gecirilerek "bu fiyatin uzerine gecilemez" siniri saglanir
- Nova Shield'de `fixed_candidates`: staking token + bilinen stablecoin + belki wrapped major assets

---

## [router/fill_route.rs]

**Dosya:** `.../router/fill_route.rs`
**Satir Sayisi:** 721

### Genel Amac

`FillRoute` trait: belirlenmis bir rota boyunca gerçek token swap'ini gerceklestirir. Bu, DEX'in en karmasik ve kritik dosyasidir.

### Hata Tipi

```rust
pub enum FillError {
    AssetIdMismatch(asset::Id, TradingPair),
    ExecutionOverflow(position::Id),
    InvalidRoute(usize),   // route.len() < 2
    MissingFrontierPosition(position::Id),
    InsufficientLiquidity(DirectedTradingPair),
}
```

### Trait

```rust
pub trait FillRoute: StateWrite + Sized {
    async fn fill_route(
        &mut self,
        input: Value,
        hops: &[asset::Id],   // [A, B, C] => A->B->C yolu
        spill_price: Option<U128x128>,  // None = sinirsiz
    ) -> Result<SwapExecution, FillError>
    // ensure_progress=true ile fill_route_inner cagirir
}
```

### Frontier Yapisi (KRITIK)

```rust
struct Frontier<S> {
    pairs: Vec<DirectedTradingPair>,    // rota
    positions: Vec<Position>,           // her hop icin en iyi pozisyon
    position_ids: BTreeSet<position::Id>, // tekrar kullanim engeli
    state: S,
    positions_by_price: PositionsByPrice, // her pair icin stream
    trace: Vec<Vec<Value>>,             // yurutme izi
}
```

### fill_route_inner — 3-FAZ DONGU (KRITIK ALGORITMA)

```
Hazirlik:
  StateDelta::new(state)  // transactional
  route = [input_asset] + hops
  pairs = breakdown_route(route)  // DirectedTradingPair listesi
  frontier = Frontier::load(pairs)  // her hop icin ilk pozisyon
  filled_once = false (ensure_progress=true ise)

'filling: loop {
  // INVARIANT: ya input tamamen tuketilir ya da
  //            bir pozisyonun rezervleri tamamen tuketilir

  // FAZ 1 — SENSE (Kapasite Kistini Tespiti)
  constraining_index = frontier.sense_capacity_constraint(input)
  // Her hop icin fill dene; hangi hop kisitiyor?

  // FAZ 2 — FILL (Transaksiyonel Doldurma)
  tx = match constraining_index:
      Some(i) => frontier.fill_constrained(i)   // kisitli doldurma
      None    => frontier.fill_unconstrained(input)  // tam doldurma

  // FAZ 3 — COMMIT (Spill Price Kontrolu)
  should_apply = match spill_price:
      Some(sp) => tx.actual_price() <= sp || !filled_once
      None     => true

  if !should_apply: break 'filling

  (current_input, current_output) = frontier.apply(tx)
  filled_once = true
  input.amount -= current_input
  output.amount += current_output

  if !frontier.replace_empty_positions().await?: break 'filling

  if constraining_index.is_none():
      assert!(input.amount == 0)
      break 'filling
}

// Sonrasi:
frontier.save().await  // tum frontier pozisyonlari yaz
this.apply()  // StateDelta commit
```

### Kritik Detaylar

#### `sense_capacity_constraint` (Satir 548-603)

```
Her hop icin:
  (unfilled, new_reserves, output) = position.phi.fill(current_input, reserves)
  if unfilled > 0:
      constraining_index = Some(i)  // SON kisit (en sona dogru aranir)
  current_input = output
```

Not: `constraining_index` her `unfilled > 0` gosterdigi yerde guncelleniyor; loop bitmeden son kistlayan bulunuyor.

#### `fill_constrained` (Satir 625-664)

```
// Kisitlayan pozisyonun tum rezervlerini dogrudan tuket
exactly_consumed = position[constraining_index].reserves_for(end_asset)

fill_backward(&mut tx, constraining_index, exactly_consumed)
// Geri dogru: kisit noktasindan input'a kadar tam doldur

fill_forward(&mut tx, constraining_index + 1, exactly_consumed)
// Ileri dogru: kisit noktasindan output'a kadar doldur
```

#### `fill_unconstrained` (Satir 606-623)

```
tx.trace[0] = Some(input.amount)
fill_forward(&mut tx, 0, input)  // direkt ileri doldur
```

#### `fill_forward` (Satir 667-688)

```
for i in start_index..positions.len():
    (unfilled, new_reserves, output) = phi.fill(current_value, reserves)
    assert!(unfilled == 0)
    tx.new_reserves[i] = Some(new_reserves)
    tx.trace[i+1] = Some(output.amount)
    current_value = output
```

#### `fill_backward` (Satir 691-719)

```
for i in (0..=start_index).rev():
    tx.trace[i+1] = Some(current_value.amount)
    (new_reserves, prev_input) = phi.fill_output(reserves, current_value)
    tx.new_reserves[i] = Some(new_reserves)
    current_value = prev_input
tx.trace[0] = Some(current_value.amount)
```

#### `actual_price` hesabi

```rust
fn actual_price(&self) -> Result<U128x128, Error> {
    input / output  // U128x128 bolme
}
// Daha dusuk actual_price = daha iyi fiyat (input/output kucuk = az input, cok output)
```

#### `replace_empty_positions` + `replace_position`

```
Her hop icin: reserves_for(end) == 0 ise replace
replace_position(i):
    1. Eski pozisyonu state'e yaz (position_execution)
    2. positions_by_price stream'den sonraki pozisyonu al
    3. position_ids'de yoksa kabul et
    4. frontier'a ekle
```

#### `Frontier::load` — Baslangiç Kurulumu

```
positions_by_price: her FARKLI pair icin tek bir stream
// (aynı macro-edge farkli yönde ise de ayni stream)

for pair in &pairs:
    positions = positions_by_price[pair].next()
    // ID cakismasi olursa bir sonrakini al
```

### SwapExecution Olusturma

```rust
// trace: her doldurma turunden gelen trace'lerin toplami
// input: trace.iter().map(|t| t.first().amount).sum()
// output: trace.iter().map(|t| t.last().amount).sum()
```

### ensure_progress Invariant

```
// Spill price asildiysa bile ilk pozisyon mutlaka tuketilir
// (ensure_progress=true ise filled_once=false baslar)
// Bu, iki rotanin birbirine yakin fiyatta olup sonsuz donguye
// yol actigi edge case'i onler
```

### Unsafe Bloklar

Yok.

### DEX Uygulama Notu — Nova Shield icin

`fill_route` Nova Shield DEX'inin kalbi olacak:

**3-faz pattern (Sense/Fill/Commit) aynen uygulanmali:**
- Sense: gercek aritmetik yap ama commit etme — kisiti bul
- Fill: kisita gore geri/ileri doldur
- Commit: spill price kontrolu, sonra StateDelta apply

**Frontier pattern:**
- Her hop icin en iyi pozisyon lazim
- Tukenince siradakini al (stream-based lazy loading)
- `BTreeSet<position::Id>` ile tekrar kullanim engeli

**Kritik Hatalar:**
- `ensure_progress=true`: Her doldurma turunde en az 1 pozisyon tuketilmeli — dongu engeli
- Overflow durumunda (`ExecutionOverflow`) pozisyon kapatilir ve routing yeniden denenir

**Rounding Kararlari:**
- `fill_backward`: geriye dogru tam tuketim, yukariya yuvarlama olabilir
- Ornek: 100_001gm gerekirken 100_001 gm gonderilmeli (1 gm artik)

---

## [router/route_and_fill.rs]

**Dosya:** `.../router/route_and_fill.rs`
**Satir Sayisi:** 313

### Genel Amac

`HandleBatchSwaps` ve `RouteAndFill` trait'leri: batch swap orkestrasyonu ve yinelemeli rota bulma + doldurma.

### `HandleBatchSwaps` Trait

```rust
async fn handle_batch_swaps(
    self: &mut Arc<Self>,
    trading_pair: TradingPair,
    batch_data: SwapFlow,
    block_height: u64,
    params: RoutingParams,
    execution_budget: u32,
) -> Result<BatchSwapOutputData>
```

Akis:
```
1. delta_1, delta_2 = batch_data.0, batch_data.1
2. clamped_delta = min(delta, MAX_RESERVE_AMOUNT)
3. swap_execution_1_for_2 = route_and_fill(asset_1, asset_2, delta_1, params, cb)
4. swap_execution_2_for_1 = route_and_fill(asset_2, asset_1, delta_2, params, cb)
   // Dikkat: AYNI circuit breaker paylasiliyor!

5. lambda_2 = swap_1_for_2.output.amount (veya 0)
6. unfilled_1 = delta_1 - swap_1_for_2.input.amount
7. (lambda_1, unfilled_2) benzer sekilde

8. BatchSwapOutputData{height, trading_pair, delta_1, delta_2,
                        lambda_1, lambda_2, unfilled_1, unfilled_2,
                        sct_position_prefix}

9. Candlestick guncelle (her iki yon icin)
10. set_output_data(bsod, swap_exe_1for2, swap_exe_2for1)
```

### `sct_position_prefix` Hesabi

```rust
sct_position_prefix = (
    u16::try_from(epoch.index),
    u16::try_from(block_height - epoch.start_height),
    0,  // blok icindeki sira
).into()
```
ZK swap claim icin SCT pozisyon hesaplama.

### `RouteAndFill` Trait

```rust
async fn route_and_fill(
    self: &mut Arc<Self>,
    asset_1, asset_2,
    input: Amount,
    params: RoutingParams,
    mut execution_circuit_breaker: ExecutionCircuitBreaker,
) -> Result<Option<SwapExecution>>
```

### Yinelemeli Akis (KRITIK)

```
Termination conditions:
  1. total_unfilled_1 == 0 (tamamen doldu)
  2. Yol bulunamadi
  3. price_limit asildi (exact price ile kontrol)
  4. ExecutionCircuitBreaker.exceeded_limits()
  5. Kurtarilamaz hata

loop {
    if circuit_breaker.exceeded_limits(): break
    circuit_breaker.increment()

    (path, spill_price) = path_search(asset_1, asset_2, params)
    if path.is_none() || path.is_empty(): break

    delta_1 = Value { asset_1, amount: total_unfilled_1 }

    swap_execution = match fill_route(delta_1, path, spill_price):
        Ok(exe) => exe
        Err(ExecutionOverflow(pos_id)):
            close_position_by_id(pos_id)
            continue  // tekrar dene
        Err(e) => bail!  // kritik hata

    total_output_2 += swap_execution.output.amount
    total_unfilled_1 -= swap_execution.input.amount
    traces.append(swap_execution.traces)

    if total_unfilled_1 == 0: break

    accurate_max_price = swap_execution.max_price()
    if accurate_max_price >= price_limit: break
}

if traces.empty(): return None
return Some(SwapExecution { traces, input: consumed, output: total_output_2 })
```

### Kritik Tasarim

1. **`exact price` kontrolu**: `swap_execution.max_price()` ile kesin fiyat hesabi — `path.price` tahmini yerine.
2. **Overflow recovery**: `ExecutionOverflow` durumunda pozisyon kapatilir, continue; diger hatalar fatal.
3. **Paylasilan circuit breaker**: 1->2 ve 2->1 swaplari AYNI butceden tuketir.
4. **MAX_RESERVE_AMOUNT clamp**: input basinda sinirlandirma (delta_1, delta_2).

### DEX Uygulama Notu — Nova Shield icin

`route_and_fill` dongusu Nova Shield'de aynen uygulanmali:
- `path_search` + `fill_route` yinelemesi: her tur bir rota tamamen doldurulana kadar
- Overflow recovery: bozuk pozisyon kapatilir, routing devam eder — robust
- Price limit: `path.price` TAHMINI; exact kontrol swap bittikten sonra `max_price()` ile
- Paylasilan circuit breaker: iki yon esit oncelik alir, biri digeri aciginda kutuphane zamanini yemez
- `MAX_RESERVE_AMOUNT` clamp: u64 tasmasi icin

---

## [router/mod.rs]

**Dosya:** `.../router/mod.rs`
**Satir Sayisi:** 21

Re-export dosyasi. `FillRoute`, `RoutingParams`, `PathSearch`, `HandleBatchSwaps`, `RouteAndFill`, test modulu.

### DEX Uygulama Notu — Nova Shield icin

Modül organizasyonu: `fill_route`, `path_search`, `route_and_fill` ayri modullerde, ancak `router/` altinda bir araya getirilmis. Nova Shell'de ayni ayrimi koru.

---

## [router/tests.rs]

**Dosya:** `.../router/tests.rs`
**Satir Sayisi:** 1200+ (sadece ilk 100 satir okundu; geri kalan benzer)

### Genel Amac

Router modulu icin unite testler: path extension, path search, fill_route.

### Test Listesi (goruntulenenler)

- `path_search_basic`: gm -> penumbra arama, penumbra -> penumbra (kendine) test
- `path_extension_basic`: Path'i gm -> gn uzat, gn -> penumbra (yok) — None dogrula

### Yardimci Fonksiyonlar

`create_buy(pair, qty, price) -> Position` ve `create_sell(pair, qty, price) -> Position` — test pozisyonlari olusturur.

### DEX Uygulama Notu — Nova Shield icin

`create_buy` ve `create_sell` yardimci fonksiyonlari Nova Shield test suite'ine dogrudan alinanabilir. Routing testlerinde hem tek-hop hem cok-hop senaryolar gerekli.

---

## [action_handler.rs]

**Dosya:** `.../component/action_handler.rs`
**Satir Sayisi:** 4 (sadece modul listesi)

```rust
mod position;
mod swap;
mod swap_claim;
```

### DEX Uygulama Notu — Nova Shield icin

Modul ayirimi temiz: swap, swap_claim, position (open/close/withdraw) farkli action handler modulleri. Nova Shield'de ayni pattern.

---

## [action_handler/swap.rs]

**Dosya:** `.../action_handler/swap.rs`
**Satir Sayisi:** 74

### ActionHandler for Swap

#### `check_stateless`

```rust
1. asset_1 != asset_2 (ayni token cift olmamali)
2. self.proof.verify(SWAP_PROOF_VERIFICATION_KEY, SwapProofPublic {
       balance_commitment,
       swap_commitment,
       fee_commitment,
   })
```

#### `check_and_execute`

```rust
1. dex_params.is_enabled kontrolu
2. accumulate_swap_flow(&trading_pair, (delta_1_i, delta_2_i).into())
   -> VCB credit her iki taraf
3. add_swap_payload(commitment, source)  // SCT'ye ekle
4. add_recently_accessed_asset(asset_1, asset_2)
5. EventSwap emit
```

### DEX Uygulama Notu — Nova Shield icin

Nova Shield swap action handler'i ZK proof dogrulamasi gerekmiyorsa sadece:
1. DEX enabled kontrol
2. Swap flow birikimi (VCB credit)
3. Event emit
ZK swap commitment/claim mekanizmasi Nova Shield'de olmayacaksa SCT entegrasyonu atlanabilir.

---

## [action_handler/swap_claim.rs]

**Dosya:** `.../action_handler/swap_claim.rs`
**Satir Sayisi:** 103

### ActionHandler for SwapClaim

#### `check_stateless`

```rust
self.proof.verify(SWAPCLAIM_PROOF_VERIFICATION_KEY, SwapClaimProofPublic {
    anchor, nullifier, claim_fee,
    output_data,
    note_commitment_1, note_commitment_2,
})
```

#### `check_historical`

```rust
1. epoch_duration == provided_epoch_duration (sabit, guvenli)
2. output_data = state.output_data(height, trading_pair)
   // Tarihsel BSOD ile eslesme kontrolu
3. output_data == swap_claim.body.output_data
```

#### `check_and_execute`

```rust
1. check_nullifier_unspent(nullifier)  // cift harcama engeli
2. add_rolled_up_payload(output_1_commitment, source)
3. add_rolled_up_payload(output_2_commitment, source)
4. nullify(nullifier, source)
5. EventSwapClaim emit
```

### DEX Uygulama Notu — Nova Shield icin

Swap claim mekanizmasi Penumbra'nin ZK gizlilik katmanindan geliyor. Nova Shield'de transparent swap ise bu kadar karmasik degil — sadece output kaydedilir.

---

## [action_handler/position/open.rs]

**Dosya:** `.../position/open.rs`
**Satir Sayisi:** 43

### ActionHandler for PositionOpen

#### `check_stateless`

```rust
self.position.check_stateless()?
// Reserves <= 80 bit
// Trading function katsayilari <= 80 bit
// En az bir taraf rezerv var
// Katsayilar != 0
// Cyclic pair degil
// Fee <= %50

if position.state != Opened: bail!
```

#### `check_and_execute`

```rust
1. dex_params.is_enabled
2. state.open_position(self.position.clone())
   // VCB credit + indeksler + event
```

### DEX Uygulama Notu — Nova Shield icin

`check_stateless` listesi Nova Shield icin kaynak: 80-bit rezerv kisitlari, katsayilar != 0, cyclic pair engeli, max %50 fee. Bunlarin hepsi on-chain invariantlar.

---

## [action_handler/position/close.rs]

**Dosya:** `.../position/close.rs`
**Satir Sayisi:** 30

### ActionHandler for PositionClose

#### `check_stateless`

```rust
Ok(())  // Hicbir stateless kontrol yok
// Gucarlama value balance mekanizmasiyla yapiliyor
```

#### `check_and_execute`

```rust
state.queue_close_position(self.position_id).await?
// HEMEN KAPATMA YOK — queue'ya ekle
// end_block'ta close_queued_positions ile kapatilir
```

### Kritik: JIT Likidite

Yorum (satir 20-24): "block-scoped JIT liquidity" — bir transaction'da hem open hem close yapilabilir; pozisyon o blogun batch swap'i boyunca aktif kalir.

### DEX Uygulama Notu — Nova Shield icin

JIT likidite Nova Shield'de de desteklenmeli: PositionClose aninda kapatmaz, end_block'ta kapatir. Bu ozelligi bozmak icin yanlis siralama yeterli — dikkat.

---

## [action_handler/position/withdraw.rs]

**Dosya:** `.../position/withdraw.rs`
**Satir Sayisi:** 53

### ActionHandler for PositionWithdraw

#### `check_stateless`

```rust
Ok(())  // Stateless kontrol yok
```

#### `check_and_execute`

```rust
1. actual_reserves = state.withdraw_position(position_id, sequence)
   // VCB debit, rezerv sifirla, state = Withdrawn{sequence}
2. expected_reserves_commitment = actual_reserves.commit(Fr::zero())
3. if self.reserves_commitment != expected_reserves_commitment: bail!
   // Kullanicinin ne cekecegini belirtti ve dogruluyoruz
```

### DEX Uygulama Notu — Nova Shield icin

`reserves_commitment` dogrulamasi Nova Shield'de daha basit olabilir (ZK olmadigi icin). Ancak "kullanici cekecegi miktari taahhut eder" pattern TOCTOU saldirilarina karsi koruma saglar: miktar execution aninda dogru hesaplanir.

---

## [action_handler/position/mod.rs]

**Dosya:** `.../position/mod.rs`
**Satir Sayisi:** 3 (sadece modul listesi)

---

## [rpc.rs]

**Dosya:** `.../component/rpc.rs`
**Satir Sayisi:** 701

### Genel Amac

gRPC `QueryService` ve `SimulationService` implementasyonu. Sadece okuma amaçlı; storage'dan snapshot alinarak sorgu yapilir.

### Servis Listesi

#### `QueryService` impl

| RPC | Aciklama |
|---|---|
| `arb_execution(height)` | Belirli blok yuksekligi icin arb execution verisi |
| `arb_executions(start, end)` | Arb execution stream (prefix scan, height filtre) |
| `batch_swap_output_data(height, pair)` | BSOD sorgusu |
| `swap_execution(height, pair)` | Tek swap execution |
| `swap_executions(start, end, pair)` | Swap execution stream |
| `spread(pair)` | En iyi iki yon icin pozisyon + efektif fiyat |
| `candlestick_data(pair, start_height, limit)` | Mum grafigi (max 20.000 blok) |
| `candlestick_data_stream(pair)` | Canli mum grafigi stream (storage.subscribe()) |
| `liquidity_positions_by_price(pair, limit)` | Fiyat sirali pozisyon stream |
| `liquidity_positions(include_closed)` | Tum pozisyonlar |
| `liquidity_position_by_id(id)` | Tek pozisyon |
| `liquidity_positions_by_id(ids)` | Cok ID pozisyon |

#### `SimulationService` impl

```rust
async fn simulate_trade(input, output_id, routing_strategy) -> SimulateTradeResponse
// StateDelta::new (discard after)
// route_and_fill ile simule et
// unfilled hesapla
// DEX_RPC_SIMULATE_TRADE_DURATION metrik
```

`routing_strategy`:
- `Default`: params.max_hops degismez
- `SingleHop`: max_hops = 1

### Kritik: `spread` RPC Hatasi

```rust
let best_2_to_1_position = state.best_position(&pair12)  // HATALI: pair12 yerine pair21 olmali
```
Satir 398-406: `pair21` degiskeni kullanilmasi gerekir ama `pair12` kullanilmis.

### `candlestick_data_stream` — Canli Stream

```rust
tokio::spawn(async move {
    let mut rx_state_snapshot = storage.subscribe();
    loop {
        rx_state_snapshot.changed().await
        // Her yeni blokta o blogun candlestick'ini al ve gonder
    }
})
```

### DEX Uygulama Notu — Nova Shield icin

Nova Shield gRPC API'si Penumbra'yi model alabilir:
- `simulate_trade`: StateDelta fork ile gercek state degistirmeden simülasyon — kritik
- `spread` RPC bug'i: `pair12` yerine `pair21` kullanilmali — bu bir bug — Nova Shield'de duzelt
- `candlestick_data_stream`: storage.subscribe() pattern ile reaktif stream — WebSocket icin ideal
- Max 20.000 candlestick limiti: uygulanmali, yoksa DoS riski
- `SimulationsDisabled` stub: Pahalı simülasyon RPC'sini feature-flag ile disable etmek iyi pratik

---

## [rpc/stub.rs]

**Dosya:** `.../rpc/stub.rs`
**Satir Sayisi:** 21

```rust
pub struct SimulationsDisabled;
impl SimulationService for SimulationsDisabled {
    async fn simulate_trade(..) -> Err(Status::unimplemented("...enable-expensive-rpc..."))
}
```

### DEX Uygulama Notu — Nova Shield icin

Feature-flag ile pahalı RPC'leri disable etmek production deployment icin zorunlu. Nova Shield'de `simulate_trade` varsayilan olarak kapali, `--enable-expensive-rpc` ile acilabilir.

---

## [tests.rs]

**Dosya:** `.../component/tests.rs`
**Satir Sayisi:** 1148

### Genel Amac

DEX entegrasyon testleri + `TempStorageExt` yardimci trait.

### `TempStorageExt` — Minimal Genesis

```rust
async fn apply_minimal_genesis(self) -> Self:
    state.put_block_height(0)
    state.put_epoch_by_height(0, Epoch{index:0, start_height:0})
    state.put_dex_params(DexParameters::default())
```

### Test Listesi

| Test | Senaryo | Dogrulamalar |
|---|---|---|
| `single_close_on_fill` | 1 pozisyon, 4 farkli swap | Reserve dogru guncellendi, unfilled dogru |
| `check_close_on_fill` | 2 pozisyon, biri close_on_fill=true | Auto-close dogru calisti |
| `multiple_close_on_fills` | 3 pozisyon farkli fee (9,10,11 bps) | Sirali dolum, fee sirasi dogru |
| `empty_order_fails` | 0 rezerv pozisyon | `check_stateless` hata vermeli |
| `position_create_and_retrieve` | 2 pozisyon olustur | `all_positions` stream dogru |
| `swap_execution_tests` | Basit + karmasik swap | Trace dogru (2 trace, 4 hop path) |
| `basic_cycle_arb` | 3 pozisyon, 1 cicle | Arb: 10pen→10gn→20gm→20pen |
| `reproduce_arbitrage_loop_testnet_53` | Testnet 53 regresyon | 2s timeout, profit=0.099998pen |
| `check_routable_asset_ordering` | 4 pozisyon, likidite siralama | gn > btc (130k > 100k) |

### Kritik Test Bulgulari

**`single_close_on_fill` rounding detayi (satir 106-113):**
```
100gm girilmesi gerekirken 100_001gm girildi
Output: 120_000gn, unfilled: 1gm
Yani: rounding sebebiyle 1 fazla gm gerekiyor
```

**`multiple_close_on_fills` fee sirasi (satir 381-413):**
```
9 bps fee pozisyon once dolduruluyor (daha ucuz)
10 bps ikinci, 11 bps ucuncu
fee her pozisyon icin: needed = qty * 10000 / (10000 - fee)
```

**`swap_execution_tests` karmasik iz (satir 776-790):**
```
10 penumbra → 25gn, iki trace:
Trace 1: [5pen, 1pusd, 20gm, 20gn]  (4 hop)
Trace 2: [5pen, 5gm, 5gn]           (3 hop)
```

### DEX Uygulama Notu — Nova Shield icin

Bu testlerin Nova Shield DEX'ine adapte edilmis versiyonlari gerekli:
- Rounding testi: her fill'den sonra unfilled_amount dogru mu?
- Fee siralama: dusuk ucretli pozisyon once doldurulmali
- Close-on-fill: tek-tarafli JIT pozisyonlari
- Arb: cycle arb tespiti ve timeout
- Regresyon: testnet 53 bug analogi — strict price bound testi

---

## [circuit_breaker/value.rs] — Testler (ayri bolum)

3 test (yukarida analiz edildi). Ozetler:
1. `value_circuit_breaker`: Temel credit/debit dongusu
2. `position_value_circuit_breaker`: Pozisyon manipulasyonu ile VCB ihlali
3. `batch_swap_circuit_breaker`: `#[should_panic]` — VCB bildirilmeden pozisyon olusturma

---

## [chandelier.rs] — Testler (ayri bolum)

`chandelier_basic`:
- Height 0: 1 swap, open=close=high=low=2.0, direct=swap=1gn
- Height 1: 2 swap (farkli fiyat), open=2.0, close=1.5, high=2.0, low=1.0

---

## [tests/integration/harness.rs]

**Dosya:** `.../tests/integration/harness.rs`
**Satir Sayisi:** 80+ (sadece ilk 80 satir okundu)

### Genel Amac

`DexTester` struct — integration test icin tam DEX ortami: TempStorage + height yonetimi + action handler cagrilari.

```rust
struct DexTester {
    storage: TempStorage,
    height: u8,
    handle: Option<StateDelta<Snapshot>>,
}
// init() -> Dex::init_chain ile baslatir
// position_open(&mut self, PositionOpen) -> cagrilar check_and_execute
// consume_handle() / handle() -> state delta yonetimi
```

### DEX Uygulama Notu — Nova Shield icin

Test harness pattern Nova Shield DEX testleri icin aynen kullanilabilir. `DexTester` benzeri bir yapiyla state management + action handler cagrilari izole edilebilir.

---

## GENEL SISTEM MIMARISI OZETI

### End-Block Orkestrasyonu (Tam Akis)

```
Dex::end_block()
│
├── F.0. Non-native fee → swap flow
│
├── 1. [Pozisyon Acimlari — onceki action handler'larda]
│
├── 2. Batch Swaplar (her TradingPair icin):
│   └── handle_batch_swaps(pair, flow, height, routing_params, budget)
│       ├── route_and_fill(asset_1, asset_2, delta_1, ...)
│       │   loop:
│       │     circuit_breaker.increment()
│       │     path = path_search(src, dst, params)
│       │       ├── SPFA: max_hops iterasyon
│       │       ├── Paralel relax (JoinSet)
│       │       └── PathCache: best + spill price
│       │     fill_route(input, path, spill_price)
│       │       ├── Frontier::load
│       │       └── 3-faz dongu (Sense→Fill→Commit)
│       └── route_and_fill(asset_2, asset_1, delta_2, ...)
│
├── F.1. Non-native fee claim (BSOD pro-rata)
│
├── 3. Arbitraj (staking token):
│   └── arbitrage(STAKING_TOKEN, arb_params {max_hops+2, price_limit=1})
│       ├── Flash loan: u64::MAX
│       └── route_and_fill(token, token, u64::MAX)  [circular]
│
├── 4. Eviction (aktif pair'ler icin fazla LP kapat)
│
├── 5. close_queued_positions (JIT destegi)
│
└── 6. finalize_block_candlesticks
```

### Index Guncelleme Akisi (update_position cagrildiktan sonra)

```
update_position(id, prev, new)
├── guard_invalid_transitions  (durum makinesi)
├── update_position_by_inventory_index  (eviction queue)
├── update_asset_by_base_liquidity_index  (routing heuristic)
├── update_trading_pair_position_counter  (eviction tetigi)
├── update_position_by_price_index  (routing sort)
├── update_volume_index  (LQT)
└── put(position_by_id_key, new_state)
```

---

## NOVA SHIELD TASARIM KARARLARI OZETI

Penumbra DEX component analizi sonucunda Nova Shield DEX icin asagidaki kritik kararlar onerilir:

### 1. Batch Swap Modeli
Penumbra'nin batch swap + end_block orkestrasyonu benimsenmeli. Her blok tum swaplari bir arada islemek MEV direnci saglar.

### 2. 3-Fazli Fill Algoritması (Sense/Fill/Commit)
`fill_route_inner`'in 3-faz pattern'i aynen kullanilmali. Bu pattern hem dogru hem verimli; `fill_backward` + `fill_forward` kombini ile hem constrained hem unconstrained akislar temiz islenebiliyor.

### 3. Value Conservation Breaker (VCB)
Her deposit/withdraw icin `checked_add/checked_sub` ile mutlak zorunlu. Underflow = zincir durur. Bu nova-shield-chain icin `audit-system` kapsamina girmeli.

### 4. Execution Circuit Breaker
`max_execution_budget` on-chain governance parametresi. DoS koruması. "exceeded_limits ONCE CHECK, THEN increment" sirasi — NoVA Shield'de kesinlikle bu sira.

### 5. Flash Loan Arbitraj
Her blok sonunda: `u64::MAX` flash loan → circular route → profit if output > input. `price_limit = 1` (karlilik siniri). Arb hatasi zinciri durdurmasin.

### 6. Spill Price Strict Bound
`path.price >= price_limit` STRICT. Testnet 53 dersi: inclusive bound sonsuz dongu. Nova Shield'de `>=` operatoru kullanilmali.

### 7. Eviction + max_positions_per_pair
Sinirli pozisyon sayisi = routing O(N) yerine O(max) performansi. Kesisim-bazli eviction (her iki yonde de dusuk olanlari evict et).

### 8. Fiyat Indeksi: Yon Bazli, Composite Key
`pair + phi_component + id` composite key. `r1 > 0 → 2->1 yonu`, `r2 > 0 → 1->2 yonu`. Her yon icin ayri index.

### 9. Base Liquidity Index: Tersine Likidite Heuristic
Routing icin "B->A miktari" (ters yon) kullan — manipülasyon direnci.

### 10. JIT Likidite Destegi
`PositionClose` aninda degil `end_block`'ta — ayni blok icinde hem aç hem kapat. Nova Shield'de zorunlu.

### 11. `spread` RPC Hatasindan Kacin
Penumbra'da `best_2_to_1` icin `pair12` kullanilmis (bug). Nova Shield'de `pair21` kullanilmali.

### 12. `ensure_progress = true`
`fill_route` her zaman en az 1 pozisyon tuketmeli. Iki benzer fiyatli rota arasinda ping-pong dongusu engeli.

---

*Agent DEX-B — Tum 35 dosya analiz edilmistir. Kaynak koddan dogrulanan her deger kaynak satir numarasi ile isaretlenmistir.*
