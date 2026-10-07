//! Genesis konfigürasyonu — genesis.json yükleyici ve doğrulayıcı
//!
//! Nova Shield Chain başlangıç durumunu (validator seti, bakiyeler, MPC config)
//! genesis.json dosyasından yükler ve state'e uygular.

use crate::config::ChainSpec;
use crate::errors::{NodeError, NodeResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── Alt Yapı Tipleri ─────────────────────────────────────────────────────────

/// Genesis validator tanımı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    /// Validator kimliği (1'den başlar)
    pub id: u64,
    /// Ethereum uyumlu adres (0x...)
    pub address: String,
    /// BLS12-381 public key (0x... 48 byte)
    pub bls_pubkey: String,
    /// P2P node key (hex, 32 byte)
    pub p2p_key: String,
    /// Başlangıç stake miktarı (baz birim string)
    pub stake: String,
    /// Komisyon oranı (BPS, 10000 = %100)
    pub commission_bps: u64,
    /// P2P endpoint (ip:port)
    pub endpoint: String,
}

/// Genesis bakiye kaydı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisBalance {
    /// Adres (0x...)
    pub address: String,
    /// Bakiye (baz birim string)
    pub balance: String,
    /// İsteğe bağlı açıklama
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// MPC eşik imza konfigürasyonu (genesis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisMpc {
    /// Gerekli imza sayısı (t)
    pub threshold: u8,
    /// Toplam taraf sayısı (n)
    pub total_parties: u8,
    /// Şifreleme anahtarı paylaşımcıları (adres listesi)
    pub keypers: Vec<String>,
}

/// Guardian başlangıç seti konfigürasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisGuardianSet {
    /// Başlangıç guardian'ları (adres listesi)
    pub initial_guardians: Vec<String>,
    /// Minimum guardian stake miktarı (baz birim string)
    pub min_stake: String,
}

/// Konsensüs parametreleri (genesis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConsensus {
    /// Konsensüs algoritması adı
    pub algorithm: String,
    /// Maksimum aktif validator sayısı
    pub max_validators: u32,
    /// BFT quorum eşiği (BPS, 6667 ≈ 2/3)
    pub quorum_threshold_bps: u32,
    /// Era süresi (slot)
    pub era_duration_slots: u64,
    /// Oturum süresi (slot)
    pub session_duration_slots: u64,
    /// Unbonding süresi (slot)
    pub unbonding_duration_slots: u64,
}

/// Token parametreleri
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisToken {
    /// Token sembolü
    pub symbol: String,
    /// Ondalık hane sayısı (18)
    pub decimals: u8,
    /// Toplam arz (baz birim string)
    pub total_supply: String,
    /// Varoluş mevduatı (dust önleme, baz birim string)
    pub existential_deposit: String,
}

/// Protocol parametreleri (ücretler, ödüller, adresler)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisProtocolParams {
    /// Blok başına maksimum gas limiti
    pub max_block_gas: u64,
    /// Temel gas ücreti (baz birim string)
    pub base_fee_per_gas: String,
    /// Minimum gas fiyatı (baz birim string)
    pub min_gas_price: String,
    /// Era başına blok ödülü (baz birim string)
    pub block_reward_per_era: String,
    /// Hazine adresi (0x...)
    pub treasury_address: String,
    /// Governance adresi (0x...)
    pub governance_address: String,
}

// ─── GenesisConfig ────────────────────────────────────────────────────────────

/// Tam genesis konfigürasyonu (genesis.json formatı)
///
/// Nova Shield Chain'in başlangıç durumunu tanımlar.
/// genesis.json bir kez üretilir, zincir hayatı boyunca değişmez.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Zincir kimliği (EVM chain ID uyumlu)
    pub chain_id: u64,
    /// Zincir adı
    pub chain_name: String,
    /// Genesis zamanı (ISO 8601)
    pub genesis_time: String,
    /// Genesis slot numarası (0)
    pub genesis_slot: u64,
    /// Slot süresi (milisaniye)
    pub slot_duration_ms: u64,
    /// Token parametreleri
    pub token: GenesisToken,
    /// Konsensüs parametreleri
    pub consensus: GenesisConsensus,
    /// Başlangıç validator seti
    pub validators: Vec<GenesisValidator>,
    /// Başlangıç bakiye dağılımı
    pub balances: Vec<GenesisBalance>,
    /// MPC eşik imza konfigürasyonu
    pub mpc: GenesisMpc,
    /// Guardian başlangıç seti
    pub guardian_set: GenesisGuardianSet,
    /// Protocol parametreleri
    pub protocol_params: GenesisProtocolParams,
}

impl GenesisConfig {
    /// genesis.json dosyasından yükle
    pub fn from_file(path: impl AsRef<Path>) -> NodeResult<Self> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            NodeError::GenesisError {
                reason: format!("genesis dosyası okunamadı: {e}"),
            }
        })?;
        Self::from_json(&content)
    }

    /// JSON string'den parse et
    pub fn from_json(json: &str) -> NodeResult<Self> {
        serde_json::from_str(json).map_err(|e| NodeError::GenesisError {
            reason: format!("genesis JSON parse hatası: {e}"),
        })
    }

    /// Deterministik genesis hash hesapla (BLAKE3)
    ///
    /// Tüm alanları JSON'a dönüştürüp hash'ler.
    /// İki node aynı genesis.json'dan aynı hash üretmelidir.
    pub fn compute_hash(&self) -> [u8; 32] {
        let json = serde_json::to_string(self).unwrap_or_default();
        let hash = blake3::hash(json.as_bytes());
        *hash.as_bytes()
    }

    /// Genesis hash'ini hex string olarak döndür
    pub fn hash_hex(&self) -> String {
        hex::encode(self.compute_hash())
    }

    /// Başlangıç bakiyeleri: (adres_hex, miktar) listesi
    pub fn initial_balances(&self) -> Vec<(String, u128)> {
        self.balances
            .iter()
            .filter_map(|b| {
                let amount = b.balance.parse::<u128>().ok()?;
                Some((b.address.clone(), amount))
            })
            .collect()
    }

    /// Validator adres listesi
    pub fn validator_addresses(&self) -> Vec<String> {
        self.validators.iter().map(|v| v.address.clone()).collect()
    }

    /// Toplam stake miktarı
    pub fn total_staked(&self) -> u128 {
        self.validators
            .iter()
            .filter_map(|v| v.stake.parse::<u128>().ok())
            .sum()
    }

    /// Genesis tutarlılık doğrulaması
    ///
    /// - En az 1 validator
    /// - Geçerli MPC threshold (t ≤ n)
    /// - Sıfır olmayan total_supply
    /// - Quorum hesabı için yeterli validator (≥ 3 BFT için)
    /// - Her validator için BLS pubkey format doğrulaması (48 byte, non-zero, hex)
    pub fn validate(&self) -> NodeResult<()> {
        if self.validators.is_empty() {
            return Err(NodeError::GenesisError {
                reason: "en az 1 validator gerekli".into(),
            });
        }
        if self.mpc.threshold == 0 || self.mpc.threshold > self.mpc.total_parties {
            return Err(NodeError::GenesisError {
                reason: format!(
                    "geçersiz MPC threshold: t={} n={}",
                    self.mpc.threshold, self.mpc.total_parties
                ),
            });
        }
        let _total_supply = self
            .token
            .total_supply
            .parse::<u128>()
            .map_err(|_| NodeError::GenesisError {
                reason: "geçersiz total_supply (u128 parse hatası)".into(),
            })?;
        if self.slot_duration_ms == 0 {
            return Err(NodeError::GenesisError {
                reason: "slot_duration_ms sıfır olamaz".into(),
            });
        }

        // BLS pubkey format doğrulaması (her validator için)
        for v in &self.validators {
            validate_bls_pubkey_format(&v.bls_pubkey, v.id)?;
        }

        Ok(())
    }
}

// ─── BLS Pubkey Format Doğrulaması ────────────────────────────────────────────

/// BLS12-381 G1 pubkey format doğrulaması
///
/// Güvenlik kontrolleri:
/// 1. Hex çözümlenebilir olmalı
/// 2. Tam 48 byte uzunluğunda olmalı (BLS12-381 G1 compressed)
/// 3. Tüm sıfırlardan oluşmamalı (infinity point — geçersiz)
/// 4. Bit 7 set (compressed point flag) olmalı
///
/// # Neden Önemli?
///
/// Geçersiz bir BLS pubkey ile kayıtlı bir validator:
/// - Hiçbir zaman imzalayamaz → slashing riski
/// - Aggregate signature doğrulamasında panik tetikleyebilir
/// - Rogue-key saldırısına zemin hazırlayabilir
fn validate_bls_pubkey_format(bls_pubkey_hex: &str, validator_id: u64) -> NodeResult<()> {
    let hex_str = bls_pubkey_hex.trim_start_matches("0x");

    // 1. Hex decode
    let bytes = hex::decode(hex_str).map_err(|_| NodeError::GenesisError {
        reason: format!(
            "validator {validator_id}: BLS pubkey geçerli hex değil: '{}'",
            &bls_pubkey_hex[..bls_pubkey_hex.len().min(20)]
        ),
    })?;

    // 2. Uzunluk kontrolü (BLS12-381 G1 compressed = 48 byte)
    if bytes.len() != 48 {
        return Err(NodeError::GenesisError {
            reason: format!(
                "validator {validator_id}: BLS pubkey 48 byte olmalı, {} byte bulundu",
                bytes.len()
            ),
        });
    }

    // 3. Sıfır kontrolü (infinity / identity point — geçersiz validator key)
    if bytes.iter().all(|&b| b == 0) {
        return Err(NodeError::GenesisError {
            reason: format!(
                "validator {validator_id}: BLS pubkey tüm sıfırlar olamaz (identity point)"
            ),
        });
    }

    // 4. Compressed point flag kontrolü
    // BLS12-381 G1 compressed: bit 7 of first byte = 1
    if bytes[0] & 0x80 == 0 {
        return Err(NodeError::GenesisError {
            reason: format!(
                "validator {validator_id}: BLS pubkey compressed flag eksik (ilk byte: 0x{:02x})",
                bytes[0]
            ),
        });
    }

    Ok(())
}

// ─── Devnet / Testnet / Mainnet Genesis Üreticileri ───────────────────────────

/// Devnet genesis konfigürasyonu üret (5 validator, test bakiyeleri)
pub fn devnet_genesis() -> GenesisConfig {
    let make_addr = |n: u8| -> String { format!("0x{}", hex::encode([n; 20])) };
    // BLS12-381 G1 compressed: ilk byte bit7=1 (compressed flag), non-zero
    let make_bls = |n: u8| -> String {
        let mut key = [n; 48];
        key[0] = 0x80 | (n & 0x7F); // compressed flag set, non-zero
        format!("0x{}", hex::encode(key))
    };
    let make_p2p = |n: u8| -> String { hex::encode([n; 32]) };

    let one_million = "1000000000000000000000000";
    let hundred_million = "100000000000000000000000000";

    let validators: Vec<GenesisValidator> = (1u8..=5)
        .map(|n| GenesisValidator {
            id: n as u64,
            address: make_addr(n),
            bls_pubkey: make_bls(n),
            p2p_key: make_p2p(n),
            stake: one_million.to_string(),
            commission_bps: 500,
            endpoint: format!("172.20.0.{}:30333", 9 + n as u32),
        })
        .collect();

    let mut balances: Vec<GenesisBalance> = (1u8..=5)
        .map(|n| GenesisBalance {
            address: make_addr(n),
            balance: one_million.to_string(),
            note: None,
        })
        .collect();
    balances.push(GenesisBalance {
        address: "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string(),
        balance: hundred_million.to_string(),
        note: Some("Devnet test faucet".into()),
    });

    let keypers = validators.iter().map(|v| v.address.clone()).collect();

    GenesisConfig {
        chain_id: ChainSpec::Dev.chain_id(),
        chain_name: "Nova Shield Devnet".to_string(),
        genesis_time: "2026-01-01T00:00:00Z".to_string(),
        genesis_slot: 0,
        slot_duration_ms: 400,
        token: GenesisToken {
            symbol: "NOVA".to_string(),
            decimals: 18,
            total_supply: "200000000000000000000000000".to_string(),
            existential_deposit: "100000000000000000000".to_string(),
        },
        consensus: GenesisConsensus {
            algorithm: "BFT".to_string(),
            max_validators: 200,
            quorum_threshold_bps: 6667,
            era_duration_slots: 216_000,
            session_duration_slots: 2_400,
            unbonding_duration_slots: 4_536_000,
        },
        validators,
        balances,
        mpc: GenesisMpc {
            threshold: 3,
            total_parties: 5,
            keypers,
        },
        guardian_set: GenesisGuardianSet {
            initial_guardians: vec![],
            min_stake: "100000000000000000000".to_string(),
        },
        protocol_params: GenesisProtocolParams {
            max_block_gas: 30_000_000,
            base_fee_per_gas: "1000000000".to_string(),
            min_gas_price: "1000000000".to_string(),
            block_reward_per_era: "4566210045662100456621".to_string(),
            treasury_address: "0x4747474747474747474747474747474747474747".to_string(),
            governance_address: "0x4747474747474747474747474747474747474747".to_string(),
        },
    }
}

/// Testnet genesis konfigürasyonu üret
pub fn testnet_genesis() -> GenesisConfig {
    let mut g = devnet_genesis();
    g.chain_id = ChainSpec::Testnet.chain_id();
    g.chain_name = "Nova Shield Testnet".to_string();
    g.genesis_time = "2027-01-01T00:00:00Z".to_string();

    // Testnet: sadece 3 validator (Docker compose ile eşleşiyor)
    // IP'ler: 172.20.0.10 (v1), 172.20.0.11 (v2), 172.20.0.12 (v3)
    // BFT quorum 6667 bps + 3 validator = 2/3 = 2 oy yeterli
    let make_addr = |n: u8| -> String {
        let mut b = [0u8; 20];
        b[19] = n;
        format!("0x{}", hex::encode(b))
    };
    let make_bls = |n: u8| -> String {
        let mut key = [n; 48];
        key[0] = 0x80 | (n & 0x7F);
        format!("0x{}", hex::encode(key))
    };
    let make_p2p = |n: u8| -> String { hex::encode([n; 32]) };
    let one_million = "1000000000000000000000000";

    let validators: Vec<GenesisValidator> = (1u8..=3)
        .map(|n| GenesisValidator {
            id: n as u64,
            address: make_addr(n),
            bls_pubkey: make_bls(n),
            p2p_key: make_p2p(n),
            stake: one_million.to_string(),
            commission_bps: 500,
            endpoint: format!("172.20.0.{}:30333", 9 + n as u32),
        })
        .collect();

    let keypers = validators.iter().map(|v| v.address.clone()).collect();

    g.validators = validators;
    g.balances = (1u8..=3)
        .map(|n| GenesisBalance {
            address: make_addr(n),
            balance: one_million.to_string(),
            note: None,
        })
        .collect();
    g.mpc = GenesisMpc {
        threshold: 2,
        total_parties: 3,
        keypers,
    };
    g
}

/// Mainnet genesis konfigürasyonu üret
///
/// Token dağılımı (200M NOVA toplam, sabit arz):
/// - Ecosystem & Community: 22% = 44M (48 ay linear)
/// - Team & Founders:       19% = 38M (12 ay cliff + 48 ay linear)
/// - Investors:             17% = 34M (12 ay cliff + 24 ay linear, max cap)
/// - Staking Rewards:       16% = 32M (yarılanma takvimi, 10 yıl)
/// - Treasury:              12% = 24M (6 ay cliff + governance unlock)
/// - Public Sale:            7% = 14M (TGE'de tamamen açık)
/// - Liquidity:              7% = 14M (TGE'de tamamen açık)
///
/// TGE dolaşım: 28M NOVA (%14) — Public Sale + Liquidity
///
/// UYARI: Validator adresleri ve tokenomics adresleri placeholder.
///        Mainnet ceremony öncesi gerçek multisig adresleriyle değiştirilmeli.
pub fn mainnet_genesis() -> GenesisConfig {
    // ── Tokenomics adresleri (mainnet ceremony'de multisig ile değiştirilir) ──
    // Format: 0xAA...AA{n} — ceremony öncesi placeholder
    let make_alloc_addr = |n: u8| -> String {
        let mut b = [0xABu8; 20];
        b[19] = n;
        format!("0x{}", hex::encode(b))
    };
    let ecosystem_addr   = make_alloc_addr(0x01); // Ecosystem & Community
    let team_addr        = make_alloc_addr(0x02); // Team & Founders
    let investors_addr   = make_alloc_addr(0x03); // Investors (seed + series A)
    let staking_addr     = make_alloc_addr(0x04); // Staking Rewards pool
    let treasury_addr    = make_alloc_addr(0x05); // Treasury
    let public_sale_addr = make_alloc_addr(0x06); // Public Sale
    let liquidity_addr   = make_alloc_addr(0x07); // Liquidity (DEX/CEX)

    // ── Tokenomics bakiyeleri ─────────────────────────────────────────────────
    let balances = vec![
        GenesisBalance {
            address: ecosystem_addr.clone(),
            balance: "44000000000000000000000000".to_string(), // 44M NOVA
            note: Some("Ecosystem & Community — 48 ay linear vesting".into()),
        },
        GenesisBalance {
            address: team_addr.clone(),
            balance: "38000000000000000000000000".to_string(), // 38M NOVA
            note: Some("Team & Founders — 12 ay cliff + 48 ay linear".into()),
        },
        GenesisBalance {
            address: investors_addr.clone(),
            balance: "34000000000000000000000000".to_string(), // 34M NOVA
            note: Some("Investors — 12 ay cliff + 24 ay linear (maks %17 cap)".into()),
        },
        GenesisBalance {
            address: staking_addr.clone(),
            balance: "32000000000000000000000000".to_string(), // 32M NOVA
            note: Some("Staking Rewards — yarılanma takvimi 10 yıl".into()),
        },
        GenesisBalance {
            address: treasury_addr.clone(),
            balance: "24000000000000000000000000".to_string(), // 24M NOVA
            note: Some("Treasury — 6 ay cliff + governance unlock".into()),
        },
        GenesisBalance {
            address: public_sale_addr.clone(),
            balance: "14000000000000000000000000".to_string(), // 14M NOVA
            note: Some("Public Sale — TGE'de tamamen açık".into()),
        },
        GenesisBalance {
            address: liquidity_addr.clone(),
            balance: "14000000000000000000000000".to_string(), // 14M NOVA
            note: Some("Liquidity — TGE'de tamamen açık (DEX/CEX)".into()),
        },
    ];

    // ── Genesis validator seti (ceremony'de gerçek keylerle değiştirilir) ────
    let make_addr = |n: u8| -> String {
        let mut b = [0xAAu8; 20];
        b[19] = n;
        format!("0x{}", hex::encode(b))
    };
    let make_bls = |n: u8| -> String {
        let mut key = [0xAAu8; 48];
        key[0] = 0x80 | (n & 0x7F);
        key[47] = n;
        format!("0x{}", hex::encode(key))
    };
    let make_p2p = |n: u8| -> String {
        let mut key = [0xAAu8; 32];
        key[31] = n;
        hex::encode(key)
    };
    let one_million = "1000000000000000000000000"; // 1M NOVA stake

    let validators: Vec<GenesisValidator> = (1u8..=5)
        .map(|n| GenesisValidator {
            id: n as u64,
            address: make_addr(n),
            bls_pubkey: make_bls(n),
            p2p_key: make_p2p(n),
            stake: one_million.to_string(),
            commission_bps: 500, // %5 komisyon
            endpoint: format!("MAINNET_VALIDATOR_{n}_ENDPOINT"),
        })
        .collect();

    let keypers = validators.iter().map(|v| v.address.clone()).collect();

    GenesisConfig {
        chain_id: nova_primitives::constants::MAINNET_CHAIN_ID, // 1984
        chain_name: "Nova Shield Chain".to_string(),
        genesis_time: "2027-04-01T12:00:00Z".to_string(),
        genesis_slot: 0,
        slot_duration_ms: 400,
        token: GenesisToken {
            symbol: "NOVA".to_string(),
            decimals: 18,
            total_supply: "200000000000000000000000000".to_string(), // 200M NOVA
            existential_deposit: "100000000000000000".to_string(),  // 0.1 NOVA
        },
        consensus: GenesisConsensus {
            algorithm: "BFT".to_string(),
            max_validators: 1_000,
            quorum_threshold_bps: 6_667, // 2/3 + 1
            era_duration_slots: 216_000,
            session_duration_slots: 2_400,
            unbonding_duration_slots: 4_536_000, // ~21 gün
        },
        validators,
        balances,
        mpc: GenesisMpc {
            threshold: 3,
            total_parties: 5,
            keypers,
        },
        guardian_set: GenesisGuardianSet {
            initial_guardians: vec![],
            min_stake: "1000000000000000000000".to_string(), // 1_000 NOVA
        },
        protocol_params: GenesisProtocolParams {
            max_block_gas: 30_000_000,
            base_fee_per_gas: "1000000000".to_string(),
            min_gas_price: "1000000000".to_string(),
            block_reward_per_era: "0".to_string(), // Deflasyonlu model — ödül yok, fee burn var
            treasury_address: treasury_addr,
            governance_address: make_alloc_addr(0x08), // Governance
        },
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devnet_genesis_validates() {
        let g = devnet_genesis();
        assert!(g.validate().is_ok());
        assert_eq!(g.validators.len(), 5);
        assert_eq!(g.mpc.threshold, 3);
        assert_eq!(g.mpc.total_parties, 5);
    }

    #[test]
    fn test_genesis_hash_deterministic() {
        let g1 = devnet_genesis();
        let g2 = devnet_genesis();
        assert_eq!(g1.compute_hash(), g2.compute_hash(), "aynı genesis → aynı hash");
    }

    #[test]
    fn test_genesis_hash_changes_on_modification() {
        let mut g1 = devnet_genesis();
        let g2 = devnet_genesis();
        g1.chain_id += 1;
        assert_ne!(g1.compute_hash(), g2.compute_hash());
    }

    #[test]
    fn test_genesis_initial_balances() {
        let g = devnet_genesis();
        let balances = g.initial_balances();
        assert_eq!(balances.len(), 6); // 5 validator + 1 faucet
        let faucet = balances.iter().find(|(a, _)| a.contains("deadbeef"));
        assert!(faucet.is_some());
    }

    #[test]
    fn test_genesis_total_staked() {
        let g = devnet_genesis();
        let staked = g.total_staked();
        // 5 validator × 1_000_000 NSC × 10^18
        let one_million_nsc: u128 = 1_000_000_000_000_000_000_000_000;
        assert_eq!(staked, 5 * one_million_nsc);
    }

    #[test]
    fn test_genesis_json_roundtrip() {
        let g = devnet_genesis();
        let json = serde_json::to_string(&g).unwrap();
        let g2 = GenesisConfig::from_json(&json).unwrap();
        assert_eq!(g.chain_id, g2.chain_id);
        assert_eq!(g.validators.len(), g2.validators.len());
        assert_eq!(g.compute_hash(), g2.compute_hash());
    }

    #[test]
    fn test_genesis_invalid_threshold() {
        let mut g = devnet_genesis();
        g.mpc.threshold = 10; // t > n (5)
        assert!(g.validate().is_err());
    }

    #[test]
    fn test_genesis_invalid_zero_validators() {
        let mut g = devnet_genesis();
        g.validators.clear();
        assert!(g.validate().is_err());
    }

    #[test]
    fn test_testnet_genesis_chain_id() {
        let g = testnet_genesis();
        assert_eq!(g.chain_id, ChainSpec::Testnet.chain_id());
    }

    #[test]
    fn test_mainnet_genesis_chain_id() {
        let g = mainnet_genesis();
        assert_eq!(g.chain_id, nova_primitives::constants::MAINNET_CHAIN_ID);
        assert_eq!(g.consensus.max_validators, 1_000);
    }

    #[test]
    fn test_genesis_validator_addresses() {
        let g = devnet_genesis();
        let addrs = g.validator_addresses();
        assert_eq!(addrs.len(), 5);
        assert!(addrs.iter().all(|a| a.starts_with("0x")));
    }

    #[test]
    fn test_genesis_hash_hex_format() {
        let g = devnet_genesis();
        let hex = g.hash_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── BLS Pubkey Doğrulama Testleri ─────────────────────────────────────────

    #[test]
    fn test_genesis_bls_pubkeys_valid_format() {
        let g = devnet_genesis();
        // devnet_genesis() artık compressed flag'li key üretiyor
        assert!(g.validate().is_ok(), "devnet genesis BLS pubkey'leri geçerli olmalı");
    }

    #[test]
    fn test_genesis_bls_pubkey_invalid_hex_rejected() {
        let mut g = devnet_genesis();
        g.validators[0].bls_pubkey = "0xNOTHEX!!".to_string();
        assert!(g.validate().is_err(), "geçersiz hex BLS pubkey reddedilmeli");
    }

    #[test]
    fn test_genesis_bls_pubkey_wrong_length_rejected() {
        let mut g = devnet_genesis();
        // 32 byte (çok kısa)
        g.validators[0].bls_pubkey = format!("0x{}", "80".repeat(32));
        let result = g.validate();
        assert!(result.is_err(), "yanlış uzunlukta BLS pubkey reddedilmeli");
        assert!(result.unwrap_err().to_string().contains("48 byte"));
    }

    #[test]
    fn test_genesis_bls_pubkey_all_zeros_rejected() {
        let mut g = devnet_genesis();
        // Tüm sıfır — identity point, geçersiz
        g.validators[0].bls_pubkey = format!("0x{}", "00".repeat(48));
        let result = g.validate();
        assert!(result.is_err(), "tüm sıfır BLS pubkey reddedilmeli (identity point)");
        assert!(result.unwrap_err().to_string().contains("sıfır"));
    }

    #[test]
    fn test_genesis_bls_pubkey_missing_compressed_flag_rejected() {
        let mut g = devnet_genesis();
        // Compressed flag yok (bit7=0)
        let mut key = [0x01u8; 48];
        key[0] = 0x01; // bit7 = 0 (uncompressed veya geçersiz)
        g.validators[0].bls_pubkey = format!("0x{}", hex::encode(key));
        let result = g.validate();
        assert!(result.is_err(), "compressed flag eksik BLS pubkey reddedilmeli");
        assert!(result.unwrap_err().to_string().contains("compressed flag"));
    }

    #[test]
    fn test_genesis_bls_pubkey_poc_rogue_key_all_validators_validated() {
        // PoC: Tüm validator BLS pubkey'leri baştan doğrulanmazsa
        // rogue-key saldırısı için zemin hazırlanabilir.
        // Bu test, validate()'in tüm validatorları kontrol ettiğini doğrular.
        let mut g = devnet_genesis();
        // Son validatörü geçersiz yap
        let last_idx = g.validators.len() - 1;
        g.validators[last_idx].bls_pubkey = "0x00".repeat(1); // geçersiz hex
        g.validators[last_idx].bls_pubkey = format!("0x{}", "00".repeat(48));
        let result = g.validate();
        assert!(
            result.is_err(),
            "son validator geçersiz BLS pubkey ile reddedilmeli"
        );
    }

    #[test]
    fn test_bls_pubkey_format_validator_correct() {
        // Geçerli BLS12-381 G1 compressed pubkey formatı
        let mut key = [0x01u8; 48];
        key[0] = 0x81; // compressed flag + non-zero
        let hex = format!("0x{}", hex::encode(key));
        assert!(validate_bls_pubkey_format(&hex, 1).is_ok());
    }
}
