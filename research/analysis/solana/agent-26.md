# Solana Analiz — Agent 26
## Kapsam: rpc/src/ — RPC Sistemi (16 dosya + 1 alt dizin)

**Toplam Dosya Sayısı:** 17 (rpc/account_resolver.rs dahil)
**Toplam Satır:** ~17,906 satır (rpc.rs: 9279, rpc_subscriptions.rs: 3130, rpc_pubsub.rs: 1424, rpc_service.rs: 898, rpc_subscription_tracker.rs: 789, optimistically_confirmed_bank_tracker.rs: 669, transaction_status_service.rs: 453, rpc_pubsub_service.rs: 515, rpc_health.rs: 214, cluster_tpu_info.rs: 248, parsed_token_accounts.rs: 102, rpc_cache.rs: 77, rpc_completed_slots_service.rs: 54, max_slots.rs: 7, transaction_notifier_interface.rs: 18, lib.rs: 29, account_resolver.rs: 15)

---

## DOSYA ANALİZLERİ

---

### [lib.rs] (29 satır)

**Amaç:** Crate kök modülü — tüm alt modülleri export eder ve makro crate'lerini kayıt eder.

**Struct/Enum/Trait Listesi:**
- Tanım yok; yalnızca `mod` ve `extern crate` direktifleri

**Kritik Öğeler:**
- `cluster_tpu_info` → private mod (dışarıya açık değil)
- `rpc_cache` → private mod (yalnızca crate içi kullanım)
- Diğer tüm modüller `pub mod` ile dışarıya açık
- `#[macro_use] extern crate log` → tüm crte boyunca `debug!`, `warn!`, `error!` macro'ları erişilebilir
- `#[macro_use] extern crate serde_derive` → tüm crate boyunca `#[derive(Serialize, Deserialize)]` otomatik
- `#[macro_use] extern crate serde_json` → yalnızca `#[cfg(test)]` → test JSON oluşturma kolaylaştırır
- `#[macro_use] extern crate solana_metrics` → `datapoint_info!`, `inc_new_counter_info!` macro'ları

**Güvenlik Notları:**
- Hiçbir unsafe blok yok
- cluster_tpu_info private tutulmuş: TPU adres bilgileri dışa sızmamalı

**Nova Shield İçin Öğrenilen:**
- RPC crate'i kendi içinde tutarlı bir modül yapısıyla bölünmüş: sağlık, abonelik, pubsub, servis ayrı katmanlarda
- `rpc_cache` private tutmak iyi pratik: implementasyon detayı dışarıya sızmamalı

---

### [max_slots.rs] (7 satır)

**Amaç:** Retransmit ve shred insert için en yüksek görülen slot değerlerini atomik counter olarak tutar.

**Struct/Enum/Trait Listesi:**
- `MaxSlots` (satır 4-7):
  - `retransmit: AtomicU64` — turbine retransmit aşamasından gelen en yüksek slot
  - `shred_insert: AtomicU64` — shred insert'ten gelen en yüksek slot

**Kritik Fonksiyonlar:**
- `#[derive(Default)]` → her iki AtomicU64 sıfır başlar

**Unsafe Bloklar:** Yok. `AtomicU64` güvenlidir.

**Güvenlik Notları:**
- Bu değerler `Ordering::Relaxed` ile okunuyor (bkz. rpc.rs: `get_max_retransmit_slot`, `get_max_shred_insert_slot`)
- Relaxed ordering doğru: sadece monoton artan bir counter, cross-thread senkronizasyona ihtiyaç duymaz

**Nova Shield İçin Öğrenilen:**
- RPC'nin validator iç durumuna bakabilmesi için "shared atomic counters" gerekir
- Nova Shield: benzer bir `MaxSlots` eşdeğeri ile shred/block işleme ilerlemesi RPC'den sorgulanabilir

---

### [transaction_notifier_interface.rs] (18 satır)

**Amaç:** Dış plug-in sistemlerinin (örn. bigtable, Geyser eklentileri) transaction bildirimlerini alabilmesi için trait tanımlar.

**Struct/Enum/Trait Listesi:**
- `TransactionNotifier` (satır 7-15): trait
  - `fn notify_transaction(slot, transaction_slot_index, signature, transaction_status_meta, transaction)`
- `TransactionNotifierArc` (satır 18): `Arc<dyn TransactionNotifier + Sync + Send>` type alias

**Kritik Fonksiyonlar:**
- `notify_transaction()`: Her başarıyla işlenen transaction için çağrılır, slot ve index bilgisiyle birlikte

**Unsafe Bloklar:** Yok.

**Güvenlik Notları:**
- `Send + Sync` bound: thread güvenliği zorunlu, implementasyon doğruluğu derleme zamanında denetlenir
- Trait object (`dyn`) kullanımı: runtime dispatch, dolaylı performans maliyeti var ama esneklik kazanılıyor

**Nova Shield İçin Öğrenilen:**
- Geyser-benzeri bir eklenti arayüzü için bu minimal trait pattern mükemmel
- Nova Shield event sistemi: `TransactionNotifier` + `AccountNotifier` + `BlockNotifier` üçlüsü aynı pattern ile uygulanabilir

---

### [rpc_cache.rs] (77 satır)

**Amaç:** `getLargestAccounts` RPC çağrısı için TTL tabanlı in-memory cache.

**Struct/Enum/Trait Listesi:**
- `LargestAccountsCache` (satır 10-13):
  - `duration: u64` — saniye cinsinden TTL
  - `cache: HashMap<Option<RpcLargestAccountsFilter>, LargestAccountsCacheValue>`
- `LargestAccountsCacheValue` (satır 16-20): private struct
  - `accounts: Vec<RpcAccountBalance>`
  - `slot: u64`
  - `cached_time: SystemTime`

**Kritik Fonksiyonlar:**
- `new(duration: u64) -> Self` (satır 23): TTL ile cache oluşturur
- `get_largest_accounts(filter) -> Option<(u64, Vec<RpcAccountBalance>)>` (satır 30-43):
  - `SystemTime::elapsed()` ile TTL kontrolü
  - Süresi dolmuş kayıt için `None` döner (expiry ile silmez, sadece ignore eder)
- `set_largest_accounts(filter, slot, accounts)` (satır 44-58):
  - `SystemTime::now()` ile kayıt eder
  - HashMap üzerine insert (overwrite)

**Rate Limiting / DoS Koruması:**
- Cache, getLargestAccounts'un pahalı full account scan'ini önler
- rpc_service.rs'de `LARGEST_ACCOUNTS_CACHE_DURATION = 60 * 60 * 2` → 2 saatlik TTL

**Unsafe Bloklar:** Yok.

**Güvenlik Notları:**
- Cache boyutu sınırsız değil: `RpcLargestAccountsFilter` enum'u sadece 3 değer alır (`Circulating`, `NonCirculating`, `None`), yani maksimum 3 giriş
- Süresi dolmuş kayıtlar hafızada kalır ama döndürülmez — hafıza büyüme riski minimumdur
- Cache'e erişim `Arc<RwLock<LargestAccountsCache>>` ile korunur (rpc_service.rs satır 377)

**Test:** `test_old_entries_expire` (satır 65-76) — 1 saniyelik TTL, sleep ve None kontrolü

**Nova Shield İçin Öğrenilen:**
- Pahalı scan operasyonları için basit TTL cache yeterlidir
- Nova Shield: `getSupply`, `getLargestAccounts` gibi ağır sorgular için aynı pattern
- Cache keyi olarak filter enum'u kullanmak çok yerinde (az sayıda olası değer)

---

### [rpc_completed_slots_service.rs] (54 satır)

**Amaç:** Blockstore'dan gelen "tamamlanmış slot" bildirimlerini WebSocket abonelerine iletir.

**Struct/Enum/Trait Listesi:**
- `RpcCompletedSlotsService`: unit struct (satır 19)

**Const/Static:**
- `COMPLETE_SLOT_REPORT_SLEEP_MS: u64 = 100` (satır 17)

**Kritik Fonksiyonlar:**
- `spawn(completed_slots_receiver, rpc_subscriptions, exit) -> JoinHandle<()>` (satır 21-53):
  - Thread adı: `solRpcComplSlot`
  - `recv_timeout(100ms)` ile non-blocking loop
  - Her tamamlanan slot için `rpc_subscriptions.notify_slot_update(SlotUpdate::Completed{slot, timestamp})`
  - `RecvTimeoutError::Disconnected` → temiz çıkış

**Unsafe Bloklar:** Yok.

**Güvenlik Notları:**
- exit flag kontrolü loop başında — hızlı shutdown garantisi
- Kanal kopması graceful exit ile sonuçlanır

**Nova Shield İçin Öğrenilen:**
- `recv_timeout` + exit flag loop pattern'i Solana'nın tüm background service'lerinde tekrarlanıyor
- Nova Shield: slot notification servisi için birebir kopyalanabilir pattern

---

### [parsed_token_accounts.rs] (102 satır)

**Amaç:** SPL Token hesaplarını human-readable JSON formatına ayrıştırır.

**Kritik Fonksiyonlar:**
- `get_parsed_token_account(bank, pubkey, account, overwrite_accounts) -> UiAccount` (satır 18-44):
  - Mint adresini account data'sından çıkarır (`get_token_account_mint`)
  - Mint hesabını `account_resolver::get_account_from_overwrites_or_bank` ile çeker (simulasyon desteği)
  - `UiAccountEncoding::JsonParsed` encoding ile döner
- `get_parsed_token_accounts<I>(bank, keyed_accounts) -> impl Iterator<Item=RpcKeyedAccount>` (satır 46-79):
  - Lazy iterator: lazily map → filter (sadece başarıyla parse edilenleri döner)
  - `mint_decimals: HashMap<Pubkey, u8>` ile mint decimals cache (duplicate fetch'i önler)
  - `UiAccountData::Json` olmayan hesapları filtreler (token olmayan hesapları çıkartır)
- `get_mint_owner_and_decimals(bank, mint) -> Result<(Pubkey, u8)>` (satır 84-93):
  - Native mint için hard-coded `spl_token::id()` ve `DECIMALS`
  - Diğer mintler için bank'tan çekip `StateWithExtensions<Mint>::unpack`
- `get_mint_decimals(data: &[u8]) -> Result<u8>` (satır 96-101): private, StateWithExtensions unpack

**Unsafe Bloklar:** Yok.

**Güvenlik Notları:**
- `Error::invalid_params` ile kullanıcıya belirsiz iç detay verilmez
- Mint validasyonu: token olmayan hesaplar filtre edilir

**Nova Shield İçin Öğrenilen:**
- Native asset (lamports) ile token varlık (SPL) ayrımı RPC katmanında handle edilmeli
- Lazy iterator ile N hesap için N mint isteği yerine memoized decimals cache gerekli

---

### [rpc_health.rs] (214 satır)

**Amaç:** Node'un cluster ile senkronizasyon durumunu 3 durumlu model ile belirler.

**Struct/Enum/Trait Listesi:**
- `RpcHealthStatus` (satır 12-16): enum
  - `Ok` — senkronize
  - `Behind { num_slots: Slot }` — geri kalmış (kaç slot geride olduğunu da bildirir)
  - `Unknown` — henüz bilgi yok (startup aşamasında veya blockstore hatalı)
- `RpcHealth` (satır 18-26): struct
  - `optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>`
  - `blockstore: Arc<Blockstore>`
  - `health_check_slot_distance: u64` — kaç slot fark tolerans olacağı
  - `override_health_check: Arc<AtomicBool>` — test/startup için bypass
  - `startup_verification_complete: Arc<AtomicBool>` — snapshot replay bitmeden Unknown
  - `stub_health_status` → yalnızca `#[cfg(test)]` — test mock'u

**Kritik Fonksiyonlar:**
- `check(&self) -> RpcHealthStatus` (satır 47-113):
  1. Test stub kontrolü (sadece cfg(test))
  2. `override_health_check` → true ise `Ok`
  3. `startup_verification_complete` → false ise `Unknown`
  4. `optimistically_confirmed_bank.bank.slot()` → my_latest_optimistically_confirmed_slot
  5. `blockstore.get_latest_optimistic_slots(1)` → cluster_latest_optimistically_confirmed_slot
  6. `my >= cluster - health_check_slot_distance` → `Ok`, aksi halde `Behind{num_slots}`
- `stub()` (satır 116-127): test yardımcısı, `Arc<Self>` döner
- `stub_set_health_status()` (satır 130-132): test'te durumu override eder

**Sağlık Mantığı:**
- `OptimisticallyConfirmedBank` (replay'de görülen) vs `Blockstore` optimistik slot'ları (gossip'ten de gelebilir) karşılaştırılır
- Bu fark önemli: gossip üzerinden bir slot optimistik confirm'lenmiş ama node henüz replay etmemiş olabilir
- `health_check_slot_distance` default değeri: rpc_service.rs'de `config.health_check_slot_distance` üzerinden gelir

**Test:** `test_get_health` (satır 147-213) — 11 assertion; override, startup, bekleyen slot, behind, ok durumları tam kapsanmış

**Nova Shield İçin Öğrenilen:**
- 3 durumlu sağlık modeli (Ok/Behind/Unknown) HTTP `/health` endpoint için mükemmel
- Startup_verification_complete gibi bir flag, snapshot yüklenirken "hazır değil" sinyali vermek için şart
- Nova Shield: validator startup'ında benzer bir startup gate gerekli

---

### [cluster_tpu_info.rs] (248 satır)

**Amaç:** Cluster'daki TPU adres bilgilerini takip eder; `sendTransaction` için leader seçimi yapar.

**Struct/Enum/Trait Listesi:**
- `ClusterTpuInfo` (satır 17-21): struct
  - `cluster_info: Arc<ClusterInfo>` — gossip peer listesi
  - `poh_recorder: Arc<RwLock<PohRecorder>>` — leader schedule bilgisi
  - `recent_peers: HashMap<Pubkey, (SocketAddr, SocketAddr)>` — key: validator pubkey, value: (UDP TPU, QUIC TPU)

**Kritik Fonksiyonlar (impl TpuInfo for ClusterTpuInfo):**
- `refresh_recent_peers(&mut self)` (satır 34-49):
  - `cluster_info.tpu_peers()` → tüm peer'ları alır
  - `filter_map` → hem UDP hem QUIC adresi olmayanları filtreler
  - Sonucu `recent_peers` HashMap'ine yazar (full overwrite)
- `get_leader_tpus(&self, max_count: u64, protocol: Protocol) -> Vec<&SocketAddr>` (satır 51-68):
  - `poh_recorder.leader_after_n_slots(i * NUM_CONSECUTIVE_LEADER_SLOTS)` ile sıradaki leaderleri bulur (0..max_count)
  - `recent_peers`'tan adres eşleştirir, deduplicate eder
  - Sonuç: UDP veya QUIC SocketAddr listesi
- `get_leader_tpus_with_slots(&self, max_count: u64, protocol: Protocol) -> Vec<(&SocketAddr, Slot)>` (satır 71-101):
  - Benzer ama slot bilgisini de döner
  - Ters iterasyon: `future_slot = max_count.wrapping_sub(future_slot)` (reversed order)
  - HashMap ile deduplicate: aynı leader aynı adresi paylaşır
  - `sort_by_key(|(_addr, slot)| *slot)` → slot sırasına göre sıralar

**Unsafe Bloklar:** Yok.

**Güvenlik Notları:**
- `O_NOFOLLOW` symlink koruması yok (bu dosyada değil, rpc_service.rs'de var)
- `lock().unwrap()` → zehirlenmiş mutex panic yapar (acceptable)

**Test:** `test_get_leader_tpus` (satır 130-247) — 3 validator, 3 socket çifti, birden fazla leader query senaryosu

**Nova Shield İçin Öğrenilen:**
- `sendTransaction` için çoklu leader hedef: hem UDP hem QUIC
- Leader schedule'dan gelecek liderler önceden bilinir → forward edilebilir
- Nova Shield mempool: transaction'ı doğrudan gelecek leader'lara iletmek için benzer bir mekanizma gerekir

---

### [rpc/account_resolver.rs] (15 satır)

**Amaç:** Transaction simulasyon senaryolarında overwrite edilmiş hesap değerlerini bank'a tercih eden tek fonksiyon.

**Kritik Fonksiyonlar:**
- `get_account_from_overwrites_or_bank(pubkey, bank, overwrite_accounts) -> Option<AccountSharedData>` (satır 7-15):
  - `overwrite_accounts.and_then(|accounts| accounts.get(pubkey).cloned())` → varsa override'ı döner
  - `.or_else(|| bank.get_account(pubkey))` → yoksa bank'tan çeker

**Nova Shield İçin Öğrenilen:**
- `simulateTransaction` için aynı pattern: override accounts + bank fallback
- Test amaçlı account override ile prod hesap durumu aynı kod yoluyla servis edilebilir

---

### [optimistically_confirmed_bank_tracker.rs] (669 satır)

**Amaç:** `BankNotification` kanalından gelen mesajları alır, `OptimisticallyConfirmedBank`'i günceller, RPC abonelerine ve slot abonelerine bildirim yapar.

**Struct/Enum/Trait Listesi:**
- `OptimisticallyConfirmedBank` (satır 30-32): `pub bank: Arc<Bank>` — RPC'nin "confirmed" view'u
  - `locked_from_bank_forks_root(bank_forks)` → root bank'tan başlatır
- `BankNotification` (satır 43-49): enum (Clone)
  - `OptimisticallyConfirmed(Slot)` — gossip veya replay'den confirmed slot sinyali
  - `Frozen(Arc<Bank>)` — bank freeze edildi
  - `NewRootBank(Arc<Bank>)` — yeni root
  - `NewRootedChain(Vec<Slot>)` — root chain (parent dahil)
- `SlotNotification` (satır 51-58): enum (Clone, Debug)
  - `OptimisticallyConfirmed(Slot)`
  - `Frozen((Slot, Slot))` — (slot, parent_slot)
  - `Root((Slot, Slot))` — (slot, parent_slot)
- `BankNotificationReceiver/Sender`: crossbeam_channel type alias'ları
- `BankNotificationSenderConfig` (satır 77-80):
  - `sender: BankNotificationSender`
  - `should_send_parents: bool`
- `SlotNotificationReceiver/Sender`: crossbeam_channel type alias'ları
- `OptimisticallyConfirmedBankTracker` (satır 85-87): `thread_hdl: JoinHandle<()>`

**Kritik Fonksiyonlar:**
- `new(...)` (satır 90-127): Thread `solOpConfBnkTrk` spawn eder; exit flag loop, `recv_notification` dispatcher
- `recv_notification(...)` (satır 130-156): `recv_timeout(1 sn)` → `process_notification` dispatch
- `notify_slot_status(...)` (satır 158-175): Tüm `slot_notification_subscribers`'a klonlanarak iletir, hata durumunda uyarı verir
- `notify_or_defer(...)` (satır 177-206):
  - Bank dondurulduysa (`is_frozen()`) ve slot > last_notified → gossip bildirimi + prioritization fee finalize
  - Bank henüz dondurulmadıysa ama root'tan büyükse → `pending_optimistically_confirmed_banks`'e ekle (deferred)
- `notify_or_defer_confirmed_banks(...)` (satır 208-235): Bank'ın parent zincirini tararak notify_or_defer uygular
- `notify_new_root_slots(...)` (satır 237-263): NewRootedChain'i sıralar, en son notified'dan büyük slot'ları `SlotNotification::Root` ile gönderir
- `process_notification(...)` (satır 265-388): ANA FONKSİYON
  - `OptimisticallyConfirmed(slot)`:
    - Bank varsa ve frozen ise → optimistically_confirmed_bank güncelle (monoton artış)
    - slot > highest_confirmed_slot → notify_or_defer_confirmed_banks çağır
    - `pending_optimistically_confirmed_banks`'e ekle (bank henüz replay'e gelmemişse)
    - SlotUpdate::OptimisticConfirmation bildirimi her zaman gönderilir
  - `Frozen(bank)`:
    - SlotUpdate::Frozen bildirimi (stats ile: tx sayısı, hata sayısı)
    - SlotNotification::Frozen gönder
    - `pending_optimistically_confirmed_banks.remove(bank.slot())` → bekleyen varsa şimdi işle
  - `NewRootBank(bank)`:
    - optimistically_confirmed_bank güncelle
    - pending listesi temizle (root geçmiş slotları)
  - `NewRootedChain(roots)`:
    - `notify_new_root_slots` çağır
- `close/join()` (satır 390-396): thread join

**Optimistic Confirmation vs Finalized Farkı:**
- `OptimisticallyConfirmed`: cluster'da 2/3 stake onayı var, replay henüz tamamlanmamış olabilir
- `Finalized` (root): node kendi replay etti + rooted
- Tracker bu ikisini ayrı tutar: gossip'ten gelen optimistik onay hemen RPC'ye yansır, ancak `bank.is_frozen()` kontrolü ile gerçek replay tamamlanmadan aboneler bilgilendirilmez
- `pending_optimistically_confirmed_banks`: gossip daha hızlı gelebilir, replay sonrasında işlenir

**Güvenlik Notları:**
- `write().unwrap()` → mutex zehirlenmesi panic; kritik sistemde acceptable (crash > tutarsız durum)
- Monoton slot güncelleme: `bank.slot() > w_optimistically_confirmed_bank.bank.slot()` kontrolü race condition'ı önler
- `pending_optimistically_confirmed_banks.retain(|&s| s > root_slot)` → artık geçerli olmayan bekleyenleri temizler

**Test:** `test_process_notification` (satır 422-668) — 668 satır boyunca slot 0'dan 7'ye kadar tüm notification türleri kapsamlı test edilmiş; 8 process_notification çağrısı, SlotNotification sayım kontrolleri

**Nova Shield İçin Öğrenilen:**
- "Optimistic vs Finalized" iki katmanlı kesinlik modeli: gossip hızlı, replay güvenli
- Deferred pending list: gossip önce gelirse beklet, replay sonrası bildir
- PrioritizationFeeCache'in `finalize_priority_fee` burada tetiklenmesi: blok kapandığında fee istatistiklerini kilitlemek
- Nova Shield: iki ayrı commitment level (optimistic/finalized) için aynı tracker pattern

---

### [transaction_status_service.rs] (453 satır)

**Amaç:** Transaction işlem sonuçlarını Blockstore'a yazar ve plug-in notifier'larını çağırır.

**Struct/Enum/Trait Listesi:**
- `TransactionStatusService` (satır 23-25): `thread_hdl: JoinHandle<()>`

**Kritik Fonksiyonlar:**
- `new(write_transaction_status_receiver, max_complete_transaction_status_slot, enable_rpc_transaction_history, transaction_notifier, blockstore, enable_extended_tx_metadata_storage, exit) -> Self` (satır 28-57):
  - Thread adı: `solTxStatusWrtr`
  - Loop: exit check + `write_transaction_status_batch`
- `write_transaction_status_batch(...)` (satır 59-200): `recv_timeout(1 sn)`
  - `TransactionStatusMessage::Batch(batch)`: her tx için:
    1. `lamports_per_signature` hesapla (durable nonce veya blockhash'ten)
    2. `get_fee_for_message_with_lamports_per_signature` ile ücreti hesapla
    3. `get_account_locks_unchecked` → writable/readonly listeler (status yazımı için)
    4. `map_inner_instructions` → inner instruction'ları düzleştir
    5. `TransactionStatusMeta` oluştur (status, fee, pre/post balances, inner ix, logs, token balances, rewards, loaded addresses, return_data, compute_units)
    6. `transaction_notifier.notify_transaction(...)` → plug-in hook
    7. `enable_extended_tx_metadata_storage` veya notifier yoksa: log, inner ix, return data silinir (disk tasarrufu)
    8. `enable_rpc_transaction_history` varsa: memo yazılır, tx status Blockstore'a yazılır
  - `TransactionStatusMessage::Freeze(slot)`: `max_complete_transaction_status_slot.fetch_max(slot, SeqCst)` → atomik güncelleme
- `join()`: thread join

**Önemli Detaylar:**
- `izip!` macro'su (itertools): paralel iterasyon, N tuple eleman senkron okunur
- `get_account_locks_unchecked()`: lock bilgisi ya önceden doğrulandı ya da burada sadece metadata için kullanılıyor
- `DurableNonceFee::Valid(lamports_per_signature)` vs `DurableNonceFee::Invalid`: geçersiz nonce → fee metadata yok
- `SeqCst` ordering: en güçlü memory order, cross-thread görünürlük garantisi — slot ordering kritik

**Unsafe Bloklar:** Yok.

**Güvenlik Notları:**
- `.expect("Expect database write to succeed")` → Blockstore yazma hatası panic! — kasıtlı: veri kaybı kabul edilemez
- Log/inner_ix/return_data opsiyonel silme: gizlilik değil disk tasarrufu; notifier varsa her zaman tam veri gider

**Test:** `test_notify_transaction` (satır 321-452) — TestTransactionNotifier, DashMap, NonceFull oluşturma, tam TransactionStatusMeta doğrulama

**Nova Shield İçin Öğrenilen:**
- TransactionStatusMeta yapısı: fee, pre/post balances, compute units, inner ix, logs, return data — tam kayıt tutumunu kapsıyor
- Plug-in notifier hook: "Geyser benzeri" sistemler için `TransactionNotifierArc` pattern
- `SeqCst` ile max slot güncelleme: WebSocket abonelerine hangi slot'un hazır olduğunu bildirmek için
- Nova Shield: blockstore'a yazarken panic yerine kanal tabanlı hata işleme tercih edilmeli

---

### [rpc_service.rs] (898 satır)

**Amaç:** HTTP JSON-RPC server lifecycle — başlatma, middleware, snapshot endpoint, health endpoint, shutdown.

**Struct/Enum/Trait Listesi:**
- `JsonRpcService` (satır 61-68): `thread_hdl: JoinHandle<()>`, `close_handle: Option<CloseHandle>`
  - `request_processor: JsonRpcRequestProcessor` — sadece `#[cfg(test)]`
- `RpcRequestMiddleware` (satır 70-77): hyper request middleware
  - `ledger_path: PathBuf`
  - `full_snapshot_archive_path_regex: Regex`
  - `incremental_snapshot_archive_path_regex: Regex`
  - `snapshot_config: Option<SnapshotConfig>`
  - `bank_forks: Arc<RwLock<BankForks>>`
  - `health: Arc<RpcHealth>`

**Const/Static:**
- `FULL_SNAPSHOT_REQUEST_PATH: &str = "/snapshot.tar.bz2"` (satır 57)
- `INCREMENTAL_SNAPSHOT_REQUEST_PATH: &str = "/incremental-snapshot.tar.bz2"` (satır 58)
- `LARGEST_ACCOUNTS_CACHE_DURATION: u64 = 60 * 60 * 2` (satır 59) → 7200 saniye = 2 saat

**Kritik Fonksiyonlar:**

`impl RpcRequestMiddleware`:
- `new(ledger_path, snapshot_config, bank_forks, health)` (satır 80-100):
  - Snapshot regex'lerini `Regex::new` ile compile eder
- `redirect(location)` (satır 102-108): HTTP 303 See Other
- `not_found()` (satır 110-114): HTTP 404
- `internal_server_error()` (satır 116-123): HTTP 500 (dead_code)
- `strip_leading_slash(path) -> Option<&str>` (satır 125-127):
  - `path.strip_prefix('/')` → `/` ile başlamayanlar için `None` (path traversal önleme)
- `is_file_get_path(&self, path) -> bool` (satır 129-144):
  - `DEFAULT_GENESIS_DOWNLOAD_PATH` → true
  - snapshot_config yoksa → false
  - Regex match: full veya incremental snapshot archive formatı
- `open_no_follow(path) -> io::Result<File>` (satır 146-161):
  - Unix: `libc::O_NOFOLLOW` ile açar — **symlink takip etmez** (path traversal/symlink attack koruması)
  - Non-Unix: normal open (TODO notu var)
- `find_snapshot_file(&self, stem) -> PathBuf` (satır 163-190):
  - Önce local dizinde arar, sonra remote snapshot dizininde
- `process_file_get(&self, path) -> RequestMiddlewareAction` (satır 192-234):
  - `strip_leading_slash` ile path prefix doğrulama
  - `std::fs::metadata` ile dosya boyutu
  - Async `open_no_follow` + FramedRead stream → chunked HTTP response
  - `should_validate_hosts: true` → host header doğrulaması aktif
- `health_check(&self) -> &'static str` (satır 236-244): "ok" | "behind" | "unknown"

`impl RequestMiddleware for RpcRequestMiddleware`:
- `on_request(request) -> RequestMiddlewareAction` (satır 248-312):
  1. Snapshot redirect: `/snapshot.tar.bz2` → en yüksek snapshot dosyasına HTTP 303
  2. `/v0/circulating-supply`, `/v0/total-supply` → `process_rest` text response
  3. Regex match snapshot dosyası → `process_file_get`
  4. `/health` → `health_check`
  5. Aksi: JSON-RPC handler'a devret

`fn process_rest(bank_forks, path) -> Option<String>` (satır 315-336):
- `/v0/circulating-supply`: `bank.capitalization() - non_circulating_supply` → sol cinsinden
- `/v0/total-supply`: `bank.capitalization()` → sol cinsinden

`impl JsonRpcService`:
- `new(rpc_addr, config, snapshot_config, bank_forks, ...) -> Result<Self, String>` (satır 338-579):
  - `rpc_threads = max(1, config.rpc_threads)`
  - `RpcHealth::new(...)` + `LargestAccountsCache::new(7200)`
  - Tokio multi-thread runtime: `rpc_threads` worker, `solRpcEl` thread adı
  - BigTable conditional init (bigtable_config varsa)
  - `JsonRpcRequestProcessor::new(...)` + transaction sender channel
  - `ClusterTpuInfo::new(...)` → `SendTransactionService::new_with_config(...)`
  - Thread `solJsonRpcSvc`: io handler extend, server builder
  - `x-bigtable: disabled` header → `clone_without_bigtable()` (per-request bigtable disable)
  - CORS: `AllowOnly([AccessControlAllowOrigin::Any])`, `cors_max_age(86400)`
  - `max_request_body_size` → `MAX_REQUEST_BODY_SIZE = 50KB` default
  - Server `wait()` → blocking, `exit_bigtable_ledger_upload_service.store(true)` shutdown sonrası
  - `validator_exit.register_exit` → server.close() shutdown hook
- `exit(&mut self)` (satır 581-585): `close_handle.close()`
- `join(mut self)` (satır 587-590): `exit()` + `thread_hdl.join()`

**Rate Limiting / DoS Koruması:**
- `max_request_body_size = 50KB` (satır 458, 460) — büyük body'ler reddedilir
- Snapshot ve genesis file'lar için sadece izin verilen yollar (`is_file_get_path`)
- `O_NOFOLLOW` — symlink attack koruması
- `should_validate_hosts: true` — DNS rebinding koruması
- CORS açık (any origin) — bu bir tasarım kararı: public RPC node için makul

**RPC Grupları (MetaIoHandler'a eklenenler):**
- Minimal (her zaman): `rpc_minimal::MinimalImpl`
- Full API (config.full_api varsa):
  - `rpc_bank::BankDataImpl`
  - `rpc_accounts::AccountsDataImpl`
  - `rpc_accounts_scan::AccountsScanImpl`
  - `rpc_full::FullImpl`
  - `rpc_deprecated_v1_7::DeprecatedV1_7Impl`
  - `rpc_deprecated_v1_9::DeprecatedV1_9Impl`
  - `rpc_deprecated_v1_18::DeprecatedV1_18Impl`
- Obsolete (config.obsolete_v1_7_api varsa):
  - `rpc_obsolete_v1_7::ObsoleteV1_7Impl`

**Unsafe Bloklar:** Yok (platform koşullu `O_NOFOLLOW` güvenli sarmalama).

**Test (3 test):**
- `test_rpc_new` (satır 615-679): Tam server start/stop, bakiye kontrolü
- `test_process_rest_api` (satır 691-700): Circulating supply == total supply (genesis'te)
- `test_strip_prefix` (satır 702-728): Path traversal senaryoları
- `test_is_file_get_path` (satır 731-837): 30+ assertion, path traversal, symlink, unicode emoji senaryoları
- `test_process_file_get` (satır 839-896): Var olmayan, var olan, ve symlink dosya senaryoları

**Nova Shield İçin Öğrenilen:**
- `O_NOFOLLOW` file serving güvenliği şart: snapshot/genesis dosyaları için symlink attack gerçek tehdit
- `max_request_body_size = 50KB` — DoS için kritik boundary
- `x-bigtable: disabled` header ile per-request feature disable: gizli admin feature'u
- CORS `cors_max_age(86400)` — preflight cacheleme (1 gün)
- Full API vs Minimal API ayrımı: validator node vs API node farkını net çizer
- BigTable ledger upload servisi conditional init: uzun dönem depolama opsiyonel
- Nova Shield: Tam/Kısıtlı API modu için `full_api` flag eşdeğeri şart

---

### [rpc_subscription_tracker.rs] (789 satır)

**Amaç:** WebSocket aboneliklerini takip eder: subscription oluşturma/silme, multi-index lookup, logs filtre senkronizasyonu.

**Struct/Enum/Trait Listesi:**
- `SubscriptionId(u64)` (satır 28): newtype wrapper, `From<u64>` ve `From<SubscriptionId>` impl'i
- `SubscriptionParams` (satır 43-53): enum (Debug, Clone, PartialEq, Eq, Hash)
  - `Account(AccountSubscriptionParams)`
  - `Block(BlockSubscriptionParams)`
  - `Logs(LogsSubscriptionParams)`
  - `Program(ProgramSubscriptionParams)`
  - `Signature(SignatureSubscriptionParams)`
  - `Slot`, `SlotsUpdates`, `Root`, `Vote` — parametresiz
- `AccountSubscriptionParams` (satır 126-131): pubkey, encoding, data_slice, commitment
- `BlockSubscriptionParams` (satır 133-141): commitment, encoding, kind, transaction_details, show_rewards, max_supported_transaction_version
- `BlockSubscriptionKind` (satır 143-147): `All` | `MentionsAccountOrProgram(Pubkey)`
- `LogsSubscriptionParams` (satır 149-152): kind, commitment
- `LogsSubscriptionKind` (satır 155-160): `All` | `AllWithVotes` | `Single(Pubkey)`
- `ProgramSubscriptionParams` (satır 162-170): pubkey, filters, encoding, data_slice, commitment, with_context
- `SignatureSubscriptionParams` (satır 172-177): signature, commitment, enable_received_notification
- `SubscriptionControl(Arc<SubscriptionControlInner>)` (satır 180): newtype, Clone
- `WeakSubscriptionTokenRef(Weak<SubscriptionTokenInner>, SubscriptionId)` (satır 181): weak ref + id
- `SubscriptionControlInner` (satır 183-190): private struct
  - `subscriptions: DashMap<SubscriptionParams, WeakSubscriptionTokenRef>` — lock-free concurrent map
  - `next_id: AtomicU64`
  - `max_active_subscriptions: usize`
  - `sender: crossbeam_channel::Sender<TimestampedNotificationEntry>`
  - `broadcast_sender: broadcast::Sender<RpcNotification>`
  - `counter: TokenCounter` — metrics
- `SubscriptionInfo` (satır 317-324):
  - `id: SubscriptionId`
  - `params: SubscriptionParams`
  - `method: &'static str`
  - `pub last_notified_slot: RwLock<Slot>` — son bildirim yapılan slot (concurrent write)
  - `commitment: Option<CommitmentConfig>`
- `Error` (satır 344-347): enum, `TooManySubscriptions`
- `LogsSubscriptionsIndex` (satır 350-356): private struct
  - `all_count`, `all_with_votes_count`: sayaçlar
  - `single_count: HashMap<Pubkey, usize>` — adres başına abonelik sayısı
  - `bank_forks: Arc<RwLock<BankForks>>`
- `SubscriptionsTracker` (satır 417-426):
  - `logs_subscriptions_index: LogsSubscriptionsIndex`
  - `by_signature: HashMap<Signature, HashMap<SubscriptionId, Arc<SubscriptionInfo>>>` — multi-id
  - `commitment_watchers: HashMap<SubscriptionId, Arc<SubscriptionInfo>>` — finalized/processed
  - `gossip_watchers: HashMap<SubscriptionId, Arc<SubscriptionInfo>>` — confirmed (gossip)
  - `node_progress_watchers: HashMap<SubscriptionParams, Arc<SubscriptionInfo>>` — slot/root/vote
- `SubscriptionTokenInner` (satır 537-541): private, `Drop` impl ile RAII unsubscribe
- `SubscriptionToken(Arc<SubscriptionTokenInner>, CounterToken)` (satır 578-589): public RAII handle

**SubscriptionParams metodları:**
- `method() -> &'static str`: notification method adı (accountNotification, logsNotification, ...)
- `commitment() -> Option<CommitmentConfig>`
- `is_commitment_watcher() -> bool`: confirmed değilse (finalized/processed) → commitment watcher
- `is_gossip_watcher() -> bool`: confirmed ise → gossip watcher
- `is_node_progress_watcher() -> bool`: Slot, SlotsUpdates, Root, Vote

**Kritik Fonksiyonlar:**

`impl SubscriptionControl`:
- `new(max_active_subscriptions, sender, broadcast_sender) -> Self` (satır 193-206): DashMap init, AtomicU64(0), TokenCounter
- `broadcast_receiver() -> broadcast::Receiver` (satır 208-210): her bağlantı için yeni receiver
- `subscribe(params) -> Result<SubscriptionToken, Error>` (satır 212-263):
  - `DashEntry::Occupied` + `weak.upgrade()` → var olan subscription'ı paylaş
  - `weak.upgrade().is_none()` → dropping sırasında race; id'yi koru, yeni token oluştur
  - `DashEntry::Vacant` → `count >= max_active_subscriptions` → `TooManySubscriptions` döner
  - Yeni subscription: `fetch_add(1)`, NotificationEntry::Subscribed kanalına gönder
- `total() -> usize`: toplam abonelik sayısı

`impl LogsSubscriptionsIndex`:
- `add(params)` / `remove(params)` (satır 359-414): sayaçları günceller → `update_config()`
- `update_config(&self)` (satır 387-414): `bank_forks.root_bank().transaction_log_collector_config` günceller
  - AllWithVotes > All > OnlyMentionedAddresses önceliği
  - Abonelik değişikliği anında bank'ın log filtresi güncellenir

`impl SubscriptionsTracker`:
- `subscribe(params, id, last_notified_slot_fn)` (satır 444-479): 5 index'e eşzamanlı kayıt
- `unsubscribe(params, id)` (satır 481-516): tüm index'lerden silir, tutarsızlık için warn!
- 4 getter: `by_signature()`, `commitment_watchers()`, `gossip_watchers()`, `node_progress_watchers()`

`impl Drop for SubscriptionTokenInner` (satır 551-576):
- **RAII unsubscribe**: token drop edildiğinde otomatik unsubscribe
- DashEntry'den strong_count == 0 kontrolü → race condition'a karşı
- `NotificationEntry::Unsubscribed` kanalına gönder, DashMap'ten sil
- Metrics: `datapoint_info!("rpc-subscription")`

**Memory Leak Analizi:**
- `WeakSubscriptionTokenRef`: `Weak<SubscriptionTokenInner>` → son token drop edildiğinde `DashMap` girişi de silinir
- `strong_count == 0` kontrolü: drop yarış koşulu (iki thread aynı anda drop ediyor) güvenli şekilde ele alınmış
- Signature abonelikleri: `by_signature` nested HashMap → son subscriber düşünce dış giriş de silinir
- Logs index `single_count` → 0 olduğunda entry `remove()` ile Map'ten çıkar

**Rate Limiting:**
- `max_active_subscriptions` → `MAX_ACTIVE_SUBSCRIPTIONS = 1_000_000` (rpc_pubsub_service.rs'den)
- Aşılırsa `inc_new_counter_info!("rpc-subscription-refused-limit-reached", 1)` metrics

**Test (4 test):**
- `notify_subscribe` (satır 645-651): sub → drop → unsubscribe bildirimi
- `notify_subscribe_multiple` (satır 654-665): paylaşılan token, son drop'ta unsubscribe
- `notify_subscribe_two_subscriptions` (satır 668-697): 2 farklı tip abonelik, id monoton artış
- `subscription_info` (satır 699-733): Slot params → node_progress, Account finalized → commitment_watcher
- `subscription_indexes` (satır 736-788): 4 farklı abonelik tipi, index count kontrolü

**Nova Shield İçin Öğrenilen:**
- **RAII Drop pattern** ile abonelik yönetimi: token drop = unsubscribe; hafıza sızıntısı imkansız
- `DashMap<SubscriptionParams, Weak<...>>` kombinasyonu: lock-free, deduplication, auto-cleanup
- Multi-index tracker (by_signature, commitment_watchers, gossip_watchers, node_progress_watchers): farklı event tiplerini farklı watcher setlerine yönlendirme
- Logs filtre `update_config()`: abonelik değişikliği anında bank log koleksiyonunu güncelle — opsiz log toplamayı önler
- Nova Shield WebSocket sistemi için bu pattern doğrudan alınabilir

---

### [rpc_subscriptions.rs] (3130 satır)

**Amaç:** WebSocket bildirim işçi thread'i — event kuyruğundan tüketir, bank/gossip watcher'ları paralel bildirir.

**Const/Static:**
- `RECEIVE_DELAY_MILLIS: u64 = 100` (satır 59)
- `RPC_NOTIFICATIONS_METRICS_SUBMISSION_INTERVAL_MS: Duration = 2000ms` (satır 208)

**Struct/Enum/Trait Listesi:**
- `TimestampedNotificationEntry` (satır 79-82):
  - `entry: NotificationEntry`
  - `queued_at: Instant` — kuyruğa alınma zamanı (gecikme ölçümü için)
- `NotificationEntry` (satır 93-103): enum
  - `Slot(SlotInfo)`, `SlotUpdate(SlotUpdate)`, `Vote((Pubkey, VoteTransaction, Signature))`
  - `Root(Slot)`, `Bank(CommitmentSlots)`, `Gossip(Slot)`, `SignaturesReceived((Slot, Vec<Signature>))`
  - `Subscribed(SubscriptionParams, SubscriptionId)`, `Unsubscribed(SubscriptionParams, SubscriptionId)`
- `RpcNotification` (satır 174-179):
  - `subscription_id: SubscriptionId`
  - `is_final: bool` — signature notification gibi tek seferlik
  - `json: Weak<String>` — shared string, broadcast channel'a kopyalanmadan geçer
  - `created_at: Instant` — latency ölçümü
- `RpcNotificationResponse<T>` (satır 182-185): `context: RpcNotificationContext`, `value: T`
- `RpcNotificationContext` (satır 203-206): `slot: Slot`
- `RecentItems` (satır 210-215):
  - `queue: VecDeque<Arc<String>>` — son N JSON mesajını tutar
  - `total_bytes: usize`, `max_len: usize`, `max_total_bytes: usize`
  - `last_metrics_submission: Instant`
- `RpcNotifier` (satır 263-266):
  - `sender: broadcast::Sender<RpcNotification>`
  - `recent_items: Mutex<RecentItems>`
- `PubsubNotificationStats` (satır 477-482): entry count ve zaman metrikleri
- `RpcSubscriptions` (satır 511-517):
  - `notification_sender: Option<Sender<TimestampedNotificationEntry>>` — main channel
  - `t_cleanup: Option<JoinHandle<()>>` — notification processor thread
  - `exit: Arc<AtomicBool>`
  - `control: SubscriptionControl`

**Thread-Local:**
- `RPC_NOTIFIER_BUF: RefCell<Vec<u8>>` (satır 268-270) — per-thread serialization buffer, allocation azaltır

**Notification Struct'ları:**
- `NotificationParams<T>`: `result: T`, `subscription: SubscriptionId`
- `Notification<T>`: `jsonrpc`, `method`, `params: NotificationParams<T>` — JSON-RPC 2.0 formatı

**Filter Fonksiyonları:**
- `get_transaction_logs(bank, params) -> Option<Vec<TransactionLogInfo>>` (satır 61-77):
  - `LogsSubscriptionKind::All` → vote'ları filtrele
  - `LogsSubscriptionKind::AllWithVotes` → filtre yok
  - `LogsSubscriptionKind::Single(pubkey)` → belirli adres
- `filter_block_result_txs(block, last_modified_slot, params)` (satır 324-367):
  - `BlockSubscriptionKind::MentionsAccountOrProgram(pk)` → tx'in account keys'inde pk aranır
  - Boş tx listesi + MentionsAccountOrProgram → `None` (bildirim yok)
  - `ConfirmedBlock::encode_with_options` → encoding, transaction_details, rewards
- `filter_account_result(result, params, last_notified_slot, bank)` (satır 369-391):
  - `last_modified_slot != last_notified_slot` → değişim var, bildir
  - Fork reversion: son bildirim fork'taydıysa, reversion da bildirilir
  - SPL token encoding → `get_parsed_token_account`
- `filter_signature_result(result, ...)` (satır 393-405): transaction result → ProcessedSignatureResult
- `filter_program_results(accounts, params, ...)` (satır 407-435): filtre + SPL token aware encoding
- `filter_logs_results(logs, ...)` (satır 437-449): RpcLogsResponse iterator
- `initial_last_notified_slot(params, ...)` (satır 451-475): ilk subscription için başlangıç slot'u
  - Finalized → `highest_super_majority_root`
  - Confirmed → `optimistically_confirmed_bank.bank.slot()`
  - Processed → `block_commitment_cache.slot()`
  - Account params: `bank.get_account_modified_slot(pubkey)` → son değişim slot'u

**Kritik Fonksiyonlar:**

`impl RpcNotifier`:
- `notify<T: Serialize>(value, subscription, is_final)` (satır 286-321):
  - `RPC_NOTIFIER_BUF` thread-local buf'a JSON serialize → `Arc<String>` oluştur
  - `RpcNotification { json: Arc::downgrade(&buf_arc) }` — Weak ref (broadcast'te kopyalanmaz)
  - `sender.send(notification)` — broadcast channel (lagged subscriber kaybeder)
  - `recent_items.lock().push(buf_arc)` — Arc kuvvetli ref tutar, Weak sonraki kopyalar için geçerli

`impl RecentItems`:
- `push(item)` (satır 229-261):
  - `total_bytes + item.len()` → overflow check (`checked_add`)
  - `while total_bytes > max_total_bytes || len > max_len` → pop_front (eviction)
  - metrics 2 sn'de bir gönderilir

`impl RpcSubscriptions`:
- `new(...)` (satır 528-548): `new_with_config` ile default PubSubConfig
- `new_for_tests(...)` (satır 550-607): geçici ledger + `default_for_tests` config + notifier hazır bekle (5sn timeout)
- `new_for_tests_with_blockstore(...)` (satır 573-607): `rpc_notifier_ready` AtomicBool ile sync
- `new_with_config(...)` (satır 609-677): ANA CONSTRUCTOR
  - unbounded crossbeam channel → notification_sender/receiver
  - `broadcast::channel(config.queue_capacity_items)` — tokio broadcast, kapasite 10M item
  - `RpcNotifier { sender: broadcast_sender, recent_items: RecentItems::new(capacity) }`
  - Thread `solRpcNotifier`: rayon thread pool (`notification_threads` adet)
  - `SubscriptionControl::new(max_active_subscriptions, notification_sender, broadcast_sender)`
  - `notification_sender: config.notification_threads.map(|_| notification_sender)` — threads yoksa None
- `control()` (satır 701-703): `&self.control` döner
- `notify_subscribers(commitment_slots)` (satır 705-709): `NotificationEntry::Bank` kuyruğa ekler
- `notify_gossip_subscribers(slot)` (satır 711-714): `NotificationEntry::Gossip` ekler
- `notify_slot_update(slot_update)` (satır 716-718): `NotificationEntry::SlotUpdate`
- `notify_slot(slot, parent, root)` (satır 720-728): `Slot` + `SlotUpdate::CreatedBank` çift bildirim
- `notify_signatures_received(slot_signatures)` (satır 730-732): received notification için
- `notify_vote(vote_pubkey, vote, signature)` (satır 734-736): vote bildirimi
- `notify_roots(rooted_slots)` (satır 738-747): sıralı root bildirimleri (SlotUpdate + Root)
- `enqueue_notification(notification_entry)` (satır 749-761): kanalı kullanır, hata → warn! (drop)
- `process_notifications(...)` (satır 763-921): ANA LOOP (rayon pool içinde çalışır)
  - `recv_timeout(100ms)` → TimestampedNotificationEntry
  - `NotificationEntry::Subscribed(params, id)` → `subscriptions.subscribe(params, id, ...)`
  - `NotificationEntry::Unsubscribed(params, id)` → `subscriptions.unsubscribe(params, id)`
  - `NotificationEntry::Slot(slot_info)` → node_progress watcher'a notify
  - `NotificationEntry::SlotUpdate(slot_update)` → SlotsUpdates watcher'a notify
  - `NotificationEntry::Vote(...)` → Vote watcher'a RpcVote oluştur, notify
  - `NotificationEntry::Root(root)` → Root watcher'a notify
  - `NotificationEntry::Bank(commitment_slots)` → `notify_watchers` (commitment_watchers, source="bank")
  - `NotificationEntry::Gossip(slot)` → CommitmentSlots oluştur, `notify_watchers` (gossip_watchers, source="gossip")
  - `NotificationEntry::SignaturesReceived(...)` → by_signature lookup, enable_received_notification kontrol
  - `stats.notification_entry_processing_time_us` → kuyruk gecikmesi ölçümü
- `notify_watchers(...)` (satır 923-1219): PARALEL BİLDİRİM (Rayon par_iter)
  - `commitment` → slot belirleme (finalized/confirmed/processed)
  - **Her subscription paralel işlenir**: `subscriptions.into_par_iter()`
  - Account: `check_commitment_and_notify` + `filter_account_result`
  - Block: slot window hesabı (last_notified'dan şimdiye kadar missed slot'lar dahil), `get_complete_block`, encode, notify
    - `max_complete_transaction_status_slot` ve `max_complete_rewards_slot` kontrolü (slot ready değilse skip)
  - Logs: `check_commitment_and_notify` + `filter_logs_results`
  - Program: `check_commitment_and_notify` + `filter_program_results`
  - Signature: `check_commitment_and_notify` + `filter_signature_result` → `is_final=true` (tek seferlik)
  - Metrics: `datapoint_info!("rpc_subscriptions", ...)` — 10ms'den uzun süren bildirimler loglanır
- `shutdown(&mut self)` (satır 1221-1232): exit flag + thread join

`fn check_commitment_and_notify<P, S, B, F, X, I>(...)` (satır 132-171): generic helper
  - `bank_forks.get(slot)` → Bank al
  - `bank_method(&bank, params)` → veri çek (örn. `get_account_modified_slot`)
  - `filter_results(results, params, last_notified_slot, bank)` → (iterator, result_slot)
  - Her result için `notifier.notify(RpcResponse, subscription, is_final)`
  - `last_notified_slot` güncellenir (slot idempotency sağlar)

**WebSocket Subscription Yönetimi:**
- Broadcast channel (`tokio::sync::broadcast`): tüm bağlantılar aynı kanalı dinler
- Lagged subscriber: kanalı takip edemeyenler `RecvError::Lagged` alır → bağlantı kesilir
- `Weak<String>` ile JSON: Arc büyük ref sayımı olmadan taşınır; kanaldan okuma sırasında Weak upgrade başarısız ise notification kaybolur (acceptable: receiver çok yavaşsa zaten kesilebilir)
- `RecentItems`: yeni subscriber'ın kaldığı yerden bilgilenmesi için son N mesajı tutar (ancak yeni subscriber için delivery garantisi yok)

**Optimistic vs Finalized Fark Uygulaması:**
- `gossip_watchers` → `CommitmentConfig::confirmed()` → `NotificationEntry::Gossip`
- `commitment_watchers` → `CommitmentConfig::finalized()` veya `processed()` → `NotificationEntry::Bank`
- Gossip bildirimi `notify_gossip_subscribers` ile, replay bildirimi `notify_subscribers` ile tetiklenir
- `initial_last_notified_slot`: subscription anında doğru slot'tan başlamak için

**Rate Limiting:**
- Rayon paralel işleme: N abonelik parallel span → bottleneck thread sayısı ile sınırlı
- `notify_watchers` 10ms'den uzun sürerse log (debug seviyesinde)
- `max_active_subscriptions = 1_000_000` → üstündekiler reddedilir

**Unsafe Bloklar:** Yok. Thread-local `RefCell<Vec<u8>>` güvenli (single-thread context).

**Testler (3130 satırın büyük çoğunluğu test):** 15+ test, tüm abonelik tipleri test edilmiş

**Nova Shield İçin Öğrenilen:**
- `Weak<String>` + `Arc<String>` kombinasyonu: broadcast channel'da kopya olmadan string geçirme
- Thread-local serialize buffer: per-thread allocation → N thread * 1 buffer, slab değil
- Rayon `par_iter` üzerinden paralel watcher bildirimi: abonelik sayısı arttıkça linear scaling
- Gossip vs Bank kaynak ayrımı: farklı commitment seviyeleri farklı event kaynaklarından beslenir
- Block subscription "catch-up" mekanizması: missed slot'lar sonraki notification'da teslim edilir
- `is_final=true` pattern: tek seferlik bildirimler (signature) → subscriber'ı otomatik sil
- Nova Shield: WebSocket sistemi için bu architecture doğrudan model alınmalı

---

### [rpc_subscription_tracker.rs] (789 satır) — YUKARIDAKİ ile AYNI DOSYA

*(rpc_subscription_tracker bölümü yukarıda tam analiz edildi)*

---

### [rpc_pubsub.rs] (1424 satır)

**Amaç:** WebSocket PubSub RPC trait tanımları ve implementasyonu — 9 subscribe/unsubscribe çifti + getVersion.

**Trait'ler:**
- `RpcSolPubSub` (satır 49-247): Eski jsonrpc_pubsub trait (geriye uyumluluk için korunmuş)
  - 9 subscription tipi × 2 (subscribe+unsubscribe) = 18 method
- `RpcSolPubSubInternal` (satır 257-357): Yeni iç trait (mevcut implementasyon)
  - Aynı 18 method + `getVersion` = 19 method

**PubSub Metodları (RpcSolPubSubInternal):**
1. `accountSubscribe(pubkey_str, config?) -> SubscriptionId`
2. `accountUnsubscribe(id) -> bool`
3. `programSubscribe(pubkey_str, config?) -> SubscriptionId`
4. `programUnsubscribe(id) -> bool`
5. `logsSubscribe(filter, config?) -> SubscriptionId`
6. `logsUnsubscribe(id) -> bool`
7. `signatureSubscribe(signature_str, config?) -> SubscriptionId`
8. `signatureUnsubscribe(id) -> bool`
9. `slotSubscribe() -> SubscriptionId`
10. `slotUnsubscribe(id) -> bool`
11. `slotsUpdatesSubscribe() -> SubscriptionId`
12. `slotsUpdatesUnsubscribe(id) -> bool`
13. `blockSubscribe(filter, config?) -> SubscriptionId`
14. `blockUnsubscribe(id) -> bool`
15. `voteSubscribe() -> SubscriptionId`
16. `voteUnsubscribe(id) -> bool`
17. `rootSubscribe() -> SubscriptionId`
18. `rootUnsubscribe(id) -> bool`
19. `getVersion() -> RpcVersionInfo`

**Struct/Enum/Trait Listesi:**
- `RpcSolPubSubImpl` (satır 359-363):
  - `config: PubSubConfig`
  - `subscription_control: SubscriptionControl`
  - `current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>` — bu bağlantıya ait tokenlar

**Kritik Fonksiyonlar:**

`impl RpcSolPubSubImpl`:
- `new(config, subscription_control, current_subscriptions) -> Self` (satır 366-376)
- `subscribe(&self, params: SubscriptionParams) -> Result<SubscriptionId>` (satır 378-391):
  - `subscription_control.subscribe(params)` → token al
  - Token'ı `current_subscriptions`'a ekle
  - `Ok(id)` döner
- `unsubscribe(&self, id: SubscriptionId) -> Result<bool>` (satır 393-403):
  - `current_subscriptions.remove(&id)` → yoksa error
  - Remove → token drop → RAII Drop → NotificationEntry::Unsubscribed

`impl RpcSolPubSubInternal for RpcSolPubSubImpl`:
- `account_subscribe(pubkey_str, config?)` (satır 423-441):
  - `param::<Pubkey>(&pubkey_str, "pubkey")?` — parse hata → `ErrorCode::InvalidParams`
  - `commitment.unwrap_or_default()`, `encoding.unwrap_or(Binary)`
  - `min_context_slot` → ignored (comment satır 432)
- `program_subscribe(pubkey_str, config?)` (satır 447-464): pubkey parse + filters, encoding, commitment
- `logs_subscribe(filter, config?)` (satır 471-493):
  - `RpcTransactionLogsFilter::Mentions(keys)` → `keys.len() != 1` → error (satır 481-486)
  - Tek adres kısıtı: aynı anda birden fazla adres filtrelemesi desteklenmiyor
- `signature_subscribe(signature_str, config?)` (satır 500-511): Signature parse
- `slot_subscribe()`, `root_subscribe()`, `vote_subscribe()`, `slots_updates_subscribe()`: parametresiz
- `block_subscribe(filter, config?)` (satır 534-561):
  - `!self.config.enable_block_subscription` → `MethodNotFound` error
  - `check_is_at_least_confirmed(commitment)?` → processed/finalized red
  - `BlockSubscriptionKind::MentionsAccountOrProgram(pk)` parse
- `vote_subscribe()` (satır 571-576): `!self.config.enable_vote_subscription` → `MethodNotFound`
- `get_version()` (satır 593-599): solana_version::Version → feature_set dahil

**`fn param<T: FromStr>(param_str, thing) -> Result<T>`** (satır 414-420):
- Parse hatası → `ErrorCode::InvalidParams` + açıklayıcı mesaj

**Rate Limiting / DoS Koruması:**
- `max_active_subscriptions` → `subscribe` içinde kontrol (SubscriptionControl'de)
- `block_subscribe` ve `vote_subscribe` → `enable_X_subscription` flag ile kapı

**Unsafe Bloklar:** Yok.

**Testler (820-1424 aralığı):** 8 test
- `test_signature_subscribe`: received notification dahil 3 senaryo
- `test_signature_unsubscribe`: invalid id → error
- `test_account_subscribe`: stake account oluşturma, encoding
- `test_account_subscribe_with_encoding`: nonce account, JsonParsed encoding
- `test_account_unsubscribe`: geçersiz id → error
- `test_account_commitment_not_fulfilled`: commitment beklenmiyor → panic beklentisi (should_panic)
- `test_account_commitment`: finalized commitment testi
- `test_slot_subscribe` / `test_slot_unsubscribe`
- `test_vote_subscribe` / `test_vote_unsubscribe`
- `test_get_version`

**Nova Shield İçin Öğrenilen:**
- İki trait (eski/yeni) tutulması: backward compatibility için gerekli, ancak boilerplate
- `current_subscriptions: Arc<DashMap>`: bağlantı başına token map'i → bağlantı kopunca tüm tokenlar drop
- `min_context_slot` ignored: PubSub'da context slot anlamsız (sürekli akış)
- `logs_subscribe` tek adres kısıtı: multi-address log filter pahalı, bilinçli sınırlama
- Nova Shield: block/vote subscription opsiyonel flag ile kontrol edilmeli (DoS riski yüksek)

---

### [rpc_pubsub_service.rs] (515 satır)

**Amaç:** WebSocket server lifecycle — TCP listener, per-connection handler, WebSocket handshake, message loop.

**Const/Static:**
- `MAX_ACTIVE_SUBSCRIPTIONS: usize = 1_000_000` (satır 34)
- `DEFAULT_QUEUE_CAPACITY_ITEMS: usize = 10_000_000` (satır 35)
- `DEFAULT_TEST_QUEUE_CAPACITY_ITEMS: usize = 100` (satır 36)
- `DEFAULT_QUEUE_CAPACITY_BYTES: usize = 256 * 1024 * 1024` = 256MB (satır 37)
- `DEFAULT_WORKER_THREADS: usize = 1` (satır 38)
- `METRICS_REPORT_INTERVAL_MS: u64 = 10_000` (satır 127)

**Struct/Enum/Trait Listesi:**
- `PubSubConfig` (satır 41-49):
  - `enable_block_subscription: bool`
  - `enable_vote_subscription: bool`
  - `max_active_subscriptions: usize`
  - `queue_capacity_items: usize`
  - `queue_capacity_bytes: usize`
  - `worker_threads: usize`
  - `notification_threads: Option<NonZeroUsize>`
- `PubSubService` (satır 79-81): `thread_hdl: JoinHandle<()>`
- `SentNotificationStats` (satır 129-142):
  - `num_account`, `num_logs`, `num_program`, `num_signature`, `num_slot`, `num_slots_updates`, `num_root`, `num_vote`, `num_block`: `AtomicUsize`
  - `total_creation_to_queue_time_us: AtomicU64` — latency ölçümü
  - `last_report: AtomicInterval`
- `BroadcastHandler` (satır 205-208):
  - `current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>`
  - `sent_stats: Arc<SentNotificationStats>`
- `Error` (satır 353-362): enum
  - `Handshake(soketto::handshake::Error)`
  - `Connection(soketto::connection::Error)`
  - `Broadcast(broadcast::error::RecvError)`
  - `NotificationIsGone` — Weak upgrade başarısız

**Kritik Fonksiyonlar:**

`impl PubSubConfig`:
- `default()`: production değerleri
- `default_for_tests()`: küçük queue_capacity_items (100), 2 notification_threads

`impl PubSubService`:
- `new(pubsub_config, subscriptions, pubsub_addr) -> (Trigger, Self)` (satır 84-116):
  - `Tripwire::new()` → graceful shutdown mekanizması (stream_cancel crate)
  - Thread `solRpcPubSub`: tokio runtime (`solRpcPubSubRt`, worker_threads)
  - `runtime.block_on(listen(..., tripwire))` → async listener başlatır
  - Trigger döner: dışarıdan drop edildiğinde server durur

`impl BroadcastHandler`:
- `new(current_subscriptions) -> Self` (satır 253-258): stats Arc init
- `handle(&self, notification: RpcNotification) -> Result<Option<Arc<String>>, Error>` (satır 261-283):
  - `current_subscriptions.entry(notification.subscription_id)`
  - Yoksa `Ok(None)` (bu bağlantının aboneliği değil)
  - `notification.is_final` → entry.remove() (tek seferlik)
  - `notification.json.upgrade()` → `NotificationIsGone` riski
  - Stats güncelleme

`fn increment_sent_notification_stats(...)` (satır 210-249): match SubscriptionParams → ilgili counter

`async fn handle_connection(socket, subscription_control, config, tripwire)` (satır 365-434):
- `soketto` WebSocket handshake: Server accept
- `broadcast::Receiver` al
- `DashMap::<SubscriptionId, SubscriptionToken>::new()` — bağlantıya özgü token map
- `IoHandler` → `RpcSolPubSubImpl::new(config, subscription_control, current_subscriptions)`
- `BroadcastHandler::new(current_subscriptions)`
- **Ana loop** (iki iç loop):
  - `receive_future = receiver.receive_data(&mut data)` — soketto cancel unsafe!
    - select!: receive_future tamamlanana kadar broadcast mesajları işle
    - `broadcast_receiver.recv()` → `broadcast_handler.handle(result?)` → `sender.send_text(&json)`
    - tripwire → "shutting down" warn + return
    - connection closed → `Ok(())`
  - UTF-8 kontrolü: `str::from_utf8(&data)` başarısız → break (bağlantıyı kes)
  - `json_rpc_handler.handle_request(data_str).await` → JSON-RPC response gönder
  - `data.clear()`

`async fn listen(listen_address, config, subscription_control, tripwire)` (satır 436-469):
- `TcpListener::bind(listen_address)`
- `TokenCounter::new("rpc_pubsub_connections")` — bağlantı sayısı metrics
- select! loop:
  - `listener.accept()` → `tokio::spawn(handle_connection(...))`; `counter_token` task'a taşınır (drop → dec)
  - tripwire → return

**Test:**
- `test_pubsub_new` (satır 492-514): Thread adı kontrolü (`solRpcPubSub`)
- `test_connection(subscriptions) -> (RpcSolPubSubImpl, TestBroadcastReceiver)`: test helper
- `TestBroadcastReceiver` (satır 287-327): `recv()` ve `recv_timeout()` — 50ms poll loop

**WebSocket Memory Leak Analizi:**
- `counter_token`: `TokenCounter::new` → `create_token()` → task kapanınca drop → bağlantı sayısı düşer
- `current_subscriptions`: bağlantıya özgü DashMap → bağlantı task'ı kapanınca drop → tüm tokenlar drop → RAII unsubscribe
- `broadcast::Receiver`: tokio broadcast, receiver kapanınca kanaldan ayrılır

**Soketto Cancel Safety Notları:**
- `soketto` `receive_data` cancel safe değil: bu yüzden pin! ve inner loop gerekli
- Inner loop: receive_future tamamlanana kadar broadcast mesajlarını da işle

**Nova Shield İçin Öğrenilen:**
- `Tripwire` / `Trigger` pattern: graceful WebSocket shutdown
- Soketto cancel-safe workaround: pin! + inner select loop
- `TokenCounter` ile bağlantı sayısı ölçümü
- 256MB queue kapasitesi: yüksek throughput için gerekli
- `MAX_ACTIVE_SUBSCRIPTIONS = 1_000_000`: kapı sayısı
- `counter_token` bağlantı sayacı: task drop ile otomatik azaltma
- Nova Shield WebSocket: tokio + soketto (veya tungstenite) + broadcast channel + Tripwire

---

### [rpc.rs] (9279 satır) — ANA DOSYA

**Amaç:** Tüm HTTP JSON-RPC metodlarının implementasyonu; `JsonRpcRequestProcessor` üzerinden bank, blockstore, bigtable, gossip erişimi.

**Const/Static:**
- `MAX_REQUEST_BODY_SIZE: usize = 50 * (1 << 10)` = 50KB (satır 120)
- `PERFORMANCE_SAMPLES_LIMIT: usize = 720` (satır 121)

**Struct/Enum/Trait Listesi:**
- `JsonRpcConfig` (satır 141-157):
  - `enable_rpc_transaction_history: bool`
  - `enable_extended_tx_metadata_storage: bool`
  - `faucet_addr: Option<SocketAddr>`
  - `health_check_slot_distance: u64`
  - `rpc_bigtable_config: Option<RpcBigtableConfig>`
  - `max_multiple_accounts: Option<usize>`
  - `account_indexes: AccountSecondaryIndexes`
  - `rpc_threads: usize`
  - `rpc_niceness_adj: i8`
  - `full_api: bool`
  - `obsolete_v1_7_api: bool`
  - `rpc_scan_and_fix_roots: bool`
  - `max_request_body_size: Option<usize>`
  - `disable_health_check: bool`
- `RpcBigtableConfig` (satır 169-176): BigTable bağlantı konfigürasyonu
- `JsonRpcRequestProcessor` (satır 192-213): CORE REQUEST HANDLER
  - `bank_forks: Arc<RwLock<BankForks>>`
  - `block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>`
  - `blockstore: Arc<Blockstore>`
  - `config: JsonRpcConfig`
  - `snapshot_config: Option<SnapshotConfig>`
  - `validator_exit: Arc<RwLock<Exit>>`
  - `health: Arc<RpcHealth>`
  - `cluster_info: Arc<ClusterInfo>`
  - `genesis_hash: Hash`
  - `transaction_sender: Arc<Mutex<Sender<TransactionInfo>>>`
  - `bigtable_ledger_storage: Option<solana_storage_bigtable::LedgerStorage>`
  - `optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>`
  - `largest_accounts_cache: Arc<RwLock<LargestAccountsCache>>`
  - `max_slots: Arc<MaxSlots>`
  - `leader_schedule_cache: Arc<LeaderScheduleCache>`
  - `max_complete_transaction_status_slot: Arc<AtomicU64>`
  - `max_complete_rewards_slot: Arc<AtomicU64>`
  - `prioritization_fee_cache: Arc<PrioritizationFeeCache>`
- `RpcCustomResult<T>`: `std::result::Result<T, RpcCustomError>` alias

**Tüm RPC Metodları (68 metod, 8 grup):**

**rpc_minimal (12 metod):**
1. `getBalance` — account lamport balance
2. `getEpochInfo` — epoch, slot, block height, tx count
3. `getGenesisHash` — genesis hash
4. `getHealth` — node health (ok/behind/unknown)
5. `getIdentity` — validator pubkey
6. `getSlot` — mevcut slot
7. `getBlockHeight` — mevcut block height
8. `getHighestSnapshotSlot` — full + incremental snapshot slot
9. `getTransactionCount` — toplam tx sayısı
10. `getVersion` — solana version + feature_set
11. `getVoteAccounts` — validator vote hesapları
12. `getLeaderSchedule` — epoch leader schedule

**rpc_bank (7 metod):**
13. `getMinimumBalanceForRentExemption` — veri boyutuna göre rent
14. `getInflationGovernor` — enflasyon parametreleri
15. `getInflationRate` — mevcut epoch enflasyon oranı
16. `getEpochSchedule` — epoch/slot konfigürasyonu
17. `getSlotLeader` — mevcut slot lideri
18. `getSlotLeaders` — birden fazla slot lideri
19. `getBlockProduction` — üretilen/beklenen blok istatistikleri

**rpc_accounts (5 metod):**
20. `getAccountInfo` — tek hesap bilgisi
21. `getMultipleAccounts` — çoklu hesap
22. `getBlockCommitment` — blok taahhüt bilgisi
23. `getTokenAccountBalance` — SPL token hesap bakiyesi
24. `getTokenSupply` — SPL token toplam arzı

**rpc_accounts_scan (6 metod):**
25. `getProgramAccounts` — program'a ait tüm hesaplar (scan)
26. `getLargestAccounts` — en büyük N hesap (cache'li)
27. `getSupply` — SOL arzı (circulating + non-circulating)
28. `getTokenLargestAccounts` — bir mint'in en büyük token hesapları
29. `getTokenAccountsByOwner` — owner'a göre token hesapları
30. `getTokenAccountsByDelegate` — delegate'e göre token hesapları

**rpc_full (20 metod):**
31. `getInflationReward` — epoch staking/voting ödülleri
32. `getClusterNodes` — gossip'teki tüm node bilgileri
33. `getRecentPerformanceSamples` — son perf örnekleri (max 720)
34. `getSignatureStatuses` — çoklu tx signature durumu
35. `getMaxRetransmitSlot` — max retransmit slot
36. `getMaxShredInsertSlot` — max shred insert slot
37. `requestAirdrop` — faucet'ten airdrop
38. `sendTransaction` — tx gönder (base64/base58 encoded)
39. `simulateTransaction` — tx simülasyonu
40. `minimumLedgerSlot` — en eski kullanılabilir ledger slot
41. `getBlock` — tam blok bilgisi
42. `getBlockTime` — slot'un unix timestamp'i
43. `getBlocks` — slot aralığındaki confirmed bloklar
44. `getBlocksWithLimit` — limit'li blok listesi
45. `getTransaction` — tx detayları
46. `getSignaturesForAddress` — adres için tx imzaları
47. `getFirstAvailableBlock` — ilk erişilebilir blok
48. `getLatestBlockhash` — son blockhash + last_valid_block_height
49. `isBlockhashValid` — blockhash geçerli mi
50. `getFeeForMessage` — mesaj için hesaplanan ücret
51. `getStakeMinimumDelegation` — minimum stake miktarı
52. `getRecentPrioritizationFees` — son öncelik ücretleri

**rpc_deprecated_v1_18 (1 metod):**
53. `getStakeActivation` — stake activation durumu (DEPRECATED)

**rpc_deprecated_v1_9 (5 metod):**
54. `getRecentBlockhash` (DEPRECATED)
55. `getFees` (DEPRECATED)
56. `getFeeCalculatorForBlockhash` (DEPRECATED)
57. `getFeeRateGovernor` (DEPRECATED)
58. `getSnapshotSlot` (DEPRECATED)

**rpc_deprecated_v1_7 (5 metod):**
59. `getConfirmedBlock` (DEPRECATED)
60. `getConfirmedBlocks` (DEPRECATED)
61. `getConfirmedBlocksWithLimit` (DEPRECATED)
62. `getConfirmedTransaction` (DEPRECATED)
63. `getConfirmedSignaturesForAddress2` (DEPRECATED)

**rpc_obsolete_v1_7 (5 metod — opsiyonel):**
64. `confirmTransaction` (OBSOLETE)
65. `getSignatureStatus` (OBSOLETE)
66. `getSignatureConfirmation` (OBSOLETE)
67. `getTotalSupply` (OBSOLETE)
68. `getConfirmedSignaturesForAddress` (OBSOLETE)

**TOPLAM: 68 RPC metod** (37 aktif + 5 deprecated_v1_18/1_9 + 5 deprecated_v1_7 + 5 obsolete_v1_7 + 16 PubSub = ~84 toplam endpoint)

**Kritik Fonksiyonlar (JsonRpcRequestProcessor):**

- `bank(commitment: Option<CommitmentConfig>) -> Arc<Bank>` (satır 244-304):
  - `is_confirmed()` → `optimistically_confirmed_bank` (gossip-hızlı)
  - Diğerleri: `block_commitment_cache.slot_with_commitment(level)`
  - Bank bulunamazsa: warn + root_bank (hiç hata vermeden fallback)
- `get_bank_with_config(config: RpcContextConfig) -> Result<Arc<Bank>>` (satır 226-241):
  - `min_context_slot`: bank slot < min → `MinContextSlotNotReached` error
- `get_account_info(pubkey, config?)` (satır 428-447): `get_encoded_account` → `new_response`
- `get_multiple_accounts(pubkeys, config?)` (satır 449-471): iter map → `Result<Vec<_>>`
- `get_program_accounts(program_id, config, filters, with_context)` (satır 482-529):
  - SPL token owner filter optimize edilmiş index → `get_filtered_spl_token_accounts_by_owner`
  - SPL token mint filter → `get_filtered_spl_token_accounts_by_mint`
  - Diğerleri → `get_filtered_program_accounts`
  - `OptionalContext::Context` (with_context) veya `NoContext`
- `get_inflation_reward(addresses, config?)` (satır 531-622): async, epoch reward slot bulma
- `get_balance(pubkey, config)` (satır 652-659): `bank.get_balance(pubkey)`
- `get_slot_leaders(commitment, start_slot, limit)` (satır 776-...): epoch'a göre leader schedule döngüsü
- `confirm_transaction(signature, commitment)` (satır 730-741): `bank.get_signature_status`

**Input Validation Mekanizmaları:**
- `verify_pubkey(input)` (satır 2261-2265): `input.parse::<Pubkey>()` → `Error::invalid_params`
- `verify_hash(input)` (satır 2267-2271): Hash parse
- `verify_signature(input)` (satır 2273-2277): Signature parse
- `verify_transaction(transaction, feature_set)` (satır 2239-2253):
  - `transaction.verify()` → imza doğrulama
  - `transaction.verify_precompiles(feature_set)` → secp256k1, ed25519 precompile kontrolü
- `verify_filter(input)` (satır 2255-2259): `filter.verify()` → filtre boyutu ve format kontrolü
- `verify_token_account_filter(filter)` (satır 2279-2292): Mint/ProgramId pubkey doğrulama
- `verify_and_parse_signatures_for_address_params(address, before, until, limit)` (satır 2294-2313):
  - Limit: 0 veya > MAX → error
  - Signature parse için `verify_signature`
- `check_is_at_least_confirmed(commitment)` (satır 2315-2322): processed/finalized → error

**Limit Sabitler (rpc_client_api::request):**
- `MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS2_LIMIT`: imza sorgu limiti
- `MAX_GET_CONFIRMED_BLOCKS_RANGE`: blok aralığı max
- `MAX_GET_PROGRAM_ACCOUNT_FILTERS`: program hesap filtresi max
- `MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS`: signature status batch max
- `MAX_GET_SLOT_LEADERS`: slot leader max
- `MAX_MULTIPLE_ACCOUNTS`: getMultipleAccounts max
- `NUM_LARGEST_ACCOUNTS`: getLargestAccounts eleman sayısı

**SPL Token Optimizasyonları:**
- `get_spl_token_owner_filter(program_id, filters)` (satır 2373-2425): filter'lardan owner pubkey çıkarır → index kullan
- `get_spl_token_mint_filter(program_id, filters)` (satır 2431-2483): filter'lardan mint pubkey çıkarır → index kullan
- Memcmp offset'leri: `SPL_TOKEN_ACCOUNT_OWNER_OFFSET`, `SPL_TOKEN_ACCOUNT_MINT_OFFSET` hardcoded

**sendTransaction Akışı (`_send_transaction`)** (satır 2513-2536):
- `TransactionInfo::new(signature, wire_transaction, last_valid_block_height, durable_nonce_info, max_retries, None)`
- `transaction_sender.lock().unwrap().send(transaction_info)` → unbounded channel → `SendTransactionService`

**encode_account** (satır 2347-2367):
- Binary/Base58 encoding → `MAX_BASE58_BYTES` check → `InvalidRequest` (DoS önleme)

**Unsafe Bloklar:** `rpc.rs`'de doğrudan unsafe blok yok.

**Büyük Metodlar (simülasyon ve blok çekme):**
- `simulate_transaction` implementasyonu: tx sanitize, verify, simulate, inner ix extract, return data
- `get_block`: async, blockstore + bigtable fallback, encoding, commitment check, reward filter
- `get_transaction`: async, blockstore + bigtable fallback
- `get_signatures_for_address`: paginated, before/until/limit parametreli

**Nova Shield İçin Öğrenilen:**
- 68 metod / 8 grup: API katmanının net ayrışımı → Minimal (validator) / Full (API node) / Scan (pahalı) / Deprecated / Obsolete
- `bank()` commitment routing: her API çağrısı doğru commitment seviyesine yönlendirilmeli
- SPL token filter optimization: memcmp + data_size analizi ile scan → index geçişi → N kat hızlanma
- `verify_*` helper'ları: input validation tek yerden, tutarlı hata mesajları
- `_send_transaction`: channel-based, `Mutex<Sender>` → tek giriş noktası, thread-safe
- `max_request_body_size = 50KB` ve çeşitli limit sabitleri: DoS korumasının temel hattı
- `min_context_slot`: istemci belirli bir slot'tan önce cevap almak istemez → staleness koruması
- Nova Shield: aynı minimal/full/scan üçlü yapısı, her node tipi için farklı API seti

---

## ÖZET: SOLANA RPC SİSTEMİ — GENEL MİMARİ

### Katmanlar

```
İstemci (HTTP/WebSocket)
         │
         ▼
┌─────────────────────────────────────────┐
│ HTTP: RpcRequestMiddleware (hyper)      │
│   - /health → RpcHealth                │
│   - /snapshot.tar.bz2 → redirect       │
│   - /genesis.tar.bz2 → O_NOFOLLOW file │
│   - /v0/circulating-supply → REST       │
│   - JSON → MetaIoHandler               │
├─────────────────────────────────────────┤
│ MetaIoHandler                           │
│   - rpc_minimal (her zaman)            │
│   - rpc_bank, rpc_accounts (full_api)  │
│   - rpc_accounts_scan (full_api)       │
│   - rpc_full (full_api)                │
│   - deprecated/obsolete (config'e göre)│
├─────────────────────────────────────────┤
│ JsonRpcRequestProcessor                 │
│   - bank(), get_bank_with_config()      │
│   - blockstore, bigtable               │
│   - prioritization_fee_cache           │
│   - transaction_sender channel          │
├─────────────────────────────────────────┤
│ BankForks / OptimisticallyConfirmedBank │
│ BlockCommitmentCache                    │
└─────────────────────────────────────────┘

WebSocket: PubSubService (soketto)
         │
         ▼
┌─────────────────────────────────────────┐
│ handle_connection (per-connection task) │
│   - RpcSolPubSubImpl                   │
│   - BroadcastHandler                   │
│   - current_subscriptions (DashMap)    │
├─────────────────────────────────────────┤
│ RpcSubscriptions                        │
│   - notification_sender (crossbeam)    │
│   - t_cleanup thread (solRpcNotifier)  │
│   - process_notifications (rayon pool) │
│   - notify_watchers (par_iter)         │
├─────────────────────────────────────────┤
│ SubscriptionsTracker                    │
│   - commitment_watchers                │
│   - gossip_watchers                    │
│   - by_signature                       │
│   - node_progress_watchers             │
│   - logs_subscriptions_index           │
├─────────────────────────────────────────┤
│ SubscriptionControl                     │
│   - DashMap<SubscriptionParams, Weak>  │
│   - broadcast::Sender                  │
│   - TokenCounter                       │
└─────────────────────────────────────────┘
```

### Commitment Level Routing

| Commitment    | Kaynak                              | Güncelleme tetikleyicisi         |
|---------------|-------------------------------------|----------------------------------|
| processed     | BlockCommitmentCache.slot()         | Bank::notify_subscribers         |
| confirmed     | OptimisticallyConfirmedBank.bank    | gossip/replay optimistic confirm |
| finalized     | BlockCommitmentCache.highest_super_majority_root | rooted bank      |

### Rate Limiting / DoS Koruması Özeti

| Mekanizma                        | Değer                        | Nerede                 |
|----------------------------------|------------------------------|------------------------|
| max_request_body_size            | 50 KB                        | rpc_service.rs         |
| max_active_subscriptions         | 1,000,000                    | rpc_pubsub_service.rs  |
| queue_capacity_items             | 10,000,000                   | rpc_pubsub_service.rs  |
| queue_capacity_bytes             | 256 MB                       | rpc_pubsub_service.rs  |
| getLargestAccounts TTL           | 2 saat                       | rpc_service.rs         |
| MAX_MULTIPLE_ACCOUNTS            | rpc_client_api limit         | rpc.rs                 |
| MAX_GET_PROGRAM_ACCOUNT_FILTERS  | rpc_client_api limit         | rpc.rs                 |
| O_NOFOLLOW file serving          | symlink block                | rpc_service.rs         |
| binary encoding size limit       | MAX_BASE58_BYTES             | rpc.rs                 |
| block/vote subscription flag     | enable_* config              | rpc_pubsub.rs          |
| min_context_slot                 | per-request staleness        | rpc.rs                 |
| CORS max_age                     | 86400 sn (1 gün)             | rpc_service.rs         |

### Unsafe Bloklar Özeti
Hiçbir dosyada `unsafe` blok bulunmadı. `O_NOFOLLOW` güvenli FFI sarmalama ile uygulandı.

---

## NOVA SHIELD İÇİN KRİTİK ÖĞRENIMLER

1. **RPC Mimari Ayrımı**: Minimal (validator) / Full (API node) / Scan (pahalı) gruplaması kesin olmalı. Nova Shield'da da bu üçlü ayrımı uygula.

2. **Commitment Routing**: Her API çağrısı `bank(commitment)` fonksiyonundan geçmeli. `processed → working bank`, `confirmed → optimistic bank`, `finalized → root bank`. Bu routing'i tek yerden yönet.

3. **WebSocket Architecture**:
   - Tek broadcast channel, tüm bağlantılar dinler
   - `Weak<String>` ile copy-free JSON broadcasting
   - Per-connection `DashMap<SubscriptionId, SubscriptionToken>` ile RAII unsubscribe
   - Rayon pool ile paralel watcher bildirimi
   - `Tripwire` ile graceful shutdown

4. **RAII Subscription Lifecycle**: `SubscriptionToken` drop = unsubscribe. Nova Shield'da da bu pattern, memory leak riskini sıfırlar.

5. **SPL Token Index Optimizasyonu**: `getProgramAccounts` için memcmp filtrelerden owner/mint pubkey çıkar → full scan yerine index kullan. Nova Shield token sorguları için kritik.

6. **TransactionNotifier Interface**: `Arc<dyn TransactionNotifier + Sync + Send>` — plugin sistemi bu trait üzerinden. Geyser benzeri bir sisteme geçiş kolaylaştırır.

7. **File Serving Güvenliği**: Snapshot ve genesis dosyaları için `O_NOFOLLOW` zorunlu. Symlink + path traversal attack gerçek tehdit.

8. **Deferred Optimistic Notification**: Gossip önce gelirse bank henüz freeze edilmemiş olabilir → `pending_optimistically_confirmed_banks`. Freeze event'inde işle.

9. **50KB Request Body Limiti**: DoS korumasının temel hattı. Nova Shield'da mutlaka implementasyon başlangıcında.

10. **Block Subscription Catch-Up**: `last_notified_slot..slot` aralığındaki missed slot'ları da gönder. Tek bildirim miss'i client'ı güncel tutar.
