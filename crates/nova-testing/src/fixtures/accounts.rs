//! Hesap fixture'ları

use serde::{Deserialize, Serialize};

/// Test hesabı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAccount {
    pub name: &'static str,
    pub address: [u8; 20],
    pub private_key: [u8; 32],
    pub balance: u128,
    pub nonce: u64,
}

impl TestAccount {
    pub fn address_hex(&self) -> String {
        format!("0x{}", hex_encode(&self.address))
    }

    pub fn sign_mock(&self, message: &[u8]) -> [u8; 64] {
        let mut h = blake3::Hasher::new();
        h.update(b"nova-mock-sign");
        h.update(&self.private_key);
        h.update(message);
        let bytes = h.finalize();
        let mut sig = [0u8; 64];
        sig[..32].copy_from_slice(bytes.as_bytes());
        sig[32..].copy_from_slice(bytes.as_bytes());
        sig
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Alice — birincil test hesabı (1000 NSC)
pub fn alice() -> TestAccount {
    TestAccount {
        name: "Alice",
        address: [0xAAu8; 20],
        private_key: [0x11u8; 32],
        balance: 1_000_000_000_000_000_000_000, // 1000 NSC
        nonce: 0,
    }
}

/// Bob — ikincil test hesabı (500 NSC)
pub fn bob() -> TestAccount {
    TestAccount {
        name: "Bob",
        address: [0xBBu8; 20],
        private_key: [0x22u8; 32],
        balance: 500_000_000_000_000_000_000,
        nonce: 0,
    }
}

/// Charlie — üçüncül test hesabı (sıfır bakiye)
pub fn charlie() -> TestAccount {
    TestAccount {
        name: "Charlie",
        address: [0xCCu8; 20],
        private_key: [0x33u8; 32],
        balance: 0,
        nonce: 0,
    }
}

/// Validator hesabı — staker
pub fn validator_account() -> TestAccount {
    TestAccount {
        name: "Validator-1",
        address: [0x01u8; 20],
        private_key: [0xFFu8; 32],
        balance: 100_000_000_000_000_000_000_000, // 100_000 NSC stake
        nonce: 0,
    }
}

/// Tüm standart test hesapları
pub struct TestAccounts {
    pub alice: TestAccount,
    pub bob: TestAccount,
    pub charlie: TestAccount,
    pub validator: TestAccount,
}

impl TestAccounts {
    pub fn standard() -> Self {
        Self {
            alice: alice(),
            bob: bob(),
            charlie: charlie(),
            validator: validator_account(),
        }
    }

    pub fn all(&self) -> Vec<&TestAccount> {
        vec![&self.alice, &self.bob, &self.charlie, &self.validator]
    }

    pub fn get_by_name(&self, name: &str) -> Option<&TestAccount> {
        self.all().into_iter().find(|a| a.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alice_fixture() {
        let a = alice();
        assert_eq!(a.name, "Alice");
        assert_eq!(a.address, [0xAAu8; 20]);
        assert!(a.balance > 0);
    }

    #[test]
    fn test_charlie_zero_balance() {
        let c = charlie();
        assert_eq!(c.balance, 0);
    }

    #[test]
    fn test_sign_mock_deterministic() {
        let a = alice();
        let msg = b"test message";
        let s1 = a.sign_mock(msg);
        let s2 = a.sign_mock(msg);
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_sign_mock_different_keys() {
        let a = alice();
        let b = bob();
        let msg = b"test message";
        assert_ne!(a.sign_mock(msg), b.sign_mock(msg));
    }

    #[test]
    fn test_test_accounts_standard() {
        let acc = TestAccounts::standard();
        assert_eq!(acc.all().len(), 4);
        assert!(acc.get_by_name("Alice").is_some());
        assert!(acc.get_by_name("Unknown").is_none());
    }

    #[test]
    fn test_address_hex_format() {
        let a = alice();
        let hex = a.address_hex();
        assert!(hex.starts_with("0x"));
        assert_eq!(hex.len(), 42);
    }
}
