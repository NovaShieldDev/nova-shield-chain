# Solana Gossip Protokol Analizi — Agent 16

**Analiz Tarihi:** 2026-07-22
**Kapsam:** gossip/src/ (26 dosya)
**Toplam Satır:** ~12.000+ satır

---

## İçindekiler

1. [Dosya Listesi](#dosya-listesi)
2. [crds_value.rs — Gossip Mesaj Tipleri](#crds_valuers)
3. [crds.rs — Cluster Replicated Data Store](#crdsrs)
4. [crds_shards.rs — Shard İndeksleme](#crds_shardsrs)
5. [crds_entry.rs — Tip Güvenli Lookup](#crds_entryrs)
6. [crds_gossip.rs — Push/Pull Orkestrasyon](#crds_gossiprs)
7. [crds_gossip_push.rs — Push Protokolü (Plumtree)](#crds_gossip_pushrs)
8. [crds_gossip_pull.rs — Pull + Bloom Filter Anti-Entropy](#crds_gossip_pullrs)
9. [push_active_set.rs — Ağırlıklı Push Hedef Seçimi](#push_active_setrs)
10. [received_cache.rs — Prune Kararı için Skor Sistemi](#received_cachers)
11. [weighted_shuffle.rs — Fenwick Tree Tabanlı Ağırlıklı Karıştırma](#weighted_shufflers)
12. [cluster_info.rs — Ana Gossip Katmanı](#cluster_infors)
13. [contact_info.rs — Yeni Node Adresi Yapısı](#contact_infors)
14. [legacy_contact_info.rs — Eski Node Adresi](#legacy_contact_infors)
15. [ping_pong.rs — NAT Tüneli ve Adres Doğrulama](#ping_pongrs)
16. [gossip_service.rs — Gossip Servis Thread'leri](#gossip_servicers)
17. [epoch_slots.rs — Slot Durumu Sıkıştırma](#epoch_slotsrs)
18. [duplicate_shred.rs — Çift Shred Kanıtı](#duplicate_shredrs)
19. [duplicate_shred_handler.rs — Çift Shred İşleme](#duplicate_shred_handlerrs)
20. [duplicate_shred_listener.rs — Listener Thread](#duplicate_shred_listenerrs)
21. [restart_crds_values.rs — Yeniden Başlatma Koordinasyonu](#restart_crds_valuesrs)
22. [cluster_info_metrics.rs — Metrik Altyapısı](#cluster_info_metricsrs)
23. [crds_gossip_error.rs — Gossip Hata Tipleri](#crds_gossip_errorrs)
24. [gossip_error.rs — Üst Düzey Hatalar](#gossip_errorrs)
25. [deprecated.rs — Eski Yapılar](#deprecatedrs)
26. [lib.rs ve main.rs](#librs-ve-mainrs)
27. [Nova Shield Chain için Dersler](#nova-shield-chain-için-dersler)

---

## Dosya Listesi

```
gossip/src/
├── cluster_info.rs            (~4000 satır) — Ana merkez
├── cluster_info_metrics.rs    (~300 satır)  — Metrikler
├── contact_info.rs            (~964 satır)  — Yeni node adresi
├── crds.rs                    (~800 satır)  — CRDS store
├── crds_entry.rs              (~123 satır)  — Tip güvenli lookup
├── crds_gossip.rs             (~491 satır)  — Push/Pull orkestrasyon
├── crds_gossip_error.rs       (~8 satır)    — Gossip hataları
├── crds_gossip_pull.rs        (~900 satır)  — Pull + bloom filter
├── crds_gossip_push.rs        (~622 satır)  — Push (Plumtree)
├── crds_shards.rs             (~240 satır)  — Shard indeks
├── crds_value.rs              (~1097 satır) — Mesaj tipleri
├── deprecated.rs              (~22 satır)   — Eski yapılar
├── duplicate_shred.rs         (~1140 satır) — Çift shred kanıtı
├── duplicate_shred_handler.rs (~200 satır)  — Çift shred işleme
├── duplicate_shred_listener.rs(~130 satır)  — Listener thread
├── epoch_slots.rs             (~548 satır)  — Slot sıkıştırma
├── gossip_error.rs            (~31 satır)   — Üst düzey hatalar
├── gossip_service.rs          (~451 satır)  — Servis thread'leri
├── legacy_contact_info.rs     (~400 satır)  — Eski node adresi
├── lib.rs                     (~45 satır)   — Modül bildirimleri
├── main.rs                    (~345 satır)  — CLI (spy tool)
├── ping_pong.rs               (~412 satır)  — NAT doğrulama
├── push_active_set.rs         (~320 satır)  — Push hedef seçimi
├── received_cache.rs          (~195 satır)  — Prune skoru
├── restart_crds_values.rs     (~371 satır)  — Yeniden başlatma
└── weighted_shuffle.rs        (~361 satır)  — Fenwick tree shuffle
```

---

## crds_value.rs

**Yol:** `gossip/src/crds_value.rs`
**Satır:** 1–1097

Bu dosya gossip ağında dolaşan tüm mesaj tiplerini tanımlar.

### Sabitler (satır 32–41)

```rust
pub const MAX_WALLCLOCK: u64 = 1_000_000_000_000_000;
pub const MAX_SLOT: u64 = 1_000_000_000_000_000;
pub type VoteIndex = u8;
pub const MAX_VOTES: VoteIndex = 32;          // Her node 32 oya kadar yayabilir
pub type EpochSlotsIndex = u8;
pub const MAX_EPOCH_SLOTS: EpochSlotsIndex = 255;  // 255 epoch slot chunk'ı
```

### CrdsValue Struct (satır 44–48)

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct CrdsValue {
    pub signature: Signature,   // Ed25519 imzası
    pub data: CrdsData,         // Gerçek veri
}
```

**Tasarım:** Her değer imzalı. İmza hem kimlik doğrulama hem de bütünlük denetimi sağlar.
`Signable` trait'i impl edilmiş — `signable_data()` → `serialize(&self.data)` döner.

### CrdsData Enum — Tüm Gossip Mesaj Tipleri (satır 85–100)

```rust
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample, AbiEnumVisitor)]
pub enum CrdsData {
    LegacyContactInfo(LegacyContactInfo),                     // Eski node bilgisi (deprecated yönünde)
    Vote(VoteIndex, Vote),                                    // Validator oyu (0-31 index)
    LowestSlot(/*DEPRECATED:*/ u8, LowestSlot),              // Deprecated
    LegacySnapshotHashes(LegacySnapshotHashes),               // Deprecated
    AccountsHashes(AccountsHashes),                           // Deprecated
    EpochSlots(EpochSlotsIndex, EpochSlots),                  // Slot durumu (0-254 index)
    LegacyVersion(LegacyVersion),                             // Eski versiyon bilgisi
    Version(Version),                                          // Versiyon bilgisi
    NodeInstance(NodeInstance),                                // Node kimlik tokeni
    DuplicateShred(DuplicateShredIndex, DuplicateShred),      // Çift shred kanıtı
    SnapshotHashes(SnapshotHashes),                            // Snapshot hash'leri
    ContactInfo(ContactInfo),                                  // Yeni node bilgisi
    RestartLastVotedForkSlots(RestartLastVotedForkSlots),      // Yeniden başlatma için
    RestartHeaviestFork(RestartHeaviestFork),                  // Yeniden başlatma için
}
```

**Kritik:** `CrdsData::Vote(VoteIndex, Vote)` — Her node aynı anda 32'ye kadar oy yayabilir. Bu, lockout history'nin farklı slotlarını kapsar. Bu tasarım, fork çakışmalarının gossip üzerinden tespit edilmesini sağlar.

### CrdsValueLabel Enum (satır 499–515)

```rust
#[derive(PartialEq, Hash, Eq, Clone, Debug)]
pub enum CrdsValueLabel {
    LegacyContactInfo(Pubkey),
    Vote(VoteIndex, Pubkey),
    LowestSlot(Pubkey),
    LegacySnapshotHashes(Pubkey),
    EpochSlots(EpochSlotsIndex, Pubkey),
    AccountsHashes(Pubkey),
    LegacyVersion(Pubkey),
    Version(Pubkey),
    NodeInstance(Pubkey),
    DuplicateShred(DuplicateShredIndex, Pubkey),
    SnapshotHashes(Pubkey),
    ContactInfo(Pubkey),
    RestartLastVotedForkSlots(Pubkey),
    RestartHeaviestFork(Pubkey),
}
```

**Tasarım:** Label = (tip, pubkey[, index]) üçlüsü. CRDS tablosundaki birincil anahtar. Birden fazla Vote veya EpochSlots, farklı index'ler sayesinde aynı pubkey altında birlikte var olabilir.

### Vote Struct (satır 305–350)

```rust
#[derive(Clone, Debug, PartialEq, Eq, AbiExample, Serialize)]
pub struct Vote {
    pub(crate) from: Pubkey,
    transaction: Transaction,    // Gerçek vote transaction'ı
    pub(crate) wallclock: u64,
    #[serde(skip_serializing)]   // Hash'e dahil edilmez
    slot: Option<Slot>,          // Parse edilmiş son oy slotu
}
```

`Vote::new()` — `vote_parser::parse_vote_transaction()` çağırır; transaction geçerli bir vote tx değilse `None` döner.

`Deserialize` custom impl (satır 352–370): Deserialize sırasında da parse yeniden yapılır, `slot` alanı yeniden hesaplanır.

### NodeInstance Struct (satır 427–488)

```rust
#[derive(Clone, Debug, PartialEq, Eq, AbiExample, Deserialize, Serialize)]
pub struct NodeInstance {
    from: Pubkey,
    wallclock: u64,
    timestamp: u64,  // Node başlangıç zamanı
    token: u64,      // Rastgele, her yeniden başlatmada değişir
}
```

**Algoritma — Duplicate Node Tespiti:**
- `check_duplicate()` (satır 459–468): Aynı `from`, farklı `token`, daha yeni `timestamp` → duplicate.
- `overrides()` (satır 473–487): Timestamp karşılaştırması; eşitlik durumunda `token` ile deterministik tiebreak.
- **Amaç:** Aynı node'un iki instance'ı çalışırken, eski instance suppress edilir. Yeni başlayan node, eskisini override eder.

### CrdsValue impl (satır 569–686)

```rust
pub fn new_unsigned(data: CrdsData) -> Self { ... }
pub fn new_signed(data: CrdsData, keypair: &Keypair) -> Self { ... }
pub fn wallclock(&self) -> u64 { ... }   // Her varyant için match
pub fn pubkey(&self) -> Pubkey { ... }   // Her varyant için match
pub fn label(&self) -> CrdsValueLabel { ... }  // CrdsValueLabel oluşturur
pub fn contact_info(&self) -> Option<&LegacyContactInfo> { ... }
pub fn epoch_slots(&self) -> Option<&EpochSlots> { ... }
pub fn size(&self) -> u64 { serialized_size(&self) }
pub fn should_force_push(&self, peer: &Pubkey) -> bool { ... }  // NodeInstance'lar her zaman iletilir
```

### filter_current() Fonksiyonu (satır 690–710)

```rust
pub(crate) fn filter_current<'a, I>(values: I) -> impl Iterator<Item = &'a CrdsValue>
```

Aynı label için en yüksek wallclock'u olan değeri tutar. HashMap kullanır.

### AccountsHashes Struct (satır 177–221)

```rust
pub struct AccountsHashes {
    pub from: Pubkey,
    pub hashes: Vec<(Slot, Hash)>,  // (slot, accounts_hash) çiftleri
    pub wallclock: u64,
}
```

`MAX_ACCOUNTS_HASHES = 16` — serialize edilmiş boyutu PACKET_DATA_SIZE içinde tutar.

### SnapshotHashes Struct (satır 225–249)

```rust
pub struct SnapshotHashes {
    pub from: Pubkey,
    pub full: (Slot, Hash),                    // Tam snapshot
    pub incremental: Vec<(Slot, Hash)>,        // Artımlı snapshot'lar
    pub wallclock: u64,
}
```

Sanitize: `full.slot < incremental.slot` kontrolü yapılır.

### Test Fonksiyonları (satır 720–1096)

- `test_keys_and_values` — Label-wallclock ilişkisi
- `test_lowest_slot_sanitize` — LowestSlot doğrulama
- `test_signature` — İmza oluşturma/doğrulama
- `test_max_vote_index` — MAX_VOTES aşımı reddi
- `test_vote_round_trip` — Vote serde döngüsü, `slot` alanının doğru recover edilmesi
- `test_max_epoch_slots_index` — MAX_EPOCH_SLOTS aşımı reddi
- `test_filter_current` — 4096 rastgele değer → en güncel filtrele
- `test_node_instance_crds_label` — NodeInstance label stabilitesi
- `test_check_duplicate_instance` — Duplicate tespit ve override mantığı

---

## crds.rs

**Yol:** `gossip/src/crds.rs`
**Satır:** ~800

### Sabitler (satır 57–66)

```rust
const CRDS_SHARDS_BITS: u32 = 12;          // 4096 shard (2^12)
const VOTE_SLOTS_METRICS_CAP: usize = 100;  // Metrik için LRU boyutu
const SIGNATURE_SAMPLE_LEADING_ZEROS: u32 = 19;  // Sampling için
```

### Crds Struct (satır 68–89)

```rust
pub struct Crds {
    table: IndexMap<CrdsValueLabel, VersionedCrdsValue>,
    cursor: Cursor,
    shards: CrdsShards,
    nodes: IndexSet<usize>,
    votes: BTreeMap<u64 /*insert order*/, usize /*index*/>,
    epoch_slots: BTreeMap<u64 /*insert order*/, usize /*index*/>,
    duplicate_shreds: BTreeMap<u64 /*insert order*/, usize /*index*/>,
    records: HashMap<Pubkey, IndexSet<usize>>,
    entries: BTreeMap<u64 /*insert order*/, usize /*index*/>,
    purged: VecDeque<(Hash, u64 /*timestamp*/)>,
    shred_versions: HashMap<Pubkey, u16>,
    stats: Mutex<CrdsStats>,
}
```

**Tasarım Kararları:**
- `IndexMap` kullanımı: Hem hash-tabanlı O(1) lookup hem de sıralı indexleme sağlar. Shard ve node index'leri `IndexMap` içindeki konumu (usize) saklar.
- `BTreeMap<u64, usize>` için votes/epoch_slots/entries: Insert sırasına göre range query yapabilmek için. `get_entries(cursor)` gibi fonksiyonlar burada BTreeMap range'i kullanır.
- `IndexSet` (nodes): ContactInfo'ların index'leri. Tekrar kontrol için.
- `purged: VecDeque<(Hash, u64)>`: Pull filter'ına eklenecek, yakın zamanda silinen değerlerin hash'leri. Pull requestor'ın eski değerler istemesini engeller.

### CrdsError Enum (satır 91–96)

```rust
pub enum CrdsError {
    DuplicatePush(/*num dups:*/ u8),
    InsertFailed,
    UnknownStakes,
}
```

`DuplicatePush(n)` — Aynı değer n kez daha önce push ile alınmış. `ReceivedCache` bu sayıyı kullanır.

### GossipRoute Enum (satır 98–104)

```rust
pub enum GossipRoute<'a> {
    LocalMessage,
    PullRequest,
    PullResponse,
    PushMessage(/*from:*/ &'a Pubkey),
}
```

Her insert'in kaynağı takip edilir. `PushMessage` route'u gönderenin pubkey'ini içerir.

### VersionedCrdsValue Struct (satır 124–137)

```rust
pub struct VersionedCrdsValue {
    ordinal: u64,               // Insert sırası (monoton artan)
    pub value: CrdsValue,
    pub(crate) local_timestamp: u64,    // Yerel insert zamanı (ms)
    pub(crate) value_hash: Hash,        // Bincode serialize hash
    num_push_recv: Option<u8>,
    // None → LocalMessage veya PullRequest
    // Some(0) → PullResponse
    // Some(k) k>0 → PushMessage, k-1 önceki push sayısı
}
```

### Cursor Struct (satır 139–152)

```rust
#[derive(Clone, Copy, Default)]
pub struct Cursor(u64);

impl Cursor {
    fn ordinal(&self) -> u64 { self.0 }
    fn consume(&mut self, ordinal: u64) {
        self.0 = self.0.max(ordinal + 1);
    }
}
```

**Tasarım:** Cursor, "en son okunan ordinal + 1" değerini tutar. `get_entries(cursor)` çağrısı, cursor'dan sonraki tüm yeni girişleri verir ve cursor'ı günceller. Bu sayede her subscriber kendi cursor'ını tutar ve sadece yeni değerleri alır.

### overrides() Fonksiyonu (satır 195–216)

```rust
fn overrides(value: &CrdsValue, other: &VersionedCrdsValue) -> bool {
    // NodeInstance özel kural
    if let CrdsData::NodeInstance(value) = &value.data {
        if let Some(out) = value.overrides(&other.value) {
            return out;
        }
    }
    match value.wallclock().cmp(&other.value.wallclock()) {
        Ordering::Less => false,
        Ordering::Greater => true,
        Ordering::Equal => {
            // Tiebreak: hash karşılaştırması (deterministik)
            let value_hash = hash(&serialize(&value).unwrap());
            other.value_hash < value_hash
        }
    }
}
```

**Merge Stratejisi:**
1. NodeInstance için timestamp+token karşılaştırması
2. Diğerleri için: wallclock karşılaştırması
3. Eşitlik: bincode hash karşılaştırması (tüm cluster'da tutarlı)

### insert() Fonksiyonu (satır 229–329)

```rust
pub fn insert(
    &mut self,
    value: CrdsValue,
    now: u64,
    route: GossipRoute,
) -> Result<(), CrdsError>
```

**Algoritma:**

**Yeni Giriş (Vacant):**
1. `VersionedCrdsValue::new()` ile wrapper oluştur
2. `shards.insert(entry_index, &value)` — Bloom filter lookup için
3. Tipe göre index güncelle (nodes, votes, epoch_slots, duplicate_shreds)
4. `entries.insert(ordinal, entry_index)` — Cursor tabanlı erişim için
5. `records[pubkey].insert(entry_index)` — Pubkey bazlı erişim için
6. `cursor.consume(ordinal)`
7. `table.insert(label, value)`

**Mevcut Giriş, Override Ediliyor (Occupied + overrides=true):**
1. Eski shard'ı sil, yenisini ekle
2. Tipe göre BTreeMap güncelle (eski ordinal sil, yeni ekle)
3. `purged.push_back((eski_hash, now))` — Eski hash'i "bilinen" listesine ekle
4. Yeni değeri yaz

**Mevcut Giriş, Override Edilemiyor:**
- Hash farklıysa: `purged.push_back(yeni_hash)`, `CrdsError::InsertFailed`
- Hash aynıysa ve PushMessage ise: `num_push_recv` artır, `CrdsError::DuplicatePush(sayı)`

### get() Fonksiyonu (satır 331–336)

```rust
pub fn get<'a, 'b, V>(&'a self, key: V::Key) -> Option<V>
where
    V: CrdsEntry<'a, 'b>,
{
    V::get_entry(&self.table, key)
}
```

Generic lookup. `crds.get::<&CrdsValue>(&label)`, `crds.get::<&LegacyContactInfo>(pubkey)` gibi kullanımlar.

### Cursor Tabanlı Iterator'lar (satır 342–415)

```rust
pub(crate) fn get_nodes(&self) -> impl Iterator<Item = &VersionedCrdsValue>
pub(crate) fn get_votes<'a>(&'a self, cursor: &'a mut Cursor) -> impl Iterator
pub(crate) fn get_epoch_slots<'a>(&'a self, cursor: &'a mut Cursor) -> impl Iterator
pub(crate) fn get_duplicate_shreds<'a>(&'a self, cursor: &'a mut Cursor) -> impl Iterator
pub(crate) fn get_entries<'a>(&'a self, cursor: &'a mut Cursor) -> impl Iterator
pub(crate) fn get_records(&self, pubkey: &Pubkey) -> impl Iterator
```

Hepsi BTreeMap üzerinde `range((Included(cursor.ordinal()), Unbounded))` çalıştırır.

### filter_bitmask() (satır 464–472)

```rust
pub(crate) fn filter_bitmask(
    &self,
    mask: u64,
    mask_bits: u32,
) -> impl Iterator<Item = &VersionedCrdsValue>
{
    self.shards.find(mask, mask_bits).map(move |i| self.table.index(i))
}
```

Pull response üretiminde kullanılır. CrdsFilter'daki mask/mask_bits ile eşleşen değerleri shard indeksinden hızla bulur.

### find_old_labels() (satır 496+)

Paralel thread pool üzerinden tüm değerleri tarar, timeout'u geçenleri döner. `CrdsTimeouts` ile pubkey başına farklı timeout uygulanır.

### trim_purged() (satır 453–460)

```rust
pub(crate) fn trim_purged(&mut self, timestamp: u64) {
    let count = self.purged.iter()
        .take_while(|(_, ts)| *ts < timestamp)
        .count();
    self.purged.drain(..count);
}
```

Eski purged hash'leri temizler. `CrdsGossip::purge()` tarafından `5 * crds_timeout` gecikmesiyle çağrılır.

---

## crds_shards.rs

**Yol:** `gossip/src/crds_shards.rs`
**Satır:** 1–240

### CrdsShards Struct (satır 10–17)

```rust
#[derive(Clone)]
pub struct CrdsShards {
    shards: Vec<IndexMap<usize, u64>>,  // shards[k]: hash prefix=k olan değerlerin {index → hash64}
    shard_bits: u32,                     // 12 bit → 4096 shard
}
```

**Fikir:** Bloom filter mask'ının yüksek bitlerini shard index olarak kullan. `find(mask, mask_bits)` sorgusu, mask'a uyan shard(lar)ı doğrudan bulur.

### insert() / remove() (satır 27–35)

```rust
pub fn insert(&mut self, index: usize, value: &VersionedCrdsValue) -> bool {
    let hash = CrdsFilter::hash_as_u64(&value.value_hash);
    self.shard_mut(hash).insert(index, hash).is_none()
}

pub fn remove(&mut self, index: usize, value: &VersionedCrdsValue) -> bool {
    let hash = CrdsFilter::hash_as_u64(&value.value_hash);
    self.shard_mut(hash).swap_remove(&index).is_some()
}
```

### find() (satır 39–65)

```rust
pub fn find(&self, mask: u64, mask_bits: u32) -> impl Iterator<Item = usize> + '_
```

Üç durum:
- `shard_bits < mask_bits`: Tek shard, içinde filtre
- `shard_bits == mask_bits`: Tek shard, tüm içerik
- `shard_bits > mask_bits`: Birden fazla shard, hepsi döner

**Verimliliği:** 12-bit shard → 4096 shard. 8192 değer varsa her shard ~2 değer tutar. mask_bits=12 için sadece 1 elemana bak, mask_bits=0 için tüm değerleri tara.

### Iter Enum (satır 110–131)

```rust
enum Iter<R, S, T> {
    Less(R),
    Equal(S),
    Greater(T),
}
```

`Box<dyn Iterator>` yerine statik dispatch. Daha hızlı.

---

## crds_entry.rs

**Yol:** `gossip/src/crds_entry.rs`
**Satır:** 1–123

### CrdsEntry Trait (satır 18–21)

```rust
pub trait CrdsEntry<'a, 'b>: Sized {
    type Key;
    fn get_entry(table: &'a CrdsTable, key: Self::Key) -> Option<Self>;
}
```

### impl_crds_entry! Macro

İki form:
1. **CrdsValueLabel ile lookup:** `CrdsData`, `CrdsValue`, `VersionedCrdsValue`
2. **Pubkey ile lookup:** `LegacyContactInfo`, `LegacyVersion`, `LowestSlot`, `Version`, `SnapshotHashes`

Kullanım: `crds.get::<&LegacyContactInfo>(pubkey)` — Otomatik olarak `CrdsValueLabel::LegacyContactInfo(pubkey)` oluşturur ve `CrdsData::LegacyContactInfo(node)` pattern match eder.

---

## crds_gossip.rs

**Yol:** `gossip/src/crds_gossip.rs`
**Satır:** 1–491

### CrdsGossip Struct (satır 41–45)

```rust
#[derive(Default)]
pub struct CrdsGossip {
    pub crds: RwLock<Crds>,         // Thread-safe CRDS store
    pub push: CrdsGossipPush,       // Push protokol state
    pub pull: CrdsGossipPull,       // Pull protokol state
}
```

**RwLock stratejisi:** `crds` çok sık okunur (push mesaj üretimi, pull filter oluşturma) ama nadiren yazılır. RwLock okuma için kilit almaz.

### process_push_message() (satır 51–57)

```rust
pub fn process_push_message(
    &self,
    messages: Vec<(/*from:*/ Pubkey, Vec<CrdsValue>)>,
    now: u64,
) -> HashSet<Pubkey>
```

Push modülüne delege eder. Dönen set = yeni değerlerin sahiplerinin pubkey'leri (prune kararı için kullanılır).

### prune_received_cache() (satır 60–71)

```rust
pub fn prune_received_cache<I>(
    &self,
    self_pubkey: &Pubkey,
    origins: I,
    stakes: &HashMap<Pubkey, u64>,
) -> HashMap</*gossip peer:*/ Pubkey, /*origins:*/ Vec<Pubkey>>
```

Dönen map: `{gossip_peer → prune edilecek origin'ler}`. Bu, prune mesajlarının hangi peer'lere gönderileceğini belirler.

### push_duplicate_shred() (satır 85–149)

```rust
pub(crate) fn push_duplicate_shred<F>(
    &self,
    keypair: &Keypair,
    shred: &Shred,
    other_payload: &[u8],
    leader_schedule: Option<F>,
    max_payload_size: usize,
) -> Result<(), duplicate_shred::Error>
```

Algoritma:
1. Bu slot için duplicate shred zaten varsa early return
2. `duplicate_shred::from_shred()` ile kanıtı chunk'lara böl
3. Mevcut duplicate shred'lerin sayısını say
4. `MAX_DUPLICATE_SHREDS` sınırı varsa en eski index'i override et
5. Her chunk'ı `CrdsData::DuplicateShred(index, chunk)` olarak CRDS'e ekle

### process_prune_msg() (satır 152–171)

```rust
pub fn process_prune_msg(
    &self,
    self_pubkey: &Pubkey,
    peer: &Pubkey,
    destination: &Pubkey,
    origin: &[Pubkey],
    wallclock: u64,
    now: u64,
    stakes: &HashMap<Pubkey, u64>,
) -> Result<(), CrdsGossipError>
```

- `destination != self_pubkey` → `BadPruneDestination`
- `now > wallclock + prune_timeout` → `PruneMessageTimeout`
- Geçerliyse push.process_prune_msg() çağrılır

### purge() (satır 295–314)

```rust
pub fn purge(
    &self,
    self_pubkey: &Pubkey,
    thread_pool: &ThreadPool,
    now: u64,
    timeouts: &CrdsTimeouts,
) -> usize
```

1. `CrdsGossipPull::purge_active()` — Zaman aşımına uğrayan değerleri sil
2. `crds.trim_purged(now - 5*timeout)` — Eski purge hash'leri temizle
3. `pull.purge_failed_inserts(now)` — Eski failed insert hash'lerini temizle

### get_gossip_nodes() (satır 318–363)

```rust
pub(crate) fn get_gossip_nodes<R: Rng>(
    rng: &mut R,
    now: u64,
    pubkey: &Pubkey,
    verify_shred_version: impl Fn(u16) -> bool,
    crds: &RwLock<Crds>,
    gossip_validators: Option<&HashSet<Pubkey>>,
    stakes: &HashMap<Pubkey, u64>,
    socket_addr_space: &SocketAddrSpace,
) -> Vec<ContactInfo>
```

**Algoritma:**
- `ACTIVE_TIMEOUT = 60 saniye` — Son 60 saniyede aktif olmayan node'lar elenir
- Eclipse saldırı koruması: Stake'i olan node'lar, aktif olmasa bile `1/16` olasılıkla dahil edilir
- Kendi pubkey'i, farklı shred_version, geçersiz gossip adresi filtrelenir

### dedup_gossip_addresses() (satır 366–381)

Aynı gossip adresine sahip birden fazla node varsa, en yüksek stake'li olanı tutar.

---

## crds_gossip_push.rs

**Yol:** `gossip/src/crds_gossip_push.rs`
**Satır:** 1–622

**Teori:** Plumtree (Epidemic Broadcast Tree) protokolü referansı (satır 4): http://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf

### Sabitler (satır 45–53)

```rust
const CRDS_GOSSIP_PUSH_FANOUT: usize = 9;
// 2000 node için ~3.5 hop yeterli
pub const CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS: u64 = 30000;    // 30 saniye
const CRDS_GOSSIP_PRUNE_MSG_TIMEOUT_MS: u64 = 500;          // 0.5 saniye
const CRDS_GOSSIP_PRUNE_STAKE_THRESHOLD_PCT: f64 = 0.15;   // %15 stake eşiği
const CRDS_GOSSIP_PRUNE_MIN_INGRESS_NODES: usize = 2;       // Minimum 2 kaynak tut
const CRDS_GOSSIP_PUSH_ACTIVE_SET_SIZE: usize = CRDS_GOSSIP_PUSH_FANOUT + 3;  // 12
```

### CrdsGossipPush Struct (satır 55–72)

```rust
pub struct CrdsGossipPush {
    max_bytes: usize,                           // Paket başına max bayt (PACKET_DATA_SIZE * 64)
    active_set: RwLock<PushActiveSet>,          // Push hedef node'lar
    crds_cursor: Mutex<Cursor>,                 // CRDS'deki son işlenen konum
    received_cache: Mutex<ReceivedCache>,       // Push kaynakları ve skorları
    push_fanout: usize,                         // = 9
    pub(crate) msg_timeout: u64,                // = 30000 ms
    pub prune_timeout: u64,                     // = 500 ms
    pub num_total: AtomicUsize,                 // Metrik
    pub num_old: AtomicUsize,                   // Metrik
    pub num_pushes: AtomicUsize,                // Metrik
}
```

**Default:** `max_bytes = PACKET_DATA_SIZE * 64 = 65536 bayt`. Her push roundunda en fazla 64 paket değerinde veri.

### wallclock_window() (satır 123–125)

```rust
fn wallclock_window(&self, now: u64) -> impl RangeBounds<u64> {
    now.saturating_sub(self.msg_timeout)..=now.saturating_add(self.msg_timeout)
}
```

30 saniye öncesi ile 30 saniye sonrası arasındaki değerler kabul edilir. Max hop sayısı yoktur; wallclock timeout sistemi bunu ikame eder.

### process_push_message() — Push Alma (satır 130–164)

```rust
pub(crate) fn process_push_message(
    &self,
    crds: &RwLock<Crds>,
    messages: Vec<(/*from:*/ Pubkey, Vec<CrdsValue>)>,
    now: u64,
) -> HashSet<Pubkey>
```

**Algoritma:**
1. `received_cache` ve `crds` kilitlerini al
2. Her `(from, values)` çifti için:
   - Wallclock dışındaki değerleri atla
   - `crds.insert(value, now, GossipRoute::PushMessage(&from))`
   - **Başarı:** `received_cache.record(origin, from, 0)` (zamanlı teslimat)
   - **DuplicatePush(n):** `received_cache.record(origin, from, n)` (geç teslimat)
   - **InsertFailed:** `received_cache.record(origin, from, usize::MAX)` (anlamsız mesaj)
3. Başarıyla eklenen değerlerin `origin` pubkey'lerini döndür

### new_push_messages() — Push Gönderme (satır 172–220)

```rust
pub(crate) fn new_push_messages(
    &self,
    pubkey: &Pubkey,
    crds: &RwLock<Crds>,
    now: u64,
    stakes: &HashMap<Pubkey, u64>,
) -> (HashMap<Pubkey, Vec<CrdsValue>>, usize, usize)
```

**Algoritma:**
1. `active_set` ve `crds_cursor`'ı kilitle
2. `crds.get_entries(cursor)` — Cursor'dan yeni değerleri al
3. Her değer için:
   - `total_bytes += serialized_size(value)`; `> max_bytes` ise dur
   - `active_set.get_nodes(pubkey, &origin, should_force_push, stakes)` — Push hedeflerini al
   - `take(push_fanout)` ile en fazla 9 node'a ilet
   - `push_messages[node].push(value.clone())`
4. Döndür: `(push_messages, num_values, num_pushes)`

**Kritik:** `should_force_push(node)` → `value.should_force_push(node)`. NodeInstance değerleri her zaman iletilir (prune'dan muaf).

### process_prune_msg() (satır 222–232)

```rust
pub(crate) fn process_prune_msg(
    &self,
    self_pubkey: &Pubkey,
    peer: &Pubkey,
    origins: &[Pubkey],
    stakes: &HashMap<Pubkey, u64>,
)
```

`active_set.prune(self_pubkey, peer, origins, stakes)` çağrısı yapar. Artık peer'a, belirtilen origin'lerden gelen değerler push edilmez.

### refresh_push_active_set() (satır 235–284)

```rust
pub(crate) fn refresh_push_active_set(
    &self,
    crds: &RwLock<Crds>,
    stakes: &HashMap<Pubkey, u64>,
    gossip_validators: Option<&HashSet<Pubkey>>,
    self_keypair: &Keypair,
    self_shred_version: u16,
    ping_cache: &Mutex<PingCache>,
    pings: &mut Vec<(SocketAddr, Ping)>,
    socket_addr_space: &SocketAddrSpace,
)
```

**Algoritma:**
1. Aktif gossip node'larını bul (get_gossip_nodes, aynı shred version)
2. Ping/pong ile doğrula (maybe_ping_gossip_addresses)
3. Aynı gossip adreslerini tekilleştir (dedup_gossip_addresses)
4. `cluster_size = max(num_pubkeys, stakes.len())`
5. `active_set.rotate(rng, ACTIVE_SET_SIZE=12, cluster_size, nodes, stakes)`

---

## crds_gossip_pull.rs

**Yol:** `gossip/src/crds_gossip_pull.rs`
**Satır:** ~900

### Sabitler (satır 53–57)

```rust
pub const CRDS_GOSSIP_PULL_CRDS_TIMEOUT_MS: u64 = 15000;   // 15 saniye
const FAILED_INSERTS_RETENTION_MS: u64 = 20_000;             // 20 saniye
pub const FALSE_RATE: f64 = 0.1f64;    // %10 bloom filter false positive oranı
pub const KEYS: f64 = 8f64;            // Bloom filter hash fonksiyon sayısı
```

### CrdsFilter Struct (satır 59–64)

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct CrdsFilter {
    pub filter: Bloom<Hash>,    // Bloom filtresi
    mask: u64,                  // Hangi hash prefix alanını kapsıyor
    mask_bits: u32,             // Kaç bit prefix
}
```

**Nasıl Çalışır:** Tüm CRDS değerlerini tek bir bloom filter'a sığdırmak yerine, hash alanını `2^mask_bits` bölgeye böl. Her CrdsFilter sadece belirli bir bölgeyi kapsar.

Örnek: `mask_bits=3, mask=0b101` → Hash'in yüksek 3 biti `101` olan değerleri kapsar.

### CrdsFilter::compute_mask() (satır 99–103)

```rust
fn compute_mask(seed: u64, mask_bits: u32) -> u64 {
    let seed: u64 = seed.checked_shl(64 - mask_bits).unwrap_or(0x0);
    seed | (!0u64).checked_shr(mask_bits).unwrap_or(!0x0)
}
```

Yüksek `mask_bits` bit = seed, geri kalan bitler = 1. `test_mask()` içinde `hash | ones == mask` kontrolüyle hızlı filtreleme.

### CrdsFilter::max_items() (satır 104–109)

```rust
fn max_items(max_bits: f64, false_rate: f64, num_keys: f64) -> f64 {
    let m = max_bits;
    let p = false_rate;
    let k = num_keys;
    (m / (-k / (1f64 - (p.ln() / k).exp()).ln())).ceil()
}
```

Bloom filter kapasitesi formülü. `m` bit, `k` hash fonksiyonu, `p` false positive oranı ile kaç eleman saklanabilir.

### CrdsFilter::mask_bits() (satır 110–113)

```rust
fn mask_bits(num_items: f64, max_items: f64) -> u32 {
    ((num_items / max_items).log2().ceil()).max(0.0) as u32
}
```

Cluster büyüdükçe `mask_bits` artar → daha fazla parçalanmış bloom filter gönderilir.

### CrdsFilterSet Struct (satır 143–146)

```rust
struct CrdsFilterSet {
    filters: Vec<Option<ConcurrentBloom<Hash>>>,
    mask_bits: u32,
}
```

Pull request oluşturulurken: Tüm local CRDS değerleri bu set'e eklenir. Sonra Vec<CrdsFilter> dönüştürülür.

**Önemli Optimizasyon:** `SAMPLE_RATE=8` ile sadece her 8 filter'dan biri oluşturulur (en fazla MAX_NUM_FILTERS=1024). Geri kalanlar `None`. Bu, bandwidth tasarrufu sağlar.

### CrdsGossipPull Struct (satır 206–214)

```rust
pub struct CrdsGossipPull {
    failed_inserts: RwLock<VecDeque<(Hash, u64)>>,
    pub crds_timeout: u64,       // = 15000 ms
    pub num_pulls: AtomicUsize,
}
```

`failed_inserts`: Daha önce reddedilen pull response hash'leri. Bir sonraki pull request'te bloom filter'a eklenir → gönderen node aynı değeri tekrar göndermez.

### new_pull_request() (satır 228–286)

```rust
pub(crate) fn new_pull_request(
    &self,
    thread_pool: &ThreadPool,
    crds: &RwLock<Crds>,
    self_keypair: &Keypair,
    self_shred_version: u16,
    now: u64,
    gossip_validators: Option<&HashSet<Pubkey>>,
    stakes: &HashMap<Pubkey, u64>,
    bloom_size: usize,
    ping_cache: &Mutex<PingCache>,
    pings: &mut Vec<(SocketAddr, Ping)>,
    socket_addr_space: &SocketAddrSpace,
) -> Result<HashMap<ContactInfo, Vec<CrdsFilter>>, CrdsGossipError>
```

**Algoritma:**
1. Aktif gossip node'larını bul (spy node → herhangi shred version)
2. Ping/pong doğrulama
3. Stake ağırlıklı node seçimi:
   - `stake_cap = self.stake` (kendi stake'i ile sınırla)
   - `weight = log2(stake/SOL + 1)^2` — Düşük stake node'larla da bağlantı kurulur
4. `build_crds_filters()` ile bloom filter'lar oluştur
5. Her filter için ağırlıklı rastgele bir hedef node seç
6. Döndür: `{ContactInfo → [CrdsFilter]}`

**Stake Ağırlığı Formülü:**
```
stake = peer_stake.min(self_stake) / LAMPORTS_PER_SOL
weight = (u64::BITS - stake.leading_zeros() + 1)^2
```
Düşük stake node'lar da seçilir (weight ≥ 1).

### process_pull_requests() (satır 289–299)

```rust
pub(crate) fn process_pull_requests<I>(crds: &RwLock<Crds>, callers: I, now: u64)
where I: IntoIterator<Item = CrdsValue>
```

Pull request gelen node, kendi ContactInfo'sunu gönderir. Bu değer CRDS'e eklenir ve `update_record_timestamp()` ile timestamp güncellenir → bu node'un timeout'u sıfırlanır.

### generate_pull_responses() (satır 302–311)

```rust
pub(crate) fn generate_pull_responses(
    thread_pool: &ThreadPool,
    crds: &RwLock<Crds>,
    requests: &[(CrdsValue, CrdsFilter)],
    output_size_limit: usize,
    now: u64,
    stats: &GossipStats,
) -> Vec<Vec<CrdsValue>>
```

`filter_crds_values()` çağrısı yapar. Her filter için `filter_bitmask(mask, mask_bits)` ile uygun değerleri al, bloom filter içermeyen değerleri döndür.

### filter_crds_values() (satır 450–500)

**Algoritma:**
1. `caller_wallclock_window = now ± msg_timeout` — Eski callerların requestlerini atla
2. `jitter = random(0, msg_timeout/4)` — Senkron yanıt döngülerini kır
3. Paralel (rayon thread_pool):
   - Her `(caller, filter)` için → `crds.filter_bitmask(mask, mask_bits)`
   - `entry.wallclock ≤ caller_wallclock + jitter` filtresi (çok yeni değerleri atla)
   - `!filter.filter_contains(entry.value_hash)` (bloom filter içermiyorsa gönder)
   - `output_size_limit` ile sınırla

### filter_pull_responses() (satır 319–359)

```rust
pub(crate) fn filter_pull_responses(
    &self,
    crds: &RwLock<Crds>,
    timeouts: &CrdsTimeouts,
    responses: Vec<CrdsValue>,
    now: u64,
    stats: &mut ProcessPullStats,
) -> (Vec<CrdsValue>, Vec<CrdsValue>, Vec<Hash>)
```

Döndürür:
- `.0` — Aktif değerler (timeout geçmemiş, upsert edecek)
- `.1` — Süresi dolmuş ama owner'ın ContactInfo'su var (sessiz insert)
- `.2` — Reddedilen değerlerin hash'leri (failed_inserts'e eklenecek)

### process_pull_responses() (satır 362–394)

```rust
pub(crate) fn process_pull_responses(
    &self,
    crds: &RwLock<Crds>,
    responses: Vec<CrdsValue>,
    responses_expired_timeout: Vec<CrdsValue>,
    failed_inserts: Vec<Hash>,
    now: u64,
    stats: &mut ProcessPullStats,
)
```

1. Süresi dolmuş değerleri `PullResponse` route'uyla ekle (timestamp güncellenmez)
2. Aktif değerleri `PullResponse` route'uyla ekle
3. Başarılı owner'ların timestamp'lerini güncelle
4. `failed_inserts` listesini `self.failed_inserts` VecDeque'e ekle

### build_crds_filters() (satır 414–447)

```rust
pub fn build_crds_filters(
    &self,
    thread_pool: &ThreadPool,
    crds: &RwLock<Crds>,
    bloom_size: usize,
) -> Vec<CrdsFilter>
```

**Algoritma:**
1. `num_items = crds.len() + crds.num_purged() + failed_inserts.len()`
2. `MIN_NUM_BLOOM_ITEMS = 65536` (debug: 512)
3. `CrdsFilterSet::new(rng, num_items, bloom_size)`
4. Paralel olarak tüm `value_hash`, `purged hash`, `failed_insert hash`'leri bloom'a ekle
5. `CrdsFilterSet → Vec<CrdsFilter>` dönüştür

**Neden purged ve failed_inserts de ekleniyor?**
- `purged`: Önceden var olan ama güncellenen değerlerin eski hash'leri. Diğer node bu eski versiyonu göndermesin diye filtrelenir.
- `failed_inserts`: Reddedilen değerler. Sürekli aynı şeyi isteme döngüsünü kır.

### CrdsTimeouts (satır ~)

```rust
pub struct CrdsTimeouts<'a> {
    self_pubkey: Pubkey,
    stakes: &'a HashMap<Pubkey, u64>,
    default: u64,
    extended: u64,
}

impl Index<&Pubkey> for CrdsTimeouts<'_> {
    type Output = u64;
    // self_pubkey → u64::MAX (hiçbir zaman silinme)
    // staked node → extended timeout
    // unstaked → default timeout
}
```

`make_timeouts()` → Bu struct'ı oluşturur.

---

## push_active_set.rs

**Yol:** `gossip/src/push_active_set.rs`
**Satır:** 1–320

### PushActiveSet Struct (satır 17)

```rust
#[derive(Default)]
pub(crate) struct PushActiveSet([PushActiveSetEntry; NUM_PUSH_ACTIVE_SET_ENTRIES]);
// NUM_PUSH_ACTIVE_SET_ENTRIES = 25
```

**Fikir:** 25 stake bucket'ı. Her bucket, o stake seviyesindeki değerlerin hangi node'lara push edileceğini tutar. Düşük stake değerleri düşük bucket'ta, yüksek stake değerleri yüksek bucket'ta.

### PushActiveSetEntry Struct (satır 21–22)

```rust
#[derive(Default)]
struct PushActiveSetEntry(IndexMap</*node:*/ Pubkey, /*origins:*/ ConcurrentBloom<Pubkey>>);
```

Her node için: "Bu node'un bana hangi origin'leri prune ettiğini" tutan bloom filter.

### Sabitler (satır 25–28)

```rust
const BLOOM_FALSE_RATE: f64 = 0.1;
const BLOOM_MAX_BITS: usize = 1024 * 8 * 4;  // 32KB
```

### get_stake_bucket() (satır 170–174)

```rust
fn get_stake_bucket(stake: Option<&u64>) -> usize {
    let stake = stake.copied().unwrap_or_default() / LAMPORTS_PER_SOL;
    let bucket = u64::BITS - stake.leading_zeros();
    (bucket as usize).min(NUM_PUSH_ACTIVE_SET_ENTRIES - 1)
}
```

Stake → bit sayısı → bucket index. Örnek: 1 SOL → bucket 1, 4 SOL → bucket 2, 8 SOL → bucket 3.

### PushActiveSet::get_nodes() (satır 30–40)

```rust
pub(crate) fn get_nodes<'a>(
    &'a self,
    pubkey: &Pubkey,
    origin: &'a Pubkey,
    should_force_push: impl FnMut(&Pubkey) -> bool + 'a,
    stakes: &HashMap<Pubkey, u64>,
) -> impl Iterator<Item = &Pubkey> + 'a
{
    let stake = stakes.get(pubkey).min(stakes.get(origin));
    self.get_entry(stake).get_nodes(origin, should_force_push)
}
```

`min(this_node_stake, origin_stake)` ile bucket belirlenir. Bu, her iki tarafın da stake'ini dikkate alır.

### PushActiveSetEntry::get_nodes() (satır 111–123)

```rust
fn get_nodes<'a>(
    &'a self,
    origin: &'a Pubkey,
    mut should_force_push: impl FnMut(&Pubkey) -> bool + 'a,
) -> impl Iterator<Item = &Pubkey> + 'a
{
    self.0.iter()
        .filter(move |(node, bloom_filter)| {
            !bloom_filter.contains(origin) || should_force_push(node)
        })
        .map(|(node, _bloom_filter)| node)
}
```

`bloom_filter.contains(origin)` → Bu node, bu origin'i prune etmiş → atla.
`should_force_push(node)` → NodeInstance ise her zaman ilet.

### PushActiveSetEntry::prune() (satır 125–133)

```rust
fn prune(&self, node: &Pubkey, origin: &Pubkey) {
    if let Some(bloom_filter) = self.0.get(node) {
        bloom_filter.add(origin);
    }
}
```

`ConcurrentBloom` — Thread-safe, iç içe kilit gerektirmez.

### rotate() (satır 61–100 ve 135–166)

**PushActiveSet::rotate():**
- 25 bucket için ayrı ayrı ağırlık hesapla
- Her node'un stake bucket'ı ile kendi bucket'ı arasında `min(node_bucket, k)` alınır
- Ağırlık = `(min_bucket + 1)^2`
- `PushActiveSetEntry::rotate()` ile her bucket güncellenir

**PushActiveSetEntry::rotate():**
```rust
fn rotate<R: Rng>(
    &mut self,
    rng: &mut R,
    size: usize,
    num_bloom_filter_items: usize,
    nodes: &[Pubkey],
    weights: &[u64],
)
```
1. `WeightedShuffle::new(weights).shuffle(rng)` ile ağırlıklı karıştır
2. Yeni node'ları ekle (bloom filter oluştur, node'u kendi bloom'una ekle)
3. Toplam `size`'ı aşınca dur
4. En eski girişleri `shift_remove_index(0)` ile sil

**Önemli:** Her node kendi bloom filter'ına eklenmiştir (başlangıçta). Bu sayede `get_nodes()` hiçbir zaman kendine push yapmaz.

---

## received_cache.rs

**Yol:** `gossip/src/received_cache.rs`
**Satır:** 1–195

### ReceivedCache Struct (satır 10)

```rust
pub(crate) struct ReceivedCache(LruCache</*origin/owner:*/ Pubkey, ReceivedCacheEntry>);
```

LRU cache: En fazla `2 * CRDS_UNIQUE_PUBKEY_CAPACITY = 16384` origin.

### ReceivedCacheEntry Struct (satır 12–16)

```rust
#[derive(Clone, Default)]
struct ReceivedCacheEntry {
    nodes: HashMap<Pubkey, /*score:*/ usize>,
    num_upserts: usize,
}
```

Her origin için: Bu origin'in değerlerini gönderen node'lar ve onların skorları.

### Sabitler

```rust
const MIN_NUM_UPSERTS: usize = 20;   // Prune kararı için minimum veri
const CAPACITY: usize = 50;           // Kaynak node başına max kayıt
const NUM_DUPS_THRESHOLD: usize = 2;  // ≤2. kopyada zamanlı sayılır
```

### record() (satır 76–91)

```rust
fn record(&mut self, node: Pubkey, num_dups: usize) {
    if num_dups == 0 {
        self.num_upserts = self.num_upserts.saturating_add(1);
    }
    if num_dups < Self::NUM_DUPS_THRESHOLD {
        let score = self.nodes.entry(node).or_default();
        *score = score.saturating_add(1);
    } else if self.nodes.len() < Self::CAPACITY {
        let _ = self.nodes.entry(node).or_default();
    }
}
```

- `num_dups=0`: Tamamen yeni, ilk alan → en yüksek skor
- `num_dups<2`: 1. kopya, hâlâ zamanlı → skor artır
- `num_dups≥2`: Geç teslimat → skor artırma ama node'u kayıt altına al

**Neden num_dups≥2 de skor artırılmıyor?**
Sahte saldırıları önlemek için: Saldırgan başka node'ların adreslerini kullanarak geç duplicate mesajlar gönderebilir. Eğer bu da skoru düşürürse, iyi node'lar yanlışlıkla prune edilebilir.

### prune() (satır 37–56)

```rust
pub(crate) fn prune(
    &mut self,
    pubkey: &Pubkey,
    origin: Pubkey,
    stake_threshold: f64,
    min_ingress_nodes: usize,
    stakes: &HashMap<Pubkey, u64>,
) -> impl Iterator<Item = Pubkey>
```

Koşullar:
- `num_upserts < MIN_NUM_UPSERTS (20)`: Henüz yeterli veri yok → prune etme
- Yeterli veri varsa: `ReceivedCacheEntry::prune()` çağır

### ReceivedCacheEntry::prune() (satır 93–124)

**Algoritma:**
1. `min_ingress_stake = min(self.stake, origin.stake) * stake_threshold`
2. Her node'u (score, stake) ile sırala (score azalan, stake azalan)
3. İlk `min_ingress_nodes (2)` node'u tut → bu, minimum 2 iyi kaynak tutar
4. Birikimli stake `min_ingress_stake`'i geçene kadar daha fazla tut
5. Geri kalan node'lar prune edilecek

**Sonuç:** En iyi 2+ kaynak node korunur (prune edilmez), geri kalanlar prune mesajıyla bildirilir.

---

## weighted_shuffle.rs

**Yol:** `gossip/src/weighted_shuffle.rs`
**Satır:** 1–361

### WeightedShuffle Struct (satır 20–23)

```rust
#[derive(Clone)]
pub struct WeightedShuffle<T> {
    arr: Vec<T>,       // Fenwick tree (Binary Indexed Tree)
    sum: T,            // Toplam ağırlık
    zeros: Vec<usize>, // Sıfır ağırlıklı indeksler
}
```

**Binary Indexed Tree (Fenwick Tree)** kullanımı — Kümülatif toplamı O(log n)'de sorgula, O(log n)'de güncelle.

### new() (satır 36–77)

Fenwick tree oluşturur. Negatif veya taşan ağırlıklar zeros listesine eklenir, uyarı verilir.

### cumsum() (satır 84–91)

```rust
fn cumsum(&self, mut k: usize) -> T {
    let mut out = T::default();
    while k != 0 {
        out += self.arr[k];
        k ^= k & k.wrapping_neg();  // LSB sıfırlama
    }
    out
}
```

Fenwick tree standart prefix sum sorgusu.

### search() (satır 105–123)

Binary search ile `cumsum(k) > val` olan en küçük k'yı bul. O(log^2 n).

### remove() (satır 94–101)

```rust
fn remove(&mut self, mut k: usize, weight: T) {
    self.sum -= weight;
    let size = self.arr.len();
    while k < size {
        self.arr[k] -= weight;
        k += k & k.wrapping_neg();
    }
}
```

Fenwick tree point update.

### shuffle() (satır 160–177)

```rust
pub fn shuffle<R: Rng>(mut self, rng: &'a mut R) -> impl Iterator<Item = usize> + 'a
```

Iterator olarak döner. Her `next()` çağrısında:
1. Toplam ağırlık > 0 ise: `sample = random(0, sum)`, `search(sample)` → indeks bul, `remove(indeks)` → ağırlığı çıkar
2. Zeros varsa: random zeros listesinden al

**Algoritma Karmaşıklığı:** Her adım O(log^2 n). Tam karıştırma O(n log^2 n).

**Test Coverage:**
- `test_weighted_shuffle_empty_weights` — Boş ağırlıklar
- `test_weighted_shuffle_zero_weights` — Tüm sıfır ağırlıklar
- `test_weighted_shuffle_sanity` — 100000 deneme ile olasılık doğrulaması
- `test_weighted_shuffle_negative_overflow` — Negatif/taşan ağırlıklar sıfır gibi davranır
- `test_weighted_shuffle_match_slow` — 997 elemanlı yavaş referans ile karşılaştırma

---

## cluster_info.rs

**Yol:** `gossip/src/cluster_info.rs`
**Satır:** ~4000

### Sabitler (satır 106–145)

```rust
pub const GOSSIP_SLEEP_MILLIS: u64 = 100;           // Gossip döngüsü uykusu
pub const MAX_BLOOM_SIZE: usize = MAX_CRDS_OBJECT_SIZE;  // = 928 bayt
pub const MAX_CRDS_OBJECT_SIZE: usize = 928;
const MAX_GOSSIP_TRAFFIC: usize = 128_000_000 / PACKET_DATA_SIZE;  // 128MB/~1500 = ~87000 paket
const PUSH_MESSAGE_MAX_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE - 44;
pub(crate) const DUPLICATE_SHRED_MAX_PAYLOAD_SIZE: usize = PACKET_DATA_SIZE - 115;
pub const MAX_ACCOUNTS_HASHES: usize = 16;
pub const MAX_INCREMENTAL_SNAPSHOT_HASHES: usize = 25;
const MAX_PRUNE_DATA_NODES: usize = 32;
const GOSSIP_PING_TOKEN_SIZE: usize = 32;
const GOSSIP_PING_CACHE_CAPACITY: usize = 65536;
const GOSSIP_PING_CACHE_TTL: Duration = Duration::from_secs(1280);
const GOSSIP_PING_CACHE_RATE_LIMIT_DELAY: Duration = Duration::from_secs(1280 / 64);  // 20 saniye
pub(crate) const CRDS_UNIQUE_PUBKEY_CAPACITY: usize = 8192;
const MIN_STAKE_FOR_GOSSIP: u64 = LAMPORTS_PER_SOL;     // 1 SOL minimum stake
const MIN_NUM_STAKED_NODES: usize = 500;                 // Stake bazlı filtreleme için minimum
```

### ClusterInfo Struct (satır 161–179)

```rust
pub struct ClusterInfo {
    pub gossip: CrdsGossip,
    keypair: RwLock<Arc<Keypair>>,
    entrypoints: RwLock<Vec<LegacyContactInfo>>,
    outbound_budget: DataBudget,
    my_contact_info: RwLock<ContactInfo>,
    ping_cache: Mutex<PingCache>,
    stats: GossipStats,
    socket: UdpSocket,
    local_message_pending_push_queue: Mutex<Vec<CrdsValue>>,
    contact_debug_interval: u64,
    contact_save_interval: u64,
    instance: RwLock<NodeInstance>,
    contact_info_path: PathBuf,
    socket_addr_space: SocketAddrSpace,
}
```

### ClusterInfoError Enum (satır 147–159)

```rust
pub enum ClusterInfoError {
    NoPeers,
    NoLeader,
    BadContactInfo,
    BadGossipAddress,
    TooManyIncrementalSnapshotHashes,
}
```

### PruneData Struct (satır 181–193)

```rust
pub(crate) struct PruneData {
    pubkey: Pubkey,          // Gönderenin pubkey'i
    prunes: Vec<Pubkey>,     // Prune edilecek origin'ler (max 32)
    signature: Signature,    // Ed25519 imzası
    destination: Pubkey,     // Hedef node
    wallclock: u64,
}
```

`Signable` impl: `pubkey + prunes + destination + wallclock` imzalanır (ama signature dahil değil).

### Protocol Enum (satır 266–279)

```rust
#[frozen_abi(digest = "ogEqvffeEkPpojAaSiUbCv2HdJcdXDQ1ykgYyvKvLo2")]
pub(crate) enum Protocol {
    PullRequest(CrdsFilter, CrdsValue),
    PullResponse(Pubkey, Vec<CrdsValue>),
    PushMessage(Pubkey, Vec<CrdsValue>),
    PruneMessage(Pubkey, PruneData),
    PingMessage(Ping),
    PongMessage(Pong),
}
```

**ABI frozen:** Bu enum'un değişmesi protokol kırılmasına yol açar. `#[frozen_abi]` ile ABI hash'i sabitlenmiştir.

### Protocol::par_verify() (satır 282–300)

```rust
fn par_verify(self, stats: &GossipStats) -> Option<Self>
```

PullResponse için paralel imza doğrulama. Geçersiz imzalı değerler çıkarılır.

### Thread Mimarisi (gossip_service.rs satır 56–99)

```
GossipService::new() oluşturduğu thread'ler:
  t_receiver      ← UDP socket'ten paket al
  t_socket_consume ← Paketleri parse et
  t_listen        ← Gossip protokol işleme (listen loop)
  t_gossip        ← Gossip gönderme (transmit loop)
  t_responder     ← UDP socket'e cevap gönder
```

**Listen Loop (cluster_info.rs):**
```
Gelen paket → Protocol parse → par_verify → match:
  PullRequest → process_pull_requests() + generate_pull_responses()
  PullResponse → filter_pull_responses() + process_pull_responses() + prune
  PushMessage → process_push_message() + prune
  PruneMessage → process_prune_msg()
  PingMessage → pong gönder
  PongMessage → ping_cache.add()
```

**Transmit Loop (cluster_info.rs):**
```
Her 100ms:
  refresh_push_active_set() (periyodik)
  new_pull_request() → pull request gönder
  new_push_messages() → push mesajları gönder
  PruneData oluştur ve gönder
```

---

## contact_info.rs

**Yol:** `gossip/src/contact_info.rs`
**Satır:** 1–964

### Socket Etiketleri (satır 28–41)

```rust
const SOCKET_TAG_GOSSIP: u8 = 0;
const SOCKET_TAG_RPC: u8 = 2;
const SOCKET_TAG_RPC_PUBSUB: u8 = 3;
const SOCKET_TAG_SERVE_REPAIR: u8 = 4;
const SOCKET_TAG_SERVE_REPAIR_QUIC: u8 = 1;
const SOCKET_TAG_TPU: u8 = 5;
const SOCKET_TAG_TPU_FORWARDS: u8 = 6;
const SOCKET_TAG_TPU_FORWARDS_QUIC: u8 = 7;
const SOCKET_TAG_TPU_QUIC: u8 = 8;
const SOCKET_TAG_TPU_VOTE: u8 = 9;
const SOCKET_TAG_TVU: u8 = 10;
const SOCKET_TAG_TVU_QUIC: u8 = 11;
const SOCKET_CACHE_SIZE: usize = 12;
```

### ContactInfo Struct (satır 69–89)

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContactInfo {
    pubkey: Pubkey,
    #[serde(with = "serde_varint")]
    wallclock: u64,
    outset: u64,          // Node instance'ın başlangıç zamanı (microsecond)
    shred_version: u16,
    version: solana_version::Version,
    #[serde(with = "short_vec")]
    addrs: Vec<IpAddr>,   // Benzersiz IP adresleri (max 255)
    #[serde(with = "short_vec")]
    sockets: Vec<SocketEntry>,  // Port offsetleri
    #[serde(with = "short_vec")]
    extensions: Vec<Extension>,  // Şimdilik boş
    #[serde(skip_serializing)]
    cache: [SocketAddr; SOCKET_CACHE_SIZE],  // Hızlı erişim cache'i
}
```

**Optimize Seri Format:**
- `addrs`: IP adresleri tekrar etmez. Her socket sadece addrs içindeki index'i referans alır.
- `sockets`: `{key, index, offset}` üçlüsü. Port = önceki portların offset toplamı + bu offset. VarInt encoding ile küçük offsetler az bayt kaplar.
- `cache`: Deserialize sırasında doldurulur, O(1) socket erişimi sağlar.

### SocketEntry Struct (satır 91–97)

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq, AbiExample, Deserialize, Serialize)]
struct SocketEntry {
    key: u8,               // Socket tipi (GOSSIP=0, TPU=5, vb.)
    index: u8,             // addrs içindeki IP adresi index'i
    #[serde(with = "serde_varint")]
    offset: u16,           // Port offseti
}
```

### set_socket() (satır 303–336)

**Algoritma:**
1. Mevcut aynı key'li socket'i sil
2. Yeni socket için port offset hesapla (önceki portların toplamından)
3. Sıralı konumda insert et, sonraki socket'in offset'ini ayarla
4. `push_addr()` ile IP'yi addrs'e ekle (yoksa)
5. `cache[key] = socket`

### sanitize_entries() (satır 491–535)

Doğrular:
- Benzersiz IP adresleri
- Benzersiz socket key'leri (256-bit bitmask ile O(1))
- Her socket geçerli IP index'i referans alıyor
- Kullanılmayan IP adresi yok
- Port offset'leri taşmıyor

### QUIC socket desteği (satır 539–557)

```rust
pub(crate) fn get_quic_socket(socket: &SocketAddr) -> Result<SocketAddr, Error> {
    Ok(SocketAddr::new(
        socket.ip(),
        socket.port().checked_add(QUIC_PORT_OFFSET)?,
    ))
}
```

TPU ve diğer socket'ler için QUIC = UDP_port + QUIC_PORT_OFFSET.

---

## legacy_contact_info.rs

**Yol:** `gossip/src/legacy_contact_info.rs`
**Satır:** ~400

### LegacyContactInfo Struct (satır 22–48)

```rust
pub struct LegacyContactInfo {
    id: Pubkey,
    gossip: SocketAddr,
    tvu: SocketAddr,
    tvu_quic: SocketAddr,
    serve_repair_quic: SocketAddr,
    tpu: SocketAddr,
    tpu_forwards: SocketAddr,
    tpu_vote: SocketAddr,
    rpc: SocketAddr,
    rpc_pubsub: SocketAddr,
    serve_repair: SocketAddr,
    wallclock: u64,
    shred_version: u16,
}
```

Her socket ayrı alan olarak saklanır. Yeni `ContactInfo`'dan çok daha az verimli seri format. Geriye dönük uyumluluk için korunuyor.

Macro'lar: `get_socket!`, `set_socket!`, `socketaddr!`, `socketaddr_any!`

---

## ping_pong.rs

**Yol:** `gossip/src/ping_pong.rs`
**Satır:** 1–412

### Ping Struct (satır 22–26)

```rust
pub struct Ping<T> {
    from: Pubkey,
    token: T,         // Rastgele 32 baytlık token
    signature: Signature,
}
```

### Pong Struct (satır 28–33)

```rust
pub struct Pong {
    from: Pubkey,
    hash: Hash,       // Hash(PING_PONG_HASH_PREFIX || ping.token)
    signature: Signature,
}
```

**Protokol:**
1. Ping: `token = random`, `signature = sign(serialize(token))`
2. Pong: `hash = hashv([PREFIX, serialize(token)])`, `signature = sign(hash)`
3. Doğrulama: `pending_cache[pong.hash] == (pong.pubkey, socket)`

### PingCache Struct (satır 38–51)

```rust
pub struct PingCache {
    ttl: Duration,                                      // Pong geçerlilik süresi (1280 saniye ~21 dakika)
    rate_limit_delay: Duration,                         // Ping gönderme aralığı (20 saniye)
    pings: LruCache<(Pubkey, SocketAddr), Instant>,    // Son ping zamanı
    pongs: LruCache<(Pubkey, SocketAddr), Instant>,    // Son geçerli pong zamanı
    pending_cache: LruCache<Hash, (Pubkey, SocketAddr)>, // Bekleyen ping token hash'leri
}
```

### check() (satır 214–243)

```rust
pub fn check<T, F>(
    &mut self,
    now: Instant,
    node: (Pubkey, SocketAddr),
    pingf: F,
) -> (bool, Option<Ping<T>>)
```

- Pong yok → `(false, Some(ping))` (yeni ping gönder)
- Pong var, yeni → `(true, None)` (geçerli, ping gerekmez)
- Pong var, eski (>ttl/8) → `(true, Some(ping))` (yenile)
- Pong süresi dolmuş → `(false, Some(ping))` (yeniden doğrula)

**rate_limit_delay (≤ ttl/2)**: Aynı node'a çok sık ping gönderilmez.

---

## epoch_slots.rs

**Yol:** `gossip/src/epoch_slots.rs`
**Satır:** 1–548

### Sıkıştırma Tipleri

**Uncompressed (satır 17–22):**
```rust
pub struct Uncompressed {
    pub first_slot: Slot,
    pub num: usize,
    pub slots: BitVec<u8>,   // Bit array
}
```

**Flate2 (satır 46–51):**
```rust
pub struct Flate2 {
    pub first_slot: Slot,
    pub num: usize,
    pub compressed: Vec<u8>,
}
```

### CompressedSlots Enum (satır 159–163)

```rust
pub enum CompressedSlots {
    Flate2(Flate2),
    Uncompressed(Uncompressed),
}
```

### EpochSlots Struct (satır 227–232)

```rust
pub struct EpochSlots {
    pub from: Pubkey,
    pub slots: Vec<CompressedSlots>,
    pub wallclock: u64,
}
```

Birden fazla `CompressedSlots` chunk içerebilir (seyrek slot dağılımı için).

### fill() (satır 269–288)

```rust
pub fn fill(&mut self, slots: &[Slot], now: u64) -> usize
```

Algoritma:
1. Slot'ları `add()` ile mevcut chunk'a ekle
2. Sığmazsa: `deflate()` ile sıkıştır, yeni chunk ekle
3. `max_compressed_slot_size()` pozitif olduğu sürece devam et

### max_compressed_slot_size() (satır 305–309)

```rust
pub fn max_compressed_slot_size(&self) -> isize {
    let len_header = serialized_size(self).unwrap();
    let len_slot = serialized_size(&CompressedSlots::default()).unwrap();
    MAX_CRDS_OBJECT_SIZE as isize - (len_header + len_slot) as isize
}
```

CRDS nesne boyutu sınırına göre kalan alanı hesaplar.

---

## duplicate_shred.rs

**Yol:** `gossip/src/duplicate_shred.rs`
**Satır:** 1–1140

### DuplicateShred Struct (satır 28–39)

```rust
pub struct DuplicateShred {
    pub(crate) from: Pubkey,
    pub(crate) wallclock: u64,
    pub(crate) slot: Slot,
    _unused: u32,
    _unused_shred_type: ShredType,
    num_chunks: u8,        // Toplam chunk sayısı
    chunk_index: u8,       // Bu chunk'ın sırası
    #[serde(with = "serde_bytes")]
    chunk: Vec<u8>,        // Gerçek veri
}
```

`DUPLICATE_SHRED_HEADER_SIZE = 63` bayt.

### check_shreds() (satır 103–161)

Geçerli çift shred kanıtı kontrolü:
- Aynı slot
- İmza doğrulaması (leader schedule ile)
- Merkle root çakışması → kanıt
- Aynı type, farklı index: data → last_in_slot çakışması, coding → erasure meta çakışması

### from_shred() (satır 163–206)

```rust
pub(crate) fn from_shred<F>(
    shred: Shred,
    self_pubkey: Pubkey,
    other_payload: Vec<u8>,
    leader_schedule: Option<F>,
    wallclock: u64,
    max_size: usize,
) -> Result<impl Iterator<Item = DuplicateShred>, Error>
```

`DuplicateSlotProof {shred1, shred2}` → bincode serialize → chunk'lara böl → DuplicateShred iteratörü.

### into_shreds() (satır 228–272)

Chunk'ları birleştir → `DuplicateSlotProof` deserialize → shred'leri reconstruct et → doğrula.

---

## duplicate_shred_handler.rs

**Yol:** `gossip/src/duplicate_shred_handler.rs`
**Satır:** ~200

### DuplicateShredHandler Struct (satır 31–51)

```rust
pub struct DuplicateShredHandler {
    buffer: HashMap<(Slot, Pubkey), BufferEntry>,   // Gelen chunk'lar
    consumed: HashMap<Slot, bool>,                   // İşlenen slotlar
    last_root: Slot,
    blockstore: Arc<Blockstore>,
    leader_schedule_cache: Arc<LeaderScheduleCache>,
    bank_forks: Arc<RwLock<BankForks>>,
    cached_on_epoch: Epoch,
    cached_staked_nodes: Arc<HashMap<Pubkey, u64>>,
    cached_slots_in_epoch: u64,
    duplicate_slots_sender: Sender<Slot>,
}
```

**Sınırlar:**
- `MAX_NUM_CHUNKS = 3` — 3'ten fazla chunk → geçersiz
- `MAX_NUM_ENTRIES_PER_PUBKEY = 128` — Node başına max giriş
- `BUFFER_CAPACITY = 512 * 128 = 65536`

### handle_shred_data() (satır 105–149)

1. Slot zaten işlendi mi? Atla
2. chunk geçerli mi? (MAX_NUM_CHUNKS, index sınırı)
3. Buffer'a ekle
4. Tüm chunk'lar geldi mi? (`count == num_chunks`)
5. Leader pubkey'i bul
6. `into_shreds()` ile reconstruct et
7. Blockstore'a kaydet
8. `duplicate_slots_sender.send(slot)` — Consensus state machine'e bildir

---

## restart_crds_values.rs

**Yol:** `gossip/src/restart_crds_values.rs`
**Satır:** 1–371

### RestartLastVotedForkSlots Struct (satır 16–24)

```rust
pub struct RestartLastVotedForkSlots {
    pub from: Pubkey,
    pub wallclock: u64,
    offsets: SlotsOffsets,          // Sıkıştırılmış slot listesi
    pub last_voted_slot: Slot,
    pub last_voted_hash: Hash,
    pub shred_version: u16,
}
```

**Amaç:** `wen_restart` protokolü. Yeniden başlatma koordinasyonu için validator'ların son oy geçmişlerini paylaşması.

### SlotsOffsets Enum (satır 42–46)

```rust
enum SlotsOffsets {
    RunLengthEncoding(RunLengthEncoding),
    RawOffsets(RawOffsets),
}
```

`MAX_BYTES = 824` (MAX_CRDS_OBJECT_SIZE - header). Veriye göre sıkıştırma yöntemi seçilir.

### RestartHeaviestFork Struct (satır 32–40)

```rust
pub struct RestartHeaviestFork {
    pub from: Pubkey,
    pub wallclock: u64,
    pub last_slot: Slot,
    pub last_slot_hash: Hash,
    pub observed_stake: u64,      // Bu fork'u destekleyen stake miktarı
    pub shred_version: u16,
}
```

---

## cluster_info_metrics.rs

**Yol:** `gossip/src/cluster_info_metrics.rs`
**Satır:** ~300

### Counter Struct (satır 16)

```rust
#[derive(Default)]
pub(crate) struct Counter(AtomicU64);
```

`AtomicU64` üzerine `add_measure()`, `add_relaxed()`, `clear()` metotları.

### GossipStats Struct (satır 91–)

50+ Counter alanı. Örnekler:
- `gossip_pull_request_verify_fail`
- `gossip_push_msg_verify_fail`
- `handle_batch_push_messages_time`
- `process_pull_response_count`
- `packets_received_count`

### TimedGuard ve ScopedTimer

`TimedGuard<T>`: `Drop` implementasyonu ile otomatik zaman ölçümü. Guard'ı bırakınca counter güncellenir.

---

## crds_gossip_error.rs

**Yol:** `gossip/src/crds_gossip_error.rs`

```rust
#[derive(PartialEq, Eq, Debug)]
pub enum CrdsGossipError {
    NoPeers,
    PushMessageTimeout,
    PushMessageOldVersion,
    BadPruneDestination,
    PruneMessageTimeout,
}
```

---

## gossip_error.rs

**Yol:** `gossip/src/gossip_error.rs`

```rust
pub enum GossipError {
    DuplicateNodeInstance,
    DuplicateShredError(#[from] duplicate_shred::Error),
    InvalidContactInfo(#[from] contact_info::Error),
    Io(#[from] io::Error),
    RecvTimeoutError(#[from] RecvTimeoutError),
    SendError,
    Serialize(#[from] Box<bincode::ErrorKind>),
}
```

---

## deprecated.rs

**Yol:** `gossip/src/deprecated.rs`

```rust
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, AbiExample)]
pub(crate) struct EpochIncompleteSlots {
    first: Slot,
    compression: CompressionType,
    compressed_list: Vec<u8>,
}

enum CompressionType {
    Uncompressed,
    GZip,
    BZip2,
}
```

Eski `LowestSlot` yapısının bileşeni. Yeni insert'lerde `stash` alanı boş olmalı.

---

## lib.rs ve main.rs

### lib.rs

26 modülün tamamı dışa açılmış. `push_active_set` ve `received_cache` `pub(crate)` (internal).

`#[macro_use]` ile `log`, `assert_matches`, `serde_derive`, `solana_frozen_abi_macro`, `solana_metrics` dahil edilmiş.

### main.rs

CLI aracı: `solana-gossip` komutu.

Alt komutlar:
- `spy`: Gossip ağına bağlan, node'ları izle, belirli node'ları bekle
- `rpc-url`: Cluster'dan RPC URL'i bul

`make_gossip_node()` → spy node oluşturur (gossip adresi opsiyonel).

---

## Nova Shield Chain için Dersler

### 1. CRDS Veri Modeli

**Solana Tasarımı:** `CrdsValueLabel → VersionedCrdsValue` tek bir IndexMap.
- Label = (tip, pubkey, index) — Birden fazla aynı tip değer, farklı index ile var olabilir.
- Tüm erişim yapıları (nodes, votes, entries, records) ayrı index yapıları.

**Nova Shield Chain için:**
```
P2PRecord {
    label: RecordLabel,           // (tip, node_id, index)
    value: SignedValue,           // Ed25519 imzalı veri
    ordinal: u64,                 // Insert sırası
    local_timestamp: u64,
    value_hash: Hash,             // Seri format hash'i
}
```

Merge stratejisi: wallclock karşılaştırması → hash tiebreak (cluster genelinde deterministik).

### 2. Push Protokolü (Plumtree Benzeri)

**Fanout = 9:** 2000 node için ~3.5 hop yeterli. Matematiksel argüman: `log(2000)/log(9) ≈ 3.5`.

**Önerilen Uygulama:**
- Push fanout: 7-10 arası
- Push timeout: 30 saniye
- Wallclock window: ±msg_timeout
- Prune timeout: 500ms (prune mesajı 500ms içinde gelmezse geçersiz)

**Prune Mekanizması:**
1. Her push'u alan node, göndereni `received_cache`'e kaydet + skor ver
2. Çok kaynaktan geliyorsa: En iyi 2+ kaynağı tut, diğerlerine prune gönder
3. Prune mesajı `{destination, origins[]}` formatında, imzalı

**Stake Bazlı Prune Eşiği:**
```
min_ingress_stake = min(kendi_stake, origin_stake) * 0.15
```
Düşük stake node'lar, yüksek stake node'ları prune etmekte daha kısıtlı.

### 3. Pull Protokolü (Anti-Entropy)

**Bloom Filter Tasarımı:**
- False rate: %10
- Hash fonksiyonu sayısı: 8
- Bloom filter'ı parçala: `mask_bits = ceil(log2(N/max_per_filter))`
- Her istek ayrı filter ile farklı false positive dağılımı

**Önerilen Implementasyon:**
```rust
struct PullRequest {
    caller: SignedValue,        // Gönderenin ContactInfo'su
    filter: BloomFilter,        // Mevcut bilinen değerlerin hash'leri
    mask: u64,                  // Hangi hash alanını sorguluyor
    mask_bits: u32,
}

struct PullResponse {
    from: NodeId,
    values: Vec<SignedValue>,   // Filter'ı geçen değerler
}
```

**Failed Inserts:** Reddedilen pull response hash'lerini bir sonraki pull request'te filter'a ekle. Sonsuz döngüleri önler.

**Purged Hash'ler:** Override edilen eski değerlerin hash'leri de filter'a eklenir. Eski versiyonların tekrar istenmesi önlenir.

### 4. Stake Ağırlıklı Gossip

**Push için:**
- `get_stake_bucket(min(this_stake, origin_stake))` → 25 bucket
- Ağırlık = `(bucket + 1)^2` — Karesel ağırlık, daha yüksek stake → daha çok seçilir ama sıfır değil

**Pull için:**
- Hedef seçimi: `weight = (log2(stake/unit + 1) + 1)^2`
- `stake_cap = min(peer_stake, self_stake)` — Kendi stake'i ile sınırla (çok yüksek stake'li node'a çok bağımlı olma)

**Nova Shield için:** cggmp21 MPC protokolü stake bazlı katılımcı seçimini benzer mantıkla yapabilir.

### 5. Aktif Set Rotasyonu

**Önerilen Döngü:**
```
Her X dakikada bir:
  1. Aktif gossip node'larını çek
  2. Ping/pong doğrula
  3. WeightedShuffle ile yeni active set oluştur
  4. Eski prune durumları korunur (bloom filter'lar)
```

`IndexMap` kullanımı: Yeni node eklenirken eski node'u LRU gibi `shift_remove_index(0)` ile sil.

### 6. Weighted Shuffle (Fenwick Tree)

**Nova Shield Chain için:** Validator seçimi, shard assignment, batch oluşturma — tüm bunlarda stake ağırlıklı rastgele seçim gerekir.

Solana implementasyonu: Fenwick tree ile O(log^2 n) karıştırma.

```rust
// Basit kullanım
let weights: Vec<u64> = validators.iter().map(|v| v.stake).collect();
let shuffle = WeightedShuffle::new("validator-select", &weights);
let selected: Vec<usize> = shuffle.shuffle(&mut rng).take(k).collect();
```

### 7. Ping/Pong NAT Tespiti

**Neden Gerekli:** UDP üzerinden gossip yapan node'ların adresleri sahte olabilir. Ping/pong çalışmadan aktif sete alınmamalı.

**Protocol:**
```
Ping: {from, token=[32 byte random], sig=sign(token)}
Pong: {from, hash=H("SOLANA_PING_PONG" || token), sig=sign(hash)}
```

**Cache Stratejisi:**
- TTL: 20 dakika (GOSSIP_PING_CACHE_TTL = 1280 saniye)
- Rate limit: 20 saniyede bir ping
- TTL/8 = 2.5 dakika: Yaklaşan expiry'de yenile (görünmez doğrulama)

### 8. ContactInfo Seri Format Optimizasyonu

**Eski Format (LegacyContactInfo):** Her socket ayrı alan = 12 * 18 bayt = 216 bayt (IPv4) ≈ 400+ bayt.

**Yeni Format (ContactInfo):**
- Tekrar eden IP'leri `addrs` listesinde tut
- Port'ları delta encode et (VarInt)
- `short_vec` ile kısa uzunluk prefixleri
- Çok daha kompakt!

**Nova Shield için:** node.info gossip mesajları benzer optimize format kullanmalı.

### 9. NodeInstance / Duplicate Node Tespiti

Aynı pubkey'den iki node instance çalışıyorsa:
- Her instance benzersiz `token` üretir
- Başlangıç zamanı (`timestamp`) karşılaştırılır
- Daha yeni başlayan instance kazanır
- Eski instance'ın değerleri suppress edilir

Bu mekanizma, yanlışlıkla aynı keypair ile iki node çalıştırıldığında fark edilmesini sağlar.

### 10. Shred Sıkıştırma (EpochSlots)

Slot durumu (hangi slotlar işlendi) bit array + DEFLATE ile sıkıştırılıyor. `MAX_CRDS_OBJECT_SIZE = 928 bayt` sınırına sığdırılamayan durum için birden fazla chunk.

**Nova Shield için:** Validatörlerin onayladığı blok listesi benzer kompakt formatta yayılabilir.

---

## Özet Tablo

| Dosya | Satır | Ana Katkı |
|-------|-------|-----------|
| crds_value.rs | 1097 | 14 gossip mesaj tipi, imzalı veri |
| crds.rs | ~800 | CRDS store, insert/lookup/cursor |
| crds_gossip_push.rs | 622 | Push protokolü, Plumtree |
| crds_gossip_pull.rs | ~900 | Pull + bloom filter anti-entropy |
| push_active_set.rs | 320 | Stake bucket, prune bloom filter |
| received_cache.rs | 195 | Prune skoru, saldırı koruması |
| weighted_shuffle.rs | 361 | Fenwick tree, ağırlıklı shuffle |
| cluster_info.rs | ~4000 | Ana merkez, Protocol enum, thread mimarisi |
| contact_info.rs | 964 | Optimize socket seri format |
| ping_pong.rs | 412 | NAT doğrulama, LRU cache |
| epoch_slots.rs | 548 | Slot durumu, DEFLATE |
| duplicate_shred.rs | 1140 | Çift shred kanıtı, chunk protokolü |
| crds_shards.rs | 240 | Hash prefix shard indeksi |
| crds_entry.rs | 123 | Generic type-safe lookup |
| crds_gossip.rs | 491 | Push/pull orkestrasyon |
| gossip_service.rs | 451 | 5 thread gossip mimarisi |
| restart_crds_values.rs | 371 | wen_restart koordinasyonu |
| cluster_info_metrics.rs | ~300 | AtomicU64 counter altyapısı |
| legacy_contact_info.rs | ~400 | Geriye uyumluluk |
| gossip_error.rs | 31 | Üst düzey hatalar |
| crds_gossip_error.rs | 8 | Protokol hataları |
| deprecated.rs | 22 | Eski yapılar |
| duplicate_shred_handler.rs | ~200 | Handler implementasyonu |
| duplicate_shred_listener.rs | 130 | Listener thread |
| lib.rs | 45 | Modül bildirimleri |
| main.rs | 345 | CLI (spy tool) |

---

*Agent 16 — Analiz tamamlandı. Tüm 26 dosya incelendi.*
