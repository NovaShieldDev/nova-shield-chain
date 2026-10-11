# Penumbra DEX Modülü — Agent-11 (Çift Agent: İkinci Perspektif)
## Routing Motoru, Path Search, Fill Route, Orchestration ve Flash Loan Arbitraj

**Kapsanan Dosyalar:** DEX router, batch swap, arbitrage, circuit breakers, params
**Odak:** Routing algoritmaları, Batch Swap akışı, DEX parametreleri, value conservation
**Hassasiyet:** 3x (DEX modülü — çift agent protokolü)
**Not:** Agent-04 LP çekirdek modelini kapsamıştır. Bu agent routing ve orchestration perspektifini ele alır.

---

## 1. `src/params.rs` — DEX Konfigürasyonu (77 satır)

### Struct: `DexParameters`
```rust
pub struct DexParameters {
    pub is_enabled: bool,
    pub fixed_candidates: Vec<asset::Id>,
    pub max_hops: u32,                  // default: 4
    pub max_positions_per_pair: u32,    // default: 1_000
    pub max_execution_budget: u32,      // default: 64
}
```

**Doğrulanmış default değerler (satır 62-76):**
- `max_hops = 4` → 5 asset chain (A→B→C→D→E)
- `max_positions_per_pair = 1_000` → eviction threshold
- `max_execution_budget = 64` → circuit breaker limit
- fixed_candidates: STAKING_TOKEN + 6 test asset

**Nova Shield Dersi:** Genesis parametreleri governance üzerinden değiştirilebilir. `fixed_candidates` routing stabilitesini garantiler — saldırganlar yeni bir "decoy asset" oluşturarak routingi manipüle edemez çünkü fixed set daima taranır.

---

## 2. `src/component/router/params.rs` — Routing Parametreleri (57 satır)

### Struct: `RoutingParams`
```rust
pub struct RoutingParams {
    pub price_limit: Option<U128x128>,
    pub fixed_candidates: Arc<Vec<asset::Id>>,
    pub max_hops: usize,
}
```

### `clamp_to_limit(spill_price)` (satır 27-41):
4 durum:
- (Some(spill), Some(limit)) → `if spill > limit: (limit, true) else (spill, false)`
- (Some(spill), None) → `(spill, false)`
- (None, Some(limit)) → `(limit, true)` — clamped!
- (None, None) → `(None, false)`

**Yorum:** Spill price, route_and_fill döngüsünün nerede durması gerektiğini belirler. Price limit bunu üstten kırpar.

---

## 3. `src/component/router/path.rs` — Yol (137 satır)

### Struct: `Path<S>`
```rust
pub(super) struct Path<S: StateRead + 'static> {
    pub start: asset::Id,
    pub nodes: Vec<asset::Id>,  // start hariç
    pub price: U128x128,         // birikimli effective price
    pub state: StateDelta<S>,    // izole state fork
    pub span: tracing::Span,     // debug
}
```

### Temel işlem: `extend_to(new_end)` (satır 61-109)

```
target_pair = (end, new_end)
best_pos = state.best_position(target_pair)   // None → Ok(None)
state.deindex_position(best_pos)              // bu fork'ta gizle
hop_price = best_pos.orient_end(new_end).effective_price()
path_price = self.price * hop_price           // taşma → Ok(None)
self.price = path_price
self.nodes.push(new_end)
```

**Ordering:** `price < length < start < nodes` — daha ucuz yol her zaman daha iyi.

**`fork()`:** `StateDelta::fork()` gerçek bir copy — her path kendi izole state'i ile çalışır. Aynı pozisyon farklı path fork'larında görünebilir ama aynı path'da tekrar kullanılamaz (deindex).

---

## 4. `src/component/router/path_cache.rs` — SPFA Cache (113 satır)

### Struct: `PathEntry<S>`
```rust
pub(super) struct PathEntry<S: StateRead + 'static> {
    pub path: Path<S>,
    pub active: bool,           // son iterasyonda iyileşti mi?
    pub spill: Option<Path<S>>, // 2. en iyi yol
}
```

### Güncelleme mantığı: `consider(new_path)` (satır 83-97)
- `new_path < current.path` → `spill = old_path; path = new_path; active = true`
- `new_path >= current.path && new_path < spill` → `spill = new_path`
- `new_path >= spill` → yoksay

### SPFA optimizasyonu: `extract_active()` (satır 100-112)
```rust
.filter_map(|(_, entry)| {
    if entry.active {
        entry.active = false;
        Some(entry.path.fork())  // fork edilmiş kopya
    } else {
        None
    }
})
```

Bu Bellman-Ford'un SPFA varyantı: Sadece güncellenen düğümleri tekrar genişlet.

---

## 5. `src/component/router/path_search.rs` — Algoritma (146 satır)

### Ana fonksiyon: `path_search(src, dst, params)`

```
state = StateDelta::new(self.clone())
cache = PathCache::begin(src, state)     // src→src: price=1, active=true

for _ in 0..max_hops:
    active = cache.lock().extract_active()
    JoinSet {
        for path in active:
            spawn(relax_path(cache, path, candidates))
    }.join_all()

cache.0.remove(&dst)  // en iyi yol + spill price
```

**`relax_path` iç mantığı:**
```
candidates = candidate_set(path.end, fixed_candidates)
// candidate_set: fixed (daima) + ordered_routable (max 10)

JoinSet {
    for new_end in candidates:
        spawn(async {
            if let Some(new_path) = path.fork().extend_to(new_end).await? {
                cache.lock().consider(new_path)
            }
        })
}
```

**Paralellik modeli:**
- Outer JoinSet: tüm aktif path'lar paralel
- Inner JoinSet: her path'ın tüm komşuları paralel
- Toplam eşzamanlılık: `active_paths × candidates` tokio goroutine

**Karmaşıklık:**
- max_hops=4 iterasyon
- Her iterasyon: aktif × (7 fixed + 10 dinamik) = aktif × 17
- Worst case: O(17^4) ≈ 83K path genişletme (paralel)

---

## 6. `src/component/router/fill_route.rs` — Kritik Fill Algoritması (721 satır)

### Ana fonksiyon: `fill_route_inner()`

**Initialization:**
```rust
let mut this = StateDelta::new(state);  // transactional
let pairs = breakdown_route(&route)?;   // [(A,B), (B,C), ...]
let frontier = Frontier::load(&mut this, pairs).await?;
// Her pair için: en iyi fiyatlı pozisyonu stream'den al
```

**3 Faz Ana Döngü:**

**Faz 1 — Sensing:**
```rust
let constraining_index = frontier.sense_capacity_constraint(input)?;
```
Trial fill her hop'ta: `position.phi.fill(current, reserves)`. `unfilled > 0` → kısıt. Son kısıt hop kaydedilir.

**Faz 2 — Filling:**
- `None` (kısıt yok) → `fill_unconstrained(input)` → forward fill
- `Some(i)` → `fill_constrained(i)` → backward i..0 + forward i+1..end

**fill_constrained örneği (satır 636-663):**
```
Pairs: UM=>GM=>GN=>USD=>ETH
       0    1    2    3    [pair index]

Pair 2 constraining (0.1 USD max):
exactly_consumed = positions[2].reserves_for(pairs[2].end)  // 0.1 USD

fill_backward(tx, 2, 0.1_USD):
  i=2: fill_output(reserves, 0.1_USD) → new_reserves[2], trace[3]=0.1_USD, current=needed_GN
  i=1: fill_output(reserves, needed_GN) → new_reserves[1], trace[2]=needed_GN, current=needed_GM
  i=0: fill_output(reserves, needed_GM) → new_reserves[0], trace[1]=needed_GM, trace[0]=needed_UM

fill_forward(tx, 3, 0.1_USD):
  i=3: fill(0.1_USD, reserves) → new_reserves[3], trace[4]=output_ETH
```

**Faz 3 — Commit:**
```rust
let should_apply = if let Some(spill_price) = spill_price {
    (tx.actual_price() <= spill_price) || !filled_once
} else {
    true  // limit yok → daima uygula
};
```
`!filled_once` → ilk fill her zaman uygulanır (yakınsama garantisi, satır 52-59).

**Sonraki pozisyon alma:** `replace_empty_positions()`:
- `desired_reserves == 0` → `replace_position(i)`
- `positions_by_price` stream'den sonraki al, daha önce kullanılmışsa atla (BTreeSet)

**Trace toplama:**
```rust
let input = trace.iter().map(|t| t.first().amount).sum();
let output = trace.iter().map(|t| t.last().amount).sum();
```

**Commit:**
```rust
let (mut state, events) = this.apply();
for event in events { state.record(event); }
```
Events `StateDelta`'da tutulur, apply sırasında çıkarılır ve state'e kaydedilir.

---

## 7. `src/component/router/route_and_fill.rs` — Üst Seviye Döngü (312 satır)

### `handle_batch_swaps`

**Veri akışı:**
```
batch_data = (delta_1, delta_2)  // şifresi çözülmüş toplamlar
clamped_1 = min(delta_1, MAX_RESERVE_AMOUNT)
clamped_2 = min(delta_2, MAX_RESERVE_AMOUNT)

exec_1for2 = route_and_fill(asset_1, asset_2, clamped_1, params, shared_breaker)
exec_2for1 = route_and_fill(asset_2, asset_1, clamped_2, params, shared_breaker)

lambda_2 = exec_1for2?.output.amount  // 0 if None
lambda_1 = exec_2for1?.output.amount
unfilled_1 = delta_1 - exec_1for2?.input.amount  // orijinal delta kullanılır!
unfilled_2 = delta_2 - exec_2for1?.input.amount

output_data = BatchSwapOutputData {
    delta_1, delta_2, lambda_1, lambda_2, unfilled_1, unfilled_2,
    height, trading_pair, sct_position_prefix: epoch+block
}
```

**Paylaşılan circuit breaker:** Aynı breaker hem 1→2 hem 2→1 için. Toplam 64 iterasyon iki yön arasında paylaşılır.

### `route_and_fill` Döngüsü

**5 sonlandırma koşulu:**
1. `circuit_breaker.exceeded_limits()` → bütçe aşıldı
2. `path is None` → yol bulunamadı
3. `path is empty` → boş yol
4. `total_unfilled_1 == 0` → tam dolduruldu
5. `accurate_max_price >= price_limit` → fiyat limiti aşıldı

**Overflow kurtarma (satır 225-234):**
```rust
Err(FillError::ExecutionOverflow(position_id)) => {
    close_position_by_id(&position_id).await?;
    continue;  // yeni path_search ile tekrar dene
}
Err(e) => anyhow::bail!("error filling route: {:?}", e);
```

**Toplam trace güncelleme:**
```rust
traces.append(&mut swap_execution.traces.clone());
total_output_2 += produced_output.amount;
total_unfilled_1 -= consumed_input.amount;
```

---

## 8. `src/component/arb.rs` — Arbitraj Mekanizması (151 satır)

**Flash loan tabanlı arbitraj:**
```rust
let flash_loan = Value { asset_id: arb_token, amount: u64::MAX };
let Some(swap_execution) = this
    .route_and_fill(arb_token, arb_token, flash_loan.amount, routing_params, circuit_breaker)
    .await?
else { return Ok(None); };
```

**Kar hesabı:**
```
filled_input = swap_execution.input.amount
output = swap_execution.output.amount
unfilled_input = u64::MAX - filled_input
total_output = output + unfilled_input
arb_profit = total_output - u64::MAX  // checked_sub → None ise atla
```

**Uygulama (kârlıysa):**
```rust
let (self2, cache) = Arc::try_unwrap(this)...flatten();
cache.apply_to(&mut self_mut);  // StateDelta'yı ana state'e uygula
self_mut.set_arb_execution(height, se);
self_mut.dex_vcb_debit(Value { amount: arb_profit, asset_id: arb_token }).await?;
```

**VCB debit:** Arb karı DEX VCB'sinden çıkarılır. Bu LP'lerin o bloktaki zararını temsil eder. Spread ücreti bu zararı telafi etmeli (LP tarafından önceden fiyatlandırılmış olmalı).

**Metrik:** `metrics::histogram!(metrics::DEX_ARB_DURATION)` — arb süresini ölçer.

---

## 9. `src/component/circuit_breaker/execution.rs` (28 satır)

```rust
pub(crate) struct ExecutionCircuitBreaker {
    pub counter: u32,
    pub max: u32,
}
fn exceeded_limits(&self) -> bool { self.counter >= self.max }
fn increment(&mut self) { self.counter += 1; }
```

Kontrol her döngü başında yapılır (fill öncesi). counter=64 iken exceeded_limits=true → döngü kırılır.

---

## 10. `src/component/circuit_breaker/value.rs` — VCB (83 satır + testler)

```rust
trait ValueCircuitBreaker: StateWrite {
    async fn dex_vcb_credit(&mut self, value: Value) -> Result<()>
    async fn dex_vcb_debit(&mut self, value: Value) -> Result<()>
}
```

**Credit:** `prev + amount` → `checked_add` → overflow = error → state'e yaz + event yayınla
**Debit:** `prev - amount` → `checked_sub` → underflow = error → state'e yaz + event yayınla

**Testler (satır 109-297):**
- `value_circuit_breaker`: credit 100 + credit 100 → debit 100 + debit 100 → ek debit = error
- `position_value_circuit_breaker`: VCB sıfırlanmış pozisyon withdraw = error
- `batch_swap_circuit_breaker`: VCB bilgisi olmadan put edilmiş pozisyon → batch swap panics

---

## DEX Uygulama Notu — Routing Perspektifi

### Routing Karar Ağacı

```
handle_batch_swaps(pair, flows, budget):
    route_and_fill(1→2):
        loop(budget):
            path_search(max_hops=4):
                SPFA(max_hops iterations):
                    JoinSet[relax_active_paths]:
                        JoinSet[extend_to(candidate) for each candidate]:
                            best_position(pair) + deindex + hop_price
            fill_route(path, spill_price):
                Frontier::load(pairs)
                loop:
                    sense_capacity_constraint()
                    fill_constrained() or fill_unconstrained()
                    if actual_price > spill_price and filled_once: break
                    replace_empty_positions()
    route_and_fill(2→1):  [paylaşılan circuit breaker!]
        ...aynı döngü...
    set_output_data(BatchSwapOutputData)
```

### Kritik Doğrulanmış Değerler

| Parametre | Değer | Kaynak |
|-----------|-------|--------|
| max_hops | 4 | params.rs:74 |
| max_positions_per_pair | 1_000 | params.rs:75 |
| max_execution_budget | 64 | params.rs:76 |
| DYNAMIC_ASSET_LIMIT | 10 | position_manager.rs:37 |
| Flash loan amount | u64::MAX | arb.rs:41 |
| Execution circuit breaker | exceeded when counter >= max | execution.rs:25 |
| Value breaker credit | checked_add overflow = error | value.rs:36-37 |
| Value breaker debit | checked_sub underflow = error | value.rs:65-66 |

### Nova Shield DEX Eylem Planı (Routing)

**Doğrudan uygulanabilir:**
1. `RoutingParams` + `DexParameters` struct'ı (parametrik)
2. `PathCache` SPFA mantığı (BTreeMap tabanlı)
3. `Frontier` pattern: her hop için best LP + forward/backward fill
4. `ExecutionCircuitBreaker` (basit sayaç)
5. `ValueCircuitBreaker` (checked_add/sub per asset)

**Adapte edilmesi gereken:**
1. `StateDelta` → bizim state layer'ımıza
2. `candidate_set` → bizim liquidity index'imize
3. `positions_by_price` stream → bizim B-tree scan'imize
4. `path.state.fork()` → hafıza baskısını izle (prod'da 17^4 fork maliyetli olabilir)

**Ek değerlendirme:**
- `arb_profit` → DEX treasury'sine mi, yakılsın mı, LP'lere mi dağıtılsın?
- `max_positions_per_pair = 1_000` → bizim sistemimizde kaç olmalı?
- `fixed_candidates` listesi → mainnet'te hangi asset'lar?
- Paralel path search → Tokio runtime üzerindeki yük profillenmeli
