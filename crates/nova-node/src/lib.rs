//! nova-node — Nova Shield Chain Validator Node (§10)
//!
//! Polkadot SDK ServiceBuilder + Avalanchego Node pattern:
//! - NodeConfig: konfigürasyon
//! - NodeBuilder: builder pattern
//! - NovaNode: tüm servisleri yönetir
//! - Services: Consensus, Mempool, Network, Guardian, Metrics
//! - Sync: Full, Light, Warp sync

pub mod bootnodes;
pub mod cli;
pub mod config;
pub mod errors;
pub mod genesis;
pub mod keystore;
pub mod node;
pub mod services;
pub mod snapshot;
pub mod sync;
pub mod upgrade;

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use cli::{CliArgs, CliCommand};
pub use config::{
    ChainSpec, DatabaseConfig, LogLevel, MetricsConfig, NetworkConfig, NodeConfig, RpcConfig,
    ValidatorConfig,
};
pub use errors::{NodeError, NodeResult};
pub use node::{NodeBuilder, NodeHealthReport, NodeState, NovaNode, SyncRunner};
pub use services::{
    ConsensusEngine, ConsensusService, FullNodeEngine, GuardianService, MempoolService,
    MetricsService, NetworkEngine, NetworkService, NodeService, ServiceStatus,
};
pub use sync::{FullSync, LightSync, SyncEngine, SyncMode, SyncState, WarpSync};

// ─── Node Versiyonu ───────────────────────────────────────────────────────────

pub const NODE_VERSION: &str = "0.1.0";
pub const NOVA_PROTOCOL_VERSION: u32 = 1;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use services::network::{NetworkMessageKind, PeerInfo};

    fn make_node() -> NovaNode {
        let config = NodeConfig::dev();
        NodeBuilder::new(config).unwrap().build()
    }

    // ── Node Lifecycle ────────────────────────────────────────────────────────

    #[test]
    fn test_node_full_lifecycle() {
        let mut node = make_node();
        assert_eq!(node.state(), &NodeState::Initialized);

        node.start().unwrap();
        assert!(node.state().is_running());
        assert!(node.health_check().is_healthy());

        node.stop().unwrap();
        assert_eq!(node.state(), &NodeState::Stopped);
    }

    #[test]
    fn test_node_config_dev() {
        let node = make_node();
        assert!(node.config().chain.is_dev());
        assert!(node.config().database.in_memory);
        assert!(node.config().validator.enabled);
    }

    #[test]
    fn test_node_version() {
        assert_eq!(NODE_VERSION, "0.1.0");
        assert_eq!(NOVA_PROTOCOL_VERSION, 1);
    }

    // ── Consensus + Mempool ──────────────────────────────────────────────────

    #[test]
    fn test_node_produce_and_finalize_block() {
        let mut node = make_node();
        node.start().unwrap();

        let hash = node.consensus_mut().produce_block().unwrap();
        assert_ne!(hash, [0u8; 32]);
        node.consensus_mut().finalize_block(hash).unwrap();
        assert_eq!(node.consensus_mut().blocks_finalized(), 1);

        node.stop().unwrap();
    }

    #[test]
    fn test_node_mempool_and_batch() {
        let mut node = make_node();
        node.start().unwrap();

        for i in 0u8..5 {
            node.mempool_mut()
                .add_transaction([i; 20], (i as u64 + 1) * 100, vec![i])
                .unwrap();
        }
        assert_eq!(node.mempool_mut().pending_count(), 5);

        let batch = node.mempool_mut().take_for_batch(3);
        assert_eq!(batch.len(), 3);
        assert!(batch[0].gas_price >= batch[1].gas_price);

        node.stop().unwrap();
    }

    // ── Network ───────────────────────────────────────────────────────────────

    #[test]
    fn test_node_network_peers() {
        let mut node = make_node();
        node.start().unwrap();

        let peer = PeerInfo::new([0x01u8; 32], "192.168.1.1", 30_333);
        node.network_mut().connect_peer(peer).unwrap();
        assert_eq!(node.network_mut().peer_count(), 1);

        node.stop().unwrap();
        assert_eq!(node.network_mut().peer_count(), 0);
    }

    #[test]
    fn test_node_network_broadcast() {
        let mut node = make_node();
        node.start().unwrap();

        node.network_mut().connect_peer(PeerInfo::new([1u8; 32], "1.2.3.4", 30333)).unwrap();
        node.network_mut().connect_peer(PeerInfo::new([2u8; 32], "1.2.3.5", 30333)).unwrap();

        let sent = node.network_mut().broadcast(NetworkMessageKind::BlockAnnounce, vec![0x42]);
        assert_eq!(sent, 2);

        node.stop().unwrap();
    }

    // ── Sync ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_sync_full_mode() {
        let runner = SyncRunner::new(SyncMode::Full);
        let mut engine = runner.create_engine();
        engine.start_sync(10).unwrap();

        for i in 1u64..=10 {
            engine.process_block(i, [i as u8; 32]).unwrap();
        }
        assert!(engine.state().is_synced());
        assert_eq!(engine.best_block(), 10);
    }

    #[test]
    fn test_sync_light_mode() {
        let runner = SyncRunner::new(SyncMode::Light);
        let mut engine = runner.create_engine();
        engine.start_sync(5).unwrap();

        for i in 1u64..=5 {
            engine.process_block(i, [i as u8; 32]).unwrap();
        }
        assert!(engine.state().is_synced());
    }

    // ── CLI ───────────────────────────────────────────────────────────────────

    #[test]
    fn test_cli_dev_mode_to_config() {
        let args = CliArgs::dev();
        let config = args.to_node_config().unwrap();
        assert!(config.chain.is_dev());
        assert!(config.database.in_memory);
    }

    #[test]
    fn test_cli_parse_and_run() {
        let args = CliArgs::parse(&["--dev", "--validator-id", "7", "--sync", "warp"]).unwrap();
        assert_eq!(args.validator_id, Some(7));
        assert!(args.sync_mode.is_warp());

        let config = args.to_node_config().unwrap();
        assert_eq!(config.validator.validator_id, 7);
    }
}
