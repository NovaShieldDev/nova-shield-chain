# CoW Protocol Analiz — Agent 08

## Kapsam: liquidity-sources/src/ — AMM Likidite Kaynakları

## DEX Özel Protokolü: 3x Hassasiyet — AMM Math TAM DETAY

**Analiz Tarihi:** 2026-07-22
**Kaynak Dizin:** `crates/liquidity-sources/src/`
**Toplam Dosya:** ~40 Rust kaynak dosyası

---

## 1. lib.rs (71 satır)

### Amaç
`BaselineSource` enum'u tanımlar: hangi DEX'lerin varsayılan likidite kaynağı olarak kullanılacağını ve hangi zincirde hangilerinin aktif olduğunu belirler.

### Struct/Enum/Trait Listesi
- `BaselineSource` enum: `None, UniswapV2, Honeyswap, SushiSwap, BalancerV2, Baoswap, Swapr, ZeroEx, UniswapV3, TestnetUniswapV2`

### Kritik Fonksiyonlar
- `defaults_for_network(chain: Chain) -> Vec<BaselineSource>`: zincire göre aktif kaynakları döner
  - Mainnet: `[UniswapV2, SushiSwap, BalancerV2, ZeroEx, UniswapV3]`
  - Gnosis Chain: `[Honeyswap, SushiSwap, BalancerV2, Baoswap, Swapr, UniswapV3]`
  - Linea / Plasma / Ink: sadece `[UniswapV3]`

### Güvenlik Analizi
- `None` varyantı: solver'ların "no liquidity source" konfigürasyonu için fallback. Yanlış konfigürasyonda tüm likidite kesilir.
- Zincir tespiti yanlışsa yanlış kaynak seti → yanlış fiyatlar.

### DEX Uygulama Notu
Nova Shield DEX için: `LiquiditySource` enum'u aynı pattern ile tasarlanabilir. Her zincire özel kaynak listesi genesis/config dosyasından okunabilir. `None` varyantı test ortamında faydalıdır.

---

## 2. macros.rs (21 satır)

### Amaç
İki derleme zamanı makrosu: Bfp sabit oluşturma ve JSON map oluşturma.

### Kritik Fonksiyonlar
- `bfp!(literal)`: string literal'ı `Bfp` (ufixed256x18)'e çevirir. Derleme hatası verir geçersiz değerlerde.
- `json_map!(k => v, ...)`: `serde_json::Map<String, Value>` oluşturur.

### DEX Uygulama Notu
Sabit fee, amplification parametresi gibi değerleri `bfp!("0.003")` şeklinde derleme zamanında tanımlamak tip güvenliği sağlar. Nova Shield AMM'de aynı pattern önerilir.

---

## 3. base_tokens.rs (253 satır)

### Amaç
Multi-hop routing için "base token" (WETH, USDC, DAI vb.) kavramını ve token çiftlerinden yol üretimini yönetir.

### Struct/Enum/Trait Listesi
- `BaseTokens { tokens: HashSet<Address>, pairs: HashSet<TokenPair> }`

### Kritik Fonksiyonlar
- `new(native_token, base_tokens: Vec<Address>)`: native token her zaman dahil edilir, dedup yapılır.
- `path_candidates(sell, buy) -> Vec<Vec<Address>>`: `DEFAULT_MAX_HOPS=2` ile BFS yol üretimi.
- `relevant_pairs() -> HashSet<TokenPair>`: tüm token × base_token çiftlerini üretir.

### Algoritma Detayı
```
paths = [[sell_token]]
for hop in 0..max_hops:
    new_paths = []
    for path in paths:
        for base in base_tokens:
            if base not in path:
                new_paths.push(path + [base, buy_token])
    paths = new_paths (eğer yeni path varsa)
```
Test sonuçları:
- 0 hop = 1 yol
- 1 hop, 3 base = 4 yol (3 base + doğrudan)
- 2 hop = 10 yol
- 3 hop = 16 yol (satürasyon başlar)

### Güvenlik Analizi
- Cycle koruması: `if base not in path` kontrolü sonsuz döngüyü önler.
- `MAX_HOPS=2` limiti kombinatoryel patlamayı önler.

### DEX Uygulama Notu
Nova Shield Router için: base token kümesi governance ile güncellenebilir. `max_hops` parametresi gas/latency tradeoff yapar. 3 hop üstü pratik değer katmaz çünkü likidite dağılımı artar.

---

## 4. baseline_solvable.rs (29 satır)

### Amaç
Tüm AMM uygulamalarının uyması gereken temel trait: fiyat hesaplama arayüzü.

### Struct/Enum/Trait Listesi
```rust
#[async_trait]
pub trait BaselineSolvable {
    async fn get_amount_out(
        &self,
        out_token: H160,
        input: (U256, H160),
    ) -> Option<U256>;

    async fn get_amount_in(
        &self,
        in_token: H160,
        output: (U256, H160),
    ) -> Option<U256>;

    async fn gas_cost(&self) -> usize;
}
```

### Matematiksel Formüller
- `get_amount_out(out_token, (in_amount, in_token))`: verilen `in_amount` için maksimum `out_amount`
- `get_amount_in(in_token, (out_amount, out_token))`: verilen `out_amount` için minimum `in_amount`
- **Simetrik degil**: `get_amount_in(get_amount_out(x)) >= x` (fee yüzünden)

### Güvenlik Analizi
- `Option<U256>` dönüsü: rezerv yetersizligi veya matematiksel hata durumunda `None`. Caller `None`'i "bu yol geçersiz" olarak islemeli.
- Asenkron tasarim: RPC çagrisini gerektirebilir (UniswapV3 tick hesabi).

### DEX Uygulama Notu
Nova Shield'in AMM modülü bu trait'i implemente etmeli. Router katmani `BaselineSolvable` kullanarak her AMM türünü ayni arayüzle sorgulayabilir. `gas_cost()` yol seçiminde tiebreaker olarak kullanilir.

---

## 5. recent_block_cache.rs (881 satir)

### Amaç
Block bazli TTL'li cache: hem son blok verisini hem de belirli blok numaralarindaki tarihi veriyi saklar. Parallel istek deduplication, otomatik GC, Prometheus metrikleri içerir.

### Struct/Enum/Trait Listesi
- `Block` enum: `Recent | Number(u64) | Finalized`
- `RecentBlockCache<K, V, F>`: disa açik cache tipi, `Arc<Inner<K,V,F>>` wrapper
- `Inner<K,V,F>`: gerçek state
- `Mutexed<K,V>`: mutex korumal mutable state
  - `recently_used: TimedSizedCache<K,()>`: LRU, son kullanim takibi
  - `cached_most_recently_at_block: HashMap<K, u64>`: key → en son cache'lenen blok
  - `entries: BTreeMap<(u64,K), Vec<V>>`: (block, key) → degerler
  - `last_update_block: u64`: son güncelleme blogu
  - `maximum_recent_block_age: u64`: `Block::Recent` için max yas

### Kritik Fonksiyonlar
- `RecentBlockCache::new(number_of_blocks, fetcher, block_stream, options)`:
  - `number_of_blocks`: kaç blokluk veri saklanir
  - `maximum_retries`: fetch basarisiz olursa kaç kez tekrar dene
  - Otomatik güncelleme task'i spawn eder
- `get(keys, block)`:
  1. Mutex al, cache kontrolü yap
  2. **Mutex birak** — async fetch baslat (mutex hiç tutulmaz IO sirasinda)
  3. `BoxRequestSharing` ile ayni (key, block) isteğini deduplicat et
  4. Sonuçlari cache'e yaz, mutex tekrar al
- `update(block_number)`: yeni blok geldiginde tüm `recently_used` key'lerini fetch et
- `remove_cached_blocks_older_than(max_age)`: GC — her key için sadece en yeni entry'yi sakla
- `spawn_gc_task()`: `Weak<Inner>` referansiyla periyodik GC, cache drop'laninca durur

### Cache Miss Mantigi
```
if cached at requested block:
    return cached value (hit)
elif Block::Recent and cached within max_age:
    return cached value (hit)
else:
    fetch from network in batches of REQUEST_BATCH_SIZE=200
    cache result at fetched_block_number
    return result (miss)
```

### Request Deduplication
`BoxRequestSharing`: ayni (key, block) için es zamanli N istek gelirse sadece 1 network çagrisi yapilir, N caller ayni sonucu bekler.

### Prometheus Metrikleri
- `recent_block_cache_hits`: cache hit sayaci (labels: cache_name)
- `recent_block_cache_misses`: cache miss sayaci

### Güvenlik Analizi
- **Mutex hiç IO sirasinda tutulmaz**: deadlock riski minimize edilmis.
- **`maximum_recent_block_age`**: çok eski veri `Block::Recent` olarak sunulmaz — stale data korumasi.
- **GC Weak ref**: cache drop'landiktan sonra GC task bellek sizdirmaz.
- **BTreeMap key `(block, key)`**: block-ordered, range scan verimli.

### DEX Uygulama Notu
Nova Shield'in on-chain veri önbellekleme katmani bu tasarimi kopyalamali:
1. Mutex'i IO sirasinda hiç tutma — tokio best practice
2. `BoxRequestSharing` veya esdeğeri ile istek deduplication
3. Block-keyed BTreeMap → reorg sirasinda eski blok verisi hizli silinebilir
4. `REQUEST_BATCH_SIZE=200` → RPC batching için iyi bir baslangic noktasi

---

## 6. uniswap_v2/mod.rs (375 satir)

### Amaç
UniswapV2 tipi tüm DEX'ler (Uniswap, Sushi, Honey, Bao, Swapr, Testnet) için ortak konfigürasyon ve init.

### Struct/Enum/Trait Listesi
- `UniV2BaselineSource { router, pair_provider, pool_fetching }`
- `UniV2BaselineSourceParameters { router_address, init_code_digest, pool_reading }`
- `PoolReadingStyle` enum: `Event | Contract`

### Kritik Fonksiyonlar
- `from_baseline_source(source, chain, web3)`: her DEX için hardcoded router adresi + init code digest döner
- `into_source()`: router'dan `factory()` çagrisi → `PairProvider` olustur → `PoolFetcher` olustur
- `FromStr / Display for UniV2BaselineSourceParameters`: `"0xRouter|0xDigest|contract"` format

### Init Code Digest'ler (32-byte keccak256 of pool bytecode)
```
UNISWAP_INIT:   96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f
HONEYSWAP_INIT: 3f88503e8580ab941773b59034fb4b2a63e86dbc031b3633a925533ad3ed2b93
SUSHISWAP_INIT: e18a34eb0e04b04f7a0ac29a6e80748dca96319b42c54d679cb821dca90c6303
BAOSWAP_INIT:   0bae3ead48c325ce3d638c4bdd5cf4ae4f0f5f3e45c7a9af2c09b4a78e82897c
SWAPR_INIT:     d306a548755b9295ee49cc729e13ca4a45e00199bbd890fa146da43a50571776
```

### Güvenlik Analizi
- Init code digest yanlissan `pair_address()` yanlish adres hesaplar → tamamen farkli kontrat → yanlis fiyat.
- Digest'ler hardcode: yeni fork eklenirse manuel güncelleme gerekir.

### DEX Uygulama Notu
Nova Shield kendi CPAMM pool'u için init code digest'i genesis'te sabitleyebilir. Upgrade senaryosu: V2/V3 tarzinda migration (yeni factory + yeni digest).

---

## 7. uniswap_v2/pair_provider.rs (59 satir)

### Amaç
UniswapV2 CREATE2 pair adres hesaplama.

### Struct/Enum/Trait Listesi
- `PairProvider { factory: Address, init_code_digest: [u8; 32] }`

### Kritik Fonksiyonlar
- `pair_address(pair: &TokenPair) -> Address`:
  1. `(token0, token1) = pair.get()` (düsük adres önce — canonical sort)
  2. `salt = keccak256(token0 ++ token1)` (ABI encode: 32 byte padded each)
  3. `create2_target_address(factory, salt, init_code_digest)`
- `create2_target_address(creator, salt, init_code_digest) -> Address`:
  1. `preimage = 0xff ++ creator ++ salt ++ init_code_digest` (85 bytes)
  2. `hash = keccak256(preimage)`
  3. `address = hash[12..]` (son 20 byte)

### Matematiksel Formüller
```
pair_address = keccak256(
    0xff
    ++ factory_address (20 bytes)
    ++ keccak256(abi.encode(token0, token1))  // salt
    ++ init_code_hash  // 32 bytes
)[12:]
```
Bu EIP-1014 (CREATE2) standardidir.

### Güvenlik Analizi
- Token siralamasii kritik: `pair.get()` her zaman `(min_addr, max_addr)` döner. Ters sira → farkli salt → farkli adres.
- `0xff` prefix: CREATE2'yi CREATE1'den ayirir, adres çakismasi önlenir.

### DEX Uygulama Notu
Nova Shield pool adresleri ayni CREATE2 mekanizmasiyla deterministik hesaplanabilir. Avantaj: factory contract gerekmez, off-chain hesaplanir. Dezavantaj: init code degisirse tüm adresler degisir.

---

## 8. uniswap_v2/pool_cache.rs (53 satir)

### Amaç
`RecentBlockCache` üzerine `TokenPair → Pool` cache katmani.

### Struct/Enum/Trait Listesi
- `PoolCache`: `RecentBlockCache<TokenPair, Pool, Arc<dyn PoolFetching>>`
- `CacheKey<Pool> for TokenPair`: `first_ord()` ve `for_value(pool) = pool.tokens`
- `CacheFetching for Arc<dyn PoolFetching>`: async fetch delegasyonu

### DEX Uygulama Notu
Pool cache katmani blok akisina bagli: her yeni blok → otomatik invalidasyon. Nova Shield'de de ayni mimari: block event → cache update task.

---

## 9. uniswap_v2/pool_fetching.rs (546 satir)

### Amaç
UniswapV2 CPAMM pool state okuma: rezervler, fee, elastic token korumasi ve AMM matematik.

### Struct/Enum/Trait Listesi
- `Pool { address, tokens: TokenPair, reserves: (u128, u128), fee: Ratio<u32> }`
- `PoolFetching` trait: `async fetch(pairs, block) -> Vec<Pool>`
- `DefaultPoolReader`: gerçek RPC okuma implementasyonu
- `FakePoolFetcher`: test utility, hardcoded pool listesi döner

### Sabitler
- `POOL_MAX_RESERVES: u128 = (1u128 << 112) - 1`
- `POOL_SWAP_GAS_COST: usize = 60_000`

### Matematiksel Formüller: CPAMM (x*y=k)

#### Pool::uniswap(): Standart 0.3% Fee
```rust
fee = Ratio::new(3u32, 1000u32)  // 0.3%
```

#### get_amount_out(in_amount, token_in, token_out) -> Option<U256>
```
// fee = numer/denom = 3/1000
amount_in_with_fee = in_amount * (denom - numer)
                   = in_amount * 997

numerator   = amount_in_with_fee * reserve_out
denominator = reserve_in * denom + amount_in_with_fee
            = reserve_in * 1000 + in_amount * 997

amount_out = numerator / denominator  (floor division)
```

Tam formül:
```
amount_out = (amount_in * 997 * reserve_out) /
             (reserve_in * 1000 + amount_in * 997)
```

#### get_amount_in(out_amount, token_in, token_out) -> Option<U256>
```
numerator   = reserve_in * out_amount * denom
            = reserve_in * out_amount * 1000

denominator = (reserve_out - out_amount) * (denom - numer)
            = (reserve_out - out_amount) * 997

amount_in = numerator / denominator + 1  (ceiling division)
```

Tam formül:
```
amount_in = ceil(reserve_in * out_amount * 1000 /
                ((reserve_out - out_amount) * 997))
```

#### check_final_reserves
```
final_reserve_in  = reserve_in_before + amount_in
final_reserve_out = reserve_out_before - amount_out

Kosul: final_reserve_in <= POOL_MAX_RESERVES
       final_reserve_out >= 0 (u128 underflow kontrolü)
```

### Elastic Token (Rebase) Korumasi
`DefaultPoolReader::fetch()` içinde:
1. `getReserves()` cagrisi → `(reserve0, reserve1)`
2. `token0.balanceOf(pair_address)` ve `token1.balanceOf(pair_address)` paralel cagri
3. Kontrol: `balance0 >= reserve0 && balance1 >= reserve1`
4. Eger balance < reserve: pool hariç tutulur (elastic/rebase token tespiti)

Bu kontrol olmadan rebase token'lar yanlis likidite gösterir (gerçek balance rezervden küçük olabilir).

### PoolFetcher Implementasyonu
- `non_existent_pools: TimedCache<TokenPair, ()>`: bilinen-yok havuzlar önbellegi
- `fetch()`: önce non_existent check, sonra RPC batch, bulunamayanlar cache'e eklenir
- `EthCallMany` ile toplu paralel RPC cagrisi

### Güvenlik Analizi
1. **Overflow korumasi**: `u128` aritmetik, `checked_mul` / `checked_add` kullanimi. `POOL_MAX_RESERVES = 2^112-1` UniswapV2 solidity siniriyla eslesmis.
2. **Elastic token korumasi**: balance vs reserve karsilastirmasi.
3. **Sifir reserve**: `reserve_out == 0` durumunda denominator = 0 → `None`.
4. **Ceiling division** (amount_in): +1 ekleyerek kullanicidan eksik alinmaz.

### DEX Uygulama Notu
Nova Shield CPAMM için kesin formüller:
- `amount_out`: floor division (protokol lehine)
- `amount_in`: ceiling division (kullanici tam ödeme yapar)
- Fee: `Ratio<u32>` ile fraksiyonel, genisletilebilir fee tiers için ideal
- `POOL_MAX_RESERVES = 2^112-1`: Uniswap V2 uyumlu sinir
- Rebase token korumasi: Nova Shield de native token veya yield-bearing token pool'lari için gerekli

---

## 10. uniswap_v3/mod.rs (47 satir)

### Amaç
UniswapV3 pool data source abstraction: subgraph ve pool-indexer için ortak trait.

### Struct/Enum/Trait Listesi
```rust
#[async_trait]
pub trait V3PoolDataSource: Send + Sync + 'static {
    async fn get_registered_pools(&self, target_block: u64)
        -> Result<RegisteredPools>;

    async fn get_pools_with_ticks_by_ids(
        &self,
        ids: &[Address],
        target_block: u64,
    ) -> Result<PoolsWithTicks>;
}
```

### Önemli Contract
- Her iki metod da `fetched_block_number` içeren struct döner
- `fetched_block_number >= target_block` garantisi (pool-indexer biraz ilerde olabilir)
- Caller `fetched_block_number`'i event replay anchor olarak kullanmali, `target_block`'i degil

### DEX Uygulama Notu
Pool data source abstraction: on-chain vs indexer backend'lar kolayca degistirilebilir. Nova Shield kendi indexer service'i için ayni trait'i implemente edebilir.

---

## 11. uniswap_v3/pool_fetching.rs (753 satir)

### Amaç
UniswapV3 konsantre likidite pool state yönetimi: checkpoint + event overlay sistemi.

### Struct/Enum/Trait Listesi
- `PoolFetching` trait: `async fetch(pairs, at_block) -> Vec<Arc<PoolInfo>>`
- `PoolInfo { address, tokens: Vec<Token>, state: PoolState, gas_stats: PoolStats }`
- `PoolState { sqrt_price: U256, liquidity: U256, tick: i32, liquidity_net: Arc<BTreeMap<i32, i128>>, fee: Ratio<u32> }`
- `PoolsCheckpointHandler`: ilk N pool'u likiditeye göre siralar, eksik pool tracking
- `UniswapV3PoolFetcher`: checkpoint + event cache kombinasyonu

### Sabitler
- `GAS_ESTIMATE: U256 = 108_163` (Dune query'den: median gas per UniV3 swap)

### Matematiksel Formüller: Concentrated Liquidity

#### Fee Tier Dönüsümü
```rust
// fee_tier: 500 = 0.05%, 3000 = 0.3%, 10000 = 1%
fee = Ratio::new(fee_tier as u32, 1_000_000u32)
// 500/1_000_000 = 0.0005
```

#### sqrtPriceX96 (Q96 Format)
```
sqrtPrice = sqrt(price) * 2^96
price = sqrtPrice^2 / 2^192
```
U256 olarak saklanir, hesaplamalar 256-bit overflow'a dikkat gerektirir.

#### Tick <-> Price
```
price = 1.0001^tick
tick = log_{1.0001}(price)
```

#### Liquidity Net (Event Güncelleme)
```
// Swap sonrasi: tick degisir, liquidity_net'ten geçilen tick'ler toplanir
// Mint: pozisyon araligi içindeyse aktif liquidity artar
// Burn: pozisyon araligi içindeyse aktif liquidity azalir
```

### Event Replay Mantigi
1. **Checkpoint**: belirli blokta tüm pool state'leri (V3PoolDataSource'dan)
2. **Recent events**: Swap/Mint/Burn event'leri BTreeMap'te `(block, log_index)` ile saklanir
3. **Fetch zamani**: checkpoint state + [checkpoint_block, current_block] arasi event'ler uygulanir

#### append_events() Islemleri
```rust
match event {
    Swap(e) => {
        state.tick = e.tick;
        state.liquidity = e.liquidity;
        state.sqrt_price = e.sqrt_price_x96;
    }
    Mint(e) => {
        if e.tick_lower <= state.tick && state.tick < e.tick_upper {
            state.liquidity += e.amount;
        }
        update_liquidity_net(state, e.tick_lower, +e.amount);
        update_liquidity_net(state, e.tick_upper, -e.amount);
    }
    Burn(e) => {
        if e.tick_lower <= state.tick && state.tick < e.tick_upper {
            state.liquidity -= e.amount;
        }
        update_liquidity_net(state, e.tick_lower, -e.amount);
        update_liquidity_net(state, e.tick_upper, +e.amount);
    }
}
```

#### update_liquidity_net()
- `saturating_add`: tasma olmaz (i128 araligi yeterli)
- Net = 0 olunca entry silinir (BTreeMap temiz tutulur)

### PoolsCheckpointHandler
- Init: `get_registered_pools()` → likiditeye göre sirala → top-N al
- `missing_pools`: henüz checkpoint'te olmayan ama istenen pool'lar
- Lazy update: eksik pool'lar bir sonraki `fetch_pools_with_ticks()` cagrisiinda eklenir
- Checkpoint forward: `last_event_block - MAX_REORG_BLOCK_COUNT` = safe checkpoint block

### Güvenlik Analizi
1. **Reorg korumasi**: `MAX_REORG_BLOCK_COUNT` blok geride checkpoint tutulur.
2. **Missing pools set**: bilinmeyen pool için event uygulanmaz (state corruption önlenir).
3. **Liquidity filter**: `liquidity == 0` pool'lar fetch sonucuna dahil edilmez.
4. **Arc<BTreeMap>**: liquidity_net immutable shared → race condition yok.

### DEX Uygulama Notu
Nova Shield konsantre likidite modülü için:
- Tick tabanlii liquidity_net BTreeMap: aktif tick araligi hesabi için zorunlu
- Event-driven state: her blokta RPC cagrisi yerine sadece event'ler uygulanir (çok daha verimli)
- Checkpoint + events pattern: blockchain node indexer tasariminin standardi
- `MAX_REORG_BLOCK_COUNT`: Ethereum'da 6-12 blok yeterli, Nova Shield için zincir finality'sine göre ayarla

---

## 12. uniswap_v3/event_fetching.rs (296 satir)

### Amaç
UniswapV3 Swap/Mint/Burn event'lerini RPC'den çekme ve bellekte sirali saklama.

### Struct/Enum/Trait Listesi
- `UniswapV3PoolEventFetcher { provider: DynProvider }`: RPC event fetcher
- `WithAddress<T> { inner: T, address: Address }`: event + pool adresi wrapper
- `RecentEventsCache`: `BTreeMap<(block_number, log_index), WithAddress<UniswapV3PoolEvents>>`

### Kritik Fonksiyonlar
- `fetch_events(from_block, to_block)`: 3 event tipini filtreler (Swap/Mint/Burn imzalari), address filtresi YOK (RPC overload önlemi)
- `remove_events_older_than_block(n)`: `split_off((n, 0))` → eski kismi at
- `remove_events_newer_than_block(n)`: `split_off((n+1, 0))` → yeni kismi at (reorg recovery)
- `get_events(range)`: `BTreeMap::range(from..=to)` ile inclusive range sorgu

### EventStoring Trait Implementasyonu
```rust
replace_events(range, new_events):
    // Reorg: range.start'tan itibaren tümünü sil, yenileri ekle
    remove_events_newer_than_block(range.start - 1)
    append_events(new_events)

append_events(events):
    // Normal: sona ekle
    for e in events { cache.insert((block, log_index), e); }

last_event_block() -> Option<u64>:
    cache.keys().next_back().map(|(block, _)| *block)

persist_last_indexed_block():
    // No-op: in-memory only, persistence yok
```

### Güvenlik Analizi
- **Address filtresi yok**: Tüm UniV3 event'leri çekilir, sonra pool adresi event'ten çikarilir. RPC `getLogs` call'larinda adres listesi çok uzun olunca RPC provider hata verebilir → global filter daha güvenli.
- **Duplicate log index protection**: `(block, log_index)` composite key → ayni log iki kez girilmez.
- **Reorg handling**: `replace_events` ile reorg sonrasi tutarsiz state önlenir.

### DEX Uygulama Notu
Nova Shield event indexer için:
- `(block, log_index)` composite BTreeMap key: event sirasini korur, dedup saglar
- Address filtresi tradeoff: az pool = address filter verimli; çok pool (100+) = global filter
- `persist_last_indexed_block` no-op: yeniden baslatmada tüm events checkpoint'ten yeniden çekilir. Production'da PostgreSQL persist gerekir.

---

## 13. uniswap_v3/graph_api.rs (549 satir)

### Amaç
UniswapV3 subgraph'ten pool ve tick verisi çekme.

### Struct/Enum/Trait Listesi
- `UniV3SubgraphClient { client: SubgraphClient, use_liquidity_net_filter: bool }`
- `PoolData { id, token0, token1, fee_tier, liquidity, sqrt_price, tick, ticks: Option<Vec<TickData>>, block_number }`
- `TickData { tick_idx, liquidity_net, pool_address }`
- `RegisteredPools { fetched_block_number, pools: Vec<PoolData> }`
- `PoolsWithTicks { fetched_block_number, pools: Vec<PoolData> }`

### Kritik Fonksiyonlar
- `get_registered_pools(block)`: tüm havuzlari block'ta sorgular, `liquidityNet_not` filter dener
- `set_liquidity_net_filter()`: subgraph `liquidityNet_not` destegini probe eder
- `get_safe_block()`: `latest - MAX_REORG_BLOCK_COUNT` — reorg-safe blok
- `TickData::get_id()`: `"0x{pool_addr}#{tick_idx}"` — pagination cursor

### Güvenlik Analizi
- `liquidityNet_not` filter dinamik probe: subgraph versiyona göre farkli field destekler.
- `get_safe_block()`: çok yeni blok sorgulanirsa subgraph henüz index etmemis olabilir.
- Liquidity = 0 filtreleme: subgraph'te zaten filtrelenir ama pool_fetching'de de kontrol edilir (defense in depth).

### DEX Uygulama Notu
Nova Shield kendi indexer/subgraph katmani için:
- Block bazli sorgu: historical queries için `block: {number: N}` GraphQL syntax'i
- Pagination: `id_gt: lastId` cursor pattern (QUERY_PAGE_SIZE=1000)
- Safe block offset: yeni bir bloku aninda sorgulamak race condition riski tasir

---

## 14. uniswap_v3/pool_indexer.rs (451 satir)

### Amaç
Subgraph alternatifi: kendi pool-indexer microservice'inden HTTP ile V3 pool verisi çekme.

### Struct/Enum/Trait Listesi
- `PoolIndexerClient { base_url: Url, http: Client }`
- `PoolsResponse { block_number, pools: Vec<IndexerPool>, next_cursor }`
- `BulkTicksResponse { block_number, pools: HashMap<Address, Vec<IndexerTick>> }`

### Sabitler
```rust
WAIT_UNTIL_POLL_INTERVAL: Duration = 500ms
WAIT_UNTIL_TIMEOUT: Duration = 60s
POOL_IDS_PER_REQUEST: usize = 500
LIST_PAGE_SIZE: u64 = 5000
BATCH_CONSISTENCY_MAX_RETRIES: usize = 3
```

### Kritik Fonksiyonlar
- `wait_until(target_block)`: `/pools?limit=1` poll'u, 503 = still bootstrapping, timeout = 60s
- `get_registered_pools(target_block)`: cursor-based pagination, zero-liquidity ve no-decimals filter
- `get_pools_with_ticks_by_ids(ids, target_block)`: 500'lük batch'ler, paralel pools+ticks fetch
- `fetch_batch_consistent(ids)`: pools/by-ids ve pools/ticks ayni anda fetch, block mismatch'te 3 retry

### fetch_batch_consistent Algoritmasi
```
for attempt in 0..3:
    (pools_page, ticks_response) = parallel_fetch(ids)
    if pools_page.block_number == ticks_response.block_number:
        return Ok((pools_page, ticks_response))
    warn!("block mismatch, retrying")
bail!("block mismatch after 3 retries")
```

### Güvenlik Analizi
- **Block consistency**: pools ve ticks farkli bloklardan gelmez (retry mekanizmasi)
- **Decimal validation**: `None` decimal = pool hariç (yanlis fiyat önlenir)
- **503 handling**: `wait_until` döngüsü — indexer henüz hazir degil durumu graceful handle

### DEX Uygulama Notu
Nova Shield'in kendi pool-indexer microservice tasarimi için:
- REST endpoint: `/pools` (cursor paginated) + `/pools/by-ids` + `/pools/ticks`
- Response'a `block_number` ekle: consumer'in anchor block'u bilmesi için kritik
- 503 = bootstrapping: service baslatilirken client'lar bekleyebilir
- Consistency retry: iki endpoint ayni anda yaziliyorsa MVCC veya atomic write gerekir

---

## 15. uniswap_v3_pair_provider.rs (44 satir)

### Amaç
UniswapV3 pool CREATE2 adresi hesaplama.

### Matematiksel Formüller
```
INIT = 0xe34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54

salt = keccak256(
    token0 padded to 32 bytes
    ++ token1 padded to 32 bytes
    ++ fee as 4-byte big-endian
)

pair_address = keccak256(
    0xff
    ++ factory (20 bytes)
    ++ salt (32 bytes)
    ++ INIT (32 bytes)
)[12:]
```

### V2 vs V3 Farki
- V2: salt = `keccak256(token0 ++ token1)` (2x32 bytes)
- V3: salt = `keccak256(token0 ++ token1 ++ fee)` (2x32 + 4 bytes) — fee tier dahil

### DEX Uygulama Notu
Nova Shield konsantre likidite havuzlari için: fee tier'i adreslemeye dahil et → ayni token çifti için farkli fee tier'lar farkli adreslerde. Ekonomik mantik: düsük volatilite = düsük fee tier'da daha fazla likidite.

---

## 16. subgraph.rs (314 satir)

### Amaç
Genel amaçli GraphQL subgraph client: pagination, retry, error handling.

### Struct/Enum/Trait Listesi
- `SubgraphClient { url: Url, max_pools_per_tick_query: usize, max_number_of_attempts: u32 }`
- `Query<V>` trait: `QUERY: &str`, `query_name()`, `variables(&self) -> V`, `next_page(&mut self, data: &Self::Data)`
- `QueryResponse<T>`: GraphQL yanit ayristirma

### Kritik Fonksiyonlar
- `paginated_query<Q: Query>(query)`: cursor-based pagination
  - Her sayfa: `QUERY_PAGE_SIZE = 1000` item
  - Cursor: son item'in `get_id()` → `lastId` degiskeni
  - `max_number_of_attempts = 10`: basarisiz sayfada retry
  - Bos sayfa = son sayfa, döngü biter

### Pagination Algoritmasi
```
lastId = ""
loop:
    response = graphql(QUERY, {lastId: lastId, ...})
    if response.data is empty: break
    results.extend(response.data)
    lastId = response.data.last().get_id()
    if response.data.len() < PAGE_SIZE: break
```

### QueryResponse<T> Ayristirma
- `Ok(data)`: `data` alani var, `errors` yok
- `Err(errors)`: `errors` alani var, `data` yok
- Diger: `InvalidResponse`

### Güvenlik Analizi
- `max_number_of_attempts`: subgraph hatalari geçici olabilir, retry güvenli
- Cursor pagination: offset pagination'a kiyasla tutarli (sayfa kesilmesinde item tekrari/atlanmasi olmaz)

### DEX Uygulama Notu
Nova Shield kendi indexer REST/GraphQL client'i için:
- Cursor-based pagination standardi: her zaman `id_gt: lastId` pattern
- Retry ile exponential backoff: `max_attempts * sleep` yerine `2^attempt * base_delay` daha iyi

---

## 17. swapr.rs (141 satir)

### Amaç
Swapr DEX: UniswapV2 forku ama dinamik (degisken) fee orani.

### Struct/Enum/Trait Listesi
- `SwaprPoolReader(DefaultPoolReader)`: `DefaultPoolReader` wrapper

### Sabitler
```rust
FEE_BASE: u32 = 10_000  // Swapr fee basis: 10000 = 100%
```

### Kritik Fonksiyonlar
- `fetch(pairs, block)`:
  1. `DefaultPoolReader::fetch()` ile reservleri al
  2. Her pool için `ISwaprPair.swapFee()` paralel cagri
  3. Contract hatalari ignore (havuzun fee'si yok = skip), RPC hatalari propagate
  4. `pool.fee = Ratio::new(dynamic_fee, FEE_BASE)`

### Dinamik Fee Mantigi
```
fee_ratio = swapFee_value / FEE_BASE
         = swapFee_value / 10_000

Örnek: swapFee = 30 → fee = 30/10000 = 0.003 = 0.3%
       swapFee = 5  → fee = 5/10000  = 0.0005 = 0.05%
```

### Güvenlik Analizi
- Contract error = pool skip (fee bilinmiyorsa güvensiz): dogru tasarim
- RPC error = propagate: altyapi hatasi ile pool hatasi ayrisir

### DEX Uygulama Notu
Nova Shield'e "dinamik fee" eklenebilir: her epoch'ta volatility-based fee ayari. AMM formülünde sadece `fee: Ratio<u32>` güncellenmesi yeterli.

---

## 18. zeroex.rs (678 satir)

### Amaç
0x Protocol v4 RFQ/limit order off-chain order book entegrasyonu.

### Struct/Enum/Trait Listesi
- `Order`: tam 0x v4 order struct (chain_id, expiry, maker/taker, amounts, tokens, signature, salt, pool, sender, fee_recipient, verifying_contract)
- `OrderRecord(Arc<Inner>)`: order + metadata
- `ZeroExApi` trait: `async get_orders(query) -> Result<Vec<OrderRecord>, ZeroExResponseError>`
- `DefaultZeroExApi`: HTTP implementasyon

### Sabitler
```rust
DEFAULT_VERIFICATION_CONTRACT: "0xDef1C0ded9bec7F1a1670819833240f027b25EfF"
ORDERS_MAX_PAGE_SIZE: usize = 1000
```

### Kritik Fonksiyonlar
- `remaining_maker_amount()`: proportional scaling ile U256 aritmetik (overflow yok)
- `get_orders(query)`: pagination loop — sayfalar dolana kadar devam
- `retain_valid_orders()`: expired ve duplicate hash'ler çikarilir
- `X-Current-Block-Hash` header: 0x API'ina mevcut blok hash'i ilet (MEV korumasi)

### Pagination Algoritmasi
```
page = 1
loop:
    response = GET /orders?page=N&perPage=1000&...
    results.extend(response)
    if response.len() < PAGE_SIZE: break
    page += 1
retain_valid_orders(results)  // expiry + dedup
```

### retain_valid_orders()
- Non-expired: `order.expiry > now`
- Unique by `order_hash`: HashMap, ilk gelen kalir

### Güvenlik Analizi
- **Expiry check**: expired order'lar settlement sirasinda revert eder → önceden filtrele
- **Dedup by hash**: 0x API bazen ayni order'i tekrarlayabilir
- **Rate limiting**: `ZeroExResponseError::RateLimited` → backoff uygula
- **MEV korumasi**: `X-Current-Block-Hash` ile market maker'in hangi blokta quote verdigi bilinir

### DEX Uygulama Notu
Nova Shield'e RFQ (Request for Quote) eklenebilir:
- Off-chain order book: limit order'lar için çok daha iyi fiyat
- `verifying_contract`: on-chain settlement contract adresi
- Expiry + dedup: zorunlu validasyon adimlari
- Block hash header: MEV korumasi için kritik (stale quote kullanimini engeller)

---

## 19. balancer_v2/mod.rs (50 satir)

### Amaç
Balancer V2 modülü re-export hub.

### Alt Modüller
- `graph_api`: subgraph client (private)
- `pool_fetching`: pool fetcher (pub)
- `pool_init`: initialization (private)
- `pools`: pool state types (pub)
- `swap`: AMM matematik (pub)

---

## 20. balancer_v2/graph_api.rs (486 satir)

### Amaç
Balancer subgraph'ten havuz listesi ve token bilgisi çekme.

### Struct/Enum/Trait Listesi
- `BalancerSubgraphClient { client: SubgraphClient }`
- `RegisteredPools { fetched_block_number, pools: Vec<PoolData> }`
- `PoolData { pool_type, id: B256, address, factory, swap_enabled, tokens }`
- `PoolType` enum: `Stable | Weighted | LiquidityBootstrapping | ComposableStable`
- `Token { address, decimals, weight: Option<Bfp> }`

### Kritik Fonksiyonlar
- `get_pools(block)`: subgraph'ten tüm Balancer havuzlari çek
  - `totalLiquidity_gt: "1"` filtresi (min $1 likidite)
  - `QUERY_PAGE_SIZE = 1000`
- `group_by_factory()`: `HashMap<factory_address, RegisteredPools>`

### Sorgu Edilen Pool Tipleri
1. `Stable` — StableSwap (Curve benzeri)
2. `Weighted` — Weighted AMM
3. `LiquidityBootstrapping` — dinamik agirlik, token sale için
4. `ComposableStable` — BPT (Balancer Pool Token) dahil, rate provider'li

### Güvenlik Analizi
- `totalLiquidity_gt: "1"`: düsük likidite havuzlar filtrelenir
- `swap_enabled` alani: subgraph'te false olan havuzlar pool_fetching'de de skip edilir
- Balancer pool address: `pool_id`'nin ilk 20 byte'i

---

## 21. balancer_v2/pools/mod.rs (132 satir)

### Amaç
Balancer havuz tipleri için trait hierarchy ve ortak tipler.

### Struct/Enum/Trait Listesi
- `Pool { id: B256, kind: PoolKind }`
- `PoolKind` enum: `Weighted(weighted::PoolState) | Stable(stable::PoolState)`
- `PoolStatus` enum: `Active(Pool) | Paused | Disabled`
- `FactoryIndexing` trait:
  ```rust
  type PoolInfo: PoolIndexing;
  type PoolState: Into<PoolKind>;
  fn specialize_pool_info(&self, common: CommonPoolInfo) -> Future<Result<Self::PoolInfo>>;
  fn fetch_pool_state(&self, info: &Self::PoolInfo, ...) -> Future<Result<Option<Self::PoolState>>>;
  ```
- `PoolIndexing` trait: `from_graph_data()`, `common()`

### Tasarim Karari
Generic `FactoryIndexing` trait + associated types: her factory tipi (Weighted/Stable/LBP/ComposableStable) ayni interface. Rust'in monomorphization'i → sifir maliyet.

---

## 22. balancer_v2/pools/common.rs (1045 satir)

### Amaç
Tüm Balancer havuz tipleri için ortak pool bilgisi ve state: token scaling, fee, pause state.

### Struct/Enum/Trait Listesi
- `PoolInfoFetcher<Factory> { vault, factory, token_infos }`
- `PoolInfo { id, address, tokens: Vec<Address>, scaling_factors: Vec<Bfp>, block_created }`
- `PoolState { paused: bool, swap_fee: Bfp, tokens: BTreeMap<Address, TokenState> }`
- `TokenState { balance: U256, scaling_factor: Bfp }`

### Matematiksel Formüller: Token Scaling

#### scaling_factor_from_decimals(decimals: u8) -> Result<Bfp>
```
scaling_factor = 10^(18 - decimals)

Örnekler:
  USDC (6 decimal):  scaling_factor = 10^12 = 1_000_000_000_000
  WETH (18 decimal): scaling_factor = 10^0  = 1
  Token (2 decimal): scaling_factor = 10^16

Hata: decimals > 18 → InvalidDecimal
```

#### compute_scaling_rate(factor: Bfp) -> U256
```
scaling_rate = 10^18 / factor
             = 10^18 / 10^(18-decimals)
             = 10^decimals

Bu, solver'in tükettigi format: token amount * scaling_rate = 18-decimal normalized amount
```

#### TokenState::upscale(amount: U256) -> Result<Bfp>
```
upscaled = Bfp::from_wei(amount) * scaling_factor
// amount (token decimals) → 18 decimal internal format
```

#### TokenState::downscale_up/down(bfp: Bfp) -> U256
```
downscaled_down = bfp / scaling_factor  (floor)
downscaled_up   = bfp / scaling_factor  (ceil)
// 18 decimal internal → token decimals
```

### fetch_common_pool_info() Akisi
```
1. vault.getPoolId() → poolId (B256)
2. vault.getPoolTokens(poolId) → (tokens, balances, lastChangeBlock)
3. token_infos.get(token_addrs) → decimals per token
4. scaling_factor_from_decimals(d) per token
5. Return PoolInfo { tokens, scaling_factors, block_created }
```

### fetch_common_pool_state() Akisi
```
Paralel:
  - pool.getPausedState() → paused: bool
  - pool.getSwapFeePercentage() → swap_fee: U256
  - vault.getPoolTokens(poolId) → (tokens, balances)

Token mismatch kontrolü: vault tokens == PoolInfo tokens (sira ve içerik)
```

### share_common_pool_state() Pattern
```rust
// Oneshot channel trick: Arc<Result<_>> yerine kanal
// Amaç: Ok degerini birden fazla consumer'a clone etmek
// Error Clone gerektirmez
let (tx, rx) = tokio::sync::oneshot::channel();
tx.send(state).ok();
let shared = rx.await;
```

### Güvenlik Analizi
1. **Token mismatch**: vault'tan gelen token listesi != kayitli token listesi → pool skip. Kritik: yanlis indeksleme AMM formülünü bozar.
2. **`decimals > 18` hata**: scaling factor negatif exponent → Bfp'ye sigmaz. Hard fail.
3. **Paused state**: `getPausedState()` true ise `PoolStatus::Paused` → swap yok.
4. **BTreeMap for tokens**: deterministik siralama → vault token sirasiyla eslestirme

### DEX Uygulama Notu
Nova Shield multi-decimal token pool'u için:
- `scaling_factor = 10^(18-decimals)`: tüm iç hesaplamalar 18 decimal
- `upscale` → AMM math → `downscale`: standart akis
- `downscale_up` (input için ceiling) vs `downscale_down` (output için floor): kullanici aleyhine yuvarlama önlenir

---

## 23. balancer_v2/pools/weighted.rs (330 satir)

### Amaç
Balancer Weighted Pool: arbitrar agirlikli CPMM. `x^w1 * y^w2 = k` invarianti.

### Struct/Enum/Trait Listesi
- `PoolInfo`: common + `weights: Vec<Bfp>`
- `PoolState { tokens: BTreeMap<Address, TokenState>, swap_fee: Bfp, version: Version }`
- `TokenState`: `common::TokenState + weight: Bfp`
- `Version` enum: `V0 | V3Plus`

### Kritik Fonksiyonlar
- `specialize_pool_info()`: `getNormalizedWeights()` RPC cagrisi → weights = Vec<Bfp>
  - Normalized weights: tüm agirliklar toplami ≈ 1.0

### Matematiksel Formüller

#### Weighted AMM Invariant
```
W(balances, weights) = prod(balance_i ^ weight_i)

Swap sonrasi invariant degismez:
prod(balance_i_after ^ weight_i) = prod(balance_i_before ^ weight_i)
```

#### getNormalizedWeights() Return Format
```
weights: Vec<Bfp>  // her agirlik 18-decimal fixed point
// Örnek: 80/20 pool: [0.8e18, 0.2e18]
// toplam = 1.0e18 (normalized)
```

### V0 vs V3Plus
- V0: `Bfp::pow_up()` — ln + exp via logexpmath (genel)
- V3Plus: `Bfp::pow_up_v3()` — special cases for exp=1,2,4 (integer mul, daha hizli ve hassas)

### DEX Uygulama Notu
Nova Shield Weighted AMM: 50/50 baslangic, governance ile agirlik degisimi. Agirlik smooth geçisi için LBP pattern uygulanabilir.

---

## 24. balancer_v2/pools/stable.rs (181 satir)

### Amaç
Balancer Stable Pool: Curve StableSwap benzeri, düsük slippage için.

### Struct/Enum/Trait Listesi
- `AmplificationParameter { factor: U256, precision: U256 }` (non-zero enforced)
- `PoolState`: common::PoolState + `amplification_parameter: AmplificationParameter`

### Matematiksel Formüller

#### AmplificationParameter::with_base(base) -> U256
```
amp_eff = factor * base / precision

Örnek: factor=1000, precision=1000, base=1 → amp_eff=1
       factor=2000, precision=1000, base=1 → amp_eff=2
```

#### getAmplificationParameter() Return
```
(currentValue, isUpdating, precision)
// precision = AMP_PRECISION = 1000
// effective A = currentValue / precision
```

### DEX Uygulama Notu
StableSwap için amplification parametresi `A`: yüksek A → daha düsük slippage ama daha büyük depeg riski. Governance ile ayarlanabilir (geçis yavas olmali — Balancer `startValue/endValue/startTime/endTime` kullanir).

---

## 25. balancer_v2/pools/composable_stable.rs (144 satir)

### Amaç
ComposableStable: BPT (Balancer Pool Token) dahil edilmis stable pool. Rate provider'larla dinamik scaling.

### Kritik Fark: Dynamic Scaling Factors
```rust
// Common scaling: decimals-based, sabit
// ComposableStable: getScalingFactors() → on-chain rate providers
fetch_pool_state():
    paralel:
        common_state
        pool.getScalingFactors()  // rate provider'dan yield orani dahil
        pool.getAmplificationParameter()
    // scaling_factors'i common'dan degil, on-chain'den al (override)
```

### BPT Exclusion (swap_mod.rs'de)
ComposableStable havuzu kendi BPT token'ini da kayitli token olarak içerir. Swap hesaplamasi için BPT çikarilir (sadece underlying asset'ler kullanilir).

### Güvenlik Analizi
- Rate provider manipulation: `getScalingFactors()` manipüle edilirse yanlis swap orani. Oracle risk kategorisi.
- BPT exclusion zorunlu: BPT dahil edilirse StableSwap invariant bozulur.

### DEX Uygulama Notu
Nova Shield yield-bearing token pool'u için: rate provider oracle pattern, TWAP veya circuit breaker ile korunmali.

---

## 26. balancer_v2/pools/liquidity_bootstrapping.rs (150 satir)

### Amaç
LBP (Liquidity Bootstrapping Pool): token satisi için dinamik agirlikli pool. `swap_enabled` flag ile durdurulabilir.

### Kritik Fark: Her Blok Dinamik Agirlik
```rust
fetch_pool_state():
    paralel:
        common_state
        pool.getNormalizedWeights()   // her blok sorgulanir!
        pool.getSwapEnabled()         // false ise None dön

    if !swap_enabled: return None  // pool deaktif
```

Weighted pool: `specialize_pool_info()`'da bir kez `getNormalizedWeights()` cagrisi yapilir.
LBP: her `fetch_pool_state()`'te cagirilir — agirliklar zamanla degisir.

### LBP Kullanim Senaryosu
- Yeni token satisi: baslangic %99 yeni token / %1 USDC
- Zamanla: %50/%50'ye dogru lineer geçis
- Arbitragörlerin swap'i fiyati dengeler
- `swap_enabled = false`: satis bittiginde durdur

### DEX Uygulama Notu
Nova Shield native token ilk dagitimi için LBP pattern faydali. `swap_enabled` governance call ile kontrol edilebilir.

---

## 27. balancer_v2/swap/mod.rs (688 satir)

### Amaç
Tüm Balancer pool tipleri için `BaselineSolvable` implementasyonu: upscale → AMM math → downscale akisi.

### Sabitler
```rust
WEIGHTED_SWAP_GAS_COST: usize = 100_000
STABLE_SWAP_GAS_COST: usize = 183_520  // StableSwap daha pahali (Newton-Raphson)
```

### Kritik Fonksiyonlar

#### Fee Aritmetigi
```rust
// Swap'a fee EKLEME (amount_in hesabi için):
add_swap_fee_amount(amount, fee) =
    Bfp::from_wei(amount).div_up(fee.complement())
    // complement = 1 - fee
    // Örnek: fee=0.003, complement=0.997
    // amount / 0.997 = amount * (1/0.997) ≈ amount * 1.003003...

// Fee ÇIKARMA (amount_out hesabi için):
subtract_swap_fee_amount(amount, fee) =
    amount - amount.mul_up(fee)
    // amount * (1 - fee)
```

#### Upscale / Downscale
```rust
TokenState::upscale(amount: U256) -> Result<Bfp>:
    Bfp::from_wei(amount).mul_down(scaling_factor)
    // token decimals → 18 decimal internal

TokenState::downscale_up(bfp: Bfp) -> U256:
    bfp.div_up(scaling_factor).into()
    // 18 decimal → token decimals (ceiling)

TokenState::downscale_down(bfp: Bfp) -> U256:
    bfp.div_down(scaling_factor).into()
    // 18 decimal → token decimals (floor)
```

#### WeightedPoolRef::get_amount_out_inner()
```
1. amount_in_minus_fee = subtract_swap_fee_amount(amount_in, swap_fee)
2. upscaled_in = token_in.upscale(amount_in_minus_fee)
3. upscaled_out = calc_out_given_in(balance_in, weight_in, balance_out, weight_out, upscaled_in)
4. result = token_out.downscale_down(upscaled_out)
```

#### WeightedPoolRef::get_amount_in()
```
1. upscaled_out = token_out.upscale(amount_out)
2. upscaled_in = calc_in_given_out(balance_in, weight_in, balance_out, weight_out, upscaled_out)
3. amount_before_fee = token_in.downscale_up(upscaled_in)
4. amount_with_fee = add_swap_fee_amount(amount_before_fee, swap_fee)
5. converge_in_amount(amount_with_fee, amount_out, ...)  // rounding correction
```

#### converge_in_amount() — Yuvarlama Düzeltmesi
```
// Hasta: downscale+upscale yuvarlama hatasiyla amount_out eksik olabilir
// Max 6 iterasyon, her seferinde fazla öde
for i in 0..6:
    simulated_out = get_amount_out_inner(current_amount_in)
    if simulated_out >= target_amount_out: break
    deficit = target_amount_out - simulated_out
    current_amount_in += estimated_extra * 10^i
```

#### StablePoolRef::reserves_without_bpt()
```
// ComposableStable: pool address = BPT token adresi
// BPT'yi token listesinden çikar
tokens.filter(|(addr, _)| addr != pool_address)
```

### Güvenlik Analizi
1. **`mul_up` vs `mul_down`**: fee hesabinda `mul_up` kullanilir → kullanicidan fazla fee alinamaz
2. **`div_up` downscale**: gelen miktarda kullanici aleyhine yuvarlama önlenir
3. **`converge_in_amount`**: EVM integer aritmetinin kaçinilmaz yuvarlama hatasini düzeltir
4. **BPT exclusion**: composable stable için zorunlu, aksi halde invariant hesabi yanlis

### DEX Uygulama Notu
Nova Shield AMM swap engine:
1. `upscale` → unified 18-decimal hesap → `downscale`: decimal agnostik AMM math
2. `fee.complement()`: `div_up(1-fee)` pattern — EVM'de native floating point yok, bu standart
3. `converge_in_amount`: integer AMM'de kaçinilmaz — solver'in bunu bilmesi gerekir

---

## 28. balancer_v2/swap/fixed_point.rs (498 satir)

### Amaç
Balancer Fixed Point: ufixed256x18 aritmetik — Solidity Balancer matematik kütüphanesinin Rust portu.

### Struct/Enum/Trait Listesi
- `Bfp(U256)`: 18 decimal fixed-point sayi
- `ONE_18: U256 = 10^18`

### Matematiksel Formüller

#### Temel Operasyonlar
```rust
// MUL DOWN (truncate — protokol lehine)
mul_down(a, b) = (a * b) / ONE_18

// MUL UP (ceiling — kullanici lehine)
mul_up(a, b):
    if a == 0 || b == 0: return 0
    else: (a * b - 1) / ONE_18 + 1

// DIV DOWN (truncate)
div_down(a, b) = (a * ONE_18) / b

// DIV UP (ceiling)
div_up(a, b):
    if a == 0: return 0
    else: (a * ONE_18 - 1) / b + 1

// COMPLEMENT
complement(a) = clamp(ONE_18 - a, 0, ONE_18)
```

#### pow_up(base, exp) -> Result<Bfp>
```
raw = logexpmath::pow(base, exp)
max_error = raw * MAX_POW_RELATIVE_ERROR / ONE_18 + 1
result = raw + max_error

MAX_POW_RELATIVE_ERROR = 10_000  // 1e-14 in 18-decimal = 0.00000000000001%
```

#### pow_up_v3(base, exp) — V3 Optimizasyon
```
// Special cases: integer multiplications
if exp == 1.0:  return base
if exp == 2.0:  return base * base / ONE_18  (mul_down)
if exp == 4.0:  return (base^2)^2
else:           pow_up(base, exp)  // fallback
```

#### exp10(n: i32) -> Bfp
```
10^(n+18) as Bfp  // -18 <= n <= 37 araligi
Örnek: exp10(0) = 10^18 = ONE_18 = 1.0
       exp10(-18) = 10^0 = 1 = 0.000...001 (1e-18)
```

### Güvenlik Analizi
1. **`mul_up` sifir check**: `0 * anything = 0`, özel case
2. **`div_up` sifir check**: `0/x = 0`, division by zero önlenir
3. **`MAX_POW_RELATIVE_ERROR = 10000`**: 10^-14 hata payi — EVM'de matematiksel güvenlik için yeterli
4. **`complement` clamp**: underflow önlenir

### DEX Uygulama Notu
Nova Shield için `Bfp` implementasyonu:
- `mul_down` / `div_down`: havuz lehine (LP'leri korur)
- `mul_up` / `div_up`: kullanici lehine (kullaniciyi korur)
- Hangi yuvarlama nerede? **input hesabinda div_up, output hesabinda div_down** — invariant direction

---

## 29. balancer_v2/swap/fixed_point/logexpmath.rs (579 satir)

### Amaç
Tam kayan nokta logaritma ve üs alma: `x^y` via `exp(y * ln(x))`, 18-decimal fixed-point.

### Matematiksel Formüller

#### pow(x, y) -> Result<U256>
```
// x^y = exp(y * ln(x))
1. ln_x = _ln_36(x)  // 36 decimal precision near 1.0
   OR _ln(x) * ONE_18  // 18 decimal elsewhere
2. logx_times_y = ln_x * y / ONE_36 (or ONE_18)
3. result = exp(logx_times_y)
```

#### _ln(a) -> i256 — Natural Logarithm
```
// Range reduction: a = a_constant_k * r, ln(a) = sum(ln(a_k)) + ln(r)
// Constants:
a_18[0] = e^128, a_18[1] = e^64  (18-decimal precision)
a_20[2..11] = e^32 .. e^0.5      (20-decimal precision)

// After reduction: r near 1.0
// Taylor series: ln((z+1)/(z-1)) = 2 * sum_{k=1,3,5,...} z^k/k
// where z = (r-1)/(r+1)
// Converges fast near r=1
```

#### exp(x) -> Result<U256> — Natural Exponent
```
// Range: x in [-41e18, 130e18]
// = [ln(~1e-18), ln(~2.7e56)]
// Range reduction: x = sum(x_k * ln(a_k)) + r
// result = prod(a_k^x_k) * e^r
// e^r Taylor series: 1 + r + r^2/2! + ... + r^12/12!
```

#### _ln_36(x) — High Precision Near 1.0
```
// x in (0.9e18, 1.1e18): use 36-decimal arithmetic
z = (x - ONE_36) / (x + ONE_36)
result = 2 * (z + z^3/3 + z^5/5 + ...)
```

### Sabitler
```
MILD_EXPONENT_BOUND = 2^254 / 10^20
// y > MILD_EXPONENT_BOUND → InvalidExponent error
```

### Güvenlik Analizi
1. **Sinir kontrolü**: `x <= 0` → XOutOfBounds; `y > MILD_EXPONENT_BOUND` → InvalidExponent
2. **exp siniri**: x < -41e18 veya x > 130e18 → YOutOfBounds
3. **36-decimal precision near 1.0**: küçük exp argümanlarinda daha iyi sonuç
4. **Taylor series terimleri**: 12 terim → e^r için r < 0.25 araligi makul precision

### DEX Uygulama Notu
Nova Shield AMM'de `x^y` üs alma gerekiyorsa (Weighted AMM):
- Bu logexpmath implementasyonu Balancer üretim kodundan alinmis, savas-kanitmis
- Alternatif: `num_traits::Pow` trait + BigDecimal, ama logexpmath daha hizli
- Rust portu: Solidity'deki signed 256-bit integer islemleri `i256` ile simüle edilmis

---

## 30. balancer_v2/swap/weighted_math.rs (345 satir)

### Amaç
Balancer Weighted Pool AMM matematigi: `calc_out_given_in` ve `calc_in_given_out`.

### Sabitler
```rust
MAX_IN_RATIO:  Bfp = 0.3 * 10^18 = 3 * 10^17  // %30 max input ratio
MAX_OUT_RATIO: Bfp = 0.3 * 10^18 = 3 * 10^17  // %30 max output ratio
```

### Matematiksel Formüller

#### calc_out_given_in(balance_in, weight_in, balance_out, weight_out, amount_in)

Kisitlama:
```
amount_in <= balance_in * MAX_IN_RATIO  // %30 siniri
```

Formül:
```
// Weighted invariant koruma:
// balance_in^w_in * balance_out^w_out =
//     (balance_in + amount_in)^w_in * balance_out_new^w_out

base     = balance_in / (balance_in + amount_in)
exponent = weight_in / weight_out
power    = base ^ exponent       // Bfp::pow_up(base, exponent)
result   = balance_out * (1 - power)
         = balance_out.mul_down(power.complement())
```

Türetme:
```
balance_out_new = balance_out * (balance_in / (balance_in + amount_in))^(w_in/w_out)
amount_out = balance_out - balance_out_new
           = balance_out * (1 - (balance_in/(balance_in+amount_in))^(w_in/w_out))
```

#### calc_in_given_out(balance_in, weight_in, balance_out, weight_out, amount_out)

Kisitlama:
```
amount_out <= balance_out * MAX_OUT_RATIO  // %30 siniri
```

Formül:
```
base     = balance_out / (balance_out - amount_out)
exponent = weight_out / weight_in
power    = base ^ exponent
result   = balance_in * (power - 1)
         = balance_in.mul_up(power.sub(ONE))
```

Türetme:
```
balance_in_new = balance_in * (balance_out / (balance_out - amount_out))^(w_out/w_in)
amount_in = balance_in_new - balance_in
          = balance_in * ((balance_out/(balance_out-amount_out))^(w_out/w_in) - 1)
```

#### V3Plus Varyantlari
```rust
calc_out_given_in_v3(...):
    // Ayni formül ama pow_up yerine pow_up_v3 kullan
    // weight = 0.5 → sqrt, weight = 0.25 → quad root, vs.
```

#### balance_in + amount_in Overflow Analizi
```
// Rust'ta U256: 2^256 - 1 ~ 1.15 * 10^77
// Pratikte: balance_in < 2^112 (POOL_MAX_RESERVES)
//           amount_in <= 0.3 * balance_in
// Toplam max: 1.3 * 2^112 < 2^113 << 2^256 → overflow yok
```

### Güvenlik Analizi
1. **MAX_IN_RATIO = MAX_OUT_RATIO = 30%**: büyük swap'larda asiri slippage önlenir, pool liquidity korunur
2. **`mul_down` (out için)**: out miktarini küçük yuvarla → invariant düsmez
3. **`mul_up` (in için)**: in miktarini büyük yuvarla → invariant düsmez
4. **`pow_up`**: üs alma hata payi yukari yönlü → in miktari kesinlikle yeterli

### DEX Uygulama Notu
Nova Shield Weighted AMM için bu formüller direkt kullanilabilir:
- 50/50 pool: `weight_in = weight_out = 0.5` → `base^1 = base` → lineer yaklasim (x*y=k ile esdeger!)
- 80/20 pool: platform token/stablecoin — büyük impermanent loss ama yüksek fee geliri
- MAX_IN/OUT_RATIO: governance parametresi yapilabilir

---

## 31. balancer_v2/swap/stable_math.rs (520 satir)

### Amaç
Balancer Stable Pool AMM matematigi: StableSwap (Curve benzeri) Newton-Raphson iterasyonu.

### Sabitler
```rust
AMP_PRECISION: U256 = 1000
```

### Matematiksel Formüller

#### StableSwap Invariant D
```
// n token için:
A * n^n * sum(b_i) + D = A * D * n^n + D^(n+1) / (n^n * prod(b_i))

// n=2, token0 ve token1:
A * 4 * (b0 + b1) + D = 4 * A * D + D^3 / (4 * b0 * b1)

// A → sonsuz: stablecoin AMM (1:1 swap)
// A = 0: Uniswap x*y=k
// Pratikte A = 50-2000 tipik degerler
```

#### calculate_invariant(amp, balances) -> Result<U256>
```rust
// Newton-Raphson, max 255 iterasyon:

S = sum(balances)  // baslangic: D = S
D = S

loop (255 kez):
    D_P = D
    for b in balances:
        D_P = D_P * D / (n * b)
    // D_P = D^(n+1) / (n^n * prod(b_i))

    D_prev = D
    // Newton-Raphson güncelleme:
    D = (amp * n^n * S + D_P * n) * D /
        ((amp * n^n - 1) * D + (n+1) * D_P)

    if |D - D_prev| <= 1: return Ok(D)

Err(StableInvariantDidntConverge)
```

#### calc_out_given_in(amp, balances, idx_in, idx_out, amount_in)
```
1. balances[idx_in] += amount_in  // yeni balance ekle
2. D = calculate_invariant(amp, balances)
3. balance_out_new = get_token_balance_given_invariant(amp, balances, D, idx_out)
   // newton-raphson: yeni balance_out'u bul
4. amount_out = balances[idx_out] - balance_out_new - 1
   // -1: rounding safety (kullanici her zaman biraz eksik alir)
```

#### get_token_balance_given_invariant_and_all_other_balances()
```
// Newton-Raphson for single token balance:
c = D^(n+1) / (A * n^n * prod(other_balances))
b = sum(other_balances) + D / (A * n^n) - D

loop:
    y_prev = y
    y = (y^2 + c) / (2*y + b)
    if |y - y_prev| <= 1: return Ok(y)
```

### Güvenlik Analizi
1. **Max 255 iterasyon**: sonsuz döngü yok; non-converge → error
2. **-1 wei**: output'ta her zaman 1 wei eksik → invariant kesinlikle bozulmaz
3. **A=0 korumasi**: `amp * n^n = 0` → division by zero. Caller kontrolü yapmali.
4. **Convergence criteria**: `|D - D_prev| <= 1` — integer Newton-Raphson standart
5. **AMP_PRECISION**: on-chain amp = `A * AMP_PRECISION` → daha smooth geçisler

### DEX Uygulama Notu
Nova Shield stablecoin pool için StableSwap:
- A = 100-500: USDC/USDT/DAI için iyi baslangic
- A degisimi: on-chain yavas geçis (startTime + endTime) — anlik degisim arbitraj riski
- Newton-Raphson: EVM'de 255 iterasyon yeterli; Rust'ta daha az (float point ile baslangic tahmini daha iyi)
- Invariant hesabi per-swap: gas maliyeti var ama dogruluk kritik

---

## 32. balancer_v2/swap/math.rs (120 satir)

### Amaç
Balancer checked U256 aritmetik: overflow ve underflow kontrolü.

### Struct/Enum/Trait Listesi
- `BalU256` trait: `bmul`, `badd`, `bsub`, `bdiv_down`, `bdiv_up` → `Result<U256, Error>`

### Matematiksel Formüller
```rust
bmul(a, b) = checked_mul(a, b).ok_or(MulOverflow)
badd(a, b) = checked_add(a, b).ok_or(AddOverflow)
bsub(a, b) = checked_sub(a, b).ok_or(SubOverflow)
bdiv_down(a, b) = a / b  (b == 0 → ZeroDivision)
bdiv_up(a, b):
    if a == 0: return 0
    (a - 1) / b + 1  (ceiling)
```

### DEX Uygulama Notu
Tüm AMM matematik fonksiyonlari checked aritmetik kullanmali. `checked_*` metodlari Rust standard; Solidity'de `SafeMath` esdegeri.

---

## 33. balancer_v2/swap/error.rs (72 satir)

### Amaç
Balancer hata kodlari: Solidity BAL#NNN formati Rust'a tasimis.

### Hata Kodlari
```
0:   AddOverflow
1:   SubOverflow
3:   MulOverflow
4:   ZeroDivision
5:   DivInternal
6:   XOutOfBounds        (logexpmath: x sinir disi)
7:   YOutOfBounds        (logexpmath: y sinir disi)
8:   ProductOutOfBounds
9:   InvalidExponent
304: MaxInRatio          (weighted: amount_in > 30% reserve)
305: MaxOutRatio         (weighted: amount_out > 30% reserve)
309: InvalidToken
321: StableInvariantDidntConverge
322: StableGetBalanceDidntConverge
```

Format: `"BAL#NNN: VariantName"` — Solidity event monitor'lariyla uyumlu.

### DEX Uygulama Notu
Nova Shield AMM hatalari da numarali olmali: on-chain event'lerde decode edilebilir ve monitoring tool'lariyla uyumlu.

---

## 34. balancer_v2/pool_fetching/mod.rs (470 satir)

### Amaç
Balancer pool fetcher: tüm factory tipleri için Registry, deny list, pool_id → pool_address mapping.

### Struct/Enum/Trait Listesi
- `BalancerPoolFetcher { inner: Arc<dyn InternalPoolFetching>, pool_id_deny_list: Vec<B256> }`
- `FetchedBalancerPools { stable_pools: Vec<StablePool>, weighted_pools: Vec<WeightedPool> }`
- `WeightedPool { common: PoolCommon, reserves: BTreeMap<Address, TokenState>, version: Version }`
- `StablePool { common: PoolCommon, reserves: BTreeMap<Address, TokenState>, amplification_parameter: AmplificationParameter }`

### Balancer Factory Varyantlari
```
WeightedV0, WeightedV3, WeightedV4, Weighted2Token
StableV2
LiquidityBootstrappingV1, LiquidityBootstrappingNoProtocolFeeV1
ComposableStableV1, ComposableStableV3, ComposableStableV4, ComposableStableV5, ComposableStableV6
```

### Kritik Fonksiyonlar

#### pool_address_from_id(pool_id: B256) -> Address
```rust
// Balancer pool_id: 32 byte B256
// Ilk 20 byte = pool contract adresi
Address::from_slice(&pool_id.as_slice()[..20])
```

#### create_aggregate_pool_fetcher()
```
1. subgraph.get_pools() → tüm pool listesi
2. group_by_factory() → HashMap<factory, pools>
3. Her factory için Registry olustur
4. AggregatePoolFetcher: tüm Registry'lerin birlesimi
```

#### Deny List Uygulamasi
```rust
// fetch() cagrisiinda:
ids.retain(|id| !self.pool_id_deny_list.contains(id));
// Deny list: bakiye senkronizasyon sorunu olan pool'lar
```

### Güvenlik Analizi
1. **Pool ID deny list**: yanlis bakiye raporlayan pool'lar manuel dislanir. Recovery mekanizmasi yok (statik list).
2. **`pool_address_from_id`**: Balancer'in pool_id konvansiyonu — sadece Balancer'a özgü.
3. **Registry per factory**: her factory tipi kendi logic'ini çalistirir, tip güvenligi.

### DEX Uygulama Notu
Nova Shield multi-pool-type fetcher için:
- Factory registry pattern: her pool tipi kendi fetcher'i → yeni pool tipi = yeni impl, var olan etkilenmez
- Deny list: governance vote ile pool exclusion
- Pool ID = address + entropy: Balancer konvansiyonu iyi; Nova Shield kendi pool ID semasini tanimlamali

---

## GENEL MIMARI ANALIZI

### Katmanli Tasarim
```
BaselineSolvable trait
    ↓ implements
UniV2Pool / UniV3PoolInfo / WeightedPoolRef / StablePoolRef
    ↓ kullanir
AMM Math (CPAMM, logexpmath, weighted_math, stable_math)
    ↓ veri
Pool state (RecentBlockCache / EventHandler / SubgraphClient / PoolIndexerClient)
    ↓ kaynaklar
UniswapV2 / UniswapV3 / Balancer subgraph or pool-indexer
```

### DEX Uygulama Özeti (Nova Shield için)

#### 1. AMM Matematik Seçimi
| Pool Tipi | Formül | Kullanim Senaryosu |
|-----------|--------|-------------------|
| CPAMM (UniV2) | x*y=k | Genel token çiftleri |
| Weighted | x^w1 * y^w2 = k | Asimetrik agirlikli portfolyo |
| StableSwap | Newton-Raphson | Stablecoin çiftleri |
| Concentrated (UniV3) | tick bazli | Aktif likidite yönetimi |

#### 2. Fee Yapisi
- UniV2: sabit 0.3% (Ratio<u32>)
- Swapr: dinamik, her blok RPC
- Balancer: Bfp, 18-decimal, swap_fee per pool
- UniV3: fee tier konfigürasyonu (500/3000/10000 bp)

#### 3. State Yönetimi
- UniV2: basit reserve okuma, her blok
- UniV3: checkpoint + event overlay, reorg-safe
- Balancer: subgraph init + RPC state fetch per block

#### 4. Güvenlik Kritik Noktalar
1. **Elastic token korumasi**: balance vs reserve check
2. **MAX_IN/OUT_RATIO**: büyük swap siniri
3. **Reorg handling**: `MAX_REORG_BLOCK_COUNT` blok geride checkpoint
4. **Ceiling division**: amount_in her zaman yukari yuvarlama
5. **Decimal scaling**: her token 18-decimal'e normalize et
6. **Deny list**: yanlis state raporlayan pool'lar manuel exclusion

#### 5. Gas Maliyet Benchmark'lari
```
UniV2 swap:          60,000 gas
UniV3 swap:         108,163 gas  (Dune median)
Balancer Weighted:  100,000 gas
Balancer Stable:    183,520 gas  (Newton-Raphson)
```

---

## SONUÇ

CoW Protocol'ün `liquidity-sources/src/` modülü, birden fazla AMM türünü tek bir `BaselineSolvable` arayüzü altinda birlestiren kapsamli bir likidite abstraction katmanidir.

**Nova Shield DEX için kritik çikarimlar:**
1. Bfp (ufixed256x18) aritmetik: 18-decimal fixed-point tüm AMM hesaplamalari için standart
2. `mul_down` / `div_up` yönü: invariantin kim lehine bozulacagini belirler
3. Checkpoint + event overlay: verimli blok bazli state takibi
4. Request deduplication + mutex-free I/O: yüksek throughput cache mimarisi
5. StableSwap Newton-Raphson: stablecoin pool için kaçinilmaz ama pahali (183k gas)
6. CREATE2 deterministik adres: factory contract gerektirmez
7. Elastic token korumasi: rebase/yield token destegi için zorunlu
8. `converge_in_amount`: integer AMM'lerde yuvarlama hatasi düzeltmesi standart uygulama olmali
