//! EIP-1559 benzeri dinamik fee hesabı (§8.1, go-ethereum'dan ilham)
//!
//! Nova Shield Chain fee mekanizması:
//!   - Base fee: Blok doluluğuna göre algoritma ile belirlenir (±%12.5/blok)
//!   - Gas hedefi: Max gas limitinin %50'si
//!   - Yakılır: Base fee'nin %50'si yakılır
//!   - Validator tip: priority_fee

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Başlangıç base fee: 1 Gwei = 1_000_000_000
pub const INITIAL_BASE_FEE: u128 = 1_000_000_000;

/// Minimum base fee (deflasyon koruma)
pub const MIN_BASE_FEE: u128 = 100_000_000; // 0.1 Gwei

/// Maksimum base fee değişim oranı paydası: 8 → en fazla %12.5 değişim
pub const BASE_FEE_CHANGE_DENOMINATOR: u128 = 8;

/// Elastikiyet çarpanı: 2 → hedef = max_gas / 2
pub const ELASTICITY_MULTIPLIER: u64 = 2;

/// Nova Shield blok gas limiti (Ethereum'dan büyük: 30M → 100M)
pub const BLOCK_GAS_LIMIT: u64 = 100_000_000;

/// Gas hedefi: GasLimit / ElasticityMultiplier
pub const GAS_TARGET: u64 = BLOCK_GAS_LIMIT / ELASTICITY_MULTIPLIER;

/// Minimum transfer gas maliyeti
pub const TRANSFER_GAS_COST: u64 = 21_000;

/// Staking işlemi gas maliyeti
pub const STAKING_GAS_COST: u64 = 50_000;

/// Governance önerisi gas maliyeti
pub const GOVERNANCE_GAS_COST: u64 = 100_000;

// ─── BaseFee hesabı (EIP-1559 algoritması) ────────────────────────────────────

/// Bir sonraki bloğun base fee'sini hesapla.
/// - gas_used > gas_target → base_fee artar
/// - gas_used < gas_target → base_fee azalır
/// - gas_used = gas_target → değişmez
pub fn calc_next_base_fee(parent_base_fee: u128, parent_gas_used: u64, gas_target: u64) -> u128 {
    if parent_gas_used == gas_target {
        return parent_base_fee;
    }

    if parent_gas_used > gas_target {
        // Artış: delta = parent_base_fee * (gas_used - gas_target) / gas_target / 8
        let delta_num = parent_base_fee
            * (parent_gas_used - gas_target) as u128
            / gas_target as u128
            / BASE_FEE_CHANGE_DENOMINATOR;
        let delta = delta_num.max(1); // En az 1 birim artış
        parent_base_fee.saturating_add(delta)
    } else {
        // Azalış: delta = parent_base_fee * (gas_target - gas_used) / gas_target / 8
        let delta = parent_base_fee
            * (gas_target - parent_gas_used) as u128
            / gas_target as u128
            / BASE_FEE_CHANGE_DENOMINATOR;
        parent_base_fee.saturating_sub(delta).max(MIN_BASE_FEE)
    }
}

/// İşlem effective gas price hesabı
/// effective_gas_price = min(max_fee_per_gas, base_fee + priority_fee)
pub fn effective_gas_price(
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    base_fee: u128,
) -> Option<u128> {
    if max_fee_per_gas < base_fee {
        return None; // İşlem kabul edilemez
    }
    let priority = max_priority_fee_per_gas.min(max_fee_per_gas - base_fee);
    Some(base_fee + priority)
}

/// İşlem toplam fee = effective_gas_price * gas_used
pub fn total_fee(effective_price: u128, gas_used: u64) -> u128 {
    effective_price * gas_used as u128
}

// ─── BaseFeeOracle ────────────────────────────────────────────────────────────

/// BaseFeeOracle — blok bazlı base fee takipçisi
#[derive(Debug, Clone)]
pub struct BaseFeeOracle {
    current_base_fee: u128,
    gas_target: u64,
}

impl Default for BaseFeeOracle {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseFeeOracle {
    pub fn new() -> Self {
        Self { current_base_fee: INITIAL_BASE_FEE, gas_target: GAS_TARGET }
    }

    pub fn with_initial_fee(fee: u128) -> Self {
        Self { current_base_fee: fee, gas_target: GAS_TARGET }
    }

    pub fn current_fee(&self) -> u128 {
        self.current_base_fee
    }

    /// Yeni blok tamamlandı → gas kullanımına göre fee güncelle
    pub fn update(&mut self, gas_used: u64) {
        self.current_base_fee =
            calc_next_base_fee(self.current_base_fee, gas_used, self.gas_target);
    }

    /// İşlemin geçerli olup olmadığını kontrol et
    pub fn is_tx_valid(&self, max_fee_per_gas: u128) -> bool {
        max_fee_per_gas >= self.current_base_fee
    }
}

// ─── FeeHistory ───────────────────────────────────────────────────────────────

/// Son N bloğun fee geçmişi (EIP-1559 eth_feeHistory RPC benzeri)
#[derive(Debug, Clone)]
pub struct FeeHistory {
    base_fees: std::collections::VecDeque<u128>,
    gas_used_ratios: std::collections::VecDeque<u32>,
    capacity: usize,
}

impl FeeHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            base_fees: std::collections::VecDeque::with_capacity(capacity),
            gas_used_ratios: std::collections::VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn record(&mut self, base_fee: u128, gas_used: u64) {
        let ratio = ((gas_used as u128 * 10_000) / BLOCK_GAS_LIMIT as u128) as u32;
        if self.base_fees.len() >= self.capacity {
            self.base_fees.pop_front();
            self.gas_used_ratios.pop_front();
        }
        self.base_fees.push_back(base_fee);
        self.gas_used_ratios.push_back(ratio);
    }

    pub fn average_base_fee(&self) -> u128 {
        if self.base_fees.is_empty() { return 0; }
        let sum: u128 = self.base_fees.iter().sum();
        sum / self.base_fees.len() as u128
    }

    pub fn average_gas_ratio_bps(&self) -> u32 {
        if self.gas_used_ratios.is_empty() { return 0; }
        let sum: u32 = self.gas_used_ratios.iter().sum();
        sum / self.gas_used_ratios.len() as u32
    }

    pub fn suggest_priority_fee(&self) -> u128 {
        self.average_base_fee() / 10
    }

    pub fn len(&self) -> usize { self.base_fees.len() }
    pub fn is_empty(&self) -> bool { self.base_fees.is_empty() }
}

/// Multi-dimensional fee: execution + data (EIP-4844 hazırlık)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiDimensionalFee {
    pub execution_fee: u128,
    pub data_fee: u128,
}

impl MultiDimensionalFee {
    pub fn new(execution_fee: u128, data_fee: u128) -> Self {
        Self { execution_fee, data_fee }
    }

    pub fn total(&self) -> u128 {
        self.execution_fee.saturating_add(self.data_fee)
    }
}

pub fn calc_multi_dimensional_fee(
    base_fee: u128,
    gas_used: u64,
    calldata_bytes: usize,
) -> MultiDimensionalFee {
    let execution_fee = base_fee * gas_used as u128;
    let data_fee = base_fee * calldata_bytes as u128 * 16;
    MultiDimensionalFee::new(execution_fee, data_fee)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_fee_unchanged_at_target() {
        let fee = calc_next_base_fee(INITIAL_BASE_FEE, GAS_TARGET, GAS_TARGET);
        assert_eq!(fee, INITIAL_BASE_FEE);
    }

    #[test]
    fn test_base_fee_increases_above_target() {
        let fee = calc_next_base_fee(INITIAL_BASE_FEE, GAS_TARGET * 2, GAS_TARGET);
        assert!(fee > INITIAL_BASE_FEE);
    }

    #[test]
    fn test_base_fee_decreases_below_target() {
        let fee = calc_next_base_fee(INITIAL_BASE_FEE, 0, GAS_TARGET);
        assert!(fee < INITIAL_BASE_FEE);
        assert!(fee >= MIN_BASE_FEE);
    }

    #[test]
    fn test_base_fee_max_increase_12_5_percent() {
        // Tam blok → maksimum artış
        let fee = calc_next_base_fee(INITIAL_BASE_FEE, BLOCK_GAS_LIMIT, GAS_TARGET);
        let max_increase = INITIAL_BASE_FEE / 8; // 12.5%
        assert!(fee <= INITIAL_BASE_FEE + max_increase + 1);
    }

    #[test]
    fn test_base_fee_never_below_minimum() {
        let mut fee = INITIAL_BASE_FEE;
        for _ in 0..1_000 {
            fee = calc_next_base_fee(fee, 0, GAS_TARGET); // boş bloklar
        }
        assert!(fee >= MIN_BASE_FEE);
    }

    #[test]
    fn test_effective_gas_price_normal() {
        let price = effective_gas_price(2_000_000_000, 100_000_000, 1_000_000_000);
        assert_eq!(price, Some(1_100_000_000));
    }

    #[test]
    fn test_effective_gas_price_insufficient_max() {
        // max_fee < base_fee → None
        let price = effective_gas_price(500_000_000, 100_000_000, 1_000_000_000);
        assert_eq!(price, None);
    }

    #[test]
    fn test_effective_gas_price_priority_capped() {
        // priority > max-base → capped
        let price = effective_gas_price(1_100_000_000, 500_000_000, 1_000_000_000);
        // priority = min(500M, 1100M - 1000M = 100M) = 100M
        assert_eq!(price, Some(1_100_000_000));
    }

    #[test]
    fn test_total_fee_calculation() {
        let fee = total_fee(1_100_000_000, 21_000);
        assert_eq!(fee, 1_100_000_000 * 21_000);
    }

    #[test]
    fn test_base_fee_oracle_update() {
        let mut oracle = BaseFeeOracle::new();
        let initial = oracle.current_fee();
        oracle.update(BLOCK_GAS_LIMIT); // tam blok → fee artar
        assert!(oracle.current_fee() > initial);
    }

    #[test]
    fn test_base_fee_oracle_validity() {
        let oracle = BaseFeeOracle::new();
        assert!(oracle.is_tx_valid(INITIAL_BASE_FEE));
        assert!(oracle.is_tx_valid(INITIAL_BASE_FEE * 2));
        assert!(!oracle.is_tx_valid(INITIAL_BASE_FEE - 1));
    }

    // ─── Task 99: Gelişmiş EIP-1559 testleri ─────────────────────────────────

    #[test]
    fn test_fee_history_record_and_average() {
        let mut hist = FeeHistory::new(10);
        for i in 1..=5u64 {
            hist.record(INITIAL_BASE_FEE * i as u128, GAS_TARGET);
        }
        assert_eq!(hist.len(), 5);
        let avg = hist.average_base_fee();
        assert_eq!(avg, INITIAL_BASE_FEE * 3); // (1+2+3+4+5)/5 = 3
    }

    #[test]
    fn test_fee_history_capacity_eviction() {
        let mut hist = FeeHistory::new(3);
        for i in 1..=5u64 {
            hist.record(i as u128, 0);
        }
        assert_eq!(hist.len(), 3); // max capacity
        // Son 3 kayıt: 3, 4, 5
        assert_eq!(hist.average_base_fee(), 4);
    }

    #[test]
    fn test_fee_history_gas_ratio() {
        let mut hist = FeeHistory::new(10);
        hist.record(INITIAL_BASE_FEE, BLOCK_GAS_LIMIT); // %100
        hist.record(INITIAL_BASE_FEE, 0); // %0
        // Ortalama %50 = 5000 bps
        assert_eq!(hist.average_gas_ratio_bps(), 5_000);
    }

    #[test]
    fn test_fee_history_suggest_priority_fee() {
        let mut hist = FeeHistory::new(5);
        hist.record(INITIAL_BASE_FEE * 10, GAS_TARGET);
        let suggested = hist.suggest_priority_fee();
        assert_eq!(suggested, INITIAL_BASE_FEE); // 10x / 10 = 1x
    }

    #[test]
    fn test_multi_dimensional_fee() {
        let fee = calc_multi_dimensional_fee(INITIAL_BASE_FEE, 21_000, 100);
        assert_eq!(fee.execution_fee, INITIAL_BASE_FEE * 21_000);
        assert_eq!(fee.data_fee, INITIAL_BASE_FEE * 100 * 16);
        assert_eq!(fee.total(), fee.execution_fee + fee.data_fee);
    }

    #[test]
    fn test_multi_dimensional_fee_zero_data() {
        let fee = calc_multi_dimensional_fee(INITIAL_BASE_FEE, 21_000, 0);
        assert_eq!(fee.data_fee, 0);
        assert_eq!(fee.total(), fee.execution_fee);
    }

    #[test]
    fn test_fee_history_empty() {
        let hist = FeeHistory::new(10);
        assert!(hist.is_empty());
        assert_eq!(hist.average_base_fee(), 0);
        assert_eq!(hist.average_gas_ratio_bps(), 0);
    }
}
