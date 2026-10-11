//! Miras Planı Oluşturma ve Yönetimi
//!
//! ## Araştırma Kaynağı: Cosmos SDK x/bank + Go-Ethereum Account Manager
//!
//! ### Cosmos SDK x/bank
//! `research/chains/cosmos-sdk/x/bank/`:
//! ```text
//! func (k Keeper) SendCoins(ctx Context, fromAddr, toAddr sdk.AccAddress, amt sdk.Coins) error {
//!     if err := k.subUnlockedCoins(ctx, fromAddr, amt); err != nil { ... }
//!     k.addCoins(ctx, toAddr, amt)
//! }
//! ```
//!
//! ### Go-Ethereum Account Manager
//! `research/chains/go-ethereum/accounts/manager.go`:
//! ```text
//! type Manager struct {
//!     backends map[reflect.Type][]Backend
//!     updaters []event.Subscription
//! }
//! func (am *Manager) Accounts() []accounts.Account { ... }
//! ```
//!
//! ## Nova Uyarlaması
//!
//! Plan yönetimi: oluştur, güncelle, iptal et, sorgula.
//! Nova-account'taki AccountId ile ilişkilendirir.

use crate::errors::{InheritanceError, InheritanceResult};
use crate::heir::HeirManager;
use crate::types::{
    HeirRecord, HeirTier, InheritancePlan, InheritancePlanId, InheritanceState,
    MAX_CONDITIONS, MAX_HEIRS, MAX_RELEASE_STEPS, StagedReleaseStep, ConditionalRule,
    TriggerType, InheritanceSummary,
};
use nova_primitives::types::{Address, SlotId};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

// ─── PLAN OLUŞTURUCU ─────────────────────────────────────────────────────────

/// Miras planı oluşturucu — builder pattern.
///
/// Go-Ethereum `accounts.Manager` + Cosmos `MsgServer` analog.
#[derive(Debug)]
pub struct PlanBuilder {
    owner_account_id: u64,
    owner_address: Address,
    trigger: Option<TriggerType>,
    grace_period_slots: u64,
    heirs: Vec<HeirRecord>,
    release_steps: Vec<StagedReleaseStep>,
    conditions: Vec<ConditionalRule>,
}

impl PlanBuilder {
    /// Yeni builder.
    pub fn new(owner_account_id: u64, owner_address: Address) -> Self {
        Self {
            owner_account_id,
            owner_address,
            trigger: None,
            grace_period_slots: crate::types::MIN_GRACE_PERIOD_SLOTS * 12, // 84 gün varsayılan
            heirs: Vec::new(),
            release_steps: Vec::new(),
            conditions: Vec::new(),
        }
    }

    /// Tetikleyici ayarla.
    pub fn with_trigger(mut self, trigger: TriggerType) -> Self {
        self.trigger = Some(trigger);
        self
    }

    /// Grace period ayarla (slot cinsinden).
    pub fn with_grace_period_slots(mut self, slots: u64) -> Self {
        self.grace_period_slots = slots;
        self
    }

    /// Birincil varis ekle.
    pub fn add_primary_heir(mut self, address: Address, share_bps: u32, timeout_slots: u64) -> Self {
        self.heirs.push(HeirRecord::new(address, HeirTier::Primary, share_bps, timeout_slots));
        self
    }

    /// İkincil varis ekle.
    pub fn add_secondary_heir(mut self, address: Address, share_bps: u32, timeout_slots: u64) -> Self {
        self.heirs.push(HeirRecord::new(address, HeirTier::Secondary, share_bps, timeout_slots));
        self
    }

    /// Üçüncül varis ekle.
    pub fn add_tertiary_heir(mut self, address: Address, share_bps: u32, timeout_slots: u64) -> Self {
        self.heirs.push(HeirRecord::new(address, HeirTier::Tertiary, share_bps, timeout_slots));
        self
    }

    /// Kademeli devir adımı ekle.
    pub fn add_release_step(
        mut self,
        delay_slots: u64,
        fraction_bps: u32,
        recipient: Address,
    ) -> Self {
        let idx = self.release_steps.len() as u32;
        self.release_steps.push(StagedReleaseStep::new(idx, delay_slots, fraction_bps, recipient));
        self
    }

    /// Koşullu kural ekle.
    pub fn add_condition(mut self, condition: ConditionalRule) -> Self {
        self.conditions.push(condition);
        self
    }

    /// Planı inşa et ve doğrula.
    pub fn build(self, created_at_slot: SlotId) -> InheritanceResult<InheritancePlan> {
        // Tetikleyici zorunlu
        let trigger = self.trigger.ok_or(InheritanceError::InvalidParameter(
            "tetikleyici tipi gerekli".to_string(),
        ))?;

        let plan_id = InheritancePlanId::derive(self.owner_account_id, created_at_slot);

        let plan = InheritancePlan {
            plan_id,
            owner_account_id: self.owner_account_id,
            owner_address: self.owner_address,
            trigger,
            grace_period_slots: self.grace_period_slots,
            heirs: self.heirs,
            release_steps: self.release_steps,
            conditions: self.conditions,
            created_at_slot,
            updated_at_slot: created_at_slot,
            is_active: true,
            guardian_ids: vec![],
        };

        // Plan doğrulama
        plan.validate()?;

        // Varis doğrulama
        HeirManager::validate_heirs(&plan)?;

        info!(
            plan_id = %plan.plan_id,
            owner = plan.owner_account_id,
            heir_count = plan.heirs.len(),
            trigger = plan.trigger.name(),
            "Miras planı oluşturuldu"
        );

        Ok(plan)
    }
}

// ─── PLAN YÖNETİCİSİ ─────────────────────────────────────────────────────────

/// Miras planı yönetim operasyonları.
///
/// Cosmos Bank Keeper + Go-Ethereum Account Manager analog.
pub struct PlanManager;

impl PlanManager {
    /// Planın aktif ve işlenebilir durumda olduğunu doğrula.
    pub fn assert_active(plan: &InheritancePlan, state: &InheritanceState) -> InheritanceResult<()> {
        if !plan.is_active {
            return Err(InheritanceError::PlanNotActive {
                phase: state.phase.name().to_string(),
            });
        }
        if state.phase.is_terminal() {
            return Err(InheritanceError::PlanNotActive {
                phase: state.phase.name().to_string(),
            });
        }
        Ok(())
    }

    /// Plan özeti oluştur.
    pub fn create_summary(
        plan: &InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> InheritanceSummary {
        InheritanceSummary {
            plan_id: plan.plan_id,
            owner_account_id: plan.owner_account_id,
            phase: state.phase.name().to_string(),
            trigger_type: plan.trigger.name().to_string(),
            last_activity_slot: state.last_activity_slot,
            inactivity_secs: state.inactivity_secs(current_slot),
            primary_heir_count: plan.primary_heirs().len(),
            secondary_heir_count: plan.secondary_heirs().len(),
            tertiary_heir_count: plan.tertiary_heirs().len(),
            heartbeat_count: state.heartbeat_count,
            duress_detected: state.duress_detected,
            total_transferred: state.total_transferred.0,
        }
    }

    /// Plan güncelleme: yeni varis ekle (sadece Dormant aşamasında).
    pub fn add_heir(
        plan: &mut InheritancePlan,
        state: &InheritanceState,
        heir: HeirRecord,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        // Sadece Dormant aşamasında güncelleme yapılabilir
        use crate::types::InheritancePhase;
        if !matches!(state.phase, InheritancePhase::Dormant) {
            return Err(InheritanceError::WrongPhase {
                expected: "Dormant".to_string(),
                current: state.phase.name().to_string(),
            });
        }

        if plan.heirs.len() >= MAX_HEIRS {
            return Err(InheritanceError::TooManyHeirs { count: plan.heirs.len() + 1 });
        }

        // Duplicate kontrol
        if plan.heirs.iter().any(|h| h.address == heir.address) {
            return Err(InheritanceError::HeirAlreadyExists {
                heir_address: *heir.address.as_bytes(),
            });
        }

        plan.heirs.push(heir);
        plan.updated_at_slot = current_slot;

        debug!(
            plan_id = %plan.plan_id,
            "Plana yeni varis eklendi"
        );

        Ok(())
    }

    /// Plan güncelleme: kademeli devir adımı ekle.
    pub fn add_release_step(
        plan: &mut InheritancePlan,
        state: &InheritanceState,
        step: StagedReleaseStep,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        use crate::types::InheritancePhase;
        if !matches!(state.phase, InheritancePhase::Dormant) {
            return Err(InheritanceError::WrongPhase {
                expected: "Dormant".to_string(),
                current: state.phase.name().to_string(),
            });
        }

        if plan.release_steps.len() >= MAX_RELEASE_STEPS {
            return Err(InheritanceError::InvalidParameter(
                format!("kademeli devir adımı sınırı aşıldı: maks={}", MAX_RELEASE_STEPS),
            ));
        }

        plan.release_steps.push(step);
        plan.updated_at_slot = current_slot;

        Ok(())
    }

    /// Plan güncelleme: koşullu kural ekle.
    pub fn add_condition(
        plan: &mut InheritancePlan,
        state: &InheritanceState,
        condition: ConditionalRule,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        use crate::types::InheritancePhase;
        if !matches!(state.phase, InheritancePhase::Dormant) {
            return Err(InheritanceError::WrongPhase {
                expected: "Dormant".to_string(),
                current: state.phase.name().to_string(),
            });
        }

        if plan.conditions.len() >= MAX_CONDITIONS {
            return Err(InheritanceError::InvalidParameter(
                format!("koşul sınırı aşıldı: maks={}", MAX_CONDITIONS),
            ));
        }

        if !condition.is_valid() {
            return Err(InheritanceError::InvalidParameter(
                format!("geçersiz koşul: {}", condition.rule_type()),
            ));
        }

        plan.conditions.push(condition);
        plan.updated_at_slot = current_slot;

        Ok(())
    }

    /// Plan bütünlük doğrulaması.
    ///
    /// Cosmos Bank Keeper `validateBalance` analog.
    pub fn validate_completeness(plan: &InheritancePlan) -> InheritanceResult<()> {
        // Kademeli devir adımları varsa toplamları 10000 olmalı
        if !plan.release_steps.is_empty() {
            let total: u32 = plan.release_steps.iter().map(|s| s.fraction_bps).sum();
            if total != 10_000 {
                return Err(InheritanceError::InvalidParameter(
                    format!("kademeli devir toplamı {total} bps (10000 olmalı)"),
                ));
            }
        }

        // Varis doğrulama
        HeirManager::validate_heirs(plan)?;

        Ok(())
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConditionalRule, HeirTier, TriggerType, MIN_GRACE_PERIOD_SLOTS};
    use nova_primitives::types::Address;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn default_trigger() -> TriggerType {
        TriggerType::Inactivity { period_secs: 90 * 24 * 3600 }
    }

    // Builder: basit plan oluşturma
    #[test]
    fn test_plan_builder_basic() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .build(SlotId(0))
            .unwrap();

        assert_eq!(plan.owner_account_id, 1);
        assert_eq!(plan.heirs.len(), 1);
        assert!(plan.is_active);
    }

    // Builder: tetikleyici olmadan → hata
    #[test]
    fn test_plan_builder_no_trigger_error() {
        let err = PlanBuilder::new(1, addr(0))
            .add_primary_heir(addr(1), 10_000, 100_000)
            .build(SlotId(0))
            .unwrap_err();
        assert!(matches!(err, InheritanceError::InvalidParameter(_)));
    }

    // Builder: varis listesi boş → hata
    #[test]
    fn test_plan_builder_no_heirs_error() {
        let err = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .build(SlotId(0))
            .unwrap_err();
        assert!(matches!(err, InheritanceError::EmptyHeirList));
    }

    // Builder: kademeli devir adımları
    #[test]
    fn test_plan_builder_with_release_steps() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .add_release_step(0, 2_500, addr(1))
            .add_release_step(100_000, 2_500, addr(1))
            .add_release_step(500_000, 2_500, addr(1))
            .add_release_step(1_000_000, 2_500, addr(1))
            .build(SlotId(0))
            .unwrap();

        assert_eq!(plan.release_steps.len(), 4);
        let total: u32 = plan.release_steps.iter().map(|s| s.fraction_bps).sum();
        assert_eq!(total, 10_000);
    }

    // Builder: yanlış release toplamı → hata
    #[test]
    fn test_plan_builder_wrong_release_total() {
        let err = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .add_release_step(0, 3_000, addr(1))  // Sadece %30
            .build(SlotId(0))
            .unwrap_err();
        assert!(matches!(err, InheritanceError::InvalidParameter(_)));
    }

    // PlanManager: active kontrol
    #[test]
    fn test_assert_active_ok() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .build(SlotId(0))
            .unwrap();
        let state = InheritanceState::new(plan.plan_id, SlotId(0));
        assert!(PlanManager::assert_active(&plan, &state).is_ok());
    }

    // PlanManager: tamamlanmış plan → hata
    #[test]
    fn test_assert_active_completed_error() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .build(SlotId(0))
            .unwrap();
        let mut state = InheritanceState::new(plan.plan_id, SlotId(0));
        state.phase = crate::types::InheritancePhase::Completed { completed_at_slot: SlotId(0) };

        let err = PlanManager::assert_active(&plan, &state).unwrap_err();
        assert!(matches!(err, InheritanceError::PlanNotActive { .. }));
    }

    // PlanManager: varis ekleme — geçerli plan ile başla, farklı katmana heir ekle
    #[test]
    fn test_add_heir_in_dormant() {
        let mut plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000) // Primary %100 — geçerli plan
            .build(SlotId(0))
            .unwrap();
        let state = InheritanceState::new(plan.plan_id, SlotId(0));

        // İkincil katmana heir ekle (add_heir share doğrulaması yapmaz)
        let new_heir = HeirRecord::new(addr(2), HeirTier::Secondary, 10_000, 100_000);
        PlanManager::add_heir(&mut plan, &state, new_heir, SlotId(100)).unwrap();
        assert_eq!(plan.heirs.len(), 2);
    }

    // PlanManager: active olmayan aşamada varis eklenemez
    #[test]
    fn test_add_heir_in_wrong_phase() {
        let mut plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .build(SlotId(0))
            .unwrap();
        let mut state = InheritanceState::new(plan.plan_id, SlotId(0));
        state.phase = crate::types::InheritancePhase::GracePeriod {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(2_000_000),
            extension_count: 0,
        };

        let new_heir = HeirRecord::new(addr(2), HeirTier::Secondary, 10_000, 100_000);
        let err = PlanManager::add_heir(&mut plan, &state, new_heir, SlotId(100)).unwrap_err();
        assert!(matches!(err, InheritanceError::WrongPhase { .. }));
    }

    // Plan ID deterministik
    #[test]
    fn test_plan_id_deterministic() {
        let id1 = InheritancePlanId::derive(42, SlotId(1000));
        let id2 = InheritancePlanId::derive(42, SlotId(1000));
        assert_eq!(id1, id2);
    }

    // Plan ID farklı girdiler → farklı ID
    #[test]
    fn test_plan_id_unique() {
        let id1 = InheritancePlanId::derive(1, SlotId(1000));
        let id2 = InheritancePlanId::derive(2, SlotId(1000));
        let id3 = InheritancePlanId::derive(1, SlotId(1001));
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
    }

    // Plan özeti
    #[test]
    fn test_create_summary() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .build(SlotId(0))
            .unwrap();
        let state = InheritanceState::new(plan.plan_id, SlotId(0));

        let summary = PlanManager::create_summary(&plan, &state, SlotId(500));
        assert_eq!(summary.owner_account_id, 1);
        assert_eq!(summary.phase, "Dormant");
        assert_eq!(summary.primary_heir_count, 1);
        assert_eq!(summary.heartbeat_count, 0);
    }

    // Builder: hiyerarşik varisler
    #[test]
    fn test_hierarchical_heirs() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(default_trigger())
            .with_grace_period_slots(MIN_GRACE_PERIOD_SLOTS * 4)
            .add_primary_heir(addr(1), 10_000, 100_000)
            .add_secondary_heir(addr(2), 10_000, 200_000)
            .add_tertiary_heir(addr(3), 10_000, 300_000)
            .build(SlotId(0))
            .unwrap();

        assert_eq!(plan.primary_heirs().len(), 1);
        assert_eq!(plan.secondary_heirs().len(), 1);
        assert_eq!(plan.tertiary_heirs().len(), 1);
    }
}
