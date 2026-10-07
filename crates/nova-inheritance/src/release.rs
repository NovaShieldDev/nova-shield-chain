//! Kademeli Devir (Staged Release) Yönetimi
//!
//! ## Araştırma Kaynağı: Cosmos SDK x/auth/vesting
//!
//! `research/chains/cosmos-sdk/x/auth/vesting/types/period.go`:
//! ```text
//! // Cosmos PeriodicVestingAccount: her period belirli miktar serbest bırakır
//! type Period struct {
//!     Length int64     // saniye (slot → saniye dönüşümü Nova'da)
//!     Amount sdk.Coins // o period'da serbest bırakılan miktar
//! }
//!
//! type Periods []Period
//! func (p Periods) TotalLength() int64 { ... }
//! func (p Periods) TotalAmount() sdk.Coins { ... }
//! ```
//!
//! Cosmos Vesting'den öğrenilenler:
//! - Period = (delay, amount) çifti
//! - Kümülatif hesap: start_time + Σ periods[0..i].Length = period i'nin zamanı
//! - TotalAmount() doğrulama: tüm periodlar toplamı = hesap bakiyesi
//!
//! ### Polkadot Scheduler
//! ```text
//! pub struct Scheduled<Name, Call, BlockNumber, ...> {
//!     maybe_periodic: Option<Period<BlockNumber>>,
//!     // ... belirtilen blokta tekrar çalıştır
//! }
//! ```
//!
//! ## Nova Uyarlaması
//!
//! §5.4.6: "Varlıkların tamamı tek seferde devredilmez."
//! Cosmos'un `PeriodicVestingAccount` + Polkadot'un `Scheduler` birleşimi.

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{InheritancePlan, InheritanceState, StagedReleaseStep, TransferCommand};
use nova_primitives::types::{Address, SlotId};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ─── SERBEST BIRAKILACAK ADIM SORGUSU ────────────────────────────────────────

/// Bir slot'ta çalıştırılacak adımlar.
///
/// Cosmos Vesting `GetVestedCoins(t)` analog.
/// Polkadot Scheduler `Agenda[blockNumber]` analog.
pub struct ReleaseEngine;

impl ReleaseEngine {
    /// Bu slot'ta çalıştırılabilecek devir adımlarını bul.
    ///
    /// Cosmos Vesting: `for period in periods { if time > start+length → vest }`
    /// Polkadot Scheduler: `on_initialize(now)` — bu blokta çalışacak görevler
    pub fn pending_steps<'a>(
        plan: &'a InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
    ) -> Vec<&'a StagedReleaseStep> {
        let Some(release_start) = state.release_started_at else {
            return vec![];
        };

        plan.release_steps
            .iter()
            .filter(|step| {
                !step.executed && step.is_due(release_start, current_slot)
            })
            .collect()
    }

    /// Belirli bir adımı çalıştır (transfer komutu üret).
    ///
    /// Cosmos Vesting: `SendCoins(from, to, amount)` analog.
    /// Gerçek transfer `nova-runtime` tarafından yapılır.
    pub fn execute_step(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        step_index: u32,
        total_balance: u128,
        current_slot: SlotId,
    ) -> InheritanceResult<TransferCommand> {
        // Adım var mı?
        let step = plan.release_steps.get(step_index as usize)
            .ok_or(InheritanceError::ReleaseStepNotDue {
                step: step_index,
                required_slot: current_slot,
            })?;

        // Zaten çalıştırıldı mı?
        if step.executed {
            return Err(InheritanceError::ReleaseStepAlreadyExecuted { step: step_index });
        }

        // Önceki adım tamamlandı mı? (step_index > 0 ise)
        if step_index > 0 {
            let prev = plan.release_steps.get((step_index - 1) as usize)
                .ok_or(InheritanceError::PreviousReleaseStepPending { step: step_index - 1 })?;
            if !prev.executed {
                return Err(InheritanceError::PreviousReleaseStepPending { step: step_index - 1 });
            }
        }

        // Hazır mı?
        let release_start = state.release_started_at
            .ok_or(InheritanceError::WrongPhase {
                expected: "StagedRelease".to_string(),
                current: state.phase.name().to_string(),
            })?;

        if !step.is_due(release_start, current_slot) {
            let due_at = SlotId(release_start.0 + step.delay_slots);
            return Err(InheritanceError::ReleaseStepNotDue {
                step: step_index,
                required_slot: due_at,
            });
        }

        // Transfer miktarı
        let amount = step.amount_for(total_balance);
        let recipient = step.recipient;

        info!(
            plan_id = %hex::encode(&plan.plan_id.0[..4]),
            step = step_index,
            amount,
            slot = current_slot.0,
            "Kademeli devir adımı çalıştırılıyor"
        );

        // State güncelle (executed flag)
        // NOT: Gerçek state değişikliği nova-runtime tarafından onaylanınca yapılır
        // Burada sadece komut üretiyoruz

        let cmd = TransferCommand {
            plan_id: plan.plan_id,
            step_index,
            from: plan.owner_address,
            to: recipient,
            amount,
            execute_at_slot: current_slot,
            requires_multisig: Self::step_requires_multisig(plan, step_index),
        };

        Ok(cmd)
    }

    /// Tüm bekleyen transfer komutlarını üret.
    ///
    /// Cosmos Vesting `GetVestedCoins` + Polkadot Scheduler `on_initialize` birleşimi.
    pub fn execute_pending_steps(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        total_balance: u128,
        current_slot: SlotId,
    ) -> Vec<InheritanceResult<TransferCommand>> {
        let pending_indices: Vec<u32> = {
            let Some(release_start) = state.release_started_at else {
                return vec![];
            };
            plan.release_steps
                .iter()
                .filter(|s| !s.executed && s.is_due(release_start, current_slot))
                .map(|s| s.step_index)
                .collect()
        };

        pending_indices
            .into_iter()
            .map(|idx| Self::execute_step(plan, state, idx, total_balance, current_slot))
            .collect()
    }

    /// Adım tamamlandı — state güncelle.
    pub fn mark_step_completed(
        plan: &InheritancePlan,
        state: &mut InheritanceState,
        step_index: u32,
        transferred_amount: u128,
        current_slot: SlotId,
    ) -> InheritanceResult<()> {
        // Adım var mı?
        let _ = plan.release_steps.get(step_index as usize)
            .ok_or(InheritanceError::ReleaseStepAlreadyExecuted { step: step_index })?;

        state.total_transferred.0 = state.total_transferred.0
            .saturating_add(transferred_amount);
        state.updated_at_slot = current_slot;

        // StagedRelease aşamasındaki completed_steps güncelle
        if let crate::types::InheritancePhase::StagedRelease { completed_steps, .. } = &mut state.phase {
            *completed_steps = completed_steps.saturating_add(1);
        }

        Ok(())
    }

    /// Tüm adımlar tamamlandı mı?
    pub fn all_steps_completed(plan: &InheritancePlan, state: &InheritanceState) -> bool {
        if plan.release_steps.is_empty() {
            return true;
        }
        // State'in completed_steps sayısına bak
        if let crate::types::InheritancePhase::StagedRelease { completed_steps, total_steps, .. } = &state.phase {
            *completed_steps >= *total_steps
        } else {
            false
        }
    }

    /// Belirli bir adımın MultiSig gerektirip gerektirmediğini kontrol et.
    fn step_requires_multisig(plan: &InheritancePlan, _step_index: u32) -> bool {
        use crate::types::ConditionalRule;
        plan.conditions.iter().any(|c| matches!(c, ConditionalRule::MultiSigRequired { .. }))
    }

    /// Kalan devredilecek miktar.
    ///
    /// Cosmos Vesting `GetVestingCoins(t)` analog.
    pub fn remaining_amount(plan: &InheritancePlan, total_balance: u128, state: &InheritanceState) -> u128 {
        total_balance.saturating_sub(state.total_transferred.0)
    }

    /// Release planı özeti.
    pub fn plan_summary(plan: &InheritancePlan, state: &InheritanceState, current_slot: SlotId) -> ReleasePlanSummary {
        let total_steps = plan.release_steps.len();
        let completed = plan.release_steps.iter().filter(|s| s.executed).count();

        let next_step = plan.release_steps
            .iter()
            .find(|s| !s.executed)
            .map(|s| {
                let release_start = state.release_started_at.unwrap_or(current_slot);
                let due_at = SlotId(release_start.0 + s.delay_slots);
                (s.step_index, due_at)
            });

        ReleasePlanSummary {
            total_steps,
            completed_steps: completed,
            remaining_steps: total_steps.saturating_sub(completed),
            next_step_index: next_step.map(|(i, _)| i),
            next_step_due_at: next_step.map(|(_, s)| s),
            total_transferred: state.total_transferred.0,
        }
    }
}

/// Kademeli devir özeti.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleasePlanSummary {
    pub total_steps: usize,
    pub completed_steps: usize,
    pub remaining_steps: usize,
    pub next_step_index: Option<u32>,
    pub next_step_due_at: Option<SlotId>,
    pub total_transferred: u128,
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        HeirRecord, HeirTier, InheritancePlan, InheritancePlanId, InheritancePhase,
        InheritanceState, StagedReleaseStep, TriggerType,
    };
    use nova_primitives::types::{Address, Balance};

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_plan_with_steps(steps: Vec<StagedReleaseStep>) -> InheritancePlan {
        InheritancePlan {
            plan_id: InheritancePlanId([0u8; 32]),
            owner_account_id: 1,
            owner_address: addr(0),
            trigger: TriggerType::Inactivity { period_secs: 30 * 24 * 3600 },
            grace_period_slots: 2_000_000,
            heirs: vec![HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100_000)],
            release_steps: steps,
            conditions: vec![],
            created_at_slot: SlotId(0),
            updated_at_slot: SlotId(0),
            is_active: true,
        }
    }

    fn make_state_in_release(release_start: u64) -> InheritanceState {
        let mut state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));
        state.release_started_at = Some(SlotId(release_start));
        state.phase = InheritancePhase::StagedRelease {
            started_at_slot: SlotId(release_start),
            completed_steps: 0,
            total_steps: 3,
        };
        state
    }

    // Cosmos Vesting: anında serbest bırakılan pay
    #[test]
    fn test_immediate_step_due() {
        let steps = vec![
            StagedReleaseStep::new(0, 0, 5_000, addr(1)), // Anında %50
            StagedReleaseStep::new(1, 100_000, 5_000, addr(2)), // 100k slot sonra %50
        ];
        let plan = make_plan_with_steps(steps);
        let state = make_state_in_release(1000);

        let pending = ReleaseEngine::pending_steps(&plan, &state, SlotId(1000));
        assert_eq!(pending.len(), 1, "Sadece anındaki adım hazır");
        assert_eq!(pending[0].step_index, 0);
    }

    // Cosmos Vesting: delay sonrası adım
    #[test]
    fn test_delayed_step_due_after_delay() {
        let steps = vec![
            StagedReleaseStep::new(0, 0, 5_000, addr(1)),
            StagedReleaseStep::new(1, 100_000, 5_000, addr(2)),
        ];
        let plan = make_plan_with_steps(steps);
        let state = make_state_in_release(0);

        // 100_001 slot sonra → her iki adım hazır (ama 0 zaten executed değil)
        let pending = ReleaseEngine::pending_steps(&plan, &state, SlotId(100_001));
        assert_eq!(pending.len(), 2, "Her iki adım hazır");
    }

    // execute_step: anında transfer komutu
    #[test]
    fn test_execute_immediate_step() {
        let steps = vec![StagedReleaseStep::new(0, 0, 10_000, addr(1))];
        let plan = make_plan_with_steps(steps);
        let mut state = make_state_in_release(0);

        let cmd = ReleaseEngine::execute_step(&plan, &mut state, 0, 1_000_000, SlotId(0)).unwrap();
        assert_eq!(cmd.amount, 1_000_000);
        assert_eq!(cmd.to, addr(1));
        assert_eq!(cmd.step_index, 0);
    }

    // execute_step: henüz hazır değil → hata
    #[test]
    fn test_execute_step_not_due() {
        let steps = vec![StagedReleaseStep::new(0, 1_000_000, 10_000, addr(1))];
        let plan = make_plan_with_steps(steps);
        let mut state = make_state_in_release(0);

        let err = ReleaseEngine::execute_step(&plan, &mut state, 0, 1_000_000, SlotId(0)).unwrap_err();
        assert!(matches!(err, InheritanceError::ReleaseStepNotDue { step: 0, .. }));
    }

    // mark_step_completed: state güncellenir
    #[test]
    fn test_mark_step_completed_updates_state() {
        let steps = vec![StagedReleaseStep::new(0, 0, 10_000, addr(1))];
        let plan = make_plan_with_steps(steps);
        let mut state = make_state_in_release(0);

        ReleaseEngine::mark_step_completed(&plan, &mut state, 0, 500_000, SlotId(100)).unwrap();
        assert_eq!(state.total_transferred.0, 500_000);
    }

    // Cosmos Vesting: toplam miktar doğrulama
    #[test]
    fn test_total_steps_fraction_sums_to_10000() {
        let steps = vec![
            StagedReleaseStep::new(0, 0, 2_500, addr(1)),
            StagedReleaseStep::new(1, 100_000, 2_500, addr(1)),
            StagedReleaseStep::new(2, 500_000, 3_000, addr(2)),
            StagedReleaseStep::new(3, 5_000_000, 2_000, addr(3)),
        ];
        let total: u32 = steps.iter().map(|s| s.fraction_bps).sum();
        assert_eq!(total, 10_000, "Cosmos Vesting: tüm periodlar toplamı = bakiye");
    }

    // Kısmi miktar hesabı
    #[test]
    fn test_step_amount_calculation() {
        let step = StagedReleaseStep::new(0, 0, 2_500, addr(1)); // %25
        let amount = step.amount_for(1_000_000);
        assert_eq!(amount, 250_000);
    }

    // all_steps_completed: tamamlandığında true
    #[test]
    fn test_all_steps_completed() {
        let steps = vec![
            StagedReleaseStep::new(0, 0, 5_000, addr(1)),
            StagedReleaseStep::new(1, 100_000, 5_000, addr(2)),
        ];
        let plan = make_plan_with_steps(steps);
        let mut state = make_state_in_release(0);

        // Başlangıçta tamamlanmadı
        assert!(!ReleaseEngine::all_steps_completed(&plan, &state));

        // 2 adım tamamlandı — total_steps de 2 olarak güncelle (plan'da 2 adım var)
        if let InheritancePhase::StagedRelease { ref mut completed_steps, ref mut total_steps, .. } = state.phase {
            *total_steps = 2;
            *completed_steps = 2;
        }
        assert!(ReleaseEngine::all_steps_completed(&plan, &state));
    }

    // remaining_amount
    #[test]
    fn test_remaining_amount() {
        let plan = make_plan_with_steps(vec![]);
        let mut state = make_state_in_release(0);
        state.total_transferred = Balance(300_000);

        let remaining = ReleaseEngine::remaining_amount(&plan, 1_000_000, &state);
        assert_eq!(remaining, 700_000);
    }

    // plan_summary: doğru meta veri
    #[test]
    fn test_release_plan_summary() {
        let steps = vec![
            StagedReleaseStep::new(0, 0, 5_000, addr(1)),
            StagedReleaseStep::new(1, 200_000, 5_000, addr(2)),
        ];
        let plan = make_plan_with_steps(steps);
        let state = make_state_in_release(1000);

        let summary = ReleaseEngine::plan_summary(&plan, &state, SlotId(1000));
        assert_eq!(summary.total_steps, 2);
        assert_eq!(summary.completed_steps, 0);
        assert_eq!(summary.remaining_steps, 2);
        assert_eq!(summary.next_step_index, Some(0));
    }

    // release_start yoksa pending_steps boş
    #[test]
    fn test_pending_steps_without_release_start() {
        let steps = vec![StagedReleaseStep::new(0, 0, 10_000, addr(1))];
        let plan = make_plan_with_steps(steps);
        let state = InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0));

        let pending = ReleaseEngine::pending_steps(&plan, &state, SlotId(1000));
        assert!(pending.is_empty());
    }
}
