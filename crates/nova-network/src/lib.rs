//! nova-network — Nova Shield Chain P2P ağ katmanı (§5.12)
//!
//! ## Mimari
//!
//! ### Transport (Polkadot pattern)
//! - TCP → Noise XX el sıkışma → Yamux çoklama
//! - `NoiseSession`: Ed25519 statik anahtar, ephemeral DH
//! - `TransportLayer`: dial/accept + şifreleme
//!
//! ### Peer Yönetimi (Polkadot + Avalanchego)
//! - `PeerStore`: itibat yönetimi (BANNED_THRESHOLD, INVERSE_DECREMENT)
//! - `HandshakeValidator`: protokol kimliği, genesis hash, versiyon uyumluluk
//! - `MessageType`: Gossip/Block/Tx/MpcShare/EonPublicKey (Rolling Shutter pattern)
//!
//! ### Gossip Protokolü (Solana CRDS pattern)
//! - `CrdsData`: ContactInfo, Vote, EpochSlots, MpcShare, BatchSlot
//! - `CrdsGossipPush`: FANOUT=9, Plumtree, ReceivedCache
//! - `CrdsGossipPull`: Bloom filtresi anti-entropy (FALSE_RATE=0.1)
//! - `GossipService`: kanal tabanlı olay döngüsü
//!
//! ### Turbine Blok Yayılımı (Solana)
//! - `Shred`: blok veri birimi, Merkle kanıtı
//! - `ShredBuilder`: blok → shred'ler (SHRED_DATA_PAYLOAD_SIZE=1232)
//! - `ClusterNodes`: stake ağırlıklı `WeightedShuffle`
//! - `ShredPropagator`: DATA_PLANE_FANOUT=200, MAX_NUM_TURBINE_HOPS=4
//!
//! ### Keşif (Polkadot Kademlia)
//! - `KademliaTable`: XOR mesafeli routing table, k-bucket=20
//! - `DiscoveryBehaviour`: bootstrap + random walk + DHT kayıt
//!
//! ### NetworkWorker/NetworkService (Polkadot)
//! - `NetworkWorker`: async yürütücü
//! - `NetworkService`: Arc<> ile paylaşılan kontrol handle

pub mod bridge;
pub mod config;
pub mod discovery;
pub mod errors;
pub mod fork_table;
pub mod gossip;
pub mod light_client;
pub mod message_queue;
pub mod metrics;
pub mod peer;
pub mod service;
pub mod transport;
pub mod turbine;
pub mod xcm;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use config::{BootstrapNode, GossipConfig, KademliaConfig, NetworkConfig, TurbineConfig};
pub use errors::{NetworkError, NetworkResult};
pub use gossip::{
    BloomFilter, CrdsData, CrdsFilter, CrdsTable, CrdsValue, GossipEvent, GossipOutput,
    GossipService, GossipStats, PUSH_FANOUT,
};
pub use peer::{
    HandshakeMessage, HandshakeResult, HandshakeValidator, MessageType, NetworkMessage, PeerId,
    PeerInfo, PeerRole, PeerState, PeerStore, ReputationChange, BANNED_THRESHOLD,
};
pub use bridge::{
    BridgeError, DenomTrace, EscrowAccount, FungibleTokenPacket, IbcChannel, IbcPacketSent,
    IbcTransferModule, VoucherMinter, IBC_PREFIX, IBC_TRANSFER_PORT, NOVA_NATIVE_DENOM,
};
pub use light_client::{
    BLS12381LightClient, BlsPubkey, BlsSignature, LightClientHeader, LightClientStore,
    LightClientUpdate, LightClientUpdateResult, SyncAggregate, SyncCommittee,
    make_sync_committee, make_valid_update,
    MIN_SYNC_COMMITTEE_PARTICIPANTS, SYNC_COMMITTEE_SIZE, SLOTS_PER_SYNC_COMMITTEE_PERIOD,
};
pub use fork_table::{
    ChainForkConfig, ForkActivation, ForkId, ForkTable, NovaFork,
    NOVA_DEVNET_CHAIN_ID, NOVA_MAINNET_CHAIN_ID, NOVA_TESTNET_CHAIN_ID,
};
pub use message_queue::{
    MessageQueueChain, MessageQueueChannel, MessageQueueRouter, MqcError, QueuedMessage,
    GENESIS_MQC_HEAD, MAX_MESSAGE_SIZE, MAX_QUEUE_DEPTH,
};
pub use metrics::{
    HandlerRegistry, HealthChecker, HealthReport, HealthStatus, MessageHandler, MessageMetrics,
    MessageValidator, P2pMetricsCollector, ShutdownContext, SubnetIpLimiter, ValidatorRegistry,
    BUCKET_IP_LIMIT, TABLE_IP_LIMIT,
};
pub use service::{NetworkEvent, NetworkService, NetworkStatus, NetworkWorker};
pub use transport::{ConnectionId, EstablishedConnection, FrameCodec, NoiseSession, TransportLayer};
pub use transport::tcp::{TcpPeer, TcpPeerReader, TcpPeerWriter, TcpTransport};
pub use xcm::{
    OriginKind, XcmAsset, XcmAssetId, XcmChannel, XcmError, XcmExecutor,
    XcmInstruction, XcmLocation, XcmMessage, XcmOutcome, XcmRouter,
    DEFAULT_XCM_WEIGHT, MAX_XCM_INSTRUCTIONS, MAX_XCM_MESSAGE_SIZE,
    NOVA_PARACHAIN_ID, RELAY_CHAIN_ID,
};
pub use turbine::{
    ClusterNodes, CodeShred, Deduper, ErasureBatchBuilder, ErasureConfig, ErasureDecoder,
    ErasureEncoder, ErasureError, FecSet, Shred, ShredBuilder, ShredPropagator, WeightedNode,
    CODE_SHREDS_PER_FEC, DATA_PLANE_FANOUT, DATA_SHREDS_PER_FEC, MAX_NUM_TURBINE_HOPS,
    MIN_SHREDS_FOR_RECOVERY, SHRED_DATA_PAYLOAD_SIZE,
};

// ─── Integration Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gossip::crds::{CrdsData, CrdsValue};
    use crate::turbine::shred::ShredType;
    use crate::peer::handshake::HandshakeMessage;

    fn pid(n: u8) -> PeerId {
        PeerId::from_bytes([n; 32])
    }

    fn contact(n: u8) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: [n; 32],
            addr: format!("127.0.0.{n}:30333"),
            wallclock: 2000 + n as u64,
        })
    }

    // ─── 1: Transport Noise el sıkışma ─────────────────────────────────────

    #[test]
    fn integration_noise_handshake_and_encrypt() {
        let transport = TransportLayer::new([1u8; 32]);
        let mut conn = transport.dial("127.0.0.1:30333", [2u8; 32]).unwrap();

        let msg = b"Nova Shield: MEV matematiksel imkansiz";
        let ct = conn.send_encrypted(msg).unwrap();
        let pt = conn.receive_encrypted(&ct).unwrap();
        assert_eq!(&pt, msg);
    }

    // ─── 2: Peer Store itibar yönetimi ─────────────────────────────────────

    #[test]
    fn integration_peer_store_reputation() {
        let store = PeerStore::new();
        for i in 1u8..=10 {
            store.add_peer(pid(i), format!("127.0.0.{i}:30333"), PeerRole::Full);
        }

        // İyi mesajlar
        for _ in 0..5 {
            store.report_peer(&pid(1), &crate::peer::store::GOOD_MESSAGE);
        }
        // Kötü mesaj
        store.report_peer(&pid(2), &ReputationChange::new(i32::MIN, "kötü"));

        assert!(store.reputation(&pid(1)) > 0);
        assert!(store.is_banned(&pid(2)));

        let candidates = store.outgoing_candidates(5, &Default::default());
        assert!(!candidates.contains(&pid(2)));
    }

    // ─── 3: El sıkışma protokolü ───────────────────────────────────────────

    #[test]
    fn integration_handshake_validation() {
        let genesis = [0xAB; 32];
        let validator = HandshakeValidator::new("/nova-shield/1.0", genesis);

        // Gerçek BLS imzası ile validator handshake
        let kp = nova_crypto::BlsKeypair::from_ikm(&[0x55u8; 32]).unwrap();
        let msg = HandshakeMessage::new(
            [5u8; 32],
            "10.0.0.5:30333",
            "nova-shield/0.1.0",
            "/nova-shield/1.0",
            genesis,
            PeerRole::Validator,
        ).with_bls_validator_sig(&kp);

        let result = validator.validate(&msg).unwrap();
        assert!(result.is_validator);
        assert_eq!(result.peer_addr, "10.0.0.5:30333");
    }

    // ─── 4: CRDS Gossip push döngüsü ───────────────────────────────────────

    #[test]
    fn integration_gossip_push_cycle() {
        use crate::gossip::push::{CrdsGossipPush, PUSH_FANOUT};

        let mut push = CrdsGossipPush::new([0u8; 32], PUSH_FANOUT);
        for i in 1u8..=PUSH_FANOUT as u8 + 2 {
            push.add_peer([i; 32], 100);
        }

        push.push_value(contact(1));
        push.push_value(contact(2));

        let messages = push.new_push_messages(1000);
        assert!(messages.len() <= PUSH_FANOUT);
        for (_, msg) in &messages {
            assert_eq!(msg.values.len(), 2);
        }
    }

    // ─── 5: CRDS Gossip pull anti-entropy ──────────────────────────────────

    #[test]
    fn integration_gossip_pull_anti_entropy() {
        use crate::gossip::pull::CrdsGossipPull;

        let mut server_crds = CrdsTable::new();
        for i in 1u8..=10 {
            server_crds.insert(contact(i));
        }

        let mut client_pull = CrdsGossipPull::new([0u8; 32]);
        let mut client_crds = CrdsTable::new();
        // İstemci sadece 1-5'e sahip
        for i in 1u8..=5 {
            client_crds.insert(contact(i));
        }

        let req = client_pull.build_pull_request(&client_crds, 1000);
        let server_pull = CrdsGossipPull::new([1u8; 32]);
        let resp = server_pull.process_pull_request(&server_crds, &req, 1000);

        let added = client_pull.process_pull_response(&mut client_crds, &resp);
        // 6-10 eklendi (false positive nedeniyle bazıları atlanabilir)
        assert!(added > 0);
        assert!(client_crds.len() >= 5);
    }

    // ─── 6: Bloom filtresi ─────────────────────────────────────────────────

    #[test]
    fn integration_bloom_filter() {
        let mut bf = BloomFilter::new(1000);
        let hashes: Vec<[u8; 32]> = (0u8..100).map(|i| {
            let mut h = blake3::Hasher::new();
            h.update(&[i]);
            *h.finalize().as_bytes()
        }).collect();

        for h in &hashes {
            bf.add(h);
        }
        // False negative yok
        for h in &hashes {
            assert!(bf.contains(h));
        }
    }

    // ─── 7: Shred bölme ve yeniden yapılandırma ────────────────────────────

    #[test]
    fn integration_shred_split_and_reconstruct() {
        let builder = ShredBuilder::new([1u8; 32], 256);
        let block_data: Vec<u8> = (0u8..=255).cycle().take(1024).collect();

        let shreds = builder.shred_block(500, &block_data);
        assert!(!shreds.is_empty());

        // Tüm shred'ler geçerli
        for s in &shreds {
            assert!(s.verify());
        }

        // Yeniden yapılandır
        let reconstructed = builder.deshred(&shreds).unwrap();
        assert_eq!(&reconstructed[..block_data.len()], &block_data);
    }

    // ─── 8: Turbine stake ağırlıklı dağıtım ────────────────────────────────

    #[test]
    fn integration_turbine_weighted_retransmit() {
        let nodes: Vec<WeightedNode> = (1u8..=20)
            .map(|i| WeightedNode::new([i; 32], format!("127.0.0.{i}:30333"), i as u64 * 1_000_000))
            .collect();
        let cluster = ClusterNodes::new(nodes);

        // En yüksek stake'li top-5
        let top5 = cluster.top_validators(5);
        assert_eq!(top5.len(), 5);
        // İlk validator en yüksek stake'li olmalı
        assert!(top5[0].stake >= top5[1].stake);
    }

    // ─── 9: Kademlia keşif ─────────────────────────────────────────────────

    #[test]
    fn integration_kademlia_discovery() {
        use crate::discovery::DiscoveryBehaviour;

        let mut discovery = DiscoveryBehaviour::new(pid(0), KademliaConfig::default());

        // 20 peer ekle
        for i in 1u8..=20 {
            discovery.add_known_peer(pid(i), format!("127.0.0.{i}:30333"), 1000 * i as u64);
        }

        // En yakın 10'u bul
        let qid = discovery.find_node(pid(10));
        assert!(qid > 0);

        let events = discovery.poll_events();
        // QueryComplete geldi mi?
        assert!(events.iter().any(|e| matches!(e, crate::discovery::DiscoveryEvent::QueryComplete { .. })));
    }

    // ─── 10: NetworkWorker + NetworkService ────────────────────────────────

    #[test]
    fn integration_network_worker_service() {
        let (mut worker, service) = NetworkWorker::new(NetworkConfig::default(), [0u8; 32]);

        // 5 peer bağla
        for i in 1u8..=5 {
            service.connect_peer(pid(i), format!("127.0.0.{i}:30333")).unwrap();
        }

        // Gossip yayınla
        service.publish_gossip(contact(99)).unwrap();

        // DHT kayıt
        service.dht_put(b"validator-1-addr".to_vec(), b"10.0.0.1:30333".to_vec()).unwrap();

        worker.process_messages();

        assert_eq!(service.num_connected_peers(), 5);
        let status = worker.status();
        assert_eq!(status.known_peers, 5);
        assert_eq!(status.crds_entries, 1);
    }

    // ─── 11: Shred yayılım pipeline ────────────────────────────────────────

    #[test]
    fn integration_shred_propagation_pipeline() {
        let nodes: Vec<WeightedNode> = (0u8..=9)
            .map(|i| WeightedNode::new([i; 32], format!("127.0.0.{i}:30333"), (i as u64 + 1) * 500_000))
            .collect();
        let cluster = ClusterNodes::new(nodes);

        let mut propagator = ShredPropagator::new(
            [0u8; 32],
            cluster,
            MAX_NUM_TURBINE_HOPS as u8,
            DATA_PLANE_FANOUT,
        );

        // Birkaç shred işle
        let builder = ShredBuilder::new([0u8; 32], SHRED_DATA_PAYLOAD_SIZE);
        let shreds = builder.shred_block(1000, &vec![0xBBu8; 2 * SHRED_DATA_PAYLOAD_SIZE]);

        for shred in shreds {
            let result = propagator.retransmit(shred);
            assert_eq!(result.shred_id.slot, 1000);
        }

        let total = propagator.total_processed();
        assert!(total >= 2);
    }

    // ─── 12: MPC mesaj tipleri (Rolling Shutter pattern) ───────────────────

    #[test]
    fn integration_mpc_message_types() {
        let share_msg = NetworkMessage::new(
            MessageType::MpcDecryptionShare,
            [1u8; 32],
            b"decryption_share_data".to_vec(),
        );
        let eon_msg = NetworkMessage::new(
            MessageType::EonPublicKey,
            [2u8; 32],
            b"eon_public_key".to_vec(),
        );
        let commit_msg = NetworkMessage::new(
            MessageType::MpcCommitment,
            [3u8; 32],
            b"commitment".to_vec(),
        );

        // Encode-decode round-trip
        for msg in &[share_msg, eon_msg, commit_msg] {
            let encoded = msg.encode();
            let decoded = NetworkMessage::decode(&encoded).unwrap();
            assert_eq!(decoded.msg_type, msg.msg_type);
            assert_eq!(decoded.payload, msg.payload);
        }
    }

    // ─── 13: GossipService tam döngü ───────────────────────────────────────

    #[test]
    fn integration_gossip_service_full_cycle() {
        let mut svc = GossipService::new([0u8; 32]);
        let tx = svc.event_sender();

        // 5 peer bağla
        for i in 1u8..=5 {
            tx.send(GossipEvent::PeerConnected([i; 32])).unwrap();
        }

        // 10 değer yayınla
        for i in 1u8..=10 {
            tx.send(GossipEvent::Publish(contact(i))).unwrap();
        }

        // Tick
        tx.send(GossipEvent::Tick { now_ms: 5000 }).unwrap();

        let outputs = svc.process_events();
        assert_eq!(svc.crds().len(), 10);
        assert_eq!(svc.active_peer_count(), 5);

        // Push mesajları gönderildi mi?
        let push_count = outputs.iter()
            .filter(|o| matches!(o, GossipOutput::SendPush { .. }))
            .count();
        // FANOUT=9 ama peer=5, en fazla 5 push
        assert!(push_count <= 5);
    }
}
