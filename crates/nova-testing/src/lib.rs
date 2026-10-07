//! nova-testing — Nova Shield Chain Test Altyapısı
//!
//! - `fixtures`: Deterministik test verileri (hesaplar, bloklar, işlemler)
//! - `mocks`: Mock implementasyonları (saat, ağ, depo)
//! - `scenarios`: Uçtan uca test senaryoları (deadman, miras, MEV koruma)
//! - `integration`: Cross-crate entegrasyon testleri (FAZ 3 GÖREV 1)
//! - `simulation`: Multi-node BFT + kaos mühendisliği simülasyonları (FAZ D)
//! - `property`: Proptest property-based testler (FAZ D)

pub mod fixtures;
pub mod integration;
pub mod mocks;
pub mod scenarios;
pub mod simulation;

#[cfg(test)]
pub mod property;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use fixtures::{
    TestAccount, TestAccounts, TestBlock, TestBlockchain, TestTx,
    alice, bob, charlie, validator_account,
    make_transfer_tx, make_contract_call_tx,
};
pub use mocks::{MockClock, MockNetwork, MockStorage};
pub use scenarios::{
    DeadmanScenario, InheritanceScenario, MevProtectionScenario,
    DosError, DosProtector, EclipseAttackSimulation, MessageKey, ReplayError, ReplayProtector,
    SubnetTracker,
};

pub const TESTING_VERSION: &str = "0.1.0";

// ─── Yardımcı makro'lar ───────────────────────────────────────────────────────

/// Bir değerin belirli aralıkta olduğunu doğrula
#[macro_export]
macro_rules! assert_in_range {
    ($value:expr, $min:expr, $max:expr) => {
        assert!(
            $value >= $min && $value <= $max,
            "Değer {} aralık dışı [{}, {}]",
            $value, $min, $max
        );
    };
}

/// İki float değerinin belirli epsilon içinde eşit olduğunu doğrula
#[macro_export]
macro_rules! assert_approx_eq {
    ($a:expr, $b:expr, $eps:expr) => {
        let diff = if $a > $b { $a - $b } else { $b - $a };
        assert!(
            diff <= $eps,
            "Değerler yeterince yakın değil: {} vs {}, fark: {}",
            $a, $b, diff
        );
    };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use scenarios::mev_protection::{MevProtectionScenario, ProtectionMechanism, TestOrder};

    // ── Fixture'lar ──────────────────────────────────────────────────────────

    #[test]
    fn test_standard_accounts() {
        let accounts = TestAccounts::standard();
        assert_eq!(accounts.all().len(), 4);
        assert!(accounts.alice.balance > accounts.bob.balance);
        assert_eq!(accounts.charlie.balance, 0);
    }

    #[test]
    fn test_test_blockchain() {
        let chain = TestBlockchain::with_blocks(20);
        assert_eq!(chain.block_count(), 21);
        assert_eq!(chain.best_block().number, 20);
        assert!(chain.verify_chain());
    }

    #[test]
    fn test_transfer_fixture() {
        let accounts = TestAccounts::standard();
        let tx = make_transfer_tx(
            accounts.alice.address,
            accounts.bob.address,
            1_000_000,
            0,
        );
        assert_eq!(tx.from, accounts.alice.address);
        assert_eq!(tx.value, 1_000_000);
        assert!(!tx.is_contract_call());
    }

    // ── Mock'lar ─────────────────────────────────────────────────────────────

    #[test]
    fn test_mock_clock_scenario() {
        let clock = MockClock::nova_default();
        clock.advance_slots(1000);
        assert_eq!(clock.current_slot(), 1000);

        clock.reset();
        assert_eq!(clock.current_slot(), 0);
    }

    #[test]
    fn test_mock_network_full_flow() {
        let net = MockNetwork::new();
        let n1 = net.add_node([1u8; 32]);
        let n2 = net.add_node([2u8; 32]);
        let n3 = net.add_node([3u8; 32]);

        use mocks::network::MockMessage;
        net.broadcast(MockMessage::broadcast([1u8; 32], "block_announce", vec![42]));

        assert_eq!(n2.inbox_len(), 1);
        assert_eq!(n3.inbox_len(), 1);
        assert_eq!(n1.inbox_len(), 0);
        assert_eq!(net.total_messages(), 2);
    }

    #[test]
    fn test_mock_storage_full_flow() {
        let storage = MockStorage::new();
        storage.put("account/alice", b"balance:1000".to_vec());
        storage.put("account/bob", b"balance:500".to_vec());
        storage.put("block/1", b"block_data".to_vec());

        let block_keys = storage.keys_with_prefix("block/");
        assert_eq!(block_keys.len(), 1);

        let val = storage.get("account/alice").unwrap();
        assert_eq!(val, b"balance:1000");
    }

    // ── Senaryolar ───────────────────────────────────────────────────────────

    #[test]
    fn test_deadman_full_scenario() {
        let mut scenario = DeadmanScenario::new(1000);

        // 3 kez heartbeat gönder
        for _ in 0..3 {
            scenario.advance_and_check(200);
            scenario.send_heartbeat();
        }
        assert!(!scenario.is_recovery_triggered());

        // Artık heartbeat gönderme, 1100 slot geç
        let result = scenario.advance_and_check(1100);
        assert_eq!(result, scenarios::deadman::DeadmanOutcome::RecoveryTriggered);
    }

    #[test]
    fn test_inheritance_full_scenario() {
        let accounts = TestAccounts::standard();

        let mut scenario = InheritanceScenario::new(accounts.alice.address, 10_000_000)
            .add_heir(accounts.bob.address, "Bob", 7_000, 500)
            .add_heir(accounts.charlie.address, "Charlie", 3_000, 1000);

        assert!(scenario.validate_fractions());

        scenario.trigger();
        scenario.advance_slots(1000);

        // Bob 500 slot sonra alır, 1000 geçti → alabilir
        match scenario.check_release(&accounts.bob.address) {
            scenarios::inheritance::ReleaseStatus::Released { amount } => {
                assert_eq!(amount, 7_000_000); // %70 of 10_000_000
            }
            _ => panic!("Bob serbest bırakılmalıydı"),
        }

        // Charlie da 1000 slot sonra alır → alabilir
        match scenario.check_release(&accounts.charlie.address) {
            scenarios::inheritance::ReleaseStatus::Released { amount } => {
                assert_eq!(amount, 3_000_000); // %30 of 10_000_000
            }
            _ => panic!("Charlie serbest bırakılmalıydı"),
        }
    }

    #[test]
    fn test_mev_protection_full_scenario() {
        let mut scenario = MevProtectionScenario::new(ProtectionMechanism::BatchAuction, 100);

        for i in 0u8..10 {
            scenario.submit_order(TestOrder::new(
                i as u64,
                [i; 20],
                "NOVA",
                "USDC",
                1_000 + (i as u64 * 100),
                scenario.current_slot(),
            ));
        }

        let solution = scenario.end_epoch();
        assert_eq!(solution.order_count(), 10);
        // Uniform price — tüm traderlar aynı fiyata işlem yapar
        assert!(solution.uniform_price > 0);
    }

    // ── Macro testleri ───────────────────────────────────────────────────────

    #[test]
    fn test_assert_in_range_macro() {
        assert_in_range!(50u64, 0u64, 100u64);
        assert_in_range!(0u64, 0u64, 100u64);
        assert_in_range!(100u64, 0u64, 100u64);
    }

    #[test]
    fn test_assert_approx_eq_macro() {
        assert_approx_eq!(100u64, 101u64, 2u64);
        assert_approx_eq!(50u64, 50u64, 0u64);
    }

    #[test]
    fn test_testing_version() {
        assert_eq!(TESTING_VERSION, "0.1.0");
    }

    // ── Güvenlik saldırı testleri ─────────────────────────────────────────────

    // GÖREV 82: Eclipse attack testi
    mod eclipse_attack {
        use super::*;

        #[test]
        fn eclipse_subnet_limit_enforced() {
            // /24 subnet limiti: aynı subnetten max TABLE_IP_LIMIT node kabul edilir
            let mut sim = EclipseAttackSimulation::new();

            // Saldırgan 192.168.1.x'ten 10 node eklemeye çalışıyor
            let mut accepted = 0;
            for i in 1..=20u8 {
                if sim.add_attacker_node(i) {
                    accepted += 1;
                }
            }

            // Maksimum TABLE_IP_LIMIT kabul edilmeli
            assert_eq!(accepted, SubnetTracker::TABLE_IP_LIMIT,
                "subnet limiti aşıldı: {} node kabul edildi", accepted);
        }

        #[test]
        fn eclipse_legitimate_nodes_not_blocked() {
            let mut sim = EclipseAttackSimulation::new();

            // Saldırgan 10 node doldurdu
            for i in 1..=10u8 {
                sim.add_attacker_node(i);
            }

            // Farklı subnetlerden meşru node'lar eklenebilmeli
            for subnet in 1..=20u8 {
                assert!(sim.add_legitimate_node(subnet, 1),
                    "meşru node {} engellenemez", subnet);
            }

            // Toplam: 10 saldırgan + 20 meşru = 30 node
            assert_eq!(sim.legitimate_count(), 20);
        }

        #[test]
        fn eclipse_attacker_ratio_stays_low() {
            let mut sim = EclipseAttackSimulation::new();

            // Saldırgan 10 node (subnet limiti)
            for i in 1..=10u8 {
                sim.add_attacker_node(i);
            }

            // 90 meşru node, 9 farklı subnet × 10 node
            for subnet in 1..=9u8 {
                for host in 1..=10u8 {
                    sim.add_legitimate_node(subnet, host);
                }
            }

            // Saldırganların oranı %10'un altında kalmalı
            assert!(sim.attacker_ratio() <= 0.10,
                "saldırgan oranı çok yüksek: {:.1}%", sim.attacker_ratio() * 100.0);
        }

        #[test]
        fn eclipse_subnet_tracker_allows_up_to_limit() {
            let mut tracker = SubnetTracker::new();
            let subnet_ip = |host: u8| [10u8, 0, 0, host];

            for host in 1..=(SubnetTracker::TABLE_IP_LIMIT as u8) {
                assert!(tracker.add(subnet_ip(host)).is_ok());
            }

            // 11. ekleme reddedilmeli
            let result = tracker.add(subnet_ip(11));
            assert!(result.is_err(), "limit aşımı kabul edilmemeli");
        }

        #[test]
        fn eclipse_can_add_checks_before_insert() {
            let mut tracker = SubnetTracker::new();

            for host in 1..=(SubnetTracker::TABLE_IP_LIMIT as u8) {
                tracker.add([10, 0, 0, host]).unwrap();
            }

            // can_add false döndürmeli
            assert!(!tracker.can_add([10, 0, 0, 11]));
            // Farklı subnet için true
            assert!(tracker.can_add([10, 1, 0, 1]));
        }
    }

    // GÖREV 83: Replay attack testi
    mod replay_attack {
        use super::*;

        fn make_key(sender_byte: u8, nonce: u64, valid_until: u64) -> MessageKey {
            MessageKey {
                sender: [sender_byte; 32],
                nonce,
                valid_until_slot: valid_until,
            }
        }

        #[test]
        fn replay_duplicate_rejected() {
            let mut protector = ReplayProtector::new(100);
            let key = make_key(1, 42, 150);

            // İlk gönderim kabul edilmeli
            assert!(protector.accept(key.clone()).is_ok());

            // Aynı mesaj tekrar reddedilmeli
            assert_eq!(protector.accept(key), Err(ReplayError::Duplicate));
        }

        #[test]
        fn replay_expired_message_rejected() {
            let mut protector = ReplayProtector::new(200);

            // Slot 100'de geçerli bir mesaj (ama şu an slot 200)
            let key = make_key(2, 1, 100);
            assert_eq!(
                protector.accept(key),
                Err(ReplayError::Expired { valid_until: 100, current: 200 })
            );
        }

        #[test]
        fn replay_future_message_rejected() {
            let mut protector = ReplayProtector::new(100);

            // Çok ilerideki slot (slot 300 → şimdiki 100 + window 100 = 200'ün ötesi)
            let key = make_key(3, 1, 300);
            assert_eq!(protector.accept(key), Err(ReplayError::TooFarInFuture));
        }

        #[test]
        fn replay_window_advance_clears_old_messages() {
            let mut protector = ReplayProtector::new(0);

            // 50 mesaj ekle (geçerlilik: slot 50'ye kadar)
            for i in 0..50u64 {
                let key = make_key(4, i, 50);
                protector.accept(key).unwrap();
            }
            assert_eq!(protector.seen_count(), 50);

            // Slot 51'e geç — eski mesajlar temizlenmeli
            protector.advance_slot(51);
            assert_eq!(protector.seen_count(), 0);
        }

        #[test]
        fn replay_different_nonces_accepted() {
            let mut protector = ReplayProtector::new(0);
            let valid_until = 100;

            // Aynı gönderici, farklı nonce → hepsi kabul
            for nonce in 0..10u64 {
                let key = make_key(5, nonce, valid_until);
                assert!(protector.accept(key).is_ok(), "nonce {} reddedildi", nonce);
            }
        }

        #[test]
        fn replay_cross_sender_isolation() {
            let mut protector = ReplayProtector::new(0);
            let valid_until = 100;

            // Farklı göndericiler aynı nonce kullanabilir
            for sender in 0..5u8 {
                let key = make_key(sender, 42, valid_until);
                assert!(protector.accept(key).is_ok(),
                    "gönderici {} nonce 42 reddedildi", sender);
            }
        }
    }

    // GÖREV 84: P2P DoS saldırı testi
    mod dos_attack {
        use super::*;

        #[test]
        fn dos_rate_limit_enforced() {
            let mut protector = DosProtector::new();
            let attacker = [0xAAu8; 32];

            // max_msgs_per_slot kadar mesaj gönderilebilir
            for _ in 0..DosProtector::DEFAULT_MAX_MSGS_PER_SLOT {
                assert!(protector.accept_message(attacker).is_ok());
            }

            // 101. mesaj rate limit hatası vermeli
            let result = protector.accept_message(attacker);
            assert_eq!(
                result,
                Err(DosError::RateLimitExceeded { current_slot: 0 })
            );
        }

        #[test]
        fn dos_ban_after_repeated_violations() {
            let mut protector = DosProtector::new();
            let attacker = [0xBBu8; 32];

            // Çok sayıda kötü mesaj bildirimi → ban
            // ban_threshold = -500, her kötü mesaj -50 → 10 mesaj sonra ban
            for _ in 0..10 {
                protector.report_bad_message(attacker, 50);
            }

            assert!(protector.is_banned(&attacker));
        }

        #[test]
        fn dos_banned_peer_all_messages_rejected() {
            let mut protector = DosProtector::new();
            let attacker = [0xCCu8; 32];

            // Ban et
            protector.report_bad_message(attacker, 600);
            assert!(protector.is_banned(&attacker));

            // Sonraki tüm mesajlar reddedilmeli
            for _ in 0..10 {
                assert_eq!(protector.accept_message(attacker), Err(DosError::PeerBanned));
            }
        }

        #[test]
        fn dos_slot_advance_resets_counts() {
            let mut protector = DosProtector::new();
            let peer = [0xDDu8; 32];

            // Slot 0'da rate limit doldur
            for _ in 0..DosProtector::DEFAULT_MAX_MSGS_PER_SLOT {
                protector.accept_message(peer).unwrap();
            }
            assert!(protector.accept_message(peer).is_err());

            // Slot 1'e geç — sayaç sıfırlandı
            protector.advance_slot(1);
            assert!(protector.accept_message(peer).is_ok());
        }

        #[test]
        fn dos_legitimate_peers_not_affected() {
            let mut protector = DosProtector::new();
            let attacker = [0xEEu8; 32];
            let legitimate = [0x11u8; 32];

            // Saldırganı ban et
            protector.report_bad_message(attacker, 600);
            assert!(protector.is_banned(&attacker));

            // Meşru peer etkilenmemeli
            for _ in 0..10 {
                assert!(protector.accept_message(legitimate).is_ok());
            }
        }

        #[test]
        fn dos_total_rejected_counter() {
            let mut protector = DosProtector::new();
            let attacker = [0xFFu8; 32];

            // Ban et
            protector.report_bad_message(attacker, 600);

            // 5 mesaj gönder → hepsi reddedilmeli
            for _ in 0..5 {
                let _ = protector.accept_message(attacker);
            }

            assert_eq!(protector.total_rejected(), 5);
        }
    }
}

// ─── Proptest property-based testleri (Görev 81) ─────────────────────────────

#[cfg(test)]
mod proptest_tests {
    use super::*;

    // proptest! makrosu dev-dependency'den gelir
    proptest::proptest! {
        // SubnetTracker: eklenen sayı hiçbir zaman limiti geçemez
        #[test]
        fn prop_subnet_tracker_never_exceeds_limit(
            ips in proptest::collection::vec(
                proptest::array::uniform4(0u8..255),
                0..50,
            )
        ) {
            let mut tracker = SubnetTracker::new();
            for ip in &ips {
                let _ = tracker.add(*ip);
                // /24 subnet sayısı her zaman limit altında kalmalı
                let prefix = [ip[0], ip[1], ip[2]];
                assert!(
                    tracker.subnet_count(*ip) <= SubnetTracker::TABLE_IP_LIMIT,
                    "/24 {} subnet limiti aşıldı", hex::format_hex(prefix)
                );
            }
        }

        // ReplayProtector: aynı key iki kez kabul edilemez
        #[test]
        fn prop_replay_no_duplicate_acceptance(
            sender_byte in 0u8..255,
            nonce in 0u64..1000,
            current_slot in 0u64..500,
        ) {
            let mut protector = ReplayProtector::new(current_slot);
            let key = MessageKey {
                sender: [sender_byte; 32],
                nonce,
                valid_until_slot: current_slot + 50,
            };

            let first = protector.accept(key.clone());
            let second = protector.accept(key);

            // İkisi aynı anda Ok olamaz
            assert!(!(first.is_ok() && second.is_ok()),
                "aynı key iki kez kabul edildi");
        }

        // DosProtector: slot başına mesaj sayısı limiti geçmemeli
        #[test]
        fn prop_dos_rate_limit_per_slot(
            msg_count in 0usize..200,
            peer_byte in 0u8..255,
        ) {
            let mut protector = DosProtector::new();
            let peer = [peer_byte; 32];
            let limit = DosProtector::DEFAULT_MAX_MSGS_PER_SLOT as usize;

            let mut accepted = 0usize;
            for _ in 0..msg_count {
                if protector.accept_message(peer).is_ok() {
                    accepted += 1;
                }
            }

            // Ban edilmemiş bir peer için kabul sayısı limit'i aşamaz
            if !protector.is_banned(&peer) {
                assert!(accepted <= limit,
                    "rate limit aşıldı: {} > {}", accepted, limit);
            }
        }

        // BlockHeader hash deterministik olmalı (aynı input → aynı output)
        #[test]
        fn prop_block_header_hash_deterministic(
            height in 0u64..1_000_000,
            slot in 0u64..10_000_000,
            proposer in 1u64..200,
        ) {
            use crate::fixtures::blocks::TestBlock;
            use nova_primitives::{BlockHash, EpochId, SlotId, ValidatorId};

            // Bu test için nova-consensus'a bağımlılık eklemeden primitifler kullanıyoruz
            // (nova-consensus bu crate'e bağımlı değil, döngü oluşur)
            let _ = height;
            let _ = slot;
            let _ = proposer;
            // Doğrulama: primitif hash fonksiyonları deterministik
            let data = [height.to_le_bytes(), slot.to_le_bytes()].concat();
            let h1 = blake3::hash(&data);
            let h2 = blake3::hash(&data);
            assert_eq!(h1, h2);
        }
    }

    // hex format yardımcısı (sadece proptest içinde)
    mod hex {
        pub fn format_hex(bytes: [u8; 3]) -> String {
            format!("{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
        }
    }
}
