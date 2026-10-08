//! Cross-chain miras entegrasyonu — §5.6.4
//!
//! "Cross-chain varlıklar da deadman switch kapsamına girer:
//!  BTC, ETH, SOL — hepsi tek inheritance plan'a dahil edilebilir"

use crate::errors::{GuardError, GuardResult};
use crate::types::{AssetAddress, ChainId, CrossChainAsset};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

// ─── CrossChainHeirAllocation ─────────────────────────────────────────────────

/// Mirasçı ve zincir başına pay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainHeirAllocation {
    /// Mirasçı adı
    pub heir_name: String,
    /// Her zincirdeki alım adresleri
    pub addresses: Vec<AssetAddress>,
    /// Toplam bakiyeden pay yüzdesi (bps, 10000=100%)
    pub share_bps: u32,
}

// ─── CrossChainInheritancePlan ────────────────────────────────────────────────

/// Cross-chain miras planı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainInheritancePlan {
    /// Plan kimliği
    pub plan_id: u64,
    /// Nova Shield hesabı sahibi
    pub owner: [u8; 20],
    /// Sahip olunan vaultlar (vault_id listesi)
    pub vault_ids: Vec<u64>,
    /// Mirasçı tahsisleri
    pub heir_allocations: Vec<CrossChainHeirAllocation>,
    /// Plan oluşturulma slotu
    pub created_at_slot: u64,
    /// Tetiklenme slotu (ölüm/inaktivite)
    pub trigger_slot: Option<u64>,
    /// Aktif mi?
    pub active: bool,
    /// Tamamlandı mı? (dağıtım yapıldı)
    pub completed: bool,
}

impl CrossChainInheritancePlan {
    pub fn new(plan_id: u64, owner: [u8; 20], vault_ids: Vec<u64>, created_at_slot: u64) -> Self {
        Self {
            plan_id,
            owner,
            vault_ids,
            heir_allocations: vec![],
            created_at_slot,
            trigger_slot: None,
            active: true,
            completed: false,
        }
    }

    /// Mirasçı ekle
    pub fn add_heir(
        &mut self,
        heir_name: impl Into<String>,
        addresses: Vec<AssetAddress>,
        share_bps: u32,
    ) {
        self.heir_allocations.push(CrossChainHeirAllocation {
            heir_name: heir_name.into(),
            addresses,
            share_bps,
        });
    }

    /// Toplam share bps 10000 mi?
    pub fn validate_shares(&self) -> GuardResult<()> {
        let total: u32 = self.heir_allocations.iter().map(|h| h.share_bps).sum();
        if total != 10_000 {
            return Err(GuardError::InvalidTransaction {
                reason: format!("toplam pay {} bps, 10000 olmalı", total),
            });
        }
        Ok(())
    }

    /// Planı tetikle (deadman switch devreye girdi)
    pub fn trigger(&mut self, current_slot: u64) -> GuardResult<()> {
        if !self.active {
            return Err(GuardError::InheritancePlanNotFound);
        }
        self.trigger_slot = Some(current_slot);
        Ok(())
    }

    /// Mirasçı başına düşen miktar (satoshi/wei/lamport/uatom)
    pub fn calculate_heir_shares(&self, total_amount: u64) -> Vec<(String, u64)> {
        self.heir_allocations.iter().map(|h| {
            let share = total_amount * h.share_bps as u64 / 10_000;
            (h.heir_name.clone(), share)
        }).collect()
    }

    /// Hangi zincirlerin adresi var?
    pub fn covered_chains(&self) -> Vec<ChainId> {
        let mut chains = vec![];
        for heir in &self.heir_allocations {
            for addr in &heir.addresses {
                if !chains.contains(&addr.chain) {
                    chains.push(addr.chain.clone());
                }
            }
        }
        chains
    }
}

// ─── CrossChainInheritanceManager ────────────────────────────────────────────

/// Miras plan yöneticisi
pub struct CrossChainInheritanceManager {
    plans: DashMap<u64, CrossChainInheritancePlan>,
    plan_counter: std::sync::atomic::AtomicU64,
}

impl CrossChainInheritanceManager {
    pub fn new() -> Self {
        Self {
            plans: DashMap::new(),
            plan_counter: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Plan oluştur
    pub fn create_plan(
        &self,
        owner: [u8; 20],
        vault_ids: Vec<u64>,
        created_at_slot: u64,
    ) -> u64 {
        let plan_id = self.plan_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let plan = CrossChainInheritancePlan::new(plan_id, owner, vault_ids, created_at_slot);
        self.plans.insert(plan_id, plan);
        plan_id
    }

    /// Plan al
    pub fn get_plan(&self, plan_id: u64) -> GuardResult<CrossChainInheritancePlan> {
        self.plans.get(&plan_id)
            .map(|p| p.clone())
            .ok_or(GuardError::InheritancePlanNotFound)
    }

    /// Mirasçı ekle
    pub fn add_heir_to_plan(
        &self,
        plan_id: u64,
        heir_name: impl Into<String>,
        addresses: Vec<AssetAddress>,
        share_bps: u32,
    ) -> GuardResult<()> {
        self.plans.get_mut(&plan_id)
            .ok_or(GuardError::InheritancePlanNotFound)?
            .add_heir(heir_name, addresses, share_bps);
        Ok(())
    }

    /// Planı tetikle
    ///
    /// ## NOVA-RED-GUARD-004 Güvenlik Düzeltmesi
    ///
    /// Yalnızca planın sahibi (owner) tetikleyebilir.
    pub fn trigger_plan(&self, caller: [u8; 20], plan_id: u64, current_slot: u64) -> GuardResult<()> {
        let mut plan = self.plans.get_mut(&plan_id)
            .ok_or(GuardError::InheritancePlanNotFound)?;
        // NOVA-RED-GUARD-004: yalnızca plan sahibi tetikleyebilir
        if plan.owner != caller {
            return Err(GuardError::Unauthorized);
        }
        plan.trigger(current_slot)
    }

    /// Aktif planlar
    pub fn active_plans(&self) -> Vec<CrossChainInheritancePlan> {
        self.plans.iter()
            .filter(|p| p.active && !p.completed)
            .map(|p| p.clone())
            .collect()
    }

    /// Tetiklenmiş planlar (dağıtım bekleniyor)
    pub fn triggered_plans(&self) -> Vec<CrossChainInheritancePlan> {
        self.plans.iter()
            .filter(|p| p.trigger_slot.is_some() && !p.completed)
            .map(|p| p.clone())
            .collect()
    }

    /// Planı tamamla
    pub fn complete_plan(&self, plan_id: u64) -> GuardResult<()> {
        let mut plan = self.plans.get_mut(&plan_id)
            .ok_or(GuardError::InheritancePlanNotFound)?;
        plan.completed = true;
        plan.active = false;
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> [u8; 20] {
        [0x01u8; 20]
    }

    fn make_plan() -> CrossChainInheritancePlan {
        let mut plan = CrossChainInheritancePlan::new(1, owner(), vec![1, 2, 3], 100);
        plan.add_heir("Alice", vec![AssetAddress::bitcoin("bc1qalice")], 5_000);
        plan.add_heir("Bob", vec![AssetAddress::ethereum([0xBBu8; 20])], 5_000);
        plan
    }

    #[test]
    fn test_plan_validate_shares_ok() {
        let plan = make_plan();
        assert!(plan.validate_shares().is_ok());
    }

    #[test]
    fn test_plan_validate_shares_fail() {
        let mut plan = CrossChainInheritancePlan::new(1, owner(), vec![], 100);
        plan.add_heir("Alice", vec![], 3_000);
        assert!(matches!(plan.validate_shares(), Err(GuardError::InvalidTransaction { .. })));
    }

    #[test]
    fn test_plan_calculate_heir_shares() {
        let plan = make_plan();
        let shares = plan.calculate_heir_shares(1_000_000);
        assert_eq!(shares.len(), 2);
        assert_eq!(shares[0].1, 500_000); // 50%
        assert_eq!(shares[1].1, 500_000); // 50%
    }

    #[test]
    fn test_plan_covered_chains() {
        let plan = make_plan();
        let chains = plan.covered_chains();
        assert!(chains.contains(&ChainId::Bitcoin));
        assert!(chains.contains(&ChainId::Ethereum));
    }

    #[test]
    fn test_plan_trigger() {
        let mut plan = make_plan();
        plan.trigger(500).unwrap();
        assert_eq!(plan.trigger_slot, Some(500));
    }

    #[test]
    fn test_inheritance_manager_create_plan() {
        let mgr = CrossChainInheritanceManager::new();
        let id = mgr.create_plan(owner(), vec![1, 2], 100);
        assert!(id >= 1);
        let plan = mgr.get_plan(id).unwrap();
        assert_eq!(plan.plan_id, id);
    }

    #[test]
    fn test_inheritance_manager_add_heir() {
        let mgr = CrossChainInheritanceManager::new();
        let id = mgr.create_plan(owner(), vec![], 100);

        mgr.add_heir_to_plan(id, "Alice", vec![AssetAddress::bitcoin("bc1q")], 6_000).unwrap();
        mgr.add_heir_to_plan(id, "Bob", vec![AssetAddress::ethereum([0xBBu8; 20])], 4_000).unwrap();

        let plan = mgr.get_plan(id).unwrap();
        assert_eq!(plan.heir_allocations.len(), 2);
        plan.validate_shares().unwrap();
    }

    #[test]
    fn test_inheritance_manager_trigger() {
        let mgr = CrossChainInheritanceManager::new();
        let id = mgr.create_plan(owner(), vec![], 100);
        mgr.trigger_plan(owner(), id, 1000).unwrap();

        let plan = mgr.get_plan(id).unwrap();
        assert_eq!(plan.trigger_slot, Some(1000));

        assert_eq!(mgr.triggered_plans().len(), 1);
    }

    #[test]
    fn test_inheritance_manager_complete() {
        let mgr = CrossChainInheritanceManager::new();
        let id = mgr.create_plan(owner(), vec![], 100);
        mgr.complete_plan(id).unwrap();

        let plan = mgr.get_plan(id).unwrap();
        assert!(plan.completed);
        assert!(!plan.active);
        assert_eq!(mgr.active_plans().len(), 0);
    }

    #[test]
    fn test_inheritance_not_found() {
        let mgr = CrossChainInheritanceManager::new();
        assert!(matches!(mgr.get_plan(999), Err(GuardError::InheritancePlanNotFound)));
    }
}
