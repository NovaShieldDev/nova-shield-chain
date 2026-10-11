//! SpendCircuit kısıt doğrulayıcısına kötü/rastgele witness fuzz testi.
//!
//! Hedef: Herhangi bir witness için prove()/check_constraints() paniğe yol açmaz.
//! Çalıştırma: `cargo fuzz run fuzz_spend_circuit`
#![no_main]

use libfuzzer_sys::fuzz_target;
use nova_zk::{
    Note, NSC_ASSET_ID, NullifierKey,
    circuits::spend::SpendWitness,
    utxo::AssetId,
};
use nova_primitives::SlotId;

fuzz_target!(|data: &[u8]| {
    if data.len() < 128 {
        return;
    }

    // Data layout:
    //   [0..16]  = value (u128 LE)
    //   [16..48] = randomness
    //   [48..80] = transmission_key
    //   [80..112]= nullifier_key
    //   [112..144]=merkle sibling (optional)

    let value = u128::from_le_bytes(data[0..16].try_into().unwrap_or([0u8; 16]));
    let randomness: [u8; 32] = data[16..48].try_into().unwrap_or([0u8; 32]);
    let transmission_key: [u8; 32] = data[48..80].try_into().unwrap_or([1u8; 32]);
    let nullifier_key_bytes: [u8; 32] = data[80..112].try_into().unwrap_or([2u8; 32]);

    // Transmission key sıfır değilse geçerli note
    let asset = NSC_ASSET_ID;
    let note = Note::new(value, asset, randomness, transmission_key);
    let nk = NullifierKey(nullifier_key_bytes);

    let commitment = note.commit();

    // Merkle yolu — rastgele veya boş
    let (merkle_path, merkle_sides, anchor) = if data.len() >= 144 {
        let sibling: [u8; 32] = data[112..144].try_into().unwrap_or([0xAAu8; 32]);
        let is_right = data.get(144).map(|&b| b & 1 == 0).unwrap_or(true);
        let anchor = {
            let mut h = blake3::Hasher::new();
            if is_right {
                h.update(commitment.as_bytes());
                h.update(&sibling);
            } else {
                h.update(&sibling);
                h.update(commitment.as_bytes());
            }
            *h.finalize().as_bytes()
        };
        (vec![sibling], vec![is_right], anchor)
    } else {
        (vec![*commitment.as_bytes()], vec![false], *commitment.as_bytes())
    };

    let randomizer: [u8; 32] = data.get(145..177).and_then(|s| s.try_into().ok()).unwrap_or([0x11u8; 32]);
    let rcv: [u8; 32] = data.get(177..209).and_then(|s| s.try_into().ok()).unwrap_or([0x22u8; 32]);

    let witness = SpendWitness::new(
        note,
        nk,
        merkle_path,
        merkle_sides,
        randomizer,
        rcv,
    );

    // check_constraints: anchor eşleşmeyebilir → hata döner ama paniğe yol açmamalı
    let _ = witness.check_constraints(anchor, SlotId(1));

    // prove: herhangi bir girdi için paniğe yol açmamalı
    let _ = witness.prove(anchor, SlotId(1));
});
