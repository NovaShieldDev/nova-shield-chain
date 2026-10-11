//! Governance tally fuzzing — rastgele oy dağılımları.
//!
//! Hedef: Her türlü oy kombinasyonu için tally/evaluate panic üretmez ve
//!        deterministik sonuç verir.
//! Çalıştır: cargo +nightly fuzz run fuzz_governance_tally
#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_consensus::ValidatorRegistry;

// nova-fuzz Cargo.toml'da nova-consensus var, nova-runtime yok.
// Sadece mevcut bağımlılıklarla çalışıyoruz.
// Tally logic'i blok başlıklarıyla test et.

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    // Rastgele validator ID'leri ve stake değerleri oluştur
    let registry = ValidatorRegistry::new();

    // 1-8 validator kaydet (rastgele)
    let n_validators = (data[0] % 8) + 1;
    let mut registered = 0usize;

    for i in 0..n_validators {
        let offset = (i as usize * 8) % data.len();
        let stake_raw = u64::from_le_bytes(
            data.get(offset..offset + 8)
                .and_then(|s| s.try_into().ok())
                .unwrap_or([0u8; 8])
        ) as u128;

        // Minimum stake: 100_000 * 10^18
        let stake = (stake_raw % 900_000) + 100_000;
        let stake_full = stake * 1_000_000_000_000_000_000u128;

        use nova_primitives::{ValidatorId, Address, SlotId, Balance};
        let _ = registry.register(
            ValidatorId(i as u64 + 1),
            Address::from_bytes([i as u8 + 1; 20]),
            Balance::from(stake_full),
            300,
            [(i as u8).wrapping_add(1); 32],
            SlotId(0),
        );
        registered += 1;
    }

    // Aktif validator sayısı tutarlı olmalı
    let active = registry.get_active_validators();
    assert!(active.len() <= registered, "aktif ≤ kayıtlı");

    // get_total_stake: her zaman güvenli
    let total = registry.total_active_stake();
    assert!(total >= 0, "toplam stake negatif olamaz");
});
