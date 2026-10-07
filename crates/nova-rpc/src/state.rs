//! Zincir durumu — RPC handler'larının kullandığı veri deposu
//!
//! `ChainState` iki modda çalışır:
//! - **Mock modu** (varsayılan, testler): DashMap tabanlı in-memory state
//! - **Backend modu** (production): `RuntimeBackend` trait üzerinden gerçek
//!   nova-runtime verisi (AccountStateCache + RocksDB blok deposu)

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── RuntimeBackend trait ─────────────────────────────────────────────────────

/// Gerçek runtime state sorgu arayüzü.
///
/// nova-rpc bu trait üzerinden nova-runtime'a erişir — doğrudan bağımlılık
/// olmadan. nova-node, `EngineStateHandle` ile bu trait'i uygular.
pub trait RuntimeBackend: Send + Sync {
    /// En son execute edilmiş blok yüksekliği
    fn best_block_number(&self) -> u64;
    /// Verilen yükseklikteki blok hash'i
    fn block_hash_at(&self, height: u64) -> Option<[u8; 32]>;
    /// Verilen yükseklikteki parent hash
    fn block_parent_hash_at(&self, height: u64) -> Option<[u8; 32]>;
    /// Verilen yükseklikteki blok zaman damgası (ms)
    fn block_timestamp_at(&self, height: u64) -> Option<u64>;
    /// Verilen yükseklikteki TX hash listesi
    fn block_tx_hashes_at(&self, height: u64) -> Vec<[u8; 32]>;
    /// Verilen yükseklikteki proposer validator ID
    fn block_proposer_at(&self, height: u64) -> Option<u64>;
    /// Adrese ait bakiye (bilinmiyorsa 0)
    fn account_balance(&self, addr_hex: &str) -> u128;
    /// Adrese ait nonce (bilinmiyorsa 0)
    fn account_nonce(&self, addr_hex: &str) -> u64;
}

// ─── MockBlock ────────────────────────────────────────────────────────────────

/// Mock blok
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockBlock {
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: u64,
    pub transactions: Vec<String>,
    pub validator: String,
    pub tx_count: u32,
}

impl MockBlock {
    pub fn genesis() -> Self {
        Self {
            number: 0,
            hash: "0x0000000000000000000000000000000000000000000000000000000000000000".into(),
            parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".into(),
            timestamp: 0,
            transactions: vec![],
            validator: "0x0000000000000000000000000000000000000000".into(),
            tx_count: 0,
        }
    }

    pub fn new(number: u64, parent_hash: String) -> Self {
        let hash = {
            let mut h = blake3::Hasher::new();
            h.update(b"nova-block");
            h.update(&number.to_le_bytes());
            h.update(parent_hash.as_bytes());
            format!("0x{}", hex::encode(h.finalize().as_bytes()))
        };
        Self {
            number,
            hash,
            parent_hash,
            timestamp: number * 400,
            transactions: vec![],
            validator: "0x0000000000000000000000000000000000000001".into(),
            tx_count: 0,
        }
    }
}

// ─── MockAccount ──────────────────────────────────────────────────────────────

/// Mock hesap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockAccount {
    pub address: String,
    pub nonce: u64,
    pub balance: u128,
    pub guardian_count: u32,
}

// ─── MockTx ───────────────────────────────────────────────────────────────────

/// Mock işlem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockTx {
    pub hash: String,
    pub from: String,
    pub to: String,
    pub value: u128,
    pub gas_price: u64,
    pub status: TxStatus,
    pub block_number: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxStatus {
    Pending,
    Confirmed,
    Failed,
}

// ─── ChainState ───────────────────────────────────────────────────────────────

/// Zincir durumu — tüm RPC handler'larının paylaştığı state
///
/// Mock modu (testler): `ChainState::new()` — DashMap tabanlı.
/// Backend modu (production): `ChainState::with_backend()` — gerçek runtime.
#[derive(Clone)]
pub struct ChainState {
    pub blocks: Arc<DashMap<u64, MockBlock>>,
    pub accounts: Arc<DashMap<String, MockAccount>>,
    pub transactions: Arc<DashMap<String, MockTx>>,
    pub best_block: Arc<parking_lot::RwLock<u64>>,
    /// Production runtime backend (None = mock modu)
    backend: Option<Arc<dyn RuntimeBackend>>,
}

impl ChainState {
    pub fn new() -> Self {
        let state = Self {
            blocks: Arc::new(DashMap::new()),
            accounts: Arc::new(DashMap::new()),
            transactions: Arc::new(DashMap::new()),
            best_block: Arc::new(parking_lot::RwLock::new(0)),
            backend: None,
        };

        // Genesis bloğu ekle
        state.blocks.insert(0, MockBlock::genesis());

        // Test hesapları ekle
        state.accounts.insert(
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            MockAccount {
                address: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                nonce: 5,
                balance: 1_000_000_000_000_000_000_000, // 1000 NSC
                guardian_count: 3,
            },
        );

        state
    }

    /// Yeni blok üret
    pub fn produce_block(&self) -> MockBlock {
        let mut best = self.best_block.write();
        let parent = self.blocks.get(&*best).map(|b| b.hash.clone())
            .unwrap_or_default();
        *best += 1;
        let block = MockBlock::new(*best, parent);
        self.blocks.insert(*best, block.clone());
        block
    }

    /// İşlem gönder (mock)
    pub fn send_transaction(&self, from: String, to: String, value: u128, gas_price: u64) -> String {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-tx");
        h.update(from.as_bytes());
        h.update(to.as_bytes());
        h.update(&value.to_le_bytes());
        let hash = format!("0x{}", hex::encode(h.finalize().as_bytes()));

        let tx = MockTx {
            hash: hash.clone(),
            from,
            to,
            value,
            gas_price,
            status: TxStatus::Pending,
            block_number: None,
        };
        self.transactions.insert(hash.clone(), tx);
        hash
    }

    /// Production runtime backend ile yeni state oluştur
    pub fn with_backend(backend: Arc<dyn RuntimeBackend>) -> Self {
        let mut s = Self::new();
        s.backend = Some(backend);
        s
    }

    pub fn best_block_number(&self) -> u64 {
        if let Some(ref b) = self.backend {
            return b.best_block_number();
        }
        *self.best_block.read()
    }

    /// Verilen yükseklikteki blok verisini döndür.
    ///
    /// Backend varsa gerçek runtime'dan, yoksa mock DashMap'ten okur.
    pub fn get_block_at(&self, height: u64) -> Option<MockBlock> {
        if let Some(ref b) = self.backend {
            let hash = b.block_hash_at(height)?;
            let parent_hash = b.block_parent_hash_at(height).unwrap_or([0u8; 32]);
            let timestamp = b.block_timestamp_at(height).unwrap_or(height * 400);
            let tx_hashes = b.block_tx_hashes_at(height);
            let proposer = b.block_proposer_at(height).unwrap_or(0);
            return Some(MockBlock {
                number: height,
                hash: format!("0x{}", hex::encode(hash)),
                parent_hash: format!("0x{}", hex::encode(parent_hash)),
                timestamp,
                transactions: tx_hashes.iter().map(|h| format!("0x{}", hex::encode(h))).collect(),
                validator: format!("0x{:040x}", proposer),
                tx_count: tx_hashes.len() as u32,
            });
        }
        self.blocks.get(&height).map(|b| b.clone())
    }

    /// Hesap bul veya varsayılan döndür
    pub fn get_or_default_account(&self, address: &str) -> MockAccount {
        if let Some(ref b) = self.backend {
            let balance = b.account_balance(address);
            let nonce = b.account_nonce(address);
            if balance > 0 || nonce > 0 {
                return MockAccount {
                    address: address.into(),
                    nonce,
                    balance,
                    guardian_count: 0,
                };
            }
        }
        self.accounts.get(address).map(|a| a.clone()).unwrap_or(MockAccount {
            address: address.into(),
            nonce: 0,
            balance: 0,
            guardian_count: 0,
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_state_new() {
        let state = ChainState::new();
        assert!(state.blocks.contains_key(&0)); // genesis
        assert_eq!(state.best_block_number(), 0);
    }

    #[test]
    fn test_chain_state_produce_block() {
        let state = ChainState::new();
        let b1 = state.produce_block();
        assert_eq!(b1.number, 1);
        assert_eq!(state.best_block_number(), 1);

        let b2 = state.produce_block();
        assert_eq!(b2.number, 2);
        assert_ne!(b1.hash, b2.hash);
    }

    #[test]
    fn test_chain_state_get_account() {
        let state = ChainState::new();
        let acc = state.get_or_default_account("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(acc.balance > 0);
        assert_eq!(acc.nonce, 5);
    }

    #[test]
    fn test_chain_state_default_account() {
        let state = ChainState::new();
        let acc = state.get_or_default_account("0x0000000000000000000000000000000000000099");
        assert_eq!(acc.balance, 0);
        assert_eq!(acc.nonce, 0);
    }

    #[test]
    fn test_chain_state_send_tx() {
        let state = ChainState::new();
        let hash = state.send_transaction(
            "0xaaaa".into(),
            "0xbbbb".into(),
            1_000_000,
            100,
        );
        assert!(hash.starts_with("0x"));
        assert!(state.transactions.contains_key(&hash));
    }
}
