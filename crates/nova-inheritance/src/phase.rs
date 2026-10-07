//! Miras Protokolü Aşama Durum Makinesi
//!
//! ## Araştırma Kaynağı: Polkadot Scheduler + Cosmos Vesting
//!
//! ### Polkadot Scheduler
//! `research/chains/polkadot-sdk/substrate/frame/scheduler/src/lib.rs`:
//! ```text
//! // Her blokta görevleri kontrol et:
//! fn on_initialize(now: BlockNumber) -> Weight {
//!     let agenda = Agenda::<T>::take(now);
//!     for (index, s) in agenda.iter().enumerate() {
//!         if let Some(task) = s {
//!             Self::execute_dispatch(task, (now, index as u32).into());
//!         }
//!     }
//! }
//! ```
//!
//! ### Cosmos Vesting
//! `research/chains/cosmos-sdk/x/auth/vesting/types/period.go`:
//! ```text
//! // Her period: belirli bir süre sonra belirli miktarı serbest bırak
//! type Period struct {
//!     Length int64    // saniye
//!     Amount sdk.Coins
//! }
//! ```
//!
//! ## Nova Uyarlaması
//!
//! Nova'da aşama geçişleri deterministik slot-bazlı çalışır.
//! Her `process_slot()` çağrısı aşama değişikliklerini kontrol eder.

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{
    CancellationReason, InheritancePhase, InheritancePlan, InheritanceState,
    MAX_GUARDIAN_EXTENSIONS, MIN_GRACE_PERIOD_SLOTS, PRE_WARNING_SLOTS, FINAL_WARNING_SLOTS,
};
use crate::trigger::{TriggerEvaluation, TriggerEvaluator};
use nova_primitives::types::SlotId;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ─── AŞAMA GEÇİŞ OLAYLARI ────────────────────────────────────────────────────

/// Aşama geçişi sırasında üretilen olaylar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PhaseEvent {
    /// Ön uyarı başladı — guardian'lara bildirim.
    PreWarningStarted {
        plan_id: [u8; 32],
        ends_at_slot: SlotId,
    },
    /// Son uyarı başladı — varis(lere) bildirim.
    FinalWarningStarted {
        plan_id: [u8; 32],
        ends_at_slot: SlotId,
    },
    /// Grace period başladı — kullanıcı iptal edebilir.
    GracePeriodStarted {
        plan_id: [u8; 32],
        ends_at_slot: SlotId,
    },
    /// Grace period uzatıldı — guardian kararı.
    GracePeriodExtended {
        plan_id: [u8; 32],
        new_ends_at_slot: SlotId,
        extension_number: u8,
    },
    /// Kademeli devir başladı.
    StagedReleaseStarted {
        plan_id: [u8; 32],
        total_steps: u32,
    },
    /// Devir tamamlandı.
    Completed {
        plan_id: [u8; 32],
    },
    /// İptal edildi.
    Cancelled {
        plan_id: [u8; 32],
        reason: CancellationReason,
    },
    /// Duress tespiti — gizli guardian alarmı.
    DuressDetected {
        plan_id: [u8; 32],
    },
}

// ─── AŞAMA YÖNETİCİSİ ─────────────────────────────────────────────────────────

/// Aşama geçiş yöneticisi.
///
/// Polkadot Scheduler'ın `on_initialize` + Cosmos Vesting'in period management analog.
pub struct PhaseManager;

impl PhaseManager {
    /// Her slot başında çağrılır — aşama geçişlerini kontrol et.
    ///
    /// Polkadot Scheduler `on_initialize` analog.
    /// Cosmos Vesting period check analog.
    ///
    /// Döndürülen liste: bu slot'ta oluşan olaylar.
    pub fn process_slot(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
    ) -> Vec<PhaseEvent> {
        let mut events = Vec::new();

        match &state.phase.clone() {
            InheritancePhase::Dormant => {
                // Tetikleyici kontrol et
                let eval = TriggerEvaluator::evaluate(plan, state, current_slot);
                Self::handle_dormant_eval(plan, state, current_slot, eval, &mut events);
            }
            InheritancePhase::PreWarning { ends_at_slot, .. } => {
                let ends = *ends_at_slot;
                if current_slot.0 >= ends.0 {
                    // Ön uyarı süresi doldu → son uyarıya geç
                    Self::transition_to_final_warning(plan, state, current_slot, &mut events);
                }
            }
            InheritancePhase::FinalWarning { ends_at_slot, .. } => {
                let ends = *ends_at_slot;
                if current_slot.0 >= ends.0 {
                    // Son uyarı süresi doldu → grace period
                    Self::transition_to_grace_period(plan, state, current_slot, &mut events);
                }
            }
            InheritancePhase::GracePeriod { ends_at_slot, .. } => {
                let ends = *ends_at_slot;
                if current_slot.0 >= ends.0 {
                    // Grace period doldu → kademeli devir başlat
                    Self::transition_to_staged_release(plan, state, current_slot, &mut events);
                }
            }
            InheritancePhase::StagedRelease { .. } => {
                // Kademeli devir engine tarafından yönetilir
                // Bu aşamada sadece tamamlanma kontrolü
            }
            InheritancePhase::Completed { .. } | InheritancePhase::Cancelled { .. } => {
                // Terminal aşama — hiçbir şey yapma
            }
        }

        events
    }

    /// Dormant aşamasında tetikleyici değerlendirmesi.
    fn handle_dormant_eval(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        eval: TriggerEvaluation,
        events: &mut Vec<PhaseEvent>,
    ) {
        match eval {
            TriggerEvaluation::PreWarningShouldStart => {
                Self::transition_to_pre_warning(plan, state, current_slot, events);
            }
            TriggerEvaluation::FinalWarningShouldStart => {
                // Ön uyarıyı atlayıp direkt son uyarıya geç
                Self::transition_to_final_warning(plan, state, current_slot, events);
            }
            TriggerEvaluation::ShouldTrigger | TriggerEvaluation::ManualActivationReady => {
                // Uyarıları atlayıp direkt grace period'a geç
                Self::transition_to_grace_period(plan, state, current_slot, events);
            }
            TriggerEvaluation::NotYet { .. } => {
                // Henüz bir şey yok
            }
        }
    }

    // ─── AŞAMA GEÇİŞ FONKSİYONLARI ──────────────────────────────────────────

    /// Dormant → PreWarning geçişi.
    fn transition_to_pre_warning(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        events: &mut Vec<PhaseEvent>,
    ) {
        // Ön uyarı 30 gün sürer
        let ends_at = SlotId(current_slot.0 + PRE_WARNING_SLOTS);
        info!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            slot = current_slot.0,
            ends_at = ends_at.0,
            "Miras planı ön uyarı aşamasına geçti"
        );
        state.phase = InheritancePhase::PreWarning {
            started_at_slot: current_slot,
            ends_at_slot: ends_at,
        };
        state.updated_at_slot = current_slot;
        events.push(PhaseEvent::PreWarningStarted {
            plan_id: plan.plan_id.0,
            ends_at_slot: ends_at,
        });
    }

    /// PreWarning → FinalWarning geçişi.
    fn transition_to_final_warning(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        events: &mut Vec<PhaseEvent>,
    ) {
        let ends_at = SlotId(current_slot.0 + FINAL_WARNING_SLOTS);
        info!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            slot = current_slot.0,
            ends_at = ends_at.0,
            "Miras planı son uyarı aşamasına geçti"
        );
        state.phase = InheritancePhase::FinalWarning {
            started_at_slot: current_slot,
            ends_at_slot: ends_at,
        };
        state.updated_at_slot = current_slot;
        events.push(PhaseEvent::FinalWarningStarted {
            plan_id: plan.plan_id.0,
            ends_at_slot: ends_at,
        });
    }

    /// → GracePeriod geçişi.
    fn transition_to_grace_period(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        events: &mut Vec<PhaseEvent>,
    ) {
        let ends_at = SlotId(current_slot.0 + plan.grace_period_slots);
        warn!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            slot = current_slot.0,
            grace_ends_at = ends_at.0,
            "Miras planı grace period aşamasına girdi — kullanıcı hâlâ iptal edebilir"
        );
        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: current_slot,
            ends_at_slot: ends_at,
            extension_count: 0,
        };
        state.updated_at_slot = current_slot;
        events.push(PhaseEvent::GracePeriodStarted {
            plan_id: plan.plan_id.0,
            ends_at_slot: ends_at,
        });
    }

    /// → StagedRelease geçişi.
    fn transition_to_staged_release(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        events: &mut Vec<PhaseEvent>,
    ) {
        let total_steps = plan.release_steps.len() as u32;
        warn!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            slot = current_slot.0,
            total_steps,
            "Miras protokolü devir aşaması başladı"
        );
        state.phase = InheritancePhase::StagedRelease {
            started_at_slot: current_slot,
            completed_steps: 0,
            total_steps,
        };
        state.release_started_at = Some(current_slot);
        state.updated_at_slot = current_slot;
        events.push(PhaseEvent::StagedReleaseStarted {
            plan_id: plan.plan_id.0,
            total_steps,
        });
    }

    // ─── KULLANICI AKSİYONLARI ────────────────────────────────────────────────

    /// Kullanıcı heartbeat aldı → planı sıfırla (iptal et).
    ///
    /// §5.4.3: "Kullanıcı tek bir heartbeat ile tüm süreci sıfırlar."
    pub fn handle_heartbeat(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        is_duress: bool,
    ) -> Vec<PhaseEvent> {
        let mut events = Vec::new();

        // Duress tespiti
        if is_duress {
            events.push(PhaseEvent::DuressDetected { plan_id: plan.plan_id.0 });
            // Duress: sayacı sıfırla ama protokolü durdurmak yerine devam ettir
            // (saldırgan "durdu" zannetsin)
            debug!(
                plan_id = %hex::encode(&plan.plan_id.0[..4]),
                "Duress heartbeat — gizli alarm, protokol görünürde devam ediyor"
            );
            // Ama state'i normal aktivite olarak işaretle
            state.record_heartbeat(current_slot, true);
        }

        if !is_duress && state.phase.is_cancellable() {
            // Normal heartbeat → Dormant'a dön
            info!(
                plan_id = %hex::encode(&plan.plan_id.0[..4]),
                slot = current_slot.0,
                "Heartbeat alındı — miras planı sıfırlandı (Dormant)"
            );
            state.phase = InheritancePhase::Dormant;
            state.record_heartbeat(current_slot, false);
            events.push(PhaseEvent::Cancelled {
                plan_id: plan.plan_id.0,
                reason: CancellationReason::HeartbeatReceived,
            });
        }

        events
    }

    /// Manuel iptal — kullanıcı kararı.
    pub fn cancel(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
    ) -> InheritanceResult<Vec<PhaseEvent>> {
        if !state.phase.is_cancellable() {
            return Err(InheritanceError::WrongPhase {
                expected: "Cancellable".to_string(),
                current: state.phase.name().to_string(),
            });
        }

        state.phase = InheritancePhase::Cancelled {
            cancelled_at_slot: current_slot,
            reason: CancellationReason::ManualCancellation,
        };
        state.updated_at_slot = current_slot;

        Ok(vec![PhaseEvent::Cancelled {
            plan_id: plan.plan_id.0,
            reason: CancellationReason::ManualCancellation,
        }])
    }

    /// Guardian grace period uzatma.
    ///
    /// §5.4.4 Yöntem 3: "Kullanıcı erişimi tamamen yoksa, belirlenen
    /// threshold sayıda guardian grace period'u uzatabilir."
    /// Maksimum 3 kez.
    pub fn extend_grace_period(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
        extension_slots: u64,
    ) -> InheritanceResult<Vec<PhaseEvent>> {
        // Grace period'da mıyız?
        let (started_at, current_ends, ext_count) = match &state.phase {
            InheritancePhase::GracePeriod { started_at_slot, ends_at_slot, extension_count } => {
                (*started_at_slot, *ends_at_slot, *extension_count)
            }
            other => {
                return Err(InheritanceError::WrongPhase {
                    expected: "GracePeriod".to_string(),
                    current: other.name().to_string(),
                })
            }
        };

        // Maksimum 3 uzatma
        if ext_count >= MAX_GUARDIAN_EXTENSIONS {
            return Err(InheritanceError::GracePeriodExtensionLimitExceeded);
        }

        let new_ends = SlotId(current_ends.0.saturating_add(extension_slots));
        let new_ext_count = ext_count + 1;

        warn!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            extension = new_ext_count,
            new_ends_at = new_ends.0,
            "Guardian grace period uzattı"
        );

        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: started_at,
            ends_at_slot: new_ends,
            extension_count: new_ext_count,
        };
        state.guardian_extension_count = new_ext_count;
        state.updated_at_slot = current_slot;

        Ok(vec![PhaseEvent::GracePeriodExtended {
            plan_id: plan.plan_id.0,
            new_ends_at_slot: new_ends,
            extension_number: new_ext_count,
        }])
    }

    /// Deviri tamamla.
    pub fn complete(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        current_slot: SlotId,
    ) -> Vec<PhaseEvent> {
        info!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            slot = current_slot.0,
            "Miras protokolü tamamlandı"
        );
        state.phase = InheritancePhase::Completed {
            completed_at_slot: current_slot,
        };
        state.updated_at_slot = current_slot;
        vec![PhaseEvent::Completed { plan_id: plan.plan_id.0 }]
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        HeirRecord, HeirTier, InheritancePlan, InheritancePlanId, InheritanceState, TriggerType,
    };
    use nova_primitives::types::Address;

    fn make_plan_and_state(period_secs: u64) -> (InheritancePlan, InheritanceState) {
        let plan_id = InheritancePlanId([0u8; 32]);
        let owner = Address::from_bytes([1u8; 20]);
        let heir = HeirRecord::new(
            Address::from_bytes([2u8; 20]),
            HeirTier::Primary,
            10_000,
            100_000,
        );
        let plan = InheritancePlan {
            plan_id,
            owner_account_id: 1,
            owner_address: owner,
            trigger: TriggerType::Inactivity { period_secs },
            grace_period_slots: MIN_GRACE_PERIOD_SLOTS,
            heirs: vec![heir],
            release_steps: vec![],
            conditions: vec![],
            created_at_slot: SlotId(0),
            updated_at_slot: SlotId(0),
            is_active: true,
        };
        let state = InheritanceState::new(plan_id, SlotId(0));
        (plan, state)
    }

    const SLOT_MS: u64 = crate::types::SLOT_DURATION_MS;

    #[test]
    fn test_dormant_no_event_early() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        let events = PhaseManager::process_slot(&plan, &mut state, SlotId(1000));
        assert!(events.is_empty());
        assert!(matches!(state.phase, InheritancePhase::Dormant));
    }

    #[test]
    fn test_dormant_transitions_to_pre_warning() {
        let period_secs = 90 * 24 * 3600u64;
        let (plan, mut state) = make_plan_and_state(period_secs);

        // 65 gün geçti → PreWarning aralığında
        let elapsed = 65 * 24 * 3600 * 1000 / SLOT_MS;
        let events = PhaseManager::process_slot(&plan, &mut state, SlotId(elapsed));

        assert!(!events.is_empty());
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::PreWarningStarted { .. })));
        assert!(matches!(state.phase, InheritancePhase::PreWarning { .. }));
    }

    #[test]
    fn test_pre_warning_transitions_to_final_warning() {
        let period_secs = 90 * 24 * 3600u64;
        let (plan, mut state) = make_plan_and_state(period_secs);

        // Manuel olarak PreWarning'e sok
        state.phase = InheritancePhase::PreWarning {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(100),
        };

        // PreWarning süresi doldu
        let events = PhaseManager::process_slot(&plan, &mut state, SlotId(101));
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::FinalWarningStarted { .. })));
        assert!(matches!(state.phase, InheritancePhase::FinalWarning { .. }));
    }

    #[test]
    fn test_final_warning_transitions_to_grace_period() {
        let period_secs = 90 * 24 * 3600u64;
        let (plan, mut state) = make_plan_and_state(period_secs);

        state.phase = InheritancePhase::FinalWarning {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(200),
        };

        let events = PhaseManager::process_slot(&plan, &mut state, SlotId(201));
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::GracePeriodStarted { .. })));
        assert!(matches!(state.phase, InheritancePhase::GracePeriod { .. }));
    }

    #[test]
    fn test_grace_period_transitions_to_staged_release() {
        let period_secs = 90 * 24 * 3600u64;
        let (plan, mut state) = make_plan_and_state(period_secs);

        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(300),
            extension_count: 0,
        };

        let events = PhaseManager::process_slot(&plan, &mut state, SlotId(301));
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::StagedReleaseStarted { .. })));
        assert!(matches!(state.phase, InheritancePhase::StagedRelease { .. }));
    }

    #[test]
    fn test_heartbeat_cancels_pre_warning() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);

        state.phase = InheritancePhase::PreWarning {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(1000),
        };

        let events = PhaseManager::handle_heartbeat(&plan, &mut state, SlotId(500), false);
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::Cancelled { reason: CancellationReason::HeartbeatReceived, .. })));
        assert!(matches!(state.phase, InheritancePhase::Dormant));
    }

    #[test]
    fn test_manual_cancel() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(1000),
            extension_count: 0,
        };

        let events = PhaseManager::cancel(&plan, &mut state, SlotId(500)).unwrap();
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::Cancelled { .. })));
        assert!(matches!(state.phase, InheritancePhase::Cancelled { .. }));
    }

    #[test]
    fn test_grace_period_extension() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(1_000_000),
            extension_count: 0,
        };

        let events = PhaseManager::extend_grace_period(&plan, &mut state, SlotId(500_000), 500_000).unwrap();
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::GracePeriodExtended { extension_number: 1, .. })));

        if let InheritancePhase::GracePeriod { ends_at_slot, extension_count, .. } = &state.phase {
            assert_eq!(ends_at_slot.0, 1_500_000);
            assert_eq!(*extension_count, 1);
        }
    }

    #[test]
    fn test_grace_period_extension_limit() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(1_000_000),
            extension_count: 3, // Maks'a ulaştı
        };
        state.guardian_extension_count = 3;

        let err = PhaseManager::extend_grace_period(&plan, &mut state, SlotId(500_000), 100_000).unwrap_err();
        assert!(matches!(err, InheritanceError::GracePeriodExtensionLimitExceeded));
    }

    #[test]
    fn test_cannot_cancel_staged_release() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        state.phase = InheritancePhase::StagedRelease {
            started_at_slot: SlotId(0),
            completed_steps: 0,
            total_steps: 3,
        };

        let err = PhaseManager::cancel(&plan, &mut state, SlotId(100)).unwrap_err();
        assert!(matches!(err, InheritanceError::WrongPhase { .. }));
    }

    #[test]
    fn test_complete_transition() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        state.phase = InheritancePhase::StagedRelease {
            started_at_slot: SlotId(0),
            completed_steps: 3,
            total_steps: 3,
        };

        let events = PhaseManager::complete(&plan, &mut state, SlotId(9_000_000));
        assert!(events.iter().any(|e| matches!(e, PhaseEvent::Completed { .. })));
        assert!(matches!(state.phase, InheritancePhase::Completed { .. }));
    }

    #[test]
    fn test_terminal_phase_no_events() {
        let (plan, mut state) = make_plan_and_state(90 * 24 * 3600);
        state.phase = InheritancePhase::Completed { completed_at_slot: SlotId(0) };

        let events = PhaseManager::process_slot(&plan, &mut state, SlotId(999_999_999));
        assert!(events.is_empty(), "Terminal aşamada event üretilmemeli");
    }
}
