//! GÖREV 80: cargo-fuzz — konsensüs mesajı fuzzing
//!
//! Hedef: BlockHeader JSON deserializasyonu rastgele baytlarla çökmemeli.
//! Çalıştır: cargo +nightly fuzz run fuzz_block_header

#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_consensus::BlockHeader;

fuzz_target!(|data: &[u8]| {
    // 1. JSON deserializasyonu — panik üretmemeli
    if let Ok(s) = std::str::from_utf8(data) {
        let _: Result<BlockHeader, _> = serde_json::from_str(s);
    }

    // 2. Yapısal fuzzing: veriden BlockHeader alanları oluşturmaya çalış
    if data.len() >= 32 {
        use nova_primitives::{BlockHash, EpochId, SlotId, ValidatorId};
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&data[..32]);

        let header = BlockHeader {
            height: data.get(32).copied().unwrap_or(0) as u64,
            slot: SlotId(data.get(33).copied().unwrap_or(0) as u64),
            epoch: EpochId(data.get(34).copied().unwrap_or(0) as u64),
            parent_hash: BlockHash::from_bytes(hash_bytes),
            state_root: {
                let mut sr = [0u8; 32];
                if data.len() >= 64 {
                    sr.copy_from_slice(&data[32..64]);
                }
                sr
            },
            proposer: ValidatorId(data.get(64).copied().unwrap_or(1) as u64),
            timestamp: data.get(65).copied().unwrap_or(0) as u64,
            tx_count: 0,
        };

        // hash tutarlılığı — her zaman deterministik olmalı
        let h1 = header.compute_hash();
        let h2 = header.compute_hash();
        assert_eq!(h1, h2, "hash deterministik değil");

        // is_genesis kontrol
        let _ = header.is_genesis();

        // JSON round-trip — panik üretmemeli
        if let Ok(json) = serde_json::to_string(&header) {
            let _: Result<BlockHeader, _> = serde_json::from_str(&json);
        }
    }
});
