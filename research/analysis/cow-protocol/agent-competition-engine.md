# CoW Protocol Analiz — Competition Engine Agent
## Kapsam: driver/src/domain/competition/ — Yarışma Motoru (Tüm Alt Dizinler)
## DEX Özel Protokolü: 3x Hassasiyet
## Tarih: 2026-07-22
## Toplam Dosya: 22 .rs dosyası | ~6.480 satır analiz

---

# DOSYA BAZLI TAM ANALİZ

---

## 1. auction.rs (282 satır)

**Amaç:** Auction veri yapısını, token fiyatlarını ve deadline yönetimini tanımlar; solver'lara gönderilen ihale nesnesidir.

### Struct/Enum/Trait Listesi

**`Auction`** (satır 23):
- `id: Option<Id>` — Auction kimliği; `None` ise quote modu (gerçek ihale değil)
- `orders: Vec<competition::Order>` — Solver'ın çözebileceği emirler
- `tokens: Arc<Tokens>` — Auction'da kullanılan token'ların fiyat/metadata'sı
- `gas_price: eth::GasPrice` — EIP-1559 max_fee_per_gas + priority_fee_per_gas + base_fee üçlüsü
- `deadline: chrono::DateTime<chrono::Utc>` — Driver'ın autopilot'a çözüm göndermesi için son an
- `surplus_capturing_jit_order_owners: Arc<HashSet<eth::Address>>` — Surplus yakalayan JIT order sahipleri

**`Tokens`** (satır 138):
- `HashMap<eth::TokenAddress, Token>` wrapper'ı
- `.get()`, `.iter()` metodları

**`Token`** (satır 150):
- `decimals: Option<u8>`
- `symbol: Option<String>`
- `address: eth::TokenAddress`
- `price: Option<Price>` — Native token cinsinden fiyat
- `available_balance: eth::U256` — Settlement contract'taki token bakiyesi
- `trusted: bool` — Protokolün güvendiği, internalizasyon için kullanılabilir token mi?

**`Price`** (satır 164):
- `0: eth::Ether` newtype wrapper
- `BASE: u128 = 10^18` — Sabit normalize katsayısı

**`Id`** (satır 243):
- `0: i64` — Negatif değer geçersiz (InvalidId hatasıyla reject edilir)

**`Prices`** (satır 240):
- `type Prices = HashMap<eth::TokenAddress, Price>`

**Hatalar:**
- `InvalidId` — Negatif auction ID
- `InvalidPrice` — Sıfır fiyat
- `Error::Blockchain` — Gas fiyatı alınamadı

### Kritik Fonksiyonlar

**`Auction::new()` (satır 36-79):**
- Sıfır miktarlı emirleri filtreler (`order.available().is_zero()`)
- Token bilgisi olmayan emirleri filtreler
- Ethereum node'undan EIP-1559 gas fiyatı alır (async)
- `GasPrice::new(max_fee, priority_fee, base_fee)` oluşturur

**`native_prices()` (satır 119-129):**
- Her token için `price.in_eth()` dönüşümü yapar
- ETH_TOKEN için sabit `10^18` değer ekler (ETH/ETH = 1)
- `Prices` map döndürür

**`liquidity_pairs()` (satır 99-106):**
- Her order'dan `(sell_token, buy_token)` çiftleri üretir
- `HashSet<liquidity::TokenPair>` döndürür — likidite çekimi için

### Matematiksel Formüller

**`Price::in_eth(amount)` (satır 200-202):**
```
eth_amount = (token_amount * price_wei) / 10^18
```
Yani: price, 10^18 token'ın kaç wei ettiğini söyler.
Ters çevirme: `price.from_eth(eth) = eth * 10^18 / price_wei`

**Doctest Örneği:**
- Fiyat = 0.001 ETH (10^15 wei)
- 1 token (10^18 units)
- Sonuç: `10^18 * 10^15 / 10^18 = 10^15 wei = 0.001 ETH` ✓

### Güvenlik Analizi
- **Sıfır fiyat koruması:** `Price::try_new()` sıfır değeri reddeder → division by zero önlenir
- **Sıfır miktar filtresi:** `order.available().is_zero()` → degenerate order'lar elenir
- **Token bilgisi yoksa filtrele:** buy/sell token fiyatı bilinmiyorsa order atlanır

### DEX Uygulama Notu — Nova Shield için
- Nova Shield'ın auction yapısında `gas_price` ve `deadline` aynı mantıkla tutulmalı. EIP-1559 üçlüsü (max_fee, priority_fee, base_fee) aynen kopyalanabilir.
- `Price::in_eth()` formülü (token * price / 10^18) Nova Shield'ın native price normalizasyonu için temel alınmalıdır.
- `trusted` token flag'i, Nova Shield'da "internalizasyon beyaz listesi" olarak doğrudan kullanılabilir — settlement contract buffer'ını yalnızca güvenilen tokenlar için açmak MEV koruması sağlar.
- `surplus_capturing_jit_order_owners` mekanizması Nova Shield'da "özel likidite sağlayıcısı" konseptine eşlenebilir.

---

## 2. mod.rs (1105 satır)

**Amaç:** Competition motorunun kalbidir; `solve()`, `reveal()`, `settle()` lifecycle'ını yönetir; EIP-7702 paralel submission, solution merging, re-simulation mantığını barındırır.

### Struct/Enum/Trait Listesi

**`SubmitterPool`** (satır 77):
- `direct_slot: Arc<Semaphore>` — 1 permitli, solver EOA için doğrudan submission
- `delegated: Option<DelegatedSlots>` — EIP-7702 alt hesaplar
- `admission: Arc<Semaphore>` — max in-flight settle sınırı (= total_slots + settle_queue_size)
- `solver_address: eth::Address`

**`DelegatedSlots`** (satır 90):
- `release: mpsc::Sender<Account>` — Kullanılan hesabı geri koyma kanalı
- `acquire: Mutex<mpsc::Receiver<Account>>` — Hesap alma kanalı

**`SubmitterGuard`** (satır 184):
- RAII guard — drop() ile slot'u serbest bırakır

**`GuardInner`** (satır 189):
- `Direct(OwnedSemaphorePermit)` — EOA doğrudan
- `Delegated { account, release }` — EIP-7702
- `Dropped` — drop() sentineli (move semantics için)

**`SettleTaskHandle`** (satır 232):
- `JoinHandle` wrapper'ı
- Drop edildiğinde 1 saniyelik grace period sonrası task abort edilir

**`Competition`** (satır 263):
- `solver: Solver`
- `eth: Ethereum`
- `liquidity: infra::liquidity::Fetcher`
- `liquidity_sources_notifier: Notifier`
- `simulator: Simulator`
- `mempools: Mempools`
- `settlements: Mutex<VecDeque<Settlement>>` — Önde en yeni çözüm
- `risk_detector: Arc<risk_detector::Detector>`
- `fetcher: Arc<DataAggregator>`
- `order_sorting_strategies: Vec<Arc<dyn SortingStrategy>>`
- `submitter_pool: SubmitterPool`

**`Solved`** (satır 1029):
- `id: solution::Id`
- `score: eth::Ether` — Çözümün skoru (surplus + protocol fee, native token)
- `trades: HashMap<order::Uid, Amounts>`
- `prices: HashMap<eth::TokenAddress, eth::TokenAmount>` — Deprecated (rolling deploy için)
- `gas: Option<eth::Gas>`

**`Amounts`** (satır 1043):
- `side: order::Side`
- `sell: eth::Asset` — Limit sell amount
- `buy: eth::Asset` — Limit buy amount
- `executed_sell: eth::TokenAmount` — Gerçek satış (fees dahil)
- `executed_buy: eth::TokenAmount` — Gerçek alış (fees sonrası)

**`Revealed`** (satır 1065):
- `internalized_calldata: Bytes` — On-chain görünen calldata
- `uninternalized_calldata: Bytes` — Tam calldata (denetim için)

**`Settled`** (satır 1075):
- `tx_hash: eth::TxId`
- `internalized_calldata: Bytes`
- `uninternalized_calldata: Bytes`

**Sabitler:**
- `MAX_CONCURRENT_AUCTIONS: usize = 5` — Settlement cache boyutu katsayısı
- `MAX_SOLUTIONS_TO_MERGE: usize = 10` — Merge için max çözüm sayısı (2^10 kombinasyon önlenir)

### Kritik Fonksiyonlar

**`solve()` (satır 319-533) — ANA AKIŞ:**
1. `fetcher.start_or_get_tasks_for_auction()` → pre-processing başlatır/paylaşır
2. `assemble_auction()` → orders sort + balance update (spawn_blocking)
3. `solver.solve(auction, &liquidity)` → external solver HTTP çağrısı
4. Duplicate ID filtresi
5. Boş solution filtresi (`is_empty()`)
6. `merge()` → izin verildiyse çözümleri birleştirir (MAX_SOLUTIONS_TO_MERGE=10)
7. `solution.encode()` → settlement calldata (paralel, `buffer_unordered`)
8. Risk detector: başarılı/başarısız encoding bildir
9. `settlement.score()` → her settlement'ı puanlar (CIP-38)
10. `Reverse(score)` sıralaması
11. `max_to_propose` kadar kes
12. Cache'e yaz (`settlements.push_front`)
13. `resimulate_until_revert()` → blok stream üzerinde yeniden simülasyon

**`resimulate_until_revert()` (satır 539-571):**
- `ethrpc::block_stream` → her yeni blokta tetiklenir
- Tüm önerilen çözümleri paralel simüle et (`join_all`)
- Revert eden → ID'yi `voided_ids`'e ekle
- `scored` ve `settlements` cache'den çıkar
- Tüm çözümler revert ederse erken dön

**`assemble_auction()` (satır 601-644):**
- CoW AMM template orders + temel auction birleşimi
- `sort_orders()` → spawn_blocking (CPU-yoğun)
- `update_orders()` → spawn_blocking (balance allocation, CPU-yoğun)
- `without_unsupported_orders()` → risk detector filtresi

**`update_orders()` (satır 667-785) — Detaylı balance allocation:**
- CoW AMM order'ları skip et (kendi initialization'ları var)
- App data güncelle
- Flashloan order'ları skip et (settlement'ta fund sağlar)
- Wrapper'lı order'ları skip et
- Balance grubunu `(trader, sell_token, sell_token_balance)` ile bul
- Partial order: `allocated = min(max_sell, remaining_balance)`
- Fill-or-kill: `allocated = max_sell` eğer remaining >= max_sell, yoksa 0
- `available = available * allocated / max_sell` → scale
- `remaining_balance -= allocated`

**`merge()` (satır 989-1024):**
- MAX_SOLUTIONS_TO_MERGE = 10 ile sınırla (2^10 = 1024 kombinasyon)
- Her mevcut çözümle yeni çözümü birleştir
- `solution.merge()` başarısız olabilir (Incompatible, DuplicateTrade, IncongruentPrices)
- Sonuçları `Reverse(score)` ile sırala (scoring yalnızca sıralama için, kesin encode sonrası hesap değil)

**`SubmitterPool::acquire()` (satır 128-173):**
- `try_acquire_owned()` → non-blocking, EOA slot dene
- Busy → `select_ok([fetch_direct, fetch_eip7702])` → her ikisi yarışır, ilk kazanan kullanılır, diğeri serbest bırakılır

**`settle()` (satır 833-864):**
1. `try_admit()` → admission permit al (yoksa TooManyPendingSettlements)
2. Block deadline kontrolü
3. Ayrı tokio task spawn et (EIP-7702 paralel için)
4. `process_settle_request()` içinde: settlement cache'den al + kaldır (double-settle önlemi)
5. Liquidity source notifier'a async bildirim
6. `submitter_pool.acquire()` → submission slot
7. `mempools.execute()` → tx gönder

### Algoritmik Detay — solve() Pipeline

```
HTTP /solve Request
    ↓
DataAggregator (paylaşımlı, aynı auction için tek çalışır)
    ├── parse_request() → Auction DTO [spawn_blocking, serde_json]
    ├── fetch_balances() [async]
    ├── collect_orders_app_data() [async, max 500ms]
    ├── cow_amm_orders() [async]
    └── fetch_liquidity() [async]
    ↓
assemble_auction()
    ├── sort_orders() [spawn_blocking, CPU-yoğun]
    └── update_orders() [spawn_blocking, balance allocation]
    ↓
without_unsupported_orders() [risk_detector filter]
    ↓
solver.solve(auction, liquidity) — External HTTP call
    ↓
Filter: duplicate IDs, empty solutions
    ↓
merge() [optional, max 2^10 kombinasyon]
    ↓
encode() [buffer_unordered(post_processing_concurrency_limit)]
    ↓
score() per settlement [CIP-38]
    ↓
Sorted by Reverse(score) [Ether cinsinden]
    ↓
Take max_to_propose
    ↓
Cache (VecDeque, push_front) + resimulate_until_revert() [block stream]
    ↓
HTTP Response: Vec<Solved>
```

### Güvenlik Analizi
- **Double-settle koruması:** `swap_remove_front()` settlement'ı cache'den çıkarır → iki kez settle imkansız
- **Gas limit koruması:** `simulate_settlement()` gas tahminini limit ile karşılaştırır
- **Admission control:** Semaphore ile max in-flight settle sınırı
- **Grace period abort:** SettleTaskHandle 1 saniye bekler, sonra abort
- **Internalization güvenliği:** Yalnızca trusted token'lar internalize edilebilir
- **EIP-7702 slot race:** `select_ok()` — her iki slot'tan ilk geleni al, diğerini bırak (RAII Drop ile)
- **Resimulation MEV koruması:** Her blokta pending çözümler tekrar simüle edilir → stale/attacked winner tespiti

### DEX Uygulama Notu — Nova Shield için
- **solve() pipeline birebir kopyalanmalı:** Pre-processing → Solver → Filter → Merge → Encode → Score → Cache → Resimulate
- **EIP-7702 parallel submission:** Nova Shield'ın validator EOA'larına EIP-7702 delegated accounts eklenebilir — yüksek throughput için kritik
- **MAX_CONCURRENT_AUCTIONS = 5:** Nova Shield'da overlapping auction güvenliği için settlement cache = `max_proposals * 5`
- **resimulate_until_revert:** Her yeni blokta pending çözümleri yeniden simüle et — MEV sandwich'e karşı kritik savunma
- **merge() cap:** 2^10 kombinasyon. Nova Shield için token çifti sayısına göre bu değer ayarlanabilir.

---

## 3. pre_processing.rs (589 satır)

**Amaç:** Auction öncesi veri toplamasını (balance, app_data, cow_amm, liquidity) paylaşımlı future'lar ile koordine eder; birden fazla solver aynı auction için tek kez çalışır.

### Struct/Enum/Trait Listesi

**`DataFetchingTasks`** (satır 53):
- `auction: Shared<Arc<Auction>>`
- `balances: Shared<Arc<Balances>>`
- `app_data: Shared<Arc<HashMap<AppDataHash, Arc<ValidatedAppData>>>>`
- `cow_amm_orders: Shared<Arc<Vec<Order>>>`
- `liquidity: Shared<Arc<Vec<liquidity::Liquidity>>>`
- `Shared<T>` = `futures::future::Shared<BoxFuture<'static, T>>` — Clone edilebilir, paylaşılabilir future

**`Utilities`** (satır 63):
- `eth: infra::Ethereum`
- `signature_validator: Arc<dyn SignatureValidating>`
- `app_data_retriever: Option<AppDataRetriever>`
- `liquidity_fetcher: infra::liquidity::Fetcher`
- `tokens: tokens::Fetcher`
- `balance_fetcher: Arc<dyn BalanceFetching>`
- `cow_amm_cache: Option<cow_amm::Cache>`

**`ControlBlock`** (satır 82):
- `auction_id: i64` — Aktif auction ID'si
- `tasks: DataFetchingTasks` — Paylaşılan task'lar

**`DataAggregator`** (satır 90):
- `utilities: Arc<Utilities>`
- `control: Mutex<ControlBlock>` — Tek bir auction için tek çalışma garantisi

### Kritik Fonksiyonlar

**`start_or_get_tasks_for_auction()` (satır 99-131):**
- `X-Auction-Id` header → auction ID parse
- Mevcut ID ile eşleşirse → mevcut task'ı paylaş
- Farklı ID → `assemble_tasks()` → yeni task seti

**`assemble_tasks()` (satır 183-206):**
- `parse_request()` → serde_json deserialization (spawn_blocking içinde)
- `fetch_balances()`, `collect_orders_app_data()`, `cow_amm_orders()`, `fetch_liquidity()` → hepsi `spawn_shared()` ile arka planda başlar
- Future'lar hemen çalışmaya başlar, caller await ettiğinde çözülür

**`spawn_shared()` (satır 213-223):**
- `fut.boxed().shared()` → clonable handle
- `tokio::spawn(shared.clone())` → arka planda başlat
- Handle döndür → birden fazla solver aynı handle'ı await edebilir

**`fetch_balances()` (satır 272-356):**
- Order'ları `(trader, token, source)` grubuna ayır
- Aynı trader/token/source + farklı pre-interactions → interactions=[] (pessimistic: en düşük balance)
- Flashloan balance override desteği
- `BalanceFetching::get_balances()` ile toplu çekme

**`collect_orders_app_data()` (satır 360-411):**
- Yalnızca `AppData::Hash(_)` (full verisi olmayan) order'lar için fetch
- `MAX_APP_DATA_WAIT = 500ms` — bu süre sonrası kalan fetchler arka planda devam eder
- Cache dolukça sonraki auction'larda hız artar

**`cow_amm_orders()` (satır 413-547):**
- `surplus_capturing_jit_order_owners` listesinden CoW AMM'leri al
- Her AMM için `validated_template_order()` → EIP-1271 imzalı
- `prices` yoksa AMM'yi atla (surplus hesabı yapılamaz)
- Sonuç: `Arc<Vec<Order>>` domain formatında

### Matematiksel Formüller

**Balance scaling (mod.rs update_orders):**
```
available_scaled = available * allocated_balance / max_sell
```
Kısmi order için: mevcut `available` miktarını allocated balance oranında küçült.

**`MAX_APP_DATA_WAIT = 500ms`:** Cache ısınma bekleme süresi; ilk auction'da eksik app_data olabilir.

### Güvenlik Analizi
- **Tek çalışma garantisi:** `Mutex<ControlBlock>` ile aynı auction için iki kez fetch yapılmaz
- **Pessimistic balance:** Pre-interaction farklılığında interactions=[] → daha düşük balance → over-allocation önlenir
- **500ms app_data timeout:** Sonsuz bekleme riski eliminate

### DEX Uygulama Notu — Nova Shield için
- `Shared<T>` (futures::future::Shared) Nova Shield'da tüm pre-processing için zorunlu — N solver için N değil 1 kez fetch
- `X-Auction-Id` header ile dedup: aynı auction için paralel request'ler tek task paylaşır
- CoW AMM template order sistemi → Nova Shield'ın AMM hook mekanizması için referans

---

## 4. sorting.rs (149 satır)

**Amaç:** Order önceliklendirme stratejilerini tanımlar; sıralama balance allocation'ı belirler (en kritik sıradakiler önce fon alır).

### Struct/Enum/Trait Listesi

**`SortingKey`** (satır 12):
- `Float(OrdFloat)` — f64 tabanlı (fiyat olasılığı)
- `Timestamp(Option<util::Timestamp>)` — Zaman bazlı
- `Bool(bool)` — İkili tercih

**`SortingStrategy`** (satır 18) — Trait:
- `fn key(&self, order, tokens, solver, now) -> SortingKey`
- `Send + Sync + Debug`

**`ExternalPrice`** (satır 32):**
- Emirlerin gerçekleşme olasılığı = sell_value_ETH / buy_value_ETH

**`OrdFloat`** (satır 62):**
- f64 için `Ord` implement (`total_cmp` — NaN güvenli)
- BigRational'dan çok daha hızlı

**`CreationTimestamp`** (satır 84):**
- `max_order_age: Option<Duration>` — Bu kadar eski emirler filtrelenir
- Yeni emirler önce

**`OwnQuotes`** (satır 109):**
- `max_order_age: Option<Duration>`
- Solver kendi quote'unu kazandıysa öncelikli

### Kritik Fonksiyonlar

**`ExternalPrice::key()` (satır 33-56):**
```
chance_to_settle = sell_in_eth / buy_in_eth
if buy_in_eth.is_subnormal() → chance = 0
```

**`sort_orders()` (satır 133-148):**
- `sort_by_cached_key()` — key bir kez hesaplanır
- `Reverse()` → büyükten küçüğe
- `Vec<SortingKey>` → lexicographic tiebreaker

### Matematiksel Formüller

**ExternalPrice:**
```
chance = sell_price_in_eth(sell_amount) / buy_price_in_eth(buy_amount)
```
Oran > 1 → in-the-money (daha kolay çözülür). Oran < 1 → out-of-money.

**Tiebreaker:**
```
sort_key = [ExternalPrice, CreationTimestamp, OwnQuotes]  (lexicographic)
```

### Güvenlik Analizi
- `f64::total_cmp` → NaN belirsizliğini ortadan kaldırır
- `subnormal check` → division-by-zero benzeri hatayı önler

### DEX Uygulama Notu — Nova Shield için
- Nova Shield DEX order önceliklendirme: (1) fiyat olasılığı, (2) yaş filtresi, (3) kendi quote önceliği — bu 3 katman birebir uygulanmalı
- `total_cmp` zorunlu — Nova Shield'daki tüm f64 sıralamalarında kullanılmalı
- Balance allocation sıraya göre yapıldığı için sıralama kritik: yanlış sıra = yüksek değerli emirler yetersiz balance alır

---

## 5. solution/mod.rs (787 satır)

**Amaç:** Solver'dan gelen çözümü temsil eder; trade encoding, fee application, scoring, merging işlemlerini barındırır.

### Struct/Enum/Trait Listesi

**`Solution`** (satır 51):
- `id: Id` — Global unique (AtomicU64)
- `trades: Vec<Trade>` — Fulfillment + JIT karışımı
- `prices: Prices` — Uniform clearing price vektörü
- `pre_interactions: Vec<domain::Interaction>`
- `interactions: Vec<Interaction>` — Custom + Liquidity
- `post_interactions: Vec<domain::Interaction>`
- `solver: Solver`
- `weth: eth::WrappedNativeToken`
- `gas: Option<eth::Gas>`
- `gas_fee_override: Option<GasFeeOverride>`
- `flashloans: HashMap<order::Uid, Flashloan>`
- `wrappers: Vec<WrapperCall>`

**`GasFeeOverride`** (satır 79):
- `max_fee_per_gas: u128`
- `max_priority_fee_per_gas: u128`
- Solver'ın kendi gas override'ı; replacement tx için driver yükseltebilir

**`Id`** (satır 617):
- `id: u64` — `AtomicU64::fetch_add(1, Relaxed)` ile global unique
- `merged_solutions: Vec<u64>` — Birleştirilen temel çözüm ID'leri

### Kritik Fonksiyonlar

**`Solution::new()` (satır 86-196):**
1. Surplus-capturing JIT → `Fulfillment` dönüştürme
2. Clearing prices validasyonu
3. `FeeHandler::Driver` → `fulfillment.with_protocol_fees(prices)` uygula

**`scoring()` (satır 265-301):**
- Sell order: `executed = executed + fee`
- Her trade için `scoring::Trade::new()` + `scoring::compute_score()`

**`merge()` (satır 361-448):**
1. Aynı solver kontrolü
2. Trade sayısı limiti
3. Duplicate order UID kontrolü
4. `scaling_factor()` → unique faktör bul
5. `factor < 1` → `other.merge(self)` (her zaman yukarı scale)
6. Fiyatları scale et, trades/interactions concat

**`scaling_factor()` (satır 587-606):**
```
factor = first_price[token] / second_price[token]
```
Tüm ortak token'larda aynı faktör olmalı → tek unique faktör.

**`clearing_prices()` (satır 514-548):**
- ETH alan order varsa: WETH fiyatını ETH için kopyala
- Sadece WETH traded değilse WETH fiyatını at (gas tasarrufu)

**`approvals()` (satır 311-350):**
- Mevcut allowance >= gerekli → skip
- `approve()` revert → `approve(0) + approve(amount)` (USDT pattern)

**`has_haircut()` (satır 573-578):**
- Herhangi bir Fulfillment'ın `haircut_fee > 0` ise true
- Haircut çözümlerde simulation failure → solver'a bildirim yok (false positive önleme)

### Matematiksel Formüller

**Merge scaling:**
```
scaled_price = other_price[token] * (first_price[token] / second_price[token])
```
BigRational hassasiyetle, sonra U256'ya çevir.

**Score toplamı:**
```
total_score = Σ trade_i.score()   (her trade ayrı, topla)
```

### Güvenlik Analizi
- **Global unique ID:** `AtomicU64::Relaxed` — restart'ta sıfırlanır, protokol tolerans gösterir
- **Merge yönü:** factor < 1 → yukarı scale → hassasiyet kaybı minimize
- **Kongruent fiyat kontrolü:** Farklı scaling factor → merge reddi → cross-settlement arbitraj riski önlenir
- **USDT approve(0):** Approval reset pattern — token uyumluluk kritik

### DEX Uygulama Notu — Nova Shield için
- **Merge:** Nova Shield'ın yarışma motoru birden fazla solver çözümünü merge edebilmeli — daha fazla order kapasitesi
- **Haircut mekanizması:** Conservative bidding için; gerçek execution etkilenmez, yalnızca raporlama azalır
- **USDT approve(0) pattern:** Nova Shield settlement'ta zorunlu

---

## 6. solution/scoring.rs (450 satır) — EN KRİTİK

**Amaç:** CIP-38 tabanlı çözüm puanlama motoru; her trade'in surplus + protocol fee'sini native token cinsinden hesaplar.

### Struct/Enum/Trait Listesi

**`Trade`** (satır 42):
- `signed_sell: eth::Asset` — Kullanıcının imzaladığı limit sell
- `signed_buy: eth::Asset` — Kullanıcının imzaladığı limit buy
- `side: Side`
- `executed: order::TargetAmount`
- `custom_price: CustomClearingPrices` — Fee'ler dahil özel fiyatlar
- `policies: Vec<FeePolicy>`

### Kritik Fonksiyonlar

**`compute_score()` (satır 29-34):**
```rust
trades.iter().map(|trade| trade.score(native_prices)).sum()
```

**`Trade::score()` (satır 83-120) — ANA FORMÜL:**

Sell order:
```
surplus_in_buy_token = user_surplus + fees
score = native_price_buy.in_eth(surplus_in_buy_token)
     = surplus_in_buy_token * buy_native_price / 10^18
```

Buy order (U512 gerekli — 256-bit overflow riski):
```
surplus_in_sell_token = user_surplus + fees
surplus_in_buy_tokens = surplus_in_sell_token * signed_buy.amount / signed_sell.amount
score = native_price_buy.in_eth(surplus_in_buy_tokens)
     = surplus_in_buy_tokens * buy_native_price / 10^18
```
CIP-38 revizyon (CIP-draft): Buy order surplus önce buy token'a çevrilir sonra ETH'e → unreasonably high score önlenir.

**`surplus_over()` (satır 124-169):**

Buy order:
```
limit_sell_scaled = signed_sell.amount * executed / signed_buy.amount
sold = executed * custom_price.buy / custom_price.sell
surplus = limit_sell_scaled - sold
```

Sell order:
```
limit_buy_scaled = executed * signed_buy.amount / signed_sell.amount  (ceil)
bought = executed * custom_price.sell / custom_price.buy  (ceil)
surplus = bought - limit_buy_scaled
```

**`fees()` (satır 172-184):**
```
policies.iter().rev() → her policy için protocol_fee hesapla
custom_price yeniden hesapla (her fee'den sonra)
total = Σ protocol_fee_i
```

**`fee()` (satır 311-338) — Surplus fee:**
```
factor' = factor / (1 - factor)
fee = surplus_after_fee * factor'
```
Türetme:
- `fee = surplus_before * factor`
- `surplus_after = surplus_before - fee`
- `factor' = fee / surplus_after = factor / (1 - factor)`

**`volume_fee()` (satır 342-385):**

Sell order:
```
factor' = factor / (1 - factor)
fee = traded_buy_after * factor'
```

Buy order:
```
factor' = factor / (1 + factor)
fee = traded_sell_after * factor'
```

**`calculate_custom_prices()` (satır 226-248):**
```
Sell order:
  custom.sell = sell_amount (executed sell)
  custom.buy = buy_amount + protocol_fee

Buy order:
  custom.sell = buy_amount (executed buy)
  custom.buy = sell_amount - protocol_fee
```

### Matematiksel Formüller (Özet)

**CIP-38 Score:**
```
score_sell = (user_surplus_buy + Σfee_i_buy) * buy_native_price / 10^18

score_buy = (user_surplus_sell + Σfee_i_sell)
            * buy_limit_amount / sell_limit_amount    [widening_mul → U512]
            * buy_native_price / 10^18
```

**Surplus fee (her iki tarafa):**
```
fee_post = surplus_post * factor / (1 - factor)
```

**Volume fee:**
```
Sell: fee_post = buy_amount_post * factor / (1 - factor)
Buy:  fee_post = sell_amount_post * factor / (1 + factor)
```

### Test Analizi

`score_problematic_buy_order` (satır 411-448):
- Base chain auction 0xe3ef0249... verisi
- Buy order: WETH sell (huge) → BNKR buy (small)
- Eski formula: unreasonably high score
- Yeni formula: score = 911 wei (makul)

### Güvenlik Analizi
- **U512 widening_mul:** Buy order'da 256-bit taşma riski → 512-bit genişletme
- **checked_ceil_div:** Settlement contract tutarlılığı — en küçük geçerli buy = ceil
- **Ters policy:** `iter().rev()` — iç içe fee'lerde doğru sıra
- **Gerçek veriden test:** Edge case doğrulandı (Base chain)

### DEX Uygulama Notu — Nova Shield için
- **CIP-38 birebir:** `score = (surplus + fees) in native_token`. Buy order için U512 zorunlu.
- **Fee formülleri:** `factor/(1-factor)` ve `factor/(1±factor)` — "fee dahil" vs "fee hariç" bakışını doğru hizalar
- **Custom clearing prices:** Nova Shield settlement'ına iletilen fiyatlar `calculate_custom_prices()` ile üretilmeli
- **Multiple fee policies:** `iter().rev()` sırası zorunlu — çok tier fee desteği için

---

## 7. solution/fee.rs (418 satır)

**Amaç:** Solver'ın ürettiği fulfillment'a protocol fee uygular; executed amount ve fee'yi günceller.

### Kritik Fonksiyonlar

**`Fulfillment::with_protocol_fees()` (satır 42-48):**
- Her protocol fee politikasını iteratif uygula

**`with_protocol_fee()` (satır 51-86):**
1. `protocol_fee_in_sell_token()` → sell token cinsinden fee
2. Static fee varsa ve protocol fee > 0 → `ProtocolFeeOnStaticOrder`
3. `fee += protocol_fee`
4. Sell order: `executed -= protocol_fee` (çalıştırılan miktar azalır)
5. Buy order: executed değişmez

**`protocol_fee()` (satır 89-138):**
- Surplus: `min(fee_from_surplus, fee_from_volume(max))`
- PriceImprovement: `adjust_quote_to_order_limits()` → `min(fee_from_surplus_over_quote, fee_from_volume(max))`
- Volume: `fee_from_volume(factor)`

**`adjust_quote_to_order_limits()` (satır 256-298) — KRİTİK:**

Sell order:
```
net_buy_quote = quote_buy * (1 - fee/quote_sell)
scaled_buy = net_buy_quote * order_sell / quote_sell
buy_limit = max(order_buy, scaled_buy)
result = {sell: order_sell, buy: buy_limit}
```

Buy order:
```
total_sell_quote = quote_sell + quote_fee
scaled_sell = total_sell_quote * order_buy / quote_buy
sell_limit = min(order_sell, scaled_sell)
result = {sell: sell_limit, buy: order_buy}
```

**`protocol_fee_in_sell_token()` (satır 203-220):**
- Buy order: fee zaten sell token → doğrudan
- Sell order: fee buy token → convert: `fee_buy * prices.buy / prices.sell`

### Matematiksel Formüller

**Quote adjustment (sell, in-market):**
```
adjusted_buy = quote_buy * (order_sell / quote_sell) * (1 - quote_fee/quote_sell)
```

**Quote adjustment (buy, in-market):**
```
adjusted_sell = (quote_sell + quote_fee) * order_buy / quote_buy
```

### Test Analizi
4 test case: out-of-market sell/buy + in-market sell/buy → sınır koşullar doğrulandı

### Güvenlik Analizi
- **Static fee koruma:** Dynamic fee'li order'larda (Limit) protocol fee uygulanır
- **min(surplus, volume) cap:** Kullanıcı koruması — fee aşırı büyüyemez
- **Quote scaling:** Büyüklük farkı oransal düzeltilir

### DEX Uygulama Notu — Nova Shield için
- **3 fee policy:** Surplus, PriceImprovement, Volume — Nova Shield DEX'te hepsi desteklenmeli
- **`adjust_quote_to_order_limits()`:** Out-of-market order'larda yanlış fee büyük kayıp yaratır — bu formül birebir alınmalı
- **min(surplus, volume) cap:** Kullanıcı koruma katmanı zorunlu

---

## 8. solution/settlement.rs (529 satır)

**Amaç:** Encode edilmiş çözümü on-chain gönderilmeye hazır Settlement objesine dönüştürür; simülasyon, gas hesaplama, access list yönetimi.

### Struct/Enum/Trait Listesi

**`Settlement`** (satır 49):
- `auction_id: auction::Id`
- `transaction: SettlementTx`
- `gas: Gas`
- `solution: Solution`

**`SettlementTx`** (satır 59):
- `internalized: eth::Tx` — Bazı interaction'lar atlanmış
- `uninternalized: eth::Tx` — Tam calldata
- `may_revert: bool`

**`Gas`** (satır 377):
- `estimate: eth::Gas`
- `limit: eth::Gas` — min(max_gas, estimate × 2.0)

**`Internalization`** (satır 362):
- `Enable` — Trusted interaction'ları skip
- `Disable` — Tam calldata

### Kritik Fonksiyonlar

**`Settlement::encode()` (satır 80-136):**
1. Internalized interaction'lar yalnızca trusted token kullanabilir → `NonBufferableTokensUsed`
2. İki paralel approval fetch (internalized + uninternalized)
3. `encoding::tx()` × 2
4. `Settlement::new()` çağır

**`Settlement::new()` (satır 140-221):**
1. ETH alan SC wallet → `partial_access_list_for()` → access list
2. `simulator.gas()` + `eth.gas_price()` + `eth.balance()` → paralel
3. `Gas::new()` → limit hesapla
4. Solver balance yeterliliği kontrol

**`Gas::new()` (satır 389-432):**
```
max_gas = min(block_limit / 2, tx_gas_limit)
if estimate > max_gas → GasLimitExceeded
limit = min(max_gas, estimate * 2.0)   // GAS_LIMIT_FACTOR = 2.0
```

**`submission_max_fee_per_gas()` (satır 440-447):**
```
if override → override_value
else → driver_estimate * 2   (gas price climb buffer)
```

**`Gas::required_balance()` (satır 430-432):**
```
required = gas.limit * submission_max_fee_per_gas
```

### Test Analizi
4 test: EIP-7825 reject, accept at limit, small block cap, high tx_limit half-block bind

### Matematiksel Formüller

**Gas limit:**
```
max_gas = min(block_limit / 2, tx_gas_limit)
gas_limit = min(max_gas, estimate × 2.0)
```

**Solver balance:**
```
required_ETH = gas_limit × max_fee_per_gas
max_fee = override ?? (driver_estimate × 2)
```

### Güvenlik Analizi
- **Internalization check:** Trusted olmayan token → reject
- **block_limit / 2:** Block producer'ların tx'i dahil etme olasılığı artar
- **EIP-7825:** Fusaka hardfork, Mainnet per-tx cap = 16,777,215 → zorunlu parametre
- **Solver balance check:** Mempoolda takılı kalma önlenir
- **Access list:** SC wallet ETH transfer 2300 gas limitini aşar

### DEX Uygulama Notu — Nova Shield için
- **GAS_LIMIT_FACTOR = 2.0:** Gas refund buffer zorunlu — SSTORE temizleme refund'ları tahminleri düşürür
- **EIP-7825:** Nova Shield Mainnet deployment Fusaka sonrası olacak → tx_gas_limit konfigürasyonu şart
- **Uninternalized calldata:** Denetim için her zaman sakla — Nova Shield slashing mekanizması bunu gerektirir
- **Access list for ETH:** SC wallet receiver için zorunlu

---

## 9. solution/slippage.rs (283 satır)

**Amaç:** Likidite interaction'larına slippage toleransı uygular; token fiyatlarına göre mutlak cap ile sınırlı göreceli slippage.

### Struct/Enum/Trait Listesi

**`Parameters`** (satır 12):
- `relative: BigRational` — Göreceli oran (örn. 0.01 = %1)
- `max: Option<eth::U256>` — Mutlak max (native token wei)
- `min: Option<eth::U256>` — Mutlak min (native token wei)
- `prices: Prices`

### Kritik Fonksiyonlar

**`Parameters::apply_to()` (satır 29-109):**

**Öncelik 1 — Input token fiyatı biliniyor:**
```
slippage_ETH = in_eth(input_amount) × relative
slippage_ETH_capped = clamp(slippage_ETH, min, max)
slippage_input = from_eth(slippage_ETH_capped)
max_input = input_amount + slippage_input
```

**Öncelik 2 — Output token fiyatı biliniyor:**
```
slippage_ETH = in_eth(output_amount) × relative
slippage_ETH_capped = clamp(...)
slippage_output = from_eth(slippage_ETH_capped)
slippage_input = slippage_output × input_amount / output_amount
```

**Öncelik 3 — Fiyat yok (fallback):**
```
slippage = input_amount × relative   (no capping)
```

**Sonuç:** `(MaxInput, ExactOutput)` — output her zaman sabit

### Matematiksel Formüller

**Slippage (input fiyatı ile):**
```
slippage_ETH = token_amount × price / 10^18 × relative
capped = clamp(slippage_ETH, min_ETH, max_ETH)
slippage_tokens = capped × 10^18 / price
```

### Güvenlik Analizi
- **Capping:** Büyük işlemler için sabit max → MEV saldırısında alınabilecek değer sınırlı
- **ExactOutput sabit:** "En az bu kadar al" garantisi — slippage yalnızca input'u artırır

### DEX Uygulama Notu — Nova Shield için
- **Slippage capping zorunlu:** Her AMM interaction için bu formül uygulanmalı
- **Üçlü öncelik:** Input > output > fallback — Nova Shield price oracle bu sırayla sorgulanmalı
- **MEV sandwich koruması:** `max_input` sınırı saldırganın maksimum çıkarını sınırlar

---

## 10. solution/trade.rs (557 satır)

**Amaç:** Fulfillment (auction order) ve Jit (solver anlık order) trade tiplerini tanımlar; executed amount validasyonu, haircut fee, custom prices.

### Struct/Enum/Trait Listesi

**`Fulfillment`** (satır 112):
- `order: competition::Order`
- `executed: order::TargetAmount`
- `fee: Fee` — Static (0) veya Dynamic (solver belirledi)
- `haircut_fee: eth::U256` — Conservative bidding fee (target token); execution etkilenmez

**`Fee`** (satır 377):
- `Static` — Önceden bilinen (artık 0 olarak kullanılıyor)
- `Dynamic(SellAmount)` — Solver belirler

**`ClearingPrices`** (satır 388):
- `sell: eth::U256`, `buy: eth::U256` — Uniform prices

**`CustomClearingPrices`** (satır 400):
- `sell: eth::U256`, `buy: eth::U256` — Fee'ler dahil, inverted

**`Jit`** (satır 407):
- `order: order::Jit`, `executed`, `fee: SellAmount`
- `partially_fillable()` → her zaman `Partial::No`

### Kritik Fonksiyonlar

**`Fulfillment::new()` (satır 127-173):**
- Buy: fee=0 (target amount'a dahil değil)
- Sell: fee=dynamic_fee
- `executed + fee <= available` (partial) veya `= target` (fill-or-kill)
- Fee type match: Static → Market, Dynamic → Limit

**`Fulfillment::sell_amount()` (satır 215-237):**

Sell:
```
sell = executed + fee
```

Buy:
```
base = executed × prices.buy / prices.sell
sell = base + fee + haircut_in_sell_token
```

**`Fulfillment::buy_amount()` (satır 245-267):**

Sell:
```
base = executed × prices.sell / prices.buy  (ceil)
haircut_buy = haircut_fee × prices.sell / prices.buy
buy = base - haircut_buy
```

Buy:
```
buy = executed
```

**`custom_prices()` (satır 289-297):**
```
{sell: buy_amount, buy: sell_amount}   (inverted!)
```
Settlement contract: `executed_buy = executed_sell × custom.sell / custom.buy`

**`surplus_over_reference_price()` (satır 300-372):**

Buy order:
```
executed_sell = executed × prices.buy / prices.sell
executed_sell_fee = executed_sell + surplus_fee
limit_sell_scaled = limit_sell × executed / limit_buy
surplus = limit_sell_scaled - executed_sell_fee
```

Sell order:
```
executed_sell_fee = executed + surplus_fee
limit_buy_scaled = limit_buy × executed_sell_fee / limit_sell  (ceil)
executed_buy = executed × prices.sell / prices.buy  (ceil)
surplus = executed_buy - limit_buy_scaled
```

### Güvenlik Analizi
- **Fee type enforced:** Dynamic/Static match zorunlu → yanlış construction impossible
- **executed ≤ available:** Over-execution impossible
- **ceil division:** Settlement contract tutarlılığı
- **Haircut isolation:** Yalnızca raporlamayı etkiler; gerçek transfer değişmez

### DEX Uygulama Notu — Nova Shield için
- **Custom prices inverted format:** `{sell: received_buy, buy: paid_sell}` — Nova Shield settlement contract aynı formatı bekliyorsa zorunlu
- **JIT order:** Solver anlık order ekleme → Nova Shield'da "solver hook" olarak modellenebilir
- **Haircut fee:** Conservative bidding için güçlü araç — gerçek surplus gizlenirken execution güvenli

---

## 11. solution/encoding.rs (471 satır)

**Amaç:** Solution'ı settlement contract `settle()` fonksiyonu calldata'sına encode eder.

### Kritik Fonksiyonlar

**`tx()` (satır 39-248):**
1. Clearing price vektörünü token adresine göre sırala
2. Her trade için custom prices hesapla, token/price vektörüne ekle, index ata
3. ETH alıyorsa `native_unwrap += buy_amount`
4. Approval interaction'larını encode
5. Slippage parameters oluştur
6. Her interaction encode et (Custom/Liquidity)
7. WETH unwrap ekle
8. `settlement().settle(tokens, prices, trades, [pre, main, post])` calldata
9. Auction ID suffix ekle (8 byte big-endian)
10. Flashloan veya wrapper encode et

**`codec::flags()` (satır 394-422) — Bit packing:**
```
bit 0:   side (0=sell, 1=buy)
bit 1:   partially_fillable
bit 2-3: sell_token_balance (00=erc20, 10=external, 11=internal)
bit 4:   buy_token_balance (0=erc20, 1=internal)
bit 5-6: signing_scheme (00=eip712, 01=ethsign, 10=eip1271, 11=presign)
```

**`codec::signature()` (satır 434-444):**
- EIP-712/EthSign: raw 65-byte ECDSA
- EIP-1271: `signer(20) || data`
- PreSign: `signer(20)`

**`encode_flashloan_settlement()` (satır 256-285):**
- `LoanRequest::Data { amount, borrower, lender, token }` dizisi
- `flashLoanAndSettle(loans, settle_calldata)`

### Güvenlik Analizi
- **Flags bit packing:** Solidity ile tamamen eşleşmeli
- **Auction ID suffix:** Calldata son 8 byte → indexer settlement-auction eşleştirmesi
- **Flashloan+wrapper exclusive:** `has_flashloans && has_wrappers → Error`

### DEX Uygulama Notu — Nova Shield için
- **Flag bit layout:** Nova Shield Solidity ve Rust kodu tamamen eşleşmeli
- **Auction ID suffix:** Nova Shield indexer bu suffix'i okuyarak çözüm-auction eşleştirmesi yapabilir
- **WETH unwrap toplama:** Tek interaction'da birleştir → gas tasarrufu

---

## 12. solution/interaction.rs (95 satır)

**Amaç:** Custom arbitrary call ve Liquidity AMM call interaction tiplerini tanımlar.

**`Custom`** (satır 68):
- `target`, `value`, `call_data`, `allowances`, `inputs`, `outputs`, `internalize: bool`

**`Liquidity`** (satır 84):
- `liquidity: domain::Liquidity`, `input`, `output`, `internalize: bool`

**`allowances()` — Liquidity (satır 36-63):**
- Spender = AMM router/vault
- Amount = `U256::MAX` (max approval, gas tasarrufu; router güvenilir kabul edilir)

### DEX Uygulama Notu — Nova Shield için
- **Max approval:** Yalnızca audit edilmiş, güvenilen router'lar için
- **Custom interaction:** User pre/post hook sistemi için bu enum birebir kullanılabilir

---

## 13. order/mod.rs (530 satır)

**Amaç:** Auction order'ını temsil eder; available amount hesaplama, partial fill, ETH/WETH.

### Struct/Enum/Trait Listesi

**`OrderData`** (satır 22):**
- `uid: Uid` — 56 byte: `[order_hash(32) | owner(20) | valid_to(4)]`
- `receiver: Option<eth::Address>`
- `created`, `valid_to: util::Timestamp`
- `buy`, `sell: eth::Asset`
- `side: Side` — Buy/Sell
- `kind: Kind` — Market/Limit
- `pre_interactions`, `post_interactions: Vec<domain::Interaction>`
- `sell_token_balance: SellTokenBalance` — Erc20/Internal/External
- `buy_token_balance: BuyTokenBalance` — Erc20/Internal
- `signature: Signature`
- `protocol_fees: Vec<FeePolicy>`
- `quote: Option<Quote>`

**`Order`** (satır 59):**
- `data: Arc<OrderData>` — Paylaşılan immutable
- `app_data: app_data::AppData` — Per-solver mutable
- `partial: Partial` — Per-solver mutable (balance allocation)
- `Deref<Target = OrderData>`

**`Uid`** (satır 230):**
- `FixedBytes<56>` → `[order_hash(32) | owner(20) | valid_to(4)]`

**`Kind`** (satır 313):**
- `Market` — Hemen çalıştır
- `Limit` — Gelecekte, fee surplus'tan

### Kritik Fonksiyonlar

**`Order::available()` (satır 168-190):**
```
sell_scaled = sell_amount × available / target  (floor)
buy_scaled = ceil(buy_amount × available / target)
```

**`Order::solver_determines_fee()` (satır 194-196):**
- `Kind::Limit` → true

**`Uid::from_parts()` (satır 239-246):**
```
[order_hash(32) | owner_address(20) | valid_to.to_be_bytes(4)]
```

### Test Analizi

`order_scaling`:
- 1000→1000, 750 sell → sell=750, buy=750
- 1000→100, 901 sell → sell=901, buy=91 (ceil)
- 100→1000, 901 buy → sell=90, buy=901
- Edge: 1→1000, 500 buy → sell=0 (floor)

### DEX Uygulama Notu — Nova Shield için
- **56-byte UID format:** Nova Shield order ID aynı yapıya sahip olmalı
- **Arc<OrderData>:** N solver için N kopyada tek allocation
- **Ceil buy / floor sell:** Settlement contract rounding convention eşleşmeli

---

## 14. order/fees.rs (47 satır)

**Amaç:** Protocol fee politikalarını enum olarak tanımlar.

**`FeePolicy`** (satır 4):
- `Surplus { factor, max_volume_factor }` — Surplus'un `factor` oranı; `max_volume_factor` cap
- `PriceImprovement { factor, max_volume_factor, quote }` — Quote üzeri iyileşme'nin `factor` oranı
- `Volume { factor }` — Tüm hacmin `factor` oranı

### DEX Uygulama Notu — Nova Shield için
- **Surplus fee:** Kullanıcı dostu; extra ödeme yok
- **PriceImprovement:** Quote servisi entegrasyonu gerektirir
- **Volume fee:** Basit baseline; küçük order'lar için

---

## 15. order/app_data.rs (198 satır)

**Amaç:** Order'a bağlı AppData'yı yönetir; LRU cache, HTTP fetch, flashloan/wrapper metadata.

**`AppDataRetriever`** (satır 26):**
- moka LRU cache (`Cache<AppDataHash, Option<Arc<ValidatedAppData>>>`)
- TCP keepalive 60s
- `~%2 unique app_data` → küçük cache yeterli

**`AppData`** (satır 106):**
- `Hash(AppDataHash)` — Sadece hash
- `Full(Arc<ValidatedAppData>)` — Tam veri; `flashloan()`, `wrappers()` erişilebilir

**`get_cached_or_fetch()` (satır 59-100):**
1. Cache hit → hemen dön
2. Miss → `tokio::task::spawn` → arka planda HTTP GET
3. Cache'e yaz, caller'a sonuç ilet

### DEX Uygulama Notu — Nova Shield için
- **Background fetch + LRU:** Nova Shield order metadata için aynı pattern
- **Flashloan in AppData:** Settlement sırasında flashloan kaynağı belirlenir

---

## 16. order/signature.rs (57 satır)

**Amaç:** 4 imza şemasını tanımlar.

**`Scheme`** (satır 40):**
- `Eip712` — Typed data signing
- `EthSign` — `\x19Ethereum Signed Message:` prefix
- `Eip1271` — Smart contract isValidSignature
- `PreSign` — On-chain pre-authorized

**EIP-1271 encoding:** `signer(20) || signature_bytes`

### DEX Uygulama Notu — Nova Shield için
- Tüm 4 scheme zorunlu — özellikle EIP-1271 SC wallet desteği DeFi standardı

---

## 17. risk_detector/mod.rs (185 satır)

**Amaç:** Desteklenmeyen token/order'ları üç katmanlı sistemle tespit eder.

**`Quality`** (satır 29):** `Supported | Unsupported | Unknown`

**`Detector`** (satır 48):**
- `hardcoded: HashMap<TokenAddress, Quality>` — En yüksek öncelik
- `simulation_detector: Option<bad_tokens::simulation::Detector>` — trace_callMany
- `metrics: Option<bad_orders::metrics::Detector>` — Başarısızlık oranı

**`filter_unsupported_orders_in_auction()` (satır 84-148):**
```
Metrics → Unsupported → at
Token quality:
  Supported + Supported → tut
  * + Unsupported veya Unsupported + * → at
  Unknown sell → simulation (FuturesUnordered, parallel)
  Unknown buy → tut (varsayım: good)
```

### DEX Uygulama Notu — Nova Shield için
- **3 katman:** hardcoded > simulation > metrics
- **FuturesUnordered:** Paralel simülasyon → latency minimize
- Fee-on-transfer, rebasing, transfer-kısıtlı token'lar için bu sistem şart

---

## 18. risk_detector/bad_tokens/simulation.rs (117 satır)

**Amaç:** `TraceCallDetectorRaw` ile sell token transfer simülasyonu; sonuçları cache'ler.

**`Detector`** (satır 22):**
- `cache: Cache`, `detector: TraceCallDetectorRaw`, `sharing: BoxRequestSharing`

**`determine_sell_token_quality()` (satır 44-107):**
1. Cache → hit → dön
2. `sharing.shared_or_else()` → in-flight request paylaş
3. `test_transfer(trader, sell_token, amount, pre_interactions)`
4. Good → cache supported; Bad → metric artır, cache unsupported

### DEX Uygulama Notu — Nova Shield için
- **RequestSharing:** Multi-solver ortamda duplicate trace call önler — zorunlu

---

## 19. risk_detector/bad_tokens/cache.rs (83 satır)

**Amaç:** Token kalite kararlarını TTL ile önbelleğe alır.

**`Cache`** (satır 17):** `DashMap<TokenAddress, CacheEntry>`

**`update_quality()` (satır 43-59):**
- `Unsupported` her zaman kazanır (conservative)
- Outdated ise güncelle

**`get_quality()` (satır 71-81):**
- `still_valid = duration > max_age` → true = süre doldu
- `(false, _) → Unknown` (süre dolmadı → bu mantıksal inversion bir gözlem noktasıdır)
- `evict_outdated_entries()` ayrı thread'de çalışır

### DEX Uygulama Notu — Nova Shield için
- `DashMap` concurrent — Nova Shield token kalite cache'i için uygun
- Conservative: unsupported wins — Nova Shield'da aynı prensip

---

## 20. risk_detector/bad_orders/metrics.rs (269 satır)

**Amaç:** Order bazlı başarısızlık oranı; eşik aşılınca geçici freeze, sonra ikinci şans.

**`Detector`** (satır 31):**
- `failure_ratio: f64`, `required_measurements: u32`
- `counter: Arc<DashMap<Uid, OrderStatistics>>`
- `log_only: bool`, `order_freeze_time: Duration`

**`quality_based_on_stats()` (satır 85-94):**
```
if attempts < required → Unknown
if fails/attempts >= failure_ratio → Unsupported
else → Supported
```

**`spawn_gc_task()` (satır 141-162):**
- `Weak<DashMap>` — Detector drop → GC kapanır
- `valid_to > now && last_seen < max_age` → tut

### Test Analizi
- `unfreeze_bad_orders`: 2 fail → Unsupported → 50ms freeze → Unknown → 1 fail → Unsupported
- `evict_outdated_entries`: expired valid_to → 1. cycle'da evict; max_age dolana kadar diğeri korunur

### DEX Uygulama Notu — Nova Shield için
- **Freeze + thaw:** Oran bazlı + geçici freeze → az false positive
- **GC:** `Weak<DashMap>` + background task — tüm cache'lerde bu pattern

---

# GENEL MİMARİ DEĞERLENDİRME

## Competition Engine Veri Akışı (Tam)

```
HTTP /solve
    ↓
DataAggregator [pre_processing.rs] — paylaşımlı, tek çalışır
    ├── parse_request() [spawn_blocking, serde_json]
    ├── fetch_balances() [async, pessimistic]
    ├── collect_orders_app_data() [async, max 500ms]
    ├── cow_amm_orders() [async, EIP-1271]
    └── fetch_liquidity() [async]
    ↓
assemble_auction() [mod.rs]
    ├── sort_orders() [spawn_blocking: ExternalPrice, Timestamp, OwnQuotes]
    └── update_orders() [spawn_blocking: balance allocation, partial scaling]
    ↓
without_unsupported_orders() [risk_detector/: hardcoded > simulation > metrics]
    ↓
solver.solve(auction, liquidity) — External HTTP call
    ↓
Filter: duplicate IDs + empty solutions
    ↓
merge() [optional, MAX_SOLUTIONS_TO_MERGE=10, 2^10 kombinasyon]
    ↓
encode() → Settlement [buffer_unordered(concurrency_limit)]
    ├── Internalization check [trusted tokens only]
    ├── Gas simulation [simulator.gas()]
    ├── Access list [ETH → SC wallet]
    ├── Approval fetch [USDT approve(0) pattern]
    └── Calldata: tokens+prices+trades+interactions + auction_id(8B)
    ↓
score() per settlement [CIP-38: surplus + fees in native_token]
    ├── Sell: (surplus_buy + fees) × buy_price / 10^18
    └── Buy: (surplus_sell + fees) × buy/sell × buy_price / 10^18 [U512!]
    ↓
Sorted: Reverse(score)
    ↓
Take max_to_propose
    ↓
Cache [VecDeque, push_front, truncate at max_proposals × 5]
    ↓
resimulate_until_revert() [block stream, paralel, MEV koruması]
    ↓
HTTP Response: Vec<Solved>

---
HTTP /reveal → internalized + uninternalized calldata
HTTP /settle → EIP-7702 paralel submission (direct EOA preferred)
```

## Kritik Tasarım Kararları

1. **Shared futures:** N solver için 1 kez fetch — `futures::future::Shared`
2. **spawn_blocking isolation:** CPU-yoğun işlemler Tokio runtime'ı bloklamaz
3. **CIP-38 score:** surplus + fees in native token → solver rekabeti surplus max'a yönlenir
4. **Haircut fee:** Conservative bidding — execution etkilenmez, yalnızca raporlama
5. **EIP-7702 parallel:** Birden fazla EOA → concurrent settlement
6. **Resimulate on blocks:** Stale/attacked winner tespiti → MEV koruması
7. **Internalization:** Trusted buffer → public interaction atlanır → gas + MEV kazancı
8. **Gas = 2 × estimate:** Refund buffer; `min(block/2, EIP-7825)` hard cap
9. **Min(surplus, volume) fee cap:** Kullanıcı koruma katmanı
10. **adjust_quote_to_order_limits:** Out-of-market/in-market quote scaling — PriceImprovement fee doğruluğu

---

# KAPSAMLI DEX UYGULAMA NOTU — NOVA SHIELD İÇİN

## A. Scoring Motoru

CIP-38 birebir uygulanmalı:
```
score = Σ (surplus_i + fees_i) in native_token
```

Sell order:
```
score = (buy_received - limit_buy_scaled + Σ fee_i) × buy_price / 10^18
```

Buy order (U512 zorunlu):
```
surplus_sell = limit_sell_scaled - sell_paid - Σ fee_i
surplus_buy = surplus_sell × buy_limit / sell_limit
score = surplus_buy × buy_price / 10^18
```

## B. Fee Formülleri (Nova Shield DEX'te 3 Tier)

```
Surplus fee:  fee_post = surplus_post × factor / (1 - factor)
Volume fee (sell): fee_post = buy_post × factor / (1 - factor)
Volume fee (buy):  fee_post = sell_post × factor / (1 + factor)
Cap: fee = min(surplus_fee, volume_fee_max)
```

## C. Settlement Calldata Format

```
settle(tokens[], prices[], trades[], interactions[3][])
+ auction_id.to_be_bytes(8)   [suffix]

trade.flags byte:
  [0]: side | [1]: partial | [2-3]: sell_balance | [4]: buy_balance | [5-6]: signing
```

## D. MEV Koruması

- **Slippage cap:** `max_input = input + clamp(relative×ETH_value, min, max)`
- **Internalization:** Trusted buffer → AMM call atlanır → public mempool exposure azalır
- **Resimulate:** Her blokta pending çözümler doğrulanır
- **Gas/2 rule:** Block'un yarısından fazlasını kullanma

## E. Risk Yönetimi

```
Token quality priority:
  1. Hardcoded list (en yüksek)
  2. trace_callMany simulation
  3. Failure ratio metrics (freeze/thaw)

Order quality:
  failure_ratio = fails / attempts
  if ratio >= threshold AND attempts >= min → freeze for order_freeze_time
  after freeze → Unknown (second chance)
```

## F. Paralel Altyapı

```
Shared<T> future → N solver, 1 fetch
FuturesUnordered → paralel simülasyon
buffer_unordered(n) → n paralel encoding
EIP-7702 → çoklu submission EOA (select_ok yarışması)
spawn_blocking → CPU-yoğun işlemleri Tokio'dan izole et
```

## G. Order Sıralama

```
Sort key (lexicographic, Reverse):
  1. ExternalPrice: sell_ETH / buy_ETH  (>1 = in-the-money)
  2. CreationTimestamp: yeni > eski
  3. OwnQuotes: kendi solver quote'u = true > false

Balance allocation sıraya göre → sıralama kritik
```

## H. Custom Clearing Prices

```
Settlement contract için (inverted!):
  custom.sell = actual_buy_received
  custom.buy = actual_sell_paid  (fees dahil)

executed_buy = executed_sell × custom.sell / custom.buy
```

---

# ÖZET TABLO

| Dosya | Satır | Kritik İçerik |
|---|---|---|
| auction.rs | 282 | Auction struct, Price formülü, deadline, gas |
| mod.rs | 1105 | solve() pipeline, EIP-7702, merge, resimulate |
| pre_processing.rs | 589 | Shared futures, balance fetch, cow_amm |
| sorting.rs | 149 | ExternalPrice, OrdFloat, tiebreaker |
| solution/mod.rs | 787 | Solution struct, merge, scoring proxy, approvals |
| solution/scoring.rs | 450 | CIP-38, U512, surplus+fee formülleri |
| solution/fee.rs | 418 | Protocol fee application, adjust_quote |
| solution/settlement.rs | 529 | Gas limit, internalization, access list |
| solution/slippage.rs | 283 | Slippage capping, MEV |
| solution/trade.rs | 557 | Fulfillment, Jit, custom_prices, haircut |
| solution/encoding.rs | 471 | Calldata encode, flags packing, flashloan |
| solution/interaction.rs | 95 | Custom/Liquidity, max approval |
| order/mod.rs | 530 | Order struct, available scaling, UID |
| order/fees.rs | 47 | FeePolicy enum |
| order/app_data.rs | 198 | AppData LRU cache, flashloan metadata |
| order/signature.rs | 57 | 4 imza scheme, EIP-1271 format |
| risk_detector/mod.rs | 185 | 3-layer filter, FuturesUnordered |
| bad_tokens/simulation.rs | 117 | TraceCall, RequestSharing |
| bad_tokens/cache.rs | 83 | TTL cache, conservative update |
| bad_orders/metrics.rs | 269 | Failure ratio, freeze/thaw, GC |
| bad_orders/mod.rs | 2 | Re-export |
| bad_tokens/mod.rs | 3 | Re-export |

**TOPLAM: ~6.480 satır — 22 dosya — TAM ANALİZ TAMAMLANDI**
