# Polkadot/Substrate → Nova Shield Chain Araştırması
# Kaynak: research/analysis/polkadot/ (30 analiz dosyası + 3 doğrulama)
# Tarih: 2026-07-22

---

## A. TOKEN EKONOMİSİ (DOT → NSC)

### A.1 DOT Token Utility

DOT dört temel işlev üstlenir:
1. **Governance** — referendum oylaması, proposal deposit
2. **Staking** — NPoS validator/nominator sistemi
3. **Parachain auctions** — parachain kirasını teminat altına alma (NSC için geçersiz)
4. **Fee payment** — transaction ücretleri

NSC için tüm dört işlev geçerli: governance (conviction voting), staking (validator/delegator), cross-chain vault teminatı, fee payment.

### A.2 NPoS Enflasyon Modeli

**Polkadot parametreleri (kaynak: agent-25, impls.rs:95-128):**
```
ideal_stake       = 75%     (Polkadot)
min_annual_inflation = 2.5%
max_annual_inflation = 10%
falloff           = 5%
```

**Nova Shield parametreleri (mevcut kod):**
```
ideal_stake       = 50%
ideal_APY         = 5%
Emisyon formülü   = max(800 - year*50, 200) bps
```

**Polkadot'un tam enflasyon eğrisi hesabı (relay_era_payout, impls.rs:95-128):**

```rust
// Adım 1: Mevcut staking oranı
let stake = total_staked / total_stakable;

// Adım 2: NPoS eğrisi (sp-npos-elections compute_inflation)
// stake < ideal_stake ise:
//   adjustment = stake / ideal_stake   (doğrusal büyüme)
// stake >= ideal_stake ise:
//   adjustment = 2^((ideal_stake - stake) / falloff)  (üstel düşüş)

// Adım 3: Staking enflasyonu
let staking_inflation = min_inflation + (max_inflation - min_inflation) * adjustment;

// Adım 4: Era ödülü
let staking_payout = period_fraction * staking_inflation * total_stakable;

// Adım 5: Kalan → Treasury
let treasury_payout = period_fraction * max_inflation * total_stakable - staking_payout;
```

**NSC için önerilen eğri:**
- `ideal_stake = 50%` (mevcut kod uygun)
- `min_inflation = 2%` (yıl 12+ emisyon oranına yakın)
- `max_inflation = 8%` (yıl 0 başlangıç)
- `falloff = 5%` (Polkadot değeri)
- Eğri tipi: stake < ideal → doğrusal artış; stake > ideal → üstel düşüş
- NSC mevcut kodu `rate_bps = max(800 - year*50, 200)` düz azalma; Polkadot'unkine yaklaştırılabilir

**Rust implementasyon notu:**
```rust
// sp-arithmetic ile hassas hesaplama
use sp_arithmetic::{Perbill, Perquintill, FixedU128};
// ideal_stake, falloff → Perquintill kullan (10^18 hassasiyet)
// adjustment → FixedU128
// per_block ödül → saturating_mul ile overflow koruması
```

### A.3 Era Ödül Hesabı

**Era reward points (pallet-staking, impls.rs:1734-1735):**
```
note_author çağrıldığında: validator → +20 puan
note_uncle çağrıldığında: +2 puan
```

**Validator/Nominatör payı (inflation.rs, REWARD_F1):**
```
validator_commission = kendi belirlediği oran (örn. %10)
validator_net = REWARD_F1 * era_reward + commission * nominatör_payı
nominatör_net = (1 - commission) * (era_reward * nominatör_stake / toplam_stake)
```

`REWARD_F1 = 50%` (slashing.rs:72) — sadece validator'a ayrılan minimum taban pay; gerçek Polkadot'ta bu daha karmaşık `compute_total_payout` ile belirlenir.

**NSC için:** Mevcut `ValidatorRewardShare=%30, StakerShare=%30` yapısını koruyun ama hesabı Polkadot'taki `era_reward_points_ratio` mantığıyla yapın — blok üretim katkısına orantılı dağıtım.

### A.4 Treasury Finansmanı

**Polkadot fee dağılımı (DealWithFees, impls.rs:47-68):**
```
Transaction fee → %80 Treasury + %20 Block author
Tip             → %100 Block author
```

**NSC mevcut kodu (burn.rs + fee_dist.rs):**
```
Base fee → %50 burn + %50 validator
Protocol fee → %0.25 DEX/Auction
```

**NSC için öneri — Polkadot modelini adapte et:**
```
Transaction base fee → %40 burn + %30 validator + %30 treasury
Tip                  → %100 validator
Protocol fee (DEX)   → %15 treasury + geri kalanı mevcut dağılım
```

Treasury burn formülü (compound, treasury/lib.rs:964-978):
```rust
// N dönem geçmişse:
let one_minus_burn = Burn::get().left_from_one();  // 1 - burn_rate
let percent_left = one_minus_burn.saturating_pow(N);
let burned = budget_remaining * (1 - percent_left);
// NOT: missed_any==true ise burn yapılmaz (güvenlik mekanizması)
```

**SpendPeriod:** Polkadot'ta ~24 gün. NSC için 7 gün önerisi.

### A.5 Fee Modeli — Exact Formül

**Polkadot fee formülü (agent-04, agent-30):**
```
final_fee = base_fee + len_fee + (multiplier * weight_fee) + tip
```

Bileşenler:
- `base_fee = ExtrinsicBaseWeight * WeightToFee`
- `len_fee = LengthToFee * encoded_extrinsic_bytes`
- `weight_fee = dispatch_weight * WeightToFee`
- `multiplier`: `NextFeeMultiplier` storage değeri (EIP-1559 benzeri, on_finalize'da güncellenir)
- `tip`: isteğe bağlı, priority artırır

**Multiplier güncelleme (TargetedFeeAdjustment, transaction-payment/lib.rs:262-264):**
```
target_fill = 25%  (hedef blok doluluk)
diff = (current_fill - target_fill) / target_fill
first_term  = v * diff
second_term = v^2 * diff^2 / 2
multiplier  = multiplier * (1 + first_term + second_term)
```
Hedefin altındaysa multiplier düşer, üstündeyse artar.

**2 boyutlu weight (sp-weights):**
```rust
Weight {
    ref_time: u64,    // picosaniye cinsinden hesaplama süresi
    proof_size: u64,  // storage proof boyutu (byte)
}
// Limiting dimension = max(normalized_ref_time, normalized_proof_size)
```

**NSC için öneri:**
- Mevcut 1D gas sisteminden 2D weight'e geçin: `ref_time + proof_size`
- `WeightToFee = ConstantMultiplier<Balance, BlockWeightFee>` ile başlangıç
- Multiplier mekanizmasını Polkadot'tan doğrudan alın
- EIP-1559'a göre üstünlük: gas'ı yakmanıza gerek yok, multiplier değişir

---

## B. NPoS — SEQUENTIAL PHRAGMÉN

### B.1 Algoritma Tam Açıklaması

Sequential Phragmén, `sp_npos_elections::seq_phragmen` ile implement edilmiştir (pallet-elections-phragmen, agent-28).

**Temel fikir:** Basit stake sıralaması yetersiz çünkü birkaç büyük nominator tüm oyları tek bir validatöre yığabilir. Phragmén, yükü dengeler.

**Algoritma adımları:**

```
1. Her nominator başlangıçta "yük" (load) = 0

2. n_seats doldurulana kadar döngü:
   a. Her aday için score hesapla:
      score = (1 + Σ(nominator_load_i * nominator_stake_i)) / aday_backing
      backing = Σ(bu adayı destekleyen nominatörler)

   b. En düşük score'lu adayı seç → winner

   c. Seçilen aday için her nominator'ın yükünü güncelle:
      new_load_i = (1 + load_old_i * stake_i * weight_i) / backing
      nominator_load_i += new_load_i * stake_i

   d. Seçilen adayı setten çıkar

3. Sonuç: Her validator için nominatörlerin orantılı payı
```

**Aday score formülü (kaynak: agent-28, do_phragmen):**
```
score(candidate) = (1 + Σ (load_n × stake_n)) / backing(candidate)
```
Düşük score = az yük = önce seçilmeli.

### B.2 Integer Arithmetic (Hassasiyet)

Polkadot, `sp_npos_elections` içinde `ExtendedBalance = u128` ile çalışır. f64 kesinlikle kullanılmamalı — sonuçlar deterministik olmalıdır.

```rust
// Integer scale: 1e9 = 1.0
const SCALE: u128 = 1_000_000_000;
// load hesaplamalarında Rational128 kullanılır (biguint.rs)
```

### B.3 election-provider-multi-phase Yaklaşımı (agent-28)

Büyük validator seti için off-chain hesaplama:

```
Faz Takvimi:
OFF → (SignedPhase) → (UnsignedPhase) → elect()

- Signed Faz: Herhangi biri çözüm gönderir, deposit yatırır
- Unsigned Faz: Validator off-chain worker (OCW) çözüm gönderir
- Emergency: Governance acil çözüm
```

**Signed submission deposit:**
```
deposit = SignedDepositBase(queue_size) + solution_bytes * SignedDepositByte + weight * SignedDepositWeight
```

### B.4 NSC için Phragmén Uygulama Adımları

1. `nova-runtime/src/validator.rs` içindeki basit sort'u kaldırın
2. `sp_npos_elections::seq_phragmen(desired_validators, candidates, voters)` çağrısı ekleyin
3. `ElectionBounds` ile voter/target limitlerini belirleyin
4. f64 yerine `ExtendedBalance = u128` kullanın
5. `MILLISECONDS_PER_YEAR = 31_557_600_000` (Julian yıl) referans alın
6. `MaxVoters`, `MaxTargets` config parametrelerini belirleyin (OOM riski)
7. İlk aşamada `BasicElectionProvider` (on-chain); büyüdükçe `multi-phase`

---

## C. GRANDPA FİNALİTY — TAM

### C.1 Protokol Adımları

GRANDPA, blok bazlı değil chain bazlı finality sağlar — her round'da hangi bloğun final olduğu toplu karara bağlanır (agent-06, agent-20).

```
Round N başlangıcı:
  ↓
PREVOTE: Her validator mevcut en iyi zinciri görünce prevote gönderir
  ↓
PREVOTE_GHOST hesabı:
  prevote_ghost = "2/3+1 supermajority'nin ortak ataları arasındaki en yüksek blok"
  ↓
PRECOMMIT: prevote_ghost veya onun atasına precommit gönderilir
  ↓
COMMIT: 2/3+1 precommit toplanınca Commit mesajı yayınlanır → blok finalize
  ↓
Round N+1 başlar
```

**Safety threshold:** `2n/3 + 1` — n=200 validator için `134`

### C.2 prevote_ghost Hesabı

```
prevote_ghost = max blok yüksekliği B öyle ki:
  |prevotes for B or descendant of B| >= 2n/3 + 1
```

Tüm prevote'lar toplanır, her blok için destekçi sayısı hesaplanır, eşiği geçen en yüksek blok seçilir.

### C.3 CATCH_UP Mekanizması

```
CATCH_UP_THRESHOLD = 2 round (client/consensus/grandpa/environment.rs:98)
```

Bir node, en son finalize edilmiş round'dan 2+ round geride kalırsa:
1. Akran node'lardan `CatchUp` mesajı ister
2. Mesajda: tamamlanan roundların prevote + precommit setleri
3. Node, bu verilerle kendi state'ini hızla günceller

**Gossip parametreleri (agent-20, agent-30):**
```
LUCKY_PEERS = 4       (gossip hedef sayısı)
MAX_NOTIFICATION = 1 MB  (mesaj boyut sınırı)
NUM_LAST_COMPLETED_ROUNDS = 2  (saklanan son round sayısı)
```

### C.4 Timeout Değerleri

Polkadot kaynağında sabit timeout yoktur — slot duration ile göreceli:
- `PREVOTE_TIMEOUT = 2 * slot_duration`
- `PRECOMMIT_TIMEOUT = 4 * slot_duration`
(Bu değerler finality_grandpa kütüphanesinde, NSC için yapılandırılabilir)

NSC slot duration = 400ms → prevote timeout ~800ms, precommit ~1600ms

### C.5 Equivocation

Aynı round'da iki farklı prevote veya precommit gönderilmesi = equivocation.

**Tespit (slots/aux_schema.rs:53-133):**
```
MAX_SLOT_CAPACITY = 1000 slot → bu pencerede ekipasman tespit edilir
PRUNING_BOUND = 2000          → temizleme eşiği
```

**Slash formülü (grandpa/equivocation.rs:101-105):**
```
slash = min((3k/n)^2, 1)
k = suçlu validator sayısı
n = toplam validator sayısı
```

1 validator double sign → k=1, n=200 → slash = (3/200)^2 = %0.0225
100 validator double sign → k=100, n=200 → slash = (3*100/200)^2 = 2.25 → %100 cap

**NSC için:** Mevcut `double_sign = %100 sabit` yerine `min((3k/n)^2, 1)` dinamik formülü kullanmak daha adil. Tek hata küçük ceza, organize saldırı ağır ceza.

### C.6 BFT + GRANDPA Entegrasyon Stratejisi

Nova Shield mevcut durumu: BFT (Simplex) blok üretir, GRANDPA finalize eder.

Polkadot'taki BABE + GRANDPA mimarisi:
- BABE (veya Simplex BFT): Blok Producer
- GRANDPA: Finality Gadget
- Ayrı key'ler: BABE sr25519, GRANDPA ed25519

**NSC uygulama:**
1. Simplex BFT blok üretir ve kendi BFT finality'sini sağlar
2. GRANDPA ek bir finality katmanı olarak üstünde çalışır
3. Blok GRANDPA ile finalize edilmeden "kesin final" sayılmaz
4. `grandpa.rs`'e eksik olan timeout logic ve BFT bağlantısı:
   ```rust
   // Nova grandpa.rs'e eklenmesi gereken:
   const PREVOTE_TIMEOUT: Duration = Duration::from_millis(800);
   const PRECOMMIT_TIMEOUT: Duration = Duration::from_millis(1600);
   // BFT finalized block → GRANDPA'ya bildir → GRANDPA başlangıç noktası olarak kullan
   ```

### C.7 GRANDPA Justification

```rust
// sp_consensus_grandpa/src/lib.rs:133-137
pub struct GrandpaJustification<Header> {
    pub round: u64,
    pub commit: Commit<Header>,
    pub votes_ancestries: Vec<Header>,
}
// localized_payload: (message, round, set_id).encode() → imza
```

Hafif istemci doğrulaması için justification yeterli — tüm blok geçmişi gerekmez.

---

## D. BABE VE SLOT LEADER SEÇİMİ

### D.1 Primary Slot (VRF-Tabanlı)

**Tam threshold formülü (authorship.rs:37-97):**
```
theta = authority_weight / total_authority_weight
p = 1 - (1 - c)^theta
threshold = (2^128) * p
```

VRF claim akışı:
```rust
// AUTHORING_SCORE_VRF_CONTEXT = b"substrate-babe-vrf"  (lib.rs:156)
// AUTHORING_SCORE_LENGTH = 16 byte (128-bit)           (lib.rs:159)

data = make_vrf_sign_data(randomness, slot, epoch_index);
vrf_sig = keystore.sr25519_vrf_sign(authority_id, data);
score = make_bytes::<16>(AUTHORING_SCORE_VRF_CONTEXT, data, vrf_sig.pre_output);
can_claim = u128::from_le_bytes(score) < threshold;
```

### D.2 Secondary Slot (Fallback)

Primary slot claim olmazsa deterministik fallback (authorship.rs:102-123):
```
rand = blake2_256(epoch_randomness || slot_number)
idx  = U256::from_big_endian(rand) % authorities_len
expected_author = authorities[idx]
```

Bu `SecondaryPlain` (imzasız) veya `SecondaryVRF` (ek VRF) olabilir.

### D.3 c Parametresi

`c` = hedef slot doluluk oranı. Genellikle `c = 1/6` (Polkadot) — her slot'ta bir validator %1/6 ihtimalle primary olur.

`c` düşükse: Daha az primary slot → daha az boş slot → saldırı direnci düşer
`c` yüksekse: Çok validatör aynı slotu claim edebilir → fork olasılığı artar

### D.4 Epoch Süresi ve NSC Çakışması

**Polkadot:** EpochDuration = 2400 slot (normal) — ama bu 600 slot'luk BABE epoch'u

**NSC mevcut bug:** `nova-consensus` epoch = 2400 slot, `nova-runtime` epoch = 216,000 slot

**Çözüm:**
- `SLOT_DURATION = 400ms` sabitte kal
- `EPOCH_DURATION = 216_000 slot` = 24 saat (1 epoch = 1 gün)
- Consensus motorunu bu değere hizalayın
- BABE'deki `EpochDuration` parametresini 216_000 olarak set edin

### D.5 BLAKE3 Mock VRF → sr25519 VRF Geçişi

NSC mevcut: `VrfVerifier::verify stub (always Err)` (kritik bug #3)

**Geçiş adımları:**
```rust
// Adım 1: sp-keystore crate'ini ekle
// Cargo.toml: sp-keystore = { workspace = true }

// Adım 2: sr25519 VRF kullan
use sp_core::sr25519::vrf::{VrfSignature, VrfPreOutput, VrfInput};

// Adım 3: VRF imzalama (block authoring sırasında)
let vrf_input = VrfInput::new(b"nova-babe-vrf", &[(b"slot", &slot_bytes)]);
let vrf_signature = keypair.vrf_sign(&vrf_input);

// Adım 4: VRF doğrulama (blok import sırasında)
let result = public_key.vrf_verify(&vrf_input, &vrf_signature);

// Adım 5: Threshold karşılaştırma
let score = vrf_signature.pre_output.make_bytes::<16>(b"nova-score", &vrf_input);
let score_u128 = u128::from_le_bytes(score);
assert!(score_u128 < threshold);
```

### D.6 Epoch Randomness

Epoch N'nin randomness'ı, Epoch N-2'de üretilmiş VRF çıktılarından türetilir. Bu 2 epoch gecikme kuralı, validators'ın önceki randomness'ı etkileyememesini garanti eder.

**NSC için:** Session sistemi N+2 gecikmesini uygular. `pallet-babe`nin `ShouldEndSession` ve `rotate_session` mantığı bu gecikmeyi yönetir.

---

## E. GOVERNANCE — OPENGOV VE CONVİCTİON VOTING

### E.1 Conviction Voting Sistemi

**Conviction enum ve multiplier'lar (conviction.rs:44-59, 100-122):**
| Conviction | Oy Gücü | Lock Periyot |
|------------|---------|--------------|
| None       | 0.1x    | 0 dönem      |
| Locked1x   | 1x      | 1 dönem      |
| Locked2x   | 2x      | 2 dönem      |
| Locked3x   | 3x      | 4 dönem      |
| Locked4x   | 4x      | 8 dönem      |
| Locked5x   | 5x      | 16 dönem     |
| Locked6x   | 6x      | 32 dönem     |

Lock süreleri katlanarak artıyor: 1→2→4→8→16→32

**Oy ağırlığı formülü:**
```rust
// conviction.rs:113-122
let votes = match conviction {
    Conviction::None => capital.checked_div(&10u8.into()).unwrap_or(Zero::zero()),
    x => capital.checked_mul(&u8::from(x).into()).unwrap_or(Balance::max_value()),
};
```

**Lock hesabı (kritik):**
```
Toplam lock = MAX(tüm oy miktarları, prior_lock)  // TOPLAM değil, MAX!
PriorLock = MAX(yeni süre, eski süre) ve MAX(yeni miktar, eski miktar)
```

**Kilit süresi formülü:**
```
unlock_at = poll_end_block + VoteLockingPeriod * conviction.lock_periods()
```

Örnek: VoteLockingPeriod=7 gün, Locked3x kullanırsa → 7 × 4 = 28 gün kilit

### E.2 Tally Hesabı

```rust
// types.rs:64-79
fn support() -> Perbill {
    support / MaxTurnout  // Katılım oranı (HAM kapital, conviction'sız)
}

fn approval() -> Perbill {
    ayes / (ayes + nays)  // Onay oranı (conviction SONRASI değerler)
}
```

**Kritik fark:** `support` = ham kapital (aye + abstain, conviction'sız); `ayes/nays` = post-conviction

### E.3 Referendum Türleri (OpenGov Tracks)

**pallet-democracy (ESKI, deprecated):**
- `SuperMajorityApprove`: `nays/√turnout < ayes/√electorate`
- `SuperMajorityAgainst`: `nays/√electorate < ayes/√turnout`
- `SimpleMajority`: `ayes > nays`

**pallet-referenda (OpenGov, YENI):**
- Track-based sistem: her track farklı `min_approval`, `min_support`, `prepare_period`
- Decision Deposit: spam önlemi (zorunlu)
- Onay eğrisi: `LinearDecreasing` veya `Reciprocal`
- `confirm_period`: onay eşiği N blok boyunca korunmalı
- Multi-track: birden fazla referendum eş zamanlı

**NSC için önerilen tracks:**
```
Track 0 (Root): origin=Root, prepare=7gün, decision=28gün, confirm=1gün
Track 1 (Governance): origin=Governance, prepare=1gün, decision=14gün, confirm=1gün
Track 2 (Treasury): origin=Treasury, prepare=3gün, decision=7gün, confirm=12saat
Track 3 (Emergency): origin=Emergency, prepare=0, decision=1saat, confirm=30dk
```

### E.4 Delegasyon Kuralları

```
Delegasyon derinliği = YALNIZCA 1 seviye (lib.rs:573-577)
Bir Delegating hesaba delege edilebilir ama o hesabın delegasyonu upstream yayılmaz
```

NSC için: Guardian sistemi delegasyonu benzer — guardian'ın kendi guardian'ı olamaz.

### E.5 NSC Governance'ına Conviction Voting Ekleme

1. `nova-runtime/src/governance.rs`'e `Conviction` enum ekleyin
2. `VoteLockingPeriod = 216_000 slot` (1 epoch = 1 gün) referans alın
3. `MaxVotes = 100` (bir hesabın aynı anda oy kullanabileceği max referendum sayısı)
4. Lock için `CONVICTION_VOTING_ID = b"pyconvot"` (veya NSC'ye özgü 8 byte)
5. `ClassLocksFor` storage: her hesap için class bazlı kilit miktarları
6. `update_lock` → `Currency::set_lock` veya yeni Freeze API'si

### E.6 On-Chain Treasury Harcama

```
spend() → SpendStatus::Pending → payout() → check_status() → SpendProcessed
```

`spend_local` DEPRECATED (Mayıs 2025). Yeni API `spend + Paymaster` kullanın.

---

## F. SLASHING VE KRİMİNAL EVIDENCE

### F.1 Slash Formülleri

**Equivocation slash (BABE/GRANDPA):**
```
slash = min((3k/n)^2, 1)
k = aynı suçu işleyen validator sayısı
n = toplam aktif validator sayısı
```

**Im-online slash (lib.rs:862-872):**
```
threshold = max(0, k - n/10 - 1)
slash_raw = min(3 * threshold / n, 1)
slash = slash_raw * 7%   (tam cetvelin %7'si kadar)
```

Yani offline slash çok daha küçük — tek validator offline = neredeyse sıfır ceza, %33+ offline = anlamlı ceza.

### F.2 Cumulative (Kümülatif) Slash

Aynı era içinde birden fazla suç varsa:
```rust
// slashing.rs içinde:
let new_slash = max(existing_slash, new_slash_fraction);
// Additive DEĞİL, MAX alınır
```

Birden fazla suçun cezası toplanmaz, en yüksek olanı uygulanır.

### F.3 Slash Deferral Mekanizması

```
SlashDeferDuration = 27 era (Polkadot)
```

Slash hemen uygulanmaz, N era sonra uygulanır. Bu süre içinde:
- Governance iptal edebilir (`cancel_deferred_slash`)
- Validator itiraz edebilir

NSC için öneri: `SlashDeferDuration = 7 epoch` (7 gün)

### F.4 NSC İçin Slash Tasarımı

**Mevcut NSC (kritik):**
```
double_sign: %100 (sabit tombstone)  ← çok sert
mempool_leak: %50
wrong_batch_clearing: %25
extended_offline: %1
```

**Polkadot'tan öğrenilenler:**
- Double sign için `min((3k/n)^2, 1)` kullanın — tek hata küçük ceza
- `tomstone` (permaban) yerine `slash + unbonding_deferral`
- `im-online` benzeri offline cezası: `n/10`'u geçmedikçe ceza yok

**Equivocation kanıt formatı:**
```rust
// sp_consensus_slots/SlotDuration ile slot window
// EquivocationProof { offender, slot, first_header, second_header }
// generate_key_ownership_proof(hash, set_id, offender) → validator üyelik kanıtı
```

---

## G. FEE VE AĞIRLIK SİSTEMİ

### G.1 2D Weight Sistemi

```rust
// sp-weights/weight_v2.rs
pub struct Weight {
    ref_time: u64,    // picosaniye (hesaplama)
    proof_size: u64,  // byte (storage proof)
}

// Her iki boyut için ayrı blok limiti
// WeightToFee tipik olarak ref_time baz alır
```

### G.2 Fee Formülü (Tam)

```
base_fee     = ExtrinsicBaseWeight * WeightToFee(ref_time)
len_fee      = LengthToFee * len(encoded_extrinsic_in_bytes)
weight_fee   = dispatch_weight.ref_time * WeightToFee
adjusted_fee = weight_fee * NextFeeMultiplier
inclusion_fee = base_fee + len_fee + adjusted_fee
final_fee    = inclusion_fee + tip
```

**Multiplier güncelleme (on_finalize, transaction-payment/lib.rs:458-471):**
```
Hedef doluluk = %25 (TargetedFeeAdjustment)
Hedef altında → multiplier düşer
Hedef üstünde → multiplier artar
```

**MULTIPLIER_DEFAULT_VALUE = Multiplier::from_u32(1)** (lib.rs:320)

### G.3 Weight Reclaim

```rust
// frame-system/extensions/weight_reclaim.rs
// Blok sonunda kullanılmayan weight geri verilir
// WeightReclaim extension → on_post_dispatch
```

### G.4 NSC İçin 1D vs 2D Karar

**1D Gas (Ethereum tarzı):** Tüm maliyeti tek sayıya indirger. Basit ama proof_size maliyetini ayırt edemez.

**2D Weight (Polkadot tarzı):** Hesaplama maliyeti + storage proof maliyetini ayrı izler. Daha adil ama daha karmaşık.

**NSC önerisi:** 2D weight ile başlayın. Gerekçe:
- Guardian MPC işlemleri proof_size yoğun
- ZK proof doğrulaması ref_time yoğun
- İkisi ayrı fiyatlanabilir
- Light client desteği için proof_size kritik

```rust
// NSC weight tanımı
pub struct NscWeight {
    pub ref_time: u64,    // computational cost (picosec)
    pub proof_size: u64,  // storage proof (bytes)
}

// Her extrinsic için benchmark ile ölçün
#[pallet::weight(T::WeightInfo::my_call())]
pub fn my_call(origin) -> DispatchResult { ... }
```

---

## H. PALLET SİSTEMİ VE MİMARİ

### H.1 FRAME Pallet Mimarisi

```rust
#[pallet::pallet]
pub struct Pallet<T>(PhantomData<T>);

// Config trait
#[pallet::config]
pub trait Config: frame_system::Config {
    type RuntimeEvent: From<Event<Self>>;
    // pallet-specific types...
}

// Storage
#[pallet::storage]
pub type MyStorage<T> = StorageMap<_, Blake2_128Concat, Key, Value>;

// Events
#[pallet::event]
#[pallet::generate_deposit(pub(super) fn deposit_event)]
pub enum Event<T: Config> { SomethingHappened { ... } }

// Errors
#[pallet::error]
pub enum Error<T> { SomeError, }

// Extrinsics
#[pallet::call]
impl<T: Config> Pallet<T> {
    #[pallet::weight(T::WeightInfo::my_call())]
    pub fn my_call(origin: OriginFor<T>) -> DispatchResult { ... }
}

// Hooks
#[pallet::hooks]
impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
    fn on_initialize(n: BlockNumberFor<T>) -> Weight { ... }
    fn on_finalize(n: BlockNumberFor<T>) { ... }
    fn on_idle(n: BlockNumberFor<T>, remaining_weight: Weight) -> Weight { ... }
}
```

### H.2 Blok Yaşam Döngüsü

```
initialize_block()
  → on_initialize() (her pallet, weight limiti dahilinde, önce)
  → execute_extrinsics() (inherent önce, signed sonra)
  → on_idle() (kalan weight ile)
  → on_finalize()
  → finalize_block()
```

### H.3 Locks vs Freezes vs Holds (KRİTİK)

```
frozen = max(all_locks, all_freezes)   // ADDİTİF DEĞİL, MAX!
spendable = free - max(frozen, reserved) - existential_deposit
```

- **Locks** (deprecated API): MAX ile birleşir
- **Freezes** (yeni API): named, MAX ile birleşir — transfer kısıtlar
- **Holds** (en yeni API): named, SUM'lanır — reserve eder

**NSC için:** Sadece Freeze + Hold kullanın. Lock API kullanmayın (deprecated).

```rust
// Freeze örneği (Guardian staking için)
T::Currency::freeze(&FreezeReason::GuardianStake, &who, amount)?;

// Hold örneği (recovery deposit için)
T::Currency::hold(&HoldReason::RecoveryDeposit, &who, amount)?;
```

### H.4 construct_runtime! ve Pallet Index

```rust
frame_support::construct_runtime!(
    pub enum Runtime {
        System: frame_system = 0,          // ZORUNLU, her zaman 0
        Balances: pallet_balances = 1,
        Staking: nova_staking = 2,
        // ...
    }
);
```

**KRİTİK:** Index'ler sabit olmalı. Sonradan değiştirmek call dispatch wire format'ı bozar.

### H.5 Runtime Upgrade Mekanizması

```
set_code(new_wasm) → WASM doğrulama → storage'a kayıt
→ Sonraki blokta on_runtime_upgrade() çağrılır
→ StorageVersion kontrol edilir
→ Migration yalnızca doğru versiyon için çalışır
```

**Multi-block migration (pallet-migrations, agent-29):**
```rust
// Her blokta step() çağrılır, weight budget içinde kalan iş yapılır
impl MigrationList for MyMigrations {
    fn list() -> Vec<Box<dyn SteppedMigration>> {
        vec![Box::new(MigrationV1ToV2::new())]
    }
}
```

`OnRuntimeUpgrade` → Eylül 2026'da deprecated, `pallet-migrations` kullanın.

### H.6 NSC Crate Yapısı Karşılaştırması

| Polkadot Pallet | NSC Karşılığı | Durum |
|----------------|---------------|-------|
| pallet-system | nova-runtime/system | Mock |
| pallet-balances | nova-runtime/token | Kısmi |
| pallet-staking | nova-runtime/staking | Kısmi |
| pallet-transaction-payment | nova-runtime/fee | Kısmi |
| pallet-conviction-voting | nova-runtime/governance | Eksik |
| pallet-referenda | nova-runtime/governance | Eksik |
| pallet-treasury | nova-runtime/treasury | Eksik |
| pallet-nomination-pools | nova-guardian/pool | Eksik |
| pallet-safe-mode | nova-freeze/emergency | Partial |
| pallet-tx-pause | nova-freeze/pause | Eksik |

---

## I. XCM — CROSS-CHAIN MESSAGING

### I.1 Versiyon Seçimi

```
v3: MIN_XCM_VERSION — minimum desteklenen
v4: LTS (Long-Term Support) — önerilen başlangıç
v5: LATEST — InitiateTransfer, PayFees, SetHints

// Encoding:
VersionedXcm::V4(xcm) → SCALE byte[0] = 4
VersionedXcm::V5(xcm) → SCALE byte[0] = 5
```

**NSC mevcut:** `MAX_INSTRUCTIONS=32, NOVA_PARACHAIN_ID=1000`
**Polkadot:** `MAX_INSTRUCTIONS_TO_DECODE=100, MAX_XCM_DECODE_DEPTH=8`

NSC'nin limitini 100'e yükseltmek gerekiyor.

### I.2 Tam Limit Tablosu

| Sabit | Değer | Kaynak |
|-------|-------|--------|
| MIN_XCM_VERSION | 3 | xcm/src/lib.rs:533 |
| MAX_XCM_DECODE_DEPTH | 8 | xcm/src/lib.rs:55 |
| MAX_INSTRUCTIONS_TO_DECODE | 100 | xcm/src/lib.rs:59 |
| MAX_ITEMS_IN_ASSETS | 20 | xcm/src/v5/asset.rs:556 |
| RECURSION_LIMIT | 10 | xcm-executor/src/lib.rs:72 |
| MAX_ASSETS_FOR_BUY_EXECUTION | 2 | xcm-builder/barriers.rs:58 |
| MaxAssetTransferFilters | 6 | xcm/src/v5/mod.rs:213 |

### I.3 PayFees vs BuyExecution

```
BuyExecution (v3/v4): Holding Register'dan weight satın al
PayFees (v5): Holding'den fees register'a koy; delivery fee da buradan

// Her iki instruction'ı destekleyin (geriye dönük uyumluluk)
```

### I.4 IsReserve Güvenlik Riski

```
IsReserve yanlış konfigüre edilirse → mint exploit!

// Güvenli yapılandırma:
type IsReserve = Case<(NativeAsset, NovaReserveLocation)>;
// "Sadece Nova Shield'in sovereign account'ı reserve'dir"

// Tehlikeli:
type IsReserve = Everything;  // TÜM kaynaklara güven → saldırı vektörü
```

### I.5 Asset Trap Mekanizması

```
Execution başarısız, Holding Register'da asset kalırsa:
→ AssetTrap::drop_assets(claimer, holding) çağrılır
→ claimer: SetHints { AssetClaimer } ile önceden belirlenebilir (v5)
→ ClaimAsset instruction ile geri alınabilir
```

**NSC için:** `AssetTrap` mutlaka implement edilmeli. Aksi halde kullanıcı fonları kaybolur.

### I.6 Barrier Stack Tasarımı

```rust
// NSC için önerilen barrier:
type Barrier = DenyThenTry<
    // Önce yasakla
    DenyRecursively<Everything>,
    // Sonra izin ver
    (
        TakeWeightCredit,
        AllowTopLevelPaidExecutionFrom<Everything>,
        AllowKnownQueryResponses<NscXcm>,
        AllowSubscriptionsFrom<Everything>,
        TrailingSetTopicAsId<Everything>,
    )
>;
```

### I.7 v5 InitiateTransfer (Multi-hop)

```rust
// Tek instruction ile multi-hop transfer:
InitiateTransfer {
    destination: Location,
    remote_fees: Some(AssetTransferFilter::ReserveDeposit(Wild)),
    preserve_origin: false,
    assets: bounded_vec![AssetTransferFilter::ReserveDeposit(Wild)],
    remote_xcm: Xcm(vec![DepositAsset { ... }]),
}
// MaxAssetTransferFilters = 6 limitine dikkat!
```

### I.8 BEEFY Light Client

NSC için cross-chain vault (BEEFY + IBC):

```
BEEFY quorum: 2/3+1 (NSC'deki MAX_COMMITMENTS=256 değeri)
Commitment format: merkle root of all parachain headers
```

**NSC + IBC ikili strateji:**
- Cosmos varlıkları için → IBC ICS-20
- Ethereum/Polkadot varlıkları için → BEEFY light client
- XCM → Nova'nın kendi mesajlaşma protokolü

---

## J. STAKING VE NOMİNATÖR HAVUZLARI

### J.1 Nomination Pools Mimari

```
Pool State: Open → Blocked → Destroying (tek yön)

Roller:
- Depositor: pool kurucusu, son ayrılan
- Root: tam kontrol
- Nominator: validator seçer
- Bouncer: üye kabul/ret

Genesis defaults:
- max_pools = 16
- max_members_per_pool = 32
- max_members = 512  (= 16×32)
```

Üretim için: `max_pools = None` (sınırsız) veya büyük değer.

### J.2 Points Sistemi

```
Başlangıç: 1 point = 1 token (POINTS_TO_BALANCE_INIT_RATIO = 1)
Slash sonrası: points değişmez, balance düşer → oran değişir

// Pool'a katılırken:
points_issued = (current_points / current_balance) * amount_joining
// U256 ile hesaplanmalı (overflow koruması)
```

### J.3 Reward Counter Mekanizması

```rust
// RewardCounter = FixedU128
pending_rewards = (current_rc - member.last_rc) * member.active_points

// KRİTİK: update_records() her points değişikliğinden ÖNCE çağrılmalı!
// join(), bond_extra(), unbond() sırasında
```

### J.4 Komisyon Sistemi

```
Komisyon artırma: max_increase/min_delay throttle kuralına uymalı
Komisyon max:     bir kez ayarlandıktan sonra SADECE azaltılabilir
GlobalMaxCommission: governance tarafından belirlenen üst sınır
```

### J.5 Stash + Controller Hesap Ayrımı

Polkadot'ta stash (fon sahibi) ve controller (işlem yapan) ayrı hesaplar. Bu ayrım artık `pallet-staking-async`'te sadeleştirildi ama güvenlik katmanı olarak hala değerli.

**NSC için Guardian Staking:** Guardian, hem stash hem controller rolünü üstlenebilir. Ama kurumsal guardian'lar için ayrım düşünülmeli.

### J.6 DelegateStake vs TransferStake

```
TransferStake: DEPRECATED (adapter.rs:255)
DelegateStake: KULLANIN — kullanıcı stake'i kendi hesabında tutar

// TransferStake'de slash anında uygulanır (greedy)
// DelegateStake'de slash ertelenmiş, apply_slash ile uygulanır (lazy)
```

### J.7 NSC Delegatör Reward Claim

```
ClaimPermission default: PermissionlessWithdraw
// Başkası kullanıcı adına reward çekebilir → auto-compound botlar için ideal
// Güvenlik istiyorsanız: Permissioned default

// Auto-compound mekanizması:
1. Bot claim_payout() çağırır
2. Ödenen miktar bond_extra()'ya verilir
3. Net sonuç: compound staking
```

---

## K. CONTRACTS VE VM

### K.1 pallet-contracts (WASM/Ink!)

**Limitler (schedule.rs):**
```
memory_pages     = 16  (= 1 MB)
payload_len      = 16 KB
event_topics     = 4
call_stack_depth = T::CallStack::size() + 1
```

**Storage deposit formülü:**
```
deposit = DepositPerByte * bytes_added + DepositPerItem * items_added
base_deposit = info_deposit + CodeHashLockupPercent * code_deposit
refund = (bytes_removed / total_bytes) * total_byte_deposit  [pro rata]
```

**Host functions:** 111 stable seal_* API (compile-time sayımı ile)

### K.2 pallet-revive (PolkaVM + EVM)

**KRİTİK limitler (limits.rs):**
```
CALL_STACK_DEPTH    = 25 (sabit, değiştirilemez)
EVM_MEMORY_BYTES    = 1 MB
CALLDATA_BYTES      = 128 KB
EVENT_BYTES         = 64 KB
TRANSIENT_STORAGE   = 4 KB
BLOB_BYTES          = 1 MB (max kod boyutu)
BASIC_BLOCK_SIZE    = 1000 instruction
```

**Gas dönüşümü:**
```
GasScale = 10        → EVM gas × 10 = native weight
NativeToEthRatio = 1_000_000 → token precision dönüşümü
```

**Adres türetme:**
```rust
// create1: keccak(RLP(address, nonce))
// create2: keccak(0xff ++ address ++ salt ++ init_code_hash)
```

**EVM precompile'lar:** ecrecover(0x01), sha256(0x02), ripemd160(0x03), identity(0x04), modexp(0x05), bn128_add/mul/pair(0x06-08), blake2f(0x09), point_eval(0x0A), p256_verify

### K.3 Reentrance Koruması

```rust
// pallet-revive: 3 seviye
enum ReentrancyProtection {
    AllowReentry,  // serbest
    Strict,        // callee ve callee's callees bu kontratı çağıramaz
    AllowNext,     // callee çağırabilir, recursive olamaz
}

// Nova Shield DEX kontratları için: Strict kullanın
// Guardian MPC kontratları için: AllowNext (guardian chain)
```

### K.4 NSC VM Sistemi İçin Polkadot'tan Alınacaklar

1. **Storage deposit:** DepositPerByte + DepositPerItem spam'i önler → alın
2. **Call stack depth:** 25 (revive) veya yapılandırılabilir (contracts) → alın
3. **Wasm kısıtlamaları:** float yasağı, multi-value yasağı → alın
4. **Lazy deletion:** DeletionQueue + on_idle → alın
5. **Gas/weight dönüşüm:** GasScale=10 ≈ makul başlangıç
6. **EVM uyumluluğu:** pallet-revive modelini inceleyin, H160 adres uyumluluğu kritik
7. **Transient storage:** EIP-1153 eşdeğeri, within-tx reentrancy lock için

---

## L. NOVA SHIELD'E ÖZEL UYARLAMA

### L.1 Guardian Staking → Nomination Pool Benzeri Yapı

Polkadot nomination pool ile Guardian pool arasındaki analoglar:

| Nomination Pool | Guardian Pool |
|----------------|---------------|
| Pool Creator (Depositor) | Guardian Coordinator |
| Pool Members | Guardian Set |
| Nominator role | Guardian Selection Committee |
| Bouncer role | Trust Score Manager |
| Commission | Guardian Fee |
| MaxPointsToBalance | GuardianTrustThreshold |

**Guardian Pool Tasarımı:**
```rust
pub struct GuardianPool {
    guardian_id: AccountId,
    min_stake: Balance,            // Kategori'ye göre (1000/10000/5000 NSC)
    trust_score: u64,              // stake*30 + years*25 + success*20 + slash*15 + response*10
    available_slots: u32,
    commission: Perbill,
    state: PoolState,              // Open/Blocked/Destroying
}
```

**Reward dağıtımı:** Guardian fee toplam cross-chain koruma ücretinin %40'ı (mevcut kod). NSC pool'da bu komisyon update_records() mantığıyla hesaplanmalı.

### L.2 ZK Shield + Polkadot Governance Entegrasyonu

ZK Shield parametrelerinin governance ile güncellenmesi:

```rust
// pallet-parameters ile ZK parametreleri on-chain güncellenebilir
// Runtime upgrade gerektirmez
parameter_types! {
    pub NscZkShieldGasFullShield: u64 = 200_000;
    pub NscZkShieldGasSemiShield: u64 = 80_000;
    pub NscFMDPrecision: u8 = 4;
}

// Governance oyu ile değiştirme:
type FmdPrecisionParam: Parameter<Key=FmdPrecisionKey, Value=u8>;
```

### L.3 Cross-Chain Vault → XCM ile BEEFY Light Client

**Mevcut:** Vault adresi guardian içermiyor (bug #14)

**Polkadot'tan alınacak:** XCM sovereign account derivation
```rust
// Vault hesap türetme (Polkadot HashedDescription'dan):
let vault_id = blake2_256(
    b"nova-xchain-vault",
    chain_id,
    asset_type
);
// Bu keyless, deterministik hesap guardian MPC ile imzalanır
```

**Multi-hop XCM (v5 InitiateTransfer):**
```
Nova Shield → Cosmos Hub (IBC ICS-20)
Nova Shield → Ethereum   (BEEFY light client + XCM bridge)
Nova Shield → Polkadot   (XCM native)
```

### L.4 Nova'nın 3 Konsensüs Motoru Entegrasyon Stratejisi

**Mevcut durum:** BFT + GRANDPA + Snowball bağımsız çalışıyor, birbirine bağlanmamış.

**Polkadot'tan öğrenilen entegrasyon:**

```
Katman 1 - Blok Üretimi: Simplex BFT
  → Quorum: (2n+2)/3 = 134 (n=200)
  → Primary slot seçimi: VRF threshold (BABE formülü)
  → Blok sealed → Gossip (LUCKY_PEERS=4 gibi)

Katman 2 - Fork Çözümü: Snowball
  → K=20, α=15, β_virtuous=15, β_rogue=20
  → BFT'nin forks'larını çözer
  → Polkadot'ta fork choice = primary block count ağırlığı

Katman 3 - Kesin Finality: GRANDPA
  → BFT/Snowball'ın finalize ettiği zincir üzerinde çalışır
  → CATCH_UP_THRESHOLD = 2 round
  → 2/3+1 supermajority

Entegrasyon noktaları:
  BFT → GRANDPA: BFT finalized block → GRANDPA anchor point
  Snowball → BFT: fork tercih kararı → BFT'ye bildir
  GRANDPA → zincir: final blok → state commitment
```

**Implementasyon adımları:**
1. `nova-consensus/src/integration.rs` oluşturun
2. `BftEngine::on_finalize(block)` → `GrandpaEngine::set_base(block)` çağrısı
3. `SnowballEngine::preferred_chain()` → `BftEngine::set_preferred()` çağrısı
4. Her motor için ayrı key tipi: BFT=sr25519, GRANDPA=ed25519

### L.5 Safe Mode ve Emergency Pause (Nova Freeze Entegrasyonu)

Polkadot'ta güvenlik paleti üçlüsü (agent-29):
```
pallet-safe-mode:  TÜM extrinsic'leri durdur (acil)
pallet-tx-pause:   Belirli call'ları durdur (granüler)
pallet-whitelist:  Önceden yetkilendirilmiş call'lar
```

NSC freeze sistemi ile entegrasyon:
```rust
// nova-freeze/src/emergency.rs
pub enum FreezeLevel {
    // Polkadot safe-mode benzeri
    EmergencyAll,    // Sadece Receive + Unfreeze
    // Polkadot tx-pause benzeri
    Selective(Vec<CallSelector>),
    // Normal
    None,
}

// Guardian report → FreezeLevel::Selective
// Attack confirmed → FreezeLevel::EmergencyAll
// 72h timeout → Legacy/Inheritance
```

### L.6 Vesting Schedule — Miras Sistemi Entegrasyonu

Polkadot vesting formülü (vesting_info.rs:88-101):
```
locked_at(n) = locked - ((n - starting_block) * per_block)
ending_block = starting_block + ceil(locked / per_block)
```

NSC miras sistemi (staged release, max 20 adım) ile entegrasyon:
```rust
// Her miras adımı → bir VestingInfo schedule
pub struct StagedRelease {
    steps: Vec<VestingInfo<Balance, BlockNumber>>,  // max 20
    conditions: Vec<ConditionalRule>,               // max 10
}
// vest() manuel çağrı gerektiriyor — heartbeat ile birleştirin
// Kullanıcı sinyal gönderdiğinde vest() de çağrılsın
```

---

## M. MASTER DEĞERLER TABLOSU (DOĞRULANMIŞ)

| Değer | Sayı | Kaynak Dosya |
|-------|------|-------------|
| EXTRINSIC_FORMAT_VERSION | 5 | sp-runtime/unchecked_extrinsic.rs:55 |
| MILLISECONDS_PER_YEAR | 31_557_600_000 | inflation.rs:42 |
| REWARD_F1 (validator pay) | 50% | slashing.rs:72 |
| Staking slash formülü | min((3k/n)^2, 1) | equivocation.rs:101-105 |
| im-online slash | min((3*(k-n/10-1))/n, 1) * 7% | lib.rs:862-872 |
| GRANDPA CATCH_UP | 2 round | environment.rs:98 |
| GRANDPA LUCKY_PEERS | 4 | gossip.rs |
| GRANDPA notification max | 1 MB | client/grandpa |
| MAX_SLOT_CAPACITY (BABE) | 1000 | aux_schema.rs:53 |
| BABE VRF context | b"substrate-babe-vrf" | lib.rs:156 |
| BABE score length | 16 byte | lib.rs:159 |
| AMM scaling A | 1_000_000 (Permill ACCURACY) | lib.rs |
| AMM LPFee default | 3 (= 0.3%) | mock.rs:127 |
| MintMinLiquidity | 100 | mock.rs:163 |
| MaxSwapPathLength | 4 | mock.rs:162 |
| POINTS_TO_BALANCE_INIT | 1 | lib.rs:423 |
| Nomination pool max_pools (genesis) | 16 | lib.rs:1848 |
| Nomination pool max_members (genesis) | 512 | lib.rs:1850 |
| Treasury burn formülü | (1-(1-Burn)^N) * budget | lib.rs:967-971 |
| Tip median | tips[len/2] | lib.rs:578 |
| Tip threshold | ceil(N/2) | lib.rs:532 |
| Fee: Treasury | 80% | impls.rs:59-65 |
| Fee: Author | 20% | impls.rs:59-65 |
| Fee: Tip | 100% author | impls.rs:59-65 |
| Polkadot ideal_stake | 75% | impls.rs:95-98 |
| Polkadot min_inflation | 2.5% | impls.rs:95-98 |
| Polkadot max_inflation | 10% | impls.rs:95-98 |
| Polkadot falloff | 5% | impls.rs:95-98 |
| TX pool Ready limit | 8192 tx / 20 MB | fork_aware_txpool |
| TX pool Future limit | 512 tx / 1 MB | fork_aware_txpool |
| TX pool ban süresi | 30 dakika | rotator.rs |
| TX pool FINALITY_TIMEOUT | 512 blok | fork_aware_txpool |
| RPC MAX_PINNED_BLOCKS | 512 | rpc |
| RPC MAX_ONGOING_OPERATIONS | 16 | rpc |
| pallet-contracts memory | 1 MB (16 pages) | schedule.rs |
| pallet-contracts payload | 16 KB | schedule.rs |
| pallet-contracts event topics | 4 | schedule.rs |
| pallet-revive CALL_STACK_DEPTH | 25 (sabit) | limits.rs |
| pallet-revive EVM_MEMORY | 1 MB | limits.rs |
| pallet-revive CALLDATA | 128 KB | limits.rs |
| pallet-revive TRANSIENT_STORAGE | 4 KB | limits.rs |
| pallet-revive GasScale | 10 | lib.rs |
| pallet-revive NativeToEthRatio | 1_000_000 | lib.rs |
| Conviction None multiplier | 0.1x | conviction.rs:118 |
| Conviction Locked6x multiplier | 6x / 32 dönem | conviction.rs:108 |
| CONVICTION_VOTING_ID | b"pyconvot" | lib.rs:67 |
| Delegation max depth | 1 | lib.rs:573-577 |
| XCM MIN_XCM_VERSION | 3 | xcm/src/lib.rs:533 |
| XCM MAX_XCM_DECODE_DEPTH | 8 | xcm/src/lib.rs:55 |
| XCM MAX_INSTRUCTIONS_TO_DECODE | 100 | xcm/src/lib.rs:59 |
| XCM MAX_ITEMS_IN_ASSETS | 20 | xcm/src/v5/asset.rs:556 |
| XCM RECURSION_LIMIT | 10 | xcm-executor/src/lib.rs:72 |
| Identity deposit | BasicDeposit + ByteDeposit * size | lib.rs:1490-1494 |
| Multisig deposit | DepositBase + DepositFactor * threshold | lib.rs |
| Pure proxy address | blake2_256(prefix ++ who ++ height ++ ext_index...) | lib.rs |
| Vesting formülü | locked - ((n-start) * per_block) | vesting_info.rs:88-101 |
| Bounty curator deposit | clamp(CuratorDepositMult * fee, Min, Max) | lib.rs:1048-1060 |
| Approval additive BUG | approve_transfer = saturation_add (ERC-20'den farklı) | assets/lib.rs:662 |
| TransferredApproved event | ASLA emit edilmiyor | assets/lib.rs:662 |

---

## N. KRİTİK GÜVENLİK NOTLARI

### N.1 Doğrulanmış Buglar (NSC İçin Dikkat)

1. **pallet-assets Approval Additive (BUG-07):** `approve_transfer` mevcut approval'a EKLER, reset etmez. NSC token sisteminde: `approve(0)` → `approve(amount)` iki adım zorunlu.

2. **AMM TOCTOU (BUG-02, SDK Issue #1698):** Aynı blokta reserve manipülasyonu. Çözüm: `with_transaction` + öncesi/sonrası reserve kontrolü.

3. **pallet-recovery cancel_delay=0 (BUG-08):** Sıfır delay = anında recovery başarılı. NSC recovery: `cancel_delay >= 1` zorunlu.

4. **RPC storage_keys OOM (BUG-03):** Tüm sonuçları belleğe toplar. NSC RPC: pagination zorunlu.

5. **pallet-scheduler Periodic Task Kaybı (BUG-06):** Agenda dolu ise periyodik görev sessizce silinir. NSC deadman switch: scheduler yerine custom storage kullanın.

### N.2 NSC'ye Özgü Güvenlik Önerileri

1. **ZK Merkle tree tutarsızlığı (bug #17):** İki şema uyumsuz. Penumbra'dan öğrenildiği üzere tek TCT kullanın (2^48 kapasite).

2. **Eon key uyumsuzluğu (bug #19):** secp256k1[33] vs IBE Ristretto255[32]. IBE için sr25519/Ristretto255 kullanın, secp256k1 değil.

3. **TX hash içerik bağımsız (bug #5):** `to` ve `amount` hash'e dahil değil. Acilen düzeltin — replay attack vektörü.

4. **TX imzası doğrulanmıyor (bug #4):** Tüm işlemler imzasız geçiyor. Polkadot'un `CheckedExtrinsic` ve `UncheckedExtrinsic` modelini uygulayın.

5. **EVM function_selector BLAKE3 (bug #12):** Keccak256 olmalı. EVM toolchain uyumluluğu için kritik.

---

## O. RUST CRATE ÖNERİLERİ

```toml
[dependencies]
# Aritmetik (kesinlikle)
sp-arithmetic = { version = "28", default-features = false }
# Weight sistemi
sp-weights = { version = "27", default-features = false }
# NPoS elections (Phragmén)
sp-npos-elections = { version = "26", default-features = false }
# Staking primitives
sp-staking = { version = "26", default-features = false }
# Consensus primitives
sp-consensus-babe = { version = "0.34", default-features = false }
sp-consensus-grandpa = { version = "13", default-features = false }
# XCM
xcm = { package = "staging-xcm", version = "14", default-features = false }
xcm-executor = { package = "staging-xcm-executor", version = "14", default-features = false }
# Runtime frame
frame-support = { version = "37", default-features = false }
frame-system = { version = "37", default-features = false }
# Specific pallets
pallet-conviction-voting = { version = "19", default-features = false }
pallet-referenda = { version = "19", default-features = false }
pallet-nomination-pools = { version = "26", default-features = false }
pallet-asset-conversion = { version = "20", default-features = false }
pallet-safe-mode = { version = "18", default-features = false }
pallet-tx-pause = { version = "18", default-features = false }
pallet-migrations = { version = "3", default-features = false }
# VRF için
schnorrkel = { version = "0.11", default-features = false }
```

---

## P. ÖNCELIK SIRASI (NSC İçin Aksiyon Planı)

### Kritik (Hemen Düzeltilmeli)
1. **TX hash içerik bağımsız (bug #5)** → `to + amount + nonce + chain_id` hash'e girmeli
2. **TX imzası doğrulanmıyor (bug #4)** → UncheckedExtrinsic → CheckedExtrinsic pipeline
3. **VRF stub (bug #3)** → sr25519 VRF gerçek implementasyon
4. **Epoch çakışması (bug #6)** → 2400 vs 216_000 birleştir

### Yüksek Öncelik (1-2 Ay)
5. **Fee sistemi** → 2D weight + TargetedFeeAdjustment multiplier
6. **Conviction voting** → pallet-conviction-voting modelini al
7. **Phragmén seçimi** → sp_npos_elections::seq_phragmen
8. **GRANDPA timeout logic** → prevote/precommit timeout ekleme
9. **Slash dinamik formül** → `min((3k/n)^2, 1)`

### Orta Öncelik (3-6 Ay)
10. **Nomination pools** → Guardian delegasyon sistemi
11. **OpenGov tracks** → pallet-referenda ile conviction voting entegrasyonu
12. **pallet-safe-mode benzeri** → freeze emergency sistemi
13. **Treasury harcama** → spend + Paymaster API
14. **XCM limit güncelleme** → MAX_INSTRUCTIONS=100, DEPTH=8
15. **Runtime migration** → pallet-migrations multi-block framework

---

*Belge Kaynağı: research/analysis/polkadot/ (30 agent, 157k satır Polkadot SDK analizi)*
*Tüm değerler DOGRULAMA-batch1/2/3.md ile teyit edilmiştir*
*Nova Shield mevcut durum referansı: research/tokenomics/NOVA_SHIELD_CONTEXT.md*
