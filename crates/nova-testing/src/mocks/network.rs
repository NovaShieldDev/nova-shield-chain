//! Mock ağ — P2P mesajlaşma simülasyonu

use dashmap::DashMap;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Ağ mesajı
#[derive(Debug, Clone)]
pub struct MockMessage {
    pub from: [u8; 32],
    pub to: Option<[u8; 32]>, // None = broadcast
    pub topic: String,
    pub payload: Vec<u8>,
}

impl MockMessage {
    pub fn broadcast(from: [u8; 32], topic: impl Into<String>, payload: Vec<u8>) -> Self {
        Self { from, to: None, topic: topic.into(), payload }
    }

    pub fn direct(from: [u8; 32], to: [u8; 32], topic: impl Into<String>, payload: Vec<u8>) -> Self {
        Self { from, to: Some(to), topic: topic.into(), payload }
    }
}

/// Mock ağ node'u
#[derive(Clone)]
pub struct MockNetworkNode {
    pub id: [u8; 32],
    inbox: Arc<Mutex<VecDeque<MockMessage>>>,
}

impl MockNetworkNode {
    pub fn new(id: [u8; 32]) -> Self {
        Self { id, inbox: Arc::new(Mutex::new(VecDeque::new())) }
    }

    /// Mesaj al
    pub fn receive(&self, msg: MockMessage) {
        self.inbox.lock().push_back(msg);
    }

    /// Gelen mesajları oku
    pub fn drain_inbox(&self) -> Vec<MockMessage> {
        self.inbox.lock().drain(..).collect()
    }

    pub fn inbox_len(&self) -> usize {
        self.inbox.lock().len()
    }
}

/// Mock ağ — node'lar arası mesajlaşma
pub struct MockNetwork {
    nodes: DashMap<[u8; 32], MockNetworkNode>,
    total_messages: Arc<parking_lot::RwLock<u64>>,
    latency_ms: u64,
}

impl MockNetwork {
    pub fn new() -> Self {
        Self {
            nodes: DashMap::new(),
            total_messages: Arc::new(parking_lot::RwLock::new(0)),
            latency_ms: 0,
        }
    }

    /// Belirtilen gecikmeyle
    pub fn with_latency(latency_ms: u64) -> Self {
        let mut n = Self::new();
        n.latency_ms = latency_ms;
        n
    }

    /// Node ekle
    pub fn add_node(&self, id: [u8; 32]) -> MockNetworkNode {
        let node = MockNetworkNode::new(id);
        self.nodes.insert(id, node.clone());
        node
    }

    /// Broadcast mesaj gönder
    pub fn broadcast(&self, msg: MockMessage) {
        let from = msg.from;
        let mut count = 0u64;
        for node in self.nodes.iter() {
            if *node.key() != from {
                node.receive(msg.clone());
                count += 1;
            }
        }
        *self.total_messages.write() += count;
    }

    /// Direkt mesaj gönder
    pub fn send(&self, msg: MockMessage) -> bool {
        if let Some(to) = msg.to {
            if let Some(node) = self.nodes.get(&to) {
                node.receive(msg);
                *self.total_messages.write() += 1;
                return true;
            }
        }
        false
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn total_messages(&self) -> u64 {
        *self.total_messages.read()
    }

    pub fn latency_ms(&self) -> u64 {
        self.latency_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> [u8; 32] { [n; 32] }

    #[test]
    fn test_mock_network_add_nodes() {
        let net = MockNetwork::new();
        net.add_node(id(1));
        net.add_node(id(2));
        assert_eq!(net.node_count(), 2);
    }

    #[test]
    fn test_mock_network_broadcast() {
        let net = MockNetwork::new();
        let n1 = net.add_node(id(1));
        let n2 = net.add_node(id(2));
        let n3 = net.add_node(id(3));

        net.broadcast(MockMessage::broadcast(id(1), "block", vec![1, 2, 3]));

        // n1 kendi mesajını almaz
        assert_eq!(n1.inbox_len(), 0);
        assert_eq!(n2.inbox_len(), 1);
        assert_eq!(n3.inbox_len(), 1);
        assert_eq!(net.total_messages(), 2);
    }

    #[test]
    fn test_mock_network_direct_message() {
        let net = MockNetwork::new();
        let n1 = net.add_node(id(1));
        let n2 = net.add_node(id(2));

        let sent = net.send(MockMessage::direct(id(1), id(2), "ping", vec![]));
        assert!(sent);
        assert_eq!(n2.inbox_len(), 1);
        assert_eq!(n1.inbox_len(), 0);
    }

    #[test]
    fn test_mock_network_send_unknown_node() {
        let net = MockNetwork::new();
        let sent = net.send(MockMessage::direct(id(1), id(99), "ping", vec![]));
        assert!(!sent);
    }

    #[test]
    fn test_mock_network_drain_inbox() {
        let net = MockNetwork::new();
        let node = net.add_node(id(1));
        net.add_node(id(2));

        net.broadcast(MockMessage::broadcast(id(2), "t1", vec![]));
        net.broadcast(MockMessage::broadcast(id(2), "t2", vec![]));

        let msgs = node.drain_inbox();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].topic, "t1");
        assert_eq!(node.inbox_len(), 0); // drain sonrası boş
    }

    #[test]
    fn test_mock_network_with_latency() {
        let net = MockNetwork::with_latency(50);
        assert_eq!(net.latency_ms(), 50);
    }
}
