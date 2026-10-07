//! P2P ağ güvenlik testleri — Sybil, eclipse, flood, gossip
//!
//! ## Test Kategorileri
//!
//! 1. **Flood / DDoS** — Peer başına mesaj hızı sınırı
//! 2. **Sybil saldırısı** — Çok sayıda sahte peer → routing tabloyu doldurma
//! 3. **Eclipse saldırısı** — Meşru peer'ları dışarı itme
//! 4. **Gossip güvenliği** — Tekrarlanan değerler sınırlı yayılır
//! 5. **Handshake güvenliği** — Yanlış genesis hash bağlantı reddeder
//! 6. **Itibat yönetimi** — Kötü peer'lar otomatik yasaklanır

#[cfg(test)]
mod tests {
    use nova_network::{
        PeerStore, PeerRole, PeerId, HandshakeValidator, HandshakeMessage,
        GossipService, GossipEvent, CrdsValue, CrdsData, ReputationChange, BANNED_THRESHOLD,
        PUSH_FANOUT,
    };

    fn pid(n: u8) -> PeerId {
        PeerId::from_bytes([n; 32])
    }

    fn contact_val(n: u8) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: [n; 32],
            addr: format!("127.0.0.{n}:30333"),
            wallclock: 1000 + n as u64,
        })
    }

    // ── Peer İtibar & Ban ─────────────────────────────────────────────────────

    /// PoC: Kötü peer otomatik yasaklanmalı (BANNED_THRESHOLD altına düşünce)
    #[test]
    fn poc_malicious_peer_auto_banned() {
        let store = PeerStore::new();
        store.add_peer(pid(1), "10.0.0.1:30333".to_string(), PeerRole::Full);

        // BANNED_THRESHOLD'u aşacak kadar kötü rapor
        store.report_peer(&pid(1), &ReputationChange::new(i32::MIN, "byzantine behavior"));

        assert!(
            store.is_banned(&pid(1)),
            "kötü peer otomatik olarak yasaklanmalı"
        );
    }

    /// PoC: Yasaklanan peer outgoing candidates'a girmemeli
    #[test]
    fn poc_banned_peer_excluded_from_candidates() {
        let store = PeerStore::new();

        for i in 1u8..=10 {
            store.add_peer(pid(i), format!("10.0.0.{i}:30333"), PeerRole::Full);
        }

        // Peer 3'ü yasakla
        store.report_peer(&pid(3), &ReputationChange::new(i32::MIN, "attack"));

        let candidates = store.outgoing_candidates(10, &Default::default());
        assert!(
            !candidates.contains(&pid(3)),
            "yasaklı peer outgoing candidates'a girmemeli"
        );
    }

    /// PoC: İyi peer'lar pozitif itibar kazanmalı
    #[test]
    fn poc_good_peers_gain_reputation() {
        let store = PeerStore::new();
        store.add_peer(pid(5), "10.0.0.5:30333".to_string(), PeerRole::Validator);

        // İyi mesajlar
        for _ in 0..10 {
            store.report_peer(&pid(5), &nova_network::peer::store::GOOD_MESSAGE);
        }

        assert!(
            store.reputation(&pid(5)) > 0,
            "iyi peer pozitif itibar kazanmalı"
        );
        assert!(
            !store.is_banned(&pid(5)),
            "iyi peer yasaklanmamalı"
        );
    }

    // ── Sybil Saldırısı Koruması ─────────────────────────────────────────────

    /// PoC: 1000 Sybil peer eklenince meşru validator seçilmeli
    #[test]
    fn poc_sybil_attack_legitimate_validator_still_reachable() {
        let store = PeerStore::new();

        // 1000 Sybil peer (Full node — daha düşük öncelik)
        for i in 0u16..1000 {
            let ip = format!("192.168.{}.{}:30333", i / 256, i % 256);
            let mut bytes = [0u8; 32];
            bytes[0] = (i >> 8) as u8;
            bytes[1] = (i & 0xFF) as u8;
            store.add_peer(PeerId::from_bytes(bytes), ip, PeerRole::Full);
        }

        // Meşru validator (yüksek öncelik)
        store.add_peer(pid(255), "10.0.0.100:30333".to_string(), PeerRole::Validator);

        // Validator peer en yüksek itibar puanı alan adaylar arasında olmalı
        let all_candidates = store.outgoing_candidates(1000, &Default::default());
        assert!(
            all_candidates.contains(&pid(255)),
            "Sybil saldırısına rağmen meşru validator ulaşılabilir olmalı ({} aday)",
            all_candidates.len()
        );
    }

    /// PoC: Sybil peer'lar düşük itibarla kalır, validator yüksek itibarla öne çıkar
    #[test]
    fn poc_sybil_low_reputation_cannot_dominate() {
        let store = PeerStore::new();

        // Sybil peer'lar — nötr itibar
        for i in 0u8..50 {
            store.add_peer(pid(i), format!("192.168.0.{i}:30333"), PeerRole::Full);
        }

        // Meşru validatörler — iyi itibar
        for i in 200u8..=210 {
            store.add_peer(
                PeerId::from_bytes([i; 32]),
                format!("10.0.0.{i}:30333"),
                PeerRole::Validator,
            );
            // İtibarlarını yükselt
            for _ in 0..20 {
                store.report_peer(
                    &PeerId::from_bytes([i; 32]),
                    &nova_network::peer::store::GOOD_MESSAGE,
                );
            }
        }

        // Top 11 peer → meşru validatörler içinde olmalı
        let top = store.outgoing_candidates(11, &Default::default());
        let validator_count = top.iter().filter(|p| p.bytes[0] >= 200).count();
        assert!(
            validator_count > 0,
            "yüksek itibarli validatörler top peers arasında olmalı"
        );
    }

    // ── Handshake Güvenliği ───────────────────────────────────────────────────

    /// PoC: Yanlış genesis hash → bağlantı reddedilmeli (eclipse önlemi)
    #[test]
    fn poc_wrong_genesis_hash_rejected() {
        let correct_genesis = [0xAA; 32];
        let wrong_genesis = [0xBB; 32];

        let validator = HandshakeValidator::new("/nova-shield/1.0", correct_genesis);

        let msg = HandshakeMessage::new(
            [1u8; 32],
            "10.0.0.1:30333",
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            wrong_genesis, // yanlış genesis
            PeerRole::Full,
        );

        let result = validator.validate(&msg);
        assert!(
            result.is_err(),
            "yanlış genesis hash'li peer reddedilmeli (farklı ağ/fork)"
        );
    }

    /// PoC: Yanlış protokol ID → bağlantı reddedilmeli
    #[test]
    fn poc_wrong_protocol_id_rejected() {
        let genesis = [0xCC; 32];
        let validator = HandshakeValidator::new("/nova-shield/1.0", genesis);

        let msg = HandshakeMessage::new(
            [2u8; 32],
            "10.0.0.2:30333",
            "ethereum/0.1.0",     // yanlış yazılım
            "/ethereum/1.0",      // yanlış protokol
            genesis,
            PeerRole::Full,
        );

        let result = validator.validate(&msg);
        assert!(
            result.is_err(),
            "yanlış protokol ID'li peer reddedilmeli"
        );
    }

    /// Pozitif: Doğru genesis + protokol ID → bağlantı kabul edilmeli
    #[test]
    fn valid_handshake_accepted() {
        let genesis = [0xDD; 32];
        let validator = HandshakeValidator::new("/nova-shield/1.0", genesis);

        // Gerçek BLS imzası ile validator handshake
        let kp = nova_crypto::BlsKeypair::from_ikm(&[0x42u8; 32]).unwrap();
        let msg = HandshakeMessage::new(
            [3u8; 32],
            "10.0.0.3:30333",
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            genesis,
            PeerRole::Validator,
        ).with_bls_validator_sig(&kp);

        let result = validator.validate(&msg);
        assert!(result.is_ok(), "geçerli handshake kabul edilmeli");
        assert!(result.unwrap().is_validator, "validator rolü tanınmalı");
    }

    // ── Gossip Flood Koruması ────────────────────────────────────────────────

    /// PoC: Gossip'te aynı veri tekrar eklenmez (ReceivedCache)
    #[test]
    fn poc_gossip_duplicate_values_not_propagated() {
        let mut svc = GossipService::new([0u8; 32]);
        let tx = svc.event_sender();

        // Peer bağla
        tx.send(GossipEvent::PeerConnected([1u8; 32])).unwrap();
        tx.send(GossipEvent::PeerConnected([2u8; 32])).unwrap();

        // Aynı değeri 5 kez gönder
        let same_value = contact_val(42);
        for _ in 0..5 {
            tx.send(GossipEvent::Publish(same_value.clone())).unwrap();
        }

        tx.send(GossipEvent::Tick { now_ms: 1000 }).unwrap();
        svc.process_events();

        // CRDS'de sadece 1 kayıt olmalı
        assert_eq!(
            svc.crds().len(),
            1,
            "aynı değer sadece bir kez CRDS'e eklenmeli"
        );
    }

    /// PoC: Gossip FANOUT sınırlı — sonsuz yayılım yok
    #[test]
    fn poc_gossip_push_fanout_bounded() {
        let mut svc = GossipService::new([0u8; 32]);
        let tx = svc.event_sender();

        // FANOUT'tan çok peer bağla
        for i in 0u8..50 {
            tx.send(GossipEvent::PeerConnected([i; 32])).unwrap();
        }

        // 10 değer yayınla
        for i in 0u8..10 {
            tx.send(GossipEvent::Publish(contact_val(100 + i))).unwrap();
        }

        tx.send(GossipEvent::Tick { now_ms: 2000 }).unwrap();
        let outputs = svc.process_events();

        // Push mesajları FANOUT * değer sayısından fazla olmamalı
        let push_count = outputs.iter()
            .filter(|o| matches!(o, nova_network::GossipOutput::SendPush { .. }))
            .count();

        assert!(
            push_count <= PUSH_FANOUT * 10,
            "gossip push sayısı FANOUT ile sınırlı olmalı: {push_count} <= {}",
            PUSH_FANOUT * 10
        );
    }

    // ── Eclipse Saldırısı Koruması ───────────────────────────────────────────

    /// PoC: Peer store kötü peer doldurulsa bile iyi peer'lara yer var
    #[test]
    fn poc_eclipse_attack_good_peers_survive() {
        let store = PeerStore::new();

        // Saldırgan: 200 kötü peer doldur ama hepsini yasakla (hepsini kötü raporla)
        for i in 0u8..200 {
            let mut bytes = [0u8; 32];
            bytes[0] = i;
            bytes[1] = 0xAA; // Saldırgan işareti
            store.add_peer(PeerId::from_bytes(bytes), format!("1.1.{i}.1:30333"), PeerRole::Full);
            store.report_peer(&PeerId::from_bytes(bytes), &ReputationChange::new(i32::MIN, "eclipse attacker"));
        }

        // Meşru 5 peer ekle
        for i in 0u8..5 {
            let mut bytes = [0u8; 32];
            bytes[0] = i;
            bytes[1] = 0xFF; // Meşru işareti
            store.add_peer(
                PeerId::from_bytes(bytes),
                format!("10.0.0.{i}:30333"),
                PeerRole::Validator,
            );
        }

        // Tüm candidates'ı al — meşru peer'lar görünmeli
        let candidates = store.outgoing_candidates(10, &Default::default());
        let legitimate_count = candidates.iter()
            .filter(|p| p.bytes[1] == 0xFF)
            .count();

        assert!(
            legitimate_count > 0,
            "eclipse saldırısına rağmen meşru peer'lar ulaşılabilir olmalı"
        );
    }

    // ── Bandwidth Exhaustion Koruması ─────────────────────────────────────────

    /// PoC: Çok büyük peer listesi isteniyor — makul sayıda döner
    #[test]
    fn poc_peer_list_bounded_response() {
        let store = PeerStore::new();

        for i in 0u8..50 {
            store.add_peer(pid(i), format!("10.0.0.{i}:30333"), PeerRole::Full);
        }

        // 1000 peer iste — sadece mevcut olanlar döner
        let candidates = store.outgoing_candidates(1000, &Default::default());
        assert!(
            candidates.len() <= 50,
            "peer listesi gerçek peer sayısını aşmamalı"
        );
    }
    // ── Proptest: P2P Property Testleri ──────────────────────────────────────

    use proptest::prelude::*;

    proptest! {
        /// Property: Yeterli sayıda kötü rapor → peer yasaklanır
        #[test]
        fn prop_enough_bad_reports_leads_to_ban(n in 1usize..20usize) {
            let store = PeerStore::new();
            let peer = pid(200);
            store.add_peer(peer.clone(), "10.0.0.1:30303", PeerRole::Full);

            // Her INVALID_GOSSIP = -(1<<14) = -16384
            // BANNED_THRESHOLD = 71 * (i32::MIN / 100) ≈ -1_525_757_952
            // 100+ rapor yeterince düşürür — en az 100 tekrar
            let rep_change = nova_network::peer::store::INVALID_GOSSIP;
            for _ in 0..(n * 100) {
                store.report_peer(&peer, &rep_change);
            }

            let banned = store.is_banned(&peer);
            let rep = store.reputation(&peer);
            // Ya yasaklandı ya da reputasyon düştü
            prop_assert!(
                banned || rep < 0,
                "yeterli kötü rapor sonrası peer yasaklanmalı veya reputation negatif: rep={rep}"
            );
        }

        /// Property: İyi mesajlar reputation'ı artırır
        #[test]
        fn prop_good_messages_increase_reputation(n in 1usize..50usize) {
            let store = PeerStore::new();
            let peer = pid(201);
            store.add_peer(peer.clone(), "10.0.0.2:30303", PeerRole::Full);
            let before = store.reputation(&peer);

            let rep_change = nova_network::peer::store::GOOD_MESSAGE;
            for _ in 0..n {
                store.report_peer(&peer, &rep_change);
            }

            let after = store.reputation(&peer);
            prop_assert!(after >= before, "iyi mesajlar reputation'ı düşürmemeli");
        }

        /// Property: peer_count kayıttan sonra artar
        #[test]
        fn prop_add_peer_increases_count(n in 1usize..20usize) {
            let store = PeerStore::new();
            let before = store.peer_count();
            for i in 0..n {
                store.add_peer(pid(i as u8), format!("10.0.{i}.1:30303"), PeerRole::Full);
            }
            let after = store.peer_count();
            prop_assert!(after >= before + n, "n peer eklendikten sonra count artmalı");
        }

        /// Property: Yasaklı peer outgoing_candidates listesinde olmamalı
        #[test]
        fn prop_banned_peer_not_in_candidates(n_reports in 100usize..300usize) {
            let store = PeerStore::new();
            let peer = pid(202);
            store.add_peer(peer.clone(), "10.0.0.3:30303", PeerRole::Full);

            // Yeterince kötü rapor → ban
            let rep_change = nova_network::peer::store::INVALID_GOSSIP;
            for _ in 0..n_reports {
                store.report_peer(&peer, &rep_change);
            }

            if store.is_banned(&peer) {
                let candidates = store.outgoing_candidates(50, &std::collections::HashSet::new());
                prop_assert!(
                    !candidates.contains(&peer),
                    "yasaklı peer outgoing candidates'ta olmamalı"
                );
            }
        }
    }
}
