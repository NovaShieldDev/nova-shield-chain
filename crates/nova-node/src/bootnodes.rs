//! Well-known bootnode listesi — Nova Shield Chain
//!
//! Yeni bir node başlatıldığında `--boot-nodes` verilmezse bu liste kullanılır.
//! Her chain için ayrı statik listeler tutulur; format: `host:port`.
//!
//! ## DNS Seed
//!
//! DNS-SD kayıtları:
//!   `_nova._tcp.mainnet.novashield.io` TXT → mainnet bootnode listesi
//!   `_nova._tcp.testnet.novashield.io` TXT → testnet bootnode listesi
//!
//! DNS güncellenmesi merkezi olmayan süreci engeller; statik liste
//! fallback olarak binary'ye gömülüdür.
//!
//! ## Güvenlik
//!
//! Bootnode bağlantısı ilk temas noktasıdır; BFT konsensüsüne katılım için
//! genesis hash doğrulaması yapılır — yanlış ağa bağlanma imkansızdır.

use nova_primitives::constants::{DEVNET_CHAIN_ID, MAINNET_CHAIN_ID, TESTNET_CHAIN_ID};

// ─── Mainnet Bootnode Listesi ──────────────────────────────────────────────────

/// Nova Shield Mainnet (chain_id=1984) well-known bootnode'ları
///
/// Format: `hostname:port` veya `ip:port`
/// P2P port: 30333 (default)
pub const MAINNET_BOOTNODES: &[&str] = &[
    "bootnode-1.mainnet.novashield.io:30333",
    "bootnode-2.mainnet.novashield.io:30333",
    "bootnode-3.mainnet.novashield.io:30333",
    "bootnode-4.mainnet.novashield.io:30333",
    "bootnode-5.mainnet.novashield.io:30333",
];

// ─── Testnet Bootnode Listesi ──────────────────────────────────────────────────

/// Nova Shield Testnet (chain_id=19840) well-known bootnode'ları
pub const TESTNET_BOOTNODES: &[&str] = &[
    "bootnode-1.testnet.novashield.io:30333",
    "bootnode-2.testnet.novashield.io:30333",
    "bootnode-3.testnet.novashield.io:30333",
];

// ─── Devnet Bootnode Listesi ───────────────────────────────────────────────────

/// Nova Shield Devnet (chain_id=198400) — yerel geliştirme ortamı
pub const DEVNET_BOOTNODES: &[&str] = &[
    "127.0.0.1:30333",
    "127.0.0.1:30334",
    "127.0.0.1:30335",
];

// ─── BootnodeInfo ──────────────────────────────────────────────────────────────

/// Tek bir bootnode'un tüm bilgileri
#[derive(Debug, Clone)]
pub struct BootnodeInfo {
    /// Sıra numarası (1'den başlar)
    pub id: u8,
    /// P2P endpoint (host:port)
    pub p2p_endpoint: String,
    /// Coğrafi bölge (monitoring için)
    pub region: &'static str,
    /// Ait olduğu chain ID
    pub chain_id: u64,
}

impl BootnodeInfo {
    fn new(id: u8, p2p_endpoint: &str, region: &'static str, chain_id: u64) -> Self {
        Self {
            id,
            p2p_endpoint: p2p_endpoint.to_string(),
            region,
            chain_id,
        }
    }

    /// Sadece host:port formatındaki P2P adresini döndür
    pub fn endpoint(&self) -> &str {
        &self.p2p_endpoint
    }
}

impl std::fmt::Display for BootnodeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "bootnode-{} [{}] {} (chain_id={})",
            self.id, self.region, self.p2p_endpoint, self.chain_id
        )
    }
}

// ─── Yardımcı Fonksiyonlar ─────────────────────────────────────────────────────

/// Verilen chain ID için bootnode listesini döndür
///
/// # Dönüş
/// P2P endpoint string listesi (`host:port` formatında).
/// `--boot-nodes` CLI argümanı verilmemişse bu liste kullanılır.
///
/// # Örnek
/// ```
/// use nova_node::bootnodes::bootnodes_for_chain;
/// use nova_primitives::constants::MAINNET_CHAIN_ID;
///
/// let nodes = bootnodes_for_chain(MAINNET_CHAIN_ID);
/// assert!(!nodes.is_empty());
/// ```
pub fn bootnodes_for_chain(chain_id: u64) -> Vec<String> {
    let list: &[&str] = match chain_id {
        id if id == MAINNET_CHAIN_ID => MAINNET_BOOTNODES,
        id if id == TESTNET_CHAIN_ID => TESTNET_BOOTNODES,
        id if id == DEVNET_CHAIN_ID => DEVNET_BOOTNODES,
        _ => DEVNET_BOOTNODES,
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// Verilen chain ID için detaylı BootnodeInfo listesi döndür
pub fn bootnode_infos_for_chain(chain_id: u64) -> Vec<BootnodeInfo> {
    match chain_id {
        id if id == MAINNET_CHAIN_ID => mainnet_bootnode_infos(),
        id if id == TESTNET_CHAIN_ID => testnet_bootnode_infos(),
        _ => devnet_bootnode_infos(),
    }
}

/// Bootnode sayısı
pub fn bootnode_count(chain_id: u64) -> usize {
    bootnodes_for_chain(chain_id).len()
}

// ─── Detaylı Listeler ──────────────────────────────────────────────────────────

fn mainnet_bootnode_infos() -> Vec<BootnodeInfo> {
    vec![
        BootnodeInfo::new(1, MAINNET_BOOTNODES[0], "EU-West",   MAINNET_CHAIN_ID),
        BootnodeInfo::new(2, MAINNET_BOOTNODES[1], "EU-Central", MAINNET_CHAIN_ID),
        BootnodeInfo::new(3, MAINNET_BOOTNODES[2], "US-East",   MAINNET_CHAIN_ID),
        BootnodeInfo::new(4, MAINNET_BOOTNODES[3], "AP-East",   MAINNET_CHAIN_ID),
        BootnodeInfo::new(5, MAINNET_BOOTNODES[4], "AP-South",  MAINNET_CHAIN_ID),
    ]
}

fn testnet_bootnode_infos() -> Vec<BootnodeInfo> {
    vec![
        BootnodeInfo::new(1, TESTNET_BOOTNODES[0], "EU-West",  TESTNET_CHAIN_ID),
        BootnodeInfo::new(2, TESTNET_BOOTNODES[1], "US-East",  TESTNET_CHAIN_ID),
        BootnodeInfo::new(3, TESTNET_BOOTNODES[2], "AP-East",  TESTNET_CHAIN_ID),
    ]
}

fn devnet_bootnode_infos() -> Vec<BootnodeInfo> {
    vec![
        BootnodeInfo::new(1, DEVNET_BOOTNODES[0], "localhost", DEVNET_CHAIN_ID),
        BootnodeInfo::new(2, DEVNET_BOOTNODES[1], "localhost", DEVNET_CHAIN_ID),
        BootnodeInfo::new(3, DEVNET_BOOTNODES[2], "localhost", DEVNET_CHAIN_ID),
    ]
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_bootnodes_not_empty() {
        let nodes = bootnodes_for_chain(MAINNET_CHAIN_ID);
        assert_eq!(nodes.len(), 5);
    }

    #[test]
    fn test_testnet_bootnodes_not_empty() {
        let nodes = bootnodes_for_chain(TESTNET_CHAIN_ID);
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_devnet_bootnodes_not_empty() {
        let nodes = bootnodes_for_chain(DEVNET_CHAIN_ID);
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_unknown_chain_id_falls_back_to_devnet() {
        let nodes = bootnodes_for_chain(99999);
        assert!(!nodes.is_empty());
        // devnet localhost'lar gelir
        assert!(nodes[0].starts_with("127.0.0.1"));
    }

    #[test]
    fn test_bootnodes_format_host_port() {
        for node in bootnodes_for_chain(MAINNET_CHAIN_ID) {
            assert!(node.contains(':'), "bootnode format hatalı: {node}");
            let parts: Vec<&str> = node.rsplitn(2, ':').collect();
            let port: u16 = parts[0].parse().expect("port sayı olmalı");
            assert!(port > 0, "port sıfır olamaz");
        }
    }

    #[test]
    fn test_mainnet_default_port_30333() {
        for node in bootnodes_for_chain(MAINNET_CHAIN_ID) {
            assert!(
                node.ends_with(":30333"),
                "mainnet bootnode standart port 30333 olmalı: {node}"
            );
        }
    }

    #[test]
    fn test_bootnode_count() {
        assert_eq!(bootnode_count(MAINNET_CHAIN_ID), 5);
        assert_eq!(bootnode_count(TESTNET_CHAIN_ID), 3);
        assert_eq!(bootnode_count(DEVNET_CHAIN_ID), 3);
    }

    #[test]
    fn test_bootnode_infos_mainnet() {
        let infos = bootnode_infos_for_chain(MAINNET_CHAIN_ID);
        assert_eq!(infos.len(), 5);
        for (i, info) in infos.iter().enumerate() {
            assert_eq!(info.id as usize, i + 1);
            assert_eq!(info.chain_id, MAINNET_CHAIN_ID);
            assert!(!info.region.is_empty());
        }
    }

    #[test]
    fn test_bootnode_infos_testnet() {
        let infos = bootnode_infos_for_chain(TESTNET_CHAIN_ID);
        assert_eq!(infos.len(), 3);
        for info in &infos {
            assert_eq!(info.chain_id, TESTNET_CHAIN_ID);
        }
    }

    #[test]
    fn test_bootnode_info_display() {
        let info = BootnodeInfo::new(1, "bootnode-1.mainnet.novashield.io:30333", "EU-West", MAINNET_CHAIN_ID);
        let s = format!("{info}");
        assert!(s.contains("bootnode-1"));
        assert!(s.contains("EU-West"));
        assert!(s.contains("chain_id=1984"));
    }

    #[test]
    fn test_bootnode_info_endpoint() {
        let info = BootnodeInfo::new(2, "1.2.3.4:30333", "US-East", MAINNET_CHAIN_ID);
        assert_eq!(info.endpoint(), "1.2.3.4:30333");
    }

    #[test]
    fn test_no_duplicate_bootnodes() {
        let nodes = bootnodes_for_chain(MAINNET_CHAIN_ID);
        let mut seen = std::collections::HashSet::new();
        for n in &nodes {
            assert!(seen.insert(n.clone()), "tekrar eden bootnode: {n}");
        }
    }

    #[test]
    fn test_devnet_bootnodes_are_localhost() {
        let nodes = bootnodes_for_chain(DEVNET_CHAIN_ID);
        for node in &nodes {
            assert!(
                node.starts_with("127.0.0.1"),
                "devnet bootnodes localhost olmalı: {node}"
            );
        }
    }

    #[test]
    fn test_mainnet_bootnodes_are_not_localhost() {
        let nodes = bootnodes_for_chain(MAINNET_CHAIN_ID);
        for node in &nodes {
            assert!(
                !node.starts_with("127.0.0.1"),
                "mainnet bootnode localhost olamaz: {node}"
            );
            assert!(
                !node.starts_with("0.0.0.0"),
                "mainnet bootnode 0.0.0.0 olamaz: {node}"
            );
        }
    }

    #[test]
    fn test_static_lists_constants_len() {
        assert_eq!(MAINNET_BOOTNODES.len(), 5);
        assert_eq!(TESTNET_BOOTNODES.len(), 3);
        assert_eq!(DEVNET_BOOTNODES.len(), 3);
    }
}
