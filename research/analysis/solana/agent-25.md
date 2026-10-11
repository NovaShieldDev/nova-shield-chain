# Solana Analiz — Agent 25
## Kapsam: sdk/src/ Kalan Utility + bloom/ + bucket_map/

**Toplam Analiz Edilen Satır:** ~6.290 satır (14 sdk dosyası + 2 bloom dosyası + 9 bucket_map dosyası)

---

## BÖLÜM 1: sdk/src/ — Kalan Utility Dosyaları

---

### [client.rs] (241 satır)

**Amaç:** Solana istemcileriyle blokzinciriyle senkron ve asenkron iletişim için trait tanımları; doğrudan implementasyon içermez, sadece arayüz sözleşmeleri.

**Trait Listesi:**

- `Client` (satır 30): `SyncClient + AsyncClient` birleşimi. Tek ek metot: `tpu_addr() -> String`. Hem sync hem async yetenekleri zorunlu kılar.

- `SyncClient` (satır 34-174): Senkron RPC istemcisi trait'i. Tüm metodlar zorunlu (default impl yok):
  - `send_and_confirm_message<T: Signers>()` — çok imzacı transaction gönder + onayla
  - `send_and_confirm_instruction()` — tek imzacı instruction gönder + onayla
  - `transfer_and_confirm()` — lamport transferi + onaylama
  - `get_account_data()`, `get_account()`, `get_account_with_commitment()` — account okuma
  - `get_balance()`, `get_balance_with_commitment()` — bakiye sorgu
  - `get_minimum_balance_for_rent_exemption()` — kira muafiyeti minimum bakiyesi
  - `get_recent_blockhash()` — DEPRECATED (1.9.0), `get_latest_blockhash()` kullanılmalı
  - `get_recent_blockhash_with_commitment()` — DEPRECATED (1.9.0)
  - `get_fee_calculator_for_blockhash()` — DEPRECATED (1.9.0)
  - `get_fee_rate_governor()` — DEPRECATED (1.9.0)
  - `get_signature_status()`, `get_signature_status_with_commitment()` — imza durum sorgusu
  - `get_slot()`, `get_slot_with_commitment()` — slot sorgusu
  - `get_transaction_count()`, `get_transaction_count_with_commitment()` — transaction sayısı
  - `get_epoch_info()` — epoch bilgisi
  - `poll_for_signature_confirmation()` — N blok onayına kadar polling
  - `poll_for_signature()` — tek onaya kadar polling
  - `get_new_blockhash()` — DEPRECATED (1.9.0)
  - `get_latest_blockhash()` — güncel blockhash
  - `get_latest_blockhash_with_commitment()` — commitment seviyeli blockhash + son geçerli blok yüksekliği
  - `is_blockhash_valid()` — blockhash geçerliliği kontrolü
  - `get_fee_for_message()` — Message için ücret hesaplama

- `AsyncClient` (satır 176-241): Asenkron (fire-and-forget) istemci trait'i. Zorunlu tek metot: `async_send_versioned_transaction()`. Diğerleri default impl içerir:
  - `async_send_transaction()` (satır 178) — Transaction'ı VersionedTransaction'a çevirip gönderir
  - `async_send_batch()` (satır 183) — Batch Transaction gönderimi
  - `async_send_versioned_transaction_batch()` (satır 195) — default: tek tek gönderir, override edilebilir
  - `async_send_message()` (satır 207) — Message'dan Transaction yaratır + gönderir
  - `async_send_instruction()` (satır 219) — tek Instruction → Message → Transaction → gönderir
  - `async_transfer()` (satır 230) — lamport transfer instruction → asenkron gönderim

**Önemli Notlar:**
- `#![cfg(feature = "full")]` ile gated — light client ortamlarında derlenmez
- Deprecated metodlar hâlâ trait'te tutuluyor (backward compat), ancak hiç default impl verilmemiş
- `VersionedTransaction` desteği `async_send_transaction` içinde otomatik dönüşümle sağlanmış

**Unsafe Bloklar:** Yok.

**Test Coverage:** Test yok — sadece trait tanımları.

**Nova Shield İçin Öğrenilen:**
- Sync/Async client trait ayrımı temiz bir tasarım: nova-shield-chain kendi RPC katmanı için aynı pattern'i kullanabilir
- `get_latest_blockhash_with_commitment()` → `(Hash, u64)` dönüş tipi önemli — son geçerli blok yüksekliği transaction expiry için kullanılır; Nova Shield'da benzer mekanizma: son geçerli slot/height ile transaction ömrü
- Deprecated API'lar trait'te sıfır impl olmadan tutulmuş — büyük breaking change'den kaçınmak için iyi pattern
- `async_send_versioned_transaction_batch()` default impl'ı tek tek gönderir; Nova Shield için gerçek batching (UDP burst veya QUIC multiplexing) ile override edilebilir

---

### [account_utils.rs] (88 satır)

**Amaç:** Account verilerini serde ile okuyup yazmak için kolaylık trait'leri; bincode hatalarını InstructionError'a çevirir.

**Trait Listesi:**

- `StateMut<T>` (satır 13): Mutable account için state okuma/yazma.
  - `fn state(&self) -> Result<T, InstructionError>` — deserialize
  - `fn set_state(&mut self, state: &T) -> Result<(), InstructionError>` — serialize

- `State<T>` (satır 17): Interior mutability ile state okuma/yazma (RefCell kullanımı için).
  - Aynı metodlar ama `set_state` `&self` alır (RefCell içi mutasyon için)

**impl Blokları:**

- `impl<T: Serialize + DeserializeOwned> StateMut<T> for Account` (satır 22-36):
  - `state()`: `self.deserialize_data()` çağırır, hata → `InvalidAccountData`
  - `set_state()`: `self.serialize_data(state)` çağırır, `SizeLimit` → `AccountDataTooSmall`, diğer → `GenericError`

- `impl<T> StateMut<T> for AccountSharedData` (satır 38-52): Account ile aynı mantık.

- `impl<T> StateMut<T> for Ref<'_, AccountSharedData>` (satır 54-65): **Özel durum**:
  - `state()` çalışır (okuma)
  - `set_state()` → `panic!("illegal")` — Ref üzerinden yazma imkânsız, compile-time değil runtime koruması

**Test Coverage** (satır 67-88):
- `test_account_state`: Default AccountSharedData'ya set_state → hata doğrulanır, sonra doğru boyutlu account ile set/get roundtrip test edilir.

**Nova Shield İçin Öğrenilen:**
- `Ref<'_, AccountSharedData>` üzerinde `set_state` → `panic!` tasarımı kötü; Nova Shield'da bu için compile-time trait ayrımı tercih edilmeli (`ReadOnly` vs `Writable` marker trait'ler ile)
- Bincode hata eşleştirmesi: `SizeLimit` → spesifik hata, diğer → generic — Nova Shield'da custom error enum daha iyi olur
- `StateMut` pattern'i program state yönetimi için evrensel; Nova Shield'ın program/contract state'i için direkt uygulanabilir

---

### [deserialize_utils.rs] (141 satır)

**Amaç:** Versioned struct deserializasyonu — yeni field'lar eklense bile eski binary data okunabilsin diye EOF hatalarını varsayılan değere çevirir.

**Kritik Fonksiyonlar:**

- `default_on_eof<'de, T, D>()` (satır 9-16): Deserializer'dan T okumaya çalışır; EOF hatası gelirse `T::default()` döner. Yeni field'ların geriye dönük uyumlu eklenmesini sağlar.

- `ignore_eof_error<'de, T, D>()` (satır 18-28): Raw result'u alır; şu iki string literal'i hata olarak tanır:
  - `"io error: unexpected end of file"`
  - `"io error: failed to fill whole buffer"`
  Bu string matching yaklaşımı dikkat çekici — bincode error türleri daha kararlı olsaydı type-safe matching kullanılırdı.

**Test Coverage** (satır 30-141):
- `test_default_on_eof`: `bar: u16`, `baz: Option<u16>`, `quz: String` içeren struct ile eksik field senaryoları binary düzeyde test edilir. 6 farklı byte dizisi ile sınır koşulları doğrulanır.
- `test_default_on_eof_additional_untagged_fields`: `#[should_panic]` ile etiketli — yalnızca bazı field'lara `deserialize_with` uygulanırsa, sonrakiler eksik olduğunda panic oluşur. Bu davranış belgelenmiş.

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Solana'nın snapshot/state migration stratejisi: field'lar sona eklenir + `default_on_eof` ile eski binary okunur — Nova Shield için state migration aynı şekilde; ancak string literal ile error matching yerine custom bincode error type kullanılmalı
- `#[serde(deserialize_with = "default_on_eof")]` her yeni opsiyonel field'a uygulanmalı — bunu zorunlu kılmak için custom derive macro önerilebilir
- `should_panic` test pattern'i — bilerek kırılan pre-condition'lar için iyi belgeleme

---

### [native_loader.rs] (36 satır)

**Amaç:** Native program (shared object) account'ları oluşturmak için yardımcı fonksiyonlar; account verisine SO adı yazılır.

**Sabitler:**
- `declare_id!("NativeLoader1111111111111111111111111111111")` (satır 10): Native loader program ID.

**Kritik Fonksiyonlar:**

- `create_loadable_account()` (satır 17): DEPRECATED (1.5.17). `create_loadable_account_with_fields` çağırır.

- `create_loadable_account_with_fields()` (satır 21-32): Çekirdek fonksiyon.
  - `lamports` ve `rent_epoch` alır (`InheritableAccountFields`)
  - `owner: id()` — native loader kendi sahibi
  - `data: name.as_bytes().to_vec()` — SO adı account verisine yazılır
  - `executable: true` — çalıştırılabilir flag
  - AccountSharedData::from(Account{...}) döner

- `create_loadable_account_for_test()` (satır 34-36): Test için sabit `DUMMY_INHERITABLE_ACCOUNT_FIELDS` ile çağırır.

**Unsafe Bloklar:** Yok. **Test:** Yok (test yardımcı fonksiyonu).

**Nova Shield İçin Öğrenilen:**
- Native program yükleme mekanizması: account verisine SO/binary adı yazılır, executor bu adı kullanarak yükler — Nova Shield'da Wasm veya EVM bytecode için benzer indirection: account data = modül adı/hash, runtime bunu çözümler
- `executable: true` flag'i hesap modelinde önemli; Nova Shield'da da program account'ları için benzer flag zorunlu

---

### [net.rs] (3 satır)

**Amaç:** Ağ katmanı parametreleri.

**Sabitler:**
- `DEFAULT_TPU_COALESCE: Duration = Duration::from_millis(5)`: TPU paket biriktirme süresi. Transaction'lar gönderilmeden önce 5ms beklenir, daha büyük batch'ler oluşturmak için.

**Nova Shield İçin Öğrenilen:**
- 5ms coalesce süresi ciddi throughput optimizasyonu — Nova Shield'ın mempool/transaction pipeline'ında benzer batching window kullanılabilir; UDP/QUIC üzerinde paket başlığı overhead'ini düşürür

---

### [nonce_account.rs] (133 satır)

**Amaç:** Durable nonce account mantığı — "advance nonce" mekanizması ile transaction replay koruması.

**Kritik Fonksiyonlar:**

- `create_account()` (satır 16-26): Yeni nonce account oluşturur.
  - `RefCell<AccountSharedData>` döner (iç mutasyon için)
  - `State::Uninitialized` ile başlar, owner = `system_program::id()`
  - `AccountSharedData::new_data_with_space(lamports, &Versions::new(State::Uninitialized), State::size(), &system_program::id())`

- `verify_nonce_account()` (satır 30-42): Bir account'ın geçerli nonce account olup olmadığını ve `recent_blockhash`'in nonce değeriyle eşleşip eşleşmediğini doğrular.
  - **Güvenlik kontrolü #1**: `account.owner() == &system_program::id()` — yanlış owner varsa None
  - **Güvenlik kontrolü #2**: `StateMut::<Versions>::state(account)?.verify_recent_blockhash(recent_blockhash)` — state doğrulama
  - `Versions::Legacy` hiçbir zaman `verify_recent_blockhash` geçemez (test satır 107-113 bunu doğruluyor) — legacy nonce'lar derecate edilmiş
  - Başarıda `Some(Data)` döner — Data içinde `authority`, `durable_nonce`, `fee_calculator` var

- `lamports_per_signature_of()` (satır 44-49): Account'tan ücret bilgisi okur.
  - `State::Initialized(data)` → `Some(data.fee_calculator.lamports_per_signature)`
  - `State::Uninitialized` → `None`

**Nonce Versiyonlama Sistemi:**
- `Versions::Legacy` — eski format, hiçbir blockhash için verify geçemez (güvenlik downgrade koruması)
- `Versions::Current` — `DurableNonce::from_blockhash(durable_nonce.as_hash())` ile çift hash zinciri

**Test Coverage** (satır 51-133):
- `test_verify_bad_account_owner_fails`: Sistem program dışı owner → None
- `test_verify_nonce_account`: Legacy uninitialized, Current uninitialized, Legacy initialized (blockhash ve durable_nonce her ikisi de reddedilir), Current initialized (sadece `data.blockhash()` ve `durable_nonce.as_hash()` kabul edilir)

**Nova Shield İçin Öğrenilen:**
- Durable nonce güvenlik mimarisi: owner check + state version check + blockhash match, üç katmanlı doğrulama
- `Versions::Legacy` hiçbir zaman verify edemez — downgrade attack koruması; Nova Shield'da gelecekte format değişikliği yapılırsa eski formatlar aynı şekilde geçersizleştirilmeli
- `DurableNonce::from_blockhash()` çağrısı hash'i bir kez daha hash'ler — bu "double hashing" nonce ile blockhash arasında deterministic ama kopuk bir ilişki kurar; Nova Shield'da sequence number tabanlı benzer mekanizma uygulanabilir

---

### [nonce_info.rs] (272 satır)

**Amaç:** Transaction doğrulama sürecinde nonce bilgisini taşıyan yapılar; iki aşamalı işleme modeli (partial check → full rollback).

**Trait:**

- `NonceInfo` (satır 12-17): Nonce state erişimi için ortak arayüz.
  - `fn address(&self) -> &Pubkey`
  - `fn account(&self) -> &AccountSharedData`
  - `fn lamports_per_signature(&self) -> Option<u64>`
  - `fn fee_payer_account(&self) -> Option<&AccountSharedData>`

**Struct'lar:**

- `NoncePartial` (satır 21-45): Transaction kontrolü sırasında tutulan sınırlı nonce bilgisi.
  - Fields: `address: Pubkey`, `account: AccountSharedData`
  - `fee_payer_account()` → her zaman `None` (partial aşamada fee payer henüz bilinmiyor)
  - `lamports_per_signature()` → `nonce_account::lamports_per_signature_of()` delegesi

- `NonceFull` (satır 49-116): Ücret düşürülmüş nonce bilgisi — rollback için gereken tam bilgi.
  - Fields: `address`, `account`, `fee_payer_account: Option<AccountSharedData>`
  - **`from_partial()` (satır 67-100)**: Kritik dönüşüm fonksiyonu:
    1. Message'daki ilk non-loader key'i fee payer olarak bulur
    2. `rent_debits.get_account_rent_debit(fee_payer_address)` → rent borcu hesaplar
    3. `fee_payer_account.lamports().saturating_add(rent_debit)` — rent geri eklenir (rollback için pre-rent state)
    4. Nonce address == fee payer address ise: `fee_payer_account = None` (nonce account kendisi)
    5. Farklı ise: `fee_payer_account = Some(pre_rent_fee_payer_account)`
    6. Fee payer hiç bulunamazsa: `TransactionError::AccountNotFound`

**Test Coverage** (satır 118-272): Kapsamlı entegrasyon testi:
- `test_nonce_info`: 5 hesap (nonce, from, to, sysvar), rent debit ile birlikte:
  - NoncePartial create + tüm NonceInfo metodları
  - NonceFull::from_partial — fee payer from account
  - NonceFull::from_partial — fee payer = nonce account
  - NonceFull::from_partial — boş accounts → AccountNotFound
  - Rent geri eklemesi doğrulanıyor (`from_account` lamports pre-rent değerinde olmalı)

**Nova Shield İçin Öğrenilen:**
- İki aşamalı transaction processing: `NoncePartial` (hızlı ön kontrol) → `NonceFull` (tam rollback bilgisi) pattern'i güzel ayrım — Nova Shield'da mempool validation vs execution phase ayrımı için benzer pattern
- Rent debit'in rollback'te geri eklenmesi kritik doğruluk garantisi — Nova Shield'da state fees/storage fees için benzer pre-fee snapshot mekanizması
- `fee_payer_account = None` when nonce == fee payer: single account için duplicate depolama önlenir

---

### [poh_config.rs] (39 satır)

**Amaç:** Proof of History yapılandırması — tick hızı ve hash sayısı ayarları.

**Struct:**

- `PohConfig` (satır 9-20):
  - `target_tick_duration: Duration` — hedef tick süresi
  - `target_tick_count: Option<u64>` — sadece test için toplam tick sayısı
  - `hashes_per_tick: Option<u64>` — `None` → "Low power mode" (uyku), `Some(n)` → n hash sonra tick

**impl Blokları:**

- `PohConfig::new_sleep()` (satır 23-30): `hashes_per_tick = None` (sleep mode), test için kullanılır.
- `Default::default()` (satır 32-39): `new_sleep(Duration::from_micros(1_000_000 / DEFAULT_TICKS_PER_SECOND))` — bu değer compile-time `unchecked_div_by_const!` macro ile hesaplanır.

**Nova Shield İçin Öğrenilen:**
- PoH konfigürasyonunda `hashes_per_tick: None` = sleep mode; yüksek frekanslı PoH için `Some(n)` gerekli — Nova Shield'ın zaman kanıtı mekanizması için benzer dual-mode yapı
- `DEFAULT_TICKS_PER_SECOND` ile `Duration` hesaplama compile-time constant — Nova Shield'da zaman sabitleri aynı şekilde const olmalı

---

### [program_utils.rs] (38 satır)

**Amaç:** Program instruction deserializasyonu için güvenli limit uygulaması.

**Kritik Fonksiyonlar:**

- `limited_deserialize<T>()` (satır 9-17): `PACKET_DATA_SIZE` (1232 byte) ile sınırlı bincode deserializasyonu.
  - `solana_program::program_utils::limited_deserialize(data, PACKET_DATA_SIZE as u64)` delegesi
  - OOM saldırılarını önler: saldırgan çok büyük `Vec` veya çok derin nesne gönderemez

**Test Coverage** (satır 19-38):
- `test_limited_deserialize`: `Foo::Bar(Vec<u8>)` ile PACKET_DATA_SIZE sınırında — `len-12` byte vec başarılı, `len-11` byte vec başarısız (12 byte = enum variant 4B + vec uzunluk 8B serializasyon overhead)

**Nova Shield İçin Öğrenilen:**
- `PACKET_DATA_SIZE = 1232` byte sınırı zorunlu güvenlik önlemi; Nova Shield transaction payload'ları için sabit boyut sınırı aynı şekilde uygulanmalı
- Limit = PACKET_DATA_SIZE tasarımı bilinçli: ağ katmanından gelen veri zaten bu boyutla sınırlı, fazlası asla gelmez; tutarlılık için program deserializasyonu da aynı sınırı kullanır

---

### [recent_blockhashes_account.rs] (172 satır)

**Amaç:** Recent blockhashes sysvar account'ını yönetmek için yardımcı fonksiyonlar — tümü deprecated.

**Sabitler (dışarıdan):**
- `MAX_ENTRIES`: RecentBlockhashes sysvar'ın maksimum giriş sayısı (150).

**Kritik Fonksiyonlar (tümü deprecated):**

- `update_account()` (satır 23-38, deprecated 1.9.0): BinaryHeap'e sıralar (`IntoIterSorted`), MAX_ENTRIES kadar alır, account'a yazar.
- `create_account_with_data()` (satır 45-51, deprecated 1.5.17)
- `create_account_with_data_and_fields()` (satır 58-71, deprecated 1.9.0): fields (lamports, rent_epoch) ile birlikte
- `create_account_with_data_for_test()` (satır 78-83, deprecated 1.9.0): Test için sabit fields ile

**Test Coverage** (satır 85-172):
- `test_create_account_empty`: Boş iterator → RecentBlockhashes::default()
- `test_create_account_full`: MAX_ENTRIES kadar giriş → tam dolu
- `test_create_account_truncate`: MAX_ENTRIES+1 → MAX_ENTRIES kırpılır
- `test_create_account_unsorted`: Rastgele sıralı girişler → BinaryHeap sort → doğru sıra (yüksek slot önce)

**Nova Shield İçin Öğrenilen:**
- `BinaryHeap + IntoIterSorted` → max-heap + sorted iteration pattern; Nova Shield'da benzer "son N slot" takibi için efficient heap kullanımı
- `MAX_ENTRIES = 150` → blockhash validasyon penceresi; Nova Shield'da transaction expiry window boyutu benzer şekilde sınırlandırılmalı
- Tüm bu API'ların deprecated olması: Solana 1.9.0+'dan itibaren `get_latest_blockhash` + `last_valid_block_height` kullanılıyor — daha basit ve daha güvenli

---

### [epoch_info.rs] (29 satır)

**Amaç:** `getEpochInfo` RPC methodunun döndürdüğü epoch bilgisi struct'ı.

**Struct:**

- `EpochInfo` (satır 11-29): `#[serde(rename_all = "camelCase")]`
  - `epoch: Epoch` — mevcut epoch numarası
  - `slot_index: u64` — epoch içindeki slot indeksi (0..slots_in_epoch)
  - `slots_in_epoch: u64` — bu epoch'taki toplam slot sayısı
  - `absolute_slot: Slot` — mutlak slot numarası (genesis'ten beri)
  - `block_height: u64` — güncel blok yüksekliği
  - `transaction_count: Option<u64>` — genesis'ten beri hatasız işlenen transaction sayısı (opsiyonel)

**Nova Shield İçin Öğrenilen:**
- `slot_index = absolute_slot - epoch_start_slot` ilişkisi; Nova Shield'da epoch → slot → block height üç katmanlı zaman koordinatı benzer şekilde ifade edilmeli
- `transaction_count: Option<u64>` — validator bu istatistiği tutmayabilir; Nova Shield'da performans metriği opsiyonel field olarak tutulmalı

---

### [epoch_rewards_hasher.rs] (186 satır)

**Amaç:** Epoch reward dağıtımında validator adreslerini partition'lara eşlemek için SipHasher13 tabanlı hasher.

**Struct:**

- `EpochRewardsHasher` (satır 8-11):
  - `hasher: SipHasher13` — SipHash-1-3 (daha hızlı, biraz daha düşük güvenlik garantisi)
  - `partitions: usize` — kaç partition var

**Kritik Fonksiyonlar:**

- `EpochRewardsHasher::new()` (satır 15-19): `seed` (bir `Hash`) ile SipHasher13'ü başlatır ve hasher durumunu yazar. Bu sayede her epoch için farklı ama deterministik hash davranışı sağlanır.

- `hash_address_to_partition()` (satır 22-31): `self` consume eder (clone ile çağrılmalı — test bunu doğrular).
  - `hasher.write(address.as_ref())` — pubkey'i ekler
  - `hasher.finish()` → u64
  - `hash_to_partition(hash64, partitions)` → partition index

- `hash_to_partition()` (satır 39-43): **Tarafsız modulo algoritması**:
  ```
  partition_index = (partitions as u128 * hash as u128) / (u64::MAX as u128 + 1)
  ```
  Bu formül klasik `hash % partitions` yerine kullanılır çünkü modulo basit büyük N için düzgün dağılım sağlamaz.
  Matematik: `(rand_int * DESIRED_RANGE_MAX) / (RAND_MAX + 1)` — bias-free integer range mapping.

**Test Coverage** (satır 45-186):
- `test_get_equal_partition_range`: 2 partition için [0, MAX/2] ve [MAX/2+1, MAX] aralıkları doğrulanır
- `test_hash_to_partitions`: 16 partition için boundary değerler kontrol edilir
- `test_hash_to_partitions_equal_ranges`: 2,4,8,16,4096 ve 3,19,1019,4095 partition sayıları — eşit bölünemeyen durumda partition boyutları en fazla 1 farklı olmalı
- `test_hasher_copy`: Clone'un doğru çalıştığı, seed'in dahil edildiği doğrulanır

**Nova Shield İçin Öğrenilen:**
- `hash_to_partition` formülü bias-free range mapping için standart — Nova Shield'ın sharding/partition mantığında aynı formül kullanılmalı
- SipHasher13 keyed on seed: her epoch/round için farklı ama deterministik mapping; DoS saldırısına karşı korunma (saldırgan bir epoch'un seed'ini bilmeden tüm pubkey'leri aynı partition'a yönlendiremez)
- Hasher'ı consume eden API (satır 22): caller `clone()` zorunda — immutable shared state, thread-safe kullanım garantisi

---

### [exit.rs] (32 satır)

**Amaç:** Validator graceful shutdown mekanizması — tüm shutdown callback'lerini kaydet ve sırayla çalıştır.

**Struct:**

- `Exit` (satır 6-9): `#[derive(Default)]`
  - `exited: bool` — shutdown başladı mı?
  - `exits: Vec<Box<dyn FnOnce() + Send + Sync>>` — kayıtlı shutdown callback'leri

**Kritik Fonksiyonlar:**

- `register_exit()` (satır 12-18):
  - `if self.exited` → callback'i hemen çalıştır (geç kayıt)
  - Aksi halde listeye ekle
  - Bu "late registration" yaklaşımı race condition'ı önler

- `exit()` (satır 20-25):
  - `self.exited = true`
  - `self.exits.drain(..)` ile tümünü tüketir ve çalıştırır

- `fmt::Debug for Exit` (satır 28-32): sadece `exits.len()` yazdırır (closure içerik debug edilemez)

**Unsafe Bloklar:** Yok. **Test:** Yok.

**Nova Shield İçin Öğrenilen:**
- Graceful shutdown pattern: `Vec<Box<dyn FnOnce() + Send + Sync>>` ile callback kayıt; Nova Shield'ın shutdown koordinasyonu için aynı pattern — network bağlantılarını kapat, PoH'ı durdur, state'i flush et vb.
- `exited` flag + late registration: shutdown sırasında yeni bileşenler başlatılsa bile cleanup garantisi
- `drain(..)` ile tek kullanımlık consumption: double-exit sorununu önler

---

### [example_mocks.rs] (51 satır)

**Amaç:** Döngüsel bağımlılık olmadan kod örneklerinde dış crate API'larını taklit etmek.

**Modül Yapısı:**
- `#[doc(hidden)]`, `#![cfg(feature = "full")]`
- `pub mod solana_rpc_client::rpc_client`:
  - `RpcClient` struct (satır 23): mock implementasyon
  - `new(_url: String) -> Self`: bağlantı kurmuyor
  - `get_latest_blockhash()` → `Ok(Hash::default())`
  - `send_and_confirm_transaction()` → `Ok(Signature::default())`
- `pub mod solana_rpc_client_api::client_error`:
  - `ClientError` struct: `#[error("mock-error")]`
  - `Result<T>` type alias

**Nova Shield İçin Öğrenilen:**
- Test/örnek için mock crate pattern — döngüsel bağımlılık olmadan API örnekleri yazmak için; Nova Shield'da benzer mock layer, integration test'leri basitleştirir

---

## BÖLÜM 2: bloom/ — Bloom Filter Implementasyonu

---

### [bloom.rs] (482 satır)

**Amaç:** Gossip/duplicate detection için probabilistic set membership — hem seri (tek thread) hem concurrent versiyonları.

**Trait:**

- `BloomHashIndex` (satır 23-25): Her eleman için hash üretme arayüzü.
  - `fn hash_at_index(&self, hash_index: u64) -> u64`
  - `impl<T: AsRef<[u8]>> BloomHashIndex for T` (satır 143-147): FNV hasher ile `hash_index` key olarak kullanılır — farklı key'ler = farklı hash fonksiyonları

**Struct'lar:**

- `Bloom<T: BloomHashIndex>` (satır 27-33):
  - `keys: Vec<u64>` — hash fonksiyon key'leri (her biri bir "hash fonksiyon")
  - `bits: BitVec<u64>` — bit array, u64 chunk'larla saklı
  - `num_bits_set: u64` — set bit sayacı (doluluk takibi)
  - `_phantom: PhantomData<T>`
  - Serde, AbiExample implement eder → ağ üzerinden serileştirilebilir

- `ConcurrentBloom<T>` (satır 152-157): Lock-free concurrent implementasyon.
  - `num_bits: u64`
  - `keys: Vec<u64>`
  - `bits: Vec<AtomicU64>` — her u64 chunk atomic
  - `_phantom: PhantomData<T>`

- `ConcurrentBloomInterval<T>` (satır 238-248): Periyodik reset özellikli concurrent bloom.
  - `interval: AtomicInterval` — son reset zamanı
  - `bloom: ConcurrentBloom<T>`
  - `Deref<Target = ConcurrentBloom<T>>` → tüm ConcurrentBloom metodları doğrudan erişilebilir

**Kritik Fonksiyonlar (Bloom<T>):**

- `Bloom::random()` (satır 86-92): Optimal bloom filter boyutu hesaplar:
  ```
  num_bits = ceil(n * ln(p) / ln(1 / 2^ln(2)))
  num_bits = max(1, min(calculated, max_bits))
  num_keys = round((num_bits / n) * ln(2))
  ```
  keys = random u64 vektörü

- `num_bits()` (satır 93-97): `m = ceil((n * ln(p)) / ln(1/2^ln(2)))` — klasik bloom formülü

- `num_keys()` (satır 98-107): `k = max(1, round((m/n) * ln(2)))` — `n==0` özel durumu (0.0 / 0.0 = NaN'den kaçınır)

- `pos()` (satır 108-112): `key.hash_at_index(k).checked_rem(bits.len())` — zero-length koruması

- `add()` (satır 117-125): Her key için bit seti, yeni bit set edilirse `num_bits_set` artırılır

- `contains()` (satır 126-134): Her key için bit kontrol, ilk sıfır bit'te false döner

**Kritik Fonksiyonlar (ConcurrentBloom<T>):**

- `pos()` (satır 176-188): `(index, mask)` tuple döner:
  - `index = pos >> 6` — hangi AtomicU64
  - `mask = 1u64 << (pos & 63)` — hangi bit

- `add()` (satır 192-199): `fetch_or(mask, Ordering::Relaxed)` — atomic OR; `prev_val & mask == 0` ise `added = true`

- `contains()` (satır 202-208): `load(Ordering::Relaxed) & mask != 0` — atomic load

- `clear()` (satır 210-214): `store(0u64, Ordering::Relaxed)` her AtomicU64 için

**Dönüşümler:**
- `From<Bloom<T>> for ConcurrentBloom<T>` (satır 159-173): BitVec'i boxed slice'a alır, her u64'ü AtomicU64'e çevirir
- `From<ConcurrentBloom<T>> for Bloom<T>` (satır 217-234): AtomicU64'leri `into_inner()` ile alır, popcount ile `num_bits_set` hesaplar, `bits.truncate(num_bits)` ile fazla bitleri keser

**ConcurrentBloomInterval metodları:**
- `new()` (satır 254-260): `Bloom::random()` → `ConcurrentBloom::from()`
- `maybe_reset()` (satır 263-267): `interval.should_update(ms)` → `bloom.clear()`

**Matematiksel Parametre Doğrulaması (testlerden):**
- 10 item, 0.1 false rate → 3 key, 48 bit
- 100 item, 0.1 false rate → 480 bit; 0.01 false rate → 959 bit
- `num_keys(1000, 50) = 14`, `num_keys(2000, 50) = 28`, `num_keys(2000, 25) = 55`
- `1287 item, 0.1 false rate, max 7424 bit` → `3 key, 6168 bit, 97 AtomicU64 chunk`

**Unsafe Bloklar:** Yok doğrudan. `bv::BitVec` kütüphanesi içinde olabilir.

**Test Coverage** (satır 271-482):
- `test_bloom_filter`: boş/normal/saturated parametreler
- `test_add_contains`: bilinen key'ler ile false positive olmadan ekle/sorgula
- `test_random`: iki ayrı random filter'ın farklı key'lere sahip olduğu
- `test_filter_math`: matematiksel formüller doğrulanır
- `test_debug`: bit dizisi görsel doğrulama
- `test_atomic_bloom`: 1200 hash parallel ekleme, false positive < %20
- `test_atomic_bloom_round_trip`: Bloom ↔ ConcurrentBloom dönüşüm, multi-round test

**Nova Shield İçin Öğrenilen:**
- Gossip duplicate detection için bloom filter; Nova Shield'ın gossip katmanı için `ConcurrentBloomInterval` ile periyodik sıfırlamalı lock-free filter ideal
- `Ordering::Relaxed` yeterli: bloom filter hatalı false negative kabul eder (henüz setlenmemiş görünebilir) ama correctness bozulmaz; Nova Shield'da da aynı güvenlik modeli
- `fetch_or` ile atomic set — CAS döngüsü gerekmez; throughput açısından önemli
- Bloom interval reset süresi ayarlanabilir: gossip mesaj TTL'sine göre ayarlanmalı

---

### [lib.rs — bloom] (5 satır)

```rust
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]
pub mod bloom;
#[macro_use]
extern crate solana_frozen_abi_macro;
```

ABI stabilizasyonu için `frozen_abi_macro`, nightly feature flag'ı ile min_specialization desteği.

---

## BÖLÜM 3: bucket_map/ — Disk-Backed Concurrent HashMap

---

### [lib.rs — bucket_map] (11 satır)

```rust
pub type MaxSearch = u8;
pub type RefCount = u64;
```

- `MaxSearch = u8`: maksimum collision probe sayısı (varsayılan: 32)
- `RefCount = u64`: account başvuru sayacı

---

### [bucket_item.rs] (8 satır)

**Struct:**

- `BucketItem<T>` (satır 4-8): Bir bucket içinden okunan tek öğe.
  - `pubkey: Pubkey` — account adresi
  - `ref_count: RefCount` — referans sayısı
  - `slot_list: Vec<T>` — slot listesi (genellikle `Vec<(Slot, AccountInfo)>`)

---

### [bucket_stats.rs] (43 satır)

**Struct'lar:**

- `StartupBucketStats` (satır 7-10): Başlangıç istatistikleri.
  - `entries_created: AtomicU64` — yeni oluşturulan girişler
  - `entries_reused: AtomicU64` — disk'ten yeniden kullanılan girişler

- `BucketStats` (satır 13-37): Tek bucket performans metrikleri.
  - `resizes: AtomicU64`, `failed_resizes: AtomicU64`
  - `max_size: AtomicU64`, `resize_us: AtomicU64`
  - `new_file_us: AtomicU64`, `flush_file_us: AtomicU64`, `mmap_us: AtomicU64`
  - `find_index_entry_mut_us: AtomicU64`
  - `file_count: AtomicU64`, `total_file_size: AtomicU64`
  - `startup: StartupBucketStats`
  - `update_max_size()`: `fetch_max(size, Relaxed)` — atomic max
  - `resize_grow()`: `total_file_size.fetch_add(new-old, Relaxed)`

- `BucketMapStats` (satır 39-43): Global stats container.
  - `index: Arc<BucketStats>` — index bucket istatistikleri
  - `data: Arc<BucketStats>` — data bucket istatistikleri

---

### [index_entry.rs] (542 satır)

**Amaç:** Disk index bucket'ındaki her giriş için veri yapıları ve bit-level optimizasyonlar.

**Sabitler:**
- `OCCUPIED_OCCUPIED: u8 = 1`, `OCCUPIED_FREE: u8 = 0`
- `MAX_LEGAL_REFCOUNT: RefCount = RefCount::MAX >> 1` (63-bit max, 1 bit occupied için)

**Enum'lar:**

- `OccupiedEnumTag` (satır 292-300): 4 durum, 2-bit ile temsil:
  - `Free = 0` — boş slot
  - `ZeroSlots = 1` — dolu ama slot list boş
  - `OneSlotInIndex = 2` — tek slot, index'te inline
  - `MultipleSlots = 3` — data file'da

- `OccupiedEnum<'a, T>` (satır 303-314): Gerçek data'yı taşıyan enum:
  - `Free`
  - `ZeroSlots`
  - `OneSlotInIndex(&'a T)` — tek eleman referansı
  - `MultipleSlots(&'a MultipleSlots)` — data dosyası referansı

- `OccupyIfMatches` (satır 40-47):
  - `FoundDuplicate` — aynı pubkey, farklı değer → güncelleme yapılamaz
  - `SuccessfulInit` — başarıyla işaretlendi
  - `PubkeyMismatch` — farklı pubkey

**Struct'lar:**

- `DataBucketRefCountOccupiedHeader` (satır 33-37): Data bucket hücresi başlığı.
  - `packed_ref_count: PackedRefCount`

- `PackedRefCount` (satır 183-191): `#[bitfield(bits = 64)]`
  - `occupied: B1` — 1 bit occupied flag
  - `ref_count: B63` — 63 bit ref count

- `MultipleSlots` (satır 195-278): Çok slot entry için data file referansı.
  - `storage_cap_and_offset: PackedStorage` — kapasite pow2 + offset
  - `num_slots: Slot`
  - `data_bucket_from_num_slots(n)`: `ceil(log2(n))` — hangi data bucket'ı kullanılacak
  - `data_loc(storage)`: `offset << (current_pow2 - created_pow2)` — bucket grow sonrası konum remapping

- `PackedStorage` (satır 317-323): `#[bitfield(bits = 64)]`
  - `capacity_when_created_pow2: B8` — 8 bit
  - `offset: B56` — 56 bit (max offset: 2^56 - 1 ≈ 72 petabyte)

- `BucketWithHeader` (satır 52-80): Data bucket hücresi wrapper.
  - `capacity_pow2: Capacity`

- `IndexBucketUsingBitVecBits<T>` (satır 83-157): Index bucket — 2-bit enum tag ile slot durumu.
  - `enum_tag: BitVec` — kapasitede 2x kapasite bit (her giriş için 2 bit)
  - `set_bits/get_bits`: BitVec'te 2*ix ve 2*ix+1 bit çifti
  - `set_enum_tag/get_enum_tag`: 2-bit → `OccupiedEnumTag` dönüşüm

- `IndexEntry<T>` (satır 172-177): `#[repr(C)]`
  - `key: Pubkey` (32 byte)
  - `contents: SingleElementOrMultipleSlots<T>` (union, 16 byte)
  - Toplam boyut: 48 byte (test: `assert_eq!(size_of::<IndexEntry<u64>>(), 32 + 8 + 8)`)

- `SingleElementOrMultipleSlots<T>` (satır 281-288): Union.
  - `single_element: T` — inline tek eleman
  - `multiple_slots: MultipleSlots` — data file referansı

- `IndexEntryPlaceInBucket<T>` (satır 164-467): Index bucket içindeki belirli bir konumu temsil eden handle.
  - `ix: u64` — konumu
  - `get_slot_count_enum()`: enum tag okur, uygun OccupiedEnum varyantını döner
  - `get_multiple_slots_mut()`: MultipleSlots varyantı için mutable ref (unsafe içerir)
  - `set_slot_count_enum_value()`: enum tag yazar + data kopyalar
  - `init()`: yeni key için başlatır (ZeroSlots)
  - `occupy_if_matches()`: key + value eşleşirse slot'u işgal eder
  - `read_value()`: slot listesi ve ref_count döner
  - `key()`: entry pubkey'ini döner

**Unsafe Bloklar:**
- `get_slot_count_enum()` (satır 335-342): `index_entry.contents.single_element` ve `multiple_slots` union erişimi
- `get_multiple_slots_mut()` (satır 350-358): MultipleSlots için union erişimi
- `get_from_bytes<T>()` (satır 469-474): raw pointer cast → `&T`
- `get_mut_from_bytes<T>()` (satır 476-481): raw pointer cast → `&mut T`

**Test Coverage** (satır 483-542):
- `test_api`: MultipleSlots offset ve capacity_pow2 bağımsızlığı
- `test_size`: PackedStorage = 8 byte, IndexEntry<u64> = 48 byte
- `test_set_storage_offset_value_too_large`: 2^56 → panic
- `test_data_bucket_from_num_slots`: 0..512 ve u32::MAX senaryoları

**Nova Shield İçin Öğrenilen:**
- 4-durum enum + 2-bit tag: ZeroSlots/OneSlotInIndex/MultipleSlots ayrımı bellek verimliliği için kritik; tek elemanlı account'lar (çok yaygın) inline saklanır, disk I/O gerektirmez
- `PackedRefCount` bitfield: 63-bit ref_count + 1-bit occupied tek u64'te; Nova Shield'da benzer compact encoding
- `data_loc` remapping formülü: bucket büyütüldüğünde index güncellenmeden konum yeniden hesaplanabilir — zarif grow mekanizması

---

### [bucket_storage.rs] (738 satır)

**Amaç:** MmapMut ile disk-backed blob storage — her bucket fiziksel bir dosya.

**Sabitler:**
- `DEFAULT_CAPACITY_POW2: u8 = 5` → başlangıç kapasitesi: 2^5 = 32 eleman

**Trait'ler:**

- `BucketOccupied` (satır 41-66): Her hücrenin dolu/boş durumunu yönetme.
  - `occupy(&mut self, element: &mut [u8], ix: usize)` — işgal et
  - `free(&mut self, element: &mut [u8], ix: usize)` — serbest bırak
  - `is_free(&self, element: &[u8], ix: usize) -> bool`
  - `offset_to_first_data() -> usize` — header boyutu (u64 katı olmalı)
  - `new(capacity: Capacity) -> Self`
  - `copying_entry()` — no-op default, override edilebilir

- `BucketCapacity` (satır 68-73): Kapasite sorgulama.
  - `capacity() -> u64`
  - `capacity_pow2() -> u8` — default: `unimplemented!()`

**Enum:**
- `Capacity` (satır 110-115):
  - `Pow2(u8)` → `1 << pow2` eleman
  - `Actual(u64)` → tam sayı eleman
  - `Actual` → `capacity_pow2()` panic — yalnızca data bucket'larda kullanılır

**Struct:**

- `BucketStorage<O: BucketOccupied>` (satır 75-85):
  - `path: PathBuf` — disk dosyası yolu
  - `mmap: MmapMut` — memory-mapped dosya
  - `cell_size: u64` — her hücrenin byte boyutu
  - `count: Arc<AtomicU64>` — dolu hücre sayısı
  - `stats: Arc<BucketStats>`
  - `max_search: MaxSearch`
  - `contents: O` — BucketOccupied implementasyonu
  - `delete_file_on_drop: bool` — true → drop'ta dosya silinir

**Kritik Fonksiyonlar:**

- `new_with_capacity()` (satır 137-164): Ana constructor.
  - `cell_size = elem_size * num_elems + offset_to_first_data`
  - `bytes = allocate_to_fill_page(capacity, cell_size)` — tam sayfa dolacak şekilde yuvarlar
  - `new_map(drives, bytes, stats)` → rastgele isimli dosya oluşturur
  - `delete_file_on_drop: true`

- `allocate_to_fill_page()` (satır 166-181): `Actual` kapasitede PAGE_SIZE (4096 byte) katına yuvarlar.

- `load_on_restart()` (satır 223-249): Mevcut dosyayı yeniden açar.
  - `metadata.len().saturating_sub(offset) / elem_size` → kapasite hesaplar
  - `delete_file_on_drop: false` — var olan dosya silinmez

- `get_start_offset_with_header()` (satır 304-307): `cell_size * ix`
- `get_start_offset()` (satır 309-315): Header dahil veya hariç offset
- `get_header<T>()`, `get_header_mut<T>()`: İlk u64 chunk'ı header olarak okur/yazar
- `get<T>()`, `get_mut<T>()`: NoHeader offset ile data erişimi
- `get_slice<T>()`, `get_slice_mut<T>()` (satır 341-384): Raw pointer → slice dönüşümü

- `try_lock()` (satır 271-280): is_free ise occupy et ve true döner.
- `occupy()` (satır 284-295): `is_resizing=false` ise count artır.
- `free()` (satır 297-302): count azalt.

- `map_open_file()` (satır 388-447): Dosya aç veya oluştur → MmapMut.
  - `create=true`: dosya boyutuna byte yaz (teorik perf optimizasyonu), flush, mmap
  - `create=false`: sadece aç, mmap

- `new_map()` (satır 450-459): `thread_rng().gen_range(0..u128::MAX)` — rastgele dosya ismi. Saldırgan dosya adını tahmin edemez.

- `copy_contents()` (satır 463-497): Data bucket grow'u sırasında eski içeriği kopyalar.
  - `increment = new_pow2 - old_pow2`
  - `index_grow = 1 << increment`
  - `new_ix = old_ix * index_grow` — yeni konumu hesaplar
  - `ptr::copy_nonoverlapping` — memcpy

- `new_resized()` (satır 504-529): Yeni kapasiteli bucket oluşturur + eski içeriği kopyalar.

**Unsafe Bloklar:**
- `MmapMut::map_mut(&data)` (satır 430): mmap doğası gereği unsafe
- `get_slice<T>()` (satır 353-358): raw pointer → slice cast
- `get_slice_mut<T>()` (satır 378-383): raw pointer → mutable slice cast
- `copy_contents()` (satır 486-489): `ptr::copy_nonoverlapping`

**Test Coverage** (satır 542-738):
- `test_bucket_storage`: occupy/free/re-occupy döngüsü
- `test_load_on_restart_failures`: 0,1,47,48,49,4097 byte dosyalar
- `test_load_on_restart`: oluştur → kapat → yeniden yükle, veri korunuyor
- `test_header_bad_size`: u64 katı olmayan header → panic

**Nova Shield İçin Öğrenilen:**
- MmapMut tabanlı storage: kernel buffer cache otomatik I/O yönetimi; Nova Shield'ın disk-backed state için rocksdb yerine alternatif olarak değerlendirilebilir (okuma-yoğun workload'larda daha hızlı)
- Rastgele dosya ismi (u128): DoS koruması, dosya collsion önlemi
- `PAGE_SIZE = 4096` byte'a hizalama: OS page fault overhead minimize edilir
- `delete_file_on_drop` flag: restart sırasında dosya korunur; Nova Shield'da crash recovery için aynı pattern

---

### [bucket_api.rs] (160 satır)

**Amaç:** Tek bir bucket (pubkey shard) için public API — RwLock ile concurrent erişim yönetimi.

**Tür Tanımları:**
- `LockedBucket<T> = RwLock<Option<Bucket<T>>>`

**Struct:**

- `BucketApi<T>` (satır 19-30):
  - `drives: Arc<Vec<PathBuf>>`
  - `max_search: MaxSearch`
  - `stats: Arc<BucketMapStats>`
  - `bucket: LockedBucket<T>` — RwLock ile korunan, lazy init edilmiş bucket
  - `count: Arc<AtomicU64>` — giriş sayısı
  - `restartable_bucket: RestartableBucket` — restart state

**Kritik Tasarım Kararı:** `bucket: RwLock<Option<Bucket<T>>>` — lazy initialization: bucket ilk yazma anında oluşturulur.

**Kritik Fonksiyonlar:**

- `allocate_bucket()` (satır 92-102): Bucket hiç yoksa yeni oluşturur.

- `get_write_bucket()` (satır 104-112): **Core concurrency pattern**:
  1. Write lock al
  2. Bucket varsa `handle_delayed_grows()` — read lock sırasında birikmiş grow'ları uygula
  3. Bucket yoksa `allocate_bucket()`

- `grow()` (satır 119-125): **Read lock ile grow**: bucket varsa `bucket.grow(err)`. Grow'un uygulanması write lock'a defer edilir.

- `items_in_range()`, `keys()`, `read_value()`: Read lock ile erişim.

- `insert()`, `delete_key()`, `update()`, `try_write()`: `get_write_bucket()` ile write erişim.

- `batch_insert_non_duplicates()` (satır 136-139): Batch insert — write lock altında.

- `set_anticipated_count()` (satır 128-132): Resize hint — write lock altında.

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Grow'u read lock sırasında hesapla, write lock sırasında uygula — **minimum contention grow stratejisi**; Nova Shield'ın concurrent shard yönetimi için aynı deferred grow pattern
- `Option<Bucket<T>>` ile lazy init: bellek verimliliği, boş shard'lar için sıfır maliyet

---

### [bucket_map.rs] (634 satır)

**Amaç:** Pubkey → (slot_list, ref_count) eşlemesi için disk-backed concurrent hash map — Solana accounts index'inin temel veri yapısı.

**Sabitler:**
- `MAX_SEARCH_DEFAULT: MaxSearch = 32`: Collision probe sınırı.

**Struct'lar:**

- `BucketMapConfig` (satır 19-27):
  - `max_buckets: usize` — power of 2 zorunlu
  - `drives: Option<Vec<PathBuf>>` — disk yolları (None → TempDir)
  - `max_search: Option<MaxSearch>`
  - `restart_config_file: Option<PathBuf>` — restart state dosyası

- `BucketMap<T>` (satır 39-48):
  - `buckets: Vec<Arc<BucketApi<T>>>` — N bucket (N = max_buckets)
  - `drives: Arc<Vec<PathBuf>>`
  - `max_buckets_pow2: u8` — log2(max_buckets)
  - `stats: Arc<BucketMapStats>`
  - `temp_dir: Option<TempDir>` — test için geçici dizin
  - `erase_drives_on_drop: bool`

- `BucketMapError` (satır 69-77):
  - `DataNoSpace((u64, u8))` — data bucket index + mevcut kapasite pow2
  - `IndexNoSpace(u64)` — mevcut index kapasitesi

**Kritik Fonksiyonlar:**

- `BucketMap::new()` (satır 80-143): Constructor.
  1. `max_buckets` power-of-two assert
  2. `Restart::get_restart_file()` → mevcut restart state yükle
  3. restart yoksa eski drive'ları sil (erase_previous_drives)
  4. `Restart::new()` — yeni restart state oluştur
  5. drives: config'den veya TempDir
  6. `Restart::get_restartable_buckets()` → her bucket için RestartableBucket
  7. `max_buckets_pow2 = log2(max_buckets)`

- `bucket_ix()` (satır 193-200): **Sharding fonksiyonu**:
  - `read_be_u64(key.as_ref())` — pubkey'in ilk 8 byte'ını big-endian u64 olarak oku
  - `location >> (64 - max_buckets_pow2)` — üst N bit'i bucket index olarak al
  - Pubkey uniform distribution → bucket'lar eşit dolar

- `read_be_u64()` (satır 204-207): `u64::from_be_bytes(input[0..8].try_into())`

- CRUD metodları tümü `get_bucket(key)` → `BucketApi` delegesi.

**Drop impl** (satır 50-56): `erase_drives_on_drop && temp_dir.is_none()` → drive'ları sil.

**Test Coverage** (satır 209-634): Kapsamlı:
- `bucket_map_test_insert/insert2/update2/update/update_to_0_len/delete/delete_2`
- `bucket_map_test_n_drives`, `bucket_map_test_grow_read`, `bucket_map_test_n_delete`
- `hashmap_compare` (satır 391-633): **En kritik test** — 10000 rastgele operasyon (insert/update/delete/addref/unref), BucketMap sonuçlarını standart HashMap ile karşılaştırır; batch_insert de dahil

**Nova Shield İçin Öğrenilen:**
- `bucket_ix()` upper-bit sharding: pubkey'in ilk byte'ları en entropi yoğun kısım değil, ancak uniform Pubkey dağılımı için yeterli; Nova Shield'da kendi account hash → shard mapping doğru analiz edilmeli
- Restart state: crash sonrası disk dosyaları yeniden kullanılır — Nova Shield'da crash recovery için accounts index restart mekanizması (bu, her restart'ta accounts-db replay'in 10+ dakika sürmesini önler)
- `max_search = 32`: linear probe collision chain sınırı; Nova Shield'da hash table density %70-80 tutulmalı (32 probe ile güvenli doluluk seviyesi)

---

### [bucket.rs] (1582 satır)

**Amaç:** Tek bir pubkey shard'ının disk I/O, resize, hash collision yönetimi — bucket_map sisteminin kalbi.

**Struct'lar:**

- `ReallocatedItems<I, D>` (satır 34-48):
  - `index: Option<BucketStorage<I>>` — yeni index
  - `data: Option<(u64, BucketStorage<D>)>` — yeni data bucket + index

- `Reallocated<I, D>` (satır 51-89):
  - `active_reallocations: AtomicUsize` — bekleyen reallocation sayısı
  - `items: Mutex<ReallocatedItems<I, D>>`
  - `add_reallocation()`: `fetch_add(1, Release)` — yeni reallocation var
  - `get_reallocated()`: `load(Acquire) > 0` — bekleyen var mı? `compare_exchange` ile 0'a sıfırla

- `Bucket<T>` (satır 94-120): Ana struct.
  - `drives`, `index: BucketStorage<IndexBucket<T>>`, `random: u64`
  - `data: Vec<BucketStorage<DataBucket>>` — boyuta göre (pow2) data bucket'ları
  - `stats: Arc<BucketMapStats>`
  - `anticipated_size: u64` — beklenen eleman sayısı (grow hint)
  - `reallocated: Reallocated<...>` — deferred grow state
  - `at_least_one_entry_deleted: bool` — silme sonrası tam arama gerekli
  - `restartable_bucket: RestartableBucket`
  - `reused_file_at_startup: bool`

**Kritik Fonksiyonlar:**

- `Bucket::new()` (satır 123-179): Restart'ta disk dosyasını yeniden kullanmayı dener; başarısız olursa yeni dosya oluşturur. `random = thread_rng().gen()`.

- `find_index_entry_mut()` (satır 225-262): **Linear probe hash table arama**:
  - `ix = bucket_index_ix(key, random) % capacity`
  - `for i in ix..ix + max_search`: collision chain tarar
  - Boş slot bulunursa `first_free` kaydedilir
  - Key eşleşirse mevcut entry döner
  - `max_search` sonunda: `first_free` varsa → `Ok((None, first_free))`, yoksa → `Err(IndexNoSpace)`

- `bucket_create_key()` (satır 283-314): Boş slot bul ve occupy et.

- `batch_insert_non_duplicates()` (satır 336-393): **Startup bulk insert**:
  - `at_least_one_entry_deleted` assert — silme olduktan sonra kullanılamaz
  - `index_entries()` ile hash değerlerini hesapla
  - Sort by hash % cap (ascending, ama `reverse()` ile yığılıyor → `pop()` ascending order verir)
  - `batch_insert_non_duplicates_internal()` → başarısız olanlar için grow + retry

- `batch_insert_non_duplicates_reusing_file()` (satır 401-443): **Disk'te zaten var olan verileri yeniden kullan**:
  - `occupy_if_matches()` ile pubkey + value eşleşiyor mu? → Evet: SuccessfulInit
  - Duplicate → duplicates'e ekle
  - PubkeyMismatch → başka slot ara
  - Bulunamazsa `not_found`'a ekle

- `batch_insert_non_duplicates_internal()` (satır 449-514): İç batch insert.
  1. `try_to_reuse_disk_data` → `batch_insert_non_duplicates_reusing_file()`
  2. Kalan entry'ler için klasik linear probe: `try_lock()` → başarılı → `init()` + `set_slot_count_enum_value(OneSlotInIndex)`

- `try_write()` (satır 516-656): **Ana yazma fonksiyonu**:
  - `num_slots > 1 || ref_count != 1` → data file gerekli
  - Data file gerekli ama yok → `Err(DataNoSpace)`
  - `find_index_entry_mut()` → slot bul/oluştur
  - Veri kısa (0-1 slot, rc=1) → inline (ZeroSlots/OneSlotInIndex)
  - Uzun veri → `best_fit_bucket = ceil(log2(data_len))` → data file bucket
  - Aynı boyut bucket → in-place güncelle
  - Farklı boyut → `pos = random_start`, `max_search * 10` adım ara → yeni slot
  - Eski slot `old_data_entry_to_free` ile serbest bırakılır

- `delete_key()` (satır 658-674): Data file slot serbest bırakır + index'ten kaldırır. `at_least_one_entry_deleted = true`.

- `grow_index()` (satır 680-742): Index bucket'ı büyüt.
  - `new_capacity = max(current * 1.1, anticipated_size * 1.4)` — %10 büyüme veya beklenti * 1.4
  - Tüm eski girişleri yeniden hash'le; başarısızsa daha büyük boyut dene
  - Başarılı → `reallocated.add_reallocation()`

- `grow_data()` (satır 799-815): Data bucket'ı büyüt (pow2 + 1).

- `handle_delayed_grows()` (satır 844-857): Write lock altında çalışır. Bekleyen reallocation varsa `apply_grow_index()` veya `apply_grow_data()`.

- `bucket_index_ix()` (satır 817-826): `DefaultHasher` + key + random → u64. **Anti-DoS**: random olmadan saldırgan tüm pubkey'leri aynı bucket'a yönlendirebilir.

- `insert()` (satır 859-868): `try_write()` başarılı olana kadar grow + retry döngüsü.

- `update()` (satır 870-882): Mevcut değeri okur, updatefn'e verir, None → delete, Some → insert.

**Unsafe Bloklar:** Dolaylı olarak `get_slot_count_enum()` ve `get_multiple_slots_mut()` içindeki union erişimleri (`index_entry.rs`'den).

**Test Coverage** (satır 885-1582): 11 test fonksiyonu, kapsamlı:
- `test_batch_insert_non_duplicates_reusing_file_many_entries`: 3 reuse type senaryosu
- `test_batch_insert_non_duplicates_reusing_file_blank_file`, `_insert_twice`, `_insert_duplicate`, `_skip_one`, `_existing_zero`
- `test_index_entries`, `create_test_index`, `batch_insert_duplicates_internal_simple`
- `batch_insert_non_duplicates_internal_simple`
- `batch_insert_non_duplicates_internal_same_ix_exceeds_max_search`
- `test_occupy_if_matches`, `test_occupy_if_matches_panic`
- `batch_insert_after_delete`

**Nova Shield İçin Öğrenilen:**
- Linear probe `max_search = 32` ile hash table: %70-80 dolulukta bile 32 adımda bulunur. Nova Shield'da accounts index için aynı yaklaşım — rocksdb'den daha hızlı okuma
- `grow_index`: deferred grow (read lock hesapla, write lock uygula) — lock contention minimizasyonu; Nova Shield'ın concurrent state için kritik pattern
- `at_least_one_entry_deleted` flag: batch insert sadece silme öncesi çalışır — startup'ta bulk insert mümkün. Nova Shield genesis/snapshot yükleme için aynı optimization
- `random` hash seed: DoS koruması; Nova Shield'da her bucket için ayrı random seed zorunlu
- `batch_insert_non_duplicates_reusing_file`: Restart sonrası disk data yeniden kullanımı — yazma I/O sıfıra yakın; Nova Shield accounts index restart 10-20 dakika → saniyeye düşürebilir

---

### [restart.rs] (624 satır)

**Amaç:** Process restart sonrası disk bucket dosyalarının yeniden kullanılması için kalıcı state yönetimi.

**Sabitler:**
- `HEADER_VERSION: u64 = 1`

**Struct'lar:**

- `Header` (satır 20-32): `#[derive(Pod, Zeroable)]` — bytemuck ile zero-copy seri/deserializasyon.
  - `version: u64`, `buckets: u64`, `max_search: u8`
  - `_dummy: [u8; 15]` — u128 katına hizalama için padding
  - `assert!(size_of::<Header>() == size_of::<u128>() * 2)` — compile-time boyut kontrolü

- `OneIndexBucket` (satır 40-49): `#[derive(Pod, Zeroable)]`
  - `file_name: u128` — dosya adı (rastgele u128)
  - `random: u64` — bucket'ın hash random değeri
  - `_dummy: u64` — padding
  - `assert!(size_of::<OneIndexBucket>() == size_of::<u128>() * 2)`

- `Restart` (satır 57-59): `mmap: MmapMut` — tüm restart state tek dosyada.

- `RestartableBucket` (satır 61-70):
  - `restart: Option<Arc<Mutex<Restart>>>` — paylaşılan restart state
  - `index: usize` — bu bucket'ın indeksi
  - `path: Option<PathBuf>` — mevcut dosya yolu

**Kritik Fonksiyonlar:**

- `Restart::new()` (satır 122-144): Restart dosyası oluşturur.
  - `expected_len = size_of::<Header>() + max_buckets * size_of::<OneIndexBucket>()`
  - Header yazar, tüm bucket entry'leri sıfırlar

- `Restart::get_restart_file()` (satır 148-178): Mevcut restart dosyasını yükler.
  - Boyut eşleşmiyorsa → None
  - `version`, `buckets`, `max_search` eşleşmiyorsa → None
  - Başarılı → `Some(Restart{mmap})`

- `get_all_possible_index_files_in_drives()` (satır 186-205): Drive'lardaki tüm u128 isimli dosyaları hash map olarak döner.

- `get_restartable_buckets()` (satır 210-237): Her bucket için `RestartableBucket` oluşturur.
  - Restart state'teki `file_name`'e göre `paths` map'ten dosya yolunu çıkarır
  - Eşleşmeyen dosyalar silinir
  - Eşleşen dosyalar bucket'a atanır

- `RestartableBucket::set_file()` (satır 75-81): Mutex ile restart state günceller.
- `RestartableBucket::get()` (satır 84-90): Mutex ile oku.

- Layout metodları: `get_header()`, `get_bucket()` — bytemuck::from_bytes ile zero-copy okuma.

**Unsafe Bloklar:**
- `MmapMut::map_mut(&file)` (satır 165, 256) — mmap inherently unsafe

**Test Coverage** (satır 286-624):
- `test_header_alignment`: Header ve OneIndexBucket u128 katı
- `test_get_restartable_buckets`: 4 bucket, 1 eksik (skip=2), 1 ekstra dosya → ekstra silinir, eksik None
- `test_get_all_possible_index_files_in_drives`: valid/invalid u128 isimler, 2 drive
- `test_restartable_bucket_load`: yanlış bucket sayısı, yanlış max_search, yanlış version, yanlış boyut → tümü None
- `test_restartable_bucket`: set/get round-trip, process exit simülasyonu (drop), yeniden yükleme

**Nova Shield İçin Öğrenilen:**
- Zero-copy struct layout + compile-time size assert: crash-safe persistent state için temel pattern; Nova Shield'ın WAL/snapshot header'ları için aynı yaklaşım
- Restart state boyutu sabit ve bilinen → dosya boyutu uyuşmazlığı = yanlış versiyon tespiti; Nova Shield'da state dosyası formatı değişirse version check ile graceful fallback
- `bytemuck::Pod + Zeroable`: unsafe olmadan memory-mapped struct erişimi; Nova Shield'da rocksdb yerine bazı hot path'ler için custom mmap state

---

## GENEL ÖZET VE NOVA SHIELD TASARIM PRENSİPLERİ

### 1. Bloom Filter Mimarisi
Solana gossip katmanı için `ConcurrentBloomInterval` kullanır:
- FNV hash, random key'ler ile çoklu hash fonksiyonu simülasyonu
- Lock-free `AtomicU64` bit array
- Periyodik reset ile false positive oranı kontrol altında

**Nova Shield'a Uygulama**: P2P katmanında duplicate mesaj detection için aynı yapı kullanılabilir. Reset interval = gossip mesaj TTL (örn. 10s). Parametreler: n=50000 mesaj/s * 10s = 500000 item, false_rate=0.01, max_bits=10_000_000.

### 2. BucketMap Tasarım Deseni
Disk-backed accounts index için 4 katmanlı mimari:
- `BucketMap` → sharding (N = 2^k bucket, üst k bit ile)
- `BucketApi` → RwLock concurrent erişim + lazy init + deferred grow
- `Bucket` → linear probe hash table + indexed data file
- `BucketStorage` → MmapMut disk dosyası

**Nova Shield'a Uygulama**: Accounts state index için aynı mimari. Disk yolu: SSD'ler üzerinde birden fazla drive → paralel I/O. Startup batch insert → disk data reuse → çok hızlı yeniden başlatma.

### 3. Transaction Processing Utility'leri
- `NoncePartial` → `NonceFull` iki aşamalı işleme
- `limited_deserialize` → PACKET_DATA_SIZE ile OOM koruması
- `default_on_eof` → geriye dönük uyumlu struct migration
- `Exit` → graceful shutdown callback chain

**Nova Shield'a Uygulama**: Mempool ön-doğrulama → execution tam doğrulama iki aşaması. Nova Shield transaction boyut limiti: 1232 byte veya benzer sabit.

### 4. Güvenlik Desenleri
- BucketStorage dosya adı: rastgele u128 → tahmin edilemez
- Bucket random hash seed: per-bucket DoS koruması
- Durable nonce: `Versions::Legacy` hiçbir zaman verify edemez (downgrade koruması)
- `hash_to_partition` bias-free formula: saldırı yüzeyi azaltılmış shard mapping

### 5. Performans Desenleri
- `DEFAULT_TPU_COALESCE = 5ms`: batch window
- `allocate_to_fill_page`: PAGE_SIZE (4096B) hizalama
- `grow_index` deferred: read lock sırasında hesapla, write lock sırasında uygula
- `at_least_one_entry_deleted` flag: bulk insert path optimizasyonu
- `MAX_SEARCH_DEFAULT = 32`: %70-80 dolulukta yeterli
