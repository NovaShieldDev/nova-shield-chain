//! MPC / Threshold İmza güvenlik testleri — CGGMP21 / GG18 / Shamir seviyesinde
//!
//! ## Test Kategorileri
//!
//! 1. **Threshold güvenliği** — t-1 share ile gizli kurtarılamaz
//! 2. **Shamir reconstruct** — t share ile kurtarma doğru
//! 3. **Sahte share** — Geçersiz share ile reconstruct yanlış sonuç
//! 4. **Threshold imza formatı** — Geçersiz format reddedilmeli
//! 5. **P2P şifreli share** — Share iletimi güvenli (ECDH + AES-GCM)
//! 6. **Feldman VSS** — Commitment doğrulaması
//! 7. **Yanlış anahtar/session** — Şifre çözme başarısız

#[cfg(test)]
mod tests {
    use nova_mpc::{
        vss::{feldman_vss_generate, feldman_vss_verify, shamir_reconstruct},
        p2p_encrypt::{EcdhSecret, encrypt_share, decrypt_share},
        types::{ThresholdSignature, PartyIndex, SessionId},
        sign::online::is_valid_signature_format,
        MpcResult,
    };

    // ── Shamir Secret Sharing ─────────────────────────────────────────────────

    /// PoC: 3-of-5 threshold → 3 share ile gizli kurtarılır
    #[test]
    fn poc_shamir_3_of_5_reconstruct() {
        let secret = [0x42u8; 32];
        let dealer = PartyIndex(1);
        let vss = feldman_vss_generate(3, 5, Some(secret), dealer)
            .expect("Feldman VSS üretilmeli");

        // İlk 3 share ile reconstruct
        let shares: Vec<(PartyIndex, [u8; 32])> = vss.shares[..3]
            .iter()
            .map(|s| (s.party, s.value))
            .collect();

        let recovered = shamir_reconstruct(&shares).expect("3-of-5 reconstruct başarılı olmalı");
        assert_eq!(recovered, secret, "3 share ile gizli kurtarılmalı");
    }

    /// PoC: 3-of-5 threshold → 2 share YETMEZ (güvenlik garantisi)
    #[test]
    fn poc_shamir_below_threshold_cannot_reconstruct() {
        let secret = [0xBEu8; 32];
        let dealer = PartyIndex(1);
        let vss = feldman_vss_generate(3, 5, Some(secret), dealer).unwrap();

        // Sadece 2 share (threshold altında)
        let shares: Vec<(PartyIndex, [u8; 32])> = vss.shares[..2]
            .iter()
            .map(|s| (s.party, s.value))
            .collect();

        let result = shamir_reconstruct(&shares);

        // 2 share ile reconstruct ya hata döner ya da yanlış değer üretir
        match result {
            Err(_) => {} // Hata → güvenli
            Ok(recovered) => {
                assert_ne!(
                    recovered, secret,
                    "eşik altı share sayısı gizliyi ifşa etmemeli"
                );
            }
        }
    }

    /// PoC: Farklı subset'ler aynı gizliyi üretir (Shamir tutarlılığı)
    #[test]
    fn poc_shamir_different_subsets_same_secret() {
        let secret = [0xCAu8; 32];
        let dealer = PartyIndex(1);
        let vss = feldman_vss_generate(3, 5, Some(secret), dealer).unwrap();

        let all_shares: Vec<(PartyIndex, [u8; 32])> = vss.shares.iter()
            .map(|s| (s.party, s.value))
            .collect();

        // Subset 1: share 0,1,2
        let r1 = shamir_reconstruct(&all_shares[..3]).unwrap();
        // Subset 2: share 1,2,3
        let r2 = shamir_reconstruct(&all_shares[1..4]).unwrap();
        // Subset 3: share 2,3,4
        let r3 = shamir_reconstruct(&all_shares[2..5]).unwrap();

        assert_eq!(r1, secret, "subset 1 doğru");
        assert_eq!(r2, secret, "subset 2 doğru");
        assert_eq!(r3, secret, "subset 3 doğru");
    }

    // ── Feldman VSS Commitment Doğrulaması ────────────────────────────────────

    /// PoC: Geçerli share → commitment doğrulaması başarılı
    #[test]
    fn poc_feldman_vss_valid_share_verified() {
        let secret = [0xFEu8; 32];
        let dealer = PartyIndex(1);
        let vss = feldman_vss_generate(2, 3, Some(secret), dealer).unwrap();

        for share in &vss.shares {
            let result = feldman_vss_verify(&share.value, share.party, &vss.commitment);
            assert!(
                result.is_ok(),
                "geçerli Feldman VSS share doğrulanmalı: party {:?}", share.party
            );
        }
    }

    /// PoC: Tahrif edilmiş share → commitment doğrulaması başarısız
    #[test]
    fn poc_feldman_vss_tampered_share_rejected() {
        let secret = [0xDDu8; 32];
        let dealer = PartyIndex(1);
        let vss = feldman_vss_generate(2, 3, Some(secret), dealer).unwrap();

        // Tüm byte'ları tersine çevir — büyük sapma, büyük olasılıkla eğri noktası dışında
        let tampered = [0xFFu8; 32];

        let result = feldman_vss_verify(&tampered, vss.shares[0].party, &vss.commitment);
        // Tamamen farklı değer commitment doğrulamasını geçmemeli
        match result {
            Err(_) => {} // hata → güvenli
            Ok(valid) => assert!(!valid, "tahrif edilmiş share commitment doğrulamasını geçmemeli"),
        }
    }

    // ── Threshold İmza Formatı ────────────────────────────────────────────────

    /// PoC: Sıfır r/s ile threshold imza geçersiz
    #[test]
    fn poc_zero_threshold_signature_invalid() {
        let zero_sig = ThresholdSignature {
            r_point: [0u8; 33],
            s_scalar: [0u8; 32],
        };
        assert!(
            !is_valid_signature_format(&zero_sig),
            "sıfır threshold imzası geçersiz sayılmalı"
        );
    }

    /// PoC: Geçerli formatlı threshold imza kabul edilir
    #[test]
    fn poc_valid_format_threshold_signature() {
        let valid_sig = ThresholdSignature {
            r_point: {
                let mut p = [0u8; 33];
                p[0] = 0x02; // compressed pubkey prefix
                p[1] = 0x01;
                p
            },
            s_scalar: {
                let mut s = [0u8; 32];
                s[0] = 0x01; // non-zero
                s
            },
        };
        assert!(
            is_valid_signature_format(&valid_sig),
            "geçerli formatlı threshold imza kabul edilmeli"
        );
    }

    // ── P2P Şifreli Share İletimi ─────────────────────────────────────────────

    /// PoC: Share ECDH + AES-GCM ile şifrelenip doğru şifre çözülür
    #[test]
    fn poc_p2p_share_encryption_roundtrip() {
        let sender   = EcdhSecret::generate();
        let receiver = EcdhSecret::generate();

        let sender_pub   = sender.public_key();
        let receiver_pub = receiver.public_key();

        let original_share = [0x77u8; 32];
        let session_id = SessionId::from_bytes([0x01u8; 32]);
        let from = PartyIndex(1);
        let to   = PartyIndex(2);

        // Gönderici şifreler
        let (nonce, ciphertext) = encrypt_share(
            &sender,
            &receiver_pub,
            &session_id,
            from,
            to,
            &original_share,
        ).expect("şifreleme başarılı olmalı");

        // Alıcı şifre çözer
        let decrypted = decrypt_share(
            &receiver,
            &sender_pub,
            &session_id,
            from,
            to,
            &nonce,
            &ciphertext,
        ).expect("şifre çözme başarılı olmalı");

        assert_eq!(decrypted, original_share, "P2P şifreli share roundtrip başarılı");
    }

    /// PoC: Yanlış özel anahtar ile şifre çözme başarısız
    #[test]
    fn poc_p2p_wrong_key_decryption_fails() {
        let sender   = EcdhSecret::generate();
        let receiver = EcdhSecret::generate();
        let attacker = EcdhSecret::generate(); // üçüncü kişi

        let receiver_pub = receiver.public_key();
        let sender_pub   = sender.public_key();

        let original_share = [0x88u8; 32];
        let session_id = SessionId::from_bytes([0x02u8; 32]);
        let from = PartyIndex(1);
        let to   = PartyIndex(2);

        let (nonce, ciphertext) = encrypt_share(
            &sender,
            &receiver_pub,
            &session_id,
            from,
            to,
            &original_share,
        ).unwrap();

        // Saldırgan kendi özel anahtarıyla şifre çözmeye çalışıyor
        let result = decrypt_share(
            &attacker, // yanlış anahtar
            &sender_pub,
            &session_id,
            from,
            to,
            &nonce,
            &ciphertext,
        );

        assert!(result.is_err(), "yanlış anahtar ile şifre çözme başarısız olmalı");
    }

    /// PoC: Farklı session ID ile şifre çözme başarısız
    #[test]
    fn poc_wrong_session_id_decryption_fails() {
        let sender   = EcdhSecret::generate();
        let receiver = EcdhSecret::generate();

        let receiver_pub = receiver.public_key();
        let sender_pub   = sender.public_key();

        let original_share   = [0x99u8; 32];
        let correct_session  = SessionId::from_bytes([0x03u8; 32]);
        let wrong_session    = SessionId::from_bytes([0x04u8; 32]);
        let from = PartyIndex(1);
        let to   = PartyIndex(2);

        let (nonce, ciphertext) = encrypt_share(
            &sender,
            &receiver_pub,
            &correct_session,
            from,
            to,
            &original_share,
        ).unwrap();

        // Yanlış session ID ile çözme
        let result = decrypt_share(
            &receiver,
            &sender_pub,
            &wrong_session, // yanlış session
            from,
            to,
            &nonce,
            &ciphertext,
        );

        assert!(result.is_err(), "yanlış session ID ile şifre çözme başarısız olmalı");
    }

    /// PoC: Farklı party index ile şifre çözme başarısız (replay protection)
    #[test]
    fn poc_wrong_party_index_decryption_fails() {
        let sender   = EcdhSecret::generate();
        let receiver = EcdhSecret::generate();

        let receiver_pub = receiver.public_key();
        let sender_pub   = sender.public_key();

        let original_share = [0xAAu8; 32];
        let session_id = SessionId::from_bytes([0x05u8; 32]);
        let from = PartyIndex(1);
        let to   = PartyIndex(2);

        let (nonce, ciphertext) = encrypt_share(
            &sender,
            &receiver_pub,
            &session_id,
            from,
            to,
            &original_share,
        ).unwrap();

        // Yanlış party index ile şifre çözme
        let result = decrypt_share(
            &receiver,
            &sender_pub,
            &session_id,
            from,
            PartyIndex(3), // yanlış hedef
            &nonce,
            &ciphertext,
        );

        assert!(result.is_err(), "yanlış party index ile şifre çözme başarısız olmalı");
    }
    // ── Proptest: MPC / VSS Property Testleri ────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: t-1 share hiçbir zaman gizliyi kurtaramaz
        #[test]
        fn prop_below_threshold_cannot_reconstruct(
            threshold in 2u32..6u32,
        ) {
            let party_count = threshold + 2;
            let secret = [0x42u8; 32];
            let vss = feldman_vss_generate(threshold as u8, party_count, Some(secret), PartyIndex(1))
                .expect("VSS üretilmeli");

            // t-1 share al
            let shares: Vec<(PartyIndex, [u8; 32])> = vss.shares[..(threshold - 1) as usize]
                .iter()
                .map(|s| (s.party, s.value))
                .collect();

            let result = shamir_reconstruct(&shares);
            // Ya hata döner ya da yanlış sonuç
            if let Ok(recovered) = result {
                prop_assert_ne!(recovered, secret,
                    "t-1 share ile gizli kurtarılmamalı");
            }
        }

        /// Property: Tam t share ile reconstruct deterministik
        #[test]
        fn prop_full_threshold_reconstruct_deterministic(
            seed in 1u8..255u8,
        ) {
            let secret = [seed; 32];
            let threshold = 3u8;
            let vss = feldman_vss_generate(threshold, 5, Some(secret), PartyIndex(1))
                .expect("VSS üretilmeli");

            let shares: Vec<(PartyIndex, [u8; 32])> = vss.shares[..threshold as usize]
                .iter()
                .map(|s| (s.party, s.value))
                .collect();

            let r1 = shamir_reconstruct(&shares).expect("reconstruct 1 başarılı");
            let r2 = shamir_reconstruct(&shares).expect("reconstruct 2 başarılı");
            prop_assert_eq!(r1, r2, "reconstruct deterministik olmalı");
            prop_assert_eq!(r1, secret, "reconstruct doğru gizliyi vermeli");
        }

        /// Property: ECDH encrypt→decrypt round-trip
        #[test]
        fn prop_ecdh_encrypt_decrypt_roundtrip(
            share_seed in proptest::array::uniform32(1u8..),
        ) {
            let sender = EcdhSecret::generate();
            let receiver = EcdhSecret::generate();
            let sender_pub = sender.public_key();
            let receiver_pub = receiver.public_key();

            let session_id = SessionId::from_bytes([0xABu8; 32]);
            let from = PartyIndex(1);
            let to   = PartyIndex(2);

            let (nonce, cipher) = encrypt_share(&sender, &receiver_pub, &session_id, from, to, &share_seed)
                .expect("şifreleme başarılı");
            let decrypted = decrypt_share(&receiver, &sender_pub, &session_id, from, to, &nonce, &cipher)
                .expect("şifre çözme başarılı");

            prop_assert_eq!(decrypted, share_seed, "ECDH round-trip: decrypt(encrypt(x)) = x");
        }

        /// Property: Farklı session_id → şifre çözme başarısız
        #[test]
        fn prop_wrong_session_id_always_fails(
            share_seed in proptest::array::uniform32(1u8..),
        ) {
            let sender = EcdhSecret::generate();
            let receiver = EcdhSecret::generate();
            let receiver_pub = receiver.public_key();
            let sender_pub = sender.public_key();

            let session_id_a = SessionId::from_bytes([0x01u8; 32]);
            let session_id_b = SessionId::from_bytes([0x02u8; 32]);
            let from = PartyIndex(1);
            let to   = PartyIndex(2);

            let (nonce, cipher) = encrypt_share(&sender, &receiver_pub, &session_id_a, from, to, &share_seed)
                .expect("şifreleme başarılı");

            // Yanlış session_id ile çözme başarısız olmalı
            let result = decrypt_share(&receiver, &sender_pub, &session_id_b, from, to, &nonce, &cipher);
            prop_assert!(result.is_err(), "yanlış session_id → şifre çözme başarısız olmalı");
        }
    }
}
