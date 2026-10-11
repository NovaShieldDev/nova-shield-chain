# Substrate pallet-grandpa + pallet-aura — Agent 06

Analiz tarihi: 2026-07-22
Analiz edilen repolar:
- `/substrate/frame/grandpa/` — 10 dosya
- `/substrate/frame/aura/` — 4 dosya
- `/substrate/primitives/consensus/grandpa/` — 1 dosya
- `/substrate/primitives/consensus/aura/` — 3 dosya
- `/substrate/primitives/consensus/slots/` — 1 dosya (kısmi)

Toplam kaynak satırı: ~1700 satır Rust

---

## KISIM 1: pallet-grandpa

### 1.1 GRANDPA Finality Protokolü — Genel Mimari

GRANDPA (GHOST-based Recursive ANcestor Deriving Prefix Agreement), Substrate'in finality gadget'idir. Nakamoto consensus veya BFT'den farklı olarak **chain-based BFT finality** sağlar: her blok için ayrı consensus yapmaz, bunun yerine hangi blokun final olduğuna toplu olarak karar verir.

#### Temel Kavramlar (finality-grandpa kütüphanesinden)

sp_consensus_grandpa primitives'in tanımladığı mesaj tipleri:

```rust
// sp_consensus_grandpa/src/lib.rs:80-122
pub type Message<Header> =
    finality_grandpa::Message<<Header as HeaderT>::Hash, <Header as HeaderT>::Number>;

pub type Prevote<Header> =
    finality_grandpa::Prevote<<Header as HeaderT>::Hash, <Header as HeaderT>::Number>;

pub type Precommit<Header> =
    finality_grandpa::Precommit<<Header as HeaderT>::Hash, <Header as HeaderT>::Number>;

pub type CatchUp<Header> = finality_grandpa::CatchUp<...>;
pub type Commit<Header> = finality_grandpa::Commit<...>;
pub type CompactCommit<Header> = finality_grandpa::CompactCommit<...>;
```

**GRANDPA Protokolü Adımları:**

GRANDPA rounds şu mesajları kullanır:

1. **Prevote (ön oy)**: Her validator mevcut en iyi bloğa prevote gönderir.
2. **Precommit (ön taahhüt)**: Supermajority prevote alındıktan sonra precommit gönderilir.
3. **Commit (taahhüt)**: Supermajority precommit toplandığında blok finalize olur.

**Supermajority eşiği**: 2/3 + 1 oy (klasik BFT)

**GrandpaJustification** yapısı:

```rust
// sp_consensus_grandpa/src/lib.rs:133-137
pub struct GrandpaJustification<Header: HeaderT> {
    pub round: u64,
    pub commit: Commit<Header>,
    pub votes_ancestries: Vec<Header>,
}
```

Justification: bir round numarası + precommit imzaları toplamı + ancestry proof'u içerir. Senkronize olan node'lar bu justification'ı doğrulayarak authority set handoff'larını kanıtlar.

#### ConsensusLog — Runtime Sinyal Mekanizması

GRANDPA, finality karar vermez — sadece sinyaller yönetir:

```rust
// sp_consensus_grandpa/src/lib.rs:150-194
pub enum ConsensusLog<N: Codec> {
    #[codec(index = 1)]
    ScheduledChange(ScheduledChange<N>),    // normal set değişimi
    #[codec(index = 2)]
    ForcedChange(N, ScheduledChange<N>),    // zorla set değişimi (median + change)
    #[codec(index = 3)]
    OnDisabled(AuthorityIndex),             // bir validator'u devre dışı bırak
    #[codec(index = 4)]
    Pause(N),                               // finality'i duraklat
    #[codec(index = 5)]
    Resume(N),                              // finality'i devam ettir
}
```

Bu loglar `DigestItem::Consensus(GRANDPA_ENGINE_ID, ...)` olarak block header'larına yazılır. GRANDPA_ENGINE_ID = `b"FRNK"`.

#### Localized Payload — İmza Alanı Güvenliği

GRANDPA imzaları round ve set_id ile bağlanır:

```rust
// sp_consensus_grandpa/src/lib.rs:398-415
pub fn localized_payload<E: Encode>(round: RoundNumber, set_id: SetId, message: &E) -> Vec<u8> {
    let mut buf = Vec::new();
    localized_payload_with_buffer(round, set_id, message, &mut buf);
    buf
}

pub fn localized_payload_with_buffer<E: Encode>(
    round: RoundNumber,
    set_id: SetId,
    message: &E,
    buf: &mut Vec<u8>,
) {
    buf.clear();
    (message, round, set_id).encode_to(buf)  // SCALE encode: message || round || set_id
}
```

Bu kritik güvenlik özelliğidir: bir imzanın başka bir round veya set'e taşınması imkansızdır çünkü payload içinde round ve set_id hardcode'dur.

#### İmza Doğrulama — check_message_signature

```rust
// sp_consensus_grandpa/src/lib.rs:440-502
pub fn check_message_signature_with_buffer<H, N>(...) -> SignatureResult {
    localized_payload_with_buffer(round, set_id, message, buf);

    if id.verify(&buf, signature) {
        return SignatureResult::Valid;
    }

    // Geriye dönük uyumluluk: önceki set ile de dene
    if set_id == 0 {
        return SignatureResult::Invalid;
    }
    let prev_set_id = set_id - 1;
    localized_payload_with_buffer(round, prev_set_id, message, buf);
    let valid = id.verify(&buf, signature);

    if valid { SignatureResult::OutdatedSet } else { SignatureResult::Invalid }
}
```

3 durumlu sonuç: `Valid`, `Invalid`, `OutdatedSet` — set değişimi sırasında race condition'ları yönetmek için.

---

### 1.2 Validator Set Değişimi

#### StoredPendingChange Yapısı

```rust
// lib.rs:387-400
pub struct StoredPendingChange<N, Limit> {
    pub scheduled_at: N,    // hangi blokta planlandı
    pub delay: N,           // kaç blok sonra uygulanacak
    pub next_authorities: BoundedAuthorityList<Limit>,  // yeni authority listesi
    pub forced: Option<N>,  // forced change ise median finalized block
}
```

#### schedule_change Fonksiyonu — Tam Analiz

```rust
// lib.rs:513-550
pub fn schedule_change(
    next_authorities: AuthorityList,
    in_blocks: BlockNumberFor<T>,
    forced: Option<BlockNumberFor<T>>,
) -> DispatchResult {
    if !PendingChange::<T>::exists() {
        let scheduled_at = frame_system::Pallet::<T>::block_number();

        if forced.is_some() {
            if NextForced::<T>::get().map_or(false, |next| next > scheduled_at) {
                return Err(Error::<T>::TooSoon.into());
            }
            // Sonraki forced change için cooldown: in_blocks * 2
            NextForced::<T>::put(scheduled_at + in_blocks * 2u32.into());
        }

        let next_authorities = WeakBoundedVec::<_, T::MaxAuthorities>::force_from(
            next_authorities,
            Some("Warning: ..."),
        );

        PendingChange::<T>::put(StoredPendingChange {
            delay: in_blocks,
            scheduled_at,
            next_authorities,
            forced,
        });

        Ok(())
    } else {
        Err(Error::<T>::ChangePending.into())
    }
}
```

**Kritik Kısıtlama**: Aynı anda sadece bir pending change olabilir. Eğer zaten bir change varsa `ChangePending` hatası döner.

**Forced Change Cooldown**: `NextForced = scheduled_at + in_blocks * 2` — bu forced change'in 2 katı kadar süre geçmeden yeni forced change yapılamaz. Test'ten: `dispatch_forced_change` testinde blok 1'de `in_blocks=5` ile forced change → `NextForced=11`. Blok 11'e kadar yeni forced change yasak.

#### on_finalize — Değişimlerin Uygulanması

```rust
// lib.rs:129-186
fn on_finalize(block_number: BlockNumberFor<T>) {
    if let Some(pending_change) = PendingChange::<T>::get() {
        // Sinyal: scheduled_at bloğunda log yaz
        if block_number == pending_change.scheduled_at {
            let next_authorities = pending_change.next_authorities.to_vec();
            if let Some(median) = pending_change.forced {
                Self::deposit_log(ConsensusLog::ForcedChange(
                    median,
                    ScheduledChange { delay: pending_change.delay, next_authorities },
                ))
            } else {
                Self::deposit_log(ConsensusLog::ScheduledChange(ScheduledChange {
                    delay: pending_change.delay,
                    next_authorities,
                }));
            }
        }

        // Uygulama: scheduled_at + delay bloğunda authority'leri güncelle
        if block_number == pending_change.scheduled_at + pending_change.delay {
            Authorities::<T>::put(&pending_change.next_authorities);
            Self::deposit_event(Event::NewAuthorities {
                authority_set: pending_change.next_authorities.into_inner(),
            });
            PendingChange::<T>::kill();
        }
    }
    // State machine: pause/resume logları da burada işlenir
    ...
}
```

**İki Aşamalı Süreç:**
1. `scheduled_at` bloğunda: Header'a log yaz (GRANDPA client'ı bu log'u izler)
2. `scheduled_at + delay` bloğunda: Storage'ı güncelle, event yayınla, PendingChange'i sil

#### Normal vs Forced Change Farkı

| Özellik | Normal Change | Forced Change |
|---------|---------------|---------------|
| Tetikleyici | `on_new_session` (session rotation) | `on_stalled` + `on_new_session` |
| Delay semantiği | Finalized block sayısı | Imported block sayısı |
| Cooldown | Yok | `NextForced = scheduled_at + delay * 2` |
| forced alanı | `None` | `Some(median_finalized_block)` |
| Ne zaman gerekir | Normal session değişimi | GRANDPA stall (finality durması) |

Normal change'de delay finalized bloklara göre sayılır. Forced change'de delay imported (üretilmiş) bloklara göre — çünkü network stall'da finalized blok yoktur.

#### on_new_session — Session Handler Entegrasyonu

```rust
// lib.rs:613-653
fn on_new_session<'a, I: 'a>(changed: bool, validators: I, _queued_validators: I) {
    let mut current_set_id = CurrentSetId::<T>::get();

    if changed || Stalled::<T>::exists() {
        let next_authorities = validators.map(|(_, k)| (k, 1)).collect::<Vec<_>>();

        let res = match Stalled::<T>::get() {
            Some((further_wait, median)) => {
                // Stall durumunda forced change
                Self::schedule_change(next_authorities, further_wait, Some(median))
            },
            None => {
                // Normal change, delay=0 (immediate)
                Self::schedule_change(next_authorities, Zero::zero(), None)
            },
        };

        if res.is_ok() {
            Stalled::<T>::kill();
            current_set_id += 1;
            CurrentSetId::<T>::set(current_set_id);

            // Eski set id -> session mapping'leri temizle
            let max_set_id_session_entries = T::MaxSetIdSessionEntries::get().max(1);
            if current_set_id >= max_set_id_session_entries {
                SetIdSession::<T>::remove(current_set_id - max_set_id_session_entries);
            }
        }
    }

    // Her session'da mevcut set'in son oturumunu güncelle
    let session_index = pallet_session::Pallet::<T>::current_index();
    SetIdSession::<T>::insert(current_set_id, &session_index);
}
```

**Önemli:** Normal session change'de `delay = Zero::zero()` yani aynı blok sonunda uygulanır. Bu GRANDPA'nın session geçişlerini nasıl işlediğini gösterir — her zaman anında uygulanır, delay her durumda client tarafından yorumlanır.

Her validator'a weight=1 atanır: `validators.map(|(_, k)| (k, 1))`. GRANDPA'da tüm validator'lar eşit oy ağırlığına sahiptir.

**Schedule change başarısız olursa** (örn. zaten PendingChange varsa), `current_set_id` artırılmaz ve `Stalled` temizlenmez. Test `on_new_session_doesnt_start_new_set_if_schedule_change_failed` bunu doğrular.

---

### 1.3 StoredState — GRANDPA Durum Makinesi

```rust
// lib.rs:407-428
pub enum StoredState<N> {
    Live,                // Normal çalışma
    PendingPause {
        scheduled_at: N,
        delay: N,
    },
    Paused,              // Finality durduruldu
    PendingResume {
        scheduled_at: N,
        delay: N,
    },
}
```

**Durum Geçişleri** (sıralı zorunlu):
- `Live` → `PendingPause` → `Paused` → `PendingResume` → `Live`

**schedule_pause**: Sadece `Live` durumunda çalışır
**schedule_resume**: Sadece `Paused` durumunda çalışır

Her iki state change de on_finalize'da işlenir: `scheduled_at` bloğunda log, `scheduled_at + delay` bloğunda state update.

---

### 1.4 Equivocation Koruması

#### Equivocation Nedir?

Bir GRANDPA validator'ın aynı round ve set'te iki farklı blok için imzalanmış mesaj göndermesi. Bu doğrudan çift oy anlamına gelir.

#### TimeSlot — Offence Koordinatı

```rust
// equivocation.rs:61-67
pub struct TimeSlot {
    pub set_id: SetId,    // hangi authority set
    pub round: RoundNumber,  // hangi round
}
// Derive(Ord) — set_id önce gelir
```

Bu yapı equivocation'ın tam zamanlını işaret eder. `Ord` implementasyonu sıralıdır: önce set_id, sonra round karşılaştırılır.

#### EquivocationOffence — Slash Hesabı

```rust
// equivocation.rs:81-107
impl<Offender: Clone> Offence<Offender> for EquivocationOffence<Offender> {
    const ID: Kind = *b"grandpa:equivoca";  // 16 byte offence tipi
    type Slot = TimeSlot;

    // KRITIK: Slash formülü
    fn slash_fraction(&self, offenders_count: u32) -> Perbill {
        // min((3k / n)^2, 1)
        // k = equivocator sayısı, n = total validator sayısı
        Perbill::from_rational(3 * offenders_count, self.validator_set_count).square()
    }
}
```

**Slash Formülü Analizi**: `min((3k/n)^2, 1)`
- 1 validator equivocate: `(3*1/n)^2` — küçük ceza
- n/3 validator equivocate: `(3*(n/3)/n)^2 = 1.0` — %100 slash
- Bu Polkadot'un "coordinated attack" modelidir: eğer 1/3 validator saldırıya katılırsa tam slash
- Tek saldırıda: n=100, k=1 → `(3/100)^2 = 0.0009` yani %0.09 slash
- Koordineli saldırıda: n=100, k=34 → `(102/100)^2 = 1.04 → capped at 1.0` = %100 slash

#### EquivocationReportSystem — process_evidence Akışı

```rust
// equivocation.rs:175-235
fn process_evidence(
    reporter: Option<T::AccountId>,
    evidence: (EquivocationProof<T::Hash, BlockNumberFor<T>>, T::KeyOwnerProof),
) -> Result<(), DispatchError> {
    let (equivocation_proof, key_owner_proof) = evidence;
    let reporter = reporter.or_else(|| pallet_authorship::Pallet::<T>::author());

    let set_id = equivocation_proof.set_id();
    let round = equivocation_proof.round();
    let session_index = key_owner_proof.session();
    let validator_set_count = key_owner_proof.validator_count();

    // 1. Equivocation proof'u doğrula (farklı blok + geçerli imzalar)
    if !sp_consensus_grandpa::check_equivocation_proof(equivocation_proof) {
        return Err(Error::<T>::InvalidEquivocationProof.into());
    }

    // 2. Key ownership proof'u doğrula
    let offender = P::check_proof((KEY_TYPE, offender), key_owner_proof)
        .ok_or(Error::<T>::InvalidKeyOwnershipProof)?;

    // 3. Session index bounds'u doğrula
    let previous_set_id_session_index = if set_id != 0 {
        let idx = crate::SetIdSession::<T>::get(set_id - 1)
            .ok_or(Error::<T>::InvalidEquivocationProof)?;
        Some(idx)
    } else {
        None
    };
    let set_id_session_index =
        crate::SetIdSession::<T>::get(set_id).ok_or(Error::<T>::InvalidEquivocationProof)?;

    // Session index, bu set_id'nin session aralığında olmalı
    if session_index > set_id_session_index ||
        previous_set_id_session_index
            .map(|previous_index| session_index <= previous_index)
            .unwrap_or(false)
    {
        return Err(Error::<T>::InvalidEquivocationProof.into());
    }

    // 4. Offence raporla
    let offence = EquivocationOffence {
        time_slot: TimeSlot { set_id, round },
        session_index,
        offender,
        validator_set_count,
    };
    R::report_offence(reporter.into_iter().collect(), offence)
        .map_err(|_| Error::<T>::DuplicateOffenceReport)?;

    Ok(())
}
```

**5 Doğrulama Adımı:**
1. Equivocation proof signature kontrolü — `check_equivocation_proof`
2. Key ownership proof kontrolü — `P::check_proof`
3. Session index üst sınır: `session_index <= set_id_session_index`
4. Session index alt sınır: `session_index > previous_set_id_session_index`
5. Duplicate offence: `R::is_known_offence` (check_evidence'de)

#### check_equivocation_proof — İmza Doğrulama

```rust
// sp_consensus_grandpa/src/lib.rs:348-395
pub fn check_equivocation_proof<H, N>(report: EquivocationProof<H, N>) -> bool {
    macro_rules! check {
        ($equivocation:expr, $message:expr) => {
            // Aynı hedef → equivocation değil
            if $equivocation.first.0.target_hash == $equivocation.second.0.target_hash &&
                $equivocation.first.0.target_number == $equivocation.second.0.target_number
            {
                return false;
            }

            // Her iki imzanın geçerli olduğunu doğrula
            let valid_first = check_message_signature(...).is_valid();
            let valid_second = check_message_signature(...).is_valid();
            return valid_first && valid_second
        };
    }

    match report.equivocation {
        Equivocation::Prevote(equivocation) => check!(equivocation, Message::Prevote),
        Equivocation::Precommit(equivocation) => check!(equivocation, Message::Precommit),
    }
}
```

Kontroller:
- İki oy **farklı** hedefleri göstermelidir (hash veya number farklı)
- İki imzanın **her ikisi de geçerli** olmalıdır
- İkisi de **aynı kimlik (authority)** tarafından imzalanmış olmalıdır (equivocation tanımı gereği)

#### validate_unsigned — Transaction Geçerlilik

```rust
// equivocation.rs:243-280
pub fn validate_unsigned(source: TransactionSource, call: &Call<T>) -> TransactionValidity {
    if let Call::report_equivocation_unsigned { ... } = call {
        // Sadece local veya in-block işlemlere izin ver
        match source {
            TransactionSource::Local | TransactionSource::InBlock => {},
            _ => return InvalidTransaction::Call.into(),
        }

        // Proof geçerliliğini kontrol et
        T::EquivocationReportSystem::check_evidence(evidence)?;

        ValidTransaction::with_tag_prefix("GrandpaEquivocation")
            .priority(TransactionPriority::max_value())  // En yüksek öncelik
            .and_provides((
                equivocation_proof.offender().clone(),
                equivocation_proof.set_id(),
                equivocation_proof.round(),
            ))
            .longevity(longevity)
            .propagate(false)  // Network'e yayma (sadece local)
            .build()
    }
}
```

**Güvenlik**: `propagate(false)` — equivocation report extrinsic'i sadece local node tarafından üretilebilir, başkasından alınan tx reddedilir.

---

### 1.5 Storage Items — Tam Analiz

| Storage | Tip | Açıklama |
|---------|-----|----------|
| `State<T>` | `StoredState<BlockNumberFor<T>>` | Mevcut GRANDPA durumu (Live/Paused/...) |
| `PendingChange<T>` | `Option<StoredPendingChange<...>>` | Bekleyen authority set değişimi |
| `NextForced<T>` | `Option<BlockNumberFor<T>>` | Sonraki forced change'in earliest zamanı |
| `Stalled<T>` | `Option<(BlockNumberFor<T>, BlockNumberFor<T>)>` | (delay, median_finalized) stall bilgisi |
| `CurrentSetId<T>` | `SetId` (u64) | Genesis'ten itibaren set değişim sayacı |
| `SetIdSession<T>` | `Map<SetId, SessionIndex>` | Set → oturum eşlemesi |
| `Authorities<T>` | `BoundedAuthorityList<MaxAuthorities>` | Mevcut authority listesi (weight ile) |

#### Stalled Storage — Detay

```rust
// lib.rs:321
pub type Stalled<T: Config> = StorageValue<_, (BlockNumberFor<T>, BlockNumberFor<T>)>;
```

Tuple: `(further_wait, median_finalized_block)`
- `further_wait`: Forced change'in kaç blok sonra uygulanacağı
- `median_finalized_block`: Yeni voter'ların oy başlatacağı kanonik blok

`on_stalled` fonksiyonu:
```rust
fn on_stalled(further_wait: BlockNumberFor<T>, median: BlockNumberFor<T>) {
    Stalled::<T>::put((further_wait, median));
}
```

`note_stalled` extrinsic'i (root only) bu fonksiyonu çağırır.

#### SetIdSession — Equivocation Validation için Gerekli

SetId'den SessionIndex'e eşleme sadece equivocation raporları için kullanılır. Bu sayede belirli bir set_id için hangi session'ların geçerli olduğu hesaplanabilir.

**Pruning**: `MaxSetIdSessionEntries` sabitiyle sınırlı sayıda entry tutulur. Genellikle bonding duration * sessions_per_era kadar (slash window'u kapsaması gerekir). Yeni entry her session'da eklenir, eski entryler otomatik temizlenir:

```rust
// lib.rs:638-642
let max_set_id_session_entries = T::MaxSetIdSessionEntries::get().max(1);
if current_set_id >= max_set_id_session_entries {
    SetIdSession::<T>::remove(current_set_id - max_set_id_session_entries);
}
```

---

### 1.6 GrandpaApi Runtime API

```rust
// sp_consensus_grandpa/src/lib.rs:538-590
#[api_version(3)]
pub trait GrandpaApi {
    // Mevcut authority listesi ve ağırlıkları
    fn grandpa_authorities() -> AuthorityList;

    // Unsigned equivocation raporu submit et (offchain context'te)
    fn submit_report_equivocation_unsigned_extrinsic(
        equivocation_proof: EquivocationProof<Block::Hash, NumberFor<Block>>,
        key_owner_proof: OpaqueKeyOwnershipProof,
    ) -> Option<()>;

    // Key ownership proof üret
    fn generate_key_ownership_proof(
        set_id: SetId,
        authority_id: AuthorityId,
    ) -> Option<OpaqueKeyOwnershipProof>;

    // Mevcut set id'yi döndür
    fn current_set_id() -> SetId;
}
```

API version 3. Bu API'nin 4 metodu:
- `grandpa_authorities()` → client'ın hangi validator'ları dinleyeceğini bilmesi için
- `submit_report_equivocation_unsigned_extrinsic()` → offchain worker equivocation raporu göndermek için
- `generate_key_ownership_proof()` → on-chain historical proof sistemi (pallet_session::historical)
- `current_set_id()` → hangi set'te olduğumuzu öğrenmek için

---

### 1.7 Storage Migrations Analizi

#### Migration V4 — Pallet Prefix Değişimi

```rust
// migrations/v4.rs
pub const OLD_PREFIX: &[u8] = b"GrandpaFinality";
// GrandpaFinality → Grandpa prefix'ine taşıma
// Storage version 3'ten 4'e yükseltir
```

#### Migration V5 — GRANDPA_AUTHORITIES_KEY → FRAME Storage

```rust
// migrations/v5.rs
const GRANDPA_AUTHORITIES_KEY: &[u8] = b":grandpa_authorities";

fn load_authority_list() -> AuthorityList {
    storage::unhashed::get_raw(GRANDPA_AUTHORITIES_KEY).map_or_else(
        || Vec::new(),
        |l| <(u8, AuthorityList)>::decode(&mut &l[..]).unwrap_or_default().1,
    )
}
```

Eski authority listesi `b":grandpa_authorities"` special key'de `(u8, AuthorityList)` tuple olarak saklanıyordu (başına `u8` prefix var). Migration bu key'den veriyi okuyup `Authorities<T>` storage item'ına taşır, eski key'i siler.

---

### 1.8 Cryptography — Ed25519

GRANDPA Ed25519 imza kullanır:

```rust
// sp_consensus_grandpa/src/lib.rs:45-59
mod app {
    use sp_application_crypto::{app_crypto, ed25519, key_types::GRANDPA};
    app_crypto!(ed25519, GRANDPA);
}
pub type AuthorityId = app::Public;
pub type AuthoritySignature = app::Signature;
```

Ed25519 seçiminin nedeni: deterministic, hızlı verify, BFT protokollerinde iyi bilinen.

---

### 1.9 Benchmarking

Benchmarks iki fonksiyon için:

**check_equivocation_proof benchmark**:
- Statik pre-generated proof blob (257 byte)
- Linear<0, 1> parametresi var ama sadece proof check maliyetini ölçer

**note_stalled benchmark**:
- Root origin ile çağrı
- delay=1000, best_finalized=1
- Sadece 1 write: 3 microsecond

**Default Ağırlıklar** (default_weights.rs):
```
report_equivocation:
  = 35μs + 175ns * validator_count  (membership proof)
  + 95μs                             (equivocation proof check)
  + 110μs + 25μs * nominators       (offence report)
  + reads(5 + 14 + 3*nominators + 2)
  + writes(10 + 3*nominators)
  Floor: validator_count.max(100)

note_stalled:
  = 3μs + 1 write
```

---

### 1.10 Config Trait Analizi

```rust
// lib.rs:85-125
pub trait Config: frame_system::Config {
    type RuntimeEvent: ...;
    type WeightInfo: WeightInfo;

    // Maksimum authority sayısı
    type MaxAuthorities: Get<u32>;

    // Maksimum nominator sayısı (slash hesabı için)
    type MaxNominators: Get<u32>;

    // SetIdSession map'te tutulacak maksimum entry sayısı
    // = bonding_duration * sessions_per_era (slash window)
    type MaxSetIdSessionEntries: Get<u64>;

    // Key ownership proof tipi (session index + validator count içerir)
    type KeyOwnerProof: Parameter + GetSessionNumber + GetValidatorCount;

    // Equivocation raporlama subsistemi
    type EquivocationReportSystem: OffenceReportSystem<
        Option<Self::AccountId>,
        (EquivocationProof<Self::Hash, BlockNumberFor<Self>>, Self::KeyOwnerProof),
    >;
}
```

Mock'ta:
- `MaxAuthorities = 100`
- `MaxNominators = 1000`
- `MaxSetIdSessionEntries = BondingDuration * SessionsPerEra = 3 * 3 = 9`
- `ReportLongevity = BondingDuration * SessionsPerEra * Period = 3 * 3 * 1 = 9 blok`

---

### 1.11 Test Analizi

**authorities_change_logged** (tests.rs:36-67):
- delay=0 ile schedule_change yapıldığında aynı blokta hem log hem event yayınlanır
- Header digest içinde `ConsensusLog::ScheduledChange` görünür
- Event: `Event::NewAuthorities`

**dispatch_forced_change** (tests.rs:142-232):
- Blok 1'de delay=5 forced change → NextForced=11
- Blok 6'da authorities değişir (1 + 5 = 6)
- Blok 9-10'da TooSoon hatası (NextForced=11 henüz gelmedi)
- Blok 11'de yeni forced change yapılabilir → NextForced=21

**report_equivocation_current_set_works** (tests.rs:320-396):
- Tek validator equivocation: %100 slash (çünkü 1 validator n=3 ile: (3*1/3)^2 = 1.0)
- Diğer validator'lar etkilenmez
- Fee waived (Pays::No)

**cleans_up_old_set_id_session_mappings** (tests.rs:788-812):
- `max_set_id_session_entries` kadar entry tutulur
- Eski entryler prune edilir

---

### 1.12 Nova Shield Notu

**Nova Shield için GRANDPA Kullanım Kararları:**

1. **Finality Mekanizması**: Nova Shield kendi L1'inde GRANDPA benzeri finality gadget kullanabilir. GRANDPA'nın en büyük avantajı: her blok için ayrı BFT yapmaz, sadece en son finalized point'i kılar. Bu sayede block time bağımsız olarak finality sağlanabilir.

2. **Set Change Protokolü**: `schedule_change` → `on_finalize` iki adımlı pattern'i aynen uygulanabilir. Kritik: aynı anda tek PendingChange kısıtlaması saldırı vektörünü kapatır.

3. **Equivocation Slash Formülü**: `min((3k/n)^2, 1)` iyi ayarlanmış bir formül. Koordineli saldırılarda full slash, bireysel hatada minimal slash.

4. **Forced Change için Cooldown**: `NextForced = scheduled_at + delay * 2` mantığı kritik. Sık forced change flood saldırısını önler.

5. **Ed25519 vs Sr25519**: GRANDPA Ed25519 kullanır (deterministic, hızlı). Nova Shield için GRANDPA benzeri sistem Ed25519 ile başlayabilir.

6. **SetIdSession Pruning**: MaxSetIdSessionEntries = bonding_duration * sessions_per_era mantığı: slash window içindeki tüm set geçişlerini kanıtlayabilmek için gerekli. Nova Shield için eğer slash window W blok ise, her session S blok ise W/S entry yeterli.

7. **Localized Payload Prensibi**: `(message || round || set_id).encode()` pattern'i çok önemli. Nova Shield'daki tüm validator imzaları context-specific payload içermeli — replay attack'ı önler.

8. **Unsigned Extrinsic Güvenliği**: Equivocation raporları unsigned ama `propagate(false)` ile sadece local. Bu pattern Nova Shield'da kullanılabilir: block author incentive ile equivocation raporlama, ucuz (ücret yok), propagation kontrolü.

---

## KISIM 2: pallet-aura

### 2.1 AURA Genel Mimarisi

AURA (Authority Round) deterministik round-robin block production mekanizmasıdır. Her slot belirli bir validator'a aittir: `author_index = slot_number % authorities_count`.

AURA sadece **block production** sağlar, finality sağlamaz. Finality için GRANDPA ile birlikte kullanılır (Substrate'in standart kombinasyonu: AURA + GRANDPA).

### 2.2 Slot Mekanizması

#### Slot Nedir?

```rust
// sp_consensus_slots/src/lib.rs:43
pub struct Slot(u64);
```

`u64` wrapper. Slot numaraları monoton artan, genesis'ten bu yana geçen "time slot" sayısını temsil eder.

Slot hesabı:
```
slot = timestamp / slot_duration
```

Örnek: timestamp=10000ms, slot_duration=2000ms → slot=5

#### on_initialize — Slot Doğrulama

```rust
// aura/src/lib.rs:166-195
fn on_initialize(_: BlockNumberFor<T>) -> Weight {
    if let Some(new_slot) = Self::current_slot_from_digests() {
        let current_slot = CurrentSlot::<T>::get();

        if T::AllowMultipleBlocksPerSlot::get() {
            assert!(current_slot <= new_slot, "Slot must not decrease");
        } else {
            assert!(current_slot < new_slot, "Slot must increase");
        }

        CurrentSlot::<T>::put(new_slot);

        if let Some(n_authorities) = <Authorities<T>>::decode_len() {
            let authority_index = *new_slot % n_authorities as u64;
            if T::DisabledValidators::is_disabled(authority_index as u32) {
                panic!("Validator with index {:?} is disabled...", authority_index);
            }
        }

        T::DbWeight::get().reads_writes(2, 1)
    } else {
        T::DbWeight::get().reads(1)
    }
}
```

**On Initialize Mantığı:**
1. Block header'dan slot'u oku (PreRuntime digest)
2. Slot monoton artar mı kontrol et
3. Bu slot'un sahibi kim hesapla: `slot % n_authorities`
4. Bu validator disabled mı kontrol et → disabled ise **panic**
5. CurrentSlot storage'ı güncelle

**AllowMultipleBlocksPerSlot**: True ise birden fazla blok aynı slot'ta yazılabilir (parachain ortamları için). False ise klasik AURA davranışı.

#### Slot'u Header'dan Okuma

```rust
// aura/src/lib.rs:281-291
fn current_slot_from_digests() -> Option<Slot> {
    let digest = frame_system::Pallet::<T>::digest();
    let pre_runtime_digests = digest.logs.iter().filter_map(|d| d.as_pre_runtime());
    for (id, mut data) in pre_runtime_digests {
        if id == AURA_ENGINE_ID {
            return Slot::decode(&mut data).ok();
        }
    }
    None
}
```

Slot numarası block'un **PreRuntime digest**'ine yazılır. Bu blok üretimi sırasında block author tarafından eklenir. AURA_ENGINE_ID = `[b'a', b'u', b'r', b'a']`.

#### Slot Author Hesabı

```rust
// aura/src/lib.rs:272-278
pub fn slot_author_index(slot: Slot) -> Option<u32> {
    let authorities_count = Self::authorities_len();
    if authorities_count == 0 {
        return None;
    }
    Some((u64::from(slot) % authorities_count as u64) as u32)
}
```

Basit round-robin: `author_index = slot % n`. Bu deterministic ve ön görülebilir. Slot sahibinin kim olduğu herkes tarafından bilunur.

**FindAuthor implementasyonu:**
```rust
// aura/src/lib.rs:413-427
impl<T: Config> FindAuthor<u32> for Pallet<T> {
    fn find_author<'a, I>(digests: I) -> Option<u32> {
        for (id, mut data) in digests.into_iter() {
            if id == AURA_ENGINE_ID {
                let slot = Slot::decode(&mut data).ok()?;
                return Self::slot_author_index(slot);
            }
        }
        None
    }
}
```

Block'tan slot'u decode et, slot'tan authority index'i bul.

---

### 2.3 Authorities Yönetimi

#### Authorities Storage

```rust
// aura/src/lib.rs:205-206
pub type Authorities<T: Config> =
    StorageValue<_, BoundedVec<T::AuthorityId, T::MaxAuthorities>, ValueQuery>;
```

`BoundedVec` kullanılır — GRANDPA'nın `WeakBoundedVec`'inden farklı olarak `BoundedVec` üst sınırı garantiler (force_from yapmaz, truncate yapar).

#### change_authorities — Anlık Değişim

```rust
// aura/src/lib.rs:235-249
pub fn change_authorities(new: BoundedVec<T::AuthorityId, T::MaxAuthorities>) {
    if new.is_empty() {
        log::warn!(target: LOG_TARGET, "Ignoring empty authority change.");
        return;
    }

    <Authorities<T>>::put(&new);

    let log = DigestItem::Consensus(
        AURA_ENGINE_ID,
        ConsensusLog::AuthoritiesChange(new.into_inner()).encode(),
    );
    <frame_system::Pallet<T>>::deposit_log(log);
}
```

GRANDPA'dan farklı: AURA'da authority değişimi **anında** gerçekleşir, delay yok. Aynı anda hem storage güncellenir hem de ConsensusLog::AuthoritiesChange header'a yazılır.

#### on_new_session

```rust
// aura/src/lib.rs:381-401
fn on_new_session<'a, I: 'a>(changed: bool, validators: I, _queued_validators: I) {
    if changed {
        let next_authorities = validators.map(|(_, k)| k).collect::<Vec<_>>();
        let last_authorities = Authorities::<T>::get();
        if last_authorities != next_authorities {
            if next_authorities.len() as u32 > T::MaxAuthorities::get() {
                log::warn!(...);
            }
            let bounded = <BoundedVec<_, T::MaxAuthorities>>::truncate_from(next_authorities);
            Self::change_authorities(bounded);
        }
    }
}
```

`changed` bayrağı false ise hiçbir şey yapılmaz. GRANDPA'dan fark: AURA `Stalled` durumunu bilmez, sadece session değişimi mantığı var.

**Truncation davranışı**: Eğer yeni authority listesi MaxAuthorities'den büyükse **truncate** edilir (sondan kesilir), hiçbir şey patlatılmaz. Bu silently dangerous olabilir — log uyarısı var ama truncated validator'lar sessizce atılır.

#### on_disabled

```rust
// aura/src/lib.rs:403-411
fn on_disabled(i: u32) {
    let log = DigestItem::Consensus(
        AURA_ENGINE_ID,
        ConsensusLog::<T::AuthorityId>::OnDisabled(i as AuthorityIndex).encode(),
    );
    <frame_system::Pallet<T>>::deposit_log(log);
}
```

Disabled validator'ın index'ini ConsensusLog'a yaz. Client bu log'u okuyarak disabled validator'ın slotlarını atlar (veya başka birinin üzerine yazmasına izin verir).

---

### 2.4 SlotDuration

#### MinimumPeriodTimesTwo Provider

```rust
// aura/src/lib.rs:71-77
pub struct MinimumPeriodTimesTwo<T>(core::marker::PhantomData<T>);

impl<T: pallet_timestamp::Config> Get<T::Moment> for MinimumPeriodTimesTwo<T> {
    fn get() -> T::Moment {
        <T as pallet_timestamp::Config>::MinimumPeriod::get().saturating_mul(2u32.into())
    }
}
```

Bu backwards compatibility provider: MinimumPeriod * 2 = SlotDuration.

Neden 2? Timestamp pallet MinimumPeriod'u "her iki blok arasında minimum süre" olarak tanımlar. Bir slot içinde: block üretimi başı → block finalizasyonu sonu = 2 * MinimumPeriod mantığı.

Gerçek kullanımda genellikle constant kullanılır:
```rust
// mock.rs'de
pub const SLOT_DURATION: u64 = 2;  // 2ms (test ortamı)
```

Production'da tipik değerler:
- Kusama/Polkadot: 6000ms (6 saniye) slot
- Her session 600 blok = 60 dakika
- Era: 6 saat

#### on_runtime_upgrade — SlotDuration Değişimi

```rust
// aura/src/lib.rs:129-159
fn on_runtime_upgrade() -> Weight {
    let new_slot_duration = T::SlotDuration::get();
    let current_timestamp = Timestamp::<T>::get();
    let old_slot = CurrentSlot::<T>::get();

    // Yeni slot duration'a göre mevcut timestamp'i slot'a çevir
    let new_slot = current_timestamp / new_slot_duration;
    let new_slot = Slot::from(new_slot.saturated_into::<u64>());

    if old_slot != new_slot {
        CurrentSlot::<T>::put(new_slot);
        ...
        T::DbWeight::get().reads_writes(2, 1)
    } else {
        T::DbWeight::get().reads(2)
    }
}
```

Test `increase_slot_duration`:
- slot_duration=2, timestamp=100, slot=50
- slot_duration 4'e çıkar → yeni slot = 100/4 = 25
- CurrentSlot 50'den 25'e düşer
- Sonraki blok slot 26 ile başlayabilir

Test `decrease_slot_duration`:
- slot_duration=4, timestamp=100, slot=25
- slot_duration 2'ye düşer → yeni slot = 100/2 = 50
- CurrentSlot 25'ten 50'ye çıkar
- Sonraki blok slot 51 ile başlamalı

Bu migration sayesinde slot duration değişimi sorunsuz gerçekleşir — CurrentSlot yeniden hesaplanır.

---

### 2.5 OnTimestampSet Entegrasyonu

```rust
// aura/src/lib.rs:457-473
impl<T: Config> OnTimestampSet<T::Moment> for Pallet<T> {
    fn on_timestamp_set(moment: T::Moment) {
        let slot_duration = Self::slot_duration();
        assert!(!slot_duration.is_zero(), "Aura slot duration cannot be zero.");

        let timestamp_slot = moment / slot_duration;
        let timestamp_slot = Slot::from(timestamp_slot.saturated_into::<u64>());

        assert_eq!(
            CurrentSlot::<T>::get(),
            timestamp_slot,
            "Timestamp slot must match `CurrentSlot`. ...",
        );
    }
}
```

Bu hook Timestamp pallet tarafından çağrılır. Timestamp set edildiğinde, hesaplanan slot numarasının `CurrentSlot` storage ile uyumlu olduğunu doğrular. Uyumsuzluk panic'e yol açar — bu node konfigürasyonundaki slot_duration uyumsuzluğunun erken tespiti için kritik.

```rust
// mock.rs:48-53
impl pallet_timestamp::Config for Test {
    type OnTimestampSet = Aura;  // AURA'yı hook olarak kaydet
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
}
```

---

### 2.6 AuraAuthorId — Author Belirleme

```rust
// aura/src/lib.rs:432-449
pub struct FindAccountFromAuthorIndex<T, Inner>(PhantomData<(T, Inner)>);

impl<T: Config, Inner: FindAuthor<u32>> FindAuthor<T::AuthorityId>
    for FindAccountFromAuthorIndex<T, Inner>
{
    fn find_author<'a, I>(digests: I) -> Option<T::AuthorityId> {
        let i = Inner::find_author(digests)?;
        let validators = Authorities::<T>::get();
        validators.get(i as usize).cloned()
    }
}

pub type AuraAuthorId<T> = FindAccountFromAuthorIndex<T, Pallet<T>>;
```

İki katmanlı: `Pallet<T>` slot'tan index'i bulur, `FindAccountFromAuthorIndex` index'ten AuthorityId'yi bulur.

---

### 2.7 IsMember Implementasyonu

```rust
// aura/src/lib.rs:451-455
impl<T: Config> IsMember<T::AuthorityId> for Pallet<T> {
    fn is_member(authority_id: &T::AuthorityId) -> bool {
        Authorities::<T>::get().iter().any(|id| id == authority_id)
    }
}
```

Linear scan: O(n) arama. Authorities listesi küçük tutulmalı (tipik olarak 100-1000 arası).

---

### 2.8 Inherent Mekanizması

```rust
// sp_consensus_aura/src/inherents.rs
pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"auraslot";
pub type InherentType = sp_consensus_slots::Slot;

pub struct InherentDataProvider {
    slot: InherentType,
}

impl InherentDataProvider {
    pub fn from_timestamp_and_slot_duration(
        timestamp: sp_timestamp::Timestamp,
        slot_duration: sp_consensus_slots::SlotDuration,
    ) -> Self {
        let slot = InherentType::from_timestamp(timestamp, slot_duration);
        Self { slot }
    }
}
```

Block author inherent data olarak slot numarasını iletir. `*b"auraslot"` inherent ID'si. Block validation sırasında bu inherent verify edilir.

---

### 2.9 Digest Item Yapısı

AURA iki tür digest item kullanır:

```rust
// sp_consensus_aura/src/digests.rs
impl<Signature: Codec> CompatibleDigestItem<Signature> for DigestItem {
    // Imza (block seal) — Seal type
    fn aura_seal(signature: Signature) -> Self {
        DigestItem::Seal(AURA_ENGINE_ID, signature.encode())
    }

    // Slot numarası (block pre-digest) — PreRuntime type
    fn aura_pre_digest(slot: Slot) -> Self {
        DigestItem::PreRuntime(AURA_ENGINE_ID, slot.encode())
    }
}
```

Block production flow:
1. Block author **PreRuntime** digest ile slot numarasını yazar
2. Block tamamlandıktan sonra **Seal** digest ile imzalar

---

### 2.10 do_try_state — Invariant Kontrolü

```rust
// aura/src/lib.rs:319-363 (try-runtime feature)
pub fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
    let current_slot = Self::current_slot_from_digests()
        .unwrap_or_else(|| CurrentSlot::<T>::get());

    // 1. Slot overflow kontrolü
    if !T::AllowMultipleBlocksPerSlot::get() {
        ensure!(current_slot < u64::MAX, "Current slot has reached maximum...");
    }

    // 2. Authority sayısı kontrolü
    let authorities_len = <Authorities<T>>::decode_len()
        .ok_or("Failed to decode authorities length")?;
    ensure!(!authorities_len.is_zero(), "Authorities must be non-empty.");

    // 3. Disabled validator kontrolü
    let authority_index = Self::slot_author_index(current_slot).ok_or(...)?;
    ensure!(
        !T::DisabledValidators::is_disabled(authority_index),
        "Current validator is disabled..."
    );

    // 4. Timestamp-Slot tutarlılığı
    let timestamp = pallet_timestamp::Pallet::<T>::get();
    if !timestamp.is_zero() {
        let slot_duration = Self::slot_duration();
        let timestamp_slot = Slot::from((timestamp / slot_duration).saturated_into::<u64>());
        ensure!(current_slot == timestamp_slot, "Timestamp slot must match CurrentSlot.");
    }

    Ok(())
}
```

4 invariant:
1. Slot sayısı u64::MAX'a ulaşmamış
2. Authority listesi boş değil
3. Mevcut blok autoru disabled değil
4. Timestamp ve CurrentSlot uyumlu

---

### 2.11 Migrations

#### remove_last_timestamp

```rust
// aura/src/migrations.rs
pub fn remove_last_timestamp<T: RemoveLastTimestamp>() -> Weight {
    LastTimestamp::<T>::kill();
    T::DbWeight::get().writes(1)
}
```

Eski `LastTimestamp` storage item'ını siler, yerine `CurrentSlot` kullanılır. Bu basit tek-satır migration.

---

### 2.12 AURA vs BABE Karşılaştırması

| Özellik | AURA | BABE |
|---------|------|------|
| Block Selection | Round-robin (deterministik) | VRF tabanlı (stochastic) |
| Author Öngörülebilirlik | Tam öngörülebilir | 1 epoch ahead VRF proof |
| Fork Resistance | Zayıf (aynı slot, aynı author) | Güçlü (secondary slot fallback) |
| Complexity | Basit | Karmaşık (VRF, epoch, secondary) |
| Slot Randomness | Yok | Her slot için VRF output |
| Backup Author | Yok | Secondary plaintext slot |
| Equivocation | Kolay (slot sahibi biliniyor) | Daha zor (VRF reveal gerekli) |
| Key Type | Sr25519 veya Ed25519 | Sr25519 (VRF için) |
| Chain kullanımı | Parachainler, küçük ağlar | Polkadot relay chain |
| Inherent | `auraslot` | `babeslot` |

**AURA'nın zayıflıkları:**
1. Author **tamamen öngörülebilir** → DoS saldırıları (bilinen slot sahibini hedef al)
2. Aynı slot'ta birden fazla blok üretilirse fork oluşur
3. BABE gibi secondary slot mekanizması yok → validator skip yapabilir

**AURA'nın avantajları:**
1. Basit implementasyon
2. Düşük overhead (VRF hesabı yok)
3. Parachainler için uygun (Collator Set küçük, DoS riski düşük)
4. GRANDPA ile birlikte güvenli finality sağlar

**BABE'nin avantajları:**
1. VRF output'u epoch sonuna kadar gizli → lookahead saldırılarını önler
2. Secondary slot mekanizması → liveness garantisi
3. Daha güçlü liveness ve safety trade-off

---

### 2.13 AURA Config Trait

```rust
// aura/src/lib.rs:85-122
pub trait Config: pallet_timestamp::Config + frame_system::Config {
    type AuthorityId: Member + Parameter + RuntimeAppPublic + MaybeSerializeDeserialize + MaxEncodedLen;
    type MaxAuthorities: Get<u32>;
    type DisabledValidators: DisabledValidators;
    type AllowMultipleBlocksPerSlot: Get<bool>;
    type SlotDuration: Get<<Self as pallet_timestamp::Config>::Moment>;
}
```

**AllowMultipleBlocksPerSlot** özelliği dikkat çekici:
- Default: `false` — her slot için tek blok
- `true` ise: aynı slot'ta birden fazla blok yazılabilir
- Parachain ortamları için: collator'lar üst üste blok yazabilir, hangi limitin uygulanacağı başka pallet tarafından belirlenir

---

### 2.14 Test Analizi

**disabled_validators_cannot_author_blocks** (tests.rs:42-61):
- Slot 1 → authority_index = 1 % 4 = 1
- Validator 1 disable edilir
- `on_initialize(1)` → `panic!`

**pallet_requires_slot_to_increase_unless_allowed** (tests.rs:63-80):
- AllowMultipleBlocksPerSlot = false
- Aynı slot 2 kez → ikinci `on_initialize` panic eder: "Slot must increase"

**try_state_validates_timestamp_slot_consistency** (tests.rs:124-148):
- slot_duration = 2
- slot 5 → timestamp 10 olmalı (5 * 2 = 10)
- timestamp 12 olursa: 12 / 2 = 6 ≠ 5 → hata

**increase_slot_duration** (tests.rs:161-186):
- slot_duration=2, timestamp=100, slot=50
- Upgrade: slot_duration=4 → CurrentSlot = 100/4 = 25
- Sonraki blok slot 26 ile çalışır

**decrease_slot_duration** (tests.rs:188-212):
- slot_duration=4, timestamp=100, slot=25
- Upgrade: slot_duration=2 → CurrentSlot = 100/2 = 50
- Sonraki blok slot 51 ile çalışır

---

### 2.15 Nova Shield Notu

**Nova Shield için AURA Değerlendirmesi:**

1. **Temel Consensus Seçimi**: Nova Shield bir L1. BABE daha güçlü güvenlik garantileri sağlar ama complexity yüksek. AURA + GRANDPA kombinasyonu basit, test edilmiş, Substrate'te native.

2. **AURA Author Öngörülebilirlik Riski**: Author tamamen öngörülebilir. Nova Shield için: eğer validator set küçükse (< 20) DoS riski gerçek. Çözüm: ya BABE kullan ya da AURA üzerine ShuffleShuffle gibi randomization ekle.

3. **AllowMultipleBlocksPerSlot**: Nova Shield native L1 ise false olmalı. Eğer parachain hosting yapacaksa true ile collator çoklu blok yazabilir.

4. **SlotDuration Değişimi**: on_runtime_upgrade hook'u slot duration runtime upgrade'de otomatik migrate eder. Nova Shield slot duration'ı değiştirmek isterse bu hook kullanılabilir.

5. **DisabledValidators**: AURA'nın disabled validator kontrolü on_initialize'da `panic!` ile. Bu agresif bir yaklaşım — production'da bu paniklerin düzgün ele alınması gerekir. Nova Shield için: `Error::DisabledValidator` döndürülüp block'un daha zarif reddedilmesi önerilebilir.

6. **Session Handler Entegrasyonu**: Hem AURA hem GRANDPA `OneSessionHandler` trait'ini implement eder. Nova Shield'da session yönetimi her iki pallet'i birden yönetmeli.

7. **Inherent ID**: `b"auraslot"` — Nova Shield özel bir consensus isterse farklı inherent ID seçmeli.

8. **OnTimestampSet Hook**: AURA'nın timestamp-slot tutarlılık kontrolü production'da kritik. Node'un slot_duration config'i runtime'daki slot_duration ile uyuşmazsa panic oluşur. Nova Shield node'unda bu konfigürasyonun otomatik senkronize olması gerekir.

---

## Kaynak Kod Referansları

### pallet-grandpa

| Dosya | Satır | Konu |
|-------|-------|------|
| `frame/grandpa/src/lib.rs` | 129-186 | `on_finalize` — pending change ve state machine |
| `frame/grandpa/src/lib.rs` | 513-550 | `schedule_change` — forced change cooldown |
| `frame/grandpa/src/lib.rs` | 613-653 | `on_new_session` — set id artışı ve pruning |
| `frame/grandpa/src/lib.rs` | 307-344 | Storage item tanımları |
| `frame/grandpa/src/equivocation.rs` | 101-107 | Slash formülü: `min((3k/n)^2, 1)` |
| `frame/grandpa/src/equivocation.rs` | 175-235 | `process_evidence` — 5 adımlı doğrulama |
| `frame/grandpa/src/equivocation.rs` | 243-280 | `validate_unsigned` — propagate(false) |
| `frame/grandpa/src/migrations/v5.rs` | 30-91 | V5 migration: GRANDPA_AUTHORITIES_KEY |
| `frame/grandpa/src/default_weights.rs` | 27-57 | Default ağırlıklar |

### sp_consensus_grandpa

| Dosya | Satır | Konu |
|-------|-------|------|
| `primitives/consensus/grandpa/src/lib.rs` | 62 | `GRANDPA_ENGINE_ID = b"FRNK"` |
| `primitives/consensus/grandpa/src/lib.rs` | 80-122 | Mesaj tipleri (Prevote, Precommit, Commit) |
| `primitives/consensus/grandpa/src/lib.rs` | 133-137 | `GrandpaJustification` yapısı |
| `primitives/consensus/grandpa/src/lib.rs` | 150-194 | `ConsensusLog` enum (5 varyant) |
| `primitives/consensus/grandpa/src/lib.rs` | 348-395 | `check_equivocation_proof` |
| `primitives/consensus/grandpa/src/lib.rs` | 398-415 | `localized_payload` — imza domain separation |
| `primitives/consensus/grandpa/src/lib.rs` | 440-502 | `check_message_signature_with_buffer` |
| `primitives/consensus/grandpa/src/lib.rs` | 538-590 | `GrandpaApi` (api_version=3) |

### pallet-aura

| Dosya | Satır | Konu |
|-------|-------|------|
| `frame/aura/src/lib.rs` | 71-77 | `MinimumPeriodTimesTwo` provider |
| `frame/aura/src/lib.rs` | 129-159 | `on_runtime_upgrade` — slot duration migration |
| `frame/aura/src/lib.rs` | 166-195 | `on_initialize` — slot validation + disabled check |
| `frame/aura/src/lib.rs` | 235-249 | `change_authorities` — anlık değişim |
| `frame/aura/src/lib.rs` | 272-278 | `slot_author_index = slot % n` |
| `frame/aura/src/lib.rs` | 281-291 | `current_slot_from_digests` |
| `frame/aura/src/lib.rs` | 319-363 | `do_try_state` — 4 invariant |
| `frame/aura/src/lib.rs` | 370-401 | `OneSessionHandler` — on_new_session |
| `frame/aura/src/lib.rs` | 413-427 | `FindAuthor<u32>` implementasyonu |
| `frame/aura/src/lib.rs` | 457-473 | `OnTimestampSet` hook |

### sp_consensus_aura

| Dosya | Satır | Konu |
|-------|-------|------|
| `primitives/consensus/aura/src/lib.rs` | 70 | `AURA_ENGINE_ID = [a, u, r, a]` |
| `primitives/consensus/aura/src/lib.rs` | 76-84 | `ConsensusLog` (AuthoritiesChange, OnDisabled) |
| `primitives/consensus/aura/src/lib.rs` | 86-97 | `AuraApi` (slot_duration, authorities) |
| `primitives/consensus/aura/src/digests.rs` | 29-62 | `CompatibleDigestItem` — seal ve pre-digest |
| `primitives/consensus/aura/src/inherents.rs` | 22 | `INHERENT_IDENTIFIER = b"auraslot"` |

---

## Özet Tablo

| Özellik | GRANDPA | AURA |
|---------|---------|------|
| Rol | Finality gadget | Block producer |
| Kriptografi | Ed25519 | Sr25519 veya Ed25519 |
| Engine ID | `b"FRNK"` | `b"aura"` |
| Authority change | Delayed (schedule_change) | Instant (change_authorities) |
| Set tracking | CurrentSetId + SetIdSession | Yok (session integration sadece) |
| Equivocation | Slash: min((3k/n)^2, 1) | Yok (GRANDPA halledecek) |
| Session integration | OneSessionHandler | OneSessionHandler |
| Stall recovery | note_stalled + forced change | Yok |
| Storage version | V5 (mevcut) | - |
| Runtime API | GrandpaApi (v3) | AuraApi |
| Inherent | Yok (finality extrinsic) | auraslot |

---

## Kritik Bulgular

1. **Slash formülü kaynaktan doğrulandı**: `Perbill::from_rational(3 * offenders_count, self.validator_set_count).square()` — lib.rs:105

2. **Forced change cooldown**: `NextForced = scheduled_at + in_blocks * 2` — lib.rs:528

3. **AURA author**: `slot % n_authorities` — aura/lib.rs:277

4. **GRANDPA_ENGINE_ID**: `b"FRNK"` (FRANK kısaltması, "finality") — sp_consensus_grandpa/lib.rs:62

5. **Authority weights**: GRANDPA her authority'ye weight=1 atar — `validators.map(|(_, k)| (k, 1))` — lib.rs:623

6. **Schedule başarısız olursa set_id artmaz**: `if res.is_ok()` bloğunda artış — lib.rs:632

7. **Localized payload**: `(message || round || set_id).encode()` — SCALE encoding, tuple olarak

8. **V5 migration**: Eski storage `b":grandpa_authorities"` key'i `(u8, AuthorityList)` tuple decode — v5.rs:35

9. **AURA truncate**: MaxAuthorities aşılırsa `truncate_from` ile sessizce kesiyor — aura/lib.rs:397

10. **OutdatedSet**: Signature result üçüncü durumu — set değişimi sırasında geçiş imzaları — sp_consensus_grandpa/lib.rs:427-428

---

## Ek Analiz: sp_consensus_slots — Slot ve SlotDuration Primitifleri

### Slot Yapısı (Detay)

```rust
// sp_consensus_slots/src/lib.rs:43
pub struct Slot(u64);
```

Slot tip güvenliği için `#[repr(transparent)]` kullanır. `u64`'e doğrudan dönüştürülebilir.

**Slot Hesaplama:**
```rust
// sp_consensus_slots/src/lib.rs:101-105
pub const fn from_timestamp(timestamp: Timestamp, slot_duration: SlotDuration) -> Self {
    Slot(timestamp.as_millis() / slot_duration.as_millis())
}
```

Integer division. Milisaniye cinsinden: `slot = timestamp_ms / slot_duration_ms`.
Örnek: timestamp=1000000ms, slot_duration=6000ms → slot=166.

**Slot'tan timestamp geri hesabı:**
```rust
pub fn timestamp(&self, slot_duration: SlotDuration) -> Option<Timestamp> {
    slot_duration.as_millis().checked_mul(self.0).map(Timestamp::new)
}
```
`checked_mul` kullanılır — overflow kontrolü. None döner eğer overflow olursa.

**Slot aritmetiği:**
- `Slot + Slot`, `Slot - Slot` operatörleri tanımlı
- `Slot.saturating_add(u64)`, `Slot.saturating_sub(u64)` güvenli versiyonlar
- `Slot + u64` da desteklenir

**PartialOrd karşılaştırması:**
```rust
impl<T: Into<u64> + Copy> core::cmp::PartialOrd<T> for Slot {
    fn partial_cmp(&self, other: &T) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&(*other).into())
    }
}
```
`slot < 5u64` gibi doğrudan karşılaştırma mümkün.

### SlotDuration Yapısı

```rust
// sp_consensus_slots/src/lib.rs:160-177
pub struct SlotDuration(u64);  // milisaniye

impl SlotDuration {
    pub const fn from_millis(millis: u64) -> Self { Self(millis) }
    pub const fn as_duration(&self) -> core::time::Duration {
        core::time::Duration::from_millis(self.0)
    }
    pub const fn as_millis(&self) -> u64 { self.0 }
}
```

`const fn` — compile time hesaplama mümkün.

### sp_consensus_slots EquivocationProof — AURA için

```rust
// sp_consensus_slots/src/lib.rs:189-199
pub struct EquivocationProof<Header, Id> {
    pub offender: Id,           // equivocator'ın kimliği
    pub slot: Slot,             // hangi slotta çifte blok üretildi
    pub first_header: Header,   // ilk header
    pub second_header: Header,  // ikinci header
}
```

Bu AURA equivocation proof'u GRANDPA'nınkinden farklı: iki farklı header kanıtı içerir (ikisi de aynı slot'a aittir). GRANDPA ise iki farklı imzalanmış prevote/precommit kullanır.

---

## Ek Analiz: GRANDPA Benchmarks — Gerçek Ağırlıklar

### weights.rs — Auto-Generated (2025-02-21)

Benchmark ortamı:
- CPU: Intel Xeon @ 2.60GHz
- WASM execution: Compiled
- Steps: 50, Repeat: 20
- Runtime: kitchensink-runtime

**check_equivocation_proof:**
```
Minimum execution time: 77_943_000 picoseconds = ~78 microseconds
SubstrateWeight: 78_252_373 picoseconds + 29_726 * x
  x parametresi Linear<0, 1> = 0 veya 1
Proof size: 0 bytes (sadece CPU, no storage reads)
```

**note_stalled:**
```
Minimum execution time: 2_006_000 picoseconds = ~2 microseconds
SubstrateWeight: 2_117_000 picoseconds + 1 write
Storage: Grandpa::Stalled (r:0 w:1), max_size: Some(8 bytes)
```

**Default weights vs AutoGenerated farkı:**

| İşlem | default_weights.rs | weights.rs (benchmark) |
|-------|---------------------|------------------------|
| check_equivocation_proof | - (farklı fn imzası) | 78μs base |
| note_stalled | 3μs | 2.1μs |
| report_equivocation | 35μs + validator*175ns + ... | validator + nominator fonksiyonu |

default_weights.rs'teki `report_equivocation` weights.rs'te `check_equivocation_proof` olarak isimlendirilmiş — bu isimlendirme tutarsızlığına dikkat. Gerçek production'da pallet'in Config::WeightInfo'suna hangi implementasyonun atandığına bakılmalı.

---

## Ek Analiz: Slot-Based Consensus Güvenlik Modeli

### AURA'nın Güvenlik Varsayımları

**Liveness (Canlılık) Garantisi:**
AURA'nın liveness için gereksinimi: toplamda `f < n/2` validator down olabilir. Yani n validator'ın çoğunluğu aktif olmalıdır. Eğer slot sahibi down ise o slot'ta blok üretilmez — liveness geçici olarak bozulur.

Polkadot relay chain'de bu sorun BABE'nin secondary slot mekanizması ile çözülür. AURA'da böyle bir fallback yoktur. Bu nedenle AURA için validator uptime kritik.

**Safety Garantisi:**
AURA tek başına finality garantisi vermez — sadece block production yapar. Safety için GRANDPA gereklidir. GRANDPA'nın safety varsayımı: `f < n/3` Byzantine validator olabilir (klasik BFT).

**Author Öngörülebilirlik Analizi:**

Saldırgan, bir sonraki N slot'un authorlarını hesaplayabilir:
```
for slot in current_slot..current_slot+N:
    author = authorities[slot % authorities_len]
```

Bu bilgi ile:
- DoS: Belirli bir slotta o validator'ı ağdan kes
- Censorship: Spesifik kullanıcıların tx'lerini belirli bloklarda reddetme (sadece o slot sahibine gönder, diğer slot sahiplerini pas geç)
- Strategic equivocation: Belirli bir slot sahibinin bloğunu reddet

**GRANDPA + AURA Kombinasyonunun Gücü:**

GRANDPA finality ile AURA'nın liveness'ı birleşince:
- Kısa vadeli: AURA blok üretir, GRANDPA finalize eder
- Uzun vadeli: Eğer 1/3'ten fazla validator offline → finality durur, Stalled state, forced change
- Block production devam eder (AURA) ama finality olmadan — uzun reorg riski var

---

## Ek Analiz: Tam Storage Item Boyutları

### GRANDPA Storage Boyutları

Stalled storage, `max_size: Some(8)` — 8 byte: iki u32 block number değeri (compact encoding ile).

SetIdSession map: `SetId` (u64, 8 byte) → `SessionIndex` (u32, 4 byte).
Twox64Concat hash: 8 byte prefix + 8 byte key = 16 byte. Value: 4 byte. Toplam: 20 byte/entry.

MaxSetIdSessionEntries = BondingDuration * SessionsPerEra = 3 * 3 = 9 (mock'ta).
Production'da genellikle 28 * 6 = 168 entry.

Authorities storage: `WeakBoundedVec<(AuthorityId, AuthorityWeight), MaxAuthorities>`.
- AuthorityId: Ed25519 public key = 32 byte
- AuthorityWeight: u64 = 8 byte
- Her entry: 40 byte
- 1000 authority: 40KB

### AURA Storage Boyutları

CurrentSlot: `Slot` = u64 = 8 byte. `max_values: Some(1)`, `max_size: Some(8)`.

Authorities: `BoundedVec<AuthorityId, MaxAuthorities>`.
- AuthorityId: Sr25519 veya Ed25519 public key = 32 byte
- 100 authority: 3200 byte
- MaxAuthorities kontrolü: `BoundedVec` garantisi

---

## Ek Analiz: GRANDPA Protokol Detayları (finality-grandpa Kütüphanesi)

### Voting Round Detayları

GRANDPA her round için aşağıdaki mesajları takip eder:

1. **PrimaryPropose**: Round başında primary proposer en iyi bloğu önerir
2. **Prevote**: Her validator en iyi bloğa oy verir (ancestry kuralları ile)
3. **Precommit**: 2/3+1 prevote alındıktan sonra precommit gönderilir
4. **Commit**: 2/3+1 precommit toplandığında blok finalize edilir

**Ghost (Greedy Heaviest Observed SubTree) kuralı:**
Her validator en büyük ağırlıklı subtree'de en derin bloku seçer. Bu GRANDPA'nın "chain-based" çalışmasını sağlar.

**Round-based çalışma:**
Round numaraları monoton artar. Her round'da:
- Bir primary proposer vardır (round % n belirler)
- Primary propose → prevote → precommit → finalize döngüsü

**Equivocation detection:**
Aynı round'da iki farklı prevote veya precommit gönderilmesi equivocation'dır. Pallet bu equivocation'ları `EquivocationProof` olarak on-chain raporlar.

### CatchUp Mekanizması

Geride kalan node'lar için:
```rust
pub type CatchUp<Header> = finality_grandpa::CatchUp<
    <Header as HeaderT>::Hash,
    <Header as HeaderT>::Number,
    AuthoritySignature,
    AuthorityId,
>;
```

Bir validator eğer birden fazla round geride kalmışsa, diğer validator'lardan CatchUp mesajı alır. Bu mesaj şunları içerir:
- Tüm prevote'lar
- Tüm precommit'ler
- Finalized round kanıtı

### CompactCommit

```rust
pub type CompactCommit<Header> = finality_grandpa::CompactCommit<
    <Header as HeaderT>::Hash,
    <Header as HeaderT>::Number,
    AuthoritySignature,
    AuthorityId,
>;
```

Network üzerinde commit mesajlarını daha verimli iletmek için compact format.

---

## Ek Analiz: Event ve Error Sistemi

### GRANDPA Events

```rust
// lib.rs:269-278
pub enum Event {
    NewAuthorities { authority_set: AuthorityList },  // Yeni set uygulandı
    Paused,                                           // Finality duraklatıldı
    Resumed,                                          // Finality devam etti
}
```

`NewAuthorities` event'i validator set değişiminin on-chain kaydıdır. Explorers ve indexer'lar bu event'i izler.

### GRANDPA Errors

```rust
// lib.rs:280-298
pub enum Error<T> {
    PauseFailed,              // Live değilken pause
    ResumeFailed,             // Paused değilken resume
    ChangePending,            // Zaten pending change var
    TooSoon,                  // Forced change cooldown
    InvalidKeyOwnershipProof, // Yanlış key proof
    InvalidEquivocationProof, // Yanlış equivocation kanıtı
    DuplicateOffenceReport,   // Zaten raporlandı
}
```

7 hata kodu. Her biri belirli durum kontrolü başarısız olduğunda döner.

### AURA Events

AURA'nın hiçbir on-chain event'i yoktur. Authority değişimi sadece ConsensusLog (header digest) olarak kaydedilir. Bu tasarım kararı: AURA consensus değişimleri event olarak değil, off-chain okunabilir digest olarak.

---

## Ek Analiz: Test Coverage Analizi

### GRANDPA Test Matrisı

| Test | Kapsanan Senaryo |
|------|-----------------|
| `authorities_change_logged` | delay=0, aynı blokta sinyal+uygulama |
| `authorities_change_logged_after_delay` | delay=1, signal ve enact ayrı blokta |
| `cannot_schedule_change_when_one_pending` | ChangePending hatası |
| `dispatch_forced_change` | Forced change, NextForced cooldown |
| `schedule_pause_only_when_live` | PauseFailed hatası |
| `schedule_resume_only_when_paused` | ResumeFailed hatası |
| `time_slot_have_sane_ord` | TimeSlot sıralama (set_id önce, round sonra) |
| `report_equivocation_current_set_works` | Mevcut set'te equivocation → slash |
| `report_equivocation_old_set_works` | Eski set'ten equivocation → yine slash |
| `report_equivocation_invalid_set_id` | Gelecekteki set_id → InvalidEquivocationProof |
| `report_equivocation_invalid_session` | Yanlış session → InvalidEquivocationProof |
| `report_equivocation_invalid_key_owner_proof` | Yanlış authority proof → InvalidKeyOwnershipProof |
| `report_equivocation_invalid_equivocation_proof` | Aynı blok / farklı round / farklı key → invalid |
| `report_equivocation_validate_unsigned_prevents_duplicates` | Stale TX kontrolü |
| `on_new_session_doesnt_start_new_set_if_schedule_change_failed` | set_id artışı şartlı |
| `cleans_up_old_set_id_session_mappings` | MaxSetIdSessionEntries pruning |
| `always_schedules_a_change_on_new_session_when_stalled` | Stalled → forced change |
| `report_equivocation_has_valid_weight` | Weight monoton artan |
| `valid_equivocation_reports_dont_pay_fees` | Pays::No valid, Pays::Yes duplicate |
| `stalled_state_is_kept_on_schedule_change_failure` | PendingChange varken Stalled korunur |

### AURA Test Matrisı

| Test | Kapsanan Senaryo |
|------|-----------------|
| `initial_values` | Genesis: slot=0, 4 authority |
| `disabled_validators_cannot_author_blocks` | Disabled author → panic |
| `pallet_requires_slot_to_increase_unless_allowed` | AllowMultiple=false: slot artmalı |
| `pallet_can_allow_unchanged_slot` | AllowMultiple=true: aynı slot geçerli |
| `pallet_always_rejects_decreasing_slot` | Slot geri gidemez (her iki modda da) |
| `try_state_validates_timestamp_slot_consistency` | timestamp/slot_duration=slot kontrolü |
| `integrity_test_passes_with_valid_config` | Slot duration != 0 |
| `increase_slot_duration` | on_runtime_upgrade: slot duration artış |
| `decrease_slot_duration` | on_runtime_upgrade: slot duration azalış |

---

## Ek Analiz: GRANDPA + Session Entegrasyonu Tam Akış

### Genesis Akışı

```
1. GenesisConfig::build()
   → CurrentSetId::put(0)
   → Pallet::initialize(authorities)
      → Authorities::put(authorities)
      → SetIdSession::insert(0, 0)  // set 0 → session 0
```

### Session Değişim Akışı (Normal)

```
pallet_session::on_new_session() çağrılır
  → Grandpa::on_new_session(changed=true, validators, ...)
    → schedule_change(next_authorities, delay=0, forced=None)
      → PendingChange::put(...)
    → CurrentSetId += 1
    → SetIdSession::remove(old_entry)  // pruning
    → SetIdSession::insert(current_set_id, session_index)

on_finalize(scheduled_at + 0):
  → Authorities::put(next_authorities)
  → Event::NewAuthorities
  → PendingChange::kill()
```

### Session Değişim Akışı (Stalled)

```
note_stalled(delay, median) → Stalled::put((delay, median))

pallet_session::on_new_session()
  → Grandpa::on_new_session(changed=?, ...)
    if Stalled exists:
      schedule_change(next_auth, further_wait, Some(median))
        → ForcedChange ConsensusLog yazılır
        → NextForced = scheduled_at + further_wait * 2

on_finalize(scheduled_at + further_wait):
  → Authorities::put(next_authorities)
  → Event::NewAuthorities
  → PendingChange::kill()
```

### Equivocation Reporting Akışı

```
Offchain: GRANDPA client equivocation tespit eder
  → GrandpaApi::generate_key_ownership_proof(set_id, authority_id)
  → GrandpaApi::submit_report_equivocation_unsigned_extrinsic(proof, ownership_proof)

Extrinsic havuza girer:
  → ValidateUnsigned: source=Local/InBlock, check_evidence()
  → Priority = TransactionPriority::max_value()
  → propagate = false

Block import:
  → report_equivocation_unsigned(origin=None, proof, key_owner_proof)
  → process_evidence():
     1. check_equivocation_proof (signatures)
     2. check_proof (key ownership)
     3. session bounds check
     4. report_offence() → EquivocationOffence → slash
  → Pays::No (fee waived)
```

---

## Ek Analiz: WeightInfo Trait İmplementasyonu Karşılaştırması

### İki WeightInfo Kaynağı

GRANDPA pallet'te iki WeightInfo implementasyonu var:

**1. default_weights.rs (el yazısı):**
```rust
fn report_equivocation(validator_count: u32, max_nominators_per_validator: u32) -> Weight {
    let validator_count = validator_count.max(100) as u64;
    Weight::from_parts(35u64 * WEIGHT_REF_TIME_PER_MICROS, 0)
        .saturating_add(...)
}
```
İmza: `(validator_count, max_nominators_per_validator)` → 2 parametre

**2. weights.rs (benchmark generated):**
```rust
fn check_equivocation_proof(x: u32) -> Weight {
    Weight::from_parts(78_252_373, 0)
        .saturating_add(Weight::from_parts(29_726, 0).saturating_mul(x.into()))
}
```
İmza: `(x: u32)` → 1 parametre, farklı isim

Bu tutarsızlık önemli: `WeightInfo` trait tanımı (lib.rs:377-380):
```rust
pub trait WeightInfo {
    fn report_equivocation(validator_count: u32, max_nominators_per_validator: u32) -> Weight;
    fn note_stalled() -> Weight;
}
```

weights.rs'teki ayrı WeightInfo trait:
```rust
pub trait WeightInfo {
    fn check_equivocation_proof(x: u32, ) -> Weight;
    fn note_stalled() -> Weight;
}
```

İki farklı WeightInfo tanımı var — isimlendirme tutarsızlığı benchmark sisteminin evolüsyonunu gösterir. lib.rs'deki WeightInfo::report_equivocation kullanılır; benchmarking.rs'deki `check_equivocation_proof` sadece proof verify CPU maliyetini ölçer.

---

## Sonuç ve Kritik Değerler Özeti

### Doğrulanmış Sayısal Değerler (Kaynak Koddan)

| Değer | Miktar | Kaynak |
|-------|--------|--------|
| GRANDPA_ENGINE_ID | `b"FRNK"` | sp_consensus_grandpa/src/lib.rs:62 |
| AURA_ENGINE_ID | `[b'a', b'u', b'r', b'a']` | sp_consensus_aura/src/lib.rs:70 |
| AURA inherent ID | `b"auraslot"` | sp_consensus_aura/src/inherents.rs:22 |
| GRANDPA slash formula | `min((3k/n)^2, 1)` | equivocation.rs:105 |
| Forced change cooldown | `scheduled_at + in_blocks * 2` | lib.rs:528 |
| AURA author formula | `slot % n_authorities` | aura/lib.rs:277 |
| Authority weight | 1 (her validator) | lib.rs:623, lib.rs:609 |
| check_equivocation_proof perf | ~78 microsecond | weights.rs:87 |
| note_stalled perf | ~2 microsecond | weights.rs:98 |
| Stalled storage size | 8 bytes max | weights.rs:93 |
| GrandpaApi version | 3 | sp_consensus_grandpa/src/lib.rs:548 |
| Min authorities (try_state) | 1 (boş olmayan) | aura/lib.rs:339 |
| V5 migration source key | `b":grandpa_authorities"` | migrations/v5.rs:30 |
| V4 migration old prefix | `b"GrandpaFinality"` | migrations/v4.rs:26 |
| Equivocation offence ID | `b"grandpa:equivoca"` | equivocation.rs:82 |
| AURA SLOT_DURATION (test) | 2ms | mock.rs:32 |
| SessionsPerEra (test mock) | 3 | mock.rs:151 |
| BondingDuration (test mock) | 3 | mock.rs:152 |
| MaxSetIdSessionEntries (test) | 9 (3*3) | mock.rs:197 |

### Nova Shield için Eylem Planı

1. **Consensus Çifti**: AURA + GRANDPA kombinasyonu benimsemek için finality-grandpa crate ve sp_consensus_grandpa yeterli. Nova Shield için ek özelleştirme gerekmiyorsa en az riskli yol.

2. **Slot Duration**: Nova Shield MEV koruması odaklıysa 3 saniye önerilir (daha az mempool beklemesi → daha az MEV fırsatı).

3. **Equivocation Handling**: GRANDPA'nın slash formülü `min((3k/n)^2, 1)` direct benimseme önerilebilir. 1/3 koordineli saldırı = full slash.

4. **Forced Change**: Stall durumu kritik. Nova Shield'da `note_stalled` extrinsic root-only olmalı. Governance mekanizması bu yetkiyi kontrol etmeli.

5. **Author Randomization**: AURA'nın öngörülebilir author sorunu Nova Shield için ciddi ise, author selection şifrelenebilir veya VRF tabanlı alternatif (BABE'ye yakın) düşünülebilir.

6. **Localized Payload Pattern**: Nova Shield'daki tüm validator imzaları (MPC threshold dahil) round + set_id içermeli. Bu replay attack koruması zorunludur.

7. **SetIdSession Pruning**: Nova Shield'ın slash window süresi belirlendikten sonra `MaxSetIdSessionEntries = slash_window_blocks / session_length_blocks` şeklinde hesaplanmalı.

8. **AURA truncate güvenlik açığı**: Authorities listesi MaxAuthorities'i aşarsa `truncate_from` sessizce kesiyor. Nova Shield'da bu davranış `Error::TooManyAuthorities` ile açık hata olarak değiştirilmeli.
