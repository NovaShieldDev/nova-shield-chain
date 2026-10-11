# Solana Analiz — Agent 24
## Kapsam: sdk/src/ Kalan Dosyalar — Bölüm 1

**Analiz Tarihi:** 2026-07-22
**Toplam Satır:** ~5.500 satır (18 dosya)
**Dizin:** `/home/ali-haydar-karadas/Desktop/nova-shield-chain/research/chains/solana/sdk/src/`

---

## Analiz Edilen Dosyalar

| Dosya | Satır | Kategori |
|-------|-------|----------|
| feature_set.rs | 1149 | Runtime özellik yönetimi |
| fee.rs | 233 | Ücret hesaplama |
| genesis_config.rs | 366 | Genesis yapısı |
| hard_forks.rs | 98 | Hard fork mekanizması |
| inflation.rs | 142 | Enflasyon modeli |
| packet.rs | 323 | Ağ paketi yapısı |
| quic.rs | 42 | QUIC parametreleri |
| compute_budget.rs | 68 | Compute unit yönetimi |
| ed25519_instruction.rs | 382 | ED25519 precompile |
| derivation_path.rs | 771 | BIP-44 türetme |
| precompiles.rs | 127 | Precompile kayıt sistemi |
| secp256k1_instruction.rs | 1318 | secp256k1 precompile |
| rent_collector.rs | 477 | Kira toplama motoru |
| rent_debits.rs | 64 | Kira borç kaydı |
| reserved_account_keys.rs | 256 | Korumalı hesap anahtarları |
| commitment_config.rs | 243 | Onay seviyeleri |
| offchain_message.rs | 302 | Off-chain mesaj imzalama |
| reward_info.rs | 12 | Ödül bilgisi |
| inner_instruction.rs | 21 | İç instruction yapısı |
| reward_type.rs | 26 | Ödül türleri |
| shred_version.rs | 69 | Shred versiyon hesabı |
| poh_config.rs | 39 | PoH konfigürasyonu |
| timing.rs | 226 | Zaman yardımcı fonksiyonları |
| nonce_account.rs | 133 | Nonce hesap yardımcıları |

---

## Detaylı Dosya Analizleri

---

### [feature_set.rs] (1149 satır)

**Amaç:** Solana runtime'ında tüm özellik (feature) bayraklarını, isimlendirmelerini ve özellik kümesi yönetimini tanımlar.

**Struct/Enum/Trait Listesi:**

- `FeatureSet` (satır 1011): `active: HashMap<Pubkey, Slot>` + `inactive: HashSet<Pubkey>` — aktif/inaktif feature'ların durumu
- `FullInflationFeaturePair` (satır 991): `vote_id` + `enable_id` — tam enflasyonu tetikleyen özellik çiftleri

**Kritik Sabitler / Modüller (feature id'leri):**

Dosya ~150+ adet pub mod içerir. Her birinde `solana_sdk::declare_id!("...")` ile benzersiz bir `Pubkey` tanımlanır.

Kategorilere göre kritik özellikler:

**Enflasyon:**
- `pico_inflation` — 0.0001% test enflasyonu
- `full_inflation::devnet_and_testnet` — devnet/testnet tam enflasyon
- `full_inflation::mainnet::certusone::vote` + `enable` — mainnet tam enflasyon (topluluk oylamasına bağlı çift-anahtar mekanizması)

**Kriptografi / Syscall'lar:**
- `secp256k1_program_enabled` — secp256k1 precompile aktif
- `ed25519_program_enabled` — ed25519 precompile aktif
- `secp256k1_recover_syscall_enabled` — secp256k1_recover syscall
- `blake3_syscall_enabled` — blake3 hash syscall
- `curve25519_syscall_enabled` — curve25519 eğri syscall'ları
- `curve25519_restrict_msm_length` — MSM vektör uzunluğu kısıtlaması
- `enable_alt_bn128_syscall` — alt_bn128 eşleştirme syscall'ları
- `enable_alt_bn128_compression_syscall` — alt_bn128 sıkıştırma
- `enable_big_mod_exp_syscall` — büyük modüler üs alma
- `enable_poseidon_syscall` — Poseidon hash (ZK dostu)
- `zk_token_sdk_enabled` — ZK Token proof program ve syscall'lar
- `enable_zk_transfer_with_fee` — ücretli ZK token transferi
- `enable_zk_proof_from_account` — ZK proof hesaptan okuma

**Ücret / Compute:**
- `tx_wide_compute_cap` — transaction genelinde compute cap
- `add_compute_budget_program` — ComputeBudget programı
- `add_set_compute_unit_price_ix` — öncelik ücreti instruction'ı
- `remove_congestion_multiplier_from_fee_calculation` — sıkışıklık çarpanını kaldır
- `include_loaded_accounts_data_size_in_fee_calculation` — yüklenen hesap verisi boyutu ücrete dahil
- `remove_rounding_in_fee_calculation` — ücret hesabında yuvarlama kaldır
- `add_set_tx_loaded_accounts_data_size_instruction` — compute budget ile hesap veri boyutu limiti
- `cap_transaction_accounts_data_size` — transaction hesap veri boyutu üst sınırı
- `use_default_units_in_fee_calculation` — varsayılan CU ücret hesabında kullan
- `round_up_heap_size` — heap maliyeti hesabında yukarı yuvarlama

**Kira:**
- `skip_rent_rewrites` — kira muaf hesaplarda kira epoch yeniden yazma atla
- `set_exempt_rent_epoch_max` — muaf hesaplara `Epoch::MAX` ata
- `disable_rent_fees_collection` — kira ücreti toplamayı devre dışı bırak
- `preserve_rent_epoch_for_rent_exempt_accounts` — kira muaf hesapların kira epoch'unu koru

**Nonce / Blockhash:**
- `separate_nonce_from_blockhash` — durable nonce ve blockhash alanlarını ayır
- `enable_durable_nonce` — kalıcı nonce aktif
- `nonce_must_be_writable` — nonce hesabı yazılabilir olmalı
- `nonce_must_be_authorized` — nonce yetkilendirilmiş olmalı
- `nonce_must_be_advanceable` — nonce ilerletilebilir olmalı

**Turbine / Gossip / Shred:**
- `drop_redundant_turbine_path` — gereksiz turbine yolunu düşür
- `add_shred_type_to_shred_seed` — shred tipini shred seed'e ekle
- `enable_chained_merkle_shreds` — zincirli Merkle shred
- `drop_legacy_shreds` — eski shred formatlarını düşür
- `enable_gossip_duplicate_proof_ingestion` — gossip üzerinden çift blok kanıtı

**Stake:**
- `reduce_stake_warmup_cooldown` — ısınma/soğuma %25'ten %9'a düşürüldü
- `stake_raise_minimum_delegation_to_1_sol` — minimum stake 1 SOL
- `stake_minimum_delegation_for_rewards` — ödül için minimum stake
- `timely_vote_credits` — oy zamanlamasına göre kredi

**Lazy Static'ler:**
- `FEATURE_NAMES: HashMap<Pubkey, &'static str>` (satır 785) — tüm özellik açıklamaları, ~185 girdi
- `ID: Hash` (satır 980) — mevcut yazılımın tüm feature id'lerinin karmasından üretilen benzersiz kimlik
- `FULL_INFLATION_FEATURE_PAIRS: HashSet<FullInflationFeaturePair>` (satır 999)

**Kritik Fonksiyonlar:**

- `FeatureSet::is_active()` (satır 1026): `active.contains_key(feature_id)` — O(1) HashMap sorgusu
- `FeatureSet::activated_slot()` (satır 1030): hangi slot'ta aktive edildiğini döner
- `FeatureSet::all_enabled()` (satır 1054): tüm feature'ları aktif yapar (test için)
- `FeatureSet::activate()` (satır 1062): inactive'den active'e taşır
- `FeatureSet::deactivate()` (satır 1068): active'den inactive'e taşır
- `FeatureSet::full_inflation_features_enabled()` (satır 1035): çift-anahtar oylamasını kontrol ederek tam enflasyonu tetikler
- `FeatureSet::new_warmup_cooldown_rate_epoch()` (satır 1073): `reduce_stake_warmup_cooldown` aktivasyon epoch'unu döner

**Default Davranışı:**
`FeatureSet::default()` → tüm feature'lar INACTIVE. Mainnet'te feature'lar bank'a `Feature` account'u deploy edilerek aktive edilir.

**Önemli Tasarım Kararı:**
Her feature bir `Pubkey` (hesap adresi) ile temsil edilir. Validatörler, bu adresteki hesabı okuyarak feature'ın ne zaman aktive edildiğini anlar. Bu on-chain governance mekanizmasıdır — kodda değil, chain durumunda saklı.

**Test Fonksiyonları:**
- `test_full_inflation_features_enabled_devnet_and_testnet`
- `test_full_inflation_features_enabled` — çift-anahtar mekanizması (vote+enable) test
- `test_feature_set_activate_deactivate`

**Nova Shield İçin Öğrenilen:**
- Feature gate sistemi kodu hard fork olmadan güncellenebilir yapar. Nova Shield'da da tüm protokol değişiklikleri feature ID'li on-chain flag ile kontrol edilmeli.
- `HashMap<Pubkey, Slot>` (aktif feature + slot numarası) izlenebilirlik sağlar — hangi blok'tan itibaren değiştiğini kaydetmek önemli.
- `FEATURE_NAMES` gibi tek kaynaklı bir dizin, dokümantasyon otomasyonu için değerlidir.
- Feature çifti mekanizması (vote+enable) stake-weighted governance'ın zincir üzerinde nasıl çalışabileceğine dair iyi bir örnek.

---

### [fee.rs] (233 satır)

**Amaç:** Solana transaction ücret yapısını ve hesaplama formüllerini tanımlar.

**Struct/Enum Listesi:**

- `FeeBin` (satır 9): `limit: u64` (compute unit limiti) + `fee: u64` (lamports cinsinden ücret) — kademeli ücret tablosu satırı
- `FeeBudgetLimits` (satır 16): `loaded_accounts_data_size_limit`, `heap_cost`, `compute_unit_limit`, `prioritization_fee`
- `FeeStructure` (satır 25): `lamports_per_signature: u64`, `lamports_per_write_lock: u64`, `compute_fee_bins: Vec<FeeBin>`
- `FeeDetails` (satır 35): `transaction_fee: u64` + `prioritization_fee: u64`

**Önemli Sabitler:**
- `ACCOUNT_DATA_COST_PAGE_SIZE: u64 = 32_u64.saturating_mul(1024)` = 32 KB sayfalama (satır 52)

**Kritik Fonksiyonlar:**

- `FeeStructure::new()` (satır 55): SOL değerlerini lamports'a çevirir, compute_fee_bins oluşturur
- `FeeStructure::get_max_fee()` (satır 74):
  ```
  max_fee = num_signatures × lamports_per_signature
           + num_write_locks × lamports_per_write_lock
           + compute_fee_bins.last().fee
  ```
- `FeeStructure::calculate_memory_usage_cost()` (satır 86):
  ```
  cost = ceil(loaded_accounts_data_size_limit / 32KB) × heap_cost
  ```
  (sayfa bazlı fiyatlandırma, 32 KB bloklar halinde)
- `FeeStructure::calculate_fee_details()` (satır 124):
  ```
  signature_fee = num_signatures × lamports_per_signature
  write_lock_fee = num_write_locks × lamports_per_write_lock
  loaded_accounts_cost = calculate_memory_usage_cost(...)  [feature gate ile]
  total_compute_units = loaded_accounts_cost + compute_unit_limit
  compute_fee = find first FeeBin where total_compute_units <= bin.limit
  transaction_fee = signature_fee + write_lock_fee + compute_fee
  total = transaction_fee + prioritization_fee
  ```
- `FeeDetails::total_fee()` (satır 41): `remove_rounding_in_fee_calculation` feature gate: eski davranış float'a dönüştürüp yuvarlıyordu (`(total as f64).round() as u64`), yeni davranış doğrudan toplam döner.
- `FeeStructure::calculate_fee()` (satır 98): `congestion_multiplier` hesaplar (şu an sabit 1 — eski sıkışıklık çarpanı kaldırıldı)

**Default Değerler** (satır 172):
```rust
FeeStructure::new(0.000005, 0.0, vec![(1_400_000, 0.0)])
// lamports_per_signature = 5000 lamports = 0.000005 SOL
// lamports_per_write_lock = 0
// compute_fee_bins: 1_400_000 CU için 0.0 SOL (yani ücretsiz compute, sadece öncelik ücreti)
```

**Unsafe Bloklar:** YOK

**Nova Shield İçin Öğrenilen:**
- Ücret 3 bileşenden oluşuyor: imza ücreti + write lock ücreti + compute ücreti. Nova Shield benzer ayrıştırma yapmalı.
- `remove_rounding_in_fee_calculation` ile float yuvarlama sorunu önemli bir bug kaynağıydı. Büyük ücretlerde `(u64::MAX as f64).round()` hassasiyeti kaybeder. Nova Shield kesinlikle integer aritmetiği kullanmalı.
- 32 KB sayfalı bellek maliyeti: hesap veri boyutu ücrete dahil edilmesi DDOS'a karşı koruma sağlar.
- `congestion_multiplier` eski koda göre hâlâ kodda var ama etkisiz — bu gelecekte dinamik ücrete geçiş için yer açık.

---

### [genesis_config.rs] (366 satır)

**Amaç:** Solana zincirinin genesis konfigürasyonunu tanımlar — ağın başlangıç parametreleri, hesaplar ve programlar.

**Struct/Enum Listesi:**

- `ClusterType` (satır 45): `Testnet | MainnetBeta | Devnet | Development` — 4 ağ tipi
- `GenesisConfig` (satır 88): `#[frozen_abi(digest = "3V3ZVRyzNhRfe8RJwDeGpeTP8xBWGGFBEbwTkvKKVjEa")]`
  - `creation_time: UnixTimestamp`
  - `accounts: BTreeMap<Pubkey, Account>` — genesis hesapları (sıralı BTreeMap: deterministik hash için)
  - `native_instruction_processors: Vec<(String, Pubkey)>` — built-in programlar
  - `rewards_pools: BTreeMap<Pubkey, Account>` — kapitalizasyona sayılmayan ödül havuzları
  - `ticks_per_slot: u64`
  - `unused: u64` — değeri UNUSED_DEFAULT=1024, ABI uyumluluğu için korunuyor
  - `poh_config: PohConfig`
  - `__backwards_compat_with_v0_23: u64` — v0.23 binary uyumluluğu için boş alan
  - `fee_rate_governor: FeeRateGovernor`
  - `rent: Rent`
  - `inflation: Inflation`
  - `epoch_schedule: EpochSchedule`
  - `cluster_type: ClusterType`

**Önemli Sabitler:**
- `DEFAULT_GENESIS_FILE: &str = "genesis.bin"`
- `DEFAULT_GENESIS_ARCHIVE: &str = "genesis.tar.bz2"`
- `DEFAULT_GENESIS_DOWNLOAD_PATH: &str = "/genesis.tar.bz2"`
- `UNUSED_DEFAULT: u64 = 1024`

**Kritik Fonksiyonlar:**

- `ClusterType::get_genesis_hash()` (satır 56): hardcoded genesis hash'leri — mainnet `5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d`, testnet `4uhcVJyU9pJkvQyS88uRDiswHXSCkY3zQawwpjk2NsNY`, devnet `EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG`
- `GenesisConfig::hash()` (satır 170): tüm genesis config bincode serialize edilip SHA256 alınır
- `GenesisConfig::load()` (satır 179): `memmap2::Mmap` ile dosya belleğe map edilir, sonra bincode deserialize — **unsafe blok** içerir (Mmap::map)
- `GenesisConfig::write()` (satır 208): bincode serialize + `create_dir_all` + dosya yazma
- `GenesisConfig::ns_per_slot()` (satır 238): `target_tick_duration.as_nanos() × ticks_per_slot`
- `GenesisConfig::slots_per_year()` (satır 245): `timing::years_as_slots(1.0, ...)` çağırır

**Unsafe Bloklar:**
- Satır 192: `unsafe { Mmap::map(&file) }` — bellek eşleşmesi için zorunlu

**ABI Kısıtlaması:**
`#[frozen_abi]` directive: Bu struct'ın binary düzeni donmuştur. Değiştiremezsiniz. `__backwards_compat_with_v0_23` ve `unused` alanları bu katılıktan kaynaklanır.

**Nova Shield İçin Öğrenilen:**
- Genesis config'de `BTreeMap` kullanmak kritik: sıralama deterministik hash sağlar. `HashMap` kullanılırsa aynı state farklı hash verebilir.
- `frozen_abi` directive erken uygulanmalı — ileride ABI kırılmaları çok maliyetli.
- Genesis dosyası memmap ile yüklenmesi performans için akıllıca ama unsafe. Nova Shield'da `mlock()` ile de güvence altına alınabilir.
- Sabit genesis hash'leri client-side doğrulamasına imkan verir (hangi ağa bağlıyım kontrolü).

---

### [hard_forks.rs] (98 satır)

**Amaç:** Hard fork slot sınırlarını kayıt altına alır ve bank hash hesabında kullanılacak veriyi sağlar.

**Struct/Enum Listesi:**

- `HardForks` (satır 12): `hard_forks: Vec<(Slot, usize)>` — (slot, sayaç) çiftleri

**Kritik Fonksiyonlar:**

- `HardForks::register()` (satır 17): Bir slot için fork kaydeder. Aynı slot ikinci kez register edilirse sayaç artar (idempotent değil — sayacı artırır). Her kayıt sonrası `sort()` çağrılır.
- `HardForks::iter()` (satır 32): Sıralı iterator döner
- `HardForks::is_empty()` (satır 37): fork listesi boş mu
- `HardForks::get_hash_data()` (satır 42): Bir slot ve parent_slot verildiğinde, parent_slot < fork_slot <= slot koşulunu sağlayan fork'ların toplam sayacını döner. Eğer fork_count > 0 ise 8 byte LittleEndian encoded verisi döner; bu bank hash'ine dahil edilir.

**Mekanizma:**
Hard fork, bank hash'i değiştirir. Aynı genesis'ten başlayan iki node, hard fork sonrası farklı bank hash'lerine sahip olur → ağ bölünür. Bu, eski protokolle uyumsuz node'ları ağdan atmak için kullanılır.

**Test Fonksiyonları:**
- `iter_is_sorted`: register sırası önemli değil, her zaman sıralı
- `multiple_hard_forks_since_parent`: çift fork senaryosu, `(2, [2,0,0,0,0,0,0,0])` sonucu bekleniyor

**Nova Shield İçin Öğrenilen:**
- Bank hash'e fork verisi dahil etme yaklaşımı güçlü: eski software çalışmaya devam eder ama farklı hash üretir → fork güvenceli.
- Sayaç sistemi (aynı slot için birden fazla kayıt): bu kasıtlı olabilir, örneğin birden fazla sebeple aynı slot'ta fork yapılması. Nova Shield'da benzer ihtiyaç olabilir.
- Linear scan performansı: yorum "küçük sayı bekleniyor, eğer büyürse daha verimli yapı gerekir" diyor. Nova Shield da fork sayısını izlemeli.

---

### [inflation.rs] (142 satır)

**Amaç:** Solana'nın enflasyon eğrisini ve validator/foundation dağıtımını modellemler.

**Struct/Enum Listesi:**

- `Inflation` (satır 5): `initial: f64`, `terminal: f64`, `taper: f64`, `foundation: f64`, `foundation_term: f64`, `__unused: f64`

**Önemli Sabitler:**
```
DEFAULT_INITIAL: f64 = 0.08       // %8 başlangıç enflasyonu
DEFAULT_TERMINAL: f64 = 0.015     // %1.5 terminal enflasyon
DEFAULT_TAPER: f64 = 0.15         // yıllık %15 azalma
DEFAULT_FOUNDATION: f64 = 0.05    // toplam enflasyonun %5'i foundation'a
DEFAULT_FOUNDATION_TERM: f64 = 7.0 // 7 yıl süreyle foundation payı
```

**Kritik Fonksiyonlar:**

- `Inflation::total(year: f64)` (satır 84):
  ```
  tapered = initial × (1 - taper)^year
  total = max(tapered, terminal)
  ```
  Formül: exponential decay, minimum terminal'de sabitlenir.

- `Inflation::validator(year)` (satır 96):
  ```
  validator = total(year) - foundation(year)
  ```

- `Inflation::foundation(year)` (satır 101):
  ```
  if year < foundation_term:
      foundation = total(year) × foundation_fraction
  else:
      foundation = 0.0
  ```

- `Inflation::new_disabled()` (satır 45): tüm alanlar 0.0
- `Inflation::new_fixed(validator)` (satır 57): sabit enflasyon — taper=1.0 (azalmaz), foundation=0
- `Inflation::pico()` (satır 68): `new_fixed(0.0001)` — %0.01
- `Inflation::full()` (satır 72): default değerler ama foundation=0

**Enflasyon Eğrisi Örneği (yıl → toplam oran):**
- Yıl 0: %8.0
- Yıl 1: 0.08 × 0.85 = %6.8
- Yıl 2: 0.08 × 0.85² = %5.78
- Yıl ~15: terminal ≈ %1.5 (yaklaşık)
- Yıl ∞: %1.5 (terminal)

**Nova Shield İçin Öğrenilen:**
- Exponential decay enflasyonu matematiksel olarak zarif; tahmin edilebilir arz.
- `__unused` alanı: ABI uyumluluğu için serde'de korunuyor — gelecekte field eklenirse seri bozulmaz trick'i.
- Foundation term sonrası (yıl 7) foundation payı sıfıra düşer — bu tokennomics'te önemli bir geçiş. Nova Shield'da benzer bir kliff mekanizması uygulanabilir.
- `f64` kullanımı: yeterli hassasiyet ama deterministik olmayabilir farklı mimarilerde. Kritik hesaplamalarda fixed-point arithmetic düşünülmeli.

---

### [packet.rs] (323 satır)

**Amaç:** Solana ağ paketi yapısını ve meta veri bayraklarını tanımlar.

**Struct/Enum Listesi:**

- `PacketFlags` (satır 26): `bitflags!` macro ile u8 bayrak seti:
  - `DISCARD = 0b0000_0001` — geçersiz paket, işleme alma
  - `FORWARDED = 0b0000_0010` — başka TPU'ya yönlendirildi
  - `REPAIR = 0b0000_0100` — repair paket
  - `SIMPLE_VOTE_TX = 0b0000_1000` — basit oy işlemi
  - `TRACER_PACKET = 0b0001_0000` — performans izleme paketi
  - `ROUND_COMPUTE_UNIT_PRICE = 0b0010_0000` — feature gate bağlı CU fiyat yuvarlama
- `Meta` (satır 41): `size: usize`, `addr: IpAddr`, `port: u16`, `flags: PacketFlags`
- `Packet` (satır 91): `buffer: [u8; PACKET_DATA_SIZE]` + `meta: Meta` — `#[repr(C)]`

**Önemli Sabitler:**
```
PACKET_DATA_SIZE = 1280 - 40 - 8 = 1232 bytes
// IPv6 minimum MTU = 1280 bytes
// IPv6 header = 40 bytes
// Fragment header = 8 bytes
```
Statik doğrulama: `static_assertions::const_assert_eq!(PACKET_DATA_SIZE, 1232)`

**Kritik Fonksiyonlar:**

- `Packet::data<I>(&self, index: I)` (satır 110): Discard flag kontrol eder; eğer set ise `None` döner. Aksi halde `buffer[..meta.size][index]` döner. Güvenli API: invalid/discarded paketlerin verisi okunamaz.
- `Packet::buffer_mut()` (satır 128): tüm buffer'a mut referans. `debug_assert!(!discard)` ile guard var.
- `Packet::from_data<T: Serialize>()` (satır 143): herhangi serializable veriyi paket'e dönüştürür
- `Packet::populate_packet()` (satır 149): bincode ile buffer'a yazar, `meta.size` günceller
- `Packet::deserialize_slice<T, I>()` (satır 164): bincode options ile boyut sınırlı deserialize; `PACKET_DATA_SIZE` aşılamaz

**Unsafe Bloklar:**
- Satır 192-196: `Default` implementasyonunda `MaybeUninit::<[u8; PACKET_DATA_SIZE]>::uninit().assume_init()` — 1232 byte'lık buffer başlatılmadan kullanılır. Bu performans kritik bir karar: her paket için sıfırlama maliyeti yoktur. `meta.size` doğru set edildiği sürece güvenli (uninit bölge okunmaz).

**Özel Serde Çözümü:**
`serde_as(as = "Bytes")` attribute — Rust'ın büyük array'lerin serde desteğindeki eksikliği için serde_with crate kullanımı. Detaylı yorum satırı 63-87 arası bu geçici çözümü ve ilgili RFC'leri açıklıyor.

**Nova Shield İçin Öğrenilen:**
- `1280 - 40 - 8 = 1232` byte limit: IPv6 MTU temel. Nova Shield L1 de en az 1232 byte paket desteği hedeflemeli; daha büyük bloklar için TCP/QUIC üzerinden straming tercih edilmeli.
- `DISCARD` flag: pipeline başlarında geçersiz paketleri işaretleyip sonraki aşamalara taşıma, sonunda topluca temizleme. Çok verimli early filtering.
- `MaybeUninit` buffer: performance critical network code'da her byte'ı başlatma lüksü yok. Nova Shield de network buffer'larında benzer yaklaşım kullanmalı.
- bitflags metadata: paket türü/durumu için bitmask kullanımı önbelleğe dost ve atomik.

---

### [quic.rs] (42 satır)

**Amaç:** Solana'nın QUIC üzerinden transaction iletimindeki parametre sabitlerini tanımlar.

**Önemli Sabitler:**
```
QUIC_PORT_OFFSET: u16 = 6
// TPU port + 6 = QUIC port (örn. 8003 + 6 = 8009)

QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS: usize = 128
QUIC_MIN_STAKED_CONCURRENT_STREAMS: usize = 128
QUIC_TOTAL_STAKED_CONCURRENT_STREAMS: usize = 100_000
QUIC_MAX_STAKED_CONCURRENT_STREAMS: usize = 512
// Not: 2048'den 512'ye düşürüldü (receive_window rekabeti sorunu)

QUIC_MAX_TIMEOUT: Duration = 2 saniye
QUIC_KEEP_ALIVE: Duration = 1 saniye
QUIC_CONNECTION_HANDSHAKE_TIMEOUT: Duration = 60 saniye

QUIC_UNSTAKED_RECEIVE_WINDOW_RATIO: u64 = 128
// receive_window = 128 × PACKET_DATA_SIZE = 128 × 1232 = 157,696 bytes

QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO: u64 = 128
QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO: u64 = 512
// max staked receive_window = 512 × 1232 = 630,784 bytes
```

**Trait Listesi:**
- `NotifyKeyUpdate` (satır 40): `fn update_key(&self, key: &Keypair) -> Result<(), Box<dyn Error>>` — TLS sertifikası rotasyonu için

**Stake Tabanlı Akış Kontrolü:**
Staked node'lar daha fazla concurrent stream ve daha büyük receive window hakkına sahip. Bu Sybil korumasıdır: stake olmadan ağ bant genişliğinden orantısız yararlanamazsınız.

**Nova Shield İçin Öğrenilen:**
- Stake bazlı QoS (Quality of Service): yüksek stake = daha fazla akış kapasitesi. Nova Shield da validator stake miktarına göre diferansiyel bant genişliği ayırımı yapmalı.
- `QUIC_PORT_OFFSET = 6` yaklaşımı: aynı host üzerinde birden fazla protokol için offset tabanlı port yönetimi. Nova Shield port planlamasında sabit offsetler kullanmalı.
- `NotifyKeyUpdate` trait: TLS sertifikası süresi dolduğunda anahtar rotasyonu zorunlu. Nova Shield ağ katmanında sertifika yaşam döngüsü yönetimi tasarlanmalı.

---

### [compute_budget.rs] (68 satır)

**Amaç:** Compute budget programını ve instruction türlerini tanımlar.

**Program ID:**
`ComputeBudget111111111111111111111111111111` (32 byte, base58)

**Struct/Enum Listesi:**

- `ComputeBudgetInstruction` (satır 25): 5 varyant:
  - `Unused` — deprecated (eski RequestUnitsDeprecated), rezerve edilmiş slot
  - `RequestHeapFrame(u32)` — 1024'ün katı byte heap boyutu talep et (her CPI çağrısına da uygulanır)
  - `SetComputeUnitLimit(u32)` — transaction genelinde maksimum CU
  - `SetComputeUnitPrice(u64)` — öncelik ücreti, micro-lamports/CU cinsinden
  - `SetLoadedAccountsDataSizeLimit(u32)` — yüklenen hesap veri boyutu limiti

**Kritik Fonksiyonlar:**

- `ComputeBudgetInstruction::request_heap_frame(bytes: u32)` (satır 43): `new_with_borsh(id(), &RequestHeapFrame(bytes), vec![])`
- `ComputeBudgetInstruction::set_compute_unit_limit(units: u32)` (satır 48)
- `ComputeBudgetInstruction::set_compute_unit_price(micro_lamports: u64)` (satır 53)
- `ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(bytes: u32)` (satır 65)
- `ComputeBudgetInstruction::pack(self)` (satır 60): `#[cfg(feature = "dev-context-only-utils")]` — sadece test/bench için

**Serializasyon:** Borsh (BorshSerialize + BorshDeserialize)

**Nova Shield İçin Öğrenilen:**
- `SetComputeUnitPrice` micro-lamports/CU: öncelik ücreti sistemi. Nova Shield da benzer "priority fee" mekanizması eklemeli — bu MEV koruması ve kullanıcı deneyimi için kritik.
- Heap frame talebi per-CPI'a uygulanır: bu recursion limiti ve kaynak yönetimi için önemli. Nova Shield smart contract runtime'ında heap yönetimini tasarlarken dikkate alınmalı.
- `Unused` varyant — deprecated slot ABI uyumluluğu için korunuyor. Enum'lara yeni varyant eklerken eski kodlar `Unused` ile uyumsuz kalmaz.

---

### [ed25519_instruction.rs] (382 satır)

**Amaç:** ED25519 imza doğrulama precompile'ının instruction formatını ve doğrulama mantığını tanımlar.

**Önemli Sabitler:**
```
PUBKEY_SERIALIZED_SIZE: usize = 32      // ED25519 public key
SIGNATURE_SERIALIZED_SIZE: usize = 64   // ED25519 signature
SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14  // Ed25519SignatureOffsets struct boyutu
SIGNATURE_OFFSETS_START: usize = 2      // bytemuck alignment için 2 byte offset
DATA_START: usize = 14 + 2 = 16         // gerçek veri başlangıcı
```

**Struct Listesi:**

- `Ed25519SignatureOffsets` (satır 22): `#[repr(C)]`, bytemuck `Pod + Zeroable`:
  - `signature_offset: u16` — 64 byte imzanın offseti
  - `signature_instruction_index: u16` — imzanın hangi instruction'da olduğu
  - `public_key_offset: u16` — 32 byte public key offseti
  - `public_key_instruction_index: u16` — public key'in hangi instruction'da olduğu
  - `message_data_offset: u16` — mesaj başlangıç offseti
  - `message_data_size: u16` — mesaj boyutu
  - `message_instruction_index: u16` — mesajın hangi instruction'da olduğu

  `u16::MAX` değeri "bu instruction'da" anlamına gelir (self-referential).

**Kritik Fonksiyonlar:**

- `new_ed25519_instruction(keypair, message)` (satır 32): Ed25519 ile mesajı imzalar, instruction data layout'u oluşturur:
  ```
  [0]: num_signatures (1)
  [1]: padding (0)
  [2..16]: Ed25519SignatureOffsets (bytemuck serialize)
  [16..48]: public key (32 byte)
  [48..112]: signature (64 byte)
  [112..]: message
  ```

- `verify(data, instruction_datas, feature_set)` (satır 85): Tüm imzaları doğrular:
  1. `data.len() >= SIGNATURE_OFFSETS_START` kontrol
  2. num_signatures = 0 ama data uzunsa hata
  3. Her imza için: bytemuck ile offsets parse et, veri dilimlerini al, `ed25519_dalek::PublicKey::verify_strict()` çağır
  4. `verify_strict` kullanılır (malleability'ye karşı daha sıkı)

- `get_data_slice()` (satır 154): instruction index `u16::MAX` ise kendi instruction_data'sına, aksi halde başka instruction'a referans

**Tasarım Kararı: Cross-instruction veri referansı**
imza, public key ve mesaj farklı instruction'ların data'sında bulunabilir. Bu esnek ve gas-efficient: tek imzayla birden fazla instruction'ı yetkilendirilebilir.

**Nova Shield İçin Öğrenilen:**
- `u16::MAX` sentineli "current instruction" anlamına geliyor — bu pointer semantiği zarif. Nova Shield precompile tasarımında benzer self-referential offsetler kullanılabilir.
- `bytemuck::Pod` ve `#[repr(C)]`: zero-copy struct parse etme için. Nova Shield transaction parsing'de bytemuck kullanmalı.
- ED25519 için `verify_strict` tercih edilmiş — malleability'yi engeller. BLS gibi diğer imza şemaları için de benzer "strict" mode düşünülmeli.

---

### [derivation_path.rs] (771 satır)

**Amaç:** BIP-44 standart HD cüzdan türetme yollarını Solana bağlamında tanımlar.

**Sabitler:**
```
ACCOUNT_INDEX: usize = 2    // path[2] = account indeksi
CHANGE_INDEX: usize = 3     // path[3] = change indeksi
```

**Struct/Enum Listesi:**

- `DerivationPathError` (satır 29): `InvalidDerivationPath(String)` + `Infallible` — hata türleri
- `DerivationPath` (satır 43): `DerivationPathInner` wrapping struct (newtype pattern)
- `QueryKey` (satır 218): `FullPath | Key` — URI query string anahtarları

**Trait Listesi:**

- `Bip44` (satır 251): `PURPOSE: u32 = 44`, `COIN: u32` — BIP-44 implementasyonu için trait
  - `base_indexes()`: `[Hardened(44), Hardened(COIN)]` döner

**`Solana` struct** (satır 263): `impl Bip44 for Solana { const COIN: u32 = 501; }` — Solana'nın BIP-44 coin numarası

**Kritik Fonksiyonlar:**

- `DerivationPath::new_bip44(account, change)` (satır 106): Solana BIP-44 yolu oluşturur:
  - Account yok → `m/44'/501'`
  - Account=1, Change=None → `m/44'/501'/1'`
  - Account=1, Change=2 → `m/44'/501'/1'/2'`
  - Tüm indexler Hardened (')

- `DerivationPath::from_key_str(path)` (satır 69): Kullanıcı girişi parse eder. `"1/2"` → `m/44'/501'/1'/2'`. Maksimum derinlik 2 (account + change). 3+ segment hata.

- `DerivationPath::from_absolute_path_str(path)` (satır 92): Tam BIP32 yolu parse eder (`m/44'/501'/1'/2'`). Tüm segmentler Hardened olarak muamele görür.

- `DerivationPath::from_uri_key_query(uri)` (satır 146): `?key=1/2` format
- `DerivationPath::from_uri_any_query(uri)` (satır 150): `?key=` veya `?full-path=`
- `DerivationPath::get_query()` (satır 134): URI query string üretir

**Güvenlik Notu:**
Tüm indexler `Hardened` (2^31 + index): `m/44'/501'/account'/change'`. Bu standart Solana HD cüzdan davranışı. Hardened türetme, public key'den child key türetmeyi imkansız kılar.

**Test Kapsamı:**
- `test_from_key_str`: çeşitli format kombinasyonları (1/2, 1'/2', 1\'/2\')
- `test_from_absolute_path_str`: tam yol, diğer coin tipleri (999), non-bip44 yollar
- `test_from_uri` + `test_from_uri_full_path`: 20+ URI test durumu
- `test_get_query`: query string üretimi
- `test_derivation_path_debug`: Debug formatı

**Nova Shield İçin Öğrenilen:**
- BIP-44 coin numarası 501 Solana'ya özgüdür. Nova Shield için SLIP-0044'e kayıt yapılmalı veya development sırasında özel bir numara kullanılmalı.
- `from_key_str` sadece 2 derinliğe izin veriyor — bu güvenlik kararı, kullanıcıların yanlışlıkla derin yollar kullanarak key'leri karıştırmasını engeller.
- URI'den türetme yolu parse etme: signer URL'leri için standart (örn. `usb://ledger?key=0/0`). Nova Shield hardware wallet entegrasyonunda bu format desteklenmeli.

---

### [precompiles.rs] (127 satır)

**Amaç:** Solana'nın tüm precompile programlarını kaydeden ve doğrulama dispatch'ini yöneten merkezi modül.

**Struct/Enum Listesi:**

- `PrecompileError` (satır 16): 5 variant:
  - `InvalidPublicKey` — geçersiz public key
  - `InvalidRecoveryId` — geçersiz recovery ID
  - `InvalidSignature` — geçersiz imza
  - `InvalidDataOffsets` — geçersiz offset
  - `InvalidInstructionDataSize` — hatalı boyut

- `Precompile` (satır 38): `program_id: Pubkey`, `feature: Option<Pubkey>`, `verify_fn: Verify`

**Tip Tanımları:**
- `Verify = fn(&[u8], &[&[u8]], &FeatureSet) -> Result<(), PrecompileError>` (satır 35) — function pointer tipi

**Lazy Static:**
```rust
static ref PRECOMPILES: Vec<Precompile> = vec![
    Precompile::new(secp256k1_program::id(), None, secp256k1_instruction::verify),
    Precompile::new(ed25519_program::id(), None, ed25519_instruction::verify),
];
```
Her ikisi de `feature: None` → her zaman aktif (genesis'ten itibaren).

**Kritik Fonksiyonlar:**

- `Precompile::check_id()` (satır 56): program_id eşleşmesi + feature gate kontrolü
- `Precompile::verify()` (satır 66): `(self.verify_fn)(data, instruction_datas, feature_set)` — function pointer dispatch
- `is_precompile()` (satır 93): program ID'nin precompile olup olmadığını kontrol eder
- `get_precompiles()` (satır 102): tüm precompile'ların static referansını döner
- `verify_if_precompile()` (satır 107): program_id precompile ise doğrulama yapar; değilse `Ok(())` döner

**Nova Shield İçin Öğrenilen:**
- `fn(&[u8], &[&[u8]], &FeatureSet) -> Result<(), PrecompileError>` function pointer: precompile'lar dispatch table üzerinden çalışır. Nova Shield'a yeni precompile eklemek için bu Vec'e girdi eklemek yeterli.
- `feature: Option<Pubkey>` mekanizması: precompile feature gate arkasında olabilir — mevcut iki precompile hep aktif ama gelecekte ZK precompile'ları feature gate'li olabilir.
- `verify_if_precompile` imzası not: tüm instruction'ların data'sını alıyor (`&[CompiledInstruction]`) çünkü imza, mesaj ve pubkey farklı instruction'larda olabilir.

---

### [secp256k1_instruction.rs] (1318 satır)

**Amaç:** Ethereum uyumlu secp256k1 ECDSA imza doğrulama precompile'ı. Solana'nın en kapsamlı precompile dokümantasyonu.

**Önemli Sabitler:**
```
HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20   // Ethereum address (Keccak256 hash'in son 20 byte'ı)
SIGNATURE_SERIALIZED_SIZE: usize = 64        // secp256k1 signature
SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11  // SecpSignatureOffsets binary boyutu
DATA_START: usize = 11 + 1 = 12              // offsets verisi başlangıcı
```

**Struct Listesi:**

- `SecpSignatureOffsets` (satır 813): 11 byte binary layout (little-endian):
  - `signature_offset: u16` — 64+1 byte imza+recovery_id offseti
  - `signature_instruction_index: u8`
  - `eth_address_offset: u16` — 20 byte Ethereum adresi
  - `eth_address_instruction_index: u8`
  - `message_data_offset: u16`
  - `message_data_size: u16`
  - `message_instruction_index: u8`

**Kritik Fonksiyonlar:**

- `new_secp256k1_instruction(priv_key, message_arr)` (satır 846):
  1. `PublicKey::from_secret_key` → secp256k1 public key
  2. `construct_eth_pubkey(&secp_pubkey)` → 20 byte Ethereum adresi
  3. `Keccak256::new().update(message_arr).finalize()` → mesaj hash
  4. `libsecp256k1::sign(&message, priv_key)` → (signature, recovery_id)
  5. Instruction data layout:
     ```
     [0]: num_signatures = 1
     [1..12]: SecpSignatureOffsets (bincode serialize)
     [12..32]: eth_address (20 bytes)
     [32..96]: signature (64 bytes)
     [96]: recovery_id (1 byte)
     [97..]: message
     ```

- `construct_eth_pubkey(pubkey)` (satır 907):
  ```
  eth_addr = Keccak256(pubkey.serialize()[1..])[12..]
  // secp256k1 pubkey'nin uncompressed formatının (65 byte) prefix byte'ı atılır (0x04)
  // kalan 64 byte Keccak256 alınır, son 20 byte adres olur
  ```

- `verify(data, instruction_datas, feature_set)` (satır 929):
  1. `data[0]` = signature count
  2. `libsecp256k1_fail_on_bad_count` feature gate: count=0 ama data uzunsa hata
  3. Her imza için:
     - `libsecp256k1::Signature::parse_standard_slice()` — imzayı parse et
     - `libsecp256k1::RecoveryId::parse()` — recovery ID
     - Keccak256(message) → message hash
     - `libsecp256k1::recover()` → public key geri al
     - `construct_eth_pubkey(pubkey)` → Ethereum adresi hesapla
     - `eth_address_slice == eth_address` → doğrula

**Kritik Güvenlik Notu: Signature Malleability**
Secp256k1 ECDSA doğası gereği signature malleability'ye açıktır: verilen bir geçerli imzadan başka bir geçerli imza üretilebilir (S değerini `n - S` ile değiştirerek). Solana bu kontrolü yapmaz. Programlar `libsecp256k1::Signature::parse_standard_slice()` ile high-S değerlerini reddedebilir.

**Ethereum Adresi Uyumluluğu:**
`construct_eth_pubkey` tam Ethereum adresi formatı üretir. Bu, Ethereum işlemlerini Solana üzerinde doğrulamak için kullanılabilir (EIP-712, ECDSA imzaları).

**ED25519 vs secp256k1 Farkları:**
- ED25519: `u16::MAX` sentinel, `public_key_instruction_index` 16-bit
- secp256k1: `u8` instruction index (256 instruction limiti), `signature_instruction_index` 8-bit
- ED25519: 14-byte SecpOffsets struct; secp256k1: 11-byte struct
- ED25519: mesaj hash'lenmez (doğrudan verify); secp256k1: Keccak256(message)

**Nova Shield İçin Öğrenilen:**
- Ethereum interoperability için secp256k1 precompile şablonu kullanılabilir — ETH imzalı mesajları Solana/Nova Shield üzerinde doğrulamak için tam referans.
- Malleability uyarısı kritik: eğer Nova Shield secp256k1 kullanıyorsa ve imzayı transaction ID olarak kullanıyorsa malleability saldırısına açık olabilir. `parse_standard_slice` ile low-S zorunlu kılınmalı.
- Cross-instruction referans sistemi (imza başka instruction'da olabilir): MEV koruma açısından ilginç — imzayı ayrı instruction'da tutmak visibility'yi artırır.

---

### [rent_collector.rs] (477 satır)

**Amaç:** Hesaplardan kira toplar, kira muafiyeti ve kira borcu hesaplar.

**Struct/Enum Listesi:**

- `RentCollector` (satır 15): `epoch: Epoch`, `epoch_schedule: EpochSchedule`, `slots_per_year: f64`, `rent: Rent`
- `RentResult` (satır 41): dahili enum:
  - `Exempt` — hiç kira toplanmaz
  - `NoRentCollectionNow` — şu an değil (gelecekte)
  - `CollectRent { new_rent_epoch: Epoch, rent_due: u64 }` — kira topla
- `CollectedInfo` (satır 182): `rent_amount: u64` + `account_data_len_reclaimed: u64`

**Önemli Sabitler:**
- `RENT_EXEMPT_RENT_EPOCH: Epoch = Epoch::MAX` (satır 37) — kira muaf hesapların işaret değeri

**Kritik Fonksiyonlar:**

- `RentCollector::should_collect_rent(address, account)` (satır 76):
  - Executable hesaplar → muaf (çünkü muaf balance gerekir)
  - `incinerator::id()` → muaf (özel yakma adresi)
  - Aksi halde → kira toplanabilir

- `RentCollector::get_rent_due(account)` (satır 83):
  ```
  if is_exempt(lamports, data_len):
      return RentDue::Exempt
  slots_elapsed = sum(slots_in_epoch for epoch in rent_epoch..=current_epoch)
  years_elapsed = slots_elapsed / slots_per_year
  due = rent.due_amount(data_len, years_elapsed)
  return RentDue::Paying(due)
  ```

- `RentCollector::collect_from_existing_account()` (satır 115): `#[must_use]` — sonuç bank'e eklenmeli
  - Exempt → `set_rent_epoch(RENT_EXEMPT_RENT_EPOCH)`, sıfır döner
  - NoRentCollectionNow → sıfır döner
  - CollectRent → lamports düş; lamports sıfıra gelirse hesabı sil (`mem::take`), veri boyutunu raporla

- `RentCollector::calculate_rent_result()` (satır 151): dahili mantık:
  - `rent_epoch == Epoch::MAX` veya `rent_epoch > current_epoch` → `NoRentCollectionNow`
  - `should_collect_rent` false → `Exempt`
  - `get_rent_due` Exempt → `Exempt`
  - `get_rent_due` Paying(0) → `NoRentCollectionNow`
  - `get_rent_due` Paying(due) → `CollectRent { new_rent_epoch: current+1, rent_due: due }`

**`CollectedInfo` Operatörler:**
- `Add` ve `AddAssign` implement edilmiş: kira toplamları toplanabilir. `#![allow(clippy::arithmetic_side_effects)]`

**Thread Safety Uyarısı:**
Satır 112 yorumu: "NOT thread safe at some level. Paralelde aynı hesaptan toplanırsa iki kez toplanabilir."

**Nova Shield İçin Öğrenilen:**
- `RENT_EXEMPT_RENT_EPOCH = Epoch::MAX` sentinel değeri: gelecekte bu alanı tamamen kaldırmak için zemin hazırlıyor — tüm hesaplar muaf olduğunda alan anlamsız.
- `mem::take(account)` ile sıfır lamportlu hesabı silme: Rust'ın bellek sahipliği sistemi account temizleme için kullanılıyor, allocation gerektirmez.
- Kira hesabı `years_elapsed` ile: yıllık rent_rate × veri_boyutu × geçen_yıl. Nova Shield'da depolama maliyeti benzer time-proportional model izlemeli.
- `#[must_use]` işareti: ücret toplayan fonksiyonlarda dönüş değeri yoksayılmamalı — bu kritik bir Rust güvenlik özelliği.

---

### [rent_debits.rs] (64 satır)

**Amaç:** Epoch sonunda kira toplama sonuçlarını `RewardInfo` formatına dönüştürmek için yardımcı veri yapısı.

**Struct Listesi:**

- `RentDebit` (satır 7): `rent_collected: u64` + `post_balance: u64`
- `RentDebits` (satır 27): `HashMap<Pubkey, RentDebit>` wrapper

**Kritik Fonksiyonlar:**

- `RentDebit::try_into_reward_info()` (satır 13): `rent_collected` i64'e çevirir, negatif yapar → `RewardInfo { reward_type: Rent, lamports: -rent, post_balance }`. Taşma durumunda `None`.
- `RentDebits::insert()` (satır 47): sadece `rent_collected > 0` ise ekler
- `RentDebits::get_account_rent_debit()` (satır 29): adres için toplanan kira miktarı
- `RentDebits::into_unordered_rewards_iter()` (satır 59): `RewardInfo` iterator'ına dönüştürür

**`dev-context-only-utils` Feature Gate:**
`len()` ve `is_empty()` sadece dev/test context'te kullanılabilir — production kod bu detayları gizler.

**Nova Shield İçin Öğrenilen:**
- Negatif ödül (kira borcu) `i64` ile temsil: aynı `RewardInfo` struct hem pozitif (staking) hem negatif (kira) ödülleri ifade edebilir.
- `filter_map` + `try_into_reward_info`: başarısız dönüşümler sessizce atlanır. Nova Shield ödül sistemi taşma durumlarını daha açık loglamalı.

---

### [reserved_account_keys.rs] (256 satır)

**Amaç:** Transaction'larda write-lock edilemeyen korumalı hesap adreslerini yönetir.

**Struct Listesi:**

- `ReservedAccountKeys` (satır 29): `active: HashSet<Pubkey>` + `inactive: HashMap<Pubkey, Pubkey>`
- `ReservedAccount` (satır 113): `key: Pubkey` + `feature_id: Option<Pubkey>`

**Lazy Static:**
```rust
static ref RESERVED_ACCOUNTS: Vec<ReservedAccount> = [
    // Aktif (daima korumalı):
    ReservedAccount::new_active(bpf_loader::id()),
    ReservedAccount::new_active(bpf_loader_upgradeable::id()),
    ReservedAccount::new_active(config::program::id()),
    ReservedAccount::new_active(feature::id()),
    ReservedAccount::new_active(stake::config::id()),
    ReservedAccount::new_active(stake::program::id()),
    ReservedAccount::new_active(system_program::id()),
    ReservedAccount::new_active(vote::program::id()),
    // Sysvars (aktif):
    ReservedAccount::new_active(sysvar::clock::id()),
    ReservedAccount::new_active(sysvar::epoch_schedule::id()),
    ReservedAccount::new_active(sysvar::fees::id()),
    ReservedAccount::new_active(sysvar::instructions::id()),
    ReservedAccount::new_active(sysvar::rent::id()),
    ReservedAccount::new_active(sysvar::rewards::id()),
    ReservedAccount::new_active(sysvar::slot_hashes::id()),
    ReservedAccount::new_active(sysvar::slot_history::id()),
    ReservedAccount::new_active(sysvar::stake_history::id()),
    ReservedAccount::new_active(native_loader::id()),
    // Feature gate'li (pending):
    ReservedAccount::new_pending(address_lookup_table::program::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(compute_budget::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(ed25519_program::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(loader_v4::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(secp256k1_program::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(zk_token_proof_program::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(sysvar::epoch_rewards::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(sysvar::last_restart_slot::id(), add_new_reserved_account_keys::id()),
    ReservedAccount::new_pending(sysvar::id(), add_new_reserved_account_keys::id()),
].to_vec()
```

**Kritik Fonksiyonlar:**

- `ReservedAccountKeys::new()` (satır 49): feature_id None ise active set'e, varsa inactive map'e
- `ReservedAccountKeys::new_all_activated()` (satır 68): off-chain araçlar için — tüm keyleri aktif say
- `ReservedAccountKeys::is_reserved()` (satır 76): sadece `active.contains(key)` — inactive'ler korunmaz
- `ReservedAccountKeys::update_active_set()` (satır 82): inactive'den aktive edilenleri taşır, `retain` ile efficient güncelleme
- `ReservedAccountKeys::all_keys_iter()` (satır 96): off-chain filtreleme için tüm keyler
- `ReservedAccountKeys::empty_key_set()` (satır 104): test için boş set

**Kısıtlama:**
Yorum satırı 136: "Reserved account'lar bu listeden çıkarılamaz — konsensüs bozulur." Listeden silmek hard fork gerektirir.

**Nova Shield İçin Öğrenilen:**
- Write-lock koruması: protokol programları yanlışlıkla veya kasıtlı olarak kullanıcı transaction'larıyla değiştirilemez.
- Feature gate'li reserved keys: yeni built-in programlar önce güvensiz/unprotected olabilir, feature aktivasyonuyla güvenceye alınır.
- `new_all_activated()` ayrımı: runtime'da feature gate kontrol edilirken off-chain araçlar tüm keyleri korumalı sayabilir.

---

### [commitment_config.rs] (243 satır)

**Amaç:** Solana'nın query commitment seviyelerini tanımlar.

**Struct/Enum Listesi:**

- `CommitmentConfig` (satır 10): `commitment: CommitmentLevel` wrapper
- `CommitmentLevel` (satır 140): 8 varyant:
  - **Deprecated (v1.5.5'ten itibaren):** `Max`, `Recent`, `Root`, `Single`, `SingleGossip`
  - **Aktif:** `Processed`, `Confirmed`, `Finalized`

**CommitmentLevel Semantiği:**
- `Processed`: Validator tarafından işlenmiş, en hızlı, en az güvenli. Rollback mümkün.
- `Confirmed`: Supermajority tarafından oylanmış (gossip + replay), optimistic confirmation. Rollback çok nadir.
- `Finalized`: Max vote lockout'a ulaşmış, supermajority tarafından tanınmış. Matematiksel olarak final.

**Kritik Fonksiyonlar:**

- `CommitmentConfig::is_finalized()` (satır 91): `Finalized | Max | Root` → true
- `CommitmentConfig::is_confirmed()` (satır 98): `Confirmed | SingleGossip | Single` → true
- `CommitmentConfig::is_processed()` (satır 105): `Processed | Recent` → true
- `CommitmentConfig::is_at_least_confirmed()` (satır 112): confirmed veya daha güçlü
- `CommitmentConfig::use_deprecated_commitment()` (satır 116): deprecated → aktif mapping
- `CommitmentConfig::ok()` (satır 83): Default ise `None`, aksi `Some(self)`

**Default:** `CommitmentLevel::Finalized` — en güvenli seviye varsayılan

**ParseCommitmentLevelError:** `Invalid` varyantı

**Nova Shield İçin Öğrenilen:**
- Üç aktif seviye (Processed/Confirmed/Finalized) yeterli abstraction katmanı sağlıyor. Nova Shield RPC'sinde aynı üçlü model uygulanmalı.
- Deprecated varyantların korunması: eski client'ların bozulmaması için geriye uyumluluk kritik. Nova Shield v1'den v2'ye geçişte aynı yaklaşım izlenmeli.
- `is_at_least_confirmed()` helper: "en az confirmed" kontrolü çok yaygın kullanım — bu tür composite helper'lar API kullanımını kolaylaştırır.

---

### [offchain_message.rs] (302 satır)

**Amaç:** Off-chain mesajları (cüzdan imzalama, hardware wallet mesajı) için versiyonlu format tanımlar.

**Önemli Sabitler:**
```
OffchainMessage::SIGNING_DOMAIN: &[u8] = b"\xffsolana offchain"  // 16 byte
OffchainMessage::HEADER_LEN: usize = 16 + 1 = 17               // domain + version byte

v0::OffchainMessage::HEADER_LEN: usize = 3   // format(1) + length(2)
v0::OffchainMessage::MAX_LEN: usize = 65535 - 17 - 3 = 65515  // u16::MAX - headers
v0::OffchainMessage::MAX_LEN_LEDGER: usize = 1232 - 17 - 3 = 1212  // PACKET_DATA_SIZE uyumlu
```

**Struct/Enum Listesi:**

- `MessageFormat` (satır 39): `RestrictedAscii | LimitedUtf8 | ExtendedUtf8` — mesaj karakter seti
- `v0::OffchainMessage` (satır 59): `format: MessageFormat` + `message: Vec<u8>`
- `OffchainMessage` (satır 167): `V0(v0::OffchainMessage)` — versiyonlu enum

**Mesaj Sınıflandırması:**
```
len <= 1212 bytes AND printable ASCII → RestrictedAscii (Ledger donanım cüzdanı destekler)
len <= 1212 bytes AND UTF8            → LimitedUtf8     (Ledger Solana app v1.6+)
len <= 65515 bytes AND UTF8           → ExtendedUtf8    (software cüzdanlar)
```

**Signing Domain:**
`\xff` prefix: transaction veri'sinin başlamasını önler (transaction başlangıcı `\xff` değildir). `solana offchain` label ile transaction replay attack önlenir.

**Kritik Fonksiyonlar:**

- `v0::OffchainMessage::new(message)` (satır 73): Otomatik format algılama (ASCII mı, UTF8 mı, çok uzun mu?)
- `v0::OffchainMessage::serialize()` (satır 100): format(1) + len(2) + message
- `v0::OffchainMessage::deserialize()` (satır 114): format kontrolü + boyut kontrolü
- `v0::OffchainMessage::hash(serialized)` (satır 150): SHA256 hash
- `OffchainMessage::serialize()` (satır 185): `SIGNING_DOMAIN + version(1) + v0_serialize()`
- `OffchainMessage::sign()` (satır 238): `signer.sign_message(&self.serialize()?)`
- `OffchainMessage::verify()` (satır 243): `signature.verify(signer, &self.serialize()?)`

**Test Fonksiyonları:**
- `test_offchain_message_ascii`: hash = `HG5JydBGjtjTfD3sSn21ys5NTWPpXzmqifiGC2BVUjkD`
- `test_offchain_message_utf8`: Rusça metin testi
- `test_offchain_message_sign_and_verify`: Keypair ile sign+verify döngüsü

**Nova Shield İçin Öğrenilen:**
- `\xff` domain separator: transaction replay attack'a karşı off-chain mesajları transaction'lardan ayıran önemli bir teknik. Nova Shield off-chain imzalama sisteminde bu ayrım şart.
- Donanım cüzdan kısıtlaması (1212 byte, ASCII/UTF8): cüzdan ekranı kısıtlı olduğundan içerik kontrolü gerekli. Nova Shield hardware wallet desteği tasarlarken mesaj format kısıtlamalarını cüzdan tipine göre uygulamalı.

---

### [reward_info.rs] (12 satır)

**Amaç:** Bir hesabın aldığı tek bir ödülü temsil eder.

**Struct Listesi:**

- `RewardInfo` (satır 4):
  - `reward_type: RewardType`
  - `lamports: i64` — pozitif=ödül, negatif=ceza
  - `post_balance: u64` — ödül sonrası bakiye
  - `commission: Option<u8>` — sadece vote/staking ödülleri için komisyon (0-100)

**Nova Shield İçin Öğrenilen:**
- `i64` kullanımı: hem ödül hem ceza tek struct ile temsil edilebilir. Slashing mekanizması için negatif `lamports` değeri yeterli.

---

### [inner_instruction.rs] (21 satır)

**Amaç:** CPI (Cross-Program Invocation) iç instruction'larının kaydını tutar.

**Tip Tanımları:**

- `InnerInstruction` (satır 8): `instruction: CompiledInstruction` + `stack_height: u8`
  - `stack_height` 1'den başlar (transaction-level instruction), CPI ile artar
- `InnerInstructions = Vec<InnerInstruction>` — tek instruction'ın iç çağrıları
- `InnerInstructionsList = Vec<InnerInstructions>` — tüm transaction instruction'larının iç çağrıları

**Nova Shield İçin Öğrenilen:**
- `stack_height` sayesinde CPI derinliği izlenebilir. Nova Shield debug/tracing için CPI çağrı ağacını reconstruct etmek bu bilgiyle mümkün.

---

### [reward_type.rs] (26 satır)

**Amaç:** Ödül türlerini tanımlar.

**Enum Listesi:**

- `RewardType` (satır 6): `Fee | Rent | Staking | Voting`

**Display:** `fee | rent | staking | voting`

**Nova Shield İçin Öğrenilen:**
- 4 ödül türü: Nova Shield da aynı kategoriler kullanabilir. `Fee` burn edilebilir, `Rent` toplandıktan sonra ne olacağı (burn mu, validator'a mı?) protokol kararı.

---

### [shred_version.rs] (69 satır)

**Amaç:** Shred versiyonunu genesis hash ve hard fork bilgisinden hesaplar.

**Kritik Fonksiyonlar:**

- `version_from_hash(hash: &Hash) -> u16` (satır 12):
  ```
  32 byte'ı 2'şer byte chunklara böl
  XOR ile 2 byte'a indir → 16-bit akümülatör
  version = ((accum[0] as u16) << 8) | accum[1]
  version = version.saturating_add(1)  // asla 0 olmasın
  ```

- `compute_shred_version(genesis_hash, hard_forks)` (satır 30):
  ```
  hash = genesis_hash
  for (slot, count) in hard_forks:
      buf = LittleEndian(slot)[0..8] + LittleEndian(count)[8..16]
      hash = extend_and_hash(&hash, &buf)
  version = version_from_hash(&hash)
  ```

**Test:**
- `Hash::default()`, no hard forks → version = 1
- Hard fork slot=1 → version = 55551
- Hard fork slot=1 (2. kez) → version = 46353

**Nova Shield İçin Öğrenilen:**
- Shred versiyonu ağ kimliğidir: farklı ağlar (farklı genesis veya hard fork) farklı versiyon numarasına sahip → yanlış node'lara shred gönderilmez.
- `saturating_add(1)` → versiyon asla 0 olamaz (sıfır = başlatılmamış anlamına gelir).

---

### [poh_config.rs] (39 satır)

**Amaç:** Proof of History konfigürasyonu.

**Struct Listesi:**

- `PohConfig` (satır 9):
  - `target_tick_duration: Duration` — hedef tick süresi
  - `target_tick_count: Option<u64>` — sadece test için; toplam tick sayısı hedefi
  - `hashes_per_tick: Option<u64>` — `None` = "Low power mode" (hashing yerine sleep)

**Kritik Fonksiyonlar:**

- `PohConfig::new_sleep(duration)` (satır 23): `hashes_per_tick: None` → validator uyur, hash yapmaz
- `Default::default()` (satır 33): `new_sleep(Duration::from_micros(1_000_000 / DEFAULT_TICKS_PER_SECOND))`

**Low Power Mode:** `hashes_per_tick: None` validator'ın tick başına hash yapmak yerine `target_tick_duration` süre uyumasını sağlar. Test ortamları için uygun.

**Nova Shield İçin Öğrenilen:**
- PoH konfigürasyonu genesis config'in parçası, sonradan değiştirilemez. Nova Shield PoH parametrelerini genesis'te dikkatlice ayarlamalı.
- `target_tick_count` sadece test için: sonsuz PoH döngüsünü belirli bir süre sonra durdurmak için kullanılır.

---

### [timing.rs] (226 satır)

**Amaç:** Süre dönüşümleri, zaman hesaplama ve atomik interval yönetimi.

**Önemli Sabitler:**
```
SECONDS_PER_YEAR: f64 = 365.242_199 × 24 × 60 × 60 = 31_556_926.08  (tropical year)
```

**Struct Listesi:**

- `AtomicInterval` (satır 69): `last_update: AtomicU64` — thread-safe periyodik güncelleme denetçisi

**Kritik Fonksiyonlar:**

- `duration_as_ns(d)` (satır 10): nanosaniye, `saturating_mul` + `saturating_add`
- `duration_as_us(d)` (satır 16): mikrosaniye, `unchecked_div_by_const!(subsec_nanos, 1000)` — const bölme
- `duration_as_ms(d)` (satır 22): milisaniye
- `duration_as_s(d)` (satır 31): `f32` döner — hassasiyeti düşük
- `timestamp()` (satır 36): UNIX epoch'tan ms cinsinden anlık zaman
- `years_as_slots(years, tick_duration, ticks_per_slot)` (satır 46):
  ```
  slots = years × SECONDS_PER_YEAR × (10^9 / tick_duration_ns) / ticks_per_slot
  ```
- `slot_duration_from_slots_per_year(slots_per_year)` (satır 58): ters dönüşüm

**`AtomicInterval` Detayı:**
- `should_update(interval_ms)` (satır 75): Lock-free periyodik kontrol. `compare_exchange` ile race condition korumalı.
- `should_update_ext(interval_ms, skip_first)` (satır 82): `skip_first=false` → struct oluşturulduğunda ilk kez true verir
- `elapsed_ms()` (satır 94): Son güncellemeden bu yana geçen süre
- `remaining_until_next_interval()` (satır 101): Saturating subtraction ile negatif olmayan kalan

**Nova Shield İçin Öğrenilen:**
- `AtomicInterval` multi-threaded metrik raporlama için ideal. Nova Shield'da monitoring, gossip, heartbeat için bu pattern kullanılmalı.
- `unchecked_div_by_const!` macro: sabit bölen ile integer division'da compiler tarafından optimize edilebilir — bu kritik path'lerde önemli.
- `SECONDS_PER_YEAR` tropical year kullanılmış (365.242199 gün) — bu enflasyon hesaplamalarında yıl tutarlılığı için önemli.

---

### [nonce_account.rs] (133 satır)

**Amaç:** Durable nonce hesap yönetimi yardımcı fonksiyonları.

**Kritik Fonksiyonlar:**

- `create_account(lamports)` (satır 16): Nonce hesabı oluşturur:
  - `AccountSharedData::new_data_with_space(lamports, &Versions::new(State::Uninitialized), State::size(), &system_program::id())`
  - `RefCell<AccountSharedData>` döner (test ortamında mutability için)

- `verify_nonce_account(account, recent_blockhash)` (satır 30): Nonce hesabını doğrular:
  1. `account.owner() == system_program::id()` kontrol
  2. `StateMut::<Versions>::state(account)` ile state deserialize
  3. `verify_recent_blockhash(recent_blockhash)` — `Versions::Current` gerektirir, `Legacy` kabul etmez
  4. Eşleşme durumunda `Some(Data)` döner

- `lamports_per_signature_of(account)` (satır 44): Nonce hesabındaki fee calculator'ı okur:
  - `State::Initialized(data)` → `Some(data.fee_calculator.lamports_per_signature)`
  - `State::Uninitialized` → `None`

**`Versions::Legacy` vs `Versions::Current` Ayrımı:**
- Legacy nonce: eski format, artık `verify_recent_blockhash` kabul etmez
- Current nonce: `separate_nonce_from_blockhash` feature gate sonrası, hash domain ayrılmış

**Test:**
- `test_verify_bad_account_owner_fails`: system_program dışı sahip → None
- `test_verify_nonce_account`: Legacy/Current/Initialized kombinations

**Nova Shield İçin Öğrenilen:**
- Durable nonce, blockhash'in süresi dolduğunda transaction yeniden kullanımını engeller — offline/sıra tabanlı transaction için kritik. Nova Shield bu mekanizmayı desteklemeli.
- `Versions` enum versiyonlanmış state machine: state format değişimlerinde eski versiyonların soft rejectio'ı için iyi örnek.

---

## Genel Özet ve Cross-Cutting Bulgular

### Mimari Örüntüler

1. **Feature Gate Tabanlı Geçiş:** Her önemli protokol değişikliği bir `Pubkey` ID'si ile temsil edilir ve on-chain feature hesabı aktive edilince devreye girer. Bu yaklaşım hard fork gerektirmez.

2. **`#[repr(C)]` + bytemuck:** Network-critical struct'lar (`Packet`, `Ed25519SignatureOffsets`) `#[repr(C)]` ve `Pod + Zeroable` ile zero-copy parse için tasarlanmış.

3. **Saturating Arithmetic Everywhere:** Tüm sayısal işlemlerde `saturating_add`, `saturating_mul`, `saturating_sub` — integer overflow'u panic yerine saturation'a dönüştürür. Bu ağ kodunda kritik.

4. **`#[must_use]` Critical Functions:** Ücret, kira gibi kritik değerleri döndüren fonksiyonlar `#[must_use]` ile işaretlenmiş — derleyici seviyesinde güvenlik.

5. **`lazy_static!` Registry Pattern:** `PRECOMPILES`, `FEATURE_NAMES`, `RESERVED_ACCOUNTS` gibi global kayıtlar `lazy_static!` ile thread-safe initialize edilir.

6. **ABI Stability Annotations:** `#[frozen_abi]` ile struct binary layout'u dondurulur. `__unused` ve `__backwards_compat` alanları ABI koruma mekanizması.

### Nova Shield İçin Stratejik Kararlar

| Konu | Solana Yaklaşımı | Nova Shield Önerisi |
|------|-----------------|---------------------|
| Feature rollout | On-chain Pubkey feature gate | Aynı mekanizma, ek governance layer |
| Ücret hesabı | Integer + feature gate | Tamamen integer, no float rounding |
| Enflasyon | Exponential decay (f64) | Fixed-point arithmetic için araştır |
| MTU | 1232 bytes (IPv6 minimum) | 1232 baseline, jumbo frame opsiyonel |
| Precompile | Function pointer dispatch | Aynı — ekleme için Vec'e entry |
| Commitment | 3 seviye (Processed/Confirmed/Finalized) | Aynı 3-seviye model |
| BIP-44 | Coin 501, max 2 derinlik | Nova Shield coin numarası belirle |
| Kira | time-proportional, 32KB sayfalı | Depolama ücret modeli benzer tutulmalı |
| Off-chain mesaj | `\xff` domain separator | Transaction replay koruması şart |
| Hard fork | Bank hash'e fork veri karıştır | Aynı mekanizma |

### Kritik Güvenlik Bulguları

1. **secp256k1 signature malleability:** Solana runtime'da düzeltilmemiş. Programlar kendi kendine `parse_standard_slice` ile low-S enforce etmeli. Nova Shield bu özelliği runtime'da enforce etmeli.

2. **`MaybeUninit` buffer:** `Packet::default()` başlatılmamış buffer içerir; `meta.size` sınırları aşan okuma tanımsız davranış. Nova Shield ağ katmanında benzer yaklaşım dikkatli kullanılmalı.

3. **Rent collection thread-safety:** `collect_from_existing_account` aynı hesap üzerinde paralel çalıştırılırsa çift kira alınabilir. Nova Shield hesap lock'ları bu durumu engeller.

4. **`ClusterType::get_genesis_hash()` hardcoded hash'ler:** Mainnet genesis hash kodda sabit. Bu değer değişirse tüm node'lar güncellenmeli — Nova Shield genesis hash'ini configurable yapmalı.

---

*Agent 24 — Analiz tamamlandı. Toplam ~5.500 satır, 23 dosya analiz edildi.*
