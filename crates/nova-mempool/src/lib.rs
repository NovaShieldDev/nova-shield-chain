//! # nova-mempool — Şifreli Bellek Havuzu
//!
//! Nova Shield Chain'in MEV-proof, threshold-şifreli işlem havuzu.
//! Whitepaper §5.2 "Şifreli Mempool" ve §6.1.4 "Threshold Şifreleme"
//! spesifikasyonuna göre implemente edilmiştir.
//!
//! ## Genel Bakış
//!
//! Geleneksel blockchain'lerde mempool tamamen şeffaftır; MEV botları sıralanmamış
//! TX'leri okuyarak öne geçebilir (frontrunning) veya sıkıştırabilir (sandwich).
//! Nova Shield Chain bu sorunu **matematiksel olarak imkânsız** kılar:
//!
//! 1. Kullanıcı TX'ini threshold public key ile ECDH hibrit şifreleme kullanarak gönderir.
//! 2. TX, belirlenen slot'a kadar herkes tarafından kör (yalnızca commitment hash) görülür.
//! 3. Slot kapandığında validators threshold decryption share'lerini gönderir.
//! 4. Lagrange interpolasyon ile shared secret yeniden elde edilir.
//! 5. Açılan TX'ler **commitment hash lexicographic sırası** ile sıralanır (MEV-free).
//! 6. Sıralı batch, nova-auction'a aktarılır.
//!
//! ## Şifreleme Şeması (ECDH Threshold Hybrid, §6.1.4)
//!
//! ```text
//! ┌─ KULLANICI TARAFI ──────────────────────────────────────────────────┐
//! │  e    = random scalar (ephemeral secret)                            │
//! │  E    = e * G                     (ephemeral pubkey, 32 byte)       │
//! │  S    = e * threshold_pk          (ECDH shared secret point)        │
//! │  K    = BLAKE3("nova-mempool-aes-key-v1" ‖ S.compress())           │
//! │  body = pad_to_512(borsh_serialize(raw_tx))  [sabit boyut §6.4.2]  │
//! │  (C, nonce) = AES-256-GCM(K, nonce=random[12], aad=slot_le, body) │
//! │  commitment = BLAKE3("nova-mempool-commitment-v1" ‖ E ‖ C ‖ nonce  │
//! │                       ‖ slot_le)                                    │
//! └────────────────────────────────────────────────────────────────────┘
//!
//! ┌─ VALIDATOR TARAFI (threshold decryption) ──────────────────────────┐
//! │  D_i  = sk_share_i * E            (her validator'ın share'i)       │
//! │  S    = Lagrange(D_1..D_t)        = sk_threshold * E               │
//! │  K    = BLAKE3("nova-mempool-aes-key-v1" ‖ S.compress())          │
//! │  body = AES-256-GCM_decrypt(K, nonce, aad=slot_le, C)             │
//! │  raw  = borsh_deserialize(unpad(body))                              │
//! │  assert commitment == BLAKE3(E ‖ C ‖ nonce ‖ slot_le)             │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## MEV-Free Sıralama (§5.2.2, §5.3)
//!
//! Tüm TX'ler commitment hash'lerine göre lexicographic sıralanır.
//! Bu sıra **şifreleme öncesinde deterministik değildir** — slot kapanana kadar
//! hiç kimse hangi TX'in hangi sırada olacağını bilemez. Frontrunning imkânsızdır.
//!
//! ## Sabit Boyut Padding (§6.4.2)
//!
//! Tüm TX'ler şifrelenmeden önce 512 byte'a pad edilir. Bu sayede:
//! - Timing analizi ile TX boyutundan bilgi elde edilemez.
//! - Ağ gözlemcisi farklı boyuttaki TX'leri ayırt edemez.
//!
//! ## Modül Yapısı
//!
//! | Modül | Açıklama |
//! |-------|----------|
//! | [`errors`] | Tüm mempool hata tipleri ve `MempoolResult<T>` |
//! | [`types`] | Veri yapıları: TX, entry, batch, config, stats |
//! | [`ibe`] | ECDH threshold encrypt/decrypt (IBE-benzeri slot-bazlı) |
//! | [`validation`] | Şifreli TX doğrulama kuralları |
//! | [`ordering`] | MEV-free commitment-hash tabanlı TX sıralama |
//! | [`reveal`] | Slot-bazlı threshold decryption koordinasyonu |
//! | [`pool`] | Thread-safe DashMap havuzu, submit/expire/stats |

// ─── MODÜLLER ─────────────────────────────────────────────────────────────────

pub mod check_tx_state;
pub mod eon_key;
pub mod errors;
pub mod gulf_stream;
pub mod ibe;
pub mod nonce_tracker;
pub mod ordering;
pub mod pool;
pub mod reveal;
pub mod types;
pub mod validation;

// ─── ÜST DÜZEY RE-EXPORT'LAR ─────────────────────────────────────────────────

// Hata tipleri
pub use errors::{MempoolError, MempoolResult};
pub use gulf_stream::{
    ForwardedTx, ForwardedTxCache, GulfStreamRouter, GulfStreamStats, LeaderSchedule,
    DEFAULT_MAX_AGE_SLOTS, FORWARD_FANOUT, MAX_FORWARDED_TX_PER_VALIDATOR,
};

// Temel veri tipleri
pub use types::{
    DecryptedTx, EphemeralAesKey, EncryptedTx, MempoolConfig, MempoolEntry, MempoolStats,
    RawTransaction, RevealedBatch, SlotBatch, TxDecryptionShare, TxHash, TxStatus, TxType,
    TxFailureReason,
};

// Şifreleme / çözme
pub use ibe::{
    decrypt::{
        compute_tx_decryption_share, decrypt_transaction, lagrange_combine_shares,
    },
    encrypt::{compute_commitment, encrypt_transaction, verify_commitment, EncryptOutput},
    COMMITMENT_DOMAIN, ECDH_KEY_DOMAIN,
};

// Doğrulama
pub use validation::{
    validate_decrypted_tx_format, validate_encrypted_tx, ValidationConfig,
    SenderBlockLimiter, MAX_TXS_PER_SENDER_PER_BLOCK,
};

// Sıralama
pub use ordering::{
    assign_ordering_indices, compute_batch_merkle_root, compute_revealed_batch_merkle_root,
    deduplicate_and_order, find_duplicates, order_encrypted_batch, order_encrypted_batch_in_place,
    order_revealed_batch, BatchOrderingStats,
};

// Reveal koordinatörü
pub use reveal::{RevealStatusSummary, SlotRevealState};

// NonceTracker (random nonce üretimi)
pub use nonce_tracker::{NonceTracker, NonceValidation, NONCE_HISTORY_DEPTH};

// CheckTxState (üyelik + TX sayım + nonce takip)
pub use check_tx_state::{
    CheckTxResult, CheckTxRejectReason, CheckTxState, MembershipFilter, SenderNonceState,
    BLOOM_BIT_SIZE, MAX_PENDING_PER_SENDER, MAX_PENDING_TX_COUNT,
};

// Havuz
pub use pool::MempoolPool;

// Eon Key (Rolling Shutter keyper analog)
pub use eon_key::{
    BatchIndex, DecryptionKeyAggregator, EonId, EonKeyPublisher, EonPublicKey,
    KeyperDecryptionShare,
};

// ─── ENTEGRASYON TESTLERİ ─────────────────────────────────────────────────────

#[cfg(test)]
mod entegrasyon_testleri {
    //! Modüller arası entegrasyon testleri.
    //!
    //! Her test tam bir uçtan-uca senaryoyu kapsar:
    //! encrypt → submit → decryption_shares → reveal → ordering

    use super::*;
    use curve25519_dalek::{
        constants::RISTRETTO_BASEPOINT_POINT as G,
        scalar::Scalar,
    };
    use nova_primitives::types::{Address, Balance, Nonce, SlotId};
    use rand::rngs::OsRng;

    // ─── Test Yardımcıları ────────────────────────────────────────────────────

    /// (t, n) Shamir threshold keypair üret.
    ///
    /// Polinom: f(x) = sk + a1*x + ... + a_{t-1}*x^{t-1}
    /// Share i: (i, f(i)) for i in 1..=n
    fn threshold_keypair_olustur(
        t: u8,
        n: u8,
    ) -> (Scalar, [u8; 32], Vec<(u8, Scalar)>) {
        let mut rng = OsRng;
        let sk = Scalar::random(&mut rng);
        let pk_bytes = (sk * G).compress().to_bytes();

        // Polinom katsayıları: f(x) = sk + a1*x + ... + a_{t-1}*x^{t-1}
        let coeffs: Vec<Scalar> = (0..(t - 1))
            .map(|_| Scalar::random(&mut rng))
            .collect();

        // f(i) for i in 1..=n
        let mut shares = Vec::with_capacity(n as usize);
        for i in 1u8..=n {
            let x = Scalar::from(i as u64);
            let mut share = sk;
            let mut x_pow = x;
            for c in &coeffs {
                share += c * x_pow;
                x_pow *= x;
            }
            shares.push((i, share));
        }

        (sk, pk_bytes, shares)
    }

    /// Basit test TX oluştur.
    fn test_tx_olustur() -> RawTransaction {
        RawTransaction::new_transfer(
            Address::from_bytes([1u8; 20]),
            Address::from_bytes([2u8; 20]),
            Balance(1_000_000),
            Nonce(42),
            1,
        )
    }

    /// Slot'a özgü test TX oluştur.
    fn test_tx_olustur_nonce(n: u64) -> RawTransaction {
        RawTransaction::new_transfer(
            Address::from_bytes([1u8; 20]),
            Address::from_bytes([2u8; 20]),
            Balance((n * 1000 + 1) as u128),
            Nonce(n),
            1,
        )
    }

    // ─── Test 1 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 1**: Uçtan uca encrypt → submit → reveal.
    ///
    /// - (2, 3) threshold key kurulur
    /// - TX şifrelenir, commitment doğrulanır
    /// - Havuza gönderilir
    /// - 2 decryption share toplanır
    /// - Threshold decrypt ile TX açılır, içerik doğrulanır
    #[test]
    fn uctan_uce_encrypt_submit_reveal() {
        let (_sk, pk_bytes, shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(100);
        let raw_tx = test_tx_olustur();

        // 1. Şifrele
        let out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![0xAAu8; 64])
            .expect("şifreleme başarılı olmalı");

        // 2. Commitment doğrula
        verify_commitment(&out.encrypted_tx).expect("commitment doğrulanmalı");

        // 3. Havuza ekle
        let config = MempoolConfig {
            max_slot_advance: 200,
            decryption_threshold: 2,
            chain_id: 1,
            verify_sender_signature: false,
            ..Default::default()
        };
        let havuz = MempoolPool::new(config);
        havuz.update_current_slot(SlotId(1));
        let hash = havuz.submit(out.encrypted_tx.clone()).expect("submit başarılı");
        assert!(havuz.contains(&hash));

        // 4. Decryption share'leri hesapla (share 1 ve 2 → threshold = 2)
        let epk = out.encrypted_tx.ephemeral_pubkey;
        let share1 = compute_tx_decryption_share(
            shares[0].1.as_bytes(), &epk, shares[0].0, hash, slot,
        ).expect("share 1 hesaplanmalı");
        let share2 = compute_tx_decryption_share(
            shares[1].1.as_bytes(), &epk, shares[1].0, hash, slot,
        ).expect("share 2 hesaplanmalı");

        // 5. Threshold decrypt
        let decrypted = decrypt_transaction(
            &out.encrypted_tx, &[share1, share2], 2, 0,
        ).expect("decrypt başarılı");

        // 6. İçerik doğrula
        assert_eq!(decrypted.raw_tx.from, raw_tx.from);
        assert_eq!(decrypted.raw_tx.value, raw_tx.value);
        assert_eq!(decrypted.raw_tx.nonce, raw_tx.nonce);
        assert_eq!(decrypted.commitment, hash);
    }

    // ─── Test 2 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 2**: (3, 5) threshold ile decrypt.
    #[test]
    fn uc_bes_threshold_tam_decrypt() {
        let (_sk, pk_bytes, shares) = threshold_keypair_olustur(3, 5);
        let slot = SlotId(200);
        let raw_tx = test_tx_olustur();

        let out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![]).unwrap();

        // 3 farklı share: indices 1, 3, 5
        let epk = out.encrypted_tx.ephemeral_pubkey;
        let hash = out.encrypted_tx.commitment;

        let s1 = compute_tx_decryption_share(shares[0].1.as_bytes(), &epk, shares[0].0, hash, slot).unwrap();
        let s3 = compute_tx_decryption_share(shares[2].1.as_bytes(), &epk, shares[2].0, hash, slot).unwrap();
        let s5 = compute_tx_decryption_share(shares[4].1.as_bytes(), &epk, shares[4].0, hash, slot).unwrap();

        let decrypted = decrypt_transaction(&out.encrypted_tx, &[s1, s3, s5], 3, 0)
            .expect("3/5 threshold decrypt başarılı");

        assert_eq!(decrypted.raw_tx.chain_id, 1);
        assert_eq!(decrypted.raw_tx.value, raw_tx.value);
    }

    // ─── Test 3 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 3**: Çoklu TX, MEV-free sıralama deterministik.
    ///
    /// 5 farklı TX şifrelenir, commitment hash'lerine göre sıralanır.
    /// Sıralama deterministik ve bağımsız → MEV imkânsız.
    #[test]
    fn coklu_tx_mev_free_siralama_deterministik() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(300);

        let txler: Vec<EncryptedTx> = (0u64..5)
            .map(|i| {
                let tx = test_tx_olustur_nonce(i);
                encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
            })
            .collect();

        // Sırala
        let sirali = order_encrypted_batch(txler.clone());
        // Aynı girdi → aynı çıktı (deterministik)
        let sirali2 = order_encrypted_batch(txler.clone());

        let commitments1: Vec<TxHash> = sirali.iter().map(|t| t.commitment).collect();
        let commitments2: Vec<TxHash> = sirali2.iter().map(|t| t.commitment).collect();
        assert_eq!(commitments1, commitments2, "sıralama deterministik olmalı");

        // Artan sıra doğrula
        for pencere in sirali.windows(2) {
            assert!(
                pencere[0].commitment <= pencere[1].commitment,
                "commitment hash sıralı olmalı (MEV-free invariant)"
            );
        }

        // Merkle kök deterministik
        let kok1 = compute_batch_merkle_root(&sirali);
        let kok2 = compute_batch_merkle_root(&sirali2);
        assert_eq!(kok1, kok2, "aynı sıralama → aynı merkle kök");
    }

    // ─── Test 4 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 4**: SlotRevealState ile slot bazlı tam açılma akışı.
    #[test]
    fn slot_reveal_state_tam_akis() {
        let (_sk, pk_bytes, shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(400);

        // 3 TX oluştur ve şifrele
        let encrypted_txler: Vec<EncryptedTx> = (0u64..3)
            .map(|i| {
                let tx = test_tx_olustur_nonce(i);
                encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
            })
            .collect();

        // MEV-free ordering uygula
        let sirali_txler = order_encrypted_batch(encrypted_txler.clone());

        // RevealState oluştur
        let mut durum = SlotRevealState::new(sirali_txler.clone(), 2).with_slot(slot);
        assert!(!durum.can_reveal(), "henüz share yok, açılamaz");

        // Her TX için ilk 2 share'i hesapla (threshold = 2)
        let mut tum_shareler = Vec::new();
        for etx in &sirali_txler {
            for (idx, skalal) in shares.iter().take(2) {
                let s = compute_tx_decryption_share(
                    skalal.as_bytes(),
                    &etx.ephemeral_pubkey,
                    *idx,
                    etx.commitment,
                    slot,
                ).unwrap();
                tum_shareler.push(s);
            }
        }

        // Share'leri ekle
        let eklenen = durum.add_decryption_shares(tum_shareler);
        assert!(eklenen > 0, "en az bir share eklenmeli");
        assert!(durum.can_reveal(), "threshold sağlanınca açılabilmeli");

        // Reveal
        let sonuc = durum.reveal_all().expect("reveal başarılı");
        assert_eq!(sonuc.slot, slot);
        assert_eq!(sonuc.transactions.len(), 3, "3 TX açılmalı");
        assert!(sonuc.failed_hashes.is_empty(), "başarısız TX olmamalı");

        // Sıralama indeksleri monoton artan
        for (i, dtx) in sonuc.transactions.iter().enumerate() {
            assert_eq!(dtx.ordering_index, i as u32, "ordering_index monoton artan olmalı");
        }
    }

    // ─── Test 5 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 5**: Havuz kapasitesi ve slot expire.
    #[test]
    fn havuz_kapasite_ve_slot_expire() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);

        let config = MempoolConfig {
            max_total_txs: 100,
            max_slot_advance: 50,
            decryption_threshold: 2,
            chain_id: 1,
            verify_sender_signature: false,
            ..Default::default()
        };
        let havuz = MempoolPool::new(config);
        havuz.update_current_slot(SlotId(10));

        // 5 TX ekle, slot 11, 12, 13 arasında dağıt
        let mut eklenenler = Vec::new();
        for i in 0u64..5 {
            let tx = test_tx_olustur_nonce(i);
            let slot = SlotId(11 + i % 3);
            let out = encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap();
            let h = havuz.submit(out.encrypted_tx).unwrap();
            eklenenler.push(h);
        }
        assert_eq!(havuz.len(), 5, "5 TX eklenmeli");

        // Slot 12'den öncekini (slot 11) expire et
        let silinen = havuz.expire_before_slot(SlotId(12));
        // Slot 11'e ait TX'ler silinmeli (i=0,3 → slot 11)
        assert!(silinen > 0, "slot expire çalışmalı");
        assert!(havuz.len() < 5, "expire sonrası daha az TX bulunmalı");
    }

    // ─── Test 6 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 6**: Geçersiz TX validation reddedilmeli.
    #[test]
    fn gecersiz_tx_validation_reddedilmeli() {
        let vconfig = ValidationConfig {
            verify_commitment: true,
            ..Default::default()
        };

        // Sıfır commitment → hata
        let etx_sifir_commitment = EncryptedTx {
            commitment: TxHash::ZERO,
            ephemeral_pubkey: [1u8; 32], // geçerli değil ama testi erken kıracak
            aes_ciphertext: vec![0u8; 10],
            aes_nonce: [0u8; 12],
            target_slot: SlotId(101),
            sender: Address::from_bytes([1u8; 20]),
            signature: vec![],
            plaintext_size_hint: 0,
        };
        let hata = validate_encrypted_tx(&etx_sifir_commitment, &vconfig, SlotId(100));
        assert!(hata.is_err(), "sıfır commitment reddedilmeli");

        // Boş ciphertext → hata
        let etx_bos_cipher = EncryptedTx {
            commitment: TxHash::from_bytes([1u8; 32]),
            ephemeral_pubkey: [2u8; 32],
            aes_ciphertext: vec![], // boş
            aes_nonce: [0u8; 12],
            target_slot: SlotId(101),
            sender: Address::from_bytes([1u8; 20]),
            signature: vec![],
            plaintext_size_hint: 0,
        };
        let hata2 = validate_encrypted_tx(&etx_bos_cipher, &vconfig, SlotId(100));
        assert!(hata2.is_err(), "boş ciphertext reddedilmeli");
    }

    // ─── Test 7 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 7**: Duplicate TX submission reddedilmeli.
    #[test]
    fn duplicate_tx_submission_reddedilmeli() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(500);

        let config = MempoolConfig {
            max_slot_advance: 200,
            decryption_threshold: 2,
            chain_id: 1,
            verify_sender_signature: false,
            ..Default::default()
        };
        let havuz = MempoolPool::new(config);
        havuz.update_current_slot(SlotId(300));

        let raw_tx = test_tx_olustur();
        let out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![]).unwrap();
        let etx = out.encrypted_tx;

        // İlk submit başarılı
        havuz.submit(etx.clone()).expect("ilk submit başarılı olmalı");

        // Aynı TX tekrar → DuplicateTransaction hatası
        let hata = havuz.submit(etx);
        assert!(hata.is_err(), "duplicate TX reddedilmeli");
        assert!(
            matches!(hata.unwrap_err(), MempoolError::DuplicateTransaction { .. }),
            "DuplicateTransaction hatası bekleniyor"
        );
    }

    // ─── Test 8 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 8**: Batch istatistikleri ve deduplication.
    #[test]
    fn batch_istatistikleri_ve_deduplication() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(600);

        // tx_a iki kez → duplicate
        let tx_a_encrypted = {
            let tx = test_tx_olustur();
            encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
        };
        let tx_b_encrypted = {
            let tx = test_tx_olustur_nonce(99);
            encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
        };

        let txler = vec![
            tx_a_encrypted.clone(),
            tx_a_encrypted.clone(), // duplicate
            tx_b_encrypted,
        ];

        // Duplicate bul
        let duplar = find_duplicates(&txler);
        assert_eq!(duplar.len(), 1, "1 duplicate commitment olmalı");

        // Dedup + sırala
        let temiz = deduplicate_and_order(txler);
        assert_eq!(temiz.len(), 2, "2 benzersiz TX kalmalı");

        // İstatistikler
        let stats = BatchOrderingStats::compute(&temiz);
        assert_eq!(stats.ordered_count, 2);
        assert_eq!(stats.duplicate_count, 0, "temizlenmiş batch'te duplicate kalmamalı");
    }

    // ─── Test 9 ───────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 9**: Merkle kökü slot değişince farklılaşmalı.
    #[test]
    fn merkle_kok_slota_gore_benzersiz() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);
        let raw_tx = test_tx_olustur();

        let slot_a = SlotId(700);
        let slot_b = SlotId(701);

        let etx_a = encrypt_transaction(&raw_tx, &pk_bytes, slot_a, vec![]).unwrap().encrypted_tx;
        let etx_b = encrypt_transaction(&raw_tx, &pk_bytes, slot_b, vec![]).unwrap().encrypted_tx;

        let kok_a = compute_batch_merkle_root(&[etx_a]);
        let kok_b = compute_batch_merkle_root(&[etx_b]);

        assert_ne!(
            kok_a, kok_b,
            "farklı slot → farklı commitment → farklı merkle kök"
        );
    }

    // ─── Test 10 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 10**: Reveal sonrası ordering_index monoton artar.
    #[test]
    fn reveal_sonrasi_ordering_index_monoton_artar() {
        let (_sk, pk_bytes, shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(800);

        let encrypted_txler: Vec<EncryptedTx> = (0u64..4)
            .map(|i| {
                let tx = test_tx_olustur_nonce(i * 7 + 3);
                encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
            })
            .collect();

        let sirali = order_encrypted_batch(encrypted_txler);
        let mut durum = SlotRevealState::new(sirali.clone(), 2).with_slot(slot);

        // Her TX için 2 share hesapla
        let mut shareler = Vec::new();
        for etx in &sirali {
            for (idx, skalal) in shares.iter().take(2) {
                let s = compute_tx_decryption_share(
                    skalal.as_bytes(),
                    &etx.ephemeral_pubkey,
                    *idx,
                    etx.commitment,
                    slot,
                ).unwrap();
                shareler.push(s);
            }
        }

        durum.add_decryption_shares(shareler);
        let sonuc = durum.reveal_all().expect("reveal başarılı");

        // ordering_index değerleri 0, 1, 2, 3 sırasında olmalı
        for pencere in sonuc.transactions.windows(2) {
            assert!(
                pencere[0].ordering_index < pencere[1].ordering_index,
                "ordering_index monoton artmalı: {} < {}",
                pencere[0].ordering_index,
                pencere[1].ordering_index
            );
        }
    }

    // ─── Test 11 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 11**: Yetersiz threshold share → InsufficientDecryptionShares.
    #[test]
    fn yetersiz_share_hata_dondurur() {
        let (_sk, pk_bytes, shares) = threshold_keypair_olustur(3, 5);
        let slot = SlotId(900);

        let raw_tx = test_tx_olustur();
        let out = encrypt_transaction(&raw_tx, &pk_bytes, slot, vec![]).unwrap();
        let etx = out.encrypted_tx;
        let hash = etx.commitment;

        // Sadece 2 share topla (threshold = 3)
        let s1 = compute_tx_decryption_share(
            shares[0].1.as_bytes(), &etx.ephemeral_pubkey, shares[0].0, hash, slot,
        ).unwrap();
        let s2 = compute_tx_decryption_share(
            shares[1].1.as_bytes(), &etx.ephemeral_pubkey, shares[1].0, hash, slot,
        ).unwrap();

        let sonuc = decrypt_transaction(&etx, &[s1, s2], 3, 0);
        assert!(sonuc.is_err(), "2 share ile 3/5 threshold açılmamalı");

        match sonuc.unwrap_err() {
            MempoolError::InsufficientDecryptionShares { required, available } => {
                assert_eq!(required, 3);
                assert_eq!(available, 2);
            }
            e => panic!("beklenmedik hata tipi: {e:?}"),
        }
    }

    // ─── Test 12 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 12**: Havuz istatistikleri doğru raporlanıyor.
    #[test]
    fn havuz_istatistikleri_dogru_raporlaniyor() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);

        let config = MempoolConfig {
            max_slot_advance: 200,
            decryption_threshold: 2,
            chain_id: 1,
            verify_sender_signature: false,
            ..Default::default()
        };
        let havuz = MempoolPool::new(config);
        havuz.update_current_slot(SlotId(1000));

        let baslangic = havuz.stats();
        assert_eq!(baslangic.pending_count, 0);
        assert_eq!(baslangic.total_submitted, 0);

        for i in 0u64..3 {
            let tx = test_tx_olustur_nonce(i);
            let out = encrypt_transaction(&tx, &pk_bytes, SlotId(1001), vec![]).unwrap();
            havuz.submit(out.encrypted_tx).unwrap();
        }

        let stats = havuz.stats();
        assert_eq!(stats.pending_count, 3);
        assert_eq!(stats.total_submitted, 3);
    }

    // ─── Test 13 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 13**: Ordering index atama doğruluğu.
    #[test]
    fn ordering_index_atama_dogrulugu() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(1100);

        let txler: Vec<EncryptedTx> = (0u64..6)
            .map(|i| {
                let tx = test_tx_olustur_nonce(i);
                encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
            })
            .collect();

        let sirali = order_encrypted_batch(txler);
        let indeksler = assign_ordering_indices(&sirali);

        assert_eq!(indeksler.len(), 6);
        for (i, (hash, idx)) in indeksler.iter().enumerate() {
            assert_eq!(*idx, i as u32, "index={i} eşleşmeli");
            assert_eq!(*hash, sirali[i].commitment, "hash eşleşmeli");
        }
    }

    // ─── Test 14 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 14**: RevealState durum özeti (status_summary).
    #[test]
    fn reveal_state_durum_ozeti() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(1200);
        let threshold: u8 = 2;

        let txler: Vec<EncryptedTx> = (0u64..2)
            .map(|i| {
                let tx = test_tx_olustur_nonce(i);
                encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap().encrypted_tx
            })
            .collect();

        let durum = SlotRevealState::new(txler, threshold).with_slot(slot);
        let ozet = durum.status_summary();

        assert_eq!(ozet.slot, slot);
        assert_eq!(ozet.total_txs, 2);
        assert_eq!(ozet.threshold, threshold);
        assert!(!ozet.can_reveal, "share olmadan açılamaz");
        assert!(!ozet.revealed);
    }

    // ─── Test 15 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 15**: Farklı slotlardaki TX'ler ayrı yönetilmeli.
    #[test]
    fn farkli_slotlardaki_txler_ayri_yonetilmeli() {
        let (_sk, pk_bytes, _shares) = threshold_keypair_olustur(2, 3);

        let config = MempoolConfig {
            max_slot_advance: 50,
            decryption_threshold: 2,
            chain_id: 1,
            verify_sender_signature: false,
            ..Default::default()
        };
        let havuz = MempoolPool::new(config);
        havuz.update_current_slot(SlotId(10));

        // Slot 11, 12, 13'e TX ekle
        for (i, slot_num) in [(0u64, 11u64), (1, 12), (2, 13)] {
            let tx = test_tx_olustur_nonce(i);
            let out = encrypt_transaction(&tx, &pk_bytes, SlotId(slot_num), vec![]).unwrap();
            havuz.submit(out.encrypted_tx).unwrap();
        }

        // Her slot'ta 1 TX olmalı
        assert_eq!(havuz.slot_tx_count(SlotId(11)), 1);
        assert_eq!(havuz.slot_tx_count(SlotId(12)), 1);
        assert_eq!(havuz.slot_tx_count(SlotId(13)), 1);
        assert_eq!(havuz.slot_tx_count(SlotId(99)), 0, "boş slot 0 dönmeli");
    }

    // ─── Test 16 ──────────────────────────────────────────────────────────────

    /// **Entegrasyon Testi 16**: Tek validator'dan birden fazla share → dedup.
    #[test]
    fn tek_validatordan_birden_fazla_share_dedup() {
        let (_sk, pk_bytes, shares) = threshold_keypair_olustur(2, 3);
        let slot = SlotId(1300);

        let tx = test_tx_olustur();
        let out = encrypt_transaction(&tx, &pk_bytes, slot, vec![]).unwrap();
        let etx = out.encrypted_tx;
        let txler = vec![etx.clone()];

        let mut durum = SlotRevealState::new(txler, 2).with_slot(slot);

        // Validator 1'den iki kez share gönder
        let s1a = compute_tx_decryption_share(
            shares[0].1.as_bytes(), &etx.ephemeral_pubkey, shares[0].0, etx.commitment, slot,
        ).unwrap();
        let s1b = s1a.clone(); // tamamen aynı share

        let s2 = compute_tx_decryption_share(
            shares[1].1.as_bytes(), &etx.ephemeral_pubkey, shares[1].0, etx.commitment, slot,
        ).unwrap();

        // 3 share gönder ama gerçekte 2 benzersiz share (s1a dedup edilmeli)
        durum.add_decryption_shares(vec![s1a, s1b, s2]);

        // 2 benzersiz share yeterli (threshold = 2)
        assert!(durum.can_reveal(), "dedup sonrası 2 benzersiz share yeterli olmalı");

        let sonuc = durum.reveal_all().expect("reveal başarılı");
        assert_eq!(sonuc.transactions.len(), 1);
    }
}
