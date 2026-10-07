//! Komut satırı arayüzü — nova-node CLI argümanları

use crate::config::{ChainSpec, LogLevel, NodeConfig};
use crate::errors::{NodeError, NodeResult};
use crate::sync::SyncMode;

// ─── CliCommand ───────────────────────────────────────────────────────────────

/// CLI komutu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Node başlat
    Run,
    /// Yeni anahtar üret
    GenerateKey,
    /// Mainnet genesis ceremony — N validator key üret + genesis oluştur
    Ceremony { validator_count: u8 },
    /// Node bilgisi göster
    NodeInfo,
    /// Genesis bloğu oluştur
    BuildGenesis,
    /// Benchmark çalıştır
    Benchmark { duration_secs: u64 },
    /// Well-known bootnode listesini yazdır
    PrintBootnodes,
    /// Versiyon göster
    Version,
}

// ─── CliArgs ──────────────────────────────────────────────────────────────────

/// CLI argümanları
#[derive(Debug, Clone)]
pub struct CliArgs {
    pub command: CliCommand,
    pub chain: ChainSpec,
    pub validator_id: Option<u64>,
    pub p2p_port: Option<u16>,
    pub rpc_port: Option<u16>,
    pub sync_mode: SyncMode,
    pub log_level: LogLevel,
    pub data_dir: Option<String>,
    pub boot_nodes: Vec<String>,
    pub no_metrics: bool,
    pub dev_mode: bool,
    /// Genesis dosyası yolu (opsiyonel — verilmezse zincir tipine göre varsayılan kullanılır)
    pub genesis_file: Option<String>,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            command: CliCommand::Run,
            chain: ChainSpec::Dev,
            validator_id: None,
            p2p_port: None,
            rpc_port: None,
            sync_mode: SyncMode::Full,
            log_level: LogLevel::Info,
            data_dir: None,
            boot_nodes: vec![],
            no_metrics: false,
            dev_mode: false,
            genesis_file: None,
        }
    }
}

impl CliArgs {
    /// Dev modu için kısayol
    pub fn dev() -> Self {
        Self {
            command: CliCommand::Run,
            chain: ChainSpec::Dev,
            dev_mode: true,
            sync_mode: SyncMode::Full,
            log_level: LogLevel::Debug,
            ..Default::default()
        }
    }

    /// CLI argümanlarından NodeConfig üret
    pub fn to_node_config(&self) -> NodeResult<NodeConfig> {
        let mut config = if self.dev_mode {
            NodeConfig::dev()
        } else {
            match &self.chain {
                ChainSpec::Mainnet => NodeConfig::mainnet(
                    self.validator_id.unwrap_or(1),
                ),
                ChainSpec::Testnet => NodeConfig::testnet(
                    self.validator_id.unwrap_or(1),
                ),
                _ => NodeConfig::dev(),
            }
        };

        if let Some(port) = self.p2p_port {
            config.network.p2p_port = port;
        }
        if let Some(port) = self.rpc_port {
            config.rpc.http_port = port;
            config.rpc.ws_port = port + 11;
        }
        if let Some(id) = self.validator_id {
            config.validator.validator_id = id;
        }
        if let Some(dir) = &self.data_dir {
            config.database.data_dir = dir.clone();
        }
        if !self.boot_nodes.is_empty() {
            config.network.boot_nodes = self.boot_nodes.clone();
        }
        if self.no_metrics {
            config.metrics.enabled = false;
        }
        config.log_level = self.log_level.clone();

        config.validate()?;
        Ok(config)
    }

    /// Basit string argüman ayrıştırıcı (mock)
    pub fn parse(args: &[&str]) -> NodeResult<Self> {
        let mut cli = CliArgs::default();

        let mut i = 0;
        while i < args.len() {
            match args[i] {
                "--dev" => { cli.dev_mode = true; cli.chain = ChainSpec::Dev; }
                "--mainnet" => { cli.chain = ChainSpec::Mainnet; }
                "--testnet" => { cli.chain = ChainSpec::Testnet; }
                "--validator-id" => {
                    i += 1;
                    let id = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--validator-id değeri eksik".into(),
                    })?;
                    cli.validator_id = Some(id.parse::<u64>().map_err(|_| NodeError::ConfigError {
                        reason: "geçersiz validator-id".into(),
                    })?);
                }
                "--p2p-port" => {
                    i += 1;
                    let port = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--p2p-port değeri eksik".into(),
                    })?;
                    cli.p2p_port = Some(port.parse::<u16>().map_err(|_| NodeError::ConfigError {
                        reason: "geçersiz p2p-port".into(),
                    })?);
                }
                "--sync" => {
                    i += 1;
                    let mode = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--sync değeri eksik".into(),
                    })?;
                    cli.sync_mode = match *mode {
                        "full" => SyncMode::Full,
                        "light" => SyncMode::Light,
                        "warp" => SyncMode::Warp,
                        _ => return Err(NodeError::ConfigError {
                            reason: format!("geçersiz sync modu: {}", mode),
                        }),
                    };
                }
                "--log" => {
                    i += 1;
                    let level = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--log değeri eksik".into(),
                    })?;
                    cli.log_level = match *level {
                        "error" => LogLevel::Error,
                        "warn" => LogLevel::Warn,
                        "info" => LogLevel::Info,
                        "debug" => LogLevel::Debug,
                        "trace" => LogLevel::Trace,
                        _ => return Err(NodeError::ConfigError {
                            reason: format!("geçersiz log seviyesi: {}", level),
                        }),
                    };
                }
                "--rpc-port" => {
                    i += 1;
                    let port = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--rpc-port değeri eksik".into(),
                    })?;
                    cli.rpc_port = Some(port.parse::<u16>().map_err(|_| NodeError::ConfigError {
                        reason: "geçersiz rpc-port".into(),
                    })?);
                }
                "--data-dir" => {
                    i += 1;
                    let dir = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--data-dir değeri eksik".into(),
                    })?;
                    cli.data_dir = Some(dir.to_string());
                }
                "--boot-nodes" => {
                    i += 1;
                    let nodes = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--boot-nodes değeri eksik".into(),
                    })?;
                    cli.boot_nodes = nodes.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                "--genesis" => {
                    i += 1;
                    let path = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--genesis değeri eksik".into(),
                    })?;
                    cli.genesis_file = Some(path.to_string());
                }
                "--duration" => {
                    i += 1;
                    let secs = args.get(i).ok_or_else(|| NodeError::ConfigError {
                        reason: "--duration değeri eksik".into(),
                    })?;
                    let d = secs.parse::<u64>().map_err(|_| NodeError::ConfigError {
                        reason: "geçersiz duration değeri".into(),
                    })?;
                    cli.command = CliCommand::Benchmark { duration_secs: d };
                }
                "--no-metrics" => { cli.no_metrics = true; }
                "run" => { cli.command = CliCommand::Run; }
                "generate-key" => { cli.command = CliCommand::GenerateKey; }
                "ceremony" => { cli.command = CliCommand::Ceremony { validator_count: 5 }; }
                "node-info" => { cli.command = CliCommand::NodeInfo; }
                "build-genesis" => { cli.command = CliCommand::BuildGenesis; }
                "benchmark" => { cli.command = CliCommand::Benchmark { duration_secs: 10 }; }
                "version" => { cli.command = CliCommand::Version; }
                "print-bootnodes" => { cli.command = CliCommand::PrintBootnodes; }
                _ => {} // bilinmeyen argümanlar yoksayılır
            }
            i += 1;
        }

        Ok(cli)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_dev_default() {
        let args = CliArgs::dev();
        let config = args.to_node_config().unwrap();
        assert!(config.chain.is_dev());
        assert!(config.database.in_memory);
    }

    #[test]
    fn test_cli_parse_dev_flag() {
        let args = CliArgs::parse(&["--dev"]).unwrap();
        assert!(args.dev_mode);
        assert_eq!(args.chain, ChainSpec::Dev);
    }

    #[test]
    fn test_cli_parse_validator_id() {
        let args = CliArgs::parse(&["--dev", "--validator-id", "42"]).unwrap();
        assert_eq!(args.validator_id, Some(42));
    }

    #[test]
    fn test_cli_parse_p2p_port() {
        let args = CliArgs::parse(&["--dev", "--p2p-port", "30334"]).unwrap();
        assert_eq!(args.p2p_port, Some(30334));
    }

    #[test]
    fn test_cli_parse_sync_warp() {
        let args = CliArgs::parse(&["--dev", "--sync", "warp"]).unwrap();
        assert!(args.sync_mode.is_warp());
    }

    #[test]
    fn test_cli_parse_sync_light() {
        let args = CliArgs::parse(&["--dev", "--sync", "light"]).unwrap();
        assert!(args.sync_mode.is_light());
    }

    #[test]
    fn test_cli_parse_log_level() {
        let args = CliArgs::parse(&["--dev", "--log", "debug"]).unwrap();
        assert_eq!(args.log_level, LogLevel::Debug);
    }

    #[test]
    fn test_cli_parse_no_metrics() {
        let args = CliArgs::parse(&["--dev", "--no-metrics"]).unwrap();
        let config = args.to_node_config().unwrap();
        assert!(!config.metrics.enabled);
    }

    #[test]
    fn test_cli_parse_generate_key_command() {
        let args = CliArgs::parse(&["generate-key"]).unwrap();
        assert_eq!(args.command, CliCommand::GenerateKey);
    }

    #[test]
    fn test_cli_parse_invalid_validator_id() {
        let result = CliArgs::parse(&["--validator-id", "not-a-number"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parse_invalid_sync_mode() {
        let result = CliArgs::parse(&["--sync", "turbo"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_to_config_testnet() {
        let args = CliArgs::parse(&["--testnet", "--validator-id", "5"]).unwrap();
        let config = args.to_node_config().unwrap();
        assert_eq!(config.chain, ChainSpec::Testnet);
    }

    #[test]
    fn test_cli_parse_mainnet_flag() {
        let args = CliArgs::parse(&["--mainnet", "--validator-id", "3"]).unwrap();
        assert_eq!(args.chain, ChainSpec::Mainnet);
        assert_eq!(args.validator_id, Some(3));
    }

    #[test]
    fn test_cli_to_config_mainnet() {
        let args = CliArgs::parse(&["--mainnet", "--validator-id", "1"]).unwrap();
        let config = args.to_node_config().unwrap();
        assert_eq!(config.chain, ChainSpec::Mainnet);
        assert_eq!(config.chain.chain_id(), 1984);
        assert!(!config.database.in_memory);
        assert!(config.metrics.enabled);
    }

    #[test]
    fn test_cli_parse_rpc_port() {
        let args = CliArgs::parse(&["--dev", "--rpc-port", "8545"]).unwrap();
        assert_eq!(args.rpc_port, Some(8545));
        let config = args.to_node_config().unwrap();
        assert_eq!(config.rpc.http_port, 8545);
    }

    #[test]
    fn test_cli_parse_data_dir() {
        let args = CliArgs::parse(&["--dev", "--data-dir", "/tmp/nova-test"]).unwrap();
        assert_eq!(args.data_dir, Some("/tmp/nova-test".to_string()));
        let config = args.to_node_config().unwrap();
        assert_eq!(config.database.data_dir, "/tmp/nova-test");
    }

    #[test]
    fn test_cli_parse_genesis_file() {
        let args = CliArgs::parse(&["--dev", "--genesis", "/tmp/genesis.json"]).unwrap();
        assert_eq!(args.genesis_file, Some("/tmp/genesis.json".to_string()));
    }

    #[test]
    fn test_cli_parse_boot_nodes_multiple() {
        let args = CliArgs::parse(&[
            "--testnet",
            "--validator-id", "2",
            "--boot-nodes", "172.20.0.10:30333,172.20.0.11:30333",
        ]).unwrap();
        assert_eq!(args.boot_nodes.len(), 2);
        assert_eq!(args.boot_nodes[0], "172.20.0.10:30333");
        assert_eq!(args.boot_nodes[1], "172.20.0.11:30333");
    }

    #[test]
    fn test_cli_parse_boot_nodes_single() {
        let args = CliArgs::parse(&["--dev", "--boot-nodes", "127.0.0.1:30333"]).unwrap();
        assert_eq!(args.boot_nodes.len(), 1);
        assert_eq!(args.boot_nodes[0], "127.0.0.1:30333");
    }

    #[test]
    fn test_cli_parse_benchmark_command_default_duration() {
        let args = CliArgs::parse(&["benchmark"]).unwrap();
        assert_eq!(args.command, CliCommand::Benchmark { duration_secs: 10 });
    }

    #[test]
    fn test_cli_parse_benchmark_with_duration() {
        let args = CliArgs::parse(&["benchmark", "--duration", "30"]).unwrap();
        assert_eq!(args.command, CliCommand::Benchmark { duration_secs: 30 });
    }

    #[test]
    fn test_cli_parse_run_command_explicit() {
        let args = CliArgs::parse(&["run", "--dev"]).unwrap();
        assert_eq!(args.command, CliCommand::Run);
        assert!(args.dev_mode);
    }

    #[test]
    fn test_cli_parse_invalid_rpc_port() {
        let result = CliArgs::parse(&["--rpc-port", "not-a-port"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_genesis_file_default_none() {
        let args = CliArgs::default();
        assert!(args.genesis_file.is_none());
    }

    #[test]
    fn test_cli_parse_print_bootnodes_command() {
        let args = CliArgs::parse(&["print-bootnodes", "--mainnet"]).unwrap();
        assert_eq!(args.command, CliCommand::PrintBootnodes);
        assert_eq!(args.chain, ChainSpec::Mainnet);
    }

    #[test]
    fn test_cli_parse_print_bootnodes_testnet() {
        let args = CliArgs::parse(&["print-bootnodes", "--testnet"]).unwrap();
        assert_eq!(args.command, CliCommand::PrintBootnodes);
        assert_eq!(args.chain, ChainSpec::Testnet);
    }

    #[test]
    fn test_cli_parse_print_bootnodes_dev_default() {
        let args = CliArgs::parse(&["print-bootnodes"]).unwrap();
        assert_eq!(args.command, CliCommand::PrintBootnodes);
        // varsayılan zincir Dev
        assert_eq!(args.chain, ChainSpec::Dev);
    }
}
