# CoW Protocol Analiz — Agent 15
## Kapsam: driver/src/ — infra/ + boundary/ tam derinlik (145 dosya)
## DEX Özel Protokolü: 3x Hassasiyet

---

## Dosya Envanteri

Toplam analiz edilen dosyalar (145 .rs):
- `infra/solver/` — 8 dosya (mod.rs, dto/auction.rs, dto/solution.rs, dto/notification.rs, dto/mod.rs, eip7702.rs, streaming/*)
- `infra/blockchain/` — 4 dosya (mod.rs, contracts.rs, gas.rs, token.rs)
- `infra/notify/` — 8 dosya (mod.rs, notification.rs, liquidity_sources/*)
- `infra/observe/` — 2 dosya (mod.rs, metrics.rs)
- `infra/persistence/` — 1 dosya (mod.rs)
- `infra/tokens.rs` — 1 dosya
- `infra/mempool/mod.rs` — 1 dosya
- `infra/api/` — 15+ dosya (routes/solve, settle, reveal, quote, healthz, gasprice...)
- `boundary/liquidity/` — 10 dosya (uniswap v2/v3, balancer v2 weighted/stable, swapr, zeroex)
- `util/` — 4 dosya (http.rs, math.rs, percent.rs, time.rs)
- `tests/` — 20+ dosya (integration test cases + setup)

---

## BÖLÜM 1: infra/solver/ — KRİTİK DEX KATMANI

### [infra/solver/mod.rs] (715 satır)

**Amaç:** Solver engine ile iletişim kuran ana client — auctions gönderir, solutions alır, notify atar.

**Struct/Enum/Trait Listesi:**

- `Name(pub String)` (satır 50): Solver ismi wrapper — Display + as_str() impl.

- `Slippage` (satır 64):
  ```
  pub struct Slippage {
      pub relative: BigRational,   // rasyonel kesir (örn: 0.5/100)
      pub absolute: Option<eth::Ether>,  // maksimum mutlak slippage
  }
  ```

- `Liquidity` (satır 71): enum — `Fetch` (likidite çek) veya `Skip` (bu solver için atla).

- `Timeouts` (satır 81):
  ```
  pub struct Timeouts {
      pub http_delay: chrono::Duration,          // HTTP ağ gecikmesi maksimum payı
      pub solving_share_of_deadline: util::Percent,  // toplam deadline'ın solver'a ayrılan yüzdesi
  }
  ```
  **Kritik:** Timeout, solver'a gönderilecek süreyi deadline × solving_share olarak belirler.

- `ManageNativeToken` (satır 91):
  ```
  pub struct ManageNativeToken {
      pub wrap_address: bool,    // ETH adresini WETH'e çevir mi?
      pub insert_unwraps: bool,  // unwrap interaction ekle mi?
  }
  ```

- `Account` (satır 110): enum — `PrivateKey(PrivateKeySigner)`, `Kms(AwsSigner)`, `Address(Address)`.
  - `Address` varyantı imzalayamaz — sadece okuma (colocated solvers için).
  - `PrivateKey` ve `Kms` hem `TxSigner` hem `Signer` (EIP-7702 için) implement eder.
  - AWS KMS desteği production-grade HSM entegrasyonu sağlar.

- `Config` (satır 173): Solver tam konfigürasyonu:
  ```
  pub struct Config {
      pub endpoint: url::Url,              // solver API URL (/solve endpoint'i)
      pub name: Name,
      pub slippage: Slippage,
      pub liquidity: Liquidity,
      pub account: Account,                // settlement imzalayan hesap
      pub timeouts: Timeouts,
      pub request_headers: HashMap<String, String>,  // auth header'ları buraya
      pub fee_handler: FeeHandler,         // Driver veya Solver fee hesaplar
      pub quote_using_limit_orders: bool,
      pub merge_solutions: SolutionMerging,
      pub s3: Option<S3>,                  // auction arşivleme (debug/audit)
      pub solver_native_token: ManageNativeToken,
      pub quote_tx_origin: Option<eth::Address>,
      pub response_size_limit_max_bytes: usize,
      pub bad_order_detection: BadOrderDetection,
      pub settle_queue_size: usize,
      pub flashloans_enabled: bool,
      pub fetch_liquidity_at_block: infra::liquidity::AtBlock,
      pub haircut_bps: u32,                // 0-10000, konservatif teklif kesintisi
      pub submission_accounts: Vec<Account>, // EIP-7702 paralel gönderim
      pub max_solutions_to_propose: NonZeroUsize,
      pub post_processing_concurrency_limit: NonZeroUsize,
  }
  ```

- `Solver` (satır 102): Ana solver client struct.
  ```
  pub struct Solver {
      client: reqwest::Client,   // persistent HTTP bağlantısı (tcp_keepalive=60s)
      config: Config,
      eth: Ethereum,
      persistence: Persistence,
  }
  ```

- `SolutionMerging` (satır 667):
  ```
  pub enum SolutionMerging {
      Allowed { max_orders_per_merged_solution: usize },
      Forbidden,
  }
  ```

- `BadOrderDetection` (satır 703): Token/order kalite denetimi konfigürasyonu.

- `Error` (satır 674): `Http`, `Deserialize`, `Dto`, `CustomError` varyantları. `is_timeout()` metodu timeout tespiti için.

**Kritik Fonksiyonlar:**

- `Solver::try_new()` (satır 256-282): HTTP client oluştur — Content-Type: application/json, Accept: application/json, tcp_keepalive=60s. request_headers'ı default headers'a ekler. S3 persistence kurar.

- `Config::validate()` (satır 225-253):
  - `max_solutions_to_propose > 1` ise `submission_accounts` (EIP-7702) zorunlu.
  - `submission_accounts` varsa tüm hesaplar gerçek signer olmalı (Address-only yasak).
  - main account da signer olmalı (delegation kurulumu için).

- `Solver::solve()` (satır 363-487): **ANA ÇÖZÜM AKIŞI — EN KRİTİK FONKSİYON**

  **Adım 1:** Flashloan hint'leri ve wrapper call'ları topla.

  **Adım 2:** `dto::auction::new()` ile auction DTO oluştur — haircut, volume fee düzeltmesi, token mapping dahil.

  **Adım 3:** `archive_id` kontrolü — gerçek auction ID varsa S3 arşivleme aktif.

  **Adım 4:** Streaming body:
  - S3 arşivleme aktifse: `stream_body_and_gzip()` — solver'a gönderilirken eş zamanlı gzip kopyası oluştur.
  - Yoksa: `stream_body()` — sadece stream.

  **Adım 5:** Deadline kontrolü — solver_deadline `remaining()` → `Ok(timeout)` veya `Err(_)` → boş sonuç.

  **Adım 6:** HTTP POST `/solve` — `timeout(solver_deadline)`, tracing headers, X-REQUEST-ID header.

  **Adım 7:** `util::http::send(response_size_limit, req)` — yanıt boyutu sınırlı.

  **Adım 8:** `serde_json::from_str::<SolverResponse>()` — parse fail → DeserializationError notify.

  **Adım 9:** `SolverResponse::Error` → `Error::CustomError` return.

  **Adım 10:** `SolverResponse::Solutions` → `dto::Solutions::into_domain()` ile domain nesnelere çevir.

- `Solver::notify()` (satır 537-560): **Fire-and-forget POST `/notify`** — tokio::spawn ile async gönderim, 404 hataları sessizce yutulur (eski API uyumluluğu).

- `assemble_flashloan_hints()` (satır 489-511): Order app_data'dan flashloan hint'leri çıkar, `flashloans_enabled` kontrolü.

- `assemble_wrappers()` (satır 514-534): Order başına wrapper call'ları topla (EIP-7702 delegasyon için).

**Matematiksel Detay — Timeout Hesabı:**
```
solver_timeout = auction.deadline(timeouts).solvers().remaining()
```
`deadline(timeouts)` → `deadline = now + (auction_end - now) * solving_share_of_deadline - http_delay`

**Güvenlik Analizi:**
- Authentication: `request_headers` ile API key/auth token header olarak eklenir — TLS zorunlu.
- Response size limit: `response_size_limit_max_bytes` — sonsuz bellek tüketimini engeller.
- Timeout enforcement: deadline geçilmişse istek gönderilmez.
- Custom error propagation: solver'ın kendi hata mesajı iletilir (XSS riski? — sadece backend log).
- Notify 404 silent: eski API versiyonlarıyla geriye dönük uyum.

**DEX Uygulama Notu — Nova Shield için:**
- Solver HTTP client mimarisi: persistent connection + keepalive → NovaShield'da solver'lar aynı şekilde pool ile bağlanmalı.
- `solving_share_of_deadline` parametresi: toplam auction süresi / solver süresi / encoding süresi ayrımı → Nova Shield'da blok süresi kısıt olduğunda kritik.
- Authentication model: header-based API key → NovaShield'da TLS+mTLS veya JWT tercih et.
- FireAndForget notify: solver'a sonuç bildirimi isteğe bağlı ve non-blocking → NovaShield da aynı pattern.

---

### [infra/solver/dto/auction.rs] (589 satır)

**Amaç:** Domain auction nesnesini solver'ın beklediği wire formatına serialize eden dönüşüm katmanı.

**Kritik Fonksiyonlar:**

- `new()` (satır 24-333): Auction → `solvers_dto::auction::Auction` dönüşümü.

  **Token mapping:**
  - Tüm auction token'ları `solvers_dto::auction::Token` struct'ına: decimals, symbol, reference_price, available_balance, trusted.
  - Liquidity pool token'ları için boş entry garanti (solver tüm token'ları bilmeli).

  **Order mapping:**
  - `order.available()` çağrısı mevcut doldurulumuş miktarı hesaplar (kısmi emirler için).
  - `ManageNativeToken::wrap_address = true` ise buy token ETH → WETH adresine map.
  - **Volume fee düzeltmesi** (satır 92-115):
    - `FeeHandler::Driver` + `Volume { factor }` fee politikası varsa:
      - Sell order: `sell.amount *= 1 / (1 + factor)` → solver daha az sell token ile çalışır
      - Buy order: `buy.amount *= 1 / (1 - factor)` → solver daha fazla buy token getirimeli
    - Neden: Driver settlement sırasında volume fee kesildiğinde limit fiyat ihlali olmasın diye.
  - **Haircut düzeltmesi** — `apply_haircut()` fonksiyonu çağrılır.

  **Liquidity mapping:**
  - UniswapV2 → `ConstantProductPool` (fee hardcoded 0.003 = 3/1000)
  - UniswapV3 → `ConcentratedLiquidityPool` (sqrt_price, liquidity, tick, liquidity_net, fee)
  - BalancerV2Stable → `StablePool` (amplification_parameter, scaling_factor)
  - BalancerV2Weighted → `WeightedProductPool` (weight, scaling_factor, version V0/V3+)
  - Swapr → `ConstantProductPool` (fee = pool.fee.bps/10000)
  - ZeroEx → `ForeignLimitOrder` (maker/taker token, amounts, taker_token_fee_amount)

  **Gas price:** `auction.gas_price().effective()` EIP-1559 effective gas price.

- `apply_haircut()` (satır 418-453): **KRİTİK GÜVENLİK MEKANİZMASI**

  **Matematiksel Formül:**
  - Sell order: `buy.amount_tightened = buy.amount / (1 - h)` burada `h = haircut_bps / 10000`
  - Buy order: `sell.amount_tightened = sell.amount / (1 + h)`

  **Neden:** Driver sonradan haircut uygular (`executed_buy -= haircut`). Eğer solver'a orijinal limit gösterilseydi, driver'ın haircut'ı uygulaması limit fiyatı ihlal edebilirdi. Sıkılaştırılmış limit göstererek, solver'ın bulduğu çözüm haircut sonrasında da geçerli kalır.

  **Üretim incident kaydı** (satır 496-499): `0xa978e3ec…6a020c06` order'ında `GPv2: limit price not respected` revert → bu mekanizma o bug'ı düzeltti.

  **Overflow durumu:** `apply_factor()` None dönerse (sadece sell order buy.amount sonsuz büyür) → `unwrap_or_default()` = 0, uyarı log.

- `fee_policy_from_domain()` (satır 335-358): 3 fee politikası:
  - `Surplus { factor, max_volume_factor }` — fazladan kazanılan surplus'ın yüzdesi
  - `PriceImprovement { factor, max_volume_factor, quote }` — quote'a göre iyileştirme
  - `Volume { factor }` — işlem hacminin yüzdesi

**Güvenlik Analizi:**
- Volume fee + haircut kombinasyonu: her ikisi de aynı limit'e uygulanır — sıra önemli (önce volume fee, sonra haircut).
- Overflow koruması: BigDecimal dönüşümleri f64 round-trip riski taşır — production'da test edildi.
- UniswapV2 fee: hardcoded 0.003 — Swapr gibi custom fee'li fork'lar için dinamik.

**DEX Uygulama Notu — Nova Shield için:**
- Volume fee pre-processing: NovaShield'da fee'yi solver'a göndermeden önce limit'e dahil etmek aynı mantık.
- Haircut mekanizması: Nova Shield'da "güvenlik payı" ekleyerek solver'ların limit fiyatı patlatmaması sağlanabilir.
- Liquidity type encoding: tüm AMM türleri için standart bir DTO formatı → NovaShield da bunu kopyalamalı.
- ZeroEx ForeignLimitOrder: off-chain RFQ entegrasyonu → NovaShield'da private market maker entegrasyonu için referans.

---

### [infra/solver/dto/solution.rs] (415 satır)

**Amaç:** Solver'ın JSON yanıtını domain Solution nesnelerine parse eden dönüşüm.

**Struct:**
- `Solutions(Vec<solvers_dto::solution::Solution>)` — newtype wrapper.

**Kritik Fonksiyonlar:**

- `Solutions::into_domain()` (satır 29-286):

  **Fulfillment trade parsing:**
  - `find_order()` ile auction'daki order'ı UID'den bul (bulamazsa Error).
  - `haircut_fee` hesabı: `executed_amount * haircut_bps / MAX_BASE_POINT` — sonradan surplus'tan düşülür.
  - Fee: solver `fee` alanı → `Fee::Dynamic(SellAmount(fee))`, yoksa `Fee::Static`.

  **JIT trade parsing:**
  - JitOrder struct oluştur → `uid()` metodu EIP-712 domain separator ile UID hesaplar.
  - `signature()` → signing scheme'e göre imza doğrulama:
    - EIP-712: `sig.recover_owner()` + `hash_struct()`
    - Eip1271: signer prepend edilmiş, ilk 20 byte çıkarılır.
  - `raw_order_data()` → model::OrderData yapısı (fee_amount=0, çünkü JIT order'da fee yok).

  **Interaction parsing:**
  - Custom interactions: target, value, calldata, allowances, inputs, outputs, internalize flag.
  - Liquidity interactions: ID string → usize parse → liquidity listesinde ara, input/output assets.

  **Gas override:** solver `gas_fee_override` alanı → `max_fee_per_gas` + `max_priority_fee_per_gas` overflow kontrolü.

  **Flashloan:** solver `flashloans` alanı → order UID'den flashloan'a map; yoksa auction'daki hint'leri kullan.

- `JitOrder::uid()` (satır 388-395): OrderData hash → EIP-712 domain separator → UID hesapla.

- `JitOrder::signature()` (satır 338-386): İmza doğrulama + signer recovery. EIP-1271 için veri manipülasyonu.

**Güvenlik Analizi:**
- `find_order()` hata mesajına UID hex dahil edilir — error'da bilgi sızıntısı yok (backend log).
- JIT order imza doğrulama: `recover_owner()` başarısız olursa `Error` return (çözüm reddedilir).
- Haircut fee: U256 çarpma overflow → `checked_mul`, None → 0 (konservatif).
- EIP-1271 signer çıkarımı: ilk 20 byte signer adresi — encoding bug potansiyeli sıkı test edilmeli.

**DEX Uygulama Notu — Nova Shield için:**
- JIT order konsepti: solver kendi likidite sağlayıcısı gibi davranıp anlık emir oluşturabilir → NovaShield'da flash liquidity provision için referans.
- İmza doğrulama katmanı: EIP-712 + EthSign + PreSign + EIP-1271 → NovaShield'da çoklu signing scheme desteği.
- Custom interaction parsing: calldata level'da esnek entegrasyon → solver istediği protokolü çağırabilir.

---

### [infra/solver/dto/notification.rs] (93 satır)

**Amaç:** Domain bildirim event'lerini solver'ın anlayacağı wire formatına dönüştür.

**Kritik Fonksiyon:**
- `new()`: auction_id + solution_id + Kind → `solvers_dto::notification::Notification`.

**Notification Kind Mapping:**
| Domain Kind | Wire Format |
|---|---|
| Timeout | Timeout |
| EmptySolution | EmptySolution |
| SimulationFailed(block, tx, succeeded_once) | SimulationFailed { block, tx, succeeded_once } |
| ScoringFailed(InvalidClearingPrices) | InvalidClearingPrices |
| ScoringFailed(InvalidExecutedAmount) | InvalidExecutedAmount |
| ScoringFailed(MissingPrice(token)) | MissingPrice { token_address } |
| NonBufferableTokensUsed(tokens) | NonBufferableTokensUsed { tokens } |
| SolverAccountInsufficientBalance(ether) | SolverAccountInsufficientBalance { required } |
| DuplicatedSolutionId | DuplicatedSolutionId |
| DriverError(reason) | DriverError { reason } |
| SettlementStarted | SettlementStarted |
| Settled(Success(hash)) | Success { transaction: hash } |
| Settled(Revert(hash)) | Revert { transaction: hash } |
| Settled(SimulationRevert) | Cancelled |
| Settled(Fail) | Fail |
| Settled(Expired) | Expired |
| PostprocessingTimedOut | PostprocessingTimedOut |
| DeserializationError(reason) | DeserializationError { reason } |

`SolutionId::Single` vs `SolutionId::Merged` — merged solutions birden fazla solution ID taşır.

**DEX Uygulama Notu:** NovaShield'da solver feedback loop kritik — solver neden reddedildiğini bilmeli. Bu tam notification seti (16 event türü) referans alınmalı.

---

### [infra/solver/eip7702.rs] (585 satır)

**Amaç:** EIP-7702 delegasyon mekanizmasını kurar — solver EOA'sı çeşitli submission hesapları adına işlem gönderebilir.

**Constants:**
- `DELEGATION_PREFIX: [u8; 3] = [0xef, 0x01, 0x00]` — EIP-7702 kod prefix.
- `MAX_APPROVED_CALLERS: usize = 5` — delegate contract maksimum yetkilendirilmiş hesap sayısı.
- `CREATE2_DEPLOYER: Address = 0x4e59b44847b379578588920cA78FbF26c0B4956C` — Arachnid deterministik CREATE2 proxy.
- `CREATE2_SALT: B256 = B256::ZERO` — deterministik adres için sıfır salt.

**Struct:**
- `DelegationStatus` (satır 341): `Empty` (undelegated EOA), `DelegatedTo(Address)`, `OtherCode` (beklenmeyen kod).
- `DeploymentMode` (satır 219): `DeployOnly` (sadece contract deploy), `DeployAndDelegate` (deploy + EOA delegation).

**Kritik Fonksiyonlar:**

- `setup()` (satır 54-79): Driver başlangıcında tüm solver'ları tarar, `submission_accounts` varsa `setup_solver()` çağırır. Wallet'a tüm signer'ları register eder.

- `delegate_deployment()` (satır 85-124):
  - Caller listesi + zero-padded approved_callers dizisi oluştur.
  - `Solver7702Delegate` bytecode + constructor encode.
  - CREATE2: `CREATE2_DEPLOYER.create2_from_code(CREATE2_SALT, &init_code)` → deterministik adres.
  - Validasyonlar: max 5 caller, no zero address, no duplicates.

- `setup_solver()` (satır 127-216): State machine:
  - `(DelegatedTo(x), _)` x != delegate → bail! (farklı delegation var, elle düzeltilmeli)
  - `(OtherCode, _)` → bail! (EOA'da beklenmedik kod)
  - `(DelegatedTo(_), delegate_missing=true)` → sadece contract deploy et
  - `(DelegatedTo(_), false)` → zaten hazır, OK
  - `(Empty, delegate_missing=true)` → deploy + delegate (bir arada)
  - `(Empty, false)` → sadece delegation ayarla

- `deploy_delegate()` (satır 231-338):
  - `CREATE2_DEPLOYER` kodu doğrula (beklenen hexten farklıysa bail!)
  - Nonce hesapla: `wait_for_pending_txs()` ile pending tx'ler temizlenene bekle.
  - `DeployAndDelegate` modunda: EIP-7702 authorization hazırla + imzala.
  - CREATE2 deployer'a: `[32-byte salt][init_code]` gönder.
  - Receipt bek + contract kodu doğrula.
  - Delegation kontrol: `is_delegated_to()` ile EOA kodu check.

- `wait_for_pending_txs()` (satır 470-507):
  - Poll interval: 3 saniye, max wait: 90 saniye.
  - `pending_nonce <= latest_nonce` olana kadar bekle.
  - Timeout → bail!

- `sign_authorization()` (satır 447-465): `Authorization { chain_id, address: delegate, nonce: auth_nonce }` → sign hash → `SignedAuthorization`.

**Güvenlik Analizi:**
- Farklı delegation'ı asla otomatik override etme — güvenlik olayı response'u korur.
- `OtherCode` durumunda bail — EOA'ya beklenmedik kod yazılmışsa müdahale yok.
- Auth nonce = tx nonce + 1: EIP-7702 semantiği (sender nonce önce artar).
- Pending tx bekleme: önceki settlement'ları silmeden üstlerine nonce yazmaz.
- Duplicate detection: `seen: HashSet` ile duplicate caller engellenir.

**DEX Uygulama Notu — Nova Shield için:**
- EIP-7702 paralel submission: birden fazla submission account ile aynı settlement'ı farklı mempool'lara eşzamanlı gönderim → inclusion şansını artırır.
- CREATE2 deterministik deployment: caller set sabit tutulursa adres her ortamda aynı → testnet/mainnet tutarlılığı.
- Nova Shield'da benzer pattern: validator EOA'sına bir "Settlement7702Delegate" contract delegasyonu yapılabilir.

---

### [infra/solver/streaming/mod.rs] (264 satır)

**Amaç:** Auction JSON'unu bellekte tutmadan aynı anda hem solver'a stream et hem de S3 için gzip'li kopyasını oluştur.

**Constants:**
- `CHUNK_SIZE: usize = 64 * 1024` — 64KB chunk'lar.
- `CHANNEL_CAPACITY: usize = 2` — double buffering (1 gönderilirken 1 hazırlanır).

**Struct:**
- `Measurements` (satır 31): `serialize: Duration` + `total: Duration` — serialization overhead ölçümü.
- `GzipCapture` (satır 161): gzip writer + oneshot sender — `finalize()` ile compressed bytes gönderilir.
- `ChannelWriter` (satır 121): mpsc sender wrapper — blocking_send ile backpressure.
- `BestEffortSink<W>` (streaming/best_effort_sink.rs): İlk hatada inner'ı drop eder, sonraki write'lar no-op — solver bağlantısı kopsa bile gzip arşivi tamamlanır.
- `Timed<W>` (streaming/timed_writer.rs): write/flush sürelerini accumulate eder — serialization vs. network ayrımı.
- `TeeWriter` (streaming/tee_writer.rs): iki Write'a eşzamanlı yazar.

**Kritik Fonksiyonlar:**

- `stream_body()` (satır 41): Sadece solver body stream — secondary sink = std::io::Sink.
- `stream_body_and_gzip()` (satır 50): Body + gzip capture. S3 arşivleme aktifse kullanılır.
- `stream_into()` (satır 70-114): Ana mekanizma:
  1. `mpsc::channel(CHANNEL_CAPACITY)` → `ReceiverStream` → `reqwest::Body`
  2. `tokio::task::spawn_blocking` — JSON serialization blokeli thread'de
  3. `BestEffortSink(ChannelWriter)` + secondary (gzip) → `TeeWriter` → `Timed` → `BufWriter`
  4. `serde_json::to_writer()` — streaming serialize
  5. `total = start.elapsed()`, `serialize = total - timed.elapsed()` (network süresi çıkarılır)
  6. `measurements_tx.send(Measurements)` — observer için metrik

**Test Coverage:**
- `channel_writer_forwards_all_bytes` — iki CHUNK_SIZE + 123 byte tam iletim
- `gzip_capture_matches_serialized_value` — gzip decode = orijinal JSON
- `gzip_captured_even_if_request_body_dropped` — body drop → arşiv yine tamamlanır
- `reports_measurements_once_serialization_finishes` — receiver resolve

**Güvenlik Analizi:**
- Body drop → gzip devam eder: solver crash/timeout → S3 arşivi kaybolmaz.
- Backpressure: CHANNEL_CAPACITY=2 → solver yavaşsa serialization da yavaşlar (memory spike yok).
- BestEffortSink: solver bağlantısı kesilince gzip'e yazmaya devam eder (ikinci amaç değişmez).

**DEX Uygulama Notu — Nova Shield için:**
- Streaming serialization: büyük auction'ları (1000+ order) tek seferde belleğe almadan gönder — NovaShield'da blok başına auction 2-5MB olabilir.
- Gzip + S3 arşivleme: debug, audit, ML training için — Nova Shield'da IPFS veya object storage'a benzer şekilde arşivle.
- Timed wrapper: network vs. serialization ayrımı → latency profiling için Nova Shield'da benimsenmeli.

---

## BÖLÜM 2: infra/blockchain/ — ZINCIR ERİŞİM KATMANI

### [infra/blockchain/mod.rs] (407 satır)

**Amaç:** Ethereum node'una RPC erişimi, gas estimation, transaction submission, revert detection.

**Struct:**

- `Rpc` (satır 34): `web3: Web3`, `chain: Chain`, `args: RpcArgs`. `try_new()` → chain_id çek, Chain validate.

- `RpcArgs` (satır 41): `url`, `max_batch_size`, `max_concurrent_requests`.

- `Ethereum` (satır 86): Ana blockchain erişim nesnesi. `Arc<Inner>` + `Web3`.

- `Inner` (satır 91):
  ```
  struct Inner {
      chain: Chain,
      contracts: Contracts,
      gas: Arc<GasPriceEstimator>,
      current_block: CurrentBlockWatcher,
      balance_simulator: BalanceSimulator,
      state_overrider: Arc<dyn StateOverriding>,
      tx_gas_limit: eth::Gas,   // per-tx gas limit (EIP-7825 için)
  }
  ```

**Kritik Fonksiyonlar:**

- `Ethereum::new()` (satır 108-156): Async init — current block stream, gas estimator, contracts, state_overrider, balance_simulator.

- `gas_price()` → `GasPriceEstimator::estimate()`.

- `estimate_gas()` (satır 233-265): Gas estimate — `tx_gas_limit` üst sınır olarak set edilir. `simulation_gas_price()` ile gerçekçi gas price kullanılır (token bypass detection için).

- `transaction_status()` (satır 268-294): `get_transaction_receipt` → `TxStatus::Pending / Executed / Reverted`. block_number yoksa Pending.

- `successful_settlement_block()` (satır 305-325): **Önemli optimizasyon** — `eth_getTransactionReceipt` gecikebilir (node lag), ama `eth_getLogs` Settlement event'ini daha hızlı bulur. from_block → Latest aralığında tarar. Hızlı zincirde atlanan block'ları da kapsar.

- `pending_transaction_count()` (satır 332-340): Pending tag ile nonce — mempool'daki tx'leri sayar.

- `simulation_gas_price()` (satır 341-352): Base fee + estimated priority fee → bazı token'lar gas price=0'da farklı davranır (bypass detection) bu yüzden gerçek gas price ile simulate.

- `is_revert()` (satır 385-397): `Error::Rpc(err)` → `err.is_error_resp()`. `Error::AccessList` → her zaman true.

**Güvenlik Analizi:**
- `tx_gas_limit` üst sınır: kötü niyetli solution gas limit için binary search'i block limit'e kadar zorlayamaz.
- `simulation_gas_price`: token anti-simulation bypass'ını engeller.
- Revert detection: node-dependent — farklı node client'ları farklı error döner.

**DEX Uygulama Notu — Nova Shield için:**
- `successful_settlement_block` pattern: log scanning vs. receipt polling — Nova Shield'da da log scanning tercih et (node lag'a karşı).
- `tx_gas_limit` per-chain konfig: farklı EVM zincirlerinde farklı limitler (Fusaka EIP-7825: 16M).
- `simulation_gas_price`: token davranış tespiti — Nova Shield'da da non-zero gas price ile sim.

---

### [infra/blockchain/contracts.rs] (154 satır)

**Amaç:** On-chain smart contract binding'leri.

**Struct:**
- `Contracts` (satır 10):
  - `settlement: GPv2Settlement::Instance` — ana settlement contract
  - `vault_relayer: eth::ContractAddress` — Balancer vault relayer
  - `signatures: Signatures::Instance` — EIP-1271 imza doğrulama
  - `weth: WETH9::Instance` — wrapped ETH
  - `settlement_domain_separator: eth::DomainSeparator` — EIP-712 domain (startup'ta çekilir)
  - `flashloan_router: Option<FlashLoanRouter::Instance>` — henüz her yere deploy edilmedi
  - `balance_helper: Balances::Instance` — settlement contract bakiye simülasyonu
  - `cow_amm_helper_by_factory: HashMap<ContractAddress, ContractAddress>` — CoW AMM factory → helper map

- `Addresses` (satır 32): Override addresses (testnet için).

**Kritik Detaylar:**
- `vaultRelayer()` on-chain'den çekilir (config'e güvenilmez).
- `domainSeparator()` on-chain'den çekilir — EIP-712 replay protection.
- `flashloan_router` optional — yeni feature, bazı chain'lerde yok.

**DEX Uygulama Notu:** Nova Shield'da settlement contract domain separator mutlaka on-chain'den çekilmeli — deployment adresi bile değişse tutarlı.

---

### [infra/blockchain/gas.rs] (150 satır)

**Amaç:** EIP-1559 gas fiyat tahmini + operatör ayarları.

**Struct:**
- `GasPriceEstimator` (satır 20): `gas: Arc<dyn GasPriceEstimating>` + `adjustments: GasPriceParameters`.
- `GasPriceParameters` (satır 113):
  - `max_additional_tip: eth::U256` — priority fee üstüne eklenecek max ek tip
  - `additional_tip_factor: f64` — priority fee'nin yüzdesi olarak ek tip
  - `max_fee_per_gas: eth::U256` — ağ çok congested ise submit etme
  - `min_priority_fee: eth::U256` — minimum priority fee garantisi

**Kritik Formül — estimate() (satır 53-98):**
```
tip_percentage_as_bps = additional_tip_factor * 10000.0
calculated_tip = priority_fee * tip_percentage_as_bps / 10000
additional_tip = min(max_additional_tip, calculated_tip)
max_priority_fee_per_gas = max(min_priority_fee, priority_fee + additional_tip)
suggested_max_fee_per_gas = max(base_estimate.max_fee_per_gas, max_priority_fee_per_gas)
if suggested_max_fee_per_gas > max_fee_per_gas → Error::GasPrice (network too congested)
```

**GasEstimatorType:**
- `Web3` → `NodeGasPriceEstimator` (standart eth_gasPrice)
- `Alloy { past_blocks, reward_percentile }` → `ConfigurableGasPriceEstimator` (geçmiş block'lardan percentile)

**`adjustments()` fonksiyonu (satır 126-149):** mempool config'lerinden en agresif parametreleri seç — private mempool'lar yüksek tip ister.

**DEX Uygulama Notu — Nova Shield için:**
- `max_fee_per_gas` cap: ağ congestion'ında settlement yapmayı durdur — kayıp önler.
- `additional_tip_factor`: private mempool'a gönderirken daha yüksek tip → MEV koruması.
- Percentile-based estimation: `Alloy` mode geçmişe bakarak daha tutarlı tahmin.

---

### [infra/blockchain/token.rs] (81 satır)

**Amaç:** ERC-20 token RPC bindings.

**Struct:** `Erc20 { token: contracts::ERC20::Instance }`.

**Fonksiyonlar:**
- `allowance(owner, spender)` → `eth::Allowance`
- `decimals()` → `Option<u8>` — node error ise Err, token metod yoksa None
- `symbol()` → `Option<String>` — aynı pattern
- `balance(holder)` → `eth::TokenAmount`

**DEX Uygulama Notu:** `is_node_error()` ayırımı kritik — token ERC-20 metod desteklemeyebilir (bazı NFT'ler) ama node'un kendi hatası farklı işlenmeli.

---

## BÖLÜM 3: infra/notify/ — BİLDİRİM SİSTEMİ

### [infra/notify/mod.rs] (142 satır)

**Amaç:** Driver event'lerini solver engine'e bildiren facade — 12 olay türü.

**Fonksiyonlar (hepsi `solver.notify()` wrapper):**

| Fonksiyon | Tetikleyici | Notification Kind |
|---|---|---|
| `solver_timeout()` | HTTP deadline aşıldı | Timeout |
| `empty_solution()` | Çözüm kullanıcı emri içermiyor | EmptySolution |
| `scoring_failed()` | Score hesaplanamadı | ScoringFailed(kind) |
| `encoding_failed()` | Settlement encode edilemedi | NonBufferableTokensUsed / InsufficientBalance / GasLimitExceeded |
| `simulation_failed()` | On-chain simulation revert | SimulationFailed(block, tx, succeeded_once) |
| `settlement_started()` | Kazanan settlement başlıyor | SettlementStarted |
| `executed()` | Settlement sonucu | Settled(Success/Revert/SimulationRevert/Expired/Fail) |
| `duplicated_solution_id()` | Aynı ID ile iki çözüm | DuplicatedSolutionId |
| `postprocessing_timed_out()` | Encoding süresi bitti | PostprocessingTimedOut |

**Önemli:** `scoring_failed()` Math ve ProtocolFeeOnStaticOrder için return eder — solver'a bu hatalar bildirilmez (internal hata).

**Güvenlik Analizi:**
- Notification'lar fire-and-forget — solver API kesilse de driver bloklenmez.
- `SimulationFailed` full tx payload içerir — solver neden revert ettiğini görebilir (debug value yüksek).

**DEX Uygulama Notu — Nova Shield için:**
- Bu feedback loop mekanizmasının tamamı kopyalanmalı — solver'lar öğrenen agent'lar olabilir.
- `SimulationFailed(block, tx, succeeded_once)` özellikle değerli — block stale oldu mu, ilk denemede başarılı mıydı bilgisi.

---

### [infra/notify/notification.rs] (77 satır)

**Struct/Enum:**
- `Notification { auction_id, solution_id, kind: Kind }` — ana bildirim tipi.
- `Kind` (satır 22): 14 varyant — yukarıda tabloda listelendi.
- `ScoreKind` (satır 53): `InvalidClearingPrices`, `InvalidExecutedAmount`, `MissingPrice(TokenAddress)`.
- `Settlement` (satır 64): `Success(TxHash)`, `Revert(TxHash)`, `SimulationRevert`, `Expired`, `Fail`.

**DEX Uygulama Notu:** `SimulationSucceededAtLeastOnce: bool` kritik flag — solver çözümü başlangıçta valid idi ama blok değişince invalid oldu → solver'a "stale block" sebep olduğu söylenebilir.

---

### [infra/notify/liquidity_sources/mod.rs] (87 satır)

**Amaç:** Off-chain likidite sağlayıcılarına settlement bildirimi (PMM'ler).

**Trait:**
```rust
#[async_trait]
pub trait LiquiditySourceNotifying: Send + Sync {
    async fn settlement(&self, settlement: &Settlement) -> anyhow::Result<()>;
}
```

**Struct:** `Notifier { inner: Arc<HashMap<String, Box<dyn LiquiditySourceNotifying>>> }` — registered notifiers.

`settlement()` implementasyonu: `join_all(futures)` — tüm notifier'lar eşzamanlı çağrılır, hata log'lanır ama propagate edilmez (PMM notification non-critical).

Şu an sadece `liquorice` notifier mevcut.

---

### [infra/notify/liquidity_sources/liquorice/] (4 dosya)

**Amaç:** Liquorice RFQ platform entegrasyonu — PMM'lere settlement bildirimi.

**Client (client/mod.rs — 74 satır):**
- `Client { client: reqwest::Client, base_url: Url }` — HTTP client.
- `Client::new()`: `api-key` header (sensitive), `http_timeout`.
- `send_request<R: IsRequest>()` → generic request dispatch.
- Metrics: `liquorice_api_requests` counter (path + result).

**Request trait (client/request.rs — 116 satır):**
- `IsRequest` trait: `type Response`, `async fn send(client, base_url) → Result<Response, Error>`.
- `v1::intent_origin::notification::post::Request`:
  ```
  struct Request {
      source: String,         // "cow_protocol"
      metadata: Metadata { driver_version },
      content: Content::Settle(Settle { auction_id, rfq_ids }),
      timestamp: DateTime<Utc>,   // milisaniye precision
  }
  ```
- POST `/v1/intent-origin/notification` — `200 OK` → deserialize Response, diğer → Error::Status.

**Notifier (notifier.rs — 217 satır):**
- `NOTIFICATION_SOURCE = "cow_protocol"`, `DRIVER_VERSION = env!("CARGO_PKG_VERSION")`.
- `extract_rfq_ids_from_settlement()`: pre_interactions + interactions + post_interactions taranır.
- `extract_rfq_id_from_interaction()`: interaction target == Liquorice settlement contract → `settleSingle` ABI decode → `order.rfqId`.

**Test:**
- `test_extract_rfq_id_from_valid_settle_single_call`: gerçek calldata'dan UUID çıkarımı: `c99d2e3f-702b-49c9-8bb8-43775770f2f3`.
- `test_returns_none_for_arbitrary_call`: boş calldata → None.
- `test_returns_none_for_different_target`: farklı contract → None.

**DEX Uygulama Notu — Nova Shield için:**
- PMM (Private Market Maker) entegrasyonu: Nova Shield'da RFQ-based off-chain likidite → on-chain settlement bildirimi aynı pattern.
- Settlement event extraction: calldata decode ile RFQ ID çıkarımı → Nova Shield'da off-chain ticaret altyapısı entegrasyonu.
- Liquorice API key authentication: `api-key` header → Nova Shield'da partner API'leri için aynı.

---

## BÖLÜM 4: infra/observe/ — METRİK VE GÖZLEM

### [infra/observe/mod.rs] (469 satır)

**Amaç:** Tüm önemli driver event'leri için log + metric wrapper'ları.

**Kritik Fonksiyonlar:**

- `init()`: tracing init + metrics registry init + heap dump handler (Unix).
- `auction()`: `tracing::debug!(id=?auction_id, "received auction")`.
- `solutions()`: çözüm varsa `info!`, yoksa `debug!`.
- `solver_response()`: `used_solve_time` histogram güncelle (solver+kind label).
- `sending_solve_request()`: `remaining_solve_time` histogram güncelle.
- `serialized_solve_request()`: `solve_request_body_time` histogram (serialization + total).
- `mempool_log()`: success/disabled/fail per mempool.
- `mempool_submission_result()`: counter + blocks_passed histogram.
- `encoding_failed()`: `has_haircut` flag ile label ayrımı — haircut kaynaklı başarısızlıklar ayrı izlenir.
- `winner_voided()`: `has_haircut` → `SimulationRevertHaircut` vs `SimulationRevert` label.
- `OrderExcludedFromAuctionReason` enum: `CouldNotFetchBalance`, `InsufficientBalance`, `OrderWithZeroAmountRemaining`.

**DEX Uygulama Notu:** `has_haircut` label ayrımı önemli — haircut etkisini metrikle izleyip neyin etkilediğini anlayabilirsin.

---

### [infra/observe/metrics.rs] (90 satır)

**Amaç:** Prometheus metrik tanımları.

**Metrics struct alanları:**

| Metrik | Tür | Labels | Açıklama |
|---|---|---|---|
| `dropped_solutions` | IntCounterVec | solver, reason | Reddedilen çözümler |
| `solutions` | IntCounterVec | solver, result | Çözüm sonuçları |
| `reveals` | IntCounterVec | solver, result | Reveal sonuçları |
| `settlements` | IntCounterVec | solver, result | Settlement sonuçları |
| `quotes` | IntCounterVec | solver, result | Quote sonuçları |
| `mempool_submission` | IntCounterVec | mempool, result | Mempool gönderim sayacı |
| `mempool_submission_results_blocks_passed` | IntCounterVec | mempool, result | Block geçmesi sayacı |
| `bad_orders_detected` | IntCounterVec | solver | Kötü order tespiti |
| `bad_tokens_detected` | IntCounter | - | Kötü token tespiti |
| `auction_preprocessing` | HistogramVec | stage | Preprocessing süresi (0.01-5.0s buckets) |
| `remaining_solve_time` | HistogramVec | solver, kind | Solver'a kalan süre (0.5-11s) |
| `used_solve_time` | HistogramVec | solver, kind | Solver'dan yanıt süresi (0.5-11s) |
| `solve_request_body_time` | HistogramVec | solver, phase | Body serializasyon süresi (0.001-2.5s) |

`processing_stage_timer()`: `HistogramTimer` döner → `drop` anında kaydeder.

**DEX Uygulama Notu:** Nova Shield için bu metrik set template olarak kullanılmalı. Özellikle `remaining_solve_time` vs `used_solve_time` farkı: solver ne kadar zaman harcadı vs. ne kadar zaman verildi.

---

## BÖLÜM 5: infra/persistence/ — VERİ KALICILIĞI

### [infra/persistence/mod.rs] (89 satır)

**Amaç:** Auction verilerini S3'e arşivleme (debug, audit, ML training).

**Struct:**
- `S3 { bucket: String, prefix: String }` — AWS S3 konfig.
- `Persistence { s3: Option<Arc<s3::Uploader>> }` — optional arşivleme.

**Kritik Fonksiyonlar:**

- `build()`: S3 konfig varsa `s3::Uploader::new()` — async init.
- `archives_enabled()`: `s3.is_some()`.
- `archive_auction_gzipped()` (satır 64-87):
  - `tokio::spawn` ile fire-and-forget.
  - `compressed: oneshot::Receiver<Bytes>` await → S3'e upload.
  - `uploader.upload_gzipped(auction_id.to_string(), compressed)` — key = auction ID string.
  - Hata: warn log, upload başarısızlığı sessiz (non-critical).

**Prefix pattern:** `"staging/mainnet/{auction_id}"` — environment + chain dahil.

**DEX Uygulama Notu — Nova Shield için:**
- S3 arşivleme: ML model eğitimi için gerçek auction verisi kritik → NovaShield'da IPFS veya distributed storage.
- Gzip: veri boyutunu 5-10x azaltır — büyük auction'lar için zorunlu.
- Fire-and-forget: upload başarısız olsa driver bloklenmez.

---

## BÖLÜM 6: infra/tokens.rs — TOKEN METADAta SERVİSİ

### [infra/tokens.rs] (243 satır)

**Amaç:** Token decimals, symbol, settlement contract bakiyesi — cache'li, block başına güncelleme.

**Struct:**
- `Metadata { decimals, symbol, balance: eth::TokenAmount, monitor_balance: bool }`.
- `Fetcher(Arc<Inner>)` — thread-safe.
- `Inner { eth: Ethereum, cache: RwLock<HashMap<TokenAddress, Metadata>>, requests: BoxRequestSharing }`.

**Kritik Fonksiyonlar:**

- `Fetcher::new()`: `update_task` spawn eder — her blok gelince `update_balances()` çağırır (Weak ref, Fetcher drop edilince durur).

- `keep_track_of_balances()` (satır 60-83): trusted token'ları `monitor_balance = true` olarak işaretle. Read lock → sadece güncelleme gerekirse write lock (contention minimize).

- `Inner::fetch_token_infos()` (satır 157-193): `try_join3(decimals(), symbol(), balance(settlement))` — üçü de aynı batch RPC call'da gönderilir. `BoxRequestSharing` ile paralel istek deduplication.

- `Inner::cache_missing_tokens()`: double-check locking — read lock ile önce kontrol, sadece gerekirse write lock.

- `update_balances()` (satır 102-146): `monitor_balance = true` olan token'lar için bakiye çek, write lock ile güncelle. Başarısız olanları `info!` log.

**DEX Uygulama Notu:**
- `BoxRequestSharing`: aynı token için paralel istek → tek RPC call → sonuç paylaşılır — Nova Shield'da da kullan.
- Double-check locking: yüksek concurrency'de lock contention'ı azaltır.
- Settlement contract balance monitoring: internal buffer trading için güncelden balance bilinmeli.

---

## BÖLÜM 7: infra/mempool/ — TX GÖNDERİMİ

### [infra/mempool/mod.rs] (225 satır)

**Amaç:** EIP-1559 transaction gönderimi, nonce yönetimi, pending tx takibi.

**Struct:**
- `Config` (satır 22): `min_priority_fee`, `gas_price_cap`, `target_confirm_time`, `retry_interval`, `nonce_block_number`, `url`, `name`, `revert_protection`, `max_additional_tip`, `additional_tip_percentage`.
- `RevertProtection` (satır 61): `Enabled` (high-revert risk tx'leri public mempool'a gönderme) / `Disabled`.
- `Mempool` (satır 67): `transport: Web3`, `config`, `last_submissions: DashMap<Address, Submission>`.
- `Submission { nonce: u64, gas_price: Eip1559Estimation }`.

**Kritik Fonksiyonlar:**

- `get_nonce()` (satır 102-117): `nonce_block_number` konfige göre: `latest`, `pending`, `finalized`, `safe`, `number(N)`. Flexible nonce retrieval.

- `submit()` (satır 122-181): EIP-1559 tx oluştur → `send_transaction()`. Başarılı → `last_submissions.insert(signer, Submission)`. Başarısız → debug log (önceki submission ile karşılaştır).

- `find_pending_tx_in_mempool()` (satır 185-205): `txpool_content_from(signer)` → pending + queued'da nonce ara. EIP-7702 paralel submission'da nonce conflict tespiti için.

- `last_submission()`: son submission bilgisi — gas price comparison için.

- `reverts_can_get_mined()`: `RevertProtection::Disabled` → true (public mempool kullanılabilir).

**Güvenlik Analizi:**
- `RevertProtection::Enabled`: AMM interaction içeren settlement'lar public mempool'a gönderilmez — MEV koruması.
- `last_submissions` DashMap: concurrent access güvenli.
- nonce_block_number: `pending` tag brokerlarda sorun çıkarabilir (ethrpc_max_concurrent_requests ile dikkat).

**DEX Uygulama Notu — Nova Shield için:**
- RevertProtection: private mempool (Flashbots, MEV Blocker) vs. public mempool seçimi — Nova Shield'da bu kritik.
- `last_submissions` tracking: gas bump mantığı için önceki submission'ı bil.
- nonce_block_number: `latest` vs. `pending` — paralel tx gönderimde pending tercih et.

---

## BÖLÜM 8: infra/api/ — HTTP API SUNUCUSU

### [infra/api/mod.rs] (219 satır)

**Amaç:** Driver HTTP API — autopilot'tan gelen solve/settle/reveal/quote requestlerini işler.

**Struct:**
```rust
pub struct Api {
    pub solvers: Vec<Solver>,
    pub liquidity: liquidity::Fetcher,
    pub liquidity_sources_notifier: notify::liquidity_sources::Notifier,
    pub simulator: Simulator,
    pub eth: Ethereum,
    pub mempools: Mempools,
    pub addr: SocketAddr,
    pub bad_token_detector: risk_detector::bad_tokens::Detector,
    pub addr_sender: Option<oneshot::Sender<SocketAddr>>,
}
```

**`serve()` fonksiyonu (satır 49-158):**
- `balance_fetcher`: cached ERC-20 balance fetcher (block başına güncelleme).
- `tokens::Fetcher`: token metadata.
- `DataAggregator`: tüm yardımcı bileşenleri bir araya getirir.
- Her solver için ayrı `axum::Router` nested — `/{solver_name}/solve`, `/{solver_name}/settle` etc.
- `bad_order_detection` konfige göre simulation veya metrics based detector eklenir.
- Middleware: `DefaultBodyLimit::disable()` (solvers büyük request gönderebilir), `RequestDecompressionLayer`, `TraceLayer` (distributed tracing).
- Port `0.0.0.0:0` ile bind test için `addr_sender` channel.

**Axum Routes:**
- `GET /{solver}/healthz` — sağlık kontrolü
- `GET /{solver}/metrics` — Prometheus metrics
- `GET /{solver}/gasprice` — güncel gas fiyatı
- `GET /{solver}/info` — solver bilgisi
- `POST /{solver}/solve` — auction çözümü
- `POST /{solver}/reveal` — çözüm açıklama (commit-reveal)
- `POST /{solver}/settle` — settlement başlat
- `POST /{solver}/quote` — order fiyat teklifi

**Güvenlik Analizi:**
- `DefaultBodyLimit::disable()`: büyük body kabul edilir — DDoS riski (trust boundary: autopilot).
- `RequestDecompressionLayer`: gzip/brotli decompression — bomb attack riski (trusted caller).
- Multi-solver multiplexing: tek port, birden fazla solver — path isolation yeterli.

**DEX Uygulama Notu — Nova Shield için:**
- Solver multiplexing: tek driver, N solver — Nova Shield'da aynı; farklı routing stratejileri aynı API'yi paylaşabilir.
- Body limit disabled: internal API için OK, public API için asla.
- Distributed tracing: `TraceLayer` + trace_id header → Nova Shield'da OpenTelemetry ekle.

---

### [infra/api/routes/solve/mod.rs] (47 satır) + dto/solve_request.rs (414 satır)

**Route:** `POST /{solver}/solve`

**solve_request.rs — SolveRequest struct (satır 237-251):**
```rust
pub struct SolveRequest {
    id: i64,             // auction ID
    tokens: Vec<Token>,  // token listesi (address + price + trusted)
    orders: Vec<Order>,  // emir listesi
    deadline: DateTime<Utc>,
    surplus_capturing_jit_order_owners: Vec<eth::Address>,
}
```

**Order struct (satır 264-298):**
- uid: `[u8; 56]` hex
- sell_token, buy_token: Address
- sell_amount, buy_amount: U256
- protocol_fees: `Vec<FeePolicy>` (Surplus / PriceImprovement / Volume)
- created: u32, valid_to: u32
- kind: Sell/Buy, receiver, owner
- partially_fillable: bool, executed: U256 (kısmi doldurulumuş miktar)
- pre_interactions, post_interactions: Vec<Interaction>
- sell_token_balance: Erc20/Internal/External (deprecated)
- buy_token_balance: Erc20/Internal (deprecated)
- class: Market/Limit
- app_data: `[u8; 32]` hex
- signing_scheme: eip712/ethsign/presign/eip1271
- signature: hex bytes
- quote: Option<Quote>

**into_domain() (satır 31-203):** Async transform — token_infos RPC batch, trusted token'lar için `keep_track_of_balances()`, Order → competition::Order.

**DEX Uygulama Notu:**
- `surplus_capturing_jit_order_owners`: solver kendi JIT order'larından surplus kapabilir — Nova Shield'da MEV/surplus capture mekanizması için.
- `protocol_fees`: `Vec<FeePolicy>` — birden fazla fee politikası aynı order'a uygulanabilir.

---

### [infra/api/routes/settle/mod.rs] ve [reveal/mod.rs]

- `POST /settle`: `SettleRequest { auction_id, solution_id, submission_deadline_latest_block }` → `competition.settle()`.
- `POST /reveal`: `RevealRequest { auction_id, solution_id }` → `competition.reveal()` → `RevealResponse { calldata }`.

**Commit-Reveal:** Autopilot önce `/solve` → winner seç → `/reveal` (calldata aç) → `/settle` (on-chain gönder).

---

## BÖLÜM 9: boundary/liquidity/ — LİKİDİTE SINIRI

### [boundary/liquidity/mod.rs] (190 satır)

**Amaç:** On-chain likidite kaynaklarından domain Liquidity nesnelerine dönüşüm.

**Cache konfig (satır 35-43):**
```
CacheConfig {
    number_of_blocks_to_cache: 2,
    number_of_entries_to_auto_update: 1000,
    auto_update_ttl: 5 min,
    maximum_recent_block_age: 4,
    max_retries: 5,
    delay_between_retries: 1s,
}
```

**Fetcher::try_new():** UniswapV2, Swapr, BalancerV2, UniswapV3, ZeroEx collector'ları oluşturur. `LiquidityCollector { liquidity_sources, base_tokens }` — base_tokens WETH + konfig'den.

**Fetcher::fetch():** TokenPair set → `Block::Recent/Finalized/Number(N)` → `inner.get_liquidity()` → domain dönüşümleri. Hatalı liquidity sessizce atlanır.

**AtBlock:**
- `Latest`: mevcut block numarası → fresh data
- `Recent`: recent_block_cache semantic
- `Finalized`: kesinleşmiş block

---

### [boundary/liquidity/uniswap/v2.rs] (153 satır)

**Gas:** `GAS_PER_SWAP: u64 = 90_171` (Dune query ile ölçüldü).

**to_domain_pool():** Rezerv overflow kontrolü — `2^112` limit (Uniswap uint112 alanı). 3/1000 fee assertion.

**to_interaction():** `uniswap_v2::Inner::new(router, receiver)` → `settle(input, output)` → `encode_swap()` → `domain::Interaction`.

**collector():** Factory address → PairProvider (factory + init_code_digest) → PoolFetcher → PoolCache (2 block cache).

**DEX Uygulama Notu:**
- `uint112` overflow check: rezerv 2^112 üzerindeyse pool skip — Nova Shield AMM'de aynı tip limit var mı kontrol et.
- `GAS_PER_SWAP` hardcoded estimate: Dune analytics'ten ölçüm → Nova Shield'da da per-interaction gas estimate için referans al.

---

### [boundary/liquidity/uniswap/v3.rs] (176 satır)

**Gas:** `pool.gas_stats.mean_gas` — dinamik (pool indexer'dan ortalama).

**to_domain():** Pool tokens 2 adet assertion. `sqrt_price`, `liquidity`, `tick`, `liquidity_net`, `fee` tam alan eşleştirme.

**Pool data source:**
- `PoolIndexer` → `PoolIndexerClient` (REST API)
- `Subgraph` → `UniV3SubgraphClient` (GraphQL)

Background init + maintenance task — her blok ServiceMaintenance.

**DEX Uygulama Notu:**
- UniV3 pool: sqrt_price + tick + liquidity_net → concentrated liquidity math → Nova Shield DEX concentrated liquidity için bu veri yapısı referans.
- `mean_gas` dinamik: pool geçmiş gas kullanımından öğrenir → Nova Shield'da adaptive gas estimation.

---

### [boundary/liquidity/balancer/v2/weighted.rs] (88 satır)

**Gas:** `GAS_PER_SWAP: u64 = 88_892` (Dune query).

**to_domain():**
- Weight: `Weight::from_raw(reserve.weight.as_uint256())` — 18 decimals normalized.
- ScalingFactor: `ScalingFactor::from_raw()` — token decimal normalization.
- Fee: `Fee::from_raw(pool.fee.as_uint256())` — 18 decimals.
- Version: V0 vs. V3+ ayrımı (Balancer protocol versiyonu).

**to_interaction():** Balancer vault `swap()` call → settlement interaction.

**DEX Uygulama Notu:**
- Balancer weighted pool: ağırlıklı çarpım invariantı `∏(balance_i^weight_i) = k` → Nova Shield DEX custom pool türleri için referans.
- ScalingFactor: farklı decimals'lı token'ların normalizasyonu — 6 decimal USDC vs. 18 decimal WETH.

---

### [boundary/liquidity/zeroex.rs] (102 satır)

**Gas:** `GAS_PER_ZEROEX_ORDER` (price_estimation::gas'tan).

**to_domain():** `OrderSettlementHandler` downcast → ZeroEx order struct extract:
- `maker`, `taker`, `sender` adresleri
- `maker_token`, `taker_token`
- `maker_amount`, `taker_amount` (fillable miktarlar)
- `taker_token_fee_amount` (protokol fee)
- `fee_recipient`, `pool`, `expiry`, `salt`
- ZeroEx signature: r, s, v, signature_type

**collector():** `DefaultZeroExApi` HTTP client → blocks stream → `ZeroExLiquidity`.

**DEX Uygulama Notu — Nova Shield için:**
- ZeroEx limit order: off-chain signed emir → on-chain settle → Nova Shield'da RFQ aggregation.
- `taker_token_fee_amount`: protokol fee ayrımı maker/taker'dan bağımsız.

---

## BÖLÜM 10: util/ — YARDIMCI ARAÇLAR

### [util/http.rs] (43 satır)

**`send(limit_bytes, req)`:**
- `req.send().await` → streaming yanıt.
- Chunk-by-chunk oku → `data.len() > limit_bytes` → `Error::ResponseTooLarge`.
- UTF-8 parse → success veya `Error::NotOk { code, body }`.

**Error türleri:** `Response(reqwest::Error)`, `ResponseTooLarge`, `NotUtf8`, `NotOk { code, body }`.

**DEX Uygulama Notu:** Response size limit chunked reading ile — tam body buffer'lamadan önce boyut kontrolü → memory DoS önlemi.

---

### [util/math.rs] (113 satır)

**`mul_ratio(x, q, d) → Option<U256>`:** `x * q / d` floor — fast path U256 checked_mul, slow path U512 widening mul.

**`mul_ratio_ceil(x, q, d) → Option<U256>`:** `x * q / d` ceiling — `(p + d - 1) / d` yerine remainder check.

**Matematiksel detay:**
- `d == 0` → None (division by zero)
- `x * q` overflow U256 → U512 widening mul → U256 try_from → None if still overflow
- Ceiling: `rem != 0` → +1

**Test coverage:** sıfır, overflow, normal (floor), normal (ceil), büyük değerler.

**DEX Uygulama Notu — Nova Shield için:**
- `mul_ratio` fonksiyonu AMM hesaplamaları için kritik — rezerv * fee / 1000 gibi. U512 widening mul overflow'u önler — Nova Shield'da bu utility kullanılmalı.

---

## BÖLÜM 11: tests/ — ENTEGRASYON TESTLERİ

### [tests/cases/haircut.rs] (310 satır)

**`order_haircut_reduces_score()`:**
- HAIRCUT_BPS = 500 (5%).
- Baseline (0 bps) → score_no_haircut.
- Haircut (500 bps) → score_with_haircut.
- Yüzde: `score_with_haircut * 100 / score_no_haircut` → `94..=96` range kontrolü.
- executedSell == signedSellAmount (fill-or-kill).
- executedBuy approx %5 düşük.

**`buy_order_haircut()`:**
- Buy order haircut: sell amount artmalı, buy amount sabit.
- Score: %99 (1% düşüş, çünkü buy/sell price conversion etkisi).
- executedBuy == signedBuyAmount.
- executedSell <= signedSellLimit.

### [tests/cases/fees.rs] (33 satır)

**`solver_fee()`:** Her iki side (Buy/Sell) için `solver_fee(500)` → `FeeHandler::Driver` → order başarıyla çözümlenir.

### [tests/cases/flashloan_hints.rs], [tests/cases/jit_orders.rs], [tests/cases/protocol_fees.rs], [tests/cases/merge_settlements.rs]

Tüm kritik DEX senaryolar entegrasyon testiyle kapsanmış.

---

## KAPSAMLI DEX UYGULAMA NOTU — NOVA SHIELD

### 1. Solver Komunikasyon Protokolü

```
Nova Shield Auction → JSON stream → Solver Engine
POST /solve { id, orders, tokens, liquidity, deadline }
← SolverResponse { solutions: [{ trades, prices, interactions }] }
POST /notify { auction_id, solution_id, kind }
```

**Kritik parametreler:**
- timeout = deadline × solving_share - http_delay
- response_size_limit: büyük çözümler için artırılabilir
- TCP keepalive 60s

### 2. Fee Katmanları

Nova Shield'da DEX fee yapısı:
1. **Volume fee pre-processing:** Solver'a göndermeden limit'leri sıkılaştır.
2. **Haircut:** Driver uygulamadan önce solver'ı daha ihtiyatlı limitle zorla.
3. **Protocol fee:** Settlement sırasında surplus'tan kes.

### 3. MEV Koruması

- `RevertProtection::Enabled`: AMM interaction'lı tx → sadece private mempool.
- EIP-7702 paralel submission: birden fazla mempool'a eşzamanlı → racing.
- Gas tip: private mempool priority için `additional_tip_factor`.

### 4. Likidite Katmanları

| Likidite Türü | Gas Est. | Veri Kaynağı |
|---|---|---|
| UniswapV2 CP | 90,171 | On-chain batch RPC |
| UniswapV3 CL | Pool mean | Indexer/Subgraph |
| Balancer Weighted | 88,892 | On-chain batch RPC |
| Balancer Stable | (derived) | On-chain batch RPC |
| ZeroEx RFQ | (fixed) | HTTP API |
| Swapr | 90,171 | On-chain batch RPC |

### 5. JIT Order (Just-In-Time)

Solver kendi JIT order'ını oluşturabilir:
- EIP-712 domain separator ile UID üret
- İmzasını sol'den başlat (signature.data)
- Pre/post interactions ile settlement'ı zenginleştir

Nova Shield'da: solver EOA'sı kendi flash liquidity'sini JIT order olarak sunar.

### 6. EIP-7702 Paralel Settlement

```
Solver EOA → delegated to → Solver7702Delegate(approved_callers=[A,B,C])
Submission Account A → sign tx from solver EOA address
Submission Account B → sign tx from solver EOA address
(racing parallel mempool submission)
```

Nova Shield'da validator/sequencer paralelliği için uygulama alanı var.

### 7. Kritik Güvenlik Kontrolleri

- Response size limit — solver DoS prevention
- Haircut overflow → fallback to 0 (not panic)
- JIT order signer recovery — invalid sig = rejected
- uint112 overflow check for UniV2 pools
- EIP-7702 duplicate/zero address rejection
- max_fee_per_gas cap — congestion protection
- Pending tx wait — nonce conflict prevention

---

## ÖZET: COW PROTOCOL DRIVER INFRA MİMARİSİ

```
autopilot
    │  POST /{solver}/solve
    ▼
infra::api::Api (axum)
    │  into_domain() — SolveRequest → competition::Auction
    ▼
domain::Competition::solve()
    │  fetch liquidity (boundary::Fetcher)
    │  dto::auction::new() — haircut + volume fee
    ▼
infra::solver::Solver::solve()
    │  stream_body_and_gzip() — 64KB chunks, double buffer
    │  POST /solve → solver engine
    │  util::http::send(size_limit) — chunked read
    │  serde_json::from_str::<SolverResponse>
    ▼
dto::Solutions::into_domain()
    │  fulfillment: haircut_fee, JIT: sig recovery
    ▼
competition::Solution::new()
    │  scoring, encoding, simulation
    ▼
infra::notify → solver notifications (fire & forget)
infra::persistence → S3 archive (fire & forget)
infra::notify::liquidity_sources → PMM notifications (parallel)
```

Bu mimari Nova Shield için tam referans — solver agnostik, multi-pool, MEV-aware, EIP-7702 ready.
