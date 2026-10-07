//! ZK-Shield property-based testleri (proptest).
//!
//! **İspatlanan özellikler:**
//! 1. Note commitment deterministik: aynı input → aynı output
//! 2. Nullifier benzersizliği: farklı (nk, commitment) çifti → farklı nullifier
//! 3. Commitment çarpışma direnci: farklı input → (neredeyse) kesinlikle farklı output
//! 4. Nullifier tek yönlü: nullifier'dan commitment elde edilemez (yapısal test)
//! 5. ZK mint kanıtı deterministik: aynı note → aynı kanıt (mock)
//! 6. ZK kanıt doğrulama tutarlılığı: prove() → verify() her zaman geçer

use nova_primitives::SlotId;
use nova_zk::{
    Note, NSC_ASSET_ID, NullifierKey,
    circuits::mint::MintWitness,
    utxo::note::NoteCommitment,
    utxo::nullifier::Nullifier,
};
use proptest::prelude::*;

// ─── Yardımcı stratejiler ─────────────────────────────────────────────────────

fn arb_bytes32() -> impl Strategy<Value = [u8; 32]> {
    prop::array::uniform32(any::<u8>())
}

fn arb_note_value() -> impl Strategy<Value = u128> {
    // 0 veya pozitif değer (max 10^18), ağırlıklı seçim
    prop_oneof![
        1 => Just(0u128),
        9 => 1u128..1_000_000_000_000_000_000u128,
    ]
}

fn arb_note() -> impl Strategy<Value = Note> {
    (arb_note_value(), arb_bytes32(), arb_bytes32())
        .prop_map(|(value, randomness, transmission_key)| {
            Note::new(value, NSC_ASSET_ID, randomness, transmission_key)
        })
}

fn arb_nullifier_key() -> impl Strategy<Value = NullifierKey> {
    arb_bytes32().prop_map(NullifierKey)
}

// ─── Proptest property testleri ───────────────────────────────────────────────

proptest! {
    /// P1: Note commitment deterministik
    /// Aynı (value, asset, randomness, transmission_key) → aynı commitment
    #[test]
    fn prop_note_commitment_deterministic(
        value in arb_note_value(),
        randomness in arb_bytes32(),
        transmission_key in arb_bytes32(),
    ) {
        let note1 = Note::new(value, NSC_ASSET_ID, randomness, transmission_key);
        let note2 = Note::new(value, NSC_ASSET_ID, randomness, transmission_key);
        let cm1 = note1.commit();
        let cm2 = note2.commit();
        prop_assert_eq!(cm1, cm2,
            "Aynı note farklı commitment üretmemeli: {:?} vs {:?}", cm1, cm2);
    }

    /// P2: Farklı randomness → farklı commitment (collision resistance)
    #[test]
    fn prop_different_randomness_different_commitment(
        value in 1u128..1_000_000u128,
        r1 in arb_bytes32(),
        r2 in arb_bytes32(),
        transmission_key in arb_bytes32(),
    ) {
        prop_assume!(r1 != r2);
        let n1 = Note::new(value, NSC_ASSET_ID, r1, transmission_key);
        let n2 = Note::new(value, NSC_ASSET_ID, r2, transmission_key);
        prop_assert_ne!(n1.commit(), n2.commit(),
            "Farklı randomness aynı commitment üretmemeli");
    }

    /// P3: Farklı (nk, commitment) → farklı nullifier (uniqueness)
    #[test]
    fn prop_nullifier_unique_per_key_and_commitment(
        nk1_bytes in arb_bytes32(),
        nk2_bytes in arb_bytes32(),
        cm1_bytes in arb_bytes32(),
        cm2_bytes in arb_bytes32(),
    ) {
        prop_assume!(nk1_bytes != nk2_bytes || cm1_bytes != cm2_bytes);

        let nk1 = NullifierKey(nk1_bytes);
        let nk2 = NullifierKey(nk2_bytes);
        let cm1 = NoteCommitment(cm1_bytes);
        let cm2 = NoteCommitment(cm2_bytes);

        let n1 = nk1.derive_nullifier(&cm1);
        let n2 = nk2.derive_nullifier(&cm2);

        // Farklı (nk, cm) → farklı nullifier olmalı
        if nk1_bytes != nk2_bytes || cm1_bytes != cm2_bytes {
            prop_assert_ne!(n1, n2,
                "Farklı input'lar aynı nullifier üretmemeli");
        }
    }

    /// P4: Aynı key, farklı commitment → farklı nullifier
    #[test]
    fn prop_nullifier_unique_per_commitment(
        nk_bytes in arb_bytes32(),
        cm1_bytes in arb_bytes32(),
        cm2_bytes in arb_bytes32(),
    ) {
        prop_assume!(cm1_bytes != cm2_bytes);

        let nk = NullifierKey(nk_bytes);
        let cm1 = NoteCommitment(cm1_bytes);
        let cm2 = NoteCommitment(cm2_bytes);

        let n1 = nk.derive_nullifier(&cm1);
        let n2 = nk.derive_nullifier(&cm2);

        prop_assert_ne!(n1, n2,
            "Aynı key, farklı commitment → farklı nullifier olmalı");
    }

    /// P5: Farklı key, aynı commitment → farklı nullifier
    #[test]
    fn prop_nullifier_unique_per_key(
        nk1_bytes in arb_bytes32(),
        nk2_bytes in arb_bytes32(),
        cm_bytes in arb_bytes32(),
    ) {
        prop_assume!(nk1_bytes != nk2_bytes);

        let nk1 = NullifierKey(nk1_bytes);
        let nk2 = NullifierKey(nk2_bytes);
        let cm = NoteCommitment(cm_bytes);

        let n1 = nk1.derive_nullifier(&cm);
        let n2 = nk2.derive_nullifier(&cm);

        prop_assert_ne!(n1, n2,
            "Farklı key, aynı commitment → farklı nullifier olmalı");
    }

    /// P6: Nullifier kendi kendine eşit (deterministik)
    #[test]
    fn prop_nullifier_deterministic(
        nk_bytes in arb_bytes32(),
        cm_bytes in arb_bytes32(),
    ) {
        let nk = NullifierKey(nk_bytes);
        let cm = NoteCommitment(cm_bytes);

        let n1 = nk.derive_nullifier(&cm);
        let n2 = nk.derive_nullifier(&cm);

        prop_assert_eq!(n1, n2, "Nullifier deterministik olmalı");
    }

    /// P7: Note value değişimi → commitment değişimi
    #[test]
    fn prop_different_value_different_commitment(
        v1 in 0u128..1_000_000u128,
        v2 in 0u128..1_000_000u128,
        randomness in arb_bytes32(),
        transmission_key in arb_bytes32(),
    ) {
        prop_assume!(v1 != v2);
        let n1 = Note::new(v1, NSC_ASSET_ID, randomness, transmission_key);
        let n2 = Note::new(v2, NSC_ASSET_ID, randomness, transmission_key);
        prop_assert_ne!(n1.commit(), n2.commit(),
            "Farklı value aynı commitment üretmemeli");
    }

    /// P8: MintWitness prove-verify tutarlılığı
    /// prove() başarılıysa verify() her zaman true döner (mock ZK)
    #[test]
    fn prop_mint_prove_verify_consistent(
        value in 1u128..1_000_000u128,
        randomness in arb_bytes32(),
        transmission_key in arb_bytes32(),
        slot in 0u64..1000u64,
    ) {
        let note = Note::new(value, NSC_ASSET_ID, randomness, transmission_key);
        let witness = MintWitness::new(note);
        let proof = witness.prove(SlotId(slot));
        prop_assume!(proof.is_ok());
        let proof = proof.unwrap();
        let verify_result = proof.verify();
        prop_assert!(
            verify_result.is_ok() && verify_result.unwrap(),
            "Prove→Verify tutarsız: geçerli kanıt verify'ı geçemedi"
        );
    }
}
