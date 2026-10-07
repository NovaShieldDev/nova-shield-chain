//! GÖREV 114: QosService — Cost Model Blok Bütçe Kontrolü (Solana Pattern)
//!
//! ## Mimari
//!
//! ### Solana QoS Service
//! - Her TX'in "maliyet" hesabı: compute_units + accounts_data_size + signature_cost
//! - Blok başına bütçe: MAX_BLOCK_UNITS (ör. 48M CU)
//! - TX sıralanırken önce "compute_limit_reached" kontrolü
//! - `QosService::select_and_accumulate_fee_payer_locks()` → blok dolana kadar TX seç
//!
//! ### Maliyet Bileşenleri (Solana)
//! - `signature_cost`: TX başına 720 CU (secp256k1 verify)
//! - `write_lock_cost`: 300 CU (yazılabilir account başına)
//! - `data_bytes_cost`: veri başına CU (data_bytes / DATA_BYTES_UNITS)
//! - `execution_cost_limit`: kullanıcının beyan ettiği compute limit
//!
//! ### Nova Uyarlaması
//! - `TxCostModel`: bir TX'in blok bütçesini ne kadar tükettiğini hesaplar
//! - `BlockCostAccumulator`: blok başına kümülatif maliyet takibi
//! - `QosService`: TX seçimi + maliyet kontrolü

use nova_primitives::Address;
use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Blok başına maksimum compute unit bütçesi
///
/// Solana: `MAX_BLOCK_UNITS = 48_000_000`
pub const MAX_BLOCK_UNITS: u64 = 48_000_000;

/// Blok başına maksimum yazılabilir hesap sayısı
///
/// Solana: `MAX_WRITABLE_ACCOUNT_UNITS = 12_000_000`
pub const MAX_WRITABLE_ACCOUNT_UNITS: u64 = 12_000_000;

/// İmza maliyeti (compute unit)
///
/// Solana: `SIGNATURE_COST = 720`
pub const SIGNATURE_COST: u64 = 720;

/// Yazılabilir hesap kilit maliyeti (compute unit)
///
/// Solana: `WRITE_LOCK_UNITS = 300`
pub const WRITE_LOCK_UNITS: u64 = 300;

/// Her veri byte'ı için maliyet (compute unit)
///
/// Solana: `DATA_BYTES_UNITS = 250` (her 256 byte = 1 CU)
pub const DATA_BYTES_UNITS: u64 = 250;

/// Veri birim boyutu (byte)
pub const DATA_UNIT_BYTES: u64 = 256;

/// Varsayılan compute limit (kullanıcı beyan etmezse)
///
/// Solana: `DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT = 200_000`
pub const DEFAULT_COMPUTE_LIMIT: u64 = 200_000;

/// Maksimum compute limit (tek TX)
pub const MAX_COMPUTE_LIMIT: u64 = 1_400_000;

/// Hesap başına maliyet alt sınırı (1 CU)
pub const MIN_ACCOUNT_COST: u64 = 1;

// ─── TxCostBreakdown ──────────────────────────────────────────────────────────

/// Bir TX'in blok bütçe maliyet dökümü
///
/// Solana: `TransactionCost { write_lock_cost, data_bytes_cost, builtins_execution_cost, bpf_execution_cost, .. }`
#[derive(Debug, Clone)]
pub struct TxCostBreakdown {
    /// İmza doğrulama maliyeti
    pub signature_cost: u64,
    /// Yazılabilir hesap kilit maliyeti
    pub write_lock_cost: u64,
    /// Veri boyutu maliyeti
    pub data_bytes_cost: u64,
    /// Yürütme maliyeti (CU)
    pub execution_cost: u64,
    /// Toplam maliyet
    pub total: u64,
}

impl TxCostBreakdown {
    /// Toplam hesapla
    pub fn compute_total(&mut self) {
        self.total = self.signature_cost
            + self.write_lock_cost
            + self.data_bytes_cost
            + self.execution_cost;
    }
}

// ─── TxCostModel ──────────────────────────────────────────────────────────────

/// TX maliyet hesaplama motoru
///
/// Solana: `CostModel::calculate_cost(transaction, ..)`
pub struct TxCostModel {
    pub signature_cost: u64,
    pub write_lock_cost: u64,
    pub data_bytes_cost_per_unit: u64,
}

impl TxCostModel {
    pub fn default_nova() -> Self {
        Self {
            signature_cost: SIGNATURE_COST,
            write_lock_cost: WRITE_LOCK_UNITS,
            data_bytes_cost_per_unit: DATA_BYTES_UNITS,
        }
    }

    /// Bir TX'in maliyetini hesapla
    ///
    /// - `num_signatures`: TX'teki imza sayısı
    /// - `num_write_locks`: yazılabilir hesap sayısı
    /// - `data_bytes`: TX veri boyutu (byte)
    /// - `compute_limit`: kullanıcı beyan ettiği veya varsayılan CU limit
    pub fn calculate_cost(
        &self,
        num_signatures: u64,
        num_write_locks: u64,
        data_bytes: u64,
        compute_limit: Option<u64>,
    ) -> TxCostBreakdown {
        let signature_cost = num_signatures * self.signature_cost;
        let write_lock_cost = num_write_locks * self.write_lock_cost;
        let data_bytes_cost = (data_bytes / DATA_UNIT_BYTES) * self.data_bytes_cost_per_unit
            + if data_bytes % DATA_UNIT_BYTES > 0 { self.data_bytes_cost_per_unit } else { 0 };
        let execution_cost = compute_limit
            .unwrap_or(DEFAULT_COMPUTE_LIMIT)
            .min(MAX_COMPUTE_LIMIT);

        let mut breakdown = TxCostBreakdown {
            signature_cost,
            write_lock_cost,
            data_bytes_cost,
            execution_cost,
            total: 0,
        };
        breakdown.compute_total();
        breakdown
    }
}

// ─── BlockCostAccumulator ─────────────────────────────────────────────────────

/// Blok başına kümülatif maliyet takibi
///
/// Solana: `BlockCostTracker { block_cost, vote_cost, transaction_count }`
#[derive(Debug, Default, Clone)]
pub struct BlockCostAccumulator {
    /// Toplam compute unit tüketimi
    pub total_cu: u64,
    /// Toplam yazılabilir hesap kilitleri (CU)
    pub total_write_lock_cu: u64,
    /// Blok'taki TX sayısı
    pub tx_count: u64,
    /// Her hesap başına kilit miktarı: account_addr → lock_count
    account_locks: HashMap<Address, u64>,
}

impl BlockCostAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// TX maliyetini blok bütçesine ekle
    ///
    /// Returns: ekleme başarılı mı (bütçe yeterliyse)
    pub fn try_add(&mut self, cost: &TxCostBreakdown) -> bool {
        // Blok bütçesi kontrolü
        if self.total_cu + cost.total > MAX_BLOCK_UNITS {
            return false;
        }
        if self.total_write_lock_cu + cost.write_lock_cost > MAX_WRITABLE_ACCOUNT_UNITS {
            return false;
        }

        self.total_cu += cost.total;
        self.total_write_lock_cu += cost.write_lock_cost;
        self.tx_count += 1;
        true
    }

    /// Hesap kilidi ekle
    pub fn lock_account(&mut self, account: Address) {
        *self.account_locks.entry(account).or_insert(0) += 1;
    }

    /// Kalan compute unit kapasitesi
    pub fn remaining_cu(&self) -> u64 {
        MAX_BLOCK_UNITS.saturating_sub(self.total_cu)
    }

    /// Kalan yazılabilir hesap kapasitesi
    pub fn remaining_write_lock_cu(&self) -> u64 {
        MAX_WRITABLE_ACCOUNT_UNITS.saturating_sub(self.total_write_lock_cu)
    }

    /// Blok doluluk oranı (bps)
    pub fn utilization_bps(&self) -> u64 {
        (self.total_cu * 10_000 / MAX_BLOCK_UNITS).min(10_000)
    }

    /// Blok tam dolu mu?
    pub fn is_full(&self) -> bool {
        self.total_cu >= MAX_BLOCK_UNITS
    }

    /// Bloku sıfırla (yeni blok başlangıcı)
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

// ─── TxForQos ─────────────────────────────────────────────────────────────────

/// QoS değerlendirmesi için TX temsili
#[derive(Debug, Clone)]
pub struct TxForQos {
    /// TX hash
    pub tx_hash: [u8; 32],
    /// Gönderici
    pub sender: Address,
    /// İmza sayısı
    pub num_signatures: u64,
    /// Yazılabilir hesap sayısı
    pub num_write_locks: u64,
    /// TX veri boyutu
    pub data_bytes: u64,
    /// İstenen compute limit
    pub compute_limit: Option<u64>,
    /// Öncelik ücreti (fee per CU) — yüksek ücret → öncelik
    pub priority_fee_per_cu: u64,
}

/// QoS seçim sonucu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QosDecision {
    /// TX bloka alındı
    Accepted,
    /// Blok bütçesi dolu
    BlockFull,
    /// Write lock bütçesi dolu
    WriteLocksFull,
    /// TX tek başına çok pahalı
    TxTooExpensive,
}

// ─── QosService ───────────────────────────────────────────────────────────────

/// QoS servisi: TX seçimi + blok bütçe yönetimi
///
/// Solana: `QosService::select_and_accumulate_fee_payer_locks(..)`
pub struct QosService {
    pub cost_model: TxCostModel,
    pub accumulator: BlockCostAccumulator,
}

impl QosService {
    pub fn new() -> Self {
        Self {
            cost_model: TxCostModel::default_nova(),
            accumulator: BlockCostAccumulator::new(),
        }
    }

    /// TX değerlendir ve blok bütçesine ekle
    ///
    /// Returns: (maliyet, karar)
    pub fn evaluate_tx(&mut self, tx: &TxForQos) -> (TxCostBreakdown, QosDecision) {
        let cost = self.cost_model.calculate_cost(
            tx.num_signatures,
            tx.num_write_locks,
            tx.data_bytes,
            tx.compute_limit,
        );

        // Tek TX kendi başına MAX/10'dan büyükse reddet
        if cost.total > MAX_BLOCK_UNITS / 10 {
            return (cost, QosDecision::TxTooExpensive);
        }

        if self.accumulator.total_cu + cost.total > MAX_BLOCK_UNITS {
            return (cost, QosDecision::BlockFull);
        }

        if self.accumulator.total_write_lock_cu + cost.write_lock_cost > MAX_WRITABLE_ACCOUNT_UNITS {
            return (cost, QosDecision::WriteLocksFull);
        }

        self.accumulator.try_add(&cost);
        self.accumulator.lock_account(tx.sender);

        (cost, QosDecision::Accepted)
    }

    /// Birden fazla TX'i öncelik sırasına göre seç
    ///
    /// Solana: yüksek priority fee → önce seçilir
    pub fn select_txs<'a>(&mut self, txs: &'a [TxForQos]) -> Vec<(&'a TxForQos, TxCostBreakdown)> {
        // Önce öncelik fee'ye göre sırala (yüksek önce)
        let mut sorted: Vec<&TxForQos> = txs.iter().collect();
        sorted.sort_by(|a, b| b.priority_fee_per_cu.cmp(&a.priority_fee_per_cu));

        let mut selected = Vec::new();
        for tx in sorted {
            let (cost, decision) = self.evaluate_tx(tx);
            match decision {
                QosDecision::Accepted => selected.push((tx, cost)),
                QosDecision::BlockFull => break, // Blok dolu → dur
                _ => {} // Diğer hatalar → atla, devam et
            }
        }
        selected
    }

    /// Yeni blok için sıfırla
    pub fn reset_for_new_block(&mut self) {
        self.accumulator.reset();
    }

    /// Blok doluluk oranı
    pub fn utilization_bps(&self) -> u64 {
        self.accumulator.utilization_bps()
    }
}

impl Default for QosService {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn simple_tx(n: u8, priority: u64) -> TxForQos {
        TxForQos {
            tx_hash: [n; 32],
            sender: addr(n),
            num_signatures: 1,
            num_write_locks: 2,
            data_bytes: 256,
            compute_limit: Some(100_000),
            priority_fee_per_cu: priority,
        }
    }

    // 1. Temel maliyet hesabı
    #[test]
    fn test_cost_calculation() {
        let model = TxCostModel::default_nova();
        let cost = model.calculate_cost(1, 2, 256, Some(100_000));

        assert_eq!(cost.signature_cost, SIGNATURE_COST); // 720
        assert_eq!(cost.write_lock_cost, 2 * WRITE_LOCK_UNITS); // 600
        assert_eq!(cost.data_bytes_cost, DATA_BYTES_UNITS); // 250 (256 byte = 1 unit)
        assert_eq!(cost.execution_cost, 100_000);
        assert_eq!(cost.total, 720 + 600 + 250 + 100_000);
    }

    // 2. Varsayılan compute limit uygulanır
    #[test]
    fn test_default_compute_limit() {
        let model = TxCostModel::default_nova();
        let cost = model.calculate_cost(1, 0, 0, None);
        assert_eq!(cost.execution_cost, DEFAULT_COMPUTE_LIMIT);
    }

    // 3. MAX_COMPUTE_LIMIT üstü sınırlanır
    #[test]
    fn test_max_compute_limit_capped() {
        let model = TxCostModel::default_nova();
        let cost = model.calculate_cost(1, 0, 0, Some(u64::MAX));
        assert_eq!(cost.execution_cost, MAX_COMPUTE_LIMIT);
    }

    // 4. Blok bütçe kabulü
    #[test]
    fn test_block_accumulator_accept() {
        let mut acc = BlockCostAccumulator::new();
        let model = TxCostModel::default_nova();
        let cost = model.calculate_cost(1, 1, 128, Some(10_000));

        assert!(acc.try_add(&cost));
        assert_eq!(acc.tx_count, 1);
    }

    // 5. Blok dolduğunda red
    #[test]
    fn test_block_full_rejection() {
        let mut acc = BlockCostAccumulator::new();

        // Bloku manuel olarak dolduralım
        acc.total_cu = MAX_BLOCK_UNITS - 100;

        let model = TxCostModel::default_nova();
        let cost = model.calculate_cost(1, 0, 0, Some(500)); // 500+720=1220 > 100

        assert!(!acc.try_add(&cost));
        assert_eq!(acc.tx_count, 0); // Eklenmedi
    }

    // 6. Utilization bps hesabı
    #[test]
    fn test_utilization_bps() {
        let mut acc = BlockCostAccumulator::new();
        acc.total_cu = MAX_BLOCK_UNITS / 2;
        assert_eq!(acc.utilization_bps(), 5_000); // %50
    }

    // 7. QosService TX kabulü
    #[test]
    fn test_qos_service_accept() {
        let mut qos = QosService::new();
        let tx = simple_tx(1, 100);

        let (cost, decision) = qos.evaluate_tx(&tx);
        assert_eq!(decision, QosDecision::Accepted);
        assert!(cost.total > 0);
        assert_eq!(qos.accumulator.tx_count, 1);
    }

    // 8. QosService çok pahalı TX reddi
    #[test]
    fn test_qos_too_expensive() {
        let mut qos = QosService::new();
        // MAX_BLOCK_UNITS/10 = 4_800_000
        // 7000 imza * 720 = 5_040_000 > 4_800_000
        let expensive_tx = TxForQos {
            tx_hash: [1u8; 32],
            sender: addr(1),
            num_signatures: 7_000, // çok fazla imza
            num_write_locks: 0,
            data_bytes: 0,
            compute_limit: Some(0),
            priority_fee_per_cu: 0,
        };

        let (_, decision) = qos.evaluate_tx(&expensive_tx);
        assert_eq!(decision, QosDecision::TxTooExpensive);
    }

    // 9. TX seçimi öncelik sırasına göre
    #[test]
    fn test_select_txs_by_priority() {
        let mut qos = QosService::new();
        let txs = vec![
            simple_tx(1, 10),  // düşük öncelik
            simple_tx(2, 100), // yüksek öncelik
            simple_tx(3, 50),  // orta öncelik
        ];

        let selected = qos.select_txs(&txs);
        assert_eq!(selected.len(), 3);
        // İlk seçilen en yüksek öncelikli
        assert_eq!(selected[0].0.tx_hash, [2u8; 32]);
        assert_eq!(selected[1].0.tx_hash, [3u8; 32]);
        assert_eq!(selected[2].0.tx_hash, [1u8; 32]);
    }

    // 10. BlockCostAccumulator sıfırlama
    #[test]
    fn test_accumulator_reset() {
        let mut qos = QosService::new();
        let tx = simple_tx(1, 0);
        qos.evaluate_tx(&tx);
        assert_eq!(qos.accumulator.tx_count, 1);

        qos.reset_for_new_block();
        assert_eq!(qos.accumulator.tx_count, 0);
        assert_eq!(qos.accumulator.total_cu, 0);
    }

    // 11. Kalan kapasite hesabı
    #[test]
    fn test_remaining_cu() {
        let mut acc = BlockCostAccumulator::new();
        acc.total_cu = 1_000_000;

        let remaining = acc.remaining_cu();
        assert_eq!(remaining, MAX_BLOCK_UNITS - 1_000_000);
    }

    // 12. Write lock bütçe aşımı
    #[test]
    fn test_write_lock_budget_exceeded() {
        let mut acc = BlockCostAccumulator::new();
        acc.total_write_lock_cu = MAX_WRITABLE_ACCOUNT_UNITS - 100;

        let model = TxCostModel::default_nova();
        // write_lock_cost = 2 * 300 = 600 > 100
        let cost = model.calculate_cost(0, 2, 0, Some(0));

        // Write lock bütçesi aşıldı
        assert!(!acc.try_add(&cost));
    }

    // 13. Veri boyutu maliyet hesabı (çok büyük veri)
    #[test]
    fn test_large_data_cost() {
        let model = TxCostModel::default_nova();
        // 1024 byte = 4 unit * 250 = 1000 CU
        let cost = model.calculate_cost(0, 0, 1024, Some(0));
        assert_eq!(cost.data_bytes_cost, 4 * DATA_BYTES_UNITS);
    }

    // 14. is_full() kontrolü
    #[test]
    fn test_is_full() {
        let mut acc = BlockCostAccumulator::new();
        assert!(!acc.is_full());
        acc.total_cu = MAX_BLOCK_UNITS;
        assert!(acc.is_full());
    }
}
