//! MiMC hash fonksiyonuna kötü/rastgele girdi fuzz testi.
//!
//! Hedef: Herhangi bir girdi için MiMC paniklemez veya tanımsız davranış göstermez.
//! Çalıştırma: `cargo fuzz run fuzz_mimc`
#![no_main]

use libfuzzer_sys::fuzz_target;
use nova_zk::circuits::mimc::{mimc_hash, mimc_sponge, mimc_merkle_hash, mimc_nullifier};
use ark_bls12_381::Fr;
use ark_ff::PrimeField;

fuzz_target!(|data: &[u8]| {
    if data.len() < 32 {
        return;
    }

    // Byte'lardan Fr skaleri oluştur (mod_order ile normalize)
    let a = Fr::from_le_bytes_mod_order(&data[..32]);

    let b_bytes = if data.len() >= 64 { &data[32..64] } else { &data[..32] };
    let b = Fr::from_le_bytes_mod_order(b_bytes);

    // mimc_hash: paniklememeli, NaN/Inf döndürmemeli
    let _ = mimc_hash(a, b);

    // mimc_sponge: farklı boyutlarda girdi
    if data.len() >= 96 {
        let inputs: Vec<Fr> = data
            .chunks(32)
            .map(|chunk| {
                let mut buf = [0u8; 32];
                let len = chunk.len().min(32);
                buf[..len].copy_from_slice(&chunk[..len]);
                Fr::from_le_bytes_mod_order(&buf)
            })
            .collect();
        let _ = mimc_sponge(&inputs);
    }

    // mimc_merkle_hash: rastgele iki düğüm
    if data.len() >= 64 {
        let left  = <[u8; 32]>::try_from(&data[..32]).unwrap_or([0u8; 32]);
        let right = <[u8; 32]>::try_from(&data[32..64]).unwrap_or([0u8; 32]);
        let _ = mimc_merkle_hash(&left, &right);
    }

    // mimc_nullifier: nk + cm kombinasyonu
    if data.len() >= 64 {
        let nk = <[u8; 32]>::try_from(&data[..32]).unwrap_or([0u8; 32]);
        let cm = <[u8; 32]>::try_from(&data[32..64]).unwrap_or([0u8; 32]);
        let _ = mimc_nullifier(&nk, &cm);
    }
});
