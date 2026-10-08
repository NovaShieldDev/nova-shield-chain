//! GÖREV 100: Virtual Accounts — Smart Contract Hesap Soyutlaması
//!
//! ## Mimari
//!
//! ### Cosmos SDK Module Accounts
//! - `ModuleAccount`: özel izinleri olan yerleşik hesaplar
//! - `BaseAccount` → `ModuleAccount` (minter, burner, staking izinleri)
//! - `AccountKeeper::GetModuleAccount(ctx, name) -> AccountI`
//!
//! ### EIP-4337 Account Abstraction (Ethereum)
//! - `EntryPoint` kontratı üzerinden tüm işlemler
//! - `UserOperation` (nonce, callData, paymaster, signature)
//! - Paymaster: başka birinin gas ödemesi
//!
//! ### Nova Uyarlaması
//! - `VirtualAccount`: özel izin setiyle hesap soyutlaması
//! - `VirtualAccountKind`: Normal, Module, Multisig, Escrow, Treasury
//! - `VirtualAccountRegistry`: tüm virtual account'ları yönetir
//! - `AccountPermission`: hangi işlemlere izin var?

use blake3;
use nova_primitives::Address;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

// ─── Sabitler ─────────────────────────────────────────────────────────────────

/// Staking module adresi (deterministik)
pub const STAKING_MODULE_ADDR: [u8; 20] = [0x10u8; 20];

/// Treasury module adresi
pub const TREASURY_MODULE_ADDR: [u8; 20] = [0x20u8; 20];

/// Burn module adresi
pub const BURN_MODULE_ADDR: [u8; 20] = [0x30u8; 20];

/// MPC module adresi
pub const MPC_MODULE_ADDR: [u8; 20] = [0x40u8; 20];

/// Governance module adresi
pub const GOVERNANCE_MODULE_ADDR: [u8; 20] = [0x50u8; 20];

/// Runtime upgrade module adresi
pub const UPGRADE_MODULE_ADDR: [u8; 20] = [0x60u8; 20];

/// Maksimum multisig imzacı sayısı
pub const MAX_MULTISIG_SIGNERS: usize = 20;

/// Maksimum module account sayısı
pub const MAX_MODULE_ACCOUNTS: usize = 100;

// ─── AccountPermission ────────────────────────────────────────────────────────

/// Hesap izinleri
///
/// Cosmos SDK: `minter`, `burner`, `staking` permissions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AccountPermission {
    /// Token basabilir
    Mint,
    /// Token yakabilir
    Burn,
    /// Stake edebilir
    Stake,
    /// Token transfer edebilir
    Transfer,
    /// Governance önerisi gönderebilir
    Propose,
    /// Hesabı dondurabilir
    Freeze,
    /// Diğer hesaplar adına işlem yapabilir
    Delegate,
}

// ─── VirtualAccountKind ───────────────────────────────────────────────────────

/// Virtual account türü
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualAccountKind {
    /// Normal kullanıcı hesabı (soyutlanmış)
    Normal,
    /// Yerleşik modül hesabı (staking, treasury, vb.)
    Module { name: String },
    /// Çok imzalı hesap
    Multisig { threshold: u32, signers: Vec<[u8; 20]> },
    /// Emanet hesabı (escrow — iki taraf anlaşınca serbest)
    Escrow { beneficiary: [u8; 20], arbiter: [u8; 20] },
    /// Hazine hesabı (governance kontrolünde)
    Treasury,
    /// Paymaster (EIP-4337: başkasının gas'ını ödeyen)
    Paymaster { sponsored_accounts: Vec<[u8; 20]> },
}

// ─── VirtualAccount ───────────────────────────────────────────────────────────

/// Soyutlanmış hesap
///
/// EIP-4337: `IAccount` + Cosmos: `ModuleAccount`
#[derive(Debug, Clone)]
pub struct VirtualAccount {
    /// Hesap adresi
    pub address: Address,
    /// Hesap türü
    pub kind: VirtualAccountKind,
    /// İzin seti
    pub permissions: HashSet<AccountPermission>,
    /// Nonce (replay protection)
    pub nonce: u64,
    /// Bakiye
    pub balance: u128,
    /// Oluşturulma sloту
    pub created_at_slot: u64,
    /// Aktif mi?
    pub is_active: bool,
}

impl VirtualAccount {
    /// Yeni hesap oluştur
    pub fn new(address: Address, kind: VirtualAccountKind, created_at_slot: u64) -> Self {
        let permissions = default_permissions_for(&kind);
        Self {
            address,
            kind,
            permissions,
            nonce: 0,
            balance: 0,
            created_at_slot,
            is_active: true,
        }
    }

    /// İzin kontrolü
    pub fn has_permission(&self, perm: &AccountPermission) -> bool {
        self.permissions.contains(perm)
    }

    /// İzin ekle
    pub fn grant_permission(&mut self, perm: AccountPermission) {
        self.permissions.insert(perm);
    }

    /// İzin iptal
    pub fn revoke_permission(&mut self, perm: &AccountPermission) {
        self.permissions.remove(perm);
    }

    /// Nonce'u artır
    pub fn increment_nonce(&mut self) -> u64 {
        self.nonce += 1;
        self.nonce
    }

    /// Hesabın multisig eşiğini kontrol et
    ///
    /// NOVA-RED-RUNTIME-009: tekrarlı imzacı önlendi — aynı signer 2 kez sayılamaz.
    pub fn check_multisig_threshold(&self, provided_sigs: &[[u8; 20]]) -> bool {
        match &self.kind {
            VirtualAccountKind::Multisig { threshold, signers } => {
                // Önce deduplicate et — aynı imzacı threshold'ı iki kez karşılayamaz
                let mut unique_sigs = provided_sigs.to_vec();
                unique_sigs.sort();
                unique_sigs.dedup();

                let valid_count = unique_sigs.iter()
                    .filter(|s| signers.contains(s))
                    .count();
                valid_count >= *threshold as usize
            }
            _ => true, // Multisig değil → tek imza yeterli
        }
    }

    /// Module hesabı adı
    pub fn module_name(&self) -> Option<&str> {
        match &self.kind {
            VirtualAccountKind::Module { name } => Some(name),
            _ => None,
        }
    }
}

fn default_permissions_for(kind: &VirtualAccountKind) -> HashSet<AccountPermission> {
    let mut perms = HashSet::new();
    match kind {
        VirtualAccountKind::Normal => {
            perms.insert(AccountPermission::Transfer);
            perms.insert(AccountPermission::Stake);
            perms.insert(AccountPermission::Propose);
        }
        VirtualAccountKind::Module { name } => {
            perms.insert(AccountPermission::Transfer);
            if name == "staking" {
                perms.insert(AccountPermission::Mint);
                perms.insert(AccountPermission::Burn);
                perms.insert(AccountPermission::Stake);
            } else if name == "treasury" {
                perms.insert(AccountPermission::Transfer);
            } else if name == "burn" {
                perms.insert(AccountPermission::Burn);
            }
        }
        VirtualAccountKind::Treasury => {
            perms.insert(AccountPermission::Transfer);
        }
        VirtualAccountKind::Multisig { .. } => {
            perms.insert(AccountPermission::Transfer);
            perms.insert(AccountPermission::Propose);
        }
        VirtualAccountKind::Escrow { .. } => {
            perms.insert(AccountPermission::Transfer);
        }
        VirtualAccountKind::Paymaster { .. } => {
            perms.insert(AccountPermission::Transfer);
        }
    }
    perms
}

// ─── VirtualAccountRegistry ───────────────────────────────────────────────────

/// Virtual account kayıt defteri
///
/// Cosmos SDK: `AccountKeeper`
/// EIP-4337: `EntryPoint` registry
pub struct VirtualAccountRegistry {
    inner: Mutex<VirtualAccountRegistryInner>,
}

struct VirtualAccountRegistryInner {
    accounts: HashMap<[u8; 20], VirtualAccount>,
    module_names: HashMap<String, [u8; 20]>, // name → address
    total_created: u64,
}

impl VirtualAccountRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            inner: Mutex::new(VirtualAccountRegistryInner {
                accounts: HashMap::new(),
                module_names: HashMap::new(),
                total_created: 0,
            }),
        };
        // Yerleşik module hesaplarını kaydet
        registry.init_module_accounts();
        registry
    }

    fn init_module_accounts(&mut self) {
        let modules = [
            (STAKING_MODULE_ADDR, "staking"),
            (TREASURY_MODULE_ADDR, "treasury"),
            (BURN_MODULE_ADDR, "burn"),
            (MPC_MODULE_ADDR, "mpc"),
            (GOVERNANCE_MODULE_ADDR, "governance"),
            (UPGRADE_MODULE_ADDR, "upgrade"),
        ];

        let mut inner = self.inner.lock().unwrap();
        for (addr_bytes, name) in modules {
            let addr = Address::from_bytes(addr_bytes);
            let account = VirtualAccount::new(
                addr,
                VirtualAccountKind::Module { name: name.to_string() },
                0,
            );
            inner.accounts.insert(addr_bytes, account);
            inner.module_names.insert(name.to_string(), addr_bytes);
            inner.total_created += 1;
        }
    }

    /// Normal virtual account oluştur
    pub fn create(
        &self,
        kind: VirtualAccountKind,
        slot: u64,
    ) -> VirtualAccountResult<Address> {
        let mut inner = self.inner.lock().unwrap();

        if inner.accounts.len() >= MAX_MODULE_ACCOUNTS * 100 {
            return Err(VirtualAccountError::RegistryFull);
        }

        // Deterministik adres: BLAKE3(slot || count)
        let count = inner.total_created;
        let addr_bytes_32 = *blake3::hash(&[
            b"nova-virtual-account:",
            slot.to_le_bytes().as_slice(),
            count.to_le_bytes().as_slice(),
        ].concat()).as_bytes();
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&addr_bytes_32[0..20]);
        let addr = Address::from_bytes(addr_bytes);

        if inner.accounts.contains_key(&addr_bytes) {
            return Err(VirtualAccountError::AlreadyExists);
        }

        let account = VirtualAccount::new(addr, kind, slot);
        inner.accounts.insert(addr_bytes, account);
        inner.total_created += 1;
        Ok(addr)
    }

    /// Hesabı al
    pub fn get(&self, addr: &Address) -> Option<VirtualAccount> {
        self.inner.lock().unwrap()
            .accounts.get(addr.as_bytes())
            .cloned()
    }

    /// Module hesabını isimle al
    pub fn get_module(&self, name: &str) -> Option<VirtualAccount> {
        let inner = self.inner.lock().unwrap();
        let addr = inner.module_names.get(name)?;
        inner.accounts.get(addr).cloned()
    }

    /// Hesabı güncelle
    pub fn update<F>(&self, addr: &Address, f: F) -> VirtualAccountResult<()>
    where F: FnOnce(&mut VirtualAccount)
    {
        let mut inner = self.inner.lock().unwrap();
        let account = inner.accounts.get_mut(addr.as_bytes())
            .ok_or(VirtualAccountError::NotFound)?;
        f(account);
        Ok(())
    }

    /// Toplam hesap sayısı
    pub fn total_count(&self) -> usize {
        self.inner.lock().unwrap().accounts.len()
    }

    /// Toplam oluşturulan hesap sayısı
    pub fn total_created(&self) -> u64 {
        self.inner.lock().unwrap().total_created
    }

    /// Bakiye güncelle
    pub fn deposit(&self, addr: &Address, amount: u128) -> VirtualAccountResult<u128> {
        self.update(addr, |acc| { acc.balance = acc.balance.saturating_add(amount); })?;
        Ok(self.get(addr).map(|a| a.balance).unwrap_or(0))
    }

    pub fn withdraw(&self, addr: &Address, amount: u128) -> VirtualAccountResult<u128> {
        let mut inner = self.inner.lock().unwrap();
        let account = inner.accounts.get_mut(addr.as_bytes())
            .ok_or(VirtualAccountError::NotFound)?;
        if account.balance < amount {
            return Err(VirtualAccountError::InsufficientBalance);
        }
        account.balance -= amount;
        Ok(account.balance)
    }
}

impl Default for VirtualAccountRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualAccountError {
    NotFound,
    AlreadyExists,
    RegistryFull,
    InsufficientBalance,
    PermissionDenied,
    InvalidMultisig,
}

pub type VirtualAccountResult<T> = Result<T, VirtualAccountError>;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    // 1. Module hesapları otomatik oluşturulur
    #[test]
    fn test_module_accounts_initialized() {
        let reg = VirtualAccountRegistry::new();
        let staking = reg.get_module("staking").unwrap();
        assert_eq!(staking.module_name(), Some("staking"));
        assert!(staking.has_permission(&AccountPermission::Mint));
        assert!(staking.has_permission(&AccountPermission::Burn));
    }

    // 2. Normal account oluşturma
    #[test]
    fn test_create_normal_account() {
        let reg = VirtualAccountRegistry::new();
        let addr = reg.create(VirtualAccountKind::Normal, 100).unwrap();
        let acc = reg.get(&addr).unwrap();
        assert!(acc.is_active);
        assert!(acc.has_permission(&AccountPermission::Transfer));
    }

    // 3. Multisig account
    #[test]
    fn test_create_multisig_account() {
        let reg = VirtualAccountRegistry::new();
        let signers = vec![[1u8; 20], [2u8; 20], [3u8; 20]];
        let addr = reg.create(
            VirtualAccountKind::Multisig { threshold: 2, signers: signers.clone() },
            0,
        ).unwrap();

        let acc = reg.get(&addr).unwrap();
        // 2 geçerli imza → yeterli
        assert!(acc.check_multisig_threshold(&[[1u8; 20], [2u8; 20]]));
        // 1 geçerli imza → yetersiz
        assert!(!acc.check_multisig_threshold(&[[1u8; 20]]));
        // Yanlış imzalar
        assert!(!acc.check_multisig_threshold(&[[9u8; 20], [8u8; 20]]));
    }

    /// NOVA-RED-RUNTIME-009: tekrarlı imzacı ile threshold bypass edilememeli
    #[test]
    fn test_multisig_duplicate_signer_cannot_bypass_threshold() {
        let reg = VirtualAccountRegistry::new();
        let signers = vec![[1u8; 20], [2u8; 20], [3u8; 20]];
        let addr = reg.create(
            VirtualAccountKind::Multisig { threshold: 2, signers: signers.clone() },
            0,
        ).unwrap();

        let acc = reg.get(&addr).unwrap();
        // Signer 1 iki kez → threshold bypass denemesi
        assert!(
            !acc.check_multisig_threshold(&[[1u8; 20], [1u8; 20]]),
            "aynı imzacı 2 kez verildi, threshold karşılanmamalı"
        );
    }

    /// NOVA-RED-RUNTIME-009: 3 aynı imzacı → hala reddedilmeli
    #[test]
    fn test_multisig_triple_duplicate_signer_rejected() {
        let reg = VirtualAccountRegistry::new();
        let addr = reg.create(
            VirtualAccountKind::Multisig {
                threshold: 2,
                signers: vec![[1u8; 20], [2u8; 20]],
            },
            0,
        ).unwrap();

        let acc = reg.get(&addr).unwrap();
        assert!(
            !acc.check_multisig_threshold(&[[1u8; 20], [1u8; 20], [1u8; 20]]),
            "3x aynı imzacı threshold karşılamamalı"
        );
    }

    // 4. Escrow account
    #[test]
    fn test_create_escrow_account() {
        let reg = VirtualAccountRegistry::new();
        let addr = reg.create(
            VirtualAccountKind::Escrow { beneficiary: [1u8; 20], arbiter: [2u8; 20] },
            50,
        ).unwrap();
        let acc = reg.get(&addr).unwrap();
        assert!(matches!(acc.kind, VirtualAccountKind::Escrow { .. }));
    }

    // 5. İzin verme ve iptal
    #[test]
    fn test_grant_revoke_permission() {
        let reg = VirtualAccountRegistry::new();
        let a = reg.create(VirtualAccountKind::Normal, 0).unwrap();

        reg.update(&a, |acc| {
            acc.grant_permission(AccountPermission::Mint);
        }).unwrap();

        let acc = reg.get(&a).unwrap();
        assert!(acc.has_permission(&AccountPermission::Mint));

        reg.update(&a, |acc| {
            acc.revoke_permission(&AccountPermission::Mint);
        }).unwrap();

        let acc = reg.get(&a).unwrap();
        assert!(!acc.has_permission(&AccountPermission::Mint));
    }

    // 6. Bakiye deposit/withdraw
    #[test]
    fn test_deposit_and_withdraw() {
        let reg = VirtualAccountRegistry::new();
        let a = reg.create(VirtualAccountKind::Normal, 0).unwrap();

        let after_deposit = reg.deposit(&a, 1_000_000).unwrap();
        assert_eq!(after_deposit, 1_000_000);

        let after_withdraw = reg.withdraw(&a, 300_000).unwrap();
        assert_eq!(after_withdraw, 700_000);
    }

    // 7. Yetersiz bakiye hatası
    #[test]
    fn test_withdraw_insufficient_balance() {
        let reg = VirtualAccountRegistry::new();
        let a = reg.create(VirtualAccountKind::Normal, 0).unwrap();
        reg.deposit(&a, 100).unwrap();
        let err = reg.withdraw(&a, 200).unwrap_err();
        assert_eq!(err, VirtualAccountError::InsufficientBalance);
    }

    // 8. Bulunamayan hesap hatası
    #[test]
    fn test_get_nonexistent_account() {
        let reg = VirtualAccountRegistry::new();
        assert!(reg.get(&addr(0xFF)).is_none());
    }

    // 9. Nonce artışı
    #[test]
    fn test_nonce_increment() {
        let reg = VirtualAccountRegistry::new();
        let a = reg.create(VirtualAccountKind::Normal, 0).unwrap();

        let n1 = {
            let mut inner = reg.inner.lock().unwrap();
            inner.accounts.get_mut(a.as_bytes()).unwrap().increment_nonce()
        };
        assert_eq!(n1, 1);

        let n2 = {
            let mut inner = reg.inner.lock().unwrap();
            inner.accounts.get_mut(a.as_bytes()).unwrap().increment_nonce()
        };
        assert_eq!(n2, 2);
    }

    // 10. Treasury account izinleri
    #[test]
    fn test_treasury_account_permissions() {
        let reg = VirtualAccountRegistry::new();
        let a = reg.create(VirtualAccountKind::Treasury, 0).unwrap();
        let acc = reg.get(&a).unwrap();
        assert!(acc.has_permission(&AccountPermission::Transfer));
        // Treasury mint yetkisi yok
        assert!(!acc.has_permission(&AccountPermission::Mint));
    }

    // 11. Paymaster account
    #[test]
    fn test_paymaster_account() {
        let reg = VirtualAccountRegistry::new();
        let a = reg.create(
            VirtualAccountKind::Paymaster { sponsored_accounts: vec![[9u8; 20]] },
            0,
        ).unwrap();
        let acc = reg.get(&a).unwrap();
        assert!(acc.has_permission(&AccountPermission::Transfer));
    }

    // 12. Total count
    #[test]
    fn test_total_count() {
        let reg = VirtualAccountRegistry::new();
        let initial = reg.total_count(); // 4 module accounts

        reg.create(VirtualAccountKind::Normal, 0).unwrap();
        reg.create(VirtualAccountKind::Normal, 1).unwrap();
        assert_eq!(reg.total_count(), initial + 2);
    }

    // 13. Module hesabı adres sabitleri
    #[test]
    fn test_module_address_constants() {
        let reg = VirtualAccountRegistry::new();
        let staking_addr = Address::from_bytes(STAKING_MODULE_ADDR);
        let acc = reg.get(&staking_addr).unwrap();
        assert_eq!(acc.module_name(), Some("staking"));
    }
}
