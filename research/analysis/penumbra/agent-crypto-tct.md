# Penumbra TCT (Tiered Commitment Tree) Kriptografi Analizi

**Analiz Tarihi:** 2026-07-22
**Kapsam:** `/research/chains/penumbra/crates/crypto/tct/src/` — 41 dosya, toplam ~4.000+ satır
**Analiz Eden:** agent-crypto-tct

---

## Genel Mimari Özeti

Penumbra'nın TCT (Tiered Commitment Tree) modülü, Penumbra blockchain'inin gizlilik katmanındaki **note commitment accumulator**'ıdır. Tasarımı:

- **3 katmanlı** (Block → Epoch → Global/Eternity Tree) sparse Merkle ağacı
- Her katman: 8 derinlikli **quadtree** (her node 4 çocuk)
- Toplam yükseklik: 8 + 8 + 8 = **24 seviye**
- **Poseidon377** hash fonksiyonu (BLS12-377 alanı üzerinde)
- **R1CS constraint sistemi** ile ZK devre doğrulaması

```
Eternity Tree (Global Root / Anchor)
   └─ 8 seviye (epoch root'larını içerir)
        Epoch Tree (Epoch Root)
            └─ 8 seviye (block root'larını içerir)
                  Block Tree (Block Root)
                      └─ 8 seviye (note commitment'larını içerir)
```

---

## DOSYA ANALİZLERİ

---

## [lib.rs]
**Dosya:** `src/lib.rs` **Satır Sayısı:** 152

### Genel Amaç
TCT crate'inin kök modülü. Tüm public API'yi re-export eder, iç modül hiyerarşisini tanımlar, koşullu derlemeleri yönetir.

### Struct/Enum/Type Listesi
- Re-export: `StateCommitment`, `Forgotten`, `DOMAIN_SEPARATOR`, `Proof`, `Position`, `Root`, `Tree`, `Witness`
- `pub mod r1cs` — `r1cs` feature ile etkinleşir
- `pub mod builder` — `builder::epoch` ve `builder::block` alt modülleri

### Özellikler (Attributes)
- `#![deny(clippy::unwrap_used)]` — Tüm crate'te `.unwrap()` yasak. Sağlamlık garantisi.
- `#![warn(missing_docs)]` — Tüm public itemlar dokümante edilmeli.
- `#[macro_use]` ile: `derivative`, `serde`, `tracing`, `thiserror`, `async_trait`

### Prelude Modülü (Satır 102-129)
Crate-internal import kolaylığı. Kritik tipler:
- `HashedMap<K, V>` = `im::HashMap<K, V, hash_hasher::HashBuildHasher>` — im crate'inden immutable hashmap + hash_hasher ile hızlı pre-hashed lookup.

### Test'ler
- `check_eternity_size`: Tree'nin 32 byte olmasını doğrulamak isterdi, şu an devre dışı.
- `check_eternity_proof_size`: Proof'un 2344 byte olmasını doğrulamak isterdi, devre dışı.

### Tasarım Kararları
- `internal` modülü feature flag'le kontrol edilir: `#[cfg(any(doc, feature = "internal"))]`. Normal kullanımda iç API gizlidir.
- `im::HashMap` seçimi: persistent/immutable veri yapısı — Rust'ın ownership sınırlarını aşmadan ağaç içi ref paylaşımı sağlar.

### Nova Shield Chain İçin Dersler
- `#![deny(clippy::unwrap_used)]` bizim crate'lerimizde de standart olmalı.
- `im::HashMap` + `hash_hasher` kombinasyonu, state/commitment index'i için mükemmel bir model.

---

## [tree.rs]
**Dosya:** `src/tree.rs` **Satır Sayısı:** 885

### Genel Amaç
`Tree` struct'ı: Public API'nin ana giriş noktası. Sparse Merkle ağacının en üst seviyesi. 65,536 epoch × 65,536 block × 65,536 commitment kapasitesi = 281,474,976,710,656 toplam commitment.

### Struct/Enum/Type Listesi

#### `Tree` (Satır 19-23)
```rust
pub struct Tree {
    index: HashedMap<StateCommitment, index::within::Tree>,
    inner: Arc<frontier::Top<frontier::Tier<frontier::Tier<frontier::Item>>>>,
}
```
- `index`: Commitment → pozisyon haritası (hızlı witness lookup için)
- `inner`: `Arc<frontier::Top<...>>` ile tip güvenceli 24 seviyeli frontier quadtree
- `Arc::make_mut()` kullanımı: copy-on-write semantiği sağlar

#### `Root` (Satır 44-55)
```rust
pub struct Root(pub Hash);
```
- Empty tree için: `Root(Hash::zero())`
- Protobuf: `pb::MerkleRoot` ↔ `Root` dönüşümü `Fq::from_bytes_checked` ile

#### `Position` (Satır 97-146)
```rust
pub struct Position(index::within::Tree);
```
- `.commitment() -> u16`: Block içindeki commitment indeksi
- `.block() -> u16`: Epoch içindeki block indeksi
- `.epoch() -> u16`: Tree içindeki epoch indeksi
- `From<u64>` / `Into<u64>` dönüşümü: 48-bit pozisyon kodlaması

### Fonksiyonlar

#### `Tree::insert` (Satır 192-262)
**Parametreler:** `&mut self, witness: Witness, commitment: StateCommitment` → `Result<Position, InsertError>`

Algoritma:
1. `witness` moduna göre item oluştur: `Keep` → `commitment.into()`, `Forget` → `Hash::of(commitment).into()`
2. `inner.position().ok_or(InsertError::Full)?` ile doluluk kontrolü
3. `Arc::make_mut(&mut self.inner)` ile CoW güvenceli mutasyon
4. Nested `.update()` zincirleriyle en derin açık blok bulunur:
   - Önce en son block denenir: finalized ise None dön
   - Block full ise epoch'a yeni Tier ekle
   - Epoch full veya finalized ise tree'ye yeni Tier ekle
5. `Witness::Keep` ise `self.index.insert(commitment, position)` — aynı commitment tekrar eklenirse eski silinir (`forget`)

#### `Tree::witness` (Satır 268-293)
**Parametreler:** `&self, commitment: StateCommitment` → `Option<Proof>`

Algoritma:
1. `self.index.get(&commitment)` ile pozisyon bulunur
2. `self.inner.witness(index)` ile auth path oluşturulur
3. `leaf == Hash::of(commitment)` doğrulaması (`debug_assert`)
4. `Proof` döndürülür

#### `Tree::forget` (Satır 300-315)
**Parametreler:** `&mut self, commitment: StateCommitment` → `bool`

1. `self.index.get(&commitment)` ile pozisyon bulunur
2. `Arc::make_mut(&mut self.inner).forget(within_epoch)` — ağaçtan silinir
3. `self.index.remove(&commitment)` — index'ten silinir
4. Return: daha önce witness'lanmış mıydı?

#### `Tree::insert_block` (Satır 351-476) ve `insert_epoch` (Satır 556-627)
Hazır oluşturulmuş (muhtemelen farklı thread'de) block/epoch'u tree'ye ekler. Paralel inşa için optimize edilmiş.

#### `Tree::end_block` / `Tree::end_epoch` (Satır 481-508, 632-655)
Mevcut açık block/epoch'u finalize eder, yeni boş bir tane başlatır.

#### `Tree::from_reader` / `to_writer` (Satır 767-803)
Incremental serialization desteği.

#### `From<frontier::Top<...>> for Tree` (Satır 862-884)
Ağaçtan index'i yeniden oluşturur: `structure::Kind::Leaf { commitment: Some(c) }` node'larını tarar.

### Matematiksel Formüller
- Kapasite: `65,536^3 = 2^48` commitment
- Pozisyon kodlaması: `epoch << 32 | block << 16 | commitment`

### Unsafe Bloklar
Yok.

### Test'ler
Satır boyutu testleri şu an devre dışı.

### Tasarım Kararları
- `Arc::make_mut()`: Clone-on-write. Tree'yi klonlamak ucuz, mutasyon gerektiğinde deep copy.
- `TrackForgotten::Yes`: Sadece tam Tree'de açık, builder'larda `No` — Builder'larda forgotten tracking gereksiz.
- Block/epoch finalization'ın eksplisit olması zorunlu — otomatik finalization yok.

### Nova Shield Chain İçin Dersler
- Note commitment accumulator gerektiren herhangi bir ZK sistemi için bu yapı direkt kullanılabilir.
- `Arc::make_mut()` + CoW pattern, blockchain state için ideal — block başına sadece değişen kısım kopyalanır.
- Commitment uniqueness garantisi: aynı commitment ikinci kez eklenirse eski silinir. Çift harcama önlemi.

---

## [commitment.rs]
**Dosya:** `src/commitment.rs` **Satır Sayısı:** 194

### Genel Amaç
`StateCommitment` tipi: bir note veya swap'a commitment. BLS12-377 alanı üzerinde tek bir `Fq` elemanı.

### Struct/Enum/Type Listesi

#### `StateCommitment` (Satır 7)
```rust
pub struct StateCommitment(pub Fq);
```
- `Fq`: `decaf377::Fq` — BLS12-377 alanının asal alanı, 32 byte.
- `PartialOrd, Ord, Hash`: Hashmap key olarak kullanılabilir.

#### `ParseCommitmentError`
- `InvalidHex(hex::FromHexError)`
- `InvalidCommitment(InvalidStateCommitment)`

#### `InvalidStateCommitment`
Canonical encoding geçersiz olduğunda döner.

### Fonksiyonlar

#### `StateCommitment::parse_hex` (Satır 22-25)
Hex stringden `StateCommitment` oluşturur.

### Trait Implementasyonları
- `DomainType for StateCommitment`: Protobuf `pb::StateCommitment` ile dönüşüm
- `TryFrom<pb::StateCommitment>`: `Fq::from_bytes_checked` ile güvenli dönüşüm
- `TryFrom<[u8; 32]>`: Byte array'den dönüşüm
- `TryFrom<&[u8]>`: Slice'dan dönüşüm
- `From<StateCommitment> for [u8; 32]`: `commitment.0.to_bytes()`

### Arbitrary (proptest) Desteği
`FqStrategy`: Ya önceden belirlenen bir listeden, ya da rastgele `Fq::from_le_bytes_mod_order` ile üretim.

### Test'ler
- `roundtrip_json_zero`: Zero commitment JSON round-trip testi.

### Tasarım Kararları
- `Fq::from_bytes_checked` kullanımı: Canonical encoding garantisi — field elemanı modüle büyük olamaz.
- bincode testi devre dışı: `pbjson_build` kaynaklı uyumsuzluk.

### Nova Shield Chain İçin Dersler
- Note commitment için tam olarak bu yapıyı kullanabiliriz.
- `Fq::from_bytes_checked` her deserialization'da zorunlu — raw bytes trust edilmez.

---

## [index.rs]
**Dosya:** `src/index.rs` **Satır Sayısı:** 285

### Genel Amaç
Commitment ve block pozisyonlarını encode/decode etmek için type-safe wrapper tipler. Farklı index türleri birbirine karıştırılamaz.

### Struct/Enum/Type Listesi

#### Temel Tipler (u16 newtype'lar)
- `Commitment(u16)` — Block içindeki commitment indeksi (0..=65535)
- `Block(u16)` — Epoch içindeki block indeksi (0..=65535)
- `Epoch(u16)` — Tree içindeki epoch indeksi (0..=65535)

#### `within::Block { commitment: Commitment }`
- `From<u16>` / `Into<u16>`: tek u16 olarak kodlanır

#### `within::Epoch { block: Block, commitment: Commitment }`
- `From<u32>` / `Into<u32>`: `(block << 16) | commitment`

#### `within::Tree { epoch: Epoch, block: Block, commitment: Commitment }`
- `From<u64>` / `Into<u64>`: `(epoch << 32) | (block << 16) | commitment`

### Matematiksel Formüller (Pozisyon Kodlaması)

```
u64 pozisyon = (epoch as u64) << 32
             | (block as u64) << 16
             | (commitment as u64)
```

Her biri 16 bit. Toplam 48 bit kullanılır (u64'ün alt 48 biti). Üst 16 bit her zaman 0.

### Test'ler (Satır 255-284) — proptest
- `u64_convert_eternity_inverse`: Tüm e,b,c değerleri için `u64 → Tree → u64` round-trip
- `u32_convert_epoch_inverse`: Epoch içi round-trip
- `u16_convert_block_inverse`: Block içi round-trip

### Tasarım Kararları
- Newtype pattern ile `Commitment`, `Block`, `Epoch` tipler birbirine geçirilemez — compile-time güvenlik.
- 16-bit sınır: Her seviyede maksimum 65,536 eleman → `2^16`.

### Nova Shield Chain İçin Dersler
- Blockchain state'inde farklı indeks türleri için bu newtype pattern zorunlu.
- `within::Tree` pozisyon kodlaması: veritabanında tek bir `u64` sütunuyla tüm hiyerarşiyi saklayabiliriz.

---

## [proof.rs]
**Dosya:** `src/proof.rs` **Satır Sayısı:** 144

### Genel Amaç
Public `Proof` API'si. Bir commitment'ın ağaçta var olduğunu kanıtlayan authentication path.

### Struct/Enum/Type Listesi

#### `Proof` (Satır 7-11)
```rust
pub struct Proof(
    pub(super) crate::internal::proof::Proof<
        frontier::Top<frontier::Tier<frontier::Tier<frontier::Item>>>
    >,
);
```

`Proof` wrap'ler generic internal proof'u Tree tipiyle monomorphize edilmiş olarak.

### Fonksiyonlar

#### `Proof::new` (Satır 16-56)
**Parametreler:** `commitment: StateCommitment, position: Position, auth_path: [[Hash; 3]; 24]`

24 seviye derinlikte, her seviyede 3 kardeş hash içeren auth path'i iç temsile dönüştürür. Statik olarak unwrap edilir:
```rust
let [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x] = auth_path;
// Her seviye için Node wrapper oluşturulur, sondan başa doğru
let child = Leaf;
let child = Node { siblings: x, child };
// ... 24 kez tekrarlanır
```

Bu kodu okumak için kritik: **x son seviyedir (height=1), a en üsttür (height=24)**.

#### `Proof::dummy` (Satır 59-63)
Test için: pozisyon=0, auth_path= rastgele hash'ler.

#### `Proof::verify` (Satır 70-72)
```rust
pub fn verify(&self, root: Root) -> Result<(), VerifyError> {
    self.0.verify(root.0)
}
```

#### `Proof::auth_path` (Satır 90-121)
`[[Hash; 3]; 24]` döndürür — root'tan leaf'e doğru sıralı.

#### `Proof::root` (Satır 85-87)
Proof'tan root hash'i yeniden hesaplar — verifyden bağımsız.

### Matematiksel Formüller
- Auth path: 24 × 3 = **72 hash değeri** = 72 × 32 = **2304 byte**
- Artı position (8 byte) ve commitment (32 byte): toplam ~2344 byte (lib.rs'deki yorum)

### Tasarım Kararları
- Auth path `[[Hash; 3]; 24]`: Her seviyede 4 çocuktan 1'i "biz"iz, geri kalan 3'ü proof içinde — quadtree yapısı.
- Sıralama: `auth_path[0]` = root'a en yakın (en yüksek) seviye.

### Nova Shield Chain İçin Dersler
- ZK proof sistemimizde note inclusion proof bu formatta olacak.
- 24 seviye × 3 sibling = R1CS devresine eklenecek constraint sayısı: 24 × hash_constraint.

---

## [witness.rs]
**Dosya:** `src/witness.rs` **Satır Sayısı:** 55

### Genel Amaç
`Witness` enum: commitment eklerken "sakla" veya "unut" kararı.

### Struct/Enum/Type Listesi

#### `Witness`
```rust
pub enum Witness {
    Forget,
    Keep,
}
```

- `Forget`: Hash'i sakla, commitment'ı değil. Daha az bellek.
- `Keep`: Commitment ve hash'i sakla. Sonradan witness üretilebilir.

### Trait Implementasyonları
Custom serde: `"keep"` / `"forget"` string değerleri. Case-insensitive deserialization.

### Tasarım Kararları
- `Forget` ile ekleme, `Keep` ile ekleme sonra `forget()` çağırmaktan daha verimli — hash'i direkt ekler, gereksiz index oluşturmaz.

### Nova Shield Chain İçin Dersler
- Blockchain node'ları local witness'a ihtiyaç duymuyorsa `Witness::Forget` kullanmalı — bellek tasarrufu.

---

## [internal/hash.rs]
**Dosya:** `src/internal/hash.rs` **Satır Sayısı:** 325

### Genel Amaç
TCT'nin tüm hash hesaplamalarının merkezi. `Hash` tipi, `GetHash` trait'i, `Forgotten` versiyonlama, domain separator.

### Struct/Enum/Type Listesi

#### `Hash` (Satır 74-75)
```rust
pub struct Hash(#[serde(with = "crate::storage::serialize::fq")] Fq);
```
BLS12-377 alan elemanı — 32 byte.

Özel değerler:
- `Hash::zero()` — frontier node padding
- `Hash::one()` — complete node padding (zero'dan farklı olması kritik!)
- `Hash::uninitialized()` — `Fq::SENTINEL` (alan dışı değer, sentinel olarak)

#### `DOMAIN_SEPARATOR` (Satır 101-102)
```rust
pub static DOMAIN_SEPARATOR: Lazy<Fq> =
    Lazy::new(|| Fq::from_le_bytes_mod_order(blake2b_simd::blake2b(b"penumbra.tct").as_bytes()));
```
`blake2b("penumbra.tct")` → `Fq` alanına indirge → domain separator.

#### `Forgotten` (Satır 214-270)
```rust
pub struct Forgotten([u8; 6]);
```
- 6 byte LE integer — maksimum `2^48 - 1` unutma işlemi
- `Forgotten::next()`: bir artırır
- `From<u64>` / `Into<u64>`: u64'ün alt 6 byte'ı

### Fonksiyonlar

#### `Hash::of` (Satır 153-156) — LEAF HASH
```rust
pub fn Hash::of(item: StateCommitment) -> Hash {
    Self(hash_1(&DOMAIN_SEPARATOR, item.0))
}
```
**Matematiksel formül:** `hash_leaf = poseidon377_hash_1(DOMAIN_SEPARATOR, commitment)`

#### `Hash::node` (Satır 160-210) — INTERNAL NODE HASH
```rust
pub fn Hash::node(height: u8, a: Hash, b: Hash, c: Hash, d: Hash) -> Hash {
    fn hash_node(height: u8, a: Hash, b: Hash, c: Hash, d: Hash) -> Hash {
        let height = Fq::from_le_bytes_mod_order(&height.to_le_bytes());
        Hash(hash_4(&(*DOMAIN_SEPARATOR + height), (a.0, b.0, c.0, d.0)))
    }
    // ...precomputed optimizasyon...
    hash_node(height, a, b, c, d)
}
```

**Matematiksel formül:**
```
domain_i = DOMAIN_SEPARATOR + height_i
node_hash(height, a, b, c, d) = poseidon377_hash_4(domain_i, (a, b, c, d))
```

Her height için farklı domain separator → farklı height'lardaki aynı çocuk değerleri farklı hash üretir (domain separation).

**Precomputation optimizasyonu (Satır 174-210):**
- `PRECOMPUTE_HEIGHTS: RangeInclusive<u8> = 9..=16`
- Height 9-16 arasındaki tamamen boş düğümlerin hash'leri önceden hesaplanır.
- Boş epoch içindeki boş block dizileri çok yaygın → bu optimizasyon gerçek savings sağlar.
- Height < 9: commitment'lar kriptografik açıdan rastgele → precomputing faydasız.
- Height > 16: epoch'lar tamamen dolu pek olmaz → precomputing faydasız.

### Trait Implementasyonları
#### `GetHash`
```rust
pub trait GetHash {
    fn hash(&self) -> Hash;
    fn cached_hash(&self) -> Option<Hash>;
    fn clear_cached_hash(&self) {}
}
```
- `hash()`: hesapla veya cache'den getir
- `cached_hash()`: sadece cache'den, yoksa None
- `clear_cached_hash()`: default implementasyon — hiçbir şey yapmaz (cache yoksa)

### Test'ler
- `forgotten_increments`: 10 iterasyon için `Forgotten::next()` testi.

### Unsafe Bloklar
Yok. `once_cell::sync::Lazy` güvenli lazy initialization sağlar.

### Tasarım Kararları
- `Hash::zero` vs `Hash::one` ayrımı: frontier padding ile complete padding bilinçli farklıdır. Aynı hash kullanılsaydı, frontier bir complete node ile karıştırılabilirdi.
- `Fq::SENTINEL`: Alan dışı değer → normal Fq değerlerinden güvenle ayırt edilebilir sentinel.
- Domain separator formülü `DOMAIN_SEPARATOR + height`: additive, her height benzersiz.
- `Forgotten([u8; 6])`: u64 yerine 6 byte — 2 byte tasarruf, bellekte önemli (çok sayıda node var).

### Nova Shield Chain İçin Dersler
- Nova Shield'ın kendi TCT benzeri yapısı için: `blake2b("nova-shield.tct")` ile domain separator oluştur.
- Poseidon hash kullan: arkworks veya poseidon377 benzeri.
- `Hash::zero` ve `Hash::one` ayrımını koru — frontier/complete karışmaması kritik.
- Height 9-16 precomputation, boş slot optimizasyonu için — block/epoch düzeyindeki boş aralıklar bekleniyor.

---

## [internal/hash/cache.rs]
**Dosya:** `src/internal/hash/cache.rs` **Satır Sayısı:** 74

### Genel Amaç
Thread-safe lazy hash cache. `CachedHash`: frontier node hash'lerini lazy hesaplamak için `Mutex<OptionHash>`.

### Struct/Enum/Type Listesi

#### `CachedHash`
```rust
pub struct CachedHash {
    mutex: Mutex<OptionHash>,
}
```
- Boyut: **40 byte** (test ile doğrulanmış)
- `Mutex` (parking_lot): Std Mutex'ten daha hızlı, futex tabanlı.

### Fonksiyonlar

#### `CachedHash::set_if_empty` (Satır 40-49)
```rust
pub fn set_if_empty(&self, new: impl FnOnce() -> Hash) -> Hash {
    let mut guard = self.mutex.lock();
    if let Some(hash) = Option::from(*guard) {
        hash  // Cache hit
    } else {
        let new = new();
        *guard = OptionHash::from(Some(new));
        new  // Computed and cached
    }
}
```
Lazy evaluation + cache pattern. `FnOnce` closure — sadece gerektiğinde çağrılır.

### Test'ler
- `cached_hash_size`: `assert_eq_size!(CachedHash, [u8; 40])`

### Tasarım Kararları
- `parking_lot::Mutex` seçimi: std::sync::Mutex'e göre %30-50 hız avantajı (uncontended case).
- `&self` (immutable ref) üzerinde mutex: interior mutability pattern — GetHash trait'i `&self` alır ama hash'i cache'leyebilir.

### Nova Shield Chain İçin Dersler
- Lazy hash cache için `parking_lot::Mutex<Option<Hash>>` kesin standart.
- `set_if_empty` pattern: closure geç değerlendirme için ideal.

---

## [internal/hash/option.rs]
**Dosya:** `src/internal/hash/option.rs` **Satır Sayısı:** 63

### Genel Amaç
`OptionHash`: `Option<Hash>` için tag byte'sız kompakt temsil.

### Struct/Enum/Type Listesi

#### `OptionHash`
```rust
pub struct OptionHash {
    inner: [u64; 4],
}
```
- `Default`: `[u64::MAX; 4]` = sentinel "None"
- `Option::Some(hash)`: `Fq::into_bigint(hash.0).0` — gerçek değer
- `[u64::MAX; 4]` hiçbir valid `Fq` değil çünkü modülüs daha küçük.

### Matematiksel Formüller
- BLS12-377 alanının modülüsü < `2^255` → `[u64::MAX; 4] = 2^256 - 1` alan dışı.
- Bu sentinel seçimi modüler aritmetiğin bir özelliğini exploit eder.

### Tasarım Kararları
- `Option<Hash>` 1 byte tag ekler → her node'da 1 byte kayıp → büyük ağaçlarda önemli.
- Sentinel trick: tag'siz `Option` — boyut optimizasyonu.

### Nova Shield Chain İçin Dersler
- BLS12-381 veya BN254 üzerinde de aynı sentinel trick uygulanabilir — modülüs her zaman 2^256'dan küçük.

---

## [internal/height.rs]
**Dosya:** `src/internal/height.rs` **Satır Sayısı:** 53

### Genel Amaç
Type-level Peano aritmetiği ile ağaç yüksekliğini derleme zamanında encode eder.

### Struct/Enum/Type Listesi

#### `Height` trait
```rust
pub trait Height {
    type Height: Path;
}
```

#### `IsHeight` trait
```rust
pub trait IsHeight: sealed::IsHeight {
    const HEIGHT: u8;
}
```

#### `Zero` ve `Succ<N>`
```rust
pub struct Zero;
impl IsHeight for Zero { const HEIGHT: u8 = 0; }

pub struct Succ<N>(N);
impl<N: IsHeight> IsHeight for Succ<N> {
    const HEIGHT: u8 = if let Some(n) = N::HEIGHT.checked_add(1) { n }
                       else { panic!("height overflow") };
}
```

### Tasarım Kararları
- Peano aritmetiği: `Succ<Succ<Succ<Zero>>>` = 3. Tip sisteminde encode edilmiş sayı.
- `sealed::IsHeight`: Sadece `Zero` ve `Succ` bu trait'i implement edebilir → kapalı küme.
- `checked_add` + `panic!` at const: derleme zamanı yükseklik taşma kontrolü.
- Bu sayede yanlış yükseklikte hash hesaplamak **derleme hatası** — runtime hata değil.

### Nova Shield Chain İçin Dersler
- Kritik: TCT'nin type-safety'si burada. Herhangi bir Merkle ağacı implementasyonu için aynı pattern kullanılabilir.
- Rust'ta const generics ile daha sade yazılabilir (`const HEIGHT: usize`) ama mevcut tasarım daha güçlü kısıtlamalar sağlar.

---

## [internal/path.rs]
**Dosya:** `src/internal/path.rs` **Satır Sayısı:** 365

### Genel Amaç
Authentication path temsili ve root hash hesaplaması. `WhichWay` enum ile quadtree yönlendirme.

### Struct/Enum/Type Listesi

#### `AuthPath<Tree>` type alias
```rust
pub type AuthPath<Tree> = <<Tree as Height>::Height as Path>::Path;
```
Her ağaç tipi için statik olarak doğru uzunlukta auth path.

#### `Path` trait (Satır 20-26)
```rust
pub trait Path: IsHeight + Sized {
    type Path;
    fn root(path: &Self::Path, index: u64, leaf: Hash) -> Hash;
}
```

#### `Leaf` (Satır 30)
```rust
pub struct Leaf;
// Path impl: leaf'i direkt döndürür, derinlik = 0
impl Path for Zero {
    type Path = Leaf;
    fn root(Leaf: &Leaf, _index: u64, leaf: Hash) -> Hash { leaf }
}
```

#### `Node<Child>` (Satır 42-52)
```rust
pub struct Node<Child> {
    pub siblings: [Hash; 3],
    pub child: Child,
}
```
Rekursif auth path. Derinlikçe tip encode edilmiş.

#### Auth path root hesaplama (Satır 54-68):
```rust
impl<Child, N: Path<Path = Child>> Path for Succ<N> {
    type Path = Node<Child>;

    fn root(Node { siblings, child }: &Node<Child>, index: u64, leaf: Hash) -> Hash {
        let which_way = WhichWay::at(Self::HEIGHT, index).0;
        let [leftmost, left, right, rightmost] =
            which_way.insert(N::root(child, index, leaf), *siblings);
        Hash::node(Self::HEIGHT, leftmost, left, right, rightmost)
    }
}
```

**Algoritma:**
1. `WhichWay::at(height, index)` ile bu seviyedeki yön belirlenir.
2. Alt seviyedeki child hash'i rekursif hesaplanır.
3. Child hash + 3 sibling → `which_way.insert()` ile doğru sıraya koyulur.
4. `Hash::node(height, a, b, c, d)` ile hash hesaplanır.
5. Tekrarlanır → root bulunur.

#### `WhichWay` (Satır 71-135)
```rust
pub enum WhichWay {
    Leftmost,  // 0
    Left,      // 1
    Right,     // 2
    Rightmost, // 3
}
```

#### `WhichWay::at` (Satır 87-101) — KRİTİK FONKSİYON
```rust
pub fn at(height: u8, index: u64) -> (WhichWay, u64) {
    let which_way = match (index >> (2 * (height - 1))) & 0b11 {
        0 => WhichWay::Leftmost,
        1 => WhichWay::Left,
        2 => WhichWay::Right,
        3 => WhichWay::Rightmost,
        _ => unreachable!(),
    };
    let index = index & !(0b11 << ((height - 1) * 2));
    (which_way, index)
}
```

**Matematiksel formül:**
- Quadtree: her düğümün 4 çocuğu var → her yönelim 2 bit
- Height `h` düğümünde yön: `(index >> (2*(h-1))) & 0b11`
- Çocuk index: `index & !(0b11 << ((h-1)*2))`

**Örnek:** Height=3, index=0b_01_10_11 (decimal 27):
- Height 3: `(27 >> 4) & 3 = 1 = Left`
- Height 2: `(27 >> 2) & 3 = 2 = Right`
- Height 1: `27 & 3 = 3 = Rightmost`

#### `WhichWay::insert` (Satır 108-119)
Yön + item + 3 sibling → [4 çocuk]. Karmaşık pattern matching ile sıralamayı korur.

### Matematiksel Formüller (Özet)
```
position_bits = u64 (alt 48 bit kullanılır)
height h'daki yön = (position_bits >> (2*(h-1))) & 0b11
height h'dan sonraki index = position_bits & !(0b11 << ((h-1)*2))

root_hash = hash_node(24, [
    hash_node(23, [
        ...
            hash_node(1, [sibling_a, sibling_b, sibling_c, leaf_hash])
    ])
])
```

### Test'ler (Satır 162-228) — proptest
- `which_way_indices_correct`: `directions_of_index` == `directions_via_indices` (index'ten türetilen yönler tutarlı)
- `which_way_direction_correct`: Yönlerden index geri kazanılabilir

### Protobuf Serialization (Satır 230-365)
`Node<Child>` ↔ `Vec<pb::MerklePathChunk>`: Her chunk 3 sibling hash içerir.

### Tasarım Kararları
- Rekursif tip: `Node<Node<Node<...Leaf>>>` — 24 derinlik, her derinlik farklı tip. Derleme zamanı derinlik garantisi.
- VecDeque kullanımı: Rekursif serializasyonda `push_front` ile doğru sıra.

### Nova Shield Chain İçin Dersler
- Bu authentication path algoritması herhangi bir quadtree Merkle proof'u için kullanılabilir.
- `WhichWay::at` formülü: iki bit kaydırma — çok verimli.
- R1CS devresinde aynı bit extraction kullanılır (bkz. r1cs.rs).

---

## [internal/proof.rs]
**Dosya:** `src/internal/proof.rs` **Satır Sayısı:** 116

### Genel Amaç
Generic Merkle inclusion proof. Herhangi bir height'ta çalışır.

### Struct/Enum/Type Listesi

#### `Proof<Tree: Height>` (Satır 18-22)
```rust
pub struct Proof<Tree: Height> {
    pub(crate) position: u64,
    pub(crate) auth_path: AuthPath<Tree>,
    pub(crate) leaf: StateCommitment,
}
```

### Fonksiyonlar

#### `Proof::verify` (Satır 28-34)
```rust
pub fn verify(&self, root: Hash) -> Result<(), VerifyError> {
    if root == self.root() { Ok(()) }
    else { Err(VerifyError { root }) }
}
```

#### `Proof::root` (Satır 37-39)
```rust
pub fn root(&self) -> Hash {
    Tree::Height::root(&self.auth_path, self.position, Hash::of(self.leaf))
}
```
Auth path boyunca hash'leri birleştirerek root'u yeniden hesaplar.

#### `VerifyError`
```rust
pub struct VerifyError { root: Hash }
```
Proof'un hangi root'a karşı başarısız olduğunu döner.

### Tasarım Kararları
- Verification: root yeniden hesaplanır, expected root ile karşılaştırılır — klasik Merkle proof verification.
- `Tree: Height` bound: generic parametresi olan tip, height bilgisini type-level'da taşır.

### Nova Shield Chain İçin Dersler
- Bu proof struct'ı, Nova Shield'ın note spending devre doğrulaması için temel.

---

## [internal/insert.rs]
**Dosya:** `src/internal/insert.rs` **Satır Sayısı:** 186

### Genel Amaç
`Insert<T>` enum: ağaca eklenecek ya gerçek item, ya sadece hash.

### Struct/Enum/Type Listesi

#### `Insert<T>` (Satır 9-15)
```rust
pub enum Insert<T> {
    Keep(T),   // Witness'lanacak
    Hash(Hash), // Sadece hash
}
```

#### `InsertMut<'a, T>` (Satır 19-26)
`Insert<&mut T>` için ayrı enum — lifetime bound gerektirir.

### Fonksiyonlar (Insert<T>)
- `as_ref() -> Insert<&T>`: Reference dönüşümü
- `as_mut() -> InsertMut<'_, T>`: Mutable reference dönüşümü
- `is_keep() / is_hash()`: Type check
- `map<U, F>(f: F) -> Insert<U>`: Keep kısmına fonksiyon uygula
- `and_then<U, F>(f: F) -> Insert<U>`: Flatmap benzeri — Keep içinden Insert<U> üret
- `keep() -> Option<T>`: Keep ise Some, Hash ise None

### Trait Implementasyonları
- `GetHash for Insert<T: GetHash>`: Keep ise T'nin hash'i, Hash ise direkt hash
- `Height for Insert<T>`: T'nin height'ı
- `Forget for Insert<T: ForgetOwned>` (Satır 167-185):
```rust
fn forget(&mut self, forgotten: Option<Forgotten>, index: impl Into<u64>) -> bool {
    let this = std::mem::replace(self, Insert::Hash(Hash::zero()));
    (*self, was_forgotten) = match this {
        Insert::Keep(item) => item.forget_owned(forgotten, index),
        Insert::Hash(_) => (this, false),
    };
    was_forgotten
}
```
`std::mem::replace` ile temporary swap out — borrow checker sorununu çözer.

### Nova Shield Chain İçin Dersler
- `Insert<T>` pattern: "witness veya sadece hash" kararını yükseltip lower type'lara yayma — Nova Shield'ın privacy modelinde bu temel.

---

## [internal/interface.rs]
**Dosya:** `src/internal/interface.rs` **Satır Sayısı:** 165

### Genel Amaç
İç ağaç API'sinin tüm trait tanımları. `Frontier`, `Focus`, `Complete`, `Witness`, `Forget`, `ForgetOwned`, `OutOfOrder`, `UncheckedSetHash`.

### Trait Listesi

#### `Frontier: Focus + Sized` (Satır 12-34)
```rust
pub trait Frontier: Focus + Sized {
    type Item;
    fn new(item: Self::Item) -> Self;
    fn insert_owned(self, item: Self::Item) -> Result<Self, Full<Self>>;
    fn update<T>(&mut self, f: impl FnOnce(&mut Self::Item) -> T) -> Option<T>;
    fn focus(&self) -> Option<&Self::Item>;
    fn is_full(&self) -> bool;
}
```
Frontier = en son eklenen elemanın yaşadığı "sağ kenar" ağacı.

#### `Focus: Height + GetHash` (Satır 37-44)
```rust
pub trait Focus: Height<Height = <Self::Complete as Height>::Height> + GetHash {
    type Complete: Complete<Focus = Self>;
    fn finalize_owned(self) -> Insert<Self::Complete>;
}
```
`Focus` → `Complete` dönüşümü: frontier bir düğüm dolduğunda `Complete` ağacına dönüşür.

#### `Complete: Height + GetHash` (Satır 47-53)
```rust
pub trait Complete: Height + GetHash {
    type Focus: Focus<Complete = Self>;
}
```
`Complete` ve `Focus` çift yönlü ilişki — tip sistemi bu duality'i zorlar.

#### `Full<T: Frontier>` (Satır 56-63)
Full frontier → hem eklenmemiş item hem de complete versiyonu döndürülür.

#### `Witness: Height + Sized` (Satır 65-72)
```rust
pub trait Witness: Height + Sized {
    fn witness(&self, index: impl Into<u64>) -> Option<(AuthPath<Self>, Hash)>;
}
```

#### `Forget: Height` (Satır 83-89)
```rust
pub trait Forget: Height {
    fn forget(&mut self, forgotten: Option<Forgotten>, index: impl Into<u64>) -> bool;
}
```

#### `ForgetOwned: Height + Sized` (Satır 92-106)
```rust
pub trait ForgetOwned: Height + Sized {
    fn forget_owned(
        self,
        forgotten: Option<Forgotten>,
        index: impl Into<u64>,
    ) -> (Insert<Self>, bool);
}
```
Sonunda hiç witness kalmayabilir → `Insert::Hash` döner.

#### `OutOfOrder` (crate-only) (Satır 116-131)
Deserialization için. Önce boş struktur oluştur, sonra commitment'ları pozisyona göre doldur.

#### `UncheckedSetHash` (crate-only) (Satır 152-164)
Deserialization'ın 2. aşaması: hash'leri yerleştir.

### Tasarım Kararları
- `Focus`-`Complete` duality: her frontier tipinin tam olarak bir complete karşılığı var. Compile-time garantisi.
- `OutOfOrder`: Veritabanından okuma sırasında commitment'lar sırasız gelebilir. Bu trait bu durumu handle eder.

### Nova Shield Chain İçin Dersler
- Bu trait hierarchy, herhangi bir sparse Merkle tree implementasyonu için mükemmel şablon.

---

## [internal/three.rs]
**Dosya:** `src/internal/three.rs` **Satır Sayısı:** 260

### Genel Amaç
`Three<T>`: Maksimum 3 elemanlı Vec wrapper. Frontier node'larındaki sol kardeşleri saklar (max 3 kardeş çünkü focus 4. çocuk).

### Struct/Enum/Type Listesi

#### `Three<T>`
```rust
pub struct Three<T> {
    elems: Vec<T>, // capacity = 4 (önceden ayrılmış)
}
```
Kapasite 4 olarak ayrılmış: `push` dolduğunda 4 elemanlı array'e dönüşebilir, yeniden tahsisat yok.

#### `Elems<'a, T>` / `ElemsMut<'a, T>` / `IntoElems<T>`
0, 1, 2, 3 elemanlı durumları enumerate eden enum'lar. Pattern matching kolaylaştırır.

### Fonksiyonlar

#### `Three::push(self, item: T) -> Result<Self, [T; 4]>` (Satır 47-57)
- Doldurulmamışsa `Ok(Three)`, doluysa `Err([T; 4])` — 4 elemana geçiş.
- `Vec::try_into()` ile 4 elemanlı array'e dönüşüm.

### Tasarım Kararları
- `Vec::with_capacity(4)`: Frontier node başına 1 extra allocation'ı önler.
- `Elems` enum: match expression'larda ergonomi — `elems()`, `elems_mut()`, `into_elems()`.

### Nova Shield Chain İçin Dersler
- Quadtree node'larındaki "3 sibling + focus" pattern için bu yapı.

---

## [internal/frontier/item.rs]
**Dosya:** `src/internal/frontier/item.rs` **Satır Sayısı:** 156

### Genel Amaç
Frontier'ın en derin elemanı (height=0). Bir commitment veya hash saklayan yaprak.

### Struct/Enum/Type Listesi

#### `frontier::Item`
```rust
pub struct Item {
    item: Insert<(StateCommitment, Hash)>,
}
```
- `Keep`: `(commitment, Hash::of(commitment))` çifti
- `Hash`: Sadece hash, commitment olmadan

### Fonksiyonlar

#### `From<StateCommitment> for Item` (Satır 9-15)
```rust
impl From<StateCommitment> for Item {
    fn from(commitment: StateCommitment) -> Self {
        Self { item: Insert::Keep((commitment, Hash::of(commitment))) }
    }
}
```
Commitment eklendiğinde hash direkt hesaplanır — sonra tekrar hesaplamak gerekmez.

#### `GetHash for Item` (Satır 25-38)
Her iki durumda da hash döner.

#### `Focus for Item` (Satır 44-52)
`finalize_owned`: Keep ise `complete::Item::new(hash, commitment)`, Hash ise `Insert::Hash(hash)`.

#### `Witness for Item` (Satır 54-60)
```rust
fn witness(&self, index: impl Into<u64>) -> Option<(AuthPath<Self>, Hash)> {
    debug_assert_eq!(index.into(), 0, "non-zero index when witnessing leaf");
    Some((path::Leaf, self.hash()))
}
```
Index her zaman 0 olmalı (yaprak) — sadece `(Leaf, hash)` döner.

#### `Forget for Item` (Satır 69-83)
Keep → Hash dönüşümü: commitment atılır, hash kalır.

#### `finish_initialize` (Satır 139-155)
Deserialization sonrası:
- Keep ama hash uninitialized: `Hash::of(commitment)` hesapla
- Hash uninitialized: frontier empty → `Hash::zero()`

### Tasarım Kararları
- Hash ön hesaplaması (From impl): Insert anında hesaplanır → `witness()` çağrısında ucuz.

### Nova Shield Chain İçin Dersler
- Commitment ekleme anında hash hesapla, değil sonradan.

---

## [internal/frontier/leaf.rs]
**Dosya:** `src/internal/frontier/leaf.rs` **Satır Sayısı:** 135

### Genel Amaç
`frontier::Leaf<Item>`: Tek bir item'ı saran frontier "wrapper". Dolduğunda yeni leaf oluşturmayı zorlar.

### Önemli Tasarım

`insert_owned` her zaman `Err(Full {...})` döner:
```rust
fn insert_owned(self, item: Self::Item) -> Result<Self, Full<Self>> {
    Err(Full { item, complete: self.finalize_owned() })
}
fn is_full(&self) -> bool { true }
```
Bir leaf her zaman "dolu" — üstündeki node yeni slot açmalı.

---

## [internal/frontier/node.rs]
**Dosya:** `src/internal/frontier/node.rs` **Satır Sayısı:** 629

### Genel Amaç
`frontier::Node<Child>`: Frontier'daki iç düğüm. `Three<Arc<Insert<Child::Complete>>>` + `Arc<Child>` (focus).

### Struct/Enum/Type Listesi

#### `frontier::Node<Child>`
```rust
pub struct Node<Child: Focus> {
    hash: CachedHash,
    forgotten: [Forgotten; 4],
    siblings: Three<Arc<Insert<Child::Complete>>>,
    focus: Arc<Child>,
}
```
- `hash`: Lazy hesaplanan, cache'li hash
- `forgotten`: Her 4 child için ayrı forgotten version
- `siblings`: 0-3 complete çocuk (soldaki)
- `focus`: En sağdaki aktif frontier child

### Fonksiyonlar

#### `GetHash for Node<Child>` (Satır 51-98)
```rust
fn hash(&self) -> Hash {
    self.hash.set_if_empty(|| {
        let zero = Hash::zero();
        let focus = self.focus.hash();
        let (a, b, c, d) = match self.siblings.elems() {
            Elems::_0([]) => (focus, zero, zero, zero),
            Elems::_1(full) => { let [a] = hashes_of_all(full); (a, focus, zero, zero) }
            Elems::_2(full) => { let [a, b] = hashes_of_all(full); (a, b, focus, zero) }
            Elems::_3(full) => { let [a, b, c] = hashes_of_all(full); (a, b, c, focus) }
        };
        Hash::node(<Self as Height>::Height::HEIGHT, a, b, c, d)
    })
}
```
Focus her zaman en sağdaki pozisyonda → sıfır padding soldan sağa.

#### `Frontier for Node<Child>` — `insert_owned` (Satır 178-217)
```rust
fn insert_owned(self, item: Self::Item) -> Result<Self, Full<Self>> {
    match focus.insert_owned(item) {
        Ok(focus) => Ok(Self::from_parts(forgotten, siblings, focus)), // Focus'a eklendi
        Err(Full { item, complete: sibling }) => match siblings.push(Arc::new(sibling)) {
            Ok(siblings) => Ok(Self::from_parts(forgotten, siblings, Child::new(item))), // Yeni sibling + yeni focus
            Err(children) => Err(Full { item, complete: complete::Node::from_children_or_else_hash(...) }) // Düğüm doldu
        }
    }
}
```
**Algoritma:**
1. Önce focus'a ekle
2. Focus doluysa → focus'u complete'e dönüştür, siblings'e ekle, yeni empty focus oluştur
3. Siblings de doluysa (3 + focus = 4) → bu node complete oldu, parent'a propagate et

#### `Forget for Node<Child>` (Satır 348-394)
- `WhichWay::at(height, index)` ile yön bul
- İlgili child'ın forget'ini çağır
- `self.forgotten[which_way] = forgotten.next()` — silinme versiyonunu güncelle

#### `GetPosition for Node<Child>` (Satır 225-245)
```rust
fn position(&self) -> Option<u64> {
    let child_capacity: u64 = 4u64.pow(Child::Height::HEIGHT.into());
    let siblings = self.siblings.len() as u64;
    if let Some(focus_position) = self.focus.position() {
        Some(siblings * child_capacity + focus_position)
    } else if siblings + 1 < 4 {
        Some((siblings + 1) * child_capacity)
    } else { None }
}
```
**Matematiksel formül:** `next_position = siblings_count * 4^child_height + focus_position`

### Tasarım Kararları
- `Arc<Child>` + `Arc::make_mut()`: Clone-on-write. Ağacı kopyalamak ucuz, mutasyon gerektiğinde deep copy.
- `Arc<Insert<Child::Complete>>`: Complete sibling'ler de Arc ile paylaşılabilir.
- Hash invalidation: `update()` çağrısında focus hash değişirse `CachedHash` sıfırlanır.

### Nova Shield Chain İçin Dersler
- Insert algoritması: "try focus, if full move focus to siblings and create new focus" — bu quadtree insertion'ın özü.
- `GetPosition` formülü: hangi pozisyonun dolu, hangisinin boş olduğunu hızlıca belirler.

---

## [internal/frontier/tier.rs]
**Dosya:** `src/internal/frontier/tier.rs` **Satır Sayısı:** 463

### Genel Amaç
`frontier::Tier<Item>`: 8 derinlikli quadtree — bir "katman" (block veya epoch).

### Struct/Enum/Type Listesi

#### `frontier::Tier<Item>`
```rust
pub struct Tier<Item: Focus + Clone> {
    inner: Inner<Item>,
}

enum Inner<Item: Focus + Clone> {
    Frontier(Box<Nested<Item>>),  // Aktif frontier
    Complete(complete::Nested<Item::Complete>),  // Complete + witness'lı
    Hash(Hash),  // Complete + witness'sız
}

// Nested = N<N<N<N<N<N<N<N<L<Item>>>>>>>> (8 derinlik)
pub type Nested<Item> = N<N<N<N<N<N<N<N<L<Item>>>>>>>>>;
```

### Fonksiyonlar

#### `Tier::insert` (Satır 82-93)
```rust
pub fn insert(&mut self, item: Item) -> Result<(), Item> {
    let inner = std::mem::replace(&mut self.inner, Inner::Hash(Hash::zero()));
    let result;
    (result, *self) = match Self { inner }.insert_owned(item) {
        Ok(this) => (Ok(()), this),
        Err((item, this)) => (Err(item), this),
    };
    result
}
```
`std::mem::replace` ile geçici swap — borrow checker sorununu çözer.

#### `Tier::finalize` (Satır 160-186)
```rust
pub fn finalize(&mut self) -> bool {
    // Inner::Frontier'ı Complete veya Hash'e dönüştür
    self.inner = match inner {
        Inner::Frontier(frontier) => match frontier.finalize_owned() {
            Insert::Hash(hash) => Inner::Hash(hash),
            Insert::Keep(inner) => Inner::Complete(inner),
        },
        inner @ (Inner::Complete(_) | Inner::Hash(_)) => inner, // Zaten finalized
    };
    already_finalized
}
```

#### `Forget for Tier<Item>` (Satır 264-295)
Complete tier'dan forget → `ForgetOwned` kullanır. Tüm children silinirse `Inner::Hash` olur.

#### `position_advances_by_one` test (Satır 453-462):
```rust
fn position_advances_by_one() {
    let mut tier: Tier<Item> = Tier::new(Hash::zero().into());
    for expected_position in 1..=(u16::MAX as u64) {
        assert_eq!(tier.position(), Some(expected_position));
        tier.insert(Hash::zero().into()).unwrap();
    }
    assert_eq!(tier.position(), None); // Dolu
}
```
Tier kapasitesi: 65,536 eleman (u16::MAX + 1).

### Tasarım Kararları
- `Inner::Hash` vs `Inner::Complete`: Complete ama witness'lı mı, yoksa sadece hash mı? Hash daha az bellek.
- `Box<Nested<Item>>`: 8 derinlikli ağaç stack'te çok büyük → heap'e boxing zorunlu.

---

## [internal/frontier/top.rs]
**Dosya:** `src/internal/frontier/top.rs` **Satır Sayısı:** 317

### Genel Amaç
`frontier::Top<Item>`: Ağacın en tepesi. Boş olabilir (Tier'dan farklı), finalize edilemez (Tree hiçbir zaman finalize edilmez).

### Struct/Enum/Type Listesi

#### `frontier::Top<Item>`
```rust
pub struct Top<Item: Focus + Clone> {
    track_forgotten: TrackForgotten,
    inner: Option<Nested<Item>>,
}
```
- `inner = None`: Boş ağaç (Root = `Hash::zero()`)
- `TrackForgotten::Yes/No`: Tree'de Yes, builder'larda No

#### `TrackForgotten`
```rust
pub enum TrackForgotten { Yes, No }
```

### Fonksiyonlar

#### `Top::forget` (Satır 94-106)
Tree seviyesinde forget — `inner.forgotten()` den forgotten version alır.

#### `Top::hash` (Satır 200-206)
```rust
fn hash(&self) -> Hash {
    if let Some(ref inner) = self.inner {
        inner.hash()
    } else {
        Hash::zero()  // Boş ağaç
    }
}
```

#### `Top::position` (Satır 186-193)
```rust
fn position(&self) -> Option<u64> {
    if let Some(ref frontier) = self.inner {
        frontier.position()
    } else {
        Some(0)  // Boş ağaç → bir sonraki ekleme 0'a gider
    }
}
```

### Test'ler
```rust
fn position_advances_by_one() {
    let mut top: Top<Item> = Top::new(TrackForgotten::No);
    for expected_position in 0..=(u16::MAX as u64) {
        assert_eq!(top.position(), Some(expected_position));
        top.insert(Hash::zero().into()).unwrap();
    }
    assert_eq!(top.position(), None);
}
```
`Tier`'dan farkı: position 1'den değil **0'dan** başlar.

---

## [internal/complete/item.rs]
**Dosya:** `src/internal/complete/item.rs` **Satır Sayısı:** 112

### Genel Amaç
`complete::Item`: Finalize edilmiş, witness'lı yaprak. Hash + commitment çifti.

```rust
pub struct Item {
    hash: Hash,
    commitment: StateCommitment,
}
```

### `ForgetOwned for Item` (Satır 45-54)
```rust
fn forget_owned(self, _forgotten: Option<Forgotten>, index: impl Into<u64>) -> (Insert<Self>, bool) {
    debug_assert_eq!(index.into(), 0);
    (Insert::Hash(self.hash), true)  // Commitment atılır, hash kalır
}
```

---

## [internal/complete/node.rs]
**Dosya:** `src/internal/complete/node.rs` **Satır Sayısı:** 332

### Genel Amaç
`complete::Node<Child>`: Sparse complete node. Sadece en az 1 witness'lı child varsa node mevcut.

### Struct/Enum/Type Listesi

#### `complete::Node<Child>`
```rust
pub struct Node<Child: Clone> {
    hash: Hash,                    // Önceden hesaplanmış (değişmez)
    forgotten: [Forgotten; 4],     // Per-child forgotten versiyonları
    children: Children<Child>,     // 15 farklı düzenleme
}
```
`CachedHash` yerine doğrudan `Hash` — complete node değişmez, hash bir kez hesaplanır.

### Fonksiyonlar

#### `from_children_or_else_hash` (Satır 42-61)
```rust
pub fn from_children_or_else_hash(
    forgotten: [Forgotten; 4],
    children: [Insert<Child>; 4],
) -> Insert<Self>
where Child: GetHash,
{
    match Children::try_from(children) {
        Ok(children) => Insert::Keep(Self { hash: children.hash(), forgotten, children }),
        Err([a, b, c, d]) => Insert::Hash(Hash::node(<Self as Height>::Height::HEIGHT, a, b, c, d))
    }
}
```
Tüm children hash ise → node olarak saklanmaz, sadece hash.

#### `Witness for Node<Child>` (Satır 97-116)
```rust
fn witness(&self, index: impl Into<u64>) -> Option<(AuthPath<Self>, Hash)> {
    let (which_way, index) = WhichWay::at(Self::Height::HEIGHT, index);
    let (child, siblings) = which_way.pick(self.children());
    let siblings = siblings.map(|sibling| sibling.hash());
    let (child, leaf) = child.keep()?.witness(index)?;
    Some((path::Node { siblings, child }, leaf))
}
```

#### `ForgetOwned for Node<Child>` (Satır 118-186)
Çocuğu sil. Tüm çocuklar hash oldu mu? → `Insert::Hash(self.hash)` — node yok edilir.

---

## [internal/complete/node/children.rs]
**Dosya:** `src/internal/complete/node/children.rs` **Satır Sayısı:** 330

### Genel Amaç
`Children<Child>` enum: 4 çocuktan hangilerinin mevcut (witness'lı), hangilerinin sadece hash olduğunu encode eder. 15 variant (4 çocuktan en az 1'i Keep — 0 Keep hali Node olmaz).

### Struct/Enum/Type Listesi

15 variant: `___C`, `__C_`, `__CC`, `_C__`, `_C_C`, `_CC_`, `_CCC`, `C___`, `C__C`, `C_C_`, `C_CC`, `CC__`, `CC_C`, `CCC_`, `CCCC`

Her variant (C=child, _=hash) olarak okuma kolaylığı sağlar.

```rust
pub enum Children<Child> {
    ___C(Arc<___C<Child>>),  // Sadece 3. çocuk mevcut
    ...
    CCCC(Arc<CCCC<Child>>),  // Tüm çocuklar mevcut
}
```

`Arc` ile inner: Her variant kendi büyüklüğünde box — büyük variant diğerlerini büyütmez.

### Matematiksel Formüller
- 2^4 - 1 = 15 non-empty subset (tüm hash = 0 çocuk = bu enum'da değil)
- GetHash: `Hash::node(height, [a, b, c, d].map(|x| x.hash()))`

### Tasarım Kararları
- Her variant için ayrı Arc: `Arc<___C<Child>>` boyutu sadece bu kombinasyon için gerekli belleği alır.
- Alternatif `Arc<[Insert<Child>; 4]>` hepsini same size'da tutar ama daha fazla bellek.

---

## [internal/complete/node/children/shape.rs]
**Dosya:** `src/internal/complete/node/children/shape.rs` **Satır Sayısı:** 66

15 adet tuple struct tanımı. Her biri bir children düzenlemesi.

```rust
pub struct ___C<Child>(pub Hash, pub Hash, pub Hash, pub Child);
pub struct __C_<Child>(pub Hash, pub Hash, pub Child, pub Hash);
// ... 13 tane daha
pub struct CCCC<Child>(pub Child, pub Child, pub Child, pub Child);
```

Sıralama: `(hash/child, hash/child, hash/child, hash/child)` sol'dan sağa.

---

## [internal/complete/tier.rs]
**Dosya:** `src/internal/complete/tier.rs` **Satır Sayısı:** 111

### Genel Amaç
`complete::Tier<Item>`: Finalize edilmiş, 8 derinlikli sparse quadtree.

```rust
pub struct Tier<Item: GetHash + Height + Clone> {
    pub(in super::super) inner: Nested<Item>,
}
// Nested<Item> = N<N<N<N<N<N<N<N<L<Item>>>>>>>> (8 seviye)
```

### Tasarım Kararları
- `complete::Tier`'da `Complete` trait implementasyonu: `type Focus = frontier::Tier<Item::Focus>` — frontier/complete duality tamamlanır.

---

## [internal/complete/top.rs]
**Dosya:** `src/internal/complete/top.rs` **Satır Sayısı:** 78

### Genel Amaç
`complete::Top<Item>`: Finalize edilmiş complete ağacın en tepesi. `frontier::Top`'ın complete karşılığı.

---

## [r1cs.rs]
**Dosya:** `src/r1cs.rs` **Satır Sayısı:** 335

### Genel Amaç
TCT authentication path'ini R1CS (Rank-1 Constraint System) devresi olarak doğrulama. arkworks framework ile ZK proof devre implementasyonu.

### Struct/Enum/Type Listesi

#### `PositionVar` (Satır 28-34)
```rust
pub struct PositionVar {
    pub position: FqVar,    // Fq alanında pozisyon
    pub bits: [Boolean<Fq>; 48],  // 48 bit pozisyon (3 × 16)
}
```
48 bit = epoch(16) + block(16) + commitment(16).

#### `MerkleAuthPathVar` (Satır 104-106)
```rust
pub struct MerkleAuthPathVar {
    inner: [[FqVar; 3]; 24],  // 24 seviye × 3 sibling FqVar
}
```

#### `WhichWayVar` (Satır 197-199)
```rust
pub struct WhichWayVar {
    inner: FqVar,  // 0, 1, 2, 3 değerlerinden biri (2 bit)
}
```

#### `StateCommitmentVar` (Satır 277-315)
```rust
pub struct StateCommitmentVar {
    pub inner: FqVar,
}
```

### Fonksiyonlar

#### `AllocVar<Position, Fq> for PositionVar` (Satır 36-61)
```rust
let position = UInt64::new_variable(cs, || Ok(u64::from(inner)), mode)?;
let bits = position.to_bits_le();
for bit in &bits[48..] {
    bit.enforce_equal(&Boolean::Constant(false))?; // Üst 16 bit = 0
}
let inner = Boolean::<Fq>::le_bits_to_fp_var(&bits[0..48])?;
```
48 bit constraint: üst 16 bit her zaman 0.

#### `PositionVar::commitment/block/epoch` (Satır 71-83)
```rust
pub fn commitment(&self) -> Result<FqVar, SynthesisError> {
    Boolean::<Fq>::le_bits_to_fp_var(&self.bits[0..16])
}
pub fn block(&self) -> Result<FqVar, SynthesisError> {
    Boolean::<Fq>::le_bits_to_fp_var(&self.bits[16..32])
}
pub fn epoch(&self) -> Result<FqVar, SynthesisError> {
    Boolean::<Fq>::le_bits_to_fp_var(&self.bits[32..48])
}
```

#### `MerkleAuthPathVar::hash_node` (Satır 138-148)
```rust
pub fn hash_node(cs, height_var: FqVar, a, b, c, d) -> Result<FqVar, SynthesisError> {
    let domain_separator = FqVar::new_constant(cs.clone(), *DOMAIN_SEPARATOR)?;
    poseidon377::r1cs::hash_4(cs, &(domain_separator + height_var), (a, b, c, d))
}
```
R1CS Poseidon hash = native Poseidon hash'in constraint versiyonu.

#### `MerkleAuthPathVar::verify` (Satır 151-191) — ANA DEVRE
```rust
pub fn verify(&self, cs, enforce: &Boolean<Fq>, position_bits: &[Boolean<Fq>],
              anchor_var: FqVar, commitment_var: FqVar) -> Result<(), SynthesisError> {
    let domain_separator = FqVar::new_constant(cs.clone(), *DOMAIN_SEPARATOR)?;
    let leaf_var = poseidon377::r1cs::hash_1(cs.clone(), &domain_separator, commitment_var)?;

    let mut previous_level = leaf_var;

    for height_value in 1..=24 {
        let height_var = FqVar::new_constant(cs.clone(), Fq::from(height_value as u64))?;
        let which_way_var = WhichWayVar::at(height_value, position_bits)?;
        let siblings = &self.inner[(24 - height_value) as usize];
        let [leftmost, left, right, rightmost] =
            which_way_var.insert(previous_level.clone(), siblings.clone())?;
        let parent = MerkleAuthPathVar::hash_node(cs.clone(), height_var, leftmost, left, right, rightmost)?;
        previous_level = parent;
    }

    anchor_var.conditional_enforce_equal(&previous_level, enforce)?;
    Ok(())
}
```

**R1CS Devre Algoritması:**
1. `leaf_hash = poseidon_hash_1(DOMAIN_SEPARATOR, commitment)` — yaprak hash constraint
2. Height 1'den 24'e kadar her seviyede:
   - `which_way = position_bits[2*(h-1)] + 2 * position_bits[2*h-1]` — 2 bit constraint
   - Kardeşler doğru sıraya koyulur (conditional select constraints)
   - `parent = poseidon_hash_4(DOMAIN_SEPARATOR + height, a, b, c, d)` — hash constraint
3. Son level = anchor (global root) ile eşitlik constraint

**Constraint sayısı tahmini:**
- Her height için: `poseidon_hash_4` ≈ 200-400 constraint + `conditional_select` × 4 ≈ 20-40 constraint
- 24 height × ~440 = ~10,560 constraint (yaklaşık)

#### `WhichWayVar::at` (Satır 205-218)
```rust
pub fn at(height: u8, position_bits: &[Boolean<Fq>]) -> Result<WhichWayVar, SynthesisError> {
    let shift = 2 * (height - 1);
    let bit_1 = position_bits[shift as usize].clone();
    let bit_2 = position_bits[(shift + 1) as usize].clone();
    let inner = FqVar::from(bit_1) + FqVar::constant(Fq::from(2u64)) * FqVar::from(bit_2);
    Ok(WhichWayVar { inner })
}
```
`inner = bit_1 + 2 * bit_2` → 0, 1, 2, 3 değerlerinden birini verir.

#### `WhichWayVar::insert` (Satır 222-273)
4 `is_eq` → 4 `Boolean` → 4 `conditionally_select` çağrısı. Sırayı belirleyen constraint'ler.

### Trait Implementasyonları
- `ToConstraintField<Fq> for Position`: 64 ayrı bit → field elements
- `AllocVar<Proof, Fq> for MerkleAuthPathVar`: 24 × 3 FqVar allocation

### Unsafe Bloklar
Yok.

### Tasarım Kararları
- `conditional_enforce_equal` ile `enforce` boolean: Proof'u her zaman doğrulayabilir veya koşullu doğrulayabilir (batch proving için).
- `auth_path[(24 - height_value)]`: İç temsilde auth_path leaf'ten root'a sıralı, ama verify döngüsü leaf'ten başlar → ters indeks.

### Nova Shield Chain İçin Dersler
- Herhangi bir TCT tabanlı note spending ZK proof devresine bu kodu direkt entegre edebiliriz.
- `WhichWayVar::insert` pattern: Merkle path selection'ı constraint olarak encode etme — her ZK Merkle proof devresinde gerekli.
- `conditional_enforce_equal`: Groth16/PLONK devresinde opsiyonel constraint — batch proof için kritik.

---

## [storage.rs]
**Dosya:** `src/storage.rs` **Satır Sayısı:** 330

### Genel Amaç
Incremental serialization/deserialization için storage trait'leri. Async + sync ikisi de desteklenir.

### Trait Listesi

#### `AsyncRead` / `Read`
- `position()`, `forgotten()`, `hash(position, height)`, `hashes()`, `commitment(position)`, `commitments()`

#### `AsyncWrite: AsyncRead` / `Write: Read`
- `add_hash(position, height, hash, essential)`, `add_commitment(position, commitment)`
- `delete_range(below_height, positions)` — Forgotten node'ların silinmesi
- `set_position(StoredPosition)`, `set_forgotten(Forgotten)` — monoton artış garantili

#### `StoredPosition`
```rust
pub enum StoredPosition {
    Position(Position),
    Full,
}
```

#### `Update` enum (Satır 229-240)
```rust
pub enum Update {
    SetPosition(StoredPosition),
    SetForgotten(Forgotten),
    StoreCommitment(StoreCommitment),
    StoreHash(StoreHash),
    DeleteRange(DeleteRange),
}
```

#### `Updates` struct (Satır 279-290)
`FromIterator<Update>` + `Iterator<Item = Update>` — incremental serialization için update batch'i.

### Tasarım Kararları
- `essential: bool` flag'i: Zorunlu olmayan hash'ler storage'da atlanabilir (daha yavaş deserialization ama daha az depolama).
- Monoton artış garantisi: `set_position` ve `set_forgotten` geri giderse hata — corruption önlemi.

### Nova Shield Chain İçin Dersler
- RocksDB üzerinde bu storage trait'lerini implement edebiliriz.
- `essential` hash optimizasyonu: Her hash'i saklamak zorunda değiliz, sadece "essential" olanları.
- `Updates` struct: Transaction batch'i — atomic write için.

---

## [storage/deserialize.rs]
**Dosya:** `src/storage/deserialize.rs` **Satır Sayısı:** 131

### Genel Amaç
Tree'yi storage'dan yeniden oluşturma. 3 adımlı deserialization.

### Fonksiyonlar

#### `from_reader` / `from_async_reader`
3 adım:
1. `LoadCommitments::new(position, forgotten)` → uninitalized frontier
2. `load_commitments.insert(position, commitment)` × N → out-of-order commitment ekleme
3. `load_commitments.load_hashes()` → `LoadHashes`
4. `load_hashes.insert(position, height, hash)` × M → hash'leri yerleştir
5. `load_hashes.finish()` → `Tree`

#### `LoadCommitments::new` (Satır 53-63)
`OutOfOrder::uninitialized(position, forgotten)` ile boş frontier oluşturur.

#### `LoadHashes::finish` (Satır 102-105)
```rust
pub fn finish(mut self) -> Tree {
    self.inner.finish_initialize();
    Tree::unchecked_from_parts(self.index, self.inner)
}
```
`finish_initialize()`: Tüm `Hash::uninitialized()` değerlerini gerçek değerlerle değiştirir.

### Test'ler (proptest)
`uninitialized_produces_correct_position_and_forgotten`: Position ve forgotten doğru set edilmiş mi?

---

## [storage/serialize.rs]
**Dosya:** `src/storage/serialize.rs` **Satır Sayısı:** 441

### Genel Amaç
Tree'nin incremental serializasyonu. Sadece son serialization'dan sonra değişen kısımları serialize eder.

### Struct/Enum/Type Listesi

#### `Serializer` (Satır 12-17)
```rust
struct Serializer {
    last_position: StoredPosition,
    last_forgotten: Forgotten,
}
```

### Fonksiyonlar

#### `Serializer::hashes` (Satır 97-164)
Depth-first pre-order traversal. Her node için:
- "Fresh" mi? (last_position'dan sonra geldi mi?)
- "Essential" mi? (yeniden hesaplanamaz mı?)
- "Complete" mi? (frontier'da değil mi?)

Sadece bu kriterleri karşılayanlar yield edilir.

#### `Serializer::commitments` (Satır 167-203)
Sadece fresh, witnessed commitment'ları yield eder.

#### `Serializer::forgotten` (Satır 205-263)
Silinmiş node'ları bulur → `delete_range` update'leri üretir.

#### `updates` (Satır 349-440) — ANA FONKSİYON
Position, forgotten, commitment, hash/deletion update'lerini birleştirir. Iterator olarak döner.

**Incremental serialization mantığı:**
- `essential` hash: Yeniden hesaplanamaz → mutlaka saklan
- `!essential && complete` hash: Hesaplanabilir ama saklanırsa deserialization hızlanır
- `frontier_leaf` hash: Değişmeyecek → saklanabilir
- Hash::one() hiçbir zaman saklanmaz (deserialization'da otomatik eklenir)

---

## [storage/in_memory.rs]
**Dosya:** `src/storage/in_memory.rs` **Satır Sayısı:** 248

### Genel Amaç
Test için bellek içi storage backend. `BTreeMap<Position, BTreeMap<u8, Hash>>`.

### Struct/Enum/Type Listesi

#### `InMemory`
```rust
pub struct InMemory {
    sparse: bool,    // Sadece essential hash'leri sakla
    position: StoredPosition,
    forgotten: Forgotten,
    hashes: BTreeMap<Position, BTreeMap<u8, Hash>>,
    commitments: BTreeMap<Position, StateCommitment>,
}
```

#### `Error` enum
- `DuplicateWriteCommitment`: Mevcut commitment'ı üzerine yaz
- `RepeatedWriteHash`: Aynı hash tekrar yazılıyor
- `OverwrittenHash`: Farklı hash ile üzerine yaz
- `EssentialHashHasChildren`: Essential hash'in altında children var (mantık hatası)
- `PositionDidNotIncrease` / `ForgottenDidNotIncrease`: Monoton artış ihlali

---

## [storage/serialize/fq.rs]
**Dosya:** `src/storage/serialize/fq.rs` **Satır Sayısı:** 37

### Genel Amaç
`Fq` alanı elemanının serde serialization'ı. 32 byte LE encoding.

```rust
pub fn serialize<S>(fq: &Fq, serializer: S) -> ... {
    serializer.serialize_bytes(&fq.to_bytes())
}
pub fn deserialize<'de, D>(deserializer: D) -> Result<Fq, D::Error> {
    // FqVisitor ile 32 byte → Fq::from_bytes_checked
}
```

---

## [structure.rs]
**Dosya:** `src/structure.rs` **Satır Sayısı:** 436

### Genel Amaç
Ağaç yapısını dinamik olarak keşfetmek için API. `Node` struct ile runtime introspection.

### Struct/Enum/Type Listesi

#### `Kind` (Satır 56-68)
```rust
pub enum Kind {
    Leaf { commitment: Option<StateCommitment> },
    Internal { height: u8 },
}
```

#### `Place` (Satır 80-89)
```rust
pub enum Place { Complete, Frontier }
```

#### `structure::Node` (Satır 101-106)
```rust
pub struct Node<'tree> {
    offset: u64,
    global_position: Option<Position>,
    this: HashOrNode<'tree>,
}
```

### Fonksiyonlar

#### `Node::place` (Satır 298-321)
```rust
pub fn place(&self) -> Place {
    if let Some(global_position) = self.global_position() {
        if let Some(frontier_tip) = u64::from(global_position).checked_sub(1) {
            let height = self.height();
            let position = u64::from(self.position());
            if position >> (height * 2) == frontier_tip >> (height * 2) {
                Place::Frontier  // Bu node'un prefix'i frontier tip ile eşleşiyor
            } else { Place::Complete }
        } else { Place::Frontier }
    } else { Place::Complete }  // Tam dolu → tüm node'lar Complete
}
```

**Matematiksel formül:** `node_on_frontier = (node_pos >> (height*2)) == (frontier_tip >> (height*2))`

#### `Node::range` (Satır 287-290)
Bu node'un altındaki commitment aralığı: `[position, position + stride)`.

### `Any` trait (sealed, crate-internal)
```rust
pub(crate) trait Any<'tree>: GetHash + sealed::Sealed {
    fn children(&'tree self) -> Vec<HashOrNode<'tree>>;
    fn kind(&self) -> Kind;
    fn forgotten(&self) -> Forgotten;
}
```

### Test'ler
- `indexing_correct`: 100 commitment → her node'un index'i doğru
- `place_correct`: 100 commitment → frontier/complete node'lar doğru
- `height_correct`: Tree'deki tüm node'ların yükseklikleri doğru (tree root = 24)

---

## [validate.rs]
**Dosya:** `src/validate.rs` **Satır Sayısı:** 344

### Genel Amaç
Ağacın internal consistency'sini doğrulayan fonksiyonlar. Pahalı ama kritik.

### Fonksiyonlar

#### `validate::index(tree)` (Satır 18-72)
Her leaf için: index'teki commitment ↔ ağaçtaki commitment eşleşiyor mu? Hash doğru mu?

#### `validate::all_proofs(tree)` (Satır 125-150)
Her witness'lı commitment için: proof oluştur, root'a karşı doğrula.

#### `validate::cached_hashes(tree)` (Satır 190-231)
Tüm cached hash'leri sil, yeniden hesapla, eski değerle karşılaştır.

#### `validate::forgotten(tree)` (Satır 264-308)
Her node'un `forgotten()` değeri çocuklarının max'ına eşit mi?

### Tasarım Kararları
- Bu fonksiyonlar normalde production'da çağrılmaz — sadece debug/test.
- `cached_hashes` post-order traversal gerektiriyor (yorumda açıklanmış): çocuklar önce kontrol edilmeli.

### Nova Shield Chain İçin Dersler
- Geliştirme sürecinde bu validation fonksiyonlarını regularly çalıştır.
- Özellikle `all_proofs` — her commitment hâlâ doğrulanabilir mi?

---

## [error.rs]
**Dosya:** `src/error.rs` **Satır Sayısı:** 124

### Genel Amaç
Tüm hata tipleri. `thiserror` derive ile.

### Struct/Enum/Type Listesi
- `InsertError`: `Full | EpochFull | BlockFull`
- `InsertBlockError`: `Full(block::Finalized) | EpochFull(block::Finalized)`
- `InsertEpochError(epoch::Finalized)`
- `proof::DecodeError`, `proof::VerifyError`, `proof::PathDecodeError`
- `block::InsertError`, `block::RootDecodeError`
- `epoch::InsertError`, `epoch::RootDecodeError`, `epoch::InsertBlockError`

### Tasarım Kararları
- Hata durumunda orijinal veri geri döner: `InsertBlockError::Full(block::Finalized)` — eklenemeyen block kaybedilmez.
- `Sync + Send` garantisi test edilmiş.

---

## [block.rs]
**Dosya:** `src/block.rs` **Satır Sayısı:** 200

### Genel Amaç
`block::Builder` + `block::Finalized` + `block::Root`. Paralel inşa için.

### Struct/Enum/Type Listesi

#### `block::Builder`
```rust
pub struct Builder {
    index: HashedMap<StateCommitment, index::within::Block>,
    inner: Arc<frontier::Top<Item>>,
}
```

#### `block::Finalized`
```rust
pub struct Finalized {
    pub(in super::super) index: HashedMap<StateCommitment, index::within::Block>,
    pub(in super::super) inner: Insert<complete::Top<complete::Item>>,
}
```

#### `block::Root(pub Hash)`
- `is_empty_finalized()`: `hash == Hash::one()` — finalize edilmiş boş block
- `is_empty_unfinalized()`: `hash == Hash::zero()` — frontier boş block

### Fonksiyonlar

#### `Builder::insert` (Satır 136-168)
Block kapasitesi: u16::MAX + 1 = 65,536 commitment.

#### `Builder::finalize` (Satır 179-189)
`Arc::try_unwrap` ile clone'u önler: tek referans varsa move et.

---

## [epoch.rs]
**Dosya:** `src/epoch.rs` **Satır Sayısı:** 300

### Genel Amaç
`epoch::Builder` + `epoch::Finalized` + `epoch::Root`. Block builder'a ek olarak `insert_block` ve `end_block` destekler.

### Fonksiyonlar

#### `epoch::Builder::insert_block` (Satır 202-256)
Block'u epoch'a ekler. Önceki açık block finalize edilir. Block root döner.

#### `epoch::Builder::end_block` (Satır 260-279)
Mevcut block'u finalize et. Zaten finalize ise boş block ekle.

---

## [random.rs]
**Dosya:** `src/random.rs` **Satır Sayısı:** 91

### Genel Amaç
Test/fuzzing için rastgele değer üretimi.

### Struct/Enum/Type Listesi

#### `UniformFq` (private struct)
`Distribution<Fq>`: retry-until-valid, `Fq::from_bytes_checked` ile.

`Distribution<StateCommitment>`, `Distribution<Hash>`, `Distribution<Root>`, `Distribution<epoch::Root>`, `Distribution<block::Root>` için implementasyon.

---

## KRİTİK MATEMATİKSEL FORMÜLLER (ÖZET)

### 1. Domain Separator
```
DOMAIN_SEPARATOR = Fq::from_le_bytes_mod_order(blake2b("penumbra.tct"))
```

### 2. Leaf Hash
```
leaf_hash(commitment) = poseidon377_hash_1(DOMAIN_SEPARATOR, commitment.0)
```

### 3. Internal Node Hash
```
node_hash(height, a, b, c, d) = poseidon377_hash_4(DOMAIN_SEPARATOR + height, (a, b, c, d))
```
- `+` burada `Fq` toplama — her height farklı domain
- Padding: Frontier = `Hash::zero()`, Complete = `Hash::one()`

### 4. Pozisyon Kodlaması (48-bit)
```
position(epoch, block, commitment) = (epoch << 32) | (block << 16) | commitment
```
Her bölüm 16 bit (0..=65535 arası).

### 5. Quadtree Yön Belirleme
```
direction_at_height(h, position) = (position >> (2*(h-1))) & 0b11
child_position_at_height(h, position) = position & !(0b11 << ((h-1)*2))
```

### 6. Root Yeniden Hesaplama (Auth Path Verification)
```
hash_0 = leaf_hash(commitment)
hash_h = node_hash(h, insert(hash_{h-1}, siblings_h, direction_h))
root = hash_24
```

### 7. R1CS Devre (Constraint Sayısı)
```
Per height: poseidon_hash_4 constraints + 4 * conditional_select constraints
Total: ~24 * (poseidon_4_constraints + 40) constraints
```

### 8. Frontier Node Pozisyon Hesabı
```
node.position = siblings_count * 4^child_height + focus.position
```

---

## GÜVENLİK ANALİZİ

### Güçlü Güvenceler
1. **Type-level height guarantee**: Yanlış yükseklikte hash hesaplamak derleme hatası.
2. **Domain separation**: Her height farklı hash domain — farklı height'lardaki çarpışmalar imkânsız.
3. **Frontier/Complete hash ayrımı**: `zero` vs `one` padding — frontier finalizasyon saldırısı imkânsız.
4. **Commitment uniqueness**: Aynı commitment tekrar eklenirse eski silinir.
5. **`#![deny(clippy::unwrap_used)]`**: Panic yokluğu garantisi.
6. **Canonical encoding**: `Fq::from_bytes_checked` — alan dışı değerler reddedilir.

### Potansiyel Dikkat Noktaları
1. **`unsafe` yok**: Tüm crate tamamen safe Rust.
2. **`Arc::make_mut()`**: CoW semantiği — çoklu referanslarda beklenmedik deep copy olabilir.
3. **`OutOfOrder` trait**: Deserializasyon sırasında internal invariant'lar bypass edilir — sadece güvenilir kaynaklardan gelen veri ile kullanılmalı.
4. **Forgotten versiyonlama**: `Forgotten([u8; 6])` — max 2^48 forget işlemi. Aşılması durumunda `checked_add` panic atar.

---

## UNSAFE BLOKLAR

**Tüm 41 dosyada**: SIFIR unsafe blok. TCT tamamen safe Rust.

---

## TEST KAPSAMI

### Unit Testler
- `path::test::directions_of_index_check` — Temel yönelim doğruluğu
- `path::test::which_way_indices_correct` (proptest) — Tüm geçerli (height, index) çiftleri
- `path::test::which_way_direction_correct` (proptest) — Geri dönüştürülebilirlik
- `index::test::u64_convert_eternity_inverse` (proptest) — Pozisyon round-trip
- `hash::test::forgotten_increments` — Forgotten monotonluk
- `cache::test::cached_hash_size` — Boyut assertion
- `structure::test::indexing_correct` — Node indexing
- `structure::test::place_correct` — Frontier/Complete placement
- `structure::test::height_correct` — Yükseklik tutarlılığı
- `tier::test::position_advances_by_one` — Tier kapasitesi
- `top::test::position_advances_by_one` — Top kapasitesi
- `deserialize::test::uninitialized_produces_correct_position_and_forgotten` (proptest)
- `block::test::insert_error_sync_send` — Error Sync+Send
- `error::test::insert_errors_sync_send` — Error Sync+Send

### Validate Testleri (Pahalı, runtime)
- `validate::index(tree)` — Index integrity
- `validate::all_proofs(tree)` — Her proof doğrulanabilir
- `validate::cached_hashes(tree)` — Hash cache doğruluğu
- `validate::forgotten(tree)` — Forgotten versiyonlama tutarlılığı

---

## NOVA SHIELD CHAIN İÇİN DERSLER (MASTER LİSTE)

### Kriptografi
1. **Poseidon377 hash**: BLS12-377 üzerinde, `hash_1` (leaf) ve `hash_4` (node) kullan.
2. **Domain separator**: `blake2b("nova-shield.tct")` → `Fq::from_le_bytes_mod_order`. Her crate için farklı.
3. **Height domain separation**: `DOMAIN_SEPARATOR + height` — mutlak zorunlu. Olmadan second preimage attack.
4. **Frontier/Complete hash padding ayrımı**: `Hash::zero()` vs `Hash::one()` — mutlak zorunlu.
5. **Canonical encoding everywhere**: `Fq::from_bytes_checked` — hiç trust etme.

### Mimari
6. **3 katmanlı TCT**: Block (65K) → Epoch (65K) → Tree (65K) = 281T commitment. Nova Shield için yeterli.
7. **`Witness::Keep` vs `Witness::Forget`**: Wallet sadece kendi commitments'larını `Keep`, diğerlerini `Forget` ile ekler.
8. **`Arc::make_mut()` + CoW**: Block başına kopyalama yerine sadece değişen path kopyalanır.
9. **`im::HashMap`**: Commitment index için immutable hashmap — concurrent read için ideal.
10. **Newtype index tipleri**: `Commitment(u16)`, `Block(u16)`, `Epoch(u16)` — type safety + 48-bit encoding.

### ZK Devre
11. **`MerkleAuthPathVar::verify`**: Note spending devre için bunu doğrudan kullan veya adapte et.
12. **`PositionVar`: 48-bit R1CS pozisyon**: `bits[0..48]` (48 Boolean constraint) + range check.
13. **`WhichWayVar::insert`**: Quadtree path selection constraint — 4 conditional_select.
14. **Constraint sayısı**: ~10K constraint / proof. Groth16 ile kabul edilebilir.

### Storage
15. **Incremental serialization**: Sadece `last_position`'dan sonraki değişiklikleri kaydet. Tam tree yerine delta.
16. **`essential` hash flag**: Zorunlu olmayan hash'ler atlanabilir. RocksDB'de ikincil index olarak sakla.
17. **`Updates` batch**: Atomic write için tüm update'leri önce topla, sonra uygula.
18. **`StoredPosition::Full`**: Tree'nin dolu olduğunu explicit olarak sakla.

### Güvenlik
19. **Commitment uniqueness enforcing**: Aynı commitment tekrar eklenirse eski unwitness edilir — çift harcama önlemi.
20. **`validate` modülü**: Her kritik state değişikliğinden sonra çalıştır (test/staging ortamında).
21. **`#![deny(clippy::unwrap_used)]`**: Tüm Nova Shield crate'lerinde standart.
22. **`OutOfOrder` deserialization**: Sadece kendi serialize ettiğin tree'yi deserialize et — güvenilmeyen kaynaktan alma.

---

## DOSYA BAŞVURU TABLOSU

| Dosya | Ana İşlev | Satır |
|-------|-----------|-------|
| `lib.rs` | Crate root, re-exports | 152 |
| `tree.rs` | Ana Tree API | 885 |
| `commitment.rs` | StateCommitment tipi | 194 |
| `index.rs` | Pozisyon indeks tipleri | 285 |
| `proof.rs` | Public Proof API | 144 |
| `witness.rs` | Witness::Keep/Forget | 55 |
| `error.rs` | Hata tipleri | 124 |
| `block.rs` | Block builder | 200 |
| `epoch.rs` | Epoch builder | 300 |
| `random.rs` | Test yardımcıları | 91 |
| `internal.rs` | İç modül tanımları | 162 |
| `internal/hash.rs` | Hash, GetHash, DOMAIN_SEPARATOR | 325 |
| `internal/hash/cache.rs` | CachedHash (Mutex based) | 74 |
| `internal/hash/option.rs` | OptionHash (sentinel) | 63 |
| `internal/height.rs` | Type-level Peano height | 53 |
| `internal/path.rs` | AuthPath, WhichWay | 365 |
| `internal/proof.rs` | Generic Proof | 116 |
| `internal/insert.rs` | Insert<T> enum | 186 |
| `internal/interface.rs` | Trait tanımları | 165 |
| `internal/three.rs` | Three<T> (max 3 elem) | 260 |
| `internal/frontier/item.rs` | Frontier yaprak | 156 |
| `internal/frontier/leaf.rs` | Frontier yaprak wrapper | 135 |
| `internal/frontier/node.rs` | Frontier iç düğüm | 629 |
| `internal/frontier/tier.rs` | Frontier 8-deep tier | 463 |
| `internal/frontier/top.rs` | Frontier top (boş olabilir) | 317 |
| `internal/complete/item.rs` | Complete yaprak | 112 |
| `internal/complete/leaf.rs` | Complete yaprak wrapper | 96 |
| `internal/complete/node.rs` | Sparse complete düğüm | 332 |
| `internal/complete/node/children.rs` | 15-variant Children | 330 |
| `internal/complete/node/children/shape.rs` | 15 shape struct | 66 |
| `internal/complete/tier.rs` | Complete 8-deep tier | 111 |
| `internal/complete/top.rs` | Complete top | 78 |
| `r1cs.rs` | ZK R1CS devre | 335 |
| `structure.rs` | Dinamik ağaç traversal | 436 |
| `storage.rs` | Storage trait'leri | 330 |
| `storage/deserialize.rs` | Tree deserialization | 131 |
| `storage/serialize.rs` | Incremental serialization | 441 |
| `storage/in_memory.rs` | Test storage | 248 |
| `storage/serialize/fq.rs` | Fq serde | 37 |
| `validate.rs` | Integrity validation | 344 |

**Toplam:** ~41 dosya, ~8.000+ satır analiz edildi.
