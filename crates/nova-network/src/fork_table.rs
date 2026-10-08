//! GÖREV 112: Per-Chain Fork Height Tablosu
//!
//! ## Mimari
//!
//! ### Go-Ethereum Fork Height Tablosu
//! - `ChainConfig.Homestead`, `ChainConfig.EIP150Block`, `ChainConfig.BerlinBlock` vb.
//! - Her hard fork, belirli bir blok yüksekliğinde aktive olur
//! - Ağ, bu yükseklikte yeni kurallar uygular
//! - `IsLondon(height)`, `IsShanghai(height)` gibi yardımcı fonksiyonlar
//!
//! ### Cosmos SDK Fork Height Tablosu
//! - `ConsensusParamsUpdate`: yükseklik bazlı parametre güncellemesi
//! - `UpgradeHandler`: isimli fork handler kaydı (go-cosmos/x/upgrade)
//!
//! ### Nova Uyarlaması
//! - `ForkConfig`: isimli forku aktive eden blok yüksekliği
//! - `ForkTable`: chain_id başına fork konfigürasyonu
//! - `ForkId`: benzersiz çatal kimliği (Ethereum ENR fork_id benzeri)
//! - `is_active(fork, height)`: fork etkin mi?

use std::collections::HashMap;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Nova Ana Ağ Chain ID — NOVA-RED-SDK-001 düzeltmesi (42_001 → 1984)
pub const NOVA_MAINNET_CHAIN_ID: u64 = 1984;
/// Nova Testnet Chain ID
pub const NOVA_TESTNET_CHAIN_ID: u64 = 19840;
/// Nova Devnet Chain ID
pub const NOVA_DEVNET_CHAIN_ID: u64 = 198400;

// ─── NovaFork ─────────────────────────────────────────────────────────────────

/// Nova Shield Chain hard fork tanımları
///
/// Go-Ethereum: `ChainConfig.LondonBlock`, `ShanghaiFork` vs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NovaFork {
    /// Genesis (her zaman aktif)
    Genesis,
    /// Tahliye (base fee algoritması, EIP-1559)
    Tahliye,
    /// Kalkan (MPC hesap model iyileştirmesi)
    Kalkan,
    /// Şahin (ZK proof sistemi v2)
    Sahin,
    /// Işık (IBC köprü + XCM)
    Isik,
    /// Özel isimli fork
    Custom(String),
}

impl NovaFork {
    pub fn name(&self) -> &str {
        match self {
            Self::Genesis => "genesis",
            Self::Tahliye => "tahliye",
            Self::Kalkan => "kalkan",
            Self::Sahin => "sahin",
            Self::Isik => "isik",
            Self::Custom(s) => s.as_str(),
        }
    }
}

// ─── ForkActivation ───────────────────────────────────────────────────────────

/// Bir fork aktivasyonu
#[derive(Debug, Clone)]
pub struct ForkActivation {
    /// Fork adı/kimliği
    pub fork: NovaFork,
    /// Aktive olan blok yüksekliği (None = devre dışı)
    pub block_height: Option<u64>,
    /// Açıklama
    pub description: String,
    /// Bu fork'la gelen özellikler
    pub features: Vec<String>,
}

impl ForkActivation {
    pub fn new(fork: NovaFork, block_height: u64, description: impl Into<String>) -> Self {
        Self {
            fork,
            block_height: Some(block_height),
            description: description.into(),
            features: vec![],
        }
    }

    pub fn disabled(fork: NovaFork) -> Self {
        Self {
            fork,
            block_height: None,
            description: "Devre dışı".to_string(),
            features: vec![],
        }
    }

    /// Bu fork şu an etkin mi?
    pub fn is_active_at(&self, height: u64) -> bool {
        match self.block_height {
            Some(activation) => height >= activation,
            None => false,
        }
    }
}

// ─── ForkId ───────────────────────────────────────────────────────────────────

/// Fork kimliği (Ethereum ENR fork_id benzeri)
///
/// Aynı chain_id'ye sahip ama farklı fork tablosuna sahip nod'lar farklı fork_id üretir
/// → bağlantı kurulması engellenir (yanlış fork'ta ağ ayrışması önlenir)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkId {
    /// Aktif fork listesinin hash'i (CRC32 benzeri)
    pub hash: u32,
    /// Bir sonraki fork yüksekliği (0 = bilinmiyor)
    pub next: u64,
}

impl ForkId {
    pub fn compute(chain_id: u64, active_forks: &[u64]) -> Self {
        // Basit deterministik hash: chain_id XOR fold of fork heights
        let mut hash = chain_id as u32;
        for &height in active_forks {
            hash ^= (height as u32).wrapping_mul(0x9e3779b9);
            hash = hash.rotate_left(5);
        }
        let next = active_forks.last().copied().map(|h| h + 1).unwrap_or(0);
        Self { hash, next }
    }
}

// ─── ChainForkConfig ──────────────────────────────────────────────────────────

/// Tek bir chain için tüm fork yapılandırması
///
/// Go-Ethereum: `params.ChainConfig`
#[derive(Debug, Clone)]
pub struct ChainForkConfig {
    /// Chain kimliği
    pub chain_id: u64,
    /// Chain adı
    pub chain_name: String,
    /// Fork aktivasyonları (sıralı)
    pub forks: Vec<ForkActivation>,
}

impl ChainForkConfig {
    pub fn new(chain_id: u64, chain_name: impl Into<String>) -> Self {
        Self {
            chain_id,
            chain_name: chain_name.into(),
            forks: vec![],
        }
    }

    /// Fork ekle
    pub fn with_fork(mut self, activation: ForkActivation) -> Self {
        self.forks.push(activation);
        self
    }

    /// Nova mainnet varsayılan fork konfigürasyonu
    pub fn nova_mainnet() -> Self {
        Self::new(NOVA_MAINNET_CHAIN_ID, "Nova Shield Chain Mainnet")
            .with_fork(ForkActivation::new(NovaFork::Genesis, 0, "Genesis blok"))
            .with_fork(ForkActivation::new(NovaFork::Tahliye, 100_000, "EIP-1559 fee market"))
            .with_fork(ForkActivation::new(NovaFork::Kalkan, 500_000, "MPC v2 + Guardian v2"))
            .with_fork(ForkActivation::disabled(NovaFork::Sahin))
            .with_fork(ForkActivation::disabled(NovaFork::Isik))
    }

    /// Nova testnet fork konfigürasyonu
    pub fn nova_testnet() -> Self {
        Self::new(NOVA_TESTNET_CHAIN_ID, "Nova Shield Chain Testnet")
            .with_fork(ForkActivation::new(NovaFork::Genesis, 0, "Genesis"))
            .with_fork(ForkActivation::new(NovaFork::Tahliye, 1_000, "Testnet EIP-1559"))
            .with_fork(ForkActivation::new(NovaFork::Kalkan, 5_000, "Testnet MPC v2"))
            .with_fork(ForkActivation::new(NovaFork::Sahin, 20_000, "Testnet ZK v2"))
    }

    /// Nova devnet fork konfigürasyonu (tüm fork'lar hemen aktif)
    pub fn nova_devnet() -> Self {
        Self::new(NOVA_DEVNET_CHAIN_ID, "Nova Shield Chain Devnet")
            .with_fork(ForkActivation::new(NovaFork::Genesis, 0, "Genesis"))
            .with_fork(ForkActivation::new(NovaFork::Tahliye, 0, "Devnet: hemen aktif"))
            .with_fork(ForkActivation::new(NovaFork::Kalkan, 0, "Devnet: hemen aktif"))
            .with_fork(ForkActivation::new(NovaFork::Sahin, 0, "Devnet: hemen aktif"))
            .with_fork(ForkActivation::new(NovaFork::Isik, 0, "Devnet: hemen aktif"))
    }

    /// Belirtilen yükseklikte etkin olan fork'ları getir
    pub fn active_forks_at(&self, height: u64) -> Vec<&ForkActivation> {
        self.forks.iter().filter(|f| f.is_active_at(height)).collect()
    }

    /// Belirli bir fork etkin mi?
    pub fn is_fork_active(&self, fork: &NovaFork, height: u64) -> bool {
        self.forks.iter()
            .find(|f| &f.fork == fork)
            .map(|f| f.is_active_at(height))
            .unwrap_or(false)
    }

    /// Aktif fork sayısı
    pub fn active_fork_count(&self, height: u64) -> usize {
        self.active_forks_at(height).len()
    }

    /// Bu yükseklik için ForkId hesapla
    pub fn fork_id(&self, height: u64) -> ForkId {
        let active_heights: Vec<u64> = self.forks.iter()
            .filter_map(|f| f.block_height)
            .filter(|&h| h <= height)
            .collect();
        ForkId::compute(self.chain_id, &active_heights)
    }

    /// Bir sonraki fork yüksekliği (şu anda etkin değil ama yakında olacak)
    pub fn next_fork_height(&self, height: u64) -> Option<u64> {
        self.forks.iter()
            .filter_map(|f| f.block_height)
            .filter(|&h| h > height)
            .min()
    }
}

// ─── ForkTable ────────────────────────────────────────────────────────────────

/// Çoklu chain fork konfigürasyon tablosu
///
/// Cosmos: `app.UpgradeKeeper` (upgrade/keeper.go)
pub struct ForkTable {
    configs: HashMap<u64, ChainForkConfig>,
}

impl ForkTable {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    /// Nova üçlüsü (mainnet + testnet + devnet) ile başlat
    pub fn nova_default() -> Self {
        let mut table = Self::new();
        table.register(ChainForkConfig::nova_mainnet());
        table.register(ChainForkConfig::nova_testnet());
        table.register(ChainForkConfig::nova_devnet());
        table
    }

    /// Chain konfigürasyonu ekle/güncelle
    pub fn register(&mut self, config: ChainForkConfig) {
        self.configs.insert(config.chain_id, config);
    }

    /// Chain'e ait fork konfigürasyonu
    pub fn get(&self, chain_id: u64) -> Option<&ChainForkConfig> {
        self.configs.get(&chain_id)
    }

    /// Fork etkin mi?
    pub fn is_fork_active(&self, chain_id: u64, fork: &NovaFork, height: u64) -> bool {
        self.configs.get(&chain_id)
            .map(|c| c.is_fork_active(fork, height))
            .unwrap_or(false)
    }

    /// ForkId hesapla (peer compatibility check için)
    pub fn fork_id(&self, chain_id: u64, height: u64) -> Option<ForkId> {
        self.configs.get(&chain_id).map(|c| c.fork_id(height))
    }

    /// Kayıtlı chain sayısı
    pub fn chain_count(&self) -> usize {
        self.configs.len()
    }
}

impl Default for ForkTable {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Mainnet fork konfigürasyonu
    #[test]
    fn test_mainnet_config() {
        let config = ChainForkConfig::nova_mainnet();
        assert_eq!(config.chain_id, NOVA_MAINNET_CHAIN_ID);

        // Genesis her zaman aktif
        assert!(config.is_fork_active(&NovaFork::Genesis, 0));
        assert!(config.is_fork_active(&NovaFork::Genesis, 1_000_000));
    }

    // 2. Fork yüksekliği kontrolü
    #[test]
    fn test_fork_activation_height() {
        let config = ChainForkConfig::nova_mainnet();

        // Tahliye: blok 100_000'de aktive olur
        assert!(!config.is_fork_active(&NovaFork::Tahliye, 99_999));
        assert!(config.is_fork_active(&NovaFork::Tahliye, 100_000));
        assert!(config.is_fork_active(&NovaFork::Tahliye, 200_000));
    }

    // 3. Devre dışı fork hiçbir zaman aktif olmaz
    #[test]
    fn test_disabled_fork_never_active() {
        let config = ChainForkConfig::nova_mainnet();
        // Sahin mainnet'te devre dışı
        assert!(!config.is_fork_active(&NovaFork::Sahin, 0));
        assert!(!config.is_fork_active(&NovaFork::Sahin, u64::MAX));
    }

    // 4. Devnet: tüm fork'lar 0. blokta aktif
    #[test]
    fn test_devnet_all_active_at_genesis() {
        let config = ChainForkConfig::nova_devnet();
        assert!(config.is_fork_active(&NovaFork::Genesis, 0));
        assert!(config.is_fork_active(&NovaFork::Tahliye, 0));
        assert!(config.is_fork_active(&NovaFork::Sahin, 0));
        assert!(config.is_fork_active(&NovaFork::Isik, 0));
    }

    // 5. active_forks_at doğru sayı döner
    #[test]
    fn test_active_forks_count() {
        let config = ChainForkConfig::nova_mainnet();

        // Blok 0: sadece Genesis
        let at_0 = config.active_fork_count(0);
        assert_eq!(at_0, 1); // sadece Genesis

        // Blok 100_000: Genesis + Tahliye
        let at_100k = config.active_fork_count(100_000);
        assert_eq!(at_100k, 2);

        // Blok 500_000: Genesis + Tahliye + Kalkan
        let at_500k = config.active_fork_count(500_000);
        assert_eq!(at_500k, 3);
    }

    // 6. ForkId deterministik
    #[test]
    fn test_fork_id_deterministic() {
        let config = ChainForkConfig::nova_mainnet();
        let id1 = config.fork_id(0);
        let id2 = config.fork_id(0);
        assert_eq!(id1, id2);
    }

    // 7. ForkId farklı yüksekliklerde farklı
    #[test]
    fn test_fork_id_changes_at_fork() {
        let config = ChainForkConfig::nova_mainnet();
        let id_before = config.fork_id(99_999);
        let id_after = config.fork_id(100_000);
        assert_ne!(id_before, id_after);
    }

    // 8. Sonraki fork yüksekliği
    #[test]
    fn test_next_fork_height() {
        let config = ChainForkConfig::nova_mainnet();

        // Blok 50_000'de → sonraki fork: 100_000 (Tahliye)
        let next = config.next_fork_height(50_000);
        assert_eq!(next, Some(100_000));

        // Blok 100_000'den sonra → sonraki: 500_000 (Kalkan)
        let next2 = config.next_fork_height(100_000);
        assert_eq!(next2, Some(500_000));
    }

    // 9. ForkTable çoklu chain yönetimi
    #[test]
    fn test_fork_table_multiple_chains() {
        let table = ForkTable::nova_default();
        assert_eq!(table.chain_count(), 3);

        // Mainnet'te Tahliye 100k'da aktif
        assert!(table.is_fork_active(NOVA_MAINNET_CHAIN_ID, &NovaFork::Tahliye, 100_000));

        // Testnet'te Tahliye 1000'de aktif
        assert!(table.is_fork_active(NOVA_TESTNET_CHAIN_ID, &NovaFork::Tahliye, 1_000));
        assert!(!table.is_fork_active(NOVA_TESTNET_CHAIN_ID, &NovaFork::Tahliye, 999));
    }

    // 10. Bilinmeyen chain → false
    #[test]
    fn test_unknown_chain() {
        let table = ForkTable::nova_default();
        assert!(!table.is_fork_active(99999, &NovaFork::Genesis, 0));
        assert!(table.fork_id(99999, 0).is_none());
    }

    // 11. ForkActivation is_active_at
    #[test]
    fn test_fork_activation_edge_cases() {
        let act = ForkActivation::new(NovaFork::Tahliye, 100, "test");
        assert!(!act.is_active_at(99));
        assert!(act.is_active_at(100));
        assert!(act.is_active_at(101));

        let disabled = ForkActivation::disabled(NovaFork::Sahin);
        assert!(!disabled.is_active_at(0));
        assert!(!disabled.is_active_at(u64::MAX));
    }

    // 12. ForkTable register özel chain
    #[test]
    fn test_fork_table_custom_chain() {
        let mut table = ForkTable::new();
        let custom = ChainForkConfig::new(99, "Test Chain")
            .with_fork(ForkActivation::new(NovaFork::Genesis, 0, "Genesis"))
            .with_fork(ForkActivation::new(NovaFork::Custom("v2".to_string()), 500, "V2 upgrade"));

        table.register(custom);
        assert_eq!(table.chain_count(), 1);
        assert!(table.is_fork_active(99, &NovaFork::Genesis, 0));
        assert!(!table.is_fork_active(99, &NovaFork::Custom("v2".to_string()), 499));
        assert!(table.is_fork_active(99, &NovaFork::Custom("v2".to_string()), 500));
    }

    // 13. Testnet Sahin fork aktif
    #[test]
    fn test_testnet_sahin_fork() {
        let config = ChainForkConfig::nova_testnet();
        assert!(!config.is_fork_active(&NovaFork::Sahin, 19_999));
        assert!(config.is_fork_active(&NovaFork::Sahin, 20_000));
    }

    // 14. Fork name
    #[test]
    fn test_fork_names() {
        assert_eq!(NovaFork::Genesis.name(), "genesis");
        assert_eq!(NovaFork::Tahliye.name(), "tahliye");
        assert_eq!(NovaFork::Custom("test".to_string()).name(), "test");
    }
}
