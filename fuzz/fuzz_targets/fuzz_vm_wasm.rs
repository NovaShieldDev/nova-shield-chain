//! WASM bytecode fuzz testi — rastgele/kötü bytecode için VM paniğe yol açmaz.
//!
//! Hedef: Herhangi bir bytecode için deploy/call güvenli hata döner, panic yok.
//! Çalıştırma: `cargo fuzz run fuzz_vm_wasm`
#![no_main]

use libfuzzer_sys::fuzz_target;
use nova_vm::{CallInput, DeployInput, NovaVmExecutor};
use nova_primitives::Address;

fn addr(n: u8) -> Address {
    Address::from_bytes([n; 20])
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let executor = NovaVmExecutor::new();

    // Strateji 1: Rastgele byte'ları doğrudan bytecode olarak dağıt
    let result = executor.deploy(DeployInput {
        deployer: addr(1),
        contract_address: addr(0xF0),
        bytecode: data.to_vec(),
        init_msg: data.get(..32.min(data.len())).unwrap_or(&[]).to_vec(),
        label: "fuzz-wasm".into(),
        gas_limit: 10_000,
        slot: 1,
    });

    // Deploy başarılıysa call da dene
    if result.is_ok() {
        let call_input = data.get(..32.min(data.len())).unwrap_or(&[]).to_vec();
        let _ = executor.call(CallInput {
            caller: addr(2),
            contract: addr(0xF0),
            method: "execute".into(),
            input: call_input,
            value: 0,
            gas_limit: 5_000,
            slot: 2,
        });
    }

    // Strateji 2: Geçerli WASM magic + rastgele body
    if data.len() >= 4 {
        let mut wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic
        wasm.extend_from_slice(data);
        let _ = executor.deploy(DeployInput {
            deployer: addr(3),
            contract_address: addr(0xF1),
            bytecode: wasm,
            init_msg: vec![],
            label: "fuzz-wasm-magic".into(),
            gas_limit: 10_000,
            slot: 3,
        });
    }
});
