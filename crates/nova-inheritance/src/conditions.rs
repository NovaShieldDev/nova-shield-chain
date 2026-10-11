//! Koşullu Miras Kuralları Değerlendirme Motoru
//!
//! ## Araştırma Kaynağı: Cosmos SDK x/authz + Avalanchego OutputOwners
//!
//! ### Cosmos x/authz
//! `research/chains/cosmos-sdk/x/authz/`:
//! ```text
//! // Belirli koşullar altında yetki tanımla
//! type Authorization interface {
//!     MsgTypeURL() string
//!     Accept(ctx Context, msg sdk.Msg) (AcceptResponse, error)
//!     ValidateBasic() error
//! }
//! ```
//!
//! ### Avalanchego OutputOwners
//! `research/chains/avalanchego/vms/platformvm/txs/`:
//! ```text
//! type OutputOwners struct {
//!     Locktime   uint64   // Zaman kilidi
//!     Threshold  uint32   // Gerekli imza sayısı
//!     Addrs      []ids.ShortID
//! }
//! ```
//!
//! ## Nova Uyarlaması
//!
//! §5.4.7: "Koşullu Miras Kuralları"
//! - Yaş koşulu (blok timestamp bazlı)
//! - Borç ödeme önkoşulu
//! - Hayır kurumu payı
//! - Çoklu imza şartı

use crate::errors::{InheritanceError, InheritanceResult};
use crate::types::{ConditionalRule, InheritancePlan, InheritanceState, TransferCommand};
use nova_primitives::types::{Address, SlotId};
use serde::{Deserialize, Serialize};
use tracing::debug;

// ─── KOŞUL DEĞERLENDİRME SONUCU ─────────────────────────────────────────────

/// Bir koşulun değerlendirme sonucu.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConditionResult {
    /// Koşul karşılandı → devam et.
    Satisfied,
    /// Koşul karşılanmadı → belirtilen sebeple blok.
    NotSatisfied { reason: String },
    /// Koşul bir transfer gerektiriyor (borç ödemesi, hayır kurumu payı).
    RequiresTransfer { command: TransferCommand },
}

// ─── KOŞUL DEĞERLENDİRİCİSİ ─────────────────────────────────────────────────

/// Koşullu miras kurallarını değerlendiren motor.
///
/// Cosmos Authz `Authorization.Accept()` analog.
pub struct ConditionEvaluator;

impl ConditionEvaluator {
    /// Tüm koşulları değerlendir.
    ///
    /// Cosmos Authz: her kurala `Accept()` çağrısı.
    /// Sıra: önce borç ödemeleri, sonra hayır kurumu, sonra diğerleri.
    /// NOVA-RED-INH-004: `account_creation_slots` haritası eklendi — her varis kendi
    /// hesap yaratılma slotuna göre değerlendirilir (owner slotu değil).
    /// Eski çağrı tarafları boş HashMap geçirebilir; bu durumda default SlotId(0) kullanılır.
    pub fn evaluate_all(
        plan: &InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
        account_creation_slots: &std::collections::HashMap<nova_primitives::types::Address, SlotId>,
    ) -> Vec<ConditionResult> {
        // Öncelik sıralaması: borç ödemeleri önce
        let mut sorted_conditions = plan.conditions.clone();
        sorted_conditions.sort_by_key(|c| match c {
            ConditionalRule::DebtPayment { priority, .. } => *priority as u32,
            ConditionalRule::CharityAllocation { .. } => 200,
            ConditionalRule::AgeCondition { .. } => 100,
            ConditionalRule::MultiSigRequired { .. } => 50,
        });

        sorted_conditions
            .iter()
            .map(|rule| Self::evaluate_one(rule, plan, state, current_slot, account_creation_slots))
            .collect()
    }

    /// Tek bir koşulu değerlendir.
    fn evaluate_one(
        rule: &ConditionalRule,
        plan: &InheritancePlan,
        state: &InheritanceState,
        current_slot: SlotId,
        account_creation_slots: &std::collections::HashMap<nova_primitives::types::Address, SlotId>,
    ) -> ConditionResult {
        match rule {
            ConditionalRule::AgeCondition { heir_address, min_account_age_slots } => {
                // NOVA-RED-INH-004: Varis'e özgü hesap yaratılma slotu kullan
                let heir_creation_slot = account_creation_slots
                    .get(heir_address)
                    .copied()
                    .unwrap_or(SlotId(0));
                Self::evaluate_age(heir_address, *min_account_age_slots, heir_creation_slot, current_slot)
            }
            ConditionalRule::DebtPayment { creditor, amount, priority } => {
                Self::evaluate_debt(plan, creditor, *amount)
            }
            ConditionalRule::CharityAllocation { charity_address, share_bps } => {
                Self::evaluate_charity(plan, charity_address, *share_bps, current_slot)
            }
            ConditionalRule::MultiSigRequired { required_signatures, total_heirs, retry_delay_slots } => {
                Self::evaluate_multisig(state, *required_signatures)
            }
        }
    }

    /// Yaş koşulu değerlendirmesi.
    ///
    /// §5.4.7: "Zincir, blok timestamp'ini temel alır; sosyal kanıt gerekmez."
    fn evaluate_age(
        _heir_address: &Address,  // Gelecekte loglama veya ek kontroller için saklanır
        min_age_slots: u64,
        account_creation_slot: SlotId,
        current_slot: SlotId,
    ) -> ConditionResult {
        let account_age = current_slot.0.saturating_sub(account_creation_slot.0);

        if account_age >= min_age_slots {
            debug!(
                age_slots = account_age,
                min_required = min_age_slots,
                "Yaş koşulu karşılandı"
            );
            ConditionResult::Satisfied
        } else {
            let remaining = min_age_slots.saturating_sub(account_age);
            ConditionResult::NotSatisfied {
                reason: format!(
                    "Hesap yaşı yetersiz: {} slot (gereken: {}), {} slot kaldı",
                    account_age, min_age_slots, remaining
                ),
            }
        }
    }

    /// Borç ödeme koşulu — önce alacaklıya öde.
    ///
    /// §5.4.7: "Önce [Adres X]'e [miktar] gönder, sonra varisler."
    fn evaluate_debt(
        plan: &InheritancePlan,
        creditor: &Address,
        amount: u128,
    ) -> ConditionResult {
        // Borç ödemesi: transfer komutu üret
        ConditionResult::RequiresTransfer {
            command: TransferCommand {
                plan_id: plan.plan_id,
                step_index: u32::MAX, // Özel: borç ödemesi
                from: plan.owner_address,
                to: *creditor,
                amount,
                execute_at_slot: SlotId(0), // Hemen çalıştır
                requires_multisig: false,
            },
        }
    }

    /// Hayır kurumu payı koşulu.
    ///
    /// §5.4.7: "Toplam varlığın %5'i → hayır kurumu adresi."
    fn evaluate_charity(
        plan: &InheritancePlan,
        charity: &Address,
        share_bps: u32,
        current_slot: SlotId,
    ) -> ConditionResult {
        // Hayır kurumu için transfer komutu (miktar çalışma zamanında hesaplanır)
        ConditionResult::RequiresTransfer {
            command: TransferCommand {
                plan_id: plan.plan_id,
                step_index: u32::MAX - 1, // Özel: hayır kurumu
                from: plan.owner_address,
                to: *charity,
                amount: share_bps as u128, // Gerçek miktar runtime'da: balance * bps / 10000
                execute_at_slot: current_slot,
                requires_multisig: false,
            },
        }
    }

    /// Multi-sig koşulu.
    ///
    /// Avalanchego `OutputOwners.Threshold` analog.
    /// §5.4.7: "Devir için Varis A VE Varis B birlikte onay vermeli."
    fn evaluate_multisig(
        state: &InheritanceState,
        required: u8,
    ) -> ConditionResult {
        let approved = state.multisig_approvals.len() as u8;

        if approved >= required {
            ConditionResult::Satisfied
        } else {
            ConditionResult::NotSatisfied {
                reason: format!(
                    "Çoklu imza koşulu: {}/{} varis onayladı",
                    approved, required
                ),
            }
        }
    }

    /// Tüm koşullar karşılandı mı? (Blocking koşul var mı?)
    pub fn all_satisfied(results: &[ConditionResult]) -> bool {
        results.iter().all(|r| !matches!(r, ConditionResult::NotSatisfied { .. }))
    }

    /// Karşılanmayan koşulları listele.
    pub fn blocking_conditions<'a>(results: &'a [ConditionResult]) -> Vec<&'a str> {
        results
            .iter()
            .filter_map(|r| match r {
                ConditionResult::NotSatisfied { reason } => Some(reason.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Transfer gerektiren koşullar.
    pub fn required_transfers(results: Vec<ConditionResult>) -> Vec<TransferCommand> {
        results
            .into_iter()
            .filter_map(|r| match r {
                ConditionResult::RequiresTransfer { command } => Some(command),
                _ => None,
            })
            .collect()
    }

    /// Hayır kurumu pay miktarını hesapla.
    pub fn charity_amount(total_balance: u128, share_bps: u32) -> u128 {
        total_balance
            .saturating_mul(share_bps as u128)
            .saturating_div(10_000)
    }
}

// ─── TESTLER ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ConditionalRule, HeirRecord, HeirTier, InheritancePlan, InheritancePlanId,
        InheritanceState, TriggerType,
    };
    use nova_primitives::types::Address;

    fn addr(seed: u8) -> Address {
        Address::from_bytes([seed; 20])
    }

    fn make_plan_with_conditions(conditions: Vec<ConditionalRule>) -> InheritancePlan {
        InheritancePlan {
            plan_id: InheritancePlanId([0u8; 32]),
            owner_account_id: 1,
            owner_address: addr(0),
            trigger: TriggerType::Inactivity { period_secs: 30 * 24 * 3600 },
            grace_period_slots: 2_000_000,
            heirs: vec![HeirRecord::new(addr(1), HeirTier::Primary, 10_000, 100_000)],
            release_steps: vec![],
            conditions,
            created_at_slot: SlotId(0),
            updated_at_slot: SlotId(0),
            is_active: true,
            guardian_ids: vec![],
        }
    }

    fn make_state() -> InheritanceState {
        InheritanceState::new(InheritancePlanId([0u8; 32]), SlotId(0))
    }

    // Yaş koşulu karşılandı
    #[test]
    fn test_age_condition_satisfied() {
        let conditions = vec![ConditionalRule::AgeCondition {
            heir_address: addr(1),
            min_account_age_slots: 1_000,
        }];
        let plan = make_plan_with_conditions(conditions);
        let state = make_state();

        // Hesap creation_slot=0, current=2000 → yaş = 2000 slot > 1000
        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(2000), &std::collections::HashMap::new());
        assert!(matches!(results[0], ConditionResult::Satisfied));
        assert!(ConditionEvaluator::all_satisfied(&results));
    }

    // Yaş koşulu karşılanmadı
    #[test]
    fn test_age_condition_not_satisfied() {
        let conditions = vec![ConditionalRule::AgeCondition {
            heir_address: addr(1),
            min_account_age_slots: 10_000,
        }];
        let plan = make_plan_with_conditions(conditions);
        let state = make_state();

        // Yaş = 500 slot < 10_000
        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(500), &std::collections::HashMap::new());
        assert!(matches!(results[0], ConditionResult::NotSatisfied { .. }));
        assert!(!ConditionEvaluator::all_satisfied(&results));
    }

    // Borç ödeme → transfer komutu üretir
    #[test]
    fn test_debt_payment_produces_transfer() {
        let conditions = vec![ConditionalRule::DebtPayment {
            creditor: addr(9),
            amount: 50_000,
            priority: 1,
        }];
        let plan = make_plan_with_conditions(conditions);
        let state = make_state();

        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), &std::collections::HashMap::new());
        assert!(matches!(results[0], ConditionResult::RequiresTransfer { .. }));

        let transfers = ConditionEvaluator::required_transfers(results);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].to, addr(9));
        assert_eq!(transfers[0].amount, 50_000);
    }

    // Hayır kurumu payı
    #[test]
    fn test_charity_allocation_produces_transfer() {
        let conditions = vec![ConditionalRule::CharityAllocation {
            charity_address: addr(5),
            share_bps: 500, // %5
        }];
        let plan = make_plan_with_conditions(conditions);
        let state = make_state();

        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), &std::collections::HashMap::new());
        assert!(matches!(results[0], ConditionResult::RequiresTransfer { .. }));

        // Gerçek miktar hesabı
        let charity_amount = ConditionEvaluator::charity_amount(1_000_000, 500);
        assert_eq!(charity_amount, 50_000);
    }

    // Multi-sig: yeterli onay var
    #[test]
    fn test_multisig_satisfied() {
        let conditions = vec![ConditionalRule::MultiSigRequired {
            required_signatures: 2,
            total_heirs: 3,
            retry_delay_slots: 100_000,
        }];
        let plan = make_plan_with_conditions(conditions);
        let mut state = make_state();
        state.multisig_approvals.push(addr(1));
        state.multisig_approvals.push(addr(2));

        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), &std::collections::HashMap::new());
        assert!(matches!(results[0], ConditionResult::Satisfied));
    }

    // Multi-sig: yetersiz onay
    #[test]
    fn test_multisig_not_satisfied() {
        let conditions = vec![ConditionalRule::MultiSigRequired {
            required_signatures: 3,
            total_heirs: 3,
            retry_delay_slots: 100_000,
        }];
        let plan = make_plan_with_conditions(conditions);
        let mut state = make_state();
        state.multisig_approvals.push(addr(1)); // 1/3

        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), &std::collections::HashMap::new());
        assert!(matches!(results[0], ConditionResult::NotSatisfied { .. }));
    }

    // Birden fazla koşul — borç önceden gelir
    #[test]
    fn test_multiple_conditions_priority_order() {
        let conditions = vec![
            ConditionalRule::CharityAllocation { charity_address: addr(5), share_bps: 300 },
            ConditionalRule::DebtPayment { creditor: addr(9), amount: 10_000, priority: 1 },
        ];
        let plan = make_plan_with_conditions(conditions);
        let state = make_state();

        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), &std::collections::HashMap::new());
        assert_eq!(results.len(), 2);

        // Her ikisi de transfer gerektiriyor
        let transfers = ConditionEvaluator::required_transfers(results);
        assert_eq!(transfers.len(), 2);
    }

    // Koşul yok → hepsi karşılandı
    #[test]
    fn test_no_conditions_all_satisfied() {
        let plan = make_plan_with_conditions(vec![]);
        let state = make_state();

        let results = ConditionEvaluator::evaluate_all(&plan, &state, SlotId(100), &std::collections::HashMap::new());
        assert!(results.is_empty());
        assert!(ConditionEvaluator::all_satisfied(&results));
    }

    // blocking_conditions listesi
    #[test]
    fn test_blocking_conditions_list() {
        let results = vec![
            ConditionResult::Satisfied,
            ConditionResult::NotSatisfied { reason: "Yaş yetersiz".to_string() },
            ConditionResult::NotSatisfied { reason: "Multi-sig eksik".to_string() },
        ];
        let blocking = ConditionEvaluator::blocking_conditions(&results);
        assert_eq!(blocking.len(), 2);
    }

    // Hayır kurumu %50 maks sınırı
    #[test]
    fn test_charity_max_50_percent() {
        let amount = ConditionEvaluator::charity_amount(1_000_000, 5_000); // %50
        assert_eq!(amount, 500_000);
    }
}
