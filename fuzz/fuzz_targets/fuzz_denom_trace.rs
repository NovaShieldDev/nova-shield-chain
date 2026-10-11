//! IBC DenomTrace fuzzing — rastgele path/denom kombinasyonları.
//!
//! Hedef: Herhangi bir string input için denom işlemleri panic üretmez.
//! Çalıştır: cargo +nightly fuzz run fuzz_denom_trace
#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_network::{DenomTrace, FungibleTokenPacket};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // UTF-8 string olmayabilir — güvenli dönüşüm
    let s = String::from_utf8_lossy(data).to_string();

    // Path ve base_denom olarak böl
    let mid = s.len() / 2;
    let path = &s[..mid];
    let base_denom = &s[mid..];

    // DenomTrace oluştur — panic yok
    let trace = DenomTrace::new(path, base_denom);

    // is_native: her zaman güvenli
    let _native = trace.is_native();

    // ibc_denom: hash hesapla — her zaman güvenli
    let denom = trace.ibc_denom();
    assert!(denom.starts_with("ibc/"), "ibc_denom her zaman ibc/ ile başlamalı");

    // Prefix ekle/kaldır — panic yok
    let with_prefix = trace.add_prefix("transfer", "channel-0");
    assert!(!with_prefix.is_native(), "prefix eklenince native değil");

    let removed = with_prefix.remove_prefix("transfer", "channel-0");
    // Başarılı veya başarısız — panic yok
    let _ = removed;

    // Deterministik: aynı input → aynı hash
    let trace2 = DenomTrace::new(path, base_denom);
    assert_eq!(trace.ibc_denom(), trace2.ibc_denom(), "deterministik olmalı");

    // FungibleTokenPacket fuzzing
    if data.len() >= 4 {
        let amount = u64::from_le_bytes(
            data.get(..8).and_then(|s| s.try_into().ok()).unwrap_or([0u8; 8])
        ) as u128;

        let packet = FungibleTokenPacket::new(
            &denom,
            amount,
            &format!("sender_{}", data[0]),
            &format!("receiver_{}", data[0]),
        );

        // Commitment hash: deterministik, panic yok
        let h1 = packet.commitment_hash();
        let h2 = packet.commitment_hash();
        assert_eq!(h1, h2, "commitment hash deterministik");
    }
});
