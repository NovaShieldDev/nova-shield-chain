//! Nova Network yapılandırması
//!
//! Polkadot: `FullNetworkConfiguration` + `NetworkConfiguration` pattern.
//! Avalanchego: `network.Config` + `peer.Config` pattern.

use serde::{Deserialize, Serialize};

// ─── NetworkConfig ─────────────────────────────────────────────────────────────

/// Ana ağ yapılandırması
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Dinleme adresi (ör. "0.0.0.0:30333")
    pub listen_addr: String,
    /// Bootstrap node listesi — Polkadot "bootnodes" pattern
    pub bootnodes: Vec<BootstrapNode>,
    /// Maksimum gelen peer sayısı
    pub max_inbound_peers: usize,
    /// Maksimum giden peer sayısı
    pub max_outbound_peers: usize,
    /// Gossip yapılandırması
    pub gossip: GossipConfig,
    /// Turbine (block propagation) yapılandırması
    pub turbine: TurbineConfig,
    /// Transport yapılandırması
    pub transport: TransportConfig,
    /// Kademlia DHT yapılandırması
    pub kademlia: KademliaConfig,
    /// Protokol kimliği — "/" ile başlar, örneğin "/nova-shield/1.0"
    pub protocol_id: String,
    /// Node versiyon bilgisi
    pub node_version: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:30333".into(),
            bootnodes: vec![],
            max_inbound_peers: 25,
            max_outbound_peers: 25,
            gossip: GossipConfig::default(),
            turbine: TurbineConfig::default(),
            transport: TransportConfig::default(),
            kademlia: KademliaConfig::default(),
            protocol_id: "/nova-shield/1.0".into(),
            node_version: "nova-shield/0.1.0".into(),
        }
    }
}

// ─── BootstrapNode ────────────────────────────────────────────────────────────

/// Bootstrap node — Polkadot `MultiaddrWithPeerId` benzeri
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapNode {
    /// Node kimliği (hex-encoded 32 byte)
    pub node_id: String,
    /// Adres (ip:port)
    pub addr: String,
}

impl BootstrapNode {
    pub fn new(node_id: impl Into<String>, addr: impl Into<String>) -> Self {
        Self { node_id: node_id.into(), addr: addr.into() }
    }
}

// ─── GossipConfig ─────────────────────────────────────────────────────────────

/// Gossip protokol yapılandırması
/// Solana CRDS Push FANOUT=9, Pull ile Bloom filter anti-entropy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    /// Push gossip fanout — Solana: CRDS_GOSSIP_PUSH_FANOUT=9
    pub push_fanout: usize,
    /// Push timeout ms — Solana: CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS=30_000
    pub push_timeout_ms: u64,
    /// Pull bloom filter hata oranı — Solana: FALSE_RATE=0.1
    pub bloom_false_rate: f64,
    /// Pull bloom filter anahtar sayısı — Solana: KEYS=8.0
    pub bloom_num_keys: usize,
    /// Maksimum CRDS tablo büyüklüğü
    pub max_crds_entries: usize,
    /// Gossip döngü aralığı ms
    pub gossip_interval_ms: u64,
    /// Avalanchego: ValidatorGossiper — sadece validator'lardan gossip
    pub validators_only: bool,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            push_fanout: 9,             // Solana CRDS_GOSSIP_PUSH_FANOUT
            push_timeout_ms: 30_000,    // Solana CRDS_GOSSIP_PUSH_MSG_TIMEOUT_MS
            bloom_false_rate: 0.1,      // Solana FALSE_RATE
            bloom_num_keys: 8,          // Solana KEYS
            max_crds_entries: 65_536,
            gossip_interval_ms: 1_000,
            validators_only: false,
        }
    }
}

// ─── TurbineConfig ────────────────────────────────────────────────────────────

/// Turbine blok yayılım yapılandırması
/// Solana: DATA_PLANE_FANOUT=200, MAX_NUM_TURBINE_HOPS=4
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurbineConfig {
    /// Veri düzlemi fanout — Solana: DATA_PLANE_FANOUT=200
    pub data_plane_fanout: usize,
    /// Maksimum atlama sayısı — Solana: MAX_NUM_TURBINE_HOPS=4
    pub max_hops: usize,
    /// Shred boyutu (byte)
    pub shred_size: usize,
    /// Shred timeout ms
    pub shred_timeout_ms: u64,
}

impl Default for TurbineConfig {
    fn default() -> Self {
        Self {
            data_plane_fanout: 200,     // Solana DATA_PLANE_FANOUT
            max_hops: 4,                // Solana MAX_NUM_TURBINE_HOPS
            shred_size: 1_232,          // Solana SHRED_DATA_PAYLOAD_SIZE
            shred_timeout_ms: 400,      // 1 slot = 400ms
        }
    }
}

// ─── TransportConfig ──────────────────────────────────────────────────────────

/// Transport katmanı yapılandırması
/// Polkadot: TCP → Noise XX → Yamux
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Noise handshake zaman aşımı sn — Polkadot: 20s
    pub noise_timeout_secs: u64,
    /// TCP nodelay
    pub tcp_nodelay: bool,
    /// Yamux pencere boyutu
    pub yamux_window_size: u32,
    /// Maksimum frame boyutu
    pub max_frame_size: usize,
    /// Bant genişliği limiti (byte/s), 0=sınırsız
    pub bandwidth_limit_bps: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            noise_timeout_secs: 20,
            tcp_nodelay: true,
            yamux_window_size: 256 * 1024,
            max_frame_size: 16 * 1024 * 1024,
            bandwidth_limit_bps: 0,
        }
    }
}

// ─── KademliaConfig ───────────────────────────────────────────────────────────

/// Kademlia DHT yapılandırması
/// Polkadot: DEFAULT_KADEMLIA_REPLICATION_FACTOR=20, KAD_QUERY_TIMEOUT=300s
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KademliaConfig {
    /// Replikasyon faktörü — Polkadot: DEFAULT_KADEMLIA_REPLICATION_FACTOR=20
    pub replication_factor: usize,
    /// k-bucket boyutu
    pub bucket_size: usize,
    /// Sorgu zaman aşımı sn — Polkadot: KAD_QUERY_TIMEOUT=300
    pub query_timeout_secs: u64,
    /// Random walk etkin mi?
    pub random_walk_enabled: bool,
    /// mDNS etkin mi? (yerel ağ keşfi)
    pub mdns_enabled: bool,
    /// Kayıt redundancy — Polkadot: GET_RECORD_REDUNDANCY_FACTOR=4
    pub get_record_redundancy: u32,
}

impl Default for KademliaConfig {
    fn default() -> Self {
        Self {
            replication_factor: 20,     // Polkadot DEFAULT_KADEMLIA_REPLICATION_FACTOR
            bucket_size: 20,
            query_timeout_secs: 300,    // Polkadot KAD_QUERY_TIMEOUT
            random_walk_enabled: true,
            mdns_enabled: false,
            get_record_redundancy: 4,   // Polkadot GET_RECORD_REDUNDANCY_FACTOR
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_gossip_config() {
        let cfg = GossipConfig::default();
        assert_eq!(cfg.push_fanout, 9);
        assert_eq!(cfg.push_timeout_ms, 30_000);
        assert!((cfg.bloom_false_rate - 0.1).abs() < 1e-9);
    }

    #[test]
    fn test_default_turbine_config() {
        let cfg = TurbineConfig::default();
        assert_eq!(cfg.data_plane_fanout, 200);
        assert_eq!(cfg.max_hops, 4);
    }

    #[test]
    fn test_default_kademlia_config() {
        let cfg = KademliaConfig::default();
        assert_eq!(cfg.replication_factor, 20);
        assert_eq!(cfg.query_timeout_secs, 300);
        assert_eq!(cfg.get_record_redundancy, 4);
    }

    #[test]
    fn test_network_config_defaults() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.max_inbound_peers, 25);
        assert_eq!(cfg.max_outbound_peers, 25);
        assert_eq!(cfg.protocol_id, "/nova-shield/1.0");
    }

    #[test]
    fn test_bootstrap_node() {
        let node = BootstrapNode::new("deadbeef", "127.0.0.1:30333");
        assert_eq!(node.addr, "127.0.0.1:30333");
        assert_eq!(node.node_id, "deadbeef");
    }
}
