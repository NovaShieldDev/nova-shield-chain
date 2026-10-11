# Solana Analiz — Agent 21
## Kapsam: runtime/src/bank/ alt modülleri (9 dosya)

**Toplam satır:** 16.294 (tests.rs: 13.794 + diğer 8 dosya: 2.500)
**Analiz tarihi:** 2026-07-22

---

## Dosya Listesi ve Satır Sayıları

| Dosya | Satır |
|---|---|
| address_lookup_table.rs | 51 |
| bank_hash_details.rs | 348 |
| builtin_programs.rs | 64 |
| epoch_accounts_hash_utils.rs | 202 |
| fee_distribution.rs | 821 |
| metrics.rs | 197 |
| serde_snapshot.rs | 638 |
| sysvar_cache.rs | 179 |
| tests.rs | 13.794 |
| **TOPLAM** | **16.294** |

---

## Genel Mimari Bağlam

Bu 9 dosya, `Bank` struct'ının dev `bank.rs` dosyasından ayrılan spesifik alt modülleridir. Rust'ta büyük dosyaların parçalanarak yönetilmesi (`mod` + ayrı dosya) pattern'ini kullanır. Her dosya `super::Bank`'a erişir; yani `Bank` struct'ının `impl` bloklarını bu dosyalarda genişletmek standart yaklaşımdır. `tests.rs` ise tüm `Bank` işlevselliğinin entegrasyon testleridir.

---

## Dosya 1: address_lookup_table.rs (51 satır)

**Amaç:** `Bank` struct'ı için `AddressLoader` trait'ini implement ederek V0 mesajlarının Address Lookup Table (ALT) çözümlemesini gerçekleştirir.

### Struct/Enum/Trait Listesi

- Yeni struct/enum tanımı yoktur. `impl AddressLoader for &Bank` bloğu içerir.

### Kritik Fonksiyonlar

- **`fn into_address_loader_error(err: AddressLookupError) -> AddressLoaderError`** (satır 13-22):
  - Dahili hata türü (`AddressLookupError`) → public SDK hata türüne (`AddressLoaderError`) dönüştürür.
  - Exhaustive `match` ile 4 varyant: `LookupTableAccountNotFound`, `InvalidAccountOwner`, `InvalidAccountData`, `InvalidLookupIndex`.
  - `pub` değil; sadece bu modülde kullanılan yardımcı fonksiyon.

- **`fn load_addresses(self, address_table_lookups: &[MessageAddressTableLookup]) -> Result<LoadedAddresses, AddressLoaderError>`** (satır 25-50):
  - `AddressLoader` trait'inin tek metodunu implement eder.
  - **Algoritma:**
    1. `self.transaction_processor.sysvar_cache.read().unwrap()` ile sysvar cache'i kilitle.
    2. `get_slot_hashes()` çağır — bu, ALT'nin hangi slotta aktive edildiğini doğrulamak için gereklidir.
    3. `address_table_lookups.iter().map(...)` ile her lookup için `accounts.load_lookup_table_addresses()` çağır.
    4. `collect::<Result<_, _>>()` — ilk hata gelirse hemen `Err` döndürür (short-circuit).
  - `&Bank` (shared reference) üzerinde implement edilmiştir; `self` consume edilmez.
  - Slot hash yoksa `AddressLoaderError::SlotHashesSysvarNotFound` döner.

### Unsafe Bloklar

Hiç `unsafe` blok yoktur.

### Test Coverage

Bu dosyada bağımsız `#[cfg(test)]` bloğu yoktur. Test coverage `tests.rs` üzerinden sağlanır.

### Tasarım Kararları

- `AddressLoader` trait'i `solana_sdk`'da tanımlanmıştır; `Bank` üzerine bu trait implement edilmesi, transaction sanitization pipeline'ının `Bank`'a bağlı olmadan çalışmasını sağlar (dependency inversion).
- `&Bank` referansı üzerinde implement: bank freeze edilmiş bile olsa lookup yapılabilir.
- Slot hashes cache'den okunması: her lookup için AccountsDB'ye gitmeyi önler, performans kritik yoldur.

### Nova Shield İçin Öğrenilen

- **Trait-based abstraction:** Address resolution mantığı trait arkasına saklanırsa transaction processor, bank implementasyonundan bağımsız çalışır. Nova Shield'de de `AddressLoader` benzeri bir soyutlama katmanı oluşturulabilir.
- **SlotHashes doğrulaması:** ALT lookup, slot_hashes sysvar üzerinden yapılır. Bu, stale (eski) lookup tablolarının kullanılmasını önler. Nova Shield'in address indirection mekanizmasında epoch/slot güvenlik kontrolü şarttır.
- **Error boundary:** İç hata tipi → dış hata tipine mapping yaklaşımı: `DeprecatedError` → `PublicError` pattern'i modül sınırlarında temiz API sağlar.

---

## Dosya 2: bank_hash_details.rs (348 satır)

**Amaç:** Bank hash hesaplamasına giren tüm bileşenleri JSON formatında dosyaya yazmak/okumak — debug ve hash uyuşmazlığı tespiti için.

### Struct/Enum/Trait Listesi

- **`BankHashDetails`** (satır 25-32):
  - `version: String` — client versiyonu (`solana_version::version!()`)
  - `account_data_encoding: String` — her zaman `"base64"`
  - `bank_hash_details: Vec<BankHashSlotDetails>` — her slot için detay
  - `#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]`

- **`BankHashSlotDetails`** (satır 70-88):
  - `slot: Slot` — hangi slot
  - `bank_hash: String` — bank hash (hex string)
  - `parent_bank_hash: String` — parent bank hash
  - `accounts_delta_hash: String` — sadece bu slottaki değişen hesapların hash'i
  - `signature_count: u64` — bu slottaki toplam imza sayısı
  - `last_blockhash: String` — son blockhash
  - `accounts: BankHashAccounts` — değişen tüm hesaplar
  - `#[serde(skip_serializing_if = "...")]` attribute'leri ile boş alanlar atlanır.
  - `#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Default)]`

- **`BankHashAccounts`** (satır 163-165):
  - Newtype wrapper: `accounts: Vec<PubkeyHashAccount>`
  - `Default` derive ile boş vec.
  - Özel Serialize/Deserialize implementasyonu — `SerdeAccount` intermediary struct üzerinden.

- **`SerdeAccount`** (satır 170-178) — private:
  - `pubkey: String`, `hash: String`, `owner: String`, `lamports: u64`, `rent_epoch: Epoch`, `executable: bool`, `data: String` (base64)
  - Serde-friendly human-readable format için intermediate struct.

### Kritik Fonksiyonlar

- **`BankHashDetails::new(bank_hash_details: Vec<BankHashSlotDetails>) -> Self`** (satır 35-41):
  - Basit constructor. `version` otomatik doldurulur.

- **`BankHashDetails::filename(&self) -> Result<String, String>`** (satır 44-65):
  - 1 slot varsa: `"{slot}-{hash}.json"`
  - N slot varsa: `"{first_slot}-{first_hash}_{last_slot}-{last_hash}.json"`
  - Boş `Vec` ise `Err` döner.

- **`BankHashSlotDetails::new(...) -> Self`** (satır 99-118):
  - Hash değerlerini `.to_string()` ile hex string'e çevirir.

- **`impl TryFrom<&Bank> for BankHashSlotDetails`** (satır 120-158):
  - Sadece **frozen** bank üzerinde çalışır — frozen değilse `Err`.
  - `accounts_delta_hash` alır (`.unwrap()` güvenli çünkü frozen bank'ta herzaman `Some`).
  - `get_pubkey_hash_account_for_slot()` — tüm değişen hesapları döner.
  - `.sort_by_key(|account| account.pubkey)` — accounts_delta_hash hesaplamasındaki sırayla aynı sıralama.

- **`impl From<&PubkeyHashAccount> for SerdeAccount`** (satır 180-197):
  - `account.data()` → `BASE64_STANDARD.encode(...)` dönüşümü.

- **`impl TryFrom<SerdeAccount> for PubkeyHashAccount`** (satır 199-222):
  - Tüm string parsing işlemleri `map_err(|err| err.to_string())` ile hataya dönüştürülür.
  - Base64 decode başarısız olabilir → `Result`.

- **`impl Serialize for BankHashAccounts`** (satır 224-236):
  - Manuel serialize: `serialize_seq` ile `SerdeAccount` ara tipi üzerinden dizi serialize eder.

- **`impl<'de> Deserialize<'de> for BankHashAccounts`** (satır 238-253):
  - `Vec<SerdeAccount>` deserialize et → `PubkeyHashAccount::try_from` ile dönüştür.
  - Hata: `de::Error::custom` ile serde hatasına wrap.

- **`pub fn write_bank_hash_details_file(bank: &Bank) -> std::result::Result<(), String>`** (satır 256-287):
  - Dosya path: `<base_working_path>/bank_hash_details/{slot}-{hash}.json`
  - Eğer aynı isimde dosya zaten varsa yazmaz (idempotent).
  - `BufWriter` kullanır — her sayı/virgül için ayrı syscall yerine buffered yazma.
  - `serde_json::to_writer_pretty` — okunabilir JSON format.

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Test Coverage

- **`test_serde_bank_hash_details`** (satır 338-347):
  - 10 slotluk `BankHashDetails` oluşturur → JSON serialize → deserialize → eşitlik kontrolü.
  - `build_details()` helper ile sabit test verisi oluşturur.
  - Tek test, ancak round-trip'i tam kapsıyor.

### Tasarım Kararları

- `SerdeAccount` intermediate struct pattern'i: `PubkeyHashAccount` doğrudan Serialize/Deserialize implement etmez (başka amaçlarla kullanılıyor); bu yüzden bank hash dosyası için ayrı format struct.
- Dosyanın adı hash içerir: aynı hash için duplicate dosya oluşmaz (content-addressed storage).
- `BufWriter` kullanımı: JSON yazma I/O optimizasyonu — büyük bank state'inde kritik.
- Sorting: pubkey bazlı sıralama, `accounts_delta_hash` ile tutarlılık sağlar.

### Nova Shield İçin Öğrenilen

- **Debug serialization pattern:** Bank hash'ini oluşturan tüm bileşenleri (parent_hash, delta_hash, signature_count, accounts) debug amaçlı dosyaya yazma yaklaşımı production blockchain için zorunludur. Nova Shield'de de hash uyuşmazlığı debug mekanizması baştan tasarlanmalı.
- **Content-addressed file naming:** `{slot}-{hash}.json` pattern ile dosya adından içerik anlaşılır ve duplicate yazma önlenir.
- **Idempotent write:** `if !path.exists()` kontrolü — distributed ortamda validator restart'larında güvenli.
- **Frozen bank zorunluluğu:** Hash hesapları sadece freeze sonrası alınabilir; bu constraint açıkça enforce edilmeli.

---

## Dosya 3: builtin_programs.rs (64 satır)

**Amaç:** Builtin (native) program ve precompile'ların feature activation'larında epoch geçişlerinde doğru şekilde uygulandığını test etmek.

**Not:** Bu dosya yalnızca `#[cfg(test)] mod tests { ... }` içerir. Production kodu yoktur.

### Struct/Enum/Trait Listesi

Hiç struct/enum/trait tanımı yoktur. Tüm dosya test kodu.

### Kritik Fonksiyonlar (Test Fonksiyonları)

- **`test_apply_builtin_program_feature_transitions_for_new_epoch`** (satır 11-43):
  - `FeatureSet::all_enabled()` ile tüm feature'lar aktif bank oluşturur.
  - `finish_init()` çağrısı: genesis sonrası başlatma adımı.
  - Precompile hesapları `AccountSharedData::default()` ile sıfırlar (cluster simülasyonu).
  - `ed25519_program` için özel: `system_program::id()` owner ile eklenir (tarihsel uyumluluk).
  - Bank `freeze()` edilir sonra `apply_builtin_program_feature_transitions(true, &HashSet::new())` çağrılır.
  - Freeze sonrası bu çağrının hesap değişikliği yapmadığını doğrular (implicit: no panic).

- **`test_startup_from_snapshot_after_precompile_transition`** (satır 46-63):
  - Snapshot'tan startup sırasını simüle eder.
  - Bank freeze edilir sonra tekrar `finish_init()` çağrılır.
  - Bu, snapshot restore sonrası `finish_init`'in idempotent olduğunu doğrular.

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Tasarım Kararları

- Bu dosyanın sadece test içermesi: `builtin_programs.rs` adında bir prod dosya beklenirken sadece test bulunması dikkat çekicidir. Asıl builtin program yönetimi muhtemelen ana `bank.rs` içinde kalmaktadır; sadece bu testler modüler tutulmuş.
- `Arc::new(FeatureSet::all_enabled())` ile bank'ın feature_set'ini doğrudan replace etme — test-only pattern.

### Nova Shield İçin Öğrenilen

- **Precompile/builtin idempotency:** Native program geçişleri (yeni bir epoch'ta feature activation) idempotent olmalı — aynı geçişin iki kez uygulanması state bozmamalı.
- **Snapshot restart testi:** `finish_init()` çağrısının frozen bank üzerinde de güvenli çalışması gerekir. Nova Shield'in initialization chain'i de snapshot-safe olmalı.

---

## Dosya 4: epoch_accounts_hash_utils.rs (202 satır)

**Amaç:** Epoch Accounts Hash (EAH) hesaplamasının ne zaman başlayıp ne zaman biteceğini hesaplayan utility fonksiyonlar.

### Struct/Enum/Trait Listesi

- **`CalculationInfo`** (satır 117-143):
  - `epoch: Epoch` — hangi epoch için
  - `slots_per_epoch: u64` — epoch uzunluğu
  - `first_slot_in_epoch: Slot`
  - `last_slot_in_epoch: Slot`
  - `calculation_offset_start: Slot` — epoch başından offset (= slots_per_epoch / 4)
  - `calculation_offset_stop: Slot` — epoch başından offset (= slots_per_epoch / 4 * 3)
  - `calculation_start: Slot` — mutlak slot numarası
  - `calculation_stop: Slot` — mutlak slot numarası
  - `calculation_interval: u64` — start ile stop arasındaki slot sayısı
  - `#[derive(Debug, Default, Copy, Clone)]`

### Kritik Fonksiyonlar

- **`pub fn is_enabled_this_epoch(bank: &Bank) -> bool`** (satır 13-33) — `#[must_use]`:
  - EAH bu epoch'ta aktif mi kontrolü.
  - **Algoritma:**
    - `MINIMUM_CALCULATION_INTERVAL = MAX_LOCKOUT_HISTORY + CALCULATION_INTERVAL_BUFFER`
    - `MAX_LOCKOUT_HISTORY` = 31 (vote state'den)
    - `CALCULATION_INTERVAL_BUFFER` = 150 (mainnet'te gözlemlenen max parent bank sayısı)
    - `MINIMUM_CALCULATION_INTERVAL` ≈ 181 slot
    - Hesaplanan interval bu değerden küçükse EAH disabled.
  - Küçük epoch'larda (test ortamı, 32-361 slot) EAH otomatik devre dışı.

- **`pub fn calculation_offset_start(bank: &Bank) -> Slot`** (satır 40-42) — `#[must_use]` + `#[inline]`:
  - Delegating: `calculation_info(bank).calculation_offset_start` döner.

- **`pub fn calculation_offset_stop(bank: &Bank) -> Slot`** (satır 50-52) — `#[must_use]` + `#[inline]`:
  - Delegating: `calculation_info(bank).calculation_offset_stop` döner.

- **`pub fn calculation_start(bank: &Bank) -> Slot`** (satır 57-59) — `#[must_use]` + `#[inline]`
- **`pub fn calculation_stop(bank: &Bank) -> Slot`** (satır 64-66) — `#[must_use]` + `#[inline]`
- **`pub fn calculation_interval(bank: &Bank) -> u64`** (satır 71-73) — `#[must_use]` + `#[inline]`

- **`pub fn is_in_calculation_window(bank: &Bank) -> bool`** (satır 77-81) — `#[must_use]`:
  - `range = calculation_start..calculation_stop` (exclusive stop)
  - `range.contains(&bank.slot())`
  - Bu fonksiyon: bank'ın şu anki slot'u EAH hesaplama penceresinde mi?

- **`pub fn calculation_info(bank: &Bank) -> CalculationInfo`** (satır 84-109):
  - **Matematiksel formüller:**
    - `calculation_offset_start = slots_per_epoch / 4`
    - `calculation_offset_stop = slots_per_epoch / 4 * 3`
    - `calculation_start = first_slot_in_epoch + calculation_offset_start`
    - `calculation_stop = first_slot_in_epoch + calculation_offset_stop`
    - `calculation_interval = calculation_offset_stop - calculation_offset_start`
  - EAH epoch'un %25'inde başlar, %75'inde biter → hesaplama aralığı epoch uzunluğunun yarısı.
  - `saturating_add` / `saturating_sub` ile overflow koruması.

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Test Coverage

4 test:

- **`test_is_enabled_this_epoch`** (satır 158-165) — `#[test_case]` macro ile parametrik:
  - 32 → false, 361 → false, 362 → false, 8192 → true, 432000 → true
  - 362 slot bile minimum eşiği geçmiyor (MINIMUM_CALCULATION_INTERVAL ≈ 181'dir ama interval slots/2 ≈ 181 tam sınırda).

- **`test_calculation_offset_bounds`** (satır 167-173):
  - `offset_start < offset_stop` olduğunu doğrular.

- **`test_calculation_bounds`** (satır 175-181):
  - `calculation_start < calculation_stop` olduğunu doğrular.

- **`test_calculation_info`** (satır 183-201):
  - 7 farklı `slots_per_epoch` × 2 `warmup` config = 14 kombinasyon test edilir.
  - Her kombinasyon için tüm CalculationInfo invariant'ları assert edilir.

### Tasarım Kararları

- `#[must_use]`: dönüş değeri görmezden gelinirse compile warning. EAH durumunu kontrol etmek kritik.
- `#[inline]`: Basit delegating fonksiyonlar inline olur — CPU branch overhead ortadan kalkar.
- 150-slot buffer: Mainnet gözlemleri (2022) bazında konservative seçim. Yorum satırı içinde tarih, gözlem kaynağı (brooks, mainnet-beta, 6 saat) belirtilmiş.
- EAH window: epoch'un ortasında (%25-%75) — bank root gecikmesi ve tüm node'ların tamamlaması için yeterli zaman.

### Nova Shield İçin Öğrenilen

- **Epoch-level global hash:** Tüm hesapların her epoch'ta bir kez hash'lenmesi (EAH) light client ve snapshot verification için kritik. Nova Shield'de de benzer "epoch state commitment" mekanizması tasarlanabilir.
- **Parametrik buffer:** Network gözlemlerine dayanarak buffer değerleri belirlenmeli — hardcode değil, gözlem-tabanlı.
- **Calculation window:** EAH başlangıcı bank'ın root edilmesine (max lockout = 31 slot) dayalı gecikmeye göre hesaplanır. Nova Shield'in finality mekanizması da bu tür gecikmeleri hesaba katmalı.
- **Test_case macro:** Parametrik testler aynı test mantığını birden fazla input değeri için çalıştırır — Rust'ta `test_case` crate kullanımı tavsiye edilir.

---

## Dosya 5: fee_distribution.rs (821 satır)

**Amaç:** Transaction fee ve rent fee'lerin validatörlere dağıtımını implement eder — Solana'nın ekonomi modelinin çekirdeği.

### Struct/Enum/Trait Listesi

- **`DepositFeeOptions`** (satır 18-21) — `#[derive(Debug)]`, private:
  - `check_account_owner: bool` — hesabın system program'a ait olup olmadığını kontrol et
  - `check_rent_paying: bool` — fee sonrası hesabın rent-paying durumuna düşüp düşmediğini kontrol et

- **`DepositFeeError`** (satır 23-31) — `#[derive(Error, Debug, PartialEq)]`, private:
  - `InvalidRentPayingAccount` — fee eklenince hesap rent-paying olur
  - `LamportOverflow` — u64 overflow
  - `InvalidAccountOwner` — sistem programı değil farklı owner

### Kritik Fonksiyonlar

- **`pub(super) fn distribute_transaction_fees(&self)`** (satır 48-90):
  - `self.collector_fees.load(Relaxed)` — atomik okuma; bu slotta biriken tx fee'ler.
  - `self.fee_rate_governor.burn(collector_fees)` — `(deposit, burn)` tuple döner.
    - Varsayılan burn_percent = 50 → yarısı validatöre, yarısı yakılır.
  - `validate_fee_collector` feature aktifse: owner ve rent kontrolü yapılır.
  - Başarısızlık durumunda tüm miktar yakılır + `datapoint_warn!("bank-burned_fee", ...)` metrik.
  - `self.capitalization.fetch_sub(burn, Relaxed)` — yakılan lamport supply'dan düşülür.
  - Başarı durumunda `rewards` vektörüne `RewardType::Fee` reward eklenir.

- **`fn deposit_fees(&self, pubkey: &Pubkey, fees: u64, options: DepositFeeOptions) -> Result<u64, DepositFeeError>`** (satır 93-122):
  - `get_account_with_fixed_root(pubkey).unwrap_or_default()` — hesap yoksa default (sıfır lamport).
  - `owner` kontrolü: system program değilse `InvalidAccountOwner`.
  - `RentState::from_account` → pre-state hesapla.
  - `account.checked_add_lamports(fees)` → overflow kontrolü.
  - Post-state: eğer `RentPaying → daha az RentPaying` değilse (yani sıfır bakiye → rent-paying oluyor) → `InvalidRentPayingAccount`.
  - `self.store_account(pubkey, &account)` → AccountsDB'ye yaz.
  - `Ok(account.lamports())` — yeni bakiye döner.

- **`fn distribute_rent_to_validators(&self, vote_accounts: &VoteAccountsHashMap, rent_to_be_distributed: u64)`** (satır 145-258):
  - **Algoritma:**
    1. Tüm stake'li validatör-stake çiftlerini filtrele (staked == 0 olanlar hariç).
    2. `total_staked` hesapla.
    3. Her validatör için: `rent_share = (staked * rent_to_be_distributed) / total_staked` (u128 aritmetiği, overflow önlemi).
    4. `rent_distributed_in_initial_round` topla.
    5. `leftover_lamports = rent_to_be_distributed - rent_distributed_in_initial_round` — kesir yuvarlama artığı.
    6. Sıralama: önce `staked` azalan, aynı stake'de `pubkey` sırasıyla (determinizm için).
    7. Leftover lamport'lar en yüksek stake'li validatörlerden başlayarak 1'er 1'er dağıtılır.
    8. Her validatör için `deposit_fees()` çağrılır.
    9. Başarısız deposit → yakılır (`rent_to_burn`).
  - `#[allow(clippy::needless_collect)]`: `into_iter().map(...).collect::<Vec<_>>()` — iter'ın tüketilmesi gerekiyor çünkü `total_staked` döngü içinde hesaplanıyor.
  - Test modunda boş validator_stakes → capitalization düş + return (test uyumluluğu).
  - `assert!(!validator_stakes.is_empty())` production'da panic.
  - Son `assert_eq!(leftover_lamports, 0)` — dağıtım invariant'ı: tüm lamport'lar ya dağıtıldı ya yakıldı.

- **`pub(super) fn distribute_rent_fees(&self)`** (satır 260-286):
  - `collected_rent.load(Relaxed)` — bu slotta toplanan rent.
  - `should_collect_rent()` → false ise: eğer sıfır değilse warn et, return.
  - `rent.calculate_burn(total_rent_collected)` → `(burned_portion, rent_to_be_distributed)`.
  - `capitalization.fetch_sub(burned_portion, Relaxed)` — yakılan kısım supply'dan düşer.
  - `distribute_rent_to_validators()` çağrılır.

### Matematiksel Detay — Rent Dağıtım Formülü

```
rent_share_i = (stake_i * rent_to_be_distributed) / total_staked
```
- Integer division → kayıp lamport'lar `leftover_lamports` olarak tutulur.
- Leftover dağıtımı: validatörler azalan stake sırasına göre sıralanmış → her birine 1 ekstra lamport.
- Bu, toplam dağıtılan miktarın `rent_to_be_distributed`'a tam eşit olmasını garantiler.
- u128 kullanımı: `(staked as u128) * (rent_to_be_distributed as u128)` — 64-bit overflow önlenir.

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Test Coverage

7 test fonksiyonu:

- **`test_distribute_transaction_fees`** (satır 304-413):
  - 6 senaryo: Normal/InvalidOwner/RentPaying × disable_checks true/false
  - Her kombinasyon için: capitalization değişimi, balance değişimi, rewards listesi doğrulanır.

- **`test_distribute_transaction_fees_zero`** (satır 416-433):
  - Sıfır fee durumunda hiçbir şey değişmez.

- **`test_distribute_transaction_fees_burn_all`** (satır 436-459):
  - `burn_percent = 100` → tüm fee yakılır, capitalization düşer, balance değişmez.

- **`test_distribute_transaction_fees_overflow_failure`** (satır 462-488):
  - Balance = `u64::MAX` → overflow → fee yakılır.

- **`test_deposit_fees`** (satır 491-508):
  - Normal deposit: yeni balance = initial + deposit.

- **`test_deposit_fees_with_overflow`** (satır 511-528):
  - `u64::MAX` balance + 500 → `LamportOverflow` hatası.

- **`test_deposit_fees_invalid_account_owner`** (satır 531-566):
  - Vote account (non-system owner) → `InvalidAccountOwner`.
  - `check_account_owner: false` ile bypass.

- **`test_deposit_fees_invalid_rent_paying`** (satır 569-608):
  - `initial + deposit < rent_exempt_minimum` → `InvalidRentPayingAccount`.
  - `check_rent_paying: false` ile bypass.

- **`test_distribute_rent_to_validators_rent_paying`** (satır 611-758):
  - 4 validatör: empty (0 lamport), rent_paying, becomes_rent_exempt, rent_exempt.
  - EAH sonrası: empty ve rent_paying validatörler fee alamaz (rent check), diğerleri alır.
  - Detaylı balance ve RentState assertion'ları.

- **`test_distribute_rent_to_validators_invalid_owner`** (satır 761-820):
  - 4 kombinasyon: disable_owner_check × use_invalid_owner.

### Tasarım Kararları

- **Atomik ordering `Relaxed`:** Fee toplama ve capitalization güncellemeleri aynı thread'de sıralı olduğundan `Relaxed` yeterli; daha güçlü ordering gerekmiyor.
- **u128 aritmetiği:** 64-bit × 64-bit overflow'u önlemek için intermediate u128 kullanımı — blockchain matematiğinde standart yaklaşım.
- **Leftover dağıtım determinizmi:** Aynı sıralama (stake azalan, pubkey tiebreak) her validator'da aynı leftover dağıtımını üretir → consensus kritik.
- **Burn fallback:** Deposit başarısız olursa lamport'lar yakılır (supply azalır) — ama `capitalization.fetch_sub` çağrılır, bu da toplam supply ile tracked kapitalizasyonu dengede tutar.
- **Feature flag `validate_fee_collector_account`:** Owner/rent kontrolü bir feature flag arkasında — geriye dönük uyumluluk.

### Nova Shield İçin Öğrenilen

- **Stake-weighted fee distribution:** Her epoch'ta validatörlere fee dağıtımı stake ağırlığına göre yapılır. Nova Shield'de de bu yaklaşım direkt uygulanabilir — hem tx fee hem storage fee için.
- **Leftover lamport problemi:** Integer division'dan kaynaklanan artık lamport'ların deterministic dağıtımı. Nova Shield'de token dağıtım hesaplamaları u128 kullanmalı ve artık miktar açıkça işlenmeli.
- **Deposit validation:** Fee almaya hak kazanmak için hesabın rent-exempt olması şartı ekonomik teşvik hizalaması yaratır — validatörler yeterli bakiye tutmak zorunda.
- **Burn mechanism:** Yakma mekanizması deflasyonist ekonomi modeline katkıda bulunur. Nova Shield tokenomics'inde burn oranı (şu an %50) parametrize edilebilir.
- **Atomics with Relaxed:** Single-threaded mutation + Relaxed ordering kombinasyonu performans için önemli.

---

## Dosya 6: metrics.rs (197 satır)

**Amaç:** Bank oluşturma ve epoch reward süreçlerinin performans metriklerini `datapoint_info!` macro aracılığıyla telemetri sistemine (InfluxDB/Prometheus) iletmek.

### Struct/Enum/Trait Listesi

- **`NewEpochTimings`** (satır 7-13) — `pub(crate)`:
  - `thread_pool_time_us: u64`
  - `apply_feature_activations_time_us: u64`
  - `activate_epoch_time_us: u64`
  - `update_epoch_stakes_time_us: u64`
  - `update_rewards_with_thread_pool_time_us: u64`
  - Yeni epoch geçişinde yapılan her adımın süresini taşır.

- **`RewardsMetrics`** (satır 16-24) — `#[derive(Debug, Default)]`, `pub(crate)`:
  - `load_vote_and_stake_accounts_us: AtomicU64` — paralel yüklemede thread-safe.
  - `calculate_points_us: AtomicU64` — thread-safe.
  - `redeem_rewards_us: u64` — tek thread, atomic değil.
  - `store_stake_accounts_us: AtomicU64`
  - `store_vote_accounts_us: AtomicU64`
  - `vote_accounts_cache_miss_count: usize`
  - `hash_partition_rewards_us: u64`
  - **Neden karışık atomic/non-atomic?** Paralel worker thread'leri olan alanlar `AtomicU64`, sadece main thread'den güncellenenler `u64`.

- **`NewBankTimings`** (satır 26-45) — `pub(crate)`:
  - 18 alan: bank oluşturmanın tüm adımlarını microsaniye cinsinden ölçer.
  - `bank_rc_creation_time_us`, `total_elapsed_time_us`, `status_cache_time_us`, `fee_components_time_us`, `blockhash_queue_time_us`, `stakes_cache_time_us`, `epoch_stakes_time_us`, `builtin_programs_time_us`, `rewards_pool_pubkeys_time_us`, `executor_cache_time_us`, `transaction_debug_keys_time_us`, `transaction_log_collector_config_time_us`, `feature_set_time_us`, `ancestors_time_us`, `update_epoch_time_us`, `recompilation_time_us`, `update_sysvars_time_us`, `fill_sysvar_cache_time_us`.

- **`RewardsStoreMetrics`** (satır 160-168) — `#[derive(Debug, Default)]`, `pub(crate)`:
  - `partition_index: u64` — hangi partition
  - `store_stake_accounts_us: u64`
  - `store_stake_accounts_count: usize`
  - `total_stake_accounts_count: usize`
  - `distributed_rewards: u64`
  - `pre_capitalization: u64`
  - `post_capitalization: u64`
  - Partitioned epoch reward crediting'in metrikleri.

### Kritik Fonksiyonlar

- **`pub(crate) fn report_new_epoch_metrics(epoch, slot, parent_slot, timings, metrics)`** (satır 47-108):
  - `datapoint_info!("bank-new_from_parent-new_epoch_timings", ...)` macro çağrısı.
  - 15 metric alanı raporlar.
  - `AtomicU64` alanlar `.load(Relaxed)` ile okunur.

- **`pub(crate) fn report_new_bank_metrics(slot, parent_slot, block_height, timings)`** (satır 110-156):
  - `datapoint_info!("bank-new_from_parent-heights", ...)` macro çağrısı.
  - 20 metric alanı raporlar.

- **`pub(crate) fn report_partitioned_reward_metrics(bank: &Bank, timings: RewardsStoreMetrics)`** (satır 170-197):
  - `datapoint_info!("bank-partitioned_epoch_rewards_credit", ...)` macro çağrısı.
  - Bank'tan slot/epoch/block_height/parent_slot çeker + timings alanları.

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Test Coverage

Bu dosyada `#[cfg(test)]` bloğu yoktur. Metrikler production ortamında gerçek çalıştırma ile test edilir.

### Tasarım Kararları

- **Separation of concerns:** Metrik toplama ve raporlama mantığı ayrı dosyada → bank.rs şişmez.
- **`pub(crate)` visibility:** Crate dışı kullanım yok — internal instrumentation.
- **`datapoint_info!` macro:** Solana'nın telemetri abstraction'ı — backend değişse kod değişmez.
- **AtomicU64 seçimi:** `RewardsMetrics`'te paralel okunan alanlar atomic — false sharing yoktur ama yanlış sonuç da yoktur.

### Nova Shield İçin Öğrenilen

- **Granular timing:** Bank oluşturmanın her adımını (`fill_sysvar_cache_us`, `recompilation_time_us` gibi) ayrı ayrı ölçmek; bottleneck tespiti için kritik. Nova Shield'de de block processing'in her fazı microsaniye granülaritesiyle ölçülmeli.
- **Metrics struct ayrımı:** `NewBankTimings` vs `NewEpochTimings` vs `RewardsStoreMetrics` — farklı event türleri için farklı struct. Nova Shield'de de her major event için ayrı metrics struct tasarlanmalı.
- **AtomicU64 for parallel metrics:** Paralel worker thread'leri olan operasyonların metriklerini AtomicU64 ile topla, raporlama sırasında tek okuma yap.

---

## Dosya 7: serde_snapshot.rs (638 satır)

**Amaç:** Bank struct'ının snapshot'tan serileştirilmesi ve deserileştirilmesini test etmek — tam snapshot round-trip, incremental snapshot, EAH içeren snapshot, extra fields backward compat.

**Not:** Bu dosya da yalnızca `#[cfg(test)] mod tests { ... }` içerir. Production kodu yoktur.

### Struct/Enum/Trait Listesi

Hiç struct/enum/trait tanımı yoktur. Tüm dosya test kodu.

### Kritik Fonksiyonlar (Test Helpers ve Test Fonksiyonları)

- **`fn copy_append_vecs<P: AsRef<Path>>(accounts_db: &AccountsDb, output_dir: P) -> Result<StorageAndNextAppendVecId, AccountsFileError>`** (satır 53-91):
  - Snapshot unpack sırasındaki storage reconstruction'ı simüle eder.
  - `get_snapshot_storages(RangeFull)` ile tüm storage entry'leri alır.
  - Her storage: orijinal dosyayı `output_dir`'e kopyalar, yeni `AccountsFile` okur, `AccountStorageEntry::new_existing` oluşturur.
  - `next_append_vec_id = next_append_vec_id.max(new_storage_entry.append_vec_id())` — ID monotonic artış.

- **`fn test_bank_serialize_style(serde_style, reserialize_accounts_hash, update_accounts_hash, incremental_snapshot_persistence, initial_epoch_accounts_hash)`** (satır 93-303):
  - Ana test helper — 5 boolean parametre ile kombinatoryal test.
  - **Senaryo:**
    1. `genesis_config` (400 slot/epoch) ile bank0 oluştur.
    2. bank1 (slot 1), bank2 (EAH penceresinde veya slot 2) oluştur.
    3. bank2'ye key2 ve key3 hesapları ekle.
    4. `bank2.freeze()` → `squash()` → `force_flush_accounts_cache()`.
    5. accounts_hash set et (`Hash::new(&[0; 32])`).
    6. `bank_to_stream()` ile serialize et.
    7. EAH isteniyorsa `epoch_accounts_hash_manager.set_valid()`.
    8. `reserialize_accounts_hash` isteniyorsa: pre_path'e yaz, `reserialize_bank_with_new_accounts_hash()` çağır, reserialized dosyayı oku.
    9. `bank_from_streams()` ile deserialize et.
    10. Balance, accounts_hash, incremental_hash, EAH ve bank equality assert et.
  - `std::mem::size_of::<Option<BankIncrementalSnapshotPersistence>>() - std::mem::size_of::<u64>()` — incremental persistence eklenmesiyle beklenen dosya boyutu farkı hesaplanır.

- **`#[test] fn test_bank_serialize_newer`** (satır 306-327):
  - `test_bank_serialize_style`'ı `SerdeStyle::Newer` ile çalıştırır.
  - 3 × 2 × 2 = 12 kombinasyon test edilir.

- **`fn add_root_and_flush_write_cache(bank: &Bank)`** (satır 329-332):
  - Bank slot'u root olarak ekle + write cache'i flush et.

- **`#[test] fn test_extra_fields_eof`** (satır 335-433):
  - 3 `epoch_reward_status_active` senaryosu: None, Some(empty), Some([rewards]).
  - `fee_rate_governor.lamports_per_signature = 7000` set et.
  - Serialize → deserialize → `lamports_per_signature` ve `epoch_reward_status` korunmuş mu?
  - `EpochRewardStatus::Active` içindeki `start_block_height` ve `stake_rewards_by_partition` doğrulanır.

- **`#[test] fn test_extra_fields_full_snapshot_archive`** (satır 436-526):
  - Aynı senaryoyu gerçek snapshot archive (tar) üzerinden test eder.
  - `bank_to_full_snapshot_archive()` → `bank_from_snapshot_archives()`.
  - `ArchiveFormat::Tar`, `NonZeroUsize::new(1)` (1 shard).

- **`#[test] fn test_blank_extra_fields`** (satır 528-600):
  - `bank_to_stream_no_extra_fields()` ile extra fields serialize edilmez.
  - Deserialize → default değerler kontrol edilir: `lamports_per_signature = 0`, `EpochRewardStatus::Inactive`.

- **`#[cfg(RUSTC_WITH_SPECIALIZATION)] mod test_bank_serialize`** (satır 602-637):
  - ABI stabilitesi için: `#[frozen_abi(digest = "7BH2s2Y1yKy396c3ixC4TTyvvpkyenAvWDSiZvY5yb7P")]` ile bank serialization format'ının değişmemesi garanti altına alınır.
  - `BankAbiTestWrapperNewer` → `wrapper_newer()` fonksiyonuyla serialize edilir.
  - `RUSTC_WITH_SPECIALIZATION` feature flag gerektiriyor.

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Tasarım Kararları

- **Round-trip testing:** Serialize + deserialize + equality assertion — en güçlü snapshot test stratejisi.
- **Kombinatoryal parametreler:** `test_bank_serialize_style` tek fonksiyona tüm parametreler → kombinasyon testi ekonomik.
- **ABI freeze:** `frozen_abi` attribute ile bank serialization hash'i dondurulur — istem dışı format değişimi deploy'da fark edilir.
- **Buffer boyutu validasyonu:** Reserialized dosyanın boyutu beklenen boyuta tam eşit olması `assert_eq!(size, expected)` ile kontrol edilir.

### Nova Shield İçin Öğrenilen

- **Snapshot format stabilitesi:** `frozen_abi` gibi bir mekanizma ile snapshot format'ı versiyonlanmalı. Format değişikliği = network split riski.
- **Extra fields backward compat:** Eski snapshot'lara yeni alan ekleme: `bank_to_stream_no_extra_fields` → deserialize → default değer pattern'i ile geriye dönük uyumluluk sağlanır. Nova Shield'de de snapshot forward/backward compat planlanmalı.
- **Incremental snapshot metadata boyutu:** `sizeof_incremental_snapshot_persistence - sizeof_none` gibi exact byte boyutu testleri format stabilitesini güvence altına alır.
- **EAH ve snapshot entegrasyonu:** Epoch accounts hash snapshot'a dahil edilmesi, validator'ların snapshot'tan boot sonrası EAH doğrulaması yapabilmesi için kritik.

---

## Dosya 8: sysvar_cache.rs (179 satır)

**Amaç:** Bank'ın sysvar cache'ini yönetmek — eksik sysvar'ları doldur, cache'i reset et, test erişimi sağla.

### Struct/Enum/Trait Listesi

Yeni struct/enum/trait tanımı yoktur. `impl Bank { ... }` bloğu içerir.

### Kritik Fonksiyonlar

- **`pub(crate) fn fill_missing_sysvar_cache_entries(&self)`** (satır 7-14):
  - `sysvar_cache.write().unwrap()` ile write lock alır.
  - `sysvar_cache.fill_missing_entries(|pubkey, callback| {...})`:
    - Her eksik sysvar pubkey'i için `get_account_with_fixed_root(pubkey)` → `callback(account.data())` çağrılır.
    - Hesap yoksa callback çağrılmaz → sysvar cache eksik kalır (tolere edilebilir).
  - Sadece eksik olanları doldurur → mevcut cache girişlerini override etmez.

- **`pub(crate) fn reset_sysvar_cache(&self)`** (satır 16-19):
  - `sysvar_cache.write().unwrap()` → `sysvar_cache.reset()`.
  - Tüm cache'i temizler. Genellikle test'te veya bank reset sırasında.

- **`pub fn get_sysvar_cache_for_tests(&self) -> SysvarCache`** (satır 21-27):
  - `sysvar_cache.read().unwrap().clone()` — snapshot'ı döner.
  - Sadece test erişimi için — `pub` ama isim `_for_tests` içeriyor.

### Test Coverage

2 test:

- **`test_sysvar_cache_initialization`** (satır 43-109) — `#[allow(deprecated)]`:
  - bank0 oluştur → sysvar_cache durumunu kontrol et.
  - bank0: `clock` ✓, `epoch_schedule` ✓, `fees` ✓, `rent` ✓, `slot_hashes` ✗ (slot 0'da yok), `epoch_rewards` ✗ (feature disabled).
  - bank1 (slot 1) oluştur → `slot_hashes` artık ✓.
  - bank2 (slot 2) oluştur → `slot_hashes` farklı değer ✓.
  - `clock` slot'a göre değişir, `epoch_schedule` ve `rent` değişmez.
  - `fees` slot'a göre değişir (blockhash queue güncellenmesi).

- **`test_reset_and_fill_sysvar_cache`** (satır 113-178) — `#[allow(deprecated)]`:
  - bank1 oluştur → tüm sysvar'lar cache'de ✓.
  - `reset_sysvar_cache()` → tüm sysvar'lar ✗.
  - `enable_partitioned_epoch_reward` feature aktive et.
  - `create_epoch_rewards_sysvar()` → hesap oluştur.
  - `fill_missing_sysvar_cache_entries()` → tüm sysvar'lar geri gelir.
  - `epoch_rewards` sysvar artık ✓ (önceki değerle eşit).

### Unsafe Bloklar

Hiç `unsafe` yoktur.

### Tasarım Kararları

- **`RwLock` kullanımı:** Sysvar cache read-heavy, write-rare → `RwLock` ideal. `.read().unwrap()` / `.write().unwrap()` panics on poisoned lock — bu Solana'da yaygın pattern (lock poisoning = programmatic bug).
- **`fill_missing_entries` callback pattern:** Cache hangi sysvar'ların eksik olduğunu biliyor; bank'a hangi hesapları getireceğini söylüyor. Dependency injection: cache bank'ı doğrudan değil callback aracılığıyla kullanıyor.
- **`get_account_with_fixed_root`:** Normal `get_account` yerine fixed root kullanımı — cache doldurmada snapshot-safe okuma yapılır.

### Nova Shield İçin Öğrenilen

- **Lazy sysvar cache:** Sysvar'lar her transaction'da AccountsDB'den okumak yerine cache'den okunur → büyük performans kazanımı. Nova Shield'de de system-level state (epoch info, fee schedule, slot hashes) agresif şekilde cache'lenmeli.
- **Reset + fill pattern:** Bank state reset sırasında cache tamamen temizlenip yeniden doldurulur. Bu, stale cache sorununu önler.
- **Feature-gated sysvars:** `epoch_rewards` sysvar sadece feature aktifse cache'e girer. Nova Shield'de de yeni system state öğeleri feature flag'lerle kademeli olarak aktive edilebilir.
- **`_for_tests` naming convention:** Test-only public API'lar bu suffix ile işaretlenir — prod kodu ile test infra'sı ayrımı netleşir.

---

## Dosya 9: tests.rs (13.794 satır)

**Amaç:** `Bank` struct'ının kapsamlı entegrasyon test suite'i — 200+ test fonksiyonu ile Solana'nın en büyük test dosyalarından biri.

### Genel Yapı

```
tests.rs
├── Imports (satır 1-200): 50+ crate ve modül import
├── Helper fonksiyonlar (satır 200-250): new_from_parent, test utils
├── Test fonksiyonları (satır 250-13794): ~200+ test
```

### Import Analizi (satır 1-200)

Kritik import'lar:
- `crossbeam_channel::{bounded, unbounded}` — async/concurrent test'ler
- `rayon::ThreadPoolBuilder` — paralel test'ler
- `solana_accounts_db::accounts_index::*` — index scanning test'ler
- `solana_program_runtime::compute_budget::ComputeBudget` — fee hesap test'ler
- `itertools::Itertools` — test veri manipülasyonu
- `rand::Rng` — fuzzing

### Test Kategorileri ve Önemli Test Fonksiyonları

#### 1. Bank Başlatma ve Temel Operasyonlar (satır 250-800)

- **`test_race_register_tick_freeze`** (satır 203-260):
  - Eş zamanlı tick/freeze race condition testi.
  - `std::thread::spawn` ile parallel thread'ler — `AtomicBool` ile sync.

- **`test_bank_new`** (satır 283-328):
  - Genesis config ile bank oluşturma.
  - Capitalization: `genesis_config.lamports` + sysvar/builtin lamport'ları.
  - `bank.ticks_per_slot()`, `bank.epoch_schedule()` doğrulamaları.

- **`test_bank_block_height`** (satır 330-336):
  - Block height = parent_block_height + 1.

- **`test_bank_capitalization`** (satır 409-452):
  - Capitalize invariant: her hesap mutasyonunda toplam lamport korunur.

#### 2. Rent Mekanizmaları (satır 800-1800)

- **`test_credit_debit_rent_no_side_effect_on_hash`** (satır 456-717):
  - Rent collection hash'i etkilemez doğrulaması.
  - Karmaşık multi-account senaryo.

- **`test_rent_distribution`** (satır 801-1039):
  - Rent toplama ve stake-weighted dağıtım tam döngü.
  - 3 farklı stake seviyesiyle validatör.

- **`test_rent_eager_across_epoch_*`** (satır 1282-1545):
  - Gap'li/gap'siz, warmup ile/sans epoch geçişinde rent collection.
  - `test_rent_eager_across_epoch_without_gap_under_multi_epoch_cycle` (satır 1363): 1000 epoch boyunca rent cycle testi.

- **`test_rent_eager_collect_rent_in_partition`** (satır 1637-1751):
  - Tek partition'da rent collection → `should_collect_rent` parametresi.

- **`test_collect_rent_from_accounts`** (satır 1754-1804):
  - Birden fazla hesaptan rent toplama.

#### 3. Ödül Mekanizmaları (satır 1868-2128)

- **`test_bank_update_vote_stake_rewards`** (satır 1868-2100):
  - Vote ve stake reward'larının dağıtımı.
  - Epoch başında `update_rewards` çağrısı.
  - Post-epoch account balance'ları doğrulama.

- **`test_bank_update_rewards_determinism`** (satır 2101-2127):
  - `do_test_bank_update_rewards_determinism()` iki kez çağrılır → aynı sonuç.
  - SECONDS_PER_YEAR / MINIMUM_SLOTS_PER_EPOCH / DEFAULT_TICKS_PER_SLOT tick duration ile tam yıl epoch simülasyonu.

#### 4. Transaction Processing (satır 2225-4100)

- **`test_two_payments_to_one_party`** (satır 2226-2241)
- **`test_one_source_two_tx_one_batch`** (satır 2243-2270)
- **`test_one_tx_two_out_atomic_fail`** (satır 2272-2292) — atomic tx semantiği
- **`test_one_tx_two_out_atomic_pass`** (satır 2294-2315) — atomic tx semantiği
- **`test_detect_failed_duplicate_transactions`** (satır 2317-2344)
- **`test_bank_tx_fee`** (satır 2530-2633):
  - Fee toplama: `lamports_per_signature` × signature sayısı.
  - Capitalization ve balance değişimi.
- **`test_bank_tx_compute_unit_fee`** (satır 2635-2743):
  - Priority fee + base fee hesaplaması.
- **`test_filter_program_errors_and_collect_fee`** (satır 2869-2918):
  - Başarısız program hatalarında bile fee toplanır.
- **`test_debits_before_credits`** (satır 2980-3005):
  - Solana atomic transaction semantiği: debits önce işlenir.

#### 5. Locking ve Concurrency (satır 3082-3192)

- **`test_readonly_accounts`** (satır 3007-3081)
- **`test_interleaving_locks`** (satır 3083-3130)
- **`test_readonly_relaxed_locks`** (satır 3132-3191)

#### 6. Hash ve State Verification (satır 3247-3560)

- **`test_bank_hash_internal_state`** (satır 3327-3355):
  - Bank hash, parent hash değişince değişir.
  - Account store sonrası hash değişir.
- **`test_bank_hash_internal_state_verify`** (satır 3357-3423):
  - `verify_bank_hash_and_lamports()` çağrısı.
- **`test_bank_hash_consistency`** (satır 6452-6502):
  - Deterministik: aynı genesis → aynı bank hash.
  - Mainnet uyumluluk anchor.

#### 7. Builtin Programs (satır 4559-4766)

- **`test_add_builtin`** (satır 4559-4613):
  - Custom builtin program ekleme ve çalıştırma.
- **`test_add_builtin_account`** (satır 6769-6864):
  - Builtin account'un capitalization'a doğru dahil edilmesi.

#### 8. Nonce Transactions (satır 4983-5848)

- **`test_check_transaction_for_nonce_ok`** (satır 4983-5015)
- **`test_nonce_transaction`** (satır 5200-5332):
  - Durable nonce tam döngü.
  - Nonce advance sonrası fee deduction.
- **`test_nonce_transaction_with_tx_wide_caps`** (satır 5334-5459):
  - Compute budget ile nonce tx.
- **`test_nonce_fee_calculator_updates`** (satır 5653-5719)
- **`test_check_ro_durable_nonce_fails`** (satır 5789-5847):
  - Read-only nonce account → fail.

#### 9. Fee Calculation (satır 10005-10200)

- **`calculate_test_fee(message, lamports_per_signature, fee_structure) -> u64`** (satır 10005-10015):
  - Test helper: `process_compute_budget_instructions` → `fee_structure.calculate_fee()`

- **`test_calculate_fee`** (satır 10018-10064):
  - 0 imza → 0 fee.
  - 1 imza → `lamports_per_signature`.
  - 2 imza → `2 * lamports_per_signature`.

- **`test_calculate_fee_compute_units`** (satır 10066-10127):
  - `ComputeBudget::set_compute_unit_price` ile priority fee.
  - `compute_unit_limit × price / 10^6` formülü.

- **`test_calculate_prioritization_fee`** (satır 10129-10161):
  - Heap frame, compute units, prioritization kombinasyonları.

- **`test_calculate_fee_secp256k1`** (satır 10163-10201):
  - Secp256k1 imzaları ayrı fee hesaplaması.

- **`test_calculate_fee_with_congestion_multiplier`** (satır 11775-11808):
  - Future: congestion-based fee multiplier.

#### 10. Accounts Data Size (satır 10235-10990)

- **`test_accounts_data_size_with_good_transaction`** (satır 10235-10274):
  - Transaction başarılı → data size doğru güncellenir.
- **`test_accounts_data_size_with_bad_transaction`** (satır 10276-10361):
  - Transaction başarısız → data size değişmez.
- **`test_cap_accounts_data_allocations_per_transaction`** (satır 11630-11671):
  - Tek tx'de maksimum data allocation limit.

#### 11. Rent State Transitions (satır 10363-10930)

- **`test_invalid_rent_state_changes_existing_accounts`** (satır 10363-10470):
  - RentExempt → RentPaying geçişi yasak.
  - RentPaying → RentExempt geçişi serbest (pozitif).
- **`test_invalid_rent_state_changes_fee_payer`** (satır 10680-10924):
  - Fee payer rent state değişimi kontrolü — 10 farklı combination.

#### 12. Feature Activation (satır 7883-7983)

- **`test_compute_active_feature_set`** (satır 7883-7932):
  - Feature activation set hesaplaması.
- **`test_feature_activation_idempotent`** (satır 11673-11699):
  - Aynı feature iki kez aktive etmek idempotent.
- **`test_feature_hashes_per_tick`** (satır 11701-11773):
  - PoH hashes per tick feature transition.

#### 13. Program Loading (satır 7114-7882)

- **`test_bank_load_program`** (satır 7114-7159):
  - BPF program yükleme.
- **`test_bpf_loader_upgradeable_deploy_with_max_len`** (satır 7161-7881):
  - 718 satır tek test! Upgradeable program deployment tam döngü.
  - `InitializeBuffer`, `Write`, `DeployWithMaxDataLen`, `Upgrade` instruction'larını test eder.
  - Program data account size hesaplaması.

#### 14. Snapshot ve Skipped Rewrites (satır 13599-13766)

- **`test_rehash_with_skipped_rewrites`** (satır 13599-13654):
  - Rent-exempt hesapların rent collection sırasında "skipped rewrite" olarak işaretlenmesi.
  - `rehash()` sonrası bank hash değişmemeli.
- **`test_rebuild_skipped_rewrites`** (satır 13657-13766):
  - Snapshot sonrası `calculate_skipped_rewrites()` orijinalle eşleşmeli.
  - Full snapshot → restore → skipped_rewrites eşitliği.

#### 15. Reward Dağıtımı (satır 13162-13492)

- **`test_calc_vote_accounts_to_store_empty`** (satır 13162-13167)
- **`test_calc_vote_accounts_to_store_overflow`** (satır 13170-13187):
  - `u64::MAX` bakiye + 1 reward → overflow → reward 0.
- **`test_calc_vote_accounts_to_store_three`** (satır 13190-13238):
  - 3 vote account: `vote_needs_store` true olanı sakla, diğerleri `None`.
- **`test_calc_vote_accounts_to_store_normal`** (satır 13241-13282):
  - 8 kombinasyon: commission × vote_rewards × vote_needs_store.
- **`test_calculate_stake_vote_rewards`** (satır 13406-13491):
  - `calculate_stake_vote_rewards()` tam senaryo.
  - `PointValue { rewards: 100_000, points: 1_000 }` — her point için 100 lamport.
  - Vote ve stake reward'larının ayrı hesaplanması.

#### 16. Hard Fork ve Last Restart Slot (satır 13494-13592)

- **`test_register_hard_fork`** (satır 13494-13515):
  - `bank7.register_hard_fork(6)` → reddedilir (slot 7'den önce).
  - `bank9.register_hard_fork(9)` → frozen bank'ta reddedilir.
- **`test_last_restart_slot`** (satır 13518-13592):
  - `last_restart_slot_sysvar` feature activation testi.
  - Hard fork öncesi ve sonrası `LastRestartSlot` sysvar güncellenmesi.

#### 17. Simulation (satır 13769-13794)

- **`test_failed_simulation_compute_units`** (satır 13770-13794):
  - Başarısız simulate'da bile consume edilen compute units doğru raporlanır.
  - `TEST_UNITS = 10_000` + `MOCK_BUILTIN_UNITS = 1` = 10_001 toplam.

### Unsafe Bloklar

`tests.rs` içinde hiç `unsafe` bloğu yoktur.

### Özel Patterns

- **`declare_process_instruction!` macro:** Test-only custom builtin'ler için kullanılır.
- **`Bank::new_with_mockup_builtin_for_tests`:** Mock builtin ile bank oluşturma helper.
- **`create_genesis_config_no_tx_fee_no_rent`:** Sıfır fee + sıfır rent ile genesis — fee/rent'in testi bozmadığı durumlarda.
- **`goto_end_of_slot(bank.clone())`:** Bank'ı slot sonuna götür (fill ticks).
- **`new_bank_from_parent_with_bank_forks`:** BankForks ile kayıtlı bank oluşturma.
- **Inline `impl Bank` blokları test içinde:** Satır 13284-13289, sadece test amaçlı `get_reward_total_num_blocks` implementasyonu.

### Test Coverage Özeti

| Kategori | Yaklaşık Test Sayısı |
|---|---|
| Bank init / capitalization | 15 |
| Rent collection & distribution | 25 |
| Reward distribution | 20 |
| Transaction processing | 35 |
| Locking & concurrency | 8 |
| Hash & state verification | 20 |
| Nonce transactions | 12 |
| Fee calculation | 15 |
| Accounts data size | 10 |
| Rent state transitions | 10 |
| Feature activation | 8 |
| Program loading (BPF) | 10 |
| Snapshot & skipped rewrites | 5 |
| Vote/stake rewards | 8 |
| Hard fork & last restart | 3 |
| Simulation | 1 |
| Misc / stress | 20+ |
| **TOPLAM** | **~225+** |

### Tasarım Kararları

- **Tek mega-dosya:** 13.794 satırlık test dosyası — bank.rs'den bağımsız tutulmuş. Ayrı test dosyasının avantajı: prod kod compile time'ı azalır.
- **`#[allow(deprecated)]`:** Eski Fees sysvar API hala test edilmek zorunda — backward compat.
- **Helper fonksiyon mimarisi:** `create_genesis_config_no_tx_fee_no_rent`, `add_root_and_flush_write_cache` gibi helpers — test kod tekrarını azaltır.
- **Determinism testleri:** Reward dağıtımı, hash hesaplaması aynı input → aynı output.
- **Fuzz test:** `test_fuzz_instructions` — random instruction sequences ile edge case tespiti.

### Nova Shield İçin Öğrenilen

- **Test architecture:** 225+ entegrasyon testi tek dosyada — her özellik için pozitif, negatif, boundary ve determinism testi. Nova Shield'in test suite'i de bu kapsamda olmalı.
- **Atomic transaction semantiği:** `test_one_tx_two_out_atomic_fail` — tx kısmen başarılı olamaz. Nova Shield'de de multi-instruction tx'ler ya tamamen başarılı ya tamamen başarısız olmalı.
- **Simülasyon metrikleri:** `simulate_transaction` başarısız tx'te bile `units_consumed` doğru döndürmeli — gas estimation için kritik.
- **Nonce/durable nonce:** Solana'nın durable nonce mekanizması offline signing ve MEV koruması için kritik. Nova Shield'de benzer mechanism gerekebilir.
- **Hard fork güvenliği:** Geçmiş slotlarda ve frozen bank'ta hard fork kaydı reddedilir — bu güvenlik invariant'ı Nova Shield'de de korunmalı.
- **Skipped rewrites:** Rent-exempt hesapların rent epoch'u güncellenmeden "skip" edilmesi ama hash'e dahil edilmesi — ince ama kritik state tracking. Nova Shield'de rent-like mekanizma varsa bu pattern incelenmeli.
- **BPF program deployment testi:** 718 satır deployment testi — her adım tek tek doğrulanıyor. Nova Shield program deployment pipeline'ı da bu detaylıkta test edilmeli.

---

## Genel Değerlendirme: runtime/src/bank/ Alt Modülleri

### Mimari Pattern Özeti

Bu 9 dosya birlikte `bank.rs`'nin şişmesini önleyen bir modülerleştirme stratejisi uygular:

| Dosya Türü | Dosyalar | Katkı |
|---|---|---|
| Trait implementasyonu | address_lookup_table.rs | External trait → Bank |
| Serialization helpers | bank_hash_details.rs | Debug/diag tools |
| Utility fonksiyonlar | epoch_accounts_hash_utils.rs | EAH math |
| Business logic | fee_distribution.rs | Ekonomi modeli |
| Metrics/telemetry | metrics.rs | Observability |
| Test-only kod | builtin_programs.rs, serde_snapshot.rs | Snapshot tests |
| Cache management | sysvar_cache.rs | Performance |
| Ana test suite | tests.rs | Coverage |

### Kritik Ekonomi Modeli Bulguları

1. **Transaction fee:** `collector_fees × (1 - burn_percent/100)` → validatöre, kalanı yakılır.
2. **Rent fee:** `rent_collected × (1 - burn_percent/100)` → stake'e orantılı validatör dağıtımı.
3. **Integer distribution:** `(stake × rent) / total_stake` + leftover deterministic dağıtımı.
4. **Burn:** Yakılan lamport → `capitalization.fetch_sub(burn)` → supply azalır.
5. **Deposit guard:** Fee alıcı hesap rent-paying durumuna düşemez, owner kontrolü yapılır.

### Konsensüs Kritik Invariant'lar

- `assert_eq!(leftover_lamports, 0)` — tüm rent dağıtılır veya yakılır.
- Rent dağıtım sıralaması deterministik: stake azalan, pubkey tiebreak.
- EAH hesaplama penceresi: epoch'un %25-%75'i arası.
- Snapshot format hash'i dondurulmuş: `frozen_abi(digest = "7BH2...")`.

---

## Nova Shield İçin Konsolide Eylem Planı

### Direkt Uygulanabilir Kararlar

1. **Fee burn mekanizması:** `burn_percent` parametresi → Nova Shield tokenomics'inde deflasyonist baskı yaratmak için kullanılabilir. Önerilen: %50 burn, %50 validatöre.

2. **Stake-weighted fee distribution:**
   ```
   share_i = (stake_i × total_fee) / total_stake
   ```
   u128 intermediate, deterministic sıralama, leftover dağıtımı — direkt kopyalanabilir.

3. **Sysvar cache pattern:** System-level state (clock, epoch schedule, fees, rent, slot_hashes) → `RwLock<Cache>` → `fill_missing_entries` callback pattern. Nova Shield'de de System-State-Cache katmanı tasarlanmalı.

4. **EAH benzeri global hash:** Her epoch sonunda tüm accounts'un hash'i → Light client state verification. Solana'da epoch'un %25-%75 arası — Nova Shield'de benzer window belirlenmeli, network observability data ile.

5. **AddressLoader trait abstraction:** Nova Shield'in address resolution mekanizması (hangi hesap adresinin hangi program tarafından erişileceği) trait arkasına saklanmalı.

6. **Metrics struct pattern:** Her major operation için ayrı `XxxTimings` ve `XxxMetrics` struct → `datapoint_info!` veya Nova Shield'in metrics backend'ine raporlama.

7. **Content-addressed debug files:** `{slot}-{hash}.json` pattern ile bank state debug dosyaları → hash mismatch debugging için kritik.

8. **Snapshot round-trip test:** Bank serialize → deserialize → equality assertion, plus ABI freeze. Nova Shield'in snapshot mekanizması baştan bu testlerle güvence altına alınmalı.

9. **Frozen bank requirement for hash:** Hash alınmadan önce bank kesinlikle freeze edilmiş olmalı → precondition contract.

10. **test_bpf_loader pattern:** Program deployment'ın her adımı ayrı ayrı test edilmeli — Nova Shield'in smart contract deployment pipeline'ı da adım bazlı test edilmeli.
