//! On-chain hizmet sözleşmesi — SLA koşulları zincire yazılır

use crate::errors::{GuardianError, GuardianResult};
use crate::registry::verification::SlaTerms;
use dashmap::DashMap;
use nova_primitives::{Address, SlotId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ─── ContractId ───────────────────────────────────────────────────────────────

pub type ContractId = u64;

// ─── ContractStatus ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractStatus {
    Active,
    Expired,
    Terminated,
}

// ─── ServiceContract ──────────────────────────────────────────────────────────

/// On-chain hizmet sözleşmesi
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceContract {
    pub id: ContractId,
    pub user: Address,
    pub guardian: Address,
    pub status: ContractStatus,

    /// Yıllık ücret (NSC)
    pub annual_fee: u128,
    /// Ön ödenmiş bakiye
    pub prepaid_balance: u128,
    /// SLA koşulları (opsiyonel)
    pub sla: Option<SlaTerms>,

    pub created_at_slot: u64,
    /// Bitiş slot'u (yıllık ücret bittiğinde)
    pub expires_at_slot: u64,
}

impl ServiceContract {
    pub fn is_active(&self, current_slot: SlotId) -> bool {
        self.status == ContractStatus::Active && current_slot.0 < self.expires_at_slot
    }

    /// Kalan süre (slot)
    pub fn remaining_slots(&self, current_slot: SlotId) -> u64 {
        self.expires_at_slot.saturating_sub(current_slot.0)
    }
}

// ─── ContractStore ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ContractStore {
    contracts: Arc<DashMap<ContractId, ServiceContract>>,
    next_id: Arc<AtomicU64>,
}

impl ContractStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Yeni sözleşme oluştur (teklif kabul sonrasında)
    pub fn create(
        &self,
        user: Address,
        guardian: Address,
        annual_fee: u128,
        prepaid_balance: u128,
        sla: Option<SlaTerms>,
        slot: SlotId,
    ) -> ContractId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        // 1 yıl = 78_840_000 slot (365 gün × 216_000)
        let duration = 78_840_000u64;
        let contract = ServiceContract {
            id,
            user,
            guardian,
            status: ContractStatus::Active,
            annual_fee,
            prepaid_balance,
            sla,
            created_at_slot: slot.0,
            expires_at_slot: slot.0 + duration,
        };
        self.contracts.insert(id, contract);
        id
    }

    pub fn get(&self, id: ContractId) -> GuardianResult<ServiceContract> {
        self.contracts.get(&id).map(|c| c.clone()).ok_or(GuardianError::ContractNotFound)
    }

    /// Sözleşmeyi sonlandır.
    ///
    /// NOVA-RED-GUARDIAN-002: Yalnızca sözleşmedeki `user` veya `guardian` sonlandırabilir.
    /// Üçüncü bir taraf başkasının sözleşmesini sonlandıramaz.
    pub fn terminate(&self, id: ContractId, caller: Address) -> GuardianResult<()> {
        let mut entry = self.contracts
            .get_mut(&id)
            .ok_or(GuardianError::ContractNotFound)?;
        // NOVA-RED-GUARDIAN-002: caller sözleşmenin tarafı mı?
        if caller != entry.user && caller != entry.guardian {
            return Err(GuardianError::Unauthorized);
        }
        entry.status = ContractStatus::Terminated;
        Ok(())
    }

    /// Kullanıcının aktif sözleşmeleri
    pub fn active_contracts_for_user(
        &self,
        user: &Address,
        current_slot: SlotId,
    ) -> Vec<ServiceContract> {
        self.contracts
            .iter()
            .filter(|e| &e.user == user && e.is_active(current_slot))
            .map(|e| e.clone())
            .collect()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> Address {
        Address::from_bytes([n; 20])
    }

    #[test]
    fn test_create_contract() {
        let store = ContractStore::new();
        let id = store.create(addr(1), addr(2), 50, 100, None, SlotId(0));
        let contract = store.get(id).unwrap();
        assert_eq!(contract.status, ContractStatus::Active);
        assert!(contract.is_active(SlotId(1_000)));
    }

    #[test]
    fn test_contract_expiry() {
        let store = ContractStore::new();
        let id = store.create(addr(1), addr(2), 50, 100, None, SlotId(0));
        let contract = store.get(id).unwrap();
        // 1 yıl sonra süresi dolar
        assert!(!contract.is_active(SlotId(78_840_001)));
    }

    #[test]
    fn test_contract_terminate() {
        let store = ContractStore::new();
        let id = store.create(addr(1), addr(2), 50, 100, None, SlotId(0));
        // user (addr(1)) sonlandırabilir
        store.terminate(id, addr(1)).unwrap();
        assert_eq!(store.get(id).unwrap().status, ContractStatus::Terminated);
    }

    #[test]
    fn test_contract_terminate_by_guardian() {
        let store = ContractStore::new();
        let id = store.create(addr(1), addr(2), 50, 100, None, SlotId(0));
        // guardian (addr(2)) sonlandırabilir
        store.terminate(id, addr(2)).unwrap();
        assert_eq!(store.get(id).unwrap().status, ContractStatus::Terminated);
    }

    #[test]
    fn test_contract_terminate_unauthorized() {
        let store = ContractStore::new();
        let id = store.create(addr(1), addr(2), 50, 100, None, SlotId(0));
        // üçüncü taraf sonlandıramaz
        let err = store.terminate(id, addr(99)).unwrap_err();
        assert!(matches!(err, GuardianError::Unauthorized));
    }

    #[test]
    fn test_active_contracts_for_user() {
        let store = ContractStore::new();
        store.create(addr(1), addr(10), 50, 100, None, SlotId(0));
        store.create(addr(1), addr(20), 75, 150, None, SlotId(0));
        store.create(addr(2), addr(10), 50, 100, None, SlotId(0)); // farklı user
        let active = store.active_contracts_for_user(&addr(1), SlotId(1000));
        assert_eq!(active.len(), 2);
    }
}
