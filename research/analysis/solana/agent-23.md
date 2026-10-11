# Solana Analiz — Agent 23
## Kapsam: runtime/src/ Kalan Dosyalar (~20 dosya)

**Analiz tarihi:** 2026-07-22
**Toplam satır:** ~5153 satır (20 dosya)

---

## Dosya Listesi ve Satır Sayıları

| Dosya | Satır |
|-------|-------|
| stakes.rs | 1092 |
| stake_account.rs | 112 |
| stake_history.rs | 118 |
| stake_weighted_timestamp.rs | 812 |
| status_cache.rs | 525 |
| commitment.rs | 345 |
| bank_utils.rs | 61 |
| bank_client.rs | 394 |
| root_bank_cache.rs | 98 |
| genesis_utils.rs | 285 |
| non_circulating_supply.rs | 332 |
| builtins.rs | 110 |
| loader_utils.rs | 316 |
| transaction_batch.rs | 202 |
| compute_budget_details.rs | 183 |
| epoch_rewards_hasher.rs | 99 |
| static_ids.rs | 16 |
| inline_spl_associated_token_account.rs | 6 |
| lib.rs | 47 |
| **TOPLAM** | **~5153** |

---

## Detaylı Analiz

---

### [stakes.rs] (1092 satır)

**Amaç:** Stake ve vote account'larının epoch bazlı önbelleğini (cache) yönetir; node stake dağılımını türetmek için kullanılır.

**Struct/Enum/Trait Listesi:**

- `Error` (satır 28-42): `#[derive(Debug, Error)]` — 6 varyant
  - `InvalidDelegation(Pubkey)` — delegation tutarsızlığı
  - `InvalidStakeAccount(#[from] stake_account::Error)` — otomatik dönüşüm
  - `StakeAccountNotFound(Pubkey)`
  - `VoteAccountMismatch(Pubkey)`
  - `VoteAccountNotCached(Pubkey)`
  - `VoteAccountNotFound(Pubkey)`

- `InvalidCacheEntryReason` (satır 44-49): `#[derive(Debug, Clone, PartialEq, Eq, ToPrimitive)]` — 3 varyant
  - `Missing`, `BadState`, `WrongOwner`
  - `ToPrimitive` derive: metrics'e i64 olarak aktarılabilir

- `StakeAccount` (satır 51): type alias — `stake_account::StakeAccount<Delegation>` (tam account state + stake state)

- `StakesCache` (satır 53-54): `#[derive(Default, Debug, AbiExample)]`
  - Tek field: `RwLock<Stakes<StakeAccount>>`
  - `pub(crate)` — dışarıya kapalı

- `Stakes<T: Clone>` (satır 182-198): `#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]`
  - `vote_accounts: VoteAccounts` — tüm vote account cache
  - `stake_delegations: ImHashMap<Pubkey, T>` — **im crate**: immutable/persistent HashMap — clone-on-write semantik
  - `unused: u64` — geriye dönük uyumluluk için korunan alan
  - `epoch: Epoch` — geçerli epoch
  - `stake_history: StakeHistory` — geçmiş stake seviyeleri
  - Generic T: `Delegation` (serialization için) veya `StakeAccount` (runtime cache için)

- `StakesEnum` (satır 207-211): `#[derive(Debug, AbiExample)]`
  - `Accounts(Stakes<StakeAccount>)` — runtime temsili
  - `Delegations(Stakes<Delegation>)` — serialization temsili
  - Amaç: epoch sınırında `Stakes<StakeAccount>` → `Stakes<Delegation>` dönüşüm maliyetini ertelemek

**Kritik Fonksiyonlar:**

- `StakesCache::check_and_store()` (satır 65-125): Ana giriş noktası — her account değiştiğinde çağrılır
  - Lamports == 0 ise: vote veya stake cache'den kaldır
  - Vote program sahibi ise: `VoteStateVersions::is_correct_size_and_initialized()` kontrol et → `VoteAccount::try_from()` → `upsert_vote_account()`
  - Stake program sahibi ise: `StakeAccount::try_from()` → `upsert_stake_delegation()`
  - **Tasarım notu (satır 72-74):** Hesap sahibi değişirse cache'in temizlenmesi gerektiğine dair açık bir TODO yorum var — bu bilinen bir edge case

- `StakesCache::activate_epoch()` (satır 127-135): Epoch geçişini tetikler — Stakes::activate_epoch'a delege eder

- `StakesCache::update_stake_accounts()` (satır 137-148): Epoch reward dağıtımından sonra stake cache'i günceller

- `StakesCache::handle_invalid_keys()` (satır 150-172): DashMap üzerinden gelen geçersiz vote key'lerini temizler, metrics yayar

- `Stakes<StakeAccount>::new()` (satır 228-282): `Stakes<Delegation>` → `Stakes<StakeAccount>` dönüşümü
  - Her stake için `get_account()` callback ile tam account yükler
  - Delegation tutarlılığını doğrular
  - Vote account'larının accounts-db ile uyumunu kontrol eder
  - Stake delegation'ların referans verdiği ama önbelleklenmemiş vote account'ları tespit eder

- `Stakes::activate_epoch()` (satır 288-322): Epoch sınırı işleme
  - Rayon thread pool ile paralel: her delegation için `stake_activating_and_deactivating()` hesapla
  - `StakeActivationStatus` lerini toplam al → `stake_history.add(epoch, entry)`
  - Sonraki epoch için `refresh_vote_accounts()` çağır

- `Stakes::calculate_stake()` (satır 324-338): Belirli bir voter_pubkey'e gönderilmiş toplam aktif stake'i hesaplar

- `Stakes::vote_balance_and_staked()` (satır 341-347): Vote account lamport'ları + delegation stake miktarlarının toplamı

- `Stakes::remove_vote_account()` (satır 349-351): Vote account'ı cache'den kaldır

- `Stakes::remove_stake_delegation()` (satır 353-368): Stake delegation'ı kaldır ve bağlı voter'ın stake değerini güncelle

- `Stakes::upsert_vote_account()` (satır 370-392): Vote account ekle/güncelle
  - Mevcut varsa önce kaldır → mevcut stake değerini al
  - Yeni account ile (stake, account) çiftini ekle

- `Stakes::upsert_stake_delegation()` (satır 394-420): Stake delegation ekle/güncelle
  - Eski delegation varsa: voter değişip değişmediğini veya stake değerinin değişip değişmediğini kontrol et
  - Sadece değişiklik varsa vote_accounts'u güncelle → gereksiz yeniden hesaplamayı önle

- `Stakes::update_stake_accounts()` (satır 422-453): Epoch reward batch işlemi
  - Rayon ile paralel: StakeReward listesini → StakeAccount listesine dönüştür
  - Mevcut delegations ile birleştir, sıfır lamport'luları filtrele
  - `refresh_vote_accounts()` ile vote account stake dağılımını yeniden hesapla

- `Stakes::highest_staked_node()` (satır 459-462): En çok stake alan validator'ın node public key'ini döndür

- `StakesEnum::vote_accounts()` ve `staked_nodes()` (satır 465-479): enum üzerinden polymorphik erişim

- `serde_stakes_enum_compat` modülü (satır 534-560): Geriye dönük uyumlu serialization
  - `serialize`: StakesEnum::Accounts → Stakes<Delegation>'a dönüştürüp seri hale getirir
  - `deserialize`: Her zaman Delegations varyantı olarak yükler

- `refresh_vote_accounts()` (satır 562-601): Vote account stake dağılımını yeniden hesapla
  - `thread_pool.install()` ile paralel fold: her delegation → voter_pubkey → toplam stake
  - İç yardımcı `merge()` fonksiyonu: büyük HashMap'e küçük olanı merge et (optimize edilmiş)
  - Vote account'ları güncelleme: her vote_pubkey için delegated_stake al

**Unsafe Bloklar:** Yok. Rayon/Arc/RwLock kullanılıyor, tüm eşzamanlılık safe API ile.

**Test Coverage:** (satır 603-1092) — 7 test
- `test_stakes_basic`: Temel upsert, lamport değişimi, sıfır lamport kaldırma
- `test_stakes_highest`: En çok stake alan node tespiti
- `test_stakes_vote_account_disappear_reappear`: Vote account geçici olarak silinip geri gelmesi
- `test_stakes_change_delegate`: Stake delegation'ın bir vote'dan diğerine taşınması
- `test_stakes_multiple_stakers`: Birden fazla stake'in aynı voter'a işaret etmesi
- `test_activate_epoch`: Epoch geçişi, stake geçmişi hesabı
- `test_stakes_not_delegate`: Stake account olmayan account'ın delegation'ı silmesi
- `test_vote_balance_and_staked_empty / _normal`: Toplam balance hesabı
- `test_serde_stakes_enum_compat`: Round-trip serializasyon doğrulaması (rastgele verilerle)

**Tasarım Kararları:**
1. **im::HashMap (immer):** Persistent/immutable veri yapısı kullanımı — clone maliyeti O(1) (Arc paylaşımı), mutasyon gerektiğinde copy-on-write. `stake_delegations` için ideal çünkü sık klonlanıyor
2. **Generic T:** `Stakes<Delegation>` (küçük, serializable) vs `Stakes<StakeAccount>` (büyük, runtime-optimized) ayrımı. Eski kod yolu için geriye dönük uyumluluk ve yeni yüksek performanslı yol birlikte var
3. **StakesEnum:** Epoch sınırında dönüşüm maliyetini erteleme mekanizması — `EpochStakes` içinde iki formattan birini tutabilir
4. **Rayon paralel processing:** activate_epoch ve update_stake_accounts'ta iş yükü thread pool'a dağıtılır
5. **Vote account stake değeri:** vote_accounts içinde (stake, account) çifti olarak tutulur — ayrı bir lookup yapılmasına gerek yok

**Nova Shield İçin Öğrenilen:**
- Stake cache iki katmanlı: iç (StakeAccount — tam state) ve dış seri (Delegation — sadece delegation verisi). Nova Shield için aynı pattern: runtime cache ile snapshot serializasyonu ayrılmalı
- `im::HashMap` Rust'ta persistent veri yapısı için üretim kanıtlı bir seçim — sık klonlanan veri yapılarında kullanılabilir
- Epoch sınırı paralel işleme: Rayon ile fold+reduce pattern — her validator için bağımsız hesaplamalar paralel yapılabilir
- Validator stake ağırlıklı hesaplamalar her epoch'ta yeniden yapılmalı; snapshot ile yeniden yükleme sırasında tutarlılık doğrulama kritik

---

### [stake_account.rs] (112 satır)

**Amaç:** `AccountSharedData` ve `StakeStateV2`'yi birleştiren tip-güvenli wrapper; generic T ile hangi stake state'inin bekleneceğini zorla.

**Struct/Enum/Trait Listesi:**

- `StakeAccount<T>` (satır 19-24): `#[derive(Clone, Debug, Default)]`
  - `account: AccountSharedData` — tam hesap verisi
  - `stake_state: StakeStateV2` — deserialize edilmiş state
  - `_phantom: PhantomData<T>` — T tipi için marker (sıfır alan, sadece tip güvenliği)

- `Error` (satır 26-35):
  - `InstructionError(#[from] InstructionError)` — state deserializasyon hatası
  - `InvalidDelegation(Box<StakeStateV2>)` — state Delegation değilse
  - `InvalidOwner(Pubkey)` — hesap stake program'a ait değilse

**Kritik Fonksiyonlar:**

- `impl<T> StakeAccount<T>::lamports()` (satır 38-41): `#[inline]` — account.lamports() delegate
- `impl<T> StakeAccount<T>::stake_state()` (satır 43-46): `#[inline]` — stake_state referansı
- `impl StakeAccount<Delegation>::delegation()` (satır 49-55): `unwrap()` ile çağrılır — güvenli çünkü TryFrom sırasında Delegation olduğu zaten doğrulandı. Yorum: "Safe to unwrap here because StakeAccount<Delegation> will always only wrap a stake-state which is a Delegation"

- `TryFrom<AccountSharedData> for StakeAccount<Delegation>` (satır 58-74):
  - Owner kontrolü: `solana_stake_program::id()` olmalı
  - `account.state()` ile StakeStateV2 deserialize et
  - `stake_state.delegation().is_none()` ise `InvalidDelegation` hatası
  - Sadece Delegation varyantlı stake state'leri kabul eder

- `From<StakeAccount<T>> for (AccountSharedData, StakeStateV2)` (satır 76-81): Destructure helper

- `PartialEq<StakeAccount<S>> for StakeAccount<T>` (satır 83-92): Cross-type eşitlik — sadece account ve stake_state karşılaştırılır, phantom type farklı olabilir

- `AbiExample for StakeAccount<Delegation>` (satır 94-112): `#[cfg(RUSTC_WITH_SPECIALIZATION)]` — ABI testing için örnek üretir

**Unsafe Bloklar:** Yok.

**Test Coverage:** Doğrudan test yok; `stakes.rs` içindeki entegrasyon testleri ile dolaylı test edilir.

**Tasarım Kararları:**
1. `PhantomData<T>` ile tip-güvenli state makinesi: `StakeAccount<Delegation>` sadece Delegation stake'i wrap edebilir, `StakeAccount<()>` herhangi bir stake state'i wrap edebilir
2. `pub(crate)` görünürlüğü: `stake_account` modülü `stakes.rs` tarafından type alias olarak yeniden ihraç edilir
3. `#[inline]` dekoratörleri: Hot path — sık çağrılan küçük fonksiyonlar için compiler hint

**Nova Shield İçin Öğrenilen:**
- Generic PhantomData pattern ile tip-güvenli durum makinesi: bir account'ın hangi state'de olduğunu compile time'da zorlamak mümkün — Nova Shield'da da benzer pattern uygulanabilir
- Owner kontrolü her `TryFrom` dönüşümünde yapılmalı — özellikle program hesapları için

---

### [stake_history.rs] (118 satır)

**Amaç:** SDK'nın `StakeHistory` tipini, gereksiz klonlamayı önlemek için clone-on-write semantiği ile sarmalar.

**Struct/Trait Listesi:**

- `StakeHistory` (satır 9-10): `#[derive(Default, Clone, PartialEq, Eq, Debug, Deserialize, Serialize, AbiExample)]`
  - Tek field: `Arc<StakeHistoryInner>` — Arc içinde SDK'nın stake history'si

- `StakeHistoryInner` (satır 26): type alias — `solana_sdk::stake_history::StakeHistory`

**Kritik Implementasyonlar:**

- `Deref for StakeHistory` (satır 12-17): `&Arc<StakeHistoryInner>` → `&StakeHistoryInner` — tüm SDK metotları transparan erişilebilir

- `DerefMut for StakeHistory` (satır 19-23): `Arc::make_mut(&mut self.0)` — **Clone-on-Write implementasyonu**
  - Eğer Arc'ın tek referansı varsa: in-place mutasyon
  - Birden fazla referans varsa: deep clone → mut referans ver
  - Bu şekilde klonlanmış `StakeHistory`'lerin gereksiz yere deep copy yapmadan paylaşımı sağlanır

**Test Coverage:** (satır 28-118)
- `test_stake_history_is_cow`: Arc pointer eşitliği ile COW davranışını doğrular
  - Clone + değişiklik yok → aynı Arc ptr (`Arc::ptr_eq`)
  - Clone + değişiklik var → farklı Arc ptr
- `test_stake_history_serde`: Outer ↔ Inner tip serializasyon uyumluluğu
  - Outer → serialize → Inner deserialize: aynı data
  - Inner → serialize → Outer deserialize: aynı data

**Tasarım Kararları:**
1. COW semantiği: `StakeHistory` epoch başına kopyalanır ama çoğu durumda değişmeden kullanılır. Arc + DerefMut, derin kopya maliyetini erteleyerek büyük stake geçmişlerinde bellek ve CPU tasarrufu sağlar
2. Serialization şeffaflığı: Inner ve Outer tipler aynı binary formatı üretir — snapshot uyumluluğu

**Nova Shield İçin Öğrenilen:**
- COW wrapper pattern: Sık klonlanan ama nadiren değiştirilen büyük veri yapıları için `Arc<T>` + `DerefMut` ile `Arc::make_mut` idealdir — Nova Shield'daki validator state geçmişleri için bu pattern kullanılabilir

---

### [stake_weighted_timestamp.rs] (812 satır)

**Amaç:** Validator stake ağırlıklı medyan zaman damgası hesaplama — ağ saatini stake dağılımına göre belirler.

**Struct/Const Listesi:**

- Kaldırılan sabitler (satır 13-15): `_MAX_ALLOWABLE_DRIFT_PERCENTAGE = 50`, `_MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW = 80` — prefix `_` ile "obsolete" işaretlenmiş ama ABI uyumluluğu için kaldırılmamış

- `MAX_ALLOWABLE_DRIFT_PERCENTAGE_FAST: u32 = 25` (satır 17): `pub(crate)` — POH'dan %25 daha hızlı gidebilir
- `MAX_ALLOWABLE_DRIFT_PERCENTAGE_SLOW_V2: u32 = 150` (satır 18): `pub(crate)` — POH'dan %150 daha yavaş gidebilir

- `MaxAllowableDrift` (satır 20-24): `#[derive(Copy, Clone)]`
  - `fast: u32` — POH tahmininden daha hızlı olabilecek maksimum yüzde sapma
  - `slow: u32` — POH tahmininden daha yavaş olabilecek maksimum yüzde sapma

**Kritik Fonksiyonlar:**

- `calculate_stake_weighted_timestamp()` (satır 26-101): Ana hesaplama fonksiyonu
  - **İmza:** Generic `I, K, V, T` — `BTreeMap` ve `HashMap` gibi farklı koleksiyonlarla çalışır
  - `unique_timestamps: I` — vote pubkey → (slot, timestamp) iteratoru
  - `stakes: &HashMap<Pubkey, (u64, T)>` — her vote account için (stake_miktarı, hesap)
  - `slot: Slot`, `slot_duration: Duration` — mevcut slot ve slot süresi
  - `epoch_start_timestamp: Option<(Slot, UnixTimestamp)>` — POH bounding için epoch başı
  - `max_allowable_drift: MaxAllowableDrift` — hızlı ve yavaş drift limitleri
  - `fix_estimate_into_u64: bool` — negatif timestamp cast bug fix flag

  **Algoritma detayı:**
  ```
  1. Her vote için: timestamp_slot → mevcut slot offset hesapla
     estimate = validator_timestamp + (current_slot - timestamp_slot) * slot_duration
  2. BTreeMap'e ekle: estimate → toplam_stake (aynı estimate'e sahip birden fazla validator olabilir)
  3. BTreeMap sıralıdır → artan sırada dolaş, kümülatif stake topla
     stake_accumulator > total_stake / 2 olduğunda → medyan bulundu
  4. POH bounding:
     - epoch_start_timestamp varsa:
       poh_estimate = epoch_start_timestamp + (slot - epoch_start_slot) * slot_duration
       estimate_offset = estimate - epoch_start_timestamp
       - estimate > poh + max_slow: estimate = poh + max_slow
       - estimate < poh - max_fast: estimate = poh - max_fast
  ```

  **Bug fix (satır 73-77):** `fix_estimate_into_u64`
  - `false` (eski davranış): `estimate.saturating_sub(epoch_start_timestamp) as u64` — eğer estimate < epoch_start_timestamp ise, i64 negatif değer u64'e cast'ta büyük pozitif sayıya dönüşür → yanlış bounding (bound max'a iter)
  - `true` (düzeltilmiş): `(estimate as u64).saturating_sub(epoch_start_timestamp as u64)` — doğru sıfıra saturate

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 103-812) — 5 kapsamlı test
- `test_calculate_stake_weighted_timestamp_uses_median`: Medyan seçimini doğrula
  - Düşük stake'li aykırı değerler timestamp'i etkilememeli
  - i64::MAX stake'li tek validator bile, çoğunluk başka bir değerdeyse, medyanı etkileyemez
  - 50%+ stake kontrolü olan validator istediği zaman damgasını belirleyebilir
- `test_calculate_stake_weighted_timestamp_poh`: POH bounding testi
  - %25 toleransın üstü → clamp'le
  - %25 toleransın altı → clamp'le
  - %25 içinde → değiştirme
- `test_calculate_stake_weighted_timestamp_levels`: Çift tolerans testi (25% vs 50%)
- `test_calculate_stake_weighted_timestamp_fast_slow`: Asimetrik tolerans (25% fast, 50% slow)
- `test_calculate_stake_weighted_timestamp_early`: Negatif timestamp bug fix doğrulaması
  - `fix_estimate_into_u64 = false`: epoch başından önce timestamp → yanlış bound max
  - `fix_estimate_into_u64 = true`: doğru davranış

**Tasarım Kararları:**
1. **Stake ağırlıklı medyan:** Validator sayısına göre değil, stake miktarına göre ağırlıklı medyan — 50%+1 stake kontrolü şart
2. **BTreeMap**: Otomatik sıralama sağlar — medyan hesabı için O(n log n) insert, O(n) tarama
3. **POH bounding**: Ağ saatinin gerçek zamandan çok uzaklaşmasını önlemek için iki yönlü clamp
4. **Asimetrik drift**: Saat ileri gidebilir (%150 yavaş) ama geri gidemez (%25 hızlı) — blok sürelerine etkiyi asimetrik yönet

**Nova Shield İçin Öğrenilen:**
- Validator stake ağırlıklı medyan: Nova Shield'da zaman damgası veya fiyat oracle verisi için aynı pattern uygulanabilir. BTreeMap + kümülatif stake yeterli
- Asimetrik drift limitleri (hızlı/yavaş ayrı): Zaman yönetiminde iki yönlü esneklik — consensus zamanlaması için önemli
- Bug fix pattern: `fix_estimate_into_u64` gibi feature flag ile eski davranış korunurken yeni davranışa geçiş — nova shield özellik bayraklarına model
- Toplam stake 0 ise `None` dön — boş set edge case'i her zaman ele al

---

### [status_cache.rs] (525 satır)

**Amaç:** Transaction deduplication cache — blockhash bazlı, slot/fork bazlı transaction durumu sorgulama.

**Type Aliases:**

- `ForkStatus<T> = Vec<(Slot, T)>` (satır 20): Bir transaction'ın hangi slot'larda işlendiği
- `KeySlice = [u8; CACHED_KEY_SIZE]` (satır 21): `CACHED_KEY_SIZE = 20` — transaction key'in 20 byte dilimi
- `KeyMap<T> = HashMap<KeySlice, ForkStatus<T>>` (satır 22): key slice → fork durumları
- `Status<T> = Arc<Mutex<HashMap<Hash, (usize, Vec<(KeySlice, T)>)>>>` (satır 24): Bir slot'ta işlenen her blockhash için key listesi
- `KeyStatusMap<T> = HashMap<Hash, (Slot, usize, KeyMap<T>)>` (satır 27): ana cache: blockhash → (en yüksek slot, key_index, key_map)
- `SlotDeltaMap<T> = HashMap<Slot, Status<T>>` (satır 31): slot → o slot'ta eklenen key'ler (snapshot için)
- `SlotDelta<T> = (Slot, bool, Status<T>)` (satır 35): (slot, is_root, status) — snapshot/replay için

**Const:**
- `MAX_CACHE_ENTRIES: usize = MAX_RECENT_BLOCKHASHES` (satır 16): Kaç blockhash geçmişini tut
- `CACHED_KEY_SIZE: usize = 20` (satır 17): Key dilim boyutu (bytes)

**Struct/Trait Listesi:**

- `StatusCache<T: Serialize + Clone>` (satır 37-43): `#[derive(Clone, Debug, AbiExample)]`
  - `cache: KeyStatusMap<T>` — ana arama yapısı
  - `roots: HashSet<Slot>` — kesinleşmiş slot'lar (her zaman geçerli ancestor)
  - `slot_deltas: SlotDeltaMap<T>` — snapshot/replay için delta log
  - Default: `roots` = {0} — genesis slot her zaman root

**Kritik Fonksiyonlar:**

- `clear_slot_entries()` (satır 83-117): Belirli bir slot'un tüm kayıtlarını temizle
  - slot_deltas'tan slot'u al
  - Her blockhash girdisi için: cache içinden o slot'a ait key entry'leri kaldır
  - Eğer key_list boşalırsa: entry'yi kaldır; blockhash boşalırsa: blokhash entry'yi kaldır
  - Panic: "Blockhash must exist if it exists in self.slot_deltas" — tutarsızlık tespit mekanizması

- `get_status()` (satır 119-143): Ana sorgulama
  - Blockhash cache'de bulunmazsa: None
  - Key slice al: `key.as_ref()[index..index+20]` — `arrayref::array_ref!` makrosu
  - Fork'lar arasında: `ancestors.contains_key(f) || self.roots.get(f).is_some()` — ancestor veya root ise kabul et
  - **Önemli:** key_index rastgele seçilir — kasıtlı "fuzzy" arama

- `get_status_any_blockhash()` (satır 145-163): Tüm bilinen blockhash'lerde arama — yavaş, dikkatli kullanılmalı

- `add_root()` (satır 166-170): Slot'u root olarak ekle → `purge_roots()` çağır

- `insert()` (satır 177-195): Yeni transaction kaydı ekle
  - Blockhash için key_index: `thread_rng().gen_range(0..max_key_index+1)` — **rastgele** offset
  - `insert_with_slice()` çağır

- `purge_roots()` (satır 197-205): Root sayısı MAX_CACHE_ENTRIES'i aşarsa: en küçük root'u kaldır
  - `cache.retain()`: o root'tan önce oluşan tüm blockhash entry'leri sil
  - `slot_deltas.retain()`: aynı şekilde temizle

- `clear()` (satır 207-216): Test için tüm cache içeriğini temizle (root'ları korur)

- `root_slot_deltas()` (satır 219-230): Snapshot için: tüm root slot'ların delta'larını döndür

- `append()` (satır 232-248): SlotDelta listesini mevcut cache'e uygula — "replay" mekanizması

- `from_slot_deltas()` (satır 250-255): Sıfırdan: slot_deltas'tan yeni StatusCache oluştur

- `insert_with_slice()` (satır 257-279): İç implementasyon
  - `cache` → `slot_deltas` çift yazma: her insert her iki veri yapısına da gider

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 282-524) — 11 test
- `test_empty_has_no_sigs`: Boş cache'de arama
- `test_find_sig_with_ancestor_fork`: Ancestor listesinde olan slot'ta bulma
- `test_find_sig_without_ancestor_fork`: Ancestor olmayan slot → None
- `test_find_sig_with_root_ancestor_fork`: Root olarak işaretlenmiş slot → bulunur
- `test_insert_picks_latest_blockhash_fork`: MAX_CACHE_ENTRIES root sonrası bile bulunabilirlik
- `test_root_expires`: MAX_CACHE_ENTRIES+1 root → eski root'lar ve girişler silinir
- `test_clear_signatures_sigs_are_gone`: clear() sonrası arama başarısız
- `test_clear_signatures_insert_works`: clear() sonrası insert çalışır
- `test_signatures_slice`: Key slice indeksleme doğrulaması
- `test_slot_deltas`: root_slot_deltas → from_slot_deltas round-trip
- `test_roots_deltas`: Birden fazla slot, MAX_CACHE_ENTRIES purge sonrası delta tutarlılığı
- `test_age_sanity`: `MAX_CACHE_ENTRIES <= MAX_RECENT_BLOCKHASHES`
- `test_clear_slot_signatures`: Slot bazlı kısmi temizlik
- `test_different_sized_keys`: 20 byte'tan kısa key'lerde güvenli arama

**Tasarım Kararları:**
1. **Rastgele key offset:** Her blockhash için farklı bir 20-byte key dilimi seçilir. Hedef: DoS saldırısı — bir saldırgan key'lerini aynı slice'a map'leyerek cache performansını düşüremez
2. **Çift veri yapısı (cache + slot_deltas):** cache hız için, slot_deltas snapshot/replay için. Her insert her ikisini de günceller — tutarlılık garanti
3. **KeySlice = 20 byte:** Signature 64 byte, 20 byte dilim yeterli — çarpışma riski düşük ama mükemmel değil (bu tasarımsal bir tradeoff, key yeterince uzun olduğunda güvenli)
4. **Purge on root addition:** Her root eklemesinde `purge_roots()` çağrılır — O(n) ama root seyrek eklenir

**Nova Shield İçin Öğrenilen:**
- Transaction deduplication için slot-aware cache tasarımı: blockhash + fork/ancestor yapısı Nova Shield'da da gerekli
- Rastgele key offset: DoS saldırılarına karşı basit ama etkili önlem
- SlotDelta + Status çift yazma: snapshot uyumluluğu için kritik — transaction durumunu snapshot'a dahil etmek için
- `MAX_CACHE_ENTRIES = MAX_RECENT_BLOCKHASHES` doğrudan bağlantı — geçmişte kaç blockhash kabul edileceği ile cache boyutu aynı

---

### [commitment.rs] (345 satır)

**Amaç:** Blok taahhüt (commitment) seviyelerini izler — her slot için ne kadar stake'in kaç confirmation gördüğünü takip eder.

**Const:**
- `VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64` (satır 7): 2/3 çoğunluk eşiği

**Type Aliases:**
- `BlockCommitmentArray = [u64; MAX_LOCKOUT_HISTORY + 1]` (satır 9): Her confirmation seviyesi için stake miktarı
  - `MAX_LOCKOUT_HISTORY = 31` → dizi boyutu 32
  - İndeks 0: 1 confirmation, İndeks 30: 31 confirmation, İndeks 31 (son): rooted

**Struct Listesi:**

- `BlockCommitment` (satır 11-14): `#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]`
  - Tek field: `commitment: BlockCommitmentArray`
  - Her slot için bir tane oluşturulur

- `BlockCommitmentCache` (satır 41-51): `#[derive(Default)]`
  - `block_commitment: HashMap<Slot, BlockCommitment>` — slot → taahhüt durumu
  - `commitment_slots: CommitmentSlots` — hesaplanan slot değerleri cache'i
  - `total_stake: u64` — epoch'un toplam stake'i

- `CommitmentSlots` (satır 209-219): `#[derive(Default, Clone, Copy)]`
  - `slot: Slot` — mevcut bank slot'u
  - `root: Slot` — node'un lokal root'u
  - `highest_confirmed_slot: Slot` — cluster'da 2/3+ onaylı en yüksek slot
  - `highest_super_majority_root: Slot` — cluster'da 2/3+ rooted en yüksek slot

**Kritik Fonksiyonlar:**

- `BlockCommitment::increase_confirmation_stake()` (satır 17-20): `commitment[confirmation_count - 1] += stake`
  - assert: 1 ≤ confirmation_count ≤ MAX_LOCKOUT_HISTORY
  - 1-indexed öğesi, diziye 0-indexed olarak eklenir

- `BlockCommitment::get_confirmation_stake()` (satır 22-25): Belirli confirmation sayısı için stake

- `BlockCommitment::increase_rooted_stake()` (satır 27-29): `commitment[MAX_LOCKOUT_HISTORY] += stake` — son indeks rooted stake için

- `BlockCommitment::get_rooted_stake()` (satır 31-33): Rooted stake miktarı

- `BlockCommitmentCache::slot_with_commitment()` (satır 114-125): CommitmentLevel enum'dan slot resolve et
  ```
  Processed/Recent → self.slot() (en güncel, kesinleşmemiş)
  Root             → self.root() (node root)
  Single           → highest_confirmed_slot
  SingleGossip/Confirmed → highest_gossip_confirmed_slot (= highest_confirmed_slot, TODO yorumu var)
  Max/Finalized    → highest_super_majority_root
  ```
  - `#[allow(deprecated)]` — CommitmentLevel'daki eski varyantlar

- `BlockCommitmentCache::highest_slot_with_confirmation_count()` (satır 127-137): `root..slot` aralığında geriye doğru tara, belirtilen confirmation sayısına sahip ilk slot'u bul

- `BlockCommitmentCache::calculate_highest_confirmed_slot()` (satır 139-141): Confirmation_count ≥ 1 olan en yüksek slot

- `BlockCommitmentCache::get_confirmation_count()` (satır 143-145): Bir slot için, toplam stake'in 2/3'ünü geçecek minimum confirmation sayısı

- `BlockCommitmentCache::get_lockout_count()` (satır 148-161): Belirli yüzde stake'i lock etmek için gereken minimum confirmation seviyesi
  - Dizi sondan başa dolaş, kümülatif stake topla
  - `sum / total_stake > minimum_stake_percentage` koşulu

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 231-345)
- `test_block_commitment`: Temel increase/get fonksiyonları
- `test_get_confirmations`: 3 slot, 3 farklı confirmation seviyesi doğrulama
- `test_highest_confirmed_slot`: Çeşitli senaryolarda en yüksek confirmed slot hesabı (gap'li slot'lar, hiç confirmed yok, vs.)

**Tasarım Kararları:**
1. **Sabit boyutlu dizi:** `[u64; 32]` — MAX_LOCKOUT_HISTORY + 1 (rooted için); dinamik HashMap'ten çok daha hızlı, 256 byte küçük yapı
2. **2/3 eşiği:** Byzantine fault tolerance — 1/3 Byzantine node'a karşı güvenlik
3. **CommitmentSlots ayrı:** Cache hesaplaması yapıldıktan sonra 4 değer bir arada tutulur — her sorguda yeniden hesaplama yapılmaz
4. **TODO (satır 109):** `highest_gossip_confirmed_slot` ayrı bir `OptimisticallyConfirmedBank` cache'ini kullanmalı ama henüz birleştirilmemiş

**Nova Shield İçin Öğrenilen:**
- Commitment seviyesi kavramı: processed/confirmed/finalized üç katman — Nova Shield consensus için aynı üç katman gerekli
- `[u64; N]` sabit dizi ile confirmation stake takibi: her slot için 32 element → küçük ve hızlı
- 2/3 eşiği: BFT için standart — `sum as f64 / total_stake as f64 > 2/3` koşulu

---

### [bank_utils.rs] (61 satır)

**Amaç:** Test/dev amaçlı yardımcı fonksiyonlar ve runtime'da vote transaction'larını bulmak/iletmek için.

**Fonksiyonlar:**

- `setup_bank_and_vote_pubkeys_for_tests()` (satır 16-37): `#[cfg(feature = "dev-context-only-utils")]`
  - `num_vote_accounts` adet rastgele validator keypair oluştur
  - `create_genesis_config_with_vote_accounts()` ile genesis config yap
  - `Bank::new_for_tests()` ile test bankı oluştur
  - Return: `(Bank, Vec<Pubkey>)` — bank ve vote pubkey listesi

- `find_and_send_votes()` (satır 39-61): Production fonksiyon — her zaman derlenir
  - `sanitized_txs` ve `tx_results`'u zip ile dolaş
  - Koşul: `tx.is_simple_vote_transaction() && result.was_executed_successfully()`
  - `vote_parser::parse_sanitized_vote_transaction()` ile vote'u parse et
  - `parsed_vote.1.last_voted_slot().is_some()` — boş vote iletme
  - `vote_sender.send(parsed_vote)` — `ReplayVoteSender` kanalına gönder

**Unsafe Bloklar:** Yok.
**Test Coverage:** Bağımsız test yok — bank_client.rs ve banking_stage testlerinden dolaylı kullanım.

**Nova Shield İçin Öğrenilen:**
- Vote transaction tespiti: `is_simple_vote_transaction()` hızlı bayrak kontrolü ile vote transaction'ları ayrıştırılır — vote trafik işleme için önemli
- `last_voted_slot().is_some()` kontrolü: Boş vote'ları filtreleme — ağ trafiğini azaltır

---

### [bank_client.rs] (394 satır)

**Amaç:** `Bank` için senkron ve asenkron client trait implementasyonları — test ve entegrasyon senaryoları için.

**Struct Listesi:**

- `BankClient` (satır 31-34):
  - `bank: Arc<Bank>` — paylaşımlı bank referansı
  - `transaction_sender: Mutex<Sender<VersionedTransaction>>` — async transaction kanalı

**Trait Implementasyonları:**

- `Client for BankClient` (satır 36-40): `tpu_addr()` → "Local BankClient" string döner

- `AsyncClient for BankClient` (satır 42-52): `async_send_versioned_transaction()`
  - Transaction'ı kanala gönder (`transaction_sender.send()`)
  - Return: ilk signature (ya da default)
  - Gerçek execution: arka plan thread'i yürütür

- `SyncClient for BankClient` (satır 54-298): 20+ metot
  - `send_and_confirm_message()`: `bank.process_transaction()` doğrudan çağrı — senkron
  - `send_and_confirm_instruction()`: Message oluştur → send_and_confirm_message
  - `transfer_and_confirm()`: `system_instruction::transfer()` → send_and_confirm_instruction
  - `get_account_data()`, `get_account()`, `get_balance()`: bank.get_account() / get_balance()
  - `get_recent_blockhash()`: `bank.last_blockhash()` + fee calculator
  - `get_signature_status()`: `bank.get_signature_status()`
  - `poll_for_signature_confirmation()`: assert `min_confirmed_blocks == 1` (multi-slot gözlem yok), 15s timeout ile sleep loop
  - `poll_for_signature()`: 15s timeout ile sleep loop
  - `get_epoch_info()`: `bank.get_epoch_info()`
  - `is_blockhash_valid()`: `bank.is_blockhash_valid()`
  - `get_fee_for_message()`: `SanitizedMessage::try_from_legacy_message()` → `bank.get_fee_for_message()`

- `BankClient::run()` (satır 301-309): Arka plan thread
  - `transaction_receiver.recv()` — blokla bekle
  - Birden fazla pending tx varsa hepsini topla: `try_recv()` döngüsü
  - `bank.try_process_entry_transactions(transactions)` — batch işle

- `BankClient::new_shared()` (satır 311-323): Ana constructor
  - unbounded channel oluştur
  - `Builder::new().name("solBankClient").spawn()` — arka plan thread başlat

- `BankClient::new()` (satır 325-327): Arc::new(bank) ile new_shared çağır

- `BankClient::set_sysvar_for_tests()` (satır 329-331): Test için sysvar override

- `BankClient::advance_slot()` (satır 333-356): `#[cfg(feature = "dev-context-only-utils")]`
  - `Bank::new_from_parent()` → BankForks'a insert → clock sysvar güncelle

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 359-394) — 1 test
- `test_bank_client_new_with_keypairs`: 2-of-2 multisig transfer, balance doğrulaması

**Tasarım Kararları:**
1. Async/Sync iki mod: AsyncClient batch işleme için arka plan thread, SyncClient doğrudan bank erişimi
2. `min_confirmed_blocks == 1` assert: BankClient tek blok görür — gerçek multi-blok simulasyonu yapılamaz
3. 15 saniye timeout: Testler için makul; production'da RPC client kullanılır

**Nova Shield İçin Öğrenilen:**
- Test client abstraction: `AsyncClient` + `SyncClient` trait ayrımı — Nova Shield test altyapısı için model
- unbounded channel + arka plan thread: Async transaction injection — integration test altyapısı

---

### [root_bank_cache.rs] (98 satır)

**Amaç:** Root Bank'ı önbellekle — BankForks'u yalnızca root değiştiğinde kilitleyen akıllı önbellek.

**Struct Listesi:**

- `RootBankCache` (satır 14-18):
  - `bank_forks: Arc<RwLock<BankForks>>`
  - `cached_root_bank: Weak<Bank>` — **Weak referans** — bank drop edildiğinde otomatik geçersiz
  - `root_slot: ReadOnlyAtomicSlot` — atomik slot değeri — lock almadan okunabilir

**Kritik Fonksiyonlar:**

- `RootBankCache::new()` (satır 21-31): Constructor
  - BankForks'u kilitle → root_bank'ı al → Weak referans oluştur
  - `lock.get_atomic_root()` → ReadOnlyAtomicSlot al

- `RootBankCache::root_bank()` (satır 33-44): Ana metot
  - `self.cached_root_bank.upgrade()` → Some(bank) ve bank.slot() == root_slot.get() ise: cache hit
  - Aksi halde: BankForks'u kilitle → yeni root_bank al → cached_root_bank ve slot güncelle
  - **Optimizasyon:** Atomik slot okuma (`ReadOnlyAtomicSlot`) lock almadan yapılır — çekirdek path'te lock contention yok

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 47-98)
- `test_root_bank_cache`: Slot 0 → bank oluştur → root_bank_cache yeni
  - slot 1 oluştur, insert et → root değişmedi → cache hala slot 0
  - set_root(1) → root_bank() çağrılana kadar cache güncellenmez (lazy)
  - root_bank() çağrıldıktan sonra → slot 1 bank'ı önbelleklendi

**Tasarım Kararları:**
1. `Weak<Bank>`: Root bank değiştiğinde eski bank Arc'ı drop edilirse, `upgrade()` None döner → otomatik cache invalidation
2. `ReadOnlyAtomicSlot`: Atomic okuma ile lock-free hızlı kontrol → bank forks kilidini meşgul etme
3. Lazy update: root_bank() çağrılana kadar cache güncellenmez — gereksiz lock alma yok

**Nova Shield İçin Öğrenilen:**
- Root bank cache pattern: Sık erişilen ama nadiren değişen değerleri önbelleğe almak için `Weak<T>` + atomic slot kontrolü kombinasyonu
- `Weak<T>` otomatik geçersizleştirme: Cache invalidation logic'i elle yazmak yerine drop-based invalidation

---

### [genesis_utils.rs] (285 satır)

**Amaç:** Test genesis config oluşturma yardımcıları — validator keypair, vote hesabı, stake hesabı seti oluşturur.

**Const/Fn:**
- `VALIDATOR_LAMPORTS: u64 = 42` (satır 22): Validator'a verilen varsayılan lamport
- `bootstrap_validator_stake_lamports()` (satır 25-27): `Rent::default().minimum_balance(StakeStateV2::size_of())` — minimum stake için rent hesabı
- `genesis_sysvar_and_builtin_program_lamports()` (satır 30-48): `const fn` — genesis'te kullanılan sabit lamport miktarı
  - 9 builtin program, 2 precompile
  - 6 sysvar: fees, stake_history, clock, rent, epoch_schedule, recent_blockhashes — sabit balance'lar

**Struct Listesi:**

- `ValidatorVoteKeypairs` (satır 50-54):
  - `node_keypair: Keypair`
  - `vote_keypair: Keypair`
  - `stake_keypair: Keypair`
  - `new()` ve `new_rand()` constructor'ları

- `GenesisConfigInfo` (satır 74-79):
  - `genesis_config: GenesisConfig`
  - `mint_keypair: Keypair`
  - `voting_keypair: Keypair`
  - `validator_pubkey: Pubkey`

**Kritik Fonksiyonlar:**

- `create_genesis_config()` (satır 81-91): Basit genesis — 0 validator stake, 1 random validator
  - Not (satır 82-85): 0 lamport stake hesabı accounts-db'de saklanmaz ama bank stakes cache'de tutulur → snapshot tutarsızlığına yol açabilir

- `create_genesis_config_with_vote_accounts()` (satır 93-104): Çoklu validator genesis
  - `ClusterType::Development` — tüm feature'lar aktif

- `create_genesis_config_with_vote_accounts_and_cluster_type()` (satır 106-167): Full kontrollu genesis
  - İlk validator `create_genesis_config_with_leader_ex()` ile oluşturulur
  - Sonraki validator'lar: accounts map'e eklenir (node_account + vote_account + stake_account)

- `create_genesis_config_with_leader()` (satır 169-197): Tek lider genesis

- `activate_all_features()` (satır 199-204): Development cluster — tüm inactive feature'ları aktif et

- `activate_feature()` (satır 206-216): Tekil feature aktivasyonu
  - `feature::create_account()` ile `activated_at: Some(0)` kayıt ekle

- `create_genesis_config_with_leader_ex()` (satır 218-285): En kapsamlı genesis fonksiyonu
  - Vote account, stake account, mint, validator, native mint token hesabı oluşturur
  - `solana_stake_program::add_genesis_accounts()` — stake program genesis hesapları
  - Development cluster: `activate_all_features()`

**Unsafe Bloklar:** Yok.

**Nova Shield İçin Öğrenilen:**
- Genesis config çok katmanlı: Minimum (mint + 1 validator) → Tam (N validator, her biri 3 hesap: node, vote, stake)
- Feature aktivasyon genesis'te account bazlı: `activated_at: Some(0)` ile tüm feature'lar genesis'te aktif edilebilir — nova shield geliştirme ortamı için aynı yaklaşım
- `genesis_sysvar_and_builtin_program_lamports()` const fn: Kapitalizasyon hesabı için sabit değer

---

### [non_circulating_supply.rs] (332 satır)

**Amaç:** Dolaşımdaki olmayan SOL miktarını hesapla — kilitli stake hesapları ve belirli pre-mine adresleri.

**Struct Listesi:**

- `NonCirculatingSupply` (satır 14-17):
  - `lamports: u64` — toplam kilitli lamport
  - `accounts: Vec<Pubkey>` — kilitli hesap listesi

**Kritik Fonksiyonlar:**

- `calculate_non_circulating_supply()` (satır 19-81):
  1. `non_circulating_accounts()` listesi (satır 84-198): 110 hardcoded mainnet-beta pubkey — sabit non-circulating set
  2. `withdraw_authority()` listesi (satır 200-215): 10 hardcoded mainnet-beta withdraw authority — bu key'lerin sahip olduğu stake hesapları kilitlidir
  3. `bank.rc.accounts.accounts_db.account_indexes.contains(ProgramId)` — ProgramId index varsa hızlı path: `get_filtered_indexed_accounts()`; yoksa yavaş path: `get_program_accounts()`
  4. Her stake hesabı için:
     - `StakeStateV2::Initialized(meta)`: `meta.lockup.is_in_force()` → kilitli ise ekle
     - `StakeStateV2::Stake(meta, _, _)`: aynı lockup kontrolü + `withdraw_authority_list.contains()` — withdraw authority kontrolü
  5. Toplam lamport hesapla: `non_circulating_accounts_set.iter().map(bank.get_balance).sum()`

**Macro:**
- `solana_sdk::pubkeys!()` (satır 84-215): Sabit pubkey dizileri oluşturan macro — derleme zamanında array

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 217-332)
- `test_calculate_non_circulating_supply`:
  - 10 genesis hesap, 110 non-circulating hesap, 3 locked stake hesap
  - Epoch değişimi → stake kilidi açılır → non-circulating azalır

**Tasarım Kararları:**
1. Hardcoded hesap listesi: Merkezi olmayan doğrulama imkansız — mainnet-beta için yapısal karar
2. AccountIndex optimizasyonu: ProgramId index varsa filtreli tarama, yoksa tam tarama — production'da index tercih edilir
3. Lockup kontrolü: `is_in_force(&clock, None)` — custodian olmadan lockup geçerliliği

**Nova Shield İçin Öğrenilen:**
- Non-circulating supply kavramı: Pre-mine, team, foundation vs. tokenlerin takibi için gerekli
- Lockup mekanizması: Stake hesabında `meta.lockup` ile zaman+epoch bazlı kilitleme — token vesting için model
- AccountIndex ile hızlı program hesabı taraması vs. tam tarama — nova shield'da da account indexing önemli

---

### [builtins.rs] (110 satır)

**Amaç:** Runtime'a dahil edilmiş yerleşik programların (builtin programs) listesi ve feature aktivasyonları.

**Struct Listesi:**

- `BuiltinPrototype` (satır 9-14):
  - `feature_id: Option<Pubkey>` — `None` ise her zaman aktif; `Some(id)` ise ilgili feature aktif edildiğinde aktif
  - `program_id: Pubkey` — programın onchain adresi
  - `name: &'static str` — log/debug için isim
  - `entrypoint: BuiltinFunctionWithContext` — function pointer

**Kritik Data:**

- `BUILTINS: &[BuiltinPrototype]` (satır 43-110): 11 builtin program
  - **Daima aktif (feature_id: None):** 9 program
    - `system_program` — hesap oluşturma, lamport transferi
    - `vote_program` — validator vote
    - `stake_program` — delegation, staking
    - `config_program` — on-chain config
    - `bpf_loader_deprecated` — eski BPF loader
    - `bpf_loader` — mevcut BPF loader
    - `bpf_loader_upgradeable` — yükseltilebilir BPF loader
    - `compute_budget_program` — compute unit limiti
    - `address_lookup_table_program` — ALT
  - **Feature-gated (feature_id: Some):** 2 program
    - `zk_token_proof_program` — `zk_token_sdk_enabled` feature
    - `loader_v4` — `enable_program_runtime_v2_and_loader_v4` feature

- `Debug` impl (satır 16-24): `BuiltinFunctionWithContext` Debug implement etmediği için custom impl

- `AbiExample` impl (satır 26-41): `#[cfg(RUSTC_WITH_SPECIALIZATION)]` — mock entrypoint ile örnek

**Unsafe Bloklar:** Yok.
**Test Coverage:** Yok. İçerik statik veriler — integration testlerde dolaylı doğrulama.

**Tasarım Kararları:**
1. `feature_id: Option<Pubkey>`: Feature flag ile program aktivasyonu — protocol upgrade mekanizması
2. Statik dizi `&'static [BuiltinPrototype]`: Runtime'da değişmez — özel privilege yok

**Nova Shield İçin Öğrenilen:**
- Builtin program kavramı: Nova Shield için stake, vote, token vs. yerleşik programlar. Feature flag ile yeni program aktivasyonu
- Entrypoint function pointer: `BuiltinFunctionWithContext` — nova shield'da yerleşik program arayüzü için benzer pattern

---

### [loader_utils.rs] (316 satır)

**Amaç:** Test/geliştirme için SBF program yükleme ve yükseltme yardımcıları.

`#![cfg(feature = "dev-context-only-utils")]` — tüm dosya bu feature flag altında, production build'de compile edilmez.

**Const:**
- `CHUNK_SIZE: usize = 512` (satır 26): Program yükleme chunk boyutu — transaction'a sığması gereken boyut

**Kritik Fonksiyonlar:**

- `load_program_from_file()` (satır 28-42): Binary'nin yanındaki `sbf/` dizininden `.so` dosyası oku

- `create_program()` (satır 44-57): Loader olmadan direkt program oluştur (unverified)
  - Yeni pubkey, ELF data yükle, executable=true set et, bank.store_account()

- `load_and_finalize_program()` (satır 59-99): Eski loader ile program yükle
  - Hesap oluştur (system_instruction::create_account)
  - 512-byte chunk'lar halinde `loader_instruction::write()`
  - `loader_instruction::finalize()` ile tamamla

- `load_program()` (satır 101-114): `load_and_finalize_program()` + finalize instruction gönder

- `load_upgradeable_buffer()` (satır 116-168): BPF upgradeable loader ile buffer hesabı oluştur ve doldur
  - `bpf_loader_upgradeable::create_buffer()` → 512-byte chunk'lar halinde `write()`

- `load_upgradeable_program()` (satır 170-213): Buffer'ı deploy et
  - `bpf_loader_upgradeable::deploy_with_max_program_len()` — program len * 2 max size
  - Clock sysvar'ı slot 1'e ayarla (program effective olması için)

- `load_upgradeable_program_and_advance_slot()` (satır 235-256): Program yükle + 2 slot ilerlet
  - `advance_slot()` iki kere çağrılır (clock=1 → effective sonraki epoch)

- `upgrade_program()` (satır 258-285): Mevcut programı yeni ELF ile yükselt

- `set_upgrade_authority()` (satır 287-305): Program upgrade authority'yi değiştir

- `create_invoke_instruction()` (satır 307-316): Test invoke instruction'ı oluştur

**Unsafe Bloklar:** Yok.
**Test Coverage:** Bu dosya kendisi test utility — dolaylı kullanım.

**Nova Shield İçin Öğrenilen:**
- Upgradeable program deployment süreci: buffer → deploy → upgrade — nova shield custom program deployment'ı için model
- `CHUNK_SIZE = 512`: Transaction boyut kısıtından kaynaklanan bölme — büyük program yüklemeleri birden fazla tx gerektirir

---

### [transaction_batch.rs] (202 satır)

**Amaç:** Account kilitleme sonuçlarını ve transaction setlerini tutan yaşam-süresi yönetimli batch yapısı.

**Struct Listesi:**

- `TransactionBatch<'a, 'b>` (satır 8-13):
  - `lock_results: Vec<Result<()>>` — her tx için kilit sonucu
  - `bank: &'a Bank` — yaşam süresi bank'a bağlı
  - `sanitized_txs: Cow<'b, [SanitizedTransaction]>` — zero-copy ya da owned slice
  - `needs_unlock: bool` — Drop'ta unlock yapılacak mı

**Kritik Fonksiyonlar:**

- `TransactionBatch::new()` (satır 16-28): Constructor
  - `assert_eq!(lock_results.len(), sanitized_txs.len())` — tutarlılık garantisi

- `lock_results()`, `sanitized_transactions()`, `bank()`, `needs_unlock()`: Accessor'lar

- `set_needs_unlock()` (satır 43-45): Kontrolü dışarıya ver — simulate batch için needs_unlock=false

- `unlock_failures()` (satır 52-81): Kısmi hata sonrası unlock
  - `result.is_ok() && self.lock_results[*index].is_err()` — hatadan ok'a dönüş yasak (assert)
  - Sadece ok→err geçişler: `bank.unlock_accounts()` çağır
  - `self.lock_results` = yeni sonuçlar

- `Drop for TransactionBatch` (satır 84-96): RAII kilitleme
  - `needs_unlock()` ise: `bank.unlock_accounts()` — her kilitlenen hesabı unlock et

**Unsafe Bloklar:** Yok.

**Test Coverage:** (satır 98-202)
- `test_transaction_batch`: Lock, çakışma tespiti, drop ile unlock, yeniden lock
- `test_simulation_batch`: Simulate batch (needs_unlock=false) — normal batch'lerle birlikte lock alabilir
- `test_unlock_failures`: QoS hata → kısmi unlock → drop ile kalan kilitleri çöz

**Tasarım Kararları:**
1. **RAII:** Drop implementasyonu — unlock'u unutmak imkansız
2. **Cow<[SanitizedTransaction]>:** Borrowed veya owned tx slice — sıfır kopya olasılığı
3. **ok→err dönüşüm yasak:** Lock tutarlılığını garanti eder; lock alınmamış hesabın unlock edilmesi undefined behavior

**Nova Shield İçin Öğrenilen:**
- RAII lock yönetimi: Transaction batch her zaman lock sonuçlarına sahip — drop'ta otomatik unlock — nova shield transaction processing için kritik
- `Cow<[T]>` zero-copy batch: Kopyalama maliyetini azalt
- kısmi başarı unlock: QoS veya maliyet limiti aşımında bazı tx'ler reddedilirken, kilit alındıktan sonra kilitli kalan hesaplar doğru unlock edilmeli

---

### [compute_budget_details.rs] (183 satır)

**Amaç:** Transaction'daki ComputeBudget instruction'larından compute unit fiyatı ve limitini çıkart.

**Struct/Trait Listesi:**

- `ComputeBudgetDetails` (satır 11-14): `#[derive(Clone, Debug, PartialEq, Eq)]`
  - `compute_unit_price: u64` — micro-lamport/compute-unit fee
  - `compute_unit_limit: u64` — maksimum compute unit sayısı

- `GetComputeBudgetDetails` trait (satır 16-32):
  - `get_compute_budget_details(&self, round_compute_unit_price_enabled: bool) -> Option<ComputeBudgetDetails>`
  - Default implementasyon: `process_compute_budget_instruction()` — `process_compute_budget_instructions()` çağırır

**Implementasyonlar:**

- `impl GetComputeBudgetDetails for SanitizedVersionedTransaction` (satır 34-43): `get_message().program_instructions_iter()`
- `impl GetComputeBudgetDetails for SanitizedTransaction` (satır 46-56): `self.message().program_instructions_iter()`

**Test Coverage:** (satır 58-183) — 3 test
- `test_get_compute_budget_details_with_valid_request_heap_frame_tx`: heap frame request → DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT
- `test_get_compute_budget_details_with_valid_set_compute_units_limit`: set_compute_unit_limit(101)
- `test_get_compute_budget_details_with_valid_set_compute_unit_price`: set_compute_unit_price(1000)
- Her test için hem `SanitizedVersionedTransaction` hem `SanitizedTransaction` doğrulanır

**Nova Shield İçin Öğrenilen:**
- Compute budget trait: Her transaction tipinde aynı arayüz — prioritization fee ve compute limit çıkarma için
- `round_compute_unit_price_enabled` flag: Fee hesabında yuvarlama özelliği — onchain hesaplamada önemi var

---

### [epoch_rewards_hasher.rs] (99 satır)

**Amaç:** Epoch ödüllerini partition'lara böl — her stake hesabını bir partition bucket'a deterministik olarak ata.

**Kritik Fonksiyonlar:**

- `hash_rewards_into_partitions()` (satır 6-24):
  - `EpochRewardsHasher::new(num_partitions, parent_blockhash)` ile hasher oluştur
  - Her reward için: `hasher.clone().hash_address_to_partition(&reward.stake_pubkey)` → partition index
  - `result[partition_index].push(reward)` — bucket'a ekle
  - **Not:** `hasher.clone()` kasıtlı — state yeniden kullanımı için (yorum: "prevents us from re-hashing the seed each time")

**Test Coverage:** (satır 26-99)
- `test_hash_rewards_into_partitions`: 12345 random reward → 5 partition → toplam ve sayı korunur
- `test_hash_rewards_into_partitions_empty`: Boş input → num_partitions kadar boş bucket
- `compare()` helper: HashMap ile order-independent comparison

**Tasarım Kararları:**
1. `EpochRewardsHasher.clone()` yeniden kullanım: Her adres için hasher'ı sıfırdan oluşturmak yerine, seed'i hasher durumuna pre-hash et; her klonda aynı pre-hashed state'den devam et
2. `parent_blockhash` seed: Epoch ödülleri, önceki epoch'un son blockhash'ine göre deterministik — herkes aynı partition'ı hesaplayabilir

**Nova Shield İçin Öğrenilen:**
- Epoch ödüllerini partition'lama: Büyük validator setlerinde ödülleri birden fazla slot'a bölerek dağıtmak için deterministik hashing
- Hasher clone optimizasyonu: Pre-compute seed state, her kullanımda full recompute yerine

---

### [static_ids.rs] (16 satır)

**Amaç:** Token ve mint programlarının statik ID'lerini tek bir `Vec<Pubkey>` olarak topla.

**Data:**

- `STATIC_IDS: Vec<Pubkey>` (satır 8-15): `lazy_static!` — ilk erişimde initialize edilir
  - `inline_spl_associated_token_account::id()` — ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
  - `inline_spl_associated_token_account::program_v1_1_0::id()` — NatA1Zyo48dJ7yuwR7cGURwhskKA8ywUyxb9GvG7mTC
  - `inline_spl_token::id()` — Token program
  - `inline_spl_token::native_mint::id()` — Wrapped SOL mint
  - `inline_spl_token_2022::id()` — Token-2022 program

**Kullanım:** Bank'da token hesaplarının non-circulating hesaplamalarında, hesap indekslemesinde bu ID'lere hızlı erişim için.

**Nova Shield İçin Öğrenilen:**
- Token program ID'leri tek merkezde: Dağılmış ID kontrolü yerine tek vektör → contains() ile hızlı kontrol
- `lazy_static!`: Program başlangıcında hesaplanır, sonraki erişimler O(1)

---

### [inline_spl_associated_token_account.rs] (6 satır)

**Amaç:** SPL Associated Token Account programının ID'sini inline olarak tanımla — harici bağımlılıktan kaçın.

```rust
solana_sdk::declare_id!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub(crate) mod program_v1_1_0 {
    solana_sdk::declare_id!("NatA1Zyo48dJ7yuwR7cGURwhskKA8ywUyxb9GvG7mTC");
}
```

**Tasarım Kararı:** `spl-associated-token-account` crate'ini bağımlılık olarak almak yerine sadece program ID'sini inline tanımla. Compile time ve binary boyutunu azalt.

**Nova Shield İçin Öğrenilen:**
- Harici token program bağımlılıklarını minimize et: Sadece ID gerekiyorsa inline declare — full crate import gerekmez

---

### [lib.rs] (47 satır)

**Amaç:** `solana-runtime` crate'in kök modülü — tüm alt modülleri ihraç eder.

**Feature Flags:**
- `#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]` — ABI örnek üretimi için Rust specialization nightly feature

**Attribute:**
- `#![allow(clippy::arithmetic_side_effects)]` — integer taşması kontrolü Clippy uyarısı susturulmuş; Solana codebase boyunca saturating_* tercih edilir ama her yerde zorunlu değil

**Macro İmportları:**
- `#[macro_use] extern crate lazy_static` — `lazy_static!` makrosu için
- `#[macro_use] extern crate solana_metrics` — `datapoint_warn!` vs. metrikleri için
- `#[macro_use] extern crate serde_derive` — `#[derive(Serialize, Deserialize)]` için
- `#[macro_use] extern crate solana_frozen_abi_macro` — `#[derive(AbiExample)]` için

**Modül Listesi (pub):** 32 modül ihraç
- Public: accounts_background_service, bank, bank_client, bank_forks, bank_utils, builtins, commitment, compute_budget_details, epoch_stakes, genesis_utils, inline_spl_associated_token_account, installed_scheduler_pool, loader_utils, non_circulating_supply, prioritization_fee, prioritization_fee_cache, root_bank_cache, serde_snapshot, snapshot_*, stake_history, stake_weighted_timestamp, stakes, static_ids, status_cache, transaction_batch
- Private (crate-internal): `mod epoch_rewards_hasher`, `mod stake_account`

**Nova Shield İçin Öğrenilen:**
- Crate lib.rs minimal: Sadece modul declaration ve macro import — iş mantığı alt modüllerde
- Gizli modüller (`mod` olmadan `pub`): Implementation detail'ler — `stake_account` ve `epoch_rewards_hasher` crate dışına açılmaz

---

## Genel Değerlendirme ve Nova Shield İçin Ana Dersler

### Stakes Sistemi Mimarisi
Solana'nın stake sistemi 3 katmanlı:
1. **Hesap katmanı:** `StakeAccount<T>` — tip-güvenli wrapper
2. **Cache katmanı:** `Stakes<StakeAccount>` — im::HashMap ile persistent, COW semantikli
3. **Epoch katmanı:** `StakesCache` → `activate_epoch()` → paralel StakeActivationStatus toplama

Nova Shield için öneri:
- Stake delegation cache'i im::HashMap veya Arc<DashMap> ile yönet
- Epoch geçişinde Rayon ile paralel stake aktivasyon hesabı
- `StakesEnum` gibi runtime temsili vs. serializable temsil ayrımı

### Status Cache (Transaction Deduplication)
- Blockhash bazlı + fork-aware: ancestors ve roots set ile
- Rastgele key offset: DoS koruması
- SlotDelta ile snapshot uyumluluğu

Nova Shield için: Aynı iki katman (cache + delta log) gerekli.

### Commitment Levels
Üç üretim seviyesi: processed → confirmed (2/3+ lockout) → finalized (2/3+ rooted)
- Nova Shield'da aynı seviyeleri `BlockCommitmentArray` gibi sabit boyutlu dizi ile yönet

### Stake-Weighted Timestamp
Algoritma: BTreeMap üzerinde kümülatif stake medyanı + POH bounding

Nova Shield: Fiyat oracle, zaman damgası veya diğer validator girdileri için aynı stake-weighted medyan algoritması

### RAII Transaction Batch
`TransactionBatch` Drop ile unlock — güvenli account kilitleme yönetimi

Nova Shield: Transaction işleme sırasında account locking/unlocking RAII ile yönetilmeli

### Non-Circulating Supply
Lockup + hardcoded liste + indexed scan

Nova Shield: Token ekonomisi için circulating vs. non-circulating takibi, lockup mekanizması stake hesabı meta field'ında

### Builtin Programs (Feature-gated)
`feature_id: Option<Pubkey>` ile program aktivasyonu

Nova Shield: Yerleşik program aktivasyonları feature flag ile — protocol upgrade için güvenli mekanizma
