//! VM / WASM sandbox güvenlik testleri — EVM/Cosmos WASM/Wasmtime seviyesinde
//!
//! ## Test Kategorileri
//!
//! 1. **Deploy güvenliği** — Geçersiz bytecode/adres çakışması reddedilmeli
//! 2. **Gas sınırı** — Gaz limiti aşılmamalı
//! 3. **Yokluk koruması** — Deploy edilmemiş kontrata çağrı reddedilmeli
//! 4. **Precompile güvenliği** — BLAKE3/MPC precompile kötü input'u handle eder
//! 5. **Çoklu kontrat** — Birden fazla kontrat bağımsız izole
//! 6. **Determinizm** — Aynı input = aynı gas kullanımı

#[cfg(test)]
mod tests {
    use nova_vm::{NovaVmExecutor, CallInput, DeployInput, VmError};
    use nova_primitives::Address;
    use nova_vm::evm::precompiles::{BLAKE3_PRECOMPILE_ADDR, MPC_PRECOMPILE_ADDR};

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn valid_wasm() -> Vec<u8> {
        // Minimal geçerli WASM: instantiate + execute + query export eden modül
        // wasmtime ile çalışır; nova-vm'in beklediği host fonksiyonlarını import eder
        wat::parse_str(
            r#"
            (module
                (import "env" "nova_storage_write" (func $sw (param i32 i32 i32 i32)))
                (import "env" "nova_storage_read"  (func $sr (param i32 i32 i32) (result i32)))
                (import "env" "nova_emit_event"    (func $ee (param i32 i32)))
                (memory (export "memory") 1)
                (data (i32.const 8192) "key0")
                (data (i32.const 8196) "val0")
                (func (export "instantiate") (param i32 i32) (result i32)
                    (call $sw (i32.const 8192) (i32.const 4)
                               (i32.const 8196) (i32.const 4))
                    (i32.const 0)
                )
                (func (export "execute") (param $ip i32) (param $il i32) (result i32)
                    (call $ee (local.get $ip) (local.get $il))
                    (i32.const 4)
                )
                (func (export "query") (param $kp i32) (param $kl i32) (result i32)
                    (call $sr (local.get $kp) (local.get $kl) (i32.const 4096))
                )
            )
            "#,
        )
        .expect("valid_wasm: WAT parse hatası")
    }

    fn deploy_to(executor: &NovaVmExecutor, addr_n: u8) {
        executor.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(addr_n),
            bytecode: valid_wasm(),
            init_msg: b"init".to_vec(),
            label: format!("contract-{addr_n}"),
            gas_limit: 1_000_000,
            slot: 1,
        }).expect("deploy başarılı olmalı");
    }

    // ── Deploy Güvenliği ──────────────────────────────────────────────────────

    /// PoC: Geçerli WASM kontrat deploy edilebilir
    #[test]
    fn poc_valid_wasm_deploys_successfully() {
        let executor = NovaVmExecutor::new();
        deploy_to(&executor, 10);
        assert_eq!(executor.store().contract_count(), 1);
    }

    /// PoC: Aynı adrese iki kez deploy → hata (adres çakışması)
    #[test]
    fn poc_duplicate_deploy_address_rejected() {
        let executor = NovaVmExecutor::new();
        deploy_to(&executor, 10);
        let result = executor.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(10), // aynı adres
            bytecode: valid_wasm(),
            init_msg: b"init2".to_vec(),
            label: "dup".into(),
            gas_limit: 1_000_000,
            slot: 2,
        });
        assert!(
            matches!(result, Err(VmError::ContractAlreadyExists { .. })),
            "aynı adrese iki deploy reddedilmeli"
        );
    }

    // ── Yokluk Koruması ───────────────────────────────────────────────────────

    /// PoC: Deploy edilmemiş kontrata çağrı → açık hata, panic yok
    #[test]
    fn poc_call_nonexistent_contract_fails_safely() {
        let executor = NovaVmExecutor::new();
        let result = executor.call(CallInput {
            caller: addr(1),
            contract: addr(99), // deploy edilmemiş
            method: "execute".into(),
            input: vec![],
            value: 0,
            gas_limit: 100_000,
            slot: 1,
        });
        assert!(
            matches!(result, Err(VmError::ContractNotFound { .. })),
            "deploy edilmemiş kontrat hata dönmeli"
        );
    }

    // ── Gas Sınırı ────────────────────────────────────────────────────────────

    /// PoC: Geçerli çağrı gas_limit içinde tamamlanır
    #[test]
    fn poc_call_within_gas_limit_succeeds() {
        let executor = NovaVmExecutor::new();
        deploy_to(&executor, 20);

        let result = executor.call(CallInput {
            caller: addr(1),
            contract: addr(20),
            method: "execute".into(),
            input: b"hello".to_vec(),
            value: 0,
            gas_limit: 500_000,
            slot: 2,
        }).unwrap();

        assert!(result.success, "geçerli çağrı başarılı olmalı");
        assert!(result.gas_used <= 500_000, "gas limiti aşılmamalı");
        assert!(result.gas_used > 0, "gas tüketilmeli");
    }

    // ── Çoklu Kontrat İzolasyonu ──────────────────────────────────────────────

    /// PoC: Birden fazla kontrat bağımsız deploy edilebilir
    #[test]
    fn poc_multiple_contracts_isolated() {
        let executor = NovaVmExecutor::new();
        for i in 10u8..15 {
            deploy_to(&executor, i);
        }
        assert_eq!(executor.store().contract_count(), 5);
        assert_eq!(executor.store().code_count(), 5);
    }

    // ── Precompile Güvenliği ──────────────────────────────────────────────────

    /// PoC: BLAKE3 precompile geçerli input → 32 byte hash döner
    #[test]
    fn poc_blake3_precompile_valid_input() {
        let executor = NovaVmExecutor::new();
        let precompile = Address::from_bytes(BLAKE3_PRECOMPILE_ADDR);

        let result = executor.call(CallInput {
            caller: addr(1),
            contract: precompile,
            method: "hash".into(),
            input: b"nova-shield-chain".to_vec(),
            value: 0,
            gas_limit: 10_000,
            slot: 0,
        }).unwrap();

        assert!(result.success, "BLAKE3 precompile başarılı olmalı");
        assert_eq!(result.output.len(), 32, "BLAKE3 çıktısı 32 byte olmalı");
    }

    /// PoC: BLAKE3 precompile boş input → 32 byte hash (boş mesaj hash'i deterministik)
    #[test]
    fn poc_blake3_precompile_empty_input_safe() {
        let executor = NovaVmExecutor::new();
        let precompile = Address::from_bytes(BLAKE3_PRECOMPILE_ADDR);

        let result = executor.call(CallInput {
            caller: addr(1),
            contract: precompile,
            method: "hash".into(),
            input: vec![],
            value: 0,
            gas_limit: 10_000,
            slot: 0,
        }).expect("boş input için precompile panic atmamalı");

        // Boş input için de geçerli hash üretilmeli (panic değil)
        assert!(result.success, "BLAKE3 boş input kabul etmeli");
        assert_eq!(result.output.len(), 32, "boş input için bile 32 byte hash üretilmeli");

        // Boş mesajın hash'i, dolu mesajın hash'inden farklı olmalı
        let result2 = executor.call(CallInput {
            caller: addr(1),
            contract: precompile,
            method: "hash".into(),
            input: b"non-empty".to_vec(),
            value: 0,
            gas_limit: 10_000,
            slot: 0,
        }).unwrap();

        assert_ne!(result.output, result2.output, "boş ve dolu input farklı hash üretmeli");
    }

    /// PoC: BLAKE3 precompile deterministik — aynı input = aynı hash
    #[test]
    fn poc_blake3_precompile_deterministic() {
        let executor = NovaVmExecutor::new();
        let precompile = Address::from_bytes(BLAKE3_PRECOMPILE_ADDR);
        let input = b"determinism-test".to_vec();

        let make_call = || executor.call(CallInput {
            caller: addr(1),
            contract: precompile,
            method: "hash".into(),
            input: input.clone(),
            value: 0,
            gas_limit: 10_000,
            slot: 0,
        });

        let r1 = make_call().unwrap();
        let r2 = make_call().unwrap();
        assert_eq!(r1.output, r2.output, "BLAKE3 deterministik olmalı");
        assert_eq!(r1.gas_used, r2.gas_used, "gas kullanımı deterministik olmalı");
    }

    /// PoC: MPC precompile → VM-003 fix: NoopChainStateProvider varsayılan inactive döndürür
    /// Eski hata: tüm adresler için hardcoded [1] (aktif) — güvenlik açığı.
    /// Düzeltme: ChainStateProvider::is_mpc_account() → NoopChainStateProvider = [0] (inactive).
    #[test]
    fn poc_mpc_precompile_active_check() {
        let executor = NovaVmExecutor::new();
        let precompile = Address::from_bytes(MPC_PRECOMPILE_ADDR);

        let result = executor.call(CallInput {
            caller: addr(1),
            contract: precompile,
            method: "check".into(),
            input: addr(5).as_bytes().to_vec(),
            value: 0,
            gas_limit: 10_000,
            slot: 0,
        }).unwrap();

        assert!(result.success);
        // VM-003: NoopChainStateProvider → inactive (0), artık hardcoded aktif değil
        assert_eq!(result.output, vec![0], "VM-003 fix: NoopChainStateProvider MPC inactive döndürmeli");
    }

    /// PoC: BLAKE3 precompile büyük input → OOM/timeout yok
    #[test]
    fn poc_precompile_large_input_safe() {
        let executor = NovaVmExecutor::new();
        let precompile = Address::from_bytes(BLAKE3_PRECOMPILE_ADDR);
        let large_input = vec![0xFFu8; 100_000]; // 100KB

        let result = executor.call(CallInput {
            caller: addr(1),
            contract: precompile,
            method: "hash".into(),
            input: large_input,
            value: 0,
            gas_limit: 1_000_000,
            slot: 0,
        });
        // Crash yok — hata veya başarı kabul
        match result {
            Ok(out) => assert_eq!(out.output.len(), 32),
            Err(_) => {} // gas limit aşımı da kabul
        }
    }
    // ── Proptest: VM Property Testleri ───────────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: BLAKE3 precompile çıktısı her zaman 32 byte
        #[test]
        fn prop_blake3_precompile_output_always_32_bytes(
            input in proptest::collection::vec(0u8.., 0..256),
        ) {
            let executor = NovaVmExecutor::new();
            let result = executor.call(nova_vm::CallInput {
                caller:    addr(1),
                contract:  Address::from_bytes(BLAKE3_PRECOMPILE_ADDR),
                method:    "hash".to_string(),
                input:     input,
                value:     0,
                gas_limit: 100_000,
                slot:      1,
            });
            if let Ok(out) = result {
                prop_assert_eq!(out.output.len(), 32,
                    "BLAKE3 precompile çıktısı her zaman 32 byte olmalı");
                prop_assert!(out.gas_used > 0, "gas kullanılmalı");
            }
            // Hata da kabul — ama output varsa 32 byte olmalı
        }

        /// Property: Deploy edilen kontratlar çağrılabilir (panic yok)
        #[test]
        fn prop_deploy_then_call_no_panic(contract_id in 10u8..50u8) {
            let executor = NovaVmExecutor::new();
            // Deploy
            let deploy_result = executor.deploy(DeployInput {
                deployer:         addr(1),
                contract_address: addr(contract_id),
                bytecode:         valid_wasm(),
                init_msg:         b"init".to_vec(),
                label:            format!("contract-{contract_id}"),
                gas_limit:        1_000_000,
                slot:             1,
            });
            if deploy_result.is_ok() {
                // Çağrı — hata ya da başarı, panic olmaz
                let call_result = executor.call(nova_vm::CallInput {
                    caller:    addr(1),
                    contract:  addr(contract_id),
                    method:    "execute".to_string(),
                    input:     b"test".to_vec(),
                    value:     0,
                    gas_limit: 100_000,
                    slot:      1,
                });
                // Sadece panic olmaması yeterli
                let _ = call_result;
            }
        }

        /// Property: Var olmayan kontratlara çağrı her zaman hata döner
        #[test]
        fn prop_nonexistent_contract_call_always_fails(contract_id in 200u8..255u8) {
            let executor = NovaVmExecutor::new();
            let result = executor.call(nova_vm::CallInput {
                caller:    addr(1),
                contract:  addr(contract_id),
                method:    "execute".to_string(),
                input:     vec![],
                value:     0,
                gas_limit: 100_000,
                slot:      1,
            });
            prop_assert!(result.is_err(),
                "deploy edilmemiş kontratlara çağrı her zaman hata vermeli");
        }
    }
}
