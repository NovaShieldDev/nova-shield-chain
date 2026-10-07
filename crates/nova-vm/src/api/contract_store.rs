//! Kontrat depolama — on-chain bytecode + metadata
//!
//! Cosmos SDK x/wasm Keeper: `ContractInfo` + `CodeInfo` pattern.

use crate::errors::{VmError, VmResult};
use dashmap::DashMap;
use nova_primitives::Address;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── ContractType ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractType {
    NovaWasm,
    Evm,
}

// ─── ContractCode ─────────────────────────────────────────────────────────────

/// Bytecode deposu — her kontrat için bir kayıt
/// Cosmos: `CodeInfo` + `CodeStore`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractCode {
    pub code_id: u64,
    /// Bytecode hash (BLAKE3)
    pub code_hash: [u8; 32],
    /// Ham bytecode
    pub bytecode: Vec<u8>,
    pub contract_type: ContractType,
    pub uploader: Address,
    pub uploaded_at_slot: u64,
}

impl ContractCode {
    pub fn code_hash_hex(&self) -> String {
        hex::encode(self.code_hash)
    }
}

// ─── ContractInfo ─────────────────────────────────────────────────────────────

/// Instantiate edilmiş kontrat bilgisi
/// Cosmos: `ContractInfo`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractInfo {
    pub address: Address,
    pub code_id: u64,
    pub creator: Address,
    pub label: String,
    pub created_at_slot: u64,
    pub contract_type: ContractType,
}

// ─── ContractStore ────────────────────────────────────────────────────────────

/// Kontrat deposu — bytecode + instantiated contracts
#[derive(Debug, Default)]
pub struct ContractStore {
    /// code_id → bytecode
    codes: Arc<DashMap<u64, ContractCode>>,
    /// address → contract info
    contracts: Arc<DashMap<Address, ContractInfo>>,
    next_code_id: Arc<AtomicU64>,
}

impl ContractStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytecode yükle — code_id döner
    pub fn upload_code(
        &self,
        bytecode: Vec<u8>,
        contract_type: ContractType,
        uploader: Address,
        slot: u64,
    ) -> VmResult<u64> {
        if bytecode.is_empty() {
            return Err(VmError::InvalidBytecode { reason: "boş bytecode".into() });
        }
        let code_id = self.next_code_id.fetch_add(1, Ordering::SeqCst);
        let code_hash = *blake3::hash(&bytecode).as_bytes();
        let code = ContractCode {
            code_id,
            code_hash,
            bytecode,
            contract_type,
            uploader,
            uploaded_at_slot: slot,
        };
        self.codes.insert(code_id, code);
        Ok(code_id)
    }

    /// Kontrat instantiate et
    pub fn instantiate(
        &self,
        address: Address,
        code_id: u64,
        creator: Address,
        label: String,
        slot: u64,
    ) -> VmResult<()> {
        // Code var mı?
        let code = self.codes.get(&code_id).ok_or_else(|| VmError::ContractNotFound {
            address_hex: format!("code_id={code_id}"),
        })?;

        if self.contracts.contains_key(&address) {
            return Err(VmError::ContractAlreadyExists {
                address_hex: hex::encode(address.as_bytes()),
            });
        }

        let info = ContractInfo {
            address,
            code_id,
            creator,
            label,
            created_at_slot: slot,
            contract_type: code.contract_type.clone(),
        };
        self.contracts.insert(address, info);
        Ok(())
    }

    /// Bytecode al
    pub fn get_code(&self, code_id: u64) -> VmResult<ContractCode> {
        self.codes
            .get(&code_id)
            .map(|c| c.clone())
            .ok_or_else(|| VmError::ContractNotFound { address_hex: format!("code_id={code_id}") })
    }

    /// Kontrat bilgisi al
    pub fn get_contract(&self, address: &Address) -> VmResult<ContractInfo> {
        self.contracts
            .get(address)
            .map(|c| c.clone())
            .ok_or_else(|| VmError::ContractNotFound {
                address_hex: hex::encode(address.as_bytes()),
            })
    }

    /// Bytecode — kontrat adresinden
    pub fn get_bytecode_for_contract(&self, address: &Address) -> VmResult<Vec<u8>> {
        let info = self.get_contract(address)?;
        let code = self.get_code(info.code_id)?;
        Ok(code.bytecode)
    }

    /// Toplam yüklü kod sayısı
    pub fn code_count(&self) -> usize {
        self.codes.len()
    }

    /// Toplam instantiate edilmiş kontrat sayısı
    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::runtime::WASM_MAGIC;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn valid_wasm() -> Vec<u8> {
        let mut v = WASM_MAGIC.to_vec();
        v.extend_from_slice(&[0u8; 50]);
        v
    }

    #[test]
    fn test_upload_and_get_code() {
        let store = ContractStore::new();
        let code_id = store.upload_code(valid_wasm(), ContractType::NovaWasm, addr(1), 100).unwrap();
        let code = store.get_code(code_id).unwrap();
        assert_eq!(code.code_id, code_id);
        assert_eq!(code.contract_type, ContractType::NovaWasm);
        assert_eq!(code.uploader, addr(1));
    }

    #[test]
    fn test_upload_empty_bytecode() {
        let store = ContractStore::new();
        assert!(matches!(
            store.upload_code(vec![], ContractType::NovaWasm, addr(1), 0),
            Err(VmError::InvalidBytecode { .. })
        ));
    }

    #[test]
    fn test_instantiate_contract() {
        let store = ContractStore::new();
        let code_id = store.upload_code(valid_wasm(), ContractType::NovaWasm, addr(1), 0).unwrap();
        store.instantiate(addr(10), code_id, addr(1), "token".into(), 100).unwrap();

        let info = store.get_contract(&addr(10)).unwrap();
        assert_eq!(info.creator, addr(1));
        assert_eq!(info.label, "token");
        assert_eq!(store.contract_count(), 1);
    }

    #[test]
    fn test_instantiate_duplicate_address() {
        let store = ContractStore::new();
        let code_id = store.upload_code(valid_wasm(), ContractType::NovaWasm, addr(1), 0).unwrap();
        store.instantiate(addr(10), code_id, addr(1), "first".into(), 0).unwrap();
        let err = store.instantiate(addr(10), code_id, addr(1), "second".into(), 0);
        assert!(matches!(err, Err(VmError::ContractAlreadyExists { .. })));
    }

    #[test]
    fn test_get_bytecode_for_contract() {
        let store = ContractStore::new();
        let wasm = valid_wasm();
        let code_id = store.upload_code(wasm.clone(), ContractType::NovaWasm, addr(1), 0).unwrap();
        store.instantiate(addr(5), code_id, addr(1), "test".into(), 0).unwrap();

        let bytecode = store.get_bytecode_for_contract(&addr(5)).unwrap();
        assert_eq!(bytecode, wasm);
    }

    #[test]
    fn test_multiple_contracts_same_code() {
        let store = ContractStore::new();
        let code_id = store.upload_code(valid_wasm(), ContractType::NovaWasm, addr(1), 0).unwrap();
        // 3 farklı adres aynı code_id ile
        for i in 10u8..13 {
            store.instantiate(addr(i), code_id, addr(1), format!("c{i}"), 0).unwrap();
        }
        assert_eq!(store.contract_count(), 3);
        assert_eq!(store.code_count(), 1);
    }

    #[test]
    fn test_code_hash_deterministic() {
        let store = ContractStore::new();
        let wasm = valid_wasm();
        let id1 = store.upload_code(wasm.clone(), ContractType::NovaWasm, addr(1), 0).unwrap();
        let id2 = store.upload_code(wasm.clone(), ContractType::NovaWasm, addr(2), 10).unwrap();
        let c1 = store.get_code(id1).unwrap();
        let c2 = store.get_code(id2).unwrap();
        // Aynı bytecode → aynı hash
        assert_eq!(c1.code_hash, c2.code_hash);
    }
}
