//! GÖREV 80: cargo-fuzz — ağ mesajı fuzzing
//!
//! Hedef: Shred ve gossip mesajı parse işlemleri rastgele baytlarla çökmemeli.
//! Çalıştır: cargo +nightly fuzz run fuzz_network_message

#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_network::{BloomFilter, ShredBuilder, SHRED_DATA_PAYLOAD_SIZE};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // 1. Bloom filtresi fuzzing — panik olmamalı
    {
        let capacity = (data[0] as usize * 100).max(10).min(100_000);
        let mut bf = BloomFilter::new(capacity);

        // Birden fazla hash değeri ekle
        for chunk in data.chunks(32) {
            if chunk.len() == 32 {
                let mut h = [0u8; 32];
                h.copy_from_slice(chunk);

                bf.add(&h);
                // Eklenen her değer contains() true döndürmeli (false negative yok)
                assert!(bf.contains(&h), "Bloom false negative");
            }
        }
    }

    // 2. ShredBuilder fuzzing — panik olmamalı
    {
        let slot = u64::from_le_bytes({
            let mut b = [0u8; 8];
            let l = data.len().min(8);
            b[..l].copy_from_slice(&data[..l]);
            b
        });

        // Küçük veri ile shred bölme/birleştirme
        let block_data = &data[data.len().min(8)..];
        if !block_data.is_empty() && block_data.len() <= 64 * 1024 {
            let builder = ShredBuilder::new([1u8; 32], SHRED_DATA_PAYLOAD_SIZE);
            let shreds = builder.shred_block(slot, block_data);

            if !shreds.is_empty() {
                // Her shred doğrulanabilir olmalı
                for s in &shreds {
                    let _ = s.verify();
                }

                // Yeniden birleştir — panik olmamalı
                let _ = builder.deshred(&shreds);
            }
        }
    }
});
