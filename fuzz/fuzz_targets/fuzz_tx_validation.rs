//! Transaction doğrulama fuzz testi — kötü/rastgele TX alanları.
//!
//! Hedef: Herhangi bir TX girdi kombinasyonunda validate panic yok.
//! Çalıştırma: `cargo fuzz run fuzz_tx_validation`
#![no_main]

use libfuzzer_sys::fuzz_target;
use nova_mempool::{
    validate_encrypted_tx, validate_decrypted_tx_format,
    ValidationConfig,
    types::{EncryptedTx, RawTransaction, TxHash, TxType},
};
use nova_primitives::{Address, Balance, Nonce, SlotId};

fn addr(data: &[u8]) -> Address {
    let mut buf = [0u8; 20];
    let len = data.len().min(20);
    buf[..len].copy_from_slice(&data[..len]);
    Address::from_bytes(buf)
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 64 {
        return;
    }

    let config = ValidationConfig::default();

    // ─── EncryptedTx fuzz ────────────────────────────────────────────────────

    let commitment_bytes: [u8; 32] = data[..32].try_into().unwrap_or([0u8; 32]);
    let ephemeral_pubkey: [u8; 32] = data[32..64].try_into().unwrap_or([1u8; 32]);
    let aes_nonce: [u8; 12] = data.get(64..76).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 12]);

    let ciphertext_len = data.get(76).map(|&b| (b as usize % 512) + 1).unwrap_or(32);
    let aes_ciphertext = data
        .get(77..)
        .map(|s| s[..s.len().min(ciphertext_len)].to_vec())
        .unwrap_or_else(|| vec![0u8; ciphertext_len]);

    let target_slot_raw = u64::from_le_bytes(
        data.get(77..85).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 8])
    );

    let sig_len = data.get(85).map(|&b| b as usize % 128).unwrap_or(64);
    let signature = data
        .get(86..)
        .map(|s| s[..s.len().min(sig_len)].to_vec())
        .unwrap_or_else(|| vec![0u8; sig_len]);

    let enc_tx = EncryptedTx {
        commitment: TxHash::from_bytes(commitment_bytes),
        ephemeral_pubkey,
        aes_ciphertext,
        aes_nonce,
        target_slot: SlotId(target_slot_raw),
        sender: addr(&data[32..]),
        signature,
        plaintext_size_hint: data.get(86).map(|&b| b as u32 * 4).unwrap_or(0),
    };

    let current_slot = SlotId(
        u64::from_le_bytes(
            data.get(87..95).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 8])
        )
    );

    // Herhangi bir sonuç OK — sadece panic olmamalı
    let _ = validate_encrypted_tx(&enc_tx, &config, current_slot);

    // ─── RawTransaction fuzz ─────────────────────────────────────────────────

    if data.len() >= 128 {
        let value_bytes: [u8; 16] = data[96..112].try_into().unwrap_or([0u8; 16]);
        let value = u128::from_le_bytes(value_bytes);
        let nonce = u64::from_le_bytes(data[112..120].try_into().unwrap_or([0u8; 8]));
        let gas = u64::from_le_bytes(data[120..128].try_into().unwrap_or([0u8; 8]));
        let chain_id = u64::from_le_bytes(data.get(128..136).and_then(|s| s.try_into().ok()).unwrap_or([1u8, 0, 0, 0, 0, 0, 0, 0]));

        let raw_tx = RawTransaction {
            from: addr(data),
            to: if data.get(0).map(|&b| b & 1 == 0).unwrap_or(true) {
                Some(addr(&data[10..]))
            } else {
                None
            },
            value: Balance(value),
            nonce: Nonce(nonce),
            tx_type: TxType::Transfer,
            gas_limit: gas,
            max_fee_per_gas: Balance(u128::from_le_bytes(
                data.get(136..152).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 16])
            )),
            max_priority_fee_per_gas: Balance(1_000_000),
            data: data.get(152..).unwrap_or(&[]).to_vec(),
            chain_id,
        };

        // validate_decrypted_tx_format(tx, chain_id): herhangi bir TX için paniğe yol açmamalı
        let _ = validate_decrypted_tx_format(&raw_tx, chain_id);
    }
});
