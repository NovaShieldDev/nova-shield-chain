# CoW Protocol Analiz — Agent 14
## Kapsam: order-validation/ + cow-amm/ + solvers-dto/ + testlib/ + küçük crate'ler
## DEX Özel Protokolü: 3x Hassasiyet
## Tarih: 2026-07-22

---

## YÜRÜTME ÖZETİ

Bu agent şu crate'leri A'dan Z'ye analiz etti:
- `order-validation/` — 5 dosya (banned/ alt modülü dahil): DEX güvenliğinin kalbi
- `cow-amm/` — 6 dosya: CoW AMM implementasyonu, event indexing, pool lifecycle
- `solvers-dto/` — 4 dosya: Solver←→Driver iletişim katmanı DTO'ları
- `testlib/` — 4 dosya: Protokol genelinde test yardımcıları
- `token-info/` — 1 dosya: ERC20 token meta veri fetching
- `s3/` — 1 dosya: AWS S3 gzip upload soyutlaması
- `rate-limit/` — 1 dosya: Üstel geri çekilme (exponential backoff) rate limiter

**Toplam analiz edilen dosya: 22 kaynak dosyası**

---

# BÖLÜM 1: order-validation/

## Genel Mimari

`order-validation` crate'i DEX güvenliğinin merkezi omurgasıdır. Yalnızca "yasaklı kullanıcı tespiti" işlevi sunar. Kritik bir mimari karar olarak bu crate çok dar kapsamlı tutulmuştur — sadece ve sadece adres ban kontrolü yapar. Diğer validasyon mantıkları (bakiye, imza, expiry vb.) ayrı crate'lerdedir (orderbook, shared).

Ban sistemi üç katmanlı bir savunma mimarisi izler:
1. **Lokal HashSet** — anında, sıfır gecikme, yapılandırma zamanı listeleme
2. **Onchain Chainalysis Oracle** — OFAC-uyumlu zincir üstü kara liste
3. **Hermod (zeroShadow)** — off-chain HMAC-korumalı API, ek sanction veri kaynağı

Tüm uzak kaynaklar tek bir `Cached` wrapper ile ortak önbelleğe alınır.

---

### [lib.rs] (5 satır)

**Amaç:** Crate kök dosyası; sadece `banned` modülünü dışa açar.

**Struct/Enum/Trait Listesi:** (yok — sadece mod bildirimi)

**Kritik Tasarım Kararı:**
Order validation crate'inin bu kadar küçük tutulması bilinçli bir tasarım kararıdır. Crate, gelecekte genişletilebilir olmak üzere `banned` modülünü `pub mod` olarak açar. Diğer validation mantığı (`orderbook/src/order_validation.rs` gibi büyük dosyalarda) burada değil.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield'da benzer şekilde validation modüllerini işlev bazlı ayır: `banned/`, `expiry/`, `balance/`, `signature/` ayrı sub-crate veya modüller olsun
- Bu pattern sorumluluk ayrışmasını (SoC) maksimize eder ve her alt sistemi bağımsız test etmeyi sağlar

---

### [banned/mod.rs] (89 satır)

**Amaç:** Yasaklı kullanıcı sistemi ana koordinatörü — hardcoded liste + isteğe bağlı uzak backend'ler.

**Struct/Enum/Trait Listesi:**
- `Users` (satır 21):
  - `list: HashSet<Address>` — yapılandırma zamanı hard-coded kara liste (O(1) lookup)
  - `remote: Option<Arc<Cached>>` — isteğe bağlı uzak backend wrapper; `None` ise remote kontrolü devre dışı

**Validation Kuralları / Kritik Fonksiyonlar:**

- **`Users::new(contract, hermod, banned_users, cache_max_size)`** (satır 28-45):
  - Chainalysis Oracle contract varsa `Onchain` backend ekler
  - Hermod config varsa `Hermod` backend ekler
  - Backend yoksa `Cached::new` `None` döner → remote kontrol yok
  - `cache_max_size` moka cache kapasitesini sınırlar (bellek kontrolü)

- **`Users::none()`** (satır 48-53): Hiçbir şeyi ban etmez. Test/dev ortamlar için.

- **`Users::from_set(list)`** (satır 57-59): Sadece hardcoded liste, remote yok.

- **`Users::banned(addresses)`** (satır 63-88) — KRİTİK FONKSİYON:
  ```
  1. Sıfır adresini (burn address) filtrele — quote'larda kullanılıyor, check gereksiz
  2. Hardcoded listede olanlara -> banned'a ekle
  3. Geri kalanları remote.check(&need_lookup) ile sorgula
  4. Birleştirilmiş HashSet<Address> döndür
  ```
  - Return tipi `HashSet<Address>` — birden fazla adres toplu ban kontrolü için tasarlanmış
  - Async: remote IO için `await` var

**Güvenlik Analizi:**

- **Bypass riski (sıfır adres):** Satır 69-72 `address.is_zero()` kontrolü sıfır adresi skip eder. Bu bilinçli bir tasarım — CoW Protocol quote oluştururken bazen burn adresini receiver olarak kullanır. Kullanıcı tarafından kontrol edilemeyen iç adres olduğundan güvenli.
- **Performans:** Hardcoded liste O(1) HashSet lookup ile remote'dan önce kontrol edilir. Remote sadece "ihtiyaç duyulan" adresler için sorgulanır.
- **Fail-open riski:** Remote backend tamamen başarısız olursa (network hatası), `Cached` katmanı `None` döner ve o adres cache'e eklenmez. Bu "şüpheliyi geçir" davranışıdır. Sonraki sorguda yeniden denenir. Bu bilinçli bir available > consistent tercihidir.
- **OR mantığı:** Herhangi bir backend `true` dönerse adres banlıdır. Tek bir kaynağın yanlış pozitif dönmesi riski var — ancak bu Chainalysis ve Hermod'un kendi kalite güvencesine emanet edilmiştir.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield için bu üç katmanlı mimariyi benimse: hardcoded + on-chain oracle + off-chain API
- Sıfır adres muafiyeti pattern'ini koru — protocol-internal adresler ban kontrolüne girmemeli
- Toplu `banned(addresses)` API tasarımı kritik: order batch işleminde tüm owner/receiver adreslerini tek sorguda kontrol et
- Nova Shield'da ek katman ekle: **ülke bazlı coğrafi kısıtlama** (IP düzeyinde değil, on-chain identity bazlı) — FATF uyumluluğu için

---

### [banned/cached.rs] (189 satır)

**Amaç:** Tüm yasaklı kullanıcı backend'leri için ortak önbellek — cache miss'lerde tüm backend'lere paralel fan-out yapar.

**Struct/Enum/Trait Listesi:**

- `Entry` (satır 23-26):
  - `is_banned: bool` — ban durumu
  - `last_updated: Instant` — cache refresh zamanı

- `BackendError` (satır 28-35): Chainalysis veya Hermod hata enum'u

- **`Backend` trait** (satır 38-43) — KRİTİK SOYUTLAMA:
  ```rust
  async fn fetch(&self, address: Address) -> Result<bool, BackendError>;
  fn name(&self) -> &'static str;
  ```
  Tüm ban backend'leri bu trait'i implement eder. Pure fetcher — caching dışında.

- `Cached` (satır 47-50):
  - `backends: Vec<Box<dyn Backend>>` — tüm aktif backend'ler (runtime dispatch)
  - `cache: Cache<Address, Entry>` — moka LRU cache

**Sabitler:**
- `MAX_CONCURRENT_LOOKUPS: usize = 10` — aynı anda en fazla 10 cache miss backend'e gönderilebilir
- `CACHE_EXPIRY: Duration = 3600s (1 saat)` — cache giriş ömrü
- `MAINTENANCE_TIMEOUT: Duration = 60s` — background refresh aralığı

**Kritik Fonksiyonlar:**

- **`Cached::new(backends, max_capacity)`** (satır 54-64):
  - `backends` boşsa `None` döner — gereksiz cache oluşturmaz
  - `Arc::new` + `spawn_maintenance_task` — background refresh başlatır
  - Return: `Option<Arc<Self>>`

- **`Cached::check(addresses)`** (satır 68-102):
  ```
  1. Cache hit: direkt banned setine ekle
  2. Cache miss: need_lookup listesine al
  3. stream::iter(need_lookup) -> buffer_unordered(10) -> fetch_all(address) paralel
  4. Başarılıları cache'e yaz, banlıları returned sete ekle
  ```
  `buffer_unordered(MAX_CONCURRENT_LOOKUPS)` ile paralel ama bounded I/O.

- **`Cached::fetch_all(address)`** (satır 107-116) — KRİTİK MANTIK:
  ```
  Some(true)  ← herhangi bir backend true dönerse (BAN kesin)
  None        ← hiç true yok AMA en az bir backend hata verdi (belirsiz — cache'e yazma)
  Some(false) ← tüm backend'ler false döndü (temiz)
  ```
  Bu üç durumlu mantık çok önemli: hata durumunda "false" değil "belirsiz" dönülür, cache'e yazılmaz, bir sonraki sorguda yeniden denenir.

- **`Cached::expired(now)`** (satır 120-131): `CACHE_EXPIRY - MAINTENANCE_TIMEOUT` eşiğini aşan girişleri bulur. Yani expiry'den 60 saniye önce refresh başlar → hot cache.

- **`Cached::spawn_maintenance_task`** (satır 149-171):
  - `Weak<Self>` ile tutulur → son dış Arc drop edilince task durur (memory leak yok)
  - `MissedTickBehavior::Delay` — eğer maintenance süresi aşılırsa bir sonraki tick'i geciktirir, birikmiş tick'leri çalıştırmaz
  - Her 60 saniyede expired entry'leri refresh eder

- **`fetch_one(backend, address)`** (satır 175-188): Backend hatasını log'lar ve `None` döndürür. Hata `Some(false)` ile maskelenmez.

**Güvenlik Analizi:**

- **Race condition riski:** `fetch_all` paralel çalışır; aynı adres için birden fazla concurrent fetch başlayabilir (cache miss yarışı). Moka cache thread-safe olduğundan son yazan kazanır — bu tolerable. Worst case: aynı adres için birden fazla backend sorgusu.
- **Memory sınırı:** `max_capacity` parametresi moka'ya iletilir. Sınır aşılırsa moka en az kullanılan girişi siler (LRU/LFU politikası).
- **Weak pointer memory güvenliği:** Background task `Weak<Self>` kullanır, güçlü referans tutmaz. `Arc` drop edilirse task durur. Memory leak riski yok.
- **"Fail-uncertain" > "Fail-open":** Backend hatası durumunda ne `true` (yanlış ban) ne `false` (yanlış izin) dönülür — `None` dönülür. Bu doğru tercihtir.

**DEX Uygulama Notu — Nova Shield için:**
- Bu önbellek mimarisini kopyala: moka + background refresh + bounded paralel fetch
- `MAX_CONCURRENT_LOOKUPS = 10` sabitini yapılandırılabilir hale getir
- Nova Shield için cache expiry'yi daha kısa tut (30 dakika): sanction listeleri güncelleniyor
- `fetch_all` üç durumlu mantığı: Nova Shield'da da "belirsiz = cache'e yazma, yeniden dene" pattern'ini uygula
- Background maintenance task için Weak pointer pattern'ini mutlaka kullan — memory leak önleme

---

### [banned/hermod.rs] (134 satır)

**Amaç:** Hermod (zeroShadow) sanction API client — HMAC-SHA256 imzalı sorgular, HTTP 200/404 semantiği.

**Struct/Enum/Trait Listesi:**

- **`Config`** (satır 18-25):
  - `url: Url` — Hermod agent base URL (örn: `http://hermod:3000`)
  - `hmac_key: String` — per-müşteri HMAC anahtarı (adres gizleme)
  - `api_key: Option<String>` — Bearer token kimlik doğrulama

- **`Error`** (satır 27-33):
  - `Request(reqwest::Error)` — HTTP transport hatası
  - `UnexpectedStatus(StatusCode)` — 200 veya 404 dışı HTTP status kodu

- **`Client`** (satır 35-40):
  - `client: reqwest::Client` — 5 saniyelik timeout ile kurulmuş HTTP client
  - `url: Url` — normalize edilmiş base URL (trailing slash garantili)
  - `hmac_key: Vec<u8>` — byte olarak saklanan HMAC anahtarı
  - `api_key: Option<String>` — opsiyonel Bearer token

**Sabit:**
- `REQUEST_TIMEOUT: Duration = 5s` — Hermod sorguları için zaman aşımı

**Kritik Fonksiyonlar:**

- **`Client::new(config)`** (satır 43-60):
  - URL normalizasyonu: path sonu `/` ile bitiyorsa olduğu gibi bırak, bitmiyorsa ekle
  - Neden önemli: `url.join("addresses/abc")` trailing slash olmadan son segment'i değiştirir, eklenmiş segment oluşturmaz
  - `reqwest::Client::builder().timeout(5s)` — her istek için global timeout

- **`Client::sign(address)`** (satır 63-70) — GİZLİLİK MEKANİZMASI:
  ```
  payload = lowercase_hex_with_0x_prefix(address)  // "0xdeadbeef..." 42 chars
  mac = HMAC-SHA256(hmac_key, payload.as_bytes())
  return lowercase_hex(mac.finalize())  // 64 char hex string
  ```
  Adres doğrudan URL'ye koyulmaz; HMAC imzası gönderilir. Bu Hermod API'nin adres gizliliği modelidir — API sunucusu hangi adresin sorgulandığını bilmez (teorik olarak).

- **`Client::fetch_status(address)`** (satır 72-89):
  ```
  signature = sign(address)
  endpoint = base_url.join("addresses/{signature}")
  GET endpoint (+ Bearer token varsa)
  HTTP 200 -> Ok(true)  // BANLANDI
  HTTP 404 -> Ok(false) // temiz
  diğer    -> Err(UnexpectedStatus)
  ```

**Güvenlik Analizi:**

- **HMAC gizlilik modeli:** Adres yerine HMAC(address) gönderilir. Ancak HMAC anahtarı ele geçirilirse tüm adres sorgulama geçmişi açığa çıkabilir. Anahtar yönetimi kritik.
- **URL trailing slash bug'ı:** `url.join("addresses/sig")` — trailing slash olmadan Rust'ın url kütüphanesi son path segment'ini "silip" onun yerine koyar. Bunun için normalizasyon yapılmış. Test `base_url_without_trailing_slash_is_normalised` bunu doğrular.
- **Timeout:** 5 saniye HTTP timeout. Bu ban check akışının toplam latency'sini sınırlar. Yüksek trafikte yeterli mi? Bağlıdır. `MAX_CONCURRENT_LOOKUPS = 10` ile bounded.
- **API key:** `Option<String>` — yoksa Bearer header eklenmez. Üretim ortamında mutlaka kullanılmalı.

**Test Kapsamı (satır 103-133):**
- `hmac_signature_is_deterministic`: aynı adres → aynı 64 char hex imza
- `base_url_without_trailing_slash_is_normalised`: URL normalizasyon doğrulaması

**DEX Uygulama Notu — Nova Shield için:**
- HMAC adresi gizleme modelini benimse — sanction API'lerine ham adres gönderme
- Nova Shield'da birden fazla sanction provider destekle (Hermod + TRM Labs + Elliptic gibi)
- REQUEST_TIMEOUT = 5s uzun görünüyor; order akışının kritik path'indeyse 2-3s daha uygun
- HMAC anahtarı rotasyonu için bir mekanizma ekle (şu an statik)

---

### [banned/onchain.rs] (33 satır)

**Amaç:** Chainalysis Oracle akıllı sözleşme sorgusu — `isSanctioned(address)` view call.

**Struct/Enum/Trait Listesi:**

- **`Onchain`** (satır 13-15):
  - `contract: ChainalysisOracle::Instance` — alloy contract binding

**Kritik Fonksiyonlar:**

- **`Onchain::new(contract)`** (satır 18-20): Constructor, sadece contract binding saklar.

- **Backend impl** (satır 24-32):
  ```rust
  async fn fetch(&self, address: Address) -> Result<bool, BackendError> {
      Ok(self.contract.isSanctioned(address).call().await?)
  }
  ```
  Bir EVM view call. Sonuç `bool`. Ethereum mainnet üzerinde Chainalysis OFAC oracle sözleşmesi çağrılır.

**Güvenlik Analizi:**

- **On-chain oracle güvenilirliği:** Chainalysis Oracle sözleşmesinin güncel tutulması Chainalysis'in sorumluluğundadır. CoW Protocol bu sözleşmeye güvenir.
- **Reorg riski:** View call en son blokta çalışır. Reorg durumunda oracle durumu değişebilir. Bu tolerabl çünkü cache 1 saatlik bir pencere tutar.
- **Gas yok:** `call()` (view) kullanılır, gas harcaması yok. Ancak RPC latency ekler.
- **Single point of failure:** Chainalysis Oracle down ise `BackendError::Chainalysis` fırlatılır, `fetch_one` `None` döner. `fetch_all` mantığı gereği bu "belirsiz" sonuç verir, cache'e yazılmaz.

**DEX Uygulama Notu — Nova Shield için:**
- Nova Shield kendi zincirinde deploy edilecekse, Chainalysis-uyumlu bir on-chain sanction oracle deploy etmeyi değerlendirin
- Alternatif: zincir dışından periyodik sanction listesi güncellemesi yapan bir oracle (Chainlink Functions benzeri)
- Onchain oracle kullanımının avantajı: smart contract seviyesinde enforcement yapılabilir (settlement contract direkt kontrol edebilir)

---

# BÖLÜM 2: cow-amm/

## Genel Mimari

CoW AMM, klasik AMM modelini CoW Protocol'ün batch settlement mekanizmasına uyarlayan bir AMM implementasyonudur. Temel fark:

**Klasik AMM (Uniswap v2/v3):** Her swap anında gerçekleşir, CFMM formülü anında execute edilir, MEV frontrun/sandwich mümkündür.

**CoW AMM:** AMM batch settlement içinde bir "order" gibi davranır. Uniform clearing price kullanılır. AMM, referans fiyat vektörüne göre "bu fiyata rebalance etmek istiyorum" diyen bir GPv2Order üretir. Bu order batch'e dahil edilir. Batch optimizer (solver) bu order'ı diğer kullanıcı order'larıyla eşleştirir. AMM için MEV koruması otomatik gelir çünkü clearing price tüm batch için uniform.

**EIP-1271 Signature Mekanizması:**
CoW AMM, kullanıcı imzası olmayan bir "akıllı sözleşme imzası" kullanır. Settlement contract, AMM sözleşmesinin `isValidSignature` fonksiyonunu çağırarak order'ın geçerliliğini doğrular. Bu sayede AMM kodu kendi order'larını "imzalar".

---

### [lib.rs] (108 satır)

**Amaç:** Crate kök dosyası — public API ve EIP-712/EIP-1271 imzalama yardımcı fonksiyonları.

**Struct/Enum/Trait Listesi:**

- **`Metrics`** (satır 14-18):
  - `database_queries: HistogramVec` — `cow_amm_database_queries{type}` — DB query latency

- **`gpv2_order` modülü** (satır 33-107) — KRİTİK:
  - `TYPE_HASH: [u8; 32]` (satır 51-52) — GPv2Order EIP-712 type hash:
    ```
    keccak256("Order(address sellToken,address buyToken,address receiver,uint256 sellAmount,
    uint256 buyAmount,uint32 validTo,bytes32 appData,uint256 feeAmount,string kind,
    bool partiallyFillable,string sellTokenBalance,string buyTokenBalance)")
    ```
    Not: `kind`, `sellTokenBalance`, `buyTokenBalance` alanları EIP-712'de `string` tipinde (UX için) ama zincirde `bytes32` olarak saklanır.

**Kritik Fonksiyonlar:**

- **`eip712_hash_struct(order)`** (satır 55-59):
  ```
  hash = keccak256(TYPE_HASH || order.eip712_encode_data())
  ```
  Standart EIP-712 struct hash.

- **`generate_eip1271_signature(order, trading_params, amm_address)`** (satır 71-86) — KRİTİK:
  ```
  signature_data = ABI_encode_sequence(order.clone(), trading_params.clone())
  return amm_address.as_slice() ++ signature_data
  ```
  İmza formatı: `[20 byte AMM address][ABI-encoded order + trading params]`
  Settlement contract bu imzayı decode ederek AMM sözleşmesinin `isValidSignature` çağrısını yapar.

- **`generate_commit_interaction(order, amm, domain_separator)`** (satır 92-106):
  ```
  order_hash = eip712_hash_struct(order)
  full_hash = hashed_eip712_message(domain_separator, order_hash)
  calldata = amm.commit(full_hash)
  return InteractionData { target: amm_address, value: 0, call_data: calldata }
  ```
  "Commit" interaction: belirli bir order hash'ini AMM sözleşmesine kaydeder, yalnızca bu hash'in mevcut batch'te settle edilebilmesini sağlar. Bu tek kullanımlık commit mekanizması replay saldırılarını önler.

**Güvenlik Analizi:**

- **Replay koruması:** `commit` mekanizması kritik. Bir solver belirli bir order hash'ini commit ettiğinde, aynı batch içinde farklı bir order kullanılamaz. Bu MEV saldırılarına (özellikle "order substitution") karşı savunma sağlar.
- **EIP-712 type hash doğruluğu:** TYPE_HASH hard-coded sabit. CoW Protocol contracts v1.1.2 ile senkronize olması gerekir. Değişirse güncellenmeli.

**DEX Uygulama Notu — Nova Shield için:**
- EIP-1271 tabanlı AMM order signing modelini benimse: AMM sözleşmeleri kendi order'larını "imzalar"
- Commit mekanizması anti-replay için kritik: her batch için tek bir commit, sonraki batch'te geçersiz
- TYPE_HASH hardcoded yapma — versioned constants veya on-chain registry kullan

---

### [amm.rs] (204 satır)

**Amaç:** CoW AMM domain tipi — bir pool'u temsil eder, helper contract üzerinden template order üretir.

**Struct/Enum/Trait Listesi:**

- **`Amm`** (satır 17-21):
  - `helper: CowAmmLegacyHelper::Instance` — AMM helper contract binding
  - `address: Address` — AMM sözleşme adresi
  - `tradeable_tokens: Vec<Address>` — bu pool'un desteklediği token çifti (veya daha fazlası)

- **`TemplateOrder`** (satır 147-158):
  - `order: OrderData` — gerçek GPv2Order (sell/buy token, amounts, validity)
  - `signature: Signature` — EIP-1271 imzası (ilk 20 byte zaten çıkarılmış)
  - `pre_interactions: Vec<InteractionData>` — settlement'tan önce execute edilecek çağrılar
  - `post_interactions: Vec<InteractionData>` — settlement'tan sonra execute edilecek çağrılar

**Kritik Fonksiyonlar:**

- **`Amm::new(address, helper)`** (satır 24-35):
  ```
  tradeable_tokens = helper.tokens(address).call().await?
  ```
  Pool'un desteklediği token listesini on-chain helper'dan çeker.

- **`Amm::template_order(prices)`** (satır 50-53) — KRİTİK:
  ```
  order_return = helper.order(amm_address, prices).call().await?
  ```
  Helper contract'a referans fiyat vektörü gönderilir. Helper, AMM'in hangi yönde rebalance etmesi gerektiğini hesaplar ve bir GPv2Order döndürür. `prices` common denominator cinsinden verilmeli, `tradeable_tokens` sırasına uygun olmalı.

  **Matematiksel Model:** Helper contract içinde (bu crate'de değil, Solidity tarafında):
  - Pool `(R_0, R_1)` rezervleri ile `(p_0, p_1)` referans fiyatları verildiğinde
  - Eğer `R_0 * p_0 < R_1 * p_1`: pool token_0 satmalı (sell token_0, buy token_1)
  - Sell amount: `delta_0` öyle ki `(R_0 + delta_0) / (R_1 - delta_1) = p_0 / p_1`
  - Bu CFMM invariant'ını (`k = R_0 * R_1` sabit ürün) korurken uniform price'a uyum sağlar

- **`Amm::validated_template_order(prices, validator, domain_separator)`** (satır 58-81) — EK GÜVENLİK:
  ```
  template = self.template_order(prices).await?
  hash = hashed_eip712_message(domain_separator, template.order.hash_struct())
  validator.validate_signature_and_get_additional_gas(SignatureCheck { ... }).await?
  return template
  ```
  Helper contract'ın döndürdüğü imzayı gerçekten doğrular. "Buggy helper" saldırısına karşı: hatalı bir helper contract geçersiz imza döndürse de bu fonksiyon onu yakalar.

- **`Amm::convert_orders_reponse(order_return)`** (satır 107-142):
  Solidity `orderReturn` tipini domain `TemplateOrder`'a çevirir.
  - `kind` dönüşümü: hex-encoded bytes32 → `OrderKind` enum (satır 176-182)
    - `"f3b277..."` = OrderKind::Sell
    - `"6ed88e..."` = OrderKind::Buy
  - `sellTokenBalance` / `buyTokenBalance` dönüşümü: 3 tür (ERC20, Internal, External)
  - Signature: ilk 20 byte (AMM adresi) atlanır — settlement contract signer+sig birleşik format bekler ama domain katmanı ayrı tutar

**Sabitler (satır 184-186):**
```
BALANCE_ERC20:    "5a28e9363b..."
BALANCE_INTERNAL: "4ac99ace14..."
BALANCE_EXTERNAL: "abee3b7337..."
```
GPv2Order.sol'den alınan keccak256 hash'leri.

**Güvenlik Analizi:**

- **Helper contract güven modeli:** `template_order` helper contract'ın doğru çalıştığını varsayar. `validated_template_order` bunu on-chain signature validation ile doğrular. Production'da `validated_template_order` kullanılmalı.
- **Tip dönüşümü güvenliği:** `convert_kind` bilinmeyen bytes için `anyhow::bail!` fırlatır. Sözleşme değişirse bu dönüşüm başarısız olur — doğru davranış.
- **Signature format:** İlk 20 byte çıkarma — settlement contract formatı ile domain model formatı uyumsuzluğu yönetilmiş. Fragile ancak iyi dokümante edilmiş.

**DEX Uygulama Notu — Nova Shield için:**
- CoW AMM matematiksel modeli Nova Shield DEX için doğrudan uygulanabilir
- `validated_template_order` pattern'ini kullan: helper contract output'unu her zaman doğrula
- `prices` vektörü = dış fiyat kaynağı (oracle). Nova Shield için: Pyth, Chainlink, veya kendi TWAP oracle'ı
- Pre/post interaction mekanizması güçlü: AMM rebalance öncesi/sonrası özel mantık çalıştırılabilir (flashloan, hedge işlemleri vb.)

---

### [cache.rs] (244 satır)

**Amaç:** CoW AMM event indexing depolama katmanı — in-memory cache + PostgreSQL persistence.

**Struct/Enum/Trait Listesi:**

- **`Storage`** (satır 18): `Arc<Inner>` wrapper — clone edilebilir shared storage

- **`Inner`** (satır 109-124):
  - `cache: RwLock<HashMap<u64, Vec<Arc<Amm>>>>` — block_number → [Amm] mapping
  - `start_of_index: u64` — deployment block - 1 (bu bloktan önce event olmaz)
  - `factory_address: Address` — hangi factory bu storage'a ait
  - `helper: CowAmmLegacyHelper::Instance` — AMM sorguları için
  - `db: PgPool` — PostgreSQL bağlantı havuzu

**Kritik Fonksiyonlar:**

- **`Storage::new(deployment_block, helper, factory_address, db)`** (satır 21-45):
  - `start_of_index = deployment_block - 1` — deployment bloğu dahil edilsin diye -1
  - `initialize_from_database().await` — başlangıçta DB'den cache'i doldur
  - Hata olursa `tracing::error!` ile logla ama panic etme — tolerant initialization

- **`Storage::initialize_from_database()`** (satır 47-91):
  ```
  factory_address -> fetch_by_factory_address (DB)
  her db_amm için: Amm::new(address, helper).await  [paralel]
  cache'e block_number anahtarıyla ekle
  ```
  `try_join_all` ile paralel AMM initialization. DB'de 100 AMM varsa hepsi aynı anda yüklenir.

- **`EventStoring::replace_events(events, range)`** (satır 128-155):
  ```
  1. DB'den o block range'deki AMM'leri sil (reorg handling)
  2. Cache'den o block range'deki key'leri sil
  3. append_events(events) çağır
  ```
  Bu "replace" semantiği blockchain reorg'larını handle etmek için: reorg olan bloklar yeniden işlenir.

- **`EventStoring::append_events(events)`** (satır 157-223) — KRİTİK:
  ```
  for (event, log) in events:
    block_number, tx_hash = log meta data
    CowAmmEvent::COWAMMPoolCreated(cow_amm) = event
    Amm::new(cow_amm_address, helper).await:
      TransportError -> return Err (abort, retry entire range)
      Other error -> continue (bu AMM atla, helper desteklemiyor)

  upsert_batched(db_amms) -> DB'ye yaz
  cache'e ekle
  ```
  **Önemli hata ayrımı:** Transport error (network) → abort → tüm range yeniden dene. Logic error (helper uyumsuzluğu) → bu AMM'i atla, devam et. Bu ayrım tutarlılık için kritik.

- **`EventStoring::last_event_block()`** (satır 225-232):
  DB'den son indexed block'u çeker. Yoksa `start_of_index` döndürür. Restart'lar arası state korunur.

**Güvenlik Analizi:**

- **Reorg güvenliği:** `replace_events` tam reorg semantiği implement eder: DB + cache temizle, yeniden işle. Bu tutarlılığı garanti eder.
- **Concurrency:** `RwLock<HashMap>` — okuma için read lock (paralel), yazma için write lock (exclusive). Çok sayıda concurrent reader desteklenir.
- **Partial failure:** `upsert_batched` transaction içinde — ya hepsi yazılır ya hiçbiri. Tutarlılık korunur.
- **Transport error abort:** Network hatası durumunda abort etmek ve tüm range'i yeniden denemek doğru karar — kısmi index bozuk state bırakır.

**DEX Uygulama Notu — Nova Shield için:**
- Block_number keyed cache pattern'ini kullan — reorg handling basitleşir
- `start_of_index = deployment_block - 1` trick'ini kopyala — off-by-one hataları önler
- Transport error vs. logic error ayrımını uygula: network hataları retry, logic hataları skip

---

### [factory.rs] (33 satır)

**Amaç:** CoW AMM factory sözleşmesinden event'leri okumak için Alloy event retriever implementasyonu.

**Struct/Enum/Trait Listesi:**

- **`Factory`** (satır 14-17):
  - `provider: AlloyProvider`
  - `address: Address` — factory sözleşme adresi

**Kritik Fonksiyonlar:**

- **`AlloyEventRetrieving::filter()`** (satır 22-29):
  ```rust
  Filter::new()
      .address(self.address)
      .event_signature(FilterSet::from_iter([
          CowAmmLegacyHelper::COWAMMPoolCreated::SIGNATURE_HASH,
      ]))
  ```
  Sadece bu factory adresinden `COWAMMPoolCreated` event'ini dinler. Diğer event'ler filtreden geçmez.

**Güvenlik Analizi:**
- Event filter adresi + event signature ile çift filtreleme: başka sözleşmelerden gelen aynı event signature'lı event'ler ignored.
- `AlloyEventRetrieving` trait implementasyonu — framework `filter()` ve `provider()` kullanarak event'leri çeker.

**DEX Uygulama Notu — Nova Shield için:**
- Bu pattern Nova Shield pool registry için idealdir: her factory türü için ayrı `Factory` struct, ortak `EventHandler` altyapısı
- `FilterSet` birden fazla event signature eklenebilir — pool upgrade event'leri de izlenebilir

---

### [maintainers.rs] (91 satır)

**Amaç:** Sıfır bakiyeli AMM pool'larını registry'den temizleyen background maintainer.

**Struct/Enum/Trait Listesi:**

- **`EmptyPoolRemoval`** (satır 14-17):
  - `storage: Arc<RwLock<Vec<Storage>>>` — tüm factory'lerin storage'larına erişim
  - `provider: AlloyProvider` — ERC20 balance sorgusu için

**Kritik Fonksiyonlar:**

- **`EmptyPoolRemoval::has_zero_balance(amm)`** (satır 25-48):
  ```
  futures = amm.traded_tokens().iter().map(|token|
    ERC20.balanceOf(amm_address).call()
    -> balance == 0 => Ok(())
    -> balance > 0  => Err(())
  )
  select_ok(futures)  // herhangi bir token'ın bakiyesi sıfırsa true
  ```
  `select_ok`: herhangi bir future `Ok` dönerse diğerleri iptal edilir. Pool'un herhangi bir token'ının bakiyesi sıfırsa pool "boş" sayılır. Bu optimistic bir tasarım.

- **`Maintaining::run_maintenance()`** (satır 53-84):
  ```
  1. Tüm storage'lardan AMM listesi topla
  2. FuturesUnordered ile paralel balance kontrol
  3. Boş AMM'leri tespit et
  4. storage.remove_amms(empty_amms) — tüm storage'lardan çıkar
  ```
  `FuturesUnordered` → unordered concurrent execution, sonuçlar gelince işle.

**Güvenlik Analizi:**

- **"Any zero" semantiği:** Bir token bakiyesi sıfırsa pool "boş". Bu agresif bir policy — iki token var, biri sıfır olunca bile çıkarılıyor. Mantıklı: AMM iki tokeni de ihtiyaç duyar.
- **select_ok semantiği:** İlk Ok gelince diğerleri iptal — en hızlı "evet, boş" yanıtı için optimize edilmiş.
- **Memory consistency:** `storage.remove_amms` write lock alır — okuma yarışı yok.

**DEX Uygulama Notu — Nova Shield için:**
- Empty pool removal pattern'ini uygula: boş havuzlar gas israfı ve kullanıcı deneyimi sorunlarına yol açar
- "Any zero" yerine "all zero" semantiğini de düşün: her iki token bakiyesi de sıfırsa çıkar, daha conservative
- Bu maintainer'ı periyodik tetikleme ile değil, yeni blok event'leriyle tetikle: daha reaktif

---

### [registry.rs] (98 satır)

**Amaç:** Tüm CoW AMM factory'lerinin event indexing ve AMM listesinin ana koordinatörü.

**Struct/Enum/Trait Listesi:**

- **`Registry`** (satır 19-23):
  - `block_retriever: Arc<BlockRetriever>` — blok aralığı çekme altyapısı
  - `storage: Arc<RwLock<Vec<Storage>>>` — tüm factory storage'ları (birden fazla factory desteklenir)
  - `maintenance_tasks: Vec<Arc<dyn Maintaining>>` — background task'lar

**Kritik Fonksiyonlar:**

- **`Registry::add_listener(deployment_block, factory, helper_contract, db)`** (satır 43-72):
  ```
  1. Storage::new(deployment_block, helper, factory, db).await — cache + DB init
  2. storage.push(storage.clone())
  3. Factory { provider, address: factory } — event filter
  4. EventHandler::new(block_retriever, factory, storage, None)
  5. EmptyPoolRemoval::new(storage_arc, provider)
  6. maintenance_tasks.push(Mutex::new(event_handler))
  7. maintenance_tasks.push(token_balance_maintainer)
  ```
  Her factory için iki background task: event indexer + empty pool cleaner.

- **`Registry::amms()`** (satır 76-83):
  Tüm storage'lardan AMM listesi toplar. `Vec<Arc<Amm>>`.

- **`Registry::maintenance_tasks()`** (satır 85-87):
  Background task listesi. Caller bu task'ları tokio runtime'a schedule eder.

**Güvenlik Analizi:**

- **Çoklu factory desteği:** `Vec<Storage>` — birden fazla factory (örn. farklı versiyonlar) aynı registry'de yönetilebilir.
- **`Mutex<EventHandler>`:** EventHandler mutable state gerektirir (son indexed block). `Arc<Mutex<>>` ile `Maintaining` trait object halinde saklanır.
- **Debug impl:** Manuel derive — `block_retriever` ve `storage` debug, maintenance_tasks değil (trait object debug gösterilemez).

**DEX Uygulama Notu — Nova Shield için:**
- Multi-factory registry pattern'i: Nova Shield'da v1/v2 pool versiyonları aynı registry'de yönetilebilir
- `maintenance_tasks()` → caller schedule → Nova Shield'da da bu inversion of control iyi
- `deployment_block - 1` başlangıç bloğu: yeni factory eklendiğinde dogru bloktan başlamak kritik

---

# BÖLÜM 3: solvers-dto/

## Genel Mimari

`solvers-dto`, driver ile solver engine arasındaki JSON RPC arayüzünü tanımlar. Solver'lar `/solve` endpoint'inde `Auction` alır, `SolverResponse` döndürür. Driver da solvers'a `Notification` gönderir (settlement sonuçları, hatalar vb.).

---

### [lib.rs] (7 satır)

**Amaç:** Crate kök — `auction`, `notification`, `solution` modüllerini dışa açar.

---

### [auction.rs] (327 satır)

**Amaç:** Solver'a gönderilen auction nesnesi ve tüm bağımlı tipler.

**Struct/Enum/Trait Listesi:**

- **`Auction`** (satır 17-27) — KRİTİK:
  - `id: Option<i64>` — auction ID (DisplayFromStr serializasyon)
  - `tokens: HashMap<Address, Token>` — token metadata (decimals, symbol, price, balance, trusted flag)
  - `orders: Vec<Order>` — solver'ın çözebileceği order listesi
  - `liquidity: Vec<Liquidity>` — mevcut likidite kaynakları (AMM pool'ları, limit order'lar)
  - `effective_gas_price: U256` — gas fiyatı (solver cost estimation için)
  - `deadline: DateTime<Utc>` — solver'ın yanıt vermesi gereken son zaman
  - `surplus_capturing_jit_order_owners: Vec<Address>` — JIT order surplus'u capture edebilen owner'lar

- **`Order`** (satır 30-66) — KRİTİK:
  - `uid: [u8; 56]` — 56 byte order UID (32 byte hash + 20 byte owner + 4 byte valid_to)
  - `sell_token / buy_token: Address`
  - `sell_amount / full_sell_amount: U256` — effective sell amount vs. maksimum (fee sonrası)
  - `buy_amount / full_buy_amount: U256` — effective buy amount vs. minimum
  - `fee_policies: Option<Vec<FeePolicy>>` — surplus/volume/priceImprovement fee politikaları
  - `valid_to: u32` — Unix timestamp expiry
  - `kind: Kind` — Sell veya Buy
  - `receiver: Option<Address>` — işlem çıktısının gönderileceği adres (varsayılan: owner)
  - `owner: Address` — imzalayan adres
  - `partially_fillable: bool` — kısmi doldurma izni
  - `pre_interactions / post_interactions: Vec<InteractionData>` — order bazlı etkileşimler
  - `sell_token_source / buy_token_destination` — token kaynak/hedef tipi
  - `class: Class` — Market veya Limit
  - `app_data: AppDataHash` — 32 byte app metadata hash
  - `flashloan_hint: Option<FlashloanHint>` — solver için flashloan isteği ipucu
  - `wrappers: Vec<WrapperCall>` — order execution için gerekli wrapper çağrıları
  - `signing_scheme / signature` — imzalama tipi ve ham imza

- **`BuyTokenDestination`** (satır 70-81):
  - `Erc20` — ERC20 transfer
  - `Internal` — DEPRECATED (Balancer Vault artık kullanılmıyor)

- **`SellTokenSource`** (satır 83-101):
  - `Erc20` — direkt ERC20 allowance
  - `External` / `Internal` — DEPRECATED

- **`Token`** (satır 164-175):
  - `decimals: Option<u8>`
  - `symbol: Option<String>`
  - `reference_price: Option<U256>` — solver için fiyat referansı
  - `available_balance: U256` — settlement contract'taki mevcut bakiye
  - `trusted: bool` — token güven durumu (kritik: untrusted token bazı solver'larca ignore edilebilir)

- **`Liquidity`** (satır 177-185) — enum (tag: "kind"):
  - `ConstantProduct(ConstantProductPool)` — Uniswap v2 benzeri
  - `WeightedProduct(WeightedProductPool)` — Balancer v2 benzeri
  - `Stable(StablePool)` — Curve benzeri (amplification parametresi dahil)
  - `ConcentratedLiquidity(ConcentratedLiquidityPool)` — Uniswap v3 benzeri
  - `LimitOrder(ForeignLimitOrder)` — harici limit order (0x, 1inch vb.)

- **`ConstantProductPool`** (satır 187-198):
  - `id / address / router / gas_estimate / tokens: HashMap<Address, ConstantProductReserve> / fee`
  - `ConstantProductReserve`: sadece `balance: U256`
  - Formül: `x * y = k` (sabit ürün), `fee` işlem ücreti

- **`WeightedProductPool`** (satır 208-230):
  - `tokens: HashMap<Address, WeightedProductReserve>` — her token için `balance + scaling_factor + weight`
  - `amplification_parameter` yok — Balancer weighted
  - Formül: `Π(x_i^w_i) = k` (ağırlıklı ürün invariant)

- **`StablePool`** (satır 240-260):
  - `amplification_parameter: BigDecimal` — A parametresi (Curve StableSwap)
  - Formül: `A * n^n * Σ(x_i) + D = A * D * n^n + D^(n+1) / (n^n * Π(x_i))`

- **`ConcentratedLiquidityPool`** (satır 262-282):
  - `sqrt_price: U256` — mevcut √P (Q64.96 fixed point)
  - `liquidity: u128` — aktif likidite miktarı
  - `tick: i32` — mevcut aktif tick
  - `liquidity_net: Arc<BTreeMap<i32, i128>>` — her tick'teki net likidite değişimi
  - `Arc<BTreeMap<...>>` — driver'daki domain pool ile paylaşılan pointer (derin kopya önlenir)
  - Formül: `x * y = L^2` — tick aralıkları içinde, `√P_lower ≤ √P ≤ √P_upper`

- **`ForeignLimitOrder`** (satır 284-302):
  - `maker_token / taker_token / maker_amount / taker_amount / taker_token_fee_amount`
  - Harici protokol (0x, 1inch, Paraswap) limit order'ı

- **`FlashloanHint`** (satır 304-314):
  - `liquidity_provider / protocol_adapter / receiver / token / amount`

- **`FeePolicy`** (satır 137-150):
  - `Surplus { factor, max_volume_factor }` — surplus'un `factor` oranı ücret
  - `PriceImprovement { factor, max_volume_factor, quote }` — quote'dan iyileşmenin `factor` oranı
  - `Volume { factor }` — işlem hacminin `factor` oranı

**Güvenlik Analizi:**

- **`trusted: bool`:** Güvenilmeyen token'lar (honeypot, fee-on-transfer, rebase vb.) bazı solver'larca dahil edilmeyebilir. Bu flag kritik güvenlik metadata'sı.
- **`Arc<BTreeMap>` paylaşımı:** Tick map büyük olabilir (binlerce tick); shared pointer ile derin kopya maliyeti ortadan kalkar. Thread-safe çünkü `Arc<BTreeMap>` immutable.
- **Deprecated alanlar:** `#[deprecated]` ile işaretlenmiş Balancer Vault kaynak tipleri hala deserialize edilebilir (backward compat) ama serializasyonda uyarı verir.

**DEX Uygulama Notu — Nova Shield için:**
- Bu `Auction` yapısı Nova Shield'ın kendi solver API'si için mükemmel referans
- `Token.trusted` flag'ini uygula — güvenilmeyen token'lar için özel validation pipeline
- `ConcentratedLiquidityPool.liquidity_net` için `Arc<BTreeMap>` paylaşımı: solver'a her batch'te tam kopya göndermek yerine shared state kullan
- `FeePolicy` enum'u: üç farklı ücret modeli. Nova Shield için hangisi uygun? Önerim: `Surplus` + `Volume` kombinasyonu
- `surplus_capturing_jit_order_owners`: Nova Shield'da hangi varlıkların JIT surplus capture edeceği yönetişim sorusu

---

### [notification.rs] (87 satır)

**Amaç:** Driver'dan solver'a gönderilen settlement sonuç bildirimleri.

**Struct/Enum/Trait Listesi:**

- **`Notification`** (satır 13-19):
  - `auction_id: Option<i64>`
  - `solution_id: Option<SolutionId>`
  - `kind: Kind` — flatten: tüm variant alanları Notification seviyesine açılır

- **`SolutionId`** (satır 21-27) — untagged enum:
  - `Single(u64)` — tek solution ID
  - `Merged(Vec<u64>)` — birleştirilmiş solution'lar

- **`Kind`** (satır 29-72) — KRİTİK ENUM: 16 variant, her biri farklı bir settlement sonucu:
  - `Timeout` — solver deadline'ı geçti
  - `EmptySolution` — solver boş çözüm döndürdü
  - `DuplicatedSolutionId` — aynı ID'li iki çözüm
  - `SimulationFailed { block, tx, succeeded_once }` — tx simülasyonu başarısız
  - `InvalidClearingPrices` — geçersiz uniform clearing price
  - `MissingPrice { token_address }` — token için fiyat bulunamadı
  - `InvalidExecutedAmount` — geçersiz execute miktarı
  - `NonBufferableTokensUsed { tokens }` — buffer edilemeyen token kullanımı
  - `SolverAccountInsufficientBalance { required }` — solver hesabında yetersiz bakiye
  - `SettlementStarted` — settlement başladı
  - `Success { transaction }` — başarılı settlement, tx hash
  - `Revert { transaction }` — reverted settlement, tx hash
  - `DriverError { reason }` — driver iç hatası
  - `Cancelled` — iptal edildi
  - `Expired` — süresi doldu
  - `Fail` — genel başarısızlık
  - `PostprocessingTimedOut` — settlement sonrası işlem zaman aşımı
  - `DeserializationError { reason }` — solver yanıtı deserialize edilemedi

- **`Tx`** (satır 77-87):
  - `from / to: Address`
  - `input: Vec<u8>` — calldata
  - `value: U256`
  - `access_list: AccessList`

**Güvenlik Analizi:**

- **`SimulationFailed.succeeded_once`:** Eğer bir önceki simülasyon başarılıysa ama şimdiki başarısızsa bu flag `true`. Bu, blok değişikliği kaynaklı geçici başarısızlık ile solver hatası arasında ayrım yapmak için kullanılır.
- **`NonBufferableTokensUsed`:** Settlement contract'ın internal buffer'larında tutamayacağı token'lar varsa settlement başarısız olur. Solver bu token'ları kullanmamalı.
- **`Revert` vs `Fail`:** İkisi de başarısızlık ama `Revert`'te tx hash var (zincire gönderildi, revert oldu), `Fail`'de yok.

**DEX Uygulama Notu — Nova Shield için:**
- Bu notification enum'u Nova Shield solver ekosistemi için birebir kullanılabilir
- `SimulationFailed.succeeded_once` logic'ini uygula — blok kaynaklı geçici hatalar solver ceza puanına eklenmemeli
- Solver scoring/jailing sistemi için bu notificationlar veri kaynağı olmalı

---

### [solution.rs] (349 satır, testler dahil)

**Amaç:** Solver'ın driver'a döndürdüğü çözüm nesneleri.

**Struct/Enum/Trait Listesi:**

- **`SolverError`** (satır 11-15):
  - `code: SolverErrorCode`
  - `message: Option<String>`

- **`SolverErrorCode`** (satır 17-28):
  - `TradingOutsideAllowedWindow` — zaman kısıtlı token (RWA vb.)
  - `TokenTemporarilySuspended` — geçici askı
  - `InsufficientLiquidity` — yetersiz likidite
  - `Other` — genel hata

- **`SolverResponse`** (satır 30-43) — untagged enum:
  - `Solutions { solutions: Vec<Solution> }` — normal çözümler
  - `Error { error: SolverError }` — solver başarısız oldu

- **`Solution`** (satır 45-66) — KRİTİK:
  - `id: u64` — çözüm ID
  - `prices: HashMap<Address, U256>` — uniform clearing price vektörü
  - `trades: Vec<Trade>` — fulfillment ve JIT trade'leri
  - `pre_interactions: Vec<Call>` — settlement öncesi global etkileşimler
  - `interactions: Vec<Interaction>` — ana settlement etkileşimleri (liquidity routing)
  - `post_interactions: Vec<Call>` — settlement sonrası global etkileşimler
  - `gas: Option<u64>` — solver gas tahmini
  - `gas_fee_override: Option<GasFeeOverride>` — EIP-1559 fee override
  - `flashloans: Option<HashMap<OrderUid, Flashloan>>` — order bazlı flashloan
  - `wrappers: Vec<WrapperCall>` — gerekli wrapper çağrıları

- **`Trade`** (satır 72-77) — enum:
  - `Fulfillment(Fulfillment)` — mevcut kullanıcı order'ının doldurulması
  - `Jit(JitTrade)` — just-in-time solver tarafından oluşturulan order

- **`Fulfillment`** (satır 79-89):
  - `order: OrderUid` — hangi order dolduruldu
  - `executed_amount: U256` — gerçekte execute edilen miktar
  - `fee: Option<U256>` — opsiyonel solver-specified fee

- **`JitTrade`** (satır 91-101):
  - `order: JitOrder` — JIT order detayları
  - `executed_amount: U256`
  - `fee: Option<U256>`

- **`JitOrder`** (satır 103-125) — Tam order yapısı (JIT = "Just In Time"):
  - `sell_token / buy_token / receiver / sell_amount / buy_amount`
  - `partially_fillable: bool` (default false)
  - `valid_to / app_data / kind`
  - `sell_token_balance / buy_token_balance / signing_scheme / signature`
  - JIT order'lar solver tarafından batch'e eklenir — kullanıcıdan değil

- **`Interaction`** (satır 134-138) — enum:
  - `Liquidity(LiquidityInteraction)` — bilinen likidite kaynağı
  - `Custom(CustomInteraction)` — özel calldata

- **`LiquidityInteraction`** (satır 152-163):
  - `internalize: bool` — settlement contract buffer'larından çözülebilir mi?
  - `id / input_token / output_token / input_amount / output_amount`

- **`CustomInteraction`** (satır 165-180):
  - `internalize: bool`
  - `target / value / calldata`
  - `allowances: Vec<Allowance>` — gerekli ERC20 onayları
  - `inputs / outputs: Vec<Asset>` — beklenen token akışları (simülasyon doğrulaması için)

- **`GasFeeOverride`** (satır 254-262):
  - `max_fee_per_gas / max_priority_fee_per_gas: U256` — EIP-1559 fee parametreleri

- **`Flashloan`** (satır 264-273): `liquidity_provider / protocol_adapter / receiver / token / amount`

**Matematiksel Not — Uniform Clearing Price:**
```
prices: HashMap<Address, U256>

Her trade için:
  sell_amount_cleared = executed_amount * prices[buy_token] / prices[sell_token]

Settlement constraint (sell order için):
  sell_amount_cleared <= order.sell_amount  (kullanıcı en fazla bu kadar satar)
  executed_buy_amount >= order.buy_amount   (kullanıcı en az bu kadar alır)

Surplus:
  sell order: surplus = order.sell_amount - sell_amount_cleared
  buy order:  surplus = executed_buy_amount - order.buy_amount
```

**Test Kapsamı (satır 287-348):**
- `serializes_empty_solutions_response`: `SolverResponse::default()` → `{"solutions": []}`
- `serializes_and_deserializes_error_responses`: Tüm 4 error code'u test eder, JSON roundtrip

**Güvenlik Analizi:**

- **`internalize: bool`:** Eğer `true`, driver bu interaction'ı settlement calldata'sına dahil etmez ama token bakiyelerini settlement contract'ın internal buffer'larından yönetir. Bu gas optimizasyonu ama yanlış kullanılırsa token kaybolabilir.
- **`CustomInteraction.inputs/outputs`:** Driver, simülasyon sırasında beklenen token akışlarını doğrular. Solver yanlış beyan ederse simülasyon başarısız olur. Bu solver dürüstlük mekanizması.
- **`JitTrade`:** Solver kendi order'ını batch'e ekler. Bu MEV alımının meşrulaştırılmış formu — CoW Protocol'de solver surplus capture edebilir. Nova Shield'da bu yetki dikkatle kontrol edilmeli.

**DEX Uygulama Notu — Nova Shield için:**
- `Solution.prices` uniform clearing price: Nova Shield DEX'in temel prensibi bu olmalı — aynı batch'teki herkes aynı fiyattan işlem yapar
- `CustomInteraction.inputs/outputs` doğrulama pattern'i: solver çözümlerinin simülasyon öncesi token akışlarını beyan etmesi güvenlik için kritik
- JIT order sistemi: Nova Shield'da hangi varlıkların JIT order ekleyebildiği açıkça tanımlanmalı — sadece whitelist'teki solver'lar
- `flashloans: HashMap<OrderUid, Flashloan>`: flash loan ile settlement — Nova Shield DEX'te büyük order'lar için gerekli likidite sağlayabilir

---

# BÖLÜM 4: testlib/

---

### [lib.rs] (5 satır)

**Amaç:** Test yardımcı modülleri kök dosyası.

---

### [json_matching.rs] (375 satır)

**Amaç:** JSON karşılaştırma makroları ve DFS karşılaştırma fonksiyonu.

**Struct/Enum/Trait Listesi:** (struct yok — sadece fonksiyonlar ve makrolar)

**Kritik Fonksiyonlar:**

- **`assert_json_matches!(actual, expected)`** makro (satır 79-95):
  Exclude path'siz tam eşitlik kontrolü. `json_matches_excluding` çağırır.

- **`assert_json_matches_excluding!(actual, expected, exclude_paths)`** makro (satır 30-45):
  Belirtilen path'ler ignore edilerek kısmi JSON karşılaştırma.

- **`parse_field_paths(paths: &[&str])`** (satır 98-103):
  `["user.profile.name"]` → `HashSet<Vec<String>>` — DFS path tracking için.

- **`json_matches_excluding(actual, expected, exclude_paths)`** (satır 107-184) — KRİTİK:
  DFS traversal. İç yardımcı `compare_jsons`:
  ```
  Object vs Object:
    - Her key için current_path'e push
    - exclude_paths contains current_path -> skip (backtrack)
    - recursion
    - her iki objede de yoksa: unreachable!()
    - birinde varsa: "Key missing in actual/expected at X" hatası

  Diğer değerler:
    - actual != expected -> "Mismatch at X: A != B"
  ```
  `current_path: &mut Vec<String>` accumulator — backtracking ile güncellenir.

**Test Kapsamı (satır 186-374):**
- `test_parse_field_paths`: path parsing doğrulaması
- `test_json_matches_excluding_no_exclusions`: tam eşitlik
- `test_json_matches_excluding_with_exclusions`: timestamp ve enabled field ignore
- `test_json_matches_excluding_failure`: `#[should_panic]` — "name" farklı ise panic
- `test_json_matches_excluding_key_is_missing`: "Key missing in expected" mesajı
- `test_json_matches_excluding_key_is_missing_reversed`: "Key missing in actual"

**Güvenlik Analizi:** N/A — test utility.

**DEX Uygulama Notu — Nova Shield için:**
- Bu JSON matching kütüphanesini test altyapısına dahil et
- Özellikle `assert_json_matches_excluding!` API response testlerinde kritik: timestamp gibi dinamik alanları exclude ederek geri kalanı kesin test et
- `current_path.pop()` pattern — immutable backtracking. Rust'ta owned Vec üzerinde backtracking için doğru pattern

---

### [protocol.rs] (13 satır)

**Amaç:** Mainnet protokol sözleşme adresleri sabitleri.

**Sabitler:**
- `SETTLEMENT: Address = 0x9008D19f58AAbD9eD0D60971565AA8510560ab41` — GPv2Settlement
- `RELAYER: Address = 0xC92E8bdf79f0507f65a392b0ab4667716BFE0110` — VaultRelayer
- `AUTHENTICATOR: Address = 0x2c4c28DDBdAc9C5E7055b4C863b72eA0149D8aFE` — GPv2AllowListAuthentication

**DEX Uygulama Notu — Nova Shield için:**
- Test dosyalarında hardcoded adres yerine bu pattern'i kullan: merkezi sabitler dosyası
- Nova Shield mainnet deploy edildiğinde kendi `protocol.rs` konstantları oluştur

---

### [tokens.rs] (46 satır)

**Amaç:** Mainnet token adresleri sabitleri.

**Sabitler:**
- `WETH: 0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2`
- `COW: 0xDEf1CA1fb7FBcDC777520aa7f396b4E015F497aB`
- `GNO: 0x6810e776880c02933d47db1b9fc05908e5386b96`
- `OWL: 0x1A5F9352Af8aF974bFC03399e3767DF6370d82e4`
- `BAL: 0xba100000625a3754423978a60c9317c58a424e3D`
- `DAI: 0x6B175474E89094C44Da98b954EedeAC495271d0F`
- `USDC: 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48`
- `USDT: 0xdAC17F958D2ee523a2206206994597C13D831ec7`
- `COMP: 0xc00e94Cb662C3520282E6f5717214004A7f26888`
- `MKR: 0x9f8F72aA9304c8B593d555F12eF6589cC3A579A2`
- `WBTC: 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599`
- `BNT: 0x1F573D6Fb3F13d689FF844B4cE37794d79a7FF1C`
- `STETH: 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84`
- `SUSDE: 0x9D39A5DE30e57443BfF2A8307A4256c8797A3497`

**DEX Uygulama Notu — Nova Shield için:**
- Test token adresleri merkezi sabitlere taşı — testler arasında tutarlılık

---

# BÖLÜM 5: Küçük Crate'ler

---

### [token-info/src/lib.rs] (232 satır)

**Amaç:** ERC20 token metadata (decimals, symbol) çekme ve önbellekleme.

**Sabitler:**
- `TOKEN_INFO_FETCH_CONCURRENCY: usize = 32` — paralel token fetch limiti

**Struct/Enum/Trait Listesi:**

- **`TokenInfo`** (satır 22-25):
  - `decimals: Option<u8>` — token ondalık basamak sayısı
  - `symbol: Option<String>` — token sembolü
  - `#[cfg_attr(test, derive(Eq, PartialEq))]` — testlerde karşılaştırma için

- **`Error`** (satır 27-36): `String` wrapper, Display + Error impl

- **`TokenInfoFetching` trait** (satır 38-48):
  - `async fn get_token_info(&self, address: Address) -> Result<TokenInfo, Error>`
  - `async fn get_token_infos(&self, addresses: &[Address]) -> HashMap<Address, TokenInfo>` (default impl var)
  - `#[cfg_attr(any(test, feature = "test-util"), mockall::automock)]` — test mock desteği

- **`TokenInfoFetcher`** (satır 50-101):
  - `web3: Web3`
  - `BUY_ETH_ADDRESS` için hardcoded `TokenInfo { decimals: 18, symbol: "NATIVE_ASSET" }`
  - ERC20 `decimals()` + `symbol()` paralel call: `futures::join!`
  - `ignore_non_node_error`: non-node hataları (sözleşme yok vb.) `None` olarak döndürür

- **`CachedTokenInfoFetcher`** (satır 105-164) — KRİTİK PATTERN:
  - `inner: Arc<dyn TokenInfoFetching>` — wrapped fetcher
  - `cache: Arc<Mutex<HashMap<Address, SharedTokenInfo>>>` — `Shared<BoxFuture>` future cache

  **Future caching pattern:**
  ```
  cache.entry(address).or_insert({
      let inner = self.inner.clone();
      async move { inner.get_token_info(address).await }
          .boxed()
          .shared()  // ← aynı future birden fazla .await yapılabilir
  })
  ```
  Aynı adres için birden fazla eşzamanlı request gelirse hepsi aynı future'ı `await` eder — deduplicate edilmiş.

  **Hata durumunda cache temizleme:**
  ```
  if info.is_err():
      if cache.get(address).peek() == Some(Err(_)):
          cache.remove(address)  // sadece error'ı cache'te tutma
  ```
  Başarısız fetch'ler cache'ten silinir — sonraki sorguda yeniden denenir.

**Test (satır 166-231):**
- `cached_token_info_fetcher`: `times(1)` constraint — cache sayesinde tekrar fetch yok. Hatalı token (`times(2)`) — her sorguda yeniden denenir.

**Güvenlik Analizi:**

- **`BUY_ETH_ADDRESS` özel durumu:** Native ETH için ERC20 kontrat çağrısı yapılmaz — doğrudan hardcoded bilgi döner.
- **`ignore_non_node_error`:** Sözleşme yoksa (`decimals()` revert ederse) bu `None` döner, hata değil. Bazı token'lar symbol veya decimals döndürmeyebilir.
- **Shared future concurrency:** Mutex sadece cache erişimi için alınır, future await sırasında tutulmaz. Deadlock riski yok.

**DEX Uygulama Notu — Nova Shield için:**
- `Shared<BoxFuture>` pattern'i concurrent request deduplication için ideal — Nova Shield token fetcher'ında kullan
- `TOKEN_INFO_FETCH_CONCURRENCY = 32` — yapılandırılabilir yap, RPC sağlayıcısına bağlı
- `BUY_ETH_ADDRESS` özel case: Nova Shield'da native asset adresi (genellikle 0xEEEE...EE) için aynı pattern

---

### [s3/src/lib.rs] (200 satır)

**Amaç:** AWS S3'e gzip-sıkıştırılmış JSON yükleme.

**Sabitler:**
- `COMPRESSION_LEVEL: u32 = 3` — gzip seviyesi (eager + streaming path uyumluluğu için)

**Struct/Enum/Trait Listesi:**

- **`GzipWriter`** (satır 22-51) — streaming sıkıştırıcı:
  - `GzEncoder<Vec<u8>>` wrapper
  - `Write` trait impl: yazıldıkça sıkıştırır
  - `finish()`: sıkıştırılmış byte'ları döndürür
  - Kullanım: büyük payload'ları streamlerken paralel sıkıştırma

- **`Config`** (satır 53-58):
  - `bucket: String`
  - `filename_prefix: String` — yüklenen dosyaların öneki

- **`Uploader`** (satır 60-151):
  - `bucket / filename_prefix: String`
  - `client: aws_sdk_s3::Client`

  Fonksiyonlar:
  - `Uploader::new(config)`: AWS credentials'ları env'den yükler, test upload yaparak doğrular
  - `upload(id, content: impl Serialize)`: JSON serialize → `upload_json_bytes`
  - `upload_json_bytes(id, content: Bytes)`: gzip sıkıştır → `upload_gzipped`
  - `upload_gzipped(id, compressed: Bytes)`: S3'e PUT — `Content-Encoding: gzip`, `Content-Type: application/json`
  - Key format: `{filename_prefix}/{id}.json`
  - `assert_credentials_are_usable()`: başlangıçta test yüklemesi yapar, başarısız olursa panic

**Teknik Not — Two gzip code paths:**
- `GzipWriter` (streaming): `flate2::write::GzEncoder` — büyük payload'lar için
- `gzip()` (eager): `flate2::bufread::GzEncoder` — mevcut byte'ları sıkıştır
- Her ikisi de `COMPRESSION_LEVEL = 3` kullanır — aynı output garantisi

**Güvenlik Analizi:**

- **Credential doğrulama:** `assert_credentials_are_usable()` başlangıçta test upload yapar. Prod'da yanlış credential varsa uygulama hemen başlamayı reddeder — fail-fast.
- **5MB sınırı notu:** Kaynak kodunda belirtilmiş: gzip sonrası 5MB aşılırsa multipart upload düşünülmeli (S3 minimum part size 5MB). Şu an tek PUT.
- **Spawn blocking:** JSON serialize ve gzip sıkıştırma `spawn_blocking` ile tokio thread pool'unda yapılır — async runtime bloklanmaz.

**DEX Uygulama Notu — Nova Shield için:**
- Auction/settlement arşivleme için bu S3 uploader'ı kullan veya benzeri (MinIO on-prem)
- `GzipWriter` streaming pattern: büyük auction dump'ları için — tüm JSON belleğe almadan sıkıştır
- `filename_prefix` → key namespace: farklı ağlar için ayrı prefix (mainnet/, arbitrum/ vb.)

---

### [rate-limit/src/lib.rs] (375 satır)

**Amaç:** Exponential backoff ile rate limiting — dış API çağrıları için.

**Struct/Enum/Trait Listesi:**

- **`Metrics`** (satır 12-24):
  - `requests_dropped` — rate limit nedeniyle düşürülen istek sayısı
  - `rate_limited_requests` — rate limit yanıtı alan istek sayısı
  - `successful_requests` — başarılı istek sayısı
  - Hepsi `endpoint` label'ı ile

- **`StrategyExt` trait** (satır 31-45) — dahili uzantı:
  - `response_ok(&mut self, name)` — backoff sıfırla
  - `get_current_back_off()` — mevcut backoff hesapla
  - `response_rate_limited(previous_rate_limits, name) -> Option<Duration>` — backoff artır
  - `times_rate_limited(now, name) -> Option<u64>` — rate limited mi? (None = drop)

- **`Strategy`** (configs crate'ten, satır 47-97 impl):
  - `times_rate_limited: u64` — ardışık rate limit sayısı
  - `drop_requests_until: Instant` — bu zamana kadar istekleri at
  - `back_off_growth_factor: f64` — üstel büyüme çarpanı
  - `min_back_off / max_back_off: Duration` — backoff sınırları

- **`RateLimiter`** (satır 100-135):
  - `strategy: Mutex<Strategy>` — thread-safe strateji durumu
  - `name: String` — metrics label için

**Matematiksel Formül — Backoff Hesabı:**
```
back_off = min(min_back_off * back_off_growth_factor^times_rate_limited, max_back_off)
```
Örnek (test'ten): `min=20ms, factor=2.0, times=1 → 40ms`, `times=2 → 80ms`, max'a kadar büyür.

**Kritik Fonksiyonlar:**

- **`RateLimiter::execute(task, requires_back_off)`** (satır 150-183):
  ```
  1. strategy.times_rate_limited(now):
     None → drop, return Err(RateLimited) — task evaluate bile edilmez!
     Some(n) → n önceki rate limit sayısı
  2. result = task.await
  3. requires_back_off(result):
     true  → response_rate_limited(n) → backoff artır, drop_requests_until güncelle
     false → response_ok() → backoff sıfırla
  4. return Ok(result)
  ```
  Drop edilen task: `unreachable!()` ile değil, `Err(RateLimited)` ile bildirilir — caller handle eder.

- **`RateLimiter::execute_with_back_off(task, requires_back_off)`** (satır 185-195):
  Rate limited ise `tokio::time::sleep(back_off_duration).await` — bekle, sonra execute et. Drop etmez, bekler.

- **`response_rate_limited(previous_rate_limits, name)`** (satır 72-86):
  ```
  if self.times_rate_limited != previous_rate_limits:
      return None  // başka thread zaten güncelledi, skip
  ```
  Race condition koruması: iki thread aynı anda backoff artırmaya çalışırsa sadece biri başarılı olur.

- **`back_off::on_http_429(response)`** (satır 216-218):
  ```rust
  matches!(response, Ok(response) if response.status() == 429)
  ```
  HTTP 429 Too Many Requests → backoff tetikler.

**Test Kapsamı (satır 221-374):**
- `serde_roundtrip`: Strategy JSON serialize/deserialize
- `current_back_off_does_not_panic`: `f64::MAX` growth factor overflow overflow etmez (min ile cap'lendi)
- `drops_requests_correctly`: tam senaryo testi — ilk istek OK, rate limit yanıtı, drop, bekleme, tekrar
- `test_execute_with_no_back_off` / `test_execute_with_back_off`: bekleme logic testi

**Güvenlik Analizi:**

- **Race condition koruması:** `previous_rate_limits != times_rate_limited` kontrolü. İki thread aynı anda "rate limited" yanıtı alırsa ikincisi backoff'u tekrar artırmaz. Bu önemli — yoksa backoff katlanırdı.
- **Overflow koruması:** `Duration::try_from_secs_f64(f64::MAX * ...)` → `unwrap_or(max_back_off)`. `f64::MAX` için panic yok.
- **Task evaluate edilmemesi:** Drop modunda `task` Future'ı `await` bile edilmez. Lazy evaluation — gereksiz hesaplama yok.
- **Mutex poisoning:** `strategy.lock().unwrap()` — panic etkisi. Production'da `lock().ok()` daha güvenli olabilir.

**DEX Uygulama Notu — Nova Shield için:**
- Bu rate limiter'ı Nova Shield'ın dış API çağrıları için kullan: price oracle API'leri, bridge quote API'leri, external solver API'leri
- `execute` vs `execute_with_back_off`: kritik path'lerde `execute` (drop-fast), batch/async path'lerde `execute_with_back_off` (bekle)
- `back_off::on_http_429` pattern: her dış API için uygun `requires_back_off` fonksiyonu yaz
- Rate limiter'ı Arc içinde paylaş: aynı endpoint'e farklı context'lerden gelen istekler aynı limiter'ı kullanmalı

---

# BÖLÜM 6: KAPSAMLI DEX GÜVENLİK ANALİZİ

## order-validation — Tam Güvenlik Değerlendirmesi

### Validation Pipeline Eksiklikleri (Bu Crate'de Değil)

`order-validation` crate'inin kapsamı yalnızca **kullanıcı adresi ban kontrolü**dür. Aşağıdaki validasyonlar burada bulunmaz — `orderbook/src/order_validation.rs` gibi üst katmanlarda:

1. **İmza doğrulama**: EIP-712, eth_sign, EIP-1271, pre-sign — `shared/src/signature_validator.rs`
2. **Expiry kontrolü**: `valid_to > now` — orderbook seviyesinde
3. **Bakiye kontrolü**: `owner.balanceOf(sell_token) >= sell_amount` — orderbook seviyesinde
4. **Fee yeterliliği**: Gas maliyeti karşılayacak kadar fee var mı — quote servisi
5. **Duplicate order tespiti**: UID tabanlı DB kontrolü — orderbook DB
6. **Gas limit kontrolü**: Order'ın settlement gas bütçesine sığıp sığmadığı — driver
7. **Token whitelist/blacklist**: `Token.trusted` flag — autopilot filtresi
8. **App-data doğrulama**: 8KB limit, geçerli JSON schema — `app-data` crate'i

### Bu Crate'deki Gerçek Validation Katmanı

```
[Order gelir]
       ↓
[Hardcoded kara liste kontrolü]  ← O(1) HashSet
       ↓ (temiz)
[Chainalysis Oracle sorgusu]     ← on-chain view call
       ↓ (temiz)
[Hermod API sorgusu]             ← HMAC-signed HTTP GET
       ↓ (temiz)
[Order kabul]
```

### Bypass Risk Analizi

| Risk | Durum | Açıklama |
|------|-------|----------|
| Sıfır adres bypass | Kasıtlı | Burn adresi için ban kontrolü yok — iç kullanım |
| Cache stale | Tolerable | 1 saat cache, background refresh |
| Backend hata fail-open | Bilinçli | Remote hata → "belirsiz" → cache'e yazma |
| HMAC key compromise | Kritik | Hermod key sızdırılırsa adres gizliliği biter |
| Chainalysis oracle stale | Tolerable | Oracle güncelleme gecikmesi Chainalysis'in sorumluluğu |
| Race condition (ban sonrası işlem) | Mümkün | Ban anında işlemdeyse tamamlanabilir |

---

# BÖLÜM 7: COW AMM — NOVA SHIELD DEX UYGULAMASI

## CoW AMM vs Klasik AMM — Karşılaştırmalı Analiz

### Klasik AMM (Uniswap v2)
```
x * y = k  (sabit ürün)

Her swap:
  delta_x girer, delta_y çıkar
  (x + delta_x) * (y - delta_y) = k
  delta_y = y * delta_x / (x + delta_x)

Fiyat: P = y/x (anlık, swap anında değişir)

Sorunlar:
  - Her swap ayrı tx → MEV frontrun/sandwich
  - Slippage: büyük order'lar büyük price impact
  - LVR (Loss Versus Rebalancing): LP'ler arbitrajcılara karşı kaybeder
```

### CoW AMM
```
Pool: (R_0, R_1) rezervleri
Referans fiyatlar: (p_0, p_1) — dış oracle'dan

AMM order üretimi:
  Eğer R_0/R_1 < p_0/p_1:  (pool token_0 ucuz, token_1 pahalı)
    Sell token_0, buy token_1
    Sell amount: delta_0 öyle ki:
      (R_0 + delta_0) * (R_1 - delta_1) = R_0 * R_1  (k sabit)
      delta_1/delta_0 = p_0/p_1                        (uniform price'ta rebalance)

    Çözüm:
      delta_0 = R_0 * (sqrt(p_1/p_0) - 1)
      delta_1 = R_1 * (1 - sqrt(p_0/p_1))

Batch settlement:
  Tüm order'lar (kullanıcı + AMM) aynı batch'e girer
  Solver uniform clearing price P* bulur
  P* = p_0/p_1 olursa AMM rebalance tam gerçekleşir

MEV koruması: P* herkese eşit uygulanır
LVR azalması: AMM oracle fiyatıyla rebalance → arbitraj fırsatı azalır
```

### CoW AMM Avantajları (Nova Shield için)

1. **MEV Koruması:** Batch içindeki herkes aynı P* alır → frontrun/sandwich imkansız
2. **LVR Azalması:** Oracle fiyatıyla rebalance → LP'ler daha az arbitraj kaybı
3. **Gas Verimliliği:** AMM, kullanıcı order'larıyla netting yaparak gas tasarrufu sağlar
4. **Programlanabilir Strateji:** Pre/post interaction ile AMM farklı rebalance stratejileri uygulayabilir
5. **EIP-1271:** AMM kendi order'larını imzalar → off-chain kullanıcı gerekmez

### Nova Shield DEX Mimarisi Önerisi (CoW AMM bazlı)

```
Nova Shield DEX Katmanları:

  [Kullanıcı Order'ları] → Order Validation Pipeline
                                    ↓
  [CoW AMM Pool'ları]  → Template Order Üretimi (oracle referans fiyatı ile)
                                    ↓
  [Batch Aggregation]  → Tüm order'lar tek batch
                                    ↓
  [Nova Shield Solver] → Uniform Clearing Price P* hesapla
    - CFMM constraint: x*y = k her pool için
    - User order constraint: sell_amount <= max, buy_amount >= min
    - Surplus maximize et: toplam kullanıcı artısı
                                    ↓
  [Settlement]         → P* ile herkesi settle et
    - Pre-interactions: AMM commit
    - Token transfers
    - Post-interactions
```

### CoW AMM Modelini Alabilir miyiz?

**Evet, tamamen alınabilir.** Bileşenler:

| Bileşen | Durum | Nova Shield Uygulaması |
|---------|-------|------------------------|
| GPv2Order format | Uyumlu | Nova Shield kendi order formatı olabilir |
| EIP-1271 signing | Uyumlu | Smart contract pool'lar kendi order'larını imzalar |
| Commit mekanizması | Kritik | Anti-replay için zorunlu |
| Helper contract | Yeniden tasarım gerekli | Nova Shield kendi Solidity helper'ı yazar |
| Template order üretimi | Rust kodu kopyalanabilir | `amm.rs` pattern |
| Event indexing | Doğrudan alınabilir | `cache.rs + factory.rs + registry.rs` |

**Nasıl Daha İyi Yapabiliriz:**

1. **Multi-asset AMM:** CoW AMM şu an 2 token. Nova Shield 3+ token pool destekleyebilir (Curve tarzı)
2. **Dinamik rebalance eşiği:** Oracle fiyat güncelleme frekansına göre commit threshold ayarlanabilir
3. **LP token programlanabilirliği:** AMM stratejisini NFT olarak tokenize et — farklı LP pozisyonları
4. **Cross-chain AMM:** Nova Shield cross-chain ise AMM order'ları birden fazla zincirde execute edilebilir

---

# BÖLÜM 8: SOLVERS-DTO GÜVENLİK DEĞERLENDİRMESİ

## Solver←→Driver Güven Modeli

Solver'lar **güvenilmeyen** (untrusted) varlıklardır:

```
Solver döndürür → Driver doğrular:
  1. Clearing price geçerliliği (tüm order'ları karlı mı settliyor?)
  2. Interaction simülasyonu (calldata gerçekten çalışıyor mu?)
  3. CustomInteraction.inputs/outputs beyanları
  4. Non-bufferable token kullanımı
  5. Gas limitler
  6. Solver account bakiyesi
```

`Notification.Kind` enum'u bu doğrulamaların başarısız olabileceği her noktayı kapsar.

## JIT Order Güvenliği

```
JitTrade.order: JitOrder
  - Solver tarafından oluşturulur
  - Herhangi bir token çifti olabilir
  - İmzası solver'ın imzasıdır

Risk: Solver kötü niyetli JIT order'larla kendine surplus transfer edebilir

CoW Protocol koruma:
  - Surplus scoring: solver'ın önerdiği solution değerlendirmeye tabi
  - Circuit breaker: on-chain settlement = off-chain auction? kontrol
  - Solver jailing: kötü davranış → solver devre dışı
```

**Nova Shield için:** JIT order whitelist — sadece tanımlı solver'lar JIT oluşturabilir.

---

# SONUÇ VE NOVA SHIELD EYLEM PLANI

## Öncelikli Alınacak Pattern'lar

### Acil (DEX MVP için kritik)
1. **Ban sistemi:** `Users` + `Cached` + HMAC-signed API pattern — doğrudan uygulama
2. **CoW AMM matematiksel modeli:** Batch settlement + uniform clearing price — temel DEX prensibi
3. **EIP-1271 pool signing:** Smart contract pool'ların kendi order'larını imzalaması
4. **Commit mekanizması:** Anti-replay, batch-bound order binding
5. **Notification enum:** Solver←→driver feedback loop — solver scoring altyapısı

### Orta Vadeli
6. **Event indexing altyapısı:** `cache.rs + factory.rs + registry.rs` pattern — pool lifecycle yönetimi
7. **Rate limiter:** External oracle ve bridge API çağrıları için exponential backoff
8. **Token info caching:** `Shared<BoxFuture>` — concurrent request deduplication

### Geliştirme Hedefleri
9. **JIT order kontrolü:** Whitelist tabanlı solver güven modeli
10. **Multi-source sanction list:** Chainalysis + TRM Labs + on-chain oracle
11. **CoW AMM genişletme:** 2 token → N token (Curve tarzı multi-asset)

## Tespit Edilen Güvenlik Boşlukları (CoW Protocol'e özgü, Nova Shield'da çözülmeli)

| Boşluk | CoW Protocol Çözümü | Nova Shield İyileştirme |
|--------|---------------------|------------------------|
| Fail-open ban cache | Tolerated | Stricter: hata → geçici ban (fail-closed) |
| HMAC key statik | Runtime config | Key rotation mekanizması |
| JIT order sınırsız | Scoring-based | Whitelist + hard limit |
| Revert tx zincirde kalır | Circuit breaker | Proaktif simulation + pre-check |
| 5MB S3 limit | Manuel not | Otomatik multipart upload |
| `Mutex::unwrap()` | Panic risk | Graceful lock failure handling |

---

*Agent 14 analizi tamamlandı. Toplam kapsam: 22 kaynak dosyası, ~1,850 satır Rust kodu analiz edildi.*
