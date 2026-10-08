//! Node konfigürasyonu
//!
//! Polkadot SDK `Configuration` + Avalanchego `Config` pattern

use crate::errors::{NodeError, NodeResult};
use serde::{Deserialize, Serialize};

// ─── ChainSpec ────────────────────────────────────────────────────────────────

/// Zincir spesifikasyonu
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainSpec {
    /// Mainnet
    Mainnet,
    /// Testnet
    Testnet,
    /// Geliştirici modu (tek validator, anında finality)
    Dev,
    /// Özel zincir (genesis hash ile)
    Custom { genesis_hash: [u8; 32] },
}

impl ChainSpec {
    pub fn chain_id(&self) -> u64 {
        use nova_primitives::constants::{DEVNET_CHAIN_ID, MAINNET_CHAIN_ID, TESTNET_CHAIN_ID};
        match self {
            ChainSpec::Mainnet => MAINNET_CHAIN_ID,   // 1984
            ChainSpec::Testnet => TESTNET_CHAIN_ID,   // 19840
            ChainSpec::Dev => DEVNET_CHAIN_ID,        // 198400
            ChainSpec::Custom { .. } => 42_999,
        }
    }

    pub fn is_dev(&self) -> bool {
        matches!(self, ChainSpec::Dev)
    }
}

// ─── NetworkConfig ────────────────────────────────────────────────────────────

/// P2P ağ konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Dinleme adresi
    pub listen_addr: String,
    /// P2P portu
    pub p2p_port: u16,
    /// Bootstrap node'ları
    pub boot_nodes: Vec<String>,
    /// Maksimum peer sayısı
    pub max_peers: u32,
    /// Minimum peer sayısı (bağlanmayı bekle)
    pub min_peers: u32,
    /// Noise XX protokolü etkin mi?
    pub noise_enabled: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".into(),
            p2p_port: 30_333,
            boot_nodes: vec![],
            max_peers: 50,
            min_peers: 3,
            noise_enabled: true,
        }
    }
}

// ─── RpcConfig ────────────────────────────────────────────────────────────────

/// JSON-RPC sunucu konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    /// HTTP RPC portu
    pub http_port: u16,
    /// WebSocket portu
    pub ws_port: u16,
    /// Maksimum bağlantı sayısı
    pub max_connections: u32,
    /// CORS allowed origins
    pub cors_origins: Vec<String>,
    /// HTTP etkin mi?
    pub http_enabled: bool,
    /// WebSocket etkin mi?
    pub ws_enabled: bool,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            http_port: 9_933,
            ws_port: 9_944,
            max_connections: 100,
            // NOVA-RED-NODE-003: Wildcard CORS kaldırıldı — explicit origin listesi gerektirir.
            cors_origins: vec![],
            http_enabled: true,
            ws_enabled: true,
        }
    }
}

// ─── ValidatorConfig ──────────────────────────────────────────────────────────

/// Validator konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// Validator etkin mi?
    pub enabled: bool,
    /// Validator ID (ağda unique)
    pub validator_id: u64,
    /// BLS anahtar dosyası yolu
    pub bls_key_path: Option<String>,
    /// Guardian olarak çalışıyor mu?
    pub guardian_mode: bool,
    /// Stake miktarı (NSC, en küçük birim)
    pub stake_amount: u128,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            validator_id: 0,
            bls_key_path: None,
            guardian_mode: false,
            stake_amount: 0,
        }
    }
}

// ─── MetricsConfig ────────────────────────────────────────────────────────────

/// Prometheus metrik konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self { enabled: false, port: 9_615 }
    }
}

// ─── DatabaseConfig ───────────────────────────────────────────────────────────

/// Veritabanı konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Veri dizini
    pub data_dir: String,
    /// Cache boyutu (MB)
    pub cache_size_mb: u32,
    /// In-memory mod (test için)
    pub in_memory: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            data_dir: "./nova-data".into(),
            cache_size_mb: 512,
            in_memory: false,
        }
    }
}

// ─── NodeConfig ───────────────────────────────────────────────────────────────

/// Validator node tam konfigürasyonu — Polkadot SDK `Configuration` pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Zincir spesifikasyonu
    pub chain: ChainSpec,
    /// Ağ konfigürasyonu
    pub network: NetworkConfig,
    /// RPC konfigürasyonu
    pub rpc: RpcConfig,
    /// Validator konfigürasyonu
    pub validator: ValidatorConfig,
    /// Metrik konfigürasyonu
    pub metrics: MetricsConfig,
    /// Veritabanı konfigürasyonu
    pub database: DatabaseConfig,
    /// Log seviyesi
    pub log_level: LogLevel,
    /// Telemetri URL (opsiyonel)
    pub telemetry_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl NodeConfig {
    /// Geliştirici modu konfigürasyonu
    pub fn dev() -> Self {
        Self {
            chain: ChainSpec::Dev,
            network: NetworkConfig {
                p2p_port: 30_333,
                max_peers: 10,
                min_peers: 0,
                ..Default::default()
            },
            rpc: RpcConfig::default(),
            validator: ValidatorConfig {
                enabled: true,
                validator_id: 1,
                ..Default::default()
            },
            metrics: MetricsConfig::default(),
            database: DatabaseConfig {
                in_memory: true,
                ..Default::default()
            },
            log_level: LogLevel::Debug,
            telemetry_url: None,
        }
    }

    /// Mainnet konfigürasyonu
    pub fn mainnet(validator_id: u64) -> Self {
        Self {
            chain: ChainSpec::Mainnet,
            network: NetworkConfig {
                max_peers: 50,
                min_peers: 5,
                noise_enabled: true,
                ..Default::default()
            },
            rpc: RpcConfig {
                max_connections: 500,
                // NOVA-RED-NODE-003: Mainnet'te wildcard CORS yasak — boş (kısıtlı).
                cors_origins: vec![],
                ..Default::default()
            },
            validator: ValidatorConfig {
                enabled: true,
                validator_id,
                stake_amount: nova_primitives::constants::MIN_VALIDATOR_STAKE,
                ..Default::default()
            },
            metrics: MetricsConfig { enabled: true, port: 9_615 },
            database: DatabaseConfig {
                data_dir: "/data/nova".into(),
                cache_size_mb: 2048,
                in_memory: false,
            },
            log_level: LogLevel::Info,
            telemetry_url: None,
        }
    }

    /// Testnet konfigürasyonu
    pub fn testnet(validator_id: u64) -> Self {
        Self {
            chain: ChainSpec::Testnet,
            network: NetworkConfig::default(),
            rpc: RpcConfig::default(),
            validator: ValidatorConfig {
                enabled: true,
                validator_id,
                ..Default::default()
            },
            metrics: MetricsConfig { enabled: true, port: 9_615 },
            database: DatabaseConfig::default(),
            log_level: LogLevel::Info,
            telemetry_url: None,
        }
    }

    /// Konfigürasyonu doğrula
    pub fn validate(&self) -> NodeResult<()> {
        if self.network.p2p_port == self.rpc.http_port
            || self.network.p2p_port == self.rpc.ws_port
        {
            return Err(NodeError::ConfigError {
                reason: "P2P portu RPC portlarıyla çakışamaz".into(),
            });
        }
        if self.rpc.http_port == self.rpc.ws_port {
            return Err(NodeError::ConfigError {
                reason: "HTTP ve WebSocket portu aynı olamaz".into(),
            });
        }
        if self.validator.enabled && self.validator.validator_id == 0 {
            return Err(NodeError::ConfigError {
                reason: "validator_id sıfır olamaz".into(),
            });
        }
        if self.network.max_peers < self.network.min_peers {
            return Err(NodeError::ConfigError {
                reason: "max_peers >= min_peers olmalı".into(),
            });
        }
        Ok(())
    }

    /// JSON olarak serialize et
    pub fn to_json(&self) -> NodeResult<String> {
        serde_json::to_string_pretty(self).map_err(|e| NodeError::SerializationError {
            reason: e.to_string(),
        })
    }

    /// JSON'dan deserialize et
    pub fn from_json(json: &str) -> NodeResult<Self> {
        serde_json::from_str(json).map_err(|e| NodeError::SerializationError {
            reason: e.to_string(),
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_spec_chain_id() {
        use nova_primitives::constants::{DEVNET_CHAIN_ID, MAINNET_CHAIN_ID, TESTNET_CHAIN_ID};
        assert_eq!(ChainSpec::Mainnet.chain_id(), MAINNET_CHAIN_ID); // 1984
        assert_eq!(ChainSpec::Testnet.chain_id(), TESTNET_CHAIN_ID); // 19840
        assert_eq!(ChainSpec::Dev.chain_id(), DEVNET_CHAIN_ID);      // 198400
        assert!(ChainSpec::Dev.is_dev());
        assert!(!ChainSpec::Mainnet.is_dev());
    }

    #[test]
    fn test_dev_config_valid() {
        let cfg = NodeConfig::dev();
        assert!(cfg.validate().is_ok());
        assert!(cfg.database.in_memory);
        assert!(cfg.validator.enabled);
    }

    #[test]
    fn test_testnet_config_valid() {
        let cfg = NodeConfig::testnet(42);
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.validator.validator_id, 42);
        assert_eq!(cfg.chain, ChainSpec::Testnet);
        assert_eq!(cfg.chain.chain_id(), nova_primitives::constants::TESTNET_CHAIN_ID);
    }

    #[test]
    fn test_mainnet_config_valid() {
        let cfg = NodeConfig::mainnet(1);
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.validator.validator_id, 1);
        assert_eq!(cfg.chain, ChainSpec::Mainnet);
        assert_eq!(cfg.chain.chain_id(), nova_primitives::constants::MAINNET_CHAIN_ID);
        assert_eq!(cfg.database.data_dir, "/data/nova");
        assert!(!cfg.database.in_memory);
        assert!(cfg.metrics.enabled);
        assert!(cfg.network.noise_enabled);
    }

    #[test]
    fn test_config_port_conflict_p2p_http() {
        let mut cfg = NodeConfig::dev();
        cfg.rpc.http_port = cfg.network.p2p_port;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_port_conflict_http_ws() {
        let mut cfg = NodeConfig::dev();
        cfg.rpc.ws_port = cfg.rpc.http_port;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validator_id_zero() {
        let mut cfg = NodeConfig::dev();
        cfg.validator.validator_id = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_max_peers_less_than_min() {
        let mut cfg = NodeConfig::dev();
        cfg.network.max_peers = 2;
        cfg.network.min_peers = 5;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_json_roundtrip() {
        let cfg = NodeConfig::dev();
        let json = cfg.to_json().unwrap();
        let cfg2 = NodeConfig::from_json(&json).unwrap();
        assert_eq!(cfg.chain, cfg2.chain);
        assert_eq!(cfg.validator.validator_id, cfg2.validator.validator_id);
    }

    #[test]
    fn test_network_config_default() {
        let net = NetworkConfig::default();
        assert_eq!(net.p2p_port, 30_333);
        assert!(net.noise_enabled);
        assert_eq!(net.max_peers, 50);
    }

    #[test]
    fn test_rpc_config_default() {
        let rpc = RpcConfig::default();
        assert_eq!(rpc.http_port, 9_933);
        assert_eq!(rpc.ws_port, 9_944);
        assert!(rpc.http_enabled && rpc.ws_enabled);
    }
}
