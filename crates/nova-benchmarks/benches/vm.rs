/// VM çalıştırma benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - NovaVmExecutor::deploy()    — WASM kontrat dağıtımı
/// - NovaVmExecutor::call()      — WASM kontrat çağrısı (execute)
/// - NovaVmExecutor::query()     — WASM kontrat sorgusu
/// - EVM deploy + call           — ERC-20 mock kontra
use criterion::{criterion_group, criterion_main, Criterion};
use nova_vm::{CallInput, DeployInput, NovaVmExecutor};
use nova_primitives::{Address};

fn addr(n: u8) -> Address {
    Address::from_bytes([n; 20])
}

/// Nova host fonksiyonlarını içeren minimal WASM modülü.
fn nova_wasm_bytecode() -> Vec<u8> {
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
    .expect("WASM parse hatası")
}

/// ERC-20 mock EVM bytecode (MOCK_ERC20_BYTECODE benzeri)
fn erc20_evm_bytecode() -> Vec<u8> {
    vec![
        // CALLDATALOAD(0) → SHR 224 → 4-byte selector
        0x60, 0x00, 0x35, 0x60, 0xE0, 0x1C,
        // selector dispatch: transfer / balanceOf / approve
        0x80, 0x63, 0xA9, 0x05, 0x9C, 0xBB, 0x14, 0x61, 0x00, 0x2C, 0x57,
        0x80, 0x63, 0x70, 0xA0, 0x82, 0x31, 0x14, 0x61, 0x00, 0x38, 0x57,
        0x80, 0x63, 0x09, 0x5E, 0xA7, 0xB3, 0x14, 0x61, 0x00, 0x2C, 0x57,
        // Unknown selector → REVERT
        0x60, 0x00, 0x60, 0x00, 0xFD,
        // transfer/approve → return 1 (true)
        0x5B, 0x50, 0x60, 0x01, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xF3,
        // balanceOf → return 1e18
        0x5B, 0x50, 0x67, 0x0D, 0xE0, 0xB6, 0xB3, 0xA7, 0x64, 0x00, 0x00,
        0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xF3,
    ]
}

// ─── WASM Deploy ─────────────────────────────────────────────────────────────

fn bench_wasm_deploy(c: &mut Criterion) {
    let bytecode = nova_wasm_bytecode();

    c.bench_function("wasm_deploy", |b| {
        b.iter(|| {
            let executor = NovaVmExecutor::new();
            executor.deploy(DeployInput {
                deployer: addr(1),
                contract_address: addr(10),
                bytecode: bytecode.clone(),
                init_msg: b"init".to_vec(),
                label: "bench-contract".into(),
                gas_limit: 1_000_000,
                slot: 1,
            }).ok()
        })
    });
}

// ─── WASM Call ───────────────────────────────────────────────────────────────

fn bench_wasm_call(c: &mut Criterion) {
    let bytecode = nova_wasm_bytecode();

    // Önceden deploy edilmiş executor
    let executor = NovaVmExecutor::new();
    executor.deploy(DeployInput {
        deployer: addr(1),
        contract_address: addr(20),
        bytecode: bytecode.clone(),
        init_msg: b"init".to_vec(),
        label: "bench".into(),
        gas_limit: 1_000_000,
        slot: 1,
    }).expect("deploy başarısız");

    c.bench_function("wasm_call_execute", |b| {
        b.iter(|| {
            executor.call(CallInput {
                caller: addr(2),
                contract: addr(20),
                method: "execute".into(),
                input: b"hello".to_vec(),
                value: 0,
                gas_limit: 100_000,
                slot: 2,
            }).ok()
        })
    });
}

// ─── WASM Query ──────────────────────────────────────────────────────────────

fn bench_wasm_query(c: &mut Criterion) {
    let bytecode = nova_wasm_bytecode();
    let executor = NovaVmExecutor::new();
    executor.deploy(DeployInput {
        deployer: addr(1),
        contract_address: addr(30),
        bytecode,
        init_msg: b"init".to_vec(),
        label: "bench-q".into(),
        gas_limit: 1_000_000,
        slot: 1,
    }).expect("deploy başarısız");

    c.bench_function("wasm_query", |b| {
        b.iter(|| {
            executor.query(addr(30), b"init", 3).ok()
        })
    });
}

// ─── WASM Deploy + Call roundtrip ────────────────────────────────────────────

fn bench_wasm_deploy_call_roundtrip(c: &mut Criterion) {
    let bytecode = nova_wasm_bytecode();

    c.bench_function("wasm_deploy_call_roundtrip", |b| {
        b.iter(|| {
            let executor = NovaVmExecutor::new();
            executor.deploy(DeployInput {
                deployer: addr(1),
                contract_address: addr(50),
                bytecode: bytecode.clone(),
                init_msg: b"init".to_vec(),
                label: "rt".into(),
                gas_limit: 1_000_000,
                slot: 1,
            }).ok()?;
            executor.call(CallInput {
                caller: addr(2),
                contract: addr(50),
                method: "execute".into(),
                input: b"payload".to_vec(),
                value: 0,
                gas_limit: 100_000,
                slot: 2,
            }).ok()
        })
    });
}

// ─── EVM Deploy ──────────────────────────────────────────────────────────────

fn bench_evm_deploy(c: &mut Criterion) {
    let bytecode = erc20_evm_bytecode();

    c.bench_function("evm_deploy", |b| {
        b.iter(|| {
            let executor = NovaVmExecutor::new();
            executor.deploy(DeployInput {
                deployer: addr(1),
                contract_address: addr(60),
                bytecode: bytecode.clone(),
                init_msg: vec![],
                label: "erc20".into(),
                gas_limit: 1_000_000,
                slot: 1,
            }).ok()
        })
    });
}

// ─── EVM Call (balanceOf) ────────────────────────────────────────────────────

fn bench_evm_call_balance_of(c: &mut Criterion) {
    let bytecode = erc20_evm_bytecode();
    let executor = NovaVmExecutor::new();
    executor.deploy(DeployInput {
        deployer: addr(1),
        contract_address: addr(70),
        bytecode,
        init_msg: vec![],
        label: "erc20".into(),
        gas_limit: 1_000_000,
        slot: 1,
    }).expect("EVM deploy başarısız");

    // balanceOf(address) selector = 0x70a08231
    let call_data = {
        let mut d = vec![0x70, 0xA0, 0x82, 0x31];
        d.extend_from_slice(&[0u8; 32]); // address param
        d
    };

    c.bench_function("evm_call_balanceof", |b| {
        b.iter(|| {
            executor.call(CallInput {
                caller: addr(2),
                contract: addr(70),
                method: "call".into(),
                input: call_data.clone(),
                value: 0,
                gas_limit: 100_000,
                slot: 2,
            }).ok()
        })
    });
}

criterion_group!(
    benches,
    bench_wasm_deploy,
    bench_wasm_call,
    bench_wasm_query,
    bench_wasm_deploy_call_roundtrip,
    bench_evm_deploy,
    bench_evm_call_balance_of,
);
criterion_main!(benches);
