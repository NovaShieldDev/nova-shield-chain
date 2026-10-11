# Nova Shield Chain — Solana Araştırma Raporu
# Solana Kaynak Kodundan Nova Shield İçin Çıkarılan Dersler
**Tarih:** 2026-07-22
**Kaynak:** research/analysis/solana/ (agent-01 ... agent-30-MASTER-SENTEZ)

---

## İÇİNDEKİLER

- [A. TOKEN EKONOMİSİ](#a-token-ekonomisi)
- [B. KONSENSÜS VE SLOT MEKANİZMASI](#b-konsensüs-ve-slot-mekanizması)
- [C. GULF STREAM](#c-gulf-stream)
- [D. TURBİNE VE SHRED](#d-turbine-ve-shred)
- [E. QoS SİSTEMİ](#e-qos-sistemi)
- [F. APPEND VEC VE DEPOLAMA](#f-append-vec-ve-depolama)
- [G. AĞIRLIKLI STAKE VE VALİDATOR EKONOMİSİ](#g-ağırlıklı-stake-ve-validator-ekonomisi)
- [H. GÜVENLİK ÖZELLİKLERİ](#h-güvenlik-özellikleri)
- [I. NOVA SHIELD'E ÖZEL UYARLAMA ÖNERİLERİ](#i-nova-shielde-özel-uyarlama-önerileri)

---

## A. TOKEN EKONOMİSİ

### A.1 Solana Enflasyon Modeli

**Kaynak:** agent-03.md, agent-24.md

Solana'nın enflasyon yapısı `sdk/program/src/epoch_schedule.rs` ve `sdk/src/inflation.rs` içinde tanımlanmıştır.

**Temel parametreler (kaynaktan doğrulanmış):**
```
initial: f64          // Başlangıç yıllık enflasyon oranı (örn: 0.08 = %8)
terminal: f64         // Hedef minimum oran (örn: 0.015 = %1.5)
taper: f64            // Her yıl azalma çarpanı (örn: 0.15 = %15 azalış)
foundation: f64       // Foundation payı (şu an 0.0)
foundation_term: f64  // Foundation dönem süresi (yıl)
```

**Yıllık enflasyon formülü:**
```
year_rate(year) = terminal + (initial - terminal) * (1 - taper)^year
```

**DEFAULT_BURN_PERCENT = 50:** Her slot fee'sinin %50'si yakılır, %50 validator'a gider.
Bu oran `fee_governor.rs` içindeki "target_lamports_per_signature" üzerinden dinamik olarak ayarlanır.

**Fee Governor (ücret yönetimi) parametreleri:**
```
fee_rate_governor.target_signatures_per_slot       // Hedef imza/slot
fee_rate_governor.target_lamports_per_signature    // Hedef ücret (lamport)
fee_rate_governor.min_lamports_per_signature       // Alt sınır
fee_rate_governor.max_lamports_per_signature       // Üst sınır
fee_rate_governor.burn_percent: u8 = 50            // Yakma oranı
```

**Dinamik ücret algoritması (fee_calculator.rs):**
```
// Step = %5 (adım büyüklüğü)
// Min = hedefin %50'si
// Max = hedefin 1000%'i
if actual_signatures > target * 1.05:
    new_fee = current_fee * 1.05
elif actual_signatures < target * 0.95:
    new_fee = current_fee * 0.95
new_fee = clamp(new_fee, min, max)
```

**Epoch ödülü dağıtımı (agent-03.md):**
```rust
// EpochRewards sysvari
pub struct EpochRewards {
    pub distribution_starting_block_height: u64,
    pub num_partitions: u64,
    pub parent_blockhash: Hash,
    pub total_points: u128,
    pub total_rewards: u64,
    pub distributed_rewards: u64,
    pub active: bool,
}
// Staking reward formülü:
// points_i = stake_i × earned_credits_i   (u128, taşma yok)
// reward_i = (points_i × total_rewards) / total_points
```

### A.2 Nova Shield İçin Token Ekonomisi Uyarlaması

Nova Shield'ın mevcut emisyon formülü: `rate_bps = max(800 - year * 50, 200)`

**Solana'dan öğrenilecek:**

1. **Sabit taper yerine üstel azalma:** Solana `(1 - taper)^year` kullanıyor. Nova Shield lineer azalma kullanıyor (`800 - year * 50`). Üstel azalma daha yumuşak ve öngörülebilir supply eğrisi üretir.

2. **Burn mekanizması kesinleştirmesi:** Solana `DEFAULT_BURN_PERCENT = 50` sabit. Nova Shield fee dağılımı (50% burn + 50% validator) Solana ile örtüşüyor — bu parametrenin governance ile değiştirilebilir yapılması önerilir.

3. **EpochRewards u128 kullanımı:** Points hesabında `stake × credits` çarpımı u64 taşabilir. Nova Shield'da da `reward_points: u128` zorunlu. (Mevcut kodda `Dec::mul` hassasiyet hatası — Bug #15 ile bağlantılı)

4. **NSC Fee Governor uyarlaması:**
```rust
// nova-runtime/src/token/fee_dist.rs içinde önerilen yapı
pub struct FeeGovernor {
    pub target_tx_per_slot: u64,
    pub current_base_fee: u128,      // 10^18 cinsinden
    pub burn_percent: u8,            // default: 50
    pub step_bps: u16,               // 500 = %5
    pub min_fee: u128,
    pub max_fee: u128,
}

impl FeeGovernor {
    pub fn update(&mut self, actual_tx: u64) {
        let target = self.target_tx_per_slot;
        if actual_tx > target * 105 / 100 {
            self.current_base_fee = self.current_base_fee
                .saturating_mul(10_500).saturating_div(10_000);
        } else if actual_tx < target * 95 / 100 {
            self.current_base_fee = self.current_base_fee
                .saturating_mul(9_500).saturating_div(10_000);
        }
        self.current_base_fee = self.current_base_fee
            .clamp(self.min_fee, self.max_fee);
    }
}
```

**Kullanılacak Rust crate'leri:**
- `rust_decimal` veya sabit noktalı `u128` aritmetiği (no-std uyumlu)
- `serde` + `borsh` doğruluğu için

---

## B. KONSENSÜS VE SLOT MEKANİZMASI

### B.1 Proof of History (PoH)

**Kaynak:** agent-20.md, agent-03.md, agent-14.md

**PoH temel yapısı:**
```
H_n = SHA256(H_{n-1})          // Sıradan tick
H_new = SHA256(H_prev || mixin) // TX kaydı (mixin = TX hash)
```

**Slot zamanlaması (kaynaktan doğrulanmış sabitler):**
```
DEFAULT_TICKS_PER_SECOND = 160
DEFAULT_MS_PER_SLOT      = 400    // 400ms slot süresi
DEFAULT_TICKS_PER_SLOT   = 64
DEFAULT_HASHES_PER_TICK  = 12,500  // SHA-256 op/tick
Toplam: 64 × 12,500 = 800,000 SHA-256 op/slot
```

**PohService implementasyonu (agent-20.md):**
```rust
pub struct Poh {
    pub hash: Hash,
    num_hashes: u64,
    hashes_per_tick: u64,
    remaining_hashes: u64,
    tick_number: u64,
    slot_start_time: Instant,
    target_poh_time: Option<Instant>,
}

// Kritik: busy-wait kullanılıyor, thread::sleep() KULLANILMIYOR
// Jitter azaltmak için CPU pinleme zorunlu
loop {
    poh.hash();  // H_n = SHA256(H_{n-1})
    if poh.remaining_hashes == 0 { tick(); }
}
```

**DEFAULT_HASHES_PER_BATCH = 64:** Her batch'te 64 hash hesaplanır, sonra kontrol yapılır.
**TARGET_SLOT_ADJUSTMENT_NS = 50ms:** Slot süre sapmasına tepki için nano-saniye hassasiyetli ayarlama.
**GRACE_TICKS_FACTOR = 2:** Leader yavaş başlarsa 2 tick grace süresi tanınır.

### B.2 Tower BFT

**Kaynak:** agent-07.md, agent-13.md, agent-14.md

**Temel sabitler (kaynaktan doğrulanmış):**
```
VOTE_THRESHOLD_DEPTH       = 8    // Finalize için kaç blok derinlik
VOTE_THRESHOLD_DEPTH_SHALLOW = 4  // Alternatif fork değerlendirme
SWITCH_FORK_THRESHOLD      = 0.38 // Fork değiştirmek için gerekli stake oranı
MAX_LOCKOUT_HISTORY        = 31   // Maksimum oylama geçmişi
SUPERMINORITY_THRESHOLD    = 1/3  // Küçük azınlık eşiği
```

**Lockout mekanizması:**
```
// N. oy için lockout = 2^N slot (maksimum 2^31 slot ≈ 1 yıl)
// Slot k'da verilen oy, en az 2^(depth) slot kilitlenir
lockout(vote_at_depth_d) = 2^d
```

**Fork değiştirme kuralı (SWITCH_FORK_THRESHOLD = 0.38):**
```
// Aktif fork'tan farklı forks üzerindeki toplam stake
// %38'i geçmedikçe fork değiştirme yasak
// Güvenlik: 1/3 Byzantine + buffer
switch_allowed = (other_fork_stake / total_stake) >= 0.38
```

**ReplayStage kritik eşikleri (agent-13.md):**
```
DUPLICATE_LIVENESS_THRESHOLD = 0.10  // %10 stake çift blok → aksiyon
MAX_CONCURRENT_FORKS_TO_REPLAY = 4   // Aynı anda max fork sayısı
```

**Epoch schedule (agent-03.md):**
```
DEFAULT_SLOTS_PER_EPOCH = 432,000    // Solana
// Nova Shield: 216,000 slot/epoch (24 saat, 400ms slot)
// Warmup: first_normal_epoch = log2(slots_per_epoch) epochs
// Consensus epoch = 2400 slot (Bug #6) — 216,000 olmalı!
```

**Leader schedule:**
```
NUM_CONSECUTIVE_LEADER_SLOTS = 4     // Her leader art arda 4 slot üretir
MAX_LEADER_SCHEDULE_EPOCH_OFFSET = 3 // 3 epoch önceden program yapılır
// Seçim: stake-weighted VRF lottery
```

### B.3 Nova Shield Konsensüs Uyarlaması

Nova Shield 3 motor kullanıyor: Simplex BFT + GRANDPA + Snowball.

**Solana'dan alınacak kesin değerler:**

1. **PoH doğrudan adapte edilebilir:** 64 tick × 12,500 hash = 800,000 SHA-256/slot. Nova Shield 400ms slot hedefliyorsa bu değerler aynen uygulanabilir. Tek değişiklik: Nova SHA-256 yerine BLAKE3 kullanabilir (3x daha hızlı).

2. **Tower BFT → Nova Simplex BFT entegrasyonu:**
   - `VOTE_THRESHOLD_DEPTH = 8` → Simplex BFT'de quorum sayısına dönüştür: (2n+2)/3 = 134/200
   - `SWITCH_FORK_THRESHOLD = 0.38` değerini Snowball parametreleriyle kombinle
   - `MAX_LOCKOUT_HISTORY = 31` → Nova'da maksimum finalize gecikmesi

3. **Bug #6 düzeltmesi (Konsensüs epoch çakışması):**
```rust
// nova-consensus/src/lib.rs içinde:
// YANLIŞ: const EPOCH_SLOTS: u64 = 2_400;
// DOĞRU:
const EPOCH_SLOTS: u64 = 216_000;  // 24 saat × 3600s / 0.4s
const TICKS_PER_SLOT: u64 = 64;
const HASHES_PER_TICK: u64 = 12_500;  // SHA-256 veya BLAKE3
```

4. **GRANDPA finalite için Solana dersi:** Solana Tower BFT lockout mekanizması GRANDPA'ya benzer. `VOTE_THRESHOLD_DEPTH=8` GRANDPA'nın 2 round'una (prevote + precommit) karşılık gelir.

**Kullanılacak Rust crate'leri:**
- `sha2` (SHA-256, PoH için)
- `blake3` (alternatif, daha hızlı)
- `rayon` (paralel blok doğrulama)
- `crossbeam-channel` (PoH → BankingStage pipeline)

---

## C. GULF STREAM

### C.1 Solana Gulf Stream Mekanizması

**Kaynak:** agent-11.md, agent-12.md, agent-29.md

Gulf Stream, TX'lerin lider olmadan önce forwarding'ini sağlayan mempool-less yaklaşımdır.

**Temel parametreler:**
```
MAX_PROCESSING_AGE = 150 slot    // TX bu kadar eskirse drop edilir
// 150 × 400ms = 60 saniye maksimum TX ömrü
TOTAL_BUFFERED_PACKETS = 700,000 // Toplam buffer kapasitesi
TARGET_NUM_TRANSACTIONS_PER_BATCH = 64  // Paralel execute batch
NUM_VOTE_PROCESSING_THREADS = 2  // Oy TX'leri için özel thread
```

**Pipeline aşamaları (agent-11.md):**
```
QUIC/UDP → SigVerify → CostModel → AccountLock → Execute → PoH Record → Commit
     ↓           ↓           ↓            ↓
  Drop:       Drop:       Drop:        Drop:
  invalid   too_cheap   block_full  account_busy
```

**Leader forwardlama (Gulf Stream çekirdeği):**
```rust
// Mevcut lider değilsen, sonraki lidere TX forward et
// Lider programı 3 epoch önceden bilinir
fn forward_to_leader(tx: &Transaction, leader: &Pubkey) {
    // QUIC üzerinden lider TPU portuna gönder
    // Stake-weighted bağlantı önceliği
}
```

**QUIC parametreleri (agent-29.md):**
```
MAX_QUIC_CONNECTIONS_PER_PEER = 8     // Peer başına max bağlantı
// Stake-weighted rate limiting:
// stake_weight = validator_stake / total_stake
// max_streams = base_streams * stake_weight^0.5
```

**BankingStage detayları (agent-11.md):**
```
PrioGraphScheduler: Bağımlılık grafiği (hangi TX aynı account'a yazıyor?)
  → Bağımsız TX'ler paralel execute edilir
  → Çakışanlar sıralanır

Legacy scheduler: thread-per-type (vote/non-vote ayrımı)

// Kritik: Vote TX'leri asla drop edilmez
// Non-vote TX'ler fee önceliğine göre sıralanır
```

### C.2 Nova Shield Gulf Stream Uyarlaması

Nova Shield mempool'u (`nova-mempool`) threshold ECDH MEV koruması kullanıyor. Gulf Stream'in Nova'ya adaptasyonu:

**1. MAX_PROCESSING_AGE analogu:**
```rust
// nova-mempool/src/lib.rs
const MAX_TX_AGE_SLOTS: u64 = 150;  // Solana'dan alınan değer
// Nova 400ms slot → 60 saniye maksimum TX ömrü
// Guardian recovery TX'leri için daha uzun süre?
// Öneri: Normal TX = 150 slot, Guardian TX = 432,000 slot (30 gün)
```

**2. MEV koruması + Gulf Stream kombinasyonu:**
Nova Shield IBE şifreli emirler kullanıyor. Gulf Stream ile kombinasyon:
```
TX şifreli gönderilir (IBE ile)
→ Lider programına göre TPU hedefi seçilir
→ Lider slot başında eon key ile şifre çözer
→ Batch auction clearing fiyatı belirlenir
```

**3. IBE Eon Key dağıtımı (Rolling Shutter pattern):**
```rust
// nova-mempool içinde mevcut, iyileştirilmeli:
// Sorun: eon_key secp256k1[33] vs IBE Ristretto255[32] uyumsuzluk (Bug #19)
// Çözüm: Eon key = Ristretto255 point (32 byte, sıkıştırılmış)
pub struct EonKey {
    pub epoch: u64,
    pub public_key: [u8; 32],  // Ristretto255, NOT secp256k1
    pub threshold_shares: Vec<KeyShare>,
}
```

**4. Forward buffer yapısı:**
```rust
// Solana'nın TOTAL_BUFFERED_PACKETS = 700,000 değerinden ilham
// Nova için: güvenlik TX'leri ayrı öncelik kuyruğuna
pub struct NovaMempool {
    pub guardian_queue: BoundedVec<Tx, 10_000>,   // Yüksek öncelik
    pub freeze_queue: BoundedVec<Tx, 5_000>,       // Kritik öncelik
    pub auction_queue: BoundedVec<Tx, 100_000>,    // DEX emirleri
    pub normal_queue: BoundedVec<Tx, 585_000>,     // Normal TX
    // Toplam: 700,000
}
```

**Kullanılacak Rust crate'leri:**
- `quinn` (QUIC implementasyonu, Solana'nın da kullandığı)
- `tokio` (async pipeline)
- `crossbeam-queue` (lock-free bounded queue)
- `priority-queue` (fee-weighted ordering)

---

## D. TURBİNE VE SHRED

### D.1 Solana Turbine Mekanizması

**Kaynak:** agent-17.md, agent-20.md, agent-05.md

Turbine, blok yayılımı için Merkle shred formatı + Reed-Solomon erasure coding kullanır.

**Temel sabitler (kaynaktan doğrulanmış):**
```
SHRED_DATA_PAYLOAD_SIZE    = 1,232 byte  // MTU uyumlu (1280 - IPv6/UDP header)
DATA_PLANE_FANOUT          = 200         // Turbine tree fan-out
// Layer 0: lider → 200 node
// Layer 1: her node → 200 node (200^2 = 40,000 node erişim)
```

**Shred yapısı (Merkle format, agent-17.md):**
```
ShredType::Data:
  - 1,232 byte payload
  - Merkle proof (ağaç yüksekliğine göre değişen)
  - Ed25519 imzası (lider imzalar)

ShredType::Code (Reed-Solomon):
  - Erasure parçası
  - 32 veri shred + 32 kod shred (64 toplam)
  - Herhangi 32 shred → tam blok kurtarılabilir
```

**Merkle shred avantajı:**
```
// Legacy: Her shred ayrı imzalanırdı (pahalı)
// Merkle: Sadece Merkle root imzalanır, proof ile doğrulama
// 1 imza → tüm shred kümesi doğrulanabilir
```

**Erasure coding parametreleri:**
```
num_data_shreds = 32
num_coding_shreds = 32
// Fault tolerance: 50% kayıp tolerans
// Reed-Solomon: GF(2^8) üzerinde, ferris-math/reed-solomon-erasure crate
```

**Turbine tree oluşturma:**
```
// Stake-weighted sıralama: büyük stake → ağacın üst katmanlarında
// Kötü aktör ağacın dibinde kalır → propagation etkisi azalır
fn build_turbine_tree(validators: &[ValidatorStake]) -> TurbineTree {
    let sorted = validators.sort_by(|a, b| b.stake.cmp(&a.stake));
    // Layer 0: lider (kendisi)
    // Layer 1: sorted[0..fanout]
    // Layer 2: sorted[fanout..fanout^2]
}
```

### D.2 Nova Shield Turbine Uyarlaması

Nova Shield `nova-network` tamamen mock (libp2p yok, Reed-Solomon XOR mock — Bug #13).

**Bug #13 düzeltmesi (XOR mock → gerçek Reed-Solomon):**
```rust
// Mevcut YANLIŞ (nova-network/src/erasure.rs):
// fn encode(data: &[u8]) -> Vec<u8> { data.iter().fold(0, |a, b| a ^ b) }

// DOĞRU (reed-solomon-erasure crate ile):
use reed_solomon_erasure::galois_8::ReedSolomon;

pub struct NovaErasureCoder {
    rs: ReedSolomon,
    num_data: usize,
    num_parity: usize,
}

impl NovaErasureCoder {
    pub fn new() -> Self {
        Self {
            rs: ReedSolomon::new(32, 32).unwrap(),  // Solana değerleri
            num_data: 32,
            num_parity: 32,
        }
    }

    pub fn encode(&self, data: &mut Vec<Vec<u8>>) -> Result<(), Error> {
        self.rs.encode(data).map_err(Into::into)
    }

    pub fn reconstruct(&self, shards: &mut Vec<Option<Vec<u8>>>) -> Result<(), Error> {
        self.rs.reconstruct(shards).map_err(Into::into)
    }
}
```

**Shred boyut hesabı:**
```
SHRED_DATA_PAYLOAD_SIZE = 1,232 byte (MTU-safe)
// IPv6 MTU = 1,280 byte
// IPv6 header = 40 byte, UDP header = 8 byte
// 1,280 - 40 - 8 = 1,232 byte
// Nova da aynı değeri kullanabilir (P2P plan: libp2p + Noise XX)
```

**Turbine ağacı (DATA_PLANE_FANOUT = 200):**
```rust
// 200 validator için:
// Layer 0: lider
// Layer 1: 200 node → tüm ağ!
// Nova 200 validator ile Layer 1 yeterli (Layer 2 gerekmez)
// Fanout = sqrt(200) ≈ 14 optimize edilebilir
pub const NOVA_TURBINE_FANOUT: usize = 14;  // 200 validator için yeterli
```

**Kullanılacak Rust crate'leri:**
- `reed-solomon-erasure` (galois_8 feature ile, Solana da bunu kullanır)
- `libp2p` (Noise XX transport, UDP üzerinde)
- `merkle-cbt` veya custom Merkle tree (shred signing için)

---

## E. QoS SİSTEMİ

### E.1 Solana Cost Model ve QoS

**Kaynak:** agent-08.md, agent-11.md, agent-28.md

**Temel CU maliyetleri (kaynaktan doğrulanmış):**
```
SIGNATURE_COST                = 720 CU   // Her imza doğrulama
SECP256K1_VERIFY_COST         = 720 CU   // secp256k1 imza
ED25519_VERIFY_COST           = 720 CU   // Ed25519 imza
WRITE_LOCK_UNITS              = 300 CU   // Yazma kilidi başına
MAX_COMPUTE_UNIT_LIMIT        = 1,400,000 CU/blok
// TX başına max:
MAX_COMPUTE_UNIT_LIMIT_TX     = 200,000 CU (default)
```

**Program CU maliyetleri:**
```
System Program               = 150 CU
Vote Program                 = 2,100 CU
Stake Program                = 750 CU
Address Lookup Table         = 750 CU
ComputeBudget Program        = 150 CU
LoaderV4 (BPF deploy)        = 2,000 CU
```

**Blok doldurma sınırları (agent-28.md):**
```
MAX_BLOCK_REPLAY_TIME_US     = 400,000 μs  // 400ms (1 slot)
COMPUTE_UNIT_TO_US_RATIO     = 30          // 30 CU = 1 μs işlem süresi
MAX_CONCURRENCY              = 4           // Paralel TX akışı
```

**Stake-weighted QoS (agent-12.md, agent-29.md):**
```
// QUIC bağlantı sayısı stake'e orantılı
max_streams_per_peer = base × (stake/total_stake)^0.5
// Yüksek stake → daha fazla TX teslim kapasitesi
// DoS koruması: düşük stake → sınırlı bant genişliği
```

**Priority fee mekanizması:**
```
// TX başına ek ücret (CU başına lamport)
ComputeBudgetInstruction::set_compute_unit_price(priority_fee: u64)
// Toplam priority fee = priority_fee × requested_CU
// Block dolduğunda düşük priority TX'ler drop edilir
```

### E.2 Nova Shield QoS Tasarımı

Nova Shield'ın `nova-mempool` ve transaction tipine göre QoS sistemi tasarlanmalıdır.

**Önerilen Nova CU sistemi:**
```rust
// nova-runtime/src/cost_model.rs (yeni oluşturulacak)

pub const NSC_DECIMALS: u128 = 1_000_000_000_000_000_000; // 10^18

// Nova Gas birimleri (NG = Nova Gas)
pub const SIGNATURE_COST_NG: u64 = 720;        // Ed25519/Schnorr imza
pub const FROST_SIGNATURE_COST_NG: u64 = 7_200; // FROST threshold: 10x
pub const ZK_PROOF_VERIFY_NG: u64 = 200_000;   // Full ZK proof (FullShield)
pub const ZK_SEMI_VERIFY_NG: u64 = 80_000;     // SemiShield
pub const STANDARD_TX_NG: u64 = 21_000;        // Standard (Ethereum benzeri)
pub const GUARDIAN_OP_NG: u64 = 50_000;        // Guardian update
pub const FREEZE_OP_NG: u64 = 10_000;          // Hesap dondurma (öncelikli)
pub const INHERITANCE_OP_NG: u64 = 30_000;     // Miras planı
pub const DEX_ORDER_NG: u64 = 50_000;          // Batch auction emri

// Blok sınırı
pub const MAX_BLOCK_NG: u64 = 1_400_000;       // Solana'dan alınan değer
pub const MAX_TX_NG: u64 = 200_000;
```

**TX öncelik sıralaması:**
```
Öncelik 1 (asla drop edilmez): Freeze TX, Guardian emergency
Öncelik 2 (çok yüksek):        Heartbeat TX, Recovery TX
Öncelik 3 (yüksek):            Deadman switch, Inheritance
Öncelik 4 (normal):            DEX emirleri, transfer
Öncelik 5 (düşük):             ZK shield opsiyonel
```

**Stake-weighted bağlantı (Solana'dan adapte):**
```rust
// nova-mempool/src/qos.rs
pub fn max_connections_for_peer(peer_stake: u64, total_stake: u64) -> u32 {
    const BASE_CONNECTIONS: u32 = 2;
    const MAX_CONNECTIONS: u32 = 8;  // Solana MAX_QUIC_CONNECTIONS_PER_PEER
    let weight = (peer_stake as f64 / total_stake as f64).sqrt();
    ((BASE_CONNECTIONS as f64 + weight * (MAX_CONNECTIONS - BASE_CONNECTIONS) as f64) as u32)
        .min(MAX_CONNECTIONS)
}
```

---

## F. APPEND VEC VE DEPOLAMA

### F.1 Solana AppendVec Mimarisi

**Kaynak:** agent-09.md, agent-10.md, agent-22.md

**AppendVec temel prensipleri:**
```
- Append-only memory-mapped dosya (mmap)
- Hesap verisi slot yaşam döngüsüne göre depolanır
- Hiçbir zaman "yerinde güncelleme" yapılmaz → yeni slot → yeni kayıt
- Eski slotlar GC (accounts background service) tarafından temizlenir
```

**AccountsDB yapısı (agent-09.md):**
```
AccountsDB
├── AppendVec[]     (fiziksel depolama, mmap)
├── AccountsIndex   (BucketMap ile index, bellek + disk)
│   ├── slot_list: Vec<(Slot, AccountInfo)>
│   └── ref_count: AtomicU64
└── AncientAppendVecs (eski slotlar → birleştirilmiş)
```

**Bank yaşam döngüsü (agent-10.md):**
```
Bank::new()    → open (TX kabul eder)
Bank::freeze() → frozen (sadece okuma)
Bank::root()   → root (snapshot alınabilir, GC hedefi)
// Her slot = bir Bank instance
// Parent-child ilişkisi: fork için ayrı Bank
// DashMap<Slot, Arc<Bank>>: fork tracking
```

**Snapshot sistemi (agent-22.md):**
```
Full snapshot:        Tüm hesap durumu (tar.zst veya tar.lz4)
Incremental snapshot: Son full'dan bu yana değişenler
// Hash doğrulama: AccountsHash (paralel, rayon)
// Hız: lz4 > zstd (küçük dosyalar için), zstd > lz4 (büyük dosyalar)
```

**BucketMap (accounts index, agent-25.md):**
```
// Sabit boyutlu bucket dizisi (disk üzerinde)
// Key: Pubkey (32 byte)
// Value: Vec<(Slot, AccountInfo)>
// Eski değerler silinmez → sadece yeni eklenir (log-structured)
```

### F.2 Nova Shield Depolama Tasarımı

Nova Shield hesap modelinde Guardian, Freeze durumu, Deadman switch gibi ek metadata var.

**Önerilen depolama katmanları:**
```rust
// nova-runtime/src/storage/append_vec.rs

pub struct NovaAppendVec {
    mmap: memmap2::MmapMut,
    current_offset: AtomicU64,
    capacity: u64,
}

impl NovaAppendVec {
    pub fn append(&self, account: &NovaAccount) -> Option<u64> {
        let data = borsh::to_vec(account).ok()?;
        let len = data.len() as u64 + 8; // 8 byte length prefix
        let offset = self.current_offset.fetch_add(len, Ordering::SeqCst);
        if offset + len > self.capacity { return None; } // Dolu
        // mmap'e yaz
        Some(offset)
    }
}
```

**Nova hesap yapısı (genişletilmiş):**
```rust
// Nova hesapları Solana'dan daha karmaşık — extra alanlar
pub struct NovaAccount {
    // Temel alanlar (Solana benzeri)
    pub lamports: u128,          // NSC 10^18, u64 yetmez!
    pub owner: [u8; 32],
    pub executable: bool,
    pub rent_epoch: u64,
    pub data: Vec<u8>,
    // Nova-spesifik alanlar
    pub freeze_state: FreezeState,
    pub guardian_set: Option<GuardianSetRef>,
    pub deadman_config: Option<DeadmanRef>,
    pub zk_note_commitment: Option<[u8; 32]>,
}
```

**Snapshot formatı:**
```toml
# Nova için önerilen snapshot yapısı
[snapshot]
format = "tar.zst"          # Solana: tar.zst veya tar.lz4
full_interval = 25_000      # slot başına (Solana benzeri)
incremental_interval = 100  # slot başına
hash_algo = "sha256"        # AccountsHash için
parallel_threads = 8        # rayon ile paralel hash
```

**Kullanılacak Rust crate'leri:**
- `memmap2` (memory-mapped dosya, Solana'nın kullandığı)
- `rocksdb` (index için, mevcut karar zaten bu)
- `zstd` (snapshot sıkıştırma)
- `lz4` (hızlı sıkıştırma alternatifi)
- `rayon` (paralel AccountsHash)
- `borsh` (hesap serializasyon)

---

## G. AĞIRLIKLI STAKE VE VALİDATOR EKONOMİSİ

### G.1 Solana Validator Ekonomisi

**Kaynak:** agent-07.md, agent-08.md, agent-21.md, agent-23.md

**Staking reward formülü (kaynaktan doğrulanmış):**
```
points_i = stake_i × earned_credits_i   // u128 (taşma yok!)
reward_i = (points_i × total_epoch_rewards) / total_points

// "earned_credits": validator'ın oylarının ne kadarı karar vericiydi
// Daha erken oy → daha yüksek credits
// vote_latency = current_slot - vote_slot (server-side hesaplanır, manipüle edilemez)
```

**Warmup/cooldown mekanizması (agent-08.md):**
```
warmup_cooldown_rate = 0.25  // Epoch başına maksimum %25 stake değişimi
// Stake artışı: yeni_aktif <= önceki_aktif × 1.25
// Stake azalışı: yeni_aktif >= önceki_aktif × 0.75
// Amaç: ani stake giriş/çıkışlarına karşı ağ stabilitesi
```

**Slashing (Solana'da minimal, Nova için kritik):**
```
// Solana'da resmi slashing YOK (sosyal slashing)
// Nova Shield slashing (mevcut tasarım):
double_sign:      %100 (kalıcı tombstone)
mempool_leak:     %50
wrong_batch:      %25
extended_offline: %1
```

**Stake ağırlıklı lider seçimi:**
```rust
// VRF tabanlı seçim (Solana benzeri)
fn select_leader(validators: &[ValidatorInfo], epoch: u64, slot: u64) -> Pubkey {
    // Her validator'a stake_weight orantılı slot tahsis edilir
    // 4 ardışık slot aynı lider (NUM_CONSECUTIVE_LEADER_SLOTS = 4)
    let weighted_schedule = build_weighted_schedule(validators, epoch);
    weighted_schedule[slot % SLOTS_PER_EPOCH]
}
```

**Komisyon güncelleme kısıtı (agent-07.md):**
```rust
// Solana: komisyon sadece epoch'un ilk yarısında değiştirilebilir
pub fn is_commission_update_allowed(slot: u64, epoch_info: &EpochInfo) -> bool {
    let relative_slot = slot - epoch_info.first_slot;
    relative_slot * 2 <= epoch_info.slots_per_epoch
    // Amaç: Validator oy toplama döneminde komisyon artıramaz
}
```

**Active account close protection (agent-07.md):**
```
// Stake hesabı kapatıldıktan 2 epoch sonraya kadar başka bir hesap
// aynı adresi kullanamaz (replay saldırısı koruması)
MINIMUM_DEACTIVATION_EPOCHS = 2
```

### G.2 Nova Shield Validator Ekonomisi Tasarımı

**1. NSC reward hesaplama (u128 zorunlu):**
```rust
// nova-runtime/src/staking/reward.rs
pub fn calculate_epoch_rewards(
    validators: &[ValidatorInfo],
    total_epoch_emission: u128,  // NSC 10^18, u128 zorunlu!
) -> Vec<(ValidatorId, u128)> {
    // Total points hesapla
    let total_points: u128 = validators.iter()
        .map(|v| v.active_stake.saturating_mul(v.earned_credits as u128))
        .sum();

    if total_points == 0 { return vec![]; }

    validators.iter().map(|v| {
        let points = v.active_stake.saturating_mul(v.earned_credits as u128);
        // u128 precision: büyük çarpım önce, bölme sonra
        let reward = points
            .saturating_mul(total_epoch_emission)
            .saturating_div(total_points);
        (v.id, reward)
    }).collect()
}
```

**2. Warmup/cooldown Nova'ya adaptasyonu:**
```rust
// nova-runtime/src/staking/mod.rs
pub const STAKE_WARMUP_COOLDOWN_RATE_BPS: u64 = 2_500; // %25 (Solana değeri)

pub fn apply_warmup_cooldown(
    previous_active: u128,
    requested_active: u128,
) -> u128 {
    let max_increase = previous_active
        .saturating_mul(10_000 + STAKE_WARMUP_COOLDOWN_RATE_BPS as u128)
        .saturating_div(10_000);
    let max_decrease = previous_active
        .saturating_mul(10_000 - STAKE_WARMUP_COOLDOWN_RATE_BPS as u128)
        .saturating_div(10_000);
    requested_active.clamp(max_decrease, max_increase)
}
```

**3. Bug #7 düzeltmesi (Staking modül adresi çakışması):**
```rust
// YANLIŞ: Staking modülü [0x53] ve [0x10] iki farklı adreste
// DOĞRU: Tek canonical adres
// Solana'dan ders: Her sistem programı sabit, compile-time id
pub const STAKING_MODULE_ID: [u8; 32] = {
    let mut id = [0u8; 32];
    id[0] = 0x10;  // Canonical adres
    id
};
// [0x53] adresini kaldır, tüm referansları [0x10] yap
```

**4. Guardian için stake teminat parametreleri (validator ekonomisine benzer):**
```
Min guardian stake (Personal):       0 NSC (Nova'nın tasarımı)
Min guardian stake (CertifiedInd):  1,000 NSC
Min guardian stake (Institutional): 10,000 NSC
Min guardian stake (DAO):           5,000 NSC
// Slashing: Guardian hatalı recovery → stake kesilir
// Warmup: Guardian aktivasyon da 1 epoch gecikir
```

**5. Komisyon kısıtı analogu (Guardian ücret değişimi):**
```rust
// Guardian'lar da yalnızca epoch'un ilk yarısında ücret güncelleyebilmeli
pub fn is_guardian_fee_update_allowed(slot: u64, epoch_start: u64, epoch_len: u64) -> bool {
    let relative = slot.saturating_sub(epoch_start);
    relative * 2 <= epoch_len
}
```

---

## H. GÜVENLİK ÖZELLİKLERİ

### H.1 Solana Kriptografik Altyapı

**Kaynak:** agent-04.md, agent-15.md, agent-30-MASTER-SENTEZ.md

**PDA (Program Derived Address) güvenlik kriterleri:**
```rust
// PDA = SHA256(seeds || program_id || "ProgramDerivedAddress")
// ZORUNLU: PDA eğri dışında nokta olmalı (off-curve)
// Saldırı vektörü: seed collision
// hash("abc", "def") == hash("abcdef")  // YANLIŞ
// Güvenli: her seed'i uzunluk prefixli ver
fn create_pda(seeds: &[&[u8]], program_id: &Pubkey) -> Option<Pubkey> {
    // Uzunluk-prefixli seeds ile SHA256
    // Her seed başına 1 byte uzunluk ekle
    let result = hash_with_length_prefix(seeds, program_id, b"ProgramDerivedAddress");
    if is_on_curve(&result) { return None; } // Off-curve kontrolü
    Some(result)
}
```

**Twisted ElGamal (ZK Token, agent-15.md):**
```
Ciphertext = (C, D) where:
  C = x*G + r*H   (commitment: value G + randomness H)
  D = r*P          (encryption key part)
G = Ristretto255 basepoint
H = hash_to_point("ElGamal H") on Ristretto255
P = recipient public key

// Homomorphic addition:
(C1, D1) + (C2, D2) = (C1+C2, D1+D2)
// → Bakiye güncelleme için ZK kanıt gerektirmez

// Anahtar türetimi:
secret_key_bytes = SHA3-512(signing_key.sign(b"ElGamalSecretKey" || seed))
// NOT: signing_key ile doğrudan türetme → donanım cüzdanı uyumlu
```

**Pedersen commitment:**
```
Commitment(v, r) = v*G + r*H
// Binding: v değiştirilemez (G ve H bağımsız)
// Hiding: r rastgele → commitment bilgi sızdırmaz
PEDERSEN_COMMITMENT_LEN = 32 byte (Ristretto255 sıkıştırılmış)
```

**`#[zeroize(drop)]` pattern (agent-15.md):**
```rust
// Tüm gizli anahtar struct'larında zorunlu
use zeroize::ZeroizeOnDrop;

#[derive(ZeroizeOnDrop)]
pub struct ElGamalSecretKey(Scalar);

#[derive(ZeroizeOnDrop)]
pub struct SpendingKey([u8; 32]);
// Drop sırasında bellek sıfırlanır → heap dump saldırısı engellenir
```

### H.2 Güvenlik Açıkları ve Önlemler

**Solana'nın `unsafe` envanteri (agent-30-MASTER-SENTEZ.md):**
```
Yüksek riskli unsafe kullanımları:
1. mmap: AppendVec mmap erişimi → yanlış offset → UB
2. transmute: AccountInfo zero-copy → tip uyumsuzluğu → UB
3. AtomicU64 raw ptr: TOCTOU race condition riski
4. CUDA: GPU sigverify → nvidia driver bug'ı → crash

// Nova Shield için ders: unsafe blokları audit edin
// mmap kullanırken: offset bounds check zorunlu
// transmute yerine: bytemuck::try_from_bytes kullanın
```

**DoS vektörleri (Solana'dan öğrenilen):**
```
1. Hesap şişirme: Küçük bakiyelerde büyük veri → rent ile önlenir
2. Fork bombing: Kasıtlı çatal oluşturma → MAX_CONCURRENT_FORKS = 4
3. Replay attack: Nonce sistemi ile önlenir (durable nonce)
4. Signature DoS: SIGNATURE_COST = 720 CU → ekonomik engel
5. TX flooding: QUIC stake-weighted rate limit → büyük stake öncelikli
```

**CloneZeroed pattern (agent-01.md):**
```rust
// Padding byte'larını sıfırlayan derive macro
// Konsensüs mesajları için kritik: hash tutarlılığı
#[derive(CloneZeroed)]
pub struct VoteState {
    pub validator: Pubkey,
    pub vote_slot: u64,
    // padding: [u8; 3] → otomatik sıfırlanır
}
// Normal Clone: padding byte'lar rastgele → hash farklı → konsensüs bozulur
```

### H.3 Nova Shield ZK Güvenliği

**Mevcut durum:** `nova-zk` tamamen mock (BLAKE3 simülasyon, Bug #17).

**Solana ZK Token'dan alınacak değerler:**
```
ELGAMAL_CIPHERTEXT_LEN     = 64 byte   (C + D, her 32 byte)
PEDERSEN_COMMITMENT_LEN    = 32 byte
RANGE_PROOF_LEN            = 674 byte  (Bulletproofs, 64-bit value için)
// Sigma proof boyutları değişken (transfer, withdrawal...)
```

**Nova ZK Shield implementasyon adımları:**
```
1. arkworks-rs ile Groth16 kurulumu:
   - ark-bn254 (BN254 pairing)
   - ark-groth16 (proving/verifying key)
   - ark-std (no_std uyumlu)

2. Ristretto255 üzerinde Twisted ElGamal:
   - curve25519-dalek (ristretto255)
   - Ciphertext struct: (C: RistrettoPoint, D: RistrettoPoint)

3. Nullifier hesabı (Penumbra pattern):
   - nullifier = PRF(nullifier_key, commitment)
   - PRF: BLAKE2b veya Poseidon hash

4. Not commitment tree:
   - MAX_TREE_SIZE = 2^20 (mevcut) → Penumbra: 2^48
   - Öneri: 2^32 (4 milyar not, makul başlangıç)
```

**Bug #17 düzeltmesi (ZK Merkle tree şema uyumsuzluğu):**
```rust
// İki farklı şema kullanılıyor: tek tip seçin
// Öneri: Incremental Merkle Tree (nova-zk/src/tree.rs)
pub struct IncrementalMerkleTree<const DEPTH: u32> {
    pub frontier: [[u8; 32]; DEPTH as usize + 1],
    pub size: u64,
    pub root: [u8; 32],
}
// Tek şema, tek hasher (Poseidon2 önerilir, ZK-friendly)
```

---

## I. NOVA SHIELD'E ÖZEL UYARLAMA ÖNERİLERİ

### I.1 Kritik Bug Düzeltme Öncelikleri (Solana Dersleri Işığında)

Solana kod incelemesinden çıkarılan bulgular, Nova Shield'ın mevcut bug listesiyle eşleştirilerek önceliklendirilmiştir:

**ÖNCELIK 1 — Güvenlik Açığı (Hemen Düzelt):**

```
Bug #4: TX imzası hiç doğrulanmıyor
→ Solana'da SIGNATURE_COST = 720 CU, her TX verify zorunlu
→ Düzeltme: nova-runtime/src/executor.rs
  fn execute_tx(tx: &SignedTx) -> Result<()> {
      tx.verify_signature()?;  // ED25519 veya Schnorr
      // ...
  }

Bug #5: TX hash içerik-bağımsız (to/amount dahil değil)
→ Solana: tx_hash = SHA256(message_bytes), mesaj to+amount içerir
→ Düzeltme: hash = SHA256(from || to || amount || nonce || data)

Bug #3: VrfVerifier::verify stub (always Err)
→ Solana VRF: ed25519-dalek ile prove/verify
→ Kullan: vrf-rs veya schnorrkel crate (Polkadot'un kullandığı)
```

**ÖNCELIK 2 — Konsensüs Bütünlüğü:**

```
Bug #6: Konsensüs epoch 2400 slot vs 216,000 slot çakışması
→ KESIN DÜZELTME:
  // nova-consensus/src/constants.rs
  pub const EPOCH_SLOTS: u64 = 216_000;  // Bu değer her yerde kullanılmalı

Bug #1: Threshold encrypt/decrypt simetrisi bozuk
→ cggmp21 crate kullanın (mevcut karar doğru)
→ Test: encrypt(decrypt(x)) == x VE decrypt(encrypt(x)) == x

Bug #2: DilithiumPublicKey::SIZE 1312 vs 1952
→ Dilithium3 = 1952 byte (NIST standartı)
→ Düzeltme: pub const SIZE: usize = 1952;
```

**ÖNCELIK 3 — Ekonomik Doğruluk:**

```
Bug #15: Dec::mul hassasiyet hatası
→ Solana: u128 tabanlı sabit noktalı aritmetik
→ Nova için: u128 ile 10^18 ölçekli tüm hesaplar
→ Hiçbir yerde f64 kullanma (token miktarları için)

Bug #18: Miras distribute hesabı yanlış
→ Solana reward dağıtımı: (points × total) / total_points
→ Aynı pattern: (varis_pay × toplam_miras) / 10_000 (bps ile)
```

### I.2 Guardian MPC + Solana FROST Dersleri

**Solana'nın MPC yaklaşımı (agent-30):**
```
// Solana FROST kullanmıyor, basit multisig kullanıyor
// Ama ZK Token SDK'da threshold hesap açma var

// Nova Shield için doğru yaklaşım:
// cggmp21 (CGGMP21 protocol) = modern, güvenli, identifiable abort
// Solana'dan ders: State machine pattern kullanın
// Her round açık struct, exhaustive match
pub enum FrostRound {
    Round1(Round1State),
    Round2(Round2State),
    Complete(FrostSignature),
}
```

**Guardian Offer süresi Bug #11 düzeltmesi:**
```rust
// YANLIŞ: 21,600 slot = 2.4 saat
// DOĞRU: 1 hafta = 7 × 24 × 3600 / 0.4 = 1,512,000 slot
// Solana benzeri: sabit zaman → slot dönüşümü
pub fn weeks_to_slots(weeks: u64) -> u64 {
    weeks * 7 * 24 * 3600 * 1000 / 400  // 400ms per slot
}
pub const GUARDIAN_OFFER_DURATION_SLOTS: u64 = 1_512_000; // 1 hafta
```

### I.3 DEX Batch Auction + Solana MEV Dersleri

**Solana MEV yaklaşımı vs Nova Shield:**
```
Solana MEV koruması yok (açık mempool)
→ Bu yüzden Jito Labs, MEV bots için özel infra kurdu
→ Nova Shield'ın IBE + Batch Auction yaklaşımı ÜSTÜN

Solana'dan alınacak DEX dersleri (agent-30):
1. Priority fee → Nova: auction priority = priority_fee × stake_weight
2. AccountLock → Nova: DEX emirleri account lock ile çakışma koruması
3. MAX_TX_BATCH = 64 → Nova DEX: 64 emirlik batch optimal

Bug #16 (CoW feature eksik):
// Coincidence of Wants implementasyonu
pub fn find_cow_pairs(orders: &[Order]) -> Vec<(OrderId, OrderId)> {
    // BTC/ETH sell + ETH/BTC sell → çapraz netting
    orders.iter().combinations(2)
        .filter(|pair| pair[0].sell_token == pair[1].buy_token
               && pair[0].buy_token == pair[1].sell_token)
        .map(|pair| (pair[0].id, pair[1].id))
        .collect()
}
```

### I.4 Network Katmanı (P2P Mock → Gerçek)

**Solana'nın P2P seçimlerinden dersler:**
```
Solana: Custom UDP + QUIC (libp2p kullanmadı)
Nova Shield planı: libp2p + Noise XX

Öneri: Nova Shield libp2p kullanabilir, ancak şunlara dikkat:
1. Turbine-benzeri shred yayılımı: libp2p-gossipsub yeterli
2. QUIC transport: libp2p-quic (quinn tabanlı)
3. Kademlia DHT: libp2p-kad (mevcut plan doğru)

Kritik değerler (Solana'dan):
- PUSH_FANOUT = 9 (gossip için)
- SHRED_DATA_PAYLOAD_SIZE = 1,232 byte (MTU-safe)
- Erasure: 32+32 (veri + kod shred)
- Discovery: K=20 (Kademlia)
```

**Nova P2P implementasyon öncelik sırası:**
```
1. libp2p + TCP/QUIC transport (temel bağlantı)
2. Noise XX handshake (şifreli kanal)
3. Gossipsub (blok yayılımı)
4. Kademlia DHT (peer discovery)
5. Turbine-benzeri shred protokolü (büyük bloklar)
6. Reed-Solomon erasure (gerçek RS, XOR değil — Bug #13)
```

### I.5 Solana'dan Alınacak Rust Implementasyon Desenleri

**1. Dual-target pattern (WASM/Native):**
```rust
// agent-02.md'den
#[cfg(target_os = "solana")]
pub fn syscall_hash(data: &[u8]) -> Hash { /* syscall */ }

#[cfg(not(target_os = "solana"))]
pub fn syscall_hash(data: &[u8]) -> Hash { sha2::Sha256::digest(data).into() }

// Nova için: native (full node) / wasm (light client) / embedded (HSM)
```

**2. CloneZeroed (konsensüs mesajları için):**
```rust
// nova-consensus/src/types.rs
#[derive(CloneZeroed)]
pub struct ConsensusVote {
    pub validator: [u8; 32],
    pub slot: u64,
    pub hash: [u8; 32],
    // Padding byte'ları otomatik sıfırlanır → hash tutarlı
}
```

**3. DashMap paralel banka (Bank fork takibi):**
```rust
// nova-runtime/src/bank.rs
use dashmap::DashMap;
pub struct BankForks {
    banks: DashMap<u64, Arc<NovaBank>>,  // slot → bank
    root: AtomicU64,
}
```

**4. Rayon paralel doğrulama:**
```rust
// Batch transaction verify
use rayon::prelude::*;
fn verify_batch(txs: &[SignedTx]) -> Vec<Result<(), SigError>> {
    txs.par_iter().map(|tx| tx.verify()).collect()
}
```

**5. EVM keccak256 Bug #12 düzeltmesi:**
```rust
// YANLIŞ: BLAKE3 kullanılıyor (nova-vm/src/evm.rs)
// fn function_selector(sig: &str) -> [u8; 4] {
//     blake3::hash(sig.as_bytes()).as_bytes()[..4]
// }

// DOĞRU: keccak256
use tiny_keccak::{Keccak, Hasher};
fn function_selector(sig: &str) -> [u8; 4] {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(sig.as_bytes());
    hasher.finalize(&mut output);
    [output[0], output[1], output[2], output[3]]
}
```

### I.6 Kritik Sayısal Değerler Özet Tablosu

| Parametre | Solana Değeri | Nova Shield Değeri | Kaynak |
|-----------|--------------|-------------------|--------|
| Slot süresi | 400 ms | 400 ms (eşit) | agent-03.md |
| Ticks/slot | 64 | 64 (önerilen) | agent-03.md |
| Hashes/tick | 12,500 | 12,500 SHA-256 veya BLAKE3 | agent-20.md |
| Slot/epoch | 432,000 | 216,000 (24 saat) | agent-03.md |
| Max buffer TX | 700,000 | 700,000 (önerilen) | agent-11.md |
| TX batch | 64 | 64 (önerilen) | agent-11.md |
| Fanout (Turbine) | 200 | 14 (200 validator için) | agent-17.md, agent-20.md |
| Shred payload | 1,232 byte | 1,232 byte (MTU-safe) | agent-17.md |
| Erasure data | 32 | 32 (önerilen) | agent-17.md |
| Erasure parity | 32 | 32 (önerilen) | agent-17.md |
| Signature CU cost | 720 CU | 720 NG (Nova Gas) | agent-08.md |
| FROST sig CU | N/A | 7,200 NG (10x) | agent-08.md (yorum) |
| Burn rate | %50 | %50 (mevcut) | agent-03.md |
| Fee step | %5 | %5 (önerilen) | agent-03.md |
| Warmup rate | %25/epoch | %25/epoch (önerilen) | agent-08.md |
| Fork threshold | 0.38 | 0.38 (Snowball'a entegre) | agent-14.md |
| Vote depth | 8 | 8 (Simplex BFT'ye adapte) | agent-14.md |
| Max lockout | 31 | 31 (önerilen) | agent-14.md |
| QUIC peer conn | 8 | 8 (önerilen) | agent-29.md |
| Max block replay | 400,000 μs | 400,000 μs (1 slot) | agent-28.md |
| Governance offer | N/A | 1,512,000 slot (1 hafta) | agent-07.md (yorum) |
| ElGamal ciphertext | 64 byte | 64 byte | agent-15.md |
| Pedersen commitment | 32 byte | 32 byte | agent-15.md |

### I.7 Nova Shield için Crate Listesi (Solana Dersleriyle)

```toml
# nova-runtime Cargo.toml

[dependencies]
# Kriptografi
sha2 = "0.10"            # PoH için SHA-256
blake3 = "1.5"           # Hızlı hash (PoH alternatifi)
curve25519-dalek = { version = "4", features = ["ristretto"] }  # ZK
tiny-keccak = { version = "2.0", features = ["keccak"] }  # EVM (Bug #12)
zeroize = "1.7"          # Gizli anahtar güvenliği
ed25519-dalek = "2"      # Ed25519 imza
# schnorrkel = "0.11"    # Schnorr/FROST (eğer cggmp21 yeterli değilse)
cggmp21 = "0.4"          # FROST threshold MPC

# Depolama
memmap2 = "0.9"          # AppendVec (mmap)
rocksdb = "0.21"         # Index ve kalıcı depolama
zstd = "0.13"            # Snapshot sıkıştırma
lz4 = "1.24"             # Hızlı sıkıştırma

# Paralel işlem
rayon = "1.10"           # CPU paralel (AccountsHash, sigverify)
dashmap = "5"            # Concurrent HashMap (BankForks)
crossbeam-channel = "0.5"  # Pipeline channel (PoH → Banking)
crossbeam-queue = "0.3"    # Lock-free queue (mempool)

# Network
quinn = "0.11"           # QUIC transport
libp2p = { version = "0.53", features = ["tcp", "noise", "yamux", "gossipsub", "kad"] }

# Erasure coding
reed-solomon-erasure = { version = "6", features = ["simd-accel"] }  # Bug #13

# ZK
ark-bn254 = "0.4"        # BN254 curve (Groth16)
ark-groth16 = "0.4"      # ZK proof system
ark-std = "0.4"          # no_std uyumlu

# Utility
borsh = "1.5"            # Serializasyon
bytemuck = "1.18"        # Zero-copy cast (transmute alternatifi)
tokio = { version = "1", features = ["full"] }  # Async runtime
```

---

## SONUÇ: SOLANA'DAN EN KRİTİK 10 DERS

1. **PoH zamanlaması:** 64 tick × 12,500 hash = 800,000 op/slot → 400ms kesinlikle ulaşılabilir
2. **u128 zorunluluğu:** Stake × credits çarpımında u64 taşar; tüm token hesaplarında u128 kullan
3. **AppendVec + mmap:** Hesap depolama için append-only mmap, rocksdb index üzerinde
4. **Stake-weighted her şey:** Bağlantı limiti, oy ağırlığı, TX önceliği — hepsi stake orantılı
5. **Warmup/cooldown (25%):** Ani stake değişimlerini epoch bazında sınırla
6. **Komisyon/ücret kısıtı:** Epoch ilk yarısında değiştirilebilir, ikinci yarıda kilitli
7. **CloneZeroed:** Konsensüs struct'larında padding byte sıfırlama → hash tutarlılığı
8. **Zeroize on drop:** Tüm gizli anahtar türlerinde (`#[derive(ZeroizeOnDrop)]`)
9. **Reed-Solomon (32+32):** XOR değil gerçek GF(2^8) erasure coding (Bug #13)
10. **QUIC + stake rate limit:** DoS koruması için stake büyüklüğüne göre bağlantı kotası

---
*Bu rapor, Nova Shield Chain araştırma sürecinde Solana kaynak kod analizi (30 dosya, agent-01 — agent-30-MASTER-SENTEZ) temel alınarak hazırlanmıştır. Tüm sayısal değerler kaynak dosyalardan doğrulanmıştır.*
