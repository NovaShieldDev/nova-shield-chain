//! Gas (compute unit) metrikleri — Solana ComputeBudget pattern'inden ilham
//!
//! Solana: `compute_budget.rs` — her opcode için compute unit maliyeti
//! Nova VM: WASM opcode başına sabit maliyet tablosu

use crate::errors::{VmError, VmResult};
use serde::{Deserialize, Serialize};

// ─── Gas Maliyetleri ──────────────────────────────────────────────────────────

/// Temel WASM opcode gas maliyetleri
pub const GAS_PER_WASM_OP: u64 = 1;
/// Bellek erişimi gas maliyeti (load/store)
pub const GAS_PER_MEMORY_OP: u64 = 3;
/// Host function çağrısı gas maliyeti
pub const GAS_PER_HOST_CALL: u64 = 100;
/// Storage okuma maliyeti
pub const GAS_PER_STORAGE_READ: u64 = 1_000;
/// Storage yazma maliyeti
pub const GAS_PER_STORAGE_WRITE: u64 = 5_000;
/// Kontrat deploy gas maliyeti (bytecode başına)
pub const GAS_PER_DEPLOY_BYTE: u64 = 10;
/// Maksimum call derinliği
pub const MAX_CALL_DEPTH: u32 = 128;
/// Varsayılan gas limiti (deploy)
pub const DEFAULT_DEPLOY_GAS_LIMIT: u64 = 5_000_000;
/// Varsayılan gas limiti (call)
pub const DEFAULT_CALL_GAS_LIMIT: u64 = 1_000_000;

// ─── Host Fonksiyon Boyut Limitleri ──────────────────────────────────────────

/// Storage key maksimum boyutu (bayt) — DoS koruması
pub const MAX_STORAGE_KEY_SIZE: usize = 256;
/// Storage value maksimum boyutu (bayt) — DoS koruması
pub const MAX_STORAGE_VALUE_SIZE: usize = 64 * 1024; // 64 KiB
/// Event data maksimum boyutu (bayt)
pub const MAX_EVENT_DATA_SIZE: usize = 4 * 1024; // 4 KiB
/// Log mesajı maksimum boyutu (bayt)
pub const MAX_LOG_MESSAGE_SIZE: usize = 1024;
/// Storage yazma gas'ı: sabit + KB başına ek gas
pub const GAS_PER_STORAGE_WRITE_BYTE: u64 = 1; // bayt başına

// ─── ComputeBudget ────────────────────────────────────────────────────────────

/// Gas bütçesi — Solana ComputeBudget
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeBudget {
    /// Maksimum gas (işlem başına)
    pub max_compute_units: u64,
    /// Bellek limiti (bayt)
    pub max_memory_bytes: usize,
    /// Maksimum çağrı derinliği
    pub max_call_depth: u32,
    /// Log satırı başına max uzunluk
    pub max_log_data_size: usize,
}

impl Default for ComputeBudget {
    fn default() -> Self {
        Self {
            max_compute_units: DEFAULT_CALL_GAS_LIMIT,
            max_memory_bytes: 4 * 1024 * 1024, // 4 MiB
            max_call_depth: MAX_CALL_DEPTH,
            max_log_data_size: 1024,
        }
    }
}

impl ComputeBudget {
    pub fn new(max_compute_units: u64) -> Self {
        Self { max_compute_units, ..Default::default() }
    }

    pub fn for_deploy(bytecode_size: usize) -> Self {
        Self {
            max_compute_units: DEFAULT_DEPLOY_GAS_LIMIT + bytecode_size as u64 * GAS_PER_DEPLOY_BYTE,
            ..Default::default()
        }
    }
}

// ─── GasMeter ─────────────────────────────────────────────────────────────────

/// Gas sayacı — yürütme sırasında tüketimi takip eder
#[derive(Debug, Clone)]
pub struct GasMeter {
    limit: u64,
    used: u64,
}

impl GasMeter {
    pub fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    /// Gas tüket — yetersizse Err döner
    pub fn consume(&mut self, amount: u64) -> VmResult<()> {
        let new_used = self.used.saturating_add(amount);
        if new_used > self.limit {
            return Err(VmError::OutOfGas { limit: self.limit, used: new_used });
        }
        self.used = new_used;
        Ok(())
    }

    /// Kalan gas
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }

    /// Kullanılan gas
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Gas limiti
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Kullanım oranı (0-10_000 bps)
    pub fn usage_bps(&self) -> u64 {
        if self.limit == 0 {
            return 10_000;
        }
        self.used.saturating_mul(10_000) / self.limit
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_meter_consume() {
        let mut meter = GasMeter::new(1000);
        meter.consume(400).unwrap();
        assert_eq!(meter.used(), 400);
        assert_eq!(meter.remaining(), 600);
    }

    #[test]
    fn test_gas_meter_out_of_gas() {
        let mut meter = GasMeter::new(100);
        assert!(matches!(meter.consume(101), Err(VmError::OutOfGas { .. })));
    }

    #[test]
    fn test_gas_meter_exact_limit() {
        let mut meter = GasMeter::new(100);
        meter.consume(100).unwrap();
        assert_eq!(meter.remaining(), 0);
    }

    #[test]
    fn test_gas_meter_usage_bps() {
        let mut meter = GasMeter::new(1000);
        meter.consume(500).unwrap();
        assert_eq!(meter.usage_bps(), 5_000); // %50
    }

    #[test]
    fn test_compute_budget_defaults() {
        let budget = ComputeBudget::default();
        assert_eq!(budget.max_compute_units, DEFAULT_CALL_GAS_LIMIT);
        assert_eq!(budget.max_call_depth, MAX_CALL_DEPTH);
    }

    #[test]
    fn test_compute_budget_for_deploy() {
        let budget = ComputeBudget::for_deploy(1000);
        assert!(budget.max_compute_units > DEFAULT_DEPLOY_GAS_LIMIT);
    }

    #[test]
    fn test_gas_meter_multiple_consume() {
        let mut meter = GasMeter::new(1000);
        for _ in 0..5 {
            meter.consume(100).unwrap();
        }
        assert_eq!(meter.used(), 500);
        // 501 daha tüketmeye çalış → hata
        assert!(meter.consume(501).is_err());
    }
}
