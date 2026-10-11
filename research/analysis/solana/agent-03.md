# Solana Kaynak Kodu Analizi — Agent 03

**Analiz edilen dosyalar:**
- `sdk/program/src/clock.rs`
- `sdk/program/src/entrypoint.rs`
- `sdk/program/src/epoch_schedule.rs`
- `sdk/program/src/feature.rs`
- `sdk/program/src/fee_calculator.rs`
- `sdk/program/src/compute_units.rs`
- `sdk/program/src/epoch_rewards.rs`
- `sdk/program/src/decode_error.rs`

---

## 1. `clock.rs` — Zaman Sabitleri ve Clock Yapısı

### Genel Bakış

Bu dosya Solana'nın zaman modelini tanımlar. Solana'da zaman birimi _slot_'tur; her slot yaklaşık 400 ms sürer ve her slot için bir lider (leader) seçilir.

### Sabitler

| Satır | Sabit | Değer | Açıklama |
|-------|-------|-------|----------|
| 28 | `DEFAULT_TICKS_PER_SECOND` | `160` | Saniyede hedeflenen tick sayısı |
| 34 | `MS_PER_TICK` | `1000 / 160 = 6` | Tick başına milisaniye |
| 42 | `DEFAULT_TICKS_PER_SLOT` | `64` | Slot başına tick sayısı (lider rotasyonu ve oylama her 400 ms) |
| 45 | `DEFAULT_HASHES_PER_SECOND` | `2_000_000` | GCP n1-standard / Xeon E5-2520 v4 referans hash hızı |
| 50 | `UPDATED_HASHES_PER_SECOND_2` | `2_800_000` | Mainnet'te stake'in %97.6'sı bu hızın üstünde (Temmuz 2023) |
| 52 | `UPDATED_HASHES_PER_SECOND_3` | `4_400_000` | Stake'in %96.2'si bu hızın üstünde |
| 54 | `UPDATED_HASHES_PER_SECOND_4` | `7_600_000` | Stake'in %96.2'si bu hızın üstünde |
| 56 | `UPDATED_HASHES_PER_SECOND_5` | `9_200_000` | Stake'in %96.2'si bu hızın üstünde |
| 58 | `UPDATED_HASHES_PER_SECOND_6` | `10_000_000` | Stake'in %96.2'si bu hızın üstünde |
| 62 | `DEFAULT_HASHES_PER_TICK` | `2_000_000 / 160 = 12_500` | Tick başına hash sayısı (PoH proof yoğunluğu) |
| 66-82 | `UPDATED_HASHES_PER_TICK2..6` | 17_500 / 27_500 / 47_500 / 57_500 / 62_500 | Güncellenmiş ağ hızlarına karşılık gelen tick başına hash |
| 85 | `DEFAULT_DEV_SLOTS_PER_EPOCH` | `8192` | Dev epoch = 400 ms * 8192 ≈ 55 dakika |
| 89 | `SECONDS_PER_DAY` | `24 * 60 * 60 = 86_400` | Gün başına saniye |
| 93 | `TICKS_PER_DAY` | `160 * 86_400 = 13_824_000` | Gün başına tick |
| 101 | `DEFAULT_SLOTS_PER_EPOCH` | `2 * 13_824_000 / 64 = 432_000` | Epoch başına slot (≈ 2 gün) |
| 104 | `NUM_CONSECUTIVE_LEADER_SLOTS` | `4` | Lider rotasyon granülaritesi (her lider 4 ardışık slot alır) |
| 109 | `DEFAULT_MS_PER_SLOT` | `1000 * 64 / 160 = 400` | Slot başına hedef süre (ms) |
| 110 | `DEFAULT_S_PER_SLOT` | `64.0 / 160.0 = 0.4` | Slot başına saniye (f64) |
| 120 | `MAX_HASH_AGE_IN_SECONDS` | `120` | Recent blockhash'in geçerli olduğu maksimum süre (sn) |
| 125 | `MAX_RECENT_BLOCKHASHES` | `120 * 160 / 64 = 300` | Takip edilen maksimum recent blockhash sayısı |
| 131 | `MAX_PROCESSING_AGE` | `300 / 2 = 150` | Lider tarafından kabul edilen maksimum blockhash yaşı |
| 135 | `MAX_TRANSACTION_FORWARDING_DELAY_GPU` | `2` | GPU ile imza doğrulamada maksimum yönlendirme gecikmesi (slot) |
| 138 | `MAX_TRANSACTION_FORWARDING_DELAY` | `6` | CUDA olmadan yönlendirme gecikmesi (slot) |
| 141 | `FORWARD_TRANSACTIONS_TO_LEADER_AT_SLOT_OFFSET` | `2` | Lider öncesi yönlendirme başlangıç offseti |
| 142 | `HOLD_TRANSACTIONS_SLOT_OFFSET` | `20` | Bekletme slot offseti |

### Matematiksel Formüller

```
MS_PER_TICK = 1000 / DEFAULT_TICKS_PER_SECOND
            = 1000 / 160
            = 6 ms/tick

DEFAULT_MS_PER_SLOT = 1000 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND
                    = 1000 * 64 / 160
                    = 400 ms/slot

DEFAULT_SLOTS_PER_EPOCH = 2 * TICKS_PER_DAY / DEFAULT_TICKS_PER_SLOT
                        = 2 * 13_824_000 / 64
                        = 432_000 slots/epoch  (≈ 2 gün)

MAX_RECENT_BLOCKHASHES = MAX_HASH_AGE_IN_SECONDS * DEFAULT_TICKS_PER_SECOND / DEFAULT_TICKS_PER_SLOT
                       = 120 * 160 / 64
                       = 300

MAX_PROCESSING_AGE = MAX_RECENT_BLOCKHASHES / 2 = 150
```

### Tip Tanımları (Satır Numarasıyla)

| Satır | Tip | İçerik |
|-------|-----|--------|
| 147 | `type Slot = u64` | Zaman birimi (slot numarası) |
| 153 | `type BankId = u64` | Slot versiyonunu eşsiz tanımlayan ID (duplicate slot desteği) |
| 158 | `type Epoch = u64` | Epoch numarası |
| 160 | `const GENESIS_EPOCH: Epoch = 0` | Genesis epoch numarası |
| 162 | `const INITIAL_RENT_EPOCH: Epoch = 0` | İlk rent epoch'u |
| 165 | `type SlotIndex = u64` | Epoch içindeki slot indeksi |
| 168 | `type SlotCount = u64` | Epoch başına slot sayısı tipi |
| 173 | `type UnixTimestamp = i64` | Unix zaman damgası (saniye) |

### Struct: `Clock` (Satır 178–200)

```rust
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, CloneZeroed, Default, PartialEq, Eq)]
pub struct Clock {
    pub slot: Slot,                        // Mevcut slot
    pub epoch_start_timestamp: UnixTimestamp, // Bu epoch'un başlangıç zamanı
    pub epoch: Epoch,                      // Mevcut epoch
    pub leader_schedule_epoch: Epoch,      // Son hesaplanan lider schedule'ın epoch'u
    pub unix_timestamp: UnixTimestamp,     // Oracle tabanlı gerçek dünya zamanı
}
```

**Önemli tasarım kararları:**
- `CloneZeroed` derive'ı özel bir makrodur; `Clone` yerine sıfır-tabanlı bir bellek kopyalama gerçekleştirir (güvenli bellek boşaltma / padding byte'larını sıfırlama).
- `#[repr(C)]` — C ABI uyumluluğu, BPF programlarından sysvar olarak erişmek için gerekli.
- `unix_timestamp`, başlangıçta slot*ms hesabıyla tahmin edilirdi ve büyük sürüklenme yaşanırdı. `timestamp_correction` ve `timestamp_bounding` feature'larından sonra validator oracle'dan türetiliyor.

### Test (Satır 203–218)

`test_clone`: `Clock` struct'ının `Clone` implementasyonunun tüm alanları doğru kopyaladığını doğrular.

### static_assertions Kullanımı

Dosya boyunca `static_assertions::const_assert_eq!` makrosu ile derleme zamanında sabit değerlerin doğruluğu kontrol edilir. Örneğin:
- `MS_PER_TICK == 6`
- `DEFAULT_MS_PER_SLOT == 400`
- `MAX_RECENT_BLOCKHASHES == 300`
- `DEFAULT_SLOTS_PER_EPOCH == 432_000`

Bu, refaktoring sırasında sabit tutarsızlıklarını anında yakalar.

---

## 2. `entrypoint.rs` — BPF Program Giriş Noktası

### Genel Bakış

Bu dosya Solana BPF programlarının çalışma zamanına nasıl bağlandığını tanımlar. Bir Solana programı çalıştığında, runtime bu modüldeki `entrypoint` fonksiyonunu çağırır; bu fonksiyon raw binary veriyi `(program_id, accounts, instruction_data)` üçlüsüne deserialize eder ve kullanıcının `process_instruction` fonksiyonunu çağırır.

### Tip ve Sabit Tanımları

| Satır | Tanım | Değer/Tip |
|-------|-------|-----------|
| 22 | `type ProgramResult = Result<(), ProgramError>` | Program başarı/hata tipi |
| 28–29 | `type ProcessInstruction` | `fn(&Pubkey, &[AccountInfo], &[u8]) -> ProgramResult` |
| 32 | `const SUCCESS: u64 = 0` | Program başarı dönüş değeri |
| 35 | `const HEAP_START_ADDRESS: u64 = 0x300000000` | BPF heap bellek bölgesi başlangıcı |
| 37 | `const HEAP_LENGTH: usize = 32 * 1024` | BPF heap boyutu: 32 KB |
| 40 | `const NON_DUP_MARKER: u8 = u8::MAX` | Hesabın duplicate olmadığını işaret eden byte (255) |
| 263 | `const MAX_PERMITTED_DATA_INCREASE: usize = 1_024 * 10` | Tek realloc'ta izin verilen max data artışı: 10 KB |
| 266 | `const BPF_ALIGN_OF_U128: usize = 8` | BPF'de u128 hizalaması (host'tan farklı) |

### Makro: `entrypoint!` (Satır 124–140)

```rust
#[macro_export]
macro_rules! entrypoint {
    ($process_instruction:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
            let (program_id, accounts, instruction_data) =
                unsafe { $crate::entrypoint::deserialize(input) };
            match $process_instruction(&program_id, &accounts, &instruction_data) {
                Ok(()) => $crate::entrypoint::SUCCESS,
                Err(error) => error.into(),
            }
        }
        $crate::custom_heap_default!();
        $crate::custom_panic_default!();
    };
}
```

**Tasarım kararları:**
- `#[no_mangle] extern "C"`: Runtime'ın C ABI üzerinden `entrypoint` sembolünü bulabilmesi için.
- `unsafe extern "C"`: Raw pointer'larla çalışmak zorunda; tampon overrun imkansız çünkü runtime tarafından serialize edilmiş veri kullanılıyor.
- `custom_heap_default!()` ve `custom_panic_default!()` otomatik eklenir; isteyen crate `custom-heap` / `custom-panic` Cargo feature'ını açarak bunları devre dışı bırakıp kendi implementasyonunu kullanabilir.

### Makro: `custom_heap_default!` (Satır 161–171)

`target_os = "solana"` ve `feature = "custom-heap"` yoksa `BumpAllocator`'ı global allocator olarak kurar.

### Makro: `custom_panic_default!` (Satır 217–227)

`target_os = "solana"` ve `feature = "custom-panic"` yoksa `custom_panic` fonksiyonunu tanımlar; panic mesajını `msg!()` ile runtime'a iletir.

**Not:** Panic handler'ı no-op yaparak program binary boyutunu ~25 KB azaltmak mümkündür.

### Struct: `BumpAllocator` (Satır 230–233)

```rust
pub struct BumpAllocator {
    pub start: usize,
    pub len: usize,
}
```

**Unsafe `GlobalAlloc` implementasyonu (Satır 238–260):**

```rust
unsafe impl std::alloc::GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pos_ptr = self.start as *mut usize;
        let mut pos = *pos_ptr;
        if pos == 0 {
            pos = self.start + self.len; // İlk çağrı: heap'in en üstünden başla
        }
        pos = pos.saturating_sub(layout.size());
        pos &= !(layout.align().wrapping_sub(1)); // hizalama
        if pos < self.start + size_of::<*mut u8>() {
            return null_mut(); // OOM
        }
        *pos_ptr = pos;
        pos as *mut u8
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        // Bump allocator: serbest bırakma yok
    }
}
```

**Tasarım özellikleri:**
- **Bump (aşağı büyüyen) allocator**: Position her zaman heap'in en üstünden aşağıya iner. Stack gibi davranır ama "free" yok.
- Heap başlangıcının ilk `size_of::<*mut u8>()` byte'ı `pos` (mevcut pozisyon) için rezerve edilir.
- `pos == 0` kontrolü: İlk çağrıda pointer henüz ayarlanmamış; başlangıç noktası heap'in en üstüdür.
- `saturating_sub` + `&= !(align-1)`: Hizalama için pointer aşağı yuvarlanır.
- `dealloc` no-op: Program sonrası tüm bellek runtime tarafından temizlenir, tek tek serbest bırakmaya gerek yok.
- Performans: `O(1)` alloc, sıfır fragmantasyon — kısa ömürlü program bellekleri için idealdir.

### Unsafe Fonksiyon: `deserialize` (Satır 277–371)

```rust
pub unsafe fn deserialize<'a>(input: *mut u8) -> (&'a Pubkey, Vec<AccountInfo<'a>>, &'a [u8])
```

**Serialization formatı (binary layout):**
```
[num_accounts: u64]
for each account:
  [dup_info: u8]           // 255 = not dup, otherwise = index of original
  if not dup:
    [is_signer: u8]
    [is_writable: u8]
    [executable: u8]
    [original_data_len: u32]  // padding repurposed for realloc tracking
    [key: Pubkey (32 bytes)]
    [owner: Pubkey (32 bytes)]
    [lamports: u64]
    [data_len: u64]
    [data: data_len bytes]
    [padding: MAX_PERMITTED_DATA_INCREASE bytes]
    [u128 alignment padding]
    [rent_epoch: u64]
  else:
    [7 bytes padding]
[instruction_data_len: u64]
[instruction_data: instruction_data_len bytes]
[program_id: Pubkey (32 bytes)]
```

**Önemli detaylar:**
- Satır 308: `original_data_len_offset` — orijinal olarak padding olan 4 byte, realloc boyutunu takip etmek için repurpose edildi. Geriye dönük uyumlu bir hack.
- Satır 327: `data_len as u32` olarak yazılır; `MAX_PERMITTED_DATA_LENGTH`'in u32'ye sığacağı varsayılır.
- Satır 332–333: `data_len + MAX_PERMITTED_DATA_INCREASE` byte ayırtılır; realloc için boş alan bırakılır.
- Satır 333: `align_offset(BPF_ALIGN_OF_U128)` — u128 hizalaması için padding eklenir.
- Satır 318–319: `lamports` alanı `Rc<RefCell<&mut u64>>` olarak sarılır; birden fazla AccountInfo aynı account'ı paylaşabilir ve borrow checker'ı atlatmak için bu sarmalama gereklidir.
- Satır 353: Duplicate account için orijinalin `clone()`'u kullanılır; `Rc` referans sayacı artar.

### Test: `test_bump_allocator` (Satır 378–436)

Üç senaryo test eder:
1. Tüm heap'i 1 byte'lık allocation'larla doldurma
2. Farklı hizalamalar için `align_offset` kontrolü (u8, u16, u32, u64, u128, 64-byte)
3. Büyük tek bir allocation (120 byte, 128-byte heap'ten)

---

## 3. `epoch_schedule.rs` — Epoch Zamanlama

### Genel Bakış

Epoch schedule, genesis'ten itibaren değişmeyen (immutable) ağ yapılandırmasıdır. Warmup döneminde epoch'lar kısa başlar ve her seferinde iki katına çıkarak `DEFAULT_SLOTS_PER_EPOCH`'a ulaşır.

### Sabitler

| Satır | Sabit | Değer | Açıklama |
|-------|-------|-------|----------|
| 18 | `DEFAULT_LEADER_SCHEDULE_SLOT_OFFSET` | `DEFAULT_SLOTS_PER_EPOCH (432_000)` | Lider schedule hesaplama başlangıç offseti |
| 24 | `MAX_LEADER_SCHEDULE_EPOCH_OFFSET` | `3` | Maksimum önceden hesaplanabilir epoch sayısı |
| 29 | `MINIMUM_SLOTS_PER_EPOCH` | `32` | Warmup'ta minimum epoch boyutu (`MAX_LOCKOUT_HISTORY`'den) |

### Struct: `EpochSchedule` (Satır 31–54)

```rust
#[repr(C)]
#[derive(Debug, CloneZeroed, PartialEq, Eq, Deserialize, Serialize, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct EpochSchedule {
    pub slots_per_epoch: u64,             // Epoch başına maksimum slot
    pub leader_schedule_slot_offset: u64, // Lider schedule önceden hesaplama offseti
    pub warmup: bool,                     // Warmup modu etkin mi?
    pub first_normal_epoch: Epoch,        // İlk normal epoch numarası
    pub first_normal_slot: Slot,          // İlk normal slot numarası
}
```

**`first_normal_epoch` hesabı:**
```
first_normal_epoch = log2(slots_per_epoch) - log2(MINIMUM_SLOTS_PER_EPOCH)
                   = log2(432_000.next_power_of_two()) - log2(32)
                   = log2(524_288) - 5
                   = 19 - 5
                   = 14 (örnek)
```

**`first_normal_slot` hesabı:**
```
first_normal_slot = slots_per_epoch.next_power_of_two() - MINIMUM_SLOTS_PER_EPOCH
```

### `impl EpochSchedule` Metotları

#### `fn new(slots_per_epoch: u64) -> Self` (Satır 67–69)
Warmup etkin, `leader_schedule_slot_offset = slots_per_epoch` ile oluşturur.

#### `fn without_warmup() -> Self` (Satır 70–76)
Warmup kapalı, default offset ile oluşturur. Genellikle test için.

#### `fn custom(slots_per_epoch, leader_schedule_slot_offset, warmup) -> Self` (Satır 77–99)

Warmup etkinse:
```
next_power_of_two = slots_per_epoch.next_power_of_two()
log2_slots_per_epoch = trailing_zeros(next_power_of_two) - trailing_zeros(MINIMUM_SLOTS_PER_EPOCH)
first_normal_epoch = log2_slots_per_epoch (u64)
first_normal_slot  = next_power_of_two - MINIMUM_SLOTS_PER_EPOCH
```

Warmup kapalıysa `first_normal_epoch = 0`, `first_normal_slot = 0`.

#### `fn get_slots_in_epoch(&self, epoch: Epoch) -> u64` (Satır 102–110)

```
if epoch < first_normal_epoch:
    slots = 2^(epoch + trailing_zeros(MINIMUM_SLOTS_PER_EPOCH))
          = 2^(epoch + 5)   [MINIMUM = 32 = 2^5]
          = 32, 64, 128, ..., MINIMUM*2^(first_normal_epoch-1)
else:
    slots = slots_per_epoch
```

Warmup döneminde her epoch'ta slot sayısı ikiyle çarpılır.

#### `fn get_leader_schedule_epoch(&self, slot: Slot) -> Epoch` (Satır 114–129)

Warmup sırasında lider schedule her zaman bir sonraki epoch için hesaplanır.
Normal dönemde:
```
new_slots_since_first_normal_slot = slot - first_normal_slot
leader_schedule_epoch = first_normal_epoch +
    (new_slots_since_first_normal_slot + leader_schedule_slot_offset) / slots_per_epoch
```

#### `fn get_epoch(&self, slot: Slot) -> Epoch` (Satır 132–134)
`get_epoch_and_slot_index`'in ilk elemanını döndürür.

#### `fn get_epoch_and_slot_index(&self, slot: Slot) -> (Epoch, u64)` (Satır 137–165)

Warmup döneminde:
```
epoch = trailing_zeros((slot + MINIMUM_SLOTS_PER_EPOCH + 1).next_power_of_two())
        - trailing_zeros(MINIMUM_SLOTS_PER_EPOCH) - 1
epoch_len = 2^(epoch + trailing_zeros(MINIMUM_SLOTS_PER_EPOCH))
slot_index = slot - (epoch_len - MINIMUM_SLOTS_PER_EPOCH)
```

Normal dönemde:
```
normal_slot_index = slot - first_normal_slot
epoch = first_normal_epoch + (normal_slot_index / slots_per_epoch)
slot_index = normal_slot_index % slots_per_epoch
```

#### `fn get_first_slot_in_epoch(&self, epoch: Epoch) -> Slot` (Satır 167–178)

```
if epoch <= first_normal_epoch:
    first_slot = (2^epoch - 1) * MINIMUM_SLOTS_PER_EPOCH
else:
    first_slot = (epoch - first_normal_epoch) * slots_per_epoch + first_normal_slot
```

#### `fn get_last_slot_in_epoch(&self, epoch: Epoch) -> Slot` (Satır 180–184)

```
last_slot = get_first_slot_in_epoch(epoch) + get_slots_in_epoch(epoch) - 1
```

### Test: `test_epoch_schedule` (Satır 192–246)

`MINIMUM_SLOTS_PER_EPOCH..=MINIMUM_SLOTS_PER_EPOCH * 16` (32..=512) aralığındaki tüm slot değerleri için:
- Epoch'ların sürekli arttığını doğrular (boşluksuz)
- Leader schedule epoch'larının sürekli arttığını doğrular
- Warmup sırasında her epoch'ta slot sayısının ikiye katlandığını doğrular
- Slot offset'inin epoch boyutundan küçük olduğunu doğrular

---

## 4. `feature.rs` — Runtime Feature Aktifleştirme

### Genel Bakış

Solana'nın feature flag sistemi. Yeni özellikler koordineli olarak ağa entegre edilir: feature aktifleştirme isteği bir hesap oluşturarak yapılır; sonraki epoch başında runtime aktifleştirip slot numarasını kaydeder.

### ID Tanımı (Satır 19)

```rust
crate::declare_id!("Feature111111111111111111111111111111111111");
```

Feature sistemi programının public key'i. Sysvar benzeri özel bir program adresi.

### Struct: `Feature` (Satır 21–24)

```rust
#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Feature {
    pub activated_at: Option<Slot>,
}
```

- `None`: Feature henüz aktif değil (sadece talep edilmiş)
- `Some(slot)`: Feature bu slot'ta aktifleştirildi

### `impl Feature` (Satır 26–38)

#### `fn size_of() -> usize` (Satır 27–29)
```rust
pub const fn size_of() -> usize {
    9 // bincode: 1 byte (Option discriminant) + 8 byte (u64 slot)
}
```

Bu değer `Some(0)` ve `Some(u64::MAX)` için de aynı kalır çünkü bincode'da `Option<u64>` her zaman 9 byte'tır. Test (satır 66–92) bunu doğrular.

#### `fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError>` (Satır 31–37)
- Owner kontrolü: Hesabın feature program'ına ait olması gerekir.
- bincode deserialize eder.

### Yardımcı Fonksiyonlar

#### `fn activate(feature_id, funding_address, rent) -> Vec<Instruction>` (Satır 41–47)
Kira hesabını kullanarak `activate_with_lamports`'u çağırır.

#### `fn activate_with_lamports(feature_id, funding_address, lamports) -> Vec<Instruction>` (Satır 49–59)
Üç instruction içeren bir batch:
1. `transfer`: funding_address → feature_id (lamports)
2. `allocate`: feature_id için 9 byte alan tahsis et
3. `assign`: feature_id'yi Feature program'ına ata

Bu üç işlem atomik değil (ayrı transaction olabilir) ama sıralamaları önemlidir.

---

## 5. `fee_calculator.rs` — Ücret Hesaplama

### Genel Bakış

Solana'nın dinamik ücret sistemi. İmza başına ücret (`lamports_per_signature`) ağ yüküne göre otomatik olarak ayarlanır.

### Sabitler

| Satır | Sabit | Değer | Açıklama |
|-------|-------|-------|----------|
| 75 | `DEFAULT_TARGET_LAMPORTS_PER_SIGNATURE` | `10_000` | İmza başına hedef ücret (lamport) |
| 76 | `DEFAULT_TARGET_SIGNATURES_PER_SLOT` | `50 * 400 = 20_000` | Slot başına hedef imza sayısı |
| 79 | `DEFAULT_BURN_PERCENT` | `50` | Ücretin yanacak yüzdesi |

`DEFAULT_TARGET_SIGNATURES_PER_SLOT = 50 * DEFAULT_MS_PER_SLOT = 50 * 400 = 20_000` — Bu değer Solana'nın slot başına işlemek istediği imza kapasitesini gösterir.

### Struct: `FeeCalculator` (Satır 9–18)

```rust
#[repr(C)]
#[derive(Serialize, Deserialize, Default, PartialEq, Eq, Clone, Copy, Debug, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct FeeCalculator {
    pub lamports_per_signature: u64,
}
```

**Deprecated (1.9.0):** `calculate_fee(&self, message: &Message) -> u64`

Ücret = `lamports_per_signature * (num_required_signatures + secp256k1/ed25519 imzaları)`

Bu API artık kullanılmamalıdır; ücret hesaplama runtime seviyesine taşındı.

### Struct: `FeeRateGovernor` (Satır 51–73)

```rust
pub struct FeeRateGovernor {
    #[serde(skip)]                         // Runtime-hesaplama, serialize edilmez
    pub lamports_per_signature: u64,       // Mevcut imza ücreti
    pub target_lamports_per_signature: u64, // Hedef ücret
    pub target_signatures_per_slot: u64,   // Hedef throughput (kapasite tahmini)
    pub min_lamports_per_signature: u64,   // Minimum ücret
    pub max_lamports_per_signature: u64,   // Maksimum ücret
    pub burn_percent: u8,                  // Yakılacak yüzde (u8::MAX paydasıyla)
}
```

**Not:** `lamports_per_signature` `#[serde(skip)]` ile işaretlenmiştir — bu alan her slot'ta yeniden hesaplanır, serialize edilmez.

### `impl FeeRateGovernor`

#### `fn new_derived(base: &FeeRateGovernor, latest_signatures_per_slot: u64) -> Self` (Satır 106–166)

Ücret ayarlama algoritması:

```
min = max(1, target / 2)                    // [%50 minimum]
max = target * 10                            // [%1000 maksimum]

desired = clamp(
    target * min(latest_sigs, u32::MAX) / target_sigs,
    min, max
)

gap = desired - current

if gap == 0:
    new_fee = desired
else:
    step = max(1, target / 20)               // [%5 adım]
    adjustment = step * sign(gap)
    new_fee = clamp(current + adjustment, min, max)
```

**Özellikler:**
- Yumuşak geçiş: Her slot'ta en fazla `target/20` (%5) değişim.
- Asimetrik tepki: Hızlı yükseliş için yukarı adım = aşağı adım; her ikisi de sabittir.
- `target_signatures_per_slot == 0` ise dinamik fiyatlama devre dışı bırakılır; ücret her zaman hedef değerde kalır.

#### `fn burn(&self, fees: u64) -> (u64, u64)` (Satır 176–179)

```rust
let burned = fees * u64::from(self.burn_percent) / 100;
(fees - burned, burned)
```

Döndürür: `(validator'a giden, yakılan)`.

**Tasarım kararı:** `burn_percent` `u8` tipindedir ama `/ 100` ile hesaplanır (u8::MAX ile değil). Bu biraz yanıltıcı; `u8::MAX` yorum satırında belirtilmiş ama kod 100 üzerinden hesaplıyor.

#### `fn create_fee_calculator(&self) -> FeeCalculator` (Satır 182–184)

```rust
pub fn create_fee_calculator(&self) -> FeeCalculator {
    FeeCalculator::new(self.lamports_per_signature)
}
```

### Test Özeti

- `test_fee_rate_governor_burn`: `burn_percent` 50, 0, 100 için doğrular.
- `test_fee_calculator_calculate_fee`: 0, 1, 2 imza senaryoları.
- `test_fee_calculator_calculate_fee_secp256k1`: secp256k1 program için `data[0]` imza sayacı doğrulama.
- `test_fee_rate_governor_derived_default`: Default governor'dan türetilen değerin min ücrete eşit olduğunu doğrular (çünkü 0 imzayla latest_sigs gelirse desired = 0 → min).
- `test_fee_rate_governor_derived_adjust`: Ücretin max'a çıkıp sonra min'e inmesi ve hedefe yakınsaması; her aşama < 1000 adımda bitmeli.

---

## 6. `compute_units.rs` — Compute Unit Sorgulama

### Genel Bakış

Son derece kısa bir dosya (14 satır). Programların kalan compute unit sayısını sorgulamasına olanak tanır.

### Fonksiyon: `sol_remaining_compute_units() -> u64` (Satır 3–13)

```rust
#[inline]
pub fn sol_remaining_compute_units() -> u64 {
    #[cfg(target_os = "solana")]
    unsafe {
        crate::syscalls::sol_remaining_compute_units()
    }

    #[cfg(not(target_os = "solana"))]
    {
        crate::program_stubs::sol_remaining_compute_units()
    }
}
```

**Tasarım kararı:** Platform-koşullu derleme. BPF hedefinde gerçek syscall; diğer platformlarda (test, geliştirme) stub implementasyon. `#[inline]` ile sıfır overhead syscall sarıcısı.

**Kullanım:** Program içinde kalan compute unit'i kontrol ederek erken dönebilir veya compute-heavy operasyonlardan önce yeterli bütçe olup olmadığını sorgulayabilir.

---

## 7. `epoch_rewards.rs` — Epoch Ödül Dağıtımı

### Genel Bakış

Epoch başındaki staking ödüllerini takip eden sysvar tipi. Büyük epoch'larda (432_000 slot, ~2 gün) tüm ödülleri aynı anda dağıtmak pahalı olduğundan ödüller birden fazla bloğa yayılır.

### Struct: `EpochRewards` (Satır 11–23)

```rust
#[repr(C)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, Copy, AbiExample)]
pub struct EpochRewards {
    pub total_rewards: u64,                      // Epoch için toplam ödül (lamport)
    pub distributed_rewards: u64,                // Şimdiye kadar dağıtılan ödül (lamport)
    pub distribution_complete_block_height: u64, // Dağıtımın tamamlanacağı blok yüksekliği
}
```

### `impl EpochRewards`

#### `fn distribute(&mut self, amount: u64)` (Satır 26–30)

```rust
pub fn distribute(&mut self, amount: u64) {
    assert!(self.distributed_rewards.saturating_add(amount) <= self.total_rewards);
    self.distributed_rewards.add_assign(amount);
}
```

**Koruma:** `distributed_rewards + amount > total_rewards` olursa panic. `saturating_add` overflow'u önler ama assert ile üst sınır zorlanır.

### Test Özeti

- `test_epoch_rewards_new`: Başlangıç değerlerini doğrular.
- `test_epoch_rewards_distribute`: 100 lamport dağıtımı.
- `test_epoch_rewards_distribute_panic`: Toplam ödülden fazla dağıtmaya çalışınca `#[should_panic]`.

---

## 8. `decode_error.rs` — Özel Hata Decode Etme

### Genel Bakış

Solana programlarında `ProgramError::Custom(u32)` variant'ı custom hata kodlarını taşır. Bu trait, `u32` kodunu orijinal hata enum'una geri dönüştürmeyi sağlar.

### Trait: `DecodeError<E>` (Satır 24–32)

```rust
pub trait DecodeError<E> {
    fn decode_custom_error_to_enum(custom: u32) -> Option<E>
    where
        E: FromPrimitive,
    {
        E::from_u32(custom)
    }
    fn type_of() -> &'static str;
}
```

**Tasarım kararları:**
- `decode_custom_error_to_enum` default implementasyona sahip; `num_traits::FromPrimitive`'e delege eder. Implementor yalnızca `type_of()` metodunu tanımlamak zorundadır.
- `type_of() -> &'static str`: Hata tipi adını string olarak döndürür; logging / debugging için kullanılır.
- Generic `<E>`: Aynı struct farklı hata enum'ları için farklı `DecodeError` implementasyonu sağlayabilir (nadiren kullanılır).

**Kullanım akışı:**
1. Program: `MyError::InsufficientFunds` → `ProgramError::Custom(1)`
2. Client: `u32 = 1` alır → `MyError::decode_custom_error_to_enum(1)` → `Some(MyError::InsufficientFunds)`

### Test (Satır 34–57)

```rust
#[derive(Debug, FromPrimitive, PartialEq, Eq)]
enum TestEnum { A, B, C }
impl<T> DecodeError<T> for TestEnum { fn type_of() -> &'static str { "TestEnum" } }

assert_eq!(TestEnum::decode_custom_error_to_enum(0), Some(TestEnum::A));
assert_eq!(TestEnum::decode_custom_error_to_enum(3), None); // Geçersiz kod
```

---

## Nova Shield Chain İçin Dersler

### 1. Zaman Modeli (clock.rs)

**Ders:** Ticks → Slots → Epochs hiyerarşisi net bir zaman modeli sağlar.

- Nova Shield Chain için: `tick_rate`, `slot_duration`, `epoch_length` ayrı sabitlere bölünmeli. Slot süresi (400 ms) özellikle `NUM_CONSECUTIVE_LEADER_SLOTS = 4` ile lider rotasyonu arasındaki ilişkiyi kolayca değiştirmeye olanak tanır.
- `static_assertions::const_assert_eq!` kullanımı kesinlikle benimsenmeli: tüm türetilmiş zaman sabitleri derleme zamanında doğrulanmalı.
- `BankId` fikri (duplicate slot desteği) önemli: Nova Shield Chain'de de fork/duplicate blok senaryolarını `BlockId != Slot` ayrımıyla modellemek gerekebilir.
- `MAX_PROCESSING_AGE = MAX_RECENT_BLOCKHASHES / 2`: Yayılma gecikmesi için güvenlik bufferı; Nova Shield'da da transaction geçerliliği için benzer çift-buffer mantığı uygulanabilir.
- Validator timestamp oracle: `unix_timestamp`'ın ağ üzerinde oracle ile hesaplanması, block zaman damgalarının tutarlılığı için kritik. MPC zinciri olarak Nova Shield'da validator'ların zaman anlaşmazlıklarını çözmek için eşik tabanlı oracle tercih edilmeli.

### 2. Program Mimarisi (entrypoint.rs)

**Ders:** BumpAllocator tasarımı program VM'leri için altın standart.

- Nova Shield Chain'in wasm/native çalışma ortamında program çalıştıracaksa, `BumpAllocator` benzeri sınırlı heap heap yönetimi benimsenebilir. Her program çağrısı için tam heap temizleme, bellek güvenliğini garanti eder.
- `NON_DUP_MARKER = u8::MAX` gibi sentinel değerler binary protocol'de yararlıdır.
- `MAX_PERMITTED_DATA_INCREASE = 10 KB`: Account boyutu artışını sınırlamak, DoS vektörlerini kapatır. Nova Shield'da da account realloc limiti tanımlanmalı.
- `#[cfg(target_os = "solana")]` ile platform çift-katmanlı derleme: Native'de stub, VM'de gerçek syscall. Nova Shield için de `#[cfg(feature = "native-test")]` vs `#[cfg(target = "nova")]` ayrımı temiz bir test altyapısı sağlar.
- Serialization formatında `original_data_len` için padding byte'ların repurpose edilmesi geriye dönük uyumluluk tekniğidir ama tehlikeli; Nova Shield'da protokol değişikliklerini explicit versiyonlama ile yönetmek daha iyidir.

### 3. Epoch Tasarımı (epoch_schedule.rs)

**Ders:** Warmup mekanizması ağın yavaşça olgunlaşmasını sağlar.

- `MINIMUM_SLOTS_PER_EPOCH = 32` = `MAX_LOCKOUT_HISTORY`'ye eşittir; ağ güvenliği parametreleri birbiriyle tutarlı olmalı.
- Warmup: her epoch'ta slot sayısını ikiye katlamak, validator'ların senkronize olma süresini kısaltır. Nova Shield Chain için de mainnet lansmanında warmup epoch'ları planlanabilir.
- `MAX_LEADER_SCHEDULE_EPOCH_OFFSET = 3`: Lider schedule gelecek epoch'lar için önceden hesaplanır; bu, validator'ların önceden hazırlanmasına imkân tanır. Nova Shield'da rotasyon mekanizması için benzer look-ahead önerilir.
- `get_epoch_and_slot_index`'in warmup dönemindeki karmaşık bit manipülasyonu (`trailing_zeros`, `next_power_of_two`) kafa karıştırıcı olabilir; Nova Shield'da daha basit lineer epoch modeli tercih edilebilir, warmup dönemleri ayrıca yönetilebilir.

### 4. Feature Flag Sistemi (feature.rs)

**Ders:** Koordineli, on-chain feature aktivasyonu.

- Feature'lar bir account olarak temsil edilir; bu sayede aktivasyon işlemlerine imza atılabilir, tarihçe tutulabilir.
- `activated_at: Option<Slot>` minimal alan gereksinimiyle featureların durumunu (bekleyen / aktif) kodlar.
- Epoch sınırlarında aktivasyon: Hard fork yerine sıfırıncı gün uyumluluk. Nova Shield için de konsensüs değişikliklerini epoch başlarında devreye almak zorunludur; otherwise fork riski.
- Feature aktivasyonu 3 instructionlı atomik olmayan işlemler zinciri: İşlem başarısızlık durumları idempotent olacak şekilde tasarlanmıştır.
- Nova Shield'da runtime feature sistemi: Yeni MPC parametrelerinin (threshold değişikliği, eğri değişikliği) on-chain feature flag ile koordineli devreye alınması kritiktir.

### 5. Dinamik Ücret Modeli (fee_calculator.rs)

**Ders:** Yumuşak dinamik fiyatlama DoS'u engeller, aşırı ucuz transaction'ları önler.

- Ücret aralığı `[%50 min, %1000 max]` ve %5 adım: Saldırı senaryolarında ücret hızla yükselir ama sakin dönemlerde minimum'a döner.
- `burn_percent = 50`: Ücretin yarısını yakmak deflasyoner baskı sağlar ve ücret kazancının tüm validatorlara gitmesini engeller (spamı doğrudan subvanse etmez).
- `lamports_per_signature` `#[serde(skip)]`: Ephemeral veriyi ayrı tutmak serializasyon boyutunu azaltır ve state-root hash'ini sabit tutar. Nova Shield'da da hesaplanan (türetilmiş) verileri sysvar'dan ayırmak önerilir.
- `DEFAULT_TARGET_SIGNATURES_PER_SLOT = 50 * 400 = 20_000`: Hedef TPS'nin 50x slot süresiyle orantılandırılması; Nova Shield'da da hedef TPS kapasitesi fee governor parametrelerine yansıtılmalı.
- Nova Shield için: MPC işlemleri (threshold imza üretimi) tek bir imzadan çok daha pahalıdır; fee model MPC round-trip maliyet maliyetini göz önünde bulundurmalı (compute unit tabanlı ücretlendirme daha uygun olabilir).

### 6. Compute Unit Yönetimi (compute_units.rs)

**Ders:** Program içi kaynak sorgulama mekanizması.

- `sol_remaining_compute_units()` ile programlar kendi bütçelerini sorgulayabilir ve graceful degradation uygulayabilir.
- Platform koşullu derleme (`cfg`) test/native ortamlar için stub sağlar.
- Nova Shield Chain'de smart contract'ların hesaplama bütçesi sorgulamasına izin vermek, uzun MPC işlemlerinin transaction timeout'larından önce erken dönmesini sağlar.

### 7. Ödül Dağıtımı (epoch_rewards.rs)

**Ders:** Büyük ödül dağıtımlarını yayma.

- `distribution_complete_block_height`: Ödül dağıtımının ne zaman biteceğini önceden belirtmek, client'ların durumu sorgulamasını kolaylaştırır.
- `assert!` ile aşım önleme: Dağıtılan miktar toplam ödülü geçemez; basit ama kritik invariant.
- `AddAssign` trait kullanımı: Ergonomik numeric mutation için standart trait.
- Nova Shield Chain için: MPC tabanlı staking ödülleri (threshold validators) de fazla dağıtımı önlemek için benzer koruma mekanizmasına ihtiyaç duyar.

### 8. Hata Decode Sistemi (decode_error.rs)

**Ders:** Basit, genişletilebilir hata tip sistemi.

- `num_traits::FromPrimitive`: u32 → enum dönüşümü için prosedürel makro destekli standart yol.
- `DecodeError<E>` trait'inin default implementasyonu: Kullanıcı yalnızca `type_of()` implement etmek zorunda.
- Nova Shield Chain için tüm program hataları bu pattern'i izlemeli: enum → Custom(u32) → geri decode. Bu sayede RPC client'lar hata kodlarını anlamlı mesajlara çevirebilir.

---

## Genel Mimari Gözlemler

### Serialization Stratejisi

- Solana `bincode` kullanır (compact binary format).
- `#[repr(C)]` ile C ABI uyumu: Sysvar'lara raw bellek erişimi için gerekli.
- `#[serde(rename_all = "camelCase")]`: JSON RPC API ile uyumlu.
- `AbiExample` derive: ABI stabilitesini test etmek için Solana'ya özgü derive.

### Güvenlik

- Tüm raw pointer işlemleri `unsafe` blokları içinde ve tam belgelenmiş (`# Safety` bölümü).
- Arithmetic overflow: `saturating_add`, `saturating_sub`, `checked_div` kullanımı yaygın. Overflow riskli yerlerde `#[allow(clippy::arithmetic_side_effects)]` ile kasıtlı kapatılmış.
- Realloc sınırı (`MAX_PERMITTED_DATA_INCREASE`): Account boyutu saldırı vektörünü kapatır.

### Test Kalitesi

- Her modülün test modülü var.
- `static_assertions` ile derleme zamanı doğrulama.
- `#[should_panic]` ile negatif test.
- Parametrik test (`for slots_per_epoch in range`): Sınır koşullarını kapsamlı test eder.

---

*Analiz tamamlandı. Toplam: 8 dosya, ~900 satır kod.*
