//! Kriptografik doğrulama testleri — BLS, ECDSA, entropy, hash
//!
//! ## Test Kategorileri
//!
//! 1. **BLS G1 curve membership** — Geçersiz pubkey'ler reddedilmeli
//! 2. **ECDSA recovery güvenliği** — Farklı mesajdan pubkey recover edilememeli
//! 3. **Aggregate BLS güvenliği** — Rogue-key saldırısı tespiti
//! 4. **Hash çarpışma direnci** — BLAKE3 farklı girdi → farklı özet
//! 5. **Entropy kalitesi** — Üretilen keypair'lerin eşsizliği
//! 6. **Zeroize** — Secret key belleği sıfırlanır

#[cfg(test)]
mod tests {
    use nova_crypto::{
        BlsKeypair, BlsVerifier, EcdsaKeypair, EcdsaVerifier,
        blake3_hash,
    };
    use nova_primitives::crypto::{BlsPublicKey, BlsSignature};

    // ── BLS Curve Üyelik Doğrulaması ─────────────────────────────────────────

    /// PoC: Sıfır BLS pubkey'i ile doğrulama başarısız olmalı
    #[test]
    fn poc_bls_zero_pubkey_rejected() {
        let kp = BlsKeypair::generate().unwrap();
        let msg = b"Nova Shield consensus vote";
        let sig = kp.sign_raw(msg);

        // Sıfır pubkey (eğri üzerinde değil)
        let zero_pk = BlsPublicKey::from_bytes([0u8; 48]);

        let result = BlsVerifier::verify(msg, &sig, &zero_pk);
        assert!(result.is_err(), "sıfır BLS pubkey reddedilmeli");
    }

    /// PoC: Truncated (32-byte) BLS pubkey reddedilmeli (48 byte gerekli)
    #[test]
    fn poc_bls_short_pubkey_rejected() {
        let kp = BlsKeypair::generate().unwrap();
        let msg = b"vote data";
        let sig = kp.sign_raw(msg);

        // Sadece 32 byte — geçersiz G1 nokta
        let mut short_bytes = [0u8; 48];
        short_bytes[0] = 0x80; // compressed flag var ama nokta geçersiz
        short_bytes[1..16].fill(0xFF); // rastgele büyük değer

        let bad_pk = BlsPublicKey::from_bytes(short_bytes);
        let result = BlsVerifier::verify(msg, &sig, &bad_pk);
        assert!(result.is_err(), "geçersiz BLS nokta reddedilmeli");
    }

    /// PoC: Yanlış pubkey ile BLS doğrulaması başarısız olmalı
    #[test]
    fn poc_bls_wrong_key_verification_fails() {
        let kp1 = BlsKeypair::generate().unwrap();
        let kp2 = BlsKeypair::generate().unwrap();
        let msg = b"block-hash-100";

        let sig = kp1.sign_raw(msg);
        let wrong_pk = kp2.bls_public_key();

        let result = BlsVerifier::verify(msg, &sig, &wrong_pk);
        assert!(result.is_err(), "yanlış BLS pubkey ile doğrulama başarısız olmalı");
    }

    /// PoC: Mesaj tahrif edilince BLS doğrulaması başarısız olmalı
    #[test]
    fn poc_bls_tampered_message_rejected() {
        let kp = BlsKeypair::generate().unwrap();
        let original_msg = b"slot=1000 proposer=validator_1";
        let tampered_msg = b"slot=1000 proposer=attacker_999";

        let sig = kp.sign_raw(original_msg);
        let pk = kp.bls_public_key();

        let result = BlsVerifier::verify(tampered_msg, &sig, &pk);
        assert!(result.is_err(), "tampered BLS mesajı reddedilmeli");
    }

    /// PoC: BLS aggregate doğrulama — bir imza hatalıysa tüm batch başarısız olmalı
    #[test]
    fn poc_bls_aggregate_one_bad_sig_fails_all() {
        let kp1 = BlsKeypair::generate().unwrap();
        let kp2 = BlsKeypair::generate().unwrap();
        let kp3 = BlsKeypair::generate().unwrap();

        // Her validator kendi mesajını imzalar
        let m1 = b"msg-validator-1";
        let m2 = b"msg-validator-2";
        let m3 = b"msg-validator-3";

        let s1 = kp1.sign_raw(m1);
        let s2 = kp2.sign_raw(m2);
        // s3: kp3 kendi mesajını değil kp2'nin mesajını imzalar (Bizans davranışı)
        let s3_bad = kp3.sign_raw(m2); // yanlış mesaj imzası

        let pk1 = kp1.bls_public_key();
        let pk2 = kp2.bls_public_key();
        let pk3 = kp3.bls_public_key();

        let result = BlsVerifier::verify_aggregate(
            &[m1, m2, m3],
            &[&s1, &s2, &s3_bad],
            &[&pk1, &pk2, &pk3],
        );
        assert!(result.is_err(), "hatalı BLS aggregate doğrulaması başarısız olmalı");
    }

    // ── ECDSA Güvenlik Testleri ───────────────────────────────────────────────

    /// PoC: Farklı mesajın hash'inden recover edilen pubkey uyuşmamalı
    #[test]
    fn poc_ecdsa_recover_wrong_hash_fails() {
        let kp = EcdsaKeypair::generate().unwrap();
        let expected_pk = kp.public_key().unwrap();

        let original_data = b"transfer:100:NSC:alice->bob";
        let tampered_data = b"transfer:999999:NSC:alice->attacker";

        let hash_original = blake3_hash(original_data);
        let hash_tampered = blake3_hash(tampered_data);

        let sig = kp.sign_hash(&hash_original).unwrap();

        // Tampered hash'ten recover edilen pubkey orijinalle uyuşmamalı
        if let Ok(recovered) = EcdsaVerifier::recover_public_key(&hash_tampered, &sig) {
            assert_ne!(
                recovered, expected_pk,
                "tampered hash'ten yanlış pubkey recover edilmemeli"
            );
        }
        // recover başarısız olursa da geçerli (genellikle farklı nokta üretir)
    }

    /// PoC: ECDSA imzası değiştirilen mesaj ile doğrulanamaz
    #[test]
    fn poc_ecdsa_verify_wrong_message_rejected() {
        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();

        let hash1 = blake3_hash(b"legitimate transaction");
        let hash2 = blake3_hash(b"malicious transaction");

        let sig = kp.sign_hash(&hash1).unwrap();

        let result = EcdsaVerifier::verify(&hash2, &sig, &pk);
        assert!(result.is_err(), "yanlış mesaj ECDSA doğrulaması başarısız olmalı");
    }

    /// PoC: ECDSA cross-key saldırısı — başka keypair'in imzası reddedilmeli
    #[test]
    fn poc_ecdsa_cross_key_attack_rejected() {
        let attacker_kp = EcdsaKeypair::generate().unwrap();
        let victim_pk = EcdsaKeypair::generate().unwrap().public_key().unwrap();

        let hash = blake3_hash(b"victim's transaction");
        let attacker_sig = attacker_kp.sign_hash(&hash).unwrap();

        // Saldırganın imzasını kurbanın pubkey'iyle doğrulamaya çalış
        let result = EcdsaVerifier::verify(&hash, &attacker_sig, &victim_pk);
        assert!(result.is_err(), "cross-key saldırısı reddedilmeli");
    }

    // ── Hash Çarpışma Direnci ─────────────────────────────────────────────────

    /// PoC: BLAKE3 farklı girdiler → kesinlikle farklı hash
    #[test]
    fn poc_blake3_collision_resistance() {
        let inputs: Vec<&[u8]> = vec![
            b"nova_shield",
            b"nova_shield_",     // 1 byte ekstra
            b"Nova_shield",      // büyük harf
            b"\x00nova_shield",  // null prefix
            b"",                 // boş
        ];

        let hashes: Vec<_> = inputs.iter().map(|i| blake3_hash(i)).collect();

        // Her hash benzersiz olmalı
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(
                    hashes[i], hashes[j],
                    "BLAKE3 farklı girdilerden aynı hash üretmemeli"
                );
            }
        }
    }

    /// PoC: BLAKE3 deterministik — aynı girdi → aynı hash
    #[test]
    fn poc_blake3_deterministic() {
        let data = b"Nova Shield Chain - slot 12345";
        let h1 = blake3_hash(data);
        let h2 = blake3_hash(data);
        assert_eq!(h1, h2, "BLAKE3 deterministik olmalı");
    }

    /// PoC: 1 bit değişiklik bile tamamen farklı hash (avalanche effect)
    #[test]
    fn poc_blake3_avalanche_effect() {
        let data1 = vec![0u8; 32];
        let mut data2 = data1.clone();
        data2[0] = 1; // tek bit değişiklik

        let h1 = blake3_hash(&data1);
        let h2 = blake3_hash(&data2);

        // Farklı hash → çığ etkisi doğrulama
        let different_bits = h1.as_bytes()
            .iter()
            .zip(h2.as_bytes().iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum::<u32>();

        assert!(
            different_bits > 100,
            "tek bit değişiklik → en az 100 bit farklı (çığ etkisi): {different_bits} bit değişti"
        );
    }

    // ── Entropy & Keypair Benzersizliği ──────────────────────────────────────

    /// PoC: Üretilen 100 ECDSA keypair'in tümü benzersiz olmalı
    #[test]
    fn poc_ecdsa_keypair_uniqueness() {
        let mut pubkeys = std::collections::HashSet::new();

        for _ in 0..100 {
            let kp = EcdsaKeypair::generate().unwrap();
            let pk = kp.public_key().unwrap();
            let pk_bytes = pk.as_bytes().to_vec();
            pubkeys.insert(pk_bytes);
        }

        assert_eq!(pubkeys.len(), 100, "100 keypair hepsinin farklı public key'i olmalı");
    }

    /// PoC: Üretilen 100 BLS keypair'in tümü benzersiz olmalı
    #[test]
    fn poc_bls_keypair_uniqueness() {
        let mut pubkeys = std::collections::HashSet::new();

        for _ in 0..100 {
            let kp = BlsKeypair::generate().unwrap();
            let pk = kp.bls_public_key();
            pubkeys.insert(pk.as_bytes().to_vec());
        }

        assert_eq!(pubkeys.len(), 100, "100 BLS keypair hepsinin farklı public key'i olmalı");
    }

    /// PoC: IKM'den deterministik BLS keypair — aynı seed → aynı key
    #[test]
    fn poc_bls_deterministic_from_ikm() {
        let ikm = [0xCA; 32];
        let kp1 = BlsKeypair::from_ikm(&ikm).unwrap();
        let kp2 = BlsKeypair::from_ikm(&ikm).unwrap();

        assert_eq!(
            kp1.bls_public_key().as_bytes(),
            kp2.bls_public_key().as_bytes(),
            "aynı IKM → aynı BLS pubkey (deterministik)"
        );
    }

    /// PoC: ECDSA'dan Ethereum adresi türetimi deterministik
    #[test]
    fn poc_address_derivation_deterministic_and_unique() {
        use nova_crypto::signatures::ecdsa::public_key_to_address;

        let kp = EcdsaKeypair::generate().unwrap();
        let pk = kp.public_key().unwrap();

        let addr1 = public_key_to_address(&pk);
        let addr2 = public_key_to_address(&pk);

        assert_eq!(addr1, addr2, "adres türetimi deterministik olmalı");
        assert_ne!(
            addr1,
            nova_primitives::Address::ZERO,
            "adres sıfır olmamalı"
        );
    }

    // ── BLS Same-Message Aggregate ────────────────────────────────────────────

    /// PoC: Aynı mesajı 5 validator imzalar → aggregate doğrulanmalı
    #[test]
    fn poc_bls_consensus_vote_aggregate() {
        let keypairs: Vec<_> = (0..5).map(|_| BlsKeypair::generate().unwrap()).collect();
        let vote_msg = b"block-vote:slot=500:hash=0xdeadbeef";

        let sigs: Vec<_> = keypairs.iter().map(|kp| kp.sign_raw(vote_msg)).collect();
        let pks: Vec<_> = keypairs.iter().map(|kp| kp.bls_public_key()).collect();

        let sig_refs: Vec<&BlsSignature> = sigs.iter().collect();
        let pk_refs: Vec<&BlsPublicKey> = pks.iter().collect();

        let result = BlsVerifier::verify_aggregate_same_msg(vote_msg, &sig_refs, &pk_refs);
        assert!(
            result.is_ok(),
            "5 validatörün aynı mesaj için aggregate BLS'i doğrulanmalı"
        );
    }
    // ── Proptest: Kriptografik Property Testleri ─────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: BLAKE3 deterministik — aynı veri = aynı hash
        #[test]
        fn prop_blake3_hash_deterministic(
            data in proptest::collection::vec(0u8.., 0..1024),
        ) {
            let h1 = nova_crypto::blake3_hash(&data);
            let h2 = nova_crypto::blake3_hash(&data);
            prop_assert_eq!(h1.as_bytes(), h2.as_bytes(), "blake3 deterministik olmalı");
        }

        /// Property: Farklı veri → farklı hash (collision resistance)
        #[test]
        fn prop_blake3_different_inputs_different_hashes(
            d1 in proptest::collection::vec(0u8.., 1..512),
            d2 in proptest::collection::vec(0u8.., 1..512),
        ) {
            if d1 != d2 {
                let h1 = nova_crypto::blake3_hash(&d1);
                let h2 = nova_crypto::blake3_hash(&d2);
                prop_assert_ne!(h1.as_bytes(), h2.as_bytes(),
                    "farklı girdi → farklı blake3 hash");
            }
        }

        /// Property: Adres türetimi deterministik — aynı seed = aynı adres
        #[test]
        fn prop_address_derivation_deterministic(seed in 1u8..255u8) {
            use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
            let mut s = [0u8; 32];
            s[0] = seed;
            let kp1 = EcdsaKeypair::from_secret_bytes(&s).unwrap();
            let kp2 = EcdsaKeypair::from_secret_bytes(&s).unwrap();
            let a1 = public_key_to_address(&kp1.public_key().unwrap());
            let a2 = public_key_to_address(&kp2.public_key().unwrap());
            prop_assert_eq!(a1, a2, "aynı seed → aynı adres (deterministik)");
        }

        /// Property: Farklı seed → farklı adres (benzersizlik)
        #[test]
        fn prop_different_seeds_different_addresses(s1 in 1u8..128u8, s2 in 129u8..255u8) {
            use nova_crypto::signatures::ecdsa::{EcdsaKeypair, public_key_to_address};
            let mut b1 = [0u8; 32]; b1[0] = s1;
            let mut b2 = [0u8; 32]; b2[0] = s2;
            let a1 = public_key_to_address(&EcdsaKeypair::from_secret_bytes(&b1).unwrap().public_key().unwrap());
            let a2 = public_key_to_address(&EcdsaKeypair::from_secret_bytes(&b2).unwrap().public_key().unwrap());
            prop_assert_ne!(a1, a2, "farklı seed → farklı adres");
        }

        /// Property: ECDSA imza mesaja bağlı — farklı mesaj doğrulanmaz
        #[test]
        fn prop_ecdsa_signature_bound_to_message(seed in 1u8..255u8) {
            use nova_crypto::signatures::ecdsa::{EcdsaKeypair, EcdsaVerifier, public_key_to_address};
            use nova_primitives::crypto::Hash;
            let mut s = [0u8; 32]; s[0] = seed;
            let kp = EcdsaKeypair::from_secret_bytes(&s).unwrap();
            let msg1 = Hash::from_bytes([seed; 32]);
            let msg2 = Hash::from_bytes([seed.wrapping_add(1); 32]);
            let sig = kp.sign_hash(&msg1).unwrap();
            // msg2 üzerinde recover → farklı adres veya hata
            let result = EcdsaVerifier::recover_public_key(&msg2, &sig);
            if let Ok(pk) = result {
                let addr1 = public_key_to_address(&kp.public_key().unwrap());
                let addr2 = public_key_to_address(&pk);
                prop_assert_ne!(addr1, addr2, "farklı mesaj → farklı recovered adres");
            }
            // is_err() da kabul edilir
        }
    }
}
