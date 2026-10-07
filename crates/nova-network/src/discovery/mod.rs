//! Peer keşif mekanizmaları
//!
//! Polkadot `discovery.rs`:
//! - Bootstrap nodes
//! - Kademlia random walk (FIND_NODE)
//! - mDNS (yerel ağ)
//! - `DiscoveryOut::Discovered` event

pub mod kademlia;

pub use kademlia::{KBucket, KademliaId, KademliaQuery, KademliaTable, RoutingTableEntry, KADEMLIA_K};

use crate::config::{BootstrapNode, KademliaConfig};
use crate::peer::info::PeerId;

// ─── DiscoveryEvent ───────────────────────────────────────────────────────────

/// Keşif olayı — Polkadot `DiscoveryOut`
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// Yeni peer keşfedildi
    Discovered { peer_id: PeerId, addr: String },
    /// DHT kaydı bulundu
    DhtRecord { key: Vec<u8>, value: Vec<u8> },
    /// Sorgu tamamlandı
    QueryComplete { query_id: u64, found: Vec<(PeerId, String)> },
    /// Bootstrap bağlantısı kuruldu
    BootstrapComplete,
}

// ─── DiscoveryBehaviour ───────────────────────────────────────────────────────

/// Keşif davranışı — Polkadot `DiscoveryBehaviour`
pub struct DiscoveryBehaviour {
    local_peer: PeerId,
    table: KademliaTable,
    config: KademliaConfig,
    /// Bekleyen sorgular
    queries: Vec<KademliaQuery>,
    next_query_id: u64,
    /// Çıkış kuyruğu
    pending_events: Vec<DiscoveryEvent>,
    /// Bootstrap tamamlandı mı?
    bootstrap_complete: bool,
}

impl DiscoveryBehaviour {
    pub fn new(local_peer: PeerId, config: KademliaConfig) -> Self {
        let table = KademliaTable::new(&local_peer);
        Self {
            local_peer,
            table,
            config,
            queries: vec![],
            next_query_id: 1,
            pending_events: vec![],
            bootstrap_complete: false,
        }
    }

    /// Bootstrap node'larını ekle
    pub fn add_bootstrap_nodes(&mut self, nodes: &[BootstrapNode], now_ms: u64) {
        for node in nodes {
            // node_id hex → bytes
            if let Ok(bytes) = hex::decode(&node.node_id) {
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    let peer = PeerId::from_bytes(arr);
                    self.table.add_peer(&peer, &node.addr, now_ms);
                    self.pending_events.push(DiscoveryEvent::Discovered {
                        peer_id: peer,
                        addr: node.addr.clone(),
                    });
                }
            }
        }
        if !nodes.is_empty() {
            self.pending_events.push(DiscoveryEvent::BootstrapComplete);
            self.bootstrap_complete = true;
        }
    }

    /// Yeni peer öğrenildi (add_self_reported_address benzeri)
    pub fn add_known_peer(&mut self, peer: PeerId, addr: String, now_ms: u64) {
        self.table.add_peer(&peer, &addr, now_ms);
        self.pending_events.push(DiscoveryEvent::Discovered {
            peer_id: peer,
            addr,
        });
    }

    /// FIND_NODE sorgusu başlat — Polkadot: Kademlia random walk
    pub fn find_node(&mut self, target: PeerId) -> u64 {
        let query_id = self.next_query_id;
        self.next_query_id += 1;
        let mut query = KademliaQuery::new(query_id, target);
        query.step(&self.table);
        if query.complete {
            let found = query.found.clone();
            self.pending_events.push(DiscoveryEvent::QueryComplete { query_id, found });
        } else {
            self.queries.push(query);
        }
        query_id
    }

    /// DHT kayıt koy
    pub fn put_value(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.table.put_record(key, value);
    }

    /// DHT kayıt al
    pub fn get_value(&self, key: &[u8]) -> Option<&Vec<u8>> {
        self.table.get_record(key)
    }

    /// Bekleyen olayları al
    pub fn poll_events(&mut self) -> Vec<DiscoveryEvent> {
        // Bekleyen sorguları tamamla
        let mut completed = vec![];
        for query in &mut self.queries {
            if !query.complete {
                query.step(&self.table);
            }
            if query.complete {
                completed.push(DiscoveryEvent::QueryComplete {
                    query_id: query.query_id,
                    found: query.found.clone(),
                });
            }
        }
        self.queries.retain(|q| !q.complete);
        self.pending_events.extend(completed);

        std::mem::take(&mut self.pending_events)
    }

    pub fn peer_count(&self) -> usize {
        self.table.peer_count()
    }

    pub fn is_bootstrap_complete(&self) -> bool {
        self.bootstrap_complete
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KademliaConfig;

    fn pid(n: u8) -> PeerId {
        PeerId::from_bytes([n; 32])
    }

    fn make_discovery() -> DiscoveryBehaviour {
        DiscoveryBehaviour::new(pid(0), KademliaConfig::default())
    }

    #[test]
    fn test_add_known_peer() {
        let mut d = make_discovery();
        d.add_known_peer(pid(1), "127.0.0.1:30333".into(), 1000);
        assert_eq!(d.peer_count(), 1);
    }

    #[test]
    fn test_discovery_event_on_add() {
        let mut d = make_discovery();
        d.add_known_peer(pid(1), "127.0.0.1:30333".into(), 1000);
        let events = d.poll_events();
        assert!(events.iter().any(|e| matches!(e, DiscoveryEvent::Discovered { .. })));
    }

    #[test]
    fn test_find_node() {
        let mut d = make_discovery();
        for i in 1u8..=20 {
            d.add_known_peer(pid(i), format!("127.0.0.{i}:30333"), 1000);
        }
        let qid = d.find_node(pid(10));
        assert!(qid > 0);
        let events = d.poll_events();
        // Sorgu tamamlandı olayı olmalı
        assert!(events.iter().any(|e| matches!(e, DiscoveryEvent::QueryComplete { .. })));
    }

    #[test]
    fn test_dht_put_get() {
        let mut d = make_discovery();
        d.put_value(b"key1".to_vec(), b"val1".to_vec());
        let v = d.get_value(b"key1").unwrap();
        assert_eq!(v, b"val1");
    }

    #[test]
    fn test_bootstrap_nodes() {
        let mut d = make_discovery();
        let hex_id = hex::encode([2u8; 32]);
        let nodes = vec![BootstrapNode::new(hex_id, "127.0.0.2:30333")];
        d.add_bootstrap_nodes(&nodes, 1000);
        assert!(d.is_bootstrap_complete());
        let events = d.poll_events();
        assert!(events.iter().any(|e| matches!(e, DiscoveryEvent::BootstrapComplete)));
    }

    #[test]
    fn test_empty_discovery() {
        let d = make_discovery();
        assert_eq!(d.peer_count(), 0);
        assert!(!d.is_bootstrap_complete());
    }
}
