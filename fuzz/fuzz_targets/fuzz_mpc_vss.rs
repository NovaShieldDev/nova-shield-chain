//! MPC / Shamir VSS fuzzing — rastgele share/commitment kombinasyonları.
//!
//! Hedef: Herhangi bir input için VSS generate/verify/reconstruct panic üretmez.
//! Çalıştır: cargo +nightly fuzz run fuzz_mpc_vss
#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_mpc::{
    vss::{feldman_vss_generate, feldman_vss_verify, shamir_reconstruct},
    types::PartyIndex,
};

fuzz_target!(|data: &[u8]| {
    if data.len() < 34 {
        return;
    }

    let threshold = (data[0] % 10) + 1; // 1..=10
    let party_count = threshold as u32 + (data[1] as u32 % 5); // threshold..=threshold+4
    let secret: [u8; 32] = data[2..34].try_into().unwrap_or([0u8; 32]);
    let dealer = PartyIndex((data[0] as u32 % party_count) + 1);

    // VSS generate: her zaman güvenli sonuçlanmalı
    let result = feldman_vss_generate(threshold, party_count, Some(secret), dealer);

    if let Ok(vss) = result {
        // Geçerli share'leri doğrula
        for share in &vss.shares {
            let _ = feldman_vss_verify(&share.value, share.party, &vss.commitment);
        }

        // Rastgele byte'ları share olarak doğrula — panic yok
        if data.len() >= 66 {
            let fake_share: [u8; 32] = data[34..66].try_into().unwrap_or([0xFFu8; 32]);
            let _ = feldman_vss_verify(&fake_share, PartyIndex(1), &vss.commitment);
        }

        // t share ile reconstruct
        if vss.shares.len() >= threshold as usize {
            let subset: Vec<(PartyIndex, [u8; 32])> = vss.shares[..threshold as usize]
                .iter().map(|s| (s.party, s.value)).collect();
            let _ = shamir_reconstruct(&subset);
        }

        // t-1 share ile reconstruct — hata veya yanlış değer, panic yok
        if threshold > 1 && vss.shares.len() >= (threshold - 1) as usize {
            let subset: Vec<(PartyIndex, [u8; 32])> = vss.shares[..(threshold - 1) as usize]
                .iter().map(|s| (s.party, s.value)).collect();
            let _ = shamir_reconstruct(&subset);
        }
    }

    // Sınır: threshold=0 → hata döner, panic yok
    let _ = feldman_vss_generate(0, 3, Some([0u8; 32]), PartyIndex(1));

    // Sınır: threshold > n → hata döner, panic yok
    let _ = feldman_vss_generate(10, 3, Some([0u8; 32]), PartyIndex(1));
});
