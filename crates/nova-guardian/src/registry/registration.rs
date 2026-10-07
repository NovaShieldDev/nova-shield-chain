//! Guardian kaydı ve deposu — Cosmos SDK x/staking Keeper pattern'inden ilham
//!
//! GuardianRegistry: DashMap tabanlı thread-safe guardian deposu
//! Guardian: kimlik, kategori, stake, durum, itibar özeti

use crate::errors::{GuardianError, GuardianResult};
use crate::registry::categories::{GuardianCategory, KycStatus};
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ─── GuardianStatus ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianStatus {
    /// Aktif — hizmet verebilir
    Active,
    /// Geçici askıya alınmış (slash cezası)
    Jailed { until_slot: u64 },
    /// Kalıcı olarak devre dışı
    Deactivated,
    /// KYC bekliyor (henüz aktif değil)
    PendingVerification,
}

impl GuardianStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, GuardianStatus::Active)
    }

    pub fn is_jailed(&self) -> bool {
        matches!(self, GuardianStatus::Jailed { .. })
    }

    /// Jail süresi doldu mu?
    pub fn is_jail_expired(&self, current_slot: SlotId) -> bool {
        match self {
            GuardianStatus::Jailed { until_slot } => current_slot.0 >= *until_slot,
            _ => false,
        }
    }
}

// ─── Guardian ─────────────────────────────────────────────────────────────────

/// Guardian — marketplace'e kayıtlı güvenilir kişi/kurum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardian {
    pub id: Address,
    pub category: GuardianCategory,
    pub status: GuardianStatus,
    pub kyc_status: KycStatus,

    /// Stake edilen NSC miktarı
    pub staked_amount: u128,
    /// Kayıt slot'u
    pub registered_at_slot: u64,
    /// Kaç hesabın guardian'ı olduğu
    pub accounts_guarding: u32,
    /// Maksimum kabul edebileceği hesap sayısı
    pub max_accounts: u32,
    /// Sigorta fonu katkısı (kurumsal için)
    pub insurance_contribution: u128,
    /// Son aktivite slot'u (yanıt süresi takibi için)
    pub last_activity_slot: u64,
}

impl Guardian {
    pub fn new(
        id: Address,
        category: GuardianCategory,
        staked_amount: u128,
        registered_at_slot: u64,
    ) -> GuardianResult<Self> {
        // Minimum stake kontrolü
        if staked_amount < category.min_stake() {
            return Err(GuardianError::InsufficientStake {
                required: category.min_stake(),
                provided: staked_amount,
            });
        }

        let kyc_status = if category.requires_kyc() {
            KycStatus::Pending
        } else {
            KycStatus::NotRequired
        };

        let status = if category.requires_kyc() {
            GuardianStatus::PendingVerification
        } else {
            GuardianStatus::Active
        };

        Ok(Self {
            id,
            category,
            status,
            kyc_status,
            staked_amount,
            registered_at_slot,
            accounts_guarding: 0,
            max_accounts: 100,
            insurance_contribution: 0,
            last_activity_slot: registered_at_slot,
        })
    }

    pub fn is_available(&self) -> bool {
        self.status.is_active() && self.accounts_guarding < self.max_accounts
    }

    pub fn address_hex(&self) -> String {
        hex::encode(self.id.as_bytes())
    }
}

// ─── GuardianRegistry ─────────────────────────────────────────────────────────

/// GuardianRegistry — tüm guardian'ları tutan DashMap deposu
/// Cosmos SDK Keeper pattern: DashMap backend (prod'da RocksDB olur)
#[derive(Debug, Default, Clone)]
pub struct GuardianRegistry {
    guardians: Arc<DashMap<Address, Guardian>>,
}

impl GuardianRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Guardian kaydı
    pub fn register(&self, guardian: Guardian) -> GuardianResult<()> {
        if self.guardians.contains_key(&guardian.id) {
            return Err(GuardianError::AlreadyRegistered {
                address_hex: guardian.address_hex(),
            });
        }
        self.guardians.insert(guardian.id, guardian);
        Ok(())
    }

    /// Guardian bilgisi al
    pub fn get(&self, id: &Address) -> GuardianResult<Guardian> {
        self.guardians.get(id).map(|r| r.clone()).ok_or(GuardianError::NotFound)
    }

    /// Guardian güncelle
    pub fn update(&self, id: &Address, f: impl FnOnce(&mut Guardian)) -> GuardianResult<()> {
        self.guardians.get_mut(id).map(|mut g| f(&mut g)).ok_or(GuardianError::NotFound)
    }

    /// Aktif guardian'lar
    pub fn active_guardians(&self) -> Vec<Guardian> {
        self.guardians.iter().filter(|e| e.status.is_active()).map(|e| e.clone()).collect()
    }

    /// Kategoriye göre filtrele
    pub fn by_category(&self, category: &GuardianCategory) -> Vec<Guardian> {
        self.guardians.iter().filter(|e| &e.category == category).map(|e| e.clone()).collect()
    }

    /// Toplam kayıtlı guardian sayısı
    pub fn count(&self) -> usize {
        self.guardians.len()
    }

    /// Guardian jail'e al — Cosmos x/slashing pattern
    pub fn jail(&self, id: &Address, until_slot: u64) -> GuardianResult<()> {
        self.update(id, |g| {
            g.status = GuardianStatus::Jailed { until_slot };
        })
    }

    /// Jail süresini kontrol et ve serbest bırak
    pub fn try_unjail(&self, id: &Address, current_slot: SlotId) -> GuardianResult<bool> {
        let guardian = self.get(id)?;
        if guardian.status.is_jail_expired(current_slot) {
            self.update(id, |g| g.status = GuardianStatus::Active)?;
            return Ok(true);
        }
        Ok(false)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    fn personal_guardian(n: u8) -> Guardian {
        Guardian::new(addr(n), GuardianCategory::Personal, 0, 0).unwrap()
    }

    fn certified_guardian(n: u8) -> Guardian {
        Guardian::new(
            addr(n),
            GuardianCategory::CertifiedIndividual,
            crate::registry::categories::MIN_STAKE_CERTIFIED_INDIVIDUAL,
            0,
        )
        .unwrap()
    }

    #[test]
    fn test_guardian_register() {
        let registry = GuardianRegistry::new();
        registry.register(personal_guardian(1)).unwrap();
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_guardian_duplicate_rejected() {
        let registry = GuardianRegistry::new();
        registry.register(personal_guardian(1)).unwrap();
        let err = registry.register(personal_guardian(1));
        assert!(matches!(err, Err(GuardianError::AlreadyRegistered { .. })));
    }

    #[test]
    fn test_guardian_insufficient_stake() {
        let err = Guardian::new(
            addr(1),
            GuardianCategory::CertifiedIndividual,
            100, // çok az
            0,
        );
        assert!(matches!(err, Err(GuardianError::InsufficientStake { .. })));
    }

    #[test]
    fn test_guardian_get() {
        let registry = GuardianRegistry::new();
        registry.register(personal_guardian(5)).unwrap();
        let g = registry.get(&addr(5)).unwrap();
        assert_eq!(g.id, addr(5));
    }

    #[test]
    fn test_guardian_not_found() {
        let registry = GuardianRegistry::new();
        assert!(matches!(registry.get(&addr(99)), Err(GuardianError::NotFound)));
    }

    #[test]
    fn test_guardian_jail_and_unjail() {
        let registry = GuardianRegistry::new();
        registry.register(personal_guardian(1)).unwrap();
        registry.jail(&addr(1), 1000).unwrap();
        let g = registry.get(&addr(1)).unwrap();
        assert!(g.status.is_jailed());

        // Henüz serbest değil
        let released = registry.try_unjail(&addr(1), SlotId(500)).unwrap();
        assert!(!released);

        // Süre doldu
        let released = registry.try_unjail(&addr(1), SlotId(1000)).unwrap();
        assert!(released);
    }

    #[test]
    fn test_active_guardians_filter() {
        let registry = GuardianRegistry::new();
        registry.register(personal_guardian(1)).unwrap();
        registry.register(personal_guardian(2)).unwrap();
        registry.jail(&addr(2), 9999).unwrap();
        let active = registry.active_guardians();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_certified_guardian_starts_active() {
        let g = certified_guardian(1);
        assert!(g.status.is_active()); // KYC zorunlu değil
    }

    #[test]
    fn test_institutional_guardian_starts_pending() {
        let g = Guardian::new(
            addr(1),
            GuardianCategory::Institutional,
            crate::registry::categories::MIN_STAKE_INSTITUTIONAL,
            0,
        ).unwrap();
        assert!(matches!(g.status, GuardianStatus::PendingVerification));
    }
}
