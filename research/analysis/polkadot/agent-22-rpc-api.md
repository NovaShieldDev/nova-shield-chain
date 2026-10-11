# Agent-22: substrate/client/rpc + substrate/client/api Tam Analizi

Analiz tarihi: 2026-07-22
Kaynak dizinler:
- `substrate/client/rpc/` (21 dosya)
- `substrate/client/api/` (11 dosya)
- `substrate/client/rpc-api/` (23 dosya)
- `substrate/client/rpc-spec-v2/` (50+ dosya)

---

## 1. GENEL MİMARİ

### Katman Yapısı

```
JSON-RPC Client (external)
        |
   [jsonrpsee server]
        |
   rpc-api/ (trait tanımları, #[rpc] macro)
        |
   rpc/ (eski V1 implementasyonlar)
   rpc-spec-v2/ (yeni V2 implementasyonlar)
        |
   client/api/ (Backend + Client trait'leri)
        |
   [Substrate Backend (sc-client-db)]
```

### RPC Versiyonları

| Versiyon | Crate | Durum |
|----------|-------|-------|
| V1 (legacy) | `sc-rpc`, `sc-rpc-api` | Hala destekleniyor; bazı metodlar deprecated |
| V2 (spec) | `sc-rpc-spec-v2` | Aktif geliştirme; `unstable_` prefix |

### RPC Modülleri

| Modül | V1 Metod Prefix | İşlev |
|-------|----------------|-------|
| author | `author_` | Tx submit, session keys, pending extrinsics |
| chain | `chain_` | Block/header sorgulama, new heads subscription |
| state | `state_` | Storage query, runtime call, metadata |
| system | `system_` | Network health, peers, sync state |
| offchain | `offchain_` | Offchain storage read/write |
| dev | `dev_` | block_stats (UNSAFE ONLY) |
| mixnet | `mixnet_` | Mixnet message submit |
| statement | `statement_` | Statement store |

| V2 Modül | Prefix | İşlev |
|---------|--------|-------|
| chainHead | `chainHead_unstable_` | Follow subscription + operations |
| transaction | `transaction_unstable_` | Submit+watch, broadcast |
| archive | `archive_unstable_` | Historical block/storage queries |
| chainSpec | `chainSpec_` | Chain spec metadata |
| bitswap | `protocol_bitswap_` | Bitswap protocol queries |

---

## 2. substrate/client/api — BACKEND VE CLIENT TRAIT'LERİ

### 2.1 Backend Trait (`backend.rs`)

**Temel trait:** `Backend<Block: BlockT>: AuxStore + Send + Sync`

```rust
pub trait Backend<Block: BlockT>: AuxStore + Send + Sync {
    type BlockImportOperation: BlockImportOperation<Block, State = Self::State>;
    type Blockchain: BlockchainBackend<Block>;
    type State: StateBackend<HashingFor<Block>> + Send + AsTrieBackend<...>;
    type OffchainStorage: OffchainStorage;

    fn begin_operation(&self) -> Result<Self::BlockImportOperation>;
    fn begin_state_operation(&self, operation: &mut Self::BlockImportOperation, block: Block::Hash) -> Result<()>;
    fn commit_operation(&self, transaction: Self::BlockImportOperation) -> Result<()>;
    fn finalize_block(&self, hash: Block::Hash, justification: Option<Justification>) -> Result<()>;
    fn state_at(&self, hash: Block::Hash, trie_cache_context: TrieCacheContext) -> Result<Self::State>;
    fn pin_block(&self, hash: Block::Hash) -> Result<()>;
    fn unpin_block(&self, hash: Block::Hash);
    fn revert(&self, n: NumberFor<Block>, revert_finalized: bool) -> Result<(NumberFor<Block>, HashSet<Block::Hash>)>;
    fn get_import_lock(&self) -> &RwLock<()>;
    fn requires_full_sync(&self) -> bool;
    // ...
}
```

**Kritik tasarım kararı:** Import lock (`RwLock<()>`) backend tarafından acquire EDILMEZ; bunu kullanan bileşenler (client) manage eder. (backend.rs:697-703)

**TrieCacheContext:** (backend.rs:536-558)
- `Trusted`: Sınırsız local cache (block import için); trie verisi shared cache'e propagate edilir
- `Untrusted`: Önceden yapılandırılmış boyutla sınırlı (RPC call'ları için)
- `From<CallContext>`: `Onchain` → Trusted, `Offchain` → Untrusted

### 2.2 BlockImportOperation Trait (`backend.rs`)

```rust
pub trait BlockImportOperation<Block: BlockT> {
    type State: StateBackend<HashingFor<Block>>;
    fn set_block_data(header, body, indexed_body, justifications, state: NewBlockState, register_as_leaf: bool) -> Result<()>;
    fn update_db_storage(update: BackendTransaction<HashingFor<Block>>) -> Result<()>;
    fn mark_finalized(hash: Block::Hash, justification: Option<Justification>) -> Result<()>;
    fn mark_head(hash: Block::Hash) -> Result<()>;
    fn update_transaction_index(index: Vec<IndexOperation>) -> Result<()>;
    fn set_renew_payloads(payloads: HashMap<H256, Vec<u8>>) -> Result<()>;
    fn set_create_gap(&mut self, create_gap: bool);
}
```

**NewBlockState:** (backend.rs:137-163)
- `Normal`: Sıradan import
- `Best`: Yeni best block
- `Final`: Yeni finalized block (implicit best)

**Warp sync notu:** `register_as_leaf: false` → historical block; warp sync sırasında leaf set'e eklenmez.

### 2.3 Client Trait'leri (`client.rs`)

**BlockchainEvents:**
```rust
pub trait BlockchainEvents<Block: BlockT> {
    fn import_notification_stream(&self) -> ImportNotifications<Block>;
    fn every_import_notification_stream(&self) -> ImportNotifications<Block>;
    fn finality_notification_stream(&self) -> FinalityNotifications<Block>;
    fn storage_changes_notification_stream(
        &self,
        filter_keys: Option<&[StorageKey]>,
        child_filter_keys: Option<&[(StorageKey, Option<Vec<StorageKey>>)]>,
    ) -> Result<StorageEventStream<Block::Hash>>;
}
```

**Önemli not:** `import_notification_stream()` initial sync sırasında her block için ateşlemez — sadece reorg varsa. `every_import_notification_stream()` her block için garantili. (client.rs:74-83)

**BlockImportNotification:** (client.rs:359-402)
```rust
pub struct BlockImportNotification<Block: BlockT> {
    pub hash: Block::Hash,
    pub origin: BlockOrigin,
    pub header: Block::Header,
    pub is_new_best: bool,
    pub tree_route: Option<Arc<sp_blockchain::TreeRoute<Block>>>,
    unpin_handle: UnpinHandle<Block>,   // private — drop = unpin
}
```

**FinalityNotification:** (client.rs:406-461)
```rust
pub struct FinalityNotification<Block: BlockT> {
    pub hash: Block::Hash,           // son finalized block
    pub header: Block::Header,
    pub tree_route: Arc<[Block::Hash]>, // (old_finalized, new_finalized) range implicitly finalized blocks
    pub stale_blocks: Arc<[Arc<StaleBlock<Block>>]>,
    unpin_handle: UnpinHandle<Block>,   // private
}
```

`from_summary()`: `summary.finalized.pop()` → son elemanı hash olarak alır; kalan elemanlar tree_route.

### 2.4 UnpinHandle — Block Pinning Mekanizması

```rust
pub struct UnpinHandleInner<Block: BlockT> {
    hash: Block::Hash,
    unpin_worker_sender: TracingUnboundedSender<UnpinWorkerMessage<Block>>,
}

impl<Block: BlockT> Drop for UnpinHandleInner<Block> {
    fn drop(&mut self) {
        self.unpin_worker_sender.unbounded_send(UnpinWorkerMessage::Unpin(self.hash))
    }
}
```

**Tasarım:** RAII pattern ile block pinning. Notification drop edildiğinde `Unpin` mesajı gönderilir. `into_unpin_handle()` ile notification'dan handle alınabilir ve block uzun süre pinned tutulabilir. (client.rs:292-355)

**UnpinWorkerMessage:** (client.rs:329-334)
- `AnnouncePin(Hash)`: import/finality notification yaratıldığında
- `Unpin(Hash)`: notification drop edildiğinde

### 2.5 StorageNotifications (`notifications.rs`)

```rust
pub struct StorageNotifications<Block: BlockT>(Hub<StorageNotification<Block::Hash>, Registry>);
```

- Storage değişikliklerini yayınlar
- `subscribe(100_000)` kapasite ile `Receiver` oluşturur (notifications.rs:149)
- Filter: `None` = tüm storage değişiklikleri; `Some(keys)` = sadece bu key'ler
- `child_filters`: child storage key filtreleri

**StorageChangeSet:** (notifications.rs:55-60)
- `changes: Arc<[(StorageKey, Option<StorageData>)]>`: top-level değişiklikler
- `child_changes`: child storage değişiklikleri
- `.iter()`: her iki değişiklik türünü birleştirir; `(Option<&StorageKey>, &StorageKey, Option<&StorageData>)` döner

**Registry:** (notifications/registry.rs:42-51)
```rust
pub(super) struct Registry {
    metrics: Option<SubscribersGauge>,
    wildcard_listeners: FnvHashSet<SubscriberId>,
    listeners: HashMap<StorageKey, FnvHashSet<SubscriberId>>,
    child_listeners: HashMap<StorageKey, (HashMap<StorageKey, FnvHashSet<SubscriberId>>, FnvHashSet<SubscriberId>)>,
    sinks: FnvHashMap<SubscriberId, SubscriberSink>,
}
```

`SubscriberSink::drop()` → `was_triggered = false` ise log yazar.

### 2.6 StorageProvider Trait (`backend.rs`)

```rust
pub trait StorageProvider<Block: BlockT, B: Backend<Block>> {
    fn storage(hash, key) -> Result<Option<StorageData>>;
    fn storage_hash(hash, key) -> Result<Option<Block::Hash>>;
    fn storage_keys(hash, prefix, start_key) -> Result<KeysIter<B::State, Block>>;
    fn storage_pairs(hash, prefix, start_key) -> Result<PairsIter<B::State, Block>>;
    fn child_storage(hash, child_info, key) -> Result<Option<StorageData>>;
    fn child_storage_keys(hash, child_info, prefix, start_key) -> Result<KeysIter<B::State, Block>>;
    fn child_storage_hash(hash, child_info, key) -> Result<Option<Block::Hash>>;
    fn closest_merkle_value(hash, key) -> Result<Option<MerkleValue<Block::Hash>>>;
    fn child_closest_merkle_value(hash, child_info, key) -> Result<Option<MerkleValue<Block::Hash>>>;
}
```

`KeysIter` / `PairsIter`: Iterator'lar; `start_at_exclusive = true` — başlangıç key dahil edilmez (sayfalama için). (backend.rs:371-376)

### 2.7 LeafSet (`leaves.rs`)

```rust
pub struct LeafSet<H, N> {
    storage: BTreeMap<Reverse<N>, Vec<H>>,  // block number DESC
}
```

- Tüm leaf (uç) blokları number'a göre azalan sırada tutar
- `import(hash, number, parent_hash)`: parent'ı leaf olmaktan çıkarır, yeni hash'i ekler (leaves.rs:103-116)
- `remove(hash, number, parent_hash: Option)`: caller'a bağlı; parent_hash `Some` ise parent leaf olur
- `undo()` → `Undo` struct: `undo_import`, `undo_remove`, `undo_finalization`
- `highest_leaf()`: en yüksek numaralı leaf ve hash'leri döner
- `revert(best_hash, best_number)`: `best_number`'dan yüksek tüm leaf'leri siler

**Kritik:** `revert()` sonunda best block leaf set'te yoksa otomatik eklenir (leaves.rs:199-201) — invariant koruması.

### 2.8 ProofProvider, CallExecutor, ExecutionExtensions

**ProofProvider:** (proof_provider.rs)
```rust
fn read_proof(hash, keys) -> Result<StorageProof>;
fn read_child_proof(hash, child_info, keys) -> Result<StorageProof>;
fn execution_proof(hash, method, call_data) -> Result<(Vec<u8>, StorageProof)>;
fn read_proof_collection(hash, start_keys, size_limit) -> Result<(CompactProof, u32)>;
fn storage_collection(hash, start_key, size_limit) -> Result<Vec<(KeyValueStorageLevel, bool)>>;
fn verify_range_proof(root, proof, start_keys) -> Result<(KeyValueStates, usize)>;
```

**CallExecutor:** (call_executor.rs:44-99)
```rust
fn call(at_hash, method, call_data, context: CallContext) -> Result<Vec<u8>>;
fn contextual_call(at_hash, method, call_data, changes, proof_recorder, call_context, extensions) -> Result<Vec<u8>>;
fn runtime_version(at_hash, call_context) -> Result<RuntimeVersion>;
fn prove_execution(at_hash, method, call_data) -> Result<(Vec<u8>, StorageProof)>;
```

**ExecutionExtensions:** (execution_extensions.rs:94-130)
```rust
pub struct ExecutionExtensions<Block: BlockT> {
    extensions_factory: RwLock<Box<dyn ExtensionsFactory<Block>>>,
    read_runtime_version: Arc<dyn ReadRuntimeVersion>,
}
```
- `ExtensionsFactory` trait: `extensions_for(block_hash, block_number) -> Extensions`
- `ExtensionBeforeBlock<Block, Ext>`: belirli block number'dan ÖNCE aktif olan extension
- Tüm extension'lara `ReadRuntimeVersionExt` otomatik eklenir (execution_extensions.rs:127)

---

## 3. substrate/client/rpc — V1 RPC İMPLEMENTASYONLARI

### 3.1 Author API (`author/mod.rs`)

**Struct:**
```rust
pub struct Author<P, Client> {
    client: Arc<Client>,
    pool: Arc<P>,
    keystore: KeystorePtr,
    executor: SubscriptionTaskExecutor,
}
```

**Kritik sabit:** `TX_SOURCE: TransactionSource = TransactionSource::External` (author/mod.rs:113) — tüm RPC tx'ler External olarak işaretlenir.

**Metodlar:**

| Metod | RPC Adı | Güvenlik | İşlev |
|-------|---------|----------|-------|
| `submit_extrinsic` | `author_submitExtrinsic` | Public | SCALE decode → pool.submit_one |
| `insert_key` | `author_insertKey` | UNSAFE | Keystore'a key ekle |
| `rotate_keys` | `author_rotateKeys` | UNSAFE | Session keys oluştur |
| `rotate_keys_with_owner` | `author_rotateKeysWithOwner` | UNSAFE | Owner proof ile keys oluştur |
| `has_session_keys` | `author_hasSessionKeys` | UNSAFE | Keystore check |
| `has_key` | `author_hasKey` | UNSAFE | Keystore key check |
| `pending_extrinsics` | `author_pendingExtrinsics` | Public | pool.ready() → SCALE encode |
| `remove_extrinsic` | `author_removeExtrinsic` | UNSAFE | pool.report_invalid() |
| `watch_extrinsic` | `author_submitAndWatchExtrinsic` | Public | Subscription; pool.submit_and_watch |

**watch_extrinsic akışı:** (author/mod.rs:221-253)
1. SCALE decode; hata → subscription reject
2. `pool.submit_and_watch(best_block_hash, TX_SOURCE, dxt)` → stream
3. `PendingSubscription.pipe_from_stream(stream, BoundedVecDeque::default())`
4. Subscription task olarak spawn edilir

**rotate_keys_with_owner:** SessionKeys API version check: (author/mod.rs:85-105)
- `version < 2`: `generate_session_keys_before_version_2` (deprecated)
- `version >= 2`: `generate_session_keys(owner, None)` → `GeneratedSessionKeys { keys, proof: Some(...) }`

**remove_extrinsic:** `bytes_or_hash` → hash'lere çevir → `pool.report_invalid(None, hashes)` → removed tx'ler döner.

### 3.2 State API (`state/mod.rs`, `state/state_full.rs`)

**Kritik sabitler:**
```rust
const STORAGE_KEYS_PAGED_MAX_COUNT: u32 = 1000;            // state/mod.rs:46
const MAXIMUM_SAFE_RPC_CALL_TIMEOUT: Duration = Duration::from_secs(30);  // state_full.rs:57
```

**FullState:**
```rust
pub struct FullState<BE, Block: BlockT, Client> {
    client: Arc<Client>,
    executor: SubscriptionTaskExecutor,
    execute_block: Option<Arc<dyn TracingExecuteBlock<Block>>>,
    _phantom: PhantomData<BE>,
}
```

**storage_keys_paged:** `count > 1000` → `InvalidCount` hatası döner.

**storage_size akışı:** (state_full.rs:262-309)
1. Key'e doğrudan bir değer varsa → o değerin byte boyutunu döner
2. Key yoksa → key prefix'i olarak kabul edilir; tüm eşleşen key'lerin değer boyutları toplanır
3. `DenyUnsafe::Yes` ise `MAXIMUM_SAFE_RPC_CALL_TIMEOUT = 30 saniye` timeout uygulanır
4. `spawn_blocking_with_timeout` ile çalışır; timeout → hata

**query_storage_range doğrulaması:** (state_full.rs:97-143)
- `from_number > to_number` → `InvalidBlockRange`
- `from` ve `to` farklı fork'larda → `"from and to are on different forks"` hatası
- `hashes.reverse()` → en eski'den en yeniye sıralı

**subscribe_storage:** (state_full.rs:425-478)
- `keys.is_none()` (wildcard) → `DenyUnsafe::Yes` ise reddedilir
- `keys.is_some()` → ilk yanıt olarak mevcut state gönderilir (initial snapshot)
- Sonraki yanıtlar storage change stream'den gelir
- Child storage değişiklikleri filtrelenir: `o_sk.is_none()` koşulu

**subscribe_runtime_version:** (state_full.rs:382-423)
- İlk değer olarak mevcut RuntimeVersion gönderilir
- `import_notification_stream().filter(is_new_best)` üzerinden version değişimi izlenir
- Sadece `version != previous_version` ise yeni event gönderilir

**ÖNEMLI BUG/TODO:**
- `storage_keys` ve `storage_pairs`: `.collect()` ile tüm sonuçlar bellekte toplanıyor — sonsuz/büyük storage'da OOM riski. Kod yorumlarında "TODO: Remove the `.collect`" ve "TODO: This is horribly broken; either remove it, or make it streaming" notları var. (state_full.rs:213-237)

### 3.3 Chain API (`chain/chain_full.rs`)

**FullChain:**
```rust
pub struct FullChain<Block: BlockT, Client> {
    client: Arc<Client>,
    _phantom: PhantomData<Block>,
    executor: SubscriptionTaskExecutor,
}
```

**Metodlar:**

| Metod | İşlev |
|-------|-------|
| `chain_getHeader` | `client.header(hash)` |
| `chain_getBlock` | `client.block(hash)` |
| `subscribe_all_heads` | `import_notification_stream()` tüm block'lar |
| `subscribe_new_heads` | `import_notification_stream().filter(is_new_best)` |
| `subscribe_finalized_heads` | `finality_notification_stream()` |

**subscribe_headers pattern:** (chain_full.rs:117-149)
- Best block header'ı önce gönderilir (initial state)
- Stream kurulumu ile best block arasında hole oluşabilir — bilerek kabul edilmiş risk; comment'de belirtilmiş

### 3.4 System API (`system/mod.rs`)

```rust
pub struct System<B: traits::Block> {
    info: SystemInfo,
    send_back: TracingUnboundedSender<Request<B>>,
}
```

**Pattern:** Oneshot kanallar üzerinden request-response. İstekler network service'e gönderilir; yanıtlar beklenir.

**Request enum:** (system/mod.rs:44-66)
- `Health`, `LocalPeerId`, `LocalListenAddresses`, `NodeRoles`, `SyncState` → Public
- `Peers`, `NetworkState`, `NetworkAddReservedPeer`, `NetworkRemoveReservedPeer`, `NetworkReservedPeers` → UNSAFE

**Log filter:** `system_add_log_filter` ve `system_reset_log_filter` runtime'da log seviyelerini değiştirebilir (UNSAFE). (system/mod.rs:179-188)

### 3.5 Offchain API (`offchain/mod.rs`)

**Kritik:** Sadece `StorageKind::PERSISTENT` desteklenir — `LOCAL` → `UnavailableStorageKind` hatası.
Prefix: `sp_offchain::STORAGE_PREFIX` kullanılır.

### 3.6 Dev API (`dev/mod.rs`)

**`block_stats` akışı (UNSAFE ONLY):** (dev/mod.rs:67-115)
1. Block'u al, `DigestItem::Seal` kaldır (runtime'ın beklediği sayıda digest)
2. Parent header'ı al
3. `runtime_api.record_proof()` → proof recording aç
4. `runtime_api.execute_block(parent_hash, block)` → block'u yeniden çalıştır
5. `runtime_api.extract_proof()` → state witness al; `"qed"` güvencesi: record_proof çağrıldı
6. `witness.into_compact_proof()` → compact proof

**BlockStats:** `witness_len`, `witness_compact_len`, `block_len`, `num_extrinsics`

### 3.7 Utils (`utils.rs`)

**Buffer Trait ve Implementasyonlar:**

```rust
pub trait Buffer {
    type Item;
    fn push(&mut self, item: Self::Item) -> Result<(), ()>;  // Err = buffer full
    fn pop(&mut self) -> Option<Self::Item>;
}
```

**BoundedVecDeque:**
```rust
pub struct BoundedVecDeque<T> {
    inner: VecDeque<T>,
    max_cap: usize,     // Default = DEFAULT_BUF_SIZE = 16
}
```
- Buffer dolunca → `push` `Err(())` döner → subscription DROP edilir (utils.rs:71-77)
- Test: 17. item gelince subscription düşer (utils.rs:339-341)

**RingBuffer:**
```rust
pub struct RingBuffer<T> {
    inner: VecDeque<T>,
    cap: usize,
}
```
- Buffer dolunca → en eski item silinir, yeni item eklenir (override) (utils.rs:102-108)
- V2 transaction API'de `RingBuffer::new(3)` kullanılır (transaction.rs:143)

**PendingSubscription.pipe_from_stream akışı:** (utils.rs:129-160)
1. `accept()` beklerken gelen mesajları buffer'da tutar
2. Buffer dolunca → log + subscription drop
3. `accept()` kabul edilince → `Subscription.pipe_from_stream`

**Subscription.pipe_from_stream:** (utils.rs:176-256)
- `future::select(closed, future::select(next_fut, next_item))` ile concurrent işlem
- `next_fut`: mevcut send operasyonu
- `next_item`: stream'den gelen yeni item
- `closed`: subscription kapatıldı

---

## 4. substrate/client/rpc-api — API TRAIT TANIMLARI

### 4.1 Error Code Base Values (`error.rs`)

```rust
pub mod base {
    pub const AUTHOR: i32 = 1000;
    pub const SYSTEM: i32 = 2000;
    pub const CHAIN: i32 = 3000;
    pub const STATE: i32 = 4000;
    pub const OFFCHAIN: i32 = 5000;
    pub const DEV: i32 = 6000;
    pub const STATEMENT: i32 = 7000;
    pub const MIXNET: i32 = 8000;
}
```

### 4.2 DenyUnsafe Policy (`policy.rs`)

```rust
pub enum DenyUnsafe {
    Yes,    // Unsafe RPC'lere erişimi reddet
    No,     // Her şeye izin ver
}
```

`check_if_safe(ext)` → `DenyUnsafe` extension'ı okur; `Yes` → `UnsafeRpcError` → `MethodNotFound (-32601)`.

**Güvenlik tasarımı:** Unsafe metodlar için tam "method not found" yanıtı, method keşfini engeller. (policy.rs:67-70)

### 4.3 AuthorApi Trait (`rpc-api/author/mod.rs`)

```rust
#[rpc(client, server)]
pub trait AuthorApi<Hash, BlockHash> {
    #[method(name = "author_submitExtrinsic")]
    async fn submit_extrinsic(&self, extrinsic: Bytes) -> Result<Hash, Error>;

    #[subscription(
        name = "author_submitAndWatchExtrinsic" => "author_extrinsicUpdate",
        unsubscribe = "author_unwatchExtrinsic",
        item = TransactionStatus<Hash, BlockHash>,
    )]
    fn watch_extrinsic(&self, bytes: Bytes);
    // ...
}
```

---

## 5. substrate/client/rpc-spec-v2 — V2 API İMPLEMENTASYONLARI

### 5.1 chainHead Modülü

#### ChainHeadConfig ve Kritik Sabitler (`chain_head/chain_head.rs`)

```rust
pub const MAX_PINNED_BLOCKS: usize = 512;           // chain_head.rs:85
const MAX_PINNED_DURATION: Duration = Duration::from_secs(60);  // chain_head.rs:91
const MAX_ONGOING_OPERATIONS: usize = 16;           // chain_head.rs:95
const MAX_LAGGING_DISTANCE: usize = 128;            // chain_head.rs:99
const MAX_FOLLOW_SUBSCRIPTIONS_PER_CONNECTION: usize = 4;  // chain_head.rs:102
```

**ChainHeadConfig default:**
```rust
ChainHeadConfig {
    global_max_pinned_blocks: 512,
    subscription_max_pinned_duration: 60s,
    subscription_max_ongoing_operations: 16,
    max_lagging_distance: 128,
    max_follow_subscriptions_per_connection: 4,
    subscription_buffer_cap: 512,  // = MAX_PINNED_BLOCKS
}
```

**subscription/inner.rs:**
```rust
const BUF_CAP_PER_SUBSCRIPTION: usize = 16;  // subscription/inner.rs:40
```

**STORAGE_QUERY_BUF = 16** (chain_head.rs:63, archive.rs:64)

#### ChainHead API Metodları

| Metod | İşlev |
|-------|-------|
| `chainHead_unstable_follow` | Block event subscription; `with_runtime` flag |
| `chainHead_unstable_body` | Block extrinsics (async operation) |
| `chainHead_unstable_header` | Block header (sync) |
| `chainHead_unstable_storage` | Storage query (async operation) |
| `chainHead_unstable_call` | Runtime call (async operation) |
| `chainHead_unstable_unpin` | Block unpin (single veya list) |
| `chainHead_unstable_continue` | Storage WaitingForContinue → devam |
| `chainHead_unstable_stop_operation` | Operasyonu durdur |

**follow subscription akışı:** (chain_head.rs:200-253)
1. `reserve_subscription(connection_id)` → `None` ise `ReachedLimits` (max 4/connection)
2. `pending.accept()` → `Subscription`
3. `insert_subscription(sub_id, with_runtime)` → duplicate sub_id ise `Stop` event + return
4. `ChainHeadFollower.generate_events()` → event loop
5. `BlockDistanceTooLarge` hatası → `stop_all_subscriptions()` — tüm subscriptions cascade stop

**body/call/storage metodları:** `MethodResponse::Started {operation_id, discarded_items}` veya `MethodResponse::LimitReached` döner; asıl sonuç follow subscription stream üzerinden gelir.

**`chain_head_unstable_call`:** `block_guard.has_runtime() = false` ise `InvalidRuntimeCall` hatası döner (chain_head.rs:503-507).

**`chain_head_unstable_continue`:** Bu implementasyonda `WaitingForContinue` event hiç emit edilmiyor; `get_operation()` `Some` döndürürse `InvalidContinue` hatası.

**`chain_head_unstable_unpin`:** `ListOrValue<Hash>` — tek hash veya hash listesi kabul eder. `DuplicateHashes` → `InvalidDuplicateHashes` hatası.

#### BlockStateMachine (`subscription/inner.rs:69-99`)

```
[ REGISTERED ]  ──(register)──> [ FULLY_REGISTERED ]
      │                                  │
   (unpin)                           (unpin)
      │                                  │
      v           (register)             v
[ UNPINNED ]  ──────────────> [ FULLY_UNPINNED ]
```

Her block iki kez kayıt olur: `BestBlock` event + `Finalized` event. Yanlış unpin order'a karşı koruma sağlar. `FullyUnpinned` state'e ulaşınca backend'de unpin yapılır.

#### FollowEvent Türleri (`chain_head/event.rs`)

```rust
pub enum FollowEvent<Hash> {
    Initialized(Initialized<Hash>),          // İlk event; finalized block hashes + optional runtime
    NewBlock(NewBlock<Hash>),                // block_hash + parent_block_hash + optional new_runtime
    BestBlockChanged(BestBlockChanged<Hash>), // best_block_hash
    Finalized(Finalized<Hash>),              // finalized_block_hashes + pruned_block_hashes
    OperationBodyDone(OperationBodyDone),    // operation_id + extrinsics[]
    OperationCallDone(OperationCallDone),    // operation_id + hex output
    OperationStorageItems(OperationStorageItems), // operation_id + items[]
    OperationWaitingForContinue(OperationId),
    OperationStorageDone(OperationId),
    OperationInaccessible(OperationId),      // retry olabilir
    OperationError(OperationError),          // retry OLMAZ
    Stop,
}
```

**Sıralama:** `Initialized` → `NewBlock` → `BestBlockChanged` → `Finalized` (ve bunlar arasında operation event'leri)

**with_runtime flag:** Custom `Serialize` implementasyonu (event.rs:129-147, 170-190):
- `false`: `finalizedBlockRuntime` ve `newRuntime` alanları JSON'da hiç yer almaz
- `true` + runtime değişti: runtime event gönderilir
- `true` + runtime değişmedi: `"newRuntime": null`

#### ChainHeadRuntimeVersion (`event.rs:52-82`)

Simplified `sp_version::RuntimeVersion` — spec V2'ye uygun:
- `spec_name`, `impl_name`, `spec_version`, `impl_version`
- `apis: BTreeMap<String, u32>` — API hash'ler hex encoded, sorted (deterministic JSON)
- `transaction_version`

### 5.2 Transaction V2 Modülü

#### TransactionEvent (`transaction/event.rs`)

```rust
pub enum TransactionEvent<Hash> {
    Validated,                                    // Pool'a girdi
    BestChainBlockIncluded(Option<TransactionBlock<Hash>>), // None = retracted
    Finalized(TransactionBlock<Hash>),
    Error(TransactionError),
    Invalid(TransactionError),
    Dropped(TransactionDropped),
}
```

**Final events** (`is_final()`): `Finalized`, `Error`, `Invalid`, `Dropped` (event.rs:99-108)

**V1 → V2 Mapping** (`transaction/transaction.rs:164-194`):

| V1 TransactionStatus | V2 TransactionEvent |
|---------------------|---------------------|
| `Ready` / `Future` | `Validated` |
| `InBlock((hash, index))` | `BestChainBlockIncluded(Some({hash, index}))` |
| `Retracted(_)` | `BestChainBlockIncluded(None)` |
| `FinalityTimeout(_)` | `Dropped { error: "Maximum number of finality watchers..." }` |
| `Finalized((hash, index))` | `Finalized({hash, index})` |
| `Usurped(_)` | `Invalid { error: "rendered invalid by another extrinsic" }` |
| `Dropped` | `Dropped { error: "exceeding limits" }` |
| `Invalid` | `Invalid { error: "marked as invalid" }` |
| `Broadcast(_)` | **None** — V2'de desteklenmiyor |

**Yavaş subscription:** `RingBuffer::new(3)` ile eski event'ler override edilir (transaction.rs:143)

**JSON IR yapısı:** Block event'ler `tag+content`, non-block event'ler sadece `tag` ile serialize. Bu nedenle `TransactionEventIR<Hash>` ara katmanı kullanılır. (event.rs:122-213)

#### TransactionBroadcast (`transaction_broadcast.rs`)

```rust
pub struct TransactionBroadcast<Pool: TransactionPool, Client> {
    client: Arc<Client>,
    pool: Arc<Pool>,
    executor: SubscriptionTaskExecutor,
    broadcast_ids: Arc<RwLock<HashMap<String, BroadcastState<Pool>>>>,
    rpc_connections: RpcConnections,
}

struct BroadcastState<Pool: TransactionPool> {
    handle: AbortHandle,
    tx_hash: <Pool as TransactionPool>::Hash,
}
```

**Operation ID üretimi:** (transaction_broadcast.rs:85-99)
- 16 karakter alphanumeric random string
- Unique olana kadar tekrar dener (broadcast_ids lock altında kontrol)

**`stop` çağrısı:** `AbortHandle.abort()` → broadcast future iptal edilir; tx hash ile pool'dan da kaldırılır.

### 5.3 Archive Modülü

```rust
pub struct Archive<BE: Backend<Block>, Block: BlockT, Client> {
    client: Arc<Client>,
    backend: Arc<BE>,
    executor: SubscriptionTaskExecutor,
    genesis_hash: String,        // hex encoded
    _phantom: PhantomData<Block>,
}
```

Historical block/storage queries için — `chainHead` gibi pinning gerektirmez; sadece mevcut finalized data. `STORAGE_QUERY_BUF = 16` kapasite ile storage sorguları yapılır.

### 5.4 RpcConnections (`common/connections.rs`)

```rust
pub struct RpcConnections {
    capacity: usize,
    data: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

struct ConnectionData {
    num_identifiers: usize,     // reserved + registered toplam
    identifiers: HashSet<String>,
}
```

**RAII pattern:** (connections.rs:146-188)
- `reserve_space(conn_id)` → `Some(ReservedConnection)` veya `None` (kapasite dolu)
- `ReservedConnection.register(id)` → `Some(RegisteredConnection)` veya `None` (duplicate)
- `ReservedConnection` drop → `unreserve_space` (accept olmadan kaynak sızıntısı engellenir)
- `RegisteredConnection` drop → `unregister_identifier`

**Kullanım:** chainHead: max 4 subscription/connection; transactionBroadcast: `max_transactions_per_connection`

---

## 6. KRİTİK SAYISAL DEĞERLER

| Sabit | Değer | Kaynak |
|-------|-------|--------|
| `DEFAULT_BUF_SIZE` | 16 | utils.rs:33 |
| `STORAGE_KEYS_PAGED_MAX_COUNT` | 1000 | state/mod.rs:46 |
| `MAXIMUM_SAFE_RPC_CALL_TIMEOUT` | 30 saniye | state_full.rs:57 |
| `StorageNotifications subscribe capacity` | 100_000 | notifications.rs:149 |
| `MAX_PINNED_BLOCKS` | 512 | chain_head.rs:85 |
| `MAX_PINNED_DURATION` | 60 saniye | chain_head.rs:91 |
| `MAX_ONGOING_OPERATIONS` | 16 | chain_head.rs:95 |
| `MAX_LAGGING_DISTANCE` | 128 blok | chain_head.rs:99 |
| `MAX_FOLLOW_SUBSCRIPTIONS_PER_CONNECTION` | 4 | chain_head.rs:102 |
| `BUF_CAP_PER_SUBSCRIPTION` | 16 | subscription/inner.rs:40 |
| `STORAGE_QUERY_BUF` | 16 | chain_head.rs:63; archive.rs:64 |
| `TransactionBroadcast RingBuffer` | 3 | transaction.rs:143 |
| `OPERATION_ID_LEN` | 16 karakter | transaction_broadcast.rs:88 |
| `author_rpc error base` | 1000 | error.rs |
| `state_rpc error base` | 4000 | error.rs |
| `DenyUnsafe → ErrorCode` | -32601 (MethodNotFound) | policy.rs:67-70 |

---

## 7. GÜVENLİK PARADİGMASI

### 7.1 DenyUnsafe Mekanizması

- Her RPC endpoint extension'ından `DenyUnsafe` okunur
- `check_if_safe(ext)` → `UnsafeRpcError` → `MethodNotFound (-32601)` — kasıtlı olarak metod yok gibi görünür
- UNSAFE metodlar: insert_key, rotate_keys, has_session_keys, has_key, remove_extrinsic, system_peers, storage_pairs, query_storage, trace_block, offchain_set_local_storage, vb.
- `storage_size` unsafe değil ama `DenyUnsafe::Yes` ile 30s timeout uygulanır

### 7.2 Wildcard Storage Subscription

`subscribe_storage(keys: None)` → tüm storage değişikliklerini yayınlar → `DenyUnsafe::Yes` ise reddedilir.

### 7.3 CallContext

Tüm RPC çağrıları `CallContext::Offchain` kullanır → `TrieCacheContext::Untrusted` → sınırlı trie cache. Block import ise `CallContext::Onchain` → `TrieCacheContext::Trusted`.

### 7.4 Transaction Source

Tüm RPC üzerinden gelen tx'ler `TransactionSource::External` — local/in-block kaynaklardan ayrı tutulur.

---

## 8. SUBSCRIPTION YÖNETİMİ VE BACKPRESSURE

### 8.1 V1 Subscription Pattern

```
PendingSubscriptionSink
  → PendingSubscription.pipe_from_stream(stream, BoundedVecDeque::default())
    → accept() beklerken buffer'a ekler (max 16)
    → buffer dolunca log + subscription DROP
  → Subscription.pipe_from_stream()
    → concurrent: send_fut + stream.next() + closed()
    → buffer dolunca log + DROP
```

### 8.2 V2 Chain Head Pattern

```
pending.accept() → Subscription
  → ChainHeadFollower.generate_events(sink, sub_data)
    → tokio::select! üzerinde block events dinle
    → max_lagging_distance aşılırsa tüm subscriptions STOP
```

### 8.3 V2 Transaction Pattern

```
pool.submit_and_watch() → stream
  → filter_map(handle_event)  // V1→V2 dönüşüm + Broadcast → None filtreleme
  → sink.pipe_from_stream(stream, RingBuffer::new(3))  // yavaş client için ring
```

---

## 9. BACKEND PINNING MEKANİZMASI

### Neden Pinning?

Block pruning sistemi, backend block state'ini silebilir. RPC operasyonları devam ederken block state silinmemesi için pinning gereklidir.

### Pin/Unpin Akışı

1. Block import: `pin_block(hash)` (backend)
2. Notification oluştur: `UnpinHandle::new(hash, sender)` → `AnnouncePin` mesajı gönderilmez (sadece Unpin gönderilir)
3. Notification kullanıcıya gönderilir
4. Notification drop edilince: `UnpinHandleInner::drop()` → `Unpin(hash)` mesajı → `unpin_block(hash)`
5. `into_unpin_handle()` kullanılırsa: kullanıcı handle'ı tuttuğu sürece block pinned

**chainHead:** Her subscription başlı başına block'ları pin tutar; `lock_block()` ile operation süresince ek pin. `BlockStateMachine::FullyUnpinned` → backend unpin.

---

## 10. TEST ANALİZİ

### utils.rs Tests

**`pipe_from_stream_works`:** 16 item → hepsi alınır.

**`pipe_from_stream_with_bounded_vec`:** 32 item, cap 16 → 17. item'da subscription drop.

**`subscription_is_dropped_when_stream_is_empty`:** Empty stream → pipe_from_stream anında döner, notify gönderir.

**`subscription_replace_old_messages`:** RingBuffer(3), 0-20 arası, 10s sleep → `[0, 17, 18, 19]` alınır. (0 alınır çünkü pending send cancel edilemiyor; utils.rs:402-403)

### connections.rs Tests

**`reserve_space`:** capacity=2; reserve → register → drop → data tamamen temizlenir.

**`reserve_space_capacity_reached`:** 2/2 dolu iken `reserve_space` → `None`. Biri drop edilince tekrar reserve edilebilir.

### event.rs Tests (chain_head, transaction)

`FollowEvent` serialization/deserialization doğrulaması — camelCase format, `event` tag kullanımı.

`TransactionEvent` IR serialization — block event'ler `tag+content`, non-block `tag only`.

---

## 11. NOVA SHIELD CHAIN İÇİN ÇIKARIMLAR

### 11.1 Backend Trait Implementasyonu

Nova Shield Chain için custom Backend trait implementasyonu gerekiyor:
- `pin_block`/`unpin_block`: RocksDB'de referans sayımı
- `state_at(hash, TrieCacheContext)`: context'e göre cache boyutu
- `get_import_lock()`: dış bileşenler için import kilidi

**Öneri:** `sc-client-db`'nin implementasyonunu başlangıç noktası olarak al; trie cache boyutlarını chain yüküne göre ayarla.

### 11.2 RPC Katmanı Tasarımı

**V1 vs V2:**
- V1 (`sc-rpc`) şimdilik yeterli; daha basit, test edilmiş
- V2 (`rpc-spec-v2`) aktif geliştirmede; `unstable_` prefix — breaking changes olabilir
- İkisini birden destekleyebilirsin — Polkadot node gibi

**Güvenlik politikası:**
```rust
// Geliştirme modunda:
DenyUnsafe::No

// Production/mainnet:
DenyUnsafe::Yes (--rpc-methods=safe)
```

### 11.3 Subscription Kapasiteleri

Aşağıdaki değerleri chain yüküne göre ayarla:

| Parametre | Polkadot Default | Nova Shield Önerisi |
|-----------|-----------------|---------------------|
| MAX_PINNED_BLOCKS | 512 | 256-512 (block hızına göre) |
| MAX_FOLLOW_SUBSCRIPTIONS | 4/connection | 2-4 (resource kontrolü) |
| MAX_ONGOING_OPERATIONS | 16 | 8-16 |
| MAX_LAGGING_DISTANCE | 128 | 64-128 blok |
| BoundedVecDeque default | 16 | 16-32 |
| TransactionBroadcast RingBuffer | 3 | 3-5 |

### 11.4 storage_keys/storage_pairs Güvenlik

Bu metodları Nova Shield için ya:
- Tamamen kaldır (V2 API kullan)
- Ya da max key sayısı koy (ör. 10_000)
- Streaming implementasyonu yaz

### 11.5 chainHead V2 Pinning Kısıtlaması

chainHead kullanıyorsan `PINNING_CACHE_SIZE` (client/db'de) ≥ `MAX_PINNED_BLOCKS = 512` olmalı.

### 11.6 Transaction V2 Ring Buffer

Yavaş client'lar için `RingBuffer::new(3)` sadece son 3 event'i saklar. İlk event (`Validated`) kaybedilebilir. Nova Shield için daha büyük buffer düşünülebilir.

### 11.7 Error Code Namespace

`rpc-api/error.rs`'deki base değerlerin üzerine yazmak yerine 9000+ aralığını kullan.

### 11.8 LeafSet Invariant

`LeafSet.revert()` sonunda best block leaf set'te yoksa otomatik eklenir — bu invariant kritik, override etme.

### 11.9 ExtensionsFactory Kullanımı

Custom runtime extension'ları için `ExtensionBeforeBlock` kullanılabilir — örneğin eski runtime API'si sadece belirli bir block'a kadar destekleniyorsa.

### 11.10 import_notification_stream vs every_import

- Tx pool ve chain state dinleyiciler → `every_import_notification_stream()` kullan
- UI/explorer için → `import_notification_stream()` (sadece best block + reorg)
- Finality → `finality_notification_stream()`

---

## 12. KRİTİK BULGULAR ÖZETİ

1. **`storage_keys` / `storage_pairs` BUG:** Sonsuz iterator'ı `.collect()` ile belleğe alıyor; state_full.rs:213-237. `"TODO: This is horribly broken"` yorumu mevcut.

2. **Unsafe → MethodNotFound:** `UnsafeRpcError` kasıtlı olarak `-32601 (MethodNotFound)` olarak serialize ediliyor — security through obscurity pattern.

3. **chainHead Stop Propagation:** `BlockDistanceTooLarge` hatası geldiğinde `stop_all_subscriptions()` çağrılıyor — cascade stop.

4. **Block Pinning RAII:** `UnpinHandleInner::drop()` ile block'lar `Unpin` mesajı göndererek serbest bırakılıyor. Notification drop'u = block unpin.

5. **import_notification_stream initial sync boşluğu:** Initial sync sırasında reorg yoksa event gelmez — `every_import_notification_stream()` her block için garantili.

6. **BlockStateMachine:** Her block'u iki kez kayıt eder (BestBlock + Finalized); erken unpin'e karşı race condition koruması.

7. **Transaction V2 Broadcast kaybı:** `Broadcast(_)` event'i V2'de `None` → filter edilir; client bu event'i almaz.

8. **Storage subscribe wildcard:** `None` key → tüm storage değişiklikleri → büyük chain'lerde bandwidth riski; UNSAFE kısıtlamasıyla sınırlandırılmış.

9. **`storage_size` prefix semantiği:** Direkt key yoksa tüm eşleşen key'lerin değer toplamı hesaplanır → büyük prefix'lerde timeout riski.

10. **RpcConnections two-phase:** `reserve_space` → `register` iki aşamalı; `ReservedConnection` drop'ta otomatik unreserve sağlar.

11. **LeafSet revert invariant:** `best_hash` leaf set'te yoksa otomatik eklenir (leaves.rs:199-201) — revert sonrası chain integrity.

12. **chainHead_continue WaitingForContinue never emitted:** Bu implementasyonda `WaitingForContinue` emit edilmiyor; `continue` çağrısı `InvalidContinue` hatası döner.
