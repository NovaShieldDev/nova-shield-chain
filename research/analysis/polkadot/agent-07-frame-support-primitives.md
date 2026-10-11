# Agent-07: frame-support ve Substrate Primitives Tam Analizi

**Tarih:** 2026-07-22
**Agent:** 07 — frame-support + sp-runtime + sp-core
**Kapsam:** substrate/frame/support/src/, substrate/primitives/runtime/src/, substrate/primitives/core/src/
**Toplam Dosya:** 50+ Rust kaynak dosyası, satır satır analiz

---

## BÖLÜM 1: frame-support — Dispatch Sistemi

### 1.1 Temel Tip Hiyerarşisi

Dosya: `substrate/frame/support/src/dispatch.rs`

```rust
// Temel dispatch dönüş tipleri
pub type DispatchResult = Result<(), sp_runtime::DispatchError>;
pub type DispatchResultWithPostInfo = sp_runtime::DispatchResultWithInfo<PostDispatchInfo>;
pub type DispatchErrorWithPostInfo = sp_runtime::DispatchErrorWithPostInfo<PostDispatchInfo>;
```

Bu üç tip FRAME'in dispatch sisteminin omurgasıdır. `DispatchResult` en basit halidir: başarı ya da `DispatchError`. `DispatchResultWithPostInfo` ise dispatch sonrasında ağırlık geri bildirimi (refund) için kullanılır.

### 1.2 RawOrigin Enum

```rust
pub enum RawOrigin<AccountId> {
    Root,                  // Yönetim: en yüksek yetki
    Signed(AccountId),     // İmzalı işlem
    None,                  // İmzasız (unsigned tx veya inherent)
    Authorized,            // Runtime tarafından yetkilendirilmiş
}
```

`Authorized` variant Polkadot SDK'nın yeni özelliğidir. `#[pallet::authorize]` attribute makrosu ile tanımlanan yetkilendirme mantığını temsil eder. Bu, Signed/None ikili modelinin ötesine geçen üçüncü bir kaynak tipidir.

`From<Option<AccountId>>` implementasyonu:
```rust
impl<AccountId> From<Option<AccountId>> for RawOrigin<AccountId> {
    fn from(s: Option<AccountId>) -> RawOrigin<AccountId> {
        match s {
            Some(who) => RawOrigin::Signed(who),
            None => RawOrigin::None,
        }
    }
}
```

### 1.3 Parameter Trait

```rust
pub trait Parameter:
    Codec + DecodeWithMemTracking + EncodeLike + Clone + Eq + fmt::Debug + scale_info::TypeInfo
{}
impl<T> Parameter for T where
    T: Codec + DecodeWithMemTracking + EncodeLike + Clone + Eq + fmt::Debug + scale_info::TypeInfo
{}
```

Blanket implementasyon. Tüm pallet call argümanları bu trait'i otomatik kazanır. `DecodeWithMemTracking` yeni bir ek — WASM heap'ini izlemek için.

### 1.4 DispatchClass Enum

```rust
pub enum DispatchClass {
    Normal,        // Normal kullanıcı işlemi
    Operational,   // Operasyonel işlem (öncelikli)
    Mandatory,     // Zorunlu: her zaman dahil edilir (inherentler)
}
```

**Kritik:** `Mandatory` sınıfı blok ağırlığı limitini aşabilir. Bu özellikle tehlikelidir çünkü kötü niyetli bir validator aşırı ağır bloklar üretebilir. Bu yüzden bu sınıf yalnızca inherent extrinsic'ler için önerilir.

`DispatchClass::all()` → `[Normal, Operational, Mandatory]`
`DispatchClass::non_mandatory()` → `[Normal, Operational]`

### 1.5 DispatchInfo Struct

```rust
pub struct DispatchInfo {
    pub call_weight: Weight,        // Call'ın kendisinin ağırlığı
    pub extension_weight: Weight,   // Transaction extension ağırlığı
    pub class: DispatchClass,
    pub pays_fee: Pays,
}

impl DispatchInfo {
    pub fn total_weight(&self) -> Weight {
        self.call_weight.saturating_add(self.extension_weight)
    }
}
```

`total_weight()` sadece iki bileşeni toplar — saturating add, overflow yoktur.

### 1.6 PostDispatchInfo ve Ağırlık Refund Mekanizması

```rust
pub struct PostDispatchInfo {
    pub actual_weight: Option<Weight>,  // None = en kötü durum ağırlığı kullan
    pub pays_fee: Pays,
}
```

`calc_actual_weight` implementasyonu:

```rust
pub fn calc_actual_weight(&self, info: &DispatchInfo) -> Weight {
    if let Some(actual_weight) = self.actual_weight {
        let info_total_weight = info.total_weight();
        if actual_weight.any_gt(info_total_weight) {
            log::error!(
                target: crate::LOG_TARGET,
                "Post dispatch weight is greater than pre dispatch weight. \
                Pre dispatch weight may underestimating the actual weight. ..."
            );
        }
        actual_weight.min(info.total_weight())
    } else {
        info.total_weight()
    }
}
```

**Tasarım Kararı:** Post dispatch ağırlığı, pre dispatch ağırlığını geçemez. Eğer geçerse hata loglanır ama `min()` ile kırpılır. Bu sistem dispatch'i daha az maliyetli yapmayı sağlar ama hiçbir zaman daha maliyetli kılmaz (çünkü fee zaten ödendi).

`calc_unspent` = `info.total_weight() - actual_weight` — iade edilen ağırlık.

### 1.7 Pays Enum

```rust
pub enum Pays {
    Yes,  // Ücret öder (default)
    No,   // Ücretsiz
}

impl Default for Pays {
    fn default() -> Self { Self::Yes }
}
```

Önemli: `pays_fee` belirleme mantığı pre-dispatch bilgisine göre yapılır:
```rust
pub fn pays_fee(&self, info: &DispatchInfo) -> Pays {
    // Pre-dispatch No ise, post-dispatch değerinden bağımsız No
    // Aksi takdirde post-dispatch değerine bak
}
```

### 1.8 Callable Trait

```rust
pub trait Callable<T> {
    type RuntimeCall: UnfilteredDispatchable + Codec + Clone + PartialEq + Eq;
}
pub type CallableCallFor<A, R> = <A as Callable<R>>::RuntimeCall;
```

### 1.9 CheckIfFeeless Trait

```rust
pub trait CheckIfFeeless {
    type Origin;
    fn is_feeless(&self, origin: &Self::Origin) -> bool;
}
```

`#[pallet::feeless_if]` attribute makrosu ile işaretlenen extrinsic'ler için otomatik implement edilir. Bu kapı yalnızca "feeless" olarak işaretlenmiş call'ların ücret muafiyetini kontrol eder.

---

## BÖLÜM 2: Storage Sistemi — Derinlemesine Analiz

### 2.1 Storage Key Encoding Şeması

**StorageValue:** `Twox128(pallet_prefix) ++ Twox128(storage_prefix)` → 32 byte sabit uzunlukta key

Kaynak (`storage/generator/value.rs:27-48`):
```rust
pub trait StorageValue<T: FullCodec> {
    fn pallet_prefix() -> &'static [u8];
    fn storage_prefix() -> &'static [u8];
    fn storage_value_final_key() -> [u8; 32];
    // ...
}
```

Test'ten doğrulanmış encoding (`types/value.rs:369`):
```rust
assert_eq!(A::hashed_key().to_vec(), [twox_128(b"test"), twox_128(b"foo")].concat());
```

**StorageMap:** `Twox128(pallet_prefix) ++ Twox128(storage_prefix) ++ Hasher(key)`

KeyLen hesabı (`types/map.rs:94`):
```rust
fn get() -> u32 {
    let z = Hasher::max_len::<Key>() + Twox128::max_len::<()>() * 2;
    z as u32
}
```

**StorageDoubleMap:** `Twox128(pallet_prefix) ++ Twox128(storage_prefix) ++ Hasher1(key1) ++ Hasher2(key2)`

KeyLen (`types/double_map.rs:143-145`):
```rust
let z = Hasher1::max_len::<Key1>() + Hasher2::max_len::<Key2>() + Twox128::max_len::<()>() * 2;
```

**StorageNMap:** `Twox128(pallet_prefix) ++ Twox128(storage_prefix) ++ Hasher1(key1) ++ ... ++ HasherN(keyN)`

NMap kullanımı (`types/nmap.rs:61-82`):
```rust
pub type Foo<T> = StorageNMap<
    _,
    (
        NMapKey<Blake2_128Concat, u8>,
        NMapKey<Identity, u16>,
        NMapKey<Twox64Concat, u32>
    ),
    u64,
    ValueQuery,
>;
```

### 2.2 StorageValue — Tam API Analizi

Struct tanımı:
```rust
pub struct StorageValue<Prefix, Value, QueryKind = OptionQuery, OnEmpty = GetDefault>(
    core::marker::PhantomData<(Prefix, Value, QueryKind, OnEmpty)>
);
```

4 generic parametre:
- `Prefix: StorageInstance` — pallet ve storage adı
- `Value: FullCodec` — SCALE encode/decode edilebilir değer
- `QueryKind` — `OptionQuery` (default) veya `ValueQuery`
- `OnEmpty: Get<QueryKind::Query>` — boş değer için default

Tüm metotlar:
```rust
StorageValue::hashed_key() -> [u8; 32]   // Sabit 32 byte key
StorageValue::exists() -> bool             // Depolamada varlık kontrolü
StorageValue::get() -> QueryKind::Query    // Değeri oku (Option<V> veya V)
StorageValue::try_get() -> Result<V, ()>  // Sadece Some(v) ise Ok
StorageValue::put<Arg: EncodeLike<V>>(v)  // Yaz
StorageValue::set(q: QueryKind::Query)    // QueryKind üzerinden yaz/sil
StorageValue::mutate<R, F: FnOnce(&mut Query) -> R>(f) -> R
StorageValue::mutate_extant<R: Default, F>(f) -> R  // Sadece varsa mutate et
StorageValue::try_mutate<R, E, F>(f) -> Result<R, E>
StorageValue::mutate_exists<R, F: FnOnce(&mut Option<V>) -> R>(f) -> R
StorageValue::try_mutate_exists<R, E, F>(f) -> Result<R, E>
StorageValue::kill()                       // Sil
StorageValue::take() -> QueryKind::Query   // Al ve sil
StorageValue::append<Item>(item)           // Vec'e ekle (decode etmeden)
StorageValue::decode_len() -> Option<usize> // Vec uzunluğunu decode etmeden oku
StorageValue::decode_non_dedup_len() -> Option<usize>  // Duplicate dahil uzunluk
StorageValue::try_append<Item>(item) -> Result<(), ()> // BoundedVec'e ekle
StorageValue::translate<O: Decode, F>(f) -> Result<Option<V>, ()> // Tip dönüşümü
```

`translate` önemlidir: storage migration için eski tipten yeni tipe dönüşüm yapar. `decode_len()` ise Vec'in tamamını decode etmeden uzunluğunu okur — bu büyük listeler için kritik bir optimizasyon.

### 2.3 StorageMap — Tam API Analizi

```rust
pub struct StorageMap<
    Prefix, Hasher, Key, Value,
    QueryKind = OptionQuery,
    OnEmpty = GetDefault,
    MaxValues = GetDefault,
>(PhantomData<...>);
```

7 generic parametre. `MaxValues: Get<Option<u32>>` — iterasyon sınırı.

Kritik metotlar:
```rust
StorageMap::hashed_key_for<KeyArg: EncodeLike<Key>>(key) -> Vec<u8>
StorageMap::contains_key(key) -> bool
StorageMap::get(key) -> QueryKind::Query
StorageMap::try_get(key) -> Result<Value, ()>
StorageMap::swap(key1, key2)          // İki key'in değerlerini değiştir
StorageMap::set(key, q)
StorageMap::insert(key, val)
StorageMap::remove(key)
StorageMap::mutate(key, f)
StorageMap::mutate_exists(key, f)
StorageMap::try_mutate(key, f)
StorageMap::try_mutate_exists(key, f)
StorageMap::take(key) -> QueryKind::Query
StorageMap::iter() -> PrefixIterator    // Tüm map'i iterate et
StorageMap::iter_from(start) -> PrefixIterator
StorageMap::iter_keys() -> KeyPrefixIterator
StorageMap::drain() -> PrefixIterator  // Iterate ederek sil
StorageMap::translate<O, F>(f)         // Migration
StorageMap::clear(limit, cursor) -> MultiRemovalResults  // Toplu silme
```

`clear()` metodunun imzası önemli: `cursor: Option<&[u8]>` ile başlangıç noktası verir, `limit: u32` ile siler. Bu paginated deletion için kritik — on-chain depolama temizleme sınırlı blok ağırlığı dahilinde yapılmalıdır.

### 2.4 StorageDoubleMap

```rust
pub struct StorageDoubleMap<
    Prefix, Hasher1, Key1, Hasher2, Key2, Value,
    QueryKind = OptionQuery,
    OnEmpty = GetDefault,
    MaxValues = GetDefault,
>(PhantomData<...>);
```

Tek key1'e göre kısmi silme: `StorageDoubleMap::remove_prefix(key1, limit)` — key1 değeri olan tüm (key1, key2) çiftlerini sil.

Key1'e göre kısmi iterasyon: `StorageDoubleMap::iter_prefix(key1)` — yalnızca key1 eşleşen key-value çiftleri.

Bunlar `StorageMap`'ten ayrışan temel özelliktir.

### 2.5 StorageNMap

```rust
pub struct StorageNMap<
    Prefix, Key, Value,
    QueryKind = OptionQuery,
    OnEmpty = GetDefault,
    MaxValues = GetDefault,
>(PhantomData<...>);
```

`Key` tipi bir `KeyGenerator` tuple'ıdır: `(NMapKey<H1, K1>, NMapKey<H2, K2>, ...)`.

Key encoding (`types/key.rs:79-83`):
```rust
fn final_key<KArg: EncodeLikeTuple<Self::KArg> + TupleToEncodedIter>(key: KArg) -> Vec<u8> {
    H::hash(&key.to_encoded_iter().next().expect("..."))
        .as_ref()
        .to_vec()
}
```

Her key parçası ayrı encode edilip ayrı hash'lenir, sonra concatenate edilir. Bu DoubleMap'in N'ye genişletilmiş halidir.

### 2.6 CountedStorageMap

```rust
pub struct CountedStorageMap<
    Prefix, Hasher, Key, Value,
    QueryKind = OptionQuery,
    OnEmpty = GetDefault,
    MaxValues = GetDefault,
>(PhantomData<...>);
```

Dahili olarak iki storage kullanır:
1. Asıl map: `StorageMap<Prefix, Hasher, Key, Value, ...>`
2. Sayaç: `StorageValue<CounterPrefix, u32>`

Ek overhead:
- Ekleme/silme: 1 ek okuma (key var mı kontrolü) + 1 ek okuma-yazma (sayaç güncelleme)
- Mutate: 2 ek okuma (önceki ve sonraki durum) + 1 ek okuma-yazma

`CountedStorageMap::count() -> u32` — O(1) sayma, iterate etmeye gerek yok.

### 2.7 QueryKind Tasarımı

`OptionQuery` (default):
- `get()` → `Option<Value>`
- `None` = storage'da yok

`ValueQuery`:
- `get()` → `Value`
- Yok ise `OnEmpty::get()` döner
- Varsayılan `OnEmpty = GetDefault` — `Default::default()` kullanır

Örnek test (`types/value.rs:362-460`):
```rust
struct ADefault;
impl crate::traits::Get<u32> for ADefault {
    fn get() -> u32 { 97 }
}

type AValueQueryWithAnOnEmpty = StorageValue<Prefix, u32, ValueQuery, ADefault>;
// Boş iken:
assert_eq!(AValueQueryWithAnOnEmpty::get(), 97);
```

### 2.8 Transactional Storage

Dosya: `storage/transactional.rs`

```rust
pub const TRANSACTION_LEVEL_KEY: &[u8] = b":transaction_level:";
pub const TRANSACTIONAL_LIMIT: Layer = 255;
```

Katman takibi storage key `0x3a7472616e73616374696f6e5f6c6576656c3a` ile yapılır.

```rust
pub fn with_transaction<R, F: FnOnce() -> TransactionOutcome<R>>(f: F) -> Result<R, DispatchError>
pub fn with_storage_layer<R, E, F>(f: F) -> Result<R, E>
pub fn in_storage_layer<R, F: FnOnce() -> R>(f: F) -> R
pub fn with_transaction_unchecked<R, F>(f: F) -> R
```

`inc_transaction_level()` → `TRANSACTIONAL_LIMIT` (255) aşılırsa hata döner.

`StorageLayerGuard`: Drop implementasyonu otomatik `dec_transaction_level()` çağırır — RAII pattern.

**Kritik Tasarım:** Transactional storage overlay'i Substrate'ın kendi storage backend'inden (sp-io) ayrıdır. `start_transaction`, `commit_transaction`, `rollback_transaction` çağrıları sp-io'ya delege edilir. Katman sayacı ise depolamada tutulur.

### 2.9 BoundedVec / BoundedBTreeMap

`BoundedVec<T, S: Get<u32>>` — maksimum uzunluk S tarafından belirlenir.

`StorageTryAppend` implementasyonu:
```rust
impl<T, S: Get<u32>> StorageTryAppend<T> for BoundedVec<T, S> {
    fn bound() -> usize {
        S::get() as usize
    }
}
```

`StorageDecodeLength` — uzunluğu decode etmeden okur (Vec SCALE format'ı compact length prefix ile başlar).

---

## BÖLÜM 3: frame-support Traits Analizi

### 3.1 Currency Trait Ailesi

Dosya: `src/traits/tokens/currency/`

```rust
pub trait Currency<AccountId> {
    type Balance: Member + Parameter + ...;
    type PositiveImbalance: Imbalance<Self::Balance, Opposite = Self::NegativeImbalance>;
    type NegativeImbalance: Imbalance<Self::Balance, Opposite = Self::PositiveImbalance>;

    fn total_balance(who: &AccountId) -> Self::Balance;
    fn can_slash(who: &AccountId, value: Self::Balance) -> bool;
    fn total_issuance() -> Self::Balance;
    fn minimum_balance() -> Self::Balance;
    fn burn(amount: Self::Balance) -> Self::PositiveImbalance;
    fn issue(amount: Self::Balance) -> Self::NegativeImbalance;
    fn free_balance(who: &AccountId) -> Self::Balance;
    fn ensure_can_withdraw(...) -> DispatchResult;
    fn transfer(source, dest, value, existence_requirement) -> DispatchResult;
    fn slash(who, value) -> (Self::NegativeImbalance, Self::Balance);
    fn deposit_into_existing(who, value) -> Result<Self::PositiveImbalance, DispatchError>;
    fn deposit_creating(who, value) -> Self::PositiveImbalance;
    fn withdraw(who, value, reasons, liveness) -> Result<Self::NegativeImbalance, DispatchError>;
    fn make_free_balance_be(who, balance) -> SignedImbalance<...>;
}
```

`Imbalance` pair sistemi kritiktir: `burn` → `PositiveImbalance` (arzı azalttı), `issue` → `NegativeImbalance` (arzı artırdı). Birbirinin `Opposite`'i. Drop edildiğinde otomatik denge sağlanır.

### 3.2 ReservableCurrency

```rust
pub trait ReservableCurrency<AccountId>: Currency<AccountId> {
    fn can_reserve(who, value) -> bool;
    fn slash_reserved(who, value) -> (NegativeImbalance, Balance);
    fn reserved_balance(who) -> Balance;
    fn reserve(who, value) -> DispatchResult;
    fn unreserve(who, value) -> Balance;
    fn repatriate_reserved(slashed, beneficiary, value, status) -> Result<Balance, DispatchError>;
}
```

### 3.3 LockableCurrency

```rust
pub type LockIdentifier = [u8; 8];

pub trait LockableCurrency<AccountId>: Currency<AccountId> {
    type Moment;
    type MaxLocks: Get<u32>;

    fn set_lock(id: LockIdentifier, who, amount, reasons: WithdrawReasons);
    fn extend_lock(id: LockIdentifier, who, amount, reasons: WithdrawReasons);
    fn remove_lock(id: LockIdentifier, who);
}
```

`LockIdentifier` = 8 byte string (ör. `b"staking "`, `b"vesting "`).

### 3.4 Fungible Token Traits (Modern API)

`src/traits/tokens/fungible/`:

```rust
// Okuma
pub trait Inspect<AccountId> {
    type Balance: Balance;
    fn total_issuance() -> Self::Balance;
    fn minimum_balance() -> Self::Balance;
    fn balance(who: &AccountId) -> Self::Balance;
    fn total_balance(who: &AccountId) -> Self::Balance;
    fn reducible_balance(who, preservation, force) -> Self::Balance;
    fn can_deposit(who, amount, provenance) -> DepositConsequence;
    fn can_withdraw(who, amount) -> WithdrawConsequence<Self::Balance>;
}

// Yazma
pub trait Mutate<AccountId>: Inspect<AccountId> {
    fn mint_into(who, amount) -> Result<Self::Balance, DispatchError>;
    fn burn_from(who, amount, precision, force) -> Result<Self::Balance, DispatchError>;
    fn shelve(who, amount) -> Result<Self::Balance, DispatchError>;
    fn restore(who, amount) -> Result<Self::Balance, DispatchError>;
    fn transfer(source, dest, amount, preservation) -> Result<Self::Balance, DispatchError>;
    fn set_balance(who, amount) -> Self::Balance;
}

// Hold (rezerv) API
pub trait MutateHold<AccountId>: InspectHold<AccountId> + Inspect<AccountId> {
    fn hold(reason, who, amount) -> DispatchResult;
    fn release(reason, who, amount, precision) -> Result<Self::Balance, DispatchError>;
    fn burn_held(reason, who, amount, precision, force) -> Result<Self::Balance, DispatchError>;
    fn transfer_on_hold(reason, source, dest, amount, ...) -> Result<Self::Balance, DispatchError>;
}
```

### 3.5 Hooks Trait

```rust
pub trait Hooks<BlockNumber> {
    fn on_initialize(n: BlockNumber) -> Weight { Weight::zero() }
    fn on_finalize(n: BlockNumber) {}
    fn on_idle(n: BlockNumber, remaining_weight: Weight) -> Weight { Weight::zero() }
    fn on_poll(n: BlockNumber, weight: &mut WeightMeter) {}
    fn on_runtime_upgrade() -> Weight { Weight::zero() }
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> { Ok(vec![]) }
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError> { Ok(()) }
    fn offchain_worker(n: BlockNumber) {}
    fn integrity_test() {}
}
```

**Çalışma sırası:**
1. `on_initialize` (her blok başında)
2. Extrinsic'lerin execute edilmesi
3. `on_idle` (kalan ağırlık ile)
4. `on_finalize` (her blok sonunda)
5. `on_poll` (blok üretim zamanında)

`on_idle` → `remaining_weight` ile çalışır. Döndürülen değer kullanılan ağırlıktır.

### 3.6 Get Trait

```rust
// sp-core'dan re-export
pub trait Get<T> {
    fn get() -> T;
}
```

`ConstU32<N>`, `ConstU64<N>`, `ConstBool<B>` gibi const generic implementasyonlar:
```rust
pub struct ConstU32<const N: u32>;
impl Get<u32> for ConstU32<N> {
    fn get() -> u32 { N }
}
```

`parameter_types!` makrosu da `Get` implement eder, ancak runtime'da dinamik olarak değiştirilebilir (governance üzerinden).

### 3.7 Contains / SortedMembers Traits

```rust
pub trait Contains<T> {
    fn contains(t: &T) -> bool;
}

pub trait SortedMembers<T: Ord> {
    fn sorted_members() -> Vec<T>;
    fn count() -> usize;
    fn contains(t: &T) -> bool;
    fn add(t: &T);
    fn remove(t: &T);
}
```

`Everything` / `Nothing` implementasyonları:
```rust
impl<T> Contains<T> for Everything {
    fn contains(_: &T) -> bool { true }
}

impl<T> Contains<T> for Nothing {
    fn contains(_: &T) -> bool { false }
}
```

`InsideBoth<A, B>` — iki filter'ı AND ile birleştirir.

### 3.8 StorageInstance Trait

```rust
pub trait StorageInstance {
    fn pallet_prefix() -> &'static str;
    const STORAGE_PREFIX: &'static str;

    fn prefix_hash() -> [u8; 32] {
        // Twox128(pallet_prefix) ++ Twox128(STORAGE_PREFIX)
        let mut final_key = [0u8; 32];
        final_key[..16].copy_from_slice(&sp_io::hashing::twox_128(
            Self::pallet_prefix().as_bytes()
        ));
        final_key[16..].copy_from_slice(&sp_io::hashing::twox_128(
            Self::STORAGE_PREFIX.as_bytes()
        ));
        final_key
    }
}
```

`#[pallet::storage]` makrosu her storage item için bir `StorageInstance` implement eder.

---

## BÖLÜM 4: Weight Sistemi

### 4.1 Weight Yapısı

`frame-support` `sp-weights` crate'ini re-export eder. `Weight` iki boyutlu:

```rust
pub struct Weight {
    ref_time: u64,    // CPU zaman referansı (picosaniye)
    proof_size: u64,  // Proof of validity boyutu (byte)
}
```

Bu two-dimensional model Polkadot'un SNARK proof sistemi için zorunludur. `ref_time` CPU maliyetini, `proof_size` ise zincir durum kanıtının boyutunu temsil eder.

### 4.2 Benchmark Sabitler

**ExtrinsicBaseWeight** (`weights/extrinsic_weights.rs:55-56`):
```rust
pub const ExtrinsicBaseWeight: Weight =
    Weight::from_parts(WEIGHT_REF_TIME_PER_NANOS.saturating_mul(108_157), 0);
```
= 108,157 nanosaniye ≈ 0.108 ms (Intel Xeon @ 2.60GHz)

Benchmark tarihi: 2024-11-08, 100 tekrar, ortalama değer.

**BlockExecutionWeight** (`weights/block_weights.rs:55-56`):
```rust
pub const BlockExecutionWeight: Weight =
    Weight::from_parts(WEIGHT_REF_TIME_PER_NANOS.saturating_mul(431_614), 0);
```
= 431,614 nanosaniye ≈ 0.43 ms

**RocksDbWeight** (`weights/rocksdb_weights.rs:26-29`):
```rust
pub const RocksDbWeight: RuntimeDbWeight = RuntimeDbWeight {
    read: 25_000 * constants::WEIGHT_REF_TIME_PER_NANOS,
    write: 100_000 * constants::WEIGHT_REF_TIME_PER_NANOS,
};
```
- Read: 25 µs
- Write: 100 µs

**ParityDbWeight** — farklı ağırlıklar (RocksDB'den daha hızlı okuma/yazma oranları).

### 4.3 Weight Sabitleri

```rust
// sp-weights'ten
pub const WEIGHT_REF_TIME_PER_NANOS: u64 = 1_000;
pub const WEIGHT_REF_TIME_PER_MICROS: u64 = 1_000_000;
pub const WEIGHT_REF_TIME_PER_MILLIS: u64 = 1_000_000_000;
pub const WEIGHT_REF_TIME_PER_SECOND: u64 = 1_000_000_000_000;
```

### 4.4 MAX_EXTRINSIC_DEPTH

```rust
// frame-support/src/lib.rs:39
pub const MAX_EXTRINSIC_DEPTH: u32 = 256;
```

Nested extrinsic call derinliği limiti.

---

## BÖLÜM 5: sp-runtime — Runtime Primitives

### 5.1 DispatchError — Tam Enum

Dosya: `primitives/runtime/src/lib.rs:624-660`

```rust
pub enum DispatchError {
    Other(&'static str),                    // Genel hata (serileştirilmez)
    CannotLookup,                           // Lookup başarısız
    BadOrigin,                              // Kötü kaynak
    Module(ModuleError),                    // Pallet-özgü hata
    ConsumerRemaining,                      // Consumer sayısı > 0, hesap silinemez
    NoProviders,                            // Provider yokken hesap oluşturulamaz
    TooManyConsumers,                       // Çok fazla consumer
    Token(TokenError),                      // Token hatası
    Arithmetic(ArithmeticError),            // Aritmetik hata
    Transactional(TransactionalError),      // Transactional storage hatası
    Exhausted,                              // Kaynak tükendi
    Corruption,                             // Storage bozuldu
    Unavailable,                            // Operasyon mevcut değil
    RootNotAllowed,                         // Root origin izin verilmez
    Trie(TrieError),                        // Trie hatası
}
```

### 5.2 ModuleError Yapısı

```rust
pub const MAX_MODULE_ERROR_ENCODED_SIZE: usize = 4;

pub struct ModuleError {
    pub index: u8,                          // Pallet index (metadata'daki)
    pub error: [u8; MAX_MODULE_ERROR_ENCODED_SIZE],  // 4 byte hata kodu
    #[codec(skip)]
    pub message: Option<&'static str>,      // Opsiyonel mesaj (serileştirilmez)
}
```

`PartialEq` implementasyonu:
```rust
impl PartialEq for ModuleError {
    fn eq(&self, other: &Self) -> bool {
        (self.index == other.index) && (self.error == other.error)
        // message dahil değil
    }
}
```

Yani `message` eşitlik karşılaştırmasına dahil edilmez — sadece `index` ve `error` bytes önemlidir.

### 5.3 TokenError

```rust
pub enum TokenError {
    FundsUnavailable,
    OnlyProvider,
    BelowMinimum,
    CannotCreate,
    UnknownAsset,
    Frozen,
    Unsupported,
    CannotCreateHold,
    NotExpendable,
    Blocked,
}
```

### 5.4 ArithmeticError

```rust
pub enum ArithmeticError {
    Underflow,
    Overflow,
    DivisionByZero,
}
```

### 5.5 TransactionalError

```rust
pub enum TransactionalError {
    LimitReached,  // 255 katman aşıldı
    NoLayer,       // Transactional layer bekleniyor ama yok
}
```

### 5.6 MultiSignature Enum

Dosya: `primitives/runtime/src/lib.rs:283-291`

```rust
pub enum MultiSignature {
    Ed25519(ed25519::Signature),    // Deterministik, zk-SNARK dostu
    Sr25519(sr25519::Signature),    // Schnorr üzerine, nonce randomness
    Ecdsa(ecdsa::Signature),        // Ethereum/Bitcoin uyumlu
    Eth(ecdsa::KeccakSignature),    // Ethereum adres derivasyonu
}
```

**Önemli:** `Eth` variant yeni eklenmiştir. `pallet_revive` (Ethereum uyumlu akıllı kontrat pallet'i) için gereklidir. Aynı ECDSA public key'inden hem standart hem ETH adresi türetilebilir.

### 5.7 MultiSigner → AccountId32 Dönüşümü

```rust
impl traits::IdentifyAccount for MultiSigner {
    type AccountId = AccountId32;
    fn into_account(self) -> AccountId32 {
        match self {
            Self::Ed25519(who) => <[u8; 32]>::from(who).into(),
            Self::Sr25519(who) => <[u8; 32]>::from(who).into(),
            Self::Ecdsa(who) => sp_io::hashing::blake2_256(who.as_ref()).into(),
            Self::Eth(who) => {
                let eth_address = &sp_io::hashing::keccak_256(who.as_ref())[12..];
                let mut address = [0xEE; 32];  // 0xEE ile doldu
                address[..20].copy_from_slice(eth_address);
                address.into()
            },
        }
    }
}
```

Derivasyon kuralları:
- `Ed25519`: Public key bytes → direkt AccountId32 (32 byte = 32 byte)
- `Sr25519`: Public key bytes → direkt AccountId32 (32 byte = 32 byte)
- `Ecdsa`: blake2_256(compressed_pub_key_33_bytes) → AccountId32
- `Eth`: keccak_256(pub_key)[12..] = 20 byte eth address → `[0xEE; 32]` ile dolgulayarak AccountId32

### 5.8 MultiSignature Verify

```rust
impl Verify for MultiSignature {
    type Signer = MultiSigner;
    fn verify<L: Lazy<[u8]>>(&self, mut msg: L, signer: &AccountId32) -> bool {
        let who: [u8; 32] = *signer.as_ref();
        match self {
            Self::Ed25519(sig) => sig.verify(msg, &who.into()),
            Self::Sr25519(sig) => sig.verify(msg, &who.into()),
            Self::Ecdsa(sig) => {
                let m = sp_io::hashing::blake2_256(msg.get());
                sp_io::crypto::secp256k1_ecdsa_recover_compressed(sig.as_ref(), &m)
                    .map_or(false, |pubkey| sp_io::hashing::blake2_256(&pubkey) == who)
            },
            Self::Eth(sig) => {
                let m = sp_io::hashing::keccak_256(msg.get());
                sp_io::crypto::secp256k1_ecdsa_recover_compressed(sig.as_ref(), &m)
                    .map_or(false, |pubkey| {
                        &MultiSigner::Eth(pubkey.into()).into_account() == signer
                    })
            },
        }
    }
}
```

**Kritik Fark:**
- ECDSA: `blake2_256(msg)` hash'liyerek recover et
- Eth: `keccak_256(msg)` hash'liyerek recover et

Bu fark Ethereum uyumluluğu için kritiktir.

### 5.9 AnySignature

```rust
pub struct AnySignature(H512);  // 64 byte

impl Verify for AnySignature {
    type Signer = sr25519::Public;
    fn verify<L: Lazy<[u8]>>(&self, mut msg: L, signer: &sr25519::Public) -> bool {
        let msg = msg.get();
        sr25519::Signature::try_from(...)
            .map(|s| s.verify(msg, signer))
            .unwrap_or(false) ||
        ed25519::Signature::try_from(...)
            .map(|s| match ed25519::Public::from_slice(signer.as_ref()) {
                Err(()) => false,
                Ok(signer) => s.verify(msg, &signer),
            })
            .unwrap_or(false)
    }
}
```

`AnySignature` hem sr25519 hem de ed25519 imzaları kabul eder. Geriye uyumluluk için kullanılır.

### 5.10 Justifications

```rust
pub type Justification = (ConsensusEngineId, EncodedJustification);
pub type ConsensusEngineId = [u8; 4];  // GRANDPA, BABE gibi 4 byte ID

pub struct Justifications(Vec<Justification>);
```

`append` methodunda duplicate kontrol:
```rust
pub fn append(&mut self, justification: Justification) -> bool {
    if self.get(justification.0).is_some() {
        return false;  // Aynı engine'den sadece bir justification
    }
    self.0.push(justification);
    true
}
```

### 5.11 Era Encoding

Dosya: `primitives/runtime/src/generic/era.rs`

```rust
pub enum Era {
    Immortal,               // Sonsuza kadar geçerli
    Mortal(Period, Phase),  // Belirli bir dönem
}
```

`mortal()` hesaplama:
```rust
pub fn mortal(period: u64, current: u64) -> Self {
    let period = period.checked_next_power_of_two()
        .unwrap_or(1 << 16)
        .clamp(4, 1 << 16);          // 4 ile 65536 arası, 2'nin kuvveti
    let phase = current % period;
    let quantize_factor = (period >> 12).max(1);
    let quantized_phase = phase / quantize_factor * quantize_factor;
    Self::Mortal(period, quantized_phase)
}
```

Dönem sınırları: [4, 65536], 2'nin kuvveti.

`birth(current)`:
```rust
(current.max(phase) - phase) / period * period + phase
```

`death(current) = birth(current) + period`

### 5.12 UncheckedExtrinsic Format

Dosya: `primitives/runtime/src/generic/unchecked_extrinsic.rs`

```rust
pub const EXTRINSIC_FORMAT_VERSION: ExtrinsicVersion = 5;  // Yeni
pub const LEGACY_EXTRINSIC_FORMAT_VERSION: ExtrinsicVersion = 4;  // Eski

pub const DEFAULT_MAX_CALL_SIZE: usize = 16 * 1024 * 1024;  // 16 MiB

const VERSION_MASK: u8 = 0b0011_1111;  // Düşük 6 bit = versiyon
const TYPE_MASK: u8 = 0b1100_0000;     // Yüksek 2 bit = tip
const BARE_EXTRINSIC: u8 = 0b0000_0000;
const SIGNED_EXTRINSIC: u8 = 0b1000_0000;
const GENERAL_EXTRINSIC: u8 = 0b0100_0000;
```

İlk byte encoding'i: `version | type_bits`

```rust
pub enum Preamble<Address, Signature, ExtensionV0, ExtensionOtherVersions = InvalidVersion> {
    Bare(ExtrinsicVersion),
    Signed(Address, Signature, ExtensionV0),
    General(ExtensionVariant<ExtensionV0, ExtensionOtherVersions>),
}
```

V5 yenilikleri:
- `General` extrinsic: imza zorunlu değil. Yetkilendirme transaction extension içinde yapılır.
- `Bare`: imza yok, extension yok (inherentler için)

---

## BÖLÜM 6: sp-runtime — Traits Modülü

### 6.1 Aritmetik Trait'ler

```rust
// sp-arithmetic'ten re-export
pub use sp_arithmetic::traits::{
    AtLeast32Bit, AtLeast32BitUnsigned, Bounded,
    CheckedAdd, CheckedDiv, CheckedMul, CheckedShl, CheckedShr, CheckedSub,
    Ensure, EnsureAdd, EnsureAddAssign, EnsureDiv, EnsureDivAssign,
    EnsureFixedPointNumber, EnsureFrom, EnsureInto,
    EnsureMul, EnsureMulAssign, EnsureOp, EnsureOpAssign,
    EnsureSub, EnsureSubAssign,
    IntegerSquareRoot, One, SaturatedConversion, Saturating,
    UniqueSaturatedFrom, UniqueSaturatedInto, Zero,
};
```

`Ensure*` ailesinin `Checked*` ailesinden farkı: `Ensure*` `DispatchError::Arithmetic` döndürür, `Checked*` `None` döndürür.

### 6.2 Verify Trait

```rust
pub trait Verify {
    type Signer: IdentifyAccount;
    fn verify<L: Lazy<[u8]>>(
        &self,
        msg: L,
        signer: &<Self::Signer as IdentifyAccount>::AccountId,
    ) -> bool;
}
```

`Lazy<[u8]>` — mesaj gerektiğinde hesaplanır (lazy evaluation).

```rust
impl<'a> Lazy<[u8]> for &'a [u8] {
    fn get(&mut self) -> &[u8] { self }
}
```

### 6.3 StaticLookup

```rust
pub trait StaticLookup {
    type Source: Codec + Clone + PartialEq + Debug + TypeInfo;
    type Target;
    fn lookup(s: Self::Source) -> Result<Self::Target, LookupError>;
    fn unlookup(t: Self::Target) -> Self::Source;
}
```

`IdentityLookup<T>` — kaynak ve hedef aynı tip:
```rust
impl<T: Codec + Clone + PartialEq + Debug + TypeInfo> StaticLookup for IdentityLookup<T> {
    type Source = T;
    type Target = T;
    fn lookup(x: T) -> Result<T, LookupError> { Ok(x) }
    fn unlookup(x: T) -> T { x }
}
```

`AccountIdLookup<AccountId, AccountIndex>` — `MultiAddress` → `AccountId`:
```rust
fn lookup(x: Self::Source) -> Result<Self::Target, LookupError> {
    match x {
        crate::MultiAddress::Id(i) => Ok(i),
        _ => Err(LookupError),
    }
}
```

Tuple StaticLookup: `(A, B)` → önce A dener, başarısız olursa B dener.

### 6.4 TransactionExtension Trait

Dosya: `primitives/runtime/src/traits/transaction_extension/mod.rs`

```rust
pub type ValidateResult<Val, Call> =
    Result<(ValidTransaction, Val, DispatchOriginOf<Call>), TransactionValidityError>;

pub struct TxBaseImplication<T>(pub T);
pub struct ImplicationParts<Base, Explicit, Implicit> {
    pub base: Base,
    pub explicit: Explicit,
    pub implicit: Implicit,
}
```

Implication kavramı: bir transaction'ın "implied content"i — imzanın kapsadığı veriler. 3 parçadan oluşur:
1. `base`: Call + extension version (her zaman dahil)
2. `explicit`: extension'ın açıkça bildirdiği veriler
3. `implicit`: extension'ın örtük olarak bildirdiği veriler (ör. nonce, block hash)

---

## BÖLÜM 7: sp-core — Kriptografi

### 7.1 Crypto Tipleri Genel Bakış

Her crypto modülü aynı pattern'i takip eder:
- `Public` tipi = `CryptoBytes<SIZE, Tag>` (veya `PublicBytes`)
- `Signature` tipi = `SignatureBytes<SIZE, Tag>`
- `Pair` tipi = anahtar çifti
- `CRYPTO_ID: CryptoTypeId = CryptoTypeId(*b"xxxx")`

### 7.2 Sr25519

Dosya: `primitives/core/src/sr25519.rs`

```rust
pub const CRYPTO_ID: CryptoTypeId = CryptoTypeId(*b"sr25");
pub const PUBLIC_KEY_SERIALIZED_SIZE: usize = 32;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub type Public = CryptoBytes<PUBLIC_KEY_SERIALIZED_SIZE, Sr25519PublicTag>;
pub type Signature = SignatureBytes<SIGNATURE_SERIALIZED_SIZE, Sr25519Tag>;

const SIGNING_CTX: &[u8] = b"substrate";
```

Temel kütüphane: `schnorrkel` crate.

Soft derivation:
```rust
fn derive<Iter: Iterator<Item = DeriveJunction>>(&self, path: Iter) -> Option<Public> {
    let mut acc = PublicKey::from_bytes(self.as_ref()).ok()?;
    for j in path {
        match j {
            DeriveJunction::Soft(cc) => acc = acc.derived_key_simple(ChainCode(cc), &[]).0,
            DeriveJunction::Hard(_cc) => return None,
        }
    }
    Some(Self::from(acc.to_bytes()))
}
```

Sr25519 soft derivation PUBLIC key üzerinden yapılabilir. Hard derivation için gizli anahtar gerekir.

Signing context: `b"substrate"` — tüm Sr25519 imzaları bu context ile yapılır.

### 7.3 Ed25519

Dosya: `primitives/core/src/ed25519.rs`

```rust
pub const CRYPTO_ID: CryptoTypeId = CryptoTypeId(*b"ed25");
pub const PUBLIC_KEY_SERIALIZED_SIZE: usize = 32;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub type Public = PublicBytes<PUBLIC_KEY_SERIALIZED_SIZE, Ed25519Tag>;
pub type Signature = SignatureBytes<SIGNATURE_SERIALIZED_SIZE, Ed25519Tag>;
```

Kütüphane: `ed25519_zebra` (zebra implementasyonu — güvenlik odaklı).

Hard derivation:
```rust
fn derive_hard_junction(secret_seed: &Seed, cc: &[u8; 32]) -> Seed {
    use codec::Encode;
    ("Ed25519HDKD", secret_seed, cc).using_encoded(sp_crypto_hashing::blake2_256)
}
```

Ed25519 yalnızca hard derivation destekler (soft derivation yoktur — güvenlik açığı yaratır).

### 7.4 ECDSA (secp256k1)

Dosya: `primitives/core/src/ecdsa.rs`

```rust
pub const CRYPTO_ID: CryptoTypeId = CryptoTypeId(*b"ecds");
pub const PUBLIC_KEY_SERIALIZED_SIZE: usize = 33;  // Compressed
pub const SIGNATURE_SERIALIZED_SIZE: usize = 65;   // 64 + recovery byte

pub type Public = GenericPublic<EcdsaTag>;       // blake2 recovery
pub type KeccakPublic = GenericPublic<EcdsaKeccakTag>;  // keccak recovery
```

İki public key tipi: `EcdsaTag` için blake2, `EcdsaKeccakTag` için keccak256.

Std implementasyonu: `secp256k1` crate
No-std implementasyonu: `k256` crate (Rust-native)

```rust
#[cfg(feature = "std")]
type NativeSignature = secp256k1::ecdsa::RecoverableSignature;

#[cfg(all(not(feature = "std"), feature = "full_crypto"))]
type NativeSignature = (k256::ecdsa::Signature, k256::ecdsa::RecoveryId);
```

`from_full()` — 65 byte uncompressed public key'i 33 byte compressed'a dönüştürür:
```rust
pub fn from_full(full: &[u8]) -> Result<Self, ()> {
    let mut tagged_full = [u8; 65];
    let full = if full.len() == 64 {
        // Untagged uncompressed: 0x04 prefix ekle
        tagged_full[0] = 0x04;
        tagged_full[1..].copy_from_slice(full);
        &tagged_full
    } else {
        full
    };
    // ...
}
```

### 7.5 DeriveJunction

Dosya: `primitives/core/src/crypto.rs`

```rust
pub const JUNCTION_ID_LEN: usize = 32;

pub enum DeriveJunction {
    Soft([u8; JUNCTION_ID_LEN]),  // Soft: public key'den türetilebilir
    Hard([u8; JUNCTION_ID_LEN]),  // Hard: secret key gerekli
}
```

String'den parsing:
```rust
impl<T: AsRef<str>> From<T> for DeriveJunction {
    fn from(j: T) -> DeriveJunction {
        let j = j.as_ref();
        let (code, hard) = if let Some(stripped) = j.strip_prefix('/') {
            (stripped, true)
        } else {
            (j, false)
        };

        let res = if let Ok(n) = str::parse::<u64>(code) {
            DeriveJunction::soft(n)         // Sayısal index
        } else {
            DeriveJunction::soft(code)      // String path
        };

        if hard { res.harden() } else { res }
    }
}
```

Kullanım:
- `//alice` → hard junction ("alice")
- `/0` → soft junction (index 0)
- `alice` → soft junction ("alice")

`soft<T: Encode>(index: T)`:
```rust
pub fn soft<T: Encode>(index: T) -> Self {
    let mut cc: [u8; JUNCTION_ID_LEN] = Default::default();
    index.using_encoded(|data| {
        if data.len() > JUNCTION_ID_LEN {
            cc.copy_from_slice(&sp_crypto_hashing::blake2_256(data));
        } else {
            cc[0..data.len()].copy_from_slice(data);
        }
    });
    DeriveJunction::Soft(cc)
}
```

32 byte'tan uzun veriler blake2_256 ile hash'lenir.

### 7.6 Hasher Implementasyonları

Dosya: `primitives/core/src/hasher.rs`

```rust
pub mod blake2 {
    pub struct Blake2Hasher;
    impl Hasher for Blake2Hasher {
        type Out = H256;
        type StdHasher = Hash256StdHasher;
        const LENGTH: usize = 32;
        fn hash(x: &[u8]) -> Self::Out {
            sp_crypto_hashing::blake2_256(x).into()
        }
    }
}

pub mod keccak {
    pub struct KeccakHasher;
    impl Hasher for KeccakHasher {
        type Out = H256;
        type StdHasher = Hash256StdHasher;
        const LENGTH: usize = 32;
        fn hash(x: &[u8]) -> Self::Out {
            sp_crypto_hashing::keccak_256(x).into()
        }
    }
}
```

Bu `Hasher` trait'i `hash_db` crate'inden gelir. Substrate trie'ı her iki hasher ile çalışabilir.

### 7.7 Storage Hasher Tipleri

Frame'de kullanılan storage hasher'lar (`frame-support/src/hash.rs`):

```rust
// Blake2 128 bit + key'i concatenate et (reversible)
pub struct Blake2_128Concat;
// Twox 64 bit + key'i concatenate et (reversible)
pub struct Twox64Concat;
// Sadece Identity (reversible, güvensiz — yalnızca güvenilir key'ler için)
pub struct Identity;
// Blake2 128 bit (sadece hash)
pub struct Blake2_128;
// Blake2 256 bit (sadece hash)
pub struct Blake2_256;
// Twox 128 bit (sadece hash)
pub struct Twox128;
// Twox 64 bit (sadece hash)
pub struct Twox64;
```

`ReversibleStorageHasher` — `Blake2_128Concat`, `Twox64Concat`, `Identity` bu trait'i implement eder. Reversible hasher'lar key iteration'a izin verir.

### 7.8 H256, H160, U256

Dosya: `primitives/core/src/hash.rs`

```rust
pub use primitive_types::{H160, H256, H512};
```

Doğrudan `primitive_types` crate'inden.

`convert_hash<H1, H2>(src: &H2) -> H1`:
```rust
pub fn convert_hash<H1: Default + AsMut<[u8]>, H2: AsRef<[u8]>>(src: &H2) -> H1 {
    let mut dest = H1::default();
    assert_eq!(dest.as_mut().len(), src.as_ref().len());
    dest.as_mut().copy_from_slice(src.as_ref());
    dest
}
```

Farklı hash tipleri arasında dönüşüm — aynı boyut zorunlu, aksi takdirde panic.

Dosya: `primitives/core/src/uint.rs`

```rust
pub use primitive_types::{U256, U512};
```

SCALE encoding desteği (WASM uyumlu little-endian):
```rust
// U256::from(120).encode() = [120, 0, 0, ..., 0]  (32 byte little-endian)
// U256::max_value().encode() = [255, 255, ..., 255]  (32 byte)
```

---

## BÖLÜM 8: TransactionValidity Sistemi

### 8.1 InvalidTransaction Enum

Dosya: `primitives/runtime/src/transaction_validity.rs`

```rust
pub enum InvalidTransaction {
    Call,                    // Çağrı beklenmiyor
    Payment,                 // Ödeme yetersiz
    Future,                  // Henüz geçerli değil (ör. yüksek nonce)
    Stale,                   // Çok eski (ör. düşük nonce)
    BadProof,                // Kötü imza/kanıt
    AncientBirthBlock,       // Doğum bloğu çok eski
    ExhaustsResources,       // Blok kaynakları tükendi
    Custom(u8),              // Özel hata (0-255)
    BadMandatory,            // Zorunlu dispatch başarısız
    MandatoryValidation,     // Zorunlu dispatch validate edilmeye çalışıldı
    BadSigner,               // Devre dışı veya geçersiz imzalayan
    IndeterminateImplicit,   // Implicit veri hesaplanamadı
    UnknownOrigin,           // Extension origin'i yetkilendirmedi
}
```

`UnknownOrigin` V5 extrinsic'leri için yeni: General extrinsic'lerde extension bir origin sağlamazsa bu hata döner.

`IndeterminateImplicit` da yeni: implicit veriler (ör. block hash, state root) hesaplanamıyorsa bu hata döner.

### 8.2 UnknownTransaction

```rust
pub enum UnknownTransaction {
    CannotLookup,            // Gerekli bilgi bulunamadı
    NoUnsignedValidator,     // Unsigned tx için validator yok
    Custom(u8),              // Özel
}
```

### 8.3 ValidTransaction Struct

```rust
pub struct ValidTransaction {
    pub priority: TransactionPriority,      // u64, additive, yüksek = iyi
    pub requires: Vec<TransactionTag>,      // Gerekli taglar (sıralama için)
    pub provides: Vec<TransactionTag>,      // Sağlanan taglar
    pub longevity: TransactionLongevity,    // Minimum geçerlilik blok sayısı (u64)
    pub propagate: bool,                    // Peer'lara yayılsın mı
}

impl Default for ValidTransaction {
    fn default() -> Self {
        Self {
            priority: 0,
            requires: vec![],
            provides: vec![],
            longevity: TransactionLongevity::max_value(),  // Sonsuz
            propagate: true,
        }
    }
}
```

`combine_with`:
```rust
pub fn combine_with(mut self, mut other: ValidTransaction) -> Self {
    Self {
        priority: self.priority.saturating_add(other.priority),
        requires: { self.requires.append(&mut other.requires); self.requires },
        provides: { self.provides.append(&mut other.provides); self.provides },
        longevity: self.longevity.min(other.longevity),  // En kısa longevity
        propagate: self.propagate && other.propagate,    // AND
    }
}
```

### 8.4 ValidTransactionBuilder

```rust
pub struct ValidTransactionBuilder {
    prefix: Option<&'static str>,
    validity: ValidTransaction,
}

// API:
builder
    .priority(1000)
    .longevity(25)
    .and_provides(nonce)
    .and_requires(prev_nonce)
    .propagate(true)
    .build()
```

Taglar prefix ile oluşturulur: `(prefix, tag_data).encode()` — farklı pallet'ların tag'leri çakışmaz.

### 8.5 TransactionSource

```rust
pub enum TransactionSource {
    InBlock,   // Zaten bloka dahil
    Local,     // Local node üretimi (OCW, offchain call)
    External,  // Dışarıdan geldi (ağ, RPC)
}
```

---

## BÖLÜM 9: sp-core Genel Tipler

### 9.1 AccountId32

```rust
// sp-core/src/crypto.rs'dan (re-export)
pub use sp_core::crypto::AccountId32;
```

32 byte opak hesap ID. Substrate'ın varsayılan hesap tipidir. SS58 formatında display edilir.

### 9.2 CryptoTypeId

```rust
pub struct CryptoTypeId(pub [u8; 4]);
```

4 byte crypto tip tanımlayıcı:
- `sr25` → Sr25519
- `ed25` → Ed25519
- `ecds` → ECDSA

### 9.3 KeyTypeId

```rust
pub struct KeyTypeId(pub [u8; 4]);
```

Keystore'da anahtar türü belirteci. Önceden tanımlıları:
```rust
pub mod key_types {
    pub const BABE: KeyTypeId = KeyTypeId(*b"babe");
    pub const GRANDPA: KeyTypeId = KeyTypeId(*b"gran");
    pub const IM_ONLINE: KeyTypeId = KeyTypeId(*b"imon");
    pub const AUTHORITY_DISCOVERY: KeyTypeId = KeyTypeId(*b"audi");
    pub const ACCOUNT: KeyTypeId = KeyTypeId(*b"acco");
    // ...
}
```

### 9.4 DEV_PHRASE ve DEV_ADDRESS

```rust
pub const DEV_PHRASE: &str =
    "bottom drive obey lake curtain smoke basket hold race lonely fit walk";

pub const DEV_ADDRESS: &str = "5DfhGyQdFobKM8NsWvEeAKk5EQQgYe9AydgJ7rMB6E1EqRzV";
```

Test ve geliştirme için public mnemonic. `//Alice`, `//Bob` vb. bu phrase'den türetilir.

---

## BÖLÜM 10: Storage Generator Katmanı

### 10.1 Generator Pattern

`storage/generator/value.rs` — storage implementation katmanı:

```rust
impl<T: FullCodec, G: StorageValue<T>> storage::StorageValue<T> for G {
    fn get() -> Self::Query {
        let value = unhashed::get(&Self::storage_value_final_key());
        G::from_optional_value_to_query(value)
    }

    fn try_mutate<R, E, F: FnOnce(&mut G::Query) -> Result<R, E>>(f: F) -> Result<R, E> {
        let mut val = G::get();
        let ret = f(&mut val);
        if ret.is_ok() {
            match G::from_query_to_optional_value(val) {
                Some(ref val) => G::put(val),
                None => G::kill(),
            }
        }
        ret
    }
}
```

`try_mutate` — closure hata döndürürse storage değişmez. `Ok` ise yaz.

`mutate` → `try_mutate` üzerine inşa edilmiş:
```rust
fn mutate<R, F: FnOnce(&mut G::Query) -> R>(f: F) -> R {
    Self::try_mutate(|v| Ok::<R, Never>(f(v)))
        .expect("`Never` can not be constructed; qed")
}
```

`Never` tipi: construct edilemez, dolayısıyla `expect` hiçbir zaman panic etmez.

### 10.2 unhashed Storage

Dosya: `storage/unhashed.rs` — ham storage erişimi:

```rust
pub fn get<T: Decode>(key: &[u8]) -> Option<T>
pub fn get_raw(key: &[u8]) -> Option<Vec<u8>>
pub fn put<T: Encode>(key: &[u8], val: &T)
pub fn put_raw(key: &[u8], value: &[u8])
pub fn kill(key: &[u8])
pub fn exists(key: &[u8]) -> bool
pub fn get_or_default<T: Decode + Default>(key: &[u8]) -> T
```

Tüm storage işlemleri nihayetinde `sp_io::storage::*` fonksiyonlarına çevrilir (host functions).

---

## BÖLÜM 11: Diğer Önemli Özellikler

### 11.1 Storage Migration Altyapısı

`traits/hooks.rs`:
```rust
pub trait OnRuntimeUpgrade {
    fn on_runtime_upgrade() -> Weight;
    #[cfg(feature = "try-runtime")]
    fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError>;
    #[cfg(feature = "try-runtime")]
    fn post_upgrade(state: Vec<u8>) -> Result<(), TryRuntimeError>;
}
```

`pre_upgrade` → `post_upgrade` arası state doğrulama. `pre_upgrade` bir `Vec<u8>` döndürür, bu `post_upgrade`'e geçer. Migration öncesi ve sonrası durumu doğrulayabilirsiniz.

`storage/migration.rs`:
```rust
pub fn move_storage_from_pallet(
    storage_name: &[u8],
    old_pallet_name: &[u8],
    new_pallet_name: &[u8],
)
pub fn move_pallet(old_name: &[u8], new_name: &[u8])
pub fn clear_storage_prefix(
    pallet_prefix: &[u8],
    storage_prefix: &[u8],
    limit: u32,
    maybe_cursor: Option<&[u8]>,
) -> MultiRemovalResults
```

### 11.2 dispatch_context

Dosya: `src/dispatch_context.rs`

```rust
pub fn with_context<T: Any + Default, R, F: FnOnce(&mut T) -> R>(f: F) -> R
pub fn use_context<T: Any + Default, R, F: FnOnce(Option<&mut T>) -> R>(f: F) -> R
```

Dispatch sırasında pallet'lar arası context paylaşımı. Thread-local storage kullanır. Bu özellikle birden fazla extrinsic'in aynı blokta etkileşim kurduğu senaryolarda kullanılır.

### 11.3 view_functions

Dosya: `src/view_functions.rs`

```rust
pub trait ViewFunction {
    const ID: QueryId;
    type Error: Encode;
    type Result: Encode;

    fn execute() -> Result<Self::Result, Self::Error>;
}
```

Yeni özellik: palletle ilgili sorguları runtime API yerine view function olarak expose etmek.

### 11.4 genesis_builder_helper

```rust
pub fn build_state<RuntimeGenesisConfig: BuildGenesisConfig + Default + serde::de::DeserializeOwned>(
    json: Vec<u8>,
) -> Result<Vec<u8>, alloc::string::String>

pub fn get_preset<RuntimeGenesisConfig: BuildGenesisConfig + Default + serde::Serialize>(
    id: &Option<sp_genesis_builder::PresetId>,
    preset: impl Fn(&Option<sp_genesis_builder::PresetId>) -> Option<Vec<u8>>,
) -> Option<Vec<u8>>
```

Runtime genesis konfigürasyonunu JSON'dan deserialize eder.

---

## BÖLÜM 12: Tasarım Kararları ve Kritik Notlar

### 12.1 SCALE Encoding

Tüm on-chain tipler SCALE (Simple Concatenated Aggregate Little-Endian) ile encode edilir. Neden:
- Minimal boyut (Protobuf'tan küçük)
- Rust'ta hızlı decode
- WASM'da çalışır
- Endian: little-endian (x86 doğal)

### 12.2 PhantomData Pattern

Tüm storage type'lar:
```rust
pub struct StorageValue<...>(core::marker::PhantomData<(...)>);
```

Bu zero-sized type'lardır. Runtime'da hiç veri tutmazlar. Yalnızca tip-sisteminde anlam taşırlar. Tüm metodlar `impl` bloklarında tanımlanmıştır — statik dispatch.

### 12.3 Two-Key Storage Security

Neden `Twox128` pallet ve storage prefix için?

- Kullanıcı inputu değil — pallet adları developer kontrolündedir
- Hız için XOR-based Twox128 kullanılır
- Collision direnci zayıf ama deterministic key gereksinimi için yeterli
- Keys küçüktür (16 byte)

Neden `Blake2_128Concat` map key'leri için?

- Kullanıcı inputu olabilir (arbitrary AccountId, token ID)
- Kriptografik güç gerekir
- `Concat` suffix: key'i hash sonrasına ekler → reversible (key recovery mümkün)

### 12.4 Weight Bidimensionality

`Weight { ref_time: u64, proof_size: u64 }` iki boyutlu sistem:

- `ref_time`: CPU nanosaniyesi (1 ns = 1000 units)
- `proof_size`: State proof boyutu

Polkadot parachain validation WASM runtime'ın state proof'unu doğrular. Proof boyutu sınırlı olmazsa relay chain validation yavaşlar. Bu yüzden hem CPU hem storage kanıt boyutu birlikte limit içinde tutulmalıdır.

---

## Nova Shield Chain — Uygulama Notu

### Storage Tasarım Rehberi

Nova Shield Chain'in kendi pallet'ları için önerilen storage patterns:

**1. Basit değerler için:**
```rust
#[pallet::storage]
pub type SomeConfig<T> = StorageValue<_, u128, ValueQuery, DefaultU128>;
```

**2. Hesap bazlı map için:**
```rust
#[pallet::storage]
pub type Balances<T: Config> = StorageMap<
    _,
    Blake2_128Concat,  // User input → kriptografik hash
    T::AccountId,
    Balance,
    ValueQuery,
>;
```

**3. DEX order book için (iki key):**
```rust
#[pallet::storage]
pub type Orders<T: Config> = StorageDoubleMap<
    _,
    Blake2_128Concat, TradingPair,
    Twox64Concat, OrderId,       // OrderId güvenilirse Twox64 yeterli
    Order<T::AccountId, Balance>,
>;
```

**4. Sayım gerektiren map için:**
```rust
#[pallet::storage]
pub type ActiveValidators<T: Config> = CountedStorageMap<
    _,
    Blake2_128Concat,
    T::AccountId,
    ValidatorInfo,
    OptionQuery,
>;
// count() → O(1) aktif validator sayısı
```

### Dispatch Design

Nova Shield'da pallet call'ları için:
1. `#[pallet::weight((base + per_item * count, DispatchClass::Normal, Pays::Yes))]`
2. Ağırlık overestimate: `PostDispatchInfo { actual_weight: Some(real_weight), pays_fee: Pays::Yes }`
3. Basit inherentler için `DispatchClass::Mandatory` (BELİRTİN: yalnızca çok hafif işlemler)

### Signature Tipi Seçimi

Nova Shield için önerim:
- **Validator imzaları:** Sr25519 — Schnorr, VRF uyumlu, aggregate edilebilir
- **Kullanıcı hesapları:** MultiSignature — hem Sr25519 hem Ed25519 hem ECDSA destekle
- **ETH uyumluluğu gerekirse:** `Eth` variant ekle, `KeccakPublic` kullan

### Weight Kalibrasyonu

RocksDB için Nova Shield ağırlık referansları:
- 1 DB Read ≈ 25 µs = `25_000 * WEIGHT_REF_TIME_PER_NANOS`
- 1 DB Write ≈ 100 µs = `100_000 * WEIGHT_REF_TIME_PER_NANOS`
- Extrinsic base ≈ 108 µs = `108_157 * WEIGHT_REF_TIME_PER_NANOS`
- Boş blok ≈ 431 µs

Nova Shield RocksDB yerine RocksDB + değişken overhead hedefliyorsa kendi benchmark'larını çalıştırmalı.

### TransactionValidity Tasarımı

MEV koruma sistemi için:
```rust
ValidTransaction {
    priority: base_fee + tip,   // Tip arttıkça öncelik artar
    provides: vec![(account, nonce).encode()],  // Nonce sıralaması
    requires: vec![(account, nonce - 1).encode()],
    longevity: 25,              // 25 blok sonra yeniden doğrula
    propagate: true,
}
```

Pre-trade intelligence için `TransactionSource::External` ile gelen tx'leri tespit et — bunlar dışarıdan ve muhtemelen MEV hedefi.

### DispatchError Genişletme

Nova Shield'a özel hatalar için `Module(ModuleError)` yeterli. Ancak `Other(&'static str)` sadece node-side logging için — on-chain depolama ve RPC response'unda `message` skip edilir (`#[codec(skip)]`).

### Kritik Bulgular — Doğrulanmış Değerler

| Parametre | Değer | Kaynak |
|-----------|-------|--------|
| MAX_MODULE_ERROR_ENCODED_SIZE | 4 byte | lib.rs:138 |
| MAX_EXTRINSIC_DEPTH | 256 | frame-support/lib.rs:39 |
| TRANSACTIONAL_LIMIT | 255 | transactional.rs:39 |
| DEFAULT_MAX_CALL_SIZE | 16 MiB | unchecked_extrinsic.rs:64 |
| EXTRINSIC_FORMAT_VERSION | 5 (current) | unchecked_extrinsic.rs:55 |
| LEGACY_EXTRINSIC_FORMAT_VERSION | 4 | unchecked_extrinsic.rs:61 |
| ExtrinsicBaseWeight | 108,157 ns | extrinsic_weights.rs:55 |
| BlockExecutionWeight | 431,614 ns | block_weights.rs:55 |
| RocksDB Read | 25 µs | rocksdb_weights.rs:27 |
| RocksDB Write | 100 µs | rocksdb_weights.rs:28 |
| JUNCTION_ID_LEN | 32 byte | crypto.rs:53 |
| Sr25519 Pub Key | 32 byte | sr25519.rs:55 |
| Sr25519 Signature | 64 byte | sr25519.rs:58 |
| Ed25519 Pub Key | 32 byte | ed25519.rs:36 |
| Ed25519 Signature | 64 byte | ed25519.rs:39 |
| ECDSA Pub Key | 33 byte (compressed) | ecdsa.rs:48 |
| ECDSA Signature | 65 byte | ecdsa.rs:51 |
| StorageValue key | 32 byte (Twox128 × 2) | generator/value.rs |
| Era period min | 4 | era.rs:66 |
| Era period max | 65536 | era.rs:66 |
| SIGNING_CTX (sr25519) | `b"substrate"` | sr25519.rs:49 |
| DEV_PHRASE | "bottom drive obey lake..." | crypto.rs:46 |
| CRYPTO_ID sr25519 | `*b"sr25"` | sr25519.rs:52 |
| CRYPTO_ID ed25519 | `*b"ed25"` | ed25519.rs:33 |
| CRYPTO_ID ecdsa | `*b"ecds"` | ecdsa.rs:45 |

---

## Özet

Bu analiz Substrate'ın üç kritik crate'ini kapsamıştır:

1. **frame-support**: FRAME pallet sisteminin omurgası. Storage types (Value, Map, DoubleMap, NMap, CountedMap), dispatch sistemi (DispatchResult, PostDispatchInfo, DispatchClass, Pays), trait ekosistemi (Currency, Hooks, Get, Contains), weight sabitleri ve transactional storage.

2. **sp-runtime**: Runtime tipler. MultiSignature (Ed25519/Sr25519/ECDSA/Eth), DispatchError (15 variant), TransactionValidity sistemi (InvalidTransaction, ValidTransaction, builder pattern), Era encoding, UncheckedExtrinsic V4/V5 format.

3. **sp-core**: Kriptografi. Sr25519 (schnorrkel, signing_ctx="substrate"), Ed25519 (ed25519_zebra, yalnızca hard derivation), ECDSA (secp256k1/k256 dual backend, blake2/keccak dual recovery), DeriveJunction (soft/hard), Hasher (Blake2/Keccak), H256/H160/U256/U512 (primitive_types wrapper).

Nova Shield Chain için bu altyapının tamamı doğrudan kullanılabilir. Substrate compatibility sayesinde ekosistem araçları (Polkadot.js, SubQuery, OnFinality) de devreye girebilir.

---

## BÖLÜM 13: TransactionExtension — Tam Trait Analizi

### 13.1 TransactionExtension Trait Tanımı

Dosya: `primitives/runtime/src/traits/transaction_extension/mod.rs:229-315`

```rust
pub trait TransactionExtension<Call: Dispatchable>:
    Codec + DecodeWithMemTracking + Debug + Sync + Send + Clone + Eq + PartialEq + StaticTypeInfo
{
    const IDENTIFIER: &'static str;
    type Implicit: Codec + StaticTypeInfo;
    type Val;
    type Pre;

    fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError>;
    fn metadata() -> Vec<TransactionExtensionMetadata>;
    fn weight(&self, call: &Call) -> Weight;
    fn validate(
        &self,
        origin: DispatchOriginOf<Call>,
        call: &Call,
        info: &DispatchInfoOf<Call>,
        len: usize,
        self_implicit: Self::Implicit,
        inherited_implication: &impl Implication,
        source: TransactionSource,
    ) -> ValidateResult<Self::Val, Call>;
    fn prepare(
        self,
        val: Self::Val,
        origin: &DispatchOriginOf<Call>,
        call: &Call,
        info: &DispatchInfoOf<Call>,
        len: usize,
    ) -> Result<Self::Pre, TransactionValidityError>;
    fn post_dispatch_details(
        pre: Self::Pre,
        info: &DispatchInfoOf<Call>,
        post_info: &PostDispatchInfoOf<Call>,
        len: usize,
        result: &DispatchResult,
    ) -> Result<Option<Weight>, TransactionValidityError>;
    fn post_dispatch(
        pre: Self::Pre,
        info: &DispatchInfoOf<Call>,
        post_info: &mut PostDispatchInfoOf<Call>,
        len: usize,
        result: &DispatchResult,
    ) -> Result<(), TransactionValidityError>;
    fn bare_validate(
        call: &Call,
        info: &DispatchInfoOf<Call>,
        len: usize,
    ) -> TransactionValidity;
    fn bare_validate_and_prepare(
        call: &Call,
        info: &DispatchInfoOf<Call>,
        len: usize,
    ) -> Result<(), TransactionValidityError>;
    fn bare_post_dispatch(
        info: &DispatchInfoOf<Call>,
        post_info: &mut PostDispatchInfoOf<Call>,
        len: usize,
        result: &DispatchResult,
    ) -> Result<(), TransactionValidityError>;
}
```

### 13.2 Pipeline Çalışma Modeli

Transaction extension pipeline tuple composition:
```
(ExtA, ExtB, ExtC) için:
validate: ExtA → ExtB → ExtC  (sıralı, her biri origin ve implication'ı devralır)
prepare: ExtA → ExtB → ExtC   (sıralı)
post_dispatch: ExtC → ExtB → ExtA  (ters sıra, post-dispatch refund için)
```

**Post-dispatch sırası kritiktir:** Ücret ödeyen extension pipeline'ın sonunda olmalıdır. Bu şekilde gerçek ağırlık hesaplanabilir. Ters sıradaki post-dispatch sonrasında toplam gerçek ağırlık bilinir.

### 13.3 Implicit vs Explicit Veri

- **Explicit**: Transaction body'sinde encode edilmiş extension verisi (ör. nonce değeri)
- **Implicit**: On-chain state'den türetilen, imzada yer alan ama tx body'sinde olmayan veri (ör. genesis hash, block hash)

`implicit()` metodunun varsayılan implementasyonu:
```rust
fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError> {
    use crate::transaction_validity::InvalidTransaction::IndeterminateImplicit;
    Ok(Self::Implicit::decode(&mut &[][..]).map_err(|_| IndeterminateImplicit)?)
}
```

Eğer `Self::Implicit = ()` ise (boş), decode başarılı olur ve `Ok(())` döner.

### 13.4 Val ve Pre Tipleri

- `Val`: `validate` → `prepare` arasında taşınan token (ör. ödenen fee miktarı)
- `Pre`: `prepare` → `post_dispatch` arasında taşınan token (ör. pre-dispatch fee rezervasyonu)

Tipik kullanım:
```rust
type Val = Option<BalanceOf<T>>;  // validate'de hesaplanan fee
type Pre = (Option<BalanceOf<T>>, T::AccountId);  // fee + ödeyici hesap
```

### 13.5 Metadata Yapısı

```rust
pub struct TransactionExtensionMetadata {
    pub identifier: &'static str,
    pub ty: MetaType,          // Extension verisi tip bilgisi
    pub implicit: MetaType,    // Implicit veri tip bilgisi
}
```

Her extension tam olarak bir metadata öğesi döndürür.

### 13.6 `bare_*` Metodlar

Bare (imzasız, extension'sız) transaction'lar için — inherent'lar:
- `bare_validate`: Validate only, no val needed
- `bare_validate_and_prepare`: Validate + prepare combo
- `bare_post_dispatch`: Post cleanup

Bu metodlar inherent extrinsic'ler için kullanılır, extension pipeline çalıştırılmaz.

---

## BÖLÜM 14: SS58 Address Format

### 14.1 SS58 Encoding Detayları

Dosya: `primitives/core/src/crypto.rs:275-349`

SS58 formatı: `Base58Check(prefix_bytes ++ public_key_bytes ++ checksum_2_bytes)`

Prefix encoding:
- 0..=63: 1 byte prefix
- 64..=16383: 2 byte prefix (bit manipulation ile)
- 64..=127 için decode: `lower = (d[0] << 2) | (d[1] >> 6)`, `upper = d[1] & 0b00111111`

Checksum: `blake2b_512(b"SS58PRE" ++ prefix_bytes ++ key_bytes)[0..2]`

```rust
let hash = ss58hash(&data[0..body_len + prefix_len]);
let checksum = &hash[0..CHECKSUM_LEN];  // CHECKSUM_LEN = 2
```

Polkadot: prefix = 0 → `5...`
Kusama: prefix = 2 → `C...`
Generic Substrate: prefix = 42 → `5...` (aynı görünüm, farklı ağ)

### 14.2 PublicError Enum

```rust
pub enum PublicError {
    BadBase58,
    BadLength,
    UnknownSs58AddressFormat(Ss58AddressFormat),
    InvalidChecksum,
    InvalidPrefix,
    InvalidFormat,
    InvalidPath,
    FormatNotAllowed,
    PasswordNotAllowed,
    MalformedUri(AddressUriError),  // std only
}
```

### 14.3 Ss58Codec Trait

```rust
pub trait Ss58Codec: Sized + AsMut<[u8]> + AsRef<[u8]> + ByteArray {
    fn format_is_allowed(f: Ss58AddressFormat) -> bool {
        !f.is_reserved()
    }
    fn from_ss58check(s: &str) -> Result<Self, PublicError>;
    fn from_ss58check_with_version(s: &str) -> Result<(Self, Ss58AddressFormat), PublicError>;
    fn to_ss58check_with_version(&self, version: Ss58AddressFormat) -> String;
    fn to_ss58check(&self) -> String;
}
```

---

## BÖLÜM 15: TrieError ve Proving Trie

### 15.1 TrieError Enum

Dosya: `primitives/runtime/src/proving_trie/mod.rs`

```rust
pub enum TrieError {
    // sp-trie TrieError kaynaklı:
    InvalidStateRoot,        // DB'de olmayan state root
    IncompleteDatabase,      // Trie item DB'de bulunamadı
    ValueAtIncompleteKey,    // Nibble key byte-aligned değil
    DecoderError,            // Corrupt trie item
    InvalidHash,             // Geçersiz hash

    // sp-trie VerifyError kaynaklı:
    DuplicateKey,             // Aynı key birden fazla kez
    ExtraneousNode,           // Proof'ta fazla node var
    ExtraneousValue,          // Proof'ta fazla value var
    ExtraneousHashReference,  // Proof'ta fazla hash reference
    InvalidChildReference,    // Hash uzunluğunu aşan child reference
    ValueMismatch,            // Beklenen value bulunamadı
    IncompleteProof,          // Proof eksik node içeriyor
    RootMismatch,             // Hesaplanan root yanlış
    DecodeError,              // Proof node decode hatası
}
```

`TrieError` de `DispatchError::Trie(TrieError)` olarak paketlenir. Bu runtime içinde trie doğrulama hatalarının temiz bir şekilde işlenmesini sağlar.

### 15.2 Base16 ve Base2 Trie

`proving_trie/base16.rs` — 16-ary (hexary) merkle trie (Substrate'ın varsayılanı)
`proving_trie/base2.rs` — Binary merkle trie (bazı proof sistemleri için)

---

## BÖLÜM 16: CountedStorageMap — Derin Analiz

### 16.1 Counter Güncelleme Mekanizması

`insert` implementasyonu (`types/counted_map.rs:198-203`):
```rust
pub fn insert<KeyArg: EncodeLike<Key>, ValArg: EncodeLike<Value>>(key: KeyArg, val: ValArg) {
    if !<Self as MapWrapper>::Map::contains_key(Ref::from(&key)) {
        CounterFor::<Prefix>::mutate(|value| value.saturating_inc());
    }
    <Self as MapWrapper>::Map::insert(key, val)
}
```

`remove` implementasyonu (`types/counted_map.rs:206-211`):
```rust
pub fn remove<KeyArg: EncodeLike<Key>>(key: KeyArg) {
    if <Self as MapWrapper>::Map::contains_key(Ref::from(&key)) {
        CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
    }
    <Self as MapWrapper>::Map::remove(key)
}
```

**Kritik Gözlem:** Her insert/remove için `contains_key` ile önce bir okuma yapılıyor. Bu güvenli sayaç güncellemesi için gerekli ama her operasyon için 1 ek DB read demektir.

### 16.2 CounterPrefix

```rust
pub trait CountedStorageMapInstance: StorageInstance {
    type CounterPrefix: StorageInstance;
}

type CounterFor<P> =
    StorageValue<<P as CountedStorageMapInstance>::CounterPrefix, Counter, ValueQuery>;

pub type Counter = u32;  // Maksimum 4,294,967,295 item
```

Counter ayrı bir `StorageValue` olarak tutulur. `CounterPrefix` asıl map'ten farklı bir prefix kullanır.

`counter_storage_final_key()`:
```rust
pub fn counter_storage_final_key() -> [u8; 32] {
    CounterFor::<Prefix>::hashed_key()
}
```

### 16.3 OnRemovalCounterUpdate

```rust
pub struct OnRemovalCounterUpdate<Prefix>(core::marker::PhantomData<Prefix>);

impl<Prefix: CountedStorageMapInstance> crate::storage::PrefixIteratorOnRemoval
    for OnRemovalCounterUpdate<Prefix>
{
    fn on_removal(_key: &[u8], _value: &[u8]) {
        CounterFor::<Prefix>::mutate(|value| value.saturating_dec());
    }
}
```

`drain()` veya prefix bazlı silme sırasında her silinen item için callback çağrılır ve sayaç azaltılır.

### 16.4 try_mutate_exists Mantığı

```rust
pub fn try_mutate<KeyArg, R, E, F>(key: KeyArg, f: F) -> Result<R, E>
where ... {
    Self::try_mutate_exists(key, |option_value_ref| {
        let option_value = core::mem::replace(option_value_ref, None);
        let mut query = <Self as MapWrapper>::Map::from_optional_value_to_query(option_value);
        let res = f(&mut query);
        let option_value = <Self as MapWrapper>::Map::from_query_to_optional_value(query);
        let _ = core::mem::replace(option_value_ref, option_value);
        res
    })
}
```

`mem::replace` ile geçici `None` koyma ve geri yerleştirme — Rust borrow checker'ı aşmak için klasik pattern.

---

## BÖLÜM 17: Aritmetik Tipler — sp-runtime Re-exports

### 17.1 Fixed Point Number Tipleri

```rust
// sp-arithmetic'ten re-export
pub use sp_arithmetic::{
    FixedI128,   // i128 tabanlı sabit noktalı, 18 ondalık basamak
    FixedI64,    // i64 tabanlı sabit noktalı, 9 ondalık basamak
    FixedU128,   // u128 tabanlı sabit noktalı, 18 ondalık basamak
    FixedU64,    // u64 tabanlı sabit noktalı, 9 ondalık basamak
    FixedPointNumber,  // trait
    FixedPointOperand, // trait
};
```

`FixedU128`: Dahili değer `u128`, `1.0 = 10^18`. Toplam 38 basamak, 18'i ondalık.

### 17.2 Permill, Perbill, Percent, Perquintill

```rust
pub use sp_arithmetic::{
    PerU16,      // u16 tabanlı, 0..65536 → 0%..100%
    Perbill,     // u32 tabanlı, milliardda bir (10^-9)
    Percent,     // u8 tabanlı, 0..100
    Permill,     // u32 tabanlı, binde bir (10^-3)
    Perquintill, // u64 tabanlı, quintillionth (10^-18)
    PerThing,    // trait: tüm per-x tipleri için
    Rounding,    // yuvarlamanın nasıl yapılacağı
    Rational128, // p/q rasyonel sayı (p: i128, q: u128)
};
```

`Perbill` kullanımı: `Perbill::from_percent(10)` = %10 = `100_000_000u32`

### 17.3 ArithmeticError ile Checked Ops

`Ensure*` ailesinin avantajı:
```rust
// Checked: None döner, context kaybolur
let result = a.checked_add(b).ok_or(DispatchError::Arithmetic(ArithmeticError::Overflow))?;

// EnsureAdd: doğrudan DispatchError::Arithmetic döner
let result = a.ensure_add(b)?;
```

### 17.4 Saturating Trait

```rust
pub trait Saturating {
    fn saturating_add(self, rhs: Self) -> Self;
    fn saturating_sub(self, rhs: Self) -> Self;
    fn saturating_mul(self, rhs: Self) -> Self;
    fn saturating_pow(self, exp: usize) -> Self;
}
```

Saturation: overflow/underflow yerine min/max değerine "doyar". Runtime'da güvenli ama matematiksel hata maskelenir.

---

## BÖLÜM 18: sp-core Traits

### 18.1 CryptoBytes / PublicBytes / SignatureBytes

Dosya: `primitives/core/src/crypto_bytes.rs`

```rust
pub struct CryptoBytes<const N: usize, Tag>(pub [u8; N], PhantomData<Tag>);
pub type PublicBytes<const N: usize, Tag> = CryptoBytes<N, (Tag, PublicTag)>;
pub type SignatureBytes<const N: usize, Tag> = CryptoBytes<N, (Tag, SignatureTag)>;
```

Bu nedenle:
- `Sr25519 Public = CryptoBytes<32, Sr25519PublicTag>` → sadece 32 byte, zero overhead
- `Sr25519 Signature = SignatureBytes<64, Sr25519Tag>` → sadece 64 byte

`ByteArray` trait:
```rust
pub trait ByteArray: AsRef<[u8]> + AsMut<[u8]> + for<'a> TryFrom<&'a [u8]> {
    const LEN: usize;
    fn from_slice(data: &[u8]) -> Result<Self, ()>;
    fn as_slice(&self) -> &[u8] { self.as_ref() }
    fn to_raw_vec(&self) -> Vec<u8> { self.as_ref().to_vec() }
}
```

### 18.2 Pair Trait

```rust
pub trait Pair: Sized {
    type Public: Public + Hash;
    type Seed: Default + AsRef<[u8]>;
    type Signature: AsRef<[u8]>;
    type ProofOfPossession: Default;

    fn from_seed(seed: &Self::Seed) -> Self;
    fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError>;
    fn sign(&self, message: &[u8]) -> Self::Signature;
    fn verify<M: AsRef<[u8]>>(sig: &Self::Signature, message: M, pubkey: &Self::Public) -> bool;
    fn public(&self) -> Self::Public;
    fn generate() -> (Self, Self::Seed);
    fn generate_with_phrase(password: Option<&str>) -> (Self, String, Self::Seed);
    fn from_phrase(phrase: &str, password: Option<&str>) -> Result<(Self, Self::Seed), SecretStringError>;
    fn derive<Iter: Iterator<Item = DeriveJunction>>(
        &self, path: Iter, seed: Option<Self::Seed>
    ) -> Result<(Self, Option<Self::Seed>), DeriveError>;
    fn from_string(s: &str, password_override: Option<&str>) -> Result<Self, SecretStringError>;
    fn to_raw_vec(&self) -> Vec<u8>;
}
```

### 18.3 Proof of Possession

Dosya: `primitives/core/src/proof_of_possession.rs`

```rust
pub trait ProofOfPossession<P: TraitPair>: Sized {
    fn generate_for_pair(pair: &P) -> Self;
    fn verify(self, public: &P::Public) -> bool;
}

pub trait NonAggregatable {}
```

BLS imzalar aggregate edilebilir. `NonAggregatable` marker trait ile Sr25519 ve Ed25519'ın aggregatable olmadığı belirtilir.

---

## BÖLÜM 19: frame-support Makro Sistemi

### 19.1 storage_alias Makrosu

`src/tests/storage_alias.rs` ve kullanımı:

```rust
#[crate::storage_alias]
type Foo = StorageValue<Prefix, BoundedVec<u32, ConstU32<7>>>;

#[crate::storage_alias]
type FooMap = StorageMap<Prefix, Twox128, u32, BoundedVec<u32, ConstU32<7>>>;
```

Migration ve test kodunda eski storage tipine referans vermek için kullanılır. Makro `StorageInstance`'ı otomatik implement eder.

### 19.2 impl_tx_ext_default Makrosu

```rust
#[macro_export]
macro_rules! impl_tx_ext_default {
    ($call:ty; $($rest:tt)*) => { ... }
}
```

`TransactionExtension` trait'inin `validate` ve `prepare` metodları için default implementasyon üretir. Extension yalnızca bir şeyi kontrol etmek ve geçirmek istiyorsa kullanılır.

### 19.3 parameter_types! Makrosu

`sp-core`'dan:
```rust
parameter_types! {
    pub const MaxValidators: u32 = 1000;
    pub const BlockHashCount: u64 = 2400;
}
```

Her konstant için bir struct üretir ve `Get<T>` trait'ini implement eder. Runtime upgrade ile değiştirilemez (const), ama storage'a yazılmış versiyonu (non-const) governance ile değiştirilebilir.

### 19.4 construct_runtime! Makrosu

```rust
construct_runtime!(
    pub struct Runtime {
        System: frame_system,
        Balances: pallet_balances,
        Staking: pallet_staking,
        // ...
    }
);
```

Bu makro:
1. Her pallet için index atar (MetadataVersion'da kullanılır)
2. `AllPalletsWithSystem`, `AllPalletsWithoutSystem` tuple'larını oluşturur
3. `RuntimeCall`, `RuntimeEvent`, `RuntimeError` enum'larını aggregate eder
4. `Hooks` pipeline'ını kurar

---

## BÖLÜM 20: Offchain Worker Altyapısı

### 20.1 Offchain Storage

Dosya: `primitives/core/src/offchain/storage.rs`

```rust
pub struct InMemOffchainStorage {
    storage: HashMap<Vec<u8>, Vec<u8>>,
}

impl OffchainStorage for InMemOffchainStorage {
    fn set(&mut self, prefix: &[u8], key: &[u8], value: &[u8]);
    fn remove(&mut self, prefix: &[u8], key: &[u8]);
    fn get(&self, prefix: &[u8], key: &[u8]) -> Option<Vec<u8>>;
    fn compare_and_set(
        &mut self,
        prefix: &[u8],
        key: &[u8],
        old_value: Option<&[u8]>,
        new_value: &[u8],
    ) -> bool;
}
```

`compare_and_set` — CAS (Compare-And-Swap) primitifi, OCW'de mutex olarak kullanılır.

### 20.2 StorageLock

Dosya: `primitives/runtime/src/offchain/storage_lock.rs`

```rust
pub struct StorageLock<'a, L: Lockable> {
    key: &'a [u8],
    value_ref: PhantomData<L>,
}
```

OCW'de tekil çalışmayı garantilemek için distributed lock mekanizması. CAS primitive üzerine inşa edilmiştir.

### 20.3 HTTP Client

Dosya: `primitives/runtime/src/offchain/http.rs`

```rust
pub struct Request<'a> {
    id: RequestId,
    method: &'a str,
    url: &'a str,
    body: Vec<&'a [u8]>,
    headers: Vec<(&'a str, &'a str)>,
}

pub struct Response {
    pub id: RequestId,
    pub code: u16,
    body: Vec<u8>,
    headers: Headers,
}
```

OCW'de HTTP requestler sp-io host function'larına delege edilir. WASM runtime içinde gerçek HTTP yoktur — host function sandbox'ı kullanılır.

---

## BÖLÜM 21: Generic Block ve Header Tipleri

### 21.1 Generic Block

Dosya: `primitives/runtime/src/generic/block.rs`

```rust
pub struct Block<Header, Extrinsic: MaybeSerialize> {
    pub header: Header,
    pub extrinsics: Vec<Extrinsic>,
}
```

`Block<Header, Extrinsic>` — generic. Substrate her runtime için farklı header/extrinsic tipleri kullanabilir.

### 21.2 Generic Header

Dosya: `primitives/runtime/src/generic/header.rs`

```rust
pub struct Header<Number: Copy + Into<U256> + TryFrom<U256>, Hashing: Hash> {
    pub parent_hash: Hashing::Output,
    pub number: Compact<Number>,
    pub state_root: Hashing::Output,
    pub extrinsics_root: Hashing::Output,
    pub digest: Digest,
}
```

`Number` genellikle `u32` (Polkadot) veya `u64`.
`Hashing` genellikle `BlakeTwo256`.

`Digest` — konsensüs motorundan gelen ek veriler (BABE pre-digest, GRANDPA signal vb.)

### 21.3 Digest ve DigestItem

```rust
pub struct Digest {
    pub logs: Vec<DigestItem>,
}

pub enum DigestItem {
    PreRuntime(ConsensusEngineId, Vec<u8>),
    Consensus(ConsensusEngineId, Vec<u8>),
    Seal(ConsensusEngineId, Vec<u8>),
    Other(Vec<u8>),
    RuntimeEnvironmentUpdated,
}
```

`PreRuntime` — block production öncesi (BABE VRF output)
`Consensus` — konsensüs mesajı (GRANDPA vote, BABE secondary slot)
`Seal` — block author imzası

---

## BÖLÜM 22: Genesis Builder

### 22.1 BuildGenesisConfig Trait

```rust
pub trait BuildGenesisConfig: Default {
    fn build(&self);
}
```

Her pallet genesis konfigürasyonunu buradan çeker.

### 22.2 genesis_builder_helper

```rust
pub fn build_state<RuntimeGenesisConfig: BuildGenesisConfig + Default + DeserializeOwned>(
    json: Vec<u8>,
) -> Result<Vec<u8>, String>
```

JSON → `RuntimeGenesisConfig` → genesis storage.

`get_preset`:
```rust
pub fn get_preset<RuntimeGenesisConfig: BuildGenesisConfig + Default + Serialize>(
    id: &Option<PresetId>,
    preset: impl Fn(&Option<PresetId>) -> Option<Vec<u8>>,
) -> Option<Vec<u8>>
```

Named genesis presets (development, testnet, mainnet) için.

---

## BÖLÜM 23: MultiAddress Tipi

### 23.1 Tanım

Dosya: `primitives/runtime/src/multiaddress.rs`

```rust
pub enum MultiAddress<AccountId, AccountIndex> {
    Id(AccountId),           // 32 byte hesap ID
    Index(Compact<AccountIndex>),  // Hesap index (daha kısa)
    Raw(Vec<u8>),            // Ham bytes
    Address32([u8; 32]),     // 32 byte adres
    Address20([u8; 20]),     // 20 byte Ethereum tarzı adres
}
```

Substrate extrinsic'lerinde hedef adresi farklı formatlarda belirtebilirsiniz. `AccountIdLookup` yalnızca `Id` variant'ını kabul eder.

`Address20` — Ethereum hesap adresi formatı, `pallet_revive` ile kullanılır.

---

## BÖLÜM 24: Weight Meter ve Limit Sistemi

### 24.1 WeightMeter

`sp-weights`'ten:

```rust
pub struct WeightMeter {
    consumed: Weight,
    limit: Weight,
}

impl WeightMeter {
    pub fn new(limit: Weight) -> Self;
    pub fn with_limit(limit: Weight) -> Self;
    pub fn remaining(&self) -> Weight;
    pub fn consumed(&self) -> Weight;
    pub fn can_consume(&self, amount: Weight) -> bool;
    pub fn consume(&mut self, amount: Weight);
    pub fn try_consume(&mut self, amount: Weight) -> Result<(), ()>;
}
```

`on_idle` hook signature:
```rust
fn on_idle(n: BlockNumber, remaining_weight: Weight) -> Weight;
```

`on_poll` hook signature:
```rust
fn on_poll(n: BlockNumber, weight: &mut WeightMeter);
```

`on_poll` WeightMeter referansı alır ve consume eder; `on_idle` ise kalan ağırlığı alır ve kullandığını döner.

### 24.2 RefTimeWeight Sabitler

```rust
pub const WEIGHT_REF_TIME_PER_NANOS: u64 = 1_000;
pub const WEIGHT_REF_TIME_PER_MICROS: u64 = 1_000_000;
pub const WEIGHT_REF_TIME_PER_MILLIS: u64 = 1_000_000_000;
pub const WEIGHT_REF_TIME_PER_SECOND: u64 = 1_000_000_000_000;
```

Blok ağırlığı limiti genellikle `2 * WEIGHT_REF_TIME_PER_SECOND` = 2 saniye → gerçek donanımda ~2s CPU zamanına karşılık gelir.

Substrate default block time: 6s, normal tx kapasitesi: `0.75 * 2s = 1.5s`
Mandatory + operational için: kalan `0.25 * 2s = 0.5s`

---

## BÖLÜM 25: Ek Kritik Bulgular ve Nova Shield DEX Uygulama Notu

### 25.1 Frame-Support'ta Mevcut Değişiklikler (Yeni Özellikler)

Bu analiz sırasında tespit edilen yeni/değişen özellikler:

1. **V5 Extrinsic Format**: `General` extrinsic tipi — imzasız ama extension'lı. MEV koruması için önemli: transaction'ın imzası yoksa sıralı olmak zorunda değil → MEV bot'ları da kullanabilir. Nova Shield'ın MEV koruması bu tipi dikkate almalı.

2. **Authorized Origin**: `RawOrigin::Authorized` — `#[pallet::authorize]` ile. Bu yeni origin tipi, Nova Shield'ın "pre-approved" işlemleri için kullanılabilir (ör. validator onaylı swap'lar).

3. **`Eth` MultiSigner/MultiSignature**: EVM uyumluluğu. Nova Shield cross-chain köprüsü için bu tip kullanılabilir.

4. **`DecodeWithMemTracking`**: WASM heap monitoring. Nova Shield'ın WASM runtime'ı büyük veri yapıları decode ederken bunu izlemeli.

5. **`IndeterminateImplicit` InvalidTransaction variant**: Extension'lar implicit veri üretemezse bu hata. Nova Shield custom extension'larında dikkat.

### 25.2 DEX Uygulama Notu

Nova Shield DEX implementasyonu için frame-support altyapısından öğrenilenler:

**OrderBook Storage Tasarımı:**
```rust
// Fiyat seviyesine göre gruplu order storage
#[pallet::storage]
pub type BuyOrders<T: Config> = StorageDoubleMap<
    _,
    Blake2_128Concat, TradingPair,   // (base, quote) çifti
    Twox64Concat, PriceLevel,        // Fiyat seviyesi (u128 scaled)
    BoundedVec<Order<T>, ConstU32<MAX_ORDERS_PER_LEVEL>>,
    ValueQuery,
>;

// Order sayacı — O(1) toplam order sayısı
#[pallet::storage]
pub type OrderCount<T: Config> = CountedStorageMap<
    _,
    Blake2_128Concat,
    T::AccountId,
    u32,  // Hesap başına açık order sayısı
    ValueQuery,
>;
```

**Fee Hesaplama için FixedU128:**
```rust
// Maker fee: 0.1% = Permill::from_rational(1u32, 1000u32)
// Taker fee: 0.3% = Permill::from_rational(3u32, 1000u32)
let maker_fee = Permill::from_rational(1u32, 1000u32);
let fee_amount = maker_fee.mul_floor(swap_amount);
```

`mul_floor` → aşağıya yuvarlama. Fee hesabında "müşteri lehine" yuvarlama standart pratik.

**AMM için FixedPointNumber:**
```rust
// Constant product AMM: x * y = k
// Swap miktarı: Δy = (y * Δx) / (x + Δx)
// FixedU128 ile 18 ondalık basamak hassasiyeti
let x = FixedU128::from_inner(pool.reserve_x);
let y = FixedU128::from_inner(pool.reserve_y);
let dx = FixedU128::from_inner(amount_in);
let dy = y.saturating_mul(dx) / (x.saturating_add(dx));
```

**Transaction Validity (MEV Koruması):**
```rust
// Commit-reveal swap extension
pub struct CommitRevealExtension;

impl TransactionExtension<RuntimeCall> for CommitRevealExtension {
    const IDENTIFIER: &'static str = "CommitReveal";
    type Implicit = BlockHash;  // Hangi blok için commit yapıldı
    type Val = CommitHash;      // Commit hash
    type Pre = (CommitHash, BlockHash);

    fn implicit(&self) -> Result<Self::Implicit, TransactionValidityError> {
        Ok(current_block_hash())  // On-chain state'den
    }

    fn validate(...) -> ValidateResult<Self::Val, Call> {
        // Commit var mı kontrol et
        // Reveal zamanlaması doğru mu
    }
}
```

**Weight Optimizasyonu:**
```rust
#[pallet::weight({
    let order_count = Orders::<T>::count();
    T::WeightInfo::execute_swap(order_count)
})]
pub fn execute_swap(
    origin: OriginFor<T>,
    pair: TradingPair,
    amount: Balance,
) -> DispatchResultWithPostInfo {
    // ... swap logic ...
    let actual_matches = do_swap(...)?;
    Ok(PostDispatchInfo {
        actual_weight: Some(T::WeightInfo::execute_swap(actual_matches)),
        pays_fee: Pays::Yes,
    })
}
```

Pre-dispatch: worst case (tüm order'lar). Post-dispatch: gerçek match sayısına göre refund.

### 25.3 Güvenlik Notları

1. **ModuleError 4 byte limit**: Pallet hataları 4 byte ile sınırlı. Nova Shield'ın hata enum'ları bu sınırı aşamaz. `#[codec(compact)]` ile büyük enum'ları sıkıştır.

2. **MAX_EXTRINSIC_DEPTH = 256**: Nested pallet call'ları bu derinliği aşamaz. Cross-pallet çağrılar zincirlendiğinde dikkat.

3. **TRANSACTIONAL_LIMIT = 255**: Nested transactional layer sınırı. DEX swap'ları genellikle 1-3 layer kullanır, güvende.

4. **saturating_inc/dec (CountedStorageMap)**: Counter'lar saturate eder. `u32::MAX = 4,294,967,295` item — aşılması durumunda yanlış sayım. Nova Shield'da per-account item limiti ekle.

5. **Blake2_128Concat vs Twox64Concat seçimi**: Kullanıcı-controlled key'ler için her zaman Blake2_128Concat kullan. Sadece güvenilir (pallet-internal) key'ler için Twox64Concat.

6. **Lazy Evaluation in Verify**: `Lazy<[u8]>` — mesaj doğrulama başarısız olursa mesaj compute edilmez. Bu önemli bir optimizasyon.

### 25.4 Nova Shield'a Direkt Transfer Edilebilecek Patternler

| Pattern | Substrate | Nova Shield Kullanımı |
|---------|-----------|----------------------|
| StorageValue + OnEmpty | Pallet parametreleri | DEX fee oranları, slippage limit |
| CountedStorageMap | Pallet-balances | Aktif validator/delegator sayısı |
| StorageDoubleMap remove_prefix | Staking unbonding | Kullanıcının tüm açık order'larını temizle |
| BoundedVec + StorageTryAppend | Multisig approvals | Order book entry limiti per level |
| try_mutate_exists | Account killing | Boş vault/position otomatik kapat |
| TransactionExtension pipeline | Fee payment | MEV commit-reveal + fee payment |
| PostDispatchInfo refund | Variable weight ops | Gerçek match sayısına göre fee refund |
| storage_alias | Migrations | DEX v1 → v2 order format migration |
| with_storage_layer | Atomic swap | Cross-pallet atomik işlem |

---

## Ek Tablo: Dosya Başına Analiz Özeti

| Dosya | Satır (tahmini) | Kritik İçerik |
|-------|----------------|---------------|
| frame/support/src/dispatch.rs | ~400 | RawOrigin, DispatchClass, PostDispatchInfo, Pays |
| frame/support/src/storage/types/value.rs | ~460 | StorageValue tam API |
| frame/support/src/storage/types/map.rs | ~500+ | StorageMap tam API |
| frame/support/src/storage/types/double_map.rs | ~500+ | StorageDoubleMap, partial iteration |
| frame/support/src/storage/types/nmap.rs | ~400+ | StorageNMap, KeyGenerator |
| frame/support/src/storage/types/counted_map.rs | ~500+ | CountedStorageMap, counter update |
| frame/support/src/storage/types/key.rs | ~200+ | KeyGenerator, NMapKey |
| frame/support/src/storage/generator/value.rs | ~200 | Storage backend implementasyonu |
| frame/support/src/storage/transactional.rs | ~200 | Nested transaction layers |
| frame/support/src/storage/bounded_vec.rs | ~80 | BoundedVec StorageTryAppend |
| frame/support/src/storage/mod.rs | ~600+ | StorageValue/Map/List traits |
| frame/support/src/traits.rs | ~144 | Tüm trait module'lerini re-export |
| frame/support/src/weights.rs | ~40 | Weight constants re-export |
| frame/support/src/weights/rocksdb_weights.rs | ~62 | RocksDB 25µs/100µs |
| frame/support/src/weights/extrinsic_weights.rs | ~82 | 108,157ns extrinsic base |
| frame/support/src/weights/block_weights.rs | ~60 | 431,614ns block base |
| frame/support/src/lib.rs | ~100+ | MAX_EXTRINSIC_DEPTH=256 |
| primitives/runtime/src/lib.rs | ~700+ | MultiSignature, DispatchError, AnySignature |
| primitives/runtime/src/transaction_validity.rs | ~430+ | InvalidTransaction, ValidTransaction |
| primitives/runtime/src/generic/era.rs | ~200+ | Era, Mortal, birth/death |
| primitives/runtime/src/generic/unchecked_extrinsic.rs | ~400+ | V4/V5 format, Preamble |
| primitives/runtime/src/traits/mod.rs | ~600+ | Verify, StaticLookup, Saturating |
| primitives/runtime/src/traits/transaction_extension/mod.rs | ~400+ | TransactionExtension trait |
| primitives/runtime/src/proving_trie/mod.rs | ~130 | TrieError |
| primitives/core/src/crypto.rs | ~600+ | DeriveJunction, SS58, PublicError |
| primitives/core/src/sr25519.rs | ~300+ | Schnorrkel, soft derivation |
| primitives/core/src/ed25519.rs | ~250+ | Ed25519-zebra, hard derivation only |
| primitives/core/src/ecdsa.rs | ~300+ | secp256k1/k256, dual backend |
| primitives/core/src/hasher.rs | ~58 | Blake2Hasher, KeccakHasher |
| primitives/core/src/hash.rs | ~100 | H256, H160 (primitive_types) |
| primitives/core/src/uint.rs | ~80 | U256, U512 (primitive_types) |
