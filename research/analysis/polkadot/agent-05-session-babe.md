# Substrate pallet-session + pallet-babe — Agent 05

Analiz Tarihi: 2026-07-22
Kaynak Dizinler:
- `/substrate/frame/session/` — 15 dosya, ~1280 satir
- `/substrate/frame/babe/` — 8 dosya, ~1161 satir + randomness.rs (174 satir) + equivocation.rs (251 satir)

---

## KISIM 1: pallet-session

### Kapsam

Analiz edilen dosyalar:
| Dosya | Satir | Icerik |
|-------|-------|--------|
| `src/lib.rs` | 1280 | Ana pallet, tum trait ve mantik |
| `src/disabling.rs` | 200 | Validator disabling stratejileri |
| `src/historical/mod.rs` | 517 | Merkle trie tabanli tarihsel session kaydı |
| `src/historical/offchain.rs` | 263 | Off-chain proof uretimi |
| `src/historical/onchain.rs` | 60 | On-chain validator set kaydi |
| `src/historical/shared.rs` | 37 | Paylasilan anahtar turetme sabitleri |
| `src/migrations/v1.rs` | 104 | DisabledValidators V0->V1 migrasyonu |
| `src/migrations/mod.rs` | (re-export) | Migration modulu |
| `src/weights.rs` | (trait) | WeightInfo trait tanimlamalari |
| `src/mock.rs` | (test) | Test mock yapilandirmasi |
| `src/tests.rs` | (test) | Entegrasyon testleri |
| `benchmarking/src/lib.rs` | (bench) | Benchmark modulu |
| `benchmarking/src/inner.rs` | (bench) | Benchmark implementasyonu |
| `benchmarking/src/mock.rs` | (bench) | Benchmark mock |

---

### Session Key Yonetimi

#### OpaqueKeys Trait ve Key Tipleri

Session key yonetimi `OpaqueKeys` trait uzerinden soyutlanmistir. Her validator birden fazla anahtar turunu bir arada tutar — ornegin BABE VRF anahtari, GRANDPA imzalama anahtari, IMONLINE anahtari vb.

`lib.rs:492` — Config trait icerisinde `Keys` tipi:
```rust
type Keys: OpaqueKeys + Member + Parameter + MaybeSerializeDeserialize;
```

`KeyTypeId` her anahtar turunu ayirt eder. `KeyOwner` storage map, `(KeyTypeId, Vec<u8>)` cifti uzerinden sahiplik sorgusuna izin verir:

```rust
// lib.rs:626-627
pub type KeyOwner<T: Config> =
    StorageMap<_, Twox64Concat, (KeyTypeId, Vec<u8>), T::ValidatorId, OptionQuery>;
```

#### set_keys Dispatchable (lib.rs:704-713)

```rust
pub fn set_keys(origin: OriginFor<T>, keys: T::Keys, proof: Vec<u8>) -> DispatchResult {
    let who = ensure_signed(origin)?;
    ensure!(
        who.using_encoded(|who| keys.ownership_proof_is_valid(who, &proof)),
        Error::<T>::InvalidProof,
    );
    Self::do_set_keys(&who, keys)?;
    Ok(())
}
```

Kritik noktalar:
1. Sahiplik kaniti (`proof`) zorunludur — caller'in anahtarlarin ozel yarısına erisimi oldugunu kanitlar
2. `do_set_keys` -> `inner_set_keys` cagri zinciri calisir
3. Anahtarlar bir sonraki session'da degil, sonraki 2. session'dan itibaren gecerli olur (`NextKeys` storage'a yazilir)
4. `KeyDeposit` miktarinda hold konur (ilk kayit veya external->local gecisinde)
5. Consumer referansi artirilir (`inc_consumers`)

#### inner_set_keys (lib.rs:962-994)

```rust
fn inner_set_keys(who: &T::ValidatorId, keys: T::Keys) -> Result<Option<T::Keys>, DispatchError> {
    let old_keys = Self::load_keys(who);

    for id in T::Keys::key_ids() {
        let key = keys.get_raw(*id);
        // Duplikasyon kontrolu: bu anahtar baska birine ait olamaz
        ensure!(
            Self::key_owner(*id, key).map_or(true, |owner| &owner == who),
            Error::<T>::DuplicatedKey,
        );
    }

    for id in T::Keys::key_ids() {
        let key = keys.get_raw(*id);
        if let Some(old) = old_keys.as_ref().map(|k| k.get_raw(*id)) {
            if key == old { continue; }
            Self::clear_key_owner(*id, old);  // Eski sahipligi sil
        }
        Self::put_key_owner(*id, key, who);   // Yeni sahipligi kaydet
    }

    Self::put_keys(who, &keys);
    Ok(old_keys)
}
```

Bu fonksiyon:
- Her key_id icin duplikasyon denetimi yapar (DuplicatedKey hatasi)
- Eski anahtarlarin sahipligini temizler
- Yeni anahtarlar icin sahiplik iliskisi kurar
- `NextKeys` map'e yazar
- Eski anahtarlari geri dondurur (yoksa `None`)

#### purge_keys Dispatchable (lib.rs:725-728)

```rust
pub fn purge_keys(origin: OriginFor<T>) -> DispatchResult {
    let who = ensure_signed(origin)?;
    Self::do_purge_keys(&who)?;
    Ok(())
}
```

`do_purge_keys` (lib.rs:996-1023):
- ValidatorId donusumu yapar (controller-stash modeli veya direct)
- `take_keys` ile NextKeys'ten anahtarlari alir ve siler
- Her key_id icin `clear_key_owner` cagirir
- `release_all` ile `HoldReason::Keys` altindaki fonlari serbest birakir (`BestEffort` precision)
- `ExternallySetKeys` kontrolu: Harici set edilmis ise consumer'i azaltmaz; local ise `dec_consumers` cagirir

#### HoldReason ve KeyDeposit

`lib.rs:587-591`:
```rust
#[pallet::composite_enum]
pub enum HoldReason {
    #[codec(index = 0)]
    Keys,
}
```

Validator, session key kaydederken `T::KeyDeposit` miktarinda token hold'a alinir. Bu mekanizma:
- Spam key kayitlarini onler
- Validator'u sistemde "diri" tutar
- Purge sonrasi serbest birakilir

#### ExternallySetKeys (lib.rs:629-635)

```rust
pub type ExternallySetKeys<T: Config> =
    StorageMap<_, Twox64Concat, T::AccountId, (), OptionQuery>;
```

`SessionInterface::set_keys` (XCM gibi harici kaynaklar icin) cagirildiginda:
- Deposit hold edilmez
- Consumer referansi artirilmaz
- Hesap `ExternallySetKeys`'e eklenir
- `purge_keys` sirasinda bu duruma gore farkli temizleme yapilir

---

### Session Rotation Mekanizmasi

#### Tetikleme: on_initialize (lib.rs:671-681)

```rust
fn on_initialize(n: BlockNumberFor<T>) -> Weight {
    if T::ShouldEndSession::should_end_session(n) {
        Self::rotate_session();
        T::BlockWeights::get().max_block
    } else {
        Weight::zero()
    }
}
```

Her blok basinda `ShouldEndSession` sorgulanir. Eger session bitmesi gerekiyorsa `rotate_session()` cagrilir. Agirlik: session dondugunde `max_block`, donmediginde `zero`.

#### PeriodicSessions (lib.rs:176-246)

En yaygın `ShouldEndSession` implementasyonu:

```rust
pub struct PeriodicSessions<Period, Offset>(PhantomData<(Period, Offset)>);

impl<...> ShouldEndSession<BlockNumber> for PeriodicSessions<Period, Offset> {
    fn should_end_session(now: BlockNumber) -> bool {
        let offset = Offset::get();
        now >= offset && ((now - offset) % Period::get()).is_zero()
    }
}
```

Formul: `session biter if: now >= offset AND (now - offset) % period == 0`

Ilk session uzunlugu `Offset` kadardir. Sonraki sessionlar `Period` kadardir.

Onemli: BABE kullanan zincirlerde `ShouldEndSession` BABE pallet'i tarafindan saglanir — blok numarasına degil slot sayisina gore karar verir.

#### rotate_session (lib.rs:776-867) — Tam Analiz

Bu kritik fonksiyon 7 ana adimda calisir:

**Adim 1: Onceki session bildirgesi**
```rust
let session_index = CurrentIndex::<T>::get();
let changed = QueuedChanged::<T>::get();

T::SessionHandler::on_before_session_ending();
T::SessionManager::end_session(session_index);
```

**Adim 2: Queued validator set'i aktive et**
```rust
let session_keys = QueuedKeys::<T>::get();
let validators = session_keys.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>();
Validators::<T>::put(&validators);

if changed {
    DisabledValidators::<T>::kill();  // Validator seti degistiyse disable listesini sifirla
}
```

**Adim 3: Index artir ve yeni session'i baslat**
```rust
let session_index = session_index + 1;
CurrentIndex::<T>::put(session_index);
T::SessionManager::start_session(session_index);
```

**Adim 4: Sonraki session icin validator aday listesi hazirla**
```rust
let maybe_next_validators = T::SessionManager::new_session(session_index + 1);
let (next_validators, next_identities_changed) = if let Some(validators) = maybe_next_validators {
    Self::deposit_event(Event::<T>::NewQueued);
    (validators, true)  // Her zaman changed=true (ekonomik kosullar degismis olabilir)
} else {
    (Validators::<T>::get(), false)  // Ayni validator seti
};
```

**Adim 5: Yeni validator'lar icin anahtarlari yukle ve kuyruga al**
```rust
let queued_amalgamated = next_validators.into_iter()
    .filter_map(|a| {
        let k = Self::load_keys(&a).or_else(|| {
            log!(warn, "failed to load session key for {:?}...", a);
            None
        })?;
        check_next_changed(&k);
        Some((a, k))
    })
    .collect::<Vec<_>>();

QueuedKeys::<T>::put(queued_amalgamated.clone());
QueuedChanged::<T>::put(next_changed);
```

NOT: Anahtari olmayan validator'lar filtre edilir ve uyari log'lanir!

**Adim 6: Event yayinla**
```rust
Self::deposit_event(Event::NewSession { session_index });
```

**Adim 7: Session handler'lari bilgilendir**
```rust
T::SessionHandler::on_new_session::<T::Keys>(changed, &session_keys, &queued_amalgamated);
```

#### Session Gecikmesi (N+2 Kurali)

Onemli tasarim karari: Anahtarlar 2 session gecikmeyle aktif olur.

- Session N'de `set_keys()` cagirilir → `NextKeys`'e yazilir
- Session N+1'de `rotate_session()` sirasinda `new_session(N+2)` cagirilir → `NextKeys`'ten okunur → `QueuedKeys`'e yazilir
- Session N+2'de `QueuedKeys` aktif validator set olur

Bu gecikme, fork durumunda cezalandirma olasiliklarini korur.

---

### Validator Set Degisimi

#### SessionManager Trait (lib.rs:249-280)

```rust
pub trait SessionManager<ValidatorId> {
    fn new_session(new_index: SessionIndex) -> Option<Vec<ValidatorId>>;
    fn new_session_genesis(new_index: SessionIndex) -> Option<Vec<ValidatorId>> {
        Self::new_session(new_index)  // Default: yeni session ile ayni
    }
    fn end_session(end_index: SessionIndex);
    fn start_session(start_index: SessionIndex);
}
```

Kritik semantik:
- `new_session(index)`: index numarali session icin validator aday listesi verir. `None` donerse mevcut set korunur. `Some(validators)` donerse ekonomik kosullar degismis sayilir ve `changed=true` olur.
- `new_session(session)` her zaman `end_session(session-1)`'den once cagirilacagi garanti edilmistir.
- `new_session_genesis(0)` ile `new_session_genesis(1)` genesis'te cagrilarak ilk iki session'i initialize eder.

#### SessionHandler Trait (lib.rs:291-388)

```rust
pub trait SessionHandler<ValidatorId> {
    const KEY_TYPE_IDS: &'static [KeyTypeId];
    fn on_genesis_session<Ks: OpaqueKeys>(validators: &[(ValidatorId, Ks)]);
    fn on_new_session<Ks: OpaqueKeys>(
        changed: bool,
        validators: &[(ValidatorId, Ks)],
        queued_validators: &[(ValidatorId, Ks)],
    );
    fn on_before_session_ending() {}
    fn on_disabled(validator_index: u32);
}
```

Tuple implementasyonu (lib.rs:330-378): `#[impl_trait_for_tuples::impl_for_tuples(1, 30)]` makrosu ile 1-30 elemanlı tuple'lar icin otomatik implementasyon saglanir. Her tuple elemani `OneSessionHandler<AId>`'yi implement etmelidir. Dispatch sirasinda her handler kendi key type ID'sine gore filtreleme yapar:

```rust
fn on_new_session<Ks: OpaqueKeys>(changed, validators, queued_validators) {
    for_tuples!(
        #(
            let our_keys = validators.iter()
                .filter_map(|k| k.1.get::<Tuple::Key>(<Tuple::Key as RuntimeAppPublic>::ID)
                    .map(|k1| (&k.0, k1)));
            let queued_keys = queued_validators.iter()
                .filter_map(|k| k.1.get::<Tuple::Key>(<Tuple::Key as RuntimeAppPublic>::ID)
                    .map(|k1| (&k.0, k1)));
            Tuple::on_new_session(changed, our_keys, queued_keys);
        )*
    )
}
```

#### ValidatorSet ve ValidatorRegistration

`lib.rs:1158-1197` — Pallet bu trait'leri implement eder:

```rust
impl<T: Config> ValidatorRegistration<T::ValidatorId> for Pallet<T> {
    fn is_registered(id: &T::ValidatorId) -> bool {
        Self::load_keys(id).is_some()
    }
}

impl<T: Config> ValidatorSet<T::AccountId> for Pallet<T> {
    type ValidatorId = T::ValidatorId;
    type ValidatorIdOf = T::ValidatorIdOf;
    fn session_index() -> sp_staking::SessionIndex { CurrentIndex::<T>::get() }
    fn validators() -> Vec<Self::ValidatorId> { Validators::<T>::get() }
}
```

#### upgrade_keys (lib.rs:884-917)

Runtime upgrade sirasinda key tip degisiklikleri icin kullanilir:

```rust
pub fn upgrade_keys<Old, F>(upgrade: F)
where
    Old: OpaqueKeys + Member + Decode,
    F: Fn(T::ValidatorId, Old) -> T::Keys,
{
    // NextKeys cevir
    NextKeys::<T>::translate::<Old, _>(|val, old_keys| {
        for i in old_ids.iter() { Self::clear_key_owner(*i, old_keys.get_raw(*i)); }
        let new_keys = upgrade(val.clone(), old_keys);
        for i in new_ids.iter() { Self::put_key_owner(*i, new_keys.get_raw(*i), &val); }
        Some(new_keys)
    });
    // QueuedKeys cevir
    let _ = QueuedKeys::<T>::translate::<Vec<(T::ValidatorId, Old)>, _>(|k| { ... });
}
```

---

### OneSessionHandler Trait

`OneSessionHandler`, pallet-session ve consensus pallet'leri (BABE, GRANDPA, IMONLINE vb.) arasindaki kopruyu olusturur. `frame_support::traits` icerisinde tanimlanmistir ve BABE pallet'i bunu implement eder (bkz. KISIM 2).

Temel metotlar:
```rust
pub trait OneSessionHandler<AId>: BoundToRuntimeAppPublic {
    type Key: Decode + Default + RuntimeAppPublic;

    fn on_genesis_session<'a, I: 'a>(validators: I)
    where I: Iterator<Item = (&'a AId, Self::Key)>;

    fn on_new_session<'a, I: 'a>(_changed: bool, validators: I, queued_validators: I)
    where I: Iterator<Item = (&'a AId, Self::Key)>;

    fn on_before_session_ending() {}

    fn on_disabled(i: u32);
}
```

Her consensus pallet kendi `Key` tipiyle bu trait'i implement eder. `SessionHandler` trait'i bunu `for_tuples!` makrosu ile birden fazla `OneSessionHandler`'i tek bir `SessionHandler`'a donusturur.

---

### Disabling Stratejileri (disabling.rs)

#### DisablingStrategy Trait

```rust
pub trait DisablingStrategy<T: Config> {
    fn decision(
        offender_stash: &T::ValidatorId,
        offender_slash_severity: OffenceSeverity,
        currently_disabled: &Vec<(u32, OffenceSeverity)>,
    ) -> DisablingDecision;
}

pub struct DisablingDecision {
    pub disable: Option<u32>,   // Devre disi birakilacak validator indeksi
    pub reenable: Option<u32>,  // Tekrar etkinlestirilecek validator indeksi
}
```

#### UpToLimitDisablingStrategy<FACTOR=3>

```rust
pub struct UpToLimitDisablingStrategy<const DISABLING_LIMIT_FACTOR: usize = 3>;

// Limit formulu:
fn disable_limit(validators_len: usize) -> usize {
    (validators_len - 1) / DISABLING_LIMIT_FACTOR
}
```

Varsayilan FACTOR=3 -> Toplam validator sayisinin 1/3'ü devre disi birakilabilir (Byzantine eşigi). Limit asilinca yeni disable islemi yapilmaz.

#### UpToLimitWithReEnablingDisablingStrategy<FACTOR=3>

Daha akilli strateji:
1. Offender zaten disabled ve ayni/dusuk severity: Hicbir sey yapma
2. Offender zaten disabled ve daha yuksek severity: Severity'yi guncelle
3. Limit asilmadiysa: Normal disable
4. Limit asildi ve yeni offender daha yuksek severity: En dusuk severity'li disabled validator'i re-enable et, yeni offender'i disable et
5. Limit asildi ve yeni offender daha dusuk/esit severity: Hicbir sey yapma

Bu "trade-off" karar: Blokzincirin guvenligini daha az tehlikeye atan (daha dusuk severity) offender'i re-enable edip daha tehlikeli offender'i disable et.

#### disable_index_with_severity (lib.rs:1055-1089)

```rust
pub fn disable_index_with_severity(i: u32, severity: OffenceSeverity) -> bool {
    DisabledValidators::<T>::mutate(|disabled| {
        match disabled.binary_search_by_key(&i, |(index, _)| *index) {
            Ok(index) => {
                // Zaten disabled: severity guncelle (sadece daha yuksekse)
                if severity > disabled[index].1 {
                    disabled[index].1 = severity;
                }
                true
            },
            Err(index) => {
                // Yeni disable: event yayinla, on_disabled cagirir
                Self::deposit_event(Event::ValidatorDisabled { validator: ... });
                disabled.insert(index, (i, severity));
                T::SessionHandler::on_disabled(i);
                true
            },
        }
    })
}
```

`DisabledValidators` listesi her zaman sirali tutulur (binary_search icin).

---

### Historical Session Modulu (historical/mod.rs)

#### Amac ve Tasarim

Historical module, slashing icin hesap verebilirlik saglar: Bir onceki session'lardan gelen validator'lari keserek cezalandirmak icin o session'daki validator setine erisim gerekir. Tum validator setini saklamak yerine, Merkle trie koku (hash) ve validator sayisi saklanir. Ihtiyac aninda Merkle kaniti (proof) olusturulur.

```rust
pub type HistoricalSessions<T: Config> =
    StorageMap<_, Twox64Concat, SessionIndex, (T::Hash, ValidatorCount), OptionQuery>;

pub type StoredRange<T> = StorageValue<_, (SessionIndex, SessionIndex), OptionQuery>;
```

#### ProvingTrie (historical/mod.rs:257-391)

Trie yapisi:
- **Key → Index mapping**: `(KeyTypeId, key_bytes)` -> `u32` (validator indeksi)
- **Index → Identification mapping**: `u32` -> `(ValidatorId, FullIdentification)`

TrieDB'nin `LayoutV0<T::Hashing>` layout'u kullanilir.

`generate_for`:
```rust
fn generate_for<I>(validators: I) -> Result<Self, &'static str>
where I: IntoIterator<Item = (T::ValidatorId, T::FullIdentification)>
{
    let mut db = MemoryDB::with_hasher(RandomState::default());
    let mut root = Default::default();
    let mut trie = TrieDBMutBuilderV0::new(&mut db, &mut root).build();

    for (i, (validator, full_id)) in validators.into_iter().enumerate() {
        let i = i as u32;
        let keys = Session::load_keys(&validator).ok_or continue;

        // Her anahtar turu icin: (key_id, key) -> index
        for key_id in T::Keys::key_ids() {
            let key = keys.get_raw(*key_id);
            (key_id, key).using_encoded(|k| i.using_encoded(|v| trie.insert(k, v)));
        }

        // Index -> (validator, full_identification)
        i.using_encoded(|k| id_tuple.using_encoded(|v| trie.insert(k, v)));
    }
}
```

`prove`: `Recorder` kullanarak Merkle kaniti olusturur.
`check_proof`: `StorageProof`'dan trie yeniden insar eder ve `AccessedNodesTracker` ile kullanilmayan node'lar kontrolu yapar.

#### KeyOwnerProofSystem

```rust
impl<T: Config, D: AsRef<[u8]>> KeyOwnerProofSystem<(KeyTypeId, D)> for Pallet<T> {
    type Proof = MembershipProof;
    type IdentificationTuple = IdentificationTuple<T>;

    fn prove(key: (KeyTypeId, D)) -> Option<Self::Proof> { ... }
    fn check_proof(key: (KeyTypeId, D), proof: Self::Proof) -> Option<IdentificationTuple<T>> { ... }
}
```

`check_proof` akisi:
1. `proof.session == current_index()` ise: Mevcut validator setinden trie olustur
2. Aksi halde: `HistoricalSessions[proof.session]`'dan (root, count) al
3. `count != proof.validator_count` ise `None`
4. `StorageProof::new_with_duplicate_nodes_check` ile kaniti dogrula
5. `AccessedNodesTracker::ensure_no_unused_nodes()` — kullanilmayan trie node'lari var mi?
6. Trie'yi sorgula ve `IdentificationTuple` dondur

#### Off-chain Validator Set Depolama (onchain.rs + offchain.rs)

```rust
// onchain.rs
pub fn store_session_validator_set_to_offchain<T>(session_index: SessionIndex) {
    let encoded_validator_list = <SessionModule<T>>::validators()
        .into_iter()
        .filter_map(|validator_id| {
            T::FullIdentificationOf::convert(validator_id.clone())
                .map(|full_identification| (validator_id, full_identification))
        })
        .collect::<Vec<_>>();

    encoded_validator_list.using_encoded(|encoded| {
        let derived_key = shared::derive_key(shared::PREFIX, session_index);
        sp_io::offchain_index::set(derived_key.as_slice(), encoded);
    });
}
```

Key derivasyon formulu (`shared.rs:29-36`):
```
key = b"session_historical" + b"/" + SCALE_encoded(session_index)
```

`NoteHistoricalRoot<T, I>` wrapperi `SessionManager`'i sarar ve her session sonunda `store_session_validator_set_to_offchain` cagirir.

Off-chain tarafta `prove_session_membership` fonksiyonu, offchain DB'den validator setini yukleyerek Merkle kaniti olusturur.

---

### Storage Items (Tam Liste)

| Storage Item | Tip | Aciklama |
|---|---|---|
| `Validators<T>` | `Vec<T::ValidatorId>` | Aktif validator seti |
| `CurrentIndex<T>` | `SessionIndex` | Mevcut session indeksi |
| `QueuedChanged<T>` | `bool` | Queued validator seti degisti mi? |
| `QueuedKeys<T>` | `Vec<(T::ValidatorId, T::Keys)>` | Sonraki session icin anahtar ciftleri |
| `DisabledValidators<T>` | `Vec<(u32, OffenceSeverity)>` | Devre disi validator'lar (sirali) |
| `NextKeys<T>` | `Map<ValidatorId, T::Keys>` | Validator'in kayitli session anahtarlari |
| `KeyOwner<T>` | `Map<(KeyTypeId, Vec<u8>), ValidatorId>` | Anahtar sahipligi |
| `ExternallySetKeys<T>` | `Map<AccountId, ()>` | Harici ayarlanan anahtarlar |
| Historical: `HistoricalSessions<T>` | `Map<SessionIndex, (Hash, ValidatorCount)>` | Tarihsel session kokleri |
| Historical: `StoredRange<T>` | `Option<(SessionIndex, SessionIndex)>` | [first, last) aralik |

---

### Events ve Errors

#### Events (lib.rs:637-650)
```rust
pub enum Event<T: Config> {
    NewSession { session_index: SessionIndex },  // Yeni session basladı
    NewQueued,                                   // Yeni validator seti kuyruğa alındı
    ValidatorDisabled { validator: T::ValidatorId },   // Validator devre dışı bırakıldı
    ValidatorReenabled { validator: T::ValidatorId },  // Validator yeniden etkinleştirildi
}
```

Historical Events:
```rust
pub enum Event<T> {
    RootStored { index: SessionIndex },   // Merkle root saklandı
    RootsPruned { up_to: SessionIndex },  // Eski rootlar temizlendi
}
```

#### Errors (lib.rs:652-665)
```rust
pub enum Error<T> {
    InvalidProof,              // Gecersiz sahiplik kaniti
    NoAssociatedValidatorId,   // Hesaba bagli ValidatorId yok
    DuplicatedKey,             // Ayni anahtar baska hesapta kayitli
    NoKeys,                    // Bu hesabin anahtari yok
    NoAccount,                 // Hesap aktif degil (consumer kapasitesi yok)
}
```

---

### Migrasyonlar

#### V0 -> V1: DisabledValidators (migrations/v1.rs)

Onceki format: `Vec<u32>` (sadece indeks)
Yeni format: `Vec<(u32, OffenceSeverity)>` (indeks + siddet)

```rust
fn on_runtime_upgrade() -> Weight {
    let disabled = S::take_disabled();  // Eski format oku
    NewDisabledValidators::<T>::put(disabled);  // Yeni format yaz
    T::DbWeight::get().reads_writes(1, 1)
}
```

Migrasyon stratejisi: `InitOffenceSeverity<T>` — mevcut disabled validator'lara `OffenceSeverity::max_severity()` atanir (conservative).

---

### Nova Shield Notu (pallet-session)

**Dogrudan kullanim**: pallet-session aynen kullanilabilir; kendi consensus pallet'imizi `OneSessionHandler` ile entegre ederiz.

**Kritik kararlar**:
1. `ShouldEndSession` implementasyonu: BABE gibi slot tabanli ya da basit `PeriodicSessions` secebiliriz. Nova Shield icin validasyon periyodu slot tabanli olmali.
2. `SessionManager` implementasyonu: Staking pallet'imiz (gelecekte) bu trait'i implement edecek. Simdilik statik validator set icin `()` implementation yeterli.
3. `DisablingStrategy`: `UpToLimitWithReEnablingDisablingStrategy<3>` tercih edilmeli — Byzantine esigi korunurken dinamik re-enabling saglar.
4. `KeyDeposit`: Spam key kayitlarini onlemek icin pozitif bir deger set edilmeli. Sifir da olur (test/basit network), ancak production'da mutlaka non-zero olmali.
5. **N+2 gecikmesi**: Tasarim geregi — kendi staking/slashing sistemimizde bunu dikkate almaliyiz.
6. `historical` feature: Ceza mekanizmasi (slashing) tasarlarken zorunlu. MPC/TSS tabanlı ceza mekanizmamizda `ProvingTrie` + `KeyOwnerProofSystem` kullanilacak.
7. `ExternallySetKeys`: Relay chain <-> parachain key senkronizasyonu icin. Nova Shield tek L1 olarak buna simdilik ihtiyac duymaz, ancak XCM entegrasyonu geldikce gerekli olacak.

---

## KISIM 2: pallet-babe

### Kapsam

Analiz edilen dosyalar:
| Dosya | Satir | Icerik |
|-------|-------|--------|
| `src/lib.rs` | 1161 | Ana pallet: VRF, epoch, randomness, equivocation |
| `src/randomness.rs` | 174 | Randomness implementasyonlari |
| `src/equivocation.rs` | 251 | Equivocation detection ve reporting |
| `src/mock.rs` | 441 | Test mock ve yardimci fonksiyonlar |
| `src/tests.rs` | (test) | Entegrasyon testleri |
| `src/benchmarking.rs` | (bench) | Benchmark implementasyonu |
| `src/default_weights.rs` | 55 | Manuel yazilmis agirlik hesaplari |
| `src/weights.rs` | (trait) | WeightInfo trait |

---

### VRF Tabanlı Lider Secimi

#### BABE Protokol Ozeti

BABE (Blind Assignment for Blockchain Extension), slot-tabanli probabilistik bir lider secim protokoluudur. Her slot icin her validator bir VRF hesaplamasi yapar. VRF ciktisi belirli bir esik degerinin altindaysa o validator o slotu claim edebilir (Primary slot). VRF ciktisinın konfigurasyona bagli `c` parametresiyle belirlenen bir olasilikla slot sahibi olunur.

#### Slot Tipleri

BABE 3 slot tipini destekler (`AllowedSlots`):
1. **PrimarySlots**: Sadece VRF-winning validatorlar
2. **PrimaryAndSecondaryPlainSlots**: Primary + fallback secondary (deterministik round-robin, VRF yok)
3. **PrimaryAndSecondaryVRFSlots**: Primary + VRF-based secondary

Test mock'unda (`mock.rs:376-381`):
```rust
epoch_config: sp_consensus_babe::BabeEpochConfiguration {
    c: (1, 4),  // 1/4 = %25 doluluk orani hedefi
    allowed_slots: sp_consensus_babe::AllowedSlots::PrimaryAndSecondaryPlainSlots,
},
```

#### PreDigest ve AuthorityIndex

Her blok, block header digest'ine bir `PreDigest` ekler:

```rust
// sp_consensus_babe/digests
pub enum PreDigest {
    Primary(PrimaryPreDigest),           // VRF winning slot
    SecondaryPlain(SecondaryPlainPreDigest), // Round-robin fallback
    SecondaryVRF(SecondaryVRFPreDigest), // VRF-based secondary
}
```

Her digest'te `authority_index: u32` var — bu index `Authorities` listesindeki pozisyona karsilik gelir.

#### on_initialize'da VRF Islemi (lib.rs:335-338)

```rust
fn on_initialize(now: BlockNumberFor<T>) -> Weight {
    Self::initialize(now);
    Weight::zero()
}
```

#### on_finalize'da VRF Dogrulama (lib.rs:346-401)

```rust
fn on_finalize(_now: BlockNumberFor<T>) {
    if let Some(pre_digest) = Initialized::<T>::take().flatten() {
        let authority_index = pre_digest.authority_index();

        // Disabled validator blok yazamaz — panic!
        if T::DisabledValidators::is_disabled(authority_index) {
            panic!("Validator with index {:?} is disabled...", authority_index);
        }

        if let Some(signature) = pre_digest.vrf_signature() {
            let randomness: Option<BabeRandomness> = Authorities::<T>::get()
                .get(authority_index as usize)
                .and_then(|(authority, _)| {
                    let public = authority.as_inner_ref();
                    let transcript = sp_consensus_babe::make_vrf_transcript(
                        &Randomness::<T>::get(),
                        CurrentSlot::<T>::get(),
                        EpochIndex::<T>::get(),
                    );

                    // Client tarafinda dogrulama zaten yapildi; runtime'da yeniden dogrulama yok
                    debug_assert!({
                        use sp_core::crypto::VrfPublic;
                        public.vrf_verify(&transcript.clone().into_sign_data(), &signature)
                    });

                    public.make_bytes(
                        RANDOMNESS_VRF_CONTEXT,
                        &transcript,
                        &signature.pre_output,
                    ).ok()
                });

            // Sadece Primary slot VRF ciktisi randomness'a katkida bulunur
            if let Some(randomness) = pre_digest.is_primary().then(|| randomness).flatten() {
                Self::deposit_randomness(&randomness);
            }

            AuthorVrfRandomness::<T>::put(randomness);
        }
    }

    Lateness::<T>::kill();
}
```

Kritik gozlem: `debug_assert!` — VRF dogrulamasi runtime'da PRODUCTION'da calistirilmaz. Dogrulama istemci (client) tarafindan blok import edilirken yapilir. Runtime sadece ciktiyi kullanir.

#### VRF Transcript Yapisi

```rust
sp_consensus_babe::make_vrf_transcript(
    &Randomness::<T>::get(),  // Mevcut epoch randomness
    CurrentSlot::<T>::get(),  // Slot numarasi
    EpochIndex::<T>::get(),   // Epoch indeksi
)
```

Bu uc deger VRF'in "mesaji"ni olusturur. VRF, deterministic olmakla birlikte verifiable randomness uretir: ayni anahtar + ayni transcript -> ayni cikti, ama ciktiyi onceden tahmin etmek imkansiz.

#### Randomness Birikimi: deposit_randomness (lib.rs:783-800)

```rust
fn deposit_randomness(randomness: &BabeRandomness) {
    let segment_idx = SegmentIndex::<T>::get();
    let mut segment = UnderConstruction::<T>::get(&segment_idx);
    if segment.try_push(*randomness).is_ok() {
        UnderConstruction::<T>::insert(&segment_idx, &segment);
    } else {
        // Segment doldu, yeni segment ac
        let segment_idx = segment_idx + 1;
        let bounded_randomness = BoundedVec::<_, ConstU32<UNDER_CONSTRUCTION_SEGMENT_LENGTH>>::
            try_from(vec![*randomness]).expect(">=1");
        UnderConstruction::<T>::insert(&segment_idx, bounded_randomness);
        SegmentIndex::<T>::put(&segment_idx);
    }
}
```

`UNDER_CONSTRUCTION_SEGMENT_LENGTH = 256` (lib.rs:110): Her segment maksimum 256 VRF ciktisi tutar.

---

### Epoch Degisimi

#### EpochChangeTrigger Trait (lib.rs:81-108)

```rust
pub trait EpochChangeTrigger {
    fn trigger<T: Config>(now: BlockNumberFor<T>);
}
```

Iki implementasyon:

1. **ExternalTrigger** (lib.rs:89-93): Hicbir sey yapmaz. pallet-session `ShouldEndSession`'i tetikler ve BABE `OneSessionHandler::on_new_session` uzerinden epoch degistirir.

```rust
impl EpochChangeTrigger for ExternalTrigger {
    fn trigger<T: Config>(_: BlockNumberFor<T>) {}
}
```

2. **SameAuthoritiesForever** (lib.rs:97-108): Dahili tetikleyici, session pallet'i olmadan da epoch degistirir. Ayni authority seti sonsuza kadar devam eder.

```rust
impl EpochChangeTrigger for SameAuthoritiesForever {
    fn trigger<T: Config>(now: BlockNumberFor<T>) {
        if Pallet::<T>::should_epoch_change(now) {
            let authorities = Authorities::<T>::get();
            let next_authorities = authorities.clone();
            Pallet::<T>::enact_epoch_change(authorities, next_authorities, None);
        }
    }
}
```

**Nova Shield icin**: `ExternalTrigger` kullanilmali — pallet-session ile entegrasyon saglar.

#### should_epoch_change (lib.rs:577-590)

```rust
pub fn should_epoch_change(now: BlockNumberFor<T>) -> bool {
    // Block #1'de epoch donusumu yapilmaz (genesis slotu 0 olarak basladi)
    now != One::one() && {
        let diff = CurrentSlot::<T>::get().saturating_sub(Self::current_epoch_start());
        *diff >= T::EpochDuration::get()
    }
}
```

Formul: `current_slot - epoch_start_slot >= EpochDuration`

Slot tabanli karar: Blok numarasina degil, slot sayisina gore. Bos slotlar varsa blok sayisi slot sayisindan az olacak ve epoch daha erken bitecek.

#### ShouldEndSession implementasyonu (lib.rs:515-524)

BABE, `pallet_session::ShouldEndSession`'i implement eder:

```rust
impl<T: Config> pallet_session::ShouldEndSession<BlockNumberFor<T>> for Pallet<T> {
    fn should_end_session(now: BlockNumberFor<T>) -> bool {
        // Session pallet on_initialize'dan once cagrilabilir; initialize'i zorla calistir
        Self::initialize(now);
        Self::should_epoch_change(now)
    }
}
```

#### enact_epoch_change (lib.rs:623-723)

En kritik fonksiyon — epoch degisimini gerceklestirir:

```rust
pub fn enact_epoch_change(
    authorities: WeakBoundedVec<(AuthorityId, BabeAuthorityWeight), T::MaxAuthorities>,
    next_authorities: WeakBoundedVec<(AuthorityId, BabeAuthorityWeight), T::MaxAuthorities>,
    session_index: Option<SessionIndex>,
) {
    debug_assert!(Initialized::<T>::get().is_some());

    if authorities.is_empty() {
        log::warn!(target: LOG_TARGET, "Ignoring empty epoch change.");
        return;
    }

    // Epoch indeksini slottan hesapla (boş slot atlamalarina karsin)
    let epoch_index = sp_consensus_babe::epoch_index(
        CurrentSlot::<T>::get(),
        GenesisSlot::<T>::get(),
        T::EpochDuration::get(),
    );

    // Epoch atlama kontrolu
    let current_epoch_index = EpochIndex::<T>::get();
    if current_epoch_index.saturating_add(1) != epoch_index {
        if let Some(session_index) = session_index {
            SkippedEpochs::<T>::mutate(|skipped_epochs| {
                if epoch_index < session_index as u64 { warn!(...); return; }
                if skipped_epochs.is_full() { skipped_epochs.remove(0); } // O(n) ama kucuk liste
                skipped_epochs.force_push((epoch_index, session_index));
            })
        }
    }

    EpochIndex::<T>::put(epoch_index);
    Authorities::<T>::put(authorities);

    // Randomness degisimi: mevcut epoch randomness'i NextRandomness'tan al
    let next_epoch_index = epoch_index.checked_add(1).expect("...");
    let randomness = Self::randomness_change_epoch(next_epoch_index);
    Randomness::<T>::put(randomness);

    // Sonraki epoch authority'lerini kaydet
    NextAuthorities::<T>::put(&next_authorities);

    // EpochStart guncelle: (onceki_epoch_baslangic, mevcut_epoch_baslangic)
    EpochStart::<T>::mutate(|(prev_start, curr_start)| {
        *prev_start = core::mem::take(curr_start);
        *curr_start = <frame_system::Pallet<T>>::block_number();
    });

    // NextEpochData konsensus log'u yayinla
    let next_randomness = NextRandomness::<T>::get();
    let next_epoch = NextEpochDescriptor {
        authorities: next_authorities.into_inner(),
        randomness: next_randomness,
    };
    Self::deposit_consensus(ConsensusLog::NextEpochData(next_epoch));

    // Bekleyen epoch config degisimini uygula
    if let Some(next_config) = NextEpochConfig::<T>::get() {
        EpochConfig::<T>::put(next_config);
    }
    if let Some(pending_change) = PendingEpochConfigChange::<T>::take() {
        let next_epoch_config: BabeEpochConfiguration = pending_change.clone().into();
        NextEpochConfig::<T>::put(next_epoch_config);
        Self::deposit_consensus(ConsensusLog::NextConfigData(pending_change));
    }
}
```

#### Epoch Atlama (Skipping) Mekanizmasi

Eger zincir `EpochDuration` slot'tan uzun suredur cevrimdisi kaldiysa, epoch'lar atlanir. Bu durumda:
- `epoch_index = (current_slot - genesis_slot) / EpochDuration`
- Atlanan epoch'lar `SkippedEpochs: BoundedVec<(u64, SessionIndex), ConstU32<100>>` de tutulur
- Bu liste equivocation kaniti dogrulamada epoch-session eslesmesi icin kullanilir
- Liste dolunca ilk eleman silinir (O(n) ama maks 100 eleman)
- Kural: `epoch_index >= session_index` her zaman gecerli (epoch'lar atlanabilir, session'lar atlanamaz)

#### current_epoch_start (lib.rs:729-735)

```rust
pub fn current_epoch_start() -> Slot {
    sp_consensus_babe::epoch_start_slot(
        EpochIndex::<T>::get(),
        GenesisSlot::<T>::get(),
        T::EpochDuration::get(),
    )
}
```

Formul (sp_consensus_babe'de): `genesis_slot + epoch_index * epoch_duration`

#### OneSessionHandler implementasyonu (lib.rs:1048-1092)

```rust
impl<T: Config> OneSessionHandler<T::AccountId> for Pallet<T>
where T: pallet_session::Config
{
    type Key = AuthorityId;

    fn on_genesis_session<'a, I: 'a>(validators: I)
    where I: Iterator<Item = (&'a T::AccountId, AuthorityId)> {
        let authorities = validators.map(|(_, k)| (k, 1)).collect::<Vec<_>>();
        Self::initialize_genesis_authorities(&authorities);
    }

    fn on_new_session<'a, I: 'a>(_changed: bool, validators: I, queued_validators: I)
    where I: Iterator<Item = (&'a T::AccountId, AuthorityId)> {
        let authorities = validators.map(|(_account, k)| (k, 1)).collect::<Vec<_>>();
        let bounded_authorities = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(
            authorities, Some("Warning: ..."),
        );

        let next_authorities = queued_validators.map(|(_account, k)| (k, 1)).collect::<Vec<_>>();
        let next_bounded_authorities = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(
            next_authorities, Some("Warning: ..."),
        );

        let session_index = <pallet_session::Pallet<T>>::current_index();
        Self::enact_epoch_change(bounded_authorities, next_bounded_authorities, Some(session_index))
    }

    fn on_disabled(i: u32) {
        Self::deposit_consensus(ConsensusLog::OnDisabled(i))
    }
}
```

Onemli: Tum authority weight'leri `1` olarak set edilir (uniform weight). Gercek stake-weighted BABE icin bu degerin staking ile entegre edilmesi gerekir.

`on_disabled`: Consensus log yayinlar, istemciler bu log'u gordukleri anda o validator'dan gelen bloklari reddeder.

---

### Randomness Uretimi

#### randomness_change_epoch (lib.rs:877-892)

```rust
fn randomness_change_epoch(next_epoch_index: u64) -> BabeRandomness {
    let this_randomness = NextRandomness::<T>::get();
    let segment_idx: u32 = SegmentIndex::<T>::mutate(|s| core::mem::replace(s, 0));

    // Segment boyutunu fazla tahmin et (tam segment olduğunu varsay)
    let rho_size = (segment_idx.saturating_add(1) * UNDER_CONSTRUCTION_SEGMENT_LENGTH) as usize;

    let next_randomness = compute_randomness(
        this_randomness,
        next_epoch_index,
        (0..segment_idx).flat_map(|i| UnderConstruction::<T>::take(&i)),
        Some(rho_size),
    );
    NextRandomness::<T>::put(&next_randomness);
    this_randomness  // Mevcut epoch icin kullanilacak randomness
}
```

Bu fonksiyon cagrildiginda:
- `NextRandomness` -> `this_randomness` (mevcut epoch'a atanacak)
- `SegmentIndex` sifirlanir (tum segment'ler temizlenir)
- `UnderConstruction` take ile silinir
- `next_randomness` hesaplanir ve `NextRandomness`'a yazilir

#### compute_randomness (lib.rs:1098-1113) — Matematiksel Formul

```rust
fn compute_randomness(
    last_epoch_randomness: BabeRandomness,
    epoch_index: u64,
    rho: impl Iterator<Item = BabeRandomness>,
    rho_size_hint: Option<usize>,
) -> BabeRandomness {
    let mut s = Vec::with_capacity(40 + rho_size_hint.unwrap_or(0) * RANDOMNESS_LENGTH);
    s.extend_from_slice(&last_epoch_randomness);        // 32 byte: onceki epoch randomness
    s.extend_from_slice(&epoch_index.to_le_bytes());    // 8 byte: epoch indeksi (LE)

    for vrf_output in rho {
        s.extend_from_slice(&vrf_output[..]);           // 32 byte/vrf: bu epoch'taki VRF ciktilari
    }

    sp_io::hashing::blake2_256(&s)   // Blake2-256 hash
}
```

**Formul**: `next_randomness = Blake2_256(r_n || n+1 || VRF_1 || VRF_2 || ... || VRF_k)`

- `r_n`: Mevcut epoch randomness (32 byte)
- `n+1`: Sonraki epoch indeksi (8 byte, little-endian)
- `VRF_1..VRF_k`: Bu epoch'ta Primary slot'larda uretan tum VRF ciktilari

Test sabiti (`tests.rs:33-36`):
```rust
const EMPTY_RANDOMNESS: [u8; RANDOMNESS_LENGTH] = [
    74, 25, 49, 128, 53, 97, 244, 49, 222, 202, 176, 2, 231, 66, 95, 10,
    133, 49, 213, 228, 86, 161, 164, 127, 217, 153, 138, 37, 48, 192, 248, 0,
];
// Blake2_256([0u8;32] || 0u64_le) = EMPTY_RANDOMNESS — dogrulanmis
```

#### Randomness Tipleri (randomness.rs)

Dort randomness implementasyonu mevcuttur:

**1. RandomnessFromTwoEpochsAgo** (randomness.rs:54, 133-141)
```rust
impl<T: Config> RandomnessT<T::Hash, BlockNumberFor<T>> for RandomnessFromTwoEpochsAgo<T> {
    fn random(subject: &[u8]) -> (T::Hash, BlockNumberFor<T>) {
        let mut subject = subject.to_vec();
        subject.extend_from_slice(&Randomness::<T>::get()[..]);
        (T::Hashing::hash(&subject[..]), EpochStart::<T>::get().0)  // .0 = onceki epoch baslangici
    }
}
```
- Kullanilan veri: `Randomness` storage (mevcut epoch randomness = iki epoch onceki VRF'lerden turetildi)
- Zaman damgasi: Onceki epoch'un baslangic blogu
- Guvenligi: En yuksek (finality bagimli sistemler icin)

**2. RandomnessFromOneEpochAgo** (randomness.rs:82, 143-151)
```rust
impl<T: Config> RandomnessT<T::Hash, BlockNumberFor<T>> for RandomnessFromOneEpochAgo<T> {
    fn random(subject: &[u8]) -> (T::Hash, BlockNumberFor<T>) {
        let mut subject = subject.to_vec();
        subject.extend_from_slice(&NextRandomness::<T>::get()[..]);
        (T::Hashing::hash(&subject[..]), EpochStart::<T>::get().1)  // .1 = mevcut epoch baslangici
    }
}
```
- Kullanilan veri: `NextRandomness` (bir onceki epoch'un VRF'lerinden hesaplandi)
- Kullanim: Polkadot parachain muzayedesı — bias azalir, finality gecikmesi gerekmez
- Guvenligi: Orta

**3. ParentBlockRandomness** (randomness.rs:122, 153-165)
```rust
impl<T: Config> RandomnessT<Option<T::Hash>, BlockNumberFor<T>> for ParentBlockRandomness<T> {
    fn random(subject: &[u8]) -> (Option<T::Hash>, BlockNumberFor<T>) {
        let random = AuthorVrfRandomness::<T>::get().map(|random| {
            let mut subject = subject.to_vec();
            subject.extend_from_slice(&random);
            T::Hashing::hash(&subject[..])
        });
        (random, current_block_number - 1)
    }
}
```
- Kullanilan veri: `AuthorVrfRandomness` (parent blokta yazildi, finalize sirasinda guncellendi)
- Zaman damgasi: parent block number
- Guvenligi: En dusuk — blok productor kendi slotunu atlamayı tercih edebilir
- `Option<T::Hash>`: Secondary plain slot'larda VRF yok, `None` doner

**4. CurrentBlockRandomness** (randomness.rs:131, 167-172) — DEPRECATED
```rust
#[deprecated(note = "Will not provide fresh randomness for the current block. Use ParentBlockRandomness.")]
pub struct CurrentBlockRandomness<T>(...);
```

#### RANDOMNESS_VRF_CONTEXT

VRF ciktisini randomness byte'larına donusturmek icin kullanilan context:

```rust
// sp_consensus_babe'de
pub const RANDOMNESS_VRF_CONTEXT: &[u8] = b"substrate-babe-vrf";
```

`public.make_bytes(RANDOMNESS_VRF_CONTEXT, &transcript, &signature.pre_output)` bu context ile 32 byte deterministik randomness uretir.

---

### BABE Sabitleri [kaynak koddan]

| Sabit | Deger / Tip | Konum | Aciklama |
|-------|-------------|-------|----------|
| `UNDER_CONSTRUCTION_SEGMENT_LENGTH` | `256` | lib.rs:110 | Her segmentte maks VRF ciktisi |
| `RANDOMNESS_LENGTH` | `32` (sp_consensus_babe) | babe primitive | Randomness byte uzunlugu |
| `BABE_ENGINE_ID` | `*b"BABE"` (sp_consensus_babe) | babe primitive | Konsensus engine ID |
| `RANDOMNESS_VRF_CONTEXT` | `b"substrate-babe-vrf"` | sp_consensus_babe | VRF context |
| `KEY_TYPE` | `sp_core::crypto::key_types::BABE` | sp_consensus_babe | Key type ID |
| `SkippedEpochs` bound | `100` | lib.rs:312 | Maks saklanan atlanan epoch |
| Test `EpochDuration` | `3` | mock.rs:189 | Testlerde 3 slot = 1 epoch |
| Test `SessionsPerEra` | `3` | mock.rs:147 | Testlerde 3 session = 1 era |
| Test `BondingDuration` | `3` | mock.rs:148 | Era cinsinden bonding suresi |
| Test `c` (doluluk) | `(1, 4)` = 0.25 | mock.rs:378 | Primary slot doluluk orani |
| `slot_duration()` | `2 * MinimumPeriod` | lib.rs:572 | SlotDuration = 2 * timestamp minimum |

**slot_duration hesabi** (lib.rs:569-573):
```rust
pub fn slot_duration() -> T::Moment {
    <T as pallet_timestamp::Config>::MinimumPeriod::get().saturating_mul(2u32.into())
}
```
Tipik degerler: MinimumPeriod=6000ms -> SlotDuration=12000ms=12s (Polkadot'ta 6000ms -> 12s)

---

### Equivocation Korumasi

#### EquivocationOffence (equivocation.rs:60-97)

```rust
pub struct EquivocationOffence<Offender> {
    pub slot: Slot,                  // Hangi slotta cift blok uretti
    pub session_index: SessionIndex, // Hangi session'da
    pub validator_set_count: u32,    // O session'daki validator sayisi
    pub offender: Offender,          // Suclunun tam identifikasyonu
}

impl<Offender: Clone> Offence<Offender> for EquivocationOffence<Offender> {
    const ID: Kind = *b"babe:equivocatio";  // 16 byte offence ID

    fn slash_fraction(&self, offenders_count: u32) -> Perbill {
        // min((3k/n)^2, 1)
        // k = ayni slotta equivocate eden validator sayisi
        // n = toplam validator sayisi
        Perbill::from_rational(3 * offenders_count, self.validator_set_count).square()
    }
}
```

**Slashing formulu**: `slash = min((3k/n)^2, 1.0)` — tek validator equivocate etse bile minimum kesinti; bircok validator koordineli equivocate ederse maksimuma ulasilir.

Ornekler:
- k=1, n=100: `(3*1/100)^2 = 0.0009 = %0.09`
- k=10, n=100: `(3*10/100)^2 = 0.09 = %9`
- k=34, n=100: `(3*34/100)^2 ~= 1.04 -> 1.0 = %100`

#### EquivocationReportSystem (equivocation.rs:107-198)

```rust
pub struct EquivocationReportSystem<T, R, P, L>(PhantomData<(T, R, P, L)>);
// T: BABE Config
// R: ReportOffence implementation
// P: KeyOwnerProofSystem
// L: Longevity (transaction gecerliligi suresi)
```

**publish_evidence** (off-chain): Unsigned extrinsic olusturur ve transaction pool'a gonderir.

**check_evidence** (dogrulama):
```rust
fn check_evidence(evidence: (...)) -> Result<(), TransactionValidityError> {
    // 1. Key ownership kaniti kontrol: offender'in key'ine sahip mi?
    let key = (sp_consensus_babe::KEY_TYPE, equivocation_proof.offender.clone());
    let offender = P::check_proof(key, key_owner_proof.clone()).ok_or(InvalidTransaction::BadProof)?;

    // 2. Daha onceden raporlanmis mi?
    if R::is_known_offence(&[offender], &equivocation_proof.slot) {
        Err(InvalidTransaction::Stale.into())
    } else { Ok(()) }
}
```

**process_evidence** (on-chain isleme):
```rust
fn process_evidence(reporter: Option<T::AccountId>, evidence: (...)) -> Result<(), DispatchError> {
    // 1. Equivocation kaniti dogrula (iki farkli imzali blok, ayni slot)
    if !sp_consensus_babe::check_equivocation_proof(equivocation_proof) {
        return Err(Error::<T>::InvalidEquivocationProof.into());
    }

    // 2. Epoch-session eslesme dogrulama
    let epoch_index = *slot.saturating_sub(genesis_slot) / EpochDuration;
    if Pallet::<T>::session_index_for_epoch(epoch_index) != session_index {
        return Err(Error::<T>::InvalidKeyOwnershipProof.into());
    }

    // 3. Key ownership kaniti dogrula
    let offender = P::check_proof((KEY_TYPE, offender), key_owner_proof)
        .ok_or(Error::<T>::InvalidKeyOwnershipProof)?;

    // 4. Offence raporla
    let offence = EquivocationOffence { slot, validator_set_count, offender, session_index };
    R::report_offence(reporter.into_iter().collect(), offence)
        .map_err(|_| Error::<T>::DuplicateOffenceReport)?;
    Ok(())
}
```

#### ValidateUnsigned (equivocation.rs:201-249)

Unsigned equivocation raporlari icin gecerlilik kurallari:

```rust
ValidTransaction::with_tag_prefix("BabeEquivocation")
    .priority(TransactionPriority::max_value())  // En yuksek oncelik
    .and_provides((equivocation_proof.offender.clone(), *equivocation_proof.slot))  // Tekrar engeli
    .longevity(longevity)          // Kac blok gecerli?
    .propagate(false)              // Hicbir zaman propagate edilmez (sadece local)
    .build()
```

`propagate(false)`: Unsigned rapor sadece blok uretici tarafindan eklenir, network genelinde yayilmaz.

Kabul edilen kaynaklar: Sadece `Local` veya `InBlock` — network'ten gelen unsigned raporlar reddedilir.

#### Extrinsic'lere Fee Muafiyeti

```rust
// lib.rs:411-426
#[pallet::weight(...)]
pub fn report_equivocation(...) -> DispatchResultWithPostInfo {
    ...
    Ok(Pays::No.into())  // Fee yok!
}
```

Her iki equivocation extrinsic (`report_equivocation` ve `report_equivocation_unsigned`) basarili durumda fee odenmez. Bu mekanizma, raporu gönderme teşvikini artirır.

---

### BABE Storage Items (Tam Liste)

| Storage Item | Tip | Aciklama |
|---|---|---|
| `EpochIndex<T>` | `u64` | Mevcut epoch indeksi |
| `Authorities<T>` | `WeakBoundedVec<(AuthorityId, Weight), MaxAuthorities>` | Mevcut epoch authority'leri |
| `GenesisSlot<T>` | `Slot` | Birinci blogun slotu (0 iken henuz baslamadi) |
| `CurrentSlot<T>` | `Slot` | Mevcut slot numarasi |
| `Randomness<T>` | `BabeRandomness` | Mevcut epoch randomness (32 byte) |
| `PendingEpochConfigChange<T>` | `Option<NextConfigDescriptor>` | Bekleyen epoch konfigurasyonu |
| `NextRandomness<T>` | `BabeRandomness` | Sonraki epoch randomness (inşa asamasinda) |
| `NextAuthorities<T>` | `WeakBoundedVec<(AuthorityId, Weight), MaxAuthorities>` | Sonraki epoch authority'leri |
| `SegmentIndex<T>` | `u32` | UnderConstruction'daki mevcut segment indeksi |
| `UnderConstruction<T>` | `Map<u32, BoundedVec<BabeRandomness, 256>>` | Inşa halindeki randomness segmentleri |
| `Initialized<T>` | `Option<Option<PreDigest>>` | Blok baslatildi mi? (finalize'da temizlenir) |
| `AuthorVrfRandomness<T>` | `Option<BabeRandomness>` | Mevcut blok VRF randomness (finalize'da yazilir) |
| `EpochStart<T>` | `(BlockNumber, BlockNumber)` | (Onceki epoch baslangici, Mevcut epoch baslangici) |
| `Lateness<T>` | `BlockNumber` | Ebeveyn bloga gore kac slot gec (finalize'da temizlenir) |
| `EpochConfig<T>` | `Option<BabeEpochConfiguration>` | Mevcut epoch konfigurasyonu (c, allowed_slots) |
| `NextEpochConfig<T>` | `Option<BabeEpochConfiguration>` | Sonraki epoch konfigurasyonu |
| `SkippedEpochs<T>` | `BoundedVec<(u64, SessionIndex), 100>` | Atlanan epoch kayitlari |

#### EpochConfig Yapisi

```rust
// sp_consensus_babe
pub struct BabeEpochConfiguration {
    pub c: (u64, u64),              // (numerator, denominator) — doluluk oranı
    pub allowed_slots: AllowedSlots, // Primary/Secondary slot politikası
}
```

`c = (1, 4)` -> P(primary slot) = 1/4 = %25. Bu deger BABE SASSAFRAS analysis'ine gore optimal guvenligi saglar.

---

### Events ve Errors (BABE)

#### Errors (lib.rs:177-186)
```rust
pub enum Error<T> {
    InvalidEquivocationProof,    // Equivocation kaniti gecersiz
    InvalidKeyOwnershipProof,    // Key sahiplik kaniti gecersiz
    DuplicateOffenceReport,      // Bu equivocation daha once raporlandi
    InvalidConfiguration,        // plan_config_change icin gecersiz konfigurasyon
}
```

BABE pallet'inde Event yok — sadece storage degisiklikleri ve ConsensusLog.

#### ConsensusLog Tipleri

BABE blok header digest'lerine asagidaki consensus log'lari ekler:
- `ConsensusLog::NextEpochData(NextEpochDescriptor)`: Sonraki epoch authority ve randomness
- `ConsensusLog::NextConfigData(NextConfigDescriptor)`: Sonraki epoch konfigurasyon degisimi
- `ConsensusLog::OnDisabled(u32)`: Bir authority devre disi birakildi

---

### Istemci-Runtime Ayrimi

BABE'de kripto dogrulama tasarimsal olarak istemcide yapilir:

```rust
// lib.rs:374-379 (on_finalize icerisinde)
debug_assert!({
    use sp_core::crypto::VrfPublic;
    public.vrf_verify(&transcript.clone().into_sign_data(), &signature)
});
```

Bu sadece `debug_assert!` — production'da (`release` build) calistirilmaz. VRF imza dogrulamasi istemci tarafindan blok import sirasinda yapilir. Runtime sadece VRF ciktisini randomness birikiminde kullanir. Bu tasarim:
- Runtime'i hafif tutar
- Kritik kripto operasyonlari icin istemciye guvenilir
- Hasimane validator durumunda bile guvenligi korur (istemci block import'u reddeder)

---

### plan_config_change Extrinsic (lib.rs:461-476)

```rust
pub fn plan_config_change(origin: OriginFor<T>, config: NextConfigDescriptor) -> DispatchResult {
    ensure_root(origin)?;
    match config {
        NextConfigDescriptor::V1 { c, allowed_slots } => {
            ensure!(
                (c.0 != 0 || allowed_slots != AllowedSlots::PrimarySlots) && c.1 != 0,
                Error::<T>::InvalidConfiguration
            );
        },
    }
    PendingEpochConfigChange::<T>::put(config);
    Ok(())
}
```

Gecerlilik kurallari:
- `c.1 != 0` — denominator sifir olamaz
- `c.0 == 0` ise `allowed_slots != PrimarySlots` olmali — aksi halde hic primary slot olmaz

Konfigurasyon hemen etkin olmaz — pending'e alinir ve bir sonraki `enact_epoch_change`'de `NextEpochConfig`'e alinir, onu takip eden epoch'ta `EpochConfig`'e tasinir. Yani degisim 2 epoch sonra efektif olur.

---

### BABE Invariantlari (do_try_state)

```rust
pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
    // 1. Authorities <= MaxAuthorities
    ensure!(Authorities length <= T::MaxAuthorities);
    // 2. NextAuthorities <= MaxAuthorities
    ensure!(NextAuthorities length <= T::MaxAuthorities);
    // 3. EpochConfig her zaman Some
    ensure!(EpochConfig::<T>::get().is_some());
    // 4. CurrentSlot >= GenesisSlot (genesis'ten sonra)
    ensure!(*CurrentSlot >= genesis_slot);
    // 5. SkippedEpochs epoch_index'e gore artan sirayla sirali
    ensure!(skipped_epochs.windows(2).all(|w| w[0].0 < w[1].0));
    // 6. Her SkippedEpochs girisi: epoch_index >= session_index
    ensure!(all (epoch_index, session_index) => epoch_index >= session_index as u64);
    // 7. EpochStart: onceki <= mevcut
    ensure!(previous_epoch_start <= current_epoch_start);
}
```

---

### Kaynak Kod Referanslari

#### pallet-session
| Satir | Icerik |
|-------|--------|
| lib.rs:166-188 | `ShouldEndSession` trait + `PeriodicSessions` |
| lib.rs:249-280 | `SessionManager` trait |
| lib.rs:291-378 | `SessionHandler` trait + tuple implementasyonu |
| lib.rs:456-509 | Config trait + StorageVersion |
| lib.rs:594-650 | Storage items + Events + Errors |
| lib.rs:671-687 | `on_initialize` — session rotation tetikleme |
| lib.rs:776-867 | `rotate_session()` — tam session donusum mantigi |
| lib.rs:923-994 | `do_set_keys` + `inner_set_keys` |
| lib.rs:996-1023 | `do_purge_keys` |
| lib.rs:1055-1115 | `disable_index_with_severity`, `reenable_index`, `report_offence` |
| lib.rs:1199-1261 | `SessionInterface` implementasyonu (historical feature ile) |
| disabling.rs:21-28 | `DisablingStrategy` trait |
| disabling.rs:56-64 | `factor_based_disable_limit` formulu |
| disabling.rs:72-113 | `UpToLimitDisablingStrategy` |
| disabling.rs:125-199 | `UpToLimitWithReEnablingDisablingStrategy` |
| historical/mod.rs:93-110 | Historical storage items |
| historical/mod.rs:257-337 | `ProvingTrie` |
| historical/mod.rs:339-391 | `KeyOwnerProofSystem` implementasyonu |
| historical/onchain.rs:35-51 | `store_session_validator_set_to_offchain` |
| historical/offchain.rs:76-90 | `prove_session_membership` |
| migrations/v1.rs:63-103 | V0->V1 migration |

#### pallet-babe
| Satir | Icerik |
|-------|--------|
| lib.rs:81-108 | `EpochChangeTrigger` trait + iki implementasyon |
| lib.rs:110 | `UNDER_CONSTRUCTION_SEGMENT_LENGTH = 256` |
| lib.rs:124-174 | Config trait — tum tip bagimliliklari |
| lib.rs:177-186 | Errors |
| lib.rs:188-313 | Tum storage items |
| lib.rs:323-330 | GenesisConfig |
| lib.rs:332-401 | `on_initialize` + `on_finalize` — VRF islemi |
| lib.rs:404-476 | Extrinsics: report_equivocation, plan_config_change |
| lib.rs:515-524 | `ShouldEndSession` implementasyonu |
| lib.rs:569-573 | `slot_duration()` = 2 * MinimumPeriod |
| lib.rs:577-590 | `should_epoch_change()` — slot tabanli karar |
| lib.rs:623-723 | `enact_epoch_change()` — tam epoch degisim mantigi |
| lib.rs:783-800 | `deposit_randomness()` — segment mekanizmasi |
| lib.rs:877-892 | `randomness_change_epoch()` — randomness rotasyonu |
| lib.rs:1048-1092 | `OneSessionHandler` implementasyonu |
| lib.rs:1098-1113 | `compute_randomness()` — Blake2_256 formulu |
| randomness.rs:54 | `RandomnessFromTwoEpochsAgo` |
| randomness.rs:82 | `RandomnessFromOneEpochAgo` |
| randomness.rs:122 | `ParentBlockRandomness` |
| equivocation.rs:60-97 | `EquivocationOffence` + slash formulu: min((3k/n)^2, 1) |
| equivocation.rs:107-198 | `EquivocationReportSystem` |
| equivocation.rs:201-249 | `ValidateUnsigned` |
| default_weights.rs:31-53 | `report_equivocation` agirlik hesabi |
| mock.rs:188-192 | `EpochDuration=3`, `ReportLongevity=BondingDuration*SessionsPerEra*EpochDuration` |
| tests.rs:33-36 | `EMPTY_RANDOMNESS` — Blake2_256([0;32]||0u64_le) dogrulamasi |

---

## Nova Shield Notu — Ikisi Birlikte

### pallet-session Entegrasyon Plani

Nova Shield icin pallet-session'i aynen kullanmak, kendi session mantigi yazmaktan cok daha saglikli:

```rust
// Nova Shield runtime'inda Config ornegi
impl pallet_session::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type ValidatorId = AccountId;
    type ValidatorIdOf = ConvertInto;
    type ShouldEndSession = NovaConsensus;  // Kendi ShouldEndSession'imiz
    type NextSessionRotation = NovaConsensus;
    type SessionManager = NovaStaking;      // Gelecekteki staking palletimiz
    type SessionHandler = <SessionKeys as OpaqueKeys>::KeyTypeIdProviders;
    type Keys = SessionKeys;
    type DisablingStrategy = UpToLimitWithReEnablingDisablingStrategy<3>;
    type WeightInfo = weights::SessionWeights;
    type Currency = Balances;
    type KeyDeposit = ConstU128<SESSION_KEY_DEPOSIT>;
}
```

### BABE Yerine Nova Consensus

Nova Shield BABE'yi dogrudan kullanmak zorunda degil, ancak BABE'nin design pattern'lerini benimseyebilir:

1. **Slot-based epoch**: EpochDuration sabitini belirle. Polkadot'ta 2400 slot = 4 saat. Nova Shield icin: muhtemelen daha kisa, ornegin 600 slot = 1 saat (1 slot = 6s varsayimla).

2. **VRF-based leader selection**: `sp_consensus_babe` primitive'lerini yeniden kullanabiliriz ya da kendi VRF implementasyonumuzu arkworks uzerinden yazabiliriz.

3. **Randomness**: Nova Shield'in kendi randomness modulu `compute_randomness` mantikini kopyalayabilir. Blake2_256 uygundur.

4. **OneSessionHandler**: Nova Consensus pallet'imiz bu trait'i implement etmeli. BABE'nin `on_new_session` implementasyonu template olarak kullanilabilir.

### Kritik Tasarim Kararlari

1. **SlotDuration**: `2 * MinimumPeriod` formulu. Nova Shield icin: Hedef blok suresi 6s ise `MinimumPeriod = 3000ms`.

2. **Randomness Guvenlik Seviyesi**:
   - DeFi/high-value operations: `RandomnessFromTwoEpochsAgo` (en guvenli)
   - Genel on-chain operasyonlar: `RandomnessFromOneEpochAgo`
   - Block-level fresh randomness: `ParentBlockRandomness` (zayif, sadece dusuk degerde)

3. **Equivocation**: `EquivocationOffence` slash formulu `min((3k/n)^2, 1)` Nova Shield icin de iyi bir baslangic noktasidir. MPC/TSS konsensus'u icin adaptasyon gerekebilir.

4. **Disabled Validators Panigi**: `on_finalize`'daki `panic!` gerçek bir guvenlik onlemi. Devre disi birakilmis validator blok yazarsa chain durdurulur. Nova Shield'de benzer mekanizma olmali.

5. **SkippedEpochs boundary**: Maks 100 atlanan epoch. Uzun cevrimdisi sureler icin bu yeterli olabilir. Daha agresif epoch atlama senaryolari icin artirılabilir.

6. **authority weight**: BABE'de tum authority'ler `1` agirligindadir. Nova Shield'de stake-proportional weight eklemek icin `BabeAuthorityWeight` tipini ve `Authorities` storage'ini guncelleyebiliriz.

### MPC/TSS ile Entegrasyon Notu

Nova Shield'in MPC-tabanli konsensus'u icin BABE'den alinan dersler:
- Session key rotasyonu: Her yeni session'da TSS anahtar paylaslari yenilenebilir
- OneSessionHandler: TSS anahtar üretim/dagitim protokolu bu hook uzerinden tetiklenebilir
- Equivocation: TSS'de "ayni slot, farkli imza" daha zordur (esikli imza), ama double-signing mekanizmasi benzer sekilde tasarlanabilir
- Historical session: Slashing icin `KeyOwnerProofSystem` TSS imzacilara da uygulanabilir

### Performans Notu

`rotate_session()` cok pahali: Her session donusumunde `QueuedKeys` tam iterasyonu, `SessionHandler::on_new_session` chain cagirisi vs. Bu agirlik `T::BlockWeights::get().max_block` olarak ayarlanmistir — yani session donusum blogu neredeyse tam kapasiteye ulasilmis sayilir. Bu yuzden session donusum bloklari normal bloklardan daha az kullanici extrinsici alacaktir.

`ProvingTrie::generate_for` da pahali: Tum validator setini trie'ye yazar. Cok buyuk validator setlerinde bu bellek yogun olabilir. Production'da `HistoricalSessions` Merkle root'unu off-chain validator set yerine kullanmak (mevcut tasarim) bu sorunu azaltir.

---

## Test Analizleri ve Dogrulanmis Davranislar

### pallet-session Test Davranislari (tests.rs)

Testler, pallet-session'in kritik yaşam dongusu davranislarini net sekilde ortaya koymaktadir:

**Test: `authorities_should_track_validators`** — N+2 Gecikmesinin Somut Kanitı

```
Block 1 (Session 0 biter, Session 1 baslar):
  - set_next_validators([1, 2]) cagirildi
  - force_new_session() ile Session 0 bitti
  - QueuedKeys = [(1, key1), (2, key2)]  -- session 2 icin hazir
  - Validators = [1, 2, 3]               -- session 1'in validatorlari
  - authorities = [key1, key2, key3]     -- session 1'in anahtarlari

Block 2 (Session 1 biter, Session 2 baslar):
  - QueuedKeys = [(1, key1), (2, key2)]  -- hala ayni
  - Validators = [1, 2]                  -- artık [1,2]
  - authorities = [key1, key2]           -- artık [key1, key2]

Block 3 (set_next_validators([1,2,4]) yapildi):
  - Validator 4 set_keys cagirdi
  - QueuedKeys = [(1,k1),(2,k2),(4,k4)]  -- session 4 icin hazir
  - Validators = [1, 2]                  -- session 3, hala [1,2]

Block 4 (Session 3 biter, Session 4 baslar):
  - Validators = [1, 2, 4]              -- artık 4 de eklendi
  - authorities = [key1, key2, key4]
```

Bu, N+2 gecikmesinin tam olarak calistığini doğrular: Block 2'de eklenen validator ancak Block 4'te aktif olur.

**Test: `should_work_with_early_exit`** — Manuel Session Sonlandirma

```
Blok 1-2: CurrentIndex=0 (session devam ediyor)
Blok 3: force_new_session() -> CurrentIndex=1
Blok 4-9: CurrentIndex=1 (session_length=10, henüz bitmedi)
Blok 10: Otomatik session bitisi -> CurrentIndex=2
```

`force_new_session()` mekanizmasi: `QueuedChanged` flag'ini `true` yapar, bir sonraki `should_end_session` sorgusu `true` doner.

### pallet-babe Test Davranislari

**Test: `first_block_epoch_zero_start`** — Ilk Epoch Baslangici

```
genesis_slot = 100
Block 1'de:
  - should_end_session(1) = false (blok #1'de epoch donmez)
  - GenesisSlot = 100 (onceden 0'di, initialize'de set edildi)
  - CurrentSlot = 100
  - EpochIndex = 0
  - on_finalize sonrasi:
    - AuthorVrfRandomness = Some(vrf_randomness)
    - UnderConstruction[0] = [vrf_randomness]  -- ilk VRF eklendi
    - Randomness = [0;32]                       -- henuz degismedi
    - NextRandomness = [0;32]                   -- henuz degismedi
  - Header digest'inde 2 log:
    - [0]: PreRuntime (BABE pre-digest)
    - [1]: Consensus (NextEpochData — ilk epoch icin authorities ve randomness)
```

**Test: `current_slot_is_processed_on_initialization`** — Slot Zamanlama

```
on_initialize cagirilmadan once:
  - CurrentSlot = 0
  - Initialized = None

Babe::initialize(1) sonrasi:
  - CurrentSlot = genesis_slot (10)
  - Initialized = Some(Some(pre_digest))
  - AuthorVrfRandomness = None (henuz finalize olmadi)

on_finalize sonrasi:
  - AuthorVrfRandomness = Some(vrf_randomness)
```

**Test: `empty_randomness_is_correct`** — Randomness Bos Durum Dogrulamasi

```rust
let s = compute_randomness([0; 32], 0, std::iter::empty(), None);
// s = Blake2_256([0u8;32] || 0u64_le_bytes)
// = Blake2_256([0,0,...,0 (32 byte), 0,0,0,0,0,0,0,0 (8 byte)])
assert_eq!(s, [74, 25, 49, 128, 53, 97, 244, 49, 222, 202, 176, 2, 231, 66, 95, 10,
               133, 49, 213, 228, 86, 161, 164, 127, 217, 153, 138, 37, 48, 192, 248, 0]);
```

Bu, `compute_randomness` fonksiyonunun determinizmini dogrular: ayni girdi her zaman ayni Blake2-256 ciktiyi uretir.

---

## BABE Benchmarking Analizi

### check_equivocation_proof Benchmark (benchmarking.rs)

```rust
#[benchmark]
fn check_equivocation_proof(x: Linear<0, 1>) {
    const EQUIVOCATION_PROOF_BLOB: [u8; 416] = [...];
    let equivocation_proof: sp_consensus_babe::EquivocationProof<Header> =
        Decode::decode(&mut &EQUIVOCATION_PROOF_BLOB[..]).unwrap();
    #[block]
    {
        sp_consensus_babe::check_equivocation_proof::<Header>(equivocation_proof1);
    }
    assert!(sp_consensus_babe::check_equivocation_proof::<Header>(equivocation_proof2));
}
```

Benchmark notlari:
- Equivocation kanit blob'u 416 byte (SCALE encoded EquivocationProof<Header>)
- `x: Linear<0, 1>` — validator sayisina gore lineer; burada maksimum 1 ile benchmark alinmis
- Blob deterministik degil (random key generation), ancak boyut sabittir
- `#[block]` blogu icinde yapilan tek operasyon `check_equivocation_proof` cagridir — bu islemin saf agirligini verir

### default_weights.rs Analizi

Manuel yazilmis (otomatik olusturulmamis) agirlik hesaplari:

```rust
// plan_config_change: Sadece 1 write
fn plan_config_change() -> Weight {
    DbWeight::get().writes(1)
}

// report_equivocation: Karmasik cok adimli hesap
fn report_equivocation(validator_count: u32, max_nominators_per_validator: u32) -> Weight {
    let validator_count = validator_count.max(100) as u64; // Minimum 100 validator varsay

    // Adim 1: Membership kaniti dogrulama
    Weight::from_parts(35_000 * WEIGHT_REF_TIME_PER_NANOS, 0)  // 35 mikrosaniye sabit
    + Weight::from_parts(175 * WEIGHT_REF_TIME_PER_NANOS, 0) * validator_count  // 175ns/validator

    // Adim 2: 5 DB okumasi (membership)
    + DbWeight::get().reads(5)

    // Adim 3: Equivocation kaniti dogrulama
    + Weight::from_parts(110_000 * WEIGHT_REF_TIME_PER_NANOS, 0)  // 110 mikrosaniye

    // Adim 4: Offence raporlama
    + Weight::from_parts(110_000 * WEIGHT_REF_TIME_PER_NANOS, 0)  // 110 mikrosaniye
    + Weight::from_parts(25_000 * WEIGHT_REF_TIME_PER_NANOS, 0) * max_nominators_per_validator as u64

    // Adim 5: DB okumalari/yazmalari (nominator sayisina bagli)
    + DbWeight::get().reads(14 + 3 * max_nominators_per_validator as u64)
    + DbWeight::get().writes(10 + 3 * max_nominators_per_validator as u64)
}
```

Agirlik analizi:
- Dogrusal bilesenleri: `validator_count` ve `max_nominators_per_validator`
- DB dominansi: Yuksek nominator sayisinda DB okuma/yazma dominandir
- Floor: `validator_count.max(100)` — validator seti kucuk olsa bile en az 100 validator agirligiyla hesaplanir (underestimation onlemi)

---

## EstimateNextSessionRotation Uygulamasi

### PeriodicSessions (session/lib.rs:190-246)

```rust
impl<...> EstimateNextSessionRotation<BlockNumber> for PeriodicSessions<Period, Offset> {
    fn average_session_length() -> BlockNumber {
        Period::get()
    }

    fn estimate_current_session_progress(now: BlockNumber) -> (Option<Permill>, Weight) {
        let offset = Offset::get();
        let period = Period::get();

        let progress = if now >= offset {
            // +1 cunku mevcut blok da gecmis sayilir; son blokta ilerleme %100 olmali
            let current = (now - offset) % period.clone() + One::one();
            Some(Permill::from_rational(current, period))
        } else {
            // Offset'e henuz ulasilmadi
            Some(Permill::from_rational(now + One::one(), offset))
        };

        (progress, Zero::zero())  // Agirlik: sifir (storage okumasi yok)
    }

    fn estimate_next_session_rotation(now: BlockNumber) -> (Option<BlockNumber>, Weight) {
        let next_session = if now > offset {
            let block_after_last_session = (now.clone() - offset) % period.clone();
            if block_after_last_session > Zero::zero() {
                now + period - block_after_last_session  // Kalan bloklari ekle
            } else {
                now + period  // Zaten session sinirindayiz, bir period ekle
            }
        } else {
            offset  // Henuz offset'e ulasilmadi
        };

        (Some(next_session), Zero::zero())
    }
}
```

### BABE (babe/lib.rs:1010-1036)

```rust
impl<T: Config> EstimateNextSessionRotation<BlockNumberFor<T>> for Pallet<T> {
    fn average_session_length() -> BlockNumberFor<T> {
        T::EpochDuration::get().saturated_into()
    }

    fn estimate_current_session_progress(_now: BlockNumberFor<T>) -> (Option<Permill>, Weight) {
        let elapsed = CurrentSlot::<T>::get().saturating_sub(Self::current_epoch_start()) + 1;
        (
            Some(Permill::from_rational(*elapsed, T::EpochDuration::get())),
            T::DbWeight::get().reads(3),  // CurrentSlot, EpochIndex, GenesisSlot
        )
    }

    fn estimate_next_session_rotation(now: BlockNumberFor<T>) -> (Option<BlockNumberFor<T>>, Weight) {
        (
            Self::next_expected_epoch_change(now),
            T::DbWeight::get().reads(3),
        )
    }
}
```

BABE'nin `next_expected_epoch_change` (lib.rs:607-614):
```rust
pub fn next_expected_epoch_change(now: BlockNumberFor<T>) -> Option<BlockNumberFor<T>> {
    let next_slot = Self::current_epoch_start().saturating_add(T::EpochDuration::get());
    next_slot.checked_sub(*CurrentSlot::<T>::get()).map(|slots_remaining| {
        // Yaklasik: slot/blok oranındaki sapmalar hataya yol acar
        let blocks_remaining: BlockNumberFor<T> = slots_remaining.saturated_into();
        now.saturating_add(blocks_remaining)
    })
}
```

Onceki epoch'tan tahmini: `next_slot - current_slot = kalan_slot`. Bu tahmin sadece hicbir slot bos olmadigi varsayimiyla dogrudur — ust sinir olarak yorumlanmalidir.

---

## FindAuthor Implementasyonlari

### BABE FindAuthor (babe/lib.rs:493-507)

```rust
impl<T: Config> FindAuthor<u32> for Pallet<T> {
    fn find_author<'a, I>(digests: I) -> Option<u32>
    where I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>
    {
        for (id, mut data) in digests.into_iter() {
            if id == BABE_ENGINE_ID {
                let pre_digest: PreDigest = PreDigest::decode(&mut data).ok()?;
                return Some(pre_digest.authority_index());
            }
        }
        None
    }
}
```

`BABE_ENGINE_ID = *b"BABE"` — 4 byte consensus engine ID.

### Session FindAccountFromAuthorIndex (session/lib.rs:1266-1280)

```rust
pub struct FindAccountFromAuthorIndex<T, Inner>(PhantomData<(T, Inner)>);

impl<T: Config, Inner: FindAuthor<u32>> FindAuthor<T::ValidatorId>
    for FindAccountFromAuthorIndex<T, Inner>
{
    fn find_author<'a, I>(digests: I) -> Option<T::ValidatorId>
    where I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>
    {
        let i = Inner::find_author(digests)?;        // BABE'den u32 indeks al
        let validators = Validators::<T>::get();     // Mevcut validator listesi
        validators.get(i as usize).cloned()          // Indeksi ValidatorId'ye cevir
    }
}
```

Zincir: `block digest -> BABE authority_index -> Session ValidatorId -> pallet_authorship author`

---

## Guvenlik Modeli ve Saldirganlık Analizi

### BABE VRF Guvenligi

**Saldiri 1: VRF manipulasyonu** — Imkansiz. VRF ozelligi: Verifiable Uniqueness. Bir anahtar + transcript icin tek bir gecerli cikti vardir. Sahteciliğe karsi cryptographic guvenlik saglar.

**Saldiri 2: Slot atlatma (nothing-at-stake)** — Kismi onleme. Bir validator kendi VRF ciktisini hesaplar; kazandiginda yayin yapar, kaybettiginde kacirabilir. BABE bu nedenle secondary slot'lara sahip. Ekonomik tesvikler (staking reward) duzgun davranisin pekistirilmesini saglar.

**Saldiri 3: Randomness manipulasyonu** — Sinirli. Son epoch'taki validatorlar, son blocklarda kendi bloklarini yayin yapmayarak randomness'i etkileyebilir (grinding attack). Bu nedenle:
- `RandomnessFromTwoEpochsAgo`: Saldirgan epoch-2'deki VRF'leri maniple etmeli (2 epoch onceden)
- `SASSAFRAS` protokolu bu sorunu tamamen cozer (future upgrade)

**Saldiri 4: Equivocation** — Tespit edilebilir. Ayni slotta iki farkli blok yayini yaparsa offender imzali kanit on-chain raporlanir ve slash uygulanir.

**Saldiri 5: Disabled validator blok uretimi** — Aninda durdurulur. `on_finalize`'daki `panic!` zinciri durdurur.

### pallet-session Guvenligi

**Saldiri 1: Key spam** — `KeyDeposit` ile onlenir. Her key kaydi deposit gerektirir.

**Saldiri 2: Key calintisi** — `ownership_proof_is_valid` sahiplik kaniti gerektirir. Ozel anahtar olmadan key kaydi yapılamaz.

**Saldiri 3: Validator set manipulasyonu** — `SessionManager` (staking pallet) validator seçimini kontrol eder. pallet-session sadece keyleri yonetir; kim validator olacagini staking pallet belirler.

**Saldiri 4: Replay saldirilari** — `DuplicatedKey` kontrolu ayni key'in baska bir validatora atanmasini onler.

---

## Interoperability: Session-BABE Entegrasyonu

Tam entegrasyon akisi:

```
[Staking Pallet]
    |
    | new_session(N) -> Some([val1, val2, ...])
    v
[pallet-session: SessionManager]
    |
    | rotate_session()
    |   -> on_before_session_ending()
    |   -> end_session(N-1)
    |   -> start_session(N)
    |   -> on_new_session(changed, current_keys, queued_keys)
    v
[BABE: OneSessionHandler::on_new_session]
    |
    | enact_epoch_change(authorities, next_authorities, session_index)
    |   -> EpochIndex update
    |   -> Randomness rotation: NextRandomness -> Randomness
    |   -> NextRandomness = compute_randomness(...)
    |   -> NextEpochData consensus log -> block header
    v
[Consensus clients]
    |
    | NextEpochData log parse et
    | Yeni epoch icin VRF threshold hesapla
    | Slotlari iddia et (c parametresine gore)
```

Bu zincirde kritik siralama: pallet-session `on_initialize`'dan `should_end_session` sorgular -> BABE'nin kendi `initialize()` fonksiyonunu cagirir -> BABE slot guncellenir -> `should_epoch_change` duzgün calisir.

---

## Session Key Guvenligi Modeli: Harici vs Yerel

Iki farkli key kaydi yolu:

### Yerel Yol (kullanici tarafindan): `set_keys` extrinsic

```
Kullanici -> set_keys(keys, proof)
  -> ownership proof dogrula
  -> KeyDeposit hold et
  -> inc_consumers
  -> NextKeys'e yaz
  -> KeyOwner'a yaz
```

### Harici Yol (XCM/privileged): `SessionInterface::set_keys`

```
Harici Arama -> SessionInterface::set_keys(account, keys)
  -> Sahiplik kaniti YOK (caller sorumluluktur)
  -> Deposit hold yok
  -> Consumer increment yok
  -> ExternallySetKeys'e kaydet
  -> NextKeys'e yaz
  -> KeyOwner'a yaz
```

### Gecis Senaryolari

| Onceki Durum | Yeni Arama | Sonuc |
|---|---|---|
| Yok | local `set_keys` | Deposit + consumer, ExternallySetKeys yok |
| Yerel | local `set_keys` (key rotasyonu) | Deposit eklenmez, consumer eklenmez |
| Harici | local `set_keys` | Deposit + consumer, ExternallySetKeys silindi |
| Yerel | external `set_keys` | Deposit serbest, consumer azaltildi, ExternallySetKeys eklendi |
| Harici | external `set_keys` | Hicbir degisiklik |

Bu matris `do_set_keys` (lib.rs:923-954) kodunda tam olarak uygulanmistir.

---

## Ozet: Kritik Sayisal Degerler (Kaynak Koddan Dogrulanmis)

### pallet-session

| Deger | Sabit/Tipler | Kaynak |
|-------|-------------|--------|
| Maksimum OneSessionHandler tuple boyutu | 30 | lib.rs:330 (`impl_for_tuples(1, 30)`) |
| Default disabling limit factor | 3 (1/3 Byzantine esigi) | disabling.rs:72 |
| `DisabledValidators` sirali olmali | binary_search ile O(log n) | lib.rs:1061, 1105 |
| V0 migration: eski disabled = max severity | OffenceSeverity::max_severity() | migrations/v1.rs:52 |
| Historical storage key prefix | `b"session_historical"` | historical/shared.rs:25 |
| Historical prune tracker key | `b"session_historical_last_prune"` | historical/shared.rs:26 |
| Storage version | 1 | lib.rs:448 |

### pallet-babe

| Deger | Sabit | Kaynak |
|-------|-------|--------|
| Segment uzunlugu | 256 VRF ciktisi | lib.rs:110 |
| Randomness uzunlugu | 32 byte | RANDOMNESS_LENGTH (sp_consensus_babe) |
| Engine ID | `*b"BABE"` | BABE_ENGINE_ID (sp_consensus_babe) |
| VRF context | `b"substrate-babe-vrf"` | RANDOMNESS_VRF_CONTEXT |
| Offence ID | `*b"babe:equivocatio"` | equivocation.rs:72 |
| Skipped epochs bound | 100 | lib.rs:312 |
| Test epoch duration | 3 slot | mock.rs:189 |
| Test c param | (1, 4) = %25 | mock.rs:378 |
| Test sessions per era | 3 | mock.rs:147 |
| Test bonding duration | 3 era | mock.rs:148 |
| Slash formula | min((3k/n)^2, 1) | equivocation.rs:95 |
| plan_config change delay | 2 epoch | (pending->next->active) |
| Slot duration | 2 * MinimumPeriod | lib.rs:572 |
| Default weights: plan_config | 1 DB write | default_weights.rs:28 |
| Default weights: equivoc base | 35μs + 175ns/validator | default_weights.rs:37-40 |
| Default weights: equivoc check | 110μs | default_weights.rs:44 |
| Default weights: offence report | 110μs + 25μs/nominator | default_weights.rs:46-48 |
| BABE storage version | (implicit, genesis init) | lib.rs:324-330 |
