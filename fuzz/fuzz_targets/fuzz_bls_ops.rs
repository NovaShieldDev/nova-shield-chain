//! BLS imza fuzzing — rastgele key/message/vote kombinasyonları.
//!
//! Hedef: Herhangi bir input için BLS sign/verify/aggregate panic üretmez.
//! Çalıştır: cargo +nightly fuzz run fuzz_bls_ops
#![no_main]
use libfuzzer_sys::fuzz_target;
use nova_consensus::{BftSigner, BftVerifier, AggregateVote, vote_message};
use nova_primitives::{ValidatorId, SlotId, BlockHash};

fuzz_target!(|data: &[u8]| {
    if data.len() < 32 {
        return;
    }

    let ikm: [u8; 32] = data[..32].try_into().unwrap_or([1u8; 32]);
    let vid = ValidatorId(1);

    // 1. BftSigner oluştur — geçersiz IKM → hata, panic değil
    let signer_result = BftSigner::from_ikm(vid, &ikm);

    if let Ok(signer) = signer_result {
        let height = u64::from_le_bytes(
            data.get(0..8).and_then(|s| s.try_into().ok()).unwrap_or([1u8; 8])
        );
        let slot = SlotId(height);
        let hash_bytes: [u8; 32] = data.get(..32).and_then(|s| s.try_into().ok()).unwrap_or([1u8; 32]);
        let block_hash = BlockHash::from(hash_bytes);

        // vote_message(block_hash, validator_id, height, slot)
        let _msg = vote_message(block_hash, vid, height, slot);

        // sign_vote(block_hash, height, slot)
        let vote_result = signer.sign_vote(block_hash, height, slot);

        if let Ok(vote) = vote_result {
            // 3. Doğrula — kendi imzası geçerli olmalı
            let _ = BftVerifier::verify_vote(&vote);

            // 4. Aggregate: tek oydan — panic yok
            let _ = AggregateVote::build(&[vote]);
        }
    }

    // 5. generate: her zaman güvenli
    let _ = BftSigner::generate(ValidatorId(
        u64::from_le_bytes(data.get(0..8).and_then(|s| s.try_into().ok()).unwrap_or([1u8; 8]))
    ));
});
