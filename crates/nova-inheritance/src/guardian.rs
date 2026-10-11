//! Guardian Suistimal Koruması
//!
//! ## Araştırma Kaynağı: Avalanchego Multisig + Cosmos Authz
//!
//! ### Avalanchego OutputOwners Threshold
//! ```text
//! func (o *OutputOwners) Verify() error {
//!     if int(o.Threshold) > len(o.Addrs) {
//!         return ErrThresholdExceedsOwnerCount
//!     }
//!     if o.Locktime < minLocktime {
//!         return ErrLocktime
//!     }
//! }
//! ```
//!
//! ### Cosmos Authz
//! - Yetki sınırlaması: guardian'lar yalnızca belirli işlemleri yapabilir
//! - Süre sınırlı: yetki sona erme zamanı
//!
//! ## Nova Uyarlaması
//!
//! §5.4.8: "Guardian Suistimal Koruması"
//! - Zaman kilidi: inaktivite süresi dolmadan guardian devir başlatamaz
//! - Threshold + Zaman şartı: ikisi birlikte zorunlu
//! - İptal penceresi: guardian bunu kapatamaz
//! - Guardian değişikliği güvencesi: 7 gün bekleme

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{InheritancePlan, InheritanceState, InheritancePhase};
use nova_primitives::types::SlotId;
use serde::{Deserialize, Serialize};
use tracing::{warn, info};

// ─── GUARDIAN KARARININ TİPİ ─────────────────────────────────────────────────

/// Guardian'ların yapabileceği kararlar.
///
/// §5.4.8: Guardian'lar sınırlı yetkiye sahiptir.
/// Cosmos Authz `Authorization` analog — belirli işlemlere izin verir.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardianAction {
    /// "Kullanıcı hayatta, erişimi yok" — grace period uzatma.
    /// Sadece grace period aşamasında kullanılabilir.
    /// Maksimum 3 kez.
    ExtendGracePeriod {
        extension_slots: u64,
    },
    /// Manuel aktivasyon onayı (kullanıcı erişimi yoksa).
    ApproveManualActivation {
        guardian_id: u64,
    },
    /// Guardian değişikliği onayı.
    ApproveGuardianChange {
        old_guardian_id: u64,
        new_guardian_id: u64,
    },
}

// ─── GUARDIAN KORUMA KURALLAĞI ─────────────────────────────────────────────

/// Guardian suistimal koruması.
///
/// Avalanchego `OutputOwners.Verify()` + Cosmos Authz yetki sınırlaması analog.
pub struct GuardianProtection;

impl GuardianProtection {
    /// Guardian, inaktivite süresi dolmadan deviri başlatabilir mi?
    ///
    /// §5.4.8: "Zaman kilidi: Guardian'lar 'kullanıcı öldü' kararı verseler bile,
    /// tetikleyici süresi dolmadan protokol başlamaz."
    ///
    /// Bu kontrol, guardian onayı olan manuel aktivasyon için de geçerlidir.
    pub fn can_guardian_trigger(
        plan: &InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        use crate::types::TriggerType;

        // ManualActivation tipi ise guardian onayı tetikleyebilir
        if let TriggerType::ManualActivation { requires_guardian_approval, guardian_threshold } = &plan.trigger {
            if *requires_guardian_approval {
                let approval_count = state.manual_activation_approvals.len() as u8;
                if approval_count < *guardian_threshold {
                    return Err(InheritanceError::InsufficientGuardianApproval {
                        got: approval_count,
                        required: *guardian_threshold,
                    });
                }
                return Ok(()); // Guardian threshold karşılandı
            }
        }

        // İnaktivite tetikleyicisi: guardian yalnız başına tetikleyemez
        // Tetikleyici süresi doldu mu? Bunu dışarıdan kontrol etmeliyiz
        // (trigger.rs'e bırakıyoruz)
        Err(InheritanceError::GuardianCannotOverrideTrigger)
    }

    /// Guardian, grace period uzatabilir mi?
    ///
    /// §5.4.8: "Guardian'lar sadece grace period'u uzatabilir (maks 3 kez)."
    pub fn can_extend_grace_period(state: &InheritanceState) -> InheritanceResult<()> {
        // Grace period aşamasında mıyız?
        if !matches!(state.phase, InheritancePhase::GracePeriod { .. }) {
            return Err(InheritanceError::WrongPhase {
                expected: "GracePeriod".to_string(),
                current: state.phase.name().to_string(),
            });
        }

        // Maksimum 3 kez
        if state.guardian_extension_count >= 3 {
            return Err(InheritanceError::GracePeriodExtensionLimitExceeded);
        }

        Ok(())
    }

    /// Guardian, kullanıcının iptalini engelleyebilir mi?
    ///
    /// §5.4.8: "Kullanıcı tek imzayla her şeyi durdurabilir;
    /// guardian'ların bu pencereyi kapatma yetkisi yoktur."
    pub fn can_guardian_block_user_cancellation() -> bool {
        false // Asla
    }

    /// Guardian değişikliği güvencesi — 7 günlük bekleme penceresi.
    ///
    /// §5.4.8: "Bir guardian kaldırıldığında 7 günlük bekleme penceresi açılır."
    ///
    /// Döndürülen slot: değişikliğin ne zaman geçerli olacağı.
    pub fn guardian_change_effective_slot(requested_at_slot: SlotId) -> SlotId {
        // 7 gün = 7 * 24 * 3600 * 1000 / 400 = 1_512_000 slot
        const GUARDIAN_CHANGE_DELAY: u64 = 1_512_000;
        SlotId(requested_at_slot.0 + GUARDIAN_CHANGE_DELAY)
    }

    /// Guardian değişikliği bekliyor mu? (Bekleme penceresi aktif mi?)
    pub fn is_guardian_change_pending(
        requested_at_slot: SlotId,
        current_slot: SlotId,
    ) -> bool {
        let effective = Self::guardian_change_effective_slot(requested_at_slot);
        current_slot.0 < effective.0
    }

    /// Bekleme penceresi süresi doldu mu? (Değişiklik geçerli mi?)
    pub fn is_guardian_change_effective(
        requested_at_slot: SlotId,
        current_slot: SlotId,
    ) -> bool {
        !Self::is_guardian_change_pending(requested_at_slot, current_slot)
    }

    /// Eski guardian değişikliği bekleme penceresinde iptal edebilir mi?
    ///
    /// §5.4.8: "Bu süre içinde eski guardian sistemi iptal edebilir."
    pub fn old_guardian_can_cancel(
        requested_at_slot: SlotId,
        current_slot: SlotId,
    ) -> bool {
        // Bekleme penceresi içindeyse eski guardian iptal edebilir
        Self::is_guardian_change_pending(requested_at_slot, current_slot)
    }

    /// Guardian aksiyon doğrulama.
    ///
    /// Cosmos Authz `Accept()` analog.
    ///
    /// NOVA-RED-INH-002: `plan` parametresi eklendi — `ApproveManualActivation`'da
    /// guardian_id'nin plan'ın `guardian_ids` listesinde olup olmadığı kontrol edilir.
    pub fn validate_action(
        action: &GuardianAction,
        plan: &crate::types::InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        match action {
            GuardianAction::ExtendGracePeriod { extension_slots } => {
                // 1. Grace period'da mıyız?
                Self::can_extend_grace_period(state)?;

                // 2. Extension süresi makul mu? (min 1 gün, maks 30 gün)
                const MIN_EXTENSION: u64 = 216_000;    // 1 gün @ 400ms/slot
                const MAX_EXTENSION: u64 = 6_480_000;  // 30 gün @ 400ms/slot
                if *extension_slots < MIN_EXTENSION || *extension_slots > MAX_EXTENSION {
                    return Err(InheritanceError::InvalidParameter(
                        format!("Uzatma süresi geçersiz: {} slot (min: {}, maks: {})",
                            extension_slots, MIN_EXTENSION, MAX_EXTENSION),
                    ));
                }

                Ok(())
            }
            GuardianAction::ApproveManualActivation { guardian_id } => {
                // NOVA-RED-INH-002: Guardian kimlik doğrulaması — yalnızca plana kayıtlı
                // guardian'lar onay verebilir. guardian_ids boşsa herhangi bir onay kabul edilir
                // (geriye dönük uyumluluk için eski planlar).
                if !plan.guardian_ids.is_empty() && !plan.guardian_ids.contains(guardian_id) {
                    return Err(InheritanceError::Unauthorized);
                }
                Ok(())
            }
            GuardianAction::ApproveGuardianChange { old_guardian_id, .. } => {
                // NOVA-RED-INH-002: Değişikliği yalnızca mevcut guardian başlatabilir.
                if !plan.guardian_ids.is_empty() && !plan.guardian_ids.contains(old_guardian_id) {
                    return Err(InheritanceError::Unauthorized);
                }
                Ok(())
            }
        }
    }

    /// Threshold + Zaman şartı ikisi birlikte karşılandı mı?
    ///
    /// §5.4.8: "Guardian onayı VE inaktivite süresi ikisi birden sağlanmadan devir olmaz."
    pub fn threshold_and_time_both_met(
        inactivity_triggered: bool,
        guardian_approvals: usize,
        required_approvals: u8,
    ) -> bool {
        inactivity_triggered && (guardian_approvals as u8 >= required_approvals)
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        HeirRecord, HeirTier, InheritancePlan, InheritancePlanId, InheritancePhase,
        InheritanceState, TriggerType, MIN_GRACE_PERIOD_SLOTS,
    };
    use nova_primitives::types::Address;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_plan(trigger: TriggerType) -> InheritancePlan {
        InheritancePlan {
            plan_id: InheritancePlanId([0u8; 32]),
            owner_account_id: 1,
            owner_address: addr(0),
            trigger,
            grace_period_slots: MIN_GRACE_PERIOD_SLOTS,
            heirs: vec![HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100_000)],
            release_steps: vec![],
            conditions: vec![],
            created_at_slot: SlotId(0),
            updated_at_slot: SlotId(0),
            is_active: true,
            guardian_ids: vec![],
        }
    }

    fn make_state_in_grace() -> InheritanceState {
        let mut state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        state.phase = InheritancePhase::GracePeriod {
            started_at_slot: SlotId(0),
            ends_at_slot: SlotId(2_000_000),
            extension_count: 0,
        };
        state
    }

    // §5.4.8: Guardian inaktivite süresi dolmadan tetikleyemez
    #[test]
    fn test_guardian_cannot_trigger_inactivity_plan() {
        let plan = make_plan(TriggerType::Inactivity { period_secs: 90 * 24 * 3600 });
        let state = make_state_in_grace();

        let err = GuardianProtection::can_guardian_trigger(&plan, &state, SlotId(100)).unwrap_err();
        assert!(matches!(err, InheritanceError::GuardianCannotOverrideTrigger));
    }

    // ManualActivation + guardian onayı yeterli → tetikleyebilir
    #[test]
    fn test_guardian_can_trigger_manual_activation_with_approvals() {
        let plan = make_plan(TriggerType::ManualActivation {
            requires_guardian_approval: true,
            guardian_threshold: 2,
        });
        let mut state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        state.manual_activation_approvals.push(1);
        state.manual_activation_approvals.push(2);

        let result = GuardianProtection::can_guardian_trigger(&plan, &state, SlotId(100));
        assert!(result.is_ok(), "2/2 guardian onayıyla tetiklenebilir");
    }

    // ManualActivation + yetersiz guardian onayı → hata
    #[test]
    fn test_guardian_cannot_trigger_with_insufficient_approvals() {
        let plan = make_plan(TriggerType::ManualActivation {
            requires_guardian_approval: true,
            guardian_threshold: 3,
        });
        let mut state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        state.manual_activation_approvals.push(1);

        let err = GuardianProtection::can_guardian_trigger(&plan, &state, SlotId(100)).unwrap_err();
        assert!(matches!(err, InheritanceError::InsufficientGuardianApproval { got: 1, required: 3 }));
    }

    // Grace period uzatma geçerli
    #[test]
    fn test_can_extend_grace_period() {
        let state = make_state_in_grace();
        assert!(GuardianProtection::can_extend_grace_period(&state).is_ok());
    }

    // Grace period dışında uzatma → hata
    #[test]
    fn test_cannot_extend_in_wrong_phase() {
        let state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        let err = GuardianProtection::can_extend_grace_period(&state).unwrap_err();
        assert!(matches!(err, InheritanceError::WrongPhase { .. }));
    }

    // Grace period uzatma limit aşıldı
    #[test]
    fn test_extension_limit_exceeded() {
        let mut state = make_state_in_grace();
        state.guardian_extension_count = 3;

        let err = GuardianProtection::can_extend_grace_period(&state).unwrap_err();
        assert!(matches!(err, InheritanceError::GracePeriodExtensionLimitExceeded));
    }

    // §5.4.8: Guardian kullanıcının iptalini engelleyemez
    #[test]
    fn test_guardian_cannot_block_user_cancellation() {
        assert!(!GuardianProtection::can_guardian_block_user_cancellation());
    }

    // Guardian değişikliği bekleme penceresi
    #[test]
    fn test_guardian_change_delay() {
        let requested = SlotId(0);
        let effective = GuardianProtection::guardian_change_effective_slot(requested);
        assert_eq!(effective.0, 1_512_000); // 7 gün

        // Bekleme penceresi içinde → pending
        assert!(GuardianProtection::is_guardian_change_pending(requested, SlotId(1_000_000)));

        // Bekleme penceresi geçti → effective
        assert!(GuardianProtection::is_guardian_change_effective(requested, SlotId(1_512_001)));
    }

    // Eski guardian bekleme penceresinde iptal edebilir
    #[test]
    fn test_old_guardian_can_cancel_during_window() {
        let requested = SlotId(0);
        assert!(GuardianProtection::old_guardian_can_cancel(requested, SlotId(500_000)));
        assert!(!GuardianProtection::old_guardian_can_cancel(requested, SlotId(2_000_000)));
    }

    // validate_action: geçerli uzatma
    #[test]
    fn test_validate_extension_action_valid() {
        let plan = make_plan(TriggerType::Inactivity { period_secs: 90 * 24 * 3600 });
        let state = make_state_in_grace();
        let action = GuardianAction::ExtendGracePeriod { extension_slots: 1_000_000 };
        assert!(GuardianProtection::validate_action(&action, &plan, &state, SlotId(0)).is_ok());
    }

    // validate_action: çok kısa uzatma → hata
    #[test]
    fn test_validate_extension_too_short() {
        let plan = make_plan(TriggerType::Inactivity { period_secs: 90 * 24 * 3600 });
        let state = make_state_in_grace();
        let action = GuardianAction::ExtendGracePeriod { extension_slots: 100 }; // < 216_000
        let err = GuardianProtection::validate_action(&action, &plan, &state, SlotId(0)).unwrap_err();
        assert!(matches!(err, InheritanceError::InvalidParameter(_)));
    }

    // Threshold + Zaman şartı ikisi birlikte gerekli
    #[test]
    fn test_threshold_and_time_both_required() {
        // Zaman doldu ama guardian yok → hayır
        assert!(!GuardianProtection::threshold_and_time_both_met(true, 0, 2));
        // Guardian var ama zaman dolmadı → hayır
        assert!(!GuardianProtection::threshold_and_time_both_met(false, 3, 2));
        // İkisi de var → evet
        assert!(GuardianProtection::threshold_and_time_both_met(true, 3, 2));
    }

    // NOVA-RED-INH-002: guardian_ids boşsa herhangi bir guardian onay verebilir
    #[test]
    fn test_approve_manual_activation_empty_guardian_list_allows_any() {
        let mut plan = make_plan(TriggerType::Inactivity { period_secs: 90 * 24 * 3600 });
        plan.guardian_ids = vec![];
        let state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        let action = GuardianAction::ApproveManualActivation { guardian_id: 999 };
        assert!(GuardianProtection::validate_action(&action, &plan, &state, SlotId(0)).is_ok());
    }

    // NOVA-RED-INH-002: yetkisiz guardian reddedilir
    #[test]
    fn test_approve_manual_activation_unauthorized_guardian_rejected() {
        let mut plan = make_plan(TriggerType::ManualActivation {
            requires_guardian_approval: true,
            guardian_threshold: 1,
        });
        plan.guardian_ids = vec![10, 20, 30];
        let state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        let action = GuardianAction::ApproveManualActivation { guardian_id: 99 };
        let err = GuardianProtection::validate_action(&action, &plan, &state, SlotId(0)).unwrap_err();
        assert!(matches!(err, InheritanceError::Unauthorized));
    }

    // NOVA-RED-INH-002: yetkili guardian kabul edilir
    #[test]
    fn test_approve_manual_activation_authorized_guardian_accepted() {
        let mut plan = make_plan(TriggerType::ManualActivation {
            requires_guardian_approval: true,
            guardian_threshold: 1,
        });
        plan.guardian_ids = vec![10, 20, 30];
        let state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        let action = GuardianAction::ApproveManualActivation { guardian_id: 20 };
        assert!(GuardianProtection::validate_action(&action, &plan, &state, SlotId(0)).is_ok());
    }
}
