//! ZK devre güvenlik testleri — Zcash/Aztec/Tornado seviyesinde
//!
//! ## Test Kategorileri
//!
//! 1. **Mint proof soundness** — Geçerli note → kanıt doğrulanmalı
//! 2. **Transfer value imbalance** — Input < output → reddedilmeli
//! 3. **Burn proof** — Geçerli burn kanıtı doğrulanmalı
//! 4. **Nullifier uniqueness** — Aynı nota iki kez harcanamaz
//! 5. **Double-spend koruması** — NullifierSet atomik insert
//! 6. **Note commitment deterministik** — Aynı note = aynı commitment
//! 7. **Nullifier türetme deterministik** — Aynı nk + commitment = aynı nullifier
//! 8. **Farklı randomness → farklı commitment** — Collision resistance

#[cfg(test)]
mod tests {
    use nova_zk::{
        MintWitness, BurnWitness, TransferWitness,
        Note, NullifierKey, NullifierSet, Nullifier,
        NSC_ASSET_ID, AssetId, ZkError,
    };
    use nova_primitives::SlotId;

    fn make_note(value: u128, randomness: [u8; 32], tk: [u8; 32]) -> Note {
        Note::new(value, NSC_ASSET_ID, randomness, tk)
    }

    fn make_nk(bytes: [u8; 32]) -> NullifierKey {
        NullifierKey(bytes)
    }

    // ── Mint Proof Güvenliği ──────────────────────────────────────────────────

    /// PoC: Geçerli mint witness kanıt üretir ve doğrulanır
    #[test]
    fn poc_valid_mint_proof_verified() {
        let note = make_note(1_000_000, [2u8; 32], [1u8; 32]);
        let witness = MintWitness::new(note);
        let proof = witness.prove(SlotId(1)).expect("mint kanıtı üretilmeli");
        let result = proof.verify();
        assert!(result.is_ok() && result.unwrap(), "geçerli mint kanıtı doğrulanmalı");
    }

    /// PoC: Farklı value → farklı commitment (mint commitment value'yu encode eder)
    #[test]
    fn poc_mint_commitment_encodes_value() {
        let n1 = make_note(1_000, [1u8; 32], [1u8; 32]);
        let n2 = make_note(2_000, [1u8; 32], [1u8; 32]);

        assert_ne!(
            n1.commit().0,
            n2.commit().0,
            "farklı value → farklı commitment"
        );
    }

    /// PoC: Farklı randomness → farklı commitment (binding gizleme)
    #[test]
    fn poc_mint_commitment_randomness_sensitive() {
        let n1 = make_note(1_000, [0x01u8; 32], [1u8; 32]);
        let n2 = make_note(1_000, [0x02u8; 32], [1u8; 32]);

        assert_ne!(
            n1.commit().0,
            n2.commit().0,
            "farklı randomness → farklı commitment (collision resistance)"
        );
    }

    /// PoC: Aynı note → aynı commitment (deterministik)
    #[test]
    fn poc_note_commitment_deterministic() {
        let n1 = make_note(500_000, [0xAAu8; 32], [0xBBu8; 32]);
        let n2 = make_note(500_000, [0xAAu8; 32], [0xBBu8; 32]);

        assert_eq!(
            n1.commit().0,
            n2.commit().0,
            "aynı note → aynı commitment (deterministik)"
        );
    }

    // ── Transfer Proof Güvenliği ──────────────────────────────────────────────

    /// PoC: Geçerli transfer kanıtı — input_value >= output_value
    #[test]
    fn poc_valid_transfer_proof_verified() {
        let input_note  = make_note(1_000_000, [0x10u8; 32], [0x11u8; 32]);
        let output_note = make_note(900_000,   [0x20u8; 32], [0x21u8; 32]); // 100k fee
        let nk = make_nk([0x30u8; 32]);

        let witness = TransferWitness::new(input_note, nk, output_note)
            .expect("geçerli transfer witness oluşturulmalı");
        let proof = witness.prove(SlotId(2)).expect("transfer kanıtı üretilmeli");
        assert!(proof.verify().unwrap(), "geçerli transfer kanıtı doğrulanmalı");
    }

    /// PoC: input < output → değer dengesizliği hatası
    #[test]
    fn poc_transfer_value_imbalance_rejected() {
        let input_note  = make_note(100,   [0x10u8; 32], [0x11u8; 32]);
        let output_note = make_note(1_000, [0x20u8; 32], [0x21u8; 32]); // output > input
        let nk = make_nk([0x30u8; 32]);

        let result = TransferWitness::new(input_note, nk, output_note);
        assert!(
            matches!(result, Err(ZkError::ValueImbalance { .. })),
            "input < output → ValueImbalance hatası"
        );
    }

    // ── Burn Proof Güvenliği ──────────────────────────────────────────────────

    /// PoC: Geçerli burn kanıtı doğrulanır
    #[test]
    fn poc_valid_burn_proof_verified() {
        let note = make_note(100_000, [0x50u8; 32], [0x51u8; 32]);
        let nk = make_nk([0x60u8; 32]);
        let recipient = [0xAAu8; 20];

        let witness = BurnWitness::new(note, nk, recipient);
        let proof = witness.prove(SlotId(3)).expect("burn kanıtı üretilmeli");
        assert!(proof.verify().unwrap(), "geçerli burn kanıtı doğrulanmalı");
    }

    // ── Nullifier Güvenliği ───────────────────────────────────────────────────

    /// PoC: Nullifier deterministik türetilir
    #[test]
    fn poc_nullifier_derivation_deterministic() {
        let nk = make_nk([0x77u8; 32]);
        let note = make_note(500, [0x88u8; 32], [0x99u8; 32]);
        let commitment = note.commit();

        let n1 = nk.derive_nullifier(&commitment);
        let n2 = nk.derive_nullifier(&commitment);

        assert_eq!(n1.0, n2.0, "nullifier türetimi deterministik olmalı");
    }

    /// PoC: Farklı nk → farklı nullifier (key binding)
    #[test]
    fn poc_different_nk_different_nullifier() {
        let nk1 = make_nk([0x01u8; 32]);
        let nk2 = make_nk([0x02u8; 32]);
        let note = make_note(1_000, [0xAAu8; 32], [0xBBu8; 32]);
        let commitment = note.commit();

        let n1 = nk1.derive_nullifier(&commitment);
        let n2 = nk2.derive_nullifier(&commitment);

        assert_ne!(n1.0, n2.0, "farklı nk → farklı nullifier");
    }

    /// PoC: Aynı nullifier iki kez harcanamazm (çift harcama imkânsız)
    #[test]
    fn poc_nullifier_prevents_double_spend() {
        let ns = NullifierSet::new();
        let nk = make_nk([0xABu8; 32]);
        let note = make_note(1_000, [0xCDu8; 32], [0xEFu8; 32]);
        let nullifier = nk.derive_nullifier(&note.commit());

        // İlk harcama → başarılı
        ns.insert(nullifier, SlotId(1)).expect("ilk nullifier insert kabul edilmeli");

        // İkinci harcama aynı nullifier → reddedilmeli
        let second = ns.insert(nullifier, SlotId(2));
        assert!(
            matches!(second, Err(ZkError::NullifierAlreadySpent)),
            "aynı nullifier iki kez kullanılamaz — çift harcama imkânsız"
        );
    }

    /// PoC: Farklı nullifier'lar bağımsız olarak harcabilir
    #[test]
    fn poc_different_nullifiers_independent() {
        let ns = NullifierSet::new();
        let nk = make_nk([0x01u8; 32]);

        for i in 1u8..=5 {
            let note = make_note(1_000, [i; 32], [0u8; 32]);
            let nullifier = nk.derive_nullifier(&note.commit());
            ns.insert(nullifier, SlotId(i as u64))
                .unwrap_or_else(|_| panic!("nullifier {i} insert başarısız"));
        }

        assert_eq!(ns.len(), 5, "5 farklı nullifier insert edilmeli");
    }

    /// PoC: NullifierSet büyük hacim — 10.000 benzersiz nullifier yönetilebilir
    #[test]
    fn poc_nullifier_set_handles_large_volume() {
        let ns = NullifierSet::new();
        let nk = make_nk([0xFFu8; 32]);
        let count = 10_000usize;

        for i in 0..count {
            let mut r = [0u8; 32];
            r[0..8].copy_from_slice(&(i as u64).to_le_bytes());
            let note = make_note(1_000, r, [0u8; 32]);
            let nullifier = nk.derive_nullifier(&note.commit());
            ns.insert(nullifier, SlotId(i as u64)).expect("büyük hacim insert başarısız");
        }

        assert_eq!(ns.len(), count, "nullifier seti boyutu doğru olmalı");
    }

    /// PoC: NullifierSet::contains doğru çalışır
    #[test]
    fn poc_nullifier_set_contains_check() {
        let ns = NullifierSet::new();
        let nk = make_nk([0x10u8; 32]);
        let note = make_note(2_000, [0x20u8; 32], [0x30u8; 32]);
        let nullifier = nk.derive_nullifier(&note.commit());

        assert!(!ns.contains(&nullifier), "insert öncesi contains false olmalı");

        ns.insert(nullifier, SlotId(1)).unwrap();

        assert!(ns.contains(&nullifier), "insert sonrası contains true olmalı");
    }

    // ── Proptest: ZK Kriptografik Property Testleri ───────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: Farklı value → farklı commitment (collision resistance)
        #[test]
        fn prop_different_values_different_commitments(
            v1 in 0u128..u128::MAX,
            v2 in 0u128..u128::MAX,
        ) {
            if v1 != v2 {
                let n1 = Note::new(v1, NSC_ASSET_ID, [1u8; 32], [1u8; 32]);
                let n2 = Note::new(v2, NSC_ASSET_ID, [1u8; 32], [1u8; 32]);
                prop_assert_ne!(n1.commit().0, n2.commit().0,
                    "farklı value → farklı commitment");
            }
        }

        /// Property: Farklı randomness → farklı commitment (blinding faktör)
        #[test]
        fn prop_different_randomness_different_commitments(
            r1 in proptest::array::uniform32(0u8..),
            r2 in proptest::array::uniform32(0u8..),
        ) {
            if r1 != r2 {
                let n1 = Note::new(1_000, NSC_ASSET_ID, r1, [1u8; 32]);
                let n2 = Note::new(1_000, NSC_ASSET_ID, r2, [1u8; 32]);
                prop_assert_ne!(n1.commit().0, n2.commit().0,
                    "farklı randomness → farklı commitment");
            }
        }

        /// Property: Commitment deterministik — aynı note = aynı commitment
        #[test]
        fn prop_commitment_deterministic(
            value in 0u128..1_000_000_000_000u128,
            r     in proptest::array::uniform32(0u8..),
            tk    in proptest::array::uniform32(0u8..),
        ) {
            let n1 = Note::new(value, NSC_ASSET_ID, r, tk);
            let n2 = Note::new(value, NSC_ASSET_ID, r, tk);
            prop_assert_eq!(n1.commit().0, n2.commit().0,
                "aynı note → aynı commitment (deterministik)");
        }

        /// Property: Nullifier türetimi deterministik
        #[test]
        fn prop_nullifier_derivation_deterministic(
            nk_bytes in proptest::array::uniform32(1u8..),
            r        in proptest::array::uniform32(0u8..),
        ) {
            let nk = NullifierKey(nk_bytes);
            let note = Note::new(500, NSC_ASSET_ID, r, [0u8; 32]);
            let commitment = note.commit();
            let n1 = nk.derive_nullifier(&commitment);
            let n2 = nk.derive_nullifier(&commitment);
            prop_assert_eq!(n1.0, n2.0, "nullifier türetimi deterministik");
        }

        /// Property: NullifierSet'e aynı nullifier iki kez insert → ikincisi hata
        #[test]
        fn prop_nullifier_double_insert_always_fails(
            nk_bytes in proptest::array::uniform32(1u8..),
            r        in proptest::array::uniform32(0u8..),
        ) {
            let ns = NullifierSet::new();
            let nk = NullifierKey(nk_bytes);
            let note = Note::new(1_000, NSC_ASSET_ID, r, [0u8; 32]);
            let nullifier = nk.derive_nullifier(&note.commit());

            let first = ns.insert(nullifier, SlotId(1));
            prop_assert!(first.is_ok(), "첫번째 insert başarılı olmalı");

            let second = ns.insert(nullifier, SlotId(2));
            prop_assert!(second.is_err(), "ikinci insert her zaman hata vermelidir");
        }
    }
}
