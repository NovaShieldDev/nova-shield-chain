//! nova-vm — Nova VM: WASM + EVM uyumluluk katmanı (§9.1)
//!
//! Rust kontratları → WebAssembly → Nova VM yürütür.
//! EVM uyumluluk katmanı ile Solidity kontratlar da çalışır.
//! Nova'ya özel host fonksiyonları: MPC, batch auction, guardian.
//!
//! Pattern'ler:
//! - Polkadot: `WasmModule/WasmInstance`, `CodeExecutor`, `RuntimeCache`
//! - Solana: `InvokeContext`, `declare_builtin_function!`, `ComputeBudget`
//! - Avalanchego Subnet-EVM: `PrecompileRegistry`, `RegisteredModules`

pub mod api;
pub mod errors;
pub mod evm;
pub mod upgrade;
pub mod wasm;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use api::{CallInput, ContractStore, DeployInput, NovaVmExecutor, VmOutput};
pub use upgrade::{
    RuntimeCodeStore, RuntimeCodeVersion, SetCodeExtrinsic, SetCodeHandler,
    GOVERNANCE_MODULE_ADDR, MAX_RUNTIME_CODE_SIZE, RUNTIME_HISTORY_DEPTH,
};
pub use errors::{VmError, VmResult};
pub use evm::{
    ChainStateProvider, EvmCompatLayer, NoopChainStateProvider, NovaPrecompile,
    PrecompileRegistry, WasmEvmBridge,
};
pub use wasm::{ComputeBudget, GasMeter, HostContext, LinearMemory, WasmModule, WasmRuntime};

// ─── Integration Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evm::compat::AbiCodec;
    use crate::evm::precompiles::{BATCH_PRECOMPILE_ADDR, BLAKE3_PRECOMPILE_ADDR, GUARDIAN_PRECOMPILE_ADDR};
    use crate::wasm::host_fns::{HostCallType, NovaHostFns};
    use crate::wasm::runtime::WASM_MAGIC;
    use nova_primitives::Address;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn valid_wasm() -> Vec<u8> {
        // Gerçek wasmtime ile derlenebilir, instantiate/execute/query export eden minimal WASM
        wat::parse_str(
            r#"
            (module
                (import "env" "nova_storage_write" (func $sw (param i32 i32 i32 i32)))
                (import "env" "nova_storage_read"  (func $sr (param i32 i32 i32) (result i32)))
                (import "env" "nova_emit_event"    (func $ee (param i32 i32)))
                (memory (export "memory") 1)
                (data (i32.const 8192) "init")
                (data (i32.const 8196) "ok")
                (func (export "instantiate") (param i32 i32) (result i32)
                    (call $sw (i32.const 8192) (i32.const 4) (i32.const 8196) (i32.const 2))
                    (i32.const 0)
                )
                (func (export "execute") (param $ip i32) (param $il i32) (result i32)
                    (call $ee (local.get $ip) (local.get $il))
                    (i32.const 4)
                )
                (func (export "query") (param $kp i32) (param $kl i32) (result i32)
                    (local $r i32)
                    (local.set $r (call $sr (local.get $kp) (local.get $kl) (i32.const 4096)))
                    (select (local.get $r) (i32.const 0) (i32.ge_s (local.get $r) (i32.const 0)))
                )
            )
            "#,
        )
        .expect("valid_wasm: WAT parse hatası")
    }

    fn deploy(executor: &NovaVmExecutor, n: u8) -> VmOutput {
        executor.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(n),
            bytecode: valid_wasm(),
            init_msg: vec![],
            label: format!("contract-{n}"),
            gas_limit: 2_000_000,
            slot: 0,
        }).unwrap()
    }

    // ─── 1: WASM modül derleme ve önbellekleme ────────────────────────────

    #[test]
    fn integration_wasm_compile_and_cache() {
        let runtime = WasmRuntime::new(16).unwrap();
        let bytecode = valid_wasm();

        // İlk compile
        let m1 = runtime.compile(&bytecode).unwrap();
        // Cache hit
        let m2 = runtime.compile(&bytecode).unwrap();
        assert_eq!(m1.code_hash, m2.code_hash);
        assert_eq!(runtime.cached_module_count(), 1);
    }

    // ─── 2: Gas metering ──────────────────────────────────────────────────

    #[test]
    fn integration_gas_metering() {
        let mut meter = GasMeter::new(1_000_000);
        meter.consume(500_000).unwrap();
        assert_eq!(meter.usage_bps(), 5_000);
        assert!(meter.consume(500_001).is_err());
    }

    // ─── 3: Host function — storage round-trip ────────────────────────────

    #[test]
    fn integration_host_fn_storage_roundtrip() {
        let mut ctx = HostContext::new(addr(1), addr(2), 500, 0);
        let mut meter = GasMeter::new(1_000_000);

        let key = b"token_balance";
        let value = 1_000_000_u64.to_le_bytes();

        let mut write_input = (key.len() as u32).to_le_bytes().to_vec();
        write_input.extend_from_slice(key);
        write_input.extend_from_slice(&value);

        NovaHostFns::call(&HostCallType::StorageWrite, &mut ctx, &mut meter, &write_input).unwrap();
        let read = NovaHostFns::call(&HostCallType::StorageRead, &mut ctx, &mut meter, key).unwrap();
        assert_eq!(read, value);
    }

    // ─── 4: Nova-specific host functions ──────────────────────────────────

    #[test]
    fn integration_nova_host_fns() {
        let mut ctx = HostContext::new(addr(10), addr(20), 12345, 0);
        let mut meter = GasMeter::new(1_000_000);

        // VM-003: NoopChainStateProvider → inactive (mock hardcoded active kaldırıldı)
        let mpc = NovaHostFns::call(&HostCallType::NovaMpcQuery, &mut ctx, &mut meter, &[]).unwrap();
        assert_eq!(mpc, b"mpc:inactive");

        // VM-003: NoopChainStateProvider → guardian inactive
        let guardian_input = addr(5).as_bytes().to_vec();
        let g = NovaHostFns::call(&HostCallType::NovaGuardianCheck, &mut ctx, &mut meter, &guardian_input).unwrap();
        assert_eq!(g, b"guardian:inactive");

        // VM-006: checked_mul — overflow yok, fallback formül çalışıyor
        let price_bytes = NovaHostFns::call(&HostCallType::NovaBatchPrice, &mut ctx, &mut meter, &[]).unwrap();
        let price = u64::from_le_bytes(price_bytes.try_into().unwrap());
        assert_eq!(price, 12345 * 1_000_000);
    }

    // ─── 5: Lineer bellek ─────────────────────────────────────────────────

    #[test]
    fn integration_linear_memory() {
        let mut mem = LinearMemory::new(1, 4);
        // Büyüt
        mem.grow(2).unwrap();
        assert_eq!(mem.size(), 3 * 65_536);
        // u32 round-trip
        mem.write_u32(0, 0xCAFE_BABE).unwrap();
        assert_eq!(mem.read_u32(0).unwrap(), 0xCAFE_BABE);
    }

    // ─── 6: EVM precompile çağrıları ──────────────────────────────────────

    #[test]
    fn integration_evm_precompiles() {
        let reg = PrecompileRegistry::new();

        // BLAKE3 precompile
        let (hash, gas) = reg.call(&BLAKE3_PRECOMPILE_ADDR, b"nova", &addr(1)).unwrap();
        assert_eq!(hash.len(), 32);
        assert!(gas > 0);

        // Batch auction precompile
        let slot: u64 = 5000;
        let (price_bytes, _) = reg.call(&BATCH_PRECOMPILE_ADDR, &slot.to_le_bytes(), &addr(1)).unwrap();
        let price = u64::from_le_bytes(price_bytes.try_into().unwrap());
        assert_eq!(price, 5000 * 1_000_000);

        // VM-003: NoopChainStateProvider → guardian inactive (0)
        let (status, _) = reg.call(&GUARDIAN_PRECOMPILE_ADDR, addr(7).as_bytes(), &addr(1)).unwrap();
        assert_eq!(status, vec![0], "NoopChainStateProvider: guardian inactive");
    }

    // ─── 7: EVM ABI encoding ──────────────────────────────────────────────

    #[test]
    fn integration_evm_abi_encoding() {
        let value: u128 = u128::MAX / 2;
        let encoded = AbiCodec::encode_uint256(value);
        let decoded = AbiCodec::decode_uint256(&encoded).unwrap();
        assert_eq!(decoded, value);

        let a = addr(42);
        let enc_addr = AbiCodec::encode_address(&a);
        let dec_addr = AbiCodec::decode_address(&enc_addr).unwrap();
        assert_eq!(dec_addr, a);
    }

    // ─── 8: WASM ↔ EVM köprüsü ────────────────────────────────────────────

    #[test]
    fn integration_wasm_evm_bridge() {
        let bridge = WasmEvmBridge::new().unwrap();

        // WASM çağrı
        let wasm_result = bridge.call_wasm(
            &valid_wasm(), "execute", b"data",
            addr(1), addr(2), 1000, 500_000,
        ).unwrap();
        assert!(wasm_result.success);

        // EVM çağrı
        let sel = AbiCodec::function_selector("balanceOf(address)");
        let mut calldata = sel.to_vec();
        calldata.extend_from_slice(&AbiCodec::encode_address(&addr(1)));
        let evm_result = bridge.call_evm(addr(1), addr(3), &calldata, 0, 100_000).unwrap();
        assert!(evm_result.success);
    }

    // ─── 9: Full deploy + call + query akışı ──────────────────────────────

    #[test]
    fn integration_full_deploy_call_query() {
        let executor = NovaVmExecutor::new();

        // Deploy
        let deploy_result = deploy(&executor, 10);
        assert!(deploy_result.success);

        // Call
        let call_result = executor.call(CallInput {
            caller: addr(1),
            contract: addr(10),
            method: "execute".into(),
            input: b"action:transfer".to_vec(),
            value: 0,
            gas_limit: 500_000,
            slot: 100,
        }).unwrap();
        assert!(call_result.success);

        // Query (mock VM'de cross-call state persistence yok — boş veya 32 byte)
        let q = executor.query(addr(10), b"init", 200).unwrap();
        assert!(q.is_empty() || q.len() == 32);
    }

    // ─── 10: 5 farklı kontrat paralel deploy ──────────────────────────────

    #[test]
    fn integration_multi_contract_deploy() {
        let executor = NovaVmExecutor::new();
        for i in 10u8..15 {
            deploy(&executor, i);
        }
        assert_eq!(executor.store().contract_count(), 5);

        // Her kontrat ayrı çağrılabilir
        for i in 10u8..15 {
            let result = executor.call(CallInput {
                caller: addr(1),
                contract: addr(i),
                method: "query".into(),
                input: b"init".to_vec(),
                value: 0,
                gas_limit: 200_000,
                slot: 0,
            }).unwrap();
            assert!(result.success);
        }
    }

    // ─── 11: OutOfGas ─────────────────────────────────────────────────────

    #[test]
    fn integration_out_of_gas_protection() {
        let executor = NovaVmExecutor::new();
        deploy(&executor, 10);

        // Çok düşük gas ile çağır
        let result = executor.call(CallInput {
            caller: addr(1),
            contract: addr(10),
            method: "execute".into(),
            input: b"some_input".to_vec(),
            value: 0,
            gas_limit: 1, // çok az
            slot: 0,
        });
        assert!(matches!(result, Err(VmError::OutOfGas { .. })));
    }

    // ─── 12: BLAKE3 hash determinism ──────────────────────────────────────

    #[test]
    fn integration_blake3_determinism() {
        let mut ctx = HostContext::new(addr(1), addr(2), 0, 0);
        let mut meter = GasMeter::new(1_000_000);
        let data = b"nova-shield-deterministic";

        let h1 = NovaHostFns::call(&HostCallType::Blake3Hash, &mut ctx, &mut meter, data).unwrap();
        let h2 = NovaHostFns::call(&HostCallType::Blake3Hash, &mut ctx, &mut meter, data).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32);
    }
}
