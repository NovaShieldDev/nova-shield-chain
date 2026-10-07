//! nova-runtime ↔ nova-vm entegrasyon testi
//!
//! ## Test Akışı
//! 1. nova-runtime AccountStateCache + ParallelExecutor oluştur
//! 2. nova-vm NovaVmExecutor ile WASM kontrat deploy et
//! 3. nova-vm kontrat call et
//! 4. nova-runtime Transaction'ları execute_block ile çalıştır
//! 5. State değişimlerini doğrula
//!
//! ## Araştırma Kaynakları
//! - Polkadot (04-POLKADOT-NOVA-ARASTIRMA.md): Wasm executor, CodeExecutor
//! - go-ethereum (06-go-ethereum.md): state db, EVM executor, tx receipt
//! - Avalanchego (GAP-ANALIZ.md): Block executor, state transition

use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
use nova_primitives::{Address, SlotId, crypto::Hash};
use nova_runtime::{
    state::{AccountState, AccountStateCache},
    executor::ParallelExecutor,
    tx::{compute_tx_hash, Transaction, TransactionKind},
};
use nova_vm::{
    api::{CallInput, DeployInput, NovaVmExecutor},
};
use std::sync::Arc;

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn addr(n: u8) -> Address {
    Address::from_bytes([n; 20])
}

fn minimal_nova_wasm() -> Vec<u8> {
    // Nova VM convention: instantiate/execute/query export eder
    // import'lar memory'den önce gelir (WASM spec)
    wat::parse_str(
        r#"
        (module
            (import "env" "nova_storage_write" (func $sw (param i32 i32 i32 i32)))
            (import "env" "nova_storage_read"  (func $sr (param i32 i32 i32) (result i32)))
            (import "env" "nova_emit_event"    (func $ee (param i32 i32)))
            (memory (export "memory") 1)
            (data (i32.const 8192) "key1")
            (data (i32.const 8196) "value1")
            (func (export "instantiate") (param i32 i32) (result i32)
                (call $sw (i32.const 8192) (i32.const 4) (i32.const 8196) (i32.const 6))
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
    .expect("WASM parse hatası")
}

fn make_state_with_accounts() -> Arc<AccountStateCache> {
    let cache = Arc::new(AccountStateCache::new());
    cache.set(addr(1), AccountState::new_eoa(10_000_000_000_000_000_000u128));
    cache.set(addr(2), AccountState::new_eoa(5_000_000_000_000_000_000u128));
    cache
}

fn make_transfer_tx(from: Address, to: Address, amount: u128, nonce: u64) -> Transaction {
    Transaction::mock_transfer(from, to, amount, nonce)
}

/// Seed'den ECDSA imzalı transfer TX'i üret; türetilen sender adresini de döndür.
fn signed_transfer(seed: u8, to: Address, amount: u128, nonce: u64) -> (Transaction, Address) {
    let mut seed_bytes = [0u8; 32];
    seed_bytes[0] = seed;
    let kp = EcdsaKeypair::from_secret_bytes(&seed_bytes).expect("geçerli seed");
    let pk = kp.public_key().expect("pubkey");
    let sender = public_key_to_address(&pk);

    let mut tx = Transaction {
        sender,
        nonce,
        kind: TransactionKind::Transfer { to, amount },
        max_fee_per_gas: 1_000_000_000,
        max_priority_fee_per_gas: 100_000_000,
        gas_limit: 21_000,
        signature: Vec::new(),
        slot: SlotId(0),
    };

    let hash_bytes = compute_tx_hash(&tx);
    let hash = Hash::from_bytes(hash_bytes);
    let sig = kp.sign_hash(&hash).expect("sign");
    tx.signature = sig.as_bytes().to_vec();
    (tx, sender)
}

// ─── Testler ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// nova-vm WASM deploy + call entegrasyonu
    #[test]
    fn vm_deploy_and_call_wasm_contract() {
        let executor = NovaVmExecutor::new();
        let bytecode = minimal_nova_wasm();

        // Deploy
        let deploy_result = executor.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(50),
            bytecode: bytecode.clone(),
            init_msg: vec![],
            label: "test-contract".into(),
            gas_limit: 2_000_000,
            slot: 100,
        }).unwrap();

        assert!(deploy_result.success, "Deploy başarılı olmalı");
        assert!(deploy_result.gas_used > 0, "Gas kullanılmış olmalı");

        // Execute
        let call_result = executor.call(CallInput {
            caller: addr(1),
            contract: addr(50),
            method: "execute".into(),
            input: b"hello_nova".to_vec(),
            value: 0,
            gas_limit: 500_000,
            slot: 101,
        }).unwrap();

        assert!(call_result.success, "Execute başarılı olmalı");
        assert!(call_result.gas_used > 0, "Execute gas kullanmalı");
    }

    /// nova-vm: Paralel deploy + call — 5 kontrat aynı anda
    #[test]
    fn vm_multiple_contracts_independent() {
        let executor = NovaVmExecutor::new();
        let bytecode = minimal_nova_wasm();

        // 5 kontrat deploy
        for i in 60u8..65 {
            let result = executor.deploy(DeployInput {
                deployer: addr(1),
                contract_address: addr(i),
                bytecode: bytecode.clone(),
                init_msg: vec![],
                label: format!("contract-{i}"),
                gas_limit: 2_000_000,
                slot: 200,
            }).unwrap();
            assert!(result.success, "Kontrat {i} deploy başarısız");
        }

        assert_eq!(executor.store().contract_count(), 5);

        // Her kontrat bağımsız çağrılabilir
        for i in 60u8..65 {
            let result = executor.call(CallInput {
                caller: addr(1),
                contract: addr(i),
                method: "execute".into(),
                input: format!("data-{i}").into_bytes(),
                value: 0,
                gas_limit: 300_000,
                slot: 201,
            }).unwrap();
            assert!(result.success, "Kontrat {i} call başarısız");
        }
    }

    /// nova-runtime: ParallelExecutor transfer işlemleri
    #[test]
    fn runtime_parallel_transfer_execution() {
        let state = Arc::new(AccountStateCache::new());
        let executor = ParallelExecutor::new(state.clone(), 1_000_000_000u128);

        let (tx1, s1) = signed_transfer(1, addr(90), 1_000_000_000_000_000_000u128, 0);
        let (tx2, s2) = signed_transfer(2, addr(91), 500_000_000_000_000_000u128, 0);

        state.set(s1, AccountState::new_eoa(10_000_000_000_000_000_000u128));
        state.set(s2, AccountState::new_eoa(5_000_000_000_000_000_000u128));

        let results = executor.execute_block(&[tx1, tx2]);
        assert_eq!(results.len(), 2, "2 TX sonucu olmalı");

        for (i, result) in results.iter().enumerate() {
            assert!(result.success, "TX {} başarılı olmalı: {:?}", i, result.error);
            assert!(result.gas_used > 0, "TX {} gas kullanmalı", i);
        }
    }

    /// nova-runtime: Yetersiz bakiye → başarısız TX
    #[test]
    fn runtime_insufficient_balance_fails() {
        let state = make_state_with_accounts();
        let executor = ParallelExecutor::new(state.clone(), 0u128);

        // Bob'un bakiyesinden fazla transfer
        let tx = make_transfer_tx(
            addr(2),
            addr(1),
            999_999_999_999_999_999_999u128, // bakiyeden çok fazla
            0,
        );

        let results = executor.execute_block(&[tx]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].success, "Yetersiz bakiye TX başarısız olmalı");
    }

    /// nova-runtime + nova-vm entegrasyonu: runtime TX → VM kontrat çağrısı
    #[test]
    fn runtime_and_vm_joint_operation() {
        // Runtime state
        let state = make_state_with_accounts();
        let runtime = ParallelExecutor::new(state.clone(), 1_000_000_000u128);

        // VM executor
        let vm = NovaVmExecutor::new();
        let bytecode = minimal_nova_wasm();

        // Önce kontrat deploy et
        vm.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(70),
            bytecode: bytecode.clone(),
            init_msg: vec![],
            label: "runtime-vm-test".into(),
            gas_limit: 2_000_000,
            slot: 300,
        }).unwrap();

        // Runtime'da imzalı TX çalıştır
        let (runtime_tx, runtime_sender) = signed_transfer(3, addr(80), 100_000_000_000_000_000u128, 0);
        state.set(runtime_sender, AccountState::new_eoa(10_000_000_000_000_000_000u128));

        let runtime_results = runtime.execute_block(&[runtime_tx]);
        assert!(runtime_results[0].success, "Runtime TX başarılı olmalı: {:?}", runtime_results[0].error);

        // VM'de kontrat çağır — ayrı paralel işlem
        let vm_result = vm.call(CallInput {
            caller: addr(1),
            contract: addr(70),
            method: "execute".into(),
            input: b"after_runtime_tx".to_vec(),
            value: 0,
            gas_limit: 300_000,
            slot: 300,
        }).unwrap();
        assert!(vm_result.success, "VM call başarılı olmalı");
    }

    /// nova-vm: Gas tükenmesi koruması
    #[test]
    fn vm_out_of_gas_protection() {
        let vm = NovaVmExecutor::new();
        let bytecode = minimal_nova_wasm();

        vm.deploy(DeployInput {
            deployer: addr(1),
            contract_address: addr(80),
            bytecode: bytecode.clone(),
            init_msg: vec![],
            label: "gas-test".into(),
            gas_limit: 2_000_000,
            slot: 400,
        }).unwrap();

        // Çok düşük gas limit → OutOfGas hatası
        let result = vm.call(CallInput {
            caller: addr(1),
            contract: addr(80),
            method: "execute".into(),
            input: b"data".to_vec(),
            value: 0,
            gas_limit: 1, // 1 fuel
            slot: 401,
        });

        assert!(
            matches!(result, Err(nova_vm::VmError::OutOfGas { .. })),
            "1 fuel ile OutOfGas hatası bekleniyor"
        );
    }
}
