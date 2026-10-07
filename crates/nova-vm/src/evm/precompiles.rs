//! EVM precompile'lar — Avalanchego Subnet-EVM pattern'inden ilham
//!
//! Avalanchego: `modules.RegisteredModules()` ile precompile registry.
//! Nova: Her precompile bir `NovaPrecompile` implementasyonu.

use crate::errors::{VmError, VmResult};
use nova_primitives::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Precompile Adresleri ─────────────────────────────────────────────────────

/// MPC hesap precompile adresi: 0x0000...0001
pub const MPC_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x01;
    a
};

/// Batch auction precompile: 0x0000...0002
pub const BATCH_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x02;
    a
};

/// Guardian precompile: 0x0000...0003
pub const GUARDIAN_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x03;
    a
};

/// ZK-Shield precompile: 0x0000...0004
pub const ZK_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x04;
    a
};

/// BLAKE3 hash precompile: 0x0000...0005
pub const BLAKE3_PRECOMPILE_ADDR: [u8; 20] = {
    let mut a = [0u8; 20];
    a[19] = 0x05;
    a
};

// ─── NovaPrecompile ───────────────────────────────────────────────────────────

/// Precompile türü
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NovaPrecompile {
    /// MPC hesap operasyonları
    MpcAccount,
    /// Batch clearing price sorgulama
    BatchAuction,
    /// Guardian durum sorgulama
    GuardianCheck,
    /// ZK kanıt doğrulama
    ZkVerifier,
    /// BLAKE3 hash hesaplama
    Blake3Hash,
}

impl NovaPrecompile {
    /// Precompile gas maliyeti
    pub fn gas_cost(&self, input_len: usize) -> u64 {
        let base = match self {
            NovaPrecompile::MpcAccount => 5_000,
            NovaPrecompile::BatchAuction => 1_000,
            NovaPrecompile::GuardianCheck => 2_000,
            NovaPrecompile::ZkVerifier => 50_000,
            NovaPrecompile::Blake3Hash => 500 + input_len as u64 * 3,
        };
        base
    }

    /// Precompile adı
    pub fn name(&self) -> &'static str {
        match self {
            NovaPrecompile::MpcAccount => "nova-mpc",
            NovaPrecompile::BatchAuction => "nova-batch",
            NovaPrecompile::GuardianCheck => "nova-guardian",
            NovaPrecompile::ZkVerifier => "nova-zk-verify",
            NovaPrecompile::Blake3Hash => "nova-blake3",
        }
    }

    /// Mock yürütme
    pub fn call(&self, input: &[u8], caller: &Address) -> VmResult<Vec<u8>> {
        match self {
            NovaPrecompile::MpcAccount => {
                // input: [20 byte account_addr]
                if input.len() < 20 {
                    return Err(VmError::InvalidInput);
                }
                // Mock: "active" durum döner
                Ok(b"\x01".to_vec()) // 1 = active
            }

            NovaPrecompile::BatchAuction => {
                // input: [8 byte slot_id]
                if input.len() < 8 {
                    return Err(VmError::InvalidInput);
                }
                let slot = u64::from_le_bytes(input[..8].try_into().unwrap());
                let price = slot.wrapping_mul(1_000_000_u64);
                Ok(price.to_le_bytes().to_vec())
            }

            NovaPrecompile::GuardianCheck => {
                // input: [20 byte guardian_addr]
                if input.len() < 20 {
                    return Err(VmError::InvalidInput);
                }
                Ok(b"\x01".to_vec()) // 1 = active
            }

            NovaPrecompile::ZkVerifier => {
                // input: [proof bytes]
                if input.is_empty() {
                    return Err(VmError::InvalidInput);
                }
                // Mock: BLAKE3 ile proof hash kontrol et
                let hash = blake3::hash(input);
                let valid = hash.as_bytes()[0] < 200; // Mock validity: %78
                Ok(vec![valid as u8])
            }

            NovaPrecompile::Blake3Hash => {
                let hash = blake3::hash(input);
                Ok(hash.as_bytes().to_vec())
            }
        }
    }
}

// ─── PrecompileRegistry ───────────────────────────────────────────────────────

/// Precompile kayıt defteri — Avalanchego RegisteredModules pattern
#[derive(Debug)]
pub struct PrecompileRegistry {
    precompiles: HashMap<[u8; 20], NovaPrecompile>,
}

impl PrecompileRegistry {
    pub fn new() -> Self {
        let mut precompiles = HashMap::new();
        precompiles.insert(MPC_PRECOMPILE_ADDR, NovaPrecompile::MpcAccount);
        precompiles.insert(BATCH_PRECOMPILE_ADDR, NovaPrecompile::BatchAuction);
        precompiles.insert(GUARDIAN_PRECOMPILE_ADDR, NovaPrecompile::GuardianCheck);
        precompiles.insert(ZK_PRECOMPILE_ADDR, NovaPrecompile::ZkVerifier);
        precompiles.insert(BLAKE3_PRECOMPILE_ADDR, NovaPrecompile::Blake3Hash);
        Self { precompiles }
    }

    pub fn get(&self, address: &[u8; 20]) -> Option<&NovaPrecompile> {
        self.precompiles.get(address)
    }

    pub fn is_precompile(&self, address: &[u8; 20]) -> bool {
        self.precompiles.contains_key(address)
    }

    pub fn count(&self) -> usize {
        self.precompiles.len()
    }

    /// Precompile çağrısı yürüt
    pub fn call(
        &self,
        address: &[u8; 20],
        input: &[u8],
        caller: &Address,
    ) -> VmResult<(Vec<u8>, u64)> {
        let precompile = self
            .precompiles
            .get(address)
            .ok_or_else(|| VmError::ContractNotFound {
                address_hex: hex::encode(address),
            })?;

        let gas = precompile.gas_cost(input.len());
        let output = precompile.call(input, caller)?;
        Ok((output, gas))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_precompile_registry_init() {
        let reg = PrecompileRegistry::new();
        assert_eq!(reg.count(), 5);
        assert!(reg.is_precompile(&MPC_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&BATCH_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&GUARDIAN_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&ZK_PRECOMPILE_ADDR));
        assert!(reg.is_precompile(&BLAKE3_PRECOMPILE_ADDR));
    }

    #[test]
    fn test_precompile_non_existent() {
        let reg = PrecompileRegistry::new();
        let unknown = [0xFF; 20];
        assert!(!reg.is_precompile(&unknown));
    }

    #[test]
    fn test_mpc_precompile() {
        let precompile = NovaPrecompile::MpcAccount;
        let input = [1u8; 20]; // valid account address
        let result = precompile.call(&input, &addr(1)).unwrap();
        assert_eq!(result, vec![1]); // active
    }

    #[test]
    fn test_batch_precompile() {
        let precompile = NovaPrecompile::BatchAuction;
        let slot: u64 = 1000;
        let result = precompile.call(&slot.to_le_bytes(), &addr(1)).unwrap();
        let price = u64::from_le_bytes(result.try_into().unwrap());
        assert_eq!(price, 1000 * 1_000_000);
    }

    #[test]
    fn test_blake3_precompile() {
        let precompile = NovaPrecompile::Blake3Hash;
        let data = b"nova-shield-chain";
        let result = precompile.call(data, &addr(1)).unwrap();
        assert_eq!(result.len(), 32);
        // Deterministic
        let result2 = precompile.call(data, &addr(1)).unwrap();
        assert_eq!(result, result2);
    }

    #[test]
    fn test_guardian_precompile() {
        let precompile = NovaPrecompile::GuardianCheck;
        let guardian = [2u8; 20];
        let result = precompile.call(&guardian, &addr(1)).unwrap();
        assert_eq!(result, vec![1]); // active
    }

    #[test]
    fn test_precompile_gas_cost() {
        let p = NovaPrecompile::Blake3Hash;
        let gas1 = p.gas_cost(0);
        let gas2 = p.gas_cost(100);
        assert!(gas2 > gas1);
    }

    #[test]
    fn test_registry_call() {
        let reg = PrecompileRegistry::new();
        let (output, gas) = reg.call(&BLAKE3_PRECOMPILE_ADDR, b"test", &addr(1)).unwrap();
        assert_eq!(output.len(), 32);
        assert!(gas > 0);
    }

    #[test]
    fn test_mpc_precompile_invalid_input() {
        let precompile = NovaPrecompile::MpcAccount;
        let short_input = [0u8; 5]; // too short
        assert!(matches!(precompile.call(&short_input, &addr(1)), Err(VmError::InvalidInput)));
    }
}
