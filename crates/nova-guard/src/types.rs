//! Cross-chain temel türler
//!
//! Nova Shield Guard §5.6 — BTC/ETH/SOL/Cosmos varlık koruması
//! Polkadot bridge relay: `Chain` trait pattern
//! go-ethereum: `common.Address`, `common.Hash`, `big.Int` → Rust karşılıkları

use serde::{Deserialize, Serialize};

// ─── ChainId ──────────────────────────────────────────────────────────────────

/// Desteklenen zincirler — §5.6.2
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChainId {
    /// Bitcoin mainnet
    Bitcoin,
    /// Ethereum mainnet (chain_id=1)
    Ethereum,
    /// Solana mainnet
    Solana,
    /// Cosmos Hub (chain-id: cosmoshub-4)
    Cosmos,
}

impl ChainId {
    pub fn name(&self) -> &'static str {
        match self {
            ChainId::Bitcoin  => "bitcoin",
            ChainId::Ethereum => "ethereum",
            ChainId::Solana   => "solana",
            ChainId::Cosmos   => "cosmos",
        }
    }

    /// Zincir için yerel para birimi
    pub fn native_asset(&self) -> &'static str {
        match self {
            ChainId::Bitcoin  => "BTC",
            ChainId::Ethereum => "ETH",
            ChainId::Solana   => "SOL",
            ChainId::Cosmos   => "ATOM",
        }
    }
}

// ─── AssetAddress ─────────────────────────────────────────────────────────────

/// Zincire özgü adres — go-ethereum `common.Address`, Bitcoin bech32 vb.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetAddress {
    pub chain: ChainId,
    /// Adres ham verisi (zincire göre format farklı)
    pub raw: Vec<u8>,
    /// İnsan-okunabilir adres
    pub display: String,
}

impl AssetAddress {
    pub fn new(chain: ChainId, raw: Vec<u8>, display: impl Into<String>) -> Self {
        Self { chain, raw, display: display.into() }
    }

    /// Ethereum adresi (20 byte)
    pub fn ethereum(bytes: [u8; 20]) -> Self {
        let display = format!("0x{}", hex::encode(bytes));
        Self::new(ChainId::Ethereum, bytes.to_vec(), display)
    }

    /// Bitcoin adresi (P2PKH/P2WSH string)
    pub fn bitcoin(addr: impl Into<String>) -> Self {
        let s = addr.into();
        Self::new(ChainId::Bitcoin, s.as_bytes().to_vec(), s)
    }

    /// Solana adresi (32-byte pubkey, base58 display)
    pub fn solana(bytes: [u8; 32]) -> Self {
        // Mock base58: hex kodlama kullan
        let display = format!("SOL{}", hex::encode(&bytes[..8]));
        Self::new(ChainId::Solana, bytes.to_vec(), display)
    }

    /// Cosmos adresi (bech32, "cosmos1..." prefix)
    pub fn cosmos(bytes: [u8; 20]) -> Self {
        let display = format!("cosmos1{}", hex::encode(bytes));
        Self::new(ChainId::Cosmos, bytes.to_vec(), display)
    }
}

// ─── CrossChainAsset ──────────────────────────────────────────────────────────

/// Cross-chain varlık bilgisi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainAsset {
    /// Hangi zincir
    pub chain: ChainId,
    /// Varlık tipi (ör. "BTC", "ETH", "USDC", "SOL")
    pub symbol: String,
    /// Token sözleşmesi (ETH/SOL için, BTC için None)
    pub contract: Option<Vec<u8>>,
    /// Miktar (en küçük birim: satoshi, wei, lamport, uatom)
    pub amount: u64,
    /// Ondalık hassasiyet (BTC=8, ETH=18, SOL=9, ATOM=6)
    pub decimals: u8,
}

impl CrossChainAsset {
    pub fn btc(satoshis: u64) -> Self {
        Self { chain: ChainId::Bitcoin, symbol: "BTC".into(), contract: None, amount: satoshis, decimals: 8 }
    }

    pub fn eth(wei: u64) -> Self {
        Self { chain: ChainId::Ethereum, symbol: "ETH".into(), contract: None, amount: wei, decimals: 18 }
    }

    pub fn sol(lamports: u64) -> Self {
        Self { chain: ChainId::Solana, symbol: "SOL".into(), contract: None, amount: lamports, decimals: 9 }
    }

    pub fn atom(uatom: u64) -> Self {
        Self { chain: ChainId::Cosmos, symbol: "ATOM".into(), contract: None, amount: uatom, decimals: 6 }
    }

    /// Insan-okunabilir miktar
    pub fn human_amount(&self) -> f64 {
        self.amount as f64 / 10f64.powi(self.decimals as i32)
    }
}

// ─── MpcThreshold ─────────────────────────────────────────────────────────────

/// MPC eşik konfigürasyonu — §5.6.3
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpcThreshold {
    /// Gerekli imza sayısı (t)
    pub threshold: u32,
    /// Toplam pay sayısı (n)
    pub total: u32,
    /// Guardian adresleri
    pub guardians: Vec<[u8; 20]>,
}

impl MpcThreshold {
    pub fn new(threshold: u32, total: u32, guardians: Vec<[u8; 20]>) -> Option<Self> {
        if threshold == 0 || total == 0 || threshold > total || guardians.len() != total as usize {
            return None;
        }
        Some(Self { threshold, total, guardians })
    }

    pub fn is_quorum_met(&self, signers: &[[u8; 20]]) -> bool {
        let count = signers.iter().filter(|s| self.guardians.contains(s)).count();
        count >= self.threshold as usize
    }
}

// ─── GuardedVault ─────────────────────────────────────────────────────────────

/// MPC korumalı vault — §5.6.3
/// Cross-chain'de MPC ile kontrol edilen bir adres
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardedVault {
    /// Benzersiz vault ID
    pub id: u64,
    /// Sahip (Nova Shield hesabı)
    pub owner: [u8; 20],
    /// Hangi zincirde
    pub chain: ChainId,
    /// Vault adresi (MPC türetilmiş)
    pub address: AssetAddress,
    /// MPC eşik konfigürasyonu
    pub threshold: MpcThreshold,
    /// Vault oluşturulma slotu
    pub created_at_slot: u64,
    /// Aktif mi?
    pub active: bool,
    /// Anlık bakiye (satoshi/wei/lamport/uatom)
    pub balance: u64,
}

// ─── CrossChainTx ─────────────────────────────────────────────────────────────

/// Cross-chain işlem talebi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTxRequest {
    /// Kaynak vault ID
    pub vault_id: u64,
    /// Hedef adres
    pub to: AssetAddress,
    /// Varlık ve miktar
    pub asset: CrossChainAsset,
    /// İşlem ücreti (yerel para biriminde)
    pub fee: u64,
    /// İmza isteyen guardian'lar
    pub requested_by: [u8; 20],
    /// Talep zamanı (slot)
    pub requested_at_slot: u64,
}

/// Cross-chain işlem sonucu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTxReceipt {
    /// İşlem hash'i (hedef zincirde)
    pub tx_hash: [u8; 32],
    /// Vault ID
    pub vault_id: u64,
    /// Nova Shield zincirinde kayıt slotu
    pub recorded_slot: u64,
    /// MPC imzalayanlar
    pub signers: Vec<[u8; 20]>,
    /// Başarılı mı?
    pub success: bool,
}

// ─── BLAKE3 yardımcı ──────────────────────────────────────────────────────────

/// BLAKE3 ile 32-byte hash üret
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

/// BLAKE3 ile çok girdili hash
pub fn blake3_hash_many(inputs: &[&[u8]]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for input in inputs {
        h.update(input);
    }
    *h.finalize().as_bytes()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_name() {
        assert_eq!(ChainId::Bitcoin.name(), "bitcoin");
        assert_eq!(ChainId::Ethereum.name(), "ethereum");
        assert_eq!(ChainId::Solana.name(), "solana");
        assert_eq!(ChainId::Cosmos.name(), "cosmos");
    }

    #[test]
    fn test_chain_native_asset() {
        assert_eq!(ChainId::Bitcoin.native_asset(), "BTC");
        assert_eq!(ChainId::Ethereum.native_asset(), "ETH");
        assert_eq!(ChainId::Solana.native_asset(), "SOL");
        assert_eq!(ChainId::Cosmos.native_asset(), "ATOM");
    }

    #[test]
    fn test_ethereum_address() {
        let addr = AssetAddress::ethereum([0x12u8; 20]);
        assert_eq!(addr.chain, ChainId::Ethereum);
        assert!(addr.display.starts_with("0x"));
        assert_eq!(addr.raw.len(), 20);
    }

    #[test]
    fn test_bitcoin_address() {
        let addr = AssetAddress::bitcoin("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh");
        assert_eq!(addr.chain, ChainId::Bitcoin);
    }

    #[test]
    fn test_solana_address() {
        let addr = AssetAddress::solana([0xABu8; 32]);
        assert_eq!(addr.chain, ChainId::Solana);
        assert!(addr.display.starts_with("SOL"));
    }

    #[test]
    fn test_cosmos_address() {
        let addr = AssetAddress::cosmos([0x05u8; 20]);
        assert_eq!(addr.chain, ChainId::Cosmos);
        assert!(addr.display.starts_with("cosmos1"));
    }

    #[test]
    fn test_cross_chain_asset_btc() {
        let a = CrossChainAsset::btc(100_000_000); // 1 BTC
        assert_eq!(a.human_amount(), 1.0);
        assert_eq!(a.symbol, "BTC");
    }

    #[test]
    fn test_cross_chain_asset_eth() {
        let a = CrossChainAsset::eth(1_000_000_000_000_000_000); // 1 ETH
        assert!((a.human_amount() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cross_chain_asset_sol() {
        let a = CrossChainAsset::sol(1_000_000_000); // 1 SOL
        assert!((a.human_amount() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_mpc_threshold_valid() {
        let g: Vec<[u8; 20]> = (0..5).map(|i| [i as u8; 20]).collect();
        let t = MpcThreshold::new(3, 5, g).unwrap();
        assert_eq!(t.threshold, 3);
        assert_eq!(t.total, 5);
    }

    #[test]
    fn test_mpc_threshold_invalid() {
        let g: Vec<[u8; 20]> = (0..3).map(|i| [i as u8; 20]).collect();
        // threshold > total
        assert!(MpcThreshold::new(4, 3, g.clone()).is_none());
        // guardian sayısı uyuşmuyor
        assert!(MpcThreshold::new(2, 5, g).is_none());
    }

    #[test]
    fn test_mpc_quorum_met() {
        let g: Vec<[u8; 20]> = (0..5).map(|i| [i as u8; 20]).collect();
        let t = MpcThreshold::new(3, 5, g).unwrap();

        let signers: Vec<[u8; 20]> = vec![[0u8; 20], [1u8; 20], [2u8; 20]];
        assert!(t.is_quorum_met(&signers));

        let partial: Vec<[u8; 20]> = vec![[0u8; 20], [1u8; 20]];
        assert!(!t.is_quorum_met(&partial));
    }

    #[test]
    fn test_blake3_hash_deterministic() {
        let h1 = blake3_hash(b"nova-guard-test");
        let h2 = blake3_hash(b"nova-guard-test");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_blake3_hash_many() {
        let h = blake3_hash_many(&[b"chain:btc", b"amount:100"]);
        assert_ne!(h, [0u8; 32]);
    }
}
