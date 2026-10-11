# pallet-identity + pallet-proxy + pallet-multisig + pallet-utility — Tam Analiz
**Agent**: 19 | **Tarih**: 2026-07-22

---

## BÖLÜM 1: pallet-identity

### 1.1 IdentityInfo Yapısı (legacy.rs)

`legacy.rs` içinde tanımlı `IdentityInfo<FieldLimit>` yapısı:

```rust
pub struct IdentityInfo<FieldLimit: Get<u32>> {
    pub additional: BoundedVec<(Data, Data), FieldLimit>,  // Ekstra alanlar
    pub display:          Data,   // Görünen ad (UTF-8)
    pub legal:            Data,   // Yasal ad (UTF-8)
    pub web:              Data,   // Web sitesi, https:// prepend edilir
    pub riot:             Data,   // Matrix/Riot kullanıcı adı
    pub email:            Data,   // E-posta (UTF-8)
    pub pgp_fingerprint:  Option<[u8; 20]>,  // PGP parmak izi (opsiyonel)
    pub image:            Data,   // Logo/fotoğraf
    pub twitter:          Data,   // Twitter kullanıcı adı (@'siz)
}
```
- **Kaynak**: `identity/src/legacy.rs:81-125`
- `additional` alanı `FieldLimit` ile sınırlı key-value çiftleri tutar (test config'de `MaxAdditionalFields = 2`)
- `pgp_fingerprint` 20 byte sabit boyut, diğerleri `Data` enum'u

`Data` enum'u (`types.rs:40-57`):
- `Data::None` — veri yok
- `Data::Raw(BoundedVec<u8, ConstU32<32>>)` — maks 32 byte ham veri
- `Data::BlakeTwo256([u8; 32])` — hash referansı
- `Data::Sha256([u8; 32])`, `Data::Keccak256([u8; 32])`, `Data::ShaThree256([u8; 32])` — hash türleri

`IdentityField` bitflags (`legacy.rs:35-44`):
```rust
#[bitflags] #[repr(u64)]
pub enum IdentityField {
    Display, Legal, Web, Riot, Email, PgpFingerprint, Image, Twitter
}
```
Toplam 8 alan, `u64` bitflag olarak temsil edilir.

### 1.2 Registrar Sistemi

**Registrar ekleme** (`lib.rs:494-519`):
- `add_registrar`: `T::RegistrarOrigin` gerektirir (genellikle Root veya Council)
- Registrar başlangıç fee'si `Zero`, fields `Default`
- `BoundedVec<Option<RegistrarInfo>, T::MaxRegistrars>` şeklinde saklanır

**Registrar kaldırma** (`lib.rs:539-558`):
- Tombstone (None) bırakır, slot serbest BIRAKILMAZ (index stabil kalır)
- `RegistrarIndex` eski yargıları geçersiz kılmaz
- Bekleyen `request_judgement` depoları iade edilmez

**Yargı talep etme** (`request_judgement`, `lib.rs:731-771`):
1. Registrar var mı kontrol et, `max_fee >= registrar.fee` kontrol
2. Mevcut sticky yargıyı değiştirmeye çalışırsa `StickyJudgement` hatası
3. `Judgement::FeePaid(fee)` olarak kaydedilir
4. `T::Currency::reserve(&sender, registrar.fee)` ile miktar tutulur

**Yargı sağlama** (`provide_judgement`, `lib.rs:928-976`):
- Sadece registrar hesabı çağırabilir
- `judgement.has_deposit()` == false olmalı (FeePaid doğrudan sağlanamaz)
- `T::Hashing::hash_of(&id.info) != identity` ise `JudgementForDifferentIdentity` hatası
- Mevcut FeePaid yargısı varsa, `repatriate_reserved` ile registrar'a ücret aktarılır

**Judgement enum** (`types.rs:202-223`):
```rust
pub enum Judgement<Balance> {
    Unknown,          // Varsayılan, görüş yok
    FeePaid(Balance), // Ücret tutuldu, bekleniyor — STICKY
    Reasonable,       // Makul doğrulama
    KnownGood,        // Tam attestation
    OutOfDate,        // Eskimiş veri (kimlik güncellenerek kaldırılabilir)
    LowQuality,       // Düşük kalite (güncellenerek kaldırılabilir)
    Erroneous,        // Hatalı/kötü niyetli — STICKY (sadece registrar kaldırabilir)
}
```
- `is_sticky()`: `FeePaid` veya `Erroneous` — `lib.rs:237-239`
- `has_deposit()`: sadece `FeePaid` — `lib.rs:230-232`
- `Erroneous` en güçlü olumsuz yargı; sadece registrar değiştirebilir

### 1.3 Sub-Account Sistemi

**add_sub** (`lib.rs:1027-1057`):
- Gönderen kimliği kayıtlı olmalı
- Alt hesap başka bir kimliğe bağlı olmamalı
- `T::SubAccountDeposit::get()` her ekleme için tutulur
- `SubsOf` ve `SuperOf` storage'larını günceller

**rename_sub** (`lib.rs:1063-1078`):
- Gönderen, sub'ın sahibi olmalı
- Sadece `SuperOf` tablosunu günceller (deposit değişmez)

**remove_sub** (`lib.rs:1088-1105`):
- Sahibi tarafından çağrılır
- `T::SubAccountDeposit::get()` miktarı iade edilir (min ile sınırlı)
- `SuperOf` kaldırılır

**quit_sub** (`lib.rs:1118-1135`):
- Sub hesap kendi çıkar
- Depozito SUB hesaba aktarılır (sahibine değil!)
- `repatriate_reserved` kullanılır

**set_subs** (`lib.rs:619-677`):
- Toplu sub güncelleme
- Tüm eskiler silinir, yenileri eklenir
- Deposit: `SubAccountDeposit * new_sub_count`

### 1.4 Deposit Formülleri

**Kimlik depozitosu** (`lib.rs:1490-1494`):
```rust
fn calculate_identity_deposit(info: &T::IdentityInformation) -> BalanceOf<T> {
    let bytes = info.encoded_size() as u32;
    let byte_deposit = T::ByteDeposit::get().saturating_mul(BalanceOf::<T>::from(bytes));
    T::BasicDeposit::get().saturating_add(byte_deposit)
}
```
**Formul**: `identity_deposit = BasicDeposit + ByteDeposit * encoded_size_bytes`

**Sub-account depozitosu** (`lib.rs:1461-1463`):
```rust
fn subs_deposit(subs: u32) -> BalanceOf<T> {
    T::SubAccountDeposit::get().saturating_mul(BalanceOf::<T>::from(subs))
}
```
**Formul**: `subs_deposit = SubAccountDeposit * N` (N = sub sayısı)

**Test config değerleri** (`tests.rs:78-82`):
- `BasicDeposit = 100`
- `ByteDeposit = 10`
- `SubAccountDeposit = 100`
- `UsernameDeposit = 10`

**Toplam deposit** (`types.rs:290-296`):
```rust
pub(crate) fn total_deposit(&self) -> Balance {
    self.deposit +
        self.judgements.iter()
            .map(|(_, j)| if let Judgement::FeePaid(fee) = j { *fee } else { Zero::zero() })
            .fold(Zero::zero(), |a, i| a + i)
}
```
`total = identity_deposit + sum(FeePaid fees)` — yargı ücretleri kimlik deposite eklenir

### 1.5 Sabitler

| Sabit | Tip | Kaynak |
|-------|-----|--------|
| `MaxRegistrars` | `Get<u32>` | `lib.rs:211` |
| `MaxSubAccounts` | `Get<u32>` | `lib.rs:203` |
| `BasicDeposit` | `Get<BalanceOf<T>>` | `lib.rs:184` |
| `ByteDeposit` | `Get<BalanceOf<T>>` | `lib.rs:188` |
| `SubAccountDeposit` | `Get<BalanceOf<T>>` | `lib.rs:199` |
| `UsernameDeposit` | `Get<BalanceOf<T>>` | `lib.rs:193` |
| `PendingUsernameExpiration` | `Get<BlockNumberFor<T>>` | `lib.rs:235` |
| `UsernameGracePeriod` | `Get<BlockNumberFor<T>>` | `lib.rs:240` |
| `MaxSuffixLength` | `Get<u32>` | `lib.rs:244` |
| `MaxUsernameLength` | `Get<u32>` | `lib.rs:248` |

### 1.6 Username Mekanizması

Username sistemi kimlik sisteminden BAĞIMSIZDIR (`lib.rs:43-64` döküman):
- Kimlik olmadan username alınabilir
- Username olmadan kimlik kaydedilebilir

**Username format**: `"kullaniciadi.suffix"` (küçük harf, alfanumerik, nokta separator)
- `lib.rs:1515-1520`: `[a-z0-9]+.[suffix]`

**Username sağlayıcıları** (`types.rs:347-366`):
```rust
pub enum Provider<Balance> {
    Allocation,           // Governance'dan tahsis
    AuthorityDeposit(Balance), // Otorite depozit koyar
    System,               // Sistem tarafından atanan (kaldırılamaz)
}
```

**Pending mekanizması**: Username pre-signed veya queue'ya alınır. Queue'daki username'ler `PendingUsernameExpiration` geçtikten sonra kaldırılabilir.

**Grace period**: `unbind_username` çağrıldıktan `UsernameGracePeriod` blok sonra silinebilir.

**set_identity yaparken**: non-sticky yargılar silinir (sticky: FeePaid, Erroneous) — `lib.rs:581`

---

## BÖLÜM 2: pallet-proxy

### 2.1 ProxyType Enum

Pallet'te generic `ProxyType` bulunur — implementasyon çalışma zamanına bırakılmıştır:
```rust
type ProxyType: Parameter + Member + Ord + PartialOrd
    + frame::traits::InstanceFilter<<Self as Config>::RuntimeCall>
    + Default + MaxEncodedLen;
```
- `lib.rs:152-158`
- `Default::default()` en geniş izin veren tip olmalı (IMPORTANT notu var)
- `InstanceFilter::filter()` çağrıyla belirli call'lara izin verir/reddeder
- Polkadot'ta `ProxyType::Any`, `NonTransfer`, `Governance`, `Staking`, `IdentityJudgement`, `CancelProxy`, `Auction` olarak örneklenir

### 2.2 Delay Mekanizması

**Delay = 0**: Doğrudan `proxy` çağrısıyla yürütülür, announce gerekmez.

**Delay > 0**: 2 adım gerekir:
1. `announce(real, call_hash)` — `lib.rs:434-469`
   - Proxy, çağrının hash'ini duyurur
   - `Announcement { real, call_hash, height: current_block }` kaydedilir
   - `AnnouncementDepositBase + AnnouncementDepositFactor * count` tutulur
2. `proxy_announced(delegate, real, force_proxy_type, call)` — `lib.rs:541-574`
   - `now - announcement.height >= def.delay` kontrolü: `lib.rs:567`
   - Eşleşen duyuru kaldırılır, call yürütülür

**Gerçek hesap reddedebilir**: `reject_announcement(delegate, call_hash)` — `lib.rs:516-528`
- Kötü niyetli proxy'nin çağrısını engelle

### 2.3 Proxy Deposit Formülü

```rust
pub fn deposit(num_proxies: u32) -> BalanceOf<T> {
    if num_proxies == 0 {
        Zero::zero()
    } else {
        T::ProxyDepositBase::get() + T::ProxyDepositFactor::get() * num_proxies.into()
    }
}
```
- **Kaynak**: `lib.rs:927-933`
- **Formul**: `deposit = ProxyDepositBase + ProxyDepositFactor * N` (N proxy sayısı, N=0 ise sıfır)

**Announcement deposit** (`lib.rs:942-943`):
```rust
let new_deposit = if len == 0 { BalanceOf::<T>::zero() } else { base + factor * (len as u32).into() };
```
- **Formul**: `announcement_deposit = AnnouncementDepositBase + AnnouncementDepositFactor * count`

**Config parametreleri** (`lib.rs:161-201`):
- `ProxyDepositBase`: AccountId + Balance boyutu için temel depo
- `ProxyDepositFactor`: Proxy başına `32 + proxy_type.encode().len()` byte için
- `AnnouncementDepositBase`: Balance boyutu (16 byte) için
- `AnnouncementDepositFactor`: AccountId + Hash + BlockNumber (~68 byte) için

### 2.4 Pure (Anonymous) Proxy

**Oluşturma** (`lib.rs:338-371`, `create_pure`):
```rust
let pure = Self::pure_account(&who, &proxy_type, index, None);
```

**Adres türetme** (`lib.rs:826-843`):
```rust
pub fn pure_account(who, proxy_type, index, maybe_when) -> T::AccountId {
    let (height, ext_index) = maybe_when.unwrap_or_else(|| (
        T::BlockNumberProvider::current_block_number(),
        frame_system::Pallet::<T>::extrinsic_index().unwrap_or_default(),
    ));
    let entropy = (b"modlpy/proxy____", who, height, ext_index, proxy_type, index)
        .using_encoded(blake2_256);
    Decode::decode(&mut TrailingZeroInput::new(entropy.as_ref())).expect(...)
}
```
- **Prefix**: `b"modlpy/proxy____"` (16 byte)
- **Giriş**: `(prefix, spawner, block_height, extrinsic_index, proxy_type, disambiguation_index)`
- **Hash**: `blake2_256`

**Güvenlik Notları**:
- Pure proxy hesabın private key'i YOKTUR — sadece proxy ilişkisiyle kontrol edilir
- `Duplicate` hatası: Aynı parametrelerle aynı blok/extrinsic'de iki kez çağrılamaz
- `kill_pure` sadece spawner tarafından çağrılabilir (hesap kendini imha edebilir)
- `remove_proxies` çağrılırsa pure proxy'ye erişim KALICI OLARAK KAYBOLUR (`lib.rs:310-312`)
- Proxy sadece kendi izninden daha geniş proxy ekleyemez: `lib.rs:1008-1013`

**kill_pure** (`lib.rs:391-416`):
- Spawner + orijinal parametreler kullanılarak adres yeniden türetilir
- `proxy == who` kontrolü: hesap kendisi olmalı
- Deposit spawner'a iade edilir (pure hesaptan değil, spawner'ın rezervinden)

### 2.5 Sabitler

| Sabit | Tip | Kaynak |
|-------|-----|--------|
| `ProxyDepositBase` | `Get<BalanceOf<T>>` | `lib.rs:165` |
| `ProxyDepositFactor` | `Get<BalanceOf<T>>` | `lib.rs:173` |
| `MaxProxies` | `Get<u32>` | `lib.rs:177` |
| `MaxPending` | `Get<u32>` | `lib.rs:184` |
| `AnnouncementDepositBase` | `Get<BalanceOf<T>>` | `lib.rs:194` |
| `AnnouncementDepositFactor` | `Get<BalanceOf<T>>` | `lib.rs:201` |

**Storage Yapıları**:
- `Proxies`: `StorageMap<AccountId, (BoundedVec<ProxyDefinition, MaxProxies>, Balance)>` — `lib.rs:750-762`
- `Announcements`: `StorageMap<AccountId, (BoundedVec<Announcement, MaxPending>, Balance)>` — `lib.rs:765-775`

### 2.6 Call Filtering (do_proxy)

`do_proxy` (`lib.rs:994-1026`):
- `RawOrigin::Signed(real)` origin oluşturulur
- `add_filter` ile kısıtlamalar eklenir:
  - Proxy kendi izninden daha geniş proxy EKLEYEMEZ
  - Proxy kendi izninden daha geniş proxy KALDIRMAZ
  - Full izin olmayan proxy `remove_proxies` veya `kill_pure` çağıramaz
  - Sonra `def.proxy_type.filter(c)` uygulanır

**poke_deposit** (`lib.rs:585-673`): Hem proxy hem announcement depositlerini mevcut değerlere göre günceller. Değişim varsa ücret alınmaz (`Pays::No`).

---

## BÖLÜM 3: pallet-multisig

### 3.1 as_multi Akışı

**Ana fonksiyon**: `operate()` (`lib.rs:651-778`) — hem `as_multi` hem `approve_as_multi` tarafından çağrılır.

**Akış (ilk çağrı)**:
1. `threshold >= 2` kontrolü — `lib.rs:659`
2. `other_signatories.len() < max_sigs` — diğerleri gönderenden az
3. `ensure_sorted_and_insert`: signatories sıralı olmalı, gönderen dahil edilir — `lib.rs:664`
4. `multi_account_id(&signatories, threshold)` ile deterministik hesap türetilir
5. `maybe_timepoint.is_none()` kontrolü (ilk çağrıda None olmalı) — `lib.rs:751`
6. `deposit = Self::deposit(threshold)` hesaplanır — `lib.rs:754`
7. `T::Currency::reserve(&who, deposit)` — `lib.rs:756`
8. `Multisigs` storage'a kaydedilir — `lib.rs:761-770`

**Akış (sonraki çağrılar)**:
1. `maybe_timepoint.ok_or(Error::<T>::NoTimepoint)` — `lib.rs:680`
2. `m.when == timepoint` kontrolü — `lib.rs:681`
3. Approval sayısı threshold'a ulaştıysa ve call verisi mevcutsa YÜRÜTÜLÜR
4. **Önemli**: Depolama call yürütmeden önce temizlenir (reentrancy koruması) — `lib.rs:702`
5. `call.dispatch(RawOrigin::Signed(id).into())` — `lib.rs:705`

**as_multi_threshold_1** (`lib.rs:322-366`):
- Threshold = 1, stateless — direkt yürütür
- Deposit yok, storage güncelleme yok
- Multisig ID türetilir ve `RawOrigin::Signed(id)` kullanılır

**approve_as_multi** (`lib.rs:485-502`):
- Hash'i kaydeder, call'ı ASLA yürütmez
- Son approval için `as_multi` (tam call verisiyle) çağrılmalı

**cancel_as_multi** (`lib.rs:527-558`):
- Sadece başlangıç depositor'u iptal edebilir: `m.depositor == who` — `lib.rs:545`
- `timepoint == m.when` kontrolü — `lib.rs:544`
- Deposit iade edilir

### 3.2 Deposit Formülü

```rust
pub fn deposit(threshold: u16) -> BalanceOf<T> {
    T::DepositBase::get() + T::DepositFactor::get() * threshold.into()
}
```
- **Kaynak**: `lib.rs:813-815`
- **Formul**: `deposit = DepositBase + DepositFactor * threshold`
- DİKKAT: `threshold` üzerinden hesaplanır, signatories sayısı üzerinden değil!

Config açıklaması (`lib.rs:161-173`):
- `DepositBase`: `4 + sizeof((BlockNumber, Balance, AccountId))` byte + `32 + sizeof(AccountId)` byte key için
- `DepositFactor`: Her ek approval için 32 byte

### 3.3 Timepoint

```rust
pub struct Timepoint<BlockNumber> {
    pub height: BlockNumber,  // Blok yüksekliği
    pub index: u32,           // Extrinsic index
}
```
- **Kaynak**: `lib.rs:99-104`
- Multisig işlemini benzersiz olarak tanımlar

`timepoint()` fonksiyonu (`lib.rs:781-786`):
```rust
pub fn timepoint() -> Timepoint<BlockNumberFor<T>> {
    Timepoint {
        height: T::BlockNumberProvider::current_block_number(),
        index: <frame_system::Pallet<T>>::extrinsic_index().unwrap_or_default(),
    }
}
```

**Neden önemli**: Aynı signatories + threshold = aynı multisig ID. Timepoint olmadan hangisi olduğunu ayırt etmek imkansız.

### 3.4 Multisig Hesap ID'si

```rust
pub fn multi_account_id(who: &[T::AccountId], threshold: u16) -> T::AccountId {
    let entropy = (b"modlpy/utilisuba", who, threshold).using_encoded(blake2_256);
    Decode::decode(&mut TrailingZeroInput::new(entropy.as_ref())).expect(...)
}
```
- **Kaynak**: `lib.rs:645-649`
- **Prefix**: `b"modlpy/utilisuba"` (16 byte)
- **Giriş**: `(prefix, sorted_signatories[], threshold)`
- `who` sıralı olmalı (ensure_sorted_and_insert'ten gelir)

### 3.5 Sabitler

| Sabit | Tip | Kaynak |
|-------|-----|--------|
| `DepositBase` | `Get<BalanceOf<T>>` | `lib.rs:167` |
| `DepositFactor` | `Get<BalanceOf<T>>` | `lib.rs:173` |
| `MaxSignatories` | `Get<u32>` | `lib.rs:177` |
| STORAGE_VERSION | `StorageVersion::new(1)` | `lib.rs:208` |

**Storage Yapısı**:
```rust
pub type Multisigs<T: Config> = StorageDoubleMap<
    _, Twox64Concat, T::AccountId,
    Blake2_128Concat, [u8; 32],
    Multisig<BlockNumberFor<T>, BalanceOf<T>, T::AccountId, T::MaxSignatories>,
>;
```
- Key1: Multisig hesap ID'si
- Key2: Call hash ([u8; 32])

**poke_deposit** (`lib.rs:573-636`): Sadece orijinal depositor çağırabilir. Değişim varsa ücret alınmaz.

### 3.6 Hata Durumları

| Hata | Açıklama |
|------|---------|
| `MinimumThreshold` | threshold < 2 |
| `AlreadyApproved` | Zaten approve edilmiş, call verisi sunulmamış |
| `NoApprovalsNeeded` | Threshold zaten karşılandı |
| `TooFewSignatories` | other_signatories boş |
| `TooManySignatories` | len >= MaxSignatories |
| `SignatoriesOutOfOrder` | Sıralı olmayan signatories |
| `SenderInSignatories` | Gönderen diğer listesinde |
| `NotFound` | Multisig storage'da yok |
| `NotOwner` | Cancel için orijinal depositor değil |
| `NoTimepoint` | Devam ederken timepoint yok |
| `WrongTimepoint` | Yanlış timepoint |
| `UnexpectedTimepoint` | İlk çağrıda timepoint verildi |
| `MaxWeightTooLow` | max_weight yetersiz |

---

## BÖLÜM 4: pallet-utility

### 4.1 batch vs batch_all Farkı

**`batch`** (`lib.rs:199-239`):
- `Ok` döner hep (başarısız olsa bile)
- İlk başarısız call'da durur (`BatchInterrupted` event)
- Başarısız call'a kadar yapılan değişiklikler KALICI
- Önceki call'ların etkisi GERİ ALINAMAZ
- Event: `BatchInterrupted { index, error }` veya `BatchCompleted`

**`batch_all`** (`lib.rs:308-353`):
- ATOMIK: herhangi bir call başarısız olursa tüm transaction GERİ ALINIR
- Hata propagation: `result.map_err(...)?;` ile — `lib.rs:341-347`
- Başarısız call'da `DispatchError` döner, tüm state rollback
- Nested `batch_all` yasak: `!matches!(c.is_sub_type(), Some(Call::batch_all { .. }))` — `lib.rs:334`
- Event: `BatchCompleted` veya hiçbir şey (rollback)

**`force_batch`** (`lib.rs:404-445`):
- Hataları yok sayar, devam eder
- `has_error: bool` flag tutar
- `BatchCompletedWithErrors` veya `BatchCompleted` event
- Her item için `ItemCompleted` veya `ItemFailed { error }` event

**Root davranışı** (üç fonksiyon için):
- Root origin: `dispatch_bypass_filter` çağrılır (BaseCallFilter atlanır)
- Normal origin: `dispatch` çağrılır (filtreler uygulanır)

**batched_calls_limit** (`lib.rs:143-152`):
```rust
fn batched_calls_limit() -> u32 {
    let allocator_limit = sp_core::MAX_POSSIBLE_ALLOCATION;
    let call_size = (size_of::<RuntimeCall>() as u32).div_ceil(CALL_ALIGN) * CALL_ALIGN;
    let margin_factor = 3;
    allocator_limit / margin_factor / call_size
}
```
- `CALL_ALIGN = 1024` (`lib.rs:138`)
- Call enum boyutu <= 1KB olmalı (integrity_test: `lib.rs:158-164`)

### 4.2 as_derivative

**Adres türetme** (`lib.rs:640-643`):
```rust
pub fn derivative_account_id<AccountId: Encode + Decode>(who: AccountId, index: u16) -> AccountId {
    let entropy = (b"modlpy/utilisuba", who, index).using_encoded(blake2_256);
    Decode::decode(&mut TrailingZeroInput::new(entropy.as_ref())).expect(...)
}
```
- **Prefix**: `b"modlpy/utilisuba"` (16 byte)
- **Formul**: `blake2_256("modlpy/utilisuba" ++ who ++ index_as_u16)`
- Her hesabın `2^16 = 65536` farklı türev hesabı vardır
- MEMORY.md'deki doğrulanmış değer: `derivative_account_id = blake2_256("modlpy/utilisuba" || who || idx)`

**Çağrı** (`lib.rs:265-287`):
```rust
pub fn as_derivative(origin, index: u16, call: Box<RuntimeCall>) -> DispatchResultWithPostInfo {
    let who = ensure_signed(origin.clone())?;
    let pseudonym = derivative_account_id(who, index);
    origin.set_caller_from(frame_system::RawOrigin::Signed(pseudonym));
    // ... dispatch with modified origin
}
```
- Origin filtreleri GEÇER (proxy filtreleri dahil)
- `set_caller_from` ile origin'in caller değiştirilir ama filtreler korunur

**Deprecated**: `Pallet::derivative_account_id` method'u deprecated (August 2025 sonrası kaldırılacak) — `lib.rs:624-629`

**IndexedUtilityPalletId** (`lib.rs:614-621`):
```rust
struct IndexedUtilityPalletId(u16);
impl TypeId for IndexedUtilityPalletId { const TYPE_ID: [u8; 4] = *b"suba"; }
```

### 4.3 dispatch_as

```rust
pub fn dispatch_as(origin, as_origin: Box<T::PalletsOrigin>, call) -> DispatchResult {
    ensure_root(origin)?;  // ROOT ZORUNLU
    let res = call.dispatch_bypass_filter((*as_origin).into());
    Self::deposit_event(Event::DispatchedAs { result: ... });
    Ok(())
}
```
- **Kaynak**: `lib.rs:370-383`
- **Root zorunlu** — güvenlik kritik
- `dispatch_bypass_filter` kullanılır — tüm filtreler atlanır
- Call'ın kendi hatası döndürülmez, her zaman `Ok(())` (hata event'te)

**dispatch_as_fallible** (`lib.rs:573-585`, call_index 7):
- `dispatch_as` ile aynı ama inner call hatasını propagate eder
- `call.dispatch_bypass_filter(...).map_err(|e| e.error)?;`

### 4.4 with_weight

```rust
pub fn with_weight(origin, call, weight: Weight) -> DispatchResult {
    ensure_root(origin)?;
    let _ = weight; // weight kontrolü yapılmaz!
    let res = call.dispatch_bypass_filter(frame_system::RawOrigin::Root.into());
    res.map(|_| ()).map_err(|e| e.error)
}
```
- **Kaynak**: `lib.rs:453-465`
- Root tarafından call'a custom weight atanır
- Weight witness kontrolü yok (`let _ = weight;`)
- Dispatch her zaman Root origin ile yapılır

### 4.5 if_else (Yeni)

```rust
pub fn if_else(origin, main, fallback) -> DispatchResultWithPostInfo {
    // main çalıştır
    // main başarılı -> IfElseMainSuccess event, Ok
    // main başarısız -> fallback çalıştır
    // fallback başarılı -> IfElseFallbackCalled { main_error } event, Ok
    // ikisi de başarısız -> fallback hatası ile Err
}
```
- **Kaynak**: `lib.rs:501-557`
- `call_index(6)` — yeni eklenen özellik
- İkisi de başarısız olursa FALLBACK hatası döner (main hatası değil)

### 4.6 Nested Call Derinliği

Pallet doğrudan bir derinlik limiti uygulamaz. Limitler:
1. `batched_calls_limit`: Bir batch'te maksimum call sayısı
2. Recursive `batch_all` yasak: Filtre ile engellenir
3. Call enum boyutu <= 1KB (integrity_test)
4. Stack derinliği Wasm/native stack'e göre sınırlı

---

## Kritik Bulgular Özeti

1. **Identity deposit formülü**: `BasicDeposit + ByteDeposit * encoded_size_bytes` — `lib.rs:1490-1494`
2. **Sub deposit formülü**: `SubAccountDeposit * N` (N = sub sayısı) — `lib.rs:1461-1463`
3. **Judgement sticky kuralı**: `FeePaid` ve `Erroneous` sticky — set_identity güncellemesinde non-sticky yargılar silinir, sticky kalır — `lib.rs:581`
4. **Registrar tombstone**: Kaldırılan registrar slot serbest bırakılmaz, `None` bırakılır; index stabil — `lib.rs:527-530`
5. **Proxy deposit formülü**: `ProxyDepositBase + ProxyDepositFactor * N` (N=0 ise sıfır) — `lib.rs:927-933`
6. **Pure proxy adresi**: `blake2_256(b"modlpy/proxy____" || who || height || ext_index || proxy_type || index)` — `lib.rs:839`
7. **remove_proxies uyarısı**: Pure proxy üzerinde çağrılırsa hesaba erişim sonsuza dek kaybolur — `lib.rs:310-312`
8. **Proxy escalation koruması**: Proxy kendi izninden daha geniş proxy ekleyemez — `lib.rs:1008-1013`
9. **Multisig deposit formülü**: `DepositBase + DepositFactor * threshold` (signatories sayısı değil!) — `lib.rs:813-815`
10. **Multisig reentrancy koruması**: Depolama call yürütmeden önce temizlenir — `lib.rs:702-703`
11. **Multisig hesap ID**: `blake2_256(b"modlpy/utilisuba" || sorted_signatories || threshold)` — `lib.rs:646`
12. **batch vs batch_all**: batch kısmi başarı kabul eder, batch_all atomiktir — `lib.rs:199-353`
13. **as_derivative adresi**: `blake2_256(b"modlpy/utilisuba" || who || index_u16)` — `lib.rs:641`
14. **batch_all nested yasak**: Nested batch_all filtre ile engellenir — `lib.rs:334`
15. **Username + identity bağımsız**: Username ve kimlik ayrı sistemler; biri olmadan diğeri kullanılabilir — `lib.rs:43-64`

---

## Nova Shield Chain Notları

### Kimlik Sistemi (pallet-identity)

**Tavsiye 1 — Pluggable IdentityInfo**: Polkadot'un `IdentityInformationProvider` trait yaklaşımı mükemmel. Nova Shield Chain için özel alanlar (örn. KYC tier, SBT referans) eklemek için aynı trait pattern kullanılabilir. Legacy `IdentityInfo` yerine custom struct implement edilebilir.

**Tavsiye 2 — Deposit kalibrasyonu**: `ByteDeposit` modeli encoded boyuta göre ölçeklenir — verimli. Nova Shield'da yeterli büyüklükte `BasicDeposit` belirle ki kimlik spam'ı önlensin.

**Tavsiye 3 — Registrar merkezi değil**: Federated model birden fazla güvenilir registrar destekler. Merkezi bir KYC yerine bu model tercih edilmeli; yargı kalitesi rekabetçi olur.

**Tavsiye 4 — sticky judgement dikkat**: `Erroneous` yargısı kullanıcı tarafından kaldırılamaz — güçlü bir ceza mekanizması. Kullanım politikası carefully belgelenmelidir.

**Tavsiye 5 — Username mekanizması**: Allocation-based (governance) + deposit-based (authority ödüyor) iki model var. Nova Shield için authority-deposit modeli daha ölçeklenebilir (governance bant genişliği tüketmez).

### Proxy Sistemi (pallet-proxy)

**Tavsiye 6 — ProxyType hiyerarşisi**: `InstanceFilter::is_superset` ile hiyerarşik proxy tipler tasarlanmalı. Öneri: `Any > Staking > Governance > Transfer > ReadOnly`.

**Tavsiye 7 — Pure proxy kullanım alanı**: Pure proxy = keyless hesaplar. DeFi protocol'lar için ideal (treasury, pool, vault). Nova Shield DEX havuzları için değerlendirilebilir.

**Tavsiye 8 — Delay mekanizması**: Büyük işlemler için delay + announce zorunlu yapılabilir (governance düzeyinde proxy). Bu kullanıcıya veto fırsatı verir.

**Tavsiye 9 — poke_deposit**: Ağ parametreleri değiştikten sonra kullanıcılar depositlerini güncelleyebilir. Ücret iadesi (Pays::No) teşvik edici.

### Multisig Sistemi (pallet-multisig)

**Tavsiye 10 — as_multi vs approve_as_multi**: Intermediate approver'lar için `approve_as_multi` kullanılmalı (sadece hash, daha verimli). Son approver full call verisiyle `as_multi` çağırır.

**Tavsiye 11 — Threshold depositu**: `DepositFactor * threshold` formülü — threshold yükseldikçe deposit artar. Bu threshold seçimini ekonomik hale getirir.

**Tavsiye 12 — Reentrancy koruması zorunlu**: Storage-then-dispatch pattern kritik. Nova Shield'daki custom pallet'larda aynı pattern mutlaka uygulanmalı.

**Tavsiye 13 — Multisig + proxy kombinasyonu**: `as_multi_threshold_1` ile threshold=1 multisig, proxy benzeri ama farklı origin/filtering kuralları. Durum bağımlı tasarımda ikisi birlikte değerlendirin.

### Utility Sistemi (pallet-utility)

**Tavsiye 14 — batch_all atomiklik varsayımı**: İşlem atomikliği gereken yerlerde `batch_all` kullan. `batch` kısmi başarıyı kabul ettiğinden dikkatli kullanılmalı.

**Tavsiye 15 — dispatch_as güvenliği**: Root-only olması kritik. Nova Shield'da governance'ın `dispatch_as` kullanarak herhangi bir origin'den işlem yapabilmesi güçlü ama tehlikeli; yetki kontrolü katı tutulmalı.

**Tavsiye 16 — with_weight**: Runtime upgrade'lerde ağır operasyonları manuel weight ile çalıştırmak için kullanılabilir. Sadece root çağırabilir, weight doğrulanmaz.

**Tavsiye 17 — as_derivative key management**: Her hesabın 65536 türev adresi (index 0-65535) var. Staking controller, farklı validator setleri gibi senaryolar için faydalı. Nova Shield'da key yönetimi standartlaştırılmalı.

---

## Ek: Dosya Analiz Kapsamı

| Dosya | Boyut (satır tahmini) | Durum |
|-------|----------------------|-------|
| `identity/src/lib.rs` | ~1600 | Okundu |
| `identity/src/types.rs` | 440 | Okundu |
| `identity/src/legacy.rs` | 205 | Okundu |
| `identity/src/tests.rs` | ilk 100 | Okundu |
| `identity/src/migration.rs` | — | Mevcut (migration V1->V2) |
| `proxy/src/lib.rs` | 1044 | Okundu (tam) |
| `multisig/src/lib.rs` | 827 | Okundu (tam) |
| `utility/src/lib.rs` | 644 | Okundu (tam) |
