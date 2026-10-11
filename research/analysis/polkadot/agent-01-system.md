# Substrate pallet-system — Agent 01

## Kapsam

**Analiz Edilen Repo:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/polkadot-sdk/substrate/frame/system/`

**Dosyalar ve Satır Sayıları:**

| Dosya | Satır |
|-------|-------|
| src/lib.rs | 2706 |
| src/limits.rs | 519 |
| src/tests.rs | 1122 |
| src/weights.rs | 311 |
| src/offchain.rs | 843 |
| src/mock.rs | 142 |
| src/mocking.rs | 47 |
| src/migrations/mod.rs | 121 |
| src/extensions/authorize_call.rs | 343 |
| src/extensions/check_genesis.rs | 70 |
| src/extensions/check_mortality.rs | 166 |
| src/extensions/check_nonce.rs | 445 |
| src/extensions/check_non_zero_sender.rs | 126 |
| src/extensions/check_spec_version.rs | 69 |
| src/extensions/check_tx_version.rs | 69 |
| src/extensions/check_weight.rs | 1150 |
| src/extensions/mod.rs | 29 |
| src/extensions/weight_reclaim.rs | 401 |
| src/extensions/weights.rs | 232 |
| benchmarking/src/extensions.rs | 302 |
| benchmarking/src/inner.rs | 233 |
| benchmarking/src/lib.rs | 32 |
| benchmarking/src/mock.rs | 107 |
| rpc/runtime-api/src/lib.rs | 36 |
| benches/bench.rs | 99 |
| **TOPLAM** | **~9720 satır** |

---

## Config Trait

`pallet-system`'in kalbi, `Config` trait'idir. Bu trait, runtime tarafindan implement edilir ve her associated type bir konfigürasyon noktasidir.

### Tam Config Trait Analizi (lib.rs:491-685)

```rust
#[pallet::config(with_default, frame_system_config)]
#[pallet::disable_frame_system_supertrait_check]
pub trait Config: 'static + Eq + Clone {
```

**Not:** `disable_frame_system_supertrait_check` — System pallet kendisi `Config`'e dayandiginda circular dependency olacagi icin bu kisitlama kaldiriliyor. Önemli tasarim karari.

#### Associated Types (Tam Liste):

**1. `RuntimeEvent`**
- Tip: `Parameter + Member + From<Event<Self>> + Debug + IsType<...>`
- Amac: Runtime'daki tüm event'lerin aggregate tipi
- `#[inject_runtime_type]` ile `construct_runtime!` tarafindan otomatik inject edilir
- `IsType` constraint: System event'lerin RuntimeEvent'e convert edilebilmesi

**2. `BaseCallFilter`** (lib.rs:511)
- Tip: `Contains<Self::RuntimeCall>`
- Amac: Root haricinde tüm origin'lerin kullandigi temel cagri filtresi
- Default: `frame_support::traits::Everything` (her seye izin ver)
- Root origin bu filtreyi atlar (`dispatch_bypass_filter`)
- Composable: `InsideBoth`, `TheseExcept`, `EverythingBut` ile birlestirilir

**3. `BlockWeights`** (lib.rs:515)
- Tip: `Get<limits::BlockWeights>`
- `#[pallet::constant]` — on-chain constant olarak saklanir
- Amac: Blok agirlik limitlari ve per-class konfigürasyonu
- `limits::BlockWeights` struct'i kapsayan wrapper

**4. `BlockLength`** (lib.rs:519)
- Tip: `Get<limits::BlockLength>`
- `#[pallet::constant]`
- Amac: Blokun bayt cinsinden maksimum uzunlugu (per dispatch class)

**5. `RuntimeOrigin`** (lib.rs:523-527)
- Tip: `Into<Result<RawOrigin<Self::AccountId>, Self::RuntimeOrigin>> + From<RawOrigin<Self::AccountId>> + Clone + OriginTrait<...> + AsTransactionAuthorizedOrigin`
- `RawOrigin`'a ve `RawOrigin`'dan dönüstürülebilir olmali
- `AsTransactionAuthorizedOrigin`: Authorize edilmis tx'leri is/degil belirler

**6. `RuntimeCall`** (lib.rs:532-537)
- Tip: `Parameter + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin> + Debug + GetDispatchInfo + From<Call<Self>> + Authorize`
- Aggregate call tipi — tüm pallet call'larinin birlesmis enum'u
- `Authorize` trait: `AuthorizeCall` extension icin gerekli
- `#[docify::export(system_runtime_call)]` ile dökümante edilmis

**7. `RuntimeTask`** (lib.rs:540-541)
- Tip: `Task`
- `#[cfg(feature = "experimental")]` altinda kullanilir
- Asenkron görevler icin (multi-block migrations vb.)

**8. `Nonce`** (lib.rs:543-553)
- Tip: `Parameter + HasCompact + Member + MaybeSerializeDeserialize + AtLeast32Bit + Copy + MaxEncodedLen`
- Account transaction counter — replay protection icin
- `HasCompact`: Compact encoding destegi
- Tipik deger: `u32` (test) veya `u32` (production)

**9. `Hash`** (lib.rs:555-569)
- Tip: `Parameter + Member + SimpleBitOps + Ord + Default + Copy + CheckEqual + AsRef<[u8]> + AsMut<[u8]> + MaxEncodedLen`
- Hashing fonksiyonunun ciktisi
- Tipik deger: `sp_core::hash::H256` (32 byte)

**10. `Hashing`** (lib.rs:571-572)
- Tip: `Hash<Output = Self::Hash> + TypeInfo`
- Kullanilan hashing algoritmasi
- Tipik deger: `sp_runtime::traits::BlakeTwo256`

**11. `AccountId`** (lib.rs:574-581)
- Tip: `Parameter + Member + MaybeSerializeDeserialize + Debug + MaybeDisplay + Ord + MaxEncodedLen`
- Kullanici hesap tanimlayici tipi
- Test'te: `u64`
- Production'da: `sp_runtime::AccountId32` (32 byte pubkey hash)

**12. `Lookup`** (lib.rs:583-589)
- Tip: `StaticLookup<Target = Self::AccountId>`
- Dispatch'te AccountId'yi kaynak tipten çevirir
- Test'te: `IdentityLookup<Self::AccountId>` (dogrudan)
- Production'da: `AccountIdLookup<Self::AccountId, ()>` (indeksli)

**13. `Block`** (lib.rs:593-594)
- Tip: `Parameter + Member + traits::Block<Hash = Self::Hash>`
- `#[pallet::no_default]` — runtime tarafindan mutlaka saglanmali
- Blok tipi: hem extrinsic'lere hem hash'e bagli

**14. `BlockHashCount`** (lib.rs:597-599)
- Tip: `Get<BlockNumberFor<Self>>`
- `#[pallet::constant]`
- Saklanan eski blok hash'i sayisi (en eski önce silinir)
- Default: 256 (solochain), 10 (test)

**15. `DbWeight`** (lib.rs:602-603)
- Tip: `Get<RuntimeDbWeight>`
- `#[pallet::constant]`
- DB okuma/yazma agirliklari
- `RuntimeDbWeight { read: u64, write: u64 }`

**16. `Version`** (lib.rs:606-607)
- Tip: `Get<RuntimeVersion>`
- `#[pallet::constant]`
- Runtime versiyonu: `spec_version`, `spec_name`, `transaction_version` içerir

**17. `PalletInfo`** (lib.rs:615-616)
- Tip: `PalletInfo`
- `construct_runtime!` tarafindan üretilir
- Pallet ismi, indeksi, modül adi vb. sorgulanabilir

**18. `AccountData`** (lib.rs:620)
- Tip: `Member + FullCodec + Clone + Default + TypeInfo + MaxEncodedLen`
- Account'a ek veri (nonce/sayac haricinde)
- Tipik: `pallet_balances::AccountData<Balance>` — bakiye bilgisi burada saklanir
- Test'te: `()` (bos)

**19. `OnNewAccount`** (lib.rs:623)
- Tip: `OnNewAccount<Self::AccountId>`
- Yeni hesap olusturuldugunda cagrilir
- Default: `()` (hicbir sey yapma)

**20. `OnKilledAccount`** (lib.rs:627-628)
- Tip: `OnKilledAccount<Self::AccountId>`
- Hesap yokedildiginde cagrilir, ilgili kaynaklar temizlenir
- Default: `()` (hicbir sey yapma)

**21. `SystemWeightInfo`** (lib.rs:631)
- Tip: `WeightInfo` (from src/weights.rs)
- Extrinsic agirlik fonksiyonlari
- Benchmark'dan üretilen agirliklar

**22. `ExtensionsWeightInfo`** (lib.rs:634)
- Tip: `extensions::WeightInfo`
- Transaction extension'larin agirlik fonksiyonlari

**23. `SS58Prefix`** (lib.rs:641-642)
- Tip: `Get<u16>`
- `#[pallet::constant]`
- Chain'in SS58 adresi prefiksi (Polkadot=0, Kusama=2, Substrate=42)
- Chain spec'teki `ss58Format`'in yerini alir

**24. `OnSetCode`** (lib.rs:651-652)
- Tip: `SetCode<Self>`
- Runtime kodu degistiginde ne yapilacagi
- Default `()`: direk storage'a yazar (`update_code_in_storage`)
- Parachain'de: Cumulus defer eder (validator onaylamadan once uygulama yok)

**25. `MaxConsumers`** (lib.rs:655)
- Tip: `ConsumerLimits`
- Tek account'taki maksimum consumer ref sayisi
- Default: 128 (solochain), 16 (test)
- `ConsumerLimits` trait: `max_consumers()` ve `max_overflow()` metodlari

**26. `SingleBlockMigrations`** (lib.rs:663)
- Tip: `OnRuntimeUpgrade`
- Runtime upgrade'de tek blokta calistirilan migrasyonlar
- Eskiden Executive'de konfig edilirdi, simdi System'de

**27. `MultiBlockMigrator`** (lib.rs:669)
- Tip: `MultiStepMigrator`
- Cok bloklu migrasyonlar icin (pallet_migrations)
- `ongoing()` methodu varsa runtime upgrade engellenir

**28. `PreInherents`** (lib.rs:674)
- Tip: `PreInherents`
- Her blokta inherent'lardan once calisir

**29. `PostInherents`** (lib.rs:679)
- Tip: `PostInherents`
- Her blokta tüm inherent'lar uygulandiktan sonra calisir

**30. `PostTransactions`** (lib.rs:684)
- Tip: `PostTransactions`
- Her blokta tüm tx'ler uygulandiktan sonra calisir

---

## Storage Items

### 1. `Account<T>` (lib.rs:976-983)
```rust
pub type Account<T: Config> = StorageMap<
    _,
    Blake2_128Concat,
    T::AccountId,
    AccountInfo<T::Nonce, T::AccountData>,
    ValueQuery,
>;
```
- **Tip:** `StorageMap<AccountId -> AccountInfo>`
- **Hasher:** `Blake2_128Concat` — AccountId'nin hashlenip prefix olarak kullanilmasi
- **ValueQuery:** Olmayan hesap icin `Default` döner
- **Getter:** `fn account()` ile erisim
- **AccountInfo yapisi:**
  ```rust
  pub struct AccountInfo<Nonce, AccountData> {
      pub nonce: Nonce,       // Transaction sayaci
      pub consumers: RefCount, // Bu hesaba bagli consumer sayisi
      pub providers: RefCount, // Bu hesabin var olmasini saglayan provider sayisi
      pub sufficients: RefCount, // Self-sufficient ref count
      pub data: AccountData,   // Pallet-specific ek veri (bakiye vb.)
  }
  ```
- Bu map tüm hesap verilerini tutar — merkezi hesap registrisi

### 2. `ExtrinsicCount<T>` (lib.rs:987-988)
```rust
pub(super) type ExtrinsicCount<T: Config> = StorageValue<_, u32>;
```
- **Tip:** `Option<u32>` (OptionQuery)
- **Amac:** Mevcut blokta islenen extrinsic sayisi
- **Yazma:** `note_finished_extrinsics()` ile set edilir (extrinsic index'ten alinir)
- **Okuma:** `finalize()` sirasinda extrinsic root hesabi icin
- `#[pallet::whitelist_storage]` — PoV hesabinda cift saymaz

### 3. `InherentsApplied<T>` (lib.rs:992-993)
```rust
pub type InherentsApplied<T: Config> = StorageValue<_, bool, ValueQuery>;
```
- **Tip:** `bool`, varsayilan `false`
- **Amac:** Tüm inherent'larin uygulanip uygulanmadigini takip eder
- **Yazma:** `note_inherents_applied()` ile `true` yapilir
- **Temizleme:** `finalize()` sirasinda kill edilir

### 4. `BlockWeight<T>` (lib.rs:996-999)
```rust
pub type BlockWeight<T: Config> = StorageValue<_, ConsumedWeight, ValueQuery>;
```
- **Tip:** `ConsumedWeight = PerDispatchClass<Weight>`
- Her dispatch class icin ayri agirlik tutulur: Normal, Operational, Mandatory
- **Getter:** `fn block_weight()`
- **Yazma:** `register_extra_weight_unchecked()`, `CheckWeight::do_prepare()`
- **Temizleme:** `initialize()` sirasinda kill edilir (yeni blok basinda sifirlanir)
- `#[pallet::whitelist_storage]` — hot storage, PoV'da cift sayilmaz

### 5. `BlockSize<T>` (lib.rs:1003-1006)
```rust
pub type BlockSize<T: Config> = StorageValue<_, u32>;
```
- **Tip:** `Option<u32>` byte cinsinden
- **Amac:** Mevcut blok toplam boyutu (header + tüm extrinsic'ler)
- **Baslangiç:** `initialize()` sirasinda overhead (digest + empty header) ile set edilir
- **Güncelleme:** Her extrinsic'ten sonra `CheckWeight::do_prepare()` ile arttirilir
- **Digest yaziminda:** `deposit_log()` de boyutu günceller

### 6. `BlockHash<T>` (lib.rs:1009-1012)
```rust
pub type BlockHash<T: Config> =
    StorageMap<_, Twox64Concat, BlockNumberFor<T>, T::Hash, ValueQuery>;
```
- **Tip:** `StorageMap<BlockNumber -> Hash>`
- **Hasher:** `Twox64Concat` — blok numaralari diger paydas saldirilardan etkilenmez (sayisal)
- **Getter:** `fn block_hash()`
- **Yazma:** `initialize()` sirasinda önceki blogun hash'i eklenir
- **Silme:** `finalize()` sirasinda eski hashler temizlenir (`BlockHashCount`'a göre)
- **Kullanim:** `CheckMortality` ve `CheckGenesis` extension'lari okur

### 7. `ExtrinsicData<T>` (lib.rs:1015-1019)
```rust
pub(super) type ExtrinsicData<T: Config> =
    StorageMap<_, Twox64Concat, u32, Vec<u8>, ValueQuery>;
```
- **Tip:** `StorageMap<u32 -> Vec<u8>>` — extrinsic indeksi -> encoded extrinsic
- `#[pallet::unbounded]` — boyut limiti yok (extrinsic'ler farkli uzunluklarda)
- **Yazma:** `note_extrinsic()` ile dispatch oncesinde set edilir
- **Okuma/Temizleme:** `finalize()` sirasinda extrinsics root hesabi icin kullanilir
- Bu veriler extrinsic merkle root'u icin kullanilir

### 8. `Number<T>` (lib.rs:1022-1025)
```rust
pub(super) type Number<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;
```
- **Tip:** `BlockNumber`, varsayilan `0`
- **Getter:** `fn block_number()`
- **Yazma:** `initialize()` sirasinda set edilir
- Mevcut blok numarasini saklar
- `#[pallet::whitelist_storage]` — hot storage

### 9. `ParentHash<T>` (lib.rs:1028-1030)
```rust
pub(super) type ParentHash<T: Config> = StorageValue<_, T::Hash, ValueQuery>;
```
- **Tip:** `Hash`, varsayilan `Default::default()`
- **Getter:** `fn parent_hash()`
- **Yazma:** `initialize()` sirasinda
- Genesis'te `hash69` (0x45 ile dolu) kullanilir

### 10. `Digest<T>` (lib.rs:1033-1037)
```rust
pub(super) type Digest<T: Config> = StorageValue<_, generic::Digest, ValueQuery>;
```
- **Tip:** `generic::Digest` — `Vec<DigestItem>`
- **Getter:** `fn digest()`
- **Yazma:** `initialize()` ve `deposit_log()` ile
- `#[pallet::unbounded]` ve `#[pallet::whitelist_storage]`

### 11. `Events<T>` (lib.rs:1046-1051)
```rust
pub(super) type Events<T: Config> =
    StorageValue<_, Vec<Box<EventRecord<T::RuntimeEvent, T::Hash>>>, ValueQuery>;
```
- **Tip:** `Vec<Box<EventRecord>>` — Box kullanimi: bellekte event'ler büyük olabilir
- `#[pallet::unbounded]` — limit yok, **ON-CHAIN OKUNMAMALI**
- `#[pallet::disable_try_decode_storage]` — try-decode atlaniyor (büyük veri)
- **Yazma:** `deposit_event_indexed()` ile append edilir
- **Kritik Not:** PoV boyutunu sismirmemek icin runtime'da okunmamali
- `#[pallet::whitelist_storage]` — hot storage

### 12. `EventCount<T>` (lib.rs:1054-1057)
```rust
pub(super) type EventCount<T: Config> = StorageValue<_, EventIndex, ValueQuery>;
```
- **Tip:** `u32`, varsayilan `0`
- **Getter:** `fn event_count()`
- `checked_add` ile max'a ulastiginda event atilir (overflow koruması)

### 13. `EventTopics<T>` (lib.rs:1062-1073)
```rust
pub(super) type EventTopics<T: Config> =
    StorageMap<_, Blake2_128Concat, T::Hash, Vec<(BlockNumberFor<T>, EventIndex)>, ValueQuery>;
```
- **Tip:** `StorageMap<Hash -> Vec<(BlockNumber, EventIndex)>>`
- **Amac:** Topic tabanlı event indeksleme — light client subscription için
- `(BlockNumber, EventIndex)` tuple: ayni topic farkli bloklarda tekrarlanabilir (notification kaybı önlenir)
- `#[pallet::unbounded]`

### 14. `LastRuntimeUpgrade<T>` (lib.rs:1077-1078)
```rust
pub type LastRuntimeUpgrade<T: Config> = StorageValue<_, LastRuntimeUpgradeInfo>;
```
- **Tip:** `Option<LastRuntimeUpgradeInfo>`
- Genesis'te `T::Version::get()` ile dolduruluyor
- `LastRuntimeUpgradeInfo { spec_version: Compact<u32>, spec_name: Cow<'static, str> }`
- `was_upgraded()` metodu: yeni runtime mi?
- `#[pallet::unbounded]` — `spec_name` degisken uzunlukta

### 15. `BlocksTillUpgrade<T>` (lib.rs:1081-1082)
```rust
pub(super) type BlocksTillUpgrade<T: Config> = StorageValue<_, u8>;
```
- **Tip:** `Option<u8>` — kac blok sonra pending code uygulanacak
- `system_version >= 3` iken kullanilir
- `update_code_in_storage()`: `put(2u8)` ile 2 blok sonraya ayarlar
- `maybe_apply_pending_code_upgrade()`: her blokta azaltir, 0'a gelince code uygulanir

### 16. `UpgradedToU32RefCount<T>` (lib.rs:1085-1086)
```rust
pub(super) type UpgradedToU32RefCount<T: Config> = StorageValue<_, bool, ValueQuery>;
```
- Migration flag: eski `u8` ref count'tan `u32`'ye geçildi mi
- Genesis'te `true` set edilir (yeni chain'de zaten u32)

### 17. `UpgradedToTripleRefCount<T>` (lib.rs:1089-1091)
```rust
pub(super) type UpgradedToTripleRefCount<T: Config> = StorageValue<_, bool, ValueQuery>;
```
- Migration flag: tek sayactan (consumers, providers, sufficients) üçlüye geçildi mi
- Genesis'te `true` set edilir

### 18. `ExecutionPhase<T>` (lib.rs:1094-1096)
```rust
pub(super) type ExecutionPhase<T: Config> = StorageValue<_, Phase>;
```
- **Tip:** `Option<Phase>`
- `Phase` enum: `Initialization`, `ApplyExtrinsic(u32)`, `Finalization`
- Event record'larinda hangi fazda oldugunu belirtir
- `#[pallet::whitelist_storage]`

### 19. `AuthorizedUpgrade<T>` (lib.rs:1098-1102)
```rust
pub(super) type AuthorizedUpgrade<T: Config> =
    StorageValue<_, CodeUpgradeAuthorization<T>, OptionQuery>;
```
- **Tip:** `Option<CodeUpgradeAuthorization>`
- `CodeUpgradeAuthorization { code_hash: T::Hash, check_version: bool }`
- Two-phase upgrade: önce hash authorize edilir, sonra code apply edilir

### 20. `ExtrinsicWeightReclaimed<T>` (lib.rs:1111-1113)
```rust
pub type ExtrinsicWeightReclaimed<T: Config> = StorageValue<_, Weight, ValueQuery>;
```
- **Tip:** `Weight`, varsayilan `Weight::zero()`
- Dispatch sonrasi weight reclaim takibi
- `note_applied_extrinsic()` sifirlıyor, `reclaim_weight()` güncelliyor
- Duplicate reduction'i önlemek icin

---

## Account Ref Counting

### Neden Gerekli?

Substrate'de bir account'in storage'da yasamasi, onun "canliligi" ile ilgili kararlar verilmesi için referans sayim mekanizmasi kullanilir. Bu, Account silme/olusturma mantigi icin kritiktir.

### Üc Sayac: providers, consumers, sufficients

#### providers (lib.rs:1676-1688)
```rust
pub fn inc_providers(who: &T::AccountId) -> IncRefStatus {
    Account::<T>::mutate(who, |a| {
        if a.providers == 0 && a.sufficients == 0 {
            // Account olusturuluyor
            a.providers = 1;
            Self::on_created_account(who.clone(), a);
            IncRefStatus::Created
        } else {
            a.providers = a.providers.saturating_add(1);
            IncRefStatus::Existed
        }
    })
}
```

- **Anlami:** "Bu account'in var olmasina izin veren bir modül var"
- **Tipik kullanici:** pallet-balances (bakiye varsa provider)
- **Kural:** Account silinebilmesi icin providers = 0 OLMALI
- **Önemli:** `providers > 0` iken consumer eklenebilir

#### dec_providers (lib.rs:1693-1731)
```rust
pub fn dec_providers(who: &T::AccountId) -> Result<DecRefStatus, DispatchError> {
    Account::<T>::try_mutate_exists(who, |maybe_account| {
        if let Some(mut account) = maybe_account.take() {
            match (account.providers, account.consumers, account.sufficients) {
                (1, 0, 0) => {
                    // Son provider + consumer yok + sufficient yok = ÖLÜM
                    Pallet::<T>::on_killed_account(who.clone());
                    Ok(DecRefStatus::Reaped)
                },
                (1, c, _) if c > 0 => {
                    // Son provider ama consumer var = HATA
                    Err(DispatchError::ConsumerRemaining)
                },
                (x, _, _) => {
                    // Hala birden fazla provider var, hayatta
                    account.providers = x - 1;
                    *maybe_account = Some(account);
                    Ok(DecRefStatus::Exists)
                },
            }
        }
    })
}
```

**Kritik mantik:**
- `(1, 0, 0)` → hesap ölür, `on_killed_account` tetiklenir
- `(1, c > 0, _)` → `ConsumerRemaining` hatasi — consumer varken provider silinemez
- `(x > 1, _, _)` → hayatta, providers--

#### consumers (lib.rs:1802-1815)
```rust
pub fn inc_consumers(who: &T::AccountId) -> Result<(), DispatchError> {
    Account::<T>::try_mutate(who, |a| {
        if a.providers > 0 {
            if a.consumers < T::MaxConsumers::max_consumers() {
                a.consumers = a.consumers.saturating_add(1);
                Ok(())
            } else {
                Err(DispatchError::TooManyConsumers)
            }
        } else {
            Err(DispatchError::NoProviders)
        }
    })
}
```

- **Anlami:** "Bu modül bu account'a baginli (ör: açik bir reserve var)"
- **Kural:** `providers > 0` olmazsa consumer eklenemez
- **Kural:** `MaxConsumers::max_consumers()` asililamaz
- `inc_consumers_without_limit()` MaxConsumers'i atlayan versiyon (dahili kullanim icin)

**dec_consumers (lib.rs:1833-1844):**
```rust
pub fn dec_consumers(who: &T::AccountId) {
    Account::<T>::mutate(who, |a| {
        if a.consumers > 0 {
            a.consumers -= 1;
        } else {
            log::error!(target: LOG_TARGET, "Logic error: Unexpected underflow in reducing consumer");
        }
    })
}
```
- Hata durumunda paniklemez, sadece log atar (non-fatal)

#### sufficients (lib.rs:1734-1780)
```rust
pub fn inc_sufficients(who: &T::AccountId) -> IncRefStatus {
    Account::<T>::mutate(who, |a| {
        if a.providers + a.sufficients == 0 {
            // Account olusturuluyor — self-sufficient olarak
            a.sufficients = 1;
            Self::on_created_account(who.clone(), a);
            IncRefStatus::Created
        } else {
            a.sufficients = a.sufficients.saturating_add(1);
            IncRefStatus::Existed
        }
    })
}
```

- **Anlami:** Modül özel amaçlari icin hesabın yasayabilmesi
- **Farki:** Provider gerekmez — kendi basina yeterli (sufficient)
- **Örnek:** pallet-assets, assets tutanlar icin sufficient kullanır
- **Ölüm kurali:** `sufficients == 0 && providers == 0` → hesap ölür

### Referans Sayimi Özeti

```
providers = 0 && sufficients = 0 → Hesap var OLMAZ
providers > 0 → consumers eklenebilir
consumers > 0 → son provider silinemez
sufficients > 0 → kendi kendine yasayabilir (providers gereksiz)
```

### Helper Structlar (lib.rs:2508-2543)

```rust
pub struct Provider<T>(PhantomData<T>);
impl<T: Config> HandleLifetime<T::AccountId> for Provider<T> {
    fn created(t: &T::AccountId) -> Result<(), DispatchError> {
        Pallet::<T>::inc_providers(t); Ok(())
    }
    fn killed(t: &T::AccountId) -> Result<(), DispatchError> {
        Pallet::<T>::dec_providers(t).map(|_| ())
    }
}

pub struct SelfSufficient<T>(...);  // inc_sufficients / dec_sufficients
pub struct Consumer<T>(...);         // inc_consumers / dec_consumers
```

Bu structlar `HandleLifetime` trait'ini implement eder ve pallet-balances gibi pallet'lerin kullanabilmesi icin sarici gorevi gorur.

---

## Origin Sistemi

### RawOrigin Enum (lib.rs:971-972)
```rust
#[pallet::origin]
pub type Origin<T> = RawOrigin<<T as Config>::AccountId>;
```

`RawOrigin` frame_support'ta tanimlanir:
```rust
pub enum RawOrigin<AccountId> {
    Root,              // Governance/sudo
    Signed(AccountId), // Normal imzalanmis tx
    None,              // Imzasiz tx
    Authorized,        // AuthorizeCall ile onaylanmis
}
```

### ensure_* Fonksiyonlari

**ensure_signed (lib.rs:1452-1460):**
```rust
pub fn ensure_signed<OuterOrigin, AccountId>(o: OuterOrigin) -> Result<AccountId, BadOrigin>
where
    OuterOrigin: Into<Result<RawOrigin<AccountId>, OuterOrigin>>,
{
    match o.into() {
        Ok(RawOrigin::Signed(t)) => Ok(t),
        _ => Err(BadOrigin),
    }
}
```

**ensure_root (lib.rs:1479-1487):**
```rust
pub fn ensure_root<OuterOrigin, AccountId>(o: OuterOrigin) -> Result<(), BadOrigin>
where OuterOrigin: Into<Result<RawOrigin<AccountId>, OuterOrigin>> {
    match o.into() {
        Ok(RawOrigin::Root) => Ok(()),
        _ => Err(BadOrigin),
    }
}
```

**ensure_none (lib.rs:1490-1498):**
```rust
pub fn ensure_none<OuterOrigin, AccountId>(o: OuterOrigin) -> Result<(), BadOrigin>
where OuterOrigin: Into<Result<RawOrigin<AccountId>, OuterOrigin>> {
    match o.into() {
        Ok(RawOrigin::None) => Ok(()),
        _ => Err(BadOrigin),
    }
}
```

**ensure_authorized (lib.rs:1502-1510) — YENİ:**
```rust
pub fn ensure_authorized<OuterOrigin, AccountId>(o: OuterOrigin) -> Result<(), BadOrigin>
where OuterOrigin: Into<Result<RawOrigin<AccountId>, OuterOrigin>> {
    match o.into() {
        Ok(RawOrigin::Authorized) => Ok(()),
        _ => Err(BadOrigin),
    }
}
```

**ensure_signed_or_root (lib.rs:1465-1476):**
```rust
pub fn ensure_signed_or_root<OuterOrigin, AccountId>(
    o: OuterOrigin,
) -> Result<Option<AccountId>, BadOrigin> {
    match o.into() {
        Ok(RawOrigin::Root) => Ok(None),
        Ok(RawOrigin::Signed(t)) => Ok(Some(t)),
        _ => Err(BadOrigin),
    }
}
```

### EnsureOrigin Implementasyonlari

**EnsureRoot (lib.rs:1278-1292):**
- Her call icin `as_system_ref()` kullanarak origin'i kontrol eder
- `try_origin()`: `Root`sa `Ok(())`, degilse `Err(o)` (origin geri döner!)

**EnsureSigned (lib.rs:1348-1366):**
- `type Success = AccountId`
- `Some(RawOrigin::Signed(who))` → `Ok(who.clone())`

**EnsureSignedBy (lib.rs:1375-1404):**
- `Who: SortedMembers<AccountId>` constraint
- Belirli bir kümedeki üyelerden imzalanmis olmali

**EnsureNone (lib.rs:1407-1421):**
- Imzasiz tx icin

**EnsureNever (lib.rs:1430-1447):**
- Her zaman basarisiz olur — yasakli origin tanimlamak icin

**EnsureRootWithSuccess (lib.rs:1301-1325):**
- Root olmali ama `TypedGet` ile özel bir success degeri döndürür
- Governance tarafindan configure edilmis degerler icin kullanisli

### Kullanim Senaryolari

| Origin | Kullanim |
|--------|----------|
| `Root` | Governance kararları, sudo, runtime upgrade |
| `Signed(who)` | Normal kullanici tx'leri, fee ödenir |
| `None` | Oracle price feed, inherent benzeri ama signed olmayan |
| `Authorized` | `AuthorizeCall` extension'i ile zincir tarafindan onaylanmis tx'ler |

---

## Blok Agirlik Sistemi

### Weight Yapisi (sp_weights)

```rust
pub struct Weight {
    pub ref_time: u64,    // Referans donanim üzerinde pico-saniye cinsinden execution time
    pub proof_size: u64,  // PoV (Proof of Validity) boyutu bayt cinsinden
}
```

**İki boyutlu kaynak modeli:**
- `ref_time`: CPU süre ağırlığı (hesaplama maliyeti)
- `proof_size`: Parachain validation icin state proof boyutu

### DispatchClass Enum

```rust
pub enum DispatchClass {
    Normal,       // Normal kullanici tx'leri — %75 blok alanı
    Operational,  // Yüksek öncelik (node operatörleri tarafindan) — full blok erişimi + rezerv
    Mandatory,    // Zorunlu (inherent'lar) — limit yok, her zaman dahil edilir
}
```

### BlockWeights Yapisi (limits.rs:268-275)

```rust
pub struct BlockWeights {
    pub base_block: Weight,                     // Her blok için baz maliyet
    pub max_block: Weight,                      // Maksimum blok ağırlığı
    pub per_class: PerDispatchClass<WeightsPerClass>, // Per-class limitler
}
```

### WeightsPerClass Yapisi (limits.rs:167-191)

```rust
pub struct WeightsPerClass {
    pub base_extrinsic: Weight,     // Her extrinsic için baz maliyet (base_block hariç)
    pub max_extrinsic: Option<Weight>, // Tek extrinsic max ağırlığı (base haric)
    pub max_total: Option<Weight>,  // Bu class için toplam blok limiti (None = limitsiz)
    pub reserved: Option<Weight>,   // Bu class için garantili alan
}
```

**None semantigi:**
- `max_total = None`: Bu class limitsiz (yalnizca Mandatory için tavsiye edilir)
- `reserved = None`: Blok dolu olsa bile max_total'a kadar girmeye çalışılır

### BlockLength Yapisi (limits.rs:38-51)

```rust
pub struct BlockLength {
    pub max: PerDispatchClass<u32>,   // Her class icin max uzunluk (bayt)
    pub max_header_size: Option<u32>, // Header max boyutu (None = max_block/5)
}
```

**Defaults:**
- 5MB blok boyutu
- Normal: `%75 * 5MB = 3.75MB`
- max_header_size: blok max'ının %20'si

### with_sensible_defaults (limits.rs:392-405)

```rust
pub fn with_sensible_defaults(expected_block_weight: Weight, normal_ratio: Perbill) -> Self {
    let normal_weight = normal_ratio * expected_block_weight;
    Self::builder()
        .for_class(DispatchClass::Normal, |weights| {
            weights.max_total = normal_weight.into();   // Normal: %75 blok
        })
        .for_class(DispatchClass::Operational, |weights| {
            weights.max_total = expected_block_weight.into(); // Operational: full blok
            weights.reserved = (expected_block_weight - normal_weight).into(); // Rezerv: %25
        })
        .avg_block_initialization(Perbill::from_percent(10)) // on_initialize: %10 maliyet beklentisi
        .build()
}
```

**Neden önemli:** Normal tx %75 doldurunca blok kapanmaz — Operational tx hala %25 rezerv kullanabilir.

### Blok Agirlik Hesabi Akisi (check_weight.rs)

**calculate_consumed_extrinsic_weight (check_weight.rs:157-168):**
```rust
pub fn calculate_consumed_extrinsic_weight<Call>(
    maximum_weight: &BlockWeights,
    info: &DispatchInfoOf<Call>,
    len: usize,
) -> Weight {
    info.total_weight()
        .saturating_add(maximum_weight.get(info.class).base_extrinsic)
        .saturating_add_proof_size(len as u64)
}
```

**Formul:** `call_weight + extension_weight + base_extrinsic + len_bytes (proof_size olarak)`

**calculate_consumed_weight (check_weight.rs:173-237):**
1. Extrinsic agirligini hesapla
2. Class'a bagli: checked_accrue (limitsiz degilse) veya accrue
3. Per-class limit kontrolü: `max_total` asildi mi?
4. Genel blok limiti asildi mi? → `reserved` pool'a bak
5. Eğer reserved'den de taşıyorsa: `ExhaustsResources`

**Mandatory extrinsic'ler limiti atamazlar:** Limit yok (None), `saturating_add` kullanilır.

---

## Kritik Fonksiyonlar

### initialize (lib.rs:1970-2006)

```rust
pub fn initialize(number: &BlockNumberFor<T>, parent_hash: &T::Hash, digest: &generic::Digest) {
    let expected_block_number = Self::block_number() + One::one();
    assert_eq!(expected_block_number, *number, "Block number must be strictly increasing.");

    ExecutionPhase::<T>::put(Phase::Initialization);
    storage::unhashed::put(well_known_keys::EXTRINSIC_INDEX, &0u32);
    Self::initialize_intra_block_entropy(parent_hash);
    <Number<T>>::put(number);
    <Digest<T>>::put(digest);
    <ParentHash<T>>::put(parent_hash);
    <BlockHash<T>>::insert(*number - One::one(), parent_hash);

    BlockWeight::<T>::kill(); // Agirlik sifirla

    // Digest + empty header boyutunu hesapla
    let digest_size = digest.encoded_size();
    let empty_header = ...;
    let overhead = digest_size.saturating_add(empty_header_size) as u32;
    BlockSize::<T>::put(overhead);

    // Header max limiti kontrol et
    assert!(overhead <= max_total_header as u32, "Header size exceeds limit");
}
```

**Kritik:** Blok numarasinin kesinlikle arttigi kontrol edilir (assert ile). Test'te `set_block_number()` kullanilmali.

### finalize (lib.rs:2072-2113)

```rust
pub fn finalize() -> HeaderFor<T> {
    Self::resource_usage_report();
    ExecutionPhase::<T>::kill();
    BlockSize::<T>::kill();
    storage::unhashed::kill(well_known_keys::INTRABLOCK_ENTROPY);
    InherentsApplied::<T>::kill();

    let number = <Number<T>>::get();
    let parent_hash = <ParentHash<T>>::get();
    let digest = <Digest<T>>::get();

    // Extrinsics root hesapla
    let extrinsics = (0..ExtrinsicCount::<T>::take().unwrap_or_default())
        .map(ExtrinsicData::<T>::take)
        .collect();
    let extrinsics_root = extrinsics_data_root::<T::Hashing>(extrinsics, state_version);

    // Eski block hash'i temizle
    let to_remove = number.saturating_sub(block_hash_count).saturating_sub(One::one());
    if !to_remove.is_zero() {
        <BlockHash<T>>::remove(to_remove);
    }

    // Storage root hesapla
    let storage_root = T::Hash::decode(&mut &sp_io::storage::root(version)[..])
        .expect("Node is configured...; qed");

    HeaderFor::<T>::new(number, extrinsics_root, storage_root, parent_hash, digest)
}
```

**Önemli:** Events, EventCount, EventTopics, Number, ParentHash, Digest — `finalize` tarafindan temizlenmez. Client'in incelemesi icin saklanir; `initialize()` temizler.

### note_applied_extrinsic (lib.rs:2341-2381)

```rust
pub fn note_applied_extrinsic(r: &DispatchResultWithPostInfo, info: DispatchInfo) {
    let weight = extract_actual_weight(r, &info)
        .saturating_add(T::BlockWeights::get().get(info.class).base_extrinsic);
    let class = info.class;
    let pays_fee = extract_actual_pays_fee(r, &info);
    let dispatch_event_info = DispatchEventInfo { weight, class, pays_fee };

    Self::deposit_event(match r {
        Ok(_) => Event::ExtrinsicSuccess { dispatch_info: dispatch_event_info },
        Err(err) => Event::ExtrinsicFailed { dispatch_error: err.error, dispatch_info: dispatch_event_info },
    });

    // Extrinsic index'i artir
    let next_extrinsic_index = Self::extrinsic_index().unwrap_or_default() + 1u32;
    storage::unhashed::put(well_known_keys::EXTRINSIC_INDEX, &next_extrinsic_index);
    ExecutionPhase::<T>::put(Phase::ApplyExtrinsic(next_extrinsic_index));
    ExtrinsicWeightReclaimed::<T>::kill(); // Weight reclaim sifirla
}
```

### update_code_in_storage (lib.rs:1595-1609)

```rust
pub fn update_code_in_storage(code: &[u8]) {
    match T::Version::get().system_version {
        0..=2 => {
            storage::unhashed::put_raw(well_known_keys::CODE, code); // Direk yaz
        },
        _ => {
            BlocksTillUpgrade::<T>::put(2u8); // 2 blok sonraya planla
            storage::unhashed::put_raw(well_known_keys::PENDING_CODE, code);
        },
    }
    let hash = T::Hashing::hash(code);
    Self::deposit_log(generic::DigestItem::RuntimeEnvironmentUpdated);
    Self::deposit_event(Event::CodeUpdated { hash });
}
```

**system_version >= 3** icin: kod direk uygulanmaz, 2 blok bekler. Bu parachain'lerde validation'ın tamamlanmasını sağlamak için.

### can_set_code (lib.rs:2413-2443)

```rust
pub fn can_set_code(code: &[u8], check_version: bool) -> CanSetCodeResult<T> {
    if T::MultiBlockMigrator::ongoing() {
        return CanSetCodeResult::MultiBlockMigrationsOngoing;
    }

    if check_version {
        let current_version = T::Version::get();
        let Some(new_version) = sp_io::misc::runtime_version(code)
            .and_then(|v| RuntimeVersion::decode(&mut &v[..]).ok())
        else {
            return CanSetCodeResult::InvalidVersion(Error::<T>::FailedToExtractRuntimeVersion);
        };

        if new_version.spec_name != current_version.spec_name {
            return CanSetCodeResult::InvalidVersion(Error::<T>::InvalidSpecName)
        }

        if new_version.spec_version <= current_version.spec_version {
            return CanSetCodeResult::InvalidVersion(Error::<T>::SpecVersionNeedsToIncrease)
        }
    }

    CanSetCodeResult::Ok
}
```

### reclaim_weight (lib.rs:2467-2487)

```rust
pub fn reclaim_weight(
    info: &DispatchInfoOf<T::RuntimeCall>,
    post_info: &PostDispatchInfoOf<T::RuntimeCall>,
) -> Result<(), TransactionValidityError> {
    let already_reclaimed = crate::ExtrinsicWeightReclaimed::<T>::get();
    let unspent = post_info.calc_unspent(info);
    let accurate_reclaim = already_reclaimed.max(unspent);
    // Saturation asla olmaz, yukarida max aldik
    let to_reclaim_more = accurate_reclaim.saturating_sub(already_reclaimed);
    if to_reclaim_more != Weight::zero() {
        crate::BlockWeight::<T>::mutate(|current_weight| {
            current_weight.reduce(to_reclaim_more, info.class);
        });
        crate::ExtrinsicWeightReclaimed::<T>::put(accurate_reclaim);
    }
    Ok(())
}
```

**Çift reclaim koruması:** `already_reclaimed.max(unspent)` — daha fazlasını geri almak ancak şimdi kesin olarak biliniyorsa yapılır.

### deposit_event_indexed (lib.rs:1892-1921)

```rust
pub fn deposit_event_indexed(topics: &[T::Hash], event: T::RuntimeEvent) {
    let block_number = Self::block_number();
    if block_number.is_zero() { return; } // Genesis'te event yok

    let phase = ExecutionPhase::<T>::get().unwrap_or_default();
    let event = EventRecord { phase, event, topics: topics.to_vec() };

    let event_idx = {
        let old_event_count = EventCount::<T>::get();
        let new_event_count = match old_event_count.checked_add(1) {
            None => return, // MAX event sayisina ulasindi, sessizce atla
            Some(nc) => nc,
        };
        EventCount::<T>::put(new_event_count);
        old_event_count
    };

    Events::<T>::append(event);

    for topic in topics {
        <EventTopics<T>>::append(topic, &(block_number, event_idx));
    }
}
```

### unique (lib.rs:2500-2506)

```rust
pub fn unique(entropy: impl Encode) -> [u8; 32] {
    let mut last = [0u8; 32];
    sp_io::storage::read(well_known_keys::INTRABLOCK_ENTROPY, &mut last[..], 0);
    let next = (b"frame_system::unique", entropy, last).using_encoded(blake2_256);
    sp_io::storage::set(well_known_keys::INTRABLOCK_ENTROPY, &next);
    next
}
```

**Kullanim:** Blok icinde benzersiz, önceden hesaplanamaz 32 byte üretmek icin. `initialize_intra_block_entropy()` ile parent_hash kullanilarak baslatilir.

---

## Runtime Upgrade Hooks

### Hooks Implementasyonu (lib.rs:690-696)

```rust
#[pallet::hooks]
impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
    #[cfg(feature = "std")]
    fn integrity_test() {
        T::BlockWeights::get().validate().expect("The weights are invalid.");
    }
}
```

**Not:** System pallet'in kendisi `on_initialize`, `on_finalize`, `on_runtime_upgrade` implement etmez! Bu islevler `frame_executive` tarafindan dogrudan `Pallet::initialize()` ve `Pallet::finalize()` çagrilarak yapilir.

### Runtime Upgrade Akisi

1. `set_code` veya `apply_authorized_upgrade` cagrisi
2. `can_set_code()` kontrolü (MultiBlockMigrations check, version check)
3. `T::OnSetCode::set_code(code)` cagrisi
4. Default impl (`()`): `update_code_in_storage()` cagrisi
5. `system_version >= 3`: `BlocksTillUpgrade::put(2u8)` ve `PENDING_CODE`'a yazilir
6. 2 blok sonra `maybe_apply_pending_code_upgrade()` ile aktif `CODE`'a tasınır

### AuthorizedUpgrade (iki fazli) (lib.rs:837-904)

**Faz 1 — authorize_upgrade:**
```rust
pub fn authorize_upgrade(origin: OriginFor<T>, code_hash: T::Hash) -> DispatchResult {
    ensure_root(origin)?;
    Self::do_authorize_upgrade(code_hash, true);
    Ok(())
}
```

**Faz 2 — apply_authorized_upgrade (tüm origin'lerden):**
```rust
pub fn apply_authorized_upgrade(_: OriginFor<T>, code: Vec<u8>) -> DispatchResultWithPostInfo {
    let res = Self::validate_code_is_authorized(&code)?;
    AuthorizedUpgrade::<T>::kill();

    match Self::can_set_code(&code, res.check_version) {
        CanSetCodeResult::Ok => {},
        CanSetCodeResult::MultiBlockMigrationsOngoing => return Err(...),
        CanSetCodeResult::InvalidVersion(error) => {
            Self::deposit_event(Event::RejectedInvalidAuthorizedUpgrade {...});
            return Ok(Pays::No.into()); // Gecersiz upgrade için fee yok
        },
    };
    T::OnSetCode::set_code(code)?;
    Ok(PostDispatchInfo {
        actual_weight: Some(T::BlockWeights::get().max_block), // Blogu doldur
        pays_fee: Pays::No, // Gecerli upgrade için fee yok
    })
}
```

**Tasarim kararlari:**
- `Pays::No` — upgrade çağıranı cezalandirma, teşvik et
- `max_block` weight — code uygulandiktan sonra başka tx'lere yer kalmasın

---

## Transaction Extension Sistemi

### TransactionExtension Trait (yeni sistem)

Substrate'de eski `SignedExtension` yerine daha güclü `TransactionExtension` kullanilmaktadir. Temel metodlar:

```rust
trait TransactionExtension<Call> {
    const IDENTIFIER: &'static str;
    type Implicit;  // Signed payload'a eklenen implicit veri
    type Val;       // validate -> prepare arasi iletilen veri
    type Pre;       // prepare -> post_dispatch arasi iletilen veri

    fn weight(&self, call: &Call) -> Weight;
    fn validate(...) -> ValidateResult<Self::Val, Call>;
    fn prepare(...) -> Result<Self::Pre, TransactionValidityError>;
    fn post_dispatch_details(...) -> Result<Weight, TransactionValidityError>;
}
```

**validate:** Tx pool'a eklenip eklenmemesi karari + Val üretimi
**prepare:** Dispatch öncesi son kontroller + Pre üretimi
**post_dispatch_details:** Dispatch sonrasi temizlik + refund

---

## Extensions Detayli Analizi

### CheckWeight (check_weight.rs)

```rust
pub struct CheckWeight<T: Config + Send + Sync>(core::marker::PhantomData<T>);

impl<T> TransactionExtension<T::RuntimeCall> for CheckWeight<T> {
    const IDENTIFIER: &'static str = "CheckWeight";
    type Implicit = ();
    type Pre = ();
    type Val = u32; // Sonraki blok uzunlugu
```

**validate:**
1. `check_block_length()` — uzunluk limiti kontrolü (validation'da blok weight check atlanir)
2. `check_extrinsic_weight()` — tek extrinsic max weight kontrolü
3. Sonraki blok uzunlugunu `Val` olarak döndürür

**prepare:**
1. `calculate_consumed_weight()` — detayli agirlik hesabi
2. `BlockSize` ve `BlockWeight` güncellenir

**post_dispatch_details:**
1. `reclaim_weight()` — kullanilmayan weight geri alinir

**Kritik check_block_length (check_weight.rs:82-102):**
```rust
fn check_block_length(info: &..., len: usize) -> Result<u32, ...> {
    let next_len = current_len.saturating_add(added_len);
    if next_len > *length_limit.max.get(info.class) {
        Err(InvalidTransaction::ExhaustsResources.into())
    } else {
        Ok(next_len)
    }
}
```

### CheckNonce (check_nonce.rs)

```rust
pub struct CheckNonce<T: Config>(#[codec(compact)] pub T::Nonce);
```

**validate_nonce_for_account (check_nonce.rs:70-91):**
```rust
pub fn validate_nonce_for_account(who, nonce) -> Result<ValidNonceInfo, ...> {
    let account = crate::Account::<T>::get(who);
    if account.providers.is_zero() && account.sufficients.is_zero() {
        return Err(InvalidTransaction::Payment.into()); // Hesap yok = fee ödenemez
    }
    if nonce < account.nonce {
        return Err(InvalidTransaction::Stale.into()); // Eski nonce
    }

    let provides = vec![Encode::encode(&(who.clone(), nonce))];
    let requires = if account.nonce < nonce {
        vec![Encode::encode(&(who.clone(), nonce - 1))] // Önceki tx önce
    } else {
        vec![]
    };
    Ok(ValidNonceInfo { provides, requires })
}
```

**provides/requires:** Tx pool sıralama için! `nonce=5` tx'i nonce=4 tx'i bekledir.

**prepare_nonce_for_account (check_nonce.rs:94-105):**
```rust
pub fn prepare_nonce_for_account(who, mut nonce) -> Result<(), ...> {
    let account = crate::Account::<T>::get(who);
    if nonce > account.nonce {
        return Err(InvalidTransaction::Future.into()); // Validate'den bu yana nonce degisti
    }
    nonce = nonce.checked_add(&T::Nonce::one()).unwrap_or(T::Nonce::zero());
    crate::Account::<T>::mutate(who, |account| account.nonce = nonce);
    Ok(())
}
```

**Güvenlik:** `validate` ve `prepare` arasi nonce degismediyse gecerli. Eger degistiyse `Future` hatasi.

### CheckMortality (check_mortality.rs)

```rust
pub struct CheckMortality<T>(pub Era, PhantomData<T>);
```

**Era:** `Immortal` veya `Mortal { period: u64, phase: u64 }`

**implicit (check_mortality.rs:64-72):**
```rust
fn implicit(&self) -> Result<Self::Implicit, ...> {
    let current_u64 = Pallet::<T>::block_number().saturated_into::<u64>();
    let n = self.0.birth(current_u64).saturated_into::<BlockNumberFor<T>>();
    if !<BlockHash<T>>::contains_key(n) {
        Err(InvalidTransaction::AncientBirthBlock.into())
    } else {
        Ok(<Pallet<T>>::block_hash(n))  // Birth block hash signed payload'a eklenir
    }
}
```

**validate:**
- `valid_till = era.death(current_u64)` — hangi blokta expire olur
- `longevity = valid_till - current` — tx pool ömrü

**Önem:** `Implicit = T::Hash` — birth block hash'i signed payload'a eklenir. Replay korumasi: baska bir chain'de bu hash farkli olur.

### CheckGenesis (check_genesis.rs)

```rust
fn implicit(&self) -> Result<Self::Implicit, ...> {
    Ok(<Pallet<T>>::block_hash(BlockNumberFor::<T>::zero()))
}
```

Genesis hash signed payload'a eklenir. Farkli network'lerde genesis farkli olacagi icin cross-network replay saldirisi engellenir. `Implicit = T::Hash`.

**proof_size 0:** Genesis hash her tx'te okunur, PoV'da cift saymasın.

### CheckSpecVersion (check_spec_version.rs)

```rust
fn implicit(&self) -> Result<Self::Implicit, ...> {
    Ok(<Pallet<T>>::runtime_version().spec_version)
}
```

`Implicit = u32` — spec_version signed payload'a eklenir. Farkli runtime versiyonunda tx geçersiz sayılır.

### CheckTxVersion (check_tx_version.rs)

```rust
fn implicit(&self) -> Result<Self::Implicit, ...> {
    Ok(<Pallet<T>>::runtime_version().transaction_version)
}
```

`Implicit = u32` — transaction_version signed payload'a eklenir. Extrinsic formatı değiştiginde eski tx'ler reddedilir.

### CheckNonZeroSender (check_non_zero_sender.rs)

```rust
fn validate(&self, origin, ...) -> ValidateResult<...> {
    if let Some(who) = origin.as_signer() {
        if who.using_encoded(|d| d.iter().all(|x| *x == 0)) {
            return Err(InvalidTransaction::BadSigner.into());
        }
    }
    Ok((Default::default(), (), origin))
}
```

Zero account ID'li tx'leri engeller. Özellikle Ethereum uyumlulugu icin kritik (0x0000 adresi).

### AuthorizeCall (authorize_call.rs)

```rust
fn validate(&self, origin, call, ...) -> ValidateResult<...> {
    if !origin.is_transaction_authorized() {
        if let Some(authorize) = call.authorize(source) {
            return authorize.map(|(validity, unspent)| {
                (validity, unspent, crate::Origin::<T>::Authorized.into())
            });
        }
    }
    Ok((Default::default(), Weight::zero(), origin))
}
```

**Konsept:** Tx imzasiz gönderilir ama `call.authorize()` metodu cagrilarak runtime tarafindan validate edilir. Basan: origin `Authorized` olarak ayarlaniyor. Başarisiz: tx reddedilir.

**Kullanim:** `#[pallet::authorize(...)]` macro ile per-call authorization mantigi tanimlanir.

### WeightReclaim (weight_reclaim.rs)

```rust
fn post_dispatch_details(&pre, info, post_info, ...) -> Result<Weight, ...> {
    crate::Pallet::<T>::reclaim_weight(info, post_info).map(|()| Weight::zero())
}
```

Dispatch sonrasi kullanilmayan weight'i geri alir. `CheckWeight` ile ayni islevi yapar — ikisi birlikte kullanilmaz, hangisi daha sonra cagrilirnca o etki eder (idempotent `max` mantigi sayesinde).

---

## Events ve Errors

### Events (lib.rs:907-935)

```rust
pub enum Event<T: Config> {
    // Extrinsic basarili tamamlandi
    ExtrinsicSuccess { dispatch_info: DispatchEventInfo },

    // Extrinsic basarisiz oldu
    ExtrinsicFailed { dispatch_error: DispatchError, dispatch_info: DispatchEventInfo },

    // :code güncellendi, hash ile
    CodeUpdated { hash: T::Hash },

    // Yeni hesap olusturuldu
    NewAccount { account: T::AccountId },

    // Hesap silindi
    KilledAccount { account: T::AccountId },

    // On-chain remark yapildi
    Remarked { sender: T::AccountId, hash: T::Hash },

    // [experimental] Task basladi
    TaskStarted { task: T::RuntimeTask },

    // [experimental] Task tamamlandi
    TaskCompleted { task: T::RuntimeTask },

    // [experimental] Task basarisiz
    TaskFailed { task: T::RuntimeTask, err: DispatchError },

    // Upgrade authorize edildi
    UpgradeAuthorized { code_hash: T::Hash, check_version: bool },

    // Gecersiz authorized upgrade reddedildi
    RejectedInvalidAuthorizedUpgrade { code_hash: T::Hash, error: DispatchError },
}
```

**DispatchEventInfo:**
```rust
pub struct DispatchEventInfo {
    pub weight: Weight,       // Gerçek kullanilan agirlik
    pub class: DispatchClass, // Normal/Operational/Mandatory
    pub pays_fee: Pays,       // Ücret ödendi mi
}
```

### Errors (lib.rs:938-968)

```rust
pub enum Error<T> {
    InvalidSpecName,                  // spec_name uyusmuyor
    SpecVersionNeedsToIncrease,       // spec_version azalmis veya degismemis
    FailedToExtractRuntimeVersion,    // Yeni code'dan version alinamiyor
    NonDefaultComposite,              // Hesapta non-default data var, silinemez
    NonZeroRefCount,                  // RefCount > 0, hesap silinemez
    CallFiltered,                     // BaseCallFilter engelledi
    MultiBlockMigrationsOngoing,      // MBM sürüyor, code değiştirilemiyor
    InvalidTask,                      // [experimental] Görev geçersiz
    FailedTask,                       // [experimental] Görev başarısız
    NothingAuthorized,                // apply_authorized_upgrade ama authorize yok
    Unauthorized,                     // Code hash uyuşmuyor
}
```

---

## Offchain Worker Altyapisi (offchain.rs)

### Temel Yapılar

```rust
pub struct ForAll {}   // Tüm hesaplar ile imzala
pub struct ForAny {}   // Herhangi bir hesap ile imzala

pub struct Signer<T: SigningTypes, C: AppCrypto<T::Public, T::Signature>, X = ForAny> {
    accounts: Option<Vec<T::Public>>,
    _phantom: PhantomData<(X, C)>,
}

pub struct SubmitTransaction<T: CreateTransactionBase<RuntimeCall>, RuntimeCall> {...}
```

### SigningTypes Trait
```rust
pub trait SigningTypes: crate::Config {
    type Public: ...;
    type Signature: ...;
}
```

### AppCrypto Trait
```rust
pub trait AppCrypto<Public, Signature>: {
    type RuntimeAppPublic: RuntimeAppPublic;
    type GenericSignature: Into<Signature>;
    type GenericPublic: From<Self::RuntimeAppPublic> + Into<Public>;
}
```

**Kullanim:**
1. Off-chain worker keystore'dan key al
2. Payload imzala
3. Unsigned tx (signed payload ile) veya signed tx göndermek

---

## Migrations (migrations/mod.rs)

### Referans Sayim Migration Tarihi

Eski → Yeni format:
- `V1`: Tek `u8` ref count
- `V2`: Tek `u32` ref count
- `V3`: Üçlü `u32` (consumers, providers, sufficients)

```rust
pub fn migrate_from_single_u8_to_triple_ref_count<V: V2ToV3, T: Config>() -> Weight {
    <Account<V, T>>::translate::<(V::Nonce, u8, V::AccountData), _>(
        |_key, (nonce, rc, data)| {
            Some(AccountInfo {
                nonce,
                consumers: rc as RefCount,
                providers: 1,  // Eski hesaplarda provider = 1 var sayilir
                sufficients: 0,
                data
            })
        }
    );
    ...
}
```

**Önemli:** Migration'larda eski hesaplar `providers: 1` ile baslatilir — varligina garantilenmiş.

---

## Weights Analizi (weights.rs)

**Benchmark ortami:** Intel Xeon 2.60GHz, WASM compiled, 50 steps, 20 repeat

Önemli weight degerleri:

| Fonksiyon | ref_time (ps) | proof_size (B) |
|-----------|---------------|----------------|
| `remark(b)` | 1_586_000 + 10_920*b | 0 |
| `remark_with_event(b)` | 4_725_000 + 12_386*b | 0 |
| `set_heap_pages` | 2_836_000 | 0 |
| `set_code` | 164_247_810_000 | 67035 |
| `set_storage(i)` | 1_565_000 + 733_648*i | 0 |
| `kill_storage(i)` | linear(i) | 0 |
| `authorize_upgrade` | ~5_000_000 | ~1500 |
| `apply_authorized_upgrade` | ~166M | ~1535 |

**Extensions weights:**

| Extension | ref_time (ps) | proof_size (B) |
|-----------|---------------|----------------|
| `check_genesis` | 3_577_000 | 3509 |
| `check_mortality_mortal` | 6_703_000 | 3509 |
| `check_mortality_immortal` | 6_605_000 | 0 (set_proof_size=0) |
| `check_nonce` | ~4_500_000 | ~1500 |
| `check_weight` | ~450_000 | 0 |
| `weight_reclaim` | ~500_000 | 0 |

---

## Dispatchable Fonksiyonlar (Extrinsic'ler)

### call_index ve DispatchClass Özeti

| Index | Fonksiyon | Class | Origin |
|-------|-----------|-------|--------|
| 0 | `remark` | Normal | any |
| 1 | `set_heap_pages` | Operational | Root |
| 2 | `set_code` | Operational | Root |
| 3 | `set_code_without_checks` | Operational | Root |
| 4 | `set_storage` | Operational | Root |
| 5 | `kill_storage` | Operational | Root |
| 6 | `kill_prefix` | Operational | Root |
| 7 | `remark_with_event` | Normal | Signed |
| 8 | `do_task` [experimental] | Normal | any |
| 9 | `authorize_upgrade` | Operational | Root |
| 10 | `authorize_upgrade_without_checks` | Operational | Root |
| 11 | `apply_authorized_upgrade` | Operational | any |

### set_code Analizi (lib.rs:722-729)

```rust
pub fn set_code(origin: OriginFor<T>, code: Vec<u8>) -> DispatchResultWithPostInfo {
    ensure_root(origin)?;
    Self::can_set_code(&code, true).into_result()?;  // Version check yapılır
    T::OnSetCode::set_code(code)?;
    // Blokun geri kalanini tüket — daha fazla tx girmesin
    Ok(Some(T::BlockWeights::get().max_block).into())
}
```

**max_block döndürmesi:** Runtime upgrade sonrasi aynı blokta başka tx'ler çalıştırılması güvensiz olabilir.

### remark vs remark_with_event

**remark (call_index=0):**
```rust
pub fn remark(_origin: OriginFor<T>, remark: Vec<u8>) -> DispatchResultWithPostInfo {
    let _ = remark; // Sadece veri depolamak — event yok
    Ok(().into())
}
```

**remark_with_event (call_index=7):**
```rust
pub fn remark_with_event(origin, remark: Vec<u8>) -> DispatchResultWithPostInfo {
    let who = ensure_signed(origin)?;
    let hash = T::Hashing::hash(&remark[..]);
    Self::deposit_event(Event::Remarked { sender: who, hash }); // Event emit
    Ok(().into())
}
```

---

## ValidateUnsigned (lib.rs:1136-1181)

System pallet'in imzasiz tx validation'ı:

```rust
fn validate_unsigned(source: TransactionSource, call: &Self::Call) -> TransactionValidity {
    if let Call::apply_authorized_upgrade { ref code } = call {
        if let Ok(res) = Self::validate_code_is_authorized(&code[..]) {
            if Self::can_set_code(&code, false).is_ok() {
                return Ok(ValidTransaction {
                    priority: u64::max_value(),    // Maksimum öncelik
                    requires: Vec::new(),           // Bağımlılık yok
                    provides: vec![res.code_hash.encode()],
                    longevity: TransactionLongevity::max_value(),
                    propagate: true,
                });
            }
        }
    }
    ...
}
```

`apply_authorized_upgrade` imzasiz gönderilebilir ve max priority verilir — kimse engelleyemesin.

---

## Phase Enum ve Execution Lifecycle

### Phase (lib.rs:1188-1203)

```rust
pub enum Phase {
    ApplyExtrinsic(u32),  // u32 = extrinsic indeksi
    Finalization,
    Initialization,
}
```

### EventRecord (lib.rs:1206-1215)

```rust
pub struct EventRecord<E: Parameter + Member, T> {
    pub phase: Phase,    // Event'in hangi fazda oluştugunu
    pub event: E,        // Event verisi
    pub topics: Vec<T>,  // Hash topic'ler
}
```

Her event hangi fazda ve hangi extrinsic sırasında olduğunu kaydeder. Off-chain indexer'lar bunu kullanır.

---

## ChainContext ve StoredMap

### ChainContext (lib.rs:2599-2613)

```rust
pub struct ChainContext<T>(PhantomData<T>);

impl<T: Config> Lookup for ChainContext<T> {
    type Source = <T::Lookup as StaticLookup>::Source;
    type Target = <T::Lookup as StaticLookup>::Target;

    fn lookup(&self, s: Self::Source) -> Result<Self::Target, LookupError> {
        <T::Lookup as StaticLookup>::lookup(s)
    }
}
```

`Lookup` trait implementasyonu — extrinsic check sırasında address lookup için kullanılır.

### StoredMap Implementasyonu (lib.rs:2563-2583)

```rust
impl<T: Config> StoredMap<T::AccountId, T::AccountData> for Pallet<T> {
    fn get(k: &T::AccountId) -> T::AccountData {
        Account::<T>::get(k).data
    }

    fn try_mutate_exists<R, E: From<DispatchError>>(
        k: &T::AccountId,
        f: impl FnOnce(&mut Option<T::AccountData>) -> Result<R, E>,
    ) -> Result<R, E> {
        let account = Account::<T>::get(k);
        let is_default = account.data == T::AccountData::default();
        let mut some_data = if is_default { None } else { Some(account.data) };
        let result = f(&mut some_data)?;
        if Self::providers(k) > 0 || Self::sufficients(k) > 0 {
            Account::<T>::mutate(k, |a| a.data = some_data.unwrap_or_default());
        } else {
            Account::<T>::remove(k) // Hesap ölür
        }
        Ok(result)
    }
}
```

`pallet-balances` bu `StoredMap` impl'ini kullanarak bakiye verilerini yönetir.

---

## Genesis Konfigürasyonu (lib.rs:1115-1133)

```rust
pub struct GenesisConfig<T: Config> {
    pub _config: PhantomData<T>, // Sadece PhantomData
}

fn build(&self) {
    <BlockHash<T>>::insert::<_, T::Hash>(BlockNumberFor::<T>::zero(), hash69());
    <ParentHash<T>>::put::<T::Hash>(hash69());
    <LastRuntimeUpgrade<T>>::put(LastRuntimeUpgradeInfo::from(T::Version::get()));
    <UpgradedToU32RefCount<T>>::put(true);
    <UpgradedToTripleRefCount<T>>::put(true);
    sp_io::storage::set(well_known_keys::EXTRINSIC_INDEX, &0u32.encode());
}
```

**hash69:** Her byte 0x45 (69) olan sahte genesis hash — genesis blok icin yer tutucu.

---

## Nova Shield Uygulama Notu

### 1. Account Ref Counting — KRITİK

**Alınacak Ders:** Substrate'in `providers/consumers/sufficients` sistemi, account garbage collection'ı için son derece elegant bir çözüm.

**Nova Shield Uygulamasi:**
- `AccountInfo` struct'ını aynı şekilde implement et: `{ nonce, consumers, providers, sufficients, data }`
- `inc_providers` / `dec_providers` mantığını kopyala: `(1, 0, 0)` → hesap ölür, `(1, c>0, _)` → hata
- `MaxConsumers` konfigürasyon olarak tut (nova chain'de 128 makul başlangıç)
- `OnNewAccount` ve `OnKilledAccount` hook'larını uygula — balances pallet entegrasyonu için şart
- `saturating_add/sub` kullan — overflow koruması zorunlu
- `inc_consumers_without_limit` iç kullanim için gerekli (bazi migrasyonlarda MaxConsumers atlanmalı)

**Kac satır kod:** Yaklaşık 300-400 satır Rust ile aynı mekanizma implement edilebilir.

### 2. Weight Sistemi — ÇIFT BOYUTLU ZORUNLU

**Alınacak Ders:** `Weight { ref_time: u64, proof_size: u64 }` — iki boyutlu kaynak modeli.

**Nova Shield Uygulaması:**
- Kendi `Weight` struct'ını `ref_time + proof_size` ile tanımla
- Eğer parachain yapılmasa bile `proof_size` gelecekte kullanışlı olabilir
- `PerDispatchClass<Weight>` kullan: Normal, Operational, Mandatory
- `BlockWeights` konfigürasyonu: `base_block`, `max_block`, per-class limitler
- Normal: `%75 blok alanı`, Operational: `%100 + %25 rezerv`, Mandatory: limitsiz
- `with_sensible_defaults()` mantığını aynen kullan — kanıtlanmış oranlar

**Benchmark sistemi:**
- Her extrinsic için `WeightInfo` trait'i tanımla
- Benchmark `#[pallet::weight(T::SystemWeightInfo::function_name())]` pattern'i
- Auto-generated weights for production

### 3. Origin Sistemi — TEMEL GÜVENLİK

**Alınacak Ders:** `RawOrigin` enum + `ensure_*` fonksiyonları + `EnsureOrigin` trait ailesi.

**Nova Shield Uygulaması:**
```rust
pub enum RawOrigin<AccountId> {
    Root,
    Signed(AccountId),
    None,
    Authorized, // AuthorizeCall için
}
```
- `ensure_root`, `ensure_signed`, `ensure_none`, `ensure_authorized` fonksiyonlarını implement et
- `EnsureRoot`, `EnsureSigned`, `EnsureNone` struct'larını implement et
- `BaseCallFilter` — paletlerin filterlanabilmesi için şart
- Governance modülü Root origin üretmeli

### 4. Transaction Extensions — REPLAY KORUMASI

**Alınacak Ders:** Extension pipeline'ı sayesinde her tx'e check eklenir.

**Nova Shield için minimum extension seti:**
```
CheckNonZeroSender → CheckSpecVersion → CheckTxVersion → CheckGenesis → CheckMortality → CheckNonce → CheckWeight → WeightReclaim
```

**Her extension'ın görevi:**
- `CheckNonZeroSender`: Zero hesap koruması
- `CheckSpecVersion`: Runtime version uyumu
- `CheckTxVersion`: Tx format uyumu
- `CheckGenesis`: Network-specific replay koruması (genesis hash)
- `CheckMortality`: Zaman-bazlı geçerlilik (Era::mortal/immortal)
- `CheckNonce`: Replay koruması + ordering (provides/requires)
- `CheckWeight`: Blok kaynak limiti
- `WeightReclaim`: Post-dispatch weight geri alımı

### 5. Storage Item Tasarımı — WHITELIST VE UNBOUNDED

**Alınacak Ders:** Hot storage items için `#[pallet::whitelist_storage]`, sınırsız veri için `#[pallet::unbounded]`.

**Nova Shield Uygulaması:**
- `BlockWeight`, `BlockSize`, `Number`, `ExtrinsicCount`, `Digest`, `Events`, `ExecutionPhase`, `ExtrinsicWeightReclaimed` → whitelist_storage
- `Events`, `EventTopics`, `ExtrinsicData`, `Digest` → unbounded (boyutu önceden bilinmiyor)
- Events'i runtime'da okuma! PoV şişirmez
- `BlockHash` → `Twox64Concat` hasher (güvensiz veriler Blake2 gerektirir, blok numaraları Twox yeterli)
- `Account` → `Blake2_128Concat` (AccountId dışarıdan gelir, güvenli hasher şart)

### 6. Runtime Upgrade Güvenliği

**Alınacak Ders:** Two-phase upgrade pattern.

**Nova Shield Uygulaması:**
- `authorize_upgrade` → sadece hash kaydet
- `apply_authorized_upgrade` → herkes gönderebilir (bridge senaryoları için önemli)
- Code hash kontrolü: `T::Hashing::hash(code) == authorization.code_hash`
- Geçerli upgrade'de `Pays::No` — teşvik
- Upgrade sonrası `max_block` weight → bloku kapat
- `MultiBlockMigrator::ongoing()` check — migration sürerken upgrade yok

### 7. Event Sistemi Tasarımı

**Alınacak Ders:** `EventRecord { phase, event, topics }` + topic-based indexing.

**Nova Shield Uygulaması:**
- Her event'e `Phase` bilgisi ekle (hangi extrinsic, initialization mı, finalization mi)
- Topic-based indexing light-client için şart
- `EventTopics` storage'ı: `hash -> Vec<(block_number, event_index)>`
- `block_number` + `event_index` tuple: aynı topic tekrar emit edilirse önceki kaybolmaz
- Event sayısı overflow'da silently drop — sonuç hatalı değil, sadece eksik

### 8. Intra-Block Entropy

**Alınacak Ders:** `unique()` fonksiyonu + `INTRABLOCK_ENTROPY`.

**Nova Shield Uygulaması:**
- Aynı blok içinde benzersiz ve öngörülemeyen 32 byte üretmek için
- Başlangıç: `hash(b"frame_system::initialize" || parent_hash)`
- Her çağrıda: `hash(b"frame_system::unique" || entropy || last_entropy)`
- **Kullanım yeri:** Lottery, random selection, MEV koruma nonces

### 9. Blok Lifecycle Management

**Alınacak Ders:** `initialize()` → extrinsics → `finalize()` akışı.

**Nova Shield Uygulaması:**
- `initialize()`: Blok numarası kontrolü (assert strictly increasing), tüm hot storage'ı temizle, entropy başlat
- `note_extrinsic()`: Her dispatch öncesi çağır
- `note_applied_extrinsic()`: Her dispatch sonrası çağır (ExtrinsicSuccess/Failed event'i)
- `note_finished_extrinsics()`: Tüm tx'lerden sonra
- `finalize()`: Extrinsic merkle root hesapla, storage root hesapla, header döndür

### 10. BlockHashCount Yönetimi

**Alınacak Ders:** Kaç blok geçmişi saklanır.

**Nova Shield Uygulaması:**
- Default 256 blok yeterli (yaklaşık 25-50 dakika 6s block time ile)
- `finalize()`: `number - block_hash_count - 1` numaralı blocku temizle
- Genesis hash'i ASLA temizleme (`!to_remove.is_zero()` koruması)
- `BlockHashCount` on-chain constant olarak tut

### 11. MultiBlockMigration Entegrasyonu

**Alınacak Ders:** Uzun süren migrasyonları bloka böl.

**Nova Shield Uygulaması:**
- `MultiStepMigrator` trait'i implement eden bir migration orchestrator
- `ongoing()` → runtime upgrade engelle
- `step()` → her blokta bir parça çalıştır
- Migration tamamlanana kadar normal tx'ler çalışmaya devam eder

### 12. Kritik Tasarim Karar: exec-phase tracking

**Alınacak Ders:** `ExecutionPhase` storage.

**Neden önemli:** Event'lerin hangi extrinsic sırasında emit edildiğini indexlemek için. Off-chain indexer'lar bunu kullanarak hangi event hangi tx'e ait bulmak için. Nova Shield da bunu implement etmeli.

---

## Kaynak Kod Referansları

**Ana dosyalar:**
- `Config trait`: lib.rs:488-685
- `Storage items`: lib.rs:975-1113
- `AccountInfo struct`: lib.rs:1235-1251
- `inc_providers`: lib.rs:1676-1688
- `dec_providers`: lib.rs:1693-1731
- `inc_sufficients`: lib.rs:1734-1746
- `inc_consumers`: lib.rs:1802-1815
- `dec_consumers`: lib.rs:1833-1844
- `initialize()`: lib.rs:1970-2006
- `finalize()`: lib.rs:2072-2113
- `note_applied_extrinsic()`: lib.rs:2341-2381
- `can_set_code()`: lib.rs:2413-2443
- `reclaim_weight()`: lib.rs:2467-2487
- `deposit_event_indexed()`: lib.rs:1892-1921
- `unique()`: lib.rs:2500-2506
- `update_code_in_storage()`: lib.rs:1595-1609
- `EnsureRoot`: lib.rs:1278-1292
- `RawOrigin alias`: lib.rs:971-972
- `Phase enum`: lib.rs:1188-1197
- `EventRecord struct`: lib.rs:1206-1215
- `LastRuntimeUpgradeInfo`: lib.rs:1253-1269
- `CodeUpgradeAuthorization`: lib.rs:263-273
- `CanSetCodeResult`: lib.rs:1537-1563
- `Provider/Consumer/SelfSufficient`: lib.rs:2508-2543
- `StoredMap impl`: lib.rs:2563-2583
- `ChainContext`: lib.rs:2599-2613
- `pallet_prelude types`: lib.rs:2682-2706

**limits.rs:**
- `BlockLength struct`: limits.rs:38-51
- `BlockWeights struct`: limits.rs:268-275
- `WeightsPerClass struct`: limits.rs:167-191
- `DEFAULT_NORMAL_RATIO`: limits.rs:164 (`Perbill::from_percent(75)`)
- `with_sensible_defaults()`: limits.rs:392-405
- `BlockWeightsBuilder`: limits.rs:432-508

**extensions:**
- `CheckWeight::calculate_consumed_weight()`: check_weight.rs:173-237
- `CheckWeight::do_validate()`: check_weight.rs:114-126
- `CheckWeight::do_prepare()`: check_weight.rs:131-145
- `CheckNonce::validate_nonce_for_account()`: check_nonce.rs:70-91
- `CheckNonce::prepare_nonce_for_account()`: check_nonce.rs:94-105
- `CheckMortality::implicit()`: check_mortality.rs:64-72
- `AuthorizeCall::validate()`: authorize_call.rs:68-88
- `WeightReclaim::post_dispatch_details()`: weight_reclaim.rs:87-95

**migrations:**
- `migrate_from_single_u8_to_triple_ref_count`: migrations/mod.rs:71-85
- `migrate_from_dual_to_triple_ref_count`: migrations/mod.rs:106-120

**weights:**
- `set_code weight`: weights.rs:123-131 — `164_247_810_000 ps` (164ms!) + 67KB proof
- `remark weight`: weights.rs:90-98 — `1_586_000 + 10_920 * bytes`

---

## Ek: Önemli Tasarim Kararlari

### 1. Events Neden Box Kullanir?
```rust
pub(super) type Events<T: Config> =
    StorageValue<_, Vec<Box<EventRecord<T::RuntimeEvent, T::Hash>>>, ValueQuery>;
```
Event'ler büyük olabilir (RuntimeEvent tüm palletlerin event enum'unu içerir). Box ile heap'te saklanır, stack overflow önlenir.

### 2. BlockHash Twox64Concat, Account Blake2_128Concat

- `BlockHash`: Anahtar blok numarası (iç sistem verisi) → `Twox64Concat` yeterli (preimage attack yok)
- `Account`: Anahtar AccountId (dışarıdan gelir) → `Blake2_128Concat` şart (enumeration saldırısı önlenir)

### 3. EXTRINSIC_INDEX neden unhashed storage?

```rust
storage::unhashed::put(well_known_keys::EXTRINSIC_INDEX, &0u32);
```

Well-known keys (`:extrinsic_index`) Wasm runtime ile native client arasında paylaşılan özel alanlar. Pallet storage prefix'i olmadan direk yazılır.

### 4. Neden hash69?

```rust
fn hash69<T: AsMut<[u8]> + Default>() -> T {
    let mut h = T::default();
    h.as_mut().iter_mut().for_each(|byte| *byte = 69);
    h
}
```

Genesis'te parent hash olarak kullanılır. `69 = 0x45` — keyfi ama tutarlı bir sentinel değer. `Default::default()` (sıfır) ile karışmasın.

### 5. ValidateUnsigned için apply_authorized_upgrade max priority

Governance tarafından authorize edilmiş runtime upgrade'in herhangi bir node tarafından gönderilebilmesi şart. `u64::max_value()` priority ile tx pool'a önce girer, hiç kimse bloke edemez.

### 6. Mortality ve Longevity Hesabı

```rust
let valid_till = self.0.death(current_u64);
longevity: valid_till.saturating_sub(current_u64),
```

Era'nın ölüm bloğu eksi mevcut blok = tx pool'da kaç blok kalacak. `Immortal` için `longevity = max`.

### 7. Weight Reclaim İdempotentliği

```rust
let accurate_reclaim = already_reclaimed.max(unspent);
```

`CheckWeight.post_dispatch_details` ve `WeightReclaim.post_dispatch_details` ikisi de `reclaim_weight()`'i çağırabilir. `max` semantiği sayesinde hangisi önce çalışırsa çalışsın, en yüksek reclaim uygulanır. Duplicate reduction yok.

---

*Agent 01 — pallet-system analizi tamamlandi. ~9720 satır kaynak kod kapsandi.*
*Dosya: /home/ali-haydar-karadas/Desktop/nova-shield-chain/research/analysis/polkadot/agent-01-system.md*
