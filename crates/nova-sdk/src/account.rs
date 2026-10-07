//! MPC hesap yönetimi SDK
//!
//! Kullanım:
//! ```ignore
//! let account = NovaAccountBuilder::new()
//!     .with_guardian([0x01;20], "Alice")
//!     .with_guardian([0x02;20], "Bob")
//!     .with_threshold(2)
//!     .build();
//! ```

use crate::errors::{SdkError, SdkResult};
use serde::{Deserialize, Serialize};

// ─── GuardianSpec ─────────────────────────────────────────────────────────────

/// Guardian spesifikasyonu
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianSpec {
    pub address: [u8; 20],
    pub name: String,
    pub role: GuardianRole,
}

/// Guardian rolü
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianRole {
    Personal,
    Institutional,
    Hardware,
}

// ─── NovaAccount ──────────────────────────────────────────────────────────────

/// Nova Shield hesabı — MPC tabanlı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NovaAccount {
    /// Hesap adresi (BLAKE3 tabanlı)
    pub address: [u8; 20],
    /// Hesap kimliği
    pub account_id: u64,
    /// Guardian listesi
    pub guardians: Vec<GuardianSpec>,
    /// İmza eşiği (t/n)
    pub threshold: u32,
    /// Nonce
    pub nonce: u64,
    /// Bakiye (NSC, en küçük birim)
    pub balance: u64,
    /// Oluşturulma slotu
    pub created_at_slot: u64,
}

impl NovaAccount {
    /// Adres görüntüleme (EIP-55 benzeri)
    pub fn address_hex(&self) -> String {
        format!("0x{}", hex::encode(self.address))
    }

    /// İnsan-okunabilir bakiye (NSC)
    pub fn balance_nsc(&self) -> f64 {
        self.balance as f64 / 1_000_000_000_000_000_000.0
    }
}

// ─── NovaAccountBuilder ───────────────────────────────────────────────────────

/// Builder pattern ile hesap oluşturma
pub struct NovaAccountBuilder {
    guardians: Vec<GuardianSpec>,
    threshold: Option<u32>,
    seed: Option<[u8; 32]>,
}

impl NovaAccountBuilder {
    pub fn new() -> Self {
        Self { guardians: vec![], threshold: None, seed: None }
    }

    /// Guardian ekle
    pub fn with_guardian(
        mut self,
        address: [u8; 20],
        name: impl Into<String>,
    ) -> Self {
        self.guardians.push(GuardianSpec {
            address,
            name: name.into(),
            role: GuardianRole::Personal,
        });
        self
    }

    /// Kurumsal guardian ekle
    pub fn with_institutional_guardian(
        mut self,
        address: [u8; 20],
        name: impl Into<String>,
    ) -> Self {
        self.guardians.push(GuardianSpec {
            address,
            name: name.into(),
            role: GuardianRole::Institutional,
        });
        self
    }

    /// Eşik sayısı belirle
    pub fn with_threshold(mut self, t: u32) -> Self {
        self.threshold = Some(t);
        self
    }

    /// Rastgele seed belirle (test için)
    pub fn with_seed(mut self, seed: [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Hesabı oluştur
    pub fn build(self) -> SdkResult<NovaAccount> {
        let n = self.guardians.len() as u32;
        if n == 0 {
            return Err(SdkError::InvalidParameter {
                param: "guardians".into(),
                reason: "en az 1 guardian gerekli".into(),
            });
        }

        let threshold = self.threshold.unwrap_or((n + 1) / 2);
        if threshold == 0 || threshold > n {
            return Err(SdkError::InvalidParameter {
                param: "threshold".into(),
                reason: format!("eşik {} geçersiz, 1..{} aralığında olmalı", threshold, n),
            });
        }

        // Adres türet: BLAKE3(guardians || threshold)
        let seed = self.seed.unwrap_or([0u8; 32]);
        let flat: Vec<u8> = self.guardians.iter()
            .flat_map(|g| g.address.iter().copied())
            .collect();
        let hash = blake3_hash_many(&[b"nova-account-addr-v1", &seed, &flat, &threshold.to_le_bytes()]);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[..20]);

        Ok(NovaAccount {
            address: addr,
            account_id: u64::from_le_bytes(hash[20..28].try_into().unwrap()),
            guardians: self.guardians,
            threshold,
            nonce: 0,
            balance: 0,
            created_at_slot: 0,
        })
    }
}

// ─── AccountManager ───────────────────────────────────────────────────────────

/// Hesap yöneticisi
pub struct AccountManager {
    accounts: std::collections::HashMap<[u8; 20], NovaAccount>,
    next_slot: u64,
}

impl AccountManager {
    pub fn new() -> Self {
        Self { accounts: std::collections::HashMap::new(), next_slot: 1 }
    }

    /// Hesap ekle
    pub fn register(&mut self, mut account: NovaAccount) -> [u8; 20] {
        account.created_at_slot = self.next_slot;
        self.next_slot += 1;
        let addr = account.address;
        self.accounts.insert(addr, account);
        addr
    }

    /// Hesap al
    pub fn get(&self, address: &[u8; 20]) -> SdkResult<&NovaAccount> {
        self.accounts.get(address).ok_or_else(|| SdkError::AccountNotFound {
            address: format!("0x{}", hex::encode(address)),
        })
    }

    /// Bakiye güncelle
    pub fn credit(&mut self, address: &[u8; 20], amount: u64) -> SdkResult<()> {
        self.accounts.get_mut(address)
            .ok_or_else(|| SdkError::AccountNotFound { address: hex::encode(address) })?
            .balance += amount;
        Ok(())
    }

    /// Bakiye düş
    pub fn debit(&mut self, address: &[u8; 20], amount: u64) -> SdkResult<()> {
        let acc = self.accounts.get_mut(address)
            .ok_or_else(|| SdkError::AccountNotFound { address: hex::encode(address) })?;
        if acc.balance < amount {
            return Err(SdkError::InsufficientBalance {
                available: acc.balance,
                required: amount,
            });
        }
        acc.balance -= amount;
        Ok(())
    }

    /// Nonce artır
    pub fn increment_nonce(&mut self, address: &[u8; 20]) -> SdkResult<u64> {
        let acc = self.accounts.get_mut(address)
            .ok_or_else(|| SdkError::AccountNotFound { address: hex::encode(address) })?;
        acc.nonce += 1;
        Ok(acc.nonce)
    }

    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }
}

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

fn blake3_hash_many(inputs: &[&[u8]]) -> [u8; 32] {
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

    fn g(n: u8) -> [u8; 20] {
        [n; 20]
    }

    #[test]
    fn test_account_builder_basic() {
        let acc = NovaAccountBuilder::new()
            .with_guardian(g(1), "Alice")
            .with_guardian(g(2), "Bob")
            .with_threshold(2)
            .build()
            .unwrap();

        assert_eq!(acc.guardians.len(), 2);
        assert_eq!(acc.threshold, 2);
        assert_ne!(acc.address, [0u8; 20]);
    }

    #[test]
    fn test_account_builder_default_threshold() {
        let acc = NovaAccountBuilder::new()
            .with_guardian(g(1), "Alice")
            .with_guardian(g(2), "Bob")
            .with_guardian(g(3), "Carol")
            .build()
            .unwrap();

        // Varsayılan eşik: ceil(n/2) = 2
        assert_eq!(acc.threshold, 2);
    }

    #[test]
    fn test_account_builder_no_guardians() {
        let result = NovaAccountBuilder::new().build();
        assert!(matches!(result, Err(SdkError::InvalidParameter { .. })));
    }

    #[test]
    fn test_account_builder_invalid_threshold() {
        let result = NovaAccountBuilder::new()
            .with_guardian(g(1), "Alice")
            .with_threshold(5) // 5 > 1
            .build();
        assert!(matches!(result, Err(SdkError::InvalidParameter { .. })));
    }

    #[test]
    fn test_account_address_deterministic() {
        let seed = [0x42u8; 32];
        let a1 = NovaAccountBuilder::new()
            .with_guardian(g(1), "A")
            .with_seed(seed)
            .build()
            .unwrap();
        let a2 = NovaAccountBuilder::new()
            .with_guardian(g(1), "A")
            .with_seed(seed)
            .build()
            .unwrap();
        assert_eq!(a1.address, a2.address);
    }

    #[test]
    fn test_account_address_hex() {
        let acc = NovaAccountBuilder::new()
            .with_guardian(g(1), "A")
            .build()
            .unwrap();
        assert!(acc.address_hex().starts_with("0x"));
        assert_eq!(acc.address_hex().len(), 42);
    }

    #[test]
    fn test_account_institutional_guardian() {
        let acc = NovaAccountBuilder::new()
            .with_institutional_guardian(g(1), "Fireblocks")
            .build()
            .unwrap();
        assert_eq!(acc.guardians[0].role, GuardianRole::Institutional);
    }

    #[test]
    fn test_account_manager_register() {
        let mut mgr = AccountManager::new();
        let acc = NovaAccountBuilder::new()
            .with_guardian(g(1), "A")
            .build()
            .unwrap();
        let addr = mgr.register(acc);
        let found = mgr.get(&addr).unwrap();
        assert_eq!(found.address, addr);
    }

    #[test]
    fn test_account_manager_credit_debit() {
        let mut mgr = AccountManager::new();
        let acc = NovaAccountBuilder::new().with_guardian(g(1), "A").build().unwrap();
        let addr = mgr.register(acc);

        mgr.credit(&addr, 1_000_000).unwrap();
        assert_eq!(mgr.get(&addr).unwrap().balance, 1_000_000);

        mgr.debit(&addr, 400_000).unwrap();
        assert_eq!(mgr.get(&addr).unwrap().balance, 600_000);
    }

    #[test]
    fn test_account_manager_insufficient_balance() {
        let mut mgr = AccountManager::new();
        let acc = NovaAccountBuilder::new().with_guardian(g(1), "A").build().unwrap();
        let addr = mgr.register(acc);

        let result = mgr.debit(&addr, 100);
        assert!(matches!(result, Err(SdkError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_account_manager_nonce() {
        let mut mgr = AccountManager::new();
        let acc = NovaAccountBuilder::new().with_guardian(g(1), "A").build().unwrap();
        let addr = mgr.register(acc);

        assert_eq!(mgr.increment_nonce(&addr).unwrap(), 1);
        assert_eq!(mgr.increment_nonce(&addr).unwrap(), 2);
    }

    #[test]
    fn test_account_not_found() {
        let mgr = AccountManager::new();
        let result = mgr.get(&[0u8; 20]);
        assert!(matches!(result, Err(SdkError::AccountNotFound { .. })));
    }
}
