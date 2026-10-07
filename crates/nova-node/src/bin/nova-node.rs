//! Nova Shield Chain — Validator Node binary
//!
//! Kullanım:
//!   nova-node --dev
//!   nova-node --chain mainnet --validator-id 1 --data-dir /data/nova
//!   nova-node generate-key --validator-id 1 --data-dir /data/nova
//!   nova-node version

use nova_node::{
    bootnodes::{bootnode_infos_for_chain, bootnodes_for_chain},
    genesis::{devnet_genesis, mainnet_genesis, testnet_genesis, GenesisConfig},
    keystore::Keystore,
    node::AsyncNodeRunner,
    upgrade::UpgradeManager,
    CliArgs, CliCommand, NODE_VERSION, NOVA_PROTOCOL_VERSION,
};
use nova_primitives::{Address, Balance, ValidatorId};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = run(&args[1..]) {
        eprintln!("HATA: {e}");
        std::process::exit(1);
    }
}

fn run(raw_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let str_args: Vec<&str> = raw_args.iter().map(|s| s.as_str()).collect();
    let cli = CliArgs::parse(&str_args)?;

    match &cli.command {
        CliCommand::Version => {
            println!("nova-node {NODE_VERSION}");
            println!("protocol: v{NOVA_PROTOCOL_VERSION}");
        }

        CliCommand::PrintBootnodes => {
            let chain_id = match &cli.chain {
                nova_node::config::ChainSpec::Mainnet => nova_primitives::constants::MAINNET_CHAIN_ID,
                nova_node::config::ChainSpec::Testnet => nova_primitives::constants::TESTNET_CHAIN_ID,
                _ => nova_primitives::constants::DEVNET_CHAIN_ID,
            };
            let infos = bootnode_infos_for_chain(chain_id);
            println!("=== Nova Shield Chain — Bootnode Listesi ===");
            println!("chain_id : {chain_id}");
            println!("toplam   : {}", infos.len());
            println!();
            for info in &infos {
                println!("  {info}");
            }
        }

        CliCommand::NodeInfo => {
            let config = cli.to_node_config()?;
            println!("=== Nova Shield Node Info ===");
            println!("chain    : {:?}", config.chain);
            println!("p2p port : {}", config.network.p2p_port);
            println!("rpc port : {}", config.rpc.http_port);
            println!("validator: {}", config.validator.enabled);
        }

        CliCommand::GenerateKey => {
            let validator_id = cli.validator_id.unwrap_or(1);
            let data_dir = cli
                .data_dir
                .clone()
                .unwrap_or_else(|| format!("/tmp/nova-data/validator-{validator_id}"));
            let keystore_dir = format!("{data_dir}/keystore");

            eprintln!("Validator anahtarları üretiliyor (id={validator_id})...");
            let mut ks = Keystore::load(&keystore_dir)?;
            let keys = ks.generate(validator_id)?;

            println!("=== Validator Anahtarları ===");
            println!("validator_id : {}", keys.validator_id);
            println!("node_key     : {}", keys.node_key);
            println!("bls_pubkey   : {}", keys.bls_pubkey);
            println!("ecdsa_pubkey : {}", keys.ecdsa_pubkey);
            println!("address      : {}", keys.address);
            println!("keystore_dir : {keystore_dir}");
            eprintln!("OK: anahtarlar kaydedildi.");
        }

        CliCommand::Ceremony { validator_count } => {
            let n = *validator_count as u64;
            let out_dir = cli
                .data_dir
                .clone()
                .unwrap_or_else(|| "/tmp/nova-ceremony".to_string());

            eprintln!("=== Nova Shield Chain — Genesis Ceremony ===");
            eprintln!("Validator sayısı : {n}");
            eprintln!("Çıktı dizini     : {out_dir}");
            eprintln!("");

            std::fs::create_dir_all(&out_dir)?;

            // Her validator için key üret
            let mut validator_keys = Vec::new();
            for id in 1..=n {
                let keystore_dir = format!("{out_dir}/validator-{id}/keystore");
                let mut ks = Keystore::load(&keystore_dir)?;
                let keys = ks.generate(id)?;
                eprintln!("  [v{id}] address: {} bls: {}...", keys.address, &keys.bls_pubkey[..14]);
                validator_keys.push(keys);
            }

            // Genesis'i bu keylerle doldur
            let mut genesis = mainnet_genesis();
            let one_million = "1000000000000000000000000";

            genesis.validators = validator_keys
                .iter()
                .map(|k| nova_node::genesis::GenesisValidator {
                    id: k.validator_id,
                    address: k.address.clone(),
                    bls_pubkey: k.bls_pubkey.clone(),
                    p2p_key: k.node_key.clone(),
                    stake: one_million.to_string(),
                    commission_bps: 500,
                    endpoint: format!("VALIDATOR_{}_ENDPOINT:30333", k.validator_id),
                })
                .collect();

            genesis.mpc.keypers = validator_keys.iter().map(|k| k.address.clone()).collect();

            genesis.validate()?;
            let genesis_hash = genesis.hash_hex();

            // Genesis dosyasını yaz
            let genesis_path = format!("{out_dir}/genesis.json");
            let genesis_json = serde_json::to_string_pretty(&genesis)?;
            std::fs::write(&genesis_path, &genesis_json)?;

            // Ceremony report yaz
            let report = serde_json::json!({
                "ceremony": "nova-mainnet-genesis-v1",
                "chain_id": genesis.chain_id,
                "chain_name": genesis.chain_name,
                "genesis_time": genesis.genesis_time,
                "genesis_hash": genesis_hash,
                "validator_count": n,
                "validators": validator_keys.iter().map(|k| serde_json::json!({
                    "id": k.validator_id,
                    "address": k.address,
                    "bls_pubkey": k.bls_pubkey,
                })).collect::<Vec<_>>(),
                "token": genesis.token.symbol,
                "total_supply": genesis.token.total_supply,
                "genesis_file": genesis_path,
            });
            let report_path = format!("{out_dir}/ceremony_report.json");
            std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;

            println!("=== Ceremony Tamamlandı ===");
            println!("chain_id      : {}", genesis.chain_id);
            println!("genesis_hash  : {genesis_hash}");
            println!("validator_count: {n}");
            println!("genesis_file  : {genesis_path}");
            println!("report_file   : {report_path}");
            for k in &validator_keys {
                println!("  v{}: {} | bls: {}...", k.validator_id, k.address, &k.bls_pubkey[..14]);
            }
        }

        CliCommand::BuildGenesis => {
            let genesis: GenesisConfig = match &cli.chain {
                nova_node::config::ChainSpec::Mainnet => mainnet_genesis(),
                nova_node::config::ChainSpec::Testnet => testnet_genesis(),
                _ => devnet_genesis(),
            };
            genesis.validate()?;
            let json = serde_json::to_string_pretty(&genesis)?;

            let out_path = cli
                .data_dir
                .as_deref()
                .map(|d| format!("{d}/genesis.json"))
                .unwrap_or_else(|| "genesis.json".to_string());

            std::fs::write(&out_path, json)?;
            println!("Genesis oluşturuldu: {out_path}");
            println!("  chain_id   : {}", genesis.chain_id);
            println!("  validators : {}", genesis.validators.len());
            println!("  hash       : {}", genesis.hash_hex());
        }

        CliCommand::Benchmark { duration_secs } => {
            use nova_node::{node::NodeBuilder, NodeConfig};
            eprintln!("Benchmark {duration_secs}s çalıştırılıyor...");
            let config = NodeConfig::dev();
            let mut node = NodeBuilder::new(config)?.build();
            node.start()?;

            let mut blocks = 0u64;
            let start = std::time::Instant::now();
            let duration = std::time::Duration::from_secs(*duration_secs);

            while start.elapsed() < duration {
                let hash = node.consensus_mut().produce_block()?;
                node.consensus_mut().finalize_block(hash)?;
                blocks += 1;
            }
            node.stop()?;

            let elapsed = start.elapsed().as_secs_f64();
            println!("=== Benchmark Sonuçları ===");
            println!("Bloklar    : {blocks}");
            println!("Süre       : {elapsed:.2}s");
            println!("BPS        : {:.2}", blocks as f64 / elapsed);
        }

        CliCommand::Run => {
            let config = cli.to_node_config()?;
            let validator_id = config.validator.validator_id;
            let max_peers = config.network.max_peers as usize;
            let listen_port = config.network.p2p_port;
            let data_dir = cli
                .data_dir
                .clone()
                .unwrap_or_else(|| "/tmp/nova-data".to_string());

            // Upgrade manager
            let upgrade_path = format!("{data_dir}/upgrade_state.json");
            let _upgrade_mgr = UpgradeManager::load(&upgrade_path)?;

            // Keystore
            let keystore_dir = format!("{data_dir}/keystore");
            let mut ks = Keystore::load(&keystore_dir)?;
            if !ks.is_ready() {
                eprintln!("Anahtar bulunamadı, yeni anahtar üretiliyor...");
                ks.generate(validator_id)?;
            }
            let local_key = ks
                .keys()
                .and_then(|k| k.node_key_bytes())
                .unwrap_or([0u8; 32]);

            // Genesis: --genesis dosyasından yükle, yoksa zincir tipine göre varsayılan
            let genesis = if let Some(ref gf) = cli.genesis_file {
                eprintln!("Genesis dosyasından yükleniyor: {gf}");
                GenesisConfig::from_file(gf)?
            } else {
                match &cli.chain {
                    nova_node::config::ChainSpec::Testnet => testnet_genesis(),
                    nova_node::config::ChainSpec::Mainnet => mainnet_genesis(),
                    _ => devnet_genesis(),
                }
            };
            genesis.validate()?;

            eprintln!("Nova Shield Chain başlatılıyor...");
            eprintln!("  chain    : {:?}", config.chain);
            eprintln!("  chain_id : {}", genesis.chain_id);
            eprintln!("  p2p port : {listen_port}");
            eprintln!("  rpc port : {}", config.rpc.http_port);
            eprintln!("  data_dir : {data_dir}");
            eprintln!("  genesis  : {}", genesis.hash_hex());

            // Validator seti genesis'ten
            let validators: Vec<(ValidatorId, Address, Balance)> = genesis
                .validators
                .iter()
                .map(|v| {
                    let mut addr_bytes = [0u8; 20];
                    let hex_str = v.address.trim_start_matches("0x");
                    if let Ok(bytes) = hex::decode(hex_str) {
                        let len = bytes.len().min(20);
                        addr_bytes[..len].copy_from_slice(&bytes[..len]);
                    }
                    let stake: u128 = v.stake.parse().unwrap_or(0);
                    (ValidatorId(v.id), Address::from(addr_bytes), Balance(stake))
                })
                .collect();

            // Boot nodes öncelik sırası:
            //   1. CLI --boot-nodes (açıkça verilmişse)
            //   2. Genesis'teki diğer validator endpoint'leri (lokal ağ / docker)
            //   3. Binary'ye gömülü well-known bootnode listesi (mainnet/testnet)
            let boot_nodes: Vec<String> = if !cli.boot_nodes.is_empty() {
                eprintln!("  boot nodes: CLI'den alındı");
                cli.boot_nodes.clone()
            } else {
                // Genesis endpoint'leri gerçek IP içeriyorsa kullan
                let genesis_endpoints: Vec<String> = genesis
                    .validators
                    .iter()
                    .filter(|v| v.id != validator_id)
                    .map(|v| v.endpoint.clone())
                    .filter(|ep| {
                        // Placeholder endpoint'leri filtrele (VALIDATOR_N_ENDPOINT:port formatı)
                        !ep.starts_with("VALIDATOR_")
                    })
                    .collect();

                if !genesis_endpoints.is_empty() {
                    eprintln!("  boot nodes: genesis'ten alındı");
                    genesis_endpoints
                } else {
                    // Fallback: binary'ye gömülü well-known bootnode listesi
                    let embedded = bootnodes_for_chain(genesis.chain_id);
                    if !embedded.is_empty() {
                        eprintln!("  boot nodes: embedded well-known listesi kullanılıyor");
                    }
                    embedded
                }
            };

            if !boot_nodes.is_empty() {
                eprintln!("  boot nodes ({}):", boot_nodes.len());
                for bn in &boot_nodes {
                    eprintln!("    {bn}");
                }
            }

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;

            let rpc_port = config.rpc.http_port;
            let in_memory = config.database.in_memory;
            rt.block_on(async move {
                let mut runner = if in_memory {
                    AsyncNodeRunner::new(
                        validator_id, validators, local_key, max_peers, listen_port, rpc_port,
                    )?
                } else {
                    AsyncNodeRunner::new_with_data_dir(
                        validator_id, validators, local_key, max_peers, listen_port, rpc_port,
                        &data_dir,
                    )?
                };
                if !boot_nodes.is_empty() {
                    runner = runner.with_boot_nodes(boot_nodes);
                }
                runner.run(400, None).await
            })?;
        }
    }

    Ok(())
}
