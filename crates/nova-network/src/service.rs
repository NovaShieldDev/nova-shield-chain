//! NetworkWorker + NetworkService — Polkadot pattern
//!
//! Polkadot `service.rs`:
//! - `NetworkWorker` — ağı yürüten async future (`run()` çağrısı ile)
//! - `NetworkService` — paylaşılan handle, `Arc<NetworkService>` olarak kullanılır
//! - `ServiceToWorkerMsg` kanalı — worker'a mesaj gönderir
//! - `NetworkWorker::service()` → `Arc<NetworkService>` döner
//!
//! Avalanchego: `Network.Dispatch()` + `Network.ManuallyTrack(nodeID, ip)`

use crate::config::NetworkConfig;
use crate::discovery::DiscoveryBehaviour;
use crate::errors::{NetworkError, NetworkResult};
use crate::gossip::service::{GossipEvent, GossipOutput, GossipService};
use crate::gossip::crds::CrdsValue;
use crate::peer::info::{PeerId, PeerRole, PeerState};
use crate::peer::store::PeerStore;
use crate::turbine::shred::Shred;
use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

// ─── ServiceToWorkerMsg ───────────────────────────────────────────────────────

/// Service → Worker mesajları
#[derive(Debug)]
pub enum ServiceToWorkerMsg {
    /// Peer'a bağlan
    ConnectPeer { peer_id: PeerId, addr: String },
    /// Peer bağlantısını kes
    DisconnectPeer { peer_id: PeerId },
    /// CRDS değeri yayınla (gossip)
    PublishGossip(CrdsValue),
    /// Shred yayılımı
    RetransmitShred(Shred),
    /// DHT kayıt koy
    DhtPut { key: Vec<u8>, value: Vec<u8> },
    /// DHT kayıt al isteği
    DhtGet { key: Vec<u8> },
    /// Node durumu al
    GetStatus,
}

// ─── NetworkStatus ────────────────────────────────────────────────────────────

/// Network durumu — Polkadot `NetworkStatus`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub connected_peers: usize,
    pub known_peers: usize,
    pub banned_peers: usize,
    pub crds_entries: usize,
    pub shreds_processed: u64,
    pub gossip_stats: crate::gossip::GossipStats,
}

// ─── NetworkEvent ─────────────────────────────────────────────────────────────

/// Network olayı (dışarıya bildirim)
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// Yeni peer bağlandı
    PeerConnected(PeerId),
    /// Peer ayrıldı
    PeerDisconnected(PeerId),
    /// Yeni gossip değeri öğrenildi
    GossipValue(CrdsValue),
    /// Shred alındı
    ShredReceived(Shred),
    /// DHT kaydı geldi
    DhtRecord { key: Vec<u8>, value: Vec<u8> },
}

// ─── NetworkService ───────────────────────────────────────────────────────────

/// Paylaşılan network servis handle'ı
/// Polkadot: `Arc<NetworkService<B, H>>`
pub struct NetworkService {
    /// Bağlı peer sayısı
    num_connected: Arc<AtomicUsize>,
    /// Worker'a mesaj gönder
    to_worker: Sender<ServiceToWorkerMsg>,
    /// Olay alıcı (aboneler için)
    event_rx: Receiver<NetworkEvent>,
    local_peer_id: PeerId,
}

impl NetworkService {
    /// Peer'a bağlan — Polkadot: `NetworkPeers::connect_peer()`
    pub fn connect_peer(&self, peer_id: PeerId, addr: String) -> NetworkResult<()> {
        self.to_worker.send(ServiceToWorkerMsg::ConnectPeer { peer_id, addr })
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// Peer bağlantısını kes
    pub fn disconnect_peer(&self, peer_id: PeerId) -> NetworkResult<()> {
        self.to_worker.send(ServiceToWorkerMsg::DisconnectPeer { peer_id })
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// CRDS değeri yayınla
    pub fn publish_gossip(&self, value: CrdsValue) -> NetworkResult<()> {
        self.to_worker.send(ServiceToWorkerMsg::PublishGossip(value))
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// Shred retransmit
    pub fn retransmit_shred(&self, shred: Shred) -> NetworkResult<()> {
        self.to_worker.send(ServiceToWorkerMsg::RetransmitShred(shred))
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// DHT kayıt koy
    pub fn dht_put(&self, key: Vec<u8>, value: Vec<u8>) -> NetworkResult<()> {
        self.to_worker.send(ServiceToWorkerMsg::DhtPut { key, value })
            .map_err(|_| NetworkError::ChannelClosed)
    }

    /// Bağlı peer sayısı
    pub fn num_connected_peers(&self) -> usize {
        self.num_connected.load(Ordering::Relaxed)
    }

    /// Yerel peer ID
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Olay al (non-blocking)
    pub fn try_recv_event(&self) -> Option<NetworkEvent> {
        self.event_rx.try_recv().ok()
    }
}

// ─── NetworkWorker ────────────────────────────────────────────────────────────

/// Ana network yürütücüsü
/// Polkadot: `NetworkWorker` — `run()` future ile çalışır
pub struct NetworkWorker {
    config: NetworkConfig,
    peer_store: PeerStore,
    gossip: GossipService,
    discovery: DiscoveryBehaviour,
    local_peer_id: PeerId,
    num_connected: Arc<AtomicUsize>,
    /// Service'ten gelen mesajlar
    from_service: Receiver<ServiceToWorkerMsg>,
    /// Olayları service'e ilet
    event_tx: Sender<NetworkEvent>,
    /// Shred sayacı
    shreds_processed: u64,
}

impl NetworkWorker {
    /// Yeni NetworkWorker ve NetworkService çifti oluştur
    pub fn new(config: NetworkConfig, local_key: [u8; 32]) -> (Self, Arc<NetworkService>) {
        let local_peer_id = PeerId::from_bytes(local_key);
        let num_connected = Arc::new(AtomicUsize::new(0));

        let (to_worker, from_service) = crossbeam_channel::unbounded();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();

        let service = Arc::new(NetworkService {
            num_connected: num_connected.clone(),
            to_worker,
            event_rx,
            local_peer_id: local_peer_id.clone(),
        });

        let worker = Self {
            config: config.clone(),
            peer_store: PeerStore::new(),
            gossip: GossipService::new(local_key),
            discovery: DiscoveryBehaviour::new(local_peer_id.clone(), config.kademlia.clone()),
            local_peer_id,
            num_connected,
            from_service,
            event_tx,
            shreds_processed: 0,
        };

        (worker, service)
    }

    /// Tüm bekleyen mesajları işle — Polkadot: `NetworkWorker::next_action()`
    pub fn process_messages(&mut self) {
        // Service mesajlarını işle
        while let Ok(msg) = self.from_service.try_recv() {
            self.handle_service_msg(msg);
        }

        // Gossip döngüsü
        let gossip_out = self.gossip.process_events();
        for out in gossip_out {
            self.handle_gossip_output(out);
        }

        // Discovery olayları
        let disc_events = self.discovery.poll_events();
        for event in disc_events {
            use crate::discovery::DiscoveryEvent;
            match event {
                DiscoveryEvent::Discovered { peer_id, addr } => {
                    self.peer_store.add_peer(peer_id.clone(), &addr, PeerRole::Unknown);
                }
                DiscoveryEvent::QueryComplete { query_id: _, found } => {
                    for (peer, addr) in found {
                        self.peer_store.add_peer(peer, addr, PeerRole::Unknown);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_service_msg(&mut self, msg: ServiceToWorkerMsg) {
        match msg {
            ServiceToWorkerMsg::ConnectPeer { peer_id, addr } => {
                // Yasaklı peer ile bağlantı kurma — banlama mekanizması uygulanmalı
                if self.peer_store.is_banned(&peer_id) {
                    // Yasaklı peer reddedildi — log kaydı yapılabilir ancak tracing bağımlılığı yok
                } else {
                    self.peer_store.add_peer(peer_id.clone(), &addr, PeerRole::Unknown);
                    self.peer_store.set_state(&peer_id, PeerState::Connected);
                    self.num_connected.fetch_add(1, Ordering::Relaxed);
                    let gossip_tx = self.gossip.event_sender();
                    let _ = gossip_tx.send(GossipEvent::PeerConnected(peer_id.bytes));
                    let _ = self.event_tx.send(NetworkEvent::PeerConnected(peer_id));
                }
            }

            ServiceToWorkerMsg::DisconnectPeer { peer_id } => {
                self.peer_store.report_disconnect(&peer_id);
                self.num_connected.fetch_sub(1, Ordering::Relaxed);
                let _ = self.event_tx.send(NetworkEvent::PeerDisconnected(peer_id));
            }

            ServiceToWorkerMsg::PublishGossip(value) => {
                let tx = self.gossip.event_sender();
                let _ = tx.send(GossipEvent::Publish(value));
            }

            ServiceToWorkerMsg::RetransmitShred(shred) => {
                self.shreds_processed += 1;
                let _ = self.event_tx.send(NetworkEvent::ShredReceived(shred));
            }

            ServiceToWorkerMsg::DhtPut { key, value } => {
                self.discovery.put_value(key.clone(), value.clone());
                let _ = self.event_tx.send(NetworkEvent::DhtRecord { key, value });
            }

            ServiceToWorkerMsg::DhtGet { key } => {
                if let Some(value) = self.discovery.get_value(&key) {
                    let _ = self.event_tx.send(NetworkEvent::DhtRecord {
                        key,
                        value: value.clone(),
                    });
                }
            }

            ServiceToWorkerMsg::GetStatus => {
                // Status sorgusu — gerçekte oneshot channel ile dönerdi
                // Mock: yok sayılır
            }
        }
    }

    fn handle_gossip_output(&self, out: GossipOutput) {
        match out {
            GossipOutput::NewValue(value) => {
                let _ = self.event_tx.send(NetworkEvent::GossipValue(value));
            }
            // Push/Pull mesajları gerçek ağda peer'lara gönderilir
            _ => {}
        }
    }

    /// Network durumu al
    pub fn status(&self) -> NetworkStatus {
        NetworkStatus {
            connected_peers: self.num_connected.load(Ordering::Relaxed),
            known_peers: self.peer_store.peer_count(),
            banned_peers: self.peer_store.banned_peers().len(),
            crds_entries: self.gossip.crds().len(),
            shreds_processed: self.shreds_processed,
            gossip_stats: self.gossip.stats().clone(),
        }
    }

    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    pub fn peer_store(&self) -> &PeerStore {
        &self.peer_store
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gossip::crds::{CrdsData, CrdsValue};
    use crate::turbine::shred::{Shred, ShredType};

    fn local_key() -> [u8; 32] {
        [0u8; 32]
    }

    fn make_network() -> (NetworkWorker, Arc<NetworkService>) {
        NetworkWorker::new(NetworkConfig::default(), local_key())
    }

    fn pid(n: u8) -> PeerId {
        PeerId::from_bytes([n; 32])
    }

    fn contact(n: u8) -> CrdsValue {
        CrdsValue::new(CrdsData::ContactInfo {
            node_id: [n; 32],
            addr: format!("127.0.0.{n}:30333"),
            wallclock: 1000,
        })
    }

    #[test]
    fn test_network_worker_creation() {
        let (worker, service) = make_network();
        assert_eq!(worker.local_peer_id(), service.local_peer_id());
    }

    #[test]
    fn test_connect_peer() {
        let (mut worker, service) = make_network();
        service.connect_peer(pid(1), "127.0.0.1:30333".into()).unwrap();
        worker.process_messages();
        assert_eq!(service.num_connected_peers(), 1);
    }

    #[test]
    fn test_disconnect_peer() {
        let (mut worker, service) = make_network();
        service.connect_peer(pid(1), "127.0.0.1:30333".into()).unwrap();
        worker.process_messages();
        assert_eq!(service.num_connected_peers(), 1);
        service.disconnect_peer(pid(1)).unwrap();
        worker.process_messages();
        assert_eq!(service.num_connected_peers(), 0);
    }

    #[test]
    fn test_publish_gossip() {
        let (mut worker, service) = make_network();
        service.publish_gossip(contact(1)).unwrap();
        worker.process_messages();
        // CRDS tablosuna eklendi
        let status = worker.status();
        assert_eq!(status.crds_entries, 1);
    }

    #[test]
    fn test_gossip_event_emitted() {
        let (mut worker, service) = make_network();
        service.publish_gossip(contact(5)).unwrap();
        worker.process_messages();

        let event = service.try_recv_event();
        assert!(matches!(event, Some(NetworkEvent::GossipValue(_))));
    }

    #[test]
    fn test_retransmit_shred() {
        let (mut worker, service) = make_network();
        let shred = Shred::new(100, 0, ShredType::Data, [1u8; 32], b"data".to_vec());
        service.retransmit_shred(shred).unwrap();
        worker.process_messages();
        let status = worker.status();
        assert_eq!(status.shreds_processed, 1);
    }

    #[test]
    fn test_dht_put_get() {
        let (mut worker, service) = make_network();
        service.dht_put(b"key".to_vec(), b"value".to_vec()).unwrap();
        worker.process_messages();

        // Doğrulama
        let val = worker.discovery.get_value(b"key").unwrap();
        assert_eq!(val, b"value");
    }

    #[test]
    fn test_peer_connect_event() {
        let (mut worker, service) = make_network();
        service.connect_peer(pid(2), "127.0.0.2:30333".into()).unwrap();
        worker.process_messages();

        let event = service.try_recv_event();
        assert!(matches!(event, Some(NetworkEvent::PeerConnected(_))));
    }

    #[test]
    fn test_network_status() {
        let (worker, _service) = make_network();
        let status = worker.status();
        assert_eq!(status.connected_peers, 0);
        assert_eq!(status.shreds_processed, 0);
    }

    #[test]
    fn test_multiple_peers() {
        let (mut worker, service) = make_network();
        for i in 1u8..=5 {
            service.connect_peer(pid(i), format!("127.0.0.{i}:30333")).unwrap();
        }
        worker.process_messages();
        assert_eq!(service.num_connected_peers(), 5);
        let status = worker.status();
        assert_eq!(status.known_peers, 5);
    }

    /// Yasaklı peer'a bağlantı girişimi engellenmeli
    #[test]
    fn test_banned_peer_connection_rejected() {
        use crate::peer::store::ReputationChange;

        let (mut worker, service) = make_network();

        // Önce peer'i ekleyip yasakla
        let banned = pid(99);
        worker.peer_store.add_peer(banned.clone(), "10.0.0.1:30333", crate::peer::info::PeerRole::Full);
        worker.peer_store.report_peer(&banned, &ReputationChange::new(i32::MIN, "kötü peer"));
        assert!(worker.peer_store.is_banned(&banned));

        // Şimdi bağlanmaya çalış
        service.connect_peer(banned, "10.0.0.1:30333".into()).unwrap();
        worker.process_messages();

        // Yasaklı peer bağlantı sayısını artırmamalı
        assert_eq!(service.num_connected_peers(), 0, "Yasaklı peer bağlantısı reddedilmeli");
    }
}
