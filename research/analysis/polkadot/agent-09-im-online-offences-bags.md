# pallet-im-online + offences + authority-discovery + bags-list — Agent 09

## Analiz Kapsamı

- **pallet-im-online**: `/substrate/frame/im-online/src/` — 6 dosya tam analiz
- **pallet-offences**: `/substrate/frame/offences/src/` + `benchmarking/` — 8 dosya tam analiz
- **pallet-authority-discovery**: `/substrate/frame/authority-discovery/src/lib.rs` — 1 dosya tam analiz
- **pallet-bags-list**: `/substrate/frame/bags-list/src/` — 13 dosya tam analiz
- **Toplam**: ~4.500 satır Rust kodu incelendi

---

## KISIM 1: pallet-im-online

### 1.1 Genel Bakış ve Amaç

`pallet-im-online`, Substrate validator node'larının aktif olup olmadığını tespit etmek için tasarlanmış bir pallet'tir. Temel mekanizma son derece basit: Her validator, her session boyunca bir "heartbeat" (kalp atışı) işlemi gönderir. Session sonunda heartbeat göndermeyen ve o session'da blok üretmemiş validator'lar "offline" sayılır ve ceza mekanizmaları tetiklenir.

Pallet iki alternatif çevrimiçilik kanıtı kabul eder:
1. **Heartbeat Mesajı**: Off-chain worker tarafından imzalanıp gönderilen özel bir işlem
2. **Blok Üretimi**: Validator o session'da en az bir blok ürettiyse, heartbeat gönderme gerekmez

Bu iki kanıt yolunun birlikte kullanılması, gereksiz heartbeat mesajlarını azaltır: Çoğu aktif validator zaten blok üretiyor olacaktır.

### 1.2 Heartbeat Mekanizması — Derinlemesine Analiz

#### 1.2.1 Heartbeat Yapısı

```rust
// lib.rs:222-235
pub struct Heartbeat<BlockNumber>
where
    BlockNumber: PartialEq + Eq + Decode + Encode,
{
    pub block_number: BlockNumber,    // Mesajın oluşturulduğu blok
    pub session_index: SessionIndex,  // Hangi session için
    pub authority_index: AuthIndex,   // Validator'ın sıra numarası (u32)
    pub validators_len: u32,          // Mevcut validator sayısı (doğrulama için)
}
```

Heartbeat mesajı 4 alan içerir. `validators_len` alanı önemlidir: doğrulama sırasında on-chain'deki gerçek validator sayısıyla eşleşmesi zorunludur; aksi takdirde işlem `InvalidTransaction::Custom(INVALID_VALIDATORS_LEN)` hatasıyla reddedilir.

#### 1.2.2 HeartbeatStatus — Off-chain Lock Mekanizması

```rust
// lib.rs:163-189
struct HeartbeatStatus<BlockNumber> {
    pub session_index: SessionIndex,
    pub sent_at: BlockNumber,
}

const INCLUDE_THRESHOLD: u32 = 3;  // lib.rs:155

fn is_recent(&self, session_index: SessionIndex, now: BlockNumber) -> bool {
    self.session_index == session_index && self.sent_at + INCLUDE_THRESHOLD.into() > now
}
```

`HeartbeatStatus`, off-chain storage'da her authority için ayrı tutulur. `is_recent()` fonksiyonu şunu kontrol eder:
- Aynı session mı? (session değiştiyse heartbeat yeniden gönderilmeli)
- Gönderimden sonra 3'ten az blok geçmiş mi? Geçmişse bekle (INCLUDE_THRESHOLD=3)

Bu mekanizma, aynı session için birden fazla heartbeat gönderilmesini engeller. Eğer gönderim başarısız olursa (`sent_at = 0` olarak sıfırlanır), bir sonraki blokta yeniden denenir.

#### 1.2.3 Heartbeat Gönderimi — Zamanlama Algoritması

`send_heartbeats()` fonksiyonu (lib.rs:506-564) kritik bir zamanlama algoritması içerir:

```rust
const START_HEARTBEAT_RANDOM_PERIOD: Permill = Permill::from_percent(10);
const START_HEARTBEAT_FINAL_PERIOD: Permill = Permill::from_percent(80);

// Threshold hesabı (progress = session ilerlemesi 0.0-1.0 arası):
// threshold = progress^6 + 1/session_length
let residual = Permill::from_rational(1u32, session_length.saturated_into());
let threshold: Permill = progress.saturating_pow(6).saturating_add(residual);
```

**Üç farklı durum:**

**Durum 1 — Çok erken (< %10 session):** Hiçbir heartbeat gönderilmez. `TooEarly` hatası döner.

**Durum 2 — Erken dönem (%10-%80 session):** Rastgele seçim (`random_choice(progress)`) uygulanır. Olasılık formülü: `progress^6 + 1/session_length`. Session başında çok düşük, ilerledikçe polinom olarak artar. Bu, tüm validator'ların aynı anda heartbeat gönderip işlem patlaması yaratmasını önler.

**Durum 3 — Final dönem (>= %80 session):** Heartbeat koşulsuz olarak gönderilir. Bu, hiçbir validator'ın heartbeat göndermeyi kaçırmamasını garanti eder.

**Fallback:** Eğer `NextSessionRotation::estimate_current_session_progress()` `None` döndürürse (ör. session rotasyonu tahmin edilemiyorsa), `HeartbeatAfter` storage değeri kullanılır. Bu değer her yeni session'da `current_block + half_session` olarak set edilir.

**Test kanıtı (tests.rs):**
```
progress=0.05 → TooEarly (kesin)
progress=0.10, random=0.010 → başarılı  (eşik = 0.10^6 + 0.01 ≈ 0.01)
progress=0.10, random=0.011 → TooEarly  (eşiğin üstünde random sayı)
progress=0.40, random=0.014 → başarılı
progress=0.80+ → koşulsuz başarılı
```

#### 1.2.4 HeartbeatAfter Başlangıç Değeri

```rust
// lib.rs:770-771 (on_new_session içinde)
let block_number = <frame_system::Pallet<T>>::block_number();
let half_session = T::NextSessionRotation::average_session_length() / 2u32.into();
<HeartbeatAfter<T>>::put(block_number + half_session);
```

Her yeni session başladığında, `HeartbeatAfter` değeri `mevcut_blok + yarım_session` olarak ayarlanır. Fallback modunda bu, heartbeat'in session'ın ikinci yarısında gönderileceği anlamına gelir.

### 1.3 Offline Detection — Tam Analiz

#### 1.3.1 is_online() Mantığı

```rust
// lib.rs:473-490
pub fn is_online(authority_index: AuthIndex) -> bool {
    let current_validators = T::ValidatorSet::validators();
    if authority_index >= current_validators.len() as u32 {
        return false;
    }
    let authority = &current_validators[authority_index as usize];
    Self::is_online_aux(authority_index, authority)
}

fn is_online_aux(authority_index: AuthIndex, authority: &ValidatorId<T>) -> bool {
    let current_session = T::ValidatorSet::session_index();
    ReceivedHeartbeats::<T>::contains_key(current_session, authority_index) ||
        AuthoredBlocks::<T>::get(current_session, authority) != 0
}
```

Bir validator şu iki koşuldan birini sağlıyorsa "online" sayılır:
1. `ReceivedHeartbeats[current_session][authority_index]` = `true`
2. `AuthoredBlocks[current_session][authority]` > 0

#### 1.3.2 Offline Detection Tetikleyici — `on_before_session_ending()`

```rust
// lib.rs:785-820
fn on_before_session_ending() {
    let offenders = current_validators
        .into_iter()
        .enumerate()
        .filter(|(index, id)| !Self::is_online_aux(*index as u32, id))
        .filter_map(|(_, id)| {
            <T::ValidatorSet as ValidatorSetWithIdentification<T::AccountId>>::IdentificationOf::convert(id.clone())
                .map(|full_id| (id, full_id))
        })
        .collect::<Vec<IdentificationTuple<T>>>();

    // Storage temizliği
    ReceivedHeartbeats::<T>::remove_prefix(T::ValidatorSet::session_index(), None);
    AuthoredBlocks::<T>::remove_prefix(T::ValidatorSet::session_index(), None);

    if offenders.is_empty() {
        Self::deposit_event(Event::<T>::AllGood);
    } else {
        Self::deposit_event(Event::<T>::SomeOffline { offline: offenders.clone() });
        let offence = UnresponsivenessOffence { session_index, validator_set_count, offenders };
        if let Err(e) = T::ReportUnresponsiveness::report_offence(vec![], offence) {
            sp_runtime::print(e);
        }
    }
}
```

**Kilit Soru: Kaç heartbeat miss edilirse offline sayılır?**

Cevap: **Session başına 1 heartbeat** yeterlidir. Eğer bir validator tüm session boyunca **hiç heartbeat göndermedi VE hiç blok üretmedi**, o session'ın sonunda offline olarak raporlanır.

Bu "tek session" eşiği oldukça düşük görünse de gerçek hayatta şunları göz önünde bulundurmak gerekir:
- Çoğu validator zaten blok üretiyor ve heartbeat gerekmez
- Heartbeat gönderimi birden fazla kez denenir (başarısız olunca tekrar)
- Off-chain worker her blokta çalışır

Offline tespiti **session sonu** olayında (`on_before_session_ending`) gerçekleşir — session başında değil, sonunda. Bu, gecikmeli bir tespit anlamına gelir: Bir validator tüm session boyunca offline olabilir ve ancak session bittiğinde rapor edilir.

### 1.4 Storage Yapısı

```
HeartbeatAfter<T>: StorageValue<_, BlockNumberFor<T>, ValueQuery>
  → Session başında set edilir: mevcut_blok + yarım_session

Keys<T>: StorageValue<_, WeakBoundedVec<T::AuthorityId, T::MaxKeys>, ValueQuery>
  → ImOnline public key'leri (her session'da güncellenir)

ReceivedHeartbeats<T>: StorageDoubleMap<SessionIndex, AuthIndex, bool>
  → (session_idx, auth_idx) → true (heartbeat alındı)
  → Session sonunda temizlenir

AuthoredBlocks<T>: StorageDoubleMap<SessionIndex, ValidatorId<T>, u32, ValueQuery>
  → (session_idx, validator_id) → blok_sayısı
  → Session sonunda temizlenir
```

### 1.5 Transaction Doğrulama — authorize_heartbeat_call()

```rust
// lib.rs:691-737
fn authorize_heartbeat_call(
    _source: TransactionSource,
    heartbeat: &Heartbeat<BlockNumberFor<T>>,
    signature: &<T::AuthorityId as RuntimeAppPublic>::Signature,
) -> TransactionValidityWithRefund {
    // 1. Zaten online mu? → Stale
    if Pallet::<T>::is_online(heartbeat.authority_index) {
        return Err(InvalidTransaction::Stale.into());
    }

    // 2. Session index doğru mu? → Stale
    let current_session = T::ValidatorSet::session_index();
    if heartbeat.session_index != current_session {
        return Err(InvalidTransaction::Stale.into());
    }

    // 3. validators_len doğru mu? → Custom(INVALID_VALIDATORS_LEN)
    if keys.len() as u32 != heartbeat.validators_len {
        return Err(InvalidTransaction::Custom(INVALID_VALIDATORS_LEN).into());
    }

    // 4. Authority mevcut mu? → BadProof
    // 5. İmza geçerli mi? → BadProof (en pahalı kontrol, en son yapılır!)

    ValidTransaction::with_tag_prefix("ImOnline")
        .priority(T::UnsignedPriority::get())
        .and_provides((current_session, authority_id))
        .longevity(average_session_length / 2)
        .propagate(true)
        .build()
}
```

**Önemli tasarım kararları:**
- İmza doğrulama **en son** yapılır (en pahalı işlem)
- `and_provides((current_session, authority_id))` → Aynı authority'den aynı session için sadece bir işlem kabul edilir
- `longevity = session_length / 2` → İşlem mempool'da yarım session boyunca geçerli kalır

### 1.6 AuthorityId ve Kripto Altyapısı

```rust
// lib.rs:116-150
pub mod sr25519 {
    mod app_sr25519 {
        use sp_application_crypto::{app_crypto, key_types::IM_ONLINE, sr25519};
        app_crypto!(sr25519, IM_ONLINE);
    }
    pub type AuthorityPair = app_sr25519::Pair;
    pub type AuthoritySignature = app_sr25519::Signature;
    pub type AuthorityId = app_sr25519::Public;
}

pub mod ed25519 {  // alternatif ed25519 desteği de mevcut
    // ...
}
```

Varsayılan olarak SR25519 kullanılır. Özel key tipi: `IM_ONLINE` (sp_application_crypto::key_types). Ed25519 desteği de mevcut ama SR25519 yaygın kullanım.

### 1.7 Off-chain Lock Mekanizması — with_heartbeat_lock()

```rust
// lib.rs:631-675
fn with_heartbeat_lock<R>(authority_index: u32, session_index: SessionIndex, now: BlockNumberFor<T>, f: impl FnOnce() -> OffchainResult<T, R>) -> OffchainResult<T, R> {
    let key = {
        let mut key = DB_PREFIX.to_vec();  // b"parity/im-online-heartbeat/"
        key.extend(authority_index.encode());
        key
    };
    let storage = StorageValueRef::persistent(&key);
    let res = storage.mutate(|status| {
        match status {
            Ok(Some(status)) if status.is_recent(session_index, now) => {
                Err(OffchainErr::WaitingForInclusion(status.sent_at))
            },
            _ => Ok(HeartbeatStatus { session_index, sent_at: now }),
        }
    });
    // ...
    let res = f();
    if res.is_err() {
        new_status.sent_at = 0u32.into();  // Hata durumunda lock'u sıfırla
        storage.set(&new_status);
    }
    res
}
```

Her authority için ayrı bir off-chain storage key tutulur: `"parity/im-online-heartbeat/" + authority_index`. Bu atomic `mutate` işlemi, eş zamanlı off-chain worker çalışmalarında race condition'ı önler.

### 1.8 UnresponsivenessOffence Yapısı

```rust
// lib.rs:830-873
pub struct UnresponsivenessOffence<Offender> {
    pub session_index: SessionIndex,
    pub validator_set_count: u32,
    pub offenders: Vec<Offender>,
}

impl<Offender: Clone> Offence<Offender> for UnresponsivenessOffence<Offender> {
    const ID: Kind = *b"im-online:offlin";  // 16 byte sabit ID
    type Slot = SessionIndex;

    fn slash_fraction(&self, offenders: u32) -> Perbill {
        // min((3 * (k - (n/10 + 1))) / n, 1) * 0.07
        if let Some(threshold) = offenders.checked_sub(self.validator_set_count / 10 + 1) {
            let x = Perbill::from_rational(3 * threshold, self.validator_set_count);
            x.saturating_mul(Perbill::from_percent(7))
        } else {
            Perbill::default()  // 0
        }
    }
}
```

### 1.9 Sabitler — Kaynak Koddan Doğrulanmış

| Sabit | Değer | Konum |
|-------|-------|-------|
| `DB_PREFIX` | `b"parity/im-online-heartbeat/"` | lib.rs:152 |
| `INCLUDE_THRESHOLD` | `3` blok | lib.rs:155 |
| `START_HEARTBEAT_RANDOM_PERIOD` | `Permill::from_percent(10)` — %10 | lib.rs:509 |
| `START_HEARTBEAT_FINAL_PERIOD` | `Permill::from_percent(80)` — %80 | lib.rs:510 |
| `INVALID_VALIDATORS_LEN` | `10u8` | lib.rs:455 |
| Offence ID | `*b"im-online:offlin"` | lib.rs:843 |
| `STORAGE_VERSION` | `StorageVersion::new(1)` | lib.rs:258 |
| `UnsignedPriority` mock | `ConstU64<{ 1 << 20 }>` | mock.rs:200 |
| `MaxKeys` mock | `ConstU32<10_000>` | mock.rs:202 |
| `MaxPeerInHeartbeats` mock | `ConstU32<10_000>` | mock.rs:203 |

### 1.10 SlashFraction Hesabı — Matematiksel Analiz

Formül: `min((3 * (k - (n/10 + 1))) / n, 1) * 0.07`

Burada:
- `k` = offline validator sayısı (concurrent offenders)
- `n` = toplam validator set sayısı

**Test vektörleri (n=50, tests.rs:36-56):**
```
k=1   → 0%    (1 < 50/10+1=6 → eşik altında)
k=5   → 0%    (5 < 6 → eşik altında)
k=7   → 0.42% (threshold=7-6=1, x=3*1/50=0.06, result=0.06*0.07=0.0042)
k=17  → 4.62% (threshold=17-6=11, x=3*11/50=0.66, result=0.66*0.07=0.0462)
```

**Ekonomik mantık:**
- %10 + 1 validator offline olana kadar slash yok
- Ondan sonra lineer artış
- Maksimum slash = %7 (%100 offline senaryosu teorik max, ama pratikte ulaşılamaz)
- 1/3 offline → yaklaşık %5 slash

Bu formül BABE/GRANDPA slash'larından çok daha hafiftir. Amaç, geçici ağ sorunlarını cezalandırmak değil, sistematik offline durumları fark ettirmektir.

### 1.11 Migration v0→v1

v0: `ReceivedHeartbeats` `WrapperOpaque<BoundedOpaqueNetworkState>` saklıyordu (peer ID + dış adresler)
v1: Sadece `bool` saklıyor (`true` = heartbeat alındı)

Bu migration, heartbeat'in içerik değil varlık kontrolüne indirgenmesini yansıtır. Ağ durumu artık heartbeat içinde taşınmıyor.

---

## KISIM 2: pallet-offences

### 2.1 Genel Bakış

`pallet-offences`, tüm Substrate tabanlı suç raporlarını (equivocation, unresponsiveness, vb.) merkezi bir yerde takip eden bir pallet'tir. Bir offence pallet'i değil, offence **kayıt ve dağıtım** pallet'idir. Asıl slash mantığı `OnOffenceHandler` trait'ini implement eden palletlerde (pallet-staking gibi) bulunur.

### 2.2 Temel Veri Yapıları

#### 2.2.1 OffenceDetails

```rust
// sp_staking/src/offence.rs (sp-staking crate'inden import ediliyor)
pub struct OffenceDetails<Reporter, Offender> {
    pub offender: Offender,     // Suçlu kimliği + tam tanımlama
    pub reporters: Vec<Reporter>, // Kim raporladı
}
```

`OffenceDetails` standart bir yapıdır. `offender` field'ı genellikle `(ValidatorId, FullIdentification)` tuple'ıdır; FullIdentification validator'ın staking bilgilerini içerir.

#### 2.2.2 Storage Yapısı

```rust
// lib.rs:76-94
// Tüm suç raporları — report_id → OffenceDetails
pub type Reports<T: Config> = StorageMap<
    _,
    Twox64Concat,
    ReportIdOf<T>,  // T::Hash
    OffenceDetails<T::AccountId, T::IdentificationTuple>,
>;

// Aynı kind ve slot'ta yaşanan concurrent offence'lar
pub type ConcurrentReportsIndex<T: Config> = StorageDoubleMap<
    _,
    Twox64Concat,
    Kind,           // [u8; 16] — offence tipi
    Twox64Concat,
    OpaqueSlot,     // SCALE-encoded slot (Vec<u8>)
    Vec<ReportIdOf<T>>,
    ValueQuery,
>;
```

### 2.3 Offence Trait — Tam Analiz

```rust
// sp_staking/src/offence.rs (im-online/lib.rs'den görülüyor)
pub trait Offence<Offender> {
    const ID: Kind;              // [u8; 16] sabit — suç tipi tanımlayıcı
    type Slot: Encode;           // Zaman birimi (session index, blok no, vb.)

    fn offenders(&self) -> Vec<Offender>;
    fn session_index(&self) -> SessionIndex;
    fn validator_set_count(&self) -> u32;
    fn slot(&self) -> Self::Slot;
    fn slash_fraction(&self, offenders: u32) -> Perbill;  // CONCURRENT offender sayısı

    // Varsayılan implementasyon
    fn time_slot(&self) -> OpaqueTimeSlot {
        self.slot().encode()
    }
}
```

**Önemli Detay:** `slash_fraction(offenders: u32)` parametresi, **rapor edilen toplam offender değil, CONCURRENT offender sayısıdır**. Aynı kind ve slot'ta kaç farklı validator suç işledi?

Farklı offence ID örnekleri:
- `*b"im-online:offlin"` — pallet-im-online
- `*b"aura:equivocatio"` — AURA equivocation
- `*b"babe:equivocatio"` — BABE equivocation
- `*b"grandpa:equivoca"` — GRANDPA equivocation

### 2.4 ReportOffence Pipeline — Tam Akış

```rust
// lib.rs:107-152
impl<T, O> ReportOffence<T::AccountId, T::IdentificationTuple, O> for Pallet<T>
where
    T: Config,
    O: Offence<T::IdentificationTuple>,
{
    fn report_offence(reporters: Vec<T::AccountId>, offence: O) -> Result<(), OffenceError> {
        // 1. Offender'ları al
        let offenders = offence.offenders();
        let slot = offence.slot();

        // 2. Triage: Daha önce raporlanmış mı?
        let TriageOutcome { concurrent_offenders } =
            match Self::triage_offence_report::<O>(reporters, &slot, offenders) {
                Some(triage) => triage,
                None => return Err(OffenceError::DuplicateReport),
            };

        // 3. Concurrent offender sayısını al
        let offenders_count = concurrent_offenders.len() as u32;

        // 4. Slash fraction hesapla (TÜMÜ aynı fraction alır)
        let new_fraction = offence.slash_fraction(offenders_count);
        let slash_perbill: Vec<_> = (0..concurrent_offenders.len())
            .map(|_| new_fraction)
            .collect();

        // 5. OnOffenceHandler'ı çağır (pallet-staking bunu implement eder)
        T::OnOffenceHandler::on_offence(
            &concurrent_offenders,
            &slash_perbill,
            offence.session_index(),
        );

        // 6. Event yayınla
        Self::deposit_event(Event::Offence { kind: O::ID, slot: slot.encode() });

        Ok(())
    }
}
```

**6 Adımlı Pipeline:**

1. **Offender Listesi Alma**: `offence.offenders()` çağrısı
2. **Triage (Ayıklama)**: Daha önce raporlanmış offender'lar filtrelenir
3. **Concurrent Sayım**: Aynı slot+kind için kaç unique offender var?
4. **Slash Hesabı**: `slash_fraction(concurrent_count)` — herkese aynı fraction uygulanır
5. **Handler Çağrısı**: Gerçek slash işlemi dış pallet'e delege edilir
6. **Event**: Blockchain event kaydı

### 2.5 Triage Mekanizması — Duplikasyon Kontrolü

```rust
// lib.rs:174-210
fn triage_offence_report<O>(reporters, slot, offenders) -> Option<TriageOutcome<T>> {
    let mut storage = ReportIndexStorage::<T, O>::load(slot);
    let mut any_new = false;

    for offender in offenders {
        let report_id = Self::report_id::<O>(slot, &offender);

        if !<Reports<T>>::contains_key(&report_id) {
            any_new = true;
            <Reports<T>>::insert(&report_id, OffenceDetails { offender, reporters: reporters.clone() });
            storage.insert(report_id);
        }
    }

    if any_new {
        let concurrent_offenders = storage.concurrent_reports
            .iter()
            .filter_map(<Reports<T>>::get)
            .collect::<Vec<_>>();
        storage.save();
        Some(TriageOutcome { concurrent_offenders })
    } else {
        None  // Tüm offender'lar zaten biliniyor → DuplicateReport
    }
}
```

**Kritik Tasarım Kararı:** `triage_offence_report` hem yeni offender'ları kaydeder hem de **tüm concurrent offender'ları** döndürür (sadece yenileri değil). Bu, `slash_fraction(concurrent_count)` hesabının her raporlamada güncel tutulmasını sağlar.

### 2.6 report_id Hesabı

```rust
// lib.rs:165-170
fn report_id<O: Offence<T::IdentificationTuple>>(slot: &O::Slot, offender: &T::IdentificationTuple) -> ReportIdOf<T> {
    (O::ID, slot.encode(), offender).using_encoded(T::Hashing::hash)
}
```

Report ID = `Hash(kind [16 byte] + slot [encoded] + offender [encoded])`

Bu formül şunu garanti eder: Aynı kind + slot + offender kombinasyonu için her zaman aynı ID üretilir. Bu, duplikasyon tespitinin O(1) yapılmasını sağlar.

### 2.7 ReportIndexStorage — Concurrent Reports Takibi

```rust
// lib.rs:223-249
struct ReportIndexStorage<T: Config, O: Offence<T::IdentificationTuple>> {
    slot: OpaqueSlot,
    concurrent_reports: Vec<ReportIdOf<T>>,
    _phantom: PhantomData<O>,
}

impl<T, O> ReportIndexStorage<T, O> {
    fn load(slot: &O::Slot) -> Self {
        let slot = slot.encode();
        let concurrent_reports = <ConcurrentReportsIndex<T>>::get(&O::ID, &slot);
        Self { slot, concurrent_reports, _phantom: Default::default() }
    }

    fn insert(&mut self, report_id: ReportIdOf<T>) {
        self.concurrent_reports.push(report_id);
    }

    fn save(self) {
        <ConcurrentReportsIndex<T>>::insert(&O::ID, &self.slot, &self.concurrent_reports);
    }
}
```

`#[must_use]` attribute'u unutulursa derleme uyarısı verir: "Changes are not saved without called `save`". Bu, kayıt bütünlüğünü kodlama düzeyinde zorunlu kılar.

### 2.8 is_known_offence()

```rust
// lib.rs:144-151
fn is_known_offence(offenders: &[T::IdentificationTuple], slot: &O::Slot) -> bool {
    let any_unknown = offenders.iter().any(|offender| {
        let report_id = Self::report_id::<O>(slot, offender);
        !<Reports<T>>::contains_key(&report_id)
    });
    !any_unknown  // Tüm offender'lar biliniyorsa true
}
```

Dikkat: `any_unknown` mantığı ters! "Hiç bilinmeyen yoksa" → tümü biliniyor → `true` döner. Tek bir bilinmeyen bile varsa `false` döner.

### 2.9 Test Doğrulaması

**Test: should_report_an_authority_and_trigger_on_offence (tests.rs:52-68)**
```
n=5, k=1 suç → slash_fraction = 5 + 1*100/5 = %25 (test mock'undan)
```

**Test: should_properly_count_offences (tests.rs:238-268)**
```
Slot 42'de:
  1. Offence: offender=[5], slash = %25 (1 concurrent)
  2. Offence: offender=[4], slash = (5 + 2*100/5)% = %45? (2 concurrent)
  → ConcurrentReportsIndex'te [5, 4] concurrent tutulur
```

Bu test, her yeni raporlamada tüm concurrent offender'ların yeniden değerlendirildiğini gösterir.

### 2.10 OnOffenceHandler — Dış Pallet Arayüzü

```rust
pub trait OnOffenceHandler<Reporter, Offender, Res = ()> {
    fn on_offence(
        offenders: &[OffenceDetails<Reporter, Offender>],
        slash_fraction: &[Perbill],
        offence_session: SessionIndex,
    ) -> Res;
}
```

pallet-staking bu trait'i implement eder. Burada gerçek token slash'ı, ödül kesintileri ve chilling işlemleri gerçekleşir. pallet-offences, bu kararı pallet-staking'e delege eder; kendisi sadece kayıt tutar.

---

## KISIM 3: pallet-authority-discovery

### 3.1 Genel Bakış

`pallet-authority-discovery`, Substrate validator node'larının public key'lerini on-chain'de yayınlamak için kullanılan son derece basit bir pallet'tir. Asıl DHT (Distributed Hash Table) entegrasyonu `client/authority-discovery` crate'inde yaşar; bu pallet sadece key listesini on-chain tutar.

**Amaç:** Diğer node'lar, `pallet-authority-discovery::authorities()` çağırarak mevcut ve gelecek validator kümesinin public key'lerini öğrenir. Bu key'leri DHT'de arayarak validator'ların ağ adreslerini keşfedebilirler.

### 3.2 Storage

```rust
// lib.rs:53-61
/// Mevcut authority set'in key'leri
pub type Keys<T: Config> =
    StorageValue<_, WeakBoundedVec<AuthorityId, T::MaxAuthorities>, ValueQuery>;

/// Sonraki authority set'in key'leri
pub type NextKeys<T: Config> =
    StorageValue<_, WeakBoundedVec<AuthorityId, T::MaxAuthorities>, ValueQuery>;
```

Sadece iki storage item. `AuthorityId` = `sp_authority_discovery::AuthorityId` (sr25519 tabanlı).

### 3.3 Public API

```rust
// lib.rs:90-109
/// Mevcut ve sonraki authority set'in ID'leri — sort + dedup edilmiş
pub fn authorities() -> Vec<AuthorityId> {
    let mut keys = Keys::<T>::get().to_vec();
    let next = NextKeys::<T>::get().to_vec();
    keys.extend(next);
    keys.sort();
    keys.dedup();
    keys.to_vec()
}

/// Mevcut authority set — orijinal sırada
pub fn current_authorities() -> WeakBoundedVec<AuthorityId, T::MaxAuthorities> {
    Keys::<T>::get()
}

/// Sonraki authority set — orijinal sırada
pub fn next_authorities() -> WeakBoundedVec<AuthorityId, T::MaxAuthorities> {
    NextKeys::<T>::get()
}
```

`authorities()` hem mevcut hem sonraki authority'leri birleştirip sort+dedup ederek döndürür. Bu, parachain mantığı ve `client/authority-discovery`'nin kullandığı birincil API'dir.

### 3.4 OneSessionHandler Implementasyonu

```rust
// lib.rs:129-176
impl<T: Config> OneSessionHandler<T::AccountId> for Pallet<T> {
    type Key = AuthorityId;

    fn on_genesis_session<'a, I: 'a>(authorities: I) {
        Self::initialize_keys(&authorities.map(|x| x.1).collect::<Vec<_>>());
    }

    fn on_new_session<'a, I: 'a>(changed: bool, validators: I, queued_validators: I) {
        // Keys: yalnızca changed=true ise güncellenir
        if changed {
            let keys = validators.map(|x| x.1).collect::<Vec<_>>();
            let bounded_keys = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(keys, ...);
            Keys::<T>::put(bounded_keys);
        }

        // NextKeys: HER ZAMAN güncellenir (changed'dan bağımsız)
        let next_keys = queued_validators.map(|x| x.1).collect::<Vec<_>>();
        let next_bounded_keys = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(next_keys, ...);
        NextKeys::<T>::put(next_bounded_keys);
    }

    fn on_disabled(_i: u32) {
        // ignore — authority-discovery disabled event'i yoksayar
    }
}
```

**Kritik Asimetri:** `Keys` (mevcut) yalnızca `changed=true` ise güncellenir; `NextKeys` (sonraki) her yeni session'da koşulsuz güncellenir. Bu, `changed` flag'ının mevcut session'ın değil **önceki** session'ın değişikliğini ifade etmesinden kaynaklanır.

### 3.5 initialize_keys()

```rust
// lib.rs:111-122
fn initialize_keys(keys: &Vec<AuthorityId>) {
    if !keys.is_empty() {
        assert!(Keys::<T>::get().is_empty(), "Keys are already initialized!");
        let bounded_keys = WeakBoundedVec::<AuthorityId, T::MaxAuthorities>::try_from((*keys).clone())
            .expect("Keys vec too big");
        Keys::<T>::put(&bounded_keys);
        NextKeys::<T>::put(&bounded_keys);  // Başlangıçta ikisi de aynı
    }
}
```

Genesis'te hem `Keys` hem `NextKeys` aynı başlangıç değeriyle set edilir.

### 3.6 DHT Entegrasyonu — client/authority-discovery

Authority discovery'nin asıl mantığı on-chain pallet'ten ayrıdır:

1. `Pallet::authorities()` çağrısı ile public key listesi alınır
2. Her key için DHT'ye lookup yapılır (Libp2p Kademlia)
3. DHT'de bulunan kayıtlar SCALE encode edilmiş `PeerRecord` içerir: `(PeerId, external_addresses[])`
4. Kayıtlar authority key'i ile imzalanmış olmalıdır (spam koruması)
5. Bulunan adresler Libp2p `NetworkBehaviour`'a eklenir

**Pallet sadece key listesi yönetir; ağ discovery tamamen off-chain gerçekleşir.**

### 3.7 Invariant Kontrolü (try-runtime)

```rust
// lib.rs:188-209
pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
    // Keys MaxAuthorities sınırı içinde mi?
    ensure!(keys.len() as u32 <= T::MaxAuthorities::get(), "Keys exceeds MaxAuthorities");
    // Duplikasyon var mı?
    ensure!(sorted_keys.windows(2).all(|w| w[0] != w[1]), "Duplicate keys found in Keys");
    // NextKeys için aynı kontroller...
}
```

### 3.8 Test Senaryoları

```rust
// Senaryo 1: changed=false → Keys değişmez, NextKeys güncellenir
AuthorityDiscovery::on_new_session(false, second_authorities_iter, third_authorities_iter);
// Sonuç: authorities() = first ∪ third (Keys=first, NextKeys=third)

// Senaryo 2: changed=true → Keys güncellenir, NextKeys de güncellenir
AuthorityDiscovery::on_new_session(true, third_authorities_iter, fourth_authorities_iter);
// Sonuç: authorities() = third ∪ fourth

// Senaryo 3: Överlap durumu — dedup çalışır
AuthorityDiscovery::on_new_session(true, same_iter, same_iter);
// Sonuç: authorities() = deduplicated set
```

### 3.9 Config Yapısı

```rust
pub trait Config: frame_system::Config + pallet_session::Config {
    type MaxAuthorities: Get<u32>;
}
// Test: MaxAuthorities = ConstU32<100>
```

Minimal konfigürasyon: sadece `MaxAuthorities` limiti gereklidir.

---

## KISIM 4: pallet-bags-list

### 4.1 Genel Bakış ve Motivasyon

`pallet-bags-list`, validator seçimi için gereken "en büyük N stake'e sahip account" listesini verimli bir şekilde tutmak için tasarlanmıştır. Tamamen sıralı bir liste naif görünse de, her stake değişikliğinde listeyi yeniden sıralamak çok maliyetli olur. Bags-list, bu problemi "yeterince iyi sıralama" ile çözer.

**Temel Fikir:** Account'lar stake miktarına göre "çantalar"a (bags) ayrılır. Her çanta, belirli bir score aralığını temsil eder. Aynı çanta içindeki account'lar **ekleme sırasına** göre sıralanır. Çantalar ise score'a göre sıralanır. Böylece:
- İterasyon: Yüksek çantadan düşük çantaya doğru (genel olarak artan stake sırası)
- Ekleme/Silme: O(1) — sadece çantanın linked list'ine ekleme/çıkarma
- Score güncelleme: Muhtemelen çanta değişmez (O(1)), nadiren çanta değişir (O(1) ama storage yazması)

### 4.2 BagThresholds — Çanta Eşikleri

```rust
// Config trait (lib.rs:234)
#[pallet::constant]
type BagThresholds: Get<&'static [Self::Score]>;
```

Test konfigürasyonu (mock.rs:57):
```rust
pub static BagThresholds: &'static [VoteWeight] = &[10, 20, 30, 40, 50, 60, 1_000, 2_000, 10_000];
```

**Eşik Kuralları:**
- Eşikler **kesinlikle artan** sırada olmalı (integrity_test() bunu kontrol eder)
- Duplikasyon yok
- `T::Score::max_value()` implicit olarak son eşik kabul edilir
- Önerilen: Geometrik seri (`threshold[k+1] = max(threshold[k] * ratio, threshold[k] + 1)`)
- 64 eşik → sabit oran ≈ 2x
- 200 eşik → sabit oran ≈ 1.248x

**notional_bag_for() — Hangi çantaya ait?**

```rust
// list/mod.rs:88-92
pub fn notional_bag_for<T: Config<I>, I: 'static>(score: T::Score) -> T::Score {
    let thresholds = T::BagThresholds::get();
    let idx = thresholds.partition_point(|&threshold| score > threshold);
    thresholds.get(idx).copied().unwrap_or_else(T::Score::max_value)
}
```

`partition_point` binary search kullanır: kaç eşik `score`'dan küçük? O eşiğin bir üstüne gider.

**Test vektörleri (list/tests.rs:73-93):**
```
score=0  → bag 10  (0 > 0 eşik yok → idx=0 → threshold[0]=10)
score=9  → bag 10  (10'dan küçük)
score=10 → bag 10  (10 = eşik, ≤ 10 → aynı çanta)
score=11 → bag 20  (10'dan büyük → idx=1 → threshold[1]=20)
score=10_001 → bag max (tüm explicit eşiklerden büyük → max_value)
```

### 4.3 Node Yapısı — Çift Yönlü Bağlı Liste Elemanı

```rust
// list/mod.rs:848-862
pub struct Node<T: Config<I>, I: 'static = ()> {
    pub id: T::AccountId,           // Account kimliği (storage key)
    pub prev: Option<T::AccountId>,  // Önceki node
    pub next: Option<T::AccountId>,  // Sonraki node
    pub bag_upper: T::Score,         // Ait olduğu çantanın üst eşiği
    pub score: T::Score,             // Mevcut score (stale olabilir!)
    pub _phantom: PhantomData<I>,
}
```

**Önemli:** `score` alanı stale (eski) olabilir! Account'ın gerçek score'u `T::ScoreProvider::score()` ile alınmalıdır. Bir node'un `score != ScoreProvider::score()` ise, `rebag` gereklidir.

**Node Operasyonları:**
```rust
fn excise(&self) {          // Yalnızca komşuları günceller, kendini silmez
    if let Some(mut prev) = self.prev() { prev.next = self.next.clone(); prev.put(); }
    if let Some(mut next) = self.next() { next.prev = self.prev.clone(); next.put(); }
}

fn remove_from_storage_unchecked(&self) {
    crate::ListNodes::<T, I>::remove(&self.id)
}

fn is_misplaced(&self, current_score: T::Score) -> bool {
    notional_bag_for::<T, I>(current_score) != self.bag_upper
}

fn is_terminal(&self) -> bool {
    self.prev.is_none() || self.next.is_none()  // Head veya tail
}
```

### 4.4 Bag Yapısı

```rust
// list/mod.rs:652-660
pub struct Bag<T: Config<I>, I: 'static = ()> {
    pub head: Option<T::AccountId>,  // Listenin başı
    pub tail: Option<T::AccountId>,  // Listenin sonu
    #[codec(skip)]
    pub bag_upper: T::Score,         // Storage'da encode edilmez (key'den geri alınır)
    #[codec(skip)]
    pub _phantom: PhantomData<I>,
}
```

`bag_upper` codec skip'tir çünkü storage key'i olarak zaten kullanılır; ayrıca encode edilmesine gerek yoktur.

**Bag Operasyonları:**
```rust
fn put(self) {
    if self.is_empty() {
        crate::ListBags::<T, I>::remove(self.bag_upper);  // Boş bag → storage'dan sil
    } else {
        crate::ListBags::<T, I>::insert(self.bag_upper, self);
    }
}
```

Boş bir bag, storage'dan otomatik silinir. Bu, boş bag'lerin birikmesini önler.

### 4.5 Rebag Mekanizması — Tam Analiz

Rebag, bir account'ın score değiştiğinde yanlış çantadan doğru çantaya taşınmasıdır.

#### 4.5.1 update_position_for() — Çekirdek Mantık

```rust
// list/mod.rs:425-461
pub(crate) fn update_position_for(mut node: Node<T, I>, new_score: T::Score) -> Option<(T::Score, T::Score)> {
    node.score = new_score;
    if node.is_misplaced(new_score) {
        let old_bag_upper = node.bag_upper;

        if !node.is_terminal() {
            // Normal node: sadece komşuları güncelle (bag güncelleme gerekmez)
            node.excise();
        } else if let Some(mut bag) = Bag::<T, I>::get(node.bag_upper) {
            // Head/tail: bag da güncellenmeli
            bag.remove_node_unchecked(&node);
            bag.put();
        }

        // Doğru çantaya ekle
        let new_bag_upper = notional_bag_for::<T, I>(new_score);
        let mut bag = Bag::<T, I>::get_or_make(new_bag_upper);
        bag.insert_node_unchecked(node);
        bag.put();

        Some((old_bag_upper, new_bag_upper))
    } else {
        // Çanta değişmedi, sadece score'u güncelle
        node.put();
        None
    }
}
```

**Rebag Senaryoları:**
- Terminal olmayan node (head ve tail değil): `excise()` → komşu güncelle, yeni çantaya ekle
- Terminal node (head veya tail): Eski çantanın head/tail pointer'ını güncelle, yeni çantaya ekle
- Çanta değişmiyorsa: Sadece score yazılır, `None` döner

#### 4.5.2 rebag() Extrinsic

```rust
// lib.rs:354-363
pub fn rebag(origin: OriginFor<T>, dislocated: AccountIdLookupOf<T>) -> DispatchResult {
    ensure_signed(origin)?;             // Herhangi biri çağırabilir!
    let dislocated = T::Lookup::lookup(dislocated)?;
    Self::ensure_unlocked().map_err(|_| Error::<T, I>::Locked)?;
    Self::rebag_internal(&dislocated).map_err::<DispatchError, _>(Into::into)?;
    Ok(())
}
```

**Permissionless rebag!** Herhangi bir hesap, herhangi bir misplaced account'ı doğru çantaya taşıyabilir. Ücret rebag yapan kişi öder.

#### 4.5.3 rebag_internal() — İç Mantık

```rust
// lib.rs:638-673
fn rebag_internal(account: &T::AccountId) -> Result<Option<(T::Score, T::Score)>, Error<T, I>> {
    Self::ensure_unlocked().map_err(|_| Error::<T, I>::Locked)?;
    PendingRebag::<T, I>::remove(account);  // Bekleyen listeden çıkar

    let existed = ListNodes::<T, I>::contains_key(account);
    let maybe_score = T::ScoreProvider::score(account);  // GÜNCEL score

    match (existed, maybe_score) {
        (true, Some(current_score)) => {
            Pallet::<T, I>::do_rebag(account, current_score)  // Normal rebag
        },
        (false, Some(current_score)) => {
            List::<T, I>::insert(account.clone(), current_score)  // Yeni ekleme
            Ok(None)
        },
        (true, None) => {
            List::<T, I>::remove(account)  // Score yok → çıkar
            Ok(None)
        },
        (false, None) => {
            Err(Error::<T, I>::List(ListError::NodeNotFound))
        },
    }
}
```

`rebag_internal` dört durumu ele alır. Score her zaman `ScoreProvider`'dan taze olarak alınır — bu, slash veya reward sonrası güncel değerin kullanılmasını garanti eder.

### 4.6 on_idle — Otomatik Rebag

```rust
// lib.rs:435-590
fn on_idle(_n: BlockNumberFor<T>, limit: Weight) -> Weight {
    let mut meter = WeightMeter::with_limit(limit);
    let per_item = T::WeightInfo::on_idle_rebag();
    let rebag_budget = T::MaxAutoRebagPerBlock::get();

    // Önce PendingRebag (kilitli dönemde biriken account'lar)
    // Sonra normal ListNodes cursor'dan devam
    let combined_iter = PendingRebag::<T, I>::iter_keys().chain(regular_iter);
    let accounts: Vec<_> = combined_iter.take((rebag_budget + 1) as usize).collect();

    // İşle
    for (i, account) in to_process.iter().enumerate() {
        if i > 0 && meter.try_consume(per_item).is_err() { break; }
        Self::rebag_internal(account)...
    }

    // Cursor güncelle (sadece normal list için, PendingRebag için değil)
    NextNodeAutoRebagged::<T, I>::put(next_regular_account);
}
```

**Otomatik rebag özellikleri:**
- `MaxAutoRebagPerBlock` kadar account her blokta işlenir
- Kalıcı cursor (`NextNodeAutoRebagged`) sayesinde tüm liste bloklar arasında işlenir
- PendingRebag (kilitli dönemde biriken) öncelikli olarak işlenir
- Weight limiti aşılırsa durur (blok overload koruması)
- Mock: `AutoRebagNumber = 10`

### 4.7 Lock Mekanizması

```rust
// lib.rs:283-302
/// Snapshot alınırken tüm güncellemeleri kilitler
pub type Lock<T: Config<I>, I: 'static = ()> = StorageValue<_, (), OptionQuery>;

/// Kilitli dönemde eklenemeyen account'lar
pub type PendingRebag<T: Config<I>, I: 'static = ()> = CountedStorageMap<_, Twox64Concat, T::AccountId, ()>;

impl SortedListProvider for Pallet {
    fn lock() { Lock::<T, I>::put(()) }
    fn unlock() { Lock::<T, I>::kill() }

    fn on_insert(id, score) -> Result<(), ListError> {
        if Pallet::<T, I>::ensure_unlocked().is_err() {
            if T::MaxAutoRebagPerBlock::get() > 0u32 {
                PendingRebag::<T, I>::insert(&id, ());
                return Ok(());  // Hata değil, sonra işlenecek
            }
            return Err(ListError::Locked);
        }
        List::<T, I>::insert(id, score)
    }
}
```

Lock mekanizması, election snapshot alınırken listenin değişmemesini sağlar. Kilitli dönemde yeni ekleme talepleri `PendingRebag`'a kaydedilir, kilit açılınca `on_idle`'da işlenir.

**PendingRebag unbounded olması:** Kasıtlı tasarım kararı. Nedenleri:
1. Geçici — on_idle'da hızla işlenir
2. Snapshot kısa sürer (weight limiti var)
3. Saldırgan sınırlı miktarda exploit edebilir
4. Yapay sınır meşru auto-rebagging'i engelleyebilir

### 4.8 put_in_front_of() — Sıra İyileştirme

```rust
// lib.rs:377-387
pub fn put_in_front_of(origin, lighter) -> DispatchResult {
    let heavier = ensure_signed(origin)?;
    let lighter = T::Lookup::lookup(lighter)?;
    List::<T, I>::put_in_front_of(&lighter, &heavier)...
}
```

```rust
// list/mod.rs:465-495
pub(crate) fn put_in_front_of(lighter_id, heavier_id) -> Result<(), ListError> {
    // 1. Her iki node da var mı?
    let lighter_node = Node::get(lighter_id).ok_or(ListError::NodeNotFound)?;
    let heavier_node = Node::get(heavier_id).ok_or(ListError::NodeNotFound)?;

    // 2. Aynı çantada mı?
    ensure!(lighter_node.bag_upper == heavier_node.bag_upper, ListError::NotInSameBag);

    // 3. Heavier gerçekten daha ağır mı? (PAHALALI KONTROL — EN SON!)
    ensure!(
        T::ScoreProvider::score(heavier_id) > T::ScoreProvider::score(lighter_id),
        ListError::NotHeavier
    );

    // 4. Heavier'ı çıkar, lighter'ın önüne ekle
    Self::remove(heavier_id)...
    let lighter_node = Node::get(lighter_id)...  // Yeniden fetch gerekli!
    Self::insert_at_unchecked(lighter_node, heavier_node);
    Ok(())
}
```

Koşullar:
- Aynı çantada olmalı (farklı çantalar arası sıralama `rebag` ile yapılır)
- Çağıran account (heavier) diğerinden (lighter) daha fazla score'a sahip olmalı
- Ücret her zaman çağıran tarafından ödenir

### 4.9 List::iter() — İterasyon Sırası

```rust
// list/mod.rs:260-279
pub(crate) fn iter() -> impl Iterator<Item = Node<T, I>> {
    let thresholds = T::BagThresholds::get();
    let iter = thresholds.iter().copied();

    // Eğer son eşik max_value değilse, onu ekle
    let iter: Box<dyn Iterator<Item = T::Score>> = if thresholds.last() == Some(&T::Score::max_value()) {
        Box::new(iter.rev())  // Yüksekten düşüğe (zaten max_value ile bitiyor)
    } else {
        Box::new(iter.chain(iter::once(T::Score::max_value())).rev())  // max_value'yu ekle, sonra rev
    };

    iter.filter_map(Bag::get).flat_map(|bag| bag.iter())
}
```

**İterasyon örüntüsü:**
1. Eşikler `rev()` ile yüksekten düşüğe alınır: `[max, 10_000, 2_000, 1_000, ...]`
2. Her eşik için çanta varsa (`filter_map(Bag::get)`), o çantanın node'ları ekleme sırasıyla iterate edilir
3. Boş çantalar atlanır

Test kanıtı (list/tests.rs:64-68):
```
Genesis: [(1, 10), (2, 1000), (3, 1000), (4, 1000)]
iter() → [2, 3, 4, 1]  ← 1000 çantası önce (yüksek), 10 çantası sonra (düşük)
                          1000 çantası içinde: ekleme sırası [2, 3, 4]
```

### 4.10 migrate() — Eşik Değişikliği

```rust
// list/mod.rs:163-244
pub fn migrate(old_thresholds: &[T::Score]) -> u32 {
    // 1. Yeni eşikler eklendi mi? → Eski çantadan taşı
    let new_bags = new_set.difference(&old_set).copied();
    for inserted_bag in new_bags {
        // Eklenen eşiğin etkilediği eski çantayı bul
        let affected_bag = ... // old_thresholds üzerinde partition_point
        // O çantanın tüm account'larını affected_accounts'a ekle
    }

    // 2. Eski eşikler silindi mi? → O çantanın hepsini taşı
    let removed_bags = old_set.difference(&new_set).copied();
    for removed_bag in removed_bags {
        // O çantanın tüm account'larını affected_accounts'a ekle
    }

    // 3. Etkilenen account'ları remove → insert (yeni eşiklere göre)
    let _removed = Self::remove_many(&affected_accounts);
    let _inserted = Self::insert_many(affected_accounts.into_iter(), score_of);

    // 4. Boşalan eski çantaları storage'dan sil
    for removed_bag in removed_bags { crate::ListBags::<T, I>::remove(removed_bag); }

    num_affected
}
```

### 4.11 SortedListProvider Trait Implementasyonu

```rust
impl SortedListProvider<T::AccountId> for Pallet<T, I> {
    fn range() -> (Self::Score, Self::Score) {
        // (en küçük eşik, en büyük eşik)
    }
    fn iter() -> Box<dyn Iterator<Item = T::AccountId>>
    fn iter_from(start: &T::AccountId) -> Result<...>
    fn count() -> u32 { ListNodes::count() }
    fn contains(id: &T::AccountId) -> bool
    fn on_insert(id, score) -> Result<(), ListError>
    fn on_update(id, new_score) -> Result<(), ListError>  // rebag çağırır
    fn get_score(id) -> Result<T::Score, ListError>
    fn on_remove(id) -> Result<(), ListError>
    fn lock() / fn unlock()
    fn unsafe_regenerate(all, score_of) -> u32
    fn unsafe_clear()
}
```

Bu trait, pallet-bags-list'i election provider'a bağlar. pallet-staking, validator seçimi için bu trait'i kullanır.

### 4.12 Sabitler ve Config Değerleri

| Parametre | Test Değeri | Konum |
|-----------|-------------|-------|
| `BagThresholds` | `[10, 20, 30, 40, 50, 60, 1_000, 2_000, 10_000]` | mock.rs:57 |
| `MaxAutoRebagPerBlock` | `10` | mock.rs:58 |
| Genesis IDs | `[(1,10), (2,1000), (3,1000), (4,1000)]` | mock.rs:79 |

### 4.13 Storage Yapısı

```
ListNodes<T, I>: CountedStorageMap<AccountId, Node<T, I>>
  → Tüm node'lar. Count() O(1).

ListBags<T, I>: StorageMap<T::Score, Bag<T, I>>
  → Her çanta. Boş çantalar silinir.

NextNodeAutoRebagged<T, I>: StorageValue<_, T::AccountId, OptionQuery>
  → Otomatik rebag cursor'ı. Bloklar arası devam.

Lock<T, I>: StorageValue<_, (), OptionQuery>
  → Snapshot kilidi. None=açık, Some(())=kilitli.

PendingRebag<T, I>: CountedStorageMap<AccountId, ()>
  → Kilitli dönemde eklenemeyen account'lar.
```

### 4.14 do_rebag() — Event Yayını ile Rebag

```rust
// lib.rs:601-619
pub fn do_rebag(account: &T::AccountId, new_score: T::Score) -> Result<Option<(T::Score, T::Score)>, ListError> {
    let node = list::Node::<T, I>::get(&account).ok_or(ListError::NodeNotFound)?;

    // Score değiştiyse ScoreUpdated event'i yayınla
    if node.score != new_score {
        Self::deposit_event(Event::ScoreUpdated { who: account.clone(), new_score });
    }

    let maybe_movement = List::update_position_for(node, new_score);

    // Çanta değiştiyse Rebagged event'i yayınla
    if let Some((from, to)) = maybe_movement {
        Self::deposit_event(Event::Rebagged { who: account.clone(), from, to });
    }

    Ok(maybe_movement)
}
```

---

## Nova Shield Uygulama Notları

### A. Validator Liveness Monitoring (im-online referansı)

**Öneri:** Nova Shield, im-online'a benzer bir heartbeat mekanizması uygulayabilir, ancak geliştirilmiş versiyon:

```
1. Zamanlama: Session'ın %10-%80'i arası stokastik gönderim (polinom eşik formülü)
   → Ağ tıkanıklığını önler
   → p^6 + 1/session_length formülü kopyalanabilir

2. Çift kanıt: heartbeat VEYA blok üretimi
   → Gereksiz heartbeat trafiğini azaltır

3. Off-chain lock: INCLUDE_THRESHOLD=3 (3 blok bekle)
   → Duplicate gönderimi önler

4. Session sonu tespiti: tek session miss → offline raporu
   → Tek session'da tepki, birden fazla session bekleme
```

**Nova Shield farkı:** Penumbra-benzeri ZK heartbeat: validator'ın bazı private state'leri içeren ZK kanıtı → kimlik ifşa edilmeden liveness kanıtı.

### B. Slash Hesabı (offences referansı)

**im-online slash formülü uygulanabilirlik değerlendirmesi:**

```
Nova Shield'da seçenekler:
1. Aynı formülü al: min((3*(k-n/10-1))/n, 1) * 0.07
   → %10 tolerance, max %7 slash

2. Daha sert: tolerance %5, max %15
   → Güvenlik odaklı tasarım için

3. Tier-based:
   → 1 session miss: uyarı
   → 2 consecutive miss: küçük slash + chilling
   → 3+ consecutive miss: büyük slash
```

**Offence pipeline mimarisi:** Nova Shield'ın aynen kopyalaması gereken pattern:
- Merkezi `pallet-offences` benzeri kayıt sistemi
- `Offence` trait: `ID`, `Slot`, `slash_fraction(concurrent_count)`
- `ReportOffence` → `OnOffenceHandler` delegation
- report_id = Hash(kind + slot + offender) → O(1) duplikasyon tespiti

### C. Authority Discovery Entegrasyonu

**Nova Shield P2P (libp2p + gossipsub) için:**
```
1. pallet-authority-discovery pattern'i:
   → Keys (current) + NextKeys (queued) storage
   → authorities() = sort+dedup birleşim
   → changed=true ise Keys güncelle, NextKeys her zaman güncelle

2. DHT entegrasyonu (libp2p Kademlia):
   → Authority key → (PeerId, external_addresses[]) DHT'ye yaz
   → İmzalı kayıt (spam koruması)
   → Periyodik refresh (TTL süresi dolmadan)

3. Nova Shield ek: ZK kimlik gizleme
   → Authority key'i DHT'ye koyma
   → Sadece commit → validator ağ kimliğini gizler
   → MEV bot'larının validator'ları hedeflemesini engeller
```

### D. Voter Listesi (bags-list referansı)

**Nova Shield staking için VoterBagsList:**

Polkadot'ta pallet-bags-list, pallet-staking ile entegre çalışır. Nova Shield'da aynı pattern:
```
1. BagThresholds: Geometrik seri önerileri
   → Max stake = 1B NOVA
   → 200 çanta → oran ≈ 1.248x
   → İterasyon hassasiyeti yeterli

2. MaxAutoRebagPerBlock: 10-50 arası
   → Blok başına weight maliyeti
   → Stake değişim sıklığına göre ayarla

3. Permissionless rebag incentive:
   → Rebag yapan küçük ödül alabilir (opsiyon)
   → Nova Shield tokenomics'e entegre edilebilir

4. Lock → PendingRebag pattern:
   → Snapshot alınırken liste donduruluyor
   → Snapshot sonrası PendingRebag işleniyor
   → Nova Shield election de aynı pattern kullanabilir
```

### E. Güvenlik Notları

**im-online attack surface:**

1. **Heartbeat spam:** `and_provides((session, authority_id))` ile önleniyor. Nova Shield da aynı tag mekanizmasını kullanmalı.

2. **Stale heartbeat:** Session index kontrolü ile reddediliyor. Nova Shield'da MPC signing'de session bağlama önemli.

3. **Validator DoS:** Offline offence formülü (%10 tolerance), küçük ağ sorunlarında validator'ların cezalandırılmamasını sağlar. Nova Shield'da bu tolerance projeye özgü ayarlanabilir.

**bags-list attack surface:**

1. **Cursor manipulation:** `NextNodeAutoRebagged` döngü tespiti var (defensive! ile). Nova Shield da cursor tabanlı sistemlerde döngü koruması şart.

2. **Lock bypass:** `ensure_unlocked()` kontrolü her mutating path'te. Nova Shield snapshot sisteminde aynı pattern.

3. **Score staleness:** `rebag_internal` her zaman `ScoreProvider::score()` ile taze score alır. Nova Shield'da score cache'leme tehlikeli.

---

## Kaynak Kod Referansları

### pallet-im-online

| Referans | Konum |
|----------|-------|
| `Heartbeat` struct | lib.rs:222-235 |
| `HeartbeatStatus` ve `is_recent()` | lib.rs:163-189 |
| `INCLUDE_THRESHOLD = 3` | lib.rs:155 |
| `DB_PREFIX` | lib.rs:152 |
| `START_HEARTBEAT_RANDOM_PERIOD = 10%` | lib.rs:509 |
| `START_HEARTBEAT_FINAL_PERIOD = 80%` | lib.rs:510 |
| Heartbeat zamanlama formülü (`p^6 + 1/L`) | lib.rs:517-520 |
| `is_online()` çift koşul | lib.rs:485-490 |
| `on_before_session_ending()` offline detection | lib.rs:785-820 |
| `UnresponsivenessOffence` slash formülü | lib.rs:862-872 |
| `slash_fraction` test vektörleri | tests.rs:36-56 |
| `authorize_heartbeat_call()` doğrulama sırası | lib.rs:691-737 |
| `INVALID_VALIDATORS_LEN = 10u8` | lib.rs:455 |
| Offence ID `*b"im-online:offlin"` | lib.rs:843 |
| Migration v0→v1 | migration.rs:56-118 |
| `UnsignedPriority = 1<<20` (mock) | mock.rs:200 |

### pallet-offences

| Referans | Konum |
|----------|-------|
| `Reports` StorageMap | lib.rs:76-82 |
| `ConcurrentReportsIndex` StorageDoubleMap | lib.rs:85-94 |
| `report_offence()` 6 adımlı pipeline | lib.rs:107-142 |
| `report_id()` = Hash(kind+slot+offender) | lib.rs:165-170 |
| `triage_offence_report()` | lib.rs:174-210 |
| `ReportIndexStorage` `#[must_use]` | lib.rs:223 |
| `is_known_offence()` ters mantık | lib.rs:144-151 |
| Mock `slash_fraction = 5 + k*100/n` | mock.rs:136 |
| Concurrent offender test | tests.rs:238-268 |

### pallet-authority-discovery

| Referans | Konum |
|----------|-------|
| `Keys` ve `NextKeys` storage | lib.rs:53-61 |
| `authorities()` sort+dedup | lib.rs:90-99 |
| `on_new_session()` changed asimetrisi | lib.rs:139-171 |
| `initialize_keys()` genesis | lib.rs:111-122 |
| `on_disabled()` ignore | lib.rs:173 |
| `do_try_state()` invariant kontrolü | lib.rs:188-209 |
| `MaxAuthorities = 100` (test) | lib.rs:245 |

### pallet-bags-list

| Referans | Konum |
|----------|-------|
| `BagThresholds` config | lib.rs:233 |
| `MaxAutoRebagPerBlock` config | lib.rs:241 |
| `notional_bag_for()` binary search | list/mod.rs:88-92 |
| `Node` struct (5 alan) | list/mod.rs:848-862 |
| `Bag` struct (head+tail) | list/mod.rs:652-660 |
| `is_misplaced()` | list/mod.rs:909-911 |
| `is_terminal()` | list/mod.rs:913-915 |
| `excise()` komşu güncelleme | list/mod.rs:878-889 |
| `update_position_for()` | list/mod.rs:425-461 |
| `put_in_front_of()` | list/mod.rs:465-495 |
| `List::iter()` yüksekten düşüğe | list/mod.rs:260-279 |
| `migrate()` eşik değişikliği | list/mod.rs:163-244 |
| `on_idle()` otomatik rebag | lib.rs:435-590 |
| Lock mekanizması | lib.rs:283-302 |
| `PendingRebag` unbounded rationale | lib.rs:289-296 |
| `rebag_internal()` 4 durum | lib.rs:638-673 |
| `do_rebag()` event yayını | lib.rs:601-619 |
| Test thresholds `[10,20,30,...,10_000]` | mock.rs:57 |
| Genesis IDs | mock.rs:79 |
| `AutoRebagNumber = 10` | mock.rs:58 |
| `notional_bag_for()` test vektörleri | list/tests.rs:73-93 |

---

## Özet Tablo

| Pallet | Satır Sayısı | Kritik Pattern | Nova Shield Uygulaması |
|--------|-------------|----------------|------------------------|
| im-online | ~874 | Stokastik heartbeat (p^6), çift kanıt, session sonu tespiti | Validator liveness, MPC node monitoring |
| offences | ~251 | Concurrent offence tracking, report dedup (Hash key), OnOffenceHandler delegation | Merkezi ceza kayıt sistemi |
| authority-discovery | ~434 | Keys/NextKeys asimetrisi, sort+dedup authorities() | P2P validator key yönetimi |
| bags-list | ~965+ | Çift yönlü linked list çantaları, permissionless rebag, lock+PendingRebag | Voter listesi, election sıralaması |

**Toplam analiz kapsamı:** ~2.524+ satır doğrudan kaynak kodu + testler + mock'lar

---

## EK KISIM 5: Migration Analizi — Tüm Palletler

### 5.1 pallet-offences Migration v0 → v1

**v0 storage: `ReportsByKindIndex` (offences/src/migration.rs:38-45)**
```rust
type ReportsByKindIndex<T: Config> = StorageMap<
    Pallet<T>,
    Twox64Concat,
    Kind,
    Vec<u8>,  // (O::TimeSlot, ReportIdOf<T>) — opaque binary blob
    ValueQuery,
>;
```

v0'da `ReportsByKindIndex` mevcuttu: Kind bazında tüm raporların binary blob'u olarak tutuluyordu. Bu storage item v1'de **tamamen kaldırıldı**. `ConcurrentReportsIndex` (Kind + OpaqueSlot → Vec<ReportId>) ile değiştirildi. Yeni yapı daha granüler: slot bazında ayrıştırma mümkün.

**Migration işlemi (offences/src/migration.rs:65-75):**
```rust
fn on_runtime_upgrade() -> Weight {
    if Pallet::<T>::on_chain_storage_version() > 0 {
        return T::DbWeight::get().reads(1);
    }
    let keys_removed = v0::ReportsByKindIndex::<T>::clear(u32::MAX, None).unique as u64;
    StorageVersion::new(1).put::<Pallet<T>>();
    T::DbWeight::get().reads_writes(keys_removed + 1, keys_removed + 1)
}
```

v0'daki tüm kayıtlar silinir. Bu agresif yaklaşım, v0 storage yapısının v1 ile uyumsuz olmasından kaynaklanır. Eski raporlar discarded edilir — geri alınamaz.

**Deferred Offences — Tarihi Kalıntı (offences/src/migration.rs:92-114):**
```rust
type DeferredOffenceOf<T> = (
    Vec<OffenceDetails<AccountId, IdentificationTuple>>,
    Vec<Perbill>,
    SessionIndex,
);

type DeferredOffences<T> = StorageValue<Pallet<T>, Vec<DeferredOffenceOf<T>>, ValueQuery>;

pub fn remove_deferred_storage<T: Config>() -> Weight {
    let deferred = <DeferredOffences<T>>::take();
    for (offences, perbill, session) in deferred.iter() {
        T::OnOffenceHandler::on_offence(offences, perbill, *session);
    }
    weight
}
```

Eski versiyonlarda `OnOffenceHandler` başarısız olduğunda offence'lar `DeferredOffences`'a kaydediliyordu. Bu mekanizma artık kullanılmıyor fakat migration kodu geriye uyumluluk için korunuyor. Test (migration.rs:146-179) deferred offence uygulamasının doğruluğunu şöyle doğrular:
```rust
// Perbill::from_percent(5 + 1 * 100 / 5) = %25
assert_eq!(f.clone(), vec![Perbill::from_percent(5 + 1 * 100 / 5)]);
```

### 5.2 pallet-bags-list Migration — AddScore

**v0 Node yapısı (`PreScoreNode`) — bags-list/src/migrations.rs:68-75:**
```rust
pub struct PreScoreNode<T: crate::Config<I>, I: 'static = ()> {
    pub id: T::AccountId,
    pub prev: Option<T::AccountId>,
    pub next: Option<T::AccountId>,
    pub bag_upper: T::Score,
    // score alanı YOKTU — v1'de eklendi
}
```

v0'da node'lar score bilgisi taşımıyordu; score her zaman `ScoreProvider`'dan sorgulanıyordu. Bu storage okuma maliyetli olduğu için `score` field node'a gömüldü.

**AddScore migration (bags-list/src/migrations.rs:105-121):**
```rust
fn on_runtime_upgrade() -> frame_support::weights::Weight {
    for (_key, node) in old::ListNodes::<T, I>::iter() {
        let score = T::ScoreProvider::score(&node.id);
        let new_node = crate::Node {
            id: node.id.clone(),
            prev: node.prev,
            next: node.next,
            bag_upper: node.bag_upper,
            score: score.unwrap_or_default(),  // ScoreProvider'dan taze al, yoksa 0
            _phantom: node._phantom,
        };
        crate::ListNodes::<T, I>::insert(node.id, new_node);
    }
    return frame_support::weights::Weight::MAX;  // Unbounded!
}
```

Her node için `ScoreProvider::score()` çağırılır (pallet-staking'e erişim gerektirir). `Weight::MAX` döndürülmesi, migration'ın tek blokta tamamlanamayabileceği anlamına gelir. Gerçek production'da multi-block migration framework zorunludur.

**CheckCounterPrefix (migrations.rs:34-61) — yalnızca doğrulama:**
```rust
fn pre_upgrade() -> Result<Vec<u8>, TryRuntimeError> {
    ensure!(
        crate::ListNodes::<T, I>::count() == CounterForListNodes::<T, I>::get().unwrap(),
        "wrong list node counter"
    );
    Ok(Vec::new())
}
```

`CounterForListNodes` artık `CountedStorageMap`'in dahili counter'ıyla yönetilir. Migration sadece sayım tutarlılığını doğrular, herhangi bir storage yazımı yoktur.

### 5.3 pallet-im-online Migration v0 → v1 (Özet)

- v0: `ReceivedHeartbeats` → `WrapperOpaque<BoundedOpaqueNetworkState>` (peer_id + external_addresses)
- v1: `ReceivedHeartbeats` → `bool` (sadece heartbeat alındı mı)
- Storage boyutu dramatik küçüldü; içerik değil, varlık önemli

### 5.4 Migration Güvenlik Dersleri — Nova Shield İçin

| Migration Anti-Pattern | Substrate Örneği | Nova Shield Önerisi |
|------------------------|------------------|---------------------|
| `Weight::MAX` döndür | bags-list AddScore | Multi-block migration kullan |
| Eski veriyi sil (refund yok) | offences v0→v1 | Mümkünse dönüştür, silmek son çare |
| `u32::MAX` clear limit | offences clear() | Sayıyı önce ölç, gerçek limit koy |
| pre/post eksik | — | Her migration'da zorunlu |
| Storage version kontrolü eksik | — | `on_chain_version > expected` check şart |

---

## EK KISIM 6: Trait Sistemi — Eksiksiz Analiz

### 6.1 ReportOffence Tam Zinciri

```
im-online::on_before_session_ending()
  → T::ReportUnresponsiveness::report_offence(reporters=[], offence)
    [pallet-offences::Pallet<T> implements ReportOffence]
      → triage_offence_report() → concurrent_offenders listesi
      → offence.slash_fraction(concurrent_count)  → Perbill
      → slash_perbill: Vec<_> = vec![fraction; n]
      → T::OnOffenceHandler::on_offence(concurrent_offenders, slash_perbill, session)
        [pallet-staking implements OnOffenceHandler]
          → validator slash (Perbill * bonded_stake)
          → chilling (validator setinden çıkar)
          → reward reduction
```

### 6.2 SortedListProvider Tam Arayüzü

```rust
pub trait SortedListProvider<AccountId> {
    type Error: Debug;
    type Score: AtLeast32BitUnsigned + Bounded + ...;

    fn iter() -> Box<dyn Iterator<Item = AccountId>>;
    fn iter_from(start: &AccountId) -> Result<Box<dyn Iterator<Item = AccountId>>, Self::Error>;
    fn count() -> u32;
    fn contains(id: &AccountId) -> bool;
    fn on_insert(id: AccountId, score: Self::Score) -> Result<(), Self::Error>;
    fn on_update(id: &AccountId, score: Self::Score) -> Result<(), Self::Error>;
    fn get_score(id: &AccountId) -> Result<Self::Score, Self::Error>;
    fn on_remove(id: &AccountId) -> Result<(), Self::Error>;
    fn unsafe_regenerate(all: impl IntoIterator<Item = AccountId>, score_of: Box<dyn Fn(&AccountId) -> Option<Self::Score>>) -> u32;
    fn unsafe_clear();
    fn lock();
    fn unlock();
    fn range() -> (Self::Score, Self::Score);
    // benchmarks/try-runtime:
    fn score_update_worst_case(who: &AccountId, is_increase: bool) -> Self::Score;
    fn try_state() -> Result<(), TryRuntimeError>;
}
```

Bu trait, bags-list'in election provider ile olan tüm arayüzünü tanımlar. Herhangi bir alternatif implementasyon (örn. heap tabanlı) bags-list'in yerini alabilir — Nova Shield'da farklı bir sıralı liste implementasyonu denenebilir.

### 6.3 OneSessionHandler Lifecycle

```
Session 0 (genesis):
  on_genesis_session(authorities_iter)
    → im-online: Keys set et
    → authority-discovery: Keys + NextKeys set et

Session N başlangıcı:
  on_new_session(changed, validators, queued_validators)
    → im-online: Keys güncelle, HeartbeatAfter = block + half_session
    → authority-discovery: changed=true ise Keys güncelle, NextKeys her zaman güncelle

Session N sonu (on_before_session_ending):
  → im-online: Offline tespiti, UnresponsivenessOffence raporu, storage temizliği
  → authority-discovery: override yok (default boş)

Validator disabled:
  on_disabled(index)
    → im-online: ignore
    → authority-discovery: ignore
```

### 6.4 EstimateNextSessionRotation — Fallback Mekanizması

```rust
// im-online mock (mock.rs:167-192)
impl EstimateNextSessionRotation<u64> for TestNextSessionRotation {
    fn average_session_length() -> u64 {
        // Mock'tan al, yoksa PeriodicSessions'tan hesapla
        MockAverageSessionLength::mutate(|p| p.take())
            .unwrap_or(pallet_session::PeriodicSessions::<Period, Offset>::average_session_length())
    }

    fn estimate_current_session_progress(now: u64) -> (Option<Permill>, Weight) {
        let (estimate, weight) = pallet_session::PeriodicSessions::<...>::estimate_current_session_progress(now);
        let mock = MockCurrentSessionProgress::mutate(|p| p.take());
        (mock.unwrap_or(estimate), weight)
    }
}
```

Bu mock yapısı, im-online testlerinde session progress'in kontrollü manipülasyonuna olanak tanır. Gerçek implementasyonda BABE slot'larına göre hesaplama yapılır.

---

## EK KISIM 7: Güvenlik Analizi ve Edge Cases

### 7.1 im-online Race Condition Koruması

**Problem:** Birden fazla off-chain worker aynı anda heartbeat gönderebilir.

**Çözüm:** `StorageValueRef::persistent` + atomic `mutate` (lib.rs:643-658):
```rust
let res = storage.mutate(|status| {
    match status {
        Ok(Some(status)) if status.is_recent(session_index, now) => {
            Err(OffchainErr::WaitingForInclusion(status.sent_at))
        },
        _ => Ok(HeartbeatStatus { session_index, sent_at: now }),
    }
});
```

`mutate` operasyonu off-chain storage'da atomik CAS (Compare-And-Swap) benzeri davranır. İlk worker lock'u alır, sonrakiler `WaitingForInclusion` hatası alır.

**Başarısız gönderim durumu (lib.rs:668-673):**
```rust
if res.is_err() {
    new_status.sent_at = 0u32.into();  // Sıfırla
    storage.set(&new_status);
}
```

Gönderim başarısız olursa `sent_at=0` set edilir. `is_recent(session_index, 0)` → `0 + 3 > now` yalnızca `now <= 2` ise doğru; bu pratikte neredeyse hiç olmaz. Yani başarısız gönderimde lock hemen serbest bırakılır.

### 7.2 offences Concurrent Count Ekonomisi

**Neden `slash_fraction` toplam değil concurrent count alır?**

Daha büyük koordineli saldırıya daha sert ceza vermek için. 30 validator aynı anda offline olursa, her birinin tek başına offline olmasından çok daha ağır cezalandırılır.

**Örnek (im-online, n=50, slash_fraction formülü):**
```
k=1  validator offline → slash: 0%    (k < n/10+1=6 → threshold negatif)
k=6  validator offline → slash: 0%    (k = 6, threshold = 6-6 = 0 → 0*0.07=0)
k=7  validator offline → slash: 0.42% (threshold=1, x=3/50=0.06, 0.06*0.07=0.0042)
k=10 validator offline → slash: 2.52% (threshold=4, x=12/50=0.24, 0.24*0.07=0.0168) — test'te doğrulanmadı
k=17 validator offline → slash: 4.62% (threshold=11, x=33/50=0.66, 0.66*0.07=0.0462)
k=50 validator offline → slash: 7%    (max, threshold=44, x=132/50=2.64→1.0, 1.0*0.07=0.07)
```

### 7.3 bags-list Stale Score Edge Cases

**Node `score` alanı stale olabilir.** `is_misplaced()` her zaman dışarıdan güncel score alır:
```rust
pub fn is_misplaced(&self, current_score: T::Score) -> bool {
    notional_bag_for::<T, I>(current_score) != self.bag_upper
}
```

Ama `get_score()` node'daki stale değeri döndürür:
```rust
pub fn get_score(id: &T::AccountId) -> Result<T::Score, ListError> {
    Node::<T, I>::get(id).map(|node| node.score()).ok_or(ListError::NodeNotFound)
}
```

`scores()` view function (lib.rs:336-339) bu ayrımı açıkça gösterir:
```rust
pub fn scores(who: T::AccountId) -> (Option<T::Score>, Option<T::Score>) {
    (
        ListNodes::<T, I>::get(&who).map(|node| node.score),  // Stale (cached)
        T::ScoreProvider::score(&who)                          // Gerçek (taze)
    )
}
```

İki değer farklıysa rebag gereklidir.

### 7.4 bags-list Cursor Loop Koruması

```rust
// lib.rs:556-559
Some(next) if to_process.contains(next) => {
    NextNodeAutoRebagged::<T, I>::kill();
    defensive!("Loop detected: {:?} already processed — cursor killed", next);
},
```

`defensive!` macro'su:
- Test ortamında: `panic!` → hata açıkça görülür
- Production'da: `log::error!` → sessiz devam, ama loglanır

Bu, on_idle'da sonsuz döngüyü önleyen son savunma hattıdır.

### 7.5 authority-discovery force_from Truncation Riski

```rust
let bounded_keys = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(
    keys,
    Some("Warning: The session has more validators than expected..."),
);
```

`force_from`, `MaxAuthorities` sınırını aşarsa sessizce **truncate eder** (sadece warning loglar). Bu, bazı validator'ların discovery'den kaybolmasına yol açabilir. Daha güvenli alternatif `try_from` olurdu, ama session lifecycle'ında hata döndürmek zor olduğundan `force_from` tercih edilmiştir.

**Nova Shield için:** `MaxAuthorities` değerini gerçekçi bir üst limite set et, aşılma durumunda validator kaybolmasın.

---

## EK KISIM 8: Nova Shield Uygulama Planı — Detaylı

### 8.1 Heartbeat Sistemi Implementasyon Adımları

**Adım 1: Config Trait**
```rust
pub trait Config: frame_system::Config + CreateAuthorizedTransaction<Call<Self>> {
    type AuthorityId: RuntimeAppPublic + ...;
    type MaxKeys: Get<u32>;
    type NextSessionRotation: EstimateNextSessionRotation<BlockNumberFor<Self>>;
    type ReportUnresponsiveness: ReportOffence<...>;
    type UnsignedPriority: Get<TransactionPriority>;
}
```

**Adım 2: Zamanlama (Polkadot'tan direkt kopyala)**
```rust
const START_HEARTBEAT_RANDOM_PERIOD: Permill = Permill::from_percent(10);
const START_HEARTBEAT_FINAL_PERIOD: Permill = Permill::from_percent(80);

let threshold = progress.saturating_pow(6).saturating_add(residual);
let should_send = progress >= FINAL || (progress >= RANDOM && random <= threshold);
```

**Adım 3: MPC Uyarlaması**
Nova Shield'da validator'lar tek key değil MPC partisipanıdır:
- Her partisipan kendi heartbeat'ini imzalar
- On-chain: t-of-n threshold heartbeat aggregation
- Alternatif: Partisipanlar individual heartbeat gönderir, `t` tanesi yeterliyse "online"

**Adım 4: Slash Hesabı**
```rust
fn slash_fraction(&self, offenders: u32) -> Perbill {
    // Polkadot'tan uyarlanmış: %10 tolerance, max %7
    if let Some(threshold) = offenders.checked_sub(self.validator_set_count / 10 + 1) {
        let x = Perbill::from_rational(3 * threshold, self.validator_set_count);
        x.saturating_mul(Perbill::from_percent(7))
    } else {
        Perbill::default()
    }
}
```

### 8.2 Voter Listesi (Bags-List) Konfigürasyonu

**BagThresholds üretimi:**
```bash
# Polkadot'un generate-bags utility'si kullanılabilir:
# cargo run -p generate-bags -- --total-issuance 1_000_000_000 --minimum-balance 1 200
# 200 çanta, toplam arz 1B, minimum bakiye 1

# Nova Shield örnek (küçük ağ için 64 çanta):
# [1000, 1584, 2511, 3981, 6309, 10000, ...]  (her eşik ~1.585x öncekinden büyük)
```

**MaxAutoRebagPerBlock hesabı:**
```
Nova Shield hedefleri:
- Blok süresi: 2 saniye
- Maksimum validator: 1000
- Tüm listeyi işleme süresi: 10 dakika (300 blok)
- MaxAutoRebagPerBlock = ceil(1000 / 300) = 4 → 5 (güvenlik payı ile)
```

### 8.3 Authority Discovery ZK Uyarlaması

**Standard Polkadot akışı:**
```
Keys storage → authorities() → DHT lookup → PeerRecord → libp2p connect
```

**Nova Shield ZK akışı (MEV koruması):**
```
Keys storage → authorities() → ZK commitment yayınla →
  → Proof: "Ben bu key'e sahibim ve şu IP aralığındayım"
  → Gerçek IP sadece whitelist'e açık
  → Doğrulama sırasında açıkla (commit-reveal)
```

Bu yaklaşım, MEV botlarının validator'ları hedefleyip DoS saldırısı yapmasını engeller.

---

## Nihai Özet

### Kaynak Kod Satır Sayıları (Doğrulanmış)

| Dosya | Satır |
|-------|-------|
| im-online/src/lib.rs | 874 |
| im-online/src/tests.rs | 519 |
| im-online/src/mock.rs | 241 |
| im-online/src/migration.rs | 178 |
| offences/src/lib.rs | 251 |
| offences/src/tests.rs | 269 |
| offences/src/mock.rs | 139 |
| offences/src/migration.rs | 183 |
| authority-discovery/src/lib.rs | 434 |
| bags-list/src/lib.rs | 808 |
| bags-list/src/list/mod.rs | 965 |
| bags-list/src/list/tests.rs | 100+ |
| bags-list/src/mock.rs | 153 |
| bags-list/src/tests.rs | 100+ |
| bags-list/src/migrations.rs | 142 |
| **TOPLAM** | **~5.356 satır** |

### En Kritik Bulgular

1. **im-online offline eşiği:** Tek session miss yeterli. `on_before_session_ending()`'de tespit.
2. **im-online zamanlama:** `p^6 + 1/L` stokastik formül. %80 sonrası koşulsuz.
3. **offences slash:** concurrent offender count ile hesaplanır; toplam değil.
4. **offences dedup:** `report_id = Hash(kind + slot + offender)` — O(1) duplikasyon tespiti.
5. **authority-discovery changed asimetrisi:** Keys (current) sadece changed=true, NextKeys her zaman.
6. **bags-list notional_bag_for:** `partition_point` binary search — O(log n) bag lookup.
7. **bags-list stale score:** Node.score stale olabilir; her zaman ScoreProvider::score() ile doğrula.
8. **bags-list lock:** Snapshot alınırken PendingRebag, sonra on_idle'da işlenir.
