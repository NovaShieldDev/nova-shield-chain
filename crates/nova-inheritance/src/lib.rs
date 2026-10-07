//! # nova-inheritance
//!
//! Nova Shield Chain §5.4 — Gelişmiş Yerel Miras Protokolü
//!
//! ## Modül Yapısı
//!
//! ```text
//! nova-inheritance/
//! ├── types.rs       — Temel veri yapıları
//! ├── errors.rs      — InheritanceError + InheritanceResult
//! ├── heartbeat.rs   — HeartbeatTracker (Polkadot Im-Online analogu)
//! ├── trigger.rs     — TriggerEvaluator
//! ├── phase.rs       — PhaseManager (4-aşamalı durum makinesi)
//! ├── heir.rs        — HeirManager
//! ├── release.rs     — ReleaseEngine (Cosmos Vesting analogu)
//! ├── conditions.rs  — ConditionEvaluator (Cosmos Authz + Avalanchego analogu)
//! ├── guardian.rs    — GuardianProtection (§5.4.8)
//! ├── plan.rs        — PlanBuilder + PlanManager
//! └── engine.rs      — InheritanceEngine (ana orkestrasyon)
//! ```

// ─── MODÜLLER ─────────────────────────────────────────────────────────────────

pub mod conditions;
pub mod engine;
pub mod errors;
pub mod guardian;
pub mod heartbeat;
pub mod heir;
pub mod phase;
pub mod plan;
pub mod release;
pub mod trigger;
pub mod types;

// ─── RE-EXPORT ────────────────────────────────────────────────────────────────

pub use engine::{InheritanceEngine, SlotProcessingResult};

pub use types::{
    ConditionalRule, HeartbeatKind, HeartbeatMsg, HeirRecord, HeirTier, InheritancePlan,
    InheritancePlanId, InheritancePhase, InheritanceState, InheritanceSummary, StagedReleaseStep,
    TransferCommand, TriggerType,
};

pub use types::{
    FINAL_WARNING_SLOTS, MAX_CONDITIONS, MAX_GUARDIAN_EXTENSIONS, MAX_HEIRS, MAX_RELEASE_STEPS,
    MIN_GRACE_PERIOD_SLOTS, PRE_WARNING_SLOTS, SLOT_DURATION_MS,
};

pub use errors::{InheritanceError, InheritanceResult};

pub use conditions::{ConditionEvaluator, ConditionResult};
pub use guardian::GuardianProtection;
pub use heartbeat::{HeartbeatBuilder, HeartbeatRecord, HeartbeatTracker};
pub use heir::{HeirManager, HeirResolution};
pub use phase::{PhaseEvent, PhaseManager};
pub use plan::{PlanBuilder, PlanManager};
pub use release::{ReleasePlanSummary, ReleaseEngine};
pub use trigger::{TriggerEvaluation, TriggerEvaluator};

// ─── ENTEGRASYON TESTLERİ ─────────────────────────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use nova_primitives::types::{Address, SlotId};

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    /// Tam miras senaryosu: plan oluştur → heartbeat → Dormant'ta kal
    #[test]
    fn test_full_inheritance_scenario_inactivity() {
        let engine = InheritanceEngine::new();

        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .build(SlotId(0))
            .unwrap();

        let plan_id = engine.create_plan(plan).unwrap();

        // Birkaç slot heartbeat gönder
        for slot in [100u64, 200, 300] {
            let msg = HeartbeatBuilder::new(1, plan_id, SlotId(slot)).build_unsigned();
            engine.process_heartbeat(msg).unwrap();
        }

        let state = engine.get_state(plan_id).unwrap();
        assert_eq!(state.heartbeat_count, 3);
        assert!(matches!(state.phase, InheritancePhase::Dormant));
    }

    /// Koşullu kural: yaş koşulu karşılandı
    #[test]
    fn test_age_condition_integration() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .add_condition(ConditionalRule::AgeCondition {
                heir_address: addr(1),
                min_account_age_slots: 1_000,
            })
            .build(SlotId(0))
            .unwrap();

        let state = InheritanceState::new(plan.plan_id, SlotId(0));

        // Hesap 2000 slot yaşında → koşul karşılandı
        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(2000), SlotId(0));
        assert!(ConditionEvaluator::all_satisfied(&results));
    }

    /// Çoklu varis: birincil + ikincil — her tier ayrı ayrı 10000 bps
    #[test]
    fn test_heir_tier_distribution() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)   // Primary tier: %100
            .add_secondary_heir(addr(2), 10_000, 500_000) // Secondary tier: %100
            .build(SlotId(0))
            .unwrap();

        assert_eq!(plan.primary_heirs().len(), 1);
        assert_eq!(plan.secondary_heirs().len(), 1);
    }

    /// Kademeli serbest bırakma: release_started_at sonrası pending_steps
    #[test]
    fn test_staged_release_pending_steps() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .add_release_step(0, 5_000, addr(1))
            .add_release_step(1_000_000, 5_000, addr(1))
            .build(SlotId(0))
            .unwrap();

        assert_eq!(plan.release_steps.len(), 2);

        // Release başlamadan önce
        let state = InheritanceState::new(plan.plan_id, SlotId(0));
        let pending = ReleaseEngine::pending_steps(&plan, &state, SlotId(0));
        assert!(pending.is_empty()); // release_started_at = None

        // Release başladıktan sonra
        let mut state2 = state.clone();
        state2.release_started_at = Some(SlotId(0));
        state2.phase = InheritancePhase::StagedRelease {
            started_at_slot: SlotId(0),
            completed_steps: 0,
            total_steps: 2,
        };

        let pending2 = ReleaseEngine::pending_steps(&plan, &state2, SlotId(0));
        assert_eq!(pending2.len(), 1); // Sadece delay=0 olan hazır
    }

    /// Multi-sig: yeterli onay var mı?
    #[test]
    fn test_multisig_integration() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 5_000, 500_000)
            .add_primary_heir(addr(2), 5_000, 500_000)
            .add_condition(ConditionalRule::MultiSigRequired {
                required_signatures: 2,
                total_heirs: 2,
                retry_delay_slots: 100_000,
            })
            .build(SlotId(0))
            .unwrap();

        let mut state = InheritanceState::new(plan.plan_id, SlotId(0));

        // Tek onay — yeterli değil
        state.multisig_approvals.push(addr(1));
        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), SlotId(0));
        assert!(!ConditionEvaluator::all_satisfied(&results));

        // İki onay — yeterli
        state.multisig_approvals.push(addr(2));
        let results2 = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), SlotId(0));
        assert!(ConditionEvaluator::all_satisfied(&results2));
    }

    /// Borç ödeme koşulu — transfer komutu üretir
    #[test]
    fn test_debt_payment_produces_transfer() {
        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .add_condition(ConditionalRule::DebtPayment {
                creditor: addr(9),
                amount: 100_000,
                priority: 1,
            })
            .build(SlotId(0))
            .unwrap();

        let state = InheritanceState::new(plan.plan_id, SlotId(0));
        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), SlotId(0));

        let transfers = ConditionEvaluator::required_transfers(results);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].to, addr(9));
        assert_eq!(transfers[0].amount, 100_000);
    }

    /// Engine çoklu plan — slot işleme
    #[test]
    fn test_engine_multiple_plans_on_slot() {
        let engine = InheritanceEngine::new();

        for i in 0..5u8 {
            // Her plan farklı account_id + farklı slot → farklı plan_id
            let plan = PlanBuilder::new(i as u64 + 1, addr(i))
                .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
                .with_grace_period_slots(2_000_000)
                .add_primary_heir(addr(i + 10), 10_000, 500_000)
                .build(SlotId(i as u64))
                .unwrap();
            engine.create_plan(plan).unwrap();
        }

        assert_eq!(engine.active_plan_count(), 5);

        let result = engine.on_slot(SlotId(1000));
        assert_eq!(result.plans_processed, 5);
    }

    /// Duress tespiti: duress heartbeat sonrası state güncellenir
    ///
    /// §5.4.8: can_guardian_block_user_cancellation() daima false döner —
    /// guardian ASLA kullanıcı iptalini engelleyemez. Duress gizli alarm için kullanılır.
    #[test]
    fn test_duress_detection_sets_state() {
        let engine = InheritanceEngine::new();

        let plan = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .build(SlotId(0))
            .unwrap();

        let plan_id = engine.create_plan(plan).unwrap();

        // Duress heartbeat gönder
        let msg = HeartbeatBuilder::new(1, plan_id, SlotId(500))
            .with_duress()
            .build_unsigned();
        engine.process_heartbeat(msg).unwrap();

        let state = engine.get_state(plan_id).unwrap();
        assert!(state.duress_detected, "duress flag set olmalı");

        // Guardian politikası: can_guardian_block_user_cancellation() daima false
        assert!(!GuardianProtection::can_guardian_block_user_cancellation());

        // Kullanıcı her zaman iptal edebilir (guardian kısıtı yok)
        engine.cancel_plan(plan_id, addr(0), SlotId(600)).unwrap();
        let final_state = engine.get_state(plan_id).unwrap();
        assert!(matches!(final_state.phase, InheritancePhase::Cancelled { .. }));
    }

    /// PlanBuilder doğrulama — heir olmadan build başarısız
    #[test]
    fn test_plan_builder_no_heirs_fails() {
        let result = PlanBuilder::new(1, addr(0))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .build(SlotId(0));
        assert!(result.is_err());
    }

    /// InheritanceSummary doğrulama
    #[test]
    fn test_summary_fields() {
        let engine = InheritanceEngine::new();
        let plan = PlanBuilder::new(42, addr(5))
            .with_trigger(TriggerType::Inactivity { period_secs: 30 * 24 * 3600 })
            .with_grace_period_slots(2_000_000)
            .add_primary_heir(addr(1), 10_000, 500_000)
            .build(SlotId(0))
            .unwrap();

        let plan_id = engine.create_plan(plan).unwrap();
        let summary = engine.get_summary(plan_id, SlotId(100)).unwrap();

        assert_eq!(summary.owner_account_id, 42);
        assert_eq!(summary.primary_heir_count, 1);
        assert!(!summary.duress_detected);
        assert_eq!(summary.total_transferred, 0);
    }

    /// Sabitler doğru mu?
    #[test]
    fn test_constants() {
        assert_eq!(PRE_WARNING_SLOTS, 6_480_000);
        assert_eq!(FINAL_WARNING_SLOTS, 1_512_000);
        assert_eq!(SLOT_DURATION_MS, 400);
        assert_eq!(MAX_HEIRS, 15);
        assert_eq!(MAX_CONDITIONS, 10);
        assert_eq!(MAX_RELEASE_STEPS, 20);
    }
}
