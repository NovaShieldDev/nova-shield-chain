//! WASM ↔ EVM köprüsü — iki runtime arası çağrı

use crate::errors::{VmError, VmResult};
use crate::evm::compat::{AbiCodec, EvmCallResult, EvmCompatLayer};
use crate::wasm::runtime::{ExecutionResult, WasmModule, WasmRuntime};
use crate::wasm::host_fns::HostContext;
use crate::wasm::gas::ComputeBudget;
use nova_primitives::Address;
use serde::{Deserialize, Serialize};

// ─── ContractType ─────────────────────────────────────────────────────────────

/// Kontrat türü
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractType {
    /// Nova WASM kontratı (Rust → WASM)
    NovaWasm,
    /// EVM uyumlu kontrat (Solidity)
    Evm,
}

// ─── BridgeCallResult ─────────────────────────────────────────────────────────

/// Köprü çağrısı sonucu — WASM veya EVM'den dönen unified result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeCallResult {
    pub contract_type: ContractType,
    pub output: Vec<u8>,
    pub gas_used: u64,
    pub success: bool,
}

// ─── WasmEvmBridge ────────────────────────────────────────────────────────────

/// WASM ↔ EVM köprüsü — her iki runtime'a unified call interface
pub struct WasmEvmBridge {
    wasm_runtime: WasmRuntime,
    evm_layer: EvmCompatLayer,
}

impl WasmEvmBridge {
    pub fn new() -> VmResult<Self> {
        Ok(Self {
            wasm_runtime: WasmRuntime::new(32)?,
            evm_layer: EvmCompatLayer::new(),
        })
    }

    pub fn deploy_evm_contract(&self, address: Address, bytecode: &[u8]) {
        self.evm_layer.deploy_contract(address, bytecode);
    }

    /// WASM kontratı çağır
    pub fn call_wasm(
        &self,
        bytecode: &[u8],
        method: &str,
        input: &[u8],
        caller: Address,
        contract: Address,
        slot: u64,
        gas_limit: u64,
    ) -> VmResult<BridgeCallResult> {
        let module = self.wasm_runtime.compile(bytecode)?;
        let mut ctx = HostContext::new(caller, contract, slot, 0);
        let budget = ComputeBudget::new(gas_limit);

        let result = self.wasm_runtime.execute(&module, method, input, &mut ctx, budget)?;

        Ok(BridgeCallResult {
            contract_type: ContractType::NovaWasm,
            output: result.output,
            gas_used: result.gas_used,
            success: true,
        })
    }

    /// EVM kontratı çağır
    pub fn call_evm(
        &self,
        caller: Address,
        to: Address,
        calldata: &[u8],
        value: u128,
        gas_limit: u64,
    ) -> VmResult<BridgeCallResult> {
        let result = self.evm_layer.evm_call(caller, to, calldata, value, gas_limit)?;

        Ok(BridgeCallResult {
            contract_type: ContractType::Evm,
            output: result.return_data,
            gas_used: result.gas_used,
            success: result.success,
        })
    }

    /// WASM kontratından EVM kontratı çağır (cross-runtime CPI)
    pub fn wasm_to_evm(
        &self,
        caller: Address,
        evm_contract: Address,
        selector: &str,
        args_encoded: &[u8],
        gas_limit: u64,
    ) -> VmResult<BridgeCallResult> {
        let sel = AbiCodec::function_selector(selector);
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(args_encoded);

        self.call_evm(caller, evm_contract, &calldata, 0, gas_limit)
    }

    pub fn wasm_runtime(&self) -> &WasmRuntime {
        &self.wasm_runtime
    }
}

impl Default for WasmEvmBridge {
    fn default() -> Self {
        Self::new().expect("WasmEvmBridge başlatılamadı")
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evm::compat::MOCK_ERC20_BYTECODE;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    /// Gerçek wasmtime ile derlenebilir minimal WASM modülü
    /// execute(i32, i32) -> i32 export eder
    fn minimal_execute_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "execute") (param i32 i32) (result i32)
                    i32.const 0
                )
            )
            "#,
        )
        .expect("WAT parse hatası")
    }

    #[test]
    fn test_bridge_wasm_call() {
        let bridge = WasmEvmBridge::new().unwrap();
        let result = bridge.call_wasm(
            &minimal_execute_wasm(),
            "execute",
            b"hello",
            addr(1),
            addr(2),
            1000,
            500_000,
        ).unwrap();
        assert!(result.success);
        assert_eq!(result.contract_type, ContractType::NovaWasm);
        assert!(result.gas_used > 0);
    }

    #[test]
    fn test_bridge_evm_call() {
        let bridge = WasmEvmBridge::new().unwrap();
        // Mock ERC-20 deploy et
        bridge.deploy_evm_contract(addr(2), MOCK_ERC20_BYTECODE);

        let sel = AbiCodec::function_selector("transfer(address,uint256)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&AbiCodec::encode_address(&addr(3)));
        calldata.extend_from_slice(&AbiCodec::encode_uint256(1000));

        let result = bridge.call_evm(addr(1), addr(2), &calldata, 0, 100_000).unwrap();
        assert_eq!(result.contract_type, ContractType::Evm);
        assert!(result.success, "ERC-20 transfer başarılı olmalı");
    }

    #[test]
    fn test_wasm_to_evm_cross_call() {
        let bridge = WasmEvmBridge::new().unwrap();
        // EVM kontratını deploy et
        bridge.deploy_evm_contract(addr(10), MOCK_ERC20_BYTECODE);

        let mut args = AbiCodec::encode_address(&addr(3)).to_vec();
        args.extend_from_slice(&AbiCodec::encode_uint256(500));

        let result = bridge.wasm_to_evm(
            addr(1),
            addr(10),
            "transfer(address,uint256)",
            &args,
            100_000,
        ).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_bridge_invalid_wasm() {
        let bridge = WasmEvmBridge::new().unwrap();
        let bad_bytecode = vec![0xFF; 100];
        let result = bridge.call_wasm(&bad_bytecode, "execute", &[], addr(1), addr(2), 0, 100_000);
        assert!(matches!(result, Err(VmError::InvalidBytecode { .. })));
    }
}
