//! Nova VM host fonksiyonları — Solana syscall pattern
//!
//! Solana: `declare_builtin_function!` makrosu ile syscall'lar tanımlanır.
//! Nova: Her host function bir enum variant — kontratlar bu API'ye erişir.
//!
//! Nova özel host fonksiyonları (§9.1):
//! - `nova_mpc_call`: MPC hesap operasyonu
//! - `nova_batch_price`: Batch auction clearing price
//! - `nova_guardian_check`: Guardian durumu sorgulama
//! - `nova_emit_event`: Zincir üstü event yayınlama

use crate::errors::{VmError, VmResult};
use crate::wasm::gas::{
    GasMeter, GAS_PER_HOST_CALL, GAS_PER_STORAGE_READ, GAS_PER_STORAGE_WRITE,
    GAS_PER_STORAGE_WRITE_BYTE,
    MAX_EVENT_DATA_SIZE, MAX_LOG_MESSAGE_SIZE, MAX_STORAGE_KEY_SIZE, MAX_STORAGE_VALUE_SIZE,
};
use nova_primitives::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── HostCallType ─────────────────────────────────────────────────────────────

/// Desteklenen host function türleri
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HostCallType {
    /// Kontrat için depolama okuma
    StorageRead,
    /// Kontrat için depolama yazma
    StorageWrite,
    /// Zincir üstü event yayınla
    EmitEvent,
    /// MPC hesap sorgulama (§9.1)
    NovaMpcQuery,
    /// Batch clearing price (§9.1)
    NovaBatchPrice,
    /// Guardian durum sorgulama (§9.1)
    NovaGuardianCheck,
    /// Kriptografik hash (BLAKE3)
    Blake3Hash,
    /// Mevcut slot numarası
    CurrentSlot,
    /// Gönderen adresi
    CallerAddress,
    /// Log/debug mesajı
    Log,
}

impl HostCallType {
    /// Gas maliyeti
    pub fn gas_cost(&self) -> u64 {
        match self {
            HostCallType::StorageRead => GAS_PER_STORAGE_READ,
            HostCallType::StorageWrite => GAS_PER_STORAGE_WRITE,
            HostCallType::EmitEvent => GAS_PER_HOST_CALL * 5,
            HostCallType::NovaMpcQuery => GAS_PER_HOST_CALL * 10,
            HostCallType::NovaBatchPrice => GAS_PER_HOST_CALL * 2,
            HostCallType::NovaGuardianCheck => GAS_PER_HOST_CALL * 3,
            HostCallType::Blake3Hash => GAS_PER_HOST_CALL,
            HostCallType::CurrentSlot => GAS_PER_HOST_CALL / 10,
            HostCallType::CallerAddress => GAS_PER_HOST_CALL / 10,
            HostCallType::Log => GAS_PER_HOST_CALL / 5,
        }
    }

    /// İsim
    pub fn name(&self) -> &'static str {
        match self {
            HostCallType::StorageRead => "nova_storage_read",
            HostCallType::StorageWrite => "nova_storage_write",
            HostCallType::EmitEvent => "nova_emit_event",
            HostCallType::NovaMpcQuery => "nova_mpc_query",
            HostCallType::NovaBatchPrice => "nova_batch_price",
            HostCallType::NovaGuardianCheck => "nova_guardian_check",
            HostCallType::Blake3Hash => "nova_blake3",
            HostCallType::CurrentSlot => "nova_current_slot",
            HostCallType::CallerAddress => "nova_caller",
            HostCallType::Log => "nova_log",
        }
    }
}

// ─── HostContext ──────────────────────────────────────────────────────────────

/// Yürütme sırasındaki zincir bağlamı — Solana InvokeContext benzeri
#[derive(Debug, Clone)]
pub struct HostContext {
    /// Çağıran adres
    pub caller: Address,
    /// Mevcut kontrat adresi
    pub contract: Address,
    /// Mevcut slot
    pub current_slot: u64,
    /// Mesaj değeri (gönderilen NSC)
    pub value: u128,
    /// Kontrat deposu (in-memory: prod'da RocksDB)
    pub storage: HashMap<Vec<u8>, Vec<u8>>,
    /// Yayınlanan eventler
    pub events: Vec<ContractEvent>,
    /// Log mesajları
    pub logs: Vec<String>,
}

impl HostContext {
    pub fn new(caller: Address, contract: Address, slot: u64, value: u128) -> Self {
        Self {
            caller,
            contract,
            current_slot: slot,
            value,
            storage: HashMap::new(),
            events: Vec::new(),
            logs: Vec::new(),
        }
    }

    pub fn storage_read(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.storage.get(key).cloned()
    }

    pub fn storage_write(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.storage.insert(key, value);
    }

    pub fn emit_event(&mut self, event: ContractEvent) {
        self.events.push(event);
    }

    pub fn log(&mut self, message: String) {
        self.logs.push(message);
    }
}

// ─── ContractEvent ────────────────────────────────────────────────────────────

/// Kontrat event — EVM Log benzeri
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractEvent {
    pub contract: Address,
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
    pub slot: u64,
}

// ─── HostFunctionRegistry ─────────────────────────────────────────────────────

/// Desteklenen host function'ları kayıt defteri
/// Solana: `FunctionRegistry<InvokeContext>` pattern'inden ilham
#[derive(Debug, Default)]
pub struct HostFunctionRegistry {
    registered: HashMap<String, HostCallType>,
}

impl HostFunctionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tüm Nova host fonksiyonlarını kayıt et
    pub fn register_nova_functions(&mut self) {
        let all = [
            HostCallType::StorageRead,
            HostCallType::StorageWrite,
            HostCallType::EmitEvent,
            HostCallType::NovaMpcQuery,
            HostCallType::NovaBatchPrice,
            HostCallType::NovaGuardianCheck,
            HostCallType::Blake3Hash,
            HostCallType::CurrentSlot,
            HostCallType::CallerAddress,
            HostCallType::Log,
        ];
        for call_type in all {
            self.registered.insert(call_type.name().to_string(), call_type);
        }
    }

    pub fn get(&self, name: &str) -> Option<&HostCallType> {
        self.registered.get(name)
    }

    pub fn registered_count(&self) -> usize {
        self.registered.len()
    }
}

// ─── NovaHostFns ──────────────────────────────────────────────────────────────

/// Host function yürütücü — her çağrıyı gas'ı düşerek işler
pub struct NovaHostFns;

impl NovaHostFns {
    /// Host function çağrısını yürüt
    pub fn call(
        call_type: &HostCallType,
        ctx: &mut HostContext,
        meter: &mut GasMeter,
        input: &[u8],
    ) -> VmResult<Vec<u8>> {
        // Gas düş
        meter.consume(call_type.gas_cost())?;

        match call_type {
            HostCallType::StorageRead => {
                if input.len() > MAX_STORAGE_KEY_SIZE {
                    return Err(VmError::InvalidInput);
                }
                let value = ctx.storage_read(input).unwrap_or_default();
                Ok(value)
            }

            HostCallType::StorageWrite => {
                // input: [4-byte key_len][key][value]
                if input.len() < 4 {
                    return Err(VmError::InvalidInput);
                }
                let key_len = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
                if key_len > MAX_STORAGE_KEY_SIZE || input.len() < 4 + key_len {
                    return Err(VmError::InvalidInput);
                }
                let key = input[4..4 + key_len].to_vec();
                let value = input[4 + key_len..].to_vec();
                if value.len() > MAX_STORAGE_VALUE_SIZE {
                    return Err(VmError::InvalidInput);
                }
                // Bayt başına ek gas — sabit gas zaten tüketildi
                meter.consume(value.len() as u64 * GAS_PER_STORAGE_WRITE_BYTE)?;
                ctx.storage_write(key, value);
                Ok(vec![1]) // success
            }

            HostCallType::EmitEvent => {
                if input.len() > MAX_EVENT_DATA_SIZE {
                    return Err(VmError::InvalidInput);
                }
                let event = ContractEvent {
                    contract: ctx.contract,
                    topics: Vec::new(),
                    data: input.to_vec(),
                    slot: ctx.current_slot,
                };
                ctx.emit_event(event);
                Ok(vec![1])
            }

            HostCallType::NovaMpcQuery => {
                // MPC hesap bilgisi — mock: her zaman "active" döner
                Ok(b"mpc:active".to_vec())
            }

            HostCallType::NovaBatchPrice => {
                // Clearing price — mock: slot bazlı fiyat
                let price: u64 = ctx.current_slot.wrapping_mul(1_000_000);
                Ok(price.to_le_bytes().to_vec())
            }

            HostCallType::NovaGuardianCheck => {
                // Guardian durumu — input: guardian adres (20 byte)
                if input.len() < 20 {
                    return Err(VmError::InvalidInput);
                }
                // Mock: her zaman "active" döner
                Ok(b"guardian:active".to_vec())
            }

            HostCallType::Blake3Hash => {
                let hash = blake3::hash(input);
                Ok(hash.as_bytes().to_vec())
            }

            HostCallType::CurrentSlot => Ok(ctx.current_slot.to_le_bytes().to_vec()),

            HostCallType::CallerAddress => Ok(ctx.caller.as_bytes().to_vec()),

            HostCallType::Log => {
                let input = if input.len() > MAX_LOG_MESSAGE_SIZE {
                    &input[..MAX_LOG_MESSAGE_SIZE]
                } else {
                    input
                };
                let msg = String::from_utf8_lossy(input).to_string();
                ctx.log(msg);
                Ok(vec![1])
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nova_primitives::SlotId;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn ctx() -> HostContext {
        HostContext::new(addr(1), addr(2), 1000, 0)
    }

    fn meter() -> GasMeter {
        GasMeter::new(1_000_000)
    }

    #[test]
    fn test_storage_write_and_read() {
        let mut ctx = ctx();
        let mut m = meter();

        // Yaz
        let key = b"my_key";
        let val = b"my_value";
        let mut input = (key.len() as u32).to_le_bytes().to_vec();
        input.extend_from_slice(key);
        input.extend_from_slice(val);
        NovaHostFns::call(&HostCallType::StorageWrite, &mut ctx, &mut m, &input).unwrap();

        // Oku
        let result = NovaHostFns::call(&HostCallType::StorageRead, &mut ctx, &mut m, key).unwrap();
        assert_eq!(result, val);
    }

    #[test]
    fn test_current_slot() {
        let mut ctx = ctx();
        let mut m = meter();
        let result = NovaHostFns::call(&HostCallType::CurrentSlot, &mut ctx, &mut m, &[]).unwrap();
        let slot = u64::from_le_bytes(result.try_into().unwrap());
        assert_eq!(slot, 1000);
    }

    #[test]
    fn test_blake3_hash() {
        let mut ctx = ctx();
        let mut m = meter();
        let data = b"nova-shield";
        let result = NovaHostFns::call(&HostCallType::Blake3Hash, &mut ctx, &mut m, data).unwrap();
        assert_eq!(result.len(), 32);
        // Aynı girdi aynı hash
        let result2 = NovaHostFns::call(&HostCallType::Blake3Hash, &mut ctx, &mut m, data).unwrap();
        assert_eq!(result, result2);
    }

    #[test]
    fn test_emit_event() {
        let mut ctx = ctx();
        let mut m = meter();
        let data = b"transfer(100,user)";
        NovaHostFns::call(&HostCallType::EmitEvent, &mut ctx, &mut m, data).unwrap();
        assert_eq!(ctx.events.len(), 1);
        assert_eq!(ctx.events[0].data, data);
    }

    #[test]
    fn test_log() {
        let mut ctx = ctx();
        let mut m = meter();
        NovaHostFns::call(&HostCallType::Log, &mut ctx, &mut m, b"hello from contract").unwrap();
        assert_eq!(ctx.logs.len(), 1);
        assert!(ctx.logs[0].contains("hello"));
    }

    #[test]
    fn test_host_fn_registry() {
        let mut reg = HostFunctionRegistry::new();
        reg.register_nova_functions();
        assert_eq!(reg.registered_count(), 10);
        assert!(reg.get("nova_storage_read").is_some());
        assert!(reg.get("nova_mpc_query").is_some());
        assert!(reg.get("nova_batch_price").is_some());
    }

    #[test]
    fn test_gas_consumed_per_call() {
        let mut ctx = ctx();
        let mut m = GasMeter::new(GAS_PER_HOST_CALL * 2);
        // İlk çağrı geçer
        NovaHostFns::call(&HostCallType::Blake3Hash, &mut ctx, &mut m, b"x").unwrap();
        // İkinci çağrı gas tükenimine yaklaşır (kalan = tam GAS_PER_HOST_CALL veya daha az)
        assert!(m.remaining() <= GAS_PER_HOST_CALL);
    }

    #[test]
    fn test_nova_mpc_query() {
        let mut ctx = ctx();
        let mut m = meter();
        let result = NovaHostFns::call(&HostCallType::NovaMpcQuery, &mut ctx, &mut m, &[]).unwrap();
        assert_eq!(result, b"mpc:active");
    }

    #[test]
    fn test_nova_guardian_check() {
        let mut ctx = ctx();
        let mut m = meter();
        let guardian = addr(50);
        let result = NovaHostFns::call(&HostCallType::NovaGuardianCheck, &mut ctx, &mut m, guardian.as_bytes()).unwrap();
        assert_eq!(result, b"guardian:active");
    }

    #[test]
    fn test_caller_address() {
        let mut ctx = ctx();
        let mut m = meter();
        let result = NovaHostFns::call(&HostCallType::CallerAddress, &mut ctx, &mut m, &[]).unwrap();
        assert_eq!(result, addr(1).as_bytes());
    }
}
