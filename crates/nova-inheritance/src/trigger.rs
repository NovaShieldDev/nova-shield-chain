//! Miras Tetikleyici Değerlendirme Motoru
//!
//! ## Araştırma Kaynağı: Polkadot Scheduler Pallet
//!
//! `research/chains/polkadot-sdk/substrate/frame/scheduler/src/lib.rs`
//!
//! ```text
//! // Scheduler: belirtilen block'ta çalıştır
//! pub struct Scheduled<Name, Call, BlockNumber, PalletsOrigin, AccountId> {
//!     maybe_id: Option<Name>,
//!     priority: Priority,
//!     call: Bounded<Call, T::Hashing>,
//!     maybe_periodic: Option<Period<BlockNumber>>,
//!     origin: PalletsOrigin,
//! }
//!
//! fn on_initialize(now: BlockNumber) -> Weight {
//!     // Tüm Agenda[now] görevlerini çalıştır
//! }
//! ```
//!
//! Scheduler'dan öğrenilenler:
//! - Belirli bir blok numarasına göre tetikleme
//! - `on_initialize` hook'u: her blok/slot başında kontrol
//! - Priority sistemi
//!
//! ## Nova Uyarlaması
//!
//! Nova'da her slot başında `TriggerEvaluator::check()` çağrılır.
//! Bu, Polkadot'un `on_initialize`'ına eşdeğerdir.

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{InheritancePlan, InheritanceState, TriggerType, SLOT_DURATION_MS};
use nova_primitives::types::SlotId;
use serde::{Deserialize, Serialize};

// ─── TETİKLEYİCİ DEĞERLENDİRME SONUCU ───────────────────────────────────────

/// Tetikleyici değerlendirme sonucu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerEvaluation {
    /// Tetikleyici henüz aktif değil.
    NotYet {
        /// Kaç saniye kaldı (tahmini).
        remaining_secs: u64,
    },
    /// Ön uyarı aşaması başlamalı (30 gün kala).
    PreWarningShouldStart,
    /// Son uyarı aşaması başlamalı (7 gün kala).
    FinalWarningShouldStart,
    /// Tetikleyici aktif → grace period başlamalı.
    ShouldTrigger,
    /// Manuel aktivasyon yeterli guardian onayı aldı.
    ManualActivationReady,
}

// ─── TETİKLEYİCİ DEĞERLENDİRİCİSİ ───────────────────────────────────────────

/// Tetikleyici değerlendirme motoru.
///
/// Polkadot Scheduler `on_initialize` hook analog.
/// Her slot başında plan durumunu kontrol eder.
pub struct TriggerEvaluator;

impl TriggerEvaluator {
    /// Ana değerlendirme fonksiyonu — her slot başında çağrılır.
    ///
    /// Polkadot Scheduler `on_initialize(now)` analog.
    ///
    /// Döndürülen değer:
    /// - `NotYet`: henüz bir şey yok
    /// - `PreWarningShouldStart`: 30 gün kala uyarı ver
    /// - `FinalWarningShouldStart`: 7 gün kala son uyarı
    /// - `ShouldTrigger`: inaktivite/slot süresi doldu
    /// - `ManualActivationReady`: yeterli guardian onayı var
    pub fn evaluate(
        plan: &InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> TriggerEvaluation {
        match &plan.trigger {
            TriggerType::Inactivity { period_secs } => {
                Self::evaluate_inactivity(state, current_slot, *period_secs)
            }
            TriggerType::FixedSlot { activation_slot, inactivity_period_secs } => {
                Self::evaluate_fixed_slot(state, current_slot, *activation_slot, *inactivity_period_secs)
            }
            TriggerType::ManualActivation { requires_guardian_approval, guardian_threshold } => {
                Self::evaluate_manual(
                    state,
                    *requires_guardian_approval,
                    *guardian_threshold,
                )
            }
        }
    }

    /// İnaktivite tetikleyicisi değerlendirmesi.
    ///
    /// Zaman zinciri:
    /// ```text
    /// [last_activity] ──────────── [period - 30 gün] ── [period - 7 gün] ── [period]
    ///                              PreWarning          FinalWarning        ShouldTrigger
    /// ```
    fn evaluate_inactivity(
        state: &InheritanceState,
        current_slot: SlotId,
        period_secs: u64,
    ) -> TriggerEvaluation {
        let inactivity_secs = state.inactivity_secs(current_slot);

        // 30 gün öncesi eşiği
        const PRE_WARNING_SECS: u64 = 30 * 24 * 3600;
        // 7 gün öncesi eşiği
        const FINAL_WARNING_SECS: u64 = 7 * 24 * 3600;

        if inactivity_secs >= period_secs {
            TriggerEvaluation::ShouldTrigger
        } else if inactivity_secs >= period_secs.saturating_sub(FINAL_WARNING_SECS) {
            TriggerEvaluation::FinalWarningShouldStart
        } else if inactivity_secs >= period_secs.saturating_sub(PRE_WARNING_SECS) {
            TriggerEvaluation::PreWarningShouldStart
        } else {
            let remaining = period_secs.saturating_sub(inactivity_secs);
            TriggerEvaluation::NotYet { remaining_secs: remaining }
        }
    }

    /// Sabit slot tetikleyicisi değerlendirmesi.
    fn evaluate_fixed_slot(
        state: &InheritanceState,
        current_slot: SlotId,
        activation_slot: SlotId,
        inactivity_period_secs: u64,
    ) -> TriggerEvaluation {
        // Aktivasyon slot'una henüz ulaşılmadıysa → bekle
        if current_slot.0 < activation_slot.0 {
            let remaining_slots = activation_slot.0.saturating_sub(current_slot.0);
            let remaining_secs = remaining_slots.saturating_mul(SLOT_DURATION_MS) / 1000;
            return TriggerEvaluation::NotYet {
                remaining_secs: remaining_secs + inactivity_period_secs,
            };
        }

        // Aktivasyon slot'u geçti → inaktivite sayacı çalışır
        Self::evaluate_inactivity(state, current_slot, inactivity_period_secs)
    }

    /// Manuel aktivasyon değerlendirmesi.
    fn evaluate_manual(
        state: &InheritanceState,
        requires_guardian_approval: bool,
        guardian_threshold: u8,
    ) -> TriggerEvaluation {
        if !requires_guardian_approval {
            // Guardian onayı gerekmez → kullanıcı tek başına aktive eder
            // Bu kontrol dışarıda yapılır (kullanıcı imzası yeterli)
            TriggerEvaluation::ManualActivationReady
        } else {
            let approval_count = state.manual_activation_approvals.len() as u8;
            if approval_count >= guardian_threshold {
                TriggerEvaluation::ManualActivationReady
            } else {
                TriggerEvaluation::NotYet {
                    remaining_secs: 0, // Guardian onayı bekleniyor
                }
            }
        }
    }

    /// İnaktivite süresinin dolup dolmadığını doğrula (hata varyantı).
    ///
    /// Plan doğrulaması için kullanılır.
    pub fn assert_triggered(
        plan: &InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        let eval = Self::evaluate(plan, state, current_slot);
        match eval {
            TriggerEvaluation::ShouldTrigger | TriggerEvaluation::ManualActivationReady => Ok(()),
            TriggerEvaluation::NotYet { remaining_secs } => {
                Err(InheritanceError::InactivityPeriodNotElapsed { remaining_secs })
            }
            TriggerEvaluation::PreWarningShouldStart
            | TriggerEvaluation::FinalWarningShouldStart => {
                // Uyarı aşamasındayız ama tetikleyici henüz dolmadı
                let remaining = match &plan.trigger {
                    TriggerType::Inactivity { period_secs } => {
                        let elapsed = state.inactivity_secs(current_slot);
                        period_secs.saturating_sub(elapsed)
                    }
                    _ => 0,
                };
                Err(InheritanceError::InactivityPeriodNotElapsed { remaining_secs: remaining })
            }
        }
    }

    /// Ön uyarı eşiği slot cinsinden.
    ///
    /// Scheduler'ın `Agenda` lookup analog — belirli bir slot'ta ne yapılacak.
    pub fn pre_warning_threshold_slots(period_secs: u64) -> u64 {
        let pre_warning_secs = 30 * 24 * 3600u64;
        let effective = period_secs.saturating_sub(pre_warning_secs);
        // saniye → slot (400ms/slot)
        effective.saturating_mul(1000) / SLOT_DURATION_MS
    }

    /// Son uyarı eşiği slot cinsinden.
    pub fn final_warning_threshold_slots(period_secs: u64) -> u64 {
        let final_warning_secs = 7 * 24 * 3600u64;
        let effective = period_secs.saturating_sub(final_warning_secs);
        effective.saturating_mul(1000) / SLOT_DURATION_MS
    }
}

// ─── GUARDIAN ONAY DURUMU ─────────────────────────────────────────────────────

/// Manuel aktivasyon için guardian onay durumu.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuardianApprovalStatus {
    /// Onay veren guardian_id'ler.
    pub approvers: Vec<u64>,
    /// Gerekli onay sayısı.
    pub required: u8,
    /// Threshold karşılandı mı?
    pub threshold_met: bool,
}

impl GuardianApprovalStatus {
    pub fn new(approvers: Vec<u64>, required: u8) -> Self {
        let threshold_met = approvers.len() as u8 >= required;
        Self { approvers, required, threshold_met }
    }

    pub fn current_count(&self) -> u8 {
        self.approvers.len() as u8
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        InheritancePlan, InheritancePlanId, InheritancePhase, InheritanceState,
        TriggerType, HeirRecord, HeirTier, StagedReleaseStep, ConditionalRule,
    };
    use nova_primitives::types::{Address, Balance, SlotId};

    fn make_plan(trigger: TriggerType) -> InheritancePlan {
        let plan_id = InheritancePlanId([0u8; 32]);
        let owner = Address::from_bytes([1u8; 20]);
        let heir = HeirRecord::new(
            Address::from_bytes([2u8; 20]),
            HeirTier::Primary,
            10_000,
            100_000,
        );
        InheritancePlan {
            plan_id,
            owner_account_id: 1,
            owner_address: owner,
            trigger,
            grace_period_slots: 2_000_000,
            heirs: vec![heir],
            release_steps: vec![],
            conditions: vec![],
            created_at_slot: SlotId(0),
            updated_at_slot: SlotId(0),
            is_active: true,
        }
    }

    fn make_state(last_activity_slot: u64) -> InheritanceState {
        let plan_id = InheritancePlanId([0u8; 32]);
        let mut state = InheritanceState::new(plan_id, SlotId(0));
        state.last_activity_slot = SlotId(last_activity_slot);
        state
    }

    // Tetikleyici henüz dolmadı
    #[test]
    fn test_inactivity_not_yet_triggered() {
        let trigger = TriggerType::Inactivity {
            period_secs: 90 * 24 * 3600, // 90 gün
        };
        let plan = make_plan(trigger);
        let state = make_state(0); // last_activity = slot 0

        // Slot 100 → çok kısa süre geçti (100 * 0.4s = 40s)
        let eval = TriggerEvaluator::evaluate(&plan, &state, SlotId(100));
        assert!(matches!(eval, TriggerEvaluation::NotYet { .. }));
    }

    // Ön uyarı eşiği
    #[test]
    fn test_inactivity_pre_warning() {
        let period_secs = 90 * 24 * 3600u64; // 90 gün
        let trigger = TriggerType::Inactivity { period_secs };
        let plan = make_plan(trigger);

        // 90 gün - 25 gün = 65 gün geçti → PreWarning aralığında (60-83 gün)
        let elapsed_secs = 65 * 24 * 3600u64;
        let elapsed_slots = elapsed_secs * 1000 / SLOT_DURATION_MS;
        let state = make_state(0);
        let current = SlotId(elapsed_slots);

        let eval = TriggerEvaluator::evaluate(&plan, &state, current);
        assert!(matches!(eval, TriggerEvaluation::PreWarningShouldStart));
    }

    // Son uyarı eşiği
    #[test]
    fn test_inactivity_final_warning() {
        let period_secs = 90 * 24 * 3600u64;
        let trigger = TriggerType::Inactivity { period_secs };
        let plan = make_plan(trigger);

        // 90 - 5 = 85 gün geçti → FinalWarning (83-90 gün)
        let elapsed_secs = 85 * 24 * 3600u64;
        let elapsed_slots = elapsed_secs * 1000 / SLOT_DURATION_MS;
        let state = make_state(0);
        let current = SlotId(elapsed_slots);

        let eval = TriggerEvaluator::evaluate(&plan, &state, current);
        assert!(matches!(eval, TriggerEvaluation::FinalWarningShouldStart));
    }

    // Tetikleyici aktif
    #[test]
    fn test_inactivity_triggered() {
        let period_secs = 30 * 24 * 3600u64; // 30 gün (minimum)
        let trigger = TriggerType::Inactivity { period_secs };
        let plan = make_plan(trigger);

        // 31 gün geçti → tetiklendi
        let elapsed_secs = 31 * 24 * 3600u64;
        let elapsed_slots = elapsed_secs * 1000 / SLOT_DURATION_MS;
        let state = make_state(0);

        let eval = TriggerEvaluator::evaluate(&plan, &state, SlotId(elapsed_slots));
        assert_eq!(eval, TriggerEvaluation::ShouldTrigger);
    }

    // Son aktiviteden sonra sıfırlanınca tetiklenmez
    // 90 günlük periyot: 1 günlük inaktivite → PreWarning eşiği 60 gün → NotYet
    #[test]
    fn test_inactivity_reset_by_activity() {
        let period_secs = 90 * 24 * 3600u64; // 90 gün — min 30 gün olan periyot
        let trigger = TriggerType::Inactivity { period_secs };
        let plan = make_plan(trigger);

        // 31 gün sonra son aktivite yaptı
        let activity_slot = 31 * 24 * 3600u64 * 1000 / SLOT_DURATION_MS;
        let state = make_state(activity_slot);

        // 1 gün daha geçti → toplam 1 gün inaktif
        // PreWarning eşiği: 90 - 30 = 60 gün → 1 gün < 60 gün → NotYet
        let current = SlotId(activity_slot + 1 * 24 * 3600 * 1000 / SLOT_DURATION_MS);
        let eval = TriggerEvaluator::evaluate(&plan, &state, current);
        assert!(matches!(eval, TriggerEvaluation::NotYet { .. }));
    }

    // FixedSlot: aktivasyon slot'u gelmedi
    #[test]
    fn test_fixed_slot_not_reached() {
        let trigger = TriggerType::FixedSlot {
            activation_slot: SlotId(1_000_000),
            inactivity_period_secs: 30 * 24 * 3600,
        };
        let plan = make_plan(trigger);
        let state = make_state(0);

        let eval = TriggerEvaluator::evaluate(&plan, &state, SlotId(500_000));
        assert!(matches!(eval, TriggerEvaluation::NotYet { .. }));
    }

    // FixedSlot: aktivasyon slot'u geçti, inaktivite doldu
    #[test]
    fn test_fixed_slot_triggered_after_activation() {
        let activation_slot = SlotId(1_000_000);
        let inactivity_period_secs = 30 * 24 * 3600u64;
        let trigger = TriggerType::FixedSlot {
            activation_slot,
            inactivity_period_secs,
        };
        let plan = make_plan(trigger);

        // Aktivasyon slot'undan itibaren son aktivite
        let state = make_state(activation_slot.0);

        // 31 gün sonraki slot
        let elapsed_slots = 31 * 24 * 3600u64 * 1000 / SLOT_DURATION_MS;
        let current = SlotId(activation_slot.0 + elapsed_slots);

        let eval = TriggerEvaluator::evaluate(&plan, &state, current);
        assert_eq!(eval, TriggerEvaluation::ShouldTrigger);
    }

    // ManualActivation: guardian onayı olmadan hazır
    #[test]
    fn test_manual_activation_no_guardian_required() {
        let trigger = TriggerType::ManualActivation {
            requires_guardian_approval: false,
            guardian_threshold: 0,
        };
        let plan = make_plan(trigger);
        let state = make_state(0);

        let eval = TriggerEvaluator::evaluate(&plan, &state, SlotId(100));
        assert_eq!(eval, TriggerEvaluation::ManualActivationReady);
    }

    // ManualActivation: yetersiz guardian onayı
    #[test]
    fn test_manual_activation_insufficient_approvals() {
        let trigger = TriggerType::ManualActivation {
            requires_guardian_approval: true,
            guardian_threshold: 2,
        };
        let plan = make_plan(trigger);
        let mut state = make_state(0);
        state.manual_activation_approvals.push(1); // 1 onay, 2 gerekli

        let eval = TriggerEvaluator::evaluate(&plan, &state, SlotId(100));
        assert!(matches!(eval, TriggerEvaluation::NotYet { .. }));
    }

    // ManualActivation: yeterli guardian onayı
    #[test]
    fn test_manual_activation_sufficient_approvals() {
        let trigger = TriggerType::ManualActivation {
            requires_guardian_approval: true,
            guardian_threshold: 2,
        };
        let plan = make_plan(trigger);
        let mut state = make_state(0);
        state.manual_activation_approvals.push(1);
        state.manual_activation_approvals.push(2);

        let eval = TriggerEvaluator::evaluate(&plan, &state, SlotId(100));
        assert_eq!(eval, TriggerEvaluation::ManualActivationReady);
    }

    // assert_triggered: tetiklenmemişse hata
    #[test]
    fn test_assert_triggered_error() {
        let trigger = TriggerType::Inactivity { period_secs: 30 * 24 * 3600 };
        let plan = make_plan(trigger);
        let state = make_state(0);

        let err = TriggerEvaluator::assert_triggered(&plan, &state, SlotId(100)).unwrap_err();
        assert!(matches!(err, InheritanceError::InactivityPeriodNotElapsed { .. }));
    }

    // assert_triggered: tetiklendiyse Ok
    #[test]
    fn test_assert_triggered_ok() {
        let period_secs = 30 * 24 * 3600u64;
        let trigger = TriggerType::Inactivity { period_secs };
        let plan = make_plan(trigger);
        let state = make_state(0);

        let elapsed = 31 * 24 * 3600 * 1000 / SLOT_DURATION_MS;
        TriggerEvaluator::assert_triggered(&plan, &state, SlotId(elapsed)).unwrap();
    }

    // GuardianApprovalStatus
    #[test]
    fn test_guardian_approval_status() {
        let status = GuardianApprovalStatus::new(vec![1, 2], 3);
        assert!(!status.threshold_met);
        assert_eq!(status.current_count(), 2);

        let status2 = GuardianApprovalStatus::new(vec![1, 2, 3], 3);
        assert!(status2.threshold_met);
    }
}
